//! A real zipped shapefile bundle, checked against independent coordinates (ROADMAP_PARITY M4.2).
//!
//! `cb_2023_us_county_20m.zip` is the U.S. Census Bureau's 2023 cartographic county boundaries at
//! 1:20,000,000, exactly as published (`.shp`, `.shx`, `.dbf`, `.prj` in NAD83, `.cpg` UTF-8 and
//! two metadata XMLs). `2023_Gaz_counties_national.zip` is the Census 2023 Gazetteer, which
//! publishes an internal point for every county from the full-resolution TIGER boundaries: an
//! independent source for where each county is. Both are public domain, from www2.census.gov:
//!
//! - `geo/tiger/GENZ2023/shp/cb_2023_us_county_20m.zip`, 900,375 bytes, SHA-256
//!   `479956f6e3cb1573705c1ceb35d17ca7cd1b5179f3947ee6adbaa274b7e931e4`
//! - `geo/docs/maps-data/data/gazetteer/2023_Gazetteer/2023_Gaz_counties_national.zip`,
//!   141,650 bytes, SHA-256 `919df59ba90759cce85468c0337e898e4b39c08eaffce86ddd88fa41f1f7f0c8`
//!
//! Retrieved 2026-10-05. The import is right when each county's polygon, found by the GEOID in its
//! own `.dbf` row, contains the Gazetteer's internal point for that GEOID: that checks geometry,
//! projection and attribute alignment together.

use std::collections::HashMap;
use wxdata::gis::Geometry;

const COUNTIES: &[u8] = include_bytes!("data/gis/cb_2023_us_county_20m.zip");
const GAZETTEER: &[u8] = include_bytes!("data/gis/2023_Gaz_counties_national.zip");

/// GEOID to (longitude, latitude) of each county's internal point.
fn gazetteer() -> HashMap<String, (f64, f64)> {
    let entries = wxdata::zip::entries(GAZETTEER).expect("gazetteer zip");
    let txt = entries
        .iter()
        .find(|e| e.name.ends_with(".txt"))
        .expect("gazetteer text");
    let bytes = wxdata::zip::read(GAZETTEER, txt, 4 << 20).expect("gazetteer reads");
    let text = String::from_utf8(bytes).expect("utf-8");
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap().split('\t').map(str::trim).collect();
    let col = |n: &str| header.iter().position(|h| *h == n).unwrap();
    let (g, la, lo) = (col("GEOID"), col("INTPTLAT"), col("INTPTLONG"));
    lines
        .map(|l| l.split('\t').map(str::trim).collect::<Vec<_>>())
        .filter(|f| f.len() > lo)
        .map(|f| {
            (
                f[g].to_string(),
                (f[lo].parse().unwrap(), f[la].parse().unwrap()),
            )
        })
        .collect()
}

fn contains(geometry: &Geometry, lon: f64, lat: f64) -> bool {
    let in_polygon = |rings: &Vec<Vec<[f64; 2]>>| {
        rings
            .first()
            .is_some_and(|outer| wxdata::overlay::point_in_ring(outer, lon, lat))
            && !rings[1..]
                .iter()
                .any(|hole| wxdata::overlay::point_in_ring(hole, lon, lat))
    };
    match geometry {
        Geometry::Polygon(rings) => in_polygon(rings),
        Geometry::MultiPolygon(parts) => parts.iter().any(in_polygon),
        _ => false,
    }
}

#[test]
fn census_counties_import_with_their_attributes_where_the_gazetteer_puts_them() {
    let sets = wxdata::shapefile::parse_zip(COUNTIES).expect("the published bundle imports");
    assert_eq!(sets.len(), 1, "one dataset");
    let set = &sets[0];
    assert_eq!(set.name, "cb_2023_us_county_20m.shp");
    assert!(
        set.notes.is_empty(),
        "every sidecar honoured: {:?}",
        set.notes
    );

    let points = gazetteer();
    let (mut matched, mut inside) = (0usize, 0usize);
    let mut outside = Vec::new();
    for f in &set.features {
        let geoid = f.properties["GEOID"].as_str().expect("GEOID text");
        assert!(f.properties["NAME"].as_str().is_some_and(|n| !n.is_empty()));
        assert_eq!(&geoid[..2], f.properties["STATEFP"].as_str().unwrap());
        let Some(&(lon, lat)) = points.get(geoid) else {
            continue;
        };
        matched += 1;
        if contains(&f.geometry, lon, lat) {
            inside += 1;
        } else {
            outside.push(geoid.to_string());
        }
    }
    eprintln!(
        "{} counties imported, {matched} with a Gazetteer point, {inside} contain it; outside: {outside:?}",
        set.features.len()
    );
    assert!(set.features.len() > 3200, "{}", set.features.len());
    assert!(
        matched as f64 >= 0.99 * set.features.len() as f64,
        "{matched}"
    );
    // The 1:20M boundaries are generalized, so a coastal or island county's internal point can fall
    // just outside its simplified outline. Measured: 3,222 imported, all with a Gazetteer point,
    // 3,215 containing it; the seven outside are San Francisco, Island WA, Door WI, Poquoson VA,
    // Franklin FL, Kalawao HI and Knox ME, every one on a coast or an island.
    assert!(
        inside as f64 >= 0.995 * matched as f64,
        "{inside} of {matched}"
    );

    // Spot checks by name and place.
    let find = |id: &str| {
        set.features
            .iter()
            .find(|f| f.properties["GEOID"] == id)
            .unwrap_or_else(|| panic!("{id}"))
    };
    let cook = find("17031");
    assert_eq!(cook.properties["NAME"], "Cook");
    assert!(
        contains(&cook.geometry, -87.75, 41.84),
        "Chicago's west side is in Cook County"
    );
    let oklahoma = find("40109");
    assert_eq!(oklahoma.properties["NAME"], "Oklahoma");
    assert!(
        contains(&oklahoma.geometry, -97.52, 35.47),
        "downtown Oklahoma City"
    );
    assert!(
        !contains(&oklahoma.geometry, -97.44, 35.22),
        "Norman is in Cleveland County"
    );
}
