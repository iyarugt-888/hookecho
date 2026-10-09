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
        mrms_uploads: Vec::new(),
        mrms_fields: Vec::new(),
        drop_mrms_fields: Vec::new(),
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

/// ROADMAP_PARITY M3.3: a column user product drawn through the production field renderer lands
/// where its CPU cells say — coloured from the same quantization and LUT the app uploads, and
/// transparent wherever the column has no value. Checked at stable pixel neighbourhoods (all five
/// subpixel samples in one cell), so a cell edge where the shader may pick the neighbour is
/// excluded rather than tolerated.
#[test]
#[ignore = "gpu: explicitly provision an adapter for real radar visual certification"]
fn gpu_column_product_renders_where_its_cells_are() {
    use wxdata::udp_column::{evaluate_grid, ColumnEnv, ColumnProduct, ColumnTilt};
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bytes =
        std::fs::read(repo.join("crates/wxdata/tests/data/corpus/mayfield-2021-first-records.ar2"))
            .expect("required radar input");
    let scan = level2::decode_volume(bytes).expect("real partial volume");
    let tilts: Vec<ColumnTilt> = (0..level2::elevation_angles(&scan).len())
        .map(|t| {
            let get = |m| level2::bin_scan(&scan, m, t).ok();
            [
                get(Moment::Reflectivity),
                None,
                None,
                None,
                None,
                get(Moment::CorrelationCoefficient),
            ]
        })
        .filter(|t| t.iter().any(Option::is_some))
        .collect();
    let output = repo.join("target/parity-review/m3.3");
    std::fs::create_dir_all(&output).expect("review directory");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (device, queue, adapter) =
        init_gpu(&rt).expect("required GPU adapter; certification remains open without one");
    eprintln!("column product adapter: {:?}", adapter.get_info());
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut resources = RenderResources::new(&device, format);
    let target = new_target(&device, format, SIZE);
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let layer = crate::render::FieldLayer::UserColumn;
    let stable = |p: &ColumnProduct, camera: &Camera, x: u32, y: u32| {
        let mut out: Option<Option<u32>> = None;
        for (dx, dy) in [
            (0.0, 0.0),
            (-0.25, 0.0),
            (0.25, 0.0),
            (0.0, -0.25),
            (0.0, 0.25),
        ] {
            let w = camera.screen_to_world(
                (x as f32 + 0.5 + dx, y as f32 + 0.5 + dy),
                (SIZE as f32, SIZE as f32),
            );
            let (lon, lat) = world_to_lonlat(w.0, w.1);
            let (v, _) = p.at(lon, lat)?;
            let cell = v.map(f32::to_bits);
            if out.is_some_and(|o| o != cell) {
                return None;
            }
            out = Some(cell);
        }
        out.map(|c| c.map(f32::from_bits))
    };
    for (name, src) in [
        ("composite", "max_vertical(REF)"),
        ("core-cc", "min_vertical(CC, REF >= 30)"),
        ("column-mean", "mean_vertical(REF)"),
        ("column-fraction", "fraction_vertical(REF >= 40)"),
        ("peak-height", "max_height(REF)"),
        (
            "nested-band",
            "min_vertical(REF, REF >= max_vertical(REF) - 5)",
        ),
        ("outside-cold-layer", "max_vertical(REF, BEAM_ALTITUDE_M < MINUS30C_HEIGHT_M || BEAM_ALTITUDE_M > MINUS40C_HEIGHT_M)"),
    ] {
        // Explicit rendering parameters, not an observed Mayfield environment. Numerical
        // source matching and recorded-height accuracy are checked separately on pinned RAOBs.
        let env = ColumnEnv { antenna_altitude_m: Some(400.0), levels: wxdata::udp_column::Levels {
            hm30_m: Some(7900.0), hm40_m: Some(9100.0), ..Default::default()
        }};
        let p = evaluate_grid(
            &wxdata::udp::parse(src).unwrap(),
            &tilts,
            &env,
            chrono::DateTime::from_timestamp(1_639_193_029, 0).unwrap(),
        )
        .expect("column product on the real partial volume");
        let values: Vec<Option<f32>> = p
            .field
            .values
            .iter()
            .map(|v| v.is_finite().then_some(*v))
            .collect();
        let range = wxdata::udp_volume::auto_range(values.iter(), None).unwrap();
        let table = crate::colormap::ramp_table(range.0, range.1);
        let upload = crate::app::column_upload(&p.field, &table, range);
        let lut = crate::colormap::bake_lut(&table, range, None);
        let (lon0, lat0) = (
            f64::from(tilts[0][0].as_ref().unwrap().radar_lon),
            f64::from(tilts[0][0].as_ref().unwrap().radar_lat),
        );
        let camera = Camera::at_lonlat(lon0, lat0, 8.0);
        let mut cb = callback(tilts[0][0].as_ref().unwrap(), &camera);
        cb.radar_upload = None;
        cb.draw_radar = false;
        resources.render_once(&device, &queue, &view, &cb, BACKGROUND);
        let empty = read_target(&device, &queue, &target, SIZE);
        cb.field_uploads = vec![(layer, upload)];
        cb.field_draws = vec![(layer, 1.0)];
        resources.render_once(&device, &queue, &view, &cb, BACKGROUND);
        let actual = read_target(&device, &queue, &target, SIZE);
        image::save_buffer(
            output.join(format!("column-{name}.png")),
            &actual,
            SIZE,
            SIZE,
            image::ColorType::Rgba8,
        )
        .expect("save column render");
        let (lo, hi) = range;
        let (mut colored, mut wrong, mut clear, mut filled) = (0usize, 0usize, 0usize, 0usize);
        for y in 2..SIZE - 2 {
            for x in 2..SIZE - 2 {
                let Some(cell) = stable(&p, &camera, x, y) else {
                    continue;
                };
                let i = ((y * SIZE + x) * 4) as usize;
                let pixel = &actual[i..i + 4];
                match cell {
                    None => {
                        clear += 1;
                        filled += usize::from(pixel != &empty[i..i + 4]);
                    }
                    Some(v) => {
                        let code =
                            (2.0 + ((v - lo) / (hi - lo)).clamp(0.0, 1.0) * 253.0).round() as usize;
                        let e = &lut[code * 4..code * 4 + 4];
                        if e[3] == 0 {
                            continue;
                        }
                        colored += 1;
                        let a = f64::from(e[3]) / 255.0;
                        let want = [
                            srgb(linear(e[0]) * a + BACKGROUND.r * (1.0 - a)),
                            srgb(linear(e[1]) * a + BACKGROUND.g * (1.0 - a)),
                            srgb(linear(e[2]) * a + BACKGROUND.b * (1.0 - a)),
                        ];
                        let bad = pixel
                            .iter()
                            .zip(want)
                            .any(|(a, e)| (i16::from(*a) - i16::from(e)).abs() > 8);
                        wrong += usize::from(bad);
                    }
                }
            }
        }
        eprintln!(
            "{src}: {colored} stable coloured pixels, {wrong} colour mismatches; \
             {clear} empty-cell pixels, {filled} filled"
        );
        assert!(
            colored > 500 && clear > 1000,
            "{src}: too few stable samples"
        );
        assert!(
            wrong * 200 <= colored,
            "{src}: {wrong} of {colored} pixels mis-coloured"
        );
        assert_eq!(filled, 0, "{src}: an empty column was drawn");
    }
}

