//! A user-defined product ([`crate::udp`]) evaluated at every gate of a volume, as sweeps the 3D
//! volume builder can resample like any moment (ROADMAP_NEW H1: GR2Analyst draws user-defined
//! products in 3D too).
//!
//! Each tilt is evaluated on the grid of one of its own moments (reflectivity when the formula
//! reads it, else the first moment it reads), with every other moment looked up at the same
//! azimuth and slant range. The values are then quantized over one range for the whole volume —
//! the product's own `min`/`max` when it gives them, else the 2nd to 98th percentile of what it
//! produced — into the same 2..=255 codes a binned moment uses.
//!
//! Formulas using a vertical/layer function have no value at a single gate (they reduce a whole
//! column to one number), so they produce nothing here.

use crate::level2::{BinnedSweep, Moment};
use crate::udp::{evaluate, Expr, GateInputs, Input};

/// The moments a formula can read, in the order [`evaluate_tilt`] takes them.
pub const MOMENTS: [Moment; 6] = [
    Moment::Reflectivity,
    Moment::Velocity,
    Moment::SpectrumWidth,
    Moment::DifferentialReflectivity,
    Moment::SpecificDifferentialPhase,
    Moment::CorrelationCoefficient,
];

fn moment_input(m: Moment) -> Input {
    match m {
        Moment::Velocity => Input::Velocity,
        Moment::SpectrumWidth => Input::SpectrumWidth,
        Moment::DifferentialReflectivity => Input::DifferentialReflectivity,
        Moment::SpecificDifferentialPhase => Input::SpecificDifferentialPhase,
        Moment::CorrelationCoefficient => Input::CorrelationCoefficient,
        _ => Input::Reflectivity,
    }
}

/// What a formula can read that is not in the sweeps.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Env {
    /// Antenna altitude above sea level, metres (for `BEAM_ALTITUDE_M`).
    pub antenna_altitude_m: Option<f32>,
    /// (0 °C, −20 °C) heights above sea level, metres.
    pub freezing: Option<(f32, f32)>,
}

fn decode(s: &BinnedSweep, code: u8) -> Option<f32> {
    (code >= 2).then(|| {
        let t = (code - 2) as f32 / 253.0;
        s.value_min + t * (s.value_max - s.value_min)
    })
}

/// The value `s` recorded at azimuth `az_deg` and slant range `slant_km`.
fn lookup(s: &BinnedSweep, az_deg: f32, slant_km: f32) -> Option<f32> {
    if s.az_bins == 0 || s.gate_count == 0 {
        return None;
    }
    let g = (slant_km - s.first_gate_km) / s.gate_interval_km.max(1e-6);
    if g < 0.0 || g >= s.gate_count as f32 {
        return None;
    }
    let bin = ((az_deg.rem_euclid(360.0) / 360.0 * s.az_bins as f32) as usize) % s.az_bins;
    decode(s, *s.data.get(bin * s.gate_count + g as usize)?)
}

/// `expr` at every gate of one tilt. `tilt` holds the tilt's sweeps in [`MOMENTS`] order, `None`
/// where the tilt has no such moment. Returns the grid it evaluated on (a sweep whose data is
/// still that moment's, to be replaced by [`quantize`]) and one value per gate, or `None` when
/// the tilt has none of the moments the formula reads.
pub fn evaluate_tilt(
    expr: &Expr,
    tilt: &[Option<&BinnedSweep>; 6],
    env: Env,
) -> Option<(BinnedSweep, Vec<Option<f32>>)> {
    let used = expr.inputs();
    let reads: Vec<usize> = (0..6)
        .filter(|&i| used.contains(&moment_input(MOMENTS[i])))
        .collect();
    // The grid: reflectivity when read (the finest, longest-range moment), else the first moment
    // read, else (a geometry-only formula) whatever the tilt has.
    let base_i = if reads.contains(&0) && tilt[0].is_some() {
        0
    } else {
        reads
            .iter()
            .copied()
            .find(|&i| tilt[i].is_some())
            .or_else(|| (0..6).find(|&i| tilt[i].is_some()))?
    };
    if !reads.is_empty() && !reads.iter().any(|&i| tilt[i].is_some()) {
        return None;
    }
    let base = tilt[base_i]?;
    let elev = base.elevation_deg;
    let mut out = vec![None; base.az_bins * base.gate_count];
    for bin in 0..base.az_bins {
        let az = (bin as f32 + 0.5) * 360.0 / base.az_bins as f32;
        for gate in 0..base.gate_count {
            let slant = base.first_gate_km + (gate as f32 + 0.5) * base.gate_interval_km;
            let mut values = [None; 6];
            for &i in &reads {
                values[i] = if i == base_i {
                    decode(base, base.data[bin * base.gate_count + gate])
                } else {
                    tilt[i].and_then(|s| lookup(s, az, slant))
                };
            }
            // Nothing the formula reads was recorded here: no value, and no evaluation. Most of
            // a volume is clear air, so this is most of the work skipped.
            if !reads.is_empty() && values.iter().all(Option::is_none) {
                continue;
            }
            let height_m =
                crate::xsection::beam_height_km(slant as f64, elev as f64) as f32 * 1000.0;
            let inputs = GateInputs {
                reflectivity: values[0],
                velocity: values[1],
                spectrum_width: values[2],
                differential_reflectivity: values[3],
                specific_diff_phase: values[4],
                correlation_coefficient: values[5],
                range_km: Some(
                    crate::xsection::ground_from_slant_km(slant as f64, elev as f64) as f32,
                ),
                azimuth_deg: Some(az),
                elevation_deg: Some(elev),
                beam_height_m: Some(height_m),
                beam_altitude_m: env.antenna_altitude_m.map(|a| a + height_m),
                freezing_level_m: env.freezing.map(|f| f.0),
                minus20c_height_m: env.freezing.map(|f| f.1),
            };
            out[bin * base.gate_count + gate] = evaluate(expr, &inputs).filter(|v| v.is_finite());
        }
    }
    Some((base.clone(), out))
}

