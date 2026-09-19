//! Generic GIS import (ROADMAP_NEW I1/I3): parse an arbitrary GeoJSON document into every
//! geometry type the format defines, for a user-supplied file this app has no prior knowledge of
//! the shape of — distinct from [`crate::overlay`], which decodes the specific *known*,
//! polygon-shaped schemas of NWS/NOAA feeds this app already fetches from fixed URLs (alerts,
//! outlooks, watch boxes). Reuses that module's own [`crate::overlay::for_each_feature`] (the
//! FeatureCollection/Feature dispatch and the "an ArcGIS error reported as HTTP 200" detection)
//! rather than re-implementing either — the only genuinely new work here is recognizing every
//! geometry type, not just Polygon/MultiPolygon.
//!
//! What this module deliberately does *not* do yet, per I1's own suggested order: Shapefile, KML,
//! KMZ and GeoPackage import (GeoJSON first; the others are separate, larger parsers with their
//! own formats to get right); reprojection from a non-WGS84 CRS (I2 — a GeoJSON document is
//! supposed to always be WGS84 per the spec, so this isn't blocking GeoJSON specifically, but a
//! Shapefile's `.prj` will need it); and rendering the result as a map layer or any file-picker
//! UI to reach this function from (I4's styling, and the actual "import a file" user flow) — this
//! is the parsing layer only, the same "ship the evaluator, defer the renderer" split this
//! codebase already used for C1's user-defined products.

use geojson::{GeometryValue, Position};

/// One shape from a GeoJSON document, in this module's own coordinate convention: `[lon, lat]`
/// pairs, matching [`crate::overlay::GeoFeature`]'s rings so a future renderer can treat both the
/// same way. Unlike that type, this covers every geometry GeoJSON defines (I3), not just polygons
/// — a user's own imported data has no reason to be alert-shaped.
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    Point([f64; 2]),
    MultiPoint(Vec<[f64; 2]>),
    LineString(Vec<[f64; 2]>),
    MultiLineString(Vec<Vec<[f64; 2]>>),
    /// Rings; ring 0 is the outer boundary, any others are holes — same convention as
    /// [`crate::overlay::GeoFeature::rings`].
    Polygon(Vec<Vec<[f64; 2]>>),
    /// One ring-group per polygon part, each shaped like [`Self::Polygon`]'s own payload.
    MultiPolygon(Vec<Vec<Vec<[f64; 2]>>>),
}

impl Geometry {
    /// A short, stable name for a legend/inspector — mirrors the GeoJSON spec's own type strings.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Point(_) => "Point",
            Self::MultiPoint(_) => "MultiPoint",
            Self::LineString(_) => "LineString",
            Self::MultiLineString(_) => "MultiLineString",
            Self::Polygon(_) => "Polygon",
            Self::MultiPolygon(_) => "MultiPolygon",
        }
    }
}

/// One imported feature: a shape plus whatever attributes its source document attached to it.
/// `properties` is the document's own JSON object verbatim — I4's "labels from chosen attribute"
/// and "graduated color by numeric attribute" both need arbitrary access to it, not a
/// pre-selected subset decided at parse time.
#[derive(Debug, Clone)]
pub struct GisFeature {
    pub geometry: Geometry,
    pub properties: serde_json::Map<String, serde_json::Value>,
}

fn point(p: &Position) -> Option<[f64; 2]> {
    let s = p.as_slice();
    (s.len() >= 2).then(|| [s[0], s[1]])
}

fn line(coords: &[Position]) -> Vec<[f64; 2]> {
    coords.iter().filter_map(point).collect()
}

fn polygon(rings: &[Vec<Position>]) -> Vec<Vec<[f64; 2]>> {
    rings.iter().map(|r| line(r)).collect()
}

