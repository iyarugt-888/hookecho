//! Converting a generic GIS import (`wxdata::gis`, ROADMAP_NEW I1/I3) into this app's existing
//! renderable overlay shape.
//!
//! `wxdata::overlay::GeoFeature` (rings + fill/stroke + click-through text) is already the one
//! shape every NWS/SPC feed's polygons render and hit-test through — `app.rs`'s `rebuild_overlays`
//! assembles one combined `Vec<GeoFeature>` from all of them, and the map painter/click-tester
//! both work off that alone. Landing an import in that same list means it draws and is clickable
//! with no new rendering code, not a second overlay pipeline.
//!
//! What this deliberately does not attempt: `GeoFeature` is rings-only, so `Point`/`MultiPoint`/
//! `LineString`/`MultiLineString` geometries have nowhere to go yet — no marker or stroked-line
//! rendering exists anywhere in this app today (every existing feed is polygon-shaped too).
//! Inventing that just for this one caller is I4's own separate, larger styling work, not this
//! module's job; those features are counted and reported back to the caller instead of silently
//! dropped.

use wxdata::gis::{Geometry, GisFeature};
use wxdata::overlay::{FeatureKind, GeoFeature};

/// A neutral blue, distinct from every existing feed's own convention (warnings red, watches
/// yellow, SPC risk colors, …) so an imported shape never reads as an official product.
const FILL: [u8; 4] = [80, 140, 220, 60];
const STROKE: [u8; 4] = [80, 140, 220, 220];

/// Convert `features` into renderable overlay polygons, dropping (and counting) any geometry this
/// app has no rendering path for yet. A `MultiPolygon` becomes one `GeoFeature` per part —
/// `GeoFeature::rings`' own "ring 0 is the outer boundary, the rest are holes" convention is
/// already one polygon's worth, not a whole multi-part feature's.
pub fn to_overlay_features(features: Vec<GisFeature>) -> (Vec<GeoFeature>, usize) {
    let mut out = Vec::new();
    let mut skipped = 0;
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
            Geometry::Point(_)
            | Geometry::MultiPoint(_)
            | Geometry::LineString(_)
            | Geometry::MultiLineString(_) => skipped += 1,
        }
    }
    (out, skipped)
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
        let (out, skipped) = to_overlay_features(vec![f]);
        assert_eq!(skipped, 0);
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
        let (out, skipped) = to_overlay_features(vec![f]);
        assert_eq!(skipped, 0);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].rings, part_a);
        assert_eq!(out[1].rings, part_b);
        assert!(out.iter().all(|g| g.title == "Two islands"));
    }

    #[test]
    fn a_point_or_line_is_skipped_and_counted_not_silently_dropped() {
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
        let (out, skipped) = to_overlay_features(features);
        assert_eq!(out.len(), 1, "only the polygon renders");
        assert_eq!(skipped, 2, "the point and the line are both counted");
    }

    #[test]
    fn a_shape_with_no_recognized_name_attribute_falls_back_to_its_geometry_kind() {
        let f = feature(
            Geometry::Polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]),
            json!({"unrelated_field": 42}),
        );
        let (out, _) = to_overlay_features(vec![f]);
        assert_eq!(out[0].title, "Polygon");
    }

    #[test]
    fn the_click_detail_lists_every_attribute_sorted() {
        let f = feature(
            Geometry::Polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]),
            json!({"pop": 12345, "name": "Ward 3"}),
        );
        let (out, _) = to_overlay_features(vec![f]);
        assert_eq!(out[0].detail, "name: Ward 3\npop: 12345");
    }

    #[test]
    fn no_attributes_reads_as_an_explicit_message_not_an_empty_string() {
        let f = feature(
            Geometry::Polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]),
            json!({}),
        );
        let (out, _) = to_overlay_features(vec![f]);
        assert_eq!(out[0].detail, "Imported shape (no attributes)");
    }
}