/// ROADMAP_PARITY M3.4: a trail's age fade is per-gate display opacity. Gates at full opacity
/// must draw exactly as with no opacity texture at all; faded gates blend their unchanged colour
/// at the requested alpha; the uploaded values are untouched.
#[test]
#[ignore = "gpu: explicitly provision an adapter for real radar visual certification"]
fn gpu_gate_opacity_fades_display_only() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bytes =
        std::fs::read(repo.join("crates/wxdata/tests/data/corpus/mayfield-2021-first-records.ar2"))
            .expect("required radar input");
    let scan = level2::decode_volume(bytes).expect("real partial volume");
    let sweep = level2::bin_scan(&scan, Moment::Reflectivity, 0).expect("real reflectivity");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (device, queue, _adapter) =
        init_gpu(&rt).expect("required GPU adapter; certification remains open without one");
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut resources = RenderResources::new(&device, format);
    let target = new_target(&device, format, SIZE);
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let camera = Camera::at_lonlat(f64::from(sweep.radar_lon), f64::from(sweep.radar_lat), 8.5);
    let render = |resources: &mut RenderResources, alpha: Vec<u8>| {
        let mut cb = callback(&sweep, &camera);
        if let Some(up) = cb.radar_upload.as_mut() {
            up.gate_alpha = alpha;
        }
        resources.render_once(&device, &queue, &view, &cb, BACKGROUND);
        read_target(&device, &queue, &target, SIZE)
    };
    let data_before = sweep.data.clone();
    let plain = render(&mut resources, Vec::new());
    // Opaque everywhere: identical to no texture.
    let opaque = render(&mut resources, vec![255; sweep.data.len()]);
    assert_eq!(plain, opaque, "an opaque fade must not change a pixel");
    // The eastern half of the azimuths at a quarter opacity.
    const FLOOR: u8 = 64;
    let half: Vec<u8> = (0..sweep.az_bins)
        .flat_map(|bin| {
            let a = if bin < sweep.az_bins / 2 { FLOOR } else { 255 };
            std::iter::repeat_n(a, sweep.gate_count)
        })
        .collect();
    // Upload into a fresh resource set so the opacity texture is built, not just rewritten.
    let mut fresh = RenderResources::new(&device, format);
    let faded = render(&mut fresh, half);
    let empty = {
        let mut cb = callback(&sweep, &camera);
        cb.radar_upload = None;
        cb.draw_radar = false;
        fresh.render_once(&device, &queue, &view, &cb, BACKGROUND);
        read_target(&device, &queue, &target, SIZE)
    };
    let (mut kept, mut blended, mut wrong) = (0usize, 0usize, 0usize);
    for y in 2..SIZE - 2 {
        for x in 2..SIZE - 2 {
            let i = ((y * SIZE + x) * 4) as usize;
            if plain[i..i + 4] == empty[i..i + 4] {
                continue; // nothing drawn here
            }
            let w = camera
                .screen_to_world((x as f32 + 0.5, y as f32 + 0.5), (SIZE as f32, SIZE as f32));
            let (lon, lat) = world_to_lonlat(w.0, w.1);
            let Some(g) = sweep.sample_at(lon, lat) else {
                continue;
            };
            // Stay clear of the north and south seams between the two halves.
            let az = f64::from(g.azimuth_deg);
            if !(5.0..175.0).contains(&az) && !(185.0..355.0).contains(&az) {
                continue;
            }
            if az >= 180.0 {
                kept += 1;
                wrong += usize::from(faded[i..i + 4] != plain[i..i + 4]);
            } else {
                blended += 1;
                let a = f64::from(FLOOR) / 255.0;
                let bg = [BACKGROUND.r, BACKGROUND.g, BACKGROUND.b];
                let bad = (0..3).any(|c| {
                    let full = linear(plain[i + c]);
                    let want = srgb(full * a + bg[c] * (1.0 - a));
                    (i16::from(faded[i + c]) - i16::from(want)).abs() > 8
                });
                wrong += usize::from(bad);
            }
        }
    }
    eprintln!("gate opacity: {kept} opaque pixels, {blended} faded pixels, {wrong} wrong");
    assert!(kept > 1_000 && blended > 1_000, "too few drawn pixels");
    assert!(wrong * 200 <= kept + blended, "{wrong} pixels wrong");
    assert_eq!(sweep.data, data_before, "the fade changed values");
}

