//! ROADMAP_NEW B6.11 step 9: deciding whether it is safe to continue an in-progress live volume
//! from a different provider than the one that started it, and deduplicating radials already
//! rendered from the old source when it is.
//!
//! The invariant this module exists to enforce, in the roadmap's own words: **"availability may
//! degrade; scientific identity may not."** A visibly brief reset is preferable to silently
//! composing radials from incompatible scans — so every decision here is conservative by
//! construction: [`check_volume_continuation`] permits continuation only on an *exact*
//! [`crate::live_block::VolumeKey`] match, never a fuzzy or partial one, and returns
//! [`ContinuationDecision::Incompatible`] for anything else, including two `VolumeKey`s that
//! merely look close. There is no "probably fine" outcome.
//!
//! This module is pure decision/bookkeeping logic — no network, no provider, no rendering. It
//! does not itself switch what `MapView` shows or reset any live assembly state; it answers the
//! two questions the caller (ROADMAP_NEW B6.11 step 11's UI/pipeline wiring, not yet built) needs
//! answered before it safely can: "can I mix these two sources' data right now?" and "have I
//! already rendered this exact radial?"

use crate::live_block::{LiveLevel2Block, RadialIdentity, VolumeKey};
use std::collections::HashSet;

/// Whether a candidate volume can be safely spliced into an in-progress live assembly that
/// started on a different provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationDecision {
    /// Identical volume identity — safe to accept the candidate's radials into the same live
    /// assembly.
    Compatible,
    /// Different (or otherwise unverifiable) volume identity — must not mix. The caller should
    /// reset live assembly at a safe sweep/volume boundary and make the discontinuity visible
    /// (ROADMAP_NEW B6.7), not silently start blending in data from a different volume.
    Incompatible,
}

/// Decide whether `candidate_volume` (e.g. the backup provider's current volume) can safely
/// continue `active_volume`'s (the currently-rendering provider's) in-progress live assembly.
///
/// Compatible only on exact equality of [`VolumeKey`] (site + volume-start truncated to the
/// second, already the identity two independently-timestamped providers are expected to agree on
/// — see `VolumeKey`'s own doc comment). ROADMAP_NEW B6.7 also lists VCP/cut and radial
/// chronology as compatibility inputs; those are properties of individual radials/cuts within a
/// volume; use [`RadialDedup`] and this volume-level check together rather than expecting this
/// one function to validate everything at once.
pub fn check_volume_continuation(
    active_volume: &VolumeKey,
    candidate_volume: &VolumeKey,
) -> ContinuationDecision {
    if active_volume == candidate_volume {
        ContinuationDecision::Compatible
    } else {
        ContinuationDecision::Incompatible
    }
}

/// Tracks which radials have already been accepted into the current live volume, so a newly
/// joined backup's overlapping coverage is not rendered twice (ROADMAP_NEW B6.7: "deduplicate
/// overlapping radials/blocks already rendered from the old source").
///
/// One `RadialDedup` per site's live volume — reset on volume rollover, the same lifetime
/// contract [`crate::live_block::CutTracker`] already has, and for the same reason: a SAILS/MRLE
/// revisit legitimately reuses an elevation number, so radial identity (which already factors in
/// [`crate::live_block::CutKey`]'s repeat index) — not just "have I seen this azimuth before at
/// all" — is what must be tracked.
#[derive(Debug, Default)]
pub struct RadialDedup {
    seen: HashSet<RadialIdentity>,
}

impl RadialDedup {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `id` has now been accepted into the live assembly.
    pub fn mark_seen(&mut self, id: RadialIdentity) {
        self.seen.insert(id);
    }

    /// Every radial identity `block` covers that has not already been seen, marking each as seen
    /// in the process — the filter a caller applies before accepting a block's radials into a
    /// live assembly that may already have overlapping coverage from another source. A block with
    /// no cut or azimuth span (e.g. a pass-through non-radial block) yields nothing to
    /// deduplicate, so it is passed through as-is by returning an empty list — callers accept
    /// such blocks unconditionally.
    pub fn accept_new(&mut self, block: &LiveLevel2Block) -> Vec<RadialIdentity> {
        let mut new_ids = Vec::new();
        for id in radial_identities(block) {
            if self.seen.insert(id.clone()) {
                new_ids.push(id);
            }
        }
        new_ids
    }

