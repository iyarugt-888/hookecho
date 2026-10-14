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
// The core box's half-width, and a storm's core read on every tilt.
use crate::app::cell_core::{CellCore, CORE_KM};
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
    /// Look up the Census towns in its projected path.
    FindTowns,
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
        rows.push((
            "Velocity",
            format!("{:+.0} / {:+.0} m/s", v.min, v.max),
            false,
        ));
    }
    // Inbound and outbound within the core: the rotational couplet's full spread, on the one tilt
    // where it is widest (a couplet is on one tilt; one tilt's inbound against another's outbound
    // is not one), named when the core reads several.
    if let Some((lo, hi, e)) = couplet_spread(s) {
        let delta = hi - lo;
        let at = if s.tilts().len() > 1 {
            format!(" at {e:.1}\u{b0}")
        } else {
            String::new()
        };
        rows.push((
            "\u{394}V",
            format!("{delta:.0} m/s ({:.0} kt){at}", delta * 1.943_844),
            lo < -10.0 && hi > 10.0 && delta >= 40.0,
        ));
    }
    if let Some(v) = summary(Moment::SpectrumWidth) {
        rows.push(("Max SW", format!("{:.1} m/s", v.max), v.max >= 10.0));
    }
    rows
}

/// The By tilt table of a whole-cell core, lowest tilt first: the tilt and the beam's height above
/// the radar over the cell's centre (`ground_km` from the radar, when known), then that tilt's
/// strongest echo, the core's velocity spread, lowest CC and highest ZDR (the Core statistics
/// rows' own readings and thresholds), and whether any is notable.
pub(super) fn tilt_rows(
    s: &RegionSamples,
    ground_km: Option<f64>,
    metric: bool,
) -> Vec<(String, String, bool)> {
    s.tilts()
        .into_iter()
        .filter_map(|e| {
            let t = s.tilt(e)?;
            let elev = f64::from(e);
            let height = ground_km.map(|g| {
                wxdata::xsection::beam_height_km(
                    wxdata::xsection::slant_from_ground_km(g, elev),
                    elev,
                )
            });
            let label = match height {
                Some(h) if metric => format!("{e:.1}\u{b0} \u{b7} {h:.1} km"),
                Some(h) => format!("{e:.1}\u{b0} \u{b7} {:.0} kft", h * 3.280_84),
                None => format!("{e:.1}\u{b0}"),
            };
            let summary = |m: Moment| t.index_of(m).and_then(|i| t.summary(i));
            let n = CoreNumbers::of(&t);
            // Columns as `TILT_COLUMNS` names them; "–" where the tilt has none, so they align.
            let none = || "\u{2013}".to_string();
            let mut hot = false;
            let z = summary(Moment::Reflectivity).map_or_else(none, |z| {
                hot |= z.max >= 60.0;
                format!("{:.0}", z.max)
            });
            let dv = summary(Moment::Velocity).map_or_else(none, |v| {
                let delta = v.max - v.min;
                hot |= v.min < -10.0 && v.max > 10.0 && delta >= 40.0;
                format!("{delta:.0}")
            });
            let cc = n.cc.map_or_else(none, |cc| {
                hot |= cc < 0.8;
                format!("{cc:.2}")
            });
            let zdr = n.zdr.map_or_else(none, |zdr| {
                hot |= zdr >= 4.0;
                format!("{zdr:.1}")
            });
            Some((
                label,
                format!("{z} \u{b7} {dv} \u{b7} {cc} \u{b7} {zdr}"),
                hot,
            ))
        })
        .collect()
}

/// The By tilt table's header row: what the label and each column of [`tilt_rows`] hold.
pub(super) const TILT_COLUMNS: (&str, &str) = (
    "Tilt \u{b7} height",
    "dBZ \u{b7} \u{394}V \u{b7} CC \u{b7} ZDR",
);

/// What the cell card's core covers: every tilt (how many have gates there), or the displayed
/// tilt while every tilt is still being read (tilts done, of how many) or while the loop plays.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CoreScope {
    Every(usize),
    Displayed(usize, usize),
    Playing,
}

impl CoreScope {
    /// The line under the Core statistics rows.
    fn caption(self) -> String {
        match self {
            CoreScope::Every(_) => format!(
                "Every tilt within {CORE_KM:.0} km of the cell center; dual-pol rows read gates \u{2265}40 dBZ"
            ),
            CoreScope::Displayed(done, of) if of > 0 => format!(
                "Displayed tilt around the cell center while every tilt is read ({done} of {of}); dual-pol rows read gates \u{2265}40 dBZ"
            ),
            CoreScope::Displayed(..) => {
                "Displayed tilt around the cell center while every tilt is read; dual-pol rows read gates \u{2265}40 dBZ"
                    .to_string()
            }
            CoreScope::Playing => {
                "Displayed tilt around the cell center while the loop plays; every tilt once it stops. Dual-pol rows read gates \u{2265}40 dBZ"
                    .to_string()
            }
        }
    }
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
        let delta_v = couplet_spread(s).map(|(lo, hi, _)| hi - lo);
        CoreNumbers {
            zdr: q(Moment::DifferentialReflectivity, 0.95),
            kdp: q(Moment::SpecificDifferentialPhase, 0.95),
            cc: q(Moment::CorrelationCoefficient, 0.05),
            delta_v,
        }
    }
}

