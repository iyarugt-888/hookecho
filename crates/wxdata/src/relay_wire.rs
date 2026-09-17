//! JSON wire encoding for [`crate::live_block::LiveLevel2Block`] — shared by the HookEcho
//! `radar-ingest` backend (which serializes it) and any [`crate::live_block`]-based client of it
//! (which deserializes it), so the two never drift out of sync with each other independently.
//!
//! `LiveLevel2Block` is deliberately not `Serialize`/`Deserialize` itself — ROADMAP_NEW B6.3 calls
//! the block envelope "not a frozen wire contract," and coupling the canonical in-process identity
//! type to a specific JSON shape would make evolving one force-evolve the other. [`BlockDto`] is
//! that JSON shape, converted to/from the canonical type at the network boundary only.

use crate::live_block::{checksum, CutKey, LiveLevel2Block, VolumeKey};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VolumeKeyDto {
    pub site: String,
    pub volume_start: DateTime<Utc>,
}

impl From<&VolumeKey> for VolumeKeyDto {
    fn from(v: &VolumeKey) -> Self {
        Self {
            site: v.site.clone(),
            volume_start: v.volume_start,
        }
    }
}

impl From<&VolumeKeyDto> for VolumeKey {
    fn from(dto: &VolumeKeyDto) -> Self {
        VolumeKey::new(&dto.site, dto.volume_start)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct CutKeyDto {
    pub elevation_number: u16,
    pub repeat_index: u16,
}

impl From<CutKey> for CutKeyDto {
    fn from(c: CutKey) -> Self {
        Self {
            elevation_number: c.elevation_number,
            repeat_index: c.repeat_index,
        }
    }
}

impl From<CutKeyDto> for CutKey {
    fn from(dto: CutKeyDto) -> Self {
        CutKey {
            elevation_number: dto.elevation_number,
            repeat_index: dto.repeat_index,
        }
    }
}

/// The wire form of one [`LiveLevel2Block`]: identical fields, with `checksum` hex-encoded and
/// `payload` base64-encoded so the whole thing is plain JSON (binary framing is a possible future
/// optimization, not a fidelity requirement — see ROADMAP_NEW B6.4's "compression only when it
/// produces a measured win").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockDto {
    pub site: String,
    pub volume: VolumeKeyDto,
    pub cut: Option<CutKeyDto>,
    pub elevation_angle_deg: Option<f32>,
    pub first_azimuth_number: Option<u16>,
    pub last_azimuth_number: Option<u16>,
    pub radar_start: DateTime<Utc>,
    pub radar_end: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub emitted_at: DateTime<Utc>,
    pub sequence: u64,
    pub source_id: String,
    pub checksum_hex: String,
    pub payload_base64: String,
}

impl From<&LiveLevel2Block> for BlockDto {
    fn from(b: &LiveLevel2Block) -> Self {
        Self {
            site: b.site.clone(),
            volume: (&b.volume).into(),
            cut: b.cut.map(CutKeyDto::from),
            elevation_angle_deg: b.elevation_angle_deg,
            first_azimuth_number: b.first_azimuth_number,
            last_azimuth_number: b.last_azimuth_number,
            radar_start: b.radar_start,
            radar_end: b.radar_end,
            received_at: b.received_at,
            emitted_at: b.emitted_at,
            sequence: b.sequence,
            source_id: b.source_id.clone(),
            checksum_hex: hex_encode(&b.checksum),
            payload_base64: base64::engine::general_purpose::STANDARD.encode(&b.payload),
        }
    }
}

/// Errors turning a [`BlockDto`] received off the wire back into a [`LiveLevel2Block`] — every
/// case is an untrusted-input problem (bad encoding, or a checksum mismatch implying transport
/// corruption or tampering), never a panic.
#[derive(Debug, thiserror::Error)]
pub enum BlockDtoError {
    #[error("payload is not valid base64: {0}")]
    InvalidPayloadEncoding(#[from] base64::DecodeError),
    #[error("checksum is not valid hex or not 32 bytes (sha-256)")]
    InvalidChecksumEncoding,
    #[error("payload checksum mismatch: declared {declared}, computed {computed}")]
    ChecksumMismatch { declared: String, computed: String },
}

impl TryFrom<&BlockDto> for LiveLevel2Block {
    type Error = BlockDtoError;

