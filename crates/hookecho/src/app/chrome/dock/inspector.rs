//! The Inspector: a card floating over the map's top-right corner, clear of the colour scale. What
//! is on screen (product, site, VCP, tilt, valid time, age), what is under the pointer (the value
//! in the colour it is drawn in, and where that gate is), and the model block while a model layer
//! is on the map. A reading can be pinned so it stays after the pointer leaves.

use super::*;
use crate::ui::a11y::Named as _;
use egui_phosphor::regular as ph;

/// The card's width.
pub(super) const CARD_W: f32 = 304.0;
/// Room the card leaves at the map's right edge for the colour scale drawn there.
const SCALE_CLEAR: f32 = 64.0;

impl HookEchoApp {
    /// Sample the active pane's displayed sweep at `(lon, lat)`: the gate there, its value (in
    /// the moment's own units), and its geometry. `None` off the sweep or without a volume.
    pub(super) fn dock_probe(&mut self, lon: f64, lat: f64) -> Option<Probe> {
        let dealias = self.settings.dealias_velocity;
        let storm_uv = self.views[self.active].storm_motion_uv();
        let v = &mut self.views[self.active];
        let (moment, tilt) = (v.moment, v.tilt);
        // Same rule as the renderer: velocity is dealiased where it can be folded, and a TDWR's
        // Level 3 velocity already is.
        let dealias = dealias
            && moment == Moment::Velocity
            && !v.site.as_deref().is_some_and(wxdata::tdwr::is_tdwr);
        let pass_site = v.site.clone();
        let vol = v.volume.as_mut()?;
        if vol.elevations.is_empty() {
            return None;
        }
        let pass_index = vol
            .acquisition_for(pass_site.as_deref())
            .and_then(|receipt| receipt.inventory().source_attribution.clone());
        let sweep = vol.binned(moment, tilt, dealias).ok()?;
        let s = sweep.sample_at(lon, lat)?;
        Some(Probe {
            lon,
            lat,
            value: crate::app::radar_probe::relative_value(
                moment,
                s.value,
                s.azimuth_deg,
                storm_uv,
            ),
            folded: s.folded,
            azimuth_deg: s.azimuth_deg,
            range_km: s.range_km,
            beam_ft: sweep.beam_height_ft(s.range_km),
            collected_ms: s.collected_ms,
            native_row_ms: s
                .source_radial
                .map(|row| row.collected_ms)
                .filter(|&ms| ms > 0 && chrono::DateTime::from_timestamp_millis(ms).is_some()),
            native_pass: s
                .source_radial
                .zip(pass_index.as_ref())
                .map_or(wxdata::live_pass::RowPass::Unavailable, |(row, index)| {
                    index.resolve(row)
                }),
            pass_site,
            // A dealiased sweep's values run past the Nyquist velocity, so an estimate read off
            // them would be too high: it says what it was unfolded at instead.
            nyquist_mps: if dealias {
                use wxdata::dealias::NyquistSource as N;
                matches!(
                    sweep.nyquist_source,
                    N::Estimated | N::EstimatedVaries | N::EstimatedInconsistent
                )
                .then_some(sweep.nyquist_ms)
            } else {
                sweep.estimated_nyquist_mps()
            },
            nyquist_decoded_mps: sweep.row_value(&sweep.row_nyquist_mps, lon, lat),
            unambiguous_km: sweep.row_value(&sweep.row_unambiguous_km, lon, lat),
            dealiased: dealias,
            unfolded: (dealias && sweep.nyquist_ms > 0.0)
                .then_some((sweep.nyquist_ms, sweep.nyquist_source)),
        })
    }

