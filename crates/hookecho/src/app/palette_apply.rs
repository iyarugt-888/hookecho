//! What each PaletteAction does: the one dispatcher every surface (palette, layers, keys, phone sheet) goes through.
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Run one registry action. Every surface (drawer, pills, mobile sheets) routes through it.
    pub(crate) fn apply_palette(&mut self, action: PaletteAction, ctx: &egui::Context) {
        use AppWindow as W;
        match action {
            PaletteAction::SetMoment(m, srv) => {
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
                // ROADMAP_NEW J2: each pane keeps its own product/tilt, only the site follows —
                // this is for comparing several products of one storm, not making every pane
                // identical.
                if changed && self.link_site {
                    for (i, other) in self.views.iter_mut().enumerate() {
                        if i != active {
                            other.site = Some(id.clone());
                        }
                    }
                }
            }
            PaletteAction::SeekTime(seconds) => {
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
                    if self.link_times {
                        self.linked_analysis.select_explicit(
                            self.active,
                            self.views[self.active].site.as_deref(),
                            Some(target),
                        );
                    }
                }
            }
            PaletteAction::SetModel(model) => {
                let next = self.model_sel.with_model(model);
                self.commit_model_selection(next, true);
            }
            PaletteAction::SetModelProduct(product) => {
                let next = self.model_sel.with_product(product);
                self.commit_model_selection(next, true);
            }
            PaletteAction::ToggleModelProduct(product) => {
                let layer = product.layer();
                if self.views[self.active].fields_on.contains(&layer) {
                    self.set_field(layer, false);
                } else {
                    let next = self.model_sel.with_product(product);
                    // A row adds its layer without displacing others: HRRR reflectivity under
                    // GFS pressure contours is a normal thing to want.
                    self.commit_model_selection(next, false);
                }
            }
            PaletteAction::SetModelLead(minutes) => self.set_model_lead_min(minutes),
            PaletteAction::CompareSelected => {
                let Some((field, _)) = crate::model_browser::compare_field(self.model_sel) else {
                    return;
                };
                // Carry the lead across: comparisons read the shared forecast hour.
                if let Some((max, _)) = field.lead_hours() {
                    self.global_fcst_hour = (self.model_lead_min() / 60).min(max);
                }
                let already = self.diff_field == field && self.views[self.active].swipe_compare;
                self.diff_field = field;
                // The single-model layer would paint over the halves, so it steps aside.
                let layer = self.model_sel.layer();
                self.views[self.active].fields_on.remove(&layer);
                if already || !self.views[self.active].swipe_compare {
                    self.apply_palette(PaletteAction::ToggleCompareSwipe, ctx);
                }
            }
            // An analysis has no lead: its steps are hours, through the Hour menu's own list.
            PaletteAction::StepModelLead(steps) if !self.model_sel.model.has_lead() => {
                let model = self.model_sel.model;
                let runs = model.runs_around(self.model_run, Utc::now(), model.run_list_len());
                self.model_run = crate::model_browser::step_run(&runs, self.model_run, steps);
            }
            PaletteAction::StepModelLead(steps) => {
                // Step along the model's own published leads, which are not evenly spaced for
                // every model (the NAM 12 km and the global models thin out with lead).
                let range = self.model_sel.model.leads_for(self.model_run, Utc::now());
                let mut lead = range.clamp(self.model_lead_min());
                for _ in 0..steps.unsigned_abs() {
                    lead = range.neighbour(lead, steps > 0);
                }
                self.set_model_lead_min(lead);
            }
            PaletteAction::SetModelRun(run) => {
                self.model_run = run.and_then(|secs| DateTime::from_timestamp(secs, 0));
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
                let f = self.overlay_flag(t);
                *f = !*f;
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
            PaletteAction::GoLive => self.views[self.active].timeline.go_head(),
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
            PaletteAction::ZoomToGis => self.zoom_to_imported_gis(),
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
                let on = !OverlayToggle::PANE_LINKS
                    .iter()
                    .all(|t| *self.overlay_flag(*t));
                for t in OverlayToggle::PANE_LINKS {
                    *self.overlay_flag(t) = on;
                }
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
                    self.generate_digest();
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
}
