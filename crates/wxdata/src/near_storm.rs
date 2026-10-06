//! The near-storm environment beside a rotation column (detectionplan.md, "Environment"): what an
//! HRRR hour says about the air feeding a storm. Instability, low-level helicity, cloud-base height
//! and deep shear, and the fixed-layer Significant Tornado Parameter they make
//! ([`crate::severe::stp`]), the ingredients that separate tornadic from non-tornadic supercells in
//! the climatologies (Thompson et al. 2003, 2012) and that NSSL's ProbSevere ProbTor feeds beside
//! its radar evidence. The radar sees rotation; the environment says how likely rotation that
//! strong is to reach the ground.
//!
//! A storm changes its own grid points: rain-cooled outflow and anvil shade cut the model's CAPE
//! under it and lift its LCL. So each ingredient is read over the inflow, every cell within
//! [`INFLOW_KM`] of the column, at the favourable end ([`HIGH`] percentile, the low one for the LCL
//! height), rather than at the column's own point. STP is computed cell by cell and read the same
//! way, so it is never built from ingredients of different places.

use crate::mrms::MrmsField;
use crate::severe::stp;

/// The HRRR surface-file fields the environment reads, as `(var, level, min_valid)` for
/// [`crate::hrrr::fetch_field_at_run`]. Helicity, shear components, CIN and heights keep their
/// negatives.
pub const HRRR_SPECS: [(&str, &str, f64); 8] = [
    ("CAPE", "surface", 0.0),
    ("CAPE", "90-0 mb above ground", 0.0),
    ("CIN", "90-0 mb above ground", f64::NEG_INFINITY),
    ("HLCY", "1000-0 m above ground", f64::NEG_INFINITY),
    ("HLCY", "3000-0 m above ground", f64::NEG_INFINITY),
    ("VUCSH", "0-6000 m above ground", f64::NEG_INFINITY),
    ("VVCSH", "0-6000 m above ground", f64::NEG_INFINITY),
    // Above ground already (see `severe::fetch_grid`): over the mountains it reads far below the
    // terrain height, which a height above sea level cannot.
    (
        "HGT",
        "level of adiabatic condensation from sfc",
        f64::NEG_INFINITY,
    ),
];

const SBCAPE: usize = 0;
const MLCAPE: usize = 1;
const MLCIN: usize = 2;
const SRH1: usize = 3;
const SRH3: usize = 4;
const USHEAR: usize = 5;
const VSHEAR: usize = 6;
const LCL: usize = 7;

/// How far from the column the inflow is read (km): a supercell's inflow region, wide enough to
/// reach past its own outflow, narrow enough to stay on its side of a boundary.
pub const INFLOW_KM: f64 = 40.0;
/// The percentile read at the favourable end of each ingredient over the inflow.
pub const HIGH: f64 = 0.9;

/// One HRRR hour's ingredients, in [`HRRR_SPECS`] order, all on one grid.
#[derive(Clone)]
pub struct EnvHour {
    fields: Vec<MrmsField>,
}

/// The environment beside one column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvSample {
    /// Surface-based and mixed-layer (lowest 90 mb) CAPE, J/kg.
    pub sbcape: f32,
    pub mlcape: f32,
    /// Mixed-layer CIN, J/kg (negative; the inflow's weakest inhibition).
    pub mlcin: f32,
    /// Storm-relative helicity over 0-1 and 0-3 km, m²/s².
    pub srh1: f32,
    pub srh3: f32,
    /// 0-6 km bulk shear magnitude, m/s.
    pub shear6: f32,
    /// LCL height above ground, m (the inflow's lowest).
    pub lcl_m: f32,
    /// Fixed-layer STP, cell by cell, read over the inflow.
    pub stp: f32,
    /// Fixed-layer STP at the column's own grid point, for comparison with the inflow read.
    pub stp_point: f32,
    /// Grid cells the inflow read covered.
    pub cells: usize,
}

