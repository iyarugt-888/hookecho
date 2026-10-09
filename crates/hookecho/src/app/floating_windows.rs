//! The frame's floating windows: site picker, search, dialogs and every tool window, drawn after
//! the panels and before the map. Moved out of `ui_frame` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    pub(crate) fn floating_windows(
        &mut self,
        ctx: &egui::Context,
        root: &mut egui::Ui,
        dock_layout: bool,
    ) {
        // Floating windows.
        if let Some(dialog) = &mut self.site_dialog {
            let keep = ui::site_dialog::show(
                ctx,
                dialog,
                &mut self.views[self.active],
                &mut self.settings,
                &mut self.drawer,
            );
            if !keep {
                self.site_dialog = None;
            }
        }
        self.settings_frame(ctx, dock_layout);
        let pf_status: Vec<ui::placefile_window::PlacefileStatus> = self
            .placefiles
            .iter()
            .map(|lp| ui::placefile_window::PlacefileStatus {
                url: lp.url.clone(),
                loaded: lp.loaded,
                items: lp.pf.items.len(),
                title: lp.pf.title.clone(),
                error: lp.error.clone(),
            })
            .collect();
        self.placefile_window
            .show(ctx, &mut self.settings, &pf_status, &mut self.drawer);
        let active = self.active;
        let before = self.views[active].user_product.clone();
        let column_before = self.views[active].column_product.clone();
        let column_status = match self.column_status(active) {
            Some(column_product::ColumnStatus::Unavailable(why)) => Some(why),
            _ => None,
        };
        let view = &mut self.views[active];
        self.udp_window.show(
            ctx,
            &mut self.settings,
            &mut self.drawer,
            &mut view.user_product,
            &mut view.column_product,
            column_status,
        );
        if self.views[active].column_product != column_before {
            // The product and its field layer go on and off together.
            let on = self.views[active].column_product.is_some();
            self.set_field(crate::render::FieldLayer::UserColumn, on);
        }
        if self.views[active].user_product != before {
            let v = &mut self.views[active];
            v.product_range = None;
            v.product_moment = v.moment;
            self.pane_shown.remove(&active);
        }
        // Names come from the action registry, so a layer reads the same here as in the layers
        // panel — the enum's Debug spelling ("Mrms") is not a label.
        let names: std::collections::HashMap<crate::render::FieldLayer, String> =
            if self.layer_window_open {
                self.palette_entries()
                    .iter()
                    .filter_map(|e| match e.action {
                        PaletteAction::ToggleField(l) => Some((l, e.label.clone())),
                        _ => None,
                    })
                    .collect()
            } else {
                Default::default()
            };
        let active_fields: Vec<(crate::render::FieldLayer, String)> =
            crate::render::FieldLayer::DRAW_ORDER
                .into_iter()
                .filter(|l| self.field_wanted(*l))
                .map(|l| {
                    let name = names.get(&l).cloned().unwrap_or_else(|| format!("{l:?}"));
                    (l, name)
                })
                .collect();
        // The imported GIS layers (ROADMAP_PARITY M4.1): one row each, and the attributes,
        // legend and time count of the one being edited.
        let rows: Vec<ui::layer_window::GisRow> = self
            .settings
            .gis_layers
            .iter()
            .map(|c| {
                let l = self.gis_loaded(c.id);
                ui::layer_window::GisRow {
                    id: c.id,
                    features: l.map_or(0, |l| l.len()),
                    error: l.and_then(|l| l.error.clone()),
                }
            })
            .collect();
        let edited = self
            .gis_selected
            .filter(|id| self.settings.gis_layer(*id).is_some())
            .or_else(|| self.settings.gis_layers.last().map(|l| l.id));
        let edited_layer = edited.and_then(|id| self.gis_loaded(id));
        let label_keys = match edited_layer {
            Some(l) if self.layer_window_open => crate::gis_import::label_keys(&l.marks),
            _ => Vec::new(),
        };
        let legend = edited_layer.and_then(|l| l.colors.as_ref().map(|(_, _, g)| g.clone()));
        let time_count = edited_layer
            .and_then(|l| l.shown.as_ref())
            .map(|m| (m.iter().filter(|&&s| s).count(), m.len()));
        let mut selected = edited;
        let imported = ui::layer_window::Imported {
            rows: &rows,
            keys: &label_keys,
            legend: legend.as_ref(),
            time_count,
            filter_error: edited_layer.and_then(|l| l.filter_error.clone()),
        };
        let outcome = ui::layer_window::show(
            ctx,
            &mut self.layer_window_open,
            &mut self.settings,
            &active_fields,
            &imported,
            &mut selected,
            &mut self.drawer,
        );
        self.gis_selected = selected;
        if let Some(id) = outcome.remove {
            self.remove_gis(id);
        }
        if let Some(id) = outcome.zoom {
            self.zoom_to_gis(Some(id));
        }
        if let Some(id) = outcome.export {
            self.export_gis_layer(id);
        }
        if let Some(id) = outcome.table {
            self.gis_table = Some(gis_layers::GisTable::of(id));
        }
        self.gis_table_window(ctx);
        if outcome.changed {
            // Imported polygon colors are applied while assembling `self.overlays`, so style
            // edits need a rebuild; placefile/field opacity changes also remain safely covered.
            self.rebuild_overlays();
        }
        // The edge's geo-IP fix, if it beat the user to it: move the default view to the radar
        // that covers them. Skipped once they have panned or picked a site themselves.
        while let Ok((lon, lat)) = self.ipgeo_rx.try_recv() {
            if self.geocode_nav || self.views.len() > 1 {
                continue;
            }
            if let Some(site) = crate::geo::nearest_site_id(lon, lat) {
                self.goto_view(&site, lon, lat, 8.0, None);
            }
        }
        // Drain geocode results: the search pill navigates, the marker window adds a marker.
        while let Ok(res) = self.geocode_rx.try_recv() {
            if std::mem::take(&mut self.geocode_nav) {
                match res {
                    Ok((name, lat, lon)) => {
                        let cam = &mut self.views[self.active].camera;
                        cam.center = crate::render::mercator::lonlat_to_world(lon, lat);
                        cam.zoom = cam.zoom.max(9.0);
                        self.save_offer = Some((
                            short_place_name(&name).to_string(),
                            lat,
                            lon,
                            Instant::now(),
                        ));
                        self.place_status = Some((name, Instant::now()));
                        self.place_query.clear();
                    }
                    Err(e) => self.place_status = Some((e, Instant::now())),
                }
                continue;
            }
            self.marker_window.searching = false;
            match res {
                Ok((name, lat, lon)) => {
                    self.settings.markers.push(crate::settings::Marker {
                        id: crate::settings::new_marker_id(),
                        name: name.clone(),
                        lat,
                        lon,
                        icon: None,
                        alert_radius_mi: crate::settings::default_alert_radius_mi(),
                        video_url: String::new(),
                        home: false,
                    });
                    self.settings.save();
                    // Fly the active pane to the new marker (same idiom as the alert panel).
                    let cam = &mut self.views[self.active].camera;
                    cam.center = crate::render::mercator::lonlat_to_world(lon, lat);
                    cam.zoom = cam.zoom.max(9.0);
                    self.marker_window.status = Some(format!("Added \"{name}\""));
                    self.marker_window.query.clear();
                }
                Err(e) => self.marker_window.status = Some(e),
            }
        }
        let metric = self.metric();
        let query = self.marker_window.show(
            ctx,
            &mut self.settings,
            &self.marker_icon_tex,
            &mut self.drawer,
            metric,
        );
        // The map popup indexes into the same list: a delete above it leaves it describing the
        // wrong marker, which is the one way this UI can lie about which place you are editing.
        if let (Some(gone), Some(open)) = (self.marker_window.removed, self.marker_popup) {
            self.marker_popup = match gone.cmp(&open) {
                std::cmp::Ordering::Less => Some(open - 1),
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(open),
            };
        }
        if let Some(query) = query {
            self.marker_window.searching = true;
            self.marker_window.status = Some("Searching…".into());
            let http = self.http.clone();
            let tx = self.geocode_tx.clone();
            let ctx2 = ctx.clone();
            self.spawner.spawn(async move {
                let _ = tx.send(wxdata::geocode::search(&http, &query).await);
                ctx2.request_repaint();
            });
        }
        // Drain chase-pack worker outcomes; drop the download state once every tile is accounted for.
        if let Some(pack) = &mut self.chasepack {
            while let Ok((ok, n)) = pack.rx.try_recv() {
                pack.done += 1;
                pack.bytes += n;
                if !ok {
                    pack.errors += 1;
                }
            }
            if pack.done >= pack.total {
                self.chasepack = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
        self.palette_editor
            .show(ctx, &mut self.settings, &self.palettes, &mut self.drawer);
        // Storm digest: poll a pending model result, then render + handle Generate.
        if let Some(rx) = &self.digest_rx {
            if let Ok(res) = rx.try_recv() {
                self.digest_window.busy = false;
                self.digest_rx = None;
                match res {
                    Ok(text) => {
                        self.digest_window.text = text;
                        self.digest_window.enhanced = true;
                    }
                    Err(e) => {
                        log::warn!("digest enhancement failed: {e}");
                        // Shown, not only logged: a wrong key otherwise just looks like the
                        // template every time.
                        self.digest_window.error = Some(e);
                    }
                }
            }
        }
        if let Some(ui::digest_window::DigestAction::Generate) =
            self.digest_window.show(ctx, &mut self.drawer)
        {
            self.generate_digest(ctx);
        }
        // Live station cards. Video keeps arriving between input events, so a playing card asks
        // for the next frame itself rather than waiting for the idle heartbeat.
        {
            // A card opened before the camera catalog landed gets its camera as soon as it does.
            let (rt, http) = (self.spawner.clone(), self.http.clone());
            self.stations.pair_cameras(&rt, &http, ctx);
            let tz = self.active_tz();
            if self.stations.show_cards(ctx, tz) {
                ctx.request_repaint_after(std::time::Duration::from_millis(33));
            }
        }
        // River-gauge cards.
        {
            let (rt, http) = (self.spawner.clone(), self.http.clone());
            let tz = self.active_tz();
            for action in self.gauge_cards.show(ctx, tz, &rt, &http) {
                self.gauge_card_action(action, ctx);
            }
            // A stage a card put on the map: move the map to its flooding when it lands.
            if let Some(bounds) = self.gauge_cards.impact.take_fit() {
                self.follow_cell = None;
                let v = &mut self.views[self.active];
                v.camera = crate::ui::flood_impact::fit_camera(
                    &v.camera,
                    bounds,
                    ctx.content_rect().size(),
                );
            }
            // The flood-gauge dashboard, as a window, in the layouts without the workstation's
            // docks (the workstation draws it as a tool window).
            if !self.workstation_chrome() && self.gauge_dash.window_open {
                let t = self.ws_tokens();
                let mut open = true;
                let mut outs = Vec::new();
                let zoomed_out = self.gauges_zoomed_out();
                egui::Window::new("Flood gauges")
                    .open(&mut open)
                    .default_size([440.0, 640.0])
                    .resizable(true)
                    .show(ctx, |ui| {
                        crate::ui::workstation::style_scope(ui, &t);
                        egui::ScrollArea::vertical()
                            .id_salt("gauge_dash_window")
                            .max_height(ctx.content_rect().height() * 0.8)
                            .show(ui, |ui| {
                                outs = crate::ui::gauge_dashboard::body(
                                    ui,
                                    &t,
                                    &mut self.gauge_dash,
                                    &self.gauges,
                                    &mut self.gauge_cards,
                                    crate::ui::gauge_dashboard::Env {
                                        tz,
                                        spawner: &rt,
                                        http: &http,
                                        layer_on: self.show_gauges,
                                        zoomed_out,
                                        max_table_h: 260.0,
                                    },
                                );
                            });
                    });
                self.gauge_dash.window_open = open;
                for o in outs {
                    self.gauge_dash_out(o, ctx);
                }
            }
        }
        // Area Forecast Discussion: poll the async fetch, then render the text window.
        if let Some(rx) = &self.afd_rx {
            if let Ok(res) = rx.try_recv() {
                self.afd_busy = false;
                self.afd_rx = None;
                match res {
                    Ok(afd) => self.afd = Some(afd),
                    Err(e) => self.afd_error = Some(e),
                }
            }
        }
        if self.afd_open {
            let refresh = ui::afd_window::show(
                ctx,
                &mut self.afd_open,
                self.afd.as_ref(),
                self.afd_busy,
                self.afd_error.as_deref(),
                &mut self.drawer,
            );
            if refresh {
                self.fetch_afd();
            }
        }
        // NHC text products: poll the fetch, then draw the reader.
        if let Some(rx) = &self.tropical_text_rx {
            if let Ok(res) = rx.try_recv() {
                self.tropical_window.busy = false;
                self.tropical_text_rx = None;
                match res {
                    Ok(a) => {
                        self.tropical_window.text = Some(a);
                        self.tropical_window.error = None;
                    }
                    Err(e) => {
                        self.tropical_window.text = None;
                        self.tropical_window.error = Some(e);
                    }
                }
            }
        }
        if std::mem::take(&mut self.spaghetti.open_window) {
            self.tropical_window.open = true;
            self.tropical_window.tab = ui::tropical_window::Tab::Models;
        }
        if self.tropical_window.open {
            let storms = self
                .tropical
                .as_ref()
                .map(|t| t.storms.clone())
                .unwrap_or_default();
            let tz = self.active_tz();
            let out = ui::tropical_window::show(
                &mut self.tropical_window,
                ctx,
                &storms,
                &mut self.spaghetti,
                tz,
                &mut self.drawer,
            );
            if let Some((id, product)) = out.fetch {
                self.fetch_tropical_text(&id, product);
            }
            if let Some((lat, lon)) = out.center {
                self.follow_cell = None;
                self.views[self.active].camera.center =
                    crate::render::mercator::lonlat_to_world(lon, lat);
            }
            if out.refresh_models {
                let (rt, http) = (self.spawner.clone(), self.http.clone());
                self.spaghetti.fetch(&rt, &http, ctx);
            }
        }
        // Point sounding: poll the async fetch, then render the Skew-T / hodograph.
        if let Some(rx) = &self.sounding_rx {
            if let Ok(res) = rx.try_recv() {
                self.sounding_window.busy = false;
                self.sounding_rx = None;
                // A new profile makes the previous run's stale: fetch it again if it is shown.
                self.sounding_window.previous = None;
                self.sounding_window.previous_error = None;
                self.previous_sounding_rx = None;
                self.sounding_window.want_previous = self.sounding_window.show_previous;
                match res {
                    Ok(s) => self.sounding_window.sounding = Some(s),
                    Err(e) => {
                        self.sounding_window.error = Some(e);
                    }
                }
            }
        }
        if let Some(rx) = &self.previous_sounding_rx {
            if let Ok(res) = rx.try_recv() {
                self.previous_sounding_rx = None;
                match res {
                    Ok(s) => self.sounding_window.previous = Some(s),
                    Err(e) => self.sounding_window.previous_error = Some(e),
                }
            }
        }
        if std::mem::take(&mut self.sounding_window.want_previous) {
            self.fetch_previous_sounding();
        }
        if let Some(rx) = &self.raob_rx {
            if let Ok(res) = rx.try_recv() {
                self.raob_rx = None;
                match res {
                    Ok(s) => self.sounding_window.observed = Some(s),
                    Err(e) => self.sounding_window.observed_error = Some(e),
                }
            }
        }
        let tz = self.active_tz();
        // The workstation draws the sounding as a dock tab (`app::chrome::dock::sounding`).
        if !self.workstation_chrome() {
            self.sounding_window.show(ctx, tz, &mut self.drawer);
        }
        self.route_frame(ctx);
        if std::mem::take(&mut self.sounding_window.refetch) {
            self.refetch_sounding();
        }
        // Warning verification lab: drain the query, then draw and act on its clicks.
        if let Some(rx) = &self.verify_rx {
            if let Ok(res) = rx.try_recv() {
                self.verify_window.busy = false;
                self.verify_rx = None;
                match res {
                    Ok(v) => self.verify_window.data = Some(v),
                    Err(e) => {
                        self.verify_window.error = Some(format!("verification unavailable: {e}"));
                    }
                }
            }
        }
        let tz = self.active_tz();
        let vact = self.verify_window.show(ctx, tz, &mut self.drawer);
        if vact.refresh {
            self.verify_window.data = None;
            self.fetch_verify();
        }
        // Model verification: drain the scoring, then draw and act on the window.
        if let Some(rx) = &self.model_verify_rx {
            if let Ok(res) = rx.try_recv() {
                self.model_verify.busy = false;
                self.model_verify_rx = None;
                match res {
                    Ok(scored) => self.model_verify.results = Some(scored),
                    Err(e) => {
                        self.model_verify.error = Some(format!("verification unavailable: {e}"));
                    }
                }
            }
        }
        let unit = self.settings.temp_unit;
        let mact = self
            .model_verify
            .show(ctx, self.active_tz(), unit, &mut self.drawer);
        if mact.run {
            self.fetch_model_verify();
        }
        if let Some((lon, lat, time)) = vact.goto {
            let site = self.views[self.active].site.clone().unwrap_or_default();
            self.goto_view(&site, lon, lat, 8.5, time);
        }
        // Point forecast: drain the fetch, cache the win, then draw.
        if let Some((key, rx)) = &self.forecast_rx {
            if let Ok(res) = rx.try_recv() {
                let key = *key;
                self.forecast_rx = None;
                self.forecast_state = match res {
                    Ok(f) => {
                        self.forecast_cache.insert(key, (Instant::now(), f.clone()));
                        ui::forecast_window::State::Ready(Box::new(f))
                    }
                    Err(e) => ui::forecast_window::State::Failed(e),
                };
            }
        }
        if let Some((key, rx)) = &self.forecast_obs_rx {
            if let Ok((station, ob)) = rx.try_recv() {
                let key = *key;
                self.forecast_obs_rx = None;
                self.forecast_obs_cache
                    .insert(key, (Instant::now(), station, ob));
            }
        }
        // Model-forecast meteogram: drain the fetch, cache it under (point, model, field, period).
        if let Some((key, rx)) = &self.model_series_rx {
            if let Ok(res) = rx.try_recv() {
                let key = *key;
                self.model_series_rx = None;
                self.model_series_state = match res {
                    Ok(series) => {
                        self.model_series_cache
                            .insert(key, (Instant::now(), series.clone()));
                        ui::forecast_window::SeriesState::Ready(series)
                    }
                    Err(e) => ui::forecast_window::SeriesState::Failed(e),
                };
            }
        }
        // Ensemble plume: drain the fetch, cache it under (point, field, period).
        if let Some((key, rx)) = &self.plume_rx {
            if let Ok(res) = rx.try_recv() {
                let key = *key;
                self.plume_rx = None;
                self.plume_state = match res {
                    Ok(points) => {
                        self.plume_cache
                            .insert(key, (Instant::now(), points.clone()));
                        ui::forecast_window::PlumeState::Ready(points)
                    }
                    Err(e) => ui::forecast_window::PlumeState::Failed(e),
                };
            }
        }
        if self.forecast_open {
            let at = self.forecast_at.unwrap_or((0.0, 0.0));
            let tz = self.active_tz();
            let minute = self.minute_profile(at).map(|m| m.to_vec());
            let key = ((at.1 * 20.0).round() as i32, (at.0 * 20.0).round() as i32);
            let now = self
                .forecast_obs_cache
                .get(&key)
                .map(|(_, station, ob)| (station.as_str(), ob));
            let result = ui::forecast_window::show(
                ctx,
                &self.forecast_state,
                at,
                tz,
                minute.as_deref(),
                now,
                &mut self.popovers,
                &mut self.model_series_ui,
                &self.model_series_state,
                &mut self.plume_ui,
                &self.plume_state,
            );
            if result.series_changed {
                self.fetch_model_series(at.0, at.1);
            }
            if result.plume_changed {
                self.fetch_plume(at.0, at.1);
            }
            if !result.open {
                self.forecast_open = false;
            }
        }
        // Tornado climatology: receive the loaded database, then run any queued query.
        if let Some(rx) = &self.climo_rx {
            if let Ok(res) = rx.try_recv() {
                self.climo_loading = false;
                self.climo_rx = None;
                match res {
                    Ok(tracks) => {
                        let tracks = std::sync::Arc::new(tracks);
                        self.climo_tracks = Some(tracks.clone());
                        if let Some((lon, lat)) = self.climo_pending_query.take() {
                            self.climo_hits = wxdata::torclimo::near(&tracks, lon, lat, 40.0);
                            self.climo_center = Some((lon, lat));
                        }
                    }
                    Err(e) => self.climo_error = Some(e),
                }
            }
        }
        // Warning history for the same point (independent request; a failure just leaves the
        // section blank rather than sinking the whole card).
        if let Some(rx) = &self.climo_warn_rx {
            if let Ok(res) = rx.try_recv() {
                self.climo_warn_rx = None;
                match res {
                    Ok(s) => self.climo_warn = Some(s),
                    Err(e) => log::warn!("warning history: {e}"),
                }
            }
        }
        self.show_climatology_window(ctx);
        // GOES frame times arrived → keep the scrub at latest until the user moves it.
        if let Some(rx) = &self.goes_times_rx {
            if let Ok(times) = rx.try_recv() {
                self.goes_times = times;
                self.goes_times_rx = None;
            }
        }
        self.goes_time_bar(ctx);
        self.sat_loop_bar(ctx);
        self.model_fields_window(ctx);
        self.ensemble_stamps_window(ctx);
        self.beam_diagram_window(ctx);
        if let Some(act) = self
            .event_window
            .show(ctx, &mut self.settings, &mut self.drawer)
        {
            use ui::event_window::EventAction;
            match act {
                EventAction::Goto {
                    site,
                    lon,
                    lat,
                    zoom,
                    time,
                    span_min,
                } => {
                    self.goto_view(&site, lon, lat, zoom, time);
                    if span_min > 0 && time.is_some() {
                        self.views[self.active].timeline.replay_span_min = span_min;
                        // A replay without its warnings and its damage reports is just a loop;
                        // both already follow the playhead once they are on.
                        self.filters.show_alerts = true;
                        self.show_storm_reports = true;
                        self.rebuild_overlays();
                    }
                }
                EventAction::AddBookmark(span_min) => {
                    let n = self.settings.bookmarks.len() + 1;
                    self.add_bookmark(format!("Bookmark {n}"), span_min);
                }
            }
        }

        let metric = self.metric();
        if let Some(act) = self
            .chase_replay
            .show(ctx, &self.chase_track, &mut self.drawer, metric)
        {
            use ui::chase_replay::ReplayAction;
            match act {
                ReplayAction::Seek { lon, lat, time } => {
                    // Empty site: the replay flies the camera and moves the clock, and leaves the
                    // radar choice alone — the same handoff rule the deep links use.
                    let zoom = self.views[self.active].camera.zoom.max(8.0);
                    self.goto_view("", lon, lat, zoom, Some(time));
                }
                ReplayAction::OpenFile => {
                    crate::dialog::request_open(crate::dialog::ImportKind::ChaseGpx, "");
                }
            }
        }

        if let Some((i, day)) = self.rules_window.backtest_request.take() {
            self.start_backtest(i, day);
        }
        self.feature_chooser_window(ctx);
        if let Some(detail) = &self.detail {
            let tex = detail
                .image
                .as_ref()
                .and_then(|k| self.pf_icon_tex.get(k))
                .and_then(|t| t.as_ref());
            let impact = self
                .detail_impact
                .as_ref()
                .and_then(|k| self.impacts.by_id.get(k));
            if !ui::detail_window::show(ctx, detail, tex, &mut self.popovers, impact) {
                self.detail = None;
                self.detail_impact = None;
            }
        }
        // Storm attributes table: clicking a row flies there and opens that cell's popup, the
        // same destination as clicking the dot on the map.
        let entries = self.palette_entries();
        let bindings = crate::hotkeys::active(&self.settings).into_owned();
        if self
            .help_hub
            .show(ctx, &mut self.drawer, &bindings, &entries)
        {
            self.tour.start();
        }
        let cells: &[Cell] = if self.archive_bucket().is_some() {
            &[]
        } else {
            &self.storm_cells
        };
        // A cell within 15 km of a ZDR column owns it — the column marks the updraft, and the
        // updraft belongs to the storm the table is already listing.
        let zdr_cells: std::collections::HashSet<String> = match &self.zdr_cache {
            Some((_, hits, _)) if self.filters.show_zdr_columns => cells
                .iter()
                .filter(|c| {
                    hits.iter().any(|h| {
                        // Small distances: a flat-earth step is plenty and needs no helper.
                        let dy = (h.lat - c.lat) * 111.0;
                        let dx = (h.lon - c.lon) * 111.0 * c.lat.to_radians().cos();
                        (dx * dx + dy * dy).sqrt() <= 15.0
                    })
                })
                .map(|c| c.id.clone())
                .collect(),
            _ => std::collections::HashSet::new(),
        };
        let metric = self.metric();
        if ui::rules_window::show(
            &mut self.rules_window,
            ctx,
            &mut self.settings,
            &mut self.drawer,
            metric,
        ) {
            self.settings.save();
        }
        // One score per cell so the table can rank them; the join lives in wxdata. Raw (pre-debris
        // corroboration) couplets — the same cache `compute_couplets` reads, see its doc comment.
        let couplets: &[wxdata::rotation::CoupletHit] = match &self.couplet_cache {
            Some((_, (hits, ..))) => hits,
            None => &[],
        };
        // One computation, two views: `.score` on each explanation is exactly what `score_all`
        // itself returns, so the table's ranking and the detail panel's hover breakdown can never
        // disagree with each other.
        let cell_explanations =
            wxdata::cellscore::score_all_explained(cells, &self.probsevere, couplets);
        let cell_scores: Vec<u8> = cell_explanations.iter().map(|e| e.score).collect();
        // Each row's trend is its storm's, not its (recycled) SCIT ID's; built only while the
        // table is open.
        let storm_trends: std::collections::HashMap<String, Vec<ui::cell_window::CellSample>> =
            if self.cells_window.open {
                cells
                    .iter()
                    .map(|c| (c.id.clone(), self.storm_trend(&c.id)))
                    .collect()
            } else {
                Default::default()
            };
        if let Some(id) = ui::cells_window::show(
            &mut self.cells_window,
            ctx,
            cells,
            &cell_scores,
            &cell_explanations,
            &zdr_cells,
            &storm_trends,
            crate::theme::accent(self.settings.theme),
            &mut self.drawer,
        ) {
            if let Some(c) = self
                .active_storm_cells()
                .iter()
                .find(|c| c.id == id)
                .cloned()
            {
                let cam = &mut self.views[self.active].camera;
                cam.center = crate::render::mercator::lonlat_to_world(c.lon, c.lat);
                cam.zoom = cam.zoom.max(8.0);
                self.select_storm(c);
            }
        }
        let mut open_3d: Option<[f32; 6]> = None;
        // In the workstation the details are the Cell dock window (`dock/cell.rs`); its buttons
        // come back through `cell_follow_toggle` / `cell_view3d` and are answered here.
        let workstation = self.workstation_chrome();
        let dock_follow = std::mem::take(&mut self.cell_follow_toggle);
        let dock_3d = std::mem::take(&mut self.cell_view3d);
        let show_details = !workstation || dock_follow || dock_3d;
        if let Some(cell) = self.cell_popup.as_ref().filter(|_| show_details) {
            // The storm's trend while it is in the table; a storm that has left it has none to
            // add to, and its old ID may be another storm's.
            let trend_owned = match self.selected_storm_live() {
                Some((live, true)) => self.storm_trend(&live.id),
                _ => Vec::new(),
            };
            let trend = trend_owned.as_slice();
            let following = self
                .follow_cell
                .as_ref()
                .is_some_and(|(_, c, _)| c.id == cell.id);
            let tz = self.active_tz();
            let (open, toggled, to_3d) = if workstation {
                (true, dock_follow, dock_3d)
            } else {
                ui::cell_window::show(ctx, cell, trend, following, tz, &mut self.popovers)
            };
            // Crop the volume to this storm before opening it: a wall-to-wall box is a wall of
            // echo you would then have to hunt through by hand. The clip is computed here, where
            // the cell is still borrowed, and applied below.
            if to_3d {
                let (cell_lat, cell_lon) = (cell.lat, cell.lon);
                open_3d = Some(
                    self.views[self.active]
                        .site
                        .as_deref()
                        .and_then(wxdata::sites::site_by_id)
                        .map(|site| {
                            let (slat, slon) = (site.latitude as f64, site.longitude as f64);
                            let dy = ((cell_lat - slat) * 111.0) as f32;
                            let dx = ((cell_lon - slon) * 111.0 * slat.to_radians().cos()) as f32;
                            // `build_volume3d` (called right below once this clip is set) derives
                            // its own half_km from these same sweeps, so computing it again here
                            // — rather than guessing a fixed radius — keeps the clip box's [0,1]
                            // fractions meaningful against the box that actually gets built.
                            let half_km = self.views[self.active]
                                .volume
                                .as_mut()
                                .map(|v| {
                                    wxdata::volume3d::max_sample_range_km(&v.reflectivity_tilts())
                                })
                                .filter(|h| *h > 0.0)
                                .unwrap_or(150.0)
                                .max(50.0);
                            wxdata::volume3d::clip_around(half_km, dx, dy, 30.0)
                        })
                        .unwrap_or([0.0, 1.0, 0.0, 1.0, 0.0, 1.0]),
                );
            }
            if toggled {
                if following {
                    self.follow_cell = None;
                } else if let Some(site) = self.cells_site.clone() {
                    self.follow_cell = Some((site, cell.clone(), Instant::now()));
                    self.follow_notice = None;
                }
            }
            if !open {
                // In the workstation the window is the storm's detail, not its selection.
                if self.workstation_chrome() {
                    self.cell_details = false;
                } else {
                    self.cell_popup = None;
                }
            }
        }
        if let Some(clip) = open_3d {
            self.vol3d.clip = clip;
            self.build_volume3d();
        }
        if let Some(popup) = &self.gate_popup {
            let tz = self.active_tz();
            if !ui::gate_inspector::show(
                ctx,
                popup,
                tz,
                &self.settings.udp_products,
                &mut self.popovers,
            ) {
                self.gate_popup = None;
            }
        }
        if let Some(popup) = &self.suitability_popup {
            let current_site = self.views[self.active].site.clone();
            let (keep_open, switch_to, compare_with) = ui::suitability_popup::show(
                ctx,
                popup,
                current_site.as_deref(),
                &mut self.popovers,
            );
            if let Some(id) = switch_to {
                self.apply_palette(PaletteAction::SetSite(encode_site_id(id)), ctx);
            }
            if let Some(id) = compare_with {
                if let Some(cur) = current_site {
                    let pair = (cur, id.to_string());
                    // Clicking "Compare" again on the same pair turns the overlay back off.
                    self.coverage_compare =
                        (self.coverage_compare.as_ref() != Some(&pair)).then_some(pair);
                }
            }
            if !keep_open {
                self.suitability_popup = None;
                self.coverage_compare = None;
            }
        }
        if let Some(i) = self.marker_popup {
            match self.settings.markers.get_mut(i) {
                // The list shrank under us (the manager window deleted a row this frame).
                None => self.marker_popup = None,
                Some(m) => {
                    let r = ui::marker_popup::show(ctx, m, &mut self.popovers);
                    let watch = r
                        .watch
                        .then(|| (m.name.clone(), m.video_url.trim().to_string()));
                    if r.manage {
                        self.marker_window.open = true;
                    }
                    if let Some((name, url)) = watch {
                        self.watch_stream(name, url);
                    }
                    if r.remove {
                        self.settings.markers.remove(i);
                        self.marker_popup = None;
                    } else if !r.open {
                        self.marker_popup = None;
                    }
                }
            }
        }
        self.zone_popup_card(ctx);
        self.zone_naming_dialog(ctx);
        if let Some(sp) = self.pending_spotter.take() {
            self.open_spotter(&sp);
        }
        if let Some(p) = &mut self.video_player {
            if !p.show(ctx, &mut self.drawer) {
                self.video_player = None; // dropping the player stops the download
            }
        }
        self.follow_badge(ctx);
        if !self.obs_mode {
            self.chase_hud(ctx);
            self.yall_card(ctx);
        }
        // The workstation reads the bulletin in its Alerts window (`chrome/dock/alerts.rs`).
        let workstation = self.workstation_chrome();
        if let Some(popup) = self.warning_popup.as_mut().filter(|_| !workstation) {
            if !ui::warning_window::show(ctx, popup, &mut self.popovers, &self.impacts.by_id) {
                self.warning_popup = None;
            }
        }
        let tz = self.active_tz();
        if self.show_sensors
            && !ui::sensor_window::show(ctx, self.sensor_data.as_ref(), tz, &mut self.drawer)
        {
            self.show_sensors = false;
        }
        if self.show_hodo
            && !ui::hodograph_window::show(
                ctx,
                self.hodo_site.as_deref(),
                &self.hodo_data,
                self.hodo_history.make_contiguous(),
                &mut self.hodo_tab,
                self.settings.tz_for(self.hodo_site.as_deref()),
                &mut self.drawer,
            )
        {
            self.show_hodo = false;
        }
        self.show_region_stats(ctx);
        // The ground along the cut (M3.6): its tiles asked for, and what is here sampled the way
        // the section samples its columns (straight between A and B in longitude and latitude).
        let ground = self.xsection_line().map(|line| {
            const N: usize = 101;
            let at = |i: usize| {
                let t = i as f64 / (N - 1) as f64;
                [
                    line.a[0] + (line.b[0] - line.a[0]) * t,
                    line.a[1] + (line.b[1] - line.a[1]) * t,
                ]
            };
            for i in (0..N).step_by(10) {
                let p = at(i);
                self.request_ground(p[0], p[1], ctx);
            }
            let mut profile = ui::xsection_window::GroundProfile::default();
            for i in 0..N {
                let p = at(i);
                profile.msl_m.push(match self.terrain.ground(p[0], p[1]) {
                    terrain_cache::Ground::Known {
                        msl_m,
                        resolution_m,
                    } => {
                        profile.resolution_m = Some(resolution_m);
                        Some(msl_m as f32)
                    }
                    terrain_cache::Ground::Loading => {
                        profile.loading = true;
                        None
                    }
                    terrain_cache::Ground::Unknown => None,
                });
            }
            profile
        });
        if let (Some(xs), Some(tex), Some(line)) =
            (&self.xsection, &self.xsection_tex, self.xsection_line())
        {
            let mut moment = self.xsection_moment;
            let before = ui::xsection_window::XsControls {
                bearing_deg: line.bearing(),
                length_km: line.length_km(),
                cut_3d: self.xsection_cut_3d,
                info: self.xsection_info(xs),
                antenna_msl_km: self.views[self
                    .xsection_source
                    .as_ref()
                    .map_or(self.active, |s| s.pane)]
                .site
                .as_deref()
                .and_then(wxdata::sites::site_by_id)
                .map(|s| (f64::from(s.elevation_meters) + wxdata::towers::tower_m(s.id)) / 1000.0),
                ground: ground.clone().unwrap_or_default(),
                ..Default::default()
            };
            let mut ctl = before.clone();
            let open = ui::xsection_window::show(
                ctx,
                xs,
                tex,
                &mut moment,
                &mut self.xsection_beam_rise,
                &mut ctl,
                &mut self.drawer,
            );
            let pane = self
                .xsection_source
                .as_ref()
                .map_or(self.active, |s| s.pane);
            if !open {
                self.xsection = None;
                self.xsection_tex = None;
                self.xsection_pts.clear();
                self.xsection_source = None;
            } else if moment != self.xsection_moment {
                self.xsection_moment = moment;
                self.build_xsection(pane, ctx);
            } else if ctl != before {
                self.apply_xsection_controls(pane, line, &before, &ctl, ctx);
            }
        }
        // The workstation shows the volume in its 3D volume tool window (`chrome/dock/volume.rs`).
        if self.show_3d {
            // Source revisions, sweep policy, selected beams and palette all belong to the grid.
            if self.volume3d_supported {
                self.build_volume3d();
            }
            self.drain_volume3d(ctx);
        }
        if self.show_3d && !self.workstation_chrome() {
            let mut open = true;
            ui::volume3d_window::show(
                ctx,
                &mut open,
                &mut self.vol3d,
                &mut self.vol3d_pending,
                VOL3D_N as u32,
                VOL3D_NZ as u32,
                VOL3D_TOP_KM,
                self.vol3d_range,
                self.cappi_alt_km,
                &mut self.drawer,
                ui::motion::degraded(),
            );
            self.show_3d = open;
        }
        if self.show_cappi {
            self.update_cappi(ctx);
            let open = match self.cappi_tex.clone() {
                Some(tex) => ui::cappi_window::show(
                    ctx,
                    &tex,
                    &mut self.cappi_alt_km,
                    300.0,
                    &mut self.drawer,
                ),
                None => ui::cappi_window::show_empty(ctx, &mut self.drawer),
            };
            self.show_cappi = open;
        }
        if self.show_data_health {
            let entries = self.source_entries();
            ui::source_health_window::show(
                ctx,
                &entries,
                &mut self.show_data_health,
                &mut self.drawer,
            );
        }
        // theme_plan.md §4: self-gates on `settings.analyst_mode`, so this costs nothing when off.
        // The workstation shows the log as a dock tab (`app::chrome::dock::log`).
        if !self.workstation_chrome() {
            ui::analyst_log_window::show(
                ctx,
                &mut self.settings,
                &mut self.drawer,
                &self.views[self.active].live_scan,
            );
        }
        self.show_warning_banners(ctx);
        self.show_toasts(ctx);

        // Turn this frame's UI mutations into uploads/fetches before painting the map.
        spatial_groups::sync_sites(&mut self.views, self.active);
        if self.link_times && !self.views.is_empty() {
            let active = self.active.min(self.views.len() - 1);
            self.sync_pane(active, ctx);
            if self.sync_linked_pane_times() {
                ctx.request_repaint();
            }
            for idx in 0..self.views.len() {
                if idx != active {
                    self.sync_pane(idx, ctx);
                }
            }
        } else {
            self.linked_analysis = pane_time::LinkedTimeState::default();
            for idx in 0..self.views.len() {
                self.sync_pane(idx, ctx);
            }
        }
        self.sync_gis_layers();
        self.release_3d();
        for idx in 0..self.views.len() {
            self.sync_isosurface(idx, ctx);
            self.prebuild_loop3d(idx, ctx);
        }
        self.sync_cloud_top();
        self.sync_boundaries(ctx);
        self.sync_impacts(ctx);
        self.sync_radar_wind(ctx);
        self.sync_terrain3d(ctx);
        self.sync_yall(ctx);
        // One Smoothing toggle for radar and every gridded layer.
        crate::render::set_field_smoothing(self.settings.smooth_radar);
        self.sync_model_isotherms();
        self.sync_overlay();

        // Streaming mode's touch strip: the way out (and between scenes) without a keyboard.
        self.presentation_controls(root);
        // Streaming mode's broadcast dressing: clock, caption, crawl, logo.
        self.broadcast_dressing(root);
        // OBS-mode hint so the chrome-free view is still escapable. Top centre: the corners are
        // the dressing's.
        if self.obs_mode {
            egui::Area::new("obs_hint".into())
                .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 10.0))
                .interactable(false)
                .show(root, |ui| {
                    let txt = self.presentation_hint();
                    egui::Frame::new()
                        .fill(egui::Color32::from_black_alpha(150))
                        .corner_radius(4.0)
                        .inner_margin(egui::Margin::symmetric(8, 4))
                        .show(ui, |ui| {
                            ui.colored_label(egui::Color32::from_white_alpha(200), txt)
                        });
                });
        }
    }
}
