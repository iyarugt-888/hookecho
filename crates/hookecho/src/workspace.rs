//! Saved pane layouts.
//!
//! Rebuilding "KTLX reflectivity beside KDMX velocity, storm-relative, with these overlays on"
//! costs a dozen clicks, and it's the layout people rebuild every time they sit down. A workspace
//! is a snapshot of that arrangement, restored in one command.
//!
//! It stores state, not actions: the command palette's actions all target the active pane, so a
//! recorded action list has no way to say "and pane two looks like this". What it deliberately
//! doesn't capture is anything tied to the moment rather than the arrangement — the archive
//! playhead, most open windows, per-moment thresholds. The one window an arrangement can ask for
//! is the sounding (`sound_center`), taken fresh at wherever the map is when it is applied.
//!
//! Per-pane display thresholds and field layers ride along, since a pane that shows one field
//! above 50 dBZ beside another that shows a different one is exactly the arrangement worth saving.

use crate::view::MapView;

/// How panes divide the available map area. `Balanced` is the familiar equal strip/grid;
/// `Focus` gives pane one the working area and tiles every other pane into an adaptive detail
/// rail, matching the asymmetric "large analysis + supporting products" layouts used in AWIPS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PaneLayout {
    #[default]
    Balanced,
    Focus,
}

impl PaneLayout {
    pub const ALL: [Self; 2] = [Self::Balanced, Self::Focus];

    pub fn label(self) -> &'static str {
        match self {
            Self::Balanced => "Even",
            Self::Focus => "Focus",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Balanced => "Give every pane equal space",
            Self::Focus => "Make pane 1 large and tile the other panes in a detail rail",
        }
    }
}

/// One saved pane arrangement.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Workspace {
    pub name: String,
    pub panes: Vec<PaneSnap>,
    /// Equal grid or AWIPS-style large-primary arrangement. Older workspaces remain balanced.
    #[serde(default)]
    pub pane_layout: PaneLayout,
    /// Which pane was focused.
    #[serde(default)]
    pub active: usize,
    #[serde(default)]
    pub link_cameras: bool,
    /// Archive pane time linking. Older workspaces remain independent.
    #[serde(default)]
    pub link_times: bool,
    /// Snap the linked analysis cursor to the active radar's actual scan after a nearest-frame
    /// seek. Older workspaces retain the requested valid time.
    #[serde(default)]
    pub lock_source_time: bool,
    /// ROADMAP_NEW J2: picking a new radar site in one pane sets it in every other pane too.
    /// `default` so a workspace saved before this field existed loads with it off.
    #[serde(default)]
    pub link_site: bool,
    /// ROADMAP_NEW J3: hovering any pane shows the same geographic point on every other pane, plus
    /// a compact probe table. `default` so a workspace saved before this field existed loads with
    /// it off.
    #[serde(default)]
    pub link_cursor: bool,
    /// ROADMAP_NEW J2: share the selected storm across panes (`OverlayToggle::LinkStorm`).
    #[serde(default)]
    pub link_storm: bool,
    /// Overlay toggles that were on, by slug — the same names `Settings::overlays_on` uses, so an
    /// unknown one from a newer build is skipped rather than fatal.
    #[serde(default)]
    pub overlays_on: Vec<String>,
    /// Fill panes that have no site from whatever site is on screen when the workspace is
    /// applied. Set on the starter workspaces, which describe an arrangement rather than a place:
    /// "two panes of the same radar" has to mean *your* radar, not one shipped in a default file.
    #[serde(default)]
    pub adopt_site: bool,
    /// National field layers that were on, by slug. Same forward-compatibility rule as the
    /// overlays; `default` so a workspace written before this field existed still loads.
    #[serde(default)]
    pub fields_on: Vec<String>,
    /// What the chrome looked like: which panel was showing, which drawer page was open. `None`
    /// on a workspace written before this field existed, and on the starters — both mean "leave
    /// the chrome where the user left it" rather than "close everything", which is what a
    /// defaulted struct would have meant.
    #[serde(default)]
    pub chrome: Option<Chrome>,
    /// Sound the point the map was centered on when the workspace is applied: an arrangement
    /// that needs the environment beside the radar (the hail preset) opens with its sounding.
    /// The point is the one the analyst was looking at before switching, not a saved one — a
    /// saved point would be yesterday's storm.
    #[serde(default)]
    pub sound_center: bool,
}

