//! The action vocabulary: [`AppWindow`] (every window the palette can open) and [`PaletteAction`]
//! (every thing the user can do, addressable from any surface — layers panel, command palette,
//! menus, keybindings). Variant names are what keybindings and workspaces serialize, so they do
//! not change here. Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

/// A floating window the palette can open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum AppWindow {
    Site,
    Settings,
    Markers,
    Placefiles,
    Palettes,
    Events,
    /// Replay a recorded drive against the archive.
    ChaseReplay,
    Digest,
    Afd,
    Cappi,
    Volume3d,
    StormTable,
    /// Every river gauge in view, with flood categories, sparklines and the selected hydrograph.
    FloodGauges,
    /// Phase C1: define/edit GR2Analyst-style formula products, evaluated live in the gate
    /// inspector.
    UdpProducts,
    /// Shortcuts, vocabulary, the tour, and what changed — one searchable page.
    #[serde(alias = "Glossary")]
    Help,
    /// The user's own alert rules.
    AlertRules,
    /// Warning verification lab (IEM Cow): how the office's warnings scored on an event day.
    Verify,
    /// ROADMAP_NEW K1: score a forecast run against the RTMA analysis for the same hours.
    ModelVerify,
    Climatology,
    LayerManager,
    /// First-run setup: which radar to open to.
    #[serde(alias = "Wizard")]
    Setup,
    /// The spotlight tour of the live chrome.
    Tour,
    About,
    /// ROADMAP_NEW N1: every active source's fetch health in one list.
    DataHealth,
    /// Tropical model guidance (spaghetti), intensity guidance, and NHC advisories.
    Tropical,
}

/// A step through the analysis: tilt, frame, an hour, product, or play/pause the loop. The keys
/// already did these (`hotkeys::BindableAction`); this is what makes them palette rows too, so a
/// storm can be worked from Ctrl+K and pointer alone (ROADMAP_2 §0.2). Its own enum because
/// `BindableAction` already holds a `PaletteAction` and cannot be held by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum NavStep {
    TiltUp,
    TiltDown,
    StepBack,
    StepForward,
    StepHourBack,
    StepHourForward,
    ProductPrev,
    ProductNext,
    PlayPause,
}

impl NavStep {
    #[cfg(test)]
    pub(crate) const ALL: [NavStep; 9] = [
        NavStep::TiltUp,
        NavStep::TiltDown,
        NavStep::StepBack,
        NavStep::StepForward,
        NavStep::StepHourBack,
        NavStep::StepHourForward,
        NavStep::ProductPrev,
        NavStep::ProductNext,
        NavStep::PlayPause,
    ];

    /// The key action that already does this step, if one does (play/pause had no key).
    pub(crate) fn bindable(self) -> Option<crate::hotkeys::BindableAction> {
        use crate::hotkeys::BindableAction as A;
        Some(match self {
            NavStep::TiltUp => A::TiltUp,
            NavStep::TiltDown => A::TiltDown,
            NavStep::StepBack => A::StepBack,
            NavStep::StepForward => A::StepForward,
            NavStep::StepHourBack => A::StepHourBack,
            NavStep::StepHourForward => A::StepHourForward,
            NavStep::ProductPrev => A::ProductPrev,
            NavStep::ProductNext => A::ProductNext,
            NavStep::PlayPause => return None,
        })
    }
}

