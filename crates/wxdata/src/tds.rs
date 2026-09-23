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
//!   in whichever single sweep sees clutter;
//! * and it is **rooted in the lowest sweep**, because debris is lofted from the ground. Low CC
//!   that starts in the middle tilts over a clean one beneath is hail or a melting layer aloft
//!   (see [`UNROOTED_FACTOR`]).
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

/// Gates nearer the radar than this (km) are not read. Right at the tower the beam is in the
/// clutter, the sidelobes and the cone of silence, and low CC in strong echo there is static, not
/// debris.
pub const MIN_RANGE_KM: f32 = 3.0;

/// From [`MIN_RANGE_KM`] out to here the clutter risk fades: the score is halved at the edge of the
/// blind zone and full strength by this range.
pub const CLUTTER_RANGE_KM: f32 = 15.0;

/// Hits from different tilts closer than this (km) are the same column.
const ASSOCIATE_KM: f64 = 3.0;

/// Elevation angles at or below this (degrees) are the low-level tilts lofted debris has to appear
/// in. Every WSR-88D VCP starts at 0.5 deg with a second cut near 0.9 deg, so a volume scanned
/// normally always has one.
pub const LOW_TILT_MAX_DEG: f32 = 1.5;

/// The factor applied to the vertical evidence of a column that skips the lowest low-level tilt
/// scanned.
///
/// Debris is lofted *from the ground*: a real debris ball is rooted in the lowest sweep and thins
/// upward. A low-CC column that lives in the middle tilts with a clean sweep beneath it is the
/// other thing entirely -- wet or melting hail aloft, or a brightband layer -- which after high
/// ZDR is the commonest structure a CC-and-Z detector mistakes for debris. It discounts rather
/// than rejects, since the lowest beam can be blocked by terrain, attenuated on its way through
/// the core, or looking under the debris at close range.
pub const UNROOTED_FACTOR: f32 = 0.7;

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
    /// Beam-center height (km AGL) of the *lowest* tilt contributing to this hit -- where the
    /// column starts, against `top_km`'s where it ends. Equal to `top_km` out of [`detect`],
    /// which only ever sees one tilt.
    pub base_km: f32,
    /// Whether the column reaches the lowest low-level tilt the volume scanned (see
    /// [`LOW_TILT_MAX_DEG`] and [`UNROOTED_FACTOR`]). `None` when no low tilt was read, so
    /// nothing can be said either way; [`detect`] always reports `None`, since one tilt cannot
    /// know what the tilts under it show.
    pub rooted: Option<bool>,
    /// Rotational velocity (m/s) of the strongest rotation couplet within
    /// [`ROTATION_ASSOCIATE_KM`], once [`corroborate_with_rotation`] has been run. `None` means no
    /// rotation was found near this hit (or it was never checked); it is not evidence against it.
    pub rotation_ms: Option<f32>,
    /// Mean differential reflectivity (dB) around the hit, once [`apply_zdr`] has been run. Debris
    /// is randomly oriented, so it reads near 0 dB; a high mean is rain or large drops with low CC
    /// from mixing, not debris. `None` when there was no ZDR to read.
    pub zdr_db: Option<f32>,
    /// What people and forecasters said about it: a tornado report near it, or an observed
    /// tornado warning over it. Separate from `confidence`, which is radar alone and stops at 100%;
    /// see [`crate::confirm`]. Set by the caller, never by the detector.
    pub confirmation: crate::confirm::Confirmation,
    /// 0..1 confidence, from the evidence above and the vertical continuity. A single tilt has no
    /// vertical evidence, so [`detect`] never reports more than [`SINGLE_TILT_CAP`]; a hit that
    /// repeats up through the tilts earns the rest. Rotation nearby ([`corroborate_with_rotation`])
    /// can raise it past the cap: a separate line of evidence, not more of the same.
    pub confidence: f32,
}

/// A couplet this close to a debris signature (km, ground distance) is read as the same storm's
/// circulation. A debris ball sits in or beside the strongest low-level rotation, within a few km.
pub const ROTATION_ASSOCIATE_KM: f64 = 5.0;

/// Rotation only corroborates a hit within this range (km), and only couplets within it count. Past
/// it the beam is wide enough that neither a couplet nor a debris ball is well resolved, and the
/// velocity data is where dealiasing failures cluster: a swarm of near-identical couplets at
/// 130-150 km, each about twice the Nyquist velocity, is an artifact, not a set of tornadoes.
pub const ROTATION_MAX_RANGE_KM: f32 = 100.0;

/// A couplet must itself score at least this (see [`crate::rotation`]) to corroborate a debris
/// signature. The rotation detector's own noise, mostly weak far-out shear, scores below it.
pub const CORROBORATING_COUPLET_CONFIDENCE: f32 = 0.35;

/// Whether a rotation couplet at `range_km` with `confidence` is credible enough to corroborate a
/// debris signature: close enough to the radar to be resolved ([`ROTATION_MAX_RANGE_KM`]) and
/// confident enough in its own right.
pub fn couplet_corroborates(range_km: f32, confidence: f32) -> bool {
    range_km <= ROTATION_MAX_RANGE_KM && confidence >= CORROBORATING_COUPLET_CONFIDENCE
}

/// A debris signature must itself score at least this (its own reading, before corroboration) to
/// corroborate a rotation couplet. Mirrors [`CORROBORATING_COUPLET_CONFIDENCE`] from the other
/// side: a marginal low-CC patch is not debris, so it should not be read as confirmation of
/// anything.
pub const CORROBORATING_DEBRIS_CONFIDENCE: f32 = 0.40;

/// Whether a debris signature at `range_km` with `confidence` is credible enough to corroborate a
/// rotation couplet: close enough to be resolved ([`ROTATION_MAX_RANGE_KM`], the same range past
/// which neither a couplet nor a debris ball is well resolved) and confident enough in its own
/// right. Mirrors [`couplet_corroborates`] from the other side.
pub fn debris_corroborates(range_km: f32, confidence: f32) -> bool {
    range_km <= ROTATION_MAX_RANGE_KM && confidence >= CORROBORATING_DEBRIS_CONFIDENCE
}

/// Rotation may only corroborate a hit that already stands on its own at this confidence. It is a
/// second line of evidence for a credible detection, not a way to promote a marginal one.
pub const ROTATION_MIN_CONFIDENCE: f32 = 0.5;

