//! Writing what is currently on the map out as GeoJSON (ROADMAP_NEW I6) — the mirror of
//! [`crate::gis_import`], and the half Phase I's stated audience (emergency management, research,
//! broadcast) actually needs to get work out of this app and into QGIS/ArcGIS or a briefing.
//!
//! Everything exportable here was already held in `[lon, lat]`, so this is a translation, not a
//! computation: freehand annotations, saved markers, watch zones, storm cells, and every overlay
//! polygon currently assembled for display (which is `app.rs`'s own `rebuild_overlays` output, so
//! an export contains exactly what the map shows — filters, toggles and all — rather than every
//! feature ever fetched).
//!
//! Each feature carries a `hookecho` property naming what it came from, so a re-import (or a
//! third-party tool) can tell an annotation from a warning polygon without guessing from geometry.

use crate::app::Stroke2d;
use crate::settings::{AlertPolygon, Marker};
use serde_json::{Map, Value};
use wxdata::gis::{Geometry, GisFeature};
use wxdata::level3::Cell;
use wxdata::overlay::GeoFeature;

/// Everything on the map worth writing out, borrowed from the app for one export.
pub(crate) struct MapContents<'a> {
    pub strokes: &'a [Stroke2d],
    pub markers: &'a [Marker],
    pub zones: &'a [AlertPolygon],
    pub cells: &'a [Cell],
    /// The assembled overlay set (`app.rs`'s `self.overlays`): warnings, outlooks, watch boxes,
    /// ProbSevere, fire perimeters and any imported shapes, already filtered to what is displayed.
    pub overlays: &'a [GeoFeature],
}

