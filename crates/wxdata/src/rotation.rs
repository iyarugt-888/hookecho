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
    /// What people and forecasters said about it: a tornado report near it, or an observed
    /// tornado warning over it. Separate from `confidence`, which is radar alone and stops at 100%;
    /// see [`crate::confirm`]. Set by the caller, never by the detector.
    pub confirmation: crate::confirm::Confirmation,
}

/// Which revision of the scoring produced a hit; bump it when any weight or rule below changes.
pub const ALGORITHM_VERSION: &str = "rot-2";

/// The most confidence a couplet can have from one tilt alone.
pub const SINGLE_TILT_CAP: f32 = 0.5;

/// How well the gate-to-gate shear supports a real circulation, 0..1: 25 m/s is the documented
/// weak-signature threshold and scores 0, 36 m/s the strong one and scores 1.
pub fn strength_term(g2g_ms: f32) -> f32 {
    ((g2g_ms - 25.0) / (36.0 - 25.0)).clamp(0.0, 1.0)
}

/// How well the size of the cluster supports it, 0..1, from the candidate gate pairs per tilt: a
/// couplet is a coherent patch, and a few stray pairs are as often a glitch. Three pairs (the
/// smallest cluster reported) scores 0, twelve or more scores 1.
pub fn size_term(pairs_per_tilt: f32) -> f32 {
    ((pairs_per_tilt - 3.0) / 9.0).clamp(0.0, 1.0)
}

/// Range factor, 1 out to 60 km and fading to 0.6 by 150 km. Far out the beam is wide enough that
/// a couplet is a handful of gates, and the velocity data is where dealiasing failures cluster.
pub fn range_factor(range_km: f32) -> f32 {
    1.0 - 0.4 * ((range_km - 60.0) / 90.0).clamp(0.0, 1.0)
}

/// Vertical continuity, 0..1: height reached and number of tilts, each worth half. About 3 km AGL
/// saturates the height and three tilts the depth. One tilt is worth nothing: its height is just its
/// range (a far low beam is high, and that is not a tall circulation), so height only counts once a
/// second tilt shows the couplet is a column.
pub fn vertical_term(top_km: f32, tilts: usize) -> f32 {
    if tilts < 2 {
        return 0.0;
    }
    let height = (top_km / 3.0).clamp(0.0, 1.0);
    let depth = ((tilts as f32 - 1.0) / 2.0).clamp(0.0, 1.0);
    0.5 * height + 0.5 * depth
}

/// Evidence from one cluster's own measurements: strength (65%) and size (35%), faded by range.
pub fn evidence(g2g_ms: f32, pairs_per_tilt: f32, range_km: f32) -> f32 {
    ((0.65 * strength_term(g2g_ms) + 0.35 * size_term(pairs_per_tilt)) * range_factor(range_km))
        .clamp(0.0, 1.0)
}

/// Confidence from the evidence and the vertical continuity. With none of the latter the most it
/// can be is [`SINGLE_TILT_CAP`]; a couplet that repeats up through the tilts earns the rest.
pub fn confidence(evidence: f32, vertical: f32) -> f32 {
    (evidence * (SINGLE_TILT_CAP + (1.0 - SINGLE_TILT_CAP) * vertical)).clamp(0.0, 1.0)
}

/// Why a [`CoupletHit`] scored what it did; see [`CoupletHit::explain`].
#[derive(Debug, Clone, PartialEq)]
pub struct Explanation {
    pub version: &'static str,
    /// The weighted terms.
    pub reasons: Vec<crate::tds::Reason>,
    pub range_factor: f32,
    /// The weighted terms times the range factor.
    pub evidence: f32,
    /// Vertical continuity, 0..1; 0 for a single shallow tilt.
    pub vertical: f32,
    pub confidence: f32,
}

impl CoupletHit {
    /// Break this hit's confidence down into the evidence behind it, built with the same functions
    /// that scored it so it cannot drift from the number on the map.
    pub fn explain(&self) -> Explanation {
        let per_tilt = self.gates as f32 / self.tilts.max(1) as f32;
        let reasons = vec![
            crate::tds::Reason {
                label: "Strength",
                detail: format!(
                    "{:.0} kt gate-to-gate, {:.0} kt rotational",
                    self.g2g_ms * 1.943_844,
                    self.vrot_ms * 1.943_844
                ),
                score: strength_term(self.g2g_ms),
                weight: 0.65,
            },
            crate::tds::Reason {
                label: "Size",
                detail: format!("{per_tilt:.0} gate pairs per tilt"),
                score: size_term(per_tilt),
                weight: 0.35,
            },
        ];
        Explanation {
            version: ALGORITHM_VERSION,
            reasons,
            range_factor: range_factor(self.range_km),
            evidence: evidence(self.g2g_ms, per_tilt, self.range_km),
            vertical: vertical_term(self.top_km, self.tilts),
            confidence: self.confidence,
        }
    }
}

