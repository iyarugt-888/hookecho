//! Explicit GPU evidence for the field crossfade (`crate::field_fade`) through production
//! prepare/paint: the previous frame is drawn under the new one, which fades in by its share.
use super::*;
use crate::render::{FieldLayer, FieldTexture, MrmsTextureKey};

const SIZE: u32 = 160;

fn callback(key: MrmsTextureKey, fade: Option<(MrmsTextureKey, f32)>) -> MapCallback {
    let mut cb = mrms_panes_callback(key);
    cb.field_fades = fade
        .map(|(from, share)| (FieldLayer::Mesh, FieldTexture::Mrms(from), share))
        .into_iter()
        .collect();
    cb
}

fn mrms_panes_callback(key: MrmsTextureKey) -> MapCallback {
    mrms_panes_callback_at(key, SIZE, 7.0)
}

/// [`mrms_panes_callback`] for a `size` px square pane at `zoom`.
fn mrms_panes_callback_at(key: MrmsTextureKey, size: u32, zoom: f64) -> MapCallback {
    let camera = Camera::at_lonlat(-97.0, 35.0, zoom);
    let viewport = (size as f32, size as f32);
    let (center, scale) = camera.world_to_clip_uniform(viewport);
    MapCallback {
        pane: 0,
        camera_center: center,
        camera_scale: scale,
        world_per_pixel: camera.world_per_pixel() as f32,
        camera_view_proj: camera.view_projection_uniform(viewport),
        camera_3d: 0.0,
        camera_globe: [0.0; 4],
        basemap_key: 0,
        basemap_context: Default::default(),
        vector_over_raster: false,
        new_tiles: Vec::new(),
        visible: Vec::new(),
        radar_upload: None,
        draw_radar: false,
        observed_upload: None,
        draw_observed: false,
        overlay_upload: None,
        draw_overlay: false,
        field_uploads: Vec::new(),
        mrms_uploads: Vec::new(),
        mrms_fields: vec![(FieldLayer::Mesh, key)],
        drop_mrms_fields: Vec::new(),
        model_uploads: Vec::new(),
        model_fields: Vec::new(),
        drop_model_fields: Vec::new(),
        field_draws: vec![(FieldLayer::Mesh, 1.0)],
        field_swipe: None,
        field_fades: Vec::new(),
        clear_tiles: false,
        drop_tiles: Vec::new(),
        drop_fields: Vec::new(),
        new_vector_tiles: Vec::new(),
        visible_vector: Vec::new(),
        clear_vector: false,
        drop_vector_tiles: Vec::new(),
        wind_upload: None,
        wind: None,
    }
}

fn upload(color: [u8; 4]) -> crate::render::MrmsUpload {
    let grid = wxdata::mrms::MrmsField {
        values: vec![280.0; 4],
        nx: 2,
        ny: 2,
        lon_west: -100.0,
        lon_east: -94.0,
        lat_north: 40.0,
        lat_south: 30.0,
        time: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
    };
    let mut lut = vec![0; 1024];
    lut[8..12].copy_from_slice(&color);
    crate::app::field_index_upload(&grid, |_| 2, lut)
}

