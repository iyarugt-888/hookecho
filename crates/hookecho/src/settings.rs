//! Persisted user settings (JSON at the platform config dir).
//!
//! Mirrors Supercell WX's settings tabs; only the General tab is wired in U1, the rest
//! land in later milestones. `#[serde(default)]` makes old config files forward-compatible
//! — new fields fill from `Default`, unknown fields are ignored.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The colour scheme. There is one: the Dear ImGui look the workstation dock was designed in
/// (`theme.rs`). The app's other schemes were retired so every surface draws the same way; their
/// names still load, onto this one, so no settings file breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    /// Dear ImGui's own default dark style, reproduced as closely as this app's single-accent
    /// palette abstraction can: near-black window fill, the exact "ImGui blue" accent
    /// (`#4296FA`, `ImGuiCol_CheckMark`/`ImGuiCol_Header` in `StyleColorsDark()`), and the muted
    /// navy `FrameBg` blend for idle input fields that gives ImGui's widgets their identifiable
    /// look.
    #[default]
    #[serde(
        alias = "Dark",
        alias = "Light",
        alias = "System",
        alias = "Classic",
        alias = "Synthwave",
        alias = "Aurora",
        alias = "HighContrast",
        alias = "Oled",
        alias = "Magma",
        alias = "Redline",
        alias = "AcidStorm",
        alias = "Glacier",
        alias = "Ultraviolet",
        alias = "Bubblegum",
        alias = "Voltage",
        alias = "Riptide"
    )]
    DearImGui,
}

impl Theme {
    pub fn label(self) -> &'static str {
        match self {
            Theme::DearImGui => "Dear ImGui",
        }
    }
}

/// How an in-progress Level II sweep displays radial rows from the previous pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LiveSweepMode {
    /// Keep earlier-pass coverage dimmed until the antenna replaces it.
    #[default]
    ContinuousComposite,
    /// Show only rows proven to belong to the newest pass.
    StrictCurrentSweep,
}

/// Alert sound choice. Built-ins are synthesized in `audio.rs` (no asset files); `Custom` plays a
/// user file (wav/mp3/ogg/flac). Serializes as `"Chime"` or `{"Custom":"/path/f.wav"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum AlertSound {
    #[default]
    Chime,
    Ding,
    Siren,
    Alarm,
    Pulse,
    /// The EAS/NWS Attention Signal: 853 Hz and 960 Hz together, the sound that precedes an
    /// emergency broadcast. Synthesized from the two published frequencies like every other
    /// built-in — no recording is bundled.
    Eas,
    Custom(String),
}

impl AlertSound {
    /// The synthesized built-ins, for sound-picker combos.
    pub const BUILTINS: [AlertSound; 6] = [
        AlertSound::Chime,
        AlertSound::Ding,
        AlertSound::Siren,
        AlertSound::Alarm,
        AlertSound::Pulse,
        AlertSound::Eas,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            AlertSound::Chime => "Chime",
            AlertSound::Ding => "Ding",
            AlertSound::Siren => "Siren",
            AlertSound::Alarm => "Alarm",
            AlertSound::Pulse => "Pulse",
            AlertSound::Eas => "Emergency (EAS tone)",
            AlertSound::Custom(_) => "Custom…",
        }
    }
}

fn default_custom_tile_max_z() -> u8 {
    19
}

fn default_radar_stale_minutes() -> u16 {
    15
}

fn default_time_mismatch_minutes() -> u16 {
    10
}

fn default_volume() -> f32 {
    0.2
}

fn default_live_loop_frames() -> usize {
    10
}

/// Where the desktop window was last time. Size only, not position: a saved position is wrong the
/// moment a monitor is unplugged or a laptop is docked, and a window that opens off-screen is a
/// worse bug than one that opens in the wrong place.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WindowGeom {
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub maximized: bool,
}

/// Presentation for the one user-imported GIS layer.
///
/// Keep this independent of the imported file: replacing a county boundary with an updated
/// export should not unexpectedly reset the way the analyst chose to distinguish it from
/// official warning/outlook geometry.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImportedGisStyle {
    /// One RGB color drives polygons, lines, and points so the imported file reads as one layer.
    pub color: [u8; 3],
    /// Screen-space outline width shared by polygon boundaries, lines, and point symbols.
    pub stroke_width: f32,
    /// Multiplies the established polygon-fill/stroke alpha instead of replacing it; 100% is
    /// therefore byte-for-byte compatible with the appearance shipped before this control.
    pub opacity: f32,
    /// The map zoom below which the layer is hidden (and not clickable), so a dense file of
    /// parcels or sites does not smother a national view. 0 shows it at every zoom.
    pub min_zoom: f32,
    /// The map zoom above which the layer is hidden, so a state outline gives way to the
    /// county layer under it. 0 sets no maximum.
    pub max_zoom: f32,
    /// A polygon fill colour of its own, `None` filling in [`Self::color`] as before. With a
    /// colour-by attribute, a separate fill is what the attribute colours, and the outline keeps
    /// [`Self::color`].
    pub fill_color: Option<[u8; 3]>,
    /// Multiplies the fill's alpha on top of [`Self::opacity`]; 0 draws outlines only.
    pub fill_opacity: f32,
    /// How lines and polygon outlines are stroked.
    pub dash: LineDash,
    /// The symbol drawn at each point.
    pub symbol: PointSymbol,
    /// A point symbol's radius in screen pixels; 0 sizes it from the stroke width, as before.
    pub point_size: f32,
}

/// How an imported layer's lines and polygon outlines are stroked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum LineDash {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

impl LineDash {
    pub const ALL: [LineDash; 3] = [LineDash::Solid, LineDash::Dashed, LineDash::Dotted];

    pub fn label(self) -> &'static str {
        match self {
            LineDash::Solid => "Solid",
            LineDash::Dashed => "Dashed",
            LineDash::Dotted => "Dotted",
        }
    }

    /// On and off lengths in screen pixels for a stroke `width` wide; `None` when solid. A dot's
    /// "on" is a sliver (half a pixel or less) drawn with round caps, so each dot is a disc
    /// about as wide as the stroke.
    pub fn pattern(self, width: f32) -> Option<(f32, f32)> {
        match self {
            LineDash::Solid => None,
            LineDash::Dashed => Some((6.0 + 3.0 * width, 4.0 + 2.0 * width)),
            LineDash::Dotted => Some((0.25, 2.0 + 2.0 * width)),
        }
    }
}

/// The restorable part of the output window: whether it is open, its size, fullscreen and strap.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputPrefs {
    pub open: bool,
    /// "1080", "1440", "2160" or "free"; anything else reads as 1080.
    pub size: String,
    pub fullscreen: bool,
    pub strap: String,
}

/// The symbol an imported layer draws at each point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PointSymbol {
    #[default]
    Circle,
    Square,
    Triangle,
    Diamond,
    Cross,
}

impl PointSymbol {
    pub const ALL: [PointSymbol; 5] = [
        PointSymbol::Circle,
        PointSymbol::Square,
        PointSymbol::Triangle,
        PointSymbol::Diamond,
        PointSymbol::Cross,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PointSymbol::Circle => "Circle",
            PointSymbol::Square => "Square",
            PointSymbol::Triangle => "Triangle",
            PointSymbol::Diamond => "Diamond",
            PointSymbol::Cross => "Cross",
        }
    }
}

impl ImportedGisStyle {
    pub const DEFAULT_COLOR: [u8; 3] = [80, 140, 220];
    pub const DEFAULT_STROKE_WIDTH: f32 = 1.6;
    const FILL_ALPHA: u8 = 60;
    const STROKE_ALPHA: u8 = 220;

    fn alpha(self, base: u8) -> u8 {
        (f32::from(base) * self.opacity.clamp(0.0, 1.0)).round() as u8
    }

    pub fn fill_rgba(self) -> [u8; 4] {
        let [r, g, b] = self.fill_color.unwrap_or(self.color);
        let a = f32::from(self.alpha(Self::FILL_ALPHA)) * self.fill_opacity.clamp(0.0, 1.0);
        [r, g, b, a.round() as u8]
    }

    pub fn stroke_rgba(self) -> [u8; 4] {
        [
            self.color[0],
            self.color[1],
            self.color[2],
            self.alpha(Self::STROKE_ALPHA),
        ]
    }

    /// Clamp persisted or manually-edited settings at the render boundary.
    pub fn rendered_stroke_width(self) -> f32 {
        self.stroke_width.clamp(0.5, 8.0)
    }

    /// Whether the layer shows at a map zoom.
    pub fn visible_at(self, zoom: f64) -> bool {
        zoom >= f64::from(self.min_zoom)
            && (self.max_zoom <= 0.0 || zoom <= f64::from(self.max_zoom))
    }

    /// A point symbol's radius in screen pixels: its own size, or from the stroke width (the
    /// default 1.6 px stroke gives the 3.5 px dot shipped before sizes could be chosen).
    pub fn point_radius(self) -> f32 {
        if self.point_size > 0.0 {
            self.point_size.clamp(1.0, 24.0)
        } else {
            2.5 + self.rendered_stroke_width() * 0.625
        }
    }

    /// The style with `c` as the colour a colour-by attribute gives a feature: the fill when the
    /// layer has a fill of its own, every part otherwise.
    pub fn colored(mut self, c: [u8; 3]) -> Self {
        match self.fill_color {
            Some(_) => self.fill_color = Some(c),
            None => self.color = c,
        }
        self
    }
}

impl Default for ImportedGisStyle {
    fn default() -> Self {
        Self {
            color: Self::DEFAULT_COLOR,
            stroke_width: Self::DEFAULT_STROKE_WIDTH,
            opacity: 1.0,
            min_zoom: 0.0,
            max_zoom: 0.0,
            fill_color: None,
            fill_opacity: 1.0,
            dash: LineDash::Solid,
            symbol: PointSymbol::Circle,
            point_size: 0.0,
        }
    }
}

/// One imported GIS layer (ROADMAP_PARITY M4.1): where it came from and how it is drawn. Layers
/// are independent: importing another file adds a layer, and each keeps its own style, labels,
/// colouring, time mapping, visibility, order and group. `settings.gis_layers` order is the paint
/// order, first underneath (within the layers drawn above or below the official products).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GisLayerConfig {
    /// Stable for the layer's life and never reused, so nothing that names a layer can bind to
    /// a different one after a removal.
    pub id: u64,
    /// A path on native/Android, or the name of a [`Settings::web_files`] entry in a browser.
    pub source: String,
    /// What the layer is called in lists; the file name when imported.
    pub name: String,
    pub visible: bool,
    pub style: ImportedGisStyle,
    /// The attribute whose value labels each feature, by name so it survives a re-import.
    pub label: Option<String>,
    /// A label built from several attributes, e.g. `{NAME} ({POP})`; when set it is used
    /// instead of [`Self::label`].
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label_template: String,
    /// The attribute that colours the features, or `None` for the layer's one colour.
    pub color_by: Option<String>,
    /// The attribute whose values each get their own point symbol (1008.md D2), or `None` for
    /// the layer's one symbol. Colour and symbol can follow different attributes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_by: Option<String>,
    /// The attributes holding each feature's valid start and end.
    pub time_start: Option<String>,
    pub time_end: Option<String>,
    /// Paint under the official products (outlooks, watches, warnings) instead of over them.
    pub below: bool,
    /// The named group it belongs to, if any.
    pub group: Option<String>,
    /// Its points and areas are impact targets: a storm's arrival and closest approach at each
    /// are listed with the storm (ROADMAP_PARITY M2.3), named by its label attribute.
    pub targets: bool,
    /// An attribute filter (`crate::gis_filter`): only features it is true for are drawn,
    /// clickable, labelled, exported and targeted. Empty shows every feature.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub filter: String,
    /// Its labels are placed before other imported layers' labels, whatever the paint order
    /// (1008.md D2): a hospital name wins its spot over a road name. Storm, town and station
    /// labels still come first.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub labels_first: bool,
}

impl Default for GisLayerConfig {
    fn default() -> Self {
        Self {
            id: 0,
            source: String::new(),
            name: String::new(),
            visible: true,
            style: ImportedGisStyle::default(),
            label: None,
            label_template: String::new(),
            color_by: None,
            symbol_by: None,
            time_start: None,
            time_end: None,
            below: false,
            group: None,
            targets: false,
            filter: String::new(),
            labels_first: false,
        }
    }
}

/// A named group of GIS layers. Hiding a group hides its layers without touching their own
/// visibility, so showing it again brings back exactly the ones that were on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GisGroup {
    pub name: String,
    pub visible: bool,
}

impl Default for GisGroup {
    fn default() -> Self {
        Self {
            name: String::new(),
            visible: true,
        }
    }
}

/// The imported GIS layers as a workspace or scene saw them (ROADMAP_PARITY M4.4): the master
/// switch, every layer's state by its stable ID, and the group switches.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GisSnapshot {
    pub shown: bool,
    pub layers: Vec<GisLayerConfig>,
    pub groups: Vec<GisGroup>,
}

