//! Column user-defined products (ROADMAP_PARITY M3.3): a formula built on a vertical/layer
//! function ([`crate::udp`]'s `max_vertical`, `max_layer`, `first_height_above`, …) reduces every
//! tilt over a ground point to one number, so its output is a 2D field — not a sweep and not a
//! voxel moment. This evaluates one on the same 0.01° grid the locally derived products use
//! ([`crate::derived`]), sampling each tilt through the same nearest-gate rule
//! ([`crate::xsection::gate_over_ground`]), so `max_vertical(REF)` is the local composite to the
//! bit.
//!
//! The gate inspector reads the same answer at a clicked point through [`evaluate_column`], which
//! applies the same rules to a column it sampled itself: the column's *base* (the inputs a bare
//! `REF` outside any vertical function reads, and where a layer's bounds are evaluated) is its
//! lowest sampled level, and a column where nothing the formula reads was recorded has no value.
//!
//! Environmental heights are taken only from [`ColumnEnv`], which the caller fills from a source
//! matched to the volume's own site and time; a formula that reads a height the environment does
//! not have is refused as a whole ([`ColumnError::MissingEnvironment`]) rather than drawn blank or
//! drawn from a substitute.

use crate::derived::Grid;
use crate::level2::BinnedSweep;
use crate::mrms::MrmsField;
use crate::udp::{evaluate_at_column, Expr, GateInputs, Input};
use crate::udp_volume::{moment_input, MOMENTS};
use chrono::{DateTime, Utc};
use std::fmt;

/// Environmental isotherm heights, metres above sea level. `None` where the source has none.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Levels {
    pub h0_m: Option<f32>,
    pub hm10_m: Option<f32>,
    pub hm20_m: Option<f32>,
}

/// What a column formula can read that is not in the sweeps.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ColumnEnv {
    /// Antenna altitude above sea level, metres (for `BEAM_ALTITUDE_M`).
    pub antenna_altitude_m: Option<f32>,
    pub levels: Levels,
}

impl ColumnEnv {
    fn apply(&self, g: &mut GateInputs) {
        g.freezing_level_m = self.levels.h0_m;
        g.minus10c_height_m = self.levels.hm10_m;
        g.minus20c_height_m = self.levels.hm20_m;
        g.beam_altitude_m = match (self.antenna_altitude_m, g.beam_height_m) {
            (Some(a), Some(h)) => Some(a + h),
            _ => None,
        };
    }

    /// The non-radar inputs `expr` reads that this environment cannot supply.
    pub fn missing_for(&self, expr: &Expr) -> Vec<Input> {
        expr.inputs()
            .into_iter()
            .filter(|input| match input {
                Input::FreezingLevelM => self.levels.h0_m.is_none(),
                Input::Minus10cHeightM => self.levels.hm10_m.is_none(),
                Input::Minus20cHeightM => self.levels.hm20_m.is_none(),
                Input::BeamAltitudeM => self.antenna_altitude_m.is_none(),
                _ => false,
            })
            .collect()
    }
}

/// One distinct tilt's sweeps, in [`MOMENTS`] order, `None` where the tilt has no such moment.
pub type ColumnTilt = [Option<BinnedSweep>; 6];

/// Nested vertical functions re-walk the column once per entry per level of nesting; the map
/// allows one level. The gate inspector, evaluating a single column, has no such limit.
pub const MAX_COLUMN_DEPTH: usize = 1;

/// Ceiling on formula-node evaluations for one grid (cells × levels × nodes), so a pasted formula
/// cannot pin a worker for minutes. A 460 km volume with 20 tilts and a 50-node formula is ~1e9.
pub const MAX_NODE_EVALUATIONS: u64 = 3_000_000_000;

/// Why a column product could not be evaluated. Each is a reason a user can act on.
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnError {
    /// The formula has no vertical/layer function: it is a gate product, drawn per tilt.
    NotColumn,
    /// Vertical functions nested deeper than [`MAX_COLUMN_DEPTH`].
    TooDeep(usize),
    /// More work than [`MAX_NODE_EVALUATIONS`].
    TooExpensive(u64),
    /// Environmental inputs the formula reads that no matched source supplied.
    MissingEnvironment(Vec<Input>),
    /// Radar moments the formula reads that no tilt of this volume carries.
    MissingMoments(Vec<Input>),
    /// No tilt to build a column from.
    NoTilts,
}

