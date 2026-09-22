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
//! [`detect_volume`] runs the same detector across several tilts and raises a couplet's confidence
//! by how many of them show it and how high it reaches — and discounts the two shapes that clear
//! the gate-to-gate criterion without being the signature it is named for:
//!
//! * rotation that **never reaches the lowest tilt** is a mid-level mesocyclone, which precedes
//!   the great majority of tornadoes it never produces (see [`UNROOTED_FACTOR`]);
//! * rotation turning **the wrong way for the hemisphere** is almost never tornadic, and is
//!   exactly the shape ordinary shear zones and dealiasing failures take (see [`Sense`]).
//!
//! Velocity alone has no idea whether there is a storm at a gate or clear air — a receiver
//! glitch, a sidelobe return, or ordinary clear-air/AP noise can clear the gate-to-gate shear
//! threshold just as easily as a real vortex. [`detect`] and [`detect_volume`] both take a
//! collocated reflectivity sweep and require some real echo at the couplet before counting it,
//! the same collocation `crate::tds` already leans on for its own moments.

use crate::level2::{BinnedSweep, Moment};

/// Which way a couplet turns, allowing for the hemisphere the radar is in.
///
/// Radar azimuth increases clockwise from north, so at a fixed range a counterclockwise
/// circulation reads inbound on its lower-azimuth side and outbound on its higher-azimuth one:
/// radial velocity rises with azimuth across the couplet, and a clockwise one falls. Counter-
/// clockwise is the cyclonic sense north of the equator and the anticyclonic one south of it, so
/// the mapping from measured shear to sense flips with the sign of the radar's latitude.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sense {
    /// Turning the way this hemisphere's tornadoes almost always do.
    Cyclonic,
    /// Turning the other way: real, but rare, and the shape most shear artifacts have.
    Anticyclonic,
}

impl Sense {
    /// The sense of a couplet whose radial velocity rises with azimuth by `signed` (the summed
    /// signed gate-to-gate difference over the cluster), as seen by a radar at `radar_lat`.
    pub fn of(signed: f64, radar_lat: f32) -> Sense {
        if (signed >= 0.0) == (radar_lat >= 0.0) {
            Sense::Cyclonic
        } else {
            Sense::Anticyclonic
        }
    }

    /// The sense in a word, for a tooltip or an export.
    pub fn label(self) -> &'static str {
        match self {
            Sense::Cyclonic => "cyclonic",
            Sense::Anticyclonic => "anticyclonic",
        }
    }
}

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
    /// Beam-center height (km AGL) of the *lowest* tilt contributing to this hit -- where the
    /// column starts, against `top_km`'s where it ends. Equal to `top_km` out of [`detect`],
    /// which only ever sees one tilt.
    pub base_km: f32,
    /// Whether the column reaches the lowest low-level tilt the volume scanned (see
    /// [`LOW_TILT_MAX_DEG`] and [`UNROOTED_FACTOR`]). `None` when no low tilt was read, so
    /// nothing can be said either way; [`detect`] always reports `None`, since one tilt cannot
    /// know what the tilts under it show.
    pub rooted: Option<bool>,
    /// Which way the couplet turns. See [`Sense`] and [`sense_factor`].
    pub sense: Sense,
    /// The strongest nearby debris signature's own confidence (before either side was
    /// corroborated), once [`corroborate_with_debris`] has been run. `None` means no debris was
    /// found near this couplet (or it was never checked); it is not evidence against it.
    pub debris_confidence: Option<f32>,
    /// 0..1 confidence. `detect()`'s single-tilt read has no vertical evidence at all and caps out
    /// at 0.5 on rotational strength alone; [`detect_volume`] can go higher once a couplet repeats
    /// up through the tilts. Debris nearby ([`corroborate_with_debris`]) can raise it further: a
    /// separate line of evidence, not more of the same.
    pub confidence: f32,
    /// What people and forecasters said about it: a tornado report near it, or an observed
    /// tornado warning over it. Separate from `confidence`, which is radar alone and stops at 100%;
    /// see [`crate::confirm`]. Set by the caller, never by the detector.
    pub confirmation: crate::confirm::Confirmation,
}

/// Which revision of the scoring produced a hit; bump it when any weight or rule below changes.
pub const ALGORITHM_VERSION: &str = "rot-4";

