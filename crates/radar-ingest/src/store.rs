//! The bounded, per-site in-memory holding area raw products land in once accepted
//! (ROADMAP_NEW B6.2: "maintain per-site rolling state and enough recent blocks for
//! reconnect/resume", "use bounded queues and explicit backpressure", "reject malformed/oversized
//! input without killing the stream").
//!
//! This is deliberately not yet aware of Level II message structure, volumes, or cuts — it holds
//! opaque [`RawProduct`]s. The rechunker (B6.3) reads from here to build
//! [`wxdata::live_block::LiveLevel2Block`]s; this module's only job is "don't lose recent data,
//! don't grow without bound, and don't let one bad or oversized product take down a site's
//! stream."

use crate::input::RawProduct;
use std::collections::{HashMap, HashSet, VecDeque};

/// Why [`IngestStore::ingest`] declined a product — every rejection is a deliberate, named
/// decision rather than a silent drop, so a deployment can tell "the feed is quiet" apart from
/// "the feed is sending garbage" or "this site isn't allowlisted."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// Empty site identifier, or one implausible enough to be a framing error rather than a real
    /// site (e.g. absurdly long).
    InvalidSite,
    /// Zero-length product — nothing to store, and a real Level II product is never empty.
    EmptyProduct,
    /// Exceeded the configured per-product byte limit. A well-formed Level II product from a
    /// single LDM delivery is bounded in practice; something this large is either misframed or
    /// hostile, and admitting it would let one product blow the whole site's memory budget.
    OversizedProduct { len: usize, max: usize },
    /// The site is not in the configured allowlist (ROADMAP_NEW B6.2: "support site allowlists so
    /// a deployment can ingest a selected radar set instead of being forced to retain the entire
    /// national feed").
    NotAllowlisted,
}

impl std::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RejectReason::InvalidSite => write!(f, "invalid site identifier"),
            RejectReason::EmptyProduct => write!(f, "empty product"),
            RejectReason::OversizedProduct { len, max } => {
                write!(f, "product of {len} bytes exceeds the {max}-byte limit")
            }
            RejectReason::NotAllowlisted => write!(f, "site is not allowlisted"),
        }
    }
}

/// A bounded rolling window of recently ingested products for one site. Bounded on **both** item
/// count and total byte size: capping only the count would still let a site's memory footprint
/// balloon if its products happened to be unusually large, and capping only bytes could let a
/// flood of tiny products hold an unbounded number of items.
#[derive(Debug)]
pub struct SiteRingBuffer {
    max_items: usize,
    max_bytes: usize,
    items: VecDeque<RawProduct>,
    total_bytes: usize,
}

impl SiteRingBuffer {
    pub fn new(max_items: usize, max_bytes: usize) -> Self {
        assert!(max_items > 0, "a zero-capacity ring buffer retains nothing");
        assert!(max_bytes > 0, "a zero-byte ring buffer retains nothing");
        Self {
            max_items,
            max_bytes,
            items: VecDeque::new(),
            total_bytes: 0,
        }
    }

