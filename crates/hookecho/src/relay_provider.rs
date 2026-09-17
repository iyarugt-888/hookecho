//! ROADMAP_NEW B6.11 step 6: [`Level2LiveProvider`] for the HookEcho `radar-ingest` relay backend
//! (`crates/radar-ingest`) — the second, independently acquired progressive Level II path B6 is
//! ultimately about, alongside the existing [`crate::volume::UnidataLevel2Provider`].
//!
//! **Native only for this increment.** The WebSocket client here is `tokio-tungstenite`; a
//! browser client would need `web_sys::WebSocket` instead — a genuinely different implementation,
//! not just the `?Send` `cfg_attr` swap `Level2LiveProvider` itself got in B6.1 — so wasm32 support
//! is a follow-up rather than built blind. HookEcho's core public-data functionality is entirely
//! unaffected: this provider is opt-in (a user points it at their own self-hosted relay), never
//! the default, and [`UnidataLevel2Provider`](crate::volume::UnidataLevel2Provider) is unchanged.
//!
//! Reassembly reuses [`wxdata::live_block::assemble_scan`] (itself reusing `nexrad-data`'s
//! existing real-time chunk assembly) and [`wxdata::live::merge_scan`] (the same incremental merge
//! the Unidata path already uses) — this provider's own job is only receiving blocks and deciding
//! when enough of them exist to attempt a merge, not decoding radar data a second way.

use crate::volume::{LatestVolume, Level2LiveProvider};
use futures_util::StreamExt;
use std::sync::Arc;
use wxdata::level2::Scan;
use wxdata::live::{ScanProgress, Update};
use wxdata::live_block::{assemble_scan, LiveLevel2Block, ProviderCapabilities, VolumeKey};
use wxdata::relay_wire::BlockDto;

/// A live Level II source backed by a self-hosted `radar-ingest` instance (ROADMAP_NEW B6.2-B6.4).
pub struct HookEchoRelayLevel2Provider {
    /// Base URL of the relay, e.g. `http://localhost:8080` or `https://relay.example.com` — no
    /// trailing slash (normalized in [`Self::new`]).
    base_url: String,
}

impl HookEchoRelayLevel2Provider {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    fn ws_url(&self, site: &str) -> String {
        let ws_base = if let Some(rest) = self.base_url.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if let Some(rest) = self.base_url.strip_prefix("http://") {
            format!("ws://{rest}")
        } else {
            // No scheme given: assume plain ws, matching a bare `host:port` the way a user would
            // type it into a local/LAN relay's settings field.
            format!("ws://{}", self.base_url)
        };
        format!("{ws_base}/sites/{site}/live")
    }

    fn http_url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl Level2LiveProvider for HookEchoRelayLevel2Provider {
    fn label(&self) -> &'static str {
        "HookEcho Relay"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::relay()
    }

