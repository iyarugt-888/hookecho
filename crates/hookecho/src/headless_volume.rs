//! 3D volume regressions (ROADMAP_PARITY gap 4): the smooth raymarch, its value floor, the
//! vertical slice/slab and the isosurface, on a synthetic storm whose geometry is known in closed
//! form, so each mode's answer can be checked against where the echo really is rather than
//! against a picture. GPU tests need an adapter (`HOOKECHO_GPU_FALLBACK=1` on CI's lavapipe).
use super::{echo_pixels, init_gpu};
use wxdata::level2::{BinnedSweep, Moment};

const SIZE: u32 = 256;
const BG: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

/// The storm, dBZ at `(x east, y north, h)` km from the radar: a 60 dBZ core 60 km east, falling
/// 1 dBZ per km from it and 4 dBZ per km of height above 8 km.
fn storm_dbz(x: f64, y: f64, h: f64) -> f64 {
    let d = ((x - 60.0).powi(2) + y.powi(2)).sqrt();
    60.0 - d - (h - 8.0).max(0.0) * 4.0
}

/// Eight tilts of a standard VCP sampling [`storm_dbz`] at each gate's centre.
fn storm_volume() -> Vec<BinnedSweep> {
    let (az_bins, gate_count) = (360usize, 600usize);
    let (first_gate_km, gate_interval_km) = (2.125f32, 0.25f32);
    let (vmin, vmax) = Moment::Reflectivity.value_range();
    [0.5f32, 1.5, 2.4, 3.4, 4.3, 6.0, 9.9, 14.6]
        .into_iter()
        .map(|elevation_deg| {
            let mut data = vec![0u8; az_bins * gate_count];
            for az in 0..az_bins {
                let th = (az as f64 + 0.5).to_radians();
                for g in 0..gate_count {
                    let slant = (first_gate_km + g as f32 * gate_interval_km) as f64;
                    let ground =
                        wxdata::xsection::ground_from_slant_km(slant, elevation_deg as f64);
                    let h = wxdata::xsection::beam_height_km(slant, elevation_deg as f64);
                    let dbz = storm_dbz(ground * th.sin(), ground * th.cos(), h);
                    if dbz >= 5.0 {
                        let t = ((dbz as f32 - vmin) / (vmax - vmin)).clamp(0.0, 1.0);
                        data[az * gate_count + g] = 2 + (t * 253.0).round() as u8;
                    }
                }
            }
            BinnedSweep {
                moment: Moment::Reflectivity,
                az_bins,
                gate_count,
                data,
                first_gate_km,
                gate_interval_km,
                radar_lat: 35.0,
                radar_lon: -97.0,
                elevation_deg,
                value_min: vmin,
                value_max: vmax,
                ..Default::default()
            }
        })
        .collect()
}

fn volume() -> wxdata::volume3d::Volume3d {
    wxdata::volume3d::build(&storm_volume(), 96, 36, 100.0, 18.0).expect("storm volume")
}

