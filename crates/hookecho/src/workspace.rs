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
    /// The imported GIS layers as saved (ROADMAP_PARITY M4.4); `None` in a workspace saved
    /// before they were kept, or with none imported, which leaves the layers as they are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gis: Option<crate::settings::GisSnapshot>,
    /// Fields this build does not know, written by a newer one: kept, so opening and saving a
    /// workspace here does not silently drop what a later version put in it.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    /// Docked under the map, between the side docks: for the wide, short windows (the storm and
    /// gauge tables, the analyst log).
    Bottom,
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
    /// The complete Settings editor, closed and floating until asked for.
    #[serde(default = "settings_default")]
    pub settings: WindowChrome,
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
    /// The bottom dock's height in whole pixels when the user has dragged it; `None` is its
    /// default.
    #[serde(default)]
    pub bottom_h: Option<u16>,
    /// The order the windows' tabs were dragged into, by window name (`"Storms"`); windows not
    /// named keep their usual place after the named ones. Empty is the usual order.
    #[serde(default)]
    pub tab_order: Vec<String>,
    /// The storm table. Closed until asked for; docks right when opened.
    #[serde(default = "storms_default")]
    pub storms: WindowChrome,
    /// The point sounding, shown once a point has been sounded. Docks right by default.
    #[serde(default = "sounding_default")]
    pub sounding: WindowChrome,
    /// The selected storm's details, shown once Details… is asked for. Docks right by default.
    #[serde(default = "sounding_default")]
    pub cell: WindowChrome,
    /// The region-statistics box, shown once one is drawn. Docks right by default.
    #[serde(default = "sounding_default")]
    pub region: WindowChrome,
    /// The orbitable 3D volume, shown while it is open. Floats over the map by default.
    #[serde(default = "volume_default")]
    pub volume: WindowChrome,
    /// The flood-gauge dashboard. Closed until asked for; docks right when opened.
    #[serde(default = "storms_default")]
    pub gauges: WindowChrome,
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

fn volume_default() -> WindowChrome {
    WindowChrome::at(true, Place::Float)
}

fn settings_default() -> WindowChrome {
    WindowChrome::at(false, Place::Float)
}

#[cfg(test)]
mod settings_chrome_tests {
    use super::*;

    #[test]
    fn older_arrangements_keep_main_settings_closed_and_floating() {
        let chrome: WorkstationChrome = serde_json::from_str("{}").unwrap();
        assert_eq!(chrome.settings, WindowChrome::at(false, Place::Float));
    }
}

