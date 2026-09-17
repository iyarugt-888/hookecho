//! ROADMAP_NEW B6 foundation: a provider-neutral radar-volume/cut/radial identity model and the
//! canonical `LiveLevel2Block` envelope both live acquisition paths (the existing Unidata/AWS
//! chunk stream and the new HookEcho `radar-ingest` relay) normalize into before the data reaches
//! the app's existing decode/merge/render pipeline (`level2.rs`, `live.rs`'s `merge_scan`/
//! `stitch`, `view::Volume`).
//!
//! This module defines identity and provenance only — it does not decode Level II, does not touch
//! the network, and does not know about any one provider. `wxdata::live`'s existing
//! `Update`/`ScanProgress`/`stream()` (the Unidata implementation) are unchanged; B6 wraps them
//! rather than replacing them.
//!
//! Two problems this module exists to solve up front, before any failover logic can be safe:
//!
//! 1. **What counts as "the same cut"** when two independent acquisition paths both claim to have
//!    radials for elevation number 2 of KTLX's current volume — including when that elevation
//!    number has already appeared once this volume via a SAILS/MRLE mid-volume revisit, so a
//!    naive "elevation number matches" check would wrongly collapse two legitimate passes into
//!    one. See [`CutTracker`].
//! 2. **What makes two blocks the same radial** for deduplication when both an old and a newly
//!    promoted acquisition path have delivered overlapping coverage. See [`RadialIdentity`].

use chrono::{DateTime, SubsecRound, Utc};

/// What one acquisition path can offer, independent of which concrete provider it is. The
/// per-site arbiter (B6.6) reasons about capabilities, not provider names, so a third progressive
/// source added later needs no arbiter changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCapabilities {
    /// Delivers sub-volume radial data as it arrives, not just completed volumes.
    pub progressive_radials: bool,
    /// Supports resuming a stream after a short disconnect without re-delivering everything
    /// already received (a sequence/cursor-based reconnect).
    pub resume: bool,
    /// Can serve a fully completed volume (even if it cannot stream progressively).
    pub completed_volume: bool,
    /// Can serve volumes from before "now" (backfill/catch-up), not just the live edge.
    pub historical_backfill: bool,
    /// The provider pushes updates to the client rather than requiring the client to poll.
    pub server_push: bool,
}

impl ProviderCapabilities {
    /// The existing Unidata/AWS live-chunk path (`UnidataLevel2Provider`): progressive, no
    /// resume (a dropped chunk stream restarts the `ChunkIterator` from the current volume rather
    /// than a resumable cursor — see `wxdata::live::stream`), serves completed volumes via
    /// `latest_complete_volume`, and is push-like in practice (S3 polling tight enough to read as
    /// live) without a true server-push transport.
    pub const fn unidata() -> Self {
        Self {
            progressive_radials: true,
            resume: false,
            completed_volume: true,
            historical_backfill: false,
            server_push: false,
        }
    }

    /// The HookEcho `radar-ingest` relay (B6.2-B6.4): progressive, resumable by sequence, serves
    /// completed volumes assembled from the same retained bytes, and pushes over its live stream
    /// transport.
    pub const fn relay() -> Self {
        Self {
            progressive_radials: true,
            resume: true,
            completed_volume: true,
            historical_backfill: false,
            server_push: true,
        }
    }

    /// NOAA TGFTP (B6.8): completed volumes only, polled — the last-resort degraded-continuity
    /// path, never a progressive source.
    pub const fn tgftp() -> Self {
        Self {
            progressive_radials: false,
            resume: false,
            completed_volume: true,
            historical_backfill: false,
            server_push: false,
        }
    }
}

/// Stable, provider-neutral identity for one radar volume. Two providers describing the same
/// physical volume must produce equal `VolumeKey`s even though their own object-naming schemes
/// differ (an AWS chunk-stream object name vs. an LDM product ID vs. a TGFTP filename) — this is
/// the join key the failover arbiter uses to ask "is the backup on the same volume as the
/// primary?"
///
/// `volume_start` is truncated to whole seconds: two providers timestamping the same physical
/// volume start rarely agree to the millisecond (different receive/decode paths), but a NEXRAD
/// volume start is itself only ever reported to the second.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VolumeKey {
    pub site: String,
    pub volume_start: DateTime<Utc>,
}

impl VolumeKey {
    pub fn new(site: impl Into<String>, volume_start: DateTime<Utc>) -> Self {
        Self {
            site: site.into().to_ascii_uppercase(),
            volume_start: volume_start.trunc_subsecs(0),
        }
    }
}

/// Stable identity for one elevation cut within a volume: the elevation number NEXRAD's own
/// message headers report, plus which pass through that elevation number this is. `repeat_index`
/// is `0` for a volume's base pass at this elevation number and `1`, `2`, … for each SAILS/MRLE
/// mid-volume revisit — see [`CutTracker`] for how this gets assigned from an arrival-ordered
/// stream rather than guessed from the angle alone (a SAILS insert reports the *same* elevation
/// angle as the volume's base low tilt, so angle alone cannot tell them apart).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CutKey {
    pub elevation_number: u16,
    pub repeat_index: u16,
}