/// Where the isosurface may sit: low enough that height does not change the value (below 8 km,
/// less a beam's width), every vertex should be about 15 km from the core, give or take a grid
/// cell (~2.1 km) and the gate/azimuth quantization.
#[test]
fn the_45_dbz_isosurface_wraps_the_core_where_the_storm_says() {
    let v3 = volume();
    let mesh = wxdata::isosurface::isosurface(&v3, 45.0, true, 400_000);
    assert!(!mesh.truncated && !mesh.tris.is_empty());
    let low: Vec<&[f32; 3]> = mesh
        .verts
        .iter()
        .filter(|v| v[2] < 5.0 && v[2] > 0.5)
        .collect();
    assert!(low.len() > 50, "{} low vertices", low.len());
    for v in &low {
        let d = ((f64::from(v[0]) - 60.0).powi(2) + f64::from(v[1]).powi(2)).sqrt();
        assert!(
            (d - 15.0).abs() < 3.5,
            "vertex {v:?} is {d:.1} km from the core"
        );
    }
    // Every vertex, low or high, encloses only air the storm makes at least ~40 dBZ.
    for v in &mesh.verts {
        let dbz = storm_dbz(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
        assert!(dbz > 35.0, "vertex {v:?} sits in {dbz:.0} dBZ air");
    }
    // Lower threshold, bigger surface: a 30 dBZ skin encloses the 45 dBZ one.
    let outer = wxdata::isosurface::isosurface(&v3, 30.0, true, 400_000);
    let extent = |m: &wxdata::isosurface::IsoMesh| {
        m.verts.iter().map(|v| v[0]).fold(f32::INFINITY, f32::min)
    };
    assert!(
        extent(&outer) < extent(&mesh),
        "the 30 dBZ skin reaches further west"
    );
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    res: crate::render3d::Volume3dResources,
    target: wgpu::Texture,
}

impl Gpu {
    fn new() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (device, queue, adapter) =
            init_gpu(&rt).expect("required GPU adapter; certification remains open without one");
        eprintln!("3D adapter: {}", adapter.get_info().name);
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let res = crate::render3d::Volume3dResources::new(&device, format);
        let target = super::new_target(&device, format, SIZE);
        Gpu {
            device,
            queue,
            res,
            target,
        }
    }

    /// Raymarch `upload` with `view` from straight above (north up), so screen x is east.
    fn render(
        &mut self,
        upload: &crate::render3d::Volume3dUpload,
        view: crate::render3d::View3d,
        el_deg: f32,
    ) -> Vec<u8> {
        let uniform = crate::render3d::orbit_uniform(
            180.0,
            el_deg,
            3.2,
            1.0,
            upload.n,
            upload.nz,
            upload.half_km,
            upload.top_km,
            256,
            view,
        );
        let tv = self
            .target
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.res
            .render_once(&self.device, &self.queue, &tv, upload, uniform, BG);
        super::read_target(&self.device, &self.queue, &self.target, SIZE)
    }
}

fn upload(v3: &wxdata::volume3d::Volume3d) -> crate::render3d::Volume3dUpload {
    let table = crate::colormap::default_table(Moment::Reflectivity);
    crate::render3d::Volume3dUpload {
        data: crate::render3d::pack_rg8(&v3.data),
        n: v3.n as u32,
        nz: v3.nz as u32,
        lut: crate::colormap::bake_lut(table, (v3.value_min, v3.value_max), None).to_vec(),
        half_km: v3.half_km,
        center_km: [0.0, 0.0],
        top_km: v3.top_km,
        outside: 0.0,
        value_range: None,
        lut_range: None,
    }
}

/// Mean screen x of the echo pixels (0 west edge, 1 east edge).
fn echo_centroid_x(rgba: &[u8]) -> f32 {
    let (mut sum, mut n) = (0.0f64, 0usize);
    for (i, p) in rgba.as_chunks::<4>().0.iter().enumerate() {
        if (p[0] as i16 - 48).abs() + (p[1] as i16 - 48).abs() + (p[2] as i16 - 63).abs() > 30
            && p[..3] != [0, 0, 0]
        {
            sum += (i as u32 % SIZE) as f64;
            n += 1;
        }
    }
    if n == 0 {
        return f32::NAN;
    }
    (sum / n as f64 / SIZE as f64) as f32
}

/// Echo pixels against the black clear colour.
fn lit(rgba: &[u8]) -> usize {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] as u16 + p[1] as u16 + p[2] as u16 > 24)
        .count()
}