/// One thing the user can do, addressable from any surface (layers panel, command palette,
/// mobile quick-layers sheet). The single registry keeps those surfaces in sync for free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum PaletteAction {
    /// Select a radar moment; the bool is the storm-relative flag (velocity only).
    SetMoment(Moment, bool),
    /// Switch the active pane straight to this radar site — the search box's answer to typing a
    /// station id or a city name, rather than only opening the site picker dialog to do it in a
    /// second step. A fixed, zero-padded byte buffer rather than a `String`/`&str`: this enum
    /// derives `Copy` and round-trips through the keybind system's JSON, and a borrowed
    /// `&'static str` cannot implement `Deserialize` (its lifetime would have to outlive the
    /// deserializer's own input). Site ids top out at 5 ASCII characters today (OPERA); 8 bytes
    /// is comfortable headroom. Build one with [`encode_site_id`], read it back with
    /// [`decode_site_id`].
    SetSite([u8; 8]),
    /// Seek the active radar timeline to a UTC instant (Unix seconds).
    SeekTime(i64),
    /// Four panes, one product, four distinct tilts, cameras linked.
    AllTilts,
    /// Remove every manual storm-motion track (`app::storm_track`).
    ClearStormTracks,
    /// Open or close the clean output window (`app::output_window`).
    ToggleOutputWindow,
    /// Two panes, one model's own field in each (`app.diff_field`), cameras linked — the
    /// side-by-side alternative to the `ModelDiff` subtraction layer.
    CompareInPanes,
    /// ROADMAP_NEW F6/J4 "blink A/B": alternate the active pane's own field between the two
    /// compared models on a timer, instead of splitting into two linked panes.
    ToggleBlinkCompare,
    /// ROADMAP_NEW F6/J4 transparent overlay: draw A normally and B at half opacity in one pane.
    ToggleCompareOverlay,
    /// ROADMAP_NEW F6/J4 swipe: A left, B right, with a draggable divider.
    ToggleCompareSwipe,
    ToggleField(crate::render::FieldLayer),
    /// Model browser: pick a model, keeping the current product when that model has it.
    SetModel(crate::model_browser::BModel),
    /// Model browser: pick a product and show it, moving to a model that has it if needed.
    SetModelProduct(crate::model_browser::Product),
    /// Model browser, a layers-list row: show this product, or clear it if it is showing.
    ToggleModelProduct(crate::model_browser::Product),
    /// Model browser: scrub the forecast to this lead, in minutes.
    SetModelLead(u16),
    /// Model browser: use this model run (Unix seconds), or `None` for the newest available.
    SetModelRun(Option<i64>),
    /// Model browser: swipe the selected model against its natural counterpart at this lead.
    CompareSelected,
    /// Model browser: step the lead by this many of the model's own steps.
    StepModelLead(i8),
    ToggleOverlay(OverlayToggle),
    SetContours(ContourKind),
    Tool(MapTool),
    OpenWindow(AppWindow),
    SetPanes(usize),
    /// Switch between equal panes and an AWIPS-style large pane 1 with a supporting detail rail.
    SetPaneLayout(crate::workspace::PaneLayout),
    CycleBasemap,
    ToggleMute,
    /// Show/hide the docked timeline bar under the map (desktop).
    /// Show/hide the docked sidebar on the left (desktop).
    TogglePanel,
    /// Show/hide the WSV3 ribbon and its docked colour scale, for a full-window map view.
    ToggleRibbon,
    /// Show or hide one of the workstation's tool windows (bringing it forward if it is behind
    /// another tab), so each is reachable from search, Ctrl+K and a key binding.
    DockWindow(chrome::DockWin),
    Reload,
    InstantReplay,
    GoLive,
    /// Tilt, frame, hour, product or play/pause (`NavStep`).
    Nav(NavStep),
    /// Hand the current view off to windy.com in the browser.
    OpenInWindy,
    /// Open the file picker to import a GeoJSON file (ROADMAP_NEW I1).
    ImportGis,
    /// Write everything currently drawn on the map out as GeoJSON (ROADMAP_NEW I6).
    ExportGis,
    /// Frame the active pane on the last GeoJSON import's own extent.
    ZoomToGis,
    /// Switch the active pane between the flat map and the map-pitch 3D view (ROADMAP_NEW J6).
    ToggleMap3d,
    /// While live, change tilt as each new sweep starts (`MapView::follow_live_sweep`).
    ToggleFollowSweep,
    /// While live, jump to the lowest tilt each time it is rescanned (`follow_lowest_cut`).
    ToggleFollowLowest,
    /// The workstation's status footer under the timeline (roadmap Q2).
    ToggleStatusFooter,
    /// Put the workstation's windows back where the layout starts them.
    ResetWindowLayout,
    /// ROADMAP_NEW J6 "link/unlink": every pane link at once — on if any is off, else all off.
    ToggleLinkAll,
    /// Copy a `hookecho://goto/…` link to this view (site, center, zoom, archive time).
    CopyViewLink,
    /// Open Help at the glossary entry that explains a label's abbreviation. An index into
    /// `ui::glossary::ENTRIES` rather than the term itself, so the action stays `Copy`.
    Explain(usize),
    /// Snapshot the current pane layout as a new workspace.
    SaveWorkspace,
    /// Restore the saved workspace at this index (an index, not the workspace itself, so the enum
    /// stays `Copy` and the palette rows stay cheap).
    ApplyWorkspace(usize),
}

/// Pack a site id into [`PaletteAction::SetSite`]'s fixed buffer. Truncates past 8 bytes, which
/// no real site id reaches (see that variant's own doc comment).
pub(crate) fn encode_site_id(id: &str) -> [u8; 8] {
    let mut buf = [0u8; 8];
    let bytes = id.as_bytes();
    let n = bytes.len().min(buf.len());
    buf[..n].copy_from_slice(&bytes[..n]);
    buf
}

/// The inverse of [`encode_site_id`].
pub(crate) fn decode_site_id(buf: [u8; 8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}
