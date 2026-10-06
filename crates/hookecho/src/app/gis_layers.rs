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

/// A layer's valid windows and the start/end attributes they were read with.
pub(crate) type ImportedTime = ((Option<String>, Option<String>), TimeBounds);

/// What was read from one layer's file.
pub(crate) struct LoadedGis {
    pub id: u64,
    pub shapes: Vec<GeoFeature>,
    pub marks: Marks,
    /// The features' colours by the layer's colour-by attribute, and their legend.
    pub colors: Option<ColoredBy>,
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
        changed
    }

    /// The polygons to draw for `config`: time-filtered, coloured and styled.
    fn styled_shapes(&self, config: &crate::settings::GisLayerConfig) -> Vec<GeoFeature> {
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
                    style.color = c;
                }
                crate::gis_import::apply_style(&mut feature, style);
                feature
            })
            .collect()
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

/// The point or line of `marks` under world position `at`, if any: a point within `point_tol`, a
/// line within `line_tol` of any segment (world units), points before lines (they sit on top),
/// the last drawn first, features invalid at the view's time left out. Returns the source
/// feature and the geometry's name.
pub(crate) fn mark_at(
    marks: &Marks,
    valid: impl Fn(Option<&usize>) -> bool,
    at: (f64, f64),
    point_tol: f64,
    line_tol: f64,
) -> Option<(usize, &'static str)> {
    let world = |ll: &[f64; 2]| crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
    for (i, p) in marks.points.iter().enumerate().rev() {
        let w = world(p);
        if (w.0 - at.0).hypot(w.1 - at.1) <= point_tol && valid(marks.point_src.get(i)) {
            return marks.point_src.get(i).map(|&s| (s, "Point"));
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
            return marks.line_src.get(i).map(|&s| (s, "Line"));
        }
    }
    None
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
    pub(crate) fn gis_overlay_parts(&self) -> [Vec<(GeoFeature, u64)>; 2] {
        if !self.show_imported_gis {
            return [Vec::new(), Vec::new()];
        }
        let (below, above) = paint_order(&self.settings);
        let part = |ids: Vec<u64>| -> Vec<(GeoFeature, u64)> {
            ids.into_iter()
                .filter_map(|id| Some((id, self.settings.gis_layer(id)?, self.gis_loaded(id)?)))
                .flat_map(|(id, c, l)| l.styled_shapes(c).into_iter().map(move |f| (f, id)))
                .collect()
        };
        [part(below), part(above)]
    }

    /// Per overlay feature, the outline width of the imported layer it belongs to while that
    /// layer shows at `zoom`, `None` when it is hidden there; official features read nothing
    /// from it.
    pub(crate) fn overlay_imported_px(&self, zoom: f64) -> Vec<Option<f32>> {
        self.overlay_layer
            .iter()
            .map(|l| {
                let c = self.settings.gis_layer((*l)?)?;
                c.style
                    .visible_at(zoom)
                    .then(|| c.style.rendered_stroke_width())
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
                    .is_none_or(|id| {
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

    /// The imported point or line under `(lon, lat)` on a map at `cam`, topmost layer first, as
    /// the popup it opens: its title, every attribute, and its layer's colour. Within the drawn
    /// symbol, plus a few pixels for a finger or an unsteady hand.
    pub(crate) fn gis_mark_hit(
        &self,
        lon: f64,
        lat: f64,
        cam: &crate::render::mercator::Camera,
        touch: bool,
    ) -> Option<Detail> {
        let at = crate::render::mercator::lonlat_to_world(lon, lat);
        let slack = if touch { 12.0 } else { 4.0 };
        let px = cam.world_per_pixel();
        for (config, layer) in self.gis_marks_shown(cam.zoom).into_iter().rev() {
            let width = f64::from(config.style.rendered_stroke_width());
            let point_tol = (2.5 + width * 0.625 + slack) * px;
            let line_tol = (width / 2.0 + slack) * px;
            let Some((src, kind)) =
                mark_at(&layer.marks, |s| layer.valid(s), at, point_tol, line_tol)
            else {
                continue;
            };
            let props = layer.marks.props.get(src)?;
            let c = config.style.stroke_rgba();
            return Some(Detail {
                title: crate::gis_import::props_title(props, kind),
                body: format!(
                    "{}\n\nLayer: {}",
                    crate::gis_import::props_detail(props),
                    config.name
                ),
                color: c,
                image: None,
                link: None,
            });
        }
        None
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
                let by_label = c
                    .label
                    .as_deref()
                    .and_then(|k| crate::gis_import::label_text(props?, k));
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
        use wxdata::gis::{Geometry, GisFeature};
        let zoom = self.views[self.active].camera.zoom;
        let mut out = Vec::new();
        for (config, layer) in self.gis_marks_shown(zoom) {
            let m = &layer.marks;
            let props = |src: Option<&usize>| {
                let mut p = src
                    .and_then(|&s| m.props.get(s))
                    .cloned()
                    .unwrap_or_default();
                p.insert("hookecho".into(), "imported".into());
                p.insert("layer".into(), config.name.clone().into());
                p
            };
            for (i, f) in layer.shapes.iter().enumerate() {
                let src = m.shape_src.get(i);
                if layer.valid(src) && !f.rings.is_empty() {
                    out.push(GisFeature {
                        geometry: Geometry::Polygon(f.rings.clone()),
                        properties: props(src),
                    });
                }
            }
            for (i, line) in m.lines.iter().enumerate() {
                let src = m.line_src.get(i);
                if layer.valid(src) {
                    out.push(GisFeature {
                        geometry: Geometry::LineString(line.clone()),
                        properties: props(src),
                    });
                }
            }
            for (i, p) in m.points.iter().enumerate() {
                let src = m.point_src.get(i);
                if layer.valid(src) {
                    out.push(GisFeature {
                        geometry: Geometry::Point(*p),
                        properties: props(src),
                    });
                }
            }
        }
        out
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
            for (i, line) in marks.lines.iter().enumerate() {
                if !layer.valid(marks.line_src.get(i)) {
                    continue;
                }
                let pts: Vec<egui::Pos2> = line.iter().map(screen).collect();
                let color = color_of(marks.line_src.get(i));
                painter.add(egui::Shape::line(pts, egui::Stroke::new(width, color)));
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
                // manage. The outline width also scales point symbols so a mixed-geometry file
                // keeps one coherent visual weight. The default 1.6 px remains a 3.5 px dot.
                let radius = 2.5 + width * 0.625;
                painter.circle_filled(p, radius, color);
                painter.circle_stroke(
                    p,
                    radius,
                    egui::Stroke::new(1.0, egui::Color32::from_black_alpha(180)),
                );
            }
        }
    }

    /// Labels of every shown layer with a label attribute, thinned to one per screen-grid cell
    /// across all layers (topmost layer first, so it wins a contested cell).
    pub(crate) fn paint_gis_labels(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
    ) {
        let font = egui::FontId::proportional(11.5);
        let (cell_w, cell_h) = (90.0_f32, 18.0_f32);
        let mut taken = std::collections::HashSet::new();
        let mut drawn = 0;
        for (config, layer) in self.gis_marks_shown(cam.zoom).into_iter().rev() {
            let Some(key) = config.label.as_deref() else {
                continue;
            };
            let c = config.style.stroke_rgba();
            let text_color = egui::Color32::from_rgb(
                c[0].saturating_add(90),
                c[1].saturating_add(90),
                c[2].saturating_add(90),
            );
            for &(at, src) in &layer.marks.anchors {
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
                let cell = ((p.x / cell_w) as i32, (p.y / cell_h) as i32);
                if !taken.insert(cell) {
                    continue;
                }
                let Some(text) = layer
                    .marks
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{GisLayerConfig, Settings};

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
        // On the point (which also sits on the line): the point, drawn on top, wins.
        assert_eq!(
            mark_at(&marks, all, at(-97.0, 35.0), tol, tol),
            Some((1, "Point"))
        );
        // Along the line away from the point.
        assert_eq!(
            mark_at(&marks, all, at(-96.5, 35.001), tol, tol),
            Some((0, "Line"))
        );
        // Off both.
        assert_eq!(mark_at(&marks, all, at(-96.5, 35.5), tol, tol), None);
        // The point outside the view's time is not there to click; the line under it is.
        let not_point = |s: Option<&usize>| s != Some(&1);
        assert_eq!(
            mark_at(&marks, not_point, at(-97.0, 35.0), tol, tol),
            Some((0, "Line"))
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
    }
}
