//! The imported GIS layers (ROADMAP_PARITY M4.1): every file the analyst imported, each its own
//! layer with its own style, labels, colouring, time mapping, visibility, order and group.
//!
//! What a layer *is* lives in `settings.gis_layers` ([`crate::settings::GisLayerConfig`]); what
//! was read from its file lives here, keyed by the same stable ID. Importing another file adds a
//! layer instead of replacing the last one; importing a file that is already a layer refreshes
//! that layer's shapes and keeps its settings. A layer whose file cannot be read stays in the
//! list with the reason, to be located again or removed, rather than silently dropped.
//!
//! Polygons ride the shared overlay pipeline (`self.overlays`), with `self.overlay_layer` saying
//! which layer each one came from, so tessellation, hit-testing and paint order read each layer's
//! own outline width, minimum zoom and position. Points, lines and labels paint here.
use super::*;
use crate::gis_import::{ColoredBy, Marks, TimeBounds};

/// A layer's symbol-by attribute, each source feature's symbol, the legend, and how many more
/// values keep the layer's own symbol.
pub(crate) type SymbolsBy = (
    String,
    Vec<Option<crate::settings::PointSymbol>>,
    crate::gis_import::SymbolLegend,
    usize,
);

/// A layer's valid windows and the start/end attributes they were read with.
pub(crate) type ImportedTime = ((Option<String>, Option<String>), TimeBounds);

/// What was read from one layer's file.
pub(crate) struct LoadedGis {
    pub id: u64,
    pub shapes: Vec<GeoFeature>,
    pub marks: Marks,
    /// The features' colours by the layer's colour-by attribute, and their legend.
    pub colors: Option<ColoredBy>,
    /// The features' point symbols by the layer's symbol-by attribute: the attribute, each
    /// source feature's symbol, the legend and how many values share the layer's own symbol.
    pub symbols: Option<SymbolsBy>,
    pub time: Option<ImportedTime>,
    /// Per source feature, whether it is valid at the view's time; `None` without a time mapping.
    time_mask: Option<Vec<bool>>,
    /// The attribute filter in use (the last one that parsed), its text, and which source
    /// features it passes (M4.3).
    filter: Option<(String, Vec<bool>)>,
    /// Why the layer's current filter text does not parse, while the previous one stays in use.
    pub filter_error: Option<String>,
    /// Per source feature, whether it is drawn: valid at the view's time and passing the filter;
    /// `None` with neither, which shows every feature. Drawing, clicks, labels, export and impact
    /// targets all read this one mask.
    pub shown: Option<Vec<bool>>,
    /// Why the file could not be read, when it could not.
    pub error: Option<String>,
}

impl LoadedGis {
    fn new(id: u64, features: Vec<wxdata::gis::GisFeature>) -> Self {
        let (shapes, marks) = crate::gis_import::to_renderable(features);
        LoadedGis {
            id,
            shapes,
            marks,
            colors: None,
            symbols: None,
            time: None,
            time_mask: None,
            filter: None,
            filter_error: None,
            shown: None,
            error: None,
        }
    }

    fn failed(id: u64, error: String) -> Self {
        LoadedGis {
            error: Some(error),
            ..LoadedGis::new(id, Vec::new())
        }
    }

    /// How many shapes, lines and points it holds.
    pub fn len(&self) -> usize {
        self.shapes.len() + self.marks.len()
    }

    /// Whether source feature `src` is valid at the view's time (always, with no time mapping).
    pub fn valid(&self, src: Option<&usize>) -> bool {
        self.shown
            .as_ref()
            .is_none_or(|m| src.and_then(|&s| m.get(s)).copied().unwrap_or(true))
    }

    /// Bring the time mask and colours in step with `config` at time `t`. Returns whether the
    /// drawn polygons change.
    fn sync(&mut self, config: &crate::settings::GisLayerConfig, t: chrono::DateTime<Utc>) -> bool {
        let mut changed = false;
        let keys = (config.time_start.clone(), config.time_end.clone());
        if keys == (None, None) || self.marks.props.is_empty() {
            self.time = None;
            self.time_mask = None;
        } else {
            if self.time.as_ref().is_none_or(|(k, _)| *k != keys) {
                let bounds = crate::gis_import::time_bounds(
                    &self.marks,
                    keys.0.as_deref(),
                    keys.1.as_deref(),
                );
                self.time = Some((keys, bounds));
            }
            if let Some((_, bounds)) = &self.time {
                self.time_mask = Some(crate::gis_import::shown_at(bounds, t));
            }
        }
        // The attribute filter: re-parsed only when its text changes; one that does not parse
        // leaves the previous one in force and says why.
        let text = config.filter.trim();
        if text.is_empty() {
            self.filter = None;
            self.filter_error = None;
        } else if self.filter.as_ref().is_none_or(|(f, _)| f != text) {
            match crate::gis_filter::parse(text) {
                Ok(f) => {
                    let mask = self.marks.props.iter().map(|p| f.shows(p)).collect();
                    self.filter = Some((text.to_string(), mask));
                    self.filter_error = None;
                }
                Err(e) => self.filter_error = Some(e),
            }
        }
        let shown = match (&self.time_mask, &self.filter) {
            (None, None) => None,
            (Some(m), None) | (None, Some((_, m))) => Some(m.clone()),
            (Some(a), Some((_, b))) => Some(a.iter().zip(b).map(|(x, y)| *x && *y).collect()),
        };
        if shown != self.shown {
            self.shown = shown;
            changed = true;
        }
        match &config.color_by {
            None => changed |= self.colors.take().is_some(),
            Some(key) if self.colors.as_ref().is_some_and(|(k, _, _)| k == key) => {}
            Some(key) => {
                let (colors, legend) = crate::gis_import::color_by(&self.marks, key);
                self.colors = Some((key.clone(), colors, legend));
                changed = true;
            }
        }
        match &config.symbol_by {
            None => changed |= self.symbols.take().is_some(),
            Some(key) if self.symbols.as_ref().is_some_and(|s| &s.0 == key) => {}
            Some(key) => {
                let (symbols, legend, others) = crate::gis_import::symbol_by(&self.marks, key);
                self.symbols = Some((key.clone(), symbols, legend, others));
                changed = true;
            }
        }
        changed
    }

    /// The polygons to draw for `config`: time-filtered, coloured and styled.
    fn styled_shapes(&self, config: &crate::settings::GisLayerConfig) -> Vec<(GeoFeature, usize)> {
        let colors = self.colors.as_ref().map(|(_, c, _)| c);
        let src = &self.marks.shape_src;
        self.shapes
            .iter()
            .enumerate()
            .filter(|&(i, _)| self.valid(src.get(i)))
            .map(|(i, feature)| {
                let mut feature = feature.clone();
                let mut style = config.style;
                if let Some(c) = colors.and_then(|c| *c.get(*src.get(i)?)?) {
                    style = style.colored(c);
                }
                crate::gis_import::apply_style(&mut feature, style);
                (feature, src.get(i).copied().unwrap_or(i))
            })
            .collect()
    }
}