/// The floating chrome's state, as far as it is worth restoring: which surface was showing, not
/// how far it was scrolled.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Chrome {
    #[serde(default)]
    pub panel_open: bool,
    /// The panel's Alerts tab rather than Data.
    #[serde(default)]
    pub alerts_tab: bool,
    #[serde(default)]
    pub basemap_open: bool,
    /// Title of the drawer page on top, if any — matched back to a window by
    /// `app::chrome::window_for_page`. A title this build no longer has is skipped, same rule as
    /// the overlay slugs.
    #[serde(default)]
    pub drawer: Option<String>,
    /// The workstation layouts' window arrangement (Dock, WSV3), when one of them was showing.
    /// `None` from a floating-chrome layout and from files written before it existed.
    #[serde(default)]
    pub workstation: Option<WorkstationChrome>,
}

/// Where a workstation tool window sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Place {
    #[default]
    Left,
    Right,
    /// Over the map, movable, and collapsible to its title bar.
    Float,
}

/// One workstation tool window's state: shown or not, where it sits, and whether it is folded to
/// its title bar (only meaningful while it floats).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct WindowChrome {
    #[serde(default)]
    pub open: bool,
    #[serde(default)]
    pub place: Place,
    #[serde(default)]
    pub collapsed: bool,
}

impl WindowChrome {
    pub const fn at(open: bool, place: Place) -> Self {
        WindowChrome {
            open,
            place,
            collapsed: false,
        }
    }
}

/// The workstation layouts' window arrangement: each tool window's state, which workspace tab
/// the Layers window shows, and whether the timeline is up. Saved per layout in
/// `Settings::workstation` (so it survives a restart) and with a workspace (so a saved workspace
/// brings its arrangement back).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkstationChrome {
    /// The workspace tab by its label ("Radar", "GIS", ...); an unknown one falls back to Radar.
    #[serde(default)]
    pub tab: String,
    #[serde(default)]
    pub layers: WindowChrome,
    #[serde(default)]
    pub inspector: WindowChrome,
    #[serde(default)]
    pub alerts: WindowChrome,
    /// Map settings and the app preferences.
    #[serde(default)]
    pub prefs: WindowChrome,
    /// The active pane's 3D controls, shown while that pane is in 3D. Defaults to open and docked
    /// right, where it joins the Inspector as a tab.
    #[serde(default = "view3d_default")]
    pub view3d: WindowChrome,
    /// The compact source-health list. Closed until asked for; docks right when opened.
    #[serde(default = "sources_default")]
    pub sources: WindowChrome,
    /// Analyst Mode's live log, shown while Analyst Mode is on. Docks right by default.
    #[serde(default = "log_default")]
    pub log: WindowChrome,
    /// Each dock's width in whole pixels when the user has dragged it, left then right; `None`
    /// is the width its windows ask for.
    #[serde(default)]
    pub dock_widths: [Option<u16>; 2],
    /// The storm table. Closed until asked for; docks right when opened.
    #[serde(default = "storms_default")]
    pub storms: WindowChrome,
    /// The point sounding, shown once a point has been sounded. Docks right by default.
    #[serde(default = "sounding_default")]
    pub sounding: WindowChrome,
    #[serde(default = "yes")]
    pub timeline_open: bool,
    /// The status footer under the timeline (roadmap Q2). Off unless asked for.
    #[serde(default)]
    pub footer_open: bool,
}

fn yes() -> bool {
    true
}

fn view3d_default() -> WindowChrome {
    WindowChrome::at(true, Place::Right)
}

