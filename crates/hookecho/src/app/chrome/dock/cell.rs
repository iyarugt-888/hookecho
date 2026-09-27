//! The Cell window: a selected storm's full picture as a workstation tool window, in place of the
//! floating attributes card. Sections fold; the trends are interactive (hover reads a scan, click
//! expands). Beyond the SCIT attributes it carries what an analyst otherwise gathers by hand:
//!
//! - **Severity** — the composite score and the evidence behind it (`wxdata::cellscore`), the same
//!   breakdown the storm table's hover gives.
//! - **Core statistics** — every gate of the displayed tilt within [`CORE_KM`] of the cell,
//!   across every moment (`wxdata::regionstats`, the Region statistics tool's own engine, centred
//!   on the storm): strongest echo, and ZDR, KDP and CC read from the ≥40 dBZ core, the strongest inbound and outbound
//!   velocity and the gate-to-gate spread between them.
//! - **Forecast track** — SCIT's 15–60 minute positions with the clock time of each.

use super::*;
use crate::ui::a11y::Named as _;
use egui_phosphor::regular as ph;
use wxdata::level2::Moment;

pub(super) const CELL_W: f32 = 320.0;
/// Radius of the core-statistics box around the cell centroid.
const CORE_KM: f64 = 8.0;

/// What the Cell window's buttons asked for.
enum CellAct {
    Follow,
    View3d,
    Center,
}

/// One row of the core-statistics table: the label, the value, and whether it is notable.
pub(super) fn core_rows(
    s: &wxdata::regionstats::RegionSamples,
) -> Vec<(&'static str, String, bool)> {
    let summary = |m: Moment| s.index_of(m).and_then(|i| s.summary(i));
    let mut rows = vec![("Gates", s.rows.len().to_string(), false)];
    if let Some(v) = summary(Moment::Reflectivity) {
        rows.push((
            "Max dBZ",
            format!("{:.0} (90% {:.0})", v.max, v.p90),
            v.max >= 60.0,
        ));
    }
    // Dual-pol extremes over the whole box are the echo's ragged edge (CC 0.2, KDP 8 in noise),
    // so these read the core only, and a percentile rather than the single worst gate.
    let core = |m: Moment| core_quantiles(s, m);
    if let Some(q) = core(Moment::DifferentialReflectivity) {
        let v = q(0.95);
        rows.push(("ZDR (p95)", format!("{v:.1} dB"), v >= 4.0));
    }
    if let Some(q) = core(Moment::SpecificDifferentialPhase) {
        let v = q(0.95);
        rows.push(("KDP (p95)", format!("{v:.1} \u{b0}/km"), v >= 3.0));
    }
    if let Some(q) = core(Moment::CorrelationCoefficient) {
        // Below 0.80 alongside strong echo is what debris and big hail look like.
        let v = q(0.05);
        rows.push(("CC (p5)", format!("{v:.2}"), v < 0.8));
    }
    if let Some(v) = summary(Moment::Velocity) {
        let delta = v.max - v.min;
        rows.push((
            "Velocity",
            format!("{:+.0} / {:+.0} m/s", v.min, v.max),
            false,
        ));
        // Inbound and outbound within the core: the rotational couplet's full spread.
        rows.push((
            "\u{394}V",
            format!("{delta:.0} m/s ({:.0} kt)", delta * 1.943_844),
            v.min < -10.0 && v.max > 10.0 && delta >= 40.0,
        ));
    }
    if let Some(v) = summary(Moment::SpectrumWidth) {
        rows.push(("Max SW", format!("{:.1} m/s", v.max), v.max >= 10.0));
    }
    rows
}

/// Echo at least this strong is the "core" the dual-pol rows read.
const CORE_DBZ: f32 = 40.0;