/// One point symbol at `p`, `radius` pixels out, filled with `color` and edged in black so it
/// reads over a bright core and a dark basemap alike.
pub(crate) fn paint_symbol(
    painter: &egui::Painter,
    p: egui::Pos2,
    symbol: crate::settings::PointSymbol,
    radius: f32,
    color: egui::Color32,
) {
    use crate::settings::PointSymbol;
    let edge = egui::Stroke::new(1.0, egui::Color32::from_black_alpha(180));
    let r = radius;
    let poly = |pts: Vec<egui::Pos2>| {
        painter.add(egui::Shape::convex_polygon(pts, color, edge));
    };
    match symbol {
        PointSymbol::Circle => {
            painter.circle_filled(p, r, color);
            painter.circle_stroke(p, r, edge);
        }
        PointSymbol::Square => {
            let s = r * 0.9;
            let rect = egui::Rect::from_center_size(p, egui::vec2(2.0 * s, 2.0 * s));
            painter.rect_filled(rect, 0.0, color);
            painter.rect_stroke(rect, 0.0, edge, egui::StrokeKind::Middle);
        }
        PointSymbol::Triangle => {
            let s = r * 1.25;
            poly(vec![
                p + egui::vec2(0.0, -s),
                p + egui::vec2(s * 0.866, s * 0.5),
                p + egui::vec2(-s * 0.866, s * 0.5),
            ]);
        }
        PointSymbol::Diamond => {
            let s = r * 1.2;
            poly(vec![
                p + egui::vec2(0.0, -s),
                p + egui::vec2(s, 0.0),
                p + egui::vec2(0.0, s),
                p + egui::vec2(-s, 0.0),
            ]);
        }
        PointSymbol::Cross => {
            let s = r * 1.1;
            let w = (r * 0.45).max(1.5);
            for (a, b) in [
                (egui::vec2(-s, -s), egui::vec2(s, s)),
                (egui::vec2(-s, s), egui::vec2(s, -s)),
            ] {
                painter.line_segment(
                    [p + a, p + b],
                    egui::Stroke::new(w + 2.0, egui::Color32::from_black_alpha(180)),
                );
            }
            for (a, b) in [
                (egui::vec2(-s, -s), egui::vec2(s, s)),
                (egui::vec2(-s, s), egui::vec2(s, -s)),
            ] {
                painter.line_segment([p + a, p + b], egui::Stroke::new(w, color));
            }
        }
    }
}

/// The most point and area targets one storm's arrivals are computed for: arrivals run every
/// frame the storm's card is open, and an area entry walks the projection minute by minute.
pub(crate) const MAX_POINT_TARGETS: usize = 2000;
pub(crate) const MAX_AREA_TARGETS: usize = 200;

/// A place a storm's arrival is computed for, from an imported layer marked as targets.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Target {
    pub name: String,
    pub layer: String,
    pub shape: TargetShape,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TargetShape {
    Point([f64; 2]),
    /// An area's outer ring.
    Area(Vec<[f64; 2]>),
}

/// Read a layer's source: a browser's remembered text (a shapefile or KMZ there was stored as
/// GeoJSON, a KML as itself), or a path, read whichever format it is.
fn load_source(
    settings: &crate::settings::Settings,
    source: &str,
) -> Result<crate::gis_import::Loaded, String> {
    match settings.web_files.get(source) {
        Some(text) if crate::gis_import::is_kml(source) => crate::gis_import::load_kml(text),
        Some(text) => crate::gis_import::load_geojson(text),
        None => crate::gis_import::load_path(source),
    }
}

/// A layer's shown features (valid at the view's time and passing its filter) with the
/// attributes its file gave them, plus `hookecho: "imported"` and the layer's name.
pub(crate) fn layer_features(
    config: &crate::settings::GisLayerConfig,
    layer: &LoadedGis,
) -> Vec<wxdata::gis::GisFeature> {
    layer_features_of(config, layer, |_| true)
}

/// [`layer_features`] for only the source features `keep` accepts (a feature table's searched
/// rows, or the one picked in it): every part of each, polygons, lines and points alike.
pub(crate) fn layer_features_of(
    config: &crate::settings::GisLayerConfig,
    layer: &LoadedGis,
    keep: impl Fn(usize) -> bool,
) -> Vec<wxdata::gis::GisFeature> {
    use wxdata::gis::{Geometry, GisFeature};
    let wanted = |src: Option<&usize>| src.is_some_and(|&s| keep(s));
    let mut out = Vec::new();
    let config_name = &config.name;
    let m = &layer.marks;
    let props = |src: Option<&usize>| {
        let mut p = src
            .and_then(|&s| m.props.get(s))
            .cloned()
            .unwrap_or_default();
        p.insert("hookecho".into(), "imported".into());
        p.insert("layer".into(), config_name.clone().into());
        p
    };
    for (i, f) in layer.shapes.iter().enumerate() {
        let src = m.shape_src.get(i);
        if layer.valid(src) && wanted(src) && !f.rings.is_empty() {
            out.push(GisFeature {
                geometry: Geometry::Polygon(f.rings.clone()),
                properties: props(src),
            });
        }
    }
    for (i, line) in m.lines.iter().enumerate() {
        let src = m.line_src.get(i);
        if layer.valid(src) && wanted(src) {
            out.push(GisFeature {
                geometry: Geometry::LineString(line.clone()),
                properties: props(src),
            });
        }
    }
    for (i, p) in m.points.iter().enumerate() {
        let src = m.point_src.get(i);
        if layer.valid(src) && wanted(src) {
            out.push(GisFeature {
                geometry: Geometry::Point(*p),
                properties: props(src),
            });
        }
    }
    out
}

/// One row of a layer's feature table: the source feature, and its value for each column (empty
/// where it has none).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableRow {
    pub src: usize,
    pub values: Vec<String>,
}

fn cell_text(v: Option<&serde_json::Value>) -> String {
    match v {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// A layer's feature table (ROADMAP_PARITY M4.3): every source feature the layer shows (its
/// filter and time window applied, so the table and the map agree), one column per attribute in
/// `keys`, rows whose values contain `query` (any column, ignoring case), sorted by column `sort`
/// — numerically when both values are numbers, as text otherwise, empty values last — or in file
/// order.
pub(crate) fn feature_table(
    layer: &LoadedGis,
    keys: &[String],
    query: &str,
    sort: Option<(usize, bool)>,
) -> Vec<TableRow> {
    let q = query.trim().to_lowercase();
    let mut rows: Vec<TableRow> = layer
        .marks
        .props
        .iter()
        .enumerate()
        .filter(|(i, _)| layer.valid(Some(i)))
        .map(|(src, props)| TableRow {
            src,
            values: keys.iter().map(|k| cell_text(props.get(k))).collect(),
        })
        .filter(|r| q.is_empty() || r.values.iter().any(|v| v.to_lowercase().contains(&q)))
        .collect();
    if let Some((col, descending)) = sort {
        rows.sort_by(|a, b| {
            let (x, y) = (&a.values[col], &b.values[col]);
            let ord = match (x.is_empty(), y.is_empty()) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => return std::cmp::Ordering::Greater,
                (false, true) => return std::cmp::Ordering::Less,
                _ => match (x.parse::<f64>(), y.parse::<f64>()) {
                    (Ok(p), Ok(q)) => p.total_cmp(&q),
                    _ => x.to_lowercase().cmp(&y.to_lowercase()),
                },
            };
            let ord = if descending { ord.reverse() } else { ord };
            ord.then(a.src.cmp(&b.src))
        });
    }
    rows
}

/// The box around every part of source feature `src`: its polygons, lines and points.
pub(crate) fn feature_bounds(layer: &LoadedGis, src: usize) -> Option<(f64, f64, f64, f64)> {
    let m = &layer.marks;
    let mut acc: Option<(f64, f64, f64, f64)> = None;
    let mut add = |p: [f64; 2]| {
        acc = Some(match acc {
            None => (p[0], p[1], p[0], p[1]),
            Some((w, s, e, n)) => (w.min(p[0]), s.min(p[1]), e.max(p[0]), n.max(p[1])),
        });
    };
    for (i, f) in layer.shapes.iter().enumerate() {
        if m.shape_src.get(i) == Some(&src) {
            f.rings.iter().flatten().for_each(|p| add(*p));
        }
    }
    for (i, l) in m.lines.iter().enumerate() {
        if m.line_src.get(i) == Some(&src) {
            l.iter().for_each(|p| add(*p));
        }
    }
    for (i, p) in m.points.iter().enumerate() {
        if m.point_src.get(i) == Some(&src) {
            add(*p);
        }
    }
    acc
}

/// What the table's Copy puts on the clipboard for a feature: every attribute, `name: value`,
/// one per line, sorted by name.
pub(crate) fn feature_text(props: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut lines: Vec<String> = props
        .iter()
        .map(|(k, v)| format!("{k}: {}", cell_text(Some(v))))
        .collect();
    lines.sort();
    lines.join("\n")
}

/// What the feature table's export buttons write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportRows {
    /// Every row the search leaves, listed or past the display limit.
    Listed,
    Picked,
}

