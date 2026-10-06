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
/// Under this inflow STP a Possible Tornado ID verdict is not shown
/// ([`crate::llsd_analyst::VerdictOptions::environment`]). On eight random years of severe-weather
/// windows it took the app's false markers from 0.56 to 0.31 per radar-hour, fewer in every year,
/// at POD 0.38 to 0.33; on four random tornado samples it cost at most a point of POD
/// (detectionplan.md, "The near-storm environment").
pub const GATE_STP: f32 = 0.25;

/// The HRRR archive on AWS begins at 2014-07-30 00Z (seconds since the epoch).
pub const ARCHIVE_START: i64 = 1_406_678_400;
/// Where an hour's fields come from, in order: (hours before the valid hour the run began,
/// forecast hour). The run an hour before at F+1 is what a live app has by then; when it is not
/// there (a run missing from the archive, or not yet posted), the on-hour analysis, then the run
/// two hours before.
pub const SOURCES: [(i64, u8); 3] = [(1, 1), (0, 0), (2, 2)];

/// One HRRR hour's ingredients, in [`HRRR_SPECS`] order, all on one grid.
#[derive(Clone)]
pub struct EnvHour {
    fields: Vec<MrmsField>,
}

impl std::fmt::Debug for EnvHour {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let g = &self.fields[0];
        f.debug_struct("EnvHour")
            .field("valid", &g.time)
            .field("nx", &g.nx)
            .field("ny", &g.ny)
            .finish()
    }
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

impl EnvSample {
    /// The environment in one line, for a detection's card: what STP is made of, in the units the
    /// rest of the card uses.
    pub fn summary(&self) -> String {
        format!(
            "STP {:.1} \u{b7} 0\u{2013}1 km helicity {:.0} m\u{b2}/s\u{b2} \u{b7} CAPE {:.0} J/kg \u{b7} \
             cloud base {:.0} m \u{b7} 0\u{2013}6 km shear {:.0} m/s",
            self.stp, self.srh1, self.sbcape, self.lcl_m, self.shear6
        )
    }
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

impl EnvHour {
    /// Whether the air beside a column at `(lon, lat)` rules out a Possible tornado: inflow STP
    /// under [`GATE_STP`]. No sample (off the grid, or masked) rules nothing out.
    pub fn rules_out(&self, lon: f64, lat: f64) -> bool {
        self.sample(lon, lat).is_some_and(|s| s.stp < GATE_STP)
    }

    /// The hour cut to the cells a column within `radius_km` of `(lon, lat)` can read (that far,
    /// and the inflow beyond it): one radar's coverage is a few hundred KB, the CONUS grid tens of
    /// MB. A column inside the radius samples exactly as it does on the whole grid.
    pub fn crop(&self, lon: f64, lat: f64, radius_km: f64) -> EnvHour {
        let g = &self.fields[0];
        if g.nx == 0 || g.ny == 0 {
            return self.clone();
        }
        let dlon = (g.lon_east - g.lon_west) / g.nx as f64;
        let dlat = (g.lat_north - g.lat_south) / g.ny as f64;
        let r = radius_km + INFLOW_KM + 2.0 * 111.0 * dlat;
        let ry = r / 111.0;
        let rx = r / (111.0 * (lat.abs() + ry).min(89.0).to_radians().cos());
        let col = |lon: f64| ((lon - g.lon_west) / dlon).clamp(0.0, g.nx as f64);
        let row = |lat: f64| ((g.lat_north - lat) / dlat).clamp(0.0, g.ny as f64);
        let (x0, x1) = (
            col(lon - rx).floor() as usize,
            col(lon + rx).ceil() as usize,
        );
        let (y0, y1) = (
            row(lat + ry).floor() as usize,
            row(lat - ry).ceil() as usize,
        );
        let (nx, ny) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        let fields = self
            .fields
            .iter()
            .map(|f| MrmsField {
                values: (y0..y0 + ny)
                    .flat_map(|y| f.values[y * f.nx + x0..y * f.nx + x0 + nx].iter().copied())
                    .collect(),
                nx,
                ny,
                lon_west: g.lon_west + x0 as f64 * dlon,
                lon_east: g.lon_west + (x0 + nx) as f64 * dlon,
                lat_north: g.lat_north - y0 as f64 * dlat,
                lat_south: g.lat_north - (y0 + ny) as f64 * dlat,
                time: f.time,
            })
            .collect();
        EnvHour { fields }
    }