/// The share of the remaining gap to 1 that a full-strength couplet closes.
const ROTATION_GAP_SHARE: f32 = 0.4;

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
    let t = Terms::of(min_cc, mean_cc, mean_z, area_km2, contrast, range_km);
    ((0.30 * t.depth + 0.30 * t.contrast + 0.20 * t.core + 0.20 * t.size) * t.range).clamp(0.0, 1.0)
}

/// The scored terms behind [`evidence`], each 0..1, kept apart so a detection can be explained
/// with exactly the numbers that produced its score.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Terms {
    depth: f32,
    contrast: f32,
    core: f32,
    size: f32,
    range: f32,
}

impl Terms {
    fn of(
        min_cc: f32,
        mean_cc: f32,
        mean_z: f32,
        area_km2: f32,
        contrast: Option<f32>,
        range_km: f32,
    ) -> Terms {
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
        // Far out the beam is wide and a ball is a few gates; up close, ground clutter, the tower
        // and wind turbines and their sidelobes give low CC in strong echo of their own. Both
        // discount the whole score, and the nearest gates are not read at all (`MIN_RANGE_KM`).
        let far = 1.0 - 0.3 * ((range_km - 60.0) / 90.0).clamp(0.0, 1.0);
        let near = 0.5
            + 0.5 * ((range_km - MIN_RANGE_KM) / (CLUTTER_RANGE_KM - MIN_RANGE_KM)).clamp(0.0, 1.0);
        let range = far * near;
        Terms {
            depth,
            contrast,
            core,
            size,
            range,
        }
    }
}

/// Vertical evidence, 0..1: how many tilts show the hit, how high the column reaches, and whether
/// it is rooted in the lowest tilt scanned. About three tilts and 3 km AGL each saturate their
/// half — a debris signature repeating through the lowest three or four elevation angles is about
/// as convincing as this heuristic gets — and a column that skips the lowest low-level tilt keeps
/// only [`UNROOTED_FACTOR`] of what it earned.
///
/// Height only counts when more than one tilt confirms the signature there. At long range even the
/// lowest beam is kilometres above the ground purely from the beam's geometry, so a lone hit's
/// `top_km` says where the beam was, not that debris was lofted; crediting it would rank a distant
/// four-gate speck above a nearby one for no reason. And it is absolute, not "what share of the
/// tilts a caller happened to check": a caller that only looks at the lowest tilt must not make a
/// lone hit read as the whole column.
///
/// `rooted` is `None` when the caller read no low tilt at all, and then nothing is deducted: a
/// column cannot be faulted for missing a sweep nobody looked at.
pub fn vertical_term(top_km: f32, tilts: usize, rooted: Option<bool>) -> f32 {
    if tilts < 2 {
        return 0.0;
    }
    let height = (top_km / 3.0).clamp(0.0, 1.0);
    let depth = ((tilts as f32 - 1.0) / 2.0).clamp(0.0, 1.0);
    let rooting = if rooted == Some(false) {
        UNROOTED_FACTOR
    } else {
        1.0
    };
    (0.5 * height + 0.5 * depth) * rooting
}

/// Which revision of the scoring produced a hit. Bump it whenever a weight, threshold or rule above
/// changes, so a saved or exported detection says what logic scored it.
pub const ALGORITHM_VERSION: &str = "tds-5";

/// One scored piece of evidence behind a detection.
#[derive(Debug, Clone, PartialEq)]
pub struct Reason {
    pub label: &'static str,
    /// The measurement, in words.
    pub detail: String,
    /// How well it supports a debris signature, 0..1.
    pub score: f32,
    /// Its share of the evidence, 0..1 (the weights sum to 1).
    pub weight: f32,
}

/// Why a [`TdsHit`] scored what it did: every term with its measurement, then each stage that
/// turned the evidence into the final confidence. Built by [`TdsHit::explain`] from the hit's own
/// fields with the same functions that scored it, so it cannot drift from the number on the map.
#[derive(Debug, Clone, PartialEq)]
pub struct Explanation {
    pub version: &'static str,
    /// The four weighted terms, in weight order.
    pub reasons: Vec<Reason>,
    /// Multiplier for range (1 inside 60 km, down to 0.7 at 150 km).
    pub range_factor: f32,
    /// The weighted terms times the range factor.
    pub evidence: f32,
    /// Vertical continuity, 0..1; 0 for a single tilt.
    pub vertical: f32,
    /// Mean ZDR (dB) around the hit and the factor it applied to the score, when it was read.
    pub zdr: Option<(f32, f32)>,
    /// Evidence times the vertical factor (0.6 to 1), times any ZDR factor: the confidence before
    /// rotation.
    pub base_confidence: f32,
    /// What rotation beside the hit added, if it counted.
    pub rotation_gain: Option<f32>,
    pub confidence: f32,
}

impl TdsHit {
    /// Break this hit's confidence down into the evidence behind it.
    pub fn explain(&self) -> Explanation {
        let t = Terms::of(
            self.min_cc,
            self.mean_cc,
            self.mean_z,
            self.area_km2,
            self.contrast,
            self.range_km,
        );
        let low = 0.5 * self.min_cc + 0.5 * self.mean_cc;
        let contrast_detail = match self.contrast {
            Some(c) => format!("surroundings {c:.2} higher in CC"),
            None => "no surroundings read, scored neutral".to_string(),
        };
        let reasons = vec![
            Reason {
                label: "Depth",
                detail: format!(
                    "CC down to {:.2}, typically {low:.2} across {} gates",
                    self.min_cc, self.gates
                ),
                score: t.depth,
                weight: 0.30,
            },
            Reason {
                label: "Contrast",
                detail: contrast_detail,
                score: t.contrast,
                weight: 0.30,
            },
            Reason {
                label: "Core",
                detail: format!("mean {:.0} dBZ, peak {:.0} dBZ", self.mean_z, self.max_z),
                score: t.core,
                weight: 0.20,
            },
            Reason {
                label: "Size",
                detail: format!("{:.1} km\u{b2} footprint", self.area_km2),
                score: t.size,
                weight: 0.20,
            },
        ];
        let evidence = evidence(
            self.min_cc,
            self.mean_cc,
            self.mean_z,
            self.area_km2,
            self.contrast,
            self.range_km,
        );
        let vertical = vertical_term(self.top_km, self.tilts, self.rooted);
        let zdr = self.zdr_db.map(|d| (d, zdr_factor(d)));
        let base_confidence = confidence(evidence, vertical) * zdr.map_or(1.0, |(_, f)| f);
        let rotation_gain = self
            .rotation_ms
            .map(|_| (self.confidence - base_confidence).max(0.0));
        Explanation {
            version: ALGORITHM_VERSION,
            reasons,
            range_factor: t.range,
            evidence,
            vertical,
            zdr,
            base_confidence,
            rotation_gain,
            confidence: self.confidence,
        }
    }
}

