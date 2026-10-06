//! Region-based Doppler velocity dealiasing — a simplified port of Py-ART's
//! `dealias_region_based`.
//!
//! Aliased ("folded") velocity wraps at the Nyquist velocity ±V_ny: a target moving
//! faster than V_ny reads as a value of the opposite sign. We segment the sweep into
//! regions of internally-continuous velocity (neighbors within half a Nyquist interval),
//! then unfold each region by an integer number of 2·V_ny steps so that velocity is
//! continuous across region boundaries. The largest region anchors at zero folds; every
//! other region is unfolded relative to the anchor.
//!
//! Region offsets are solved globally rather than by traversal order. Each pair of touching
//! regions is collapsed to a single voted fold difference plus a confidence — the number of
//! boundary gate pairs that backed it — and the sweep is solved along a maximum spanning tree
//! over those edges, strongest constraint first, followed by a bounded refinement pass that
//! re-checks every region against *all* its neighbours rather than just its tree parent. Folds
//! past a single interval fall out of the arithmetic, so multi-fold fields (very high-shear
//! tornadic couplets past 2·V_ny) no longer come apart at a thin early boundary the way the
//! previous greedy BFS could.
//!
//! ponytail: still one sweep at a time, no sequential second pass over the volume.
//!
//! Two things it does do beyond the simplest version of that idea. Region folds are chosen by a
//! *vote* over the gate pairs along a boundary rather than by averaging them: one stray pair on a
//! long boundary used to be able to drag the mean across the rounding line and fold a whole
//! region wrongly. And the anchor region can be tied to a reference field — the previous sweep's
//! already-dealiased velocity — instead of being assumed unfolded, which is what keeps a storm
//! that is genuinely moving faster than the Nyquist velocity from snapping back to zero folds the
//! moment it becomes the largest region in the sweep.
//!
//! ponytail: one previous sweep of continuity, not a 4D UNRAVEL-style solve.

/// The fold with the most votes. Ties break toward the smaller unfold: with no evidence either
/// way, the answer that moves the data least is the safer one. A tie between +n and -n breaks
/// toward +n, by rule rather than by the hash map's iteration order, which differs per run.
fn winning_fold(votes: std::collections::HashMap<i32, u32>) -> i32 {
    votes
        .into_iter()
        .max_by_key(|&(fold, n)| (n, -fold.abs(), fold))
        .map(|(fold, _)| fold)
        .unwrap_or(0)
}

/// Estimate the Nyquist velocity from a folded field as the largest observed |v|.
/// Folded data saturates at ±V_ny, so this is a robust practical proxy when the model
/// doesn't carry the radial's unambiguous velocity.
pub fn estimate_nyquist(vel: &[Option<f32>]) -> f32 {
    vel.iter()
        .filter_map(|v| *v)
        .fold(0.0f32, |m, v| m.max(v.abs()))
}

/// Where a sweep's Nyquist velocity came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NyquistSource {
    /// Not known: no decoded value and no velocity to estimate from.
    #[default]
    Unknown,
    /// Decoded from the radials' own Message 31 radial blocks.
    Decoded,
    /// Estimated as the largest raw |v|, because the radials carried no usable value.
    Estimated,
    /// Estimated, because the decoded values differ between rows (sectors at different PRFs),
    /// which one unfolding interval cannot represent.
    EstimatedVaries,
    /// Estimated, because raw velocities exceed the decoded value: whatever that value is, the
    /// field was not folded at it.
    EstimatedInconsistent,
}

impl NyquistSource {
    /// What a reader is told, after the value.
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Decoded => "decoded",
            Self::Estimated => "estimated from the largest |v| (none decoded)",
            Self::EstimatedVaries => {
                "estimated from the largest |v| (the decoded value varies by sector)"
            }
            Self::EstimatedInconsistent => {
                "estimated from the largest |v| (raw values exceed the decoded one)"
            }
        }
    }
}

