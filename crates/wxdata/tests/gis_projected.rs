//! Projected shapefiles inverse-projected against PROJ (ROADMAP_PARITY M4.2's non-WGS84 fixtures).
//!
//! The 77 Oklahoma counties of the Census Bureau's 2023 20M cartographic boundaries
//! (`data/gis/cb_2023_us_county_20m.zip`, NAD83 geographic), and the same counties reprojected by
//! PROJ through GDAL 3.8.4's `ogr2ogr -t_srs` into three systems, each zipped with its `.shp`,
//! `.shx`, `.dbf`, `.prj` (as GDAL writes it) and `.cpg`, made 2026-10-06:
//!
//! - `ok_counties_nad83.zip` — the reference, 14,921 bytes, SHA-256 `354f3b1b…5d1b`
//! - `ok_counties_26914.zip` — NAD83 / UTM zone 14N (Transverse Mercator), 15,710 bytes,
//!   SHA-256 `4c06d710…e9ce`
//! - `ok_counties_2268.zip` — NAD83 / Oklahoma South State Plane, US survey feet (Lambert
//!   Conformal Conic, two parallels), 15,909 bytes, SHA-256 `2950ea15…82ea`
//! - `ok_counties_5070.zip` — NAD83 / CONUS Albers Equal Area, 15,942 bytes, SHA-256
//!   `0e3c2c5f…67c7`
//!
//! `ogr2ogr` transforms coordinates one for one, so each county's vertices after this app's
//! inverse projection should land on the reference vertices. The check is the largest distance
//! between any pair, for every vertex of every county, matched by GEOID from each file's own
//! `.dbf` — PROJ is the independent implementation.

use std::collections::HashMap;
use wxdata::gis::Geometry;

fn vertices(g: &Geometry) -> Vec<[f64; 2]> {
    match g {
        Geometry::Polygon(rings) => rings.iter().flatten().copied().collect(),
        Geometry::MultiPolygon(parts) => parts.iter().flatten().flatten().copied().collect(),
        other => panic!("not a county polygon: {other:?}"),
    }
}

fn by_geoid(zip: &[u8]) -> HashMap<String, Vec<[f64; 2]>> {
    let sets = wxdata::shapefile::parse_zip(zip).expect("the bundle imports");
    assert_eq!(sets.len(), 1);
    assert!(sets[0].notes.is_empty(), "{:?}", sets[0].notes);
    sets[0]
        .features
        .iter()
        .map(|f| {
            (
                f.properties["GEOID"].as_str().unwrap().to_string(),
                vertices(&f.geometry),
            )
        })
        .collect()
}

/// Metres between two nearby geographic points.
fn metres(a: [f64; 2], b: [f64; 2]) -> f64 {
    let lat = a[1].to_radians();
    let dy = (b[1] - a[1]) * 110_574.0;
    let dx = (b[0] - a[0]) * 111_320.0 * lat.cos();
    dx.hypot(dy)
}

#[test]
fn projected_counties_land_on_the_geographic_ones_proj_made_them_from() {
    let reference = by_geoid(include_bytes!("data/gis/ok_counties_nad83.zip"));
    assert_eq!(reference.len(), 77, "Oklahoma's counties");
    for (name, zip) in [
        (
            "UTM 14N",
            &include_bytes!("data/gis/ok_counties_26914.zip")[..],
        ),
        (
            "Oklahoma South ftUS",
            &include_bytes!("data/gis/ok_counties_2268.zip")[..],
        ),
        (
            "CONUS Albers",
            &include_bytes!("data/gis/ok_counties_5070.zip")[..],
        ),
    ] {
        let got = by_geoid(zip);
        assert_eq!(got.len(), reference.len(), "{name}");
        let (mut worst, mut n) = (0.0_f64, 0usize);
        for (geoid, want) in &reference {
            let have = &got[geoid];
            assert_eq!(have.len(), want.len(), "{name} {geoid}");
            for (a, b) in have.iter().zip(want) {
                worst = worst.max(metres(*a, *b));
                n += 1;
            }
        }
        eprintln!("{name}: {n} vertices, worst {worst:.4} m from PROJ's");
        assert!(
            worst < 0.01,
            "{name}: a vertex {worst} m from where PROJ put it"
        );
    }
}