    /// Push one product, evicting the oldest items first while either bound is exceeded.
    pub fn push(&mut self, product: RawProduct) {
        self.total_bytes += product.bytes.len();
        self.items.push_back(product);
        while self.items.len() > self.max_items || self.total_bytes > self.max_bytes {
            let Some(evicted) = self.items.pop_front() else {
                break;
            };
            self.total_bytes -= evicted.bytes.len();
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    /// Oldest-first iteration, matching arrival order.
    pub fn iter(&self) -> impl Iterator<Item = &RawProduct> {
        self.items.iter()
    }

    pub fn newest(&self) -> Option<&RawProduct> {
        self.items.back()
    }
}

/// Per-site collection of [`SiteRingBuffer`]s sharing one configured budget, plus the admission
/// checks ROADMAP_NEW B6.2 requires before anything reaches a buffer. Sites are created lazily on
/// first accepted product rather than pre-registered, so an allowlist alone controls what is ever
/// retained.
pub struct IngestStore {
    per_site_max_items: usize,
    per_site_max_bytes: usize,
    max_product_bytes: usize,
    allowlist: Option<HashSet<String>>,
    sites: HashMap<String, SiteRingBuffer>,
}

/// Tunable limits for one [`IngestStore`]. Kept as its own type rather than a long constructor
/// argument list, since a deployment configures these from environment/config (B6.10), not code.
#[derive(Debug, Clone)]
pub struct IngestLimits {
    pub per_site_max_items: usize,
    pub per_site_max_bytes: usize,
    pub max_product_bytes: usize,
}

impl Default for IngestLimits {
    fn default() -> Self {
        Self {
            per_site_max_items: 4096,
            // 64 MiB per site: generous headroom for a full volume's worth of recent products
            // without letting an idle deployment's memory grow toward "however many sites the
            // national feed has."
            per_site_max_bytes: 64 * 1024 * 1024,
            // A single LDM NEXRAD2 product is at most a few hundred KB in practice; 8 MiB is a
            // wide margin above that, not a target to size against.
            max_product_bytes: 8 * 1024 * 1024,
        }
    }
}

impl IngestStore {
    pub fn new(limits: IngestLimits) -> Self {
        Self {
            per_site_max_items: limits.per_site_max_items,
            per_site_max_bytes: limits.per_site_max_bytes,
            max_product_bytes: limits.max_product_bytes,
            allowlist: None,
            sites: HashMap::new(),
        }
    }

    /// Restrict ingest to exactly these sites (case-insensitive). `None` (the default after
    /// [`IngestStore::new`]) admits any syntactically valid site.
    pub fn with_allowlist(mut self, sites: impl IntoIterator<Item = String>) -> Self {
        self.allowlist = Some(sites.into_iter().map(|s| s.to_ascii_uppercase()).collect());
        self
    }

    fn site_is_valid(site: &str) -> bool {
        // Real NEXRAD site identifiers are four characters, but this only guards against
        // obviously-malformed framing (empty, or implausibly long) rather than enforcing the
        // four-character convention exactly — a deployment's allowlist is the place to be
        // precise about which real sites are wanted.
        !site.is_empty() && site.len() <= 8 && site.chars().all(|c| c.is_ascii_alphanumeric())
    }

    /// Validate and admit one product, routing it to its site's ring buffer. Returns `Ok(())` on
    /// acceptance or the specific [`RejectReason`] otherwise — rejection never panics and never
    /// affects any other site's stream (ROADMAP_NEW B6.2: "reject malformed/oversized input
    /// without killing the stream").
    pub fn ingest(&mut self, product: RawProduct) -> Result<(), RejectReason> {
        if !Self::site_is_valid(&product.site) {
            return Err(RejectReason::InvalidSite);
        }
        if product.bytes.is_empty() {
            return Err(RejectReason::EmptyProduct);
        }
        if product.bytes.len() > self.max_product_bytes {
            return Err(RejectReason::OversizedProduct {
                len: product.bytes.len(),
                max: self.max_product_bytes,
            });
        }
        let site_key = product.site.to_ascii_uppercase();
        if let Some(allowlist) = &self.allowlist {
            if !allowlist.contains(&site_key) {
                return Err(RejectReason::NotAllowlisted);
            }
        }
        self.sites
            .entry(site_key)
            .or_insert_with(|| {
                SiteRingBuffer::new(self.per_site_max_items, self.per_site_max_bytes)
            })
            .push(product);
        Ok(())
    }

    pub fn site(&self, site: &str) -> Option<&SiteRingBuffer> {
        self.sites.get(&site.to_ascii_uppercase())
    }

    pub fn known_sites(&self) -> impl Iterator<Item = &str> {
        self.sites.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn product(site: &str, len: usize) -> RawProduct {
        RawProduct {
            site: site.to_string(),
            bytes: vec![0u8; len],
            received_at: Utc::now(),
        }
    }

    #[test]
    fn ring_buffer_evicts_oldest_first_once_the_item_count_is_exceeded() {
        let mut buf = SiteRingBuffer::new(2, 1_000_000);
        buf.push(product("KTLX", 10));
        buf.push(product("KTLX", 10));
        buf.push(product("KTLX", 10));
        assert_eq!(buf.len(), 2);
        // The buffer keeps the two newest, not the two oldest.
        assert_eq!(buf.total_bytes(), 20);
    }

    #[test]
    fn ring_buffer_evicts_on_byte_budget_even_under_the_item_cap() {
        let mut buf = SiteRingBuffer::new(100, 25);
        buf.push(product("KTLX", 10));
        buf.push(product("KTLX", 10));
        buf.push(product("KTLX", 10));
        assert!(
            buf.total_bytes() <= 25,
            "byte budget must be enforced independently of item count"
        );
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn ring_buffer_newest_reflects_the_last_push() {
        let mut buf = SiteRingBuffer::new(4, 1_000_000);
        buf.push(product("KTLX", 1));
        buf.push(RawProduct {
            site: "KTLX".into(),
            bytes: b"newest".to_vec(),
            received_at: Utc::now(),
        });
        assert_eq!(buf.newest().unwrap().bytes, b"newest");
    }

    #[test]
    fn store_routes_products_to_their_own_site() {
        let mut store = IngestStore::new(IngestLimits::default());
        store.ingest(product("KTLX", 10)).unwrap();
        store.ingest(product("KOHX", 10)).unwrap();
        assert_eq!(store.site("KTLX").unwrap().len(), 1);
        assert_eq!(store.site("KOHX").unwrap().len(), 1);
        assert!(store.site("KABC").is_none());
    }

    #[test]
    fn store_normalizes_site_case() {
        let mut store = IngestStore::new(IngestLimits::default());
        store.ingest(product("ktlx", 10)).unwrap();
        assert_eq!(store.site("KTLX").unwrap().len(), 1);
    }

    #[test]
    fn store_rejects_empty_site() {
        let mut store = IngestStore::new(IngestLimits::default());
        assert_eq!(
            store.ingest(product("", 10)),
            Err(RejectReason::InvalidSite)
        );
    }

    #[test]
    fn store_rejects_empty_product() {
        let mut store = IngestStore::new(IngestLimits::default());
        assert_eq!(
            store.ingest(product("KTLX", 0)),
            Err(RejectReason::EmptyProduct)
        );
    }

    #[test]
    fn store_rejects_oversized_product_without_affecting_other_sites() {
        let limits = IngestLimits {
            max_product_bytes: 100,
            ..IngestLimits::default()
        };
        let mut store = IngestStore::new(limits);
        assert_eq!(
            store.ingest(product("KTLX", 200)),
            Err(RejectReason::OversizedProduct { len: 200, max: 100 })
        );
        // The rejection must not have poisoned the store for a well-formed product on another site.
        store.ingest(product("KOHX", 10)).unwrap();
        assert_eq!(store.site("KOHX").unwrap().len(), 1);
        assert!(store.site("KTLX").is_none());
    }

    #[test]
    fn store_enforces_allowlist_when_configured() {
        let mut store =
            IngestStore::new(IngestLimits::default()).with_allowlist(["KTLX".to_string()]);
        store.ingest(product("KTLX", 10)).unwrap();
        assert_eq!(
            store.ingest(product("KOHX", 10)),
            Err(RejectReason::NotAllowlisted)
        );
    }

    #[test]
    fn store_allowlist_is_case_insensitive() {
        let mut store =
            IngestStore::new(IngestLimits::default()).with_allowlist(["ktlx".to_string()]);
        assert!(store.ingest(product("KTLX", 10)).is_ok());
    }
}