/// The most confidence a couplet can have from one tilt alone.
pub const SINGLE_TILT_CAP: f32 = 0.5;

/// Couplets from different tilts closer than this (km) are the same circulation. Wider than
/// [`crate::tds`]'s own 3 km, because a vortex leans downshear with height and the centroid of a
/// handful of gate pairs wanders more than a debris ball's does.
const ASSOCIATE_KM: f64 = 4.0;

/// Elevation angles at or below this (degrees) are the low-level tilts a tornadic circulation has
/// to appear in. Every WSR-88D VCP starts at 0.5 deg with a second cut near 0.9 deg, so a volume
/// scanned normally always has one.
pub const LOW_TILT_MAX_DEG: f32 = 1.5;

/// The factor applied to the vertical evidence of a couplet that skips the lowest low-level tilt
/// scanned.
///
/// The signature this detector is named for is a *low-level* one. Rotation confined to the middle
/// tilts with the sweep beneath it clean is a mid-level mesocyclone: a real and useful thing to
/// see, and routinely there for a volume or two before anything reaches the ground, but not a
/// tornado signature, and the great majority of mid-level mesocyclones never become one. It
/// discounts rather than rejects, since the lowest beam can be blocked by terrain, sit under the
/// circulation at close range, or lose its velocity data to the clutter filter.
pub const UNROOTED_FACTOR: f32 = 0.7;

/// The factor an anticyclonic couplet's confidence is multiplied by.
///
/// Tornadoes turn cyclonically almost without exception; the anticyclonic ones are mostly
/// satellite and companion tornadoes and the odd QLCS mesovortex, a couple of percent of the
/// total. An anticyclonic couplet, meanwhile, is exactly the shape an ordinary shear zone, a
/// dealiasing failure and the anticyclonic half of a splitting storm all make. So the sense is
/// real evidence -- and it only ever discounts. A strong anticyclonic couplet is still worth a
/// look, and the cyclonic sense is shared with every mesocyclone that produces nothing at all, so
/// it is no proof by itself and earns no credit.
pub const ANTICYCLONIC_FACTOR: f32 = 0.7;

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

/// Vertical continuity, 0..1: height reached and number of tilts, each worth half, and then
/// [`UNROOTED_FACTOR`] if the column skips the lowest low-level tilt scanned. About 3 km AGL
/// saturates the height and three tilts the depth. One tilt is worth nothing: its height is just its
/// range (a far low beam is high, and that is not a tall circulation), so height only counts once a
/// second tilt shows the couplet is a column.
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

/// The factor a couplet's sense applies to its score: 1 for cyclonic, [`ANTICYCLONIC_FACTOR`] the
/// other way.
pub fn sense_factor(sense: Sense) -> f32 {
    match sense {
        Sense::Cyclonic => 1.0,
        Sense::Anticyclonic => ANTICYCLONIC_FACTOR,
    }
}

/// The share of the remaining gap to 1 that a full-strength debris signature closes.
///
/// Larger than [`crate::tds::ROTATION_GAP_SHARE`] (rotation corroborating debris): debris is a
/// physical object confirmed on the ground, where a couplet beside it is not just more
/// radar-measured shear but confirmation from the thing the shear was supposed to be lofting. A
/// couplet beside a debris ball is about as strong a case as this heuristic can make for a real
/// tornado.
const DEBRIS_GAP_SHARE: f32 = 0.45;

/// A couplet may only be corroborated by debris once it already stands on its own at this
/// confidence. Corroboration cannot rescue a marginal detection, only strengthen a credible one --
/// the same rule [`crate::tds::ROTATION_MIN_CONFIDENCE`] applies from the other side.
pub const DEBRIS_BOOST_MIN_CONFIDENCE: f32 = 0.4;

/// How much a debris signature of `confidence` (its own, read before any corroboration) supports a
/// nearby couplet, 0..1. Below [`crate::tds::CORROBORATING_DEBRIS_CONFIDENCE`] scores 0 -- a
/// marginal low-CC patch is not debris -- and a well-formed debris ball (deep, contrasting,
/// sizeable) at 80% scores 1.
pub fn debris_term(confidence: f32) -> f32 {
    let floor = crate::tds::CORROBORATING_DEBRIS_CONFIDENCE;
    ((confidence - floor) / (0.80 - floor)).clamp(0.0, 1.0)
}

