//! Where a radar volume comes from, per target.
//!
//! Native reads through the on-disk cache (any archived volume, kept indefinitely); the browser
//! reads through an offline chase pack in IndexedDB (`webcache.rs`) before touching the network,
//! which is what makes a saved loop play with no signal at all, then falls back to the same
//! module's automatic archive-volume cache for everything else archived. Every timeline fetch
//! goes through here so all three caches are one seam, not several call sites each.

use wxdata::level2::{self, Identifier, Scan};

/// Fetch and decode one volume, using whatever cache this target has.
///
/// `archived` is true for a volume this timeline already names — a published, immutable S3
/// object safe to keep — and false at the live head, where the newest object may still be
/// uploading and must always be re-read. `cache` is the native disk cache directory; native holds
/// this alongside `archived` for the same reason `download_scan` always has (a `None` cache_dir on
/// a filesystem-less test build should still behave, archived or not). The browser build has no
/// `cache_dir` at all, so `archived` is its own, and only, signal to try IndexedDB.
pub async fn fetch(
    id: Identifier,
    archived: bool,
    cache: Option<std::path::PathBuf>,
) -> anyhow::Result<Scan> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = &cache;
        let name = id.name().to_string();
        if let Some(bytes) = crate::webcache::volume(&name).await {
            log::debug!("volume {name} came from an offline pack");
            return level2::scan_from_volume_bytes(&name, bytes).await;
        }
        if archived {
            if let Some(bytes) = crate::webcache::auto_cached_volume(&name).await {
                log::debug!("volume {name} came from the browser's automatic archive cache");
                return level2::scan_from_volume_bytes(&name, bytes).await;
            }
            let bytes = level2::volume_bytes(id).await?;
            crate::webcache::spawn_auto_cache_put(name.clone(), bytes.clone());
            return level2::scan_from_volume_bytes(&name, bytes).await;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = archived;
    level2::download_scan(id, cache).await
}
