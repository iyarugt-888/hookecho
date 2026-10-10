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
    let camera = Camera::at_lonlat(-97.0, 35.0, 7.0);
    let viewport = (SIZE as f32, SIZE as f32);
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