/// Raise the confidence of couplets that have a debris signature beside them, and record it on the
/// hit. `debris` is `(lon, lat, confidence)`, where `confidence` is the debris hit's own reading
/// from *before* [`crate::tds::corroborate_with_rotation`] ran on it -- the two directions must
/// each work from the other side's pre-corroboration evidence, or they double-count the same
/// signature by feeding each other's boost back in. [`crate::tds::cross_corroborate`] does this
/// safely from raw hits on both sides; call this directly only when you can make the same
/// guarantee yourself.
///
/// Mirrors [`crate::tds::corroborate_with_rotation`] exactly, from the other side: only couplets
/// within [`crate::tds::ROTATION_MAX_RANGE_KM`] are considered (the same range past which neither
/// a couplet nor a debris ball is well resolved), a couplet below
/// [`DEBRIS_BOOST_MIN_CONFIDENCE`] is left alone, and `debris_confidence` is recorded only when
/// the debris actually counted. The caller should pass only debris hits filtered with
/// [`crate::tds::debris_corroborates`], mirroring how `corroborate_with_rotation`'s own callers
/// filter couplets with [`crate::tds::couplet_corroborates`].
///
/// The absence of a debris signature changes nothing: many real tornadoes never loft debris the
/// radar can see, and a couplet is often seen well before any debris appears (or on a tilt the TDS
/// pass didn't scan). Hits come back strongest first.
pub fn corroborate_with_debris(hits: &mut [CoupletHit], debris: &[(f64, f64, f32)]) {
    for h in hits.iter_mut() {
        h.debris_confidence = None;
        if h.range_km > crate::tds::ROTATION_MAX_RANGE_KM
            || h.confidence < DEBRIS_BOOST_MIN_CONFIDENCE
        {
            continue;
        }
        let strongest = debris
            .iter()
            .filter(|(lon, lat, _)| {
                crate::tds::ground_km((h.lon, h.lat), (*lon, *lat))
                    <= crate::tds::ROTATION_ASSOCIATE_KM
            })
            .map(|(_, _, c)| *c)
            .filter(|c| c.is_finite())
            .fold(None, |best: Option<f32>, c| {
                Some(best.map_or(c, |b| b.max(c)))
            });
        h.debris_confidence = strongest;
        if let Some(c) = strongest {
            let gap = 1.0 - h.confidence;
            h.confidence = (h.confidence + gap * DEBRIS_GAP_SHARE * debris_term(c)).clamp(0.0, 1.0);
        }
    }
    hits.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
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
    /// Which way it turns, and the factor that applied to the score.
    pub sense: (Sense, f32),
    /// Evidence, vertical and sense factors combined: the confidence before debris.
    pub base_confidence: f32,
    /// What debris beside the couplet added, if it counted.
    pub debris_gain: Option<f32>,
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
        let evidence = evidence(self.g2g_ms, per_tilt, self.range_km);
        let vertical = vertical_term(self.top_km, self.tilts, self.rooted);
        let sense = (self.sense, sense_factor(self.sense));
        let base_confidence = confidence(evidence, vertical) * sense.1;
        let debris_gain = self
            .debris_confidence
            .map(|_| (self.confidence - base_confidence).max(0.0));
        Explanation {
            version: ALGORITHM_VERSION,
            reasons,
            range_factor: range_factor(self.range_km),
            evidence,
            vertical,
            sense,
            base_confidence,
            debris_gain,
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
                "Vertical  {:.0}%   {} tilts, {:.1}-{:.1} km{}",
                self.vertical * 100.0,
                hit.tilts,
                hit.base_km,
                hit.top_km,
                match hit.rooted {
                    // The one worth spelling out: the rotation is aloft, so it kept only
                    // `UNROOTED_FACTOR` of the vertical evidence it would otherwise have earned.
                    Some(false) => ", not rooted in the lowest tilt",
                    Some(true) => ", rooted in the lowest tilt",
                    None => "",
                }
            )
        } else {
            format!(
                "Vertical  {:.0}%   one tilt only: capped at {:.0}%",
                self.vertical * 100.0,
                SINGLE_TILT_CAP * 100.0
            )
        });
        out.push(format!(
            "Sense     x{:.2}   {}",
            self.sense.1,
            self.sense.0.label()
        ));
        match (hit.debris_confidence, self.debris_gain) {
            (Some(c), Some(g)) => out.push(format!(
                "Debris    +{:.0} points   {:.0}% debris signature within {:.0} km",
                g * 100.0,
                c * 100.0,
                crate::tds::ROTATION_ASSOCIATE_KM
            )),
            _ => out.push("Debris    none counted (not evidence against it)".to_string()),
        }
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
    /// One grid cell's running totals, named so the map's own type stays readable.
    struct Cell {
        /// Candidate gate pairs.
        pairs: usize,
        /// Summed lon / lat / range, for the centroid and the mean range.
        sum_lon: f64,
        sum_lat: f64,
        sum_range: f64,
        /// The extremes of the cluster's velocity spread; `vrot` is half of it.
        v_min: f32,
        v_max: f32,
        /// Strongest gate-to-gate difference.
        max_dv: f32,
        /// Summed *signed* gate-to-gate difference, higher azimuth minus lower. Its sign is which
        /// way the cluster turns, and summing rather than counting means the strongest pairs have
        /// the say when a noisy edge of the cluster shears the other way; see [`Sense::of`].
        signed: f64,
    }
    impl Cell {
        fn new() -> Cell {
            Cell {
                pairs: 0,
                sum_lon: 0.0,
                sum_lat: 0.0,
                sum_range: 0.0,
                v_min: f32::MAX,
                v_max: f32::MIN,
                max_dv: 0.0,
                signed: 0.0,
            }
        }
    }
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
            let e = cells.entry(key).or_insert_with(Cell::new);
            e.pairs += 1;
            e.sum_lon += lon;
            e.sum_lat += lat;
            e.sum_range += range as f64;
            e.v_min = e.v_min.min(a).min(b);
            e.v_max = e.v_max.max(a).max(b);
            e.max_dv = e.max_dv.max(dv);
            e.signed += f64::from(b - a);
        }
    }

    let elev = vel.elevation_deg as f64;
    let mut hits: Vec<CoupletHit> = cells
        .into_values()
        .filter(|c| c.pairs >= min_gates)
        .map(|c| {
            let n = c.pairs as f64;
            let range_km = (c.sum_range / n) as f32;
            let sense = Sense::of(c.signed, vel.radar_lat);
            // One tilt has no vertical evidence, so it never gets past the single-tilt cap;
            // `detect_volume` rescores the hit once it can see the column.
            let confidence = (confidence(evidence(c.max_dv, c.pairs as f32, range_km), 0.0)
                * sense_factor(sense))
            .max(0.05);
            let height = crate::xsection::beam_height_km(range_km as f64, elev) as f32;
            CoupletHit {
                lon: c.sum_lon / n,
                lat: c.sum_lat / n,
                vrot_ms: (c.v_max - c.v_min) / 2.0,
                g2g_ms: c.max_dv,
                range_km,
                gates: c.pairs,
                tilts: 1,
                top_km: height,
                base_km: height,
                rooted: None,
                sense,
                debris_confidence: None,
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
/// caller sees no change) within `ASSOCIATE_KM` of one another on the ground are merged into one
/// [`CoupletHit`] with the combined gate count, the strongest rotation and gate-to-gate shear seen
/// at any contributing tilt, the span from its lowest contributing beam to its highest, and a
/// correspondingly higher confidence. `sweeps` need not be given lowest-first — every height, and
/// which tilt counts as the lowest, comes from each sweep's own `elevation_deg` and not from its
/// position in the slice.
///
/// Two further readings come out of the column that one tilt cannot give: whether it reaches the
/// lowest low-level tilt scanned ([`UNROOTED_FACTOR`]), and which way the bulk of it turns
/// ([`Sense`]).
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
    // The lowest low-level tilt in the volume, found by elevation angle and not by position in
    // the slice (which `sweeps` is explicitly not required to order). A column that misses it is
    // rotation aloft; see `UNROOTED_FACTOR`. With no low tilt read at all there is nothing to
    // miss, and rooting stays unknown.
    let low_tilt = sweeps
        .iter()
        .enumerate()
        .filter(|(_, (vel, _))| vel.elevation_deg <= LOW_TILT_MAX_DEG)
        .min_by(|a, b| a.1 .0.elevation_deg.total_cmp(&b.1 .0.elevation_deg))
        .map(|(i, _)| i);

    let mut per_tilt: Vec<(usize, CoupletHit)> = Vec::new();
    for (tilt, (vel, z)) in sweeps.iter().enumerate() {
        for h in detect(
            vel,
            z,
            g2g_min_ms,
            z_min,
            min_range_km,
            max_range_km,
            min_gates,
        ) {
            per_tilt.push((tilt, h));
        }
    }

    // Associate by ground distance rather than by snapping to a second grid. `detect` has already
    // clustered each tilt's gate pairs; re-snapping those clusters split a column whenever two
    // tilts' centroids fell either side of a cell edge, which turned one two-tilt couplet into two
    // single-tilt ones, each held to `SINGLE_TILT_CAP`, and lost the vertical evidence this
    // function exists to find. Same helper `crate::tds::detect_volume` associates debris with.
    let centres: Vec<(f64, f64)> = per_tilt.iter().map(|(_, h)| (h.lon, h.lat)).collect();
    let mut out: Vec<CoupletHit> = crate::tds::associate_by_ground(&centres, ASSOCIATE_KM)
        .into_iter()
        .map(|members| {
            let hits: Vec<&CoupletHit> = members.iter().map(|&i| &per_tilt[i].1).collect();
            let gates: usize = hits.iter().map(|h| h.gates).sum();
            let w = gates.max(1) as f64;
            let weighted = |f: &dyn Fn(&CoupletHit) -> f64| -> f64 {
                hits.iter().map(|h| f(h) * h.gates as f64).sum::<f64>() / w
            };
            let contributing: std::collections::HashSet<usize> =
                members.iter().map(|&i| per_tilt[i].0).collect();
            let rooted = low_tilt.map(|t| contributing.contains(&t));
            let tilts = contributing.len();
            // Which way the bulk of the column turns: the gates vote, not the tilts, so one
            // tilt's stray counter-turning cluster cannot flip a column that is otherwise plainly
            // cyclonic. Each per-tilt hit already carries its own hemisphere-corrected sense, so
            // this is a straight tally and needs no second look at the latitude.
            let votes: f64 = hits
                .iter()
                .map(|h| match h.sense {
                    Sense::Cyclonic => h.gates as f64,
                    Sense::Anticyclonic => -(h.gates as f64),
                })
                .sum();
            let sense = if votes >= 0.0 {
                Sense::Cyclonic
            } else {
                Sense::Anticyclonic
            };
            let vrot_ms = hits.iter().map(|h| h.vrot_ms).fold(0.0, f32::max);
            let g2g_ms = hits.iter().map(|h| h.g2g_ms).fold(0.0, f32::max);
            let top_km = hits.iter().map(|h| h.top_km).fold(0.0, f32::max);
            let base_km = hits.iter().map(|h| h.base_km).fold(f32::MAX, f32::min);
            let range_km = weighted(&|h| f64::from(h.range_km)) as f32;
            // What the cluster itself shows (strength and size, faded by range), scaled by the
            // vertical evidence: a couplet that repeats through several tilts is an established
            // circulation, where a wide single-tilt patch is as often a gust front or a glitch.
            // Size is per tilt, so a tall column is not scored as a big patch.
            let per_tilt_gates = gates as f32 / tilts.max(1) as f32;
            let confidence = confidence(
                evidence(g2g_ms, per_tilt_gates, range_km),
                vertical_term(top_km, tilts, rooted),
            ) * sense_factor(sense);
            CoupletHit {
                lon: weighted(&|h| h.lon),
                lat: weighted(&|h| h.lat),
                vrot_ms,
                g2g_ms,
                range_km,
                gates,
                tilts,
                top_km,
                base_km,
                rooted,
                sense,
                debris_confidence: None,
                confidence,
                confirmation: crate::confirm::Confirmation::NONE,
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

    /// The fixture paints inbound at the lower azimuth and outbound at the higher one, which is
    /// velocity rising with azimuth: counterclockwise, and so cyclonic at this radar's latitude.
    /// Painted the other way round it is the same couplet turning the other way, and it scores
    /// lower for it.
    #[test]
    fn the_detector_reads_which_way_the_couplet_turns() {
        let z = z_sweep(0.5, Some(45.0));
        let run = |vel: &BinnedSweep| detect(vel, &z, 25.0, 20.0, 5.0, 150.0, 3);
        let cyclonic = run(&couplet_sweep(-30.0, 30.0, 0.0));
        let anticyclonic = run(&couplet_sweep(30.0, -30.0, 0.0));
        assert_eq!(cyclonic[0].sense, Sense::Cyclonic);
        assert_eq!(anticyclonic[0].sense, Sense::Anticyclonic);
        // Same rotation either way, so only the sense can be moving the score.
        assert!((cyclonic[0].vrot_ms - anticyclonic[0].vrot_ms).abs() < 1e-3);
        assert!(
            anticyclonic[0].confidence < cyclonic[0].confidence,
            "{} vs {}",
            anticyclonic[0].confidence,
            cyclonic[0].confidence
        );
    }

    /// Two tilts' views of one circulation are a kilometre or two apart on the ground. They used
    /// to be merged by snapping to a ~4 km grid, which split the column whenever the pair fell
    /// either side of a cell edge and cost it all of its vertical evidence. Association is by
    /// ground distance now, so an offset that size merges wherever the grid would have fallen.
    #[test]
    fn tilts_offset_on_the_ground_still_merge_into_one_column() {
        // The upper tilt's couplet sits ~1 km further out in range than the lower one's.
        let higher = {
            let (lo, hi) = Moment::Velocity.value_range();
            let idx = |v: f32| (2.0 + (v - lo) / (hi - lo) * 253.0).round() as u8;
            let mut s = couplet_sweep_tilt(1.4, 0.0, 0.0, 0.0);
            for g in 44..52 {
                for az in 98..100 {
                    s.data[az * s.gate_count + g] = idx(-30.0);
                }
                for az in 100..102 {
                    s.data[az * s.gate_count + g] = idx(30.0);
                }
            }
            s
        };
        let sweeps = [
            (
                couplet_sweep_tilt(0.5, -30.0, 30.0, 0.0),
                z_sweep(0.5, Some(45.0)),
            ),
            (higher, z_sweep(1.4, Some(45.0))),
        ];
        let hits = detect_volume(&sweeps, 25.0, 20.0, 5.0, 150.0, 3);
        assert_eq!(hits.len(), 1, "one circulation, not two");
        assert_eq!(hits[0].tilts, 2);
        assert_eq!(hits[0].rooted, Some(true), "the lowest tilt shows it");
        assert!(
            hits[0].base_km < hits[0].top_km,
            "{} to {}",
            hits[0].base_km,
            hits[0].top_km
        );
    }

    /// Rotation that lives in the middle tilts with a clean sweep beneath it is a mid-level
    /// mesocyclone, not the low-level signature this detector is named for.
    #[test]
    fn a_column_that_misses_the_lowest_tilt_is_marked_unrooted() {
        let flat = || couplet_sweep_tilt(0.5, 0.0, 0.0, 0.0);
        let unrooted = detect_volume(
            &[
                (flat(), z_sweep(0.5, Some(45.0))),
                (
                    couplet_sweep_tilt(1.4, -30.0, 30.0, 0.0),
                    z_sweep(1.4, Some(45.0)),
                ),
                (
                    couplet_sweep_tilt(2.4, -30.0, 30.0, 0.0),
                    z_sweep(2.4, Some(45.0)),
                ),
            ],
            25.0,
            20.0,
            5.0,
            150.0,
            3,
        );
        assert!(!unrooted.is_empty());
        assert_eq!(unrooted[0].rooted, Some(false));
        // With no low tilt scanned at all, there is nothing to have missed.
        let no_low = detect_volume(
            &[
                (
                    couplet_sweep_tilt(1.8, -30.0, 30.0, 0.0),
                    z_sweep(1.8, Some(45.0)),
                ),
                (
                    couplet_sweep_tilt(2.4, -30.0, 30.0, 0.0),
                    z_sweep(2.4, Some(45.0)),
                ),
            ],
            25.0,
            20.0,
            5.0,
            150.0,
            3,
        );
        assert!(!no_low.is_empty());
        assert_eq!(no_low[0].rooted, None);
    }
}

#[cfg(test)]
mod scoring_tests {
    use super::*;

    fn hit(g2g_ms: f32, gates: usize, range_km: f32, tilts: usize, top_km: f32) -> CoupletHit {
        sensed_hit(g2g_ms, gates, range_km, tilts, top_km, Sense::Cyclonic)
    }

    /// `hit`, but turning whichever way is asked for -- scored exactly the way the detectors
    /// score, so a test cannot disagree with the map about what a couplet is worth.
    fn sensed_hit(
        g2g_ms: f32,
        gates: usize,
        range_km: f32,
        tilts: usize,
        top_km: f32,
        sense: Sense,
    ) -> CoupletHit {
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
            base_km: top_km,
            rooted: None,
            sense,
            debris_confidence: None,
            confidence: confidence(
                evidence(g2g_ms, per_tilt, range_km),
                vertical_term(top_km, tilts, None),
            ) * sense_factor(sense),
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
        assert_eq!(vertical_term(4.0, 1, None), 0.0);
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
        let rebuilt = confidence(e.evidence, e.vertical) * e.sense.1;
        assert!((rebuilt - h.confidence).abs() < 1e-5);
        let text = e.lines(&h).join("\n");
        for want in [
            "Strength",
            "Size",
            "Range",
            "Vertical",
            "Sense",
            "cyclonic",
            ALGORITHM_VERSION,
        ] {
            assert!(text.contains(want), "missing {want}: {text}");
        }
        let lone = hit(32.0, 10, 45.0, 1, 0.5);
        assert!(lone
            .explain()
            .lines(&lone)
            .join("\n")
            .contains("one tilt only"));
    }

    #[test]
    fn turning_the_wrong_way_costs_a_couplet_but_never_hides_it() {
        let cyclonic = hit(36.0, 45, 30.0, 3, 2.0);
        let anticyclonic = sensed_hit(36.0, 45, 30.0, 3, 2.0, Sense::Anticyclonic);
        assert!(
            anticyclonic.confidence < cyclonic.confidence,
            "{} vs {}",
            anticyclonic.confidence,
            cyclonic.confidence
        );
        assert!((anticyclonic.confidence / cyclonic.confidence - ANTICYCLONIC_FACTOR).abs() < 1e-5);
        // A discount, not a rejection: a strong anticyclonic couplet is still worth a look.
        assert!(anticyclonic.confidence > 0.3, "{}", anticyclonic.confidence);
        assert_eq!(sense_factor(Sense::Cyclonic), 1.0);
        // And the explanation names it.
        let text = anticyclonic.explain().lines(&anticyclonic).join("\n");
        assert!(text.contains("anticyclonic"), "{text}");
    }

    /// Velocity rising with azimuth is counterclockwise, which is cyclonic north of the equator
    /// and anticyclonic south of it.
    #[test]
    fn the_sense_of_a_shear_sign_flips_with_the_hemisphere() {
        assert_eq!(Sense::of(12.0, 35.0), Sense::Cyclonic);
        assert_eq!(Sense::of(-12.0, 35.0), Sense::Anticyclonic);
        assert_eq!(Sense::of(12.0, -33.0), Sense::Anticyclonic);
        assert_eq!(Sense::of(-12.0, -33.0), Sense::Cyclonic);
    }

    #[test]
    fn rotation_that_skips_the_lowest_tilt_keeps_less_of_its_vertical_evidence() {
        let anchored = vertical_term(2.0, 3, Some(true));
        let aloft = vertical_term(2.0, 3, Some(false));
        assert!(aloft < anchored, "{aloft} vs {anchored}");
        assert!((aloft / anchored - UNROOTED_FACTOR).abs() < 1e-6);
        // Nothing is deducted when no low tilt was read: there was nothing to miss.
        assert_eq!(vertical_term(2.0, 3, None), anchored);
        // A single tilt has no vertical evidence to discount either way.
        assert_eq!(vertical_term(2.0, 1, Some(false)), 0.0);
    }

    #[test]
    fn debris_beside_a_couplet_raises_its_confidence_and_is_recorded() {
        let mut hits = [hit(36.0, 45, 30.0, 3, 2.0)]; // a credible, well-rooted couplet
                                                      // A strong debris signature about 2 km east.
        corroborate_with_debris(&mut hits, &[(-97.478, 35.3, 0.75)]);
        assert_eq!(hits[0].debris_confidence, Some(0.75));
        assert!(
            hits[0].confidence > SINGLE_TILT_CAP,
            "debris is separate evidence from the couplet's own single-tilt cap: {}",
            hits[0].confidence
        );
    }

    #[test]
    fn debris_cannot_promote_a_marginal_couplet_or_reach_past_its_range() {
        // Below the floor, debris is ignored: it corroborates, it does not rescue.
        let mut weak = [hit(36.0, 45, 30.0, 3, 2.0)];
        weak[0].confidence = DEBRIS_BOOST_MIN_CONFIDENCE - 0.05;
        corroborate_with_debris(&mut weak, &[(-97.5, 35.3, 0.9)]);
        assert_eq!(weak[0].debris_confidence, None);
        assert_eq!(weak[0].confidence, DEBRIS_BOOST_MIN_CONFIDENCE - 0.05);
        // Far from the radar, the couplet is left alone even with debris right on it.
        let mut far = hit(36.0, 45, crate::tds::ROTATION_MAX_RANGE_KM + 10.0, 3, 2.0);
        far.confidence = 0.7;
        let mut far = [far];
        corroborate_with_debris(&mut far, &[(-97.5, 35.3, 0.9)]);
        assert_eq!(far[0].debris_confidence, None);
        assert_eq!(far[0].confidence, 0.7);
        // A stale value from an earlier pass is cleared, not left claiming debris that no longer
        // counts.
        let mut again = hit(36.0, 45, 30.0, 3, 2.0);
        again.debris_confidence = Some(0.8);
        let mut again = [again];
        corroborate_with_debris(&mut again, &[]);
        assert_eq!(again[0].debris_confidence, None);
    }

    #[test]
    fn a_stronger_debris_signature_helps_more_and_a_marginal_one_helps_not_at_all() {
        let conf = |c: f32| {
            let mut h = [hit(36.0, 45, 30.0, 3, 2.0)];
            h[0].confidence = 0.55;
            corroborate_with_debris(&mut h, &[(-97.5, 35.3, c)]);
            h[0].confidence
        };
        assert!(
            (conf(crate::tds::CORROBORATING_DEBRIS_CONFIDENCE) - 0.55).abs() < 1e-6,
            "a marginal low-CC patch is not corroboration"
        );
        assert!(conf(0.55) > conf(0.45));
        assert!(conf(0.80) > conf(0.55));
        // Saturates: nothing past 80% confidence adds more, and confidence never leaves 0..1.
        assert!((conf(1.0) - conf(0.80)).abs() < 1e-6);
        assert!(conf(1.0) <= 1.0);
        assert_eq!(
            debris_term(crate::tds::CORROBORATING_DEBRIS_CONFIDENCE),
            0.0
        );
        assert_eq!(debris_term(0.80), 1.0);
    }

    #[test]
    fn debris_far_away_or_absent_changes_nothing_and_is_not_held_against_the_couplet() {
        let mut hits = [hit(36.0, 45, 30.0, 3, 2.0)];
        hits[0].confidence = 0.6;
        // 30 km away: a different storm's debris.
        corroborate_with_debris(&mut hits, &[(-97.2, 35.3, 0.9)]);
        assert_eq!(hits[0].debris_confidence, None);
        assert_eq!(hits[0].confidence, 0.6);
        corroborate_with_debris(&mut hits, &[]);
        assert_eq!(hits[0].debris_confidence, None);
        assert_eq!(hits[0].confidence, 0.6);
    }

    #[test]
    fn debris_corroboration_shows_up_in_the_explanation() {
        let mut hits = [hit(36.0, 45, 30.0, 3, 2.0)];
        corroborate_with_debris(&mut hits, &[(-97.478, 35.3, 0.75)]);
        let e = hits[0].explain();
        assert_eq!(e.debris_gain, Some(e.confidence - e.base_confidence));
        assert!(e.debris_gain.unwrap() > 0.0);
        let text = e.lines(&hits[0]).join("\n");
        assert!(text.contains("Debris"), "{text}");
        assert!(text.contains("75%"), "{text}");
    }
}
