//! Turns raw ingested products into canonical, lossless [`LiveLevel2Block`]s (ROADMAP_NEW B6.3).
//!
//! This is the seam between "opaque bytes routed by site" ([`crate::store`]) and the
//! provider-neutral identity model both acquisition paths converge on
//! ([`wxdata::live_block`]). It parses just enough of each NEXRAD Level II message to know which
//! volume/cut/radial it belongs to — via [`nexrad_decode::messages::decode_messages`], the same
//! message-level decoder `nexrad-data` uses internally for whole-volume decode — and re-emits the
//! **original message bytes verbatim** as a block's payload; nothing here resamples, recompresses,
//! or reformats a radar value.
//!
//! A raw product's bytes are assumed to already be one or more back-to-back NEXRAD Level II
//! messages with no additional framing — stripping any LDM/IDD-specific transport framing (a CTM
//! header, product headers) before handing bytes to [`Rechunker::ingest`] is the input adapter's
//! job (B6.11 step 5's live LDM adapter), the same way [`crate::input::ReplayInputAdapter`]'s
//! fixtures already are.
//!
//! Blocks are flushed on three independent triggers, per B6.3's "emit sub-volume blocks
//! immediately" and "do not hold a partially filled block indefinitely" requirements:
//!
//! 1. **Identity boundary** — a radial reports the start of a new elevation cut or volume scan.
//!    A block never spans two [`CutKey`]s; canonical reordering/merging across blocks is the
//!    downstream assembler's job, not this one's.
//! 2. **Size limit** — [`RechunkConfig::max_radials_per_block`] radials have accumulated.
//! 3. **Age limit** — [`Rechunker::tick`], called periodically by the service loop independent of
//!    new data arriving, flushes anything older than [`RechunkConfig::max_block_age`].
//!
//! Any message this module doesn't need to inspect for identity (VCP, RDA status, and so on) is
//! still forwarded, verbatim, as its own single-message block — B6.3's fidelity requirement means
//! nothing arriving on the wire is silently dropped, even when this layer has nothing to say about
//! its meteorological identity beyond "whichever volume/cut this site is currently on."

use crate::input::RawProduct;
use chrono::{DateTime, Utc};
use nexrad_decode::messages::digital_radar_data::RadialStatus;
use nexrad_decode::messages::MessageContents;
use std::collections::HashMap;
use wxdata::live_block::{checksum, CutKey, CutTracker, LiveLevel2Block, VolumeKey};

/// Flush thresholds. A deployment tunes these from configuration (B6.10), not code — smaller
/// values trade a little framing overhead for lower latency to the first visible radial.
#[derive(Debug, Clone)]
pub struct RechunkConfig {
    pub max_radials_per_block: usize,
    pub max_block_age: chrono::Duration,
}

impl Default for RechunkConfig {
    fn default() -> Self {
        Self {
            // Small enough that a client sees radials within a fraction of an elevation sweep;
            // large enough not to dominate a block's byte count with per-block framing overhead.
            max_radials_per_block: 8,
            max_block_age: chrono::Duration::seconds(2),
        }
    }
}

/// A site's current position in the stream, queryable independent of any one block — the
/// "manifest" B6.4's future per-site head endpoint serves from.
#[derive(Debug, Clone, Default)]
pub struct SiteManifest {
    /// Sequence number of the most recent block emitted for this site, if any.
    pub newest_sequence: Option<u64>,
    pub current_volume: Option<VolumeKey>,
    pub current_cut: Option<CutKey>,
    /// The most recent volume this site has completed (seen a `VolumeScanEnd` radial for), if
    /// any — distinct from `current_volume`, which may already be a newer, still-in-progress one.
    pub latest_complete_volume: Option<VolumeKey>,
}

/// Radials (or, for a pass-through message, exactly one message) accumulating toward the next
/// emitted block for one site.
struct PendingBlock {
    volume: VolumeKey,
    cut: Option<CutKey>,
    elevation_angle_deg: Option<f32>,
    first_azimuth_number: Option<u16>,
    last_azimuth_number: Option<u16>,
    radar_start: DateTime<Utc>,
    radar_end: DateTime<Utc>,
    received_at: DateTime<Utc>,
    payload: Vec<u8>,
    radial_count: usize,
    /// Backend processing wall-clock time this pending block was started — what
    /// [`Rechunker::tick`]'s age limit measures against, independent of any radar timestamp.
    started_at: DateTime<Utc>,
}

