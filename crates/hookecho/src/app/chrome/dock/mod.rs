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

mod app_bar;
mod inspector;
mod layers;
mod rail;
mod timeline;

/// Width of the Layers panel.
const LEFT_WIDTH: f32 = 284.0;

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
}

/// Everything the dock remembers between frames.
pub(crate) struct DockState {
    pub tab: DockTab,
    pub filter: LayerFilter,
    pub query: String,
    pub left_open: bool,
    pub inspector_open: bool,
    pub timeline_open: bool,
    /// The inspector's model-forecast block (shown only while a model layer is on the map).
    pub model_open: bool,
    /// A pinned reading: kept on the card after the pointer leaves the map.
    pub pinned: Option<Probe>,
    /// The last reading taken under the pointer, which the card keeps showing (and a pin keeps)
    /// once the pointer has left the map for the card.
    pub last: Option<Probe>,
    /// The "Jump to…" field's text.
    pub jump: String,
}

impl Default for DockState {
    fn default() -> Self {
        Self {
            tab: DockTab::Radar,
            filter: LayerFilter::All,
            query: String::new(),
            left_open: true,
            inspector_open: true,
            timeline_open: true,
            model_open: true,
            pinned: None,
            last: None,
            jump: String::new(),
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
    for cat in crate::ui::layers_panel::CATEGORIES {
        if !every_category && !tab.categories().contains(&cat) {
            continue;
        }
        let mut rows = Vec::new();
        let mut on = 0;
        for (i, e) in entries.iter().enumerate() {
            if e.category != cat {
                continue;
            }
            let is_on = e.on == Some(true);
            let keep = match filter {
                LayerFilter::All => true,
                LayerFilter::Active => is_on,
                LayerFilter::Favorites => {
                    favorite_slug(e).is_some_and(|s| favorites.iter().any(|f| f == s))
                }
            };
            if !keep {
                continue;
            }
            if !query.is_empty()
                && crate::ui::layers_panel::fuzzy(query, &e.label).is_none()
                && crate::ui::layers_panel::fuzzy(query, e.desc).is_none()
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

    /// Draw the whole dock. Called once per frame before the map's own rect is read, so the map
    /// gets whatever the panels leave. Order is layout: the two top bars, the timeline across the
    /// full width at the bottom, then the Layers panel and the rail down the left.
    pub(crate) fn dock_layout(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        self.dock_app_bar(root, ctx);
        self.dock_toolbar(root, ctx);
        self.dock_timeline(root);
        self.dock_left(root, ctx);
        self.dock_rail(root, ctx);
    }

    /// Over the map: the Inspector card.
    pub(crate) fn dock_map_overlay(&mut self, ctx: &egui::Context) {
        self.dock_inspector(ctx);
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
        ("Model:", sel.model.label().to_string()),
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

/// Which tilt (an index into the volume's elevations) the live stream is sweeping right now.
/// `None` when nothing is streaming, the indicator is turned off, or no chunk has arrived.
pub(crate) fn sweeping_tilt(
    progress: Option<wxdata::live::ScanProgress>,
    streaming: bool,
    indicator_on: bool,
) -> Option<usize> {
    if !streaming || !indicator_on {
        return None;
    }
    // Sweeps count from 1; zero means the stream has not said, and must not wrap to a real index.
    progress.and_then(|p| p.elevation_number.checked_sub(1))
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
        let g = group_entries(&sample(), DockTab::Radar, LayerFilter::All, "hrrr", &[]);
        assert_eq!(cats(&g), ["Models"]);
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

    #[test]
    fn the_sweeping_tilt_is_the_streams_sweep_number_as_an_index() {
        assert_eq!(sweeping_tilt(Some(progress(1)), true, true), Some(0));
        assert_eq!(sweeping_tilt(Some(progress(5)), true, true), Some(4));
    }

    #[test]
    fn nothing_is_marked_when_not_streaming_switched_off_or_silent() {
        assert_eq!(sweeping_tilt(Some(progress(3)), false, true), None);
        assert_eq!(sweeping_tilt(Some(progress(3)), true, false), None);
        assert_eq!(sweeping_tilt(None, true, true), None);
        // Sweep zero is "not said yet", never the last tilt of the volume.
        assert_eq!(sweeping_tilt(Some(progress(0)), true, true), None);
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