#[test]
#[ignore = "gpu: explicitly provision an adapter for real radar visual certification"]
fn gpu_volume_modes_draw_where_the_storm_is() {
    use crate::render3d::{threshold_index, VerticalPlane, View3d};
    let v3 = volume();
    let up = upload(&v3);
    let range = (v3.value_min, v3.value_max);
    let mut gpu = Gpu::new();
    let output =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/parity-review/gap4-3d");
    std::fs::create_dir_all(&output).unwrap();
    let save = |name: &str, rgba: &[u8]| {
        image::save_buffer(
            output.join(format!("{name}.png")),
            rgba,
            SIZE,
            SIZE,
            image::ColorType::Rgba8,
        )
        .unwrap();
    };
    // Smooth MIP from straight above: echo, east of the middle where the core is, and the same
    // bytes on a second draw of the same input.
    let top = gpu.render(&up, View3d::default(), 89.0);
    save("smooth-top", &top);
    let all = lit(&top);
    assert!(all > 2_000, "{all} lit pixels");
    let cx = echo_centroid_x(&top);
    assert!(
        cx > 0.6,
        "the echo centroid {cx} should be east of the middle"
    );
    assert_eq!(
        top,
        gpu.render(&up, View3d::default(), 89.0),
        "replay differs"
    );
    let _ = echo_pixels(&top);

    // The value floor: above the storm's peak nothing draws; at 45 dBZ only the core does.
    let floor = |dbz: f32| View3d {
        threshold_idx: threshold_index(dbz, range),
        ..Default::default()
    };
    assert_eq!(
        lit(&gpu.render(&up, floor(70.0), 89.0)),
        0,
        "nothing reaches 70 dBZ"
    );
    let core = gpu.render(&up, floor(45.0), 89.0);
    save("floor-45", &core);
    let core_px = lit(&core);
    assert!(core_px > 50 && core_px < all, "{core_px} of {all}");
    // The ≥45 dBZ disc is ~15 km in radius on a 200 km box: about (15/100)² π of the box's
    // footprint, which is itself a fraction of the frame; compare against the full echo instead.
    let expected = (15.0f32 / 55.0).powi(2); // core area over the ≥5 dBZ disc's
    let ratio = core_px as f32 / all as f32;
    assert!(
        ratio > expected * 0.4 && ratio < expected * 2.5,
        "core/echo area {ratio:.3}, expected about {expected:.3}"
    );

    // Slicing: a half-space keeping the east keeps the storm; keeping the west keeps nothing.
    let plane = |bearing_deg: f32, offset: f32, thickness: Option<f32>| View3d {
        plane: Some(VerticalPlane {
            bearing_deg,
            offset,
            thickness,
        }),
        ..Default::default()
    };
    let east = gpu.render(&up, plane(90.0, 0.0, None), 89.0);
    let west = gpu.render(&up, plane(270.0, 0.0, None), 89.0);
    save("plane-east", &east);
    assert!(lit(&east) > all / 2, "{} of {all}", lit(&east));
    assert_eq!(lit(&west), 0, "the storm is all east of the radar");
    // A thin north-south slab through the core shows a band of it; one 60 km west, nothing.
    let through = gpu.render(&up, plane(90.0, 0.6, Some(0.03)), 89.0);
    let away = gpu.render(&up, plane(90.0, -0.6, Some(0.03)), 89.0);
    save("slab-core", &through);
    assert!(
        lit(&through) > 50 && lit(&through) < all / 3,
        "{}",
        lit(&through)
    );
    assert_eq!(lit(&away), 0);
    // From the side the slab is a vertical section: lit, and still nothing on the far side.
    let side = gpu.render(&up, plane(90.0, 0.6, Some(0.03)), 10.0);
    save("slab-side", &side);
    assert!(lit(&side) > 50);
    eprintln!(
        "smooth {all}, floor-45 {core_px}, east {}, slab {}, side {}",
        lit(&east),
        lit(&through),
        lit(&side)
    );
}

