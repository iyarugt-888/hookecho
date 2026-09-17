//! ROADMAP_NEW B6.10: the actual deployable `radar-ingest` service. Everything else in this crate
//! (the rechunker, the block store, the HTTP/WebSocket API) was a tested library with no way to
//! actually run it — this binary wires [`radar_ingest::store::IngestStore`]'s admission checks in
//! front of [`radar_ingest::pipeline::Pipeline`], and serves [`radar_ingest::server::router`] on an
//! environment-configured address, so a deployment can actually stand this up as a container.
//!
//! Configuration is entirely environment-driven (ROADMAP_NEW B6.2's "treat LDM access as
//! deployment configuration, not an entitlement," applied to every other knob too — nothing here
//! assumes a default upstream, a default public bind address, or a default retention budget for a
//! production deployment):
//!
//! - `RADAR_INGEST_LISTEN_ADDR` — HTTP/WebSocket bind address (default `0.0.0.0:8080`).
//! - `RADAR_INGEST_SOURCE_ID` — provenance label stamped on every emitted block (default
//!   `radar-ingest`; set this per-deployment if you run more than one instance a client might see).
//! - `RADAR_INGEST_ALLOWED_SITES` — comma-separated site allowlist for *admission* (distinct from
//!   `RADAR_INGEST_LDM_*`'s own allowlist, which shapes what is *requested* upstream — see
//!   [`radar_ingest::ldm`]'s doc comment). Unset accepts every site products arrive for.
//! - `RADAR_INGEST_MAX_PRODUCT_BYTES`, `RADAR_INGEST_SITE_RAW_MAX_ITEMS`,
//!   `RADAR_INGEST_SITE_RAW_MAX_BYTES` — [`radar_ingest::store::IngestLimits`] overrides.
//! - `RADAR_INGEST_BLOCK_RETENTION_ITEMS`, `RADAR_INGEST_BLOCK_RETENTION_BYTES` —
//!   [`radar_ingest::block_store::BlockStoreLimits`] overrides: how much recent history a
//!   reconnecting client can resume from.
//! - `RADAR_INGEST_MAX_RADIALS_PER_BLOCK`, `RADAR_INGEST_MAX_BLOCK_AGE_SECS` —
//!   [`radar_ingest::rechunk::RechunkConfig`] overrides.
//! - `RADAR_INGEST_REPLAY_FILE` — path to a [`radar_ingest::fixture`] JSON file, replayed through
//!   [`radar_ingest::input::ReplayInputAdapter`]. **This is the only input source that exists
//!   today** — there is no live LDM adapter yet (see [`radar_ingest::ldm`]'s own doc comment) — so
//!   without it, the service serves the HTTP/WS API with nothing to ingest, which is still useful
//!   for exercising a real deployment's networking/TLS/reverse-proxy setup and for testing a
//!   client (`HookEchoRelayLevel2Provider`) against a real running relay rather than only an
//!   in-process test server.
//! - `RADAR_INGEST_REPLAY_INTERVAL_MS` — optional pacing between replayed products (default: as
//!   fast as the channel accepts them).
//! - `RADAR_INGEST_LDM_*` — read by [`radar_ingest::ldm::LdmSourceConfig`] but not yet acted on by
//!   this binary; logged as a warning if set, so a deployment configuring it doesn't silently get
//!   nothing.
//! - `RUST_LOG` — standard `env_logger` filter (default `info`).

