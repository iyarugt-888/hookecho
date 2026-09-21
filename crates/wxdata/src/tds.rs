//! Tornado Debris Signature (TDS) detection.
//!
//! A TDS is a "debris ball": lofted tornado debris scatters radar energy incoherently, dropping the
//! correlation coefficient (CC/ρhv) well below meteorological values while reflectivity stays high.
//! The classic operational heuristic — low CC collocated with high reflectivity in the storm core —
//! is what this module flags. (Collocation with a velocity couplet / rotation strengthens a real
//! diagnosis; that's left to the human reading the flagged location.)
//!
//! Input is two same-tilt [`BinnedSweep`]s (reflectivity + CC) sharing the 720-azimuth grid.
//!
//! # What separates a debris ball from the things that look like one
//!
//! Low CC in strong echo is also what large wet hail, melting hail, biological scatter and ground
//! clutter can produce, so the two thresholds alone are a candidate finder, not a verdict. What
//! tells them apart is the *shape of the evidence*:
//!
//! * a debris ball is a **compact hole in otherwise high CC**: the gates around it are meteorological
//!   (CC near 1) and it is small. A broad region of uniformly low CC is not one, however low its
//!   minimum;
//! * it is **deep**: debris drives CC well below the threshold, while marginal hits sit just under it;
//! * it sits in **strong echo**, and repeats **up through the tilts** as a column rather than living
//!   in whichever single sweep sees clutter.
//!
//! [`detect`] reads one tilt: it clusters candidate gates by contiguity in the radar's own
//! azimuth/range grid (so a ball is never split by an arbitrary map grid) and scores each cluster on
//! depth, core strength, size and contrast with its surroundings. [`detect_volume`] runs it across
//! tilts, associates hits that overlap on the ground into one column, and scales the confidence by
//! the vertical evidence.

use crate::level2::{BinnedSweep, Moment};
use std::collections::HashSet;

/// A cluster bigger than this (km², summed gate areas) is not a debris ball: it is a region of low
/// CC, the shape hail cores and biological scatter make.
pub const MAX_AREA_KM2: f32 = 120.0;

/// Hits from different tilts closer than this (km) are the same column.
const ASSOCIATE_KM: f64 = 3.0;

/// How many gates around a cluster (in azimuth and range) are read as its surroundings.
const RING: isize = 3;

/// The fewest surrounding gates with data for a contrast to mean anything.
const MIN_RING_GATES: usize = 8;

/// A detected debris-signature cluster.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TdsHit {
    pub lon: f64,
    pub lat: f64,
    /// Number of candidate gates in the cluster.
    pub gates: usize,
    /// Lowest CC seen in the cluster.
    pub min_cc: f32,
    /// Average CC over the cluster's gates.
    pub mean_cc: f32,
    /// Average reflectivity over the cluster's gates, dBZ.
    pub mean_z: f32,
    /// Strongest reflectivity in the cluster, dBZ.
    pub max_z: f32,
    /// Ground covered by the cluster, km². A debris ball is small; a broad low-CC region is not.
    pub area_km2: f32,
    /// How much higher the CC is in the gates just around the cluster than inside it. A debris ball
    /// is a hole in high CC, so this is large; where the surroundings are just as low it is near
    /// zero. `None` when too few surrounding gates have data to say.
    pub contrast: Option<f32>,
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
    /// 0..1 confidence, from the evidence above and the vertical continuity. A single tilt has no
    /// vertical evidence, so [`detect`] never reports more than [`SINGLE_TILT_CAP`]; a hit that
    /// repeats up through the tilts earns the rest.
    pub confidence: f32,
}

/// The most confidence a hit can have from one tilt alone.
pub const SINGLE_TILT_CAP: f32 = 0.6;

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