/// The range a product's values are drawn over: `range` when given, else the 2nd to 98th
/// percentile of `values` (a few wild gates should not wash out the palette). `None` with no
/// values at all.
pub fn auto_range<'a>(
    values: impl Iterator<Item = &'a Option<f32>>,
    range: Option<(f32, f32)>,
) -> Option<(f32, f32)> {
    if let Some((lo, hi)) = range.filter(|(lo, hi)| hi > lo) {
        return Some((lo, hi));
    }
    let mut v: Vec<f32> = values.flatten().copied().collect();
    if v.is_empty() {
        return None;
    }
    // Enough for a percentile; a whole volume can be millions of gates.
    if v.len() > 400_000 {
        let step = v.len() / 400_000 + 1;
        v = v.into_iter().step_by(step).collect();
    }
    v.sort_by(f32::total_cmp);
    let at = |q: f32| v[((v.len() - 1) as f32 * q).round() as usize];
    let (lo, hi) = (at(0.02), at(0.98));
    Some(if hi - lo < 1e-3 {
        (lo - 1.0, hi + 1.0)
    } else {
        (lo, hi)
    })
}

/// Quantize evaluated tilts over one range for the whole volume into binned sweeps (codes
/// 2..=255, 0 where there is no value), and say the range used.
pub fn quantize(
    tilts: Vec<(BinnedSweep, Vec<Option<f32>>)>,
    range: Option<(f32, f32)>,
) -> Option<(Vec<BinnedSweep>, (f32, f32))> {
    let (lo, hi) = auto_range(tilts.iter().flat_map(|(_, v)| v.iter()), range)?;
    let sweeps = tilts
        .into_iter()
        .map(|(mut s, values)| {
            s.data = values
                .iter()
                .map(|v| match v {
                    Some(v) => {
                        let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
                        2 + (t * 253.0).round() as u8
                    }
                    None => 0,
                })
                .collect();
            s.value_min = lo;
            s.value_max = hi;
            s.nyquist_ms = 0.0;
            s
        })
        .collect();
    Some((sweeps, (lo, hi)))
}

