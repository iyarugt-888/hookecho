//! JSON wire types for B6.4's HTTP/WebSocket distribution API.
//!
//! The block DTO ([`BlockDto`] and its nested [`VolumeKeyDto`]/[`CutKeyDto`]) lives in
//! `wxdata::relay_wire`, not here — it is shared with any [`wxdata::live_block`]-based client of
//! this server (starting with `hookecho`'s `HookEchoRelayLevel2Provider`, ROADMAP_NEW B6.11 step
//! 6), so both sides serialize/deserialize the exact same shape without `hookecho` depending on
//! this crate's server-only dependencies (axum, tokio's `net` feature, etc.). [`ManifestDto`]
//! stays here: it wraps [`crate::rechunk::SiteManifest`], which is specific to this crate.

pub use wxdata::relay_wire::{BlockDto, CutKeyDto, VolumeKeyDto};

use crate::rechunk::SiteManifest;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManifestDto {
    pub newest_sequence: Option<u64>,
    pub current_volume: Option<VolumeKeyDto>,
    pub current_cut: Option<CutKeyDto>,
    pub latest_complete_volume: Option<VolumeKeyDto>,
    /// This server process's current stream epoch (ROADMAP_NEW B6.10) — `#[serde(default)]` so an
    /// older client that doesn't know about epochs yet still deserializes this response. A client
    /// that stores `newest_sequence` for later `resume_after` use should store this alongside it,
    /// and only send both back together (see `crate::server`'s `/live` handler).
    #[serde(default)]
    pub epoch: u64,
}

impl ManifestDto {
    /// [`Self::from`], plus the server-process-level epoch a bare `&SiteManifest` doesn't carry.
    pub fn with_epoch(manifest: &SiteManifest, epoch: u64) -> Self {
        Self {
            epoch,
            ..Self::from(manifest)
        }
    }
}

impl From<&SiteManifest> for ManifestDto {
    fn from(m: &SiteManifest) -> Self {
        Self {
            newest_sequence: m.newest_sequence,
            current_volume: m.current_volume.as_ref().map(VolumeKeyDto::from),
            current_cut: m.current_cut.map(CutKeyDto::from),
            latest_complete_volume: m.latest_complete_volume.as_ref().map(VolumeKeyDto::from),
            epoch: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_with_no_activity_serializes_to_all_nulls() {
        let dto = ManifestDto::from(&SiteManifest::default());
        let json = serde_json::to_value(&dto).unwrap();
        assert!(json["newest_sequence"].is_null());
        assert!(json["current_volume"].is_null());
    }

    #[test]
    fn with_epoch_carries_the_epoch_alongside_the_manifest_fields() {
        let dto = ManifestDto::with_epoch(&SiteManifest::default(), 42);
        assert_eq!(dto.epoch, 42);
        assert_eq!(dto.newest_sequence, None);
    }

    #[test]
    fn a_manifest_json_missing_the_epoch_field_still_deserializes() {
        // An older server (or a hand-written fixture) that predates this field must not break a
        // client that now expects it.
        let json = r#"{"newest_sequence":3,"current_volume":null,"current_cut":null,"latest_complete_volume":null}"#;
        let dto: ManifestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.epoch, 0);
        assert_eq!(dto.newest_sequence, Some(3));
    }
}