/// A saved 3D map look for one 3D product: its floor, ceiling and opacity curve, in that
/// product's own units (ROADMAP_NEW H2).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Volume3dPreset {
    pub name: String,
    /// The 3D product it is for, by its label ("Smooth reflectivity").
    pub representation: String,
    pub floor: f32,
    pub ceiling: Option<f32>,
    /// Four `[value, opacity]` points: the curve itself when it has four stops, otherwise a
    /// four-point sampling of it, which is what a build that predates `stops` reads.
    pub curve: Option<[[f32; 2]; 4]>,
    /// The curve's stops when it does not have exactly four (M3.5 increment 2); preferred over
    /// `curve` by builds that read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stops: Option<Vec<[f32; 2]>>,
    /// The rendering the curve was drawn for (M3.5): with translucent rendering its opacities
    /// are per kilometre of path. Absent in presets saved before translucent rendering existed,
    /// which were all drawn for MIP and load as MIP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render: Option<crate::render3d::VolumeRender>,
    /// Colour stops as `[value, r, g, b]` (M3.5, 1008.md C2), when the preset replaced the
    /// palette's colours; absent in presets saved before colour stops, which keep the palette.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<Vec<[f32; 4]>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Default/home radar site (ICAO id).
    pub default_site: String,
    /// Seconds between live-update polls for the newest volume.
    pub poll_interval_secs: u64,
    pub theme: Theme,
    /// Desktop/web chrome: the WSV3 ribbon (default) or the original map-first minimal chrome.
    #[serde(default)]
    pub layout: Layout,
    /// The workstation layouts' last window arrangement, per layout, so a docked Inspector or a
    /// floating Layers window is where it was left after a restart.
    #[serde(default)]
    pub workstation: std::collections::BTreeMap<Layout, crate::workspace::WorkstationChrome>,
    /// Whether the one-time move of a tablet from the ribbon to the docked layout has happened;
    /// see [`Settings::adopt_tablet_default`].
    #[serde(default)]
    pub tablet_layout_adopted: bool,
    /// Whether the one-time move from the ribbon to the dock, when the dock became the default
    /// layout, has happened; see [`Settings::adopt_dock_default`].
    #[serde(default)]
    pub dock_adopted: bool,
    /// Whether the one-time move of a phone from the old default design to Station has happened;
    /// see [`Settings::adopt_station_default`].
    #[serde(default)]
    pub station_adopted: bool,
    /// Serve the running app's state on `127.0.0.1:local_api_port` (ROADMAP_NEW M4; see
    /// `crate::local_api`). Off unless turned on: it answers anything on this machine.
    #[serde(default)]
    pub local_api: bool,
    #[serde(default = "default_local_api_port")]
    pub local_api_port: u16,
    /// The touch chrome's look on a phone; see [`PhoneDesign`].
    #[serde(default)]
    pub phone_design: PhoneDesign,
    /// User accent override as RGB. `None` keeps the theme's own accent.
    pub accent: Option<[u8; 3]>,
    /// Hold every animation at its endpoint. The app also sets this for itself when frames get
    /// slow; this flag is only the user's half of that.
    #[serde(default)]
    pub reduce_motion: bool,
    /// In a pitched 3D map, stop drawing markers, reports and labels that are far beyond the
    /// centre: near the horizon they all pile onto a sliver and float in the sky.
    #[serde(default = "default_true")]
    pub hide_far_3d: bool,
    /// How far out that is, as a multiple of the camera's distance to the map centre.
    #[serde(default = "default_far_3d")]
    pub far_3d_factor: f32,
    /// Starred radar sites shown in the toolbox presets dropdown.
    pub presets: Vec<String>,
    /// Per-moment color-table override: moment short name (`REF`, `VEL`, …) -> `.pal` path.
    /// A missing key uses the built-in default table.
    pub palettes: BTreeMap<String, String>,
    /// File name -> file content, for platforms with nowhere to put a file. A browser hands an
    /// imported `.pal` over as bytes and there is no path that would survive the next reload, so
    /// the content rides along in the settings — which already persist, and already export with
    /// the settings bundle. Empty everywhere else.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub web_files: BTreeMap<String, String>,
    /// The imported GIS layers (ROADMAP_PARITY M4.1), in paint order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gis_layers: Vec<GisLayerConfig>,
    /// Named groups the layers can belong to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gis_groups: Vec<GisGroup>,
    /// The next layer ID to hand out; IDs are never reused.
    #[serde(default)]
    pub gis_next_id: u64,
    /// Before M4.1 there was one imported layer, remembered in these fields. They are read once,
    /// by [`Self::migrate_imported_gis`], into a [`GisLayerConfig`] and never written again.
    #[serde(default, skip_serializing)]
    pub imported_gis: Option<String>,
    #[serde(default, skip_serializing)]
    pub imported_gis_style: ImportedGisStyle,
    #[serde(default, skip_serializing)]
    pub imported_gis_label: Option<String>,
    #[serde(default, skip_serializing)]
    pub imported_gis_color_by: Option<String>,
    #[serde(default, skip_serializing)]
    pub imported_gis_time_start: Option<String>,
    #[serde(default, skip_serializing)]
    pub imported_gis_time_end: Option<String>,
    #[serde(default, skip_serializing)]
    pub imported_gis_below: bool,
    /// Velocity/spectrum-width display unit (internal data stays m/s).
    pub velocity_unit: VelocityUnit,
    /// Temperature display unit for the surface station plots (internal data stays Celsius).
    #[serde(default)]
    pub temp_unit: TempUnit,
    /// Whether radar timestamps read in the site's local time or in UTC.
    #[serde(default)]
    pub time_display: TimeDisplay,
    /// Maximum allowed difference between a displayed radar scan and another layer's valid time.
    #[serde(default = "default_time_mismatch_minutes")]
    pub time_mismatch_minutes: u16,
    /// How old the newest radar scan can be before the radar reads Stale (`radar_fresh_secs`).
    #[serde(default = "default_radar_stale_minutes")]
    pub radar_stale_minutes: u16,
    /// Check the CI build at launch and offer a newer one (`self_update`).
    #[serde(default = "default_true")]
    pub check_builds: bool,
    /// UI text/widget zoom factor (egui `zoom_factor`); also captures Ctrl+= / Ctrl+- / Ctrl+0.
    pub ui_scale: f32,
    /// User-added GRLevelX placefile overlays.
    pub placefiles: Vec<PlacefileConfig>,
    /// User-placed location markers.
    pub markers: Vec<Marker>,
    /// User-defined radar products (Phase C1): saved GR2Analyst-style formulas, evaluated live at
    /// whatever gate the inspector is showing. See `wxdata::udp`.
    #[serde(default)]
    pub udp_products: Vec<wxdata::udp::ProductDef>,
    /// Thresholds the map's GeoJSON export outlines the displayed sweep at, by moment code
    /// ("REF", "CC"...), in the moment's own units (1008.md D3). A moment not listed uses its
    /// defaults (`radar_outlines::default_thresholds`); an empty list exports none for it.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub outline_thresholds: std::collections::BTreeMap<String, Vec<f32>>,
    /// Recolour reflectivity by the MRMS surface precipitation type: blue where it is falling
    /// as snow, pink where it is freezing rain or sleet.
    #[serde(default)]
    pub precip_tint: bool,
    /// Unfold aliased Doppler velocity (region-based dealiasing) when displaying VEL.
    #[serde(default)]
    pub dealias_velocity: bool,
    /// Dim the map outside an alert's polygon while its card is open.
    #[serde(default = "default_true")]
    pub alert_spotlight: bool,
    /// Y'all mode: the Y'all-O-Meter card and Y'all Tracks for your spot (`crate::yall`).
    #[serde(default)]
    pub yall_mode: bool,
    /// Show every visible layer's reading under the pointer (`app::layer_probe`).
    #[serde(default)]
    pub layer_probe: bool,
    /// Draw the zoomed-out map as a globe (`render::mercator::set_globe`).
    #[serde(default)]
    pub globe: bool,
    /// A distance ruler in the corner of each flat map pane (`app::scale_bar`).
    #[serde(default = "default_true")]
    pub scale_bar: bool,
    /// Saved 3D map looks: a floor, ceiling and opacity curve per 3D product (ROADMAP_NEW H2).
    #[serde(default)]
    pub volume3d_presets: Vec<Volume3dPreset>,
    /// ROADMAP_NEW B6: base URL of a self-hosted `radar-ingest` relay (e.g.
    /// `https://relay.example.com`), the second independently-acquired live Level II path. Empty
    /// (default) means no relay is configured — live radar then runs Unidata-only with a NOAA
    /// TGFTP degraded fallback, same as before B6. Never a hosted HookEcho service: you point this
    /// at infrastructure you or someone you trust runs.
    #[serde(default)]
    pub radar_relay_url: String,
    /// ROADMAP_NEW B6.9's manual failover override, for diagnostics. `Auto` in normal operation.
    #[serde(default)]
    pub radar_provider_override: RadarProviderOverride,
    /// Mapbox access token (enables the Mapbox raster basemap styles). Held locally only.
    #[serde(default)]
    pub mapbox_key: String,
    /// MapTiler API key (enables the MapTiler raster basemap styles). Held locally only.
    #[serde(default)]
    pub maptiler_key: String,
    /// `{z}/{x}/{y}` tile URL template for the "Custom (XYZ URL)" basemap. Validated at the
    /// settings boundary — see [`crate::tiles::valid_xyz_template`] — and desktop/Android only.
    #[serde(default)]
    pub custom_tile_url: String,
    /// Max zoom the custom tile source serves. Deeper views stretch the deepest tile that loaded.
    #[serde(default = "default_custom_tile_max_z")]
    pub custom_tile_max_z: u8,
    /// Attribution line to paint for the custom tile source.
    #[serde(default)]
    pub custom_tile_attribution: String,
    /// WeatherFlow Tempest personal access token — adds your Tempest stations to the live station
    /// cards. Held locally only, same as the basemap keys.
    #[serde(default)]
    pub tempest_token: String,
    /// Weather Underground API key — adds nearby PWS stations to the live station cards.
    #[serde(default)]
    pub wu_key: String,
    /// Synoptic Data token — adds the mesonets (state, university, DOT) to the station cards.
    /// Held locally only, and never sent through the web build's proxy.
    #[serde(default)]
    pub synoptic_token: String,
    /// AirNow API key — turns on the AQI station layer. Held locally, same as the other keys.
    #[serde(default)]
    pub airnow_key: String,
    /// Per-field-layer opacity 0..1 (Layer Manager sliders). A missing entry means fully opaque.
    #[serde(default)]
    pub field_opacity: std::collections::HashMap<crate::render::FieldLayer, f32>,
    /// The user's paint order for field layers, bottom to top, applied within each band by
    /// `FieldLayer::paint_order`. Empty is the built-in order.
    #[serde(default)]
    pub field_order: Vec<crate::render::FieldLayer>,
    /// The model comparison last chosen (what is differenced, and how it is drawn), so a restart
    /// keeps it. `None` is the default comparison.
    #[serde(default)]
    pub compare_view: Option<(crate::fielddiff::DiffField, crate::fielddiff::DiffMode)>,
    /// Windy API key — adds the Windy webcam network to the keyless FAA cameras, which is what
    /// gives the layer any coverage outside the United States. Held locally, same as the rest.
    #[serde(default)]
    pub windy_key: String,
    /// A ground field mill publishing JSON (`{"time":…, "kv_per_m":…}`, or an array of those).
    /// When set, the station cards chart real kV/m instead of NOAA's ionospheric model.
    #[serde(default)]
    pub field_mill_url: String,
    /// Saved startup view (radar site + camera). `None` = open on `default_site`.
    #[serde(default)]
    pub start_view: Option<StartView>,
    /// Where the app was looking when it last closed, written on exit only while no explicit
    /// `start_view` is set. Kept apart from `start_view` so "save this view" stays a deliberate
    /// choice you can clear, rather than something the first quit freezes forever.
    #[serde(default)]
    pub last_view: Option<StartView>,
    /// Google OAuth client id + secret for settings sync (see `docs/sync.md`). You create the
    /// client; there is no shipped default, so an open-source binary carries nobody's quota.
    #[serde(default)]
    pub sync_client_id: String,
    #[serde(default)]
    pub sync_client_secret: String,
    /// Sync settings through your Google Drive app folder once signed in.
    #[serde(default)]
    pub sync_enabled: bool,
    /// Share your GPS position with other HookEcho instances (LAN broadcast, and the relay below
    /// when one is set). Off by default: your live position is not shared without asking.
    #[serde(default)]
    pub share_position: bool,
    /// The name other instances label your dot with. Empty = "me".
    #[serde(default)]
    pub share_name: String,
    /// Optional HTTP endpoint that relays positions when the devices aren't on one network
    /// (`POST` a peer, `GET` the list). Empty = LAN only. You host it; it sees your position.
    #[serde(default)]
    pub share_relay: String,
    /// A live-stream URL broadcast alongside your shared position, so chase partners can watch
    /// your feed from your dot. Empty = share position only.
    #[serde(default)]
    pub share_video_url: String,
    /// Averaging window for the MRMS cloud-to-ground strike-density layer, in minutes
    /// (1/5/15/30). Short windows show where lightning is right now; long ones show the storm's
    /// track.
    #[serde(default = "default_lightning_minutes")]
    pub lightning_minutes: u16,
    /// Also poll GOES-West (GOES-18) for GLM lightning, not only GOES-East. Costs one extra S3
    /// listing per 20-second cycle and covers the Pacific and the west coast.
    #[serde(default)]
    pub glm_goes_west: bool,
    /// Read the IR/visible/water-vapor satellite layers from GOES-West instead of GOES-East.
    /// Unlike GLM lightning (which polls both and combines the flashes) this is exclusive, not
    /// additive — the two satellites' CONUS-sector scans overlap, and showing both images at
    /// once would double-paint that overlap rather than usefully extend coverage. Pick West for
    /// the Pacific and the western half of the country, East for everywhere else.
    #[serde(default)]
    pub goes_satellite_west: bool,
    /// Which GOES RGB composite the RGB layer shows, by recipe slug (`wxdata::goes_rgb`).
    #[serde(default = "default_goes_rgb_recipe")]
    pub goes_rgb_recipe: String,
    /// Which ABI scan the GOES band and RGB layers read: CONUS every 5 minutes, or a mesoscale
    /// sector every minute (ROADMAP_NEW E5).
    #[serde(default)]
    pub goes_sector: wxdata::goes_abi::Sector,
    /// How far from the active radar to draw Spotter Network dots, in km. 0 = no limit (the whole
    /// CONUS feed). Default 230 km, roughly the radar's own useful range.
    #[serde(default = "default_spotter_range_km")]
    pub spotter_range_km: f64,
    /// Play an audible chime when a new NWS warning appears.
    #[serde(default = "default_true")]
    pub alert_sound: bool,
    /// Interpolate radar gates (and the color lookup) instead of drawing hard gate squares.
    #[serde(default = "default_true")]
    pub smooth_radar: bool,
    /// Show the animated ring and tilt-progress bar beside the scrubber's Live badge while a live
    /// chunk stream is actively updating the active pane.
    #[serde(default = "default_true")]
    pub live_scan_indicator: bool,
    /// Display policy while following an in-progress Level II sweep.
    #[serde(default)]
    pub live_sweep_mode: LiveSweepMode,
    /// ntfy.sh topic for push notifications when a warning covers a saved location (empty = off).
    #[serde(default)]
    pub ntfy_topic: String,
    /// Discord incoming-webhook URL for alert delivery (empty = off). A user secret: it lives in
    /// settings.json only and is never committed.
    #[serde(default)]
    pub discord_webhook: String,
    /// Slack incoming-webhook URL for alert delivery (empty = off). User secret, as above.
    #[serde(default)]
    pub slack_webhook: String,
    /// Matrix homeserver base URL, e.g. `https://matrix.org` (empty = off).
    #[serde(default)]
    pub matrix_homeserver: String,
    /// Matrix room ID to post alerts into, e.g. `!abc:matrix.org`.
    #[serde(default)]
    pub matrix_room: String,
    /// Matrix access token. User secret, as above.
    #[serde(default)]
    pub matrix_token: String,
    /// MQTT broker hostname for publishing alerts and status (empty = off).
    #[serde(default)]
    pub mqtt_host: String,
    /// Broker port. 1883 is the plain default, 8883 the TLS one.
    #[serde(default = "default_mqtt_port")]
    pub mqtt_port: u16,
    /// Connect over TLS. Off by default: a house broker on the same LAN usually is not.
    #[serde(default)]
    pub mqtt_tls: bool,
    /// Broker username (empty = anonymous).
    #[serde(default)]
    pub mqtt_user: String,
    /// Broker password. A user secret: settings.json only, never committed.
    #[serde(default)]
    pub mqtt_pass: String,
    /// Topic prefix everything is published under, e.g. `home/weather`.
    #[serde(default = "default_mqtt_prefix")]
    pub mqtt_prefix: String,
    /// Topic the broker republishes lightning strikes on, e.g. `blitzortung/1.1/#`. Empty is off.
    ///
    /// The app never talks to a strike network itself — Blitzortung's terms ask third-party apps
    /// to run their own relay rather than point clients at theirs, and this is the subscriber end
    /// of that arrangement (see `scripts/strikes-relay/`).
    #[serde(default)]
    pub strikes_topic: String,
    /// Topic the user's NWWS-OI relay republishes warning text products on, e.g.
    /// `hookecho/nwws/#`. Empty is off, and the alerts feed alone is used, as before.
    ///
    /// The NWS Weather Wire pushes warnings as they are issued but needs the user's own account
    /// and a held connection; `scripts/nwws-relay/` is the publisher end (1008.md A1).
    #[serde(default)]
    pub warnings_topic: String,
    /// Publish retained Home Assistant discovery configs so it creates the device by itself.
    ///
    /// Off by default, and deliberately: retained config topics on a broker that is not running
    /// Home Assistant are litter somebody else has to clear up.
    #[serde(default)]
    pub mqtt_discovery: bool,
    /// Fetch terrain at z12 (~40 m/px) instead of z10. Sixteen times the tiles, so it is off by
    /// default and meant for chase packs you download deliberately.
    #[serde(default)]
    pub pack_hires_dem: bool,
    /// Include vector street tiles in a chase pack even when the active basemap is raster. Streets
    /// are what a chase pack is for; the raster imagery alone leaves you without road names.
    #[serde(default = "default_true")]
    pub pack_include_vector: bool,
    /// Include satellite imagery in a chase pack even when the active basemap is vector. The
    /// mirror of `pack_include_vector`: a road map offline is no help for spotting a wall cloud
    /// against terrain you have never seen.
    #[serde(default)]
    pub pack_include_satellite: bool,
    /// User-drawn zones that alert when an NWS warning polygon intersects them. The Android
    /// background service reads this file too, so the name is load-bearing across the boundary.
    #[serde(default)]
    pub alert_polygons: Vec<AlertPolygon>,
    /// User-written alert rules (see [`AlertRule`]). Empty by default: the five built-in alerts
    /// are what the app fires on until somebody says otherwise.
    #[serde(default)]
    pub alert_rules: Vec<AlertRule>,
    /// External-process plugins that emit placefiles (desktop only). Off by default and empty:
    /// each entry is a command the user chose to run.
    #[serde(default)]
    pub plugins: Vec<PluginConfig>,
    /// Android only: run a foreground service that watches `markers` for NWS alerts and posts a
    /// notification, so warnings arrive with the app closed. Opt-in — it costs a permanent
    /// notification and some battery. The switch itself reaches Kotlin over JNI
    /// (`platform::set_background_alerts`), not through this file; what the service reads here is
    /// `markers` and `alert_polygons`, so those names are load-bearing across the boundary.
    #[serde(default)]
    pub background_alerts: bool,
    /// Keep running in the background (hide to tray) instead of quitting when the window closes.
    #[serde(default)]
    pub close_to_tray: bool,
    /// User-saved view bookmarks (time-machine library, alongside the curated events).
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
    /// Anthropic API key for the optional plain-language storm digest (held locally only). Used
    /// when `ai_provider` is Anthropic; empty = the built-in templated summary.
    #[serde(default)]
    pub anthropic_key: String,
    /// Google AI Studio API key: the alternative to `anthropic_key` for the storm digest (held
    /// locally only). Used when `ai_provider` is Gemini.
    #[serde(default)]
    pub gemini_key: String,
    /// Which model writes the storm digest's prose. Claude by default, so a saved Anthropic key
    /// keeps working as before.
    #[serde(default)]
    pub ai_provider: crate::digest::Provider,
    /// Chime + push when cloud-to-ground lightning strikes within ~15 km of a saved location.
    #[serde(default)]
    pub lightning_alarm: bool,
    /// Read new warnings aloud after the alert tone.
    ///
    /// On by default. The tone says a warning exists and nothing else: not which counties, not
    /// which towns are in the path, not whether it is coming at you, not what to do. Someone
    /// driving cannot read the banner that carries all of that.
    #[serde(default = "default_true")]
    pub speak_warnings: bool,
    /// Path to a Piper binary, or blank to look on `PATH`. Piper is a local neural voice; when it
    /// and a voice model are both present, spoken warnings go through it instead of espeak.
    #[serde(default)]
    pub piper_path: String,
    /// Path to a Piper `.onnx` voice model. Blank turns Piper off — an engine with no model has
    /// nothing to say. Never committed: it is a ~60 MB download or a file the user already has.
    #[serde(default)]
    pub piper_voice: String,
    /// Which curated voice the picker has selected for download. Only the picker reads it;
    /// `piper_voice` above is still what actually speaks.
    #[serde(default)]
    pub piper_download_voice: String,
    /// While chase mode is on, speak the nearest storm's bearing and distance as it changes.
    #[serde(default)]
    pub speak_position: bool,
    /// Alert when radar echo is heading for a saved location.
    #[serde(default)]
    pub rain_alerts: bool,
    /// Sound for the rain-arrival alert.
    #[serde(default)]
    pub rain_sound: AlertSound,
    /// One-time hints already shown, by id. A hint is a sentence somebody needs once: showing it
    /// twice is nagging, and the only way to know is to remember. Ids, not text, so rewording a
    /// hint doesn't bring it back for everyone who already read it.
    #[serde(default)]
    pub hints_seen: Vec<String>,
    /// First-run setup completed (or dismissed). `false` shows the first-run card at startup.
    #[serde(default)]
    pub setup_done: bool,
    /// Post alerts to the platform's own notification centre, so they arrive with the window
    /// behind something else. Ignored on Android, where the alert service posts its own.
    #[serde(default)]
    pub desktop_notify: bool,
    /// Battery saver: relax every cadence the app controls — the idle repaint clock, the volume
    /// poll, the widget snapshot, and the background alert alarm on Android. Warnings still
    /// arrive; they arrive on a slower schedule. Off by default, because a weather app that
    /// quietly polls less often than it says it does is the wrong kind of surprise.
    #[serde(default)]
    pub battery_saver: bool,
    /// Draw the wind layer as barbs on a screen lattice as well as (or with particles off,
    /// instead of) the particles.
    #[serde(default)]
    pub wind_barbs: bool,
    /// Draw the wind layer, and a wind picked in the model field browser, as streamlines.
    #[serde(default)]
    pub wind_streamlines: bool,
    /// Record a breadcrumb track of the session's GPS fixes, exportable as GPX. Off by default:
    /// where you drove is yours, and nothing records it unless you say so. The track lives in
    /// memory only until you save it.
    #[serde(default)]
    pub chase_log: bool,
    /// ROADMAP_NEW L1: the routing server the Route tool asks — the user's own OSRM or Valhalla,
    /// or a public demo they chose knowingly. Empty until they pick one: no provider is assumed.
    #[serde(default)]
    pub route_engine: wxdata::route::Engine,
    #[serde(default)]
    pub route_url: String,
    /// Attach a picture of the radar to the ntfy push when a warning fires. Desktop only: the
    /// Android background service has no GPU surface to render from, and says so in the UI.
    #[serde(default)]
    pub ntfy_snapshot: bool,
    /// Watch wherever you are, not only the places you saved: while a GPS fix is coming in it
    /// joins the marker list for the proximity alerts (lightning, rotation), under the name
    /// "my location". Nothing is written to the saved markers and nothing is shared.
    #[serde(default)]
    pub alert_follow_gps: bool,
    /// Connect to the local gpsd at launch and stay in chase mode. The Chase tab's connect
    /// button was never remembered, so a laptop with a receiver on the dash had to be clicked
    /// back into following itself every start. Desktop only: Android and the web ask for a
    /// permission instead, and launch is the wrong moment to ask.
    #[serde(default)]
    pub gps_autoconnect: bool,
    /// Quiet hours: between `quiet_start_hour` and `quiet_end_hour` (local, 24h), alert sounds
    /// and pushes are held back. Escalated warnings — Tornado Emergency, PDS, destructive — go
    /// through anyway: the whole point of the tier is that it is worth waking up for.
    #[serde(default)]
    pub quiet_hours: bool,
    #[serde(default = "default_quiet_start")]
    pub quiet_start_hour: u32,
    #[serde(default = "default_quiet_end")]
    pub quiet_end_hour: u32,
    /// Lowest NWS escalation tier (see `wxdata::alerts::escalation`) allowed to push and sound.
    /// 0 lets everything through, which is the default.
    #[serde(default)]
    pub alert_min_escalation: u8,
    /// Alerts inside `alert_rollup_window_min` before pushes collapse into one rolling summary.
    /// 0 turns the rollup off. Escalated alerts always push as themselves.
    #[serde(default = "default_alert_rollup_threshold")]
    pub alert_rollup_threshold: usize,
    /// Window the rollup threshold counts over, in minutes.
    #[serde(default = "default_alert_rollup_window_min")]
    pub alert_rollup_window_min: u64,
    /// Chime when a new radar volume lands on the live pane in view.
    #[serde(default)]
    pub scan_chime: bool,
    /// Sound for the new-scan chime. Ding by default — a scan every four minutes should be a tap
    /// on the shoulder, not a warning tone.
    #[serde(default = "default_scan_sound")]
    pub scan_sound: AlertSound,
    /// Sound played when a new NWS warning appears (gated by `alert_sound`).
    #[serde(default)]
    pub warn_sound: AlertSound,
    /// Sound played on tornado-debris-signature detection.
    #[serde(default)]
    pub tds_sound: AlertSound,
    /// Sound played when a rotation couplet is detected (defaults to Siren — distinct from TDS).
    #[serde(default = "default_rotation_sound")]
    pub rotation_sound: AlertSound,
    /// Sound played on the lightning proximity alarm.
    #[serde(default)]
    pub lightning_sound: AlertSound,
    /// Sound played when an escalated (Tornado Emergency / PDS / destructive) warning appears.
    #[serde(default = "default_emergency_sound")]
    pub emergency_sound: AlertSound,
    /// Playback volume for all alert sounds (0.0..=1.0).
    #[serde(default = "default_volume")]
    pub alert_volume: f32,
    /// Number of newest volumes the live loop cycles over when playing.
    #[serde(default = "default_live_loop_frames")]
    pub live_loop_frames: usize,
    /// theme_plan.md §7: which visual layout the bottom timeline draws itself in.
    #[serde(default)]
    pub timeline_style: TimelineStyle,
    /// theme_plan.md §2.3: replace the WSV3 ribbon's docked "Search" group with a small floating
    /// icon button over the map instead. Off by default (today's docked behavior unchanged). Only
    /// affects the ribbon layouts (`Layout::is_ribbon()`) — the Minimal layout's own search pill
    /// is a combined menu/site/search bar, not a standalone control, and isn't affected by this
    /// (see that section's own note on why it needs a different fix).
    #[serde(default)]
    pub floating_search_button: bool,
    /// theme_plan.md §4: raises the process's log verbosity to debug (`crate::devlog`'s capture
    /// buffer only ever holds what actually passed the ambient `RUST_LOG` filter, which defaults
    /// to `info`) and opens a live filtered view of it — the level of detail an analyst wants
    /// (live-sweep chunk arrival, provider/failover health) that `info` never surfaces. Off by
    /// default: negligible cost when off (a boolean check), real log-volume cost when on.
    #[serde(default)]
    pub analyst_mode: bool,
    /// Persisted basemap style slug for startup (empty = the pane default, [`crate::tiles::BasemapStyle::default`]).
    #[serde(default)]
    pub basemap: String,
    /// Overlay toggles that were on when the app last ran, by name (see `OverlayToggle::slug`).
    /// Names rather than the enum on purpose: a name this build doesn't know is skipped, where an
    /// unknown enum variant would fail the parse and reset every other setting with it.
    ///
    /// `None` means no run has recorded its layers yet — a fresh install, or a file written before
    /// this key existed — and the app's built-in defaults stand. An empty list is a different
    /// statement: someone turned everything off, and it has to survive a restart.
    #[serde(default)]
    pub overlays_on: Option<Vec<String>>,
    /// Model contour kinds that were on (`ContourKind::token`), restored at startup.
    #[serde(default)]
    pub contours_on: Vec<String>,
    /// Desktop window size in logical points, and whether it was maximized, as of the last run.
    /// `None` on a first run, where the built-in 1280x800 stands.
    ///
    /// Kept here rather than turning on eframe's `persistence` feature: that pulls in a whole
    /// key-value store and its serialization to remember two floats and a bool that this file is
    /// already being written for.
    #[serde(default)]
    pub window: Option<WindowGeom>,
    /// Saved pane layouts (see `crate::workspace`), applied from the command palette. Distinct
    /// from `presets`, which is the starred-radar-site list.
    #[serde(default)]
    pub workspaces: Vec<crate::workspace::Workspace>,
    /// Whether the starter workspaces have been offered. Seeding on "the list is empty" alone
    /// would resurrect them every time someone deleted the last one, which is the opposite of
    /// what deleting the last one means.
    #[serde(default)]
    pub seeded_workspaces: bool,
    /// Every starter workspace name offered so far (`workspace::offer_new_starters`), so one
    /// shipped later reaches this file once and a deleted one is not offered again.
    #[serde(default)]
    pub offered_starters: Vec<String>,
    /// The model browser's last choice, as `model/product` (see `model_browser::Selection`).
    /// Empty means the default. An unknown value from a newer build falls back to the default.
    #[serde(default)]
    pub model_pick: String,
    /// NOAA Weather Radio relays to listen to. Empty by default on purpose: NOAA runs no streams of
    /// its own, so every URL here is a third-party relay someone runs for their own county, and
    /// shipping a guessed list would mostly ship dead links. Add the one for your area.
    #[serde(default)]
    pub nwr_streams: Vec<NwrStream>,
    /// Silence every alert sound and spoken warning at once, without touching the per-feature
    /// sound choices — the "I'm in a meeting" switch.
    #[serde(default)]
    pub mute_alerts: bool,
    /// User keyboard bindings. Empty means "never edited" and the app uses
    /// [`crate::hotkeys::defaults`]; the Hotkeys settings tab materializes the whole table on the
    /// first edit, so a later change to the defaults doesn't silently rewrite someone's keys.
    #[serde(default)]
    pub(crate) keybinds: Vec<crate::hotkeys::Binding>,
    /// mPING API key (free, from mping.ou.edu) — enables the crowd precipitation-type reports.
    /// Held locally only, same as the other keys.
    #[serde(default)]
    pub mping_key: String,
    /// Reflectivity threshold (dBZ) defining the derived echo-top height. 18.5 matches the NWS
    /// Enhanced Echo Tops product; raise it to track the core rather than the anvil.
    #[serde(default = "default_etop_dbz")]
    pub etop_dbz: f32,
    /// Caption saved and copied images with site, product, valid time and source. On by default:
    /// a radar picture that leaves the app without those four things cannot be checked by whoever
    /// receives it.
    #[serde(default = "default_true")]
    pub share_card: bool,
    /// Exported loops hold each frame for the real time to the next scan (scaled to the playback
    /// speed) rather than all alike — so a SAILS rescan is a quick step and a gap in the data is
    /// a pause, as they were.
    #[serde(default = "default_true")]
    pub loop_real_timing: bool,
    /// The frame rate exported MP4 loops are encoded at: 30, or 60 for broadcast (1008.md F2).
    #[serde(default = "default_mp4_fps")]
    pub loop_mp4_fps: u32,
    /// Show a continuous archived field (MRMS reflectivity, rotation) at the radar's own time,
    /// interpolated between the frames either side, instead of the nearest frame (1008.md E2).
    /// Never for categories or accumulations; probes and exports say a frame is interpolated.
    #[serde(default)]
    pub blend_frames: bool,
    /// Fade a continuous field layer's new frame in over the old one for a quarter second
    /// (`crate::field_fade`), instead of switching at once. Visual only: probes and exports read
    /// the new frame. Categories and satellite RGB composites never fade. Off by default.
    #[serde(default)]
    pub field_crossfade: bool,
    /// Percent of the wind particles drawn, set by the quality profile (`crate::quality`).
    #[serde(default = "default_particle_pct")]
    pub wind_particle_pct: u32,
    /// The output window as it was left (1008.md F3): reopened at launch, so an OBS window
    /// capture finds "HookEcho Output" again after a restart without anyone reopening it.
    #[serde(default)]
    pub output_window: OutputPrefs,
    /// How streaming mode dresses the map for air: margins, clock, caption, crawl, logo and
    /// whether the colour scale shows (`crate::broadcast`).
    #[serde(default)]
    pub broadcast: crate::broadcast::Broadcast,
    /// Saved broadcast scenes, switched with Alt+1..9 (`app::scenes`, ROADMAP_2 §6.3).
    #[serde(default)]
    pub scenes: Vec<crate::broadcast::Scene>,
    /// Registry labels in the order the user dragged them, across every category. Labels not in
    /// here keep their registry order behind the ones that are — so a reorder never hides a row,
    /// and a renamed action just falls back to its default place.
    #[serde(default)]
    pub layer_order: Vec<String>,
    /// ROADMAP_NEW D1/D3 "recent products": [`crate::render::FieldLayer`] slugs the user has
    /// turned on, most-recent-first, capped at [`crate::ui::layers_panel::RECENT_LAYERS_CAP`].
    /// Only a genuine user toggle pushes here (`apply_palette`'s `ToggleField` arm) — workspace
    /// restore and internal bookkeeping (HRRR sub-mode, model compare A/B) write `fields_on`
    /// directly and never touch this, so loading a saved workspace does not masquerade as
    /// something the user just picked.
    #[serde(default)]
    pub recent_layers: Vec<String>,
    /// ROADMAP_NEW D3 "favorites": [`crate::render::FieldLayer`] slugs the user has explicitly
    /// starred, in the order they were added — unlike `recent_layers`, nothing here happens except
    /// by clicking the star, and there is no cap: a short list the user curated on purpose is the
    /// opposite case from an automatic recency trail, which is exactly why it needed a cap to stay
    /// useful.
    #[serde(default)]
    pub favorite_layers: Vec<String>,
    /// Thresholds the signature detectors fire at (see [`DetectorTuning`]).
    #[serde(default)]
    pub detectors: DetectorTuning,
    /// Bearer token `--serve` requires on every request; empty leaves the server open, which is
    /// what loopback-only has always been. A user secret: settings.json only, never committed,
    /// and device-local so it does not travel to machines that are not this server.
    #[serde(default)]
    pub serve_token: String,
    /// Pushes quiet hours held back, kept across a restart so the catch-up summary still arrives
    /// when the window ends. Device-local (see `cloud::DEVICE_LOCAL`): they are owed to whoever
    /// is at this machine.
    #[serde(default)]
    pub quiet_pending: Vec<(String, String)>,
    /// Cap for the on-disk radar-volume cache, in MB. 0 = the platform default (2 GB desktop,
    /// 300 MB Android). Applied by the startup sweep, so a change takes effect next launch.
    #[serde(default)]
    pub volume_cache_mb: u32,
    /// Cap for each on-disk map-tile cache (raster and vector), in MB. 0 = platform default.
    #[serde(default)]
    pub tile_disk_cache_mb: u32,
}