use radar_ingest::block_store::BlockStoreLimits;
use radar_ingest::input::{InputAdapter, RawProduct, ReplayInputAdapter};
use radar_ingest::pipeline::Pipeline;
use radar_ingest::rechunk::RechunkConfig;
use radar_ingest::store::{IngestLimits, IngestStore};
use std::env;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let listen_addr: SocketAddr = env::var("RADAR_INGEST_LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()
        .map_err(|e| anyhow::anyhow!("RADAR_INGEST_LISTEN_ADDR: {e}"))?;
    let source_id =
        env::var("RADAR_INGEST_SOURCE_ID").unwrap_or_else(|_| "radar-ingest".to_string());

    if env::var("RADAR_INGEST_LDM_HOST").is_ok() {
        log::warn!(
            "RADAR_INGEST_LDM_* is configured but no live LDM adapter exists yet (see \
             radar_ingest::ldm's doc comment for why) — it will be read, not connected to. Use \
             RADAR_INGEST_REPLAY_FILE for now."
        );
    }

    let default_rechunk = RechunkConfig::default();
    let rechunk_config = RechunkConfig {
        max_radials_per_block: env_or(
            "RADAR_INGEST_MAX_RADIALS_PER_BLOCK",
            default_rechunk.max_radials_per_block,
        ),
        max_block_age: chrono::Duration::seconds(env_or(
            "RADAR_INGEST_MAX_BLOCK_AGE_SECS",
            default_rechunk.max_block_age.num_seconds(),
        )),
    };

    let default_block_limits = BlockStoreLimits::default();
    let block_limits = BlockStoreLimits {
        per_site_max_items: env_or(
            "RADAR_INGEST_BLOCK_RETENTION_ITEMS",
            default_block_limits.per_site_max_items,
        ),
        per_site_max_bytes: env_or(
            "RADAR_INGEST_BLOCK_RETENTION_BYTES",
            default_block_limits.per_site_max_bytes,
        ),
    };

    let default_ingest_limits = IngestLimits::default();
    let ingest_limits = IngestLimits {
        per_site_max_items: env_or(
            "RADAR_INGEST_SITE_RAW_MAX_ITEMS",
            default_ingest_limits.per_site_max_items,
        ),
        per_site_max_bytes: env_or(
            "RADAR_INGEST_SITE_RAW_MAX_BYTES",
            default_ingest_limits.per_site_max_bytes,
        ),
        max_product_bytes: env_or(
            "RADAR_INGEST_MAX_PRODUCT_BYTES",
            default_ingest_limits.max_product_bytes,
        ),
    };
    let mut ingest_store = IngestStore::new(ingest_limits);
    if let Ok(raw) = env::var("RADAR_INGEST_ALLOWED_SITES") {
        let sites: Vec<String> = raw
            .split(',')
            .map(|s| s.trim().to_ascii_uppercase())
            .filter(|s| !s.is_empty())
            .collect();
        log::info!("admitting only allowlisted sites: {sites:?}");
        ingest_store = ingest_store.with_allowlist(sites);
    }
    let ingest_store = Arc::new(Mutex::new(ingest_store));

    let pipeline = Arc::new(Mutex::new(Pipeline::new(
        rechunk_config,
        block_limits,
        source_id,
    )));

    // Every accepted product goes through `IngestStore` first (site allowlist, size/framing
    // sanity) before the rechunker ever sees it — ROADMAP_NEW B6.2's admission rules, applied to
    // whatever `InputAdapter` is configured below rather than duplicated per-adapter.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<RawProduct>(64);
    {
        let ingest_store = ingest_store.clone();
        let pipeline = pipeline.clone();
        tokio::spawn(async move {
            while let Some(product) = rx.recv().await {
                let site = product.site.clone();
                let product_for_pipeline = product.clone();
                let outcome = ingest_store
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .ingest(product);
                match outcome {
                    Ok(()) => pipeline
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .ingest(&product_for_pipeline),
                    Err(reason) => log::warn!("rejected product for {site}: {reason}"),
                }
            }
        });
    }

    match env::var("RADAR_INGEST_REPLAY_FILE") {
        Ok(path) => {
            let products = radar_ingest::fixture::load(Path::new(&path))?;
            log::info!(
                "replaying {} fixture product(s) from {path}",
                products.len()
            );
            let mut adapter = ReplayInputAdapter::new(products);
            if let Ok(ms) = env::var("RADAR_INGEST_REPLAY_INTERVAL_MS") {
                let ms: u64 = ms.parse().map_err(|_| {
                    anyhow::anyhow!("RADAR_INGEST_REPLAY_INTERVAL_MS must be a number")
                })?;
                adapter = adapter.with_delay(Duration::from_millis(ms));
            }
            tokio::spawn(async move {
                if let Err(e) = Box::new(adapter).run(tx).await {
                    log::warn!("replay adapter ended: {e}");
                }
            });
        }
        Err(_) => {
            log::warn!(
                "no input source configured — set RADAR_INGEST_REPLAY_FILE to replay a fixture \
                 for local testing. Serving the HTTP/WS API with nothing to ingest until then."
            );
        }
    }

    // Flushes a partially-filled block once it ages past `max_block_age`, independent of new
    // products arriving — see `Rechunker::tick`'s own doc comment for why this can't just be
    // driven by ingest alone.
    {
        let pipeline = pipeline.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                pipeline
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .tick(chrono::Utc::now());
            }
        });
    }

    log::info!("radar-ingest listening on {listen_addr}");
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    axum::serve(listener, radar_ingest::server::router(pipeline)).await?;
    Ok(())
}