impl PendingBlock {
    fn start(
        volume: VolumeKey,
        cut: Option<CutKey>,
        elevation_angle_deg: Option<f32>,
        radar_time: DateTime<Utc>,
        received_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            volume,
            cut,
            elevation_angle_deg,
            first_azimuth_number: None,
            last_azimuth_number: None,
            radar_start: radar_time,
            radar_end: radar_time,
            received_at,
            payload: Vec::new(),
            radial_count: 0,
            started_at: now,
        }
    }

    fn push_radial(&mut self, bytes: &[u8], azimuth_number: u16, radar_time: DateTime<Utc>) {
        self.payload.extend_from_slice(bytes);
        self.radial_count += 1;
        self.first_azimuth_number.get_or_insert(azimuth_number);
        self.last_azimuth_number = Some(azimuth_number);
        if radar_time < self.radar_start {
            self.radar_start = radar_time;
        }
        if radar_time > self.radar_end {
            self.radar_end = radar_time;
        }
    }

    fn finish(self, site: &str, source_id: &str, sequence: u64) -> LiveLevel2Block {
        let checksum = checksum(&self.payload);
        LiveLevel2Block {
            site: site.to_string(),
            volume: self.volume,
            cut: self.cut,
            elevation_angle_deg: self.elevation_angle_deg,
            first_azimuth_number: self.first_azimuth_number,
            last_azimuth_number: self.last_azimuth_number,
            radar_start: self.radar_start,
            radar_end: self.radar_end,
            received_at: self.received_at,
            emitted_at: Utc::now(),
            sequence,
            source_id: source_id.to_string(),
            checksum,
            payload: self.payload,
        }
    }
}

#[derive(Default)]
struct SiteState {
    cut_tracker: CutTracker,
    current_volume: Option<VolumeKey>,
    current_cut: Option<CutKey>,
    latest_complete_volume: Option<VolumeKey>,
    pending: Option<PendingBlock>,
    sequence: u64,
}

impl SiteState {
    fn next_sequence(&mut self) -> u64 {
        let seq = self.sequence;
        self.sequence += 1;
        seq
    }

    fn manifest(&self) -> SiteManifest {
        SiteManifest {
            newest_sequence: if self.sequence == 0 {
                None
            } else {
                Some(self.sequence - 1)
            },
            current_volume: self.current_volume.clone(),
            current_cut: self.current_cut,
            latest_complete_volume: self.latest_complete_volume.clone(),
        }
    }
}

/// Parses raw products into canonical [`LiveLevel2Block`]s, one [`SiteState`] per site.
pub struct Rechunker {
    config: RechunkConfig,
    source_id: String,
    sites: HashMap<String, SiteState>,
}

impl Rechunker {
    /// `source_id` is stamped on every block this rechunker emits (ROADMAP_NEW B6.5 per-block
    /// provenance) — e.g. `"relay"` for the HookEcho backend path.
    pub fn new(config: RechunkConfig, source_id: impl Into<String>) -> Self {
        Self {
            config,
            source_id: source_id.into(),
            sites: HashMap::new(),
        }
    }

    /// Parse one raw product, returning every block this makes ready to emit (zero, one, or more
    /// — a single product can complete a pending block and also trigger new ones). Never panics
    /// on malformed input: an undecodable product is logged and dropped without affecting any
    /// other site's stream (ROADMAP_NEW B6.2/B6.3).
    pub fn ingest(&mut self, product: &RawProduct) -> Vec<LiveLevel2Block> {
        let messages = match nexrad_decode::messages::decode_messages(&product.bytes) {
            Ok(messages) => messages,
            Err(e) => {
                log::warn!(
                    "radar-ingest: dropping unparseable product for {}: {e}",
                    product.site
                );
                return Vec::new();
            }
        };

        let mut flushed = Vec::new();
        let now = Utc::now();
        for message in messages {
            let offset = message.offset();
            let size = message.size();
            let raw_bytes = match product.bytes.get(offset..offset + size) {
                Some(b) => b,
                None => continue,
            };
            match message.contents() {
                MessageContents::DigitalRadarData(radar_message) => {
                    self.ingest_radial(product, radar_message, raw_bytes, now, &mut flushed);
                }
                _ => {
                    flushed.push(self.pass_through(product, raw_bytes, now));
                }
            }
        }
        flushed
    }

