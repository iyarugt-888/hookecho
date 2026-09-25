//! The Inspector: a card floating over the map's top-right corner, clear of the colour scale. What
//! is on screen (product, site, VCP, tilt, valid time, age), what is under the pointer (the value
//! in the colour it is drawn in, and where that gate is), and the model block while a model layer
//! is on the map. A reading can be pinned so it stays after the pointer leaves.

use super::*;
use crate::ui::a11y::Named as _;
use egui_phosphor::regular as ph;

/// The card's width.
const CARD_W: f32 = 276.0;
/// Room the card leaves at the map's right edge for the colour scale drawn there.
const SCALE_CLEAR: f32 = 64.0;

impl HookEchoApp {
    /// Sample the active pane's displayed sweep at `(lon, lat)`: the gate there, its value (in
    /// the moment's own units), and its geometry. `None` off the sweep or without a volume.
    pub(super) fn dock_probe(&mut self, lon: f64, lat: f64) -> Option<Probe> {
        let dealias = self.settings.dealias_velocity;
        let v = &mut self.views[self.active];
        let (moment, tilt) = (v.moment, v.tilt);
        // Same rule as the renderer: velocity is dealiased where it can be folded, and a TDWR's
        // Level 3 velocity already is.
        let dealias = dealias
            && moment == Moment::Velocity
            && !v.site.as_deref().is_some_and(wxdata::tdwr::is_tdwr);
        let vol = v.volume.as_mut()?;
        if vol.elevations.is_empty() {
            return None;
        }
        let sweep = vol.binned(moment, tilt, dealias).ok()?;
        let s = sweep.sample_at(lon, lat)?;
        Some(Probe {
            lon,
            lat,
            value: s.value,
            folded: s.folded,
            azimuth_deg: s.azimuth_deg,
            range_km: s.range_km,
            beam_ft: sweep.beam_height_ft(s.range_km),
            collected_ms: s.collected_ms,
            nyquist_mps: sweep.estimated_nyquist_mps(),
            dealiased: dealias,
        })
    }

    pub(super) fn dock_inspector(&mut self, host: Host<'_>, ctx: &egui::Context) {
        if !self.dock.inspector_open {
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
        let valid = v.timeline.current().and_then(|id| id.date_time());
        let age = valid.map(|d| humanize((chrono::Utc::now() - d).num_seconds().max(0)));
        let live = v.timeline.following && v.timeline.forecast_hour().is_none();
        let view_3d = v
            .map_3d
            .enabled
            .then_some((cam.pitch, cam.bearing, cam.zoom));
        let (disp_factor, disp_unit) = display_units(moment, &self.settings);
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
            .and_then(|p| p.nyquist_mps)
            .map(|n| format!("\u{b1}{:.1} {disp_unit}", n * disp_factor));
        let model_shown = crate::model_browser::model_layers()
            .any(|layer| self.views[self.active].fields_on.contains(&layer));
        let model_rows = if model_shown && self.dock.model_open {
            model_card_rows(&self.model_panel_input(), tz, chrono::Utc::now())
        } else {
            Vec::new()
        };
        let pinned = self.dock.pinned.is_some();
        let can_pin = pinned || self.dock.last.is_some();
        let place = self.dock.inspector_place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.inspector_collapsed;
        let mut header = ws::HeaderAction::None;
        let mut want = None;
        let mut pin = false;
        let mut step_model = None;
        let mut body = |ui: &mut egui::Ui| {
            ui.horizontal(|ui| {
                ui.label(ws::text(product, 14.5, t.accent).strong());
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
                            ws::kv(
                                ui,
                                &t,
                                "Sampled",
                                &crate::timefmt::fmt_clock(d, tz, true),
                                None,
                            );
                        }
                    }
                    if let Some(n) = &nyquist {
                        ws::kv(ui, &t, "Nyquist", n, None);
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
                        12.0,
                        t.text_dim,
                    ));
                }
            }
            if let Some((pitch, bearing, zoom)) = view_3d {
                ui.add_space(6.0);
                ws::section_rule(ui, &t, "3D view");
                ws::kv(ui, &t, "Pitch", &format!("{pitch:.0}\u{b0}"), None);
                ws::kv(ui, &t, "Bearing", &format!("{bearing:.0}\u{b0}"), None);
                ws::kv(ui, &t, "Zoom", &format!("{zoom:.1}"), None);
            }
            if !model_rows.is_empty() {
                ui.add_space(6.0);
                ws::section_rule(ui, &t, "Model forecast");
                for (k, v) in &model_rows {
                    ws::kv(ui, &t, k.trim_end_matches(':'), v, None);
                }
                ui.horizontal(|ui| {
                    if ws::icon_button(ui, &t, ph::CARET_LEFT, "", false)
                        .named("One lead step earlier")
                        .clicked()
                    {
                        step_model = Some(-1i8);
                    }
                    if ws::icon_button(ui, &t, ph::CARET_RIGHT, "", false)
                        .named("One lead step later")
                        .clicked()
                    {
                        step_model = Some(1);
                    }
                });
            }
            if volume_name.is_some() || provider.is_some() {
                ui.add_space(6.0);
                ws::section_rule(ui, &t, "Source");
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
        apply_header(
            header,
            &mut self.dock.inspector_open,
            &mut self.dock.inspector_place,
            &mut self.dock.inspector_collapsed,
        );
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
                self.dock.layers_open = true;
            }
            None => {}
        }
        if let Some(step) = step_model {
            self.apply_palette(crate::app::PaletteAction::StepModelLead(step), ctx);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

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
            nyquist_mps: None,
            dealiased: false,
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
}
