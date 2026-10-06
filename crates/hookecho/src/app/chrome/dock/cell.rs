//! The Cell window: storms' full pictures as a workstation tool window, in place of the floating
//! attributes card. Sections fold; the trends are interactive (hover reads a scan, click
//! expands). Beyond the SCIT attributes it carries what an analyst otherwise gathers by hand:
//!
//! - **Severity** — the composite score and the evidence behind it (`wxdata::cellscore`), the same
//!   breakdown the storm table's hover gives.
//! - **Core statistics** — every gate of the displayed tilt within [`CORE_KM`] of the cell,
//!   across every moment (`wxdata::regionstats`, the Region statistics tool's own engine, centred
//!   on the storm): strongest echo, and ZDR, KDP and CC read from the ≥40 dBZ core, the
//!   strongest inbound and outbound velocity and the gate-to-gate spread between them.
//! - **Forecast track** — SCIT's 15–60 minute positions with the clock time of each.
//! - **Trends** over time, starting with the site's earlier scans (`merge_cell_history`), not
//!   only the volumes seen since the app started.
//!
//! Several storms can be open at once: Details… opens one, and selecting another storm while the
//! window is open adds it (up to [`MAX_OPEN`]). Chips switch between them, and **Compare** puts
//! them side by side — a table with the most severe value of each row marked, and the trends as
//! one chart per quantity with a line per storm.

use super::*;
use crate::ui::a11y::Named as _;
use egui_phosphor::regular as ph;
use wxdata::level2::Moment;
use wxdata::level3::Cell;
use wxdata::regionstats::RegionSamples;

pub(super) const CELL_W: f32 = 320.0;
/// Radius of the core-statistics box around the cell centroid.
const CORE_KM: f64 = 8.0;
/// Most storms the window keeps open; opening another closes the oldest.
pub(super) const MAX_OPEN: usize = 6;
/// A line colour per open storm, in the order they were opened.
const LINE_COLORS: [egui::Color32; MAX_OPEN] = [
    egui::Color32::from_rgb(88, 166, 255),
    egui::Color32::from_rgb(255, 159, 67),
    egui::Color32::from_rgb(63, 207, 142),
    egui::Color32::from_rgb(242, 106, 170),
    egui::Color32::from_rgb(240, 214, 70),
    egui::Color32::from_rgb(150, 130, 255),
];

/// What the Cell window's buttons asked for.
enum CellAct {
    Follow,
    View3d,
    Center,
    /// Start a manual storm-motion track from SCIT's motion, to adjust by hand.
    TrackManually,
    /// Show this open storm (select it on the map).
    Focus(String),
    Compare,
    /// Close this storm from the window.
    Close(String),
}

