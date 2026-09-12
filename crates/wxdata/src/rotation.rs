//! Client-side velocity-couplet (mesocyclone / TVS) detection from a single Level 2 sweep.
//!
//! A rotation couplet is adjacent inbound and outbound velocity maxima at the same range: the
//! radar sees one side of the vortex moving toward it and the other away. The legacy operational
//! criterion is gate-to-gate azimuthal shear — the velocity difference between neighboring
//! azimuths at one range — with ~25 m/s marking a weak signature and ~36 m/s a strong one.
//!
//! This runs on the dealiased velocity [`BinnedSweep`] the app already bins for display, so it
//! works per volume with no extra download, and it is complementary to the coarse MRMS AzShear
//! national grid (2-min cadence, ~1 km cells) rather than a replacement for it.
//!
//! [`detect`] reads one tilt at a time, which is exactly the gap the *classic* TVS criterion
//! fills with vertical continuity: a couplet confined to a single sweep is as often a gust front,
//! a data glitch, or shallow non-tornadic shear as it is a real, established circulation.
//! [`detect_volume`] runs the same detector across several tilts and raises a cell's confidence
//! by how many of them show the couplet and how high the highest one reaches.
//!
//! Velocity alone has no idea whether there is a storm at a gate or clear air — a receiver
//! glitch, a sidelobe return, or ordinary clear-air/AP noise can clear the gate-to-gate shear
//! threshold just as easily as a real vortex. [`detect`] and [`detect_volume`] both take a
//! collocated reflectivity sweep and require some real echo at the couplet before counting it,
//! the same collocation `crate::tds` already leans on for its own moments.

use crate::level2::{BinnedSweep, Moment};

/// A detected rotation couplet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoupletHit {
    pub lon: f64,
    pub lat: f64,
    /// Rotational velocity: half the inbound/outbound spread over the cluster (m/s).
    pub vrot_ms: f32,
    /// Strongest gate-to-gate velocity difference in the cluster (m/s).
    pub g2g_ms: f32,
    /// Range from the radar to the cluster centroid (km).
    pub range_km: f32,
    /// Candidate gate pairs in the cluster (bigger = more confident).
    pub gates: usize,
    /// How many of the volume's tilts show a couplet at this location. Always 1 out of [`detect`],
    /// which only ever sees one tilt; [`detect_volume`] fills this in for real. The classic
    /// operational TVS criterion is vertical continuity — a couplet that persists through several
    /// low tilts is a real, established circulation, where a single-tilt one is as often a
    /// transient gust front or a data glitch.
    pub tilts: usize,
    /// Beam-center height (km AGL) of the highest tilt contributing to this hit.
    pub top_km: f32,
    /// 0..1 confidence. `detect()`'s single-tilt read has no vertical evidence at all and caps out
    /// at 0.5 on rotational strength alone; [`detect_volume`] can go higher once a couplet repeats
    /// up through the tilts.
    pub confidence: f32,
}

/// Decode a binned `u8` gate index back to its physical value, or `None` for below-threshold /
/// range-folded gates (indices 0/1). Same encoding as [`crate::tds`].
fn decode(sweep: &BinnedSweep, idx: u8) -> Option<f32> {
    if idx < 2 {
        return None;
    }
    let (lo, hi) = (sweep.value_min, sweep.value_max);
    Some(lo + (idx as f32 - 2.0) / 253.0 * (hi - lo))
}

/// Great-circle destination point (places a gate at its azimuth/range).
fn dest(lon: f64, lat: f64, bearing_deg: f64, dist_km: f64) -> (f64, f64) {
    let r = 6371.0;
    let ad = dist_km / r;
    let (br, la1, lo1) = (bearing_deg.to_radians(), lat.to_radians(), lon.to_radians());
    let la2 = (la1.sin() * ad.cos() + la1.cos() * ad.sin() * br.cos()).asin();
    let lo2 = lo1 + (br.sin() * ad.sin() * la1.cos()).atan2(ad.cos() - la1.sin() * la2.sin());
    (lo2.to_degrees(), la2.to_degrees())
}