/// Every [`Geometry`] one GeoJSON geometry value expands to — more than one only for a
/// `GeometryCollection`, flattened here rather than kept as its own variant: a caller asking "every
/// shape this feature has" doesn't need to know the source nested some of them one level deeper.
/// `point()`'s `Option` is a second, defensive check against a `Position` with fewer than two
/// coordinates contributing a bogus `[0.0, 0.0]`; in practice the `geojson` crate itself already
/// refuses to deserialize such a `Position` at parse time (see this module's own test), so this
/// case is believed unreachable in normal use, not confirmed reachable and handled.
fn geometries_of(value: &GeometryValue) -> Vec<Geometry> {
    match value {
        GeometryValue::Point { coordinates } => point(coordinates)
            .map(Geometry::Point)
            .into_iter()
            .collect(),
        GeometryValue::MultiPoint { coordinates } => {
            vec![Geometry::MultiPoint(
                coordinates.iter().filter_map(point).collect(),
            )]
        }
        GeometryValue::LineString { coordinates } => vec![Geometry::LineString(line(coordinates))],
        GeometryValue::MultiLineString { coordinates } => vec![Geometry::MultiLineString(
            coordinates.iter().map(|l| line(l)).collect(),
        )],
        GeometryValue::Polygon { coordinates } => vec![Geometry::Polygon(polygon(coordinates))],
        GeometryValue::MultiPolygon { coordinates } => vec![Geometry::MultiPolygon(
            coordinates.iter().map(|p| polygon(p)).collect(),
        )],
        GeometryValue::GeometryCollection { geometries } => geometries
            .iter()
            .flat_map(|g| geometries_of(&g.value))
            .collect(),
    }
}