/// Where the dual-pol signature detectors and the GLM flash-extent grid draw their lines.
///
/// Every one of these was a constant, which is fine until you point the app at a radar whose
/// calibration or season disagrees: a ZDR floor that is right in May over Oklahoma is noise in
/// December over Buffalo. The defaults are the values the detectors shipped with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectorTuning {
    /// Reflectivity (dBZ) a core must reach before a hail spike behind it is looked for.
    pub tbss_core_dbz: f32,
    /// Differential reflectivity (dB) a gate must reach to count toward a ZDR column.
    pub zdr_min_db: f32,
    /// How far a column must extend above the freezing level (km) to be reported.
    pub zdr_min_depth_km: f64,
    /// GLM flash-extent density grid cell size, in degrees (~5 km at the default).
    pub glm_fed_cell_deg: f64,
    /// How far back the flash-extent density grid counts, in minutes.
    pub glm_fed_window_min: i64,
    /// The least confidence (0..1) a debris signature needs to be drawn, to count for alert rules,
    /// or to raise the TDS alert. Zero shows everything the detector finds. See
    /// [`DEFAULT_TDS_MIN_CONFIDENCE`] for the default.
    #[serde(default = "default_tds_min_confidence")]
    pub tds_min_confidence: f32,
    /// The same for rotation couplets: the least confidence (0..1) one needs to be drawn, to count
    /// for alert rules, or to raise the rotation alert. Zero shows everything. See
    /// [`DEFAULT_ROTATION_MIN_CONFIDENCE`].
    #[serde(default = "default_rotation_min_confidence")]
    pub rotation_min_confidence: f32,
    /// Whether the one-time move off the old 0% floors has happened; see
    /// [`Settings::adopt_detector_floors`]. A settings file from before it reads `false`.
    #[serde(default)]
    pub floors_adopted: bool,
    /// Draw the experimental LLSD rotation pipeline (`wxdata::llsd_analyst`) beside the regular
    /// detectors, for analysts: columns, tracks, classified debris and their evidence terms. Off by
    /// default; it raises no alert and feeds nothing else (detectionplan.md Phases 12-13).
    #[serde(default)]
    pub llsd_preview: bool,
    /// Where Tornado ID's verdicts come from. The redesigned fusion by default (detectionplan.md
    /// Phase 13); the legacy couplet-and-debris Tornado ID stays selectable for comparison. The
    /// rotation and debris layers, and their alerts, are the same either way.
    #[serde(default)]
    pub tornado_id_source: TornadoIdSource,
    /// The fused Tornado ID shows a rooted, cyclonic column with no debris beside it as Possible
    /// once its 0-2 km shear is at least this (s⁻¹), though its evidence score is under Possible
    /// (`llsd_analyst::Analysed::tornado_id_with`); `None` is off. Starts at 0.018
    /// ([`DEFAULT_ROTATION_ONLY_POSSIBLE`]). A file without the key gets that default; one that
    /// says `null` keeps it off.
    #[serde(default = "default_rotation_only_possible")]
    pub rotation_only_possible: Option<f32>,
    /// Draw a rotation-only Possible at the first low-level scan that reads it. Off (the
    /// default), it waits until its track has been a verdict on an earlier scan too
    /// (`llsd_analyst::PassConfirmation`): drawn at every scan, these doubled the false Possible
    /// markers; waiting one scan kept most of the tornadoes found on ordinary severe days at about
    /// the false rate of reading once a volume.
    #[serde(default)]
    pub early_rotation: bool,
    /// Hide a Possible verdict where the HRRR's air beside it cannot support a tornado
    /// (`wxdata::near_storm::GATE_STP`). On by default: with every other rule in place it took
    /// ordinary severe days from 0.48 to 0.38 false marker episodes per radar-hour, for POD 0.45
    /// to 0.40 (detectionplan.md). Off, the card still shows the environment. A file without the
    /// key gets it on.
    #[serde(default = "default_environment_gate")]
    pub environment_gate: bool,
}

fn default_environment_gate() -> bool {
    true
}

/// Which pipeline Tornado ID shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TornadoIdSource {
    /// LLSD rotation columns, tracked, with classified debris, fused into one evidence score
    /// (`wxdata::llsd_analyst`). Held out by event on the 25-event backtest, at about 1.5 false
    /// alarms per radar-hour it found more tornadoes than the legacy one (POD 0.32 against 0.28)
    /// with a far lower false-alarm ratio (0.34 against 0.48).
    #[default]
    Fusion,
    /// The original: legacy couplets and debris signatures (`wxdata::tornado_id`).
    Legacy,
    /// Both at once, to compare them (a test mode): the fused pipeline's markers as with
    /// [`Self::Fusion`] (they alert and open the card), and the original's beside them, drawn
    /// hollow and labelled "Original", which never alert.
    Compare,
}

impl TornadoIdSource {
    /// Whether the fused pipeline makes the verdicts that are shown and alert.
    pub fn fused(self) -> bool {
        !matches!(self, TornadoIdSource::Legacy)
    }
}

/// Where Tornado detection's rotation-only Possible bar starts (s⁻¹). On 259 random ordinary
/// severe-weather windows (2018-2025; 2018, 2020 and 2025 held out from every rule's tuning), the
/// markers the app draws (the backtest's `tornado_marker` rows, `scripts/fusion/markers.py`) find
/// 38% of the tornadoes at 0.56 false Possible markers per radar-hour with it; the original
/// Tornado ID finds 16% at 3.14. Without it the fused tier found about a tenth (detectionplan.md).
/// Possible raises no alert. The maintainer chose it over 0.020 and leaving it off.
pub const DEFAULT_ROTATION_ONLY_POSSIBLE: f32 = 0.018;

fn default_rotation_only_possible() -> Option<f32> {
    Some(DEFAULT_ROTATION_ONLY_POSSIBLE)
}

/// The debris-signature floor a fresh install starts with. From the archived-event backtest
/// (`--headless-backtest-file docs/backtest-events.txt`, `tds-6`): at 60% the detector found as
/// many of the 37 tornado reports as at 50% (49%) with far fewer false alarms (FAR 51% against
/// 70%); below 50% the extra reports come at FAR over 80%, above 60% a tornado starts to drop out.
pub const DEFAULT_TDS_MIN_CONFIDENCE: f32 = 0.6;

/// The rotation-couplet floor a fresh install starts with. Rotation comes before debris, so this
/// one leans toward finding more: at 50% the backtest found 65% of the tornado reports (FAR 64%);
/// at 60%, 53% (FAR 52%) — twelve points of detection is too much to give up for an early alarm.
pub const DEFAULT_ROTATION_MIN_CONFIDENCE: f32 = 0.5;

fn default_tds_min_confidence() -> f32 {
    DEFAULT_TDS_MIN_CONFIDENCE
}

fn default_rotation_min_confidence() -> f32 {
    DEFAULT_ROTATION_MIN_CONFIDENCE
}

impl Default for DetectorTuning {
    fn default() -> Self {
        Self {
            tbss_core_dbz: 60.0,
            zdr_min_db: 1.0,
            zdr_min_depth_km: 1.0,
            glm_fed_cell_deg: 0.05,
            glm_fed_window_min: 15,
            tds_min_confidence: DEFAULT_TDS_MIN_CONFIDENCE,
            rotation_min_confidence: DEFAULT_ROTATION_MIN_CONFIDENCE,
            floors_adopted: true,
            llsd_preview: false,
            tornado_id_source: TornadoIdSource::Fusion,
            rotation_only_possible: default_rotation_only_possible(),
            early_rotation: false,
            environment_gate: true,
        }
    }
}

impl Settings {
    /// Move a tablet off the floating ribbon and onto the docked layout, once.
    ///
    /// The ribbon is a desktop layout: on a tablet its groups overflow the width and get clipped,
    /// the colour scale runs across the tilt row and the timeline floats over the map, which reads
    /// as clutter. The dock lays the same controls out in panels with nothing over the map. Only a
    /// tablet still on the shipped default (the ribbon) is moved, and only once, so a layout picked
    /// on purpose is kept and one changed back later stays changed. Returns whether it changed
    /// anything.
    pub fn adopt_tablet_default(&mut self, is_tablet: bool) -> bool {
        if !is_tablet || self.tablet_layout_adopted {
            return false;
        }
        self.tablet_layout_adopted = true;
        if self.layout != Layout::CommandRibbon {
            return false;
        }
        self.layout = Layout::Dock;
        true
    }

    /// Move anyone still on the ribbon, the old default, to the dock, now the default, once —
    /// A ribbon picked again afterwards stays picked, and any other
    /// layout is kept. Returns whether it changed anything.
    pub fn adopt_dock_default(&mut self) -> bool {
        if self.dock_adopted {
            return false;
        }
        self.dock_adopted = true;
        if self.layout != Layout::CommandRibbon {
            return false;
        }
        self.layout = Layout::Dock;
        true
    }

    /// Move a phone still on the old default design (Aurora) to Station, the workstation's
    /// windows in a bottom sheet, once. A design picked on purpose is kept, and one changed back
    /// later stays changed. Returns whether it changed anything.
    pub fn adopt_station_default(&mut self) -> bool {
        if self.station_adopted {
            return false;
        }
        self.station_adopted = true;
        if self.phone_design != PhoneDesign::Aurora {
            return false;
        }
        self.phone_design = PhoneDesign::Station;
        true
    }

    /// Move the detector floors off the old 0% default onto the backtested ones, once.
    ///
    /// Floors used to default to 0%, so every settings file saved since has an explicit 0 in it
    /// that is almost never a choice. Each floor still at exactly 0 moves to its new default; one
    /// set to anything else was picked on purpose and stays. Once only, so a floor put back to 0
    /// afterwards stays at 0. Returns whether it changed anything.
    pub fn adopt_detector_floors(&mut self) -> bool {
        let d = &mut self.detectors;
        if d.floors_adopted {
            return false;
        }
        d.floors_adopted = true;
        let mut changed = false;
        if d.tds_min_confidence == 0.0 {
            d.tds_min_confidence = DEFAULT_TDS_MIN_CONFIDENCE;
            changed = true;
        }
        if d.rotation_min_confidence == 0.0 {
            d.rotation_min_confidence = DEFAULT_ROTATION_MIN_CONFIDENCE;
            changed = true;
        }
        changed
    }

    /// Timezone to render `site`'s timestamps in — `None` means "show Zulu", either because the
    /// user picked UTC or because the site has no known zone.
    pub fn tz_for(&self, site: Option<&str>) -> Option<wxdata::tz::Tz> {
        match self.time_display {
            TimeDisplay::Utc => None,
            TimeDisplay::SiteLocal => site.and_then(wxdata::tz::site_tz),
        }
    }
}

/// A saved view: site + camera, and (for archive views) the UTC instant to seek to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bookmark {
    pub name: String,
    pub site: String,
    /// Camera center in web-mercator world space `[0,1]²`.
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
    /// UTC time to seek to (Unix seconds); `None` = live/head.
    #[serde(default)]
    pub time_secs: Option<i64>,
    /// Replay window around `time_secs`, in minutes. `0` (and any bookmark written before this
    /// existed) means a still: jump there and stop.
    #[serde(default)]
    pub span_min: u16,
}

fn default_quiet_start() -> u32 {
    22
}

