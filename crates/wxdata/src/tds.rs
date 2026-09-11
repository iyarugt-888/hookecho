//! Tornado Debris Signature (TDS) detection.
//!
//! A TDS is a "debris ball": lofted tornado debris scatters radar energy incoherently, dropping the
//! correlation coefficient (CC/ρhv) well below meteorological values while reflectivity stays high.
//! The classic operational heuristic — low CC collocated with high reflectivity in the storm core —
//! is what this module flags. (Collocation with a velocity couplet / rotation strengthens a real
//! diagnosis; that's left to the human reading the flagged location.)
//!
//! Input is two same-tilt [`BinnedSweep`]s (reflectivity + CC) sharing the 720-azimuth grid.
//! Candidate gates are clustered on a coarse geographic grid so a debris ball reports as one hit.
//!
//! [`detect`] reads one tilt at a time and has no way to tell a genuine, lofted debris ball from a
//! one-tilt patch of biological scatter or clutter that happens to clear the CC/Z thresholds.
//! [`detect_volume`] is the same detector run across several tilts, with each cell's confidence
//! raised by how many of them show a hit and how high the highest one reaches — the vertical
//! continuity a single sweep cannot offer at all.

use crate::level2::{BinnedSweep, Moment};

/// A detected debris-signature cluster.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TdsHit {
    pub lon: f64,
    pub lat: f64,
    /// Number of candidate gates in the cluster (bigger = more confident).
    pub gates: usize,
    /// Lowest CC seen in the cluster.
    pub min_cc: f32,
    /// Range from the radar to the cluster centroid (km).
    pub range_km: f32,
    /// How many of the volume's tilts show a hit at this location. Always 1 out of [`detect`],
    /// which only ever sees one tilt; [`detect_volume`] fills this in for real.
    pub tilts: usize,
    /// Beam-center height (km AGL) of the highest tilt contributing to this hit — how far the
    /// debris is lofted above the radar's own elevation. Ground clutter and biological scatter
    /// are confined to whichever one tilt sees them and do not repeat one elevation angle up; a
    /// genuine tornado debris ball often does, which single-tilt CC/Z collocation alone cannot
    /// tell apart.
    pub top_km: f32,
    /// 0..1 confidence. `detect()`'s single-tilt read has no vertical evidence at all and caps out
    /// at 0.5 on gate count alone; [`detect_volume`] can go higher once a hit repeats up through
    /// the tilts.
    pub confidence: f32,
}

/// Decode a binned `u8` gate index back to its physical value, or `None` for below-threshold /
/// range-folded gates (indices 0/1).
pub(crate) fn decode(sweep: &BinnedSweep, idx: u8) -> Option<f32> {
    if idx < 2 {
        return None;
    }
    let (lo, hi) = (sweep.value_min, sweep.value_max);
    Some(lo + (idx as f32 - 2.0) / 253.0 * (hi - lo))
}

/// Great-circle destination point (used to place a gate at its azimuth/range).
pub(crate) fn dest(lon: f64, lat: f64, bearing_deg: f64, dist_km: f64) -> (f64, f64) {
    let r = 6371.0;
    let ad = dist_km / r;
    let (br, la1, lo1) = (bearing_deg.to_radians(), lat.to_radians(), lon.to_radians());
    let la2 = (la1.sin() * ad.cos() + la1.cos() * ad.sin() * br.cos()).asin();
    let lo2 = lo1 + (br.sin() * ad.sin() * la1.cos()).atan2(ad.cos() - la1.sin() * la2.sin());
    (lo2.to_degrees(), la2.to_degrees())
}