/// Parse an arbitrary GeoJSON document — a `FeatureCollection`, a bare `Feature`, or a bare
/// `Geometry` (all three are valid top-level GeoJSON) — into every feature it contains. A
/// document that parses as JSON but isn't valid GeoJSON, or one that's actually an upstream error
/// report (see [`crate::overlay::for_each_feature`]'s own doc comment), is a named error rather
/// than an empty result — an import with nothing in it and an import that silently failed to read
/// must not look the same to the person who just picked the file.
pub fn parse_geojson(json: &str) -> anyhow::Result<Vec<GisFeature>> {
    let mut out = Vec::new();
    crate::overlay::for_each_feature(json, |value, props| {
        for geometry in geometries_of(value) {
            out.push(GisFeature {
                geometry,
                properties: props.clone(),
            });
        }
    })?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_geometry_type_from_a_feature_collection() {
        let json = r#"{
            "type": "FeatureCollection",
            "features": [
                {"type": "Feature", "properties": {"name": "a"},
                 "geometry": {"type": "Point", "coordinates": [-97.5, 35.2]}},
                {"type": "Feature", "properties": {"name": "b"},
                 "geometry": {"type": "MultiPoint", "coordinates": [[-97.5, 35.2], [-96.0, 34.0]]}},
                {"type": "Feature", "properties": {},
                 "geometry": {"type": "LineString", "coordinates": [[-97.5, 35.2], [-96.0, 34.0]]}},
                {"type": "Feature", "properties": {},
                 "geometry": {"type": "MultiLineString", "coordinates": [[[-97.5, 35.2], [-96.0, 34.0]]]}},
                {"type": "Feature", "properties": {},
                 "geometry": {"type": "Polygon", "coordinates": [[[-98.0, 35.0], [-97.0, 35.0], [-97.0, 36.0], [-98.0, 35.0]]]}},
                {"type": "Feature", "properties": {},
                 "geometry": {"type": "MultiPolygon", "coordinates": [[[[-98.0, 35.0], [-97.0, 35.0], [-97.0, 36.0], [-98.0, 35.0]]]]}}
            ]
        }"#;
        let features = parse_geojson(json).unwrap();
        assert_eq!(features.len(), 6);
        assert_eq!(features[0].geometry.kind_name(), "Point");
        assert_eq!(features[1].geometry.kind_name(), "MultiPoint");
        assert_eq!(features[2].geometry.kind_name(), "LineString");
        assert_eq!(features[3].geometry.kind_name(), "MultiLineString");
        assert_eq!(features[4].geometry.kind_name(), "Polygon");
        assert_eq!(features[5].geometry.kind_name(), "MultiPolygon");
        assert_eq!(
            features[0].properties.get("name").and_then(|v| v.as_str()),
            Some("a")
        );
        assert_eq!(
            features[0].geometry,
            Geometry::Point([-97.5, 35.2]),
            "coordinates must survive the round trip exactly"
        );
    }

    #[test]
    fn parses_a_bare_feature_and_a_bare_geometry_not_just_a_collection() {
        let feature = r#"{"type": "Feature", "properties": {"x": 1},
            "geometry": {"type": "Point", "coordinates": [1.0, 2.0]}}"#;
        assert_eq!(parse_geojson(feature).unwrap().len(), 1);

        let geometry = r#"{"type": "Point", "coordinates": [1.0, 2.0]}"#;
        let features = parse_geojson(geometry).unwrap();
        assert_eq!(features.len(), 1);
        assert!(
            features[0].properties.is_empty(),
            "a bare geometry has no feature to carry properties"
        );
    }

    #[test]
    fn a_geometry_collection_flattens_into_one_feature_per_shape() {
        let json = r#"{"type": "Feature", "properties": {},
            "geometry": {"type": "GeometryCollection", "geometries": [
                {"type": "Point", "coordinates": [1.0, 2.0]},
                {"type": "LineString", "coordinates": [[1.0, 2.0], [3.0, 4.0]]}
            ]}}"#;
        let features = parse_geojson(json).unwrap();
        assert_eq!(features.len(), 2);
        assert_eq!(features[0].geometry.kind_name(), "Point");
        assert_eq!(features[1].geometry.kind_name(), "LineString");
    }

    #[test]
    fn a_position_with_too_few_coordinates_is_a_parse_error_not_a_bogus_point() {
        // The `geojson` crate itself refuses to deserialize a Position with fewer than two
        // elements, before this module's own code ever sees it — confirmed here rather than
        // assumed, since that's what actually keeps a malformed point from silently mislocating
        // as Null Island. `Geometry::Point`'s own construction in `geometries_of` is a second,
        // defensive line for a case this test shows the parser already can't produce.
        let json = r#"{"type": "Feature", "properties": {},
            "geometry": {"type": "Point", "coordinates": [1.0]}}"#;
        let err = parse_geojson(json).unwrap_err();
        assert!(err.to_string().contains("two or more elements"), "{err}");
    }

    #[test]
    fn invalid_json_is_a_named_error_not_an_empty_result() {
        let err = parse_geojson("not json at all").unwrap_err();
        assert!(err.to_string().contains("geojson parse"), "{err}");
    }

    #[test]
    fn an_upstream_error_report_is_a_named_error_not_an_empty_result() {
        // Reuses overlay::for_each_feature's own ArcGIS-error detection -- confirming this
        // module's parse_geojson actually goes through it, not a duplicated parser that missed it.
        let err =
            parse_geojson(r#"{"error":{"code":404,"message":"Layer not found"}}"#).unwrap_err();
        assert!(err.to_string().contains("Layer not found"), "{err}");
    }

    #[test]
    fn polygon_holes_survive_as_rings_after_the_outer_boundary() {
        let json = r#"{"type": "Feature", "properties": {},
            "geometry": {"type": "Polygon", "coordinates": [
                [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]],
                [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 1.0]]
            ]}}"#;
        let features = parse_geojson(json).unwrap();
        let Geometry::Polygon(rings) = &features[0].geometry else {
            panic!("expected a polygon");
        };
        assert_eq!(rings.len(), 2, "outer boundary plus one hole");
        assert_eq!(rings[0].len(), 5);
        assert_eq!(rings[1].len(), 4);
    }
}
