//! The docked-panel layout ("Dock (ImGui)" under Appearance → Theme): an analyst workstation for
//! a tablet or a desktop that wants every control on screen at once. See
//! `docs/WSV3_IMGUI_MODERN_DESIGN_PLAN.md` for the design and the reasons behind it.
//!
//! Top to bottom: an app bar (workspace tabs, panel buttons, clock), a context toolbar of radar
//! controls, the Layers panel and the tool rail beside the map, a floating Inspector card over the
//! map, and the timeline under everything.
//!
//! It is a second *presentation* of things that already exist, not a second set of features. The
//! layers tree is the command registry (`palette_entries`) grouped by category; every checkbox and
//! button goes through the same palette action as every other surface, and the timeline drives the
//! same `Timeline`. So a layer switched on here is the layer switched on in the ribbon, the palette
//! and the phone sheet. Colours and sizes come from `ui::workstation`; nothing here sets one inline.

use super::*;
use crate::ui::workstation as ws;
use crate::workspace::{Place, WindowChrome, WorkstationChrome};

mod alerts;
mod app_bar;
mod cell;
mod footer;
mod gauges;
mod inspector;
mod layers;
mod log;
mod menus;
mod order;
mod phone;
mod prefs;
mod rail;
mod region;
mod settings;
mod sounding;
mod sources;
mod storm_associations;
mod storms;
mod timeline;
mod view3d;
mod volume;

/// Width of the Layers panel.
const LEFT_WIDTH: f32 = 284.0;

/// How narrow and how wide a dragged dock may get. The upper bound is also held under half the
/// window, so a dock can never take the map.
const DOCK_MIN_W: f32 = 240.0;
const DOCK_MAX_W: f32 = 560.0;

/// The bottom dock's default height, and how short and how tall a drag may make it. The upper
/// bound is also held under half the window's height, so the map keeps the rest.
const BOTTOM_H: f32 = 240.0;
const BOTTOM_MIN_H: f32 = 120.0;
const BOTTOM_MAX_H: f32 = 520.0;

/// Below this window width only one side dock shows at a time (design plan §9, "laptop / tablet
/// landscape"): a Layers dock, a right dock and the rail together would leave the map a strip.
/// It is the two docks and the rail plus a map about as wide as either dock pair.
const ONE_DOCK_BELOW: f32 = 1120.0;

/// The app bar's workspace tabs. Each is a view of the Layers panel: which registry categories it
/// lists, and for Models and Analysis the controls drawn above the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum DockTab {
    #[default]
    Radar,
    /// The model browser, then the forecast-model layers.
    Models,
    /// National grids: satellite, MRMS, the national fields.
    Satellite,
    /// Observations and severe weather.
    Surface,
    /// The tools, under the settings of the layers that are on.
    Analysis,
    /// Map reference layers, and importing your own.
    Gis,
}

impl DockTab {
    pub(crate) const ALL: [DockTab; 6] = [
        DockTab::Radar,
        DockTab::Models,
        DockTab::Satellite,
        DockTab::Surface,
        DockTab::Analysis,
        DockTab::Gis,
    ];

    /// A tab by its label, as a saved arrangement names it.
    pub(crate) fn from_label(label: &str) -> Option<DockTab> {
        DockTab::ALL.into_iter().find(|t| t.label() == label)
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            DockTab::Radar => "Radar",
            DockTab::Models => "Models",
            DockTab::Satellite => "Satellite",
            DockTab::Surface => "Surface",
            DockTab::Analysis => "Analysis",
            DockTab::Gis => "GIS",
        }
    }

    /// The registry categories this tab lists.
    pub(crate) fn categories(self) -> &'static [&'static str] {
        match self {
            DockTab::Radar => &["Radar", "Sites"],
            DockTab::Models => &["Models"],
            DockTab::Satellite => &["National"],
            DockTab::Surface => &["Obs", "Severe"],
            DockTab::Analysis => &["Tools"],
            DockTab::Gis => &["Reference"],
        }
    }
}

/// The Layers panel's segmented filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LayerFilter {
    /// Everything in the tab's categories.
    #[default]
    All,
    /// Only what is switched on, in every category.
    Active,
    /// The layers starred as favourites (`Settings::favorite_layers`), in every category.
    Favorites,
}

/// A reading of the displayed sweep at a point, for the Inspector card.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Probe {
    pub lon: f64,
    pub lat: f64,
    pub value: Option<f32>,
    pub folded: bool,
    pub azimuth_deg: f32,
    pub range_km: f32,
    pub beam_ft: f64,
    pub collected_ms: Option<i64>,
    /// The sweep's estimated Nyquist velocity, m/s (velocity only).
    pub nyquist_mps: Option<f32>,
    /// The value came from the dealiased sweep.
    pub dealiased: bool,
}

/// The workstation's tool windows, in the order a dock's tab group lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum DockWin {
    Layers,
    Inspector,
    Alerts,
    Prefs,
    Settings,
    /// The active pane's 3D controls (only while it is in 3D).
    View3d,
    /// Every active feed's health, compactly.
    Sources,
    /// Analyst Mode's live log (only while Analyst Mode is on).
    Log,
    /// The point sounding (once a point has been sounded).
    Sounding,
    /// Every storm cell, ranked.
    Storms,
    /// The selected storm's full details (once Details… is asked for).
    Cell,
    /// The region-statistics tool's box: summary, histogram and scatter.
    Region,
    /// The orbitable 3D reflectivity volume (the floating "3D Reflectivity" window elsewhere).
    Volume,
    /// Every river gauge in view: flood categories, sparklines, the selected gauge's hydrograph.
    Gauges,
}

impl DockWin {
    pub(crate) const ALL: [DockWin; 14] = [
        DockWin::Layers,
        DockWin::Inspector,
        DockWin::View3d,
        DockWin::Sounding,
        DockWin::Storms,
        DockWin::Cell,
        DockWin::Region,
        DockWin::Volume,
        DockWin::Gauges,
        DockWin::Alerts,
        DockWin::Sources,
        DockWin::Log,
        DockWin::Prefs,
        DockWin::Settings,
    ];

    /// The glyph and plain title a dock's tab shows (the window's own header may add a count).
    fn tab(self) -> ws::HeaderTab {
        use egui_phosphor::regular as ph;
        let (glyph, title) = match self {
            DockWin::Layers => (ph::STACK, "Layers"),
            DockWin::Inspector => (ph::INFO, "Inspector"),
            DockWin::Alerts => (ph::BELL, "Alerts"),
            DockWin::Prefs => (ph::SLIDERS_HORIZONTAL, "Preferences"),
            DockWin::Settings => (ph::GEAR, "Settings"),
            DockWin::View3d => (ph::CUBE, "3D view"),
            DockWin::Sources => (ph::PULSE, "Sources"),
            DockWin::Log => (ph::TERMINAL_WINDOW, "Analyst log"),
            DockWin::Sounding => (ph::THERMOMETER, "Sounding"),
            DockWin::Storms => (ph::TORNADO, "Storms"),
            DockWin::Cell => (ph::CROSSHAIR, "Cell"),
            DockWin::Region => (ph::CHART_SCATTER, "Region"),
            DockWin::Volume => (ph::CUBE, "3D volume"),
            DockWin::Gauges => (ph::DROP, "Gauges"),
        };
        ws::HeaderTab {
            glyph,
            title,
            dot: None,
        }
    }

    /// Its name in a saved arrangement (the tab order): stable, unlike its title.
    pub(crate) fn name(self) -> &'static str {
        match self {
            DockWin::Layers => "Layers",
            DockWin::Inspector => "Inspector",
            DockWin::Alerts => "Alerts",
            DockWin::Prefs => "Prefs",
            DockWin::Settings => "Settings",
            DockWin::View3d => "View3d",
            DockWin::Sources => "Sources",
            DockWin::Log => "Log",
            DockWin::Sounding => "Sounding",
            DockWin::Storms => "Storms",
            DockWin::Cell => "Cell",
            DockWin::Region => "Region",
            DockWin::Volume => "Volume",
            DockWin::Gauges => "Gauges",
        }
    }

    /// The glyph its header, its dock tab and its row in the Layers tree share.
    pub(crate) fn glyph(self) -> &'static str {
        self.tab().glyph
    }

    fn width(self) -> f32 {
        match self {
            DockWin::Layers => LEFT_WIDTH,
            DockWin::Inspector => inspector::CARD_W,
            DockWin::Alerts => alerts::ALERTS_W,
            DockWin::Prefs => prefs::PREFS_W,
            DockWin::Settings => settings::SETTINGS_W,
            DockWin::View3d => view3d::VIEW3D_W,
            DockWin::Sources => sources::SOURCES_W,
            DockWin::Log => log::LOG_W,
            DockWin::Sounding => sounding::SOUNDING_W,
            DockWin::Storms => storms::STORMS_W,
            DockWin::Cell => cell::CELL_W,
            DockWin::Region => region::REGION_W,
            DockWin::Volume => volume::VOLUME_W,
            DockWin::Gauges => gauges::GAUGES_W,
        }
    }
}

/// A saved tab order back into windows: the named ones first, in that order, then every other
/// window in its usual place. Unknown names (a window a later version dropped) are skipped.
fn tab_order_from(names: &[String]) -> Vec<DockWin> {
    let mut v: Vec<DockWin> = names
        .iter()
        .filter_map(|n| DockWin::ALL.into_iter().find(|w| w.name() == n))
        .collect();
    v.dedup();
    for w in DockWin::ALL {
        if !v.contains(&w) {
            v.push(w);
        }
    }
    v
}

/// The widest a dock may be dragged in a window this wide: [`DOCK_MAX_W`], and never more than
/// 45% of the window.
/// Runs a dock's contents in a child its panel does not grow around. A right panel whose
/// content runs wider than it is re-sized from its outer (screen) edge, so one over-wide row
/// would move its inner edge inward and hand the column to the map; contained, the row is
/// clipped at the column instead.
fn contained<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let inner = ui.max_rect();
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(inner.intersect(ui.clip_rect()));
    add(&mut child)
}

fn dock_max_width(window_w: f32) -> f32 {
    DOCK_MAX_W.min(window_w * 0.45)
}

/// Index of a docked side in [`DockState::front`].
fn side_slot(place: Place) -> Option<usize> {
    match place {
        Place::Left => Some(0),
        Place::Right => Some(1),
        Place::Bottom => Some(2),
        Place::Float => None,
    }
}

/// The docks, in the order they are laid out.
const DOCKS: [Place; 3] = [Place::Left, Place::Right, Place::Bottom];

/// The tallest the bottom dock may be in a window this tall: [`BOTTOM_MAX_H`], and never more than
/// half the window.
fn bottom_max_height(window_h: f32) -> f32 {
    BOTTOM_MAX_H.min(window_h * 0.5)
}

/// A shape as well as a color for every feed state: the compact lists stay readable when color
/// cannot distinguish their status marks. The full word remains in the hover and accessible name.
pub(super) fn health_glyph(state: HealthState) -> &'static str {
    use egui_phosphor::regular as ph;
    match state {
        HealthState::Fresh => ph::CHECK_CIRCLE,
        HealthState::Fetching => ph::ARROWS_CLOCKWISE,
        HealthState::Delayed => ph::CLOCK,
        HealthState::Stale => ph::WARNING_CIRCLE,
        HealthState::Cached => ph::DATABASE,
        HealthState::Failed => ph::X_CIRCLE,
        HealthState::Waiting => ph::HOURGLASS,
    }
}

/// How far the phone's bottom sheet is pulled up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Sheet {
    /// Only the handle and the tab strip: the map gets the screen.
    Peek,
    /// About a third of the screen: the Inspector under the map, as in the mock.
    #[default]
    Half,
    /// Most of the screen, for reading a long window (Layers, a bulletin, the gauges).
    Full,
}