    pub(super) fn dock_inspector(&mut self, host: Host<'_>, ctx: &egui::Context) {
        if !self.dock.inspector.open {
            return;
        }
        let t = self.ws_tokens();
        let map_rect = self.chrome_rect;
        let tz = self.active_tz();
        let metric = self.metric_in(self.active);
        let single = self.views.len() == 1;
        // The reading under the pointer, when the pointer is over a single map.
        let cam = self.views[self.active].camera;
        let pointer = ctx
            .input(|i| i.pointer.hover_pos())
            .filter(|p| map_rect.contains(*p) && single)
            .map(|p| {
                let w = cam.screen_to_world(
                    (p.x - map_rect.left(), p.y - map_rect.top()),
                    self.last_viewport,
                );
                crate::render::mercator::world_to_lonlat(w.0, w.1)
            });
        let live_probe = pointer.and_then(|(lon, lat)| self.dock_probe(lon, lat));
        if live_probe.is_some() {
            self.dock.last = live_probe.clone();
        }
        let (probe, probe_kind) = match (&self.dock.pinned, &live_probe) {
            (Some(p), _) => (Some(p.clone()), "Pinned"),
            (None, Some(p)) => (Some(p.clone()), "Under pointer"),
            (None, None) => (self.dock.last.clone(), "Last pointer reading"),
        };
        // Where the data came from: the provider serving this radar right now.
        let provider = self
            .radar_health()
            .details
            .into_iter()
            .find(|(k, _)| *k == "Active provider")
            .map(|(_, v)| v);
        let v = &self.views[self.active];
        let frame_acquisition = v.volume.as_ref().map(|volume| {
            (
                volume.name.clone(),
                volume.revision(),
                volume.acquisition_for(v.site.as_deref()).cloned(),
            )
        });
        let moment = v.moment;
        let product = crate::products::name(moment, v.srv);
        let site_id = v.site.clone();
        let site = site_id.as_deref().and_then(wxdata::sites::site_by_id);
        let radar = site.map(|s| (f64::from(s.longitude), f64::from(s.latitude)));
        let site_line = match (&site_id, site) {
            (Some(id), Some(s)) => format!("{id}  \u{b7}  {}, {}", s.city, s.state),
            (Some(id), None) => id.clone(),
            (None, _) => "\u{2014}".to_string(),
        };
        let vcp = v.volume.as_ref().map(|x| x.vcp.clone()).unwrap_or_default();
        let volume_name = v.volume.as_ref().map(|x| x.name.clone());
        let tilt = v
            .volume
            .as_ref()
            .and_then(|x| x.elevations.get(v.tilt).copied());
        let valid = v.displayed_radar_time();
        let age = valid.map(|d| humanize((chrono::Utc::now() - d).num_seconds().max(0)));
        let live = v.timeline.following && v.timeline.forecast_hour().is_none();
        let view_3d = v
            .map_3d
            .enabled
            .then_some((cam.pitch, cam.bearing, cam.zoom));
        let observed_mode = v.map_3d.enabled
            && v.map_3d.representation == crate::view::Map3dRepresentation::ObservedSweeps;
        let observed_current = v.map_3d.observed_key.as_ref().is_some_and(|key| {
            key.matches_source(v, moment, radar_products::policy(v, &self.settings))
        });
        let rows_3d = if observed_mode && !observed_current {
            vec![("Mode", v.map_3d.representation.label().into())]
        } else if v.map_3d.enabled {
            volume_3d_rows(&v.map_3d, tz)
        } else {
            Vec::new()
        };
        let observed_rows = observed_mode.then(|| {
            if moment == Moment::SpecificDifferentialPhase {
                vec![("Status", "KDP is derived; displayed on map plane".into())]
            } else {
                let coverage = v
                    .map_3d
                    .observed_coverage
                    .as_ref()
                    .filter(|_| observed_current);
                let mut rows = observed_coverage_rows(coverage, tz);
                if coverage.is_some() {
                    rows.extend(crate::ui::acquisition_inventory::receipt_rows(
                        v.map_3d
                            .observed_key
                            .as_ref()
                            .and_then(|key| key.acquisition()),
                    ));
                }
                rows
            }
        });
        let smooth_rows = self.current_smooth_key(self.active).map(|key| {
            let job = JobKey::Smooth(self.active, key.clone());
            let coverage = self.smooth_vol_coverage[self.active]
                .as_ref()
                .filter(|_| self.smooth_vol_key[self.active].as_ref() == Some(&key));
            let mut rows = map_volume_coverage_rows(
                coverage,
                &key.source,
                self.loop3d_jobs.came_up_empty(&job),
                tz,
            );
            if coverage.is_some() {
                rows.extend(crate::ui::acquisition_inventory::receipt_rows(
                    key.source.acquisition(),
                ));
            }
            rows
        });
        let iso_rows = self.current_iso_key(self.active).map(|key| {
            let job = JobKey::Iso(self.active, key.clone());
            let coverage = self.iso_mesh[self.active]
                .as_ref()
                .filter(|(accepted, _)| accepted == &key)
                .map(|(_, frame)| &frame.coverage);
            let mut rows = map_volume_coverage_rows(
                coverage,
                &key.source,
                self.loop3d_jobs.came_up_empty(&job),
                tz,
            );
            if coverage.is_some() {
                rows.extend(crate::ui::acquisition_inventory::receipt_rows(
                    key.source.acquisition(),
                ));
            }
            rows
        });
        let (disp_factor, disp_unit) = display_units(moment, &self.settings);
        let source_rows =
            radar_source_rows(moment, v.srv, disp_unit, self.settings.dealias_velocity);
        let table = self.palettes.table(moment);
        let value_line = probe.as_ref().map(|p| match p.value {
            Some(x) => {
                let c = table
                    .sample(x)
                    .map(|[r, g, b, _]| egui::Color32::from_rgb(r, g, b));
                (format!("{:.1} {disp_unit}", x * disp_factor), c)
            }
            None if p.folded => ("range folded".to_string(), Some(t.warn)),
            None => ("below threshold".to_string(), None),
        });
        let flags = probe.as_ref().map(probe_flags).unwrap_or_default();
        let nyquist = probe
            .as_ref()
            .filter(|_| moment == Moment::Velocity)
            .and_then(|p| {
                nyquist_line(p.nyquist_decoded_mps, p.nyquist_mps, disp_factor, disp_unit)
            });
        let unfolded = probe
            .as_ref()
            .and_then(|p| unfolded_line(p.unfolded, disp_factor, disp_unit));
        let unambiguous = probe
            .as_ref()
            .and_then(|p| p.unambiguous_km)
            .map(|km| format!("{km:.0} km (decoded)"));
        let model_shown = crate::model_browser::model_layers()
            .any(|layer| self.views[self.active].fields_on.contains(&layer));
        // An analysis steps by hour, a forecast by lead: the card's section and buttons say which.
        let analysis = !self.views[self.active].models.model_sel.model.has_lead();
        let model_rows = if model_shown && self.dock.model_open {
            model_card_rows(&self.model_panel_input(), tz, chrono::Utc::now())
        } else {
            Vec::new()
        };
        let local_radar_rows = if v
            .fields_on
            .iter()
            .any(|l| radar_products::LAYERS.contains(l))
        {
            let metadata = radar_products::LAYERS
                .iter()
                .filter(|l| v.fields_on.contains(l) && self.radar_field_ready(self.active, **l))
                .find_map(|l| self.fields.get(l)?.radar.as_ref());
            let mut rows = radar_coverage_rows(metadata.map(|m| &m.coverage), tz);
            if let Some(metadata) = metadata {
                rows.extend(crate::ui::acquisition_inventory::receipt_rows(
                    metadata.acquisition(),
                ));
            }
            rows
        } else {
            Vec::new()
        };
        let pinned = self.dock.pinned.is_some();
        let can_pin = pinned || self.dock.last.is_some();
        let place = self.dock.inspector.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.inspector.collapsed;
        let mut header = ws::HeaderAction::None;
        let mut want = None;
        let mut pin = false;
        let mut step_model = None;
        // The selected storm (a click on a SCIT cell), current as of the newest update.
        let storm = self.selected_storm();
        let storm_rows = storm.as_ref().map(|c| self.storm_card_rows(c, metric));
        let link_storm = self.link_storm;
        let mut storm_act = None;
        let mut body = |ui: &mut egui::Ui| {
            ui.horizontal(|ui| {
                ui.label(ws::text(product, crate::theme::FONT, t.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if live {
                        ws::badge(ui, &t, "Live", t.live);
                    } else {
                        ws::badge(ui, &t, "Archive", t.warn);
                    }
                });
            });
            ui.add_space(6.0);
            ws::kv_text(ui, &t, "Site", &site_line);
            if !vcp.is_empty() {
                ws::kv(ui, &t, "VCP", &vcp, None);
            }
            if let Some(d) = tilt {
                ws::kv(ui, &t, "Tilt", &format!("{d:.1}\u{b0}"), None);
            }
            if let Some(d) = valid {
                ws::kv(
                    ui,
                    &t,
                    "Valid",
                    &crate::timefmt::fmt_date_clock(d, tz),
                    None,
                );
            }
            if let Some(a) = &age {
                ws::kv(ui, &t, "Age", &format!("{a} ago"), None);
            }
            ui.add_space(6.0);
            ws::section_rule(ui, &t, probe_kind);
            match (&probe, &value_line) {
                (Some(p), Some((val, color))) => {
                    ws::kv(ui, &t, "Value", val, *color);
                    for (k, v) in probe_rows(cursor_readout((p.lon, p.lat), radar, metric)) {
                        ws::kv(ui, &t, k, &v, None);
                    }
                    ws::kv(ui, &t, "Beam height", &fmt_beam(p.beam_ft, metric), None);
                    if let Some(ms) = p.collected_ms {
                        if let Some(d) = chrono::DateTime::from_timestamp_millis(ms) {
                            let mut when = crate::timefmt::fmt_clock(d, tz, true);
                            // Preserve the existing bin clock; its actual recorded writer
                            // can have a different clock on overlapping input.
                            if live {
                                let now = chrono::Utc::now().timestamp_millis();
                                when.push_str(&format!("  \u{b7}  {}", radial_age(now - ms)));
                            }
                            ws::kv(
                                ui,
                                &t,
                                if p.native_row_ms.is_some_and(|row| row != ms) {
                                    "Bin maximum"
                                } else {
                                    "Sampled"
                                },
                                &when,
                                None,
                            );
                        }
                    }
                    for (label, value) in crate::ui::acquisition_inventory::gate_pass_rows(
                        p.native_pass,
                        p.pass_site.as_deref(),
                    )
                    .into_iter()
                    .chain(crate::ui::acquisition_inventory::gate_clock_rows(
                        p.native_row_ms,
                        p.collected_ms,
                    )) {
                        ui.add(
                            egui::Label::new(ws::text(
                                format!("{label}: {value}"),
                                crate::theme::FONT,
                                t.text_dim,
                            ))
                            .wrap(),
                        );
                    }
                    if let Some(n) = &nyquist {
                        ws::kv(ui, &t, "Nyquist", n, None);
                    }
                    if let Some(u) = &unfolded {
                        ws::kv(ui, &t, "Unfolded at", u, None);
                    }
                    if let Some(r) = &unambiguous {
                        ws::kv(ui, &t, "Unambiguous range", r, None);
                    }
                    if !flags.is_empty() {
                        ws::kv(ui, &t, "Quality", &flags.join(", "), Some(t.warn));
                    }
                }
                _ => {
                    ui.label(ws::text(
                        if single {
                            "Point at the radar to read a value."
                        } else {
                            "Readings need a single map pane."
                        },
                        crate::theme::FONT,
                        t.text_dim,
                    ));
                }
            }
            if let Some((pitch, bearing, zoom)) = view_3d {
                ui.add_space(6.0);
                ws::section_rule(ui, &t, "3D view");
                for (k, v) in &rows_3d {
                    ws::kv(ui, &t, k, v, None);
                }
                ws::kv(ui, &t, "Pitch", &format!("{pitch:.0}\u{b0}"), None);
                ws::kv(ui, &t, "Bearing", &format!("{bearing:.0}\u{b0}"), None);
                ws::kv(ui, &t, "Zoom", &format!("{zoom:.1}"), None);
            }
            if let Some(rows) = &smooth_rows {
                paint_coverage_rows(ui, &t, "Smooth source coverage", rows);
            }
            if let Some(rows) = &iso_rows {
                paint_coverage_rows(ui, &t, "Isosurface source coverage", rows);
            }
            if let Some(rows) = &observed_rows {
                ui.add_space(6.0);
                paint_coverage_rows(ui, &t, "Observed source coverage", rows);
            }
            if !local_radar_rows.is_empty() {
                ui.add_space(6.0);
                paint_radar_coverage(ui, &t, &local_radar_rows);
            }
            if let Some((name, revision, receipt)) = &frame_acquisition {
                crate::ui::acquisition_inventory::show_frame(
                    ui,
                    &t,
                    name,
                    *revision,
                    receipt.as_ref(),
                );
            }
            if !model_rows.is_empty() {
                ui.add_space(6.0);
                ws::section_rule(
                    ui,
                    &t,
                    if analysis {
                        "Analysis"
                    } else {
                        "Model forecast"
                    },
                );
                for (k, v) in &model_rows {
                    ws::kv(ui, &t, k.trim_end_matches(':'), v, None);
                }
                ui.horizontal(|ui| {
                    if ws::icon_button(ui, &t, ph::CARET_LEFT, "", false)
                        .named(if analysis {
                            "One hour earlier"
                        } else {
                            "One lead step earlier"
                        })
                        .clicked()
                    {
                        step_model = Some(-1i8);
                    }
                    if ws::icon_button(ui, &t, ph::CARET_RIGHT, "", false)
                        .named(if analysis {
                            "One hour later"
                        } else {
                            "One lead step later"
                        })
                        .clicked()
                    {
                        step_model = Some(1);
                    }
                });
            }
            if let (Some(c), Some(rows)) = (&storm, &storm_rows) {
                ui.add_space(6.0);
                ws::section_rule(ui, &t, &format!("Storm {}", c.id));
                for (k, v, warn) in rows {
                    ws::kv(ui, &t, k, v, warn.then_some(t.warn));
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if ws::button(ui, &t, "Center", 0.0)
                        .named("Center the map on this storm")
                        .clicked()
                    {
                        storm_act = Some(StormAct::Center);
                    }
                    let mut linked = link_storm;
                    if ws::check(ui, &t, &mut linked, "All panes")
                        .on_hover_text(
                            "Mark this storm in every pane and keep each on it as it moves",
                        )
                        .changed()
                    {
                        storm_act = Some(StormAct::Link);
                    }
                    if ws::button(ui, &t, "Details\u{2026}", 0.0)
                        .named("The storm's full attributes, trend and 3D view")
                        .clicked()
                    {
                        storm_act = Some(StormAct::Details);
                    }
                    if ws::button(ui, &t, "Clear", 0.0)
                        .named("Deselect the storm")
                        .clicked()
                    {
                        storm_act = Some(StormAct::Clear);
                    }
                });
            }
            if volume_name.is_some() || provider.is_some() {
                ui.add_space(6.0);
                ws::section_rule(ui, &t, "Source");
                for (k, val) in &source_rows {
                    ws::kv_text(ui, &t, k, val);
                }
                if let Some(n) = &volume_name {
                    ws::kv(ui, &t, "Volume", n, None);
                }
                if let Some(p) = &provider {
                    ws::kv_text(ui, &t, "Provider", p);
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if ws::button(ui, &t, "Product settings", 0.0)
                    .named("The settings of the layers that are on")
                    .clicked()
                {
                    want = Some(DockTab::Analysis);
                }
                if ws::button(ui, &t, "Site info", 0.0)
                    .named("Choose or look up a radar site")
                    .clicked()
                {
                    want = Some(DockTab::Radar);
                }
                let r = ui
                    .add_enabled_ui(can_pin, |ui| {
                        ws::icon_button(ui, &t, ph::PUSH_PIN, "", pinned)
                    })
                    .inner
                    .named_toggle(
                        if pinned {
                            "Unpin the reading"
                        } else {
                            "Pin this reading"
                        },
                        pinned,
                    );
                if r.clicked() && can_pin {
                    pin = true;
                }
            });
        };
        tool_window(
            host,
            ToolWindow {
                id: "dock_inspector",
                place,
                width: CARD_W,
                float_at: egui::pos2(
                    map_rect.right() - SCALE_CLEAR - CARD_W - 24.0,
                    map_rect.top() + 12.0,
                ),
            },
            map_rect,
            &t,
            |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                header = ws::window_header(
                    ui,
                    &t,
                    ph::INFO,
                    "Inspector",
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                let inner = egui::Frame::NONE.inner_margin(egui::Margin::symmetric(12, 10));
                if floating {
                    inner.show(ui, |ui| body(ui));
                } else {
                    // Docked, the column can be shorter than the card: it scrolls.
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| inner.show(ui, |ui| body(ui)));
                }
            },
        );
        self.dock.apply_header(DockWin::Inspector, header);
        if pin {
            self.dock.pinned = match self.dock.pinned.take() {
                Some(_) => None,
                None => self.dock.last.clone(),
            };
        }
        match want {
            Some(DockTab::Radar) => self.site_dialog = Some(Default::default()),
            Some(tab) => {
                self.dock.tab = tab;
                self.dock.filter = LayerFilter::All;
                self.dock.query.clear();
                self.dock.layers.open = true;
            }
            None => {}
        }
        if let Some(step) = step_model {
            self.apply_palette(crate::app::PaletteAction::StepModelLead(step), ctx);
        }
        match (storm_act, storm) {
            (Some(StormAct::Center), Some(c)) => {
                let cam = &mut self.views[self.active].camera;
                cam.center = crate::render::mercator::lonlat_to_world(c.lon, c.lat);
            }
            (Some(StormAct::Link), _) => self.apply_palette(
                crate::app::PaletteAction::ToggleOverlay(crate::app::OverlayToggle::LinkStorm),
                ctx,
            ),
            (Some(StormAct::Clear), _) => {
                self.cell_popup = None;
                self.cell_details = false;
            }
            (Some(StormAct::Details), _) => self.cell_details = true,
            _ => {}
        }
    }
}