/// The core's widest velocity spread on any one tilt: that tilt's most inbound and most
/// outbound value, and the tilt. One pass over the rows, which on a whole cell are tens of
/// thousands. `None` without velocity.
fn couplet_spread(s: &RegionSamples) -> Option<(f32, f32, f32)> {
    let i = s.index_of(Moment::Velocity)?;
    let mut per: Vec<(f32, f32, f32)> = Vec::new();
    for r in &s.rows {
        let Some(v) = r.values[i].filter(|v| v.is_finite()) else {
            continue;
        };
        match per.iter_mut().find(|p| p.2 == r.elevation_deg) {
            Some(p) => {
                p.0 = p.0.min(v);
                p.1 = p.1.max(v);
            }
            None => per.push((v, v, r.elevation_deg)),
        }
    }
    per.into_iter()
        .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)))
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
    wxdata::regionstats::gather(sweeps, crate::app::cell_core::core_bbox(lon, lat))
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

    /// The core-statistics rows (label, value) around each of `at` on pane `idx`'s volume, the
    /// same table this window shows: over every tilt once read (`cell_core`, asked for here), the
    /// displayed tilt's until then, binned once for all of them. The Storm Digest's brief reads
    /// its cores through this.
    pub(crate) fn core_rows_at(
        &mut self,
        idx: usize,
        at: &[(f64, f64)],
    ) -> Vec<Vec<(String, String)>> {
        if at.is_empty() {
            return Vec::new();
        }
        let whole = self.cell_cores(idx, at, None);
        let sweeps = if whole
            .iter()
            .any(|w| matches!(w, CellCore::Gathering(..) | CellCore::Paused))
        {
            self.core_sweeps(idx)
        } else {
            Vec::new()
        };
        at.iter()
            .zip(&whole)
            .map(|(&(lon, lat), w)| {
                let rows = match w {
                    CellCore::Ready(s) => Some(core_rows(s)),
                    CellCore::Empty => None,
                    CellCore::Gathering(..) | CellCore::Paused => {
                        core_at(&sweeps, lon, lat).map(|s| core_rows(&s))
                    }
                };
                rows.map(|r| r.into_iter().map(|(l, v, _)| (l.to_string(), v)).collect())
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
        // Another radar's table (another pane made active): the open storms were this one's.
        let site = self.storm_table_site().map(str::to_string);
        if site != self.dock.cells_open_site {
            self.dock.cells_open.clear();
            self.dock.cell_last_sel = None;
            self.dock.cell_compare = false;
            self.dock.cells_open_site = site;
        }
        // A new table: each open storm is carried to its cell in it, by the storm history.
        let scan = self.dock.storm_ids.fed_scan();
        if scan != self.dock.cells_open_scan {
            let then = self.dock.cells_open_scan;
            let open = std::mem::take(&mut self.dock.cells_open);
            self.dock.cells_open = self.dock.storm_ids.carry_ids(&open, then, &live);
            let last: Vec<String> = self.dock.cell_last_sel.take().into_iter().collect();
            self.dock.cell_last_sel = self.dock.storm_ids.carry_ids(&last, then, &live).pop();
            self.dock.cells_open_scan = scan;
        }
        self.dock.cells_open.retain(|id| live.contains(id));
        let sel = self
            .selected_storm()
            .map(|c| c.id)
            .filter(|id| !id.is_empty())
            .filter(|_| self.cell_popup_site.as_deref() == self.storm_table_site());
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

    /// The selected storm's trend while it is in the table: a storm that has left it, or one
    /// picked on another radar, has none to add to, and its ID may be another storm's.
    pub(super) fn card_trend(&self, c: &Cell) -> Vec<crate::ui::cell_window::CellSample> {
        match self.selected_storm_live() {
            Some((_, true)) => self.storm_trend(&c.id),
            _ => Vec::new(),
        }
    }

    /// The selected storm's persistent identity across scans (ROADMAP_PARITY M2.1). A storm
    /// SCIT no longer reports, or one picked on another radar, is shown as it was and says why;
    /// its ID may be another storm's here, so nothing is looked up by it.
    pub(super) fn card_identity(&self, c: &Cell, tz: Option<wxdata::tz::Tz>) -> Option<String> {
        let as_of = || {
            c.time
                .map(|d| crate::timefmt::fmt_clock(d, tz, false))
                .unwrap_or_else(|| "its selection".into())
        };
        if self.cell_popup.is_some() && self.cell_popup_site.as_deref() != self.storm_table_site() {
            return Some(format!(
                "picked on {} — shown as of {}",
                self.cell_popup_site.as_deref().unwrap_or("another radar"),
                as_of()
            ));
        }
        let gone = self.cell_popup.as_ref().and_then(|p| {
            match self
                .dock
                .storm_ids
                .resolve(&p.id, p.time.map(|t| t.timestamp()))
            {
                super::Resolved::Gone(storm) => Some(self.dock.storm_ids.gone_note(storm)),
                _ => None,
            }
        });
        match gone {
            Some(why) => Some(format!(
                "not in the latest SCIT table ({why}) — shown as of {}",
                as_of()
            )),
            None => self.dock.storm_ids.describe(&c.id),
        }
    }

    pub(super) fn dock_cell(&mut self, host: Host<'_>) {
        let ctx = match &host {
            Host::Docked(ui) => ui.ctx().clone(),
            Host::Floating(c) => (*c).clone(),
        };
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
        // Every tilt's gates around this storm (and, comparing, each open one), read off the UI
        // thread (`cell_core`); the displayed tilt's stand in until they are.
        let mut centres = vec![(c.lon, c.lat)];
        if compare {
            centres.extend(open.iter().map(|(o, _)| (o.lon, o.lat)));
        }
        let whole = self.cell_cores(self.active, &centres, Some(&ctx));
        let gathering = whole
            .iter()
            .any(|w| matches!(w, CellCore::Gathering(..) | CellCore::Paused));
        let sweeps = if gathering {
            self.core_sweeps(self.active)
        } else {
            Vec::new()
        };
        let (core, core_scope): (Option<Arc<RegionSamples>>, CoreScope) = match &whole[0] {
            CellCore::Ready(s) => (Some(Arc::clone(s)), CoreScope::Every(s.tilts().len())),
            CellCore::Empty => (None, CoreScope::Every(0)),
            CellCore::Gathering(done, of) => (
                core_at(&sweeps, c.lon, c.lat).map(Arc::new),
                CoreScope::Displayed(*done, *of),
            ),
            CellCore::Paused => (
                core_at(&sweeps, c.lon, c.lat).map(Arc::new),
                CoreScope::Playing,
            ),
        };
        // Its rows, worked out once per core: a whole cell is tens of thousands of gates.
        let (core_rows_now, by_tilt) = match (&core, &core_scope) {
            (Some(s), CoreScope::Every(_)) => {
                let key = (Arc::as_ptr(s) as usize, metric);
                let fresh = self.whole_cores.card.as_ref().is_none_or(|r| r.key != key);
                if fresh {
                    let ground_km = self.views[self.active]
                        .site
                        .as_deref()
                        .and_then(wxdata::sites::site_by_id)
                        .map(|r| {
                            crate::geo::great_circle(
                                [f64::from(r.longitude), f64::from(r.latitude)],
                                [c.lon, c.lat],
                            )
                            .0
                        });
                    self.whole_cores.card = Some(crate::app::cell_core::CardRows {
                        key,
                        core: core_rows(s),
                        tilts: tilt_rows(s, ground_km, metric),
                    });
                }
                let r = self.whole_cores.card.as_ref().expect("just set");
                (r.core.clone(), r.tilts.clone())
            }
            (Some(s), _) => (core_rows(s), Vec::new()),
            (None, _) => (Vec::new(), Vec::new()),
        };
        let trends: Vec<Vec<crate::ui::cell_window::CellSample>> =
            open.iter().map(|(o, _)| self.storm_trend(&o.id)).collect();
        let trend = self.card_trend(&c);
        let in_table = self.selected_storm_live().is_some_and(|(_, live)| live);
        let identity = self.card_identity(&c, tz);
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
            self.active_probsevere(),
            &couplets,
        )
        .pop();
        let open_cells: Vec<Cell> = open.iter().map(|(o, _)| o.clone()).collect();
        let scores = if compare {
            wxdata::cellscore::score_all(&open_cells, self.active_probsevere(), &couplets)
        } else {
            Vec::new()
        };
        // Compared on every tilt once every open storm is read, else all on the displayed tilt:
        // never one storm's whole cell against another's single slice.
        let cores: Vec<CoreNumbers> = if compare {
            open.iter()
                .zip(&whole[1..])
                .map(|((o, _), w)| match w {
                    CellCore::Ready(s) if !gathering => CoreNumbers::of(s),
                    CellCore::Empty if !gathering => CoreNumbers::default(),
                    _ => core_at(&sweeps, o.lon, o.lat)
                        .map(|s| CoreNumbers::of(&s))
                        .unwrap_or_default(),
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
        let rows = self.storm_card_rows(&c, metric);
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
        // Towns in its path (M2.3), searched along the manual motion when one is set, else SCIT's.
        let search = {
            let vol = self.views[self.active].volume.as_ref();
            let t0 = c
                .time
                .or_else(|| vol.map(|v| v.time))
                .unwrap_or_else(chrono::Utc::now);
            self.manual_tracks_for(&c)
                .first()
                .map(|t| (*t).clone())
                .or_else(|| crate::app::storm_track::ManualTrack::from_cell(&c, t0))
        };
        let (town_targets, towns) = match &search {
            Some(track) => {
                let (t, state) = self.town_targets(track);
                (t, Some(state.cloned()))
            }
            None => (Vec::new(), None),
        };
        // ProbSevere is not kept for past times (`active_probsevere`): an archived storm's card
        // says so, rather than that no polygon contains it.
        let probsevere_archived = self.archive_bucket().is_some();
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
            let (mut targets, targets_dropped) = self.impact_targets();
            targets.extend(town_targets);
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
                    probsevere: self.active_probsevere(),
                },
                &markers,
                t0,
                metric,
                self.manual_tracks_for(&c).first().copied(),
                (&targets, targets_dropped),
            )
        };
        let mut header = ws::HeaderAction::None;
        let mut act = None;
        let mut hl = None;
        let mut card_cache = std::mem::take(&mut self.whole_cores.card_cache);
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
                                    let (tracker, _) = wxdata::celltrack::tracker_of(&c.id);
                                    ui.label(ws::text(format!("{} nearby circulation match(es) ambiguous between {tracker} cores", threat.ambiguous), 11.0, t.warn));
                                }
                                for details in &threat.probsevere {
                                    ui.label(ws::text(details, 11.0, t.text));
                                }
                                if probsevere_archived {
                                    ws::kv(ui, &t, "ProbSevere", "not kept for past times", None);
                                } else if threat.probsevere.is_empty() {
                                    ws::kv(ui, &t, "ProbSevere", "no containing source polygon", None);
                                }
                                if threat.warnings.is_empty() {
                                    ws::kv(ui, &t, "Warnings", "none over it", None);
                                }
                                for w in &threat.warnings {
                                    ws::kv(ui, &t, "Warning", w, Some(t.warn));
                                }
                                if threat.motion.is_none() {
                                    let (tracker, _) = wxdata::celltrack::tracker_of(&c.id);
                                    ws::kv(ui, &t, "Arrivals", &format!("no {tracker} motion to project"), None);
                                } else if threat.etas.is_empty() {
                                    ws::kv(ui, &t, "Arrivals", "no saved place ahead within 2 h", None);
                                }
                                for (name, when, hot) in &threat.etas {
                                    ws::kv(ui, &t, name, when, hot.then_some(t.warn));
                                }
                                if let Some(m) = &threat.motion {
                                    ui.label(ws::text(m, 10.5, t.text_faint));
                                }
                                for (name, when, hot) in &threat.target_etas {
                                    ws::kv(ui, &t, name, when, hot.then_some(t.warn));
                                }
                                if let Some(n) = &threat.target_note {
                                    ui.label(ws::text(n, 10.5, t.text_faint));
                                }
                                {
                                    use crate::app::community_targets::TownsState;
                                    match &towns {
                                        None => {}
                                        Some(None) => {
                                            if ws::button(ui, &t, "Towns in its path", 0.0)
                                                .named("Look up the Census 2020 towns its projected path touches (asks the Census Bureau's service)")
                                                .clicked()
                                            {
                                                act = Some(CellAct::FindTowns);
                                            }
                                        }
                                        Some(Some(TownsState::Pending)) => {
                                            ui.label(ws::text("Looking up towns in its path\u{2026}", 10.5, t.text_faint));
                                        }
                                        Some(Some(TownsState::Ready(p))) if p.is_empty() => {
                                            ui.label(ws::text("No Census places touch its projected path", 10.5, t.text_faint));
                                        }
                                        Some(Some(TownsState::Ready(_))) => {
                                            ui.label(ws::text(
                                                "Towns: Census 2020 places its 1-hour path touches; times are when the path enters each town's limits (a town listed at its centre had no boundary)",
                                                10.5,
                                                t.text_faint,
                                            ));
                                        }
                                        Some(Some(TownsState::Failed(e))) => {
                                            ui.label(ws::text(format!("Town lookup failed: {e}"), 10.5, t.text_faint));
                                        }
                                    }
                                }
                                if let Some(m) = &threat.manual_motion {
                                    for (name, when, hot) in &threat.manual_target_etas {
                                        ws::kv(ui, &t, name, when, hot.then_some(t.warn));
                                    }
                                    if threat.manual_etas.is_empty() {
                                        ws::kv(ui, &t, "Manual", "no saved place ahead within 2 h", None);
                                    }
                                    for (name, when, hot) in &threat.manual_etas {
                                        ws::kv(ui, &t, name, when, hot.then_some(t.warn));
                                    }
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
                                    let label = match core_scope {
                                        CoreScope::Every(n) => format!(
                                            "{CORE_KM:.0} km \u{b7} {n} tilt{}",
                                            if n == 1 { "" } else { "s" }
                                        ),
                                        CoreScope::Displayed(..) | CoreScope::Playing => {
                                            format!("{CORE_KM:.0} km \u{b7} this tilt")
                                        }
                                    };
                                    ws::fold_section(ui, &t, "cell_core", "Core statistics", Some(&label), true, |ui| {
                                        for (k, v, hot) in &core_rows_now {
                                            ws::kv(ui, &t, k, v, hot.then_some(t.warn));
                                        }
                                        ui.label(ws::text(core_scope.caption(), 10.5, t.text_faint));
                                    });
                                    // The same cell tilt by tilt, lowest first, with the beam's
                                    // height over its centre.
                                    if !by_tilt.is_empty() {
                                        let n = by_tilt.len().to_string();
                                        ws::fold_section(ui, &t, "cell_tilts", "By tilt", Some(&n), false, |ui| {
                                            ws::kv(ui, &t, TILT_COLUMNS.0, TILT_COLUMNS.1, Some(t.text_faint));
                                            for (k, v, hot) in &by_tilt {
                                                ws::kv(ui, &t, k, v, hot.then_some(t.warn));
                                            }
                                            ui.label(ws::text(
                                                "Each tilt's gates in the core box: peak dBZ, velocity spread (m/s), and the lowest CC (p5) and highest ZDR (p95) of its \u{2265}40 dBZ core; height above the radar",
                                                10.5,
                                                t.text_faint,
                                            ));
                                        });
                                    }
                                    // The same gates as a distribution: the Region window's
                                    // histogram, centred on the storm, over every tilt once read.
                                    // Hovering a bar outlines those gates, on their own tilts in 3D.
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
                                        crate::ui::region_stats_window::ws_histogram(ui, &t, ("cell", &c.id), s, &mut card_cache, i, &mut hl);
                                    });
                                }
                                None => {
                                    ws::section_rule(ui, &t, "Core statistics");
                                    let why = match core_scope {
                                        CoreScope::Every(_) => "No gates around this storm on any tilt",
                                        CoreScope::Displayed(..) | CoreScope::Playing => "No volume on this pane",
                                    };
                                    ui.label(ws::text(why, 11.0, t.text_faint));
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
                                    // Five empty charts say less than one line does. Trends are
                                    // SCIT's per-scan attributes (`cell_trends`, fed by the live
                                    // poll only); an archive's local cell has none to wait for.
                                    ui.label(ws::text(
                                        if wxdata::celltrack::is_local_id(&c.id) {
                                            "No trends for a locally tracked cell: they come from SCIT's per-scan attributes, which the archive does not keep."
                                        } else {
                                            "Trends draw once SCIT has tracked this cell through a second volume."
                                        },
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
        self.whole_cores.card_cache = card_cache;
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
            Some(CellAct::FindTowns) => {
                if let Some(track) = &search {
                    self.request_towns(track, &ctx);
                }
            }
            Some(CellAct::Center) => {
                let cam = &mut self.views[self.active].camera;
                cam.center = crate::render::mercator::lonlat_to_world(c.lon, c.lat);
            }
            Some(CellAct::Focus(id)) => {
                if let Some(cell) = self.active_storm_cells().iter().find(|o| o.id == id) {
                    self.cell_popup = Some(cell.clone());
                    self.cell_popup_site = self.storm_table_site().map(str::to_string);
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
                        self.cell_popup_site = self.storm_table_site().map(str::to_string);
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
    /// The same from a manual motion set for this storm, beside SCIT's, never merged with it.
    manual_etas: Vec<(String, String, bool)>,
    manual_motion: Option<String>,
    /// Arrivals at the imported layers' impact targets (ROADMAP_PARITY M2.3), per motion: SCIT's,
    /// then the manual one when set; and a note when targets were left out past the bounds.
    target_etas: Vec<(String, String, bool)>,
    manual_target_etas: Vec<(String, String, bool)>,
    target_note: Option<String>,
}

/// Arrival at each impact target from `track`, in-path first then soonest: a point's arrival and
/// closest approach as for a saved place; an area's entry by the storm's path, or contact by the
/// uncertainty swath's edge only, said as such; an area it is already in, said as inside.
fn target_arrivals(
    track: &crate::app::storm_track::ManualTrack,
    targets: &[crate::app::gis_layers::Target],
    scan: chrono::DateTime<chrono::Utc>,
    metric: bool,
) -> Vec<(f64, String, String, bool)> {
    use crate::app::gis_layers::TargetShape;
    let points: Vec<(String, [f64; 2])> = targets
        .iter()
        .filter_map(|t| match t.shape {
            TargetShape::Point(p) => Some((format!("{} ({})", t.name, t.layer), p)),
            TargetShape::Area(_) => None,
        })
        .collect();
    let mut out = arrivals(track, &points, scan, metric);
    for t in targets {
        let TargetShape::Area(ring) = &t.shape else {
            continue;
        };
        let Some(z) = track.zone_eta(ring) else {
            continue;
        };
        let when = track.t0 + chrono::Duration::seconds((z.minutes * 60.0) as i64);
        let rel = (when - scan).num_seconds() as f64 / 60.0;
        let at = if rel >= 0.0 {
            format!("~{} (+{rel:.0} min)", when.format("%H:%MZ"))
        } else {
            format!(
                "~{} ({:.0} min before this scan)",
                when.format("%H:%MZ"),
                -rel
            )
        };
        let text = if z.grazes {
            format!("{at} \u{b7} only the swath's edge reaches it")
        } else if z.minutes == 0.0 {
            format!(
                "inside at the motion's time ({})",
                track.t0.format("%H:%MZ")
            )
        } else {
            format!("{at} \u{b7} path enters")
        };
        out.push((rel, format!("{} ({})", t.name, t.layer), text, !z.grazes));
    }
    out.sort_by(|a, b| b.3.cmp(&a.3).then(a.0.total_cmp(&b.0)));
    out
}

/// Arrival at each saved place from `track`, soonest in-path first: `(minutes after the scan,
/// place, "~21:15Z (+23 min) · passes 2 mi N", in the path)`. Times are from the track's own
/// analysis time, stated against the scan on screen.
fn arrivals(
    track: &crate::app::storm_track::ManualTrack,
    markers: &[(String, [f64; 2])],
    scan: chrono::DateTime<chrono::Utc>,
    metric: bool,
) -> Vec<(f64, String, String, bool)> {
    use crate::app::storm_track::{compass, distance};
    let mut out: Vec<(f64, String, String, bool)> = markers
        .iter()
        .filter_map(|(name, p)| {
            let e = track.eta(*p)?;
            let when = track.t0 + chrono::Duration::seconds((e.minutes * 60.0) as i64);
            let rel = (when - scan).num_seconds() as f64 / 60.0;
            let pass = if e.closest_km < 0.5 {
                "direct hit".to_string()
            } else {
                format!(
                    "passes {} {}",
                    distance(e.closest_km, metric),
                    compass(track.bearing_deg + if e.right { -90.0 } else { 90.0 })
                )
            };
            let rel_text = if rel >= 0.0 {
                format!("+{rel:.0} min")
            } else {
                format!("{:.0} min before this scan", -rel)
            };
            Some((
                rel,
                name.clone(),
                format!("~{} ({rel_text}) \u{b7} {pass}", when.format("%H:%MZ")),
                e.in_path,
            ))
        })
        .collect();
    out.sort_by(|a, b| b.3.cmp(&a.3).then(a.0.total_cmp(&b.0)));
    out
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
    manual: Option<&crate::app::storm_track::ManualTrack>,
    // The impact targets, and how many were left out past their bounds.
    (targets, targets_dropped): (&[crate::app::gis_layers::Target], usize),
) -> Threat {
    use crate::app::storm_track::{compass, distance, ManualTrack};
    let at = [c.lon, c.lat];
    let km = |lon: f64, lat: f64| crate::geo::great_circle(at, [lon, lat]);
    // An archive's local cell is not SCIT's: its cores and motion are the local tracker's.
    let (tracker, tracker_s) = wxdata::celltrack::tracker_of(&c.id);
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
            "{} · evidence {} · {} signal{}, {place}; nearest {tracker} core ({} from nearest signal)",
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
                "{}\nAssociation: {tracker} core inside source polygon; source time unavailable",
                evidence.probsevere[i].detail
            )
        })
        .collect();
    probsevere.sort();
    probsevere.dedup();
    let track = ManualTrack::from_cell(c, t0);
    let motion = track.as_ref().map(|t| {
        format!(
            "Arrivals from {tracker_s} motion, {:03.0}\u{b0} at {:.0} kt, rounded to the minute",
            t.bearing_deg,
            t.speed_kmh / 1.852
        )
    });
    let etas = track
        .as_ref()
        .map(|t| arrivals(t, markers, t0, metric))
        .unwrap_or_default();
    let manual_motion = manual.map(|m| {
        format!(
            "Arrivals from your manual motion, {:03.0}\u{b0} at {:.0} kt set for {}, rounded to the \
             minute",
            m.bearing_deg,
            m.speed_kmh / 1.852,
            m.t0.format("%H:%MZ")
        )
    });
    let manual_etas = manual
        .map(|m| arrivals(m, markers, t0, metric))
        .unwrap_or_default();
    let five = |v: Vec<(f64, String, String, bool)>| -> Vec<(String, String, bool)> {
        v.into_iter()
            .take(5)
            .map(|(_, n, w, h)| (n, w, h))
            .collect()
    };
    let eight = |v: Vec<(f64, String, String, bool)>| -> Vec<(String, String, bool)> {
        v.into_iter()
            .take(8)
            .map(|(_, n, w, h)| (n, w, h))
            .collect()
    };
    let target_etas = track
        .as_ref()
        .map(|t| target_arrivals(t, targets, t0, metric))
        .unwrap_or_default();
    let manual_target_etas = manual
        .map(|m| target_arrivals(m, targets, t0, metric))
        .unwrap_or_default();
    let target_note = (targets_dropped > 0).then(|| {
        format!(
            "{targets_dropped} more targets beyond the first 2,000 points and 200 areas are not \
             checked"
        )
    });
    Threat {
        probsevere,
        ambiguous: associations.ambiguous,
        tornado,
        warnings,
        etas: five(etas),
        motion,
        manual_etas: five(manual_etas),
        manual_motion,
        target_etas: eight(target_etas),
        manual_target_etas: eight(manual_target_etas),
        target_note,
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
            None,
            (&[], 0),
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

    /// An archive's local cell's arrivals are projected from the local tracker's motion and say
    /// so; a SCIT cell's still name SCIT's (roadmaps/storm-analysis S2: motion provenance).
    #[test]
    fn arrivals_name_whose_motion_they_come_from() {
        let motion = |id: &str| {
            let c = Cell {
                id: id.into(),
                ..cell()
            };
            threat_for(
                &c,
                &super::super::storm_associations::Evidence {
                    cells: std::slice::from_ref(&c),
                    circulations: &[],
                    warnings: &[],
                    probsevere: &[],
                },
                &[],
                chrono::Utc::now(),
                false,
                None,
                (&[], 0),
            )
            .motion
            .unwrap()
        };
        assert!(motion("K4").starts_with("Arrivals from SCIT's motion, 090"));
        let local = motion("L-18");
        assert!(
            local.starts_with("Arrivals from the local tracker's motion, 090"),
            "{local}"
        );
        assert!(!local.contains("SCIT"));
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
            None,
            (&[], 0),
        );
        assert!(th.motion.is_none() && th.etas.is_empty());
        assert!(th.manual_motion.is_none() && th.manual_etas.is_empty());
    }

    /// An imported layer's targets: a point ahead gets its arrival, an area the path crosses gets
    /// its entry, an area only the uncertainty swath's edge touches is said as such (and listed
    /// after the ones in the path), and an area the storm is in is inside now.
    #[test]
    fn targets_get_arrivals_entries_and_edge_contact_said_apart() {
        use crate::app::gis_layers::{Target, TargetShape};
        use chrono::TimeZone;
        let scan = chrono::Utc.with_ymd_and_hms(2026, 5, 6, 21, 0, 0).unwrap();
        let c = cell(); // due east at 30 kt from 35N 97W
        let track = crate::app::storm_track::ManualTrack::from_cell(&c, scan).unwrap();
        let o = [c.lon, c.lat];
        let at = |east_km: f64, north_km: f64| {
            let p = crate::geo::destination_point(o, 90.0, east_km);
            crate::geo::destination_point(p, 0.0, north_km)
        };
        let square = |e0: f64, e1: f64, n0: f64, n1: f64| {
            vec![at(e0, n0), at(e1, n0), at(e1, n1), at(e0, n1)]
        };
        let target = |name: &str, shape| Target {
            name: name.into(),
            layer: "Sites".into(),
            shape,
        };
        let targets = vec![
            target("Siren", TargetShape::Point(at(28.0, 0.0))),
            target("Town", TargetShape::Area(square(35.0, 45.0, -5.0, 5.0))),
            target("Farm", TargetShape::Area(square(25.0, 35.0, 6.0, 12.0))),
            target("Here", TargetShape::Area(square(-5.0, 5.0, -5.0, 5.0))),
            target("Far", TargetShape::Area(square(30.0, 40.0, 60.0, 70.0))),
        ];
        let got = target_arrivals(&track, &targets, scan, true);
        let find = |n: &str| got.iter().find(|r| r.1.starts_with(n));
        let here = find("Here").expect("inside");
        assert!(here.2.starts_with("inside") && here.3, "{here:?}");
        let siren = find("Siren (Sites)").expect("point");
        assert!(siren.2.contains("(+30 min)") && siren.3, "{siren:?}");
        let town = find("Town").expect("area");
        assert!(town.2.contains("path enters") && town.3, "{town:?}");
        let farm = find("Farm").expect("edge");
        assert!(
            farm.2.contains("only the swath's edge") && !farm.3,
            "{farm:?}"
        );
        assert!(find("Far (").is_none(), "out of reach: not listed");
        assert_eq!(
            got.last().unwrap().1,
            "Farm (Sites)",
            "edge contact after the path"
        );
    }

    #[test]
    fn a_manual_motion_gives_its_own_arrivals_from_its_own_time() {
        use crate::app::storm_track::ManualTrack;
        use chrono::TimeZone;
        let scan = chrono::Utc.with_ymd_and_hms(2026, 5, 6, 21, 0, 0).unwrap();
        let c = Cell {
            time: Some(scan),
            ..cell()
        };
        // Set ten minutes before the scan, due east at 60 km/h from where the storm was then.
        let set = scan - chrono::Duration::minutes(10);
        let mut m = ManualTrack::from_cell(&c, set).unwrap();
        m.bearing_deg = 90.0;
        m.speed_kmh = 60.0;
        let home = crate::geo::destination_point([c.lon, c.lat], 90.0, 30.0);
        let th = threat_for(
            &c,
            &super::super::storm_associations::Evidence {
                cells: std::slice::from_ref(&c),
                circulations: &[],
                warnings: &[],
                probsevere: &[],
            },
            &[("Home".into(), home)],
            scan,
            true,
            Some(&m),
            (&[], 0),
        );
        // 30 km at 60 km/h is 30 min after it was set: 20 min after the scan.
        assert_eq!(th.manual_etas.len(), 1, "{:?}", th.manual_etas);
        assert!(
            th.manual_etas[0].1.contains("(+20 min)"),
            "{}",
            th.manual_etas[0].1
        );
        let line = th.manual_motion.unwrap();
        assert!(
            line.contains("manual motion, 090\u{b0} at 32 kt set for"),
            "{line}"
        );
        // SCIT's arrivals stand on their own beside it.
        assert_eq!(th.etas.len(), 1);
        assert_ne!(th.etas[0].1, th.manual_etas[0].1);
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
                elevation_deg: 0.5,
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
                elevation_deg: 0.5,
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

    /// A headless app for the full-app tests: storage isolated under `target/` (a folder per
    /// process), network denied (asserted), KTLX the default site. Returns the app, its render
    /// state and the capture folder.
    #[cfg(not(target_arch = "wasm32"))]
    fn headless_app() -> (
        HookEchoApp,
        eframe::egui_wgpu::RenderState,
        std::path::PathBuf,
    ) {
        assert_eq!(
            std::env::var("HOOKECHO_APP_CARD_CAPTURE").as_deref(),
            Ok("1")
        );
        for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
            assert_eq!(std::env::var(key).as_deref(), Ok("http://127.0.0.1:9"));
        }
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let out = repo.join("target/parity-review/cell-core-tilts");
        let storage = out.join(format!("storage-{}", std::process::id()));
        std::fs::create_dir_all(&storage).unwrap();
        crate::paths::set_base(storage.clone());
        let config = crate::paths::config_dir().unwrap();
        assert!(config.starts_with(&storage));
        std::fs::create_dir_all(&config).unwrap();
        let settings = crate::settings::Settings {
            default_site: "KTLX".into(),
            setup_done: true,
            scan_chime: false,
            overlays_on: Some(Vec::new()),
            ..Default::default()
        };
        std::fs::write(
            config.join("settings.json"),
            serde_json::to_vec(&settings).unwrap(),
        )
        .unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (device, queue, adapter) = crate::headless::init_gpu(&rt).unwrap();
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
            renderer: std::sync::Arc::new(egui::mutex::RwLock::new(renderer)),
            surface_config: eframe::egui_wgpu::SurfaceConfig::LOW_LATENCY,
        };
        let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
        cc.wgpu_render_state = Some(state.clone());
        let app = HookEchoApp::new(&cc);
        (app, state, out)
    }

    /// [`headless_app`] with the cached Moore volume (KTLX 2013-05-20 20:16Z) on pane 0, archive
    /// mode, the Moore storm selected and the cell card open. Returns the app, its render state,
    /// the storm's centre and the capture folder.
    #[cfg(not(target_arch = "wasm32"))]
    fn headless_moore_app() -> (
        HookEchoApp,
        eframe::egui_wgpu::RenderState,
        (f64, f64),
        std::path::PathBuf,
    ) {
        let (mut app, state, out) = headless_app();
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        // The volume comes from the local cache only: the network is denied.
        let name = "KTLX20130520_201643_V06.gz";
        let scan = std::sync::Arc::new(
            rt.block_on(wxdata::level2::download_scan(
                wxdata::level2::Identifier::new(name.into()),
                Some(repo.join("target/trail-volumes")),
            ))
            .expect("the Moore volume cached by a_full_volume_core_reads_the_storm_on_many_tilts"),
        );
        let time = wxdata::level2::Identifier::new(name.into())
            .date_time()
            .unwrap();
        app.active = 0;
        app.views[0].site = Some("KTLX".into());
        app.views[0].show_volume(scan, name.into(), time);
        app.views[0].timeline.following = false;
        app.views[0].timeline.playing = false;
        let centre = (-97.51, 35.32);
        app.cell_popup_site = Some("KTLX".into());
        app.cell_popup = Some(Cell {
            id: "L-1".into(),
            title: "L-1".into(),
            lon: centre.0,
            lat: centre.1,
            time: Some(time),
            ..Default::default()
        });
        app.cell_details = true;
        app.dock.cell.open = true;
        app.dock.cell_available = true;
        (app, state, centre, out)
    }

    /// A storm picked on one radar is not another radar's storm with the same ID. Two live panes
    /// (KTLX, KINX); "A3" picked on KTLX; KINX's pane made active. Until KINX's own cells arrive,
    /// its storm table is empty (KTLX's are not KINX's) and they are asked for at once. When they
    /// arrive with a different storm also called "A3", the selection stays the storm picked on
    /// KTLX, read as it was picked, and the Cell window does not open KINX's "A3". Back on KTLX,
    /// the pick is matched there again once KTLX's table is back.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "GPU + isolated app storage/proxy: invoke this exact test in its own process"]
    fn a_storm_picked_on_one_radar_is_not_another_radars_storm() {
        let (mut app, _state, _out) = headless_app();
        let time = chrono::Utc::now();
        let cell = |id: &str, lon: f64, lat: f64| Cell {
            id: id.into(),
            title: id.into(),
            lon,
            lat,
            time: Some(time),
            mvt_deg: Some(60.0),
            mvt_kt: Some(30.0),
            ..Default::default()
        };
        let camera = crate::render::mercator::Camera::at_lonlat(-97.0, 36.0, 6.0);
        app.views = vec![
            crate::view::MapView::new(Some("KTLX".into()), camera),
            crate::view::MapView::new(Some("KINX".into()), camera),
        ];
        for v in &mut app.views {
            v.loaded_site = v.site.clone();
            v.timeline.following = true;
        }
        app.active = 0;
        let ktlx = vec![cell("A3", -97.5, 35.3), cell("B1", -97.0, 35.6)];
        app.dock.storm_ids.feed(Some("KTLX"), &ktlx);
        app.storm_cells = ktlx;
        app.cells_site = Some("KTLX".into());
        app.cells_asked = Some("KTLX".into());
        app.select_storm(app.active_storm_cells()[0].clone());
        app.cell_details = true;
        app.sync_open_cells();
        assert_eq!(app.dock.cells_open, ["A3"]);
        let (sel, live) = app.selected_storm_live().unwrap();
        assert!(live && sel.lon == -97.5);
        // A manual track started from it, on KTLX.
        app.track_cell_manually(&sel);
        assert_eq!(app.manual_tracks_for(&sel).len(), 1);

        // KINX's pane made active: no table yet, and KINX is asked for now.
        app.active = 1;
        assert!(
            app.active_storm_cells().is_empty(),
            "KTLX's cells as KINX's"
        );
        let (sel, live) = app.selected_storm_live().unwrap();
        assert!(!live && sel.lon == -97.5);
        app.overlay_last_fetch = Some(std::time::Instant::now());
        let ctx = egui::Context::default();
        app.sync_pane(1, &ctx);
        assert_eq!(app.cells_asked.as_deref(), Some("KINX"));

        // KINX's cells arrive, with another storm called "A3".
        let kinx = vec![cell("A3", -95.9, 36.4)];
        app.dock.storm_ids.feed(Some("KINX"), &kinx);
        app.storm_cells = kinx;
        app.cells_site = Some("KINX".into());
        assert_eq!(app.active_storm_cells().len(), 1);
        let (sel, live) = app.selected_storm_live().unwrap();
        assert!(!live, "KINX's A3 taken for the storm picked on KTLX");
        assert_eq!((sel.lon, sel.lat), (-97.5, 35.3));
        app.sync_open_cells();
        assert!(app.dock.cells_open.is_empty(), "{:?}", app.dock.cells_open);
        let kinx_a3 = app.active_storm_cells()[0].clone();
        app.cell_trends.insert(
            "A3".into(),
            vec![crate::ui::cell_window::CellSample {
                vil: Some(40.0),
                top: None,
                dbz: Some(60.0),
                severity: None,
                time: Some(time),
                dbz_hgt: None,
            }],
        );
        assert_eq!(app.storm_trend("A3").len(), 1, "KINX's A3 has a trend");
        assert!(
            app.card_trend(&sel).is_empty(),
            "KINX's A3 trend on the KTLX pick"
        );
        let tz = app.active_tz();
        let identity = app.card_identity(&sel, tz).unwrap();
        assert!(identity.starts_with("picked on KTLX"), "{identity}");
        assert!(
            app.manual_tracks_for(&kinx_a3).is_empty(),
            "KTLX's track shown for KINX's A3"
        );

        // Back on KTLX: empty until KTLX's table is back, then the pick is matched there.
        app.active = 0;
        assert!(app.active_storm_cells().is_empty());
        let ktlx = vec![cell("A3", -97.45, 35.32)];
        app.dock.storm_ids.feed(Some("KTLX"), &ktlx);
        app.storm_cells = ktlx;
        app.cells_site = Some("KTLX".into());
        let (sel, live) = app.selected_storm_live().unwrap();
        assert!(live && sel.lon == -97.45);
        app.sync_open_cells();
        assert_eq!(app.dock.cells_open, ["A3"]);
        assert_eq!(app.manual_tracks_for(&sel).len(), 1);
        // The track keeps its radar through a case file.
        let case = app.storm_tracks.tracks[0].to_case();
        assert_eq!(case.source_site.as_deref(), Some("KTLX"));
        let back = crate::app::storm_track::ManualTrack::from_case(&case);
        assert_eq!(back.source.unwrap().site.as_deref(), Some("KTLX"));
    }

    /// "Switch to" a better radar for a selected storm keeps it selected: the switch drops the
    /// old radar's table and the selection with it, and once the new radar's own table is in
    /// (for about the same time) the storm is selected there as the cell nearest where its motion
    /// puts it, never by its old ID. A table from another time (live SCIT under an archived
    /// storm) is not used, and with no storm within 15 km nothing is selected.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "GPU + isolated app storage/proxy: invoke this exact test in its own process"]
    fn a_storm_taken_to_a_better_radar_stays_selected() {
        let (mut app, _state, _out) = headless_app();
        let t0 = chrono::Utc::now() - chrono::Duration::minutes(10);
        let cell = |id: &str, lon: f64, lat: f64, time: chrono::DateTime<chrono::Utc>| Cell {
            id: id.into(),
            title: id.into(),
            lon,
            lat,
            time: Some(time),
            mvt_deg: Some(60.0),
            mvt_kt: Some(30.0),
            ..Default::default()
        };
        let camera = crate::render::mercator::Camera::at_lonlat(-97.0, 36.0, 6.0);
        app.views = vec![crate::view::MapView::new(Some("KTLX".into()), camera)];
        app.views[0].loaded_site = Some("KTLX".into());
        app.views[0].timeline.following = true;
        app.active = 0;
        let ktlx = vec![cell("A3", -97.5, 35.3, t0)];
        app.dock.storm_ids.feed(Some("KTLX"), &ktlx);
        app.storm_cells = ktlx;
        app.cells_site = Some("KTLX".into());
        let picked = app.active_storm_cells()[0].clone();
        app.select_storm(picked.clone());
        let ctx = egui::Context::default();

        // Switched: the old table and the selection go with the old radar.
        app.switch_radar_for_storm("KINX", &picked);
        app.sync_pane(0, &ctx);
        assert_eq!(app.views[0].site.as_deref(), Some("KINX"));
        assert!(app.selected_storm().is_none());
        assert!(app.selection_carry.is_some());

        // KINX's table from two hours later: not this storm's time; nothing adopted yet.
        let arrive = |app: &mut HookEchoApp, cells: Vec<Cell>| {
            app.dock.storm_ids.feed(Some("KINX"), &cells);
            app.storm_cells = cells;
            app.cells_site = Some("KINX".into());
        };
        let later = t0 + chrono::Duration::hours(2);
        arrive(&mut app, vec![cell("C9", -97.45, 35.33, later)]);
        app.sync_pane(0, &ctx);
        assert!(app.selected_storm().is_none());
        assert!(app.selection_carry.is_some(), "still waiting");

        // KINX's table five minutes on: the storm has moved 4.6 km north-east. KINX numbers it
        // C9; its "A3" is another storm 30 km off.
        let t1 = t0 + chrono::Duration::minutes(5);
        let moved = crate::geo::destination_point([-97.5, 35.3], 60.0, 30.0 * 1.852 * 5.0 / 60.0);
        arrive(
            &mut app,
            vec![
                cell("A3", -97.2, 35.4, t1),
                cell("C9", moved[0] + 0.01, moved[1], t1),
                // Nearer where it was than C9 is: only its motion says C9.
                cell("D2", -97.5, 35.3 - 0.03, t1),
            ],
        );
        app.sync_pane(0, &ctx);
        let (sel, live) = app.selected_storm_live().expect("selected on KINX");
        assert_eq!(sel.id, "C9");
        assert!(live);
        assert_eq!(app.cell_popup_site.as_deref(), Some("KINX"));
        assert!(app.selection_carry.is_none());

        // Taken back to KTLX with no storm within 15 km there: nothing is selected.
        app.switch_radar_for_storm("KTLX", &sel);
        app.sync_pane(0, &ctx);
        let t2 = t1 + chrono::Duration::minutes(5);
        app.dock
            .storm_ids
            .feed(Some("KTLX"), &[cell("B1", -96.5, 35.0, t2)]);
        app.storm_cells = vec![cell("B1", -96.5, 35.0, t2)];
        app.cells_site = Some("KTLX".into());
        app.sync_pane(0, &ctx);
        assert!(app.selected_storm().is_none());
        assert!(app.selection_carry.is_none(), "given up");
    }

    /// The real cell card in a real app: the app built headlessly with its storage isolated
    /// under `target/` and its network denied (run with `HOOKECHO_APP_CARD_CAPTURE=1` and every
    /// proxy at `http://127.0.0.1:9`, in its own process), the Moore volume (KTLX 2013-05-20
    /// 20:16Z, cached in `target/trail-volumes`) on its pane, the Moore storm selected. The card
    /// first shows the displayed tilt while the app's own job reads every tilt; once read, it
    /// shows the whole cell. Both frames are written to `target/parity-review/cell-core-tilts/`.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "GPU + isolated app storage/proxy: invoke this exact test in its own process"]
    fn app_cell_card_reads_the_whole_cell() {
        let (mut app, _state, centre, out) = headless_moore_app();
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter");
        let capture = |label: &str, app: &mut HookEchoApp| {
            gpu.save(
                &out.join(format!("{label}.png")),
                CELL_W as u32,
                900,
                |ui| {
                    app.dock_cell(super::super::Host::Docked(ui));
                },
            )
            .unwrap();
        };
        // The first frame asks for the whole cell and shows the displayed tilt.
        capture("app-card-displayed-tilt", &mut app);
        let first = app.cell_cores(0, &[centre], None);
        assert!(matches!(first[0], CellCore::Gathering(..)), "being read");
        // The app's own worker reads every tilt.
        let started = std::time::Instant::now();
        let core = loop {
            match app.cell_cores(0, &[centre], None).remove(0) {
                CellCore::Ready(s) => break s,
                CellCore::Empty => panic!("no gates around the Moore storm"),
                _ => {
                    assert!(started.elapsed().as_secs() < 300, "read in five minutes");
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }
        };
        assert_eq!(core.tilts().len(), 14);
        capture("app-card-whole-cell", &mut app);
        let rows = app
            .whole_cores
            .card
            .as_ref()
            .expect("the card's rows for it");
        assert_eq!(rows.tilts.len(), 14, "By tilt: one row per tilt");
        assert!(rows
            .core
            .iter()
            .any(|r| r.0 == "\u{394}V" && r.1.ends_with("at 0.5\u{b0}")));
        println!("read in {:.1} s", started.elapsed().as_secs_f32());
    }

    /// The Core distribution outline in the real pane, in 3D: the app built as for the card,
    /// its whole-cell core read, the gates of the core's top reflectivity bar (>= 65 dBZ) set as
    /// the hovered outline, and pane 0 drawn by the app's own `render_pane` through its own
    /// renderer, over the observed sweeps in 3D, then flat for comparison, and in 3D without the
    /// outline. Run alone as `app_cell_card_reads_the_whole_cell` is.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "GPU + isolated app storage/proxy: invoke this exact test in its own process"]
    fn app_core_outline_sits_on_its_tilts_in_3d() {
        let (mut app, state, centre, out) = headless_moore_app();
        let started = std::time::Instant::now();
        let core = loop {
            match app.cell_cores(0, &[centre], None).remove(0) {
                CellCore::Ready(s) => break s,
                CellCore::Empty => panic!("no gates around the Moore storm"),
                _ => {
                    assert!(started.elapsed().as_secs() < 300);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }
        };
        let z = core.index_of(Moment::Reflectivity).unwrap();
        let gates: Vec<crate::app::region_stats::HighlightGate> = core
            .rows
            .iter()
            .filter(|r| r.values[z].is_some_and(|v| v >= 65.0))
            .map(|r| crate::app::region_stats::HighlightGate {
                ll: [r.lon, r.lat],
                az_deg: r.az_deg,
                range_km: r.range_km,
                elevation_deg: r.elevation_deg,
            })
            .collect();
        let tilts: std::collections::BTreeSet<u32> = gates
            .iter()
            .map(|g| (g.elevation_deg * 10.0) as u32)
            .collect();
        println!("{} gates >= 65 dBZ on {} tilts", gates.len(), tilts.len());
        assert!(tilts.len() > 3, "the top bar spans tilts: {tilts:?}");
        app.views[0].camera = crate::render::mercator::Camera::at_lonlat(centre.0, centre.1, 10.0);
        app.views[0].camera.pitch = 55.0;
        app.views[0].map_3d.representation = crate::view::Map3dRepresentation::ObservedSweeps;
        let size = (900u32, 700u32);
        let shoot = |label: &str, app: &mut HookEchoApp, three_d: bool, outline: bool| {
            app.views[0].map_3d.enabled = three_d;
            // Kept on: the outline draws while something keeps hovering, which here is always.
            app.region.set_highlight(
                if outline { 1_000_000 } else { 0 },
                if outline { gates.clone() } else { Vec::new() },
            );
            crate::headless::ui::save_with_state(
                &state,
                &out.join(format!("{label}.png")),
                size,
                8,
                |ui| {
                    let rect = ui.max_rect();
                    let ctx = ui.ctx().clone();
                    app.render_pane(ui, &ctx, 0, rect, false, false, true, true, &[]);
                },
            )
            .unwrap();
        };
        shoot("app-outline-3d", &mut app, true, true);
        shoot("app-outline-3d-none", &mut app, true, false);
        shoot("app-outline-flat", &mut app, false, true);
        // Where the outline changes the 3D frame: a mask of those pixels, and their spread. Gates
        // on their tilts rise up the storm; on the ground they would lie in a flat patch.
        let img = |l: &str| {
            image::open(out.join(format!("{l}.png")))
                .unwrap()
                .to_rgba8()
        };
        let (with, without) = (img("app-outline-3d"), img("app-outline-3d-none"));
        let changed = |a: &image::Rgba<u8>, b: &image::Rgba<u8>| {
            a.0.iter().zip(b.0).take(3).any(|(x, y)| x.abs_diff(y) > 60)
        };
        let mut mask = image::RgbaImage::new(with.width(), with.height());
        let (mut n, mut y_lo, mut y_hi) = (0usize, u32::MAX, 0u32);
        for (x, y, p) in with.enumerate_pixels() {
            let q = without.get_pixel(x, y);
            if changed(p, q) {
                n += 1;
                y_lo = y_lo.min(y);
                y_hi = y_hi.max(y);
                mask.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            } else {
                let g = (q.0[0] as u16 + q.0[1] as u16 + q.0[2] as u16) / 12;
                mask.put_pixel(x, y, image::Rgba([g as u8, g as u8, g as u8, 255]));
            }
        }
        mask.save(out.join("app-outline-3d-mask.png")).unwrap();
        println!("{n} pixels changed by the outline, rows {y_lo}..{y_hi}");
        assert!(n > 100, "the outline is drawn in 3D");
        assert!(
            y_hi - y_lo > 60,
            "spread up the storm's tilts, not one flat patch"
        );
    }

    /// The same outline over the resampled (smooth) volume: there the gates sit at their height
    /// above sea level (`highlight_screen`'s other branch), in the volume's own frame. The app
    /// builds the volume off-thread, so the pane is drawn until it is accepted, then captured with
    /// and without the outline. Run alone as `app_cell_card_reads_the_whole_cell` is.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "GPU + isolated app storage/proxy: invoke this exact test in its own process"]
    fn app_core_outline_sits_in_the_smooth_volume() {
        let (mut app, state, centre, out) = headless_moore_app();
        let started = std::time::Instant::now();
        let core = loop {
            match app.cell_cores(0, &[centre], None).remove(0) {
                CellCore::Ready(s) => break s,
                CellCore::Empty => panic!("no gates around the Moore storm"),
                _ => {
                    assert!(started.elapsed().as_secs() < 300);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }
        };
        let z = core.index_of(Moment::Reflectivity).unwrap();
        let gates: Vec<crate::app::region_stats::HighlightGate> = core
            .rows
            .iter()
            .filter(|r| r.values[z].is_some_and(|v| v >= 65.0))
            .map(|r| crate::app::region_stats::HighlightGate {
                ll: [r.lon, r.lat],
                az_deg: r.az_deg,
                range_km: r.range_km,
                elevation_deg: r.elevation_deg,
            })
            .collect();
        app.views[0].moment = Moment::Reflectivity;
        app.views[0].camera = crate::render::mercator::Camera::at_lonlat(centre.0, centre.1, 10.0);
        app.views[0].camera.pitch = 55.0;
        app.views[0].map_3d.enabled = true;
        app.views[0].map_3d.representation = crate::view::Map3dRepresentation::SmoothVolume;
        let size = (900u32, 700u32);
        let shoot = |label: &str, app: &mut HookEchoApp, frames: usize, outline: bool| {
            app.region.set_highlight(
                if outline { 1_000_000 } else { 0 },
                if outline { gates.clone() } else { Vec::new() },
            );
            crate::headless::ui::save_with_state(
                &state,
                &out.join(format!("{label}.png")),
                size,
                frames,
                |ui| {
                    let rect = ui.max_rect();
                    let ctx = ui.ctx().clone();
                    app.render_pane(ui, &ctx, 0, rect, false, false, true, true, &[]);
                },
            )
            .unwrap();
        };
        // Drawing starts the build off-thread; drawing again collects it.
        let waited = std::time::Instant::now();
        while app.smooth_vol_key[0].is_none() {
            assert!(
                waited.elapsed().as_secs() < 300,
                "the smooth volume is built"
            );
            shoot("app-outline-smooth-warmup", &mut app, 2, false);
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        println!(
            "smooth volume accepted after {:.1} s",
            waited.elapsed().as_secs_f32()
        );
        shoot("app-outline-smooth", &mut app, 6, true);
        shoot("app-outline-smooth-none", &mut app, 6, false);
        let _ = std::fs::remove_file(out.join("app-outline-smooth-warmup.png"));
        let img = |l: &str| {
            image::open(out.join(format!("{l}.png")))
                .unwrap()
                .to_rgba8()
        };
        let (with, without) = (img("app-outline-smooth"), img("app-outline-smooth-none"));
        let changed = |a: &image::Rgba<u8>, b: &image::Rgba<u8>| {
            a.0.iter().zip(b.0).take(3).any(|(x, y)| x.abs_diff(y) > 60)
        };
        let mut mask = image::RgbaImage::new(with.width(), with.height());
        let (mut n, mut y_lo, mut y_hi) = (0usize, u32::MAX, 0u32);
        for (x, y, p) in with.enumerate_pixels() {
            let q = without.get_pixel(x, y);
            if changed(p, q) {
                n += 1;
                y_lo = y_lo.min(y);
                y_hi = y_hi.max(y);
                mask.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            } else {
                let g = (q.0[0] as u16 + q.0[1] as u16 + q.0[2] as u16) / 12;
                mask.put_pixel(x, y, image::Rgba([g as u8, g as u8, g as u8, 255]));
            }
        }
        mask.save(out.join("app-outline-smooth-mask.png")).unwrap();
        println!("{n} pixels changed by the outline, rows {y_lo}..{y_hi}");
        assert!(n > 100, "the outline is drawn over the smooth volume");
        assert!(y_hi - y_lo > 60, "spread up the storm, not one flat patch");
    }

    /// A column product's height along the 3D cut (roadmaps/radar-analysis R3), in the real pane:
    /// "Echo top 18" = `last_height_above(REF, 18)` built by the app's own column job on the Moore
    /// volume, a section cut across the storm, the smooth volume with the section's vertical cut
    /// in it. Captured with the cut's column line and without (the plane stays), the line's
    /// pixels are where the line's own points project: on the cut, at the echo top's height.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "GPU + isolated app storage/proxy: invoke this exact test in its own process"]
    fn app_column_height_follows_the_3d_cut() {
        let (mut app, state, centre, out) = headless_moore_app();
        app.settings.udp_products.push(wxdata::udp::ProductDef {
            id: String::new(),
            name: "Echo top 18".into(),
            units: "m".into(),
            expression: "last_height_above(REF, 18)".into(),
            range: None,
            palette: None,
        });
        app.views[0].column_product = Some("Echo top 18".into());
        app.views[0]
            .fields_on
            .insert(crate::render::FieldLayer::UserColumn);
        let ctx = egui::Context::default();
        let started = std::time::Instant::now();
        while app.column_shown(0).is_none() {
            assert!(
                started.elapsed().as_secs() < 300,
                "the column product is built"
            );
            app.recompute_column_product(&ctx);
            app.poll_overlays();
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        println!(
            "column product built in {:.1} s",
            started.elapsed().as_secs_f32()
        );
        // A 50 km cut across the storm, south-west to north-east.
        let (a, b) = (
            [centre.0 - 0.25, centre.1 - 0.12],
            [centre.0 + 0.25, centre.1 + 0.12],
        );
        app.xsection_pts = vec![a, b];
        app.xsection_cut_3d = true;
        app.views[0].moment = Moment::Reflectivity;
        app.views[0].camera = crate::render::mercator::Camera::at_lonlat(centre.0, centre.1, 9.0);
        app.views[0].camera.pitch = 55.0;
        app.views[0].map_3d.enabled = true;
        app.views[0].map_3d.representation = crate::view::Map3dRepresentation::SmoothVolume;
        let size = (900u32, 700u32);
        let shoot = |label: &str, app: &mut HookEchoApp, frames: usize| {
            crate::headless::ui::save_with_state(
                &state,
                &out.join(format!("{label}.png")),
                size,
                frames,
                |ui| {
                    let rect = ui.max_rect();
                    let ctx = ui.ctx().clone();
                    app.render_pane(ui, &ctx, 0, rect, false, false, true, true, &[]);
                },
            )
            .unwrap();
        };
        let waited = std::time::Instant::now();
        while app.smooth_vol_key[0].is_none() {
            assert!(
                waited.elapsed().as_secs() < 300,
                "the smooth volume is built"
            );
            shoot("app-cut-warmup", &mut app, 2);
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        let _ = std::fs::remove_file(out.join("app-cut-warmup.png"));
        app.sync_xsection_3d(0);
        assert!(
            app.views[0].map_3d.plane.is_some(),
            "the section is cut in 3D"
        );
        let line = app.xsection_line().unwrap();
        let height = app.column_height_line(0, &line).expect("a height line");
        let antenna = app.antenna_msl_km(0).unwrap();
        let tops: Vec<f32> = height.above_antenna_km.iter().flatten().copied().collect();
        let peak = tops.iter().copied().fold(0.0, f32::max);
        println!(
            "{} of {} points have an echo top, highest {peak:.1} km above the antenna",
            tops.len(),
            height.above_antenna_km.len()
        );
        shoot("app-cut-column", &mut app, 6);
        app.xsection_cut_3d = false;
        shoot("app-cut-plain", &mut app, 6);
        app.xsection_cut_3d = true;
        let img = |l: &str| {
            image::open(out.join(format!("{l}.png")))
                .unwrap()
                .to_rgba8()
        };
        let (with, without) = (img("app-cut-column"), img("app-cut-plain"));
        let changed = |x: u32, y: u32| {
            let (p, q) = (with.get_pixel(x, y), without.get_pixel(x, y));
            p.0.iter().zip(q.0).take(3).any(|(m, n)| m.abs_diff(n) > 60)
        };
        let mut mask = image::RgbaImage::new(with.width(), with.height());
        let mut n = 0usize;
        for (x, y, q) in without.enumerate_pixels() {
            if changed(x, y) {
                n += 1;
                mask.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            } else {
                let g = (q.0[0] as u16 + q.0[1] as u16 + q.0[2] as u16) / 12;
                mask.put_pixel(x, y, image::Rgba([g as u8, g as u8, g as u8, 255]));
            }
        }
        // Where the line's own points project, with the pane's camera over the whole capture.
        let cam = app.views[0].camera;
        let vex = app.views[0].map_3d.vertical_exaggeration as f64;
        let vp = (size.0 as f32, size.1 as f32);
        let (mut placed, mut near) = (0usize, 0usize);
        for run in column_product_line(a, b, &height.above_antenna_km, antenna) {
            for (lon, lat, km) in run {
                let Some((x, y)) = crate::render3d::lonlat_alt_screen(&cam, vp, lon, lat, km, vex)
                else {
                    continue;
                };
                let (x, y) = (x.round() as i64, y.round() as i64);
                if x < 2 || y < 2 || x >= size.0 as i64 - 2 || y >= size.1 as i64 - 2 {
                    continue;
                }
                placed += 1;
                let hit = (-2..=2)
                    .flat_map(|dx| (-2..=2).map(move |dy| (dx, dy)))
                    .any(|(dx, dy)| changed((x + dx) as u32, (y + dy) as u32));
                near += usize::from(hit);
                mask.put_pixel(x as u32, y as u32, image::Rgba([255, 0, 0, 255]));
            }
        }
        mask.save(out.join("app-cut-column-mask.png")).unwrap();
        println!("{n} pixels changed by the line; {near} of {placed} projected points on it");
        assert!(n > 100, "the column line is drawn in the 3D cut");
        assert!(placed > 20, "the line's points are on screen");
        assert!(
            near * 10 >= placed * 9,
            "the drawn line is where its points project"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn column_product_line(
        a: [f64; 2],
        b: [f64; 2],
        above_antenna_km: &[Option<f32>],
        antenna_msl_km: f64,
    ) -> Vec<Vec<(f64, f64, f64)>> {
        crate::app::column_product::height_line_msl(a, b, above_antenna_km, antenna_msl_km)
    }

    /// The card's Core statistics and By tilt rows for the Moore storm (KTLX 2013-05-20 20:16Z),
    /// read on every tilt by the card's own job and drawn with its own rows, for review
    /// (`target/parity-review/cell-core-tilts/`). Network on first run (volume cached in
    /// `target/trail-volumes`) and a GPU.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu + network: writes the Moore storm's whole-cell card rows"]
    fn gpu_moore_whole_cell_rows() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let id = wxdata::level2::Identifier::new("KTLX20130520_201643_V06.gz".into());
        let scan = std::sync::Arc::new(
            rt.block_on(wxdata::level2::download_scan(
                id,
                Some(repo.join("target/trail-volumes")),
            ))
            .unwrap(),
        );
        let centre = (-97.51, 35.32);
        let mut job = crate::app::cell_core::CoreJob::new(scan, vec![centre]);
        while !job.step() {}
        let core = job.finish().remove(0).1.expect("the Moore storm");
        let radar = wxdata::sites::site_by_id("KTLX").unwrap();
        let ground_km = crate::geo::great_circle(
            [f64::from(radar.longitude), f64::from(radar.latitude)],
            [centre.0, centre.1],
        )
        .0;
        let rows = core_rows(&core);
        let tilts = tilt_rows(&core, Some(ground_km), false);
        let scope = CoreScope::Every(core.tilts().len());
        let out = repo.join("target/parity-review/cell-core-tilts");
        std::fs::create_dir_all(&out).unwrap();
        let t = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter");
        gpu.save(
            &out.join("moore-whole-cell-rows.png"),
            CELL_W as u32,
            620,
            |ui| {
                egui::Frame::new()
                    .fill(t.panel)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ws::section_rule(
                            ui,
                            &t,
                            &format!(
                                "Core statistics \u{b7} {CORE_KM:.0} km \u{b7} {} tilts",
                                core.tilts().len()
                            ),
                        );
                        for (k, v, hot) in &rows {
                            ws::kv(ui, &t, k, v, hot.then_some(t.warn));
                        }
                        ui.label(ws::text(scope.caption(), 10.5, t.text_faint));
                        ws::section_rule(ui, &t, "By tilt");
                        ws::kv(ui, &t, TILT_COLUMNS.0, TILT_COLUMNS.1, Some(t.text_faint));
                        for (k, v, hot) in &tilts {
                            ws::kv(ui, &t, k, v, hot.then_some(t.warn));
                        }
                    });
            },
        )
        .unwrap();
        for (k, v, _) in rows
            .iter()
            .map(|(k, v, h)| (k.to_string(), v, h))
            .chain(tilts.iter().map(|(k, v, h)| (k.clone(), v, h)))
        {
            println!("{k}: {v}");
        }
    }

    /// A whole cell reads tilt by tilt: one row per tilt, lowest first, each with the beam's
    /// height over the centre and that tilt's own values; a couplet or debris on one tilt flags
    /// that tilt only, and the whole cell's Core statistics read every tilt's gates.
    #[test]
    fn a_whole_cell_reads_tilt_by_tilt() {
        let gate = |elevation_deg: f32, z: f32, v: f32, cc: f32| GateRow {
            lon: -97.5,
            lat: 35.0,
            range_km: 40.0,
            az_deg: 0.0,
            elevation_deg,
            values: vec![Some(z), Some(v), Some(cc)],
        };
        // 0.5 deg: a 64 dBZ core with a 60 m/s couplet and low CC; 9.9 deg: 45 dBZ, quiet.
        let mut rows: Vec<GateRow> = (0..10)
            .map(|i| gate(0.5, 55.0 + i as f32, if i < 5 { -30.0 } else { 30.0 }, 0.7))
            .collect();
        // Upper tilt: a steady 50 m/s. Against the low tilt's -30 it would read 80 across tilts.
        rows.extend((0..10).map(|_| gate(9.9, 45.0, 50.0, 0.98)));
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
        let t = tilt_rows(&s, Some(40.0), true);
        assert_eq!(t.len(), 2);
        assert!(t[0].0.starts_with("0.5\u{b0} \u{b7} "), "{}", t[0].0);
        assert!(t[1].0.starts_with("9.9\u{b0} \u{b7} "), "{}", t[1].0);
        let km = |l: &str| -> f64 {
            l.rsplit(" \u{b7} ")
                .next()
                .unwrap()
                .trim_end_matches(" km")
                .parse()
                .unwrap()
        };
        let want = |e: f64| {
            wxdata::xsection::beam_height_km(wxdata::xsection::slant_from_ground_km(40.0, e), e)
        };
        assert!((km(&t[0].0) - want(0.5)).abs() < 0.06);
        assert!((km(&t[1].0) - want(9.9)).abs() < 0.06);
        assert!(
            km(&t[1].0) > km(&t[0].0) + 5.0,
            "the upper tilt is kilometres higher"
        );
        assert_eq!(
            t[0].1, "64 \u{b7} 60 \u{b7} 0.70 \u{b7} \u{2013}",
            "no ZDR here"
        );
        assert!(t[0].2, "the low tilt's couplet and debris");
        assert_eq!(t[1].1, "45 \u{b7} 0 \u{b7} 0.98 \u{b7} \u{2013}");
        assert!(!t[1].2);
        // Feet, and no height without the radar's distance.
        assert!(tilt_rows(&s, Some(40.0), false)[1].0.ends_with(" kft"));
        assert_eq!(tilt_rows(&s, None, true)[0].0, "0.5\u{b0}");
        // The whole cell: every tilt's gates.
        let whole = core_rows(&s);
        assert_eq!(whole.iter().find(|r| r.0 == "Gates").unwrap().1, "20");
        let dv = whole.iter().find(|r| r.0 == "\u{394}V").unwrap();
        assert_eq!(
            dv.1, "60 m/s (117 kt) at 0.5\u{b0}",
            "the widest single tilt, named"
        );
        assert!(dv.2);
        assert_eq!(CoreNumbers::of(&s).delta_v, Some(60.0));
        assert_eq!(
            CoreScope::Every(2).caption(),
            "Every tilt within 8 km of the cell center; dual-pol rows read gates \u{2265}40 dBZ"
        );
        assert!(CoreScope::Displayed(3, 14).caption().contains("(3 of 14)"));
        assert!(CoreScope::Playing
            .caption()
            .contains("while the loop plays"));
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
