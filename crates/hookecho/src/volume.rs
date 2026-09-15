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
            // Decode before caching, not after: `archived` means "this timeline already names
            // it," not "the radar has finished uploading it" — the trailing edge of the loop
            // window can still be mid-write, and a half-written object downloads as bytes that
            // fail to decode. Caching first would pin that broken download to IndexedDB forever,
            // since a cache hit is never re-checked against the network once it exists.
            let scan = level2::scan_from_volume_bytes(&name, bytes.clone()).await?;
            crate::webcache::spawn_auto_cache_put(name, bytes);
            return Ok(scan);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = archived;
    level2::download_scan(id, cache).await
}

/// A source of Level II data for one radar site: a live subscription plus a fallback poll for
/// the newest complete volume (ROADMAP_NEW.md Phase B1).
///
/// Formalizes what this app has always done — the Unidata/AWS chunk feed for live updates,
/// falling back to interval polling of the same bucket's archive/current objects — behind one
/// interface, so a second provider (a different aggregator, a user's own relay) could be added
/// without every caller learning a new shape, and so a future automatic failover (Phase B6) has
/// something to fail over between. Not `dyn`-safe on purpose: nothing yet needs to pick a
/// provider at runtime, and boxing every method just to support a hypothetical second
/// implementation before one exists is exactly the premature abstraction the roadmap's own rules
/// warn against (section 2). Add the boxing when B6 actually needs to choose between two.
///
/// `async fn` in a `pub` trait normally warns because an external implementor could return a
/// non-`Send` future — moot here since this crate is the only caller and the only implementor,
/// and every current call site already awaits it inside a `Send` spawned task.
#[allow(async_fn_in_trait)]
pub trait Level2LiveProvider {
    /// Stable label for source-health / diagnostics display.
    #[allow(dead_code)] // read by a future B6 failover/health surface, not yet by anything
    fn label(&self) -> &'static str;

    /// Stream live updates for `site`, starting from `base` — see [`wxdata::live::stream`] for
    /// the exact contract `active`/`on_update`/`on_progress` follow. Boxed rather than generic so
    /// the method itself stays a plain, nameable type — this trait has one implementor today, but
    /// its shape should not change when a second one arrives.
    async fn subscribe(
        &self,
        site: String,
        base: std::sync::Arc<Scan>,
        active: Box<dyn Fn() -> bool + Send + Sync>,
        on_update: Box<dyn FnMut(wxdata::live::Update) + Send>,
        on_progress: Box<dyn FnMut(wxdata::live::ScanProgress) + Send>,
    ) -> anyhow::Result<()>;

    /// The provider's best current answer for `site`: unchanged from `current_name`, a new
    /// volume, or an error when nothing usable could be found.
    async fn latest_complete_volume(
        &self,
        site: &str,
        current_name: Option<&str>,
    ) -> anyhow::Result<LatestVolume>;
}

/// Outcome of [`Level2LiveProvider::latest_complete_volume`].
pub enum LatestVolume {
    /// Still `current_name` — nothing to redraw.
    UpToDate,
    New {
        name: String,
        time: chrono::DateTime<chrono::Utc>,
        scan: Scan,
    },
}

/// The provider this app has always used: Unidata's Level II chunk feed on AWS S3 for live
/// updates, falling back to the same organization's archive/current-object bucket.
pub struct UnidataLevel2Provider;

impl Level2LiveProvider for UnidataLevel2Provider {
    fn label(&self) -> &'static str {
        "Unidata Level II (AWS S3)"
    }

    async fn subscribe(
        &self,
        site: String,
        base: std::sync::Arc<Scan>,
        active: Box<dyn Fn() -> bool + Send + Sync>,
        on_update: Box<dyn FnMut(wxdata::live::Update) + Send>,
        on_progress: Box<dyn FnMut(wxdata::live::ScanProgress) + Send>,
    ) -> anyhow::Result<()> {
        wxdata::live::stream(site, base, active, on_update, on_progress).await
    }

    async fn latest_complete_volume(
        &self,
        site: &str,
        current_name: Option<&str>,
    ) -> anyhow::Result<LatestVolume> {
        // Ask for two: the newest volume is usually still uploading, and one caught before its
        // metadata record lands can't be decoded at all. Falling back one volume shows
        // ~5-minute-old data instead of nothing.
        let mut ids = level2::latest_identifiers(site, 2).await?;
        let first = ids.remove(0);
        let prev = ids.pop();
        let name = first.name().to_string();
        if current_name == Some(name.as_str()) {
            return Ok(LatestVolume::UpToDate);
        }
        let time = first.date_time().unwrap_or_else(chrono::Utc::now);
        // No cache at the live head: the newest object can still be uploading, and a
        // half-written volume is not something to keep.
        match fetch(first, false, None).await {
            Ok(scan) => Ok(LatestVolume::New { name, time, scan }),
            Err(e) => match prev {
                // Only worth retrying when there IS an older volume and we're not already
                // showing it.
                Some(p) if current_name != Some(p.name()) => {
                    let pname = p.name().to_string();
                    let ptime = p.date_time().unwrap_or_else(chrono::Utc::now);
                    log::debug!("newest volume unusable ({e}); falling back to {pname}");
                    fetch(p, false, None)
                        .await
                        .map(|scan| LatestVolume::New {
                            name: pname,
                            time: ptime,
                            scan,
                        })
                        .map_err(|_| e)
                }
                _ => Err(e),
            },
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn the_provider_names_itself() {
        assert_eq!(UnidataLevel2Provider.label(), "Unidata Level II (AWS S3)");
    }

    /// A real end-to-end exercise of the fallback logic against the live network: the first call
    /// must find a new volume from nothing, and an immediate second call naming what the first
    /// one just returned must come back `UpToDate` rather than re-downloading — the exact
    /// dedupe `spawn_fetch` relied on before this moved here. Can flake if the site happens to
    /// publish a new volume in between the two calls; that is a real event, not a false positive.
    #[tokio::test]
    #[ignore = "network"]
    async fn latest_complete_volume_finds_a_new_one_then_reports_up_to_date() {
        let provider = UnidataLevel2Provider;
        let name = match provider.latest_complete_volume("KTLX", None).await.unwrap() {
            LatestVolume::New { name, scan, .. } => {
                assert!(
                    scan.site().is_some(),
                    "a real volume decodes with site metadata"
                );
                name
            }
            LatestVolume::UpToDate => panic!("nothing to be up to date with on the first call"),
        };
        let again = provider
            .latest_complete_volume("KTLX", Some(&name))
            .await
            .unwrap();
        assert!(matches!(again, LatestVolume::UpToDate));
    }
}
