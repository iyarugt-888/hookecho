//! ROADMAP_NEW B6.11 step 6: [`Level2LiveProvider`] for the HookEcho `radar-ingest` relay backend
//! (`crates/radar-ingest`) — a second progressive transport whose upstream independence B6 must
//! establish, alongside the existing [`crate::volume::UnidataLevel2Provider`].
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
use wxdata::continuation::{SourceAdmission, SourceVolumeCursor};
use wxdata::level2::Scan;
use wxdata::live::{CutKind, RadialCoverage, ScanProgress, Update};
use wxdata::live_block::{assemble_scan, LiveLevel2Block, ProviderCapabilities, VolumeKey};
use wxdata::live_sequence::{SequenceLedger, SequenceOrigin};
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
        let base = if self.base_url.starts_with("http://") || self.base_url.starts_with("https://")
        {
            self.base_url.clone()
        } else {
            format!("http://{}", self.base_url)
        };
        format!("{base}{path}")
    }
}

/// Translate one canonical relay radial block into the same provider-neutral sweep progress the
/// direct Unidata path emits. Real Archive-II azimuth numbers are one-based; synthetic fixtures
/// sometimes use zero for the north radial, which `saturating_sub` intentionally maps to the same
/// first bin.
fn relay_scan_progress(block: &LiveLevel2Block, scan: &Scan) -> Option<ScanProgress> {
    let cut = block.cut?;
    let cuts = scan.coverage_pattern().elevation_cuts();
    let vcp_cut = cuts.get(cut.elevation_number.saturating_sub(1) as usize)?;
    let chunks_in_sweep = if vcp_cut.super_resolution_half_degree_azimuth() {
        6
    } else {
        3
    };
    let bins = chunks_in_sweep * 120;
    let first = block.first_azimuth_number?.saturating_sub(1) as usize % bins;
    let last = block.last_azimuth_number?.saturating_sub(1) as usize % bins;
    let azimuth_start_deg = first as f64 * 360.0 / bins as f64;
    let azimuth_end_deg = (last + 1) as f64 * 360.0 / bins as f64;
    let nominal_sector = 360.0 / chunks_in_sweep as f64;
    let chunk_index = if azimuth_end_deg < azimuth_start_deg {
        chunks_in_sweep
    } else {
        (azimuth_end_deg / nominal_sector)
            .ceil()
            .clamp(1.0, chunks_in_sweep as f64) as usize
    };
    Some(ScanProgress {
        volume_start_ms: Some(block.volume.volume_start.timestamp_millis()),
        vcp_number: Some(scan.coverage_pattern_number().number()),
        cut_kind: CutKind::from_flags(
            vcp_cut.is_sails_cut(),
            vcp_cut.is_mrle_cut(),
            vcp_cut.is_mpda_cut(),
        ),
        elevation_number: cut.elevation_number as usize,
        total_elevations: cuts.len(),
        elevation_angle_deg: block
            .elevation_angle_deg
            .map(f64::from)
            .unwrap_or_else(|| vcp_cut.elevation_angle_degrees()),
        azimuth_rate_dps: vcp_cut.azimuth_rate_degrees_per_second(),
        azimuth_start_deg,
        azimuth_end_deg,
        chunk_index,
        chunks_in_sweep,
    })
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl Level2LiveProvider for HookEchoRelayLevel2Provider {
    async fn inspect_topology(&self) -> wxdata::provider_topology::ProviderTopology {
        use wxdata::provider_topology::{
            DeclarationUnavailable as U, ProviderTopology, UpstreamDeclaration,
            MAX_DECLARATION_BYTES,
        };
        // Metadata is advisory and optional. Never put the configured URL or an untrusted
        // error body into retained topology diagnostics, and never follow metadata redirects.
        let result = async {
            let http = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(2))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| U::HttpFailure)?;
            let mut response =
                http.get(self.http_url("/provider"))
                    .send()
                    .await
                    .map_err(|error| {
                        if error.is_timeout() {
                            U::Timeout
                        } else {
                            U::HttpFailure
                        }
                    })?;
            if !response.status().is_success() {
                return Err(U::HttpFailure);
            }
            if response
                .content_length()
                .is_some_and(|bytes| bytes > MAX_DECLARATION_BYTES as u64)
            {
                return Err(U::TooLarge);
            }
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|error| {
                if error.is_timeout() {
                    U::Timeout
                } else {
                    U::HttpFailure
                }
            })? {
                if chunk.len() > MAX_DECLARATION_BYTES.saturating_sub(body.len()) {
                    return Err(U::TooLarge);
                }
                body.extend_from_slice(&chunk);
            }
            serde_json::from_slice::<UpstreamDeclaration>(&body).map_err(|_| U::Invalid)
        }
        .await;
        match result {
            Ok(declaration) => ProviderTopology::relay(declaration),
            Err(reason) => ProviderTopology::unknown(reason),
        }
    }

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
        mut on_progress: Box<dyn FnMut(ScanProgress) + Send>,
    ) -> anyhow::Result<()> {
        let url = self.ws_url(&site);
        let (ws_stream, _) = tokio_tungstenite::connect_async(&url)
            .await
            .map_err(|e| anyhow::anyhow!("connecting to relay at {url}: {e}"))?;
        let mut read = ws_stream;

        let mut merged = base;
        let mut source_scope = SourceVolumeCursor::new(&site);
        let mut current_volume: Option<VolumeKey> = None;
        let mut pending_blocks: Vec<LiveLevel2Block> = Vec::new();
        let mut update_count: u64 = 0;
        let mut passes = wxdata::live_pass::PassTracker::default();
        let mut last_decoded_sequence: Option<u64> = None;
        let mut sequences: Option<SequenceLedger> = None;

        while active() {
            // A quiet relay must still release a superseded/background subscription. Keep the
            // read future alive between checks: no reconnects or loss of partially read frames.
            let next = read.next();
            tokio::pin!(next);
            let msg = loop {
                tokio::select! {
                    msg = &mut next => break msg,
                    _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {
                        if !active() { return Ok(()); }
                    }
                }
            };
            // Cancellation may race with a ready frame. Do not decode/deliver it after stop.
            if !active() {
                return Ok(());
            }
            let Some(msg) = msg else {
                break; // relay closed the connection
            };
            let received_at = wxdata::clock::Instant::now();
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

            // The socket subscription is the outer scope of every accepted receipt.
            if !block.site.eq_ignore_ascii_case(&site)
                || !block.volume.site.eq_ignore_ascii_case(&site)
            {
                source_scope.refuse_foreign_radar();
                log::warn!("relay {site}: discarded block for another radar");
                continue;
            }

            let admission = source_scope.admit(&block.volume, None);
            if let SourceAdmission::Refused(reason) = admission {
                log::warn!("relay {site}: source scope refused: {reason}");
                continue;
            }

            // A new volume from the relay's own identity model: never mix blocks from two
            // different volumes into one assembly attempt (ROADMAP_NEW B6.5's cross-source
            // mixing guard applies just as much within a single source's own volume rollover).
            let origin = SequenceOrigin::RelayBlocks {
                upstream_id: Some(block.source_id.clone()),
            };
            let upstream_changed = sequences
                .as_ref()
                .is_some_and(|ledger| ledger.origin() != &origin);
            if matches!(
                admission,
                SourceAdmission::FirstVolume | SourceAdmission::NewVolume
            ) || upstream_changed
            {
                if admission == SourceAdmission::CurrentVolume && upstream_changed {
                    source_scope.declared_upstream_reset();
                }
                pending_blocks.clear();
                current_volume = Some(block.volume.clone());
                passes = wxdata::live_pass::PassTracker::default();
                last_decoded_sequence = None;
                // A changed declared upstream cannot lend its counter or payload prefix to
                // another source. Even an unchanged label proves no emitter instance identity.
                sequences = Some(SequenceLedger::new(origin));
            }
            let sequences = sequences
                .as_mut()
                .expect("validated block established source scope");
            sequences.observe(block.sequence);
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
                Err(_) => {
                    sequences.decode_failed();
                    last_decoded_sequence = None;
                    continue;
                }
            };
            let progress = pending_blocks
                .last()
                .and_then(|block| relay_scan_progress(block, &partial));
            let sequence = pending_blocks
                .last()
                .expect("current source block")
                .sequence;
            if relay_sequences_contiguous(pending_blocks.iter().map(|block| block.sequence)) {
                passes.observe(
                    &partial,
                    last_decoded_sequence.and_then(|old| old.checked_add(1)) == Some(sequence),
                );
            } else {
                passes.observe_discontinuous_assembly(&partial);
            }
            last_decoded_sequence = Some(sequence);
            let mut radial_coverage = progress.map(|progress| {
                let block = pending_blocks
                    .last()
                    .expect("the current block was appended");
                let start = block.radar_start.timestamp_millis();
                let end = block.radar_end.timestamp_millis();
                RadialCoverage {
                    progress,
                    radials: partial
                        .sweeps()
                        .iter()
                        .filter(|sweep| {
                            sweep.elevation_number() as usize == progress.elevation_number
                        })
                        .flat_map(|sweep| sweep.radials())
                        .filter(|radial| (start..=end).contains(&radial.collection_timestamp()))
                        .map(|radial| (radial.azimuth_number(), radial.collection_timestamp()))
                        .collect(),
                    source_passes: Some(passes.inventory()),
                    source_sequences: Some(Box::new(sequences.inventory())),
                    source_attribution: None,
                    source_scope: source_scope.receipt().map(Arc::new),
                }
            });
            let (new_scan, changed) = wxdata::live::merge_scan(&merged, partial);
            if changed.is_empty() {
                continue;
            }
            if let Some(progress) = progress {
                on_progress(progress);
            }
            if let Some(coverage) = &mut radial_coverage {
                coverage.source_attribution =
                    Some(Arc::new(passes.attribution_for_scan(&new_scan)));
            }
            merged = Arc::new(new_scan);
            update_count += 1;
            // Use the newest radar acquisition time, never the client's wall clock. A relay
            // catching up after failover can deliver a valid but older block; stamping it "now"
            // would defeat the app's time-reversal guard and source-age display.
            let newest_radial_time = pending_blocks
                .iter()
                .filter(|block| block.first_azimuth_number.is_some())
                .map(|block| block.radar_end)
                .max()
                .unwrap_or_else(|| {
                    pending_blocks
                        .iter()
                        .map(|block| block.radar_end)
                        .max()
                        .expect("a successfully assembled volume has at least one block")
                });
            let volume_start_ms = current_volume
                .as_ref()
                .expect("the current block established a volume")
                .volume_start
                .timestamp_millis();
            on_update(Update {
                name: format!("relay-{site}-{volume_start_ms}-{update_count}"),
                time: newest_radial_time,
                received_at: Some(received_at),
                radial_coverage,
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

        let first = &blocks[0];
        anyhow::ensure!(
            blocks
                .iter()
                .all(|block| block.site.eq_ignore_ascii_case(site)
                    && block.volume.site.eq_ignore_ascii_case(site)
                    && block.volume == first.volume
                    && block.source_id == first.source_id),
            "relay {site}: completed volume contains incompatible radar, volume or upstream scope"
        );
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

/// A mid-volume join's first sequence is an unknown prefix, not a gap. Only positions actually
/// present in a coalesced input are checked; a jump, duplicate or reversal makes association unsafe.
fn relay_sequences_contiguous(mut sequences: impl Iterator<Item = u64>) -> bool {
    let Some(mut previous) = sequences.next() else {
        return false;
    };
    sequences.all(|sequence| {
        let continuous = previous.checked_add(1) == Some(sequence);
        previous = sequence;
        continuous
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesced_relay_positions_require_continuity_without_dating_an_unknown_prefix() {
        assert!(relay_sequences_contiguous([500, 501, 502].into_iter()));
        assert!(relay_sequences_contiguous([u64::MAX].into_iter()));
        assert!(!relay_sequences_contiguous([500, 502].into_iter()));
        assert!(!relay_sequences_contiguous([500, 500].into_iter()));
        assert!(!relay_sequences_contiguous([502, 501].into_iter()));
        assert!(!relay_sequences_contiguous([u64::MAX, 0].into_iter()));
        assert!(!relay_sequences_contiguous([].into_iter()));
    }

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
        assert_eq!(
            provider.http_url("/provider"),
            "http://localhost:8080/provider"
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

    fn a_completed_volume_at(site: &str, t: chrono::DateTime<chrono::Utc>) -> Vec<u8> {
        [
            synthetic_vcp(t),
            synthetic_radial(site, 1, 1, VOLUME_START, t),
            synthetic_radial(site, 1, 2, ELEVATION_END, t),
            synthetic_radial(site, 2, 1, VOLUME_END, t),
        ]
        .concat()
    }

    #[tokio::test]
    async fn relay_topology_roundtrips_real_server_metadata_without_using_deployment_labels() {
        use wxdata::provider_topology::{
            InputMode, ProviderTopology, UpstreamDeclaration, UNIDATA_AWS_DOMAIN,
        };
        let pipeline = Pipeline::new(
            RechunkConfig::default(),
            BlockStoreLimits::default(),
            "independent-looking-deployment",
        );
        let declaration =
            UpstreamDeclaration::new(InputMode::Replay, vec![UNIDATA_AWS_DOMAIN.into()]).unwrap();
        let app = radar_ingest::server::router_with_declaration(
            Arc::new(Mutex::new(pipeline)),
            declaration.clone(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let provider = HookEchoRelayLevel2Provider::new(address.to_string());
        assert_eq!(
            provider.inspect_topology().await,
            ProviderTopology::relay(declaration)
        );
        server.abort();
    }

    #[tokio::test]
    async fn relay_topology_missing_invalid_redirected_and_oversized_metadata_stays_unknown() {
        use axum::{
            body::Body,
            http::{Response, StatusCode},
            routing::get,
            Router,
        };
        use wxdata::provider_topology::{
            DeclarationUnavailable as U, ProviderTopology, MAX_DECLARATION_BYTES,
        };
        // Both a declared length and chunked transfer are bounded. Raw body/Location text
        // never enters the fixed diagnostic reasons.
        let oversized = "s".repeat(MAX_DECLARATION_BYTES + 1);
        let chunked = oversized.clone();
        let cases = [
            (Router::new(), U::HttpFailure),
            (
                Router::new().route(
                    "/provider",
                    get(|| async { r#"{"schema_version":99,"token":"untrusted-secret"}"# }),
                ),
                U::Invalid,
            ),
            (
                Router::new().route(
                    "/provider",
                    get(|| async {
                        (
                            StatusCode::FOUND,
                            [("Location", "http://example.invalid/untrusted-secret")],
                        )
                    }),
                ),
                U::HttpFailure,
            ),
            (
                Router::new().route("/provider", get(move || async move { oversized })),
                U::TooLarge,
            ),
            (
                Router::new().route(
                    "/provider",
                    get(move || async move {
                        let stream =
                            futures_util::stream::once(
                                async move { Ok::<_, std::io::Error>(chunked) },
                            );
                        Response::new(Body::from_stream(stream))
                    }),
                ),
                U::TooLarge,
            ),
        ];
        for (app, reason) in cases {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let result = HookEchoRelayLevel2Provider::new(format!("http://{address}"))
                .inspect_topology()
                .await;
            assert_eq!(result, ProviderTopology::unknown(reason));
            let retained = serde_json::to_string(&result).unwrap();
            assert!(
                !retained.contains("untrusted-secret") && !retained.contains(&address.to_string())
            );
            server.abort();
        }
    }

    #[tokio::test]
    async fn relay_topology_times_out_when_response_body_never_finishes() {
        use axum::{body::Body, routing::get, Router};
        use wxdata::provider_topology::{DeclarationUnavailable, ProviderTopology};
        let app = Router::new().route(
            "/provider",
            get(|| async {
                Body::from_stream(futures_util::stream::pending::<
                    Result<String, std::io::Error>,
                >())
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(4),
            HookEchoRelayLevel2Provider::new(format!("http://{address}")).inspect_topology(),
        )
        .await
        .unwrap();
        assert_eq!(
            result,
            ProviderTopology::unknown(DeclarationUnavailable::Timeout)
        );
        server.abort();
    }

    #[tokio::test]
    async fn latest_complete_volume_fetches_and_assembles_a_real_scan_over_http() {
        let mut pipeline = Pipeline::new(
            RechunkConfig::default(),
            BlockStoreLimits::default(),
            "relay",
        );
        let source_time = chrono::Utc::now() - chrono::Duration::minutes(2);
        pipeline.ingest(&RawProduct {
            site: "KTLX".into(),
            bytes: a_completed_volume_at("KTLX", source_time),
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
            LatestVolume::New { scan, time, .. } => {
                assert_eq!(scan.sweeps().len(), 2, "two elevation cuts were ingested");
                assert_eq!(time.timestamp(), source_time.timestamp());
            }
            LatestVolume::UpToDate => panic!("a real completed volume was ingested"),
        }
    }

    #[tokio::test]
    async fn completed_http_volume_refuses_incompatible_envelope_scopes_before_assembly() {
        use wxdata::live_block::checksum;
        let time = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let payload = a_completed_volume_at("KTLX", time);
        let block = LiveLevel2Block {
            site: "KTLX".into(),
            volume: VolumeKey::new("KTLX", time),
            cut: None,
            elevation_angle_deg: None,
            first_azimuth_number: None,
            last_azimuth_number: None,
            radar_start: time,
            radar_end: time,
            received_at: time,
            emitted_at: time,
            sequence: 1,
            source_id: "ldm".into(),
            checksum: checksum(&payload),
            payload,
        };
        for mismatch in 0..4 {
            let mut bad = block.clone();
            match mismatch {
                0 => bad.site = "KOUN".into(),
                1 => bad.volume.site = "KOUN".into(),
                2 => bad.volume = VolumeKey::new("KTLX", time + chrono::Duration::seconds(60)),
                _ => bad.source_id = "different-label".into(),
            }
            let body =
                serde_json::to_string(&vec![BlockDto::from(&block), BlockDto::from(&bad)]).unwrap();
            let app = axum::Router::new().route(
                "/sites/KTLX/volume/latest",
                axum::routing::get(move || async move { body }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let provider = HookEchoRelayLevel2Provider::new(format!("http://{address}"));
            let error = provider
                .latest_complete_volume("KTLX", None)
                .await
                .err()
                .expect("mixed envelope scope must be rejected");
            assert!(error
                .to_string()
                .contains("incompatible radar, volume or upstream scope"));
            server.abort();
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
    async fn websocket_sequence_receipts_track_late_fill_duplicates_and_source_rollover() {
        use futures_util::SinkExt;
        use wxdata::live_block::{checksum, CutKey};
        let time = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let make = |sequence, offset, source: &str, volume_offset| {
            let clock = time + chrono::Duration::seconds(offset);
            let payload = a_completed_volume_at("KTLX", clock);
            LiveLevel2Block {
                site: "KTLX".into(),
                volume: VolumeKey::new("KTLX", time + chrono::Duration::seconds(volume_offset)),
                cut: Some(CutKey {
                    elevation_number: 1,
                    repeat_index: 0,
                }),
                elevation_angle_deg: Some(0.5),
                first_azimuth_number: Some(1),
                last_azimuth_number: Some(2),
                radar_start: clock,
                radar_end: clock,
                received_at: clock,
                emitted_at: clock,
                sequence,
                source_id: source.into(),
                checksum: checksum(&payload),
                payload,
            }
        };
        let full = make(502, 1, "ldm", 0);
        let decoded = assemble_scan(std::slice::from_ref(&full)).unwrap();
        let base = Arc::new(Scan::new(decoded.coverage_pattern().clone(), Vec::new()));
        let mut no_vcp = make(500, 0, "ldm", 0);
        no_vcp.payload = synthetic_radial("KTLX", 1, 1, VOLUME_START, time);
        no_vcp.checksum = checksum(&no_vcp.payload);
        let late = make(501, 2, "ldm", 0);
        let mut corrupt = BlockDto::from(&full);
        corrupt.sequence = 5000;
        corrupt.checksum_hex = "0".repeat(64);
        let mut foreign = make(6000, 5, "ldm", 0);
        foreign.site = "KOUN".into();
        let mut foreign_volume = make(6001, 6, "ldm", 0);
        foreign_volume.volume.site = "KOUN".into();
        let messages = [
            "malformed JSON".to_string(),
            serde_json::to_string(&BlockDto::from(&foreign)).unwrap(),
            serde_json::to_string(&BlockDto::from(&foreign_volume)).unwrap(),
            serde_json::to_string(&corrupt).unwrap(),
            serde_json::to_string(&BlockDto::from(&no_vcp)).unwrap(),
            serde_json::to_string(&BlockDto::from(&full)).unwrap(),
            serde_json::to_string(&BlockDto::from(&late)).unwrap(),
            serde_json::to_string(&BlockDto::from(&late)).unwrap(),
            serde_json::to_string(&BlockDto::from(&make(503, 3, "ldm", 0))).unwrap(),
            serde_json::to_string(&BlockDto::from(&make(0, 4, "other-upstream", 0))).unwrap(),
            serde_json::to_string(&BlockDto::from(&make(5, 61, "other-upstream", 60))).unwrap(),
            // A delayed older envelope from another label must not reset the current upstream.
            serde_json::to_string(&BlockDto::from(&make(9000, 62, "late-upstream", 0))).unwrap(),
            serde_json::to_string(&BlockDto::from(&make(6, 63, "other-upstream", 60))).unwrap(),
        ];
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
            for message in messages {
                ws.send(tokio_tungstenite::tungstenite::Message::Text(message))
                    .await
                    .unwrap();
            }
            ws.close(None).await.unwrap();
        });
        let updates = Arc::new(Mutex::new(Vec::new()));
        let collect = updates.clone();
        let provider = HookEchoRelayLevel2Provider::new(format!("http://{address}"));
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            provider.subscribe(
                "KTLX".into(),
                base,
                Box::new(|| true),
                Box::new(move |update| collect.lock().unwrap().push(update)),
                Box::new(|_| {}),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        server.await.unwrap();
        let updates = updates.lock().unwrap();
        assert_eq!(
            updates.len(),
            6,
            "duplicate-only decode does not produce an accepted gate update"
        );
        for update in updates.iter() {
            let index = update
                .radial_coverage
                .as_ref()
                .unwrap()
                .source_attribution
                .as_ref()
                .unwrap();
            assert!(
                update
                    .scan
                    .sweeps()
                    .iter()
                    .any(|sweep| sweep.radials().iter().any(|radial| {
                        matches!(
                            index.resolve(wxdata::live_pass::NativeRadialKey::from_radial(
                                u16::from(sweep.elevation_number()),
                                radial
                            )),
                            wxdata::live_pass::RowPass::Anchored(_)
                        )
                    })),
                "accepted websocket inputs retain exact native starts"
            );
        }
        let receipts: Vec<_> = updates
            .iter()
            .map(|update| {
                update
                    .radial_coverage
                    .as_ref()
                    .unwrap()
                    .source_sequences
                    .as_ref()
                    .unwrap()
            })
            .collect();
        assert_eq!(
            receipts[0].received_bounds,
            Some((500, 502)),
            "corrupt metadata cannot become a source position"
        );
        assert_eq!(receipts[0].bounded_holes, [(501, 501)]);
        assert_eq!(receipts[0].failed_decode_attempts, 1);
        assert_eq!(receipts[1].recovered_spans, [(501, 501)]);
        assert!(receipts[1].bounded_holes.is_empty());
        assert_eq!(
            receipts[2].duplicate_arrivals, 1,
            "non-rendering arrivals survive to the next accepted frame"
        );
        assert_eq!(receipts[2].retained_received, 4);
        assert_eq!(receipts[3].received_bounds, Some((0, 0)));
        assert_eq!(
            receipts[3].origin,
            SequenceOrigin::RelayBlocks {
                upstream_id: Some("other-upstream".into())
            }
        );
        assert_eq!(receipts[3].failed_decode_attempts, 0);
        assert_eq!(
            receipts[4].received_bounds,
            Some((5, 5)),
            "new volume has no invented prefix holes"
        );
        assert_eq!(receipts[5].received_bounds, Some((5, 6)));
        assert_eq!(receipts[5].origin, receipts[4].origin);
        let scopes: Vec<_> = updates
            .iter()
            .map(|update| {
                update
                    .radial_coverage
                    .as_ref()
                    .unwrap()
                    .source_scope
                    .as_ref()
                    .unwrap()
            })
            .collect();
        assert_eq!(scopes[0].refused_foreign_radars, 2);
        assert_eq!(scopes[3].declared_upstream_resets, 1);
        assert_eq!(scopes[4].volume_rollovers, 1);
        assert_eq!(
            scopes[4].refused_older_volumes, 0,
            "earlier accepted receipts stay frozen"
        );
        assert_eq!(scopes[5].refused_older_volumes, 1);
        assert_eq!(
            scopes[5].declared_upstream_resets, 1,
            "refused label cannot change origin"
        );
        assert!(updates.windows(2).all(|pair| pair[0].time <= pair[1].time));
        assert_eq!(receipts[4].recovered_positions, 0);
        assert_eq!(
            receipts[0].bounded_holes,
            [(501, 501)],
            "accepted receipt is immutable"
        );
    }

    #[tokio::test]
    async fn subscribe_receives_live_update_and_progress_over_a_real_websocket() {
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
        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
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
                    Box::new(move |progress| {
                        let _ = progress_tx.send(progress);
                    }),
                )
                .await
        });

        // Give the WebSocket handshake + server-side subscribe a moment to land before ingesting
        // — a short, generous sleep rather than a synchronization primitive threaded through
        // production code purely for test determinism.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let source_time = chrono::Utc::now() - chrono::Duration::minutes(2);
        pipeline.lock().unwrap().ingest(&RawProduct {
            site: "KTLX".into(),
            bytes: a_completed_volume_at("KTLX", source_time),
            received_at: chrono::Utc::now(),
        });

        let update = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("no live update arrived over the websocket in time")
            .expect("update channel closed unexpectedly");
        assert!(!update.changed.is_empty());
        assert!(!update.scan.sweeps().is_empty());
        assert_eq!(
            update.time.timestamp_millis(),
            source_time.timestamp_millis()
        );
        assert!(update
            .name
            .contains(&(source_time.timestamp() * 1_000).to_string()));
        let passes = update
            .radial_coverage
            .as_ref()
            .and_then(|coverage| coverage.source_passes.as_ref())
            .expect("native boundary evidence delivered over the real relay transport");
        assert!(passes
            .passes
            .iter()
            .any(|pass| pass.key.elevation_number == 1
                && pass.key.start_ms == source_time.timestamp_millis()));
        let progress = tokio::time::timeout(std::time::Duration::from_secs(5), progress_rx.recv())
            .await
            .expect("no live progress arrived over the websocket in time")
            .expect("progress channel closed unexpectedly");
        assert!(progress.elevation_number > 0);
        assert!(progress.azimuth_span_deg() > 0.0);
        assert!(progress.chunk_duration_secs() > 0.0);

        active.store(false, std::sync::atomic::Ordering::Relaxed);
        // The server stays open without another ingest. Cancellation must release an idle read,
        // not rely on another message to wake it or leave a detached task behind.
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("idle relay subscription did not stop after cancellation")
            .expect("relay subscription task panicked")
            .expect("intentional cancellation reported a transport error");
    }

    /// One relay server in-process, its address, and its pipeline to ingest into. Every radial
    /// is its own block, so a volume cut off mid-tilt still reaches the client.
    async fn relay_server() -> (std::net::SocketAddr, Arc<Mutex<Pipeline>>) {
        let pipeline = Arc::new(Mutex::new(Pipeline::new(
            RechunkConfig {
                max_radials_per_block: 1,
                ..RechunkConfig::default()
            },
            BlockStoreLimits::default(),
            "relay",
        )));
        let app = radar_ingest::server::router(pipeline.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (addr, pipeline)
    }

    /// Subscribe to `addr` with `base`, ingest `bytes`, and return the scan once updates stop, or
    /// `None` if no update arrives within five seconds.
    async fn scan_after(
        addr: std::net::SocketAddr,
        pipeline: Arc<Mutex<Pipeline>>,
        base: Arc<wxdata::level2::Scan>,
        bytes: Vec<u8>,
    ) -> Option<Arc<wxdata::level2::Scan>> {
        let provider = HookEchoRelayLevel2Provider::new(format!("http://{addr}"));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let flag = active.clone();
        let handle = tokio::spawn(async move {
            provider
                .subscribe(
                    "KTLX".to_string(),
                    base,
                    Box::new(move || flag.load(std::sync::atomic::Ordering::Relaxed)),
                    Box::new(move |update| {
                        let _ = tx.send(update);
                    }),
                    Box::new(|_| {}),
                )
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        pipeline.lock().unwrap().ingest(&RawProduct {
            site: "KTLX".into(),
            bytes,
            received_at: chrono::Utc::now(),
        });
        let mut update = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .ok()
            .flatten();
        while let Ok(Some(next)) =
            tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await
        {
            update = Some(next);
        }
        active.store(false, std::sync::atomic::Ordering::Relaxed);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
        update.map(|update| update.scan)
    }

    fn azimuths(scan: &wxdata::level2::Scan, elevation: u8) -> Vec<u16> {
        let mut v: Vec<u16> = scan
            .sweeps()
            .iter()
            .filter(|s| s.elevation_number() == elevation)
            .flat_map(|s| s.radials().iter().map(|r| r.azimuth_number()))
            .collect();
        v.sort_unstable();
        v
    }

    /// A provider switch mid-volume over the real wire (1008.md A2/A5). The primary delivered the
    /// first two radials of a volume; the app starts the backup with that scan as its base (as
    /// `spawn_stream` does), and the backup relay serves the whole volume. The backup's first
    /// update holds every azimuth once: the primary's radials continue, the overlap is not drawn
    /// twice, and nothing the primary delivered is lost. A backup serving a volume more than the
    /// retention window older sends nothing, and an older shown radial is dropped, not mixed in.
    #[tokio::test]
    async fn a_backup_continues_the_primarys_volume_without_duplicating_radials() {
        let t = chrono::Utc::now() - chrono::Duration::minutes(2);
        let (primary_addr, primary) = relay_server().await;
        let empty = Arc::new(wxdata::level2::Scan::new(
            nexrad_model::data::VolumeCoveragePattern::new(
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
            ),
            Vec::new(),
        ));
        // The primary: the volume's first two radials at the lowest tilt, then the feed is lost.
        let primary_part = [
            synthetic_vcp(t),
            synthetic_radial("KTLX", 1, 1, VOLUME_START, t),
            synthetic_radial("KTLX", 1, 2, 1, t),
        ]
        .concat();
        let shown = scan_after(primary_addr, primary, empty.clone(), primary_part)
            .await
            .expect("the primary's update");
        assert_eq!(azimuths(&shown, 1), [1, 2], "what the primary delivered");

        // The backup relay has the whole volume, the primary's two radials included.
        let (backup_addr, backup) = relay_server().await;
        let whole = [
            synthetic_vcp(t),
            synthetic_radial("KTLX", 1, 1, VOLUME_START, t),
            synthetic_radial("KTLX", 1, 2, 1, t),
            synthetic_radial("KTLX", 1, 3, 1, t),
            synthetic_radial("KTLX", 1, 4, ELEVATION_END, t),
            synthetic_radial("KTLX", 2, 1, VOLUME_END, t),
        ]
        .concat();
        let continued = scan_after(backup_addr, backup, shown, whole)
            .await
            .expect("the backup's update");
        assert_eq!(
            azimuths(&continued, 1),
            [1, 2, 3, 4],
            "each azimuth once: the overlap is not drawn twice and nothing is lost"
        );
        assert_eq!(azimuths(&continued, 2), [1]);

        // A backup lagging a volume 20 minutes behind the shown scan: every radial it has is
        // pruned against the newer base, so it sends nothing and cannot rewind the display.
        let old_t = t - chrono::Duration::minutes(20);
        let old_part = [
            synthetic_vcp(old_t),
            synthetic_radial("KTLX", 1, 9, VOLUME_START, old_t),
        ]
        .concat();
        let (lag_addr, lagging) = relay_server().await;
        assert!(
            scan_after(lag_addr, lagging, continued, old_part.clone())
                .await
                .is_none(),
            "a lagging backup sends no update over a newer scan"
        );

        // A shown scan from a volume older than the retention window: its radial is pruned
        // from the tilt the backup refreshes, never mixed into the new volume there.
        let (old_addr, old_relay) = relay_server().await;
        let old_shown = scan_after(old_addr, old_relay, empty, old_part)
            .await
            .expect("the old volume's update");
        assert!(azimuths(&old_shown, 1).contains(&9));
        let (fresh_addr, fresh) = relay_server().await;
        let fresh_part = [
            synthetic_vcp(t),
            synthetic_radial("KTLX", 1, 1, VOLUME_START, t),
        ]
        .concat();
        let rolled = scan_after(fresh_addr, fresh, old_shown, fresh_part)
            .await
            .expect("the current volume's update");
        assert!(
            !azimuths(&rolled, 1).contains(&9),
            "a radial 20 minutes older than the newest is not kept: {:?}",
            azimuths(&rolled, 1)
        );
    }
}