    pub fn is_duplicate(&self, id: &RadialIdentity) -> bool {
        self.seen.contains(id)
    }

    /// Start tracking a fresh volume — call this on volume rollover.
    pub fn reset(&mut self) {
        self.seen.clear();
    }
}

/// Every [`RadialIdentity`] a block covers, derived from its azimuth span. A block's cut and
/// first/last azimuth number together describe a contiguous run of radials
/// (`first_azimuth_number..=last_azimuth_number`) within one cut — see
/// [`crate::live_block::LiveLevel2Block`]'s own field docs.
///
/// Returns an empty `Vec` for a block missing a cut or azimuth span (a pass-through non-radial
/// block carries no radial identity), and for an azimuth span that wraps past the sweep's 0°
/// boundary within a single block (`first_azimuth_number > last_azimuth_number`) — correctly
/// expanding a wrapped span needs the sweep's total azimuth count, which isn't available at this
/// layer, and guessing wrong would silently skip real radials rather than merely failing to
/// deduplicate a rare edge case. In practice this only matters for a block whose azimuth span
/// itself straddles 0°, which the default flush limits (a handful of radials per block) make
/// very unlikely to ever occur.
pub fn radial_identities(block: &LiveLevel2Block) -> Vec<RadialIdentity> {
    let (Some(cut), Some(first), Some(last)) = (
        block.cut,
        block.first_azimuth_number,
        block.last_azimuth_number,
    ) else {
        return Vec::new();
    };
    if first > last {
        return Vec::new();
    }
    (first..=last)
        .map(|azimuth_number| RadialIdentity {
            volume: block.volume.clone(),
            cut,
            azimuth_number,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live_block::CutKey;
    use chrono::{TimeZone, Utc};

    fn block(
        site: &str,
        volume_start_secs: i64,
        elevation_number: u16,
        first_az: u16,
        last_az: u16,
    ) -> LiveLevel2Block {
        let t = Utc.with_ymd_and_hms(2026, 5, 1, 3, 0, 0).unwrap()
            + chrono::Duration::seconds(volume_start_secs);
        LiveLevel2Block {
            site: site.to_string(),
            volume: VolumeKey::new(site, t),
            cut: Some(CutKey {
                elevation_number,
                repeat_index: 0,
            }),
            elevation_angle_deg: Some(0.5),
            first_azimuth_number: Some(first_az),
            last_azimuth_number: Some(last_az),
            radar_start: t,
            radar_end: t,
            received_at: t,
            emitted_at: t,
            sequence: 0,
            source_id: "test".into(),
            checksum: [0u8; 32],
            payload: Vec::new(),
        }
    }

    #[test]
    fn identical_volumes_are_compatible() {
        let t = Utc::now();
        let a = VolumeKey::new("KTLX", t);
        let b = VolumeKey::new("KTLX", t);
        assert_eq!(
            check_volume_continuation(&a, &b),
            ContinuationDecision::Compatible
        );
    }

    #[test]
    fn different_sites_are_never_compatible() {
        let t = Utc::now();
        let a = VolumeKey::new("KTLX", t);
        let b = VolumeKey::new("KOHX", t);
        assert_eq!(
            check_volume_continuation(&a, &b),
            ContinuationDecision::Incompatible
        );
    }

    #[test]
    fn near_but_not_exactly_matching_volume_starts_are_incompatible() {
        // A different volume that merely started close in time to the active one — this must
        // never be treated as "close enough," per the roadmap's "if identity is ambiguous, do not
        // mix" rule. `VolumeKey` truncates to the second, so five whole seconds apart is
        // unambiguously two different volumes, not a rounding wobble.
        let t = Utc::now();
        let a = VolumeKey::new("KTLX", t);
        let b = VolumeKey::new("KTLX", t + chrono::Duration::seconds(5));
        assert_eq!(
            check_volume_continuation(&a, &b),
            ContinuationDecision::Incompatible
        );
    }

    #[test]
    fn radial_identities_expands_the_full_azimuth_span() {
        let b = block("KTLX", 0, 1, 10, 13);
        let ids = radial_identities(&b);
        assert_eq!(ids.len(), 4);
        assert_eq!(ids[0].azimuth_number, 10);
        assert_eq!(ids[3].azimuth_number, 13);
        assert!(ids
            .iter()
            .all(|id| id.volume == b.volume && id.cut == b.cut.unwrap()));
    }

    #[test]
    fn radial_identities_is_empty_for_a_pass_through_block_without_a_cut() {
        let mut b = block("KTLX", 0, 1, 0, 0);
        b.cut = None;
        b.first_azimuth_number = None;
        b.last_azimuth_number = None;
        assert!(radial_identities(&b).is_empty());
    }

    #[test]
    fn dedup_filters_overlapping_azimuths_but_keeps_new_ones() {
        let mut dedup = RadialDedup::new();
        let primary_block = block("KTLX", 0, 1, 0, 5);
        let accepted = dedup.accept_new(&primary_block);
        assert_eq!(accepted.len(), 6, "every radial in the first block is new");

        // A backup block covering azimuths 3..8 for the same cut: 3,4,5 overlap what the primary
        // already delivered; only 6 and 7 are genuinely new.
        let backup_block = block("KTLX", 0, 1, 3, 7);
        let accepted = dedup.accept_new(&backup_block);
        let azimuths: Vec<u16> = accepted.iter().map(|id| id.azimuth_number).collect();
        assert_eq!(
            azimuths,
            vec![6, 7],
            "only the non-overlapping azimuths are new"
        );
    }

    #[test]
    fn dedup_reset_starts_a_fresh_volume() {
        let mut dedup = RadialDedup::new();
        let b = block("KTLX", 0, 1, 0, 2);
        dedup.accept_new(&b);
        assert_eq!(
            dedup.accept_new(&b).len(),
            0,
            "the same block again is entirely duplicate"
        );
        dedup.reset();
        assert_eq!(
            dedup.accept_new(&b).len(),
            3,
            "after reset, a fresh volume's radials are new again"
        );
    }

    #[test]
    fn is_duplicate_reports_without_mutating_state() {
        let mut dedup = RadialDedup::new();
        let id = RadialIdentity {
            volume: VolumeKey::new("KTLX", Utc::now()),
            cut: CutKey {
                elevation_number: 1,
                repeat_index: 0,
            },
            azimuth_number: 0,
        };
        assert!(!dedup.is_duplicate(&id));
        dedup.mark_seen(id.clone());
        assert!(dedup.is_duplicate(&id));
    }

    /// ROADMAP_NEW B6.12's own acceptance-test list calls this out by name: "SAILS/MRLE repeated
    /// low-level cuts survive cross-provider deduplication correctly." A SAILS revisit of
    /// elevation 1 must not be treated as a duplicate of the base tilt's radials, even though both
    /// passes cover the same azimuth range at the same elevation number — `CutKey`'s
    /// `repeat_index` is exactly what keeps `RadialIdentity` (and therefore `RadialDedup`) from
    /// collapsing them.
    #[test]
    fn a_sails_revisit_is_not_deduplicated_against_the_base_tilt() {
        let mut dedup = RadialDedup::new();

        let mut base_tilt = block("KTLX", 0, 1, 0, 5);
        base_tilt.cut = Some(CutKey {
            elevation_number: 1,
            repeat_index: 0,
        });
        let accepted_base = dedup.accept_new(&base_tilt);
        assert_eq!(accepted_base.len(), 6);

        // Same elevation number, same azimuth range, but the SAILS revisit's own CutKey carries
        // repeat_index 1 — a different logical cut.
        let mut sails_revisit = block("KTLX", 0, 1, 0, 5);
        sails_revisit.cut = Some(CutKey {
            elevation_number: 1,
            repeat_index: 1,
        });
        let accepted_revisit = dedup.accept_new(&sails_revisit);
        assert_eq!(
            accepted_revisit.len(),
            6,
            "the SAILS revisit's radials must all be new, not deduplicated against the base tilt"
        );
    }
}