fn sources_default() -> WindowChrome {
    WindowChrome::at(false, Place::Right)
}

fn log_default() -> WindowChrome {
    WindowChrome::at(true, Place::Right)
}

fn storms_default() -> WindowChrome {
    WindowChrome::at(false, Place::Right)
}

fn sounding_default() -> WindowChrome {
    WindowChrome::at(true, Place::Right)
}

/// One pane's state. Camera as lon/lat/zoom, basemap as its slug: both survive a file written by
/// a different build.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneSnap {
    pub site: Option<String>,
    pub moment: wxdata::level2::Moment,
    pub tilt: usize,
    #[serde(default)]
    pub srv: bool,
    pub basemap: String,
    pub lon: f64,
    pub lat: f64,
    pub zoom: f64,
    /// Field layers this pane drew, by slug. Same forward-compatibility rule as the overlays: a
    /// slug this build does not have is skipped.
    ///
    /// `None` is a snapshot written before per-pane layers existed, which falls back to the
    /// workspace-wide list so an old file still restores what it meant. `Some([])` is a pane that
    /// deliberately had no layers on, and has to come back that way — as a plain `Vec` the two
    /// were the same value, so a pane you had cleared came back wearing the union of every other
    /// pane's layers.
    #[serde(default)]
    pub fields_on: Option<Vec<String>>,
    /// Enabled display thresholds, as `(moment, physical value)`. Only the enabled ones are
    /// stored: a threshold that was set but switched off is not part of the arrangement.
    #[serde(default)]
    pub thresholds: Vec<(wxdata::level2::Moment, f32)>,
}

impl PaneSnap {
    /// Snapshot a live pane.
    pub fn capture(v: &MapView) -> Self {
        let (lon, lat) =
            crate::render::mercator::world_to_lonlat(v.camera.center.0, v.camera.center.1);
        Self {
            site: v.site.clone(),
            moment: v.moment,
            tilt: v.tilt,
            srv: v.srv,
            basemap: v.basemap.slug().to_string(),
            lon,
            lat,
            zoom: v.camera.zoom,
            fields_on: Some(
                crate::render::FieldLayer::DRAW_ORDER
                    .iter()
                    .filter(|l| v.fields_on.contains(l))
                    .map(|l| l.slug().to_string())
                    .collect(),
            ),
            thresholds: wxdata::level2::Moment::ALL
                .iter()
                .enumerate()
                .filter(|&(i, _)| v.threshold_enabled[i])
                .filter_map(|(i, m)| v.thresholds[i].map(|t| (*m, t)))
                .collect(),
        }
    }

    /// Apply this snapshot to a pane. The volume itself isn't restored — a pane with a site and no
    /// data fetches through the normal poll path, which is also what a fresh pane does.
    pub fn apply(&self, v: &mut MapView) {
        v.site = self.site.clone();
        v.moment = self.moment;
        v.tilt = self.tilt;
        v.srv = self.srv;
        v.basemap = crate::tiles::BasemapStyle::from_slug(&self.basemap);
        v.camera = crate::render::mercator::Camera::at_lonlat(self.lon, self.lat, self.zoom);
        // The camera came from the saved layout, not from a site recenter — hold it through the
        // site change the restore just triggered.
        v.camera_placed = true;
        // Thresholds are a per-moment array; clear it first so restoring an arrangement without
        // one does not leave the previous pane's filter in place.
        v.thresholds = Default::default();
        v.threshold_enabled = Default::default();
        for (m, t) in &self.thresholds {
            v.thresholds[m.index()] = Some(*t);
            v.threshold_enabled[m.index()] = true;
        }
    }
}