impl EnvHour {
    /// The ingredients of one hour, in [`HRRR_SPECS`] order. Every grid must match the first.
    pub fn new(fields: Vec<MrmsField>) -> anyhow::Result<Self> {
        anyhow::ensure!(
            fields.len() == HRRR_SPECS.len(),
            "{} ingredient grids, not {}",
            fields.len(),
            HRRR_SPECS.len()
        );
        let f0 = &fields[0];
        anyhow::ensure!(
            fields.iter().all(|f| f.nx == f0.nx
                && f.ny == f0.ny
                && f.values.len() == f0.values.len()
                && f.lon_west == f0.lon_west
                && f.lat_north == f0.lat_north),
            "ingredient grids disagree"
        );
        Ok(EnvHour { fields })
    }

    fn cell(&self, k: usize, i: usize) -> f64 {
        self.fields[k].values[i] as f64
    }

    /// Cell `i`'s STP, its 0-6 km shear and LCL height; NaN where an ingredient is missing.
    fn derived(&self, i: usize) -> (f64, f64, f64) {
        let shear = self.cell(USHEAR, i).hypot(self.cell(VSHEAR, i));
        let lcl = self.cell(LCL, i).max(0.0);
        let s = stp(self.cell(SBCAPE, i), self.cell(SRH1, i), shear, lcl);
        (s, shear, lcl)
    }

    /// The environment beside a column at `(lon, lat)`; `None` where the inflow has no complete
    /// cell (off the grid, or masked).
    pub fn sample(&self, lon: f64, lat: f64) -> Option<EnvSample> {
        let g = &self.fields[0];
        if g.nx == 0 || g.ny == 0 {
            return None;
        }
        let dlon = (g.lon_east - g.lon_west) / g.nx as f64;
        let dlat = (g.lat_north - g.lat_south) / g.ny as f64;
        let wy = (INFLOW_KM / 111.0 / dlat).ceil() as isize + 1;
        let wx = (INFLOW_KM / (111.0 * lat.to_radians().cos().abs().max(0.05)) / dlon).ceil()
            as isize
            + 1;
        let cx = ((lon - g.lon_west) / dlon - 0.5).round() as isize;
        let cy = ((g.lat_north - lat) / dlat - 0.5).round() as isize;
        let mut cols: [Vec<f64>; 8] = Default::default();
        let mut point = f64::NAN;
        let mut point_d = f64::INFINITY;
        for iy in (cy - wy).max(0)..=(cy + wy).min(g.ny as isize - 1) {
            for ix in (cx - wx).max(0)..=(cx + wx).min(g.nx as isize - 1) {
                let clon = g.lon_west + (ix as f64 + 0.5) * dlon;
                let clat = g.lat_north - (iy as f64 + 0.5) * dlat;
                let d = ground_km(lon, lat, clon, clat);
                if d > INFLOW_KM {
                    continue;
                }
                let i = iy as usize * g.nx + ix as usize;
                let (s, shear, lcl) = self.derived(i);
                let row = [
                    self.cell(SBCAPE, i),
                    self.cell(MLCAPE, i),
                    self.cell(MLCIN, i),
                    self.cell(SRH1, i),
                    self.cell(SRH3, i),
                    shear,
                    lcl,
                    s,
                ];
                if row.iter().any(|v| !v.is_finite()) {
                    continue;
                }
                if d < point_d {
                    point_d = d;
                    point = s;
                }
                for (c, v) in cols.iter_mut().zip(row) {
                    c.push(v);
                }
            }
        }
        let cells = cols[0].len();
        if cells == 0 {
            return None;
        }
        let hi = |c: &mut Vec<f64>| percentile(c, HIGH) as f32;
        let [sb, ml, cin, s1, s3, sh, lcl, st] = &mut cols;
        Some(EnvSample {
            sbcape: hi(sb),
            mlcape: hi(ml),
            mlcin: hi(cin),
            srh1: hi(s1),
            srh3: hi(s3),
            shear6: hi(sh),
            lcl_m: percentile(lcl, 1.0 - HIGH) as f32,
            stp: hi(st),
            stp_point: point as f32,
            cells,
        })
    }
}