impl fmt::Display for ColumnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = |v: &[Input]| v.iter().map(|i| i.name()).collect::<Vec<_>>().join(", ");
        match self {
            Self::NotColumn => write!(f, "not a column formula (it uses no vertical function)"),
            Self::TooDeep(d) => write!(
                f,
                "vertical functions nested {d} deep; the map evaluates at most {MAX_COLUMN_DEPTH}"
            ),
            Self::TooExpensive(n) => write!(f, "too expensive to map ({n} evaluations)"),
            Self::MissingEnvironment(v) => write!(
                f,
                "needs {} for this radar and time, and none is available",
                names(v)
            ),
            Self::MissingMoments(v) => write!(f, "this volume carries no {}", names(v)),
            Self::NoTilts => write!(f, "no tilts to build a column from"),
        }
    }
}

impl std::error::Error for ColumnError {}

/// A column product on the derived-product grid.
#[derive(Clone)]
pub struct ColumnProduct {
    /// The value at each cell, NaN where it has none, in the product's own units.
    pub field: MrmsField,
    /// Per cell, how many tilts' beams pass over it — geometric coverage, whatever the values:
    /// a cell near the radar under the cone of silence, or beyond the upper tilts' range, has
    /// fewer levels than its neighbours, and the inspector says so.
    pub levels_sampled: Vec<u8>,
    /// Tilts contributing to the grid.
    pub tilts: usize,
    /// Cells with a value.
    pub cells_with_value: usize,
}

impl fmt::Debug for ColumnProduct {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ColumnProduct")
            .field("nx", &self.field.nx)
            .field("ny", &self.field.ny)
            .field("tilts", &self.tilts)
            .field("cells_with_value", &self.cells_with_value)
            .finish()
    }
}

impl ColumnProduct {
    /// `(value, levels sampled)` at `(lon, lat)`, by the cell the point falls in.
    pub fn at(&self, lon: f64, lat: f64) -> Option<(Option<f32>, u8)> {
        let f = &self.field;
        if f.nx == 0 || f.ny == 0 {
            return None;
        }
        let fx = (lon - f.lon_west) / (f.lon_east - f.lon_west) * f.nx as f64;
        let fy = (f.lat_north - lat) / (f.lat_north - f.lat_south) * f.ny as f64;
        if !(0.0..f.nx as f64).contains(&fx) || !(0.0..f.ny as f64).contains(&fy) {
            return None;
        }
        let i = fy as usize * f.nx + fx as usize;
        let v = f.values[i];
        Some((v.is_finite().then_some(v), self.levels_sampled[i]))
    }
}

/// Indices into [`MOMENTS`] the formula reads.
fn moment_reads(expr: &Expr) -> Vec<usize> {
    let used = expr.inputs();
    (0..MOMENTS.len())
        .filter(|&i| used.contains(&moment_input(MOMENTS[i])))
        .collect()
}

fn decode(s: &BinnedSweep, code: u8) -> Option<f32> {
    (code >= 2).then(|| s.value_min + (code as f32 - 2.0) / 253.0 * (s.value_max - s.value_min))
}

/// Check that `expr` can be mapped over `tilts` with `env`, before any work is done.
pub fn validate(expr: &Expr, tilts: &[ColumnTilt], env: &ColumnEnv) -> Result<(), ColumnError> {
    if !expr.uses_column() {
        return Err(ColumnError::NotColumn);
    }
    let depth = expr.column_depth();
    if depth > MAX_COLUMN_DEPTH {
        return Err(ColumnError::TooDeep(depth));
    }
    if tilts.iter().all(|t| t.iter().all(Option::is_none)) {
        return Err(ColumnError::NoTilts);
    }
    let missing = env.missing_for(expr);
    if !missing.is_empty() {
        return Err(ColumnError::MissingEnvironment(missing));
    }
    let absent: Vec<Input> = moment_reads(expr)
        .into_iter()
        .filter(|&i| tilts.iter().all(|t| t[i].is_none()))
        .map(|i| moment_input(MOMENTS[i]))
        .collect();
    if !absent.is_empty() {
        return Err(ColumnError::MissingMoments(absent));
    }
    Ok(())
}

