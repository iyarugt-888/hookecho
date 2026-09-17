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

/// Reassembles a [`crate::level2::Scan`] from a set of canonical blocks — the piece a
/// `Level2LiveProvider` for the HookEcho `radar-ingest` relay (ROADMAP_NEW B6.11 step 6) needs to
/// turn received blocks back into something the existing render pipeline already understands.
///
/// Reuses `nexrad-data`'s own real-time assembly (`nexrad_data::aws::realtime::assemble_volume`)
/// rather than a parallel decoder, per this engagement's explicit "reuse existing radar
/// decode/assembly infra" rule: each block's raw, uncompressed message bytes are wrapped as an
/// `IntermediateOrEnd` LDM record and handed straight to it. This works even though nothing here
/// produces a "Start" chunk (with its Archive II volume header) because `assemble_volume` only
/// *needs* one for the site identifier, which is optional — it still builds a `Scan` from whichever
/// chunks carry a VCP (Volume Coverage Pattern) message and digital radar data, which every one of
/// [`crate::live_block`]'s blocks legitimately might (radial blocks and pass-through blocks alike).
///
/// Blocks are consumed in the order given; callers are responsible for supplying them in arrival
/// (sequence) order — this does not sort them, matching [`CutTracker`]'s own "arrival order is the
/// only order that matters" stance.
///
/// Fails if none of the given blocks carries a VCP message, since `assemble_volume` itself cannot
/// build a `Scan` without one — the same requirement the existing Unidata live path already has.
pub fn assemble_scan(blocks: &[LiveLevel2Block]) -> anyhow::Result<crate::level2::Scan> {
    use nexrad_data::aws::realtime::{assemble_volume, Chunk};
    use nexrad_data::volume::Record;

    let chunks = blocks
        .iter()
        .map(|b| Chunk::IntermediateOrEnd(Record::from_slice(&b.payload)));
    assemble_volume(chunks).map_err(|e| anyhow::anyhow!("assembling relay blocks into a scan: {e}"))
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

    /// Builds the raw bytes of a minimal (zero data blocks) NEXRAD Level II Message Type 31
    /// "Digital Radar Data" — see `radar-ingest`'s `rechunk::test_support::synthetic_radial` for
    /// the from-first-principles derivation of this exact byte layout (28-byte transport header +
    /// 32-byte digital-radar-data header); duplicated here rather than shared across crates
    /// because wxdata sits below radar-ingest in the dependency graph.
    fn synthetic_radial(
        site: &str,
        elevation_number: u8,
        azimuth_number: u16,
        radial_status: u8,
        time: DateTime<Utc>,
    ) -> Vec<u8> {
        let mut msg = Vec::with_capacity(60);
        let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
        let date_field = ((time.date_naive() - epoch).num_days() + 1) as u16;
        let midnight = time.date_naive().and_hms_opt(0, 0, 0).unwrap();
        let ms_past_midnight = (time.naive_utc() - midnight).num_milliseconds() as u32;

        msg.extend_from_slice(&[0u8; 12]);
        msg.extend_from_slice(&0u16.to_be_bytes());
        msg.push(8);
        msg.push(31);
        msg.extend_from_slice(&1u16.to_be_bytes());
        msg.extend_from_slice(&date_field.to_be_bytes());
        msg.extend_from_slice(&ms_past_midnight.to_be_bytes());
        msg.extend_from_slice(&0u16.to_be_bytes());
        msg.extend_from_slice(&0u16.to_be_bytes());

        let mut id = [b' '; 4];
        for (dst, src) in id.iter_mut().zip(site.as_bytes()) {
            *dst = *src;
        }
        msg.extend_from_slice(&id);
        msg.extend_from_slice(&ms_past_midnight.to_be_bytes());
        msg.extend_from_slice(&date_field.to_be_bytes());
        msg.extend_from_slice(&azimuth_number.to_be_bytes());
        msg.extend_from_slice(&0f32.to_be_bytes());
        msg.push(0);
        msg.push(0);
        msg.extend_from_slice(&0u16.to_be_bytes());
        msg.push(1);
        msg.push(radial_status);
        msg.push(elevation_number);
        msg.push(0);
        msg.extend_from_slice(&0f32.to_be_bytes());
        msg.push(0);
        msg.push(0);
        msg.extend_from_slice(&0u16.to_be_bytes());
        msg
    }

    /// Builds the raw bytes of a minimal, single-elevation NEXRAD Level II Message Type 5
    /// "Volume Coverage Pattern" — a fixed-segment message (28-byte transport header + a
    /// single 2432-byte frame whose content area holds the 22-byte VCP header followed by one
    /// 46-byte elevation data block, zero-padded the rest of the way). Field values beyond
    /// `pattern_type` (which the ICD fixes at 2) and `number_of_elevation_cuts` are arbitrary —
    /// `assemble_scan`'s test only needs a VCP that decodes at all, not a scientifically
    /// meaningful one.
    fn synthetic_vcp(time: DateTime<Utc>) -> Vec<u8> {
        const FRAME_SIZE: usize = 2432;
        const HEADER_SIZE: usize = 28;
        let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
        let date_field = ((time.date_naive() - epoch).num_days() + 1) as u16;
        let midnight = time.date_naive().and_hms_opt(0, 0, 0).unwrap();
        let ms_past_midnight = (time.naive_utc() - midnight).num_milliseconds() as u32;

        let mut frame = vec![0u8; FRAME_SIZE];
        // Outer 28-byte transport header: a single-segment fixed frame.
        frame[12..14].copy_from_slice(&((FRAME_SIZE / 2) as u16).to_be_bytes()); // segment_size
        frame[14] = 8; // redundant_channel
        frame[15] = 5; // message_type = Volume Coverage Pattern
        frame[16..18].copy_from_slice(&1u16.to_be_bytes()); // sequence_number
        frame[18..20].copy_from_slice(&date_field.to_be_bytes());
        frame[20..24].copy_from_slice(&ms_past_midnight.to_be_bytes());
        frame[24..26].copy_from_slice(&1u16.to_be_bytes()); // segment_count
        frame[26..28].copy_from_slice(&1u16.to_be_bytes()); // segment_number

        // VCP content: 22-byte header + one 46-byte elevation data block.
        let content = HEADER_SIZE;
        frame[content..content + 2].copy_from_slice(&68u16.to_be_bytes()); // message_size (halfwords, unused by the parser)
        frame[content + 2..content + 4].copy_from_slice(&2u16.to_be_bytes()); // pattern_type = 2 (fixed by the ICD)
        frame[content + 4..content + 6].copy_from_slice(&12u16.to_be_bytes()); // pattern_number
        frame[content + 6..content + 8].copy_from_slice(&1u16.to_be_bytes()); // number_of_elevation_cuts
        frame[content + 8] = 1; // version
                                // clutter_map_group_number, doppler_velocity_resolution, pulse_width, reserved_1,
                                // vcp_sequencing, vcp_supplemental_data, reserved_2 all left zero — none of them
                                // gate whether `assemble_volume` succeeds.
                                // The single elevation data block starts right after the 22-byte header; every field left
                                // zero decodes to a valid (if scientifically meaningless) cut.
        frame
    }

    #[test]
    fn assemble_scan_fails_without_a_vcp_among_the_blocks() {
        let t = Utc::now();
        let block = LiveLevel2Block {
            site: "KTLX".into(),
            volume: VolumeKey::new("KTLX", t),
            cut: Some(CutKey {
                elevation_number: 1,
                repeat_index: 0,
            }),
            elevation_angle_deg: Some(0.5),
            first_azimuth_number: Some(0),
            last_azimuth_number: Some(0),
            radar_start: t,
            radar_end: t,
            received_at: t,
            emitted_at: t,
            sequence: 0,
            source_id: "relay".into(),
            checksum: checksum(&synthetic_radial("KTLX", 1, 0, 3, t)),
            payload: synthetic_radial("KTLX", 1, 0, 3, t),
        };
        assert!(
            assemble_scan(&[block]).is_err(),
            "a scan cannot be built without a VCP message among the blocks"
        );
    }

    #[test]
    fn assemble_scan_builds_a_scan_from_radial_and_vcp_blocks() {
        let t = Utc::now();
        let vcp_block = LiveLevel2Block {
            site: "KTLX".into(),
            volume: VolumeKey::new("KTLX", t),
            cut: None,
            elevation_angle_deg: None,
            first_azimuth_number: None,
            last_azimuth_number: None,
            radar_start: t,
            radar_end: t,
            received_at: t,
            emitted_at: t,
            sequence: 0,
            source_id: "relay".into(),
            checksum: checksum(&synthetic_vcp(t)),
            payload: synthetic_vcp(t),
        };
        let radial_payload = synthetic_radial("KTLX", 1, 0, 3, t);
        let radial_block = LiveLevel2Block {
            site: "KTLX".into(),
            volume: VolumeKey::new("KTLX", t),
            cut: Some(CutKey {
                elevation_number: 1,
                repeat_index: 0,
            }),
            elevation_angle_deg: Some(0.5),
            first_azimuth_number: Some(0),
            last_azimuth_number: Some(0),
            radar_start: t,
            radar_end: t,
            received_at: t,
            emitted_at: t,
            sequence: 1,
            source_id: "relay".into(),
            checksum: checksum(&radial_payload),
            payload: radial_payload,
        };

        let scan = assemble_scan(&[vcp_block, radial_block])
            .expect("a VCP block plus a radial block must assemble into a scan");
        assert_eq!(
            scan.sweeps().len(),
            1,
            "the one radial's elevation must become one sweep"
        );
    }
}