/// Detect rotation couplets in a velocity sweep. A gate pair is a candidate when two azimuthally
/// adjacent gates at the same range differ by `>= g2g_min_ms` in opposite senses (one inbound,
/// one outbound — same-sign shear is convergence/divergence, not rotation), between
/// `min_range_km` and `max_range_km`. Candidates cluster on a ~4 km geographic grid; clusters
/// with `>= min_gates` pairs become hits, and `vrot` is half the cluster's inbound→outbound spread.
///
/// The near-range floor matters as much as the far one: inside ~15 km the azimuthal bins are only
/// a few hundred metres apart, so ground clutter and ordinary turbulence routinely clear a 25 m/s
/// gate-to-gate difference — a live KTLX clear-air sweep reported six "couplets", all within 8 km.
///
/// `z` is the same-tilt reflectivity sweep; a gate pair only counts when at least one side shows
/// real echo (`>= z_min` dBZ) — a generously low floor, since a TVS often sits at a storm's weaker
/// flank rather than its core, but enough to reject a couplet with no storm behind it at all.
///
/// Feed this the *dealiased* sweep: folded velocity manufactures huge false gate-to-gate jumps.
pub fn detect(
    vel: &BinnedSweep,
    z: &BinnedSweep,
    g2g_min_ms: f32,
    z_min: f32,
    min_range_km: f32,
    max_range_km: f32,
    min_gates: usize,
) -> Vec<CoupletHit> {
    debug_assert_eq!(vel.moment, Moment::Velocity);
    debug_assert_eq!(z.moment, Moment::Reflectivity);
    if vel.az_bins == 0 || vel.gate_count == 0 {
        return Vec::new();
    }
    const CELL: f64 = 0.04; // ~4 km cluster cells, matching the TDS detector
    use std::collections::HashMap;
    /// Running per-cell accumulator: pairs, summed lon/lat/range, min/max velocity, max |dv|.
    type Cell = (usize, f64, f64, f64, f32, f32, f32);
    let mut cells: HashMap<(i64, i64), Cell> = HashMap::new();
    let (rlon, rlat) = (vel.radar_lon as f64, vel.radar_lat as f64);

    for az in 0..vel.az_bins {
        let next = (az + 1) % vel.az_bins; // wraps 719 → 0
        let az_deg = (az as f64 + 0.5) * 360.0 / vel.az_bins as f64;
        for gate in 0..vel.gate_count {
            let range = vel.first_gate_km + gate as f32 * vel.gate_interval_km;
            if range > max_range_km {
                break; // ranges increase with index; nothing further qualifies on this radial
            }
            if range < min_range_km {
                continue;
            }
            let Some(a) = decode(vel, vel.data[az * vel.gate_count + gate]) else {
                continue;
            };
            let Some(b) = decode(vel, vel.data[next * vel.gate_count + gate]) else {
                continue;
            };
            let dv = (a - b).abs();
            if dv < g2g_min_ms || a * b >= 0.0 {
                continue; // too weak, or both gates on the same side of zero (not a couplet)
            }
            // Real echo has to be behind the shear somewhere, or this is clear-air noise or a
            // receiver artifact wearing a couplet's shape, not rotation.
            let zi = ((range - z.first_gate_km) / z.gate_interval_km).round() as i64;
            if zi < 0 || zi as usize >= z.gate_count {
                continue;
            }
            let z_here = z.data[az * z.gate_count + zi as usize];
            let z_next = z.data[next * z.gate_count + zi as usize];
            let has_echo = decode(z, z_here).is_some_and(|v| v >= z_min)
                || decode(z, z_next).is_some_and(|v| v >= z_min);
            if !has_echo {
                continue;
            }
            let (lon, lat) = dest(rlon, rlat, az_deg, range as f64);
            let key = ((lon / CELL).round() as i64, (lat / CELL).round() as i64);
            let e = cells
                .entry(key)
                .or_insert((0, 0.0, 0.0, 0.0, f32::MAX, f32::MIN, 0.0));
            e.0 += 1;
            e.1 += lon;
            e.2 += lat;
            e.3 += range as f64;
            e.4 = e.4.min(a).min(b);
            e.5 = e.5.max(a).max(b);
            e.6 = e.6.max(dv);
        }
    }

    let elev = vel.elevation_deg as f64;
    let mut hits: Vec<CoupletHit> = cells
        .into_values()
        .filter(|(n, ..)| *n >= min_gates)
        .map(|(n, slon, slat, srange, vmin, vmax, g2g)| {
            let range_km = (srange / n as f64) as f32;
            // 25 m/s is the documented weak-signature threshold and 36 the strong one (see the
            // module doc comment); a single-tilt read is capped at 0.5 regardless — the same
            // "this alone isn't real confirmation" cap `tds` applies to gate count, here applied
            // to rotational strength instead.
            let confidence = ((g2g - 25.0) / (36.0 - 25.0)).clamp(0.05, 0.5);
            CoupletHit {
                lon: slon / n as f64,
                lat: slat / n as f64,
                vrot_ms: (vmax - vmin) / 2.0,
                g2g_ms: g2g,
                range_km,
                gates: n,
                tilts: 1,
                top_km: crate::xsection::beam_height_km(range_km as f64, elev) as f32,
                confidence,
            }
        })
        .collect();
    // Strongest rotation first — the alarm and the banner both want the worst one.
    hits.sort_by(|a, b| b.vrot_ms.total_cmp(&a.vrot_ms));
    hits
}