    /// Decodes the payload and re-verifies its checksum against the decoded bytes — this is the
    /// one place data crosses a network boundary back into the canonical type, so it is also the
    /// one place worth actually checking the transport-integrity checksum ROADMAP_NEW B6.3 asks
    /// every block to carry, rather than trusting it unconditionally.
    fn try_from(dto: &BlockDto) -> Result<Self, Self::Error> {
        let payload = base64::engine::general_purpose::STANDARD.decode(&dto.payload_base64)?;
        let computed = checksum(&payload);
        let computed_hex = hex_encode(&computed);
        if computed_hex != dto.checksum_hex.to_ascii_lowercase() {
            return Err(BlockDtoError::ChecksumMismatch {
                declared: dto.checksum_hex.clone(),
                computed: computed_hex,
            });
        }
        Ok(LiveLevel2Block {
            site: dto.site.clone(),
            volume: (&dto.volume).into(),
            cut: dto.cut.map(CutKey::from),
            elevation_angle_deg: dto.elevation_angle_deg,
            first_azimuth_number: dto.first_azimuth_number,
            last_azimuth_number: dto.last_azimuth_number,
            radar_start: dto.radar_start,
            radar_end: dto.radar_end,
            received_at: dto.received_at,
            emitted_at: dto.emitted_at,
            sequence: dto.sequence,
            source_id: dto.source_id.clone(),
            checksum: computed,
            payload,
        })
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(s, "{b:02x}").expect("writing to a String never fails");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_block() -> LiveLevel2Block {
        let t = Utc.with_ymd_and_hms(2026, 5, 1, 3, 0, 0).unwrap();
        LiveLevel2Block {
            site: "KTLX".into(),
            volume: VolumeKey::new("KTLX", t),
            cut: Some(CutKey {
                elevation_number: 1,
                repeat_index: 0,
            }),
            elevation_angle_deg: Some(0.5),
            first_azimuth_number: Some(0),
            last_azimuth_number: Some(10),
            radar_start: t,
            radar_end: t,
            received_at: t,
            emitted_at: t,
            sequence: 7,
            source_id: "relay".into(),
            checksum: checksum(b"hello"),
            payload: b"hello".to_vec(),
        }
    }

    #[test]
    fn a_block_round_trips_through_json_and_back_to_the_canonical_type() {
        let original = sample_block();
        let dto = BlockDto::from(&original);
        let json = serde_json::to_string(&dto).unwrap();
        let back_dto: BlockDto = serde_json::from_str(&json).unwrap();
        let restored = LiveLevel2Block::try_from(&back_dto).unwrap();

        assert_eq!(restored.site, original.site);
        assert_eq!(restored.volume, original.volume);
        assert_eq!(restored.cut, original.cut);
        assert_eq!(restored.sequence, original.sequence);
        assert_eq!(restored.payload, original.payload);
        assert_eq!(restored.checksum, original.checksum);
    }

    #[test]
    fn a_tampered_payload_fails_the_checksum_check_instead_of_being_trusted() {
        let mut dto = BlockDto::from(&sample_block());
        // Tamper with the payload after encoding, as if it were corrupted or altered in transit.
        dto.payload_base64 = base64::engine::general_purpose::STANDARD.encode(b"tampered!!");
        assert!(matches!(
            LiveLevel2Block::try_from(&dto),
            Err(BlockDtoError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn invalid_base64_is_a_named_error_not_a_panic() {
        let mut dto = BlockDto::from(&sample_block());
        dto.payload_base64 = "not valid base64!!".to_string();
        assert!(matches!(
            LiveLevel2Block::try_from(&dto),
            Err(BlockDtoError::InvalidPayloadEncoding(_))
        ));
    }
}
