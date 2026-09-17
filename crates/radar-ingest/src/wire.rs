//! JSON wire types for B6.4's HTTP/WebSocket distribution API.
//!
//! `wxdata::live_block::LiveLevel2Block` and `rechunk::SiteManifest` are deliberately not
//! `Serialize`/`Deserialize` themselves — ROADMAP_NEW B6.3 calls the suggested block envelope "not
//! a frozen wire contract," and coupling the canonical in-process identity types to a specific
//! JSON shape would make evolving one force-evolve the other. These DTOs are that wire shape,
//! converted to/from the canonical types at the HTTP/WebSocket boundary only.

use crate::rechunk::SiteManifest;
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use wxdata::live_block::{CutKey, LiveLevel2Block, VolumeKey};

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

/// The wire form of one [`LiveLevel2Block`]: identical fields, with `checksum` hex-encoded and
/// `payload` base64-encoded so the whole thing is plain JSON (binary framing is a possible future
/// optimization, not a fidelity requirement — see B6.4's "compression only when it produces a
/// measured win").
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

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(s, "{b:02x}").expect("writing to a String never fails");
    }
    s
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManifestDto {
    pub newest_sequence: Option<u64>,
    pub current_volume: Option<VolumeKeyDto>,
    pub current_cut: Option<CutKeyDto>,
    pub latest_complete_volume: Option<VolumeKeyDto>,
}

impl From<&SiteManifest> for ManifestDto {
    fn from(m: &SiteManifest) -> Self {
        Self {
            newest_sequence: m.newest_sequence,
            current_volume: m.current_volume.as_ref().map(VolumeKeyDto::from),
            current_cut: m.current_cut.map(CutKeyDto::from),
            latest_complete_volume: m.latest_complete_volume.as_ref().map(VolumeKeyDto::from),
        }
    }
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
            checksum: wxdata::live_block::checksum(b"hello"),
            payload: b"hello".to_vec(),
        }
    }

    #[test]
    fn a_block_round_trips_through_json() {
        let dto = BlockDto::from(&sample_block());
        let json = serde_json::to_string(&dto).unwrap();
        let back: BlockDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back.site, "KTLX");
        assert_eq!(back.sequence, 7);
        assert_eq!(
            back.checksum_hex.len(),
            64,
            "sha-256 is 32 bytes = 64 hex chars"
        );
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&back.payload_base64)
            .unwrap();
        assert_eq!(decoded, b"hello");
    }

    #[test]
    fn a_manifest_with_no_activity_serializes_to_all_nulls() {
        let dto = ManifestDto::from(&SiteManifest::default());
        let json = serde_json::to_value(&dto).unwrap();
        assert!(json["newest_sequence"].is_null());
        assert!(json["current_volume"].is_null());
    }
}
