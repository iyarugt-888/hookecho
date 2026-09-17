//! ROADMAP_NEW B6.11 step 10 / B6.8: [`NoaaTgftpLevel2Provider`], the last-resort
//! completed-volume-only fallback when neither progressive path (Unidata or the HookEcho relay)
//! is usable.
//!
//! NOAA's public TGFTP mirror (`https://tgftp.nws.noaa.gov/data/radar/nexrad_level2/{SITE}/`)
//! serves each site's most recent several hours of completed Level II volumes as plain Archive II
//! `.bz2` files (per-record bzip2, the same format `wxdata::level2::decode_volume` already
//! decodes for the AWS archive path — confirmed against a real downloaded volume's header bytes
//! while building this: `AR2V0006....` immediately followed by a `BZh9`-prefixed first record,
//! not a whole-file compression wrapper despite the `.bz2` filename suffix), alongside a small
//! `dir.list` index (`<size> <filename>` per line) cheap enough to poll on its own without ever
//! re-fetching the full HTML directory listing (ROADMAP_NEW B6.8: "poll efficiently using the
//! site's directory/index metadata rather than redownloading listings unnecessarily").
//!
//! This provider never claims progressive radials — [`wxdata::live_block::ProviderCapabilities::
//! tgftp`] already says so (`progressive_radials: false`), and this implementation's `subscribe`
//! never calls `on_progress` at all, matching B6.8's "do not show live chunk progress when no
//! progressive feed exists." Every successfully decoded volume is cached in memory
//! ([`Self::last_known`]) so a total network outage degrades [`Level2LiveProvider::
//! latest_complete_volume`] to serving the last good volume (an honest stale display) rather than
//! erroring out to a blank pane, per B6.8's explicit ask.
//!
//! Cross-platform (native and wasm32) — unlike [`crate::relay_provider`]/[`crate::
//! provider_health`], this needs nothing beyond `reqwest` (already used cross-platform elsewhere
//! in this codebase for the Unidata path) and [`wxdata::task::sleep_while`].

use crate::volume::{LatestVolume, Level2LiveProvider};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use std::sync::{Arc, Mutex};
use wxdata::level2::Scan;
use wxdata::live::{ScanProgress, Update};
use wxdata::live_block::ProviderCapabilities;

const DEFAULT_BASE_URL: &str = "https://tgftp.nws.noaa.gov/data/radar/nexrad_level2";

#[derive(Clone)]
struct CachedVolume {
    name: String,
    time: DateTime<Utc>,
    scan: Arc<Scan>,
}

/// NOAA TGFTP's completed-volume-only Level II mirror — the roadmap's own "final fallback mode
/// for availability when neither progressive source is usable."
pub struct NoaaTgftpLevel2Provider {
    base_url: String,
    poll_interval: std::time::Duration,
    last_known: Mutex<Option<CachedVolume>>,
}

impl Default for NoaaTgftpLevel2Provider {
    fn default() -> Self {
        Self::new()
    }
}

impl NoaaTgftpLevel2Provider {
    pub fn new() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            poll_interval: std::time::Duration::from_secs(60),
            last_known: Mutex::new(None),
        }
    }

    #[cfg(test)]
    fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            ..Self::new()
        }
    }

    async fn fetch_dir_list(&self, site: &str) -> anyhow::Result<String> {
        let url = format!("{}/{}/dir.list", self.base_url, site.to_ascii_uppercase());
        let response = reqwest::get(&url).await?.error_for_status()?;
        Ok(response.text().await?)
    }

    async fn fetch_volume_bytes(&self, site: &str, filename: &str) -> anyhow::Result<Vec<u8>> {
        let url = format!("{}/{}/{filename}", self.base_url, site.to_ascii_uppercase());
        let response = reqwest::get(&url).await?.error_for_status()?;
        Ok(response.bytes().await?.to_vec())
    }

    /// Fetch and decode the newest volume for `site` — network + decode, no caching or
    /// `current_name` comparison (that's [`Level2LiveProvider::latest_complete_volume`]'s job,
    /// which wraps this with the degraded-cache fallback).
    async fn fetch_latest(&self, site: &str) -> anyhow::Result<(String, DateTime<Utc>, Scan)> {
        let listing = self.fetch_dir_list(site).await?;
        let entries = parse_dir_list(&listing);
        let (name, time) = newest_entry(&entries)
            .ok_or_else(|| anyhow::anyhow!("no volume files found for {site} in dir.list"))?;
        let bytes = self.fetch_volume_bytes(site, &name).await?;
        let scan = wxdata::level2::decode_volume(bytes)?;
        Ok((name, time, scan))
    }
}

/// Parses one `dir.list` response body into `(filename, size_bytes)` pairs. Skips any line that
/// doesn't parse as `<size> <filename>` rather than failing the whole listing — a stray blank
/// line or a future NOAA-side format tweak on one line should not take down every other real
/// entry.
fn parse_dir_list(body: &str) -> Vec<(String, u64)> {
    body.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, ' ');
            let size: u64 = parts.next()?.trim().parse().ok()?;
            let filename = parts.next()?.trim();
            if filename.is_empty() {
                return None;
            }
            Some((filename.to_string(), size))
        })
        .collect()
}