/// The `q` quantile of `v` (nearest rank), which must be non-empty and finite.
fn percentile(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    let k = ((v.len() - 1) as f64 * q).round() as usize;
    v[k.min(v.len() - 1)]
}

fn ground_km(lon1: f64, lat1: f64, lon2: f64, lat2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let (dp, dl) = ((lat2 - lat1).to_radians(), (lon2 - lon1).to_radians());
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    6371.0 * 2.0 * h.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 0.04° grid over Oklahoma with every ingredient uniform, then edited cell by cell.
    fn hour(values: [f32; 8]) -> EnvHour {
        let (nx, ny) = (100, 75);
        let field = |v: f32| MrmsField {
            values: vec![v; nx * ny],
            nx,
            ny,
            lon_west: -100.0,
            lon_east: -96.0,
            lat_north: 37.0,
            lat_south: 34.0,
            time: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        };
        EnvHour::new(values.iter().map(|&v| field(v)).collect()).unwrap()
    }

    // SBCAPE, MLCAPE, MLCIN, SRH1, SRH3, U shear, V shear, LCL height above ground.
    const MOIST: [f32; 8] = [3000.0, 2500.0, -20.0, 300.0, 400.0, 18.0, 24.0, 900.0];

    #[test]
    fn a_uniform_environment_reads_its_own_values_and_stp() {
        let s = hour(MOIST).sample(-98.0, 35.5).expect("inside");
        assert_eq!((s.sbcape, s.mlcape, s.mlcin), (3000.0, 2500.0, -20.0));
        assert_eq!((s.srh1, s.srh3), (300.0, 400.0));
        assert!((s.shear6 - 30.0).abs() < 1e-4, "{}", s.shear6);
        assert!((s.lcl_m - 900.0).abs() < 1e-3, "{}", s.lcl_m);
        let want = stp(3000.0, 300.0, 30.0, 900.0) as f32;
        assert!((s.stp - want).abs() < 1e-5 && (s.stp_point - want).abs() < 1e-5);
        // 40 km of 0.04° cells at 35.5° N: on the order of 400 cells.
        assert!((300..600).contains(&s.cells), "{}", s.cells);
    }

    #[test]
    fn the_storms_own_outflow_does_not_hide_its_inflow() {
        // A rain-cooled pool 10 km across under the column: no CAPE, a high cloud base.
        let mut h = hour(MOIST);
        let g = h.fields[0].clone();
        let (dlon, dlat) = (0.04, 0.04);
        for iy in 0..g.ny {
            for ix in 0..g.nx {
                let lon = g.lon_west + (ix as f64 + 0.5) * dlon;
                let lat = g.lat_north - (iy as f64 + 0.5) * dlat;
                if ground_km(-98.0, 35.5, lon, lat) < 10.0 {
                    let i = iy * g.nx + ix;
                    h.fields[SBCAPE].values[i] = 0.0;
                    h.fields[MLCAPE].values[i] = 0.0;
                    h.fields[LCL].values[i] = 3000.0;
                }
            }
        }
        let s = h.sample(-98.0, 35.5).unwrap();
        assert_eq!(s.sbcape, 3000.0, "the inflow, not the cold pool");
        assert!((s.lcl_m - 900.0).abs() < 1e-3);
        assert!(s.stp > 2.0, "{}", s.stp);
        assert_eq!(s.stp_point, 0.0, "the column's own point is in the pool");
    }

    #[test]
    fn a_dry_or_missing_environment_reads_as_such() {
        let mut dry = MOIST;
        dry[SBCAPE] = 0.0;
        assert_eq!(hour(dry).sample(-98.0, 35.5).unwrap().stp, 0.0);
        // Off the grid, and inside a masked grid: no sample rather than zeros.
        assert!(hour(MOIST).sample(-90.0, 35.5).is_none());
        let mut masked = hour(MOIST);
        masked.fields[SRH1].values.fill(f32::NAN);
        assert!(masked.sample(-98.0, 35.5).is_none());
        // Grids that disagree are refused.
        let mut fields = hour(MOIST).fields;
        fields[3].nx -= 1;
        assert!(EnvHour::new(fields).is_err());
    }
}