pub fn default_alert_rollup_threshold() -> usize {
    5
}
pub fn default_alert_rollup_window_min() -> u64 {
    10
}
fn default_quiet_end() -> u32 {
    7
}

fn default_scan_sound() -> AlertSound {
    AlertSound::Ding
}

fn default_far_3d() -> f32 {
    2.5
}

fn default_mp4_fps() -> u32 {
    crate::loopexport::MP4_FPS
}

fn default_particle_pct() -> u32 {
    100
}

fn default_true() -> bool {
    true
}

fn default_etop_dbz() -> f32 {
    18.5
}

fn default_rotation_sound() -> AlertSound {
    AlertSound::Siren
}

fn default_emergency_sound() -> AlertSound {
    AlertSound::Eas
}

/// A portable settings export: the full settings plus inlined `.pal` contents (by moment short
/// name), so palette overrides survive moving to a machine where the original paths don't exist.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SettingsBundle {
    settings: Settings,
    #[serde(default)]
    palette_files: BTreeMap<String, String>,
    /// The imported GIS layers' files (ROADMAP_PARITY M4.4), by the path the layers name them
    /// by, each with its SHA-256, so the bundle opens on another machine and a damaged copy is
    /// refused. Absent in a bundle written before they were packaged.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    gis_files: BTreeMap<String, PackedFile>,
    /// Layer files that could not be read when the bundle was written, so are not in it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    gis_unpacked: Vec<String>,
}

/// One file carried in a settings bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PackedFile {
    /// Its file name, for the copy written on import.
    name: String,
    sha256: String,
    base64: String,
}

/// The file a GIS layer source names: the path itself, or the zip of a `bundle.zip#dataset.shp`
/// source.
fn gis_source_file(source: &str) -> &str {
    crate::gis_import::split_dataset(source).0
}

/// A remembered startup camera: which site to load and where the map sits (world coords).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartView {
    pub site: String,
    /// Camera center in web-mercator world space `[0,1]²`.
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

/// One external-process plugin: a command that prints a placefile on stdout.
///
/// `refresh_secs` is the app's cadence, not the placefile's own `RefreshSeconds` — a plugin that
/// samples something live wants to be asked again on a schedule the user controls.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_plugin_refresh")]
    pub refresh_secs: u32,
    pub enabled: bool,
}

fn default_plugin_refresh() -> u32 {
    60
}

/// One NOAA Weather Radio relay the user has added.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NwrStream {
    /// What to call it in the picker, e.g. "KEC55 Norman".
    pub name: String,
    /// A streaming audio URL (Icecast-style MP3).
    pub url: String,
}

/// A configured placefile overlay (URL + on/off + opacity), persisted across sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacefileConfig {
    pub url: String,
    pub enabled: bool,
    /// Draw opacity 0..=1, set in the Layer Manager.
    #[serde(default = "default_opacity")]
    pub opacity: f32,
}

fn default_opacity() -> f32 {
    1.0
}

/// A user-drawn watch zone: a closed ring of `[lon, lat]` that raises an alert whenever an NWS
/// warning polygon touches it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertPolygon {
    pub name: String,
    /// Outer ring only, `[lon, lat]`.
    // ponytail: no holes, and editing is delete-and-redraw. A vertex editor is a lot of UI for a
    // shape that takes four clicks to redraw.
    pub ring: Vec<[f64; 2]>,
}

/// What a rule watches for. The scan signatures are the ones the app already computes per
/// volume; the rest ride on feeds it already polls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RuleTrigger {
    /// Tornado debris signature.
    Tds,
    /// Three-body scatter spike (hail).
    Tbss,
    /// A ZDR column above the freezing level.
    ZdrColumn,
    /// A velocity couplet; the threshold is minimum Vrot in knots.
    Rotation,
    /// NOAA ProbSevere; the threshold is the minimum Severe percentage.
    ProbSevere,
    /// GLM flash-extent density; the threshold is minimum flashes per cell per window.
    GlmFed,
    /// A lightning jump: how fast flash-extent density is *rising*, in flashes per cell per
    /// minute. The threshold is that rate.
    GlmJump,
    /// An NWS warning whose event name contains this text, case-insensitively. Empty matches
    /// every warning.
    Warning { event_contains: String },
}

impl RuleTrigger {
    /// Every trigger, with the defaults a fresh rule starts on — menu order.
    pub const ALL: [RuleTrigger; 8] = [
        RuleTrigger::Tds,
        RuleTrigger::Tbss,
        RuleTrigger::ZdrColumn,
        RuleTrigger::Rotation,
        RuleTrigger::ProbSevere,
        RuleTrigger::GlmFed,
        RuleTrigger::GlmJump,
        RuleTrigger::Warning {
            event_contains: String::new(),
        },
    ];

    pub fn label(&self) -> &'static str {
        match self {
            RuleTrigger::Tds => "Debris signature (TDS)",
            RuleTrigger::Tbss => "Hail spike (TBSS)",
            RuleTrigger::ZdrColumn => "ZDR column",
            RuleTrigger::Rotation => "Rotation",
            RuleTrigger::ProbSevere => "ProbSevere",
            RuleTrigger::GlmFed => "Lightning density",
            RuleTrigger::GlmJump => "Lightning jump",
            RuleTrigger::Warning { .. } => "NWS warning",
        }
    }

    /// What the threshold means for this trigger, and its default — `None` where the trigger is
    /// its own answer (a debris signature does not come in degrees).
    pub fn threshold_hint(&self) -> Option<(&'static str, f64)> {
        match self {
            RuleTrigger::Rotation => Some(("kt Vrot", 40.0)),
            RuleTrigger::ProbSevere => Some(("% severe", 50.0)),
            RuleTrigger::GlmFed => Some(("flashes", 20.0)),
            RuleTrigger::GlmJump => Some(("flashes/min rise", 4.0)),
            _ => None,
        }
    }

    /// Whether this trigger is answered by a per-volume scan of the active pane's radar, as
    /// opposed to a national feed. Scan triggers are what the headless verifier can replay.
    pub fn is_scan(&self) -> bool {
        matches!(
            self,
            RuleTrigger::Tds | RuleTrigger::Tbss | RuleTrigger::ZdrColumn | RuleTrigger::Rotation
        )
    }
}

/// Where a rule is allowed to fire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum RulePlace {
    /// Anywhere the active radar can see. Loud by design — worth pairing with a cooldown.
    #[default]
    Anywhere,
    /// Within a marker's own `alert_radius_mi`. Keyed by [`Marker::id`], so renaming the marker
    /// does not silently detach the rule.
    Marker { id: String },
    /// Touching a drawn watch zone, by [`AlertPolygon::name`].
    Zone { name: String },
}

/// One user rule: "if this signature shows up there, tell me".
///
/// The five built-in alerts are fixed — they fire on what somebody else decided was worth
/// waking up for. This is the same machinery pointed at the user's own question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertRule {
    /// Stable id; the cooldown map keys on it, so renaming a rule does not re-arm it.
    pub id: String,
    /// What the user calls it. Empty falls back to the trigger's label.
    #[serde(default)]
    pub name: String,
    pub trigger: RuleTrigger,
    /// Trigger-dependent minimum (see [`RuleTrigger::threshold_hint`]); ignored where the
    /// trigger takes none.
    #[serde(default)]
    pub threshold: Option<f64>,
    #[serde(default)]
    pub place: RulePlace,
    /// Push past quiet hours. Off by default: the user decides what is worth waking up for, and
    /// the default answer is "not this".
    #[serde(default)]
    pub urgent: bool,
    /// Minutes before the same rule may fire for the same place again.
    #[serde(default = "default_rule_cooldown")]
    pub cooldown_min: u16,
    /// Rules are created switched off, and armed deliberately.
    #[serde(default)]
    pub enabled: bool,
    /// Play this sound when the rule fires, instead of nothing. `None` keeps the old behaviour:
    /// the rule banners and pushes but makes no noise of its own.
    #[serde(default)]
    pub sound: Option<AlertSound>,
    /// Attach a picture of the map to the rule's push (desktop only, same path the warning
    /// snapshot uses).
    #[serde(default)]
    pub snapshot: bool,
    /// Extra conditions on top of the trigger. Empty is the old single-condition rule, and an old
    /// rule deserializes into exactly that.
    #[serde(default)]
    pub conditions: Vec<RuleCondition>,
    /// How the extra conditions combine among themselves. The rule's own trigger is always
    /// required — it is what starts the evaluation.
    #[serde(default)]
    pub combine: RuleCombinator,
}

/// One extra thing that must (or may) also be true for a rule to fire.
///
/// Deliberately a trigger and a threshold, not an expression: one level, no nesting. A rule
/// nobody can read at 3am is a rule nobody trusts.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RuleCondition {
    pub trigger: RuleTrigger,
    #[serde(default)]
    pub threshold: Option<f64>,
}

/// How a rule's extra conditions combine.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RuleCombinator {
    /// Every extra condition must also hold.
    #[default]
    And,
    /// At least one of them must.
    Or,
}

impl RuleCombinator {
    pub const ALL: [RuleCombinator; 2] = [RuleCombinator::And, RuleCombinator::Or];

    pub fn label(self) -> &'static str {
        match self {
            RuleCombinator::And => "and also",
            RuleCombinator::Or => "and either",
        }
    }
}

/// Ten minutes, matching the built-in proximity alerts' own cooldown.
pub fn default_rule_cooldown() -> u16 {
    10
}

impl AlertRule {
    /// A fresh, disabled rule.
    pub fn new(trigger: RuleTrigger) -> Self {
        let threshold = trigger.threshold_hint().map(|(_, d)| d);
        Self {
            id: new_marker_id(),
            name: String::new(),
            trigger,
            threshold,
            place: RulePlace::Anywhere,
            urgent: false,
            cooldown_min: default_rule_cooldown(),
            enabled: false,
            sound: None,
            snapshot: false,
            conditions: Vec::new(),
            combine: RuleCombinator::default(),
        }
    }

    /// What to call it in a notification.
    pub fn title(&self) -> String {
        if self.name.trim().is_empty() {
            self.trigger.label().to_string()
        } else {
            self.name.clone()
        }
    }
}

pub fn default_lightning_minutes() -> u16 {
    5
}

fn default_mqtt_port() -> u16 {
    1883
}

fn default_mqtt_prefix() -> String {
    "hookecho".to_string()
}

pub fn default_spotter_range_km() -> f64 {
    230.0
}

/// A named location marker at a geographic point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    /// Stable identity, independent of the name. Names are the display field and are not unique
    /// — "Marker 3" comes back after a delete — so anything that remembers a marker between
    /// frames (alert cooldowns, most of all) keys on this instead. Empty in files written before
    /// ids existed; [`Settings::load`] fills those in once.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// Optional icon: a filename inside [`Settings::marker_icons_dir`] (not a full path, so the
    /// settings stay portable). `None` draws the default accent dot.
    #[serde(default)]
    pub icon: Option<String>,
    /// Alert when a warning comes within this many miles of the marker, not only when the polygon
    /// covers it. A warning three counties wide still matters when its edge is down the road.
    #[serde(default = "default_alert_radius_mi")]
    pub alert_radius_mi: f64,
    /// Optional live-video URL for this place (a yard camera, a chase stream). Direct HLS/MJPEG
    /// plays in-app; anything else opens in the browser.
    #[serde(default)]
    pub video_url: String,
    /// The one marker that is home: drawn with a ring, and the place alerts speak of by default.
    /// At most one marker has this set (the marker editor enforces it).
    #[serde(default)]
    pub home: bool,
}

/// A fresh marker id: 8 hex characters from the system's randomness, which is plenty for a list
/// a person types by hand.
pub fn new_marker_id() -> String {
    let mut b = [0u8; 4];
    // A duplicate id would only collapse two markers' alert cooldowns; falling back to a
    // time-based id beats refusing to make the marker.
    if getrandom::fill(&mut b).is_err() {
        let t = chrono::Utc::now().timestamp_subsec_nanos();
        b = t.to_le_bytes();
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Default watch radius for a marker, in miles.
pub fn default_alert_radius_mi() -> f64 {
    20.0
}

/// Which desktop/web chrome the app draws — theme_plan.md §6's "Theme" (chrome + a recommended
/// color scheme), as distinct from `Theme` itself (§6.1's "Color scheme": colors only). Renamed
/// from a 2-variant enum where the ribbon layout was itself called `Wsv3`: that name is now the
/// new, denser theme built to match TempoQuest's WSV3 desktop app rather than the original
/// HookEcho ribbon, which is renamed `CommandRibbon` to make room for it — an old `settings.json`
/// with `"layout": "Wsv3"` still loads as `CommandRibbon` via the alias below, not silently reset
/// to a different theme.
///
/// `CommandRibbon` is the original WSV3-*style* pro layout: a docked ribbon of labeled control
/// groups over a navy→black gradient, a docked colour scale, and a bottom status bar. `Wsv3` and
/// `Dock` are the analyst workstation (docs/WSV3_IMGUI_MODERN_DESIGN_PLAN.md): a two-tier top
/// bar, tool windows that dock or float, a tool rail and a docked timeline — `Wsv3` opening
/// map-first, `Dock` with its windows showing. `Minimal` is the original map-first floating
/// chrome — a search pill, a right-edge control column, and panels that slide over the map.
/// Android always uses its own touch chrome regardless of this.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum Layout {
    #[serde(alias = "Wsv3")]
    CommandRibbon,
    /// Serialized as `"Wsv3Theme"`, not `"Wsv3"` — that string is `CommandRibbon`'s alias above
    /// (its pre-rename name), and a unit variant's own implicit tag would otherwise collide with
    /// it (this variant is new, so there is no pre-existing `settings.json` naming to preserve
    /// for it specifically).
    ///
    /// The analyst workstation, map-first: the same chrome as `Dock` with its tool windows closed
    /// until asked for.
    #[serde(rename = "Wsv3Theme")]
    Wsv3,
    Minimal,
    /// The analyst workstation with its tool windows showing: Layers docked left, the Inspector
    /// over the map, the timeline under it. For a tablet or a desktop that wants every control
    /// visible at once. The default layout.
    #[default]
    Dock,
}

impl Layout {
    /// The default first.
    pub const ALL: [Layout; 4] = [
        Layout::Dock,
        Layout::CommandRibbon,
        Layout::Wsv3,
        Layout::Minimal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Layout::CommandRibbon => "Command Ribbon",
            Layout::Wsv3 => "WSV3",
            Layout::Minimal => "Minimal (map-first)",
            Layout::Dock => "Dock (ImGui)",
        }
    }

    /// Whether this theme uses the docked ribbon chrome (`wsv3_ribbon`/`wsv3_status_bar`) — only
    /// `CommandRibbon` now; `Wsv3` moved to the workstation chrome ([`Layout::is_workstation`]).
    pub fn is_ribbon(self) -> bool {
        self == Layout::CommandRibbon
    }

    /// Whether this layout draws the analyst workstation (`app::chrome::dock`): app bar, context
    /// toolbar, tool windows and a docked timeline, and none of the floating chrome. `Dock` opens
    /// with its tool windows showing; `Wsv3` opens map-first ([`Layout::map_first`]).
    pub fn is_workstation(self) -> bool {
        matches!(self, Layout::Dock | Layout::Wsv3)
    }

    /// A workstation layout that opens with only the map, the bars and the timeline: its Layers
    /// and Inspector windows wait to be asked for (docs/WSV3_IMGUI_MODERN_DESIGN_PLAN.md §2.1).
    pub fn map_first(self) -> bool {
        self == Layout::Wsv3
    }
}

/// Which of the five phone designs draws the touch chrome (`ui::phone_design` says how each
/// differs). Only used where the phone chrome is: on a tablet the desktop layout is drawn and this
/// is ignored, and it changes how the chrome looks and where its buttons sit, never what the app
/// can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PhoneDesign {
    /// The analyst workstation on a phone: an app bar, a Site / Product / Tilt row, a tool rail
    /// over the map, and the workstation's windows as the tabs of a bottom sheet
    /// (`app::chrome::dock::phone`).
    #[default]
    Station,
    /// Clean, minimal, touch-friendly: a right-hand tool rail and a 2D / Tilt toggle.
    Aurora,
    /// Analyst-focused: a left tool rail, a tall dBZ scale and a squarer look.
    Storm,
    /// Dark and high-contrast, with labelled tools and a red accent.
    Carbon,
    /// Translucent panels over the map, with a bottom tab bar.
    Glass,
    /// Built around the 3D modes: all four mode buttons and a corner colour scale.
    Atlas,
}

/// theme_plan.md §7: which visual layout the bottom timeline (`app/chrome/scrubber.rs`) draws
/// itself in — independent of `Layout`/chrome theme (a WSV3-*look* user might still prefer a
/// leaner timeline, or vice versa), since `scrubber()` is shared by every `Layout` today. Every
/// style drives the same underlying `crate::timeline::Timeline` state (`playhead`, `following`,
/// `playing`) — only the paint/interaction surface differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TimelineStyle {
    /// Today's look: a floating pill with a hand-drawn scrub track (hour ticks, the forecast
    /// tail, the live window shaded), transport buttons, and a popup menu carrying the calendar,
    /// DVR-depth and rain-ETA extras.
    #[default]
    Default,
    /// Styled after the WSV3 desktop app's timeline (theme_plan.md Ref 4): an explicit transport
    /// row (skip-to-start/rewind/pause-play/fast-forward/skip-to-end), a loop-length control, and
    /// a plain position slider below — no hand-drawn track, no popup menu.
    Wsv3,
    /// A slim single-row strip: the same hand-drawn scrub track as `Default` at its existing
    /// `compact` painting, but without the popup menu or the rain-ETA/DVR-depth extras — the
    /// general "compact timeline" archetype common to GR2Analyst/WeatherFront-style tools rather
    /// than a pixel-exact copy of either (no reference for either was available to build against
    /// — see theme_plan.md §7's own note on this).
    Compact,
}

impl TimelineStyle {
    pub const ALL: [TimelineStyle; 3] = [
        TimelineStyle::Default,
        TimelineStyle::Wsv3,
        TimelineStyle::Compact,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TimelineStyle::Default => "Default",
            TimelineStyle::Wsv3 => "WSV3",
            TimelineStyle::Compact => "Compact",
        }
    }
}

/// Whether radar timestamps read in the selected site's local time or in UTC ("Zulu").
///
/// Site-local is the default: the clock a chaser cares about is the one the storm is under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TimeDisplay {
    #[default]
    SiteLocal,
    Utc,
}

impl TimeDisplay {
    pub const ALL: [TimeDisplay; 2] = [TimeDisplay::SiteLocal, TimeDisplay::Utc];

    pub fn label(self) -> &'static str {
        match self {
            TimeDisplay::SiteLocal => "Site local",
            TimeDisplay::Utc => "UTC (Zulu)",
        }
    }
}

/// Display unit for temperature. Observations arrive in Celsius; US surface plots read in
/// Fahrenheit, which is why that is the default here and not the one on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TempUnit {
    #[default]
    Fahrenheit,
    Celsius,
}

impl TempUnit {
    pub const ALL: [TempUnit; 2] = [TempUnit::Fahrenheit, TempUnit::Celsius];

    pub fn label(self) -> &'static str {
        match self {
            TempUnit::Fahrenheit => "°F",
            TempUnit::Celsius => "°C",
        }
    }

    /// Convert an observation's Celsius into this unit.
    pub fn from_c(self, c: f32) -> f32 {
        match self {
            TempUnit::Fahrenheit => c * 9.0 / 5.0 + 32.0,
            TempUnit::Celsius => c,
        }
    }
}

/// ROADMAP_NEW B6.9's "preserve a manual provider override in Advanced settings for diagnostics" —
/// forces which of the three [`crate::radar_provider_manager`] tiers is active for live NEXRAD
/// radar, bypassing the automatic failover arbiter. `Auto` (default) is ordinary operation; the
/// other three hold a specific source active regardless of health, until switched back to `Auto`.
/// Native only in effect (the manager itself doesn't build on wasm32), but kept a plain settings
/// enum rather than `#[cfg]`-gated so a settings file round-trips identically on every platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RadarProviderOverride {
    #[default]
    Auto,
    /// Force the Unidata/AWS chunk feed.
    Primary,
    /// Force the HookEcho relay (only meaningful with `radar_relay_url` set).
    Backup,
    /// Force the NOAA TGFTP completed-volume fallback.
    Degraded,
}