    fn ingest_radial(
        &mut self,
        product: &RawProduct,
        radar_message: &nexrad_decode::messages::digital_radar_data::Message<'_>,
        raw_bytes: &[u8],
        now: DateTime<Utc>,
        flushed: &mut Vec<LiveLevel2Block>,
    ) {
        let header = radar_message.header();
        let radar_time = header.date_time().unwrap_or(product.received_at);
        let elevation_number = header.elevation_number() as u16;
        let azimuth_number = header.azimuth_number();
        let elevation_angle = header.elevation_angle_raw();
        let status = header.radial_status();

        let state = self.sites.entry(product.site.clone()).or_default();

        let starts_new_volume = matches!(status, RadialStatus::VolumeScanStart);
        let starts_new_cut = starts_new_volume
            || matches!(
                status,
                RadialStatus::ElevationStart | RadialStatus::ElevationStartVCPFinal
            );

        if starts_new_volume {
            if let Some(done) = state.pending.take() {
                flushed.push(done.finish(&product.site, &self.source_id, state.next_sequence()));
            }
            state.current_volume = Some(VolumeKey::new(&product.site, radar_time));
            state.cut_tracker.reset();
            state.current_cut = None;
        }
        let volume = state
            .current_volume
            .get_or_insert_with(|| VolumeKey::new(&product.site, radar_time))
            .clone();

        if starts_new_cut {
            if !starts_new_volume {
                if let Some(done) = state.pending.take() {
                    flushed.push(done.finish(
                        &product.site,
                        &self.source_id,
                        state.next_sequence(),
                    ));
                }
            }
            state.current_cut = Some(state.cut_tracker.observe(elevation_number));
        } else if state.current_cut.is_none() {
            // Joined mid-cut (e.g. the very first radial this process has ever seen for the
            // site) — start tracking from here rather than dropping the radial.
            state.current_cut = Some(state.cut_tracker.observe(elevation_number));
        }
        let cut = state.current_cut;

        let pending = state.pending.get_or_insert_with(|| {
            PendingBlock::start(
                volume,
                cut,
                Some(elevation_angle),
                radar_time,
                product.received_at,
                now,
            )
        });
        pending.push_radial(raw_bytes, azimuth_number, radar_time);

        let hit_size_limit = state
            .pending
            .as_ref()
            .is_some_and(|p| p.radial_count >= self.config.max_radials_per_block);
        let is_boundary_end = matches!(
            status,
            RadialStatus::ElevationEnd | RadialStatus::VolumeScanEnd
        );

        if hit_size_limit || is_boundary_end {
            if let Some(done) = state.pending.take() {
                flushed.push(done.finish(&product.site, &self.source_id, state.next_sequence()));
            }
        }
        if matches!(status, RadialStatus::VolumeScanEnd) {
            state.latest_complete_volume = state.current_volume.clone();
        }
    }

    /// Forward a non-radial (or otherwise unrecognized) message verbatim as its own block, tagged
    /// with whatever volume/cut context is currently known for the site.
    fn pass_through(
        &mut self,
        product: &RawProduct,
        raw_bytes: &[u8],
        now: DateTime<Utc>,
    ) -> LiveLevel2Block {
        let state = self.sites.entry(product.site.clone()).or_default();
        let volume = state
            .current_volume
            .clone()
            .unwrap_or_else(|| VolumeKey::new(&product.site, product.received_at));
        let cut = state.current_cut;
        let sequence = state.next_sequence();
        LiveLevel2Block {
            site: product.site.clone(),
            volume,
            cut,
            elevation_angle_deg: None,
            first_azimuth_number: None,
            last_azimuth_number: None,
            radar_start: product.received_at,
            radar_end: product.received_at,
            received_at: product.received_at,
            emitted_at: now,
            sequence,
            source_id: self.source_id.clone(),
            checksum: checksum(raw_bytes),
            payload: raw_bytes.to_vec(),
        }
    }

    /// Flush any pending block older than [`RechunkConfig::max_block_age`]. Call periodically
    /// (e.g. once a second) from the service loop, independent of new products arriving — a quiet
    /// stream must not hold a partially filled block forever (ROADMAP_NEW B6.3).
    pub fn tick(&mut self, now: DateTime<Utc>) -> Vec<LiveLevel2Block> {
        let mut flushed = Vec::new();
        for (site, state) in self.sites.iter_mut() {
            let is_stale = state.pending.as_ref().is_some_and(|p| {
                now.signed_duration_since(p.started_at) >= self.config.max_block_age
            });
            if is_stale {
                if let Some(done) = state.pending.take() {
                    flushed.push(done.finish(site, &self.source_id, state.next_sequence()));
                }
            }
        }
        flushed
    }