/// One column at ground range `ground_km`, azimuth `az`: each tilt whose beam passes over the
/// point, low to high. Returns how many tilts did.
fn sample_column(
    tilts: &[[Option<&BinnedSweep>; 6]],
    reads: &[usize],
    env: &ColumnEnv,
    ground_km: f64,
    az: f64,
    out: &mut Vec<GateInputs>,
) -> u8 {
    out.clear();
    for tilt in tilts {
        // The tilt's geometry: reflectivity's (the longest-range moment) when the tilt has it,
        // else its first moment — the same base `udp_volume` evaluates a gate product on.
        let Some(base) = tilt[0].or_else(|| tilt.iter().flatten().next().copied()) else {
            continue;
        };
        let mut g = GateInputs::default();
        let mut covered = false;
        for &i in reads {
            let Some(s) = tilt[i] else { continue };
            let Some((cell, _)) = crate::xsection::gate_over_ground(s, ground_km, az) else {
                continue;
            };
            covered = true;
            let v = decode(s, s.data[cell]);
            match i {
                0 => g.reflectivity = v,
                1 => g.velocity = v,
                2 => g.spectrum_width = v,
                3 => g.differential_reflectivity = v,
                4 => g.specific_diff_phase = v,
                _ => g.correlation_coefficient = v,
            }
        }
        if reads.is_empty() {
            covered = crate::xsection::gate_over_ground(base, ground_km, az).is_some();
        }
        if !covered {
            continue;
        }
        let elev = base.elevation_deg as f64;
        let slant = crate::xsection::slant_from_ground_km(ground_km, elev);
        g.range_km = Some(ground_km as f32);
        g.azimuth_deg = Some(az as f32);
        g.elevation_deg = Some(base.elevation_deg);
        g.beam_height_m = Some((crate::xsection::beam_height_km(slant, elev) * 1000.0) as f32);
        env.apply(&mut g);
        out.push(g);
    }
    out.sort_by(|a, b| a.beam_height_m.total_cmp_opt(&b.beam_height_m));
    out.len().min(u8::MAX as usize) as u8
}

trait TotalCmpOpt {
    fn total_cmp_opt(&self, other: &Self) -> std::cmp::Ordering;
}

impl TotalCmpOpt for Option<f32> {
    fn total_cmp_opt(&self, other: &Self) -> std::cmp::Ordering {
        self.unwrap_or(f32::NAN)
            .total_cmp(&other.unwrap_or(f32::NAN))
    }
}

fn evaluate_sampled(expr: &Expr, reads: &[usize], column: &[GateInputs]) -> Option<f32> {
    let base = column.first()?;
    if !reads.is_empty() {
        let recorded = column.iter().any(|g| {
            reads.iter().any(|&i| {
                match i {
                    0 => g.reflectivity,
                    1 => g.velocity,
                    2 => g.spectrum_width,
                    3 => g.differential_reflectivity,
                    4 => g.specific_diff_phase,
                    _ => g.correlation_coefficient,
                }
                .is_some()
            })
        });
        if !recorded {
            return None;
        }
    }
    evaluate_at_column(expr, base, column).filter(|v| v.is_finite())
}

/// A column formula's value over one sampled column (low to high by `BEAM_HEIGHT_M`), with the
/// map's rules: the base is the lowest level, and a column with nothing the formula reads
/// recorded has no value. What the gate inspector shows for a column product.
pub fn evaluate_column(expr: &Expr, column: &[GateInputs]) -> Option<f32> {
    let mut sorted = column.to_vec();
    sorted.sort_by(|a, b| a.beam_height_m.total_cmp_opt(&b.beam_height_m));
    evaluate_sampled(expr, &moment_reads(expr), &sorted)
}