/// The windows a phone's bottom sheet lists, in its tab order: the everyday ones first.
pub(crate) const PHONE_ORDER: [DockWin; 14] = [
    DockWin::Inspector,
    DockWin::Layers,
    DockWin::Storms,
    DockWin::Cell,
    DockWin::Alerts,
    DockWin::Gauges,
    DockWin::Sounding,
    DockWin::Region,
    DockWin::Volume,
    DockWin::View3d,
    DockWin::Sources,
    DockWin::Log,
    DockWin::Prefs,
    DockWin::Settings,
];

/// The sheet's standing tabs on a phone: always there, so closing one folds the sheet instead.
pub(crate) const PHONE_CORE: [DockWin; 4] = [
    DockWin::Inspector,
    DockWin::Layers,
    DockWin::Storms,
    DockWin::Alerts,
];

/// Which page the Preferences window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PrefsPage {
    /// Background, radar appearance, launch position, offline maps.
    #[default]
    Map,
    /// Display, location, weather radio, share, backup, help.
    App,
}

/// Everything the dock remembers between frames.
pub(crate) struct DockState {
    pub tab: DockTab,
    pub filter: LayerFilter,
    pub query: String,
    /// Put the keyboard in the Layers search box on the next frame (Ctrl+K, the palette's search).
    pub focus_search: bool,
    pub layers: WindowChrome,
    pub inspector: WindowChrome,
    pub alerts: WindowChrome,
    pub prefs: WindowChrome,
    pub settings: WindowChrome,
    pub settings_action: Option<crate::ui::settings_window::SyncAction>,
    pub view3d: WindowChrome,
    pub sources: WindowChrome,
    pub log: WindowChrome,
    pub sounding: WindowChrome,
    pub storms: WindowChrome,
    pub cell: WindowChrome,
    /// Whether the Cell window has a storm whose details were asked for. Set each frame.
    pub cell_available: bool,
    pub region: WindowChrome,
    /// Whether a region box has been gathered. Set each frame.
    pub region_available: bool,
    pub volume: WindowChrome,
    pub gauges: WindowChrome,
    /// Whether the 3D volume is open (`show_3d`). Set each frame.
    pub volume_available: bool,
    /// The Storms table's sort column and direction.
    pub storm_sort: crate::ui::cells_window::SortCol,
    pub storm_desc: bool,
    /// Session-only filter for the Storms table, independent of the Layers search.
    pub storm_query: String,
    /// Whether a point has been sounded (the sounding window's own `open`). Set each frame.
    pub sounding_available: bool,
    /// Whether the Analyst log has anything to show: Analyst Mode is on. Set each frame.
    pub log_available: bool,
    /// Whether the 3D view window has anything to show: the active pane is in 3D. Set each frame
    /// before the docks are laid out.
    pub view3d_available: bool,
    pub prefs_page: PrefsPage,
    pub timeline_open: bool,
    /// The status footer under the timeline.
    pub footer_open: bool,
    /// Frame time in milliseconds, smoothed, for the footer.
    pub frame_ms: f32,
    /// The inspector's model-forecast block (shown only while a model layer is on the map).
    pub model_open: bool,
    /// A pinned reading: kept on the card after the pointer leaves the map.
    pub pinned: Option<Probe>,
    /// The last reading taken under the pointer, which the card keeps showing (and a pin keeps)
    /// once the pointer has left the map for the card.
    pub last: Option<Probe>,
    /// The "Jump to…" field's text.
    pub jump: String,
    /// The layout whose saved arrangement is in effect; a different layout loads its own.
    pub arranged_for: Option<crate::settings::Layout>,
    /// The window in front of each dock's tab group, left then right, when more than one window
    /// shares that side.
    pub front: [Option<DockWin>; 3],
    /// Each window's `(open, place)` last frame, to bring a window that has just opened or just
    /// moved into a dock to the front of it.
    seen: [(bool, Place); DockWin::ALL.len()],
    /// The window is too narrow for docks on both sides ([`ONE_DOCK_BELOW`]). Set each frame.
    pub narrow: bool,
    /// The side used most recently (0 left, 1 right): the one that stays while `narrow`.
    last_side: usize,
    /// Each dock's dragged width, left then right (`None`: its windows' own width).
    pub dock_widths: [Option<f32>; 2],
    /// The bottom dock's dragged height (`None`: [`BOTTOM_H`]).
    pub bottom_h: Option<f32>,
    /// A window just torn out of a dock, and where to put it: under the pointer that tore it.
    pending_float: Option<(DockWin, egui::Pos2)>,
    /// The windows' tab order, as dragged: every window once.
    pub tab_order: Vec<DockWin>,
    /// Where each dock (left, right, bottom) was drawn last frame, for Ctrl+Tab to find the one
    /// under the pointer.
    rects: [Option<egui::Rect>; 3],
    /// The alert whose bulletin the Alerts window last brought forward, so a new one (a click on
    /// a warning polygon) brings it forward once and the analyst's own tab choice then holds.
    pub bulletin_seen: Option<String>,
    /// The storms open in the Cell window, oldest first (at most [`cell::MAX_OPEN`]).
    pub cells_open: Vec<String>,
    /// The Cell window shows its open storms side by side.
    pub cell_compare: bool,
    /// The storm selected last frame, so a newly selected one joins the Cell window once.
    cell_last_sel: Option<String>,
    /// On a phone (the Station design): every window is a tab of one bottom sheet rather than
    /// docked to a side. Set each frame.
    pub phone: bool,
    /// The window in front of the phone's sheet.
    pub phone_front: Option<DockWin>,
    /// How far the phone's sheet is pulled up.
    pub sheet: Sheet,
}

impl Default for DockState {
    fn default() -> Self {
        let mut s = Self {
            tab: DockTab::Radar,
            filter: LayerFilter::All,
            query: String::new(),
            focus_search: false,
            layers: WindowChrome::default(),
            inspector: WindowChrome::default(),
            alerts: WindowChrome::default(),
            prefs: WindowChrome::default(),
            settings: WindowChrome::at(false, Place::Float),
            settings_action: None,
            view3d: WindowChrome::default(),
            sources: WindowChrome::default(),
            log: WindowChrome::default(),
            log_available: false,
            sounding: WindowChrome::default(),
            sounding_available: false,
            storms: WindowChrome::default(),
            cell: WindowChrome::at(true, Place::Right),
            cell_available: false,
            region: WindowChrome::at(true, Place::Right),
            region_available: false,
            volume: WindowChrome::at(true, Place::Float),
            gauges: WindowChrome::at(false, Place::Right),
            volume_available: false,
            storm_sort: Default::default(),
            storm_desc: true,
            storm_query: String::new(),
            view3d_available: false,
            prefs_page: PrefsPage::Map,
            timeline_open: true,
            footer_open: false,
            frame_ms: 0.0,
            model_open: true,
            pinned: None,
            last: None,
            jump: String::new(),
            arranged_for: None,
            front: [None; 3],
            seen: [(false, Place::Float); DockWin::ALL.len()],
            narrow: false,
            last_side: 0,
            dock_widths: [None; 2],
            bottom_h: None,
            pending_float: None,
            tab_order: DockWin::ALL.to_vec(),
            rects: [None; 3],
            bulletin_seen: None,
            cells_open: Vec::new(),
            cell_compare: false,
            cell_last_sel: None,
            phone: false,
            phone_front: None,
            sheet: Sheet::Half,
        };
        s.arrange(&DockState::preset(crate::settings::Layout::Dock));
        s
    }
}

impl DockState {
    /// How a layout opens before the user has moved anything. `Dock` shows its windows — Layers
    /// docked left, the Inspector floating over the map as in the reference mock. `Wsv3` opens
    /// map-first: the bars and the timeline, with Layers (left) and the Inspector (docked right)
    /// waiting to be asked for. Alerts and Preferences dock right when opened, in either.
    pub(crate) fn preset(layout: crate::settings::Layout) -> WorkstationChrome {
        let map_first = layout.map_first();
        WorkstationChrome {
            tab: DockTab::Radar.label().to_string(),
            layers: WindowChrome::at(!map_first, Place::Left),
            inspector: WindowChrome::at(
                !map_first,
                if map_first {
                    Place::Right
                } else {
                    Place::Float
                },
            ),
            alerts: WindowChrome::at(false, Place::Right),
            prefs: WindowChrome::at(false, Place::Right),
            settings: WindowChrome::at(false, Place::Float),
            view3d: WindowChrome::at(true, Place::Right),
            sources: WindowChrome::at(false, Place::Right),
            log: WindowChrome::at(true, Place::Right),
            dock_widths: [None; 2],
            bottom_h: None,
            tab_order: Vec::new(),
            sounding: WindowChrome::at(true, Place::Right),
            storms: WindowChrome::at(false, Place::Right),
            cell: WindowChrome::at(true, Place::Right),
            region: WindowChrome::at(true, Place::Right),
            volume: WindowChrome::at(true, Place::Float),
            gauges: WindowChrome::at(false, Place::Right),
            timeline_open: true,
            footer_open: false,
        }
    }

    /// The window arrangement as saved with a workspace and in the settings.
    pub(crate) fn arrangement(&self) -> WorkstationChrome {
        WorkstationChrome {
            tab: self.tab.label().to_string(),
            layers: self.layers,
            inspector: self.inspector,
            alerts: self.alerts,
            prefs: self.prefs,
            settings: self.settings,
            view3d: self.view3d,
            sources: self.sources,
            log: self.log,
            dock_widths: self.dock_widths.map(|w| w.map(|w| w.round() as u16)),
            bottom_h: self.bottom_h.map(|h| h.round() as u16),
            tab_order: if self.tab_order == DockWin::ALL {
                Vec::new()
            } else {
                self.tab_order
                    .iter()
                    .map(|w| w.name().to_string())
                    .collect()
            },
            sounding: self.sounding,
            storms: self.storms,
            cell: self.cell,
            region: self.region,
            volume: self.volume,
            gauges: self.gauges,
            timeline_open: self.timeline_open,
            footer_open: self.footer_open,
        }
    }

    /// Put the windows where a saved arrangement says.
    pub(crate) fn arrange(&mut self, w: &WorkstationChrome) {
        self.tab = DockTab::from_label(&w.tab).unwrap_or_default();
        self.layers = w.layers;
        self.inspector = w.inspector;
        self.alerts = w.alerts;
        self.prefs = w.prefs;
        self.settings = w.settings;
        self.view3d = w.view3d;
        self.sources = w.sources;
        self.log = w.log;
        self.dock_widths = w.dock_widths.map(|w| w.map(f32::from));
        self.bottom_h = w.bottom_h.map(f32::from);
        self.tab_order = tab_order_from(&w.tab_order);
        self.sounding = w.sounding;
        self.storms = w.storms;
        self.cell = w.cell;
        self.region = w.region;
        self.volume = w.volume;
        self.gauges = w.gauges;
        self.timeline_open = w.timeline_open;
        self.footer_open = w.footer_open;
    }

    pub(crate) fn chrome(&self, w: DockWin) -> &WindowChrome {
        match w {
            DockWin::Layers => &self.layers,
            DockWin::Inspector => &self.inspector,
            DockWin::Alerts => &self.alerts,
            DockWin::Prefs => &self.prefs,
            DockWin::Settings => &self.settings,
            DockWin::View3d => &self.view3d,
            DockWin::Sources => &self.sources,
            DockWin::Log => &self.log,
            DockWin::Sounding => &self.sounding,
            DockWin::Storms => &self.storms,
            DockWin::Cell => &self.cell,
            DockWin::Region => &self.region,
            DockWin::Volume => &self.volume,
            DockWin::Gauges => &self.gauges,
        }
    }

    fn chrome_mut(&mut self, w: DockWin) -> &mut WindowChrome {
        match w {
            DockWin::Layers => &mut self.layers,
            DockWin::Inspector => &mut self.inspector,
            DockWin::Alerts => &mut self.alerts,
            DockWin::Prefs => &mut self.prefs,
            DockWin::Settings => &mut self.settings,
            DockWin::View3d => &mut self.view3d,
            DockWin::Sources => &mut self.sources,
            DockWin::Log => &mut self.log,
            DockWin::Sounding => &mut self.sounding,
            DockWin::Storms => &mut self.storms,
            DockWin::Cell => &mut self.cell,
            DockWin::Region => &mut self.region,
            DockWin::Volume => &mut self.volume,
            DockWin::Gauges => &mut self.gauges,
        }
    }

