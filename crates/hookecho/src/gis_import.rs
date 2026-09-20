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
//! projection the freehand annotation strokes already use. What is still deferred to I4 is
//! *styling* them: per-layer color, width, labels from a chosen attribute.

use wxdata::gis::{Geometry, GisFeature};
use wxdata::overlay::{FeatureKind, GeoFeature};

/// A neutral blue, distinct from every existing feed's own convention (warnings red, watches
/// yellow, SPC risk colors, …) so an imported shape never reads as an official product.
const FILL: [u8; 4] = [80, 140, 220, 60];
pub(crate) const STROKE: [u8; 4] = [80, 140, 220, 220];

/// The imported geometry the overlay pipeline can't hold, kept in the app and painted directly.
/// A `MultiPoint`/`MultiLineString` flattens into its parts here — nothing downstream needs to
/// know the file grouped them.
#[derive(Default, Debug, Clone, PartialEq)]
pub(crate) struct Marks {
    pub points: Vec<[f64; 2]>,
    pub lines: Vec<Vec<[f64; 2]>>,
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
        match f.geometry {
            Geometry::Polygon(rings) => out.push(polygon_feature(rings, title, detail)),
            Geometry::MultiPolygon(parts) => {
                for rings in parts {
                    out.push(polygon_feature(rings, title.clone(), detail.clone()));
                }
            }
            Geometry::Point(p) => marks.points.push(p),
            Geometry::MultiPoint(ps) => marks.points.extend(ps),
            // A one-position line has nothing to draw between; dropping it here keeps the painter
            // from having to care, the same way the annotation painter skips its own stub strokes.
            Geometry::LineString(l) => {
                if l.len() >= 2 {
                    marks.lines.push(l);
                }
            }
            Geometry::MultiLineString(ls) => {
                marks.lines.extend(ls.into_iter().filter(|l| l.len() >= 2));
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
}
