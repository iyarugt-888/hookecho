//! Where wind turbines stand, for masking their radar clutter: rotating blades scatter fast,
//! noisy velocities and strong echo, which the rotation pipeline can read as a small circulation in
//! a storm core. On 139 random severe-weather windows every Tornado ID marker within 2 km of a
//! turbine was false (25 of them, none verified), as were the 22 on the 65-event corpus
//! (detectionplan.md).
//!
//! From the USGS U.S. Wind Turbine Database (public domain), as 0.01-degree cells holding a
//! turbine, each with the earliest year one there came online so archived volumes are masked only
//! by turbines that stood then. `scripts/wind_turbines.py` builds `data/wind_turbines.bin`; see it
//! for the format.

use std::sync::OnceLock;

/// The database the embedded cells were built from.
pub const SOURCE: &str = "USGS U.S. Wind Turbine Database v9.0 (2026-06-26)";

const DATA: &[u8] = include_bytes!("../data/wind_turbines.bin");
const BASE_YEAR: i32 = 1980;
const NLON: u32 = 36_000;

/// The cells, keys ascending: `(key, year online)`, year 0 when the database gives none.
fn cells() -> &'static [(u32, i32)] {
    static CELLS: OnceLock<Vec<(u32, i32)>> = OnceLock::new();
    CELLS.get_or_init(|| decode(DATA).expect("embedded wind turbine cells decode"))
}

fn decode(data: &[u8]) -> Option<Vec<(u32, i32)>> {
    let rest = data.strip_prefix(b"HEWT")?;
    let n = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?) as usize;
    let mut rest = &rest[4..];
    let mut out = Vec::with_capacity(n);
    let mut key = 0u32;
    for _ in 0..n {
        let (mut delta, mut shift) = (0u32, 0);
        loop {
            let (&b, tail) = rest.split_first()?;
            rest = tail;
            delta |= u32::from(b & 0x7f) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                break;
            }
        }
        key += delta;
        let (&y, tail) = rest.split_first()?;
        rest = tail;
        out.push((key, if y == 0 { 0 } else { BASE_YEAR + i32::from(y) }));
    }
    rest.is_empty().then_some(out)
}

/// How many 0.01-degree cells hold a turbine.
pub fn cell_count() -> usize {
    cells().len()
}

/// Whether a turbine in service by `year` stands within `radius_km` of (lon, lat), measured to the
/// centre of its 0.01-degree cell (about 1 km across, so within ~0.7 km of exact). A turbine whose
/// year the database does not give counts as always in service.
pub fn near(lon: f64, lat: f64, year: i32, radius_km: f64) -> bool {
    let cells = cells();
    let (ila, ilo) = ((lat * 100.0).floor() as i64, (lon * 100.0).floor() as i64);
    let dla = (radius_km / 1.11).ceil() as i64 + 1;
    let dlo = (radius_km / (1.11 * lat.to_radians().cos().max(0.1))).ceil() as i64 + 1;
    for a in ila - dla..=ila + dla {
        let row = (a + 9000) * i64::from(NLON) + 18_000;
        let (lo, hi) = (row + ilo - dlo, row + ilo + dlo);
        let (Ok(lo), Ok(hi)) = (u32::try_from(lo), u32::try_from(hi)) else {
            continue;
        };
        let start = cells.partition_point(|&(k, _)| k < lo);
        for &(k, y) in cells[start..].iter().take_while(|&&(k, _)| k <= hi) {
            if y != 0 && y > year {
                continue;
            }
            let o = i64::from(k) - row;
            let centre = ((o as f64 + 0.5) / 100.0, (a as f64 + 0.5) / 100.0);
            if crate::tds::ground_km((lon, lat), centre) <= radius_km {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_cells_decode_and_place_known_farms() {
        assert!(cell_count() > 40_000, "{}", cell_count());
        // 25 Mile Creek, Ellis County OK: online 2022 (USWTDB), a turbine at -99.7786, 36.5025.
        assert!(near(-99.7786, 36.5025, 2023, 2.0));
        assert!(near(-99.7786, 36.5150, 2023, 2.0), "1.4 km north");
        assert!(!near(-99.7786, 36.5025, 2021, 2.0), "not built yet");
        // Open Gulf of Mexico, and central Oklahoma City: none.
        assert!(!near(-90.0, 26.0, 2026, 2.0));
        assert!(!near(-97.52, 35.47, 2026, 2.0));
    }

    #[test]
    fn a_truncated_file_does_not_decode() {
        assert!(decode(&DATA[..DATA.len() - 1]).is_none());
        assert!(decode(b"NOPE").is_none());
    }
}
