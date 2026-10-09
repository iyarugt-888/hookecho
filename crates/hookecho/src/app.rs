//! HookEcho application shell: the map view, its floating chrome, and the async data flow.
//!
//! UI code only mutates the active [`MapView`]; a single per-frame sync step turns those
//! mutations into GPU uploads and background fetches, so buttons and hotkeys share one path.

mod acquisition;
mod actions;
mod boundaries;
mod case;
/// Touch-first Android chrome (top bar, bottom dock, slide-up sheets), replacing the desktop
/// drawer / pills / alert dock. Only the chrome differs; the map,
/// windows, and every data path are shared.
mod chrome;
mod column_product;
mod column_trail;
mod trail;
mod xsection_edit;
#[cfg(test)]
use trail::trail_status_line;
use trail::TrailState;
// For the headless verifier and its GPU check, which are native-only.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use column_product::column_upload;
mod env_levels;
mod field_state;
mod goes_context;
mod goes_timeline;
pub(crate) use goes_context::GoesRequest;
pub(crate) mod impact;
mod impact_report;
mod layer_probe;
mod live_session;
#[cfg(not(target_arch = "wasm32"))]
mod local_api;
pub(crate) mod long_loop;
mod overlay_health;
mod pane_time;
mod radar_wind;
mod region_stats;
pub(crate) mod model_field;
mod report;
mod beam_diagram;
mod sat_loop;
pub(crate) use actions::{decode_site_id, encode_site_id, AppWindow, NavStep, PaletteAction};
pub(crate) use contours::{summarize_contours, ContourEntry, ContourKind};
use fetch_schedule::field_refresh_secs;
pub(crate) use pane_detect::PaneDetections;
mod overlay_fetch;
pub(crate) use overlay_fetch::{OverlayDelivery, OverlayMsg, OverlaySource};
mod account_sync;
mod alert_latency;
mod gis_bundle;
mod wire_alerts;
mod alerts_watch;
mod beam_tools;
pub(crate) mod camera_flight;
mod capture_upload;
mod chase;
mod construct;
mod contours;
mod ensemble_stamps;
mod radar_outlines;
mod data_age;
mod data_poll;
mod detectors;
mod digest_brief;
mod draw_panes;
mod fetch_schedule;
mod floating_windows;
mod frame_end;
mod frame_intake;
mod gis_import;
mod goto;
mod model_groups;
mod models;
mod near_storm;
mod output_window;
mod presentation;
mod feature_chooser;
mod overlay_poll;
mod overlay_toggle;
mod packs_soundings;
mod palette_apply;
mod pane_detect;
mod pane_gpu;
mod pane_input;
mod pane_sync;
mod pane_upload;
mod placefiles_sync;
mod prefs_app;
mod prefs_map;
mod rebuild;
mod route_frame;
mod self_update_ui;
mod spatial_groups;
pub(crate) use goto::{goto_link, parse_goto, Goto};
mod cell_markers;
mod community_targets;
mod detector_markers;
mod gis_layers;
/// The imported-layer point symbols, for the Layer Manager's symbol legend.
pub(crate) use gis_layers::paint_symbol as paint_gis_symbol;
mod loop_capture;
mod map_click;
mod map_volume;
mod measure;
mod pane_3d_overlays;
mod pane_feeds;
mod pane_layout;
mod pane_legends;
mod pane_marks;
mod pane_overlays;
mod pane_places;
mod pane_points;
mod pane_stations;
mod radar_feed;
mod radar_probe;
mod radar_products;
mod rules;
mod scenes;
mod settings_bundle;
mod sharing;
mod standalone_volume;
mod surface_feeds;
mod terrain_cache;
mod time_layers;
mod touch_hover;
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
use acquisition::OverlayAcquisition;
mod model_cache;
mod model_context;
mod mrms_cache;
mod mrms_context;
pub(crate) use field_state::{FieldState, MrmsRequest};
use goes_timeline::nearest_goes;
pub(crate) use model_context::ModelRequest;
pub(crate) use mrms_context::MrmsContext;
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

/// The same, for a Level 2 volume: bigger file, more patience, still finite. A volume fetch that
/// never returns leaves its pane marked loading, and a pane marked loading never polls again.
/// Again longer than the request's own 90 s deadline in the vendored S3 client, so the abort
/// happens before we stop listening for it.
const VOLUME_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(100);

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
    /// Tornado detection: the rotation and debris detectors and Tornado ID's verdicts, as one
    /// marker per tornado (`OverlayToggle::TornadoId`).
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
mod cell_history_tests;

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

/// What a cross-section was sampled from (see `HookEchoApp::xsection_source`).
#[derive(Clone, Debug, PartialEq)]
struct XsectionSource {
    pane: usize,
    volume: String,
    revision: u64,
    moment: Moment,
    tilts: usize,
    span: Option<(DateTime<Utc>, DateTime<Utc>)>,
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
pub(crate) enum ShotDest {
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
    /// Frames to let the stepped radar paint once it is on screen, before grabbing.
    settle: u8,
    /// When the timeline was stepped to the frame being waited for (`loop_capture`).
    step_at: Instant,
    /// A screenshot has been requested; waiting for its event.
    capturing: bool,
    /// Playback speed the scrubber was set to when the export started — the exported clip plays
    /// at the speed the user was watching, instead of a hardcoded 5 fps.
    fps: f32,
    /// Each captured frame's volume and valid time, for real timing and the sidecar.
    volumes: Vec<Option<(String, DateTime<Utc>)>>,
    /// Hold each frame for its real scan gap (`Settings::loop_real_timing`) or all alike.
    real_timing: bool,
    /// What each captured frame asked for and found (ROADMAP_PARITY M6.3), for the manifest.
    records: Vec<crate::capture_manifest::FrameRecord>,
    /// The frame being captured: the scan asked for, and whether the wait for it ran out.
    asked: Option<String>,
    timed_out: bool,
}

/// A pane's smooth volume grid: `n`, `nz`, half-width km, top km, centre km east/north.
type SmoothDims = (u32, u32, f32, f32, [f32; 2]);

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
        /// Ended while still wanted (lost), not stopped by the app.
        lost: bool,
        /// Current subscription failure; ignored with an obsolete stream generation.
        error: Option<String>,
    },
    /// Observed progress for newly decoded live input, delivered before its merged `Live`
    /// update. Failed or unchanged decodes do not invent fresh progress.
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