/// A quantile reader over moment `m` at the core's gates (reflectivity ≥ [`CORE_DBZ`]; every
/// gate when the volume has no reflectivity). None when no such gate has a value.
fn core_quantiles(
    s: &wxdata::regionstats::RegionSamples,
    m: Moment,
) -> Option<impl Fn(f32) -> f32> {
    let i = s.index_of(m)?;
    let z = s.index_of(Moment::Reflectivity);
    let mut v: Vec<f32> = s
        .rows
        .iter()
        .filter(|r| z.is_none_or(|z| r.values[z].is_some_and(|d| d >= CORE_DBZ)))
        .filter_map(|r| r.values[i])
        .filter(|x| x.is_finite())
        .collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f32::total_cmp);
    Some(move |q: f32| v[((v.len() - 1) as f32 * q).round() as usize])
}

impl HookEchoApp {
    /// Gates of pane `idx`'s displayed tilt within [`CORE_KM`] of `(lon, lat)`, every moment.
    fn cell_core(
        &mut self,
        idx: usize,
        lon: f64,
        lat: f64,
    ) -> Option<wxdata::regionstats::RegionSamples> {
        let dlat = CORE_KM / 110.57;
        let dlon = CORE_KM / (111.32 * lat.to_radians().cos().max(0.1));
        let bbox = wxdata::regionstats::bbox_of([lon - dlon, lat - dlat], [lon + dlon, lat + dlat]);
        let view = &mut self.views[idx];
        let tilt = view.tilt;
        let vol = view.volume.as_mut()?;
        let sweeps: Vec<_> = Moment::ALL
            .into_iter()
            .filter_map(|m| vol.binned(m, tilt, m == Moment::Velocity).ok().cloned())
            .collect();
        wxdata::regionstats::gather(&sweeps, bbox)
    }