    /// Whether `w` is open and has something to show (the 3D view needs a pane in 3D, the
    /// Analyst log needs Analyst Mode).
    fn present(&self, w: DockWin) -> bool {
        self.chrome(w).open
            && match w {
                DockWin::View3d => self.view3d_available,
                DockWin::Log => self.log_available,
                DockWin::Sounding => self.sounding_available,
                DockWin::Cell => self.cell_available,
                DockWin::Region => self.region_available,
                DockWin::Volume => self.volume_available,
                _ => true,
            }
    }

    /// The phone sheet's tabs: every present window, in [`PHONE_ORDER`].
    pub(crate) fn phone_stack(&self) -> Vec<DockWin> {
        PHONE_ORDER
            .into_iter()
            .filter(|w| self.present(*w))
            .collect()
    }

    /// Put a window in front of the phone's sheet, pulling a folded sheet up to show it.
    fn phone_show(&mut self, w: DockWin) {
        self.phone_front = Some(w);
        if self.sheet == Sheet::Peek {
            self.sheet = Sheet::Half;
        }
    }

    /// The present windows at `side`, in tab order.
    pub(crate) fn stack(&self, side: Place) -> Vec<DockWin> {
        let mut v: Vec<DockWin> = DockWin::ALL
            .into_iter()
            .filter(|w| self.present(*w) && self.chrome(*w).place == side)
            .collect();
        v.sort_by_key(|w| {
            self.tab_order
                .iter()
                .position(|o| o == w)
                .unwrap_or(usize::MAX)
        });
        v
    }

    /// Whether `w` has something to show now, open or not (the sounding needs a sounded point,
    /// the Cell window a storm's details, ...). The windows menu greys out the ones that do not.
    pub(crate) fn available(&self, w: DockWin) -> bool {
        match w {
            DockWin::View3d => self.view3d_available,
            DockWin::Log => self.log_available,
            DockWin::Sounding => self.sounding_available,
            DockWin::Cell => self.cell_available,
            DockWin::Region => self.region_available,
            DockWin::Volume => self.volume_available,
            _ => true,
        }
    }

    /// Put every window back where the layout starts them, at their usual sizes and in their
    /// usual tab order.
    pub(crate) fn reset_layout(&mut self, layout: crate::settings::Layout) {
        self.arrange(&DockState::preset(layout));
        self.dock_widths = [None; 2];
        self.bottom_h = None;
        self.tab_order = DockWin::ALL.to_vec();
        self.pending_float = None;
    }

    /// Move `w` into a dock (or float it), in front there.
    pub(crate) fn place_window(&mut self, w: DockWin, place: Place) {
        let c = self.chrome_mut(w);
        c.place = place;
        c.collapsed = false;
        self.bring_forward(w);
    }

    /// Ctrl+Tab: the next (or, `back`, the previous) tab of the dock at `side` to the front.
    pub(crate) fn cycle_tab(&mut self, side: Place, back: bool) {
        let stack = self.stack(side);
        let Some(slot) = side_slot(side) else {
            return;
        };
        if stack.len() < 2 {
            return;
        }
        let at = self.front[slot]
            .and_then(|f| stack.iter().position(|w| *w == f))
            .unwrap_or(0);
        let n = stack.len();
        let next = if back { (at + n - 1) % n } else { (at + 1) % n };
        self.front[slot] = Some(stack[next]);
        self.note_side(slot);
    }

    /// The dock Ctrl+Tab steps through: the one under `pointer` (as drawn last frame), else the
    /// side used last.
    fn cycle_target(&self, pointer: Option<egui::Pos2>) -> Place {
        let under = pointer.and_then(|p| {
            DOCKS
                .into_iter()
                .zip(self.rects)
                .find(|(_, r)| r.is_some_and(|r| r.contains(p)))
                .map(|(side, _)| side)
        });
        under.unwrap_or(if self.last_side == 0 {
            Place::Left
        } else {
            Place::Right
        })
    }

    /// Say whether a point has been sounded. A new sounding shows the tab again, and brings it
    /// forward (the analyst just clicked the map for it).
    pub(crate) fn set_sounding_available(&mut self, available: bool) {
        if available && !self.sounding_available {
            self.sounding.open = true;
        }
        self.sounding_available = available;
    }

    /// Say whether a storm's details are wanted: becoming wanted shows the Cell window in front.
    pub(crate) fn set_cell_available(&mut self, available: bool) {
        if available && !self.cell_available {
            self.cell.open = true;
        }
        self.cell_available = available;
    }

    /// Say whether a region box is gathered: a new box shows the Region window in front.
    pub(crate) fn set_region_available(&mut self, available: bool) {
        if available && !self.region_available {
            self.region.open = true;
        }
        self.region_available = available;
    }

    /// Say whether the 3D volume is open: opening it shows its window in front.
    pub(crate) fn set_volume_available(&mut self, available: bool) {
        if available && !self.volume_available {
            self.volume.open = true;
            self.volume.collapsed = false;
        }
        self.volume_available = available;
    }

    /// Say whether Analyst Mode is on. Turning it on shows the log again, as the floating
    /// window always appeared with it.
    pub(crate) fn set_log_available(&mut self, available: bool) {
        if available && !self.log_available {
            self.log.open = true;
        }
        self.log_available = available;
    }

    /// Say whether the active pane is in 3D. Entering 3D reopens the 3D view window even if it
    /// was closed last time, as the floating 3D controls always appeared; closing it hides it
    /// for this visit to 3D only.
    pub(crate) fn set_view3d_available(&mut self, available: bool) {
        if available && !self.view3d_available {
            self.view3d.open = true;
        }
        self.view3d_available = available;
    }

    /// Keep each dock's front tab sensible: a window that has just opened, or just been docked,
    /// comes to the front of its side (opening Alerts from the app bar must show Alerts, not
    /// leave it behind the Inspector); a front window that has left falls back to the first.
    ///
    /// When several arrive on one side at once (a restored arrangement, a workspace), the first
    /// in tab order takes the front, so a dock reopens on its Inspector rather than whichever
    /// window happens to be listed last.
    pub(crate) fn update_fronts(&mut self) {
        if self.phone {
            // One sheet: the first window (in the sheet's order) that has just appeared comes to
            // its front, and a front that has gone falls back to the first tab.
            let mut arrived = Vec::new();
            for (i, w) in DockWin::ALL.into_iter().enumerate() {
                let now = (self.present(w), self.chrome(w).place);
                if now.0 && !self.seen[i].0 {
                    arrived.push(w);
                }
                self.seen[i] = now;
            }
            if let Some(w) = PHONE_ORDER.into_iter().find(|w| arrived.contains(w)) {
                self.phone_front = Some(w);
            }
            let stack = self.phone_stack();
            if !self.phone_front.is_some_and(|f| stack.contains(&f)) {
                self.phone_front = stack.first().copied();
            }
            return;
        }
        let mut claimed = [false; 3];
        for (i, w) in DockWin::ALL.into_iter().enumerate() {
            let c = *self.chrome(w);
            let now = (self.present(w), c.place);
            if now != self.seen[i] && now.0 {
                if let Some(slot) = side_slot(c.place) {
                    if !claimed[slot] {
                        self.front[slot] = Some(w);
                        claimed[slot] = true;
                    }
                    self.note_side(slot);
                }
            }
            self.seen[i] = now;
        }
        for side in DOCKS {
            let stack = self.stack(side);
            let slot = side_slot(side).unwrap_or_default();
            if !self.front[slot].is_some_and(|f| stack.contains(&f)) {
                self.front[slot] = stack.first().copied();
            }
        }
    }

    /// Whether the dock at `side` is drawn: always, unless the window is too narrow for two
    /// docks and the other side, used more recently, has windows too.
    pub(crate) fn side_visible(&self, side: Place) -> bool {
        let Some(slot) = side_slot(side).filter(|s| *s < 2) else {
            // Floating windows and the bottom dock are never set aside.
            return true;
        };
        let other = if slot == 0 { Place::Right } else { Place::Left };
        !self.narrow || self.last_side == slot || self.stack(other).is_empty()
    }

    /// Whether `w` can be seen: open, not behind another window in its dock's tab group, and not
    /// in a dock the narrow window has set aside.
    pub(crate) fn shown(&self, w: DockWin) -> bool {
        let c = self.chrome(w);
        if !self.present(w) {
            return false;
        }
        if self.phone {
            return self.sheet != Sheet::Peek && self.phone_front == Some(w);
        }
        match side_slot(c.place) {
            Some(slot) => {
                self.side_visible(c.place)
                    && (self.stack(c.place).len() < 2 || self.front[slot] == Some(w))
            }
            None => true,
        }
    }

    /// A button for `w` (the app bar's, the rail's, a key): hide it when it is showing,
    /// otherwise show it — which for a window behind another tab means bringing it to the front,
    /// not closing it.
    pub(crate) fn toggle(&mut self, w: DockWin) {
        if self.shown(w) {
            self.chrome_mut(w).open = false;
        } else {
            self.bring_forward(w);
        }
    }

    /// Open `w`, unfolded, in front of its dock's tab group.
    pub(crate) fn bring_forward(&mut self, w: DockWin) {
        let c = self.chrome_mut(w);
        c.open = true;
        c.collapsed = false;
        let place = c.place;
        if self.phone {
            self.phone_show(w);
            return;
        }
        if let Some(slot) = side_slot(place) {
            self.front[slot] = Some(w);
            self.note_side(slot);
        }
    }

    /// Remember the side dock used most recently (the one that stays in a narrow window). The
    /// bottom dock is not a side.
    fn note_side(&mut self, slot: usize) {
        if slot < 2 {
            self.last_side = slot;
        }
    }

    /// Apply what `win`'s header asked for: a tab click changes its dock's front window, anything
    /// else changes the window itself.
    pub(super) fn apply_header(&mut self, win: DockWin, action: ws::HeaderAction) {
        match action {
            ws::HeaderAction::Tab(i) if self.phone => {
                if let Some(&w) = self.phone_stack().get(i) {
                    self.phone_show(w);
                }
            }
            ws::HeaderAction::Tab(i) => {
                let side = self.chrome(win).place;
                if let (Some(slot), Some(&w)) = (side_slot(side), self.stack(side).get(i)) {
                    self.front[slot] = Some(w);
                    self.note_side(slot);
                }
            }
            ws::HeaderAction::Reorder(from, to) if !self.phone => {
                let side = self.chrome(win).place;
                let stack = self.stack(side);
                if let (Some(&a), Some(&b)) = (stack.get(from), stack.get(to)) {
                    self.tab_order.retain(|w| *w != a);
                    let at = self
                        .tab_order
                        .iter()
                        .position(|w| *w == b)
                        .map_or(self.tab_order.len(), |i| if from < to { i + 1 } else { i });
                    self.tab_order.insert(at, a);
                }
            }
            ws::HeaderAction::Reorder(..) => {}
            ws::HeaderAction::TearOff(_, _) if self.phone => {}
            ws::HeaderAction::TearOff(tab, at) => {
                // A tab of a group tears off that tab's window; a lone header, the window itself.
                let side = self.chrome(win).place;
                let w = tab
                    .and_then(|i| self.stack(side).get(i).copied())
                    .unwrap_or(win);
                let c = self.chrome_mut(w);
                c.place = Place::Float;
                c.collapsed = false;
                c.open = true;
                self.pending_float = Some((w, at));
            }
            other => apply_header(other, self.chrome_mut(win)),
        }
    }

    /// Open the Layers window with the keyboard in its search box, on every tab (Ctrl+K and the
    /// other "search everything" ways in).
    pub(crate) fn open_search(&mut self) {
        self.bring_forward(DockWin::Layers);
        self.filter = LayerFilter::All;
        self.focus_search = true;
    }
}