/// Assigns each elevation-number observation within one volume its [`CutKey`], in arrival order.
/// Stateful because "which pass is this" is a property of the *sequence* of cuts seen so far in
/// the current volume, not of one observation in isolation — a fresh `CutTracker` per
/// `VolumeKey` (reset on volume rollover) is the intended lifetime.
///
/// This does not read `wxdata::level2::tilt_cuts`/`TiltCuts` (the VCP coverage pattern's own
/// declared SAILS/MRLE cut counts): that requires a decoded `Scan`'s VCP message, which arrives
/// once at volume start, whereas a live block's own elevation number is known the instant it
/// arrives — `CutTracker` only needs to count repeats, not know the VCP's own labels for them, to
/// give every legitimate pass a distinct, stable identity.
#[derive(Debug, Default)]
pub struct CutTracker {
    seen: std::collections::HashMap<u16, u16>,
}

impl CutTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one observation of `elevation_number` and return its `CutKey`. Calling this again
    /// with the same `elevation_number` — a SAILS/MRLE revisit — returns a `CutKey` with the next
    /// `repeat_index`, never the same key twice.
    pub fn observe(&mut self, elevation_number: u16) -> CutKey {
        let repeat_index = self.seen.entry(elevation_number).or_insert(0);
        let key = CutKey {
            elevation_number,
            repeat_index: *repeat_index,
        };
        *repeat_index += 1;
        key
    }

    /// Start a new volume: every elevation number's repeat count resets to zero. Call this on
    /// volume rollover (a new `VolumeKey`), not on every cut — an uncalled reset mid-volume is
    /// exactly the bug this type exists to prevent.
    pub fn reset(&mut self) {
        self.seen.clear();
    }
}

/// Stable identity for one radial within one cut, for cross-provider deduplication. Payload
/// content hash alone is not used as identity (two providers can losslessly re-encode/re-frame
/// the same physical radial into different bytes) — the *meteorological* identity is the tuple of
/// where and when the radar pointed, and that's what this compares.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RadialIdentity {
    pub volume: VolumeKey,
    pub cut: CutKey,
    /// NEXRAD's own azimuth number for this radial (0..(azimuths per sweep)), not the raw azimuth
    /// angle — two providers rarely round the angle identically, but both decode the same
    /// message-level azimuth number.
    pub azimuth_number: u16,
}

/// Why the failover arbiter changed which provider is active for a site (B6.6/B6.9). Recorded in
/// provenance/diagnostics on every transition, never inferred after the fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSwitchReason {
    /// The active provider's transport failed (connection error, exhausted retries).
    TransportError,
    /// The active provider is still connected but its data has fallen behind the configured
    /// freshness threshold.
    StaleData,
    /// A gap in the active provider's own sequence/object numbering was detected.
    SequenceGap,
    /// The user explicitly picked a provider in Advanced settings.
    ManualOverride,
    /// The preferred provider recovered and passed the hysteresis check to fail back to it.
    Recovery,
}

/// A canonical, provider-neutral radial block — the unit `radar-ingest`'s rechunker (B6.3) and
/// distribution protocol (B6.4) hand to clients, and what any `Level2LiveProvider` normalizes its
/// own transport into. `payload` is the original lossless Level II message bytes for this span —
/// this type carries transport/provenance metadata *around* that payload, never a resampled or
/// reformatted radar value (ROADMAP_NEW B6.3's explicit fidelity requirement).
#[derive(Debug, Clone)]
pub struct LiveLevel2Block {
    pub site: String,
    pub volume: VolumeKey,
    pub cut: Option<CutKey>,
    pub elevation_angle_deg: Option<f32>,
    pub first_azimuth_number: Option<u16>,
    pub last_azimuth_number: Option<u16>,
    /// Wall-clock span the radar itself covered while collecting this block's radials — not when
    /// any of this was received or processed.
    pub radar_start: DateTime<Utc>,
    pub radar_end: DateTime<Utc>,
    /// When the emitting side (a relay backend, or the client itself for the direct Unidata path)
    /// first received the bytes this block is built from.
    pub received_at: DateTime<Utc>,
    /// When this block was emitted onto the distribution stream — always `>= received_at`; the
    /// difference is backend processing/rechunk latency (ROADMAP_NEW B6.3's instrumentation ask).
    pub emitted_at: DateTime<Utc>,
    /// Monotonic per-site sequence number from the emitting provider, for resume/dedup ordering.
    /// Two different providers' sequence numbers are never compared to each other — only
    /// `RadialIdentity`/`VolumeKey`/`CutKey` are cross-provider identity; `sequence` is
    /// provider-local.
    pub sequence: u64,
    /// Which acquisition path produced this block (e.g. `"unidata"`, `"relay"`, `"tgftp"`) — the
    /// per-block provenance stamp ROADMAP_NEW B6.5 requires so a provider switch mid-volume is
    /// inspectable after the fact.
    pub source_id: String,
    pub checksum: [u8; 32],
    pub payload: Vec<u8>,
}