impl RadarProviderOverride {
    pub const ALL: [RadarProviderOverride; 4] = [
        RadarProviderOverride::Auto,
        RadarProviderOverride::Primary,
        RadarProviderOverride::Backup,
        RadarProviderOverride::Degraded,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RadarProviderOverride::Auto => "Auto",
            RadarProviderOverride::Primary => "Unidata (primary)",
            RadarProviderOverride::Backup => "HookEcho relay (backup)",
            RadarProviderOverride::Degraded => "NOAA TGFTP (degraded)",
        }
    }
}

/// Display unit for velocity products. GRLevelX defaults to knots; internal math is m/s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VelocityUnit {
    #[default]
    Knots,
    MetersPerSecond,
    Mph,
}

impl VelocityUnit {
    pub const ALL: [VelocityUnit; 3] = [
        VelocityUnit::Knots,
        VelocityUnit::MetersPerSecond,
        VelocityUnit::Mph,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VelocityUnit::Knots => "kt",
            VelocityUnit::MetersPerSecond => "m/s",
            VelocityUnit::Mph => "mph",
        }
    }

    /// Factor to convert internal m/s into this unit.
    pub fn factor_from_ms(self) -> f32 {
        match self {
            VelocityUnit::Knots => 1.943_844,
            VelocityUnit::MetersPerSecond => 1.0,
            VelocityUnit::Mph => 2.236_936,
        }
    }
}

fn default_goes_rgb_recipe() -> String {
    wxdata::goes_rgb::AIR_MASS.slug.to_string()
}

fn default_local_api_port() -> u16 {
    47_914
}

impl Volume3dPreset {
    /// The preset's curve: its exact stops when it has them, else the four-point curve.
    pub fn tf(&self) -> Option<crate::render3d::TfStops> {
        match &self.stops {
            Some(s) => crate::render3d::TfStops::new(s),
            None => self.curve.map(Into::into),
        }
    }

    /// Store `tf` so both this build and older ones read it (see `curve` and `stops`).
    pub fn set_tf(&mut self, tf: Option<crate::render3d::TfStops>) {
        self.curve = tf.map(|t| t.as_four());
        self.stops = tf.filter(|t| t.len() != 4).map(|t| t.points().to_vec());
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_site: "KTLX".to_string(),
            web_files: BTreeMap::new(),
            gis_layers: Vec::new(),
            gis_groups: Vec::new(),
            gis_next_id: 0,
            imported_gis: None,
            imported_gis_style: ImportedGisStyle::default(),
            imported_gis_label: None,
            imported_gis_color_by: None,
            imported_gis_time_start: None,
            imported_gis_time_end: None,
            imported_gis_below: false,
            detectors: DetectorTuning::default(),
            alert_rules: Vec::new(),
            serve_token: String::new(),
            custom_tile_url: String::new(),
            custom_tile_max_z: default_custom_tile_max_z(),
            custom_tile_attribution: String::new(),
            quiet_pending: Vec::new(),
            volume_cache_mb: 0,
            tile_disk_cache_mb: 0,
            share_card: true,
            loop_real_timing: true,
            loop_mp4_fps: default_mp4_fps(),
            wind_particle_pct: default_particle_pct(),
            blend_frames: false,
            field_crossfade: false,
            output_window: OutputPrefs::default(),
            broadcast: Default::default(),
            scenes: Vec::new(),
            layer_order: Vec::new(),
            recent_layers: Vec::new(),
            favorite_layers: Vec::new(),
            mping_key: String::new(),
            etop_dbz: default_etop_dbz(),
            poll_interval_secs: 30,
            // The dock's own look: it is the default layout.
            theme: Theme::DearImGui,
            layout: Layout::default(),
            workstation: Default::default(),
            tablet_layout_adopted: false,
            dock_adopted: true,
            station_adopted: false,
            local_api: false,
            local_api_port: default_local_api_port(),
            phone_design: PhoneDesign::default(),
            accent: None,
            reduce_motion: false,
            hide_far_3d: true,
            far_3d_factor: default_far_3d(),
            presets: Vec::new(),
            palettes: BTreeMap::new(),
            velocity_unit: VelocityUnit::default(),
            temp_unit: TempUnit::default(),
            time_display: TimeDisplay::default(),
            time_mismatch_minutes: default_time_mismatch_minutes(),
            radar_stale_minutes: default_radar_stale_minutes(),
            check_builds: true,
            // 1.0 everywhere: this multiplies the native scale factor, and Android's display
            // density already sizes widgets for touch — an extra 1.3 shrank the S24's logical
            // canvas to ~277 pt wide (nothing fit).
            ui_scale: 1.0,
            placefiles: Vec::new(),
            markers: Vec::new(),
            udp_products: Vec::new(),
            outline_thresholds: Default::default(),
            precip_tint: false,
            dealias_velocity: false,
            alert_spotlight: true,
            yall_mode: false,
            layer_probe: false,
            globe: false,
            scale_bar: true,
            volume3d_presets: Vec::new(),
            radar_relay_url: String::new(),
            radar_provider_override: RadarProviderOverride::default(),
            mapbox_key: String::new(),
            maptiler_key: String::new(),
            tempest_token: String::new(),
            wu_key: String::new(),
            synoptic_token: String::new(),
            field_opacity: Default::default(),
            field_order: Vec::new(),
            compare_view: None,
            airnow_key: String::new(),
            windy_key: String::new(),
            field_mill_url: String::new(),
            start_view: None,
            sync_client_id: String::new(),
            sync_client_secret: String::new(),
            sync_enabled: false,
            share_position: false,
            share_name: String::new(),
            share_relay: String::new(),
            share_video_url: String::new(),
            lightning_minutes: default_lightning_minutes(),
            glm_goes_west: false,
            goes_satellite_west: false,
            goes_rgb_recipe: default_goes_rgb_recipe(),
            goes_sector: Default::default(),
            spotter_range_km: default_spotter_range_km(),
            alert_sound: true,
            smooth_radar: true,
            live_scan_indicator: true,
            live_sweep_mode: LiveSweepMode::default(),
            ntfy_topic: String::new(),
            discord_webhook: String::new(),
            slack_webhook: String::new(),
            matrix_homeserver: String::new(),
            matrix_room: String::new(),
            matrix_token: String::new(),
            mqtt_host: String::new(),
            mqtt_port: default_mqtt_port(),
            mqtt_tls: false,
            mqtt_user: String::new(),
            mqtt_pass: String::new(),
            mqtt_prefix: default_mqtt_prefix(),
            strikes_topic: String::new(),
            warnings_topic: String::new(),
            mqtt_discovery: false,
            background_alerts: false,
            pack_hires_dem: false,
            pack_include_vector: true,
            pack_include_satellite: false,
            alert_polygons: Vec::new(),
            plugins: Vec::new(),
            close_to_tray: false,
            bookmarks: Vec::new(),
            anthropic_key: String::new(),
            gemini_key: String::new(),
            ai_provider: Default::default(),
            lightning_alarm: false,
            speak_warnings: true,
            piper_path: String::new(),
            piper_voice: String::new(),
            piper_download_voice: String::new(),
            speak_position: false,
            rain_alerts: false,
            rain_sound: AlertSound::default(),
            hints_seen: Vec::new(),
            setup_done: false,
            desktop_notify: false,
            chase_log: false,
            route_engine: Default::default(),
            route_url: String::new(),
            battery_saver: false,
            wind_barbs: false,
            wind_streamlines: false,
            ntfy_snapshot: false,
            alert_follow_gps: false,
            gps_autoconnect: false,
            quiet_hours: false,
            quiet_start_hour: default_quiet_start(),
            quiet_end_hour: default_quiet_end(),
            alert_min_escalation: 0,
            alert_rollup_threshold: default_alert_rollup_threshold(),
            alert_rollup_window_min: default_alert_rollup_window_min(),
            scan_chime: false,
            scan_sound: default_scan_sound(),
            warn_sound: AlertSound::default(),
            tds_sound: AlertSound::default(),
            rotation_sound: default_rotation_sound(),
            lightning_sound: AlertSound::default(),
            emergency_sound: default_emergency_sound(),
            alert_volume: default_volume(),
            live_loop_frames: default_live_loop_frames(),
            timeline_style: TimelineStyle::default(),
            floating_search_button: false,
            analyst_mode: false,
            basemap: String::new(),
            overlays_on: None,
            contours_on: Vec::new(),
            window: None,
            workspaces: Vec::new(),
            seeded_workspaces: false,
            offered_starters: Vec::new(),
            model_pick: String::new(),
            last_view: None,
            nwr_streams: Vec::new(),
            mute_alerts: false,
            keybinds: Vec::new(),
        }
    }
}

impl Settings {
    /// Where settings.json lives. Web has no filesystem — it persists to `localStorage` instead.
    #[cfg(not(target_arch = "wasm32"))]
    fn path() -> Option<PathBuf> {
        crate::paths::config_dir().map(|d| d.join("settings.json"))
    }

    /// The auto-scanned color-tables folder (`<data_dir>/colortables`). Created on first use.
    pub fn colortables_dir() -> Option<PathBuf> {
        let dir = crate::paths::data_dir()?.join("colortables");
        let _ = std::fs::create_dir_all(&dir);
        Some(dir)
    }

    /// Folder holding uploaded marker icons (`<data_dir>/marker-icons`). Created on first use.
    pub fn marker_icons_dir() -> Option<PathBuf> {
        let dir = crate::paths::data_dir()?.join("marker-icons");
        let _ = std::fs::create_dir_all(&dir);
        Some(dir)
    }

    /// Resolve the per-moment `.pal` override paths (`None` = built-in default), indexed by
    /// [`wxdata::level2::Moment::index`].
    pub fn palette_paths(&self) -> [Option<PathBuf>; wxdata::level2::Moment::ALL.len()] {
        use wxdata::level2::Moment;
        Moment::ALL.map(|m| {
            let mut p = self.palettes.get(m.short_name());
            if p.is_none() && m == Moment::CorrelationCoefficient {
                p = self.palettes.get("RHO"); // legacy key, pre-CC rename
            }
            // A name that is in `web_files` names content, not a file. Resolved here rather than
            // in the loader so the loader keeps taking one kind of thing.
            p.map(|v| match self.web_files.get(v) {
                Some(text) => PathBuf::from(format!("{}{text}", crate::colormap::INLINE_PREFIX)),
                None => PathBuf::from(v),
            })
        })
    }

    /// A GIS layer's remembered content, or why it couldn't be read: a `web_files` name holds
    /// its own content, anything else is a path to read.
    pub fn gis_source_text(&self, source: &str) -> Result<String, String> {
        match self.web_files.get(source) {
            Some(text) => Ok(text.clone()),
            None => std::fs::read_to_string(source).map_err(|e| e.to_string()),
        }
    }

    /// Move the pre-M4.1 single imported layer into the collection, once: the legacy fields are
    /// cleared (and never written again), so a second call, or a later launch, changes nothing.
    /// Returns whether anything moved.
    pub fn migrate_imported_gis(&mut self) -> bool {
        let Some(source) = self.imported_gis.take() else {
            return false;
        };
        let legacy = GisLayerConfig {
            style: std::mem::take(&mut self.imported_gis_style),
            label: self.imported_gis_label.take(),
            color_by: self.imported_gis_color_by.take(),
            time_start: self.imported_gis_time_start.take(),
            time_end: self.imported_gis_time_end.take(),
            below: std::mem::take(&mut self.imported_gis_below),
            ..Default::default()
        };
        if self.gis_layers.iter().any(|l| l.source == source) {
            return true;
        }
        let id = self.add_gis_layer(source);
        if let Some(l) = self.gis_layer_mut(id) {
            *l = GisLayerConfig {
                id: l.id,
                source: std::mem::take(&mut l.source),
                name: std::mem::take(&mut l.name),
                ..legacy
            };
        }
        true
    }

    /// Add a layer for `source` (drawn above the others) and return its new ID.
    pub fn add_gis_layer(&mut self, source: String) -> u64 {
        let id = self
            .gis_layers
            .iter()
            .map(|l| l.id + 1)
            .max()
            .unwrap_or(1)
            .max(self.gis_next_id)
            .max(1);
        self.gis_next_id = id + 1;
        let name = source
            .rsplit(['/', '\\'])
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(&source)
            .to_string();
        self.gis_layers.push(GisLayerConfig {
            id,
            source,
            name,
            ..Default::default()
        });
        id
    }

    pub fn gis_layer(&self, id: u64) -> Option<&GisLayerConfig> {
        self.gis_layers.iter().find(|l| l.id == id)
    }

    pub fn gis_layer_mut(&mut self, id: u64) -> Option<&mut GisLayerConfig> {
        self.gis_layers.iter_mut().find(|l| l.id == id)
    }

    /// Whether a layer is to be drawn: its own visibility and its group's. A group that no
    /// longer exists hides nothing.
    pub fn gis_layer_shown(&self, id: u64) -> bool {
        self.gis_layer(id).is_some_and(|l| {
            l.visible
                && l.group.as_deref().is_none_or(|g| {
                    self.gis_groups
                        .iter()
                        .find(|x| x.name == g)
                        .is_none_or(|x| x.visible)
                })
        })
    }

    /// Remove a layer, and the browser-stored content it alone used.
    pub fn remove_gis_layer(&mut self, id: u64) -> Option<GisLayerConfig> {
        let i = self.gis_layers.iter().position(|l| l.id == id)?;
        let gone = self.gis_layers.remove(i);
        if !self.gis_layers.iter().any(|l| l.source == gone.source) {
            self.web_files.remove(&gone.source);
        }
        Some(gone)
    }

    /// The layers as they are now, for a workspace or scene; `None` with no layers, so a saved
    /// view that never had any leaves later imports alone.
    pub fn gis_snapshot(&self, shown: bool) -> Option<GisSnapshot> {
        (!self.gis_layers.is_empty()).then(|| GisSnapshot {
            shown,
            layers: self.gis_layers.clone(),
            groups: self.gis_groups.clone(),
        })
    }

    /// Put the layers back as `snap` saw them: each layer it names by ID (and the same source)
    /// takes its saved state and order; a layer imported since is hidden, since it was not part
    /// of the view; group switches are restored. Returns the layers it names that are no longer
    /// imported (`name (source)`), which are reported rather than bound to a similar layer.
    pub fn apply_gis_snapshot(&mut self, snap: &GisSnapshot) -> Vec<String> {
        let same = |a: &GisLayerConfig, b: &GisLayerConfig| a.id == b.id && a.source == b.source;
        let missing = snap
            .layers
            .iter()
            .filter(|s| !self.gis_layers.iter().any(|l| same(l, s)))
            .map(|s| format!("{} ({})", s.name, s.source))
            .collect();
        let mut ordered: Vec<GisLayerConfig> = snap
            .layers
            .iter()
            .filter(|s| self.gis_layers.iter().any(|l| same(l, s)))
            .cloned()
            .collect();
        for l in &self.gis_layers {
            if !snap.layers.iter().any(|s| same(l, s)) {
                ordered.push(GisLayerConfig {
                    visible: false,
                    ..l.clone()
                });
            }
        }
        self.gis_layers = ordered;
        for g in &snap.groups {
            match self.gis_groups.iter_mut().find(|x| x.name == g.name) {
                Some(x) => x.visible = g.visible,
                None => self.gis_groups.push(g.clone()),
            }
        }
        missing
    }

    /// Move a layer `delta` places in paint order (positive: drawn later, on top).
    pub fn move_gis_layer(&mut self, id: u64, delta: isize) {
        let Some(i) = self.gis_layers.iter().position(|l| l.id == id) else {
            return;
        };
        let j = (i as isize + delta).clamp(0, self.gis_layers.len() as isize - 1) as usize;
        let l = self.gis_layers.remove(i);
        self.gis_layers.insert(j, l);
    }

    /// Is local `hour` inside the quiet-hours window? Handles the ordinary case of a window that
    /// crosses midnight (22 → 7). Start == end means "no window", not "all day" — a 24-hour mute
    /// is what `mute_alerts` is for, and reading it the other way would silence someone by
    /// accident.
    pub fn in_quiet_hours(&self, hour: u32) -> bool {
        if !self.quiet_hours {
            return false;
        }
        let (s, e, h) = (
            self.quiet_start_hour % 24,
            self.quiet_end_hour % 24,
            hour % 24,
        );
        match s.cmp(&e) {
            std::cmp::Ordering::Equal => false,
            std::cmp::Ordering::Less => (s..e).contains(&h),
            std::cmp::Ordering::Greater => h >= s || h < e,
        }
    }

    /// The saved settings JSON, or `None` if there is none to read.
    #[cfg(not(target_arch = "wasm32"))]
    fn read_saved() -> Option<String> {
        std::fs::read_to_string(Self::path()?).ok()
    }

    /// Web: the same JSON, out of `localStorage` (no filesystem to read).
    #[cfg(target_arch = "wasm32")]
    fn read_saved() -> Option<String> {
        local_storage()?.get_item(WEB_KEY).ok()?
    }

    /// Load saved settings, falling back to defaults on any error (nothing saved, parse failure).
    /// Parse a settings file, keeping every field that reads and defaulting only the ones that
    /// do not.
    ///
    /// This used to be a plain `serde_json::from_str().unwrap_or_default()`. Every field carries
    /// `#[serde(default)]`, so a *missing* key was always fine — but one bad *value* anywhere in
    /// the file failed the whole struct, and the whole file was thrown away: every marker, every
    /// alert rule, every API key, replaced by defaults, and then written back over the file by the
    /// next `save()`. A settings file whose `theme` said `"light"` instead of `"Light"` is enough
    /// to do it, which is how this was found.
    ///
    /// The repair is per-key: rebuild the object one key at a time and drop any key that stops it
    /// parsing. That is one full deserialize per key, but only on a file that already failed, and
    /// only at startup.
    ///
    /// ponytail: quadratic in the number of keys on the error path. It runs once, on a file that
    /// is already broken.
    fn from_json_lossy(text: &str) -> Self {
        match serde_json::from_str::<Self>(text) {
            Ok(v) => return v,
            Err(e) => log::warn!("settings parse failed ({e}); salvaging what reads"),
        }
        let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(text)
        else {
            log::warn!("settings file is not a JSON object; using defaults");
            return Self::default();
        };
        let mut good = serde_json::Map::new();
        for (k, v) in map {
            good.insert(k.clone(), v);
            if serde_json::from_value::<Self>(serde_json::Value::Object(good.clone())).is_err() {
                good.remove(&k);
                log::warn!("settings: ignoring unreadable value for `{k}`");
            }
        }
        serde_json::from_value(serde_json::Value::Object(good)).unwrap_or_default()
    }

    /// The saved workspaces, read without the repairs [`Settings::load`] writes back — for a
    /// headless command that only wants to look (`--watch --workspace`). The shipped starters
    /// when nothing has been saved yet.
    pub fn saved_workspaces() -> Vec<crate::workspace::Workspace> {
        Self::read_saved()
            .map(|s| Self::from_json_lossy(&s).workspaces)
            .unwrap_or_else(|| Self::default().workspaces)
    }