/// The radar product's provenance beyond where it came from (ROADMAP_2 §9.1): whether it is
/// observed or derived, its units (and the native ones when the display converts), and what this
/// app did to it on the way to the screen.
pub(super) fn radar_source_rows(
    moment: Moment,
    srv: bool,
    disp_unit: &str,
    dealias: bool,
) -> Vec<(&'static str, String)> {
    let native = moment.units();
    // The SRV switch stays set while another moment is shown; it only means anything on velocity.
    let srv = srv && moment == Moment::Velocity;
    let derived = srv || moment == Moment::SpecificDifferentialPhase;
    let mut rows = vec![(
        "Class",
        if derived { "Derived" } else { "Observed" }.to_string(),
    )];
    rows.push((
        "Units",
        match (disp_unit, native) {
            ("", _) => "unitless (ratio)".to_string(),
            (d, n) if d == n => d.to_string(),
            (d, n) => format!("{d} (native {n})"),
        },
    ));
    let mut steps = Vec::new();
    if moment == Moment::SpecificDifferentialPhase {
        steps.push("computed from differential phase");
    }
    if moment == Moment::Velocity && dealias {
        steps.push("dealiased");
    }
    if srv {
        steps.push("storm motion subtracted");
    }
    if !steps.is_empty() {
        rows.push(("Processing", format!("By HookEcho: {}", steps.join(", "))));
    }
    rows
}

/// What the Inspector's storm section asked for.
#[derive(Clone, Copy)]
enum StormAct {
    Center,
    Link,
    Details,
    Clear,
}