/// Parses a TGFTP NEXRAD Level II filename (`"{SITE}_{YYYYMMDD}_{HHMMSS}.bz2"`) into its
/// collection time. Returns `None` for anything that doesn't match this exact shape — `dir.list`
/// is not guaranteed to contain only volume files forever (NOAA has changed sibling index files
/// like `dir.list` itself appearing in the same listing before).
fn parse_filename_time(filename: &str) -> Option<DateTime<Utc>> {
    let stem = filename.strip_suffix(".bz2")?;
    let mut parts = stem.splitn(3, '_');
    let _site = parts.next()?;
    let date = parts.next()?;
    let time = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let date = NaiveDate::parse_from_str(date, "%Y%m%d").ok()?;
    let time = NaiveTime::parse_from_str(time, "%H%M%S").ok()?;
    Some(DateTime::from_naive_utc_and_offset(
        NaiveDateTime::new(date, time),
        Utc,
    ))
}

/// The newest (by embedded timestamp, not list order — `dir.list`'s own ordering is not a
/// documented contract) entry in `entries`, or `None` if nothing parses as a volume filename.
fn newest_entry(entries: &[(String, u64)]) -> Option<(String, DateTime<Utc>)> {
    entries
        .iter()
        .filter_map(|(name, _)| parse_filename_time(name).map(|t| (name.clone(), t)))
        .max_by_key(|(_, t)| *t)
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Level2LiveProvider for NoaaTgftpLevel2Provider {
    fn label(&self) -> &'static str {
        "NOAA TGFTP (degraded)"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::tgftp()
    }

    /// Polls [`Level2LiveProvider::latest_complete_volume`] on an interval, merging each new
    /// completed volume into the running scan with the same [`wxdata::live::merge_scan`] every
    /// other live path already uses. Never calls `on_progress` — there is no progressive signal
    /// to report (ROADMAP_NEW B6.8: "do not show live chunk progress when no progressive feed
    /// exists").
    async fn subscribe(
        &self,
        site: String,
        base: Arc<Scan>,
        active: Box<dyn Fn() -> bool + Send + Sync>,
        mut on_update: Box<dyn FnMut(Update) + Send>,
        _on_progress: Box<dyn FnMut(ScanProgress) + Send>,
    ) -> anyhow::Result<()> {
        let mut merged = base;
        let mut current_name: Option<String> = None;

        while active() {
            match self
                .latest_complete_volume(&site, current_name.as_deref())
                .await
            {
                Ok(LatestVolume::New { name, time, scan }) => {
                    let (new_scan, changed) = wxdata::live::merge_scan(&merged, scan);
                    current_name = Some(name.clone());
                    if !changed.is_empty() {
                        merged = Arc::new(new_scan);
                        on_update(Update {
                            name,
                            time,
                            scan: merged.clone(),
                            changed,
                            retries: 0,
                            decode_time: std::time::Duration::ZERO,
                        });
                    }
                }
                Ok(LatestVolume::UpToDate) => {}
                Err(e) => {
                    log::warn!("TGFTP poll failed for {site}: {e}");
                }
            }
            if !wxdata::task::sleep_while(self.poll_interval, &active).await {
                break;
            }
        }
        Ok(())
    }

    async fn latest_complete_volume(
        &self,
        site: &str,
        current_name: Option<&str>,
    ) -> anyhow::Result<LatestVolume> {
        match self.fetch_latest(site).await {
            Ok((name, time, scan)) => {
                let scan = Arc::new(scan);
                *self.last_known.lock().unwrap() = Some(CachedVolume {
                    name: name.clone(),
                    time,
                    scan: scan.clone(),
                });
                if current_name == Some(name.as_str()) {
                    Ok(LatestVolume::UpToDate)
                } else {
                    Ok(LatestVolume::New {
                        name,
                        time,
                        scan: (*scan).clone(),
                    })
                }
            }
            // A total network outage (or a decode failure on a half-written file) degrades to
            // serving the last known-good volume rather than erroring out to a blank pane —
            // ROADMAP_NEW B6.8's explicit ask. Genuinely nothing cached yet (e.g. the very first
            // call, before any fetch has ever succeeded) is still a real error.
            Err(e) => {
                let cached = self.last_known.lock().unwrap().clone();
                match cached {
                    Some(c) if current_name != Some(c.name.as_str()) => Ok(LatestVolume::New {
                        name: c.name,
                        time: c.time,
                        scan: (*c.scan).clone(),
                    }),
                    Some(_) => Ok(LatestVolume::UpToDate),
                    None => Err(e),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_provider_advertises_completed_volume_only_capabilities() {
        let caps = NoaaTgftpLevel2Provider::new().capabilities();
        assert!(!caps.progressive_radials);
        assert!(caps.completed_volume);
        assert!(!caps.server_push);
    }

    #[test]
    fn the_provider_is_usable_as_a_trait_object() {
        let boxed: Box<dyn Level2LiveProvider + Send + Sync> =
            Box::new(NoaaTgftpLevel2Provider::new());
        assert_eq!(boxed.label(), "NOAA TGFTP (degraded)");
    }

    #[test]
    fn parse_dir_list_reads_size_and_filename_pairs() {
        let body = "3487682 KTLX_20260916_230507.bz2\n6142664 KTLX_20260917_012228.bz2\n";
        let entries = parse_dir_list(body);
        assert_eq!(
            entries,
            vec![
                ("KTLX_20260916_230507.bz2".to_string(), 3487682),
                ("KTLX_20260917_012228.bz2".to_string(), 6142664),
            ]
        );
    }

    #[test]
    fn parse_dir_list_skips_unparseable_lines_without_failing_the_rest() {
        let body = "not a valid line\n3487682 KTLX_20260916_230507.bz2\n\nalso not valid\n";
        let entries = parse_dir_list(body);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "KTLX_20260916_230507.bz2");
    }

    #[test]
    fn parse_filename_time_reads_the_embedded_timestamp() {
        let t = parse_filename_time("KTLX_20260917_074402.bz2").unwrap();
        assert_eq!(t.to_rfc3339(), "2026-09-17T07:44:02+00:00");
    }

    #[test]
    fn parse_filename_time_rejects_anything_else() {
        assert!(parse_filename_time("dir.list").is_none());
        assert!(parse_filename_time("KTLX_20260917_074402.gz").is_none());
        assert!(parse_filename_time("garbage").is_none());
    }

    #[test]
    fn newest_entry_picks_by_embedded_timestamp_not_list_order() {
        // Deliberately out of chronological order, unlike a real dir.list — the point of this
        // test is that list order is not trusted.
        let entries = vec![
            ("KTLX_20260917_012228.bz2".to_string(), 1),
            ("KTLX_20260916_230507.bz2".to_string(), 1),
            ("KTLX_20260917_074402.bz2".to_string(), 1),
        ];
        let (name, _) = newest_entry(&entries).unwrap();
        assert_eq!(name, "KTLX_20260917_074402.bz2");
    }

    #[test]
    fn newest_entry_ignores_non_volume_files_like_dir_list_itself() {
        let entries = vec![
            ("dir.list".to_string(), 2820),
            ("KTLX_20260917_012228.bz2".to_string(), 1),
        ];
        let (name, _) = newest_entry(&entries).unwrap();
        assert_eq!(name, "KTLX_20260917_012228.bz2");
    }

    #[test]
    fn newest_entry_is_none_for_an_empty_or_all_unrecognized_listing() {
        assert!(newest_entry(&[]).is_none());
        assert!(newest_entry(&[("dir.list".to_string(), 1)]).is_none());
    }

    /// Real end-to-end fetch against the live TGFTP mirror — network-dependent, so `#[ignore]`
    /// like every other real-network test in this codebase (see `UnidataLevel2Provider`'s own).
    #[tokio::test]
    #[ignore = "network"]
    async fn latest_complete_volume_fetches_and_decodes_a_real_tgftp_volume() {
        let provider = NoaaTgftpLevel2Provider::new();
        match provider.latest_complete_volume("KTLX", None).await.unwrap() {
            LatestVolume::New { scan, .. } => {
                assert!(!scan.sweeps().is_empty());
            }
            LatestVolume::UpToDate => panic!("nothing to be up to date with on the first call"),
        }
    }

    async fn spawn_test_server(app: axum::Router) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        addr
    }

    /// Deterministic, network-free proof that `fetch_dir_list` builds the right URL (including
    /// upper-casing a lower-case site) and reads the response body back correctly — the actual
    /// HTTP wiring, not just the pure parsing functions the tests above already cover thoroughly.
    #[tokio::test]
    async fn fetch_dir_list_hits_the_uppercased_site_path_and_returns_the_body() {
        let app = axum::Router::new().route(
            "/KTLX/dir.list",
            axum::routing::get(|| async { "123 KTLX_20260917_000000.bz2\n" }),
        );
        let addr = spawn_test_server(app).await;

        let provider = NoaaTgftpLevel2Provider::with_base_url(format!("http://{addr}"));
        let body = provider.fetch_dir_list("ktlx").await.unwrap();
        assert!(body.contains("KTLX_20260917_000000.bz2"));
    }

    /// With no cached volume yet and an undecodable (here, simply too-short) file, the real error
    /// must surface rather than being swallowed — the degraded-cache fallback only ever serves
    /// something once at least one real fetch has succeeded.
    #[tokio::test]
    async fn latest_complete_volume_surfaces_a_real_error_when_nothing_is_cached_yet() {
        let app = axum::Router::new()
            .route(
                "/KTLX/dir.list",
                axum::routing::get(|| async { "2 KTLX_20260917_000000.bz2\n" }),
            )
            .route(
                "/KTLX/KTLX_20260917_000000.bz2",
                axum::routing::get(|| async { "xx" }), // far too short to be a real volume
            );
        let addr = spawn_test_server(app).await;

        let provider = NoaaTgftpLevel2Provider::with_base_url(format!("http://{addr}"));
        assert!(provider.latest_complete_volume("KTLX", None).await.is_err());
    }
}
