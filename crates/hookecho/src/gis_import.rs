//! Converting a generic GIS import (`wxdata::gis`, ROADMAP_NEW I1/I3) into this app's existing
//! renderable overlay shape.
//!
//! `wxdata::overlay::GeoFeature` (rings + fill/stroke + click-through text) is already the one
//! shape every NWS/SPC feed's polygons render and hit-test through — `app.rs`'s `rebuild_overlays`
//! assembles one combined `Vec<GeoFeature>` from all of them, and the map painter/click-tester
//! both work off that alone. Landing an import in that same list means it draws and is clickable
//! with no new rendering code, not a second overlay pipeline.
//!
//! `GeoFeature` is rings-only, so it can only carry the polygon half. Points and lines — a file of
//! city sites, a river or road network, the ordinary contents of a GIS export — come back as
//! [`Marks`] instead and are painted directly by the map, using the same lon/lat → world → screen
//! projection the freehand annotation strokes already use. ROADMAP_NEW I4's first styling slice
//! applies one persistent color/opacity pair to all three geometry families; labels from a chosen
//! attribute and data-driven category styling remain deliberately separate work.

use crate::settings::ImportedGisStyle;
use chrono::{DateTime, Utc};
use wxdata::gis::{Geometry, GisFeature};
use wxdata::overlay::{FeatureKind, GeoFeature};

/// A neutral blue, distinct from every existing feed's own convention (warnings red, watches
/// yellow, SPC risk colors, …) so an imported shape never reads as an official product.
const FILL: [u8; 4] = [80, 140, 220, 60];
const STROKE: [u8; 4] = [80, 140, 220, 220];

/// Recolor one imported polygon at the overlay assembly boundary. The source feature stays in
/// its neutral default style, so changing a slider never mutates imported geometry or attributes.
pub(crate) fn apply_style(feature: &mut GeoFeature, style: ImportedGisStyle) {
    feature.fill = style.fill_rgba();
    feature.stroke = style.stroke_rgba();
}

/// The imported geometry the overlay pipeline can't hold, kept in the app and painted directly.
/// A `MultiPoint`/`MultiLineString` flattens into its parts here — nothing downstream needs to
/// know the file grouped them.
#[derive(Default, Debug, Clone, PartialEq)]
pub(crate) struct Marks {
    pub points: Vec<[f64; 2]>,
    pub lines: Vec<Vec<[f64; 2]>>,
    /// Each source feature's attributes, once (I4): what labels and colours are chosen from.
    pub props: Vec<serde_json::Map<String, serde_json::Value>>,
    /// The feature (an index into `props`) each point, line and polygon part came from, in the
    /// same order as `points`, `lines` and the polygons `to_renderable` returned.
    pub point_src: Vec<usize>,
    pub line_src: Vec<usize>,
    pub shape_src: Vec<usize>,
    /// Where each feature's label goes, and whose it is: a point at itself, a line at its middle
    /// vertex, a polygon at its outer ring's centroid. One per part, so every island of a
    /// multi-part county is named. Not counted as marks to draw.
    pub anchors: Vec<([f64; 2], usize)>,
}

/// Every attribute name among the imported features, sorted: the choices for a label or a
/// colour.
pub(crate) fn label_keys(marks: &Marks) -> Vec<String> {
    let mut keys: Vec<String> = marks
        .props
        .iter()
        .flat_map(|props| props.keys().cloned())
        .collect();
    keys.sort_unstable_by_key(|k| k.to_lowercase());
    keys.dedup();
    keys
}

/// A feature's label for attribute `key`: its value as text, trimmed and shortened to a label's
/// length; `None` when the feature lacks it or it is empty.
pub(crate) fn label_text(
    props: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<String> {
    let text = match props.get(key)? {
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Null => return None,
        other => other.to_string(),
    };
    if text.is_empty() {
        return None;
    }
    const MAX: usize = 32;
    Some(if text.chars().count() > MAX {
        let cut: String = text.chars().take(MAX - 1).collect();
        format!("{}…", cut.trim_end())
    } else {
        text
    })
}

/// How an imported layer is coloured by an attribute (I4): a ramp over a numeric attribute's
/// range, or one colour per value of a categorical one.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Legend {
    Graduated {
        min: f64,
        max: f64,
    },
    /// The most common values first, each with its colour; `others` counts the values past the
    /// palette, which share one grey.
    Categories {
        values: Vec<(String, [u8; 3])>,
        others: usize,
    },
}

/// Features coloured by an attribute: the attribute, each feature's colour and the legend.
pub(crate) type ColoredBy = (String, Vec<Option<[u8; 3]>>, Legend);

/// The graduated ramp, low to high: blue through green and yellow to red, every stop light
/// enough to read over the dark basemap.
pub(crate) const GRADUATED: [[u8; 3]; 5] = [
    [60, 120, 230],
    [40, 190, 190],
    [120, 210, 70],
    [250, 200, 40],
    [235, 70, 50],
];