/// The selected storm's rows: strength, height, water aloft, hail, rotation and motion — what
/// an analyst reads off a SCIT cell first. `true` marks a value worth a second look (a TVS or
/// meso, a 50 %+ chance of severe hail). Unknown values are left out rather than shown as dashes.
impl HookEchoApp {
    /// [`storm_rows`], and a manual motion set for this storm on its own line beside SCIT's
    /// (ROADMAP_PARITY M2.1), never in place of it. The Inspector and the Cell window show the
    /// same rows.
    pub(super) fn storm_card_rows(
        &self,
        c: &wxdata::level3::Cell,
        metric: bool,
    ) -> Vec<(&'static str, String, bool)> {
        let mut rows = storm_rows(c, metric);
        if let Some(track) = self.manual_tracks_for(c).first() {
            let tz = self.active_tz();
            let line = crate::app::storm_track::manual_motion_line(track, c.time, metric, |d| {
                crate::timefmt::fmt_clock(d, tz, false)
            });
            rows.push(("Manual motion", line, false));
        }
        rows
    }
}

pub(super) fn storm_rows(
    c: &wxdata::level3::Cell,
    metric: bool,
) -> Vec<(&'static str, String, bool)> {
    let mut rows = Vec::new();
    if let Some(dbz) = c.max_dbz {
        let at = c
            .max_dbz_hgt_kft
            .map(|h| format!(" at {}", fmt_kft(h, metric)))
            .unwrap_or_default();
        rows.push(("Max", format!("{dbz:.0} dBZ{at}"), false));
    }
    if let Some(top) = c.top_kft {
        rows.push(("Top", fmt_kft(top, metric), false));
    }
    if let Some(vil) = c.vil {
        rows.push(("VIL", format!("{vil:.0} kg/m\u{b2}"), false));
    }
    match (c.posh, c.poh) {
        (Some(s), Some(h)) => rows.push(("Hail", format!("POSH {s}% \u{b7} POH {h}%"), s >= 50)),
        (Some(s), None) => rows.push(("Hail", format!("POSH {s}%"), s >= 50)),
        (None, Some(h)) => rows.push(("Hail", format!("POH {h}%"), false)),
        (None, None) => {}
    }
    if let Some(inch) = c.hail_in.filter(|x| *x > 0.0) {
        let size = if metric {
            format!("{:.0} mm", inch * 25.4)
        } else {
            format!("{inch:.2} in")
        };
        rows.push(("Max hail", size, inch >= 1.0));
    }
    if let Some(t) = c.tvs.as_ref().filter(|t| !t.is_empty()) {
        rows.push(("TVS", t.clone(), true));
    }
    if let Some(m) = c.meso.as_ref().filter(|m| !m.is_empty()) {
        rows.push(("Meso", m.clone(), true));
    }
    if let (Some(dir), Some(kt)) = (c.mvt_deg, c.mvt_kt) {
        let speed = if metric {
            format!("{:.0} km/h", kt * 1.852)
        } else {
            format!("{:.0} mph", kt * 1.150_78)
        };
        rows.push(("Moving", format!("{} at {speed}", compass(dir)), false));
    }
    rows
}

fn fmt_kft(kft: f32, metric: bool) -> String {
    if metric {
        format!("{:.1} km", kft * 0.3048)
    } else {
        format!("{kft:.0} kft")
    }
}

/// The 16-point compass name for a bearing the storm is moving toward.
fn compass(deg: f32) -> &'static str {
    const NAMES: [&str; 16] = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW",
        "NW", "NNW",
    ];
    NAMES[((deg.rem_euclid(360.0) / 22.5).round() as usize) % 16]
}

/// A radial's age from its millisecond timestamp: tenths of a second under a hundred seconds,
/// where the difference between beams of one sweep lives, then minutes and seconds.
fn radial_age(age_ms: i64) -> String {
    let ms = age_ms.max(0);
    if ms < 100_000 {
        format!("{:.1} s old", ms as f64 / 1000.0)
    } else {
        let s = ms / 1000;
        format!("{}m {:02}s old", s / 60, s % 60)
    }
}

/// What a reading's quality notes say: the gate is range folded, or its velocity was dealiased
/// (unfolded past the Nyquist interval) before it was read.
pub(super) fn probe_flags(p: &Probe) -> Vec<&'static str> {
    let mut flags = Vec::new();
    if p.folded {
        flags.push("range folded");
    }
    if p.dealiased {
        flags.push("dealiased");
    }
    flags
}

/// The 3D block's volume rows (design plan §4): the mode, and for observed sweeps which real
/// tilts are drawn, the span of time they were collected over and how much of the beam's climb
/// is shown; for a smooth volume, its quality preset. The camera rows follow separately.
fn volume_3d_rows(
    m: &crate::view::Map3dState,
    tz: Option<wxdata::tz::Tz>,
) -> Vec<(&'static str, String)> {
    use crate::view::Map3dRepresentation as R;
    let mut rows = vec![("Mode", m.representation.label().to_string())];
    if m.representation == R::ObservedSweeps {
        // Accepted native coverage below reports retained cuts and their clocks explicitly.
        if let Some(sum) = crate::view::observed_summary(&m.observed_layers)
            .filter(|_| m.observed_coverage.is_none())
        {
            rows.push((
                "Tilts",
                format!(
                    "{}  \u{b7}  {:.1}\u{b0}\u{2013}{:.1}\u{b0}",
                    sum.tilts, sum.lowest_deg, sum.highest_deg
                ),
            ));
            // Two rows, not one "start–end": two clock times do not fit the card's value column.
            if let Some((a, b)) = sum.span {
                rows.push(("Scan start", crate::timefmt::fmt_clock(a, tz, true)));
                rows.push(("Scan span", humanize((b - a).num_seconds().max(0))));
            }
        }
        rows.push(("Beam rise", format!("{:.0}%", m.beam_rise * 100.0)));
    } else {
        let q = crate::view::quality_label(m.quality_steps)
            .map_or_else(|| format!("{} steps", m.quality_steps), str::to_string);
        rows.push(("Quality", q));
        // A display transformation, recorded so a translucent picture is not read as MIP.
        let render = match m.volume_render {
            crate::render3d::VolumeRender::Mip => "Maximum (MIP)",
            crate::render3d::VolumeRender::Translucent => "Translucent, per km",
            crate::render3d::VolumeRender::TranslucentLit => "Translucent, lit",
        };
        rows.push(("Render", render.to_string()));
    }
    if m.vertical_exaggeration > 1.0 {
        rows.push(("Vertical", format!("{:.1}\u{d7}", m.vertical_exaggeration)));
    }
    rows
}

/// Source acquisition coverage of the inputs actually allowed into the local integration.
/// Angular coverage is distinct from echo strength and does not certify a complete column.
fn paint_radar_coverage(ui: &mut egui::Ui, t: &ws::Tokens, rows: &[(&str, String)]) {
    paint_coverage_rows(ui, t, "Local radar coverage", rows);
}

fn paint_coverage_rows(ui: &mut egui::Ui, t: &ws::Tokens, title: &str, rows: &[(&str, String)]) {
    ws::section_rule(ui, t, title);
    for (key, value) in rows {
        // Both labels and values wrap. The usual Inspector rows deliberately truncate;
        // coverage qualifications must remain readable on a narrow touch dock.
        ui.horizontal_top(|ui| {
            let key_width = 88.0f32.min(ui.available_width() * 0.4);
            ui.allocate_ui_with_layout(
                egui::vec2(key_width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(key_width);
                    ui.add(egui::Label::new(ws::text(*key, crate::theme::FONT, t.text_dim)).wrap());
                },
            );
            let value_width = ui.available_width();
            ui.allocate_ui_with_layout(
                egui::vec2(value_width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(value_width);
                    ui.add(egui::Label::new(ws::text(value, crate::theme::FONT, t.text)).wrap());
                },
            );
        });
    }
}

