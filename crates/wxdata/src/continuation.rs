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
//! Pure decisions/bookkeeping, with source admission used by direct and relay subscriptions
//! before accumulation and publication. Cross-provider volume/cut splicing still needs the
//! lifecycle integration and operational verification in ROADMAP_PARITY M1.3.

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

/// Admission of a source envelope before assembly. Number hints are provider-local, never clocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceAdmission {
    FirstVolume,
    CurrentVolume,
    NewVolume,
    Refused(SourceRefusal),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceRefusal {
    ForeignRadar,
    OlderVolume,
    ConflictingVolumeNumber,
}

impl std::fmt::Display for SourceRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ForeignRadar => "foreign radar",
            Self::OlderVolume => "older declared volume start",
            Self::ConflictingVolumeNumber => {
                "conflicting number for the same declared volume start"
            }
        })
    }
}

/// Immutable guard evidence attached to an accepted frame. Counts cover this subscription,
/// including non-rendering envelopes; this is not a radial/completeness or emitter-epoch claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceScopeReceipt {
    pub volume: VolumeKey,
    pub volume_number: Option<usize>,
    pub volume_rollovers: u64,
    pub declared_upstream_resets: u64,
    pub refused_older_volumes: u64,
    pub refused_foreign_radars: u64,
    pub refused_number_conflicts: u64,
}

impl SourceScopeReceipt {
    pub fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.volume.site.capacity()
    }
}

/// One provider subscription's volume high-water mark. Refusals never rewind it. Exact start
/// clocks distinguish reused native numbers; same-volume late messages remain admissible.
pub struct SourceVolumeCursor {
    site: String,
    current: Option<(VolumeKey, Option<usize>)>,
    rollovers: u64,
    upstream_resets: u64,
    older: u64,
    foreign: u64,
    conflicts: u64,
}

impl SourceVolumeCursor {
    pub fn new(site: &str) -> Self {
        Self {
            site: site.to_ascii_uppercase(),
            current: None,
            rollovers: 0,
            upstream_resets: 0,
            older: 0,
            foreign: 0,
            conflicts: 0,
        }
    }

    pub fn refuse_foreign_radar(&mut self) {
        self.foreign = self.foreign.saturating_add(1);
    }

    /// A changed declared relay label resets assembly, without establishing an emitter identity.
    pub fn declared_upstream_reset(&mut self) {
        self.upstream_resets = self.upstream_resets.saturating_add(1);
    }

    pub fn admit(&mut self, candidate: &VolumeKey, number: Option<usize>) -> SourceAdmission {
        if !candidate.site.eq_ignore_ascii_case(&self.site) {
            self.refuse_foreign_radar();
            return SourceAdmission::Refused(SourceRefusal::ForeignRadar);
        }
        if let Some((current, current_number)) = &self.current {
            if candidate.volume_start < current.volume_start {
                self.older = self.older.saturating_add(1);
                return SourceAdmission::Refused(SourceRefusal::OlderVolume);
            }
            if candidate.volume_start == current.volume_start {
                if matches!((*current_number,number),(Some(old),Some(new)) if old != new) {
                    self.conflicts = self.conflicts.saturating_add(1);
                    return SourceAdmission::Refused(SourceRefusal::ConflictingVolumeNumber);
                }
                return SourceAdmission::CurrentVolume;
            }
            self.rollovers = self.rollovers.saturating_add(1);
            self.current = Some((candidate.clone(), number));
            SourceAdmission::NewVolume
        } else {
            self.current = Some((candidate.clone(), number));
            SourceAdmission::FirstVolume
        }
    }