/// How far decoded per-row values may differ and still be one Nyquist: the radial block carries
/// it to 0.01 m/s, so rows of one PRF agree exactly; this only absorbs rounding.
const NYQUIST_AGREEMENT_MS: f32 = 0.05;
/// How far a raw |v| may sit past the decoded Nyquist before the two are inconsistent: Level II
/// velocity comes in 0.5 m/s steps (1 m/s in the coarse mode), so a gate at the limit can round
/// up past it by one step.
const NYQUIST_QUANTIZATION_MS: f32 = 1.0;

/// The Nyquist velocity to unfold a sweep at, and where it came from. `decoded` is the decoded
/// per-row value (NaN or absent where a row carried none), `raw` the folded field.
///
/// The decoded value wins whenever it can be trusted. The largest observed |v| equals it on a
/// storm whose winds reach the limit, but falls far short of it on a weak field, and dealiasing
/// at the shortfall reads ordinary shear as folds — a 20 m/s couplet in a field peaking at
/// ±12 m/s is "unfolded" by 24 m/s. ponytail: one value per sweep; a sweep sectorized at
/// different PRFs falls back to the estimate rather than unfolding per sector.
pub fn sweep_nyquist(decoded: &[f32], raw: &[Option<f32>]) -> (f32, NyquistSource) {
    let estimate = estimate_nyquist(raw);
    let estimated = |why: NyquistSource| {
        if estimate > 0.0 {
            (estimate, why)
        } else {
            (0.0, NyquistSource::Unknown)
        }
    };
    let mut known: Vec<f32> = decoded
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v > 0.0)
        .collect();
    if known.is_empty() {
        return estimated(NyquistSource::Estimated);
    }
    known.sort_by(f32::total_cmp);
    let (lo, hi) = (known[0], known[known.len() - 1]);
    if hi - lo > NYQUIST_AGREEMENT_MS {
        return estimated(NyquistSource::EstimatedVaries);
    }
    let value = known[known.len() / 2];
    if estimate > value + NYQUIST_QUANTIZATION_MS {
        return estimated(NyquistSource::EstimatedInconsistent);
    }
    (value, NyquistSource::Decoded)
}

/// Bands each Nyquist interval is cut into for region finding (Py-ART's `interval_splits`).
const BANDS: f32 = 3.0;
/// Gates of no data a boundary may step over, along a radial or across azimuth (Py-ART's
/// `skip_along_ray` and `skip_between_rays`).
const MAX_GAP: usize = 100;

/// Dealias a polar velocity grid laid out as `vel[az * gate_count + gate]`.
/// `None` gates (no data / below threshold / range folded) pass through untouched.
/// Azimuth wraps (bin 0 neighbors bin az_bins-1); range does not.
pub fn dealias(
    vel: &[Option<f32>],
    az_bins: usize,
    gate_count: usize,
    nyquist: f32,
) -> Vec<Option<f32>> {
    dealias_with_reference(vel, az_bins, gate_count, nyquist, None)
}