    pub fn load() -> Self {
        let mut loaded = match Self::read_saved() {
            Some(s) => Self::from_json_lossy(&s),
            None => Self::default(),
        };
        // One-shot repair: early Android builds persisted ui_scale 1.3 as the default, which
        // multiplied on top of display density and left a ~277-pt-wide canvas. A saved exact 1.3
        // on Android is that bug, not a choice — the slider steps land there for almost no one.
        if cfg!(target_os = "android") && (loaded.ui_scale - 1.3).abs() < 0.001 {
            loaded.ui_scale = 1.0;
        }
        loaded.adopt_tablet_default(cfg!(target_os = "android"));
        loaded.adopt_dock_default();
        loaded.adopt_detector_floors();
        // The pre-M4.1 single imported layer becomes the first of the collection, once.
        let migrated = loaded.migrate_imported_gis();
        // User products saved before IDs existed get theirs now, and keep them (M3.2).
        let migrated = wxdata::udp_file::ensure_ids(&mut loaded.udp_products) || migrated;
        // Saved key tables gain the plain-key alternatives, so a tablet keyboard without an F row
        // or Page keys can reach every action (see `hotkeys::fill_plain_keys`).
        crate::hotkeys::fill_plain_keys(&mut loaded.keybinds);
        // Markers saved before ids existed get one now, and keep it: written straight back so the
        // Android alert service reads the same identities this process does.
        let filled = loaded.markers.iter().any(|m| m.id.is_empty());
        for m in loaded.markers.iter_mut().filter(|m| m.id.is_empty()) {
            m.id = new_marker_id();
        }
        if filled || migrated {
            loaded.save();
        }
        loaded
    }

    /// Export a portable bundle: this Settings plus the *contents* of every referenced `.pal`
    /// file (inlined by moment short name), so it restores identically on another machine where
    /// the palette paths don't exist. Returns pretty JSON.
    pub fn export_bundle(&self) -> Result<String, String> {
        let mut palette_files = BTreeMap::new();
        for (moment, path) in &self.palettes {
            // A built-in alternate is compiled in on the other machine too — nothing to inline.
            if path.starts_with(crate::colormap::BUILTIN_PREFIX) {
                continue;
            }
            match std::fs::read_to_string(path) {
                Ok(text) => {
                    palette_files.insert(moment.clone(), text);
                }
                Err(e) => log::warn!("bundle: skipping palette {moment} ({path}): {e}"),
            }
        }
        // Every imported layer's file, once each (several datasets can share one zip). A
        // browser's stored file already travels in `web_files`.
        let mut gis_files = BTreeMap::new();
        let mut gis_unpacked = Vec::new();
        for layer in &self.gis_layers {
            let file = gis_source_file(&layer.source);
            if self.web_files.contains_key(&layer.source)
                || gis_files.contains_key(file)
                || gis_unpacked.iter().any(|f: &String| f == file)
            {
                continue;
            }
            match std::fs::read(file) {
                Ok(bytes) => {
                    use base64::Engine as _;
                    use sha2::Digest as _;
                    let name = std::path::Path::new(file)
                        .file_name()
                        .map_or_else(|| "layer".into(), |n| n.to_string_lossy().into_owned());
                    gis_files.insert(
                        file.to_string(),
                        PackedFile {
                            name,
                            sha256: format!("{:x}", sha2::Sha256::digest(&bytes)),
                            base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
                        },
                    );
                }
                Err(e) => {
                    log::warn!("bundle: GIS layer file {file} not packaged: {e}");
                    gis_unpacked.push(file.to_string());
                }
            }
        }
        let bundle = SettingsBundle {
            settings: self.clone(),
            palette_files,
            gis_files,
            gis_unpacked,
        };
        serde_json::to_string_pretty(&bundle).map_err(|e| e.to_string())
    }

    /// Import a bundle produced by [`export_bundle`]: writes each inlined `.pal` into the local
    /// colortables dir and rewrites the palette paths to point there, so the imported palettes
    /// resolve locally. Returns the ready-to-use Settings (caller assigns + saves).
    pub fn import_bundle(json: &str) -> Result<Settings, String> {
        Self::import_bundle_report(json).map(|(s, _)| s)
    }

    /// [`Self::import_bundle`] with what the caller should tell the person: GIS layer files the
    /// bundle could not carry, so those layers will not load here.
    pub fn import_bundle_report(json: &str) -> Result<(Settings, Vec<String>), String> {
        Self::import_bundle_with_dir(json, Self::colortables_dir, Self::gis_files_dir)
    }

    /// Folder for GIS layer files unpacked from a bundle (`<data_dir>/gis-imports`).
    pub fn gis_files_dir() -> Option<PathBuf> {
        let dir = crate::paths::data_dir()?.join("gis-imports");
        let _ = std::fs::create_dir_all(&dir);
        Some(dir)
    }

    fn import_bundle_with_dir(
        json: &str,
        palette_dir: impl FnOnce() -> Option<PathBuf>,
        gis_dir: impl FnOnce() -> Option<PathBuf>,
    ) -> Result<(Settings, Vec<String>), String> {
        let bundle: SettingsBundle = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut settings = bundle.settings;
        let mut notes: Vec<String> = bundle
            .gis_unpacked
            .iter()
            .map(|f| format!("{f} was not in the bundle; its layers will not load"))
            .collect();
        if !bundle.gis_files.is_empty() {
            use base64::Engine as _;
            use sha2::Digest as _;
            let dir = gis_dir().ok_or("no folder for GIS layer files")?;
            // Every file is checked before anything is written: a damaged bundle changes nothing.
            let mut decoded = Vec::new();
            for (original, packed) in &bundle.gis_files {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(&packed.base64)
                    .map_err(|e| format!("GIS layer file {}: {e}", packed.name))?;
                let sum = format!("{:x}", sha2::Sha256::digest(&bytes));
                if sum != packed.sha256 {
                    return Err(format!(
                        "GIS layer file {} does not match its checksum; nothing was imported",
                        packed.name
                    ));
                }
                decoded.push((original, packed, bytes));
            }
            for (original, packed, bytes) in decoded {
                // The packed name, or "name-2.ext" and on when another file already has it.
                let stem = std::path::Path::new(&packed.name)
                    .file_stem()
                    .map_or_else(|| "layer".into(), |s| s.to_string_lossy().into_owned());
                let ext = std::path::Path::new(&packed.name)
                    .extension()
                    .map(|e| format!(".{}", e.to_string_lossy()))
                    .unwrap_or_default();
                let mut path = dir.join(&packed.name);
                let mut n = 2;
                while path.exists() && std::fs::read(&path).ok().as_deref() != Some(&bytes[..]) {
                    path = dir.join(format!("{stem}-{n}{ext}"));
                    n += 1;
                }
                std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
                let local = path.to_string_lossy().into_owned();
                for layer in &mut settings.gis_layers {
                    if gis_source_file(&layer.source) == original.as_str() {
                        layer.source = match crate::gis_import::split_dataset(&layer.source).1 {
                            Some(dataset) => format!("{local}#{dataset}"),
                            None => local.clone(),
                        };
                    }
                }
            }
        }
        notes.dedup();
        if !bundle.palette_files.is_empty() {
            let dir = palette_dir().ok_or("no colortables dir")?;
            for (moment, text) in &bundle.palette_files {
                let path = dir.join(format!("{moment}.pal"));
                std::fs::write(&path, text).map_err(|e| e.to_string())?;
                settings
                    .palettes
                    .insert(moment.clone(), path.to_string_lossy().into_owned());
            }
        }
        Ok((settings, notes))
    }

    /// Persist `json`: a settings.json on native, a `localStorage` entry on the web.
    #[cfg(not(target_arch = "wasm32"))]
    fn write_saved(json: &str) {
        let Some(path) = Self::path() else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = atomic_write(&path, json.as_bytes()) {
            log::warn!("settings save failed: {e}");
        }
    }

    /// Web: one `localStorage` key holding the same JSON. Quota/private-mode failures are logged.
    ///
    // ponytail: settings only. Caches and palette files stay in memory on the web; IndexedDB is the
    // upgrade path if offline-web ever becomes real.
    #[cfg(target_arch = "wasm32")]
    fn write_saved(json: &str) {
        let Some(store) = local_storage() else { return };
        if let Err(e) = store.set_item(WEB_KEY, json) {
            log::warn!("settings save failed: {e:?}");
        }
    }

    /// Write out, logging (not failing) on error.
    pub fn save(&self) {
        match serde_json::to_string_pretty(self) {
            Ok(json) => Self::write_saved(&json),
            Err(e) => log::warn!("settings serialize failed: {e}"),
        }
    }
}

/// `localStorage` key holding the serialized [`Settings`] on the web.
#[cfg(target_arch = "wasm32")]
const WEB_KEY: &str = "hookecho-settings";

/// The page's `localStorage`, or `None` where the browser denies it (private mode, no window).
#[cfg(target_arch = "wasm32")]
fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// Write via a sibling temp file + rename. A crash mid-write must never tear a config file:
/// a torn settings.json loses every setting on the next launch, and on Android the Kotlin
/// alert service reads this file while we write it.
pub fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    replace_file(&tmp, path)
}