/// Ten distinct category colours (the Tableau 10 set), then grey for the rest.
const CATEGORIES: [[u8; 3]; 10] = [
    [78, 121, 167],
    [242, 142, 43],
    [225, 87, 89],
    [118, 183, 178],
    [89, 161, 79],
    [237, 201, 72],
    [176, 122, 161],
    [255, 157, 167],
    [156, 117, 95],
    [186, 176, 172],
];
const OTHER: [u8; 3] = [150, 150, 150];

/// A colour along the graduated ramp, `t` from 0 to 1.
pub(crate) fn ramp(t: f64) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0) * (GRADUATED.len() - 1) as f64;
    let i = (t.floor() as usize).min(GRADUATED.len() - 2);
    let f = t - i as f64;
    let (a, b) = (GRADUATED[i], GRADUATED[i + 1]);
    std::array::from_fn(|j| (a[j] as f64 + f * (b[j] as f64 - a[j] as f64)).round() as u8)
}

/// An attribute's value as a number, if it is one (a string holding a number counts: text
/// formats carry every value as text).
fn number(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
    .filter(|x: &f64| x.is_finite())
}

/// Each feature's colour by attribute `key` (indexed like `marks.props`; `None` where the
/// feature lacks it, which then keeps the layer's own colour), and the legend that explains
/// them. Numeric when every present value is a number and they are not all equal; categorical
/// otherwise, the commonest values getting the first colours.
pub(crate) fn color_by(marks: &Marks, key: &str) -> (Vec<Option<[u8; 3]>>, Legend) {
    let values: Vec<Option<&serde_json::Value>> = marks
        .props
        .iter()
        .map(|p| p.get(key).filter(|v| !v.is_null()))
        .collect();
    let nums: Vec<Option<f64>> = values.iter().map(|v| v.and_then(number)).collect();
    let present = values.iter().flatten().count();
    let numeric = nums.iter().flatten().count();
    let (min, max) = nums
        .iter()
        .flatten()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &x| {
            (lo.min(x), hi.max(x))
        });
    if present > 0 && numeric == present && max > min {
        let colors = nums
            .iter()
            .map(|x| x.map(|x| ramp((x - min) / (max - min))))
            .collect();
        return (colors, Legend::Graduated { min, max });
    }
    let text = |v: &serde_json::Value| match v {
        serde_json::Value::String(s) => s.trim().to_string(),
        other => other.to_string(),
    };
    let mut counts: std::collections::HashMap<String, usize> = Default::default();
    for v in values.iter().flatten() {
        *counts.entry(text(v)).or_default() += 1;
    }
    let mut order: Vec<(String, usize)> = counts.into_iter().collect();
    order.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let palette: std::collections::HashMap<&str, [u8; 3]> = order
        .iter()
        .zip(CATEGORIES)
        .map(|((v, _), c)| (v.as_str(), c))
        .collect();
    let colors = values
        .iter()
        .map(|v| v.map(|v| palette.get(text(v).as_str()).copied().unwrap_or(OTHER)))
        .collect();
    let others = order.len().saturating_sub(CATEGORIES.len());
    let values = order
        .into_iter()
        .zip(CATEGORIES)
        .map(|((v, _), c)| (v, c))
        .collect();
    (colors, Legend::Categories { values, others })
}

/// Each feature's valid window (I5), from the attributes mapped to its start and end: `None` on
/// a side means unbounded there (no attribute chosen, or this feature's value is missing or not
/// a time).
pub(crate) type TimeBounds = Vec<(Option<DateTime<Utc>>, Option<DateTime<Utc>>)>;

/// An attribute value as a time: RFC 3339 / ISO 8601 with or without an offset (no offset reads
/// as UTC), a `YYYY-MM-DD HH:MM[:SS]` or bare date (midnight UTC), a compact `YYYYMMDD`, or a
/// Unix timestamp in seconds or milliseconds.
pub(crate) fn parse_time(v: &serde_json::Value) -> Option<DateTime<Utc>> {
    use chrono::{NaiveDate, NaiveDateTime, TimeZone};
    let epoch = |x: f64| {
        let ms = if x.abs() >= 1e11 { x } else { x * 1000.0 };
        Utc.timestamp_millis_opt(ms as i64).single()
    };
    let s = match v {
        serde_json::Value::Number(n) => return epoch(n.as_f64()?),
        serde_json::Value::String(s) => s.trim(),
        _ => return None,
    };
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Some(t.with_timezone(&Utc));
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
        "%Y/%m/%d %H:%M:%S",
        "%Y/%m/%d %H:%M",
    ] {
        if let Ok(t) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(t.and_utc());
        }
    }
    for fmt in ["%Y-%m-%d", "%Y/%m/%d", "%Y%m%d"] {
        if let Ok(d) = NaiveDate::parse_from_str(s, fmt) {
            return Some(d.and_hms_opt(0, 0, 0)?.and_utc());
        }
    }
    // A bare number in text: a timestamp, but only if it is not also a plausible `YYYYMMDD`
    // (handled above) — ten or thirteen digits.
    if s.len() >= 10 && s.bytes().all(|b| b.is_ascii_digit()) {
        return epoch(s.parse().ok()?);
    }
    None
}