    async fn subscribe(
        &self,
        site: String,
        base: Arc<Scan>,
        active: Box<dyn Fn() -> bool + Send + Sync>,
        mut on_update: Box<dyn FnMut(Update) + Send>,
        // The relay does not yet send a chunk-level progress signal analogous to the Unidata
        // path's per-chunk metadata (ROADMAP_NEW B6.9's latency/progress instrumentation is a
        // later step) — accepted for trait-signature compatibility, unused for now.
        _on_progress: Box<dyn FnMut(ScanProgress) + Send>,
    ) -> anyhow::Result<()> {
        let url = self.ws_url(&site);
        let (ws_stream, _) = tokio_tungstenite::connect_async(&url)
            .await
            .map_err(|e| anyhow::anyhow!("connecting to relay at {url}: {e}"))?;
        let mut read = ws_stream;

        let mut merged = base;
        let mut current_volume: Option<VolumeKey> = None;
        let mut pending_blocks: Vec<LiveLevel2Block> = Vec::new();
        let mut update_count: u64 = 0;

        while active() {
            let Some(msg) = read.next().await else {
                break; // relay closed the connection
            };
            let msg = msg.map_err(|e| anyhow::anyhow!("relay websocket error: {e}"))?;
            let text = match msg {
                tokio_tungstenite::tungstenite::Message::Text(t) => t,
                tokio_tungstenite::tungstenite::Message::Close(_) => break,
                _ => continue,
            };
            let dto: BlockDto = match serde_json::from_str(text.as_str()) {
                Ok(dto) => dto,
                Err(e) => {
                    log::warn!("relay {site}: malformed block JSON: {e}");
                    continue;
                }
            };
            let block = match LiveLevel2Block::try_from(&dto) {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("relay {site}: block failed integrity check: {e}");
                    continue;
                }
            };

            // A new volume from the relay's own identity model: never mix blocks from two
            // different volumes into one assembly attempt (ROADMAP_NEW B6.5's cross-source
            // mixing guard applies just as much within a single source's own volume rollover).
            if current_volume.as_ref() != Some(&block.volume) {
                pending_blocks.clear();
                current_volume = Some(block.volume.clone());
            }
            pending_blocks.push(block);

            // Re-assemble from everything accumulated so far this volume. Same O(n) per new
            // block accepted by `wxdata::live`'s own `emit()` for the Unidata path (see that
            // function's comment); a windowed re-assembly is a possible future optimization, not
            // a correctness requirement.
            let partial = match assemble_scan(&pending_blocks) {
                Ok(s) => s,
                // Most commonly: no VCP block has arrived yet this volume (e.g. a client that
                // joined mid-volume before the VCP message was seen). Not an error worth
                // surfacing — the next block may complete it.
                Err(_) => continue,
            };
            let (new_scan, changed) = wxdata::live::merge_scan(&merged, partial);
            if changed.is_empty() {
                continue;
            }
            merged = Arc::new(new_scan);
            update_count += 1;
            on_update(Update {
                name: format!("relay-{site}-{update_count}"),
                time: chrono::Utc::now(),
                scan: merged.clone(),
                changed,
                retries: 0,
                decode_time: std::time::Duration::ZERO,
            });
        }
        Ok(())
    }

    async fn latest_complete_volume(
        &self,
        site: &str,
        current_name: Option<&str>,
    ) -> anyhow::Result<LatestVolume> {
        let url = self.http_url(&format!("/sites/{site}/volume/latest"));
        let response = reqwest::get(&url).await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            // No completed volume yet for this site — not an error, just nothing to report.
            return Ok(LatestVolume::UpToDate);
        }
        let body = response.error_for_status()?.text().await?;
        let dtos: Vec<BlockDto> = serde_json::from_str(&body)?;
        if dtos.is_empty() {
            return Ok(LatestVolume::UpToDate);
        }
        let blocks: Vec<LiveLevel2Block> = dtos
            .iter()
            .map(LiveLevel2Block::try_from)
            .collect::<Result<_, _>>()?;

        let volume = blocks[0].volume.clone();
        let name = format!("relay-{site}-{}", volume.volume_start.timestamp_millis());
        if current_name == Some(name.as_str()) {
            return Ok(LatestVolume::UpToDate);
        }
        let scan = assemble_scan(&blocks)?;
        Ok(LatestVolume::New {
            name,
            time: volume.volume_start,
            scan,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_provider_names_itself() {
        assert_eq!(
            HookEchoRelayLevel2Provider::new("http://localhost:8080").label(),
            "HookEcho Relay"
        );
    }

    #[test]
    fn the_provider_advertises_resumable_progressive_capabilities() {
        let caps = HookEchoRelayLevel2Provider::new("http://localhost:8080").capabilities();
        assert!(caps.progressive_radials);
        assert!(
            caps.resume,
            "the relay is resumable, unlike the Unidata chunk stream"
        );
        assert!(caps.server_push);
    }

    #[test]
    fn the_provider_is_usable_as_a_trait_object() {
        let boxed: Box<dyn Level2LiveProvider> =
            Box::new(HookEchoRelayLevel2Provider::new("http://localhost:8080"));
        assert_eq!(boxed.label(), "HookEcho Relay");
    }

    #[test]
    fn ws_url_translates_http_scheme_to_ws() {
        let provider = HookEchoRelayLevel2Provider::new("http://localhost:8080/");
        assert_eq!(
            provider.ws_url("KTLX"),
            "ws://localhost:8080/sites/KTLX/live"
        );
    }

    #[test]
    fn ws_url_translates_https_scheme_to_wss() {
        let provider = HookEchoRelayLevel2Provider::new("https://relay.example.com");
        assert_eq!(
            provider.ws_url("KTLX"),
            "wss://relay.example.com/sites/KTLX/live"
        );
    }

    #[test]
    fn ws_url_assumes_plain_ws_for_a_bare_host() {
        let provider = HookEchoRelayLevel2Provider::new("localhost:8080");
        assert_eq!(
            provider.ws_url("KTLX"),
            "ws://localhost:8080/sites/KTLX/live"
        );
    }
}

/// End-to-end tests against a real, in-process `radar-ingest` server — not just this provider in
/// isolation, but the actual wire path: HTTP/WebSocket requests over a real `TcpListener`, real
/// JSON, real checksum verification, real `assemble_scan` reassembly. `radar-ingest` is a
/// dev-dependency (native-only, see `Cargo.toml`) purely for this.
#[cfg(test)]
mod integration_tests {
    use super::*;
    use radar_ingest::block_store::BlockStoreLimits;
    use radar_ingest::input::RawProduct;
    use radar_ingest::pipeline::Pipeline;
    use radar_ingest::rechunk::RechunkConfig;
    use std::sync::Mutex;

    // radial_status codes (see nexrad-decode's `RadialStatus`).
    const ELEVATION_END: u8 = 2;
    const VOLUME_START: u8 = 3;
    const VOLUME_END: u8 = 4;

    /// Duplicated from `wxdata::live_block`'s own test-only copy (which is itself duplicated from
    /// `radar-ingest::rechunk::test_support`) — `#[cfg(test)]` items never cross a crate boundary
    /// even via a dev-dependency, so there is no way to share this one definition three ways.
    fn synthetic_radial(
        site: &str,
        elevation_number: u8,
        azimuth_number: u16,
        radial_status: u8,
        time: chrono::DateTime<chrono::Utc>,
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

    fn synthetic_vcp(time: chrono::DateTime<chrono::Utc>) -> Vec<u8> {
        const FRAME_SIZE: usize = 2432;
        const HEADER_SIZE: usize = 28;
        let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
        let date_field = ((time.date_naive() - epoch).num_days() + 1) as u16;
        let midnight = time.date_naive().and_hms_opt(0, 0, 0).unwrap();
        let ms_past_midnight = (time.naive_utc() - midnight).num_milliseconds() as u32;

        let mut frame = vec![0u8; FRAME_SIZE];
        frame[12..14].copy_from_slice(&((FRAME_SIZE / 2) as u16).to_be_bytes());
        frame[14] = 8;
        frame[15] = 5;
        frame[16..18].copy_from_slice(&1u16.to_be_bytes());
        frame[18..20].copy_from_slice(&date_field.to_be_bytes());
        frame[20..24].copy_from_slice(&ms_past_midnight.to_be_bytes());
        frame[24..26].copy_from_slice(&1u16.to_be_bytes());
        frame[26..28].copy_from_slice(&1u16.to_be_bytes());

        let content = HEADER_SIZE;
        frame[content..content + 2].copy_from_slice(&68u16.to_be_bytes());
        frame[content + 2..content + 4].copy_from_slice(&2u16.to_be_bytes());
        frame[content + 4..content + 6].copy_from_slice(&12u16.to_be_bytes());
        frame[content + 6..content + 8].copy_from_slice(&1u16.to_be_bytes());
        frame[content + 8] = 1;
        frame
    }

    fn a_completed_volume(site: &str) -> Vec<u8> {
        let t = chrono::Utc::now();
        [
            synthetic_vcp(t),
            synthetic_radial(site, 1, 0, VOLUME_START, t),
            synthetic_radial(site, 1, 1, ELEVATION_END, t),
            synthetic_radial(site, 2, 0, VOLUME_END, t),
        ]
        .concat()
    }

    #[tokio::test]
    async fn latest_complete_volume_fetches_and_assembles_a_real_scan_over_http() {
        let mut pipeline = Pipeline::new(
            RechunkConfig::default(),
            BlockStoreLimits::default(),
            "relay",
        );
        pipeline.ingest(&RawProduct {
            site: "KTLX".into(),
            bytes: a_completed_volume("KTLX"),
            received_at: chrono::Utc::now(),
        });
        let app = radar_ingest::server::router(Arc::new(Mutex::new(pipeline)));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = HookEchoRelayLevel2Provider::new(format!("http://{addr}"));
        match provider.latest_complete_volume("KTLX", None).await.unwrap() {
            LatestVolume::New { scan, .. } => {
                assert_eq!(scan.sweeps().len(), 2, "two elevation cuts were ingested");
            }
            LatestVolume::UpToDate => panic!("a real completed volume was ingested"),
        }
    }

    #[tokio::test]
    async fn latest_complete_volume_is_up_to_date_when_nothing_has_completed() {
        let pipeline = Pipeline::new(
            RechunkConfig::default(),
            BlockStoreLimits::default(),
            "relay",
        );
        let app = radar_ingest::server::router(Arc::new(Mutex::new(pipeline)));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = HookEchoRelayLevel2Provider::new(format!("http://{addr}"));
        let result = provider.latest_complete_volume("KTLX", None).await.unwrap();
        assert!(matches!(result, LatestVolume::UpToDate));
    }

    #[tokio::test]
    async fn subscribe_receives_a_real_live_update_over_a_real_websocket() {
        let pipeline = Arc::new(Mutex::new(Pipeline::new(
            RechunkConfig::default(),
            BlockStoreLimits::default(),
            "relay",
        )));
        let app = radar_ingest::server::router(pipeline.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = HookEchoRelayLevel2Provider::new(format!("http://{addr}"));
        // A minimal but validly-constructed empty scan (no unsafe/zeroed construction) — the same
        // shape `wxdata::level2`'s own tests use as a base for `merge_scan`.
        let empty_vcp = nexrad_model::data::VolumeCoveragePattern::new(
            212,
            0,
            0.5,
            nexrad_model::data::PulseWidth::Short,
            false,
            0,
            false,
            0,
            false,
            false,
            0,
            false,
            false,
            Vec::new(),
        );
        let base = Arc::new(wxdata::level2::Scan::new(empty_vcp, Vec::new()));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let active_clone = active.clone();
        let handle = tokio::spawn(async move {
            provider
                .subscribe(
                    "KTLX".to_string(),
                    base,
                    Box::new(move || active_clone.load(std::sync::atomic::Ordering::Relaxed)),
                    Box::new(move |update| {
                        let _ = tx.send(update);
                    }),
                    Box::new(|_progress| {}),
                )
                .await
        });

        // Give the WebSocket handshake + server-side subscribe a moment to land before ingesting
        // — a short, generous sleep rather than a synchronization primitive threaded through
        // production code purely for test determinism.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        pipeline.lock().unwrap().ingest(&RawProduct {
            site: "KTLX".into(),
            bytes: a_completed_volume("KTLX"),
            received_at: chrono::Utc::now(),
        });

        let update = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("no live update arrived over the websocket in time")
            .expect("update channel closed unexpectedly");
        assert!(!update.changed.is_empty());
        assert!(!update.scan.sweeps().is_empty());

        active.store(false, std::sync::atomic::Ordering::Relaxed);
        handle.abort();
    }
}
