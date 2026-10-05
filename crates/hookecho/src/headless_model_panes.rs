//! Explicit GPU evidence for model context isolation through production prepare/paint.
use super::*;
use crate::render::{FieldLayer, ModelTextureKey};

const SIZE: u32 = 200;

fn callback(pane: u32, key: ModelTextureKey, opacity: f32) -> MapCallback {
    let camera = Camera::at_lonlat(-97.0, 35.0, 7.0);
    let viewport = (SIZE as f32, SIZE as f32);
    let (center, scale) = camera.world_to_clip_uniform(viewport);
    MapCallback {
        pane,
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
        model_uploads: Vec::new(),
        model_fields: vec![(FieldLayer::GlobalTemp2m, key)],
        drop_model_fields: Vec::new(),
        field_draws: vec![(FieldLayer::GlobalTemp2m, opacity)],
        field_swipe: None,
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
#[ignore = "gpu: verifies distinct model contexts, shared opacity and missing/retired textures"]
fn gpu_model_pane_context_isolation() {
    use sha2::{Digest, Sha256};
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (device, queue, adapter) = init_gpu(&rt).expect("GPU required for model pane isolation");
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut resources = RenderResources::new(&device, format);
    let targets = [
        new_target(&device, format, SIZE),
        new_target(&device, format, SIZE),
    ];
    let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/parity-review/model-panes/gpu");
    std::fs::create_dir_all(&destination).unwrap();
    let mut evidence = Vec::new();
    let capture = |name: &str,
                   pane: usize,
                   resources: &RenderResources,
                   evidence: &mut Vec<serde_json::Value>| {
        let view = targets[pane].create_view(&Default::default());
        resources.draw_pane(&device, &queue, &view, pane as u32, wgpu::Color::BLACK);
        let pixels = read_target(&device, &queue, &targets[pane], SIZE);
        let center = pixels[((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize..][..4].to_vec();
        let path = destination.join(format!("{name}.png"));
        image::RgbaImage::from_raw(SIZE, SIZE, pixels.clone())
            .unwrap()
            .save(&path)
            .unwrap();
        evidence.push(serde_json::json!({"name": name, "pane": pane, "center_rgba": center, "sha256": format!("{:x}", Sha256::digest(&pixels))}));
        pixels
    };
    let center = |pixels: &[u8]| -> [u8; 4] {
        pixels[((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize..][..4]
            .try_into()
            .unwrap()
    };
    let a = ModelTextureKey(1);
    let b = ModelTextureKey(2);
    let mut first = callback(0, a, 1.0);
    first.model_uploads = vec![(a, upload([255, 0, 0, 255])), (b, upload([0, 0, 255, 255]))];
    // A same-layer legacy texture must never become a fallback for a missing model key.
    first.field_uploads = vec![(FieldLayer::GlobalTemp2m, upload([0, 255, 0, 255]))];
    resources.prepare_pane(&device, &queue, &first);
    resources.prepare_pane(&device, &queue, &callback(1, b, 1.0));
    let red = capture("different-run-pane-a", 0, &resources, &mut evidence);
    let blue = capture("different-run-pane-b", 1, &resources, &mut evidence);
    assert_eq!(center(&red), [255, 0, 0, 255]);
    assert_eq!(center(&blue), [0, 0, 255, 255]);
    // Focus/order changes do not upload or relabel either cached context.
    resources.prepare_pane(&device, &queue, &callback(1, b, 1.0));
    resources.prepare_pane(&device, &queue, &callback(0, a, 1.0));
    assert_eq!(capture("focus-change-a", 0, &resources, &mut evidence), red);
    assert_eq!(
        capture("focus-change-b", 1, &resources, &mut evidence),
        blue
    );
    // Shared scientific content still has per-pane presentation uniforms.
    resources.prepare_pane(&device, &queue, &callback(0, a, 0.25));
    resources.prepare_pane(&device, &queue, &callback(1, a, 1.0));
    let dim = capture("shared-quarter-opacity", 0, &resources, &mut evidence);
    assert!(center(&dim)[0] > 100 && center(&dim)[0] < 160);
    assert_eq!(
        capture("shared-full-opacity", 1, &resources, &mut evidence),
        red
    );
    resources.prepare_pane(&device, &queue, &callback(0, ModelTextureKey(99), 1.0));
    let missing = capture("missing-context", 0, &resources, &mut evidence);
    assert_eq!(center(&missing), [0, 0, 0, 255]);
    let mut retire = callback(0, a, 1.0);
    retire.drop_model_fields = vec![a];
    resources.prepare_pane(&device, &queue, &retire);
    resources.prepare_pane(&device, &queue, &callback(1, b, 1.0));
    assert_eq!(
        capture("retired-context", 0, &resources, &mut evidence),
        missing
    );
    assert_eq!(
        capture("surviving-context", 1, &resources, &mut evidence),
        blue
    );
    let replacement = ModelTextureKey(3);
    let mut restore = callback(0, replacement, 1.0);
    restore.model_uploads = vec![(replacement, upload([255, 255, 0, 255]))];
    resources.prepare_pane(&device, &queue, &restore);
    resources.prepare_pane(&device, &queue, &callback(1, b, 1.0));
    assert_eq!(
        center(&capture(
            "replacement-context",
            0,
            &resources,
            &mut evidence
        )),
        [255, 255, 0, 255]
    );
    assert_eq!(
        capture("surviving-after-replacement", 1, &resources, &mut evidence),
        blue
    );
    std::fs::write(
        destination.join("verification.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"adapter": adapter.get_info().name, "captures": evidence}),
        )
        .unwrap(),
    )
    .unwrap();
}