impl Explanation {
    /// The breakdown as plain lines for a tooltip or export.
    pub fn lines(&self, hit: &TdsHit) -> Vec<String> {
        let mut out = vec![format!(
            "Debris signature {:.0}%  ({})",
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
                "Vertical  {:.0}%   {} tilts, {:.1}-{:.1} km{}",
                self.vertical * 100.0,
                hit.tilts,
                hit.base_km,
                hit.top_km,
                match hit.rooted {
                    // The one worth spelling out: the column is aloft, so it kept only
                    // `UNROOTED_FACTOR` of the vertical evidence it would otherwise have earned.
                    Some(false) => ", not rooted in the lowest tilt",
                    Some(true) => ", rooted in the lowest tilt",
                    None => "",
                }
            )
        } else {
            format!(
                "Vertical  0%   one tilt only: capped at {:.0}%",
                SINGLE_TILT_CAP * 100.0
            )
        });
        if let Some((d, f)) = self.zdr {
            out.push(format!(
                "ZDR       x{f:.2}   mean {d:.1} dB around it (debris is near 0)"
            ));
        }
        match (hit.rotation_ms, self.rotation_gain) {
            (Some(v), Some(g)) => out.push(format!(
                "Rotation  +{:.0} points   {:.0} kt couplet within {:.0} km",
                g * 100.0,
                v * 1.943_844,
                ROTATION_ASSOCIATE_KM
            )),
            _ => out.push("Rotation  none counted (not evidence against it)".to_string()),
        }
        out
    }
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
            if range < MIN_RANGE_KM {
                continue;
            }
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
            base_km: top_km,
            rooted: None,
            rotation_ms: None,
            zdr_db: None,
            confirmation: crate::confirm::Confirmation::NONE,
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

/// How much a rotation couplet of `vrot_ms` corroborates a debris signature, 0..1. Rotational
/// velocity under 10 m/s is ordinary storm-scale shear and counts for nothing; 35 m/s (about 68 kt),
/// a strong low-level circulation, counts for everything.
pub fn rotation_term(vrot_ms: f32) -> f32 {
    ((vrot_ms - 10.0) / 25.0).clamp(0.0, 1.0)
}

/// Raise the confidence of debris signatures that have a rotation couplet beside them, and record
/// that rotation on the hit. `couplets` are `(lon, lat, vrot_ms)`.
///
/// Debris lofted by a tornado sits in or beside the strong low-level rotation that made it, while
/// hail and biological scatter carry no such circulation, so a couplet nearby is the strongest
/// single piece of corroboration there is, and independent of the correlation-coefficient evidence.
/// It moves the confidence up to 40% of the remaining way to 1 (a full-strength couplet on a 60%
/// hit gives 76%), so it lifts a good hit without letting rotation alone make a weak one certain.
///
/// Three limits keep it from amplifying noise. Only hits within [`ROTATION_MAX_RANGE_KM`] are
/// considered, and the caller should pass only couplets within it too; a hit below
/// [`ROTATION_MIN_CONFIDENCE`] is left alone, since corroboration cannot rescue a marginal
/// detection; and `rotation_ms` is recorded only when the rotation actually counted.
///
/// The absence of a couplet changes nothing: the rotation may be undetected, out of the velocity
/// data's range (a tornado close to the radar is inside the rotation detector's own minimum range),
/// or on a tilt not scanned, and a debris ball is often seen before its couplet is. Hits come back
/// strongest first.
pub fn corroborate_with_rotation(hits: &mut [TdsHit], couplets: &[(f64, f64, f32)]) {
    for h in hits.iter_mut() {
        h.rotation_ms = None;
        if h.range_km > ROTATION_MAX_RANGE_KM || h.confidence < ROTATION_MIN_CONFIDENCE {
            continue;
        }
        let strongest = couplets
            .iter()
            .filter(|(lon, lat, _)| {
                ground_km((h.lon, h.lat), (*lon, *lat)) <= ROTATION_ASSOCIATE_KM
            })
            .map(|(_, _, v)| *v)
            .filter(|v| v.is_finite())
            .fold(None, |best: Option<f32>, v| {
                Some(best.map_or(v, |b| b.max(v)))
            });
        h.rotation_ms = strongest;
        if let Some(v) = strongest {
            let gap = 1.0 - h.confidence;
            h.confidence =
                (h.confidence + gap * ROTATION_GAP_SHARE * rotation_term(v)).clamp(0.0, 1.0);
        }
    }
    hits.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
}

/// Corroborate debris signatures and rotation couplets from each other, safely.
///
/// [`corroborate_with_rotation`] raises a debris signature's confidence from a nearby couplet, and
/// [`crate::rotation::corroborate_with_debris`] does the reverse. Calling both by hand risks a
/// feedback loop: if a couplet already boosted by debris is then used to decide whether it, in
/// turn, boosts that debris signature, the same evidence gets counted twice under two different
/// names. This snapshots each side's own confidence *before* either function runs, so both
/// directions always corroborate from raw, single-source evidence -- exactly what
/// [`corroborate_with_rotation`] and [`crate::rotation::corroborate_with_debris`] each already
/// assume of the slice the other side hands them.
///
/// Both `tds_hits` and `rot_hits` should be raw (uncorroborated from either direction) on the way
/// in; corroboration cannot be re-run to layer on more of the same evidence. Both come back
/// strongest first, per hit type.
pub fn cross_corroborate(tds_hits: &mut [TdsHit], rot_hits: &mut [crate::rotation::CoupletHit]) {
    // Every couplet and debris signature that clears the *other* function's own floor and range,
    // read from confidence as it stood before either direction ran.
    let raw_couplets: Vec<(f64, f64, f32)> = rot_hits
        .iter()
        .filter(|c| couplet_corroborates(c.range_km, c.confidence))
        .map(|c| (c.lon, c.lat, c.vrot_ms))
        .collect();
    let raw_debris: Vec<(f64, f64, f32)> = tds_hits
        .iter()
        .filter(|h| debris_corroborates(h.range_km, h.confidence))
        .map(|h| (h.lon, h.lat, h.confidence))
        .collect();
    corroborate_with_rotation(tds_hits, &raw_couplets);
    crate::rotation::corroborate_with_debris(rot_hits, &raw_debris);
}