/// Pane at a site-less default camera: the starters describe arrangements, and `adopt_site`
/// fills the site in when one is applied.
fn pane(moment: wxdata::level2::Moment, tilt: usize, srv: bool) -> PaneSnap {
    PaneSnap {
        site: None,
        moment,
        tilt,
        srv,
        basemap: "dark".into(),
        // Centered on the southern plains at a single-radar zoom; overwritten the moment the
        // pane adopts a site and recenters on it.
        lon: -97.5,
        lat: 35.5,
        zoom: 7.5,
        // `None`, not an empty list: a starter describes an arrangement, and its panes take the
        // workspace-wide layers rather than asserting that every pane is bare.
        fields_on: None,
        thresholds: Vec::new(),
    }
}

/// Bring seeded starters up to date with what the starters ask for now. Starters are copied into
/// the settings once, on first run, so a capability added later (the hail preset's sounding)
/// never reached them. Only a stored workspace that is still the starter — same name, same panes
/// — is touched; one the analyst has rebuilt is theirs. Returns whether anything changed.
pub fn upgrade_starters(saved: &mut [Workspace]) -> bool {
    let mut changed = false;
    for start in starters() {
        for w in saved.iter_mut() {
            if w.name == start.name
                && w.panes == start.panes
                && w.sound_center != start.sound_center
            {
                w.sound_center = start.sound_center;
                changed = true;
            }
        }
    }
    changed
}