/// Detect couplets across a volume's tilts, not just one — the vertical-continuity check a single
/// sweep cannot offer, and the classic operational TVS criterion this client-side detector has
/// never had. Per-tilt hits (each already computed by [`detect`], so an existing single-tilt
/// caller sees no change) that land in the same ~4 km cell as another tilt's are merged into one
/// [`CoupletHit`] with the combined gate count, the strongest rotation and gate-to-gate shear seen
/// at any contributing tilt, the deepest reach, and a correspondingly higher confidence. `sweeps`
/// need not be given lowest-first — height comes from each sweep's own `elevation_deg`, not its
/// position in the slice.
///
/// A couplet seen at only one tilt is still returned (shallow, transient rotation happens), just
/// without the confidence boost a taller column earns.
///
/// `sweeps` is (velocity, reflectivity) pairs, one per tilt, matching `tds::detect_volume`'s own
/// (z, cc) pairing convention.
pub fn detect_volume(
    sweeps: &[(BinnedSweep, BinnedSweep)],
    g2g_min_ms: f32,
    z_min: f32,
    min_range_km: f32,
    max_range_km: f32,
    min_gates: usize,
) -> Vec<CoupletHit> {
    const CELL: f64 = 0.04; // same grid `detect` clusters on
    use std::collections::HashMap;
    // gates, sum_lon*w, sum_lat*w, sum_range*w, max_vrot, max_g2g, tilts, top_km
    let mut cells: HashMap<(i64, i64), (usize, f64, f64, f64, f32, f32, usize, f32)> =
        HashMap::new();

    for (vel, z) in sweeps {
        for h in detect(vel, z, g2g_min_ms, z_min, min_range_km, max_range_km, min_gates) {
            let key = ((h.lon / CELL).round() as i64, (h.lat / CELL).round() as i64);
            let w = h.gates as f64;
            let e = cells
                .entry(key)
                .or_insert((0, 0.0, 0.0, 0.0, 0.0, 0.0, 0, 0.0));
            e.0 += h.gates;
            e.1 += h.lon * w;
            e.2 += h.lat * w;
            e.3 += h.range_km as f64 * w;
            e.4 = e.4.max(h.vrot_ms);
            e.5 = e.5.max(h.g2g_ms);
            e.6 += 1;
            e.7 = e.7.max(h.top_km);
        }
    }

    let mut out: Vec<CoupletHit> = cells
        .into_values()
        .map(|(gates, slon, slat, srange, vrot_ms, g2g_ms, tilts, top_km)| {
            let w = gates.max(1) as f64;
            // Vertical extent matters more than raw gate count: a couplet that repeats through
            // several tilts is a real, established circulation, while a wide but single-tilt
            // patch is exactly the shape a gust front or a data glitch makes. ~3 km AGL saturates
            // the height term, matching `tds`'s own reference height.
            let height_term = (top_km / 3.0).clamp(0.0, 1.0);
            // Absolute, not "what fraction of the tilts a caller happened to check" — a caller
            // that only ever looks at the lowest tilt must not make a lone hit read as the whole
            // column just because it's 1 out of the 1 it checked. Saturates at 3 tilts.
            let depth_term = ((tilts as f32 - 1.0) / 2.0).clamp(0.0, 1.0);
            let confidence = (0.5 * height_term + 0.5 * depth_term).clamp(0.0, 1.0);
            CoupletHit {
                lon: slon / w,
                lat: slat / w,
                vrot_ms,
                g2g_ms,
                range_km: (srange / w) as f32,
                gates,
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

    /// A velocity sweep that is uniformly `background`, with `inbound`/`outbound` values painted
    /// on two adjacent azimuth wedges over one gate span — a synthetic couplet.
    fn couplet_sweep(inbound: f32, outbound: f32, background: f32) -> BinnedSweep {
        let (az_bins, gate_count) = (720usize, 200usize);
        let (lo, hi) = Moment::Velocity.value_range();
        let idx = |v: f32| (2.0 + (v - lo) / (hi - lo) * 253.0).round() as u8;
        let mut data = vec![idx(background); az_bins * gate_count];
        for g in 40..60 {
            for az in 96..100 {
                data[az * gate_count + g] = idx(inbound);
            }
            for az in 100..104 {
                data[az * gate_count + g] = idx(outbound);
            }
        }
        BinnedSweep {
            moment: Moment::Velocity,
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

    /// Like `couplet_sweep`, but at a chosen elevation and a deliberately small wedge (2 km
    /// radial, 2°/~0.5 km tangential at this range) so it lands inside one ~4 km cluster cell —
    /// `couplet_sweep`'s wider wedge straddles two, which is fine for tests that only ever check
    /// `hits[0]`, but would make a single-hit-per-tilt assumption wrong.
    fn couplet_sweep_tilt(elevation_deg: f32, inbound: f32, outbound: f32, background: f32) -> BinnedSweep {
        let (az_bins, gate_count) = (720usize, 200usize);
        let (lo, hi) = Moment::Velocity.value_range();
        let idx = |v: f32| (2.0 + (v - lo) / (hi - lo) * 253.0).round() as u8;
        let mut data = vec![idx(background); az_bins * gate_count];
        for g in 40..48 {
            for az in 98..100 {
                data[az * gate_count + g] = idx(inbound);
            }
            for az in 100..102 {
                data[az * gate_count + g] = idx(outbound);
            }
        }
        BinnedSweep {
            moment: Moment::Velocity,
            az_bins,
            gate_count,
            data,
            first_gate_km: 2.0,
            gate_interval_km: 0.25,
            radar_lat: 35.0,
            radar_lon: -97.5,
            elevation_deg,
            value_min: lo,
            value_max: hi,
        }
    }

    /// A same-geometry reflectivity companion sweep, uniformly `dbz` — or the no-data sentinel
    /// everywhere when `None`, standing in for clear air (or a corrupted/no-return gate).
    fn z_sweep(elevation_deg: f32, dbz: Option<f32>) -> BinnedSweep {
        let (az_bins, gate_count) = (720usize, 200usize);
        let (lo, hi) = Moment::Reflectivity.value_range();
        let data = match dbz {
            Some(v) => {
                let idx = (2.0 + (v - lo) / (hi - lo) * 253.0).round() as u8;
                vec![idx; az_bins * gate_count]
            }
            None => vec![0u8; az_bins * gate_count],
        };
        BinnedSweep {
            moment: Moment::Reflectivity,
            az_bins,
            gate_count,
            data,
            first_gate_km: 2.0,
            gate_interval_km: 0.25,
            radar_lat: 35.0,
            radar_lon: -97.5,
            elevation_deg,
            value_min: lo,
            value_max: hi,
        }
    }

    #[test]
    fn flags_adjacent_inbound_outbound() {
        let hits = detect(
            &couplet_sweep(-30.0, 30.0, 0.0),
            &z_sweep(0.5, Some(45.0)),
            25.0,
            20.0,
            5.0,
            150.0,
            3,
        );
        assert!(!hits.is_empty(), "a ±30 m/s couplet should be flagged");
        let h = hits[0];
        assert!(
            (h.vrot_ms - 30.0).abs() < 2.0,
            "vrot ≈ 30 m/s, got {}",
            h.vrot_ms
        );
        assert!(h.g2g_ms >= 55.0, "gate-to-gate ≈ 60 m/s, got {}", h.g2g_ms);
        assert!(
            h.range_km > 10.0 && h.range_km < 30.0,
            "range {} km",
            h.range_km
        );
    }

    #[test]
    fn ignores_weak_and_same_sign_shear() {
        let z = z_sweep(0.5, Some(45.0));
        // Weak couplet: ±8 m/s is well under the 25 m/s criterion.
        assert!(detect(&couplet_sweep(-8.0, 8.0, 0.0), &z, 25.0, 20.0, 5.0, 150.0, 3).is_empty());
        // Strong shear but both sides inbound: convergence, not rotation.
        assert!(
            detect(&couplet_sweep(-40.0, -5.0, -5.0), &z, 25.0, 20.0, 5.0, 150.0, 3).is_empty()
        );
    }

    #[test]
    fn range_gates_exclude_far_and_near_couplets() {
        // The synthetic couplet sits ~12-17 km out.
        let s = couplet_sweep(-30.0, 30.0, 0.0);
        let z = z_sweep(0.5, Some(45.0));
        assert!(
            detect(&s, &z, 25.0, 20.0, 5.0, 8.0, 3).is_empty(),
            "beyond the far gate"
        );
        assert!(
            detect(&s, &z, 25.0, 20.0, 30.0, 150.0, 3).is_empty(),
            "inside the near gate"
        );
    }

    #[test]
    fn a_strong_shear_with_no_real_echo_is_not_flagged() {
        // The same ±30 m/s couplet `flags_adjacent_inbound_outbound` accepts, but paired with a
        // clear-air (no-data) reflectivity sweep instead of a real storm — exactly what a
        // receiver glitch, sidelobe return, or clear-air/AP artifact looks like: strong apparent
        // shear with no actual echo behind it.
        let hits = detect(
            &couplet_sweep(-30.0, 30.0, 0.0),
            &z_sweep(0.5, None),
            25.0,
            20.0,
            5.0,
            150.0,
            3,
        );
        assert!(hits.is_empty(), "no real echo behind the shear — not rotation");
    }

    #[test]
    fn a_couplet_seen_through_two_tilts_scores_higher_than_one() {
        let one_tilt = [(couplet_sweep_tilt(0.5, -30.0, 30.0, 0.0), z_sweep(0.5, Some(45.0)))];
        let two_tilts = [
            (couplet_sweep_tilt(0.5, -30.0, 30.0, 0.0), z_sweep(0.5, Some(45.0))),
            (couplet_sweep_tilt(1.5, -30.0, 30.0, 0.0), z_sweep(1.5, Some(45.0))),
        ];

        let single = detect_volume(&one_tilt, 25.0, 20.0, 5.0, 150.0, 3);
        let double = detect_volume(&two_tilts, 25.0, 20.0, 5.0, 150.0, 3);
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
        // A single-tilt read never claims more than half confidence — no vertical evidence to
        // claim it with.
        assert!(single[0].confidence <= 0.5);
    }

    #[test]
    fn detect_volume_still_reports_a_lone_single_tilt_hit() {
        let hits = detect_volume(
            &[(couplet_sweep_tilt(0.5, -30.0, 30.0, 0.0), z_sweep(0.5, Some(45.0)))],
            25.0,
            20.0,
            5.0,
            150.0,
            3,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tilts, 1);
        assert!(hits[0].gates > 0);
    }

    #[test]
    fn detect_volume_of_nothing_is_nothing() {
        assert!(detect_volume(&[], 25.0, 20.0, 5.0, 150.0, 3).is_empty());
    }
}