/// One row of the core-statistics table: the label, the value, and whether it is notable.
pub(super) fn core_rows(s: &RegionSamples) -> Vec<(&'static str, String, bool)> {
    let summary = |m: Moment| s.index_of(m).and_then(|i| s.summary(i));
    let mut rows = vec![("Gates", s.rows.len().to_string(), false)];
    if let Some(v) = summary(Moment::Reflectivity) {
        rows.push((
            "Max dBZ",
            format!("{:.0} (90% {:.0})", v.max, v.p90),
            v.max >= 60.0,
        ));
    }
    let c = CoreNumbers::of(s);
    if let Some(v) = c.zdr {
        rows.push(("ZDR (p95)", format!("{v:.1} dB"), v >= 4.0));
    }
    if let Some(v) = c.kdp {
        rows.push(("KDP (p95)", format!("{v:.1} \u{b0}/km"), v >= 3.0));
    }
    if let Some(v) = c.cc {
        // Below 0.80 alongside strong echo is what debris and big hail look like.
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

/// The core numbers the table and the comparison share. Dual-pol extremes over the whole box are
/// the echo's ragged edge (CC 0.2, KDP 8 in noise), so those read the core only, and a
/// percentile rather than the single worst gate.
#[derive(Debug, Default, Clone, Copy)]
struct CoreNumbers {
    zdr: Option<f32>,
    kdp: Option<f32>,
    cc: Option<f32>,
    delta_v: Option<f32>,
}

impl CoreNumbers {
    fn of(s: &RegionSamples) -> CoreNumbers {
        let q = |m: Moment, at: f32| core_quantiles(s, m).map(|q| q(at));
        let delta_v = s
            .index_of(Moment::Velocity)
            .and_then(|i| s.summary(i))
            .map(|v| v.max - v.min);
        CoreNumbers {
            zdr: q(Moment::DifferentialReflectivity, 0.95),
            kdp: q(Moment::SpecificDifferentialPhase, 0.95),
            cc: q(Moment::CorrelationCoefficient, 0.05),
            delta_v,
        }
    }
}

/// Echo at least this strong is the "core" the dual-pol rows read.
const CORE_DBZ: f32 = 40.0;

/// A quantile reader over moment `m` at the core's gates (reflectivity ≥ [`CORE_DBZ`]; every
/// gate when the volume has no reflectivity). None when no such gate has a value.
fn core_quantiles(s: &RegionSamples, m: Moment) -> Option<impl Fn(f32) -> f32> {
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

/// Every gate of `sweeps` within [`CORE_KM`] of `(lon, lat)`.
fn core_at(sweeps: &[wxdata::level2::BinnedSweep], lon: f64, lat: f64) -> Option<RegionSamples> {
    let dlat = CORE_KM / 110.57;
    let dlon = CORE_KM / (111.32 * lat.to_radians().cos().max(0.1));
    let bbox = wxdata::regionstats::bbox_of([lon - dlon, lat - dlat], [lon + dlon, lat + dlat]);
    wxdata::regionstats::gather(sweeps, bbox)
}

/// The index of the most severe value in `v` (the highest, or the lowest when `low_is_worse`),
/// when at least two storms have one to compare.
fn worst(v: &[Option<f32>], low_is_worse: bool) -> Option<usize> {
    let (lo, hi) = v
        .iter()
        .flatten()
        .fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
    // Two values at least, and a difference: a tie marks nothing.
    if v.iter().flatten().count() < 2 || hi <= lo {
        return None;
    }
    v.iter()
        .enumerate()
        .filter_map(|(i, x)| x.map(|x| (i, x)))
        .max_by(|a, b| {
            let (a, b) = if low_is_worse { (b.1, a.1) } else { (a.1, b.1) };
            a.total_cmp(&b)
        })
        .map(|(i, _)| i)
}

/// A trend's points as `(Unix seconds, value)`, samples without a time or a value left out.
fn points(
    trend: &[crate::ui::cell_window::CellSample],
    f: fn(&crate::ui::cell_window::CellSample) -> Option<f32>,
) -> Vec<(f64, f32)> {
    trend
        .iter()
        .filter_map(|s| Some((s.time?.timestamp() as f64, f(s)?)))
        .collect()
}

/// The trend quantities, as `(id, title, unit, reader)`.
type Reader = fn(&crate::ui::cell_window::CellSample) -> Option<f32>;
const TRENDS: [(&str, &str, &str, Reader); 5] = [
    ("dbz", "Peak reflectivity", "dBZ", |s| s.dbz),
    ("hgt", "Height of peak dBZ", "kft", |s| s.dbz_hgt),
    ("top", "Cell top", "kft", |s| s.top),
    ("vil", "VIL", "kg/m\u{b2}", |s| s.vil),
    ("sev", "Severity", "", |s| s.severity.map(f32::from)),
];

impl HookEchoApp {
    /// Pane `idx`'s displayed tilt, every moment, binned once for any number of cores.
    fn core_sweeps(&mut self, idx: usize) -> Vec<wxdata::level2::BinnedSweep> {
        let view = &mut self.views[idx];
        let tilt = view.tilt;
        let Some(vol) = view.volume.as_mut() else {
            return Vec::new();
        };
        Moment::ALL
            .into_iter()
            .filter_map(|m| vol.binned(m, tilt, m == Moment::Velocity).ok().cloned())
            .collect()
    }

    /// The core-statistics rows (label, value) around each of `at` on pane `idx`'s displayed
    /// tilt, the same table this window shows, with the tilt binned once for all of them. The
    /// Storm Digest's brief reads its cores through this.
    pub(crate) fn core_rows_at(
        &mut self,
        idx: usize,
        at: &[(f64, f64)],
    ) -> Vec<Vec<(String, String)>> {
        if at.is_empty() {
            return Vec::new();
        }
        let sweeps = self.core_sweeps(idx);
        at.iter()
            .map(|&(lon, lat)| {
                core_at(&sweeps, lon, lat)
                    .map(|s| {
                        core_rows(&s)
                            .into_iter()
                            .map(|(l, v, _)| (l.to_string(), v))
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .collect()
    }

    /// Keep the window's open storms in step with the selection: a storm selected while the
    /// window is open joins it (the oldest leaves past [`MAX_OPEN`]), a storm SCIT no longer
    /// tracks leaves, and closing the window empties it. Every frame.
    pub(super) fn sync_open_cells(&mut self) {
        if !self.cell_details {
            self.dock.cells_open.clear();
            self.dock.cell_compare = false;
            self.dock.cell_last_sel = None;
            return;
        }
        let live: Vec<String> = self
            .active_storm_cells()
            .iter()
            .map(|c| c.id.clone())
            .collect();
        self.dock.cells_open.retain(|id| live.contains(id));
        let sel = self
            .selected_storm()
            .map(|c| c.id)
            .filter(|id| !id.is_empty());
        if sel != self.dock.cell_last_sel {
            if let Some(id) = &sel {
                let open = &mut self.dock.cells_open;
                if !open.contains(id) {
                    open.push(id.clone());
                    if open.len() > MAX_OPEN {
                        open.remove(0);
                    }
                }
                self.dock.cell_compare = false;
            }
            self.dock.cell_last_sel = sel;
        }
        if self.dock.cells_open.len() < 2 {
            self.dock.cell_compare = false;
        }
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
        let couplets: Vec<wxdata::rotation::CoupletHit> = match &self.couplet_cache {
            Some((_, (hits, ..))) => hits.clone(),
            None => Vec::new(),
        };
        let open_ids = self.dock.cells_open.clone();
        let compare = self.dock.cell_compare && open_ids.len() >= 2;
        // The storms the window holds, each with its line colour (by the order opened).
        let open: Vec<(Cell, egui::Color32)> = open_ids
            .iter()
            .enumerate()
            .filter_map(|(i, id)| {
                let cell = self
                    .active_storm_cells()
                    .iter()
                    .find(|c| &c.id == id)?
                    .clone();
                Some((cell, LINE_COLORS[i % MAX_OPEN]))
            })
            .collect();
        let sweeps = self.core_sweeps(self.active);
        let core = core_at(&sweeps, c.lon, c.lat);
        let trends: Vec<Vec<crate::ui::cell_window::CellSample>> =
            open.iter().map(|(o, _)| self.storm_trend(&o.id)).collect();
        let trend = self.storm_trend(&c.id);
        // Its persistent identity across scans (ROADMAP_PARITY M2.1).
        let in_table = self.selected_storm_live().is_some_and(|(_, live)| live);
        let gone = self.cell_popup.as_ref().is_some_and(|p| {
            matches!(
                self.dock
                    .storm_ids
                    .resolve(&p.id, p.time.map(|t| t.timestamp())),
                super::Resolved::Gone(_)
            )
        });
        // A storm SCIT no longer reports is shown as it was, and says so; its old ID may be
        // another storm's now, so nothing is looked up by it.
        let identity = if gone {
            Some(format!(
                "not in the latest SCIT table — shown as of {}",
                c.time
                    .map(|d| crate::timefmt::fmt_clock(d, tz, false))
                    .unwrap_or_else(|| "its selection".into())
            ))
        } else {
            self.dock.storm_ids.describe(&c.id)
        };
        // What it has been linked to over those scans, each by the rule that linked it.
        let evidence = if !in_table {
            Vec::new()
        } else {
            self.dock.storm_ids.evidence_lines(&c.id, |s| {
                chrono::DateTime::from_timestamp(s, 0)
                    .map(|d| crate::timefmt::fmt_clock(d, tz, false))
                    .unwrap_or_default()
            })
        };
        let explained = wxdata::cellscore::score_all_explained(
            std::slice::from_ref(&c),
            &self.probsevere,
            &couplets,
        )
        .pop();
        let open_cells: Vec<Cell> = open.iter().map(|(o, _)| o.clone()).collect();
        let scores = if compare {
            wxdata::cellscore::score_all(&open_cells, &self.probsevere, &couplets)
        } else {
            Vec::new()
        };
        let cores: Vec<CoreNumbers> = if compare {
            open.iter()
                .map(|(o, _)| {
                    core_at(&sweeps, o.lon, o.lat)
                        .map(|s| CoreNumbers::of(&s))
                        .unwrap_or_default()
                })
                .collect()
        } else {
            Vec::new()
        };
        drop(sweeps);
        let following = self
            .follow_cell
            .as_ref()
            .is_some_and(|(_, f, _)| f.id == c.id);
        let rows = super::inspector::storm_rows(&c, metric);
        // Whether the tornado detectors ran on this volume at all: "no detection" means nothing
        // only if they did.
        let (detectors_ran, tornado_lineage) = {
            let name = self.views[self.active]
                .volume
                .as_ref()
                .map(|v| v.name.clone())
                .unwrap_or_default();
            (
                self.rot_shown_cache.peek(&name).is_some()
                    || self.tds_shown_cache.peek(&name).is_some(),
                // Where Tornado ID's verdicts on this volume came from (`detection_lineage`).
                self.tornado_shown_for(&name)
                    .map(|(_, _, l)| l.lines())
                    .unwrap_or_default(),
            )
        };
        let threat = {
            let vol = self.views[self.active].volume.as_ref();
            let name = vol.map(|v| v.name.clone()).unwrap_or_default();
            let t0 = c
                .time
                .or_else(|| vol.map(|v| v.time))
                .unwrap_or_else(chrono::Utc::now);
            let rot = self
                .rot_shown_cache
                .peek(&name)
                .cloned()
                .unwrap_or_default();
            let tds = self
                .tds_shown_cache
                .peek(&name)
                .cloned()
                .unwrap_or_default();
            let circulations = self.cached_circulations(&name, &rot, &tds);
            let markers: Vec<(String, [f64; 2])> = self
                .settings
                .markers
                .iter()
                .map(|m| (m.name.clone(), [m.lon, m.lat]))
                .collect();
            threat_for(
                &c,
                &super::storm_associations::Evidence {
                    cells: self.active_storm_cells(),
                    circulations: &circulations,
                    warnings: self.active_alert_features(),
                    probsevere: &self.probsevere,
                },
                &markers,
                t0,
                metric,
            )
        };
        let mut header = ws::HeaderAction::None;
        let mut act = None;
        let mut hl = None;
        let mut pass = 0;
        let title = if compare {
            format!("Compare ({})", open.len())
        } else {
            format!("Cell {}", c.id)
        };
        let fmt_x = |x: f64| {
            chrono::DateTime::from_timestamp(x as i64, 0)
                .map(|d| crate::timefmt::fmt_clock(d, tz, false))
                .unwrap_or_default()
        };
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
                            // The open storms, and Compare once there are two.
                            if open.len() >= 2 {
                                let mut labels: Vec<&str> =
                                    open.iter().map(|(o, _)| o.id.as_str()).collect();
                                labels.push("Compare");
                                let on = if compare {
                                    Some(open.len())
                                } else {
                                    open.iter().position(|(o, _)| o.id == c.id)
                                };
                                if let Some(i) = ws::chips(ui, &t, &labels, on) {
                                    act = Some(if i == open.len() {
                                        CellAct::Compare
                                    } else {
                                        CellAct::Focus(open[i].0.id.clone())
                                    });
                                }
                                ui.add_space(6.0);
                            } else {
                                ui.label(ws::text(
                                    "Select another storm to open it here and compare.",
                                    10.5,
                                    t.text_faint,
                                ));
                            }
                            if compare {
                                compare_body(ui, &t, &open, &scores, &cores, &trends, metric, &fmt_x);
                                ui.add_space(8.0);
                                ui.label(ws::text(
                                    "Worst of each row in amber. Core rows read the displayed tilt.",
                                    10.5,
                                    t.text_faint,
                                ));
                                return;
                            }
                            ui.label(ws::text(
                                crate::ui::cell_window::track_time(c.time, 0, tz),
                                11.0,
                                t.text_dim,
                            ));
                            if let Some(e) = &explained {
                                let col = severity_color(&t, e.score);
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
                                if let Some(line) = &identity {
                                    ws::kv(ui, &t, "History", line, None);
                                }
                                for (i, line) in evidence.iter().take(8).enumerate() {
                                    ws::kv(ui, &t, if i == 0 { "Linked" } else { "" }, line, None);
                                }
                                if evidence.len() > 8 {
                                    ws::kv(ui, &t, "", &format!("and {} more", evidence.len() - 8), None);
                                }
                            });
                            // What threatens where: the tornado detection at this storm, the
                            // warnings over it, and when its motion brings it to your places.
                            let n = threat.count();
                            let badge = (n > 0).then(|| n.to_string());
                            ws::fold_section(ui, &t, "cell_threat", "Threat", badge.as_deref(), true, |ui| {
                                match &threat.tornado {
                                    Some(line) => ws::kv(ui, &t, "Tornado", line, Some(t.warn)),
                                    None if detectors_ran => ws::kv(ui, &t, "Tornado", "no uniquely associated detection", None),
                                    None => ws::kv(ui, &t, "Tornado", "off (turn on Tornado detection to run the detectors)", None),
                                }
                                if threat.tornado.is_some() {
                                    for line in &tornado_lineage {
                                        ui.label(ws::text(line, 10.5, t.text_faint));
                                    }
                                }
                                if threat.ambiguous > 0 {
                                    ui.label(ws::text(format!("{} nearby circulation match(es) ambiguous between SCIT cores", threat.ambiguous), 11.0, t.warn));
                                }
                                for details in &threat.probsevere {
                                    ui.label(ws::text(details, 11.0, t.text));
                                }
                                if threat.probsevere.is_empty() {
                                    ws::kv(ui, &t, "ProbSevere", "no containing source polygon", None);
                                }
                                if threat.warnings.is_empty() {
                                    ws::kv(ui, &t, "Warnings", "none over it", None);
                                }
                                for w in &threat.warnings {
                                    ws::kv(ui, &t, "Warning", w, Some(t.warn));
                                }
                                if threat.motion.is_none() {
                                    ws::kv(ui, &t, "Arrivals", "no SCIT motion to project", None);
                                } else if threat.etas.is_empty() {
                                    ws::kv(ui, &t, "Arrivals", "no saved place ahead within 2 h", None);
                                }
                                for (name, when, hot) in &threat.etas {
                                    ws::kv(ui, &t, name, when, hot.then_some(t.warn));
                                }
                                if let Some(m) = &threat.motion {
                                    ui.label(ws::text(m, 10.5, t.text_faint));
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
                                    // The same gates as a distribution: the Region window's histogram, centred on the
                                    // storm. Hovering a bar outlines those gates on the map.
                                    let key = ui.make_persistent_id("cell_dist_moment");
                                    let mut i = ui.ctx().data(|d| d.get_temp::<usize>(key)).unwrap_or(0);
                                    i = i.min(s.moments.len().saturating_sub(1));
                                    let name = crate::products::info(s.moments[i]).short;
                                    ws::fold_section(ui, &t, "cell_dist", "Core distribution", Some(name), false, |ui| {
                                        let labels: Vec<&str> =
                                            s.moments.iter().map(|m| crate::products::info(*m).short).collect();
                                        if let Some(j) = ws::chips(ui, &t, &labels, Some(i)) {
                                            i = j;
                                            ui.ctx().data_mut(|d| d.insert_temp(key, j));
                                        }
                                        ui.add_space(4.0);
                                        pass = ui.ctx().cumulative_pass_nr();
                                        // A cell's core is a couple of thousand gates: a fresh cache per frame is cheap.
                                        let cache = &mut crate::ui::region_stats_window::RegionCache::default();
                                        crate::ui::region_stats_window::ws_histogram(ui, &t, ("cell", &c.id), s, cache, i, &mut hl);
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
                            let n = format!("{} scans", trend.len());
                            ws::fold_section(ui, &t, "cell_trends", "Trends", Some(&n), true, |ui| {
                                if trend.len() < 2 {
                                    // Five empty charts say less than one line does.
                                    ui.label(ws::text(
                                        "Trends draw once SCIT has tracked this cell through a second volume.",
                                        10.5,
                                        t.text_faint,
                                    ));
                                    return;
                                }
                                ui.spacing_mut().item_spacing.y = 4.0;
                                for (key, name, unit, read) in TRENDS {
                                    let pts = points(&trend, read);
                                    if pts.is_empty() {
                                        continue;
                                    }
                                    let color = if key == "sev" { t.warn } else { t.accent };
                                    let one = [ws::Series { name: &c.id, color, points: &pts }];
                                    ws::series_chart(ui, &t, (key, &c.id), name, unit, &one, &fmt_x);
                                }
                                ui.label(ws::text(
                                    "Earlier scans carry SCIT's peak dBZ and its height; VIL, top and severity start with the live products.",
                                    10.5,
                                    t.text_faint,
                                ));
                            });
                            ui.add_space(8.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                if ws::button(ui, &t, if following { "Stop following" } else { "Follow" }, 0.0)
                                    .named("Keep the map on this cell as it moves")
                                    .clicked()
                                {
                                    act = Some(CellAct::Follow);
                                }
                                if ws::button(ui, &t, "Center", 0.0).clicked() {
                                    act = Some(CellAct::Center);
                                }
                                if c.mvt_deg.is_some()
                                    && c.mvt_kt.is_some()
                                    && ws::button(ui, &t, "Track manually", 0.0)
                                        .named("Start a manual motion track from SCIT's, to adjust by hand")
                                        .clicked()
                                {
                                    act = Some(CellAct::TrackManually);
                                }
                                if ws::button(ui, &t, "View in 3D", 0.0)
                                    .named("Open the 3D volume cropped to this cell")
                                    .clicked()
                                {
                                    act = Some(CellAct::View3d);
                                }
                                if open.len() >= 2
                                    && ws::button(ui, &t, &format!("Close {}", c.id), 0.0)
                                        .named("Close this storm; the others stay open")
                                        .clicked()
                                {
                                    act = Some(CellAct::Close(c.id.clone()));
                                }
                            });
                        });
                });
            },
        );
        if let (Some(h), Some(s)) = (hl, &core) {
            let pts = crate::app::region_stats::highlight_points(s, &h);
            self.region.set_highlight(pass, pts);
        }
        if header == ws::HeaderAction::Close {
            self.cell_details = false;
        }
        self.dock.apply_header(DockWin::Cell, header);
        match act {
            Some(CellAct::Follow) => self.cell_follow_toggle = true,
            Some(CellAct::View3d) => self.cell_view3d = true,
            Some(CellAct::TrackManually) => {
                self.track_cell_manually(&c);
            }
            Some(CellAct::Center) => {
                let cam = &mut self.views[self.active].camera;
                cam.center = crate::render::mercator::lonlat_to_world(c.lon, c.lat);
            }
            Some(CellAct::Focus(id)) => {
                if let Some(cell) = self.active_storm_cells().iter().find(|o| o.id == id) {
                    self.cell_popup = Some(cell.clone());
                }
                self.dock.cell_compare = false;
            }
            Some(CellAct::Compare) => self.dock.cell_compare = true,
            Some(CellAct::Close(id)) => {
                self.dock.cells_open.retain(|o| *o != id);
                let next = self.dock.cells_open.last().and_then(|n| {
                    self.active_storm_cells()
                        .iter()
                        .find(|o| &o.id == n)
                        .cloned()
                });
                match next {
                    Some(cell) => {
                        // Selecting the next one must not re-add the one just closed.
                        self.dock.cell_last_sel = Some(cell.id.clone());
                        self.cell_popup = Some(cell);
                    }
                    None => self.cell_details = false,
                }
            }
            None => {}
        }
    }
}

fn severity_color(t: &ws::Tokens, score: u8) -> egui::Color32 {
    if score >= 60 {
        t.danger
    } else if score >= 35 {
        t.warn
    } else {
        t.live
    }
}

/// The open storms side by side: a table (a column per storm, the most severe value of each row
/// in amber) and each trend as one chart with a line per storm.
#[allow(clippy::too_many_arguments)]
fn compare_body(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    open: &[(Cell, egui::Color32)],
    scores: &[u8],
    cores: &[CoreNumbers],
    trends: &[Vec<crate::ui::cell_window::CellSample>],
    metric: bool,
    fmt_x: &dyn Fn(f64) -> String,
) {
    type Row<'a> = (&'a str, Vec<Option<f32>>, &'a dyn Fn(f32) -> String, bool);
    let col = |f: &dyn Fn(usize) -> Option<f32>| (0..open.len()).map(f).collect::<Vec<_>>();
    let whole = |v: f32| format!("{v:.0}");
    let one = |v: f32| format!("{v:.1}");
    let two = |v: f32| format!("{v:.2}");
    let speed = move |v: f32| {
        if metric {
            format!("{:.0} km/h", v * 1.852)
        } else {
            format!("{:.0} mph", v * 1.150_78)
        }
    };
    let rows: Vec<Row> = vec![
        (
            "Severity",
            col(&|i| scores.get(i).map(|s| f32::from(*s))),
            &whole,
            false,
        ),
        ("Max dBZ", col(&|i| open[i].0.max_dbz), &whole, false),
        (
            "dBZ height",
            col(&|i| open[i].0.max_dbz_hgt_kft),
            &one,
            false,
        ),
        ("Top kft", col(&|i| open[i].0.top_kft), &one, false),
        ("VIL", col(&|i| open[i].0.vil), &whole, false),
        (
            "POSH %",
            col(&|i| open[i].0.posh.map(|p| p as f32)),
            &whole,
            false,
        ),
        ("Hail in", col(&|i| open[i].0.hail_in), &two, false),
        ("Speed", col(&|i| open[i].0.mvt_kt), &speed, false),
        (
            "ZDR p95",
            col(&|i| cores.get(i).and_then(|c| c.zdr)),
            &one,
            false,
        ),
        (
            "KDP p95",
            col(&|i| cores.get(i).and_then(|c| c.kdp)),
            &one,
            false,
        ),
        (
            "CC p5",
            col(&|i| cores.get(i).and_then(|c| c.cc)),
            &two,
            true,
        ),
        (
            "\u{394}V m/s",
            col(&|i| cores.get(i).and_then(|c| c.delta_v)),
            &whole,
            false,
        ),
    ];
    egui::ScrollArea::horizontal()
        .id_salt("cell_compare_table")
        .show(ui, |ui| {
            egui::Grid::new("cell_compare")
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    ui.label("");
                    for (c, color) in open {
                        ui.horizontal(|ui| {
                            ws::status_dot(ui, *color, 4.0);
                            ui.label(ws::mono(&c.id, 12.0, t.text).strong());
                        });
                    }
                    ui.end_row();
                    for (name, values, fmt, low_is_worse) in &rows {
                        if values.iter().all(Option::is_none) {
                            continue;
                        }
                        ui.label(ws::text(*name, 11.0, t.text_dim));
                        let hot = worst(values, *low_is_worse);
                        for (i, v) in values.iter().enumerate() {
                            let (text, ink) = match v {
                                Some(v) if hot == Some(i) => (fmt(*v), t.warn),
                                Some(v) => (fmt(*v), t.text),
                                None => ("\u{2014}".into(), t.text_faint),
                            };
                            ui.label(ws::mono(text, 11.5, ink));
                        }
                        ui.end_row();
                    }
                });
        });
    ws::section_rule(ui, t, "Trends");
    ui.spacing_mut().item_spacing.y = 4.0;
    for (key, name, unit, read) in TRENDS {
        let pts: Vec<Vec<(f64, f32)>> = trends.iter().map(|tr| points(tr, read)).collect();
        if pts.iter().all(Vec::is_empty) {
            continue;
        }
        let series: Vec<ws::Series> = open
            .iter()
            .zip(&pts)
            .map(|((c, color), p)| ws::Series {
                name: &c.id,
                color: *color,
                points: p,
            })
            .collect();
        ws::series_chart(ui, t, ("cmp", key), name, unit, &series, fmt_x);
    }
}

/// What threatens where, for one storm (ROADMAP_2 §2.5): the merged tornado detection at it, the
/// warnings whose polygon holds it, and when SCIT's motion brings it to each saved place.
#[derive(Debug, Default, PartialEq)]
struct Threat {
    tornado: Option<String>,
    warnings: Vec<String>,
    probsevere: Vec<String>,
    ambiguous: usize,
    /// `(place, "~21:15Z (+23 min) · passes 2 mi N", in the path)`, soonest first.
    etas: Vec<(String, String, bool)>,
    /// Where the arrival times come from, said plainly; `None` without a motion.
    motion: Option<String>,
}

impl Threat {
    fn count(&self) -> usize {
        usize::from(self.tornado.is_some()) + self.warnings.len()
    }
}

fn threat_for(
    c: &Cell,
    evidence: &super::storm_associations::Evidence<'_>,
    markers: &[(String, [f64; 2])],
    t0: chrono::DateTime<chrono::Utc>,
    metric: bool,
) -> Threat {
    use crate::app::storm_track::{compass, distance, ManualTrack};
    let at = [c.lon, c.lat];
    let km = |lon: f64, lat: f64| crate::geo::great_circle(at, [lon, lat]);
    let associations = super::storm_associations::associate(c, evidence);
    let tornado = associations.circulations.first().map(|&(i, separation)| {
        let z = &evidence.circulations[i];
        let (d, bearing) = km(z.id.lon, z.id.lat);
        let place = if d < 1.0 {
            "at the core".to_string()
        } else {
            format!("{} {} of it", distance(d, metric), compass(bearing))
        };
        format!(
            "{} · evidence {} · {} signal{}, {place}; nearest SCIT core ({} from nearest signal)",
            z.id.tier.label(),
            wxdata::evidence::out_of_100(z.id.score),
            z.members.len(),
            if z.members.len() == 1 { "" } else { "s" },
            distance(separation, metric)
        )
    });
    let mut warnings: Vec<String> = associations
        .warnings
        .iter()
        .filter_map(|&i| {
            let a = evidence.warnings[i].alert.as_ref()?;
            Some(match &a.tornado_detection {
                Some(t) => format!("{} (tornado {})", a.event, t.to_lowercase()),
                None => a.event.clone(),
            })
        })
        .collect();
    warnings.sort();
    warnings.dedup();
    let mut probsevere: Vec<String> = associations
        .probsevere
        .iter()
        .map(|&i| {
            format!(
                "{}\nAssociation: SCIT core inside source polygon; source time unavailable",
                evidence.probsevere[i].detail
            )
        })
        .collect();
    probsevere.sort();
    probsevere.dedup();
    let track = ManualTrack::from_cell(c, t0);
    let motion = track.as_ref().map(|t| {
        format!(
            "Arrivals from SCIT's motion, {:03.0}\u{b0} at {:.0} kt, rounded to the minute",
            t.bearing_deg,
            t.speed_kmh / 1.852
        )
    });
    let mut etas: Vec<(f64, String, String, bool)> = track
        .map(|t| {
            markers
                .iter()
                .filter_map(|(name, p)| {
                    let e = t.eta(*p)?;
                    let when = t0 + chrono::Duration::seconds((e.minutes * 60.0) as i64);
                    let pass = if e.closest_km < 0.5 {
                        "direct hit".to_string()
                    } else {
                        format!(
                            "passes {} {}",
                            distance(e.closest_km, metric),
                            compass(t.bearing_deg + if e.right { -90.0 } else { 90.0 })
                        )
                    };
                    Some((
                        e.minutes,
                        name.clone(),
                        format!(
                            "~{} (+{:.0} min) \u{b7} {pass}",
                            when.format("%H:%MZ"),
                            e.minutes
                        ),
                        e.in_path,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    etas.sort_by(|a, b| b.3.cmp(&a.3).then(a.0.total_cmp(&b.0)));
    Threat {
        probsevere,
        ambiguous: associations.ambiguous,
        tornado,
        warnings,
        etas: etas
            .into_iter()
            .take(5)
            .map(|(_, n, w, h)| (n, w, h))
            .collect(),
        motion,
    }
}

#[cfg(test)]
mod threat_tests {
    use super::*;

    fn cell() -> Cell {
        Cell {
            lon: -97.0,
            lat: 35.0,
            id: "K4".into(),
            mvt_deg: Some(90.0),
            mvt_kt: Some(30.0),
            ..Default::default()
        }
    }

    #[test]
    fn a_storm_names_its_tornado_its_warning_and_its_arrivals() {
        use chrono::TimeZone;
        let t0 = chrono::Utc.with_ymd_and_hms(2026, 5, 6, 21, 0, 0).unwrap();
        let c = cell();
        // A warning box around it.
        let warning = wxdata::overlay::GeoFeature {
            rings: vec![vec![
                [-97.2, 34.8],
                [-96.8, 34.8],
                [-96.8, 35.2],
                [-97.2, 35.2],
            ]],
            fill: [0; 4],
            stroke: [0; 4],
            kind: wxdata::overlay::FeatureKind::Warning,
            title: "Tornado Warning".into(),
            detail: String::new(),
            alert: Some(wxdata::overlay::AlertInfo {
                id: "tor".into(),
                event: "Tornado Warning".into(),
                headline: String::new(),
                area: String::new(),
                description: String::new(),
                instruction: String::new(),
                expires: None,
                max_hail_in: None,
                max_wind: None,
                tornado_detection: Some("OBSERVED".into()),
                damage_threat: None,
                source: None,
                motion: None,
                vtec: None,
                issued: None,
                effective: None,
            }),
        };
        // A place 28 km east (about 30 min at 30 kt), and one behind it.
        let ahead = crate::geo::destination_point([-97.0, 35.0], 90.0, 28.0);
        let behind = crate::geo::destination_point([-97.0, 35.0], 270.0, 20.0);
        let th = threat_for(
            &c,
            &super::super::storm_associations::Evidence {
                cells: std::slice::from_ref(&c),
                circulations: &[],
                warnings: &[warning],
                probsevere: &[],
            },
            &[("Home".into(), ahead), ("Work".into(), behind)],
            t0,
            false,
        );
        assert_eq!(th.tornado, None);
        assert_eq!(th.warnings, ["Tornado Warning (tornado observed)"]);
        assert_eq!(th.etas.len(), 1, "{:?}", th.etas);
        assert_eq!(th.etas[0].0, "Home");
        assert!(
            th.etas[0].1.starts_with("~21:30Z (+30 min)"),
            "{}",
            th.etas[0].1
        );
        assert!(th.etas[0].2, "in the path");
        assert_eq!(th.count(), 1);
    }

    #[test]
    fn without_motion_it_says_so_rather_than_guessing() {
        let mut c = cell();
        c.mvt_kt = None;
        let th = threat_for(
            &c,
            &super::super::storm_associations::Evidence {
                cells: std::slice::from_ref(&c),
                circulations: &[],
                warnings: &[],
                probsevere: &[],
            },
            &[("Home".into(), [-96.8, 35.0])],
            chrono::Utc::now(),
            true,
        );
        assert!(th.motion.is_none() && th.etas.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wxdata::regionstats::GateRow;

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

    #[test]
    fn the_worst_value_is_marked_only_when_there_is_something_to_compare() {
        assert_eq!(worst(&[Some(50.0), Some(62.0), None], false), Some(1));
        assert_eq!(worst(&[Some(0.95), Some(0.71)], true), Some(1));
        assert_eq!(worst(&[Some(50.0), None], false), None);
        assert_eq!(worst(&[Some(0.0), Some(0.0)], false), None);
    }

    #[test]
    fn trend_points_skip_samples_without_a_time_or_value() {
        use chrono::TimeZone;
        let s = |m: Option<u32>, dbz: Option<f32>| crate::ui::cell_window::CellSample {
            vil: None,
            top: None,
            dbz,
            severity: None,
            time: m.map(|m| chrono::Utc.with_ymd_and_hms(2026, 9, 27, 1, m, 0).unwrap()),
            dbz_hgt: None,
        };
        let p = points(
            &[
                s(Some(0), Some(50.0)),
                s(None, Some(51.0)),
                s(Some(5), None),
            ],
            |s| s.dbz,
        );
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].1, 50.0);
    }
}