/// Pair a volume's per-tilt moments for [`evaluate_tilt`]: tilt `t`'s own sweep of each moment,
/// else one from another tilt within 0.25° of it (a split cut records reflectivity and velocity
/// on separate rotations at the same elevation).
pub fn pair_tilts(per_moment: &[Vec<Option<BinnedSweep>>; 6]) -> Vec<[Option<&BinnedSweep>; 6]> {
    let n = per_moment.iter().map(Vec::len).max().unwrap_or(0);
    let elev = |t: usize| {
        per_moment
            .iter()
            .find_map(|m| m.get(t).and_then(|s| s.as_ref()).map(|s| s.elevation_deg))
    };
    (0..n)
        .filter_map(|t| {
            let e = elev(t)?;
            Some(std::array::from_fn(|i| {
                let m = &per_moment[i];
                m.get(t).and_then(|s| s.as_ref()).or_else(|| {
                    m.iter()
                        .flatten()
                        .filter(|s| (s.elevation_deg - e).abs() < 0.25)
                        .min_by(|a, b| {
                            (a.elevation_deg - e)
                                .abs()
                                .total_cmp(&(b.elevation_deg - e).abs())
                        })
                })
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sweep(moment: Moment, value: impl Fn(usize, usize) -> Option<f32>) -> BinnedSweep {
        let (az_bins, gate_count) = (36, 100);
        let (value_min, value_max) = moment.value_range();
        let mut data = vec![0u8; az_bins * gate_count];
        for b in 0..az_bins {
            for g in 0..gate_count {
                if let Some(v) = value(b, g) {
                    let t = ((v - value_min) / (value_max - value_min)).clamp(0.0, 1.0);
                    data[b * gate_count + g] = 2 + (t * 253.0).round() as u8;
                }
            }
        }
        BinnedSweep {
            moment,
            az_bins,
            gate_count,
            data,
            first_gate_km: 0.0,
            gate_interval_km: 1.0,
            elevation_deg: 0.5,
            value_min,
            value_max,
            ..Default::default()
        }
    }

    #[test]
    fn a_product_is_evaluated_where_its_inputs_are_and_nowhere_else() {
        // Echo in the first 10 azimuth bins only; ZDR 2 dB everywhere there is echo.
        let refl = sweep(Moment::Reflectivity, |b, _| (b < 10).then_some(50.0));
        let zdr = sweep(Moment::DifferentialReflectivity, |b, _| {
            (b < 10).then_some(2.0)
        });
        let expr = crate::udp::parse("REF > 45 && ZDR > 1 ? REF - 10 * ZDR : 0").unwrap();
        let tilt = [Some(&refl), None, None, Some(&zdr), None, None];
        let (grid, values) = evaluate_tilt(&expr, &tilt, Env::default()).unwrap();
        assert_eq!(values.len(), grid.az_bins * grid.gate_count);
        let v = values[5 * 100 + 50].unwrap();
        assert!((v - 30.0).abs() < 0.6, "50 - 10*2 = 30, got {v}");
        assert!(values[20 * 100 + 50].is_none(), "clear air has no value");
    }

    #[test]
    fn quantizing_keeps_the_values_within_a_code() {
        let refl = sweep(Moment::Reflectivity, |_, g| Some(g as f32 * 0.5));
        let expr = crate::udp::parse("REF * 2").unwrap();
        let tilt = [Some(&refl), None, None, None, None, None];
        let t = evaluate_tilt(&expr, &tilt, Env::default()).unwrap();
        let (sweeps, (lo, hi)) = quantize(vec![t], Some((0.0, 100.0))).unwrap();
        assert_eq!((lo, hi), (0.0, 100.0));
        let s = decode(&sweeps[0], sweeps[0].data[3 * 100 + 40]).unwrap();
        assert!((s - 40.0).abs() < 0.8, "REF 20 doubled is 40, got {s}");
    }

    #[test]
    fn an_auto_range_ignores_a_few_wild_gates() {
        let mut v: Vec<Option<f32>> = (0..1000).map(|i| Some(i as f32 / 100.0)).collect();
        v.push(Some(1.0e6));
        let (lo, hi) = auto_range(v.iter(), None).unwrap();
        assert!(lo < 0.5 && hi > 9.0 && hi < 11.0, "{lo}..{hi}");
        assert_eq!(auto_range([None::<f32>].iter(), None), None);
    }

    #[test]
    fn a_split_cut_lends_its_velocity_to_the_reflectivity_tilt() {
        let r = sweep(Moment::Reflectivity, |_, _| Some(30.0));
        let mut v = sweep(Moment::Velocity, |_, _| Some(10.0));
        v.elevation_deg = 0.52;
        let per: [Vec<Option<BinnedSweep>>; 6] = [
            vec![Some(r.clone()), None],
            vec![None, Some(v)],
            vec![],
            vec![],
            vec![],
            vec![],
        ];
        let tilts = pair_tilts(&per);
        assert_eq!(tilts.len(), 2);
        assert!(
            tilts[0][0].is_some() && tilts[0][1].is_some(),
            "velocity borrowed"
        );
    }

    #[test]
    fn a_column_formula_is_recognised() {
        assert!(crate::udp::parse("max_vertical(REF)")
            .unwrap()
            .uses_column());
        assert!(!crate::udp::parse("REF + ZDR").unwrap().uses_column());
    }
}
