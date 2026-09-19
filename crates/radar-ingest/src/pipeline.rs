//! Wires [`Rechunker`] to [`BlockStore`] retention and live subscriber fan-out — the shared state
//! both the ingestion side (draining an [`InputAdapter`](crate::input::InputAdapter)) and the
//! HTTP/WebSocket server ([`crate::server`]) act on.

use crate::block_store::{BlockStore, BlockStoreLimits};
use crate::input::RawProduct;
use crate::rechunk::{RechunkConfig, Rechunker, SiteManifest};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use tokio::sync::broadcast;
use wxdata::live_block::LiveLevel2Block;

/// How many not-yet-delivered blocks a live subscriber may lag behind before it starts missing
/// them (`tokio::sync::broadcast`'s own overflow behavior: the oldest undelivered message is
/// dropped for a lagging receiver, which then observes `RecvError::Lagged`). A slow WebSocket
/// client can catch up over HTTP via `resume_after`, so this only needs to be "generous enough
/// that a normal brief stall doesn't lose data," not unbounded.
const SUBSCRIBER_CHANNEL_CAPACITY: usize = 256;

/// Wall-clock nanoseconds since the Unix epoch, truncated to `u64` — used only as a per-process-
/// start value that's overwhelmingly unlikely to repeat across restarts (no crypto/uniqueness
/// requirement here: the failure mode of an actual collision is identical to today's status quo
/// with no epoch check at all, so this needs "different from last time" in practice, not a formal
/// guarantee). Deliberately not a new dependency (`uuid`/`rand`) for one call site.
fn fresh_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or_default()
}

/// Ties the rechunker to durable-enough-for-resume retention and to live subscribers, so
/// ingesting one raw product does exactly three things to every block it produces: store it,
/// broadcast it, and make it available for the next `blocks_after` backfill.
pub struct Pipeline {
    rechunker: Rechunker,
    blocks: BlockStore,
    subscribers: HashMap<String, broadcast::Sender<LiveLevel2Block>>,
    epoch: u64,
}

impl Pipeline {
    pub fn new(
        rechunk_config: RechunkConfig,
        block_limits: BlockStoreLimits,
        source_id: impl Into<String>,
    ) -> Self {
        Self {
            rechunker: Rechunker::new(rechunk_config, source_id),
            blocks: BlockStore::new(block_limits),
            subscribers: HashMap::new(),
            epoch: fresh_epoch(),
        }
    }

    /// A value that changes every time a new `Pipeline` is constructed — in practice, once per
    /// process start (ROADMAP_NEW B6.10's "deliberately announce a new stream epoch so old
    /// sequence IDs cannot collide"). Every sequence number this pipeline hands out starts back at
    /// 0 on restart (nothing is persisted to disk — see that same roadmap item's "or" clause), so a
    /// client that reconnects after a restart and blindly resumes from its last-seen sequence could
    /// otherwise be served a same-numbered but semantically different block. Comparing epochs
    /// (`crate::server`'s `/live?epoch=`) is how a resuming client tells "the server I'm resuming
    /// against is the same one that handed me this sequence" from "it restarted meanwhile."
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Parse one raw product, storing and broadcasting every block it produces.
    pub fn ingest(&mut self, product: &RawProduct) {
        for block in self.rechunker.ingest(product) {
            self.publish(block);
        }
    }

    /// Flush any pending block that has aged out. Call periodically from the service loop,
    /// independent of new products arriving (see [`Rechunker::tick`]).
    pub fn tick(&mut self, now: DateTime<Utc>) {
        for block in self.rechunker.tick(now) {
            self.publish(block);
        }
    }

    fn publish(&mut self, block: LiveLevel2Block) {
        let site = block.site.clone();
        self.blocks.insert(block.clone());
        // A `send` error here only means no one is currently subscribed to this site — not a
        // failure of the pipeline. The block is still safely retained in `self.blocks` for the
        // next client to arrive and backfill via `resume_after`.
        let _ = self
            .subscribers
            .entry(site)
            .or_insert_with(|| broadcast::channel(SUBSCRIBER_CHANNEL_CAPACITY).0)
            .send(block);
    }

    pub fn manifest(&self, site: &str) -> Option<SiteManifest> {
        self.rechunker.manifest(site)
    }

    /// Every site with at least one retained block, plus its current item count and byte
    /// footprint (ROADMAP_NEW B6.10's `/metrics` observability).
    pub fn retention_stats(&self) -> impl Iterator<Item = (&str, usize, usize)> {
        self.blocks.retention_stats()
    }

