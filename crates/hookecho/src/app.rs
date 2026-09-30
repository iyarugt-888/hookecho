//! HookEcho application shell: the map view, its floating chrome, and the async data flow.
//!
//! UI code only mutates the active [`MapView`]; a single per-frame sync step turns those
//! mutations into GPU uploads and background fetches, so buttons and hotkeys share one path.

mod actions;
mod boundaries;
mod case;
/// Touch-first Android chrome (top bar, bottom dock, slide-up sheets), replacing the desktop
/// drawer / pills / alert dock. Only the chrome differs; the map,
/// windows, and every data path are shared.
mod chrome;
mod field_state;
mod goes_timeline;
pub(crate) mod impact;
mod layer_probe;
#[cfg(not(target_arch = "wasm32"))]
mod local_api;
pub(crate) mod long_loop;
mod overlay_health;
mod pane_time;
mod radar_wind;
mod region_stats;
mod report;
pub(crate) use actions::{decode_site_id, encode_site_id, AppWindow, PaletteAction};
mod overlay_fetch;
pub(crate) use overlay_fetch::{OverlayDelivery, OverlayMsg, OverlaySource};
mod account_sync;
mod alerts_watch;
mod beam_tools;
pub(crate) mod camera_flight;
mod chase;
mod detectors;
mod goto;
mod models;
mod output_window;
mod overlay_toggle;
mod packs_soundings;
pub(crate) use goto::{goto_link, parse_goto, Goto};
mod cell_markers;
mod detector_markers;
mod map_click;
mod pane_layout;
mod radar_feed;
mod rules;
mod scenes;
mod sharing;
mod surface_feeds;
mod time_layers;
mod view3d_state;
mod workspace_apply;
pub(crate) use overlay_toggle::OverlayToggle;
pub(crate) use pane_layout::arranged_pane_rects;
mod request_book;
mod scale_bar;
pub(crate) mod storm_track;
pub(crate) mod telemetry;
pub(crate) use request_book::{
    CacheState, DiagnosticsBundle, DiagnosticsSourceHealth, HealthState, RequestBook, RequestLane,
    SourceHealth,
};
mod terrain3d;
mod yall_mode;
pub(crate) use field_state::FieldState;
use goes_timeline::nearest_goes;
mod mobile;

use crate::colormap::{ColorTable, Palettes};
use crate::hotkeys::{self, BindableAction};
use crate::loop3d::{IsoKey, IsoShell, JobKey, Loop3dCache, Loop3dJobs, SmoothKey};
use crate::overlay_build;
#[cfg(not(target_arch = "wasm32"))]
use crate::perf::PerfReadout;
use crate::render::{
    mercator::Camera, MapCallback, ObservedRadialInstance, ObservedSweepLayer, ObservedSweepUpload,
    OverlayUpload, RadarUpload, RenderResources,
};
use crate::settings::Settings;
use crate::source_health::FeedSource;
use crate::tiles::TileManager;
use crate::ui;
use crate::ui::detail_window::Detail;
use crate::view::{Map3dRepresentation, MapView, Volume, MAX_HIGHLIGHTED_LAYERS};
use chrono::{DateTime, NaiveDate, Utc};
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
#[cfg(not(target_arch = "wasm32"))]
use tokio::runtime::Runtime;
use wxdata::alerts::{self};
use wxdata::clock::Instant;
use wxdata::level2::{self, BinnedSweep, Identifier, Moment, Scan};
use wxdata::level3::{self, Cell, CellKind};
use wxdata::overlay::{self, GeoFeature};

/// Frames to let a stepped archive volume load before grabbing it for the loop GIF.
const LOOP_SETTLE_FRAMES: u8 = 12;

/// How long with no input at all before the idle heartbeat slows to [`IDLE_QUIET_MS`]. Long
/// enough that it never fires between two deliberate actions — reading a detail window and then
/// reaching for the mouse is not "idle" — and short enough that a window left alone stops costing
/// a core within a couple of seconds.
const QUIET_AFTER: std::time::Duration = std::time::Duration::from_secs(2);

/// The heartbeat in a window nobody is touching. Two frames a second: the clocks the heartbeat
/// exists for (volume age, countdowns) are minute- and second-resolution, so the visible cost is
/// a reading up to 0.4 s stale. Everything that actually moves asks for its own repaint and is
/// unaffected — see the comment at the use site.
const IDLE_QUIET_MS: u64 = 500;

/// River gauges are fetched only for a view narrower than this, in degrees of longitude: wider,
/// there are too many to read.
const GAUGE_MAX_SPAN_DEG: f64 = 12.0;

/// How long any one overlay or field fetch may run before it is abandoned.
///
/// Shorter than the cadences that drive them (120 s for the alert/watch/MD burst, 60 s at the
/// fastest for a gridded field), which is what keeps a stalled feed from stacking a second copy
/// of itself on every tick. Deliberately *longer* than `wxdata::net::FEED_TIMEOUT`, which is the
/// deadline that actually aborts the request: this one only drops our future, and dropping it
/// first would leave the browser's `fetch` running with its connection held.
const OVERLAY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(55);

/// The same, for a Level 2 volume: bigger file, more patience, still finite. A volume fetch that
/// never returns leaves its pane marked loading, and a pane marked loading never polls again.
/// Again longer than the request's own 90 s deadline in the vendored S3 client, so the abort
/// happens before we stop listening for it.
const VOLUME_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(100);

/// How old the newest known radar volume can be before the site counts as stale rather than
/// "just between scans." The LIVE/Stale scrubber badge and the Radar row's source-health popup
/// both key off this one constant so the two can never disagree — a live site reading Live at
/// the scrubber and Stale in the health popup at the same instant was exactly that drift (two
/// separately hand-picked numbers, 120 s and 900 s, for what is the same question). NEXRAD's
/// slowest common VCP (clear-air, ~10 min between volumes) still reads fresh with room to spare;
/// a genuinely dead feed clears this inside two cycles of even that slowest cadence.
const RADAR_FRESH_SECS: i64 = 900;

/// Loop frames in flight, keyed by volume name, with when each was kicked off.
type PrefetchBook = std::collections::HashMap<String, Instant>;

/// Take the prefetch book, recovering from poisoning.
///
/// A panic elsewhere while this is held must not take the loop down with it: the book is a list
/// of names in flight, not an invariant. Losing track of one of them costs a duplicate download.
fn book(b: &Mutex<PrefetchBook>) -> std::sync::MutexGuard<'_, PrefetchBook> {
    b.lock().unwrap_or_else(|e| e.into_inner())
}

/// Even-odd point-in-ring test on a `[lon, lat]` ring — the click test for watch zones.
/// The most rotation couplet markers drawn at once.
const MAX_COUPLET_MARKERS: usize = 40;

/// The ring and badge colour of a detection confirmed by a report or an observed warning: distinct
/// from the detector colours, because it is a different kind of evidence.
const CONFIRMED_GOLD: egui::Color32 = egui::Color32::from_rgb(255, 215, 64);

fn point_in_ring_ll(ring: &[[f64; 2]], lon: f64, lat: f64) -> bool {
    wxdata::overlay::rings_intersect(
        ring,
        // A tiny square around the click: reuses the one geometry primitive rather than adding a
        // second point-in-polygon implementation here.
        &[
            [lon - 1e-6, lat - 1e-6],
            [lon + 1e-6, lat - 1e-6],
            [lon + 1e-6, lat + 1e-6],
            [lon - 1e-6, lat + 1e-6],
        ],
    )
}

/// The first `http(s)://` URL in a free-text line, if there is one. Spotter reports and chase
/// partners paste stream links into their status text; this is how we find them.
fn first_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|w| w.starts_with("http://") || w.starts_with("https://"))
        .map(|w| w.trim_end_matches(['.', ',', ')', '"', '\'']).to_string())
}

/// 3D volume grid size: `VOL3D_N` cells across each horizontal axis, `VOL3D_NZ` up. Big enough to
/// resolve a hail core, small enough to resample in about a second.
const VOL3D_N: usize = 192;
const VOL3D_NZ: usize = 48;
/// Smallest 3D texture edge a device must support for 3D at all (the standalone window's grid).
const VOL3D_MIN_DIM: usize = VOL3D_N;
/// Top of the 3D volume's vertical span (km), same "beam height above the radar" unit as the
/// CAPPI window's altitude slider — shared so the 3D view's CAPPI reference plane (Phase H4) can
/// place itself without guessing what the volume it's drawn against actually covers.
const VOL3D_TOP_KM: f32 = 18.0;

/// Squared screen-space hit radius (px²) for a tap/click target of nominal `px` radius. Android
/// finger taps need a fatter target than a mouse cursor, so targets grow ~1.8× there; desktop is
/// unchanged.
fn tap_r2(px: f32) -> f32 {
    let r = if cfg!(target_os = "android") {
        px * 1.8
    } else {
        px
    };
    r * r
}

/// Colour for an EF rating, running the same yellow→red ramp the NWS uses in its own survey maps.
/// Unrateable damage (EFU) draws grey so it can't be mistaken for a weak rating.
fn ef_color(efscale: &str) -> egui::Color32 {
    match wxdata::dat::ef_number(efscale) {
        Some(0) => egui::Color32::from_rgb(120, 200, 120),
        Some(1) => egui::Color32::from_rgb(240, 220, 80),
        Some(2) => egui::Color32::from_rgb(245, 165, 50),
        Some(3) => egui::Color32::from_rgb(240, 100, 50),
        Some(4) => egui::Color32::from_rgb(225, 45, 45),
        Some(_) => egui::Color32::from_rgb(200, 60, 200),
        None => egui::Color32::from_rgb(160, 160, 165),
    }
}

/// The first segment of a geocoder result — "Norman, Cleveland County, Oklahoma, United States"
/// is a fine answer to a query and a terrible name for a pin on a map.
fn short_place_name(s: &str) -> &str {
    s.split(',').next().unwrap_or(s).trim()
}

/// Which severe-weather overlays are shown.
pub struct OverlayFilters {
    pub show_alerts: bool,
    pub alert_cats: [bool; 6],
    /// SPC categorical outlook day (0 = off, else 1–3).
    pub outlook_day: u8,
    /// Day-1 outlook hazard: categorical risk, or a tornado/wind/hail probability grid.
    pub outlook_kind: wxdata::spc::OutlookKind,
    pub show_mds: bool,
    /// SPC tornado and severe thunderstorm watches in effect.
    pub show_watches: bool,
    /// WPC Winter Storm Severity Index day (0 = off, else 1-3).
    pub wssi_day: u8,
    /// WPC Excessive Rainfall Outlook day (0 = off, else 1-3).
    pub ero_day: u8,
    /// SPC Fire Weather Outlook day (0 = off, else 1-2 — the only days it publishes a single
    /// categorical risk; see `wxdata::firewx`).
    pub fire_day: u8,
    /// Level 3 storm cells (clickable dots: storm tracking, hail, mesocyclone).
    pub show_cells: bool,
    /// SCIT forecast tracks (painter-only; no overlay rebuild).
    pub show_tracks: bool,
    /// Storm arrival-time cones: project cell motion forward + ETA to watched markers.
    pub show_arrival_cones: bool,
    /// Optical-flow nowcast: advect the current reflectivity echo forward by the mean storm motion.
    pub show_nowcast: bool,
    /// Nowcast lead time in minutes (how far ahead to extrapolate).
    pub nowcast_lead_min: u8,
    /// C2: replace the pane's radar sweep with the per-gate extremum over a trailing window.
    pub show_trail: bool,
    /// Trail window in minutes, ending at the playhead.
    pub trail_window_min: u16,
    /// Keep the weakest value instead of the strongest (CC-minimum paths).
    pub trail_keep_min: bool,
    /// Fade the older part of the trail (ROADMAP_NEW C2 decay), so the path reads as a direction.
    pub trail_decay: bool,
    /// One line of trail progress or restart reason, written by the app and read by Layer options.
    pub trail_status: String,
    /// Auto tornado-debris-signature detection (low CC collocated with high reflectivity).
    pub show_tds: bool,
    /// Flag velocity rotation couplets (client-side gate-to-gate azimuthal shear).
    pub show_couplets: bool,
    /// Tornado ID: one verdict per place from the couplet and debris detectors together.
    pub show_tornado_id: bool,
    /// Flag three-body scatter spikes (hail spikes) off the lowest tilt.
    pub show_tbss: bool,
    /// Flag ZDR columns — rain lofted above the freezing level, an updraft proxy.
    pub show_zdr_columns: bool,
}

impl Default for OverlayFilters {
    fn default() -> Self {
        Self {
            show_alerts: true,
            alert_cats: [true; 6],
            outlook_day: 0, // SPC outlook off by default; user opts in from Layer options
            outlook_kind: wxdata::spc::OutlookKind::Categorical,
            wssi_day: 0, // off by default, like the SPC outlook
            ero_day: 0,
            fire_day: 0,

            show_mds: true,
            show_watches: true,
            show_cells: true,
            show_tracks: true,
            show_arrival_cones: false,
            show_nowcast: false,
            nowcast_lead_min: 15,
            show_trail: false,
            trail_window_min: 30,
            trail_keep_min: false,
            trail_decay: false,
            trail_status: String::new(),
            show_tds: false,
            show_couplets: false,
            show_tornado_id: false,
            show_tbss: false,
            show_zdr_columns: false,
        }
    }
}

/// How many TFR shapes to fetch per refresh.
///
/// The FAA lists ~135 active restrictions and each shape is a separate document, so a first load
/// cannot be one request. Shapes never change once issued, so this only paces that first load: a
/// few refresh cycles and the set is complete and stays complete.
const TFR_BATCH: usize = 25;

/// The selected cursor travels with an MRMS reply so late frames cannot replace a new choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MrmsRequest {
    product: String,
    archive: Option<(chrono::DateTime<chrono::Utc>, u16)>,
}

impl MrmsRequest {
    fn accepts(&self, stamp: &wxdata::field::DataStamp) -> bool {
        stamp.product_id == self.product
            && self.archive.is_none_or(|(target, minutes)| {
                !wxdata::time_align::TimeOffset::between(
                    stamp.valid_time,
                    target,
                    chrono::Duration::minutes(minutes as i64),
                )
                .outside_tolerance
            })
    }
}

/// Background overlay fetch results.
/// Earlier scans' storm cells with each product's time, oldest first.
type CellHistory = Vec<(DateTime<Utc>, Vec<Cell>)>;

/// How many earlier SCIT scans a cell's trend starts with (about two hours at 5-minute volumes).
const CELL_HISTORY_SCANS: usize = 24;

/// Most samples a cell's trend keeps.
const CELL_TREND_MAX: usize = 40;

/// Fold earlier scans into the trends of the cells still tracked (SCIT keeps a storm's id from
/// volume to volume). A scan already in a trend (the same product time) is not added twice;
/// earlier scans carry only what the NST product has (max dBZ and its height), so the VIL, top
/// and severity lines start where the live products do.
fn merge_cell_history(
    trends: &mut std::collections::HashMap<String, Vec<ui::cell_window::CellSample>>,
    past: &CellHistory,
) {
    for (time, cells) in past {
        for c in cells {
            let Some(hist) = trends.get_mut(&c.id) else {
                continue;
            };
            if hist.iter().any(|s| s.time == Some(*time)) {
                continue;
            }
            hist.push(ui::cell_window::CellSample {
                vil: None,
                top: None,
                dbz: c.max_dbz,
                severity: None,
                time: Some(*time),
                dbz_hgt: c.max_dbz_hgt_kft,
            });
        }
    }
    for hist in trends.values_mut() {
        hist.sort_by_key(|s| s.time);
        let over = hist.len().saturating_sub(CELL_TREND_MAX);
        hist.drain(..over);
    }
}

#[cfg(test)]
mod cell_history_tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn history_prepends_tracked_cells_once_in_time_order() {
        let at = |m| Utc.with_ymd_and_hms(2026, 9, 27, 1, m, 0).unwrap();
        let live = ui::cell_window::CellSample {
            vil: Some(40.0),
            top: Some(30.0),
            dbz: Some(60.0),
            severity: Some(40),
            time: Some(at(20)),
            dbz_hgt: Some(19.0),
        };
        let mut trends = std::collections::HashMap::from([("B2".to_string(), vec![live])]);
        let cell = |id: &str, dbz| Cell {
            id: id.into(),
            max_dbz: Some(dbz),
            ..Default::default()
        };
        let past = vec![
            (at(10), vec![cell("B2", 52.0), cell("Z9", 50.0)]),
            (at(15), vec![cell("B2", 56.0)]),
            // The live scan again: not a second sample.
            (at(20), vec![cell("B2", 60.0)]),
        ];
        merge_cell_history(&mut trends, &past);
        let b2 = &trends["B2"];
        let dbz: Vec<_> = b2.iter().map(|s| s.dbz.unwrap()).collect();
        assert_eq!(dbz, vec![52.0, 56.0, 60.0]);
        assert!(b2[0].vil.is_none() && b2[2].vil.is_some());
        assert!(
            !trends.contains_key("Z9"),
            "a cell no longer tracked gets no trend"
        );
    }
}

/// One freehand annotation stroke: a lon/lat polyline and the colour it was drawn in.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Stroke2d {
    pub points: Vec<[f64; 2]>,
    pub color: egui::Color32,
}

/// The four annotation colours, picked to stay legible over both radar and satellite basemaps.
pub(crate) const DRAW_COLORS: [egui::Color32; 4] = [
    egui::Color32::from_rgb(255, 80, 80),
    egui::Color32::from_rgb(255, 215, 60),
    egui::Color32::from_rgb(90, 220, 255),
    egui::Color32::WHITE,
];

/// Append `pt` to the newest stroke, or start one in `color` when `new_stroke`.
pub(crate) fn draw_append(
    strokes: &mut Vec<Stroke2d>,
    pt: [f64; 2],
    color: egui::Color32,
    new_stroke: bool,
) {
    if new_stroke || strokes.is_empty() {
        strokes.push(Stroke2d {
            points: vec![pt],
            color,
        });
        return;
    }
    let last = strokes.last_mut().expect("checked non-empty");
    // Skip points the previous one already covers — a 60 fps drag would otherwise pile up
    // thousands of coincident vertices.
    if last.points.last() != Some(&pt) {
        last.points.push(pt);
    }
}

/// What a left-click on the map does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub(crate) enum MapTool {
    /// Explore storm cells / overlay features (the default).
    #[default]
    Interrogate,
    /// Sample the exact radar gate and open its inspector on a map click.
    GateInspector,
    /// Click a point to rank nearby radars by beam geometry there, not just distance.
    RadarSuitability,
    /// Measure great-circle distance/bearing between two clicks.
    Measure,
    /// Drop a location marker at the clicked point.
    Marker,
    /// Draw a two-click line, then reconstruct a vertical cross-section along it.
    CrossSection,
    /// Click two opposite corners of a box for statistics of every gate in it, across every
    /// moment: a summary table, histogram, scatter plot and CSV (ROADMAP_NEW C4).
    RegionStats,
    /// Click a point to pull an HRRR point sounding (Skew-T / hodograph).
    Sounding,
    /// Click to set your position for chase mode (follow-me + nearest-radar handoff).
    Chase,
    /// Click a point for the plain NWS forecast there (7-day + hourly).
    Forecast,
    /// Click a point to list historical tornado tracks near it (SPC climatology).
    Climatology,
    /// Freehand annotation: drag to draw a line on the map (session-only).
    Draw,
    /// Click out a watch zone vertex by vertex; double-click (or Enter) closes and names it.
    AlertZone,
    /// Click route points (start, stops, destination) for a driving route and what it runs into
    /// (ROADMAP_NEW L1-L3).
    Route,
    /// Drag a storm's motion for the next hour: time marks, a swath and ETAs at the saved
    /// markers (`app::storm_track`, ROADMAP_2 §2.2).
    StormTrack,
}

/// Which set of controls the WSV3 ribbon shows. WSV3 swaps its whole toolbar by data type; this
/// is the same idea — the shared groups (data-type pills, overlays, tools, capture, transport)
/// stay put and the middle of the ribbon changes. Session state, not a saved preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RibbonMode {
    #[default]
    Radar,
    Model,
    Mrms,
}

impl RibbonMode {
    pub(crate) const ALL: [RibbonMode; 3] =
        [RibbonMode::Radar, RibbonMode::Model, RibbonMode::Mrms];

    pub(crate) fn label(self) -> &'static str {
        match self {
            RibbonMode::Radar => "Radar",
            RibbonMode::Model => "Model",
            RibbonMode::Mrms => "MRMS",
        }
    }
}

/// HRRR model field drawn as contour lines over the radar (surface `f00`). SB-CAPE / 0-3 km SRH
/// are fixed here — `// ponytail: not wired to the env suite's env_cape_ml / env_srh_km toggles.`
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
pub(crate) enum ContourKind {
    #[default]
    Off,
    Mslp,
    T2m,
    Td2m,
    Cape,
    Srh,
    /// Significant Tornado Parameter (composite of several HRRR fields — see `wxdata::severe`).
    Stp,
    /// Supercell Composite Parameter.
    Scp,
    /// Energy-Helicity Index, 0-1 km.
    Ehi,
    /// 700–500 hPa lapse rate (°C/km).
    Lapse700500,
    /// 850–500 hPa lapse rate (°C/km).
    Lapse850500,
    /// Effective bulk wind difference (kt).
    EffShear,
    /// Effective storm-relative helicity (m²/s²).
    EffSrh,
    /// STP in its effective-layer form — the one SPC mesoanalysis draws.
    StpEff,
}

impl ContourKind {
    pub(crate) const ALL: [ContourKind; 14] = [
        ContourKind::Off,
        ContourKind::Mslp,
        ContourKind::T2m,
        ContourKind::Td2m,
        ContourKind::Cape,
        ContourKind::Srh,
        ContourKind::Stp,
        ContourKind::Scp,
        ContourKind::Ehi,
        ContourKind::Lapse700500,
        ContourKind::Lapse850500,
        ContourKind::EffShear,
        ContourKind::EffSrh,
        ContourKind::StpEff,
    ];

    /// The composite parameters, which combine several GRIB fields instead of drawing one.
    pub(crate) fn severe(self) -> Option<wxdata::severe::SevereKind> {
        use wxdata::severe::SevereKind as S;
        Some(match self {
            ContourKind::Stp => S::Stp,
            ContourKind::Scp => S::Scp,
            ContourKind::Ehi => S::Ehi,
            ContourKind::Lapse700500 => S::Lapse700500,
            ContourKind::Lapse850500 => S::Lapse850500,
            ContourKind::EffShear => S::EffShear,
            ContourKind::EffSrh => S::EffSrh,
            ContourKind::StpEff => S::StpEff,
            _ => return None,
        })
    }

    /// Contour interval in display units (composites only; single fields carry theirs in `params`).
    pub(crate) fn severe_interval(self) -> f32 {
        match self {
            ContourKind::Stp | ContourKind::StpEff => 0.5,
            ContourKind::Scp => 2.0,
            // °C/km: 0.5 resolves the 7-8 °C/km band steep-lapse-rate plumes live in.
            ContourKind::Lapse700500 | ContourKind::Lapse850500 => 0.5,
            ContourKind::EffShear => 10.0, // kt
            ContourKind::EffSrh => 100.0,  // m²/s²
            _ => 1.0,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            ContourKind::Off => "Off",
            ContourKind::Mslp => "MSLP",
            ContourKind::T2m => "2 m temp",
            ContourKind::Td2m => "2 m dewpoint",
            ContourKind::Cape => "SB-CAPE",
            ContourKind::Srh => "0-3 km SRH",
            ContourKind::Stp => "STP (fixed)",
            ContourKind::Scp => "SCP",
            ContourKind::Ehi => "EHI 0-1 km",
            ContourKind::Lapse700500 => "700-500 lapse",
            ContourKind::Lapse850500 => "850-500 lapse",
            ContourKind::EffShear => "Eff. bulk shear",
            ContourKind::EffSrh => "Eff. SRH",
            ContourKind::StpEff => "STP (effective)",
        }
    }

    /// The token [`Self::from_token`] reads, also the name a kind is saved under; `None` for Off.
    pub(crate) fn token(self) -> Option<&'static str> {
        Some(match self {
            ContourKind::Off => return None,
            ContourKind::Mslp => "mslp",
            ContourKind::T2m => "t2m",
            ContourKind::Td2m => "td2m",
            ContourKind::Cape => "cape",
            ContourKind::Srh => "srh",
            ContourKind::Stp => "stp",
            ContourKind::Scp => "scp",
            ContourKind::Ehi => "ehi",
            ContourKind::Lapse700500 => "lapse700",
            ContourKind::Lapse850500 => "lapse850",
            ContourKind::EffShear => "ebwd",
            ContourKind::EffSrh => "esrh",
            ContourKind::StpEff => "stpeff",
        })
    }

    /// Parse a headless CLI token (`mslp|t2m|td2m|cape|srh`) into a kind.
    pub(crate) fn from_token(s: &str) -> Option<ContourKind> {
        Some(match s {
            "mslp" => ContourKind::Mslp,
            "t2m" => ContourKind::T2m,
            "td2m" => ContourKind::Td2m,
            "cape" => ContourKind::Cape,
            "srh" => ContourKind::Srh,
            "stp" => ContourKind::Stp,
            "scp" => ContourKind::Scp,
            "ehi" => ContourKind::Ehi,
            "lapse700" => ContourKind::Lapse700500,
            "lapse850" => ContourKind::Lapse850500,
            "ebwd" => ContourKind::EffShear,
            "esrh" => ContourKind::EffSrh,
            "stpeff" => ContourKind::StpEff,
            _ => return None,
        })
    }

    fn model_field(self) -> Option<wxdata::model::ModelField> {
        use wxdata::model::ModelField as MF;
        Some(match self {
            ContourKind::Mslp => MF::MeanSeaLevelPressure,
            ContourKind::T2m => MF::Temperature2m,
            ContourKind::Td2m => MF::Dewpoint2m,
            ContourKind::Cape => MF::SurfaceCape,
            ContourKind::Srh => MF::Srh3km,
            _ => return None,
        })
    }

    /// GRIB `(var, level, native contour interval)`, or `None` for `Off` and derived composites.
    /// HRRR and RAP intentionally share these spellings; the model-catalog contract test protects
    /// that invariant because this contour UI can switch between them without changing fields.
    pub(crate) fn params(self) -> Option<(&'static str, &'static str, f32)> {
        let field = self.model_field()?;
        let key = field.grib(wxdata::hrrr::Model::Hrrr)?;
        Some((
            key.var,
            key.level,
            field.descriptor().default_contour_interval?,
        ))
    }

    pub(crate) fn interval(self, temp_unit: crate::settings::TempUnit) -> f32 {
        match (self, temp_unit) {
            // Two kelvin is a useful metric interval; five Fahrenheit is the conventional rounded
            // chart interval rather than the awkward exact conversion (3.6 °F).
            (ContourKind::T2m | ContourKind::Td2m, crate::settings::TempUnit::Fahrenheit) => 5.0,
            (ContourKind::Mslp, _) => self.params().map_or(2.0, |(_, _, pa)| pa / 100.0),
            _ => self
                .params()
                .map_or_else(|| self.severe_interval(), |(_, _, interval)| interval),
        }
    }

    /// Convert a raw GRIB value to the display unit the interval is expressed in.
    pub(crate) fn to_display(self, raw: f32, temp_unit: crate::settings::TempUnit) -> f32 {
        match self {
            ContourKind::Mslp => raw / 100.0, // Pa → hPa
            ContourKind::T2m | ContourKind::Td2m => temp_unit.from_c(raw - 273.15), // K → selected unit
            _ => raw, // CAPE / SRH as-is
        }
    }

    fn unit(self, temp_unit: crate::settings::TempUnit) -> Option<&'static str> {
        matches!(self, ContourKind::T2m | ContourKind::Td2m).then(|| temp_unit.label())
    }

    fn display_label(self, temp_unit: crate::settings::TempUnit) -> String {
        match self.unit(temp_unit) {
            Some(unit) => format!("{} {unit}", self.label()),
            None => self.label().into(),
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            ContourKind::Mslp => egui::Color32::from_rgb(235, 235, 235),
            ContourKind::T2m => egui::Color32::from_rgb(240, 120, 60),
            ContourKind::Td2m => egui::Color32::from_rgb(90, 200, 120),
            ContourKind::Cape => egui::Color32::from_rgb(240, 160, 40),
            ContourKind::Srh => egui::Color32::from_rgb(190, 110, 230),
            ContourKind::Stp => egui::Color32::from_rgb(230, 60, 90),
            ContourKind::Scp => egui::Color32::from_rgb(250, 120, 50),
            ContourKind::Ehi => egui::Color32::from_rgb(150, 110, 235),
            ContourKind::Lapse700500 => egui::Color32::from_rgb(240, 200, 90),
            ContourKind::Lapse850500 => egui::Color32::from_rgb(210, 170, 70),
            ContourKind::EffShear => egui::Color32::from_rgb(120, 190, 250),
            ContourKind::EffSrh => egui::Color32::from_rgb(200, 130, 240),
            ContourKind::StpEff => egui::Color32::from_rgb(250, 70, 110),
            ContourKind::Off => egui::Color32::WHITE,
        }
    }
}

/// Text for a multi-select contour picker's collapsed summary: "Off" for none active, the one
/// label when exactly one is, otherwise a joined list — so the picker always says what's actually
/// drawn without needing to open it.
pub(crate) fn summarize_contours(active: &std::collections::BTreeSet<ContourKind>) -> String {
    if active.is_empty() {
        return ContourKind::Off.label().to_string();
    }
    active
        .iter()
        .map(|k| k.label())
        .collect::<Vec<_>>()
        .join(", ")
}

/// One active [`ContourKind`]'s own fetched state — kept per kind so several contour overlays can
/// be in flight, cached, and stale-refreshed independently of each other.
#[derive(Default)]
pub(crate) struct ContourEntry {
    pub lines: Vec<wxdata::contour::ContourLine>,
    pub valid: Option<DateTime<Utc>>,
    /// The grid the lines were drawn from (display units), which the layer probe reads.
    pub grid: Option<Arc<wxdata::mrms::MrmsField>>,
    pub last_fetch: Option<Instant>,
    pub fetched_key: Option<(wxdata::hrrr::Model, crate::settings::TempUnit)>,
}

/// What a computed blockage raster was built for. Any change here (site, tilt, or a pan/zoom past
/// the quantization) makes the resident raster stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlockageKey {
    site: String,
    /// Tilt elevation in millidegrees (an integer so the key can be compared).
    tilt_mdeg: i32,
    /// The world-space rect, in units of 1e-7 world (~2 m at the equator).
    world: [i64; 4],
}

/// What a computed lowest-usable-tilt raster was built for — the same shape as [`BlockageKey`],
/// but keyed by the volume's whole elevation list (in millidegrees) rather than one tilt, since
/// this overlay answers a question about every tilt at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LowestTiltKey {
    site: String,
    tilts_mdeg: Vec<i32>,
    world: [i64; 4],
}

/// What a computed coverage-comparison raster was built for. Fixed at 0.5° — the same elevation
/// the suitability popup itself ranks candidates at — so the overlay always agrees with the
/// numbers the user just read there; a pan/zoom past the quantization still rebuilds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoverageCompareKey {
    site_a: String,
    site_b: String,
    world: [i64; 4],
}

/// A placefile label/marker the egui painter draws over the map.
pub(crate) struct PlaceLabel {
    pub color: egui::Color32,
    pub pos: [f64; 2],
    /// `Object:` anchor: when set, `pos` is a pixel offset from this point rather than a position.
    pub anchor: Option<[f64; 2]>,
    pub hover: String,
    pub kind: PlaceLabelKind,
}

/// What a [`PlaceLabel`] draws.
pub(crate) enum PlaceLabelKind {
    Text(String),
    /// An icon with no usable sheet (none declared, or the image hasn't loaded): ring + dot.
    Marker,
    /// One cell of a loaded icon sheet, rotated `angle` degrees clockwise.
    Sprite {
        tex: egui::TextureId,
        uv: egui::Rect,
        size: egui::Vec2,
        hot: egui::Vec2,
        angle: f32,
    },
}

/// How loud a [`Toast`] is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToastKind {
    Info,
    Success,
    Error,
}

/// Auxiliary-feed failures waiting to be told to the user.
///
/// A global rather than a channel: these happen in half a dozen spawned tasks across three
/// different message enums, and none of them owns a sender. The app drains it once a frame.
///
/// ponytail: unbounded and process-wide, which is fine for something drained every frame and
/// gated to one toast per feed. A sender per task is the upgrade if it ever needs ordering
/// against the other messages.
static FEED_ERRORS: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// How many undrained feed errors are kept. More than fits on screen, far less than a failing
/// network produces over an afternoon.
const MAX_FEED_ERRORS: usize = 64;

/// Log an auxiliary feed's failure and queue it for the user.
///
/// "Auxiliary" means a feed whose failure leaves the rest of its layer standing — buoys inside
/// the station plot, Windy inside the webcams. Those used to fail into the log alone, which is
/// indistinguishable from the feed simply having nothing to show.
pub(crate) fn note_feed_error(feed: impl Into<String>, err: impl std::fmt::Display) {
    let feed = feed.into();
    log::warn!("{feed}: {err}");
    if let Ok(mut q) = FEED_ERRORS.lock() {
        // Bounded because the drain is per frame and frames are not guaranteed: a hidden browser
        // tab does not draw, and a network that is failing produces one of these per feed per
        // refresh. The newest are the ones worth showing when the frames come back.
        if q.len() >= MAX_FEED_ERRORS {
            q.remove(0);
        }
        q.push((feed, err.to_string()));
    }
}

/// A short-lived note about something the user just did.
pub(crate) struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub at: Instant,
}

/// A registry row: what it's called, where it lives, what it does, and (for toggles) its state.
#[derive(Clone)]
pub(crate) struct PaletteEntry {
    pub label: String,
    pub category: &'static str,
    pub action: PaletteAction,
    pub on: Option<bool>,
    /// One line of plain English. Jargon labels ("AzShear (0–2 km)") mean nothing on their own.
    pub desc: &'static str,
    /// Everyday entries sort before specialist entries inside their group.
    pub common: bool,
    /// The key bound to this action, if any — included in the row's hover help.
    pub key: Option<String>,
    /// Current network health. Disabled, static and local-only rows deliberately carry none.
    pub health: Option<SourceHealth>,
}

/// Refresh cadence (seconds) for a national field layer's product.
fn field_refresh_secs(layer: crate::render::FieldLayer) -> u64 {
    use crate::render::FieldLayer as FL;
    match layer {
        FL::Lightning | FL::AzShear => 60,
        FL::Mrms | FL::Mesh | FL::Rotation | FL::RotationMidLevel | FL::Hrrr | FL::Mosaic => 120,
        // Same MRMS product cadence as MESH/rotation above.
        FL::Posh
        | FL::Shi
        | FL::MrmsVil
        | FL::MrmsEchoTop18
        | FL::MrmsEchoTop30
        | FL::MrmsEchoTop50
        | FL::MrmsEchoTop60
        | FL::ReflLowestAlt
        | FL::LowLevelReflectivity
        | FL::MrmsRefl0c
        | FL::MrmsReflM5c
        | FL::MrmsReflM10c
        | FL::MrmsReflM15c
        | FL::MrmsReflM20c => 120,
        // QPE accumulations update on a ~2-minute MRMS cadence.
        // The rate product lands every 2 minutes; the accumulations move far more slowly.
        FL::PrecipRate => 120,
        FL::Qpe1h | FL::Qpe3h | FL::Qpe6h | FL::Qpe12h | FL::Qpe24h => 120,
        // MRMS precip type / flash-flood ARI on the ~2-min cadence; L3 grids on the 120 s L3 cadence.
        FL::PrecipType
        | FL::FlashFlood
        | FL::FlashFlood1h
        | FL::FlashFlood3h
        | FL::FlashFlood6h
        | FL::FlashFlood12h
        | FL::FlashFlood24h
        | FL::FlashFloodMax
        | FL::Vil
        | FL::EchoTops
        | FL::Hca => 120,
        // Bands are cut from the ~2-min mosaic, so they are as fresh as it is.
        FL::SnowBands => 120,
        FL::UpdraftHelicity => 600,
        // Snowfall accumulates over a whole model run; it moves as slowly as the run does.
        FL::Snowfall => 600,
        // The analysis is reissued four times a day; half an hour is plenty.
        FL::SnowAnalysis => 1800,
        // Global cycles are six hours apart and take hours to post. Half an hour is generous.
        FL::GlobalMslp
        | FL::GlobalHeight500
        | FL::GlobalTemp2m
        | FL::GlobalDewpoint2m
        | FL::GlobalWind10m
        | FL::GlobalPrecip
        // Two global cycles behind it, so the same half hour.
        | FL::ModelDiff
        | FL::CompareA
        | FL::CompareB
        // GEFS also cycles every six hours; the 31-file fetch is worth doing no more often.
        | FL::Ensemble => 1800,
        FL::Smoke => 900,
        // NBM posts hourly; the blend moves no faster than that.
        FL::ThunderProb => 900,
        // CONUS ABI CMIP lands on S3 about every 5 minutes, whichever band.
        FL::GoesIr
        | FL::GoesVisible
        | FL::GoesWaterVapor
        | FL::GoesShortwaveIr
        | FL::GoesMidWaterVapor
        | FL::GoesLowWaterVapor
        | FL::GoesDirtyIr
        | FL::GoesLongwaveIr
        | FL::GoesDustDiff
        | FL::GoesColdTop
        | FL::GoesCoolingRate
        | FL::GoesRgb => 300,
        // NDFD elements update on a forecaster's schedule, not a fixed clock, and each fetch is
        // a whole multi-day CONUS grid (tens of MB) with no way to ask for just the new part —
        // half an hour balances staying current against re-downloading that for no reason.
        FL::NdfdTemp2m | FL::NdfdWind10m | FL::NdfdGust10m | FL::NdfdSnow => 1800,
        // A new analysis posts hourly, about 45 minutes after its hour; ten minutes catches it
        // soon after it lands without asking constantly.
        FL::RtmaTemp2m
        | FL::RtmaDewpoint2m
        | FL::RtmaWind10m
        | FL::RtmaGust10m
        | FL::RtmaVisibility
        | FL::RtmaCeiling
        | FL::RtmaMslp
        | FL::RtmaPrecip1h => 600,
        // An accumulation moves slower than the grid it accumulates, whatever the window.
        FL::HailSwath => 300,
        // Environment (HRRR CAPE/SRH) refreshes slowly — 15 min.
        FL::Cape | FL::Srh => 900,
        // Derived products cost no network: they recompute when the volume does, not on a clock.
        FL::CompositeLocal
        | FL::VilLocal
        | FL::VilDensity
        | FL::EtopLocal
        | FL::HailMehs
        | FL::HailPosh => 60,
        // Gridded from the GLM feed the app already polls every 20 s; regridding is local work.
        FL::GlmFed => 60,
    }
}

/// Format a raw decoded grid sample exactly as the field legend does: categorical labels rather
/// than integer codes, user-selected temperature units rather than Kelvin, and each ramp's input
/// scaling for quantities such as smoke and snowfall.
fn format_probe_field_value(
    layer: crate::render::FieldLayer,
    raw: f32,
    temp_unit: crate::settings::TempUnit,
) -> Option<String> {
    if !raw.is_finite() {
        return None;
    }
    if let Some(ramp) = crate::render::field_ramps::ramp_for(layer) {
        if let crate::render::field_ramps::FieldScale::Categorical(categories) = &ramp.scale {
            let code = raw.round().clamp(0.0, 255.0) as u8;
            return Some(
                categories
                    .iter()
                    .find(|(value, ..)| *value == code)
                    .map_or_else(|| format!("{code}"), |(_, _, label)| (*label).into()),
            );
        }
        if ramp.is_temp_kelvin {
            return Some(format!(
                "{:.1} {}",
                temp_unit.from_c(raw - 273.15),
                temp_unit.label()
            ));
        }
        let value = ramp.display(raw);
        return Some(if ramp.units.is_empty() {
            format!("{value:.1}")
        } else {
            format!("{value:.1} {}", ramp.units)
        });
    }
    use crate::render::FieldLayer as FL;
    if matches!(layer, FL::Mrms | FL::Hrrr | FL::Mosaic | FL::CompositeLocal) {
        return Some(format!("{raw:.1} dBZ"));
    }
    // A composite's value is a colour, which is what the probe says; its meaning is in the
    // recipe's reading.
    if layer == FL::GoesRgb {
        return wxdata::goes_rgb::unpack(raw).map(|[r, g, b]| format!("RGB {r} {g} {b}"));
    }
    let units = layer
        .descriptor()
        .map(|descriptor| descriptor.units.symbol())
        .unwrap_or("");
    Some(if units.is_empty() {
        format!("{raw:.1}")
    } else {
        format!("{raw:.1} {units}")
    })
}

const COMPARE_OVERLAY_ALPHA: f32 = 0.5;

/// Whether a window this wide (points) is too narrow for the workstation's docks: a phone's
/// width, M3's compact class.
fn workstation_too_narrow(window_w: f32) -> bool {
    crate::ui::m3::width_class(window_w) == crate::ui::m3::WidthClass::Compact
}

/// Apply the pane-local comparison blend without changing the user's ordinary layer-opacity
/// preference. The renderer owns a field-per-pane uniform, so this stays local even when another
/// pane shows B alone at full opacity.
fn field_draw_opacity(
    layer: crate::render::FieldLayer,
    configured: f32,
    compare_overlay: bool,
) -> f32 {
    if compare_overlay && layer == crate::render::FieldLayer::CompareB {
        configured * COMPARE_OVERLAY_ALPHA
    } else {
        configured
    }
}

/// Human-readable form of one signed comparison sample for the selected display mode. Keeping
/// this shared between the linked probe and hover tooltip prevents a categorical mask from
/// claiming "disagree" in one surface and showing an unexplained number in the other.
fn format_diff_readout(
    mode: crate::fielddiff::DiffMode,
    signed: f32,
    deadband: f32,
    units: &str,
) -> String {
    use crate::fielddiff::DiffMode;
    match mode {
        DiffMode::Signed => format!("{signed:+.1} {units}"),
        DiffMode::Absolute => format!("{:.1} {units}", signed.abs()),
        DiffMode::Percent => format!("{signed:+.0} % of B"),
        DiffMode::Disagreement => {
            let magnitude = signed.abs();
            let class = if magnitude <= deadband {
                "Agree"
            } else {
                "Disagree"
            };
            format!("{class} · Δ {magnitude:.1} {units}")
        }
    }
}

/// The ZDR-column cache: the volume it was computed for, its columns, and the bright band the
/// same pass found.
/// A place the proximity alerts watch: a saved marker, or wherever the GPS says you are.
struct WatchedPoint {
    /// The marker's stable id; cooldowns key on this, never on the name.
    id: String,
    name: String,
    lon: f64,
    lat: f64,
    radius_mi: f64,
}

/// Cooldown key for the follow-GPS pseudo-marker, which has no entry in the settings.
const GPS_POINT_ID: &str = "gps";

/// A volume key with the detector thresholds folded in, so moving a slider recomputes instead of
/// handing back the answer to the previous question.
type TunedKey = ((usize, String, usize), u64);

type ZdrCache = (
    TunedKey,
    Vec<wxdata::dualpol::ZdrColumnHit>,
    Option<wxdata::dualpol::BrightBand>,
);

/// How long a field layer stays uploaded after the last pane turns it off. Long enough that
/// toggling a layer to compare it against another doesn't re-fetch, short enough that an
/// afternoon of browsing doesn't end with every layer's texture still on the GPU.
const FIELD_EVICT: std::time::Duration = std::time::Duration::from_secs(300);

/// What a settings-sync worker reports back. Everything network lives on the runtime; the app
/// thread only ever applies the outcome.
enum SyncMsg {
    /// Fresh tokens (from finishing a sign-in, or from a refresh mid-sync).
    Signed(crate::cloud::Tokens),
    /// The remote settings blob, to merge over the local one.
    Pulled {
        body: String,
        modified: String,
    },
    /// Our settings are now the remote ones, as of this `modifiedTime`.
    Pushed {
        modified: String,
        hash: u64,
    },
    /// Both sides had edits. We took the remote copy; say so rather than pretend.
    Conflict,
    UpToDate,
    Error(String),
}

/// Where a pending screenshot is delivered: saved to a file, or copied to the clipboard, or
/// captured as one frame of a loop-GIF export.
enum ShotDest {
    File(std::path::PathBuf),
    Clipboard,
    Loop,
    /// Pushed to the user's ntfy topic as an attachment, captioned with this title.
    Push(String),
    /// Written silently for the Android home-screen widget to pick up.
    Widget(std::path::PathBuf),
    /// The map image for an analysis export (ROADMAP_NEW K4); see `app/report.rs`.
    Report,
    /// The window for the local API's snapshot endpoint, sent back to the waiting request.
    #[cfg(not(target_arch = "wasm32"))]
    Api(std::sync::mpsc::Sender<Result<Vec<u8>, String>>),
}

/// In-progress loop export (GIF or MP4): steps the active timeline, grabbing one screenshot per
/// frame.
struct LoopExport {
    dest: std::path::PathBuf,
    format: crate::loopexport::LoopFormat,
    frames: Vec<image::RgbaImage>,
    /// Slots still to capture (counts down as frames are grabbed).
    remaining: usize,
    /// Frames to let the newly-stepped radar settle/load before grabbing.
    settle: u8,
    /// A screenshot has been requested; waiting for its event.
    capturing: bool,
    /// Playback speed the scrubber was set to when the export started — the exported clip plays
    /// at the speed the user was watching, instead of a hardcoded 5 fps.
    fps: f32,
    /// Each captured frame's volume and valid time, for real timing and the sidecar.
    volumes: Vec<Option<(String, DateTime<Utc>)>>,
    /// Hold each frame for its real scan gap (`Settings::loop_real_timing`) or all alike.
    real_timing: bool,
}

/// A placefile the app has fetched and is tracking (mirrors a `PlacefileConfig` by URL).
/// What the memoised placefile labels depend on: (placefile item/enabled/icon fingerprint,
/// minute, view range in nmi).
type PlaceLabelKey = (usize, i64, i32);

struct LoadedPlacefile {
    /// A URL, or the synthetic `plugin:<name>` key for a plugin-produced overlay.
    url: String,
    enabled: bool,
    pf: wxdata::placefile::Placefile,
    last_fetch: Option<Instant>,
    loaded: bool,
    /// Why the last load failed, if it did.
    error: Option<String>,
}

/// A background fetch result routed back to a specific view.
/// Loop frames a phone keeps decoded at once. Each volume is tens of MB; a longer loop than this
/// pushes the process into the range Android kills.
#[cfg(target_os = "android")]
const ANDROID_LOOP_WINDOW: usize = 6;
#[cfg(not(target_os = "android"))]
const ANDROID_LOOP_WINDOW: usize = 6;

/// Frame downloads allowed in flight at once, on top of the head poll and the frame being shown.
///
/// A phone (or a browser tab) on a cell radio gets nothing from a deeper queue: the frames arrive
/// in the same total time and each one lands later than it would have with a queue behind it.
const MAX_PREFETCH_INFLIGHT: usize = if cfg!(target_os = "android") || cfg!(target_arch = "wasm32")
{
    2
} else {
    4
};

/// How far along a playing loop's own order prefetch looks for frames it has not kept yet.
const LONG_LOOP_LOOKAHEAD: usize = 16;

/// Which frames to pull in around the playhead, nearest first.
///
/// Playing only ever moves forward, so it looks ahead. Scrubbing can go either way and usually
/// reverses, so it takes one behind as well — that one is what makes dragging the timeline back a
/// frame feel instant instead of costing a fresh download.
fn prefetch_offsets(playing: bool) -> &'static [isize] {
    if playing {
        &[1, 2, 3]
    } else {
        &[1, -1, 2, -2]
    }
}

/// Loop frames the browser build keeps decoded at once — "the last fifteen minutes", which at a
/// severe-weather VCP is four volumes. A wasm heap is 32-bit and a decoded volume is tens of MB,
/// so this is a memory budget as much as a time window.
const WEB_LOOP_WINDOW: usize = 4;

enum DataMsg {
    Volume {
        view: usize,
        site: String,
        name: String,
        time: DateTime<Utc>,
        scan: Scan,
        /// True only from the live-head poll (`spawn_fetch`'s `latest_identifiers` result), never
        /// from an archive/loop-frame fetch of an already-known `Identifier` (`spawn_frame_fetch`).
        /// `t.frames` can independently learn a name from the bucket listing (`DataMsg::Frames`)
        /// before the matching volume fetch completes, so "is this name new to `t.frames`" is not
        /// a reliable way to tell live arrivals from archive ones — this flag is set at the only
        /// place that actually knows which kind of fetch produced the result.
        live_poll: bool,
    },
    /// A live sweep-boundary update (merged full volume) from the chunk streamer.
    Live {
        view: usize,
        site: String,
        gen: u64,
        name: String,
        time: DateTime<Utc>,
        /// Client transport receipt before decode, when the provider exposes it.
        received_at: Option<wxdata::clock::Instant>,
        radial_coverage: Option<wxdata::live::RadialCoverage>,
        /// Already shared with the streaming task's running volume (see `wxdata::live::Update`).
        scan: Arc<Scan>,
        changed: Vec<f32>,
        /// Chunk fetch retries since this stream connection started — see
        /// `wxdata::live::Update::retries`.
        retries: u32,
        /// See `wxdata::live::Update::decode_time`.
        decode_time: std::time::Duration,
    },
    /// The live stream for `view` ended (error or clean exit); polling resumes.
    LiveEnded {
        view: usize,
        site: String,
        /// Stream generation — a stale end must not clear a newer stream's handle.
        gen: u64,
    },
    /// How far the live stream has scanned into the current sweep — fires on every chunk, far
    /// more often than `Live`'s full merged-volume updates, so a UI can show scan-in-progress
    /// motion between them.
    LiveProgress {
        view: usize,
        site: String,
        gen: u64,
        progress: wxdata::live::ScanProgress,
    },
    /// The archive volume listing for a site+date (timeline frames).
    Frames {
        view: usize,
        site: String,
        date: NaiveDate,
        frames: Vec<Identifier>,
    },
    UpToDate {
        view: usize,
        site: String,
    },
    /// A loop frame fetched ahead of the playhead. Goes into the scan cache and nowhere else —
    /// showing it would jump the display forward.
    Prefetched {
        view: usize,
        site: String,
        name: String,
        scan: Scan,
    },
    Error {
        view: usize,
        site: String,
        err: String,
    },
}

impl DataMsg {
    fn view(&self) -> usize {
        match self {
            DataMsg::Volume { view, .. }
            | DataMsg::Live { view, .. }
            | DataMsg::LiveEnded { view, .. }
            | DataMsg::LiveProgress { view, .. }
            | DataMsg::Frames { view, .. }
            | DataMsg::UpToDate { view, .. }
            | DataMsg::Prefetched { view, .. }
            | DataMsg::Error { view, .. } => *view,
        }
    }
    fn site(&self) -> &str {
        match self {
            DataMsg::Volume { site, .. }
            | DataMsg::Live { site, .. }
            | DataMsg::LiveEnded { site, .. }
            | DataMsg::LiveProgress { site, .. }
            | DataMsg::Frames { site, .. }
            | DataMsg::UpToDate { site, .. }
            | DataMsg::Prefetched { site, .. }
            | DataMsg::Error { site, .. } => site,
        }
    }
}

/// What is currently uploaded to the GPU, so we only re-bin/re-upload on a real change.
/// The trailing option is the storm-motion (east, north) m/s for storm-relative velocity.
///
/// The palette generation is deliberately *not* in here — it rides alongside in `pane_lut`, so
/// a color-table change re-bakes the 3 KB LUT without re-binning or re-uploading the sweep.
/// Wedges around the scan-age ring: 3° each, fine enough to read a rotation edge, coarse enough
/// to draw as a few hundred segments.
const SCAN_AGE_WEDGES: usize = 120;

/// The scan-age ring for one pane, read off the sweep as it was binned.
struct ScanAgeRing {
    origin: [f64; 2],
    radius_km: f64,
    wedges: Vec<Option<f32>>,
    summary: wxdata::scan_age::AgeSummary,
}

impl ScanAgeRing {
    /// `None` when the sweep carries no collection times: there is nothing honest to draw.
    fn from_sweep(s: &BinnedSweep) -> Option<Self> {
        let summary = wxdata::scan_age::summarize(s)?;
        let wedges = wxdata::scan_age::ring(s, SCAN_AGE_WEDGES)?;
        let edge_km =
            f64::from(s.first_gate_km) + s.gate_count as f64 * f64::from(s.gate_interval_km);
        Some(Self {
            origin: [f64::from(s.radar_lon), f64::from(s.radar_lat)],
            // Just inside the edge of the data, so the ring sits on the picture it describes.
            radius_km: edge_km * 0.98,
            wedges,
            summary,
        })
    }
}

/// Green for the newest data in the sweep through amber to red for the oldest.
fn scan_age_color(t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |a: [f32; 3], b: [f32; 3], u: f32| {
        egui::Color32::from_rgb(
            (a[0] + (b[0] - a[0]) * u) as u8,
            (a[1] + (b[1] - a[1]) * u) as u8,
            (a[2] + (b[2] - a[2]) * u) as u8,
        )
    };
    let (green, amber, red) = (
        [74.0, 201.0, 110.0],
        [240.0, 190.0, 60.0],
        [230.0, 80.0, 70.0],
    );
    if t < 0.5 {
        lerp(green, amber, t * 2.0)
    } else {
        lerp(amber, red, (t - 0.5) * 2.0)
    }
}

/// What a trail was built for, so any change starts it over.
type TrailKey = (
    usize,
    Moment,
    usize,
    wxdata::extrema::Extremum,
    u16,
    bool,
    String,
);

/// The C2 accumulator and the frames already folded into it, oldest first.
struct TrailState {
    key: TrailKey,
    folded: Vec<String>,
    acc: Option<BinnedSweep>,
    /// Bumped on every fold or restart so the shown-image key changes as the trail grows.
    generation: u32,
    restarted: Option<wxdata::extrema::Mismatch>,
    /// When the newest folded frame was taken, for the decay step to the next one.
    last_time: Option<DateTime<Utc>>,
}

type ShownKey = (
    String,
    Moment,
    usize,
    Option<f32>,
    bool,
    Option<(u32, u32)>,
    bool,
    // Precipitation-tint generation: `None` when the tint is off, else the grid revision, so a
    // new precipitation-type grid or toggling the tint rebuilds the image.
    Option<u32>,
    // A mode switch must upload the newly masked/unmasked sweep, not only its palette.
    bool,
);

/// An in-progress offline chase-pack download: the worker outcome channel, a cancel flag the
/// workers poll, and running tallies for the Map ▸ offline-pack progress bar.
struct ChasePack {
    rx: Receiver<(bool, u64)>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    total: u64,
    done: u64,
    errors: u64,
    bytes: u64,
}

/// Build a windy.com permalink for a map position.
///
/// Grammar, from Windy's own URL-parameters documentation and matching the share links their
/// satellite view produces: `https://www.windy.com/?{overlay},{lat},{lon},{zoom}`. `lat,lon,zoom`
/// are required and must appear in that order; the overlay is optional and goes first. Two rules
/// worth keeping: **latitude comes before longitude** (the opposite of this codebase's own
/// `(lon, lat)` convention, which is exactly how that gets written backwards), and coordinates
/// must carry a decimal part or Windy ignores them.
///
/// Windy's zoom tops out at 18 and its overlay names are its own — `radar`, `satellite` and `cape`
/// all resolve, alongside the documented `wind`, `temp`, `rain` and friends.
fn windy_url(overlay: &str, lon: f64, lat: f64, zoom: f64) -> String {
    let z = zoom.round().clamp(3.0, 18.0) as u32;
    format!("https://www.windy.com/?{overlay},{lat:.3},{lon:.3},{z}")
}

/// `?embed` in the query string: this build is a chromeless pane inside another page's iframe.
/// Read once at startup — nobody flips it at runtime.
#[cfg(target_arch = "wasm32")]
fn is_embed() -> bool {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .is_some_and(|s| s.trim_start_matches('?').split('&').any(|p| p == "embed"))
}

#[cfg(not(target_arch = "wasm32"))]
fn is_embed() -> bool {
    false
}

/// The credit line a radar network's licence asks for, or `None` for one that asks for nothing
/// (NOAA's WSR-88Ds and TDWRs are public domain). One arm per source, added as sources are.
///
/// ponytail: a function, not a NOTICE file or a licence registry. Attribution belongs on the map
/// beside the data it covers; if a distribution ever needs a bundled notice, generate it from here.
fn data_attribution(site_id: &str) -> Option<&'static str> {
    match wxdata::sites::network(site_id) {
        wxdata::sites::Network::Dwd => Some("Radar data © Deutscher Wetterdienst (DL-DE/BY-2.0)"),
        // ORD publishes every member's data under one licence, and the credit EUMETNET asks for
        // names the network rather than the twenty national services behind it.
        wxdata::sites::Network::Opera => {
            Some("Radar data © EUMETNET OPERA / OpenRadarData (CC BY 4.0)")
        }
        _ => None,
    }
}

/// How a lightning flash looks at `age_secs`: bright white-hot when it just happened, fading to a
/// dim orange ember by the end of the window. The brightness IS the recency cue — a map of
/// same-colored dots says where lightning has been, not where it is now.
/// Most strikes kept at once. A continent-wide topic on an active night runs tens of thousands
/// an hour, and the painter walks the whole deque every frame.
const STRIKE_CAP: usize = 20_000;

/// How long a strike stays on the map. Same window as the GLM feed, so the two layers age alike.
const STRIKE_WINDOW_SECS: i64 = 900;

/// Cyan-white for a fresh strike fading to deep blue, deliberately nothing like [`glm_style`]'s
/// white-to-orange: with both layers on, colour is the only thing telling optical flashes from
/// ground strikes.
fn strike_style(age_secs: f32) -> (egui::Color32, f32) {
    let t = (age_secs / STRIKE_WINDOW_SECS as f32).clamp(0.0, 1.0);
    let r = 3.4 - 1.4 * t;
    let lerp = |a: f32, b: f32| (a + (b - a) * t) as u8;
    (
        egui::Color32::from_rgba_unmultiplied(
            lerp(225.0, 40.0),
            lerp(250.0, 90.0),
            lerp(255.0, 220.0),
            lerp(255.0, 70.0),
        ),
        r,
    )
}

fn glm_style(age_secs: f32) -> (egui::Color32, f32) {
    const WINDOW: f32 = 900.0; // 15 minutes, matching the feed
    let t = (age_secs / WINDOW).clamp(0.0, 1.0);
    // Newest flashes get a slightly bigger dot so a live storm reads at a glance.
    let r = 3.4 - 1.4 * t;
    let lerp = |a: f32, b: f32| (a + (b - a) * t) as u8;
    let alpha = lerp(255.0, 70.0);
    (
        egui::Color32::from_rgba_unmultiplied(
            lerp(255.0, 235.0),
            lerp(250.0, 140.0),
            lerp(210.0, 40.0),
            alpha,
        ),
        r,
    )
}

/// The first `want` indices of `elevations` at distinct angles (0.1\u{b0} tolerance), lowest
/// first. SAILS/MRLE repeat the lowest cut mid-volume, so a naive `0..4` yields duplicates.
fn distinct_tilts(elevations: &[f32], want: usize) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::with_capacity(want);
    for (i, &a) in elevations.iter().enumerate() {
        if out.len() == want {
            break;
        }
        if !out.iter().any(|&j| (elevations[j] - a).abs() < 0.1) {
            out.push(i);
        }
    }
    out
}

/// How many buttons the right-edge control column shows — the badge lane stacks below them.
const CONTROL_BUTTONS: usize = 6;

/// Everything `theme::apply` is keyed on, remembered so it only re-applies when one of them moves:
/// `(theme, system_dark, density, accent)`.
type ThemeApplied = (
    crate::settings::Theme,
    bool,
    crate::ui::m3::Density,
    Option<[u8; 3]>,
);

/// Which point-forecast series a request or cache entry is for: `(lat_e5, lon_e5, which series)`.
type ModelSeriesKey = (i32, i32, ui::forecast_window::ModelSeriesUi);

/// One point-forecast series: a value (or a gap) per valid time.
type ModelSeries = Vec<(chrono::DateTime<Utc>, Option<f32>)>;

pub struct HookEchoApp {
    /// The native runtime, kept alive for as long as the app is. Work is spawned through
    /// `spawner`, which is the same thing natively and the browser's event loop on the web.
    #[cfg(not(target_arch = "wasm32"))]
    _rt: Runtime,
    spawner: crate::rt::Spawner,
    tiles: TileManager,
    vtiles: crate::vector_tiles::VectorTileManager,
    /// One shared label-occupancy pass for the whole frame — see [`crate::labelplace`].
    labels: crate::labelplace::Placer,
    settings: Settings,
    saved: Settings,
    views: Vec<MapView>,
    active: usize,
    msg_rx: Receiver<DataMsg>,
    msg_tx: Sender<DataMsg>,
    /// Geocode results for the marker window's address search: `(label, lat, lon)` or an error.
    /// About window + the once-per-session release check.
    about_open: bool,
    update_state: ui::about_window::UpdateState,
    /// Update chip dismissed this session (see `chrome::chips::update_chip`).
    update_chip_hidden: bool,
    update_tx: Sender<ui::about_window::UpdateState>,
    update_rx: Receiver<ui::about_window::UpdateState>,
    geocode_tx: Sender<Result<(String, f64, f64), String>>,
    geocode_rx: Receiver<Result<(String, f64, f64), String>>,
    /// `(lon, lat)` from the hosting edge's own geo-IP (browser build only), used once at boot to
    /// open on the nearest radar. Never fed on native, where the saved view is the answer.
    #[cfg(target_arch = "wasm32")]
    ipgeo_tx: Sender<(f64, f64)>,
    ipgeo_rx: Receiver<(f64, f64)>,
    /// In-progress offline chase-pack tile download (basemap pre-cache for the current view).
    chasepack: Option<ChasePack>,
    /// Per-pane "what's uploaded" key, so each pane re-bins/re-uploads only on a real change.
    pane_shown: std::collections::HashMap<usize, ShownKey>,
    /// Palette generation currently baked into each pane's LUT (see [`ShownKey`]).
    pane_lut: std::collections::HashMap<usize, u64>,
    /// Last `(theme, system_dark, density, accent)` handed to `theme::apply`.
    theme_applied: Option<ThemeApplied>,
    /// When the settings tree was last diffed against the saved copy.
    settings_checked: Option<Instant>,
    /// Frame counter, only used to invalidate within-frame memos.
    frame_nr: u64,
    palette_cache: Option<(u64, std::sync::Arc<[PaletteEntry]>)>,
    /// Visible city/town labels, keyed by the visible tile ids and the label-set generation.
    #[allow(clippy::type_complexity)]
    vlabel_cache: Option<(
        (Vec<crate::render::TileId>, u64),
        std::sync::Arc<[crate::vector_tiles::PlaceLabel]>,
    )>,
    /// Last result of each per-volume detector, keyed by what it depends on (see `volume_key`).
    #[allow(clippy::type_complexity)]
    nowcast_cache: Option<(
        ((usize, String, usize), usize, u8, Option<(u32, u32)>, u64),
        Vec<(f64, f64, egui::Color32)>,
    )>,
    /// Raw debris signatures — before either direction of `wxdata::tds::cross_corroborate` runs —
    /// so the couplet layer reading this to corroborate its own couplets never sees a confidence
    /// that couplets themselves already raised. See `compute_tds`.
    tds_cache: Option<((usize, String, usize), Vec<wxdata::tds::TdsHit>)>,
    /// The *corroborated* hits `compute_tds` actually shows — deliberately not `tds_cache`'s raw
    /// ones, which predate `cross_corroborate` and so would give a score-timeline sparkline a
    /// different number than the marker label right next to it. By volume name across many
    /// volumes, the history `compute_tds_score_track` replays into a confidence timeline (same
    /// role `celltrack_cache` plays for `compute_local_tracks`). Filled once per volume, in
    /// `compute_tds`, so a timeline never re-runs corroboration for a volume already seen.
    tds_shown_cache: LruCache<String, Vec<wxdata::tds::TdsHit>>,
    /// Same shape as `tds_cache`, for the hail-spike detector.
    tbss_cache: Option<(TunedKey, Vec<wxdata::dualpol::TbssHit>)>,
    /// ZDR columns, plus the bright band read off the same volume's CC — both cost a full pass
    /// over every tilt, so they share one cache and are computed together.
    zdr_cache: Option<ZdrCache>,
    /// Raw couplets, the radar position and the tilt count scanned — same "raw" guarantee as
    /// `tds_cache`, from the other side. See `compute_couplets`.
    #[allow(clippy::type_complexity)]
    couplet_cache: Option<(
        (usize, String, usize),
        (Vec<wxdata::rotation::CoupletHit>, (f32, f32), usize),
    )>,
    /// Same role as `tds_shown_cache`, for rotation: the corroborated couplets, not `couplet_cache`'s
    /// raw ones. Filled once per volume, in `compute_couplets`.
    rot_shown_cache: LruCache<String, Vec<wxdata::rotation::CoupletHit>>,
    /// Cells found per decoded volume, so a track built over a dozen frames flood-fills each
    /// sweep once rather than once a frame.
    celltrack_cache: LruCache<String, Vec<wxdata::celltrack::Blob>>,
    /// The tracks themselves, with the frame list they were built from.
    tracks_cache: Option<((usize, String, usize), Vec<wxdata::celltrack::Track>)>,
    /// Confidence-over-time for the active pane's debris/rotation detections (C5's "timeline of
    /// score changes"), rebuilt from `tds_shown_cache`/`rot_shown_cache` the same bounded-window
    /// way `tracks_cache` is. See `compute_tds_score_track`/`compute_rot_score_track`.
    #[allow(clippy::type_complexity)]
    tds_tracks_cache: Option<((usize, String, usize), Vec<wxdata::scoretrack::ScoreTrack>)>,
    #[allow(clippy::type_complexity)]
    rot_tracks_cache: Option<((usize, String, usize), Vec<wxdata::scoretrack::ScoreTrack>)>,
    show_local_tracks: bool,
    /// The running extremum trail for the active pane (C2), built a few frames per UI frame.
    trail: Option<TrailState>,
    /// More cached frames are waiting to be folded in, so keep repainting until they are.
    trail_more: bool,
    /// The docked-panel layout's state: which panels are open, the layers tab and search.
    dock: chrome::DockState,
    /// Show the scan-age ring.
    show_scan_age: bool,
    /// The ring read off each pane's sweep as it was binned; `None` means looked, nothing to draw.
    scan_age_rings: std::collections::HashMap<usize, Option<ScanAgeRing>>,
    site_dialog: Option<ui::site_dialog::SiteDialog>,
    firstrun: ui::firstrun::FirstRun,
    /// The optional spotlight tour, and where the chrome drew the things it points at this frame.
    tour: ui::tour::Tour,
    tour_anchors: ui::tour::TourAnchors,
    settings_window: ui::settings_window::SettingsWindow,
    /// Active color tables (one per moment); reloaded when the palette settings change.
    palettes: Palettes,
    /// Live chunk stream for the active view: (view index, site, the generation it was spawned
    /// at, the provider label it was started with). Cancellation is a counter bump rather than a
    /// task abort, because the browser has no abort — `spawn_local` hands back nothing to hold.
    /// The stream reads the counter before every chunk fetch and ends itself when it no longer
    /// recognizes its own generation. The label is `None` on wasm32 (no failover manager exists
    /// there, so it never disagrees with itself); on native it's ROADMAP_NEW B6.11 step 11's own
    /// addition — a live tier switch aborts the running stream on the spot rather than waiting
    /// for it to fail/end on its own before the new tier is picked up.
    live_stream: Option<(usize, String, u64, Option<&'static str>)>,
    /// Bumped to cancel whatever stream is running. Shared with the spawned task.
    live_gen: Arc<std::sync::atomic::AtomicU64>,
    last_stream_attempt: Option<Instant>,
    /// Decoded-volume LRU keyed by AWS object name, so scrubbing back and forth on the
    /// timeline doesn't re-download. ~10 volumes; each ~a few MB.
    scan_cache: LruCache<String, Arc<Scan>>,
    /// Loop frames trimmed to the tilt on screen, so a loop can run past the scan cache.
    light_frames: long_loop::LightFrames,
    /// Loop frames being fetched ahead of the playhead, with when they were kicked off.
    ///
    /// Shared with the fetch tasks so each one gives its slot back when it ends, however it ends.
    /// The book used to be main-thread-only and expire on a timer, which meant a download slower
    /// than the timer lost its entry while still running — and the next tick started it again,
    /// and the one after that, without bound. The age-out survives as a backstop for a task that
    /// is genuinely gone.
    prefetching: Arc<Mutex<PrefetchBook>>,
    /// Browser only: the opening loop has not started yet. Cleared the moment it does, or when a
    /// deep link says the visitor asked for one specific moment rather than "show me now".
    autoplay_pending: bool,
    /// When this process started drawing — used to bound how long boot work may be deferred.
    boot_at: Instant,
    // --- Overlays (severe-weather layers; geographic, shared across views) ---
    http: reqwest::Client,
    overlay_rx: Receiver<OverlayDelivery>,
    overlay_tx: Sender<OverlayDelivery>,
    /// Rejects a background reply once a newer request in its lane has started.
    overlay_requests: std::sync::Mutex<RequestBook>,
    filters: OverlayFilters,
    alert_features: Vec<GeoFeature>,
    /// Archived storm-based warnings (feature W) keyed by 5-min UTC bucket (ts/300); shown while
    /// the active pane is scrubbed off-live.
    arch_warns: LruCache<i64, Vec<GeoFeature>>,
    /// Archived mesoscale discussions by 5-minute bucket, and the bucket being fetched and shown.
    arch_mds: LruCache<i64, Vec<GeoFeature>>,
    arch_md_inflight: Option<i64>,
    arch_md_shown: Option<i64>,
    /// The 5-min bucket currently being fetched (dedupes in-flight requests).
    arch_warn_inflight: Option<i64>,
    /// The bucket whose warnings are currently substituted into the overlay set (None = live).
    arch_warn_shown: Option<i64>,
    /// Archived local storm reports (feature CC) keyed by 30-min UTC bucket (ts/1800); shown while
    /// the active pane is scrubbed off-live (each bucket = the 6 h of reports ending there).
    arch_lsr: LruCache<i64, Vec<wxdata::spc::StormReport>>,
    arch_lsr_inflight: Option<i64>,
    arch_lsr_shown: Option<i64>,
    /// One slot per SPC outlook day, 1..=8 (Days 4–8 are the experimental probability layer).
    outlook_features: [Vec<GeoFeature>; 8],
    md_features: Vec<GeoFeature>,
    /// Watch boxes in effect, one feature per county row the service returns.
    watch_features: Vec<GeoFeature>,
    /// Winter Storm Severity Index polygons for the selected day.
    wssi_features: Vec<GeoFeature>,
    /// Excessive Rainfall Outlook polygons for the selected day.
    ero_features: Vec<GeoFeature>,
    /// SPC Fire Weather Outlook polygons (risk + dry thunderstorm) for the selected day.
    fire_features: Vec<GeoFeature>,
    /// Crowd precipitation-type reports (mPING), and their fetch clock.
    show_mping: bool,
    mping_reports: Vec<wxdata::mping::Report>,
    mping_last_fetch: Option<Instant>,
    /// Hurricane-hunter flight-track observations: toggle, obs, fetch clock.
    show_recon: bool,
    recon: Vec<wxdata::recon::HdobOb>,
    recon_last_fetch: Option<Instant>,
    /// Tropical wind-field threshold (34/50/64 kt), or `None` for no wind field, and whether
    /// to draw the potential storm-surge flooding polygons.
    tropical_wind_kt: Option<u8>,
    tropical_surge: bool,
    /// Pilot reports: toggle, current reports, fetch clock (rides the METAR view bbox).
    show_pireps: bool,
    pireps: Vec<wxdata::aviation::Pirep>,
    pirep_last_fetch: Option<Instant>,
    /// ProbSevere storm-probability polygons + badges (toggle + refresh clock).
    show_probsevere: bool,
    probsevere: Vec<GeoFeature>,
    probsevere_last_fetch: Option<Instant>,
    /// The currently-displayed, filtered feature set (hit-tested + tessellated).
    overlays: Vec<GeoFeature>,
    overlay_gen: u64,
    built_gen: u64,
    built_zoom_bucket: i32,
    /// Whether the imported GIS layer was above its minimum zoom at the last tessellation (I4).
    built_imported_visible: bool,
    built_theme: crate::settings::Theme,
    /// Whether the overlay geometry was split for the globe.
    built_globe: bool,
    pending_overlay: Option<OverlayUpload>,
    overlay_ready: bool,
    overlay_last_fetch: Option<Instant>,
    detail: Option<Detail>,
    /// Open "Storm {id} Attributes" window (a clicked storm cell).
    pub(crate) cell_popup: Option<Cell>,
    /// Whether the full storm-attributes window is showing for `cell_popup`. Always, outside the
    /// workstation; there, the Inspector's storm section is the summary and this opens on request.
    pub(crate) cell_details: bool,
    /// The workstation Cell window's Follow and View in 3D buttons, answered with the floating
    /// window's own logic below.
    pub(crate) cell_follow_toggle: bool,
    pub(crate) cell_view3d: bool,
    /// Open gate-inspector popup: every geometry/value fact about the point sampled with the
    /// explicit Gate inspector tool.
    gate_popup: Option<ui::gate_inspector::GateInspectorPopup>,
    /// Open radar-suitability popup: nearby radars ranked by beam geometry at the point sampled
    /// with the explicit Radar suitability tool.
    suitability_popup: Option<ui::suitability_popup::SuitabilityPopup>,
    /// Which of `settings.markers` the tapped-marker popup is editing.
    // ponytail: index identity — markers have no id, and their names aren't unique ("Marker 3"
    // comes back after a delete). A bounds check closes the popup if the list shrinks under it.
    marker_popup: Option<usize>,
    /// Which global model the global layers read, and how far into its run.
    global_model: wxdata::global::GlobalModel,
    global_fcst_hour: u16,
    /// The (model, hour) each global layer was last fetched for, so a change refetches at once.
    global_layer_key: std::collections::HashMap<
        crate::render::FieldLayer,
        (wxdata::global::GlobalModel, u16, Option<DateTime<Utc>>),
    >,
    /// What the difference layer differences and the exact shared valid time/source runs.
    diff_field: crate::fielddiff::DiffField,
    /// Signed `A - B` or magnitude-only `|A - B|`. The fetched CPU grid always stays signed;
    /// this controls only its upload, legend and readout.
    diff_mode: crate::fielddiff::DiffMode,
    diff_valid: Option<crate::fielddiff::ComparisonTimes>,
    diff_error: Option<String>,
    /// Which `settings.goes_satellite_west` each GOES band was last fetched for, so flipping the
    /// satellite refetches at once instead of waiting out the normal cadence.
    goes_west_key: std::collections::HashMap<crate::render::FieldLayer, bool>,
    /// The GOES RGB recipe last fetched, so picking another refetches at once.
    goes_rgb_fetched: Option<&'static str>,
    /// The ABI sector each GOES layer was last fetched from, so a change of sector (a choice, or
    /// the fallback to CONUS when a mesoscale box leaves the view) refetches at once.
    goes_fetched_sector:
        std::collections::HashMap<crate::render::FieldLayer, wxdata::goes_abi::Sector>,
    /// Where the chosen mesoscale sector was last seen pointed (ROADMAP_NEW E5).
    goes_footprint: Option<(wxdata::goes_abi::Sector, wxdata::goes_abi::Footprint)>,
    /// When the mesoscale footprint was last probed on its own (while falling back to CONUS).
    goes_footprint_probe: Option<Instant>,
    /// The scan slot each GOES layer was last fetched for (`goes_slot`): `None` for the newest,
    /// else the archive slot a scrubbed view asked for (ROADMAP_NEW A2/E7: satellite follows the
    /// radar's time).
    goes_fetched_slot: std::collections::HashMap<crate::render::FieldLayer, Option<i64>>,
    /// GLM flashes for a scrubbed-back view: the window ending at that time (ROADMAP_NEW E6).
    glm_archive: Option<(DateTime<Utc>, Vec<wxdata::glm::Flash>)>,
    /// The minute slot the GLM archive window was last asked for.
    glm_archive_slot: Option<i64>,
    /// When `goto.txt` was last looked for — see the poll in `update`.
    goto_poll: Option<Instant>,
    /// The `#goto=` fragment last applied, web only — so a kiosk tab that never navigates away
    /// (a Home Assistant browser card, a wallpanel display) picks up a new link the same way a
    /// fresh tab does. See the poll in `update` and `apply_goto_hash`'s doc comment.
    #[cfg(target_arch = "wasm32")]
    last_goto_hash: Option<String>,
    /// The last difference grid, kept on the CPU after upload so the cursor can read a number off
    /// it. A diverging color says "the models disagree here"; only a value says by how much.
    diff_grid: Option<wxdata::mrms::MrmsField>,
    /// The percent-change grid behind `DiffMode::Percent`, computed with `diff_grid` from the same
    /// two fetches (`fielddiff::percent`); `None` for a field with no meaningful ratio.
    diff_pct: Option<wxdata::mrms::MrmsField>,
    /// The field the difference layer was last fetched for, so a change refetches at once.
    diff_key: Option<(crate::fielddiff::DiffField, u16)>,
    /// Which field/mode the resident GPU upload represents. Kept separate from `diff_key` so a
    /// mode switch rebuilds from `diff_grid` without downloading either model again.
    diff_display_key: Option<(crate::fielddiff::DiffField, crate::fielddiff::DiffMode)>,
    /// The ensemble layer: which statistic of which field is shown (session-only).
    ensemble: crate::ensemble_layer::EnsembleView,
    /// The fetched GEFS members, kept so a new statistic recomputes instead of refetching.
    ensemble_run: Option<wxdata::ensemble::EnsembleRun>,
    /// The displayed statistic grid, kept on the CPU for the cursor readout.
    ensemble_grid: Option<wxdata::mrms::MrmsField>,
    /// The `(field, hour)` the members were last fetched for, so a change refetches at once.
    ensemble_key: Option<(wxdata::ensemble::EnsembleField, u16)>,
    /// Which view the resident GPU upload represents (see `EnsembleView::display_key`).
    ensemble_display_key: Option<(
        wxdata::ensemble::EnsembleField,
        crate::ensemble_layer::StatKind,
        u32,
    )>,
    ensemble_error: Option<String>,
    /// The compare panes' shared valid time and distinct source runs.
    compare_valid: Option<crate::fielddiff::ComparisonTimes>,
    compare_error: Option<String>,
    /// Both sides' grids, kept on the CPU for the same cursor-readout reason as `diff_grid`.
    compare_grid: Option<(wxdata::mrms::MrmsField, wxdata::mrms::MrmsField)>,
    /// The field the compare panes were last fetched for, so a change refetches at once.
    compare_key: Option<(crate::fielddiff::DiffField, u16)>,
    /// Where the open sounding was taken, so a forecast-hour change can refetch the same point.
    sounding_at: Option<(f64, f64)>,
    /// Vertices clicked so far with the watch-zone tool, `[lon, lat]`. Empty when not drawing.
    zone_pts: Vec<[f64; 2]>,
    /// A finished ring waiting for the user to name it.
    zone_naming: Option<(Vec<[f64; 2]>, String)>,
    /// Which of `settings.alert_polygons` the tapped-zone popup is editing.
    zone_popup: Option<usize>,
    /// A spotter dot tapped this frame, opened after the radar pane's borrows end.
    pending_spotter: Option<wxdata::spotters::Spotter>,
    /// The one open live-video window, if any (a marker's or a chase partner's stream).
    // ponytail: one at a time; a Vec of players when someone wants a wall of streams.
    video_player: Option<ui::video_window::VideoPlayer>,
    cells_window: ui::cells_window::CellsWindow,
    help_hub: ui::help_hub::HelpHub,
    rules_window: ui::rules_window::RulesWindow,
    /// The one slide-over surface every browsable tool page renders into.
    drawer: ui::drawer::Drawer,
    /// Anchors for the cards that answer a click on the map.
    popovers: ui::popover::Popovers,
    /// Warning verification lab and its in-flight query.
    verify_window: ui::verify_window::VerifyWindow,
    /// Model verification against the RTMA (ROADMAP_NEW K1): the picker, and what it last scored.
    model_verify: ui::model_verify_window::ModelVerifyWindow,
    #[allow(clippy::type_complexity)]
    model_verify_rx: Option<
        std::sync::mpsc::Receiver<
            Result<
                (
                    ui::model_verify_window::Meta,
                    Vec<wxdata::gridverify::LeadResult>,
                ),
                String,
            >,
        >,
    >,
    verify_rx: Option<std::sync::mpsc::Receiver<Result<wxdata::verify::Verification, String>>>,
    /// Which moment the cross-section slices (session state, not persisted).
    xsection_moment: Moment,
    /// Storm-follow camera: the `(site, last-snapshot cell, since)` the active pane is tracking.
    /// Each new volume recenters on this cell; a manual pan or site change cancels it.
    follow_cell: Option<(String, Cell, Instant)>,
    /// Transient "follow ended" note `(text, shown-at)`; renders in the follow badge slot ~5 s.
    follow_notice: Option<(String, Instant)>,
    /// Open "Active Warnings" window (clicked warning/watch polygons).
    warning_popup: Option<ui::warning_window::WarningPopup>,
    /// People, homes and towns inside open alerts (Census), by alert id.
    impacts: impact::ImpactBook,
    /// The impact the open feature details show (a discussion's or a watch's), by its key in
    /// `impacts`; `None` for features that have no people count.
    detail_impact: Option<String>,
    /// Newest pane error and the time it appeared, for the auto-hiding bottom-center chip.
    error_chip: Option<(String, f64)>,
    /// Search text in the mobile navigation drawer's registry list.
    /// Forecast hour each HRRR-backed field layer was last fetched for, so scrubbing the tail
    /// refetches instead of showing a stale hour until the cadence expires.
    hrrr_layer_hour:
        std::collections::HashMap<crate::render::FieldLayer, (u8, Option<DateTime<Utc>>)>,
    /// Level 3 clickable storm cells for `cells_site` (the active site when last fetched).
    storm_cells: Vec<Cell>,
    cells_site: Option<String>,
    /// The site whose earlier scans have been merged into `cell_trends`, so they are fetched once.
    cells_history_site: Option<String>,
    /// Per-cell-id trend history (VIL/top/dBZ across volumes); cleared when the site changes.
    cell_trends: std::collections::HashMap<String, Vec<ui::cell_window::CellSample>>,
    /// Last `ui_scale` pushed to egui, to tell slider changes apart from keyboard zoom.
    ui_scale_applied: f32,
    /// Android: whether we've asked for the soft keyboard (tracks egui's wants_keyboard_input).
    ime_shown: bool,
    /// Android: clipboard text read via JNI, queued for injection as an egui Paste event.
    pending_paste: Option<String>,
    /// Android: the text field that was focused when Paste was tapped. Tapping the button steals
    /// focus, so we re-focus this the frame the Paste event is delivered (else it lands nowhere).
    paste_target: Option<egui::Id>,
    /// Loaded placefile overlays (reconciled from `settings.placefiles` by URL).
    placefiles: Vec<LoadedPlacefile>,
    /// [`App::placefile_labels`] memoised, keyed by [`PlaceLabelKey`].
    placefile_label_cache: Option<(PlaceLabelKey, std::sync::Arc<[PlaceLabel]>)>,
    placefile_window: ui::placefile_window::PlacefileWindow,
    /// Phase C1's formula-product manager.
    udp_window: ui::udp_window::UdpWindow,
    /// Last map viewport size (px), used to estimate the view range for placefile thresholds.
    last_viewport: (f32, f32),
    /// Active left-click map tool.
    tool: MapTool,
    /// Which control set the WSV3 ribbon is showing (desktop/web only).
    ribbon_mode: RibbonMode,
    /// Measure-tool clicked endpoints in `[lon, lat]` (max 2).
    measure: Vec<[f64; 2]>,
    /// Manual storm-motion tracks (`app::storm_track`); session-only.
    storm_tracks: storm_track::StormTracks,
    output: output_window::OutputWindow,
    /// How long recent frames took to build (`app::telemetry`), for Analyst Mode.
    frame_times: telemetry::FrameTimes,
    /// Freehand annotation strokes, in lon/lat so they stick to the ground through pan and zoom.
    /// Session-only by design: this is for pointing at a storm on a stream, not a saved document.
    strokes: Vec<Stroke2d>,
    /// The colour the next stroke gets.
    draw_color: egui::Color32,
    marker_window: ui::marker_window::MarkerWindow,
    event_window: ui::event_window::EventWindow,
    chase_replay: ui::chase_replay::ChaseReplay,
    palette_editor: ui::palette_editor::PaletteEditor,
    digest_window: ui::digest_window::DigestWindow,
    digest_rx: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    sounding_window: ui::sounding_window::SoundingWindow,
    sounding_rx: Option<std::sync::mpsc::Receiver<Result<wxdata::sounding::Sounding, String>>>,
    /// The observed RAOB fetched alongside the HRRR profile, for the same click.
    raob_rx: Option<std::sync::mpsc::Receiver<Result<wxdata::sounding::Sounding, String>>>,
    /// The route window and its fetch in flight (ROADMAP_NEW L1-L3).
    route_window: ui::route_window::RouteWindow,
    route_rx: Option<std::sync::mpsc::Receiver<Result<Vec<wxdata::route::Route>, String>>>,
    /// Exposure along the chosen route for (route generation, overlay generation, progress in
    /// 100 m steps), so it is recomputed only when one of those moves.
    route_exposure: RouteExposure,
    /// The previous HRRR run's profile at the sounding's valid time (ROADMAP_NEW F8).
    previous_sounding_rx:
        Option<std::sync::mpsc::Receiver<Result<wxdata::sounding::Sounding, String>>>,
    /// Last spoken storm-position update: when, and the distance in whole miles it reported.
    spoke_pos: Option<(Instant, i32)>,
    /// Detections seen recently, for compound rules to ask "and was there also…". Trimmed to the
    /// compound window every pass, so it stays a handful of entries.
    recent_hits: Vec<(
        crate::settings::RuleTrigger,
        crate::rules::Detection,
        Instant,
    )>,
    /// Chase mode: follow a position, auto-switching the active pane to the nearest radar.
    chase_mode: bool,
    chase_pos: Option<(f64, f64)>,
    /// Breadcrumb track of this session's fixes (see `settings.chase_log`).
    chase_track: crate::chaselog::Track,
    /// The site last warmed ahead of a chase handoff, and when — so a fix every two seconds does
    /// not queue a download every two seconds.
    warmed_site: Option<(String, Instant)>,
    /// Tornado climatology: the loaded SPC track database (lazy), a pending async load, the last
    /// query result + its center, a window-open flag, and a query queued while the CSV loads.
    climo_tracks: Option<std::sync::Arc<Vec<wxdata::torclimo::TornadoTrack>>>,
    climo_rx:
        Option<std::sync::mpsc::Receiver<Result<Vec<wxdata::torclimo::TornadoTrack>, String>>>,
    climo_hits: Vec<wxdata::torclimo::TornadoTrack>,
    climo_center: Option<(f64, f64)>,
    climo_open: bool,
    climo_loading: bool,
    climo_error: Option<String>,
    climo_pending_query: Option<(f64, f64)>,
    /// Warning history for the same clicked point (IEM VTEC by-point), fetched alongside the
    /// tornado tracks. `Some(rx)` while the request is in flight.
    climo_warn: Option<wxdata::archive_warnings::PointSummary>,
    climo_warn_rx:
        Option<std::sync::mpsc::Receiver<Result<wxdata::archive_warnings::PointSummary, String>>>,
    chase_applied: Option<(f64, f64)>,
    /// Live position stream from gpsd, when the user has connected it.
    gps_rx: Option<std::sync::mpsc::Receiver<(f64, f64)>>,
    /// Google Drive settings sync: the saved tokens, the last-agreed state, an in-flight
    /// sign-in's code pair, the line shown in the Sync tab, worker replies, and the poll clock.
    sync_tokens: Option<crate::cloud::Tokens>,
    sync_state: crate::cloud::SyncState,
    sync_login: Option<crate::cloud::Pending>,
    sync_status: String,
    sync_rx: Option<std::sync::mpsc::Receiver<SyncMsg>>,
    sync_checked: Option<wxdata::clock::Instant>,
    /// Position sharing (LAN broadcast + optional relay), started on first use.
    share: Option<crate::share::Share>,
    /// Everyone else's last known position, keyed by their device id.
    peers: std::collections::HashMap<String, crate::share::Peer>,
    /// When we last put our own fix on the wire (both transports share the cadence).
    share_sent: Option<wxdata::clock::Instant>,
    /// GOES satellite frame times (for the sub-hourly scrub), the style they were fetched for,
    /// and the selected index (`None` = latest).
    goes_times: Vec<chrono::DateTime<chrono::Utc>>,
    goes_times_style: Option<crate::tiles::BasemapStyle>,
    goes_time_idx: Option<usize>,
    /// Keep the GOES frame on the active pane's radar clock rather than on a hand-picked frame.
    goes_follow_radar: bool,
    goes_times_rx: Option<std::sync::mpsc::Receiver<Vec<chrono::DateTime<chrono::Utc>>>>,
    /// The archive hour the loaded frame times cover (`None` = the live window ending now).
    /// Scrubbing far enough back to cross into another hour refetches; staying inside one does
    /// not, which is what keeps an archive loop from asking GIBS a question per frame.
    goes_hour: Option<i64>,
    /// The previous flash-extent grid, kept only so the lightning jump has something to subtract.
    glm_fed_prev: Option<wxdata::mrms::MrmsField>,
    /// When the Android widget snapshot was last written.
    widget_shot_at: Option<Instant>,
    /// A warning that wants a radar picture pushed after it (see `settings.ntfy_snapshot`).
    snapshot_push: Option<String>,
    /// Per-location cooldown for the rotation-near-a-watched-place alert.
    rotation_alerted: std::collections::HashMap<String, Instant>,
    /// Volume start time of the last new-scan chime (see `scan_chime`).
    last_chime: Option<chrono::DateTime<Utc>>,
    /// Pushes held back by quiet hours, replayed as one summary when the window ends.
    ///
    /// A `Mutex` because `notify_alert` takes `&self` (ten call sites); a lock beats threading
    /// `&mut` through all of them.
    /// Held across a restart through `Settings::quiet_pending`, written on exit: a quiet window
    /// that spans a relaunch still owes its catch-up.
    quiet_queue: std::sync::Mutex<Vec<(String, String)>>,
    /// Outbreak rollup state. A `Mutex` for the same reason `quiet_queue` is one: `notify_alert`
    /// takes `&self`.
    rollup: std::sync::Mutex<crate::alert_rollup::Rollup>,
    /// Whether the last frame was inside quiet hours, so the end of the window is an edge.
    was_quiet: bool,
    /// Where a requested screenshot should go once the image event arrives.
    screenshot_pending: Option<ShotDest>,
    /// A capture waiting on the share-card footer to be on screen: the destination, and how many
    /// more frames to draw the footer before asking for the image (see `share_card_footer`).
    share_card: Option<(ShotDest, u8)>,
    loop_export: Option<LoopExport>,
    /// Equal pane grid or AWIPS-style large-primary/detail-rail workspace geometry. Pane identity
    /// stays in `views` order; only its rectangle changes.
    pane_layout: crate::workspace::PaneLayout,
    /// When true, all panes share the active pane's camera.
    link_cameras: bool,
    link_times: bool,
    /// When linked, use the active radar scan's actual timestamp as the analysis cursor rather
    /// than retaining an external source's requested valid time.
    lock_source_time: bool,
    /// ROADMAP_NEW J2: when true, `PaletteAction::SetSite` sets every pane's site, not just the
    /// active one's — each pane keeps its own product/tilt, so four panes can compare products
    /// of one storm instead of becoming four copies of the same pane.
    link_site: bool,
    /// ROADMAP_NEW J3: when true, hovering any pane records the geographic point under the cursor
    /// here, so every pane can draw a matching crosshair and the probe table can sample all of
    /// them at once — a shared cursor rather than each pane's own independent hover.
    link_cursor: bool,
    /// ROADMAP_NEW J2: the selected storm (`cell_popup`) is shared by every pane — marked in each,
    /// and each recenters on it as it moves. See `paint_selected_storm`/`follow_linked_storm`.
    pub(crate) link_storm: bool,
    /// The selected storm's id and position the panes were last centered on, so the link moves
    /// them once per change rather than pinning them against the user's own panning.
    storm_link_at: Option<(String, f64, f64)>,
    /// The point `link_cursor` is currently sharing across panes, refreshed every frame from
    /// whichever pane the mouse is actually over and cleared when the pointer leaves every pane.
    /// Not persisted — a live hover position, not a saved preference.
    linked_probe: Option<(f64, f64)>,
    linked_analysis: pane_time::LinkedTimeState,
    /// The always-on-top mini-loop window is open (desktop only; see `mini_loop_viewport`).
    mini_loop: bool,
    /// The mini loop's own camera while it is open; `None` until it borrows the pane's.
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    mini_cam: Option<crate::render::mercator::Camera>,
    /// The report left by a previous run that panicked, shown once until dismissed.
    crash_report: Option<String>,
    /// National gridded field layers (MRMS mosaic, rotation, MESH, AzShear, lightning), each with
    /// its own toggle + pending GPU upload + refresh throttle. Keyed by [`crate::render::FieldLayer`].
    fields: std::collections::HashMap<crate::render::FieldLayer, FieldState>,
    /// Selected rotation-track accumulation window (minutes): 30, 60, or 120.
    rotation_minutes: u16,
    /// Selected hail-swath accumulation window (minutes); see [`wxdata::mrms::hail_swath`].
    hail_minutes: u16,
    /// Environment suite (HRRR CAPE/SRH): CAPE uses the mixed-layer (90-0 mb) parcel when true,
    /// else surface-based; SRH depth in km (1 = 0-1 km, 3 = 0-3 km). Changing either clears the
    /// layer's last_fetch so the next frame refetches.
    env_cape_ml: bool,
    env_srh_km: u8,
    /// Where the environment fields and contours come from: the HRRR forecast, or the RAP f00
    /// analysis (13 km, observation-assimilated — "mesoanalysis"). Changing it refetches both.
    env_model: wxdata::hrrr::Model,
    /// The site the L3 gridded products (DVL/EET) were last fetched for (feature X); refetch on
    /// site change.
    l3grid_site: Option<String>,
    /// What the locally derived products (VIL/VILD/echo tops) were last computed from:
    /// `(volume name, echo-top threshold, enabled-layer mask, melting level)`. Any of them moving
    /// recomputes.
    derived_key: Option<(String, u32, u8, i32)>,
    /// `(site, epoch, 0 °C height, −20 °C height)` above sea level in metres, for the hail grids
    /// and every other consumer of a melting level. `epoch` is `None` for the live HRRR analysis
    /// and the synoptic time of the observed sounding for an archived volume — read through
    /// [`App::freezing_for`], which only hands a view the levels for its own site and epoch.
    freezing: Option<(String, Option<chrono::DateTime<chrono::Utc>>, f64, f64)>,
    /// When, and for which `(site, epoch)`, the last request went out — the throttle.
    freezing_last_fetch: Option<(Instant, String, Option<chrono::DateTime<chrono::Utc>>)>,
    /// Accumulation window (hours) for the observed snowfall analysis, and the one last fetched.
    snow_hours: u16,
    snow_fetched: Option<u16>,
    /// Surface obs (METAR station plots, feature U): toggle, current obs, fetch clock + bbox.
    show_metar: bool,
    metars: Vec<wxdata::metar::SurfaceOb>,
    /// Raw TAF text by ICAO, for the station tooltips (empty where a station files none).
    tafs: std::collections::HashMap<String, String>,
    metar_last_fetch: Option<Instant>,
    /// The `(lat0, lon0, lat1, lon1)` bbox the current `metars` were fetched for.
    metar_bounds: Option<(f64, f64, f64, f64)>,
    /// River flood gauges (NWPS): toggle, current gauges, fetch clock + bbox (mirrors METAR).
    show_gauges: bool,
    gauges: Vec<wxdata::river::Gauge>,
    gauge_last_fetch: Option<Instant>,
    gauge_bounds: Option<(f64, f64, f64, f64)>,
    /// Open gauge cards (hydrograph, flood stages, crests): what clicking a gauge opens.
    gauge_cards: crate::ui::gauge_card::Cards,
    /// The flood-gauge dashboard's filters and sparklines.
    gauge_dash: crate::ui::gauge_dashboard::Dashboard,
    /// HRRR model contours: which fields are on — several can be at once, e.g. MSLP and CAPE
    /// overlaid together, each independently fetched and drawn in its own color. `Off` is never a
    /// member; `PaletteAction::SetContours(Off)` clears the whole set instead of toggling it in.
    active_contours: std::collections::BTreeSet<ContourKind>,
    /// One entry per active kind: its current polylines, valid time, fetch clock, and the
    /// (model, unit) the current lines were fetched for (drives refetch-on-change). Entries for a
    /// kind that's no longer active are dropped by `sync_contours` rather than left to go stale.
    contours: std::collections::HashMap<ContourKind, ContourEntry>,
    /// NHC tropical suite (feature V): toggle, fetched data, refresh clock. On by default like
    /// the other severe layers — an active hurricane is not something to have to go and enable.
    show_tropical: bool,
    tropical: Option<wxdata::tropical::TropicalData>,
    tropical_last_fetch: Option<Instant>,
    /// County power outages (ODIN): on by default like the tropical suite — it draws nothing
    /// until a county is significantly dark, so a quiet day costs one fetch and no pixels.
    show_outages: bool,
    outage_features: Vec<overlay::GeoFeature>,
    outages_last_fetch: Option<Instant>,
    /// CAPPI slice window (feature AA): toggle, selected altitude (km), rendered texture, and the
    /// key `(volume name, altitude bits)` the texture was built for (re-slice on change).
    show_cappi: bool,
    cappi_alt_km: f32,
    cappi_tex: Option<egui::TextureHandle>,
    cappi_key: Option<(String, u32)>,
    /// Forecast reflectivity (any regional model): selected forecast hour, last-fetched hour,
    /// run/valid times, clock.
    hrrr_fcst_hour: u8,
    hrrr_fetched_hour: Option<u8>,
    hrrr_run: Option<DateTime<Utc>>,
    hrrr_valid: Option<DateTime<Utc>>,
    hrrr_last_fetch: Option<Instant>,
    /// HRRR sub-hourly (`wrfsubhf`) mode: when on, the forecast tail is scrubbed in 15-minute
    /// steps out to 18 h instead of whole hours. `hrrr_fcst_min` is the selected lead (minutes).
    hrrr_subhourly: bool,
    hrrr_fcst_min: u16,
    hrrr_fetched_min: Option<u16>,
    /// True while the HRRR layer is being driven by a forecast-tail scrub (vs. the manual toggle).
    hrrr_by_timeline: bool,
    /// The model browser's choice: which model, and which of its products (ROADMAP_NEW F-series).
    /// The layers it puts on the map are the renderer's existing field layers; the fields below
    /// are the per-engine state those layers already read.
    model_sel: crate::model_browser::Selection,
    /// Which regional model the forecast-reflectivity layer reads.
    refl_model: wxdata::hrrr::Model,
    /// The model and run the reflectivity texture was last fetched from, so switching either
    /// refetches.
    hrrr_fetched_key: Option<(wxdata::hrrr::Model, Option<DateTime<Utc>>)>,
    /// The analysis hour each RTMA layer was last fetched for (`None` = newest).
    rtma_key: std::collections::HashMap<crate::render::FieldLayer, Option<DateTime<Utc>>>,
    /// The `(model, hour, run)` each environment layer (CAPE, SRH) was last fetched for.
    env_fetch_key: std::collections::HashMap<
        crate::render::FieldLayer,
        (wxdata::hrrr::Model, u8, Option<DateTime<Utc>>),
    >,
    /// The model run the browser has pinned (`None` = newest available). Session-only: a specific
    /// cycle is a thing to look at now, not a preference to restore.
    model_run: Option<DateTime<Utc>>,
    /// Tray-menu command channel (Linux StatusNotifier); `None` if no tray host is available.
    tray_rx: std::sync::mpsc::Receiver<crate::tray::TrayCmd>,
    /// Last state pushed to the tray, so an unchanged frame sends nothing.
    tray_state: crate::tray::TrayState,
    /// True once a StatusNotifier host has taken the tray item; registration is async, so this
    /// can flip after the first frames.
    tray_present: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Set by the tray "Quit" item so the close-to-tray handler lets the window actually close.
    really_quit: bool,
    /// Local storm-report markers (live IEM LSR feed, trailing 6 h) + toggle + refresh clock.
    show_storm_reports: bool,
    storm_reports: Vec<wxdata::spc::StormReport>,
    reports_last_fetch: Option<Instant>,
    /// Aviation SIGMET/AIRMET overlay (feature GG): toggle, features, refresh clock.
    show_aviation: bool,
    aviation_features: Vec<GeoFeature>,
    aviation_last_fetch: Option<Instant>,
    /// FAA Temporary Flight Restrictions: toggle, shapes by NOTAM id, refresh clock, and how
    /// many shapes are still unfetched (the first load comes in batches).
    /// MRMS surface precipitation classes for the reflectivity tint, kept whether or not the
    /// precipitation-type layer itself is shown. Behind an `Arc` so a pane can take a cheap
    /// handle to it while the volume it is drawing is mutably borrowed.
    precip_flag_grid: Option<std::sync::Arc<PrecipGrid>>,
    /// Bumped whenever `precip_flag_grid` is replaced, so a pane knows its upload is stale.
    precip_flag_gen: u32,
    show_tfr: bool,
    /// Forecast-zone and CWA reference layers.
    boundaries: boundaries::BoundaryState,
    tfr_features: std::collections::HashMap<String, GeoFeature>,
    tfr_last_fetch: Option<Instant>,
    tfr_pending: usize,
    /// Area Forecast Discussion window (feature DD): open flag, fetched text, in-flight receiver.
    afd_open: bool,
    afd: Option<wxdata::afd::Afd>,
    afd_error: Option<String>,
    afd_busy: bool,
    afd_rx: Option<std::sync::mpsc::Receiver<Result<wxdata::afd::Afd, String>>>,
    /// The Tropical window: model guidance picker and NHC advisory / discussion reader.
    tropical_window: ui::tropical_window::TropicalWindow,
    /// Tropical model guidance (spaghetti), best tracks and invests.
    spaghetti: crate::spaghetti::Spaghetti,
    tropical_text_rx: Option<std::sync::mpsc::Receiver<Result<wxdata::tropical::Advisory, String>>>,
    /// Range rings + azimuth spokes around the active site (feature HH).
    show_range_rings: bool,
    /// Draw all NEXRAD radar sites on the map; clicking one switches the pane to that radar.
    show_radar_sites: bool,
    /// Day/night shading, the terminator line, and the lat/lon graticule (`daynight_draw`).
    /// Client-side geometry from the current clock — no fetch, no `rebuild_overlays`.
    show_daynight: bool,
    /// Beam-vs-terrain blockage shading: the resident raster, what it was built for, and the
    /// world rect it covers. Built off-thread (it fetches DEM tiles), so it arrives on a channel.
    show_blockage: bool,
    blockage_tex: Option<(BlockageKey, egui::TextureHandle, [f64; 4])>,
    /// Key of the build in flight, and when it started — one at a time, and never more often than
    /// every 600 ms, so a continuous pan doesn't queue a raster per frame.
    blockage_pending: Option<(BlockageKey, Instant)>,
    blockage_rx: Receiver<(BlockageKey, [f64; 4], egui::ColorImage)>,
    blockage_tx: Sender<(BlockageKey, [f64; 4], egui::ColorImage)>,
    /// ROADMAP_NEW C3's "lowest usable beam map": same shape as the blockage fields above, but
    /// keyed by the volume's whole elevation list rather than one displayed tilt — it answers
    /// "which tilt do I need here", not "is my current tilt good here".
    show_lowest_tilt: bool,
    lowest_tilt_tex: Option<(LowestTiltKey, egui::TextureHandle, [f64; 4])>,
    lowest_tilt_pending: Option<(LowestTiltKey, Instant)>,
    lowest_tilt_rx: Receiver<(LowestTiltKey, [f64; 4], egui::ColorImage)>,
    lowest_tilt_tx: Sender<(LowestTiltKey, [f64; 4], egui::ColorImage)>,
    /// ROADMAP_NEW C3's "radar coverage comparison between neighboring sites": the two site ids
    /// being compared (set from the suitability popup's "Compare" button), or `None` when the
    /// overlay is off. Pure geometry (`crate::coverage_compare`) rather than a DEM fetch, so
    /// unlike `blockage_tex`/`lowest_tilt_tex` above it rebuilds synchronously — no pending/
    /// channel pair needed.
    coverage_compare: Option<(String, String)>,
    coverage_compare_tex: Option<(CoverageCompareKey, egui::TextureHandle, [f64; 4])>,
    /// ROADMAP_NEW N1's "Data Source Health panel": open flag only — the content reads straight
    /// from this frame's `palette_entries()`, no separate state to keep in sync.
    show_data_health: bool,
    /// GPU name/device-type/backend, captured once at startup (see the `wgpu_render_state` block
    /// in `new`) — ROADMAP_NEW N4's diagnostics bundle wants it long after that log line scrolled
    /// away.
    gpu_info: String,
    /// Layers panel (floating, searchable layer picker): open flag + its search text.
    /// Viewport minus the docked bars, refreshed each frame — floating `Area`s constrain to this
    /// instead of `content_rect`, which egui measures before panels take their bite.
    chrome_rect: egui::Rect,
    /// The window's width in points, as of this frame (`workstation_chrome` reads it).
    window_w: f32,
    layers_query: String,
    /// Ctrl+K command palette: open flag, query, and the highlighted row.
    /// Set by Ctrl+K so the drawer grabs the search field on the frame it opens.
    /// Is the floating left panel showing? Runtime state, not a setting: the map is the app,
    /// and a panel you left open yesterday shouldn't cover it today.
    panel_open: bool,
    /// Is the background picker slid out beside the control column?
    basemap_open: bool,
    /// Is the WSV3 ribbon (and its docked colour scale) hidden for a full-window map view?
    /// Runtime state, not a setting, same as `panel_open`: reopen the app and the ribbon is back.
    /// No-op outside the WSV3 layout, which is the only one that docks a ribbon at all.
    ribbon_collapsed: bool,
    /// Streaming mode's logo texture and the path it came from (`None` inside: unreadable).
    broadcast_logo: Option<(String, Option<egui::TextureHandle>)>,
    sidebar_focus_search: bool,
    /// The `?` keyboard cheat sheet is up.
    show_cheatsheet: bool,
    /// The Hotkeys settings tab is waiting for the next keypress to bind; the global hotkey table
    /// stands down while it is.
    capture_key: bool,
    /// Top search pill: the place query and a transient "flew to …" status.
    place_query: String,
    place_status: Option<(String, Instant)>,
    /// After a search flies somewhere, the offer to keep it: `(name, lat, lon, when)`. Searching is
    /// how you look around, so dropping a pin every time would litter the map — this asks first.
    save_offer: Option<(String, f64, f64, Instant)>,
    /// True while the in-flight geocode came from the search pill (navigate only) rather than
    /// the marker window (which adds a marker).
    geocode_nav: bool,
    /// Layer manager window (per-placefile enable/order/opacity).
    layer_window_open: bool,
    /// Placefile icon-sheet textures by URL. `None` = fetch in flight or failed (negative-cached
    /// so a broken sheet isn't retried every frame), same idiom as `marker_icon_tex`.
    pf_icon_tex: std::collections::HashMap<String, Option<egui::TextureHandle>>,
    pf_icon_rx: Receiver<(String, egui::ColorImage)>,
    pf_icon_tx: Sender<(String, egui::ColorImage)>,
    /// Android: hide all floating chrome to view the whole radar (toggled by the eye button).
    mobile_chrome_hidden: bool,
    /// Touch: when and where the last tap landed, so a drag that starts right on top of it is
    /// read as the second half of a double-tap-drag zoom rather than a pan.
    last_tap: Option<(f64, egui::Pos2)>,
    /// Touch: the anchor of a double-tap-drag zoom in progress (`None` = a drag pans).
    tap_zoom: Option<egui::Pos2>,
    /// Android: rects the mobile chrome covers this frame. Two-finger gestures are read straight
    /// off the raw input, which has no idea egui drew a sheet over the map, so the pane input
    /// block checks the gesture center against these.
    mobile_occlusion: Vec<egui::Rect>,
    /// When the last two-finger gesture ended. Lifting one finger of a pinch leaves the other
    /// one down, which egui immediately reads as a click and a fresh drag — an interrogate popup
    /// and a jump for what was only the end of a zoom. A short cooldown eats both.
    last_gesture_end: Option<wxdata::clock::Instant>,
    /// Spotter Network positions + toggle + refresh clock (filtered to active site at draw).
    show_spotters: bool,
    /// FAA WeatherCams: the toggle, the sites in view, and the bbox//time they were fetched for.
    show_webcams: bool,
    webcams: Vec<wxdata::webcams::CamSite>,
    webcam_bounds: Option<(f64, f64, f64, f64)>,
    webcam_last_fetch: Option<Instant>,
    /// WFIGS wildfires: perimeters (tessellated with the other overlay polygons), incident points,
    /// and the bbox/clock they were fetched for.
    show_fires: bool,
    fire_perims: Vec<GeoFeature>,
    fire_incidents: Vec<wxdata::wfigs::FireIncident>,
    fire_bounds: Option<(f64, f64, f64, f64)>,
    fire_last_fetch: Option<Instant>,
    /// ROADMAP_NEW I1: shapes from a user-imported GeoJSON or Shapefile (converted by `gis_import`,
    /// see that module's own doc comment) — no fetch/clock fields the way the feed-backed layers
    /// above have, since there is no feed to refresh, only the one file the user picked.
    show_imported_gis: bool,
    imported_gis: Vec<GeoFeature>,
    /// The imported points and lines, which `GeoFeature`'s rings-only shape cannot hold — painted
    /// directly by `render_pane` beside the freehand annotation strokes.
    imported_marks: crate::gis_import::Marks,
    /// The imported features' colours by `settings.imported_gis_color_by` and their legend,
    /// for the attribute they were computed for; cleared on import.
    imported_colors: Option<crate::gis_import::ColoredBy>,
    /// The imported features' valid windows for the mapped start/end attributes (I5), for the
    /// attributes they were read with; cleared on import.
    imported_time: Option<ImportedTime>,
    /// Which imported features are valid at the view's time; `None` when no time attribute is
    /// mapped, which shows them all.
    imported_shown: Option<Vec<bool>>,
    /// AirNow AQI dots: toggle, the obs in view, and the bbox/clock they were fetched for. Needs
    /// a user key; without one the layer never fetches.
    show_aqi: bool,
    aqi: Vec<wxdata::airnow::AqiOb>,
    aqi_bounds: Option<(f64, f64, f64, f64)>,
    aqi_last_fetch: Option<Instant>,
    /// Live station cards: the toggle, the layer state, and the poll clocks behind it.
    show_stations: bool,
    stations: crate::stationlayer::Layer,
    station_last_poll: Option<Instant>,
    ppef_last_fetch: Option<Instant>,
    dotcam_bounds: Option<(f64, f64, f64, f64)>,
    /// NOAA Weather Radio: the running player (dropping it stops playback) and the relay picked
    /// in the drawer.
    #[cfg(not(target_arch = "wasm32"))]
    nwr: Option<crate::nwr::Player>,
    nwr_pick: String,
    /// NWS damage surveys: the toggle, the last result, and the `(bbox, day)` it was fetched for
    /// (surveys never change, so the key alone decides when to refetch).
    show_dat: bool,
    dat_points: Vec<wxdata::dat::DamagePoint>,
    dat_tracks: Vec<wxdata::dat::DamageTrack>,
    dat_key: Option<((f64, f64, f64, f64), chrono::NaiveDate)>,
    /// Multi-radar mosaic: which sites the last composite used, the oldest scan in it, the view it
    /// was built for — panning off the composite refetches instead of leaving a stale picture.
    mosaic_sites: Vec<String>,
    mosaic_oldest: Option<chrono::DateTime<chrono::Utc>>,
    mosaic_bounds: Option<(f64, f64, f64, f64)>,
    spotters: Vec<wxdata::spotters::Spotter>,
    spotters_last_fetch: Option<Instant>,
    /// Sensor dashboard: open flag, latest fetch (Ok/Err), the site it's for, and a refresh clock.
    show_sensors: bool,
    sensor_data: Option<Result<wxdata::obs::StationObs, String>>,
    sensor_site: Option<String>,
    sensor_last_fetch: Option<Instant>,
    /// VAD hodograph: open flag, latest profile, its site, and a refresh clock.
    show_hodo: bool,
    hodo_data: Vec<wxdata::level3::VwpLevel>,
    /// Profiles collected this session, oldest→newest. The radar only ever publishes the newest
    /// one, so a time-height view has to be accumulated live.
    hodo_history:
        std::collections::VecDeque<(chrono::DateTime<Utc>, Vec<wxdata::level3::VwpLevel>)>,
    hodo_tab: ui::hodograph_window::Tab,
    /// Tap-for-forecast: window state, the tapped point, the in-flight fetch, and a short cache
    /// keyed by rounded lat/lon.
    forecast_open: bool,
    forecast_at: Option<(f64, f64)>,
    forecast_state: ui::forecast_window::State,
    #[allow(clippy::type_complexity)]
    forecast_rx: Option<(
        (i32, i32),
        std::sync::mpsc::Receiver<Result<wxdata::forecast::PointForecast, String>>,
    )>,
    forecast_cache:
        std::collections::HashMap<(i32, i32), (Instant, wxdata::forecast::PointForecast)>,
    /// Current conditions for the same tapped point, fetched beside the forecast and cached the
    /// same way. Separate from the Obs overlay, which is radar-site-scoped and only live when that
    /// overlay is on.
    #[allow(clippy::type_complexity)]
    forecast_obs_rx: Option<(
        (i32, i32),
        std::sync::mpsc::Receiver<(String, wxdata::obs::Observation)>,
    )>,
    forecast_obs_cache:
        std::collections::HashMap<(i32, i32), (Instant, String, wxdata::obs::Observation)>,
    /// The forecast window's model-meteogram picker (model/field/period) and what it's holding.
    model_series_ui: ui::forecast_window::ModelSeriesUi,
    model_series_state: ui::forecast_window::SeriesState,
    model_series_rx: Option<(
        ModelSeriesKey,
        std::sync::mpsc::Receiver<Result<ModelSeries, String>>,
    )>,
    model_series_cache: std::collections::HashMap<ModelSeriesKey, (Instant, ModelSeries)>,
    /// The forecast window's ensemble-plume picker and what it is holding (ROADMAP_NEW F7).
    plume_ui: ui::forecast_window::PlumeUi,
    plume_state: ui::forecast_window::PlumeState,
    #[allow(clippy::type_complexity)]
    plume_rx: Option<(
        (i32, i32, ui::forecast_window::PlumeUi),
        std::sync::mpsc::Receiver<Result<Vec<wxdata::ensemble::PlumePoint>, String>>,
    )>,
    #[allow(clippy::type_complexity)]
    plume_cache: std::collections::HashMap<
        (i32, i32, ui::forecast_window::PlumeUi),
        (Instant, Vec<wxdata::ensemble::PlumePoint>),
    >,
    /// Rain-arrival alerting: per-point persistence/cooldown state, plus the current ETAs for the
    /// on-map chip.
    rain_detector: crate::rain_arrival::Detector,
    /// Volume the rain check last ran against, so it runs per scan (what the detector's
    /// persistence and cooldown are written for) instead of per frame.
    rain_key: Option<(usize, String, usize)>,
    /// When the flash-extent grid was last built. Its own clock, because a rule can want the
    /// grid while the layer that used to own the cadence is off.
    glm_fed_last: Option<Instant>,
    /// Same per-volume guard for the user's scan rules: evaluated once per volume, not per frame.
    rules_key: Option<(usize, String, usize)>,
    /// When each rule last fired, keyed `"{rule id}:{place}"` — a rule watching two places gets
    /// to speak about both.
    rules_fired: std::collections::HashMap<String, Instant>,
    rain_eta: Vec<(String, f32)>,
    /// Minute-by-minute rain over the forecast point, and the (volume, point, motion) key it was
    /// computed for — the walk samples the whole sweep, so it runs once per volume, not per frame.
    minute_profile: Option<Vec<Option<f32>>>,
    minute_key: Option<String>,
    /// WPC surface analysis overlay: fronts + pressure centers, refreshed a few times an hour.
    show_fronts: bool,
    fronts: Option<wxdata::fronts::SurfaceAnalysis>,
    fronts_last_fetch: Option<Instant>,
    /// GOES satellite lightning: the rolling flash window and its poll clock. The feed lives
    /// behind a mutex because the poll runs on the tokio runtime while the painter reads it.
    show_glm: bool,
    glm: std::sync::Arc<std::sync::Mutex<wxdata::glm::GlmFeed>>,
    glm_last_poll: Option<Instant>,
    glm_polling: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Ground strikes as they arrive over MQTT, oldest first. Nothing fills this unless the user
    /// points `strikes_topic` at a broker that carries them — the app never talks to a strike
    /// network itself.
    show_strikes: bool,
    strikes: std::collections::VecDeque<(f64, f64, chrono::DateTime<chrono::Utc>)>,
    /// Animated wind particles. The grids are shared; the particle sets are per pane, because each
    /// pane has its own camera. Nothing here persists to settings — neither do fronts or GLM.
    show_wind: bool,
    wind: Option<crate::wind_draw::WindField>,
    wind_level: wxdata::hrrr::WindLevel,
    wind_particles: std::collections::HashMap<usize, crate::wind_draw::Particles>,
    /// Whether the particles are advected on the GPU. `HOOKECHO_CPU_WIND=1` forces the CPU mesh,
    /// which is also what runs if the GPU layer ever fails to build.
    wind_on_gpu: bool,
    /// The wind field the GPU copy was uploaded from, so a frame that has not changed is not
    /// re-warped and re-uploaded.
    wind_uploaded: Option<(chrono::DateTime<chrono::Utc>, u8, wxdata::hrrr::WindLevel)>,
    /// What the current grids are of, so a level or forecast-hour change refetches at once.
    wind_fetched: Option<(wxdata::hrrr::WindLevel, u8)>,
    /// Wind particles from the radar's Doppler velocity instead of the HRRR.
    radar_wind: radar_wind::RadarWindState,
    /// The ground under the 3D map (ROADMAP_NEW H5).
    terrain3d: terrain3d::Terrain3d,
    /// Y'all mode's own outlooks and card state.
    yall: yall_mode::YallState,
    /// Where the layer probe is pinned: `(pane, lon, lat)`.
    layer_probe_pin: Option<(usize, f64, f64)>,
    wind_last_fetch: Option<Instant>,
    /// When the in-flight fetch started, or `None` if none is. One at a time: 10 m u+v is 4.5 MB
    /// an hour, and a fast scrub across the forecast tail would otherwise queue ~82 MB of GRIB
    /// behind itself. A timestamp rather than a flag because `spawn_overlay` drops fetch errors
    /// into the log — a plain flag would never be cleared on failure and would wedge the layer.
    wind_inflight: Option<Instant>,
    /// Previous frame's instant, and the clamped timestep derived from it. Computed once per
    /// frame so every pane advects by the same amount.
    wind_last_frame: Option<Instant>,
    wind_dt: f32,
    hodo_site: Option<String>,
    hodo_last_fetch: Option<Instant>,
    /// Streamer/OBS mode: hide all chrome (drawer/pills/docks), leaving only the map.
    obs_mode: bool,
    /// `?embed` in the browser build: chromeless map inside someone else's iframe (StormDesk).
    /// Hides chrome like OBS mode, and idles at one frame a minute until the visitor touches it —
    /// an embedded radar repainting at 10 fps costs the host page a whole core.
    embed: bool,
    /// Set by the first interaction with an embedded map: from then on it repaints normally.
    embed_live: bool,
    /// When the user last did anything — the input the idle heartbeat listens for. Also what
    /// "a gesture is in progress" is read from.
    last_input: Instant,
    /// A drag, touch, scroll or pinch is active this frame. Read at the top of `ui`, because the overlay
    /// tessellation asks it before any pane has drawn.
    gesture_live: bool,
    /// Perf readout state: whether `HOOKECHO_PERF=1` asked for the window, the frame count and
    /// mark the frames-per-minute number is derived from, and the last idle interval requested.
    #[cfg(not(target_arch = "wasm32"))]
    perf: PerfReadout,
    /// Last pane state posted to the parent frame, so only real changes cross the boundary.
    #[cfg(target_arch = "wasm32")]
    last_posted: Option<crate::workspace::PaneSnap>,
    /// Auto-tour: cycle the camera through active-warning centroids while in OBS mode.
    obs_tour: bool,
    obs_tour_last: Option<Instant>,
    obs_tour_idx: usize,
    /// Warning dedupe keys already seen (VTEC event keys), so a new warning is detected on
    /// arrival and a continuation of one already announced is not.
    known_warning_ids: std::collections::HashSet<String>,
    /// False until the first alert fetch seeds `known_warning_ids` (avoids alerting on startup).
    warnings_seeded: bool,
    /// Per-location cooldown clock for the lightning-proximity alarm (re-alert after it goes quiet).
    lightning_alerted: std::collections::HashMap<String, Instant>,
    /// True while a TDS is currently detected, so the alert fires on the rising edge only.
    tds_active: bool,
    /// True while a rotation couplet is currently detected (rising-edge alarm latch).
    rot_active: bool,
    /// Active new-warning banners (event, area, first-seen time); expire after a while.
    warning_banners: Vec<(String, String, Instant)>,
    /// Transient results of things the user just did (export saved, encode failed). The third
    /// lane, distinct from the warning banners (weather) and the error chip (radar feed).
    toasts: Vec<Toast>,
    /// Auxiliary feeds already reported to the user; see [`HookEchoApp::drain_feed_errors`].
    feed_errors_told: std::collections::HashMap<String, Instant>,
    /// Right-dock active-alerts panel toggle.
    show_alert_panel: bool,
    /// The region-statistics tool's box, samples and window; see `app/region_stats.rs`.
    region: region_stats::RegionStatsState,
    /// The local API server and what it was last told; see `app/local_api.rs`.
    #[cfg(not(target_arch = "wasm32"))]
    local_api: local_api::LocalApiState,
    /// Cross-section tool: clicked endpoints `[lon,lat]` (max 2), the built section + its texture.
    xsection_pts: Vec<[f64; 2]>,
    xsection: Option<wxdata::xsection::CrossSection>,
    xsection_tex: Option<egui::TextureHandle>,
    /// Whether the cross-section window draws its beam-rise overlay (ROADMAP_NEW C3 /
    /// suggestions.md §3.2). Pure display state — the geometry is already sitting in `xsection`'s
    /// own `beam_lines` regardless, so toggling this never needs a rebuild.
    xsection_beam_rise: bool,
    /// Lazily-loaded textures for uploaded marker icons, keyed by filename. `None` = load failed
    /// (negative-cached so a missing/corrupt file isn't retried every frame).
    marker_icon_tex: ui::marker_window::IconTextures,
    /// 3D raymarch view: open flag, orbit camera (az/el degrees + distance), and a pending
    /// volume upload (taken by the first paint after a rebuild).
    show_3d: bool,
    vol3d: ui::volume3d_window::Volume3dState,
    /// Which volume the built grid belongs to, so reopening the window doesn't rebuild it.
    /// `(volume name, grid size, tilt count)` — the tilt count so a volume that is still
    /// streaming its higher sweeps rebuilds the 3D grid as each one lands.
    /// What the 3D Reflectivity window's volume was built from: volume, grid size, tilt count,
    /// and the tilts pulled out (elevation bits; empty = the whole interpolated volume).
    vol3d_key: Option<(String, usize, usize, Vec<u32>)>,
    /// In-flight build (the resample runs off the UI thread).
    #[allow(clippy::type_complexity)]
    vol3d_rx: Option<std::sync::mpsc::Receiver<(crate::render3d::Volume3dUpload, (f32, f32))>>,
    /// The built volume's dBZ span, which the window's threshold slider works in.
    vol3d_range: (f32, f32),
    vol3d_pending: Option<crate::render3d::Volume3dUpload>,
    /// The map-pitch "Smooth" 3D representation, one slot per pane (the app's own 4-pane
    /// ceiling) rather than the window's single `vol3d_*` set — more than one pane can be
    /// showing it at once, each its own site and volume.
    ///
    /// `(volume name, tilt count, moment)` last built per pane — the moment is part of the key so
    /// switching between the reflectivity Smooth volume and the CC Debris volume on the same pane
    /// rebuilds instead of showing the stale one; an in-flight receiver while a rebuild runs
    /// off-thread; a finished upload waiting for `render_pane`'s GPU callback to consume it; and
    /// the small geometry facts (`n, nz, half_km, top_km`) of whatever is currently GPU-resident,
    /// kept separately from the (heavy) upload so the frames between rebuilds don't need the
    /// tens-of-MB volume held twice just to recompute the camera uniform.
    smooth_vol_key: [Option<SmoothKey>; crate::view::MAX_PANES],
    /// Per pane: the isosurface shells shown and the [`IsoKey`] they were built for
    /// (ROADMAP_NEW H3).
    iso_mesh: [Option<(IsoKey, Arc<Vec<IsoShell>>)>; crate::view::MAX_PANES],
    /// Per pane: built Smooth volumes and isosurfaces by frame, so a loop plays its 3D frame for
    /// frame (ROADMAP_NEW H8), and every 3D build in flight.
    loop3d: [Loop3dCache; crate::view::MAX_PANES],
    loop3d_jobs: Loop3dJobs,
    /// GOES cloud top height for the 3D map's satellite surface (ROADMAP_NEW H6), keyed by
    /// (five-minute slot of the view's time, West satellite), and the fetch in flight.
    cloud_top: Option<((i64, bool), wxdata::mrms::MrmsField)>,
    /// The HRRR's isotherm heights (0, -10, -20 °C, km MSL) for the 3D map's model surfaces
    /// (ROADMAP_NEW H6), for the hour they were fetched in, with the run they came from.
    model_isotherms: Option<(i64, ModelIsotherms)>,
    model_isotherms_rx: Option<(i64, std::sync::mpsc::Receiver<ModelIsotherms>)>,
    cloud_top_rx: Option<(
        (i64, bool),
        std::sync::mpsc::Receiver<wxdata::mrms::MrmsField>,
    )>,
    /// `(horizontal cell km, share of echo outside the box)` of each pane's resident Smooth volume,
    /// for the readout under the representation buttons.
    smooth_vol_info: [Option<(f32, f32)>; crate::view::MAX_PANES],
    /// The value range a pane's user-defined-product volume was drawn over.
    smooth_vol_range: [Option<(f32, f32)>; crate::view::MAX_PANES],
    /// The device's 3D texture edge limit, which caps the Smooth grid.
    vol3d_max_dim: usize,
    smooth_vol_pending: [Option<Arc<crate::render3d::Volume3dUpload>>; crate::view::MAX_PANES],
    smooth_vol_dims: [Option<(u32, u32, f32, f32)>; crate::view::MAX_PANES],
    /// GPU 2D texture-size cap (device limit), used to clamp field-grid decimation on mobile GPUs.
    max_texture_dim: u32,
    /// Whether this device can hold the 3D texture the raymarch window needs. See its assignment
    /// in `new` — it is a property of the adapter that turned up, not of the platform.
    volume3d_supported: bool,
}

/// Half a flash cycle, ms: a range flashes on for one beat and off for the next.
const FLASH_BEAT_MS: u64 = 450;
/// The colour a flashing range takes on its bright beats.
const FLASH_RGBA: [u8; 4] = [255, 255, 255, 255];

/// Whether a flashing range is on its bright beat. With reduced motion it stays on: a steady
/// highlight, not a blink.
fn flash_beat_on() -> bool {
    crate::ui::motion::reduced()
        || (chrono::Utc::now().timestamp_millis() as u64 / FLASH_BEAT_MS).is_multiple_of(2)
}

impl HookEchoApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
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
            live_stream: None,
            live_gen: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            last_stream_attempt: None,
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
            overlay_requests: std::sync::Mutex::new(RequestBook::default()),
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
            global_layer_key: std::collections::HashMap::new(),
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
            hrrr_layer_hour: std::collections::HashMap::new(),
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
            hrrr_fetched_hour: None,
            hrrr_run: None,
            hrrr_valid: None,
            hrrr_last_fetch: None,
            hrrr_subhourly: false,
            hrrr_fcst_min: 15,
            hrrr_fetched_min: None,
            hrrr_by_timeline: false,
            model_sel: crate::model_browser::Selection::default(),
            refl_model: wxdata::hrrr::Model::Hrrr,
            hrrr_fetched_key: None,
            rtma_key: std::collections::HashMap::new(),
            model_run: None,
            env_fetch_key: std::collections::HashMap::new(),
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
            tds_active: false,
            rot_active: false,
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
            vol3d_key: None,
            vol3d_rx: None,
            vol3d_range: (-30.0, 80.0),
            vol3d_pending: None,
            // `[None; MAX_PANES]` needs `Option<T>: Copy`, which a
            // `Receiver`/`Volume3dUpload` inside it is not; `from_fn` avoids that requirement.
            smooth_vol_key: std::array::from_fn(|_| None),
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

    /// `HOOKECHO_GOTO=SITE,lon,lat,zoom[,RFC3339]` opens straight onto a view, archive time and
    /// all — the same deep link the Event Library uses, minus the clicking.
    ///
    /// This exists for the screenshot harness (`scripts/shots/`): staging a historic storm by
    /// driving the UI means hunting for a window's row coordinates, which breaks the moment the
    /// layout moves. Companion to the headless `HOOKECHO_CAM`/`HOOKECHO_BASEMAP` knobs.
    fn apply_goto_env(&mut self) {
        let Ok(v) = std::env::var("HOOKECHO_GOTO") else {
            return;
        };
        self.apply_goto(&v);
    }

    /// Consume a `goto.txt` dropped in the storage base by the Android notification tap
    /// (`MainActivity`), which is how a background alert deep-links into the storm it fired on.
    /// The file is deleted as it's read so the jump happens once, not on every resume.
    fn drain_goto_file(&mut self) {
        let Some(path) = crate::paths::goto_file() else {
            return;
        };
        let Ok(v) = std::fs::read_to_string(&path) else {
            return;
        };
        let _ = std::fs::remove_file(&path);
        self.apply_goto(v.trim());
    }

    /// `SITE[,lon,lat,zoom][,extra…]`, with or without the `hookecho://goto/` prefix.
    fn apply_goto(&mut self, v: &str) {
        let Some(g) = parse_goto(v) else {
            log::warn!("HOOKECHO_GOTO: want SITE[,lon,lat,zoom[,RFC3339|product|tilt]], got {v:?}");
            return;
        };
        self.firstrun.open = false;
        self.settings.setup_done = true;
        self.goto_view(&g.site, g.lon, g.lat, g.zoom, g.time);
        let view = &mut self.views[self.active];
        if let Some(m) = g.moment {
            view.moment = m;
        }
        if let Some(t) = g.tilt {
            view.tilt = t;
        }
        if let Some(slug) = &g.basemap {
            view.basemap = crate::tiles::BasemapStyle::from_slug(slug);
        }
        // After the moment, so `VEL,thr:15` thresholds velocity and not whatever was showing.
        if let Some(t) = g.threshold {
            let mi = view.moment.index();
            view.threshold_enabled[mi] = t.is_some();
            if t.is_some() {
                view.thresholds[mi] = t;
            }
        }
        if g.srv {
            view.srv = true;
        }
        if !g.gauges.is_empty() {
            self.show_gauges = true;
            for lid in &g.gauges {
                self.gauge_cards.queue(lid);
            }
        }
        if let Some(id) = g.tropical {
            self.show_tropical = true;
            self.spaghetti.enabled = true;
            if !id.is_empty() {
                self.spaghetti.focus = Some(id.clone());
                self.tropical_window.storm_id = Some(id);
                self.tropical_window.tab = ui::tropical_window::Tab::Models;
                self.tropical_window.open = true;
            }
        }
    }

    /// The browser's own deep link: `https://…/#goto=KTLX,-97.3,35.3,9`. Called at boot, and again
    /// from the poll in `update` — a tab that never navigates away (a Home Assistant browser
    /// card, a kiosk display, a bookmarked tab someone keeps open) only ever gets a fragment-only
    /// navigation when the link changes, which the browser treats as same-document and never
    /// reloads. Without the re-check that tap silently did nothing.
    /// Percent-escapes are decoded by `parse_goto`, so a fragment that came back from a chat
    /// client with its commas and colons escaped still opens.
    #[cfg(target_arch = "wasm32")]
    fn apply_goto_hash(&mut self) {
        let Some(h) = web_sys::window().and_then(|w| w.location().hash().ok()) else {
            return;
        };
        if self.last_goto_hash.as_deref() == Some(h.as_str()) {
            return;
        }
        self.last_goto_hash = Some(h.clone());
        if let Some(v) = h.strip_prefix("#goto=") {
            self.apply_goto(v);
            // A shared link points at one moment on purpose. Auto-playing away from it would
            // throw away the thing the sender was pointing at.
            self.autoplay_pending = false;
        }
    }

    /// Ask our own origin where the visitor is (`/geo.json`, answered by the Pages Worker
    /// or the Cloudflare Worker from the request's geo-IP) and open on the nearest radar site.
    ///
    /// Same-origin, so no proxy and no CORS; the reply is `[lon, lat]` or `null` when the edge has
    /// no fix. A deep link wins — someone who opened a share link asked for that site, not this one.
    #[cfg(target_arch = "wasm32")]
    fn locate_by_ip(&mut self, ctx: &egui::Context) {
        let Some(win) = web_sys::window() else {
            return;
        };
        if win.location().hash().is_ok_and(|h| h.starts_with("#goto=")) {
            return;
        }
        let Ok(origin) = win.location().origin() else {
            return;
        };
        let http = self.http.clone();
        let tx = self.ipgeo_tx.clone();
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            // index.html starts this fetch before the wasm has even downloaded, so by now it has
            // almost always landed: the opening site costs no round trip. Falling back to our own
            // request keeps the app working when the page didn't (an embed, a local build).
            let body = match Self::page_geo().await {
                Some(b) => b,
                None => {
                    let Ok(resp) = http.get(format!("{origin}/geo.json")).send().await else {
                        return;
                    };
                    let Ok(b) = resp.text().await else {
                        return;
                    };
                    b
                }
            };
            if let Ok(Some(pos)) = serde_json::from_str::<Option<(f64, f64)>>(&body) {
                let _ = tx.send(pos);
                ctx.request_repaint();
            }
        });
    }

    /// The body of the `/geo.json` fetch `web/index.html` started at page load, if it started one.
    #[cfg(target_arch = "wasm32")]
    async fn page_geo() -> Option<String> {
        use wasm_bindgen::JsCast as _;
        let promise: js_sys::Promise = js_sys::Reflect::get(&js_sys::global(), &"__geo".into())
            .ok()?
            .dyn_into()
            .ok()?;
        wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .ok()?
            .as_string()
    }

    /// Post the active pane's state to the embedding page (`?embed`), tagged so the host can tell
    /// it apart from every other frame shouting into the same window. `"*"` as the target origin:
    /// this is view state, nothing secret, and the host has to origin-check us regardless.
    #[cfg(target_arch = "wasm32")]
    fn post_state_to_parent(&mut self) {
        let snap = crate::workspace::PaneSnap::capture(&self.views[self.active]);
        if self.last_posted.as_ref() == Some(&snap) {
            return;
        }
        let Some(win) = web_sys::window() else {
            return;
        };
        // No parent (or we are the top frame): nobody to tell.
        let Ok(parent) = win.parent() else { return };
        let Some(parent) = parent.filter(|p| p != &win) else {
            return;
        };
        let Ok(mut v) = serde_json::to_value(&snap) else {
            return;
        };
        if let Some(o) = v.as_object_mut() {
            o.insert("hookecho".into(), serde_json::json!(1));
            // The product goes out as its share-link code ("REF"), not the serde variant name:
            // the host hands this straight back to us in a `#goto`, and `Moment::from_code` only
            // knows the codes. Round-tripping the variant name would silently drop the product.
            o.insert("moment".into(), serde_json::json!(snap.moment.short_name()));
        }
        if parent
            .post_message(&wasm_bindgen::JsValue::from_str(&v.to_string()), "*")
            .is_ok()
        {
            self.last_posted = Some(snap);
        }
    }

    /// Volume poll cadence, doubled on a metered link. A phone on mobile data pulls a multi-MB
    /// volume every interval; halving that rate costs at most a couple of minutes of latency on
    /// the live head, which the chunk stream covers anyway when it is running.
    fn poll_interval_secs(&self) -> u64 {
        let base = self.settings.poll_interval_secs;
        let base = if crate::platform::is_metered() {
            base * 2
        } else {
            base
        };
        // Battery saver stacks with metering: both are "spend less", and a chaser who has turned
        // both on has said so twice.
        if self.settings.battery_saver {
            base * 2
        } else {
            base
        }
    }

    /// Largest edge a national field grid may keep. Bounded by what the GPU will accept, and
    /// then hard-capped: at 8192 a single f32 grid is ~268 MB of RAM before it is ever indexed,
    /// which neither a phone nor a browser tab should be asked to hold — and the Adreno 750
    /// reports 16384, so the device limit alone never bit. 4096 is still finer than either
    /// screen can show.
    fn field_texture_cap(&self) -> usize {
        let ceiling = if cfg!(target_os = "android") || cfg!(target_arch = "wasm32") {
            // The browser is in the phone's bracket here, not the desktop's: an 8192 f32 grid is
            // ~268 MB staged before it is ever indexed, and a wasm heap only grows.
            4096
        } else {
            8192
        };
        (self.max_texture_dim as usize).min(ceiling)
    }

    /// Spawn background fetches for all overlay sources (alerts, SPC outlooks, MDs).
    /// Spawn a background overlay fetch, routing the result to `overlay_rx`.
    /// Which moments the active pane's volume carries. All true when nothing is loaded, so an
    /// empty pane still offers the full product list.
    fn available_moments(&self) -> [bool; Moment::ALL.len()] {
        // The pane's remembered union, not this instant's volume: a half-arrived live volume
        // carries fewer moments than the radar sends, and the rows must not blink.
        self.views[self.active].moments()
    }

    /// Recompute the locally derived products (VIL, VIL density, echo tops) when the active pane's
    /// volume, the echo-top threshold, or the set of enabled derived layers changed.
    ///
    /// Unlike every other field layer this costs no network — the volume is already decoded here —
    /// so it has no cadence: it recomputes exactly when its inputs move, which is what makes it
    /// work in archive replay and on each live tilt.
    fn recompute_derived(&mut self, ctx: &egui::Context) {
        use crate::render::FieldLayer as FL;
        const LAYERS: [FL; 6] = [
            FL::CompositeLocal,
            FL::VilLocal,
            FL::VilDensity,
            FL::EtopLocal,
            FL::HailMehs,
            FL::HailPosh,
        ];
        /// Bit positions in the mask for the two hail grids.
        const HAIL_BITS: u8 = 0b11000;
        let mask = LAYERS
            .iter()
            .enumerate()
            .fold(0u8, |m, (i, l)| m | u8::from(self.field_wanted(*l)) << i);
        if mask == 0 {
            self.derived_key = None;
            return;
        }
        let site = self.views[self.active].site.clone();
        // The hail algorithm needs the melting level: the live HRRR analysis while following the
        // feed, the observed sounding from that day on an archived volume (`freezing_for` never
        // mixes the two). Only worth a request when a hail grid is actually on.
        let levels = if mask & HAIL_BITS != 0 {
            self.freezing_for(self.active)
        } else {
            None
        };
        if mask & HAIL_BITS != 0 && levels.is_none() {
            self.fetch_freezing_levels(ctx, self.active);
        }
        // Beam heights are above the radar; the model heights are above sea level.
        let radar_m = site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
            .map_or(0.0, |s| s.elevation_meters as f64);
        let Some(vol) = self.views[self.active].volume.as_mut() else {
            return;
        };
        let key = (
            vol.name.clone(),
            self.settings.etop_dbz.to_bits(),
            mask,
            levels.map_or(0, |(h0, _)| h0 as i32),
        );
        if self.derived_key.as_ref() == Some(&key) {
            return;
        }
        // Binning is cached on the volume; the integral is the expensive half and runs off-thread.
        let sweeps = vol.reflectivity_tilts();
        if sweeps.len() < 2 {
            return;
        }
        let opts = wxdata::derived::DerivedOpts {
            etop_dbz: self.settings.etop_dbz,
            time: vol.time,
            ..Default::default()
        };
        self.derived_key = Some(key);
        let tx = self.overlay_tx.clone();
        let lane = RequestLane::Feed(FeedSource::DerivedRadarFields);
        let generation = self
            .overlay_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .start(lane.clone());
        let cap = self.field_texture_cap();
        let ctx = ctx.clone();
        self.spawner.spawn_blocking(move || {
            let mut out: Vec<(FL, wxdata::mrms::MrmsField)> = Vec::new();
            if mask & !HAIL_BITS != 0 {
                if let Some(d) = wxdata::derived::derive(&sweeps, &opts) {
                    out.extend([
                        (FL::CompositeLocal, d.composite),
                        (FL::VilLocal, d.vil),
                        (FL::VilDensity, d.vild),
                        (FL::EtopLocal, d.etop),
                    ]);
                }
            }
            if let Some((h0, hm20)) = levels.filter(|_| mask & HAIL_BITS != 0) {
                if let Some(h) = wxdata::derived::hail(&sweeps, h0 - radar_m, hm20 - radar_m, &opts)
                {
                    out.extend([(FL::HailMehs, h.mehs), (FL::HailPosh, h.posh)]);
                }
            }
            for (layer, f) in out {
                let bit = LAYERS.iter().position(|l| *l == layer).unwrap_or(0);
                if mask & (1 << bit) != 0 {
                    let _ = tx.send(OverlayDelivery::Fetched {
                        lane: lane.clone(),
                        generation,
                        result: Ok(OverlayMsg::Field(layer, f.decimated(cap))),
                    });
                }
            }
            ctx.request_repaint();
        });
    }

    /// Refresh the melting-level heights the hail grids need, on the environment cadence.
    /// Which melting level view `idx` wants: `None` for the live HRRR analysis while it follows
    /// the feed, or the synoptic launch at or before its volume's scan time when it is scrubbing
    /// the archive — the observed ascent that day, not today's model.
    fn freezing_epoch(&self, idx: usize) -> Option<chrono::DateTime<chrono::Utc>> {
        let v = &self.views[idx];
        if v.timeline.following {
            return None;
        }
        v.volume
            .as_ref()
            .map(|vol| wxdata::raob::synoptic_before(vol.time))
    }

    /// `(0 °C, −20 °C)` heights above sea level for view `idx`, only when the cached ones were
    /// fetched for its own site *and* epoch — a live reading must never stand in for an archived
    /// storm's, or one site's for another's.
    fn freezing_for(&self, idx: usize) -> Option<(f64, f64)> {
        let site = self.views[idx].site.as_deref()?;
        let epoch = self.freezing_epoch(idx);
        self.freezing
            .as_ref()
            .filter(|(s, e, ..)| s == site && *e == epoch)
            .map(|(.., h0, hm20)| (*h0, *hm20))
    }

    /// Request the melting level view `idx` wants (see [`Self::freezing_epoch`]). Throttled per
    /// `(site, epoch)`: the live analysis refreshes on the 15-minute environment cadence, and an
    /// archived ascent never changes, so the same cadence only paces retries after a failure.
    fn fetch_freezing_levels(&mut self, ctx: &egui::Context, idx: usize) {
        let Some(site) = self.views[idx]
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
        else {
            return;
        };
        let epoch = self.freezing_epoch(idx);
        if self
            .freezing_last_fetch
            .as_ref()
            .is_some_and(|(t, s, e)| s == site.id && *e == epoch && t.elapsed().as_secs() < 900)
        {
            return;
        }
        self.freezing_last_fetch = Some((Instant::now(), site.id.to_string(), epoch));
        self.spawn_overlay(
            ctx,
            OverlaySource::FreezingLevels {
                site: site.id.to_string(),
                lon: site.longitude as f64,
                lat: site.latitude as f64,
                elev_m: site.elevation_meters as f64,
                epoch,
            },
        );
    }

    fn spawn_overlay(&self, ctx: &egui::Context, source: OverlaySource) {
        let lane = source.lane();
        let generation = self
            .overlay_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .start(lane.clone());
        let http = self.http.clone();
        let tx = self.overlay_tx.clone();
        let ctx = ctx.clone();
        let cap = self.field_texture_cap();
        self.spawner.spawn(async move {
            // Deliberately shorter than the 120 s refresh that drives this: a fetch that cannot
            // outlive its own cadence cannot stack. Before, a feed the network swallowed left a
            // task alive forever and the next tick started another one on top of it.
            let result = match wxdata::task::timeout(OVERLAY_TIMEOUT, source.fetch(&http))
                .await
                .unwrap_or_else(Err)
            {
                Ok(msg) => {
                    // Max-pool oversized grids here, on the fetch task: MRMS rotation tracks and
                    // AzShear arrive 14000x7000, and doing this on the UI thread stalled a frame
                    // for the whole pool.
                    Ok(match msg {
                        OverlayMsg::Field(layer, f) => OverlayMsg::Field(layer, f.decimated(cap)),
                        OverlayMsg::StampedField(layer, f) => {
                            let kind = layer
                                .descriptor()
                                .map_or(wxdata::field::ValueKind::Scalar, |d| d.value_kind);
                            OverlayMsg::StampedField(layer, f.for_display(cap, kind))
                        }
                        OverlayMsg::MrmsField(layer, f, request) => {
                            let kind = layer
                                .descriptor()
                                .map_or(wxdata::field::ValueKind::Scalar, |d| d.value_kind);
                            OverlayMsg::MrmsField(layer, f.for_display(cap, kind), request)
                        }
                        other => other,
                    })
                }
                Err(e) => Err(e.to_string()),
            };
            let _ = tx.send(OverlayDelivery::Fetched {
                lane,
                generation,
                result,
            });
            ctx.request_repaint();
        });
    }

    /// Hazard kind for the current outlook day: probabilistic layers exist only for Day 1;
    /// Days 2–3 always fetch the categorical risk.
    fn outlook_kind_for_day(&self) -> wxdata::spc::OutlookKind {
        if self.filters.outlook_day == 1 {
            self.filters.outlook_kind
        } else {
            wxdata::spc::OutlookKind::Categorical
        }
    }

    fn fetch_overlays(&mut self, ctx: &egui::Context) {
        self.overlay_last_fetch = Some(Instant::now());
        // Scope zone-only alert resolution (heat, advisories) to the active radar and to every
        // saved marker, so an advisory at the far edge of a wide zone resolves for the places
        // people actually care about, not just the radar — see `alerts::fetch_active`.
        let mut points: Vec<(f64, f64)> = self.views[self.active]
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
            .map(|s| (s.latitude as f64, s.longitude as f64))
            .into_iter()
            .collect();
        points.extend(self.settings.markers.iter().map(|m| (m.lat, m.lon)));
        self.spawn_overlay(ctx, OverlaySource::Alerts(points, self.view_bounds()));
        self.spawn_overlay(ctx, OverlaySource::Mds);
        self.spawn_overlay(ctx, OverlaySource::Watches);
        if (1..=3).contains(&self.filters.wssi_day) {
            self.spawn_overlay(ctx, OverlaySource::Wssi(self.filters.wssi_day));
        }
        if (1..=3).contains(&self.filters.ero_day) {
            self.spawn_overlay(ctx, OverlaySource::Ero(self.filters.ero_day));
        }
        if (1..=2).contains(&self.filters.fire_day) {
            self.spawn_overlay(ctx, OverlaySource::FireWx(self.filters.fire_day));
        }
        // Only fetch the SPC outlook the user has selected (off = day 0 fetches nothing).
        if (1..=8).contains(&self.filters.outlook_day) {
            self.spawn_overlay(
                ctx,
                OverlaySource::Outlook(self.filters.outlook_day, self.outlook_kind_for_day()),
            );
        }
        // Storm cells for the active view's site (Level 3 products are per-site). Terminal and
        // DWD radars don't publish the storm-cell algorithms under their four-letter id.
        if let Some(site) = self.views[self.active]
            .site
            .clone()
            .filter(|s| wxdata::sites::is_nexrad(s))
        {
            let history = self.cells_history_site.as_deref() != Some(site.as_str());
            self.spawn_overlay(ctx, OverlaySource::Cells(site, history));
        }
    }

    /// Reconcile loaded placefiles with `settings.placefiles`: fetch new/enabled URLs, drop
    /// removed ones, mirror the enabled flag, and refetch on each file's `RefreshSeconds`.
    fn sync_placefiles(&mut self, ctx: &egui::Context) {
        // Plugins ride the same pipeline as placefiles — they produce the same format — keyed by
        // a synthetic `plugin:<name>` instead of a URL.
        let plugin_keys: Vec<(String, bool)> = self
            .settings
            .plugins
            .iter()
            .map(|p| (format!("plugin:{}", p.name), p.enabled))
            .collect();
        // Drop entries no longer configured.
        let before = self.placefiles.len();
        self.placefiles.retain(|lp| {
            self.settings.placefiles.iter().any(|c| c.url == lp.url)
                || plugin_keys.iter().any(|(k, _)| *k == lp.url)
        });
        let mut changed = self.placefiles.len() != before;
        for (key, enabled) in &plugin_keys {
            match self.placefiles.iter_mut().find(|lp| lp.url == *key) {
                Some(lp) => {
                    if lp.enabled != *enabled {
                        lp.enabled = *enabled;
                        changed = true;
                    }
                }
                None => {
                    changed = true;
                    self.placefiles.push(LoadedPlacefile {
                        url: key.clone(),
                        enabled: *enabled,
                        pf: Default::default(),
                        last_fetch: None,
                        loaded: false,
                        error: None,
                    });
                }
            }
        }
        for cfg in &self.settings.placefiles {
            match self.placefiles.iter_mut().find(|lp| lp.url == cfg.url) {
                Some(lp) => {
                    if lp.enabled != cfg.enabled {
                        lp.enabled = cfg.enabled;
                        changed = true;
                    }
                }
                None => {
                    changed = true;
                    self.placefiles.push(LoadedPlacefile {
                        url: cfg.url.clone(),
                        enabled: cfg.enabled,
                        pf: Default::default(),
                        last_fetch: None,
                        loaded: false,
                        error: None,
                    });
                }
            }
        }
        // Fetch never-loaded and refresh stale (min 15s cadence).
        let mut to_fetch = Vec::new();
        for lp in &self.placefiles {
            if !lp.enabled {
                continue;
            }
            // A plugin's cadence is the user's setting, not the placefile's own RefreshSeconds:
            // a plugin sampling something live should be asked again on a schedule they control.
            let plugin_secs = self
                .settings
                .plugins
                .iter()
                .find(|p| lp.url == format!("plugin:{}", p.name))
                .map(|p| p.refresh_secs);
            let stale = match (lp.last_fetch, plugin_secs) {
                (None, _) => true,
                // A failed plugin retries on its cadence rather than every frame.
                (Some(t), Some(secs)) => t.elapsed().as_secs() >= secs.max(5) as u64,
                (Some(t), None) => {
                    lp.loaded
                        && lp.pf.refresh_secs > 0
                        && t.elapsed().as_secs() >= lp.pf.refresh_secs.max(15) as u64
                }
            };
            if stale {
                to_fetch.push(lp.url.clone());
            }
        }
        for url in to_fetch {
            if let Some(lp) = self.placefiles.iter_mut().find(|lp| lp.url == url) {
                lp.last_fetch = Some(Instant::now());
            }
            let source = match self
                .settings
                .plugins
                .iter()
                .find(|p| url == format!("plugin:{}", p.name))
            {
                #[cfg(not(target_arch = "wasm32"))]
                Some(p) => OverlaySource::Plugin(
                    url.clone(),
                    p.command.clone(),
                    p.args.clone(),
                    self.plugin_context(),
                ),
                #[cfg(target_arch = "wasm32")]
                Some(_) => OverlaySource::Placefile(url),
                None => OverlaySource::Placefile(url),
            };
            self.spawn_overlay(ctx, source);
        }
        if changed {
            self.overlay_gen = self.overlay_gen.wrapping_add(1);
        }
    }

    /// What the active pane is showing, for a plugin to answer about. Archive-aware: scrubbing
    /// back gives the plugin the historic instant, not now.
    #[cfg(not(target_arch = "wasm32"))]
    fn plugin_context(&self) -> crate::plugins::Context {
        let v = &self.views[self.active];
        let (min_lon, min_lat, max_lon, max_lat) = self.view_bounds();
        crate::plugins::Context {
            site: v.site.clone().unwrap_or_default(),
            bbox: (min_lon, min_lat, max_lon, max_lat),
            time: v
                .timeline
                .current()
                .and_then(|id| id.date_time())
                .unwrap_or_else(chrono::Utc::now),
            product: v.moment.short_name().to_string(),
        }
    }

    /// Vector-mean storm motion (`dir_deg`, `speed_kt`) over the current SCIT storm cells that
    /// carry a movement, or `None` if none do. Averages u/v so directions wrap correctly.
    fn scit_mean_motion(&self) -> Option<(f32, f32)> {
        let (mut u, mut v, mut n) = (0.0f32, 0.0f32, 0u32);
        for c in &self.storm_cells {
            if let (Some(dir), Some(spd)) = (c.mvt_deg, c.mvt_kt) {
                let r = dir.to_radians();
                u += spd * r.sin();
                v += spd * r.cos();
                n += 1;
            }
        }
        if n == 0 {
            return None;
        }
        let (u, v) = (u / n as f32, v / n as f32);
        let dir = u.atan2(v).to_degrees().rem_euclid(360.0);
        Some((dir, (u * u + v * v).sqrt()))
    }

    /// Height of pane `idx`'s beam centre above the radar, in feet, over the point `ll`
    /// (`[lon, lat]`). `None` when the pane has no site or no loaded tilt.
    ///
    /// Ground range is close enough to slant range for the shallow tilts this is read at, and the
    /// 4/3-earth model is the same one the cross-section draws with
    /// ([`wxdata::xsection::beam_height_km`]), so the two agree.
    fn beam_height_ft(&self, idx: usize, ll: [f64; 2]) -> Option<f64> {
        let v = &self.views[idx];
        let site = wxdata::sites::site_by_id(v.site.as_deref()?)?;
        let elev = *v.volume.as_ref()?.elevations.get(v.tilt)? as f64;
        let (km, _) = crate::geo::great_circle([site.longitude as f64, site.latitude as f64], ll);
        Some(wxdata::xsection::beam_height_km(km, elev) * 3280.84)
    }

    /// Chime when a new volume lands on the live pane you are watching — the "look up" cue for
    /// someone doing something else while a storm is on.
    ///
    /// Only the active pane, only while following the head, and only for a volume newer than the
    /// last one chimed for: a scrub, a backfilled frame, or the same volume growing another chunk
    /// is not a new scan. The first volume after a site switch or a cold start is swallowed, since
    /// that one is the user's own doing.
    fn scan_chime(&mut self, view: usize, time: chrono::DateTime<Utc>) {
        if !self.settings.scan_chime || view != self.active || !self.views[view].timeline.following
        {
            return;
        }
        let first = self.last_chime.is_none();
        if self.last_chime.is_some_and(|t| t >= time) {
            return;
        }
        self.last_chime = Some(time);
        if !first {
            self.play_alert(&self.settings.scan_sound.clone());
        }
    }

    /// Everywhere the proximity alerts watch: the saved markers, plus your own live position when
    /// "alert where I am" is on and there is a fix. Returns `(name, lon, lat, radius_mi)`.
    ///
    /// The GPS entry is deliberately not a real marker — a marker that moves would drag its way
    /// through the saved list, and it must vanish the moment the fix or the setting does.
    fn watched_points(&self) -> Vec<WatchedPoint> {
        let mut out: Vec<WatchedPoint> = self
            .settings
            .markers
            .iter()
            .map(|m| WatchedPoint {
                id: m.id.clone(),
                name: m.name.clone(),
                lon: m.lon,
                lat: m.lat,
                radius_mi: m.alert_radius_mi,
            })
            .collect();
        if self.settings.alert_follow_gps {
            if let Some((lon, lat)) = self.chase_pos {
                out.push(WatchedPoint {
                    id: GPS_POINT_ID.to_string(),
                    name: "my location".to_string(),
                    lon,
                    lat,
                    radius_mi: crate::settings::default_alert_radius_mi(),
                });
            }
        }
        out
    }

    /// Is the local clock inside the user's quiet-hours window?
    fn in_quiet_hours(&self) -> bool {
        use chrono::Timelike;
        self.settings.in_quiet_hours(chrono::Local::now().hour())
    }

    /// Surface auxiliary-feed failures queued by [`note_feed_error`], at most once per feed per
    /// half hour.
    ///
    /// Rate-limited rather than silenced: a feed that is down stays down, and a toast every
    /// refresh would be the nag this app does not do — but told-once-forever also swallowed a
    /// genuine second outage hours after the feed had recovered. The log keeps every occurrence.
    fn drain_feed_errors(&mut self) {
        let queued: Vec<(String, String)> = match FEED_ERRORS.lock() {
            Ok(mut q) => std::mem::take(&mut *q),
            Err(_) => return,
        };
        for (feed, err) in queued {
            let due = self
                .feed_errors_told
                .get(&feed)
                .is_none_or(|t| t.elapsed().as_secs() >= 1800);
            if due {
                self.toast(ToastKind::Error, format!("{feed} unavailable — {err}"));
                self.feed_errors_told.insert(feed, Instant::now());
            }
        }
    }

    /// Kilometres from the pane's radar to a point — how "closest detection" is judged.
    fn distance_from_radar_km(&self, idx: usize, lon: f64, lat: f64) -> f64 {
        let Some(site) = self.views[idx]
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
        else {
            return 0.0;
        };
        crate::geo::great_circle([site.longitude as f64, site.latitude as f64], [lon, lat]).0
    }

    /// Kick off a backtest of one rule against an archive day, on the shared runtime.
    ///
    /// The site is whichever radar the active pane is on: a rule about a place is only replayable
    /// against a radar that can see it, and the pane the user is looking at is the best guess
    /// anyone can make without asking.
    fn start_backtest(&mut self, rule_idx: usize, day: chrono::NaiveDate) {
        let Some(rule) = self.settings.alert_rules.get(rule_idx).cloned() else {
            return;
        };
        let Some(site) = self.views[self.active].site.clone() else {
            return;
        };
        let shared: crate::backtest::Shared = Default::default();
        self.rules_window.backtest = Some(shared.clone());
        let settings = self.settings.clone();
        self.spawner
            .spawn(crate::backtest::run(site, day, rule, settings, shared));
    }

    /// Remember detections so a compound rule can ask about them next pass, and forget anything
    /// older than the compound window.
    fn note_hits(
        &mut self,
        trigger: &crate::settings::RuleTrigger,
        hits: &[crate::rules::Detection],
    ) {
        let window = std::time::Duration::from_secs_f64(crate::rules::COMPOUND_WINDOW_MIN * 60.0);
        self.recent_hits.retain(|(_, _, t)| t.elapsed() < window);
        for h in hits {
            self.recent_hits.push((trigger.clone(), *h, Instant::now()));
        }
    }

    /// The recent detections in the shape `rules::compound_ok` wants.
    fn recent_for_rules(&self) -> Vec<crate::rules::RecentHit> {
        self.recent_hits
            .iter()
            .map(|(trigger, hit, t)| crate::rules::RecentHit {
                trigger: trigger.clone(),
                hit: *hit,
                age_min: t.elapsed().as_secs_f64() / 60.0,
            })
            .collect()
    }

    /// Deliver a rule's alert, unless its cooldown for this place is still running.
    ///
    /// Keyed by rule *and* place, so a rule watching two zones can speak about each of them.
    fn fire_rule(&mut self, rule: &crate::settings::AlertRule, hit: &crate::rules::Detection) {
        self.fire_rule_named(rule, hit, None);
    }

    /// [`Self::fire_rule`], with the body spelled out. Warning rules name the event they matched,
    /// which the trigger label alone does not carry.
    fn fire_rule_named(
        &mut self,
        rule: &crate::settings::AlertRule,
        hit: &crate::rules::Detection,
        detail: Option<String>,
    ) {
        let place = crate::rules::place_label(&rule.place, &self.settings);
        let key = format!("{}:{place}", rule.id);
        let cooldown = std::time::Duration::from_secs(u64::from(rule.cooldown_min) * 60);
        if self
            .rules_fired
            .get(&key)
            .is_some_and(|t| t.elapsed() < cooldown)
        {
            return;
        }
        self.rules_fired.insert(key, Instant::now());
        let title = format!("\u{25c9} {}", rule.title());
        let body = match (detail, hit.strength) {
            (Some(d), _) => format!("{d} at {place}"),
            (None, Some(v)) => format!("{} \u{2014} {v:.0} at {place}", rule.trigger.label()),
            (None, None) => format!("{} at {place}", rule.trigger.label()),
        };
        if rule.snapshot {
            // Same one-picture-per-pass rule the warning snapshot follows: newest wins.
            self.snapshot_push = Some(format!("{title} — {place}"));
        }
        self.notify_alert(&title, &body, rule.urgent);
        if let Some(sound) = rule.sound.clone() {
            if self.settings.alert_sound && !self.settings.mute_alerts {
                self.play_alert(&sound);
            }
        }
        self.banner(title, body);
    }

    /// Chime + push when cloud-to-ground lightning density exceeds a small threshold within ~15 km
    /// of any saved location. Debounced per location (re-alerts only after ≥10 min of quiet) so a
    /// persistent storm doesn't spam. No-op unless the opt-in alarm is enabled and locations exist.
    fn check_lightning_proximity(&mut self, field: &wxdata::mrms::MrmsField) {
        if !self.settings.lightning_alarm || self.settings.markers.is_empty() {
            return;
        }
        const RADIUS_KM: f64 = 15.0;
        const DENSITY_MIN: f32 = 0.05; // strikes/km²/min — any recent CG activity nearby
        const COOLDOWN: std::time::Duration = std::time::Duration::from_secs(600);
        let mut fired = false;
        // Collected first: the alert calls below need `&mut self`.
        let near: Vec<(String, String)> = self
            .watched_points()
            .into_iter()
            .filter(|p| field.max_within_km(p.lon, p.lat, RADIUS_KM) >= DENSITY_MIN)
            .map(|p| (p.id, p.name))
            .collect();
        for (id, name) in near {
            let recent = self
                .lightning_alerted
                .get(&id)
                .is_some_and(|t| t.elapsed() < COOLDOWN);
            if recent {
                continue;
            }
            self.lightning_alerted.insert(id, Instant::now());
            self.notify_alert(
                &format!("⚡ Lightning near {name}"),
                &format!("Cloud-to-ground strikes within {RADIUS_KM:.0} km of {name}"),
                false,
            );
            self.banner(
                format!("⚡ Lightning near {name}"),
                format!("within {RADIUS_KM:.0} km"),
            );
            fired = true;
        }
        if fired && self.settings.alert_sound {
            self.play_alert(&self.settings.lightning_sound.clone());
        }
    }

    /// Per-minute rain over `at` for the next hour, advected off the current volume. Live only —
    /// an advection off an archived scan describes a time that already happened. Cached per
    /// (volume, point, storm motion); recomputing the 61-point walk every frame is wasted work.
    fn minute_profile(&mut self, at: (f64, f64)) -> Option<&[Option<f32>]> {
        let idx = self.active;
        if !self.views[idx].timeline.following {
            self.minute_key = None;
            self.minute_profile = None;
            return None;
        }
        let (dir, kt) = self.scit_mean_motion()?;
        let vol = self.views[idx]
            .timeline
            .current()
            .map(|id| id.name().to_string())
            .unwrap_or_default();
        let key = format!("{vol}|{:.4},{:.4}|{dir:.0},{kt:.0}", at.0, at.1);
        if self.minute_key.as_deref() != Some(key.as_str()) {
            self.minute_key = Some(key);
            self.minute_profile = self.compute_minute_profile(at, dir, kt);
        }
        self.minute_profile.as_deref()
    }

    fn compute_minute_profile(
        &mut self,
        at: (f64, f64),
        dir: f32,
        kt: f32,
    ) -> Option<Vec<Option<f32>>> {
        let idx = self.active;
        let tilt = self.views[idx].tilt;
        let sweep = self.views[idx]
            .volume
            .as_mut()
            .and_then(|v| v.binned(Moment::Reflectivity, tilt, false).ok())
            .cloned()?;
        let sample = refl_sampler(&sweep);
        crate::rain_arrival::upstream_profile(sample, [at.0, at.1], dir as f64, kt as f64, 60)
    }

    /// Name of the source the forecast-reflectivity layer is drawing, for stamps and the banner.
    fn refl_source_label(&self) -> String {
        if self.hrrr_subhourly {
            crate::model_browser::BModel::Hrrr15.label().into()
        } else {
            self.refl_model.label().into()
        }
    }

    /// Drive the forecast-reflectivity layer from the active pane's timeline: scrubbing into the
    /// forecast tail enables HRRR at that forecast hour (and suppresses the observed radar for the
    /// scrubbed pane, done at draw time); scrubbing back to observed frames turns it off again.
    fn sync_forecast_scrub(&mut self) {
        use crate::render::FieldLayer as FL;
        match self.views[self.active].timeline.forecast_hour() {
            Some(h) => {
                self.hrrr_fcst_hour = h;
                // The timeline tail is hourly; keep the sub-hourly lead in step with it so
                // scrubbing works the same in either mode.
                self.hrrr_fcst_min = u16::from(h) * 60;
                self.views[self.active].fields_on.insert(FL::Hrrr);
                self.hrrr_by_timeline = true;
                // The browser follows what the scrub put on the map.
                let scrubbed = crate::model_browser::Selection {
                    model: if self.hrrr_subhourly {
                        crate::model_browser::BModel::Hrrr15
                    } else {
                        crate::model_browser::BModel::from_regional(self.refl_model)
                    },
                    product: crate::model_browser::Product::Reflectivity,
                };
                if self.model_sel != scrubbed {
                    self.model_sel = scrubbed;
                }
            }
            None => {
                if self.hrrr_by_timeline {
                    self.views[self.active].fields_on.remove(&FL::Hrrr);
                    self.hrrr_by_timeline = false;
                }
            }
        }
    }

    /// Load any marker icon files not yet in the texture cache (negative-cached on failure).
    fn load_marker_icons(&mut self, ctx: &egui::Context) {
        let Some(dir) = crate::settings::Settings::marker_icons_dir() else {
            return;
        };
        for m in &self.settings.markers {
            let Some(name) = &m.icon else { continue };
            if self.marker_icon_tex.contains_key(name) {
                continue;
            }
            let tex = std::fs::read(dir.join(name))
                .ok()
                .and_then(|bytes| image::load_from_memory(&bytes).ok())
                .map(|img| {
                    let rgba = img.to_rgba8();
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    let ci = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                    ctx.load_texture(format!("marker-{name}"), ci, egui::TextureOptions::LINEAR)
                });
            if tex.is_none() {
                log::warn!("marker icon load failed: {name}");
            }
            self.marker_icon_tex.insert(name.clone(), tex);
        }
    }

    fn build_xsection(&mut self, idx: usize, ctx: &egui::Context) {
        let (a, b) = (self.xsection_pts[0], self.xsection_pts[1]);
        let Some(vol) = self.views[idx].volume.as_mut() else {
            return;
        };
        let moment = self.xsection_moment;
        let sweeps = vol.moment_tilts(moment); // owned → the &mut vol borrow ends here
        if sweeps.is_empty() {
            return;
        }
        let Some(xs) = wxdata::xsection::build(&sweeps, (a[0], a[1]), (b[0], b[1]), 300, 120, 18.0)
        else {
            return;
        };
        let hc_table =
            crate::colormap::effective_table(&self.palettes, moment, self.settings.theme);
        let img = ui::xsection_window::to_image(&xs, &hc_table);
        self.xsection_tex = Some(ctx.load_texture("xsection", img, egui::TextureOptions::LINEAR));
        self.xsection = Some(xs);
    }

    /// Detail card for a tapped Spotter Network dot. Their report text sometimes carries a
    /// stream link; when it does, the card offers it as a link (which is also the Watch path).
    fn open_spotter(&mut self, sp: &wxdata::spotters::Spotter) {
        let body = format!(
            "{}\n{}",
            crate::timefmt::fmt_date_clock(sp.time, self.active_tz()),
            sp.status
        );
        let link = first_url(&sp.status).map(|u| ("▶ Watch stream".to_string(), u));
        self.cell_popup = None;
        self.detail = Some(Detail {
            title: sp.name.clone(),
            body,
            color: [0, 200, 80, 255],
            image: None,
            link,
        });
    }

    /// Open a stream URL: direct HLS/MJPEG plays in the in-app player, everything else (YouTube,
    /// Twitch, a station's watch page) goes to the system browser.
    // ponytail: no yt-dlp sidecar — the browser already plays those better than we would.
    fn watch_stream(&mut self, title: String, url: String) {
        if url.is_empty() {
            return;
        }
        if ui::video_window::playable_in_app(&url) {
            self.video_player = Some(ui::video_window::VideoPlayer::start(
                title,
                url,
                &self.spawner,
            ));
        } else if let Err(e) = crate::platform::open_url(&url) {
            log::warn!("open stream URL failed: {e}");
        }
    }

    /// Fetch the Piper voice model into the data directory.
    ///
    /// The model and the `.onnx.json` beside it are both required — Piper reads its sample rate
    /// and phoneme table from the JSON — so a download that gets one and not the other is a
    /// failure, not a half-success. Written to a `.part` file and renamed, so an interrupted
    /// download cannot leave a truncated model that Piper would then crash on.
    ///
    // ponytail: no pinned hash. The download is https from Piper's own voice repository and both
    // files are validated (the JSON parses, the model is the size of a model); pinning a digest
    // means pinning a version, and I could not verify one offline to pin.
    #[cfg(not(target_arch = "wasm32"))]
    fn download_voice(&self, id: String) {
        let Some(path) = crate::speech::voice_path(&id) else {
            crate::speech::set_voice_status(false, "no data directory to download into");
            return;
        };
        let Some(url) = crate::speech::voice_url(&id) else {
            crate::speech::set_voice_status(false, "that is not a Piper voice id");
            return;
        };
        let http = self.http.clone();
        self.spawner.spawn(async move {
            let cfg_url = format!("{url}.json");
            let cfg_path = path.with_extension("onnx.json");
            if let Some(dir) = path.parent() {
                if let Err(e) = std::fs::create_dir_all(dir) {
                    crate::speech::set_voice_status(
                        false,
                        format!("could not create {dir:?}: {e}"),
                    );
                    return;
                }
            }
            crate::speech::set_voice_status(true, "downloading voice (~60 MB)…");
            let get = |url: String| {
                let http = http.clone();
                async move {
                    http.get(url)
                        .send()
                        .await?
                        .error_for_status()?
                        .bytes()
                        .await
                }
            };
            let cfg = match get(cfg_url).await {
                Ok(b) => b,
                Err(e) => {
                    crate::speech::set_voice_status(false, format!("voice config failed: {e}"));
                    return;
                }
            };
            if serde_json::from_slice::<serde_json::Value>(&cfg).is_err() {
                crate::speech::set_voice_status(false, "voice config was not JSON; refusing it");
                return;
            }
            let model = match get(url).await {
                Ok(b) => b,
                Err(e) => {
                    crate::speech::set_voice_status(false, format!("voice download failed: {e}"));
                    return;
                }
            };
            // A medium-quality Piper voice is tens of megabytes; anything tiny is an error page
            // that happened to arrive with a 200.
            if model.len() < 1_000_000 {
                crate::speech::set_voice_status(false, "download was too small to be a voice");
                return;
            }
            let part = path.with_extension("part");
            let wrote = std::fs::write(&part, &model)
                .and_then(|()| std::fs::rename(&part, &path))
                .and_then(|()| std::fs::write(&cfg_path, &cfg));
            match wrote {
                Ok(()) => crate::speech::set_voice_status(false, "voice ready"),
                Err(e) => {
                    crate::speech::set_voice_status(false, format!("writing the voice failed: {e}"))
                }
            }
        });
    }

    /// Open the gpsd stream and enter chase mode. Shared by the Chase tab's connect button,
    /// the launch-time autoconnect, and the toggle that turns autoconnect on. A daemon that is
    /// not there just logs; chase mode stays manual.
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    fn connect_gpsd(&mut self) {
        match crate::gps::spawn() {
            Some(rx) => {
                log::info!("gpsd: connected on localhost:2947, chase mode on");
                self.gps_rx = Some(rx);
                self.chase_mode = true;
            }
            None => log::warn!("gpsd: not reachable on localhost:2947"),
        }
    }

    /// Pull the next site's newest volume into the cache before the handoff needs it.
    ///
    /// Only ever one download ahead, and only once per site: this runs off a GPS fix, which on a
    /// moving car is every couple of seconds. Needs the chase log switched on — the prediction
    /// reads the breadcrumb track, and a single fix has no direction in it.
    fn warm_next_site(&mut self) {
        const EVERY: std::time::Duration = std::time::Duration::from_secs(120);
        if !self.settings.chase_log {
            return;
        }
        let current = self.views[self.active].site.clone();
        let Some(site) = crate::chase::next_site(
            &self.chase_track,
            current.as_deref(),
            crate::chase::LOOKAHEAD_MIN,
        ) else {
            return;
        };
        if self
            .warmed_site
            .as_ref()
            .is_some_and(|(s, at)| *s == site && at.elapsed() < EVERY)
        {
            return;
        }
        self.warmed_site = Some((site.clone(), Instant::now()));
        log::debug!("chase: warming {site} ahead of the handoff");
        self.spawner.spawn(crate::chase::warm(site));
    }

    /// Start the Google sign-in: bind a loopback port, send the browser to Google, and wait for
    /// the redirect to come back with a code.
    fn sync_sign_in(&mut self) {
        let id = self.settings.sync_client_id.trim().to_string();
        if id.is_empty() {
            self.sync_status = "Add your OAuth client id first (see docs/sync.md)".into();
            return;
        }
        match crate::cloud::start_login(&id) {
            Ok(pending) => {
                self.sync_status = match crate::platform::open_url(&pending.url) {
                    Ok(()) => "Finish in the browser window that just opened…".into(),
                    // No browser we could launch — the URL is on screen with a Copy button.
                    Err(e) => format!("Open the sign-in link below ({e})"),
                };
                self.sync_login = Some(pending);
            }
            Err(e) => self.sync_status = format!("Sign-in failed: {e}"),
        }
    }

    /// Forget the tokens (and the bookkeeping, so a later sign-in starts clean). The Drive copy
    /// is left alone — signing out of a laptop should not wipe the phone's settings.
    fn sync_sign_out(&mut self) {
        crate::cloud::Tokens::forget();
        self.sync_tokens = None;
        self.sync_login = None;
        self.sync_state = crate::cloud::SyncState::default();
        self.sync_state.save();
        self.sync_status = "Signed out".into();
    }

    /// How often a signed-in, sync-enabled app checks Drive on its own.
    const SYNC_SECS: u64 = 300;

    /// Replace the settings with the synced ones, keeping this machine's own fields, and persist.
    /// Returns the hash the sync bookkeeping should record.
    fn apply_synced(&mut self, body: &str) -> Result<u64, String> {
        let local = serde_json::to_value(&self.settings).map_err(|e| e.to_string())?;
        let merged = crate::cloud::merge_in(&local, body)?;
        let settings: crate::settings::Settings =
            serde_json::from_value(merged).map_err(|e| e.to_string())?;
        self.settings = settings;
        self.settings.save();
        let hash = crate::cloud::hash(&crate::cloud::shareable(
            &serde_json::to_value(&self.settings).map_err(|e| e.to_string())?,
        ));
        Ok(hash)
    }

    /// How often our own fix goes out on both transports. Fast enough to follow a chase vehicle,
    /// slow enough to be free on a metered connection.
    const SHARE_SECS: u64 = 10;

    /// Pull an HRRR point sounding at `(lon, lat)`, shown in the Skew-T window when it arrives.
    /// Fetch the NWS point forecast for a tapped spot. Results are cached per ~0.05° cell for
    /// 15 minutes — the grid only updates hourly, and re-tapping the same neighborhood shouldn't
    /// re-hit the API.
    fn fetch_point_forecast(&mut self, lon: f64, lat: f64) {
        let key = ((lat * 20.0).round() as i32, (lon * 20.0).round() as i32);
        self.forecast_at = Some((lon, lat));
        self.forecast_open = true;
        self.fetch_point_obs(key, lon, lat);
        self.fetch_model_series(lon, lat);
        self.fetch_plume(lon, lat);
        if let Some((when, f)) = self.forecast_cache.get(&key) {
            if when.elapsed().as_secs() < 900 {
                self.forecast_state = ui::forecast_window::State::Ready(Box::new(f.clone()));
                return;
            }
        }
        self.forecast_state = ui::forecast_window::State::Loading;
        let (tx, rx) = std::sync::mpsc::channel();
        self.forecast_rx = Some((key, rx));
        let http = self.http.clone();
        self.spawner.spawn(async move {
            let res = wxdata::forecast::fetch(&http, lat, lon)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
    }

    /// The GEFS mean and spread at the tapped point, keyed and cached like the meteogram beside it
    /// (same 0.05 degree cell, 15-minute TTL) plus the picker, so switching field or period is a
    /// fresh fetch rather than a stale hit.
    fn fetch_plume(&mut self, lon: f64, lat: f64) {
        let key = (
            (lat * 20.0).round() as i32,
            (lon * 20.0).round() as i32,
            self.plume_ui,
        );
        if let Some((when, points)) = self.plume_cache.get(&key) {
            if when.elapsed().as_secs() < 900 {
                self.plume_state = ui::forecast_window::PlumeState::Ready(points.clone());
                return;
            }
        }
        self.plume_state = ui::forecast_window::PlumeState::Loading;
        let (tx, rx) = std::sync::mpsc::channel();
        self.plume_rx = Some((key, rx));
        let http = self.http.clone();
        let picker = self.plume_ui;
        let hours = picker.period.plume_hours();
        self.spawner.spawn(async move {
            let res = wxdata::ensemble::fetch_gefs_plume(&http, picker.field, lon, lat, &hours)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
    }

    /// Current conditions for the forecast point, on the same cache cell and TTL as the forecast.
    /// A failure (offshore, no station, API down) simply sends nothing — the window drops the
    /// "Now" line rather than showing an error for a decoration.
    fn fetch_point_obs(&mut self, key: (i32, i32), lon: f64, lat: f64) {
        if let Some((when, ..)) = self.forecast_obs_cache.get(&key) {
            if when.elapsed().as_secs() < 900 {
                return;
            }
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.forecast_obs_rx = Some((key, rx));
        let http = self.http.clone();
        self.spawner.spawn(async move {
            match wxdata::obs::fetch_nearest(&http, lat, lon).await {
                Ok(s) => {
                    if let Some(o) = s.obs.first() {
                        let _ = tx.send((s.station_id, o.clone()));
                    }
                }
                Err(e) => log::debug!("point obs unavailable: {e}"),
            }
        });
    }

    /// Open the verification lab, prefilled from what's on screen: the office whose warnings are
    /// in view, and the archive day the timeline is parked on. Typing either again is a fallback,
    /// not the normal path.
    fn open_verify(&mut self) {
        if self.verify_window.wfo.is_empty() {
            // Archived warnings carry their issuing office in `area`; a live pane has none, and
            // the field stays blank rather than guessing.
            // The office whose warning actually covers the camera, not merely the first one in
            // the national archive fetch — otherwise a KTLX view opens scored against St. Louis.
            let (clon, clat) = {
                let c = self.views[self.active].camera.center;
                crate::render::mercator::world_to_lonlat(c.0, c.1)
            };
            self.verify_window.wfo = self
                .arch_warns
                .iter()
                .flat_map(|(_, feats)| feats.iter())
                .filter(|f| {
                    f.bbox().is_some_and(|(w, s, e, n)| {
                        clon >= w && clon <= e && clat >= s && clat <= n
                    })
                })
                .find_map(|f| f.alert.as_ref().map(|a| a.area.clone()))
                .unwrap_or_default();
        }
        if self.verify_window.day.is_empty() {
            self.verify_window.day = self.views[self.active]
                .timeline
                .date
                .format("%Y-%m-%d")
                .to_string();
        }
        self.verify_window.open = true;
        if self.verify_window.data.is_none() && !self.verify_window.wfo.is_empty() {
            self.fetch_verify();
        }
    }

    fn fetch_verify(&mut self) {
        let wfo = self.verify_window.wfo.trim().to_ascii_uppercase();
        let Ok(day) = chrono::NaiveDate::parse_from_str(self.verify_window.day.trim(), "%Y-%m-%d")
        else {
            self.verify_window.error = Some("Day must look like 2013-05-20".into());
            return;
        };
        if wfo.is_empty() {
            self.verify_window.error = Some("Enter a WFO, e.g. OUN".into());
            return;
        }
        let start = day.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc();
        let end = start + chrono::Duration::days(1);
        self.verify_window.busy = true;
        self.verify_window.error = None;
        let (tx, rx) = std::sync::mpsc::channel();
        self.verify_rx = Some(rx);
        let http = self.http.clone();
        self.spawner.spawn(async move {
            let res = wxdata::verify::fetch(&http, &wfo, start, end)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
    }

    /// Phase C1's gate-inspector inputs: every moment a user-defined product can reference, plus
    /// geometry, sampled at `(lon, lat)` on `tilt`. Binning is cached on `vol` (LRU) — sampling a
    /// moment the pane's own display hasn't already binned costs one bin, everything else is free.
    fn udp_gate_inputs(
        vol: &mut Volume,
        tilt: usize,
        lon: f64,
        lat: f64,
        antenna_altitude_m: Option<f64>,
        // (0°C height, −20°C height), metres above sea level — `self.freezing`, already filtered
        // to the gate's own site by the caller. `None` on the very first inspection of a site (or
        // a hail grid's own request) before the proactive fetch `inspect_gate` kicks off there —
        // see that fetch's own doc comment — has actually landed; a UDP formula referencing these
        // inputs just sees them as missing in the meantime, not the app fetching a second time.
        freezing: Option<(f64, f64)>,
    ) -> wxdata::udp::GateInputs {
        let mut out = wxdata::udp::GateInputs {
            freezing_level_m: freezing.map(|(h0, _)| h0 as f32),
            minus20c_height_m: freezing.map(|(_, hm20)| hm20 as f32),
            ..Default::default()
        };
        for m in [
            Moment::Reflectivity,
            Moment::Velocity,
            Moment::SpectrumWidth,
            Moment::DifferentialReflectivity,
            Moment::SpecificDifferentialPhase,
            Moment::CorrelationCoefficient,
        ] {
            let Ok(binned) = vol.binned(m, tilt, false) else {
                continue;
            };
            let Some(sample) = binned.sample_at(lon, lat) else {
                continue;
            };
            let elevation_deg = binned.elevation_deg;
            match m {
                Moment::Reflectivity => out.reflectivity = sample.value,
                Moment::Velocity => out.velocity = sample.value,
                Moment::SpectrumWidth => out.spectrum_width = sample.value,
                Moment::DifferentialReflectivity => out.differential_reflectivity = sample.value,
                Moment::SpecificDifferentialPhase => out.specific_diff_phase = sample.value,
                Moment::CorrelationCoefficient => out.correlation_coefficient = sample.value,
                _ => {}
            }
            // Geometry is the same point regardless of which moment answered first — grab it
            // once, from whichever moment happens to be present at this gate.
            if out.azimuth_deg.is_none() {
                out.azimuth_deg = Some(sample.azimuth_deg);
                out.range_km = Some(wxdata::xsection::ground_from_slant_km(
                    sample.range_km as f64,
                    elevation_deg as f64,
                ) as f32);
                out.elevation_deg = Some(elevation_deg);
                let height_m =
                    wxdata::xsection::beam_height_km(sample.range_km as f64, elevation_deg as f64)
                        * 1000.0;
                out.beam_height_m = Some(height_m as f32);
                out.beam_altitude_m =
                    antenna_altitude_m.map(|altitude| (altitude + height_m) as f32);
            }
        }
        out
    }

    /// Every tilt's own [`Self::udp_gate_inputs`] at the same `(lon, lat)`, low to high — the
    /// "column" a vertical/layer user-defined-product function (ROADMAP_NEW C1) reduces over.
    /// Skips a tilt this point falls outside of rather than padding the column with an empty
    /// entry that has no height to sort or filter by.
    fn udp_column_inputs(
        vol: &mut Volume,
        lon: f64,
        lat: f64,
        antenna_altitude_m: Option<f64>,
        freezing: Option<(f64, f64)>,
    ) -> Vec<wxdata::udp::GateInputs> {
        (0..vol.elevations.len())
            .map(|tilt| Self::udp_gate_inputs(vol, tilt, lon, lat, antenna_altitude_m, freezing))
            .filter(|g| g.beam_height_m.is_some())
            .collect()
    }

    /// Phase B4's gate inspector: everything about the point at `(lon, lat)` on the active pane's
    /// currently displayed moment, or `None` when there is no volume here, this moment has no
    /// data on this tilt, or the point falls outside the sweep's coverage (past its last gate).
    /// Synchronous and local — unlike `fetch_sounding`/`query_climatology`, nothing here reaches
    /// the network: the volume this samples is already decoded and on screen.
    ///
    /// `tilt_override`, when given, samples that tilt index instead of the pane's own selected
    /// `v.tilt` — the 3D map-pitch view can show several tilts stacked in one frame, so a click
    /// there names a specific tilt via [`crate::render3d::pick_observed_tilt`] rather than always
    /// meaning whichever one the 2D tilt picker happens to have selected.
    fn inspect_gate(
        &mut self,
        ctx: &egui::Context,
        idx: usize,
        lon: f64,
        lat: f64,
        tilt_override: Option<usize>,
    ) -> Option<ui::gate_inspector::GateInspectorPopup> {
        let site = self.views[idx].site.clone();
        // ROADMAP_NEW C1's own named follow-up: fetch freezing levels proactively whenever a gate
        // is actually being inspected (this function's only two callers — a click with the Gate
        // Inspector tool, and the linked cursor-probe table), not only when a hail grid happens to
        // be on. Read before taking `v` below: `fetch_freezing_levels` takes `&mut self` in full,
        // which would conflict with `v`'s already-borrowed `&mut self.views[idx]` if called after.
        // `fetch_freezing_levels` self-throttles to 900s and no-ops without a site, so calling it
        // on every inspection is cheap, not a fetch storm.
        //
        // Only meaningful for the gate's own site and time — `self.freezing` is a single
        // most-recent cache, so `freezing_for` filters out another site's reading, or a live one
        // standing in for an archived volume's. Read before `v` for the same borrow reason.
        let freezing = self.freezing_for(idx);
        if freezing.is_none() {
            self.fetch_freezing_levels(ctx, idx);
        }
        let v = &mut self.views[idx];
        let antenna_altitude_m = site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
            .map(|site| site.elevation_meters as f64 + wxdata::towers::tower_m(site.id));
        let moment = v.moment;
        let tilt = tilt_override.unwrap_or(v.tilt);
        let (scan, vcp, elevation_deg) = {
            let vol = v.volume.as_ref()?;
            (
                Arc::clone(&vol.scan),
                vol.vcp.clone(),
                *vol.elevations.get(tilt)?,
            )
        };
        let vol = v.volume.as_mut()?;
        // Only velocity has a dealiased counterpart worth sampling; asking `binned` for any
        // other moment's "dealiased" sweep would just rebuild the same raw one under a
        // different cache key.
        let dealiased = if moment == Moment::Velocity {
            vol.binned(moment, tilt, true).ok().cloned()
        } else {
            None
        };
        let raw = vol.binned(moment, tilt, false).ok()?.clone();
        let inspection = raw.inspect(lon, lat, dealiased.as_ref())?;
        let time_range = level2::sweep_time_range(&scan, elevation_deg, moment);
        let gate_inputs = Self::udp_gate_inputs(vol, tilt, lon, lat, antenna_altitude_m, freezing);
        let column_inputs = Self::udp_column_inputs(vol, lon, lat, antenna_altitude_m, freezing);
        Some(ui::gate_inspector::GateInspectorPopup {
            site,
            vcp,
            moment,
            time_range,
            inspection,
            gate_inputs,
            column_inputs,
            // Filled on a click only (below): the cursor-probe table calls this on every hover
            // and keeps just the value, and the series reads every volume the pane holds.
            series: Vec::new(),
        })
    }

    /// The visible gridded layer on top of this pane, matching the draw/legend order exactly.
    fn probe_field(
        &self,
        idx: usize,
        lon: f64,
        lat: f64,
        vp: (f32, f32),
    ) -> Option<crate::render::FieldLayer> {
        use crate::render::FieldLayer as FL;
        let view = &self.views[idx];
        let swipe_layer = (view.swipe_compare
            && view.fields_on.contains(&FL::CompareA)
            && view.fields_on.contains(&FL::CompareB))
        .then(|| {
            let world = crate::render::mercator::lonlat_to_world(lon, lat);
            let screen = view.camera.world_to_screen(world, vp);
            if Self::swipe_showing_b(view.swipe_fraction, screen.0, vp.0) {
                FL::CompareB
            } else {
                FL::CompareA
            }
        });
        crate::render::FieldLayer::DRAW_ORDER
            .iter()
            .rev()
            .copied()
            .find(|layer| {
                view.fields_on.contains(layer)
                    && swipe_layer.is_none_or(|selected| {
                        !matches!(layer, FL::CompareA | FL::CompareB) || *layer == selected
                    })
                    && crate::fielddiff::layer_ready(*layer, self.diff_valid, self.compare_valid)
                    && match layer {
                        FL::ModelDiff => self.diff_grid.is_some(),
                        FL::Ensemble => self.ensemble_grid.is_some(),
                        FL::CompareA | FL::CompareB => self.compare_grid.is_some(),
                        _ => self
                            .fields
                            .get(layer)
                            .is_some_and(|state| state.grid.is_some()),
                    }
            })
    }

    fn probe_field_source(
        &self,
        layer: crate::render::FieldLayer,
        state: Option<&FieldState>,
    ) -> String {
        if let Some(source) = state
            .and_then(|field| field.stamp.as_ref())
            .map(|stamp| stamp.source_id.clone())
        {
            return source;
        }
        use crate::render::FieldLayer as FL;
        match layer {
            FL::Hrrr => self.refl_source_label(),
            FL::Mosaic => "Multi-radar mosaic".into(),
            FL::CompositeLocal
            | FL::VilLocal
            | FL::VilDensity
            | FL::EtopLocal
            | FL::HailMehs
            | FL::HailPosh => "Local radar".into(),
            FL::SnowBands => "Derived MRMS".into(),
            FL::SnowAnalysis => "NOAA NOHRSC".into(),
            FL::Vil | FL::EchoTops | FL::Hca => "NEXRAD Level III".into(),
            FL::NdfdTemp2m | FL::NdfdWind10m | FL::NdfdGust10m | FL::NdfdSnow => "NWS NDFD".into(),
            FL::RtmaTemp2m
            | FL::RtmaDewpoint2m
            | FL::RtmaWind10m
            | FL::RtmaGust10m
            | FL::RtmaVisibility
            | FL::RtmaCeiling
            | FL::RtmaMslp
            | FL::RtmaPrecip1h => "RTMA analysis".into(),
            _ => layer.descriptor().map_or_else(
                || "Gridded field".into(),
                |descriptor| descriptor.source.display_name().into(),
            ),
        }
    }

    fn probe_field_product(layer: crate::render::FieldLayer) -> String {
        use crate::render::FieldLayer as FL;
        let special = match layer {
            FL::Mosaic => Some("Multi-radar reflectivity"),
            FL::CompositeLocal => Some("Local composite reflectivity"),
            _ => None,
        };
        if let Some(label) = special {
            return label.into();
        }
        crate::render::field_ramps::ramp_for(layer).map_or_else(
            || {
                layer
                    .descriptor()
                    .map_or_else(|| layer.slug().into(), |descriptor| descriptor.name.into())
            },
            |ramp| ramp.label.into(),
        )
    }

    /// Format one raw grid sample in the same display units/category vocabulary as its legend.
    fn probe_field_value(&self, layer: crate::render::FieldLayer, raw: f32) -> Option<String> {
        format_probe_field_value(layer, raw, self.settings.temp_unit)
    }

    fn grid_probe_row(
        &self,
        idx: usize,
        layer: crate::render::FieldLayer,
        lon: f64,
        lat: f64,
    ) -> ui::cursor_probe::ProbeRow {
        use crate::render::FieldLayer as FL;
        if layer == FL::ModelDiff {
            let (a, b) = self.diff_field.pair();
            let (_, deadband) = self.diff_field.range();
            let value = self
                .diff_display_grid()
                .and_then(|grid| grid.sample_bilinear(lon, lat))
                .filter(|value| value.is_finite())
                .map(|value| self.diff_display_value(value))
                .map(|value| {
                    format_diff_readout(self.diff_mode, value, deadband, self.diff_field.units())
                });
            return ui::cursor_probe::ProbeRow {
                pane: idx,
                source: self.diff_mode.expression(a, b),
                product: if self.diff_mode == crate::fielddiff::DiffMode::Disagreement {
                    format!("{} disagreement mask", self.diff_field.label())
                } else {
                    format!("{} difference", self.diff_field.label())
                },
                time: self.diff_valid.map(|times| times.valid),
                value,
                folded: false,
            };
        }
        if layer == FL::Ensemble {
            let stamp = self
                .fields
                .get(&layer)
                .and_then(|state| state.stamp.as_ref());
            return ui::cursor_probe::ProbeRow {
                pane: idx,
                source: stamp.map_or_else(|| "GEFS".into(), |stamp| stamp.source_id.clone()),
                product: self.ensemble.title(self.settings.temp_unit),
                time: stamp.map(|stamp| stamp.valid_time),
                value: self
                    .ensemble_grid
                    .as_ref()
                    .and_then(|grid| grid.sample_bilinear(lon, lat))
                    .and_then(|raw| {
                        crate::ensemble_layer::format_value(
                            &self.ensemble,
                            raw,
                            self.settings.temp_unit,
                        )
                    }),
                folded: false,
            };
        }
        if matches!(layer, FL::CompareA | FL::CompareB) {
            let side_b = layer == FL::CompareB;
            let (a_name, b_name) = self.diff_field.pair();
            let grid = self
                .compare_grid
                .as_ref()
                .map(|(a, b)| if side_b { b } else { a });
            let source_layer = self.diff_field.source_layer();
            return ui::cursor_probe::ProbeRow {
                pane: idx,
                source: if side_b { b_name } else { a_name }.into(),
                product: self.diff_field.label().into(),
                time: self.compare_valid.map(|times| times.valid),
                value: grid
                    .and_then(|grid| grid.sample_bilinear(lon, lat))
                    .and_then(|raw| self.probe_field_value(source_layer, raw)),
                folded: false,
            };
        }

        let state = self.fields.get(&layer);
        let grid = state.and_then(|field| field.grid.as_ref());
        ui::cursor_probe::ProbeRow {
            pane: idx,
            source: self.probe_field_source(layer, state),
            product: Self::probe_field_product(layer),
            time: state
                .and_then(|field| field.stamp.as_ref().map(|stamp| stamp.valid_time))
                .or_else(|| grid.map(|grid| grid.time)),
            value: grid
                .and_then(|grid| grid.sample_bilinear(lon, lat))
                .and_then(|raw| self.probe_field_value(layer, raw)),
            folded: false,
        }
    }

    /// One row of the ROADMAP_NEW J3 cursor-probe table. The top visible grid wins; otherwise
    /// sample this pane's radar moment through the exact Interrogate-tool path. A missing sample
    /// leaves a visible row with `—` rather than silently dropping the pane.
    fn probe_row(
        &mut self,
        ctx: &egui::Context,
        idx: usize,
        lon: f64,
        lat: f64,
        vp: (f32, f32),
    ) -> ui::cursor_probe::ProbeRow {
        if let Some(layer) = self.probe_field(idx, lon, lat, vp) {
            return self.grid_probe_row(idx, layer, lon, lat);
        }
        let moment = self.views[idx].moment;
        match self.inspect_gate(ctx, idx, lon, lat, None) {
            Some(popup) => ui::cursor_probe::ProbeRow {
                pane: idx,
                source: popup.site.unwrap_or_else(|| "—".into()),
                product: popup.moment.short_name().into(),
                time: popup.time_range.map(|(_, end)| end),
                value: popup
                    .inspection
                    .sample
                    .value
                    .map(|value| format!("{value:.1} {}", popup.moment.units())),
                folded: popup.inspection.sample.folded,
            },
            None => ui::cursor_probe::ProbeRow {
                pane: idx,
                source: self.views[idx].site.clone().unwrap_or_else(|| "—".into()),
                product: moment.short_name().into(),
                time: None,
                value: None,
                folded: false,
            },
        }
    }

    /// Select a storm cell. Outside the workstation that opens its attributes window, as before;
    /// in the workstation the Inspector's storm section shows it (brought forward if it was hidden
    /// or behind another tab), and the full window waits for its Details button.
    pub(crate) fn select_storm(&mut self, c: Cell) {
        self.select_storm_from(c, true);
    }

    /// [`Self::select_storm`], with the choice of leaving the dock alone (`show: false`): a pick
    /// in the Storms table should not switch the table's own dock over to the Inspector while the
    /// analyst is stepping down the list — the ring on the map already says which one it is.
    pub(crate) fn select_storm_from(&mut self, c: Cell, show: bool) {
        self.cell_popup = Some(c);
        if self.workstation_chrome() {
            if self.cell_details {
                // The Cell window is open: the storm joins it (`sync_open_cells`) to be read
                // beside the others, rather than closing it for the Inspector.
                if show && !self.dock.shown(chrome::DockWin::Cell) {
                    self.dock.bring_forward(chrome::DockWin::Cell);
                }
            } else if show && !self.dock.shown(chrome::DockWin::Inspector) {
                self.dock.toggle(chrome::DockWin::Inspector);
            }
        } else {
            self.cell_details = true;
        }
    }

    /// The grid the difference layer is drawing: the percent grid in the percent view, else the
    /// signed difference. What the upload and every readout sample, so they cannot disagree.
    fn diff_display_grid(&self) -> Option<&wxdata::mrms::MrmsField> {
        if self.diff_mode == crate::fielddiff::DiffMode::Percent {
            self.diff_pct.as_ref()
        } else {
            self.diff_grid.as_ref()
        }
    }

    /// A sample of [`Self::diff_display_grid`] in display terms: a percent as is, a difference
    /// in the field's display units.
    fn diff_display_value(&self, v: f32) -> f32 {
        if self.diff_mode == crate::fielddiff::DiffMode::Percent {
            v
        } else {
            v * self.diff_field.input_scale()
        }
    }

    /// The GPU upload for the current difference view, built from `signed` (the difference) or,
    /// in the percent view, the percent grid — blank when there is none.
    fn diff_upload(&self, signed: &wxdata::mrms::MrmsField) -> crate::render::MrmsUpload {
        let grid = if self.diff_mode == crate::fielddiff::DiffMode::Percent {
            self.diff_pct.as_ref().unwrap_or(signed)
        } else {
            signed
        };
        model_diff_upload(grid, self.diff_field, self.diff_mode)
    }

    /// The selected storm, current: `cell_popup` is a copy taken at the click, so the newest
    /// SCIT update of the same cell (by id) replaces it, keeping its position and attributes live.
    /// `None` when nothing is selected; the click-time copy when the cell has left the product.
    pub(crate) fn selected_storm(&self) -> Option<Cell> {
        let picked = self.cell_popup.as_ref()?;
        Some(
            self.active_storm_cells()
                .iter()
                .find(|c| !picked.id.is_empty() && c.id == picked.id)
                .cloned()
                .unwrap_or_else(|| picked.clone()),
        )
    }

    /// ROADMAP_NEW J2: mark the selected storm — a ring and its id — in the active pane, and in
    /// every pane while `link_storm` is on. Geographic, so a pane on another radar or zoom marks
    /// the same storm.
    fn paint_selected_storm(&mut self, ui: &egui::Ui, rects: &[egui::Rect], solo: bool) {
        let Some(cell) = self.selected_storm() else {
            return;
        };
        let world = crate::render::mercator::lonlat_to_world(cell.lon, cell.lat);
        let accent = crate::theme::accent(self.settings.theme);
        for (idx, rect) in rects.iter().enumerate() {
            let linked = self.link_storm && !solo;
            if idx != self.active && !linked {
                continue;
            }
            let (sx, sy) = self.views[idx]
                .camera
                .world_to_screen(world, (rect.width(), rect.height()));
            let pos = egui::pos2(rect.left() + sx, rect.top() + sy);
            if !rect.contains(pos) {
                continue;
            }
            let painter = ui.painter_at(*rect);
            painter.circle_stroke(pos, 17.0, egui::Stroke::new(3.0, egui::Color32::BLACK));
            painter.circle_stroke(pos, 17.0, egui::Stroke::new(2.0, accent));
            painter.circle_stroke(
                pos,
                22.0,
                egui::Stroke::new(1.0, accent.gamma_multiply(0.5)),
            );
            if !cell.id.is_empty() {
                let at = pos + egui::vec2(20.0, -20.0);
                let galley = painter.layout_no_wrap(
                    cell.id.clone(),
                    egui::FontId::monospace(12.0),
                    egui::Color32::WHITE,
                );
                let r = egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(4.0, 2.0));
                painter.rect_filled(r, 3.0, accent.gamma_multiply(0.85));
                painter.galley(at, galley, egui::Color32::WHITE);
            }
        }
    }

    /// ROADMAP_NEW J2: while `link_storm` is on, recenter every pane on the selected storm when
    /// the selection changes or the storm has moved more than a kilometre since — once per change,
    /// so each pane's own pan and zoom hold in between.
    fn follow_linked_storm(&mut self) {
        let cell = if self.link_storm {
            self.selected_storm()
        } else {
            None
        };
        let Some(cell) = cell else {
            self.storm_link_at = None;
            return;
        };
        let moved = self.storm_link_at.as_ref().is_none_or(|(id, lon, lat)| {
            *id != cell.id || crate::geo::great_circle([*lon, *lat], [cell.lon, cell.lat]).0 > 1.0
        });
        if !moved {
            return;
        }
        let center = crate::render::mercator::lonlat_to_world(cell.lon, cell.lat);
        for v in &mut self.views {
            v.camera.center = center;
        }
        self.storm_link_at = Some((cell.id.clone(), cell.lon, cell.lat));
    }

    /// ROADMAP_NEW J3: while `link_cursor` is on and some pane is hovered, draw a matching
    /// crosshair on every pane at the same geographic point and show the compact probe table.
    /// The table samples each pane's top visible gridded layer or, when there is none, its radar
    /// moment; every row therefore describes what that pane is actually showing at the crosshair.
    fn paint_linked_cursor(&mut self, ui: &egui::Ui, rects: &[egui::Rect], solo: bool) {
        if !self.link_cursor || solo || rects.len() < 2 {
            return;
        }
        let Some((lon, lat)) = self.linked_probe else {
            return;
        };
        let world = crate::render::mercator::lonlat_to_world(lon, lat);
        let color = egui::Color32::from_rgb(255, 214, 92);
        let mut rows = Vec::with_capacity(rects.len());
        for (idx, rect) in rects.iter().enumerate() {
            let vp = (rect.width(), rect.height());
            let screen = self.views[idx].camera.world_to_screen(world, vp);
            let pos = egui::pos2(rect.left() + screen.0, rect.top() + screen.1);
            if rect.contains(pos) {
                let painter = ui.painter_at(*rect);
                painter.line_segment(
                    [pos - egui::vec2(9.0, 0.0), pos + egui::vec2(9.0, 0.0)],
                    egui::Stroke::new(1.5, color),
                );
                painter.line_segment(
                    [pos - egui::vec2(0.0, 9.0), pos + egui::vec2(0.0, 9.0)],
                    egui::Stroke::new(1.5, color),
                );
                painter.circle_stroke(pos, 4.0, egui::Stroke::new(1.5, color));
            }
            rows.push(self.probe_row(ui.ctx(), idx, lon, lat, vp));
        }
        ui::cursor_probe::show(ui.ctx(), &rows, self.active_tz());
    }

    /// Ask the configured routing server for routes through the route window's waypoints.
    fn fetch_route(&mut self) {
        let w = &mut self.route_window;
        if w.waypoints.len() < 2 {
            w.routes.clear();
            w.generation += 1;
            w.busy = false;
            self.route_rx = None;
            return;
        }
        w.busy = true;
        w.error = None;
        let (engine, url) = (self.settings.route_engine, self.settings.route_url.clone());
        let waypoints = w.waypoints.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.route_rx = Some(rx);
        let http = self.http.clone();
        self.spawner.spawn(async move {
            let res = wxdata::route::fetch(&http, engine, &url, &waypoints)
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(res);
        });
    }

    /// The route window's per-frame work: take a finished fetch, work out progress and exposure
    /// along the chosen route, draw the window, and act on what it asks.
    fn route_frame(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.route_rx {
            if let Ok(res) = rx.try_recv() {
                self.route_rx = None;
                let w = &mut self.route_window;
                w.busy = false;
                match res {
                    Ok(routes) => {
                        w.routes = routes;
                        w.selected = 0;
                    }
                    Err(e) => {
                        w.routes.clear();
                        w.error = Some(e);
                    }
                }
                w.generation += 1;
            }
        }
        if !self.route_window.open {
            return;
        }
        let route = self
            .route_window
            .routes
            .get(self.route_window.selected)
            .cloned();
        // Progress from the chase position, when it is on (or within 2 km of) the route.
        let along = route
            .as_ref()
            .zip(self.chase_pos)
            .and_then(|(r, (lon, lat))| {
                let (m, off) = wxdata::route::progress(&r.coords, [lon, lat])?;
                (off < 2_000.0).then_some(m)
            });
        let remaining = route.as_ref().zip(along).map(|(r, m)| {
            let total = wxdata::route::cumulative_m(&r.coords)
                .last()
                .copied()
                .unwrap_or(1.0);
            let left = (total - m).max(0.0);
            (left / 1000.0, r.duration_s * left / total.max(1.0))
        });
        let key = (
            self.route_window.generation,
            self.overlay_gen,
            (along.unwrap_or(0.0) / 100.0) as i64,
            self.active_storm_cells().len(),
            // Lightning and the fields refresh by the minute.
            Utc::now().timestamp() / 60,
        );
        if self.route_exposure.0 != key {
            // Warnings and watches in effect now: every overlay polygon that carries an alert.
            let alerts: Vec<(&str, Vec<Vec<[f64; 2]>>)> = self
                .overlays
                .iter()
                .filter_map(|f| Some((f.alert.as_ref()?.event.as_str(), f.rings.clone())))
                .collect();
            let polys: Vec<Vec<Vec<[f64; 2]>>> = alerts.iter().map(|(_, r)| r.clone()).collect();
            let hits = route
                .as_ref()
                .map(|r| wxdata::route::exposure(r, &polys, along.unwrap_or(0.0)))
                .unwrap_or_default();
            // One line per kind of alert: the nearest of each.
            let mut lines: Vec<(String, f64, f64, bool)> = Vec::new();
            for h in hits {
                let what = alerts[h.polygon].0.to_string();
                if !lines.iter().any(|(w, ..)| *w == what) {
                    lines.push((what, h.at_m / 1000.0, h.at_s, true));
                }
            }
            if let Some(r) = route.as_ref() {
                let from = along.unwrap_or(0.0);
                // Heavy echo from a displayed MRMS reflectivity grid that matches the view's time.
                use crate::render::FieldLayer as FL;
                let grid = [FL::Mosaic, FL::ReflLowestAlt].into_iter().find_map(|l| {
                    self.mrms_ready(l)
                        .then(|| self.fields.get(&l)?.grid.as_ref())
                        .flatten()
                });
                if let Some(g) = grid {
                    if let Some((m, s)) = wxdata::route::first_along(r, from, |p| {
                        wxdata::route::grid_value(g, p).is_some_and(|v| v >= 50.0)
                    }) {
                        lines.push(("Heavy echo (50+ dBZ, MRMS)".into(), m / 1000.0, s, false));
                    }
                }
                // Hail, heavy rain and rare rainfall from the MRMS layers that are displayed.
                for (layer, at_least, what) in [
                    (FL::Mesh, 25.4, "Hail 1 in or larger (MESH)"),
                    (
                        FL::Qpe1h,
                        50.8,
                        "2 in or more of rain in the last hour (MRMS)",
                    ),
                    (
                        FL::FlashFlood,
                        10.0,
                        "30-min rainfall rarer than 1-in-10-year (MRMS FLASH)",
                    ),
                ] {
                    let Some(g) = self
                        .mrms_ready(layer)
                        .then(|| self.fields.get(&layer)?.grid.as_ref())
                        .flatten()
                    else {
                        continue;
                    };
                    if let Some((m, sec)) = wxdata::route::first_along(r, from, |p| {
                        wxdata::route::grid_value(g, p).is_some_and(|v| v >= at_least)
                    }) {
                        lines.push((what.into(), m / 1000.0, sec, false));
                    }
                }
                // Lightning within 8 km of the road in the last 15 minutes (live only).
                if self.view_target_time().is_none() {
                    let cutoff = Utc::now() - chrono::Duration::minutes(15);
                    let (mut w, mut s, mut e, mut n) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                    for p in &r.coords {
                        (w, s, e, n) = (w.min(p[0]), s.min(p[1]), e.max(p[0]), n.max(p[1]));
                    }
                    let near: Vec<[f64; 2]> = self
                        .glm
                        .lock()
                        .map(|f| {
                            f.flashes()
                                .iter()
                                .filter(|fl| {
                                    fl.time >= cutoff
                                        && (w - 0.1..=e + 0.1).contains(&fl.lon)
                                        && (s - 0.1..=n + 0.1).contains(&fl.lat)
                                })
                                .map(|fl| [fl.lon, fl.lat])
                                .collect()
                        })
                        .unwrap_or_default();
                    if !near.is_empty() {
                        if let Some((m, sec)) = wxdata::route::first_along(r, from, |p| {
                            near.iter()
                                .any(|f| wxdata::route::haversine_m(p, *f) < 8_000.0)
                        }) {
                            lines.push((
                                "Lightning within 5 mi in the last 15 min".into(),
                                m / 1000.0,
                                sec,
                                false,
                            ));
                        }
                    }
                }
            }
            lines.sort_by(|a, b| a.1.total_cmp(&b.1));
            // Tracked storms with a motion, within 150 km of the route (L4).
            let metric = self.metric();
            let mut storms: Vec<(f64, String)> = Vec::new();
            if let Some(r) = route.as_ref() {
                let from = along.unwrap_or(0.0);
                for c in self.active_storm_cells() {
                    let (Some(deg), Some(kt)) = (c.mvt_deg, c.mvt_kt) else {
                        continue;
                    };
                    let near = wxdata::route::progress(&r.coords, [c.lon, c.lat])
                        .is_some_and(|(_, off)| off < 150_000.0);
                    if !near {
                        continue;
                    }
                    let Some(i) = wxdata::route::intercept(
                        r,
                        from,
                        [c.lon, c.lat],
                        deg as f64,
                        kt as f64,
                        2.0 * 3600.0,
                    ) else {
                        continue;
                    };
                    let name = match c.max_dbz {
                        Some(z) => format!("{} ({z:.0} dBZ)", c.title),
                        None => c.title.clone(),
                    };
                    let mut line = format!(
                        "{name}: closest {} in {} min, to your {}",
                        crate::geo::fmt_distance(i.closest_m / 1000.0, metric, 0),
                        (i.closest_s / 60.0).round(),
                        compass8(i.closest_bearing_deg)
                    );
                    if let Some((ahead, storm_s, you_s)) = i.crossing {
                        line.push_str(&format!(
                            "; crosses the route {} ahead: storm in {} min, you in {} min",
                            crate::geo::fmt_distance(ahead / 1000.0, metric, 0),
                            (storm_s / 60.0).round(),
                            (you_s / 60.0).round()
                        ));
                    }
                    storms.push((i.closest_m, line));
                }
            }
            storms.sort_by(|a, b| a.0.total_cmp(&b.0));
            let intercepts = storms.into_iter().take(6).map(|(_, l)| l).collect();
            self.route_exposure = (key, lines, intercepts);
        }
        let readout = ui::route_window::RouteReadout {
            metric: self.metric(),
            have_position: self.chase_pos.is_some(),
            remaining,
            exposure: &self.route_exposure.1,
            intercepts: &self.route_exposure.2,
        };
        let before = (self.settings.route_engine, self.settings.route_url.clone());
        let action = self.route_window.show(
            ctx,
            &mut self.settings.route_engine,
            &mut self.settings.route_url,
            &readout,
            &mut self.drawer,
        );
        match action {
            ui::route_window::RouteAction::StartHere => {
                if let Some((lon, lat)) = self.chase_pos {
                    self.route_window.waypoints.insert(0, [lon, lat]);
                    self.fetch_route();
                }
            }
            ui::route_window::RouteAction::Fetch => self.fetch_route(),
            ui::route_window::RouteAction::None => {
                if before != (self.settings.route_engine, self.settings.route_url.clone()) {
                    self.settings.save();
                }
            }
        }
    }

    /// The observed ascent to draw beside the model profile: the nearest radiosonde station, at
    /// the synoptic time before whatever instant the active pane is showing (so an archive scrub
    /// gets that day's sounding, not today's).
    fn fetch_raob(&mut self, lon: f64, lat: f64) {
        self.sounding_window.observed = None;
        self.sounding_window.observed_error = None;
        self.sounding_window.observed_station.clear();
        self.raob_rx = None;
        let Some(station) = wxdata::raob::nearest_station(lon, lat) else {
            return;
        };
        let when = self.views[self.active]
            .timeline
            .current()
            .and_then(|id| id.date_time())
            .unwrap_or_else(chrono::Utc::now);
        self.sounding_window.observed_station = format!(
            "{} ({}) {}",
            station.name,
            station.id,
            wxdata::raob::synoptic_before(when).format("%d %b %HZ")
        );
        let (tx, rx) = std::sync::mpsc::channel();
        self.raob_rx = Some(rx);
        let http = self.http.clone();
        let cache = crate::paths::cache_dir();
        self.spawner.spawn(async move {
            let res = wxdata::raob::fetch(&http, station, when, cache)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
    }

    /// Optical-flow nowcast: advect every strong reflectivity gate of the active pane forward by
    /// the mean SCIT storm motion to the configured lead time. Returns advected `(lon, lat, color)`
    /// points for the painter. Coarse (subsampled gates) — a first-order extrapolation, not a model.
    /// Cached wrapper: these three detectors each bin (and clone) a full sweep and then walk it,
    /// but their inputs only change when a new volume arrives — running them every frame while
    /// their layer was toggled on burned that cost 4-60 times a second for an identical answer.
    fn compute_nowcast(&mut self, idx: usize) -> Vec<(f64, f64, egui::Color32)> {
        let key = (
            self.volume_key(idx),
            self.views[idx].tilt,
            self.filters.nowcast_lead_min,
            self.scit_mean_motion()
                .map(|(d, k)| (d.to_bits(), k.to_bits())),
            self.palettes.gen,
        );
        if let Some((k, v)) = &self.nowcast_cache {
            if *k == key {
                return v.clone();
            }
        }
        let out = self.compute_nowcast_uncached(idx);
        self.nowcast_cache = Some((key, out.clone()));
        out
    }

    /// The volume identity a detector's result depends on: pane, volume name, and how many sweeps
    /// have merged into it (a live volume keeps the same name as it fills out).
    fn volume_key(&self, idx: usize) -> (usize, String, usize) {
        let v = self.views[idx].volume.as_ref();
        (
            idx,
            v.map(|v| v.name.clone()).unwrap_or_default(),
            // A light loop frame is a different volume to every detector cache: it has only the
            // displayed tilt, and what it finds must not stand in for the full one on pause.
            v.map(|v| v.scan.sweeps().len() + if v.light { 1 << 20 } else { 0 })
                .unwrap_or(0),
        )
    }

    /// [`Self::volume_key`] plus a fingerprint of the detector thresholds, for the caches whose
    /// answer changes when the user moves a slider.
    fn tuned_key(&self, idx: usize) -> TunedKey {
        use std::hash::{Hash, Hasher};
        let d = &self.settings.detectors;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        d.tbss_core_dbz.to_bits().hash(&mut h);
        d.zdr_min_db.to_bits().hash(&mut h);
        d.zdr_min_depth_km.to_bits().hash(&mut h);
        (self.volume_key(idx), h.finish())
    }

    fn compute_nowcast_uncached(&mut self, idx: usize) -> Vec<(f64, f64, egui::Color32)> {
        let Some((dir, kt)) = self.scit_mean_motion() else {
            return Vec::new();
        };
        if kt <= 1.0 {
            return Vec::new();
        }
        let lead_km = kt as f64 * 1.852 * (self.filters.nowcast_lead_min as f64 / 60.0);
        let tilt = self.views[idx].tilt;
        let sweep = match self.views[idx]
            .volume
            .as_mut()
            .and_then(|v| v.binned(Moment::Reflectivity, tilt, false).ok())
        {
            Some(s) => s.clone(),
            None => return Vec::new(),
        };
        // Confidence in a pure advection falls off with lead: it moves the echo that exists and
        // cannot grow, decay or turn it. Fading the points says so without a disclaimer nobody
        // reads.
        let alpha = (150.0 * nowcast_confidence(self.filters.nowcast_lead_min)) as u8;
        let table = self.palettes.table(Moment::Reflectivity);
        let span = (sweep.value_max - sweep.value_min).max(1e-3);
        let radar = [sweep.radar_lon as f64, sweep.radar_lat as f64];
        let mut out = Vec::new();
        for az in (0..sweep.az_bins).step_by(4) {
            let az_deg = az as f64 * 360.0 / sweep.az_bins as f64;
            for gate in (0..sweep.gate_count).step_by(6) {
                let vidx = sweep.data[az * sweep.gate_count + gate];
                if vidx < 2 {
                    continue;
                }
                let dbz = sweep.value_min + (vidx as f32 - 2.0) / 253.0 * span;
                if dbz < 30.0 {
                    continue; // only advect meaningful echo
                }
                let range_km = (sweep.first_gate_km + gate as f32 * sweep.gate_interval_km) as f64;
                let gate_ll = crate::geo::destination_point(radar, az_deg, range_km);
                let adv = crate::geo::destination_point(gate_ll, dir as f64, lead_km);
                let c = table.sample(dbz).unwrap_or([120, 120, 120, 255]);
                out.push((
                    adv[0],
                    adv[1],
                    egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], alpha),
                ));
            }
        }
        out
    }

    /// The real (expensive) per-tilt gate scan behind [`Self::couplets_raw`]: bin the dealiased
    /// velocity sweep and flag gate-to-gate couplets, with no corroboration and no alerting.
    /// Returns the hits, the radar's position, and how many tilts were scanned; `None` if there
    /// is no velocity data. See [`Self::compute_couplets`], which layers both on afterward.
    fn detect_couplets(
        &mut self,
        idx: usize,
    ) -> Option<(Vec<wxdata::rotation::CoupletHit>, (f32, f32), usize)> {
        // The lowest few tilts, not just the lowest one: the classic operational TVS criterion is
        // vertical continuity, which a single sweep cannot offer at all (see
        // `rotation::detect_volume`). Dealiased, so folded gates don't fake huge shear. Capped at
        // 4 tilts for the same reason `compute_tds_uncached` caps at 4 — bounded cost, and low-
        // level rotation is what a tornadic circulation actually looks like.
        const TILTS: usize = 4;
        let vol = self.views[idx].volume.as_mut()?;
        let vel_tilts = vol.velocity_tilts_dealiased();
        let z_tilts = vol.moment_tilts(Moment::Reflectivity);
        // Paired with reflectivity so `detect_volume` can require real echo behind the shear
        // (see the module doc comment) — the same (moment, moment) zip `compute_tds_uncached`
        // uses for its own (z, cc) pairing.
        let pairs: Vec<_> = vel_tilts.into_iter().zip(z_tilts).take(TILTS).collect();
        let (first, _) = pairs.first()?;
        let (radar_lon, radar_lat) = (first.radar_lon, first.radar_lat);
        // 25 m/s gate-to-gate is the legacy weak-TVS criterion; 20 dBZ is a generous echo floor
        // (real rotation happens at a storm's weaker flank, not just its core); 15-150 km is the
        // usable range band (nearer, clutter fakes couplets; farther, the beam is too high and
        // too coarsely sampled).
        let hits = wxdata::rotation::detect_volume(&pairs, 25.0, 20.0, 15.0, 150.0, 3);
        Some((hits, (radar_lon, radar_lat), pairs.len()))
    }

    /// Raw couplets for this volume plus the radar position and tilt count the alert banner and
    /// log need: the cached ones if this volume has already been scanned, otherwise a fresh
    /// (expensive, per-tilt) detection, cached before returning. Kept raw and never corroborated
    /// by debris — see [`Self::compute_couplets`] and [`wxdata::tds::cross_corroborate`] — so the
    /// TDS layer reading this to corroborate its own hits never sees a confidence debris itself
    /// already raised.
    fn couplets_raw(
        &mut self,
        idx: usize,
    ) -> (Vec<wxdata::rotation::CoupletHit>, (f32, f32), usize) {
        let key = self.volume_key(idx);
        if let Some((k, v)) = &self.couplet_cache {
            if *k == key {
                return v.clone();
            }
        }
        let out = self.detect_couplets(idx).unwrap_or_default();
        self.couplet_cache = Some((key, out.clone()));
        out
    }

    /// DVR instant replay: jump the active timeline to the earliest frame still buffered in the
    /// decode cache and loop-play from there, so the recent session replays instantly from RAM.
    fn instant_replay(&mut self) {
        let start = {
            let tl = &self.views[self.active].timeline;
            if tl.frames.is_empty() {
                return;
            }
            tl.frames
                .iter()
                .position(|id| self.scan_cache.contains(&id.name().to_string()))
                .unwrap_or(0)
        };
        let tl = &mut self.views[self.active].timeline;
        tl.following = false;
        tl.playhead = start;
        tl.playing = true;
        tl.loop_enabled = true;
    }

    /// Count of the active timeline's frames currently held in the decode cache (DVR depth).
    fn dvr_depth(&self) -> usize {
        self.views[self.active]
            .timeline
            .frames
            .iter()
            .filter(|id| self.scan_cache.contains(&id.name().to_string()))
            .count()
    }

    /// Tornado-climatology query at `(lon, lat)`: if the SPC track database is loaded, list nearby
    /// historical tornadoes; otherwise start the (cached) async load and queue the query.
    fn query_climatology(&mut self, lon: f64, lat: f64) {
        const RADIUS_KM: f64 = 40.0; // ~25 mi
        self.climo_open = true;
        self.climo_error = None;
        self.query_warning_history(lon, lat);
        if let Some(tracks) = self.climo_tracks.clone() {
            self.climo_hits = wxdata::torclimo::near(&tracks, lon, lat, RADIUS_KM);
            self.climo_center = Some((lon, lat));
            return;
        }
        self.climo_center = Some((lon, lat));
        self.climo_pending_query = Some((lon, lat));
        self.load_climatology();
    }

    /// Kick off the one-time tornado-database load: read the on-disk cache if present, else download
    /// the SPC CSV and cache it. Idempotent while a load is already in flight.
    fn load_climatology(&mut self) {
        if self.climo_loading || self.climo_tracks.is_some() {
            return;
        }
        self.climo_loading = true;
        let (tx, rx) = std::sync::mpsc::channel();
        self.climo_rx = Some(rx);
        let http = self.http.clone();
        let cache = crate::paths::cache_dir().map(|d| d.join("torclimo_1950-2022.csv"));
        self.spawner.spawn(async move {
            let res = load_or_fetch_climo(&http, cache)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
    }

    /// Lon/lat bounds `(min_lon, min_lat, max_lon, max_lat)` of the active pane's viewport.
    fn view_bounds(&self) -> (f64, f64, f64, f64) {
        use crate::render::mercator::world_to_lonlat;
        let cam = &self.views[self.active].camera;
        let vp = self.last_viewport;
        let (wx0, wy0) = cam.screen_to_world((0.0, 0.0), vp);
        let (wx1, wy1) = cam.screen_to_world((vp.0, vp.1), vp);
        let (lon0, lat0) = world_to_lonlat(wx0, wy0);
        let (lon1, lat1) = world_to_lonlat(wx1, wy1);
        (
            lon0.min(lon1),
            lat0.min(lat1),
            lon0.max(lon1),
            lat0.max(lat1),
        )
    }

    /// Act on the signals the chrome raised this frame (drawer sections, mobile sheets, pills).
    fn apply_ui_actions(&mut self, actions: ui::layer_options::UiActions, ctx: &egui::Context) {
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

    /// Basemap style, smoothing, the startup view and the offline chase pack — the map knobs you
    /// set once and forget. They used to be the toolbox's "Map" section; they now sit under the
    /// drawer's App group (and the mobile drawer's Advanced group), which is the only other place
    /// per-map state is edited.
    fn map_rows(&mut self, ui: &mut egui::Ui, actions: &mut ui::layer_options::UiActions) {
        use crate::settings::StartView;
        let chasepack = self.chasepack_ui();
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.spacing_mut().interact_size.y = 32.0;
        let current = self.views[self.active].basemap;
        ui.label(egui::RichText::new("Background").strong());
        let mut picked = None;
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
            ui.menu_button(format!("{}    Change…", current.label()), |ui| {
                let width = (ui.ctx().content_rect().width() - 48.0).clamp(220.0, 460.0);
                ui.set_min_width(width);
                egui::ScrollArea::vertical()
                    .max_height(460.0)
                    .show(ui, |ui| {
                        picked =
                            ui::basemap_picker::grid(ui, &mut self.tiles, current, &self.settings);
                    });
                if picked.is_some() {
                    ui.close();
                }
            })
            .response
            .on_hover_text("Choose a map style. Shortcut: Z cycles backgrounds.");
        });
        if let Some(style) = picked {
            self.set_basemap(style);
        }
        ui.add_space(4.0);
        ui.separator();
        ui.label(egui::RichText::new("Radar appearance").strong());
        let mut smooth = self.settings.smooth_radar;
        ui.horizontal(|ui| {
            let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            for (label, value, hint) in [
                ("Crisp", false, "Show each radar gate with a sharp edge"),
                ("Smooth", true, "Blend neighboring radar gates"),
            ] {
                if ui
                    .add_sized(
                        egui::vec2(width, 34.0),
                        egui::Button::new(label)
                            .selected(smooth == value)
                            .corner_radius(9.0),
                    )
                    .on_hover_text(hint)
                    .clicked()
                {
                    smooth = value;
                }
            }
        });
        if smooth != self.settings.smooth_radar {
            self.settings.smooth_radar = smooth;
            for view in &mut self.views {
                view.smooth = smooth;
            }
        }
        ui.checkbox(
            &mut self.settings.live_scan_indicator,
            "Live sweep indicator",
        )
        .on_hover_text(
            "Show an animated ring and tilt-progress bar next to the scrubber's Live badge \
                 while a live chunk stream is actively updating this pane.",
        );
        ui.horizontal(|ui| {
            ui.label("Live sweep display:");
            ui.selectable_value(
                &mut self.settings.live_sweep_mode,
                crate::settings::LiveSweepMode::ContinuousComposite,
                "Continuous composite",
            )
            .on_hover_text("Keep the previous pass dimmed until each azimuth is rescanned.");
            ui.selectable_value(
                &mut self.settings.live_sweep_mode,
                crate::settings::LiveSweepMode::StrictCurrentSweep,
                "Current sweep only",
            )
            .on_hover_text("Hide all radial rows identified as the previous pass.");
        });
        let (view, settings) = (&mut self.views[self.active], &mut self.settings);

        // A download in flight stays above the disclosure — progress you can't find reads as a hang.
        if let Some((done, total, errors, mb)) = chasepack.progress {
            ui.separator();
            let frac = if total > 0 {
                done as f32 / total as f32
            } else {
                1.0
            };
            ui.add(egui::ProgressBar::new(frac).text(format!("{done}/{total} tiles · {mb:.0} MB")));
            if errors > 0 {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 120, 60),
                    format!("{errors} failed"),
                );
            }
            if ui.button("Cancel download").clicked() {
                actions.cancel_chasepack = true;
            }
            return;
        }

        ui.separator();
        ui.label(egui::RichText::new("On launch").strong());
        {
            // Startup view: remember this site + camera as the launch position.
            if ui
                .add_enabled(
                    view.site.is_some(),
                    egui::Button::new("Start here next time"),
                )
                .on_hover_text("Open here (site + map position) on next launch")
                .clicked()
            {
                if let Some(site) = &view.site {
                    settings.start_view = Some(StartView {
                        site: site.clone(),
                        x: view.camera.center.0,
                        y: view.camera.center.1,
                        zoom: view.camera.zoom,
                    });
                }
            }
            if let Some(site) = settings.start_view.as_ref().map(|sv| sv.site.clone()) {
                let mut clear = false;
                ui.horizontal(|ui| {
                    ui.weak(format!("Starts at {site}"));
                    clear = ui.small_button("Reset").clicked();
                });
                if clear {
                    settings.start_view = None;
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        ui.collapsing("Offline maps", |ui| {
            // Offline chase pack: pre-cache this view's basemap tiles so it renders with no signal.
            ui.separator();
            if !chasepack.packable {
                if cfg!(target_arch = "wasm32") {
                    ui.weak("Offline packs are available in the desktop and Android apps");
                } else {
                    ui.weak("Offline pack: pick a raster or vector basemap");
                }
            } else {
                ui.weak(format!(
                    "Offline pack: {} tiles ≈ {:.0} MB (z{}–{}, current view)",
                    chasepack.tiles, chasepack.mb, chasepack.z_lo, chasepack.z_hi
                ));
                ui.checkbox(&mut settings.pack_include_vector, "Include streets")
                    .on_hover_text(
                        "Pack vector street tiles beside raster imagery, so road names still \
                         render offline",
                    );
                ui.checkbox(&mut settings.pack_include_satellite, "Include satellite")
                    .on_hover_text(
                        "Pack satellite imagery beside the vector streets, so terrain still \
                         renders offline",
                    );
                ui.checkbox(&mut settings.pack_hires_dem, "High-detail terrain")
                    .on_hover_text(
                        "z12 terrain (~40 m/px) instead of z10 — sixteen times the tiles",
                    );
                let too_big = chasepack.mb > 2000.0;
                if ui
                    .add_enabled(!too_big, egui::Button::new("⬇ Download offline pack"))
                    .on_hover_text(
                        "Cache this view's basemap tiles to disk for offline use in the field",
                    )
                    .clicked()
                {
                    actions.download_chasepack = true;
                }
                if too_big {
                    ui.colored_label(
                        egui::Color32::from_rgb(230, 90, 90),
                        "Too large (>2 GB) — zoom in or narrow the view",
                    );
                }
            }
        });
    }

    /// Whether distances should read in kilometres: they should everywhere the US networks do not
    /// reach, which the active pane's radar tells us for free. No setting, because a setting here
    /// is one more thing to get wrong — someone watching Hamburg wants kilometres, and someone
    /// watching Oklahoma wants miles, and the pane already knows which one they are looking at.
    ///
    /// A pane with no site loaded reads as the US, which is what every path assumed before there
    /// was more than one network.
    fn metric(&self) -> bool {
        self.metric_in(self.active)
    }

    /// The same question for a specific pane, which is what a per-pane drawing (the measure tool)
    /// wants: split panes can show Oklahoma beside Bavaria.
    fn metric_in(&self, idx: usize) -> bool {
        !matches!(
            self.views[idx]
                .site
                .as_deref()
                .map_or(wxdata::sites::Network::Nexrad, wxdata::sites::network),
            wxdata::sites::Network::Nexrad | wxdata::sites::Network::Tdwr
        )
    }

    /// Lon/lat box `±radius_km` around the active pane's radar site (its coverage area), or `None`
    /// when no site is selected. Used to scope new-warning banners to the viewed radar.
    fn active_site_bounds(&self, radius_km: f64) -> Option<(f64, f64, f64, f64)> {
        let site = self.views[self.active].site.as_deref()?;
        let s = wxdata::sites::site_by_id(site)?;
        let (lat, lon) = (s.latitude as f64, s.longitude as f64);
        let dlat = radius_km / 111.0;
        let dlon = radius_km / (111.0 * lat.to_radians().cos().abs().max(0.01));
        Some((lon - dlon, lat - dlat, lon + dlon, lat + dlat))
    }

    /// Thin right-edge colorbar: the active pane's moment scale, docked so it never covers the map.
    /// Docked timeline scrubber (desktop): transport + scrub + live badge in a full-width bar
    /// under the map. The date picker, loop and speed are one right-click away on the
    /// LIVE/ARCHIVE badge — the transport is the 95% case and gets the pixels.
    /// [`Settings::tz_for`] for the active pane — the zone for chrome that isn't per-pane.
    pub(crate) fn active_tz(&self) -> Option<wxdata::tz::Tz> {
        self.settings
            .tz_for(self.views[self.active].site.as_deref())
    }

    /// Sidebar header: the site, what you're looking at, its tilt, and the per-product knobs.
    ///
    /// The product list itself is the tree's Radar category (with a plain-English blurb per row);
    /// this section owns everything about the *current* product — the tilt picker and the expert
    /// options that used to hide in the toolbox. All of it writes the same fields the hotkeys do.
    fn product_section(&mut self, ui: &mut egui::Ui, actions: &mut ui::layer_options::UiActions) {
        use crate::ui::a11y::Named as _;
        use crate::ui::style;
        let (moment, srv, tilt) = {
            let v = &self.views[self.active];
            (v.moment, v.srv, v.tilt)
        };
        let elevations = self.views[self.active]
            .volume
            .as_ref()
            .map(|v| v.elevations.clone())
            .unwrap_or_default();
        let site = self.views[self.active]
            .site
            .clone()
            .unwrap_or_else(|| "Pick a site".to_string());

        let pick: Option<(wxdata::level2::Moment, bool)> = None;
        let mut pick_tilt: Option<usize> = None;
        // Expert knobs for the product you're on, edited through locals so the popup closure
        // doesn't need `self`. They used to live in the toolbox's Product ▸ Options disclosure.
        let mut srv_from_cells = false;
        let mut dealias = self.settings.dealias_velocity;
        let mut precip_tint = self.settings.precip_tint;
        let mut srv_on = srv;
        let mi = moment.index();
        let (mut dir_deg, mut speed_kt) = {
            let v = &self.views[self.active];
            (v.storm_dir_deg, v.storm_speed_kt)
        };
        let mut thr_on = self.views[self.active].threshold_enabled[mi];
        let mut thr = self.views[self.active].thresholds[mi];
        let (vmin, vmax) = moment.value_range();
        let (unit_factor, unit_label) = display_units(moment, &self.settings);
        let health = self.radar_health();
        let (status, status_color) = ui::layers_panel::health_look(health.state());
        let product_rect = style::glass(ui, 250)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(
                        egui::RichText::new(egui_phosphor::regular::BROADCAST)
                            .size(24.0).color(crate::theme::accent(self.settings.theme)))
                        .min_size(egui::vec2(42.0, 42.0)).corner_radius(21.0))
                        .named("Choose the radar site").clicked() {
                        actions.open_site_dialog = true;
                    }
                    ui.vertical(|ui| {
                        if ui.add(egui::Button::new(egui::RichText::new(&site).strong())
                            .frame(false)).on_hover_text("Choose the radar site").clicked() {
                            actions.open_site_dialog = true;
                        }
                        if let Some(site) = wxdata::sites::site_by_id(&site) {
                            ui.label(egui::RichText::new(format!("{}, {}", site.city, site.state)).size(12.0)
                                .color(ui.visuals().weak_text_color()));
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(format!("● {status}"))
                            .size(11.0).color(status_color))
                            .on_hover_text("Radar source freshness");
                    });
                });
                ui.add_space(8.0);
                ui.label(egui::RichText::new(crate::products::name(moment, srv))
                    .size(style::FONT_TITLE).strong());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Tilt")
                            .size(style::FONT_SM)
                            .color(ui.visuals().weak_text_color()),
                    )
                    .on_hover_text("How high above the ground the beam is looking");
                    let selected = elevations
                        .get(tilt)
                        .map_or_else(|| "Loading…".to_string(), |a| format!("{a:.1}\u{b0}"));
                    egui::ComboBox::from_id_salt("tilt_picker")
                        .selected_text(selected)
                        .width(78.0)
                        .show_ui(ui, |ui| {
                            for (i, angle) in elevations.iter().enumerate() {
                                if ui
                                    .selectable_label(i == tilt, format!("{angle:.1}\u{b0}"))
                                    .clicked()
                                {
                                    pick_tilt = Some(i);
                                }
                            }
                        });
                });
                egui::CollapsingHeader::new("Product settings")
                    .default_open(false)
                    .show(ui, |ui| {
                        if moment == wxdata::level2::Moment::Reflectivity {
                            ui.checkbox(&mut precip_tint, "Tint by precipitation type")
                                .on_hover_text(
                                    "Colour the echo blue where it is falling as snow and pink \
                                     where it is freezing rain or sleet, from the MRMS surface \
                                     type. Reflectivity alone cannot tell them apart.",
                                );
                        }
                        if moment == wxdata::level2::Moment::Velocity {
                            ui.checkbox(&mut dealias, "Dealias").on_hover_text(
                                "Unfold aliased velocity (region-based dealiasing)",
                            );
                            ui.checkbox(&mut srv_on, "Storm-relative");
                            if srv_on {
                                ui.horizontal(|ui| {
                                    ui.label("Motion:");
                                    ui.add(
                                        egui::DragValue::new(&mut dir_deg)
                                            .range(0.0..=359.0)
                                            .suffix("\u{b0}"),
                                    );
                                    ui.add(
                                        egui::DragValue::new(&mut speed_kt)
                                            .range(0.0..=150.0)
                                            .suffix(" kt"),
                                    );
                                });
                                if ui
                                    .button("From storm cells")
                                    .on_hover_text(
                                        "Set motion to the SCIT storm-cell mean (needs L3 storm cells)",
                                    )
                                    .clicked()
                                {
                                    srv_from_cells = true;
                                }
                            }
                        }
                        // Threshold for the active moment. The slider value stays internal (m/s
                        // for velocity); display honors the Units setting.
                        let f = unit_factor as f64;
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut thr_on, "Threshold").on_hover_text(
                                "Hide everything below a value \u{2014} cuts light rain out of the picture",
                            );
                            if thr_on {
                                let t = thr.get_or_insert((vmin + vmax) * 0.5);
                                ui.add(
                                    egui::Slider::new(t, vmin..=vmax)
                                        .custom_formatter(move |v, _| format!("{:.0}", v * f))
                                        .custom_parser(move |s| {
                                            s.parse::<f64>().ok().map(|x| x / f)
                                        })
                                        .suffix(unit_label),
                                );
                            }
                        });
                    });
            })
            .response
            .rect;
        self.tour_anchors.product = Some(product_rect);

        if let Some(i) = pick_tilt {
            self.views[self.active].tilt = i;
        }
        if self.settings.precip_tint != precip_tint {
            self.settings.precip_tint = precip_tint;
            self.settings.save();
        }
        self.settings.dealias_velocity = dealias;
        // A product row was clicked this frame: it already set the moment and SRV flag, so the
        // knob write-back must not put the pre-click values back.
        if pick.is_none() {
            let v = &mut self.views[self.active];
            v.srv = srv_on;
            v.storm_dir_deg = dir_deg;
            v.storm_speed_kt = speed_kt;
            v.threshold_enabled[mi] = thr_on;
            v.thresholds[mi] = thr;
        }
        if srv_from_cells {
            if let Some((dir, spd)) = self.scit_mean_motion() {
                let v = &mut self.views[self.active];
                v.storm_dir_deg = dir;
                v.storm_speed_kt = spd;
                v.srv = true;
            }
        }
    }

    /// The boolean behind an [`OverlayToggle`]. One place to resolve a named toggle so the
    /// layers panel, command palette, and mobile sheet never drift apart.
    /// Does any pane want this field layer? The grids are fetched once and shared, so one pane
    /// asking is enough to keep the download alive.
    fn field_wanted(&self, layer: crate::render::FieldLayer) -> bool {
        self.views.iter().any(|v| v.fields_on.contains(&layer))
    }

    /// Turn a field layer on or off in the active pane.
    fn set_field(&mut self, layer: crate::render::FieldLayer, on: bool) {
        let set = &mut self.views[self.active].fields_on;
        if on {
            set.insert(layer);
        } else {
            set.remove(&layer);
        }
    }

    /// Whether this frame draws the analyst workstation (`app::chrome::dock`) rather than the
    /// floating or ribbon chrome: the actions that open "the panel" open its windows instead.
    ///
    /// Not at phone width: the workstation is docks and bars around a map, and in a phone's
    /// browser (the native phone app has its own chrome already) there is no map left between
    /// them. The design plan's §9 rule — a phone gets the map-first composition, not shrunken
    /// desktop docking — so under 600 pt the minimal floating chrome draws instead, sharing the
    /// same state; the workstation comes back as the window widens. The arrangement is untouched.
    ///
    /// On a phone with the Station design, the workstation is drawn in its phone form instead
    /// (`app::chrome::dock::phone`): its windows are the tabs of a bottom sheet.
    pub(crate) fn workstation_chrome(&self) -> bool {
        self.phone_station()
            || (self.settings.layout.is_workstation()
                && !crate::platform::phone_layout()
                && !workstation_too_narrow(self.window_w))
    }

    fn overlay_flag(&mut self, t: OverlayToggle) -> &mut bool {
        use OverlayToggle as T;
        match t {
            // The workstation has an Alerts window of its own rather than the panel's tab.
            T::AlertPanel if self.workstation_chrome() => &mut self.dock.alerts.open,
            T::AlertPanel => &mut self.show_alert_panel,
            T::StormReports => &mut self.show_storm_reports,
            T::Spotters => &mut self.show_spotters,
            T::RadarSites => &mut self.show_radar_sites,
            T::Metar => &mut self.show_metar,
            T::Webcams => &mut self.show_webcams,
            T::Fires => &mut self.show_fires,
            T::Aqi => &mut self.show_aqi,
            T::Stations => &mut self.show_stations,
            T::Dat => &mut self.show_dat,
            T::Gauges => &mut self.show_gauges,
            T::Tropical => &mut self.show_tropical,
            T::Outages => &mut self.show_outages,
            T::ForecastZones => &mut self.boundaries.show_zones,
            T::CwaBoundaries => &mut self.boundaries.show_cwa,
            T::ProbSevere => &mut self.show_probsevere,
            T::Aviation => &mut self.show_aviation,
            T::Tfr => &mut self.show_tfr,
            T::RangeRings => &mut self.show_range_rings,
            T::Fronts => &mut self.show_fronts,
            T::GlmLightning => &mut self.show_glm,
            T::Strikes => &mut self.show_strikes,
            T::Wind => &mut self.show_wind,
            T::RadarWind => &mut self.radar_wind.on,
            T::Sensors => &mut self.show_sensors,
            T::Hodo => &mut self.show_hodo,
            T::Cells => &mut self.filters.show_cells,
            T::Tracks => &mut self.filters.show_tracks,
            T::ArrivalCones => &mut self.filters.show_arrival_cones,
            T::Nowcast => &mut self.filters.show_nowcast,
            T::Trail => &mut self.filters.show_trail,
            T::ScanAge => &mut self.show_scan_age,
            T::Tds => &mut self.filters.show_tds,
            T::Tbss => &mut self.filters.show_tbss,
            T::ZdrColumns => &mut self.filters.show_zdr_columns,
            T::Couplets => &mut self.filters.show_couplets,
            T::TornadoId => &mut self.filters.show_tornado_id,
            T::YallMode => &mut self.settings.yall_mode,
            T::LayerProbe => &mut self.settings.layer_probe,
            T::MergeTornado => &mut self.settings.merge_tornado_signals,
            T::Globe => &mut self.settings.globe,
            T::ScaleBar => &mut self.settings.scale_bar,
            T::Alerts => &mut self.filters.show_alerts,
            T::Mds => &mut self.filters.show_mds,
            T::Watches => &mut self.filters.show_watches,
            T::LocalTracks => &mut self.show_local_tracks,
            T::Mping => &mut self.show_mping,
            T::Pireps => &mut self.show_pireps,
            T::Recon => &mut self.show_recon,
            T::LinkCameras => &mut self.link_cameras,
            T::LinkTimes => &mut self.link_times,
            T::LockSourceTime => &mut self.lock_source_time,
            T::LinkSite => &mut self.link_site,
            T::LinkCursor => &mut self.link_cursor,
            T::LinkStorm => &mut self.link_storm,
            T::MiniLoop => &mut self.mini_loop,
            T::Blockage => &mut self.show_blockage,
            T::LowestTilt => &mut self.show_lowest_tilt,
            T::DayNight => &mut self.show_daynight,
            T::ImportedGis => &mut self.show_imported_gis,
        }
    }

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
                W::Settings => self.settings_window.open = true,
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

    fn poll_overlays(&mut self) {
        crate::prof_scope!("poll_overlays");
        let mut changed = false;
        while let Ok(delivery) = self.overlay_rx.try_recv() {
            let msg = match delivery {
                OverlayDelivery::Immediate(msg) => msg,
                OverlayDelivery::Fetched {
                    lane,
                    generation,
                    result,
                } => {
                    let valid_time = result.as_ref().ok().and_then(OverlayMsg::health_valid_time);
                    // Plugin failures deliberately arrive as a message so the placefile manager
                    // can show them, but they are still failures for source health. Treating the
                    // message as a successful cached value would make both the status and cache
                    // residency lie.
                    let embedded_error = result.as_ref().ok().and_then(OverlayMsg::health_error);
                    let health_error = result.as_ref().err().map(String::as_str).or(embedded_error);
                    let current = self
                        .overlay_requests
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .finish(&lane, generation, health_error, valid_time);
                    if !current {
                        log::debug!("discarding stale {} reply", lane.label());
                        continue;
                    }
                    match result {
                        Ok(msg) => msg,
                        Err(err) => {
                            use crate::render::FieldLayer as FL;
                            match &lane {
                                RequestLane::Field(FL::ModelDiff) => {
                                    self.diff_valid = None;
                                    self.diff_grid = None;
                                    self.diff_display_key = None;
                                    self.diff_error = Some(err.clone());
                                    if let Some(state) = self.fields.get_mut(&FL::ModelDiff) {
                                        state.pending = None;
                                        state.stamp = None;
                                    }
                                    self.overlay_requests
                                        .lock()
                                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                                        .set_cache_resident(&lane, false);
                                }
                                RequestLane::Field(FL::Ensemble) => {
                                    self.ensemble_run = None;
                                    self.ensemble_grid = None;
                                    self.ensemble_display_key = None;
                                    self.ensemble_error = Some(err.clone());
                                    if let Some(state) = self.fields.get_mut(&FL::Ensemble) {
                                        state.pending = None;
                                        state.stamp = None;
                                    }
                                    self.overlay_requests
                                        .lock()
                                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                                        .set_cache_resident(&lane, false);
                                }
                                RequestLane::Field(FL::CompareA) => {
                                    self.compare_valid = None;
                                    self.compare_grid = None;
                                    self.compare_error = Some(err.clone());
                                    for layer in [FL::CompareA, FL::CompareB] {
                                        if let Some(state) = self.fields.get_mut(&layer) {
                                            state.pending = None;
                                            state.stamp = None;
                                        }
                                    }
                                    self.overlay_requests
                                        .lock()
                                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                                        .set_cache_resident(&lane, false);
                                }
                                _ => {}
                            }
                            note_feed_error(lane.label(), err);
                            continue;
                        }
                    }
                }
            };
            match msg {
                OverlayMsg::AlertSeed(f) => {
                    for id in f
                        .iter()
                        .filter_map(|f| f.alert.as_ref().map(|a| a.dedupe_key()))
                    {
                        self.known_warning_ids.insert(id);
                    }
                    if self.alert_features.is_empty() {
                        self.alert_features = f;
                    }
                }
                OverlayMsg::Alerts(f) => {
                    self.detect_new_warnings(&f);
                    crate::alert_snapshot::save(&f);
                    self.alert_features = f;
                }
                OverlayMsg::Mds(f) => self.md_features = f,
                OverlayMsg::Watches(f) => self.watch_features = f,
                OverlayMsg::Mping(r) => self.mping_reports = r,
                OverlayMsg::Pireps(p) => self.pireps = p,
                OverlayMsg::Recon(o) => self.recon = o,
                OverlayMsg::Ero(day, f) => {
                    if day == self.filters.ero_day {
                        self.ero_features = f;
                    }
                }
                OverlayMsg::FireWx(day, f) => {
                    if day == self.filters.fire_day {
                        self.fire_features = f;
                    }
                }
                OverlayMsg::Wssi(day, f) => {
                    // A day change in flight must not overwrite the day now selected.
                    if day == self.filters.wssi_day {
                        self.wssi_features = f;
                    }
                }
                OverlayMsg::Outlook(day, f) => {
                    if (1..=3).contains(&day) {
                        self.outlook_features[(day - 1) as usize] = f;
                    }
                }
                OverlayMsg::Cells(site, cells, past) => {
                    // Keep only if still the active site.
                    if self.views[self.active].site.as_deref() == Some(site.as_str()) {
                        // Reset trend history on a site change; append this volume's samples.
                        if self.cells_site.as_deref() != Some(site.as_str()) {
                            self.cell_trends.clear();
                        }
                        // The same score the storm-cells table ranks by, from the same cached
                        // couplets, so the trend line and the table's number never disagree. Read
                        // from the cache only: a cell-product arrival must not kick off a
                        // rotation detection pass of its own.
                        let couplets: &[wxdata::rotation::CoupletHit] = match &self.couplet_cache {
                            Some((_, (hits, ..))) => hits,
                            None => &[],
                        };
                        let scores: Vec<u8> =
                            wxdata::cellscore::score_all(&cells, &self.probsevere, couplets);
                        for (c, &score) in cells.iter().zip(&scores) {
                            if c.id.is_empty() {
                                continue;
                            }
                            let hist = self.cell_trends.entry(c.id.clone()).or_default();
                            let sample = ui::cell_window::CellSample {
                                vil: c.vil,
                                top: c.top_kft,
                                dbz: c.max_dbz,
                                severity: Some(score),
                                time: c.time,
                                dbz_hgt: c.max_dbz_hgt_kft,
                            };
                            // Skip a duplicate of the last sample (same volume re-fetched).
                            if hist.last().is_none_or(|s| {
                                (s.vil, s.top, s.dbz, s.severity)
                                    != (sample.vil, sample.top, sample.dbz, sample.severity)
                            }) {
                                hist.push(sample);
                                if hist.len() > CELL_TREND_MAX {
                                    hist.remove(0);
                                }
                            }
                        }
                        // Cell ids churn every volume, and the map only ever grew — an entry
                        // per cell the radar has ever named, for as long as the site is the same.
                        // Keep the ones this volume still has.
                        self.cell_trends
                            .retain(|id, _| cells.iter().any(|c| &c.id == id));
                        if !past.is_empty() {
                            merge_cell_history(&mut self.cell_trends, &past);
                            self.cells_history_site = Some(site.clone());
                        }
                        self.storm_cells = cells;
                        self.cells_site = Some(site);
                        self.update_follow();
                    }
                }
                OverlayMsg::Placefile(url, pf) => {
                    if let Some(lp) = self.placefiles.iter_mut().find(|lp| lp.url == url) {
                        lp.pf = pf;
                        lp.loaded = true;
                        lp.error = None;
                        lp.last_fetch = Some(Instant::now());
                        self.overlay_gen = self.overlay_gen.wrapping_add(1);
                    }
                }
                OverlayMsg::PlacefileError(url, err) => {
                    log::warn!("{url}: {err}");
                    if let Some(lp) = self.placefiles.iter_mut().find(|lp| lp.url == url) {
                        lp.error = Some(err);
                        lp.last_fetch = Some(Instant::now());
                    }
                }
                OverlayMsg::Field(layer, field) => {
                    // A GOES layer read from a mesoscale sector says where the sector is now.
                    if let Some(&sector) = self.goes_fetched_sector.get(&layer) {
                        if sector.is_meso() {
                            self.goes_footprint =
                                Some((sector, wxdata::goes_abi::Footprint::of(&field)));
                        }
                    }
                    self.accept_field(layer, field, None);
                }
                OverlayMsg::GoesFootprint(sector, fp) => {
                    self.goes_footprint = Some((sector, fp));
                }
                OverlayMsg::GlmWindow(end, flashes) => {
                    self.glm_archive = Some((end, flashes));
                    // The density grid is rebuilt from these now, not on its next tick.
                    self.glm_fed_last = None;
                }
                OverlayMsg::StampedField(layer, field) => {
                    self.accept_field(layer, field.data, Some(field.stamp));
                }
                OverlayMsg::MrmsField(layer, field, request) => {
                    if self.mrms_request(layer).as_ref() == Some(&request)
                        && request.accepts(&field.stamp)
                    {
                        self.accept_field(layer, field.data, Some(field.stamp));
                        if let Some(state) = self.fields.get_mut(&layer) {
                            state.mrms_request = Some(request);
                            state.last_fetch = Some(Instant::now());
                        }
                    }
                }
                OverlayMsg::ModelDiff(kind, fh, field, pct, valid)
                    if kind == self.diff_field && fh == self.global_fcst_hour =>
                {
                    let layer = crate::render::FieldLayer::ModelDiff;
                    self.diff_pct = pct;
                    let upload = self.diff_upload(&field);
                    if let Some(s) = self.fields.get_mut(&layer) {
                        s.pending = Some(upload);
                        let (a, b) = self.diff_field.pair();
                        s.stamp = Some(field_state::model_stamp(
                            &self.diff_mode.expression(a, b),
                            self.diff_field.slug(),
                            &field,
                            None,
                            true,
                        ));
                    }
                    self.diff_valid = Some(valid);
                    self.diff_error = None;
                    self.diff_grid = Some(field);
                    self.diff_display_key = Some((self.diff_field, self.diff_mode));
                }
                OverlayMsg::ModelDiff(..) => {}
                OverlayMsg::Compare(field, fh, a, b, valid) => {
                    // A selection change in flight must not overwrite the field now selected.
                    if field == self.diff_field && fh == self.global_fcst_hour {
                        use crate::render::FieldLayer as FL;
                        let source = field.source_layer();
                        let upload_a = self.field_upload(source, &a);
                        let upload_b = self.field_upload(source, &b);
                        let (model_a, model_b) = field.pair();
                        if let Some(s) = self.fields.get_mut(&FL::CompareA) {
                            s.pending = Some(upload_a);
                            s.stamp = Some(field_state::model_stamp(
                                model_a,
                                field.slug(),
                                &a,
                                Some(valid.a_run),
                                false,
                            ));
                        }
                        if let Some(s) = self.fields.get_mut(&FL::CompareB) {
                            s.pending = Some(upload_b);
                            s.stamp = Some(field_state::model_stamp(
                                model_b,
                                field.slug(),
                                &b,
                                Some(valid.b_run),
                                false,
                            ));
                        }
                        self.compare_valid = Some(valid);
                        self.compare_error = None;
                        self.compare_grid = Some((a, b));
                    }
                }
                OverlayMsg::Ensemble(field, fh, run) => {
                    // A selection change in flight must not overwrite the field now selected.
                    if field == self.ensemble.field && fh == self.ensemble_lead_hour() {
                        self.ensemble_run = Some(*run);
                        self.ensemble_error = None;
                        self.ensemble_display_key = None;
                        self.rebuild_ensemble_display();
                    }
                }
                OverlayMsg::StormReports(bucket, reports) => match bucket {
                    None => self.storm_reports = reports,
                    Some(b) => {
                        self.arch_lsr.put(b, reports);
                        if self.arch_lsr_inflight == Some(b) {
                            self.arch_lsr_inflight = None;
                        }
                    }
                },
                OverlayMsg::Aviation(f) => self.aviation_features = f,
                OverlayMsg::Tfr(new, remaining) => {
                    self.tfr_features.extend(new);
                    self.tfr_pending = remaining;
                }
                OverlayMsg::Spotters(spotters) => self.spotters = spotters,
                OverlayMsg::Fronts(a) => self.fronts = Some(a),
                OverlayMsg::FreezingLevels {
                    site,
                    epoch,
                    h0,
                    hm20,
                } => self.freezing = Some((site, epoch, h0, hm20)),
                OverlayMsg::ProbSevere(f) => {
                    self.evaluate_probsevere_rules(&f);
                    self.probsevere = f;
                }
                OverlayMsg::Hrrr(fc) => {
                    use crate::render::FieldLayer;
                    let run = fc.run;
                    let valid = fc.valid();
                    let upload = self.field_upload(FieldLayer::Hrrr, &fc.field);
                    let source = self.refl_source_label();
                    let stamp = field_state::model_stamp(
                        &source,
                        "Composite reflectivity",
                        &fc.field,
                        Some(run),
                        false,
                    );
                    if let Some(s) = self.fields.get_mut(&FieldLayer::Hrrr) {
                        s.pending = Some(upload);
                        s.grid = Some(fc.field);
                        s.stamp = Some(stamp);
                    }
                    self.hrrr_run = Some(run);
                    self.hrrr_valid = Some(valid);
                }
                OverlayMsg::Obs(site, res) => {
                    // Keep only if still the active site.
                    if self.views[self.active].site.as_deref() == Some(site.as_str()) {
                        self.sensor_data = Some(res);
                        self.sensor_site = Some(site);
                    }
                }
                OverlayMsg::Vwp(site, levels) => {
                    if self.views[self.active].site.as_deref() == Some(site.as_str()) {
                        // A site change starts a new time series; mixing radars on one axis would
                        // be nonsense.
                        if self.hodo_site.as_deref() != Some(site.as_str()) {
                            self.hodo_history.clear();
                        }
                        // The product carries no timestamp, so identical profiles mean "same scan
                        // refetched" — dedupe on content rather than stamping duplicates.
                        let dup = self
                            .hodo_history
                            .back()
                            .is_some_and(|(_, prev)| *prev == levels);
                        if !dup && !levels.is_empty() {
                            self.hodo_history.push_back((Utc::now(), levels.clone()));
                            // ~2 hours at the 5-minute refetch cadence.
                            while self.hodo_history.len() > 24 {
                                self.hodo_history.pop_front();
                            }
                        }
                        self.hodo_data = levels;
                        self.hodo_site = Some(site);
                    }
                }
                OverlayMsg::ArchiveWarnings(bucket, feats) => {
                    self.arch_warns.put(bucket, feats);
                    if self.arch_warn_inflight == Some(bucket) {
                        self.arch_warn_inflight = None;
                    }
                }
                OverlayMsg::ArchiveMds(bucket, feats) => {
                    self.arch_mds.put(bucket, feats);
                    if self.arch_md_inflight == Some(bucket) {
                        self.arch_md_inflight = None;
                    }
                }
                OverlayMsg::Metar(obs, tafs) => {
                    self.metars = obs;
                    self.tafs = tafs;
                }
                OverlayMsg::Webcams(sites) => {
                    // Drop the cached stills with the list they belonged to. Windy's free-tier
                    // image URLs expire after ten minutes, and a kept texture would otherwise
                    // show the same frame until the layer was toggled off and on.
                    self.pf_icon_tex.retain(|k, _| !k.starts_with("cam:"));
                    self.webcams = sites;
                }
                OverlayMsg::Aqi(obs) => self.aqi = obs,
                OverlayMsg::Fires(perims, incidents) => {
                    self.fire_perims = perims;
                    self.fire_incidents = incidents;
                    // Perimeters ride the tessellated overlay layer, so the assembled feature
                    // set has to be rebuilt — bumping the generation alone re-tessellates the
                    // old list and the perimeters never appear.
                    self.rebuild_overlays();
                }
                OverlayMsg::Stations(obs) => self.stations.ingest(obs),
                OverlayMsg::Ppef(p) => self.stations.ppef = Some(p),
                OverlayMsg::DotCams(cams) => self.stations.cams = cams,
                OverlayMsg::Mill(kv) => self.stations.mill_kv_per_m = Some(kv),
                OverlayMsg::Dat(mut points, tracks) => {
                    // Weakest first, so the EF4/EF5 points end up painted on top of the EF0 and
                    // straight-line-wind ones that outnumber them ten to one.
                    points.sort_by_key(|p| wxdata::dat::ef_number(&p.efscale).unwrap_or(0));
                    self.dat_points = points;
                    self.dat_tracks = tracks;
                }
                OverlayMsg::Mosaic(field, sites, oldest) => {
                    self.mosaic_sites = sites;
                    self.mosaic_oldest = Some(oldest);
                    let layer = crate::render::FieldLayer::Mosaic;
                    let upload = self.field_upload(layer, &field);
                    if let Some(s) = self.fields.get_mut(&layer) {
                        s.pending = Some(upload);
                        s.grid = Some(field);
                    }
                }
                OverlayMsg::Gauges(g) => self.gauges = g,
                OverlayMsg::Contours(kind, lines, valid, grid) => {
                    // Keep only if this kind is still active — it may have been turned off while
                    // the fetch was in flight.
                    if self.active_contours.contains(&kind) {
                        let entry = self.contours.entry(kind).or_default();
                        entry.lines = lines;
                        entry.valid = Some(valid);
                        entry.grid = Some(grid);
                    }
                }
                OverlayMsg::Tropical(data) => self.tropical = Some(data),
                OverlayMsg::Outages(f) => self.outage_features = f,
                OverlayMsg::Wind(w) => {
                    self.wind_inflight = None;
                    // Keep only if the selection didn't change while the fetch was in flight, and
                    // radar winds have not taken over the particles meanwhile.
                    if self.wind_fetched == Some((w.level, w.fcst_hour)) && !self.radar_wind.on {
                        self.wind = Some(*w);
                    }
                }
            }
            changed = true;
        }
        if changed {
            // One rebuild covers every message kind (ProbSevere/Tropical included).
            self.rebuild_overlays();
        }
    }

    /// The 5-min UTC bucket (Unix secs / 300) of the active pane's displayed frame, or `None` when
    /// following live (archive warnings only apply to scrubbed archive views).
    fn archive_bucket(&self) -> Option<i64> {
        let v = &self.views[self.active];
        if v.timeline.following {
            return None;
        }
        Some(v.volume.as_ref()?.time.timestamp() / 300)
    }

    /// The storm reports to display right now: the live trailing window, or the archived set
    /// while the active pane is scrubbed off-live (feature CC).
    /// The storm cells to show, which is none of them once the playhead is in the archive.
    ///
    /// Warnings and LSRs have archived equivalents that get swapped in ([`Self::sync_archive_warnings`],
    /// [`Self::sync_archive_lsr`]); Level 3 SCIT does not — the products are only published for the
    /// last couple of days. Leaving the live set on screen drew this afternoon's cells, tracks and
    /// arrival cones over a storm from 2011.
    fn active_storm_cells(&self) -> &[Cell] {
        if self.archive_bucket().is_some() {
            return &[];
        }
        &self.storm_cells
    }

    fn active_storm_reports(&self) -> &[wxdata::spc::StormReport] {
        if let Some(b) = self.arch_lsr_shown {
            if let Some(r) = self.arch_lsr.peek(&b) {
                return r;
            }
        }
        &self.storm_reports
    }

    /// Drive the archived-LSR set from the active pane's playhead (mirrors
    /// [`Self::sync_archive_warnings`], on 30-min buckets).
    fn sync_archive_lsr(&mut self, ctx: &egui::Context) {
        if !(self.show_storm_reports || self.filters.show_tds || self.filters.show_couplets) {
            return;
        }
        let bucket = (|| {
            let v = &self.views[self.active];
            if v.timeline.following {
                return None;
            }
            Some(v.volume.as_ref()?.time.timestamp() / 1800)
        })();
        match bucket {
            None => self.arch_lsr_shown = None,
            Some(b) => {
                let cached = self.arch_lsr.contains(&b);
                if !cached && self.arch_lsr_inflight != Some(b) {
                    self.arch_lsr_inflight = Some(b);
                    self.spawn_overlay(ctx, OverlaySource::StormReports(Some(b)));
                }
                if cached {
                    self.arch_lsr_shown = Some(b);
                }
            }
        }
    }

    /// Fetch the Area Forecast Discussion for the active site's WFO (feature DD).
    fn fetch_afd(&mut self) {
        let Some((lat, lon)) = self.views[self.active]
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
            .map(|s| (s.latitude as f64, s.longitude as f64))
        else {
            self.afd_error = Some("no site selected".into());
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.afd_rx = Some(rx);
        self.afd_busy = true;
        self.afd_error = None;
        let http = self.http.clone();
        self.spawner.spawn(async move {
            let res = wxdata::afd::fetch(&http, lat, lon)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
    }

    /// Fetch one NHC text product for `storm_id`.
    ///
    /// The URL comes out of the storm feed rather than being built from the id: NHC's product
    /// filenames key off the basin bin (`EP4`), not the storm id, and the feed already carries
    /// the exact page for the current advisory number.
    fn fetch_tropical_text(&mut self, storm_id: &str, product: ui::tropical_window::Product) {
        let Some(storm) = self
            .tropical
            .as_ref()
            .and_then(|t| t.storms.iter().find(|s| s.id == storm_id))
            .cloned()
        else {
            self.tropical_window.error = Some("that storm is no longer being advised on".into());
            return;
        };
        let Some(url) = product.url(&storm).map(str::to_string) else {
            self.tropical_window.error = Some(format!(
                "no {} published for {}",
                product.label(),
                storm.name
            ));
            self.tropical_window.text = None;
            return;
        };
        let title = format!(
            "{} {} — {}",
            storm.classification,
            storm.name,
            product.label()
        );
        let (tx, rx) = std::sync::mpsc::channel();
        self.tropical_text_rx = Some(rx);
        self.tropical_window.busy = true;
        self.tropical_window.error = None;
        let http = self.http.clone();
        self.spawner.spawn(async move {
            let res = wxdata::tropical::fetch_advisory(&http, &title, &url)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
    }

    /// Keep the FAA camera sites in view loaded. Same shape as [`Self::sync_metar`] but far
    /// lazier: the camera network doesn't move, so this only refetches when the view drifts out
    /// of the last box (or every 10 min, to pick up sites going in and out of maintenance).
    /// Keep the multi-radar composite current: refetch on the L3 cadence, and immediately when the
    /// camera has moved far enough that the sites in view changed.
    ///
    /// Live only. An archive scrub would need per-site historic N0B for the same minute, which is a
    /// different (and much slower) fetch; until that exists the toggle simply has nothing to show,
    /// so it reports that rather than lying with a live composite over a historic scene.
    /// One line describing the live composite for the drawer: which radars are in it and how stale
    /// its oldest scan is.
    fn mosaic_status(&self) -> String {
        if !self.views[self.active].timeline.following {
            return "Live only — the composite is hidden while you're scrubbing the archive."
                .to_string();
        }
        if self.mosaic_sites.is_empty() {
            return "building…".to_string();
        }
        let age = self
            .mosaic_oldest
            .map(|t| (chrono::Utc::now() - t).num_minutes().max(0))
            .unwrap_or(0);
        format!(
            "{} — oldest scan {age} min old",
            self.mosaic_sites.join(", ")
        )
    }

    /// Pull AirNow AQI for the view. Monitors report hourly, so does this; without a key the
    /// layer stays empty rather than firing requests that can only 401.
    fn sync_aqi(&mut self, ctx: &egui::Context) {
        if !self.show_aqi || self.settings.airnow_key.is_empty() {
            return;
        }
        let (min_lon, min_lat, max_lon, max_lat) = self.view_bounds();
        let stale = self
            .aqi_last_fetch
            .is_none_or(|t| t.elapsed().as_secs() >= 900);
        let (clon, clat) = ((min_lon + max_lon) * 0.5, (min_lat + max_lat) * 0.5);
        let drifted = self.aqi_bounds.is_none_or(|(lo0, la0, lo1, la1)| {
            let (mlon, mlat) = ((lo0 + lo1) * 0.5, (la0 + la1) * 0.5);
            let (hw, hh) = ((lo1 - lo0) * 0.25, (la1 - la0) * 0.25);
            (clon - mlon).abs() > hw || (clat - mlat).abs() > hh
        });
        if stale || drifted {
            let b = (min_lon, min_lat, max_lon, max_lat);
            self.aqi_last_fetch = Some(Instant::now());
            self.aqi_bounds = Some(b);
            self.spawn_overlay(
                ctx,
                OverlaySource::Aqi(b.0, b.1, b.2, b.3, self.settings.airnow_key.clone()),
            );
        }
    }

    /// Pull wildfire perimeters/incidents for the view. Fires move on the scale of hours, so a
    /// 15-minute clock is plenty; the bbox check is what keeps panning cheap.
    fn sync_fires(&mut self, ctx: &egui::Context) {
        if !self.show_fires {
            return;
        }
        let (min_lon, min_lat, max_lon, max_lat) = self.view_bounds();
        let stale = self
            .fire_last_fetch
            .is_none_or(|t| t.elapsed().as_secs() >= 900);
        let (clon, clat) = ((min_lon + max_lon) * 0.5, (min_lat + max_lat) * 0.5);
        let drifted = self.fire_bounds.is_none_or(|(lo0, la0, lo1, la1)| {
            let (mlon, mlat) = ((lo0 + lo1) * 0.5, (la0 + la1) * 0.5);
            let (hw, hh) = ((lo1 - lo0) * 0.25, (la1 - la0) * 0.25);
            (clon - mlon).abs() > hw || (clat - mlat).abs() > hh
        });
        if stale || drifted {
            let b = (min_lon, min_lat, max_lon, max_lat);
            self.fire_last_fetch = Some(Instant::now());
            self.fire_bounds = Some(b);
            self.spawn_overlay(ctx, OverlaySource::Fires(b.0, b.1, b.2, b.3));
        }
    }

    /// Drive the damage-survey overlay. Surveys are immutable history, so the fetch key is just the
    /// (rounded) view box and the day the active pane is looking at — scrub the timeline onto a
    /// storm date and the survey for that day appears over it.
    fn sync_dat(&mut self, ctx: &egui::Context) {
        if !self.show_dat {
            return;
        }
        let (min_lon, min_lat, max_lon, max_lat) = self.view_bounds();
        if (max_lon - min_lon) > 12.0 {
            return; // a continental query is tens of thousands of points
        }
        let day = self.views[self.active]
            .volume
            .as_ref()
            .map(|v| v.time.date_naive())
            .unwrap_or_else(|| chrono::Utc::now().date_naive());
        // Snap the box to a half-degree grid so panning around inside a county doesn't refetch.
        let snap = |v: f64, up: bool| {
            let g = v * 2.0;
            (if up { g.ceil() } else { g.floor() }) / 2.0
        };
        let bbox = (
            snap(min_lon, false),
            snap(min_lat, false),
            snap(max_lon, true),
            snap(max_lat, true),
        );
        if self.dat_key == Some((bbox, day)) {
            return;
        }
        self.dat_key = Some((bbox, day));
        self.spawn_overlay(ctx, OverlaySource::Dat(bbox, day));
    }

    /// The weather-radio rows: pick a configured relay, play or stop it, and see honestly whether
    /// it is actually on the air. One stream at a time.
    #[cfg(target_arch = "wasm32")]
    fn nwr_rows(&mut self, ui: &mut egui::Ui) {
        // The player decodes MP3 into a native audio device; the web build says so rather than
        // showing a play button that can't do anything.
        ui.weak("Weather radio playback is desktop and Android only.");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn nwr_rows(&mut self, ui: &mut egui::Ui) {
        if self.settings.nwr_streams.is_empty() {
            ui.weak("No relays yet — add one in Settings → Alerts.");
            ui.weak("NOAA broadcasts on VHF only; these are listener-run relays.");
            return;
        }
        let playing = self.nwr.as_ref().map(|p| p.name.clone());
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("nwr_pick")
                .selected_text(if self.nwr_pick.is_empty() {
                    "Pick a relay".to_string()
                } else {
                    self.nwr_pick.clone()
                })
                .show_ui(ui, |ui| {
                    for s in &self.settings.nwr_streams {
                        ui.selectable_value(&mut self.nwr_pick, s.name.clone(), &s.name);
                    }
                });
            match &playing {
                Some(_) => {
                    if ui.button("⏹ Stop").clicked() {
                        self.nwr = None; // Drop stops the thread.
                    }
                }
                None => {
                    let stream = self
                        .settings
                        .nwr_streams
                        .iter()
                        .find(|s| s.name == self.nwr_pick)
                        .cloned();
                    if ui
                        .add_enabled(stream.is_some(), egui::Button::new("▶ Play"))
                        .clicked()
                    {
                        if let Some(s) = stream {
                            self.nwr = Some(crate::nwr::Player::start(
                                s.name,
                                s.url,
                                self.settings.alert_volume,
                                self._rt.handle(),
                            ));
                        }
                    }
                }
            }
        });
        if let Some(p) = &self.nwr {
            match p.status() {
                crate::nwr::Status::Playing => ui.weak(format!("🔊 {}", p.name)),
                crate::nwr::Status::Connecting => ui.weak("connecting…"),
                crate::nwr::Status::Offline(why) => ui.colored_label(
                    egui::Color32::from_rgb(230, 150, 90),
                    format!("stream offline ({why}) — retrying"),
                ),
                crate::nwr::Status::Stopped => ui.weak("stopped"),
            };
        }
    }

    /// Carry out what a river-gauge card (a window, or the dashboard's selected gauge) asked for.
    pub(crate) fn gauge_card_action(
        &mut self,
        action: crate::ui::gauge_card::Action,
        ctx: &egui::Context,
    ) {
        match action {
            crate::ui::gauge_card::Action::Center { lat, lon } => {
                self.follow_cell = None;
                self.views[self.active].camera.center =
                    crate::render::mercator::lonlat_to_world(lon, lat);
            }
            crate::ui::gauge_card::Action::Share { lid, lat, lon } => {
                // The gauge, close enough in that its symbol is drawn, on the radar the sender is
                // looking at.
                let v = &self.views[self.active];
                let link = goto_link(&Goto {
                    site: v.site.clone().unwrap_or_default(),
                    lon,
                    lat,
                    zoom: v.camera.zoom.max(8.0),
                    time: None,
                    moment: None,
                    tilt: None,
                    basemap: None,
                    threshold: None,
                    srv: false,
                    gauges: vec![lid],
                    tropical: None,
                });
                if !crate::platform::share_link("HookEcho", &link) {
                    ctx.copy_text(link.clone());
                    self.banner("Link copied".to_string(), link);
                }
            }
        }
    }

    /// Carry out what the flood-gauge dashboard asked for.
    pub(crate) fn gauge_dash_out(
        &mut self,
        out: crate::ui::gauge_dashboard::Out,
        ctx: &egui::Context,
    ) {
        use crate::ui::gauge_dashboard::Out;
        match out {
            Out::ShowLayer => self.show_gauges = true,
            Out::Center { lat, lon } => {
                self.follow_cell = None;
                let cam = &mut self.views[self.active].camera;
                cam.center = crate::render::mercator::lonlat_to_world(lon, lat);
                // Close enough in that the gauge's symbol is drawn.
                cam.zoom = cam.zoom.max(8.0);
            }
            Out::Card(a) => self.gauge_card_action(a, ctx),
        }
    }

    /// The view is too wide for the gauge layer to fetch (see [`Self::sync_gauges`]).
    pub(crate) fn gauges_zoomed_out(&self) -> bool {
        let (min_lon, _, max_lon, _) = self.view_bounds();
        !(max_lon - min_lon).is_finite() || max_lon - min_lon > GAUGE_MAX_SPAN_DEG
    }

    /// Gauges in view at minor flooding or worse, for the dashboard's tab dot.
    pub(crate) fn gauges_in_flood(&self) -> usize {
        if !self.show_gauges {
            return 0;
        }
        crate::ui::gauge_dashboard::summarize(&self.gauges).in_flood()
    }

    /// Drive the HRRR contour fetch for every active kind: refetch on a kind's own selection/model
    /// change or every 15 min. The HRRR surface run updates hourly and contouring is cheap enough
    /// to redo on the model cadence. Several kinds can be active at once (ROADMAP_NEW J4-adjacent:
    /// overlaying more than one model-contour field, e.g. MSLP and CAPE together) — each fetches
    /// and refreshes independently, keyed by its own kind.
    fn sync_contours(&mut self, ctx: &egui::Context) {
        // Drop entries for a kind that was turned off — otherwise a deselected field's last-drawn
        // lines would linger in the map forever, since nothing else ever clears them.
        self.contours
            .retain(|k, _| self.active_contours.contains(k));
        for kind in self.active_contours.clone() {
            let key = (self.env_model, self.settings.temp_unit);
            let entry = self.contours.entry(kind).or_default();
            let changed = entry.fetched_key != Some(key);
            let stale = entry
                .last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 900);
            if changed {
                entry.lines.clear();
                entry.valid = None;
            }
            if changed || stale {
                entry.last_fetch = Some(Instant::now());
                entry.fetched_key = Some(key);
                self.spawn_overlay(
                    ctx,
                    OverlaySource::Contours(kind, self.env_model, self.settings.temp_unit),
                );
            }
        }
    }

    /// Storm-follow camera: re-lock onto the tracked cell in the freshly-applied volume and recenter
    /// the active pane on it. Called from the `Cells` apply arm. Reacquires across SCIT renumbering
    /// by predicting the cell's position from its last motion and adopting the nearest new cell.
    fn update_follow(&mut self) {
        let Some((fsite, last, since)) = self.follow_cell.take() else {
            return;
        };
        // Active site changed out from under the follow (site switch) → stop silently.
        if self.cells_site.as_deref() != Some(fsite.as_str()) {
            return;
        }
        // Same SCIT id in the new volume → the easy case.
        if let Some(c) = self
            .storm_cells
            .iter()
            .find(|c| !c.id.is_empty() && c.id == last.id)
            .cloned()
        {
            self.recenter_follow(&c);
            self.follow_cell = Some((fsite, c, Instant::now()));
            return;
        }
        // Renumber/miss: predict where the cell drifted and adopt the nearest new cell within 15 km.
        let elapsed_h = since.elapsed().as_secs_f64() / 3600.0;
        let pred = match (last.mvt_deg, last.mvt_kt) {
            (Some(dir), Some(kt)) if kt > 0.0 => crate::geo::destination_point(
                [last.lon, last.lat],
                dir as f64,
                kt as f64 * 1.852 * elapsed_h,
            ),
            _ => [last.lon, last.lat],
        };
        if let Some(c) = nearest_cell(&self.storm_cells, pred[0], pred[1], 15.0).cloned() {
            self.recenter_follow(&c);
            self.follow_cell = Some((fsite, c, Instant::now()));
        } else {
            self.follow_notice = Some((format!("Lost {} — follow ended", last.id), Instant::now()));
            // follow_cell already taken → stays None.
        }
    }

    /// Snap the active pane's camera onto a followed cell, keeping the current zoom.
    fn recenter_follow(&mut self, c: &Cell) {
        self.views[self.active].camera.center =
            crate::render::mercator::lonlat_to_world(c.lon, c.lat);
    }

    /// Top-right badge for the storm-follow camera: a tap-to-stop pill while following, or a
    /// transient "follow ended" note for ~5 s after the tracked cell is lost. Same slot on
    /// desktop + Android (just below the top bar).
    fn follow_badge(&mut self, ctx: &egui::Context) {
        if self
            .follow_notice
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed().as_secs() >= 5)
        {
            self.follow_notice = None;
        }
        let following = self.follow_cell.is_some();
        let text = if let Some((_, c, _)) = &self.follow_cell {
            format!("⌖ Following {}  ✕", c.id)
        } else if let Some((msg, _)) = &self.follow_notice {
            msg.clone()
        } else {
            return;
        };
        // Desktop: just under the menu bar. Android: below the top glass bar + chrome-hide EYE
        // button (which sits at inset_top + 66; see app/mobile.rs), so nothing stacks.
        let y = if crate::platform::phone_layout() {
            let inset_top = (ctx.content_rect().top() - ctx.viewport_rect().top()).max(0.0);
            inset_top + 116.0
        } else {
            // Below the whole control column, in the badge lane.
            crate::ui::style::lane_right_badge_y(CONTROL_BUTTONS)
        };
        egui::Area::new("follow_badge".into())
            .anchor(
                egui::Align2::RIGHT_TOP,
                egui::vec2(crate::ui::style::LANE_RIGHT_BADGE_X, y),
            )
            // Only the following state carries a button; the notice is a read-only badge and must
            // not occlude the map's pinch test.
            .interactable(following)
            .show(ctx, |ui| {
                let fill = if following {
                    egui::Color32::from_rgba_unmultiplied(40, 90, 150, 220)
                } else {
                    egui::Color32::from_black_alpha(160)
                };
                egui::Frame::new()
                    .fill(fill)
                    .corner_radius(4.0)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        if following {
                            let btn = egui::Button::new(
                                egui::RichText::new(&text).color(egui::Color32::WHITE),
                            )
                            .frame(false);
                            if ui
                                .add(btn)
                                .on_hover_text("Stop following this storm")
                                .clicked()
                            {
                                self.follow_cell = None;
                            }
                        } else {
                            ui.colored_label(egui::Color32::from_white_alpha(210), &text);
                        }
                    });
            });
        // Keep the notice's expiry ticking without input.
        if self.follow_notice.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    /// Reassemble the displayed overlay set from the fetched sources and current filters.
    fn rebuild_overlays(&mut self) {
        // Reference lines first, so every product draws over them.
        let mut v: Vec<GeoFeature> = self.boundaries.features().cloned().collect();
        if (1..=8).contains(&self.filters.outlook_day) {
            v.extend(
                self.outlook_features[(self.filters.outlook_day - 1) as usize]
                    .iter()
                    .cloned(),
            );
        }
        if self.filters.show_mds {
            // The discussions in effect at a scrubbed frame's time, or today's.
            let archived = self.arch_md_shown.and_then(|b| self.arch_mds.peek(&b));
            v.extend(archived.unwrap_or(&self.md_features).iter().cloned());
        }
        if self.filters.show_watches {
            v.extend(self.watch_features.iter().cloned());
        }
        if (1..=3).contains(&self.filters.wssi_day) {
            v.extend(self.wssi_features.iter().cloned());
        }
        if (1..=3).contains(&self.filters.ero_day) {
            v.extend(self.ero_features.iter().cloned());
        }
        if (1..=2).contains(&self.filters.fire_day) {
            v.extend(self.fire_features.iter().cloned());
        }
        if self.show_outages {
            v.extend(self.outage_features.iter().cloned());
        }
        if self.filters.show_alerts {
            for f in self.active_alert_features() {
                if self.filters.alert_cats[alerts::category(&f.title).index()] {
                    v.push(f.clone());
                }
            }
        }
        if self.show_probsevere {
            v.extend(self.probsevere.iter().cloned());
        }
        if self.show_tropical {
            if let Some(t) = &self.tropical {
                // Surge and wind field go under the cones: the cone is the headline, these are
                // the context it sits on.
                v.extend(t.surge.iter().cloned());
                v.extend(t.wind_radii.iter().cloned());
                v.extend(t.cones.iter().cloned());
            }
        }
        if self.show_aviation {
            v.extend(self.aviation_features.iter().cloned());
        }
        if self.show_tfr {
            // Shapes that failed to parse are kept as empty placeholders so they are not
            // refetched forever; they have nothing to draw.
            v.extend(
                self.tfr_features
                    .values()
                    .filter(|f| !f.rings.is_empty())
                    .cloned(),
            );
        }
        if self.show_fires {
            v.extend(self.fire_perims.iter().cloned());
        }
        if self.show_imported_gis {
            let style = self.settings.imported_gis_style;
            self.refresh_imported_colors();
            let colors = self.imported_colors.as_ref().map(|(_, c, _)| c);
            let src = &self.imported_marks.shape_src;
            let shown = |i: usize| {
                self.imported_shown
                    .as_ref()
                    .is_none_or(|m| src.get(i).and_then(|&s| m.get(s)).copied().unwrap_or(true))
            };
            let imported = self
                .imported_gis
                .iter()
                .enumerate()
                .filter(|&(i, _)| shown(i))
                .map(|(i, feature)| {
                    let mut feature = feature.clone();
                    let mut style = style;
                    if let Some(c) = colors.and_then(|c| *c.get(*src.get(i)?)?) {
                        style.color = c;
                    }
                    crate::gis_import::apply_style(&mut feature, style);
                    feature
                });
            // The list is painted in order: first is underneath (I4 z-order).
            if self.settings.imported_gis_below {
                v.splice(0..0, imported);
            } else {
                v.extend(imported);
            }
        }
        self.overlays = v;
        self.overlay_gen = self.overlay_gen.wrapping_add(1);
    }

    /// Keep the imported features' time filter (I5) in step with the view's time, rebuilding
    /// the overlays only when the set of valid features actually changes.
    fn sync_imported_time(&mut self) {
        let keys = (
            self.settings.imported_gis_time_start.clone(),
            self.settings.imported_gis_time_end.clone(),
        );
        if keys == (None, None) || self.imported_marks.props.is_empty() {
            if self.imported_shown.take().is_some() {
                self.rebuild_overlays();
            }
            return;
        }
        if self.imported_time.as_ref().is_none_or(|(k, _)| *k != keys) {
            let bounds = crate::gis_import::time_bounds(
                &self.imported_marks,
                keys.0.as_deref(),
                keys.1.as_deref(),
            );
            self.imported_time = Some((keys, bounds));
        }
        let Some((_, bounds)) = &self.imported_time else {
            return;
        };
        let t = self.view_target_time().unwrap_or_else(Utc::now);
        let shown = crate::gis_import::shown_at(bounds, t);
        if self.imported_shown.as_ref() != Some(&shown) {
            self.imported_shown = Some(shown);
            self.rebuild_overlays();
        }
    }

    /// Whether imported feature `src` is valid at the view's time (always, with no time filter).
    fn imported_valid(&self, src: Option<&usize>) -> bool {
        self.imported_shown
            .as_ref()
            .is_none_or(|m| src.and_then(|&s| m.get(s)).copied().unwrap_or(true))
    }

    /// Keep pane `idx`'s isosurface in step with its volume, moment and threshold: show the
    /// built surface for what is displayed, from the pane's loop cache, and start its build when
    /// there is none.
    fn sync_isosurface(&mut self, idx: usize, ctx: &egui::Context) {
        self.loop3d_jobs.drain(&mut self.loop3d);
        let Some((name, rev)) = self.shown_volume_key(idx) else {
            return;
        };
        let Some(key) = self.iso_key_for(idx, &name, rev) else {
            return;
        };
        if self.iso_mesh[idx].as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        if let Some(shells) = self.loop3d[idx].iso.get(&key) {
            self.iso_mesh[idx] = Some((key, Arc::clone(shells)));
            return;
        }
        let job = JobKey::Iso(idx, key.clone());
        if self.loop3d_jobs.came_up_empty(&job) {
            // Nothing crosses the threshold in this volume: show nothing, not the last one's.
            self.iso_mesh[idx] = Some((key, Arc::new(Vec::new())));
            return;
        }
        if !self.loop3d_jobs.wants(&job) {
            return;
        }
        let spec = self.iso_spec(idx);
        let Some(vol) = self.views[idx].volume.as_mut() else {
            return;
        };
        let sweeps = if spec.moment == Moment::Velocity {
            vol.velocity_tilts_dealiased()
        } else {
            vol.moment_tilts(spec.moment)
        };
        if sweeps.is_empty() {
            return;
        }
        self.loop3d_jobs.start(job, &self.spawner, ctx, move || {
            crate::loop3d::Built::Iso(crate::loop3d::build_iso(
                crate::loop3d::Sweeps::Binned { sweeps, mask: None },
                &spec,
            ))
        });
    }

    /// The displayed volume's name and live revision (0 for a volume loaded whole, so a frame
    /// built ahead from the download cache is found again when the playhead reaches it).
    fn shown_volume_key(&self, idx: usize) -> Option<(String, u64)> {
        let vol = self.views[idx].volume.as_ref()?;
        Some((vol.name.clone(), vol.revision()))
    }

    /// What pane `idx`'s Smooth volume of volume `name` is built from, when it shows one.
    /// `loop_quality` is the smaller grid used while the loop plays.
    fn smooth_key_for(
        &self,
        idx: usize,
        name: &str,
        rev: u64,
        loop_quality: bool,
    ) -> Option<SmoothKey> {
        let v = &self.views[idx];
        let state = &v.map_3d;
        if !state.enabled {
            return None;
        }
        let (moment, _) = state
            .representation
            .smooth_moment()
            .filter(|(m, _)| *m == v.moment)?;
        Some((
            name.to_string(),
            rev,
            moment,
            state.smooth_full_range,
            loop_quality,
            v.storm_motion_uv().map(|(u, n)| (u.to_bits(), n.to_bits())),
            match state.representation {
                Map3dRepresentation::SmoothProduct => Some(self.product_spec_key(idx)?),
                _ => None,
            },
        ))
    }

    /// User product `name` as pane `idx` would evaluate it: its formula, range and the site
    /// facts it can read, plus a key identifying all of that. `None` when it no longer exists, it
    /// does not parse, or it reduces a whole column (a vertical/layer function has no value at a
    /// single gate, so there is nothing to draw per gate).
    fn product_named(&self, idx: usize, name: &str) -> Option<(crate::loop3d::ProductSpec, u64)> {
        use std::hash::{Hash, Hasher};
        let v = &self.views[idx];
        let def = self.settings.udp_products.iter().find(|p| p.name == name)?;
        let expr = def.compile().ok().filter(|e| !e.uses_column())?;
        let antenna_altitude_m = v
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
            .map(|s| s.elevation_meters as f32 + wxdata::towers::tower_m(s.id) as f32);
        // A moment's colour table, when the product names one: drawn over that table's own span
        // unless it sets a range of its own.
        let table = def
            .palette
            .as_deref()
            .and_then(Moment::from_code)
            .map(|m| crate::colormap::effective_table(&self.palettes, m, self.settings.theme));
        let table_span = table.as_ref().and_then(|t| {
            let (lo, hi) = (t.stops.first()?.value, t.stops.last()?.value);
            (hi > lo).then_some((lo, hi))
        });
        let spec = crate::loop3d::ProductSpec {
            expr,
            range: def.range.or(table_span),
            table,
            env: wxdata::udp_volume::Env {
                antenna_altitude_m,
                freezing: self.freezing_for(idx).map(|(a, b)| (a as f32, b as f32)),
            },
        };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        def.expression.hash(&mut h);
        spec.range
            .map(|(a, b)| (a.to_bits(), b.to_bits()))
            .hash(&mut h);
        spec.env
            .freezing
            .map(|(a, b)| (a.to_bits(), b.to_bits()))
            .hash(&mut h);
        spec.env.antenna_altitude_m.map(f32::to_bits).hash(&mut h);
        def.palette.hash(&mut h);
        Some((spec, h.finish()))
    }

    /// The user-defined product pane `idx`'s 3D draws, when it draws one (ROADMAP_NEW H1).
    fn product_spec(&self, idx: usize) -> Option<crate::loop3d::ProductSpec> {
        let v = &self.views[idx];
        if v.map_3d.representation != Map3dRepresentation::SmoothProduct {
            return None;
        }
        self.product_named(idx, v.map_3d.product.as_ref()?)
            .map(|(s, _)| s)
    }

    /// What identifies pane `idx`'s 3D product build.
    fn product_spec_key(&self, idx: usize) -> Option<u64> {
        let v = &self.views[idx];
        if v.map_3d.representation != Map3dRepresentation::SmoothProduct {
            return None;
        }
        self.product_named(idx, v.map_3d.product.as_ref()?)
            .map(|(_, k)| k)
    }

    /// The user product pane `idx` shows on the map in place of its moment, when it shows one.
    fn map_product(&self, idx: usize) -> Option<(crate::loop3d::ProductSpec, u64)> {
        self.product_named(idx, self.views[idx].user_product.as_ref()?)
    }

    /// The colour scale and units a pane's legend shows for its user product, when it shows one.
    pub(crate) fn product_legend(&self, idx: usize) -> Option<(ColorTable, String, String)> {
        let v = &self.views[idx];
        let name = v.user_product.as_ref()?;
        let (lo, hi) = v.product_range?;
        let def = self.settings.udp_products.iter().find(|p| &p.name == name);
        let units = def.map(|p| p.units.clone()).unwrap_or_default();
        let table = def
            .and_then(|p| p.palette.as_deref())
            .and_then(Moment::from_code)
            .map(|m| crate::colormap::effective_table(&self.palettes, m, self.settings.theme))
            .unwrap_or_else(|| crate::colormap::ramp_table(lo, hi));
        Some((table, name.clone(), units))
    }

    /// What pane `idx`'s isosurface of volume `name` is built from, when it shows one.
    fn iso_key_for(&self, idx: usize, name: &str, rev: u64) -> Option<IsoKey> {
        let v = &self.views[idx];
        let state = &v.map_3d;
        if !state.enabled || !state.iso_enabled {
            return None;
        }
        let spec = self.iso_spec(idx);
        Some((
            name.to_string(),
            rev,
            spec.moment,
            spec.value.to_bits(),
            spec.smooth,
            spec.step.map(f32::to_bits),
            spec.storm_uv.map(|(u, n)| (u.to_bits(), n.to_bits())),
        ))
    }

    fn iso_spec(&self, idx: usize) -> crate::loop3d::IsoSpec {
        let v = &self.views[idx];
        let state = &v.map_3d;
        let mi = Moment::ALL.iter().position(|m| *m == v.moment).unwrap_or(0);
        crate::loop3d::IsoSpec {
            moment: v.moment,
            value: state.iso_values[mi],
            step: state.iso_nested.then_some(state.iso_steps[mi]),
            smooth: state.iso_smooth,
            max_dim: self.vol3d_max_dim,
            top_km: VOL3D_TOP_KM,
            storm_uv: v.storm_motion_uv(),
        }
    }

    fn smooth_spec(&self, idx: usize, loop_quality: bool) -> Option<crate::loop3d::SmoothSpec> {
        let state = &self.views[idx].map_3d;
        let (moment, invert) = state.representation.smooth_moment()?;
        Some(crate::loop3d::SmoothSpec {
            moment,
            invert,
            full_range: state.smooth_full_range,
            table: crate::colormap::effective_table(&self.palettes, moment, self.settings.theme),
            max_dim: self.vol3d_max_dim,
            max_voxels: if loop_quality {
                crate::loop3d::SMOOTH_LOOP_MAX_VOXELS
            } else {
                crate::loop3d::SMOOTH_MAX_VOXELS
            },
            top_km: VOL3D_TOP_KM,
            storm_uv: self.views[idx].storm_motion_uv(),
            product: match state.representation {
                Map3dRepresentation::SmoothProduct => Some(self.product_spec(idx)?),
                _ => None,
            },
        })
    }

    /// Recompute the imported features' colours when the colouring attribute changed.
    fn refresh_imported_colors(&mut self) {
        let Some(key) = self.settings.imported_gis_color_by.clone() else {
            self.imported_colors = None;
            return;
        };
        if self
            .imported_colors
            .as_ref()
            .is_some_and(|(k, _, _)| *k == key)
        {
            return;
        }
        let (colors, legend) = crate::gis_import::color_by(&self.imported_marks, &key);
        self.imported_colors = Some((key, colors, legend));
    }

    /// Approximate map view range in nautical miles (viewport height), for placefile thresholds.
    /// `// ponytail: coarse mercator estimate; fine for zoom-gating, not for measuring.`
    fn view_range_nmi(&self) -> f32 {
        let cam = &self.views[self.active].camera;
        let world_h = self.last_viewport.1 as f64 * cam.world_per_pixel();
        let s = (cam.center.1 * 2.0 - 1.0) * std::f64::consts::PI;
        let coslat = (1.0 / s.cosh()).max(0.05); // cos(lat) = sech(mercator y)
        (world_h * 40075.017 * coslat / 1.852) as f32
    }

    /// Placefile items currently visible (enabled, zoom threshold met, within time range), as
    /// `(item, opacity, loaded-placefile index)`. Iterated in `settings.placefiles` order, which
    /// is the paint order the Layer Manager reorders.
    fn visible_placefile_items(&self) -> Vec<(&wxdata::placefile::PlaceItem, f32, usize)> {
        crate::prof_scope!("visible_placefile_items");
        self.visible_placefile_iter().collect()
    }

    /// The same set, lazily — so a caller that only asks whether there *are* any does not build
    /// the whole list to find out.
    fn visible_placefile_iter(
        &self,
    ) -> impl Iterator<Item = (&wxdata::placefile::PlaceItem, f32, usize)> {
        let range = self.view_range_nmi();
        let now = Utc::now();
        // Configured placefiles in Layer-Manager order, then plugin output on top of them: a
        // plugin is something the user wrote for this session, so it should not be buried.
        let sources = self
            .settings
            .placefiles
            .iter()
            .map(|c| (std::borrow::Cow::Borrowed(c.url.as_str()), c.opacity))
            .chain(self.settings.plugins.iter().map(|p| {
                (
                    std::borrow::Cow::Owned(format!("plugin:{}", p.name)),
                    1.0f32,
                )
            }));
        sources.flat_map(move |(url, opacity)| {
            self.placefiles
                .iter()
                .enumerate()
                .find(|(_, lp)| lp.url == url)
                .filter(|(_, lp)| lp.enabled)
                .into_iter()
                .flat_map(move |(li, lp)| {
                    lp.pf
                        .items
                        .iter()
                        .filter(move |it| !(it.threshold_nmi > 0.0 && range > it.threshold_nmi))
                        .filter(move |it| it.time.is_none_or(|(a, b)| now >= a && now <= b))
                        .map(move |it| (it, opacity, li))
                })
        })
    }

    /// [`Self::placefile_labels`] memoised for the frame's inputs — it deep-clones every item's
    /// strings, and ran once per frame over every enabled placefile.
    fn placefile_labels_cached(&mut self) -> std::sync::Arc<[PlaceLabel]> {
        // Time is in the inputs because items have on/off windows and thresholds; a minute's
        // granularity is finer than any placefile's own cadence.
        let fingerprint: usize = self
            .placefiles
            .iter()
            .map(|p| p.pf.items.len() + usize::from(p.enabled))
            .sum::<usize>()
            + self.pf_icon_tex.len();
        let key = (
            fingerprint,
            chrono::Utc::now().timestamp() / 60,
            self.view_range_nmi() as i32,
        );
        if self
            .placefile_label_cache
            .as_ref()
            .is_none_or(|(k, _)| *k != key)
        {
            let labels: std::sync::Arc<[PlaceLabel]> = self.placefile_labels().into();
            self.placefile_label_cache = Some((key, labels));
        }
        self.placefile_label_cache
            .as_ref()
            .map(|(_, l)| l.clone())
            .unwrap_or_else(|| Vec::new().into())
    }

    /// Owned labels/markers for the visible placefile items (drawn by the egui painter).
    /// Icons resolve their sheet cell here, so the painter just blits a quad.
    fn placefile_labels(&self) -> Vec<PlaceLabel> {
        crate::prof_scope!("placefile_labels");
        use wxdata::placefile::PlaceKind;
        self.visible_placefile_items()
            .iter()
            .filter_map(|(it, opacity, li)| {
                let fade = |c: [u8; 4]| rgba32(c).gamma_multiply(*opacity);
                Some(match &it.kind {
                    PlaceKind::Text {
                        color,
                        pos,
                        text,
                        hover,
                    } => PlaceLabel {
                        color: fade(*color),
                        pos: *pos,
                        anchor: it.anchor,
                        hover: hover.clone(),
                        kind: PlaceLabelKind::Text(text.clone()),
                    },
                    PlaceKind::Icon {
                        color,
                        pos,
                        angle,
                        sheet,
                        hover,
                    } => PlaceLabel {
                        color: fade(*color),
                        pos: *pos,
                        anchor: it.anchor,
                        hover: hover.clone(),
                        kind: self
                            .sprite_for(*li, *sheet, *angle)
                            .unwrap_or(PlaceLabelKind::Marker),
                    },
                    _ => return None,
                })
            })
            .collect()
    }

    /// Resolve an icon's `(file, index)` against the placefile's sheets and the loaded textures.
    /// `None` whenever anything is missing — the caller falls back to a plain marker.
    fn sprite_for(
        &self,
        li: usize,
        sheet: Option<(u32, u32)>,
        angle: f32,
    ) -> Option<PlaceLabelKind> {
        let (file, index) = sheet?;
        let sh = self.placefiles.get(li)?.pf.icon_files.get(&file)?;
        let tex = self.pf_icon_tex.get(&sh.url)?.as_ref()?;
        let [tw, th] = tex.size();
        let (cols, rows) = (
            (tw as u32 / sh.icon_w).max(1),
            (th as u32 / sh.icon_h).max(1),
        );
        // Icon numbering is 1-based, left to right then top to bottom.
        let i = index.saturating_sub(1);
        if i >= cols * rows {
            return None;
        }
        let (cx, cy) = (i % cols, i / cols);
        let (u0, v0) = (cx * sh.icon_w, cy * sh.icon_h);
        let uv = egui::Rect::from_min_max(
            egui::pos2(u0 as f32 / tw as f32, v0 as f32 / th as f32),
            egui::pos2(
                (u0 + sh.icon_w) as f32 / tw as f32,
                (v0 + sh.icon_h) as f32 / th as f32,
            ),
        );
        Some(PlaceLabelKind::Sprite {
            tex: tex.id(),
            uv,
            size: egui::vec2(sh.icon_w as f32, sh.icon_h as f32),
            hot: egui::vec2(sh.hot_x as f32, sh.hot_y as f32),
            angle,
        })
    }

    /// Fetch + decode any icon sheet a loaded placefile references but we don't have yet, and
    /// upload arrivals. `// ponytail: no disk cache — sheets are a few KB and refetch on launch.`
    fn sync_pf_icons(&mut self, ctx: &egui::Context) {
        while let Ok((url, image)) = self.pf_icon_rx.try_recv() {
            let tex =
                ctx.load_texture(format!("pficon:{url}"), image, egui::TextureOptions::LINEAR);
            self.pf_icon_tex.insert(url, Some(tex));
        }
        let wanted: Vec<String> = self
            .placefiles
            .iter()
            .filter(|lp| lp.enabled)
            .flat_map(|lp| lp.pf.icon_files.values().map(|s| s.url.clone()))
            .filter(|u| !self.pf_icon_tex.contains_key(u))
            .collect();
        for url in wanted {
            // Insert the negative entry first: it doubles as the in-flight guard.
            self.pf_icon_tex.insert(url.clone(), None);
            let http = self.http.clone();
            let tx = self.pf_icon_tx.clone();
            let ctx2 = ctx.clone();
            self.spawner.spawn(async move {
                match fetch_icon_sheet(&http, &url).await {
                    Ok(image) => {
                        let _ = tx.send((url, image));
                        ctx2.request_repaint();
                    }
                    Err(e) => note_feed_error("Placefile icons", format!("{url}: {e}")),
                }
            });
        }
    }

    /// Re-tessellate the overlay when its set or the zoom bucket changed.
    fn sync_overlay(&mut self) {
        crate::prof_scope!("sync_overlay");
        // Asked lazily: on a frame with nothing to rebuild — which is most of them — this is the
        // only question, and building the whole visible list a second time to answer it was the
        // second-largest per-frame allocation in the pane.
        if self.overlays.is_empty() && self.visible_placefile_iter().next().is_none() {
            self.overlay_ready = false;
            return;
        }
        let zoom = self.views[self.active].camera.zoom;
        let bucket = (zoom * 2.0).round() as i32;
        // A pinch crosses several half-zoom buckets, and each crossing used to re-run lyon over
        // every overlay ring on the UI thread, mid-gesture. Deferred while a finger is down: the
        // frame after the release rebuilds to whatever bucket the gesture landed on, so the
        // resting state is the same one and the intermediate tessellations were never seen for
        // more than a frame anyway.
        //
        // A geometry change (`overlay_gen`) is not deferred — that is new data arriving, not the
        // camera moving, and it should appear when it lands.
        // Turning the globe on or off re-tessellates: its overlays are split to bend with it.
        let theme_changed =
            self.settings.theme != self.built_theme || self.settings.globe != self.built_globe;
        let imported_visible = self.settings.imported_gis_style.visible_at(zoom);
        let imported_flipped = imported_visible != self.built_imported_visible;
        if should_retess(
            self.gesture_live,
            self.overlay_gen != self.built_gen || theme_changed,
            bucket != self.built_zoom_bucket || theme_changed || imported_flipped,
        ) {
            self.built_imported_visible = imported_visible;
            let mut geom = overlay_build::build_with_theme_and_imported_width(
                &self.overlays,
                zoom,
                self.settings.theme,
                self.settings.imported_gis_style.rendered_stroke_width(),
                imported_visible,
            );
            let pf: Vec<(&wxdata::placefile::PlaceItem, f32)> = self
                .visible_placefile_iter()
                .map(|(it, op, _)| (it, op))
                .collect();
            overlay_build::append_placefiles_with_theme(&mut geom, &pf, zoom, self.settings.theme);
            // Outlook-sized fills bend with the globe instead of cutting chords through it.
            if self.settings.globe {
                crate::render::subdivide_long_triangles(
                    &mut geom.vertices,
                    &mut geom.indices,
                    crate::render::GLOBE_MAX_SPAN,
                    5,
                );
            }
            self.built_globe = self.settings.globe;
            self.overlay_ready = !geom.indices.is_empty();
            self.pending_overlay = Some(OverlayUpload {
                vertices: geom.vertices,
                indices: geom.indices,
            });
            self.built_gen = self.overlay_gen;
            self.built_zoom_bucket = bucket;
            self.built_theme = self.settings.theme;
        }
    }

    fn poll_messages(&mut self) {
        self.drain_feed_errors();
        while let Ok(msg) = self.msg_rx.try_recv() {
            let idx = msg.view();
            // LiveEnded must be handled even after a site change (to drop the stream handle).
            if matches!(msg, DataMsg::LiveEnded { .. }) {
                if let DataMsg::LiveEnded { view, gen, .. } = msg {
                    let current = self
                        .live_stream
                        .as_ref()
                        .is_some_and(|(v, _, g, _)| *v == view && *g == gen);
                    if current {
                        self.live_stream = None; // interval polling resumes automatically
                        if view < self.views.len() {
                            // Only the current stream may clear the pane's acquisition state.
                            self.views[view].live_progress = None;
                            self.views[view].live_progress_at = None;
                            self.views[view].live_retries = 0;
                            self.views[view].live_scan.stream_ended();
                        }
                    }
                }
                continue;
            }
            if idx >= self.views.len() || self.views[idx].site.as_deref() != Some(msg.site()) {
                continue; // view gone or its site changed since the fetch spawned
            }
            if let DataMsg::Live { gen, .. } | DataMsg::LiveProgress { gen, .. } = &msg {
                if !self
                    .live_stream
                    .as_ref()
                    .is_some_and(|(v, _, g, _)| *v == idx && g == gen)
                {
                    continue; // a superseded provider must never rewind this pane
                }
            }
            match msg {
                DataMsg::Volume {
                    view,
                    name,
                    time,
                    scan,
                    live_poll,
                    ..
                } => {
                    let scan = Arc::new(scan);
                    self.scan_cache.put(name.clone(), Arc::clone(&scan));
                    let v = &mut self.views[view];
                    // A completed fetch may belong to a cursor the user has since left. Keep
                    // the scan in cache for a later revisit, but never paint it over the frame
                    // now selected (including a late live poll after an archive scrub).
                    if !v.timeline.accepts_fetched_volume(&name, live_poll) {
                        v.loading = false;
                        continue;
                    }
                    let previous_provider = v.live_scan.provider.clone();
                    if live_poll
                        && v.timeline.following
                        && !v.live_scan.accept_volume(&name, time, Utc::now())
                    {
                        v.loading = false;
                        continue;
                    }
                    if live_poll && v.timeline.following {
                        if let Some(from) = previous_provider
                            .filter(|from| v.live_scan.provider.as_deref() != Some(from.as_str()))
                        {
                            let reason = "live stream unavailable; completed-volume polling";
                            log::info!(
                                target: "hookecho::radar_provider_manager",
                                "{}: provider switch {from} -> Completed-volume poll: {reason}; completed volumes only",
                                v.site.as_deref().unwrap_or("?"),
                            );
                            v.live_scan.set_switch_reason(reason);
                            v.live_render_started = None;
                            v.live_queue_timings =
                                Arc::new(crate::render::LiveQueueTimings::default());
                        }
                    }
                    let looping = v.timeline.live_looping();
                    // A newly-arrived live head (following): roll the day at UTC midnight, or grow
                    // the frame list so the loop window slides forward. A frame-fetch result for a
                    // scrubbed/loop-display frame is older than the head and isn't a new head.
                    //
                    // This is *not* the same question as `live_poll`: the bucket listing
                    // (`DataMsg::Frames`) can independently learn this exact name before this
                    // fetch completes, so "new to `t.frames`" and "came from the live poll" can
                    // disagree — both checks exist because each answers a different question.
                    let new_head = v.timeline.following
                        && v.site.as_deref().is_some_and(wxdata::sites::is_nexrad)
                        && {
                            let last_time = v.timeline.frames.last().and_then(|id| id.date_time());
                            if time.date_naive() != v.timeline.date {
                                v.timeline.date = time.date_naive(); // re-list fires via frames_key
                                true
                            } else if last_time.is_none_or(|t| time > t)
                                && v.timeline.frames.last().map(|id| id.name())
                                    != Some(name.as_str())
                            {
                                v.timeline.append_head(Identifier::new(name.clone()));
                                true
                            } else {
                                false
                            }
                        };
                    log::debug!(
                        target: "hookecho::live_sweep",
                        "{}: volume loaded {name} ({time}), live_poll={live_poll}",
                        v.site.as_deref().unwrap_or("?"),
                    );
                    // While looping, the playhead frame owns the display: a new head is only
                    // appended, and a poll that returns the head already listed must not paint
                    // it over an older loop frame either (it did, every poll, so a live loop kept
                    // snapping to its newest scan between frames). Every other case updates the
                    // displayed volume.
                    if !looping
                        || (!new_head && v.timeline.current().is_some_and(|id| id.name() == name))
                    {
                        v.show_volume(scan, name, time);
                    }
                    // A stale in-flight poll completing after the user scrubbed away from live is
                    // simply not recorded — `following` is checked at the only point that matters,
                    // when the result actually lands, rather than trusted from when the fetch
                    // started.
                    if live_poll && v.timeline.following {
                        v.last_live_arrival = Some((Utc::now(), time));
                    }
                    v.loading = false;
                    v.error = None;
                    v.clamp_tilt();
                    v.clamp_moment();
                    self.pane_shown.remove(&view);
                    self.scan_chime(view, time);
                }
                DataMsg::Frames {
                    view,
                    site,
                    date,
                    frames,
                } => {
                    let v = &mut self.views[view];
                    if v.timeline.date == date && v.site.as_deref() == Some(site.as_str()) {
                        v.timeline.listing = false;
                        v.timeline.set_frames(frames, (site, date));
                        self.pane_shown.remove(&view);
                    }
                }
                DataMsg::Live {
                    view,
                    name,
                    time,
                    received_at,
                    radial_coverage,
                    scan,
                    changed,
                    retries,
                    decode_time,
                    ..
                } => {
                    let v = &mut self.views[view];
                    if v.timeline.playing {
                        continue; // looping pane owns its displayed frame (cf. Volume above)
                    }
                    if !v.live_scan.accept_volume(&name, time, Utc::now()) {
                        continue; // late chunk from an older volume cannot reverse display time
                    }
                    if let Some(coverage) = radial_coverage {
                        v.live_scan.observe_radials(coverage, Utc::now());
                    }
                    v.live_render_started = received_at;
                    log::debug!(
                        target: "hookecho::live_sweep",
                        "{}: live chunk merged into {name} ({time}), {} tilt(s) changed, \
                         decode {:.0} ms, {retries} retr{}",
                        v.site.as_deref().unwrap_or("?"),
                        changed.len(),
                        decode_time.as_secs_f64() * 1000.0,
                        if retries == 1 { "y" } else { "ies" },
                    );
                    match &mut v.volume {
                        Some(vol) => vol.apply_live(scan, name, time, &changed),
                        None => v.volume = Some(Volume::from_live(scan, name, time)),
                    }
                    v.live_scan_revision = v.live_scan_revision.wrapping_add(1);
                    // Phase B5's "follow newest low-level cut": jump to the lowest tilt the
                    // instant a sweep there lands, including a SAILS/MRLE mid-volume rescan,
                    // rather than waiting for the tilt already selected or the volume as a whole.
                    if v.follow_lowest_cut
                        && v.timeline.following
                        && v.volume
                            .as_ref()
                            .is_some_and(|vol| vol.changed_includes_lowest_tilt(&changed))
                    {
                        v.tilt = 0;
                    }
                    // Follow the live sweep: when a new sweep's first chunk has merged (so its
                    // tilt exists to show), move to it — once per sweep, so a tilt picked by hand
                    // mid-sweep holds until the radar starts the next one.
                    v.follow_sweep();
                    v.last_live_arrival = Some((Utc::now(), time));
                    // `LiveProgress` immediately precedes this partial merge. Keep it: the
                    // scrubber and 2D sweep bar need to describe/animate the chunk now on screen.
                    // Stream end and site changes clear it, so it cannot linger indefinitely.
                    v.live_retries = retries;
                    v.last_decode_time = Some(decode_time);
                    let now = Utc::now();
                    v.live_history.push_back((
                        now,
                        (now - time).num_milliseconds() as f32 / 1000.0,
                        decode_time.as_secs_f32() * 1000.0,
                    ));
                    while v.live_history.len() > crate::view::LIVE_HISTORY {
                        v.live_history.pop_front();
                    }
                    v.loading = false;
                    v.error = None;
                    v.clamp_tilt();
                    v.clamp_moment();
                    // A healthy stream pushes the poll deadline forward — this line IS the
                    // fallback: if the stream dies, interval polling resumes on schedule.
                    v.last_poll = Some(Instant::now());
                    self.pane_shown.remove(&view);
                    self.scan_chime(view, time);
                }
                DataMsg::UpToDate { view, .. } => self.views[view].loading = false,
                DataMsg::Prefetched {
                    view, name, scan, ..
                } => {
                    if view < self.views.len() {
                        self.remember_light(view, &name, &scan);
                    }
                    self.scan_cache.put(name, Arc::new(scan));
                }
                DataMsg::Error { view, err, .. } => {
                    let v = &mut self.views[view];
                    v.loading = false;
                    // The newest archive volume is published while the radar is still writing it,
                    // so the head can briefly lack its VCP message. That is a "not finished yet",
                    // not a failure: the next poll gets a complete file a minute later. Showing a
                    // red chip for it left the map looking broken while nothing was wrong.
                    if err.contains("missing coverage pattern") {
                        log::debug!("head volume not complete yet: {err}");
                    } else {
                        v.error = Some(err);
                    }
                }
                DataMsg::LiveProgress { view, progress, .. } => {
                    self.views[view].live_progress = Some(progress);
                    self.views[view].live_progress_at = Some(Instant::now());
                    self.views[view].live_scan.progress(progress, Utc::now());
                }
                DataMsg::LiveEnded { .. } => unreachable!("handled above"),
            }
        }
    }

    fn apply_action(&mut self, action: BindableAction, ctx: &egui::Context) {
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

    /// Streamer/OBS auto-tour: every ~12 s, fly the active camera to the next active-warning
    /// centroid (highest-severity first), cycling. No-op with no warnings in the feed.
    fn drive_obs_tour(&mut self) {
        if !self.obs_tour {
            return;
        }
        if self
            .obs_tour_last
            .is_some_and(|t| t.elapsed().as_secs() < 12)
        {
            return;
        }
        // Centroids of active warning polygons, tornado/severe first.
        let mut targets: Vec<(u8, f64, f64)> = self
            .alert_features
            .iter()
            .filter(|f| f.kind == overlay::FeatureKind::Warning)
            .filter_map(|f| {
                let (w, s, e, n) = f.bbox()?;
                let sev = if f.title.to_lowercase().contains("tornado") {
                    0
                } else {
                    1
                };
                Some((sev, (w + e) / 2.0, (s + n) / 2.0))
            })
            .collect();
        if targets.is_empty() {
            return;
        }
        targets.sort_by_key(|t| t.0);
        self.obs_tour_last = Some(Instant::now());
        self.obs_tour_idx = (self.obs_tour_idx + 1) % targets.len();
        let (_, lon, lat) = targets[self.obs_tour_idx];
        let cam = &mut self.views[self.active].camera;
        cam.center = crate::render::mercator::lonlat_to_world(lon, lat);
        cam.zoom = cam.zoom.max(8.5);
    }

    /// Force-refresh the active view: re-list the day's volumes, and refetch the head volume
    /// when following live.
    fn trigger_reload(&mut self, ctx: &egui::Context) {
        let idx = self.active;
        self.views[idx].timeline.frames_key = None; // force a fresh listing
        let site = self.views[idx].site.clone();
        if self.views[idx].timeline.following {
            if let Some(s) = site {
                self.views[idx].loading = true;
                self.views[idx].last_poll = Some(Instant::now());
                self.spawn_fetch(idx, s, None, ctx.clone());
            }
        }
    }

    /// Per-frame per-pane: react to site changes, keep the timeline current, and (for the active
    /// pane) manage the live stream. Each pane fetches its own volume via its view index.
    fn sync_pane(&mut self, idx: usize, ctx: &egui::Context) {
        // Site change: clear the old volume, recenter, and (if a real site) refetch.
        let site_changed = self.views[idx].site != self.views[idx].loaded_site;
        if site_changed {
            let v = &mut self.views[idx];
            v.loaded_site = v.site.clone();
            v.volume = None;
            v.forget_recent();
            v.moments_seen = [false; Moment::ALL.len()];
            v.live_progress = None;
            v.live_progress_at = None;
            v.live_scan.reset(v.site.clone());
            v.live_render_started = None;
            v.live_queue_timings = Arc::new(crate::render::LiveQueueTimings::default());
            v.error = None;
            // Clear a stuck in-flight flag: if the previous site's fetch is still running when the
            // site changes, its result is dropped on arrival (site mismatch) without clearing
            // `loading`, which would then block the new site's fetch forever ("no volume").
            v.loading = false;
            // The old site's frame list is not this site's. Left in place it is both what the
            // scrub path displays and what it downloads until the new listing lands — i.e. the
            // wrong radar's volumes at an index that means nothing here. A scrubbed pane keeps
            // the time it was looking at; a live one just goes back to the head.
            // ...unless a deep link, an event or a replay bundle already asked for an instant:
            // that target is the whole point of the jump, and overwriting it with the frame the
            // pane happened to be showing sent every Event Library entry to the wrong day.
            if !v.timeline.following && v.timeline.seek_target.is_none() {
                v.timeline.seek_target = v.timeline.current().and_then(|id| id.date_time());
            }
            v.timeline.frames.clear();
            v.timeline.playhead = 0;
            match &v.site {
                // ...unless a deep link already aimed the camera at something specific.
                Some(_) if std::mem::take(&mut v.camera_placed) => {}
                Some(s) => ui::site_dialog::center_on_site(&mut v.camera, s),
                None => {
                    self.pane_shown.remove(&idx);
                }
            }
            // Storm cells follow the active pane's site: drop the old ones (and any open old-site
            // storm popup / trend history — the ring-click path in try_pick_site does the same) and
            // refetch.
            if idx == self.active {
                self.storm_cells.clear();
                self.cells_site = None;
                self.cell_trends.clear();
                self.cell_popup = None;
                if let Some(site) = self.views[idx].site.clone() {
                    let history = self.cells_history_site.as_deref() != Some(site.as_str());
                    self.spawn_overlay(ctx, OverlaySource::Cells(site, history));
                }
            }
        }

        // Advance playback (if playing) then reconcile the displayed volume with the timeline.
        // Past the scan cache a loop plays light frames (`app::long_loop`), a few percent of a
        // volume each, so the phone and the browser loop as long as the desktop's memory allows.
        self.views[idx].timeline.live_window = self
            .settings
            .live_loop_frames
            .clamp(1, long_loop::MAX_LOOP_FRAMES);

        // The browser demo opens playing. A single frozen frame is indistinguishable from a broken
        // map to someone who has never seen this app, and the last fifteen minutes is what makes
        // radar readable — you cannot tell which way a storm is moving from a still.
        //
        // Waits for two frames rather than starting on one, so the first thing the visitor sees
        // move is an actual loop and not a one-frame stutter. `backfill_loop_frames` is what
        // fetches the tail; once playing, ordinary prefetch takes over.
        if self.autoplay_pending {
            let tl = &self.views[idx].timeline;
            if tl.following && !tl.playing {
                let window = tl.live_window.max(1);
                let start = tl.frames.len().saturating_sub(window);
                let ready = tl.frames[start..]
                    .iter()
                    .filter(|id| self.scan_cache.contains(&id.name().to_string()))
                    .count();
                if ready >= 2 {
                    log::info!("loop: playing {ready}/{window} frames");
                    self.views[idx].timeline.toggle_play();
                    self.autoplay_pending = false;
                } else {
                    self.backfill_loop_frames(idx, ctx);
                }
            }
        }
        // Hold the playhead while the next frame is still downloading. Advancing on the wall clock
        // regardless meant playback skipped frames it hadn't got yet and the loop read as juddery;
        // waiting reads as buffering, which is what it is. Only while there's a fetch to wait for,
        // so a permanently-failed frame can't stall the loop.
        let next_pending = {
            let tl = &self.views[idx].timeline;
            tl.playing
                && tl
                    .frames
                    .get(tl.playhead + 1)
                    .map(|id| id.name().to_string())
                    .is_some_and(|n| {
                        !self.scan_cache.contains(&n)
                            && !(self.light_ok(idx) && self.light_frame(idx, &n).is_some())
                            && book(&self.prefetching).get(&n).is_some_and(|at| {
                                // Bounded: a fetch that never answers must not park the loop.
                                at.elapsed() < std::time::Duration::from_secs(8)
                            })
                    })
        };
        // Likewise while its 3D is still being built, so the 3D view plays frame for frame
        // rather than showing one scan's volume under the next scan's time (ROADMAP_NEW H8).
        if !next_pending && !self.next_frame_3d_pending(idx) {
            self.views[idx].timeline.tick();
        }
        // Playback paces itself rather than riding whatever the idle heartbeat happens to give
        // it: ask for a repaint exactly when the next frame is due.
        if let Some(dt) = self.views[idx].timeline.time_to_next_frame() {
            ctx.request_repaint_after(dt);
        }
        self.sync_timeline(idx, ctx, site_changed);

        // Live streaming is limited to the active pane; others poll their head.
        if idx == self.active {
            self.manage_stream(ctx);
        }
    }

    /// Reconcile the frame listing and the displayed volume with the timeline: keep the
    /// listing current, poll the live head while following, or load the scrubbed frame.
    fn sync_timeline(&mut self, idx: usize, ctx: &egui::Context, site_changed: bool) {
        if !crate::platform::activity::is_active() {
            return; // backgrounded: no listings, no head polls, no downloads
        }
        // (Re)list volumes when the site or selected date changed.
        let (site, date, following, need_list, listing) = {
            let v = &self.views[idx];
            let key = v.site.clone().map(|s| (s, v.timeline.date));
            // TDWRs and DWD radars have no archive to list; their timeline stays empty and
            // always live.
            let need = v.site.as_deref().is_some_and(wxdata::sites::is_nexrad)
                && v.timeline.frames_key != key;
            (
                v.site.clone(),
                v.timeline.date,
                v.timeline.following,
                need,
                v.timeline.listing,
            )
        };
        if let Some(s) = &site {
            if need_list && !listing {
                self.views[idx].timeline.listing = true;
                self.spawn_list_frames(idx, s.clone(), date, ctx.clone());
            }
        }

        let looping = self.views[idx].timeline.live_looping();
        if following {
            // Live head: poll for the newest volume. While looping, the displayed volume is a
            // middle loop frame, so compare against the newest *frame* (not the shown volume) to
            // decide whether the head advanced — otherwise every poll re-downloads the head.
            let (site, current_name, due) = {
                let v = &self.views[idx];
                let due = v
                    .last_poll
                    .is_none_or(|t| t.elapsed().as_secs() >= self.poll_interval_secs());
                let current_name = if looping {
                    v.timeline.frames.last().map(|id| id.name().to_string())
                } else {
                    v.volume.as_ref().map(|vol| vol.name.clone())
                };
                (v.site.clone(), current_name, due)
            };
            if site.is_some() && !self.views[idx].loading && (site_changed || due) {
                if let Some(s) = site {
                    self.views[idx].loading = true;
                    self.views[idx].last_poll = Some(Instant::now());
                    self.spawn_fetch(idx, s, current_name, ctx.clone());
                }
            }
        }
        // A playing live loop owns its display at the head frame too: live chunks are not
        // merged while it plays, so without this the newest frame kept showing the one before it.
        if !following || looping || self.views[idx].timeline.playing {
            // Archive / loop: display the volume at the playhead (cache hit is synchronous).
            let target = self.views[idx].timeline.current().map(|id| {
                (
                    id.name().to_string(),
                    id.date_time().unwrap_or_else(Utc::now),
                    id.clone(),
                )
            });
            if let Some((name, time, id)) = target {
                let light_ok = self.light_ok(idx);
                // A light loop frame on screen stands until the loop stops (or needs the whole
                // volume for 3D, Max or Clean); then the full volume replaces it.
                let need = match self.views[idx].volume.as_ref() {
                    Some(v) if v.name == name => v.light && !light_ok,
                    _ => true,
                };
                if need {
                    if let Some(scan) = self.scan_cache.get(&name).map(Arc::clone) {
                        wxdata::stats::bump(wxdata::stats::Counter::ScanCacheHits);
                        self.remember_light(idx, &name, &scan);
                        let v = &mut self.views[idx];
                        v.show_volume(scan, name, time);
                        v.loading = false;
                        v.error = None;
                        v.clamp_tilt();
                        v.clamp_moment();
                        self.pane_shown.remove(&idx);
                    } else if let Some(light) = self.light_frame(idx, &name).filter(|_| light_ok) {
                        let v = &mut self.views[idx];
                        v.show_light_volume(light, name, time);
                        v.loading = false;
                        v.error = None;
                        self.pane_shown.remove(&idx);
                    } else if !self.views[idx].loading {
                        wxdata::stats::bump(wxdata::stats::Counter::ScanCacheMisses);
                        let s = self.views[idx].site.clone().unwrap_or_default();
                        self.views[idx].loading = true;
                        self.spawn_frame_fetch(idx, s, id, ctx.clone());
                    }
                }
                // Pull neighbouring frames in behind the playhead, on their own in-flight book so
                // they never compete with the frame being shown or with the head poll. Without
                // this, playback is a serial download per frame with the loop stalled between —
                // and scrubbing, which runs this same path paused, was a cold download per step.
                self.prefetch_frames(idx, ctx);
            }
        }
    }

    /// Debug builds only: frame time and tile-queue depth in the top-left corner. `dumpsys
    /// gfxinfo` and logcat cover most of what this shows, but neither says which frames were slow
    /// while a gesture was in progress.
    ///
    /// ponytail: an unsmoothed millisecond readout, no history graph. Add one if a single number
    /// stops being enough to tell a stutter from a stall.
    #[cfg(debug_assertions)]
    fn frame_time_overlay(&mut self, ctx: &egui::Context) {
        let (dt, pinching) = ctx.input(|i| (i.unstable_dt * 1000.0, i.multi_touch().is_some()));
        let text = format!(
            "{dt:.1} ms ({:.0} fps)  z{:.2}{}",
            1000.0 / dt.max(0.001),
            self.views[self.active].camera.zoom,
            if pinching { "  pinch" } else { "" }
        );
        egui::Area::new(egui::Id::new("frame_time_overlay"))
            .anchor(egui::Align2::LEFT_TOP, egui::vec2(6.0, 6.0))
            .order(egui::Order::Tooltip)
            .interactable(false)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(text)
                        .monospace()
                        .size(11.0)
                        .color(egui::Color32::from_rgb(120, 255, 120))
                        .background_color(egui::Color32::from_black_alpha(160)),
                );
            });
    }

    /// Fetch the two frames after the playhead into the scan cache, so playback isn't a serial
    /// download-per-frame. At most two are in flight; each task gives its slot back when it ends,
    /// and anything still booked well past the fetch deadline is aged out.
    fn prefetch_frames(&mut self, idx: usize, ctx: &egui::Context) {
        // Longer than a volume fetch is allowed to take, so this only ever reaps entries whose
        // task is genuinely gone. At 15 s it reaped entries whose download was still running,
        // and every tick after that started the same download again.
        book(&self.prefetching)
            .retain(|_, at| at.elapsed() < VOLUME_TIMEOUT + std::time::Duration::from_secs(15));
        if book(&self.prefetching).len() >= MAX_PREFETCH_INFLIGHT {
            return;
        }
        let Some(site) = self.views[idx].site.clone() else {
            return;
        };
        let tl = &self.views[idx].timeline;
        let light_ok = self.light_ok(idx);
        let wanted: Vec<Identifier> = if light_ok {
            // A long loop: the frames playback reaches next, in its own order and well ahead,
            // skipping those already kept light, so the first pass fills while it plays.
            crate::loop3d::upcoming(tl, LONG_LOOP_LOOKAHEAD)
                .into_iter()
                .filter_map(|i| tl.frames.get(i))
                .filter(|id| self.light_frame(idx, id.name()).is_none())
                .cloned()
                .collect()
        } else {
            prefetch_offsets(tl.playing)
                .iter()
                .filter_map(|d| tl.frames.get(tl.playhead.checked_add_signed(*d)?))
                .cloned()
                .collect()
        };
        for id in wanted {
            if book(&self.prefetching).len() >= MAX_PREFETCH_INFLIGHT {
                break;
            }
            self.spawn_prefetch(idx, id, &site, ctx);
        }
    }

    /// Start one frame download into the scan cache, unless it is already cached or in flight.
    fn spawn_prefetch(&mut self, idx: usize, id: Identifier, site: &str, ctx: &egui::Context) {
        let name = id.name().to_string();
        if self.scan_cache.contains(&name) || book(&self.prefetching).contains_key(&name) {
            return;
        }
        book(&self.prefetching).insert(name, Instant::now());
        let tx = self.msg_tx.clone();
        let (site, ctx) = (site.to_string(), ctx.clone());
        let view = idx;
        let slot = self.prefetching.clone();
        self.spawner.spawn(async move {
            let name = id.name().to_string();
            let fetched = wxdata::task::timeout(
                VOLUME_TIMEOUT,
                crate::volume::fetch(id, true, crate::paths::cache_dir()),
            )
            .await
            .unwrap_or_else(Err);
            match fetched {
                Ok(scan) => {
                    let _ = tx.send(DataMsg::Prefetched {
                        view,
                        site,
                        name: name.clone(),
                        scan,
                    });
                }
                Err(e) => log::debug!("prefetch failed: {e}"),
            }
            // Whatever happened, this slot is free: the message above may be dropped as stale
            // (the site changed under it) and the book must not depend on it arriving.
            book(&slot).remove(&name);
            ctx.request_repaint();
        });
    }

    /// Fill the loop window *backwards* from the head, for the browser's opening auto-play.
    ///
    /// Ordinary prefetch only looks ahead of the playhead, which is the right thing once a loop is
    /// running and useless before one starts: at the live head there is nothing ahead. This walks
    /// back from the newest frame instead, under the same in-flight budget, so the visitor's first
    /// volume is the current one and the recent past fills in behind it.
    fn backfill_loop_frames(&mut self, idx: usize, ctx: &egui::Context) {
        book(&self.prefetching)
            .retain(|_, at| at.elapsed() < VOLUME_TIMEOUT + std::time::Duration::from_secs(15));
        if book(&self.prefetching).len() >= MAX_PREFETCH_INFLIGHT {
            return;
        }
        let Some(site) = self.views[idx].site.clone() else {
            return;
        };
        let tl = &self.views[idx].timeline;
        let window = tl.live_window.max(1);
        let start = tl.frames.len().saturating_sub(window);
        let tail: Vec<Identifier> = tl.frames[start..].iter().rev().cloned().collect();
        for id in tail {
            if book(&self.prefetching).len() >= MAX_PREFETCH_INFLIGHT {
                break;
            }
            self.spawn_prefetch(idx, id, &site, ctx);
        }
    }

    /// Download a specific archive volume (a scrubbed timeline frame), routed to `view_idx`.
    fn spawn_frame_fetch(&self, view_idx: usize, site: String, id: Identifier, ctx: egui::Context) {
        let tx = self.msg_tx.clone();
        self.spawner.spawn(async move {
            let name = id.name().to_string();
            let time = id.date_time().unwrap_or_else(Utc::now);
            // The `Err` arm is what matters here: it clears the pane's `loading` flag. Without a
            // deadline a swallowed request never took either arm, and a pane stuck on `loading`
            // stops polling for the rest of the session.
            let fetched = wxdata::task::timeout(
                VOLUME_TIMEOUT,
                crate::volume::fetch(id, true, crate::paths::cache_dir()),
            )
            .await
            .unwrap_or_else(Err);
            let msg = match fetched {
                Ok(scan) => DataMsg::Volume {
                    view: view_idx,
                    site,
                    name,
                    time,
                    scan,
                    live_poll: false,
                },
                Err(e) => DataMsg::Error {
                    view: view_idx,
                    site,
                    err: e.to_string(),
                },
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    /// List the archive volumes for `site` on `date` (timeline frames).
    fn spawn_list_frames(
        &self,
        view_idx: usize,
        site: String,
        date: NaiveDate,
        ctx: egui::Context,
    ) {
        let tx = self.msg_tx.clone();
        self.spawner.spawn(async move {
            match level2::list_volumes(&site, date).await {
                Ok(frames) => {
                    let _ = tx.send(DataMsg::Frames {
                        view: view_idx,
                        site,
                        date,
                        frames,
                    });
                    ctx.request_repaint();
                }
                Err(e) => note_feed_error("Archive frame list", format!("{site} {date}: {e}")),
            }
        });
    }

    /// Volumes folded into the trail per UI frame; see [`Self::advance_trail`].
    const TRAIL_FOLDS_PER_FRAME: usize = 3;

    /// Radar upload for pane `idx`, binning the shared volume in `data` (usually the active pane)
    /// with pane `idx`'s product/tilt. Returns `(upload_when_changed, draw_radar)`; the pane's GPU
    /// buffer persists, so `None` means "reuse what's uploaded". Caches per pane via `pane_shown`.
    fn pane_radar(&mut self, idx: usize, data: usize) -> (Option<RadarUpload>, bool) {
        crate::prof_scope!("pane_radar");
        let has_volume = self.views[data].volume.is_some();
        if !self.views[idx].show_radar || !has_volume {
            self.pane_shown.remove(&idx);
            self.pane_lut.remove(&idx);
            self.scan_age_rings.remove(&idx);
            return (None, false);
        }
        if !self.show_scan_age {
            self.scan_age_rings.clear();
        }
        let count = self.views[data].elevation_count();
        self.views[idx].clamp_tilt_to(&count);
        let (moment, tilt, threshold, smooth, storm_uv) = {
            let v = &self.views[idx];
            (
                v.moment,
                v.tilt,
                v.active_threshold(),
                v.smooth,
                v.storm_motion_uv(),
            )
        };
        // The pane's product list is the union over every volume from this site, so a single frame
        // in a loop can lack the selected moment (a legacy volume, a split cut that hasn't arrived).
        // Draw what this volume does have rather than blanking the radar for that frame.
        // Caller resolved `data` to a view that has a volume; a "wait" is the honest answer if
        // that stops being true rather than taking the frame down.
        let Some(have) = self.views[data].volume.as_ref().map(|v| v.moments) else {
            return (None, true);
        };
        let moment = if have[moment.index()] {
            moment
        } else {
            match Moment::ALL.into_iter().find(|m| have[m.index()]) {
                Some(m) => m,
                None => return (None, true), // nothing decodable yet: keep the last image up
            }
        };
        let storm_uv = if moment == Moment::Velocity {
            storm_uv
        } else {
            None
        };
        let Some(name) = self.views[data].volume.as_ref().map(|v| v.name.clone()) else {
            return (None, true);
        };
        // C2: on the pane being worked in, the extremum trail stands in for the sweep. The tag
        // rides in the shown-image key so the upload repeats as the trail grows.
        let trail_tag = if self.filters.show_trail && idx == self.active {
            self.advance_trail(data, moment, tilt)
        } else {
            if !self.filters.show_trail {
                self.trail = None;
                self.trail_more = false;
            }
            None
        };
        let name = match &trail_tag {
            Some(tag) => format!("{name}\u{1}{tag}"),
            None => name,
        };
        // Max reflectivity: the column maximum over every tilt stands in for the one tilt.
        let column_max = self.views[idx].column_max && moment == Moment::Reflectivity;
        // Reflectivity X: the same with non-weather echo removed.
        let clean = self.views[idx].clean_reflectivity && moment == Moment::Reflectivity;
        let name = match (column_max, clean) {
            (true, true) => format!("{name}\u{2}maxclean"),
            (true, false) => format!("{name}\u{2}max"),
            (false, true) => format!("{name}\u{2}clean"),
            (false, false) => name,
        };
        // A user product stands in for the moment, worked out on the shown tilt. Picking another
        // moment takes it off.
        if self.views[idx].user_product.is_some()
            && self.views[idx].moment != self.views[idx].product_moment
        {
            self.views[idx].user_product = None;
            self.views[idx].product_range = None;
        }
        let product = self.map_product(idx);
        let name = match &product {
            Some((_, key)) => format!("{name}\u{2}udp{key:x}"),
            None => name,
        };
        let (threshold, storm_uv) = if product.is_some() {
            (None, None) // both are in the moment's units, which a product does not share
        } else {
            (threshold, storm_uv)
        };
        let strict_current = self.settings.live_sweep_mode
            == crate::settings::LiveSweepMode::StrictCurrentSweep
            && self.views[data].timeline.following
            && !self.views[data].timeline.playing
            && self.views[data]
                .volume
                .as_ref()
                .is_some_and(Volume::is_live_partial)
            && trail_tag.is_none()
            && !column_max
            && product.is_none();
        // Bilinear sampling can pull a valid current-pass gate across an azimuth whose old
        // row was cleared. Strict mode uses nearest sampling so the masked sector stays empty.
        let smooth = smooth && !strict_current;
        let uv_key = storm_uv.map(|(e, n)| (e.to_bits(), n.to_bits()));
        // Dealiasing only applies to Doppler velocity, and only where it is actually folded:
        // a TDWR's Level 3 velocity is already unfolded before it leaves the radar.
        let dealias = self.settings.dealias_velocity
            && moment == Moment::Velocity
            && !self.views[idx]
                .site
                .as_deref()
                .is_some_and(wxdata::tdwr::is_tdwr);
        let key: ShownKey = (
            name,
            moment,
            tilt,
            threshold,
            smooth,
            uv_key,
            dealias,
            self.settings.precip_tint.then_some(self.precip_flag_gen),
            strict_current,
        );
        // Flash a range: on its bright half-beats the band is painted white. Only the colour table
        // changes, so the beat costs a 3 KB table write, not a sweep upload.
        let flash = self.views[idx].flash_ranges[moment.index()]
            .filter(|_| flash_beat_on() && product.is_none());
        let flash_tag = flash.map_or(0u64, |(lo, hi)| {
            (u64::from(lo.to_bits()) << 32 | u64::from(hi.to_bits())) | 1
        });
        let lut_gen = self
            .palettes
            .gen
            .wrapping_add(if crate::theme::is_high_contrast(self.settings.theme) {
                0x9e37_79b9_7f4a_7c15
            } else {
                0
            })
            .wrapping_add(flash_tag.wrapping_mul(0x2545_f491_4f6c_dd1d));
        // Same sweep already up: the only thing left that can differ is the color table, and
        // that is a 3 KB write into the texture already bound.
        let lut_only = self.pane_shown.get(&idx) == Some(&key);
        // Unless the scan-age ring was just switched on: it is read off the sweep as it is binned,
        // and this sweep was binned before anyone asked.
        let ring_owed = self.show_scan_age && !self.scan_age_rings.contains_key(&idx);
        if lut_only && self.pane_lut.get(&idx) == Some(&lut_gen) && !ring_owed {
            return (None, true);
        }
        let mut table_owned =
            crate::colormap::effective_table(&self.palettes, moment, self.settings.theme);
        if let Some((lo, hi)) = flash {
            table_owned = crate::colormap::highlight(&table_owned, lo, hi, FLASH_RGBA);
        }
        let table = &table_owned;
        // Cheap handle taken before the volume is borrowed mutably below.
        let precip = (self.settings.precip_tint
            && self.mrms_ready(crate::render::FieldLayer::PrecipType))
        .then(|| self.precip_flag_grid.clone())
        .flatten();
        let trail_acc = trail_tag
            .as_ref()
            .and(self.trail.as_ref())
            .and_then(|t| t.acc.as_ref());
        // Read off the sweep in hand rather than fetched later: the paint pass only has a shared
        // borrow of the volume, and binning needs a mutable one.
        let want_age = self.show_scan_age;
        let mut ring: Option<ScanAgeRing> = None;
        let mut product_range: Option<(f32, f32)> = None;
        let upload = if let Some(acc) = trail_acc {
            Ok::<_, anyhow::Error>(to_upload(
                acc,
                table,
                threshold,
                smooth,
                storm_uv,
                precip.as_deref(),
                lut_only,
                None,
            ))
        } else {
            let telemetry = self.views[data]
                .live_render_started
                .map(|started| (started, Arc::clone(&self.views[data].live_queue_timings)));
            let Some(vol) = self.views[data].volume.as_mut() else {
                return (None, true);
            };
            // No tilts yet (a volume that has only just started arriving) is a "wait", not a
            // failure: erroring here put "tilt 0 out of range" on the map once per volume.
            if vol.elevations.is_empty() {
                return (None, true);
            }
            let sweep = if let Some((spec, key)) = &product {
                vol.product_sweep(*key, &spec.expr, spec.range, spec.env, tilt)
            } else if column_max {
                vol.column_max(moment, clean)
            } else if clean {
                vol.clean_reflectivity(tilt)
            } else {
                vol.binned(moment, tilt, dealias)
            };
            sweep.map(|s| {
                if want_age {
                    ring = ScanAgeRing::from_sweep(s);
                }
                // A product's own colour table, else a ramp across the range it came out in.
                let ramp = product.as_ref().map(|(spec, _)| {
                    product_range = Some((s.value_min, s.value_max));
                    spec.table
                        .clone()
                        .unwrap_or_else(|| crate::colormap::ramp_table(s.value_min, s.value_max))
                });
                let mut upload = to_upload(
                    s,
                    ramp.as_ref().unwrap_or(table),
                    threshold,
                    smooth,
                    storm_uv,
                    precip.as_deref(),
                    lut_only,
                    telemetry,
                );
                if strict_current && !upload.data.is_empty() {
                    wxdata::level2::mask_previous_pass_rows(s, &mut upload.data);
                    // The old rows are transparent now; no dimming pass is needed.
                    upload.uniform[19] = 0.0;
                }
                upload
            })
        };
        match upload {
            Ok(up) => {
                self.views[data].live_render_started = None;
                if product_range.is_some() || product.is_none() {
                    self.views[idx].product_range = product_range;
                }
                self.pane_shown.insert(idx, key);
                self.pane_lut.insert(idx, lut_gen);
                // Recorded even when `None` (a trail has no single rotation to age, and some
                // sweeps carry no timing), so "looked, nothing to draw" is not re-asked every frame.
                if want_age {
                    self.scan_age_rings.insert(idx, ring);
                }
                (Some(up), true)
            }
            Err(e) => {
                self.views[idx].error = Some(e.to_string());
                (None, false)
            }
        }
    }

    /// Build the static instance buffer for all real gates in a Level II volume. The cache key
    /// excludes camera state on purpose: pitch, bearing, pan and zoom only change the shared
    /// camera uniform.
    fn pane_observed_radar(
        &mut self,
        idx: usize,
        data: usize,
    ) -> (Option<ObservedSweepUpload>, bool) {
        let state = &self.views[idx].map_3d;
        if !state.enabled || state.representation != Map3dRepresentation::ObservedSweeps {
            return (None, false);
        }
        let moment = self.views[idx].moment;
        if moment == Moment::SpecificDifferentialPhase {
            // KDP is derived from PhiDP after binning; calling it an observed gate would be false.
            return (None, false);
        }
        let Some(vol) = self.views[data].volume.as_ref() else {
            return (None, false);
        };
        if !vol.moments[moment.index()] {
            return (None, false);
        }
        let name = vol.name.clone();
        let scan = Arc::clone(&vol.scan);
        let (value_min, value_max) = moment.value_range();
        // CC anomaly replaces the floor for correlation coefficient rather than stacking with it.
        // Leaving both live would be self-defeating: the floor's whole effect on CC is to discard
        // the low values the anomaly ramp exists to bring forward, so a user who turned anomaly
        // on would still lose the debris to a threshold set for a different moment's logic.
        let cc_anomaly = self.views[idx].map_3d.cc_anomaly;
        let cc_on = moment == Moment::CorrelationCoefficient && cc_anomaly.enabled;
        let cc = if cc_on {
            crate::render3d::cc_anomaly_uniform(cc_anomaly, (value_min, value_max), false)
        } else {
            [0.0; 4]
        };
        let threshold = self.views[idx].active_threshold().filter(|_| !cc_on);
        let threshold_idx = threshold.map_or(2.0, |value| {
            (2.0 + (value - value_min) / (value_max - value_min).max(f32::EPSILON) * 253.0)
                .clamp(2.0, 255.0)
        });
        let (motion_e, motion_n, srv) = self.views[idx]
            .storm_motion_uv()
            .map(|(e, n)| {
                let per_ms = 253.0 / (value_max - value_min).max(f32::EPSILON);
                (e * per_ms, n * per_ms, 1.0f32)
            })
            .unwrap_or((0.0, 0.0, 0.0));
        // -inf in an unused slot reads as "nothing here" on the shader side (`> -900.0` is false
        // for it) and is exact under `.to_bits()` round-tripping, unlike NaN's multiple bit
        // patterns. Only the first `selected_layer_elevs.len()` slots are ever real; the UI caps
        // that at `MAX_HIGHLIGHTED_LAYERS` already, so no truncation happens here.
        let mut highlight_elevs = [f32::NEG_INFINITY; MAX_HIGHLIGHTED_LAYERS];
        for (slot, elev) in highlight_elevs
            .iter_mut()
            .zip(self.views[idx].map_3d.selected_layer_elevs.iter())
        {
            *slot = *elev;
        }
        let mut controls = [0u32; 12 + MAX_HIGHLIGHTED_LAYERS];
        controls[0] = self.views[idx].map_3d.vertical_exaggeration.to_bits();
        controls[1] = self.views[idx].map_3d.opacity.to_bits();
        controls[2] = threshold_idx.to_bits();
        controls[3] = motion_e.to_bits();
        controls[4] = motion_n.to_bits();
        controls[5] = srv.to_bits();
        // Rebuild when the volume gains a sweep (live chunk stream) so every available tilt
        // is in the buffer, not just the ones present when 3D was first enabled.
        controls[6] = scan.sweeps().len() as u32;
        // The uniform only reaches the GPU alongside a fresh instance buffer, so anything that
        // lives in it has to be part of the rebuild identity or moving the control silently does
        // nothing. That is why `threshold_idx` and friends are already here, why the CC ramp has
        // to join them, and why `beam_rise` — which rides in the uniform slot the lowest-tilt
        // elevation vacated — does too.
        controls[7] = self.views[idx].map_3d.beam_rise.to_bits();
        for (slot, v) in controls[8..12].iter_mut().zip(cc.iter()) {
            *slot = v.to_bits();
        }
        for (slot, elev) in controls[12..].iter_mut().zip(highlight_elevs.iter()) {
            *slot = elev.to_bits();
        }
        let palette_gen = self.palettes.gen.wrapping_add(
            if crate::theme::is_high_contrast(self.settings.theme) {
                0x9e37_79b9_7f4a_7c15
            } else {
                0
            },
        );
        let key = (
            name,
            self.views[data].live_scan_revision,
            moment,
            palette_gen,
            controls,
        );
        if self.views[idx].map_3d.observed_key.as_ref() == Some(&key) {
            return (None, true);
        }
        // Full native resolution: every gate of every radial goes to the GPU as a texel, and the
        // shader draws each radial as a strip on its own beam surface. `max_texture_dim` is only
        // a ceiling; a sweep wider than it is max-pooled and reported, never silently thinned.
        let observed = match level2::observed_volume(&scan, moment, self.max_texture_dim as usize) {
            Ok(volume) => volume,
            Err(err) => {
                self.views[idx].error = Some(err.to_string());
                return (None, false);
            }
        };
        self.views[idx].map_3d.observed_layers = observed.layers.clone();
        // A selection surviving a moment switch or a tilt dropping out of the volume would dim
        // every gate (the shader has a selection but nothing left to match it), which reads as
        // the whole layer vanishing rather than as nothing being selected.
        self.views[idx].map_3d.selected_layer_elevs.retain(|&sel| {
            observed
                .layers
                .iter()
                .any(|l| (l.elevation_deg - sel).abs() < 0.05)
        });
        let antenna_altitude_m = self.views[idx]
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
            .map(|site| site.elevation_meters as f64 + wxdata::towers::tower_m(site.id))
            .unwrap_or(0.0) as f32;
        let table = crate::colormap::effective_table(&self.palettes, moment, self.settings.theme);
        let lut = crate::colormap::bake_lut(&table, (value_min, value_max), None).to_vec();
        #[cfg(debug_assertions)]
        log::debug!(
            "3D observed {}: {} sweeps, {} radials, {} layers, pooled {:?}",
            moment.short_name(),
            observed.sweep_count,
            observed.radial_count,
            observed.sweeps.len(),
            observed.sweeps.iter().map(|s| s.pool).max()
        );
        let (radar_lat, radar_lon) = (observed.radar_lat, observed.radar_lon);
        let mut instances = Vec::with_capacity(observed.radial_count);
        let mut layers = Vec::with_capacity(observed.sweeps.len());
        for (layer, sweep) in observed.sweeps.into_iter().enumerate() {
            for (row, r) in sweep.radials.iter().enumerate() {
                instances.push(ObservedRadialInstance {
                    polar: [
                        r.azimuth_deg,
                        r.spacing_deg,
                        r.first_gate_km,
                        r.gate_interval_km,
                    ],
                    data: [
                        r.elevation_deg,
                        layer as f32,
                        row as f32,
                        r.gate_count as f32,
                    ],
                });
            }
            layers.push(ObservedSweepLayer {
                width: sweep.width as u32,
                rows: sweep.radials.len() as u32,
                values: sweep.values,
            });
        }
        let uniform = observed_uniform(
            [
                radar_lat,
                radar_lon,
                antenna_altitude_m,
                self.views[idx].map_3d.vertical_exaggeration,
                self.views[idx].map_3d.opacity,
                threshold_idx,
                Camera::world_units_per_metre(radar_lat as f64) as f32,
                srv,
                motion_e,
                motion_n,
                // Slot 10: the volume's lowest-tilt elevation used to live here and stopped being
                // read; `beam_rise` takes it over rather than growing the buffer. See the shader's
                // own `Radar3d` comment.
                self.views[idx].map_3d.beam_rise,
            ],
            cc,
            highlight_elevs,
        );
        self.views[idx].map_3d.observed_key = Some(key);
        (
            Some(ObservedSweepUpload {
                instances,
                layers,
                uniform,
                lut,
            }),
            true,
        )
    }

    /// The map-pitch "Smooth" and "Debris" 3D representations: a continuous, interpolated volume
    /// raymarched in place on the map, as against `pane_observed_radar`'s real (and therefore
    /// gappy-at-range) Level II gates. `None` when this pane isn't in one of those two modes.
    /// `Some` carries this frame's camera/radar uniform always, and a fresh
    /// [`crate::render3d::Volume3dUpload`] only on the frame a rebuild finishes — the resample is
    /// real CPU work (`build_volume3d`'s own comment: "would drop a second of frames"), so it runs
    /// off-thread and this drains it rather than blocking the render path.
    ///
    /// Debris shares every line of this with Smooth — same resample, same raymarch, same controls
    /// — except which moment it resamples and that its volume's index is inverted afterward
    /// ([`wxdata::volume3d::invert_in_place`]) before upload, with the LUT permuted
    /// ([`crate::colormap::invert_lut`]) to match. See that function's doc comment for why: a
    /// max-intensity raymarch over plain CC only ever finds ordinary high-CC rain, never the
    /// lofted low-CC pocket a tornado debris signature actually is.
    fn pane_smooth_volume(
        &mut self,
        idx: usize,
        data: usize,
        ctx: &egui::Context,
        cam: &crate::render::mercator::Camera,
        vp: (f32, f32),
    ) -> Option<(
        Option<Arc<crate::render3d::Volume3dUpload>>,
        crate::render3d::Uniforms,
    )> {
        // Cloned (not borrowed) up front: `Map3dState` isn't `Copy`, and every branch below needs
        // `&mut self` for the async-build bookkeeping, so holding a borrow of it across this
        // function would fight the borrow checker for no benefit — it's a few scalars and a small
        // `Option`, cheap to clone.
        let state = self.views[idx].map_3d.clone();
        if !state.enabled {
            return None;
        }
        let (resample_moment, _) = state.representation.smooth_moment()?;
        if self.views[idx].moment != resample_moment {
            // `map_3d_controls` already resets back to Observed the moment this stops being
            // true; this is a second, cheap guard against ever resampling the wrong moment.
            return None;
        }
        let site = self.views[idx]
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)?;

        // Show the built volume for what is displayed, from the pane's loop cache, and start its
        // build when there is none (ROADMAP_NEW H8). Until it lands the last one stays up, and the
        // controls say so. While the loop plays, frames use the smaller loop grid, so a loop's
        // worth fits in the cache; pausing builds the paused frame at full resolution.
        self.loop3d_jobs.drain(&mut self.loop3d);
        let loop_quality = self.views[data].timeline.playing;
        if let Some((name, rev)) = self.shown_volume_key(data) {
            if let Some(key) = self.smooth_key_for(idx, &name, rev, loop_quality) {
                if self.smooth_vol_key[idx].as_ref() != Some(&key) {
                    let job = JobKey::Smooth(idx, key.clone());
                    if let Some(up) = self.loop3d[idx].smooth.get(&key) {
                        self.smooth_vol_dims[idx] = Some((up.n, up.nz, up.half_km, up.top_km));
                        self.smooth_vol_info[idx] = Some((up.cell_km(), up.outside));
                        self.smooth_vol_range[idx] = up.value_range;
                        self.smooth_vol_pending[idx] = Some(Arc::clone(up));
                        self.smooth_vol_key[idx] = Some(key);
                        ctx.request_repaint();
                    } else if self.loop3d_jobs.came_up_empty(&job) {
                        // No sweeps survived the resample: draw nothing rather than the last one.
                        self.smooth_vol_dims[idx] = None;
                        self.smooth_vol_info[idx] = None;
                        self.smooth_vol_range[idx] = None;
                        self.smooth_vol_key[idx] = Some(key);
                    } else if self.loop3d_jobs.wants(&job) {
                        if let (Some(spec), Some(vol)) = (
                            self.smooth_spec(idx, loop_quality),
                            self.views[data].volume.as_mut(),
                        ) {
                            if spec.product.is_some() {
                                // A product reads every moment: build it from the whole scan,
                                // off the UI thread.
                                let scan = Arc::clone(&vol.scan);
                                self.loop3d_jobs.start(job, &self.spawner, ctx, move || {
                                    crate::loop3d::Built::Smooth(crate::loop3d::build_smooth(
                                        crate::loop3d::Sweeps::Scan(scan),
                                        &spec,
                                    ))
                                });
                            } else {
                                // Velocity dealiased, so folded gates do not read as false
                                // couplets.
                                let sweeps = if resample_moment == Moment::Velocity {
                                    vol.velocity_tilts_dealiased()
                                } else {
                                    vol.moment_tilts(resample_moment)
                                };
                                let mask = crate::loop3d::masked_by_reflectivity(resample_moment)
                                    .then(|| vol.moment_tilts(Moment::Reflectivity));
                                if !sweeps.is_empty() {
                                    self.loop3d_jobs.start(job, &self.spawner, ctx, move || {
                                        crate::loop3d::Built::Smooth(crate::loop3d::build_smooth(
                                            crate::loop3d::Sweeps::Binned { sweeps, mask },
                                            &spec,
                                        ))
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // Nothing GPU-resident yet for this pane (first frame in Smooth mode, or the first build
        // is still in flight) — nothing to raymarch this frame.
        let (n, nz, half_km, top_km) = self.smooth_vol_dims[idx]?;
        let antenna_altitude_m =
            (site.elevation_meters as f64 + wxdata::towers::tower_m(site.id)) as f32;
        // A geometry-only stand-in: `map_uniform` reads `n`/`nz`/`half_km`/`top_km` off it and
        // nothing else, so the (empty, non-allocating) `data`/`lut` never have to hold the actual
        // multi-megabyte volume just to recompute a camera matrix on a frame with no new upload.
        let dims_only = crate::render3d::Volume3dUpload {
            data: Vec::new(),
            n,
            nz,
            lut: Vec::new(),
            half_km,
            top_km,
            outside: 0.0,
            value_range: None,
        };
        // Denoising a plain "high is interesting" field makes sense for reflectivity and spectrum
        // width; `SmoothDebris`'s inverted-CC volume is the opposite sense (low is interesting)
        // and has no floor of its own here.
        let denoise_floor = match state.representation {
            Map3dRepresentation::SmoothVolume => Some(state.reflectivity_floor_dbz),
            Map3dRepresentation::SmoothSpectrumWidth => Some(state.sw_floor_ms),
            Map3dRepresentation::SmoothZdr => Some(state.zdr_floor_db),
            Map3dRepresentation::SmoothKdp => Some(state.kdp_floor_deg_km),
            Map3dRepresentation::SmoothVelocity => Some(state.velocity_floor_ms),
            // A product's floor is in its own units, inside the range its volume was drawn over.
            Map3dRepresentation::SmoothProduct => {
                self.smooth_vol_range[idx].map(|(lo, hi)| state.product_floor.clamp(lo, hi))
            }
            Map3dRepresentation::SmoothDebris | Map3dRepresentation::ObservedSweeps => None,
        };
        // The values the volume's indices span: a product's own range, else the moment's.
        let index_range = match state.representation {
            Map3dRepresentation::SmoothProduct => self.smooth_vol_range[idx],
            _ => None,
        }
        .unwrap_or(resample_moment.value_range());
        // Velocity's volume is indexed by speed (`fold_by_speed`), so its floor and ceiling are
        // speeds: the floor is the outbound index at that speed (inbound sits one above it) and
        // the ceiling the inbound one, so both directions are kept alike.
        let speed_scale =
            (resample_moment == Moment::Velocity).then(|| resample_moment.value_range().1);
        // Velocity's values are speeds, either direction; every other moment's are signed values
        // (a -10 dBZ floor, a negative ZDR) and go in as they are.
        let value_index = |v: f32, top: bool| match speed_scale {
            Some(vmax) => {
                let v = v.abs();
                (wxdata::volume3d::speed_index(v, vmax) + u8::from(top && v < vmax)) as f32
            }
            None => crate::render3d::threshold_index(v, index_range),
        };
        let threshold_idx = match denoise_floor {
            Some(floor) if state.denoise_enabled => value_index(floor, false),
            _ => 2.0,
        };
        // Debris is the CC representation, so it gets the anomaly ramp. `inverted: true` because
        // the volume it raymarches has already had its indices flipped (`invert_in_place`) — the
        // ramp has to run the other way round in that space to still mean "low CC draws solid".
        let cc = match state.representation {
            Map3dRepresentation::SmoothDebris => crate::render3d::cc_anomaly_uniform(
                state.cc_anomaly,
                resample_moment.value_range(),
                true,
            ),
            _ => [0.0; 4],
        };
        let ceiling_idx = match state.ceilings[state.representation as usize] {
            Some(top) if denoise_floor.is_some() && state.denoise_enabled => value_index(top, true),
            _ => 0.0,
        };
        // The opacity curve, its values into this volume's index space as the floor's are.
        let tf = state.tf_curves[state.representation as usize]
            .filter(|_| denoise_floor.is_some())
            .map(|pts| pts.map(|[v, a]| [value_index(v, false), a]));
        let view = crate::render3d::View3d {
            tf,
            threshold_idx,
            ceiling_idx,
            clip: state.clip,
            plane: state.plane,
            cc,
            cappi_km: state.cappi_marker.then_some(self.cappi_alt_km),
        };
        // Same visual-guard clamp as the standalone 3D Reflectivity window: a phone under thermal
        // or battery pressure gets the coarsest march regardless of what quality was chosen.
        let steps = if ui::motion::degraded() {
            state.quality_steps.min(64)
        } else {
            state.quality_steps
        }
        // The march takes about one sample per cell (`map_uniform`), so this is a ceiling, not
        // the count: the grid is now far finer than the old fixed 64-128 steps could cover.
        * 4;
        let uniform = crate::render3d::map_uniform(
            cam,
            vp,
            site.longitude as f64,
            site.latitude as f64,
            antenna_altitude_m,
            &dims_only,
            steps,
            view,
            state.vertical_exaggeration,
            state.opacity,
        );
        Some((self.smooth_vol_pending[idx].take(), uniform))
    }

    /// The always-on-top mini loop: a small undecorated window showing the active pane, so the
    /// radar stays visible over whatever else is on screen.
    ///
    /// An *immediate* viewport, not a deferred one: deferred viewports run their closure on the
    /// egui side and demand `'static + Send + Sync`, which `HookEchoApp` is not (wgpu handles,
    /// `Rc`s in the tile caches). Immediate renders inline on this thread, which is exactly what
    /// reusing [`Self::render_pane`] needs.
    /// The loop keeps its own camera, cloned from the active pane when the window opens and
    /// swapped in for the duration of the render — panning the little window is how you look
    /// somewhere else while the main map stays where it was.
    // ponytail: not persisted; it is a window, not a layer (see `OverlayToggle::session_only`).
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    fn mini_loop_viewport(&mut self, ctx: &egui::Context) {
        if !self.mini_loop {
            return;
        }
        let idx = self.active.min(self.views.len() - 1);
        let caption = {
            let v = &self.views[idx];
            let time = v.volume.as_ref().map_or_else(
                || "—".to_string(),
                |vol| vol.time.format("%H:%MZ").to_string(),
            );
            format!(
                "{} {} {time}",
                v.site.as_deref().unwrap_or("—"),
                v.moment.short_name()
            )
        };
        let builder = egui::ViewportBuilder::default()
            .with_title("HookEcho — mini loop")
            .with_inner_size([340.0, 260.0])
            .with_decorations(false)
            // Honoured on X11 only. The comment here used to blame GNOME's policy and say KDE
            // was fine — it is not a policy question: winit's Wayland backend implements
            // `set_window_level` as an empty function (winit 0.30, `platform_impl/linux/wayland/
            // window/mod.rs`), so no Wayland compositor is ever asked. Nothing to fix here
            // without going around winit to `xdg-foreign`/layer-shell, which is not worth it for
            // one optional window. The tool's own description says so under Wayland rather than
            // leaving the user to wonder why their window keeps disappearing behind the browser.
            .with_always_on_top();
        let mut close = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("mini-loop"),
            builder,
            |ctx, _class| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        let full = ui.max_rect();
                        // Undecorated, so we draw our own caption strip: label, drag handle, close.
                        let (bar, prect) = (
                            egui::Rect::from_min_max(
                                full.min,
                                egui::pos2(full.max.x, full.min.y + 18.0),
                            ),
                            egui::Rect::from_min_max(
                                egui::pos2(full.min.x, full.min.y + 18.0),
                                full.max,
                            ),
                        );
                        ui.painter()
                            .rect_filled(bar, 0.0, egui::Color32::from_gray(24));
                        let handle = ui.interact(
                            bar,
                            egui::Id::new("mini-loop-bar"),
                            egui::Sense::click_and_drag(),
                        );
                        if handle.drag_started() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }
                        ui.painter().text(
                            bar.left_center() + egui::vec2(6.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            &caption,
                            egui::FontId::proportional(11.0),
                            egui::Color32::from_gray(190),
                        );
                        let x = egui::Rect::from_center_size(
                            bar.right_center() - egui::vec2(10.0, 0.0),
                            egui::vec2(16.0, 16.0),
                        );
                        if ui
                            .interact(x, egui::Id::new("mini-loop-close"), egui::Sense::click())
                            .clicked()
                        {
                            close = true;
                        }
                        ui.painter().text(
                            x.center(),
                            egui::Align2::CENTER_CENTER,
                            "✕",
                            egui::FontId::proportional(11.0),
                            egui::Color32::from_gray(190),
                        );
                        let vctx = ui.ctx().clone();
                        // Swap this window's camera in around the render, and take back whatever
                        // the pointer did to it. Nothing returns early in between, so the pane
                        // always gets its own camera back.
                        let mine = self.mini_cam.take().unwrap_or(self.views[idx].camera);
                        let pane_cam = std::mem::replace(&mut self.views[idx].camera, mine);
                        self.render_pane(
                            // `first`/`last` false: the mini-loop viewport is a passenger — the
                            // main window's pane loop owns draining and evicting the tile caches.
                            ui,
                            &vctx,
                            idx,
                            prect,
                            false,
                            false,
                            false,
                            false,
                            &[],
                        );
                        self.mini_cam =
                            Some(std::mem::replace(&mut self.views[idx].camera, pane_cam));
                    });
                if ctx.input(|i| i.viewport().close_requested()) {
                    close = true;
                }
            },
        );
        if close {
            self.mini_loop = false;
            // Reopening should frame what the main map is looking at now, not where the loop was
            // pointed an hour ago.
            self.mini_cam = None;
        }
    }

    /// A diff/compare grid's value under the cursor, in the field's own display units — shared by
    /// the `ModelDiff` and `CompareA`/`CompareB` hover readouts in `render_pane`, which differ
    /// only in how they word what they found (a subtraction vs. one side's own reading).
    fn diff_hover_value(
        &self,
        grid: &wxdata::mrms::MrmsField,
        cam: crate::render::mercator::Camera,
        prect: egui::Rect,
        vp: (f32, f32),
        hp: egui::Pos2,
    ) -> Option<f32> {
        let w = cam.screen_to_world((hp.x - prect.left(), hp.y - prect.top()), vp);
        let (lon, lat) = crate::render::mercator::world_to_lonlat(w.0, w.1);
        let v = grid.sample_bilinear(lon, lat)?;
        Some(v * self.diff_field.input_scale())
    }

    /// ROADMAP_NEW F6/J4 "blink A/B": which side of the comparison a pane should show at wall
    /// clock `t` (egui's own clock, not `Instant` — the phase only has to be consistent within
    /// one running session, not survive a restart). `true` means B is up.
    fn blink_showing_b(t: f64, half_cycle_secs: f64) -> bool {
        (t / half_cycle_secs) as i64 % 2 == 1
    }

    /// How long until `blink_showing_b` next flips — used to schedule the next repaint instead of
    /// animating every frame while nothing on screen is actually changing.
    fn blink_seconds_until_flip(t: f64, half_cycle_secs: f64) -> f64 {
        (half_cycle_secs - t.rem_euclid(half_cycle_secs)).max(0.01)
    }

    /// Whether a pane-local x coordinate falls on the B side of a model swipe.
    fn swipe_showing_b(fraction: f32, x: f32, width: f32) -> bool {
        x >= width.max(0.0) * fraction.clamp(0.0, 1.0)
    }

    /// Convert the divider pointer position to a stable, usable pane fraction. Keeping a little
    /// of each model visible makes the handle recoverable after an enthusiastic drag to an edge.
    fn swipe_fraction_from_pointer(pointer_x: f32, pane_left: f32, pane_width: f32) -> f32 {
        if pane_width <= f32::EPSILON {
            return 0.5;
        }
        ((pointer_x - pane_left) / pane_width).clamp(0.05, 0.95)
    }

    /// Render one pane into `prect`: input, tiles, radar, paint callback, and painter overlays.
    #[allow(clippy::too_many_arguments)]
    fn render_pane(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        idx: usize,
        prect: egui::Rect,
        clear_tiles: bool,
        clear_vector: bool,
        first: bool,
        last: bool,
        placefile_labels: &[PlaceLabel],
    ) {
        crate::prof_scope!("render_pane");
        // Every pane draws its label layers top priority first.
        self.labels.next_pane();
        use crate::tiles::BasemapStyle;
        // This pane's own basemap: panes are independent, the tile caches are keyed by style.
        // `Auto` resolves here rather than where it is stored, so the stored choice keeps
        // following the theme instead of being frozen the first time it is rendered.
        let pane_style = self.views[idx].basemap.resolve(ui.visuals().dark_mode);
        // Hybrid satellite is both: Esri imagery from the raster path, roads and boundaries from
        // the vector one drawn on top of it.
        let is_vector = pane_style.vector_palette().is_some();
        let is_raster = pane_style.is_raster();
        let vp = (prect.width(), prect.height());
        // ROADMAP_NEW F6/J4 "blink A/B": flip which compare field is in `fields_on` on a timer.
        // `field_draws` (built later this same frame from `fields_on`) picks this straight up
        // through the exact same path "View side by side" already uses — no GPU/shader change
        // needed, blinking is just which single layer is a member this frame.
        if self.views[idx].blink_compare {
            use crate::render::FieldLayer as FL;
            const HALF_CYCLE_SECS: f64 = 1.5;
            let t = ctx.input(|i| i.time);
            let (on, off) = if Self::blink_showing_b(t, HALF_CYCLE_SECS) {
                (FL::CompareB, FL::CompareA)
            } else {
                (FL::CompareA, FL::CompareB)
            };
            self.views[idx].fields_on.insert(on);
            self.views[idx].fields_on.remove(&off);
            // Wake exactly at the next flip rather than every frame — blinking is not a
            // continuous animation loop.
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                Self::blink_seconds_until_flip(t, HALF_CYCLE_SECS),
            ));
        }
        // The divider and map are one interaction target. Two overlapping `ui.interact` calls
        // made swipe unreliable because egui could give the press to the full-pane map before the
        // narrower divider saw it. Decide ownership from the press origin once, then preserve it
        // for the whole drag so the moving handle never drops the gesture.
        let response = ui
            .interact(
                prect,
                egui::Id::new(("pane", idx)),
                egui::Sense::click_and_drag(),
            )
            .on_hover_cursor(if self.views[idx].swipe_compare {
                let x = prect.left() + prect.width() * self.views[idx].swipe_fraction;
                if ui
                    .input(|i| i.pointer.hover_pos())
                    .is_some_and(|pos| (pos.x - x).abs() <= 12.0)
                {
                    egui::CursorIcon::ResizeHorizontal
                } else {
                    egui::CursorIcon::Default
                }
            } else {
                egui::CursorIcon::Default
            });
        if self.views[idx].swipe_compare {
            let x = prect.left() + prect.width() * self.views[idx].swipe_fraction;
            let hit = egui::Rect::from_min_max(
                egui::pos2(x - 12.0, prect.top()),
                egui::pos2(x + 12.0, prect.bottom()),
            );
            if response.drag_started() {
                self.views[idx].swipe_dragging = ui.input(|i| {
                    i.pointer
                        .press_origin()
                        .is_some_and(|pos| hit.contains(pos))
                });
            }
            if self.views[idx].swipe_dragging && response.dragged() {
                if let Some(pos) = response.interact_pointer_pos() {
                    self.views[idx].swipe_fraction =
                        Self::swipe_fraction_from_pointer(pos.x, prect.left(), prect.width());
                    self.active = idx;
                    ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
            }
            if response.drag_stopped() {
                self.views[idx].swipe_dragging = false;
            }
        } else {
            self.views[idx].swipe_dragging = false;
        }
        let swipe_dragging = self.views[idx].swipe_dragging;

        // --- Input (mutates this pane's camera / selects it active) ---
        // During a multi-touch gesture the first finger still drives the egui pointer, so a pinch
        // would ALSO register as a drag and fight the zoom — the gesture block below owns both
        // pan and zoom while two fingers are down.
        let gesture = ui.input(|i| i.multi_touch());
        if gesture.is_some() {
            self.last_gesture_end = Some(wxdata::clock::Instant::now());
        }
        // A finger still down after the other lifted is the tail of a pinch, not a new drag or a
        // tap on the map. 150 ms is long enough to cover a normal two-finger lift and short
        // enough to be invisible when you really did mean to tap.
        let gesture_tail = self
            .last_gesture_end
            .is_some_and(|t| t.elapsed().as_millis() < 150);
        let quiet = gesture.is_none() && !gesture_tail;
        // Watch-zone tool: a double-click (or Enter) closes the ring being clicked out.
        if self.tool == MapTool::AlertZone
            && self.zone_pts.len() >= 3
            && (response.double_clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)))
        {
            let mut ring = std::mem::take(&mut self.zone_pts);
            // The two clicks of the closing double-click each dropped a vertex on the same spot.
            ring.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
            let n = self.settings.alert_polygons.len() + 1;
            self.zone_naming = Some((ring, format!("Zone {n}")));
            self.tool = MapTool::Interrogate;
        }
        // The draw tool takes the drag away from the pan, the same deal the measure tool makes
        // with the click: while it's armed, a drag draws. Disarm it (Esc / another tool) to pan.
        let tracking =
            quiet && !swipe_dragging && self.storm_track_input(idx, prect, &response, ui);
        if tracking {
            // The storm-motion tool took the drag (or holds a handle): no pan under it.
        } else if self.tool == MapTool::Draw && quiet && !swipe_dragging {
            if response.dragged() {
                self.active = idx;
                if let Some(pos) = response.interact_pointer_pos() {
                    let cam = self.views[idx].camera;
                    let px = (pos.x - prect.left(), pos.y - prect.top());
                    let w = cam.screen_to_world(px, vp);
                    let ll = crate::render::mercator::world_to_lonlat(w.0, w.1);
                    draw_append(
                        &mut self.strokes,
                        [ll.0, ll.1],
                        self.draw_color,
                        response.drag_started(),
                    );
                }
            }
        } else if response.dragged() && quiet && !swipe_dragging {
            self.active = idx;
            let d = response.drag_delta();
            if self.views[idx].map_3d.enabled && response.dragged_by(egui::PointerButton::Secondary)
            {
                self.views[idx].camera.bearing =
                    (self.views[idx].camera.bearing - d.x * 0.35 + 180.0).rem_euclid(360.0) - 180.0;
                self.views[idx].camera.pitch = (self.views[idx].camera.pitch + d.y * 0.25)
                    .clamp(0.0, crate::render::mercator::MAX_PITCH_DEG);
            } else {
                match self.tap_zoom {
                    // Double-tap-drag: the map zoom every phone map has, and the only one you can do
                    // one-handed. Drag up to zoom in, anchored on the point that was tapped, so the
                    // thing you double-tapped is the thing that stays put.
                    Some(anchor) => {
                        let cursor = (anchor.x - prect.left(), anchor.y - prect.top());
                        self.views[idx]
                            .camera
                            .zoom_at(-d.y as f64 * 0.01, cursor, vp);
                    }
                    None => {
                        self.views[idx].camera.pan_pixels(d.x, d.y, vp);
                        self.follow_cell = None; // a manual pan takes over the camera
                    }
                }
            }
        }
        if cfg!(target_os = "android") {
            if response.drag_started() {
                // Within a third of a second of the last tap and within a thumb's width of it:
                // this is the second tap, still down. Anything else is an ordinary drag.
                self.tap_zoom = response.interact_pointer_pos().filter(|p| {
                    self.last_tap.is_some_and(|(t, at)| {
                        ui.input(|i| i.time) - t < 0.35 && at.distance(*p) < 44.0
                    })
                });
            }
            if response.drag_stopped() {
                self.tap_zoom = None;
                // One zoom per double tap: the next one has to be armed by a fresh tap.
                self.last_tap = None;
            }
        }
        // Wheel, trackpad, and pinch all land here. `zoom_delta` is a scale factor carrying the
        // macOS/precision-touchpad pinch gesture (and ctrl+wheel, which egui folds into the same
        // signal and subtracts from the scroll delta, so the two never double-apply). Horizontal
        // scroll pans: on a trackpad a two-finger swipe is the obvious way to move the map, and on
        // a mouse it is a tilt wheel nobody was using.
        let (zoom, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
        if let Some(pos) = response.hover_pos().filter(|p| prect.contains(*p)) {
            let cursor = (pos.x - prect.left(), pos.y - prect.top());
            // ROADMAP_NEW J3: share this pane's hovered geo point with every other pane. Reused
            // directly as the probe table's sample point in `paint_linked_cursor`, called once
            // after every pane has had a chance to update it this frame.
            if self.link_cursor {
                let w = self.views[idx].camera.screen_to_world(cursor, vp);
                self.linked_probe = Some(crate::render::mercator::world_to_lonlat(w.0, w.1));
            }
            // `zoom_delta()` reports a live touchscreen pinch too, which the gesture block below
            // already owns (and it is the one that knows about the mobile chrome) — skip it here
            // or a phone pinch zooms twice.
            if gesture.is_none() && (zoom - 1.0).abs() > f32::EPSILON {
                self.active = idx;
                self.views[idx]
                    .camera
                    .zoom_at((zoom as f64).log2(), cursor, vp);
            }
            if scroll.y.abs() > 0.0 {
                self.active = idx;
                self.views[idx]
                    .camera
                    .zoom_at(scroll.y as f64 * 0.005, cursor, vp);
            }
            if scroll.x.abs() > 0.0 {
                self.active = idx;
                self.views[idx].camera.pan_pixels(scroll.x, 0.0, vp);
                self.follow_cell = None; // a manual pan takes over the camera
            }
        }
        // Two-finger gesture (touchscreens): pan by the gesture's translation, and zoom by the
        // pinch. `zoom_delta` is a scale factor, so its log2 is the change in the camera's log2
        // zoom level; anchor it at the gesture center so the pinched point stays put. Fires for
        // the pane the gesture centers over. No-op with no touch.
        if let Some(mt) = gesture {
            // The mobile chrome floats over the map, and `multi_touch()` is raw input with no
            // notion of which layer the fingers are on — so a pinch on the bottom sheet used to
            // zoom the map underneath it. The chrome publishes what it covers; skip those rects.
            // Explicit rects cover the always-on chrome; the layer test covers every window and
            // popup on top of it, which is what keeps a pinch on a Skew-T or a full-screen
            // settings surface from also zooming the map behind it.
            // `is_pointer_over_egui()` cannot be used here: it tests egui's single pointer
            // position, which during a two-finger gesture is one arbitrary finger, and it treats
            // the map's own background layer as "over egui" once the central panel has consumed
            // the root ui's available rect — so it answered true over bare map and killed every
            // pinch. Ask about the gesture's own center instead: any layer above the background
            // there is real chrome.
            let over_layer = ui
                .ctx()
                .layer_id_at(mt.center_pos)
                .is_some_and(|l| l.order != egui::Order::Background);
            let occluded = over_layer
                || self
                    .mobile_occlusion
                    .iter()
                    .any(|r| r.contains(mt.center_pos));
            if prect.contains(mt.center_pos) && !occluded {
                self.active = idx;
                // Zoom first, then pan: the translation is in screen pixels, and applying it at
                // the pre-zoom scale over-moves the map by the pinch's own scale factor — which is
                // what made the anchor trail the fingers.
                //
                // Belt-and-suspenders against a runaway single-frame gesture (reported on web
                // touchscreens: the map "flies away" with multiple fingers down). egui's own
                // `TouchState` already nulls the previous sample when the touch count changes
                // within a frame (`begin_pass`'s `added_or_removed_touches` guard), so a clean
                // 2-vs-3-finger centroid jump is not the mechanism — but browser touch-event
                // dispatch upstream of egui is out of this app's control, so cap what one frame's
                // gesture is allowed to do regardless of where an anomalous sample comes from. A
                // real pinch/pan between consecutive touch samples never moves the centroid by a
                // large fraction of the pane in one frame or halves/doubles the zoom in one frame.
                let max_translation = prect.size().min_elem() * 0.4;
                if mt.translation_delta.length() > max_translation {
                    log::warn!(
                        "multi-touch gesture translation clamped: {:.0}px requested, {:.0}px pane limit",
                        mt.translation_delta.length(),
                        max_translation
                    );
                }
                let zoom_log2 = (mt.zoom_delta as f64).log2().clamp(-1.0, 1.0);
                if (mt.zoom_delta - 1.0).abs() > f32::EPSILON {
                    let cursor = (
                        mt.center_pos.x - prect.left(),
                        mt.center_pos.y - prect.top(),
                    );
                    self.views[idx].camera.zoom_at(zoom_log2, cursor, vp);
                }
                if self.views[idx].map_3d.enabled && mt.rotation_delta.abs() > 0.001 {
                    self.views[idx].camera.bearing =
                        (self.views[idx].camera.bearing - mt.rotation_delta.to_degrees() + 180.0)
                            .rem_euclid(360.0)
                            - 180.0;
                }
                let t = mt.translation_delta.clamp(
                    egui::vec2(-max_translation, -max_translation),
                    egui::vec2(max_translation, max_translation),
                );
                // On a 3D map the two fingers' vertical slide tilts the view instead of panning it
                // (one finger still pans): slide up to lean the map back toward the horizon, down
                // to stand it up flat.
                let (pan, pitch_delta) = split_two_finger_slide(t, self.views[idx].map_3d.enabled);
                if pitch_delta != 0.0 {
                    let cam = &mut self.views[idx].camera;
                    cam.pitch = (cam.pitch + pitch_delta)
                        .clamp(0.0, crate::render::mercator::MAX_PITCH_DEG);
                }
                if pan != egui::Vec2::ZERO {
                    self.views[idx].camera.pan_pixels(pan.x, pan.y, vp);
                    self.follow_cell = None; // a manual pan takes over the camera (pinch-zoom does not)
                }
            }
        }
        // Long-press explores features, whatever tool is armed. A phone has no right-click and no
        // hover, so the press that means "tell me about this" is the press people already try.
        let long_press = cfg!(target_os = "android") && response.long_touched();
        if long_press {
            // Before anything is drawn: the buzz is what says the press was heard.
            crate::platform::haptic(crate::platform::Haptic::Press);
        }
        // A click on a tornado marker opens or closes its web (drawn below); it is not also a
        // click on the storm cell or map under it. Where the markers sat is last frame's.
        let on_circulation = response.interact_pointer_pos().is_some_and(|p| {
            ui.ctx()
                .data(|d| d.get_temp::<Vec<egui::Rect>>(egui::Id::new(("circulation_hits", idx))))
                .unwrap_or_default()
                .iter()
                .any(|r| r.contains(p))
        });
        // A click or long press on the map (`app::map_click`). It returns true where the code
        // used to leave `render_pane` early (a gauge or station card opened), so this does too.
        if (response.clicked() || long_press)
            && quiet
            && !on_circulation
            && self.handle_map_click(ui, ctx, idx, prect, vp, &response, long_press)
        {
            return;
        }

        // --- Tiles (shared caches, per-pane visible list) ---
        let cam = self.views[idx].camera;
        // High-DPI screens render a 256-px raster tile across `ppp`× more physical pixels, which
        // looks blurry (bad on the S24's ~3.75× density). Fetch `round(log2(ppp))` levels deeper so
        // tiles land near 1:1. Desktop (ppp 1) → +0.
        //
        // Capped at +1, not +2: each level quadruples the tiles covering the same ground, so +2
        // asked a phone to fetch, decode, and hold 16× the imagery of a desktop for a screen that
        // is a few inches across. +1 is 4×, still sharper than the panel resolves.
        // A metered link drops to +0: the deeper level is a sharpness nicety, and it costs four
        // tile downloads for every one.
        let bias_cap = if crate::platform::is_metered() {
            0.0
        } else {
            1.0
        };
        let raster_bias = if pane_style.tiles_are_512() || self.tiles.is_retina(pane_style) {
            // 512-px and `@2x` providers already carry the extra detail in the tile itself;
            // biasing on top would fetch four of them per screen tile for nothing.
            0.0
        } else {
            ctx.pixels_per_point()
                .max(1.0)
                .log2()
                .round()
                .clamp(0.0, bias_cap) as f64
        };
        let visible = if is_raster {
            let vis = self.tiles.visible(pane_style, &cam, vp, raster_bias);
            self.tiles.request_missing(pane_style, &vis);
            self.tiles.promote_visible(pane_style, &vis);
            vis
        } else {
            Vec::new()
        };
        // Always run the vector-tile pipeline for its city/town labels — raster basemaps (satellite)
        // bake in faint labels that are hard to read, so we overlay crisp haloed ones. Only the
        // *geometry* is basemap-specific: vector basemaps draw it, raster keeps its own imagery.
        let (visible_vector, vlabels, visible_vector_tiles) = {
            let vis = self.vtiles.visible(&cam, vp);
            self.vtiles.request_missing(&vis);
            let ids: Vec<crate::render::TileId> = vis.iter().map(|v| v.id).collect();
            // Deep-copying every visible place name every frame (for every pane) showed up at the
            // 4-10 fps the phone runs at. The set only changes when the visible tiles do, or when
            // a tile's labels finish tessellating — both bump the key below.
            // Compared before it is built: the key holds a `Vec` of every visible tile id, and
            // cloning it to ask "did this change" was itself a per-pane, per-frame allocation.
            let gen = self.vtiles.label_generation();
            let stale = self
                .vlabel_cache
                .as_ref()
                .is_none_or(|((k_ids, k_gen), _)| *k_gen != gen || *k_ids != ids);
            if stale {
                let labels: std::sync::Arc<[crate::vector_tiles::PlaceLabel]> = self
                    .vtiles
                    .labels_for(ids.iter())
                    .into_iter()
                    .cloned()
                    .collect();
                self.vlabel_cache = Some(((ids.clone(), gen), labels));
            }
            // An `Arc` handle, not a deep copy of every visible place name — the cache existed to
            // stop the *lookup* running every frame, but the copy it returned survived it.
            let labels = self
                .vlabel_cache
                .as_ref()
                .map(|(_, l)| l.clone())
                .unwrap_or_else(|| Vec::new().into());
            (if is_vector { ids } else { Vec::new() }, labels, vis)
        };
        // Drain finished fetches once (on the first pane) — they upload into the shared cache.
        // Eviction lives with the tile manager (it also owns `requested`/`uploaded`), and only
        // the first pane runs it so a multi-pane frame doesn't evict what a later pane needs.
        // Eviction runs once per frame, on the last pane — after every pane has promoted its own
        // visible tiles, so a multi-pane frame can't evict what a pane it already drew still needs.
        let drop_tiles = if last {
            self.tiles.evict_excess()
        } else {
            Vec::new()
        };
        let drop_vector_tiles = if first {
            self.vtiles.touch_visible(&visible_vector_tiles);
            self.vtiles.take_evicted()
        } else {
            Vec::new()
        };
        let (new_tiles, new_vector_tiles) = if first {
            let nt = self.tiles.drain_ready();
            // Drain vtiles regardless of basemap (it populates the label cache); only upload the
            // geometry to the GPU when a vector basemap will actually draw it.
            let nv = self.vtiles.drain_ready();
            if !nt.is_empty() || !nv.is_empty() {
                ctx.request_repaint();
            }
            (nt, if is_vector { nv } else { Vec::new() })
        } else {
            (Vec::new(), Vec::new())
        };

        // --- Radar (this pane's product, its own volume) ---
        self.map_3d_controls(idx, prect, ctx);
        let (radar_upload, mut draw_radar) = self.pane_radar(idx, idx);
        // A flashing range needs a frame at its next beat, even with nothing else moving.
        if self.views[idx].flash_ranges.iter().any(Option::is_some) {
            ctx.request_repaint_after(std::time::Duration::from_millis(
                FLASH_BEAT_MS - (chrono::Utc::now().timestamp_millis() as u64 % FLASH_BEAT_MS),
            ));
        }
        if self.trail_more {
            ctx.request_repaint();
        }
        let (observed_upload, mut draw_observed) = self.pane_observed_radar(idx, idx);
        if draw_observed {
            draw_radar = false;
        }
        // In the forecast-scrub tail there's no observed volume — show the HRRR field instead.
        if self.views[idx].timeline.forecast_hour().is_some() {
            draw_radar = false;
            draw_observed = false;
        }

        // Field layers: upload freshly-fetched grids on the first pane; every pane draws the
        // currently-enabled layers.
        // Field textures are evicted the way tiles are: decided here, where the state that knows
        // whether a re-upload will follow lives, and handed to the renderer to free.
        let drop_fields: Vec<crate::render::FieldLayer> = if first {
            let now = Instant::now();
            let on: std::collections::HashSet<crate::render::FieldLayer> = self
                .views
                .iter()
                .flat_map(|v| v.fields_on.iter().copied())
                .collect();
            let mut drop = Vec::new();
            for (layer, st) in self.fields.iter_mut() {
                if on.contains(layer) {
                    st.off_since = None;
                } else if let Some(since) = st.off_since {
                    if now.duration_since(since) >= FIELD_EVICT {
                        st.off_since = None;
                        // The grid is gone from the GPU, so the next enable must re-fetch it
                        // rather than trust its refresh cadence.
                        st.last_fetch = None;
                        st.grid = None;
                        drop.push(*layer);
                    }
                } else {
                    st.off_since = Some(now);
                }
            }
            drop
        } else {
            Vec::new()
        };
        if drop_fields.contains(&crate::render::FieldLayer::Ensemble) {
            // Thirty-one grids are worth freeing along with the texture.
            self.ensemble_run = None;
            self.ensemble_grid = None;
            self.ensemble_key = None;
            self.ensemble_display_key = None;
        }
        if drop_fields.contains(&crate::render::FieldLayer::ModelDiff) {
            self.diff_grid = None;
            self.diff_valid = None;
            self.diff_display_key = None;
        }
        let compare_visible = self.views.iter().any(|view| {
            view.fields_on
                .contains(&crate::render::FieldLayer::CompareA)
                || view
                    .fields_on
                    .contains(&crate::render::FieldLayer::CompareB)
        });
        if !compare_visible
            && drop_fields.iter().any(|layer| {
                matches!(
                    layer,
                    crate::render::FieldLayer::CompareA | crate::render::FieldLayer::CompareB
                )
            })
        {
            self.compare_grid = None;
            self.compare_valid = None;
        }
        if !drop_fields.is_empty() {
            use crate::render::FieldLayer as FL;
            let mut requests = self
                .overlay_requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for &layer in &drop_fields {
                // CompareA/CompareB share one fetch and therefore one request-health lane.
                let health_layer = if layer == FL::CompareB {
                    FL::CompareA
                } else {
                    layer
                };
                requests.set_cache_resident(&RequestLane::Field(health_layer), false);
            }
        }
        let field_uploads: Vec<(crate::render::FieldLayer, crate::render::MrmsUpload)> = if first {
            self.fields
                .iter_mut()
                .filter_map(|(k, s)| s.pending.take().map(|u| (*k, u)))
                .collect()
        } else {
            Vec::new()
        };
        // Bottom to top, in the user's paint order: the renderer paints this list as given.
        let order = crate::render::FieldLayer::paint_order(&self.settings.field_order);
        let mut on: Vec<crate::render::FieldLayer> =
            self.views[idx].fields_on.iter().copied().collect();
        on.sort_by_key(|l| order.iter().position(|o| o == l));
        let field_draws: Vec<(crate::render::FieldLayer, f32)> = on
            .iter()
            .filter(|layer| {
                crate::fielddiff::layer_ready(**layer, self.diff_valid, self.compare_valid)
                    && self.mrms_ready(**layer)
            })
            .map(|k| {
                let configured = self.settings.field_opacity.get(k).copied().unwrap_or(1.0);
                (
                    *k,
                    field_draw_opacity(*k, configured, self.views[idx].overlay_compare),
                )
            })
            .collect();

        let cam = self.views[idx].camera;
        // The pitched-map "Smooth" 3D volume takes over the pane entirely when it has something
        // resident to draw — it fully occludes the flat radar plane and the observed-gates cloud
        // underneath it, the same way `draw_observed` already wins over `draw_radar` above.
        let smooth_volume = self.pane_smooth_volume(idx, idx, ctx, &cam, vp);
        if smooth_volume.is_some() {
            draw_radar = false;
            draw_observed = false;
        }
        let (center, scale) = cam.world_to_clip_uniform(vp);
        let (wind_upload, wind) = if cam.is_3d() {
            // The particle compositor is a screen-space trail buffer and cannot be pitched without
            // smearing history across the map. Hide it in 3D until that buffer is reprojected.
            (None, None)
        } else {
            self.wind_gpu_frame(idx, &cam, vp)
        };
        let cb = MapCallback {
            pane: idx as u32,
            camera_center: center,
            camera_scale: scale,
            world_per_pixel: cam.world_per_pixel() as f32,
            camera_view_proj: cam.view_projection_uniform(vp),
            camera_3d: if cam.is_3d() { 1.0 } else { 0.0 },
            camera_globe: cam.globe_uniform(vp),
            new_tiles,
            visible,
            basemap_key: pane_style.key(),
            vector_over_raster: pane_style == BasemapStyle::HybridSatellite,
            radar_upload,
            draw_radar,
            observed_upload,
            draw_observed,
            overlay_upload: if first {
                self.pending_overlay.take()
            } else {
                None
            },
            draw_overlay: self.overlay_ready,
            field_uploads,
            field_draws,
            field_swipe: self.views[idx]
                .swipe_compare
                .then_some(crate::render::FieldSwipe {
                    left: crate::render::FieldLayer::CompareA,
                    right: crate::render::FieldLayer::CompareB,
                    fraction: self.views[idx].swipe_fraction,
                }),
            drop_fields,
            clear_tiles,
            drop_tiles,
            new_vector_tiles,
            visible_vector,
            clear_vector,
            drop_vector_tiles,
            wind_upload,
            wind,
        };
        ui.painter()
            .add(egui_wgpu::Callback::new_paint_callback(prect, cb));
        // A second, independent paint callback rather than a field on `MapCallback`: it is its own
        // pipeline and its own per-pane GPU resources (`MapVolume3dResources`), keyed by a
        // different type than `RenderResources` in the same `CallbackResources` map, and it draws
        // strictly after (so on top of) the flat map `cb` just queued — the raymarched volume has
        // to composite over the basemap/tiles, never under them.
        if let Some((upload, uniform)) = smooth_volume {
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                prect,
                crate::render3d::MapVolume3dCallback {
                    pane: idx as u32,
                    upload,
                    uniform,
                },
            ));
        }

        // Per-pane product picker (multi-pane only): set THIS pane's moment directly, without
        // clicking to activate it first. Single-pane keeps using the product pill.
        if self.views.len() > 1 && !self.obs_mode {
            let cur = self.views[idx].moment;
            // Same union as the sidebar uses, so this picker doesn't blink either.
            let have = self.views[idx].moments();
            let product = self.views[idx].user_product.clone();
            egui::Area::new(egui::Id::new(("pane_product", idx)))
                .order(egui::Order::Foreground)
                .fixed_pos(prect.left_top() + egui::vec2(6.0, 6.0))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(egui::Margin::symmetric(4, 2))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for m in Moment::ALL.into_iter().filter(|m| have[m.index()]) {
                                    let on = m == cur && product.is_none();
                                    if ui.selectable_label(on, m.short_name()).clicked() {
                                        self.views[idx].moment = m;
                                        self.views[idx].user_product = None;
                                        self.views[idx].product_range = None;
                                        self.active = idx;
                                    }
                                }
                                if let Some(name) = &product {
                                    ui.selectable_label(true, name.as_str()).on_hover_text(
                                        "A user product, shown in place of the moment",
                                    );
                                }
                            });
                        });
                });
        }

        // Optical-flow nowcast points (needs &mut self to bin the sweep; done before the &view borrow).
        let nowcast_pts = if self.filters.show_nowcast && idx == self.active {
            self.compute_nowcast(idx)
        } else {
            Vec::new()
        };
        // A rule that watches a signature has to drive its detector even with the layer off —
        // otherwise arming "hail spike near home" and then hiding the layer silently disarms it.
        // The hits are computed here either way; only the drawing below checks the layer flag.
        let armed = |t: &crate::settings::RuleTrigger| {
            self.settings
                .alert_rules
                .iter()
                .any(|r| r.enabled && &r.trigger == t)
        };
        // One detection per tornado (`wxdata::tornado_id::circulations`), when any of the three
        // tornado layers is on: it reads both detectors, as Tornado ID does.
        let merge = self.settings.merge_tornado_signals
            && (self.filters.show_tornado_id
                || self.filters.show_tds
                || self.filters.show_couplets);
        // Tornado ID reads both detectors, whether or not their own layers are shown.
        let want_tds = self.filters.show_tds
            || self.filters.show_tornado_id
            || merge
            || armed(&crate::settings::RuleTrigger::Tds);
        let want_tbss = self.filters.show_tbss || armed(&crate::settings::RuleTrigger::Tbss);
        let want_zdr =
            self.filters.show_zdr_columns || armed(&crate::settings::RuleTrigger::ZdrColumn);
        let want_couplets = self.filters.show_couplets
            || self.filters.show_tornado_id
            || merge
            || armed(&crate::settings::RuleTrigger::Rotation);
        let tds_hits = if want_tds && idx == self.active {
            self.compute_tds(idx)
        } else {
            Vec::new()
        };
        // Score history for the hover sparkline below — same "only for the active pane, and only
        // when the layer is actually wanted" gating as `tds_hits` itself, since it costs a replay
        // over `tds_shown_cache` even on a cache hit's cheap path.
        let tds_score_tracks = if want_tds && idx == self.active {
            self.compute_tds_score_track(idx)
        } else {
            Vec::new()
        };
        let tbss_hits = if want_tbss && idx == self.active {
            self.compute_tbss(idx)
        } else {
            Vec::new()
        };
        let zdr_hits = if want_zdr && idx == self.active {
            self.compute_zdr_columns(idx, ctx)
        } else {
            Vec::new()
        };
        let couplets = if want_couplets && idx == self.active {
            self.compute_couplets(idx)
        } else {
            Vec::new()
        };
        let rot_score_tracks = if want_couplets && idx == self.active {
            self.compute_rot_score_track(idx)
        } else {
            Vec::new()
        };
        let local_tracks = if self.show_local_tracks && idx == self.active {
            self.compute_local_tracks()
        } else {
            Vec::new()
        };
        if idx == self.active {
            self.check_rain_arrival();
            self.evaluate_scan_rules(idx, &tds_hits, &tbss_hits, &zdr_hits, &couplets);
        }
        let tornado_ids = if self.filters.show_tornado_id && idx == self.active && !merge {
            wxdata::tornado_id::identify(&couplets, &tds_hits)
        } else {
            Vec::new()
        };
        // Merged: every rotation and debris detection near a tornado is drawn as part of that
        // tornado's one marker, not on its own; the full lists stay for the web it opens into.
        let circulations = if merge && idx == self.active {
            wxdata::tornado_id::circulations(&couplets, &tds_hits)
        } else {
            Vec::new()
        };
        let mut tied_couplet = vec![false; couplets.len()];
        let mut tied_tds = vec![false; tds_hits.len()];
        for c in &circulations {
            for m in &c.members {
                match m.evidence {
                    wxdata::tornado_id::Evidence::Rotation(i) => tied_couplet[i] = true,
                    wxdata::tornado_id::Evidence::Debris(i) => tied_tds[i] = true,
                }
            }
        }
        let (all_couplets, all_tds) = (couplets.clone(), tds_hits.clone());
        // Hidden layers computed only for a rule must not also be drawn.
        let tds_hits = if self.filters.show_tds {
            tds_hits
        } else {
            Vec::new()
        };
        let tbss_hits = if self.filters.show_tbss {
            tbss_hits
        } else {
            Vec::new()
        };
        let zdr_hits = if self.filters.show_zdr_columns {
            zdr_hits
        } else {
            Vec::new()
        };
        let couplets = if self.filters.show_couplets {
            couplets
        } else {
            Vec::new()
        };

        // --- Painter overlays (clipped to this pane) ---
        let painter = ui.painter_at(prect);
        let view = &self.views[idx];
        let basemap = pane_style;

        // A real partial Level II chunk just refreshed the displayed tilt: sweep a keyed lime
        // line through exactly that chunk's azimuth sector. It is deliberately absent for 3D,
        // archive playback, other tilts, completed-volume polling, and a stalled/ended stream.
        if !view.map_3d.enabled && view.show_radar && view.timeline.following {
            let live = view
                .live_progress
                .zip(view.live_progress_at)
                .zip(
                    view.volume
                        .as_ref()
                        .and_then(|volume| volume.elevations.get(view.tilt))
                        .copied(),
                )
                .filter(|((progress, _), elevation)| {
                    live_progress_matches_tilt(*progress, *elevation)
                })
                .and_then(|((progress, received), _)| {
                    live_sweep_frame(
                        progress,
                        received.elapsed().as_secs_f32(),
                        crate::ui::motion::reduced(),
                    )
                });
            if let (Some(frame), Some(site)) = (
                live,
                view.site.as_deref().and_then(wxdata::sites::site_by_id),
            ) {
                paint_live_sweep(
                    &painter,
                    prect,
                    cam,
                    vp,
                    [site.longitude as f64, site.latitude as f64],
                    frame,
                );
                if crate::ui::motion::reduced() {
                    ctx.request_repaint_after(std::time::Duration::from_secs_f32(
                        frame.remaining_secs.max(0.01),
                    ));
                } else {
                    ctx.request_repaint();
                }
            }
        }

        if view.swipe_compare {
            let split_x = prect.left() + prect.width() * view.swipe_fraction;
            let top = prect.top() + 30.0;
            let bottom = prect.bottom() - 8.0;
            let stroke = egui::Stroke::new(2.0, egui::Color32::WHITE);
            painter.line_segment(
                [egui::pos2(split_x, top), egui::pos2(split_x, bottom)],
                egui::Stroke::new(4.0, egui::Color32::from_black_alpha(150)),
            );
            painter.line_segment(
                [egui::pos2(split_x, top), egui::pos2(split_x, bottom)],
                stroke,
            );
            let handle = egui::pos2(split_x, prect.center().y);
            painter.circle_filled(handle, 10.0, egui::Color32::from_black_alpha(190));
            painter.circle_stroke(handle, 10.0, stroke);
            painter.text(
                handle,
                egui::Align2::CENTER_CENTER,
                "↔",
                egui::FontId::proportional(14.0),
                egui::Color32::WHITE,
            );
            let (a, b) = self.diff_field.pair();
            for (pos, label, align) in [
                (
                    egui::pos2((prect.left() + split_x) * 0.5, prect.top() + 12.0),
                    format!("A · {a}"),
                    egui::Align2::CENTER_CENTER,
                ),
                (
                    egui::pos2((split_x + prect.right()) * 0.5, prect.top() + 12.0),
                    format!("B · {b}"),
                    egui::Align2::CENTER_CENTER,
                ),
            ] {
                painter.text(
                    pos,
                    align,
                    label,
                    egui::FontId::proportional(12.0),
                    egui::Color32::WHITE,
                );
            }
        }

        // Storm-cell ids reserve their space first — they are the top tier, and the cell markers
        // themselves are drawn much further down with the rest of the cell layer. Reserving here
        // and drawing there is the whole reason the placer separates the two: a warning label
        // must not lose its slot to a town name that merely happened to be painted earlier.
        let cell_labels_shown: std::collections::HashSet<String> = if self.filters.show_cells
            && self.cells_site.as_deref() == view.site.as_deref()
        {
            let ids: Vec<(String, egui::Pos2)> = self
                .active_storm_cells()
                .iter()
                .filter(|c| c.kind == CellKind::Storm && !c.id.is_empty())
                .map(|c| {
                    let w = crate::render::mercator::lonlat_to_world(c.lon, c.lat);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    (
                        c.id.clone(),
                        egui::pos2(prect.left() + sx, prect.top() + sy),
                    )
                })
                .collect();
            ids.into_iter()
                .filter(|(_, p)| prect.contains(*p))
                .filter(|(id, p)| {
                    // Matches the draw below: 11 pt text, left-bottom anchored, up and right
                    // of the marker.
                    let anchor = *p + egui::vec2(8.0, -8.0);
                    let size = egui::vec2(id.len() as f32 * 6.5, 13.0);
                    let rect =
                        egui::Rect::from_min_size(egui::pos2(anchor.x, anchor.y - size.y), size)
                            .expand(2.0);
                    self.labels.place(
                        crate::labelplace::key(id),
                        rect,
                        crate::labelplace::Priority::Warning,
                    )
                })
                .map(|(id, _)| id)
                .collect()
        } else {
            std::collections::HashSet::new()
        };
        let view = &self.views[idx];

        // City/town labels, overlaid on every basemap. On raster (satellite) the baked-in labels
        // are faint over imagery + echoes, so we draw crisp white text with a solid black halo;
        // vector basemaps use their palette's label colors. Bigger fonts + an 8-way halo read well.
        if !vlabels.is_empty() {
            let (text_col, halo_col, big) = if is_vector {
                let st = crate::basemap_style::style(basemap.vector_palette().unwrap_or_default());
                (
                    egui::Color32::from_rgb(st.label[0], st.label[1], st.label[2]),
                    egui::Color32::from_rgb(st.label_halo[0], st.label_halo[1], st.label_halo[2]),
                    13.0,
                )
            } else {
                (
                    egui::Color32::WHITE,
                    egui::Color32::from_black_alpha(235),
                    14.5,
                )
            };
            let z = cam.zoom;
            // Repeat route shields from regional zoom; collision placement still prevents overlap.
            let repeat_shields = z >= 5.0;
            let mut labels: Vec<&crate::vector_tiles::PlaceLabel> =
                vlabels.iter().filter(|l| l.visible_at(z)).collect();
            let label_key = |l: &crate::vector_tiles::PlaceLabel| {
                let key = crate::labelplace::key(&l.name);
                if repeat_shields && l.shield != crate::vector_tiles::RoadShield::None {
                    key ^ ((l.world[0].to_bits() as u64) << 32) ^ l.world[1].to_bits() as u64
                } else {
                    key
                }
            };
            // Labels already on screen are offered their slot before newcomers of the same
            // importance; without that a name at the edge of a collision wins and loses on
            // alternate frames, which is exactly the flicker you see while panning.
            labels.sort_by_key(|l| (l.priority(), !self.labels.was_shown(label_key(l)), l.rank));
            let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
            // 8-way halo (cardinals + diagonals) for a solid, readable outline.
            const HALO: [egui::Vec2; 8] = [
                egui::vec2(1.2, 0.0),
                egui::vec2(-1.2, 0.0),
                egui::vec2(0.0, 1.2),
                egui::vec2(0.0, -1.2),
                egui::vec2(1.0, 1.0),
                egui::vec2(1.0, -1.0),
                egui::vec2(-1.0, 1.0),
                egui::vec2(-1.0, -1.0),
            ];
            let mut placed_shields: Vec<(&str, crate::vector_tiles::RoadShield, egui::Pos2)> =
                Vec::new();
            for l in labels {
                if (l.shield == crate::vector_tiles::RoadShield::None || !repeat_shields)
                    && !seen.insert(l.name.as_str())
                {
                    continue;
                }
                let (sx, sy) = cam.world_to_screen((l.world[0] as f64, l.world[1] as f64), vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(p) {
                    continue;
                }
                if l.shield != crate::vector_tiles::RoadShield::None {
                    use crate::vector_tiles::RoadShield;
                    let (height, pad, text_color) = match l.shield {
                        RoadShield::Interstate => (25.0, 10.0, egui::Color32::WHITE),
                        RoadShield::Us => (22.0, 11.0, egui::Color32::BLACK),
                        RoadShield::State => (19.0, 9.0, egui::Color32::BLACK),
                        RoadShield::Other => (17.0, 7.0, egui::Color32::BLACK),
                        RoadShield::None => unreachable!(),
                    };
                    let galley = painter.layout_no_wrap(
                        l.name.clone(),
                        egui::FontId::proportional(big - 2.5),
                        text_color,
                    );
                    let r = egui::Rect::from_center_size(
                        p,
                        egui::vec2((galley.size().x + pad).max(height), height),
                    );
                    if placed_shields.iter().any(|(name, shield, position)| {
                        *name == l.name
                            && *shield == l.shield
                            && position.distance(p) < if z < 8.0 { 160.0 } else { 220.0 }
                    }) {
                        continue;
                    }
                    if !self.labels.place(
                        label_key(l),
                        r.expand(3.0),
                        crate::labelplace::Priority::Place,
                    ) {
                        continue;
                    }
                    placed_shields.push((&l.name, l.shield, p));
                    match l.shield {
                        RoadShield::Interstate => {
                            let shield = |rect: egui::Rect| {
                                vec![
                                    egui::pos2(rect.left() + 3.0, rect.top() + 2.0),
                                    egui::pos2(rect.center().x, rect.top()),
                                    egui::pos2(rect.right() - 3.0, rect.top() + 2.0),
                                    egui::pos2(rect.right(), rect.top() + 7.0),
                                    egui::pos2(rect.right() - 1.0, rect.bottom() - 7.0),
                                    egui::pos2(rect.right() - 4.0, rect.bottom() - 3.0),
                                    egui::pos2(rect.center().x, rect.bottom()),
                                    egui::pos2(rect.left() + 4.0, rect.bottom() - 3.0),
                                    egui::pos2(rect.left() + 1.0, rect.bottom() - 7.0),
                                    egui::pos2(rect.left(), rect.top() + 7.0),
                                ]
                            };
                            painter.add(egui::Shape::convex_polygon(
                                shield(r),
                                egui::Color32::WHITE,
                                egui::Stroke::NONE,
                            ));
                            let inner = r.shrink(1.2);
                            painter.add(egui::Shape::convex_polygon(
                                shield(inner),
                                egui::Color32::from_rgb(38, 67, 145),
                                egui::Stroke::NONE,
                            ));
                            painter.add(egui::Shape::convex_polygon(
                                vec![
                                    egui::pos2(inner.left() + 1.0, inner.top() + 6.5),
                                    egui::pos2(inner.left() + 3.0, inner.top() + 2.0),
                                    egui::pos2(inner.center().x, inner.top()),
                                    egui::pos2(inner.right() - 3.0, inner.top() + 2.0),
                                    egui::pos2(inner.right() - 1.0, inner.top() + 6.5),
                                ],
                                egui::Color32::from_rgb(190, 37, 48),
                                egui::Stroke::NONE,
                            ));
                            painter.line_segment(
                                [
                                    egui::pos2(inner.left() + 1.0, inner.top() + 7.0),
                                    egui::pos2(inner.right() - 1.0, inner.top() + 7.0),
                                ],
                                egui::Stroke::new(1.2, egui::Color32::WHITE),
                            );
                        }
                        RoadShield::Us => {
                            let badge = |rect: egui::Rect| {
                                vec![
                                    egui::pos2(rect.left() + 4.0, rect.top()),
                                    egui::pos2(rect.right() - 4.0, rect.top()),
                                    egui::pos2(rect.right(), rect.top() + 5.0),
                                    egui::pos2(rect.right() - 2.0, rect.bottom() - 4.0),
                                    egui::pos2(rect.center().x, rect.bottom()),
                                    egui::pos2(rect.left() + 2.0, rect.bottom() - 4.0),
                                    egui::pos2(rect.left(), rect.top() + 5.0),
                                ]
                            };
                            painter.add(egui::Shape::convex_polygon(
                                badge(r),
                                egui::Color32::BLACK,
                                egui::Stroke::NONE,
                            ));
                            painter.add(egui::Shape::convex_polygon(
                                badge(r.shrink(1.3)),
                                egui::Color32::WHITE,
                                egui::Stroke::NONE,
                            ));
                        }
                        RoadShield::State => {
                            painter.rect_filled(r, height * 0.5, egui::Color32::BLACK);
                            painter.rect_filled(r.shrink(1.2), height * 0.5, egui::Color32::WHITE);
                        }
                        RoadShield::Other => {
                            painter.rect_filled(r, 2.0, egui::Color32::BLACK);
                            painter.rect_filled(r.shrink(1.0), 1.5, egui::Color32::WHITE);
                        }
                        RoadShield::None => unreachable!(),
                    }
                    painter.galley_with_override_text_color(
                        egui::pos2(
                            r.center().x - galley.size().x * 0.5,
                            r.center().y - galley.size().y * 0.5
                                + if l.shield == RoadShield::Interstate {
                                    2.8
                                } else {
                                    0.0
                                },
                        ),
                        galley,
                        text_color,
                    );
                    continue;
                }
                let font = egui::FontId::proportional(if l.city { big } else { big - 2.5 });
                let galley = painter.layout_no_wrap(l.name.clone(), font, text_col);
                let r = egui::Rect::from_min_size(p, galley.size()).expand(4.0);
                if !self
                    .labels
                    .place(label_key(l), r, crate::labelplace::Priority::Place)
                {
                    continue;
                }
                // One layout per label, reused for all nine draws. `painter.text` would lay the
                // string out again every time, which at eight halo offsets meant ten text
                // layouts per visible place name, every frame.
                for off in HALO {
                    painter.galley_with_override_text_color(p + off, galley.clone(), halo_col);
                }
                painter.galley_with_override_text_color(p, galley, text_col);
            }
            // OpenMapTiles/OpenStreetMap credit for the label data (raster imagery is credited below).
            painter.text(
                egui::pos2(prect.left() + 6.0, prect.bottom() - 18.0),
                egui::Align2::LEFT_BOTTOM,
                "© OpenMapTiles © OpenStreetMap",
                egui::FontId::proportional(10.0),
                egui::Color32::from_gray(200).gamma_multiply(0.55),
            );
        }

        // Raster basemap attribution (provider styles + USGS satellite).
        if pane_style.is_raster() {
            let col = egui::Color32::from_gray(200).gamma_multiply(0.6);
            painter.text(
                egui::pos2(prect.left() + 6.0, prect.bottom() - 4.0),
                egui::Align2::LEFT_BOTTOM,
                if pane_style == BasemapStyle::CustomXyz
                    && !self.settings.custom_tile_attribution.is_empty()
                {
                    self.settings.custom_tile_attribution.as_str()
                } else {
                    pane_style.attribution()
                },
                egui::FontId::proportional(10.0),
                col,
            );
        }

        // Radar-data attribution, opposite the basemap credit so neither has to know the other's
        // width. Only sources whose licence asks for a credit line get an arm.
        if let Some(credit) = view.site.as_deref().and_then(data_attribution) {
            painter.text(
                egui::pos2(prect.right() - 6.0, prect.bottom() - 4.0),
                egui::Align2::RIGHT_BOTTOM,
                credit,
                egui::FontId::proportional(10.0),
                egui::Color32::from_gray(200).gamma_multiply(0.6),
            );
        }

        // GOES lightning: one dot per flash, fading as it ages.
        if self.show_glm {
            if let Ok(feed) = self.glm.lock() {
                // Live, the feed aged against now; scrubbed back, the archive window aged
                // against the view's time (ROADMAP_NEW E6).
                let (flashes, now) = glm_flashes_for(
                    self.view_target_time(),
                    feed.flashes(),
                    self.glm_archive.as_ref(),
                    chrono::Utc::now(),
                );
                // Reject off-screen flashes in lon/lat before projecting each one: a GLM feed can
                // carry tens of thousands of flashes while the pane shows a corner of one state.
                let corner = |px: (f32, f32)| {
                    let w = cam.screen_to_world(px, vp);
                    crate::render::mercator::world_to_lonlat(w.0, w.1)
                };
                let (c0, c1) = (corner((0.0, 0.0)), corner((vp.0, vp.1)));
                let (lon_lo, lon_hi) = (c0.0.min(c1.0), c0.0.max(c1.0));
                let (lat_lo, lat_hi) = (c0.1.min(c1.1), c0.1.max(c1.1));
                for f in flashes {
                    if f.lon < lon_lo || f.lon > lon_hi || f.lat < lat_lo || f.lat > lat_hi {
                        continue;
                    }
                    let w = crate::render::mercator::lonlat_to_world(f.lon, f.lat);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                    if !prect.contains(p) {
                        continue;
                    }
                    let age = (now - f.time).num_seconds().max(0) as f32;
                    let (col, r) = glm_style(age);
                    painter.circle_filled(p, r, col);
                }
            }
        }

        // Ground strikes off the broker. Same shape as the GLM block above, including the
        // lon/lat prefilter, because the deque is just as long and the pane just as small.
        if self.show_strikes && !self.strikes.is_empty() {
            let now = chrono::Utc::now();
            let corner = |px: (f32, f32)| {
                let w = cam.screen_to_world(px, vp);
                crate::render::mercator::world_to_lonlat(w.0, w.1)
            };
            let (c0, c1) = (corner((0.0, 0.0)), corner((vp.0, vp.1)));
            let (lon_lo, lon_hi) = (c0.0.min(c1.0), c0.0.max(c1.0));
            let (lat_lo, lat_hi) = (c0.1.min(c1.1), c0.1.max(c1.1));
            for &(lon, lat, time) in &self.strikes {
                if lon < lon_lo || lon > lon_hi || lat < lat_lo || lat > lat_hi {
                    continue;
                }
                let w = crate::render::mercator::lonlat_to_world(lon, lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(p) {
                    continue;
                }
                let age = (now - time).num_seconds().max(0) as f32;
                let (col, r) = strike_style(age);
                painter.circle_filled(p, r, col);
            }
        }

        // Animated wind particles, when they are being drawn on the CPU. The GPU path draws
        // inside the map callback instead (see `wind_gpu_frame`), which is also what puts it
        // under the warning polygons rather than over them.
        if self.show_wind && !self.wind_on_gpu {
            let alpha = self.wind_alpha(idx, cam.zoom);
            // Split borrow: the grids and the per-pane particle sets are disjoint fields, and the
            // advection needs both at once.
            let Self {
                wind,
                wind_particles,
                wind_dt,
                ..
            } = self;
            if let (Some(field), true) = (wind.as_ref(), alpha > 0.01) {
                let ps = wind_particles.entry(idx).or_default();
                ps.update(field, &cam, vp, *wind_dt);
                let mesh = ps.build_mesh(&cam, vp, prect.left_top(), alpha);
                if !mesh.is_empty() {
                    painter.add(egui::Shape::mesh(mesh));
                }
            }
        }

        // Surface analysis: fronts with their pips, plus H/L centers.
        if self.show_fronts {
            if let Some(a) = &self.fronts {
                let to_screen = |lon: f64, lat: f64| {
                    let w = crate::render::mercator::lonlat_to_world(lon, lat);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    egui::pos2(prect.left() + sx, prect.top() + sy)
                };
                crate::fronts_draw::draw(&painter, a, prect, to_screen);
            }
        }

        // Where the chosen GOES mesoscale sector is pointed (ROADMAP_NEW E5), while a GOES layer
        // reads it: its box, following the sector as NOAA moves it, and whether this view is in it.
        if let Some((sector, fp)) = self
            .goes_footprint
            .filter(|(s, _)| *s == self.settings.goes_sector && s.is_meso())
            .filter(|_| self.goes_layers_on())
        {
            let to_screen = |lon: f64, lat: f64| {
                let w = crate::render::mercator::lonlat_to_world(lon, lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                egui::pos2(prect.left() + sx, prect.top() + sy)
            };
            let corners = [
                to_screen(fp.lon_west, fp.lat_north),
                to_screen(fp.lon_east, fp.lat_north),
                to_screen(fp.lon_east, fp.lat_south),
                to_screen(fp.lon_west, fp.lat_south),
            ];
            let color = egui::Color32::from_rgb(255, 214, 90);
            let mut ring = corners.to_vec();
            ring.push(corners[0]);
            painter.extend(egui::Shape::dashed_line(
                &ring,
                egui::Stroke::new(1.5, color),
                8.0,
                5.0,
            ));
            let reading = self.goes_sector_now() == sector;
            // At the bottom-left corner: the top-left is where the layer's colour scale sits.
            painter.text(
                corners[3] + egui::vec2(4.0, -4.0),
                egui::Align2::LEFT_BOTTOM,
                format!(
                    "{} {} \u{b7} {}{}",
                    if self.settings.goes_satellite_west {
                        "GOES-West"
                    } else {
                        "GOES-East"
                    },
                    match sector {
                        wxdata::goes_abi::Sector::Meso2 => "Meso 2",
                        _ => "Meso 1",
                    },
                    fp.time.format("%H:%MZ"),
                    if reading {
                        ""
                    } else {
                        " (view outside: CONUS)"
                    }
                ),
                egui::FontId::proportional(11.0),
                color,
            );
        }

        // HRRR model contours (MSLP / 2 m temp / dewpoint / CAPE / SRH / …): labeled isolines plus
        // one stacked banner per active kind. Several can be on at once, each independently
        // fetched and colored — overlaying, say, MSLP and CAPE together rather than one exclusive
        // choice.
        if !self.active_contours.is_empty() {
            let to_screen = |lon: f64, lat: f64| {
                let w = crate::render::mercator::lonlat_to_world(lon, lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                egui::pos2(prect.left() + sx, prect.top() + sy)
            };
            // This pane's lon/lat bounds, for culling lines by their precomputed bbox BEFORE
            // projecting any points — CAPE/SRH carry thousands of small rings.
            let (vmin_lon, vmin_lat, vmax_lon, vmax_lat) = {
                use crate::render::mercator::world_to_lonlat;
                let (wx0, wy0) = cam.screen_to_world((0.0, 0.0), vp);
                let (wx1, wy1) = cam.screen_to_world((vp.0, vp.1), vp);
                let (lon0, lat0) = world_to_lonlat(wx0, wy0);
                let (lon1, lat1) = world_to_lonlat(wx1, wy1);
                (
                    lon0.min(lon1),
                    lat0.min(lat1),
                    lon0.max(lon1),
                    lat0.max(lat1),
                )
            };
            let mut banner_row = 0;
            for kind in self.active_contours.clone() {
                let Some(entry) = self.contours.get(&kind) else {
                    continue;
                };
                if entry.lines.is_empty() {
                    continue;
                }
                let col = kind.color();
                for line in &entry.lines {
                    let (bx0, by0, bx1, by1) = line.bbox;
                    if bx1 < vmin_lon || bx0 > vmax_lon || by1 < vmin_lat || by0 > vmax_lat {
                        continue; // fully off-view
                    }
                    let pts: Vec<egui::Pos2> = line
                        .pts
                        .iter()
                        .map(|&(lon, lat)| to_screen(lon, lat))
                        .collect();
                    // Label the longest segment's midpoint when the line spans enough pixels.
                    let seg = longest_segment(&pts);
                    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.2, col)));
                    if let Some((a, b)) = seg {
                        if a.distance(b) > 60.0 {
                            let mid = a + (b - a) * 0.5;
                            let txt = match kind.unit(self.settings.temp_unit) {
                                Some(unit) => format!("{:.0}{unit}", line.level),
                                None => format!("{:.0}", line.level),
                            };
                            let font = egui::FontId::proportional(11.0);
                            for dx in [-1.0, 1.0] {
                                for dy in [-1.0, 1.0] {
                                    painter.text(
                                        mid + egui::vec2(dx, dy),
                                        egui::Align2::CENTER_CENTER,
                                        &txt,
                                        font.clone(),
                                        egui::Color32::from_black_alpha(200),
                                    );
                                }
                            }
                            painter.text(mid, egui::Align2::CENTER_CENTER, &txt, font, col);
                        }
                    }
                }
                if idx == self.active {
                    let vt = entry
                        .valid
                        .map(|t| crate::timefmt::fmt_clock(t, self.active_tz(), false))
                        .unwrap_or_default();
                    let text = format!(
                        "HRRR {} contours — valid {vt}",
                        kind.display_label(self.settings.temp_unit)
                    );
                    let font = egui::FontId::proportional(12.0);
                    // Stack this kind's banner below any already drawn this frame rather than
                    // overlapping them.
                    let anchor = egui::pos2(
                        prect.left() + 8.0,
                        prect.top() + 40.0 + banner_row as f32 * 22.0,
                    );
                    banner_row += 1;
                    let galley =
                        painter.layout_no_wrap(text.clone(), font.clone(), egui::Color32::WHITE);
                    let bg =
                        egui::Rect::from_min_size(anchor, galley.size() + egui::vec2(10.0, 4.0));
                    painter.rect_filled(
                        bg,
                        3.0,
                        egui::Color32::from_rgba_unmultiplied(60, 90, 60, 200),
                    );
                    painter.text(
                        anchor + egui::vec2(5.0, 2.0),
                        egui::Align2::LEFT_TOP,
                        &text,
                        font,
                        egui::Color32::WHITE,
                    );
                }
            }
        }

        // The tornado markers' click targets are this frame's, or none (set again below).
        ui.ctx()
            .data_mut(|d| d.remove::<Vec<egui::Rect>>(egui::Id::new(("circulation_hits", idx))));
        // Storm-cell dots + SCIT forecast tracks, and the detector markers. The detectors are
        // drawn whether or not the storm cells are: Tornado ID on its own showed nothing.
        let cells_here =
            self.filters.show_cells && self.cells_site.as_deref() == view.site.as_deref();
        let detectors = !tds_hits.is_empty()
            || !tbss_hits.is_empty()
            || !zdr_hits.is_empty()
            || !couplets.is_empty()
            || !tornado_ids.is_empty()
            || !circulations.is_empty()
            || !nowcast_pts.is_empty()
            || !local_tracks.is_empty()
            || (self.filters.show_zdr_columns && idx == self.active);
        if cells_here || detectors {
            self.paint_cones_and_nowcast(&painter, prect, cam, vp, idx, cells_here, &nowcast_pts);
            self.paint_detector_markers(
                ui,
                detector_markers::Markers {
                    painter: &painter,
                    prect,
                    cam,
                    vp,
                    response: &response,
                    tds_hits: &tds_hits,
                    tbss_hits: &tbss_hits,
                    zdr_hits: &zdr_hits,
                    couplets: &couplets,
                    tornado_ids: &tornado_ids,
                    circulations: &circulations,
                    tied_tds: &tied_tds,
                    tied_couplet: &tied_couplet,
                    all_couplets: &all_couplets,
                    all_tds: &all_tds,
                    tds_score_tracks: &tds_score_tracks,
                    rot_score_tracks: &rot_score_tracks,
                },
                idx,
            );

            self.paint_cell_tracks(
                &painter,
                prect,
                cam,
                vp,
                idx,
                cells_here,
                &local_tracks,
                &cell_labels_shown,
            );
        }

        // Storm-report dots (live LSRs, or the archived window while scrubbed).
        if self.show_storm_reports {
            for r in self.active_storm_reports() {
                let w = crate::render::mercator::lonlat_to_world(r.lon, r.lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(p) {
                    continue;
                }
                let col = report_color(r.kind);
                let color = egui::Color32::from_rgba_unmultiplied(col[0], col[1], col[2], 255);
                // Small filled diamond so reports read distinctly from round storm-cell dots.
                let d = 4.0;
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        p + egui::vec2(0.0, -d),
                        p + egui::vec2(d, 0.0),
                        p + egui::vec2(0.0, d),
                        p + egui::vec2(-d, 0.0),
                    ],
                    color,
                    egui::Stroke::new(1.0, egui::Color32::from_black_alpha(160)),
                ));
            }
        }

        // AirNow monitors: a dot in the EPA category color with the AQI beside it.
        if self.show_aqi {
            for o in &self.aqi {
                let w = crate::render::mercator::lonlat_to_world(o.lon, o.lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(p) {
                    continue;
                }
                let c = o.color();
                painter.circle(
                    p,
                    4.0,
                    egui::Color32::from_rgb(c[0], c[1], c[2]),
                    egui::Stroke::new(1.0, egui::Color32::from_black_alpha(160)),
                );
                painter.text(
                    p + egui::vec2(6.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    o.aqi.to_string(),
                    egui::FontId::proportional(11.0),
                    egui::Color32::from_rgb(c[0], c[1], c[2]),
                );
            }
        }

        // Wildfire incident points (the perimeters ride the tessellated overlay layer).
        if self.show_fires {
            for f in &self.fire_incidents {
                let w = crate::render::mercator::lonlat_to_world(f.lon, f.lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(p) {
                    continue;
                }
                painter.circle(
                    p,
                    4.0,
                    egui::Color32::from_rgb(235, 110, 40),
                    egui::Stroke::new(1.0, egui::Color32::from_black_alpha(160)),
                );
            }
        }

        // Hurricane-hunter flight track: one dot per 30-second observation, colored by the
        // surface wind the SFMR measured (or flight-level wind when it reported nothing).
        if self.show_recon {
            for o in &self.recon {
                let w = crate::render::mercator::lonlat_to_world(o.lon, o.lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(p) {
                    continue;
                }
                let kt = o.sfmr_kt.or(o.wspd_kt).unwrap_or(0.0);
                let (_, c) = wxdata::tropical::saffir_simpson(kt);
                painter.circle_filled(p, 2.5, egui::Color32::from_rgb(c[0], c[1], c[2]));
                let hit = egui::Rect::from_center_size(p, egui::vec2(10.0, 10.0));
                if response.hover_pos().is_some_and(|hp| hit.contains(hp)) {
                    let fl = o
                        .wspd_kt
                        .map_or_else(|| "\u{2014}".into(), |v| format!("{v:.0} kt"));
                    let sfc = o
                        .sfmr_kt
                        .map_or_else(|| "\u{2014}".into(), |v| format!("{v:.0} kt"));
                    let mb = o
                        .press_mb
                        .map_or_else(|| "\u{2014}".into(), |v| format!("{v:.1} mb"));
                    response.clone().show_tooltip_text(format!(
                        "{} \u{2014} flight level {fl}, surface {sfc}\n{mb}",
                        o.mission
                    ));
                }
            }
        }

        // Pilot reports: a small triangle per report, filled when it carries a hazard so a
        // turbulence report stands out from a routine sky observation.
        if self.show_pireps {
            for r in &self.pireps {
                let w = crate::render::mercator::lonlat_to_world(r.lon, r.lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(p) {
                    continue;
                }
                let col = if r.urgent {
                    egui::Color32::from_rgb(235, 70, 70)
                } else if r.hazard.is_empty() {
                    egui::Color32::from_rgb(150, 165, 185)
                } else {
                    egui::Color32::from_rgb(240, 190, 50)
                };
                let d = 5.0;
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        p + egui::vec2(0.0, -d),
                        p + egui::vec2(d, d * 0.8),
                        p + egui::vec2(-d, d * 0.8),
                    ],
                    col,
                    egui::Stroke::new(1.0, egui::Color32::from_black_alpha(160)),
                ));
                // Hover → altitude, aircraft and the raw report, which is what pilots read.
                let hit = egui::Rect::from_center_size(p, egui::vec2(16.0, 16.0));
                if response.hover_pos().is_some_and(|hp| hit.contains(hp)) {
                    let alt = r
                        .alt_ft
                        .map_or_else(|| "—".to_string(), |a| format!("{a} ft"));
                    response.clone().show_tooltip_text(format!(
                        "{alt}  {}\n{}\n{}",
                        r.ac_type, r.hazard, r.raw
                    ));
                }
            }
        }

        // Crowd precipitation-type reports: a lettered dot per report, so the rain/snow line
        // reads straight off the map.
        if self.show_mping {
            for r in &self.mping_reports {
                let w = crate::render::mercator::lonlat_to_world(r.lon, r.lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(p) {
                    continue;
                }
                let c = r.precip.color();
                let color = egui::Color32::from_rgb(c[0], c[1], c[2]);
                painter.circle_filled(p, 5.0, color);
                painter.circle_stroke(p, 5.0, egui::Stroke::new(1.0, egui::Color32::BLACK));
                painter.text(
                    p,
                    egui::Align2::CENTER_CENTER,
                    r.precip.glyph(),
                    egui::FontId::proportional(8.0),
                    egui::Color32::BLACK,
                );
                let hit = egui::Rect::from_center_size(p, egui::vec2(14.0, 14.0));
                if response.hover_pos().is_some_and(|hp| hit.contains(hp)) {
                    let tz = self.settings.tz_for(self.views[idx].site.as_deref());
                    response.clone().show_tooltip_text(format!(
                        "{}\n{}",
                        r.description,
                        crate::timefmt::fmt_clock(r.time, tz, false)
                    ));
                }
            }
        }

        // Spotter Network positions, filtered to within Level-II range of this pane's site.
        // FAA camera sites: a small camera-blue dot per airport, named once you're close enough
        // to tell them apart. Clicking one opens its newest frame (see `open_webcam`).
        if self.show_webcams {
            let show_labels = cam.zoom >= 8.0;
            let col = egui::Color32::from_rgb(110, 180, 240);
            // A camera under a tornado or severe-thunderstorm warning is the one worth opening,
            // and it looks exactly like the other forty until you click them all. Ring it.
            // Polygons the alert layer already holds; no extra fetch and no extra geometry.
            let threat: Vec<&GeoFeature> = self
                .alert_features
                .iter()
                .filter(|f| {
                    f.kind == overlay::FeatureKind::Warning
                        && f.alert.as_ref().is_some_and(|a| {
                            let e = a.event.to_ascii_lowercase();
                            e.contains("tornado") || e.contains("severe thunderstorm")
                        })
                })
                .collect();
            for site in &self.webcams {
                let w = crate::render::mercator::lonlat_to_world(site.lon, site.lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(p) {
                    continue;
                }
                painter.circle_filled(p, 4.0, col);
                painter.circle_stroke(
                    p,
                    4.0,
                    egui::Stroke::new(1.0, egui::Color32::from_black_alpha(170)),
                );
                // `distance_km` is 0 inside the polygon, which is the test we want here.
                if threat
                    .iter()
                    .any(|f| f.distance_km(site.lon, site.lat) == 0.0)
                {
                    painter.circle_stroke(
                        p,
                        7.0,
                        egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 120, 60)),
                    );
                }
                if show_labels {
                    painter.text(
                        p + egui::vec2(6.0, -5.0),
                        egui::Align2::LEFT_BOTTOM,
                        &site.name,
                        egui::FontId::proportional(10.0),
                        col,
                    );
                }
            }
        }

        // Live stations: a dot per station, warm where it is hot and cool where it is not, so a
        // boundary reads off the map before any card is open. Clicking one opens its card.
        if self.show_stations {
            let show_labels = cam.zoom >= 8.0;
            let temp_unit = self.settings.temp_unit;
            for ob in &self.stations.obs {
                let w = crate::render::mercator::lonlat_to_world(ob.lon, ob.lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(p) {
                    continue;
                }
                let col = match ob.temp_c {
                    // Blue at freezing through red at 38 C, the span US surface weather lives in.
                    Some(t) => {
                        let f = ((t / 38.0).clamp(0.0, 1.0) * 255.0) as u8;
                        egui::Color32::from_rgb(f, 90, 255 - f)
                    }
                    None => egui::Color32::from_gray(150),
                };
                // A personal station usually sits within a mile of the airport METAR that already
                // has a dot here, so the networks get different shapes and opposite label sides —
                // otherwise the PWS is drawn, invisible, underneath the METAR.
                let stroke = egui::Stroke::new(1.0, egui::Color32::from_black_alpha(180));
                let metar = ob.network == wxdata::stations::Network::Metar;
                if metar {
                    painter.circle_filled(p, 5.0, col);
                    painter.circle_stroke(p, 5.0, stroke);
                } else {
                    let r = egui::Rect::from_center_size(p, egui::vec2(9.0, 9.0));
                    painter.rect_filled(r, 1.0, col);
                    painter.rect_stroke(r, 1.0, stroke, egui::StrokeKind::Middle);
                }
                if show_labels {
                    let label = match ob.temp_c {
                        Some(t) => format!("{:.0}{}", temp_unit.from_c(t), temp_unit.label()),
                        None => ob.id.clone(),
                    };
                    let (off, align) = if metar {
                        (7.0, egui::Align2::LEFT_CENTER)
                    } else {
                        (-7.0, egui::Align2::RIGHT_CENTER)
                    };
                    painter.text(
                        p + egui::vec2(off, 0.0),
                        align,
                        label,
                        egui::FontId::proportional(10.0),
                        egui::Color32::from_gray(230),
                    );
                }
            }
        }

        // Damage surveys: the fitted path first, then a dot per surveyed indicator coloured by its
        // EF rating, so the rating gradient along the track reads at a glance.
        if self.show_dat {
            let to_screen = |lon: f64, lat: f64| {
                let w = crate::render::mercator::lonlat_to_world(lon, lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                egui::pos2(prect.left() + sx, prect.top() + sy)
            };
            for t in &self.dat_tracks {
                let pts: Vec<egui::Pos2> = t.path.iter().map(|p| to_screen(p[0], p[1])).collect();
                if pts.len() >= 2 {
                    painter.add(egui::Shape::line(
                        pts,
                        egui::Stroke::new(2.5, ef_color(&t.efscale).gamma_multiply(0.9)),
                    ));
                }
            }
            for p in &self.dat_points {
                let s = to_screen(p.lon, p.lat);
                if !prect.contains(s) {
                    continue;
                }
                painter.circle_filled(s, 3.5, ef_color(&p.efscale));
                painter.circle_stroke(
                    s,
                    3.5,
                    egui::Stroke::new(1.0, egui::Color32::from_black_alpha(180)),
                );
            }
        }

        // Verification reports, while the lab is open: green where a warning was already out,
        // red where nothing was. The red dots are the point of the whole feature.
        if self.verify_window.open {
            if let Some(v) = &self.verify_window.data {
                for r in &v.reports {
                    let w = crate::render::mercator::lonlat_to_world(r.lon, r.lat);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                    if !prect.contains(p) {
                        continue;
                    }
                    let color = if r.warned {
                        egui::Color32::from_rgb(120, 220, 140)
                    } else {
                        egui::Color32::from_rgb(230, 70, 70)
                    };
                    painter.circle_filled(p, 4.0, color);
                    painter.circle_stroke(
                        p,
                        4.0,
                        egui::Stroke::new(1.0, egui::Color32::from_black_alpha(180)),
                    );
                }
            }
        }

        // Radiosonde sites, only while the sounding tool is armed — a click anywhere takes the
        // nearest of these, so showing them is how you know what "nearest" will pick.
        if self.tool == MapTool::Sounding {
            for st in &wxdata::raob::STATIONS {
                let w = crate::render::mercator::lonlat_to_world(st.lon, st.lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(p) {
                    continue;
                }
                let color = egui::Color32::from_rgb(150, 200, 255);
                painter.circle_stroke(p, 4.0, egui::Stroke::new(1.5, color));
                if cam.zoom >= 6.0 {
                    painter.text(
                        p + egui::vec2(6.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        // The station's place name, not its WMO number: "72249" on a map tells
                        // nobody anything.
                        st.name.split(',').next().unwrap_or(st.name),
                        egui::FontId::proportional(10.0),
                        color,
                    );
                }
            }
        }

        if self.show_spotters {
            if let Some(site_pos) = self.views[idx]
                .site
                .as_deref()
                .and_then(wxdata::sites::site_by_id)
                .map(|s| [s.longitude as f64, s.latitude as f64])
            {
                let now = Utc::now();
                let show_labels = cam.zoom >= 9.0;
                // The range limit is at most `range/110` degrees of latitude and, at CONUS
                // latitudes, ~1.4x that in longitude — a cheap box rejects almost every spotter
                // before the haversine runs. 0 means the user asked for the whole feed.
                let range_km = self.settings.spotter_range_km.max(0.0);
                let (max_dlat, max_dlon) = if range_km <= 0.0 {
                    (f64::INFINITY, f64::INFINITY)
                } else {
                    let dlat = range_km / 110.0;
                    (dlat, dlat * 1.45)
                };
                let mut spotter_click: Option<wxdata::spotters::Spotter> = None;
                for sp in &self.spotters {
                    if (sp.lon - site_pos[0]).abs() > max_dlon
                        || (sp.lat - site_pos[1]).abs() > max_dlat
                    {
                        continue;
                    }
                    if range_km > 0.0
                        && crate::geo::great_circle(site_pos, [sp.lon, sp.lat]).0 > range_km
                    {
                        continue;
                    }
                    let w = crate::render::mercator::lonlat_to_world(sp.lon, sp.lat);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                    if !prect.contains(p) {
                        continue;
                    }
                    // Spotter Network green; faded when the report is stale (>30 min old).
                    let stale = (now - sp.time).num_minutes() > 30;
                    let color = {
                        let g = egui::Color32::from_rgb(0, 200, 80);
                        if stale {
                            g.gamma_multiply(0.35)
                        } else {
                            g
                        }
                    };
                    painter.circle_filled(p, 3.0, color);
                    painter.circle_stroke(
                        p,
                        3.0,
                        egui::Stroke::new(1.0, egui::Color32::from_black_alpha(160)),
                    );
                    // Movement arrow tick, heading clockwise from north.
                    if let Some(h) = sp.heading {
                        let r = h.to_radians();
                        let dir = egui::vec2(r.sin(), -r.cos());
                        painter.line_segment([p, p + dir * 8.0], egui::Stroke::new(1.5, color));
                    }
                    if show_labels {
                        painter.text(
                            p + egui::vec2(5.0, -5.0),
                            egui::Align2::LEFT_BOTTOM,
                            &sp.name,
                            egui::FontId::proportional(10.0),
                            color,
                        );
                    }
                    let hit = egui::Rect::from_center_size(p, egui::vec2(14.0, 14.0));
                    if response.hover_pos().is_some_and(|hp| hit.contains(hp)) {
                        let hover = format!(
                            "{}\n{}\n{}",
                            sp.name,
                            crate::timefmt::fmt_date_clock(sp.time, self.active_tz()),
                            sp.status
                        );
                        response.clone().show_tooltip_text(hover);
                    }
                    if response.clicked()
                        && response
                            .interact_pointer_pos()
                            .is_some_and(|hp| hit.contains(hp))
                    {
                        spotter_click = Some(sp.clone());
                    }
                }
                // Deferred: the surrounding pane borrow is still live here.
                self.pending_spotter = spotter_click;
            }
        }

        // ProbSevere per-storm probability badges (polygons draw via the overlay pipeline).
        if self.show_probsevere {
            for f in &self.probsevere {
                let Some(ring) = f.rings.first() else {
                    continue;
                };
                if ring.is_empty() {
                    continue;
                }
                let (mut clon, mut clat) = (0.0, 0.0);
                for p in ring {
                    clon += p[0];
                    clat += p[1];
                }
                let cw = crate::render::mercator::lonlat_to_world(
                    clon / ring.len() as f64,
                    clat / ring.len() as f64,
                );
                let (sx, sy) = cam.world_to_screen(cw, vp);
                let c = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(c) {
                    continue;
                }
                let color = egui::Color32::from_rgb(f.stroke[0], f.stroke[1], f.stroke[2]);
                let font = egui::FontId::proportional(11.0);
                let galley =
                    painter.layout_no_wrap(f.title.clone(), font.clone(), egui::Color32::BLACK);
                let rect = egui::Rect::from_center_size(c, galley.size() + egui::vec2(8.0, 4.0));
                painter.rect_filled(rect, 3.0, color);
                painter.text(
                    c,
                    egui::Align2::CENTER_CENTER,
                    &f.title,
                    font,
                    egui::Color32::BLACK,
                );
            }
        }

        // Warning intelligence: warned-storm motion vector + projected path + ETA to markers, and
        // a pulsing outline on escalated (Tornado Emergency / PDS / destructive) warnings.
        if self.filters.show_alerts {
            let to_screen = |lon: f64, lat: f64| {
                let w = crate::render::mercator::lonlat_to_world(lon, lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                egui::pos2(prect.left() + sx, prect.top() + sy)
            };
            let mut any_escalated = false;
            // Indices, not strings: the `take(6)` below drew six of them however many marker ×
            // warning pairs were formatted.
            let mut etas: Vec<(f64, usize, usize)> = Vec::new();
            let time = ctx.input(|i| i.time);
            // Viewport-center lon/lat: a polygon with every vertex off-screen can still fill the
            // whole pane (zoomed inside it) — the primary chase case for an escalated warning.
            let (center_lon, center_lat) = {
                let w = cam.screen_to_world((vp.0 * 0.5, vp.1 * 0.5), vp);
                crate::render::mercator::world_to_lonlat(w.0, w.1)
            };
            let features = self.active_alert_features();
            for (fi, f) in features.iter().enumerate() {
                let Some(a) = &f.alert else { continue };
                // Pulsing outline for escalated warnings only — watches can carry PDS wording,
                // but pulsing a state-sized watch polygon would drown the map (and `escalation`
                // uppercases the whole bulletin, too heavy to run for every alert every frame).
                if f.kind == overlay::FeatureKind::Warning && wxdata::alerts::escalation(a) >= 2 {
                    let visible =
                        f.rings.first().is_some_and(|r| {
                            r.iter().any(|p| prect.contains(to_screen(p[0], p[1])))
                        }) || f.contains(center_lon, center_lat);
                    if visible {
                        any_escalated = true;
                        let w = 2.0 + 2.0 * (time * 4.0).sin().abs() as f32;
                        let col = egui::Color32::from_rgb(255, 40, 40);
                        for ring in &f.rings {
                            let pts: Vec<egui::Pos2> =
                                ring.iter().map(|p| to_screen(p[0], p[1])).collect();
                            if pts.len() >= 2 {
                                painter.add(egui::Shape::line(pts, egui::Stroke::new(w, col)));
                            }
                        }
                    }
                }
                // Motion vector + projected path (heading = FROM + 180).
                let Some(m) = &a.motion else { continue };
                let Some(&origin) = m.points.first() else {
                    continue;
                };
                if m.kt < 1.0 {
                    continue;
                }
                let heading = ((m.deg + 180.0) % 360.0) as f64;
                let apex = to_screen(origin[0], origin[1]);
                let col = egui::Color32::from_rgb(255, 235, 90);
                painter.circle_filled(apex, 4.0, col);
                let mut prev = apex;
                for min in [15.0_f64, 30.0, 45.0, 60.0] {
                    let km = m.kt as f64 * 1.852 * (min / 60.0);
                    let tp = crate::geo::destination_point(origin, heading, km);
                    let p = to_screen(tp[0], tp[1]);
                    painter.line_segment([prev, p], egui::Stroke::new(1.5, col));
                    painter.circle_filled(p, 2.5, col);
                    if cam.zoom >= 7.0 {
                        painter.text(
                            p + egui::vec2(5.0, -2.0),
                            egui::Align2::LEFT_CENTER,
                            format!("+{min:.0}m"),
                            egui::FontId::proportional(10.0),
                            col,
                        );
                    }
                    prev = p;
                }
                // ETA to any watched marker along the storm's heading.
                for (mi, mk) in self.settings.markers.iter().enumerate() {
                    if let Some(t) = crate::geo::arrival_eta_min(
                        origin,
                        heading as f32,
                        m.kt,
                        [mk.lon, mk.lat],
                        22.5,
                        90.0,
                    ) {
                        etas.push((t, mi, fi));
                    }
                }
            }
            if idx == self.active && !etas.is_empty() {
                etas.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                let font = egui::FontId::proportional(12.0);
                let mut y = prect.top() + 64.0;
                for (t, mi, fi) in etas.iter().take(6) {
                    let mk = &self.settings.markers[*mi];
                    let event = features[*fi]
                        .alert
                        .as_ref()
                        .map(|a| a.event.as_str())
                        .unwrap_or_default();
                    let line = format!("⚠ {} — {} in {:.0} min", mk.name, event, t);
                    let galley = painter.layout_no_wrap(line, font.clone(), egui::Color32::WHITE);
                    let anchor = egui::pos2(prect.left() + 8.0, y);
                    let bg =
                        egui::Rect::from_min_size(anchor, galley.size() + egui::vec2(10.0, 4.0));
                    painter.rect_filled(
                        bg,
                        3.0,
                        egui::Color32::from_rgba_unmultiplied(150, 30, 30, 210),
                    );
                    // The galley just measured, drawn — `painter.text` would lay the same string
                    // out a second time.
                    let size = galley.size();
                    painter.galley(anchor + egui::vec2(5.0, 2.0), galley, egui::Color32::WHITE);
                    y += size.y + 6.0;
                }
            }
            if any_escalated {
                ctx.request_repaint_after(std::time::Duration::from_millis(60));
            }
        }

        // Surface obs (METAR station plots): fltCat-colored circle, wind barb, T/Td in °F.
        if self.show_metar && cam.zoom >= 6.0 {
            crate::prof_scope!("metar_plots");
            let show_labels = cam.zoom >= 7.0;
            let flt_color = |c: &str| match c {
                "VFR" => egui::Color32::from_rgb(60, 200, 90),
                "MVFR" => egui::Color32::from_rgb(80, 150, 240),
                "IFR" => egui::Color32::from_rgb(230, 60, 60),
                "LIFR" => egui::Color32::from_rgb(220, 60, 200),
                _ => egui::Color32::from_gray(180),
            };
            // Projected and clipped before either sort: everything off screen is skipped by the
            // loop below anyway, and the whole national set was being sorted twice to get there.
            let mut obs: Vec<(egui::Pos2, &wxdata::metar::SurfaceOb)> = self
                .metars
                .iter()
                .filter_map(|ob| {
                    let w = crate::render::mercator::lonlat_to_world(ob.lon, ob.lat);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                    prect.contains(p).then_some((p, ob))
                })
                .collect();
            // Windiest-first so the strongest stations survive decluttering.
            obs.sort_by(|(_, a), (_, b)| {
                b.wspd_kt
                    .partial_cmp(&a.wspd_kt)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let temp_unit = self.settings.temp_unit;
            // Same stickiness rule as the place names: a station already plotted keeps its cell
            // ahead of a windier one that has only just come into view.
            obs.sort_by_key(|(_, ob)| !self.labels.was_shown(crate::labelplace::key(&ob.icao)));
            for (p, ob) in obs {
                // Shared declutter: this also sees the place names drawn above it.
                let cell = egui::Rect::from_center_size(p, egui::vec2(44.0, 34.0));
                if !self.labels.place(
                    crate::labelplace::key(&ob.icao),
                    cell,
                    crate::labelplace::Priority::Station,
                ) {
                    continue;
                }
                let col = flt_color(&ob.flt_cat);
                painter.circle_stroke(p, 3.0, egui::Stroke::new(1.5, col));
                // Wind barb, rotated so the shaft points toward the wind source (FROM bearing).
                if let Some(dir) = ob.wdir_deg {
                    let th = dir.to_radians();
                    let (up, right) = ([th.sin(), -th.cos()], [th.cos(), th.sin()]);
                    let map = |u: [f32; 2]| {
                        p + egui::vec2(
                            (u[0] * right[0] + u[1] * up[0]) * 22.0,
                            (u[0] * right[1] + u[1] * up[1]) * 22.0,
                        )
                    };
                    for (a, b) in wxdata::metar::barb_segments(ob.wspd_kt) {
                        painter.line_segment([map(a), map(b)], egui::Stroke::new(1.3, col));
                    }
                }
                // Temperature (red, upper-left) and dewpoint (green, lower-left) in °F.
                if show_labels {
                    let f = egui::FontId::proportional(11.0);
                    if let Some(t) = ob.temp_c {
                        painter.text(
                            p + egui::vec2(-6.0, -6.0),
                            egui::Align2::RIGHT_BOTTOM,
                            format!("{:.0}", temp_unit.from_c(t)),
                            f.clone(),
                            egui::Color32::from_rgb(240, 90, 90),
                        );
                    }
                    if let Some(d) = ob.dewp_c {
                        painter.text(
                            p + egui::vec2(-6.0, 6.0),
                            egui::Align2::RIGHT_TOP,
                            format!("{:.0}", temp_unit.from_c(d)),
                            f,
                            egui::Color32::from_rgb(90, 220, 120),
                        );
                    }
                    // Sea state, under the plot — buoys only, by construction.
                    if let Some(h) = ob.wvht_ft {
                        let period = ob
                            .dpd_s
                            .map(|s| format!(" {s:.0}s"))
                            .unwrap_or_else(String::new);
                        painter.text(
                            p + egui::vec2(0.0, 9.0),
                            egui::Align2::CENTER_TOP,
                            format!("{h:.1}ft{period}"),
                            egui::FontId::proportional(10.0),
                            egui::Color32::from_rgb(120, 200, 230),
                        );
                    }
                }
                // Hover → the raw METAR text.
                let hit = egui::Rect::from_center_size(p, egui::vec2(16.0, 16.0));
                if response.hover_pos().is_some_and(|hp| hit.contains(hp)) && !ob.raw.is_empty() {
                    // Observation first, then the terminal forecast where the station files one:
                    // what it is doing, then what it is expected to do.
                    let text = match self.tafs.get(&ob.icao) {
                        Some(taf) => format!("{}\n\n{taf}", ob.raw),
                        None => ob.raw.clone(),
                    };
                    response.clone().show_tooltip_text(text);
                }
            }
        }
        // River flood gauges (NWPS): category-colored inverted-triangle droplet + stage tooltip.
        // An interrogate click opens the gauge's card (`ui::gauge_card`): hydrograph, flood
        // stages, crests. A gauge forecast to reach a worse category than it is in now wears a
        // ring in that category's color, and a gauge with its card open a white one.
        if self.show_gauges && cam.zoom >= 6.0 {
            let gcolor = crate::ui::gauge_card::cat_color;
            let glabel = crate::ui::gauge_card::cat_label;
            // Already-drawn gauges get their slot back before a newcomer takes it, the same way
            // the METAR and place-name layers already do. Without it a gauge at the edge of a
            // collision wins and loses on alternate frames, which reads as flicker while panning.
            //
            // Two passes — returning labels, then the rest — rather than sorting into a vector
            // that would be allocated and thrown away on every frame.
            for returning in [true, false] {
                crate::prof_scope!("river_gauges");
                for g in &self.gauges {
                    if self.labels.was_shown(crate::labelplace::key(&g.lid)) != returning {
                        continue;
                    }
                    let w = crate::render::mercator::lonlat_to_world(g.lon, g.lat);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                    if !prect.contains(p) {
                        continue;
                    }
                    // Shared declutter: gauges are the lowest tier, so they fill what is left.
                    let cell = egui::Rect::from_center_size(p, egui::vec2(15.0, 15.0));
                    if !self.labels.place(
                        crate::labelplace::key(&g.lid),
                        cell,
                        crate::labelplace::Priority::Minor,
                    ) {
                        continue;
                    }
                    let s = 6.0;
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            p + egui::vec2(-s * 0.85, -s * 0.6),
                            p + egui::vec2(s * 0.85, -s * 0.6),
                            p + egui::vec2(0.0, s),
                        ],
                        gcolor(g.cat).gamma_multiply(0.85),
                        egui::Stroke::new(1.2, egui::Color32::from_gray(20)),
                    ));
                    if g.forecast_ft.is_some() && g.forecast_cat.severity() < g.cat.severity() {
                        painter.circle_stroke(
                            p + egui::vec2(0.0, -0.5),
                            s + 3.0,
                            egui::Stroke::new(1.8, gcolor(g.forecast_cat)),
                        );
                    }
                    if self.gauge_cards.is_open(&g.lid) {
                        painter.circle_stroke(
                            p + egui::vec2(0.0, -0.5),
                            s + 5.5,
                            egui::Stroke::new(1.5, egui::Color32::WHITE),
                        );
                    }
                    let hit = egui::Rect::from_center_size(p, egui::vec2(16.0, 16.0));
                    if response.hover_pos().is_some_and(|hp| hit.contains(hp)) {
                        let stage = g
                            .stage_ft
                            .map_or_else(|| "n/a".to_string(), |v| format!("{v:.1} ft"));
                        let mut tip =
                            format!("{} ({})\n{stage} — {}", g.name, g.lid, glabel(g.cat));
                        if let Some(f) = g.forecast_ft {
                            tip.push_str(&format!(
                                "\nFcst: {f:.1} ft ({})",
                                glabel(g.forecast_cat)
                            ));
                        }
                        if self.tool == MapTool::Interrogate {
                            tip.push_str("\nClick for the hydrograph and crests");
                        }
                        response.clone().show_tooltip_text(tip);
                    }
                }
            }
        }

        // Day/night shading, the terminator line, and the lat/lon graticule — a plain reference
        // layer over the map, not a data source, so it needs no site/moment/pane gate beyond the
        // toggle itself.
        if self.show_daynight {
            crate::daynight_draw::draw(
                &painter,
                prect,
                |lon, lat| {
                    let w = crate::render::mercator::lonlat_to_world(lon, lat);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    egui::pos2(prect.left() + sx, prect.top() + sy)
                },
                Utc::now(),
            );
        }

        // County power outages: hatching, not a fill — see `outage_draw`.
        if self.show_outages && !self.outage_features.is_empty() {
            crate::outage_draw::draw(&painter, &self.outage_features, prect, |lon, lat| {
                let w = crate::render::mercator::lonlat_to_world(lon, lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                egui::pos2(prect.left() + sx, prect.top() + sy)
            });
        }
        // NHC tropical suite: dashed cone edge, forecast track, and per-point callouts.
        if self.show_tropical {
            // Model guidance under the NHC cone and track: the official forecast is the one
            // that must read on top of the spaghetti.
            let tip = crate::spaghetti::draw(
                &painter,
                &self.spaghetti,
                prect,
                cam.zoom as f32,
                |lon, lat| {
                    let w = crate::render::mercator::lonlat_to_world(lon, lat);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    egui::pos2(prect.left() + sx, prect.top() + sy)
                },
                response.hover_pos(),
                self.active_tz(),
            );
            if let Some(t) = &self.tropical {
                crate::tropical_draw::draw(&painter, t, prect, cam.zoom as f32, |lon, lat| {
                    let w = crate::render::mercator::lonlat_to_world(lon, lat);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    egui::pos2(prect.left() + sx, prect.top() + sy)
                });
            }
            if let Some(tip) = tip {
                response.clone().show_tooltip_text(tip);
            }
        }

        // Forecast-reflectivity banner — unmistakable that this is model forecast, not observation.
        if idx == self.active && view.fields_on.contains(&crate::render::FieldLayer::Hrrr) {
            let valid = self
                .hrrr_valid
                .map(|v| crate::timefmt::fmt_date_clock(v, self.active_tz()))
                .unwrap_or_else(|| "loading…".to_string());
            let lead_min = if self.hrrr_subhourly {
                self.hrrr_fcst_min
            } else {
                u16::from(self.hrrr_fcst_hour) * 60
            };
            let lead = crate::model_browser::format_lead(lead_min);
            let lead = lead.trim_start_matches('F');
            let model = self.refl_source_label().to_uppercase();
            let text = format!("⚠ FORECAST {lead} — {model} MODEL, NOT OBSERVED — valid {valid}");
            let font = egui::FontId::proportional(13.0);
            let pad = egui::vec2(10.0, 4.0);
            // On a phone the sentence is wider than the screen ("...valid Sep 20, 6:" ran off the
            // right edge), so it wraps, and it sits in the lane under the search pill and clear of
            // the control column instead of over the status bar. Desktop keeps the one-line strip
            // along the top.
            let phone = crate::platform::phone_layout();
            let (wrap, center_x, phone_y) = if phone {
                let (gutter_l, gutter_r) = self.phone_gutters();
                let left = prect.left() + crate::ui::m3::SP_3 + gutter_l;
                let right = prect.right() - crate::ui::m3::SP_3 - gutter_r;
                (
                    (right - left - pad.x * 2.0).max(120.0),
                    (left + right) / 2.0,
                    prect.top() + chrome::phone_top(ui.ctx()) + 56.0 + chrome::MODE_BAR_H + 8.0,
                )
            } else {
                (f32::INFINITY, prect.center().x, 0.0)
            };
            let galley = painter.layout(text, font, egui::Color32::BLACK, wrap);
            // Desktop: centred 16 pt down, exactly where the one-line strip always sat.
            let top = if phone {
                phone_y
            } else {
                prect.top() + 16.0 - (galley.size().y / 2.0 + pad.y)
            };
            let rect = egui::Rect::from_min_size(
                egui::pos2(center_x - (galley.size().x + pad.x * 2.0) / 2.0, top),
                galley.size() + pad * 2.0,
            );
            painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(255, 170, 60));
            painter.galley(rect.min + pad, galley, egui::Color32::BLACK);
        }

        ui::comparison_status::paint_for_view(
            &painter,
            prect,
            &view.fields_on,
            self.diff_field,
            self.diff_valid,
            self.compare_valid,
            self.diff_error.as_deref(),
            self.compare_error.as_deref(),
        );

        // Placefile labels/icons.
        for label in placefile_labels {
            // An anchored label is placed by projecting its object's anchor and then stepping the
            // stated pixels from it (y up), so it holds its offset as the map zooms.
            let (base, off) = match label.anchor {
                Some(a) => (a, egui::vec2(label.pos[0] as f32, -label.pos[1] as f32)),
                None => (label.pos, egui::Vec2::ZERO),
            };
            let w = crate::render::mercator::lonlat_to_world(base[0], base[1]);
            let (sx, sy) = cam.world_to_screen(w, vp);
            let p = egui::pos2(prect.left() + sx, prect.top() + sy) + off;
            if !prect.contains(p) {
                continue;
            }
            let mut hit_size = egui::vec2(16.0, 16.0);
            match &label.kind {
                PlaceLabelKind::Text(text) => {
                    painter.text(
                        p,
                        egui::Align2::CENTER_CENTER,
                        text,
                        egui::FontId::proportional(12.0),
                        label.color,
                    );
                }
                PlaceLabelKind::Marker => {
                    painter.circle_stroke(p, 5.0, egui::Stroke::new(1.5, label.color));
                    painter.circle_filled(p, 1.5, label.color);
                }
                PlaceLabelKind::Sprite {
                    tex,
                    uv,
                    size,
                    hot,
                    angle,
                } => {
                    draw_sprite(&painter, *tex, *uv, p, *size, *hot, *angle, label.color);
                    hit_size = *size;
                }
            }
            if !label.hover.is_empty() {
                let hit = egui::Rect::from_center_size(p, hit_size);
                if response.hover_pos().is_some_and(|hp| hit.contains(hp)) {
                    response.clone().show_tooltip_text(&label.hover);
                }
            }
        }

        // The difference layer reads as "they disagree here" and nothing more without a number,
        // so the cursor samples the grid it was drawn from.
        if view
            .fields_on
            .contains(&crate::render::FieldLayer::ModelDiff)
        {
            if let Some(hp) = response.hover_pos() {
                if let Some(v) = self
                    .diff_display_grid()
                    .and_then(|g| self.diff_hover_value(g, cam, prect, vp, hp))
                {
                    let f = self.diff_field;
                    let (a, b) = f.pair();
                    // The retained grid is always signed (see `diff_mode`'s own doc comment), so
                    // the readout has to be put through the same transform the upload was, or a
                    // magnitude/mask display would otherwise hand back an unexplained signed
                    // number. A forced sign belongs only on the signed view: "+6.0" for a
                    // directionless mode reads as a direction it deliberately threw away.
                    let (_, deadband) = f.range();
                    let value = format_diff_readout(self.diff_mode, v, deadband, f.units());
                    response.clone().show_tooltip_text(format!(
                        "{}: {value} ({})",
                        f.label(),
                        self.diff_mode.expression(a, b)
                    ));
                }
            }
        }

        // The compare panes show one model's own field, unsubtracted — same reasoning as the
        // difference layer's hover above, just reading whichever side this pane is showing.
        {
            use crate::render::FieldLayer as FL;
            let showing_a = view.fields_on.contains(&FL::CompareA);
            let showing_b = view.fields_on.contains(&FL::CompareB);
            if let Some(hp) = response.hover_pos() {
                // A swipe selects the grid physically under the cursor; otherwise CompareB is
                // the top draw when both layers happen to be enabled.
                let on_top_is_b = if view.swipe_compare && showing_a && showing_b {
                    Self::swipe_showing_b(view.swipe_fraction, hp.x - prect.left(), prect.width())
                } else {
                    showing_b
                };
                let grid = self.compare_grid.as_ref().and_then(|(a, b)| {
                    if on_top_is_b {
                        Some(b)
                    } else if showing_a {
                        Some(a)
                    } else {
                        None
                    }
                });
                if let Some(v) = grid.and_then(|g| self.diff_hover_value(g, cam, prect, vp, hp)) {
                    let f = self.diff_field;
                    let (label_a, label_b) = f.pair();
                    let model = if on_top_is_b { label_b } else { label_a };
                    response.clone().show_tooltip_text(format!(
                        "{}: {v:.1} {} ({model})",
                        f.label(),
                        f.units()
                    ));
                }
            }
        }

        // Beam-vs-terrain blockage shading, under the reference annotations. The raster covers a
        // world-space rect, which maps linearly to screen, so it is one stretched image — and while
        // a rebuild is in flight the previous rect keeps it registered to the ground.
        if self.show_blockage {
            if let Some((_, tex, world)) = &self.blockage_tex {
                let a = cam.world_to_screen((world[0], world[1]), vp);
                let b = cam.world_to_screen((world[2], world[3]), vp);
                let rect = egui::Rect::from_two_pos(
                    egui::pos2(prect.left() + a.0, prect.top() + a.1),
                    egui::pos2(prect.left() + b.0, prect.top() + b.1),
                );
                painter.image(
                    tex.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        }

        // Lowest-usable-tilt shading, same registration approach as blockage above.
        if self.show_lowest_tilt {
            if let Some((_, tex, world)) = &self.lowest_tilt_tex {
                let a = cam.world_to_screen((world[0], world[1]), vp);
                let b = cam.world_to_screen((world[2], world[3]), vp);
                let rect = egui::Rect::from_two_pos(
                    egui::pos2(prect.left() + a.0, prect.top() + a.1),
                    egui::pos2(prect.left() + b.0, prect.top() + b.1),
                );
                painter.image(
                    tex.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        }

        // Coverage-comparison shading (ROADMAP_NEW C3), same registration approach as blockage.
        // Toggle: the suitability popup's "Compare" button on the same pair again turns it off,
        // and closing that popup clears it too — see the call site in `apply_palette`/the update
        // loop rather than a second dedicated close control here.
        if let (Some((a_id, b_id)), Some((_, tex, world))) =
            (&self.coverage_compare, &self.coverage_compare_tex)
        {
            let a = cam.world_to_screen((world[0], world[1]), vp);
            let b = cam.world_to_screen((world[2], world[3]), vp);
            let rect = egui::Rect::from_two_pos(
                egui::pos2(prect.left() + a.0, prect.top() + a.1),
                egui::pos2(prect.left() + b.0, prect.top() + b.1),
            );
            painter.image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            painter.text(
                prect.left_top() + egui::vec2(8.0, 8.0),
                egui::Align2::LEFT_TOP,
                format!("Coverage: {a_id} (blue) vs {b_id} (red)"),
                egui::FontId::proportional(12.0),
                egui::Color32::from_gray(230),
            );
        }

        // Range rings + azimuth spokes around this pane's site (feature HH). Ring spacing follows
        // the same US-vs-international-network rule the measure tool already uses
        // (`self.metric_in`/`crate::geo::fmt_distance`) rather than a separate unit choice — a
        // NEXRAD/TDWR site reads in miles, everything else in kilometers. The four ring radii are
        // picked to be round numbers in whichever unit is showing; the geodesic math underneath
        // (`destination_point`) always takes kilometers regardless.
        if self.show_range_rings {
            if let Some(site) = view.site.as_deref().and_then(wxdata::sites::site_by_id) {
                let origin = [site.longitude as f64, site.latitude as f64];
                let col = egui::Color32::from_gray(150).gamma_multiply(0.55);
                let to_screen = |lon: f64, lat: f64| {
                    let w = crate::render::mercator::lonlat_to_world(lon, lat);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    egui::pos2(prect.left() + sx, prect.top() + sy)
                };
                let metric = self.metric_in(idx);
                let ring_values: [f64; 4] = if metric {
                    [50.0, 100.0, 150.0, 200.0]
                } else {
                    [25.0, 50.0, 75.0, 100.0]
                };
                let mut max_ring_km = 0.0f64;
                for value in ring_values {
                    let km = if metric {
                        value
                    } else {
                        value * crate::geo::KM_PER_MILE
                    };
                    max_ring_km = max_ring_km.max(km);
                    let pts: Vec<egui::Pos2> = (0..=72)
                        .map(|i| {
                            let p = crate::geo::destination_point(origin, i as f64 * 5.0, km);
                            to_screen(p[0], p[1])
                        })
                        .collect();
                    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.0, col)));
                    if cam.zoom >= 6.0 {
                        let top = crate::geo::destination_point(origin, 0.0, km);
                        painter.text(
                            to_screen(top[0], top[1]),
                            egui::Align2::CENTER_BOTTOM,
                            crate::geo::fmt_distance(km, metric, 0),
                            egui::FontId::proportional(10.0),
                            col,
                        );
                    }
                }
                for az in (0..360).step_by(45) {
                    let far = crate::geo::destination_point(origin, az as f64, max_ring_km);
                    painter.line_segment(
                        [to_screen(origin[0], origin[1]), to_screen(far[0], far[1])],
                        egui::Stroke::new(0.6, col.gamma_multiply(0.7)),
                    );
                }
            }
        }

        // Scan age: a ring at the edge of the sweep, coloured by how long before the newest data
        // each azimuth was collected. The antenna takes minutes to turn, so the two edges of a
        // picture can be a rotation apart; this is where.
        if self.show_scan_age {
            if let Some(Some(ring)) = self.scan_age_rings.get(&idx) {
                let to_screen = |p: [f64; 2]| {
                    let w = crate::render::mercator::lonlat_to_world(p[0], p[1]);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    egui::pos2(prect.left() + sx, prect.top() + sy)
                };
                let n = ring.wedges.len();
                for (i, age) in ring.wedges.iter().enumerate() {
                    let Some(age) = age else { continue };
                    let a0 = i as f64 / n as f64 * 360.0;
                    let a1 = (i + 1) as f64 / n as f64 * 360.0;
                    let pts: Vec<egui::Pos2> = [a0, (a0 + a1) / 2.0, a1]
                        .into_iter()
                        .map(|az| {
                            to_screen(crate::geo::destination_point(
                                ring.origin,
                                az,
                                ring.radius_km,
                            ))
                        })
                        .collect();
                    painter.add(egui::Shape::line(
                        pts,
                        egui::Stroke::new(5.0, scan_age_color(*age)),
                    ));
                }
                if cam.zoom >= 4.0 {
                    let top = to_screen(crate::geo::destination_point(
                        ring.origin,
                        0.0,
                        ring.radius_km,
                    ));
                    let mut text = format!(
                        "Sweep spans {}",
                        wxdata::scan_age::format_span(ring.summary.span_ms())
                    );
                    // Only meaningful on a live volume; on an archive replay the wall-clock age
                    // is years and says nothing about the picture.
                    let since = Utc::now().timestamp_millis() - ring.summary.newest_ms;
                    if (0..6 * 3_600_000).contains(&since) {
                        text.push_str(&format!(
                            " · newest {} ago",
                            wxdata::scan_age::format_span(since)
                        ));
                    }
                    if ring.summary.is_partial() {
                        text.push_str(" · partial");
                    }
                    painter.text(
                        top + egui::vec2(0.0, -8.0),
                        egui::Align2::CENTER_BOTTOM,
                        text,
                        egui::FontId::proportional(11.0),
                        egui::Color32::from_gray(225),
                    );
                }
            }
        }

        // Radar sites: a ring per site — both networks, so a TDWR you can select is a TDWR you can
        // see. The active site in accent, others muted. IDs only when zoomed in so the CONUS view
        // isn't cluttered. Click handled in the Interrogate tool.
        if self.show_radar_sites {
            let accent = crate::theme::accent(self.settings.theme);
            let current = self.views[idx].site.as_deref();
            // Sticky, for the same reason as the gauges above: a site id that wins and loses the
            // same collision on alternate frames is the flicker, not the collision.
            for returning in [true, false] {
                let show_labels = cam.zoom >= 5.0;
                for (s, w) in sites_in_world() {
                    if self.labels.was_shown(crate::labelplace::key(s.id)) != returning {
                        continue;
                    }
                    let (sx, sy) = cam.world_to_screen(*w, vp);
                    let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                    if !prect.contains(p) {
                        continue;
                    }
                    let is_current = current == Some(s.id);
                    let col = if is_current {
                        accent
                    } else {
                        egui::Color32::from_rgb(120, 190, 255)
                    };
                    let r = if is_current { 5.0 } else { 3.5 };
                    painter.circle_stroke(p, r, egui::Stroke::new(1.5, col));
                    painter.circle_filled(p, 1.5, col);
                    // The dot always draws — it is the click target, and it is small enough not to
                    // matter. Only the four-letter id competes for space, and it loses to city names:
                    // "TDAL" sitting across "Grapevine" is the exact overlap this pass exists for.
                    let id_rect = egui::Rect::from_min_size(
                        p + egui::vec2(6.0, -6.0),
                        egui::vec2(s.id.len() as f32 * 6.5, 12.0),
                    )
                    .expand(1.0);
                    if show_labels
                        && self.labels.place(
                            crate::labelplace::key(s.id),
                            id_rect,
                            crate::labelplace::Priority::Minor,
                        )
                    {
                        painter.text(
                            p + egui::vec2(6.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            s.id,
                            egui::FontId::monospace(10.0),
                            col,
                        );
                    }
                }
            }
        }

        // Location markers.
        for m in &self.settings.markers {
            let w = crate::render::mercator::lonlat_to_world(m.lon, m.lat);
            let (sx, sy) = cam.world_to_screen(w, vp);
            let p = egui::pos2(prect.left() + sx, prect.top() + sy);
            if !prect.contains(p) {
                continue;
            }
            let col = crate::theme::accent(self.settings.theme);
            // Home wears its watch radius: the ring is the ground truth for "within 20 miles",
            // and a circle you can see beats a number you have to trust.
            if m.home && m.alert_radius_mi > 0.0 {
                let km = m.alert_radius_mi * crate::geo::KM_PER_MILE;
                let edge = crate::geo::destination_point([m.lon, m.lat], 90.0, km);
                let ew = crate::render::mercator::lonlat_to_world(edge[0], edge[1]);
                let (ex, _) = cam.world_to_screen(ew, vp);
                let r = (prect.left() + ex - p.x).abs();
                if r > 4.0 && r < 4000.0 {
                    painter.circle_stroke(
                        p,
                        r,
                        egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 70),
                        ),
                    );
                }
            }
            // Uploaded icon if one is loaded; otherwise the default accent dot.
            let tex = m
                .icon
                .as_ref()
                .and_then(|n| self.marker_icon_tex.get(n))
                .and_then(|t| t.as_ref());
            let label_dx = if let Some(tex) = tex {
                // Round the icon into a disc with a white ring, so a marker reads as a map pin
                // rather than a photo pasted on the map. A corner radius of half the size is a
                // circle; the ring also separates a dark photo from a dark basemap.
                let d = crate::ui::marker_window::ICON_D;
                let r = egui::Rect::from_center_size(p, egui::vec2(d, d));
                painter.add(
                    egui::epaint::RectShape::filled(
                        r,
                        egui::CornerRadius::same((d / 2.0) as u8),
                        egui::Color32::WHITE,
                    )
                    .with_texture(
                        tex.id(),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    ),
                );
                painter.circle_stroke(
                    p,
                    d / 2.0,
                    egui::Stroke::new(1.5, egui::Color32::from_white_alpha(230)),
                );
                d / 2.0 + 2.0
            } else {
                painter.circle_filled(p, 4.0, col);
                painter.circle_stroke(p, 4.0, egui::Stroke::new(1.5, egui::Color32::WHITE));
                7.0
            };
            painter.text(
                p + egui::vec2(label_dx, 0.0),
                egui::Align2::LEFT_CENTER,
                &m.name,
                egui::FontId::proportional(12.0),
                col,
            );
        }

        // You, and anyone sharing their position with you. Drawn after the saved markers so a
        // moving dot is never hidden under a static one.
        let me = self.chase_pos.map(|(lon, lat)| crate::share::Peer {
            id: String::new(),
            name: "You".to_string(),
            lon,
            lat,
            ts: crate::share::now(),
            video_url: self.settings.share_video_url.clone(),
        });
        for (p, is_me) in me
            .iter()
            .map(|p| (p, true))
            .chain(self.peers.values().map(|p| (p, false)))
        {
            let w = crate::render::mercator::lonlat_to_world(p.lon, p.lat);
            let (sx, sy) = cam.world_to_screen(w, vp);
            let pt = egui::pos2(prect.left() + sx, prect.top() + sy);
            if !prect.contains(pt) {
                continue;
            }
            // You are blue (the convention every map app trained people on); peers are amber, and
            // fade as their fix ages so a frozen dot looks frozen.
            let col = if is_me {
                egui::Color32::from_rgb(60, 140, 255)
            } else {
                let age = (crate::share::now() - p.ts).clamp(0, crate::share::STALE_SECS) as f32;
                let a = 255.0 - 155.0 * (age / crate::share::STALE_SECS as f32);
                egui::Color32::from_rgba_unmultiplied(255, 180, 60, a as u8)
            };
            painter.circle_filled(
                pt,
                9.0,
                egui::Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 45),
            );
            painter.circle_filled(pt, 5.0, col);
            painter.circle_stroke(pt, 5.0, egui::Stroke::new(2.0, egui::Color32::WHITE));
            let label = if is_me {
                p.name.clone()
            } else {
                let mins = (crate::share::now() - p.ts) / 60;
                if mins > 0 {
                    format!("{} ({mins}m)", p.name)
                } else {
                    p.name.clone()
                }
            };
            painter.text(
                pt + egui::vec2(10.0, 0.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(12.0),
                col,
            );
        }

        // Isosurface over the 3D map (ROADMAP_NEW H3); built by `sync_isosurface` each frame.
        if self.views[idx].map_3d.enabled && self.views[idx].map_3d.iso_enabled {
            let v = &self.views[idx];
            let moment = v.moment;
            if let (Some(site), Some((_, shells))) = (
                v.site.as_deref().and_then(wxdata::sites::site_by_id),
                self.iso_mesh[idx].as_ref(),
            ) {
                let table =
                    crate::colormap::effective_table(&self.palettes, moment, self.settings.theme);
                let deepest = shells.iter().map(|s| s.1).max().unwrap_or(0);
                // Innermost first, so each fainter outer shell is painted over what it wraps.
                let mut order: Vec<&IsoShell> = shells.iter().collect();
                order.sort_by_key(|s| std::cmp::Reverse(s.1));
                for (value, depth, mesh) in order {
                    let c = table.sample(*value).unwrap_or([200, 200, 200, 255]);
                    // Outer shells fainter: the core reads through its envelope.
                    let fade = [1.0, 0.6, 0.35][(deepest - depth).min(2)];
                    let shape = crate::render3d::iso_mesh_screen(
                        &cam,
                        vp,
                        prect.min,
                        site.longitude as f64,
                        site.latitude as f64,
                        site.elevation_meters as f64 + wxdata::towers::tower_m(site.id),
                        v.map_3d.vertical_exaggeration as f64,
                        mesh,
                        [c[0], c[1], c[2]],
                        v.map_3d.iso_opacity * fade,
                        v.map_3d.iso_lit,
                    );
                    painter.add(egui::Shape::mesh(shape));
                }
            }
        }

        // An MRMS echo-top layer as a height surface (ROADMAP_NEW H6): translucent, with an analysis
        // grid over it, so it never reads as the observed radar volume.
        if self.views[idx].map_3d.enabled && self.views[idx].map_3d.mrms_surface {
            use crate::render::FieldLayer as FL;
            let v = &self.views[idx];
            let layer = [
                FL::MrmsEchoTop18,
                FL::MrmsEchoTop30,
                FL::MrmsEchoTop50,
                FL::MrmsEchoTop60,
            ]
            .into_iter()
            .find(|l| v.fields_on.contains(l) && self.mrms_ready(*l));
            if let (Some(layer), Some(ramp)) =
                (layer, layer.and_then(crate::render::field_ramps::ramp_for))
            {
                if let Some(grid) = self.fields.get(&layer).and_then(|f| f.grid.as_ref()) {
                    // The lon/lat box the view covers, from its corners, capped at a regional size.
                    let corners = [
                        (0.0, 0.0),
                        (vp.0, 0.0),
                        (0.0, vp.1),
                        (vp.0, vp.1),
                        (vp.0 * 0.5, vp.1 * 0.5),
                    ]
                    .map(|p| {
                        let w = cam.screen_to_world(p, vp);
                        crate::render::mercator::world_to_lonlat(w.0, w.1)
                    });
                    let (clon, clat) = corners[4];
                    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
                    for (lon, lat) in corners {
                        b = [b[0].min(lon), b[1].min(lat), b[2].max(lon), b[3].max(lat)];
                    }
                    let b = [
                        b[0].max(clon - 6.0),
                        b[1].max(clat - 4.0),
                        b[2].min(clon + 6.0),
                        b[3].min(clat + 4.0),
                    ];
                    let lut = match &ramp.scale {
                        crate::render::field_ramps::FieldScale::Ramp { stops, .. } => {
                            crate::render::field_ramps::bake_ramp_lut(stops, 255)
                        }
                        _ => Vec::new(),
                    };
                    let (mesh, lines) = crate::render3d::height_surface_screen(
                        &cam,
                        vp,
                        prect.min,
                        grid,
                        b,
                        160,
                        v.map_3d.vertical_exaggeration as f64,
                        0.5,
                        |km| {
                            let i = ramp.index(km) as usize;
                            (i > 0 && lut.len() >= (i + 1) * 4)
                                .then(|| [lut[i * 4], lut[i * 4 + 1], lut[i * 4 + 2]])
                        },
                    );
                    painter.add(egui::Shape::mesh(mesh));
                    for l in lines {
                        painter.add(egui::Shape::line(
                            l,
                            egui::Stroke::new(0.6, egui::Color32::from_white_alpha(70)),
                        ));
                    }
                }
            }
        }

        // The ground itself (ROADMAP_NEW H5), first so every other surface and shell draws over it.
        if self.views[idx].map_3d.enabled && self.views[idx].map_3d.terrain {
            if let Some(field) = self.terrain3d.grid.as_ref() {
                let v = &self.views[idx];
                let b = [
                    field.lon_west,
                    field.lat_south,
                    field.lon_east,
                    field.lat_north,
                ];
                let (mesh, _) = crate::render3d::height_surface_screen(
                    &cam,
                    vp,
                    prect.min,
                    field,
                    b,
                    150,
                    v.map_3d.vertical_exaggeration as f64,
                    0.55,
                    |km| Some(terrain3d::terrain_color(km)),
                );
                painter.add(egui::Shape::mesh(mesh));
            }
        }

        // GOES cloud top height as a surface (ROADMAP_NEW H6): pale grey to white by height, no
        // grid, fainter than the MRMS surface — satellite geometry, kept apart from both radar
        // and the MRMS analysis by look.
        if self.views[idx].map_3d.enabled && self.views[idx].map_3d.cloud_top_surface {
            if let Some((_, field)) = self.cloud_top.as_ref() {
                let v = &self.views[idx];
                let corners = [
                    (0.0, 0.0),
                    (vp.0, 0.0),
                    (0.0, vp.1),
                    (vp.0, vp.1),
                    (vp.0 * 0.5, vp.1 * 0.5),
                ]
                .map(|p| {
                    let w = cam.screen_to_world(p, vp);
                    crate::render::mercator::world_to_lonlat(w.0, w.1)
                });
                let (clon, clat) = corners[4];
                let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
                for (lon, lat) in corners {
                    b = [b[0].min(lon), b[1].min(lat), b[2].max(lon), b[3].max(lat)];
                }
                let b = [
                    b[0].max(clon - 8.0),
                    b[1].max(clat - 6.0),
                    b[2].min(clon + 8.0),
                    b[3].min(clat + 6.0),
                ];
                let (mesh, _) = crate::render3d::height_surface_screen(
                    &cam,
                    vp,
                    prect.min,
                    field,
                    b,
                    120,
                    v.map_3d.vertical_exaggeration as f64,
                    0.35,
                    |km| {
                        let t = (km / 15.0).clamp(0.0, 1.0);
                        let lerp = |a: f32, b: f32| (a + t * (b - a)) as u8;
                        Some([lerp(120.0, 250.0), lerp(130.0, 252.0), lerp(150.0, 255.0)])
                    },
                );
                painter.add(egui::Shape::mesh(mesh));
            }
        }

        // The HRRR's 0, -10 and -20 °C heights as surfaces (ROADMAP_NEW H6): one colour per level,
        // with a *dashed* grid, the forecast's own look — apart from the observed radar, the
        // solid-gridded MRMS analysis and the ungridded satellite sheet.
        if self.views[idx].map_3d.enabled && self.views[idx].map_3d.model_isotherms {
            if let Some((_, (fields, run))) = self.model_isotherms.as_ref() {
                let v = &self.views[idx];
                let corners = [
                    (0.0, 0.0),
                    (vp.0, 0.0),
                    (0.0, vp.1),
                    (vp.0, vp.1),
                    (vp.0 * 0.5, vp.1 * 0.5),
                ]
                .map(|p| {
                    let w = cam.screen_to_world(p, vp);
                    crate::render::mercator::world_to_lonlat(w.0, w.1)
                });
                let (clon, clat) = corners[4];
                let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
                for (lon, lat) in corners {
                    b = [b[0].min(lon), b[1].min(lat), b[2].max(lon), b[3].max(lat)];
                }
                let b = [
                    b[0].max(clon - 8.0),
                    b[1].max(clat - 6.0),
                    b[2].min(clon + 8.0),
                    b[3].min(clat + 6.0),
                ];
                // Highest first, so the lower, nearer surfaces paint over the ones above them.
                for (label, color, field) in fields.iter().rev() {
                    let (mesh, lines) = crate::render3d::height_surface_screen(
                        &cam,
                        vp,
                        prect.min,
                        field,
                        b,
                        // Smooth model fields: a coarse sheet reads the same and costs a third.
                        40,
                        v.map_3d.vertical_exaggeration as f64,
                        0.12,
                        |_| Some(*color),
                    );
                    // Where to name it: the surface point nearest the middle of the pane.
                    let centre = prect.center();
                    let anchor = mesh
                        .vertices
                        .iter()
                        .map(|v| v.pos)
                        .min_by(|a, b| a.distance(centre).total_cmp(&b.distance(centre)));
                    painter.add(egui::Shape::mesh(mesh));
                    let stroke = egui::Stroke::new(
                        1.2,
                        egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], 220),
                    );
                    for l in &lines {
                        painter.extend(egui::Shape::dashed_line(l, stroke, 4.0, 4.0));
                    }
                    // Its name, so each sheet says what it is.
                    if let Some(p) = anchor {
                        painter.text(
                            p,
                            egui::Align2::CENTER_BOTTOM,
                            format!("{label} · {}Z run", run.format("%H")),
                            egui::FontId::proportional(11.0),
                            egui::Color32::from_rgb(color[0], color[1], color[2]),
                        );
                    }
                }
            }
        }

        // A height ruler at the view's centre: km MSL ticks, labelled, so every surface, shell and
        // sweep in the scene can be read against a height.
        if self.views[idx].map_3d.enabled && self.views[idx].map_3d.height_ruler {
            let v = &self.views[idx];
            let (clon, clat) = crate::render::mercator::world_to_lonlat(cam.center.0, cam.center.1);
            let vex = v.map_3d.vertical_exaggeration as f64;
            // Finer ticks as exaggeration spreads them out.
            let step = if vex >= 4.0 { 1.0 } else { 2.0 };
            let ticks = crate::render3d::height_ruler(&cam, vp, clon, clat, vex, 18.0, step);
            let at = |p: (f32, f32)| egui::pos2(prect.left() + p.0, prect.top() + p.1);
            if ticks.len() >= 2 {
                let halo = egui::Stroke::new(3.0, egui::Color32::from_black_alpha(120));
                let stroke = egui::Stroke::new(1.2, egui::Color32::from_white_alpha(200));
                let pts: Vec<egui::Pos2> = ticks.iter().map(|t| at(t.1)).collect();
                painter.add(egui::Shape::line(pts.clone(), halo));
                painter.add(egui::Shape::line(pts, stroke));
                for (km, p) in &ticks {
                    let p = at(*p);
                    let major = (*km as i64) % 4 == 0;
                    let w = if major { 7.0 } else { 4.0 };
                    painter.line_segment([p - egui::vec2(w, 0.0), p + egui::vec2(w, 0.0)], stroke);
                    if major && *km > 0.0 {
                        painter.text(
                            p + egui::vec2(9.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            format!("{km:.0} km"),
                            egui::FontId::proportional(11.0),
                            egui::Color32::from_white_alpha(220),
                        );
                    }
                }
            }
        }

        // SCIT storm cells as 3D columns: the cell's base to its top, a mark at the height of its
        // strongest echo, the id and top, TVS red and meso yellow, and the past track on the ground.
        if self.views[idx].map_3d.enabled && self.views[idx].map_3d.cell_columns {
            let v = &self.views[idx];
            let vex = v.map_3d.vertical_exaggeration as f64;
            let at = |p: (f32, f32)| egui::pos2(prect.left() + p.0, prect.top() + p.1);
            let scr = |lon: f64, lat: f64, km: f64| {
                crate::render3d::lonlat_alt_screen(&cam, vp, lon, lat, km, vex).map(at)
            };
            const KM_PER_KFT: f64 = 0.3048;
            for c in self.active_storm_cells() {
                let Some(top) = c.top_kft.map(|t| t as f64 * KM_PER_KFT) else {
                    continue;
                };
                let base = c
                    .base_kft
                    .filter(|_| !c.base_below)
                    .map_or(0.0, |b| b as f64 * KM_PER_KFT);
                let color = if c.tvs.is_some() {
                    egui::Color32::from_rgb(255, 70, 70)
                } else if c.meso.is_some() {
                    egui::Color32::from_rgb(255, 220, 60)
                } else {
                    egui::Color32::from_rgb(235, 235, 235)
                };
                let track: Vec<egui::Pos2> = c
                    .past_track
                    .iter()
                    .filter_map(|&(lon, lat)| scr(lon, lat, 0.0))
                    .collect();
                if track.len() >= 2 {
                    painter.add(egui::Shape::line(
                        track,
                        egui::Stroke::new(1.5, color.gamma_multiply(0.6)),
                    ));
                }
                let (Some(g), Some(b), Some(t)) = (
                    scr(c.lon, c.lat, 0.0),
                    scr(c.lon, c.lat, base),
                    scr(c.lon, c.lat, top),
                ) else {
                    continue;
                };
                // Ground to base dashed (below the cell), base to top solid.
                painter.extend(egui::Shape::dashed_line(
                    &[g, b],
                    egui::Stroke::new(1.0, color.gamma_multiply(0.5)),
                    3.0,
                    3.0,
                ));
                painter.line_segment(
                    [b, t],
                    egui::Stroke::new(4.0, egui::Color32::from_black_alpha(110)),
                );
                painter.line_segment([b, t], egui::Stroke::new(2.0, color));
                if let Some(m) = c
                    .max_dbz_hgt_kft
                    .and_then(|h| scr(c.lon, c.lat, h as f64 * KM_PER_KFT))
                {
                    painter.circle(m, 4.0, color, egui::Stroke::new(1.0, egui::Color32::BLACK));
                }
                let mut label = format!("{} {:.0} kft", c.title, c.top_kft.unwrap_or(0.0));
                if let Some(dbz) = c.max_dbz {
                    label.push_str(&format!(" · {dbz:.0} dBZ"));
                }
                if c.tvs.is_some() {
                    label.push_str(" · TVS");
                } else if c.meso.is_some() {
                    label.push_str(" · meso");
                }
                painter.text(
                    t + egui::vec2(0.0, -4.0),
                    egui::Align2::CENTER_BOTTOM,
                    label,
                    egui::FontId::proportional(11.0),
                    color,
                );
            }
        }

        // Beam guides over the 3D map (ROADMAP_NEW H5): the geometry every observed gate sits on.
        if self.views[idx].map_3d.enabled && self.views[idx].map_3d.beam_guides {
            let v = &self.views[idx];
            if let (Some(site), Some(vol)) = (
                v.site.as_deref().and_then(wxdata::sites::site_by_id),
                v.volume.as_ref(),
            ) {
                let (rlon, rlat) = (site.longitude as f64, site.latitude as f64);
                let ground_m = site.elevation_meters as f64;
                let antenna_m = ground_m + wxdata::towers::tower_m(site.id);
                // Beams point toward whatever the view is looking at.
                let (clon, clat) =
                    crate::render::mercator::world_to_lonlat(cam.center.0, cam.center.1);
                let bearing = crate::geo::bearing_deg([rlon, rlat], [clon, clat]);
                let g = crate::render3d::beam_guides(
                    &cam,
                    vp,
                    rlon,
                    rlat,
                    ground_m,
                    antenna_m,
                    v.map_3d.vertical_exaggeration as f64,
                    v.map_3d.beam_rise as f64,
                    &vol.elevations,
                    bearing,
                    230.0,
                );
                let at = |p: &(f32, f32)| egui::pos2(prect.left() + p.0, prect.top() + p.1);
                let n = vol.elevations.len().max(2) as f32 - 1.0;
                for (tilt, ring) in &g.rings {
                    // Low tilts cyan, high tilts magenta.
                    let t = (*tilt as f32 / n).clamp(0.0, 1.0);
                    let c = egui::Color32::from_rgba_unmultiplied(
                        (80.0 + 170.0 * t) as u8,
                        (220.0 - 150.0 * t) as u8,
                        240,
                        110,
                    );
                    let pts: Vec<egui::Pos2> = ring.iter().map(at).collect();
                    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.0, c)));
                }
                for (edge, line) in &g.beams {
                    let pts: Vec<egui::Pos2> = line.iter().map(at).collect();
                    if *edge {
                        painter.extend(egui::Shape::dashed_line(
                            &pts,
                            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(150)),
                            4.0,
                            4.0,
                        ));
                    } else {
                        painter.add(egui::Shape::line(
                            pts,
                            egui::Stroke::new(1.8, egui::Color32::WHITE),
                        ));
                    }
                }
                if let Some([a, b]) = g.mast {
                    painter.line_segment(
                        [at(&a), at(&b)],
                        egui::Stroke::new(2.0, egui::Color32::WHITE),
                    );
                    painter.circle_filled(at(&b), 3.5, egui::Color32::WHITE);
                }
            }
        }

        // Routes (ROADMAP_NEW L2): alternatives thin and grey, the chosen one wide and blue over a
        // dark casing so it reads over radar, and the waypoints lettered.
        if !self.route_window.routes.is_empty() || !self.route_window.waypoints.is_empty() {
            let screen = |p: &[f64; 2]| {
                let w = crate::render::mercator::lonlat_to_world(p[0], p[1]);
                let (sx, sy) = cam.world_to_screen(w, vp);
                egui::pos2(prect.left() + sx, prect.top() + sy)
            };
            let chosen = self.route_window.selected;
            for (i, r) in self.route_window.routes.iter().enumerate() {
                if i == chosen {
                    continue;
                }
                let pts: Vec<egui::Pos2> = r.coords.iter().map(screen).collect();
                painter.add(egui::Shape::line(
                    pts,
                    egui::Stroke::new(
                        3.0,
                        egui::Color32::from_rgba_unmultiplied(170, 170, 185, 170),
                    ),
                ));
            }
            if let Some(r) = self.route_window.routes.get(chosen) {
                let pts: Vec<egui::Pos2> = r.coords.iter().map(screen).collect();
                painter.add(egui::Shape::line(
                    pts.clone(),
                    egui::Stroke::new(7.0, egui::Color32::from_black_alpha(170)),
                ));
                painter.add(egui::Shape::line(
                    pts,
                    egui::Stroke::new(4.0, egui::Color32::from_rgb(70, 150, 255)),
                ));
            }
            for (i, p) in self.route_window.waypoints.iter().enumerate() {
                let at = screen(p);
                painter.circle(
                    at,
                    8.0,
                    egui::Color32::from_rgb(70, 150, 255),
                    egui::Stroke::new(1.5, egui::Color32::WHITE),
                );
                painter.text(
                    at,
                    egui::Align2::CENTER_CENTER,
                    ((b'A' + (i as u8).min(25)) as char).to_string(),
                    egui::FontId::proportional(10.0),
                    egui::Color32::WHITE,
                );
            }
        }

        // Freehand annotation strokes. Painted with the rest of the tool graphics so they sit
        // above every overlay, and drawn in OBS mode too — circling a storm on a stream is the
        // whole point of the tool.
        for st in &self.strokes {
            if st.points.len() < 2 {
                continue;
            }
            let pts: Vec<egui::Pos2> = st
                .points
                .iter()
                .map(|ll| {
                    let w = crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    egui::pos2(prect.left() + sx, prect.top() + sy)
                })
                .collect();
            painter.add(egui::Shape::line(pts, egui::Stroke::new(2.5, st.color)));
        }

        // Imported GIS points and lines (ROADMAP_NEW I1). The polygon half of an import rides the
        // overlay pipeline like every NWS feed's does; these two geometries have no rings to put
        // there, so they paint here through the same lon/lat projection as the strokes above.
        let imported_shown =
            self.show_imported_gis && self.settings.imported_gis_style.visible_at(cam.zoom);
        if imported_shown && !self.imported_marks.is_empty() {
            let style = self.settings.imported_gis_style;
            let c = style.stroke_rgba();
            let layer_color = egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
            // A mark coloured by attribute keeps the layer's opacity.
            let colors = self.imported_colors.as_ref().map(|(_, c, _)| c);
            let color_of = |src: Option<&usize>| {
                colors
                    .and_then(|c| *c.get(*src?)?)
                    .map_or(layer_color, |[r, g, b]| {
                        egui::Color32::from_rgba_unmultiplied(r, g, b, c[3])
                    })
            };
            let width = style.rendered_stroke_width();
            let screen = |ll: &[f64; 2]| {
                let w = crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
                let (sx, sy) = cam.world_to_screen(w, vp);
                egui::pos2(prect.left() + sx, prect.top() + sy)
            };
            let marks = &self.imported_marks;
            for (i, line) in marks.lines.iter().enumerate() {
                if !self.imported_valid(marks.line_src.get(i)) {
                    continue;
                }
                let pts: Vec<egui::Pos2> = line.iter().map(screen).collect();
                let color = color_of(marks.line_src.get(i));
                painter.add(egui::Shape::line(pts, egui::Stroke::new(width, color)));
            }
            for (i, point) in marks.points.iter().enumerate() {
                if !self.imported_valid(marks.point_src.get(i)) {
                    continue;
                }
                let p = screen(point);
                if !prect.contains(p) {
                    continue;
                }
                let color = color_of(marks.point_src.get(i));
                // Outlined rather than a plain dot: an imported site has to stay visible over both
                // a bright radar core and a dark basemap, which one flat color cannot manage.
                // The outline-width control also scales point symbols so a mixed-geometry file
                // keeps one coherent visual weight. The default 1.6 px remains the old 3.5 px dot.
                let radius = 2.5 + width * 0.625;
                painter.circle_filled(p, radius, color);
                painter.circle_stroke(
                    p,
                    radius,
                    egui::Stroke::new(1.0, egui::Color32::from_black_alpha(180)),
                );
            }
        }
        // Labels from the chosen attribute (I4), for every geometry family. Decluttered on a
        // coarse screen grid in file order: a label whose cell is taken is skipped, so a dense
        // file reads as a scatter of names rather than an unreadable smear, and more appear as
        // the map zooms in.
        if let Some(key) = self
            .settings
            .imported_gis_label
            .as_deref()
            .filter(|_| imported_shown)
        {
            let c = self.settings.imported_gis_style.stroke_rgba();
            let text_color = egui::Color32::from_rgb(
                c[0].saturating_add(90),
                c[1].saturating_add(90),
                c[2].saturating_add(90),
            );
            let font = egui::FontId::proportional(11.5);
            let (cell_w, cell_h) = (90.0_f32, 18.0_f32);
            let mut taken = std::collections::HashSet::new();
            let mut drawn = 0;
            for &(at, src) in &self.imported_marks.anchors {
                if !self.imported_valid(Some(&src)) {
                    continue;
                }
                if drawn >= 600 {
                    break;
                }
                let w = crate::render::mercator::lonlat_to_world(at[0], at[1]);
                let (sx, sy) = cam.world_to_screen(w, vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.shrink(4.0).contains(p) {
                    continue;
                }
                let cell = ((p.x / cell_w) as i32, (p.y / cell_h) as i32);
                if !taken.insert(cell) {
                    continue;
                }
                let Some(text) = self
                    .imported_marks
                    .props
                    .get(src)
                    .and_then(|props| crate::gis_import::label_text(props, key))
                else {
                    continue;
                };
                let galley = painter.layout_no_wrap(text, font.clone(), text_color);
                // Beside a point's dot, centred on a line or polygon's anchor; a dark halo keeps
                // it legible over radar and basemap alike.
                let pos = p + egui::vec2(6.0, -galley.size().y * 0.5);
                for d in [
                    egui::vec2(-1.0, 0.0),
                    egui::vec2(1.0, 0.0),
                    egui::vec2(0.0, -1.0),
                    egui::vec2(0.0, 1.0),
                ] {
                    painter.galley_with_override_text_color(
                        pos + d,
                        galley.clone(),
                        egui::Color32::from_black_alpha(200),
                    );
                }
                painter.galley(pos, galley, text_color);
                drawn += 1;
            }
        }

        // Saved watch zones, plus the one being clicked out right now.
        {
            let screen = |ll: [f64; 2]| {
                let w = crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
                let (sx, sy) = cam.world_to_screen(w, vp);
                egui::pos2(prect.left() + sx, prect.top() + sy)
            };
            let zone_col = egui::Color32::from_rgb(120, 200, 255);
            for z in &self.settings.alert_polygons {
                if z.ring.len() < 3 {
                    continue;
                }
                let pts: Vec<egui::Pos2> = z.ring.iter().map(|&p| screen(p)).collect();
                painter.add(egui::Shape::convex_polygon(
                    pts.clone(),
                    egui::Color32::from_rgba_unmultiplied(120, 200, 255, 26),
                    egui::Stroke::new(1.5, zone_col),
                ));
                if let Some(first) = pts.first() {
                    painter.text(
                        *first + egui::vec2(4.0, -4.0),
                        egui::Align2::LEFT_BOTTOM,
                        &z.name,
                        egui::FontId::proportional(11.0),
                        zone_col,
                    );
                }
            }
            if !self.zone_pts.is_empty() {
                let pts: Vec<egui::Pos2> = self.zone_pts.iter().map(|&p| screen(p)).collect();
                for p in &pts {
                    painter.circle_filled(*p, 3.0, zone_col);
                }
                if pts.len() >= 2 {
                    // Closing edge dashed in, so the shape being made is obvious before it is.
                    let mut loop_pts = pts.clone();
                    loop_pts.push(pts[0]);
                    painter.add(egui::Shape::line(
                        loop_pts,
                        egui::Stroke::new(1.5, zone_col),
                    ));
                }
            }
        }

        // Chase breadcrumbs: where this session has been, under everything else on the map.
        if self.settings.chase_log && self.chase_track.points.len() > 1 {
            let col = egui::Color32::from_rgb(255, 170, 60);
            let pts: Vec<egui::Pos2> = self
                .chase_track
                .points
                .iter()
                .map(|f| {
                    let w = crate::render::mercator::lonlat_to_world(f.lon, f.lat);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    egui::pos2(prect.left() + sx, prect.top() + sy)
                })
                .collect();
            painter.add(egui::Shape::line(pts.clone(), egui::Stroke::new(2.0, col)));
            for (idx, label) in &self.chase_track.waypoints {
                if let Some(p) = pts.get(*idx) {
                    painter.circle_filled(*p, 4.0, col);
                    painter.text(
                        *p + egui::vec2(6.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(11.0),
                        col,
                    );
                }
            }
        }

        // Measure tool.
        if !self.measure.is_empty() {
            let col = egui::Color32::from_rgb(255, 210, 80);
            let screen = |ll: [f64; 2]| {
                let w = crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
                let (sx, sy) = cam.world_to_screen(w, vp);
                egui::pos2(prect.left() + sx, prect.top() + sy)
            };
            for &pt in &self.measure {
                painter.circle_filled(screen(pt), 3.5, col);
            }
            if self.measure.len() == 2 {
                let (a, b) = (screen(self.measure[0]), screen(self.measure[1]));
                painter.line_segment([a, b], egui::Stroke::new(2.0, col));
                let (km, brg) = crate::geo::great_circle(self.measure[0], self.measure[1]);
                let mut txt = format!(
                    "{}  @ {brg:.0}°",
                    crate::geo::fmt_distance(km, self.metric_in(idx), 1)
                );
                // How high the beam is over the far end of the line. The number that decides
                // whether "there's nothing on radar there" means the storm is weak or means the
                // scan is looking over its head, and until now it lived only in the cross-section.
                if let Some(h) = self.beam_height_ft(idx, self.measure[1]) {
                    txt.push_str(&format!("  ·  beam {h:.0} ft"));
                }
                let mid = a + (b - a) * 0.5;
                painter.text(
                    mid + egui::vec2(0.0, -10.0),
                    egui::Align2::CENTER_BOTTOM,
                    txt,
                    egui::FontId::proportional(12.0),
                    col,
                );
            }
        }

        self.region.paint(&painter, |ll| {
            let w = crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
            let (sx, sy) = cam.world_to_screen(w, vp);
            egui::pos2(prect.left() + sx, prect.top() + sy)
        });

        // Cross-section endpoints + line (cyan, distinct from the yellow measure tool).
        if !self.xsection_pts.is_empty() {
            let col = egui::Color32::from_rgb(90, 220, 255);
            let screen = |ll: [f64; 2]| {
                let w = crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
                let (sx, sy) = cam.world_to_screen(w, vp);
                egui::pos2(prect.left() + sx, prect.top() + sy)
            };
            for &pt in &self.xsection_pts {
                painter.circle_filled(screen(pt), 3.5, col);
            }
            if self.xsection_pts.len() == 2 {
                let (a, b) = (screen(self.xsection_pts[0]), screen(self.xsection_pts[1]));
                painter.line_segment([a, b], egui::Stroke::new(2.0, col));
                painter.text(
                    a,
                    egui::Align2::RIGHT_BOTTOM,
                    "A",
                    egui::FontId::proportional(12.0),
                    col,
                );
                painter.text(
                    b,
                    egui::Align2::LEFT_BOTTOM,
                    "B",
                    egui::FontId::proportional(12.0),
                    col,
                );
            }
        }

        // The 3D map's own vertical clip plane (H4's "cross-section line visible in map pane"):
        // ties the 3D view's cut back to geographic context instead of leaving it visible only
        // from inside the 3D view itself. Only meaningful for the Smooth representations —
        // ObservedSweeps raymarches real gate instances with no box for a plane to cut into.
        let map3d = &self.views[idx].map_3d;
        if map3d.enabled && map3d.representation != Map3dRepresentation::ObservedSweeps {
            if let (Some(plane), Some((.., half_km, _))) = (map3d.plane, self.smooth_vol_dims[idx])
            {
                if let Some(site) = self.views[idx]
                    .site
                    .as_deref()
                    .and_then(wxdata::sites::site_by_id)
                {
                    let (a, b) = crate::render3d::plane_ground_track(
                        plane,
                        [site.longitude as f64, site.latitude as f64],
                        half_km,
                    );
                    let col = egui::Color32::from_rgb(200, 130, 255);
                    let screen = |ll: [f64; 2]| {
                        let w = crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
                        let (sx, sy) = cam.world_to_screen(w, vp);
                        egui::pos2(prect.left() + sx, prect.top() + sy)
                    };
                    painter.line_segment([screen(a), screen(b)], egui::Stroke::new(2.0, col));
                }
            }
        }

        // Historical tornado tracks from the last climatology query (magnitude-colored segments).
        if self.climo_open && !self.climo_hits.is_empty() {
            let screen = |lon: f64, lat: f64| {
                let w = crate::render::mercator::lonlat_to_world(lon, lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                egui::pos2(prect.left() + sx, prect.top() + sy)
            };
            for t in &self.climo_hits {
                let col = tornado_mag_color(t.mag);
                let a = screen(t.slon, t.slat);
                let b = screen(t.elon, t.elat);
                if !prect.contains(a) && !prect.contains(b) {
                    continue;
                }
                painter.line_segment([a, b], egui::Stroke::new(2.0, col));
                painter.circle_filled(a, 2.5, col);
            }
            if let Some((lon, lat)) = self.climo_center {
                let c = screen(lon, lat);
                painter.circle_stroke(c, 5.0, egui::Stroke::new(2.0, egui::Color32::WHITE));
            }
        }

        // Alert spotlight: while an alert's card is open, dim the map outside its polygon. Under
        // the legends, so the scale stays readable.
        if self.settings.alert_spotlight {
            if let Some(id) = self.warning_popup.as_ref().and_then(|p| {
                p.cards
                    .get(p.selected.unwrap_or(0))
                    .map(|c| c.info.id.clone())
            }) {
                let screen = |p: &[f64; 2]| {
                    let w = crate::render::mercator::lonlat_to_world(p[0], p[1]);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    egui::pos2(prect.left() + sx, prect.top() + sy)
                };
                let rings: Vec<Vec<egui::Pos2>> = self
                    .active_alert_features()
                    .iter()
                    .filter(|f| f.alert.as_ref().is_some_and(|a| a.id == id))
                    .flat_map(|f| f.rings.iter())
                    .map(|ring| ring.iter().map(screen).collect())
                    .collect();
                if let Some(mesh) =
                    crate::spotlight::dim_outside(prect, &rings, crate::spotlight::DIM_ALPHA)
                {
                    painter.add(egui::Shape::mesh(mesh));
                }
            }
        }

        // Y'all Tracks: storms headed for the Y'all spot, on the active pane.
        if self.settings.yall_mode && idx == self.active {
            let screen = |p: &[f64; 2]| {
                let w = crate::render::mercator::lonlat_to_world(p[0], p[1]);
                let (sx, sy) = cam.world_to_screen(w, vp);
                egui::pos2(prect.left() + sx, prect.top() + sy)
            };
            self.draw_yall_tracks(&painter, &screen);
        }

        // The boxed legend is desktop-only; Android draws a full-width color scale in the mobile
        // chrome (see `app::mobile`), so drawing both would be redundant.
        // The phone keeps the thin strip along the top edge in every design; Storm and Carbon add a
        // tall scale down the edge opposite their rail, and Atlas a small boxed one in the corner.
        if crate::platform::phone_layout() && view.show_legend && view.volume.is_some() {
            use crate::ui::phone_design::Legend;
            let (df, dl) = display_units(view.moment, &self.settings);
            let table = self.palettes.table(view.moment);
            // Clear of the pill, mode bar and rail above, and the timeline below. Station's bars
            // and sheet are docked around the map, so its scale only keeps off the edges.
            let (top, clear_bottom) = if self.phone_station() {
                // Full screen, the way back (the eye) sits in the top-right corner.
                (
                    if self.mobile_chrome_hidden {
                        76.0
                    } else {
                        10.0
                    },
                    10.0,
                )
            } else {
                (
                    chrome::phone_top(ui.ctx()) + 56.0 + chrome::MODE_BAR_H + 8.0,
                    132.0 + self.phone_nav_h(),
                )
            };
            match self.settings.phone_design.spec().legend {
                Legend::StripOnly => {}
                Legend::Vertical => ui::legend::draw_vertical(
                    &painter,
                    egui::Rect::from_min_max(
                        egui::pos2(prect.left(), prect.top() + top),
                        egui::pos2(prect.right(), prect.bottom() - clear_bottom),
                    ),
                    view.moment,
                    table,
                    view.active_threshold(),
                    df,
                    dl,
                ),
                Legend::Box => ui::legend::draw_box(
                    &painter,
                    prect,
                    prect.bottom() - clear_bottom + 12.0,
                    &format!("{} ({dl})", crate::products::name(view.moment, false)),
                    table,
                    view.moment,
                    df,
                ),
            }
        }
        // Streaming mode can take the scale off the picture (`Broadcast::legend`).
        let legend_allowed = !(self.obs_mode && !self.settings.broadcast.legend);
        if view.show_legend && legend_allowed && !crate::platform::phone_layout() {
            // The moment's scale floats over this pane's right edge (no panel, no card) so the map
            // keeps the pixels; the field/wind ramps still need their cards. The WSV3 layout docks
            // this same scale under the ribbon, so drawing it here too would be the third copy.
            let wsv3_colorbar =
                self.settings.layout.is_ribbon() && !crate::platform::phone_layout();
            if view.volume.is_some() && !wsv3_colorbar {
                if let Some((table, _, units)) = self.product_legend(idx) {
                    ui::legend::draw_vertical(
                        &painter,
                        prect,
                        view.moment,
                        &table,
                        None,
                        1.0,
                        &units,
                    );
                } else {
                    let (df, dl) = display_units(view.moment, &self.settings);
                    ui::legend::draw_vertical(
                        &painter,
                        prect,
                        view.moment,
                        self.palettes.table(view.moment),
                        view.active_threshold(),
                        df,
                        dl,
                    );
                }
            }
            // The field cards stack down the pane's top-left corner, which in the full-overlay
            // chrome is where the search pill floats — the first card was drawn half under it.
            // Every pane ducks by the same amount rather than only the top row: in a 2x2 grid the
            // lower cards then sit a little further from their pane's edge, which nobody notices,
            // and the alternative is a rect comparison that has to know about window insets.
            let mut y = 48.0;
            // Whichever gridded layer the user actually sees on top — the last enabled one in
            // paint order — gets its scale keyed underneath. Without this, MESH/QPE/VIL and the
            // categorical classifications were unlabeled color.
            if let Some(top) = crate::render::FieldLayer::paint_order(&self.settings.field_order)
                .iter()
                .rev()
                .find(|l| {
                    view.fields_on.contains(l)
                        && crate::fielddiff::layer_ready(**l, self.diff_valid, self.compare_valid)
                })
            {
                use crate::render::FieldLayer as FL;
                if *top == FL::ModelDiff {
                    y += ui::legend::draw_diff(&painter, prect, self.diff_field, self.diff_mode, y);
                } else if *top == FL::Ensemble {
                    y += ui::legend::draw_ensemble(
                        &painter,
                        prect,
                        &self.ensemble,
                        y,
                        self.settings.temp_unit,
                    );
                } else if matches!(*top, FL::CompareA | FL::CompareB) {
                    let (label_a, label_b) = self.diff_field.pair();
                    let model = if view.swipe_compare {
                        format!("{label_a} A | B {label_b}")
                    } else if view.overlay_compare {
                        format!("{label_a} + 50% {label_b}")
                    } else if *top == FL::CompareA {
                        label_a.into()
                    } else {
                        label_b.into()
                    };
                    y += ui::legend::draw_compare_label(&painter, prect, y, &model);
                    y += ui::legend::draw_field(
                        &painter,
                        prect,
                        self.diff_field.source_layer(),
                        y,
                        self.settings.temp_unit,
                    );
                } else {
                    y += ui::legend::draw_field(&painter, prect, *top, y, self.settings.temp_unit);
                }
            }
            // Wind particles carry their own scale — it isn't a FieldLayer, so it needs its own
            // call rather than a slot in DRAW_ORDER.
            if self.show_wind && self.wind.is_some() {
                ui::legend::draw_ramp(
                    &painter,
                    prect,
                    &crate::render::field_ramps::WIND,
                    y,
                    self.settings.temp_unit,
                );
            }
        }
    }

    /// Resize the pane grid to `n` (1/2/4). New panes copy the active pane's site/camera but
    /// default to a distinct product, so a 4-panel shows REF/VEL/ZDR/RHO out of the box.
    /// Four panes of the SAME product at four different tilts — the layout you build by hand
    /// every time you want to see how a couplet leans with height.
    ///
    /// SAILS/MRLE re-scan the lowest cut mid-volume, so the elevation list repeats angles; taking
    /// four *distinct* ones is what makes the quad show four heights instead of three plus a
    /// duplicate.
    fn apply_all_tilts(&mut self) {
        let src = &self.views[self.active];
        let moment = src.moment;
        let srv = src.srv;
        let elevations = src
            .volume
            .as_ref()
            .map(|v| v.elevations.clone())
            .unwrap_or_default();
        let picks = distinct_tilts(&elevations, 4);
        self.set_pane_count(4);
        for (i, v) in self.views.iter_mut().enumerate() {
            v.moment = moment;
            v.srv = srv;
            if let Some(&t) = picks.get(i) {
                v.tilt = t;
            }
        }
        // Four heights of one storm only reads if all four look at the same place.
        self.link_cameras = true;
        self.link_times = true;
        self.pane_shown.clear();
    }

    /// Two panes, one model's own field in each (`self.diff_field`) — the side-by-side
    /// alternative to the `ModelDiff` subtraction layer. Pane 0 gets `CompareA`, pane 1
    /// `CompareB`; the subtraction layer is turned off in both, since all three drawn over each
    /// other answers a question nobody asked.
    fn apply_compare_panes(&mut self) {
        if !self.diff_field.supports_side_by_side() {
            return;
        }
        use crate::render::FieldLayer as FL;
        self.set_pane_count(2);
        for (i, view) in self.views.iter_mut().enumerate() {
            let (add, remove) = if i == 0 {
                (FL::CompareA, FL::CompareB)
            } else {
                (FL::CompareB, FL::CompareA)
            };
            view.fields_on.insert(add);
            view.fields_on.remove(&remove);
            view.fields_on.remove(&FL::ModelDiff);
            // Side by side already shows both fields, persistently, one per pane — blinking on
            // top of that would fight the very point of splitting into two panes.
            view.blink_compare = false;
            view.overlay_compare = false;
            view.swipe_compare = false;
        }
        // Two panes of the same field only reads if both look at the same place.
        self.link_cameras = true;
    }

    /// How visible the wind layer should be in this pane: faded out past the zoom where the
    /// 0.04-degree regrid goes visibly piecewise-linear, dimmed over reflectivity so the radar
    /// stays readable, and dimmed again when the pane is scrubbed to a time these grids are not
    /// valid for.
    fn wind_alpha(&self, idx: usize, zoom: f64) -> f32 {
        let zoom_fade = (13.0 - zoom).clamp(0.0, 1.0) as f32;
        let v = &self.views[idx];
        let over_radar = v.show_radar
            && matches!(v.moment, wxdata::level2::Moment::Reflectivity)
            && v.volume.is_some();
        let off_live = !v.timeline.following;
        zoom_fade * if over_radar { 0.7 } else { 1.0 } * if off_live { 0.4 } else { 1.0 }
    }

    /// What the GPU wind layer needs this frame: the field to upload if it changed, and the
    /// per-frame camera and timing. `None` when the layer is off, faded out, or on the CPU.
    fn wind_gpu_frame(
        &mut self,
        idx: usize,
        cam: &Camera,
        vp: (f32, f32),
    ) -> (
        Option<Box<crate::wind_gpu::WindGrid>>,
        Option<crate::wind_gpu::Frame>,
    ) {
        let alpha = self.wind_alpha(idx, cam.zoom);
        if !self.show_wind || !self.wind_on_gpu || alpha <= 0.01 {
            return (None, None);
        }
        let Some(field) = self.wind.as_ref() else {
            return (None, None);
        };
        // The grid's own bbox in mercator world units — the particles live in it, so it is also
        // the space their positions are stored in.
        let (west, north) =
            crate::render::mercator::lonlat_to_world(field.u.lon_west, field.u.lat_north);
        let (east, south) =
            crate::render::mercator::lonlat_to_world(field.u.lon_east, field.u.lat_south);
        let bbox_min = [west as f32, north as f32];
        let bbox_max = [east as f32, south as f32];

        // Warp the lon/lat grid onto the mercator-uniform texture the shader samples. Once per
        // new field, on the frame it arrives — not per frame, and not on the GPU, where it would
        // cost a dependent texture fetch in the hot path.
        let key = (field.run, field.fcst_hour, field.level);
        let upload = (self.wind_uploaded != Some(key)).then(|| {
            self.wind_uploaded = Some(key);
            Box::new(crate::wind_gpu::WindGrid {
                rgba: crate::wind_gpu::warp_field(field, bbox_min, bbox_max),
                bbox_min,
                bbox_max,
            })
        });

        let (center, scale) = cam.world_to_clip_uniform(vp);
        (
            upload,
            Some(crate::wind_gpu::Frame {
                bbox_min,
                bbox_max,
                center,
                scale,
                dt: self.wind_dt,
                // Pixels per world unit is the camera's own scale factor; the particle step is
                // calibrated in pixels, so the shader needs its inverse.
                world_per_px: (1.0 / (256.0 * 2f64.powf(cam.zoom))) as f32,
                opacity: alpha,
                viewport: (vp.0 as u32, vp.1 as u32),
            }),
        )
    }

    /// The app's own commands — the ones that aren't a layer, product, tool or window, and so
    /// have no place in the action registry: view toggles, chase, capture, settings bundles.
    /// Compact preference groups; details stay collapsed until needed.
    fn app_rows(&mut self, ui: &mut egui::Ui) {
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
                        egui::Button::new("").corner_radius(10.0),
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

    /// Put `style` under the active pane and remember it.
    ///
    /// Remembering is the point: the basemap used to live only on the view, so a style picked
    /// during a chase was gone at the next launch, which read `settings.basemap` and found
    /// whatever the first-run card wrote months earlier.
    pub(crate) fn set_basemap(&mut self, style: crate::tiles::BasemapStyle) {
        self.views[self.active].basemap = style;
        if self.settings.basemap != style.slug() {
            self.settings.basemap = style.slug().to_string();
            self.settings.save();
        }
    }

    /// Deep-link the active pane to a site + camera, and (for archive) seek the timeline to
    /// `time`. Passing `time = None` leaves the pane live at the head.
    pub(crate) fn goto_view(
        &mut self,
        site: &str,
        lon: f64,
        lat: f64,
        zoom: f64,
        time: Option<chrono::DateTime<chrono::Utc>>,
    ) {
        use crate::render::mercator::lonlat_to_world;
        let v = &mut self.views[self.active];
        // An empty site means "fly there, keep the radar" — the Android alert notification uses it,
        // since the service knows a latitude and longitude but nothing about radar coverage.
        if !site.is_empty() {
            v.site = Some(site.to_ascii_uppercase());
        }
        v.camera = crate::render::mercator::Camera {
            center: lonlat_to_world(lon, lat),
            zoom,
            pitch: 0.0,
            bearing: 0.0,
        };
        v.camera_placed = true;
        if let Some(t) = time {
            v.timeline.date = t.date_naive();
            v.timeline.following = false;
            v.timeline.playing = false;
            v.timeline.seek_target = Some(t);
        } else {
            v.timeline.go_head();
        }
        if self.link_times {
            self.linked_analysis.select_explicit(
                self.active,
                self.views[self.active].site.as_deref(),
                time,
            );
        }
    }

    /// Save the active pane's current view as a named bookmark (archive time captured if scrubbed).
    pub(crate) fn add_bookmark(&mut self, name: String, span_min: u16) {
        let v = &self.views[self.active];
        let Some(site) = v.site.clone() else { return };
        let time_secs = v
            .timeline
            .current()
            .and_then(|id| id.date_time())
            .map(|t| t.timestamp());
        self.settings.bookmarks.push(crate::settings::Bookmark {
            name,
            site,
            x: v.camera.center.0,
            y: v.camera.center.1,
            zoom: v.camera.zoom,
            time_secs,
            span_min,
        });
    }

    /// Export settings + referenced color tables to a portable JSON bundle (a save dialog on
    /// desktop, a download in a browser).
    fn export_settings_bundle(&mut self) {
        let json = match self.settings.export_bundle() {
            Ok(json) => json,
            Err(e) => {
                log::warn!("settings export failed: {e}");
                self.toast(ToastKind::Error, format!("Settings export failed: {e}"));
                return;
            }
        };
        match crate::dialog::save_bytes("hookecho-settings.json", "json", json.as_bytes()) {
            crate::dialog::Saved::Where(w) => {
                self.toast(ToastKind::Success, format!("Settings saved to {w}"))
            }
            crate::dialog::Saved::Failed(e) => {
                log::warn!("settings export failed: {e}");
                self.toast(ToastKind::Error, format!("Settings export failed: {e}"));
            }
            crate::dialog::Saved::Cancelled => {}
        }
    }

    /// ROADMAP_NEW N4's local diagnostics bundle: app version, platform, renderer, every active
    /// source's current health, recent warnings/errors, on-disk cache size, and the process-life
    /// performance counters, as one exportable JSON. Deliberately absent: no location history, no
    /// API keys, no filesystem paths of the user's own — the same discipline `crash.rs`'s own
    /// panic report already commits to.
    /// Every active source's health as the diagnostics bundle and the local API report it.
    fn diagnostics_source_health(&mut self) -> Vec<DiagnosticsSourceHealth> {
        let entries = self.palette_entries();
        ui::source_health_window::active_health_rows(&entries)
            .into_iter()
            .map(|h| DiagnosticsSourceHealth {
                source: h.source.clone(),
                endpoint_family: h.endpoint_family.id(),
                latest_valid_time: h.latest_valid_time.map(|t| t.to_rfc3339()),
                fallback_providers: h.fallback_providers.clone(),
                cache_state: h.cache_state.id(),
                status: ui::layers_panel::health_look(h.state()).0,
                last_success_secs: h.last_success.map(|d| d.as_secs()),
                cadence_secs: h.cadence.as_secs(),
                recent_successes: h.recent_outcomes.map(|(s, _)| s),
                recent_failures: h.recent_outcomes.map(|(_, f)| f),
                error: h.error.clone(),
                details: h.details.clone(),
            })
            .collect()
    }

    fn export_diagnostics_bundle(&mut self) {
        let source_health = self.diagnostics_source_health();
        let bundle = DiagnosticsBundle {
            generated_at: chrono::Utc::now().to_rfc3339(),
            version: ui::about_window::VERSION,
            platform: std::env::consts::OS,
            renderer: self.gpu_info.clone(),
            cache_bytes: crate::paths::cache_dir_bytes(),
            performance_counters: wxdata::stats::snapshot().into_iter().collect(),
            source_health,
            recent_warnings: crate::devlog::recent_warnings(200),
        };
        let json = match serde_json::to_string_pretty(&bundle) {
            Ok(json) => json,
            Err(e) => {
                log::warn!("diagnostics export failed: {e}");
                self.toast(ToastKind::Error, format!("Diagnostics export failed: {e}"));
                return;
            }
        };
        match crate::dialog::save_bytes("hookecho-diagnostics.json", "json", json.as_bytes()) {
            crate::dialog::Saved::Where(w) => {
                self.toast(ToastKind::Success, format!("Diagnostics saved to {w}"))
            }
            crate::dialog::Saved::Failed(e) => {
                log::warn!("diagnostics export failed: {e}");
                self.toast(ToastKind::Error, format!("Diagnostics export failed: {e}"));
            }
            crate::dialog::Saved::Cancelled => {}
        }
    }

    /// Bring back the GeoJSON import this user last chose, at startup. Silent when there is none.
    ///
    /// A file that has since moved or been deleted is reported rather than swallowed: the layer
    /// simply not being there is otherwise indistinguishable from the app having forgotten it,
    /// and the person is the only one who can fix a missing file. The reference is kept either
    /// way — a path on a drive that is merely not mounted right now should come back next time,
    /// not be quietly forgotten because of one failed launch.
    fn reload_imported_gis(&mut self) {
        let Some(key) = self.settings.imported_gis.clone() else {
            return;
        };
        // A browser's remembered content is text (a shapefile or KMZ there was stored as
        // GeoJSON, a KML as itself); a path is read whichever format it is, a shapefile picking
        // up its .dbf and .prj again.
        let loaded = match self.settings.web_files.get(&key) {
            Some(text) if crate::gis_import::is_kml(&key) => crate::gis_import::load_kml(text),
            Some(text) => crate::gis_import::load_geojson(text),
            None => crate::gis_import::load_path(&key),
        };
        match loaded {
            Ok(loaded) => {
                let (shapes, marks) = crate::gis_import::to_renderable(loaded.features);
                self.imported_gis = shapes;
                self.imported_marks = marks;
                self.imported_colors = None;
                self.imported_time = None;
                // The remembered layer should actually come back, not merely sit loaded and
                // invisible until the user rediscovers its toggle after every restart.
                self.show_imported_gis = true;
                self.rebuild_overlays();
            }
            Err(e) => {
                let name = self.settings.imported_gis.clone().unwrap_or_default();
                log::warn!("could not reload the imported GIS file {name}: {e}");
                self.toast(
                    ToastKind::Error,
                    format!("Couldn't reload your imported shapes from {name}: {e}"),
                );
            }
        }
    }

    /// Frame the active pane on everything the last GeoJSON import brought in. A file covering
    /// somewhere the map isn't currently looking otherwise imports to no visible effect at all —
    /// the shapes are real, just off-screen.
    fn zoom_to_imported_gis(&mut self) {
        let Some((west, south, east, north)) =
            crate::gis_import::bounds(&self.imported_gis, &self.imported_marks)
        else {
            self.toast(
                ToastKind::Error,
                "No imported shapes to zoom to".to_string(),
            );
            return;
        };
        let view = &mut self.views[self.active];
        let (center_lon, center_lat) = ((west + east) / 2.0, (south + north) / 2.0);
        // Span in world units rather than degrees: latitude degrees do not have a constant world
        // height under Mercator, so fitting on degrees would overshoot badly away from the equator.
        let (x0, y0) = crate::render::mercator::lonlat_to_world(west, north);
        let (x1, y1) = crate::render::mercator::lonlat_to_world(east, south);
        let span = (x1 - x0).abs().max((y1 - y0).abs());
        // A single point (or a shape smaller than a pixel) has no span to fit; a fixed
        // neighbourhood-scale zoom is the only sensible answer there.
        let zoom = if span > 1e-9 {
            // `2^zoom` tiles span the world per axis, so fitting `span` of the world into the
            // viewport means `2^zoom * span` tiles across it. Back off one notch so the outermost
            // shapes sit inside the edge rather than exactly on it.
            (1.0 / span).log2().clamp(1.0, 14.0) - 0.5
        } else {
            10.0
        };
        view.camera = crate::render::mercator::Camera::at_lonlat(center_lon, center_lat, zoom);
    }

    /// ROADMAP_NEW I6: write everything currently drawn on the map out as one GeoJSON file.
    ///
    /// Deliberately "what is on the map" rather than "everything fetched": `self.overlays` is
    /// already the filtered, toggled set `rebuild_overlays` assembled for display, so an export
    /// matches what the user is looking at instead of quietly carrying layers they had turned off.
    fn export_map_geojson(&mut self) {
        let features = crate::gis_export::to_features(&crate::gis_export::MapContents {
            strokes: &self.strokes,
            markers: &self.settings.markers,
            zones: &self.settings.alert_polygons,
            cells: self.active_storm_cells(),
            overlays: &self.overlays,
        });
        let count = features.len();
        if count == 0 {
            self.toast(
                ToastKind::Error,
                "Nothing on the map to export \u{2014} draw, mark or turn on a layer first"
                    .to_string(),
            );
            return;
        }
        let json = wxdata::gis::to_geojson(&features);
        match crate::dialog::save_bytes("hookecho-map.geojson", "geojson", json.as_bytes()) {
            crate::dialog::Saved::Where(w) => self.toast(
                ToastKind::Success,
                format!("Exported {count} shapes to {w}"),
            ),
            crate::dialog::Saved::Failed(e) => {
                log::warn!("GeoJSON export failed: {e}");
                self.toast(ToastKind::Error, format!("GeoJSON export failed: {e}"));
            }
            crate::dialog::Saved::Cancelled => {}
        }
    }

    /// Import a settings bundle (rfd open dialog). The next-frame dirty-diff reloads palettes
    /// and persists, and the UI (theme, layers, markers…) updates live from the new settings.
    fn import_settings_bundle(&mut self) {
        crate::dialog::request_open(crate::dialog::ImportKind::SettingsBundle, "");
    }

    /// Route a picked file to whatever asked for it.
    fn apply_import(&mut self, import: crate::dialog::Import) {
        use crate::dialog::ImportKind as K;
        match import.kind {
            // Routed to `open_case` before this, which needs the egui context.
            K::Case => {}
            K::SettingsBundle => self.apply_settings_bundle(&import),
            K::Palette if import.tag == crate::ui::palette_editor::EDITOR_TAG => {
                match import.text() {
                    Ok(text) => self.palette_editor.pending_import = Some(text),
                    Err(e) => self.toast(ToastKind::Error, format!("Palette import failed: {e}")),
                }
            }
            K::Palette => {
                // A platform that handed over content rather than a path (the browser) has no
                // path worth storing — the content goes into the settings and the override names
                // it, which is what `palette_paths` resolves back to a table.
                let value = match &import.bytes {
                    None => import.path.to_string_lossy().into_owned(),
                    Some(_) => match import.text() {
                        Ok(text) => {
                            let name = import.name();
                            self.settings.web_files.insert(name.clone(), text);
                            name
                        }
                        Err(e) => {
                            self.toast(ToastKind::Error, format!("Palette import failed: {e}"));
                            return;
                        }
                    },
                };
                // Setting the override triggers the next-frame dirty-diff palette reload.
                self.settings.palettes.insert(import.tag, value);
            }
            K::ChaseGpx => match import.text() {
                Ok(xml) => {
                    let track = crate::chaselog::from_gpx(&xml);
                    if track.points.len() < 2 {
                        self.toast(ToastKind::Error, "No track points in that file".to_string());
                    } else {
                        let n = track.points.len();
                        self.chase_replay.load(track, import.name());
                        self.toast(ToastKind::Info, format!("Loaded {n} track points"));
                    }
                }
                Err(e) => self.toast(ToastKind::Error, format!("GPX import failed: {e}")),
            },
            K::MarkerIcon => {
                let idx = import.tag.parse::<usize>().ok();
                match (
                    idx.and_then(|i| self.settings.markers.get_mut(i)),
                    crate::ui::marker_window::store_icon(&import.path),
                ) {
                    (Some(m), Some(name)) => m.icon = Some(name),
                    _ => log::warn!("marker icon import went nowhere (marker {})", import.tag),
                }
            }
            K::AlertSound => {
                let file = import.path.to_string_lossy().into_owned();
                let sound = crate::settings::AlertSound::Custom(file);
                match import.tag.as_str() {
                    "New scan" => self.settings.scan_sound = sound,
                    "Warning" => self.settings.warn_sound = sound,
                    "Emergency" => self.settings.emergency_sound = sound,
                    "TDS" => self.settings.tds_sound = sound,
                    "Rotation" => self.settings.rotation_sound = sound,
                    "Lightning" => self.settings.lightning_sound = sound,
                    other => log::warn!("no alert sound row named '{other}'"),
                }
            }
            K::GisFile => match crate::gis_import::load_import(&import) {
                Ok(loaded) => {
                    // Remember it the same two ways an imported `.pal` is remembered: a path
                    // where there is a filesystem, the content itself in a browser, which has
                    // no path that would survive a reload. A boundary file someone works with
                    // daily should not need re-picking on every launch. A browser's shapefile or
                    // KMZ is binary and `web_files` holds text, so it is kept as the GeoJSON it
                    // reads back as — lossless, since export and import share one type.
                    let remembered = match &import.bytes {
                        None => Ok(import.path.to_string_lossy().into_owned()),
                        Some(_) if crate::gis_import::is_binary(&import.name()) => {
                            let stem = import.path.file_stem().map_or_else(
                                || "import".to_string(),
                                |s| s.to_string_lossy().into_owned(),
                            );
                            let name = format!("{stem}.geojson");
                            self.settings
                                .web_files
                                .insert(name.clone(), wxdata::gis::to_geojson(&loaded.features));
                            Ok(name)
                        }
                        Some(_) => import.text().map(|text| {
                            let name = import.name();
                            self.settings.web_files.insert(name.clone(), text);
                            name
                        }),
                    };
                    let (shapes, marks) = crate::gis_import::to_renderable(loaded.features);
                    let n = shapes.len() + marks.len();
                    self.imported_gis = shapes;
                    self.imported_marks = marks;
                    self.imported_colors = None;
                    self.imported_time = None;
                    self.show_imported_gis = true;
                    self.rebuild_overlays();
                    self.settings.imported_gis = remembered.ok();
                    // Framing the import is the difference between "nothing happened" and
                    // "there it is" for a file covering somewhere the map isn't looking.
                    self.zoom_to_imported_gis();
                    let mut message = format!("Imported {n} shapes from {}", import.name());
                    if let Some(note) = &loaded.note {
                        message.push_str(&format!(" — {note}"));
                    }
                    self.toast(
                        if n == 0 {
                            ToastKind::Error
                        } else {
                            ToastKind::Info
                        },
                        message,
                    );
                }
                Err(e) => self.toast(ToastKind::Error, format!("GIS import failed: {e}")),
            },
        }
    }

    /// Apply a settings bundle the user picked.
    fn apply_settings_bundle(&mut self, import: &crate::dialog::Import) {
        match import
            .text()
            .and_then(|s| crate::settings::Settings::import_bundle(&s))
        {
            Ok(settings) => {
                self.settings = settings;
                self.toast(ToastKind::Success, "Settings imported");
            }
            Err(e) => {
                log::warn!("settings import failed: {e}");
                self.toast(ToastKind::Error, format!("Settings import failed: {e}"));
            }
        }
    }

    /// Start a loop export (GIF or MP4): rewind the active timeline and capture every frame.
    fn start_loop_export(&mut self, format: crate::loopexport::LoopFormat) {
        use crate::loopexport::LoopFormat;
        let (name, ext) = match format {
            LoopFormat::Gif => ("hookecho-loop.gif", "gif"),
            LoopFormat::Mp4 => ("hookecho-loop.mp4", "mp4"),
        };
        let Some(path) = crate::dialog::save_path(name, ext) else {
            return;
        };
        let v = &mut self.views[self.active];
        let slots = v.timeline.frames.len(); // observed frames only (skip forecast tail)
        if slots == 0 {
            log::warn!("loop export: no timeline frames");
            self.toast(
                ToastKind::Info,
                "Nothing to export — no frames in the timeline yet",
            );
            return;
        }
        let speed = v.timeline.speed;
        v.timeline.go_begin();
        self.loop_export = Some(LoopExport {
            dest: path,
            format,
            frames: Vec::with_capacity(slots),
            remaining: slots,
            settle: LOOP_SETTLE_FRAMES,
            capturing: false,
            fps: speed,
            volumes: Vec::with_capacity(slots),
            real_timing: self.settings.loop_real_timing,
        });
    }

    /// Advance the loop export: wait for the stepped radar to settle, then request a screenshot.
    fn drive_loop_export(&mut self, ctx: &egui::Context) {
        let Some(le) = &mut self.loop_export else {
            return;
        };
        if le.capturing {
            return; // waiting for the screenshot event
        }
        if le.settle > 0 {
            le.settle -= 1;
            ctx.request_repaint();
            return;
        }
        le.capturing = true;
        self.screenshot_pending = Some(ShotDest::Loop);
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
    }

    /// Record one captured loop frame; step to the next, or finish + encode the GIF.
    fn record_loop_frame(&mut self, image: &egui::ColorImage) {
        let Some(le) = &mut self.loop_export else {
            return;
        };
        let (w, h) = (image.size[0] as u32, image.size[1] as u32);
        let mut buf = Vec::with_capacity((w * h * 4) as usize);
        for px in &image.pixels {
            buf.extend_from_slice(&[px.r(), px.g(), px.b(), px.a()]);
        }
        if let Some(img) = image::RgbaImage::from_raw(w, h, buf) {
            le.frames.push(img);
            let id = self.views[self.active].timeline.current();
            le.volumes
                .push(id.and_then(|id| Some((id.name().to_string(), id.date_time()?))));
        }
        le.capturing = false;
        le.remaining -= 1;
        if le.remaining > 0 {
            self.views[self.active].timeline.step(1);
            if let Some(le) = &mut self.loop_export {
                le.settle = LOOP_SETTLE_FRAMES;
            }
        } else {
            let le = self.loop_export.take().unwrap();
            use crate::loopexport::{LoopFormat, Timing};
            // Real timing needs every frame's scan time; a frame without one (the forecast tail)
            // puts the whole loop on the fixed rate rather than guessing.
            let volumes: Option<Vec<(String, DateTime<Utc>)>> =
                le.volumes.iter().cloned().collect();
            let fps = le.fps.clamp(1.0, 15.0);
            let timing = match (&volumes, le.real_timing) {
                (Some(_), true) => Timing::Real { fps },
                _ => Timing::Fixed { fps },
            };
            let frames = volumes
                .as_deref()
                .map(|v| crate::loopexport::frame_list(v, timing));
            let delays: Vec<u32> = match &frames {
                Some(f) => f.iter().map(|f| f.delay_ms).collect(),
                None => vec![(1000.0 / fps) as u32; le.frames.len()],
            };
            let res = match le.format {
                #[cfg(not(target_arch = "wasm32"))]
                LoopFormat::Gif => crate::loopexport::encode_gif_timed(
                    le.frames.iter().cloned().map(Ok),
                    &delays,
                    &le.dest,
                ),
                // Unreachable on the web: an export needs a destination path and there is none in
                // a browser, so `start_loop_export` returns before a capture ever begins.
                #[cfg(target_arch = "wasm32")]
                LoopFormat::Gif => Err(anyhow::anyhow!("GIF export needs a filesystem")),
                LoopFormat::Mp4 => {
                    crate::loopexport::encode_mp4_timed(&le.frames, &delays, &le.dest)
                }
            };
            // The sidecar: which scans the loop shows and how long each is held.
            if res.is_ok() {
                let v = &self.views[self.active];
                let (interval, fps) = crate::loopexport::timing_words(timing);
                let meta = serde_json::json!({
                    "site": v.site,
                    "product": v.moment.short_name(),
                    "tilt_index": v.tilt,
                    "format": match le.format { LoopFormat::Gif => "gif", LoopFormat::Mp4 => "mp4" },
                    "frame_px": le.frames.first().map(|f| [f.width(), f.height()]),
                    "interval": interval,
                    "fps": fps,
                    "duration_ms": delays.iter().map(|d| u64::from(*d)).sum::<u64>(),
                    "frames": frames,
                    "source": "NOAA NEXRAD Level II, rendered by HookEcho",
                });
                if let Ok(text) = serde_json::to_string_pretty(&meta) {
                    let _ = std::fs::write(le.dest.with_extension("json"), text);
                }
            }
            match res {
                Ok(()) => {
                    log::info!(
                        "loop saved: {} ({} frames)",
                        le.dest.display(),
                        le.frames.len()
                    );
                    let msg = format!("Loop saved ({} frames)", le.frames.len());
                    self.toast(ToastKind::Success, msg);
                }
                Err(e) => {
                    log::warn!("loop encode failed: {e}");
                    self.toast(ToastKind::Error, format!("Loop export failed: {e}"));
                }
            }
        }
    }

    /// Write the widget's picture, downscaled, and tell the widget about it.
    ///
    /// Downscaled because `RemoteViews.setImageViewBitmap` sends the bitmap over a Binder
    /// transaction with a ~1 MB ceiling, and the decoded bitmap is 4 bytes a pixel — so the budget
    /// is in *pixels*, not width. 150k of them is ~600 KB decoded, and more than a home-screen
    /// tile can show anyway. (A full-screen 1440x3120 grab is 18 MB decoded: the widget would
    /// throw rather than draw.)
    fn save_widget_snapshot(&self, path: &std::path::Path, image: &egui::ColorImage) {
        const MAX_PIXELS: f32 = 150_000.0;
        let (w, h) = (image.size[0] as u32, image.size[1] as u32);
        if w == 0 || h == 0 {
            return;
        }
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for px in &image.pixels {
            rgba.extend_from_slice(&[px.r(), px.g(), px.b(), px.a()]);
        }
        let Some(buf) = image::RgbaImage::from_raw(w, h, rgba) else {
            return;
        };
        let scale = (MAX_PIXELS / (w as f32 * h as f32)).sqrt();
        let small = if scale < 1.0 {
            let (nw, nh) = (
                ((w as f32 * scale).round() as u32).max(1),
                ((h as f32 * scale).round() as u32).max(1),
            );
            image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Triangle)
        } else {
            buf
        };
        // Silent: nobody asked for this one, so a toast would be the app talking to itself. A
        // failed write costs the widget one stale caption.
        match small.save(path) {
            Ok(()) => {
                self.save_widget_caption();
                crate::platform::refresh_radar_widget();
            }
            Err(e) => log::warn!("widget snapshot failed: {e}"),
        }
    }

    /// Write the widget's storm line beside the picture, measured from the place the user cares
    /// about: where they are while chasing, else their home marker. Neither one means no line —
    /// a distance from an arbitrary map centre is a number that misleads.
    ///
    /// Deleted rather than left stale when there is nothing to say: the file is read on its own
    /// clock by a widget that has no idea how old it is.
    fn save_widget_caption(&self) {
        let Some(path) = crate::paths::widget_caption() else {
            return;
        };
        let here = self.chase_pos.or_else(|| {
            self.settings
                .markers
                .iter()
                .find(|m| m.home)
                .map(|m| (m.lon, m.lat))
        });
        let line = here.and_then(|(lon, lat)| {
            widget_storm_line(self.active_storm_cells(), lon, lat, self.metric())
        });
        match line {
            Some(line) => {
                if let Err(e) = std::fs::write(&path, line) {
                    log::warn!("widget caption failed: {e}");
                }
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    /// Keep the Android home-screen widget's picture fresh while the app is on screen.
    ///
    /// Every few minutes, not every scan: the widget's own clock cannot beat 30 minutes anyway,
    /// and a full-resolution PNG per volume is a write nobody asked for. Nothing runs off Android.
    fn drive_widget_snapshot(&mut self, ctx: &egui::Context) {
        let every = std::time::Duration::from_secs(if self.settings.battery_saver {
            900
        } else {
            300
        });
        if !cfg!(target_os = "android")
            || self.screenshot_pending.is_some()
            || self.share_card.is_some()
            || self.views[self.active].volume.is_none()
        {
            return;
        }
        if self.widget_shot_at.is_some_and(|t| t.elapsed() < every) {
            return;
        }
        let Some(path) = crate::paths::widget_snapshot() else {
            return;
        };
        self.widget_shot_at = Some(Instant::now());
        self.screenshot_pending = Some(ShotDest::Widget(path));
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
    }

    /// A warning asked for a radar picture: take one, as long as nothing else is mid-capture.
    ///
    /// Deliberately the live view rather than a headless render — it is the picture the user is
    /// looking at, product, overlays, zoom and all, and it costs one frame instead of a second
    /// render path. Skipped on Android, where the alert path runs in a service with no surface.
    fn drive_snapshot_push(&mut self, ctx: &egui::Context) {
        if self.snapshot_push.is_none()
            || self.screenshot_pending.is_some()
            || self.share_card.is_some()
            || cfg!(target_os = "android")
        {
            return;
        }
        let title = self.snapshot_push.take().expect("checked above");
        self.request_capture(ctx, ShotDest::Push(title));
    }

    /// Ask the viewport for an image, `dest` decides where it lands.
    ///
    /// With the share card on, the request waits two frames while [`share_card_footer`] draws the
    /// caption band, so the capture contains it.
    ///
    /// ponytail: the caption is drawn by egui into the frame rather than composited into the
    /// pixels afterwards — text layout, fonts and the theme are already solved here, and
    /// compositing them onto a raw RGBA buffer is a rasterizer we would have to grow.
    fn request_capture(&mut self, ctx: &egui::Context, dest: ShotDest) {
        if self.settings.share_card {
            // Two frames: one to lay the band out, one to be sure it is on screen when the
            // viewport grabs the image.
            self.share_card = Some((dest, 2));
            ctx.request_repaint();
        } else {
            self.screenshot_pending = Some(dest);
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }
}

/// Convert a binned sweep into a GPU upload with its world-space bounding box.
///
/// `threshold` (physical units) is baked into the color LUT; `None` shows all values.
/// `smooth` enables bilinear sampling in the shader. `table` selects the colormap.
/// `storm_uv` is the storm motion (east, north) in m/s for storm-relative velocity, or
/// `None` for ground-relative. `precip` is the MRMS surface precipitation-type field, when the
/// user has asked for reflectivity to be tinted by it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn to_upload(
    s: &BinnedSweep,
    table: &ColorTable,
    threshold: Option<f32>,
    smooth: bool,
    storm_uv: Option<(f32, f32)>,
    precip: Option<&PrecipGrid>,
    // Only the color table changed, so the sweep and precipitation-flag bytes the GPU already
    // holds are still correct and are not copied.
    lut_only: bool,
    telemetry: Option<(Instant, Arc<crate::render::LiveQueueTimings>)>,
) -> RadarUpload {
    use crate::render::mercator::lonlat_to_world;
    let max_range_km = s.first_gate_km + s.gate_count as f32 * s.gate_interval_km;
    let dlat = (max_range_km / 111.32) as f64;
    let coslat = (s.radar_lat as f64 * std::f64::consts::PI / 180.0)
        .cos()
        .max(0.01);
    let dlon = (max_range_km as f64 / 111.32) / coslat;
    let (lat, lon) = (s.radar_lat as f64, s.radar_lon as f64);
    let (wx0, wy0) = lonlat_to_world(lon - dlon, lat + dlat);
    let (wx1, wy1) = lonlat_to_world(lon + dlon, lat - dlat);
    // Premultiply storm motion into raw-index units (raw = 2 + t*253, t over value_span).
    let per_ms = 253.0 / (s.value_max - s.value_min).max(f32::EPSILON);
    let (srv, me, mn) = match storm_uv {
        Some((e, n)) => (1.0, e * per_ms, n * per_ms),
        None => (0.0, 0.0, 0.0),
    };
    // Only reflectivity is tinted: the tint says what kind of precipitation an echo is, and
    // that is not a statement about a velocity or a correlation coefficient.
    let tint = precip.filter(|_| s.moment == Moment::Reflectivity);
    let base = crate::colormap::bake_lut(table, (s.value_min, s.value_max), threshold);
    // Three rows, always. When the tint is off they are identical, which costs 2 KB and keeps
    // the shader and the bind group the same shape in both cases.
    let mut lut = Vec::with_capacity(1024 * 3);
    lut.extend_from_slice(&base);
    for kind in [
        crate::colormap::PrecipTint::Snow,
        crate::colormap::PrecipTint::Mix,
    ] {
        match tint {
            Some(_) => lut.extend_from_slice(&crate::colormap::tint_lut(&base, kind)),
            None => lut.extend_from_slice(&base),
        }
    }
    let (precip_flag, flag_nx, flag_ny, flag_w, flag_n, flag_e, flag_s) = match tint {
        Some(g) => (
            if lut_only {
                Vec::new()
            } else {
                g.classes.clone()
            },
            g.nx,
            g.ny,
            g.west,
            g.north,
            g.east,
            g.south,
        ),
        None => (Vec::new(), 0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
    };

    // Generation mask (suggestions.md §21). A live volume assembled from chunks keeps the
    // previous rotation in the wedge the current one has not reached; dim it so retained data
    // are visibly not the same thing as just-scanned data. `STALE_DIM` is a tint, not a hide:
    // the old sweep is still the best available answer for that wedge.
    const STALE_DIM: f32 = 0.45;
    let (stale_start, stale_end, stale_dim) = match s.stale_arc_deg {
        Some((a, b)) => (a, b, STALE_DIM),
        None => (0.0, 0.0, 0.0),
    };

    RadarUpload {
        az_bins: s.az_bins as u32,
        gate_count: s.gate_count as u32,
        data: if lut_only { Vec::new() } else { s.data.clone() },
        uniform: [
            s.radar_lat,
            s.radar_lon,
            s.first_gate_km,
            s.gate_interval_km,
            s.az_bins as f32,
            s.gate_count as f32,
            if smooth { 1.0 } else { 0.0 },
            srv,
            me,
            mn,
            if tint.is_some() { 1.0 } else { 0.0 },
            flag_nx,
            flag_ny,
            flag_w,
            flag_n,
            flag_e,
            flag_s,
            stale_start,
            stale_end,
            stale_dim,
        ],
        lut,
        precip_flag,
        world_min: [wx0 as f32, wy0 as f32],
        world_max: [wx1 as f32, wy1 as f32],
        lut_only,
        telemetry,
    }
}

/// One frame of the data-driven 2D live-sweep indicator. The beam exists only briefly after a
/// real chunk arrives; it is not a decorative clock that can imply fresh data while a feed is
/// stalled.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LiveSweepFrame {
    angle_deg: f32,
    alpha: f32,
    remaining_secs: f32,
}

/// Animate through the azimuth sector represented by one Level II chunk. Standard-resolution
/// sweeps arrive in three chunks and super-resolution sweeps in six, so the chunk metadata gives
/// an honest sector even though the progress callback deliberately carries no radar payload.
fn live_sweep_frame(
    progress: wxdata::live::ScanProgress,
    elapsed_secs: f32,
    reduced_motion: bool,
) -> Option<LiveSweepFrame> {
    if progress.chunks_in_sweep == 0
        || progress.chunk_index == 0
        || progress.chunk_index > progress.chunks_in_sweep
        || progress.azimuth_span_deg() <= 0.0
        || !elapsed_secs.is_finite()
        || elapsed_secs < 0.0
    {
        return None;
    }
    const FADE_SECS: f32 = 0.35;
    let sector = progress.azimuth_span_deg();
    let start = progress.azimuth_start_deg as f32;
    let end = progress.azimuth_end_deg as f32;
    let sweep_secs = progress.chunk_duration_secs().clamp(0.35, 15.0);
    let total_secs = sweep_secs + FADE_SECS;
    if elapsed_secs > total_secs {
        return None;
    }
    let angle_deg = if reduced_motion {
        end.rem_euclid(360.0)
    } else {
        (start + sector * (elapsed_secs / sweep_secs).clamp(0.0, 1.0)).rem_euclid(360.0)
    };
    let alpha = if elapsed_secs <= sweep_secs {
        1.0
    } else {
        (1.0 - (elapsed_secs - sweep_secs) / FADE_SECS).clamp(0.0, 1.0)
    };
    Some(LiveSweepFrame {
        angle_deg,
        alpha,
        remaining_secs: (total_secs - elapsed_secs).max(0.0),
    })
}

fn live_progress_matches_tilt(progress: wxdata::live::ScanProgress, elevation_deg: f32) -> bool {
    (progress.elevation_angle_deg as f32 - elevation_deg).abs() < 0.15
}

/// Paint a WSV3-style live refresh line: bright lime with a dark keyline and a short angular
/// tail, projected from the real radar site through the map camera. `painter` clips it to this
/// pane.
fn paint_live_sweep(
    painter: &egui::Painter,
    rect: egui::Rect,
    camera: crate::render::mercator::Camera,
    vp: (f32, f32),
    radar: [f64; 2],
    frame: LiveSweepFrame,
) {
    let to_screen = |lon: f64, lat: f64| {
        let world = crate::render::mercator::lonlat_to_world(lon, lat);
        let (x, y) = camera.world_to_screen(world, vp);
        egui::pos2(rect.left() + x, rect.top() + y)
    };
    let center = to_screen(radar[0], radar[1]);
    let ray = |angle_deg: f32| {
        let sample = crate::geo::destination_point(radar, angle_deg as f64, 100.0);
        let toward = to_screen(sample[0], sample[1]) - center;
        let len = toward.length();
        (len > 0.01).then(|| {
            let direction = toward / len;
            let reach = [
                rect.left_top(),
                rect.right_top(),
                rect.left_bottom(),
                rect.right_bottom(),
            ]
            .into_iter()
            .map(|corner| corner.distance(center))
            .fold(0.0_f32, f32::max)
                + 8.0;
            [center, center + direction * reach]
        })
    };

    let green = egui::Color32::from_rgb(82, 255, 116);
    // A sparse tail makes direction legible without covering the newly-arrived radar pixels.
    for (back, opacity) in [(4.0_f32, 0.16_f32), (2.0, 0.32)] {
        if let Some(segment) = ray(frame.angle_deg - back) {
            painter.line_segment(
                segment,
                egui::Stroke::new(1.5, green.gamma_multiply(frame.alpha * opacity)),
            );
        }
    }
    if let Some(segment) = ray(frame.angle_deg) {
        painter.line_segment(
            segment,
            egui::Stroke::new(5.0, egui::Color32::BLACK.gamma_multiply(frame.alpha * 0.72)),
        );
        painter.line_segment(
            segment,
            egui::Stroke::new(2.25, green.gamma_multiply(frame.alpha)),
        );
    }
}

/// Include inertial scrolling and trackpad pinches, which do not hold a pointer down.
fn map_gesture_live(i: &egui::InputState) -> bool {
    i.pointer.any_down()
        || i.any_touches()
        || i.smooth_scroll_delta != egui::Vec2::ZERO
        || (i.zoom_delta() - 1.0).abs() > f32::EPSILON
}

/// A zoom-bucket crossing waits for the gesture to end; new geometry does not.
fn should_retess(gesture_live: bool, geometry_changed: bool, bucket_changed: bool) -> bool {
    geometry_changed || (bucket_changed && !gesture_live)
}

/// Every radar site with its world-space position, projected once.
///
/// The table is static and the projection is a `ln(tan(...))` per site; ~350 of them ran every
/// frame, per pane, for a set of points that cannot move.
fn sites_in_world() -> &'static [(&'static wxdata::sites::SiteEntry, (f64, f64))] {
    static SITES: std::sync::OnceLock<Vec<(&'static wxdata::sites::SiteEntry, (f64, f64))>> =
        std::sync::OnceLock::new();
    SITES.get_or_init(|| {
        wxdata::sites::all()
            .map(|s| {
                (
                    s,
                    crate::render::mercator::lonlat_to_world(s.longitude as f64, s.latitude as f64),
                )
            })
            .collect()
    })
}

/// MRMS surface precipitation classes on their own lat/lon grid, ready for the GPU.
pub(crate) struct PrecipGrid {
    /// One byte per cell: 0 rain, 1 snow, 2 mix.
    pub classes: Vec<u8>,
    pub nx: f32,
    pub ny: f32,
    pub west: f32,
    pub north: f32,
    pub east: f32,
    pub south: f32,
}

impl PrecipGrid {
    fn new(f: &wxdata::mrms::MrmsField) -> Self {
        Self {
            classes: f.values.iter().map(|v| precip_class(*v)).collect(),
            nx: f.nx as f32,
            ny: f.ny as f32,
            west: f.lon_west as f32,
            north: f.lat_north as f32,
            east: f.lon_east as f32,
            south: f.lat_south as f32,
        }
    }
}

/// MRMS `PrecipFlag` categories collapsed to the three the tint distinguishes.
///
/// The product carries more classes than that — several flavours of rain, hail, tropical rain —
/// but colouring reflectivity is a coarse statement and only three destinations exist. Hail and
/// convective rain stay on the rain ramp deliberately: they are the cases where the existing
/// reflectivity colours already carry the meaning.
fn precip_class(flag: f32) -> u8 {
    match flag as i32 {
        // 3 snow, 4 wet snow.
        3 | 4 => 1,
        // 6 freezing rain, 7 ice pellets/sleet.
        6 | 7 => 2,
        _ => 0,
    }
}

/// Reflectivity at a lon/lat off a binned sweep, by inverting the polar bin geometry. `None`
/// outside the sweep's gates; below-threshold bins read as -inf (in coverage, but no echo).
fn refl_sampler(sweep: &wxdata::level2::BinnedSweep) -> impl Fn(f64, f64) -> Option<f32> + '_ {
    let span = (sweep.value_max - sweep.value_min).max(1e-3);
    let radar = [sweep.radar_lon as f64, sweep.radar_lat as f64];
    move |lon: f64, lat: f64| {
        let (range_km, bearing) = crate::geo::great_circle(radar, [lon, lat]);
        let gate = ((range_km as f32 - sweep.first_gate_km) / sweep.gate_interval_km).round();
        if gate < 0.0 || gate as usize >= sweep.gate_count {
            return None;
        }
        let az = (bearing / 360.0 * sweep.az_bins as f64).round() as usize % sweep.az_bins;
        let v = sweep.data[az * sweep.gate_count + gate as usize];
        if v < 2 {
            return Some(f32::NEG_INFINITY);
        }
        Some(sweep.value_min + (v as f32 - 2.0) / 253.0 * span)
    }
}

/// Convert an MRMS reflectivity field into a GPU upload: dBZ → 2..=255 index band
/// (no-data/NaN → 0 = transparent), the reflectivity color LUT, and the grid's
/// mercator world-space quad (plate-carrée corners projected).
fn mrms_upload(f: &wxdata::mrms::MrmsField, table: &ColorTable) -> crate::render::MrmsUpload {
    use crate::render::mercator::lonlat_to_world;
    let (vmin, vmax) = Moment::Reflectivity.value_range();
    let span = (vmax - vmin).max(f32::EPSILON);
    let data: Vec<u8> = f
        .values
        .iter()
        .map(|&v| {
            if v.is_nan() {
                0
            } else {
                let t = ((v - vmin) / span).clamp(0.0, 1.0);
                (2.0 + t * 253.0) as u8
            }
        })
        .collect();
    let (wx0, wy0) = lonlat_to_world(f.lon_west, f.lat_north);
    let (wx1, wy1) = lonlat_to_world(f.lon_east, f.lat_south);
    crate::render::MrmsUpload {
        data,
        nx: f.nx as u32,
        ny: f.ny as u32,
        world_min: [wx0 as f32, wy0 as f32],
        world_max: [wx1 as f32, wy1 as f32],
        uniform: [
            f.lon_west as f32,
            f.lat_north as f32,
            f.lon_east as f32,
            f.lat_south as f32,
            f.nx as f32,
            f.ny as f32,
            1.0, // opacity; rewritten per frame from settings.field_opacity
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ],
        lut: crate::colormap::bake_lut(table, (vmin, vmax), None).to_vec(),
    }
}

/// Build a field-layer GPU upload from a grid: `map` turns each cell value into a LUT index
/// (0 = transparent, 2..=255 = data), `lut` is the 256-entry RGBA color table.
pub(crate) fn field_index_upload(
    f: &wxdata::mrms::MrmsField,
    map: impl Fn(f32) -> u8,
    lut: Vec<u8>,
) -> crate::render::MrmsUpload {
    use crate::render::mercator::lonlat_to_world;
    let data: Vec<u8> = f
        .values
        .iter()
        .map(|&v| if v.is_nan() { 0 } else { map(v) })
        .collect();
    let (wx0, wy0) = lonlat_to_world(f.lon_west, f.lat_north);
    let (wx1, wy1) = lonlat_to_world(f.lon_east, f.lat_south);
    crate::render::MrmsUpload {
        data,
        nx: f.nx as u32,
        ny: f.ny as u32,
        world_min: [wx0 as f32, wy0 as f32],
        world_max: [wx1 as f32, wy1 as f32],
        uniform: [
            f.lon_west as f32,
            f.lat_north as f32,
            f.lon_east as f32,
            f.lat_south as f32,
            f.nx as f32,
            f.ny as f32,
            1.0, // opacity; rewritten per frame from settings.field_opacity
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ],
        lut,
    }
}

/// The lat/lon grid a GOES granule is decoded to. CONUS spans about 60 by 25 degrees; a
/// mesoscale box about 10 by 10, square, and scanned at about 500 by 500 pixels in the 2 km bands.
/// The decode scatters each source pixel to its nearest cell, so a grid finer than the source
/// leaves holes (stripes of no data): the box's grid stays just under the source.
fn goes_grid(sector: wxdata::goes_abi::Sector) -> (usize, usize) {
    if sector.is_meso() {
        (480, 480)
    } else {
        (1200, 700)
    }
}

/// How many minutes of GLM flashes a scrubbed-back view shows: the dots' fade is fifteen, but
/// the last ten carry all but the faintest, at a third fewer granules to read.
const GLM_ARCHIVE_MINUTES: i64 = 10;

/// The GLM flashes a view at `target` shows, and the time their age is told against: live, the
/// feed and now; scrubbed back, the archive window when it is the one for this minute (else
/// nothing, rather than today's flashes over an old scan).
fn glm_flashes_for<'a>(
    target: Option<DateTime<Utc>>,
    live: &'a std::collections::VecDeque<wxdata::glm::Flash>,
    archive: Option<&'a (DateTime<Utc>, Vec<wxdata::glm::Flash>)>,
    now: DateTime<Utc>,
) -> (Vec<&'a wxdata::glm::Flash>, DateTime<Utc>) {
    match target {
        None => (live.iter().collect(), now),
        Some(t) => match archive {
            Some((end, flashes)) if glm_slot(*end) == glm_slot(t) => {
                (flashes.iter().filter(|f| f.time <= t).collect(), t)
            }
            _ => (Vec::new(), t),
        },
    }
}

/// The minute a GLM archive window belongs to.
fn glm_slot(t: DateTime<Utc>) -> i64 {
    t.timestamp().div_euclid(60)
}

/// Exposure along the chosen route and what it was computed for: (route generation, overlay
/// generation, progress in 100 m steps) and `(alert kind, km ahead, seconds ahead)` lines.
type RouteExposure = (
    (u64, u64, i64, usize, i64),
    Vec<(String, f64, f64, bool)>,
    Vec<String>,
);

/// Eight-point compass name for a bearing.
fn compass8(deg: f64) -> &'static str {
    const N: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    N[(((deg.rem_euclid(360.0) + 22.5) / 45.0) as usize) % 8]
}

/// The imported features' valid windows and the start/end attributes they were read with.
type ImportedTime = (
    (Option<String>, Option<String>),
    crate::gis_import::TimeBounds,
);

/// The HRRR isotherm-height surfaces: `(label, colour, heights in km MSL)` per level, and the run.
type ModelIsotherms = (
    Vec<(&'static str, [u8; 3], wxdata::mrms::MrmsField)>,
    DateTime<Utc>,
);

/// The model isotherms drawn in 3D: HRRR level name, label, colour.
const MODEL_ISOTHERMS: [(&str, &str, [u8; 3]); 3] = [
    ("0C isotherm", "HRRR 0 °C", [60, 210, 190]),
    ("263 K level", "HRRR -10 °C", [245, 185, 70]),
    ("253 K level", "HRRR -20 °C", [230, 90, 220]),
];

/// The GOES layers read from one scan of one sector: the bands and the RGB composite.
const GOES_FRAME_LAYERS: [crate::render::FieldLayer; 10] = [
    crate::render::FieldLayer::GoesIr,
    crate::render::FieldLayer::GoesVisible,
    crate::render::FieldLayer::GoesWaterVapor,
    crate::render::FieldLayer::GoesShortwaveIr,
    crate::render::FieldLayer::GoesMidWaterVapor,
    crate::render::FieldLayer::GoesLowWaterVapor,
    crate::render::FieldLayer::GoesDirtyIr,
    crate::render::FieldLayer::GoesLongwaveIr,
    crate::render::FieldLayer::GoesColdTop,
    crate::render::FieldLayer::GoesRgb,
];

fn is_goes_frame_layer(layer: crate::render::FieldLayer) -> bool {
    GOES_FRAME_LAYERS.contains(&layer)
}

/// The scan slot a GOES request for `at` falls in: `None` for the newest, else the sector's
/// cadence-long slot holding `at`, so scrubbing within one scan does not refetch but stepping to
/// the next does.
pub(crate) fn goes_slot(
    at: Option<DateTime<Utc>>,
    sector: wxdata::goes_abi::Sector,
) -> Option<i64> {
    at.map(|t| t.timestamp().div_euclid(sector.cadence_secs() as i64))
}

/// Whether a GOES layer's grid can be painted for a view at `target` (`None`: live): only one
/// fetched for the slot now wanted, and, scrubbed back, a frame within `tolerance` of the target.
/// So an old live frame never sits over an archive scan, nor an archive frame over live radar.
pub(crate) fn goes_frame_ready(
    fetched_slot: Option<Option<i64>>,
    wanted_slot: Option<i64>,
    frame_time: Option<DateTime<Utc>>,
    target: Option<DateTime<Utc>>,
    tolerance: chrono::Duration,
) -> bool {
    if fetched_slot != Some(wanted_slot) {
        return false;
    }
    match (target, frame_time) {
        (None, _) => true,
        (Some(t), Some(f)) => (f - t).abs() <= tolerance,
        (Some(_), None) => false,
    }
}

/// Which ABI sector to read for a view centred at `(lon, lat)`: the chosen one, unless it is a
/// mesoscale box known to be pointed somewhere that does not cover the view, in which case CONUS
/// (ROADMAP_NEW E5's graceful switch). A box not yet seen is tried: its first granule says where
/// it is.
pub(crate) fn goes_sector_for(
    chosen: wxdata::goes_abi::Sector,
    footprint: Option<(wxdata::goes_abi::Sector, wxdata::goes_abi::Footprint)>,
    (lon, lat): (f64, f64),
) -> wxdata::goes_abi::Sector {
    use wxdata::goes_abi::Sector;
    if !chosen.is_meso() {
        return chosen;
    }
    match footprint {
        Some((sector, fp)) if sector == chosen && !fp.contains(lon, lat) => Sector::Conus,
        _ => chosen,
    }
}

/// An RGB composite's upload: its packed colours (`wxdata::goes_rgb::pack`) reduced to an
/// adaptive palette of 254 colours, which the field pipeline draws like any indexed layer. Index 0
/// is no data and 1 is left unused, as the pipeline reserves them.
pub(crate) fn rgb_upload(f: &wxdata::mrms::MrmsField) -> crate::render::MrmsUpload {
    let q = wxdata::goes_rgb::quantize(&f.values, 254);
    let mut lut = vec![0u8; 256 * 4];
    for (i, c) in q.palette.iter().enumerate() {
        let at = (i + 2) * 4;
        lut[at..at + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
    }
    let mut upload = field_index_upload(f, |_| 0, lut);
    upload.data = q.index.iter().map(|i| i.map_or(0, |i| i + 2)).collect();
    upload
}

/// Recolor the retained signed comparison grid for the selected display mode. All modes share
/// one fetched/scientifically-derived field; only the value-to-index mapping and LUT differ.
fn model_diff_upload(
    field: &wxdata::mrms::MrmsField,
    kind: crate::fielddiff::DiffField,
    mode: crate::fielddiff::DiffMode,
) -> crate::render::MrmsUpload {
    // A percent grid is already a ratio: its own ±100 % scale, no unit conversion.
    let ((range, deadband), scale) = if mode == crate::fielddiff::DiffMode::Percent {
        (crate::fielddiff::PERCENT_RANGE, 1.0)
    } else {
        (kind.range(), kind.input_scale())
    };
    field_index_upload(
        field,
        |value| crate::fielddiff::display_index(mode, value * scale, range, deadband),
        crate::fielddiff::display_lut(mode, range, deadband),
    )
}

/// Interpolate a 256-entry RGBA LUT from `(t, [r,g,b])` stops; index 0 is always transparent.
/// One `min..max` pair of sliders for an axis of the map-embedded 3D volume's slab, mirroring the
/// standalone "3D Reflectivity" window's `axis_slice`, which lives in a different module (a
/// per-pane `egui::Area`, not that window's own `egui::Window`) and so isn't reused directly.
fn map_3d_axis_slice(ui: &mut egui::Ui, label: &str, lo: &mut f32, hi: &mut f32) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::Slider::new(lo, 0.0..=1.0).show_value(false));
        ui.add(egui::Slider::new(hi, 0.0..=1.0).show_value(false));
    });
    // Keep the pair ordered so an inverted drag empties the view instead of inverting the slab.
    if *lo > *hi {
        std::mem::swap(lo, hi);
    }
}

/// Pack `radar_observed.wgsl`'s `Radar3d` uniform: eleven fixed scalars, the four CC-anomaly
/// slots, the highlighted-elevation slots, and one trailing pad float. Some downlevel backends
/// (mobile GLES via ANGLE) reject a uniform binding whose declared type isn't a multiple of 16
/// bytes, and 23 scalar f32s is 92 — hence the pad.
///
/// A free function rather than inline packing so a GPU test can drive exactly the layout the app
/// ships. Hand-writing the same offsets in a test would only prove the test agrees with itself.
pub(crate) fn observed_uniform(
    fixed: [f32; 11],
    cc: [f32; 4],
    highlight_elevs: [f32; MAX_HIGHLIGHTED_LAYERS],
) -> crate::render::ObservedUniform {
    let mut uniform: crate::render::ObservedUniform = [0.0; _];
    uniform[..11].copy_from_slice(&fixed);
    uniform[11..15].copy_from_slice(&cc);
    uniform[15..15 + MAX_HIGHLIGHTED_LAYERS].copy_from_slice(&highlight_elevs);
    uniform
}

/// The CC-anomaly row, shared by the Observed (correlation coefficient) and Debris 3D modes so
/// the two cannot drift into describing the same thing differently.
///
/// The thresholds are all user-set on purpose. The defaults are a reasonable starting point for
/// a warm-season CONUS debris hunt and nothing more — CC backgrounds move with the radar, the
/// beam's distance, the precipitation type and the season, so anything presented here as a fixed
/// meteorological constant would be wrong somewhere.
fn map_3d_cc_anomaly_controls(ui: &mut egui::Ui, a: &mut crate::view::CcAnomaly) {
    ui.horizontal(|ui| {
        ui.checkbox(&mut a.enabled, "CC anomaly").on_hover_text(
            "Fade ordinary high-CC precipitation toward transparent and make low CC \
             progressively more solid, so lofted debris, clutter and other non-meteorological \
             returns stand out of the storm around them. The opposite of a denoise floor, which \
             for CC would hide exactly those.",
        );
        if !a.enabled {
            return;
        }
        if ui
            .small_button("Reset")
            .on_hover_text("Back to 0.97 / 0.80")
            .clicked()
        {
            *a = crate::view::CcAnomaly::default();
        }
    });
    if !a.enabled {
        return;
    }
    ui.add(
        egui::Slider::new(&mut a.clear_cc, 0.80..=1.0)
            .text("Clear above")
            .custom_formatter(|v, _| format!("{v:.3}")),
    )
    .on_hover_text("CC at or above this draws faintest — the background scatter edge");
    ui.add(
        egui::Slider::new(&mut a.opaque_cc, 0.0..=0.99)
            .text("Solid below")
            .custom_formatter(|v, _| format!("{v:.3}")),
    )
    .on_hover_text("CC at or below this draws at full strength");
    ui.add(
        egui::Slider::new(&mut a.faintest, 0.0..=0.5)
            .text("Faintest")
            .custom_formatter(|v, _| format!("{v:.2}")),
    )
    .on_hover_text(
        "How visible the background stays. Zero removes it entirely; a little left keeps the \
         storm as context around the anomaly.",
    );
    // Ordering here rather than clamping each slider against the other: mutually-constrained
    // sliders feel stuck, and `cc_anomaly_uniform` already orders the pair before it builds the
    // ramp, so a crossed pair is only ever a display question.
    if a.opaque_cc > a.clear_cc {
        std::mem::swap(&mut a.opaque_cc, &mut a.clear_cc);
    }
}

pub(crate) fn ramp_lut(stops: &[(f32, [u8; 3])]) -> Vec<u8> {
    ramp_lut_a(stops, 255)
}

/// Like [`ramp_lut`] but with a caller-chosen opacity for non-zero indices (index 0 stays clear).
/// Environment overlays (CAPE/SRH) use a translucent alpha so the basemap reads through.
pub(crate) fn ramp_lut_a(stops: &[(f32, [u8; 3])], alpha: u8) -> Vec<u8> {
    let mut lut = vec![0u8; 256 * 4];
    for i in 0..256 {
        let t = i as f32 / 255.0;
        let mut rgb = stops[0].1;
        for w in stops.windows(2) {
            let (t0, c0) = w[0];
            let (t1, c1) = w[1];
            if t >= t0 && t <= t1 {
                let k = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
                rgb = [
                    (c0[0] as f32 + (c1[0] as f32 - c0[0] as f32) * k) as u8,
                    (c0[1] as f32 + (c1[1] as f32 - c0[1] as f32) * k) as u8,
                    (c0[2] as f32 + (c1[2] as f32 - c0[2] as f32) * k) as u8,
                ];
                break;
            }
        }
        let a = if i == 0 { 0 } else { alpha };
        lut[i * 4..i * 4 + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], a]);
    }
    lut
}

/// Build a 256-entry categorical LUT: every listed `(index, rgb)` gets `alpha`, all others clear.
/// Used for the MRMS precipitation-type flag (discrete categories, not a continuous ramp).
fn categorical_lut(slots: &[(u8, [u8; 3])], alpha: u8) -> Vec<u8> {
    let mut lut = vec![0u8; 256 * 4];
    for &(i, rgb) in slots {
        let o = i as usize * 4;
        lut[o..o + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
    }
    lut
}

/// Map color for a tornado's F/EF magnitude (green→yellow→orange→red→violet; gray = unknown).
fn tornado_mag_color(mag: i8) -> egui::Color32 {
    match mag {
        0 => egui::Color32::from_rgb(120, 200, 120),
        1 => egui::Color32::from_rgb(230, 220, 80),
        2 => egui::Color32::from_rgb(240, 170, 50),
        3 => egui::Color32::from_rgb(235, 90, 60),
        4 => egui::Color32::from_rgb(220, 50, 90),
        5 => egui::Color32::from_rgb(200, 60, 220),
        _ => egui::Color32::from_rgb(150, 150, 160),
    }
}

/// Load the SPC tornado-track database, preferring an on-disk cache; on a cache miss, download the
/// CSV and write it for next time. Parsing is the same either way.
async fn load_or_fetch_climo(
    http: &reqwest::Client,
    cache: Option<std::path::PathBuf>,
) -> anyhow::Result<Vec<wxdata::torclimo::TornadoTrack>> {
    if let Some(path) = &cache {
        if let Ok(csv) = std::fs::read_to_string(path) {
            return Ok(wxdata::torclimo::parse_tracks(&csv));
        }
    }
    // Cache miss: download once, parse, and persist the raw CSV.
    let csv = http
        .get("https://www.spc.noaa.gov/wcm/data/1950-2022_actual_tornadoes.csv")
        .header("User-Agent", wxdata::alerts::USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    if let Some(path) = &cache {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, &csv);
    }
    Ok(wxdata::torclimo::parse_tracks(&csv))
}

/// Lightning-density upload (strikes/km²/min → log index), kept public for the headless harness.
pub(crate) fn lightning_upload(f: &wxdata::mrms::MrmsField) -> crate::render::MrmsUpload {
    let map = |v: f32| {
        if v <= 0.0 {
            0
        } else {
            (2.0 + ((v.log10() + 1.7) / 2.0).clamp(0.0, 1.0) * 253.0) as u8
        }
    };
    field_index_upload(
        f,
        map,
        ramp_lut(&[
            (0.0, [255, 255, 255]),
            (0.35, [255, 240, 120]),
            (0.65, [255, 160, 40]),
            (1.0, [230, 60, 200]),
        ]),
    )
}

/// Build the GPU upload for an index-mapped field layer (everything except the reflectivity
/// mosaic, which needs the app's color table). Kept public for the headless harness.
pub(crate) fn field_upload_indexed(
    layer: crate::render::FieldLayer,
    f: &wxdata::mrms::MrmsField,
) -> crate::render::MrmsUpload {
    use crate::render::field_ramps::{ramp_for, FieldScale};
    if layer == crate::render::FieldLayer::GoesRgb {
        return rgb_upload(f);
    }
    // Lightning keeps its own mapping (density counts, not a physical scale).
    if layer
        .descriptor()
        .is_some_and(|field| field.default_palette == wxdata::field::PaletteId::LightningDensity)
    {
        return lightning_upload(f);
    }
    let Some(r) = ramp_for(layer) else {
        // The reflectivity-palette layers (mosaic, HRRR) route through the app method instead.
        return field_index_upload(f, |_| 0, vec![0u8; 256 * 4]);
    };
    let lut = match r.scale {
        FieldScale::Ramp { stops, .. } => crate::render::field_ramps::bake_ramp_lut(stops, r.alpha),
        FieldScale::Categorical(cats) => {
            let slots: Vec<(u8, [u8; 3])> = cats.iter().map(|&(i, rgb, _)| (i, rgb)).collect();
            categorical_lut(&slots, r.alpha)
        }
    };
    field_index_upload(f, |v| r.index(v), lut)
}

impl HookEchoApp {
    /// The lead the ensemble layer actually reads: the shared forecast hour, moved to the nearest
    /// one this field is published at (a 6-hour rain total only exists at multiples of six).
    pub(crate) fn ensemble_lead_hour(&self) -> u16 {
        self.ensemble.field.snap_lead(self.global_fcst_hour)
    }

    /// One line for layer options: which run this is, or why there is nothing yet.
    pub(crate) fn ensemble_status_line(&self) -> String {
        match (&self.ensemble_run, &self.ensemble_error) {
            (Some(run), _) => format!(
                "GEFS run {} · valid {} · {} of {} members",
                run.run.format("%Y-%m-%d %HZ"),
                run.valid().format("%a %H:%MZ"),
                run.members.len(),
                wxdata::ensemble::GEFS_MEMBERS
            ),
            (None, Some(error)) => format!("⚠ Ensemble unavailable: {error}"),
            (None, None) => "Fetching the 31 GEFS members…".into(),
        }
    }

    /// Recompute the ensemble statistic from the members already held and queue its texture.
    /// Cheap next to the fetch, which is why picking a statistic never goes back to the network.
    fn rebuild_ensemble_display(&mut self) {
        let layer = crate::render::FieldLayer::Ensemble;
        let Some(run) = self.ensemble_run.as_ref() else {
            return;
        };
        let view = self.ensemble;
        match wxdata::ensemble::combine(&run.members, view.statistic()) {
            Ok(grid) => {
                let upload = crate::ensemble_layer::upload(&grid, &view);
                let stamp = field_state::model_stamp(
                    &format!("GEFS ({} members)", run.members.len()),
                    &view.title(self.settings.temp_unit),
                    &grid,
                    Some(run.run),
                    false,
                );
                if let Some(state) = self.fields.get_mut(&layer) {
                    state.pending = Some(upload);
                    state.stamp = Some(stamp);
                }
                self.ensemble_grid = Some(grid);
                self.ensemble_display_key = Some(view.display_key());
                self.ensemble_error = None;
            }
            Err(err) => {
                self.ensemble_grid = None;
                self.ensemble_display_key = None;
                self.ensemble_error = Some(err.to_string());
                if let Some(state) = self.fields.get_mut(&layer) {
                    state.pending = None;
                    state.stamp = None;
                }
            }
        }
    }

    /// Build the GPU upload for `layer` from its freshly-fetched grid, picking the value→index
    /// mapping and color LUT that suit the product's units.
    fn field_upload(
        &self,
        layer: crate::render::FieldLayer,
        f: &wxdata::mrms::MrmsField,
    ) -> crate::render::MrmsUpload {
        use crate::render::FieldLayer as FL;
        if layer
            .descriptor()
            .is_some_and(|field| field.default_palette == wxdata::field::PaletteId::Reflectivity)
        {
            return mrms_upload(f, self.palettes.table(Moment::Reflectivity));
        }
        match layer {
            // Mosaic + HRRR forecast are both dBZ → the reflectivity palette.
            FL::Mrms | FL::Mosaic | FL::Hrrr => {
                mrms_upload(f, self.palettes.table(Moment::Reflectivity))
            }
            other => field_upload_indexed(other, f),
        }
    }
}

/// Marker color for a storm-cell kind (sRGB).
/// `[r,g,b,a]` -> egui `Color32` (unmultiplied).
/// Blit one icon-sheet cell centered on its hot spot, rotated `angle_deg` clockwise.
/// A raw 4-vertex mesh because `Painter::image` can't rotate.
#[allow(clippy::too_many_arguments)] // a params struct for one call site buys nothing
fn draw_sprite(
    painter: &egui::Painter,
    tex: egui::TextureId,
    uv: egui::Rect,
    at: egui::Pos2,
    size: egui::Vec2,
    hot: egui::Vec2,
    angle_deg: f32,
    tint: egui::Color32,
) {
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    // Corner offsets relative to the hot spot, then rotated about it.
    let corner = |dx: f32, dy: f32| {
        let (x, y) = (dx - hot.x, dy - hot.y);
        at + egui::vec2(x * cos - y * sin, x * sin + y * cos)
    };
    let mut mesh = egui::Mesh::with_texture(tex);
    for (dx, dy, u, v) in [
        (0.0, 0.0, uv.left(), uv.top()),
        (size.x, 0.0, uv.right(), uv.top()),
        (size.x, size.y, uv.right(), uv.bottom()),
        (0.0, size.y, uv.left(), uv.bottom()),
    ] {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: corner(dx, dy),
            uv: egui::pos2(u, v),
            color: tint,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// Where a placefile's icon sheet is kept between runs, if this platform has a disk.
///
/// Named by a hash of the URL: sheets come from arbitrary hosts and their paths are not safe to
/// use as filenames.
fn icon_sheet_cache_path(url: &str) -> Option<std::path::PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut h);
    Some(
        crate::paths::cache_dir()?
            .join("pficons")
            .join(format!("{:016x}", h.finish())),
    )
}

/// Download and decode a placefile icon sheet (PNG/GIF) into an egui image.
async fn fetch_icon_sheet(http: &reqwest::Client, url: &str) -> anyhow::Result<egui::ColorImage> {
    // Read-through disk cache, same shape as the basemap tiles: an icon sheet is a few KB of PNG
    // that never changes, and re-fetching every placefile's sheet at every startup is the kind of
    // traffic somebody else's web host notices. A corrupt file simply fails to decode and is
    // refetched. Nothing on the web, where `cache_dir()` is None.
    let cached = icon_sheet_cache_path(url);
    let hit = cached
        .as_ref()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| image::load_from_memory(&b).ok());
    let img = match hit {
        Some(img) => img.to_rgba8(),
        None => {
            // Generic web hosts, so use the browser-ish UA the tile fetches already send.
            let bytes = http
                .get(url)
                .header("User-Agent", crate::tiles::USER_AGENT)
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            let img = image::load_from_memory(&bytes)?.to_rgba8();
            if let Some(path) = &cached {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(path, &bytes);
            }
            img
        }
    };
    let (w, h) = img.dimensions();
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        img.as_raw(),
    ))
}

fn rgba32(c: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

fn cell_color(kind: CellKind) -> [u8; 4] {
    match kind {
        CellKind::Storm => [255, 235, 60, 255], // yellow
        CellKind::Hail => [80, 220, 120, 255],  // green
        CellKind::Meso => [255, 70, 70, 255],   // red
    }
}

/// The storm cell (with a non-empty SCIT id) nearest to `(lon, lat)` within `max_km`, if any.
/// Used by the storm-follow camera to reacquire a tracked cell after SCIT renumbers it.
fn nearest_cell(cells: &[Cell], lon: f64, lat: f64, max_km: f64) -> Option<&Cell> {
    cells
        .iter()
        .filter(|c| !c.id.is_empty())
        .map(|c| (c, crate::geo::great_circle([lon, lat], [c.lon, c.lat]).0))
        .filter(|(_, km)| *km <= max_km)
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(c, _)| c)
}

/// The widget's storm line: the nearest tracked cell to `(lon, lat)`, in the words a glance can
/// read — "R3 12 mi SSW, moving NE 35 mph". `None` when nothing is being tracked nearby, which
/// the widget renders as no line at all rather than as "no storms" (the app may simply have been
/// looking at another part of the country).
fn widget_storm_line(cells: &[Cell], lon: f64, lat: f64, metric: bool) -> Option<String> {
    let c = nearest_cell(cells, lon, lat, 300.0)?;
    let (km, bearing) = crate::geo::great_circle([c.lon, c.lat], [lon, lat]);
    // Bearing is measured from the cell to you, so the compass point is the side of you the storm
    // sits on once it is flipped back.
    let from = crate::geo::compass(((bearing + 180.0) % 360.0) as f32);
    let mut line = format!(
        "{} {} {from}",
        if c.id.is_empty() { &c.title } else { &c.id },
        crate::geo::fmt_distance(km, metric, 0)
    );
    if let (Some(dir), Some(kt)) = (c.mvt_deg, c.mvt_kt) {
        if kt >= 1.0 {
            line.push_str(&format!(
                ", moving {} {:.0} mph",
                crate::geo::compass(dir),
                kt * 1.150_779
            ));
        }
    }
    Some(line)
}

/// The longest consecutive segment of a screen-space polyline (for placing a contour label).
fn longest_segment(pts: &[egui::Pos2]) -> Option<(egui::Pos2, egui::Pos2)> {
    pts.windows(2).map(|w| (w[0], w[1])).max_by(|a, b| {
        a.0.distance(a.1)
            .partial_cmp(&b.0.distance(b.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

/// Marker color for an SPC storm-report kind.
fn report_color(kind: wxdata::spc::ReportKind) -> [u8; 4] {
    use wxdata::spc::ReportKind as R;
    match kind {
        R::Tornado => [230, 40, 40, 255], // red
        R::Wind => [70, 130, 240, 255],   // blue
        R::Hail => [70, 210, 110, 255],   // green
        R::Flood => [0, 150, 90, 255],    // dark green
        R::Other => [180, 180, 180, 255], // gray
    }
}

/// Display-unit factor and label for a moment: velocity/spectrum-width honor the Units
/// setting (internal data stays m/s), everything else uses its native unit.
pub(crate) fn display_units(moment: Moment, settings: &Settings) -> (f32, &'static str) {
    match moment {
        Moment::Velocity | Moment::SpectrumWidth => (
            settings.velocity_unit.factor_from_ms(),
            settings.velocity_unit.label(),
        ),
        _ => (1.0, moment.units()),
    }
}

/// The track whose most recent point is nearest `(lon, lat)`, for pairing a marker being drawn
/// right now with its own score history: `compute_tds_score_track`/`compute_rot_score_track`
/// fold every hit into a track by proximity alone, so a hit and its own track's latest point are
/// two readings of the same association, not a coincidence — a tight tolerance is enough, and
/// keeps a different, merely nearby, hit's track from being shown by mistake.
fn nearest_score_track(
    tracks: &[wxdata::scoretrack::ScoreTrack],
    lon: f64,
    lat: f64,
) -> Option<&wxdata::scoretrack::ScoreTrack> {
    const MAX_KM: f64 = 0.5;
    tracks
        .iter()
        .filter_map(|tr| {
            let last = tr.points.last()?;
            let km = crate::geo::great_circle([lon, lat], [last.lon, last.lat]).0;
            (km <= MAX_KM).then_some((tr, km))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(tr, _)| tr)
}

/// The tooltip body a TDS/rotation marker's hover already shows (`explain().lines()`), plus a
/// sparkline of its score history when one exists — same "every term, its measurement, what each
/// stage added" hover, extended with what changed from volume to volume rather than just what the
/// number is right now. A history under two points is not a timeline yet, so it is left out
/// rather than drawn as a flat, meaningless line.
fn score_tooltip(
    ui: &mut egui::Ui,
    lines: Vec<String>,
    track: Option<&wxdata::scoretrack::ScoreTrack>,
    color: egui::Color32,
) {
    ui.label(lines.join("\n"));
    if let Some(tr) = track.filter(|tr| tr.points.len() >= 2) {
        ui.separator();
        ui.small(format!("Confidence, {} volumes", tr.points.len()));
        let vals: Vec<f32> = tr.points.iter().map(|p| p.confidence * 100.0).collect();
        crate::theme::sparkline_sized(ui, &vals, color, egui::vec2(180.0, 28.0));
    }
}

/// A coarse "N ago" string for volume age.
/// Compass point for a bearing in degrees from north (used in alarm text).
fn cardinal(bearing_deg: f64) -> &'static str {
    const POINTS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    POINTS[(((bearing_deg % 360.0 + 360.0) % 360.0) / 45.0).round() as usize % 8]
}

fn humanize(secs: i64) -> String {
    const DAY: i64 = 86_400;
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 2 * DAY {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    } else if secs < 365 * DAY {
        // Scrub back to a historic event and the hours keep counting: a 2011 storm read
        // "133672h40m ago", which is technically true and completely useless.
        format!("{}d", secs / DAY)
    } else {
        format!("{:.1}y", secs as f64 / (365.25 * DAY as f64))
    }
}

/// True if the feature's bounding box overlaps `box = (min_lon, min_lat, max_lon, max_lat)`.
/// Features with no geometry (no bbox) are treated as not overlapping.
fn feature_in_box(f: &GeoFeature, bx: (f64, f64, f64, f64)) -> bool {
    let Some((x0, y0, x1, y1)) = f.bbox() else {
        return false;
    };
    let (bx0, by0, bx1, by1) = bx;
    x1 >= bx0 && x0 <= bx1 && y1 >= by0 && y0 <= by1
}

impl eframe::App for HookEchoApp {
    /// Flush any settings change the one-second dirty-diff throttle hasn't picked up yet.
    fn on_exit(&mut self) {
        // Remember where we were looking, so a relaunch picks up the map where it was left. Only
        // while no explicit startup view is saved — that one is the user's choice, not ours.
        // Written here rather than per-frame: Android's alert service reads this file concurrently.
        #[cfg(not(target_os = "android"))]
        if self.settings.start_view.is_none() {
            log::debug!("remembering the last view for next launch");
            let view = &self.views[self.active];
            if let Some(site) = &view.site {
                self.settings.last_view = Some(crate::settings::StartView {
                    site: site.clone(),
                    x: view.camera.center.0,
                    y: view.camera.center.1,
                    zoom: view.camera.zoom,
                });
            }
        }
        // Anything quiet hours is still holding: it is owed to the user when the window ends, and
        // that can be after a restart.
        // A poisoned lock still holds the data; dropping it here would silently lose
        // notifications the user is owed.
        self.settings.quiet_pending = match self.quiet_queue.lock() {
            Ok(q) => q.clone(),
            Err(p) => p.into_inner().clone(),
        };
        if self.settings != self.saved {
            self.settings.save();
        }
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // Android: feed the status-bar / gesture-bar insets so no UI draws under system chrome.
        crate::platform::apply_safe_area(ctx, raw_input);
        // Android: GameActivity's IME reports edits as editor state, not keystrokes; turn that
        // state into the text/backspace events egui's focused field expects.
        crate::platform::pump_ime(raw_input);
        // Android: clipboard text fetched by the paste bar lands as a real egui Paste event, so
        // the focused text field inserts it exactly like Ctrl+V would.
        if let Some(text) = self.pending_paste.take() {
            raw_input.events.push(egui::Event::Paste(text));
        }
        // Web: never let eframe drop a frame's texture changes as hidden (see the function).
        crate::platform::guard_font_atlas(raw_input);
    }

    fn ui(&mut self, root: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Timed from outside so every early return inside is counted too.
        let start = wxdata::clock::Instant::now();
        self.ui_frame(root, frame);
        self.frame_times
            .push(start.elapsed().as_secs_f32() * 1000.0);
    }
}

impl HookEchoApp {
    fn ui_frame(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        crate::profiling::new_frame();
        crate::prof_scope!("ui");
        let ctx = root.ctx().clone();
        let ctx = &ctx;

        // Phone or tablet, before anything asks: a tablet draws the desktop layout, and every
        // layout decision this frame reads the answer.
        crate::platform::form_factor::update(ctx.viewport_rect().size().min_elem());

        // Stamp this frame for the background workers' foreground gate (see
        // `platform::activity`). A frame that follows a gap means the app just came back, so
        // force one refresh rather than making the user wait out the poll interval.
        self.frame_nr = self.frame_nr.wrapping_add(1);
        wxdata::stats::bump(wxdata::stats::Counter::FramesDrawn);
        self.gesture_live = ctx.input(map_gesture_live);
        #[cfg(not(target_arch = "wasm32"))]
        self.perf.tick(ctx);
        #[cfg(debug_assertions)]
        self.frame_time_overlay(ctx);
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if crate::platform::activity::mark_frame(focused) {
            self.overlay_last_fetch = None;
            for v in &mut self.views {
                v.last_poll = None;
            }
            // A resume is also how a notification tap arrives: the activity wrote the target
            // before handing us back the surface.
            self.drain_goto_file();
        }
        // A deep link that arrives while the app is already on screen never produces a resume, so
        // the drain above would never see it — on Android the activity is reused
        // (`launchMode="singleTask"`) and only `onNewIntent` fires; on desktop the second process
        // hands its link over and exits. ponytail: a one-second stat of a path that usually does
        // not exist, rather than a callback into the event loop.
        if self
            .goto_poll
            .is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(1))
        {
            self.goto_poll = Some(Instant::now());
            self.drain_goto_file();
            #[cfg(target_arch = "wasm32")]
            self.apply_goto_hash();
        }
        // The settings window has no HTTP client or runtime, so the voice-download button raises
        // a flag and the work happens here, on the same spawner everything else fetches on.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(id) = crate::speech::take_voice_request() {
            self.download_voice(id);
        }
        // A file the user picked, from any of the import buttons. Routed here rather than at the
        // button, because on Android the picker is an activity result that lands long after the
        // click — through the same file handover a notification tap uses.
        #[cfg(not(target_arch = "wasm32"))]
        self.tick_local_api(ctx);
        if let Some(import) = crate::dialog::take_result() {
            // A case restores panes, which needs the context the other imports do not.
            if import.kind == crate::dialog::ImportKind::Case {
                self.open_case(&import, ctx);
            } else {
                self.apply_import(import);
            }
        }

        // Android paste: re-focus the text field that lost focus to the Paste-button tap, before
        // any window draws, so the queued Paste event (see `raw_input_hook`) lands in it.
        if let Some(id) = self.paste_target.take() {
            ctx.memory_mut(|m| m.request_focus(id));
        }

        // Tray menu commands (Linux StatusNotifier): restore the window or quit for real.
        // Keep the tray menu telling the truth: alert count, mute state, starred sites. Sent only
        // when it changes — every send is a D-Bus round trip on the tray thread.
        {
            let want = crate::tray::TrayState {
                alerts: self.active_alert_features().len(),
                muted: self.settings.mute_alerts,
                starred: self.settings.presets.clone(),
            };
            if self.tray_state != want {
                self.tray_state = want.clone();
                crate::tray::set_state(want);
            }
        }
        {
            // Drained first: the handlers below need `&mut self`, and the receiver lives in it.
            let cmds: Vec<crate::tray::TrayCmd> = self.tray_rx.try_iter().collect();
            for cmd in cmds {
                match cmd {
                    crate::tray::TrayCmd::Show => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    crate::tray::TrayCmd::Quit => {
                        self.really_quit = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    crate::tray::TrayCmd::Mute => {
                        self.apply_action(BindableAction::ToggleMute, ctx);
                    }
                    crate::tray::TrayCmd::Site(id) => {
                        // Same path the site dialog uses: `sync_pane` does the rest next frame.
                        self.views[self.active].site = Some(id);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                }
            }
        }

        // Strikes age out on their own clock: the deque only shrinks here, so a quiet topic
        // still empties the map rather than freezing the last minute of a storm on it.
        if !self.strikes.is_empty() {
            let cutoff = chrono::Utc::now() - chrono::Duration::seconds(STRIKE_WINDOW_SECS);
            while self.strikes.front().is_some_and(|&(_, _, t)| t < cutoff) {
                self.strikes.pop_front();
            }
        }

        // Commands off the broker, drained the same way and applied through the same paths the
        // tray uses. They land on the next repaint rather than instantly, which for "point at the
        // storm" is close enough and keeps every state change on one thread.
        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        for cmd in crate::mqtt::drain() {
            match cmd {
                crate::mqtt::Cmd::Mute(want) => {
                    if self.settings.mute_alerts != want {
                        self.apply_action(BindableAction::ToggleMute, ctx);
                    }
                }
                crate::mqtt::Cmd::Site(id) => {
                    if wxdata::sites::site_by_id(&id).is_some() {
                        self.views[self.active].site = Some(id);
                    } else {
                        log::warn!("mqtt: no such site {id}");
                    }
                }
                crate::mqtt::Cmd::Product(code) => {
                    if let Some(m) = Moment::from_code(&code) {
                        let srv = self.views[self.active].srv;
                        self.apply_palette(PaletteAction::SetMoment(m, srv), ctx);
                    }
                }
                crate::mqtt::Cmd::Strike { lon, lat, time } => {
                    self.strikes.push_back((lon, lat, time));
                    // A busy night over a whole continent is a lot of strikes, and the painter
                    // walks the whole deque. Cap it and let the oldest fall off early.
                    while self.strikes.len() > STRIKE_CAP {
                        self.strikes.pop_front();
                    }
                }
            }
        }

        // Run-in-background: when the user closes the window and close-to-tray is on (and it wasn't
        // a tray "Quit"), cancel the quit and hide instead — the app keeps polling alerts and
        // pushing ntfy. Restore via the tray icon (or the taskbar when no tray host is present).
        if self.settings.close_to_tray
            && !self.really_quit
            && ctx.input(|i| i.viewport().close_requested())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            let cmd = if self.tray_present.load(std::sync::atomic::Ordering::Relaxed) {
                egui::ViewportCommand::Visible(false) // hide fully; the tray restores it
            } else {
                egui::ViewportCommand::Minimized(true) // no tray → keep a taskbar entry
            };
            ctx.send_viewport_cmd(cmd);
        }

        // "Modern dark pro" styling (palette/spacing/rounding/accent). Re-applied when the theme
        // or the system light/dark preference changes — it rebuilds and installs a whole
        // `egui::Style`, which is wasted work on every other frame.
        let system_dark = ctx.input(|i| i.raw.system_theme) != Some(egui::Theme::Light);
        let theme_key = (
            self.settings.theme,
            system_dark,
            self.settings.density,
            self.settings.accent,
        );
        if self.theme_applied != Some(theme_key) {
            crate::theme::apply(
                ctx,
                self.settings.theme,
                system_dark,
                self.settings.density,
                self.settings.accent,
            );
            self.theme_applied = Some(theme_key);
        }

        // UI scale: apply the setting when the slider moved, else absorb built-in keyboard zoom
        // (Ctrl+= / Ctrl+- / Ctrl+0) back into the setting so it persists.
        if (self.settings.ui_scale - self.ui_scale_applied).abs() > 1e-3 {
            ctx.set_zoom_factor(self.settings.ui_scale);
            self.ui_scale_applied = self.settings.ui_scale;
        } else {
            let z = ctx.zoom_factor();
            self.settings.ui_scale = z;
            self.ui_scale_applied = z;
        }

        self.drive_snapshot_push(ctx);
        self.drive_widget_snapshot(ctx);
        self.save_pending_screenshot(ctx);
        self.load_marker_icons(ctx);
        self.drive_loop_export(ctx);
        self.apply_chase();
        self.sync_share(ctx);
        self.poll_sync();
        self.sync_forecast_scrub();
        self.poll_messages();
        self.poll_overlays();
        // Time-machine warnings + storm reports: swap in archived sets while scrubbed.
        self.sync_archive_warnings(ctx);
        self.sync_archive_lsr(ctx);
        // Surface obs (METAR station plots).
        self.sync_metar(ctx);
        self.sync_webcams(ctx);
        self.sync_fires(ctx);
        self.sync_aqi(ctx);
        self.sync_stations(ctx);
        self.sync_dat(ctx);
        self.sync_mosaic(ctx);
        // River flood gauges (NWPS).
        self.sync_gauges(ctx);
        // HRRR model contours.
        self.sync_contours(ctx);
        // Hurricane-hunter observations: a mission transmits every 30 s, so 10 min is plenty.
        if self.show_recon
            && self
                .recon_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 600)
        {
            self.recon_last_fetch = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::Recon);
        }
        // County outages: ODIN's upstream updates about every 15 min; 5 min keeps a fast-moving
        // event current without asking much of a free, keyless API.
        if self.show_outages
            && self
                .outages_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 300)
        {
            self.outages_last_fetch = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::Outages);
        }
        // Tropical model guidance: its own half-hourly clock, only while it is on.
        self.spaghetti.poll();
        if self.show_tropical && self.spaghetti.due() {
            let (rt, http) = (self.spawner.clone(), self.http.clone());
            self.spaghetti.fetch(&rt, &http, ctx);
        }
        // NHC tropical suite: refresh every 15 min while enabled.
        if self.show_tropical
            && self
                .tropical_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 900)
        {
            self.tropical_last_fetch = Some(Instant::now());
            self.spawn_overlay(
                ctx,
                OverlaySource::Tropical(self.tropical_wind_kt, self.tropical_surge),
            );
        }
        // Periodic overlay refresh (~2 min), honoring live weather cadence. Skipped entirely
        // while backgrounded — see `platform::activity`.
        let overlay_secs = if crate::platform::is_metered() {
            240
        } else {
            120
        };
        // On the web the first pass through here *is* the boot fetch (App::new skips it), so hold
        // it until there is radar on screen — or five seconds have gone by and the radar is
        // evidently not coming, in which case the overlays are the only thing left to draw.
        let overlays_may_start = !cfg!(target_arch = "wasm32")
            || self.views[self.active].volume.is_some()
            || self.boot_at.elapsed().as_secs() >= 5;
        if overlays_may_start
            && crate::platform::activity::is_active()
            && self
                .overlay_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= overlay_secs)
        {
            self.fetch_overlays(ctx);
        }
        // MRMS national mosaic: fetch when enabled, refresh at the ~2-min product cadence.
        // National field layers: fetch each enabled layer at its product cadence.
        use crate::render::FieldLayer as FL;
        for layer in FL::DRAW_ORDER {
            // Layers with a fetch block of their own answer `None` and are skipped here.
            let Some(request) = self.mrms_request(layer) else {
                continue;
            };
            // The reflectivity tint reads the precipitation-type grid whether or not that
            // layer is being drawn, so wanting the tint counts as wanting the layer's data.
            let wanted =
                self.field_wanted(layer) || (layer == FL::PrecipType && self.settings.precip_tint);
            let selection_changed = self
                .fields
                .get(&layer)
                .is_none_or(|s| s.mrms_request.as_ref() != Some(&request));
            let stale = wanted
                && self.fields.get(&layer).is_none_or(|s| {
                    selection_changed
                        || s.last_fetch
                            .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            if stale {
                let state = self.fields.entry(layer).or_default();
                if selection_changed {
                    state.pending = None;
                    state.stamp = None;
                    state.mrms_request = Some(request.clone());
                    if layer == FL::PrecipType {
                        self.precip_flag_grid = None;
                        self.precip_flag_gen = self.precip_flag_gen.wrapping_add(1);
                    }
                }
                state.last_fetch = Some(Instant::now());
                self.spawn_overlay(ctx, OverlaySource::Field(layer, request));
            }
        }
        // Snow bands: the mosaic and the precipitation-type grid, cut to the banded snow.
        {
            let layer = FL::SnowBands;
            let stale = self.view_target_time().is_none()
                && self.field_wanted(layer)
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            if stale {
                if let Some(s) = self.fields.get_mut(&layer) {
                    s.last_fetch = Some(Instant::now());
                }
                self.spawn_overlay(ctx, OverlaySource::SnowBands);
            }
        }
        // GOES bands: no forecast hour, no product path — read straight from S3. Which satellite
        // is a setting, not a per-layer choice, so flipping it has to refetch every band at once
        // rather than waiting out the normal cadence.
        let west = self.settings.goes_satellite_west;
        let satellite = if west {
            wxdata::goes_abi::Satellite::West
        } else {
            wxdata::goes_abi::Satellite::East
        };
        // The ABI sector to read: the chosen one, unless it is a mesoscale box that has moved
        // away from the view, which reads CONUS until it covers the view again (ROADMAP_NEW E5).
        let sector = self.goes_sector_now();
        // Scrubbed back (or linked to an archive instant), the frame nearest that time; live, the
        // newest, refreshed on the scan cadence.
        let at = self.view_target_time();
        let slot = goes_slot(at, sector);
        let refresh = |layer| {
            if sector.is_meso() {
                sector.cadence_secs()
            } else {
                field_refresh_secs(layer)
            }
        };
        for layer in [
            FL::GoesIr,
            FL::GoesVisible,
            FL::GoesWaterVapor,
            FL::GoesShortwaveIr,
            FL::GoesMidWaterVapor,
            FL::GoesLowWaterVapor,
            FL::GoesDirtyIr,
            FL::GoesLongwaveIr,
            FL::GoesColdTop,
        ] {
            let on = self.field_wanted(layer);
            // An archive frame never changes: only the newest is refreshed.
            let stale = on
                && at.is_none()
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= refresh(layer))
                });
            let changed = on
                && (self.goes_west_key.get(&layer) != Some(&west)
                    || self.goes_fetched_sector.get(&layer) != Some(&sector)
                    || self.goes_fetched_slot.get(&layer) != Some(&slot));
            if stale || changed {
                self.fields.entry(layer).or_default().last_fetch = Some(Instant::now());
                self.goes_west_key.insert(layer, west);
                self.goes_fetched_sector.insert(layer, sector);
                self.goes_fetched_slot.insert(layer, slot);
                self.spawn_overlay(ctx, OverlaySource::Goes(layer, satellite, sector, at));
            }
        }
        // Falling back to CONUS: keep an eye on where the chosen box goes, so the view gets it
        // back the minute it covers the view again.
        let chosen = self.settings.goes_sector;
        if chosen.is_meso()
            && sector != chosen
            && self.goes_layers_on()
            && self
                .goes_footprint_probe
                .is_none_or(|t| t.elapsed().as_secs() >= chosen.cadence_secs())
        {
            self.goes_footprint_probe = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::GoesFootprint(satellite, chosen, at));
        }
        // GOES channel-difference products: same staleness/satellite-flip rules as the single-band
        // layers above, but a distinct `OverlaySource` variant since each one fetches two bands.
        // Kept as a list with one entry rather than unrolled: a second difference product only
        // needs a new `FieldLayer` here, and the shape matches the multi-layer block above it.
        #[allow(clippy::single_element_loop)]
        for layer in [FL::GoesDustDiff] {
            let on = self.field_wanted(layer);
            let stale = on
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            let changed = on && self.goes_west_key.get(&layer) != Some(&west);
            if stale || changed {
                self.fields.entry(layer).or_default().last_fetch = Some(Instant::now());
                self.goes_west_key.insert(layer, west);
                self.spawn_overlay(ctx, OverlaySource::GoesDiff(layer, satellite));
            }
        }
        // GOES time-difference products: same staleness/satellite-flip rules as the two blocks
        // above, but a distinct `OverlaySource` variant since each one fetches the same band at
        // two different times instead of two bands at the same time. Same reason as above for
        // keeping the one-entry list rather than unrolling it.
        #[allow(clippy::single_element_loop)]
        for layer in [FL::GoesCoolingRate] {
            let on = self.field_wanted(layer);
            let stale = on
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            let changed = on && self.goes_west_key.get(&layer) != Some(&west);
            if stale || changed {
                self.fields.entry(layer).or_default().last_fetch = Some(Instant::now());
                self.goes_west_key.insert(layer, west);
                self.spawn_overlay(ctx, OverlaySource::GoesCoolingRate(layer, satellite));
            }
        }
        // The GOES RGB composite: the same five-minute cadence and satellite flip, and a new
        // recipe refetches at once (its bands differ).
        {
            let layer = FL::GoesRgb;
            let recipe = wxdata::goes_rgb::by_slug(&self.settings.goes_rgb_recipe)
                .unwrap_or(&wxdata::goes_rgb::AIR_MASS);
            let on = self.field_wanted(layer);
            let stale = on
                && at.is_none()
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= refresh(layer))
                });
            let changed = on
                && (self.goes_west_key.get(&layer) != Some(&west)
                    || self.goes_rgb_fetched != Some(recipe.slug)
                    || self.goes_fetched_sector.get(&layer) != Some(&sector)
                    || self.goes_fetched_slot.get(&layer) != Some(&slot));
            if stale || changed {
                self.fields.entry(layer).or_default().last_fetch = Some(Instant::now());
                self.goes_west_key.insert(layer, west);
                self.goes_rgb_fetched = Some(recipe.slug);
                self.goes_fetched_sector.insert(layer, sector);
                self.goes_fetched_slot.insert(layer, slot);
                self.spawn_overlay(ctx, OverlaySource::GoesRgb(recipe, satellite, sector, at));
            }
        }
        // NDFD elements: also no forecast hour to scrub — each fetch is the whole short-range
        // bundle and this always shows the message valid nearest to now.
        for layer in [
            FL::NdfdTemp2m,
            FL::NdfdWind10m,
            FL::NdfdGust10m,
            FL::NdfdSnow,
        ] {
            let stale = self.field_wanted(layer)
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            if stale {
                self.fields.entry(layer).or_default().last_fetch = Some(Instant::now());
                self.spawn_overlay(ctx, OverlaySource::Ndfd(layer));
            }
        }
        // RTMA analysis: a new hour posts about 45 minutes after it. Naming an analysis hour in the
        // browser refetches at once; the pin only applies while RTMA is the selected model, so a
        // run picked for the HRRR is never read as an analysis hour.
        for layer in [
            FL::RtmaTemp2m,
            FL::RtmaDewpoint2m,
            FL::RtmaWind10m,
            FL::RtmaGust10m,
            FL::RtmaVisibility,
            FL::RtmaCeiling,
            FL::RtmaMslp,
            FL::RtmaPrecip1h,
        ] {
            let hour = if self.model_sel.model == crate::model_browser::BModel::Rtma {
                self.model_run
            } else {
                None
            };
            let on = self.field_wanted(layer);
            let changed = on && self.rtma_key.get(&layer) != Some(&hour);
            let stale = on
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            if stale || changed {
                self.fields.entry(layer).or_default().last_fetch = Some(Instant::now());
                self.rtma_key.insert(layer, hour);
                self.spawn_overlay(ctx, OverlaySource::Rtma(layer, hour));
            }
        }
        // Environment suite (CAPE/SRH): the model browser's model at the scrubbed forecast hour.
        // Changing the model or the hour refetches now rather than on the slow cadence.
        for layer in [FL::Cape, FL::Srh] {
            let run = self.pinned_regional_run(self.env_model);
            let key = (self.env_model, self.hrrr_fcst_hour, run);
            let changed = self.field_wanted(layer) && self.env_fetch_key.get(&layer) != Some(&key);
            let stale = self.field_wanted(layer)
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            if stale || changed {
                if let Some(s) = self.fields.get_mut(&layer) {
                    s.last_fetch = Some(Instant::now());
                }
                self.env_fetch_key.insert(layer, key);
                self.spawn_overlay(
                    ctx,
                    OverlaySource::Env(
                        layer,
                        self.env_model,
                        self.env_cape_ml,
                        self.env_srh_km,
                        self.hrrr_fcst_hour,
                        run,
                    ),
                );
            }
        }
        // Global models: whichever source and forecast hour the user picked.
        for (layer, gfield) in [
            (FL::GlobalMslp, wxdata::global::GlobalField::Mslp),
            (FL::GlobalHeight500, wxdata::global::GlobalField::Height500),
            (FL::GlobalTemp2m, wxdata::global::GlobalField::Temp2m),
            (
                FL::GlobalDewpoint2m,
                wxdata::global::GlobalField::Dewpoint2m,
            ),
            (FL::GlobalWind10m, wxdata::global::GlobalField::Wind10m),
            (FL::GlobalPrecip, wxdata::global::GlobalField::Precip),
        ] {
            let fh = self.global_fcst_hour;
            let model = self.global_model;
            let on = self.field_wanted(layer);
            let stale = on
                && self.fields.get(&layer).is_some_and(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            // Changing the source or the hour has to refetch now, not on the next slow cadence.
            let run = self.pinned_global_run();
            let changed = on && self.global_layer_key.get(&layer) != Some(&(model, fh, run));
            if stale || changed {
                if let Some(s) = self.fields.get_mut(&layer) {
                    s.last_fetch = Some(Instant::now());
                }
                self.global_layer_key.insert(layer, (model, fh, run));
                self.spawn_overlay(ctx, OverlaySource::Global(layer, model, gfield, fh, run));
            }
        }
        // Model difference: same cadence as a global layer, and the same refetch-on-change rule.
        {
            let layer = FL::ModelDiff;
            let fh = self.global_fcst_hour;
            let on = self.field_wanted(layer);
            let stale = on
                && self.fields.get(&layer).is_some_and(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            let changed = on && self.diff_key != Some((self.diff_field, fh));
            // A field with no meaningful ratio falls back from the percent view.
            if !self.diff_mode.offered_for(self.diff_field) {
                self.diff_mode = crate::fielddiff::DiffMode::Signed;
            }
            let display_key = (self.diff_field, self.diff_mode);
            // Remembered across restarts; written only when it changes.
            if self.settings.compare_view != Some(display_key) {
                self.settings.compare_view = Some(display_key);
            }
            let display_changed = on
                && !changed
                && self.diff_display_key != Some(display_key)
                && self.diff_valid.is_some();
            if display_changed {
                if let Some(grid) = self.diff_grid.clone() {
                    let upload = self.diff_upload(&grid);
                    if let Some(state) = self.fields.get_mut(&layer) {
                        state.pending = Some(upload);
                        if let Some(stamp) = state.stamp.as_mut() {
                            let (a, b) = self.diff_field.pair();
                            stamp.source_id = self.diff_mode.expression(a, b);
                        }
                    }
                    self.diff_display_key = Some(display_key);
                }
            }
            if stale || changed {
                if changed {
                    self.diff_valid = None;
                    self.diff_grid = None;
                    self.diff_pct = None;
                    self.diff_display_key = None;
                    if let Some(s) = self.fields.get_mut(&layer) {
                        s.stamp = None;
                    }
                }
                self.diff_error = None;
                if let Some(s) = self.fields.get_mut(&layer) {
                    s.last_fetch = Some(Instant::now());
                }
                self.diff_key = Some((self.diff_field, fh));
                self.spawn_overlay(ctx, OverlaySource::ModelDiff(self.diff_field, fh));
            }
        }
        // Ensemble layer: the member fetch is keyed on (field, hour); the statistic and threshold
        // only rebuild the display from the members already held.
        {
            let layer = FL::Ensemble;
            let fh = self.ensemble_lead_hour();
            let on = self.field_wanted(layer);
            let stale = on
                && self.fields.get(&layer).is_some_and(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            let changed = on && self.ensemble_key != Some((self.ensemble.field, fh));
            if on
                && !changed
                && self.ensemble_run.is_some()
                && self.ensemble_display_key != Some(self.ensemble.display_key())
            {
                self.rebuild_ensemble_display();
            }
            if stale || changed {
                if changed {
                    self.ensemble_run = None;
                    self.ensemble_grid = None;
                    self.ensemble_display_key = None;
                    if let Some(s) = self.fields.get_mut(&layer) {
                        s.stamp = None;
                    }
                }
                self.ensemble_error = None;
                if let Some(s) = self.fields.get_mut(&layer) {
                    s.last_fetch = Some(Instant::now());
                }
                self.ensemble_key = Some((self.ensemble.field, fh));
                self.spawn_overlay(ctx, OverlaySource::Ensemble(self.ensemble.field, fh));
            }
        }
        // Model comparison: the same two grids the difference layer fetches, shown side by side
        // instead of subtracted — one fetch feeds both `CompareA`/`CompareB`, so either wanting it
        // is enough to trigger it, and both get the same staleness stamp.
        {
            let fh = self.global_fcst_hour;
            let on = self.field_wanted(FL::CompareA) || self.field_wanted(FL::CompareB);
            let stale = on
                && self.fields.get(&FL::CompareA).is_some_and(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(FL::CompareA))
                });
            let changed = on && self.compare_key != Some((self.diff_field, fh));
            if stale || changed {
                if changed {
                    self.compare_valid = None;
                    self.compare_grid = None;
                    for layer in [FL::CompareA, FL::CompareB] {
                        if let Some(s) = self.fields.get_mut(&layer) {
                            s.stamp = None;
                        }
                    }
                }
                self.compare_error = None;
                for layer in [FL::CompareA, FL::CompareB] {
                    if let Some(s) = self.fields.get_mut(&layer) {
                        s.last_fetch = Some(Instant::now());
                    }
                }
                self.compare_key = Some((self.diff_field, fh));
                self.spawn_overlay(ctx, OverlaySource::Compare(self.diff_field, fh));
            }
        }
        // HRRR rotation tracks + smoke: same forecast-hour scrub as future radar, own cadences.
        for layer in [
            FL::UpdraftHelicity,
            FL::Smoke,
            FL::Snowfall,
            FL::ThunderProb,
        ] {
            let fh = self.hrrr_fcst_hour;
            let stale = self.field_wanted(layer)
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            // Scrubbing the forecast tail must refetch immediately, not wait out the cadence.
            // Naming a different run counts as a change for the same reason.
            let run = self.pinned_regional_run(if layer == FL::ThunderProb {
                wxdata::hrrr::Model::Nbm
            } else {
                wxdata::hrrr::Model::Hrrr
            });
            let hour_changed =
                self.field_wanted(layer) && self.hrrr_layer_hour.get(&layer) != Some(&(fh, run));
            if stale || hour_changed {
                if let Some(s) = self.fields.get_mut(&layer) {
                    s.last_fetch = Some(Instant::now());
                }
                self.hrrr_layer_hour.insert(layer, (fh, run));
                self.spawn_overlay(ctx, OverlaySource::HrrrLayer(layer, fh, run));
            }
        }
        // Quiet hours just ended: replay what it held back as one push, so waking up to a silent
        // night does not mean waking up to no idea what happened during it.
        let now_quiet = self.in_quiet_hours();
        if self.was_quiet && !now_quiet {
            let held = match self.quiet_queue.lock() {
                Ok(mut q) => std::mem::take(&mut *q),
                Err(_) => Vec::new(),
            };
            if !held.is_empty() {
                let (title, body) = quiet_summary(&held);
                self.notify_alert(&title, &body, false);
            }
        }
        self.was_quiet = now_quiet;
        // Hold the queue on disk as it changes, not only on a clean exit. A crash or a kill
        // during quiet hours used to lose the whole night's held alerts; the queue is a handful
        // of short strings, so comparing it every tick and writing only on a real change costs
        // nothing worth measuring.
        if let Ok(q) = self.quiet_queue.lock() {
            if *q != self.settings.quiet_pending {
                self.settings.quiet_pending = q.clone();
                drop(q);
                self.settings.save();
            }
        }

        // GOES lightning: granules land every 20 s, so poll about that often. One in flight at a
        // time — a slow fetch must not queue up behind itself.
        let glm_fed_on = self.field_wanted(FL::GlmFed);
        // A lightning-density rule needs the flashes polled and the grid built even with the
        // layer off, exactly like the scan signatures.
        let glm_rule_armed = self.settings.alert_rules.iter().any(|r| {
            r.enabled
                && matches!(
                    r.trigger,
                    crate::settings::RuleTrigger::GlmFed | crate::settings::RuleTrigger::GlmJump
                )
        });
        if (self.show_glm || glm_fed_on || glm_rule_armed)
            && self
                .glm_last_poll
                .is_none_or(|t| t.elapsed().as_secs() >= 20)
            && !self.glm_polling.load(std::sync::atomic::Ordering::Relaxed)
        {
            self.glm_last_poll = Some(Instant::now());
            self.glm_polling
                .store(true, std::sync::atomic::Ordering::Relaxed);
            let feed = self.glm.clone();
            let west = self.settings.glm_goes_west;
            let busy = self.glm_polling.clone();
            let http = self.http.clone();
            let ctx2 = ctx.clone();
            self.spawner.spawn(async move {
                // Decode outside the lock: holding it across an await would stall the painter.
                let mut local = wxdata::glm::GlmFeed::new(15);
                local.set_west(west);
                if let Ok(f) = feed.lock() {
                    local.set_last_keys(f.last_keys().clone());
                }
                let added = local.poll(&http).await.unwrap_or(0);
                if let Ok(mut f) = feed.lock() {
                    f.absorb(local);
                }
                if added > 0 {
                    ctx2.request_repaint();
                }
                busy.store(false, std::sync::atomic::Ordering::Relaxed);
            });
        }

        // Scrubbed back: the flashes of the window ending at the view's time, once per minute of
        // scrubbing (ROADMAP_NEW E6).
        if let Some(target) = self.view_target_time() {
            if (self.show_glm || glm_fed_on) && self.glm_archive_slot != Some(glm_slot(target)) {
                self.glm_archive_slot = Some(glm_slot(target));
                self.spawn_overlay(
                    ctx,
                    OverlaySource::GlmWindow(target, self.settings.glm_goes_west),
                );
            }
        } else {
            self.glm_archive_slot = None;
        }

        // GLM flash-extent density: the same flashes the dots come from, gridded. Cheap enough
        // (one pass over a few thousand points) to do inline on the field-layer cadence rather
        // than spawning for it.
        if (glm_fed_on || glm_rule_armed)
            && self
                .glm_fed_last
                .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(FL::GlmFed))
        {
            self.glm_fed_last = Some(Instant::now());
            if let Some(s) = self.fields.get_mut(&FL::GlmFed) {
                s.last_fetch = Some(Instant::now());
            }
            let target = self.view_target_time();
            let field = self.glm.lock().ok().and_then(|f| {
                let (flashes, end) =
                    glm_flashes_for(target, f.flashes(), self.glm_archive.as_ref(), Utc::now());
                let flashes: std::collections::VecDeque<wxdata::glm::Flash> =
                    flashes.into_iter().copied().collect();
                wxdata::glm::flash_density(
                    &flashes,
                    self.settings.detectors.glm_fed_cell_deg,
                    chrono::Duration::minutes(self.settings.detectors.glm_fed_window_min),
                    end,
                )
            });
            // Alert rules watch what is happening now, never a scrubbed-back view.
            if let Some(field) = field.as_ref().filter(|_| target.is_none()) {
                self.evaluate_grid_rules(crate::settings::RuleTrigger::GlmFed, field);
                // The jump is the difference between this grid and the one before it, so it can
                // only be asked for once there is a previous one — the first grid after launch
                // has no rate.
                if let Some(prev) = &self.glm_fed_prev {
                    if let Some(jump) = wxdata::glm::flash_jump(prev, field) {
                        self.evaluate_grid_rules(crate::settings::RuleTrigger::GlmJump, &jump);
                    }
                }
                self.glm_fed_prev = Some(field.clone());
            }
            if let (Some(field), true) = (field, glm_fed_on) {
                let cap = self.field_texture_cap();
                let _ = self
                    .overlay_tx
                    .send(OverlayDelivery::Immediate(OverlayMsg::Field(
                        FL::GlmFed,
                        field.decimated(cap),
                    )));
            }
        }

        // Wind particles. HRRR posts hourly, so this gets its own 15-minute clock rather than
        // riding the 120 s overlay block — that would re-download 4.5 MB about thirty times per
        // useful update. Doubled on a metered connection.
        if self.show_wind {
            // Advection timestep, shared by every pane so they stay in step. Clamped because a
            // stalled frame or a resume from background would otherwise teleport the whole field.
            let now = Instant::now();
            self.wind_dt = self
                .wind_last_frame
                .map_or(0.0, |t| now.duration_since(t).as_secs_f32())
                // Headroom above the 100 ms Android cadence, so a normal phone frame is never
                // itself treated as a hitch and quietly slowed down.
                .clamp(0.0, 0.15);
            self.wind_last_frame = Some(now);
            // The app is otherwise idle-driven; this is its first always-on animation, and the
            // cost is not the particle mesh — it is re-rendering the whole map (radar warp,
            // vector basemap) every frame instead of sitting idle. Measured on an S24 Ultra:
            // 7% CPU idle, 78% animating at 20 fps, so the cadence is the battery knob. 10 fps on
            // a phone reads fine because the trail is itself the motion blur.
            // An unfocused window animating particles nobody is looking at is the whole cost
            // for none of the value; the idle heartbeat carries it until focus returns, and the
            // `wind_dt` clamp above absorbs the jump.
            if crate::platform::activity::is_active() && ctx.input(|i| i.focused) {
                let ms = if cfg!(target_os = "android") || ui::motion::degraded() {
                    100
                } else {
                    33
                };
                ctx.request_repaint_after(std::time::Duration::from_millis(ms));
            }

            // Panes come and go with the layout; their particle sets should not outlive them.
            self.wind_particles.retain(|k, _| *k < self.views.len());

            let want = (self.wind_level, self.hrrr_fcst_hour);
            let interval = if crate::platform::is_metered() {
                1800
            } else {
                900
            };
            let stale = self
                .wind_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= interval);
            // A level or forecast-hour change refetches at once — but the ~200 ms floor keeps a
            // fast drag across the forecast tail from firing a request per frame.
            let changed = self.wind_fetched != Some(want)
                && self
                    .wind_last_fetch
                    .is_none_or(|t| t.elapsed().as_millis() >= 200);
            // A dropped fetch (spawn_overlay only logs errors) expires rather than wedging.
            let free = self
                .wind_inflight
                .is_none_or(|t| t.elapsed().as_secs() >= 60);
            // Radar winds drive the particles on their own (`sync_radar_wind`).
            if (stale || changed) && free && !self.radar_wind.on {
                self.wind_last_fetch = Some(Instant::now());
                self.wind_inflight = Some(Instant::now());
                self.wind_fetched = Some(want);
                self.spawn_overlay(ctx, OverlaySource::Wind(want.0, want.1));
            }
        }

        // Observed snowfall analysis: its own block because the accumulation window is a knob,
        // and changing it must refetch at once rather than wait out the cadence.
        {
            let on = self.field_wanted(FL::SnowAnalysis);
            let stale = self.fields.get(&FL::SnowAnalysis).is_some_and(|s| {
                s.last_fetch
                    .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(FL::SnowAnalysis))
            });
            let window_changed = self.snow_fetched != Some(self.snow_hours);
            if on && (stale || window_changed) {
                if let Some(s) = self.fields.get_mut(&FL::SnowAnalysis) {
                    s.last_fetch = Some(Instant::now());
                }
                self.snow_fetched = Some(self.snow_hours);
                self.spawn_overlay(ctx, OverlaySource::Snow(self.snow_hours));
            }
        }
        // Crowd precip-type reports. Skipped entirely without a key — the layer is opt-in twice
        // over: you turn it on, and you supply your own mPING key.
        if self.show_mping
            && !self.settings.mping_key.trim().is_empty()
            && self
                .mping_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 300)
        {
            self.mping_last_fetch = Some(Instant::now());
            let key = self.settings.mping_key.trim().to_string();
            self.spawn_overlay(ctx, OverlaySource::Mping(key));
        }
        // Surface analysis: WPC reissues it a few times an hour.
        if self.show_fronts
            && self
                .fronts_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 1800)
        {
            self.fronts_last_fetch = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::Fronts);
        }
        // Locally derived products: no fetch, just a recompute when the volume or threshold moves.
        self.recompute_derived(ctx);
        // Beam-blockage raster: rebuilt when the camera, site, or tilt moves (DEM tiles are cached).
        self.update_blockage(ctx);
        self.update_lowest_tilt(ctx);
        self.update_coverage_compare(ctx);
        // Gridded L3 products (DVL/EET): per-site, refetch on the L3 cadence or a site change.
        let l3_site = self.views[self.active].site.clone();
        let site_changed = self.l3grid_site != l3_site;
        for layer in [FL::Vil, FL::EchoTops, FL::Hca] {
            let on = self.field_wanted(layer);
            if !on {
                continue;
            }
            let stale = self.fields.get(&layer).is_some_and(|s| {
                s.last_fetch
                    .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
            });
            if let Some(site) = &l3_site {
                if stale || site_changed {
                    if let Some(s) = self.fields.get_mut(&layer) {
                        s.last_fetch = Some(Instant::now());
                    }
                    self.spawn_overlay(ctx, OverlaySource::L3Grid(layer, site.clone()));
                }
            }
        }
        if site_changed
            && [FL::Vil, FL::EchoTops, FL::Hca]
                .iter()
                .any(|l| self.field_wanted(*l))
        {
            self.l3grid_site = l3_site;
        }
        // Forecast reflectivity: fetch when enabled and the model, forecast hour or run changed
        // (~10-min throttle; a new run posts hourly).
        let hrrr_on = self.field_wanted(FL::Hrrr);
        if hrrr_on {
            let stale = self
                .hrrr_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 600);
            // Sub-hourly and hourly are the same layer on one lane; the selected lead is the
            // 15-minute value in sub-hourly mode and the whole-hour value otherwise. Switching
            // modes counts as a change so the tail refetches at the new resolution.
            let run = self.pinned_regional_run(self.refl_model);
            let model_changed = self.hrrr_fetched_key != Some((self.refl_model, run));
            let (changed, source) = if self.hrrr_subhourly {
                (
                    model_changed || self.hrrr_fetched_min != Some(self.hrrr_fcst_min),
                    OverlaySource::HrrrSub(self.hrrr_fcst_min, run),
                )
            } else {
                (
                    model_changed || self.hrrr_fetched_hour != Some(self.hrrr_fcst_hour),
                    OverlaySource::Hrrr(self.refl_model, self.hrrr_fcst_hour, run),
                )
            };
            if changed || stale {
                self.hrrr_fetched_key = Some((self.refl_model, run));
                self.hrrr_fetched_hour = Some(self.hrrr_fcst_hour);
                self.hrrr_fetched_min = Some(self.hrrr_fcst_min);
                self.hrrr_last_fetch = Some(Instant::now());
                self.spawn_overlay(ctx, source);
            }
        }
        // Live LSR refresh (~2-min cadence; the IEM feed is minutes-fresh).
        // The reports layer, or a detector that confirms itself against them.
        if (self.show_storm_reports || self.filters.show_tds || self.filters.show_couplets)
            && self
                .reports_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 120)
        {
            self.reports_last_fetch = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::StormReports(None));
        }
        // Aviation SIGMET/AIRMET refresh (10-min cadence).
        if self.show_aviation
            && self
                .aviation_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 600)
        {
            self.aviation_last_fetch = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::Aviation);
        }
        // TFR refresh. Slow, because a restriction's shape never changes once issued — the
        // cadence is really about noticing new ones. While the first load is still filling in,
        // the next batch is asked for promptly instead.
        if self.show_tfr {
            let due = if self.tfr_pending > 0 { 5 } else { 900 };
            if self
                .tfr_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= due)
            {
                self.tfr_last_fetch = Some(Instant::now());
                let have: Vec<String> = self.tfr_features.keys().cloned().collect();
                self.spawn_overlay(ctx, OverlaySource::Tfr(have));
            }
        }
        // Spotter Network refresh (feed's own 1-min cadence).
        if self.show_spotters
            && self
                .spotters_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 60)
        {
            self.spotters_last_fetch = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::Spotters);
        }
        // ProbSevere refresh (~2-min product cadence).
        if self.show_probsevere
            && self
                .probsevere_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 120)
        {
            self.probsevere_last_fetch = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::ProbSevere);
        }
        // Sensors: fetch when the window is open and the site changed or the 10-min clock elapsed.
        if self.show_sensors {
            if let Some(site) = self.views[self.active].site.clone() {
                let stale = self
                    .sensor_last_fetch
                    .is_none_or(|t| t.elapsed().as_secs() >= 600);
                let site_changed = self.sensor_site.as_deref() != Some(site.as_str());
                if stale || site_changed {
                    if let Some(s) = wxdata::sites::site_by_id(&site) {
                        if site_changed {
                            self.sensor_data = None; // show "loading" until the new site returns
                        }
                        self.sensor_last_fetch = Some(Instant::now());
                        self.spawn_overlay(
                            ctx,
                            OverlaySource::Obs {
                                site: site.clone(),
                                lat: s.latitude as f64,
                                lon: s.longitude as f64,
                            },
                        );
                    }
                }
            }
        }
        // VAD hodograph: fetch when open and the site changed or the 5-min clock elapsed.
        if self.show_hodo {
            if let Some(site) = self.views[self.active].site.clone() {
                let stale = self
                    .hodo_last_fetch
                    .is_none_or(|t| t.elapsed().as_secs() >= 300);
                let site_changed = self.hodo_site.as_deref() != Some(site.as_str());
                if stale || site_changed {
                    if site_changed {
                        self.hodo_data.clear();
                    }
                    self.hodo_last_fetch = Some(Instant::now());
                    self.spawn_overlay(ctx, OverlaySource::Vwp(site));
                }
            }
        }
        self.sync_placefiles(ctx);
        self.sync_pf_icons(ctx);
        // Bindings are polled once, globally: a hotkey works the same in OBS mode, on mobile, and
        // with the drawer open. `capture_key` suppresses the table while the Hotkeys tab is
        // listening for the next keypress.
        if !self.capture_key {
            let bindings = hotkeys::active(&self.settings).into_owned();
            for action in hotkeys::poll(ctx, &bindings) {
                self.apply_action(action, ctx);
            }
        }

        self.drive_obs_tour();

        // eframe's root Ui spans the full viewport (deliberately edge-to-edge), so panels ignore
        // egui's safe area; reserve the system-bar strips ourselves. Floating windows/areas
        // constrain to content_rect natively. Zero-size off-Android (insets only fed there).
        let vr = ctx.viewport_rect();
        let cr = ctx.content_rect();
        if cr.top() > vr.top() {
            egui::Panel::top("safe_top")
                .exact_size(cr.top() - vr.top())
                .frame(egui::Frame::NONE)
                .show(root, |_| {});
        }
        if vr.bottom() > cr.bottom() {
            egui::Panel::bottom("safe_bottom")
                .exact_size(vr.bottom() - cr.bottom())
                .frame(egui::Frame::NONE)
                .show(root, |_| {});
        }
        if cr.left() > vr.left() {
            egui::Panel::left("safe_left")
                .exact_size(cr.left() - vr.left())
                .frame(egui::Frame::NONE)
                .show(root, |_| {});
        }
        if vr.right() > cr.right() {
            egui::Panel::right("safe_right")
                .exact_size(vr.right() - cr.right())
                .frame(egui::Frame::NONE)
                .show(root, |_| {});
        }

        // The tour highlights real chrome, so the rects have to come from this frame's draw.
        self.tour_anchors = Default::default();

        // Docked desktop chrome. Declared before every floating Area so those constrain to what's
        // left of the viewport (`self.chrome_rect`) instead of covering the bars.
        // Embedded panes keep their chrome: without it there is no play button, no site picker and
        // no product menu, and an iframe is exactly where a user can't reach those any other way.
        // `embed` still buys the idle heartbeat and the state postMessage.
        let bare = self.obs_mode;

        // The WSV3 ribbon layout: desktop/web only, off under OBS. Docked before `chrome_rect` is
        // read so the floating windows and the scrubber constrain to the map area between the
        // ribbon and the status bar.
        let wsv3_layout =
            !bare && !crate::platform::phone_layout() && self.settings.layout.is_ribbon();
        if wsv3_layout {
            self.wsv3_ribbon(root, ctx);
            self.wsv3_status_bar(root);
        }
        // The dock layout: its own tab row, layers tree, info panels and timeline, docked before
        // `chrome_rect` is read so the map gets what they leave. It draws none of the floating
        // chrome below (pill, column, scrubber, slide-in panel), which it replaces.
        self.window_w = ctx.content_rect().width();
        if crate::platform::phone_layout() {
            // A phone still on the old default design moves to Station, once.
            self.settings.adopt_station_default();
        }
        let phone_station = !bare && self.phone_station();
        let dock_layout = !bare && self.workstation_chrome();
        if phone_station {
            self.phone_layout(root, ctx);
        } else if dock_layout {
            self.dock_layout(root, ctx);
        }

        self.chrome_rect = root.available_rect_before_wrap();
        // In the dock, a drawer page opens inside the map, clear of the dock's own bars, rail and
        // windows; elsewhere it keeps its lane along the screen's left edge.
        ui::drawer::set_area(
            ctx,
            (dock_layout && !phone_station).then_some(self.chrome_rect),
        );
        // Before any chrome: everything below asks `motion::reduced()`, and the answer has to be
        // the same for every surface in a frame.
        ui::motion::frame(ctx, self.settings.reduce_motion);
        crate::render::mercator::set_globe(self.settings.globe);
        crate::render::mercator::set_far_cull(
            self.settings
                .hide_far_3d
                .then_some(self.settings.far_3d_factor),
        );

        // Chrome: touch-first on Android (top chips + bottom sheet + docked toolbar), desktop
        // otherwise (the floating map-first chrome below). Both funnel into the same `UiActions`
        // handling. The occlusion rects are rebuilt from scratch every frame; a stale rect would
        // keep swallowing gestures over a sheet that closed.
        self.mobile_occlusion.clear();
        if !bare {
            // The phone draws its own top strips and back wiring first, and can ask for the rest
            // to be skipped entirely (the hide-all-chrome eye). Desktop draws the window frame
            // first instead: its drag strip covers the top edge, and everything after it takes
            // back the clicks that land on an actual control.
            let chrome = if crate::platform::phone_layout() {
                self.mobile_chrome(ctx)
            } else {
                // A tablet draws this same desktop chrome but is still an Android app: Back has
                // to keep closing the window on top before it leaves.
                if cfg!(target_os = "android") {
                    self.android_back(ctx);
                }
                // The workstation's app bar is its own caption (drawn above, so a strip now
                // would cover its tabs).
                self.window_frame(ctx, !dock_layout);
                true
            };
            if chrome {
                self.sync_permalink();
                // WSV3 layout: the ribbon (already drawn above) replaces the search pill, the
                // right-edge control column and the pane strip. Everything else — the timeline
                // scrubber, the layers/basemap panels the ribbon opens, the corner chips — is
                // shared with the minimal layout.
                if !wsv3_layout && !dock_layout {
                    self.search_pill(ctx);
                    self.control_column(ctx);
                }
                if wsv3_layout {
                    self.wsv3_timestamp(ctx);
                }
                if phone_station {
                    self.phone_overlay(ctx);
                } else if dock_layout {
                    self.dock_map_overlay(ctx);
                }
                if !dock_layout {
                    self.scrubber(ctx);
                }
                if !wsv3_layout && !dock_layout {
                    self.pane_strip(ctx);
                }
                if !dock_layout {
                    self.panel(ctx);
                }
                self.basemap_panel(ctx);
                self.storm_track_card(ctx);
                self.info_chip(ctx);
                self.error_chip(ctx);
                self.update_chip(ctx);
                self.quality_chip(ctx);
            }
        }

        // The one decoration with no job. Costs nothing after the first second and a half, and
        // never runs at all under OBS — a capture is not a place for a flourish.
        if !bare {
            ui::motion::intro(
                ctx,
                self.chrome_rect,
                crate::theme::accent(self.settings.theme),
            );
        }

        // Over everything, and only while a capture is pending.
        self.share_card_footer(ctx);

        // The spotlight tour, over the chrome it points at. Paused under the first-run card and
        // the cheat sheet — all three draw on `Order::Foreground` and would fight for it.
        if self.tour.open && !self.firstrun.open && !self.show_cheatsheet {
            let v = &self.views[self.active];
            let sig = ui::tour::Signals {
                moment: v.moment,
                srv: v.srv,
                playhead: v.timeline.playhead,
                following: v.timeline.following,
            };
            let accent = crate::theme::accent(self.settings.theme);
            self.tour.advance_if_done(sig);
            let anchors = self.tour_anchors;
            self.tour.show(ctx, &anchors, sig, accent);
        }

        // One quiet check per session, once the app has settled — so a stale build tells you so
        // without anyone opening About.
        if ctx.input(|i| i.time) > 30.0 {
            self.check_for_update(ctx);
        }
        while let Ok(state) = self.update_rx.try_recv() {
            self.update_state = state;
        }
        if self.about_open {
            let accent = crate::theme::accent(self.settings.theme);
            let mut open = self.about_open;
            ui::about_window::show(ctx, &mut open, &self.update_state, accent, &mut self.drawer);
            self.about_open = open;
        }

        // The `?` cheat sheet floats over everything, including the first-run card.
        if self.show_cheatsheet {
            let entries = self.palette_entries();
            let bindings = hotkeys::active(&self.settings).into_owned();
            let accent = crate::theme::accent(self.settings.theme);
            self.show_cheatsheet = ui::cheatsheet::show(ctx, &bindings, &entries, accent);
        }

        // First run: pick a radar (or let the location do it) and get out of the way.
        if let Some(fin) = ui::firstrun::show(ctx, &mut self.firstrun, &mut self.settings) {
            self.settings.setup_done = true;
            self.settings.save();
            let v = &mut self.views[self.active];
            v.site = Some(fin.site.clone());
            ui::site_dialog::center_on_site(&mut v.camera, &fin.site);
            if fin.located {
                // Nobody chose this site, so say which one it is and that it can be changed.
                self.toast(
                    ToastKind::Info,
                    format!(
                        "Nearest radar: {} — change it any time in the panel",
                        fin.site
                    ),
                );
            }
            if fin.take_tour {
                self.tour.start();
            }
        }
        if !self.firstrun.open && !self.settings.setup_done {
            // Dismissed without finishing. Setup is optional and re-runnable from three places,
            // so take the ✕ at its word rather than reopening this on every launch.
            self.settings.setup_done = true;
            self.settings.save();
        }

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
        // Only the open settings window reads these; building the registry for a closed window
        // was a few hundred String allocations every frame.
        let entries = if self.settings_window.open {
            self.palette_entries()
        } else {
            std::sync::Arc::from(Vec::new())
        };
        let sync_view = ui::settings_window::SyncView {
            signed_in: self.sync_tokens.is_some(),
            status: &self.sync_status,
            login_url: self.sync_login.as_ref().map(|p| p.url.as_str()),
            last_sync: self.sync_state.last_sync,
        };
        let sync_action = self.settings_window.show(
            ctx,
            &mut self.settings,
            &self.palettes,
            sync_view,
            &entries,
            &mut self.drawer,
        );
        self.capture_key = self.settings_window.capturing;
        if std::mem::take(&mut self.settings_window.run_setup) {
            self.firstrun.start();
        }
        if std::mem::take(&mut self.settings_window.run_tour) {
            self.tour.start();
        }
        match sync_action {
            Some(ui::settings_window::SyncAction::SignIn) => self.sync_sign_in(),
            Some(ui::settings_window::SyncAction::SignOut) => self.sync_sign_out(),
            Some(ui::settings_window::SyncAction::SyncNow) => self.sync_now(),
            None => {}
        }
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
        self.udp_window.show(
            ctx,
            &mut self.settings,
            &mut self.drawer,
            &mut self.views[active].user_product,
        );
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
        let label_keys = if self.layer_window_open && self.settings.imported_gis.is_some() {
            crate::gis_import::label_keys(&self.imported_marks)
        } else {
            Vec::new()
        };
        let legend = self.imported_colors.as_ref().map(|(_, _, l)| l.clone());
        let time_count = self
            .imported_shown
            .as_ref()
            .map(|m| (m.iter().filter(|&&s| s).count(), m.len()));
        let imported = ui::layer_window::Imported {
            keys: &label_keys,
            legend: legend.as_ref(),
            time_count,
        };
        if ui::layer_window::show(
            ctx,
            &mut self.layer_window_open,
            &mut self.settings,
            &active_fields,
            &imported,
            &mut self.drawer,
        ) {
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
        // Storm digest: poll a pending Claude result, then render + handle Generate.
        if let Some(rx) = &self.digest_rx {
            if let Ok(res) = rx.try_recv() {
                self.digest_window.busy = false;
                self.digest_rx = None;
                match res {
                    Ok(text) => {
                        self.digest_window.text = text;
                        self.digest_window.enhanced = true;
                    }
                    Err(e) => log::warn!("digest enhancement failed: {e}"),
                }
            }
        }
        if let Some(ui::digest_window::DigestAction::Generate) =
            self.digest_window.show(ctx, &mut self.drawer)
        {
            self.generate_digest();
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
        if let Some(id) = ui::cells_window::show(
            &mut self.cells_window,
            ctx,
            cells,
            &cell_scores,
            &cell_explanations,
            &zdr_cells,
            &self.cell_trends,
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
            let trend = self
                .cell_trends
                .get(&cell.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
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
        if let (Some(xs), Some(tex)) = (&self.xsection, &self.xsection_tex) {
            let mut moment = self.xsection_moment;
            let open = ui::xsection_window::show(
                ctx,
                xs,
                tex,
                &mut moment,
                &mut self.xsection_beam_rise,
                &mut self.drawer,
            );
            if !open {
                self.xsection = None;
                self.xsection_tex = None;
                self.xsection_pts.clear();
            } else if moment != self.xsection_moment {
                self.xsection_moment = moment;
                let idx = self.active;
                self.build_xsection(idx, ctx);
            }
        }
        // The workstation shows the volume in its 3D volume tool window (`chrome/dock/volume.rs`).
        if self.show_3d {
            // Rebuilds only when the volume, its tilt count or the tilts pulled out change.
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
            let entries = self.palette_entries();
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
            ui::analyst_log_window::show(ctx, &mut self.settings, &mut self.drawer);
        }
        self.show_warning_banners(ctx);
        self.show_toasts(ctx);

        // Turn this frame's UI mutations into uploads/fetches before painting the map.
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
        self.sync_imported_time();
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

        // Streaming mode's broadcast dressing: clock, caption, crawl, logo.
        self.broadcast_dressing(root);
        // OBS-mode hint so the chrome-free view is still escapable. Top centre: the corners are
        // the dressing's.
        if self.obs_mode {
            egui::Area::new("obs_hint".into())
                .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 10.0))
                .interactable(false)
                .show(root, |ui| {
                    let txt = if self.obs_tour {
                        "OBS · tour (F8 exit · F9 stop tour)"
                    } else {
                        "OBS mode (F8 exit · F9 tour)"
                    };
                    egui::Frame::new()
                        .fill(egui::Color32::from_black_alpha(150))
                        .corner_radius(4.0)
                        .inner_margin(egui::Margin::symmetric(8, 4))
                        .show(ui, |ui| {
                            ui.colored_label(egui::Color32::from_white_alpha(200), txt)
                        });
                });
        }

        let placefile_labels = self.placefile_labels_cached();
        // One occupancy set for the whole frame, across every pane and every label layer. Panes
        // occupy disjoint screen rects, so sharing it between them costs nothing and saves
        // resetting it per pane.
        self.labels.begin();
        egui::CentralPanel::default().show(root, |ui| {
            let full = ui.available_rect_before_wrap();
            let n = self.views.len();
            // A phone shows one pane at a time. Two 400x400 pt panes stacked is two views of
            // nothing; the pane strip above the scrubber is how you get to the others.
            // Only where one pane is all that fits: a tablet shows the split.
            let solo = cfg!(target_os = "android") && n > 1 && chrome::compact(ctx);
            let rects = if solo {
                vec![full; n]
            } else {
                arranged_pane_rects(full, n, self.pane_layout)
            };

            self.step_camera_flights(&rects, ctx); // jumps fly there (ROADMAP_2 §4.4)
                                                   // If cameras are linked, mirror the active pane's camera to the others.
            if self.link_cameras {
                let cam = self.views[self.active.min(n - 1)].camera;
                for v in &mut self.views {
                    v.camera = cam;
                }
            }

            // Each pane fetches and draws its own `View::basemap` (see `render_pane`). Two things
            // stay global for now: the GOES frame cursor and the vector palette, both driven by
            // the active pane.
            // ponytail: one GOES cursor + one vector palette for all panes; split them when
            // someone actually wants two satellite times or two vector palettes side by side.
            use crate::tiles::BasemapStyle;
            let style = self.views[self.active.min(n - 1)]
                .basemap
                .resolve(ctx.theme() == egui::Theme::Dark);
            let is_vector = style.vector_palette().is_some();
            let raster_style = if style.is_raster() {
                style
            } else {
                BasemapStyle::None
            };
            self.tiles
                .set_keys(&self.settings.mapbox_key, &self.settings.maptiler_key);
            self.tiles
                .set_custom_template(&self.settings.custom_tile_url);
            self.tiles.set_custom_max_z(self.settings.custom_tile_max_z);
            // Ask for `@2x` tiles where the provider serves them: same tile count, twice the
            // pixels, labels drawn for the density instead of magnified. Off on a metered link —
            // a double-resolution tile is roughly double the bytes.
            let mut clear_tiles = self
                .tiles
                .set_retina(ctx.pixels_per_point() > 1.0 && !crate::platform::is_metered());
            // GOES sub-hourly scrub: fetch the available frame times when a GOES style becomes
            // active, and apply the selected frame (None = latest).
            if raster_style.timed() {
                // Which hour of imagery to ask for: the pane's own clock when it is replaying an
                // archive, otherwise the live window ending now. GIBS keeps GeoColor about two
                // weeks and Band 13 several months, so a replayed event usually has satellite.
                let radar_time = self.linked_analysis_time().or_else(|| {
                    self.views[self.active.min(n - 1)]
                        .volume
                        .as_ref()
                        .map(|v| v.time)
                });
                let (hour, from, to) = crate::tiles::goes_window(chrono::Utc::now(), radar_time);
                if self.goes_times_style != Some(raster_style) || self.goes_hour != hour {
                    self.goes_times_style = Some(raster_style);
                    self.goes_hour = hour;
                    self.goes_times.clear();
                    self.goes_time_idx = None;
                    let (tx, rx) = std::sync::mpsc::channel();
                    self.goes_times_rx = Some(rx);
                    let http = self.http.clone();
                    self.spawner.spawn(async move {
                        let times =
                            crate::tiles::fetch_frame_times(&http, raster_style, from, to, 48)
                                .await;
                        let _ = tx.send(times);
                    });
                }
                // Following the radar is the default: scrubbing back through an event should
                // take the satellite back with it, which is the whole reason to look at both.
                // Stepping the GOES arrows by hand drops out of it.
                let selected = if self.goes_follow_radar {
                    radar_time.and_then(|t| nearest_goes(&self.goes_times, t))
                } else {
                    self.goes_time_idx
                        .and_then(|i| self.goes_times.get(i).copied())
                };
                // `None` means GIBS's own default, which is the newest imagery there is — right
                // for a live pane, three days wrong for one replaying an archive. In an archive
                // window, fall back to the newest frame *in that window* instead.
                let selected = match (selected, self.goes_hour) {
                    (None, Some(_)) => self.goes_times.last().copied(),
                    (s, _) => s,
                };
                clear_tiles |= self.tiles.set_goes_time(selected);
            } else if self.goes_times_style.is_some() {
                self.goes_times_style = None;
                self.goes_hour = None;
                self.goes_times.clear();
                clear_tiles |= self.tiles.set_goes_time(None);
            }
            let mut clear_vector = false;
            if is_vector {
                clear_vector |= self
                    .vtiles
                    .set_style(style.vector_palette().unwrap_or_default());
                clear_vector |= self.vtiles.set_theme(self.settings.theme);
            }
            self.last_viewport = rects
                .get(self.active)
                .map_or((full.width(), full.height()), |r| (r.width(), r.height()));

            // Which pane carries the once-per-frame work (tile-cache clears, the shared label
            // pass): the first one actually drawn, which under `solo` is the active one.
            let head = if solo { self.active.min(n - 1) } else { 0 };
            // Cleared before every pane gets a chance to re-set it this frame (`render_pane`'s
            // hover block does, when `link_cursor` is on): leaving the map area with no pane
            // hovered must drop the shared point rather than leave the last one drawn everywhere.
            self.linked_probe = None;
            self.follow_linked_storm();
            for (i, prect) in rects.iter().enumerate() {
                if solo && i != head {
                    continue;
                }
                let first = i == head;
                self.render_pane(
                    ui,
                    ctx,
                    i,
                    *prect,
                    clear_tiles && first,
                    clear_vector && first,
                    first,
                    solo || i + 1 == n,
                    &placefile_labels,
                );
            }

            self.paint_storm_tracks(ui, &rects);
            self.paint_scale_bar(ui, &rects);
            self.paint_linked_time_badges(ui, &rects, solo);
            self.paint_linked_cursor(ui, &rects, solo);
            self.paint_layer_probe(ui, &rects);
            self.paint_selected_storm(ui, &rects, solo);

            // Pane borders; the active pane gets an accent outline. Nothing to outline under
            // `solo` — there is one pane on screen and the strip says which.
            if n > 1 && !solo {
                for (i, prect) in rects.iter().enumerate() {
                    let (w, col) = if i == self.active {
                        (2.0, crate::theme::accent(self.settings.theme))
                    } else {
                        (1.0, egui::Color32::from_gray(60))
                    };
                    ui.painter().rect_stroke(
                        *prect,
                        0.0,
                        egui::Stroke::new(w, col),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        });

        // Dirty-diff persistence: one write per actual change, from any mutation site. The
        // comparison walks the whole settings tree (palettes, placefiles, markers), so it runs at
        // most once a second rather than every frame; a change waits under a second to reach disk.
        let due = self
            .settings_checked
            .is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(1));
        if due {
            self.settings_checked = Some(Instant::now());
            // Fold the live overlay toggles into the settings so the diff below persists them
            // like any other change — no separate save path, no per-frame churn.
            let on: Vec<String> = OverlayToggle::ALL
                .into_iter()
                // Camera linking is a session decision about the panes on screen, not a layer.
                .filter(|t| !t.session_only() && *self.overlay_flag(*t))
                .map(|t| t.slug())
                .collect();
            self.settings.overlays_on = Some(on);
            // And the model contours: they were the one kind of layer a restart forgot.
            self.settings.contours_on = self
                .active_contours
                .iter()
                .filter_map(|k| k.token())
                .map(str::to_string)
                .collect();
            // Same trick for the window: fold the live size in, and the ordinary diff-and-save
            // below persists it.
            //
            // Measured from the root `Ui`, not `ViewportInfo::inner_rect`: on Wayland the
            // compositor never tells a client where its window is, so `inner_rect` is `None`
            // there and this silently saved nothing at all. The root ui covers the whole
            // viewport, in egui points — logical points divided by the ui-scale zoom — so
            // multiplying the zoom back gives the units `with_inner_size` wants.
            //
            // While maximized the size on screen is the screen's, not the one to restore to, so
            // the previous size is kept and only the flag moves.
            #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
            {
                let (maximized, minimized) = root.ctx().input(|i| {
                    (
                        i.viewport().maximized.unwrap_or(false),
                        i.viewport().minimized.unwrap_or(false),
                    )
                });
                let size = root.max_rect().size() * root.ctx().zoom_factor();
                if !minimized && size.x > 1.0 && size.y > 1.0 {
                    let (width, height) = match self.settings.window {
                        Some(w) if maximized => (w.width, w.height),
                        _ => (size.x, size.y),
                    };
                    self.settings.window = Some(crate::settings::WindowGeom {
                        width,
                        height,
                        maximized,
                    });
                }
            }
        }
        if due && self.settings != self.saved {
            // A palette-map change reloads the color tables (bumps gen -> LUT re-bake).
            if self.settings.palettes != self.saved.palettes {
                self.palettes.reload(&self.settings.palette_paths());
            }
            self.settings.save();
            self.saved = self.settings.clone();
        }

        // Embedded in another page: hand the active pane's state to the parent frame so it can
        // persist it. Our own localStorage is partitioned (or wiped) inside a third-party iframe,
        // so the host is the only place this survives a reload. Same once-a-second tick as above,
        // and only when something actually moved.
        #[cfg(target_arch = "wasm32")]
        if due && self.embed {
            self.post_state_to_parent();
        }

        // Android text input: summon/dismiss the soft keyboard as egui focus moves in/out of
        // text fields, and float a Paste button (the system clipboard is unreachable from the
        // soft keyboard otherwise — egui gets the text as a Paste event next frame).
        if cfg!(target_os = "android") {
            // Only a text field wants the soft keyboard. Any focused widget used to count, so
            // tapping a checkbox raised the keyboard and reset the IME buffer under the field.
            let wants = ctx.text_edit_focused();
            if wants != self.ime_shown {
                crate::platform::show_soft_input(wants);
                self.ime_shown = wants;
            }
            if wants {
                egui::Area::new(egui::Id::new("android_paste_bar"))
                    .anchor(egui::Align2::RIGHT_TOP, [-8.0, 64.0])
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            if ui.button("Paste").clicked() {
                                self.pending_paste = crate::platform::clipboard_text();
                                // Remember the field losing focus to this tap, to restore it next
                                // frame so the Paste event has somewhere to land.
                                self.paste_target = ui.ctx().memory(|m| m.focused());
                            }
                        });
                    });
            }
        }

        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        self.mini_loop_viewport(ctx);
        self.output_window(ctx);

        self.crash_report_window(ctx);

        // Unconditional, every frame, regardless of which (if any) drawer-hosted page ran above:
        // the self-pruning inside `Drawer::page`/`page_sized` only fires when *some* page calls
        // it, which is exactly the property the empty case doesn't have. Without this, closing
        // the last open page (Sounding, Settings, X-section, …) left the drawer's stack holding
        // its title forever, `Drawer::is_open()` reading true forever, and the floating
        // layers/alerts panel — which steps aside whenever a page is open — hidden until restart.
        self.drawer.end_frame(ctx);

        // Idle heartbeat so clocks (volume age, countdowns) tick without input. Data arrivals and
        // animations (pulse, banners) request faster repaints on their own. Slower on Android to
        // spare the battery — nothing on screen changes faster than this between frames.
        // An untouched embed is a still picture on someone else's dashboard: one frame a minute
        // keeps the clocks honest without spending the host's CPU. The first interaction wakes it
        // for good; data arrivals still request their own repaints either way.
        let busy = ctx.input(|i| !i.events.is_empty() || i.pointer.any_down() || i.any_touches());
        if busy {
            self.last_input = Instant::now();
        }
        if self.embed && !self.embed_live && ctx.input(|i| i.pointer.any_down() || i.any_touches())
        {
            self.embed_live = true;
        }
        let idle = if self.embed && !self.embed_live {
            60_000
        } else if !crate::platform::activity::is_active() {
            2_000 // backgrounded: just enough to notice coming back
        } else if self.settings.battery_saver {
            // Four frames a second is still a live clock; it is not a live animation. Anything
            // that actually moves (a banner, a play head, an arriving volume) asks for its own
            // repaint and is unaffected.
            1_000
        } else if self.last_input.elapsed() > QUIET_AFTER {
            // Nobody has touched it for a while. This is a floor, not a schedule: egui takes the
            // *minimum* of every repaint request in a frame, so playback, the warning pulse, the
            // wind field and every arriving volume all still outbid it and animate at their own
            // rate. What it changes is the cost of a window nothing is happening in — ten wasted
            // full passes a second, each re-walking thirty poll clocks and re-projecting every
            // label, becomes two. The first event of any kind snaps it back the same frame.
            //
            // The visible price is that a clock can read up to 0.4 s stale in a still window.
            IDLE_QUIET_MS
        } else if cfg!(target_os = "android") {
            250
        } else {
            100
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.perf.idle_ms = idle;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(idle));
    }
}

/// How many held-back pushes the quiet-hours queue keeps. A long quiet window over a big outbreak
/// can queue hundreds; the summary only names a handful anyway.
const QUIET_QUEUE_MAX: usize = 50;

/// Fold the pushes quiet hours held back into one catch-up notification: `(title, body)`.
///
/// Named headlines are capped so the body stays inside what a phone push will show; the rest are
/// counted.
fn quiet_summary(held: &[(String, String)]) -> (String, String) {
    const NAMED: usize = 4;
    let title = if held.len() == 1 {
        "1 alert while you were away".to_string()
    } else {
        format!("{} alerts while you were away", held.len())
    };
    let mut body: String = held
        .iter()
        .take(NAMED)
        .map(|(t, _)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if held.len() > NAMED {
        body.push_str(&format!("\n(+{} more)", held.len() - NAMED));
    }
    (title, body)
}

#[cfg(test)]
mod quiet_summary_tests {
    use super::quiet_summary;

    fn held(n: usize) -> Vec<(String, String)> {
        (0..n)
            .map(|i| (format!("alert {i}"), "body".to_string()))
            .collect()
    }

    #[test]
    fn one_alert_reads_singular() {
        let (title, body) = quiet_summary(&held(1));
        assert_eq!(title, "1 alert while you were away");
        assert_eq!(body, "alert 0");
    }

    #[test]
    fn many_alerts_name_a_few_and_count_the_rest() {
        let (title, body) = quiet_summary(&held(9));
        assert_eq!(title, "9 alerts while you were away");
        assert!(body.starts_with("alert 0\nalert 1\nalert 2\nalert 3\n"));
        assert!(body.ends_with("(+5 more)"));
    }

    #[test]
    fn exactly_the_named_count_has_no_tail() {
        let (_, body) = quiet_summary(&held(4));
        assert!(!body.contains("more"));
    }
}

#[cfg(test)]
mod humanize_tests {
    use super::humanize;

    #[test]
    fn ages_stay_readable_all_the_way_back_to_the_archive() {
        assert_eq!(humanize(45), "45s");
        assert_eq!(humanize(600), "10m");
        assert_eq!(humanize(3 * 3600 + 20 * 60), "3h20m");
        // Past a couple of days, hours stop meaning anything to a reader.
        assert_eq!(humanize(5 * 86_400), "5d");
        // Moore 2013, seen from 2026 — used to render as "113000h40m ago".
        assert_eq!(humanize(13 * 365 * 86_400), "13.0y");
    }
}

#[cfg(test)]
mod place_name_tests {
    use super::short_place_name;

    #[test]
    fn keeps_the_place_and_drops_the_administrative_tail() {
        // What Nominatim actually returns for a town query.
        assert_eq!(
            short_place_name("Norman, Cleveland County, Oklahoma, United States"),
            "Norman"
        );
        // Already short, or oddly shaped: pass it through rather than blanking the name.
        assert_eq!(short_place_name("Dallas"), "Dallas");
        assert_eq!(short_place_name(""), "");
        assert_eq!(short_place_name("  Tulsa , Oklahoma"), "Tulsa");
    }
}

#[cfg(test)]
mod follow_tests {
    use super::{nearest_cell, widget_storm_line};
    use wxdata::level3::Cell;

    fn cell(id: &str, lon: f64, lat: f64) -> Cell {
        Cell {
            id: id.into(),
            lon,
            lat,
            ..Default::default()
        }
    }

    #[test]
    fn the_widget_line_reads_from_where_you_are() {
        // A cell due west of the reader, moving northeast at 30 kt.
        let mut c = cell("R3", -97.72, 35.30);
        c.mvt_deg = Some(45.0);
        c.mvt_kt = Some(30.0);
        let line = widget_storm_line(&[c.clone()], -97.5, 35.3, false).unwrap();
        // West of you: the compass point names the side the storm is on, not the bearing to it.
        assert!(line.starts_with("R3 12 mi W"), "{line}");
        assert!(line.ends_with("moving NE 35 mph"), "{line}");
        // Same cell on a German pane: the distance turns over, the speed does not.
        let km_line = widget_storm_line(&[c], -97.5, 35.3, true).unwrap();
        assert!(km_line.starts_with("R3 20 km W"), "{km_line}");
        // Nothing within 300 km is no line at all.
        assert!(widget_storm_line(&[cell("Z1", -80.0, 35.3)], -97.5, 35.3, false).is_none());
        assert!(widget_storm_line(&[], -97.5, 35.3, false).is_none());
    }

    #[test]
    fn nearest_cell_within_radius_and_none_outside() {
        // Three cells around a predicted point near (−97.5, 35.3).
        let cells = vec![
            cell("A7", -97.60, 35.30), // ~9 km west
            cell("B3", -97.51, 35.31), // ~1.5 km — the nearest
            cell("", -97.505, 35.305), // closest of all but no SCIT id → ineligible
        ];
        let got = nearest_cell(&cells, -97.5, 35.3, 15.0).unwrap();
        assert_eq!(got.id, "B3");
        // A prediction far from every cell (radius exceeded) → nothing to adopt.
        assert!(nearest_cell(&cells, -90.0, 30.0, 15.0).is_none());
    }
}

#[cfg(test)]
mod warning_scope_tests {
    use super::{feature_in_box, GeoFeature};
    use wxdata::overlay::FeatureKind;

    fn poly(x0: f64, y0: f64, x1: f64, y1: f64) -> GeoFeature {
        GeoFeature {
            rings: vec![vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]],
            fill: [0; 4],
            stroke: [0; 4],
            kind: FeatureKind::Warning,
            title: String::new(),
            detail: String::new(),
            alert: None,
        }
    }

    #[test]
    fn feature_in_box_overlap() {
        // Box roughly around KFWS (Dallas): lon -97.3, lat 32.6, ±2.25°.
        let bx = (-99.55, 30.35, -95.05, 34.85);
        // A warning polygon overlapping the box.
        assert!(feature_in_box(&poly(-98.0, 32.0, -97.0, 33.0), bx));
        // A warning far away (Mississippi) — no overlap.
        assert!(!feature_in_box(&poly(-90.0, 32.0, -89.0, 33.0), bx));
        // Touching the edge counts as overlap.
        assert!(feature_in_box(&poly(-95.05, 32.0, -94.0, 33.0), bx));
        // Empty geometry never overlaps.
        let mut empty = poly(0.0, 0.0, 0.0, 0.0);
        empty.rings.clear();
        assert!(!feature_in_box(&empty, bx));
    }
}

#[cfg(test)]
mod field_lut_tests {
    use super::{categorical_lut, distinct_tilts, glm_style, ramp_lut, ramp_lut_a, windy_url};

    #[test]
    fn distinct_tilts_skips_sails_repeats() {
        // A VCP 212 style list: 0.5 appears three times (SAILS), 0.9 twice (MRLE).
        let els = [0.5, 0.5, 0.9, 0.5, 0.9, 1.3, 1.8, 2.4];
        let picks = distinct_tilts(&els, 4);
        assert_eq!(
            picks,
            vec![0, 2, 5, 6],
            "one index per distinct angle, lowest first"
        );
        let angles: Vec<f32> = picks.iter().map(|&i| els[i]).collect();
        assert_eq!(angles, vec![0.5, 0.9, 1.3, 1.8]);
    }

    #[test]
    fn distinct_tilts_clamps_to_what_exists() {
        assert_eq!(distinct_tilts(&[0.5, 0.5], 4), vec![0]);
        assert!(distinct_tilts(&[], 4).is_empty());
    }

    #[test]
    fn windy_url_puts_latitude_first() {
        // KTLX, zoom 7.4. Windy wants lat,lon — this codebase says (lon, lat) everywhere else,
        // so a swap here would silently send people to the Indian Ocean.
        let u = windy_url("radar", -97.3, 35.4, 7.4);
        assert_eq!(u, "https://www.windy.com/?radar,35.400,-97.300,7");
        // Decimals are mandatory: Windy ignores a whole-number coordinate.
        assert!(windy_url("wind", -97.0, 35.0, 5.0).contains("35.000,-97.000"));
        // Zoom is clamped into Windy's own range rather than passed through.
        assert!(windy_url("wind", 0.0, 0.0, 2.0).ends_with(",3"));
        assert!(windy_url("wind", 0.0, 0.0, 18.9).ends_with(",18"));
    }

    #[test]
    fn glm_flashes_fade_from_white_hot_to_ember() {
        let (fresh, r_fresh) = glm_style(0.0);
        let (old, r_old) = glm_style(900.0);
        assert_eq!(fresh.a(), 255, "a brand-new flash is fully opaque");
        assert!(old.a() < 100, "a 15-minute-old flash is nearly gone");
        assert!(r_fresh > r_old, "newest flashes draw largest");
        // Past the window the style clamps rather than inverting.
        assert_eq!(glm_style(5000.0), glm_style(900.0));
        // And it warms as it ages: green drops faster than red.
        assert!(old.g() < fresh.g() && old.r() <= fresh.r());
    }

    #[test]
    fn categorical_lut_sets_only_listed_slots() {
        let lut = categorical_lut(&[(1, [10, 20, 30]), (7, [200, 40, 40])], 200);
        assert_eq!(lut.len(), 256 * 4);
        // Index 0 clear.
        assert_eq!(&lut[0..4], &[0, 0, 0, 0]);
        // Index 1 set with alpha 200.
        assert_eq!(&lut[4..8], &[10, 20, 30, 200]);
        // Index 7 set.
        assert_eq!(&lut[28..32], &[200, 40, 40, 200]);
        // An unlisted index stays clear.
        assert_eq!(&lut[8..12], &[0, 0, 0, 0]);
    }

    #[test]
    fn ramp_lut_alpha_variants() {
        let opaque = ramp_lut(&[(0.0, [0, 0, 0]), (1.0, [255, 255, 255])]);
        assert_eq!(opaque[255 * 4 + 3], 255, "top index opaque");
        assert_eq!(opaque[3], 0, "index 0 clear");
        let translucent = ramp_lut_a(&[(0.0, [0, 0, 0]), (1.0, [255, 255, 255])], 150);
        assert_eq!(translucent[255 * 4 + 3], 150, "top index uses given alpha");
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn site_id_round_trips_through_the_palette_action_buffer() {
        use super::{decode_site_id, encode_site_id};
        for id in ["KTLX", "TADW", "DEAS", "BEHEL"] {
            assert_eq!(decode_site_id(encode_site_id(id)), id);
        }
    }

    #[test]
    fn an_id_shorter_than_the_buffer_does_not_carry_trailing_junk() {
        use super::{decode_site_id, encode_site_id};
        // The buffer is zero-padded; a 4-character id must not decode with the 4 trailing zero
        // bytes read back as anything other than "the string ends here".
        assert_eq!(decode_site_id(encode_site_id("KTLX")).len(), 4);
    }

    /// ROADMAP_2 §7.5: this file only gets smaller. A new feature belongs in its own module under
    /// `app/`; when an extraction lands, lower the ceiling to the new length so it stays down.
    #[test]
    fn app_rs_only_gets_smaller() {
        const CEILING: usize = 21117;
        let lines = include_str!("app.rs").lines().count();
        assert!(
            lines <= CEILING,
            "app.rs grew to {lines} lines (ceiling {CEILING}): put the new code in a module under app/ instead"
        );
    }

    #[test]
    fn a_source_says_how_it_recovers() {
        let h = super::SourceHealth {
            source: "Weather alerts".into(),
            endpoint_family: crate::source_health::EndpointFamily::NwsApi,
            latest_valid_time: None,
            fallback_providers: Vec::new(),
            cache_state: super::CacheState::Memory,
            fetching: false,
            last_attempt: None,
            last_success: None,
            last_failure: None,
            error: None,
            cadence: std::time::Duration::from_secs(120),
            recent_outcomes: None,
            details: Vec::new(),
            severity: crate::source_health::FeedSource::WeatherAlerts.severity(),
        };
        let r = h.recovery();
        assert!(r.contains("Retried every 2m"), "{r}");
        assert!(r.contains("stale after 4m"), "{r}");
        assert!(r.contains("Severity: critical"), "{r}");
    }

    #[test]
    fn wheel_and_trackpad_zoom_count_as_live_gestures() {
        let mut input = egui::InputState::default();
        assert!(!super::map_gesture_live(&input));
        input.smooth_scroll_delta = egui::vec2(0.0, 0.25);
        assert!(
            super::map_gesture_live(&input),
            "include the smoothed wheel tail"
        );
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            events: vec![egui::Event::Zoom(1.1)],
            ..Default::default()
        };
        let _ = ctx.run_ui(raw, |ui| {
            assert!(
                ui.input(super::map_gesture_live),
                "trackpad pinch without a button"
            );
        });
    }

    /// New geometry appears when it lands; a zoom-bucket crossing waits for the finger to lift,
    /// so a pinch across three buckets tessellates once instead of three times.
    #[test]
    fn a_pinch_defers_the_retessellation_but_new_geometry_never_waits() {
        use super::should_retess;
        assert!(
            should_retess(false, false, true),
            "quiet: rebuild on a bucket change"
        );
        assert!(!should_retess(true, false, true), "mid-gesture: wait");
        assert!(
            should_retess(true, true, true),
            "new geometry mid-gesture is data arriving, not the camera moving"
        );
        assert!(!should_retess(true, false, false));
        assert!(
            !should_retess(false, false, false),
            "nothing changed, nothing to do"
        );
    }

    /// A palette change must not re-send the sweep. Everything the GPU keeps (the gate bytes,
    /// the precipitation-flag grid) is left out of a LUT-only upload; the color table and the
    /// uniform, which are what a re-bake changes, are still there in full.
    #[test]
    fn a_palette_change_uploads_the_table_and_nothing_else() {
        let sweep = wxdata::level2::BinnedSweep {
            moment: wxdata::level2::Moment::Reflectivity,
            az_bins: 360,
            gate_count: 200,
            data: vec![7u8; 360 * 200],
            first_gate_km: 2.125,
            gate_interval_km: 0.25,
            radar_lat: 35.0,
            radar_lon: -97.0,
            elevation_deg: 0.5,
            value_min: -32.0,
            value_max: 95.0,
            ..Default::default()
        };
        let table = crate::colormap::default_table(wxdata::level2::Moment::Reflectivity);
        let full = super::to_upload(&sweep, table, None, false, None, None, false, None);
        let lut = super::to_upload(&sweep, table, None, false, None, None, true, None);
        assert_eq!(full.data.len(), 360 * 200);
        assert!(lut.data.is_empty(), "the gate texture is already uploaded");
        assert_eq!(lut.lut, full.lut, "the color table is what changed");
        assert_eq!(lut.uniform, full.uniform);
        assert_eq!((lut.az_bins, lut.gate_count), (360, 200));
        assert!(lut.lut_only);
    }

    /// The DWD arm exists because DL-DE/BY-2.0 requires the credit; the NOAA sites must stay
    /// uncredited so the corner is empty on the maps most users open.
    #[test]
    fn only_licences_that_ask_for_a_credit_get_one() {
        assert_eq!(
            super::data_attribution("DEBO"),
            Some("Radar data © Deutscher Wetterdienst (DL-DE/BY-2.0)")
        );
        assert_eq!(super::data_attribution("KTLX"), None);
    }

    /// Scrubbing reverses constantly, so the paused set has to reach backwards as well; playback
    /// only ever goes forward. Nearest frames come first either way, because the in-flight budget
    /// usually runs out before the list does.
    #[test]
    fn prefetch_reaches_backwards_only_when_paused() {
        let playing = super::prefetch_offsets(true);
        assert!(playing.iter().all(|d| *d > 0), "playback never looks back");
        let paused = super::prefetch_offsets(false);
        assert!(paused.contains(&-1), "scrubbing back a frame must be warm");
        assert_eq!(paused[0], 1, "nearest frame first");
        for set in [playing, paused] {
            let mut sorted = set.to_vec();
            sorted.sort_by_key(|d| d.abs());
            assert_eq!(set, sorted.as_slice(), "nearest-first order");
        }
    }

    #[test]
    fn goto_extras_carry_basemap_and_srv() {
        // New extras, in either order, alongside the old ones.
        let g = super::parse_goto("KTLX,-97.3,35.3,6.5,bm:dark,srv,VEL,2").unwrap();
        assert_eq!(g.site, "KTLX");
        assert_eq!(g.basemap.as_deref(), Some("dark"));
        assert!(g.srv);
        assert_eq!(g.tilt, Some(2));
        let round = super::parse_goto(&super::goto_link(&g)).unwrap();
        assert_eq!(round.basemap, g.basemap);
        assert_eq!(round.srv, g.srv);
        assert_eq!(round.moment, g.moment);
        assert_eq!(round.tilt, g.tilt);
        // Gauges ride along, validated, and come back from the link they went into.
        let g = super::parse_goto("KGRK,-97.7,30.2,9,gauge:acrt2,gauge:../x,gauge:BRTT2").unwrap();
        assert_eq!(g.gauges, ["ACRT2", "BRTT2"]);
        let link = super::goto_link(&g);
        assert!(link.ends_with(",gauge:ACRT2,gauge:BRTT2"), "{link}");
        assert_eq!(super::parse_goto(&link).unwrap().gauges, g.gauges);
        // Tropical guidance: on, or focused on a system; junk ids are refused.
        let g = super::parse_goto("KAMX,-80.4,25.6,6,tc:AL062026").unwrap();
        assert_eq!(g.tropical.as_deref(), Some("al062026"));
        assert!(super::goto_link(&g).ends_with(",tc:al062026"));
        let g = super::parse_goto("KAMX,-80.4,25.6,6,tc").unwrap();
        assert_eq!(g.tropical.as_deref(), Some(""));
        assert_eq!(
            super::parse_goto(&super::goto_link(&g)).unwrap().tropical,
            g.tropical
        );
        assert_eq!(
            super::parse_goto("KAMX,-80.4,25.6,6,tc:x/y")
                .unwrap()
                .tropical,
            None
        );
        // Old links are unchanged: no basemap, not storm-relative.
        let g = super::parse_goto(",-97.3,35.3,6.5").unwrap();
        assert_eq!(g.site, "");
        assert_eq!(g.basemap, None);
        assert!(!g.srv);
        assert_eq!(g.zoom, 6.5);
    }

    #[test]
    fn goes_frame_follows_the_radar_clock() {
        use chrono::{TimeZone, Utc};
        let t = |m: u32| Utc.with_ymd_and_hms(2026, 5, 1, 20, m, 0).unwrap();
        let times = [t(0), t(10), t(20), t(30)];
        // Nearest wins, ties included.
        assert_eq!(super::nearest_goes(&times, t(21)), Some(t(20)));
        assert_eq!(super::nearest_goes(&times, t(26)), Some(t(30)));
        // Past the tolerance, stay on the latest instead of showing the wrong hour.
        let far = Utc.with_ymd_and_hms(2026, 5, 1, 22, 0, 0).unwrap();
        assert_eq!(super::nearest_goes(&times, far), None);
        assert_eq!(super::nearest_goes(&[], t(0)), None);
    }

    #[test]
    fn temperature_contours_follow_the_selected_unit() {
        use crate::settings::TempUnit;

        for kind in [ContourKind::T2m, ContourKind::Td2m] {
            assert_eq!(kind.interval(TempUnit::Fahrenheit), 5.0);
            assert_eq!(kind.interval(TempUnit::Celsius), 2.0);
            assert!((kind.to_display(273.15, TempUnit::Fahrenheit) - 32.0).abs() < 1e-4);
            assert!(kind.to_display(273.15, TempUnit::Celsius).abs() < 1e-4);
            assert!(kind.display_label(TempUnit::Fahrenheit).ends_with("°F"));
            assert!(kind.display_label(TempUnit::Celsius).ends_with("°C"));
        }

        let model = wxdata::hrrr::Model::Hrrr;
        assert_ne!(
            (ContourKind::T2m, model, TempUnit::Fahrenheit),
            (ContourKind::T2m, model, TempUnit::Celsius),
            "the fetch key must change with units"
        );
        assert_eq!(ContourKind::Mslp.interval(TempUnit::Celsius), 2.0);
    }

    #[test]
    fn contour_summary_names_what_is_actually_on() {
        let mut active = std::collections::BTreeSet::new();
        assert_eq!(summarize_contours(&active), "Off");
        active.insert(ContourKind::Mslp);
        assert_eq!(summarize_contours(&active), "MSLP");
        // Order follows the enum's own declaration order (`BTreeSet`'s derived `Ord`), not
        // insertion order, so the summary reads the same regardless of which was toggled first.
        active.insert(ContourKind::Cape);
        assert_eq!(summarize_contours(&active), "MSLP, SB-CAPE");
        let mut inserted_other_order = std::collections::BTreeSet::new();
        inserted_other_order.insert(ContourKind::Cape);
        inserted_other_order.insert(ContourKind::Mslp);
        assert_eq!(summarize_contours(&inserted_other_order), "MSLP, SB-CAPE");
    }

    #[test]
    fn blink_alternates_on_a_clean_half_cycle_boundary() {
        // A is up for the first half of every cycle, B for the second — starting on A at t=0.
        assert!(!HookEchoApp::blink_showing_b(0.0, 1.5));
        assert!(!HookEchoApp::blink_showing_b(1.49, 1.5));
        assert!(HookEchoApp::blink_showing_b(1.5, 1.5));
        assert!(HookEchoApp::blink_showing_b(2.9, 1.5));
        // A second full cycle later, the phase repeats rather than drifting.
        assert!(!HookEchoApp::blink_showing_b(3.0, 1.5));
        assert!(HookEchoApp::blink_showing_b(4.5, 1.5));
    }

    #[test]
    fn swipe_side_tracks_the_clamped_divider() {
        assert!(!HookEchoApp::swipe_showing_b(0.5, 499.9, 1000.0));
        assert!(HookEchoApp::swipe_showing_b(0.5, 500.0, 1000.0));
        assert!(!HookEchoApp::swipe_showing_b(-1.0, -0.1, 1000.0));
        assert!(HookEchoApp::swipe_showing_b(-1.0, 0.0, 1000.0));
        assert!(!HookEchoApp::swipe_showing_b(2.0, 999.9, 1000.0));
        assert!(HookEchoApp::swipe_showing_b(2.0, 1000.0, 1000.0));
    }

    fn scan_progress(chunk_index: usize, chunks_in_sweep: usize) -> wxdata::live::ScanProgress {
        wxdata::live::ScanProgress {
            volume_start_ms: None,
            vcp_number: None,
            cut_kind: wxdata::live::CutKind::Standard,
            elevation_number: 2,
            total_elevations: 12,
            elevation_angle_deg: 0.9,
            azimuth_rate_dps: 90.0,
            azimuth_start_deg: (chunk_index.saturating_sub(1) as f64 * 360.0)
                / chunks_in_sweep.max(1) as f64,
            azimuth_end_deg: (chunk_index as f64 * 360.0) / chunks_in_sweep.max(1) as f64,
            chunk_index,
            chunks_in_sweep,
        }
    }

    #[test]
    fn live_sweep_animates_only_the_arriving_chunk_sector() {
        let progress = scan_progress(2, 3);
        let duration = progress.chunk_duration_secs();
        let start = super::live_sweep_frame(progress, 0.0, false).unwrap();
        assert!((start.angle_deg - 120.0).abs() < 1e-4);
        assert_eq!(start.alpha, 1.0);

        let middle = super::live_sweep_frame(progress, duration * 0.5, false).unwrap();
        assert!((middle.angle_deg - 180.0).abs() < 1e-3);
        let end = super::live_sweep_frame(progress, duration, false).unwrap();
        assert!((end.angle_deg - 240.0).abs() < 1e-3);
        assert!(super::live_sweep_frame(progress, duration + 0.36, false).is_none());

        let mut slower_cut = scan_progress(2, 3);
        slower_cut.azimuth_rate_dps = 60.0;
        let slower_middle =
            super::live_sweep_frame(slower_cut, slower_cut.chunk_duration_secs() * 0.5, false)
                .unwrap();
        assert!((slower_middle.angle_deg - 180.0).abs() < 1e-3);
    }

    #[test]
    fn live_sweep_wraps_north_and_reduced_motion_holds_the_arrived_edge() {
        let moving = super::live_sweep_frame(scan_progress(6, 6), 0.0, false).unwrap();
        assert!((moving.angle_deg - 300.0).abs() < 1e-4);
        let reduced = super::live_sweep_frame(scan_progress(6, 6), 0.0, true).unwrap();
        assert!(reduced.angle_deg.abs() < 1e-4);
        assert!(reduced.remaining_secs > 0.0);
    }

    #[test]
    fn live_sweep_rejects_bad_metadata_and_other_tilts() {
        assert!(super::live_sweep_frame(scan_progress(0, 6), 0.0, false).is_none());
        assert!(super::live_sweep_frame(scan_progress(7, 6), 0.0, false).is_none());
        assert!(super::live_sweep_frame(scan_progress(1, 0), 0.0, false).is_none());
        assert!(super::live_progress_matches_tilt(scan_progress(1, 6), 0.91));
        assert!(!super::live_progress_matches_tilt(scan_progress(1, 6), 1.3));
    }

    #[test]
    fn live_sweep_paints_keyed_beam_and_tail() {
        let ctx = egui::Context::default();
        let size = egui::vec2(400.0, 300.0);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| {
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let painter = ui.painter_at(rect);
            let camera = crate::render::mercator::Camera::at_lonlat(-97.0, 35.0, 6.0);
            super::paint_live_sweep(
                &painter,
                rect,
                camera,
                (size.x, size.y),
                [-97.0, 35.0],
                super::LiveSweepFrame {
                    angle_deg: 90.0,
                    alpha: 1.0,
                    remaining_secs: 1.0,
                },
            );
        });
        let segments = output
            .shapes
            .iter()
            .filter(|shape| matches!(shape.shape, egui::Shape::LineSegment { .. }))
            .count();
        assert_eq!(segments, 4, "two trail lines plus the keyed live beam");
    }

    #[test]
    fn blink_schedules_the_next_repaint_exactly_at_the_flip() {
        assert_eq!(HookEchoApp::blink_seconds_until_flip(0.0, 1.5), 1.5);
        assert!((HookEchoApp::blink_seconds_until_flip(1.0, 1.5) - 0.5).abs() < 1e-9);
        assert!((HookEchoApp::blink_seconds_until_flip(1.5, 1.5) - 1.5).abs() < 1e-9);
        // Never schedules a zero or negative delay, which would either spin every frame or panic
        // the `Duration` conversion at the call site.
        assert!(HookEchoApp::blink_seconds_until_flip(1.499_999_999, 1.5) > 0.0);
    }

    #[test]
    fn icon_sheet_cache_paths_are_per_url_and_filename_safe() {
        let Some(a) = super::icon_sheet_cache_path("https://example.com/a/icons.png") else {
            return; // no disk on this platform: nothing to name
        };
        let b = super::icon_sheet_cache_path("https://example.com/b/icons.png").unwrap();
        assert_ne!(a, b, "two sheets must not share a file");
        let name = a.file_name().unwrap().to_str().unwrap();
        assert!(
            name.chars().all(|c| c.is_ascii_hexdigit()),
            "a URL is not a filename: got {name}"
        );
    }
    use super::*;

    #[test]
    fn every_overlay_toggle_survives_a_slug_round_trip() {
        for t in OverlayToggle::ALL {
            assert_eq!(OverlayToggle::from_slug(&t.slug()), Some(t), "{t:?}");
        }
        // ALL has to actually be all of them — a variant left out would silently stop persisting.
        let mut slugs: Vec<String> = OverlayToggle::ALL.iter().map(|t| t.slug()).collect();
        slugs.sort();
        slugs.dedup();
        assert_eq!(slugs.len(), OverlayToggle::ALL.len());
        assert_eq!(OverlayToggle::from_slug("Teleportation"), None);
    }

    #[test]
    fn every_contour_kind_is_saved_and_read_back_by_its_token() {
        for k in ContourKind::ALL {
            match k.token() {
                Some(t) => assert_eq!(ContourKind::from_token(t), Some(k), "{k:?}"),
                None => assert_eq!(k, ContourKind::Off),
            }
        }
    }

    #[test]
    fn goto_parses_every_form_it_arrives_in() {
        let g = parse_goto("KTLX,-97.3,35.3,9").unwrap();
        assert_eq!(
            (g.site.as_str(), g.lon, g.lat, g.zoom),
            ("KTLX", -97.3, 35.3, 9.0)
        );
        assert!(g.time.is_none() && g.moment.is_none() && g.tilt.is_none());
        // The URL form is the same string behind a scheme.
        assert_eq!(
            parse_goto("hookecho://goto/KTLX,-97.3,35.3,9")
                .unwrap()
                .site,
            "KTLX"
        );
        // AlertService writes a site-less notification link; that must keep working.
        assert_eq!(parse_goto(",-97.3,35.3,9").unwrap().site, "");
        // Archive links carry a time.
        let g = parse_goto("KTLX,-97.3,35.3,9,2013-05-20T20:00:00Z").unwrap();
        assert_eq!(g.time.unwrap().to_rfc3339(), "2013-05-20T20:00:00+00:00");
        // A bare site resolves from the registry.
        let g = parse_goto("ktlx").unwrap();
        assert_eq!(g.site, "KTLX");
        assert!(g.lon < -97.0 && g.lat > 35.0 && g.zoom == 8.0);
        // Product and tilt are sniffed by shape, in either order.
        for v in ["KTLX,-97.3,35.3,9,VEL,2", "KTLX,-97.3,35.3,9,2,VEL"] {
            let g = parse_goto(v).unwrap();
            assert_eq!((g.moment, g.tilt), (Some(Moment::Velocity), Some(2)), "{v}");
        }
        assert!(parse_goto("").is_none());
        assert!(parse_goto("garbage").is_none());
    }

    #[test]
    fn goto_survives_a_percent_encoded_round_trip() {
        // What a chat client hands back after eating the link.
        let g = parse_goto("KTLX%2C-97.3%2C35.3%2C9%2C2013-05-20T20%3A00%3A00Z").unwrap();
        assert_eq!(g.site, "KTLX");
        assert_eq!(g.zoom, 9.0);
        assert_eq!(g.time.unwrap().to_rfc3339(), "2013-05-20T20:00:00+00:00");
        // Encoded spaces around the fields, and a stray `%` that is not an escape at all.
        assert_eq!(parse_goto("%20ktlx%20").unwrap().site, "KTLX");
        assert!(parse_goto("%GG").is_none());
    }

    #[test]
    fn goto_link_round_trips() {
        let base = |moment, tilt, threshold| Goto {
            site: "KFWS".to_string(),
            lon: -97.3031,
            lat: 32.5731,
            zoom: 8.5,
            time: None,
            moment: Some(moment),
            tilt: Some(tilt),
            basemap: None,
            threshold,
            srv: false,
            gauges: Vec::new(),
            tropical: None,
        };
        let link = goto_link(&base(Moment::Reflectivity, 0, None));
        assert!(link.starts_with("hookecho://goto/KFWS,"), "{link}");
        let g = parse_goto(&link).unwrap();
        assert_eq!(g.site, "KFWS");
        assert!((g.lon - -97.3031).abs() < 1e-4 && (g.lat - 32.5731).abs() < 1e-4);
        assert_eq!(g.zoom, 8.5);
        // Reflectivity at the base tilt is the default, so it stays out of the link.
        assert!(!link.contains("dBZ"), "{link}");

        let link = goto_link(&base(Moment::Velocity, 3, None));
        let g = parse_goto(&link).unwrap();
        assert_eq!((g.moment, g.tilt), (Some(Moment::Velocity), Some(3)));
        // No threshold set means the link says nothing, leaving the recipient's own alone.
        assert!(!link.contains("thr:"), "{link}");

        // Issue #71: an embedded dashboard needs to deep-link a threshold, and the link the Copy
        // button produces has to come back as the threshold it was copied from.
        let link = goto_link(&base(Moment::Reflectivity, 0, Some(Some(25.0))));
        assert!(link.contains(",thr:25"), "{link}");
        assert_eq!(parse_goto(&link).unwrap().threshold, Some(Some(25.0)));
        // A view with the threshold switched off shares as "nothing to say", not as `thr:off` —
        // overriding the recipient's own setting with a default nobody chose.
        assert!(!goto_link(&base(Moment::Reflectivity, 0, Some(None))).contains("thr:"));
    }

    #[test]
    fn goto_parses_a_threshold_by_shape() {
        // Order does not matter, and the field is sniffed by shape like every other extra.
        assert_eq!(
            parse_goto("KTLX,-97.3,35.3,8,thr:25,VEL")
                .unwrap()
                .threshold,
            Some(Some(25.0))
        );
        // Off is a deliberate instruction, distinct from saying nothing at all.
        assert_eq!(
            parse_goto("KTLX,-97.3,35.3,8,thr:off").unwrap().threshold,
            Some(None)
        );
        assert_eq!(parse_goto("KTLX,-97.3,35.3,8").unwrap().threshold, None);
        // Garbage is ignored, not applied as zero.
        assert_eq!(
            parse_goto("KTLX,-97.3,35.3,8,thr:loud").unwrap().threshold,
            None
        );
    }

    /// The draw tool must append into the stroke in flight and start a new one per drag, and Undo
    /// must drop exactly one stroke — the whole contract of a scribble layer.
    #[test]
    fn draw_strokes_append_and_undo() {
        let red = DRAW_COLORS[0];
        let cyan = DRAW_COLORS[2];
        let mut strokes = Vec::new();
        draw_append(&mut strokes, [-97.0, 35.0], red, true);
        draw_append(&mut strokes, [-97.1, 35.1], red, false);
        // A repeated point (a still finger during a drag) adds nothing.
        draw_append(&mut strokes, [-97.1, 35.1], red, false);
        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].points.len(), 2);

        draw_append(&mut strokes, [-98.0, 36.0], cyan, true);
        assert_eq!(strokes.len(), 2);
        assert_eq!(strokes[1].color, cyan);

        strokes.pop(); // Undo
        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].color, red);
    }
}

/// Type-in half of the archive-day control: `YYYY-MM-DD`, committed only on a complete,
/// parseable date — otherwise every keystroke mid-typing would send the timeline somewhere. The
/// buffer lives in egui's own memory rather than app state, because a half-typed date is not
/// something the app has any use for.
fn archive_day_text_input(ui: &mut egui::Ui, date: chrono::NaiveDate) -> Option<chrono::NaiveDate> {
    let id = egui::Id::new("archive-day-text");
    let shown = date.format("%Y-%m-%d").to_string();
    let mut buf: String = ui
        .data_mut(|d| d.get_temp(id))
        .unwrap_or_else(|| shown.clone());
    // Someone else moved the day (a caret, the calendar grid, a deep link): follow it rather than
    // argue. Gated on the buffer already being a complete date, not just any mismatch, so a
    // half-typed string mid-edit is never clobbered — only a fully-committed stale one. Comparing
    // full dates rather than a leading-year slice, which missed every external move that kept the
    // year the same (a caret step, most calendar picks) and left the field showing yesterday's
    // pick after today's.
    if chrono::NaiveDate::parse_from_str(&buf, "%Y-%m-%d").is_ok_and(|parsed| parsed != date) {
        buf = shown.clone();
    }
    let resp = ui
        .add(
            egui::TextEdit::singleline(&mut buf)
                .desired_width(84.0)
                .font(egui::TextStyle::Monospace),
        )
        .on_hover_text(
            "Archive days are UTC days — the S3 buckets are bucketed that way. \
             Type YYYY-MM-DD; the archive starts 1991-06-05.",
        );
    let out = chrono::NaiveDate::parse_from_str(&buf, "%Y-%m-%d")
        .ok()
        .filter(|d| *d != date);
    ui.data_mut(|d| d.insert_temp(id, buf));
    resp.changed().then_some(out).flatten()
}

/// The archive-day control inside the LIVE/ARCHIVE badge menu: a typed field plus a toggle for
/// [`archive_day_calendar`]. `Some` on the frame the typed field is edited to a complete date —
/// the calendar reports its own picks separately, since it renders as a block below this row
/// rather than inline in it.
///
/// This used to be a native-only `egui_extras::DatePickerButton` (a typed field alone on web,
/// to skip the ~120 KB gzipped the picker and its `jiff` date type cost). That button rarely
/// opened here: it draws its own `egui::Popup`, nested inside the timeline menu's popup, and
/// egui only tracks one "close me on outside click" popup at a time, so opening the inner one
/// usually closed the outer menu first. The calendar below is homegrown specifically to avoid
/// that — it is plain widgets drawn straight into the already-open menu, no nested popup and no
/// extra dependency — so native and web now share one implementation.
fn archive_day_input(ui: &mut egui::Ui, date: chrono::NaiveDate) -> Option<chrono::NaiveDate> {
    let open_id = egui::Id::new("archive-day-calendar-open");
    let mut open: bool = ui.data_mut(|d| d.get_temp(open_id)).unwrap_or(false);
    if ui
        .selectable_label(open, egui_phosphor::regular::CALENDAR)
        .on_hover_text("Browse by month and year")
        .clicked()
    {
        open = !open;
        ui.data_mut(|d| d.insert_temp(open_id, open));
    }
    archive_day_text_input(ui, date)
}

/// The calendar block toggled by [`archive_day_input`]'s button: a year field, month arrows, and
/// a day grid, drawn below the typed-field row rather than inside it so the grid gets the menu's
/// full width. `Some` on the frame a day is clicked. Closes itself on a pick, since picking a day
/// is the natural end of browsing.
fn archive_day_calendar(ui: &mut egui::Ui, date: chrono::NaiveDate) -> Option<chrono::NaiveDate> {
    use crate::ui::a11y::Named as _;
    use chrono::Datelike;
    let open_id = egui::Id::new("archive-day-calendar-open");
    if !ui.data_mut(|d| d.get_temp(open_id)).unwrap_or(false) {
        return None;
    }
    let today = chrono::Utc::now().date_naive();
    let view_id = egui::Id::new("archive-day-calendar-view");
    let (mut year, mut month) = ui
        .data_mut(|d| d.get_temp::<(i32, u32)>(view_id))
        .unwrap_or((date.year(), date.month()));
    let mut picked = None;

    ui.separator();
    ui.horizontal(|ui| {
        if ui
            .button(egui_phosphor::regular::CARET_LEFT)
            .named("Previous month")
            .clicked()
        {
            (year, month) = if month == 1 {
                (year - 1, 12)
            } else {
                (year, month - 1)
            };
        }
        ui.add(
            egui::DragValue::new(&mut year)
                .range(wxdata::level2::ARCHIVE_START.year()..=today.year())
                .custom_formatter(|v, _| format!("{v:04}")),
        )
        .on_hover_text("Year — drag, or click to type one");
        ui.label(
            chrono::NaiveDate::from_ymd_opt(year, month, 1)
                .map(|d| d.format("%B").to_string())
                .unwrap_or_default(),
        );
        if ui
            .button(egui_phosphor::regular::CARET_RIGHT)
            .named("Next month")
            .clicked()
        {
            (year, month) = if month == 12 {
                (year + 1, 1)
            } else {
                (year, month + 1)
            };
        }
    });

    let first = chrono::NaiveDate::from_ymd_opt(year, month, 1);
    let days_in_month = first
        .and_then(|_| {
            let (ny, nm) = if month == 12 {
                (year + 1, 1)
            } else {
                (year, month + 1)
            };
            chrono::NaiveDate::from_ymd_opt(ny, nm, 1)
        })
        .zip(first)
        .map(|(next, first)| (next - first).num_days())
        .unwrap_or(0);
    if let Some(first) = first {
        egui::Grid::new("archive-day-calendar-grid")
            .spacing([4.0, 4.0])
            .show(ui, |ui| {
                for wd in ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"] {
                    ui.label(
                        egui::RichText::new(wd)
                            .weak()
                            .size(crate::ui::style::FONT_SM),
                    );
                }
                ui.end_row();
                let lead = first.weekday().num_days_from_sunday();
                let mut col = 0;
                for _ in 0..lead {
                    ui.label("");
                    col += 1;
                }
                for day in 1..=days_in_month as u32 {
                    let d = chrono::NaiveDate::from_ymd_opt(year, month, day).unwrap();
                    let enabled = d >= wxdata::level2::ARCHIVE_START && d <= today;
                    let button = egui::Button::new(day.to_string())
                        .small()
                        .selected(d == date)
                        .frame(d == today && d != date);
                    if ui.add_enabled(enabled, button).clicked() {
                        picked = Some(d);
                    }
                    col += 1;
                    if col == 7 {
                        ui.end_row();
                        col = 0;
                    }
                }
            });
    }

    ui.data_mut(|d| d.insert_temp(view_id, (year, month)));
    if picked.is_some() {
        ui.data_mut(|d| d.insert_temp(open_id, false));
    }
    picked
}

#[cfg(test)]
mod archive_calendar_tests {
    use chrono::Datelike;

    /// December must roll into next January and January must roll back into last December —
    /// the two edges the plain `month - 1`/`month + 1` arithmetic in the widget cannot handle
    /// itself, which is why it is guarded with the wraparound checks it has.
    #[test]
    fn month_navigation_wraps_the_year() {
        let (year, month) = (2026i32, 1u32);
        let (py, pm) = if month == 1 {
            (year - 1, 12)
        } else {
            (year, month - 1)
        };
        assert_eq!((py, pm), (2025, 12));
        let (year, month) = (2026i32, 12u32);
        let (ny, nm) = if month == 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };
        assert_eq!((ny, nm), (2027, 1));
    }

    /// The grid needs an exact day count per month, including the leap-year edge, to avoid
    /// drawing a nonexistent Feb 30th or clipping Feb 29th on a leap year.
    #[test]
    fn days_in_month_matches_the_calendar_including_leap_years() {
        for (year, month, expected) in [
            (2026, 1, 31),
            (2026, 2, 28),
            (2024, 2, 29),
            (2026, 4, 30),
            (2026, 12, 31),
        ] {
            let first = chrono::NaiveDate::from_ymd_opt(year, month, 1).unwrap();
            let (ny, nm) = if month == 12 {
                (year + 1, 1)
            } else {
                (year, month + 1)
            };
            let next = chrono::NaiveDate::from_ymd_opt(ny, nm, 1).unwrap();
            assert_eq!((next - first).num_days(), expected, "{year}-{month}");
        }
    }

    /// The archive's actual start date must fall inside the year range the widget lets the
    /// `DragValue` reach, or 1991-06-05 itself would be unreachable by year alone.
    #[test]
    fn archive_start_year_is_a_valid_calendar_year() {
        assert_eq!(wxdata::level2::ARCHIVE_START.year(), 1991);
        assert!(wxdata::level2::ARCHIVE_START.month() >= 1);
    }

    /// A day moved from outside the text field (a caret, the calendar grid) must show up in the
    /// field even when the year does not change — the buffer used to resync only on a leading-year
    /// mismatch, so stepping from the 13th to the 3rd of the same month left "13" on screen while
    /// the map had already moved to the 3rd.
    #[test]
    fn the_typed_field_follows_a_same_year_external_move() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("archive-day-text");
        let day13 = chrono::NaiveDate::from_ymd_opt(2026, 9, 13).unwrap();
        let day03 = chrono::NaiveDate::from_ymd_opt(2026, 9, 3).unwrap();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            super::archive_day_text_input(ui, day13);
        });
        assert_eq!(
            ctx.data(|d| d.get_temp::<String>(id)),
            Some("2026-09-13".to_string())
        );
        // The caller moved `date` without the field's own text ever changing — a caret step or a
        // calendar pick, not a keystroke.
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            super::archive_day_text_input(ui, day03);
        });
        assert_eq!(
            ctx.data(|d| d.get_temp::<String>(id)),
            Some("2026-09-03".to_string()),
            "the field must drop the stale day rather than keep showing the 13th"
        );
    }
}

/// How much to trust a pure advection at `lead_min` minutes, 1.0 down to ~0.35.
///
/// The nowcast moves the echo that exists along the mean storm motion. It cannot grow a cell,
/// collapse one, or turn it, and every minute of lead is another minute for those to happen. Up
/// to 45 minutes — the range this shipped with — it is taken at face value; past that it fades,
/// which is the only honest way to keep offering it.
fn nowcast_confidence(lead_min: u8) -> f32 {
    const FULL: f32 = 45.0;
    const FLOOR: f32 = 0.35;
    if lead_min as f32 <= FULL {
        return 1.0;
    }
    let over = (lead_min as f32 - FULL) / (120.0 - FULL);
    (1.0 - over.clamp(0.0, 1.0) * (1.0 - FLOOR)).clamp(FLOOR, 1.0)
}

#[cfg(test)]
mod nowcast_tests {
    use super::nowcast_confidence;

    #[test]
    fn confidence_is_full_inside_the_old_range_and_fades_past_it() {
        for lead in [15u8, 30, 45] {
            assert_eq!(nowcast_confidence(lead), 1.0, "{lead} min");
        }
        assert!(nowcast_confidence(60) < 1.0);
        assert!(nowcast_confidence(90) < nowcast_confidence(60));
        assert!((nowcast_confidence(120) - 0.35).abs() < 1e-5);
    }

    /// It must never fade to invisible, or the layer would silently stop existing.
    #[test]
    fn confidence_never_reaches_zero() {
        for lead in 0u8..=255 {
            assert!(nowcast_confidence(lead) >= 0.35, "{lead} min");
        }
    }
}

#[cfg(test)]
mod probe_grid_tests {
    use super::{format_probe_field_value, HookEchoApp};
    use crate::render::FieldLayer as FL;
    use crate::settings::TempUnit;

    #[test]
    fn probe_values_follow_legend_units_and_categories() {
        assert_eq!(
            format_probe_field_value(FL::GlobalTemp2m, 273.15, TempUnit::Fahrenheit).as_deref(),
            Some("32.0 °F")
        );
        assert_eq!(
            format_probe_field_value(FL::Hca, 90.0, TempUnit::Celsius).as_deref(),
            Some("Graupel")
        );
        assert_eq!(
            format_probe_field_value(FL::Mrms, 42.25, TempUnit::Celsius).as_deref(),
            Some("42.2 dBZ")
        );
        assert!(format_probe_field_value(FL::Mrms, f32::NAN, TempUnit::Celsius).is_none());
    }

    /// ROADMAP_NEW E6's "brightness-temperature sample": a satellite pixel has to read as a
    /// temperature in the user's own unit, not as the raw Kelvin the grid holds.
    #[test]
    fn a_goes_channel_samples_as_a_brightness_temperature() {
        assert_eq!(
            format_probe_field_value(FL::GoesIr, 273.15, TempUnit::Celsius).as_deref(),
            Some("0.0 °C")
        );
        assert_eq!(
            format_probe_field_value(FL::GoesIr, 273.15, TempUnit::Fahrenheit).as_deref(),
            Some("32.0 °F")
        );
        // Every water-vapor channel shares one ramp, so all three have to read this way too.
        assert_eq!(
            format_probe_field_value(FL::GoesMidWaterVapor, 273.15, TempUnit::Celsius).as_deref(),
            Some("0.0 °C")
        );
    }

    /// The two derived GOES layers deliberately store a *transformed* quantity rather than an
    /// absolute brightness temperature (`GoesColdTop` holds degrees colder than its 210 K
    /// threshold; `GoesDustDiff` holds a band difference). Presenting either as a temperature
    /// would be a wrong number with a plausible-looking unit beside it, so this pins that they
    /// carry their own units instead of going through the Kelvin conversion.
    #[test]
    fn derived_goes_layers_are_not_reported_as_absolute_temperatures() {
        let cold_top = format_probe_field_value(FL::GoesColdTop, 20.0, TempUnit::Celsius)
            .expect("a finite sample formats");
        assert!(
            !cold_top.contains("°C"),
            "an offset from a threshold is not a temperature: {cold_top}"
        );
        assert!(cold_top.starts_with("20.0"), "{cold_top}");

        assert_eq!(
            format_probe_field_value(FL::GoesRgb, 0x80_40_20_u32 as f32, TempUnit::Celsius)
                .as_deref(),
            Some("RGB 128 64 32"),
            "a composite probes as its colour, not its packed number"
        );
        {
            use crate::app::{goes_grid, goes_sector_for};
            use wxdata::goes_abi::{Footprint, Sector};
            let fp = Footprint {
                lon_west: -82.0,
                lon_east: -68.0,
                lat_south: 32.0,
                lat_north: 46.0,
                time: chrono::DateTime::UNIX_EPOCH,
            };
            let boston = (-71.0, 42.3);
            let okc = (-97.5, 35.5);
            // CONUS stays CONUS; a box not yet seen is tried; a box over the view is read.
            assert_eq!(
                goes_sector_for(Sector::Conus, Some((Sector::Meso1, fp)), okc),
                Sector::Conus
            );
            assert_eq!(goes_sector_for(Sector::Meso1, None, okc), Sector::Meso1);
            assert_eq!(
                goes_sector_for(Sector::Meso1, Some((Sector::Meso1, fp)), boston),
                Sector::Meso1
            );
            // The box moved away from the view: CONUS until it covers it again.
            assert_eq!(
                goes_sector_for(Sector::Meso1, Some((Sector::Meso1, fp)), okc),
                Sector::Conus
            );
            // Where Meso 1 is says nothing about Meso 2.
            assert_eq!(
                goes_sector_for(Sector::Meso2, Some((Sector::Meso1, fp)), okc),
                Sector::Meso2
            );
            assert_eq!(goes_grid(Sector::Meso2), (480, 480));
        }
        {
            use crate::app::{goes_frame_ready, goes_slot};
            use wxdata::goes_abi::Sector;
            let t = |m: i64| chrono::DateTime::from_timestamp(1_790_000_000 + m * 60, 0).unwrap();
            let tol = chrono::Duration::minutes(10);
            // Live: the slot is None and a live fetch is ready whatever its time.
            assert_eq!(goes_slot(None, Sector::Conus), None);
            assert!(goes_frame_ready(Some(None), None, Some(t(-3)), None, tol));
            // Scrubbed within one 5-minute CONUS slot: no refetch; into the next: refetch.
            assert_eq!(
                goes_slot(Some(t(0)), Sector::Conus),
                goes_slot(Some(t(0)), Sector::Conus)
            );
            assert_ne!(
                goes_slot(Some(t(0)), Sector::Conus),
                goes_slot(Some(t(6)), Sector::Conus)
            );
            // A mesoscale slot is a minute.
            assert_ne!(
                goes_slot(Some(t(0)), Sector::Meso1),
                goes_slot(Some(t(1)), Sector::Meso1)
            );
            let s = goes_slot(Some(t(0)), Sector::Conus);
            // The archive frame for this slot, close to the target: painted.
            assert!(goes_frame_ready(Some(s), s, Some(t(2)), Some(t(0)), tol));
            // A live frame still held when the view is scrubbed back: not painted.
            assert!(!goes_frame_ready(
                Some(None),
                s,
                Some(t(0)),
                Some(t(0)),
                tol
            ));
            // An archive frame still held after going live: not painted until the newest lands.
            assert!(!goes_frame_ready(Some(s), None, Some(t(0)), None, tol));
            // The nearest frame the bucket had was far off (a gap in the archive): not painted.
            assert!(!goes_frame_ready(Some(s), s, Some(t(40)), Some(t(0)), tol));
        }
        {
            use crate::app::glm_flashes_for;
            let t = |m: i64| chrono::DateTime::from_timestamp(1_790_000_000 + m * 60, 0).unwrap();
            let flash = |m: i64| wxdata::glm::Flash {
                lon: -97.0,
                lat: 35.0,
                energy: 1.0,
                time: t(m),
            };
            let live: std::collections::VecDeque<_> = [flash(100), flash(101)].into();
            let archive = (t(0), vec![flash(-3), flash(-1)]);
            // Live: the feed, aged against now.
            let (f, clock) = glm_flashes_for(None, &live, Some(&archive), t(102));
            assert_eq!((f.len(), clock), (2, t(102)));
            // Scrubbed to the archive window's minute: its flashes, aged against the view's time.
            let (f, clock) = glm_flashes_for(Some(t(0)), &live, Some(&archive), t(102));
            assert_eq!((f.len(), clock), (2, t(0)));
            // Scrubbed elsewhere before its window arrives: nothing, not today's flashes.
            let (f, _) = glm_flashes_for(Some(t(30)), &live, Some(&archive), t(102));
            assert!(f.is_empty());
        }
        let dust = format_probe_field_value(FL::GoesDustDiff, 3.0, TempUnit::Fahrenheit)
            .expect("a finite sample formats");
        assert!(
            !dust.contains("°F"),
            "a band difference is not a temperature: {dust}"
        );
        assert!(dust.starts_with("3.0"), "{dust}");
    }

    #[test]
    fn legacy_grid_products_get_analyst_facing_names() {
        assert_eq!(
            HookEchoApp::probe_field_product(FL::Mosaic),
            "Multi-radar reflectivity"
        );
        assert_eq!(
            HookEchoApp::probe_field_product(FL::CompositeLocal),
            "Local composite reflectivity"
        );
    }
}

#[cfg(test)]
mod comparison_display_tests {
    use super::{field_draw_opacity, format_diff_readout, workstation_too_narrow};

    #[test]
    fn the_workstation_steps_aside_at_phone_width_only() {
        assert!(workstation_too_narrow(390.0));
        assert!(!workstation_too_narrow(768.0), "a tablet keeps it");
        assert!(
            !workstation_too_narrow(f32::INFINITY),
            "before the first frame"
        );
    }
    use crate::fielddiff::DiffMode;
    use crate::render::FieldLayer as FL;

    #[test]
    fn only_the_upper_comparison_side_is_dimmed_and_user_opacity_is_preserved() {
        assert_eq!(field_draw_opacity(FL::CompareA, 0.8, true), 0.8);
        assert_eq!(field_draw_opacity(FL::CompareB, 0.8, true), 0.4);
        assert_eq!(field_draw_opacity(FL::CompareB, 0.8, false), 0.8);
        assert_eq!(field_draw_opacity(FL::Mrms, 0.6, true), 0.6);
    }

    #[test]
    fn mask_readout_names_the_class_and_keeps_the_underlying_magnitude() {
        assert_eq!(
            format_diff_readout(DiffMode::Disagreement, 0.5, 1.0, "°C"),
            "Agree · Δ 0.5 °C"
        );
        assert_eq!(
            format_diff_readout(DiffMode::Disagreement, -2.5, 1.0, "°C"),
            "Disagree · Δ 2.5 °C"
        );
    }
}

/// The one-line trail readout for Layer options: progress while folding, the restart reason when
/// the sequence changed beam, and what is being kept once it is complete.
/// The level a trail outline is drawn at when the pane has no value threshold: the conventional
/// "this matters" value for the product — a 50 dBZ core, 0.80 CC (debris), and so on.
fn trail_outline_level(moment: Moment, keep: wxdata::extrema::Extremum) -> f32 {
    use wxdata::extrema::Extremum;
    match (moment, keep) {
        (Moment::CorrelationCoefficient, _) => 0.80,
        (_, Extremum::Min) => 0.0,
        (Moment::Reflectivity, _) => 50.0,
        (Moment::Velocity, _) => 30.0,
        (Moment::SpectrumWidth, _) => 8.0,
        (Moment::DifferentialReflectivity, _) => 3.0,
        (Moment::SpecificDifferentialPhase, _) => 2.0,
        _ => 0.0,
    }
}

fn trail_status_line(
    folded: usize,
    wanted: usize,
    window_min: u16,
    keep: wxdata::extrema::Extremum,
    restarted: Option<wxdata::extrema::Mismatch>,
) -> String {
    use wxdata::extrema::{Extremum, Mismatch};
    let what = match keep {
        Extremum::Max => "maximum",
        Extremum::Min => "minimum",
    };
    let mut line = if folded < wanted {
        format!("Building {what} trail: {folded} of {wanted} cached volumes")
    } else {
        format!("{what} of {wanted} cached volumes over {window_min} min")
    };
    if let Some(why) = restarted {
        let why = match why {
            Mismatch::Moment => "the product changed",
            Mismatch::ValueRange => "raw and dealiased velocity differ",
            Mismatch::Geometry => "the scan geometry changed",
            Mismatch::Elevation => "the tilt changed",
            Mismatch::Site => "the site changed",
        };
        line.push_str(&format!(" — restarted: {why}"));
    }
    line
}

#[cfg(test)]
mod trail_status_tests {
    use super::trail_status_line;
    use wxdata::extrema::{Extremum, Mismatch};

    #[test]
    fn says_how_far_along_a_trail_is_while_it_builds() {
        let line = trail_status_line(4, 12, 60, Extremum::Max, None);
        assert_eq!(line, "Building maximum trail: 4 of 12 cached volumes");
    }

    #[test]
    fn names_the_kept_end_and_the_window_once_complete() {
        let line = trail_status_line(12, 12, 60, Extremum::Min, None);
        assert_eq!(line, "minimum of 12 cached volumes over 60 min");
    }

    #[test]
    fn a_restart_always_says_why() {
        for (why, text) in [
            (Mismatch::Site, "the site changed"),
            (Mismatch::Elevation, "the tilt changed"),
            (Mismatch::Moment, "the product changed"),
            (Mismatch::Geometry, "the scan geometry changed"),
            (Mismatch::ValueRange, "raw and dealiased velocity differ"),
        ] {
            let line = trail_status_line(3, 3, 30, Extremum::Max, Some(why));
            assert!(line.ends_with(&format!("restarted: {text}")), "{line}");
        }
    }
}

/// Degrees of pitch per pixel of two-finger vertical slide — the same rate the mouse's
/// right-drag tilt uses, so both feel alike.
const TWO_FINGER_PITCH_DEG_PER_PX: f32 = 0.25;

/// Split a two-finger slide into what pans the map and how far it tilts it.
///
/// On a flat map the whole slide pans. On a 3D map the vertical part tilts instead, because one
/// finger already pans and two fingers are how a touchscreen asks for the camera: sliding up
/// raises the pitch (the map leans back toward the horizon), sliding down lowers it. The
/// horizontal part still pans, so a diagonal slide does both rather than fighting the user.
fn split_two_finger_slide(slide: egui::Vec2, map_3d: bool) -> (egui::Vec2, f32) {
    if !map_3d {
        return (slide, 0.0);
    }
    // Screen y grows downward, so an upward slide is negative.
    (
        egui::vec2(slide.x, 0.0),
        -slide.y * TWO_FINGER_PITCH_DEG_PER_PX,
    )
}

#[cfg(test)]
mod two_finger_slide_tests {
    use super::split_two_finger_slide;

    #[test]
    fn a_flat_map_pans_with_the_whole_slide_and_never_tilts() {
        let (pan, pitch) = split_two_finger_slide(egui::vec2(3.0, -8.0), false);
        assert_eq!(pan, egui::vec2(3.0, -8.0));
        assert_eq!(pitch, 0.0);
    }

    #[test]
    fn sliding_up_on_a_3d_map_raises_the_pitch_and_down_lowers_it() {
        let (_, up) = split_two_finger_slide(egui::vec2(0.0, -40.0), true);
        let (_, down) = split_two_finger_slide(egui::vec2(0.0, 40.0), true);
        assert_eq!(up, 10.0, "40 px up is 10 degrees more pitch");
        assert_eq!(down, -10.0);
    }

    #[test]
    fn on_a_3d_map_the_vertical_part_no_longer_pans() {
        let (pan, _) = split_two_finger_slide(egui::vec2(6.0, -40.0), true);
        assert_eq!(
            pan,
            egui::vec2(6.0, 0.0),
            "horizontal still pans, vertical goes to the tilt"
        );
    }

    #[test]
    fn no_movement_is_no_change() {
        assert_eq!(
            split_two_finger_slide(egui::Vec2::ZERO, true),
            (egui::Vec2::ZERO, 0.0)
        );
    }
}
