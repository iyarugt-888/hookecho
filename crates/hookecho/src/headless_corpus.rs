//! Real pinned radar inputs through the production map renderer. Explicit GPU
//! invocation requires an adapter and every input; absence is a failure.
use super::{init_gpu, new_target, read_target};
use crate::render::mercator::{world_to_lonlat, Camera};
use crate::render::{MapCallback, RenderResources};
use sha2::{Digest, Sha256};
use wxdata::level2::{self, BinnedSweep, Moment};

const SIZE: u32 = 384;
const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.02,
    g: 0.02,
    b: 0.02,
    a: 1.0,
};

fn callback(sweep: &BinnedSweep, camera: &Camera) -> MapCallback {
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
        radar_upload: Some(crate::app::to_upload(
            sweep,
            crate::colormap::default_table(Moment::Reflectivity),
            None,
            false,
            None,
            None,
            false,
            None,
        )),
        draw_radar: true,
        observed_upload: None,
        draw_observed: false,
        overlay_upload: None,
        draw_overlay: false,
        field_uploads: Vec::new(),
        model_uploads: Vec::new(),
        model_fields: Vec::new(),
        drop_model_fields: Vec::new(),
        field_draws: Vec::new(),
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

fn linear(byte: u8) -> f64 {
    let v = f64::from(byte) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb(value: f64) -> u8 {
    let v = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Inspector sampling is an independent CPU path from the shader. Require the
/// same code/coverage in a pixel neighborhood to exclude gate/sector boundaries
/// where f32 shader geography can legitimately choose a neighboring gate.
fn stable_sample(sweep: &BinnedSweep, camera: &Camera, x: u32, y: u32) -> Option<(u8, bool)> {
    let mut result = None;
    for (dx, dy) in [
        (0.0, 0.0),
        (-0.125, 0.0),
        (0.125, 0.0),
        (0.0, -0.125),
        (0.0, 0.125),
    ] {
        let world = camera.screen_to_world(
            (x as f32 + 0.5 + dx, y as f32 + 0.5 + dy),
            (SIZE as f32, SIZE as f32),
        );
        let (lon, lat) = world_to_lonlat(world.0, world.1);
        let sample = sweep.sample_at(lon, lat)?;
        // Avoid the radar origin and footprint edges, not the missing sectors.
        if sample.range_km < 5.0 || sample.range_km > 55.0 {
            return None;
        }
        let code = match sample.value {
            Some(v) => (2.0 + (v - sweep.value_min) / (sweep.value_max - sweep.value_min) * 253.0)
                .round() as u8,
            None => u8::from(sample.folded),
        };
        let candidate = (code, sample.collected_ms.is_some());
        if result.is_some_and(|old| old != candidate) {
            return None;
        }
        result = Some(candidate);
    }
    result
}

fn save_report(output: &std::path::Path, report: &serde_json::Value) {
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(report).unwrap(),
    )
    .expect("save visual report");
}

#[test]
#[ignore = "gpu: explicitly provision an adapter for real radar visual certification"]
fn pinned_radar_values_and_missing_sectors_render_consistently() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = repo.join("crates/wxdata/tests/data/corpus");
    let manifest_bytes =
        std::fs::read(root.join("manifest.json")).expect("required corpus manifest");
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest_bytes).expect("corpus manifest");
    assert_eq!(
        manifest["schema_version"], 4,
        "unsupported visual input contract"
    );
    let output = repo.join("target/parity-review/m0.3/visual-corpus");
    std::fs::create_dir_all(&output).expect("review directory");
    let mut report = serde_json::json!({"report_version": 1, "status": "running", "captured_at": chrono::Utc::now().to_rfc3339(), "manifest_sha256": format!("{:x}", Sha256::digest(&manifest_bytes)), "size": [SIZE, SIZE], "zoom": 8.5, "mode": "nearest reflectivity, flat map, no overlays", "comparison": "CPU inspector samples in stable subpixel neighborhoods; sRGB alpha blend; channel tolerance 8; bad colors <=0.5%; no filled missing sectors", "fixtures": []});
    save_report(&output, &report);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (device, queue, adapter) =
        init_gpu(&rt).expect("required GPU adapter; certification remains open without one");
    let info = adapter.get_info();
    eprintln!("real radar corpus adapter: {info:?}");
    report["adapter"] = serde_json::json!({"name": info.name, "backend": format!("{:?}", info.backend), "device_type": format!("{:?}", info.device_type), "vendor": info.vendor, "device": info.device, "driver": info.driver, "driver_info": info.driver_info});
    save_report(&output, &report);
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut resources = RenderResources::new(&device, format);
    let target = new_target(&device, format, SIZE);
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    for id in [
        "mayfield-2021-partial",
        "denver-hail-2017-partial",
        "clear-air-2019-partial",
    ] {
        report["active_fixture"] = id.into();
        save_report(&output, &report);
        let fixture = manifest["fixtures"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == id)
            .expect("required visual fixture");
        let name = fixture["path"].as_str().unwrap();
        assert!(
            !name.contains(['/', '\\', ':']) && !matches!(name, "." | ".."),
            "unsafe visual input path"
        );
        let bytes = std::fs::read(root.join(name)).expect("required radar input");
        assert_eq!(bytes.len() as u64, fixture["bytes"].as_u64().unwrap());
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            fixture["sha256"].as_str().unwrap(),
            "input checksum changed"
        );
        let scan = level2::decode_volume(bytes).expect("real partial volume");
        let sweep = level2::bin_scan(&scan, Moment::Reflectivity, 0).expect("real reflectivity");
        let data_before = sweep.data.clone();
        let times_before = sweep.bin_time_ms.clone();
        let camera = Camera::at_lonlat(f64::from(sweep.radar_lon), f64::from(sweep.radar_lat), 8.5);
        let mut cb = callback(&sweep, &camera);
        let upload = cb.radar_upload.take();
        cb.draw_radar = false;
        resources.render_once(&device, &queue, &view, &cb, BACKGROUND);
        let empty = read_target(&device, &queue, &target, SIZE);
        cb.radar_upload = upload;
        cb.draw_radar = true;
        resources.render_once(&device, &queue, &view, &cb, BACKGROUND);
        let actual = read_target(&device, &queue, &target, SIZE);
        image::save_buffer(
            output.join(format!("{id}.png")),
            &actual,
            SIZE,
            SIZE,
            image::ColorType::Rgba8,
        )
        .expect("save actual radar render");
        cb.radar_upload = None;
        resources.render_once(&device, &queue, &view, &cb, BACKGROUND);
        assert_eq!(
            actual,
            read_target(&device, &queue, &target, SIZE),
            "same GPU input must replay identically"
        );
        assert_eq!(
            sweep.data, data_before,
            "rendering changed scientific values"
        );
        assert_eq!(
            sweep.bin_time_ms, times_before,
            "rendering changed collection clocks"
        );
        let lut = crate::colormap::bake_lut(
            crate::colormap::default_table(Moment::Reflectivity),
            (sweep.value_min, sweep.value_max),
            None,
        );
        let (mut colored, mut missing, mut wrong_color, mut filled_missing) =
            (0usize, 0usize, 0usize, 0usize);
        for y in 2..SIZE - 2 {
            for x in 2..SIZE - 2 {
                let Some((code, observed)) = stable_sample(&sweep, &camera, x, y) else {
                    continue;
                };
                let index = ((y * SIZE + x) * 4) as usize;
                let pixel = &actual[index..index + 4];
                if !observed {
                    missing += 1;
                    filled_missing += usize::from(pixel != &empty[index..index + 4]);
                } else if code >= 2 && lut[usize::from(code) * 4 + 3] > 0 {
                    colored += 1;
                    let entry = &lut[usize::from(code) * 4..usize::from(code) * 4 + 4];
                    let alpha = f64::from(entry[3]) / 255.0;
                    let expected = [
                        srgb(linear(entry[0]) * alpha + BACKGROUND.r * (1.0 - alpha)),
                        srgb(linear(entry[1]) * alpha + BACKGROUND.g * (1.0 - alpha)),
                        srgb(linear(entry[2]) * alpha + BACKGROUND.b * (1.0 - alpha)),
                        255,
                    ];
                    wrong_color += usize::from(
                        pixel
                            .iter()
                            .zip(expected)
                            .any(|(a, e)| (i16::from(*a) - i16::from(e)).abs() > 8),
                    );
                }
            }
        }
        eprintln!("{id}: {colored} stable colored pixels, {wrong_color} color mismatches; {missing} missing-sector pixels, {filled_missing} filled");
        report["fixtures"].as_array_mut().unwrap().push(serde_json::json!({"fixture": id, "input_sha256": fixture["sha256"], "collection_time": fixture["source"]["acquisition_time"], "stable_colored_pixels": colored, "color_mismatches": wrong_color, "missing_sector_pixels": missing, "filled_missing_pixels": filled_missing, "rgba_sha256": format!("{:x}", Sha256::digest(&actual)), "png": format!("{id}.png"), "checks_passed": colored >= 100 && missing >= 1000 && filled_missing == 0 && wrong_color * 200 <= colored}));
        save_report(&output, &report);
        assert!(colored >= 100, "no meaningful observed-echo comparison");
        assert!(missing >= 1000, "no meaningful missing-sector comparison");
        assert_eq!(
            filled_missing, 0,
            "unobserved sectors must remain transparent"
        );
        assert!(
            wrong_color * 200 <= colored,
            "more than 0.5% of stable colors differ by >8 channels"
        );
    }
    report["status"] = "passed".into();
    report["active_fixture"] = serde_json::Value::Null;
    save_report(&output, &report);
}
