//! What each PaletteAction does: the one dispatcher every surface (palette, layers, keys, phone sheet) goes through.
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Run one registry action. Every surface (drawer, pills, mobile sheets) routes through it.
    pub(crate) fn apply_palette(&mut self, action: PaletteAction, ctx: &egui::Context) {
        use AppWindow as W;
        match action {
            PaletteAction::SetMoment(m, srv) => {
                self.radar_timeline();
                let v = &mut self.views[self.active];
                v.moment = m;
                if m == Moment::Velocity {
                    v.srv = srv;
                }
            }
            PaletteAction::SetSite(buf) => {
                let id = decode_site_id(buf);
                let active = self.active;
                let v = &mut self.views[active];
                let changed = v.site.as_deref() != Some(id.as_str());
                if changed {
                    v.site = Some(id.clone());
                    // Mirrors `try_pick_site`'s own cleanup: a popup left open for the previous
                    // site's feature under the old camera position answers nothing once the pane
                    // has jumped elsewhere.
                    self.cell_popup = None;
                    self.warning_popup = None;
                    self.detail = None;
                }
                spatial_groups::sync_sites(&mut self.views, active);
            }
            PaletteAction::SeekTime(seconds) => {
                self.radar_timeline();
                if let Some(target) = chrono::DateTime::from_timestamp(seconds, 0) {
                    let view = &mut self.views[self.active];
                    let site = view.site.as_deref().unwrap_or_default();
                    let stale_axis = view.timeline.seek_to_valid_time(site, target);
                    let selected = view.timeline.current().map(|frame| frame.name());
                    let shown = view.volume.as_ref().map(|volume| volume.name.as_str());
                    if stale_axis || selected != shown {
                        view.volume = None;
                        view.loading = false;
                    }
                    self.select_linked_explicit(self.active, Some(target));
                }
            }
            PaletteAction::SetModel(model) => {
                let next = self.views[self.active].models.model_sel.with_model(model);
                self.commit_model_selection(next, true);
            }
            PaletteAction::SetModelProduct(product) => {
                let next = self.views[self.active]
                    .models
                    .model_sel
                    .with_product(product);
                self.commit_model_selection(next, true);
            }
            PaletteAction::ToggleModelProduct(product) => {
                let layer = product.layer();
                if self.views[self.active].fields_on.contains(&layer) {
                    self.set_field(layer, false);
                } else {
                    let next = self.views[self.active]
                        .models
                        .model_sel
                        .with_product(product);
                    // A row adds its layer without displacing others: HRRR reflectivity under
                    // GFS pressure contours is a normal thing to want.
                    self.commit_model_selection(next, false);
                }
            }
            PaletteAction::SetModelLead(minutes) => self.set_model_lead_min(minutes),
            PaletteAction::CompareSelected => {
                let Some((field, _)) =
                    crate::model_browser::compare_field(self.views[self.active].models.model_sel)
                else {
                    return;
                };
                // Carry the lead across: comparisons read the shared forecast hour.
                if let Some((max, _)) = field.lead_hours() {
                    self.comparison_fcst_hour = (self.model_lead_min() / 60).min(max);
                }
                let already = self.diff_field == field && self.views[self.active].swipe_compare;
                self.diff_field = field;
                // The single-model layer would paint over the halves, so it steps aside.
                let layer = self.views[self.active].models.model_sel.layer();
                self.views[self.active].fields_on.remove(&layer);
                if already || !self.views[self.active].swipe_compare {
                    self.apply_palette(PaletteAction::ToggleCompareSwipe, ctx);
                }
            }
            // An analysis has no lead: its steps are hours, through the Hour menu's own list.
            PaletteAction::StepModelLead(steps)
                if !self.views[self.active].models.model_sel.model.has_lead() =>
            {
                let model = self.views[self.active].models.model_sel.model;
                let runs = model.runs_around(
                    self.views[self.active].models.model_run,
                    Utc::now(),
                    model.run_list_len(),
                );
                self.views[self.active].models.model_run = crate::model_browser::step_run(
                    &runs,
                    self.views[self.active].models.model_run,
                    steps,
                );
                self.activate_model_timeline();
            }
            PaletteAction::StepModelLead(steps) => {
                // Step along the model's own published leads, which are not evenly spaced for
                // every model (the NAM 12 km and the global models thin out with lead).
                let range = self.views[self.active]
                    .models
                    .model_sel
                    .model
                    .leads_for(self.views[self.active].models.model_run, Utc::now());
                let mut lead = range.clamp(self.model_lead_min());
                for _ in 0..steps.unsigned_abs() {
                    lead = range.neighbour(lead, steps > 0);
                }
                self.set_model_lead_min(lead);
            }
            PaletteAction::SetModelRun(run) => {
                self.views[self.active].models.model_run =
                    run.and_then(|secs| DateTime::from_timestamp(secs, 0));
                // A shorter run may not reach the lead that was showing; snap it back.
                self.set_model_lead_min(self.model_lead_min());
            }
            PaletteAction::ToggleField(layer) => {
                // The active pane's choice, not the app's: that is what makes two panes able to
                // show two fields.
                let on = self.views[self.active].fields_on.contains(&layer);
                {
                    use crate::render::FieldLayer as FL;
                    let view = &mut self.views[self.active];
                    if layer == FL::ModelDiff && !on {
                        view.fields_on.remove(&FL::CompareA);
                        view.fields_on.remove(&FL::CompareB);
                        view.blink_compare = false;
                        view.overlay_compare = false;
                        view.swipe_compare = false;
                    } else if matches!(layer, FL::CompareA | FL::CompareB) {
                        // A direct layer toggle means exactly what its row says, not a stale
                        // blink/overlay mode left armed behind it.
                        view.blink_compare = false;
                        view.overlay_compare = false;
                        view.swipe_compare = false;
                        if !on {
                            view.fields_on.remove(&FL::ModelDiff);
                        }
                    }
                }
                self.set_field(layer, !on);
                // ROADMAP_NEW D1/D3 "recent products": only a genuine click here, not a
                // workspace restore or the HRRR sub-mode/model-compare bookkeeping that write
                // `fields_on` directly elsewhere — and only turning it *on*, so closing something
                // doesn't bump it to the top of a list meant for finding it again.
                if !on {
                    ui::layers_panel::note_recent(&mut self.settings.recent_layers, layer.slug());
                }
            }
            PaletteAction::ToggleOverlay(t) => {
                if matches!(
                    t,
                    OverlayToggle::LinkCameras
                        | OverlayToggle::LinkSite
                        | OverlayToggle::LinkCursor
                ) {
                    self.toggle_spatial_link(t);
                } else {
                    let f = self.overlay_flag(t);
                    *f = !*f;
                }
                if t == OverlayToggle::RadarWind {
                    self.radar_wind_toggled();
                }
                // These feed the assembled feature set rather than a painter flag.
                use OverlayToggle as T;
                if matches!(
                    t,
                    T::Tropical
                        | T::Outages
                        | T::ForecastZones
                        | T::CwaBoundaries
                        | T::ProbSevere
                        | T::Aviation
                        | T::Tfr
                        | T::Alerts
                        | T::Mds
                        | T::Fires
                        | T::ImportedGis
                ) {
                    self.rebuild_overlays();
                }
            }
            // `Off` clears every active contour; any real kind toggles just that one, so several
            // can be layered on at once (each row in the command palette/layers panel already
            // shows its own checked state — this is what makes clicking one leave the rest alone).
            PaletteAction::SetContours(ContourKind::Off) => self.active_contours.clear(),
            PaletteAction::SetContours(k) => {
                if !self.active_contours.remove(&k) {
                    self.active_contours.insert(k);
                }
            }
            // Tapping the armed tool disarms it. Interrogate is the resting state, so "off" means
            // back to it — without this the row read ON with no way to turn it off.
            PaletteAction::Tool(t) => {
                self.tool = if self.tool == t {
                    MapTool::Interrogate
                } else {
                    t
                };
                if self.tool != MapTool::GateInspector {
                    self.gate_popup = None;
                }
            }
            PaletteAction::SetPanes(n) => {
                self.set_pane_count(n);
                if n > 1 {
                    self.hint(
                        "panes",
                        "Each pane keeps its own radar, product and tilt \u{2014} turn \
                         on Link pane cameras to pan them together",
                    );
                }
            }
            PaletteAction::SetPaneLayout(layout) => {
                self.pane_layout = layout;
            }
            PaletteAction::AllTilts => self.apply_all_tilts(),
            PaletteAction::ToggleOutputWindow => self.output.open = !self.output.open,
            PaletteAction::ClearStormTracks => {
                self.storm_tracks.tracks.clear();
                self.storm_tracks.pending.clear();
                self.storm_tracks.selected = None;
            }
            PaletteAction::CompareInPanes => self.apply_compare_panes(),
            PaletteAction::ToggleBlinkCompare => {
                if !self.diff_field.supports_side_by_side() {
                    return;
                }
                use crate::render::FieldLayer as FL;
                let view = &mut self.views[self.active];
                view.blink_compare = !view.blink_compare;
                if view.blink_compare {
                    // Start on A, and make sure the subtraction layer isn't also drawn under it —
                    // three overlapping fields answers a question nobody asked.
                    view.fields_on.insert(FL::CompareA);
                    view.fields_on.remove(&FL::CompareB);
                    view.fields_on.remove(&FL::ModelDiff);
                    view.overlay_compare = false;
                    view.swipe_compare = false;
                }
                // Turning it off leaves whichever field was showing at the moment as a static
                // single-field view, rather than snapping back to some other mode on its own.
            }
            PaletteAction::ToggleCompareOverlay => {
                if !self.diff_field.supports_side_by_side() {
                    return;
                }
                use crate::render::FieldLayer as FL;
                let view = &mut self.views[self.active];
                view.overlay_compare = !view.overlay_compare;
                if view.overlay_compare {
                    view.fields_on.insert(FL::CompareA);
                    view.fields_on.insert(FL::CompareB);
                    view.fields_on.remove(&FL::ModelDiff);
                    view.blink_compare = false;
                    view.swipe_compare = false;
                } else {
                    // Stop on A as a stable single-field view, mirroring blink's "leave a useful
                    // comparison side visible" behavior rather than exposing a blank pane.
                    view.fields_on.insert(FL::CompareA);
                    view.fields_on.remove(&FL::CompareB);
                }
            }
            PaletteAction::ToggleCompareSwipe => {
                if !self.diff_field.supports_side_by_side() {
                    return;
                }
                use crate::render::FieldLayer as FL;
                let view = &mut self.views[self.active];
                view.swipe_compare = !view.swipe_compare;
                if view.swipe_compare {
                    view.fields_on.insert(FL::CompareA);
                    view.fields_on.insert(FL::CompareB);
                    view.fields_on.remove(&FL::ModelDiff);
                    view.blink_compare = false;
                    view.overlay_compare = false;
                } else {
                    view.fields_on.insert(FL::CompareA);
                    view.fields_on.remove(&FL::CompareB);
                }
            }
            PaletteAction::CycleBasemap => {
                let (mb, mt) = (
                    !self.settings.mapbox_key.is_empty(),
                    !self.settings.maptiler_key.is_empty(),
                );
                let next = self.views[self.active].basemap.next(
                    mb,
                    mt,
                    crate::tiles::valid_xyz_template(&self.settings.custom_tile_url),
                );
                self.set_basemap(next);
            }
            PaletteAction::ToggleMute => self.apply_action(BindableAction::ToggleMute, ctx),
            PaletteAction::Explain(i) => self.help_hub.explain(i),
            // The workstation's Layers window is its panel.
            PaletteAction::TogglePanel if self.workstation_chrome() => {
                self.dock.toggle(crate::app::chrome::DockWin::Layers);
            }
            PaletteAction::TogglePanel => self.panel_open = !self.panel_open,
            PaletteAction::ToggleRibbon => self.ribbon_collapsed = !self.ribbon_collapsed,
            PaletteAction::DockWindow(w) => self.dock.toggle(w),
            PaletteAction::Reload => self.trigger_reload(ctx),
            PaletteAction::InstantReplay => self.instant_replay(),
            PaletteAction::ToggleSatLoop => self.toggle_sat_loop(),
            PaletteAction::ToggleWindBarbs => {
                self.settings.wind_barbs = !self.settings.wind_barbs;
                if self.settings.wind_barbs {
                    self.show_wind = true;
                }
                self.settings.save();
            }
            PaletteAction::ToggleWindStreamlines => {
                self.settings.wind_streamlines = !self.settings.wind_streamlines;
                if self.settings.wind_streamlines {
                    self.show_wind = true;
                }
                self.settings.save();
            }
            PaletteAction::BeamDiagram => self.beam_diagram.open = !self.beam_diagram.open,
            PaletteAction::ModelFields => self.field_browser.open = !self.field_browser.open,
            PaletteAction::EnsembleMembers => {
                self.ensemble_stamps.open = !self.ensemble_stamps.open;
                // The stamps are the ensemble layer's members: turn the layer on to have them.
                if self.ensemble_stamps.open {
                    self.views[self.active]
                        .fields_on
                        .insert(crate::render::FieldLayer::Ensemble);
                }
            }
            PaletteAction::GoLive => {
                self.radar_timeline();
                self.views[self.active].timeline.go_head();
            }
            PaletteAction::Nav(step) => self.apply_nav(step, ctx),
            PaletteAction::CopyViewLink => {
                let v = &self.views[self.active];
                let c = v.camera.center;
                let (lon, lat) = crate::render::mercator::world_to_lonlat(c.0, c.1);
                // A live view shares as live; a scrubbed one carries its timestamp, so the link
                // lands on the frame the sender was looking at.
                let time = (!v.timeline.following)
                    .then(|| v.timeline.current().and_then(|id| id.date_time()))
                    .flatten();
                let link = goto_link(&Goto {
                    site: v.site.clone().unwrap_or_default(),
                    lon,
                    lat,
                    zoom: v.camera.zoom,
                    time,
                    moment: Some(v.moment),
                    tilt: Some(v.tilt),
                    basemap: Some(v.basemap.slug().to_string()),
                    threshold: v.threshold_enabled[v.moment.index()]
                        .then(|| v.thresholds[v.moment.index()]),
                    srv: v.srv,
                    // Open gauge cards travel with the view: "look at this river" is the point.
                    gauges: self.gauge_cards.lids(),
                    tropical: self
                        .spaghetti
                        .enabled
                        .then(|| self.spaghetti.focus.clone().unwrap_or_default()),
                });
                // A phone or a tablet has a share sheet, and pasting into a chat is what this is
                // for; the clipboard is the fallback for everything that does not.
                if !crate::platform::share_link("HookEcho", &link) {
                    ctx.copy_text(link.clone());
                    self.banner("Link copied".to_string(), link);
                }
            }
            PaletteAction::SaveWorkspace => {
                let ws = self.capture_workspace();
                let name = ws.name.clone();
                self.settings.workspaces.push(ws);
                self.settings.save();
                // ponytail: auto-named, renamed in Settings. A naming dialog mid-storm is the
                // last thing anyone wants.
                self.toast(
                    ToastKind::Success,
                    format!("Saved \u{2014} rename \"{name}\" in Settings"),
                );
            }
            PaletteAction::ApplyWorkspace(i) => {
                if let Some(ws) = self.settings.workspaces.get(i).cloned() {
                    self.apply_workspace(&ws, ctx);
                    self.toast(ToastKind::Info, format!("Workspace: {}", ws.name));
                }
            }
            PaletteAction::OpenInWindy => {
                let v = &self.views[self.active];
                let c = v.camera.center;
                let (lon, lat) = crate::render::mercator::world_to_lonlat(c.0, c.1);
                // Pick the layer the user deliberately turned on, not the one that is always
                // there: radar is the default state, so leading with it would make every other
                // branch dead code and always land people on the same Windy page.
                let overlay = if self.show_wind {
                    "wind"
                } else if self.field_wanted(crate::render::FieldLayer::Cape) {
                    "cape"
                } else if v.basemap.slug().starts_with("goes") {
                    "satellite"
                } else if v.show_radar && v.volume.is_some() {
                    "radar"
                } else {
                    "wind"
                };
                let url = windy_url(overlay, lon, lat, v.camera.zoom);
                if let Err(e) = crate::platform::open_url(&url) {
                    log::warn!("could not open {url}: {e}");
                }
            }
            PaletteAction::ImportGis => {
                crate::dialog::request_open(crate::dialog::ImportKind::GisFile, "");
            }
            PaletteAction::ExportGis => self.export_map_geojson(),
            PaletteAction::ZoomToGis => self.zoom_to_gis(None),
            PaletteAction::ToggleMap3d => {
                let view = &mut self.views[self.active];
                let on = !view.map_3d.enabled;
                view.set_map_3d(on);
            }
            // The two follow modes are one choice: turning either on turns the other off.
            PaletteAction::ToggleFollowSweep => {
                let v = &mut self.views[self.active];
                v.follow_live_sweep = !v.follow_live_sweep;
                v.follow_lowest_cut &= !v.follow_live_sweep;
                v.followed_sweep = None;
            }
            PaletteAction::ToggleStatusFooter => self.dock.footer_open = !self.dock.footer_open,
            PaletteAction::ResetWindowLayout => {
                let layout = self.settings.layout;
                self.dock.reset_layout(layout);
            }
            PaletteAction::ToggleLinkAll => {
                let on = !self.all_pane_links_on();
                for t in OverlayToggle::PANE_LINKS {
                    *self.overlay_flag(t) = on;
                }
                for view in &mut self.views {
                    view.spatial_links = crate::pane_links::SpatialLinks::legacy(on, on, on);
                    view.spatial_restore_raw = None;
                }
                if on {
                    self.link_all_cameras();
                    let site = self.views[self.active].site.clone();
                    for view in &mut self.views {
                        view.site = site.clone();
                        view.spatial_site_snapshot = site.clone();
                    }
                }
                self.linked_probe = None;
            }
            PaletteAction::ToggleFollowLowest => {
                let v = &mut self.views[self.active];
                v.follow_lowest_cut = !v.follow_lowest_cut;
                v.follow_live_sweep &= !v.follow_lowest_cut;
            }
            PaletteAction::OpenWindow(w) => match w {
                W::Site => {
                    if self.site_dialog.is_none() {
                        self.site_dialog = Some(Default::default());
                    }
                }
                W::Settings => self.open_settings(),
                W::Markers => self.marker_window.open = true,
                W::Placefiles => self.placefile_window.open = true,
                W::UdpProducts => self.udp_window.open = true,
                W::Palettes => self.palette_editor.open = true,
                W::Events => self.event_window.open = true,
                W::ChaseReplay => self.chase_replay.open = true,
                W::Digest => {
                    self.digest_window.open = true;
                    self.generate_digest(ctx);
                }
                W::Afd => {
                    self.afd_open = true;
                    self.fetch_afd();
                }
                W::Tropical => self.tropical_window.open = true,
                W::Cappi => {
                    self.show_cappi = true;
                    self.cappi_key = None; // force a re-slice on open
                }
                W::StormTable if self.workstation_chrome() => {
                    self.dock.toggle(chrome::DockWin::Storms)
                }
                W::StormTable => self.cells_window.toggle(),
                W::FloodGauges => {
                    // The list is the map layer's fetch: asking for the dashboard asks for it.
                    self.show_gauges = true;
                    if self.workstation_chrome() {
                        self.dock.toggle(chrome::DockWin::Gauges);
                    } else {
                        self.gauge_dash.window_open = !self.gauge_dash.window_open;
                    }
                }
                W::Help => self.help_hub.toggle(),
                W::AlertRules => self.rules_window.toggle(),
                W::Verify => self.open_verify(),
                W::ModelVerify => self.model_verify.open = true,
                W::Volume3d => self.build_volume3d(),
                W::Climatology => {
                    self.climo_open = true;
                    self.load_climatology();
                }
                W::LayerManager => self.layer_window_open = true,
                W::Setup => self.firstrun.start(),
                W::Tour => self.tour.start(),
                W::About => {
                    self.about_open = true;
                    self.check_for_update(ctx);
                }
                W::DataHealth => self.show_data_health = true,
            },
        }
    }

    pub(crate) fn apply_action(&mut self, action: BindableAction, ctx: &egui::Context) {
        use BindableAction as A;
        match action {
            // Everything the registry already knows how to do runs through the one executor.
            A::Palette(p) => self.apply_palette(p, ctx),
            A::TiltUp => {
                let v = &mut self.views[self.active];
                if let Some(vol) = &v.volume {
                    if v.tilt + 1 < vol.elevations.len() {
                        v.tilt += 1;
                    }
                }
            }
            A::TiltDown => {
                let v = &mut self.views[self.active];
                v.tilt = v.tilt.saturating_sub(1);
            }
            A::Camera3dPitchUp
            | A::Camera3dPitchDown
            | A::Camera3dBearingLeft
            | A::Camera3dBearingRight => {
                let v = &mut self.views[self.active];
                if v.map_3d.enabled {
                    // One keypress, one visible step — big enough to see, small enough that
                    // holding the key down still reads as a smooth nudge rather than a jump.
                    const PITCH_STEP_DEG: f32 = 5.0;
                    const BEARING_STEP_DEG: f32 = 10.0;
                    match action {
                        A::Camera3dPitchUp => {
                            v.camera.pitch = (v.camera.pitch + PITCH_STEP_DEG)
                                .clamp(0.0, crate::render::mercator::MAX_PITCH_DEG);
                        }
                        A::Camera3dPitchDown => {
                            v.camera.pitch = (v.camera.pitch - PITCH_STEP_DEG)
                                .clamp(0.0, crate::render::mercator::MAX_PITCH_DEG);
                        }
                        A::Camera3dBearingLeft => {
                            v.camera.bearing = (v.camera.bearing - BEARING_STEP_DEG + 180.0)
                                .rem_euclid(360.0)
                                - 180.0;
                        }
                        A::Camera3dBearingRight => {
                            v.camera.bearing = (v.camera.bearing + BEARING_STEP_DEG + 180.0)
                                .rem_euclid(360.0)
                                - 180.0;
                        }
                        _ => unreachable!(),
                    }
                }
            }
            A::OpenSiteDialog => {
                if self.site_dialog.is_none() {
                    self.site_dialog = Some(Default::default());
                }
            }
            A::ToggleAlertPanel if self.workstation_chrome() => {
                self.dock.toggle(crate::app::chrome::DockWin::Alerts);
            }
            A::ToggleAlertPanel => {
                // The bell tab and the panel are one surface now: the key opens the panel on
                // Alerts, and closes it if that's already what's showing.
                let showing = self.panel_open && self.show_alert_panel;
                self.panel_open = !showing;
                self.show_alert_panel = true;
            }
            A::ToggleObs => {
                self.obs_mode = !self.obs_mode;
                if !self.obs_mode {
                    self.obs_tour = false;
                }
            }
            A::ToggleObsTour => {
                self.obs_tour = !self.obs_tour;
                self.obs_tour_last = None; // step immediately on enable
                if self.obs_tour {
                    self.obs_mode = true;
                }
            }
            // The workstation searches in its Layers window: open it with the keyboard there.
            A::ToggleDrawer | A::CommandSearch if self.workstation_chrome() => {
                self.dock.query.clear();
                self.dock.open_search();
            }
            A::ToggleDrawer => {
                // Hidden: bring it back and land in the search box. Visible: focus the search,
                // which is what the key always did.
                self.panel_open = true;
                self.show_alert_panel = false;
                self.sidebar_focus_search = true;
            }
            A::StepBack | A::StepHourBack if self.model_timeline_active() => {
                self.apply_palette(PaletteAction::StepModelLead(-1), ctx)
            }
            A::StepForward | A::StepHourForward if self.model_timeline_active() => {
                self.apply_palette(PaletteAction::StepModelLead(1), ctx)
            }
            A::StepBack => self.views[self.active].timeline.step(-1),
            A::StepForward => self.views[self.active].timeline.step(1),
            A::StepHourBack => self.views[self.active].timeline.step_time(-60),
            A::StepHourForward => self.views[self.active].timeline.step_time(60),
            A::Fullscreen => {
                // Desktop only; mobile is already fullscreen.
                if !cfg!(target_os = "android") {
                    let cur = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!cur));
                }
            }
            A::CommandSearch => {
                self.layers_query.clear();
                self.panel_open = true;
                self.show_alert_panel = false;
                self.sidebar_focus_search = true;
            }
            A::CheatSheet => self.show_cheatsheet = !self.show_cheatsheet,
            A::ToggleMute => {
                self.settings.mute_alerts = !self.settings.mute_alerts;
                let msg = if self.settings.mute_alerts {
                    "Audio alerts muted"
                } else {
                    "Audio alerts unmuted"
                };
                self.toast(ToastKind::Info, msg);
            }
            A::ProductPrev | A::ProductNext => {
                // Cycles `Moment::ALL` in its own declared order, which is the order the `1`-`7`
                // keys already select in, so stepping and jumping agree about what "next" means.
                // The pane's SRV choice is left alone: it is a way of reading velocity, not a
                // product of its own, and stepping past velocity and back should not clear it.
                let v = &mut self.views[self.active];
                let n = Moment::ALL.len();
                let at = Moment::ALL.iter().position(|m| *m == v.moment).unwrap_or(0);
                let step = if action == A::ProductNext { 1 } else { n - 1 };
                v.moment = Moment::ALL[(at + step) % n];
            }
            A::FocusPrevPane | A::FocusNextPane => {
                let n = self.views.len();
                if n > 1 {
                    self.active = if action == A::FocusNextPane {
                        (self.active + 1) % n
                    } else {
                        (self.active + n - 1) % n
                    };
                }
            }
        }
    }

    /// Act on the signals the chrome raised this frame (drawer sections, mobile sheets, pills).
    pub(crate) fn apply_ui_actions(
        &mut self,
        actions: ui::layer_options::UiActions,
        ctx: &egui::Context,
    ) {
        if let Some(a) = actions.palette {
            self.apply_palette(a, ctx);
        }
        if let Some(layer) = actions.qpe_window {
            if ui::layer_options::select_qpe_window(&mut self.views[self.active].fields_on, layer) {
                ui::layers_panel::note_recent(&mut self.settings.recent_layers, layer.slug());
            }
        }
        if let Some(layer) = actions.echo_top_threshold {
            if ui::layer_options::select_echo_top_threshold(
                &mut self.views[self.active].fields_on,
                layer,
            ) {
                ui::layers_panel::note_recent(&mut self.settings.recent_layers, layer.slug());
            }
        }
        if let Some(layer) = actions.isotherm_level {
            if ui::layer_options::select_isotherm_level(
                &mut self.views[self.active].fields_on,
                layer,
            ) {
                ui::layers_panel::note_recent(&mut self.settings.recent_layers, layer.slug());
            }
        }
        if let Some(layer) = actions.flash_ari_window {
            if ui::layer_options::select_flash_ari_window(
                &mut self.views[self.active].fields_on,
                layer,
            ) {
                ui::layers_panel::note_recent(&mut self.settings.recent_layers, layer.slug());
            }
        }
        if actions.open_site_dialog && self.site_dialog.is_none() {
            self.site_dialog = Some(Default::default());
        }
        if actions.reload {
            self.trigger_reload(ctx);
        }
        if actions.instant_replay {
            self.instant_replay();
        }
        if let Some(raster) = actions.export_trail {
            self.export_trail(raster);
        }
        if actions.reset_trail {
            // `advance_trail` always chooses frames at or before the active playhead, so dropping
            // this accumulator is a deterministic reset at the selected live/archive time. Also
            // invalidate the uploaded image: a new accumulator starts its generation at zero and
            // could otherwise collide with the previous trail's first cache key.
            self.trail = None;
            self.trail_more = true;
            self.filters.trail_status = "Reset; rebuilding at the selected time…".into();
            self.pane_shown.remove(&self.active);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if actions.download_chasepack {
            self.start_chasepack();
        }
        if actions.cancel_chasepack {
            if let Some(p) = &self.chasepack {
                p.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            self.chasepack = None;
        }
        if actions.outlook_kind_changed && self.filters.outlook_day == 1 {
            // Hazard switched: drop the stale Day-1 features so the empty-check refetches it.
            self.outlook_features[0].clear();
        }
        if actions.ero_day_changed {
            self.ero_features.clear();
            if (1..=3).contains(&self.filters.ero_day) {
                self.spawn_overlay(ctx, OverlaySource::Ero(self.filters.ero_day));
            }
        }
        if actions.fire_day_changed {
            self.fire_features.clear();
            if (1..=2).contains(&self.filters.fire_day) {
                self.spawn_overlay(ctx, OverlaySource::FireWx(self.filters.fire_day));
            }
        }
        if actions.wssi_day_changed {
            // Day switched: the shown polygons belong to the old day until the new ones land.
            self.wssi_features.clear();
            if (1..=3).contains(&self.filters.wssi_day) {
                self.spawn_overlay(ctx, OverlaySource::Wssi(self.filters.wssi_day));
            }
        }
        if actions.overlays_changed {
            // Selecting an outlook day/kind that hasn't been fetched yet pulls it on demand.
            let day = self.filters.outlook_day;
            if (1..=8).contains(&day) && self.outlook_features[(day - 1) as usize].is_empty() {
                self.spawn_overlay(
                    ctx,
                    OverlaySource::Outlook(day, self.outlook_kind_for_day()),
                );
            }
            self.rebuild_overlays();
        }
        if actions.srv_from_cells {
            if let Some((dir, spd)) = self.scit_mean_motion() {
                let v = &mut self.views[self.active];
                v.storm_dir_deg = dir;
                v.storm_speed_kt = spd;
                v.srv = true;
            }
        }
    }
}