/// Every feature's valid window from the attributes mapped to its start and end.
pub(crate) fn time_bounds(marks: &Marks, start: Option<&str>, end: Option<&str>) -> TimeBounds {
    let read = |p: &serde_json::Map<String, serde_json::Value>, key: Option<&str>| {
        key.and_then(|k| p.get(k)).and_then(parse_time)
    };
    marks
        .props
        .iter()
        .map(|p| (read(p, start), read(p, end)))
        .collect()
}

/// Which features are valid at `t`: from their start (inclusive) until their end (exclusive).
pub(crate) fn shown_at(bounds: &TimeBounds, t: DateTime<Utc>) -> Vec<bool> {
    bounds
        .iter()
        .map(|(s, e)| s.is_none_or(|s| s <= t) && e.is_none_or(|e| t < e))
        .collect()
}

/// A ring's area centroid (shoelace), or the mean of its vertices for a degenerate ring.
fn ring_anchor(ring: &[[f64; 2]]) -> Option<[f64; 2]> {
    let first = *ring.first()?;
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for w in ring.windows(2) {
        let (x0, y0) = (w[0][0] - first[0], w[0][1] - first[1]);
        let (x1, y1) = (w[1][0] - first[0], w[1][1] - first[1]);
        let cross = x0 * y1 - x1 * y0;
        a += cross;
        cx += (x0 + x1) * cross;
        cy += (y0 + y1) * cross;
    }
    if a.abs() > 1e-12 {
        return Some([first[0] + cx / (3.0 * a), first[1] + cy / (3.0 * a)]);
    }
    let n = ring.len() as f64;
    Some([
        ring.iter().map(|p| p[0]).sum::<f64>() / n,
        ring.iter().map(|p| p[1]).sum::<f64>() / n,
    ])
}

impl Marks {
    pub fn is_empty(&self) -> bool {
        self.points.is_empty() && self.lines.is_empty()
    }

    pub fn len(&self) -> usize {
        self.points.len() + self.lines.len()
    }
}

/// Split `features` into the polygons the overlay pipeline can render and hit-test, and the
/// points/lines the map paints directly. A `MultiPolygon` becomes one `GeoFeature` per part —
/// `GeoFeature::rings`' own "ring 0 is the outer boundary, the rest are holes" convention is
/// already one polygon's worth, not a whole multi-part feature's.
pub(crate) fn to_renderable(features: Vec<GisFeature>) -> (Vec<GeoFeature>, Marks) {
    let mut out = Vec::new();
    let mut marks = Marks::default();
    for f in features {
        let title = feature_title(&f);
        let detail = feature_detail(&f);
        let src = marks.props.len();
        marks.props.push(f.properties.clone());
        let anchors = &mut marks.anchors;
        let mut anchor = |at: Option<[f64; 2]>| {
            if let Some(at) = at {
                anchors.push((at, src));
            }
        };
        match &f.geometry {
            Geometry::Polygon(rings) => anchor(rings.first().and_then(|r| ring_anchor(r))),
            Geometry::MultiPolygon(parts) => {
                for rings in parts {
                    anchor(rings.first().and_then(|r| ring_anchor(r)));
                }
            }
            Geometry::Point(p) => anchor(Some(*p)),
            Geometry::MultiPoint(ps) => ps.iter().for_each(|p| anchor(Some(*p))),
            Geometry::LineString(l) => anchor(l.get(l.len() / 2).copied()),
            Geometry::MultiLineString(ls) => {
                for l in ls {
                    anchor(l.get(l.len() / 2).copied());
                }
            }
        }
        match f.geometry {
            Geometry::Polygon(rings) => {
                out.push(polygon_feature(rings, title, detail));
                marks.shape_src.push(src);
            }
            Geometry::MultiPolygon(parts) => {
                for rings in parts {
                    out.push(polygon_feature(rings, title.clone(), detail.clone()));
                    marks.shape_src.push(src);
                }
            }
            Geometry::Point(p) => {
                marks.points.push(p);
                marks.point_src.push(src);
            }
            Geometry::MultiPoint(ps) => {
                marks.point_src.extend(std::iter::repeat_n(src, ps.len()));
                marks.points.extend(ps);
            }
            // A one-position line has nothing to draw between; dropping it here keeps the painter
            // from having to care, the same way the annotation painter skips its own stub strokes.
            Geometry::LineString(l) => {
                if l.len() >= 2 {
                    marks.lines.push(l);
                    marks.line_src.push(src);
                }
            }
            Geometry::MultiLineString(ls) => {
                for l in ls.into_iter().filter(|l| l.len() >= 2) {
                    marks.lines.push(l);
                    marks.line_src.push(src);
                }
            }
        }
    }
    (out, marks)
}

/// The lon/lat box every imported shape fits in, as `(west, south, east, north)` — what "zoom to
/// the import" needs. `None` when nothing was imported. Polygons contribute through
/// [`GeoFeature::bbox`], which already knows their ring convention.
pub(crate) fn bounds(polygons: &[GeoFeature], marks: &Marks) -> Option<(f64, f64, f64, f64)> {
    let mut acc: Option<(f64, f64, f64, f64)> = None;
    let mut add = |lon: f64, lat: f64| {
        acc = Some(match acc {
            None => (lon, lat, lon, lat),
            Some((w, s, e, n)) => (w.min(lon), s.min(lat), e.max(lon), n.max(lat)),
        });
    };
    for p in polygons {
        if let Some((w, s, e, n)) = p.bbox() {
            add(w, s);
            add(e, n);
        }
    }
    for p in &marks.points {
        add(p[0], p[1]);
    }
    for line in &marks.lines {
        for p in line {
            add(p[0], p[1]);
        }
    }
    acc
}

