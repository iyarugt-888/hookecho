//! The adapter boundary between wherever raw LDM/IDD products come from and the ingest core
//! (ROADMAP_NEW B6.2). Nothing downstream of [`InputAdapter`] knows or cares whether its products
//! came from a live `ldmd` pipe or a recorded fixture — that is the point: a live LDM adapter
//! (B6.11 step 5) and [`ReplayInputAdapter`] are interchangeable from the ingest core's point of
//! view.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One raw product as delivered by the upstream feed, before any Level II message parsing or
/// rechunking happens — that is [`wxdata::live_block`]'s and the future rechunker's job (B6.3).
/// This type only carries enough to route the bytes to the right site's ring buffer and stamp
/// their arrival.
///
/// `Serialize`/`Deserialize` (new in ROADMAP_NEW B6.10) exist for exactly one purpose: letting
/// [`crate::fixture`] read a JSON list of these for the `radar-ingest` binary's `--replay` mode —
/// there being no live LDM adapter yet (see [`crate::ldm`]'s own doc comment), a fixture file is
/// the only input source the binary can actually run today. `bytes` serializes as a plain JSON
/// array of numbers rather than base64: simplicity for a hand-written test fixture beats a few
/// bytes of encoding efficiency here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawProduct {
    /// Upper-case 4-character-ish site identifier as reported by the feed (e.g. `"KTLX"`).
    /// Case-normalized by [`crate::store::IngestStore`] on ingest, not here — this type is a
    /// faithful record of what arrived.
    pub site: String,
    /// Original, unmodified bytes for this product.
    pub bytes: Vec<u8>,
    /// When the ingest backend first received these bytes (backend receipt time, per B6.2's
    /// "timestamp every product/block at backend receipt" requirement) — not a radar timestamp;
    /// nothing at this layer has parsed far enough to know one.
    pub received_at: DateTime<Utc>,
}

/// A source of raw products, independent of the real LDM process. Consuming (`self: Box<Self>`)
/// because running an adapter to completion is a one-shot operation — there is no reason to keep
/// a finished/failed adapter around afterward, live or replay.
///
/// `tx` is a bounded `tokio::sync::mpsc::Sender`: `Sender::send` awaits when the channel is full,
/// so a slow consumer applies real backpressure to the adapter rather than the adapter buffering
/// unboundedly in front of it (ROADMAP_NEW B6.2: "a slow client must never grow ingest memory
/// without bound"). A live LDM adapter built later on this same trait inherits that property for
/// free; it does not need its own flow-control logic.
#[async_trait::async_trait]
pub trait InputAdapter: Send {
    async fn run(self: Box<Self>, tx: tokio::sync::mpsc::Sender<RawProduct>) -> anyhow::Result<()>;
}

/// Replays a fixed, in-memory list of [`RawProduct`]s in order — the fixture-driven adapter
/// ROADMAP_NEW B6.2 calls for so ingest logic, the ring buffer, and (later) the rechunker can be
/// tested deterministically without a live LDM process. Also useful for local development against
/// a recorded feed capture.
pub struct ReplayInputAdapter {
    products: Vec<RawProduct>,
    /// Optional pacing between sends, to emulate real arrival cadence. `None` (the default) sends
    /// as fast as the channel accepts them, which is what deterministic tests want.
    delay_between: Option<std::time::Duration>,
}

impl ReplayInputAdapter {
    pub fn new(products: Vec<RawProduct>) -> Self {
        Self {
            products,
            delay_between: None,
        }
    }

    /// Pace replay by sleeping `delay` between each send, e.g. to demo realistic arrival timing
    /// against a local `radar-ingest` instance. Deterministic tests should leave this unset.
    pub fn with_delay(mut self, delay: std::time::Duration) -> Self {
        self.delay_between = Some(delay);
        self
    }
}

#[async_trait::async_trait]
impl InputAdapter for ReplayInputAdapter {
    async fn run(self: Box<Self>, tx: tokio::sync::mpsc::Sender<RawProduct>) -> anyhow::Result<()> {
        let mut first = true;
        for product in self.products {
            if !first {
                if let Some(delay) = self.delay_between {
                    tokio::time::sleep(delay).await;
                }
            }
            first = false;
            // The receiver closing its end (ingest core shutting down) is a normal end of replay,
            // not an adapter failure — a live LDM adapter's own connection close would be reported
            // the same way.
            if tx.send(product).await.is_err() {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product(site: &str, bytes: &[u8]) -> RawProduct {
        RawProduct {
            site: site.to_string(),
            bytes: bytes.to_vec(),
            received_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn replay_delivers_every_product_in_order() {
        let products = vec![
            product("KTLX", b"one"),
            product("KTLX", b"two"),
            product("KOHX", b"three"),
        ];
        let adapter = Box::new(ReplayInputAdapter::new(products.clone()));
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        adapter.run(tx).await.unwrap();

        let mut received = Vec::new();
        while let Some(p) = rx.recv().await {
            received.push(p);
        }
        assert_eq!(received, products);
    }

    #[tokio::test]
    async fn replay_stops_quietly_when_the_receiver_is_gone() {
        let products = vec![product("KTLX", b"one"), product("KTLX", b"two")];
        let adapter = Box::new(ReplayInputAdapter::new(products));
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        drop(rx);
        // Must return Ok, not propagate the channel-closed error as a failure — a finished replay
        // and a rejected replay are different things to a caller deciding whether to retry.
        assert!(adapter.run(tx).await.is_ok());
    }

    #[tokio::test]
    async fn a_full_channel_applies_backpressure_to_the_adapter() {
        // Bound the channel to 1 and never drain it: the second send must block, proving replay
        // cannot outrun a slow consumer by buffering unboundedly on its own side.
        let products = vec![product("KTLX", b"one"), product("KTLX", b"two")];
        let adapter = Box::new(ReplayInputAdapter::new(products));
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        let handle = tokio::spawn(async move { adapter.run(tx).await });
        // The first product fits in the channel's capacity of 1; give the task a chance to block
        // on the second before we drain anything.
        tokio::task::yield_now().await;
        assert!(
            !handle.is_finished(),
            "adapter should be blocked sending the second product into a full channel"
        );

        let first = rx.recv().await.unwrap();
        assert_eq!(first.bytes, b"one");
        let second = rx.recv().await.unwrap();
        assert_eq!(second.bytes, b"two");
        handle.await.unwrap().unwrap();
    }
}
