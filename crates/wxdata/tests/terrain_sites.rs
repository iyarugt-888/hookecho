//! Ground under three radars from pinned AWS Terrain Tiles (zoom 11, Terrarium PNGs fetched
//! 2026-10-06 from `s3.amazonaws.com/elevation-tiles-prod/terrarium/11/{x}/{y}.png`), checked
//! against each site's published antenna-site elevation in the NEXRAD registry — an independent
//! survey value. Plains sites agree to a few metres; a mountaintop radar stands above the smoothed
//! ~50 m grid cell, which is the limitation the module states.
//!
//! - `terrarium_11_470_808.png` (KTLX, Oklahoma City), 121,550 bytes, SHA-256 `b828a29d…`
//! - `terrarium_11_429_776.png` (KFTG, Denver), 106,649 bytes, SHA-256 `5d817a68…`
//! - `terrarium_11_375_719.png` (KMSX, Missoula, on Point Six), 136,522 bytes, SHA-256 `47f685fe…`

use wxdata::terrain::{decode, locate};

#[test]
fn ground_under_radars_matches_their_published_elevations() {
    for (id, tile, tolerance_m) in [
        (
            "KTLX",
            &include_bytes!("data/terrain/terrarium_11_470_808.png")[..],
            15.0,
        ),
        (
            "KFTG",
            &include_bytes!("data/terrain/terrarium_11_429_776.png")[..],
            15.0,
        ),
        // A summit radar: the grid cell around it averages the slopes below.
        (
            "KMSX",
            &include_bytes!("data/terrain/terrarium_11_375_719.png")[..],
            30.0,
        ),
    ] {
        let site = wxdata::sites::site_by_id(id).unwrap();
        let (lon, lat) = (site.longitude as f64, site.latitude as f64);
        let (tile_id, _, _) = locate(lon, lat, 11).unwrap();
        let t = decode(tile_id, tile).expect("decodes");
        let ground = t.height_at(lon, lat).expect("in this tile");
        let published = f64::from(site.elevation_meters);
        eprintln!("{id}: ground {ground:.1} m, published {published} m");
        assert!(
            (f64::from(ground) - published).abs() <= tolerance_m,
            "{id}: {ground} vs {published}"
        );
        assert!(
            t.height_at(lon + 1.0, lat).is_none(),
            "another tile's point"
        );
    }
}