fn props(pairs: impl IntoIterator<Item = (&'static str, Value)>) -> Map<String, Value> {
    pairs
        .into_iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

fn text(s: impl Into<String>) -> Value {
    Value::String(s.into())
}

fn number(v: Option<f32>) -> Value {
    v.and_then(|v| serde_json::Number::from_f64(v as f64))
        .map_or(Value::Null, Value::Number)
}

/// GeoJSON requires a linear ring to be closed (first position repeated last). Rings that came
/// from a GeoJSON feed already are; a locally drawn one (`AlertPolygon`, whose own doc comment
/// calls it an outer ring the user clicked out) is not, and writing it open produces a file some
/// readers reject and others silently repair differently.
fn closed(ring: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut out = ring.to_vec();
    match (out.first().copied(), out.last().copied()) {
        (Some(first), Some(last)) if first != last => out.push(first),
        _ => {}
    }
    out
}

fn hex(c: egui::Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

/// Every feature an export of `map` contains. Order is stable and grouped by kind so a written
/// file reads predictably; a caller that wants only one kind filters on the `hookecho` property.
pub(crate) fn to_features(map: &MapContents<'_>) -> Vec<GisFeature> {
    let mut out = Vec::new();

    // A freehand scribble is a line, not an area — even one drawn as a closed-looking circle,
    // which is the usual "circle this storm" gesture but not a polygon the user declared.
    for stroke in map.strokes {
        if stroke.points.len() < 2 {
            continue;
        }
        out.push(GisFeature {
            geometry: Geometry::LineString(stroke.points.clone()),
            properties: props([
                ("hookecho", text("annotation")),
                ("color", text(hex(stroke.color))),
            ]),
        });
    }

    for marker in map.markers {
        out.push(GisFeature {
            geometry: Geometry::Point([marker.lon, marker.lat]),
            properties: props([
                ("hookecho", text("marker")),
                ("name", text(marker.name.clone())),
            ]),
        });
    }

    for zone in map.zones {
        if zone.ring.len() < 3 {
            continue;
        }
        out.push(GisFeature {
            geometry: Geometry::Polygon(vec![closed(&zone.ring)]),
            properties: props([
                ("hookecho", text("watch-zone")),
                ("name", text(zone.name.clone())),
            ]),
        });
    }

    // Storm cells export as points carrying their own scan-derived numbers: a cell position with
    // no movement or intensity beside it is the one part of this that would be useless elsewhere.
    for cell in map.cells {
        out.push(GisFeature {
            geometry: Geometry::Point([cell.lon, cell.lat]),
            properties: props([
                ("hookecho", text("storm-cell")),
                ("id", text(cell.id.clone())),
                ("title", text(cell.title.clone())),
                ("movement_deg", number(cell.mvt_deg)),
                ("movement_kt", number(cell.mvt_kt)),
                ("max_dbz", number(cell.max_dbz)),
                ("top_kft", number(cell.top_kft)),
                ("vil", number(cell.vil)),
                (
                    "time",
                    cell.time.map_or(Value::Null, |t| text(t.to_rfc3339())),
                ),
            ]),
        });
    }

    for feature in map.overlays {
        if feature.rings.is_empty() {
            continue;
        }
        out.push(GisFeature {
            geometry: Geometry::Polygon(feature.rings.iter().map(|r| closed(r)).collect()),
            properties: props([
                (
                    "hookecho",
                    text(format!("{:?}", feature.kind).to_lowercase()),
                ),
                ("title", text(feature.title.clone())),
                ("detail", text(feature.detail.clone())),
            ]),
        });
    }

    out
}

/// The whole export as one GeoJSON document, ready for `dialog::save_bytes`.
pub(crate) fn to_geojson(map: &MapContents<'_>) -> String {
    wxdata::gis::to_geojson(&to_features(map))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wxdata::overlay::FeatureKind;

    fn empty() -> MapContents<'static> {
        MapContents {
            strokes: &[],
            markers: &[],
            zones: &[],
            cells: &[],
            overlays: &[],
        }
    }

    fn kind_of(f: &GisFeature) -> &str {
        f.properties
            .get("hookecho")
            .and_then(|v| v.as_str())
            .unwrap_or("")
    }

    #[test]
    fn an_empty_map_exports_a_valid_empty_document() {
        let json = to_geojson(&empty());
        assert!(wxdata::gis::parse_geojson(&json).unwrap().is_empty());
    }

    #[test]
    fn a_stroke_exports_as_a_line_with_its_own_color() {
        let strokes = vec![Stroke2d {
            points: vec![[-97.0, 35.0], [-96.9, 35.1]],
            color: egui::Color32::from_rgb(255, 80, 80),
        }];
        let out = to_features(&MapContents {
            strokes: &strokes,
            ..empty()
        });
        assert_eq!(out.len(), 1);
        assert_eq!(kind_of(&out[0]), "annotation");
        assert_eq!(
            out[0].properties.get("color").and_then(|v| v.as_str()),
            Some("#FF5050")
        );
        assert_eq!(
            out[0].geometry,
            Geometry::LineString(vec![[-97.0, 35.0], [-96.9, 35.1]])
        );
    }

    #[test]
    fn a_one_point_stroke_is_dropped_rather_than_written_as_a_degenerate_line() {
        let strokes = vec![Stroke2d {
            points: vec![[-97.0, 35.0]],
            color: egui::Color32::WHITE,
        }];
        assert!(to_features(&MapContents {
            strokes: &strokes,
            ..empty()
        })
        .is_empty());
    }

    #[test]
    fn a_watch_zone_ring_is_closed_on_the_way_out() {
        let zones = vec![AlertPolygon {
            name: "Yard".into(),
            ring: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]],
        }];
        let out = to_features(&MapContents {
            zones: &zones,
            ..empty()
        });
        let Geometry::Polygon(rings) = &out[0].geometry else {
            panic!("a zone must export as a polygon");
        };
        assert_eq!(
            rings[0].first(),
            rings[0].last(),
            "GeoJSON linear rings must close: {rings:?}"
        );
        assert_eq!(rings[0].len(), 4, "closing adds exactly one position");
    }

    #[test]
    fn an_already_closed_ring_is_not_closed_twice() {
        let zones = vec![AlertPolygon {
            name: "Yard".into(),
            ring: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]],
        }];
        let out = to_features(&MapContents {
            zones: &zones,
            ..empty()
        });
        let Geometry::Polygon(rings) = &out[0].geometry else {
            panic!("a zone must export as a polygon");
        };
        assert_eq!(rings[0].len(), 4);
    }

    #[test]
    fn a_storm_cell_carries_its_movement_and_intensity_and_omits_what_it_lacks() {
        let cells = vec![Cell {
            lon: -97.5,
            lat: 35.2,
            id: "O7".into(),
            title: "O7".into(),
            mvt_deg: Some(240.0),
            mvt_kt: Some(35.0),
            max_dbz: Some(62.0),
            vil: None,
            ..Default::default()
        }];
        let out = to_features(&MapContents {
            cells: &cells,
            ..empty()
        });
        assert_eq!(kind_of(&out[0]), "storm-cell");
        assert_eq!(out[0].geometry, Geometry::Point([-97.5, 35.2]));
        assert_eq!(
            out[0].properties.get("max_dbz").and_then(|v| v.as_f64()),
            Some(62.0)
        );
        assert!(
            !out[0].properties.contains_key("vil"),
            "a missing value is left out, not written as null"
        );
    }

    #[test]
    fn an_overlay_polygon_keeps_its_kind_and_title() {
        let overlays = vec![GeoFeature {
            rings: vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
            fill: [0; 4],
            stroke: [0; 4],
            kind: FeatureKind::Warning,
            title: "Tornado Warning".into(),
            detail: "…".into(),
            alert: None,
        }];
        let out = to_features(&MapContents {
            overlays: &overlays,
            ..empty()
        });
        assert_eq!(kind_of(&out[0]), "warning");
        assert_eq!(
            out[0].properties.get("title").and_then(|v| v.as_str()),
            Some("Tornado Warning")
        );
    }

    /// The point of writing GeoJSON is that something else can read it — including this app's own
    /// importer, which is the one reader available to test against here.
    #[test]
    fn an_export_reads_back_through_this_apps_own_importer() {
        let strokes = vec![Stroke2d {
            points: vec![[-97.0, 35.0], [-96.9, 35.1]],
            color: egui::Color32::WHITE,
        }];
        let markers = vec![Marker {
            id: "m1".into(),
            name: "Home".into(),
            lon: -97.5,
            lat: 35.2,
            icon: None,
            alert_radius_mi: 0.0,
            video_url: String::new(),
            home: true,
        }];
        let json = to_geojson(&MapContents {
            strokes: &strokes,
            markers: &markers,
            ..empty()
        });
        let back = wxdata::gis::parse_geojson(&json).expect("our own export must parse");
        assert_eq!(back.len(), 2);
        assert_eq!(kind_of(&back[0]), "annotation");
        assert_eq!(kind_of(&back[1]), "marker");
        assert_eq!(back[1].geometry, Geometry::Point([-97.5, 35.2]));
    }
}
