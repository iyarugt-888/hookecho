//! Bounded per-site retention of emitted [`LiveLevel2Block`]s, indexed by sequence — what B6.4's
//! HTTP "fetch a specific recent block by site/sequence" and `resume_after=<sequence>` endpoints
//! read from. Distinct from [`crate::store::IngestStore`]: that module retains raw, not-yet-parsed
//! product bytes for the ingest side; this one retains the canonical blocks the rechunker has
//! already produced, which is what a reconnecting *client* resumes from.

use std::collections::BTreeMap;
use std::collections::HashMap;
use wxdata::live_block::LiveLevel2Block;

/// A bounded, sequence-ordered window of recently emitted blocks for one site. A `BTreeMap` keyed
/// by sequence (rather than a plain `VecDeque`) makes "give me everything after sequence N" and
/// "give me exactly sequence N" both direct lookups instead of a linear scan — the two access
/// patterns B6.4's resume and single-block-fetch endpoints need.
pub struct BlockRingBuffer {
    max_items: usize,
    max_bytes: usize,
    blocks: BTreeMap<u64, LiveLevel2Block>,
    total_bytes: usize,
}

impl BlockRingBuffer {
    pub fn new(max_items: usize, max_bytes: usize) -> Self {
        assert!(max_items > 0, "a zero-capacity ring buffer retains nothing");
        assert!(max_bytes > 0, "a zero-byte ring buffer retains nothing");
        Self {
            max_items,
            max_bytes,
            blocks: BTreeMap::new(),
            total_bytes: 0,
        }
    }

    pub fn push(&mut self, block: LiveLevel2Block) {
        self.total_bytes += block.payload.len();
        self.blocks.insert(block.sequence, block);
        while self.blocks.len() > self.max_items || self.total_bytes > self.max_bytes {
            let Some((&oldest, _)) = self.blocks.iter().next() else {
                break;
            };
            if let Some(evicted) = self.blocks.remove(&oldest) {
                self.total_bytes -= evicted.payload.len();
            }
        }
    }

    pub fn get(&self, sequence: u64) -> Option<&LiveLevel2Block> {
        self.blocks.get(&sequence)
    }

    /// Every retained block with a sequence strictly greater than `sequence`, oldest first — the
    /// backlog a reconnecting client with `resume_after=<sequence>` needs replayed before it
    /// starts receiving genuinely live blocks.
    pub fn after(&self, sequence: u64) -> impl Iterator<Item = &LiveLevel2Block> {
        // `saturating_add` rather than `+`: a resuming client's own claimed "last seen sequence"
        // is untrusted input, and `u64::MAX` must return "nothing after this" rather than panic.
        self.blocks
            .range(sequence.saturating_add(1)..)
            .map(|(_, b)| b)
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn oldest_sequence(&self) -> Option<u64> {
        self.blocks.keys().next().copied()
    }
}

#[derive(Debug, Clone)]
pub struct BlockStoreLimits {
    pub per_site_max_items: usize,
    pub per_site_max_bytes: usize,
}

impl Default for BlockStoreLimits {
    fn default() -> Self {
        Self {
            per_site_max_items: 4096,
            per_site_max_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Per-site collection of [`BlockRingBuffer`]s.
pub struct BlockStore {
    limits: BlockStoreLimits,
    sites: HashMap<String, BlockRingBuffer>,
}

impl BlockStore {
    pub fn new(limits: BlockStoreLimits) -> Self {
        Self {
            limits,
            sites: HashMap::new(),
        }
    }

    pub fn insert(&mut self, block: LiveLevel2Block) {
        let site_key = block.site.to_ascii_uppercase();
        self.sites
            .entry(site_key)
            .or_insert_with(|| {
                BlockRingBuffer::new(
                    self.limits.per_site_max_items,
                    self.limits.per_site_max_bytes,
                )
            })
            .push(block);
    }

    pub fn get(&self, site: &str, sequence: u64) -> Option<&LiveLevel2Block> {
        self.sites.get(&site.to_ascii_uppercase())?.get(sequence)
    }

    /// Every retained block for `site` after `sequence`, oldest first. Returns an empty `Vec` —
    /// not an error — for an unknown site, since "nothing retained yet" and "nothing after this
    /// point" are the same answer from a resuming client's point of view.
    pub fn after(&self, site: &str, sequence: u64) -> Vec<LiveLevel2Block> {
        match self.sites.get(&site.to_ascii_uppercase()) {
            Some(buf) => buf.after(sequence).cloned().collect(),
            None => Vec::new(),
        }
    }

    pub fn known_site(&self, site: &str) -> bool {
        self.sites.contains_key(&site.to_ascii_uppercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use wxdata::live_block::VolumeKey;

    fn block(site: &str, sequence: u64, payload_len: usize) -> LiveLevel2Block {
        let t = Utc.with_ymd_and_hms(2026, 5, 1, 3, 0, 0).unwrap();
        LiveLevel2Block {
            site: site.to_string(),
            volume: VolumeKey::new(site, t),
            cut: None,
            elevation_angle_deg: None,
            first_azimuth_number: None,
            last_azimuth_number: None,
            radar_start: t,
            radar_end: t,
            received_at: t,
            emitted_at: t,
            sequence,
            source_id: "relay".into(),
            checksum: [0u8; 32],
            payload: vec![0u8; payload_len],
        }
    }

    #[test]
    fn after_returns_only_strictly_newer_blocks_in_order() {
        let mut buf = BlockRingBuffer::new(100, 1_000_000);
        for seq in 0..5 {
            buf.push(block("KTLX", seq, 1));
        }
        let after_two: Vec<u64> = buf.after(2).map(|b| b.sequence).collect();
        assert_eq!(after_two, vec![3, 4]);
    }

    #[test]
    fn get_finds_an_exact_sequence() {
        let mut buf = BlockRingBuffer::new(100, 1_000_000);
        buf.push(block("KTLX", 5, 1));
        assert!(buf.get(5).is_some());
        assert!(buf.get(6).is_none());
    }

    #[test]
    fn eviction_drops_the_oldest_sequence_first() {
        let mut buf = BlockRingBuffer::new(2, 1_000_000);
        buf.push(block("KTLX", 0, 1));
        buf.push(block("KTLX", 1, 1));
        buf.push(block("KTLX", 2, 1));
        assert_eq!(buf.len(), 2);
        assert!(
            buf.get(0).is_none(),
            "oldest sequence must be evicted first"
        );
        assert!(buf.get(1).is_some());
        assert!(buf.get(2).is_some());
        assert_eq!(buf.oldest_sequence(), Some(1));
    }

    #[test]
    fn byte_budget_evicts_independent_of_item_count() {
        let mut buf = BlockRingBuffer::new(100, 25);
        buf.push(block("KTLX", 0, 10));
        buf.push(block("KTLX", 1, 10));
        buf.push(block("KTLX", 2, 10));
        assert!(buf.len() < 3);
    }

    #[test]
    fn store_routes_by_site_case_insensitively() {
        let mut store = BlockStore::new(BlockStoreLimits::default());
        store.insert(block("ktlx", 0, 1));
        assert!(store.known_site("KTLX"));
        assert!(store.get("KTLX", 0).is_some());
    }

    #[test]
    fn store_after_on_an_unknown_site_is_an_empty_backlog_not_an_error() {
        let store = BlockStore::new(BlockStoreLimits::default());
        assert!(store.after("KABC", 0).is_empty());
    }
}