/// Ground distance in km between two lon/lat points, good over the few km hits are compared at.
pub(crate) fn ground_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let mid_lat = ((a.1 + b.1) * 0.5).to_radians();
    let dx = (b.0 - a.0) * mid_lat.cos() * 111.32;
    let dy = (b.1 - a.1) * 110.57;
    dx.hypot(dy)
}

/// The core of one associated group: its strongest member (by `strength`) and every member within
/// `radius_km` *of that one*, strongest first. A detection's position and evidence come from its
/// core, not from everything single linkage chained into the group.
///
/// That matters for debris: a violent tornado's debris ball can sit at the edge of a large field
/// of weak, fragmented low-CC echo, and linkage at 3 km steps walks from the ball through every
/// fragment. On Mayfield, KY (KPAH, 11 Dec 2021, 03:30Z) that made one column of 160 per-tilt
/// hits spread over 25 km, whose gate-weighted centroid sat 12 km north of the tornado -- so its
/// averaged CC and Z were diluted by the fragments, and it was never paired with its own 72 kt
/// couplet 3 km from the real ball. The group itself stays one detection (splitting it would
/// report every fragment as a debris signature of its own); only what it says changes.
pub(crate) fn strongest_core(
    members: &[usize],
    points: &[(f64, f64)],
    strength: &[f64],
    radius_km: f64,
) -> Vec<usize> {
    let mut order = members.to_vec();
    order.sort_by(|&a, &b| strength[b].total_cmp(&strength[a]));
    let Some(&anchor) = order.first() else {
        return order;
    };
    order
        .into_iter()
        .filter(|&j| ground_km(points[anchor], points[j]) <= radius_km)
        .collect()
}

/// Group `points` so that any two within `radius_km` of one another land in the same group
/// (single-linkage union-find), returned as lists of indices into `points`.
///
/// Association by ground distance, not by snapping to a grid. One feature seen from two tilts sits
/// a kilometre or two apart, and a grid splits that pair whenever it happens to straddle a cell
/// edge — turning one column into two single-tilt hits, each capped at its detector's single-tilt
/// cap, and losing exactly the vertical evidence a volume pass exists to find. Shared with
/// [`crate::rotation::detect_volume`], which associates couplets the same way. Debris columns then
/// describe each group by its [`strongest_core`], since linkage can chain.
pub(crate) fn associate_by_ground(points: &[(f64, f64)], radius_km: f64) -> Vec<Vec<usize>> {
    let n = points.len();
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
            if ground_km(points[i], points[j]) <= radius_km {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[rj] = ri;
                }
            }
        }
    }
    // Keyed by root so the grouping is deterministic, which keeps the output order stable for
    // callers that sort by confidence and then print.
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..n {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }
    groups.into_values().collect()
}

/// The factor mean ZDR applies to a debris score: 1 up to 1 dB, falling to 0.6 by 3.5 dB.
///
/// Tornadic debris is a jumble of random shapes and orientations, so its ZDR sits near 0 dB. Low CC
/// beside a high ZDR is a mixture of raindrops and large drops or wet hail, or a melting layer, and
/// is the commonest thing a CC-only detector mistakes for a debris ball. It only ever discounts: a
/// near-zero ZDR is shared with dry hail, so it is no proof of debris and adds nothing.
pub fn zdr_factor(mean_db: f32) -> f32 {
    1.0 - 0.4 * ((mean_db - 1.0) / 2.5).clamp(0.0, 1.0)
}

/// Mean of a sweep's decoded values within `radius_km` of a point, or `None` with fewer than four
/// gates of data there.
fn mean_around(sweep: &BinnedSweep, lon: f64, lat: f64, radius_km: f32) -> Option<f32> {
    if sweep.az_bins == 0 || sweep.gate_count == 0 || sweep.gate_interval_km <= 0.0 {
        return None;
    }
    let (rlon, rlat) = (f64::from(sweep.radar_lon), f64::from(sweep.radar_lat));
    let east = (lon - rlon) * ((lat + rlat) * 0.5).to_radians().cos() * 111.32;
    let north = (lat - rlat) * 110.57;
    let range = east.hypot(north) as f32;
    let az = east.atan2(north).to_degrees().rem_euclid(360.0);
    let bin_deg = 360.0 / sweep.az_bins as f64;
    let az0 = (az / bin_deg) as i64;
    // Half-width in bins of the azimuth arc the radius covers at this range, and in gates.
    let arc_km = (f64::from(range) * bin_deg.to_radians()).max(0.05);
    let d_az = ((f64::from(radius_km) / arc_km).ceil() as i64).clamp(1, 24);
    let g0 = ((range - sweep.first_gate_km) / sweep.gate_interval_km).round() as i64;
    let d_g = ((radius_km / sweep.gate_interval_km).ceil() as i64).clamp(1, 24);
    let (mut sum, mut n) = (0.0f32, 0usize);
    for da in -d_az..=d_az {
        let a = (az0 + da).rem_euclid(sweep.az_bins as i64) as usize;
        for dg in -d_g..=d_g {
            let g = g0 + dg;
            if g < 0 || g as usize >= sweep.gate_count {
                continue;
            }
            if let Some(v) = decode(sweep, sweep.data[a * sweep.gate_count + g as usize]) {
                sum += v;
                n += 1;
            }
        }
    }
    (n >= 4).then(|| sum / n as f32)
}