/// What a tool window's header asked for, applied to that window's own state.
pub(super) fn apply_header(action: ws::HeaderAction, w: &mut WindowChrome) {
    match action {
        ws::HeaderAction::None
        | ws::HeaderAction::Tab(_)
        | ws::HeaderAction::TearOff(..)
        | ws::HeaderAction::Reorder(..) => {}
        ws::HeaderAction::Close => w.open = false,
        ws::HeaderAction::Collapse => w.collapsed = !w.collapsed,
        ws::HeaderAction::Place(p) => {
            w.place = p;
            // A window that lands in a dock is shown whole; folding is a floating-window thing.
            if p != Place::Float {
                w.collapsed = false;
            }
        }
    }
}

/// Where the next floating tool window opens (set by a tear-off, taken by that window).
fn float_at_id() -> egui::Id {
    egui::Id::new("ws_float_at")
}

/// The dock a floating window was just dropped on (set by [`tool_window`], taken by the overlay).
fn dock_drop_id() -> egui::Id {
    egui::Id::new("ws_dock_drop")
}

/// Edge of a dock target, and how far outside it a drop still counts.
const TARGET: f32 = 40.0;
const TARGET_SLOP: f32 = 14.0;

/// The dock targets shown while a floating window is dragged: at the middle of the map's left,
/// right and bottom edges, as Dear ImGui draws them.
fn dock_targets(map: egui::Rect) -> [(Place, egui::Rect); 3] {
    let at = |c: egui::Pos2| egui::Rect::from_center_size(c, egui::vec2(TARGET, TARGET));
    let inset = TARGET * 0.5 + 12.0;
    [
        (
            Place::Left,
            at(egui::pos2(map.left() + inset, map.center().y)),
        ),
        (
            Place::Right,
            at(egui::pos2(map.right() - inset, map.center().y)),
        ),
        (
            Place::Bottom,
            at(egui::pos2(map.center().x, map.bottom() - inset)),
        ),
    ]
}

/// The dock target under `p`, if any.
fn dock_target_at(map: egui::Rect, p: egui::Pos2) -> Option<Place> {
    dock_targets(map)
        .into_iter()
        .find(|(_, r)| r.expand(TARGET_SLOP).contains(p))
        .map(|(place, _)| place)
}

/// Paint the dock targets over everything, and the area a window would take if dropped on the
/// one under the pointer.
fn paint_dock_targets(ctx: &egui::Context, t: &ws::Tokens, map: egui::Rect, hot: Option<Place>) {
    use egui_phosphor::regular as ph;
    let p = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("ws_dock_targets"),
    ));
    if let Some(place) = hot {
        let w = (map.width() * 0.3).clamp(DOCK_MIN_W.min(map.width()), DOCK_MAX_W);
        let h = BOTTOM_H.min(map.height() * 0.45);
        let area = match place {
            Place::Left => egui::Rect::from_min_size(map.min, egui::vec2(w, map.height())),
            Place::Right => {
                egui::Rect::from_min_max(egui::pos2(map.right() - w, map.top()), map.max)
            }
            _ => egui::Rect::from_min_max(egui::pos2(map.left(), map.bottom() - h), map.max),
        };
        p.rect(
            area.shrink(2.0),
            4.0,
            t.accent_soft(),
            egui::Stroke::new(1.5, t.accent),
            egui::StrokeKind::Inside,
        );
    }
    for (place, r) in dock_targets(map) {
        let on = hot == Some(place);
        p.rect(
            r,
            6.0,
            if on {
                t.accent
            } else {
                t.panel_hi.gamma_multiply(0.95)
            },
            egui::Stroke::new(1.5, if on { t.accent } else { t.line }),
            egui::StrokeKind::Inside,
        );
        p.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            match place {
                Place::Left => ph::ARROW_LINE_LEFT,
                Place::Right => ph::ARROW_LINE_RIGHT,
                _ => ph::ARROW_LINE_DOWN,
            },
            egui::FontId::proportional(20.0),
            if on { egui::Color32::WHITE } else { t.text },
        );
    }
}

/// Where a tool window draws this frame: into a side panel of the root layout (docked, before the
/// map's rect is taken) or as a window over the map (floating, after it).
pub(super) enum Host<'a> {
    /// Drawn straight into its side's dock panel (alone, or as the front of a tab group).
    Docked(&'a mut egui::Ui),
    Floating(&'a egui::Context),
}

/// A tool window: its id, where it sits, how wide it is, and where it first appears when floating.
pub(super) struct ToolWindow {
    pub id: &'static str,
    pub place: Place,
    pub width: f32,
    pub float_at: egui::Pos2,
}

/// Draw a tool window's frame where it sits and run `body` inside it (the body draws its own
/// header). Docked, the side's panel is already there ([`HookEchoApp::dock_side`]); floating, it
/// is a movable window kept inside the map.
pub(super) fn tool_window(
    host: Host<'_>,
    w: ToolWindow,
    map_rect: egui::Rect,
    t: &ws::Tokens,
    body: impl FnOnce(&mut egui::Ui),
) {
    let ToolWindow {
        id,
        place,
        width,
        float_at,
    } = w;
    match host {
        Host::Docked(ui) => {
            let _ = place;
            body(ui)
        }
        Host::Floating(ctx) => {
            let mut window = egui::Window::new(id)
                .id(egui::Id::new(id))
                .title_bar(false)
                .resizable(false)
                .constrain_to(map_rect)
                .default_pos(float_at)
                .frame(ws::card_frame(t));
            // Just torn out of a dock: its header under the pointer that tore it.
            if let Some(at) = ctx.data_mut(|d| d.remove_temp::<egui::Pos2>(float_at_id())) {
                window = window.current_pos(at - egui::vec2(40.0, 14.0));
            }
            let shown = window.show(ctx, |ui| {
                ws::style_scope(ui, t);
                ui.set_width(width);
                body(ui);
            });
            // Dragging a floating window shows where it can dock; letting go on a target docks it.
            if let Some(r) = shown {
                let moved = &r.response;
                if moved.dragged() {
                    let hot = ctx
                        .pointer_interact_pos()
                        .and_then(|p| dock_target_at(map_rect, p));
                    paint_dock_targets(ctx, t, map_rect, hot);
                }
                if moved.drag_stopped() {
                    let at = ctx.input(|i| i.pointer.latest_pos());
                    if let Some(place) = at.and_then(|p| dock_target_at(map_rect, p)) {
                        ctx.data_mut(|d| d.insert_temp(dock_drop_id(), place));
                    }
                }
            }
        }
    }
}

/// One category's rows, as indexes into the registry slice, with how many are switched on.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Group {
    pub category: &'static str,
    pub rows: Vec<usize>,
    pub on: usize,
}

/// The favourite slug a registry row can be remembered by: only a field layer has one.
pub(crate) fn favorite_slug(e: &PaletteEntry) -> Option<&'static str> {
    match e.action {
        crate::app::PaletteAction::ToggleField(layer) => Some(layer.slug()),
        _ => None,
    }
}

/// Group registry rows by category for the panel, in the panel's category order.
///
/// Which categories: the tab's own, unless a search is typed or the filter is Active/Favorites —
/// then every category, because "what's on" and "what did I star" do not stop at a tab, and a
/// search that could not find a layer on another tab would send you hunting for it. A category
/// with no rows left is dropped rather than shown empty. `on` counts the rows *shown* that are
/// switched on, which is the "2/28" beside a category's name.
pub(crate) fn group_entries(
    entries: &[PaletteEntry],
    tab: DockTab,
    filter: LayerFilter,
    query: &str,
    favorites: &[String],
) -> Vec<Group> {
    let every_category = !query.is_empty() || filter != LayerFilter::All;
    let mut groups: Vec<Group> = Vec::new();
    let keep = |e: &PaletteEntry| match filter {
        LayerFilter::All => true,
        LayerFilter::Active => e.on == Some(true),
        LayerFilter::Favorites => {
            favorite_slug(e).is_some_and(|s| favorites.iter().any(|f| f == s))
        }
    };
    // A loose subsequence match is how a short query finds a layer ("srv", "gau"), and it also
    // lets "window" match "Wind toward/away". So the rows whose names hold the query itself go
    // first, in their own group, tightest first — and Enter, which takes the first row, takes
    // the best one rather than whichever category happens to be listed first.
    let best = best_matches(entries, query, &keep);
    if !best.is_empty() {
        groups.push(Group {
            category: BEST,
            on: best
                .iter()
                .filter(|&&i| entries[i].on == Some(true))
                .count(),
            rows: best.clone(),
        });
    }
    for cat in crate::ui::layers_panel::CATEGORIES {
        if !every_category && !tab.categories().contains(&cat) {
            continue;
        }
        let mut rows = Vec::new();
        let mut on = 0;
        for (i, e) in entries.iter().enumerate() {
            if e.category != cat || best.contains(&i) {
                continue;
            }
            let is_on = e.on == Some(true);
            if !keep(e) {
                continue;
            }
            // Word by word, in any order: the name may match loosely, the description and the
            // keywords only as written (see `word_match`).
            if !query.trim().is_empty()
                && crate::ui::layers_panel::fuzzy(query, &e.label).is_none()
                && crate::ui::layers_panel::word_match(query, e).is_none()
            {
                continue;
            }
            if is_on {
                on += 1;
            }
            rows.push(i);
        }
        if !rows.is_empty() {
            groups.push(Group {
                category: cat,
                rows,
                on,
            });
        }
    }
    groups
}

/// The search results' own group, ahead of the categories: see [`best_matches`].
pub(crate) const BEST: &str = "Best";

/// Rows whose name holds `query` as written, tightest first (earliest in the name, then the
/// shortest name), at most six. Empty for an empty query.
fn best_matches(
    entries: &[PaletteEntry],
    query: &str,
    keep: &dyn Fn(&PaletteEntry) -> bool,
) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    // Every word as written somewhere in the name, in any order; ranked by where the first one
    // lands, then by the shorter name.
    let mut hits: Vec<(usize, usize, usize)> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| keep(e))
        .filter_map(|(i, e)| {
            let name = e.label.to_lowercase();
            let mut first = usize::MAX;
            for w in &words {
                first = first.min(name.find(w.as_str())?);
            }
            Some((first, e.label.len(), i))
        })
        .collect();
    hits.sort();
    hits.into_iter().take(6).map(|(_, _, i)| i).collect()
}

/// The tint for a category's glyphs in the tree, so a glance tells radar from models from warnings.
pub(crate) fn category_color(category: &str) -> egui::Color32 {
    use egui::Color32;
    match category {
        "Radar" => Color32::from_rgb(90, 160, 255),
        "Sites" => Color32::from_rgb(80, 200, 200),
        "National" => Color32::from_rgb(170, 130, 255),
        "Severe" => Color32::from_rgb(255, 150, 70),
        "Obs" => Color32::from_rgb(110, 210, 120),
        "Models" => Color32::from_rgb(220, 120, 220),
        "Reference" => Color32::from_rgb(170, 180, 195),
        _ => Color32::from_rgb(150, 160, 175),
    }
}

impl HookEchoApp {
    /// The workstation tokens for this frame: the fixed palette around the theme's accent.
    pub(crate) fn ws_tokens(&self) -> ws::Tokens {
        ws::Tokens::new(crate::theme::accent(self.settings.theme))
    }

