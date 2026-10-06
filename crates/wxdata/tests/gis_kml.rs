//! KML and KMZ written by an independent implementation (ROADMAP_PARITY M4.2): GDAL 3.8.4's KML
//! and LIBKML drivers (`ogr2ogr -f KML … -dsco NameField=NAME`, `ogr2ogr -f LIBKML`) wrote the 77
//! Oklahoma counties of the pinned Census 2023 boundaries, 2026-10-06:
//!
//! - `ok_counties_gdal.kml`, 94,702 bytes
//! - `ok_counties_gdal.kmz`, 15,003 bytes
//!
//! Each county, found by the GEOID GDAL wrote into its `ExtendedData`, must import with that
//! attribute, its name, and every vertex where the reference NAD83 shapefile has it.

use std::collections::HashMap;
use wxdata::gis::{Geometry, GisFeature};

fn vertices(g: &Geometry) -> Vec<[f64; 2]> {
    match g {
        Geometry::Polygon(rings) => rings.iter().flatten().copied().collect(),
        Geometry::MultiPolygon(parts) => parts.iter().flatten().flatten().copied().collect(),
        other => panic!("not a county polygon: {other:?}"),
    }
}

fn by_geoid(features: &[GisFeature]) -> HashMap<String, (String, Vec<[f64; 2]>)> {
    features
        .iter()
        .map(|f| {
            let geoid = f.properties["GEOID"].as_str().expect("GEOID").to_string();
            let name = f
                .properties
                .get("NAME")
                .or_else(|| f.properties.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            (geoid, (name, vertices(&f.geometry)))
        })
        .collect()
}

#[test]
fn gdal_kml_and_kmz_import_with_their_attributes_and_vertices() {
    let shp = wxdata::shapefile::parse_zip(include_bytes!("data/gis/ok_counties_nad83.zip"))
        .expect("reference");
    let reference = by_geoid(&shp[0].features);
    let kml = wxdata::kml::parse(include_str!("data/gis/ok_counties_gdal.kml")).expect("kml");
    let kmz = wxdata::kml::parse_kmz(include_bytes!("data/gis/ok_counties_gdal.kmz")).expect("kmz");
    for (what, features) in [("KML", kml), ("KMZ", kmz)] {
        let got = by_geoid(&features);
        assert_eq!(got.len(), 77, "{what}");
        let mut worst = 0.0_f64;
        for (geoid, (name, want)) in &reference {
            let (got_name, have) = &got[geoid];
            assert_eq!(got_name, name, "{what} {geoid}");
            assert_eq!(have.len(), want.len(), "{what} {geoid}");
            for (a, b) in have.iter().zip(want) {
                worst = worst.max((a[0] - b[0]).abs().max((a[1] - b[1]).abs()));
            }
        }
        eprintln!("{what}: worst vertex {worst:e} degrees from the shapefile's");
        assert!(worst < 1e-9, "{what}: {worst}");
    }
}
