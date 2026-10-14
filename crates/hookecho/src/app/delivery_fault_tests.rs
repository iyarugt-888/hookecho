//! Explicit isolated-process control through a real app, its delivery channel, upload cache,
//! and GPU resources. It does not run a window/presentation engine or an operational upstream.
use super::*;
use sha2::{Digest, Sha256};

const SIZE: u32 = 512;
const SITE: &str = "KFTG";
const NAME: &str = "KFTG20170508_203405_V06";

fn source() -> Arc<Scan> {
    Arc::new(
        wxdata::level2::decode_volume(
            include_bytes!("../../../wxdata/tests/data/corpus/denver-hail-2017-first-records.ar2")
                .to_vec(),
        )
        .unwrap(),
    )
}

fn progress(scan: &Scan) -> wxdata::live::ScanProgress {
    let sweep = &scan.sweeps()[0];
    let cut = &scan.coverage_pattern().elevation_cuts()[usize::from(sweep.elevation_number()) - 1];
    wxdata::live::ScanProgress {
        volume_start_ms: Some(
            Identifier::new(NAME.into())
                .date_time()
                .unwrap()
                .timestamp_millis(),
        ),
        vcp_number: Some(scan.coverage_pattern_number().number()),
        cut_kind: wxdata::live::CutKind::Standard,
        elevation_number: usize::from(sweep.elevation_number()),
        total_elevations: scan.coverage_pattern().elevation_cuts().len(),
        elevation_angle_deg: f64::from(sweep.elevation_angle_degrees().unwrap()),
        azimuth_rate_dps: cut.azimuth_rate_degrees_per_second(),
        azimuth_start_deg: f64::from(sweep.radials()[0].azimuth_angle_degrees()),
        azimuth_end_deg: f64::from(sweep.radials().last().unwrap().azimuth_angle_degrees()),
        chunk_index: 1,
        chunks_in_sweep: 6,
    }
}

fn send(app: &HookEchoApp, view: usize, generation: u64, name: &str, scan: Arc<Scan>) {
    let observed = progress(&scan);
    let coverage = wxdata::live::RadialCoverage {
        progress: observed,
        radials: scan.sweeps()[0]
            .radials()
            .iter()
            .map(|radial| (radial.azimuth_number(), radial.collection_timestamp()))
            .collect(),
        source_passes: None,
        source_sequences: None,
        source_attribution: None,
        source_scope: None,
    };
    app.msg_tx
        .send(DataMsg::LiveProgress {
            view,
            site: SITE.into(),
            gen: generation,
            progress: observed,
        })
        .unwrap();
    let changed = wxdata::level2::elevation_angles(&scan);
    app.msg_tx
        .send(DataMsg::Live {
            view,
            site: SITE.into(),
            gen: generation,
            name: name.into(),
            time: Identifier::new(NAME.into()).date_time().unwrap(),
            received_at: None, // controlled channel input supplies no byte-transport receipt
            radial_coverage: Some(coverage),
            scan,
            changed,
            retries: 0,
            decode_time: std::time::Duration::ZERO,
        })
        .unwrap();
}

