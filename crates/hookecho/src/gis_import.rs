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
        load_geojson(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
    }
    #[cfg(target_arch = "wasm32")]
    {
        Err(format!("{path} cannot be reopened in a browser"))
    }
}

/// Read what the picker just returned, whichever format it is.
pub(crate) fn load_import(import: &crate::dialog::Import) -> Result<Loaded, String> {
    if !is_shapefile(&import.name()) {
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
    fn imported_polygon_style_changes_only_paint_not_identity_or_geometry() {
        let rings = vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]];
        let mut f = polygon_feature(rings.clone(), "District".into(), "id: 7".into());
        apply_style(
            &mut f,
            ImportedGisStyle {
                color: [240, 80, 40],
                opacity: 0.5,
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
        assert!(!is_shapefile("c.geojson"));
        assert!(!is_shapefile("c.shp.json"));
    }
}