#[test]
#[ignore = "gpu: writes field crossfade captures for review"]
fn gpu_a_new_field_frame_fades_in_over_the_old_one() {
    use sha2::{Digest, Sha256};
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (device, queue, adapter) = init_gpu(&rt).expect("GPU required for the crossfade control");
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut resources = RenderResources::new(&device, format);
    let target = new_target(&device, format, SIZE);
    let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/parity-review/m5.4/crossfade");
    std::fs::create_dir_all(&destination).unwrap();
    let mut evidence = Vec::new();
    let mut capture = |name: &str, resources: &RenderResources| -> [u8; 4] {
        let view = target.create_view(&Default::default());
        resources.draw_pane(&device, &queue, &view, 0, wgpu::Color::BLACK);
        let pixels = read_target(&device, &queue, &target, SIZE);
        let center: [u8; 4] = pixels[((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize..][..4]
            .try_into()
            .unwrap();
        image::RgbaImage::from_raw(SIZE, SIZE, pixels.clone())
            .unwrap()
            .save(destination.join(format!("{name}.png")))
            .unwrap();
        evidence.push(serde_json::json!({
            "name": name,
            "center_rgba": center,
            "sha256": format!("{:x}", Sha256::digest(&pixels)),
        }));
        center
    };
    let (old, new) = (MrmsTextureKey(1), MrmsTextureKey(2));
    let mut first = callback(old, None);
    first.mrms_uploads = vec![
        (old, upload([255, 0, 0, 255])),
        (new, upload([0, 0, 255, 255])),
    ];
    resources.prepare_pane(&device, &queue, &first);
    assert_eq!(capture("old-frame", &resources), [255, 0, 0, 255]);

    // The frame changes: the old frame stays under the new one as it fades in.
    let mut shares = Vec::new();
    for share in [0.0f32, 0.25, 0.5, 0.75] {
        resources.prepare_pane(&device, &queue, &callback(new, Some((old, share))));
        let c = capture(&format!("fade-{:03}", (share * 100.0) as u32), &resources);
        shares.push((share, c));
    }
    assert_eq!(
        shares[0].1,
        [255, 0, 0, 255],
        "at the start only the old frame shows"
    );
    for pair in shares.windows(2) {
        let ((_, a), (_, b)) = (pair[0], pair[1]);
        assert!(
            b[0] < a[0] && b[2] > a[2],
            "red gives way to blue: {a:?} -> {b:?}"
        );
        assert_eq!(b[3], 255, "never see-through mid-fade");
    }
    let half = shares[2].1;
    assert!(
        half[0].abs_diff(half[2]) <= 2,
        "halfway is an even mix: {half:?}"
    );

    // The fade ends: the new frame alone.
    resources.prepare_pane(&device, &queue, &callback(new, None));
    assert_eq!(capture("new-frame", &resources), [0, 0, 255, 255]);

    // A previous texture that is no longer resident is skipped: the new frame draws in full.
    let gone = MrmsTextureKey(9);
    resources.prepare_pane(&device, &queue, &callback(new, Some((gone, 0.25))));
    assert_eq!(capture("previous-evicted", &resources), [0, 0, 255, 255]);

    // And the other way, blue back to red, halfway.
    resources.prepare_pane(&device, &queue, &callback(old, Some((new, 0.5))));
    let back = capture("fade-back-050", &resources);
    assert!(back[0].abs_diff(back[2]) <= 2, "{back:?}");

    // A per-layer texture (a GOES band): its next upload replaces it, and the replaced texture
    // is what stays under the new one while it fades in.
    let goes = |upload: Option<crate::render::MrmsUpload>, fade: Option<(u64, f32)>| {
        let mut cb = mrms_panes_callback(old);
        cb.mrms_fields.clear();
        cb.field_uploads = upload
            .map(|u| (FieldLayer::GoesIr, u))
            .into_iter()
            .collect();
        cb.field_draws = vec![(FieldLayer::GoesIr, 1.0)];
        cb.field_fades = fade
            .map(|(count, share)| {
                (
                    FieldLayer::GoesIr,
                    FieldTexture::Layer(FieldLayer::GoesIr, count),
                    share,
                )
            })
            .into_iter()
            .collect();
        cb
    };
    // The first upload has replaced nothing, so even a fade asked for draws it in full.
    let first_goes = goes(Some(upload([255, 0, 0, 255])), Some((0, 0.5)));
    resources.prepare_pane(&device, &queue, &first_goes);
    assert_eq!(capture("goes-first-upload", &resources), [255, 0, 0, 255]);
    let next_goes = goes(Some(upload([0, 0, 255, 255])), Some((1, 0.5)));
    resources.prepare_pane(&device, &queue, &next_goes);
    let mid = capture("goes-fade-050", &resources);
    assert!(mid[0].abs_diff(mid[2]) <= 2 && mid[0] > 150, "{mid:?}");
    resources.prepare_pane(&device, &queue, &goes(None, None));
    assert_eq!(capture("goes-new-frame", &resources), [0, 0, 255, 255]);

    std::fs::write(
        destination.join("evidence.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "adapter": adapter.get_info().name,
            "size": SIZE,
            "captures": evidence,
        }))
        .unwrap(),
    )
    .unwrap();
}

/// A 2000 by 1000 cell field over the central US, `phase` moving its pattern so two frames
/// differ everywhere, through a 254-step colour ramp.
fn big_upload(phase: f32) -> crate::render::MrmsUpload {
    let (nx, ny) = (2000usize, 1000usize);
    let values: Vec<f32> = (0..nx * ny)
        .map(|i| {
            let (x, y) = ((i % nx) as f32, (i / nx) as f32);
            ((x * 0.013 + phase).sin() * (y * 0.021 - phase).cos() + 1.0) * 126.0
        })
        .collect();
    let grid = wxdata::mrms::MrmsField {
        values,
        nx,
        ny,
        lon_west: -105.0,
        lon_east: -89.0,
        lat_north: 41.0,
        lat_south: 29.0,
        time: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
    };
    let mut lut = vec![0u8; 1024];
    for i in 2..256 {
        let t = (i - 2) as f32 / 253.0;
        lut[i * 4..i * 4 + 4].copy_from_slice(&[
            (255.0 * t) as u8,
            (255.0 * (1.0 - (2.0 * t - 1.0).abs())) as u8,
            (255.0 * (1.0 - t)) as u8,
            255,
        ]);
    }
    crate::app::field_index_upload(&grid, |v| (v as u8).clamp(2, 255), lut)
}

