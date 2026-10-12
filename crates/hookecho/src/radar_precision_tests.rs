//! Does the radar shader pick the same gate for a pixel on every GPU? (1010.md V19, m6.3)
//!
//! `radar.wgsl` turns each fragment's world position into a range and an azimuth in single
//! precision (`radar_polar.wgsl`), then reads the gate under it. This runs that function as a
//! compute shader over a 512 by 512 view of Moore (zoom 10, KTLX's super-resolution grid), beside
//! the haversine the shader used before, and compares the gate and radial each picks with a
//! double-precision reference. A gate picked differently draws a different value, whatever the
//! smoothing.

/// The shader's function and, for comparison, the haversine it replaced, written as it was.
const SHADER: &str = concat!(
    include_str!("shaders/radar_polar.wgsl"),
    r#"
const PI: f32 = 3.14159265358979;
const R_EARTH_KM: f32 = 6371.0;
struct P {
    radar_lat: f32, radar_lon: f32, first_gate_km: f32, gate_interval_km: f32,
    az_bins: f32, n: u32, radar_wx: f32, radar_wy: f32,
    sin_lat0: f32, cos_lat0: f32, _a: f32, _b: f32,
};
@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var<storage, read> world: array<vec2<f32>>;
// Per pixel: the new function's (gate, radial), then the old haversine's.
@group(0) @binding(2) var<storage, read_write> out: array<vec4<f32>>;

fn world_to_lonlat(w: vec2<f32>) -> vec2<f32> {
    let lon = w.x * 360.0 - 180.0;
    let n = PI * (1.0 - 2.0 * w.y);
    let lat = atan(sinh(n)) * 180.0 / PI;
    return vec2<f32>(lon, lat);
}

fn old_range_az(w: vec2<f32>) -> vec2<f32> {
    let ll = world_to_lonlat(w);
    let lat = ll.y * PI / 180.0;
    let lon = ll.x * PI / 180.0;
    let lat0 = p.radar_lat * PI / 180.0;
    let lon0 = p.radar_lon * PI / 180.0;
    let dlon = lon - lon0;
    let dlat = lat - lat0;
    let a = sin(dlat * 0.5) * sin(dlat * 0.5)
          + cos(lat0) * cos(lat) * sin(dlon * 0.5) * sin(dlon * 0.5);
    let range_km = 2.0 * R_EARTH_KM * asin(sqrt(clamp(a, 0.0, 1.0)));
    let y = sin(dlon) * cos(lat);
    let x = cos(lat0) * sin(lat) - sin(lat0) * cos(lat) * cos(dlon);
    var az = atan2(y, x) * 180.0 / PI;
    az = az - floor(az / 360.0) * 360.0;
    return vec2<f32>(range_km, az);
}

fn cell(polar: vec2<f32>) -> vec2<f32> {
    return vec2<f32>((polar.x - p.first_gate_km) / p.gate_interval_km, polar.y / 360.0 * p.az_bins);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= p.n) { return; }
    let w = world[i];
    let new_polar = radar_range_az(w, vec2<f32>(p.radar_wx, p.radar_wy), p.sin_lat0, p.cos_lat0);
    out[i] = vec4<f32>(cell(new_polar), cell(old_range_az(w)));
}
"#
);

/// The shader's gate and azimuth bin for `world`, in double precision.
fn reference(world: [f64; 2], radar: [f64; 2], first: f64, interval: f64, bins: f64) -> (f64, f64) {
    use std::f64::consts::PI;
    let lon = (world[0] * 360.0 - 180.0).to_radians();
    let lat = (PI * (1.0 - 2.0 * world[1])).sinh().atan();
    let (lat0, lon0) = (radar[1].to_radians(), radar[0].to_radians());
    let (dlon, dlat) = (lon - lon0, lat - lat0);
    let a = (dlat * 0.5).sin().powi(2) + lat0.cos() * lat.cos() * (dlon * 0.5).sin().powi(2);
    let range = 2.0 * 6371.0 * a.clamp(0.0, 1.0).sqrt().asin();
    let y = dlon.sin() * lat.cos();
    let x = lat0.cos() * lat.sin() - lat0.sin() * lat.cos() * dlon.cos();
    let az = y.atan2(x).to_degrees().rem_euclid(360.0);
    ((range - first) / interval, az / 360.0 * bins)
}

/// How one formula's cells compare with the reference, over the pixels beyond the first gate.
#[derive(Default)]
struct Tally {
    pixels: usize,
    gate: usize,
    radial: usize,
    either: usize,
    worst_gate: f64,
    worst_radial: f64,
}

impl Tally {
    fn add(&mut self, got: (f32, f32), want: (f64, f64)) {
        self.pixels += 1;
        let (dg, da) = (f64::from(got.0) - want.0, f64::from(got.1) - want.1);
        // A radial either side of north is one apart, not 720.
        let da = (da + 360.0).rem_euclid(720.0) - 360.0;
        self.worst_gate = self.worst_gate.max(dg.abs());
        self.worst_radial = self.worst_radial.max(da.abs());
        let g = got.0.floor() as i64 != want.0.floor() as i64;
        let a = (got.1.floor() as i64).rem_euclid(720) != (want.1.floor() as i64).rem_euclid(720);
        self.gate += usize::from(g);
        self.radial += usize::from(a);
        self.either += usize::from(g || a);
    }