type ShownKey = (
    String,
    Moment,
    usize,
    Option<f32>,
    bool,
    Option<(u32, u32)>,
    bool,
    // Precipitation-tint context and generation: changing time, refreshing the grid or toggling
    // tint rebuilds the image, including a switch to a previously cached context.
    Option<(crate::render::MrmsTextureKey, u64)>,
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

/// What `theme::apply` is keyed on, remembered so it only re-applies when it moves: the user's
/// accent.
type ThemeApplied = Option<[u8; 3]>;

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
    /// Last accent handed to `theme::apply`.
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
    /// The experimental LLSD pipeline's analysed columns for one volume (the settings flag
    /// `detectors.llsd_preview`), and the tracker that follows them from volume to volume on one
    /// site. See `compute_llsd`.
    /// With the analysed columns, when the sweeps they and the volume's earlier low-level passes
    /// were measured on were scanned (`detection_lineage`).
    #[allow(clippy::type_complexity)]
    llsd_cache: Option<(
        (usize, String, usize),
        Vec<wxdata::llsd_analyst::Analysed>,
        detectors::FusedInputs,
    )>,
    /// The fused pipeline's tracking on the active site, fed once per low-level pass.
    llsd_tracker: Option<detectors::LlsdTracking>,
    /// The HRRR hour beside the active volume, for the Tornado ID's environment gate.
    near_storm: near_storm::NearStormFeed,
    /// The background job computing one volume's columns (see `compute_llsd`).
    #[allow(clippy::type_complexity)]
    llsd_job: Option<(
        (usize, String, usize),
        std::sync::mpsc::Receiver<detectors::LlsdJob>,
    )>,
    /// When the sweeps behind `couplet_cache`'s couplets were scanned (`detection_lineage`).
    #[allow(clippy::type_complexity)]
    pub(crate) couplet_inputs: Option<(
        (usize, String, usize),
        Option<wxdata::level2::temporal::TemporalCoverage>,
    )>,
    /// When the sweeps behind `tds_cache`'s debris signatures were scanned (`detection_lineage`);
    /// `None` inside when not recorded (no dual-pol on the volume).
    #[allow(clippy::type_complexity)]
    pub(crate) tds_inputs: Option<(
        (usize, String, usize),
        Option<wxdata::level2::temporal::TemporalCoverage>,
    )>,
    /// The active pane's last Tornado ID verdicts, by volume, with where they came from: what the
    /// local API and the analysis export report. Set in `pane_detections`.
    #[allow(clippy::type_complexity)]
    pub(crate) tornado_shown: Option<(
        String,
        Vec<wxdata::tornado_id::TornadoId>,
        wxdata::detection_lineage::DetectionLineage,
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
    /// Active subscription, scoped retry and cancellation epoch; no radar payload ownership.
    live_session: live_session::LiveSession,
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
    acquisition: OverlayAcquisition,
    filters: OverlayFilters,
    alert_features: Vec<GeoFeature>,
    /// New alerts' sent → accepted → first frame built (`app/alert_latency.rs`).
    alert_latency: wxdata::alert_latency::LatencyLog,
    /// Warnings from the user's NWWS-OI relay, and their delivery (`app/wire_alerts.rs`).
    wire: wire_alerts::WireAlerts,
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
    built_imported_visible: u64,
    built_theme: crate::settings::Theme,
    /// Whether the overlay geometry was split for the globe.
    built_globe: bool,
    pending_overlay: Option<OverlayUpload>,
    overlay_ready: bool,
    overlay_last_fetch: Option<Instant>,
    /// When the 30 s warning-polygon poll last went out (`fetch_schedule`).
    warning_poll_at: Option<Instant>,
    /// A zipped bundle of several shapefiles waiting for the person to pick (`app/gis_bundle.rs`).
    gis_bundle: Option<gis_bundle::BundlePick>,
    detail: Option<Detail>,
    /// Several features under one click: the list to choose from (ROADMAP_PARITY M4.3).
    feature_chooser: Option<Vec<feature_chooser::ChoiceItem>>,
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
    /// What the difference layer differences and the exact shared valid time/source runs.
    diff_field: crate::fielddiff::DiffField,
    /// Signed `A - B` or magnitude-only `|A - B|`. The fetched CPU grid always stays signed;
    /// this controls only its upload, legend and readout.
    diff_mode: crate::fielddiff::DiffMode,
    diff_valid: Option<crate::fielddiff::ComparisonTimes>,
    diff_error: Option<String>,
    /// Request and accepted identity are separate from the shared decoded GOES texture.
    goes_fields: std::collections::HashMap<crate::render::FieldLayer, goes_context::GoesSlot>,
    goes_footprint: Option<(GoesRequest, wxdata::goes_abi::Footprint)>,
    goes_footprint_slot: goes_context::GoesSlot,
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
    ensemble_display_key: Option<crate::ensemble_layer::DisplayKey>,
    ensemble_error: Option<String>,
    /// The members' contours at the ensemble level, when spaghetti is on (see
    /// `ensemble_layer::spaghetti_key`).
    /// The ensemble's postage-stamp window (ROADMAP_NEW F7).
    ensemble_stamps: ensemble_stamps::EnsembleStamps,
    ensemble_spaghetti: Option<(
        crate::ensemble_layer::SpaghettiKey,
        std::sync::Arc<crate::ensemble_layer::Spaghetti>,
    )>,
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
    /// Towns in storms' projected paths, looked up on request (M2.3).
    towns: community_targets::TownsBook,
    terrain: terrain_cache::TerrainCache,
    /// The impact the open feature details show (a discussion's or a watch's), by its key in
    /// `impacts`; `None` for features that have no people count.
    detail_impact: Option<String>,
    /// Newest pane error and the time it appeared, for the auto-hiding bottom-center chip.
    error_chip: Option<(String, f64)>,
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
    /// Satellite-native playback (ROADMAP_PARITY M5.2); drives the GOES clock while on.
    sat_loop: sat_loop::SatLoop,
    /// The model field browser (ROADMAP_PARITY M5.3).
    field_browser: model_field::ModelFieldBrowser,
    /// The range-height beam diagram (WeatherWise-class beam rise).
    beam_diagram: beam_diagram::BeamDiagram,
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
    link_times: bool,
    /// When linked, use the active radar scan's actual timestamp as the analysis cursor rather
    /// than retaining an external source's requested valid time.
    lock_source_time: bool,
    /// ROADMAP_NEW J2: the selected storm (`cell_popup`) is shared by every pane — marked in each,
    /// and each recenters on it as it moves. See `paint_selected_storm`/`follow_linked_storm`.
    pub(crate) link_storm: bool,
    /// The selected storm's id and position the panes were last centered on, so the link moves
    /// them once per change rather than pinning them against the user's own panning.
    storm_link_at: Option<(String, f64, f64)>,
    /// The owner and point its cursor group is sharing, refreshed every frame from
    /// whichever pane the mouse is actually over and cleared when the pointer leaves every pane.
    /// Not persisted — a live hover position, not a saved preference.
    linked_probe: Option<(usize, (f64, f64))>,
    /// The map point under the pointer and its pane, whatever the cursor links (beam diagram).
    hover_lonlat: Option<(usize, (f64, f64))>,
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
    /// Global comparison and ensemble controls retain their existing shared owner.
    comparison_fcst_hour: u16,
    /// Contours retain their shared source until their separate M5.1 migration.
    contour_model: wxdata::hrrr::Model,
    model_fields: model_cache::ModelFieldCache,
    model_palette_gen: u64,
    model_drop_textures: Vec<crate::render::ModelTextureKey>,
    mrms_fields: mrms_cache::MrmsFieldCache,
    mrms_palette_gen: u64,
    mrms_drop_textures: Vec<crate::render::MrmsTextureKey>,
    /// Selected rotation-track accumulation window (minutes): 30, 60, or 120.
    rotation_minutes: u16,
    /// Selected hail-swath accumulation window (minutes); see [`wxdata::mrms::hail_swath`].
    hail_minutes: u16,
    /// The site the L3 gridded products (DVL/EET) were last fetched for (feature X); refetch on
    /// site change.
    l3grid_site: Option<String>,
    /// What the locally derived products (VIL/VILD/echo tops) were last computed from:
    /// The accepted volume revision, sweep policy, settings and both temperature levels.
    derived_key: Option<radar_products::DerivedKey>,
    /// The column user product build in flight, the one on the GPU, and the last failure with the
    /// selection it answered (`app::column_product`).
    column_requested: Option<column_product::ColumnKey>,
    column_accepted: Option<Arc<column_product::ColumnAccepted>>,
    column_failed: Option<(column_product::ColumnKey, String)>,
    /// The column product trail being built for the active pane (`app::column_trail`).
    column_trail: Option<column_trail::ColumnTrailState>,
    /// The most recent isotherm heights (`env_levels::EnvLevels`), for the hail grids and every
    /// other consumer of a melting level. Read through [`App::freezing_for`] /
    /// [`App::env_levels_for`], which only hand a view the levels for its own site and epoch.
    freezing: Option<env_levels::EnvLevels>,
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
    /// The master switch for every imported GIS layer (session only); each layer's own
    /// visibility is in `settings.gis_layers`.
    show_imported_gis: bool,
    /// What was read from each imported GIS layer's file (ROADMAP_PARITY M4.1), by layer ID.
    gis: Vec<gis_layers::LoadedGis>,
    /// Per `overlays` entry, its imported layer and source feature (`None`: official product).
    overlay_layer: Vec<Option<(u64, usize)>>,
    /// The layer the Layer Manager is editing, and the open feature table.
    gis_selected: Option<u64>,
    gis_table: Option<gis_layers::GisTable>,
    /// The layer settings the overlays were last assembled for, hashed.
    gis_settings_key: u64,
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
    /// Per pane, the wind layer's streamlines and a browsed wind's, as last traced.
    wind_streams: std::collections::HashMap<(usize, bool), crate::wind_streamlines::StreamCache>,
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
    /// Streaming mode's touch controls (ROADMAP_PARITY M6.4).
    presentation: presentation::Presentation,
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
    /// True while a rotation couplet is currently detected (rising-edge alarm latch).
    /// The highest Tornado ID tier Tornado detection last alerted on and still sees (Likely or
    /// above), so a verdict alerts once and again only when it rises (`tornado_alert`).
    tornado_alerted: Option<wxdata::tornado_id::Tier>,
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
    /// A cross-section handle being dragged (`app::xsection_edit`).
    xsection_drag: Option<xsection_edit::SectionDrag>,
    /// Carry the section into the 3D view's vertical cut as it moves.
    xsection_cut_3d: bool,
    /// What the shown section was sampled from — pane, volume, revision, moment — so it is rebuilt
    /// when that pane's volume grows or changes, and its tilt time span for the window.
    xsection_source: Option<XsectionSource>,
    /// Lazily-loaded textures for uploaded marker icons, keyed by filename. `None` = load failed
    /// (negative-cached so a missing/corrupt file isn't retried every frame).
    marker_icon_tex: ui::marker_window::IconTextures,
    /// 3D raymarch view: open flag, orbit camera (az/el degrees + distance), and a pending
    /// volume upload (taken by the first paint after a rebuild).
    show_3d: bool,
    vol3d: ui::volume3d_window::Volume3dState,
    /// One bounded worker, its attempted context and the context of the accepted 3D grid.
    vol3d_build: standalone_volume::VolumeBuildState,
    /// The built volume's dBZ span, which the window's threshold slider works in.
    vol3d_range: (f32, f32),
    vol3d_pending: Option<crate::render3d::Volume3dUpload>,
    /// The map-pitch "Smooth" 3D representation, one slot per pane (the app's own 4-pane
    /// ceiling) rather than the window's single `vol3d_*` set — more than one pane can be
    /// showing it at once, each its own site and volume.
    ///
    /// The accepted decoded source, revision, policy and build controls per pane. Upload
    /// geometry and contributor coverage are accepted together, and hidden on mismatch.
    /// Heavy samples stay shared with the byte-bounded playback cache.
    smooth_vol_key: [Option<SmoothKey>; crate::view::MAX_PANES],
    /// Panes whose GPU volume should be freed on their next paint (3D turned off).
    smooth_vol_release: [bool; crate::view::MAX_PANES],
    /// Per pane: the isosurface shells shown and the [`IsoKey`] they were built for
    /// (ROADMAP_NEW H3).
    iso_mesh: [Option<(IsoKey, Arc<crate::loop3d::IsoFrame>)>; crate::view::MAX_PANES],
    smooth_vol_coverage:
        [Option<wxdata::level2::temporal::TemporalCoverage>; crate::view::MAX_PANES],
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
    /// The colour stops the pane's Smooth volume was last uploaded with (`None`: its palette).
    smooth_vol_colors: [Option<crate::render3d::ColorStops>; crate::view::MAX_PANES],
    smooth_vol_dims: [Option<SmoothDims>; crate::view::MAX_PANES],
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

    fn spawn_overlay(&self, ctx: &egui::Context, source: OverlaySource) {
        self.acquisition
            .spawn(ctx, source, self.field_texture_cap());
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
        if self.views[self.active].models.hrrr_subhourly {
            crate::model_browser::BModel::Hrrr15.label().into()
        } else {
            self.views[self.active].models.refl_model.label().into()
        }
    }

    /// Drive the forecast-reflectivity layer from the active pane's timeline: scrubbing into the
    /// forecast tail enables HRRR at that forecast hour (and suppresses the observed radar for the
    /// scrubbed pane, done at draw time); scrubbing back to observed frames turns it off again.
    fn sync_forecast_scrub(&mut self) {
        if self.model_timeline_active() {
            return;
        }
        use crate::render::FieldLayer as FL;
        match self.views[self.active].timeline.forecast_hour() {
            Some(h) => {
                self.views[self.active].models.hrrr_fcst_hour = h;
                // The timeline tail is hourly; keep the sub-hourly lead in step with it so
                // scrubbing works the same in either mode.
                self.views[self.active].models.hrrr_fcst_min = u16::from(h) * 60;
                self.views[self.active].fields_on.insert(FL::Hrrr);
                self.views[self.active].models.hrrr_by_timeline = true;
                // The browser follows what the scrub put on the map.
                let scrubbed = crate::model_browser::Selection {
                    model: if self.views[self.active].models.hrrr_subhourly {
                        crate::model_browser::BModel::Hrrr15
                    } else {
                        crate::model_browser::BModel::from_regional(
                            self.views[self.active].models.refl_model,
                        )
                    },
                    product: crate::model_browser::Product::Reflectivity,
                };
                if self.views[self.active].models.model_sel != scrubbed {
                    self.views[self.active].models.model_sel = scrubbed;
                }
            }
            None => {
                if self.views[self.active].models.hrrr_by_timeline {
                    self.views[self.active].fields_on.remove(&FL::Hrrr);
                    self.views[self.active].models.hrrr_by_timeline = false;
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
        let [a, b] = match self.xsection_pts.as_slice() {
            [a, b] => [*a, *b],
            _ => return,
        };
        let Some(vol) = self.views[idx].volume.as_mut() else {
            return;
        };
        let moment = self.xsection_moment;
        let sweeps = vol.moment_tilts(moment); // owned → the &mut vol borrow ends here
        if sweeps.is_empty() {
            return;
        }
        // The span the contributing tilts were scanned over, from their own radial clocks.
        let span = vol
            .elevations
            .iter()
            .filter_map(|&e| level2::sweep_time_range(&vol.scan, e, moment))
            .fold(
                None,
                |acc: Option<(DateTime<Utc>, DateTime<Utc>)>, (s, e)| {
                    Some(acc.map_or((s, e), |(a, b)| (a.min(s), b.max(e))))
                },
            );
        let source = XsectionSource {
            pane: idx,
            volume: vol.name.clone(),
            revision: vol.revision(),
            moment,
            tilts: sweeps.len(),
            span,
        };
        let Some(xs) = wxdata::xsection::build(&sweeps, (a[0], a[1]), (b[0], b[1]), 300, 120, 18.0)
        else {
            return;
        };
        self.xsection_source = Some(source);
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
        let levels = self
            .env_levels_for(idx)
            .map(env_levels::EnvLevels::column_levels)
            .unwrap_or_default();
        let environment = self
            .env_levels_for(idx)
            .map(env_levels::EnvLevels::describe);
        if levels.h0_m.is_none() {
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
        let gate_inputs = Self::udp_gate_inputs(vol, tilt, lon, lat, antenna_altitude_m, levels);
        let column_inputs = Self::udp_column_inputs(vol, lon, lat, antenna_altitude_m, levels);
        Some(ui::gate_inspector::GateInspectorPopup {
            site,
            vcp,
            moment,
            time_range,
            inspection,
            gate_inputs,
            column_inputs,
            environment,
            // Filled on a click only (map_click): the hover probe keeps just the value.
            series: Vec::new(),
            display: Default::default(),
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
                        _ => {
                            self.mrms_ready_for(idx, *layer)
                                && self.radar_field_ready(idx, *layer)
                                && self
                                    .field_state_for(idx, *layer)
                                    .is_some_and(|state| state.grid.is_some())
                        }
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
            | FL::HailPosh
            | FL::UserColumn
            | FL::UserColumnTrail => "Local radar".into(),
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

    /// Draw a shared geographic point and probe only the hovered pane's cursor group.
    /// The table samples each pane's top visible gridded layer or, when there is none, its radar
    /// moment; every row therefore describes what that pane is actually showing at the crosshair.
    fn paint_linked_cursor(&mut self, ui: &egui::Ui, rects: &[egui::Rect], solo: bool) {
        if rects.len() < 2 {
            return;
        }
        let Some((owner, (lon, lat))) = self.linked_probe else {
            return;
        };
        let members = spatial_groups::cursor_members(&self.views, owner);
        if members.len() < 2 {
            return;
        }
        let world = crate::render::mercator::lonlat_to_world(lon, lat);
        let color = egui::Color32::from_rgb(255, 214, 92);
        let mut rows = Vec::with_capacity(rects.len());
        for (idx, rect) in rects.iter().enumerate() {
            if !members.contains(&idx) {
                continue;
            }
            let vp = (rect.width(), rect.height());
            let screen = self.views[idx].camera.world_to_screen(world, vp);
            let pos = egui::pos2(rect.left() + screen.0, rect.top() + screen.1);
            if rect.contains(pos) && (!solo || idx == self.active) {
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
        let n = pairs.len();
        // When these sweeps were scanned: the original Tornado ID's input clocks.
        let inputs = wxdata::detection_lineage::input_coverage(
            pairs.into_iter().flat_map(|(v, z)| [v, z]).collect(),
        );
        self.couplet_inputs = Some((self.volume_key(idx), inputs));
        Some((hits, (radar_lon, radar_lat), n))
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
            T::Tbss => &mut self.filters.show_tbss,
            T::ZdrColumns => &mut self.filters.show_zdr_columns,
            T::TornadoId => &mut self.filters.show_tornado_id,
            T::YallMode => &mut self.settings.yall_mode,
            T::LayerProbe => &mut self.settings.layer_probe,
            T::Globe => &mut self.settings.globe,
            T::ScaleBar => &mut self.settings.scale_bar,
            T::Alerts => &mut self.filters.show_alerts,
            T::Mds => &mut self.filters.show_mds,
            T::Watches => &mut self.filters.show_watches,
            T::LocalTracks => &mut self.show_local_tracks,
            T::Mping => &mut self.show_mping,
            T::Pireps => &mut self.show_pireps,
            T::Recon => &mut self.show_recon,
            T::LinkCameras => &mut self.views[self.active].spatial_links.camera.enabled,
            T::LinkTimes => &mut self.link_times,
            T::LockSourceTime => &mut self.lock_source_time,
            T::LinkSite => &mut self.views[self.active].spatial_links.site.enabled,
            T::LinkCursor => &mut self.views[self.active].spatial_links.cursor.enabled,
            T::LinkStorm => &mut self.link_storm,
            T::MiniLoop => &mut self.mini_loop,
            T::Blockage => &mut self.show_blockage,
            T::LowestTilt => &mut self.show_lowest_tilt,
            T::DayNight => &mut self.show_daynight,
            T::ImportedGis => &mut self.show_imported_gis,
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
        if !(self.show_storm_reports || self.filters.show_tornado_id) {
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
            let key = (self.contour_model, self.settings.temp_unit);
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
                    OverlaySource::Contours(kind, self.contour_model, self.settings.temp_unit),
                );
            }
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
        let imported_visible = self.gis_zoom_key(zoom);
        let imported_flipped = imported_visible != self.built_imported_visible;
        if should_retess(
            self.gesture_live,
            self.overlay_gen != self.built_gen || theme_changed,
            bucket != self.built_zoom_bucket || theme_changed || imported_flipped,
        ) {
            self.built_imported_visible = imported_visible;
            let mut geom = overlay_build::build_layered(
                &self.overlays,
                zoom,
                self.settings.theme,
                &self.overlay_imported_px(zoom),
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
        // Pointer, wheel, pinch and click handling (`app/pane_input.rs`). `None` where a click
        // opened something that ends this pane's frame, as the early return here used to.
        let Some(response) = self.pane_input(ui, ctx, idx, prect, vp) else {
            return;
        };

        // Tiles, fields, the radar and the 3D volume handed to the GPU (`app/pane_gpu.rs`); the
        // city and town labels it gathered come back for the overlay pass.
        let vlabels = self.pane_gpu(
            ui,
            ctx,
            idx,
            prect,
            vp,
            (clear_tiles, clear_vector, first, last),
            (pane_style, is_raster, is_vector),
        );
        let cam = self.views[idx].camera;

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

        // This pane's detections (`app/pane_detect.rs`): nowcast, debris, hail, ZDR columns,
        // couplets, tracks and the merged tornado detections, filtered to the layers shown.
        let PaneDetections {
            nowcast_pts,
            tds_hits,
            tds_score_tracks,
            tbss_hits,
            zdr_hits,
            couplets,
            rot_score_tracks,
            local_tracks,
            tornado_ids,
            circulations,
            original_circulations,
            tornado_lineage,
            tied_couplet,
            tied_tds,
            all_couplets,
            all_tds,
            llsd,
        } = self.pane_detections(ctx, idx);

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
        let mut labels = std::mem::take(&mut self.labels);
        self.paint_place_labels(
            &painter,
            prect,
            cam,
            vp,
            is_vector,
            basemap,
            &vlabels,
            &mut labels,
        );
        self.labels = labels;

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

        // Wind streamlines and barbs, when asked for: the same east/north grid the particles fly
        // on. Streamlines go under the barbs, which carry the values.
        let streams = self.settings.wind_streamlines;
        if self.show_wind && (self.settings.wind_barbs || streams) {
            let alpha = self.wind_alpha(idx, cam.zoom);
            if let (Some(field), true) = (self.wind.as_ref(), alpha > 0.01) {
                if streams {
                    let source = (field.u.values.as_ptr() as usize, field.valid().timestamp());
                    let lines = self.wind_streams.entry((idx, false)).or_default().get(
                        &cam,
                        vp,
                        source,
                        || {
                            crate::wind_streamlines::streamlines(
                                |lon, lat| field.sample(lon, lat),
                                &cam,
                                vp,
                                crate::wind_streamlines::STREAM_SPACING_PX,
                            )
                        },
                    );
                    let (lines, stale) = lines;
                    if stale {
                        painter
                            .ctx()
                            .request_repaint_after(std::time::Duration::from_millis(220));
                    }
                    crate::wind_streamlines::paint(&painter, prect.left_top(), &lines, alpha);
                }
                if self.settings.wind_barbs {
                    let barbs =
                        crate::wind_draw::barbs(field, &cam, vp, crate::wind_draw::BARB_SPACING_PX);
                    crate::wind_draw::paint_barbs(&painter, prect.left_top(), &barbs, alpha);
                }
            }
        }
        // A wind picked in the model field browser draws over its speed, from the components
        // staged with that speed (so the two never disagree on run or lead): as streamlines
        // when they are on, barbs otherwise.
        if let Some(uv) = self.model_wind_for(idx) {
            if streams {
                let source = (std::sync::Arc::as_ptr(&uv) as usize, uv.0.time.timestamp());
                let lines =
                    self.wind_streams
                        .entry((idx, true))
                        .or_default()
                        .get(&cam, vp, source, || {
                            crate::wind_streamlines::streamlines(
                                |lon, lat| {
                                    Some((
                                        uv.0.sample_bilinear(lon, lat)?,
                                        uv.1.sample_bilinear(lon, lat)?,
                                    ))
                                },
                                &cam,
                                vp,
                                crate::wind_streamlines::STREAM_SPACING_PX,
                            )
                        });
                let (lines, stale) = lines;
                if stale {
                    painter
                        .ctx()
                        .request_repaint_after(std::time::Duration::from_millis(220));
                }
                crate::wind_streamlines::paint(&painter, prect.left_top(), &lines, 0.95);
            } else {
                let barbs = crate::wind_draw::barbs_uv(
                    &uv.0,
                    &uv.1,
                    &cam,
                    vp,
                    crate::wind_draw::BARB_SPACING_PX,
                );
                crate::wind_draw::paint_barbs(&painter, prect.left_top(), &barbs, 0.95);
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
            .goes_footprint_for(idx)
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
            let reading = self.goes_sector_for_pane(idx) == sector;
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
        self.paint_model_contours(&painter, prect, cam, vp, idx);
        self.paint_ensemble_spaghetti(&painter, prect, cam, vp, idx);

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
            || !original_circulations.is_empty()
            || !nowcast_pts.is_empty()
            || !local_tracks.is_empty()
            || (self.filters.show_zdr_columns && idx == self.active);
        if cells_here || detectors {
            let (debris_inputs, rotation_inputs) =
                (self.debris_inputs(idx), self.rotation_inputs(idx));
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
                    original_circulations: &original_circulations,
                    tornado_lineage: tornado_lineage.as_ref(),
                    debris_inputs: debris_inputs.as_ref(),
                    rotation_inputs: rotation_inputs.as_ref(),
                    tied_tds: &tied_tds,
                    tied_couplet: &tied_couplet,
                    all_couplets: &all_couplets,
                    all_tds: &all_tds,
                    tds_score_tracks: &tds_score_tracks,
                    rot_score_tracks: &rot_score_tracks,
                    llsd: &llsd,
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
        self.paint_pireps(&painter, prect, cam, vp, &response);

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
        self.paint_webcams(&painter, prect, cam, vp);

        // Live stations: a dot per station, warm where it is hot and cool where it is not, so a
        // boundary reads off the map before any card is open. Clicking one opens its card.
        self.paint_live_stations(&painter, prect, cam, vp);

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

        // Only a click is stored. Every pane draws before the click is read (`ui_frame`), so
        // storing "no click" too would let the next pane erase this one's.
        if let Some(sp) = self.paint_spotters(&painter, prect, cam, vp, idx, &response) {
            self.pending_spotter = Some(sp);
        }

        // ProbSevere per-storm probability badges (polygons draw via the overlay pipeline).
        self.paint_probsevere(&painter, prect, cam, vp);

        // Warning intelligence: warned-storm motion vector + projected path + ETA to markers, and
        // a pulsing outline on escalated (Tornado Emergency / PDS / destructive) warnings.
        self.paint_alert_polygons(ctx, &painter, prect, cam, vp, idx);

        // Surface obs (METAR station plots): fltCat-colored circle, wind barb, T/Td in °F.
        // The label placer is lent apart from `self`, whose pane view is borrowed meanwhile.
        let mut labels = std::mem::take(&mut self.labels);
        self.paint_metar_plots(&painter, prect, cam, vp, &response, &mut labels);
        self.labels = labels;
        // River flood gauges (NWPS): category-colored inverted-triangle droplet + stage tooltip.
        // An interrogate click opens the gauge's card (`ui::gauge_card`): hydrograph, flood
        // stages, crests. A gauge forecast to reach a worse category than it is in now wears a
        // ring in that category's color, and a gauge with its card open a white one.
        let mut labels = std::mem::take(&mut self.labels);
        self.paint_gauges(&painter, prect, cam, vp, &response, &mut labels);
        self.labels = labels;

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
        self.paint_hrrr_key(ui, &painter, prect, idx);

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
        self.paint_placefile_labels(&painter, prect, cam, vp, &response, placefile_labels);

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
        self.paint_range_rings(&painter, prect, cam, vp, idx);

        // Scan age: a ring at the edge of the sweep, coloured by how long before the newest data
        // each azimuth was collected. The antenna takes minutes to turn, so the two edges of a
        // picture can be a rotation apart; this is where.
        self.paint_scan_age(&painter, prect, cam, vp, idx);

        // Radar sites: a ring per site — both networks, so a TDWR you can select is a TDWR you can
        // see. The active site in accent, others muted. IDs only when zoomed in so the CONUS view
        // isn't cluttered. Click handled in the Interrogate tool.
        let mut labels = std::mem::take(&mut self.labels);
        self.paint_radar_sites(&painter, prect, cam, vp, idx, &mut labels);
        self.labels = labels;

        // Location markers.
        self.paint_location_markers(&painter, prect, cam, vp);

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
            if let (Some(site), Some((_, frame))) = (
                v.site.as_deref().and_then(wxdata::sites::site_by_id),
                self.iso_mesh[idx]
                    .as_ref()
                    .filter(|(key, _)| self.current_iso_key(idx).as_ref() == Some(key)),
            ) {
                let table =
                    crate::colormap::effective_table(&self.palettes, moment, self.settings.theme);
                let shells = &frame.shells;
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
        self.paint_3d_mrms_surface(&painter, prect, cam, vp, idx);

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
        self.paint_3d_cloud_tops(&painter, prect, cam, vp, idx);

        // The HRRR's 0, -10 and -20 °C heights as surfaces (ROADMAP_NEW H6): one colour per level,
        // with a *dashed* grid, the forecast's own look — apart from the observed radar, the
        // solid-gridded MRMS analysis and the ungridded satellite sheet.
        self.paint_3d_isotherms(&painter, prect, cam, vp, idx);

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
        self.paint_3d_cell_columns(&painter, prect, cam, vp, idx);

        // Beam guides over the 3D map (ROADMAP_NEW H5): the geometry every observed gate sits on.
        self.paint_3d_beam_guides(&painter, prect, cam, vp, idx);

        // Routes (ROADMAP_NEW L2): alternatives thin and grey, the chosen one wide and blue over a
        // dark casing so it reads over radar, and the waypoints lettered.
        self.paint_routes(&painter, prect, cam, vp);

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
        self.paint_gis_marks(&painter, prect, cam, vp);
        self.paint_gis_selection(&painter, prect, cam, vp);
        // Labels from the chosen attribute (I4), for every geometry family. Decluttered on a
        // coarse screen grid in file order: a label whose cell is taken is skipped, so a dense
        // file reads as a scatter of names rather than an unreadable smear, and more appear as
        // the map zooms in.
        self.paint_gis_labels(&painter, prect, cam, vp);

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
        self.paint_measure(&painter, prect, cam, vp, idx);

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
            self.paint_xsection_handles(&painter, screen);
        }

        // The 3D map's own vertical clip plane (H4's "cross-section line visible in map pane"):
        // ties the 3D view's cut back to geographic context instead of leaving it visible only
        // from inside the 3D view itself. Only meaningful for the Smooth representations —
        // ObservedSweeps raymarches real gate instances with no box for a plane to cut into.
        let map3d = &self.views[idx].map_3d;
        if map3d.enabled && map3d.representation != Map3dRepresentation::ObservedSweeps {
            if let (Some(plane), Some((_, _, half_km, _, _))) =
                (map3d.plane, self.smooth_vol_dims[idx])
            {
                // The plane cuts the box, which a region of interest centres off the radar.
                if let Some(center) = self.smooth_box_center(idx) {
                    let (a, b) = crate::render3d::plane_ground_track(plane, center, half_km);
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
        self.paint_phone_legend(ui, &painter, prect, idx);
        self.paint_legend(&painter, prect, idx);
        self.paint_stale_badge(&painter, prect, idx);
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
        self.link_all_cameras();
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
        self.link_all_cameras();
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
        let entries = self.source_entries();
        ui::source_health_window::active_health_rows(&entries)
            .into_iter()
            .map(DiagnosticsSourceHealth::from)
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

    /// ROADMAP_NEW I6: write everything currently drawn on the map out as one GeoJSON file.
    ///
    /// Deliberately "what is on the map" rather than "everything fetched": `self.overlays` is
    /// already the filtered, toggled set `rebuild_overlays` assembled for display, so an export
    /// matches what the user is looking at instead of quietly carrying layers they had turned off.
    fn export_map_geojson(&mut self) {
        let mut features = self.map_export_features();
        // The displayed reflectivity sweep's threshold edges, as lines with their scan's metadata.
        features.extend(self.radar_outline_features());
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
        gate_alpha: Vec::new(),
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
type RouteExposure = (RouteExposureKey, Vec<(String, f64, f64, bool)>, Vec<String>);
type RouteExposureKey = (
    u64,
    u64,
    i64,
    usize,
    i64,
    (usize, Option<DateTime<Utc>>, Vec<(MrmsContext, u64)>),
);

/// Eight-point compass name for a bearing.
fn compass8(deg: f64) -> &'static str {
    const N: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    N[(((deg.rem_euclid(360.0) + 22.5) / 45.0) as usize) % 8]
}

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
#[cfg(test)]
pub(crate) fn goes_slot(
    at: Option<DateTime<Utc>>,
    sector: wxdata::goes_abi::Sector,
) -> Option<i64> {
    at.map(|t| t.timestamp().div_euclid(sector.cadence_secs() as i64))
}

/// Whether a GOES layer's grid can be painted for a view at `target` (`None`: live): only one
/// fetched for the slot now wanted, and, scrubbed back, a frame within `tolerance` of the target.
/// So an old live frame never sits over an archive scan, nor an archive frame over live radar.
#[cfg(test)]
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

/// Interpolate a 256-entry RGBA LUT from `(t, [r,g,b])` stops; index 0 is always transparent.
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
        self.ensemble.field.snap_lead(self.comparison_fcst_hour)
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
        match crate::ensemble_layer::display_grid(&run.members, &view) {
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
    radar_probe::display_units(moment, settings.velocity_unit)
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
        ui.small(format!("Evidence score, {} volumes", tr.points.len()));
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
        // Touch: a press-and-hold pins the tooltip it showed until the next touch, so every
        // hover reading in the app can be read with a finger (`touch_hover`).
        let now = raw_input.time.unwrap_or_else(|| ctx.input(|i| i.time));
        let id = egui::Id::new("touch_hover");
        let mut hold: touch_hover::TouchHold = ctx.data_mut(|d| d.get_temp(id).unwrap_or_default());
        touch_hover::pin_long_press_hover(&mut hold, raw_input, now);
        ctx.data_mut(|d| d.insert_temp(id, hold));
    }

    fn ui(&mut self, root: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Timed from outside so every early return inside is counted too.
        let start = wxdata::clock::Instant::now();
        self.ui_frame(root, frame);
        self.frame_times
            .push(start.elapsed().as_secs_f32() * 1000.0);
        self.note_alert_frame();
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

        self.frame_intake(ctx);
        spatial_groups::sync_sites(&mut self.views, self.active);

        self.drive_snapshot_push(ctx);
        self.drive_widget_snapshot(ctx);
        self.save_pending_screenshot(ctx);
        self.load_marker_icons(ctx);
        self.drive_loop_export(ctx);
        self.apply_chase();
        self.sync_share(ctx);
        self.poll_sync();
        self.sync_model_groups();
        self.sync_forecast_scrub();
        self.drive_model_timeline(ctx);
        self.poll_messages();
        self.poll_overlays();
        // Time-machine warnings + storm reports: swap in archived sets while scrubbed.
        self.sync_archive_warnings(ctx);
        self.sync_archive_lsr(ctx);
        // Every feed and layer whose cadence or view says it is due (`app/fetch_schedule.rs`).
        self.schedule_fetches(ctx);
        // Bindings are polled once, globally: a hotkey works the same in OBS mode, on mobile, and
        // with the drawer open. `capture_key` suppresses the table while the Hotkeys tab is
        // listening for the next keypress.
        if !self.capture_key {
            self.storm_track_keys(ctx);
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
        // Streaming mode draws no chrome, but Back still has to reach it: it is the way out.
        if bare && cfg!(target_os = "android") {
            self.android_back(ctx);
        }
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

        // Floating windows (`app/floating_windows.rs`).
        self.floating_windows(ctx, root, dock_layout);

        self.sync_model_groups();
        self.draw_panes(ctx, root);

        self.frame_end(ctx, root);
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
mod quiet_summary_tests;

#[cfg(test)]
mod humanize_tests;

#[cfg(test)]
mod place_name_tests;

#[cfg(test)]
mod follow_tests;

#[cfg(test)]
mod warning_scope_tests;

#[cfg(test)]
mod field_lut_tests;

#[cfg(test)]
mod tests;

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
mod archive_calendar_tests;

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
mod nowcast_tests;

#[cfg(test)]
mod probe_grid_tests;

#[cfg(test)]
mod comparison_display_tests;

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

#[cfg(test)]
mod trail_status_tests;

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
mod two_finger_slide_tests;