#[cfg(test)]
mod spatial_link_tests {
    use super::*;
    use crate::pane_links::{Dimension, Link, SpatialLinks};
    use crate::render::mercator::Camera;
    fn view(site: &str, lon: f64) -> MapView {
        MapView::new(Some(site.into()), Camera::at_lonlat(lon, 35.0, 8.0))
    }
    #[test]
    fn spatial_links_workspace_round_trip_keeps_each_dimension_and_model_group() {
        let mut source = view("KTLX", -97.0);
        source.spatial_links = SpatialLinks {
            camera: Link {
                group: 2,
                enabled: true,
            },
            site: Link {
                group: 3,
                enabled: false,
            },
            cursor: Link {
                group: 1,
                enabled: true,
            },
        };
        source.model_group = Some(4);
        let saved = PaneSnap::capture(&source);
        let bytes = serde_json::to_vec(&saved).unwrap();
        let decoded: PaneSnap = serde_json::from_slice(&bytes).unwrap();
        let mut restored = view("KDMX", -88.0);
        decoded.apply(&mut restored);
        assert_eq!(restored.spatial_links, source.spatial_links);
        assert_eq!(restored.model_group, Some(4));
        assert_eq!(restored.site, source.site);
        assert_eq!(restored.camera.center, source.camera.center);
        assert!(restored.spatial_restore_raw.is_none());
    }
    #[test]
    fn a_time_group_round_trips_and_older_or_odd_files_load_in_group_one() {
        let mut source = view("KTLX", -97.0);
        source.time_group = 3;
        let saved = PaneSnap::capture(&source);
        assert_eq!(saved.extra.get("time-group"), Some(&serde_json::json!(3)));
        let decoded: PaneSnap =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        let mut restored = view("KDMX", -88.0);
        decoded.apply(&mut restored);
        assert_eq!(restored.time_group, 3);
        // Group 1 is not written, so a one-group workspace is what it was before groups.
        let default = PaneSnap::capture(&view("KTLX", -97.0));
        assert!(!default.extra.contains_key("time-group"));
        for odd in [
            serde_json::json!(0),
            serde_json::json!(99),
            serde_json::json!("x"),
        ] {
            let mut snap = saved.clone();
            snap.extra.insert("time-group".into(), odd);
            let mut v = view("KDMX", -88.0);
            v.time_group = 2;
            snap.apply(&mut v);
            assert_eq!(v.time_group, 1);
        }
        let mut legacy = saved.clone();
        legacy.extra.remove("time-group");
        let mut v = view("KDMX", -88.0);
        v.time_group = 2;
        legacy.apply(&mut v);
        assert_eq!(
            v.time_group, 1,
            "a workspace from before groups is one group"
        );
    }
    #[test]
    fn spatial_links_legacy_flags_migrate_to_one_group_and_camera_uses_saved_focus() {
        let mut ws = starters().remove(0);
        ws.panes = vec![
            PaneSnap::capture(&view("KTLX", -97.0)),
            PaneSnap::capture(&view("KDMX", -88.0)),
        ];
        for snap in &mut ws.panes {
            snap.extra.remove("spatial-links");
        }
        ws.link_cameras = true;
        ws.link_site = false;
        ws.link_cursor = true;
        let mut restored = vec![view("KOUN", -98.0), view("KOUN", -98.0)];
        for (snap, view) in ws.panes.iter().zip(&mut restored) {
            snap.apply(view);
        }
        restore_spatial_memberships(&ws, &mut restored, 1);
        for view in &restored {
            assert_eq!(view.spatial_links, SpatialLinks::legacy(true, false, true));
            assert_eq!(view.camera.center, restored[1].camera.center);
        }
        assert_eq!(restored[0].site.as_deref(), Some("KTLX"));
        assert!(conflicting_spatial_groups(&ws).is_empty());
    }
    #[test]
    fn spatial_links_future_or_invalid_saved_memberships_disable_and_round_trip() {
        let mut ws = starters().remove(0);
        for value in [
            serde_json::json!({"schema": 7, "future": ["clock", "cursor"]}),
            serde_json::json!({"schema": 1, "links": {"camera": {"group": 0, "enabled": true}, "site": {"group": 1, "enabled": false}, "cursor": {"group": 1, "enabled": true}}}),
            serde_json::json!({"schema": 1, "links": {"camera": {"group": 255, "enabled": true}, "site": {"group": 1, "enabled": false}, "cursor": {"group": 1, "enabled": true}}}),
            serde_json::json!({"schema": 1, "links": {"camera": {"group": 1, "enabled": true}}}),
        ] {
            let mut snap = PaneSnap::capture(&view("KTLX", -97.0));
            snap.extra.insert("spatial-links".into(), value.clone());
            ws.panes = vec![snap.clone()];
            assert!(problems(&ws)
                .iter()
                .any(|p| p.contains("invalid spatial links")));
            let mut restored = view("KDMX", -88.0);
            restored.spatial_links = SpatialLinks::legacy(true, true, true);
            snap.apply(&mut restored);
            restore_spatial_memberships(&ws, std::slice::from_mut(&mut restored), 0);
            assert_eq!(restored.spatial_links, SpatialLinks::default());
            assert_eq!(restored.site.as_deref(), Some("KTLX"));
            assert_eq!(PaneSnap::capture(&restored).extra["spatial-links"], value);
        }
    }
    #[test]
    fn spatial_links_conflicting_saved_owners_preserve_views_and_other_dimensions() {
        let mut ws = starters().remove(0);
        let mut first = view("KTLX", -97.0);
        let mut second = view("KDMX", -88.0);
        first.spatial_links = SpatialLinks::legacy(true, true, true);
        second.spatial_links = first.spatial_links;
        ws.panes = vec![PaneSnap::capture(&first), PaneSnap::capture(&second)];
        assert_eq!(
            conflicting_spatial_groups(&ws),
            vec![(Dimension::Camera, 1), (Dimension::Site, 1)]
        );
        assert!(problems(&ws).iter().any(|p| p.contains("Camera group 1")));
        let mut restored = vec![view("KOUN", -98.0), view("KOUN", -98.0)];
        for (snap, view) in ws.panes.iter().zip(&mut restored) {
            snap.apply(view);
        }
        restore_spatial_memberships(&ws, &mut restored, 0);
        for view in &restored {
            assert!(!view.spatial_links.camera.enabled);
            assert!(!view.spatial_links.site.enabled);
            assert!(view.spatial_links.cursor.enabled);
        }
        assert_eq!(restored[0].site.as_deref(), Some("KTLX"));
        assert_eq!(restored[1].site.as_deref(), Some("KDMX"));
        assert_eq!(restored[0].camera.center, first.camera.center);
        assert_eq!(restored[1].camera.center, second.camera.center);
    }
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
    /// Fields this build does not know, written by a newer one: kept, so opening and saving a
    /// workspace here does not silently drop what a later version put in it.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PaneSnap {
    /// Snapshot a live pane.
    pub fn model_context_valid(&self) -> bool {
        self.extra.get("models").is_none_or(|value| {
            serde_json::from_value::<crate::model_pane::SavedModelContext>(value.clone())
                .is_ok_and(|saved| saved.valid())
        })
    }
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
            extra: [
                (
                    "models".into(),
                    v.model_restore_raw.clone().unwrap_or_else(|| {
                        serde_json::to_value(crate::model_pane::SavedModelContext {
                            schema: 1,
                            group: v.model_group,
                            controls: v.models.clone(),
                        })
                        .expect("Model controls are serializable")
                    }),
                ),
                (
                    "spatial-links".into(),
                    v.spatial_restore_raw.clone().unwrap_or_else(|| {
                        serde_json::to_value(crate::pane_links::SavedSpatialLinks {
                            schema: 1,
                            links: v.spatial_links,
                        })
                        .expect("Spatial links are serializable")
                    }),
                ),
            ]
            .into_iter()
            // The pane's analysis-time group (M5.1), only when it is not the default group 1,
            // so a workspace from before groups, or with one group, stays byte-identical.
            .chain((v.time_group != 1).then(|| {
                (
                    "time-group".to_string(),
                    serde_json::Value::from(v.time_group),
                )
            }))
            // The pane's column user product, by name (`MapView::column_product`); the saved
            // definition itself lives with the products in Settings. Absent when none is shown,
            // so a file that never had one stays byte-identical.
            .chain(v.column_product.clone().map(|name| {
                (
                    "column-product".to_string(),
                    serde_json::Value::String(name),
                )
            }))
            .collect(),
        }
    }

    /// Apply this snapshot to a pane. The volume itself isn't restored — a pane with a site and no
    /// data fetches through the normal poll path, which is also what a fresh pane does.
    pub fn apply(&self, v: &mut MapView) {
        v.spatial_restore_raw = None;
        if let Some(value) = self.extra.get("spatial-links") {
            match serde_json::from_value::<crate::pane_links::SavedSpatialLinks>(value.clone()) {
                Ok(saved) if saved.valid() => v.spatial_links = saved.links,
                _ => {
                    v.spatial_links = Default::default();
                    v.spatial_restore_raw = Some(value.clone());
                }
            }
        }
        v.model_playback = Default::default();
        v.last_model_fields.clear();
        v.model_restore_raw = None;
        if let Some(value) = self.extra.get("models") {
            if let Ok(saved) =
                serde_json::from_value::<crate::model_pane::SavedModelContext>(value.clone())
            {
                if saved.valid() {
                    v.models = saved.controls;
                    v.model_group = saved.group;
                } else {
                    v.model_restore_raw = Some(value.clone());
                    v.model_group = None;
                }
            } else {
                v.model_restore_raw = Some(value.clone());
                v.model_group = None;
            }
        }
        v.model_link_snapshot = v.models.clone();
        // A missing or out-of-range group (a workspace from before groups, or a newer layout)
        // is the default group 1: the old single global link.
        v.time_group = self
            .extra
            .get("time-group")
            .and_then(serde_json::Value::as_u64)
            .and_then(|g| u8::try_from(g).ok())
            .filter(|g| (1..=crate::view::MAX_PANES as u8).contains(g))
            .unwrap_or(1);
        v.column_product = self
            .extra
            .get("column-product")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        v.site = self.site.clone();
        v.moment = self.moment;
        v.tilt = self.tilt;
        v.srv = self.srv;
        v.basemap = crate::tiles::BasemapStyle::from_slug(&self.basemap);
        v.camera = crate::render::mercator::Camera::at_lonlat(self.lon, self.lat, self.zoom);
        v.spatial_camera_snapshot = v.camera;
        v.spatial_site_snapshot = v.site.clone();
        v.flight = None;
        v.shown_camera = None;
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
        extra: Default::default(),
    }
}

