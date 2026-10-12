//! A user-defined product evaluated on the GPU (ROADMAP_NEW C1, 1010.md V17): the formula's
//! compute shader ([`wxdata::udp::Expr::to_wgsl`]) run over a batch of gates, read back as the
//! same `Option<f32>` per gate the CPU evaluator gives. Gates are sent in chunks that fit the
//! device's storage-buffer binding limit, so a whole volume goes through.

use wgpu::util::DeviceExt;
use wxdata::udp::{GateInputs, GpuProgram};

/// A formula's compiled pipeline on one device.
pub struct UdpGpu {
    program: GpuProgram,
    pipeline: wgpu::ComputePipeline,
}

impl UdpGpu {
    /// Compile `program` for `device`.
    pub fn new(device: &wgpu::Device, program: GpuProgram) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("udp_gpu"),
            source: wgpu::ShaderSource::Wgsl(program.wgsl.as_str().into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("udp_gpu"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        UdpGpu { program, pipeline }
    }

    /// The formula at every gate of `gates`, in order: `None` where it has no value.
    pub fn evaluate(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        gates: &[GateInputs],
    ) -> Vec<Option<f32>> {
        let stride = self.program.stride();
        let limit = device.limits().max_storage_buffer_binding_size;
        let chunk = ((limit / (stride as u64 * 4)) as usize).clamp(1, 1 << 22);
        let mut out = Vec::with_capacity(gates.len());
        for part in gates.chunks(chunk) {
            out.extend(self.evaluate_chunk(device, queue, part));
        }
        out
    }

    fn evaluate_chunk(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        gates: &[GateInputs],
    ) -> Vec<Option<f32>> {
        let n = gates.len();
        if n == 0 {
            return Vec::new();
        }
        let (values, present) = self.program.pack(gates);
        let storage = |label: &str, bytes: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytes,
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("udp_params"),
            contents: bytemuck::cast_slice(&[n as u32, self.program.stride() as u32, 0, 0]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let values = storage("udp_values", bytemuck::cast_slice(&values));
        let present = storage("udp_present", bytemuck::cast_slice(&present));
        let size = (n * 4) as u64;
        let output = |label: &str| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let (out_value, out_present) = (output("udp_out_value"), output("udp_out_present"));
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("udp_readback"),
            size: size * 2,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("udp_gpu"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: values.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: present.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: out_value.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: out_present.as_entire_binding(),
                },
            ],
        });
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("udp_gpu"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            let (x, y) = GpuProgram::dispatch(n);
            pass.dispatch_workgroups(x, y, 1);
        }
        encoder.copy_buffer_to_buffer(&out_value, 0, &readback, 0, size);
        encoder.copy_buffer_to_buffer(&out_present, 0, &readback, size, size);
        queue.submit(Some(encoder.finish()));
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        if !matches!(rx.recv(), Ok(Ok(()))) {
            return vec![None; n];
        }
        let mapped = slice.get_mapped_range();
        let words: &[u32] = bytemuck::cast_slice(&mapped);
        let (v, ok) = words.split_at(n);
        v.iter()
            .zip(ok)
            .map(|(v, ok)| (*ok != 0).then(|| f32::from_bits(*v)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How far apart the GPU and the CPU may be, as a fraction of the value (of 1 for a value
    /// under 1). A product is drawn in 254 colour steps, so this is far below what shows.
    const TOLERANCE: f32 = 1e-5;

    fn device() -> (wgpu::Adapter, wgpu::Device, wgpu::Queue) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let instance = wgpu::Instance::default();
        let adapter = rt
            .block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: std::env::var("HOOKECHO_GPU_FALLBACK").is_ok(),
                compatible_surface: None,
            }))
            .expect("no wgpu adapter");
        let (device, queue) = rt
            .block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .unwrap();
        (adapter, device, queue)
    }

    /// Gates with every input, some missing, and values that hit each formula's edges: zero
    /// divisors, equal comparisons, clamps with their bounds swapped. A fixed generator, so a
    /// failure reproduces.
    fn gates(n: usize) -> Vec<GateInputs> {
        let mut s = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let mut pick = move |lo: f32, hi: f32, step: f32| -> Option<f32> {
            let r = next();
            // About one value in eight is missing.
            if r % 8 == 0 {
                return None;
            }
            let k = ((hi - lo) / step) as u64 + 1;
            Some(lo + ((r >> 8) % k) as f32 * step)
        };
        (0..n)
            .map(|_| GateInputs {
                reflectivity: pick(-10.0, 75.0, 0.5),
                velocity: pick(-60.0, 60.0, 0.5),
                spectrum_width: pick(0.0, 15.0, 0.5),
                differential_reflectivity: pick(-4.0, 6.0, 0.25),
                specific_diff_phase: pick(-2.0, 8.0, 0.25),
                correlation_coefficient: pick(0.2, 1.05, 0.01),
                range_km: pick(0.0, 230.0, 0.25),
                azimuth_deg: pick(0.0, 359.5, 0.5),
                elevation_deg: pick(0.5, 19.5, 0.1),
                beam_height_m: pick(0.0, 15000.0, 10.0),
                beam_altitude_m: pick(300.0, 15300.0, 10.0),
                freezing_level_m: pick(2500.0, 4500.0, 50.0),
                ..Default::default()
            })
            .collect()
    }

    /// A product over every tilt of a real volume (KTLX 2013-05-20 20:12 UTC, Moore, from the
    /// feature-track cache) is the same on the GPU as on the CPU, gate for gate; and how long
    /// each takes, gathering the inputs included.
    ///
    /// Run with `cargo test -p hookecho --release -- --ignored gpu_udp_matches_the_cpu_on --nocapture`.
    #[test]
    #[ignore = "gpu, network: the KTLX archive"]
    fn gpu_udp_matches_the_cpu_on_the_moore_volume() {
        use wxdata::level2::{self, Moment};
        use wxdata::udp_volume::{self, MOMENTS};
        let (adapter, device, queue) = device();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let cache =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/feature-track");
        std::fs::create_dir_all(&cache).unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2013, 5, 20).unwrap();
        let id = rt
            .block_on(level2::list_volumes("KTLX", date))
            .unwrap()
            .into_iter()
            .find(|id| id.name().contains("201229"))
            .expect("the 20:12:29 volume");
        let scan = rt.block_on(level2::download_scan(id, Some(cache))).unwrap();
        let tilts = level2::elevation_angles(&scan).len();
        let per_moment: [Vec<Option<wxdata::level2::BinnedSweep>>; 6] = std::array::from_fn(|i| {
            (0..tilts)
                .map(|t| {
                    level2::bin_scan_opts_recorded(
                        &scan,
                        MOMENTS[i],
                        t,
                        MOMENTS[i] == Moment::Velocity,
                    )
                    .ok()
                })
                .collect()
        });
        let paired = udp_volume::pair_tilts(&per_moment);
        let env = udp_volume::Env {
            antenna_altitude_m: Some(384.0),
            freezing_level_m: Some(4100.0),
            ..Default::default()
        };
        for src in [
            "REF > 55 && ZDR < 1 ? REF : 0",
            "REF + if_missing(KDP, 0)",
            "CC < 0.8 && REF > 35 ? 1 : 0",
            "BEAM_ALTITUDE_M < FREEZING_LEVEL_M ? REF : REF - 10",
        ] {
            let expr = wxdata::udp::parse(src).unwrap();
            let gpu = UdpGpu::new(&device, expr.to_wgsl().unwrap());
            let t0 = std::time::Instant::now();
            let cpu: Vec<_> = paired
                .iter()
                .filter_map(|t| udp_volume::evaluate_tilt(&expr, t, env))
                .collect();
            let cpu_s = t0.elapsed().as_secs_f64();
            let t0 = std::time::Instant::now();
            let on_gpu: Vec<_> = paired
                .iter()
                .filter_map(|t| {
                    udp_volume::evaluate_tilt_with(&expr, t, env, |g| {
                        gpu.evaluate(&device, &queue, g)
                    })
                })
                .collect();
            let gpu_s = t0.elapsed().as_secs_f64();
            assert_eq!(cpu.len(), on_gpu.len());
            let (mut values, mut gates) = (0usize, 0usize);
            for ((_, c), (_, g)) in cpu.iter().zip(&on_gpu) {
                assert_eq!(c.len(), g.len());
                gates += c.len();
                for (a, b) in c.iter().zip(g) {
                    match (a, b) {
                        (Some(a), Some(b)) => {
                            assert!((a - b).abs() <= TOLERANCE * a.abs().max(1.0));
                            values += 1;
                        }
                        (None, None) => {}
                        _ => panic!("{src}: cpu {a:?}, gpu {b:?}"),
                    }
                }
            }
            eprintln!(
                "{src}: {} tilts, {gates} gates, {values} values agree; CPU {cpu_s:.2} s, GPU {gpu_s:.2} s ({})",
                cpu.len(),
                adapter.get_info().name
            );
        }
    }

    /// The same formulas on the GPU and the CPU agree at every gate: the same gates have a value,
    /// and the values agree to 1 part in 100,000 ([`TOLERANCE`]). Not to the last bit: WGSL
    /// division need only be within 2.5 units in the last place, and a later subtraction can make
    /// that larger against a small result (`46 - 2 / 0.04` came out 3.6e-6 apart, 7 units in the
    /// last place of -4).
    ///
    /// Run with `cargo test -p hookecho -- --ignored gpu`.
    #[test]
    #[ignore = "gpu"]
    fn gpu_udp_matches_the_cpu_at_every_gate() {
        let (adapter, device, queue) = device();
        let gates = gates(200_003);
        let formulas = [
            "REF > 55 && ZDR < 1 ? REF : 0",
            "REF + if_missing(KDP, 0)",
            "is_missing(ZDR) ? -1 : ZDR * 2 - 1",
            "VEL / (RANGE_KM - 100)",
            "clamp(REF, ZDR * 10, 50)",
            "mean(REF, ZDR, KDP, CC * 100)",
            "abs(VEL) >= 30 || SW > 8",
            "min(REF, 50) - max(ZDR, 0) / (CC - 1)",
            "!(REF == 40) && BEAM_ALTITUDE_M < FREEZING_LEVEL_M",
            // Overflows f32 where REF is about 69 dBZ or more: infinite, so missing. Made of inputs, not
            // constants, which a GPU compiler may fold together first (see `udp::wgsl`).
            "-(-REF) * REF * REF * REF * REF * REF * REF * REF * REF * REF * REF * REF * REF * REF \
             * REF * REF * REF * REF * REF * REF * REF",
        ];
        for src in formulas {
            let expr = wxdata::udp::parse(src).unwrap();
            let gpu =
                UdpGpu::new(&device, expr.to_wgsl().expect(src)).evaluate(&device, &queue, &gates);
            let (mut both, mut neither, mut worst) = (0usize, 0usize, 0.0f32);
            for (i, g) in gates.iter().enumerate() {
                let cpu = wxdata::udp::evaluate(&expr, g);
                match (cpu, gpu[i]) {
                    (Some(c), Some(v)) => {
                        let tol = TOLERANCE * c.abs().max(1.0);
                        worst = worst.max((c - v).abs() / c.abs().max(1.0));
                        assert!(
                            (c - v).abs() <= tol,
                            "{src} at gate {i}: cpu {c}, gpu {v} ({g:?}), adapter {}",
                            adapter.get_info().name
                        );
                        both += 1;
                    }
                    (None, None) => neither += 1,
                    (c, v) => panic!(
                        "{src} at gate {i}: cpu {c:?}, gpu {v:?} ({g:?}), adapter {}",
                        adapter.get_info().name
                    ),
                }
            }
            eprintln!(
                "{src}: {both} values, {neither} missing, agree on {} ({:?}); largest \
                 difference {worst:.1e} of the value",
                adapter.get_info().name,
                adapter.get_info().backend
            );
        }
    }
}