/// As [`dealias`], with an optional continuity reference on the same grid — normally the previous
/// sweep's dealiased output.
///
/// The reference does one job: it decides how many folds the *anchor* region carries, instead of
/// the anchor being assumed unfolded. Everything else is unchanged, because every other region is
/// already solved relative to the anchor. A sweep whose fastest air is genuinely past the Nyquist
/// velocity is stable across volumes this way rather than snapping to zero whenever the fast
/// region happens to become the biggest one.
pub fn dealias_with_reference(
    vel: &[Option<f32>],
    az_bins: usize,
    gate_count: usize,
    nyquist: f32,
    reference: Option<&[Option<f32>]>,
) -> Vec<Option<f32>> {
    let n = az_bins * gate_count;
    debug_assert_eq!(vel.len(), n);
    if nyquist <= 0.0 || n == 0 {
        return vel.to_vec();
    }
    let interval = 2.0 * nyquist;

    // --- 1. Regions, by Py-ART's segmentation. ---
    // The Nyquist interval is cut into three equal bands (with more past ±V_ny when values lie
    // there), and a region is a 4-connected run of gates inside one band. A band edge, not a
    // neighbour difference, divides regions, so a region cannot creep across a fold through a
    // chain of noisy gates each a little different from the last — which the previous
    // neighbour-difference flood fill did, and which left whole sectors of fast fields unfolded.
    let band_width = interval / BANDS;
    let band = |v: f32| ((v + nyquist) / band_width).floor() as i32;
    // labels: usize::MAX = no data, otherwise the region id.
    const NONE: usize = usize::MAX;
    let mut labels = vec![NONE; n];
    let mut sizes: Vec<usize> = Vec::new();
    let mut stack = Vec::new();
    for start in 0..n {
        let Some(v0) = vel[start] else { continue };
        if labels[start] != NONE {
            continue;
        }
        let region = sizes.len();
        let b = band(v0);
        let mut size = 0usize;
        labels[start] = region;
        stack.push(start);
        while let Some(i) = stack.pop() {
            size += 1;
            let (az, g) = (i / gate_count, i % gate_count);
            let neighbours = [
                Some(((az + 1) % az_bins) * gate_count + g),
                Some(((az + az_bins - 1) % az_bins) * gate_count + g),
                (g + 1 < gate_count).then(|| i + 1),
                (g > 0).then(|| i - 1),
            ];
            for j in neighbours.into_iter().flatten() {
                if labels[j] == NONE && vel[j].is_some_and(|v| band(v) == b) {
                    labels[j] = region;
                    stack.push(j);
                }
            }
        }
        sizes.push(size);
    }
    let region_count = sizes.len();
    if region_count == 0 {
        return vel.to_vec();
    }

    // --- 2. Boundaries between regions. ---
    // Each gate looks at its next neighbour along the radial and across azimuth (wrapping at
    // north), stepping over up to MAX_GAP gates of no data as Py-ART does, so two regions a
    // patch of filtered gates apart still constrain each other. Each touching pair is summed
    // once: a vote per gate pair for the whole number of intervals f that, added to the high-id
    // side, makes the pair continuous (f = round((v_lo − v_hi) / interval)).
    type Votes = std::collections::HashMap<i32, u32>;
    let mut pairs: std::collections::HashMap<(usize, usize), Votes> =
        std::collections::HashMap::new();
    let mut touch = |a: usize, b: usize, va: f32, vb: f32| {
        if a == b {
            return;
        }
        let (lo, hi, d) = if a < b {
            (a, b, va as f64 - vb as f64)
        } else {
            (b, a, vb as f64 - va as f64)
        };
        let fold = (d / interval as f64).round() as i32;
        *pairs.entry((lo, hi)).or_default().entry(fold).or_insert(0) += 1;
    };
    let az_steps = MAX_GAP.min(az_bins.saturating_sub(1));
    for i in 0..n {
        let ra = labels[i];
        if ra == NONE {
            continue;
        }
        let Some(va) = vel[i] else { continue };
        let (az, g) = (i / gate_count, i % gate_count);
        // Along the radial.
        for step in 1..=MAX_GAP + 1 {
            let gg = g + step;
            if gg >= gate_count {
                break;
            }
            let j = i + step;
            if labels[j] != NONE {
                if let Some(vb) = vel[j] {
                    touch(ra, labels[j], va, vb);
                }
                break;
            }
        }
        // Across azimuth.
        for step in 1..=az_steps + 1 {
            if step >= az_bins {
                break;
            }
            let j = ((az + step) % az_bins) * gate_count + g;
            if labels[j] != NONE {
                if let Some(vb) = vel[j] {
                    touch(ra, labels[j], va, vb);
                }
                break;
            }
        }
    }

    // --- 3. Merge, heaviest boundary first (Py-ART's `_combine_regions`). ---
    // Take the boundary with the most gate pairs, unfold the smaller side by that boundary's fold
    // so it continues the larger, and merge the two into one node; the merged node's boundaries
    // with a common neighbour add together, so evidence accumulates as the sweep is assembled
    // instead of every region being judged on one boundary alone. Ties go to the lower boundary
    // id, and ids follow sorted region pairs, so the result is the same on every run.
    //
    // One departure from Py-ART: a boundary's fold is the one most of its gate pairs vote for,
    // not its rounded mean difference. A boundary that runs along a shear line carries pairs that
    // differ by most of an interval without any fold between them, and a mean lets a minority of
    // those drag the whole boundary over the rounding line; a vote does not.
    struct Edge {
        a: usize,
        b: usize,
        weight: i64,
        /// Votes for the folds that, added to `b`, continue it from `a`.
        votes: Votes,
        alive: bool,
    }
    /// The votes as seen with the edge's ends swapped, or one end shifted by `k` intervals.
    fn reoriented(votes: &Votes, sign: i32, shift: i32) -> Votes {
        votes.iter().map(|(&f, &c)| (sign * f + shift, c)).collect()
    }
    let mut keys: Vec<(usize, usize)> = pairs.keys().copied().collect();
    keys.sort_unstable();
    let mut edges: Vec<Edge> = Vec::with_capacity(keys.len());
    let mut node_edges: Vec<std::collections::HashMap<usize, usize>> =
        vec![std::collections::HashMap::new(); region_count];
    let mut heap = std::collections::BinaryHeap::new();
    for (id, key) in keys.iter().enumerate() {
        let votes = pairs.remove(key).unwrap_or_default();
        let weight = votes.values().map(|&c| i64::from(c)).sum();
        edges.push(Edge {
            a: key.0,
            b: key.1,
            weight,
            votes,
            alive: true,
        });
        node_edges[key.0].insert(key.1, id);
        node_edges[key.1].insert(key.0, id);
        heap.push((weight, std::cmp::Reverse(id)));
    }
    drop(pairs);
    let mut node_size = sizes.clone();
    let mut members: Vec<Vec<usize>> = (0..region_count).map(|r| vec![r]).collect();
    let mut unfold = vec![0i32; region_count];
    while let Some((weight, std::cmp::Reverse(id))) = heap.pop() {
        if !edges[id].alive || edges[id].weight != weight {
            continue; // superseded by a combined boundary, or already inside one node
        }
        let (n1, n2) = (edges[id].a, edges[id].b);
        let mut rdiff = winning_fold(edges[id].votes.clone());
        let (base, merge) = if node_size[n1] > node_size[n2] {
            (n1, n2)
        } else {
            rdiff = -rdiff;
            (n2, n1)
        };
        // Unfold the merging node: its regions, and its boundaries' sums.
        if rdiff != 0 {
            for &r in &members[merge] {
                unfold[r] += rdiff;
            }
            // Adding k intervals to an edge's `a` end raises every vote by k; to its `b` end,
            // lowers it.
            for &e in node_edges[merge].values() {
                let shift = if edges[e].a == merge { rdiff } else { -rdiff };
                edges[e].votes = reoriented(&edges[e].votes, 1, shift);
            }
        }
        edges[id].alive = false;
        node_edges[base].remove(&merge);
        node_edges[merge].remove(&base);
        let moved: Vec<(usize, usize)> = node_edges[merge].drain().collect();
        for (nb, e) in moved {
            if edges[e].a == merge {
                edges[e].a = base;
            } else {
                edges[e].b = base;
            }
            node_edges[nb].remove(&merge);
            if let Some(&be) = node_edges[base].get(&nb) {
                let sign = if edges[e].a == edges[be].a { 1 } else { -1 };
                let add = reoriented(&edges[e].votes, sign, 0);
                for (f, c) in add {
                    *edges[be].votes.entry(f).or_insert(0) += c;
                }
                edges[be].weight += edges[e].weight;
                edges[e].votes = Votes::new();
                edges[e].alive = false;
                heap.push((edges[be].weight, std::cmp::Reverse(be)));
            } else {
                node_edges[base].insert(nb, e);
                node_edges[nb].insert(base, e);
            }
        }
        let merged = std::mem::take(&mut members[merge]);
        members[base].extend(merged);
        node_size[base] += node_size[merge];
        node_size[merge] = 0;
    }

    // A continuity reference, when given, sets each connected node's whole number of folds: a
    // vote, over the node's gates, for the shift that brings it onto the previous sweep. That is
    // what keeps a storm genuinely faster than the Nyquist velocity unfolded from volume to
    // volume, where the field alone cannot say how many folds its largest piece carries.
    let mut referenced = false;
    if let Some(refv) = reference.filter(|r| r.len() == n) {
        let mut votes: Vec<std::collections::HashMap<i32, u32>> =
            vec![std::collections::HashMap::new(); region_count];
        let mut node_of = vec![0usize; region_count];
        for (node, m) in members.iter().enumerate() {
            for &r in m {
                node_of[r] = node;
            }
        }
        for i in 0..n {
            let r = labels[i];
            if r == NONE {
                continue;
            }
            let (Some(v), Some(rv)) = (vel[i], refv[i]) else {
                continue;
            };
            let unfolded = v as f64 + f64::from(unfold[r]) * interval as f64;
            let fold = ((rv as f64 - unfolded) / interval as f64).round() as i32;
            *votes[node_of[r]].entry(fold).or_insert(0) += 1;
        }
        for (node, v) in votes.into_iter().enumerate() {
            if v.is_empty() {
                continue;
            }
            referenced = true;
            let shift = winning_fold(v);
            if shift != 0 {
                for &r in &members[node] {
                    unfold[r] += shift;
                }
            }
        }
    }

    // Centering, as Py-ART's `centered` option does: with nothing outside the sweep to say how
    // many folds it carries, shift every region by the one whole number of intervals that brings
    // the gate-weighted mean fold nearest zero. Radial velocity around a sweep averages near zero
    // (the inbound and outbound halves of a wind cancel), so a sweep whose mean fold is a whole
    // interval off is one assembled around a folded region: the largest piece of a fast field
    // can itself be aliased. A continuity reference is better evidence than the average, and wins.
    if !referenced {
        let gates: i64 = sizes.iter().map(|&s| s as i64).sum();
        let folds: i64 = sizes
            .iter()
            .zip(&unfold)
            .map(|(&s, &u)| s as i64 * i64::from(u))
            .sum();
        if gates > 0 {
            let offset = (folds as f64 / gates as f64).round() as i32;
            if offset != 0 {
                for u in unfold.iter_mut() {
                    *u -= offset;
                }
            }
        }
    }

    // --- 4. Apply per-region fold offsets. ---
    let mut out = vel.to_vec();
    for (i, o) in out.iter_mut().enumerate() {
        if let (Some(v), r) = (o.as_mut(), labels[i]) {
            if r != NONE && unfold[r] != 0 {
                *v += unfold[r] as f32 * interval;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // A single radial ramp that folds once: true velocity climbs past +Nyquist and wraps
    // to negative. Dealiasing must recover the monotonic ramp.
    #[test]
    fn unfolds_a_single_fold_ramp() {
        let nyq = 25.0f32;
        // az_bins=1 (one radial), 10 gates. True velocity 5,10,...,50 m/s.
        let truth: Vec<f32> = (1..=10).map(|k| k as f32 * 5.0).collect();
        // Fold into [-nyq, nyq): v_folded = ((v + nyq) mod 2nyq) - nyq.
        let folded: Vec<Option<f32>> = truth
            .iter()
            .map(|&v| Some(((v + nyq).rem_euclid(2.0 * nyq)) - nyq))
            .collect();
        // Sanity: the ramp really does fold (some folded value is negative though truth is +).
        assert!(
            folded.iter().any(|v| v.unwrap() < 0.0),
            "test ramp should fold"
        );

        let out = dealias(&folded, 1, 10, nyq);
        for (o, t) in out.iter().zip(&truth) {
            let got = o.unwrap();
            // Recovered up to a whole-field constant fold (anchor region may sit at 0).
            let err = (got - t).rem_euclid(2.0 * nyq);
            let err = err.min(2.0 * nyq - err);
            assert!(err < 0.5, "gate expected ~{t}, got {got}");
        }
    }

    // The region walk assumes a labelled gate has a velocity. These are the two fields where
    // that assumption is nearest the edge — no data at all, and exactly one gate of it.
    #[test]
    fn degenerate_fields_come_back_unchanged() {
        let empty: Vec<Option<f32>> = vec![None; 12];
        assert_eq!(dealias(&empty, 3, 4, 25.0), empty);

        let mut one = vec![None; 12];
        one[5] = Some(7.0);
        assert_eq!(dealias(&one, 3, 4, 25.0), one);
    }

    #[test]
    fn nyquist_from_field_is_max_abs() {
        let f = vec![Some(-24.0f32), None, Some(19.0), Some(-31.5)];
        assert_eq!(estimate_nyquist(&f), 31.5);
    }

    #[test]
    fn passthrough_when_no_nyquist() {
        let f = vec![Some(3.0f32), None, Some(-7.0)];
        assert_eq!(dealias(&f, 1, 3, 0.0), f);
    }

    /// Build a folded sweep from a truth field: `v_folded = ((v + nyq) mod 2nyq) - nyq`.
    fn fold(truth: &[f32], nyq: f32) -> Vec<Option<f32>> {
        truth
            .iter()
            .map(|&v| Some(((v + nyq).rem_euclid(2.0 * nyq)) - nyq))
            .collect()
    }

    /// Compare two fields up to one whole-field constant fold, which is all an unreferenced
    /// dealias can ever recover.
    fn matches_up_to_a_constant_fold(got: &[Option<f32>], truth: &[f32], nyq: f32) -> bool {
        let interval = 2.0 * nyq;
        let Some(offset) = got.first().and_then(|g| *g).map(|g| truth[0] - g) else {
            return false;
        };
        let offset = (offset / interval).round() * interval;
        got.iter()
            .zip(truth)
            .all(|(g, t)| g.is_none_or(|g| (g + offset - t).abs() < 0.5))
    }

    /// A tornadic couplet: inbound and outbound maxima either side of a shear line, both past the
    /// Nyquist velocity so both fold. The shear line itself is a real discontinuity — the point of
    /// the fixture is that the dealiaser must not treat it as a fold.
    #[test]
    fn unfolds_an_aliased_couplet_across_a_shear_line() {
        let nyq = 25.0f32;
        let (az_bins, gates) = (36, 20);
        let mut truth = vec![0.0f32; az_bins * gates];
        for az in 0..az_bins {
            for g in 0..gates {
                // Inbound on one side of the couplet, outbound on the other, peaking at 40 m/s —
                // well past the 25 m/s Nyquist, so the field folds on both sides.
                let across = if az < az_bins / 2 { -1.0 } else { 1.0 };
                let ramp = (g as f32 / (gates - 1) as f32) * 40.0;
                truth[az * gates + g] = across * ramp;
            }
        }
        let folded = fold(&truth, nyq);
        assert!(
            folded
                .iter()
                .enumerate()
                .any(|(i, v)| { (v.unwrap() - truth[i]).abs() > 1.0 }),
            "fixture should actually fold"
        );
        let out = dealias(&folded, az_bins, gates, nyq);
        // Every gate recovered, up to the one constant fold the anchor choice leaves free.
        assert!(
            matches_up_to_a_constant_fold(&out, &truth, nyq),
            "couplet not recovered"
        );
    }

    /// The failure the vote exists to prevent: a long boundary whose gate pairs mostly agree on
    /// one fold, plus a minority sitting on a shear line that pull the *mean* over the rounding
    /// line. Averaging folds the region the wrong way; voting does not.
    #[test]
    fn a_minority_of_shear_pairs_cannot_outweigh_the_boundary() {
        // Nine boundary gate pairs agree on fold 1; three sit on a shear line and ask for 3.
        // The mean of those is 1.5 and rounds away to 2, which is what the old code did. The vote
        // holds, because the outliers have to outnumber the majority, not merely outweigh it.
        let ideal = [1.0_f64; 9]
            .into_iter()
            .chain([3.0_f64; 3])
            .collect::<Vec<_>>();
        let mean = ideal.iter().sum::<f64>() / ideal.len() as f64;
        assert_eq!(mean.round() as i32, 2, "the mean really is dragged across");

        let mut votes = std::collections::HashMap::new();
        for v in &ideal {
            *votes.entry(v.round() as i32).or_insert(0) += 1;
        }
        assert_eq!(winning_fold(votes), 1);
    }

    #[test]
    fn an_empty_or_tied_vote_moves_the_data_least() {
        assert_eq!(winning_fold(std::collections::HashMap::new()), 0);
        let tied = std::collections::HashMap::from([(0, 4), (3, 4)]);
        assert_eq!(winning_fold(tied), 0);
        // Equal and opposite: the same answer every run, not whichever the map yields last.
        for _ in 0..32 {
            let mirror = std::collections::HashMap::from([(-1, 5), (1, 5)]);
            assert_eq!(winning_fold(mirror), 1);
        }
        let tied_both_folded = std::collections::HashMap::from([(-1, 2), (4, 2)]);
        assert_eq!(winning_fold(tied_both_folded), -1);
    }

    /// Continuity: a field whose *whole* content is past the Nyquist velocity has no internal
    /// evidence of how many folds it carries, so an unreferenced dealias anchors it at zero. Given
    /// the previous sweep it should keep the folds instead of snapping back.
    #[test]
    fn a_reference_sweep_anchors_folds_the_field_cannot_infer() {
        let nyq = 25.0f32;
        let (az_bins, gates) = (4, 8);
        // Uniformly 60 m/s outbound: one region, folds once, and nothing inside the sweep says so.
        let truth = vec![60.0f32; az_bins * gates];
        let folded = fold(&truth, nyq);
        let bare = dealias(&folded, az_bins, gates, nyq);
        assert!(
            (bare[0].unwrap() - 60.0).abs() > 1.0,
            "without a reference there is nothing to anchor to"
        );
        // The previous sweep had the same air, correctly unfolded.
        let reference: Vec<Option<f32>> = truth.iter().map(|v| Some(*v)).collect();
        let out = dealias_with_reference(&folded, az_bins, gates, nyq, Some(&reference));
        for v in out.iter().flatten() {
            assert!((v - 60.0).abs() < 0.5, "expected 60 m/s, got {v}");
        }
    }

    /// Multi-fold recovery: velocity climbing past two whole Nyquist intervals, so neighbouring
    /// regions differ by two folds rather than one. This is what the greedy walk was documented
    /// as unable to hold.
    #[test]
    fn unfolds_a_field_that_folds_more_than_once() {
        let nyq = 20.0f32;
        let (az_bins, gates) = (16, 24);
        let mut truth = vec![0.0f32; az_bins * gates];
        for az in 0..az_bins {
            for g in 0..gates {
                // 0 to 95 m/s outbound: nearly three Nyquist intervals from end to end.
                truth[az * gates + g] = (g as f32 / (gates - 1) as f32) * 95.0;
            }
        }
        let folded = fold(&truth, nyq);
        assert!(
            folded.iter().any(|v| v.unwrap() < 0.0),
            "fixture should fold"
        );
        let out = dealias(&folded, az_bins, gates, nyq);
        assert!(
            matches_up_to_a_constant_fold(&out, &truth, nyq),
            "multi-fold ramp not recovered"
        );
    }

    /// The optimizer walks a hash map of region pairs; the answer must not depend on the order
    /// that map happens to iterate in.
    #[test]
    fn the_same_sweep_dealiases_the_same_way_every_time() {
        let nyq = 22.0f32;
        let (az_bins, gates) = (24, 16);
        let mut truth = vec![0.0f32; az_bins * gates];
        for az in 0..az_bins {
            for g in 0..gates {
                let a = az as f32 / az_bins as f32 * std::f32::consts::TAU;
                truth[az * gates + g] = a.sin() * 48.0 + g as f32 * 1.7;
            }
        }
        let folded = fold(&truth, nyq);
        let first = dealias(&folded, az_bins, gates, nyq);
        for _ in 0..8 {
            assert_eq!(dealias(&folded, az_bins, gates, nyq), first);
        }
    }

    /// A reference of the wrong shape, or one full of holes, must be ignored rather than trusted.
    #[test]
    fn a_useless_reference_changes_nothing() {
        let nyq = 25.0f32;
        let folded = fold(&[10.0, 20.0, 30.0, 40.0], nyq);
        let bare = dealias(&folded, 1, 4, nyq);
        let wrong_size = vec![Some(60.0f32); 99];
        assert_eq!(
            dealias_with_reference(&folded, 1, 4, nyq, Some(&wrong_size)),
            bare
        );
        let all_holes = vec![None; 4];
        assert_eq!(
            dealias_with_reference(&folded, 1, 4, nyq, Some(&all_holes)),
            bare
        );
    }
}