/// How strongly the cluster itself looks like debris, 0..1, before any vertical evidence.
///
/// Four terms, each 0..1, weighted so that the two that separate debris from hail and biological
/// scatter (depth and contrast) count most, then discounted for range:
///
/// * **depth** (0.30): how far CC sits below the meteorological range, judged on the average of the
///   cluster's lowest gate and its mean. A debris ball has many gates well down; marginal hail has a
///   low gate or two in a cluster that is barely under the threshold on average, and the minimum
///   alone would let one outlier gate carry it. 0.80 scores 0, 0.50 scores 1.
/// * **contrast** (0.30): how much higher the surroundings are. Under 0.02 scores 0 (the
///   surroundings are just as low, so this is not a hole in anything); 0.15 or more scores 1. With
///   no reading it scores a neutral 0.5 rather than pretending to know.
/// * **core** (0.20): mean reflectivity. 35 dBZ scores 0, 55 dBZ scores 1.
/// * **size** (0.20): a debris ball covers a few km². Tiny clusters ramp up from 0.1 km²; anything
///   up to 15 km² is fully plausible; beyond that it fades toward 0.2 at [`MAX_AREA_KM2`].
///
/// **Range**: out past 60 km the beam is wide and a debris ball is only a few gates, so the whole
/// score fades linearly to 70% by 150 km. The same evidence is less trustworthy far away.
pub fn evidence(
    min_cc: f32,
    mean_cc: f32,
    mean_z: f32,
    area_km2: f32,
    contrast: Option<f32>,
    range_km: f32,
) -> f32 {
    let typical_low = 0.5 * min_cc + 0.5 * mean_cc;
    let depth = ((0.80 - typical_low) / 0.30).clamp(0.0, 1.0);
    let core = ((mean_z - 35.0) / 20.0).clamp(0.0, 1.0);
    let size = if area_km2 < 0.5 {
        ((area_km2 - 0.1) / 0.4).clamp(0.0, 1.0)
    } else if area_km2 <= 15.0 {
        1.0
    } else {
        (1.0 - 0.8 * (area_km2 - 15.0) / (MAX_AREA_KM2 - 15.0)).clamp(0.2, 1.0)
    };
    let contrast = contrast.map_or(0.5, |c| ((c - 0.02) / 0.13).clamp(0.0, 1.0));
    let range = 1.0 - 0.3 * ((range_km - 60.0) / 90.0).clamp(0.0, 1.0);
    ((0.30 * depth + 0.30 * contrast + 0.20 * core + 0.20 * size) * range).clamp(0.0, 1.0)
}

/// Vertical evidence, 0..1: how many tilts show the hit, and how high the column reaches. About
/// three tilts and 3 km AGL each saturate their half — a debris signature repeating through the
/// lowest three or four elevation angles is about as convincing as this heuristic gets.
///
/// Height only counts when more than one tilt confirms the signature there. At long range even the
/// lowest beam is kilometres above the ground purely from the beam's geometry, so a lone hit's
/// `top_km` says where the beam was, not that debris was lofted; crediting it would rank a distant
/// four-gate speck above a nearby one for no reason. And it is absolute, not "what share of the
/// tilts a caller happened to check": a caller that only looks at the lowest tilt must not make a
/// lone hit read as the whole column.
pub fn vertical_term(top_km: f32, tilts: usize) -> f32 {
    if tilts < 2 {
        return 0.0;
    }
    let height = (top_km / 3.0).clamp(0.0, 1.0);
    let depth = ((tilts as f32 - 1.0) / 2.0).clamp(0.0, 1.0);
    0.5 * height + 0.5 * depth
}

/// Confidence from the cluster's evidence and its vertical evidence. With none of the latter the
/// most it can be is [`SINGLE_TILT_CAP`].
fn confidence(evidence: f32, vertical: f32) -> f32 {
    (evidence * (SINGLE_TILT_CAP + (1.0 - SINGLE_TILT_CAP) * vertical)).clamp(0.0, 1.0)
}