    fn line(&self, name: &str) -> String {
        let pct = |k: usize| 100.0 * k as f64 / self.pixels.max(1) as f64;
        format!(
            "{name}: gate differs from double precision at {} of {} pixels ({:.2}%), radial at {} \
             ({:.2}%), either at {} ({:.2}%); largest error {:.4} gates, {:.4} radials",
            self.gate,
            self.pixels,
            pct(self.gate),
            self.radial,
            pct(self.radial),
            self.either,
            pct(self.either),
            self.worst_gate,
            self.worst_radial
        )
    }
}

/// Run with `cargo test -p hookecho --lib -- --ignored gpu_radar_gate --nocapture`, and again
/// with `HOOKECHO_GPU_FALLBACK=1` for the software adapter.
#[test]
#[ignore = "gpu"]
fn gpu_radar_gate_choice_against_double_precision() {
    use wgpu::util::DeviceExt;
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

    // KTLX's super-resolution reflectivity grid, as `--watch` drew it (1192 gates by 720 radials).
    let radar = [-97.277_f64, 35.333_f64];
    let (first, interval, bins) = (2.125_f64, 0.25_f64, 720.0_f64);
    // A 512 by 512 view at zoom 10 centred on Moore: pixel centres in world units.
    let side = 512usize;
    let world_per_px = 1.0 / (256.0 * 1024.0);
    let c = crate::render::mercator::lonlat_to_world(-97.50, 35.33);
    let world32: Vec<[f32; 2]> = (0..side * side)
        .map(|i| {
            let (px, py) = ((i % side) as f64 + 0.5, (i / side) as f64 + 0.5);
            [
                (c.0 + (px - side as f64 / 2.0) * world_per_px) as f32,
                (c.1 + (py - side as f64 / 2.0) * world_per_px) as f32,
            ]
        })
        .collect();
    let n = world32.len() as u32;
    // The uniforms as the renderer fills them (`render::radar_uniform`).
    let (lat, lon) = (radar[1] as f32, radar[0] as f32);
    let rw = crate::render::mercator::lonlat_to_world(f64::from(lon), f64::from(lat));
    let lat0 = f64::from(lat).to_radians();
    let params: [f32; 12] = [
        lat,
        lon,
        first as f32,
        interval as f32,
        bins as f32,
        f32::from_bits(n),
        rw.0 as f32,
        rw.1 as f32,
        lat0.sin() as f32,
        lat0.cos() as f32,
        0.0,
        0.0,
    ];
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let uni = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&params),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&world32),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let size = u64::from(n) * 16;
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uni.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: input.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(n.div_ceil(64), 1, 1);
    }
    enc.copy_buffer_to_buffer(&output, 0, &readback, 0, size);
    queue.submit(Some(enc.finish()));
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let got: Vec<[f32; 4]> = bytemuck::cast_slice(&slice.get_mapped_range()).to_vec();

    // Against the world positions as the GPU holds them (rounded to f32), in double precision.
    let (mut new, mut old) = (Tally::default(), Tally::default());
    let mut cells = Vec::with_capacity(got.len() * 8);
    for (w, g) in world32.iter().zip(&got) {
        let want = reference(
            [f64::from(w[0]), f64::from(w[1])],
            radar,
            first,
            interval,
            bins,
        );
        // Inside the first gate nothing is drawn, and the azimuth of a point beside the radar
        // is ill-conditioned anyway.
        if want.0 < 0.0 {
            continue;
        }
        new.add((g[0], g[1]), want);
        old.add((g[2], g[3]), want);
        cells.extend((g[0].floor() as i32).to_le_bytes());
        cells.extend((g[1].floor() as i32).to_le_bytes());
    }
    // Each pixel's cell by the new function, kept so two adapters' runs can be compared.
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/parity-review/v19");
    std::fs::create_dir_all(&dir).unwrap();
    let tag = if std::env::var("HOOKECHO_GPU_FALLBACK").is_ok() {
        "software"
    } else {
        "gpu"
    };
    std::fs::write(dir.join(format!("radar-cells-{tag}.bin")), cells).unwrap();
    let who = format!(
        "{} ({:?})",
        adapter.get_info().name,
        adapter.get_info().backend
    );
    println!("{}", new.line(&format!("{who}, radar_polar.wgsl")));
    println!("{}", old.line(&format!("{who}, the old haversine")));
    // About 30 km from the radar a gate is 0.25 km and a radial about 0.26 km: the new function
    // must stay within a fiftieth of either everywhere in the view, on any adapter.
    assert!(
        new.worst_gate < 0.02 && new.worst_radial < 0.02,
        "{}",
        new.line(&who)
    );
}