/// Native recorded rows, never regular bins or a claim about absent sectors.
fn observed_coverage_rows(
    coverage: Option<&wxdata::level2::temporal::ObservedCoverage>,
    tz: Option<wxdata::tz::Tz>,
) -> Vec<(&'static str, String)> {
    use wxdata::level2::temporal::TemporalPolicy as P;
    let Some(c) = coverage else {
        return vec![("Status", "Waiting for matching radar inputs".into())];
    };
    let recorded: usize = c.cuts.iter().map(|cut| cut.recorded_radials).sum();
    let mut rows = vec![
        (
            "Policy",
            match c.policy {
                P::Continuous => "Continuous",
                P::StrictCurrent => "Strict current sweep",
            }
            .into(),
        ),
        ("Input cuts", c.cuts.len().to_string()),
        (
            "Retained",
            format!(
                "{} of {recorded} recorded radials",
                recorded - c.excluded_radials()
            ),
        ),
    ];
    if let Some((start, end)) = c.acquisition_range_ms().and_then(|(a, b)| {
        Some((
            chrono::DateTime::from_timestamp_millis(a)?,
            chrono::DateTime::from_timestamp_millis(b)?,
        ))
    }) {
        rows.push(("Input start", crate::timefmt::fmt_clock(start, tz, true)));
        rows.push(("Input span", humanize((end - start).num_seconds())));
    } else {
        rows.push(("Input time", "Unknown".into()));
    }
    if c.retained_older_radials() > 0 {
        rows.push((
            "Mixed passes",
            format!(
                "{} older recorded radials retained",
                c.retained_older_radials()
            ),
        ));
    }
    if c.excluded_radials() > 0 {
        rows.push((
            "Excluded",
            format!("{} recorded radials", c.excluded_radials()),
        ));
    }
    if c.unknown_time_radials() > 0 {
        rows.push((
            "Unknown clocks",
            format!(
                "{} input radial{}",
                c.unknown_time_radials(),
                if c.unknown_time_radials() == 1 {
                    ""
                } else {
                    "s"
                }
            ),
        ));
    }
    rows.extend(crate::ui::acquisition_inventory::contributor_rows(
        c.cuts.iter().map(|cut| cut.native_passes.as_ref()),
        "input recorded radials",
    ));
    for cut in &c.cuts {
        rows.push((
            "Source cut",
            format!(
                "{} / elevation {} ({:.2}\u{b0}): {} retained, {} excluded, {} unknown clocks{}",
                cut.source_cut + 1,
                cut.elevation_number,
                cut.elevation_deg,
                cut.recorded_radials - cut.excluded_radials,
                cut.excluded_radials,
                cut.unknown_time_radials,
                if cut.unselected_cut {
                    "; another timed moment cut selected"
                } else {
                    ""
                },
            ),
        ));
    }
    rows.push(("Coverage", "Absent radials not inventoried".into()));
    rows.push(("Column", "Completeness not established".into()));
    rows
}

fn map_volume_coverage_rows(
    coverage: Option<&wxdata::level2::temporal::TemporalCoverage>,
    source: &crate::loop3d::SourceKey,
    empty: bool,
    tz: Option<wxdata::tz::Tz>,
) -> Vec<(&'static str, String)> {
    let Some(coverage) = coverage.filter(|c| c.policy == source.policy) else {
        return vec![(
            "Status",
            if empty {
                "No renderable 3D result; retry in 3D controls"
            } else {
                "Waiting for matching 3D inputs; unmatched content hidden"
            }
            .into(),
        )];
    };
    let mut rows = vec![
        ("Frame", source.name.clone()),
        ("Revision", source.revision.to_string()),
    ];
    let mut details = radar_coverage_rows(Some(coverage), tz);
    for (key, _) in &mut details {
        if *key == "Input tilts" {
            *key = "Input sweeps";
        }
    }
    rows.extend(details);
    let mut moments = Vec::new();
    for sweep in &coverage.contributors {
        if !moments.contains(&sweep.moment.short_name()) {
            moments.push(sweep.moment.short_name());
        }
    }
    rows.push(("Inputs", moments.join(", ")));
    rows
}

fn radar_coverage_rows(
    coverage: Option<&wxdata::level2::temporal::TemporalCoverage>,
    tz: Option<wxdata::tz::Tz>,
) -> Vec<(&'static str, String)> {
    use wxdata::level2::temporal::TemporalPolicy;
    let Some(c) = coverage else {
        return vec![("Status", "Waiting for matching radar inputs".into())];
    };
    let mut rows = vec![
        (
            "Policy",
            match c.policy {
                TemporalPolicy::Continuous => "Continuous",
                TemporalPolicy::StrictCurrent => "Strict current sweep",
            }
            .into(),
        ),
        ("Input tilts", c.contributors.len().to_string()),
    ];
    if let Some((start, end)) = c.acquisition_range_ms().and_then(|(a, b)| {
        Some((
            chrono::DateTime::from_timestamp_millis(a)?,
            chrono::DateTime::from_timestamp_millis(b)?,
        ))
    }) {
        rows.push(("Input start", crate::timefmt::fmt_clock(start, tz, true)));
        rows.push(("Input span", humanize((end - start).num_seconds())));
    } else {
        rows.push(("Input time", "Unknown".into()));
    }
    if c.retained_older_rows() > 0 {
        rows.push((
            "Mixed passes",
            format!("{} older azimuth rows retained", c.retained_older_rows()),
        ));
    }
    if c.excluded_rows() > 0 {
        rows.push(("Excluded", format!("{} azimuth rows", c.excluded_rows())));
    }
    if c.unobserved_rows() > 0 {
        rows.push((
            "Unobserved",
            format!(
                "{} azimuth row{}",
                c.unobserved_rows(),
                if c.unobserved_rows() == 1 { "" } else { "s" }
            ),
        ));
    }
    if c.unknown_time_rows() > 0 {
        rows.push((
            "Unknown clocks",
            format!(
                "{} input azimuth row{}",
                c.unknown_time_rows(),
                if c.unknown_time_rows() == 1 { "" } else { "s" }
            ),
        ));
    }
    rows.extend(crate::ui::acquisition_inventory::contributor_rows(
        c.contributors
            .iter()
            .map(|sweep| sweep.native_passes.as_ref()),
        "input azimuth rows",
    ));
    rows.push(("Column", "Completeness not established".into()));
    rows
}