/// What the crossfade's second draw costs (1010.md V19): a 1080 px pane of a 2000 by 1000
/// field, 240 frames drawn with one frame and 240 mid-fade, each timed from submit until the GPU
/// has finished. Also keeps a mid-fade capture of the two frames.
///
/// `cargo test -p hookecho --release --lib -- --ignored gpu_crossfade_frame_cost --nocapture`
#[test]
#[ignore = "gpu: times the crossfade"]
fn gpu_crossfade_frame_cost() {
    const PANE: u32 = 1080;
    const FRAMES: usize = 240;
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (device, queue, adapter) = init_gpu(&rt).expect("GPU required for the crossfade timing");
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut resources = RenderResources::new(&device, format);
    let target = new_target(&device, format, PANE);
    let view = target.create_view(&Default::default());
    let (old, new) = (MrmsTextureKey(1), MrmsTextureKey(2));
    let at = |key, fade: Option<(MrmsTextureKey, f32)>| {
        let mut cb = mrms_panes_callback_at(key, PANE, 6.0);
        cb.field_fades = fade
            .map(|(from, share)| (FieldLayer::Mesh, FieldTexture::Mrms(from), share))
            .into_iter()
            .collect();
        cb
    };
    let mut first = at(old, None);
    first.mrms_uploads = vec![(old, big_upload(0.0)), (new, big_upload(1.3))];
    resources.prepare_pane(&device, &queue, &first);
    let _ = device.poll(wgpu::PollType::wait_indefinitely());

    let mut time = |cb: &MapCallback| -> Vec<f64> {
        resources.prepare_pane(&device, &queue, cb);
        let mut ms = Vec::with_capacity(FRAMES);
        for i in 0..FRAMES + 20 {
            let t = wxdata::clock::Instant::now();
            resources.draw_pane(&device, &queue, &view, 0, wgpu::Color::BLACK);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            // The first frames warm the pipeline and caches; they are not counted.
            if i >= 20 {
                ms.push(t.elapsed().as_secs_f64() * 1000.0);
            }
        }
        ms.sort_by(f64::total_cmp);
        ms
    };
    let pct = |v: &[f64], q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    let one = time(&at(new, None));
    let fading = time(&at(new, Some((old, 0.5))));
    let line = |name: &str, v: &[f64]| {
        format!(
            "{name}: median {:.3} ms, p95 {:.3} ms, p99 {:.3} ms over {} frames",
            pct(v, 0.5),
            pct(v, 0.95),
            pct(v, 0.99),
            v.len()
        )
    };
    let report = format!(
        "{} ({:?}), {PANE} px pane, 2000x1000 field\n{}\n{}\nmedian difference {:.3} ms",
        adapter.get_info().name,
        adapter.get_info().backend,
        line("one frame", &one),
        line("crossfading", &fading),
        pct(&fading, 0.5) - pct(&one, 0.5)
    );
    println!("{report}");

    // The mid-fade picture, for review.
    resources.prepare_pane(&device, &queue, &at(new, Some((old, 0.5))));
    resources.draw_pane(&device, &queue, &view, 0, wgpu::Color::BLACK);
    let pixels = read_target(&device, &queue, &target, PANE);
    let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/parity-review/v19/crossfade");
    std::fs::create_dir_all(&destination).unwrap();
    image::RgbaImage::from_raw(PANE, PANE, pixels)
        .unwrap()
        .save(destination.join("mid-fade.png"))
        .unwrap();
    std::fs::write(destination.join("timing.txt"), report).unwrap();
    // A sanity bound, not a budget: on a graphics card the double draw stays far under a 60 Hz
    // frame. A software renderer draws every pixel twice on the CPU, so it is reported, not bound.
    if adapter.get_info().device_type != wgpu::DeviceType::Cpu {
        assert!(pct(&fading, 0.5) < 16.0, "{}", line("crossfading", &fading));
    }
}