/// The starters shipped in the first seeding. A settings file seeded before
/// `Settings::offered_starters` existed was offered exactly these, so only the ones after them are
/// new to it.
const FIRST_STARTERS: [&str; 8] = [
    "Chase",
    "National overview",
    "Analysis",
    "Tornado analysis",
    "Hail analysis",
    "Mesoscale analysis",
    "Radar + satellite",
    "Forecast comparison",
];

/// Add each starter this settings file has never been offered, once: a preset shipped later
/// (Tropical) reaches people seeded before it, and a starter someone deleted stays deleted.
/// `offered` records every starter name offered so far. Returns whether anything changed.
pub fn offer_new_starters(
    saved: &mut Vec<Workspace>,
    offered: &mut Vec<String>,
    seeded: bool,
) -> bool {
    let mut changed = false;
    if offered.is_empty() && seeded {
        *offered = FIRST_STARTERS.iter().map(|s| s.to_string()).collect();
        changed = true;
    }
    for start in starters() {
        if offered.contains(&start.name) {
            continue;
        }
        offered.push(start.name.clone());
        if !saved.iter().any(|w| w.name == start.name) {
            saved.push(start);
        }
        changed = true;
    }
    changed
}

/// What in `ws` this build cannot restore, said plainly: layers and fields it does not have, radar
/// sites it does not know, map styles it cannot draw, more panes than it shows. Applying skips
/// each of these (a file from a newer build still opens); this is so it does not do so silently.
/// A file cannot advertise shared model controls while storing conflicting owners.
pub(crate) fn conflicting_model_groups(ws: &Workspace) -> Vec<u8> {
    let mut owners = std::collections::HashMap::new();
    let mut conflicts = std::collections::BTreeSet::new();
    for pane in &ws.panes {
        let Some(saved) = pane
            .extra
            .get("models")
            .and_then(|value| {
                serde_json::from_value::<crate::model_pane::SavedModelContext>(value.clone()).ok()
            })
            .filter(|saved| saved.valid())
        else {
            continue;
        };
        let Some(group) = saved.group else {
            continue;
        };
        let identity = (
            saved.controls.model_sel.model,
            saved.controls.model_run,
            saved.controls.lead_min(),
        );
        if owners
            .insert(group, identity)
            .is_some_and(|previous| previous != identity)
        {
            conflicts.insert(group);
        }
    }
    conflicts.into_iter().collect()
}