/// ROADMAP_PARITY M1.2: a live upload's receipt clock reaches the GPU-completion stage — reported
/// only after the frame that drew it has finished on the device, never before its queue writes.
#[test]
#[ignore = "gpu: explicitly provision an adapter for real radar visual certification"]
fn gpu_live_upload_reports_queue_and_completion_stages_in_order() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bytes =
        std::fs::read(repo.join("crates/wxdata/tests/data/corpus/mayfield-2021-first-records.ar2"))
            .expect("required radar input");
    let scan = level2::decode_volume(bytes).expect("real partial volume");
    let sweep = level2::bin_scan(&scan, Moment::Reflectivity, 0).expect("real reflectivity");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (device, queue, _adapter) =
        init_gpu(&rt).expect("required GPU adapter; certification remains open without one");
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut resources = RenderResources::new(&device, format);
    let target = new_target(&device, format, SIZE);
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let camera = Camera::at_lonlat(f64::from(sweep.radar_lon), f64::from(sweep.radar_lat), 8.5);
    let timings = std::sync::Arc::new(crate::render::LiveQueueTimings::default());
    let received = wxdata::clock::Instant::now();
    let mut cb = callback(&sweep, &camera);
    if let Some(up) = cb.radar_upload.as_mut() {
        up.telemetry = Some((received, std::sync::Arc::clone(&timings)));
    }
    resources.render_once(&device, &queue, &view, &cb, BACKGROUND);
    assert_eq!(
        timings.samples_micros().len(),
        1,
        "queue writes recorded at upload"
    );
    assert!(
        timings.gpu_done_samples_micros().is_empty(),
        "no completion before the drawing frame was even asked about"
    );
    // The pane's next frame registers the completion of the one that drew the upload.
    let mut next = callback(&sweep, &camera);
    next.radar_upload = None;
    resources.render_once(&device, &queue, &view, &next, BACKGROUND);
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll");
    let queued = timings.samples_micros()[0];
    let done = timings.gpu_done_samples_micros();
    assert_eq!(done.len(), 1, "one completion per live upload");
    assert!(
        done[0] >= queued,
        "completion {} µs before queue {} µs",
        done[0],
        queued
    );
    eprintln!("receipt → queue {queued} µs, → GPU done {} µs", done[0]);
    // A LUT-only recolour is not a live update and adds no sample.
    let mut recolour = callback(&sweep, &camera);
    if let Some(up) = recolour.radar_upload.as_mut() {
        up.lut_only = true;
        up.data.clear();
        up.telemetry = Some((received, std::sync::Arc::clone(&timings)));
    }
    resources.render_once(&device, &queue, &view, &recolour, BACKGROUND);
    resources.render_once(&device, &queue, &view, &next, BACKGROUND);
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll");
    assert_eq!(timings.gpu_done_samples_micros().len(), 1);
    // A frame followed too late is excluded and counted, never recorded as a long latency.
    let mut late = callback(&sweep, &camera);
    if let Some(up) = late.radar_upload.as_mut() {
        up.data[0] ^= 1; // a real change, so it is a live write
        up.telemetry = Some((
            wxdata::clock::Instant::now(),
            std::sync::Arc::clone(&timings),
        ));
    }
    resources.render_once(&device, &queue, &view, &late, BACKGROUND);
    std::thread::sleep(std::time::Duration::from_millis(150));
    resources.render_once(&device, &queue, &view, &next, BACKGROUND);
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll");
    assert_eq!(timings.gpu_done_samples_micros().len(), 1);
    assert_eq!(timings.unobserved(), 1);
}