/// Detect debris-signature clusters. A gate is a candidate when CC `< cc_max`, reflectivity
/// `>= z_min` dBZ, and its range `<= max_range_km` (resolution/low-tilt gating). Candidates are
/// clustered on a ~4 km grid; clusters with `>= min_gates` gates become hits.
pub fn detect(
    z: &BinnedSweep,
    cc: &BinnedSweep,
    cc_max: f32,
    z_min: f32,
    max_range_km: f32,
    min_gates: usize,
) -> Vec<TdsHit> {
    debug_assert_eq!(cc.moment, Moment::CorrelationCoefficient);
    if cc.az_bins == 0 || cc.gate_count == 0 || z.gate_count == 0 {
        return Vec::new();
    }
    // Accumulate candidates into ~0.04° (~4 km) geographic cells.
    const CELL: f64 = 0.04;
    use std::collections::HashMap;
    // gates, sum_lon, sum_lat, sum_range, min_cc
    let mut cells: HashMap<(i64, i64), (usize, f64, f64, f64, f32)> = HashMap::new();
    let (rlon, rlat) = (cc.radar_lon as f64, cc.radar_lat as f64);

    for az in 0..cc.az_bins {
        let az_deg = az as f64 * 360.0 / cc.az_bins as f64;
        for gate in 0..cc.gate_count {
            let range = cc.first_gate_km + gate as f32 * cc.gate_interval_km;
            if range > max_range_km {
                break; // gates increase with index; nothing further qualifies on this radial
            }
            let Some(cc_val) = decode(cc, cc.data[az * cc.gate_count + gate]) else {
                continue;
            };
            if cc_val >= cc_max {
                continue;
            }
            // Reflectivity at the same range (map through the Z sweep's own gate spacing).
            let zi = ((range - z.first_gate_km) / z.gate_interval_km).round() as i64;
            if zi < 0 || zi as usize >= z.gate_count {
                continue;
            }
            let Some(z_val) = decode(z, z.data[az * z.gate_count + zi as usize]) else {
                continue;
            };
            if z_val < z_min {
                continue; // low CC in weak echo = biological/clutter, not debris
            }
            let (lon, lat) = dest(rlon, rlat, az_deg, range as f64);
            let key = ((lon / CELL).round() as i64, (lat / CELL).round() as i64);
            let e = cells.entry(key).or_insert((0, 0.0, 0.0, 0.0, 1.05));
            e.0 += 1;
            e.1 += lon;
            e.2 += lat;
            e.3 += range as f64;
            e.4 = e.4.min(cc_val);
        }
    }

    let elev = cc.elevation_deg as f64;
    let mut hits: Vec<TdsHit> = cells
        .into_values()
        .filter(|(n, ..)| *n >= min_gates)
        .map(|(n, slon, slat, srange, min_cc)| {
            let range_km = (srange / n as f64) as f32;
            let top_km = crate::xsection::beam_height_km(range_km as f64, elev) as f32;
            // No vertical evidence at all from one tilt — gate count is the only signal, so this
            // never claims more than half confidence. `detect_volume` is what can push it higher.
            let confidence = (n as f32 / (min_gates.max(1) as f32 * 4.0)).clamp(0.05, 0.5);
            TdsHit {
                lon: slon / n as f64,
                lat: slat / n as f64,
                gates: n,
                min_cc,
                range_km,
                tilts: 1,
                top_km,
                confidence,
            }
        })
        .collect();
    hits.sort_by_key(|h| std::cmp::Reverse(h.gates));
    hits
}