/// Detect debris-signature clusters on one tilt. A gate is a candidate when CC `< cc_max`,
/// reflectivity `>= z_min` dBZ, and its range `<= max_range_km` (resolution/low-tilt gating).
/// Contiguous candidates (8-connected in azimuth/range, wrapping at north) form a cluster; a
/// cluster becomes a hit when it has `>= min_gates` gates, spans more than one azimuth (a single
/// bad radial is a glitch, not a ball) and is no bigger than [`MAX_AREA_KM2`].
pub fn detect(
    z: &BinnedSweep,
    cc: &BinnedSweep,
    cc_max: f32,
    z_min: f32,
    max_range_km: f32,
    min_gates: usize,
) -> Vec<TdsHit> {
    debug_assert_eq!(cc.moment, Moment::CorrelationCoefficient);
    if cc.az_bins == 0 || cc.gate_count == 0 || z.gate_count == 0 || z.az_bins == 0 {
        return Vec::new();
    }
    let (nb, ng) = (cc.az_bins, cc.gate_count);
    let (rlon, rlat) = (cc.radar_lon as f64, cc.radar_lat as f64);
    let az_step = 360.0 / nb as f64;

    // Candidate gates: (cc, z). Gates increase in range with index, so a radial can stop early.
    let mut cand: Vec<Option<(f32, f32)>> = vec![None; nb * ng];
    for az in 0..nb {
        let z_az = az * z.az_bins / nb;
        for gate in 0..ng {
            let range = cc.first_gate_km + gate as f32 * cc.gate_interval_km;
            if range > max_range_km {
                break;
            }
            let Some(cc_val) = decode(cc, cc.data[az * ng + gate]) else {
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
            let Some(z_val) = decode(z, z.data[z_az * z.gate_count + zi as usize]) else {
                continue;
            };
            if z_val < z_min {
                continue; // low CC in weak echo = biological/clutter, not debris
            }
            cand[az * ng + gate] = Some((cc_val, z_val));
        }
    }

    // Label 8-connected components in the polar grid, wrapping in azimuth.
    let mut seen = vec![false; nb * ng];
    let mut hits = Vec::new();
    let elev = cc.elevation_deg as f64;
    for start in 0..nb * ng {
        if seen[start] || cand[start].is_none() {
            continue;
        }
        let mut members: Vec<usize> = Vec::new();
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(i) = stack.pop() {
            members.push(i);
            let (az, gate) = (i / ng, i % ng);
            for da in -1isize..=1 {
                for dg in -1isize..=1 {
                    let g2 = gate as isize + dg;
                    if (da == 0 && dg == 0) || g2 < 0 || g2 >= ng as isize {
                        continue;
                    }
                    let a2 = (az as isize + da).rem_euclid(nb as isize) as usize;
                    let j = a2 * ng + g2 as usize;
                    if !seen[j] && cand[j].is_some() {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        if members.len() < min_gates {
            continue;
        }
        // A cluster confined to one azimuth is a single bad radial — a stuck bit or a receiver
        // glitch paints a solid low-CC/high-Z streak the length of the beam. A genuine debris ball,
        // being an actual 3D object with some width, paints across at least two azimuth samples.
        let azimuths: HashSet<usize> = members.iter().map(|i| i / ng).collect();
        if azimuths.len() < 2 {
            continue;
        }

        let n = members.len() as f64;
        let (mut s_lon, mut s_lat, mut s_range, mut area) = (0.0, 0.0, 0.0, 0.0f64);
        let (mut min_cc, mut s_cc, mut s_z, mut max_z) = (f32::MAX, 0.0f64, 0.0f64, f32::MIN);
        for &i in &members {
            let (az, gate) = (i / ng, i % ng);
            let range = (cc.first_gate_km + gate as f32 * cc.gate_interval_km) as f64;
            let (lon, lat) = dest(rlon, rlat, az as f64 * az_step, range);
            s_lon += lon;
            s_lat += lat;
            s_range += range;
            // One gate covers an arc of `range * dθ` by one gate interval.
            area += range * az_step.to_radians() * cc.gate_interval_km as f64;
            let (c, zz) = cand[i].expect("members are candidates");
            min_cc = min_cc.min(c);
            s_cc += c as f64;
            s_z += zz as f64;
            max_z = max_z.max(zz);
        }
        let area_km2 = area as f32;
        if area_km2 > MAX_AREA_KM2 {
            continue; // a region of low CC, not a debris ball
        }
        let mean_cc = (s_cc / n) as f32;
        let mean_z = (s_z / n) as f32;
        let contrast = surroundings_contrast(cc, &members, mean_cc);
        let range_km = (s_range / n) as f32;
        let top_km = crate::xsection::beam_height_km(range_km as f64, elev) as f32;
        hits.push(TdsHit {
            lon: s_lon / n,
            lat: s_lat / n,
            gates: members.len(),
            min_cc,
            mean_cc,
            mean_z,
            max_z,
            area_km2,
            contrast,
            range_km,
            tilts: 1,
            top_km,
            confidence: confidence(
                evidence(min_cc, mean_cc, mean_z, area_km2, contrast, range_km),
                0.0,
            ),
        });
    }
    hits.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    hits
}

/// The CC of the gates just around a cluster minus the cluster's own mean, or `None` if too few of
/// them have data. The surroundings are every gate within [`RING`] gates in azimuth and range that
/// is not part of the cluster; gates with no CC reading (no echo) are ignored, not counted as low.
fn surroundings_contrast(cc: &BinnedSweep, members: &[usize], mean_cc: f32) -> Option<f32> {
    let (nb, ng) = (cc.az_bins, cc.gate_count);
    let inside: HashSet<usize> = members.iter().copied().collect();
    let mut ring: HashSet<usize> = HashSet::new();
    for &i in members {
        let (az, gate) = (i / ng, i % ng);
        for da in -RING..=RING {
            for dg in -RING..=RING {
                let g2 = gate as isize + dg;
                if g2 < 0 || g2 >= ng as isize {
                    continue;
                }
                let a2 = (az as isize + da).rem_euclid(nb as isize) as usize;
                let j = a2 * ng + g2 as usize;
                if !inside.contains(&j) {
                    ring.insert(j);
                }
            }
        }
    }
    let values: Vec<f32> = ring
        .iter()
        .filter_map(|&j| decode(cc, cc.data[j]))
        .collect();
    (values.len() >= MIN_RING_GATES)
        .then(|| values.iter().sum::<f32>() / values.len() as f32 - mean_cc)
}

/// Ground distance in km between two lon/lat points, good over the few km hits are compared at.
fn ground_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let mid_lat = ((a.1 + b.1) * 0.5).to_radians();
    let dx = (b.0 - a.0) * mid_lat.cos() * 111.32;
    let dy = (b.1 - a.1) * 110.57;
    dx.hypot(dy)
}

/// Detect debris signatures across a volume's tilts, not just one — the vertical-continuity check
/// a single sweep cannot offer. Per-tilt hits (each already computed by [`detect`], so an existing
/// single-tilt caller sees no change) that lie within a few km of one another are merged into one
/// [`TdsHit`] column with the combined gate count, the deepest reach, and a confidence raised by
/// the vertical evidence. `sweeps` need not be given lowest-first — height comes from each pair's
/// own `elevation_deg`, not its position in the slice.
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
    let mut per_tilt: Vec<(usize, TdsHit)> = Vec::new();
    for (tilt, (z, cc)) in sweeps.iter().enumerate() {
        for h in detect(z, cc, cc_max, z_min, max_range_km, min_gates) {
            per_tilt.push((tilt, h));
        }
    }

    // Group hits by ground proximity (union-find), so two tilts' views of one ball merge no matter
    // where an arbitrary grid would have put the boundary.
    let n = per_tilt.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (&per_tilt[i].1, &per_tilt[j].1);
            if ground_km((a.lon, a.lat), (b.lon, b.lat)) <= ASSOCIATE_KM {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[rj] = ri;
                }
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..n {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }

    let mut out: Vec<TdsHit> = groups
        .into_values()
        .map(|members| {
            let hits: Vec<&TdsHit> = members.iter().map(|&i| &per_tilt[i].1).collect();
            let tilts = members
                .iter()
                .map(|&i| per_tilt[i].0)
                .collect::<HashSet<_>>()
                .len();
            let gates: usize = hits.iter().map(|h| h.gates).sum();
            let w = gates.max(1) as f64;
            let weighted = |f: &dyn Fn(&TdsHit) -> f64| -> f64 {
                hits.iter().map(|h| f(h) * h.gates as f64).sum::<f64>() / w
            };
            let min_cc = hits.iter().map(|h| h.min_cc).fold(f32::MAX, f32::min);
            let mean_cc = weighted(&|h| f64::from(h.mean_cc)) as f32;
            let mean_z = weighted(&|h| f64::from(h.mean_z)) as f32;
            // The footprint is the biggest any tilt saw, not their sum: tilts stack in height, they
            // do not cover more ground.
            let area_km2 = hits.iter().map(|h| h.area_km2).fold(0.0, f32::max);
            let contrasts: Vec<(f32, usize)> = hits
                .iter()
                .filter_map(|h| h.contrast.map(|c| (c, h.gates)))
                .collect();
            let contrast = (!contrasts.is_empty()).then(|| {
                let total: usize = contrasts.iter().map(|(_, g)| g).sum();
                contrasts.iter().map(|(c, g)| c * *g as f32).sum::<f32>() / total.max(1) as f32
            });
            let top_km = hits.iter().map(|h| h.top_km).fold(0.0, f32::max);
            let range_km = weighted(&|h| f64::from(h.range_km)) as f32;
            let ev = evidence(min_cc, mean_cc, mean_z, area_km2, contrast, range_km);
            TdsHit {
                lon: weighted(&|h| h.lon),
                lat: weighted(&|h| h.lat),
                gates,
                min_cc,
                mean_cc,
                mean_z,
                max_z: hits.iter().map(|h| h.max_z).fold(f32::MIN, f32::max),
                area_km2,
                contrast,
                range_km,
                tilts,
                top_km,
                confidence: confidence(ev, vertical_term(top_km, tilts)),
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
            ..Default::default()
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

    /// A (z, cc) pair with a hot wedge of the given CC/Z over a clean high-CC, low-Z background.
    fn pair(
        cc_hot: f32,
        cc_cold: f32,
        z_hot: f32,
        az: std::ops::Range<usize>,
        gates: std::ops::Range<usize>,
    ) -> (BinnedSweep, BinnedSweep) {
        (
            sweep(
                Moment::Reflectivity,
                idx(Moment::Reflectivity, z_hot),
                idx(Moment::Reflectivity, 30.0),
                az.clone(),
                gates.clone(),
            ),
            sweep(
                Moment::CorrelationCoefficient,
                idx(Moment::CorrelationCoefficient, cc_hot),
                idx(Moment::CorrelationCoefficient, cc_cold),
                az,
                gates,
            ),
        )
    }

    fn run(p: &(BinnedSweep, BinnedSweep)) -> Vec<TdsHit> {
        detect(&p.0, &p.1, 0.80, 40.0, 150.0, 4)
    }

    #[test]
    fn flags_low_cc_in_high_z() {
        // Debris ball: CC 0.55 + Z 52 dBZ over a small wedge of gates.
        let hits = run(&pair(0.55, 0.98, 52.0, 100..112, 40..60));
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

    #[test]
    fn a_single_bad_radial_is_not_a_debris_ball() {
        // Confined to one azimuth and stretched across many range gates — exactly the shape a
        // stuck-bit or receiver glitch paints down a single radial.
        assert!(
            run(&pair(0.55, 0.98, 52.0, 100..101, 40..80)).is_empty(),
            "a one-azimuth streak should read as a bad radial, not debris"
        );
    }

    #[test]
    fn one_ball_is_one_hit_however_it_falls_on_a_map_grid() {
        // A wide wedge (~12 azimuths x 20 gates) that a fixed ~4 km map grid would split across
        // cells. Contiguity in the radar's own grid keeps it whole.
        let hits = run(&pair(0.55, 0.98, 52.0, 100..112, 40..60));
        assert_eq!(hits.len(), 1, "one contiguous ball is one hit: {hits:?}");
        assert_eq!(hits[0].gates, 12 * 20);
    }

    #[test]
    fn two_separate_balls_are_two_hits_and_a_ball_across_north_is_one() {
        // Two balls well apart in azimuth.
        let (mut z, mut cc) = pair(0.55, 0.98, 52.0, 100..106, 40..46);
        let (z2, cc2) = pair(0.55, 0.98, 52.0, 300..306, 40..46);
        for i in 0..z.data.len() {
            if cc2.data[i] != idx(Moment::CorrelationCoefficient, 0.98) {
                cc.data[i] = cc2.data[i];
                z.data[i] = z2.data[i];
            }
        }
        assert_eq!(run(&(z, cc)).len(), 2);

        // A ball straddling azimuth 0/719 is one cluster, not two halves.
        let (mut z, mut cc) = pair(0.55, 0.98, 52.0, 0..3, 40..46);
        let (z2, cc2) = pair(0.55, 0.98, 52.0, 717..720, 40..46);
        for i in 0..z.data.len() {
            if cc2.data[i] != idx(Moment::CorrelationCoefficient, 0.98) {
                cc.data[i] = cc2.data[i];
                z.data[i] = z2.data[i];
            }
        }
        let hits = run(&(z, cc));
        assert_eq!(
            hits.len(),
            1,
            "wrapping at north must not split a ball: {hits:?}"
        );
    }

    #[test]
    fn a_broad_region_of_low_cc_is_not_a_debris_ball() {
        // The same CC/Z values as a debris ball, but over a huge area: hail core or biological
        // scatter, however low its minimum.
        let hits = run(&pair(0.55, 0.98, 52.0, 100..180, 40..140));
        assert!(
            hits.is_empty(),
            "an 80° x 25 km slab is not a ball: {hits:?}"
        );
    }

    #[test]
    fn a_hole_in_high_cc_outscores_a_dip_in_already_low_cc() {
        // Same marginal dip (0.78, barely under the 0.80 threshold), same size: one sits in clean
        // CC 0.98, the other in a background already at 0.82. Only the first is a hole in
        // anything; the second is a slight dip in a region that was low to begin with.
        let clean = run(&pair(0.78, 0.98, 52.0, 100..108, 40..50));
        let muddy = run(&pair(0.78, 0.82, 52.0, 100..108, 40..50));
        assert_eq!(clean.len(), 1);
        assert_eq!(muddy.len(), 1);
        let (c, m) = (clean[0].contrast.unwrap(), muddy[0].contrast.unwrap());
        assert!(c > m + 0.1, "contrast {c} vs {m}");
        assert!(
            clean[0].confidence > muddy[0].confidence + 0.1,
            "{} vs {}",
            clean[0].confidence,
            muddy[0].confidence
        );
    }

    #[test]
    fn a_deeper_dip_and_a_stronger_core_raise_confidence() {
        let deep = run(&pair(0.45, 0.98, 55.0, 100..108, 40..50));
        let shallow = run(&pair(0.78, 0.98, 55.0, 100..108, 40..50));
        assert!(deep[0].confidence > shallow[0].confidence);
        let strong = run(&pair(0.55, 0.98, 58.0, 100..108, 40..50));
        let weak = run(&pair(0.55, 0.98, 41.0, 100..108, 40..50));
        assert!(strong[0].confidence > weak[0].confidence);
        assert!(deep[0].min_cc < shallow[0].min_cc);
        assert!(strong[0].mean_z > weak[0].mean_z);
    }

    #[test]
    fn no_surroundings_to_compare_is_neutral_not_a_guess() {
        // A sweep that is empty (no echo) around the ball has no CC to compare with: contrast is
        // unknown, and the score treats that as neutral rather than as a great or a poor contrast.
        let (mut z, mut cc) = pair(0.55, 0.98, 52.0, 100..108, 40..50);
        let ng = cc.gate_count;
        for az in 0..cc.az_bins {
            for g in 0..ng {
                let inside = (100..108).contains(&az) && (40..50).contains(&g);
                if !inside {
                    cc.data[az * ng + g] = 0;
                    z.data[az * ng + g] = 0;
                }
            }
        }
        let hits = run(&(z, cc));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].contrast, None);
        let known = run(&pair(0.55, 0.98, 52.0, 100..108, 40..50));
        assert!(
            hits[0].confidence < known[0].confidence,
            "a proven hole beats an unknown one"
        );
    }

    #[test]
    fn the_evidence_terms_behave_at_their_ends() {
        // Depth and contrast at their extremes.
        assert!(evidence(0.40, 0.40, 60.0, 5.0, Some(0.30), 20.0) > 0.95);
        assert!(evidence(0.85, 0.85, 35.0, 5.0, Some(0.0), 20.0) < 0.25);
        // Size: tiny and huge are both discounted against a plausible few km².
        let plausible = evidence(0.55, 0.55, 52.0, 5.0, Some(0.2), 20.0);
        assert!(evidence(0.55, 0.55, 52.0, 0.05, Some(0.2), 20.0) < plausible);
        assert!(evidence(0.55, 0.55, 52.0, 100.0, Some(0.2), 20.0) < plausible);
        // Range: the same evidence is worth less far out, and nothing changes up close.
        assert_eq!(evidence(0.55, 0.55, 52.0, 5.0, Some(0.2), 40.0), plausible);
        let far = evidence(0.55, 0.55, 52.0, 5.0, Some(0.2), 150.0);
        assert!((far / plausible - 0.7).abs() < 1e-3, "{far} vs {plausible}");
        // Everything stays in 0..1.
        for cc in [0.0, 0.5, 1.0] {
            for area in [0.0, 5.0, 500.0] {
                let e = evidence(cc, cc, 90.0, area, Some(5.0), 500.0);
                assert!((0.0..=1.0).contains(&e), "{e}");
            }
        }
    }

    #[test]
    fn a_lone_hit_high_in_the_beam_earns_no_credit_for_the_beam_height() {
        // At long range a single low tilt is high off the ground just from beam geometry. That is
        // not lofted debris, so it must not lift a lone hit above the single-tilt cap.
        assert_eq!(vertical_term(6.0, 1), 0.0);
        assert!(
            vertical_term(6.0, 2) > 0.5,
            "but two tilts agreeing there do count"
        );
        // A steep tilt puts the beam well over 3 km AGL even at this short range.
        let high = detect_volume(&[debris_pair(25.0, 100..104)], 0.80, 40.0, 150.0, 4);
        assert_eq!(high.len(), 1);
        assert!(
            high[0].top_km > 3.0,
            "the beam really is high: {}",
            high[0].top_km
        );
        assert!(
            high[0].confidence <= SINGLE_TILT_CAP + 1e-6,
            "{}",
            high[0].confidence
        );
    }

    #[test]
    fn a_cluster_deep_throughout_beats_one_with_a_single_low_gate() {
        // Same minimum, but one cluster is low across the board and the other is barely under the
        // threshold on average with one outlier gate. The minimum alone cannot tell them apart.
        let throughout = evidence(0.35, 0.45, 52.0, 5.0, Some(0.2), 30.0);
        let outlier = evidence(0.35, 0.77, 52.0, 5.0, Some(0.2), 30.0);
        assert!(throughout > outlier + 0.04, "{throughout} vs {outlier}");
        // And the real-world pair this was written for: a Moore-like ball against marginal hail.
        let ball = evidence(0.21, 0.55, 56.0, 9.0, Some(0.25), 25.0);
        let hail = evidence(0.59, 0.73, 48.0, 1.0, Some(0.19), 88.0);
        assert!(ball > hail + 0.2, "{ball} vs {hail}");
    }

    #[test]
    fn one_tilt_can_never_claim_more_than_the_cap() {
        // Even a perfect single-tilt hole cannot exceed the cap: no vertical evidence.
        let hits = run(&pair(0.30, 0.99, 60.0, 100..108, 40..50));
        assert!(
            hits[0].confidence <= SINGLE_TILT_CAP + 1e-6,
            "{}",
            hits[0].confidence
        );
        // ... and a perfect column can reach past it.
        assert!(confidence(1.0, 1.0) > SINGLE_TILT_CAP);
        assert!((confidence(1.0, 0.0) - SINGLE_TILT_CAP).abs() < 1e-6);
    }

    /// A synthetic debris ball at one (z, cc) tilt pair, at `elev`, over the given azimuths.
    fn debris_pair(elev: f32, az: std::ops::Range<usize>) -> (BinnedSweep, BinnedSweep) {
        (
            sweep_tilt(
                elev,
                Moment::Reflectivity,
                idx(Moment::Reflectivity, 52.0),
                idx(Moment::Reflectivity, 30.0),
                az.clone(),
                40..48,
            ),
            sweep_tilt(
                elev,
                Moment::CorrelationCoefficient,
                idx(Moment::CorrelationCoefficient, 0.55),
                idx(Moment::CorrelationCoefficient, 0.98),
                az,
                40..48,
            ),
        )
    }

    #[test]
    fn a_debris_ball_seen_through_two_tilts_scores_higher_than_one() {
        let one_tilt = [debris_pair(0.5, 100..104)];
        let two_tilts = [debris_pair(0.5, 100..104), debris_pair(1.5, 100..104)];

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
        assert!(single[0].confidence <= SINGLE_TILT_CAP + 1e-6);
        // The column's footprint is one tilt's, not the sum of two.
        assert!((double[0].area_km2 - single[0].area_km2).abs() < 0.01);
    }

    #[test]
    fn tilts_associate_by_ground_distance_and_far_apart_hits_stay_separate() {
        // Same ball, drifted a couple of azimuth samples between tilts (~1.5 km at this range):
        // still one column.
        let near = [debris_pair(0.5, 100..104), debris_pair(1.5, 102..106)];
        let hits = detect_volume(&near, 0.80, 40.0, 150.0, 4);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].tilts, 2);
        // A hit on the other side of the radar is a different column, however many tilts there are.
        let far = [debris_pair(0.5, 100..104), debris_pair(1.5, 400..404)];
        let hits = detect_volume(&far, 0.80, 40.0, 150.0, 4);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert!(hits.iter().all(|h| h.tilts == 1));
    }

    #[test]
    fn detect_volume_still_reports_a_lone_single_tilt_hit() {
        let hits = detect_volume(&[debris_pair(0.5, 100..104)], 0.80, 40.0, 150.0, 4);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tilts, 1);
        assert!(hits[0].gates > 0);
    }

    #[test]
    fn detect_volume_of_nothing_is_nothing() {
        assert!(detect_volume(&[], 0.80, 40.0, 150.0, 4).is_empty());
    }

    #[test]
    fn hits_come_back_strongest_first() {
        let (mut z, mut cc) = pair(0.40, 0.98, 55.0, 100..108, 40..50);
        let (z2, cc2) = pair(0.75, 0.98, 42.0, 300..308, 40..50);
        for i in 0..z.data.len() {
            if cc2.data[i] != idx(Moment::CorrelationCoefficient, 0.98) {
                cc.data[i] = cc2.data[i];
                z.data[i] = z2.data[i];
            }
        }
        let hits = run(&(z, cc));
        assert_eq!(hits.len(), 2);
        assert!(hits[0].confidence > hits[1].confidence);
        assert!(hits[0].min_cc < hits[1].min_cc);
    }
}
