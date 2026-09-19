//! B6.4's HTTP/WebSocket distribution API: a per-site head/manifest endpoint, fetch-a-specific-
//! block-by-sequence, and a live WebSocket stream with `resume_after=<sequence>` reconnect
//! semantics. Also B6.10's operational routes: `/health`/`/ready` (liveness/readiness) and
//! `/metrics` (plain Prometheus text — per-site retention gauges plus the process's stream epoch).
//!
//! TLS and public-facing rate limiting/connection caps are explicitly the deployment's reverse
//! proxy's job (ROADMAP_NEW B6.4) — this module only speaks plain HTTP/WS, meant to sit behind
//! one.

use crate::pipeline::Pipeline;
use crate::wire::{BlockDto, ManifestDto};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

#[derive(Clone)]
struct AppState {
    pipeline: Arc<Mutex<Pipeline>>,
}

/// Build the router. `pipeline` is shared with whatever task drains the input adapter and feeds
/// [`Pipeline::ingest`]/[`Pipeline::tick`] — a plain `std::sync::Mutex` is enough since every
/// critical section here is synchronous (no `.await` while holding the lock).
pub fn router(pipeline: Arc<Mutex<Pipeline>>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/metrics", get(metrics))
        .route("/sites/{site}/head", get(head))
        .route("/sites/{site}/blocks/{sequence}", get(block_by_sequence))
        .route("/sites/{site}/volume/latest", get(latest_complete_volume))
        .route("/sites/{site}/live", get(live))
        .with_state(AppState { pipeline })
}

/// Liveness: the process is up and serving requests at all.
async fn health() -> &'static str {
    "ok"
}

/// Readiness: distinct from `health` per B6.2/B6.10 so a deployment's orchestrator can tell "the
/// process exists" apart from "the process is ready to take traffic." Nothing in this service
/// currently has a warm-up phase or an external dependency to check, so today the two agree; the
/// route is kept separate so a future check (e.g. "the LDM input adapter is connected") has
/// somewhere to plug in without changing the API shape clients already depend on.
async fn ready() -> &'static str {
    "ok"
}

/// Plain Prometheus text exposition format (ROADMAP_NEW B6.10) — no metrics library dependency
/// for the handful of gauges this service has today; the format itself is just newline-separated
/// text, not worth a crate. `mem_limit`/`cpus`/`pids_limit` in `docker-compose.radar-ingest.yml`
/// already bound the process from outside; these gauges are for watching it, not enforcing bounds.
async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let pipeline = state.pipeline.lock().unwrap();
    let mut out = String::new();
    out.push_str("# HELP radar_ingest_epoch Opaque value that changes every process start.\n");
    out.push_str("# TYPE radar_ingest_epoch gauge\n");
    out.push_str(&format!("radar_ingest_epoch {}\n", pipeline.epoch()));

    out.push_str(
        "# HELP radar_ingest_site_blocks Blocks currently retained for a site, by sequence.\n",
    );
    out.push_str("# TYPE radar_ingest_site_blocks gauge\n");
    for (site, items, _) in pipeline.retention_stats() {
        out.push_str(&format!(
            "radar_ingest_site_blocks{{site=\"{site}\"}} {items}\n"
        ));
    }

    out.push_str("# HELP radar_ingest_site_bytes Retained block payload bytes for a site.\n");
    out.push_str("# TYPE radar_ingest_site_bytes gauge\n");
    for (site, _, bytes) in pipeline.retention_stats() {
        out.push_str(&format!(
            "radar_ingest_site_bytes{{site=\"{site}\"}} {bytes}\n"
        ));
    }
    out
}