    pub(super) fn dock_cell(&mut self, host: Host<'_>) {
        if !self.dock.cell.open || !self.dock.cell_available {
            return;
        }
        let Some(c) = self.selected_storm() else {
            return;
        };
        let t = self.ws_tokens();
        let tz = self.active_tz();
        let metric = self.metric_in(self.active);
        let map_rect = self.chrome_rect;
        let place = self.dock.cell.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.cell.collapsed;
        let body_h = (map_rect.height() - 60.0).clamp(200.0, 640.0);
        let trend = self.cell_trends.get(&c.id).cloned().unwrap_or_default();
        let couplets: Vec<wxdata::rotation::CoupletHit> = match &self.couplet_cache {
            Some((_, (hits, ..))) => hits.clone(),
            None => Vec::new(),
        };
        let explained = wxdata::cellscore::score_all_explained(
            std::slice::from_ref(&c),
            &self.probsevere,
            &couplets,
        )
        .pop();
        let core = self.cell_core(self.active, c.lon, c.lat);
        let following = self
            .follow_cell
            .as_ref()
            .is_some_and(|(_, f, _)| f.id == c.id);
        let rows = super::inspector::storm_rows(&c, metric);
        let mut header = ws::HeaderAction::None;
        let mut act = None;
        let title = format!("Cell {}", c.id);
        tool_window(
            host,
            ToolWindow {
                id: "dock_cell",
                place,
                width: CELL_W,
                float_at: map_rect.right_top() + egui::vec2(-CELL_W - 24.0, 12.0),
            },
            map_rect,
            &t,
            |ui| {
                header = ws::window_header(
                    ui,
                    &t,
                    ph::TORNADO,
                    &title,
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                let scroll = egui::ScrollArea::vertical()
                    .id_salt("dock_cell_scroll")
                    .auto_shrink([false, floating]);
                let scroll = if floating {
                    scroll.max_height(body_h)
                } else {
                    scroll
                };
                scroll.show(ui, |ui| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.label(ws::text(
                                crate::ui::cell_window::track_time(c.time, 0, tz),
                                11.0,
                                t.text_dim,
                            ));
                            if let Some(e) = &explained {
                                let col = if e.score >= 60 {
                                    t.danger
                                } else if e.score >= 35 {
                                    t.warn
                                } else {
                                    t.live
                                };
                                ui.horizontal(|ui| {
                                    ui.label(ws::text("Severity", 12.0, t.text_dim));
                                    ui.label(ws::mono(format!("{}", e.score), 18.0, col).strong());
                                    ui.label(ws::text("/ 100", 11.0, t.text_faint));
                                });
                            }
                            ws::fold_section(ui, &t, "cell_storm", "Storm", None, true, |ui| {
                                for (k, v, hot) in &rows {
                                    ws::kv(ui, &t, k, v, hot.then_some(t.warn));
                                }
                                if let Some(b) = c.base_kft {
                                    let lt = if c.base_below { "<" } else { "" };
                                    ws::kv(ui, &t, "Base", &format!("{lt}{b:.1} kft"), None);
                                }
                            });
                            if let Some(e) = &explained {
                                let lines = e.lines();
                                let n = format!("{}", lines.len().saturating_sub(1));
                                ws::fold_section(ui, &t, "cell_sev", "Why this score", Some(&n), false, |ui| {
                                    for l in lines.iter().skip(1) {
                                        ui.label(ws::text(l, 11.0, t.text));
                                    }
                                });
                            }
                            match &core {
                                Some(s) => {
                                    let core_rows = core_rows(s);
                                    let label = format!("{CORE_KM:.0} km");
                                    ws::fold_section(ui, &t, "cell_core", "Core statistics", Some(&label), true, |ui| {
                                        for (k, v, hot) in &core_rows {
                                            ws::kv(ui, &t, k, v, hot.then_some(t.warn));
                                        }
                                        ui.label(ws::text(
                                            "Displayed tilt around the cell center; dual-pol rows read gates \u{2265}40 dBZ",
                                            10.5,
                                            t.text_faint,
                                        ));
                                    });
                                }
                                None => {
                                    ws::section_rule(ui, &t, "Core statistics");
                                    ui.label(ws::text("No volume on this pane", 11.0, t.text_faint));
                                }
                            }
                            ws::fold_section(ui, &t, "cell_pos", "Position & track", None, false, |ui| {
                                ws::kv(ui, &t, "Lat / Lon", &format!("{:.3}, {:.3}", c.lat, c.lon), None);
                                if let (Some(r), Some(a)) = (c.range_nm, c.az_deg) {
                                    ws::kv(ui, &t, "From radar", &format!("{r:.0} NM at {a:.0}\u{b0}"), None);
                                }
                                if let Some(e) = c.fcst_err_nm {
                                    let m = c.mean_err_nm.map_or(String::new(), |m| format!(", mean {m:.1}"));
                                    ws::kv(ui, &t, "Track error", &format!("{e:.1} NM{m}"), None);
                                }
                                for p in &c.track {
                                    ws::kv(
                                        ui,
                                        &t,
                                        "",
                                        &format!(
                                            "+{} min  {}  {:.2}, {:.2}",
                                            p.minutes,
                                            crate::ui::cell_window::track_time(c.time, p.minutes, tz),
                                            p.lat,
                                            p.lon
                                        ),
                                        None,
                                    );
                                }
                            });
                            let n = format!("{}", trend.len());
                            ws::fold_section(ui, &t, "cell_trends", "Trends", Some(&n), true, |ui| {
                                let series = |f: fn(&crate::ui::cell_window::CellSample) -> Option<f32>| {
                                    trend
                                        .iter()
                                        .filter_map(|s| {
                                            f(s).map(|v| ws::TrendPoint {
                                                label: s
                                                    .time
                                                    .map(|t| crate::timefmt::fmt_clock(t, tz, false))
                                                    .unwrap_or_else(|| "scan".into()),
                                                value: v,
                                            })
                                        })
                                        .collect::<Vec<_>>()
                                };
                                if trend.len() < 2 {
                                    // Four empty charts say less than one line does.
                                    ui.label(ws::text(
                                        "Trends draw once SCIT has tracked this cell through a second volume.",
                                        10.5,
                                        t.text_faint,
                                    ));
                                    return;
                                }
                                ui.spacing_mut().item_spacing.y = 4.0;
                                ws::trend_chart(ui, &t, ("dbz", &c.id), "Peak reflectivity", "dBZ", &series(|s| s.dbz), t.accent);
                                ws::trend_chart(ui, &t, ("top", &c.id), "Cell top", "kft", &series(|s| s.top), t.accent);
                                ws::trend_chart(ui, &t, ("vil", &c.id), "VIL", "kg/m\u{b2}", &series(|s| s.vil), t.accent);
                                ws::trend_chart(
                                    ui,
                                    &t,
                                    ("sev", &c.id),
                                    "Severity",
                                    "",
                                    &series(|s| s.severity.map(f32::from)),
                                    t.warn,
                                );
                            });
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                if ws::button(ui, &t, if following { "Stop following" } else { "Follow" }, 0.0)
                                    .named("Keep the map on this cell as it moves")
                                    .clicked()
                                {
                                    act = Some(CellAct::Follow);
                                }
                                if ws::button(ui, &t, "Center", 0.0).clicked() {
                                    act = Some(CellAct::Center);
                                }
                                if ws::button(ui, &t, "View in 3D", 0.0)
                                    .named("Open the 3D volume cropped to this cell")
                                    .clicked()
                                {
                                    act = Some(CellAct::View3d);
                                }
                            });
                        });
                });
            },
        );
        if header == ws::HeaderAction::Close {
            self.cell_details = false;
        }
        self.dock.apply_header(DockWin::Cell, header);
        match act {
            Some(CellAct::Follow) => self.cell_follow_toggle = true,
            Some(CellAct::View3d) => self.cell_view3d = true,
            Some(CellAct::Center) => {
                let cam = &mut self.views[self.active].camera;
                cam.center = crate::render::mercator::lonlat_to_world(c.lon, c.lat);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wxdata::regionstats::{GateRow, RegionSamples};

    #[test]
    fn core_quantiles_ignore_weak_echo() {
        let rows = [(20.0, 0.3), (50.0, 0.97), (55.0, 0.95)]
            .into_iter()
            .map(|(z, cc)| GateRow {
                lon: 0.0,
                lat: 0.0,
                range_km: 1.0,
                az_deg: 0.0,
                values: vec![Some(z), Some(cc)],
            })
            .collect();
        let s = RegionSamples {
            moments: vec![Moment::Reflectivity, Moment::CorrelationCoefficient],
            rows,
            elevation_deg: 0.5,
            bbox: (0.0, 0.0, 1.0, 1.0),
        };
        let q = core_quantiles(&s, Moment::CorrelationCoefficient).unwrap();
        assert_eq!(q(0.0), 0.95, "the 20 dBZ edge gate's 0.3 is not the core");
    }

    #[test]
    fn core_rows_flag_a_couplet_and_low_cc() {
        let rows = (0..20)
            .map(|i| GateRow {
                lon: -97.5,
                lat: 35.0,
                range_km: 20.0 + i as f32,
                az_deg: 0.0,
                values: vec![
                    Some(40.0 + i as f32),
                    Some(if i < 10 { -30.0 } else { 28.0 }),
                    Some(if i < 3 { 0.6 } else { 0.98 }),
                ],
            })
            .collect();
        let s = RegionSamples {
            moments: vec![
                Moment::Reflectivity,
                Moment::Velocity,
                Moment::CorrelationCoefficient,
            ],
            rows,
            elevation_deg: 0.5,
            bbox: (-97.6, 34.9, -97.4, 35.1),
        };
        let rows = core_rows(&s);
        let get = |k: &str| rows.iter().find(|r| r.0 == k).unwrap().clone();
        assert_eq!(get("Gates").1, "20");
        assert!(get("\u{394}V").2, "58 m/s inbound-outbound is a couplet");
        assert!(get("CC (p5)").2, "three low-CC gates in twenty");
        assert!(!get("Max dBZ").2, "59 dBZ is under the 60 flag");
    }
}