/// Read the differential reflectivity around each hit from the lowest ZDR sweep and discount the
/// hits whose surroundings are not ZDR-neutral (see [`zdr_factor`]). Records the mean on the hit so
/// [`TdsHit::explain`] can show it, and returns the hits strongest first. Run it before
/// [`corroborate_with_rotation`], which adds to the confidence this leaves.
pub fn apply_zdr(hits: &mut [TdsHit], zdr: &[BinnedSweep]) {
    let Some(low) = zdr
        .iter()
        .filter(|s| s.moment == Moment::DifferentialReflectivity)
        .min_by(|a, b| a.elevation_deg.total_cmp(&b.elevation_deg))
    else {
        return;
    };
    for h in hits.iter_mut() {
        h.zdr_db = None;
        // About the footprint of the hit, at least a kilometre so a small ball still has neighbours.
        let radius = ((h.area_km2 / std::f32::consts::PI).sqrt() + 0.5).clamp(1.0, 3.0);
        if let Some(mean) = mean_around(low, h.lon, h.lat, radius) {
            h.zdr_db = Some(mean);
            h.confidence *= zdr_factor(mean);
        }
    }
    hits.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
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
    // The lowest low-level tilt in the volume, found by elevation angle and not by position in
    // the slice (which `sweeps` is explicitly not required to order). A column that misses it is
    // low CC aloft rather than lofted debris; see `UNROOTED_FACTOR`. With no low tilt read at all
    // there is nothing to miss, and rooting stays unknown.
    let low_tilt = sweeps
        .iter()
        .enumerate()
        .filter(|(_, (_, cc))| cc.elevation_deg <= LOW_TILT_MAX_DEG)
        .min_by(|a, b| a.1 .1.elevation_deg.total_cmp(&b.1 .1.elevation_deg))
        .map(|(i, _)| i);

    let mut per_tilt: Vec<(usize, TdsHit)> = Vec::new();
    for (tilt, (z, cc)) in sweeps.iter().enumerate() {
        for h in detect(z, cc, cc_max, z_min, max_range_km, min_gates) {
            per_tilt.push((tilt, h));
        }
    }

    // Group hits by ground distance, so two tilts' views of one ball merge no matter where an
    // arbitrary grid would have put the boundary; then describe each group by its strongest core,
    // so a ball at the edge of a field of weak fragments is not averaged into it (see
    // `strongest_core`).
    let centres: Vec<(f64, f64)> = per_tilt.iter().map(|(_, h)| (h.lon, h.lat)).collect();
    let strength: Vec<f64> = per_tilt
        .iter()
        .map(|(_, h)| f64::from(h.confidence) + h.gates as f64 * 1e-6)
        .collect();
    let mut out: Vec<TdsHit> = associate_by_ground(&centres, ASSOCIATE_KM)
        .into_iter()
        .map(|group| strongest_core(&group, &centres, &strength, ASSOCIATE_KM))
        .map(|members| {
            let hits: Vec<&TdsHit> = members.iter().map(|&i| &per_tilt[i].1).collect();
            let contributing: HashSet<usize> = members.iter().map(|&i| per_tilt[i].0).collect();
            let rooted = low_tilt.map(|t| contributing.contains(&t));
            let tilts = contributing.len();
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
            let base_km = hits.iter().map(|h| h.base_km).fold(f32::MAX, f32::min);
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
                base_km,
                rooted,
                rotation_ms: None,
                zdr_db: None,
                confirmation: crate::confirm::Confirmation::NONE,
                confidence: confidence(ev, vertical_term(top_km, tilts, rooted)),
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
        assert_eq!(vertical_term(6.0, 1, None), 0.0);
        assert!(
            vertical_term(6.0, 2, None) > 0.5,
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

    /// Debris is lofted from the ground, so a low-CC column that starts in the middle tilts with
    /// a clean sweep beneath it is hail or a melting layer aloft. It is still reported -- the
    /// lowest beam can be blocked or attenuated -- but it keeps less of its vertical evidence.
    #[test]
    fn a_column_that_skips_the_lowest_tilt_keeps_less_of_its_vertical_evidence() {
        // The rule itself: same height, same tilt count, only the rooting differs.
        let anchored = vertical_term(2.0, 3, Some(true));
        let aloft = vertical_term(2.0, 3, Some(false));
        assert!(aloft < anchored, "{aloft} vs {anchored}");
        assert!((aloft / anchored - UNROOTED_FACTOR).abs() < 1e-6);
        // And nothing is deducted when no low tilt was read at all, so there is nothing to miss.
        assert_eq!(vertical_term(2.0, 3, None), anchored);

        // End to end: the same two tilts of debris, once with a clean 0.5 deg sweep under them.
        let rooted = detect_volume(
            &[debris_pair(0.5, 100..104), debris_pair(1.4, 100..104)],
            0.80,
            40.0,
            150.0,
            4,
        );
        assert_eq!(rooted.len(), 1);
        assert_eq!(rooted[0].rooted, Some(true));
        let unrooted = detect_volume(
            &[
                // An empty hot wedge is a clean sweep: high CC, weak echo, nothing to flag.
                debris_pair(0.5, 100..100),
                debris_pair(1.4, 100..104),
                debris_pair(2.4, 100..104),
            ],
            0.80,
            40.0,
            150.0,
            4,
        );
        assert_eq!(
            unrooted.len(),
            1,
            "the two tilts aloft are still one column"
        );
        assert_eq!(
            unrooted[0].rooted,
            Some(false),
            "the lowest tilt was scanned and showed nothing"
        );
        // With no low tilt in the volume at all, rooting is unknown rather than false.
        let no_low = detect_volume(
            &[debris_pair(1.8, 100..104), debris_pair(2.4, 100..104)],
            0.80,
            40.0,
            150.0,
            4,
        );
        assert_eq!(no_low.len(), 1);
        assert_eq!(no_low[0].rooted, None);
    }

    /// `base_km` says where the column starts and `top_km` where it ends; a single tilt is both.
    #[test]
    fn a_column_reports_the_span_it_covers_not_just_its_top() {
        let column = detect_volume(
            &[debris_pair(0.5, 100..104), debris_pair(2.4, 100..104)],
            0.80,
            40.0,
            150.0,
            4,
        );
        assert_eq!(column.len(), 1);
        assert!(
            column[0].base_km < column[0].top_km,
            "{} to {}",
            column[0].base_km,
            column[0].top_km
        );
        let (z, cc) = debris_pair(0.5, 100..104);
        let lone = detect(&z, &cc, 0.80, 40.0, 150.0, 4);
        assert!(!lone.is_empty());
        assert_eq!(lone[0].base_km, lone[0].top_km);
    }

    /// The rotation detector's association: everything within the radius of a neighbour joins
    /// one group, however the points would have fallen on a grid.
    #[test]
    fn ground_association_is_single_linkage_and_indifferent_to_grid_edges() {
        // Three points 2 km apart in a chain, and one 50 km away.
        let pts = [
            (-97.5, 35.0),
            (-97.5, 35.018),
            (-97.5, 35.036),
            (-97.5, 35.5),
        ];
        let mut groups = associate_by_ground(&pts, 3.0);
        groups.sort_by_key(|g| g.len());
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0], vec![3]);
        assert_eq!(groups[1], vec![0, 1, 2], "a chain is one group");
        // Tighter than every gap: four groups of one.
        assert_eq!(associate_by_ground(&pts, 0.5).len(), 4);
        assert!(associate_by_ground(&[], 3.0).is_empty());
    }

    /// A debris column is described by its core: the strongest member and what is within the
    /// radius of it, however far linkage chained the rest of the group.
    #[test]
    fn a_group_is_described_by_its_strongest_core() {
        // Four points 2 km apart in a line (one linked chain), strongest at one end.
        let pts = [
            (-97.5, 35.0),
            (-97.5, 35.018),
            (-97.5, 35.036),
            (-97.5, 35.054),
        ];
        let strength = [0.9, 0.2, 0.3, 0.1];
        assert_eq!(
            associate_by_ground(&pts, 3.0).len(),
            1,
            "linkage chains them"
        );
        assert_eq!(
            strongest_core(&[0, 1, 2, 3], &pts, &strength, 3.0),
            vec![0, 1]
        );
        // Strongest in the middle: both neighbours are within reach, the far end is not.
        let middle = [0.2, 0.9, 0.3, 0.1];
        assert_eq!(
            strongest_core(&[0, 1, 2, 3], &pts, &middle, 3.0),
            vec![1, 2, 0]
        );
        assert!(strongest_core(&[], &pts, &strength, 3.0).is_empty());
    }

    /// Paint `v` into a sweep's azimuth x gate box.
    fn paint(
        s: &mut BinnedSweep,
        az: std::ops::Range<usize>,
        gates: std::ops::Range<usize>,
        v: u8,
    ) {
        for a in az {
            for g in gates.clone() {
                s.data[a * s.gate_count + g] = v;
            }
        }
    }

    /// Mayfield, KY in miniature: a strong debris ball at the edge of a long field of weak,
    /// fragmented low-CC echo. The column has to stay where the ball is -- chained into the
    /// fragments, its centroid slid 12 km away on the real volume and never met its couplet.
    #[test]
    fn a_debris_ball_beside_a_field_of_weak_fragments_keeps_its_own_position() {
        let gates = 100..108; // ~27 km out, past the clutter ramp
                              // An empty hot wedge is a clean sweep: the ball is painted in below.
        let (mut z0, mut cc0) = debris_pair(0.5, 100..100);
        paint(
            &mut cc0,
            100..106,
            gates.clone(),
            idx(Moment::CorrelationCoefficient, 0.35),
        );
        paint(
            &mut z0,
            100..106,
            gates.clone(),
            idx(Moment::Reflectivity, 58.0),
        );
        // The same ball alone, for where it should be reported.
        let alone = detect_volume(&[(z0.clone(), cc0.clone())], 0.80, 40.0, 150.0, 4);
        assert_eq!(alone.len(), 1);
        let ball = (alone[0].lon, alone[0].lat);

        // Sixteen weak fragments stepping away from it, 1.4 km apart, gaps of clean echo between.
        let (mut z, mut cc) = (z0, cc0);
        for k in 0..16 {
            let az = 110 + 6 * k..114 + 6 * k;
            paint(
                &mut cc,
                az.clone(),
                gates.clone(),
                idx(Moment::CorrelationCoefficient, 0.72),
            );
            paint(&mut z, az, gates.clone(), idx(Moment::Reflectivity, 45.0));
        }
        let hits = detect_volume(&[(z, cc)], 0.80, 40.0, 150.0, 4);
        // Still one detection -- the fragments are not each a debris signature -- but it is where
        // the ball is, and it reads the ball's CC rather than the fragments' average.
        assert_eq!(hits.len(), 1);
        let best = &hits[0];
        let off = ground_km((best.lon, best.lat), ball);
        assert!(off < 1.0, "strongest hit is {off:.1} km from the ball");
        assert!(
            best.min_cc < 0.4,
            "and it is the ball: min CC {}",
            best.min_cc
        );
    }

    fn hit_at(lon: f64, lat: f64, confidence: f32) -> TdsHit {
        TdsHit {
            lon,
            lat,
            gates: 20,
            min_cc: 0.4,
            mean_cc: 0.5,
            mean_z: 50.0,
            max_z: 55.0,
            area_km2: 3.0,
            contrast: Some(0.2),
            range_km: 30.0,
            tilts: 1,
            top_km: 0.5,
            base_km: 0.5,
            rooted: None,
            rotation_ms: None,
            zdr_db: None,
            confirmation: crate::confirm::Confirmation::NONE,
            confidence,
        }
    }

    #[test]
    fn rotation_beside_a_hit_raises_its_confidence_and_is_recorded() {
        let mut hits = [hit_at(-97.5, 35.3, 0.60)];
        // A strong couplet about 2 km east.
        corroborate_with_rotation(&mut hits, &[(-97.478, 35.3, 35.0)]);
        assert_eq!(hits[0].rotation_ms, Some(35.0));
        // A full-strength couplet closes 40% of the gap to 1: 0.60 + 0.4 * 0.4 = 0.76.
        assert!(
            (hits[0].confidence - 0.76).abs() < 1e-4,
            "{}",
            hits[0].confidence
        );
        // A one-tilt hit can pass the single-tilt cap this way: rotation is separate evidence.
        assert!(hits[0].confidence > SINGLE_TILT_CAP);
    }

    #[test]
    fn rotation_cannot_promote_a_marginal_hit_or_reach_past_its_range() {
        // Below the floor, rotation is ignored: it corroborates, it does not rescue.
        let mut weak = [hit_at(-97.5, 35.3, ROTATION_MIN_CONFIDENCE - 0.05)];
        corroborate_with_rotation(&mut weak, &[(-97.5, 35.3, 40.0)]);
        assert_eq!(weak[0].rotation_ms, None);
        assert_eq!(weak[0].confidence, ROTATION_MIN_CONFIDENCE - 0.05);
        // At the floor it counts.
        let mut ok = [hit_at(-97.5, 35.3, ROTATION_MIN_CONFIDENCE)];
        corroborate_with_rotation(&mut ok, &[(-97.5, 35.3, 40.0)]);
        assert!(ok[0].confidence > ROTATION_MIN_CONFIDENCE);
        // Far from the radar, the hit is left alone even with a couplet right on it.
        let mut far = [TdsHit {
            range_km: ROTATION_MAX_RANGE_KM + 10.0,
            ..hit_at(-97.5, 35.3, 0.7)
        }];
        corroborate_with_rotation(&mut far, &[(-97.5, 35.3, 40.0)]);
        assert_eq!(far[0].rotation_ms, None);
        assert_eq!(far[0].confidence, 0.7);
        // A stale value from an earlier pass is cleared, not left claiming rotation that no
        // longer counts.
        let mut again = [TdsHit {
            rotation_ms: Some(30.0),
            ..hit_at(-97.5, 35.3, 0.7)
        }];
        corroborate_with_rotation(&mut again, &[]);
        assert_eq!(again[0].rotation_ms, None);
    }

    #[test]
    fn stronger_rotation_helps_more_and_weak_shear_helps_not_at_all() {
        let conf = |v: f32| {
            let mut h = [hit_at(-97.5, 35.3, 0.55)];
            corroborate_with_rotation(&mut h, &[(-97.5, 35.3, v)]);
            h[0].confidence
        };
        assert!(
            (conf(8.0) - 0.55).abs() < 1e-6,
            "ordinary shear is not corroboration"
        );
        assert!(conf(20.0) > conf(12.0));
        assert!(conf(35.0) > conf(20.0));
        // Saturates: nothing past 35 m/s adds more, and confidence never leaves 0..1.
        assert!((conf(80.0) - conf(35.0)).abs() < 1e-6);
        assert!(conf(80.0) <= 1.0);
        assert_eq!(rotation_term(10.0), 0.0);
        assert_eq!(rotation_term(35.0), 1.0);
    }

    #[test]
    fn a_couplet_far_away_or_absent_changes_nothing_and_is_not_held_against_the_hit() {
        let mut hits = [hit_at(-97.5, 35.3, 0.6)];
        // 30 km away: a different storm's circulation.
        corroborate_with_rotation(&mut hits, &[(-97.2, 35.3, 40.0)]);
        assert_eq!(hits[0].rotation_ms, None);
        assert_eq!(hits[0].confidence, 0.6);
        // No couplets at all.
        corroborate_with_rotation(&mut hits, &[]);
        assert_eq!(hits[0].rotation_ms, None);
        assert_eq!(hits[0].confidence, 0.6);
    }

    #[test]
    fn the_strongest_nearby_couplet_is_the_one_used_and_bad_values_are_ignored() {
        let mut hits = [hit_at(-97.5, 35.3, 0.55)];
        corroborate_with_rotation(
            &mut hits,
            &[
                (-97.5, 35.3, 15.0),
                (-97.49, 35.3, 30.0),
                (-97.51, 35.3, f32::NAN),
                (-97.2, 35.3, 60.0), // too far to count
            ],
        );
        assert_eq!(hits[0].rotation_ms, Some(30.0));
    }

    #[test]
    fn corroboration_can_reorder_hits_and_always_returns_them_strongest_first() {
        // The weaker signature has the tornado's rotation beside it, so it overtakes the stronger one.
        let mut hits = [hit_at(-97.5, 35.3, 0.55), hit_at(-97.0, 35.0, 0.62)];
        corroborate_with_rotation(&mut hits, &[(-97.5, 35.3, 40.0)]);
        assert!(hits[0].confidence >= hits[1].confidence);
        assert_eq!(
            hits[0].lon, -97.5,
            "the corroborated hit now leads: {hits:?}"
        );
    }

    /// A couplet at `confidence`, positioned to be corroborated by `hit_at`'s default location.
    fn couplet_at(lon: f64, lat: f64, confidence: f32) -> crate::rotation::CoupletHit {
        crate::rotation::CoupletHit {
            lon,
            lat,
            vrot_ms: 30.0,
            g2g_ms: 60.0,
            range_km: 30.0,
            gates: 10,
            tilts: 1,
            top_km: 0.5,
            base_km: 0.5,
            rooted: None,
            sense: crate::rotation::Sense::Cyclonic,
            debris_confidence: None,
            confidence,
            confirmation: crate::confirm::Confirmation::NONE,
        }
    }

    #[test]
    fn cross_corroborate_raises_both_sides_from_their_own_raw_evidence() {
        let mut debris = [hit_at(-97.5, 35.3, 0.60)];
        let mut couplets = [couplet_at(-97.478, 35.3, 0.55)]; // ~2 km east, both credible alone
        cross_corroborate(&mut debris, &mut couplets);
        assert_eq!(debris[0].rotation_ms, Some(30.0));
        assert!(
            debris[0].confidence > 0.60,
            "debris should gain from the couplet: {}",
            debris[0].confidence
        );
        assert_eq!(couplets[0].debris_confidence, Some(0.60));
        assert!(
            couplets[0].confidence > 0.55,
            "the couplet should gain from the debris signature: {}",
            couplets[0].confidence
        );
    }

    /// The trap `cross_corroborate` exists to avoid: applying the two directions by hand, in
    /// sequence, lets the first boost feed the second, counting the same evidence twice. Calling
    /// them through `cross_corroborate` must not reproduce that — both directions have to read
    /// off the *pre*-corroboration numbers.
    #[test]
    fn cross_corroborate_does_not_let_one_direction_feed_the_other() {
        let (debris_conf, couplet_conf) = (0.60, 0.55);
        let mut safe_debris = [hit_at(-97.5, 35.3, debris_conf)];
        let mut safe_couplets = [couplet_at(-97.478, 35.3, couplet_conf)];
        cross_corroborate(&mut safe_debris, &mut safe_couplets);

        // The naive, unsafe sequence: corroborate debris from the couplet's raw confidence (fine,
        // that's the same as above), but then corroborate the couplet from debris's *already
        // boosted* confidence rather than its raw one.
        let mut fed_debris = [hit_at(-97.5, 35.3, debris_conf)];
        let mut fed_couplets = [couplet_at(-97.478, 35.3, couplet_conf)];
        corroborate_with_rotation(&mut fed_debris, &[(-97.478, 35.3, 30.0)]);
        crate::rotation::corroborate_with_debris(
            &mut fed_couplets,
            &[(-97.5, 35.3, fed_debris[0].confidence)], // <- the bug: reads the boosted value
        );

        assert!(
            fed_couplets[0].confidence > safe_couplets[0].confidence,
            "the unsafe sequence double-counts: fed {} vs safe {}",
            fed_couplets[0].confidence,
            safe_couplets[0].confidence
        );
        // The safe path used the couplet's raw debris reading, not the boosted one.
        assert_eq!(safe_couplets[0].debris_confidence, Some(debris_conf));
    }

    #[test]
    fn cross_corroborate_of_nothing_touches_nothing() {
        let mut debris: [TdsHit; 0] = [];
        let mut couplets: [crate::rotation::CoupletHit; 0] = [];
        cross_corroborate(&mut debris, &mut couplets);
        let mut debris_only = [hit_at(-97.5, 35.3, 0.6)];
        let mut no_couplets: [crate::rotation::CoupletHit; 0] = [];
        cross_corroborate(&mut debris_only, &mut no_couplets);
        assert_eq!(debris_only[0].confidence, 0.6);
        assert_eq!(debris_only[0].rotation_ms, None);
    }

    #[test]
    fn the_explanation_reproduces_the_confidence_it_explains() {
        let mut hit = hit_at(-97.5, 35.3, 0.0);
        hit.tilts = 3;
        hit.top_km = 2.0;
        hit.min_cc = 0.25;
        hit.mean_cc = 0.45;
        hit.confidence = confidence(
            evidence(
                hit.min_cc,
                hit.mean_cc,
                hit.mean_z,
                hit.area_km2,
                hit.contrast,
                hit.range_km,
            ),
            vertical_term(hit.top_km, hit.tilts, hit.rooted),
        );
        let e = hit.explain();
        assert!((e.base_confidence - hit.confidence).abs() < 1e-6);
        assert_eq!(e.rotation_gain, None);
        // The weighted terms, times range, are the evidence; the weights are a whole.
        let sum: f32 = e.reasons.iter().map(|r| r.score * r.weight).sum();
        assert!((sum * e.range_factor - e.evidence).abs() < 1e-5);
        assert!((e.reasons.iter().map(|r| r.weight).sum::<f32>() - 1.0).abs() < 1e-6);
        // Rotation shows up as exactly what it added.
        let mut with = [hit];
        corroborate_with_rotation(&mut with, &[(-97.5, 35.3, 35.0)]);
        let e2 = with[0].explain();
        assert!(e2.rotation_gain.unwrap() > 0.0);
        assert!((e2.base_confidence + e2.rotation_gain.unwrap() - with[0].confidence).abs() < 1e-5);
        assert_eq!(e2.version, ALGORITHM_VERSION);
    }

    #[test]
    fn the_explanation_reads_in_plain_lines_and_says_why_a_single_tilt_is_capped() {
        let hit = hit_at(-97.5, 35.3, 0.5);
        let lines = hit.explain().lines(&hit);
        let text = lines.join("\n");
        assert!(lines[0].contains(ALGORITHM_VERSION), "{text}");
        for want in [
            "Depth", "Contrast", "Core", "Size", "Range", "Vertical", "Rotation",
        ] {
            assert!(text.contains(want), "missing {want}: {text}");
        }
        assert!(text.contains("one tilt only"), "{text}");
        assert!(text.contains("not evidence against"), "{text}");
    }

    /// Low CC in strong echo right at the tower is clutter and sidelobes, not debris.
    #[test]
    fn evidence_close_to_the_radar_is_discounted_for_clutter() {
        let at = |km: f32| evidence(0.35, 0.45, 55.0, 5.0, Some(0.2), km);
        assert!(at(4.0) < 0.6 * at(30.0), "{} vs {}", at(4.0), at(30.0));
        assert!(at(8.0) < at(15.0));
        assert_eq!(
            at(15.0),
            at(40.0),
            "full strength once clear of the clutter zone"
        );
        assert!(at(0.0) > 0.0, "and never a negative or NaN score");
    }

    fn zdr_sweep(db: f32) -> BinnedSweep {
        let (lo, hi) = Moment::DifferentialReflectivity.value_range();
        let idx = (2.0 + (db - lo) / (hi - lo) * 253.0).round() as u8;
        BinnedSweep {
            moment: Moment::DifferentialReflectivity,
            az_bins: 720,
            gate_count: 200,
            data: vec![idx; 720 * 200],
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

    /// A hit 20 km north of the radar.
    fn hit_20km_north(confidence: f32) -> TdsHit {
        TdsHit {
            lat: 35.0 + 20.0 / 110.57,
            lon: -97.5,
            range_km: 20.0,
            ..hit_at(-97.5, 35.0, confidence)
        }
    }

    #[test]
    fn a_high_zdr_around_a_hit_discounts_it_and_a_neutral_one_does_not() {
        assert_eq!(zdr_factor(0.0), 1.0);
        assert_eq!(zdr_factor(1.0), 1.0);
        assert!((zdr_factor(3.5) - 0.6).abs() < 1e-6);
        assert!(zdr_factor(2.0) < 1.0 && zdr_factor(2.0) > zdr_factor(3.0));
        assert_eq!(zdr_factor(9.0), zdr_factor(3.5), "it bottoms out");
        let mut rain = [hit_20km_north(0.8)];
        apply_zdr(&mut rain, &[zdr_sweep(3.0)]);
        assert!(
            (rain[0].zdr_db.unwrap() - 3.0).abs() < 0.3,
            "{:?}",
            rain[0].zdr_db
        );
        assert!(rain[0].confidence < 0.8 * 0.75, "{}", rain[0].confidence);
        let mut debris = [hit_20km_north(0.8)];
        apply_zdr(&mut debris, &[zdr_sweep(0.0)]);
        assert!(
            (debris[0].confidence - 0.8).abs() < 1e-6,
            "ZDR near zero adds and removes nothing"
        );
    }

    #[test]
    fn no_zdr_data_leaves_the_hit_alone_and_says_so() {
        let mut h = [hit_20km_north(0.8)];
        apply_zdr(&mut h, &[]);
        assert_eq!((h[0].zdr_db, h[0].confidence), (None, 0.8));
        // A hit off the edge of the sweep has nothing to read either.
        let mut far = [TdsHit {
            lat: 40.0,
            ..hit_20km_north(0.8)
        }];
        apply_zdr(&mut far, &[zdr_sweep(3.0)]);
        assert_eq!(far[0].zdr_db, None);
        assert_eq!(far[0].confidence, 0.8);
    }

    #[test]
    fn the_explanation_accounts_for_the_zdr_discount() {
        let mut hit = hit_20km_north(0.0);
        hit.tilts = 3;
        hit.top_km = 2.0;
        hit.confidence = confidence(
            evidence(
                hit.min_cc,
                hit.mean_cc,
                hit.mean_z,
                hit.area_km2,
                hit.contrast,
                hit.range_km,
            ),
            vertical_term(hit.top_km, hit.tilts, hit.rooted),
        );
        let mut v = [hit];
        apply_zdr(&mut v, &[zdr_sweep(3.0)]);
        let e = v[0].explain();
        assert!((e.base_confidence - v[0].confidence).abs() < 1e-5);
        assert!(e.zdr.is_some());
        assert!(e.lines(&v[0]).join("\n").contains("ZDR"));
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