    pub fn manifest(&self, site: &str) -> Option<SiteManifest> {
        self.sites
            .get(&site.to_ascii_uppercase())
            .map(SiteState::manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Builds the raw bytes of a single, minimal (zero data blocks) NEXRAD Level II Message Type
    /// 31 "Digital Radar Data" — the outer 28-byte transport [`nexrad_decode`] message header plus
    /// the 32-byte digital-radar-data header, enough for [`digital_radar_data::Message::parse`] to
    /// succeed and expose every field [`Rechunker`] reads. Zero data blocks is a legitimate
    /// (if minimal) message per the format: `data_block_count = 0` means the pointer table is
    /// empty and there is nothing further to parse, which is all this rechunker needs — it never
    /// reads gate/moment data itself.
    fn synthetic_radial(
        site: &str,
        elevation_number: u8,
        elevation_angle: f32,
        azimuth_number: u16,
        radial_status: u8,
        time: DateTime<Utc>,
    ) -> Vec<u8> {
        let mut msg = Vec::with_capacity(60);

        let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
        let days_since_epoch = (time.date_naive() - epoch).num_days();
        let date_field = (days_since_epoch + 1) as u16;
        let midnight = time.date_naive().and_hms_opt(0, 0, 0).unwrap();
        let ms_past_midnight = (time.naive_utc() - midnight).num_milliseconds() as u32;

        // --- outer 28-byte transport message header (nexrad_decode::messages::MessageHeader) ---
        msg.extend_from_slice(&[0u8; 12]); // rpg_unknown
        msg.extend_from_slice(&0u16.to_be_bytes()); // segment_size (ignored for type 31 framing)
        msg.push(8); // redundant_channel (ORDA single channel)
        msg.push(31); // message_type = Digital Radar Data (generic format)
        msg.extend_from_slice(&1u16.to_be_bytes()); // sequence_number
        msg.extend_from_slice(&date_field.to_be_bytes()); // date
        msg.extend_from_slice(&ms_past_midnight.to_be_bytes()); // time
        msg.extend_from_slice(&0u16.to_be_bytes()); // segment_count (unused for type 31 framing)
        msg.extend_from_slice(&0u16.to_be_bytes()); // segment_number (unused for type 31 framing)
        let outer_len = msg.len();
        assert_eq!(
            outer_len, 28,
            "outer transport header must be exactly 28 bytes"
        );

        // --- inner 32-byte digital-radar-data header ---
        let mut id = [b' '; 4];
        for (dst, src) in id.iter_mut().zip(site.as_bytes()) {
            *dst = *src;
        }
        msg.extend_from_slice(&id); // radar_identifier
        msg.extend_from_slice(&ms_past_midnight.to_be_bytes()); // time
        msg.extend_from_slice(&date_field.to_be_bytes()); // date
        msg.extend_from_slice(&azimuth_number.to_be_bytes()); // azimuth_number
        msg.extend_from_slice(&0f32.to_be_bytes()); // azimuth_angle (unused by this module)
        msg.push(0); // compression_indicator = uncompressed
        msg.push(0); // spare
        msg.extend_from_slice(&0u16.to_be_bytes()); // radial_length (unused by this module)
        msg.push(1); // azimuth_resolution_spacing = 0.5 degrees
        msg.push(radial_status); // radial_status
        msg.push(elevation_number); // elevation_number
        msg.push(0); // cut_sector_number
        msg.extend_from_slice(&elevation_angle.to_be_bytes()); // elevation_angle
        msg.push(0); // radial_spot_blanking_status
        msg.push(0); // azimuth_indexing_mode
        msg.extend_from_slice(&0u16.to_be_bytes()); // data_block_count = 0

        assert_eq!(
            msg.len(),
            outer_len + 32,
            "inner header must be exactly 32 bytes"
        );
        msg
    }

    fn product(site: &str, messages: &[Vec<u8>], received_at: DateTime<Utc>) -> RawProduct {
        let mut bytes = Vec::new();
        for m in messages {
            bytes.extend_from_slice(m);
        }
        RawProduct {
            site: site.to_string(),
            bytes,
            received_at,
        }
    }

    // radial_status codes, matching the ICD (see `RadialStatus`'s doc comment in nexrad-decode).
    const ELEVATION_START: u8 = 0;
    const INTERMEDIATE: u8 = 1;
    const ELEVATION_END: u8 = 2;
    const VOLUME_START: u8 = 3;
    const VOLUME_END: u8 = 4;

    fn t(seconds: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 1, 3, 0, 0).unwrap() + chrono::Duration::seconds(seconds)
    }

    #[test]
    fn a_single_radial_product_decodes_and_tags_volume_and_cut_identity() {
        let mut rc = Rechunker::new(RechunkConfig::default(), "relay");
        let radial = synthetic_radial("KTLX", 1, 0.5, 10, VOLUME_START, t(0));
        let blocks = rc.ingest(&product("KTLX", &[radial], t(0)));
        // A lone VolumeScanStart radial doesn't finish anything by itself (no prior pending
        // block existed, and it isn't itself an elevation/volume end) — it opens a pending block.
        assert!(blocks.is_empty());
        let manifest = rc.manifest("KTLX").unwrap();
        assert_eq!(manifest.current_volume.unwrap().site, "KTLX");
        assert_eq!(manifest.current_cut.unwrap().elevation_number, 1);
    }

    #[test]
    fn an_elevation_end_flushes_exactly_the_radials_of_that_cut() {
        let mut rc = Rechunker::new(RechunkConfig::default(), "relay");
        let radials = vec![
            synthetic_radial("KTLX", 1, 0.5, 0, VOLUME_START, t(0)),
            synthetic_radial("KTLX", 1, 0.5, 1, INTERMEDIATE, t(1)),
            synthetic_radial("KTLX", 1, 0.5, 2, ELEVATION_END, t(2)),
        ];
        let blocks = rc.ingest(&product("KTLX", &radials, t(0)));
        assert_eq!(blocks.len(), 1);
        let block = &blocks[0];
        assert_eq!(block.first_azimuth_number, Some(0));
        assert_eq!(block.last_azimuth_number, Some(2));
        assert_eq!(block.cut.unwrap().elevation_number, 1);
        assert_eq!(block.sequence, 0);
        // The three original radials' bytes are preserved verbatim and concatenated, not
        // resampled or re-encoded.
        let expected_len: usize = radials.iter().map(Vec::len).sum();
        assert_eq!(block.payload.len(), expected_len);
    }

    #[test]
    fn a_sails_revisit_gets_its_own_cut_identity_not_the_base_tilts() {
        let mut rc = Rechunker::new(RechunkConfig::default(), "relay");
        let mut blocks = Vec::new();
        blocks.extend(rc.ingest(&product(
            "KTLX",
            &[
                synthetic_radial("KTLX", 1, 0.5, 0, VOLUME_START, t(0)),
                synthetic_radial("KTLX", 1, 0.5, 1, ELEVATION_END, t(1)),
            ],
            t(0),
        )));
        blocks.extend(rc.ingest(&product(
            "KTLX",
            &[
                synthetic_radial("KTLX", 2, 1.5, 0, ELEVATION_START, t(2)),
                synthetic_radial("KTLX", 2, 1.5, 1, ELEVATION_END, t(3)),
            ],
            t(2),
        )));
        // SAILS: elevation 1 revisited mid-volume, not at volume start.
        blocks.extend(rc.ingest(&product(
            "KTLX",
            &[
                synthetic_radial("KTLX", 1, 0.5, 0, ELEVATION_START, t(4)),
                synthetic_radial("KTLX", 1, 0.5, 1, ELEVATION_END, t(5)),
            ],
            t(4),
        )));
        assert_eq!(blocks.len(), 3);
        let base_tilt = blocks[0].cut.unwrap();
        let sails_revisit = blocks[2].cut.unwrap();
        assert_eq!(base_tilt.elevation_number, 1);
        assert_eq!(sails_revisit.elevation_number, 1);
        assert_ne!(
            base_tilt, sails_revisit,
            "the SAILS revisit must not collapse into the base tilt's identity"
        );
        assert_eq!(base_tilt.repeat_index, 0);
        assert_eq!(sails_revisit.repeat_index, 1);
    }

    #[test]
    fn a_volume_scan_end_marks_the_volume_complete_in_the_manifest() {
        let mut rc = Rechunker::new(RechunkConfig::default(), "relay");
        rc.ingest(&product(
            "KTLX",
            &[
                synthetic_radial("KTLX", 1, 0.5, 0, VOLUME_START, t(0)),
                synthetic_radial("KTLX", 1, 0.5, 1, VOLUME_END, t(1)),
            ],
            t(0),
        ));
        let manifest = rc.manifest("KTLX").unwrap();
        let completed = manifest.latest_complete_volume.unwrap();
        assert_eq!(completed, manifest.current_volume.unwrap());
    }

    #[test]
    fn hitting_the_size_limit_flushes_without_waiting_for_an_elevation_boundary() {
        let config = RechunkConfig {
            max_radials_per_block: 2,
            ..RechunkConfig::default()
        };
        let mut rc = Rechunker::new(config, "relay");
        let radials = vec![
            synthetic_radial("KTLX", 1, 0.5, 0, VOLUME_START, t(0)),
            synthetic_radial("KTLX", 1, 0.5, 1, INTERMEDIATE, t(1)),
            synthetic_radial("KTLX", 1, 0.5, 2, INTERMEDIATE, t(2)),
        ];
        let blocks = rc.ingest(&product("KTLX", &radials, t(0)));
        // Two radials hit the size limit and flush immediately; the third starts a new pending
        // block that is still open (no fourth radial or boundary yet).
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].first_azimuth_number, Some(0));
        assert_eq!(blocks[0].last_azimuth_number, Some(1));
    }

    #[test]
    fn tick_flushes_a_pending_block_once_it_is_older_than_the_age_limit() {
        let config = RechunkConfig {
            max_block_age: chrono::Duration::seconds(5),
            ..RechunkConfig::default()
        };
        let mut rc = Rechunker::new(config, "relay");
        let radial = synthetic_radial("KTLX", 1, 0.5, 0, VOLUME_START, t(0));
        let blocks = rc.ingest(&product("KTLX", &[radial], t(0)));
        assert!(blocks.is_empty(), "a lone radial does not flush by itself");

        let too_soon = rc.tick(Utc::now() + chrono::Duration::seconds(1));
        assert!(too_soon.is_empty());

        let after_limit = rc.tick(Utc::now() + chrono::Duration::seconds(10));
        assert_eq!(after_limit.len(), 1);
    }

    #[test]
    fn a_non_radial_message_passes_through_verbatim_instead_of_being_dropped() {
        // Message type 2 (RDA Status Data), framed as a header-only variable-length message: a
        // `segment_size` of 0xFFFF makes `decode_messages` treat this as variable-length
        // regardless of type (the "segmented" fixed-2432-byte-frame path requires a full frame's
        // worth of bytes, which this test deliberately doesn't supply). Its content is never
        // decoded (only type 31 gets special handling in the variable-length path) — the point of
        // this test is that the message frame itself still comes back as a whole, forwardable
        // block instead of being silently discarded.
        let mut header = vec![0u8; 28];
        header[14] = 8;
        header[15] = 2; // message_type = RDA Status Data
        header[12..14].copy_from_slice(&0xFFFFu16.to_be_bytes()); // segment_size sentinel
                                                                  // segment_count/segment_number are repurposed as a 32-bit message_size_bytes (high, low)
                                                                  // when segment_size == 0xFFFF; 28 matches this header-only message's true length.
        header[24..26].copy_from_slice(&0u16.to_be_bytes());
        header[26..28].copy_from_slice(&28u16.to_be_bytes());

        let mut rc = Rechunker::new(RechunkConfig::default(), "relay");
        let blocks = rc.ingest(&product("KTLX", &[header.clone()], t(0)));
        // Even though the content can't be decoded as RDA Status Data (too short), the message
        // frame itself decodes and is forwarded rather than silently discarded.
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].cut, None);
    }

    #[test]
    fn garbage_bytes_are_dropped_without_panicking_or_affecting_other_sites() {
        let mut rc = Rechunker::new(RechunkConfig::default(), "relay");
        let garbage = product("KTLX", &[vec![0xFFu8; 10]], t(0));
        let blocks = rc.ingest(&garbage);
        assert!(blocks.is_empty());

        // A well-formed product for a different site right after must still work.
        let radials = vec![
            synthetic_radial("KOHX", 1, 0.5, 0, VOLUME_START, t(0)),
            synthetic_radial("KOHX", 1, 0.5, 1, ELEVATION_END, t(1)),
        ];
        let blocks = rc.ingest(&product("KOHX", &radials, t(0)));
        assert_eq!(blocks.len(), 1);
    }
}