pub(crate) fn conflicting_spatial_groups(
    ws: &Workspace,
) -> Vec<(crate::pane_links::Dimension, u8)> {
    use crate::pane_links::{Dimension, SavedSpatialLinks};
    let mut conflicts = Vec::new();
    for dimension in [Dimension::Camera, Dimension::Site] {
        let mut owners = std::collections::HashMap::new();
        for pane in &ws.panes {
            let Some(saved) = pane
                .extra
                .get("spatial-links")
                .and_then(|value| serde_json::from_value::<SavedSpatialLinks>(value.clone()).ok())
                .filter(|saved| saved.valid())
            else {
                continue;
            };
            let link = saved.links.get(dimension);
            if !link.enabled {
                continue;
            }
            let identity = match dimension {
                Dimension::Camera => serde_json::json!([pane.lon, pane.lat, pane.zoom]),
                Dimension::Site => serde_json::json!(pane.site),
                Dimension::Cursor => unreachable!(),
            };
            if owners
                .insert(link.group, identity.clone())
                .is_some_and(|previous| previous != identity)
                && !conflicts.contains(&(dimension, link.group))
            {
                conflicts.push((dimension, link.group));
            }
        }
    }
    conflicts
}

/// Old global flags become group 1. Typed memberships take precedence; conflicting typed
/// camera/site owners preserve their individual saved values with just that dimension disabled.
pub(crate) fn restore_spatial_memberships(ws: &Workspace, views: &mut [MapView], active: usize) {
    for (pane, view) in ws.panes.iter().zip(views.iter_mut()) {
        if !pane.extra.contains_key("spatial-links") {
            view.spatial_links = crate::pane_links::SpatialLinks::legacy(
                ws.link_cameras,
                ws.link_site,
                ws.link_cursor,
            );
        }
    }
    for (dimension, group) in conflicting_spatial_groups(ws) {
        for view in views.iter_mut() {
            let link = view.spatial_links.get_mut(dimension);
            if link.enabled && link.group == group {
                link.enabled = false;
            }
        }
    }
    let owner = views
        .get(active)
        .filter(|v| v.spatial_links.camera.enabled && v.spatial_links.camera.group == 1)
        .map(|_| active)
        .or_else(|| {
            views
                .iter()
                .position(|v| v.spatial_links.camera.enabled && v.spatial_links.camera.group == 1)
        });
    if let Some(owner) = owner {
        let camera = views[owner].camera;
        for (pane, view) in ws.panes.iter().zip(views.iter_mut()) {
            if !pane.extra.contains_key("spatial-links") && view.spatial_links.camera.enabled {
                view.camera = camera;
            }
        }
    }
    for view in views {
        view.spatial_camera_snapshot = view.camera;
        view.spatial_site_snapshot = view.site.clone();
    }
}

