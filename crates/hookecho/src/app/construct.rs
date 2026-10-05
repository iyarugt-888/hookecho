//! Building the app: HookEchoApp::new, restoring settings, panes, workspace and caches at launch.
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::self_update::cleanup();
        // Inter in front, Phosphor's icon glyphs behind it (the mobile chrome draws line icons
        // egui's default face has none of), and on native egui's own faces behind both as the
        // fallback for anything Inter's subset dropped. The browser build starts without those
        // fallbacks and fetches them a moment later — see `crate::fonts`.
        cc.egui_ctx.set_fonts(crate::fonts::base());
        #[cfg(target_arch = "wasm32")]
        crate::fonts::spawn_load(cc.egui_ctx.clone());

        #[cfg(not(target_arch = "wasm32"))]
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        #[cfg(not(target_arch = "wasm32"))]
        let spawner = crate::rt::Spawner::new(rt.handle().clone());
        #[cfg(target_arch = "wasm32")]
        let spawner = crate::rt::Spawner::new();

        let render_state = cc.wgpu_render_state.as_ref().expect("wgpu backend");
        // Which GPU actually got picked. One line, at startup, because every performance report
        // is unreadable without it — "the map is choppy" means one thing on a discrete adapter
        // and another on llvmpipe, and nothing in the app said which one was running. Kept (not
        // just logged) for ROADMAP_NEW N4's diagnostics bundle, which wants it long after startup.
        let gpu_info = {
            let info = render_state.adapter.get_info();
            log::info!(
                "gpu: {} ({:?}, {:?}) driver {}",
                info.name,
                info.device_type,
                info.backend,
                info.driver
            );
            format!("{} ({:?}, {:?})", info.name, info.device_type, info.backend)
        };
        // Device loss on wasm is unrecoverable from inside the app: WebGPU (Safari 26+) loses
        // devices silently — black canvas, `webglcontextlost` can never fire — and on the WebGL
        // fallback (WebKitGTK) wgpu marks the device lost for gles errors that never surface as
        // a JS context-loss event either (seen on NVIDIA + WebKitGTK: dies at first paint, the
        // page-level handler never fires). Reload is the only recovery on every backend.
        //
        // The throttle lives in the URL, not sessionStorage — WebKit blocks storage in
        // third-party iframes (the StormDesk embed), and a throttle that fails open there is a
        // reload loop (the Ubuntu lockup). One reload per navigation: flag already present means
        // this navigation was the retry, so stay on the dead canvas. index.src.html strips the
        // flag after 60s of healthy running, earning a future retry.
        #[cfg(target_arch = "wasm32")]
        render_state.device.set_device_lost_callback(|reason, msg| {
            // `Dropped`/`ReplacedCallback` are clean teardown, not failure.
            if !matches!(reason, wgpu::DeviceLostReason::Unknown) {
                return;
            }
            log::error!("wgpu device lost: {msg}");
            let Some(win) = web_sys::window() else { return };
            let search = win.location().search().unwrap_or_default();
            if search.contains("relaunched") {
                return;
            }
            let sep = if search.is_empty() { "?" } else { "&" };
            // Setting `search` navigates; the spec keeps the fragment, so `#goto=…` survives.
            let _ = win
                .location()
                .set_search(&format!("{search}{sep}relaunched"));
        });
        // The GPU's 2D texture-size cap: desktop/Adreno do 16384, but many mobile GPUs cap at
        // 4096. Field grids (MRMS rotation/AzShear reach 14000 px) are decimated to fit this.
        let max_texture_dim = render_state.device.limits().max_texture_dimension_2d;
        // The raymarched volume is one `VOL3D_N` cubed 3D texture, and not every backend can hold
        // one. Asked once, from the device's own limit, rather than from the target: WebGPU can do
        // this and the WebGL2 fallback cannot, and which of the two a browser gives you is a
        // runtime fact, not a compile-time one. A desktop GL driver old enough to say no gets the
        // same honest answer instead of an empty window.
        let volume3d_supported =
            render_state.device.limits().max_texture_dimension_3d as usize >= VOL3D_MIN_DIM;
        let vol3d_max_dim = render_state.device.limits().max_texture_dimension_3d as usize;
        {
            // Shaders and pipelines are compiled here, synchronously, before the first paint —
            // the suspected dominant term in a cold launch. Timed so the guess is a number.
            #[cfg(not(target_arch = "wasm32"))]
            let pipelines_at = std::time::Instant::now();
            let mut w = render_state.renderer.write();
            w.callback_resources.insert(RenderResources::new(
                &render_state.device,
                render_state.target_format,
            ));
            // The 3D volume pipeline is NOT compiled here — see `Volume3dCallback::prepare`.
            // Most sessions never open that window, and it was paying for it at every launch.
            w.callback_resources
                .insert(crate::render3d::Volume3dFormat(render_state.target_format));
            #[cfg(not(target_arch = "wasm32"))]
            log::info!(
                "perf: pipelines compiled in {} ms",
                pipelines_at.elapsed().as_millis()
            );
        }

        // Registering with the StatusNotifier host is a blocking D-Bus round trip; started here
        // so it overlaps the rest of construction instead of sitting in front of the first frame.
        let (tray_rx_init, tray_present_init) = crate::tray::spawn();

        let mut settings = Settings::load();
        let compare_view = settings.compare_view;
        // The starter arrangements worth having before you have built any of your own. Once
        // only: the flag is what makes deleting them stick.
        if settings.workspaces.is_empty() && !settings.seeded_workspaces {
            settings.workspaces = crate::workspace::starters();
            settings.seeded_workspaces = true;
            settings.save();
        }
        let seeded = settings.seeded_workspaces;
        let offered = crate::workspace::offer_new_starters(
            &mut settings.workspaces,
            &mut settings.offered_starters,
            seeded,
        );
        if crate::workspace::upgrade_starters(&mut settings.workspaces) || offered {
            settings.save();
        }
        // Sample terrain at the resolution this user packs at, so a hi-res pack is actually read.
        crate::elevation::set_hires(settings.pack_hires_dem);
        // theme_plan.md §4: a saved "Analyst mode: on" needs the log level raised again on this
        // launch too — the Settings checkbox only catches a mid-session toggle, not a level that
        // was already on when the process started.
        crate::devlog::set_analyst_mode(settings.analyst_mode);
        // A decoded volume is tens of MB, so the phone's cache is sized to the loop window it can
        // actually afford (see ANDROID_LOOP_WINDOW) plus the head and the frame in flight —
        // enough that a loop stops re-downloading itself on every wrap, without the ~900 MB RSS
        // that holding a full desktop-sized window cost.
        // The browser gets the same treatment for the same reason, only harder: a wasm heap is
        // 32-bit, so thirty decoded volumes is not a large cache there, it is an out-of-memory.
        let scan_cache_cap = if cfg!(target_os = "android") {
            ANDROID_LOOP_WINDOW + 2
        } else if cfg!(target_arch = "wasm32") {
            WEB_LOOP_WINDOW + 4
        } else {
            30
        };
        // The alert overlay from the last run, minus anything that has expired since. Also seeds
        // the known-warning ids, so a restart during an event doesn't re-banner and re-speak
        // every warning already on the map.
        let seeded_alerts: Vec<GeoFeature> = Vec::new();
        let known_warning_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        // Zone geometry (county and forecast-zone shapes) never changes, so it outlives the run.
        if let Some(dir) = crate::paths::cache_dir() {
            wxdata::alerts::set_zone_cache_dir(dir);
        }
        // Whatever the last run's quiet hours were still holding when it closed.
        let quiet_pending = settings.quiet_pending.clone();
        // The user's cap overrides, before anything that sweeps or reports against them.
        crate::tiles::set_cache_caps(settings.tile_disk_cache_mb, settings.volume_cache_mb);
        // Archived volumes are kept on disk forever within a cap; same startup sweep the tile
        // caches get, for the same reason (mid-session deletion would race the fetch tasks).
        if let Some(root) = crate::paths::cache_dir().map(|d| d.join("volumes")) {
            crate::tiles::sweep_later(root, "volume cache", crate::tiles::volume_cache_bytes());
        }
        // The small caches had no sweep at all: zone geometry and archived RAOB soundings grew
        // for the life of the install. They are small enough that the cap is a tripwire.
        if let Some(dir) = crate::paths::cache_dir() {
            for (sub, label) in [
                ("zones", "zone cache"),
                ("raob", "RAOB cache"),
                ("snapshots", "snapshot cache"),
                ("pficons", "placefile icon cache"),
            ] {
                crate::tiles::sweep_later(dir.join(sub), label, crate::tiles::SMALL_CACHE_BYTES);
            }
        }
        // GRIB messages, MRMS grids and GOES scans never change once published, so scrubbing
        // back or re-opening reads them from disk (or, on the web, IndexedDB) instead of the
        // bucket.
        #[cfg(not(target_arch = "wasm32"))]
        crate::object_store::install();
        #[cfg(target_arch = "wasm32")]
        crate::webcache::install_object_store();
        let mut tiles = TileManager::new(spawner.clone());
        let mut vtiles = crate::vector_tiles::VectorTileManager::new(spawner.clone());
        // Tile workers wake the UI the moment a tile is ready; without this a finished tile waits
        // for the next repaint the app happens to want.
        tiles.set_ctx(cc.egui_ctx.clone());
        vtiles.set_ctx(cc.egui_ctx.clone());
        // One small JSON fetch up front: a chase pack can ask for street tiles while a raster
        // basemap is showing, and without the template `pack_jobs` would return nothing.
        // Not on the web, where there are no chase packs and this is one more request racing the
        // radar on the critical path — `request_missing` calls it anyway if vector tiles are used.
        #[cfg(not(target_arch = "wasm32"))]
        vtiles.ensure_template();
        let (msg_tx, msg_rx) = std::sync::mpsc::channel();
        let (overlay_tx, overlay_rx) = std::sync::mpsc::channel();
        // Loaded off the launch path: the snapshot is a few MB of JSON, and parsing it before
        // the first paint bought nothing — it is applied as `OverlayMsg::AlertSeed`, and dropped
        // if the live fetch has already landed by then.
        {
            let tx = overlay_tx.clone();
            spawner.spawn(async move {
                let feats = wxdata::task::blocking(crate::alert_snapshot::load)
                    .await
                    .unwrap_or_default();
                if !feats.is_empty() {
                    let _ = tx.send(OverlayDelivery::Immediate(OverlayMsg::AlertSeed(feats)));
                }
            });
        }
        let (update_tx, update_rx) = std::sync::mpsc::channel();
        let (geocode_tx, geocode_rx) = std::sync::mpsc::channel();
        let (ipgeo_tx, ipgeo_rx) = std::sync::mpsc::channel::<(f64, f64)>();
        #[cfg(not(target_arch = "wasm32"))]
        drop(ipgeo_tx); // native never asks; the receiver just stays empty
        let (pf_icon_tx, pf_icon_rx) = std::sync::mpsc::channel();
        let (blockage_tx, blockage_rx) = std::sync::mpsc::channel();
        let (lowest_tilt_tx, lowest_tilt_rx) = std::sync::mpsc::channel();
        // Every app-level fetch (alerts, overlays, placefiles, radar index) goes through this one.
        // A hung request with no timeout leaves whatever it was loading stuck loading forever.
        let http = crate::platform::http_timeouts(reqwest::Client::builder())
            .build()
            .unwrap_or_default();
        let acquisition =
            OverlayAcquisition::new(http.clone(), spawner.clone(), overlay_tx.clone());

        // Open on the saved startup view if set (and its site still resolves), else where the app
        // was last looking, else the default site.
        let resume = settings.start_view.as_ref().or(settings.last_view.as_ref());
        // Nothing to resume means the default site is a guess, not a choice — the browser build
        // improves on it below with the edge's geo-IP.
        #[cfg(target_arch = "wasm32")]
        let opened_on_default =
            !matches!(resume, Some(sv) if wxdata::sites::site_by_id(&sv.site).is_some());
        let (start, camera) = match resume {
            Some(sv) if wxdata::sites::site_by_id(&sv.site).is_some() => (
                sv.site.clone(),
                Camera {
                    center: (sv.x, sv.y),
                    zoom: sv.zoom,
                    pitch: 0.0,
                    bearing: 0.0,
                },
            ),
            _ => {
                let s = settings.default_site.clone();
                let cam = wxdata::sites::site_by_id(&s)
                    .map(|site| Camera::at_lonlat(site.longitude as f64, site.latitude as f64, 8.0))
                    .unwrap_or_else(|| Camera::at_lonlat(-97.28, 35.33, 8.0));
                (s, cam)
            }
        };
        let mut view = MapView::new(Some(start.clone()), camera);
        view.smooth = settings.smooth_radar;
        // Restore the persisted basemap (empty slug = keep the default; from_slug("") = None).
        if !settings.basemap.is_empty() {
            view.basemap = crate::tiles::BasemapStyle::from_slug(&settings.basemap);
        }
        let settings_setup_done = settings.setup_done;

        let mut app = Self {
            vtiles,
            labels: crate::labelplace::Placer::default(),
            spawner,
            #[cfg(not(target_arch = "wasm32"))]
            _rt: rt,
            tiles,
            saved: settings.clone(),
            settings,
            views: vec![view],
            active: 0,
            msg_rx,
            msg_tx,
            about_open: false,
            update_state: ui::about_window::UpdateState::Idle,
            update_chip_hidden: false,
            update_tx,
            update_rx,
            geocode_tx,
            geocode_rx,
            #[cfg(target_arch = "wasm32")]
            ipgeo_tx,
            ipgeo_rx,
            chasepack: None,
            pane_shown: std::collections::HashMap::new(),
            pane_lut: std::collections::HashMap::new(),
            theme_applied: None,
            settings_checked: None,
            frame_nr: 0,
            palette_cache: None,
            vlabel_cache: None,
            nowcast_cache: None,
            tds_cache: None,
            tds_shown_cache: LruCache::new(NonZeroUsize::new(48).unwrap()),
            tbss_cache: None,
            zdr_cache: None,
            couplet_cache: None,
            llsd_cache: None,
            llsd_tracker: None,
            llsd_job: None,
            tornado_shown: None,
            couplet_inputs: None,
            rot_shown_cache: LruCache::new(NonZeroUsize::new(48).unwrap()),
            celltrack_cache: LruCache::new(NonZeroUsize::new(48).unwrap()),
            tracks_cache: None,
            tds_tracks_cache: None,
            rot_tracks_cache: None,
            show_local_tracks: false,
            trail: None,
            trail_more: false,
            dock: chrome::DockState::default(),
            show_scan_age: false,
            scan_age_rings: std::collections::HashMap::new(),
            site_dialog: None,
            firstrun: {
                let mut w = ui::firstrun::FirstRun::default();
                // Never in an embed: the host page already chose the site, and its storage is
                // partitioned, so "first run" would be every run — a setup dialog over someone
                // else's dashboard panel, forever.
                if !settings_setup_done && !is_embed() {
                    w.start();
                }
                w
            },
            tour: Default::default(),
            tour_anchors: Default::default(),
            settings_window: Default::default(),
            palettes: Palettes::default(),
            live_session: Default::default(),
            // DVR: retain a deep buffer of decoded volumes so instant replay serves recent frames
            // from RAM without re-downloading (~30 volumes ≈ 2.5 h at a 5-min cadence).
            // Phones can't hold a 2.5 h DVR buffer of decoded volumes — each is tens of MB and
            // Android kills the process long before the LRU fills.
            // On Android the cap used to be 6 against a 10-frame loop window, so every wrap of the
            // loop missed on every frame and re-downloaded the whole thing, forever. It has to
            // hold the window plus the head and the frame being fetched.
            prefetching: Arc::new(Mutex::new(PrefetchBook::new())),
            autoplay_pending: cfg!(target_arch = "wasm32"),
            boot_at: Instant::now(),
            scan_cache: LruCache::new(NonZeroUsize::new(scan_cache_cap).unwrap()),
            light_frames: Default::default(),
            http,
            overlay_rx,
            overlay_tx,
            acquisition,
            filters: OverlayFilters::default(),
            // Seeded from the last run so a restart mid-outbreak draws the warnings that are
            // already on the ground, and doesn't re-banner them as new (see `alert_snapshot`).
            alert_features: seeded_alerts,
            arch_warns: LruCache::new(NonZeroUsize::new(50).unwrap()),
            arch_mds: LruCache::new(NonZeroUsize::new(50).unwrap()),
            arch_md_inflight: None,
            arch_md_shown: None,
            arch_warn_inflight: None,
            arch_warn_shown: None,
            arch_lsr: LruCache::new(NonZeroUsize::new(50).unwrap()),
            arch_lsr_inflight: None,
            arch_lsr_shown: None,
            outlook_features: std::array::from_fn(|_| Vec::new()),
            md_features: Vec::new(),
            watch_features: Vec::new(),
            wssi_features: Vec::new(),
            ero_features: Vec::new(),
            fire_features: Vec::new(),
            show_mping: false,
            mping_reports: Vec::new(),
            mping_last_fetch: None,
            show_recon: false,
            recon: Vec::new(),
            recon_last_fetch: None,
            tropical_wind_kt: None,
            tropical_surge: false,
            show_pireps: false,
            pireps: Vec::new(),
            pirep_last_fetch: None,
            show_probsevere: false,
            probsevere: Vec::new(),
            probsevere_last_fetch: None,
            overlays: Vec::new(),
            overlay_gen: 0,
            built_gen: u64::MAX,
            built_zoom_bucket: i32::MIN,
            built_imported_visible: true,
            built_theme: crate::settings::Theme::Dark,
            built_globe: false,
            pending_overlay: None,
            overlay_ready: false,
            overlay_last_fetch: None,
            detail: None,
            cell_popup: None,
            cell_details: false,
            cell_follow_toggle: false,
            cell_view3d: false,
            gate_popup: None,
            suitability_popup: None,
            marker_popup: None,
            global_model: wxdata::global::GlobalModel::default(),
            global_fcst_hour: 0,
            diff_field: compare_view.map(|v| v.0).unwrap_or_default(),
            diff_mode: compare_view.map(|v| v.1).unwrap_or_default(),
            diff_valid: None,
            diff_error: None,
            diff_grid: None,
            diff_pct: None,
            goes_west_key: std::collections::HashMap::new(),
            goes_rgb_fetched: None,
            goes_fetched_sector: std::collections::HashMap::new(),
            goes_footprint: None,
            goes_footprint_probe: None,
            goes_fetched_slot: std::collections::HashMap::new(),
            glm_archive: None,
            glm_archive_slot: None,
            goto_poll: None,
            #[cfg(target_arch = "wasm32")]
            last_goto_hash: None,
            diff_key: None,
            diff_display_key: None,
            ensemble: crate::ensemble_layer::EnsembleView::default(),
            ensemble_run: None,
            ensemble_grid: None,
            ensemble_key: None,
            ensemble_display_key: None,
            ensemble_error: None,
            compare_valid: None,
            compare_error: None,
            compare_grid: None,
            compare_key: None,
            sounding_at: None,
            zone_pts: Vec::new(),
            zone_naming: None,
            zone_popup: None,
            pending_spotter: None,
            video_player: None,
            cells_window: Default::default(),
            help_hub: Default::default(),
            rules_window: Default::default(),
            drawer: Default::default(),
            popovers: Default::default(),
            verify_window: Default::default(),
            model_verify: Default::default(),
            model_verify_rx: None,
            verify_rx: None,
            xsection_moment: Moment::Reflectivity,
            follow_cell: None,
            follow_notice: None,
            warning_popup: None,
            impacts: Default::default(),
            detail_impact: None,
            error_chip: None,
            storm_cells: Vec::new(),
            ui_scale_applied: -1.0,
            ime_shown: false,
            pending_paste: None,
            paste_target: None,
            placefiles: Vec::new(),
            placefile_label_cache: None,
            placefile_window: Default::default(),
            udp_window: Default::default(),
            last_viewport: (1000.0, 800.0),
            tool: MapTool::default(),
            ribbon_mode: RibbonMode::default(),
            measure: Vec::new(),
            storm_tracks: Default::default(),
            output: Default::default(),
            frame_times: Default::default(),
            strokes: Vec::new(),
            draw_color: DRAW_COLORS[0],
            marker_window: Default::default(),
            event_window: Default::default(),
            chase_replay: Default::default(),
            palette_editor: Default::default(),
            digest_window: Default::default(),
            digest_rx: None,
            sounding_window: Default::default(),
            sounding_rx: None,
            raob_rx: None,
            route_window: Default::default(),
            route_rx: None,
            route_exposure: ((u64::MAX, u64::MAX, 0, 0, 0), Vec::new(), Vec::new()),
            previous_sounding_rx: None,
            chase_mode: false,
            spoke_pos: None,
            recent_hits: Vec::new(),
            chase_pos: None,
            chase_track: crate::chaselog::Track::default(),
            warmed_site: None,
            climo_tracks: None,
            climo_rx: None,
            climo_hits: Vec::new(),
            climo_center: None,
            climo_open: false,
            climo_loading: false,
            climo_error: None,
            climo_pending_query: None,
            climo_warn: None,
            climo_warn_rx: None,
            chase_applied: None,
            gps_rx: None,
            sync_tokens: crate::cloud::Tokens::load(),
            sync_state: crate::cloud::SyncState::load(),
            sync_login: None,
            sync_status: String::new(),
            sync_rx: None,
            sync_checked: None,
            share: None,
            peers: std::collections::HashMap::new(),
            share_sent: None,
            goes_times: Vec::new(),
            goes_times_style: None,
            goes_time_idx: None,
            goes_follow_radar: true,
            goes_times_rx: None,
            goes_hour: None,
            glm_fed_prev: None,
            widget_shot_at: None,
            snapshot_push: None,
            rotation_alerted: std::collections::HashMap::new(),
            last_chime: None,
            quiet_queue: std::sync::Mutex::new(quiet_pending),
            rollup: std::sync::Mutex::default(),
            was_quiet: false,
            screenshot_pending: None,
            share_card: None,
            loop_export: None,
            pane_layout: crate::workspace::PaneLayout::default(),
            link_cameras: false,
            link_times: false,
            lock_source_time: false,
            link_site: false,
            link_cursor: false,
            linked_probe: None,
            link_storm: false,
            storm_link_at: None,
            linked_analysis: pane_time::LinkedTimeState::default(),
            mini_loop: false,
            #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
            mini_cam: None,
            #[cfg(not(target_arch = "wasm32"))]
            crash_report: crate::crash::take_report(),
            #[cfg(target_arch = "wasm32")]
            crash_report: None,
            cells_site: None,
            cells_history_site: None,
            cell_trends: std::collections::HashMap::new(),
            fields: crate::render::FieldLayer::DRAW_ORDER
                .iter()
                .map(|&l| (l, FieldState::default()))
                .collect(),
            rotation_minutes: 30,
            hail_minutes: 1440,
            env_cape_ml: false,
            env_srh_km: 3,
            l3grid_site: None,
            derived_key: None,
            snow_hours: 24,
            snow_fetched: None,
            freezing: None,
            freezing_last_fetch: None,
            show_metar: false,
            metars: Vec::new(),
            tafs: Default::default(),
            metar_last_fetch: None,
            metar_bounds: None,
            show_gauges: false,
            gauges: Vec::new(),
            gauge_last_fetch: None,
            gauge_bounds: None,
            gauge_cards: Default::default(),
            gauge_dash: Default::default(),
            active_contours: std::collections::BTreeSet::new(),
            contours: std::collections::HashMap::new(),
            env_model: wxdata::hrrr::Model::Hrrr,
            show_tropical: true,
            tropical: None,
            tropical_last_fetch: None,
            show_outages: true,
            outage_features: Vec::new(),
            outages_last_fetch: None,
            show_cappi: false,
            cappi_alt_km: 3.0,
            cappi_tex: None,
            cappi_key: None,
            hrrr_fcst_hour: 1,
            hrrr_subhourly: false,
            hrrr_fcst_min: 15,
            hrrr_by_timeline: false,
            model_sel: crate::model_browser::Selection::default(),
            refl_model: wxdata::hrrr::Model::Hrrr,
            model_run: None,
            tray_rx: tray_rx_init,
            tray_state: crate::tray::TrayState::default(),
            tray_present: tray_present_init,
            really_quit: false,
            show_storm_reports: false,
            storm_reports: Vec::new(),
            reports_last_fetch: None,
            show_aviation: false,
            aviation_features: Vec::new(),
            aviation_last_fetch: None,
            precip_flag_grid: None,
            precip_flag_gen: 0,
            show_tfr: false,
            boundaries: Default::default(),
            tfr_features: std::collections::HashMap::new(),
            tfr_last_fetch: None,
            tfr_pending: 0,
            afd_open: false,
            afd: None,
            afd_error: None,
            afd_busy: false,
            afd_rx: None,
            tropical_window: ui::tropical_window::TropicalWindow::default(),
            spaghetti: Default::default(),
            tropical_text_rx: None,
            show_range_rings: false,
            show_radar_sites: true,
            show_daynight: false,
            show_blockage: false,
            blockage_tex: None,
            blockage_pending: None,
            blockage_rx,
            blockage_tx,
            show_lowest_tilt: false,
            lowest_tilt_tex: None,
            lowest_tilt_pending: None,
            lowest_tilt_rx,
            lowest_tilt_tx,
            coverage_compare: None,
            coverage_compare_tex: None,
            show_data_health: false,
            gpu_info,
            // Map-first by default on both platforms: the floating chrome covers the common paths,
            // and the full toolbox is one "Advanced" tap away.
            chrome_rect: egui::Rect::EVERYTHING,
            window_w: f32::INFINITY,
            layers_query: String::new(),
            panel_open: false,
            basemap_open: false,
            ribbon_collapsed: false,
            broadcast_logo: None,
            sidebar_focus_search: false,
            show_cheatsheet: false,
            capture_key: false,
            place_query: String::new(),
            place_status: None,
            save_offer: None,
            geocode_nav: false,
            layer_window_open: false,
            pf_icon_tex: std::collections::HashMap::new(),
            pf_icon_rx,
            pf_icon_tx,
            mobile_chrome_hidden: false,
            last_tap: None,
            tap_zoom: None,
            mobile_occlusion: Vec::new(),
            last_gesture_end: None,
            show_spotters: false,
            show_webcams: false,
            webcams: Vec::new(),
            show_fires: false,
            fire_perims: Vec::new(),
            fire_incidents: Vec::new(),
            fire_bounds: None,
            fire_last_fetch: None,
            show_imported_gis: false,
            imported_gis: Vec::new(),
            imported_marks: crate::gis_import::Marks::default(),
            imported_colors: None,
            imported_time: None,
            imported_shown: None,
            show_aqi: false,
            aqi: Vec::new(),
            aqi_bounds: None,
            aqi_last_fetch: None,
            webcam_bounds: None,
            webcam_last_fetch: None,
            show_stations: false,
            stations: Default::default(),
            station_last_poll: None,
            ppef_last_fetch: None,
            dotcam_bounds: None,
            #[cfg(not(target_arch = "wasm32"))]
            nwr: None,
            nwr_pick: String::new(),
            show_dat: false,
            dat_points: Vec::new(),
            dat_tracks: Vec::new(),
            dat_key: None,
            mosaic_sites: Vec::new(),
            mosaic_oldest: None,
            mosaic_bounds: None,
            spotters: Vec::new(),
            spotters_last_fetch: None,
            show_sensors: false,
            sensor_data: None,
            sensor_site: None,
            sensor_last_fetch: None,
            show_hodo: false,
            hodo_data: Vec::new(),
            hodo_history: std::collections::VecDeque::new(),
            hodo_tab: Default::default(),
            forecast_open: false,
            forecast_at: None,
            forecast_state: ui::forecast_window::State::Loading,
            forecast_rx: None,
            forecast_cache: std::collections::HashMap::new(),
            forecast_obs_rx: None,
            forecast_obs_cache: std::collections::HashMap::new(),
            model_series_ui: ui::forecast_window::ModelSeriesUi::default(),
            model_series_state: ui::forecast_window::SeriesState::Idle,
            model_series_rx: None,
            model_series_cache: std::collections::HashMap::new(),
            plume_ui: ui::forecast_window::PlumeUi::default(),
            plume_state: ui::forecast_window::PlumeState::Idle,
            plume_rx: None,
            plume_cache: std::collections::HashMap::new(),
            minute_profile: None,
            minute_key: None,
            rain_detector: Default::default(),
            rain_key: None,
            glm_fed_last: None,
            rules_key: None,
            rules_fired: std::collections::HashMap::new(),
            rain_eta: Vec::new(),
            show_fronts: false,
            fronts: None,
            fronts_last_fetch: None,
            show_glm: false,
            show_strikes: false,
            strikes: std::collections::VecDeque::new(),
            glm: std::sync::Arc::new(std::sync::Mutex::new(wxdata::glm::GlmFeed::new(15))),
            glm_last_poll: None,
            glm_polling: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            show_wind: false,
            wind: None,
            wind_level: wxdata::hrrr::WindLevel::Surface,
            wind_particles: std::collections::HashMap::new(),
            wind_on_gpu: std::env::var("HOOKECHO_CPU_WIND").is_err(),
            wind_uploaded: None,
            wind_fetched: None,
            radar_wind: Default::default(),
            terrain3d: Default::default(),
            yall: Default::default(),
            layer_probe_pin: None,
            wind_last_fetch: None,
            wind_inflight: None,
            wind_last_frame: None,
            wind_dt: 0.0,
            hodo_site: None,
            hodo_last_fetch: None,
            obs_mode: false,
            embed: is_embed(),
            embed_live: false,
            last_input: Instant::now(),
            gesture_live: false,
            #[cfg(not(target_arch = "wasm32"))]
            perf: PerfReadout::new(),
            #[cfg(target_arch = "wasm32")]
            last_posted: None,
            obs_tour: false,
            obs_tour_last: None,
            obs_tour_idx: 0,
            known_warning_ids,
            warnings_seeded: false,
            lightning_alerted: std::collections::HashMap::new(),
            tornado_alerted: None,
            warning_banners: Vec::new(),
            toasts: Vec::new(),
            feed_errors_told: std::collections::HashMap::new(),
            show_alert_panel: false,
            region: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            local_api: Default::default(),
            xsection_pts: Vec::new(),
            xsection: None,
            xsection_tex: None,
            xsection_beam_rise: true,
            marker_icon_tex: Default::default(),
            show_3d: false,
            vol3d: Default::default(),
            vol3d_build: Default::default(),
            vol3d_range: (-30.0, 80.0),
            vol3d_pending: None,
            // `[None; MAX_PANES]` needs `Option<T>: Copy`, which a
            // `Receiver`/`Volume3dUpload` inside it is not; `from_fn` avoids that requirement.
            smooth_vol_key: std::array::from_fn(|_| None),
            smooth_vol_coverage: std::array::from_fn(|_| None),
            iso_mesh: std::array::from_fn(|_| None),
            loop3d: std::array::from_fn(|_| Loop3dCache::default()),
            loop3d_jobs: Loop3dJobs::default(),
            cloud_top: None,
            model_isotherms: None,
            model_isotherms_rx: None,
            cloud_top_rx: None,
            smooth_vol_info: std::array::from_fn(|_| None),
            smooth_vol_range: std::array::from_fn(|_| None),
            vol3d_max_dim,
            smooth_vol_pending: std::array::from_fn(|_| None),
            smooth_vol_dims: std::array::from_fn(|_| None),
            max_texture_dim,
            volume3d_supported,
        };
        // Restore the overlays from last time, assigning rather than only ever switching on: the
        // additive version could never turn a default-on layer off, so unchecking one lasted until
        // the next restart and then came back. `None` is "no run has recorded this yet", where the
        // built-in defaults still stand; a recorded list is the whole truth about every layer.
        //
        // Unknown names (an older build reading a newer file) are skipped rather than treated as
        // an error.
        if let Some(saved) = app.settings.overlays_on.clone() {
            let restore: Vec<OverlayToggle> = saved
                .iter()
                .filter_map(|s| OverlayToggle::from_slug(s))
                .collect();
            for t in OverlayToggle::ALL {
                if t.session_only() {
                    continue;
                }
                *app.overlay_flag(t) = restore.contains(&t);
            }
            // Whatever the outcome, the overlay set now differs from the one the constructor built,
            // so the derived features have to be rebuilt from it once.
            app.rebuild_overlays();
        }
        // The model contours that were on (by token; an unknown one from a newer build is
        // skipped). They fetch on the first frame like any newly picked contour.
        app.active_contours = app
            .settings
            .contours_on
            .iter()
            .filter_map(|t| ContourKind::from_token(t))
            .collect();
        if let Some(sel) = crate::model_browser::Selection::from_slug(&app.settings.model_pick) {
            app.model_sel = sel;
            app.apply_model_engine(sel);
        }
        app.palettes.reload(&app.settings.palette_paths());
        app.reload_imported_gis();
        app.apply_goto_env();
        app.drain_goto_file();
        #[cfg(target_arch = "wasm32")]
        app.apply_goto_hash();
        #[cfg(target_arch = "wasm32")]
        if opened_on_default {
            app.locate_by_ip(&cc.egui_ctx.clone());
        }
        crate::platform::set_background_alerts(app.settings.background_alerts);
        crate::platform::set_battery_saver(app.settings.battery_saver);
        // The broker is publish-only and reconnects on its own, so it starts here and is never
        // stopped; changing the setting takes a restart, same as the tray.
        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        crate::mqtt::spawn(&app.settings, true);
        // Point the speech path at Piper before anything can speak.
        #[cfg(not(target_arch = "wasm32"))]
        crate::speech::set_piper(&app.settings.piper_path, &app.settings.piper_voice);
        // Not on the web: this is a burst of six fetches, and on the one thread a browser gives
        // us they queue ahead of the radar the visitor actually came for. The periodic refresh in
        // `update` picks them up a moment later, once there is radar on screen.
        #[cfg(not(target_arch = "wasm32"))]
        app.fetch_overlays(&cc.egui_ctx.clone());
        // The receiver on the dash does not need a click every morning.
        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        if app.settings.gps_autoconnect {
            app.connect_gpsd();
        }
        app
    }
}