/// The cursor readout folded into the card's pairs: the bearing and distance from the radar on
/// one row, the position on another.
pub(super) fn probe_rows(readout: Vec<(&'static str, String)>) -> Vec<(&'static str, String)> {
    let get = |k: &str| {
        readout
            .iter()
            .find(|(key, _)| *key == k)
            .map(|(_, v)| v.clone())
    };
    let mut rows = Vec::new();
    if let (Some(az), Some(range)) = (get("Az"), get("Range")) {
        rows.push(("Az / Range", format!("{az}  \u{b7}  {range}")));
    }
    if let (Some(lat), Some(lon)) = (get("Lat"), get("Lon")) {
        rows.push(("Lat / Lon", format!("{lat}, {lon}")));
    }
    rows
}

/// The beam's centre height above the radar, in the units setting's scale.
pub(super) fn fmt_beam(ft: f64, metric: bool) -> String {
    if metric {
        format!("{:.2} km", ft / 3280.84)
    } else {
        format!("{:.0} ft", ft)
    }
}

/// The Nyquist velocity for a reader: the decoded value when the data carried one, else the
/// estimate read off the values, labelled as such — an estimate is never shown as decoded.
pub(crate) fn nyquist_line(
    decoded: Option<f32>,
    estimated: Option<f32>,
    factor: f32,
    unit: &str,
) -> Option<String> {
    match (decoded, estimated) {
        (Some(n), _) => Some(format!("\u{b1}{:.1} {unit} (decoded)", n * factor)),
        (None, Some(n)) => Some(format!(
            "\u{2248}\u{b1}{:.1} {unit} (estimated from values)",
            n * factor
        )),
        (None, None) => None,
    }
}

/// The interval a dealiased sweep was unfolded at, and whether the radar said so or it was read
/// off the values.
pub(crate) fn unfolded_line(
    unfolded: Option<(f32, wxdata::dealias::NyquistSource)>,
    factor: f32,
    unit: &str,
) -> Option<String> {
    let (n, source) = unfolded?;
    Some(format!(
        "\u{b1}{:.1} {unit} ({})",
        n * factor,
        source.label()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unfolding_interval_says_where_it_came_from() {
        use wxdata::dealias::NyquistSource as N;
        assert_eq!(
            unfolded_line(Some((22.11, N::Decoded)), 1.0, "m/s").as_deref(),
            Some("\u{b1}22.1 m/s (decoded)")
        );
        let varies = unfolded_line(Some((26.5, N::EstimatedVaries)), 1.943_84, "kt").unwrap();
        assert!(varies.starts_with("\u{b1}51.5 kt (estimated"), "{varies}");
        assert!(varies.contains("varies by sector"), "{varies}");
        assert_eq!(unfolded_line(None, 1.0, "m/s"), None);
    }

    fn coverage(
        policy: wxdata::level2::temporal::TemporalPolicy,
    ) -> wxdata::level2::temporal::TemporalCoverage {
        let mut sweeps = vec![wxdata::level2::BinnedSweep {
            az_bins: 8,
            gate_count: 1,
            data: vec![120, 120, 120, 120, 120, 120, 0, 120],
            bin_time_ms: vec![
                1_700_000_120_000,
                1_700_000_121_000,
                1_700_000_122_000,
                1_700_000_123_000,
                1_700_000_000_000,
                1_700_000_001_000,
                0,
                0,
            ],
            elevation_deg: 0.5,
            ..Default::default()
        }];
        wxdata::level2::temporal::prepare(&mut sweeps, policy).unwrap()
    }

    fn volume_source(policy: wxdata::level2::temporal::TemporalPolicy) -> crate::loop3d::SourceKey {
        let scan = Arc::new(
            wxdata::level2::decode_volume(
                include_bytes!(
                    "../../../../../wxdata/tests/data/corpus/mayfield-2021-first-records.ar2"
                )
                .to_vec(),
            )
            .unwrap(),
        );
        crate::loop3d::SourceKey::new(
            Some("KPAH".into()),
            "KPAH20211211_032349_V06".into(),
            7,
            &scan,
            policy,
        )
    }

    #[test]
    fn map_volume_card_keeps_dependencies_and_hides_mismatched_or_unavailable_metadata() {
        use wxdata::level2::temporal::TemporalPolicy as P;
        let source = volume_source(P::StrictCurrent);
        let mut c = coverage(P::StrictCurrent);
        let mut mask = c.contributors[0].clone();
        mask.moment = Moment::DifferentialReflectivity;
        c.contributors.push(mask);
        let rows = map_volume_coverage_rows(Some(&c), &source, false, None);
        assert!(rows.contains(&("Inputs", "REF, ZDR".into())));
        assert!(rows.contains(&("Input sweeps", "2".into())));
        assert!(rows.contains(&("Revision", "7".into())));
        assert!(rows.contains(&("Column", "Completeness not established".into())));
        let mismatch =
            map_volume_coverage_rows(Some(&coverage(P::Continuous)), &source, false, None);
        assert_eq!(mismatch.len(), 1);
        assert!(!mismatch
            .iter()
            .any(|(k, _)| *k == "Input start" || *k == "Frame"));
        assert!(map_volume_coverage_rows(None, &source, true, None)[0]
            .1
            .contains("retry"));
        for width in [240.0, 300.0] {
            let ctx = egui::Context::default();
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 700.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    paint_coverage_rows(
                        ui,
                        &ws::Tokens::new(egui::Color32::LIGHT_BLUE),
                        "Smooth source coverage",
                        &rows,
                    );
                    assert!(ui.min_rect().width() <= width);
                },
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes smooth/isosurface coverage Inspector captures"]
    fn gpu_map_volume_coverage_snapshots() {
        use wxdata::level2::temporal::TemporalPolicy as P;
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for coverage review");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m1.1/map-volume-ui");
        std::fs::create_dir_all(&destination).unwrap();
        let t = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        for name in ["continuous", "strict", "pending", "unknown", "empty"] {
            let policy = if name == "strict" {
                P::StrictCurrent
            } else {
                P::Continuous
            };
            let mut c = coverage(policy);
            let mut mask = c.contributors[0].clone();
            mask.moment = Moment::DifferentialReflectivity;
            c.contributors.push(mask);
            if name == "unknown" {
                for s in &mut c.contributors {
                    s.used_start_ms = None;
                    s.used_end_ms = None;
                }
            }
            let rows = map_volume_coverage_rows(
                (!matches!(name, "pending" | "empty")).then_some(&c),
                &volume_source(policy),
                name == "empty",
                None,
            );
            for width in [240, 300] {
                gpu.save(
                    &destination.join(format!("{name}-{width}.png")),
                    width,
                    700,
                    |ui| {
                        ws::set_touch(ui.ctx(), width == 240);
                        ws::panel_frame(&t).show(ui, |ui| {
                            ws::style_scope(ui, &t);
                            ws::window_header(ui, &t, ph::INFO, "Inspector", None, None);
                            paint_coverage_rows(
                                ui,
                                &t,
                                if name == "strict" {
                                    "Isosurface source coverage"
                                } else {
                                    "Smooth source coverage"
                                },
                                &rows,
                            );
                        });
                    },
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn radar_card_names_mixed_excluded_missing_and_unknown_inputs_without_claiming_completeness() {
        use wxdata::level2::temporal::TemporalPolicy as P;
        let continuous = radar_coverage_rows(Some(&coverage(P::Continuous)), None);
        assert!(continuous.contains(&("Mixed passes", "2 older azimuth rows retained".into())));
        assert!(continuous.contains(&("Unknown clocks", "1 input azimuth row".into())));
        let strict = radar_coverage_rows(Some(&coverage(P::StrictCurrent)), None);
        assert!(strict.contains(&("Excluded", "4 azimuth rows".into())));
        assert!(strict.contains(&("Unobserved", "1 azimuth row".into())));
        assert!(!strict.iter().any(|(k, _)| *k == "Mixed passes"));
        assert!(strict.contains(&("Column", "Completeness not established".into())));
        assert_eq!(
            radar_coverage_rows(None, None),
            [("Status", "Waiting for matching radar inputs".into())]
        );
        let mut unknown = coverage(P::Continuous);
        unknown.contributors[0].used_start_ms = None;
        unknown.contributors[0].used_end_ms = None;
        let rows = radar_coverage_rows(Some(&unknown), None);
        assert!(rows.contains(&("Input time", "Unknown".into())));
        assert!(!rows.iter().any(|(k, _)| *k == "Input start"));
    }

    #[test]
    fn radar_coverage_details_fit_narrow_docks() {
        let ctx = egui::Context::default();
        let t = ws::Tokens::new(egui::Color32::LIGHT_BLUE);
        let mut rows = radar_coverage_rows(
            Some(&coverage(
                wxdata::level2::temporal::TemporalPolicy::Continuous,
            )),
            None,
        );
        let (_, receipt) = crate::live_scan::acquisition_fixture("KTLX");
        rows.extend(crate::ui::acquisition_inventory::receipt_rows(Some(
            &receipt,
        )));
        for width in [240.0, 300.0] {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 700.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    paint_radar_coverage(ui, &t, &rows);
                    assert!(
                        ui.min_rect().width() <= width,
                        "coverage details must wrap inside the dock"
                    );
                },
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes local radar coverage Inspector captures"]
    fn gpu_derived_coverage_snapshots() {
        use wxdata::level2::temporal::TemporalPolicy as P;
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for coverage review");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m1.1/coverage-ui");
        std::fs::create_dir_all(&destination).unwrap();
        let t = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        for (name, policy) in [("continuous", P::Continuous), ("strict", P::StrictCurrent)] {
            let rows = radar_coverage_rows(Some(&coverage(policy)), None);
            for width in [240, 300] {
                for touch in [false, true] {
                    gpu.save(
                        &destination.join(format!("{name}-{width}-touch-{touch}.png")),
                        width,
                        600,
                        |ui| {
                            ws::set_touch(ui.ctx(), touch);
                            ws::panel_frame(&t).show(ui, |ui| {
                                ws::style_scope(ui, &t);
                                ws::window_header(ui, &t, ph::INFO, "Inspector", None, None);
                                paint_radar_coverage(ui, &t, &rows);
                            });
                        },
                    )
                    .unwrap();
                }
            }
        }
    }

    fn observed_coverage(
        policy: wxdata::level2::temporal::TemporalPolicy,
    ) -> wxdata::level2::temporal::ObservedCoverage {
        use wxdata::level2::temporal::{
            ObservedCoverage, ObservedCutCoverage, TemporalPolicy as P,
        };
        let strict = policy == P::StrictCurrent;
        ObservedCoverage {
            policy,
            cuts: vec![
                ObservedCutCoverage {
                    source_cut: 0,
                    elevation_number: 1,
                    elevation_deg: 0.5,
                    recorded_radials: 2,
                    excluded_radials: if strict { 2 } else { 0 },
                    older_pass_radials: 2,
                    unselected_cut: true,
                    unknown_time_radials: 0,
                    used_start_ms: (!strict).then_some(1_700_000_000_000),
                    used_end_ms: (!strict).then_some(1_700_000_001_000),
                    native_passes: None,
                },
                ObservedCutCoverage {
                    source_cut: 3,
                    elevation_number: 4,
                    elevation_deg: 0.5,
                    recorded_radials: 6,
                    excluded_radials: if strict { 2 } else { 0 },
                    older_pass_radials: 1,
                    unselected_cut: false,
                    unknown_time_radials: 1,
                    used_start_ms: Some(if strict {
                        1_700_000_120_000
                    } else {
                        1_700_000_002_000
                    }),
                    used_end_ms: Some(1_700_000_123_000),
                    native_passes: None,
                },
            ],
        }
    }

    #[test]
    fn observed_source_card_distinguishes_recorded_native_rows_from_missing_inventory() {
        use wxdata::level2::temporal::TemporalPolicy as P;
        let rows = observed_coverage_rows(Some(&observed_coverage(P::Continuous)), None);
        assert!(rows.contains(&("Retained", "8 of 8 recorded radials".into())));
        assert!(rows.contains(&("Mixed passes", "3 older recorded radials retained".into())));
        assert!(rows.contains(&("Unknown clocks", "1 input radial".into())));
        let strict = observed_coverage_rows(Some(&observed_coverage(P::StrictCurrent)), None);
        assert!(strict.contains(&("Retained", "4 of 8 recorded radials".into())));
        assert!(strict.contains(&("Excluded", "4 recorded radials".into())));
        assert!(!strict.iter().any(|(key, _)| *key == "Mixed passes"));
        assert!(strict.contains(&("Coverage", "Absent radials not inventoried".into())));
        assert!(strict.contains(&("Column", "Completeness not established".into())));
        assert_eq!(
            strict
                .iter()
                .filter(|(key, _)| *key == "Source cut")
                .count(),
            2
        );
        assert!(strict
            .iter()
            .any(|(key, value)| *key == "Source cut" && value.starts_with("4 / elevation 4")));
        assert_eq!(
            observed_coverage_rows(None, None),
            [("Status", "Waiting for matching radar inputs".into())]
        );
        let mut untimed = observed_coverage(P::Continuous);
        for cut in &mut untimed.cuts {
            cut.used_start_ms = None;
            cut.used_end_ms = None;
        }
        assert!(observed_coverage_rows(Some(&untimed), None)
            .contains(&("Input time", "Unknown".into())));
    }

    #[test]
    fn observed_source_details_wrap_in_narrow_docks() {
        use wxdata::level2::temporal::TemporalPolicy as P;
        let ctx = egui::Context::default();
        let t = ws::Tokens::new(egui::Color32::LIGHT_BLUE);
        for policy in [P::Continuous, P::StrictCurrent] {
            let rows = observed_coverage_rows(Some(&observed_coverage(policy)), None);
            for width in [240.0, 300.0] {
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 1300.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        paint_coverage_rows(ui, &t, "Observed source coverage", &rows);
                        assert!(
                            ui.min_rect().width() <= width,
                            "source cut details must wrap"
                        );
                    },
                );
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes native observed source Inspector captures"]
    fn gpu_observed_coverage_snapshots() {
        use wxdata::level2::temporal::TemporalPolicy as P;
        let gpu =
            crate::headless::ui::Snapshot::new().expect("GPU adapter for observed source review");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m1.1/observed-ui");
        std::fs::create_dir_all(&destination).unwrap();
        let t = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        let mut unknown = observed_coverage(P::Continuous);
        for cut in &mut unknown.cuts {
            cut.used_start_ms = None;
            cut.used_end_ms = None;
            cut.older_pass_radials = 0;
            cut.unselected_cut = false;
            cut.unknown_time_radials = cut.recorded_radials;
        }
        for (name, rows) in [
            (
                "continuous",
                observed_coverage_rows(Some(&observed_coverage(P::Continuous)), None),
            ),
            (
                "strict",
                observed_coverage_rows(Some(&observed_coverage(P::StrictCurrent)), None),
            ),
            ("pending", observed_coverage_rows(None, None)),
            ("unknown", observed_coverage_rows(Some(&unknown), None)),
        ] {
            for (width, touch) in [(240, true), (300, false)] {
                gpu.save(
                    &destination.join(format!("{name}-{width}-touch-{touch}.png")),
                    width,
                    1000,
                    |ui| {
                        ws::set_touch(ui.ctx(), touch);
                        ws::panel_frame(&t).show(ui, |ui| {
                            ws::style_scope(ui, &t);
                            ws::window_header(ui, &t, ph::INFO, "Inspector", None, None);
                            paint_coverage_rows(ui, &t, "Observed source coverage", &rows);
                        });
                    },
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn a_storms_rows_flag_what_needs_a_look_and_skip_what_is_unknown() {
        let c = wxdata::level3::Cell {
            id: "O7".into(),
            max_dbz: Some(62.0),
            max_dbz_hgt_kft: Some(18.0),
            posh: Some(60),
            mvt_deg: Some(45.0),
            mvt_kt: Some(20.0),
            tvs: Some("TVS".into()),
            ..Default::default()
        };
        let rows = storm_rows(&c, false);
        let keys: Vec<&str> = rows.iter().map(|r| r.0).collect();
        assert_eq!(keys, ["Max", "Hail", "TVS", "Moving"]);
        assert_eq!(rows[0].1, "62 dBZ at 18 kft");
        assert!(rows[1].2 && rows[2].2, "severe hail and a TVS are flagged");
        assert_eq!(rows[3].1, "NE at 23 mph");
        assert_eq!(compass(359.0), "N");
        assert_eq!(storm_rows(&c, true)[0].1, "62 dBZ at 5.5 km");
    }

    #[test]
    fn the_3d_block_names_what_each_mode_is_drawing() {
        use crate::view::{Map3dRepresentation as R, Map3dState};
        let keys = |m: &Map3dState| -> Vec<&str> {
            volume_3d_rows(m, None)
                .into_iter()
                .map(|(k, _)| k)
                .collect()
        };
        let mut m = Map3dState::default();
        // Before the first upload there are no tilts to count, but the mode and beam rise hold.
        assert_eq!(keys(&m), ["Mode", "Beam rise"]);
        m.observed_layers = vec![wxdata::level2::ObservedLayer {
            elevation_deg: 0.5,
            radial_count: 720,
            gate_count: 1832,
            coverage_gates: 0,
            max_value: None,
            scan_start: None,
            scan_end: None,
        }];
        m.beam_rise = 0.4;
        let rows = volume_3d_rows(&m, None);
        assert_eq!(rows[1].0, "Tilts");
        assert!(rows[1].1.starts_with("1 "), "{}", rows[1].1);
        assert_eq!(rows.last().unwrap().1, "40%");
        m.representation = R::SmoothVolume;
        m.quality_steps = crate::view::QUALITY_PRESETS[1].1;
        m.vertical_exaggeration = 2.0;
        let rows = volume_3d_rows(&m, None);
        assert_eq!(rows[0].1, "Smooth reflectivity");
        assert!(rows.contains(&("Quality", "Medium".to_string())));
        assert!(rows.iter().any(|(k, _)| *k == "Vertical"));
        assert!(rows.contains(&("Render", "Maximum (MIP)".to_string())));
        m.volume_render = crate::render3d::VolumeRender::TranslucentLit;
        let rows = volume_3d_rows(&m, None);
        assert!(rows.contains(&("Render", "Translucent, lit".to_string())));
    }

    #[test]
    fn the_card_pairs_position_and_radar_geometry() {
        let rows = probe_rows(cursor_readout(
            (-97.47, 35.40),
            Some((-97.28, 35.33)),
            false,
        ));
        assert_eq!(rows[0].0, "Az / Range");
        assert!(
            rows[0].1.contains('\u{b0}') && rows[0].1.ends_with("mi"),
            "{rows:?}"
        );
        assert_eq!(rows[1], ("Lat / Lon", "35.40, -97.47".to_string()));
        // Without a radar there is nothing to measure from, so only the position is left.
        let rows = probe_rows(cursor_readout((-97.47, 35.40), None, true));
        assert_eq!(rows, [("Lat / Lon", "35.40, -97.47".to_string())]);
    }

    #[test]
    fn a_radial_is_aged_to_the_tenth_of_a_second() {
        assert_eq!(super::radial_age(42_345), "42.3 s old");
        assert_eq!(
            super::radial_age(-5),
            "0.0 s old",
            "clock skew is not negative age"
        );
        assert_eq!(super::radial_age(192_000), "3m 12s old");
    }

    #[test]
    fn the_source_rows_say_what_was_measured_and_what_was_made() {
        let rows = |m, srv, unit, dealias| {
            radar_source_rows(m, srv, unit, dealias)
                .into_iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            rows(Moment::Reflectivity, false, "dBZ", true),
            ["Class: Observed", "Units: dBZ"]
        );
        assert_eq!(
            rows(Moment::Velocity, false, "kt", true),
            [
                "Class: Observed",
                "Units: kt (native m/s)",
                "Processing: By HookEcho: dealiased"
            ]
        );
        assert_eq!(
            rows(Moment::Velocity, true, "m/s", false),
            [
                "Class: Derived",
                "Units: m/s",
                "Processing: By HookEcho: storm motion subtracted"
            ]
        );
        assert_eq!(
            rows(Moment::Reflectivity, true, "dBZ", false),
            ["Class: Observed", "Units: dBZ"]
        );
        assert_eq!(
            rows(Moment::SpecificDifferentialPhase, false, "deg/km", false)[0],
            "Class: Derived"
        );
        assert_eq!(
            rows(Moment::CorrelationCoefficient, false, "", false)[1],
            "Units: unitless (ratio)"
        );
    }

    #[test]
    fn quality_notes_name_folding_and_dealiasing() {
        let p = Probe {
            lon: 0.0,
            lat: 0.0,
            value: None,
            folded: true,
            azimuth_deg: 0.0,
            range_km: 10.0,
            beam_ft: 500.0,
            collected_ms: None,
            native_row_ms: None,
            native_pass: wxdata::live_pass::RowPass::Unavailable,
            pass_site: None,
            nyquist_mps: None,
            nyquist_decoded_mps: None,
            unambiguous_km: None,
            dealiased: false,
            unfolded: None,
        };
        assert_eq!(probe_flags(&p), ["range folded"]);
        let q = Probe {
            folded: false,
            dealiased: true,
            ..p.clone()
        };
        assert_eq!(probe_flags(&q), ["dealiased"]);
        assert!(probe_flags(&Probe {
            dealiased: false,
            ..q
        })
        .is_empty());
    }

    #[test]
    fn beam_height_follows_the_units_setting() {
        assert_eq!(fmt_beam(3280.84, true), "1.00 km");
        assert_eq!(fmt_beam(5123.4, false), "5123 ft");
    }
    fn native_contributor_fixture() -> Vec<(&'static str, String)> {
        use wxdata::level2::temporal::PassContributors;
        use wxdata::live_pass::PassKey;
        let summary = PassContributors {
            passes: vec![
                (
                    PassKey {
                        elevation_number: 1,
                        start_ms: 1_700_000_000_000,
                    },
                    120,
                ),
                (
                    PassKey {
                        elevation_number: 1,
                        start_ms: 1_700_000_020_000,
                    },
                    240,
                ),
            ],
            unanchored_rows: 3,
            unknown_clock_rows: 2,
            unavailable_rows: 1,
        };
        let mut rows = crate::ui::acquisition_inventory::contributor_rows(
            [Some(&summary), None].into_iter(),
            "input azimuth rows",
        );
        rows.extend(crate::ui::acquisition_inventory::gate_pass_rows(
            wxdata::live_pass::RowPass::Anchored(summary.passes[0].0),
            Some("KPAH"),
        ));
        rows.extend(crate::ui::acquisition_inventory::gate_clock_rows(
            Some(1_700_000_001_000),
            Some(1_700_000_021_000),
        ));
        rows
    }

    #[test]
    fn native_contributors_and_pinned_gate_scope_wrap_in_narrow_docks() {
        let rows = native_contributor_fixture();
        assert!(rows.contains(&("Pass radar", "KPAH".into())));
        assert_eq!(
            rows.iter().filter(|(k, _)| *k == "Recorded pass").count(),
            2
        );
        assert!(rows.contains(&(
            "Unavailable inputs",
            "1 input sweep lacks exact row association".into()
        )));
        let ctx = egui::Context::default();
        let t = ws::Tokens::new(egui::Color32::LIGHT_BLUE);
        for width in [240.0, 300.0] {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 1200.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    ws::set_touch(ui.ctx(), width == 240.0);
                    ws::panel_frame(&t).show(ui, |ui| {
                        ws::style_scope(ui, &t);
                        paint_radar_coverage(ui, &t, &rows);
                        assert!(ui.min_rect().right() <= width + 1.0);
                        assert!(ui.min_rect().bottom() < 1200.0);
                    });
                },
            );
        }
    }

    #[test]
    #[ignore = "gpu: writes contributor native pass review captures"]
    fn gpu_native_contributor_snapshots() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU for native pass review");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m1.1/contributor-passes-ui");
        std::fs::create_dir_all(&destination).unwrap();
        let t = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        let known = native_contributor_fixture();
        let mut unavailable = crate::ui::acquisition_inventory::contributor_rows(
            [None].into_iter(),
            "input azimuth rows",
        );
        unavailable.extend(crate::ui::acquisition_inventory::gate_pass_rows(
            wxdata::live_pass::RowPass::UnknownClock,
            Some("KTLX"),
        ));
        for (name, rows) in [("recorded", known), ("unavailable", unavailable)] {
            for width in [240, 300] {
                gpu.save(
                    &destination.join(format!("{name}-{width}.png")),
                    width,
                    1200,
                    |ui| {
                        ws::set_touch(ui.ctx(), width == 240);
                        ws::panel_frame(&t).show(ui, |ui| {
                            ws::style_scope(ui, &t);
                            paint_radar_coverage(ui, &t, &rows);
                        });
                    },
                )
                .unwrap();
            }
        }
    }
}