impl Explanation {
    /// The breakdown as plain lines for a tooltip or export.
    pub fn lines(&self, hit: &CoupletHit) -> Vec<String> {
        let mut out = vec![format!(
            "Rotation couplet {:.0}%  ({})",
            self.confidence * 100.0,
            self.version
        )];
        // Human evidence sits above the radar score, not in it.
        if let Some(line) = hit.confirmation.describe() {
            out.push(line);
        }
        for r in &self.reasons {
            out.push(format!(
                "{:<9} {:>3.0}% x {:.0}%   {}",
                r.label,
                r.score * 100.0,
                r.weight * 100.0,
                r.detail
            ));
        }
        out.push(format!(
            "Range     x{:.2}   {:.0} km from the radar",
            self.range_factor, hit.range_km
        ));
        out.push(if hit.tilts > 1 {
            format!(
                "Vertical  {:.0}%   {} tilts, up to {:.1} km",
                self.vertical * 100.0,
                hit.tilts,
                hit.top_km
            )
        } else {
            format!(
                "Vertical  {:.0}%   one tilt only: capped at {:.0}%",
                self.vertical * 100.0,
                SINGLE_TILT_CAP * 100.0
            )
        });
        out
    }
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

/// The longest gap (ms) between two neighbouring radials of one pass. Adjacent bins are a fraction
/// of a second apart; a whole rotation is about ten seconds. Two seconds passes normal jitter and
/// a slow-scan cut, and rejects any pairing of different passes.
const MAX_NEIGHBOUR_GAP_MS: i64 = 2_000;

/// Whether bins `a` and `b` of `sweep` were scanned as neighbours. Sweeps built without timing
/// (every fixture, and any path that predates partial-sweep rendering) have no way to say, and are
/// taken at their word.
fn scanned_together(sweep: &BinnedSweep, a: usize, b: usize) -> bool {
    match (sweep.bin_time_ms.get(a), sweep.bin_time_ms.get(b)) {
        (Some(&ta), Some(&tb)) => ta != 0 && tb != 0 && (ta - tb).abs() <= MAX_NEIGHBOUR_GAP_MS,
        _ => true,
    }
}

/// Whether a gate pair is a fold that dealiasing left behind rather than shear.
///
/// Velocity wraps at the Nyquist velocity, so a field that failed to unfold still jumps from about
/// +Nyquist to about -Nyquist across the fold line: a difference of almost exactly two Nyquists
/// between two gates each near the limit. A live volume showed a hundred such "couplets" in one
/// sweep, every one a spread of 51 m/s on a 26.6 m/s Nyquist.
///
/// The tell is that both sides sit *at* the limit (0.75 to 1.1 Nyquists) with a difference of
/// 1.6 to 2.2 Nyquists. Real rotation strong enough to matter unfolds past the limit, on one side or
/// both: the 2013 Moore tornado reads about 50 m/s each way on a 26.6 m/s Nyquist, well clear of
/// this band, and must not be mistaken for a fold. `nyquist_ms` of 0 (unknown) never flags anything.
fn is_leftover_fold(a: f32, b: f32, nyquist_ms: f32) -> bool {
    let at_the_limit = |v: f32| (0.75 * nyquist_ms..=1.1 * nyquist_ms).contains(&v.abs());
    nyquist_ms > 5.0
        && at_the_limit(a)
        && at_the_limit(b)
        && ((1.6 * nyquist_ms)..=(2.2 * nyquist_ms)).contains(&(a - b).abs())
}

/// Whether a radial pair is a seam or a bad radial rather than rotation. A couplet is a compact
/// thing: a few gates along a radial, at one range. When a large share of everything comparable
/// along a radial pair qualifies at once, the two radials disagree along their whole length, which
/// is what a dealiasing failure, the edge of a partial sweep, interference or a radial with bad data
/// looks like, and it draws a straight line of false couplets out from the radar. `candidates` is
/// how many gate pairs qualified and `comparable` how many had data on both sides.
fn is_seam(candidates: usize, comparable: usize) -> bool {
    candidates >= 8 && candidates as f32 > 0.2 * comparable as f32
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
        let next = (az + 1) % vel.az_bins; // wraps 719 to 0
        let az_deg = (az as f64 + 0.5) * 360.0 / vel.az_bins as f64;
        // Two neighbouring bins that were not scanned together are not neighbours: a partial live
        // sweep leaves the previous rotation beside the current one, and any difference between
        // two passes reads as shear along that radial.
        if !scanned_together(vel, az, next) {
            continue;
        }
        // Candidates are held until the whole radial pair has been read, so a seam (see
        // `is_seam`) can be thrown out as one piece instead of one gate at a time.
        let mut row: Vec<(f64, f64, f32, f32, f32, f32)> = Vec::new();
        let mut comparable = 0usize;
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
            comparable += 1;
            let dv = (a - b).abs();
            if dv < g2g_min_ms || a * b >= 0.0 {
                continue; // too weak, or both gates on the same side of zero (not a couplet)
            }
            if is_leftover_fold(a, b, vel.nyquist_ms) {
                continue;
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
            row.push((lon, lat, range, a, b, dv));
        }
        if is_seam(row.len(), comparable) {
            continue;
        }
        for (lon, lat, range, a, b, dv) in row {
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
            // One tilt has no vertical evidence, so it never gets past the single-tilt cap;
            // `detect_volume` rescores the hit once it can see the column.
            let confidence = confidence(evidence(g2g, n as f32, range_km), 0.0).max(0.05);
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
                confirmation: crate::confirm::Confirmation::NONE,
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
    /// One grid cell's running totals: gates, sum_lon*w, sum_lat*w, sum_range*w, max_vrot,
    /// max_g2g, tilts, top_km. Named so the map's own type stays readable.
    type CellAccum = (usize, f64, f64, f64, f32, f32, usize, f32);
    let mut cells: HashMap<(i64, i64), CellAccum> = HashMap::new();

    for (vel, z) in sweeps {
        for h in detect(
            vel,
            z,
            g2g_min_ms,
            z_min,
            min_range_km,
            max_range_km,
            min_gates,
        ) {
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
        .map(
            |(gates, slon, slat, srange, vrot_ms, g2g_ms, tilts, top_km)| {
                let w = gates.max(1) as f64;
                // What the cluster itself shows (strength and size, faded by range), scaled by the
                // vertical evidence: a couplet that repeats through several tilts is an established
                // circulation, where a wide single-tilt patch is as often a gust front or a glitch.
                // Size is per tilt, so a tall column is not scored as a big patch.
                let per_tilt = gates as f32 / tilts.max(1) as f32;
                let range_km = (srange / w) as f32;
                let confidence = confidence(
                    evidence(g2g_ms, per_tilt, range_km),
                    vertical_term(top_km, tilts),
                );
                CoupletHit {
                    lon: slon / w,
                    lat: slat / w,
                    vrot_ms,
                    g2g_ms,
                    range_km,
                    gates,
                    tilts,
                    top_km,
                    confidence,
                    confirmation: crate::confirm::Confirmation::NONE,
                }
            },
        )
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
            ..Default::default()
        }
    }

    /// Like `couplet_sweep`, but at a chosen elevation and a deliberately small wedge (2 km
    /// radial, 2°/~0.5 km tangential at this range) so it lands inside one ~4 km cluster cell —
    /// `couplet_sweep`'s wider wedge straddles two, which is fine for tests that only ever check
    /// `hits[0]`, but would make a single-hit-per-tilt assumption wrong.
    fn couplet_sweep_tilt(
        elevation_deg: f32,
        inbound: f32,
        outbound: f32,
        background: f32,
    ) -> BinnedSweep {
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
            ..Default::default()
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
            ..Default::default()
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
        assert!(detect(
            &couplet_sweep(-8.0, 8.0, 0.0),
            &z,
            25.0,
            20.0,
            5.0,
            150.0,
            3
        )
        .is_empty());
        // Strong shear but both sides inbound: convergence, not rotation.
        assert!(detect(
            &couplet_sweep(-40.0, -5.0, -5.0),
            &z,
            25.0,
            20.0,
            5.0,
            150.0,
            3
        )
        .is_empty());
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
        assert!(
            hits.is_empty(),
            "no real echo behind the shear — not rotation"
        );
    }

    #[test]
    fn a_couplet_seen_through_two_tilts_scores_higher_than_one() {
        let one_tilt = [(
            couplet_sweep_tilt(0.5, -30.0, 30.0, 0.0),
            z_sweep(0.5, Some(45.0)),
        )];
        let two_tilts = [
            (
                couplet_sweep_tilt(0.5, -30.0, 30.0, 0.0),
                z_sweep(0.5, Some(45.0)),
            ),
            (
                couplet_sweep_tilt(1.5, -30.0, 30.0, 0.0),
                z_sweep(1.5, Some(45.0)),
            ),
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

    /// A dealiasing failure, the edge of a partial sweep or a bad radial pair makes two neighbouring
    /// radials disagree along their whole length. That is a straight line of "couplets" from the
    /// radar, and it used to draw hundreds of them across a live sweep.
    #[test]
    fn two_radials_that_disagree_along_their_whole_length_are_a_seam_not_rotation() {
        let (lo, hi) = Moment::Velocity.value_range();
        let idx = |v: f32| (2.0 + (v - lo) / (hi - lo) * 253.0).round() as u8;
        let mut seam = couplet_sweep_tilt(0.5, 0.0, 0.0, 20.0);
        for az in 360..720 {
            for g in 0..seam.gate_count {
                seam.data[az * seam.gate_count + g] = idx(-20.0);
            }
        }
        let hits = detect(&seam, &z_sweep(0.5, Some(45.0)), 25.0, 20.0, 5.0, 150.0, 3);
        assert!(hits.is_empty(), "{} false couplets on a seam", hits.len());
        // The rule, by itself: a lot of a radial qualifying is a seam, a compact patch is not.
        assert!(is_seam(150, 190));
        assert!(
            !is_seam(8, 190),
            "a real couplet is a few gates on one radial"
        );
        assert!(
            !is_seam(3, 5),
            "and a handful of candidates is never enough to call it"
        );
    }

    #[test]
    fn a_pair_jumping_between_the_two_nyquist_limits_is_a_leftover_fold() {
        let ny = 26.6;
        // The artifact on real data: about +26 beside about -26.
        assert!(is_leftover_fold(25.9, -25.7, ny));
        assert!(is_leftover_fold(-26.4, 26.0, ny));
        // The 2013 Moore tornado: about 50 m/s each way, past the limit on both sides.
        assert!(!is_leftover_fold(51.0, -51.0, ny));
        // One side over the limit and the other not is real rotation unfolding unevenly.
        assert!(!is_leftover_fold(40.0, -20.0, ny));
        // A modest couplet nowhere near the limit.
        assert!(!is_leftover_fold(15.0, -15.0, ny));
        // Unknown Nyquist: nothing is called a fold.
        assert!(!is_leftover_fold(25.9, -25.7, 0.0));
    }

    #[test]
    fn folded_pairs_are_dropped_only_when_the_sweep_knows_its_nyquist() {
        let mut folded = couplet_sweep_tilt(0.5, -26.0, 26.0, 0.0);
        let z = z_sweep(0.5, Some(45.0));
        // A 40 m/s floor keeps the fixture's 26 m/s flanks out, so only the jump between the limits is left.
        let run = |s: &BinnedSweep| detect(s, &z, 40.0, 20.0, 5.0, 150.0, 3).len();
        assert_eq!(run(&folded), 1, "no Nyquist known: taken as rotation");
        folded.nyquist_ms = 26.6;
        assert_eq!(
            run(&folded),
            0,
            "at the limit both ways: a fold that survived dealiasing"
        );
        let mut strong = couplet_sweep_tilt(0.5, -50.0, 50.0, 0.0);
        strong.nyquist_ms = 26.6;
        assert_eq!(run(&strong), 1, "a genuine strong couplet is kept");
    }

    #[test]
    fn a_real_couplet_survives_the_seam_guard() {
        let hits = detect(
            &couplet_sweep_tilt(0.5, -30.0, 30.0, 0.0),
            &z_sweep(0.5, Some(45.0)),
            25.0,
            20.0,
            5.0,
            150.0,
            3,
        );
        assert_eq!(hits.len(), 1);
    }

    /// Bins from different passes are not neighbours: a partial live sweep has the previous
    /// rotation beside the current one.
    #[test]
    fn radials_scanned_a_pass_apart_are_not_compared() {
        let s = couplet_sweep_tilt(0.5, -30.0, 30.0, 0.0);
        let z = z_sweep(0.5, Some(45.0));
        let run = |sweep: &BinnedSweep| detect(sweep, &z, 25.0, 20.0, 5.0, 150.0, 3).len();
        // Timed as one pass, ten milliseconds a radial: found.
        let mut together = s.clone();
        together.bin_time_ms = (0..720).map(|a| 1_000_000 + a as i64 * 10).collect();
        assert_eq!(run(&together), 1);
        // Every radial pair around the couplet straddles a pass boundary (its flanks quantise to a
        // little either side of zero, so the pairs beside the main one qualify too).
        let mut apart = together.clone();
        for a in (90..110).filter(|a| a % 2 == 1) {
            apart.bin_time_ms[a] -= 10_000;
        }
        assert_eq!(run(&apart), 0);
        // A bin no radial landed in (time 0) cannot be one half of a couplet either.
        let mut hole = together.clone();
        for a in 96..=104 {
            hole.bin_time_ms[a] = 0;
        }
        assert_eq!(run(&hole), 0);
        // No timing at all: taken at its word, as every fixture is.
        assert_eq!(run(&s), 1);
    }

    #[test]
    fn detect_volume_still_reports_a_lone_single_tilt_hit() {
        let hits = detect_volume(
            &[(
                couplet_sweep_tilt(0.5, -30.0, 30.0, 0.0),
                z_sweep(0.5, Some(45.0)),
            )],
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

#[cfg(test)]
mod scoring_tests {
    use super::*;

    fn hit(g2g_ms: f32, gates: usize, range_km: f32, tilts: usize, top_km: f32) -> CoupletHit {
        let per_tilt = gates as f32 / tilts as f32;
        CoupletHit {
            lon: -97.5,
            lat: 35.3,
            vrot_ms: g2g_ms / 2.0,
            g2g_ms,
            range_km,
            gates,
            tilts,
            top_km,
            confidence: confidence(
                evidence(g2g_ms, per_tilt, range_km),
                vertical_term(top_km, tilts),
            ),
            confirmation: crate::confirm::Confirmation::NONE,
        }
    }

    #[test]
    fn strength_size_and_range_each_move_the_score_the_right_way() {
        let base = evidence(30.0, 6.0, 30.0);
        assert!(evidence(36.0, 6.0, 30.0) > base, "stronger shear");
        assert!(evidence(30.0, 12.0, 30.0) > base, "a bigger patch");
        assert!(evidence(30.0, 6.0, 140.0) < base, "far away");
        assert_eq!(strength_term(25.0), 0.0);
        assert_eq!(strength_term(36.0), 1.0);
        assert_eq!(range_factor(60.0), 1.0);
        assert!((range_factor(150.0) - 0.6).abs() < 1e-6);
        assert!(evidence(80.0, 90.0, 10.0) <= 1.0);
    }

    #[test]
    fn one_tilt_never_passes_the_cap_however_strong_or_high() {
        // The same beam-height trap as debris: a far low tilt is high in the sky.
        let lone = hit(60.0, 40, 30.0, 1, 4.0);
        assert!(
            lone.confidence <= SINGLE_TILT_CAP + 1e-6,
            "{}",
            lone.confidence
        );
        assert_eq!(vertical_term(4.0, 1), 0.0);
        let column = hit(60.0, 120, 30.0, 3, 4.0);
        assert!(column.confidence > SINGLE_TILT_CAP);
    }

    #[test]
    fn a_far_uniform_weak_couplet_no_longer_reads_as_certain() {
        // The quiet-detector noise seen on real volumes: ~50 kt gate-to-gate, near the minimum
        // size, at 130 km. It used to score 100% from height and depth alone.
        let far = hit(25.7, 9, 130.0, 3, 3.5);
        assert!(far.confidence < 0.5, "{}", far.confidence);
        let near = hit(36.0, 45, 30.0, 3, 2.0);
        assert!(near.confidence > far.confidence + 0.3);
    }

    #[test]
    fn the_explanation_reproduces_the_confidence_and_reads_in_plain_lines() {
        let h = hit(32.0, 30, 45.0, 3, 2.2);
        let e = h.explain();
        assert!((e.confidence - h.confidence).abs() < 1e-6);
        let sum: f32 = e.reasons.iter().map(|r| r.score * r.weight).sum();
        assert!((sum * e.range_factor - e.evidence).abs() < 1e-5);
        assert!((e.reasons.iter().map(|r| r.weight).sum::<f32>() - 1.0).abs() < 1e-6);
        let rebuilt = confidence(e.evidence, e.vertical);
        assert!((rebuilt - h.confidence).abs() < 1e-5);
        let text = e.lines(&h).join("\n");
        for want in ["Strength", "Size", "Range", "Vertical", ALGORITHM_VERSION] {
            assert!(text.contains(want), "missing {want}: {text}");
        }
        let lone = hit(32.0, 10, 45.0, 1, 0.5);
        assert!(lone
            .explain()
            .lines(&lone)
            .join("\n")
            .contains("one tilt only"));
    }
}