    /// Draw the workstation's docked parts. Called once per frame before the map's own rect is
    /// read, so the map gets whatever they leave. Order is layout: the two top bars (unless hidden
    /// for a full-window map), the timeline across the full width at the bottom, then the docked
    /// tool windows, and the rail last so it sits against the map.
    pub(crate) fn dock_layout(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        self.dock.phone = false;
        ws::set_touch(ctx, false);
        self.dock_sync_arrangement();
        self.dock_sync_bulletin();
        if !self.ribbon_collapsed {
            self.dock_app_bar(root, ctx);
            self.dock_toolbar(root, ctx);
        }
        // Drawn first of the bottom panels, so it is the lowest: under the timeline.
        self.dock_footer(root, ctx);
        self.dock_timeline(root, false);
        self.dock_sync_available();
        self.dock.narrow = ctx.content_rect().width() < ONE_DOCK_BELOW;
        // Ctrl+Tab steps through the tabs of the dock under the pointer (Shift goes back), as
        // Dear ImGui's window switcher does. Taken before any widget can use Tab for focus.
        let (back, fwd) = ctx.input_mut(|i| {
            (
                i.consume_key(
                    egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                    egui::Key::Tab,
                ),
                i.consume_key(egui::Modifiers::CTRL, egui::Key::Tab),
            )
        });
        if back || fwd {
            let side = self.dock.cycle_target(ctx.pointer_latest_pos());
            self.dock.cycle_tab(side, back);
        }
        self.dock.rects = [None; 3];
        for side in [Place::Left, Place::Right] {
            if !self.dock.side_visible(side) {
                continue;
            }
            let stack = self.dock.stack(side);
            if !stack.is_empty() {
                self.dock_side(root, ctx, side, &stack);
            }
        }
        self.dock_rail(root, ctx);
        // Last, so it spans only the map's width, between the side docks and the rail.
        let stack = self.dock.stack(Place::Bottom);
        if !stack.is_empty() {
            self.dock_side(root, ctx, Place::Bottom, &stack);
        }
    }

    /// A warning's bulletin reads in the Alerts window here, not over the map: a new one brings
    /// that window forward. And a newly selected storm joins the Cell window.
    fn dock_sync_bulletin(&mut self) {
        let bulletin = self
            .warning_popup
            .as_ref()
            .and_then(|p| p.cards.first())
            .map(|c| c.info.id.clone());
        self.sync_open_cells();
        if bulletin != self.dock.bulletin_seen {
            if bulletin.is_some() {
                self.dock.bring_forward(DockWin::Alerts);
            }
            self.dock.bulletin_seen = bulletin;
        }
    }

    /// Say which windows have something to show this frame, and settle which is in front.
    fn dock_sync_available(&mut self) {
        let in_3d = self.views[self.active].map_3d.enabled;
        self.dock.set_view3d_available(in_3d);
        self.dock.set_log_available(self.settings.analyst_mode);
        self.dock.set_sounding_available(self.sounding_window.open);
        self.dock
            .set_cell_available(self.cell_details && self.cell_popup.is_some());
        self.dock
            .set_region_available(self.region.samples().is_some());
        self.dock.set_volume_available(self.show_3d);
        self.dock.update_fronts();
    }

    /// A side's dock: one panel for whatever is docked there. Several windows share it as tabs
    /// (design plan §2.3) rather than each taking its own strip of the map's width, and only the
    /// front one is drawn. Its inner edge drags to resize it (double-click restores the width its
    /// windows ask for); the panel belongs to the side, not the window, so the width holds as
    /// windows come and go.
    fn dock_side(
        &mut self,
        root: &mut egui::Ui,
        ctx: &egui::Context,
        side: Place,
        stack: &[DockWin],
    ) {
        let t = self.ws_tokens();
        let slot = side_slot(side).unwrap_or_default();
        let front = self.dock.front[slot].unwrap_or(stack[0]);
        let bottom = side == Place::Bottom;
        // A side dock's width, or the bottom dock's height: dragged, or what its windows ask for.
        let (min, max) = if bottom {
            let max = bottom_max_height(ctx.content_rect().height());
            (BOTTOM_MIN_H.min(max), max)
        } else {
            let max = dock_max_width(ctx.content_rect().width());
            (DOCK_MIN_W.min(max), max)
        };
        let width = if bottom {
            self.dock.bottom_h.unwrap_or(BOTTOM_H)
        } else {
            let natural = stack.iter().map(|w| w.width()).fold(0.0, f32::max);
            self.dock.dock_widths[slot].unwrap_or(natural)
        }
        .clamp(min, max);
        let panel = match side {
            Place::Right => egui::Panel::right("dock_side_right"),
            Place::Bottom => egui::Panel::bottom("dock_side_bottom"),
            _ => egui::Panel::left("dock_side_left"),
        };
        let mut tabs = ws::HeaderTabs {
            tabs: stack.iter().map(|w| w.tab()).collect(),
            front: stack.iter().position(|w| *w == front).unwrap_or(0),
        };
        for (w, tab) in stack.iter().zip(&mut tabs.tabs) {
            tab.dot = match w {
                DockWin::Alerts if self.alert_badge().0 > 0 => Some(t.warn),
                DockWin::Sources if self.sources_attention() > 0 => Some(t.danger),
                DockWin::Gauges if self.gauges_in_flood() > 0 => Some(t.warn),
                _ => None,
            };
        }
        let grouped = stack.len() > 1;
        let rect = panel
            .exact_size(width)
            .resizable(false)
            .frame(ws::panel_frame(&t))
            .show(root, |ui| {
                ws::style_scope(ui, &t);
                if grouped {
                    ws::set_header_tabs(ctx, Some(tabs));
                }
                contained(ui, |ui| self.dock_window(front, Host::Docked(ui), ctx));
                ws::set_header_tabs(ctx, None);
            })
            .response
            .rect;
        self.dock.rects[slot] = Some(rect);
        // The resize grip: a strip along the inner edge, inside the panel, registered after its
        // contents so it wins over whatever they put there.
        let grip = match side {
            Place::Right => {
                egui::Rect::from_x_y_ranges(rect.left()..=rect.left() + 5.0, rect.y_range())
            }
            Place::Bottom => {
                egui::Rect::from_x_y_ranges(rect.x_range(), rect.top()..=rect.top() + 5.0)
            }
            _ => egui::Rect::from_x_y_ranges(rect.right() - 5.0..=rect.right(), rect.y_range()),
        };
        let cursor = if bottom {
            egui::CursorIcon::ResizeVertical
        } else {
            egui::CursorIcon::ResizeHorizontal
        };
        let resp = root
            .interact(
                grip,
                egui::Id::new(("dock_resize", slot)),
                egui::Sense::click_and_drag(),
            )
            .on_hover_cursor(cursor)
            .on_hover_text(if bottom {
                "Drag to resize; double-click for the default height"
            } else {
                "Drag to resize; double-click for the default width"
            });
        if resp.dragged() {
            let d = resp.drag_delta();
            let grown = match side {
                Place::Right => -d.x,
                Place::Bottom => -d.y,
                _ => d.x,
            };
            let size = Some((width + grown).clamp(min, max));
            if bottom {
                self.dock.bottom_h = size;
            } else {
                self.dock.dock_widths[slot] = size;
            }
            ctx.set_cursor_icon(cursor);
        }
        if resp.double_clicked() {
            if bottom {
                self.dock.bottom_h = None;
            } else {
                self.dock.dock_widths[slot] = None;
            }
        }
        if resp.hovered() || resp.dragged() {
            let line = match side {
                Place::Right => [rect.left_top(), rect.left_bottom()],
                Place::Bottom => [rect.left_top(), rect.right_top()],
                _ => [rect.right_top(), rect.right_bottom()],
            };
            root.painter()
                .line_segment(line, egui::Stroke::new(2.0, t.accent));
        }
    }

    fn dock_window(&mut self, w: DockWin, host: Host<'_>, ctx: &egui::Context) {
        match w {
            DockWin::Layers => self.dock_layers(host, ctx),
            DockWin::Inspector => self.dock_inspector(host, ctx),
            DockWin::Alerts => self.dock_alerts(host),
            DockWin::Prefs => self.dock_prefs(host, ctx),
            DockWin::Settings => self.dock_settings(host),
            DockWin::View3d => self.dock_view3d(host),
            DockWin::Sources => self.dock_sources(host),
            DockWin::Log => self.dock_log(host),
            DockWin::Sounding => self.dock_sounding(host),
            DockWin::Storms => self.dock_storms(host),
            DockWin::Cell => self.dock_cell(host),
            DockWin::Region => self.dock_region(host),
            DockWin::Volume => self.dock_volume(host),
            DockWin::Gauges => self.dock_gauges(host, ctx),
        }
    }

    /// Over the map: the floating tool windows, and the button that brings hidden bars back.
    pub(crate) fn dock_map_overlay(&mut self, ctx: &egui::Context) {
        for w in self.dock.stack(Place::Float) {
            // A window just torn out of a dock opens under the pointer that tore it.
            if let Some((torn, at)) = self.dock.pending_float {
                if torn == w {
                    ctx.data_mut(|d| d.insert_temp(float_at_id(), at));
                    self.dock.pending_float = None;
                }
            }
            self.dock_window(w, Host::Floating(ctx), ctx);
            // Dropped on a dock target while being dragged: dock it there, in front.
            if let Some(place) = ctx.data_mut(|d| d.remove_temp::<Place>(dock_drop_id())) {
                self.dock.chrome_mut(w).place = place;
                self.dock.bring_forward(w);
            }
        }
        if self.ribbon_collapsed {
            self.dock_bars_restore(ctx);
        }
    }

    /// Load the arrangement saved for the current layout when the layout changes, and save the
    /// current one back whenever a window moves, opens or closes.
    fn dock_sync_arrangement(&mut self) {
        let layout = self.settings.layout;
        if self.dock.arranged_for != Some(layout) {
            let saved = self
                .settings
                .workstation
                .get(&layout)
                .cloned()
                .unwrap_or_else(|| DockState::preset(layout));
            self.dock.arrange(&saved);
            self.dock.arranged_for = Some(layout);
        }
        let now = self.dock.arrangement();
        if self.settings.workstation.get(&layout) != Some(&now) {
            self.settings.workstation.insert(layout, now);
        }
    }

    /// The top bars are hidden for a full-window map (T, or the palette's "top bar" toggle): one
    /// small tab at the top edge brings them back.
    fn dock_bars_restore(&mut self, ctx: &egui::Context) {
        use crate::ui::a11y::Named as _;
        let t = self.ws_tokens();
        egui::Area::new(egui::Id::new("dock_bars_restore"))
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ws::card_frame(&t)
                    .corner_radius(egui::CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: 4,
                        se: 4,
                    })
                    .show(ui, |ui| {
                        if ws::icon_button(ui, &t, egui_phosphor::regular::CARET_DOWN, "", false)
                            .named("Show the top bars (T)")
                            .clicked()
                        {
                            self.ribbon_collapsed = false;
                        }
                    });
            });
    }
}