/// The arrangements worth having before you have built any of your own. Seeded once, on first
/// run; deleting them is final (see `Settings::seeded_workspaces`).
pub fn starters() -> Vec<Workspace> {
    use wxdata::level2::Moment;
    vec![
        Workspace {
            name: "Chase".into(),
            pane_layout: PaneLayout::Balanced,
            // Reflectivity beside storm-relative velocity, same radar, cameras locked: the
            // couplet and the hook in one glance.
            panes: vec![
                pane(Moment::Reflectivity, 0, false),
                pane(Moment::Velocity, 0, true),
            ],
            active: 0,
            link_cameras: true,
            link_times: true,
            lock_source_time: false,
            link_site: true,
            link_cursor: true,
            link_storm: false,
            overlays_on: vec![
                "Alerts".into(),
                "Cells".into(),
                "Spotters".into(),
                "StormReports".into(),
                "Tds".into(),
            ],
            adopt_site: true,
            fields_on: Vec::new(),
            chrome: None,
            sound_center: false,
        },
        Workspace {
            name: "National overview".into(),
            pane_layout: PaneLayout::Balanced,
            // One pane, no site, the MRMS mosaic under the warnings — what is happening anywhere.
            panes: vec![PaneSnap {
                site: None,
                moment: Moment::Reflectivity,
                tilt: 0,
                srv: false,
                basemap: "dark".into(),
                lon: -97.0,
                lat: 38.5,
                zoom: 4.0,
                fields_on: Some(vec!["mrms".into()]),
                thresholds: Vec::new(),
            }],
            active: 0,
            link_cameras: false,
            link_times: false,
            lock_source_time: false,
            link_site: false,
            link_cursor: false,
            link_storm: false,
            overlays_on: vec!["Alerts".into(), "StormReports".into(), "Fronts".into()],
            adopt_site: false,
            fields_on: vec!["mrms".into()],
            chrome: None,
            sound_center: false,
        },
        Workspace {
            name: "Analysis".into(),
            pane_layout: PaneLayout::Balanced,
            // The same storm at four heights: how a couplet leans with height, which is the
            // layout people rebuild by hand every time.
            panes: (0..4)
                .map(|t| pane(Moment::Reflectivity, t, false))
                .collect(),
            active: 0,
            link_cameras: true,
            link_times: true,
            lock_source_time: true,
            link_site: true,
            link_cursor: true,
            link_storm: false,
            overlays_on: vec!["Alerts".into(), "Cells".into(), "RangeRings".into()],
            adopt_site: true,
            fields_on: Vec::new(),
            chrome: None,
            sound_center: false,
        },
        // ROADMAP_NEW J5's first three analyst presets. Each reuses exactly the same
        // pane/link/overlay mechanism as the three starters above — a preset is a description of
        // "when I sit down for this kind of event, this is the arrangement I rebuild by hand every
        // time", not a new capability. "Forecast comparison" comes last, after the others.
        Workspace {
            name: "Tornado analysis".into(),
            pane_layout: PaneLayout::Balanced,
            // The lowest cut of every dual-pol moment that actually separates a debris signature
            // from rain: reflectivity for the hook, storm-relative velocity for the couplet, CC
            // for non-meteorological scatterers, ZDR for the drop/debris size split.
            panes: vec![
                pane(Moment::Reflectivity, 0, false),
                pane(Moment::Velocity, 0, true),
                pane(Moment::CorrelationCoefficient, 0, false),
                pane(Moment::DifferentialReflectivity, 0, false),
            ],
            active: 0,
            link_cameras: true,
            link_times: true,
            lock_source_time: false,
            link_site: true,
            // The roadmap's own worked example for J3's synchronized crosshair: probing the same
            // point across all four panes at once is exactly how these moments get read together.
            link_cursor: true,
            link_storm: false,
            overlays_on: vec![
                "Alerts".into(),
                "Cells".into(),
                "StormReports".into(),
                "ProbSevere".into(),
                "Tds".into(),
            ],
            adopt_site: true,
            fields_on: Vec::new(),
            chrome: None,
            sound_center: false,
        },
        Workspace {
            name: "Hail analysis".into(),
            // Keep reflectivity large while the three dual-pol panes provide supporting detail.
            pane_layout: PaneLayout::Focus,
            // REF for the core, ZDR/KDP/CC for size and phase, MESH for the swath a single tilt
            // can't show by itself, and (`sound_center`) the sounding at the point in view: the
            // freezing level and CAPE a hail call needs beside the radar.
            panes: vec![
                pane(Moment::Reflectivity, 0, false),
                pane(Moment::DifferentialReflectivity, 0, false),
                pane(Moment::CorrelationCoefficient, 0, false),
                pane(Moment::SpecificDifferentialPhase, 0, false),
            ],
            active: 0,
            link_cameras: true,
            link_times: true,
            lock_source_time: false,
            link_site: true,
            link_cursor: true,
            link_storm: false,
            overlays_on: vec!["Alerts".into(), "Cells".into(), "StormReports".into()],
            adopt_site: true,
            fields_on: vec!["mesh".into()],
            chrome: None,
            sound_center: true,
        },
        Workspace {
            name: "Mesoscale analysis".into(),
            pane_layout: PaneLayout::Balanced,
            // One national-scale pane: the environment fields that set the stage rather than one
            // storm's own radar signature. CAPE/SRH read from the Environment section's own model
            // choice, same as everywhere else they appear.
            panes: vec![PaneSnap {
                site: None,
                moment: Moment::Reflectivity,
                tilt: 0,
                srv: false,
                basemap: "dark".into(),
                lon: -97.0,
                lat: 38.5,
                zoom: 4.0,
                fields_on: Some(vec![
                    "goes-ir".into(),
                    "cape".into(),
                    "srh".into(),
                    "global-dewpoint2m".into(),
                ]),
                thresholds: Vec::new(),
            }],
            active: 0,
            link_cameras: false,
            link_times: false,
            lock_source_time: false,
            link_site: false,
            link_cursor: false,
            link_storm: false,
            overlays_on: vec!["Alerts".into(), "Fronts".into(), "StormReports".into()],
            adopt_site: false,
            fields_on: vec![
                "goes-ir".into(),
                "cape".into(),
                "srh".into(),
                "global-dewpoint2m".into(),
            ],
            chrome: None,
            sound_center: false,
        },
        Workspace {
            name: "Radar + satellite".into(),
            pane_layout: PaneLayout::Balanced,
            // ROADMAP_NEW E6's "radar + satellite dual/quad pane presets." Every pane keeps a
            // real radar site (`adopt_site`, like Chase/Analysis/Tornado/Hail above) rather than
            // going site-less the way "Mesoscale analysis" does — a site-less pane's `site: None`
            // would itself get adopted here (see `apply_workspace`'s "if the snap has no site,
            // fill it from the active one" rule), which is right for an all-radar preset but would
            // wrongly turn a *deliberately* national satellite pane into a personalized radar one.
            // So satellite context rides along on top of a radar pane instead, via that pane's own
            // `fields_on` — reflectivity alone as the familiar baseline, then the same radar
            // moment again with IR and water-vapor satellite context layered underneath it, plus
            // storm-relative velocity for motion.
            panes: vec![
                pane(Moment::Reflectivity, 0, false),
                PaneSnap {
                    fields_on: Some(vec!["goes-ir".into()]),
                    ..pane(Moment::Reflectivity, 0, false)
                },
                PaneSnap {
                    fields_on: Some(vec!["goes-water-vapor".into()]),
                    ..pane(Moment::Reflectivity, 0, false)
                },
                pane(Moment::Velocity, 0, true),
            ],
            active: 0,
            link_cameras: true,
            link_times: true,
            lock_source_time: false,
            link_site: true,
            link_cursor: true,
            link_storm: false,
            overlays_on: vec!["Alerts".into(), "Cells".into(), "StormReports".into()],
            adopt_site: true,
            fields_on: vec!["goes-ir".into(), "goes-water-vapor".into()],
            chrome: None,
            sound_center: false,
        },
        Workspace {
            name: "Forecast comparison".into(),
            pane_layout: PaneLayout::Balanced,
            // ROADMAP_NEW J5's "Forecast comparison": what the MRMS mosaic observes beside what
            // HRRR forecasts, cameras and cursor linked so a storm can be read across both. Both
            // panes are national and site-less (`adopt_site` off), like "Mesoscale analysis". The
            // roadmap's RRFS and ensemble-probability halves are not shipped: RRFS is not a data
            // source this app has, and ensemble probability is F7's own not-started feature.
            panes: vec![
                PaneSnap {
                    site: None,
                    moment: Moment::Reflectivity,
                    tilt: 0,
                    srv: false,
                    basemap: "dark".into(),
                    lon: -97.0,
                    lat: 38.5,
                    zoom: 4.5,
                    fields_on: Some(vec!["mrms".into()]),
                    thresholds: Vec::new(),
                },
                PaneSnap {
                    site: None,
                    moment: Moment::Reflectivity,
                    tilt: 0,
                    srv: false,
                    basemap: "dark".into(),
                    lon: -97.0,
                    lat: 38.5,
                    zoom: 4.5,
                    fields_on: Some(vec!["hrrr".into()]),
                    thresholds: Vec::new(),
                },
            ],
            active: 0,
            link_cameras: true,
            link_times: false,
            lock_source_time: false,
            link_site: false,
            link_cursor: true,
            link_storm: false,
            overlays_on: vec!["Alerts".into(), "StormReports".into()],
            adopt_site: false,
            fields_on: vec!["mrms".into(), "hrrr".into()],
            chrome: None,
            sound_center: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_roundtrips_through_a_view() {
        let mut v = MapView::new(
            Some("KTLX".into()),
            crate::render::mercator::Camera::at_lonlat(-97.28, 35.33, 8.5),
        );
        v.moment = wxdata::level2::Moment::Velocity;
        v.tilt = 2;
        v.srv = true;
        v.basemap = crate::tiles::BasemapStyle::from_slug("satellite");

        let snap = PaneSnap::capture(&v);
        let mut fresh = MapView::new(
            None,
            crate::render::mercator::Camera::at_lonlat(0.0, 0.0, 3.0),
        );
        snap.apply(&mut fresh);

        assert_eq!(fresh.site.as_deref(), Some("KTLX"));
        assert_eq!(fresh.moment, wxdata::level2::Moment::Velocity);
        assert_eq!(fresh.tilt, 2);
        assert!(fresh.srv && fresh.camera_placed);
        assert_eq!(fresh.basemap, v.basemap);
        // Camera survives the lon/lat round trip to within a pixel at this zoom.
        assert!((fresh.camera.center.0 - v.camera.center.0).abs() < 1e-9);
        assert!((fresh.camera.center.1 - v.camera.center.1).abs() < 1e-9);
        assert_eq!(fresh.camera.zoom, 8.5);
    }

    #[test]
    fn per_pane_thresholds_and_layers_survive_the_round_trip() {
        use wxdata::level2::Moment;
        let mut v = MapView::new(
            None,
            crate::render::mercator::Camera::at_lonlat(0.0, 0.0, 3.0),
        );
        v.fields_on.insert(crate::render::FieldLayer::Mrms);
        v.thresholds[Moment::Reflectivity.index()] = Some(35.0);
        v.threshold_enabled[Moment::Reflectivity.index()] = true;
        // Set but switched off: part of the session, not part of the arrangement.
        v.thresholds[Moment::Velocity.index()] = Some(20.0);

        let snap = PaneSnap::capture(&v);
        assert_eq!(
            snap.fields_on.as_deref(),
            Some(["mrms".to_string()].as_slice())
        );
        assert_eq!(snap.thresholds, vec![(Moment::Reflectivity, 35.0)]);

        let mut fresh = MapView::new(
            None,
            crate::render::mercator::Camera::at_lonlat(0.0, 0.0, 3.0),
        );
        // A leftover filter from whatever this pane was showing before must not survive a restore.
        fresh.thresholds[Moment::SpectrumWidth.index()] = Some(4.0);
        fresh.threshold_enabled[Moment::SpectrumWidth.index()] = true;
        snap.apply(&mut fresh);
        assert_eq!(fresh.thresholds[Moment::Reflectivity.index()], Some(35.0));
        assert!(fresh.threshold_enabled[Moment::Reflectivity.index()]);
        assert!(!fresh.threshold_enabled[Moment::SpectrumWidth.index()]);
        assert_eq!(fresh.thresholds[Moment::Velocity.index()], None);
    }

    #[test]
    fn a_pane_with_no_layers_says_so_rather_than_saying_nothing() {
        // The distinction the `Option` exists for. Capturing a bare pane must record "no layers",
        // not "no opinion" — as a plain `Vec` both were `[]`, and a pane you had cleared came back
        // wearing the union of every other pane's layers when the workspace was applied.
        let v = MapView::new(
            None,
            crate::render::mercator::Camera::at_lonlat(-97.3, 35.3, 8.0),
        );
        let snap = PaneSnap::capture(&v);
        assert_eq!(snap.fields_on, Some(Vec::new()));
    }

    #[test]
    fn a_pane_written_before_per_pane_layers_still_loads() {
        // No `fields_on`, no `thresholds` — exactly what an older build wrote.
        let snap: PaneSnap = serde_json::from_str(
            r#"{"site":"KTLX","moment":"Reflectivity","tilt":0,"basemap":"dark",
                "lon":-97.3,"lat":35.3,"zoom":8.0}"#,
        )
        .expect("old pane snapshots still parse");
        // `None`, not an empty list: this file predates per-pane layers and says nothing about
        // them, so the restore falls back to the workspace-wide list. A pane that really had its
        // layers off captures `Some([])`, which is a different instruction.
        assert_eq!(snap.fields_on, None);
        assert!(snap.thresholds.is_empty());
    }

    #[test]
    fn workspace_roundtrips_through_json() {
        let ws = Workspace {
            name: "Two-site chase".into(),
            pane_layout: PaneLayout::Focus,
            panes: vec![PaneSnap {
                site: Some("KDMX".into()),
                moment: wxdata::level2::Moment::CorrelationCoefficient,
                tilt: 1,
                srv: false,
                basemap: "dark".into(),
                lon: -93.72,
                lat: 41.73,
                zoom: 7.25,
                fields_on: None,
                thresholds: Vec::new(),
            }],
            active: 0,
            link_cameras: true,
            link_times: true,
            lock_source_time: true,
            link_site: true,
            link_cursor: true,
            link_storm: false,
            overlays_on: vec!["Alerts".into(), "Cells".into()],
            adopt_site: false,
            fields_on: vec!["mrms".into()],
            sound_center: true,
            chrome: Some(Chrome {
                panel_open: true,
                alerts_tab: false,
                basemap_open: false,
                drawer: Some("Settings".into()),
                workstation: Some(WorkstationChrome {
                    tab: "GIS".into(),
                    layers: WindowChrome::at(true, Place::Right),
                    inspector: WindowChrome {
                        open: true,
                        place: Place::Float,
                        collapsed: true,
                    },
                    alerts: WindowChrome::at(false, Place::Right),
                    prefs: WindowChrome::default(),
                    view3d: WindowChrome::at(true, Place::Float),
                    sources: WindowChrome::at(true, Place::Left),
                    log: WindowChrome::at(true, Place::Right),
                    dock_widths: [Some(320), None],
                    sounding: WindowChrome::at(true, Place::Float),
                    storms: WindowChrome::at(false, Place::Right),
                    timeline_open: false,
                    footer_open: false,
                }),
            }),
        };
        let json = serde_json::to_string(&ws).unwrap();
        assert_eq!(serde_json::from_str::<Workspace>(&json).unwrap(), ws);
    }

    #[test]
    fn a_seeded_starter_picks_up_new_capabilities_and_a_rebuilt_one_does_not() {
        let mut saved = starters();
        for w in &mut saved {
            w.sound_center = false; // as seeded before the field existed
        }
        let mut rebuilt = saved
            .iter()
            .find(|w| w.name == "Hail analysis")
            .unwrap()
            .clone();
        rebuilt.panes.truncate(1);
        saved.push(rebuilt);
        assert!(upgrade_starters(&mut saved));
        let hail: Vec<bool> = saved
            .iter()
            .filter(|w| w.name == "Hail analysis")
            .map(|w| w.sound_center)
            .collect();
        assert_eq!(hail, [true, false]);
        assert!(!upgrade_starters(&mut saved), "once is enough");
    }

    #[test]
    fn only_the_hail_starter_opens_a_sounding() {
        let sounding: Vec<String> = starters()
            .into_iter()
            .filter(|w| w.sound_center)
            .map(|w| w.name)
            .collect();
        assert_eq!(sounding, ["Hail analysis"]);
    }

    #[test]
    fn old_workspace_files_still_load() {
        // Written before `fields_on` and `adopt_site` existed.
        let json = r#"{"name":"old","panes":[],"active":0,"link_cameras":false,
            "overlays_on":["Alerts"]}"#;
        let ws: Workspace = serde_json::from_str(json).unwrap();
        assert!(ws.fields_on.is_empty() && !ws.adopt_site && ws.chrome.is_none());
        assert!(
            !ws.sound_center,
            "an old file opens no sounding it never asked for"
        );
        assert!(!ws.link_times);
        assert!(!ws.lock_source_time);
        assert!(!ws.link_site);
        assert!(!ws.link_cursor);
        assert_eq!(ws.pane_layout, PaneLayout::Balanced);
    }

    #[test]
    fn every_starter_names_things_this_build_has() {
        for ws in starters() {
            assert!(!ws.panes.is_empty(), "{} has no panes", ws.name);
            assert!(ws.active < ws.panes.len());
            for slug in &ws.fields_on {
                assert!(
                    crate::render::FieldLayer::from_slug(slug).is_some(),
                    "{}: unknown field layer {slug}",
                    ws.name
                );
            }
            for p in &ws.panes {
                // `from_slug` falls back to Dark on an unknown name, so compare round trips.
                let style = crate::tiles::BasemapStyle::from_slug(&p.basemap);
                assert_eq!(style.slug(), p.basemap, "{}: bad basemap", ws.name);
            }
        }
    }
}
