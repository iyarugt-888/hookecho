//! Preferences → App: the section list and each section's rows (display, location, data age, share, backup, help).
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// The app's own commands — the ones that aren't a layer, product, tool or window, and so
    /// have no place in the action registry: view toggles, chase, capture, settings bundles.
    /// Compact preference groups; details stay collapsed until needed.
    pub(crate) fn app_rows(&mut self, ui: &mut egui::Ui) {
        use crate::ui::a11y::Named;
        use crate::ui::style::toggle;
        let metric = self.metric();
        use egui_phosphor::regular as ph;
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.spacing_mut().interact_size.y = 30.0;
        let section_id = egui::Id::new("preferences_section");
        let section = ui
            .ctx()
            .data_mut(|d| d.get_temp::<&'static str>(section_id));
        if section.is_none() {
            for (title, description, icon) in [
                ("Display", "Map visibility and streaming", ph::MONITOR),
                ("Location", "GPS, route recording and sharing", ph::MAP_PIN),
                ("Data age", "When radar and layers count as old", ph::CLOCK),
                #[cfg(not(target_arch = "wasm32"))]
                (
                    "Weather radio",
                    "Listen to local weather broadcasts",
                    ph::RADIO,
                ),
                ("Share", "Share this view and export images", ph::EXPORT),
                ("Backup", "Save or restore your settings", ph::FLOPPY_DISK),
                ("Help", "Guided tour, setup and support", ph::QUESTION),
            ] {
                let width = ui.available_width();
                let response = ui
                    .add_sized(
                        egui::vec2(width, 58.0),
                        egui::Button::new("").corner_radius(0),
                    )
                    .named(title)
                    .on_hover_text(description);
                let rect = response.rect;
                let painter = ui.painter();
                painter.text(
                    rect.left_center() + egui::vec2(18.0, 0.0),
                    egui::Align2::CENTER_CENTER,
                    icon,
                    egui::FontId::proportional(20.0),
                    crate::theme::accent(self.settings.theme),
                );
                painter.text(
                    rect.left_top() + egui::vec2(40.0, 12.0),
                    egui::Align2::LEFT_TOP,
                    title,
                    egui::FontId::proportional(14.0),
                    ui.visuals().text_color(),
                );
                painter.text(
                    rect.left_top() + egui::vec2(40.0, 33.0),
                    egui::Align2::LEFT_TOP,
                    description,
                    egui::FontId::proportional(10.0),
                    ui.visuals().weak_text_color(),
                );
                painter.text(
                    rect.right_center() - egui::vec2(14.0, 0.0),
                    egui::Align2::CENTER_CENTER,
                    ph::CARET_RIGHT,
                    egui::FontId::proportional(14.0),
                    ui.visuals().weak_text_color(),
                );
                if response.clicked() {
                    ui.ctx().data_mut(|d| d.insert_temp(section_id, title));
                }
            }
            return;
        }
        if section == Some("Display") {
            ui.scope(|ui| {
                {
                    let v = &mut self.views[self.active];
                    let mut on = v.basemap != crate::tiles::BasemapStyle::None;
                    if toggle(ui, &mut on, "Basemap").changed() {
                        v.basemap = if on {
                            crate::tiles::BasemapStyle::default()
                        } else {
                            crate::tiles::BasemapStyle::None
                        };
                    }
                    toggle(ui, &mut v.show_radar, "Radar");
                    toggle(ui, &mut v.show_legend, "Color scale");
                }
                if toggle(ui, &mut self.obs_mode, "Streaming mode")
                    .on_hover_text("F8 · Hide panels for a clean streaming view")
                    .changed()
                    && !self.obs_mode
                {
                    self.obs_tour = false;
                }
                if toggle(ui, &mut self.obs_tour, "Tour active warnings")
                    .on_hover_text("F9 · Visit each active warning every 12 seconds")
                    .changed()
                {
                    self.obs_tour_last = None;
                    if self.obs_tour {
                        self.obs_mode = true;
                    }
                }
                self.streaming_rows(ui);

                if ui
                    .add_enabled(
                        !self.measure.is_empty(),
                        egui::Button::new("Clear measurements"),
                    )
                    .clicked()
                {
                    self.measure.clear();
                }
            });
        }

        if section == Some("Data age") {
            self.data_age_rows(ui);
        }

        if section == Some("Location") {
            ui.scope(|ui| {
                if toggle(ui, &mut self.chase_mode, "Follow my location").changed()
                    && !self.chase_mode
                {
                    self.chase_applied = None;
                }
                if self.chase_mode {
                    match self
                        .chase_pos
                        .and_then(|(lon, lat)| crate::geo::nearest_site_id(lon, lat))
                    {
                        Some(s) => ui.weak(format!("nearest radar: {s}")),
                        None => ui.weak("pick a location with Tool: Set chase location"),
                    };
                }
                toggle(ui, &mut self.settings.chase_log, "Record my route").on_hover_text(
                    "Record a breadcrumb track of your GPS fixes, to draw on the map and save \
                     as GPX. In memory until you save it; nothing is uploaded.",
                );
                if self.settings.chase_log && !self.chase_track.points.is_empty() {
                    ui.weak(format!(
                        "{} points · {}",
                        self.chase_track.points.len(),
                        crate::geo::fmt_distance(self.chase_track.km(), metric, 0)
                    ));
                    ui.horizontal(|ui| {
                        if ui
                            .button("\u{1f4cd} Mark")
                            .on_hover_text("Name this spot in the track (saved into the GPX)")
                            .clicked()
                        {
                            let n = self.chase_track.waypoints.len() + 1;
                            self.chase_track.mark(format!("Mark {n}"));
                        }
                        if ui.button("Save GPX…").clicked() {
                            let gpx = self.chase_track.to_gpx();
                            match crate::dialog::save_bytes("chase.gpx", "gpx", gpx.as_bytes()) {
                                crate::dialog::Saved::Where(w) => {
                                    self.toast(ToastKind::Success, format!("Saved to {w}"))
                                }
                                crate::dialog::Saved::Failed(e) => {
                                    self.toast(ToastKind::Error, format!("GPX save failed: {e}"))
                                }
                                crate::dialog::Saved::Cancelled => {}
                            }
                        }
                        if ui
                            .button("Clear")
                            .on_hover_text("Forget the track so far")
                            .clicked()
                        {
                            self.chase_track.clear();
                        }
                    });
                }
                // Desktop streams from a local gpsd; Android polls the system LocationManager over
                // JNI (see platform.rs); the web watches the browser's own Geolocation. All three
                // feed the same `gps_rx` channel.
                if self.gps_rx.is_none() {
                    let (label, tip) = if cfg!(target_os = "android") {
                        (
                            "Enable location…",
                            "Follow your device's position (asks for the location permission)",
                        )
                    } else if cfg!(target_arch = "wasm32") {
                        (
                            "Enable location…",
                            "Follow your position (asks the browser for the location permission)",
                        )
                    } else {
                        (
                            "Connect GPS (gpsd)",
                            "Stream your live position from a local gpsd on :2947",
                        )
                    };
                    if ui.button(label).on_hover_text(tip).clicked() {
                        let rx = if cfg!(target_os = "android") {
                            crate::platform::start_location()
                        } else {
                            crate::gps::spawn()
                        };
                        match rx {
                            Some(rx) => {
                                self.gps_rx = Some(rx);
                                self.chase_mode = true;
                            }
                            None => log::warn!("no position source available"),
                        }
                    }
                } else {
                    // getLastKnownLocation is null until the first fix lands (cold start,
                    // indoors, or permission still pending) — say so rather than look dead.
                    if self.chase_pos.is_some() {
                        ui.weak("📡 GPS connected");
                    } else {
                        ui.weak("📡 waiting for GPS fix…");
                    }
                    if ui.button("Disconnect GPS").clicked() {
                        self.gps_rx = None;
                        // A deliberate disconnect is also a "not at launch, either".
                        self.settings.gps_autoconnect = false;
                    }
                }
                // Desktop only: gpsd is a daemon that is either there or not, so connecting at
                // launch costs nothing and asks nobody. The permission platforms keep the click.
                #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
                {
                    let was = self.settings.gps_autoconnect;
                    toggle(
                        ui,
                        &mut self.settings.gps_autoconnect,
                        "Connect GPS at launch",
                    )
                    .on_hover_text("Connect to the local gpsd every time HookEcho starts");
                    if self.settings.gps_autoconnect && !was && self.gps_rx.is_none() {
                        self.connect_gpsd();
                    }
                }
                // Position sharing: the phone in the field and the desktop at home showing each other
                // as dots on the same radar. LAN needs no setup; the relay covers cellular.
                toggle(ui, &mut self.settings.share_position, "Share my position").on_hover_text(
                    "Broadcast your GPS fix to other HookEcho instances, and show theirs",
                );
                if self.settings.share_position {
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.settings.share_name)
                                .hint_text("me")
                                .desired_width(120.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Relay");
                        ui.add(
                        egui::TextEdit::singleline(&mut self.settings.share_relay)
                            .hint_text("https://… (optional)")
                            .desired_width(180.0),
                    )
                    .on_hover_text(
                        "HTTP endpoint you host: POST a position, GET the list. Leave empty for \
                         same-network sharing only. The endpoint sees your live position.",
                    );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Stream");
                        ui.add(
                        egui::TextEdit::singleline(&mut self.settings.share_video_url)
                            .hint_text("https://… (optional)")
                            .desired_width(180.0),
                    )
                    .on_hover_text(
                        "A live-video URL published with your dot, so partners can click it and \
                         watch. Direct HLS/MJPEG plays in-app; YouTube and Twitch open a browser.",
                    );
                    });
                    match self.peers.len() {
                        0 => ui.weak("no one else sharing yet"),
                        n => ui.weak(format!("👥 {n} sharing")),
                    };
                }
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if section == Some("Weather radio") {
                self.nwr_rows(ui);
            }
        }
        if section == Some("Share") {
            ui.scope(|ui| {
                if ui.button("Copy link to this view").clicked() {
                    self.apply_palette(PaletteAction::CopyViewLink, ui.ctx());
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    if ui.button("Save screenshot…").clicked() {
                        if let Some(path) = crate::dialog::save_path("hookecho.png", "png") {
                            self.request_capture(ui.ctx(), ShotDest::File(path));
                        }
                    }
                    if ui.button("Copy view to clipboard").clicked() {
                        self.request_capture(ui.ctx(), ShotDest::Clipboard);
                    }
                    toggle(ui, &mut self.settings.share_card, "Caption shared images")
                        .on_hover_text(
                            "Stamp the site, product, valid time and source onto saved and copied \
                     images, so a screenshot still says what it is once it leaves here",
                        );
                    toggle(ui, &mut self.settings.loop_real_timing, "Real scan timing")
                        .on_hover_text(
                            "Hold each exported frame for the real time to the next scan, scaled \
                             to the playback speed, instead of all alike",
                        );
                    if ui
                        .add_enabled(
                            self.loop_export.is_none(),
                            egui::Button::new("Export loop (GIF)…"),
                        )
                        .on_hover_text("Capture the archive timeline as a looping animation")
                        .clicked()
                    {
                        self.start_loop_export(crate::loopexport::LoopFormat::Gif);
                    }
                    // MP4 export shells out to the `ffmpeg` CLI, which isn't present on Android; GIF
                    // export (pure Rust) stays. Hide the MP4 item there rather than fail on click.
                    if !cfg!(target_os = "android")
                        && ui
                            .add_enabled(
                                self.loop_export.is_none(),
                                egui::Button::new("Export loop (MP4)…"),
                            )
                            .on_hover_text(
                                "Capture the archive timeline as an MP4 (requires ffmpeg)",
                            )
                            .clicked()
                    {
                        self.start_loop_export(crate::loopexport::LoopFormat::Mp4);
                    }
                }
            });
        }

        if section == Some("Share") {
            ui.scope(|ui| {
                if ui
                    .button("Save case…")
                    .on_hover_text(
                        "A small file that reopens this analysis anywhere: the panes and their \
                         products, this moment with an hour's replay around it, and your \
                         bookmarks, markers, zones and drawings. Radar data is refetched.",
                    )
                    .clicked()
                {
                    self.export_case();
                }
                if ui
                    .button("Open case…")
                    .on_hover_text(
                        "Reopen a saved case; its annotations and bookmarks are added to yours",
                    )
                    .clicked()
                {
                    self.import_case();
                }
                if ui
                    .button("Export analysis…")
                    .on_hover_text(
                        "One ZIP for other tools: the map as PNG, the case, annotations as \
                         GeoJSON, which radar volumes were used, the detections, and any open \
                         probes (region, profile, time series, cross-section) as CSV",
                    )
                    .clicked()
                {
                    self.export_analysis(ui.ctx());
                }
                if ui
                    .button("Export grid (GeoTIFF)…")
                    .on_hover_text(
                        "The top gridded layer on this pane — MRMS, a derived radar field such as \
                         VIL or MEHS, or a model field — as a float32 GeoTIFF for QGIS, ArcGIS \
                         or GDAL",
                    )
                    .clicked()
                {
                    self.export_geotiff();
                }
                if ui
                    .button("Export grid (NetCDF)…")
                    .on_hover_text(
                        "The same grid as CF-1.8 NetCDF, with lat/lon coordinates and its valid \
                         time — for xarray, Panoply, NCL or MATLAB",
                    )
                    .clicked()
                {
                    self.export_netcdf();
                }
                if ui
                    .button("Export volume (CF/Radial)…")
                    .on_hover_text(
                        "This pane's radar volume, every tilt and moment, as CF/Radial 1.4 \
                         NetCDF for Py-ART, LROSE or wradlib. The app's binned 8-bit data, as \
                         displayed — not the raw Level II words.",
                    )
                    .clicked()
                {
                    self.export_cfradial();
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let port = self.settings.local_api_port;
                    if ui
                        .checkbox(
                            &mut self.settings.local_api,
                            format!("Local API on 127.0.0.1:{port}"),
                        )
                        .on_hover_text(
                            "Serve what this window shows to other programs on this computer — \
                             panes, detections, warnings, feed health, a sample at a point, a \
                             screenshot and a live event stream — at \
                             http://127.0.0.1:{port}/api/v1. Only this computer can reach it.",
                        )
                        .changed()
                    {
                        self.settings.save();
                    }
                }
            });
        }

        if section == Some("Backup") {
            ui.scope(|ui| {
                if ui
                    .button("Save settings backup…")
                    .on_hover_text("Save settings + color tables to a portable bundle")
                    .clicked()
                {
                    self.export_settings_bundle();
                }
                if ui
                    .button("Restore settings backup…")
                    .on_hover_text("Load a settings bundle from another machine")
                    .clicked()
                {
                    self.import_settings_bundle();
                }
                if ui
                    .button("Export diagnostics…")
                    .on_hover_text(
                        "Version, renderer, source health, recent warnings and cache size — \
                         for a bug report. No location history, keys or tokens.",
                    )
                    .clicked()
                {
                    self.export_diagnostics_bundle();
                }
            });
        }

        if section == Some("Help") {
            // Self-update from the CI build (`crate::self_update`).
            ui.horizontal(|ui| {
                toggle(
                    ui,
                    &mut self.settings.check_builds,
                    "Check for new builds at launch",
                );
            });
            ui.horizontal(|ui| {
                let state = crate::self_update::state();
                let note = match &state {
                    crate::self_update::State::DevBuild => {
                        "This is a local build: it does not update itself.".to_string()
                    }
                    crate::self_update::State::Unsupported => {
                        "This platform does not update itself.".to_string()
                    }
                    crate::self_update::State::Checking => "Checking…".to_string(),
                    crate::self_update::State::UpToDate => format!(
                        "Build #{} is the newest.",
                        crate::self_update::local_build().unwrap_or(0)
                    ),
                    crate::self_update::State::Failed(e) => e.clone(),
                    crate::self_update::State::Available(i)
                    | crate::self_update::State::Ready { info: i, .. } => {
                        format!("Build #{} is available.", i.build)
                    }
                    crate::self_update::State::Dismissed(b) => format!("Build #{b} is available."),
                    _ => String::new(),
                };
                if ui.button("Check now").clicked() {
                    self.check_for_build(ui.ctx());
                }
                if let crate::self_update::State::Dismissed(_) = state {
                    if ui.button("Show").clicked() {
                        self.check_for_build(ui.ctx());
                    }
                }
                ui.weak(note);
            });
            ui.scope(|ui| {
                ui.hyperlink_to(
                    "HookEcho help & feedback",
                    "https://github.com/d4vid87/hookecho",
                );
                if ui.button("Set up again…").clicked() {
                    self.firstrun.start();
                }
                if ui.button("Take the tour…").clicked() {
                    self.tour.start();
                }
                #[cfg(not(target_arch = "wasm32"))]
                if ui.button("Exit HookEcho").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        }
    }
}