/// ROADMAP_PARITY M1.2 / 1008.md A3: one correlated trace from receipt to GPU completion, per
/// frame identity, on the device this runs on. Each iteration takes the Moore 2013 volume's bytes
/// as received and times, against that one receipt clock:
///
/// - decode (`level2::decode_volume`);
/// - binning the 0.5° reflectivity sweep (`level2::bin_scan`);
/// - the 2D upload's GPU queue writes and the GPU finishing the frame that drew it (the app's own
///   `LiveQueueTimings` stages);
/// - building the 3D smooth volume (192 x 192 x 48, as the 3D window does) and its upload until
///   the GPU has it.
///
/// Presentation (scan-out) is not observable here and is not reported. Writes
/// `target/parity-review/m1.2/correlated-trace.csv` (one row per iteration, with the volume's name,
/// scan time and tilt) and `correlated-trace.txt` (p50/p95 per stage).
/// `cargo test -p hookecho --release --lib gpu_correlated_latency_trace -- --ignored --nocapture`
#[test]
#[ignore = "gpu: writes the correlated receipt-to-GPU trace"]
fn gpu_correlated_latency_trace() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = repo.join("target/scientific-corpus/KTLX20130520_201229_V06.gz");
    let Ok(bytes) = std::fs::read(&path) else {
        println!("SKIP: {} not provisioned", path.display());
        return;
    };
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (device, queue, adapter) =
        init_gpu(&rt).expect("required GPU adapter; certification remains open without one");
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut resources = RenderResources::new(&device, format);
    let mut res3d = crate::render3d::Volume3dResources::new(&device, format);
    let target = new_target(&device, format, SIZE);
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
    let mut rows = vec![
        "iteration,volume,scan_time_utc,tilt,decode_ms,bin_ms,queue_writes_ms,gpu_done_ms,build3d_ms,upload3d_done_ms"
            .to_string(),
    ];
    let mut stages: [Vec<f64>; 6] = Default::default();
    const RUNS: usize = 25;
    for i in 0..RUNS + 3 {
        let received = wxdata::clock::Instant::now();
        // The same clock for the stages this test times itself.
        let received_std = received;
        let scan = level2::decode_volume(bytes.clone()).expect("Moore 2013 decodes");
        let decode = received_std.elapsed();
        let sweep = level2::bin_scan(&scan, Moment::Reflectivity, 0).expect("reflectivity");
        let bin = received_std.elapsed();
        let timings = std::sync::Arc::new(crate::render::LiveQueueTimings::default());
        let camera = Camera::at_lonlat(f64::from(sweep.radar_lon), f64::from(sweep.radar_lat), 8.5);
        let mut cb = callback(&sweep, &camera);
        if let Some(up) = cb.radar_upload.as_mut() {
            up.telemetry = Some((received, std::sync::Arc::clone(&timings)));
        }
        resources.render_once(&device, &queue, &view, &cb, BACKGROUND);
        let mut next = callback(&sweep, &camera);
        next.radar_upload = None;
        resources.render_once(&device, &queue, &view, &next, BACKGROUND);
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("device poll");
        let queued = timings.samples_micros().first().copied();
        let done = timings.gpu_done_samples_micros().first().copied();
        let (Some(queued), Some(done)) = (queued, done) else {
            panic!("iteration {i}: a stage was not observed");
        };
        // 3D: the smooth volume as the 3D window builds it, then its upload until the GPU has it.
        let sweeps: Vec<_> = (0..level2::elevation_angles(&scan).len())
            .filter_map(|t| level2::bin_scan_opts(&scan, Moment::Reflectivity, t, false).ok())
            .collect();
        let half_km = wxdata::volume3d::max_sample_range_km(&sweeps).max(50.0);
        let v3 = wxdata::volume3d::build(&sweeps, 192, 48, half_km, 18.0).expect("volume");
        let build3d = received_std.elapsed();
        let lut = crate::colormap::bake_lut(
            crate::colormap::default_table(Moment::Reflectivity),
            (v3.value_min, v3.value_max),
            None,
        )
        .to_vec();
        res3d.upload(
            &device,
            &queue,
            &crate::render3d::Volume3dUpload {
                data: crate::render3d::pack_rg8(&v3.data),
                n: v3.n as u32,
                nz: v3.nz as u32,
                lut,
                half_km: v3.half_km,
                center_km: [0.0, 0.0],
                top_km: v3.top_km,
                outside: 0.0,
                value_range: None,
                lut_range: None,
            },
        );
        queue.submit(std::iter::empty());
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("device poll");
        let upload3d = received_std.elapsed();
        if i < 3 {
            continue; // warm-up: pipelines, allocator, caches
        }
        let row = [
            ms(decode),
            ms(bin),
            queued as f64 / 1000.0,
            done as f64 / 1000.0,
            ms(build3d),
            ms(upload3d),
        ];
        for (k, v) in row.iter().enumerate() {
            stages[k].push(*v);
        }
        rows.push(format!(
            "{},{},{},0,{:.2},{:.2},{:.2},{:.2},{:.2},{:.2}",
            i - 3,
            "KTLX20130520_201229_V06",
            "2013-05-20T20:12:29Z",
            row[0],
            row[1],
            row[2],
            row[3],
            row[4],
            row[5]
        ));
    }
    let names = [
        "receipt -> decoded",
        "receipt -> 0.5 deg REF binned",
        "receipt -> 2D GPU queue writes",
        "receipt -> GPU finished the 2D frame",
        "receipt -> 3D volume built",
        "receipt -> 3D volume on the GPU",
    ];
    let mut report = format!(
        "adapter: {} ({:?})\nvolume: {} ({} UTC), {RUNS} runs after 3 warm-ups, all stages from one receipt clock per run\n\nstage                                  p50 ms   p95 ms\n",
        adapter.get_info().name,
        adapter.get_info().backend,
        "KTLX20130520_201229_V06",
        "2013-05-20 20:12:29",
    );
    for (name, mut v) in names.into_iter().zip(stages) {
        v.sort_by(f64::total_cmp);
        report.push_str(&format!(
            "{name:38} {:7.1}  {:7.1}\n",
            v[v.len() / 2],
            v[(v.len() * 95 / 100).min(v.len() - 1)]
        ));
    }
    report.push_str("\nPresentation (display scan-out) is not observed and not reported.\n");
    print!("{report}");
    let dir = repo.join("target/parity-review/m1.2");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("correlated-trace.csv"), rows.join("\n") + "\n").unwrap();
    std::fs::write(dir.join("correlated-trace.txt"), report).unwrap();
}