/// SHA-256 of `payload`, for transport integrity and de-duplication of byte-identical blocks
/// (e.g. the same relay-rechunked block re-delivered after a resume). Not used as meteorological
/// identity by itself — see [`RadialIdentity`] and this module's own doc comment.
pub fn checksum(payload: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(payload);
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_distinguish_the_three_known_providers() {
        let unidata = ProviderCapabilities::unidata();
        let relay = ProviderCapabilities::relay();
        let tgftp = ProviderCapabilities::tgftp();
        assert!(unidata.progressive_radials && !unidata.resume);
        assert!(relay.progressive_radials && relay.resume && relay.server_push);
        assert!(!tgftp.progressive_radials && tgftp.completed_volume);
        // Every known provider is a genuinely distinct capability set — if two collapsed to the
        // same bundle the arbiter would have no way to prefer one over the other on capability
        // grounds.
        assert_ne!(unidata, relay);
        assert_ne!(relay, tgftp);
        assert_ne!(unidata, tgftp);
    }

    #[test]
    fn volume_key_truncates_to_the_second_so_near_identical_timestamps_still_match() {
        use chrono::TimeZone;
        let a = Utc.with_ymd_and_hms(2026, 5, 1, 3, 0, 0).unwrap()
            + chrono::Duration::milliseconds(120);
        let b = Utc.with_ymd_and_hms(2026, 5, 1, 3, 0, 0).unwrap()
            + chrono::Duration::milliseconds(890);
        assert_eq!(VolumeKey::new("ktlx", a), VolumeKey::new("KTLX", b));
    }

    #[test]
    fn volume_key_site_is_case_insensitive() {
        let t = Utc::now();
        assert_eq!(VolumeKey::new("ktlx", t), VolumeKey::new("KTLX", t));
    }

    #[test]
    fn volume_key_differs_by_site_or_start_time() {
        use chrono::TimeZone;
        let t0 = Utc.with_ymd_and_hms(2026, 5, 1, 3, 0, 0).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 5, 1, 3, 5, 0).unwrap();
        assert_ne!(VolumeKey::new("KTLX", t0), VolumeKey::new("KOHX", t0));
        assert_ne!(VolumeKey::new("KTLX", t0), VolumeKey::new("KTLX", t1));
    }

    #[test]
    fn cut_tracker_gives_a_base_tilt_repeat_index_zero() {
        let mut t = CutTracker::new();
        assert_eq!(
            t.observe(1),
            CutKey {
                elevation_number: 1,
                repeat_index: 0
            }
        );
        assert_eq!(
            t.observe(2),
            CutKey {
                elevation_number: 2,
                repeat_index: 0
            }
        );
    }

    #[test]
    fn cut_tracker_gives_a_sails_revisit_a_distinct_repeat_index() {
        // A VCP with a SAILS insert: elevation 1 (the low tilt), then several higher tilts, then
        // elevation 1 again mid-volume. Both passes must be individually addressable.
        let mut t = CutTracker::new();
        let base = t.observe(1);
        let _ = t.observe(2);
        let _ = t.observe(3);
        let sails = t.observe(1);
        assert_ne!(
            base, sails,
            "two legitimate passes must not collapse into one"
        );
        assert_eq!(base.repeat_index, 0);
        assert_eq!(sails.repeat_index, 1);
        assert_eq!(base.elevation_number, sails.elevation_number);
    }

    #[test]
    fn cut_tracker_handles_mrle_style_multiple_revisits() {
        let mut t = CutTracker::new();
        let passes: Vec<u16> = (0..4).map(|_| t.observe(1).repeat_index).collect();
        assert_eq!(passes, vec![0, 1, 2, 3]);
    }

    #[test]
    fn cut_tracker_reset_starts_a_fresh_volume() {
        let mut t = CutTracker::new();
        let _ = t.observe(1);
        let _ = t.observe(1);
        t.reset();
        // A brand-new volume's first elevation-1 sweep is the base tilt again, not a "third pass"
        // held over from the previous volume.
        assert_eq!(t.observe(1).repeat_index, 0);
    }

    #[test]
    fn radial_identity_distinguishes_azimuth_within_the_same_cut() {
        let volume = VolumeKey::new("KTLX", Utc::now());
        let cut = CutKey {
            elevation_number: 1,
            repeat_index: 0,
        };
        let a = RadialIdentity {
            volume: volume.clone(),
            cut,
            azimuth_number: 10,
        };
        let b = RadialIdentity {
            volume,
            cut,
            azimuth_number: 11,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn checksum_is_deterministic_and_content_sensitive() {
        let a = checksum(b"hello");
        let b = checksum(b"hello");
        let c = checksum(b"hellO");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