/// A volume of the same shape is written into the textures already on the GPU (a 3D loop does
/// this every frame); what draws must be the new volume, exactly as a fresh texture draws it.
#[test]
#[ignore = "gpu: explicitly provision an adapter for real radar visual certification"]
fn gpu_a_reused_volume_texture_draws_only_the_new_volume() {
    use crate::render3d::View3d;
    let v3 = volume();
    let storm = upload(&v3);
    let empty = crate::render3d::Volume3dUpload {
        data: vec![0; storm.data.len()],
        ..upload(&v3)
    };
    let mut gpu = Gpu::new();
    let first = gpu.render(&storm, View3d::default(), 89.0);
    assert!(lit(&first) > 2_000);
    // Same shape, nothing in it: the storm must be gone, not left behind in the old texture.
    assert_eq!(lit(&gpu.render(&empty, View3d::default(), 89.0)), 0);
    // And back: the reused texture draws the storm as it did the first time.
    assert_eq!(gpu.render(&storm, View3d::default(), 89.0), first);
    let fresh = Gpu::new().render(&storm, View3d::default(), 89.0);
    assert_eq!(first, fresh, "a fresh texture draws the same");
}

/// The app path: the volume raymarched into an offscreen image and copied over the pane. At full
/// scale it draws what a direct march does; a view that has not changed is not marched again,
/// and a new volume or camera is.
#[test]
#[ignore = "gpu: explicitly provision an adapter for real radar visual certification"]
fn gpu_the_offscreen_march_matches_a_direct_one_and_only_reruns_on_change() {
    use crate::render3d::View3d;
    let v3 = volume();
    let storm = upload(&v3);
    let mut gpu = Gpu::new();
    let direct = gpu.render(&storm, View3d::default(), 89.0);
    let uniform = |el: f32| {
        crate::render3d::orbit_uniform(
            180.0,
            el,
            3.2,
            1.0,
            storm.n,
            storm.nz,
            storm.half_km,
            storm.top_km,
            256,
            View3d::default(),
        )
    };
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut res = crate::render3d::Volume3dResources::new(&gpu.device, format);
    res.upload(&gpu.device, &gpu.queue, &storm);
    let frame = |res: &mut crate::render3d::Volume3dResources, el: f32, size: u32| {
        let u = uniform(el);
        res.write_uniform(&gpu.queue, &u);
        let mut enc = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        res.march(&gpu.device, &mut enc, &u, [size, size]);
        let tv = gpu
            .target
            .create_view(&wgpu::TextureViewDescriptor::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &tv,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(BG),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            res.record_offscreen(&mut pass);
        }
        gpu.queue.submit(Some(enc.finish()));
        super::read_target(&gpu.device, &gpu.queue, &gpu.target, SIZE)
    };
    let full = frame(&mut res, 89.0, SIZE);
    let worst = full
        .iter()
        .zip(&direct)
        .map(|(a, b)| (*a as i16 - *b as i16).unsigned_abs())
        .max()
        .unwrap();
    // One sRGB round trip of premultiplied colour: a step or two per channel at most.
    assert!(
        worst <= 3,
        "offscreen differs from the direct march by {worst}"
    );
    assert_eq!(res.marches, 1);
    // The same view again: copied, not marched.
    assert_eq!(frame(&mut res, 89.0, SIZE), full);
    assert_eq!(res.marches, 1, "an unchanged view was raymarched again");
    // The camera moves: marched again.
    frame(&mut res, 60.0, SIZE);
    assert_eq!(res.marches, 2);
    // A new volume of the same shape: marched again.
    res.upload(&gpu.device, &gpu.queue, &storm);
    frame(&mut res, 60.0, SIZE);
    assert_eq!(res.marches, 3);
    // Half resolution (the phone scale): the storm is still where it was.
    let half = frame(&mut res, 89.0, SIZE / 2);
    let (a, b) = (echo_centroid_x(&half), echo_centroid_x(&direct));
    assert!((a - b).abs() < 0.02, "half-scale centroid {a} vs {b}");
    assert!((lit(&half) as f32 / lit(&direct) as f32 - 1.0).abs() < 0.1);
}