/// The rows of the inspector's model card: what is on the map from the models, when it is valid,
/// and how fresh it is. Pure, so what the card says is checked without a window.
pub(crate) fn model_card_rows(
    input: &crate::ui::model_panel::Input,
    tz: Option<wxdata::tz::Tz>,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<(&'static str, String)> {
    let sel = input.sel;
    let run = match (&input.stamp, input.run) {
        (Some(stamp), _) => stamp
            .run_time
            .map(|r| r.format("%d %HZ").to_string())
            .unwrap_or_else(|| "unknown".into()),
        (None, Some(run)) => run.format("%d %HZ").to_string(),
        (None, None) => "latest".into(),
    };
    let mut rows = vec![
        // An analysis names which one it is: the real-time RTMA, or the URMA re-analysis an older
        // hour is served from (the stamp's source says which arrived).
        (
            "Model:",
            match &input.stamp {
                Some(stamp) if sel.model == crate::model_browser::BModel::Rtma => {
                    stamp.source_id.trim_end_matches(" analysis").to_string()
                }
                _ => sel.model.label().to_string(),
            },
        ),
        ("Product:", sel.product.label().to_string()),
        ("Run:", run),
        (
            "Lead:",
            if input.range.min == input.range.max {
                "analysis".to_string()
            } else {
                crate::model_browser::format_lead(input.lead_min)
            },
        ),
    ];
    match &input.stamp {
        Some(stamp) => {
            rows.push((
                "Valid:",
                crate::timefmt::fmt_date_clock(stamp.valid_time, tz),
            ));
            rows.push((
                "Fetched:",
                crate::ui::model_panel::ago(stamp.received_time, now),
            ));
        }
        None => rows.push(("Valid:", "loading\u{2026}".into())),
    }
    rows
}

/// The cursor readout's rows for a point on the map, given the active radar's position: latitude
/// and longitude, and (when there is a radar) how far and in which direction it is. Pure, so the
/// numbers can be tested without a window.
pub(crate) fn cursor_readout(
    at: (f64, f64),
    radar: Option<(f64, f64)>,
    metric: bool,
) -> Vec<(&'static str, String)> {
    let (lon, lat) = at;
    let mut rows = vec![("Lat", format!("{lat:.2}")), ("Lon", format!("{lon:.2}"))];
    if let Some((rlon, rlat)) = radar {
        let (km, bearing) = crate::geo::great_circle([rlon, rlat], [lon, lat]);
        rows.push(("Range", crate::geo::fmt_distance(km, metric, 1)));
        rows.push(("Az", format!("{:.1}\u{b0}", bearing.rem_euclid(360.0))));
    }
    rows
}

/// Which tilt (an index into the volume's sorted `elevations`) the live stream is sweeping right
/// now, matched by angle — see [`crate::view::tilt_index_for_angle`] for why not by sweep number.
/// `None` when nothing is streaming, the indicator is turned off, no chunk has arrived, or the
/// sweep's angle is not in the volume yet.
pub(crate) fn sweeping_tilt(
    progress: Option<wxdata::live::ScanProgress>,
    elevations: &[f32],
    streaming: bool,
    indicator_on: bool,
) -> Option<usize> {
    if !streaming || !indicator_on {
        return None;
    }
    // Sweep zero is the stream not having said yet.
    let p = progress.filter(|p| p.elevation_number > 0)?;
    crate::view::tilt_index_for_angle(elevations, p.elevation_angle_deg)
}

/// The one line that says what the radar is doing, for the tilt bar. Empty when not live.
pub(crate) fn live_status(
    progress: Option<wxdata::live::ScanProgress>,
    streaming: bool,
    indicator_on: bool,
) -> String {
    if !streaming {
        return String::new();
    }
    match progress {
        Some(p) if indicator_on => format!(
            "LIVE \u{b7} sweeping {:.1}\u{b0} ({}/{}) chunk {}/{}",
            p.elevation_angle_deg,
            p.elevation_number,
            p.total_elevations,
            p.chunk_index,
            p.chunks_in_sweep
        ),
        _ => "LIVE".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::PaletteAction;

    fn entry(label: &str, category: &'static str, on: Option<bool>) -> PaletteEntry {
        PaletteEntry {
            label: label.to_string(),
            category,
            action: PaletteAction::Reload,
            on,
            desc: "",
            common: false,
            key: None,
            health: None,
        }
    }

    fn sample() -> Vec<PaletteEntry> {
        let mut mrms = entry("MRMS reflectivity", "National", Some(false));
        mrms.action = PaletteAction::ToggleField(crate::render::FieldLayer::Mrms);
        vec![
            entry("Reflectivity", "Radar", Some(true)),
            entry("Velocity", "Radar", Some(false)),
            entry("KTLX", "Sites", Some(false)),
            entry("HRRR future radar", "Models", Some(true)),
            entry("GFS precipitation", "Models", Some(false)),
            entry("Watches", "Severe", Some(false)),
            entry("Measure", "Tools", None),
            mrms,
        ]
    }

    fn cats(g: &[Group]) -> Vec<&'static str> {
        g.iter().map(|g| g.category).collect()
    }

    #[test]
    fn each_tab_lists_its_own_categories_in_the_panels_order() {
        let e = sample();
        let at = |tab| cats(&group_entries(&e, tab, LayerFilter::All, "", &[]));
        assert_eq!(at(DockTab::Radar), ["Radar", "Sites"]);
        assert_eq!(at(DockTab::Models), ["Models"]);
        assert_eq!(at(DockTab::Satellite), ["National"]);
        assert_eq!(
            at(DockTab::Surface),
            ["Severe"],
            "no Obs rows, so no Obs header"
        );
        assert_eq!(at(DockTab::Analysis), ["Tools"]);
        // Every registry category belongs to exactly one tab.
        for cat in crate::ui::layers_panel::CATEGORIES {
            let homes = DockTab::ALL
                .iter()
                .filter(|t| t.categories().contains(&cat))
                .count();
            assert_eq!(homes, 1, "{cat}");
        }
    }

    #[test]
    fn a_search_puts_names_holding_the_query_first() {
        let mut e = sample();
        let mut inspector = entry("Inspector window", "Reference", Some(false));
        inspector.desc = "The reading under the pointer";
        e.push(inspector);
        let g = group_entries(&e, DockTab::Radar, LayerFilter::All, "window", &[]);
        assert_eq!(g[0].category, BEST);
        assert_eq!(e[g[0].rows[0]].label, "Inspector window");
        // Listed once: not again under its own category.
        assert!(!g
            .iter()
            .skip(1)
            .any(|g| g.rows.iter().any(|&i| e[i].label == "Inspector window")));
        // A description has to hold the query as written, not as a scattered subsequence.
        let g = group_entries(&e, DockTab::Radar, LayerFilter::All, "tpr", &[]);
        assert!(g
            .iter()
            .all(|g| g.rows.iter().all(|&i| e[i].label != "Inspector window")));
        let g = group_entries(&e, DockTab::Radar, LayerFilter::All, "pointer", &[]);
        assert!(g
            .iter()
            .any(|g| g.rows.iter().any(|&i| e[i].label == "Inspector window")));
        // No query, no best-matches group.
        let g = group_entries(&e, DockTab::Radar, LayerFilter::All, "", &[]);
        assert!(g.iter().all(|g| g.category != BEST));
    }

    #[test]
    fn each_group_counts_the_rows_that_are_on() {
        let g = group_entries(&sample(), DockTab::Radar, LayerFilter::All, "", &[]);
        let radar = g.iter().find(|g| g.category == "Radar").unwrap();
        assert_eq!((radar.on, radar.rows.len()), (1, 2));
    }

    #[test]
    fn active_and_favorites_cross_every_tab() {
        let e = sample();
        let labels = |g: Vec<Group>| -> Vec<String> {
            g.iter()
                .flat_map(|g| g.rows.iter().map(|&i| e[i].label.clone()))
                .collect()
        };
        assert_eq!(
            labels(group_entries(
                &e,
                DockTab::Gis,
                LayerFilter::Active,
                "",
                &[]
            )),
            ["Reflectivity", "HRRR future radar"]
        );
        let fav = vec!["mrms".to_string()];
        let slug = crate::render::FieldLayer::Mrms.slug();
        let fav = if slug == "mrms" {
            fav
        } else {
            vec![slug.to_string()]
        };
        assert_eq!(
            labels(group_entries(
                &e,
                DockTab::Radar,
                LayerFilter::Favorites,
                "",
                &fav
            )),
            ["MRMS reflectivity"]
        );
        // A row that is not a field layer can never be a favourite.
        assert!(group_entries(&e, DockTab::Radar, LayerFilter::Favorites, "", &[]).is_empty());
    }

    #[test]
    fn a_search_reaches_every_tab_and_drops_empty_categories() {
        // A loose match still reaches another tab's category...
        let g = group_entries(&sample(), DockTab::Radar, LayerFilter::All, "hfr", &[]);
        assert_eq!(cats(&g), ["Models"]);
        assert_eq!(g[0].rows, vec![3]);
        // ...and a name that holds the query leads, in its own group, leaving Models empty.
        let g = group_entries(&sample(), DockTab::Radar, LayerFilter::All, "hrrr", &[]);
        assert_eq!(cats(&g), [BEST]);
        assert_eq!(g[0].rows, vec![3]);
        assert!(
            group_entries(&sample(), DockTab::Radar, LayerFilter::All, "zzzzqq", &[]).is_empty()
        );
    }

    #[test]
    fn a_row_that_is_not_a_toggle_counts_as_off() {
        let g = group_entries(&sample(), DockTab::Analysis, LayerFilter::All, "", &[]);
        assert_eq!((g[0].on, g[0].rows.len()), (0, 1));
    }

    #[test]
    fn wsv3_opens_map_first_and_the_dock_with_its_windows() {
        use crate::settings::Layout;
        let wsv3 = DockState::preset(Layout::Wsv3);
        assert!(!wsv3.layers.open && !wsv3.inspector.open && wsv3.timeline_open);
        assert_eq!(
            wsv3.inspector.place,
            Place::Right,
            "docked right when asked for"
        );
        let dock = DockState::preset(Layout::Dock);
        assert!(dock.layers.open && dock.inspector.open);
        assert_eq!(
            (dock.layers.place, dock.inspector.place),
            (Place::Left, Place::Float)
        );
        for w in [&wsv3, &dock] {
            assert!(!w.alerts.open && !w.prefs.open);
            assert_eq!(
                (w.alerts.place, w.prefs.place),
                (Place::Right, Place::Right)
            );
        }
        // A fresh state is the Dock's preset, so the Dock layout looks as it did before.
        assert_eq!(DockState::default().arrangement(), dock);
    }

    #[test]
    fn an_arrangement_round_trips_and_an_unknown_tab_falls_back() {
        let mut s = DockState::default();
        let w = WorkstationChrome {
            tab: "GIS".into(),
            layers: WindowChrome {
                open: true,
                place: Place::Float,
                collapsed: true,
            },
            inspector: WindowChrome::at(false, Place::Left),
            alerts: WindowChrome::at(true, Place::Float),
            prefs: WindowChrome::at(true, Place::Left),
            settings: WindowChrome::at(true, Place::Bottom),
            view3d: WindowChrome::at(false, Place::Float),
            sources: WindowChrome::at(true, Place::Right),
            log: WindowChrome::at(false, Place::Left),
            dock_widths: [None, Some(360)],
            bottom_h: Some(280),
            // Saved complete: the moved window first, then the rest in their usual order.
            tab_order: tab_order_from(&["Gauges".into()])
                .iter()
                .map(|w| w.name().to_string())
                .collect(),
            sounding: WindowChrome::at(false, Place::Left),
            storms: WindowChrome::at(false, Place::Right),
            cell: WindowChrome::at(true, Place::Float),
            region: WindowChrome::at(false, Place::Left),
            volume: WindowChrome::at(true, Place::Right),
            gauges: WindowChrome::at(true, Place::Left),
            timeline_open: false,
            footer_open: false,
        };
        s.arrange(&w);
        assert_eq!(s.tab, DockTab::Gis);
        assert_eq!(s.arrangement(), w);
        s.arrange(&WorkstationChrome {
            tab: "Hydrology".into(),
            ..w
        });
        assert_eq!(s.tab, DockTab::Radar);
    }

    #[test]
    fn search_opens_layers_unfolded_on_everything() {
        let mut s = DockState {
            layers: WindowChrome {
                open: false,
                place: Place::Float,
                collapsed: true,
            },
            filter: LayerFilter::Favorites,
            ..DockState::default()
        };
        s.open_search();
        assert!(s.layers.open && !s.layers.collapsed && s.focus_search);
        assert_eq!(s.filter, LayerFilter::All);
    }

    #[test]
    fn search_reveals_layers_behind_another_dock_tab() {
        let mut s = DockState {
            layers: WindowChrome::at(true, Place::Right),
            inspector: WindowChrome::at(true, Place::Right),
            ..Default::default()
        };
        s.update_fronts();
        s.bring_forward(DockWin::Inspector);
        assert!(!s.shown(DockWin::Layers));
        s.open_search();
        assert!(s.shown(DockWin::Layers) && s.focus_search);
        assert_eq!(s.front[1], Some(DockWin::Layers));
    }

    #[test]
    fn windows_docked_together_share_one_side_with_the_newest_in_front() {
        let mut s = DockState {
            layers: WindowChrome::at(true, Place::Left),
            inspector: WindowChrome::at(true, Place::Right),
            alerts: WindowChrome::at(false, Place::Right),
            prefs: WindowChrome::at(false, Place::Right),
            ..Default::default()
        };
        s.update_fronts();
        assert_eq!(
            s.front,
            [Some(DockWin::Layers), Some(DockWin::Inspector), None]
        );
        // Opening Alerts puts it in the Inspector's dock, in front.
        s.alerts.open = true;
        s.update_fronts();
        assert_eq!(s.stack(Place::Right), [DockWin::Inspector, DockWin::Alerts]);
        assert_eq!(s.front[1], Some(DockWin::Alerts));
        // A tab click brings the Inspector back; nothing changing keeps it there.
        s.apply_header(DockWin::Alerts, ws::HeaderAction::Tab(0));
        s.update_fronts();
        assert_eq!(s.front[1], Some(DockWin::Inspector));
        // Closing the front window hands the dock to what is left.
        s.apply_header(DockWin::Inspector, ws::HeaderAction::Close);
        s.update_fronts();
        assert_eq!(s.front[1], Some(DockWin::Alerts));
        // The 3D view joins the Inspector's dock while the pane is in 3D, and only then.
        let mut three = DockState {
            inspector: WindowChrome::at(true, Place::Right),
            view3d: WindowChrome::at(false, Place::Right),
            ..Default::default()
        };
        three.update_fronts();
        assert_eq!(three.stack(Place::Right), [DockWin::Inspector]);
        three.set_view3d_available(true);
        three.update_fronts();
        assert_eq!(
            three.stack(Place::Right),
            [DockWin::Inspector, DockWin::View3d]
        );
        assert!(
            three.shown(DockWin::View3d),
            "entering 3D brings its controls forward"
        );
        three.set_view3d_available(false);
        three.update_fronts();
        assert_eq!(three.front[1], Some(DockWin::Inspector));
        // The Analyst log is a tab only while Analyst Mode is on, and turning the mode back on
        // shows it again even after it was closed.
        three.log = WindowChrome::at(false, Place::Right);
        three.set_log_available(false);
        three.update_fronts();
        assert!(!three.stack(Place::Right).contains(&DockWin::Log));
        three.set_log_available(true);
        three.update_fronts();
        assert!(three.shown(DockWin::Log));
        // A new sounding brings its tab in; with none, there is no tab.
        three.sounding = WindowChrome::at(true, Place::Right);
        three.set_sounding_available(false);
        three.update_fronts();
        assert!(!three.stack(Place::Right).contains(&DockWin::Sounding));
        three.set_sounding_available(true);
        three.update_fronts();
        assert!(three.shown(DockWin::Sounding));
        // Windows arriving together (a restored arrangement) open on the first in tab order.
        let mut restored = DockState {
            inspector: WindowChrome::at(true, Place::Right),
            alerts: WindowChrome::at(true, Place::Right),
            ..Default::default()
        };
        restored.update_fronts();
        assert_eq!(restored.front[1], Some(DockWin::Inspector));
        // A button for a window behind another tab brings it forward rather than closing it.
        s.inspector.open = true;
        s.update_fronts();
        s.apply_header(DockWin::Alerts, ws::HeaderAction::Tab(1));
        assert!(!s.shown(DockWin::Inspector) && s.shown(DockWin::Alerts));
        s.toggle(DockWin::Inspector);
        assert!(s.shown(DockWin::Inspector) && s.inspector.open);
        s.toggle(DockWin::Inspector);
        assert!(!s.inspector.open);
        s.update_fronts();
        // Floating a window takes it out of the group.
        s.apply_header(DockWin::Alerts, ws::HeaderAction::Place(Place::Float));
        s.update_fronts();
        assert!(s.stack(Place::Right).is_empty());
        assert_eq!(s.front[1], None);
    }

    #[test]
    fn on_a_phone_every_window_is_a_tab_of_one_sheet() {
        let mut s = DockState {
            phone: true,
            ..DockState::default()
        };
        for w in PHONE_CORE {
            s.chrome_mut(w).open = true;
        }
        s.update_fronts();
        // Arriving together, the sheet opens on the first of its tabs, wherever they were docked.
        assert_eq!(s.phone_front, Some(DockWin::Inspector));
        assert_eq!(s.phone_stack()[..4], PHONE_CORE);
        assert!(s.shown(DockWin::Inspector) && !s.shown(DockWin::Layers));
        // A tab click brings its window forward; a folded sheet comes back up for it.
        s.sheet = Sheet::Peek;
        assert!(!s.shown(DockWin::Inspector), "a folded sheet shows nothing");
        s.apply_header(DockWin::Inspector, ws::HeaderAction::Tab(2));
        assert_eq!(
            (s.phone_front, s.sheet),
            (Some(DockWin::Storms), Sheet::Half)
        );
        // A window that appears (a sounding) comes to the front.
        s.set_sounding_available(true);
        s.update_fronts();
        assert_eq!(s.phone_front, Some(DockWin::Sounding));
        // Its going hands the front back to the first tab.
        s.set_sounding_available(false);
        s.update_fronts();
        assert_eq!(s.phone_front, Some(DockWin::Inspector));
        // A button for a window shows it rather than closing it when it is behind another.
        s.toggle(DockWin::Alerts);
        assert!(s.shown(DockWin::Alerts));
        // Every window has a place in the sheet's order.
        for w in DockWin::ALL {
            assert!(PHONE_ORDER.contains(&w), "{w:?}");
        }
    }

    #[test]
    fn a_narrow_window_shows_one_dock_the_last_one_used() {
        let mut s = DockState {
            layers: WindowChrome::at(true, Place::Left),
            inspector: WindowChrome::at(true, Place::Right),
            ..Default::default()
        };
        s.update_fronts();
        assert!(
            s.shown(DockWin::Layers) && s.shown(DockWin::Inspector),
            "wide: both"
        );
        s.narrow = true;
        s.toggle(DockWin::Layers);
        assert!(
            s.layers.open,
            "a hidden side's button shows it rather than closing it"
        );
        assert!(s.shown(DockWin::Layers) && !s.shown(DockWin::Inspector));
        assert!(s.inspector.open, "the set-aside side is hidden, not closed");
        s.toggle(DockWin::Inspector);
        assert!(!s.shown(DockWin::Layers) && s.shown(DockWin::Inspector));
        // With the other side empty there is nothing to choose between.
        s.inspector.open = false;
        s.update_fronts();
        assert!(s.side_visible(Place::Left) && s.shown(DockWin::Layers));
        // Floating windows are never set aside.
        s.inspector = WindowChrome::at(true, Place::Float);
        assert!(s.shown(DockWin::Inspector));
    }

    #[test]
    fn the_bottom_dock_holds_a_tab_group_of_its_own() {
        let mut s = DockState {
            storms: WindowChrome::at(true, Place::Bottom),
            log: WindowChrome::at(true, Place::Bottom),
            inspector: WindowChrome::at(true, Place::Right),
            ..DockState::default()
        };
        s.set_log_available(true);
        s.update_fronts();
        assert_eq!(s.stack(Place::Bottom), [DockWin::Storms, DockWin::Log]);
        assert_eq!(s.front[2], Some(DockWin::Storms));
        assert!(s.shown(DockWin::Storms) && !s.shown(DockWin::Log));
        s.apply_header(DockWin::Storms, ws::HeaderAction::Tab(1));
        assert!(s.shown(DockWin::Log));
        // A narrow window sets a side dock aside, never the bottom one, and a window arriving
        // in the bottom dock does not count as using a side.
        s.narrow = true;
        s.last_side = 0;
        s.gauges = WindowChrome::at(true, Place::Bottom);
        s.update_fronts();
        assert_eq!(
            s.front[2],
            Some(DockWin::Gauges),
            "the newcomer is in front"
        );
        assert!(s.side_visible(Place::Bottom));
        assert_eq!(s.last_side, 0, "the bottom dock is not a side");
        // It survives the settings file with its height.
        s.bottom_h = Some(300.0);
        let back = {
            let mut t = DockState::default();
            t.arrange(
                &serde_json::from_str(&serde_json::to_string(&s.arrangement()).unwrap()).unwrap(),
            );
            t
        };
        assert_eq!(back.storms.place, Place::Bottom);
        assert_eq!(back.bottom_h, Some(300.0));
        assert_eq!(bottom_max_height(400.0), 200.0);
    }

    #[test]
    fn a_tab_torn_out_of_a_dock_floats_under_the_pointer() {
        let mut s = DockState {
            inspector: WindowChrome::at(true, Place::Right),
            alerts: WindowChrome::at(true, Place::Right),
            ..DockState::default()
        };
        s.update_fronts();
        let at = egui::pos2(500.0, 300.0);
        // The group's second tab, torn off from the Inspector's header.
        s.apply_header(DockWin::Inspector, ws::HeaderAction::TearOff(Some(1), at));
        assert_eq!(s.alerts.place, Place::Float);
        assert_eq!(s.inspector.place, Place::Right, "the front window stays");
        assert_eq!(s.pending_float, Some((DockWin::Alerts, at)));
        // A lone header tears off its own window.
        s.apply_header(DockWin::Inspector, ws::HeaderAction::TearOff(None, at));
        assert_eq!(s.inspector.place, Place::Float);
        // Not on a phone: there every window lives in the sheet.
        let mut phone = DockState {
            phone: true,
            ..DockState::default()
        };
        phone.inspector = WindowChrome::at(true, Place::Right);
        phone.apply_header(DockWin::Inspector, ws::HeaderAction::TearOff(None, at));
        assert_eq!(phone.inspector.place, Place::Right);
        assert!(
            !ws::tears_off(egui::vec2(6.0, 6.0)),
            "a wobbly click is a click"
        );
        assert!(ws::tears_off(egui::vec2(0.0, 20.0)));
    }

    #[test]
    fn a_floating_window_docks_on_the_target_it_is_dropped_on() {
        let map = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(1000.0, 700.0));
        for (place, r) in dock_targets(map) {
            assert!(map.contains_rect(r), "{place:?} target inside the map");
            assert_eq!(dock_target_at(map, r.center()), Some(place));
        }
        assert_eq!(
            dock_target_at(map, map.center()),
            None,
            "the middle of the map is not a dock"
        );
        let left = dock_targets(map)[0].1;
        assert_eq!(
            dock_target_at(
                map,
                left.right_center() + egui::vec2(TARGET_SLOP - 1.0, 0.0)
            ),
            Some(Place::Left),
            "a near miss still counts"
        );
    }

    #[test]
    fn tabs_dragged_along_the_strip_change_places_and_the_order_is_saved() {
        let mut s = DockState {
            inspector: WindowChrome::at(true, Place::Right),
            alerts: WindowChrome::at(true, Place::Right),
            storms: WindowChrome::at(true, Place::Right),
            ..DockState::default()
        };
        s.update_fronts();
        assert_eq!(
            s.stack(Place::Right),
            [DockWin::Inspector, DockWin::Storms, DockWin::Alerts]
        );
        // Storms (tab 1) dragged over Inspector (tab 0): it goes first.
        s.apply_header(DockWin::Inspector, ws::HeaderAction::Reorder(1, 0));
        assert_eq!(
            s.stack(Place::Right),
            [DockWin::Storms, DockWin::Inspector, DockWin::Alerts]
        );
        // And to the end.
        s.apply_header(DockWin::Inspector, ws::HeaderAction::Reorder(0, 2));
        assert_eq!(
            s.stack(Place::Right),
            [DockWin::Inspector, DockWin::Alerts, DockWin::Storms]
        );
        // The order survives the settings file, by name; the usual order saves as nothing.
        let saved = s.arrangement();
        assert!(!saved.tab_order.is_empty());
        let mut back = DockState::default();
        back.arrange(&saved);
        assert_eq!(back.stack(Place::Right), s.stack(Place::Right));
        assert!(DockState::default().arrangement().tab_order.is_empty());
        // A name a later version dropped is skipped, and every window still has a place.
        let order = tab_order_from(&["Nope".into(), "Log".into(), "Log".into()]);
        assert_eq!(order[0], DockWin::Log);
        assert_eq!(order.len(), DockWin::ALL.len());
        // Tab-strip geometry: along the strip over another tab reorders; off it tears.
        let strip = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 28.0));
        let tabs = [
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 28.0)),
            egui::Rect::from_min_size(egui::pos2(100.0, 0.0), egui::vec2(100.0, 28.0)),
        ];
        assert_eq!(
            ws::tab_drag(0, egui::pos2(150.0, 14.0), strip, &tabs),
            ws::HeaderAction::Reorder(0, 1)
        );
        assert_eq!(
            ws::tab_drag(0, egui::pos2(50.0, 20.0), strip, &tabs),
            ws::HeaderAction::None
        );
        assert!(matches!(
            ws::tab_drag(1, egui::pos2(150.0, 90.0), strip, &tabs),
            ws::HeaderAction::TearOff(Some(1), _)
        ));
    }

    #[test]
    fn ctrl_tab_steps_through_a_dock_and_reset_puts_everything_back() {
        let mut s = DockState {
            inspector: WindowChrome::at(true, Place::Right),
            alerts: WindowChrome::at(true, Place::Right),
            storms: WindowChrome::at(true, Place::Right),
            ..DockState::default()
        };
        s.update_fronts();
        assert_eq!(s.front[1], Some(DockWin::Inspector));
        s.cycle_tab(Place::Right, false);
        assert_eq!(s.front[1], Some(DockWin::Storms));
        s.cycle_tab(Place::Right, true);
        s.cycle_tab(Place::Right, true);
        assert_eq!(s.front[1], Some(DockWin::Alerts), "back wraps round");
        // The dock under the pointer wins; otherwise the side used last.
        s.rects[0] = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(100.0, 100.0),
        ));
        assert_eq!(s.cycle_target(Some(egui::pos2(50.0, 50.0))), Place::Left);
        s.last_side = 1;
        assert_eq!(s.cycle_target(Some(egui::pos2(500.0, 50.0))), Place::Right);
        // Reset: the layout's own arrangement, usual sizes and order.
        s.bottom_h = Some(400.0);
        s.dock_widths = [Some(500.0), None];
        s.apply_header(DockWin::Inspector, ws::HeaderAction::Reorder(0, 2));
        s.reset_layout(crate::settings::Layout::Dock);
        assert_eq!(
            s.arrangement(),
            DockState::preset(crate::settings::Layout::Dock)
        );
        assert_eq!(s.tab_order, DockWin::ALL);
        // Moving a window by the menu brings it to the front where it lands.
        s.place_window(DockWin::Gauges, Place::Bottom);
        assert!(s.gauges.open && s.front[2] == Some(DockWin::Gauges));
        assert!(
            !s.available(DockWin::Cell),
            "no storm's details asked for yet"
        );
    }

    #[test]
    fn a_dock_never_takes_the_map() {
        assert_eq!(dock_max_width(1920.0), DOCK_MAX_W);
        assert_eq!(dock_max_width(900.0), 405.0);
        // Tinier than the minimum: the minimum yields rather than the map.
        assert!(dock_max_width(400.0) < DOCK_MIN_W);
    }

    /// The 3D view's representation row once ran wider than the right dock, and egui re-sized
    /// that panel from its screen edge: the column shrank and the map slid in under it.
    #[test]
    fn an_over_wide_row_does_not_narrow_the_right_dock() {
        let ctx = egui::Context::default();
        let raw = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 600.0),
            )),
            ..Default::default()
        };
        let map = |contain: bool| {
            let mut map = egui::Rect::NOTHING;
            for _ in 0..2 {
                let _ = ctx.run_ui(raw(), |root| {
                    egui::Panel::right(egui::Id::new(("right", contain)))
                        .exact_size(300.0)
                        .resizable(false)
                        .show(root, |ui| {
                            let row = |ui: &mut egui::Ui| {
                                ui.horizontal(|ui| ui.add_space(500.0));
                            };
                            if contain {
                                contained(ui, row);
                            } else {
                                row(ui);
                            }
                        });
                    map = root.available_rect_before_wrap();
                });
            }
            map
        };
        assert!(
            map(false).right() > 700.0,
            "egui still does this uncontained"
        );
        assert_eq!(map(true).right(), 700.0);
    }

    #[test]
    fn header_actions_move_fold_and_close_a_window() {
        let mut w = WindowChrome::at(true, Place::Float);
        apply_header(ws::HeaderAction::Collapse, &mut w);
        assert!(w.collapsed);
        // Docking a folded window unfolds it: a dock shows a window whole.
        apply_header(ws::HeaderAction::Place(Place::Right), &mut w);
        assert_eq!((w.place, w.collapsed), (Place::Right, false));
        apply_header(ws::HeaderAction::None, &mut w);
        assert!(w.open);
        apply_header(ws::HeaderAction::Close, &mut w);
        assert!(!w.open);
    }

    #[test]
    fn the_per_layout_arrangements_survive_the_settings_file() {
        use crate::settings::{Layout, Settings};
        let mut s = Settings::default();
        s.workstation
            .insert(Layout::Wsv3, DockState::preset(Layout::Wsv3));
        s.workstation
            .insert(Layout::Dock, DockState::preset(Layout::Dock));
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.workstation, s.workstation);
    }
}