fn polygon_feature(rings: Vec<Vec<[f64; 2]>>, title: String, detail: String) -> GeoFeature {
    GeoFeature {
        rings,
        fill: FILL,
        stroke: STROKE,
        kind: FeatureKind::Imported,
        title,
        detail,
        alert: None,
    }
}

/// A short label for the map legend/click popup title — whichever common attribute-name
/// convention (a shapefile-derived GeoJSON tends to shout its field names) the file actually
/// carries, falling back to the geometry's own type so an attribute-less shape still gets a name
/// rather than an empty title bar.
fn feature_title(f: &GisFeature) -> String {
    for key in ["name", "NAME", "Name", "title", "TITLE", "label", "LABEL"] {
        if let Some(v) = f.properties.get(key).and_then(|v| v.as_str()) {
            return v.to_string();
        }
    }
    f.geometry.kind_name().to_string()
}

/// The click popup body: every attribute the file carried, verbatim — a first GIS import has no
/// way to know which fields the person who clicked actually cares about, so showing all of them
/// beats guessing at a subset.
fn feature_detail(f: &GisFeature) -> String {
    if f.properties.is_empty() {
        return "Imported shape (no attributes)".to_string();
    }
    let mut lines: Vec<String> = f
        .properties
        .iter()
        .map(|(k, v)| {
            let v = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            format!("{k}: {v}")
        })
        .collect();
    lines.sort();
    lines.join("\n")
}

// ---- reading a picked file ---------------------------------------------------------------------

/// What reading an imported GIS file produced, plus anything the person should be told about how
/// complete it is (a shapefile picked without its `.dbf`, say).
pub(crate) struct Loaded {
    pub features: Vec<GisFeature>,
    pub note: Option<String>,
}

/// Is this file name a Shapefile's main `.shp`?
pub(crate) fn is_shapefile(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".shp")
}

/// Is this file name a KML document?
pub(crate) fn is_kml(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".kml")
}

/// Is this file name a KMZ (zipped KML)?
pub(crate) fn is_kmz(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".kmz")
}

/// Is this a binary format, which a browser has to remember as the GeoJSON it reads back as
/// rather than as its own text?
pub(crate) fn is_binary(name: &str) -> bool {
    is_shapefile(name) || is_kmz(name)
}

pub(crate) fn load_kml(text: &str) -> Result<Loaded, String> {
    wxdata::kml::parse(text)
        .map(|features| Loaded {
            features,
            note: None,
        })
        .map_err(|e| format!("{e:#}"))
}

pub(crate) fn load_kmz(bytes: &[u8]) -> Result<Loaded, String> {
    wxdata::kml::parse_kmz(bytes)
        .map(|features| Loaded {
            features,
            note: None,
        })
        .map_err(|e| format!("{e:#}"))
}

pub(crate) fn load_geojson(text: &str) -> Result<Loaded, String> {
    wxdata::gis::parse_geojson(text)
        .map(|features| Loaded {
            features,
            note: None,
        })
        .map_err(|e| e.to_string())
}

/// A shapefile handed over as one file's bytes — what a browser or a phone's picker gives. Its
/// `.dbf` and `.prj` are separate files that were not picked, so the shapes come without
/// attributes; the note says so instead of leaving the click popup mysteriously empty.
pub(crate) fn load_shapefile_bytes(shp: &[u8]) -> Result<Loaded, String> {
    let features = wxdata::shapefile::parse(shp, None, None).map_err(|e| format!("{e:#}"))?;
    Ok(Loaded {
        features,
        note: Some("picked one file, so its .dbf attributes and .prj were not read".to_string()),
    })
}

/// A shapefile on disk: the `.dbf` and `.prj` beside it are read too, whatever case their
/// extensions were written in (Windows-made sets are often `.DBF`).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn load_shapefile_path(path: &std::path::Path) -> Result<Loaded, String> {
    let sibling = |ext: &str| {
        [ext.to_string(), ext.to_ascii_uppercase()]
            .into_iter()
            .map(|e| path.with_extension(e))
            .find(|p| p.is_file())
    };
    let shp = std::fs::read(path).map_err(|e| e.to_string())?;
    let dbf = sibling("dbf")
        .map(|p| std::fs::read(p).map_err(|e| e.to_string()))
        .transpose()?;
    let prj = sibling("prj")
        .map(|p| std::fs::read_to_string(p).map_err(|e| e.to_string()))
        .transpose()?;
    let features = wxdata::shapefile::parse(&shp, dbf.as_deref(), prj.as_deref())
        .map_err(|e| format!("{e:#}"))?;
    let mut missing = Vec::new();
    if dbf.is_none() {
        missing.push("a .dbf (so no attributes)");
    }
    if prj.is_none() {
        missing.push("a .prj (so coordinates were assumed longitude/latitude)");
    }
    let note = (!missing.is_empty()).then(|| format!("no {} beside it", missing.join(" or ")));
    Ok(Loaded { features, note })
}