pub fn problems(ws: &Workspace) -> Vec<String> {
    let mut out = Vec::new();
    for (idx, pane) in ws.panes.iter().enumerate() {
        if pane.extra.get("spatial-links").is_some_and(|value| {
            !serde_json::from_value::<crate::pane_links::SavedSpatialLinks>(value.clone())
                .is_ok_and(|saved| saved.valid())
        }) {
            out.push(format!("pane {} has unsupported or invalid spatial links; camera, site and cursor links are disabled", idx + 1));
        }
        if !pane.model_context_valid() {
            out.push(format!(
                "pane {} has unsupported or invalid model controls; its model fields are disabled",
                idx + 1
            ));
        }
    }
    for group in conflicting_model_groups(ws) {
        out.push(format!(
            "model group {group} has conflicting controls; its panes restore independently"
        ));
    }
    for (dimension, group) in conflicting_spatial_groups(ws) {
        out.push(format!(
            "{} group {group} has conflicting saved views; that link restores independently",
            dimension.label()
        ));
    }
    let unknown_overlays: Vec<&str> = ws
        .overlays_on
        .iter()
        .filter(|s| crate::app::OverlayToggle::from_slug(s).is_none())
        .map(String::as_str)
        .collect();
    if !unknown_overlays.is_empty() {
        out.push(format!(
            "layers this version does not have: {}",
            unknown_overlays.join(", ")
        ));
    }
    let mut fields: Vec<&str> = ws
        .fields_on
        .iter()
        .chain(ws.panes.iter().flat_map(|p| p.fields_on.iter().flatten()))
        .filter(|s| crate::render::FieldLayer::from_slug(s).is_none())
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    fields.dedup();
    if !fields.is_empty() {
        out.push(format!(
            "fields this version does not have: {}",
            fields.join(", ")
        ));
    }
    let mut sites: Vec<&str> = ws
        .panes
        .iter()
        .filter_map(|p| p.site.as_deref())
        .filter(|s| wxdata::sites::site_by_id(s).is_none())
        .collect();
    sites.dedup();
    if !sites.is_empty() {
        out.push(format!("unknown radar sites: {}", sites.join(", ")));
    }
    let mut styles: Vec<&str> = ws
        .panes
        .iter()
        .map(|p| p.basemap.as_str())
        .filter(|b| crate::tiles::BasemapStyle::from_slug(b).slug() != *b)
        .collect();
    styles.dedup();
    if !styles.is_empty() {
        out.push(format!(
            "map styles this version does not have (shown dark): {}",
            styles.join(", ")
        ));
    }
    if ws.panes.len() > crate::view::MAX_PANES {
        out.push(format!(
            "{} panes; this version shows the first {}",
            ws.panes.len(),
            crate::view::MAX_PANES
        ));
    }
    out
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
                "TornadoId".into(),
            ],
            adopt_site: true,
            fields_on: Vec::new(),
            chrome: None,
            sound_center: false,
            gis: None,
            extra: Default::default(),
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
                extra: Default::default(),
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
            gis: None,
            extra: Default::default(),
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
            gis: None,
            extra: Default::default(),
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
                "TornadoId".into(),
            ],
            adopt_site: true,
            fields_on: Vec::new(),
            chrome: None,
            sound_center: false,
            gis: None,
            extra: Default::default(),
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
            gis: None,
            extra: Default::default(),
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
                extra: Default::default(),
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
            gis: None,
            extra: Default::default(),
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
            gis: None,
            extra: Default::default(),
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
                    extra: Default::default(),
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
                    extra: Default::default(),
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
            gis: None,
            extra: Default::default(),
        },
        // ROADMAP_2 §12.1's Tropical preset: a landfalling storm's radar beside the satellite
        // picture of the whole system, with the NHC track and cone, recon, surface obs and the
        // warnings. Reflectivity for the eyewall and bands, storm-relative velocity for the
        // embedded tornadoes landfalling bands spin up, and reflectivity over infrared satellite
        // for the structure radar cannot reach offshore. Each pane adopts the active radar.
        Workspace {
            name: "Tropical".into(),
            pane_layout: PaneLayout::Balanced,
            panes: vec![
                pane(Moment::Reflectivity, 0, false),
                pane(Moment::Velocity, 0, true),
                PaneSnap {
                    fields_on: Some(vec!["goes-ir".into()]),
                    ..pane(Moment::Reflectivity, 0, false)
                },
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
                "Tropical".into(),
                "Recon".into(),
                "Metar".into(),
                "Watches".into(),
            ],
            adopt_site: true,
            fields_on: vec!["goes-ir".into()],
            chrome: None,
            sound_center: false,
            gis: None,
            extra: Default::default(),
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
    fn a_column_product_and_its_layer_survive_the_round_trip_and_absence_stays_absent() {
        let cam = crate::render::mercator::Camera::at_lonlat(0.0, 0.0, 3.0);
        let mut v = MapView::new(None, cam);
        let bare = serde_json::to_value(PaneSnap::capture(&v)).unwrap();
        assert!(
            bare.get("column-product").is_none(),
            "nothing written when none is shown"
        );
        v.column_product = Some("ZDR above 0C".into());
        v.fields_on.insert(crate::render::FieldLayer::UserColumn);
        let json = serde_json::to_string(&PaneSnap::capture(&v)).unwrap();
        let snap: PaneSnap = serde_json::from_str(&json).unwrap();
        assert_eq!(
            snap.fields_on.as_deref(),
            Some(["user-column".to_string()].as_slice())
        );
        let mut fresh = MapView::new(None, cam);
        fresh.column_product = Some("leftover".into());
        snap.apply(&mut fresh);
        assert_eq!(fresh.column_product.as_deref(), Some("ZDR above 0C"));
        // A snapshot without one clears a leftover selection rather than keeping it.
        let plain: PaneSnap = serde_json::from_value(bare).unwrap();
        plain.apply(&mut fresh);
        assert_eq!(fresh.column_product, None);
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
                extra: Default::default(),
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
            gis: None,
            extra: Default::default(),
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
                    settings: settings_default(),
                    view3d: WindowChrome::at(true, Place::Float),
                    sources: WindowChrome::at(true, Place::Left),
                    log: WindowChrome::at(true, Place::Right),
                    dock_widths: [Some(320), None],
                    bottom_h: Some(260),
                    tab_order: vec!["Storms".into(), "Inspector".into()],
                    sounding: WindowChrome::at(true, Place::Float),
                    storms: WindowChrome::at(false, Place::Right),
                    cell: WindowChrome::at(true, Place::Left),
                    region: WindowChrome::at(true, Place::Float),
                    volume: WindowChrome::at(false, Place::Left),
                    gauges: WindowChrome::at(true, Place::Left),
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
    fn a_newer_files_unknown_fields_survive_a_round_trip_here() {
        let mut json: serde_json::Value = serde_json::to_value(&starters()[0]).unwrap();
        json["future_setting"] = serde_json::json!({ "on": true });
        json["panes"][0]["future_pane_thing"] = serde_json::json!(3);
        let ws: Workspace = serde_json::from_value(json).unwrap();
        let back = serde_json::to_value(&ws).unwrap();
        assert_eq!(back["future_setting"]["on"], true);
        assert_eq!(back["panes"][0]["future_pane_thing"], 3);
        // And a file with none writes none.
        let plain = serde_json::to_value(&starters()[0]).unwrap();
        assert!(plain.get("extra").is_none());
    }

    #[test]
    fn what_this_build_cannot_restore_is_named() {
        let mut ws = starters().remove(0);
        assert!(problems(&ws).is_empty(), "{:?}", problems(&ws));
        ws.overlays_on.push("HoloDeck".into());
        ws.fields_on.push("future-field".into());
        ws.panes[0].site = Some("KXYZ".into());
        ws.panes[0].basemap = "neon".into();
        let p = problems(&ws);
        assert_eq!(p.len(), 4, "{p:?}");
        assert!(p[0].contains("HoloDeck") && p[1].contains("future-field"));
        assert!(p[2].contains("KXYZ") && p[3].contains("neon"));
    }

    #[test]
    fn a_later_starter_reaches_an_old_seeding_once_and_a_deleted_one_stays_deleted() {
        // Seeded before `offered_starters` existed, then "Chase" deleted.
        let mut saved: Vec<Workspace> = starters()
            .into_iter()
            .filter(|w| FIRST_STARTERS.contains(&w.name.as_str()) && w.name != "Chase")
            .collect();
        let mut offered = Vec::new();
        assert!(offer_new_starters(&mut saved, &mut offered, true));
        assert!(
            saved.iter().any(|w| w.name == "Tropical"),
            "the new one arrives"
        );
        assert!(
            !saved.iter().any(|w| w.name == "Chase"),
            "the deleted one stays gone"
        );
        // Deleting the new one sticks too.
        saved.retain(|w| w.name != "Tropical");
        assert!(!offer_new_starters(&mut saved, &mut offered, true));
        assert!(!saved.iter().any(|w| w.name == "Tropical"));
        // A first seeding offers everything and adds nothing twice.
        let mut fresh = starters();
        let mut offered = Vec::new();
        offer_new_starters(&mut fresh, &mut offered, false);
        assert_eq!(fresh.len(), starters().len());
        assert_eq!(offered.len(), starters().len());
    }

    #[test]
    fn the_first_seeding_list_names_real_starters() {
        let names: Vec<String> = starters().into_iter().map(|w| w.name).collect();
        for n in FIRST_STARTERS {
            assert!(names.iter().any(|m| m == n), "{n}");
        }
    }

    #[test]
    fn every_starter_names_things_this_build_has() {
        for ws in starters() {
            assert!(!ws.panes.is_empty(), "{} has no panes", ws.name);
            assert!(ws.active < ws.panes.len());
            for t in &ws.overlays_on {
                assert!(
                    crate::app::OverlayToggle::from_slug(t).is_some(),
                    "{}: unknown overlay {t}",
                    ws.name
                );
            }
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

#[cfg(test)]
mod model_context_tests {
    use super::*;
    use crate::model_browser::{BModel, Product, Selection};
    use crate::render::mercator::Camera;
    use chrono::TimeZone;

    fn view() -> MapView {
        MapView::new(None, Camera::at_lonlat(-97.0, 35.0, 8.0))
    }
    #[test]
    fn model_workspace_restores_independent_model_run_hour_and_group() {
        let mut original = view();
        original.model_group = None;
        original.models.model_sel = Selection {
            model: BModel::Ecmwf,
            product: Product::Mslp,
        };
        original.models.apply_engine(original.models.model_sel);
        original.models.model_run =
            Some(chrono::Utc.with_ymd_and_hms(2026, 10, 4, 12, 0, 0).unwrap());
        original.models.global_fcst_hour = 9;
        let snap = PaneSnap::capture(&original);
        let decoded: PaneSnap =
            serde_json::from_str(&serde_json::to_string(&snap).unwrap()).unwrap();
        let mut restored = view();
        decoded.apply(&mut restored);
        assert_eq!(restored.models, original.models);
        assert_eq!(restored.model_group, None);
        assert_eq!(restored.model_link_snapshot, restored.models);
    }
    #[test]
    fn model_workspace_legacy_snapshot_keeps_seeded_controls() {
        let mut original = view();
        original.models.hrrr_fcst_hour = 9;
        let mut snap = PaneSnap::capture(&original);
        snap.extra.remove("models");
        assert!(snap.model_context_valid());
        let mut restored = view();
        restored.models.hrrr_fcst_hour = 12;
        snap.apply(&mut restored);
        assert_eq!(restored.models.hrrr_fcst_hour, 12);
        assert_eq!(restored.model_group, Some(1));
    }
    #[test]
    fn model_workspace_unsupported_context_is_disclosed_and_preserved_for_round_trip() {
        let mut snap = PaneSnap::capture(&view());
        let unknown =
            serde_json::json!({"schema": 99, "group": 4, "controls": {"model": "future"}});
        snap.extra.insert("models".into(), unknown.clone());
        assert!(!snap.model_context_valid());
        let mut restored = view();
        snap.apply(&mut restored);
        assert_eq!(restored.model_restore_raw, Some(unknown.clone()));
        assert_eq!(restored.model_group, None);
        assert_eq!(PaneSnap::capture(&restored).extra["models"], unknown);
        let mut workspace = starters().remove(0);
        workspace.panes = vec![snap];
        assert!(problems(&workspace)
            .iter()
            .any(|p| p.contains("model fields are disabled")));
    }
    #[test]
    fn model_workspace_conflicting_groups_are_disclosed_without_choosing_a_silent_winner() {
        let first = view();
        let mut second = view();
        second.models.hrrr_fcst_hour = 6;
        let mut workspace = starters().remove(0);
        workspace.panes = vec![PaneSnap::capture(&first), PaneSnap::capture(&second)];
        assert_eq!(conflicting_model_groups(&workspace), [1]);
        assert!(problems(&workspace)
            .iter()
            .any(|p| p.contains("restore independently")));
        second.model_group = None;
        workspace.panes[1] = PaneSnap::capture(&second);
        assert!(conflicting_model_groups(&workspace).is_empty());
    }
}