    pub fn block(&self, site: &str, sequence: u64) -> Option<LiveLevel2Block> {
        self.blocks.get(site, sequence).cloned()
    }

    pub fn blocks_after(&self, site: &str, sequence: u64) -> Vec<LiveLevel2Block> {
        self.blocks.after(site, sequence)
    }

    /// Every retained block belonging to `site`'s latest completed volume, or `None` if the site
    /// is unknown or has not yet completed a volume — the completed-volume HTTP endpoint's data
    /// source (ROADMAP_NEW B6.4).
    pub fn latest_complete_volume_blocks(&self, site: &str) -> Option<Vec<LiveLevel2Block>> {
        let volume = self.manifest(site)?.latest_complete_volume?;
        Some(self.blocks.blocks_for_volume(site, &volume))
    }

    /// Subscribe to every block emitted for `site` from this point on. Combine with
    /// [`Pipeline::blocks_after`] (called first, while still holding the lock that serializes
    /// against `publish`) to backfill a resuming client without a gap or a duplicate — see
    /// [`crate::server`]'s live-stream handler for the exact ordering this depends on.
    pub fn subscribe(&mut self, site: &str) -> broadcast::Receiver<LiveLevel2Block> {
        self.subscribers
            .entry(site.to_ascii_uppercase())
            .or_insert_with(|| broadcast::channel(SUBSCRIBER_CHANNEL_CAPACITY).0)
            .subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rechunk::RechunkConfig;

    fn radial(site: &str, elevation_number: u8, azimuth_number: u16, status: u8) -> Vec<u8> {
        crate::rechunk::test_support::synthetic_radial(
            site,
            elevation_number,
            0.5,
            azimuth_number,
            status,
            Utc::now(),
        )
    }

    const VOLUME_START: u8 = 3;
    const ELEVATION_END: u8 = 2;

    #[test]
    fn ingest_stores_and_broadcasts_every_flushed_block() {
        let mut pipeline = Pipeline::new(
            RechunkConfig::default(),
            BlockStoreLimits::default(),
            "relay",
        );
        let mut rx = pipeline.subscribe("KTLX");

        let product = RawProduct {
            site: "KTLX".into(),
            bytes: [
                radial("KTLX", 1, 0, VOLUME_START),
                radial("KTLX", 1, 1, ELEVATION_END),
            ]
            .concat(),
            received_at: Utc::now(),
        };
        pipeline.ingest(&product);

        // Stored for backfill: sequence 0 is directly fetchable, and nothing exists after it yet.
        assert!(pipeline.block("KTLX", 0).is_some());
        assert!(pipeline.blocks_after("KTLX", 0).is_empty());

        // ...and broadcast live.
        let received = rx
            .try_recv()
            .expect("a subscriber present before ingest must receive the block");
        assert_eq!(received.sequence, 0);
    }

    #[test]
    fn a_subscriber_added_after_publish_can_still_backfill_via_blocks_after() {
        let mut pipeline = Pipeline::new(
            RechunkConfig::default(),
            BlockStoreLimits::default(),
            "relay",
        );
        let product = RawProduct {
            site: "KTLX".into(),
            bytes: [
                radial("KTLX", 1, 0, VOLUME_START),
                radial("KTLX", 1, 1, ELEVATION_END),
            ]
            .concat(),
            received_at: Utc::now(),
        };
        pipeline.ingest(&product);

        // No one was subscribed during ingest, but the block is still retrievable.
        let backlog = pipeline.blocks_after("KTLX", u64::MAX - 1000);
        assert!(
            backlog.is_empty(),
            "sanity: a sequence far in the future has nothing after it"
        );
        let backlog = pipeline.blocks_after("KTLX", 0);
        assert!(backlog.is_empty());
        assert_eq!(pipeline.block("KTLX", 0).unwrap().sequence, 0);
    }

    #[test]
    fn manifest_reflects_ingested_state() {
        let mut pipeline = Pipeline::new(
            RechunkConfig::default(),
            BlockStoreLimits::default(),
            "relay",
        );
        assert!(pipeline.manifest("KTLX").is_none());
        pipeline.ingest(&RawProduct {
            site: "KTLX".into(),
            bytes: radial("KTLX", 1, 0, VOLUME_START),
            received_at: Utc::now(),
        });
        assert!(pipeline.manifest("KTLX").is_some());
    }
}