    /// The hour as bytes, for a cache: a header, then each field's cells, little-endian.
    pub fn to_bytes(&self) -> Vec<u8> {
        let g = &self.fields[0];
        let mut out = Vec::with_capacity(48 + self.fields.len() * g.values.len() * 4);
        out.extend_from_slice(BYTES_MAGIC);
        for v in [g.nx as u32, g.ny as u32, self.fields.len() as u32] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in [g.lon_west, g.lon_east, g.lat_north, g.lat_south] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&g.time.timestamp().to_le_bytes());
        for f in &self.fields {
            for v in &f.values {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }

    /// An hour written by [`Self::to_bytes`].
    pub fn from_bytes(b: &[u8]) -> anyhow::Result<EnvHour> {
        anyhow::ensure!(b.starts_with(BYTES_MAGIC), "not a near-storm hour");
        let mut at = BYTES_MAGIC.len();
        let mut take = |n: usize| -> anyhow::Result<&[u8]> {
            let s = b
                .get(at..at + n)
                .ok_or_else(|| anyhow::anyhow!("near-storm hour cut short"))?;
            at += n;
            Ok(s)
        };
        let u = |s: &[u8]| u32::from_le_bytes(s.try_into().unwrap()) as usize;
        let f = |s: &[u8]| f64::from_le_bytes(s.try_into().unwrap());
        let (nx, ny, n) = (u(take(4)?), u(take(4)?), u(take(4)?));
        anyhow::ensure!(
            n == HRRR_SPECS.len(),
            "{n} fields, not {}",
            HRRR_SPECS.len()
        );
        let (lon_west, lon_east, lat_north, lat_south) =
            (f(take(8)?), f(take(8)?), f(take(8)?), f(take(8)?));
        let time = chrono::DateTime::from_timestamp(i64::from_le_bytes(take(8)?.try_into()?), 0)
            .ok_or_else(|| anyhow::anyhow!("bad time"))?;
        let mut fields = Vec::with_capacity(n);
        for _ in 0..n {
            let values = take(nx * ny * 4)?
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                .collect();
            fields.push(MrmsField {
                values,
                nx,
                ny,
                lon_west,
                lon_east,
                lat_north,
                lat_south,
                time,
            });
        }
        anyhow::ensure!(at == b.len(), "near-storm hour has trailing bytes");
        EnvHour::new(fields)
    }
}

const BYTES_MAGIC: &[u8] = b"HEENV001";

/// Whether a fetch failed because the source is not there (no such file, or a field the file
/// lacks), rather than for a reason that may pass (a timeout, a server error).
pub fn is_missing(e: &anyhow::Error) -> bool {
    let text = format!("{e:#}");
    text.contains("404") || text.contains(" in idx")
}

/// The ingredients valid at `valid` (on the hour), from the first of [`SOURCES`] that has every
/// field, and the run they came from. Only a source that is missing ([`is_missing`]) passes the
/// hour to the next: any other failure is returned, so a timeout cannot quietly put another run's
/// air under the verdicts (the on-hour analysis read STP 1.78 where the run an hour before read
/// 1.06 at the KHGX 2023-01-24 tornado).
pub async fn fetch_hour(
    http: &reqwest::Client,
    valid: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<(chrono::DateTime<chrono::Utc>, EnvHour)> {
    anyhow::ensure!(
        valid.timestamp() >= ARCHIVE_START,
        "before the HRRR archive"
    );
    let mut last_err = None;
    for (back, fh) in SOURCES {
        let run = valid - chrono::Duration::hours(back);
        let fields =
            futures_util::future::try_join_all(HRRR_SPECS.iter().map(|(var, level, mv)| {
                crate::hrrr::fetch_field_at_run(
                    http,
                    crate::hrrr::Model::Hrrr,
                    run,
                    var,
                    level,
                    fh,
                    *mv,
                )
            }))
            .await;
        match fields {
            Ok(f) => return Ok((run, EnvHour::new(f.into_iter().map(|f| f.field).collect())?)),
            Err(e) if is_missing(&e) => last_err = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no HRRR source")))
}

/// [`fetch_hour`] for a radar at `(lon, lat)`, cropped to `radius_km` of it ([`EnvHour::crop`]) and
/// kept in `cache` (a directory), as the backtest wants it: an archived hour never changes, so a
/// rerun reads it from disk. `None` for an hour that cannot be had: before the archive, or every
/// source missing from it (remembered, so it is not asked for again). A failure that may pass (a
/// timeout, a server error) is an error, and is not remembered.
#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_hour_near(
    http: &reqwest::Client,
    valid: chrono::DateTime<chrono::Utc>,
    lon: f64,
    lat: f64,
    radius_km: f64,
    cache: Option<&std::path::Path>,
) -> anyhow::Result<Option<EnvHour>> {
    if valid.timestamp() < ARCHIVE_START {
        return Ok(None);
    }
    // Named in full: the coordinates' decimal points rule out `with_extension`.
    let path = |ext: &str| {
        cache.map(|dir| {
            dir.join("hrrr-env").join(format!(
                "{}_{lat:.2}_{lon:.2}_{radius_km:.0}km.{ext}",
                valid.format("%Y%m%d%H")
            ))
        })
    };
    let (bin, none) = (path("bin"), path("none"));
    if let Some(p) = &none {
        if p.exists() {
            return Ok(None);
        }
    }
    if let Some(p) = &bin {
        if let Ok(b) = std::fs::read(p) {
            if let Ok(hour) = EnvHour::from_bytes(&b) {
                return Ok(Some(hour));
            }
        }
    }
    let hour = match fetch_hour(http, valid).await {
        Ok((_, hour)) => hour.crop(lon, lat, radius_km),
        Err(e) if is_missing(&e) => {
            if let Some(p) = &none {
                let _ = std::fs::create_dir_all(p.parent().unwrap());
                let _ = std::fs::write(p, format!("{e:#}"));
            }
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    if let Some(p) = &bin {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::write(p, hour.to_bytes());
    }
    Ok(Some(hour))
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
    fn the_gate_rules_out_quiet_air_and_nothing_it_cannot_see() {
        assert!(!hour(MOIST).rules_out(-98.0, 35.5), "a tornado environment");
        let mut weak = MOIST;
        weak[USHEAR] = 6.0;
        weak[VSHEAR] = 6.0; // 8.5 m/s of deep shear: STP's shear term is 0
        assert!(hour(weak).rules_out(-98.0, 35.5));
        assert!(!hour(weak).rules_out(-90.0, 35.5), "off the grid: no say");
    }

    #[test]
    fn a_cropped_hour_samples_as_the_whole_one_and_round_trips() {
        // Ingredients that vary cell by cell, so a misplaced crop would read other values.
        let mut h = hour(MOIST);
        for (k, f) in h.fields.iter_mut().enumerate() {
            for (i, v) in f.values.iter_mut().enumerate() {
                *v += ((i * 7 + k * 13) % 29) as f32;
            }
        }
        let small = h.crop(-98.0, 35.5, 60.0);
        assert!(small.fields[0].nx < h.fields[0].nx && small.fields[0].ny < h.fields[0].ny);
        for (lon, lat) in [(-98.0, 35.5), (-98.5, 35.9), (-97.4, 35.1)] {
            assert_eq!(small.sample(lon, lat), h.sample(lon, lat), "{lon},{lat}");
        }
        let back = EnvHour::from_bytes(&small.to_bytes()).unwrap();
        assert_eq!(back.sample(-98.0, 35.5), small.sample(-98.0, 35.5));
        assert!(EnvHour::from_bytes(&small.to_bytes()[..40]).is_err());
        // A radar off the grid crops to nothing, which rules nothing out.
        let none = h.crop(-80.0, 35.5, 60.0);
        assert!(none.sample(-80.0, 35.5).is_none() && !none.rules_out(-80.0, 35.5));
    }

    #[test]
    fn only_a_missing_source_is_missing() {
        let e = |s: &str| anyhow::anyhow!("{s}");
        assert!(is_missing(&e(
            "HTTP status client error (404 Not Found) for url (https://x.idx)"
        )));
        assert!(is_missing(&e(
            "no HGT:level of adiabatic condensation from sfc in idx"
        )));
        assert!(!is_missing(&e("operation timed out")));
        assert!(!is_missing(&e(
            "HTTP status server error (503 Service Unavailable)"
        )));
    }

    #[test]
    fn a_sample_reads_as_one_line() {
        let s = hour(MOIST).sample(-98.0, 35.5).unwrap();
        assert_eq!(
            s.summary(),
            "STP 4.0 \u{b7} 0\u{2013}1 km helicity 300 m\u{b2}/s\u{b2} \u{b7} CAPE 3000 J/kg \u{b7} \
             cloud base 900 m \u{b7} 0\u{2013}6 km shear 30 m/s"
        );
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