/// Detect debris signatures across a volume's tilts, not just one — the vertical-continuity check
/// a single sweep cannot offer. Per-tilt hits (each already computed by [`detect`], so an existing
/// single-tilt caller sees no change) that land in the same ~4 km cell as another tilt's are
/// merged into one [`TdsHit`] with the combined gate count, the deepest reach, and a
/// correspondingly higher confidence. `sweeps` need not be given lowest-first — height comes from
/// each pair's own `elevation_deg`, not its position in the slice.
///
/// A hit seen at only one tilt is still returned (debris does sometimes stay shallow), just
/// without the confidence boost a taller column earns.
pub fn detect_volume(
    sweeps: &[(BinnedSweep, BinnedSweep)],
    cc_max: f32,
    z_min: f32,
    max_range_km: f32,
    min_gates: usize,
) -> Vec<TdsHit> {
    const CELL: f64 = 0.04; // same grid `detect` clusters on
    use std::collections::HashMap;
    // gates, sum_lon*w, sum_lat*w, sum_range*w, min_cc, tilts, top_km
    let mut cells: HashMap<(i64, i64), (usize, f64, f64, f64, f32, usize, f32)> = HashMap::new();

    for (z, cc) in sweeps {
        for h in detect(z, cc, cc_max, z_min, max_range_km, min_gates) {
            let key = ((h.lon / CELL).round() as i64, (h.lat / CELL).round() as i64);
            let w = h.gates as f64;
            let e = cells
                .entry(key)
                .or_insert((0, 0.0, 0.0, 0.0, 1.05, 0, 0.0));
            e.0 += h.gates;
            e.1 += h.lon * w;
            e.2 += h.lat * w;
            e.3 += h.range_km as f64 * w;
            e.4 = e.4.min(h.min_cc);
            e.5 += 1;
            e.6 = e.6.max(h.top_km);
        }
    }

    let mut out: Vec<TdsHit> = cells
        .into_values()
        .map(|(gates, slon, slat, srange, min_cc, tilts, top_km)| {
            let w = gates.max(1) as f64;
            // Vertical extent matters more than raw area: a debris ball that repeats through
            // several tilts is a real 3D column, while a wide but single-tilt patch is exactly
            // the shape ground clutter or biological scatter makes. ~3 km AGL saturates the
            // height term — well above where any of this app's other tilt-count/range choices
            // expect a low-level debris signature to still be found.
            let height_term = (top_km / 3.0).clamp(0.0, 1.0);
            // Absolute, not "what fraction of the tilts a caller happened to check" — a caller
            // that only ever looks at the lowest tilt must not make a lone hit read as the whole
            // column just because it's 1 out of the 1 it checked. Saturates at 3 tilts: a debris
            // signature repeating through the lowest three or four elevation angles is about as
            // convincing as this heuristic gets.
            let depth_term = ((tilts as f32 - 1.0) / 2.0).clamp(0.0, 1.0);
            let confidence = (0.5 * height_term + 0.5 * depth_term).clamp(0.0, 1.0);
            TdsHit {
                lon: slon / w,
                lat: slat / w,
                gates,
                min_cc,
                range_km: (srange / w) as f32,
                tilts,
                top_km,
                confidence,
            }
        })
        .collect();
    out.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a sweep where a wedge of azimuths/gates carries `hot` (index) and the rest `cold`.
    fn sweep(
        moment: Moment,
        hot: u8,
        cold: u8,
        hot_az: std::ops::Range<usize>,
        hot_gate: std::ops::Range<usize>,
    ) -> BinnedSweep {
        let (az_bins, gate_count) = (720usize, 200usize);
        let mut data = vec![cold; az_bins * gate_count];
        for az in hot_az.clone() {
            for g in hot_gate.clone() {
                data[az * gate_count + g] = hot;
            }
        }
        let (lo, hi) = moment.value_range();
        BinnedSweep {
            moment,
            az_bins,
            gate_count,
            data,
            first_gate_km: 2.0,
            gate_interval_km: 0.25,
            radar_lat: 35.0,
            radar_lon: -97.5,
            elevation_deg: 0.5,
            value_min: lo,
            value_max: hi,
        }
    }

    /// Map a physical value to its `u8` index for a moment's range.
    fn idx(moment: Moment, v: f32) -> u8 {
        let (lo, hi) = moment.value_range();
        (2.0 + (v - lo) / (hi - lo) * 253.0).round() as u8
    }

    /// Like `sweep`, but at a chosen elevation angle — `detect_volume`'s tests need distinct tilts
    /// that actually differ in beam height, which the plain 0.5° `sweep` always gives the same.
    fn sweep_tilt(
        elevation_deg: f32,
        moment: Moment,
        hot: u8,
        cold: u8,
        hot_az: std::ops::Range<usize>,
        hot_gate: std::ops::Range<usize>,
    ) -> BinnedSweep {
        BinnedSweep {
            elevation_deg,
            ..sweep(moment, hot, cold, hot_az, hot_gate)
        }
    }

    #[test]
    fn flags_low_cc_in_high_z() {
        // Debris ball: CC 0.55 + Z 52 dBZ over a small wedge of gates.
        let cc_hot = idx(Moment::CorrelationCoefficient, 0.55);
        let cc_cold = idx(Moment::CorrelationCoefficient, 0.98);
        let z_hot = idx(Moment::Reflectivity, 52.0);
        let z_cold = idx(Moment::Reflectivity, 20.0);
        let cc = sweep(
            Moment::CorrelationCoefficient,
            cc_hot,
            cc_cold,
            100..112,
            40..60,
        );
        let z = sweep(Moment::Reflectivity, z_hot, z_cold, 100..112, 40..60);

        let hits = detect(&z, &cc, 0.80, 40.0, 150.0, 4);
        assert!(!hits.is_empty(), "debris ball should be flagged");
        assert!(hits[0].min_cc < 0.80);
        // Centroid sits away from the radar (positive range along the hot azimuths).
        assert!(hits[0].lat > 35.0);
    }

    #[test]
    fn ignores_low_cc_in_weak_echo() {
        // Low CC but only 15 dBZ (biological / clutter) → no debris flag.
        let cc = sweep(
            Moment::CorrelationCoefficient,
            idx(Moment::CorrelationCoefficient, 0.5),
            idx(Moment::CorrelationCoefficient, 0.98),
            100..112,
            40..60,
        );
        let z = sweep(
            Moment::Reflectivity,
            idx(Moment::Reflectivity, 15.0),
            idx(Moment::Reflectivity, 10.0),
            100..112,
            40..60,
        );
        assert!(detect(&z, &cc, 0.80, 40.0, 150.0, 4).is_empty());
    }

    /// A synthetic debris ball at one (z, cc) tilt pair, at `elev`. Deliberately small (2 km
    /// radial, ~2°/0.5 km tangential at this range) so it lands inside one ~4 km cluster cell —
    /// `flags_low_cc_in_high_z`'s wider wedge straddles two, which is fine there (it only ever
    /// checks `hits[0]`) but would make these tests' single-hit-per-tilt assumption wrong.
    fn debris_pair(elev: f32) -> (BinnedSweep, BinnedSweep) {
        (
            sweep_tilt(
                elev,
                Moment::Reflectivity,
                idx(Moment::Reflectivity, 52.0),
                idx(Moment::Reflectivity, 20.0),
                100..104,
                40..48,
            ),
            sweep_tilt(
                elev,
                Moment::CorrelationCoefficient,
                idx(Moment::CorrelationCoefficient, 0.55),
                idx(Moment::CorrelationCoefficient, 0.98),
                100..104,
                40..48,
            ),
        )
    }

    #[test]
    fn a_debris_ball_seen_through_two_tilts_scores_higher_than_one() {
        let one_tilt = [debris_pair(0.5)];
        let two_tilts = [debris_pair(0.5), debris_pair(1.5)];

        let single = detect_volume(&one_tilt, 0.80, 40.0, 150.0, 4);
        let double = detect_volume(&two_tilts, 0.80, 40.0, 150.0, 4);
        assert_eq!(single.len(), 1, "one hit from one tilt");
        assert_eq!(double.len(), 1, "the two tilts' hits merge into one column");
        assert_eq!(single[0].tilts, 1);
        assert_eq!(double[0].tilts, 2);
        assert!(
            double[0].confidence > single[0].confidence,
            "two tilts ({}) should score higher than one ({})",
            double[0].confidence,
            single[0].confidence
        );
        assert!(
            double[0].top_km > single[0].top_km,
            "the higher tilt reaches further up: {} vs {}",
            double[0].top_km,
            single[0].top_km
        );
        // `detect()`'s own single-tilt read never claims more than half confidence — no vertical
        // evidence to claim it with.
        assert!(single[0].confidence <= 0.5);
    }

    #[test]
    fn detect_volume_still_reports_a_lone_single_tilt_hit() {
        let hits = detect_volume(&[debris_pair(0.5)], 0.80, 40.0, 150.0, 4);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tilts, 1);
        assert!(hits[0].gates > 0);
    }

    #[test]
    fn detect_volume_of_nothing_is_nothing() {
        assert!(detect_volume(&[], 0.80, 40.0, 150.0, 4).is_empty());
    }
}