/// Most rows the feature table lists at once; a search narrows the rest.
const MAX_TABLE_ROWS: usize = 500;

/// The open feature table: which layer, its sort and search, and the feature picked in it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct GisTable {
    pub layer: u64,
    sort: Option<(usize, bool)>,
    query: String,
    /// The source feature picked (zoomed to or copied), outlined on the map.
    pub selected: Option<usize>,
}

impl GisTable {
    pub(crate) fn of(layer: u64) -> Self {
        GisTable {
            layer,
            ..Default::default()
        }
    }
}

/// The paint order of the layers' polygons around the official products: `(below, above)`, each
/// in `settings.gis_layers` order, first underneath.
pub(crate) fn paint_order(settings: &crate::settings::Settings) -> (Vec<u64>, Vec<u64>) {
    let shown = settings
        .gis_layers
        .iter()
        .filter(|l| settings.gis_layer_shown(l.id));
    let (below, above): (Vec<_>, Vec<_>) = shown.partition(|l| l.below);
    (
        below.iter().map(|l| l.id).collect(),
        above.iter().map(|l| l.id).collect(),
    )
}

/// Every source feature with a point or line under world position `at`: a point within
/// `point_tol`, a line within `line_tol` of any segment (world units), features `valid` rejects
/// (outside the view's time) left out. Points before lines (they sit on top), the last drawn
/// first, so the first is what a click opens; each source feature once, with its geometry's name.
pub(crate) fn mark_hits(
    marks: &Marks,
    valid: impl Fn(Option<&usize>) -> bool,
    at: (f64, f64),
    point_tol: f64,
    line_tol: f64,
) -> Vec<(usize, &'static str)> {
    let world = |ll: &[f64; 2]| crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
    let mut out: Vec<(usize, &'static str)> = Vec::new();
    let mut push = |src: Option<&usize>, kind| {
        if let Some(&s) = src {
            if !out.iter().any(|(o, _)| *o == s) {
                out.push((s, kind));
            }
        }
    };
    for (i, p) in marks.points.iter().enumerate().rev() {
        let w = world(p);
        if (w.0 - at.0).hypot(w.1 - at.1) <= point_tol && valid(marks.point_src.get(i)) {
            push(marks.point_src.get(i), "Point");
        }
    }
    for (i, line) in marks.lines.iter().enumerate().rev() {
        let near = line.windows(2).any(|ab| {
            let (a, b) = (world(&ab[0]), world(&ab[1]));
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 {
                (((at.0 - a.0) * dx + (at.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (a.0 + t * dx - at.0).hypot(a.1 + t * dy - at.1) <= line_tol
        });
        if near && valid(marks.line_src.get(i)) {
            push(marks.line_src.get(i), "Line");
        }
    }
    out
}

impl HookEchoApp {
    pub(crate) fn gis_loaded(&self, id: u64) -> Option<&LoadedGis> {
        self.gis.iter().find(|l| l.id == id)
    }

    /// Load every remembered layer (at launch). A file that cannot be read keeps its layer, with
    /// the reason, and is reported once: the person is the only one who can fix a missing file,
    /// and a drive that is merely not mounted should come back next time.
    pub(crate) fn reload_gis_layers(&mut self) {
        let mut failed = Vec::new();
        self.gis = self
            .settings
            .gis_layers
            .iter()
            .map(|c| match load_source(&self.settings, &c.source) {
                Ok(loaded) => LoadedGis::new(c.id, loaded.features),
                Err(e) => {
                    log::warn!("could not reload the GIS layer {}: {e}", c.source);
                    failed.push(format!("{} ({e})", c.name));
                    LoadedGis::failed(c.id, e)
                }
            })
            .collect();
        if !self.gis.iter().all(|l| l.error.is_some()) {
            // Remembered layers should come back on, not sit loaded behind the master toggle.
            self.show_imported_gis = true;
        }
        if !failed.is_empty() {
            self.toast(
                ToastKind::Error,
                format!(
                    "Couldn't reload {} GIS layer{}: {}",
                    failed.len(),
                    if failed.len() == 1 { "" } else { "s" },
                    failed.join("; ")
                ),
            );
        }
        self.rebuild_overlays();
    }

    /// Add what was just imported from `source` as a layer, or refresh the layer already showing
    /// that source (keeping its settings). Returns the layer's ID.
    pub(crate) fn add_gis_import(
        &mut self,
        source: String,
        features: Vec<wxdata::gis::GisFeature>,
    ) -> u64 {
        let id = match self.settings.gis_layers.iter().find(|l| l.source == source) {
            Some(l) => l.id,
            None => self.settings.add_gis_layer(source),
        };
        let loaded = LoadedGis::new(id, features);
        match self.gis.iter_mut().find(|l| l.id == id) {
            Some(slot) => *slot = loaded,
            None => self.gis.push(loaded),
        }
        if let Some(c) = self.settings.gis_layer_mut(id) {
            c.visible = true;
        }
        self.show_imported_gis = true;
        self.gis_selected = Some(id);
        self.rebuild_overlays();
        id
    }

    /// Put the imported layers back as a workspace or scene (`what`) saved them, saying which
    /// layers it names that are no longer imported.
    pub(crate) fn apply_gis_snapshot(&mut self, snap: &crate::settings::GisSnapshot, what: &str) {
        self.show_imported_gis = snap.shown;
        let missing = self.settings.apply_gis_snapshot(snap);
        if !missing.is_empty() {
            let msg = format!(
                "{what} shows GIS layer{} no longer imported: {}. Import the file again to \
                 bring it back as a new layer.",
                if missing.len() == 1 { "" } else { "s" },
                missing.join(", ")
            );
            log::warn!("{msg}");
            self.toast(ToastKind::Error, msg);
        }
        self.rebuild_overlays();
    }

    /// Remove a layer and what was read from it.
    pub(crate) fn remove_gis(&mut self, id: u64) {
        self.settings.remove_gis_layer(id);
        self.gis.retain(|l| l.id != id);
        if self.gis_selected == Some(id) {
            self.gis_selected = None;
        }
        self.rebuild_overlays();
    }

    /// Every frame: keep the loaded layers matched to the configured ones (a settings bundle can
    /// replace them wholesale), and each layer's time filter and colours in step with the view,
    /// rebuilding the overlays only when what is drawn changes.
    pub(crate) fn sync_gis_layers(&mut self) {
        let mut rebuild = false;
        let configured: Vec<u64> = self.settings.gis_layers.iter().map(|l| l.id).collect();
        if self.gis.len() != configured.len()
            || self.gis.iter().any(|l| !configured.contains(&l.id))
        {
            self.gis.retain(|l| configured.contains(&l.id));
            for c in &self.settings.gis_layers {
                if !self.gis.iter().any(|l| l.id == c.id) {
                    self.gis.push(match load_source(&self.settings, &c.source) {
                        Ok(loaded) => LoadedGis::new(c.id, loaded.features),
                        Err(e) => LoadedGis::failed(c.id, e),
                    });
                }
            }
            rebuild = true;
        }
        let t = self.view_target_time().unwrap_or_else(Utc::now);
        for layer in &mut self.gis {
            if let Some(c) = self.settings.gis_layer(layer.id) {
                rebuild |= layer.sync(c, t);
            }
        }
        // Visibility, order, style and groups are settings: a change to any redraws.
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for c in &self.settings.gis_layers {
                c.id.hash(&mut h);
                self.settings.gis_layer_shown(c.id).hash(&mut h);
                c.below.hash(&mut h);
                c.style.color.hash(&mut h);
                c.style.opacity.to_bits().hash(&mut h);
                // Polygon outlines and fills are tessellated: their width, dash and fill too.
                c.style.stroke_width.to_bits().hash(&mut h);
                c.style.dash.hash(&mut h);
                c.style.fill_color.hash(&mut h);
                c.style.fill_opacity.to_bits().hash(&mut h);
            }
            self.show_imported_gis.hash(&mut h);
            h.finish()
        };
        if key != self.gis_settings_key {
            self.gis_settings_key = key;
            rebuild = true;
        }
        if rebuild {
            self.rebuild_overlays();
        }
    }

    /// The layers' polygons in paint order, each with its layer: `(below, above)`.
    pub(crate) fn gis_overlay_parts(&self) -> [Vec<(GeoFeature, (u64, usize))>; 2] {
        if !self.show_imported_gis {
            return [Vec::new(), Vec::new()];
        }
        let (below, above) = paint_order(&self.settings);
        let part = |ids: Vec<u64>| -> Vec<(GeoFeature, (u64, usize))> {
            ids.into_iter()
                .filter_map(|id| Some((id, self.settings.gis_layer(id)?, self.gis_loaded(id)?)))
                .flat_map(|(id, c, l)| {
                    l.styled_shapes(c)
                        .into_iter()
                        .map(move |(f, src)| (f, (id, src)))
                })
                .collect()
        };
        [part(below), part(above)]
    }

    /// Per overlay feature, the outline (width and dash) of the imported layer it belongs to
    /// while that layer shows at `zoom`, `None` when it is hidden there; official features read
    /// nothing from it.
    pub(crate) fn overlay_imported_px(
        &self,
        zoom: f64,
    ) -> Vec<Option<crate::overlay_build::ImportedStroke>> {
        self.overlay_layer
            .iter()
            .map(|l| {
                let c = self.settings.gis_layer((*l)?.0)?;
                let width_px = c.style.rendered_stroke_width();
                c.style
                    .visible_at(zoom)
                    .then(|| crate::overlay_build::ImportedStroke {
                        width_px,
                        dash_px: c.style.dash.pattern(width_px),
                    })
            })
            .collect()
    }

    /// Which layers show at `zoom`, as one number: a change re-tessellates.
    pub(crate) fn gis_zoom_key(&self, zoom: f64) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for l in &self.settings.gis_layers {
            (l.id, l.style.visible_at(zoom)).hash(&mut h);
        }
        h.finish()
    }

    /// The overlay features under `(lon, lat)`, highest click-priority first, leaving out an
    /// imported layer hidden below its minimum zoom (it is not there to click).
    pub(crate) fn overlay_hits(&self, lon: f64, lat: f64, zoom: f64) -> Vec<&GeoFeature> {
        let mut hits: Vec<&GeoFeature> = self
            .overlays
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                f.kind != wxdata::overlay::FeatureKind::Boundary && f.contains(lon, lat)
            })
            .filter(|(i, _)| {
                self.overlay_layer
                    .get(*i)
                    .copied()
                    .flatten()
                    .is_none_or(|(id, _)| {
                        self.settings
                            .gis_layer(id)
                            .is_some_and(|c| c.style.visible_at(zoom))
                    })
            })
            .map(|(_, f)| f)
            .collect();
        hits.sort_by_key(|f| std::cmp::Reverse(f.kind.z()));
        hits
    }

    /// The imported layer and source feature of the overlay feature a click on `(lon, lat)`
    /// opens (the first of [`Self::overlay_hits`]), when it is an imported one.
    pub(crate) fn overlay_hit_source(&self, lon: f64, lat: f64, zoom: f64) -> Option<(u64, usize)> {
        let top = *self.overlay_hits(lon, lat, zoom).first()?;
        let i = self.overlays.iter().position(|f| std::ptr::eq(f, top))?;
        self.overlay_layer.get(i).copied().flatten()
    }

    /// A feature picked on the map becomes the picked row of its layer's open feature table, so
    /// the table and the map point at the same source feature.
    pub(crate) fn note_gis_pick(&mut self, layer: u64, src: usize) {
        if let Some(t) = self.gis_table.as_mut().filter(|t| t.layer == layer) {
            t.selected = Some(src);
        }
    }

    /// Frame the active pane on one layer, or on every layer with `None`. A file covering
    /// somewhere the map isn't looking otherwise imports to no visible effect at all.
    pub(crate) fn zoom_to_gis(&mut self, id: Option<u64>) {
        let mut acc: Option<(f64, f64, f64, f64)> = None;
        for l in self.gis.iter().filter(|l| id.is_none_or(|i| i == l.id)) {
            if let Some((w, s, e, n)) = crate::gis_import::bounds(&l.shapes, &l.marks) {
                acc = Some(match acc {
                    None => (w, s, e, n),
                    Some((a, b, c, d)) => (a.min(w), b.min(s), c.max(e), d.max(n)),
                });
            }
        }
        let Some((west, south, east, north)) = acc else {
            self.toast(
                ToastKind::Error,
                "No imported shapes to zoom to".to_string(),
            );
            return;
        };
        self.fit_view((west, south, east, north));
    }

    /// Frame the active pane on a `(west, south, east, north)` box.
    pub(crate) fn fit_view(&mut self, (west, south, east, north): (f64, f64, f64, f64)) {
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

    /// The layers whose points, lines and labels paint at `zoom`, in paint order.
    fn gis_marks_shown(&self, zoom: f64) -> Vec<(&crate::settings::GisLayerConfig, &LoadedGis)> {
        if !self.show_imported_gis {
            return Vec::new();
        }
        let (below, above) = paint_order(&self.settings);
        below
            .into_iter()
            .chain(above)
            .filter_map(|id| Some((self.settings.gis_layer(id)?, self.gis_loaded(id)?)))
            .filter(|(c, _)| c.style.visible_at(zoom))
            .collect()
    }

    /// Every imported point or line under `(lon, lat)` on a map at `cam`, topmost layer first,
    /// each as the popup it opens: its title, every attribute, and its layer's colour. Within the
    /// drawn symbol, plus a few pixels for a finger or an unsteady hand.
    pub(crate) fn gis_mark_hits(
        &self,
        lon: f64,
        lat: f64,
        cam: &crate::render::mercator::Camera,
        touch: bool,
    ) -> Vec<(Detail, u64, usize)> {
        let at = crate::render::mercator::lonlat_to_world(lon, lat);
        let slack = if touch { 12.0 } else { 4.0 };
        let px = cam.world_per_pixel();
        let mut out = Vec::new();
        for (config, layer) in self.gis_marks_shown(cam.zoom).into_iter().rev() {
            let width = f64::from(config.style.rendered_stroke_width());
            let point_tol = (f64::from(config.style.point_radius()) + slack) * px;
            let line_tol = (width / 2.0 + slack) * px;
            for (src, kind) in mark_hits(&layer.marks, |s| layer.valid(s), at, point_tol, line_tol)
            {
                let Some(props) = layer.marks.props.get(src) else {
                    continue;
                };
                out.push((
                    Detail {
                        title: crate::gis_import::props_title(props, kind),
                        body: format!(
                            "{}\n\nLayer: {}",
                            crate::gis_import::props_detail(props),
                            config.name
                        ),
                        color: config.style.stroke_rgba(),
                        image: None,
                        link: None,
                    },
                    config.id,
                    src,
                ));
            }
        }
        out
    }

    /// The imported layer and source feature `f` (one of `self.overlays`) was drawn from.
    pub(crate) fn overlay_source_of(&self, f: &GeoFeature) -> Option<(u64, usize)> {
        let i = self.overlays.iter().position(|o| std::ptr::eq(o, f))?;
        self.overlay_layer.get(i).copied().flatten()
    }

    /// Everything the map's GeoJSON export writes (ROADMAP_NEW I6): annotations, markers, zones,
    /// storm cells, the official overlays shown, and the imported layers' features.
    pub(crate) fn map_export_features(&self) -> Vec<wxdata::gis::GisFeature> {
        crate::gis_export::to_features(&crate::gis_export::MapContents {
            strokes: &self.strokes,
            markers: &self.settings.markers,
            zones: &self.settings.alert_polygons,
            cells: self.active_storm_cells(),
            overlays: &self.official_overlays(),
            imported: &self.gis_export_features(),
            tracks: &self
                .storm_tracks
                .tracks
                .iter()
                .flat_map(crate::app::storm_track::ManualTrack::to_features)
                .collect::<Vec<_>>(),
            routes: &self.route_window.routes,
            route_selected: self.route_window.selected,
            route_engine: self.settings.route_engine.label(),
            contours: &self.contour_features(),
        })
    }

    /// Every target from the layers marked as targets and shown, features valid at the view's
    /// time, bounded by [`MAX_POINT_TARGETS`] and [`MAX_AREA_TARGETS`]; and how many were left
    /// out past those bounds.
    pub(crate) fn impact_targets(&self) -> (Vec<Target>, usize) {
        let mut out = Vec::new();
        let (mut points, mut areas, mut dropped) = (0, 0, 0);
        for c in
            self.settings.gis_layers.iter().filter(|c| {
                c.targets && self.show_imported_gis && self.settings.gis_layer_shown(c.id)
            })
        {
            let Some(l) = self.gis_loaded(c.id) else {
                continue;
            };
            let name = |src: Option<&usize>, kind: &str| {
                let props = src.and_then(|&s| l.marks.props.get(s));
                let by_label = props.and_then(|p| crate::gis_import::feature_label(c, p));
                by_label.unwrap_or_else(|| {
                    props.map_or_else(
                        || kind.to_string(),
                        |p| crate::gis_import::props_title(p, kind),
                    )
                })
            };
            for (i, p) in l.marks.points.iter().enumerate() {
                let src = l.marks.point_src.get(i);
                if !l.valid(src) {
                    continue;
                }
                if points == MAX_POINT_TARGETS {
                    dropped += 1;
                    continue;
                }
                points += 1;
                out.push(Target {
                    name: name(src, "Point"),
                    layer: c.name.clone(),
                    shape: TargetShape::Point(*p),
                });
            }
            for (i, f) in l.shapes.iter().enumerate() {
                let src = l.marks.shape_src.get(i);
                let Some(ring) = f.rings.first().filter(|r| r.len() >= 3) else {
                    continue;
                };
                if !l.valid(src) {
                    continue;
                }
                if areas == MAX_AREA_TARGETS {
                    dropped += 1;
                    continue;
                }
                areas += 1;
                out.push(Target {
                    name: name(src, "Area"),
                    layer: c.name.clone(),
                    shape: TargetShape::Area(ring.clone()),
                });
            }
        }
        (out, dropped)
    }

    /// The overlays that are official products, not imported layers: an export writes imported
    /// layers from their files' own attributes ([`Self::gis_export_features`]), not as overlay text.
    pub(crate) fn official_overlays(&self) -> Vec<GeoFeature> {
        self.overlays
            .iter()
            .zip(&self.overlay_layer)
            .filter(|(_, l)| l.is_none())
            .map(|(f, _)| f.clone())
            .collect()
    }

    /// Every imported feature as the active pane shows it — its layer on and above its minimum
    /// zoom, the feature valid at the view's time — with the attributes its file gave it, plus
    /// `hookecho: "imported"` and the layer's name, for the map's GeoJSON export.
    pub(crate) fn gis_export_features(&self) -> Vec<wxdata::gis::GisFeature> {
        let zoom = self.views[self.active].camera.zoom;
        self.gis_marks_shown(zoom)
            .into_iter()
            .flat_map(|(config, layer)| layer_features(config, layer))
            .collect()
    }

    /// Write one layer's shown features — valid at the view's time and passing its filter — as
    /// GeoJSON with their own attributes (ROADMAP_PARITY M4.4).
    pub(crate) fn export_gis_layer(&mut self, id: u64) {
        self.export_gis_features(id, None, "shown");
    }

    /// Write layer `id`'s shown features as GeoJSON — all of them, or only the source features in
    /// `only` (a feature table's searched rows, or its picked one) — named `<layer>-<what>`.
    pub(crate) fn export_gis_features(&mut self, id: u64, only: Option<&[usize]>, what: &str) {
        let (Some(config), Some(layer)) = (self.settings.gis_layer(id), self.gis_loaded(id)) else {
            return;
        };
        let features = match only {
            None => layer_features(config, layer),
            Some(srcs) => {
                let set: std::collections::HashSet<usize> = srcs.iter().copied().collect();
                layer_features_of(config, layer, |s| set.contains(&s))
            }
        };
        if features.is_empty() {
            self.toast(ToastKind::Error, "No features to export".to_string());
            return;
        }
        let name = format!(
            "{}-{what}.geojson",
            config
                .name
                .rsplit_once('.')
                .map_or(config.name.as_str(), |(stem, _)| stem)
        );
        let n = features.len();
        let json = wxdata::gis::to_geojson(&features);
        match crate::dialog::save_bytes(&name, "geojson", json.as_bytes()) {
            crate::dialog::Saved::Where(w) => {
                self.toast(ToastKind::Success, format!("Exported {n} features to {w}"))
            }
            crate::dialog::Saved::Failed(e) => {
                self.toast(ToastKind::Error, format!("Layer export failed: {e}"))
            }
            crate::dialog::Saved::Cancelled => {}
        }
    }

    /// The feature table window (ROADMAP_PARITY M4.3), while one is open: the layer's shown
    /// features, a column per attribute, sortable by any, searchable, each with Zoom and Copy.
    /// Buttons rather than row clicks, which a touch screen does not deliver in a scrolled table.
    pub(crate) fn gis_table_window(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.gis_table.take() else {
            return;
        };
        let (Some(config), Some(layer)) =
            (self.settings.gis_layer(st.layer), self.gis_loaded(st.layer))
        else {
            return;
        };
        let name = config.name.clone();
        let keys = crate::gis_import::label_keys(&layer.marks);
        let total = (0..layer.marks.props.len())
            .filter(|i| layer.valid(Some(i)))
            .count();
        let rows = feature_table(layer, &keys, &st.query, st.sort);
        let mut open = true;
        let (mut zoom, mut copy, mut export) = (None, None, None);
        egui::Window::new(format!("{name} \u{2014} features"))
            .id(egui::Id::new("gis_feature_table"))
            .open(&mut open)
            .default_size([560.0, 360.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Search");
                    ui.add(egui::TextEdit::singleline(&mut st.query).desired_width(180.0));
                    ui.weak(format!(
                        "{} of {total} shown features (filter and time applied)",
                        rows.len()
                    ));
                });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!rows.is_empty(), egui::Button::new("Export these rows"))
                        .on_hover_text(
                            "These rows' features as GeoJSON, with their own attributes \
                             (all of them, not only the ones listed)",
                        )
                        .clicked()
                    {
                        export = Some(ExportRows::Listed);
                    }
                    if ui
                        .add_enabled(st.selected.is_some(), egui::Button::new("Export picked"))
                        .on_hover_text("The picked feature as GeoJSON")
                        .clicked()
                    {
                        export = Some(ExportRows::Picked);
                    }
                });
                egui::ScrollArea::both().show(ui, |ui| {
                    egui::Grid::new("gis_feature_grid")
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("");
                            for (c, k) in keys.iter().enumerate() {
                                let arrow = match st.sort {
                                    Some((sc, false)) if sc == c => " \u{25b2}",
                                    Some((sc, true)) if sc == c => " \u{25bc}",
                                    _ => "",
                                };
                                if ui
                                    .small_button(format!("{k}{arrow}"))
                                    .on_hover_text("Sort by this attribute; again to reverse")
                                    .clicked()
                                {
                                    st.sort = match st.sort {
                                        Some((sc, false)) if sc == c => Some((c, true)),
                                        Some((sc, true)) if sc == c => None,
                                        _ => Some((c, false)),
                                    };
                                }
                            }
                            ui.end_row();
                            for r in rows.iter().take(MAX_TABLE_ROWS) {
                                ui.horizontal(|ui| {
                                    let picked = st.selected == Some(r.src);
                                    if ui
                                        .selectable_label(picked, "\u{2316}")
                                        .on_hover_text("Zoom to this feature and outline it")
                                        .clicked()
                                    {
                                        zoom = Some(r.src);
                                    }
                                    if ui
                                        .small_button("\u{29c9}")
                                        .on_hover_text("Copy its attributes")
                                        .clicked()
                                    {
                                        copy = Some(r.src);
                                    }
                                });
                                for v in &r.values {
                                    let short: String = v.chars().take(40).collect();
                                    let cell = ui.label(&short);
                                    if short.len() < v.len() {
                                        cell.on_hover_text(v);
                                    }
                                }
                                ui.end_row();
                            }
                        });
                    if rows.len() > MAX_TABLE_ROWS {
                        ui.weak(format!(
                            "The first {MAX_TABLE_ROWS} of {} rows; search to narrow them",
                            rows.len()
                        ));
                    }
                });
            });
        match export {
            Some(ExportRows::Listed) => {
                let srcs: Vec<usize> = rows.iter().map(|r| r.src).collect();
                let what = if st.query.trim().is_empty() {
                    "listed"
                } else {
                    "search"
                };
                self.export_gis_features(st.layer, Some(&srcs), what);
            }
            Some(ExportRows::Picked) => {
                if let Some(src) = st.selected {
                    self.export_gis_features(st.layer, Some(&[src]), "picked");
                }
            }
            None => {}
        }
        if let Some(src) = copy {
            if let Some(props) = self
                .gis_loaded(st.layer)
                .and_then(|l| l.marks.props.get(src))
            {
                ctx.copy_text(feature_text(props));
            }
            st.selected = Some(src);
        }
        if let Some(src) = zoom {
            if let Some(b) = self
                .gis_loaded(st.layer)
                .and_then(|l| feature_bounds(l, src))
            {
                self.fit_view(b);
            }
            st.selected = Some(src);
        }
        if open {
            self.gis_table = Some(st);
        }
    }

    /// The feature picked in the table, outlined over everything so it can be found.
    pub(crate) fn paint_gis_selection(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
    ) {
        let Some((layer, src)) = self
            .gis_table
            .as_ref()
            .and_then(|t| Some((self.gis_loaded(t.layer)?, t.selected?)))
        else {
            return;
        };
        let screen = |ll: &[f64; 2]| {
            let w = crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
            let (sx, sy) = cam.world_to_screen(w, vp);
            egui::pos2(prect.left() + sx, prect.top() + sy)
        };
        let halo = egui::Stroke::new(5.0, egui::Color32::from_black_alpha(200));
        let mark = egui::Stroke::new(2.5, egui::Color32::from_rgb(255, 230, 60));
        let m = &layer.marks;
        let mut lines: Vec<Vec<egui::Pos2>> = Vec::new();
        for (i, f) in layer.shapes.iter().enumerate() {
            if m.shape_src.get(i) == Some(&src) {
                lines.extend(f.rings.iter().map(|r| r.iter().map(screen).collect()));
            }
        }
        for (i, l) in m.lines.iter().enumerate() {
            if m.line_src.get(i) == Some(&src) {
                lines.push(l.iter().map(screen).collect());
            }
        }
        for pts in lines {
            painter.add(egui::Shape::line(pts.clone(), halo));
            painter.add(egui::Shape::line(pts, mark));
        }
        for (i, p) in m.points.iter().enumerate() {
            if m.point_src.get(i) == Some(&src) {
                let at = screen(p);
                painter.circle_stroke(at, 9.0, halo);
                painter.circle_stroke(at, 9.0, mark);
            }
        }
    }

    /// Points and lines of every shown layer, each in its own style.
    pub(crate) fn paint_gis_marks(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
    ) {
        let screen = |ll: &[f64; 2]| {
            let w = crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
            let (sx, sy) = cam.world_to_screen(w, vp);
            egui::pos2(prect.left() + sx, prect.top() + sy)
        };
        for (config, layer) in self.gis_marks_shown(cam.zoom) {
            let marks = &layer.marks;
            if marks.is_empty() {
                continue;
            }
            let style = config.style;
            let c = style.stroke_rgba();
            let layer_color = egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
            // A mark coloured by attribute keeps the layer's opacity.
            let colors = layer.colors.as_ref().map(|(_, c, _)| c);
            let color_of = |src: Option<&usize>| {
                colors
                    .and_then(|c| *c.get(*src?)?)
                    .map_or(layer_color, |[r, g, b]| {
                        egui::Color32::from_rgba_unmultiplied(r, g, b, c[3])
                    })
            };
            let width = style.rendered_stroke_width();
            let dash = style.dash.pattern(width);
            for (i, line) in marks.lines.iter().enumerate() {
                if !layer.valid(marks.line_src.get(i)) {
                    continue;
                }
                let pts: Vec<egui::Pos2> = line.iter().map(screen).collect();
                let color = color_of(marks.line_src.get(i));
                match (style.dash, dash) {
                    (crate::settings::LineDash::Dotted, Some((_, gap))) => {
                        painter.extend(egui::Shape::dotted_line(
                            &pts,
                            color,
                            gap + width,
                            width * 0.5 + 0.25,
                        ));
                    }
                    (_, Some((on, off))) => {
                        let stroke = egui::Stroke::new(width, color);
                        painter.extend(egui::Shape::dashed_line(&pts, stroke, on, off));
                    }
                    (_, None) => {
                        painter.add(egui::Shape::line(pts, egui::Stroke::new(width, color)));
                    }
                }
            }
            for (i, point) in marks.points.iter().enumerate() {
                if !layer.valid(marks.point_src.get(i)) {
                    continue;
                }
                let p = screen(point);
                if !prect.contains(p) {
                    continue;
                }
                let color = color_of(marks.point_src.get(i));
                // Outlined rather than a plain dot: an imported site has to stay visible over
                // both a bright radar core and a dark basemap, which one flat color cannot
                // manage. Without a size of its own, the outline width scales point symbols so
                // a mixed-geometry file keeps one coherent visual weight.
                let symbol = layer
                    .symbols
                    .as_ref()
                    .and_then(|(_, s, _, _)| *s.get(*marks.point_src.get(i)?)?)
                    .unwrap_or(style.symbol);
                paint_symbol(painter, p, symbol, style.point_radius(), color);
            }
        }
    }

    /// Labels of every shown layer with a label attribute, through the frame's shared label
    /// placer (`crate::labelplace`) at the lowest priority: storm IDs, towns, stations and gauges
    /// keep their places, and a GIS label never sits on one (1008.md D2). Among the imported
    /// layers, those marked "labels first" place before the rest, then the topmost layer first;
    /// within a layer, labels shown last frame are offered their places first so names do not
    /// flicker while the map pans.
    pub(crate) fn paint_gis_labels(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        placer: &mut crate::labelplace::Placer,
    ) {
        let font = egui::FontId::proportional(11.5);
        let mut drawn = 0;
        let shown = self.gis_marks_shown(cam.zoom);
        for k in label_layer_order(
            &shown
                .iter()
                .map(|(c, _)| (c.id, c.labels_first))
                .collect::<Vec<_>>(),
        ) {
            let (config, layer) = shown[k];
            if !crate::gis_import::labels(config) {
                continue;
            }
            let c = config.style.stroke_rgba();
            let text_color = egui::Color32::from_rgb(
                c[0].saturating_add(90),
                c[1].saturating_add(90),
                c[2].saturating_add(90),
            );
            let key = |src: usize| crate::labelplace::key(&format!("gis:{}:{src}", config.id));
            let mut anchors: Vec<([f64; 2], usize)> = layer.marks.anchors.clone();
            anchors.sort_by_key(|(_, src)| !placer.was_shown(key(*src)));
            for (at, src) in anchors {
                if !layer.valid(Some(&src)) {
                    continue;
                }
                if drawn >= 600 {
                    return;
                }
                let w = crate::render::mercator::lonlat_to_world(at[0], at[1]);
                let (sx, sy) = cam.world_to_screen(w, vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.shrink(4.0).contains(p) {
                    continue;
                }
                let Some(text) = layer
                    .marks
                    .props
                    .get(src)
                    .and_then(|props| crate::gis_import::feature_label(config, props))
                else {
                    continue;
                };
                let galley = painter.layout_no_wrap(text, font.clone(), text_color);
                // Beside a point's dot, centred on a line or polygon's anchor; a dark halo keeps
                // it legible over radar and basemap alike.
                let pos = p + egui::vec2(6.0, -galley.size().y * 0.5);
                let rect = egui::Rect::from_min_size(pos, galley.size()).expand(1.5);
                if !placer.place(key(src), rect, crate::labelplace::Priority::Minor) {
                    continue;
                }
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
    }
}

/// The order imported layers place their labels in, as indices into `layers` (`(id, labels
/// first)`, bottom layer first as painted): "labels first" layers, then the rest, each group
/// topmost layer first.
pub(crate) fn label_layer_order(layers: &[(u64, bool)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..layers.len()).rev().collect();
    // Stable: within each group the topmost-first order stays.
    order.sort_by_key(|&k| !layers[k].1);
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{GisLayerConfig, Settings};

    #[test]
    fn labels_first_layers_place_before_the_others_then_the_topmost() {
        // Painted bottom to top: roads, hospitals (labels first), towns, schools (labels first).
        let layers = [(1, false), (2, true), (3, false), (4, true)];
        let order: Vec<u64> = label_layer_order(&layers)
            .into_iter()
            .map(|k| layers[k].0)
            .collect();
        assert_eq!(order, [4, 2, 3, 1]);
        assert_eq!(label_layer_order(&[]), Vec::<usize>::new());
        // A label placed first keeps its spot; a later layer's overlapping label is refused,
        // and nothing imported can take a town's spot.
        let mut placer = crate::labelplace::Placer::default();
        placer.begin();
        let r = |x: f32| egui::Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(40.0, 12.0));
        assert!(placer.place(9, r(0.0), crate::labelplace::Priority::Place));
        assert!(!placer.place(1, r(10.0), crate::labelplace::Priority::Minor));
        assert!(placer.place(2, r(100.0), crate::labelplace::Priority::Minor));
        assert!(!placer.place(3, r(110.0), crate::labelplace::Priority::Minor));
    }

    #[test]
    fn layers_paint_in_their_own_order_on_their_own_side_and_only_when_shown() {
        let mut s = Settings::default();
        let a = s.add_gis_layer("a.geojson".into());
        let b = s.add_gis_layer("b.geojson".into());
        let c = s.add_gis_layer("c.geojson".into());
        s.gis_layer_mut(b).unwrap().below = true;
        assert_eq!(paint_order(&s), (vec![b], vec![a, c]));
        s.move_gis_layer(c, -2);
        assert_eq!(paint_order(&s), (vec![b], vec![c, a]));
        s.gis_layer_mut(a).unwrap().visible = false;
        assert_eq!(paint_order(&s), (vec![b], vec![c]));
    }

    #[test]
    fn a_layer_keeps_its_own_time_filter_and_colours() {
        let feature = |name: &str, start: &str| wxdata::gis::GisFeature {
            geometry: wxdata::gis::Geometry::Point([-97.0, 35.0]),
            properties: serde_json::json!({ "NAME": name, "START": start, "POP": 5 })
                .as_object()
                .unwrap()
                .clone(),
        };
        let mut layer = LoadedGis::new(
            1,
            vec![
                feature("early", "2026-05-06T20:00:00Z"),
                feature("late", "2026-05-06T22:00:00Z"),
            ],
        );
        let t = chrono::DateTime::parse_from_rfc3339("2026-05-06T21:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut config = GisLayerConfig {
            id: 1,
            ..Default::default()
        };
        assert!(!layer.sync(&config, t), "no mapping, nothing to filter");
        assert!(layer.valid(Some(&1)));
        config.time_start = Some("START".into());
        assert!(layer.sync(&config, t));
        assert!(layer.valid(Some(&0)) && !layer.valid(Some(&1)));
        assert!(!layer.sync(&config, t), "the same time changes nothing");
        config.color_by = Some("POP".into());
        assert!(layer.sync(&config, t));
        assert_eq!(layer.colors.as_ref().unwrap().0, "POP");
        config.color_by = None;
        assert!(layer.sync(&config, t) && layer.colors.is_none());
    }

    #[test]
    fn a_click_finds_the_point_or_line_under_it_and_not_one_out_of_its_time() {
        let at = |lon: f64, lat: f64| crate::render::mercator::lonlat_to_world(lon, lat);
        let features = vec![
            wxdata::gis::GisFeature {
                geometry: wxdata::gis::Geometry::LineString(vec![[-98.0, 35.0], [-96.0, 35.0]]),
                properties: Default::default(),
            },
            wxdata::gis::GisFeature {
                geometry: wxdata::gis::Geometry::Point([-97.0, 35.0]),
                properties: Default::default(),
            },
        ];
        let (_, marks) = crate::gis_import::to_renderable(features);
        let tol = 1e-5; // about 2 km of world at these latitudes
        let all = |_: Option<&usize>| true;
        let first = |m: &Marks, v: &dyn Fn(Option<&usize>) -> bool, a, p, l| {
            mark_hits(m, v, a, p, l).into_iter().next()
        };
        // On the point (which also sits on the line): the point, drawn on top, wins.
        assert_eq!(
            first(&marks, &all, at(-97.0, 35.0), tol, tol),
            Some((1, "Point"))
        );
        // Along the line away from the point.
        assert_eq!(
            first(&marks, &all, at(-96.5, 35.001), tol, tol),
            Some((0, "Line"))
        );
        // Off both.
        assert_eq!(first(&marks, &all, at(-96.5, 35.5), tol, tol), None);
        // The point outside the view's time is not there to click; the line under it is.
        let not_point = |s: Option<&usize>| s != Some(&1);
        assert_eq!(
            first(&marks, &not_point, at(-97.0, 35.0), tol, tol),
            Some((0, "Line"))
        );
        // Everything under the point, for the chooser: the point, then the line it sits on.
        assert_eq!(
            mark_hits(&marks, all, at(-97.0, 35.0), tol, tol),
            [(1, "Point"), (0, "Line")]
        );
    }

    /// A filter decides what is shown together with the time window; one that does not parse
    /// keeps the previous one in force and says why; a feature missing the attribute is hidden.
    #[test]
    fn a_layer_filter_and_its_time_window_decide_together() {
        let feature = |name: &str, pop: Option<i64>, start: &str| {
            let mut p = serde_json::json!({ "NAME": name, "START": start });
            if let Some(v) = pop {
                p["POP"] = v.into();
            }
            wxdata::gis::GisFeature {
                geometry: wxdata::gis::Geometry::Point([-97.0, 35.0]),
                properties: p.as_object().unwrap().clone(),
            }
        };
        let mut layer = LoadedGis::new(
            1,
            vec![
                feature("big early", Some(5000), "2026-05-06T20:00:00Z"),
                feature("small", Some(10), "2026-05-06T20:00:00Z"),
                feature("big late", Some(9000), "2026-05-06T22:00:00Z"),
                feature("unknown", None, "2026-05-06T20:00:00Z"),
            ],
        );
        let t = chrono::DateTime::parse_from_rfc3339("2026-05-06T21:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut config = GisLayerConfig {
            id: 1,
            filter: "POP > 1000".into(),
            ..Default::default()
        };
        assert!(layer.sync(&config, t));
        let shown = |l: &LoadedGis| (0..4).map(|i| l.valid(Some(&i))).collect::<Vec<_>>();
        assert_eq!(
            shown(&layer),
            [true, false, true, false],
            "the missing POP is hidden"
        );
        config.time_start = Some("START".into());
        layer.sync(&config, t);
        assert_eq!(
            shown(&layer),
            [true, false, false, false],
            "and only those valid now"
        );
        // A typo keeps the last good filter and says why.
        config.filter = "POP >".into();
        assert!(!layer.sync(&config, t));
        assert!(layer
            .filter_error
            .as_deref()
            .unwrap()
            .contains("expected a value"));
        assert_eq!(shown(&layer), [true, false, false, false]);
        config.filter.clear();
        config.time_start = None;
        assert!(layer.sync(&config, t));
        assert!(layer.filter_error.is_none() && layer.shown.is_none());
        // An export of the layer writes exactly what it shows, with each feature's attributes.
        config.filter = "POP > 1000".into();
        layer.sync(&config, t);
        let out = layer_features(&config, &layer);
        let names: Vec<&str> = out
            .iter()
            .map(|f| f.properties["NAME"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["big early", "big late"]);
        assert_eq!(out[0].properties["hookecho"], "imported");
        // A table's rows (or its picked one): only those features, and still only shown ones —
        // a hidden feature asked for by number is not written.
        let names_of = |srcs: &[usize]| -> Vec<String> {
            layer_features_of(&config, &layer, |s| srcs.contains(&s))
                .iter()
                .map(|f| f.properties["NAME"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(names_of(&[2]), ["big late"]);
        assert_eq!(names_of(&[1, 2]), ["big late"], "feature 1 is filtered out");
        assert!(names_of(&[]).is_empty());
    }

    /// The table lists what the layer shows, searches any column, sorts numbers as numbers and
    /// puts missing values last; a feature's box covers all of its parts.
    #[test]
    fn the_feature_table_lists_what_the_map_shows_sorted_and_searched() {
        let f = |name: &str, pop: Option<i64>, kind: &str| {
            let mut p = serde_json::json!({ "NAME": name, "KIND": kind });
            if let Some(v) = pop {
                p["POP"] = v.into();
            }
            wxdata::gis::GisFeature {
                geometry: wxdata::gis::Geometry::MultiPoint(vec![[-97.0, 35.0], [-96.0, 36.0]]),
                properties: p.as_object().unwrap().clone(),
            }
        };
        let mut layer = LoadedGis::new(
            1,
            vec![
                f("Alpha", Some(900), "school"),
                f("Bravo", Some(10_000), "fire"),
                f("Charlie", None, "school"),
                f("Delta", Some(95), "school"),
            ],
        );
        let keys = crate::gis_import::label_keys(&layer.marks);
        assert_eq!(keys, ["KIND", "NAME", "POP"]);
        let names =
            |rows: &[TableRow]| rows.iter().map(|r| r.values[1].clone()).collect::<Vec<_>>();
        let pop = 2;
        // Numbers as numbers (95 < 900 < 10000), the missing one last either way.
        assert_eq!(
            names(&feature_table(&layer, &keys, "", Some((pop, false)))),
            ["Delta", "Alpha", "Bravo", "Charlie"]
        );
        assert_eq!(
            names(&feature_table(&layer, &keys, "", Some((pop, true)))),
            ["Bravo", "Alpha", "Delta", "Charlie"]
        );
        assert_eq!(
            names(&feature_table(&layer, &keys, "SCHOOL", None)),
            ["Alpha", "Charlie", "Delta"]
        );
        // The layer's filter applies to the table too.
        let config = crate::settings::GisLayerConfig {
            id: 1,
            filter: "KIND = 'school'".into(),
            ..Default::default()
        };
        layer.sync(&config, Utc::now());
        assert_eq!(
            names(&feature_table(&layer, &keys, "", None)),
            ["Alpha", "Charlie", "Delta"]
        );
        assert_eq!(feature_bounds(&layer, 1), Some((-97.0, 35.0, -96.0, 36.0)));
        assert_eq!(
            feature_text(&layer.marks.props[0]),
            "KIND: school\nNAME: Alpha\nPOP: 900"
        );
    }

    /// Every drawn polygon carries the source feature it came from — both parts of a
    /// multipolygon the same one, a filtered-out feature none — so a click on the map lands on
    /// the table row of the same feature.
    #[test]
    fn drawn_polygons_name_their_source_feature() {
        let square = |x: f64| vec![vec![[x, 35.0], [x + 0.1, 35.0], [x + 0.1, 35.1], [x, 35.0]]];
        let layer_features = vec![
            wxdata::gis::GisFeature {
                geometry: wxdata::gis::Geometry::Polygon(square(-97.0)),
                properties: serde_json::json!({"KEEP": true})
                    .as_object()
                    .unwrap()
                    .clone(),
            },
            wxdata::gis::GisFeature {
                geometry: wxdata::gis::Geometry::MultiPolygon(vec![square(-96.0), square(-95.0)]),
                properties: serde_json::json!({"KEEP": true})
                    .as_object()
                    .unwrap()
                    .clone(),
            },
            wxdata::gis::GisFeature {
                geometry: wxdata::gis::Geometry::Polygon(square(-94.0)),
                properties: serde_json::json!({"KEEP": false})
                    .as_object()
                    .unwrap()
                    .clone(),
            },
        ];
        let mut layer = LoadedGis::new(1, layer_features);
        let config = crate::settings::GisLayerConfig {
            id: 1,
            filter: "KEEP = true".into(),
            ..Default::default()
        };
        layer.sync(&config, Utc::now());
        let srcs: Vec<usize> = layer
            .styled_shapes(&config)
            .into_iter()
            .map(|(_, s)| s)
            .collect();
        assert_eq!(srcs, [0, 1, 1]);
    }

    /// Writes one GeoJSON with every kind of feature the map export carries, to the path in
    /// `HOOKECHO_EXPORT_SAMPLE`, for reading back with an independent GIS reader (GDAL/OGR):
    /// `HOOKECHO_EXPORT_SAMPLE=/tmp/x.geojson cargo test -p hookecho --lib write_export_sample -- --ignored`
    #[test]
    #[ignore = "writes a file for an external reader"]
    fn write_export_sample() {
        let Ok(path) = std::env::var("HOOKECHO_EXPORT_SAMPLE") else {
            return;
        };
        let t0 = chrono::DateTime::parse_from_rfc3339("2026-05-06T21:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let attrs = |v: serde_json::Value| v.as_object().unwrap().clone();
        let imported = vec![
            wxdata::gis::GisFeature {
                geometry: wxdata::gis::Geometry::Point([-97.4, 35.2]),
                properties: attrs(
                    serde_json::json!({"NAME": "Siren 12", "POP": 5000, "hookecho": "imported", "layer": "Sirens"}),
                ),
            },
            wxdata::gis::GisFeature {
                geometry: wxdata::gis::Geometry::LineString(vec![[-97.5, 35.1], [-97.3, 35.3]]),
                properties: attrs(
                    serde_json::json!({"NAME": "Route 9", "hookecho": "imported", "layer": "Roads"}),
                ),
            },
            wxdata::gis::GisFeature {
                geometry: wxdata::gis::Geometry::Polygon(vec![vec![
                    [-97.6, 35.0],
                    [-97.2, 35.0],
                    [-97.2, 35.4],
                    [-97.6, 35.4],
                    [-97.6, 35.0],
                ]]),
                properties: attrs(
                    serde_json::json!({"NAME": "District", "hookecho": "imported", "layer": "Districts"}),
                ),
            },
        ];
        let cell = wxdata::level3::Cell {
            id: "O7".into(),
            lon: -97.5,
            lat: 35.3,
            time: Some(t0),
            mvt_deg: Some(70.0),
            mvt_kt: Some(30.0),
            max_dbz: Some(64.0),
            ..Default::default()
        };
        let tracks = crate::app::storm_track::ManualTrack::from_cell(&cell, t0)
            .unwrap()
            .to_features();
        let routes = vec![wxdata::route::Route {
            coords: vec![[-97.52, 35.47], [-97.40, 35.50], [-97.30, 35.52]],
            distance_m: 21_500.0,
            duration_s: 1_260.0,
            summary: "I-40 E".into(),
        }];
        let contours = crate::app::contours::contour_features(
            "MSLP",
            Some("hPa"),
            Some("HRRR"),
            &crate::app::contours::ContourEntry {
                lines: vec![wxdata::contour::ContourLine {
                    level: 1008.0,
                    pts: vec![(-98.0, 35.0), (-97.0, 35.1), (-96.0, 35.0)],
                    bbox: (-98.0, 35.0, -96.0, 35.1),
                }],
                valid: Some(t0),
                run: Some(t0 - chrono::Duration::hours(1)),
                received: None,
                grid: None,
                last_fetch: None,
                fetched_key: None,
            },
        );
        let features = crate::gis_export::to_features(&crate::gis_export::MapContents {
            strokes: &[],
            markers: &[],
            zones: &[],
            cells: std::slice::from_ref(&cell),
            overlays: &[],
            imported: &imported,
            tracks: &tracks,
            routes: &routes,
            route_selected: 0,
            route_engine: "OSRM",
            contours: &contours,
        });
        std::fs::write(path, wxdata::gis::to_geojson(&features)).unwrap();
    }
}