#[cfg(not(target_os = "windows"))]
fn replace_file(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

#[cfg(target_os = "windows")]
fn replace_file(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // `std::fs::rename` does not replace an existing file on Windows. Settings are rewritten
    // throughout a session, so the first save worked and every later one quietly failed.
    let ok = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_gate_starts_on_and_an_explicit_off_stays_off() {
        assert!(DetectorTuning::default().environment_gate);
        let mut v = serde_json::to_value(DetectorTuning::default()).unwrap();
        v.as_object_mut().unwrap().remove("environment_gate");
        let t: DetectorTuning = serde_json::from_value(v.clone()).unwrap();
        assert!(t.environment_gate, "an older file gets it on");
        v["environment_gate"] = serde_json::Value::Bool(false);
        let t: DetectorTuning = serde_json::from_value(v).unwrap();
        assert!(!t.environment_gate);
    }

    #[test]
    fn rotation_only_possible_starts_on_and_an_explicit_off_stays_off() {
        assert_eq!(
            DetectorTuning::default().rotation_only_possible,
            Some(DEFAULT_ROTATION_ONLY_POSSIBLE)
        );
        let mut v = serde_json::to_value(DetectorTuning::default()).unwrap();
        // A file from before the setting: the new default.
        v.as_object_mut().unwrap().remove("rotation_only_possible");
        let t: DetectorTuning = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(t.rotation_only_possible, Some(0.018));
        // Someone who turned it off: off.
        v["rotation_only_possible"] = serde_json::Value::Null;
        let t: DetectorTuning = serde_json::from_value(v).unwrap();
        assert_eq!(t.rotation_only_possible, None);
    }

    /// Nothing imported must stay silent — this runs on every launch, and an empty setting is the
    /// ordinary case, not an error worth reporting.
    #[test]
    fn no_remembered_gis_import_resolves_to_nothing_at_all() {
        let mut s = Settings::default();
        assert!(!s.migrate_imported_gis());
        assert!(s.gis_layers.is_empty());
    }

    /// A pre-M4.1 settings file's one layer, with every companion setting, becomes the first layer
    /// of the collection exactly once, and the old fields are not written back.
    #[test]
    fn the_single_imported_layer_migrates_once_with_all_its_settings() {
        let old = r#"{"imported_gis":"districts.geojson",
            "imported_gis_style":{"color":[240,80,40],"stroke_width":3.25,"opacity":0.5,"min_zoom":6.5},
            "imported_gis_label":"NAME","imported_gis_color_by":"POP",
            "imported_gis_time_start":"BEGIN","imported_gis_below":true}"#;
        let mut s: Settings = serde_json::from_str(old).expect("old settings still deserialize");
        assert!(s.migrate_imported_gis());
        assert_eq!(s.gis_layers.len(), 1);
        let l = &s.gis_layers[0];
        assert_eq!(
            (l.id, l.source.as_str(), l.name.as_str()),
            (1, "districts.geojson", "districts.geojson")
        );
        assert_eq!(l.style.color, [240, 80, 40]);
        assert_eq!(l.style.min_zoom, 6.5);
        assert_eq!(l.label.as_deref(), Some("NAME"));
        assert_eq!(l.color_by.as_deref(), Some("POP"));
        assert_eq!(l.time_start.as_deref(), Some("BEGIN"));
        assert_eq!(l.time_end, None);
        assert!(l.below && l.visible);
        assert!(!s.migrate_imported_gis(), "once");
        let saved = serde_json::to_string(&s).unwrap();
        assert!(!saved.contains("imported_gis"), "{saved}");
        let back: Settings = serde_json::from_str(&saved).unwrap();
        assert_eq!(back.gis_layers, s.gis_layers);
        let mut back = back;
        assert!(
            !back.migrate_imported_gis(),
            "a later launch migrates nothing"
        );
        // An old file with only the path gets the original neutral-blue look.
        let mut plain: Settings = serde_json::from_str(r#"{"imported_gis":"a.geojson"}"#).unwrap();
        plain.migrate_imported_gis();
        let st = plain.gis_layers[0].style;
        assert_eq!(st, ImportedGisStyle::default());
        assert_eq!(st.fill_rgba(), [80, 140, 220, 60]);
        assert_eq!(st.stroke_rgba(), [80, 140, 220, 220]);
    }

    /// Layers are independent: IDs are never reused, order and groups are per layer, a hidden
    /// group hides without overwriting its layers' own visibility, and removing the last layer
    /// that used browser-stored content removes the content.
    #[test]
    fn gis_layers_keep_their_own_ids_order_groups_and_content() {
        let mut s = Settings::default();
        let a = s.add_gis_layer("/data/counties.shp".into());
        let b = s.add_gis_layer("sites.geojson".into());
        s.web_files.insert("sites.geojson".into(), "{}".into());
        assert_eq!((a, b), (1, 2));
        assert_eq!(s.gis_layer(a).unwrap().name, "counties.shp");
        s.move_gis_layer(b, -1);
        assert_eq!(s.gis_layers[0].id, b, "sites now drawn first, underneath");
        s.move_gis_layer(b, -5);
        assert_eq!(s.gis_layers[0].id, b, "clamped");
        s.gis_layer_mut(a).unwrap().group = Some("Assets".into());
        s.gis_groups.push(GisGroup {
            name: "Assets".into(),
            visible: false,
        });
        assert!(!s.gis_layer_shown(a) && s.gis_layer(a).unwrap().visible);
        assert!(s.gis_layer_shown(b));
        s.gis_groups[0].visible = true;
        assert!(s.gis_layer_shown(a));
        s.remove_gis_layer(b);
        assert!(!s.web_files.contains_key("sites.geojson"));
        let c = s.add_gis_layer("sites.geojson".into());
        assert_eq!(c, 3, "a removed layer's ID is not handed out again");
        s.remove_gis_layer(a);
        s.remove_gis_layer(c);
        assert_eq!(s.add_gis_layer("x".into()), 4);
    }

    #[test]
    fn imported_gis_opacity_is_bounded_at_the_render_boundary() {
        let too_high = ImportedGisStyle {
            opacity: 2.0,
            ..Default::default()
        };
        let too_low = ImportedGisStyle {
            opacity: -1.0,
            ..Default::default()
        };
        assert_eq!(too_high.stroke_rgba()[3], 220);
        assert_eq!(too_low.fill_rgba()[3], 0);
    }

    #[test]
    fn imported_gis_stroke_width_is_bounded_at_the_render_boundary() {
        let too_wide = ImportedGisStyle {
            stroke_width: 99.0,
            ..Default::default()
        };
        let too_thin = ImportedGisStyle {
            stroke_width: 0.0,
            ..Default::default()
        };
        assert_eq!(too_wide.rendered_stroke_width(), 8.0);
        assert_eq!(too_thin.rendered_stroke_width(), 0.5);
    }

    #[test]
    fn imported_gis_style_extras_default_to_the_shipped_look() {
        // A style saved before fills, dashes, symbols and a maximum zoom existed reads back
        // drawing exactly as it did.
        let old: ImportedGisStyle = serde_json::from_str(
            r#"{"color":[240,80,40],"stroke_width":1.6,"opacity":0.5,"min_zoom":0.0}"#,
        )
        .unwrap();
        assert_eq!(old.fill_rgba(), [240, 80, 40, 30]);
        assert_eq!(old.stroke_rgba(), [240, 80, 40, 110]);
        assert_eq!(old.dash, LineDash::Solid);
        assert_eq!(old.dash.pattern(1.6), None);
        assert_eq!(old.symbol, PointSymbol::Circle);
        assert_eq!(old.point_radius(), 3.5);
        assert!(old.visible_at(18.0));

        // A fill of its own, half shown; an attribute colour then goes to the fill only.
        let own = ImportedGisStyle {
            fill_color: Some([10, 20, 30]),
            fill_opacity: 0.5,
            ..old
        };
        assert_eq!(own.fill_rgba(), [10, 20, 30, 15]);
        assert_eq!(own.stroke_rgba(), [240, 80, 40, 110]);
        let by = own.colored([1, 2, 3]);
        assert_eq!((by.color, by.fill_color), ([240, 80, 40], Some([1, 2, 3])));
        let by = old.colored([1, 2, 3]);
        assert_eq!((by.color, by.fill_color), ([1, 2, 3], None));
        // No fill at all.
        assert_eq!(
            ImportedGisStyle {
                fill_opacity: 0.0,
                ..old
            }
            .fill_rgba()[3],
            0
        );

        // A zoom range.
        let ranged = ImportedGisStyle {
            min_zoom: 5.0,
            max_zoom: 9.0,
            ..old
        };
        assert!(!ranged.visible_at(4.5));
        assert!(ranged.visible_at(9.0));
        assert!(!ranged.visible_at(9.5));

        // A chosen point size, bounded.
        assert_eq!(
            ImportedGisStyle {
                point_size: 8.0,
                ..old
            }
            .point_radius(),
            8.0
        );
        assert_eq!(
            ImportedGisStyle {
                point_size: 99.0,
                ..old
            }
            .point_radius(),
            24.0
        );
        // Dots are slivers (drawn round), dashes longer than their gaps.
        let (on, off) = LineDash::Dotted.pattern(2.0).unwrap();
        assert!(on <= 0.5 && off > 2.0);
        let (on, off) = LineDash::Dashed.pattern(2.0).unwrap();
        assert!(on > off);
    }

    /// A workspace or scene puts the layers back by ID: their state and order as saved, a layer
    /// imported since hidden, group switches restored, and a layer since removed reported by name
    /// rather than bound to another layer, even one re-imported from the same file.
    #[test]
    fn a_gis_snapshot_restores_layers_by_id_and_reports_the_removed() {
        let mut s = Settings::default();
        assert!(
            s.gis_snapshot(true).is_none(),
            "nothing imported, nothing saved"
        );
        let a = s.add_gis_layer("counties.geojson".into());
        let b = s.add_gis_layer("sirens.geojson".into());
        s.gis_layer_mut(a).unwrap().group = Some("Base".into());
        s.gis_groups.push(GisGroup {
            name: "Base".into(),
            visible: true,
        });
        s.gis_layer_mut(b).unwrap().style.color = [255, 0, 0];
        let snap = s.gis_snapshot(true).unwrap();
        // Afterwards: reordered, restyled, a group hidden, a new layer imported.
        s.move_gis_layer(b, -1);
        s.gis_layer_mut(b).unwrap().style.color = [0, 0, 255];
        s.gis_layer_mut(a).unwrap().visible = false;
        s.gis_groups[0].visible = false;
        let c = s.add_gis_layer("roads.geojson".into());
        assert!(s.apply_gis_snapshot(&snap).is_empty());
        let ids: Vec<u64> = s.gis_layers.iter().map(|l| l.id).collect();
        assert_eq!(ids, [a, b, c]);
        assert_eq!(s.gis_layer(b).unwrap().style.color, [255, 0, 0]);
        assert!(s.gis_layer_shown(a) && s.gis_layer_shown(b));
        assert!(
            !s.gis_layer(c).unwrap().visible,
            "not part of the saved view"
        );
        // Sirens removed and imported again: a new layer, not the saved one.
        s.remove_gis_layer(b);
        let b2 = s.add_gis_layer("sirens.geojson".into());
        let missing = s.apply_gis_snapshot(&snap);
        assert_eq!(missing, ["sirens.geojson (sirens.geojson)"]);
        assert!(!s.gis_layer(b2).unwrap().visible);
        // Saved views from before this carry no snapshot and leave the layers alone.
        let old_scene: crate::broadcast::Scene =
            serde_json::from_str(r#"{"name":"Old","lon":-97.0,"lat":35.0,"zoom":7.0}"#).unwrap();
        assert!(old_scene.gis.is_none());
        let round: crate::broadcast::Scene = serde_json::from_str(
            &serde_json::to_string(&crate::broadcast::Scene {
                gis: Some(snap.clone()),
                ..old_scene
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(round.gis, Some(snap));
    }

    /// The browser has no path that survives a reload, so a name that matches a `web_files` entry
    /// resolves to that content — the same two-way resolution `palette_paths` does for a `.pal`.
    #[test]
    fn a_web_files_name_resolves_to_its_stored_content() {
        let mut s = Settings::default();
        s.web_files
            .insert("districts.geojson".into(), "{\"type\":\"x\"}".into());
        assert_eq!(
            s.gis_source_text("districts.geojson"),
            Ok("{\"type\":\"x\"}".to_string())
        );
    }

    /// A path that no longer resolves has to come back as an error, not as silence: the layer
    /// being missing is otherwise indistinguishable from the app having forgotten the import.
    #[test]
    fn a_missing_path_is_an_error_rather_than_silence() {
        let s = Settings::default();
        let path = std::env::temp_dir()
            .join("hookecho-no-such-import.geojson")
            .to_string_lossy()
            .into_owned();
        assert!(s.gis_source_text(&path).is_err());
    }

    #[test]
    fn atomic_write_replaces_an_existing_file() {
        let dir = std::env::temp_dir().join(format!(
            "hookecho-atomic-write-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");

        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rules_are_absent_from_old_settings_and_start_disabled() {
        let old: Settings = serde_json::from_str(r#"{"default_site":"KTLX"}"#).unwrap();
        assert!(old.alert_rules.is_empty());

        let r = AlertRule::new(RuleTrigger::Rotation);
        assert!(!r.enabled, "a new rule must not start armed");
        assert_eq!(r.cooldown_min, 10);
        assert_eq!(r.threshold, Some(40.0));
        assert_eq!(r.title(), "Rotation");

        let mut s = Settings::default();
        s.alert_rules.push(r.clone());
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.alert_rules, vec![r]);
        // A trigger with no numeric meaning gets no threshold to be confused by.
        assert_eq!(AlertRule::new(RuleTrigger::Tds).threshold, None);
    }

    #[test]
    fn markers_get_distinct_ids_and_old_files_still_load() {
        // A settings file from before ids existed.
        let old: Settings = serde_json::from_str(
            r#"{"markers":[{"name":"Home","lat":35.3,"lon":-97.3},
                           {"name":"Home","lat":36.1,"lon":-96.9}]}"#,
        )
        .unwrap();
        assert_eq!(old.markers.len(), 2);
        assert!(old.markers.iter().all(|m| m.id.is_empty()));
        // Two places can share a name; ids are what tell them apart.
        let ids: Vec<String> = (0..64).map(|_| new_marker_id()).collect();
        assert!(ids.iter().all(|id| id.len() == 8));
        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len());
    }

    #[test]
    fn a_3d_preset_saved_before_translucent_rendering_loads_as_mip() {
        let legacy = r#"{"name":"Hail core","representation":"Smooth reflectivity",
            "floor":45.0,"ceiling":70.0,"curve":[[45,0.1],[55,0.4],[62,0.8],[70,1.0]]}"#;
        let p: Volume3dPreset = serde_json::from_str(legacy).unwrap();
        assert_eq!(p.render, None);
        assert_eq!(
            p.render.unwrap_or_default(),
            crate::render3d::VolumeRender::Mip
        );
        // A preset without a mode writes no field, so older builds read it unchanged.
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("render"), "{json}");
        let lit = Volume3dPreset {
            render: Some(crate::render3d::VolumeRender::TranslucentLit),
            ..p
        };
        assert_eq!(lit.stops, None);
        let back: Volume3dPreset =
            serde_json::from_str(&serde_json::to_string(&lit).unwrap()).unwrap();
        assert_eq!(back, lit);
    }

    #[test]
    fn a_preset_with_more_than_four_stops_stays_readable_by_older_builds() {
        let six = crate::render3d::TfStops::new(&[
            [40.0, 0.0],
            [45.0, 0.1],
            [55.0, 0.4],
            [60.0, 0.6],
            [62.0, 0.8],
            [70.0, 1.0],
        ])
        .unwrap();
        let mut p = Volume3dPreset {
            name: "Hail".into(),
            representation: "Smooth reflectivity".into(),
            floor: 40.0,
            ceiling: None,
            curve: None,
            stops: None,
            render: None,
            colors: None,
        };
        p.set_tf(Some(six));
        let json = serde_json::to_value(&p).unwrap();
        // An older build reads `curve` (four samples of it) and ignores `stops`.
        assert_eq!(json["curve"].as_array().unwrap().len(), 4);
        assert_eq!(json["stops"].as_array().unwrap().len(), 6);
        let back: Volume3dPreset = serde_json::from_value(json).unwrap();
        assert_eq!(back.tf(), Some(six), "this build reads the exact stops");
        // Four stops write only `curve`, exactly as before.
        let four: crate::render3d::TfStops =
            [[45.0, 0.1], [55.0, 0.4], [62.0, 0.8], [70.0, 1.0]].into();
        p.set_tf(Some(four));
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("stops"), "{json}");
        assert_eq!(
            serde_json::from_str::<Volume3dPreset>(&json).unwrap().tf(),
            Some(four)
        );
        p.set_tf(None);
        assert_eq!((p.curve, p.stops.clone(), p.tf()), (None, None, None));
    }

    #[test]
    fn quiet_pending_round_trips_and_defaults_empty() {
        let old: Settings = serde_json::from_str(r#"{"default_site":"KTLX"}"#).unwrap();
        assert!(old.quiet_pending.is_empty());
        let mut s = Settings::default();
        s.quiet_pending
            .push(("Severe Thunderstorm Warning".into(), "Cleveland Co.".into()));
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.quiet_pending, s.quiet_pending);
    }

    #[test]
    fn the_detector_floors_default_to_the_backtested_values_and_round_trip() {
        // A detector block written before the filters existed loads with the shipped defaults.
        let old: DetectorTuning = serde_json::from_str(
            r#"{"tbss_core_dbz":60.0,"zdr_min_db":1.0,"zdr_min_depth_km":1.0,
                "glm_fed_cell_deg":0.05,"glm_fed_window_min":15}"#,
        )
        .unwrap();
        assert_eq!(old.tds_min_confidence, DEFAULT_TDS_MIN_CONFIDENCE);
        assert_eq!(old.rotation_min_confidence, DEFAULT_ROTATION_MIN_CONFIDENCE);
        assert_eq!(
            DetectorTuning::default().tds_min_confidence,
            DEFAULT_TDS_MIN_CONFIDENCE
        );
        // A chosen threshold survives a save and reload.
        let mut s = Settings::default();
        s.detectors.tds_min_confidence = 0.7;
        s.detectors.rotation_min_confidence = 0.4;
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert!((back.detectors.tds_min_confidence - 0.7).abs() < 1e-6);
        assert!((back.detectors.rotation_min_confidence - 0.4).abs() < 1e-6);
    }

    #[test]
    fn tornado_id_defaults_to_the_fusion_and_the_original_stays_selectable() {
        // A settings file from before the switch loads with the fusion, the analyst preview off.
        let old: DetectorTuning = serde_json::from_str(
            r#"{"tbss_core_dbz":60.0,"zdr_min_db":1.0,"zdr_min_depth_km":1.0,
                "glm_fed_cell_deg":0.05,"glm_fed_window_min":15}"#,
        )
        .unwrap();
        assert_eq!(old.tornado_id_source, TornadoIdSource::Fusion);
        assert!(!old.llsd_preview);
        assert_eq!(
            DetectorTuning::default().tornado_id_source,
            TornadoIdSource::Fusion
        );
        // Choosing the original survives a save and reload, under a readable name.
        let mut s = Settings::default();
        let mut compare = s.clone();
        compare.detectors.tornado_id_source = TornadoIdSource::Compare;
        let text = serde_json::to_string(&compare).unwrap();
        assert!(text.contains(r#""tornado_id_source":"compare""#), "{text}");
        s.detectors.tornado_id_source = TornadoIdSource::Legacy;
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""tornado_id_source":"legacy""#), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.detectors.tornado_id_source, TornadoIdSource::Legacy);
    }

    /// A file saved while 0% was the default carries that 0 explicitly; it moves once, and only
    /// a floor still at 0 moves.
    #[test]
    fn saved_zero_floors_move_to_the_new_defaults_once() {
        let mut s: Settings = serde_json::from_str(
            r#"{"detectors":{"tbss_core_dbz":60.0,"zdr_min_db":1.0,"zdr_min_depth_km":1.0,
                "glm_fed_cell_deg":0.05,"glm_fed_window_min":15,
                "tds_min_confidence":0.0,"rotation_min_confidence":0.3}}"#,
        )
        .unwrap();
        assert!(!s.detectors.floors_adopted);
        assert!(s.adopt_detector_floors());
        assert_eq!(s.detectors.tds_min_confidence, DEFAULT_TDS_MIN_CONFIDENCE);
        assert_eq!(
            s.detectors.rotation_min_confidence, 0.3,
            "a chosen floor stays"
        );
        // Put back to 0 on purpose afterwards: it stays at 0.
        s.detectors.tds_min_confidence = 0.0;
        assert!(!s.adopt_detector_floors());
        assert_eq!(s.detectors.tds_min_confidence, 0.0);
        // And it survives a save.
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert!(back.detectors.floors_adopted);
        // A fresh install has nothing to move.
        assert!(!Settings::default().adopt_detector_floors());
    }

    #[test]
    fn detector_tuning_survives_a_settings_file_that_predates_it() {
        // Settings written before the knobs existed must load with the shipped thresholds.
        let old: Settings = serde_json::from_str(r#"{"default_site":"KTLX"}"#).unwrap();
        assert_eq!(old.detectors, DetectorTuning::default());
        assert_eq!(old.detectors.tbss_core_dbz, 60.0);

        let mut s = Settings::default();
        s.detectors.zdr_min_db = 2.5;
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.detectors.zdr_min_db, 2.5);
    }

    #[test]
    fn the_comparison_view_and_paint_order_roundtrip() {
        use crate::fielddiff::{DiffField, DiffMode};
        let s = Settings {
            compare_view: Some((DiffField::RunToRunCape, DiffMode::Percent)),
            field_order: vec![crate::render::FieldLayer::Mrms],
            ..Default::default()
        };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.compare_view, s.compare_view);
        assert_eq!(back.field_order, s.field_order);
        let bare: Settings = serde_json::from_str("{}").unwrap();
        assert!(bare.compare_view.is_none() && bare.field_order.is_empty());
    }

    #[test]
    fn field_opacity_roundtrips_and_defaults() {
        let mut s = Settings::default();
        assert!(s.field_opacity.is_empty(), "no entry = fully opaque");
        s.field_opacity.insert(crate::render::FieldLayer::Mrms, 0.4);
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.field_opacity
                .get(&crate::render::FieldLayer::Mrms)
                .copied(),
            Some(0.4)
        );
        // Old config files without the key still load.
        let bare: Settings = serde_json::from_str("{}").unwrap();
        assert!(bare.field_opacity.is_empty());
    }

    #[test]
    fn a_settings_file_from_the_coach_mark_era_still_loads() {
        // `coach_done` went away with the coach marks; every installed settings.json still has it.
        let s: Settings =
            serde_json::from_str(r#"{"coach_done": true, "setup_done": true}"#).unwrap();
        assert!(s.setup_done);
    }

    #[test]
    fn a_pre_rename_layout_field_still_loads_as_command_ribbon() {
        // theme_plan.md §6.2: `Layout::Wsv3` (the original ribbon layout) was renamed to
        // `CommandRibbon` to make room for a new, distinct `Wsv3` theme. An old settings.json's
        // `"layout": "Wsv3"` must land on the renamed variant, not silently reset to the default
        // or collide with the new variant of the same pre-rename name.
        let s: Settings = serde_json::from_str(r#"{"layout": "Wsv3"}"#).unwrap();
        assert_eq!(s.layout, Layout::CommandRibbon);
    }

    #[test]
    fn the_new_wsv3_theme_round_trips_under_its_own_distinct_name() {
        let s = Settings {
            layout: Layout::Wsv3,
            tablet_layout_adopted: false,
            ..Settings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            json.contains(r#""layout":"Wsv3Theme""#),
            "the new Wsv3 variant must not serialize under the CommandRibbon alias's name: {json}"
        );
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.layout, Layout::Wsv3);
    }

    #[test]
    fn every_layout_is_ribbon_or_not_with_no_third_option() {
        assert!(Layout::CommandRibbon.is_ribbon());
        assert!(!Layout::Wsv3.is_ribbon() && Layout::Wsv3.is_workstation());
        assert!(Layout::Wsv3.map_first() && !Layout::Dock.map_first());
        assert!(!Layout::Minimal.is_ribbon());
    }

    #[test]
    fn every_timeline_style_round_trips_and_has_a_distinct_label() {
        for s in TimelineStyle::ALL {
            let settings = Settings {
                timeline_style: s,
                ..Settings::default()
            };
            let json = serde_json::to_string(&settings).unwrap();
            let back: Settings = serde_json::from_str(&json).unwrap();
            assert_eq!(back.timeline_style, s, "{s:?} did not round-trip");
        }
        let labels: std::collections::HashSet<&str> =
            TimelineStyle::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(
            labels.len(),
            TimelineStyle::ALL.len(),
            "every timeline style needs a distinct label"
        );
    }

    #[test]
    fn an_old_settings_file_without_timeline_style_defaults_to_default() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.timeline_style, TimelineStyle::Default);
    }

    #[test]
    fn floating_search_button_defaults_off_and_round_trips() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert!(!s.floating_search_button);
        let on = Settings {
            floating_search_button: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&on).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert!(back.floating_search_button);
    }

    #[test]
    fn older_settings_keep_composite_live_sweeps() {
        let old: Settings = serde_json::from_str(r#"{"live_scan_indicator":true}"#).unwrap();
        assert_eq!(old.live_sweep_mode, LiveSweepMode::ContinuousComposite);
        let mut strict = old;
        strict.live_sweep_mode = LiveSweepMode::StrictCurrentSweep;
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&strict).unwrap()).unwrap();
        assert_eq!(restored.live_sweep_mode, LiveSweepMode::StrictCurrentSweep);
    }

    #[test]
    fn analyst_mode_defaults_off_and_round_trips() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert!(!s.analyst_mode);
        let on = Settings {
            analyst_mode: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&on).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert!(back.analyst_mode);
    }

    #[test]
    fn a_web_file_name_resolves_to_its_content() {
        let mut s = Settings::default();
        s.palettes.insert("REF".to_string(), "mine.pal".to_string());
        // Not in web_files yet: it is an ordinary path, whatever the platform makes of it.
        assert_eq!(
            s.palette_paths()[0].as_deref(),
            Some(std::path::Path::new("mine.pal"))
        );
        s.web_files
            .insert("mine.pal".to_string(), "Color: 5 255 0 0".to_string());
        let p = s.palette_paths()[0].clone().unwrap();
        assert_eq!(
            p.to_string_lossy(),
            format!("{}Color: 5 255 0 0", crate::colormap::INLINE_PREFIX)
        );
    }

    #[test]
    fn roundtrips() {
        let s = Settings {
            hints_seen: Vec::new(),
            web_files: BTreeMap::new(),
            gis_layers: vec![GisLayerConfig {
                id: 3,
                source: "districts.geojson".into(),
                name: "Districts".into(),
                visible: false,
                style: ImportedGisStyle {
                    color: [240, 80, 40],
                    stroke_width: 3.25,
                    opacity: 0.5,
                    min_zoom: 6.5,
                    max_zoom: 11.0,
                    fill_color: Some([20, 30, 40]),
                    fill_opacity: 0.25,
                    dash: LineDash::Dotted,
                    symbol: PointSymbol::Diamond,
                    point_size: 6.5,
                },
                label: Some("NAME".into()),
                label_template: "{NAME} ({POP})".into(),
                color_by: Some("POP".into()),
                symbol_by: Some("TYPE".into()),
                time_start: Some("BEGIN".into()),
                time_end: None,
                below: true,
                group: Some("Boundaries".into()),
                targets: true,
                filter: "POP > 1000".into(),
                labels_first: true,
            }],
            gis_groups: vec![GisGroup {
                name: "Boundaries".into(),
                visible: false,
            }],
            gis_next_id: 4,
            imported_gis: None,
            imported_gis_style: ImportedGisStyle::default(),
            imported_gis_label: None,
            imported_gis_color_by: None,
            imported_gis_time_start: None,
            imported_gis_time_end: None,
            imported_gis_below: false,
            reduce_motion: true,
            hide_far_3d: true,
            far_3d_factor: default_far_3d(),
            precip_tint: false,
            custom_tile_url: String::new(),
            custom_tile_max_z: default_custom_tile_max_z(),
            custom_tile_attribution: String::new(),
            detectors: DetectorTuning::default(),
            alert_rules: Vec::new(),
            serve_token: String::new(),
            quiet_pending: Vec::new(),
            volume_cache_mb: 0,
            tile_disk_cache_mb: 0,
            workspaces: Vec::new(),
            seeded_workspaces: false,
            offered_starters: Vec::new(),
            model_pick: String::new(),
            smooth_radar: false,
            live_scan_indicator: false,
            live_sweep_mode: LiveSweepMode::StrictCurrentSweep,
            share_card: true,
            loop_real_timing: true,
            loop_mp4_fps: default_mp4_fps(),
            wind_particle_pct: default_particle_pct(),
            blend_frames: false,
            field_crossfade: false,
            output_window: OutputPrefs::default(),
            broadcast: Default::default(),
            scenes: Vec::new(),
            layer_order: Vec::new(),
            recent_layers: Vec::new(),
            favorite_layers: Vec::new(),
            mping_key: String::new(),
            etop_dbz: 30.0,
            default_site: "KFWS".to_string(),
            accent: Some([255, 0, 128]),
            poll_interval_secs: 45,
            theme: Theme::DearImGui,
            layout: Layout::Minimal,
            workstation: Default::default(),
            tablet_layout_adopted: false,
            dock_adopted: false,
            station_adopted: true,
            local_api: true,
            local_api_port: 50_000,
            phone_design: PhoneDesign::Carbon,
            presets: vec!["KTLX".to_string(), "KOUN".to_string()],
            palettes: BTreeMap::from([("REF".to_string(), "/tmp/foo.pal".to_string())]),
            velocity_unit: VelocityUnit::Mph,
            temp_unit: TempUnit::Celsius,
            time_display: TimeDisplay::Utc,
            time_mismatch_minutes: 15,
            radar_stale_minutes: default_radar_stale_minutes(),
            check_builds: true,
            ui_scale: 1.2,
            sync_client_id: String::new(),
            sync_client_secret: String::new(),
            sync_enabled: false,
            share_position: true,
            share_name: "chaser".to_string(),
            share_relay: String::new(),
            share_video_url: String::new(),
            placefiles: vec![PlacefileConfig {
                url: "http://x/p.txt".to_string(),
                enabled: true,
                opacity: 1.0,
            }],
            udp_products: vec![wxdata::udp::ProductDef {
                id: String::new(),
                name: "Test product".to_string(),
                units: "dBZ".to_string(),
                expression: "REF + 1".to_string(),
                range: None,
                palette: None,
            }],
            outline_thresholds: [("CC".to_string(), vec![0.7, 0.85])].into_iter().collect(),
            markers: vec![Marker {
                id: new_marker_id(),
                name: "Home".to_string(),
                lat: 35.3,
                lon: -97.5,
                icon: Some("home.png".to_string()),
                alert_radius_mi: 20.0,
                video_url: String::new(),
                home: true,
            }],
            dealias_velocity: true,
            alert_spotlight: false,
            yall_mode: false,
            layer_probe: false,
            globe: false,
            scale_bar: true,
            volume3d_presets: vec![Volume3dPreset {
                name: "Hail core".into(),
                representation: "Smooth reflectivity".into(),
                floor: 45.0,
                ceiling: Some(70.0),
                curve: Some([[45.0, 0.1], [55.0, 0.4], [62.0, 0.8], [70.0, 1.0]]),
                render: Some(crate::render3d::VolumeRender::TranslucentLit),
                stops: Some(vec![
                    [40.0, 0.0],
                    [45.0, 0.1],
                    [55.0, 0.4],
                    [60.0, 0.6],
                    [62.0, 0.8],
                    [70.0, 1.0],
                ]),
                colors: Some(vec![[45.0, 40.0, 60.0, 170.0], [70.0, 215.0, 40.0, 40.0]]),
            }],
            radar_relay_url: "http://relay.local:8080".to_string(),
            radar_provider_override: RadarProviderOverride::Backup,
            mapbox_key: "pk.test".to_string(),
            tempest_token: String::new(),
            wu_key: String::new(),
            synoptic_token: String::new(),
            field_opacity: Default::default(),
            field_order: Vec::new(),
            compare_view: None,
            airnow_key: String::new(),
            windy_key: String::new(),
            field_mill_url: String::new(),
            maptiler_key: "mt.test".to_string(),
            start_view: Some(StartView {
                site: "KFWS".to_string(),
                x: 0.3,
                y: 0.4,
                zoom: 8.0,
            }),
            lightning_minutes: default_lightning_minutes(),
            glm_goes_west: false,
            goes_satellite_west: false,
            goes_rgb_recipe: default_goes_rgb_recipe(),
            goes_sector: Default::default(),
            spotter_range_km: default_spotter_range_km(),
            alert_sound: false,
            ntfy_topic: "hookecho-test".to_string(),
            discord_webhook: String::new(),
            slack_webhook: String::new(),
            matrix_homeserver: String::new(),
            matrix_room: String::new(),
            matrix_token: String::new(),
            mqtt_host: String::new(),
            mqtt_port: default_mqtt_port(),
            mqtt_tls: false,
            mqtt_user: String::new(),
            mqtt_pass: String::new(),
            mqtt_prefix: default_mqtt_prefix(),
            strikes_topic: String::new(),
            warnings_topic: String::new(),
            mqtt_discovery: false,
            background_alerts: false,
            pack_hires_dem: false,
            pack_include_vector: true,
            pack_include_satellite: false,
            alert_polygons: Vec::new(),
            plugins: Vec::new(),
            close_to_tray: true,
            bookmarks: vec![Bookmark {
                name: "Storm".to_string(),
                site: "KTLX".to_string(),
                x: 0.3,
                y: 0.4,
                zoom: 9.0,
                time_secs: Some(1_600_000_000),
                span_min: 60,
            }],
            anthropic_key: "sk-test".to_string(),
            gemini_key: String::new(),
            ai_provider: Default::default(),
            lightning_alarm: true,
            speak_warnings: true,
            piper_path: String::new(),
            piper_voice: String::new(),
            piper_download_voice: String::new(),
            speak_position: false,
            rain_alerts: true,
            rain_sound: AlertSound::Ding,
            setup_done: true,
            desktop_notify: false,
            chase_log: false,
            route_engine: Default::default(),
            route_url: String::new(),
            battery_saver: false,
            wind_barbs: false,
            wind_streamlines: false,
            ntfy_snapshot: false,
            alert_follow_gps: false,
            gps_autoconnect: false,
            quiet_hours: false,
            quiet_start_hour: 22,
            quiet_end_hour: 7,
            alert_min_escalation: 0,
            alert_rollup_threshold: default_alert_rollup_threshold(),
            alert_rollup_window_min: default_alert_rollup_window_min(),
            scan_chime: false,
            scan_sound: AlertSound::Ding,
            warn_sound: AlertSound::Siren,
            tds_sound: AlertSound::Custom("/tmp/tds.wav".to_string()),
            rotation_sound: AlertSound::Siren,
            lightning_sound: AlertSound::Alarm,
            emergency_sound: AlertSound::Alarm,
            alert_volume: 0.7,
            live_loop_frames: 12,
            timeline_style: TimelineStyle::Wsv3,
            floating_search_button: true,
            analyst_mode: true,
            basemap: "carto-dark".to_string(),
            overlays_on: Some(vec!["Alerts".to_string(), "Wind".to_string()]),
            contours_on: vec!["stp".to_string()],
            window: None,
            last_view: Some(StartView {
                site: "KOUN".to_string(),
                x: 0.2,
                y: 0.4,
                zoom: 7.5,
            }),
            nwr_streams: vec![NwrStream {
                name: "KEC55 Norman".into(),
                url: "https://example.invalid/nwr.mp3".into(),
            }],
            mute_alerts: true,
            keybinds: vec![crate::hotkeys::Binding {
                shortcut: egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::K),
                action: crate::hotkeys::BindableAction::Palette(
                    crate::app::PaletteAction::SetMoment(wxdata::level2::Moment::Velocity, true),
                ),
            }],
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn quiet_hours_window_wraps_midnight() {
        let mut s = Settings {
            quiet_hours: true,
            quiet_start_hour: 22,
            quiet_end_hour: 7,
            ..Settings::default()
        };
        assert!(s.in_quiet_hours(23) && s.in_quiet_hours(0) && s.in_quiet_hours(6));
        assert!(!s.in_quiet_hours(7), "the end hour is already awake");
        assert!(!s.in_quiet_hours(21) && !s.in_quiet_hours(12));
        // A window inside one day.
        s.quiet_start_hour = 9;
        s.quiet_end_hour = 17;
        assert!(s.in_quiet_hours(9) && s.in_quiet_hours(16) && !s.in_quiet_hours(17));
        // Equal bounds is no window, not all day.
        s.quiet_end_hour = 9;
        assert!(!s.in_quiet_hours(9) && !s.in_quiet_hours(3));
        // And off is off.
        s.quiet_hours = false;
        s.quiet_start_hour = 0;
        s.quiet_end_hour = 23;
        assert!(!s.in_quiet_hours(5));
    }

    #[test]
    fn a_bundle_carries_its_gis_files_checked_and_repointed() {
        let base = std::env::temp_dir().join(format!(
            "hookecho-gis-bundle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (src, dst) = (base.join("src"), base.join("dst"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        let geojson = src.join("sirens.geojson");
        std::fs::write(&geojson, r#"{"type":"FeatureCollection","features":[]}"#).unwrap();
        let zip = src.join("assets.zip");
        std::fs::write(&zip, [0x50u8, 0x4b, 3, 4, 0, 255, 7]).unwrap();
        let mut s = Settings::default();
        let a = s.add_gis_layer(geojson.to_string_lossy().into_owned());
        let b = s.add_gis_layer(format!("{}#hospitals.shp", zip.display()));
        let c = s.add_gis_layer(format!("{}#sirens.shp", zip.display()));
        s.add_gis_layer(src.join("gone.kml").to_string_lossy().into_owned());
        let json = s.export_bundle().unwrap();
        let raw: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            raw["gis_files"].as_object().unwrap().len(),
            2,
            "the zip is packed once"
        );
        // The originals go away; the bundle still opens, pointing at its own copies.
        std::fs::remove_dir_all(&src).unwrap();
        let (back, notes) =
            Settings::import_bundle_with_dir(&json, || None, || Some(dst.clone())).unwrap();
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("gone.kml"), "{notes:?}");
        let source = |id| back.gis_layer(id).unwrap().source.clone();
        assert!(source(a).starts_with(&dst.to_string_lossy().into_owned()));
        assert_eq!(
            std::fs::read_to_string(source(a)).unwrap(),
            r#"{"type":"FeatureCollection","features":[]}"#
        );
        assert!(source(b).ends_with("#hospitals.shp") && source(c).ends_with("#sirens.shp"));
        assert_eq!(gis_source_file(&source(b)), gis_source_file(&source(c)));
        assert_eq!(
            std::fs::read(gis_source_file(&source(b))).unwrap(),
            [0x50u8, 0x4b, 3, 4, 0, 255, 7]
        );
        // A damaged file is refused and nothing is written.
        let mut tampered: serde_json::Value = serde_json::from_str(&json).unwrap();
        let files = tampered["gis_files"].as_object_mut().unwrap();
        let first = files.keys().next().unwrap().clone();
        files.get_mut(&first).unwrap()["sha256"] = "00".into();
        let empty = base.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let err = Settings::import_bundle_with_dir(
            &tampered.to_string(),
            || None,
            || Some(empty.clone()),
        )
        .unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert_eq!(std::fs::read_dir(&empty).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn bundle_inlines_and_restores_palettes() {
        // A bundle with an inlined .pal should restore to a local path whose file has that text.
        let json = r#"{
            "settings": {"default_site":"KFWS","theme":"Magma","markers":[{"name":"H","lat":1.0,"lon":2.0}]},
            "palette_files": {"REF":"; test palette\nStep: 5\n"}
        }"#;
        let dir = std::env::temp_dir().join(format!(
            "hookecho-bundle-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let (s, _) =
            Settings::import_bundle_with_dir(json, || Some(dir.clone()), || None).expect("import");
        assert_eq!(s.default_site, "KFWS");
        assert_eq!(s.theme, Theme::DearImGui); // "Magma" is aliased onto the one theme
        let ref_path = s.palettes.get("REF").expect("REF palette path set");
        let text = std::fs::read_to_string(ref_path).expect("palette file written");
        assert!(text.contains("test palette"));
        std::fs::remove_file(ref_path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn an_unknown_overlay_name_does_not_take_the_file_down_with_it() {
        // A file written by a newer build, carrying a layer this one has never heard of. The rest
        // of the settings must survive: `Settings::load` falls back to defaults on a parse error,
        // so a strict enum here would silently reset the user's whole configuration.
        let json = r#"{"default_site":"KDMX","overlays_on":["Alerts","Teleportation"]}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.default_site, "KDMX");
        assert_eq!(s.overlays_on.unwrap(), vec!["Alerts", "Teleportation"]);
    }

    #[test]
    fn no_layers_and_no_record_of_layers_are_different_things() {
        // The whole reason `overlays_on` is an Option. An empty list is a user who turned
        // everything off and expects it to stay off; a missing key is a fresh install, where the
        // app's own defaults have to win. Collapsing the two booted every new user with a blank
        // map, or resurrected a layer they had just switched off.
        let none: Settings = serde_json::from_str(r#"{"default_site":"KDMX"}"#).unwrap();
        assert_eq!(none.overlays_on, None);
        let empty: Settings =
            serde_json::from_str(r#"{"default_site":"KDMX","overlays_on":[]}"#).unwrap();
        assert_eq!(empty.overlays_on, Some(Vec::new()));
    }

    #[test]
    fn a_window_size_is_remembered_and_an_old_file_still_loads() {
        // Written by a build that predates the key: no window, so the built-in 1280x800 stands.
        let old: Settings = serde_json::from_str(r#"{"default_site":"KDMX"}"#).unwrap();
        assert_eq!(old.window, None);
        // `maximized` has its own default, so a file written before that field existed is fine too.
        let s: Settings =
            serde_json::from_str(r#"{"window":{"width":1600.0,"height":900.0}}"#).unwrap();
        let w = s.window.unwrap();
        assert_eq!((w.width, w.height, w.maximized), (1600.0, 900.0, false));
        // And it round-trips, which is what the save path relies on.
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.window, s.window);
    }

    #[test]
    fn temp_unit_converts_from_celsius() {
        assert_eq!(TempUnit::Celsius.from_c(21.0), 21.0);
        assert!((TempUnit::Fahrenheit.from_c(0.0) - 32.0).abs() < 1e-4);
        assert!((TempUnit::Fahrenheit.from_c(-40.0) + 40.0).abs() < 1e-4);
    }

    #[test]
    fn tolerates_unknown_and_missing_fields() {
        // An old/newer config: extra field, and a missing one that should default.
        let json = r#"{"default_site":"KDMX","future_field":true}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.default_site, "KDMX");
        assert_eq!(s.poll_interval_secs, 30, "missing field defaults");
        assert_eq!(s.time_mismatch_minutes, 10);
    }

    /// The Android alert service (`android/app/src/main/kotlin/.../AlertService.kt`) parses
    /// settings.json by hand, in another language, with no compiler to tell it when a field is
    /// renamed here. This test is that compiler: rename one of these and it fails, loudly,
    /// pointing at the Kotlin that has to change with it.
    #[test]
    fn kotlin_alert_service_field_names_survive() {
        let json = serde_json::to_string(&Settings {
            markers: vec![Marker {
                id: new_marker_id(),
                name: "Home".to_string(),
                lat: 35.0,
                lon: -97.0,
                icon: None,
                alert_radius_mi: crate::settings::default_alert_radius_mi(),
                video_url: String::new(),
                home: false,
            }],
            alert_polygons: vec![AlertPolygon {
                name: "Farm".to_string(),
                ring: vec![[-97.0, 35.0], [-96.9, 35.0], [-96.9, 35.1]],
            }],
            background_alerts: true,
            ..Settings::default()
        })
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["background_alerts"], serde_json::json!(true));
        let m = &v["markers"][0];
        for key in ["name", "lat", "lon", "alert_radius_mi"] {
            assert!(
                m.get(key).is_some(),
                "AlertService.kt reads markers[].{key}"
            );
        }
        let z = &v["alert_polygons"][0];
        for key in ["name", "ring"] {
            assert!(
                z.get(key).is_some(),
                "AlertService.kt reads alert_polygons[].{key}"
            );
        }
        assert_eq!(
            z["ring"][0][0],
            serde_json::json!(-97.0),
            "Nws.kt reads ring vertices as [lon, lat]"
        );
    }

    /// The bug this was written for: one bad enum value used to discard the entire settings file.
    #[test]
    fn one_unreadable_value_does_not_discard_the_whole_file() {
        // `theme` serialises as "Light"; lowercase is not a valid variant. Everything else here
        // is perfectly good and must survive.
        let text = r#"{
            "theme": "light",
            "mapbox_key": "kept",
            "ui_scale": 1.25,
            "smooth_radar": true
        }"#;
        let s = Settings::from_json_lossy(text);
        assert_eq!(s.mapbox_key, "kept", "a good key was thrown away");
        assert!((s.ui_scale - 1.25).abs() < 1e-6);
        assert!(s.smooth_radar);
        // Only the unreadable field falls back, to the default settings' own.
        assert_eq!(s.theme, Settings::default().theme);
    }

    #[test]
    fn a_good_file_is_unchanged_and_a_broken_one_still_loads() {
        let original = Settings {
            mapbox_key: "abc".to_string(),
            ui_scale: 1.1,
            ..Default::default()
        };
        let text = serde_json::to_string(&original).unwrap();
        let round = Settings::from_json_lossy(&text);
        assert_eq!(round.mapbox_key, "abc");
        assert!((round.ui_scale - 1.1).abs() < 1e-6);

        // Not an object at all, and not even JSON: defaults, no panic.
        assert_eq!(Settings::from_json_lossy("[1,2,3]").mapbox_key, "");
        assert_eq!(Settings::from_json_lossy("not json").mapbox_key, "");
    }

    /// Unknown keys were already tolerated and must stay that way — a settings file written by a
    /// newer build has to open in an older one.
    #[test]
    fn unknown_keys_are_still_ignored() {
        let s = Settings::from_json_lossy(r#"{"mapbox_key":"x","a_field_from_the_future":42}"#);
        assert_eq!(s.mapbox_key, "x");
    }

    #[test]
    fn the_dock_layout_and_a_phone_design_survive_a_round_trip() {
        let s = Settings::from_json_lossy(r#"{"layout":"Dock","phone_design":"Atlas"}"#);
        assert_eq!(s.layout, Layout::Dock);
        assert!(s.layout.is_workstation() && !s.layout.is_ribbon());
        assert_eq!(s.phone_design, PhoneDesign::Atlas);
        let back = Settings::from_json_lossy(&serde_json::to_string(&s).unwrap());
        assert_eq!(
            (back.layout, back.phone_design),
            (Layout::Dock, PhoneDesign::Atlas)
        );
    }

    #[test]
    fn a_settings_file_with_no_phone_design_gets_station() {
        assert_eq!(
            Settings::from_json_lossy("{}").phone_design,
            PhoneDesign::Station
        );
    }

    #[test]
    fn a_phone_on_the_old_default_design_moves_to_station_once() {
        let mut s = Settings {
            phone_design: PhoneDesign::Aurora,
            ..Settings::default()
        };
        assert!(s.adopt_station_default());
        assert_eq!(s.phone_design, PhoneDesign::Station);
        // Changed back on purpose: stays changed.
        s.phone_design = PhoneDesign::Aurora;
        assert!(!s.adopt_station_default());
        assert_eq!(s.phone_design, PhoneDesign::Aurora);
        // A design picked on purpose is never replaced.
        let mut carbon = Settings {
            phone_design: PhoneDesign::Carbon,
            ..Settings::default()
        };
        assert!(!carbon.adopt_station_default());
        assert_eq!(carbon.phone_design, PhoneDesign::Carbon);
    }

    #[test]
    fn a_tablet_on_the_shipped_ribbon_moves_to_the_dock_once() {
        let mut s = Settings {
            layout: Layout::CommandRibbon,
            ..Settings::default()
        };
        // Not a tablet: untouched, and not marked, so it can still happen if it ever is one.
        assert!(!s.adopt_tablet_default(false));
        assert_eq!(s.layout, Layout::CommandRibbon);
        assert!(s.adopt_tablet_default(true));
        assert_eq!(s.layout, Layout::Dock);
        assert_eq!(s.theme, Theme::DearImGui);
        // Changed back on purpose: stays changed.
        s.layout = Layout::CommandRibbon;
        assert!(!s.adopt_tablet_default(true));
        assert_eq!(s.layout, Layout::CommandRibbon);
    }

    #[test]
    fn retired_theme_names_still_load() {
        for name in [
            "Dark",
            "Light",
            "System",
            "Classic",
            "Synthwave",
            "Aurora",
            "HighContrast",
            "Oled",
            "Magma",
            "Glacier",
            "DearImGui",
        ] {
            let s: Settings = serde_json::from_str(&format!(r#"{{"theme":"{name}"}}"#)).unwrap();
            assert_eq!(s.theme, Theme::DearImGui, "{name}");
        }
        // A file that still carries the retired density setting loads too.
        let s: Settings = serde_json::from_str(r#"{"density":"Comfortable"}"#).unwrap();
        assert_eq!(s.theme, Theme::DearImGui);
    }

    #[test]
    fn the_dock_is_the_default_and_the_old_default_ribbon_moves_to_it_once() {
        let fresh = Settings::default();
        assert_eq!(fresh.layout, Layout::Dock);
        assert_eq!(fresh.theme, Theme::DearImGui, "with its own look");
        // A file from before the dock was the default, still on the ribbon.
        let mut old: Settings = serde_json::from_str(r#"{"layout":"CommandRibbon"}"#).unwrap();
        assert!(!old.dock_adopted);
        assert!(old.adopt_dock_default());
        assert_eq!(old.layout, Layout::Dock);
        assert_eq!(old.theme, Theme::DearImGui);
        // The ribbon picked again afterwards stays picked.
        old.layout = Layout::CommandRibbon;
        assert!(!old.adopt_dock_default());
        assert_eq!(old.layout, Layout::CommandRibbon);
        // Any other layout is kept.
        let mut minimal: Settings = serde_json::from_str(r#"{"layout":"Minimal"}"#).unwrap();
        assert!(!minimal.adopt_dock_default());
        assert_eq!(minimal.layout, Layout::Minimal);
    }

    #[test]
    fn a_layout_chosen_on_purpose_is_never_replaced() {
        for chosen in [Layout::Minimal, Layout::Wsv3, Layout::Dock] {
            let mut s = Settings {
                layout: chosen,
                ..Settings::default()
            };
            assert!(!s.adopt_tablet_default(true));
            assert_eq!(s.layout, chosen);
            assert!(s.tablet_layout_adopted, "and it is not asked again");
        }
        // Settings saved before the field existed load as not yet adopted.
        let old: Settings = serde_json::from_str(
            &serde_json::to_string(&Settings::default())
                .unwrap()
                .replace("\"tablet_layout_adopted\":false,", ""),
        )
        .unwrap();
        assert!(!old.tablet_layout_adopted);
    }
}