async fn head(State(state): State<AppState>, Path(site): Path<String>) -> impl IntoResponse {
    let (manifest, epoch) = {
        let pipeline = state.pipeline.lock().unwrap();
        (pipeline.manifest(&site), pipeline.epoch())
    };
    match manifest {
        Some(manifest) => Json(ManifestDto::with_epoch(&manifest, epoch)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn block_by_sequence(
    State(state): State<AppState>,
    Path((site, sequence)): Path<(String, u64)>,
) -> impl IntoResponse {
    let block = {
        let pipeline = state.pipeline.lock().unwrap();
        pipeline.block(&site, sequence)
    };
    match block {
        Some(block) => Json(BlockDto::from(&block)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Every retained block belonging to the site's latest completed volume, oldest first
/// (ROADMAP_NEW B6.4's "optional completed-volume endpoint generated from the exact same
/// retained source bytes") — a client without a live WebSocket connection reassembles a full
/// scan from this, analogous to how `hookecho`'s `Level2LiveProvider::latest_complete_volume`
/// polls a completed volume from the Unidata path today.
async fn latest_complete_volume(
    State(state): State<AppState>,
    Path(site): Path<String>,
) -> impl IntoResponse {
    let blocks = {
        let pipeline = state.pipeline.lock().unwrap();
        pipeline.latest_complete_volume_blocks(&site)
    };
    match blocks {
        Some(blocks) => {
            let dtos: Vec<BlockDto> = blocks.iter().map(BlockDto::from).collect();
            Json(dtos).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct LiveQuery {
    /// Reconnect semantics (ROADMAP_NEW B6.4): a client that already has everything up to and
    /// including this sequence asks to resume from here, filling any gap from retained blocks
    /// before switching to genuinely live delivery, instead of starting over.
    resume_after: Option<u64>,
    /// The epoch (`ManifestDto::epoch`, from a prior `/head` or `/live` response) the client's
    /// `resume_after` sequence was learned under (ROADMAP_NEW B6.10). Omitted or mismatched
    /// against this process's current epoch means "not resumable" — `resume_after` is then
    /// ignored so a post-restart sequence collision can never serve the wrong block under a
    /// reused number, and the client is treated exactly like a fresh connection instead.
    epoch: Option<u64>,
}

async fn live(
    State(state): State<AppState>,
    Path(site): Path<String>,
    Query(query): Query<LiveQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        serve_live_socket(socket, state, site, query.resume_after, query.epoch)
    })
}

async fn serve_live_socket(
    mut socket: WebSocket,
    state: AppState,
    site: String,
    resume_after: Option<u64>,
    client_epoch: Option<u64>,
) {
    // Compute the backlog and subscribe to future blocks under the *same* lock acquisition, so no
    // block can be published in the gap between "read the backlog" and "start subscribing" — that
    // gap would otherwise either duplicate a block (published just before subscribing, but after
    // the backlog read) or lose one (published exactly in the gap).
    let (backlog, mut live_rx) = {
        let mut pipeline = state.pipeline.lock().unwrap();
        // A client that names an epoch which doesn't match this process's current one learned
        // `resume_after` from a server instance that no longer exists — its sequence number means
        // nothing here and must not be trusted to select a backlog (see `Pipeline::epoch`'s doc
        // comment). A client that never sends an epoch at all (older client, or a first-ever
        // connection with nothing to resume) keeps today's behavior unchanged.
        let resumable = match client_epoch {
            Some(claimed) => claimed == pipeline.epoch(),
            None => true,
        };
        let backlog = resume_after
            .filter(|_| resumable)
            .map(|seq| pipeline.blocks_after(&site, seq))
            .unwrap_or_default();
        (backlog, pipeline.subscribe(&site))
    };

    for block in backlog {
        if send_block(&mut socket, &block).await.is_err() {
            return;
        }
    }

    loop {
        tokio::select! {
            biased;
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => return,
                    Some(Ok(_)) => continue, // this stream is push-only; ignore anything a client sends
                    Some(Err(_)) => return,
                }
            }
            received = live_rx.recv() => {
                match received {
                    Ok(block) => {
                        if send_block(&mut socket, &block).await.is_err() {
                            return;
                        }
                    }
                    // The client fell far enough behind that the broadcast channel dropped
                    // messages for it. It can close and reconnect with `resume_after` set to the
                    // last sequence it actually saw, backfilling from retention rather than losing
                    // data silently — so keep the connection open rather than closing on lag.
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    }
}

async fn send_block(
    socket: &mut WebSocket,
    block: &wxdata::live_block::LiveLevel2Block,
) -> Result<(), axum::Error> {
    let dto = BlockDto::from(block);
    let text = serde_json::to_string(&dto).expect("BlockDto always serializes");
    socket.send(Message::Text(text.into())).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_store::BlockStoreLimits;
    use crate::input::RawProduct;
    use crate::rechunk::{test_support::synthetic_radial, RechunkConfig};
    use axum::body::Body;
    use axum::http::Request;
    use chrono::Utc;
    use futures_util::StreamExt;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const ELEVATION_START: u8 = 0;
    const ELEVATION_END: u8 = 2;
    const VOLUME_START: u8 = 3;
    const VOLUME_END: u8 = 4;

    fn pipeline_with_one_block() -> Arc<Mutex<Pipeline>> {
        let mut pipeline = Pipeline::new(
            RechunkConfig::default(),
            BlockStoreLimits::default(),
            "relay",
        );
        let bytes = [
            synthetic_radial("KTLX", 1, 0.5, 0, VOLUME_START, Utc::now()),
            synthetic_radial("KTLX", 1, 0.5, 1, ELEVATION_END, Utc::now()),
        ]
        .concat();
        pipeline.ingest(&RawProduct {
            site: "KTLX".into(),
            bytes,
            received_at: Utc::now(),
        });
        Arc::new(Mutex::new(pipeline))
    }

    #[tokio::test]
    async fn metrics_reports_epoch_and_per_site_retention() {
        let app = router(pipeline_with_one_block());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("radar_ingest_epoch "));
        assert!(text.contains("radar_ingest_site_blocks{site=\"KTLX\"} 1"));
        assert!(text.contains("radar_ingest_site_bytes{site=\"KTLX\"} "));
    }

    #[tokio::test]
    async fn metrics_reports_nothing_for_sites_with_no_activity() {
        let app = router(Arc::new(Mutex::new(Pipeline::new(
            RechunkConfig::default(),
            BlockStoreLimits::default(),
            "relay",
        ))));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            !text.contains("site="),
            "no site has ever ingested anything yet"
        );
    }

    #[tokio::test]
    async fn health_and_ready_are_always_ok() {
        let app = router(Arc::new(Mutex::new(Pipeline::new(
            RechunkConfig::default(),
            BlockStoreLimits::default(),
            "relay",
        ))));
        for path in ["/health", "/ready"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn head_is_not_found_for_an_unknown_site() {
        let app = router(Arc::new(Mutex::new(Pipeline::new(
            RechunkConfig::default(),
            BlockStoreLimits::default(),
            "relay",
        ))));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sites/KABC/head")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn head_reports_the_manifest_for_a_site_with_activity() {
        let app = router(pipeline_with_one_block());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sites/KTLX/head")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let manifest: ManifestDto = serde_json::from_slice(&body).unwrap();
        assert_eq!(manifest.newest_sequence, Some(0));
        assert!(manifest.current_volume.is_some());
        assert_ne!(
            manifest.epoch, 0,
            "a real process must report a nonzero epoch"
        );
    }

    #[tokio::test]
    async fn block_by_sequence_round_trips_the_original_payload() {
        let app = router(pipeline_with_one_block());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sites/KTLX/blocks/0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let block: BlockDto = serde_json::from_slice(&body).unwrap();
        assert_eq!(block.sequence, 0);
        assert_eq!(block.site, "KTLX");
    }

    #[tokio::test]
    async fn block_by_sequence_is_not_found_past_the_newest_sequence() {
        let app = router(pipeline_with_one_block());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sites/KTLX/blocks/99")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn latest_complete_volume_is_not_found_before_any_volume_completes() {
        // pipeline_with_one_block() ends its one block on ElevationEnd, not VolumeScanEnd — no
        // volume has actually completed yet.
        let app = router(pipeline_with_one_block());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sites/KTLX/volume/latest")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn latest_complete_volume_returns_every_block_of_the_completed_volume() {
        let mut pipeline = Pipeline::new(
            RechunkConfig::default(),
            BlockStoreLimits::default(),
            "relay",
        );
        let bytes = [
            synthetic_radial("KTLX", 1, 0.5, 0, VOLUME_START, Utc::now()),
            synthetic_radial("KTLX", 1, 0.5, 1, ELEVATION_END, Utc::now()),
            synthetic_radial("KTLX", 2, 1.5, 0, ELEVATION_START, Utc::now()),
            synthetic_radial("KTLX", 2, 1.5, 1, VOLUME_END, Utc::now()),
        ]
        .concat();
        pipeline.ingest(&RawProduct {
            site: "KTLX".into(),
            bytes,
            received_at: Utc::now(),
        });
        let app = router(Arc::new(Mutex::new(pipeline)));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sites/KTLX/volume/latest")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let blocks: Vec<BlockDto> = serde_json::from_slice(&body).unwrap();
        assert_eq!(blocks.len(), 2, "one block per elevation cut");
        assert!(blocks.iter().all(|b| b.volume == blocks[0].volume));
    }

    /// End-to-end over a real socket: `oneshot` cannot exercise a protocol upgrade, so this binds
    /// an actual `TcpListener`, serves the router on it, and drives a real WebSocket client
    /// against it — exactly what a resuming HookEcho client does.
    #[tokio::test]
    async fn live_websocket_replays_backlog_then_streams_new_blocks() {
        // Two blocks ingested before anyone connects: sequence 0 (elevation 1) and sequence 1
        // (elevation 2). A client that already has sequence 0 and reconnects with
        // `resume_after=0` should be replayed exactly sequence 1 as backlog — not sequence 0
        // again, and not made to wait for a third, genuinely live block.
        let pipeline = pipeline_with_one_block();
        {
            let bytes = [
                synthetic_radial("KTLX", 2, 1.5, 0, VOLUME_START, Utc::now()),
                synthetic_radial("KTLX", 2, 1.5, 1, ELEVATION_END, Utc::now()),
            ]
            .concat();
            pipeline.lock().unwrap().ingest(&RawProduct {
                site: "KTLX".into(),
                bytes,
                received_at: Utc::now(),
            });
        }

        let app = router(pipeline.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let url = format!("ws://{addr}/sites/KTLX/live?resume_after=0");
        let (mut ws_stream, _) = tokio_tungstenite::connect_async(url).await.unwrap();

        let backlog_msg = tokio::time::timeout(std::time::Duration::from_secs(5), ws_stream.next())
            .await
            .expect("backlog message did not arrive in time")
            .expect("stream ended before sending backlog")
            .unwrap();
        let backlog_block: BlockDto =
            serde_json::from_str(&backlog_msg.into_text().unwrap()).unwrap();
        assert_eq!(
            backlog_block.sequence, 1,
            "resume_after=0 must replay sequence 1, not re-send the already-seen sequence 0"
        );

        // Now ingest a third block live and confirm the already-connected client receives it
        // without reconnecting.
        let bytes = [
            synthetic_radial("KTLX", 3, 2.5, 0, VOLUME_START, Utc::now()),
            synthetic_radial("KTLX", 3, 2.5, 1, ELEVATION_END, Utc::now()),
        ]
        .concat();
        pipeline.lock().unwrap().ingest(&RawProduct {
            site: "KTLX".into(),
            bytes,
            received_at: Utc::now(),
        });

        let live_msg = tokio::time::timeout(std::time::Duration::from_secs(5), ws_stream.next())
            .await
            .expect("live message did not arrive in time")
            .expect("stream ended before sending the live block")
            .unwrap();
        let live_block: BlockDto = serde_json::from_str(&live_msg.into_text().unwrap()).unwrap();
        assert_eq!(live_block.sequence, 2);

        drop(ws_stream);
    }

    /// ROADMAP_NEW B6.10's stream-epoch check: a client that names an epoch other than the
    /// running process's must not be served a `resume_after` backlog at all — its sequence number
    /// was learned from a server instance that (from this test's point of view) no longer exists,
    /// so honoring it could silently serve the wrong block under a reused sequence number. It
    /// should instead be treated exactly like a fresh connection: no backlog, live blocks only.
    #[tokio::test]
    async fn resume_after_is_ignored_when_the_claimed_epoch_does_not_match() {
        let pipeline = pipeline_with_one_block();
        let real_epoch = pipeline.lock().unwrap().epoch();
        let wrong_epoch = real_epoch.wrapping_add(1);

        let app = router(pipeline.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // resume_after=0 would normally mean "nothing to backfill" here (sequence 0 is the only
        // block, so there's nothing *after* it) — ingest a second block first so a real backlog
        // exists for the epoch check to actually suppress.
        let bytes = [
            synthetic_radial("KTLX", 2, 1.5, 0, VOLUME_START, Utc::now()),
            synthetic_radial("KTLX", 2, 1.5, 1, ELEVATION_END, Utc::now()),
        ]
        .concat();
        pipeline.lock().unwrap().ingest(&RawProduct {
            site: "KTLX".into(),
            bytes,
            received_at: Utc::now(),
        });

        let url = format!("ws://{addr}/sites/KTLX/live?resume_after=0&epoch={wrong_epoch}");
        let (mut ws_stream, _) = tokio_tungstenite::connect_async(url).await.unwrap();

        // Ingest a third, genuinely live block — if the wrong-epoch backlog were (incorrectly)
        // sent, sequence 1 would arrive first; it must not.
        let bytes = [
            synthetic_radial("KTLX", 3, 2.5, 0, VOLUME_START, Utc::now()),
            synthetic_radial("KTLX", 3, 2.5, 1, ELEVATION_END, Utc::now()),
        ]
        .concat();
        pipeline.lock().unwrap().ingest(&RawProduct {
            site: "KTLX".into(),
            bytes,
            received_at: Utc::now(),
        });

        let msg = tokio::time::timeout(std::time::Duration::from_secs(5), ws_stream.next())
            .await
            .expect("a message should still arrive — just not a backlog one")
            .expect("stream ended unexpectedly")
            .unwrap();
        let block: BlockDto = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
        assert_eq!(
            block.sequence, 2,
            "a mismatched epoch must suppress the resume_after=0 backlog (sequence 1) entirely, \
             leaving only the genuinely live block"
        );

        drop(ws_stream);
    }
}