    pub fn receipt(&self) -> Option<SourceScopeReceipt> {
        let (volume, number) = self.current.as_ref()?;
        Some(SourceScopeReceipt {
            volume: volume.clone(),
            volume_number: *number,
            volume_rollovers: self.rollovers,
            declared_upstream_resets: self.upstream_resets,
            refused_older_volumes: self.older,
            refused_foreign_radars: self.foreign,
            refused_number_conflicts: self.conflicts,
        })
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

/// What one live assembly does with a block offered by any provider (1008.md A5): the single
/// decision a cross-provider splice applies, combining the volume identity check, volume rollover
/// and radial deduplication. Conservative in every case it cannot verify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpliceOutcome {
    /// The assembly's volume: these radials are new to it; `duplicates` were already accepted
    /// (from this provider or another) and must not be drawn twice.
    Accept {
        new: Vec<RadialIdentity>,
        duplicates: usize,
    },
    /// A newer volume started (including a VCP change, which starts a new volume): the assembly
    /// resets at this boundary and these radials begin it.
    NewVolume { new: Vec<RadialIdentity> },
    /// No radial identity to check (a metadata or pass-through block): accepted as it is.
    PassThrough,
    /// A radial block whose identities cannot be listed (an azimuth span wrapping past 0° within
    /// the block). Accepting it from a second provider could draw radials twice, so a splice
    /// refuses it; the provider that started the volume may still apply it itself.
    Unidentified,
    /// Not this assembly's: another radar, or an older volume. Never mixed in.
    Refused(SpliceRefusal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpliceRefusal {
    ForeignRadar,
    OlderVolume,
}

/// One live volume assembly fed by one or more providers. Holds the volume it is assembling and
/// every radial already accepted into it; a rollover to a newer volume clears both.
#[derive(Debug)]
pub struct CutSplice {
    site: String,
    volume: Option<VolumeKey>,
    dedup: RadialDedup,
}

impl CutSplice {
    pub fn new(site: &str) -> Self {
        Self {
            site: site.to_ascii_uppercase(),
            volume: None,
            dedup: RadialDedup::new(),
        }
    }

    /// The volume being assembled, once one has started.
    pub fn volume(&self) -> Option<&VolumeKey> {
        self.volume.as_ref()
    }

    /// Decide what to do with `block`, recording its accepted radials.
    pub fn offer(&mut self, block: &LiveLevel2Block) -> SpliceOutcome {
        if !block.volume.site.eq_ignore_ascii_case(&self.site) {
            return SpliceOutcome::Refused(SpliceRefusal::ForeignRadar);
        }
        let rollover = match &self.volume {
            None => true,
            Some(active) => match check_volume_continuation(active, &block.volume) {
                ContinuationDecision::Compatible => false,
                ContinuationDecision::Incompatible
                    if block.volume.volume_start < active.volume_start =>
                {
                    return SpliceOutcome::Refused(SpliceRefusal::OlderVolume);
                }
                ContinuationDecision::Incompatible => true,
            },
        };
        if rollover {
            self.volume = Some(block.volume.clone());
            self.dedup.reset();
        }
        let ids = radial_identities(block);
        if ids.is_empty() {
            let radial = block.cut.is_some()
                && block.first_azimuth_number.is_some()
                && block.last_azimuth_number.is_some();
            return if radial {
                SpliceOutcome::Unidentified
            } else if rollover {
                SpliceOutcome::NewVolume { new: Vec::new() }
            } else {
                SpliceOutcome::PassThrough
            };
        }
        let total = ids.len();
        let new: Vec<_> = ids
            .into_iter()
            .filter(|id| {
                let fresh = !self.dedup.is_duplicate(id);
                if fresh {
                    self.dedup.mark_seen(id.clone());
                }
                fresh
            })
            .collect();
        if rollover {
            SpliceOutcome::NewVolume { new }
        } else {
            let duplicates = total - new.len();
            SpliceOutcome::Accept { new, duplicates }
        }
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

    fn azimuths(outcome: &SpliceOutcome) -> Vec<(u16, u16, u16)> {
        let ids = match outcome {
            SpliceOutcome::Accept { new, .. } | SpliceOutcome::NewVolume { new } => new,
            _ => return Vec::new(),
        };
        ids.iter()
            .map(|id| {
                (
                    id.cut.elevation_number,
                    id.cut.repeat_index,
                    id.azimuth_number,
                )
            })
            .collect()
    }

    fn revisit(mut b: LiveLevel2Block, repeat_index: u16) -> LiveLevel2Block {
        b.cut = b.cut.map(|c| CutKey { repeat_index, ..c });
        b
    }

    /// A backup joining mid-volume adds only the radials the assembly does not have yet; the
    /// overlap it re-sends is counted as duplicates, never drawn twice.
    #[test]
    fn a_mid_volume_join_adds_only_what_is_new() {
        let mut splice = CutSplice::new("ktlx");
        let primary = block("KTLX", 0, 1, 1, 200);
        assert!(
            matches!(splice.offer(&primary), SpliceOutcome::NewVolume { ref new } if new.len() == 200)
        );
        let backup = block("KTLX", 0, 1, 150, 260);
        let out = splice.offer(&backup);
        assert_eq!(
            out,
            SpliceOutcome::Accept {
                new: (201..=260)
                    .map(|a| RadialIdentity {
                        volume: backup.volume.clone(),
                        cut: backup.cut.unwrap(),
                        azimuth_number: a,
                    })
                    .collect(),
                duplicates: 51,
            }
        );
    }

    /// The backup fills azimuths the primary missed; the primary's late arrival of the same
    /// azimuths, and any reordering between them, adds nothing twice.
    #[test]
    fn gap_fill_and_reordering_draw_each_radial_once() {
        let mut splice = CutSplice::new("KTLX");
        splice.offer(&block("KTLX", 0, 1, 1, 100));
        // The primary skips 101..=140; the backup has them.
        assert_eq!(
            azimuths(&splice.offer(&block("KTLX", 0, 1, 141, 180))).len(),
            40
        );
        let gap = splice.offer(&block("KTLX", 0, 1, 101, 140));
        assert_eq!(azimuths(&gap).len(), 40, "the gap is filled");
        // Out of order from both: everything already accepted.
        for (a, b) in [(150, 160), (101, 110), (1, 5)] {
            assert_eq!(
                splice.offer(&block("KTLX", 0, 1, a, b)),
                SpliceOutcome::Accept {
                    new: Vec::new(),
                    duplicates: (b - a + 1) as usize
                }
            );
        }
    }

    /// A repeated cut (SAILS/MRLE revisit) reuses the elevation number but is its own cut: its
    /// radials are new even where the base tilt's were seen, and its own repeats are not.
    #[test]
    fn repeated_cuts_are_their_own_radials() {
        let mut splice = CutSplice::new("KTLX");
        splice.offer(&block("KTLX", 0, 1, 1, 360));
        let sails = revisit(block("KTLX", 0, 1, 1, 360), 1);
        assert_eq!(azimuths(&splice.offer(&sails)).len(), 360);
        assert_eq!(
            splice.offer(&revisit(block("KTLX", 0, 1, 10, 20), 1)),
            SpliceOutcome::Accept {
                new: Vec::new(),
                duplicates: 11
            }
        );
        let mrle = revisit(block("KTLX", 0, 1, 10, 20), 2);
        assert_eq!(azimuths(&splice.offer(&mrle)).len(), 11);
    }

    /// A VCP change starts a new volume: the assembly resets at that boundary, so the new
    /// volume's first tilt is accepted whole; the old volume's late radials are then refused
    /// rather than mixed into the new one, from either provider.
    #[test]
    fn a_vcp_change_rolls_over_and_late_old_radials_are_refused() {
        let mut splice = CutSplice::new("KTLX");
        splice.offer(&block("KTLX", 0, 1, 1, 360));
        let new_vcp = block("KTLX", 290, 1, 1, 360);
        assert!(
            matches!(splice.offer(&new_vcp), SpliceOutcome::NewVolume { ref new } if new.len() == 360)
        );
        assert_eq!(splice.volume(), Some(&new_vcp.volume));
        assert_eq!(
            splice.offer(&block("KTLX", 0, 2, 1, 10)),
            SpliceOutcome::Refused(SpliceRefusal::OlderVolume)
        );
        assert_eq!(
            splice.offer(&block("KINX", 290, 1, 1, 10)),
            SpliceOutcome::Refused(SpliceRefusal::ForeignRadar)
        );
        // A second provider whose volume start differs by under a second is the same volume
        // (`VolumeKey` keeps whole seconds); one a second later is a different volume.
        let mut late = block("KTLX", 290, 1, 1, 10);
        late.volume = VolumeKey::new(
            "KTLX",
            new_vcp.volume.volume_start + chrono::Duration::milliseconds(400),
        );
        assert_eq!(
            splice.offer(&late),
            SpliceOutcome::Accept {
                new: Vec::new(),
                duplicates: 10
            }
        );
    }

    /// Blocks with nothing to deduplicate pass through; a radial block whose span cannot be
    /// listed is not spliced.
    #[test]
    fn unlisted_radials_are_not_spliced() {
        let mut splice = CutSplice::new("KTLX");
        splice.offer(&block("KTLX", 0, 1, 1, 10));
        let mut meta = block("KTLX", 0, 1, 1, 10);
        meta.cut = None;
        assert_eq!(splice.offer(&meta), SpliceOutcome::PassThrough);
        let wrapped = block("KTLX", 0, 1, 719, 3);
        assert_eq!(splice.offer(&wrapped), SpliceOutcome::Unidentified);
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
    #[test]
    fn source_cursor_uses_start_clocks_not_reused_or_wrapped_numbers_and_refusals_never_rewind() {
        use SourceAdmission::*;
        let start = Utc.with_ymd_and_hms(2026, 5, 1, 3, 0, 0).unwrap();
        let mut cursor = SourceVolumeCursor::new("ktlx");
        let first = VolumeKey::new("KTLX", start);
        let next = VolumeKey::new("KTLX", start + chrono::Duration::minutes(5));
        assert_eq!(cursor.admit(&first, Some(999)), FirstVolume);
        let frozen = cursor.receipt().unwrap();
        assert_eq!(
            cursor.admit(&first, Some(999)),
            CurrentVolume,
            "a repeated Start is not another volume"
        );
        assert_eq!(
            cursor.admit(&next, Some(999)),
            NewVolume,
            "a missing Start and reused number still roll over"
        );
        assert_eq!(
            cursor.admit(&first, Some(999)),
            Refused(SourceRefusal::OlderVolume)
        );
        assert_eq!(
            cursor.admit(&next, Some(1)),
            Refused(SourceRefusal::ConflictingVolumeNumber)
        );
        assert_eq!(
            cursor.admit(&VolumeKey::new("KOUN", next.volume_start), Some(999)),
            Refused(SourceRefusal::ForeignRadar)
        );
        let current = cursor.receipt().unwrap();
        assert_eq!(current.volume, next);
        assert_eq!(
            (
                current.volume_rollovers,
                current.refused_older_volumes,
                current.refused_number_conflicts,
                current.refused_foreign_radars
            ),
            (1, 1, 1, 1)
        );
        assert_eq!(
            frozen.volume, first,
            "accepted guard evidence stays immutable"
        );
        assert_eq!(frozen.refused_older_volumes, 0);
        assert_eq!(
            cursor.admit(
                &VolumeKey::new("KTLX", start + chrono::Duration::minutes(10)),
                Some(1)
            ),
            NewVolume,
            "number wrap is ordinary when the clock advances"
        );
    }

    #[test]
    fn relay_cursor_has_no_invented_number_or_epoch_and_preserves_same_volume_late_input() {
        let mut cursor = SourceVolumeCursor::new("KTLX");
        assert!(cursor.receipt().is_none());
        cursor.refuse_foreign_radar();
        let volume = VolumeKey::new("KTLX", Utc.with_ymd_and_hms(2026, 5, 1, 3, 0, 0).unwrap());
        cursor.admit(&volume, None);
        cursor.declared_upstream_reset();
        assert_eq!(cursor.admit(&volume, None), SourceAdmission::CurrentVolume);
        let receipt = cursor.receipt().unwrap();
        assert_eq!(receipt.volume_number, None);
        assert_eq!(receipt.declared_upstream_resets, 1);
        assert_eq!(receipt.refused_foreign_radars, 1);
        assert_eq!(receipt.volume_rollovers, 0);
    }
}