#[cfg(test)]
mod overlay_tests {
    use super::cursor_readout;

    #[test]
    fn the_readout_gives_position_and_the_radars_range_and_bearing() {
        // Due east of the radar, one degree of longitude at 35 N: about 91 km.
        let rows = cursor_readout((-96.0, 35.0), Some((-97.0, 35.0)), true);
        let get = |k: &str| rows.iter().find(|r| r.0 == k).map(|r| r.1.clone()).unwrap();
        assert_eq!(get("Lat"), "35.00");
        assert_eq!(get("Lon"), "-96.00");
        let km: f64 = get("Range").trim_end_matches(" km").parse().unwrap();
        assert!((km - 91.0).abs() < 2.0, "{km}");
        let az: f64 = get("Az").trim_end_matches('\u{b0}').parse().unwrap();
        assert!((az - 90.0).abs() < 1.0, "{az}");
    }

    #[test]
    fn without_a_radar_there_is_no_range_or_bearing_to_invent() {
        let rows = cursor_readout((-96.0, 35.0), None, true);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn the_range_follows_the_units_setting() {
        let miles = cursor_readout((-96.0, 35.0), Some((-97.0, 35.0)), false);
        assert!(
            miles.iter().any(|r| r.0 == "Range" && r.1.ends_with(" mi")),
            "{miles:?}"
        );
    }
}

#[cfg(test)]
mod model_card_tests {
    use super::model_card_rows;
    use crate::model_browser::{BModel, Product, Selection};
    use crate::ui::model_panel::Input;
    use chrono::{DateTime, TimeZone, Utc};
    use wxdata::field::{DataStamp, QualitySummary};