fn frame(
    app: &mut HookEchoApp,
    state: &eframe::egui_wgpu::RenderState,
    scan: &Scan,
    index: usize,
    label: &str,
    dir: &std::path::Path,
    evidence: &mut Vec<serde_json::Value>,
) -> Vec<u8> {
    let camera = app.views[index].camera;
    let sweep = wxdata::level2::bin_scan(scan, Moment::Reflectivity, 0).unwrap();
    let mut callback = crate::headless::sweep_callback(
        &sweep,
        &camera,
        SIZE,
        crate::colormap::default_table(Moment::Reflectivity),
    );
    callback.pane = index as u32;
    let (upload, draw) = app.pane_radar(index, index);
    let uploaded = upload.is_some();
    callback.radar_upload = upload;
    callback.draw_radar = draw;
    let target = crate::headless::new_target(&state.device, state.target_format, SIZE);
    let target_view = target.create_view(&Default::default());
    {
        let mut renderer = state.renderer.write();
        let resources = renderer
            .callback_resources
            .get_mut::<RenderResources>()
            .unwrap();
        resources.prepare_pane(&state.device, &state.queue, &callback);
        resources.draw_pane(
            &state.device,
            &state.queue,
            &target_view,
            index as u32,
            wgpu::Color::BLACK,
        );
    }
    let pixels = crate::headless::read_target(&state.device, &state.queue, &target, SIZE);
    image::RgbaImage::from_raw(SIZE, SIZE, pixels.clone())
        .unwrap()
        .save(dir.join(format!("{label}.png")))
        .unwrap();
    let v = &app.views[index];
    let volume = v.volume.as_ref().unwrap();
    evidence.push(serde_json::json!({
        "frame": label, "pane": index, "uploaded": uploaded,
        "volume": volume.name, "volume_clock": volume.time,
        "live_revision": v.live_scan_revision, "volume_revision": volume.revision(),
        "source_clock": v.live_scan.volume_time, "following": v.timeline.following,
        "playing": v.timeline.playing, "sha256_rgba": format!("{:x}", Sha256::digest(&pixels)),
        "source_phase_at_host_clock": v.live_scan.phase(Utc::now(), 90).label(),
        "revision_traces": v.live_queue_timings.revision_traces(),
        "source_radials": volume.scan.sweeps().iter().map(|s| s.radials().len()).sum::<usize>(),
    }));
    pixels
}