/// Evaluate column formula `expr` over every cell of the derived-product grid covering `tilts`
/// (distinct tilts, low to high), stamped with the volume's `time`.
pub fn evaluate_grid(
    expr: &Expr,
    tilts: &[ColumnTilt],
    env: &ColumnEnv,
    time: DateTime<Utc>,
) -> Result<ColumnProduct, ColumnError> {
    validate(expr, tilts, env)?;
    let reads = moment_reads(expr);
    let refs: Vec<[Option<&BinnedSweep>; 6]> = tilts
        .iter()
        .filter(|t| t.iter().any(Option::is_some))
        .map(|t| std::array::from_fn(|i| t[i].as_ref()))
        .collect();
    let geometry: Vec<BinnedSweep> = refs
        .iter()
        .flat_map(|t| t.iter().flatten().map(|s| (*s).clone_header()))
        .collect();
    let grid = Grid::for_sweeps(&geometry).ok_or(ColumnError::NoTilts)?;
    let cells = (grid.nx * grid.ny) as u64;
    let work = cells
        .saturating_mul(refs.len() as u64)
        .saturating_mul(expr.node_count() as u64);
    if work > MAX_NODE_EVALUATIONS {
        return Err(ColumnError::TooExpensive(work));
    }
    let mut values = vec![f32::NAN; grid.nx * grid.ny];
    let mut levels = vec![0u8; grid.nx * grid.ny];
    let row = |gy: usize, vals: &mut [f32], lv: &mut [u8]| {
        let mut column = Vec::with_capacity(refs.len());
        for gx in 0..grid.nx {
            let (ground_km, az) = grid.ground_az(gx, gy);
            lv[gx] = sample_column(&refs, &reads, env, ground_km, az, &mut column);
            if let Some(v) = evaluate_sampled(expr, &reads, &column) {
                vals[gx] = v;
            }
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        values
            .par_chunks_mut(grid.nx)
            .zip(levels.par_chunks_mut(grid.nx))
            .enumerate()
            .for_each(|(gy, (vals, lv))| row(gy, vals, lv));
    }
    #[cfg(target_arch = "wasm32")]
    for (gy, (vals, lv)) in values
        .chunks_mut(grid.nx)
        .zip(levels.chunks_mut(grid.nx))
        .enumerate()
    {
        row(gy, vals, lv);
    }
    let cells_with_value = values.iter().filter(|v| v.is_finite()).count();
    Ok(ColumnProduct {
        field: grid.field(values, time),
        levels_sampled: levels,
        tilts: refs.len(),
        cells_with_value,
    })
}

impl BinnedSweep {
    /// The sweep's geometry without its gates — enough for [`Grid::for_sweeps`].
    fn clone_header(&self) -> BinnedSweep {
        BinnedSweep {
            moment: self.moment,
            az_bins: self.az_bins,
            gate_count: self.gate_count,
            first_gate_km: self.first_gate_km,
            gate_interval_km: self.gate_interval_km,
            radar_lat: self.radar_lat,
            radar_lon: self.radar_lon,
            elevation_deg: self.elevation_deg,
            value_min: self.value_min,
            value_max: self.value_max,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level2::Moment;

    const LAT: f32 = 35.333;
    const LON: f32 = -97.278;

    /// A synthetic tilt: `value(az_bin, gate)` in physical units, 360 bins of 1 km gates.
    fn sweep(
        moment: Moment,
        elev: f32,
        gates: usize,
        value: impl Fn(usize, usize) -> Option<f32>,
    ) -> BinnedSweep {
        let az_bins = 360;
        let (value_min, value_max) = moment.value_range();
        let mut data = vec![0u8; az_bins * gates];
        for b in 0..az_bins {
            for g in 0..gates {
                if let Some(v) = value(b, g) {
                    let t = ((v - value_min) / (value_max - value_min)).clamp(0.0, 1.0);
                    data[b * gates + g] = 2 + (t * 253.0).round() as u8;
                }
            }
        }
        BinnedSweep {
            moment,
            az_bins,
            gate_count: gates,
            data,
            first_gate_km: 0.5,
            gate_interval_km: 1.0,
            radar_lat: LAT,
            radar_lon: LON,
            elevation_deg: elev,
            value_min,
            value_max,
            ..Default::default()
        }
    }

    /// A storm-ish volume: reflectivity rising toward a core at azimuth 40-60, gate 30-60, that
    /// weakens with height; ZDR high below 2 km, a ZDR column above; CC dipping in the core.
    fn volume() -> Vec<ColumnTilt> {
        [0.5f32, 1.5, 3.0, 6.0, 10.0]
            .iter()
            .enumerate()
            .map(|(k, &e)| {
                let gates = 120 - 15 * k; // upper tilts are shorter, as in a real VCP
                let core = move |b: usize, g: usize| (40..60).contains(&b) && (30..60).contains(&g);
                let refl = sweep(Moment::Reflectivity, e, gates, move |b, g| {
                    if core(b, g) {
                        Some(62.0 - 5.0 * k as f32 + (g % 7) as f32)
                    } else if (b + g + k) % 5 == 0 {
                        Some(18.0 + (b % 11) as f32)
                    } else {
                        None
                    }
                });
                let zdr = sweep(Moment::DifferentialReflectivity, e, gates, move |b, g| {
                    core(b, g).then_some(0.5 + k as f32 * 0.7 + (b % 3) as f32 * 0.3)
                });
                let cc = sweep(Moment::CorrelationCoefficient, e, gates, move |b, g| {
                    (core(b, g) || (b + g) % 3 == 0).then_some(
                        0.99 - if core(b, g) {
                            0.04 * (k + 1) as f32
                        } else {
                            0.0
                        },
                    )
                });
                [Some(refl), None, None, Some(zdr), None, Some(cc)]
            })
            .collect()
    }

    fn grid_of(src: &str, env: &ColumnEnv) -> Result<ColumnProduct, ColumnError> {
        evaluate_grid(&crate::udp::parse(src).unwrap(), &volume(), env, Utc::now())
    }

    fn same(a: f32, b: f32) -> bool {
        (a.is_nan() && b.is_nan()) || a == b
    }

    #[test]
    fn column_max_reflectivity_is_the_local_composite_to_the_bit() {
        let product = grid_of("max_vertical(REF)", &ColumnEnv::default()).unwrap();
        let refl: Vec<BinnedSweep> = volume().into_iter().filter_map(|t| t[0].clone()).collect();
        let derived = crate::derived::derive(
            &refl,
            &crate::derived::DerivedOpts {
                time: product.field.time,
                ..Default::default()
            },
        )
        .unwrap();
        let c = &derived.composite;
        let f = &product.field;
        assert_eq!((f.nx, f.ny), (c.nx, c.ny));
        assert_eq!((f.lon_west, f.lat_north), (c.lon_west, c.lat_north));
        let differ = f
            .values
            .iter()
            .zip(&c.values)
            .filter(|(a, b)| !same(**a, **b))
            .count();
        assert_eq!(differ, 0, "{differ} cells differ from the composite");
        assert!(product.cells_with_value > 1000);
        assert_eq!(product.tilts, 5);
    }

    /// Nearest gate over a ground point, written out independently of `gate_over_ground`.
    fn brute(s: &BinnedSweep, ground_km: f64, az: f64) -> Option<(f32, f64)> {
        let e = s.elevation_deg as f64;
        // Search the gate whose ground range is nearest rather than inverting the geometry.
        let (mut best, mut best_d) = (None, f64::MAX);
        for g in 0..s.gate_count {
            let slant = s.first_gate_km as f64 + g as f64 * s.gate_interval_km as f64;
            let d = (crate::xsection::ground_from_slant_km(slant, e) - ground_km).abs();
            if d < best_d {
                (best, best_d) = (Some((g, slant)), d);
            }
        }
        let (g, _) = best?;
        if best_d > s.gate_interval_km as f64 {
            return None; // past the end of the sweep
        }
        // Heights are the beam's over the point itself, not at the sampled gate's centre.
        let slant = crate::xsection::slant_from_ground_km(ground_km, e);
        let bin = (az / 360.0 * s.az_bins as f64).floor() as usize % s.az_bins;
        let code = s.data[bin * s.gate_count + g];
        (code >= 2).then(|| {
            let v = s.value_min + (code as f32 - 2.0) / 253.0 * (s.value_max - s.value_min);
            (v, crate::xsection::beam_height_km(slant, e) * 1000.0)
        })
    }

    /// Cells in and around the core, where every reduction has something to work with.
    fn probe_cells(p: &ColumnProduct) -> Vec<(usize, f64, f64)> {
        let g = Grid::for_sweeps(
            &volume()
                .iter()
                .filter_map(|t| t[0].clone())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let mut out = Vec::new();
        for gy in (0..g.ny).step_by(7) {
            for gx in (0..g.nx).step_by(5) {
                let (r, az) = g.ground_az(gx, gy);
                if (35.0..70.0).contains(&az) && (25.0..65.0).contains(&r) {
                    out.push((gy * p.field.nx + gx, r, az));
                }
            }
        }
        assert!(out.len() > 20, "{} probe cells", out.len());
        out
    }

    #[test]
    fn masked_cc_minimum_matches_an_independent_column_calculation() {
        let p = grid_of("min_vertical(CC, REF >= 40)", &ColumnEnv::default()).unwrap();
        let vol = volume();
        let mut checked = 0;
        for (i, r, az) in probe_cells(&p) {
            let mut expect: Option<f32> = None;
            for t in &vol {
                let (Some(refl), Some(cc)) = (&t[0], &t[5]) else {
                    continue;
                };
                let (Some((z, _)), Some((c, _))) = (brute(refl, r, az), brute(cc, r, az)) else {
                    continue;
                };
                if z >= 40.0 {
                    expect = Some(expect.map_or(c, |e: f32| e.min(c)));
                }
            }
            let got = p.field.values[i];
            match expect {
                Some(e) => {
                    assert!((got - e).abs() < 1e-6, "cell {i}: {got} vs {e}");
                    checked += 1;
                }
                None => assert!(got.is_nan(), "cell {i}: {got} with no qualifying level"),
            }
        }
        assert!(checked > 10, "only {checked} cells had a qualifying level");
    }

    #[test]
    fn zdr_above_the_freezing_level_uses_the_environment_and_refuses_without_it() {
        let src = "max_vertical(ZDR, BEAM_ALTITUDE_M > FREEZING_LEVEL_M)";
        assert_eq!(
            grid_of(src, &ColumnEnv::default()).unwrap_err(),
            ColumnError::MissingEnvironment(vec![Input::BeamAltitudeM, Input::FreezingLevelM])
        );
        let env = ColumnEnv {
            antenna_altitude_m: Some(370.0),
            levels: Levels {
                h0_m: Some(3_400.0),
                ..Default::default()
            },
        };
        let p = grid_of(src, &env).unwrap();
        let vol = volume();
        let mut above = 0;
        for (i, r, az) in probe_cells(&p) {
            let mut expect: Option<f32> = None;
            for t in &vol {
                let (Some(refl), Some(zdr)) = (&t[0], &t[3]) else {
                    continue;
                };
                // Geometry from the reflectivity tilt, as the map takes it.
                let Some((_, h)) = brute(refl, r, az).or_else(|| {
                    let e = refl.elevation_deg as f64;
                    let s = crate::xsection::slant_from_ground_km(r, e);
                    Some((0.0, crate::xsection::beam_height_km(s, e) * 1000.0))
                }) else {
                    continue;
                };
                if let Some((v, _)) = brute(zdr, r, az) {
                    if 370.0 + h as f32 > 3_400.0 {
                        expect = Some(expect.map_or(v, |e: f32| e.max(v)));
                    }
                }
            }
            let got = p.field.values[i];
            match expect {
                Some(e) => {
                    assert!((got - e).abs() < 1e-5, "cell {i}: {got} vs {e}");
                    above += 1;
                }
                None => assert!(got.is_nan(), "cell {i}: {got} with nothing above 0 °C"),
            }
        }
        assert!(
            above > 5,
            "only {above} cells had ZDR above the melting level"
        );
        // Lower the melting level and more of the column qualifies, never less.
        let lower = ColumnEnv {
            levels: Levels {
                h0_m: Some(1_000.0),
                ..Default::default()
            },
            ..env
        };
        let q = grid_of(src, &lower).unwrap();
        assert!(q.cells_with_value >= p.cells_with_value);
    }

    #[test]
    fn a_minus_10c_formula_needs_its_own_level_not_a_neighbours() {
        let env = ColumnEnv {
            antenna_altitude_m: Some(370.0),
            levels: Levels {
                h0_m: Some(3_400.0),
                hm10_m: None,
                hm20_m: Some(6_600.0),
            },
        };
        assert_eq!(
            grid_of(
                "max_vertical(REF, BEAM_ALTITUDE_M > MINUS10C_HEIGHT_M)",
                &env
            )
            .unwrap_err(),
            ColumnError::MissingEnvironment(vec![Input::Minus10cHeightM])
        );
    }

    #[test]
    fn heights_and_counts_come_from_the_sampled_beams() {
        let p = grid_of("first_height_above(REF, 50)", &ColumnEnv::default()).unwrap();
        let n = grid_of("count_above(REF, 50)", &ColumnEnv::default()).unwrap();
        let vol = volume();
        for (i, r, az) in probe_cells(&p) {
            let mut hits: Vec<f64> = vol
                .iter()
                .filter_map(|t| brute(t[0].as_ref()?, r, az))
                .filter(|(v, _)| *v >= 50.0)
                .map(|(_, h)| h)
                .collect();
            hits.sort_by(f64::total_cmp);
            match hits.first() {
                Some(h) => assert!((p.field.values[i] as f64 - h).abs() < 0.5, "cell {i}"),
                None => assert!(p.field.values[i].is_nan()),
            }
            let count = n.field.values[i];
            assert!(
                (count.is_nan() && hits.is_empty()) || count == hits.len() as f32,
                "cell {i}: count {count} vs {}",
                hits.len()
            );
        }
    }

    #[test]
    fn clear_air_has_no_value_and_coverage_counts_the_beams_overhead() {
        let p = grid_of("count_above(REF, 50)", &ColumnEnv::default()).unwrap();
        let g = &p.field;
        // The far corner of the grid is outside every tilt: no beams, no value.
        assert_eq!(p.levels_sampled[0], 0);
        assert!(g.values[0].is_nan());
        // Near the radar every tilt reaches; far out only the long low tilts do.
        let center = (g.ny / 2) * g.nx + g.nx / 2 + 3;
        assert_eq!(p.levels_sampled[center], 5);
        let edge = (g.ny / 2) * g.nx + g.nx - 2;
        assert!(p.levels_sampled[edge] < 5);
        let (v, levels) = p.at(LON as f64, LAT as f64 + 0.03).unwrap();
        assert_eq!(levels, 5);
        let _ = v;
    }

    #[test]
    fn gate_formulas_deep_nesting_and_absent_moments_are_refused_with_a_reason() {
        let env = ColumnEnv::default();
        assert_eq!(
            grid_of("REF + 1", &env).unwrap_err(),
            ColumnError::NotColumn
        );
        assert_eq!(
            grid_of("max_vertical(REF, REF > max_vertical(REF) - 5)", &env).unwrap_err(),
            ColumnError::TooDeep(2)
        );
        assert_eq!(
            grid_of("max_vertical(KDP)", &env).unwrap_err(),
            ColumnError::MissingMoments(vec![Input::SpecificDifferentialPhase])
        );
        let err = grid_of("max_vertical(KDP)", &env).unwrap_err().to_string();
        assert!(err.contains("KDP"), "{err}");
    }

    #[test]
    fn the_inspector_column_and_the_map_cell_agree() {
        let expr = crate::udp::parse("max_layer(REF, 0, 3000) - REF").unwrap();
        let vol = volume();
        let p = evaluate_grid(&expr, &vol, &ColumnEnv::default(), Utc::now()).unwrap();
        let refs: Vec<[Option<&BinnedSweep>; 6]> = vol
            .iter()
            .map(|t| std::array::from_fn(|i| t[i].as_ref()))
            .collect();
        for (i, r, az) in probe_cells(&p) {
            let mut column = Vec::new();
            sample_column(&refs, &[0], &ColumnEnv::default(), r, az, &mut column);
            // Shuffled, as a caller might hand it over: the result does not depend on order.
            column.reverse();
            let probe = evaluate_column(&expr, &column);
            let map = p.field.values[i];
            assert!(
                probe.map_or(map.is_nan(), |v| v == map),
                "cell {i}: inspector {probe:?} vs map {map}"
            );
        }
    }
}