    fn input(stamp: Option<DataStamp>, run: Option<DateTime<Utc>>) -> Input {
        Input {
            sel: Selection {
                model: BModel::Hrrr,
                product: Product::Reflectivity,
            },
            lead_min: 180,
            stamp,
            run,
            runs: Vec::new(),
            range: BModel::Hrrr.leads(),
        }
    }

    fn stamp(now: DateTime<Utc>) -> DataStamp {
        DataStamp {
            source_id: "HRRR".into(),
            product_id: "Composite reflectivity".into(),
            issue_time: None,
            run_time: Utc.with_ymd_and_hms(2026, 9, 20, 18, 0, 0).single(),
            valid_time: Utc.with_ymd_and_hms(2026, 9, 20, 21, 0, 0).unwrap(),
            received_time: now - chrono::Duration::minutes(4),
            source_latency: None,
            is_forecast: true,
            is_derived: false,
            quality: QualitySummary::Unknown,
            grid: None,
        }
    }

    fn get<'a>(rows: &'a [(&'static str, String)], key: &str) -> &'a str {
        &rows
            .iter()
            .find(|(k, _)| *k == key)
            .unwrap_or_else(|| panic!("no {key} row in {rows:?}"))
            .1
    }

    #[test]
    fn the_card_names_the_model_run_lead_and_freshness() {
        let now = Utc.with_ymd_and_hms(2026, 9, 20, 21, 30, 0).unwrap();
        let rows = model_card_rows(&input(Some(stamp(now)), None), None, now);
        assert_eq!(get(&rows, "Model:"), "HRRR");
        assert_eq!(get(&rows, "Product:"), "Reflectivity");
        assert_eq!(get(&rows, "Run:"), "20 18Z");
        assert_eq!(get(&rows, "Lead:"), "F+3h");
        assert_eq!(get(&rows, "Fetched:"), "4 min ago");
        assert!(!get(&rows, "Valid:").is_empty());
    }

    #[test]
    fn before_the_data_arrives_it_says_so_instead_of_inventing_a_time() {
        let now = Utc.with_ymd_and_hms(2026, 9, 20, 21, 30, 0).unwrap();
        let rows = model_card_rows(&input(None, None), None, now);
        assert_eq!(get(&rows, "Run:"), "latest");
        assert!(get(&rows, "Valid:").contains("loading"));
        assert!(rows.iter().all(|(k, _)| *k != "Fetched:"));
        // A run the user pinned shows even before its data lands.
        let pinned = Utc.with_ymd_and_hms(2026, 9, 20, 12, 0, 0).single();
        let rows = model_card_rows(&input(None, pinned), None, now);
        assert_eq!(get(&rows, "Run:"), "20 12Z");
    }
}

#[cfg(test)]
mod tilt_bar_tests {
    use super::*;

    fn progress(n: usize) -> wxdata::live::ScanProgress {
        wxdata::live::ScanProgress {
            volume_start_ms: None,
            vcp_number: None,
            cut_kind: wxdata::live::CutKind::Standard,
            elevation_number: n,
            total_elevations: 14,
            elevation_angle_deg: 0.9,
            azimuth_rate_dps: 18.0,
            azimuth_start_deg: 0.0,
            azimuth_end_deg: 120.0,
            chunk_index: 2,
            chunks_in_sweep: 3,
        }
    }

    const ELEV: [f32; 4] = [0.5, 0.9, 1.3, 1.8];

    #[test]
    fn the_sweeping_tilt_is_found_by_the_sweeps_angle() {
        // `progress` sweeps at 0.9°: the second tilt, whatever its place in the VCP.
        assert_eq!(sweeping_tilt(Some(progress(2)), &ELEV, true, true), Some(1));
        assert_eq!(sweeping_tilt(Some(progress(5)), &ELEV, true, true), Some(1));
        // An angle the volume does not have yet marks nothing rather than a wrong tilt.
        assert_eq!(sweeping_tilt(Some(progress(2)), &[0.5], true, true), None);
    }

    #[test]
    fn nothing_is_marked_when_not_streaming_switched_off_or_silent() {
        assert_eq!(sweeping_tilt(Some(progress(3)), &ELEV, false, true), None);
        assert_eq!(sweeping_tilt(Some(progress(3)), &ELEV, true, false), None);
        assert_eq!(sweeping_tilt(None, &ELEV, true, true), None);
        // Sweep zero is "not said yet".
        assert_eq!(sweeping_tilt(Some(progress(0)), &ELEV, true, true), None);
    }

    #[test]
    fn the_status_line_names_the_sweep_and_is_empty_when_not_live() {
        assert_eq!(live_status(Some(progress(3)), false, true), "");
        let s = live_status(Some(progress(3)), true, true);
        assert!(
            s.contains("0.9\u{b0}") && s.contains("3/14") && s.contains("chunk 2/3"),
            "{s}"
        );
        // Live but the indicator is off, or no chunk yet: say live, invent no sweep.
        assert_eq!(live_status(Some(progress(3)), true, false), "LIVE");
        assert_eq!(live_status(None, true, true), "LIVE");
    }
}