/// Read a remembered import back from its saved path, whichever format it is.
pub(crate) fn load_path(path: &str) -> Result<Loaded, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if is_shapefile(path) {
            return load_shapefile_path(std::path::Path::new(path));
        }
        if is_kmz(path) {
            return load_kmz(&std::fs::read(path).map_err(|e| e.to_string())?);
        }
        if is_kml(path) {
            return load_kml(&std::fs::read_to_string(path).map_err(|e| e.to_string())?);
        }
        load_geojson(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
    }
    #[cfg(target_arch = "wasm32")]
    {
        Err(format!("{path} cannot be reopened in a browser"))
    }
}

/// Read what the picker just returned, whichever format it is.
pub(crate) fn load_import(import: &crate::dialog::Import) -> Result<Loaded, String> {
    let name = import.name();
    if is_kmz(&name) {
        return match &import.bytes {
            Some(bytes) => load_kmz(bytes),
            None => load_kmz(&std::fs::read(&import.path).map_err(|e| e.to_string())?),
        };
    }
    if is_kml(&name) {
        return load_kml(&import.text()?);
    }
    if !is_shapefile(&name) {
        return load_geojson(&import.text()?);
    }
    match &import.bytes {
        Some(bytes) => load_shapefile_bytes(bytes),
        None => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                load_shapefile_path(&import.path)
            }
            #[cfg(target_arch = "wasm32")]
            {
                Err("no shapefile content was provided".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn feature(geometry: Geometry, props: serde_json::Value) -> GisFeature {
        GisFeature {
            geometry,
            properties: props.as_object().cloned().unwrap_or_default(),
        }
    }

    #[test]
    fn a_polygon_becomes_one_feature_with_a_name_attribute_title() {
        let rings = vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]];
        let f = feature(
            Geometry::Polygon(rings.clone()),
            json!({"name": "District 5"}),
        );
        let (out, marks) = to_renderable(vec![f]);
        assert!(marks.is_empty());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].rings, rings);
        assert_eq!(out[0].title, "District 5");
        assert_eq!(out[0].kind, FeatureKind::Imported);
        assert!(out[0].alert.is_none());
    }

    #[test]
    fn a_multipolygon_splits_into_one_feature_per_part_sharing_the_same_title() {
        let part_a = vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]];
        let part_b = vec![vec![[5.0, 5.0], [6.0, 5.0], [6.0, 6.0], [5.0, 5.0]]];
        let f = feature(
            Geometry::MultiPolygon(vec![part_a.clone(), part_b.clone()]),
            json!({"NAME": "Two islands"}),
        );
        let (out, marks) = to_renderable(vec![f]);
        assert!(marks.is_empty());
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].rings, part_a);
        assert_eq!(out[1].rings, part_b);
        assert!(out.iter().all(|g| g.title == "Two islands"));
    }

    #[test]
    fn points_and_lines_come_back_as_marks_to_paint_rather_than_being_dropped() {
        let features = vec![
            feature(Geometry::Point([1.0, 2.0]), json!({})),
            feature(
                Geometry::MultiPoint(vec![[3.0, 4.0], [5.0, 6.0]]),
                json!({}),
            ),
            feature(
                Geometry::LineString(vec![[0.0, 0.0], [1.0, 1.0]]),
                json!({}),
            ),
            feature(
                Geometry::MultiLineString(vec![
                    vec![[0.0, 0.0], [1.0, 1.0]],
                    vec![[2.0, 2.0], [3.0, 3.0]],
                ]),
                json!({}),
            ),
        ];
        let (polygons, marks) = to_renderable(features);
        assert!(polygons.is_empty(), "none of these are polygons");
        assert_eq!(marks.points, vec![[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]]);
        assert_eq!(marks.lines.len(), 3, "one line plus a two-part multiline");
        assert_eq!(marks.len(), 6);
    }

    #[test]
    fn every_feature_part_gets_a_label_anchor_with_its_attributes() {
        let square = vec![vec![
            [0.0, 0.0],
            [2.0, 0.0],
            [2.0, 2.0],
            [0.0, 2.0],
            [0.0, 0.0],
        ]];
        let features = vec![
            feature(
                Geometry::Point([5.0, 6.0]),
                json!({"name": "Site", "id": 3}),
            ),
            feature(
                Geometry::LineString(vec![[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]]),
                json!({"ROUTE": "I-35"}),
            ),
            feature(
                Geometry::MultiPolygon(vec![square.clone(), square]),
                json!({"name": "Islands"}),
            ),
        ];
        let (_, marks) = to_renderable(features);
        assert_eq!(marks.len(), 2, "anchors are not marks to draw");
        assert_eq!(marks.props.len(), 3, "one attribute set per feature");
        assert_eq!(
            (marks.point_src.clone(), marks.line_src.clone()),
            (vec![0], vec![1])
        );
        assert_eq!(
            marks.shape_src,
            [2, 2],
            "both parts are the third feature's"
        );
        assert_eq!(
            marks.anchors.len(),
            4,
            "point, line, and one per polygon part"
        );
        assert_eq!(marks.anchors[0], ([5.0, 6.0], 0));
        assert_eq!(marks.anchors[1].0, [1.0, 1.0], "a line's middle vertex");
        let c = marks.anchors[2].0;
        assert!(
            (c[0] - 1.0).abs() < 1e-9 && (c[1] - 1.0).abs() < 1e-9,
            "centroid {c:?}"
        );
        assert_eq!(label_keys(&marks), ["id", "name", "ROUTE"]);
        assert_eq!(label_text(&marks.props[0], "id").as_deref(), Some("3"));
        assert_eq!(label_text(&marks.props[1], "name"), None);
    }

    fn marks_with(values: &[serde_json::Value]) -> Marks {
        let features = values
            .iter()
            .map(|v| feature(Geometry::Point([0.0, 0.0]), json!({ "v": v })))
            .collect();
        to_renderable(features).1
    }

    #[test]
    fn times_read_in_the_forms_gis_files_write_them() {
        use chrono::TimeZone;
        let t = |y, mo, d, h, mi| Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap();
        for (v, want) in [
            (json!("2026-09-27T17:05:00Z"), t(2026, 9, 27, 17, 5)),
            (json!("2026-09-27T12:05:00-05:00"), t(2026, 9, 27, 17, 5)),
            (json!("2026-09-27T17:05:00"), t(2026, 9, 27, 17, 5)),
            (json!("2026-09-27 17:05"), t(2026, 9, 27, 17, 5)),
            (json!("2026-09-27"), t(2026, 9, 27, 0, 0)),
            (json!("20260927"), t(2026, 9, 27, 0, 0)),
            (json!(1790528700), t(2026, 9, 27, 17, 5)),
            (json!(1790528700000_i64), t(2026, 9, 27, 17, 5)),
            (json!("1790528700"), t(2026, 9, 27, 17, 5)),
        ] {
            assert_eq!(parse_time(&v), Some(want), "{v}");
        }
        assert_eq!(parse_time(&json!("soon")), None);
        assert_eq!(parse_time(&json!(null)), None);
    }

    #[test]
    fn a_feature_shows_from_its_start_until_its_end() {
        let features = [
            json!({"s": "2026-09-27T17:00:00Z", "e": "2026-09-27T18:00:00Z"}),
            json!({"s": "2026-09-27T18:00:00Z"}),
            json!({"e": "not a time"}),
        ]
        .into_iter()
        .map(|p| feature(Geometry::Point([0.0, 0.0]), p))
        .collect();
        let (_, marks) = to_renderable(features);
        let bounds = time_bounds(&marks, Some("s"), Some("e"));
        let at = |s: &str| shown_at(&bounds, s.parse().unwrap());
        assert_eq!(at("2026-09-27T16:59:00Z"), [false, false, true]);
        assert_eq!(
            at("2026-09-27T17:00:00Z"),
            [true, false, true],
            "start is inclusive"
        );
        assert_eq!(
            at("2026-09-27T18:00:00Z"),
            [false, true, true],
            "end is exclusive"
        );
        // Only an end mapped: everything is shown until its end.
        let ends = time_bounds(&marks, None, Some("e"));
        assert_eq!(
            shown_at(&ends, "2020-01-01T00:00:00Z".parse().unwrap()),
            [true; 3]
        );
    }

    #[test]
    fn a_numeric_attribute_colours_along_the_ramp() {
        let marks = marks_with(&[json!(10), json!("20"), json!(30), json!(null)]);
        let (colors, legend) = color_by(&marks, "v");
        assert_eq!(
            legend,
            Legend::Graduated {
                min: 10.0,
                max: 30.0
            }
        );
        assert_eq!(colors[0], Some(GRADUATED[0]));
        assert_eq!(
            colors[1],
            Some(GRADUATED[2]),
            "a number in text still counts"
        );
        assert_eq!(colors[2], Some(GRADUATED[4]));
        assert_eq!(colors[3], None, "no value keeps the layer colour");
        assert_eq!(color_by(&marks, "missing").0, vec![None; 4]);
    }

    #[test]
    fn a_categorical_attribute_gives_the_commonest_values_the_first_colours() {
        let mut vals: Vec<serde_json::Value> = ["b", "a", "b", "c"].map(|s| json!(s)).to_vec();
        vals.extend((0..12).map(|i| json!(format!("rare{i:02}"))));
        let marks = marks_with(&vals);
        let (colors, legend) = color_by(&marks, "v");
        let Legend::Categories { values, others } = legend else {
            panic!("categorical")
        };
        assert_eq!(values[0], ("b".to_string(), CATEGORIES[0]));
        assert_eq!(values[1].0, "a", "ties break by name");
        assert_eq!(colors[0], Some(CATEGORIES[0]));
        assert_eq!(values.len(), 10);
        assert_eq!(others, 5, "15 distinct values, 10 coloured");
        assert_eq!(colors.last().copied().flatten(), Some(OTHER));
        // One repeated number is not a range: it reads as a single category.
        let flat = marks_with(&[json!(5), json!(5)]);
        assert!(matches!(color_by(&flat, "v").1, Legend::Categories { .. }));
    }

    #[test]
    fn a_label_is_trimmed_shortened_and_never_empty() {
        let props = json!({"a": "  Norman  ", "b": "", "c": null, "d": "x".repeat(50)});
        let props = props.as_object().unwrap();
        assert_eq!(label_text(props, "a").as_deref(), Some("Norman"));
        assert_eq!(label_text(props, "b"), None);
        assert_eq!(label_text(props, "c"), None);
        let long = label_text(props, "d").unwrap();
        assert_eq!(long.chars().count(), 32);
        assert!(long.ends_with('…'));
    }

    #[test]
    fn the_layer_hides_below_its_minimum_zoom() {
        let mut style = ImportedGisStyle::default();
        assert!(style.visible_at(0.0), "shown at every zoom by default");
        style.min_zoom = 7.0;
        assert!(!style.visible_at(6.9));
        assert!(style.visible_at(7.0));
    }

    #[test]
    fn imported_polygon_style_changes_only_paint_not_identity_or_geometry() {
        let rings = vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]];
        let mut f = polygon_feature(rings.clone(), "District".into(), "id: 7".into());
        apply_style(
            &mut f,
            ImportedGisStyle {
                color: [240, 80, 40],
                stroke_width: 3.0,
                opacity: 0.5,
                min_zoom: 0.0,
            },
        );
        assert_eq!(f.fill, [240, 80, 40, 30]);
        assert_eq!(f.stroke, [240, 80, 40, 110]);
        assert_eq!(f.rings, rings);
        assert_eq!(f.title, "District");
        assert_eq!(f.detail, "id: 7");
        assert_eq!(f.kind, FeatureKind::Imported);
    }

    #[test]
    fn a_one_position_line_has_nothing_to_draw_and_is_left_out() {
        let (_, marks) = to_renderable(vec![feature(
            Geometry::LineString(vec![[0.0, 0.0]]),
            json!({}),
        )]);
        assert!(marks.lines.is_empty());
    }

    #[test]
    fn bounds_cover_polygons_points_and_lines_together() {
        let (polygons, marks) = to_renderable(vec![
            feature(
                Geometry::Polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]),
                json!({}),
            ),
            feature(Geometry::Point([-5.0, 9.0]), json!({})),
            feature(
                Geometry::LineString(vec![[2.0, -3.0], [4.0, 0.5]]),
                json!({}),
            ),
        ]);
        assert_eq!(
            bounds(&polygons, &marks),
            Some((-5.0, -3.0, 4.0, 9.0)),
            "the box has to reach the outermost of all three kinds"
        );
    }

    #[test]
    fn nothing_imported_has_no_bounds_to_zoom_to() {
        assert_eq!(bounds(&[], &Marks::default()), None);
    }

    #[test]
    fn a_mixed_file_splits_into_both_halves() {
        let features = vec![
            feature(Geometry::Point([1.0, 2.0]), json!({})),
            feature(
                Geometry::LineString(vec![[0.0, 0.0], [1.0, 1.0]]),
                json!({}),
            ),
            feature(
                Geometry::Polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]),
                json!({}),
            ),
        ];
        let (out, marks) = to_renderable(features);
        assert_eq!(out.len(), 1, "the polygon goes to the overlay pipeline");
        assert_eq!(marks.points.len(), 1, "the point goes to the painter");
        assert_eq!(marks.lines.len(), 1, "and so does the line");
    }

    #[test]
    fn a_shape_with_no_recognized_name_attribute_falls_back_to_its_geometry_kind() {
        let f = feature(
            Geometry::Polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]),
            json!({"unrelated_field": 42}),
        );
        let (out, _) = to_renderable(vec![f]);
        assert_eq!(out[0].title, "Polygon");
    }

    #[test]
    fn the_click_detail_lists_every_attribute_sorted() {
        let f = feature(
            Geometry::Polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]),
            json!({"pop": 12345, "name": "Ward 3"}),
        );
        let (out, _) = to_renderable(vec![f]);
        assert_eq!(out[0].detail, "name: Ward 3\npop: 12345");
    }

    #[test]
    fn no_attributes_reads_as_an_explicit_message_not_an_empty_string() {
        let f = feature(
            Geometry::Polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]),
            json!({}),
        );
        let (out, _) = to_renderable(vec![f]);
        assert_eq!(out[0].detail, "Imported shape (no attributes)");
    }
    // ---- reading shapefiles from disk --------------------------------------------------------------

    /// One point at (x, y) as a complete `.shp`.
    fn point_shp(x: f64, y: f64) -> Vec<u8> {
        let mut rec = 1i32.to_le_bytes().to_vec();
        rec.extend_from_slice(&x.to_le_bytes());
        rec.extend_from_slice(&y.to_le_bytes());
        let mut out = vec![0u8; 100];
        out[0..4].copy_from_slice(&9994i32.to_be_bytes());
        out[24..28].copy_from_slice(&(((100 + 8 + rec.len()) / 2) as i32).to_be_bytes());
        out.extend_from_slice(&1i32.to_be_bytes());
        out.extend_from_slice(&((rec.len() / 2) as i32).to_be_bytes());
        out.extend_from_slice(&rec);
        out
    }

    /// A one-column, one-row `.dbf`: NAME = `value`.
    fn name_dbf(value: &str) -> Vec<u8> {
        let mut out = vec![0u8; 32];
        out[0] = 3;
        out[4..8].copy_from_slice(&1u32.to_le_bytes());
        out[8..10].copy_from_slice(&(32u16 + 32 + 1).to_le_bytes());
        out[10..12].copy_from_slice(&9u16.to_le_bytes());
        let mut d = [0u8; 32];
        d[..4].copy_from_slice(b"NAME");
        d[11] = b'C';
        d[16] = 8;
        out.extend_from_slice(&d);
        out.push(0x0D);
        out.push(0x20);
        let mut cell = value.as_bytes().to_vec();
        cell.resize(8, b' ');
        out.extend_from_slice(&cell);
        out
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("hookecho_shp_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn a_shapefile_on_disk_picks_up_its_dbf_and_prj_even_in_upper_case() {
        let dir = scratch("siblings");
        std::fs::write(dir.join("sites.shp"), point_shp(-97.5, 35.2)).unwrap();
        // Windows-made sets are often shouted.
        std::fs::write(dir.join("sites.DBF"), name_dbf("Norman")).unwrap();
        std::fs::write(dir.join("sites.prj"), r#"GEOGCS["GCS_WGS_1984"]"#).unwrap();

        let loaded = load_shapefile_path(&dir.join("sites.shp")).expect("loads");
        assert_eq!(loaded.features.len(), 1);
        assert_eq!(loaded.features[0].properties["NAME"], "Norman");
        assert_eq!(
            loaded.note, None,
            "nothing is missing, so nothing to warn about"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_kml_on_disk_imports_to_shapes_and_marks_with_its_names() {
        let dir = scratch("kml");
        let kml = r#"<kml xmlns="http://www.opengis.net/kml/2.2"><Document>
            <Placemark><name>Staging</name>
              <Point><coordinates>-97.9,35.0,0</coordinates></Point></Placemark>
            <Placemark><name>Box</name><Polygon><outerBoundaryIs><LinearRing>
              <coordinates>-98,35 -97,35 -97,36 -98,35</coordinates>
            </LinearRing></outerBoundaryIs></Polygon></Placemark>
            </Document></kml>"#;
        let path = dir.join("Plan.KML");
        std::fs::write(&path, kml).unwrap();
        let loaded = load_path(path.to_str().unwrap()).expect("loads");
        assert_eq!(loaded.features.len(), 2);
        let (shapes, marks) = to_renderable(loaded.features);
        assert_eq!(shapes.len(), 1);
        assert_eq!(shapes[0].title, "Box");
        assert_eq!(marks.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lone_shp_still_imports_and_says_what_it_did_not_have() {
        let dir = scratch("lone");
        std::fs::write(dir.join("lone.shp"), point_shp(-97.5, 35.2)).unwrap();
        let loaded = load_shapefile_path(&dir.join("lone.shp")).expect("loads");
        assert_eq!(loaded.features.len(), 1);
        let note = loaded.note.expect("a note");
        assert!(note.contains(".dbf") && note.contains(".prj"), "{note}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_projected_prj_beside_the_shp_stops_the_import_with_a_reason() {
        let dir = scratch("projected");
        std::fs::write(dir.join("p.shp"), point_shp(500_000.0, 4_000_000.0)).unwrap();
        std::fs::write(
            dir.join("p.prj"),
            r#"PROJCS["NAD_1983_UTM_Zone_14N",GEOGCS["GCS_North_American_1983"]]"#,
        )
        .unwrap();
        let err = load_shapefile_path(&dir.join("p.shp"))
            .err()
            .expect("refused");
        assert!(err.contains("UTM_Zone_14N"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_remembered_path_reloads_by_extension_and_geojson_still_does() {
        let dir = scratch("reload");
        std::fs::write(dir.join("a.shp"), point_shp(-97.0, 35.0)).unwrap();
        std::fs::write(
            dir.join("b.geojson"),
            r#"{"type":"Point","coordinates":[-97.0,35.0]}"#,
        )
        .unwrap();
        assert_eq!(
            load_path(dir.join("a.shp").to_str().unwrap())
                .unwrap()
                .features
                .len(),
            1
        );
        assert_eq!(
            load_path(dir.join("b.geojson").to_str().unwrap())
                .unwrap()
                .features
                .len(),
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bytes_alone_import_geometry_and_say_the_sidecars_were_not_read() {
        let loaded = load_shapefile_bytes(&point_shp(-97.0, 35.0)).expect("loads");
        assert_eq!(loaded.features.len(), 1);
        assert!(loaded.note.expect("a note").contains("one file"));
    }

    #[test]
    fn shapefile_names_are_recognised_in_any_case() {
        assert!(is_shapefile("Parcels.SHP"));
        assert!(is_shapefile("/a/b/c.shp"));
        assert!(is_kml("Plan.KML") && is_kmz("plan.kmz") && !is_kml("plan.kmz"));
        assert!(is_binary("a.kmz") && is_binary("a.shp") && !is_binary("a.kml"));
        assert!(!is_shapefile("c.geojson"));
        assert!(!is_shapefile("c.shp.json"));
    }
}