#[test]
#[ignore = "GPU + isolated app storage/proxy: invoke this exact test in its own process"]
fn app_delivery_recovery_keeps_archive_and_sibling_frames_owned() {
    // Guard before any process-global path mutation or normal app construction. Startup public
    // HTTP work is denied by a loopback proxy; no saved user state/accounts are read or written.
    assert_eq!(
        std::env::var("HOOKECHO_APP_FAULT_CONTROL").as_deref(),
        Ok("1")
    );
    for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
        assert_eq!(std::env::var(key).as_deref(), Ok("http://127.0.0.1:9"));
    }
    assert!(
        !std::env::current_exe()
            .unwrap()
            .with_extension("exe.old")
            .exists(),
        "preserve any old executable instead of letting startup cleanup remove it"
    );
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap();
    let dir = workspace.join("target/parity-review/m1.3/app-delivery");
    let storage = dir.join(format!("storage-{}", std::process::id()));
    std::fs::create_dir_all(&storage).unwrap();
    let storage = storage.canonicalize().unwrap();
    assert!(storage.starts_with(&workspace));
    crate::paths::set_base(storage.clone());
    assert!(crate::paths::config_dir().unwrap().starts_with(&storage));
    let config = crate::paths::config_dir().unwrap();
    std::fs::create_dir_all(&config).unwrap();
    let settings = Settings {
        default_site: SITE.into(),
        setup_done: true,
        scan_chime: false,
        overlays_on: Some(Vec::new()),
        ..Settings::default()
    };
    std::fs::write(
        config.join("settings.json"),
        serde_json::to_vec(&settings).unwrap(),
    )
    .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (device, queue, adapter) = crate::headless::init_gpu(&rt).unwrap();
    let gpu = adapter.get_info();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let renderer = eframe::egui_wgpu::Renderer::new(
        &device,
        format,
        eframe::egui_wgpu::RendererOptions::PREDICTABLE,
    );
    let state = eframe::egui_wgpu::RenderState {
        available_adapters: vec![adapter.clone()],
        adapter,
        device,
        queue,
        target_format: format,
        renderer: Arc::new(egui::mutex::RwLock::new(renderer)),
        surface_config: eframe::egui_wgpu::SurfaceConfig::LOW_LATENCY,
    };
    let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    cc.wgpu_render_state = Some(state.clone());
    let mut app = HookEchoApp::new(&cc);
    let full = source();
    let partial = Arc::new(Scan::new(
        full.coverage_pattern().clone(),
        full.sweeps()
            .iter()
            .map(|sweep| {
                nexrad_model::data::Sweep::new(
                    sweep.elevation_number(),
                    sweep
                        .radials()
                        .iter()
                        .take((sweep.radials().len() / 2).max(1))
                        .cloned()
                        .collect(),
                )
            })
            .collect(),
    ));
    let time = Identifier::new(NAME.into()).date_time().unwrap();
    let camera = crate::render::mercator::Camera::at_lonlat(-104.55, 39.79, 6.0);
    app.views = vec![
        MapView::new(Some(SITE.into()), camera),
        MapView::new(Some(SITE.into()), camera),
    ];
    for view in &mut app.views {
        view.live_scan.reset(Some(SITE.into()));
        view.timeline.date = time.date_naive();
    }
    app.views[1].timeline.following = false;
    app.views[1].show_volume(full.clone(), NAME.into(), time);
    let primary = app
        .live_session
        .reconcile(
            Some(live_session::StreamRequest {
                view: 0,
                site: SITE,
                provider: "primary control",
                relay_endpoint: None,
            }),
            Instant::now(),
        )
        .start
        .unwrap();
    app.views[0]
        .live_scan
        .stream_started(primary.provider, false);
    let mut frames = Vec::new();
    send(
        &app,
        0,
        primary.generation,
        "primary-partial",
        partial.clone(),
    );
    app.poll_messages();
    assert_eq!(app.views[0].live_scan_revision, 1);
    assert_eq!(
        app.views[0].live_scan.phase(Utc::now(), 90),
        crate::live_scan::Phase::Stale,
        "an archived fixture never becomes fresh by receiving it now"
    );
    let first = frame(
        &mut app,
        &state,
        &full,
        0,
        "primary-partial",
        &dir,
        &mut frames,
    );
    let sibling = frame(
        &mut app,
        &state,
        &full,
        1,
        "sibling-archive",
        &dir,
        &mut frames,
    );
    app.msg_tx
        .send(DataMsg::LiveEnded {
            view: 0,
            site: SITE.into(),
            gen: primary.generation,
            lost: true,
            error: Some("injected transport loss".into()),
        })
        .unwrap();
    app.poll_messages();
    assert!(app.views[0]
        .live_scan
        .poll_reason()
        .starts_with("live stream unavailable"));
    assert_eq!(
        frame(
            &mut app,
            &state,
            &full,
            0,
            "lost-keeps-last-good",
            &dir,
            &mut frames
        ),
        first
    );
    let backup = app
        .live_session
        .reconcile(
            Some(live_session::StreamRequest {
                view: 0,
                site: SITE,
                provider: "backup control",
                relay_endpoint: Some("http://backup.control"),
            }),
            Instant::now(),
        )
        .start
        .unwrap();
    app.views[0].live_scan.stream_started(backup.provider, true);
    send(&app, 0, primary.generation, "late-primary", full.clone());
    send(&app, 1, backup.generation, "wrong-pane", full.clone());
    app.msg_tx
        .send(DataMsg::LiveEnded {
            view: 0,
            site: SITE.into(),
            gen: primary.generation,
            lost: true,
            error: Some("late primary end".into()),
        })
        .unwrap();
    app.poll_messages();
    assert_eq!(app.views[0].live_scan_revision, 1);
    assert!(app.live_session.accepts(0, backup.generation));
    assert_eq!(
        frame(
            &mut app,
            &state,
            &full,
            1,
            "sibling-after-failover",
            &dir,
            &mut frames
        ),
        sibling
    );
    send(&app, 0, backup.generation, "backup-full", full.clone());
    app.poll_messages();
    assert_eq!(app.views[0].live_scan_revision, 2);
    let recovered = frame(&mut app, &state, &full, 0, "backup-full", &dir, &mut frames);
    assert_ne!(
        recovered, first,
        "new pinned radials must change the rendered frame"
    );
    assert!(recovered
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| pixel[0..3] != [0, 0, 0]));

    // Reproduce the intake-before-retirement race: the user owns the archive frame now, but
    // manage_stream has not yet advanced the current subscription generation.
    app.views[0].timeline.following = false;
    app.views[0].timeline.playing = false;
    app.views[0].timeline.frames = vec![Identifier::new(NAME.into())];
    app.views[0].show_volume(full.clone(), NAME.into(), time);
    app.pane_shown.remove(&0);
    app.views[0].live_progress = None;
    let frozen_acquisition = app.views[0].live_scan.acquisition_inventory();
    let frozen_arrival = app.views[0].last_live_arrival;
    let selected = frame(
        &mut app,
        &state,
        &full,
        0,
        "selected-archive",
        &dir,
        &mut frames,
    );
    let frozen_shown = app.pane_shown.get(&0).cloned();
    assert!(
        frozen_shown.is_some(),
        "the archive frame must have a real upload"
    );
    assert!(
        selected
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[0..3] != [0, 0, 0]),
        "archive ownership must protect visible radar gates, not an empty frame"
    );
    assert!(app.live_session.accepts(0, backup.generation));
    send(
        &app,
        0,
        backup.generation,
        "queued-before-retirement",
        partial.clone(),
    );
    app.poll_messages();
    let after_selection = frame(
        &mut app,
        &state,
        &full,
        0,
        "archive-after-queued-live",
        &dir,
        &mut frames,
    );
    if app.views[0].volume.as_ref().unwrap().name != NAME {
        std::fs::copy(
            dir.join("archive-after-queued-live.png"),
            dir.join("before-overwritten-archive.png"),
        )
        .unwrap();
        std::fs::write(
            dir.join("before-fault-observations.json"),
            serde_json::to_vec_pretty(&frames).unwrap(),
        )
        .unwrap();
    }
    assert_eq!(
        app.views[0].volume.as_ref().unwrap().name,
        NAME,
        "a queued live payload must not overwrite the just-selected archive frame"
    );
    assert!(
        app.views[0].live_progress.is_none(),
        "archive cannot accept queued progress"
    );
    assert_eq!(app.views[0].live_scan_revision, 2);
    assert_eq!(
        app.views[0].live_scan.acquisition_inventory(),
        frozen_acquisition
    );
    assert_eq!(app.views[0].last_live_arrival, frozen_arrival);
    assert_eq!(app.pane_shown.get(&0), frozen_shown.as_ref());
    assert!(!frames.last().unwrap()["uploaded"].as_bool().unwrap());
    assert_eq!(after_selection, selected);
    app.views[0].timeline.following = true;
    app.views[0].timeline.playing = true;
    send(
        &app,
        0,
        backup.generation,
        "queued-during-loop",
        full.clone(),
    );
    app.poll_messages();
    assert!(
        app.views[0].live_progress.is_none(),
        "loop cannot accept queued progress"
    );
    assert_eq!(app.views[0].live_scan_revision, 2);
    assert_eq!(
        app.views[0].live_scan.acquisition_inventory(),
        frozen_acquisition
    );
    app.views[0].timeline.playing = false;
    send(&app, 0, backup.generation, "resumed-backup", full.clone());
    app.poll_messages();
    assert_eq!(app.views[0].live_scan_revision, 3);
    assert_eq!(
        frame(
            &mut app,
            &state,
            &full,
            0,
            "resumed-live",
            &dir,
            &mut frames
        ),
        recovered
    );
    assert_eq!(
        frame(
            &mut app,
            &state,
            &full,
            1,
            "sibling-after-resume",
            &dir,
            &mut frames
        ),
        sibling
    );
    std::fs::write(dir.join("observations.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "gpu": format!("{} ({:?}, {:?})", gpu.name, gpu.backend, gpu.device_type),
        "input": NAME, "frames": frames,
        "scope": "real app constructor/channel/receiver/upload/GPU; direct controlled messages from a partial NOAA fragment; no byte receipts/window/scan-out/upstream independence",
    })).unwrap()).unwrap();
}
