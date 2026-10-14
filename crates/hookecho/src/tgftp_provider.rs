//! ROADMAP_NEW B6.11 step 10 / B6.8: [`NoaaTgftpLevel2Provider`], the last-resort
//! completed-volume-only fallback when neither progressive path (Unidata or the HookEcho relay)
//! is usable.
//!
//! NOAA's public TGFTP mirror (`https://tgftp.nws.noaa.gov/data/radar/nexrad_level2/{SITE}/`)
//! serves each site's most recent several hours of Level II volumes as plain Archive II
//! `.bz2` files (per-record bzip2, the same format `wxdata::level2::decode_volume` already
//! decodes for the AWS archive path — confirmed against a real downloaded volume's header bytes
//! while building this: `AR2V0006....` immediately followed by a `BZh9`-prefixed first record,
//! not a whole-file compression wrapper despite the `.bz2` filename suffix), alongside a small
//! `dir.list` index (`<size> <filename>` per line) cheap enough to poll on its own without ever
//! re-fetching the full HTML directory listing (ROADMAP_NEW B6.8: "poll efficiently using the
//! site's directory/index metadata rather than redownloading listings unnecessarily").
//! The newest object can still be publishing. Admission requires the radar's end-of-scan marker;
//! at most three candidates are tried within one total deadline. That marker does not assert
//! complete angular coverage or the absence of missing gates inside a completed scan.
//!
//! This provider never claims progressive radials — [`wxdata::live_block::ProviderCapabilities::
//! tgftp`] already says so (`progressive_radials: false`), and this implementation's `subscribe`
//! never calls `on_progress` at all, matching B6.8's "do not show live chunk progress when no
//! progressive feed exists." Every admitted completed volume is cached in memory
//! ([`Self::last_known`]) so a total network outage degrades [`Level2LiveProvider::
//! latest_complete_volume`] to serving the last good volume (an honest stale display) rather than
//! erroring out to a blank pane, per B6.8's explicit ask.
//!
//! Listing and volume reads have whole-response deadlines. A retired subscription drops its
//! in-flight poll within a 250 ms timer slice instead of waiting for the server. Native decoding
//! uses the existing blocking-work boundary; a decode already running there can finish after
//! cancellation, but cannot deliver an update to the retired subscription.
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
const LIST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const VOLUME_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(100);
const CANCEL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);
const MAX_CANDIDATES: usize = 3;

#[derive(Clone)]
struct CachedVolume {
    site: String,
    name: String,
    listed_bytes: u64,
    time: DateTime<Utc>,
    scan: Arc<Scan>,
}

/// NOAA TGFTP's completed-volume-only Level II mirror — the roadmap's own "final fallback mode
/// for availability when neither progressive source is usable."
pub struct NoaaTgftpLevel2Provider {
    base_url: String,
    poll_interval: std::time::Duration,
    list_timeout: std::time::Duration,
    volume_timeout: std::time::Duration,
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
            list_timeout: LIST_TIMEOUT,
            volume_timeout: VOLUME_TIMEOUT,
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
        // Bound headers AND body on native and WASM; a server can accept the connection and
        // then stop sending, including after headers arrive.
        wxdata::task::timeout(self.list_timeout, async {
            let response = reqwest::get(&url).await?.error_for_status()?;
            Ok(response.text().await?)
        })
        .await?
    }

    async fn fetch_volume_bytes(&self, site: &str, filename: &str) -> anyhow::Result<Vec<u8>> {
        let url = format!("{}/{}/{filename}", self.base_url, site.to_ascii_uppercase());
        wxdata::task::timeout(self.volume_timeout, async {
            let response = reqwest::get(&url).await?.error_for_status()?;
            Ok(response.bytes().await?.to_vec())
        })
        .await?
    }

    /// Poll the small index first. A known completed object needs neither another multi-MB
    /// download nor another decode. Retain one bounded cache entry, scoped to its actual site.
    async fn fetch_latest(&self, site: &str) -> anyhow::Result<CachedVolume> {
        let listing = self.fetch_dir_list(site).await?;
        let mut entries = parse_dir_list(&listing);
        // TGFTP publishes the newest file while it is still being written. An Archive-II
        // prefix may decode successfully but is NOT a completed-volume fallback. Try a
        // bounded number of previous objects, within one total download/decode deadline.
        wxdata::task::timeout(self.volume_timeout, async {
            let mut error = anyhow::anyhow!("no completed volume found for {site} in dir.list");
            for _ in 0..MAX_CANDIDATES {
                let Some((name, time)) = newest_entry_for_site(&entries, site) else {
                    break;
                };
                let listed_bytes = entries
                    .iter()
                    .find(|(filename, _)| filename == &name)
                    .map(|(_, size)| *size)
                    .unwrap_or_default();
                entries.retain(|(filename, _)| filename != &name);
                if let Some(cached) = self.cached_for(site) {
                    if time < cached.time
                        || (name == cached.name && listed_bytes == cached.listed_bytes)
                    {
                        return Ok(cached);
                    }
                }
                match self
                    .fetch_candidate(site, name.clone(), time, listed_bytes)
                    .await
                {
                    Ok(candidate) => return Ok(candidate),
                    Err(failed) => {
                        log::warn!("TGFTP skipped {name}: {failed}");
                        error = failed;
                    }
                }
            }
            Err(error)
        })
        .await?
    }

    async fn fetch_candidate(
        &self,
        site: &str,
        name: String,
        time: DateTime<Utc>,
        listed_bytes: u64,
    ) -> anyhow::Result<CachedVolume> {
        let bytes = self.fetch_volume_bytes(site, &name).await?;
        let scan = wxdata::task::blocking(move || wxdata::level2::decode_volume(bytes)).await??;
        anyhow::ensure!(
            wxdata::level2::scan_complete(&scan),
            "TGFTP volume has no end-of-scan marker (still publishing or truncated)"
        );
        if let Some(decoded_site) = scan.site() {
            anyhow::ensure!(
                decoded_site.identifier_string().eq_ignore_ascii_case(site),
                "TGFTP volume site does not match requested {site}"
            );
        }
        let candidate = CachedVolume {
            site: site.to_ascii_uppercase(),
            name,
            listed_bytes,
            time,
            scan: Arc::new(scan),
        };
        let mut cache = self.last_known.lock().unwrap();
        // Another poll may have finished while this one downloaded/decoded. A late older
        // response cannot lower the cache's source clock, even when it was newer at admission.
        if let Some(newer) = cache
            .as_ref()
            .filter(|c| c.site == candidate.site && c.time > time)
        {
            return Ok(newer.clone());
        }
        *cache = Some(candidate.clone());
        Ok(candidate)
    }

    fn cached_for(&self, site: &str) -> Option<CachedVolume> {
        self.last_known
            .lock()
            .unwrap()
            .as_ref()
            .filter(|cached| cached.site.eq_ignore_ascii_case(site))
            .cloned()
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
    // A directory entry is a basename, never a path/query, foreign origin or loose date.
    if stem.len() != 20 || stem.as_bytes()[4] != b'_' || stem.as_bytes()[13] != b'_' {
        return None;
    }
    if !stem.as_bytes()[..4].iter().all(u8::is_ascii_alphanumeric)
        || !stem.as_bytes()[5..13].iter().all(u8::is_ascii_digit)
        || !stem.as_bytes()[14..].iter().all(u8::is_ascii_digit)
    {
        return None;
    }
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

fn newest_entry_for_site(entries: &[(String, u64)], site: &str) -> Option<(String, DateTime<Utc>)> {
    let matching: Vec<_> = entries
        .iter()
        .filter(|(name, _)| {
            name.get(..4)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(site))
        })
        .cloned()
        .collect();
    newest_entry(&matching)
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Level2LiveProvider for NoaaTgftpLevel2Provider {
    async fn inspect_topology(&self) -> wxdata::provider_topology::ProviderTopology {
        wxdata::provider_topology::ProviderTopology::adapter(
            wxdata::provider_topology::NOAA_TGFTP_DOMAIN,
        )
    }
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
            // Dropping the poll cancels an in-flight HTTP read. Switching providers, sites or
            // going to the background must not retain a hung fallback until its deadline.
            let result = {
                use futures_util::future::{select, Either};
                let poll =
                    std::pin::pin!(self.latest_complete_volume(&site, current_name.as_deref()));
                let cancelled = std::pin::pin!(async {
                    while active() {
                        wxdata::task::sleep(CANCEL_INTERVAL).await;
                    }
                });
                match select(poll, cancelled).await {
                    Either::Left((result, _)) => result,
                    Either::Right(_) => return Ok(()),
                }
            };
            // Cancellation can coincide with the response becoming ready. Never deliver it
            // into a retired subscription, even if the poll won the race above.
            if !active() {
                return Ok(());
            }
            match result {
                Ok(LatestVolume::New { name, time, scan }) => {
                    let (new_scan, changed) = wxdata::live::merge_scan(&merged, scan);
                    current_name = Some(name.clone());
                    if !changed.is_empty() {
                        merged = Arc::new(new_scan);
                        on_update(Update {
                            name,
                            time,
                            received_at: None,
                            radial_coverage: None,
                            scan: merged.clone(),
                            changed,
                            retries: 0,
                            decode_time: None,
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
        anyhow::ensure!(
            site.len() == 4 && site.bytes().all(|b| b.is_ascii_alphanumeric()),
            "invalid TGFTP radar site"
        );
        match self.fetch_latest(site).await {
            Ok(cached) => {
                if current_name == Some(cached.name.as_str()) {
                    Ok(LatestVolume::UpToDate)
                } else {
                    Ok(LatestVolume::New {
                        name: cached.name,
                        time: cached.time,
                        scan: (*cached.scan).clone(),
                    })
                }
            }
            // A total network outage (or a decode failure on a half-written file) degrades to
            // serving the last known-good volume rather than erroring out to a blank pane —
            // ROADMAP_NEW B6.8's explicit ask. Genuinely nothing cached yet (e.g. the very first
            // call, before any fetch has ever succeeded) is still a real error.
            Err(e) => {
                let cached = self.cached_for(site);
                if cached.is_some() {
                    log::warn!("TGFTP poll failed for {site}; retaining last known volume: {e}");
                }
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

    // Real complete NOAA TGFTP scan, with source/hash and format in tests/data/tgftp/README.md.
    // The old partial NOAA hail fixture is retained below as a refusal control.
    const RECOVERY_BYTES: &[u8] = include_bytes!("../tests/data/tgftp/KFTG_20261013_002258.bz2");
    const RECOVERY_NAME: &str = "KFTG_20261013_002258.bz2";
    const PARTIAL_BYTES: &[u8] =
        include_bytes!("../../wxdata/tests/data/corpus/denver-hail-2017-first-records.ar2");

    #[tokio::test]
    async fn a_publishing_head_falls_back_to_a_completed_scan_without_promoting_partial_gates() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let count = Arc::new(AtomicUsize::new(0));
        let partial_count = count.clone();
        let app = axum::Router::new()
            .route(
                "/KFTG/dir.list",
                axum::routing::get(|| async {
                    format!(
                        "{} KFTG_20261013_002635.bz2\n{} {RECOVERY_NAME}\n",
                        PARTIAL_BYTES.len(),
                        RECOVERY_BYTES.len()
                    )
                }),
            )
            .route(
                "/KFTG/KFTG_20261013_002635.bz2",
                axum::routing::get(move || {
                    partial_count.fetch_add(1, Ordering::Relaxed);
                    async { PARTIAL_BYTES }
                }),
            )
            .route(
                &format!("/KFTG/{RECOVERY_NAME}"),
                axum::routing::get(|| async { RECOVERY_BYTES }),
            );
        let addr = spawn_test_server(app).await;
        let provider = NoaaTgftpLevel2Provider::with_base_url(format!("http://{addr}"));
        let partial = wxdata::level2::decode_volume(PARTIAL_BYTES.to_vec()).unwrap();
        assert!(
            !partial.sweeps().is_empty(),
            "successful decode alone is not completeness"
        );
        assert!(!wxdata::level2::scan_complete(&partial));
        let LatestVolume::New { name, time, scan } =
            provider.latest_complete_volume("KFTG", None).await.unwrap()
        else {
            panic!("must recover from the previous completed object");
        };
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert_eq!(name, RECOVERY_NAME);
        assert_eq!(time, parse_filename_time(RECOVERY_NAME).unwrap());
        assert!(wxdata::level2::scan_complete(&scan));
        assert_eq!(scan.site().unwrap().identifier_string(), "KFTG");
        assert_eq!(provider.cached_for("KFTG").unwrap().name, RECOVERY_NAME);
    }

    #[tokio::test]
    async fn partial_only_fallback_is_bounded_and_never_cached_as_complete() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let downloads = Arc::new(AtomicUsize::new(0));
        let count = downloads.clone();
        let app = axum::Router::new()
            .route(
                "/KFTG/dir.list",
                axum::routing::get(|| async {
                    (0..8)
                        .map(|n| format!("{} KFTG_20261013_002{n}00.bz2\n", PARTIAL_BYTES.len()))
                        .collect::<String>()
                }),
            )
            .fallback(axum::routing::get(move || {
                count.fetch_add(1, Ordering::Relaxed);
                async { PARTIAL_BYTES }
            }));
        let addr = spawn_test_server(app).await;
        let provider = NoaaTgftpLevel2Provider::with_base_url(format!("http://{addr}"));
        assert!(provider.latest_complete_volume("KFTG", None).await.is_err());
        assert_eq!(downloads.load(Ordering::Relaxed), MAX_CANDIDATES);
        assert!(provider.last_known.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn unchanged_fallback_polls_only_the_index_and_keeps_original_gates() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listings = Arc::new(AtomicUsize::new(0));
        let downloads = Arc::new(AtomicUsize::new(0));
        let listing_count = listings.clone();
        let download_count = downloads.clone();
        let app = axum::Router::new()
            .route(
                "/KFTG/dir.list",
                axum::routing::get(move || {
                    listing_count.fetch_add(1, Ordering::Relaxed);
                    async { format!("{} {RECOVERY_NAME}\n", RECOVERY_BYTES.len()) }
                }),
            )
            .route(
                &format!("/KFTG/{RECOVERY_NAME}"),
                axum::routing::get(move || {
                    download_count.fetch_add(1, Ordering::Relaxed);
                    async { RECOVERY_BYTES }
                }),
            );
        let addr = spawn_test_server(app).await;
        let provider = NoaaTgftpLevel2Provider::with_base_url(format!("http://{addr}"));
        let LatestVolume::New { name, time, scan } =
            provider.latest_complete_volume("kftg", None).await.unwrap()
        else {
            panic!("first observation must deliver its source volume");
        };
        let original = provider.cached_for("KFTG").unwrap();
        assert!(matches!(
            provider
                .latest_complete_volume("KFTG", Some(&name))
                .await
                .unwrap(),
            LatestVolume::UpToDate
        ));
        // A replacement subscriber with no current volume still receives the same cached data.
        let LatestVolume::New {
            scan: again,
            time: original_time,
            ..
        } = provider.latest_complete_volume("KFTG", None).await.unwrap()
        else {
            panic!("new subscriber needs the volume, not UpToDate");
        };
        assert_eq!(original_time, time);
        assert_eq!(again.sweeps(), scan.sweeps());
        assert!(Arc::ptr_eq(
            &original.scan,
            &provider.cached_for("KFTG").unwrap().scan
        ));
        assert_eq!(listings.load(Ordering::Relaxed), 3);
        assert_eq!(downloads.load(Ordering::Relaxed), 1);
        println!("TGFTP unchanged: 3 index polls, 1 volume download/decode; original gates and clocks retained");
    }

    #[tokio::test]
    async fn a_foreign_or_missing_site_never_receives_another_sites_cached_volume() {
        let app = axum::Router::new()
            .route(
                "/KFTG/dir.list",
                axum::routing::get(|| async {
                    format!("{} {RECOVERY_NAME}\n", RECOVERY_BYTES.len())
                }),
            )
            .route(
                &format!("/KFTG/{RECOVERY_NAME}"),
                axum::routing::get(|| async { RECOVERY_BYTES }),
            )
            // Misconfigured mirror: the target site's directory contains another radar.
            .route(
                "/KTLX/dir.list",
                axum::routing::get(|| async {
                    format!("{} {RECOVERY_NAME}\n", RECOVERY_BYTES.len())
                }),
            );
        let addr = spawn_test_server(app).await;
        let provider = NoaaTgftpLevel2Provider::with_base_url(format!("http://{addr}"));
        provider.latest_complete_volume("KFTG", None).await.unwrap();
        assert!(provider.latest_complete_volume("KTLX", None).await.is_err());
        assert!(provider.latest_complete_volume("KPAH", None).await.is_err());
        assert!(provider
            .latest_complete_volume("../KFTG", None)
            .await
            .is_err());
        assert!(provider.cached_for("KFTG").is_some());
        assert!(provider.cached_for("KTLX").is_none());
    }

    #[tokio::test]
    async fn a_mislabelled_decoded_volume_cannot_seed_the_fallback_cache() {
        let app = axum::Router::new()
            .route(
                "/KTLX/dir.list",
                axum::routing::get(|| async {
                    format!("{} KTLX_20170508_203405.bz2\n", RECOVERY_BYTES.len())
                }),
            )
            .route(
                "/KTLX/KTLX_20170508_203405.bz2",
                axum::routing::get(|| async { RECOVERY_BYTES }),
            );
        let addr = spawn_test_server(app).await;
        let provider = NoaaTgftpLevel2Provider::with_base_url(format!("http://{addr}"));
        let result = provider.latest_complete_volume("KTLX", None).await;
        assert!(result.is_err(), "KFTG gates must not be delivered as KTLX");
        assert!(provider.last_known.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn a_regressed_directory_cannot_rewind_the_retained_fallback() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let app = axum::Router::new()
            .route(
                "/KFTG/dir.list",
                axum::routing::get(move || {
                    let first = count.fetch_add(1, Ordering::Relaxed) == 0;
                    async move {
                        let name = if first {
                            RECOVERY_NAME
                        } else {
                            "KFTG_20170508_202405.bz2"
                        };
                        format!("{} {name}\n", RECOVERY_BYTES.len())
                    }
                }),
            )
            .route(
                &format!("/KFTG/{RECOVERY_NAME}"),
                axum::routing::get(|| async { RECOVERY_BYTES }),
            )
            .route(
                "/KFTG/KFTG_20170508_202405.bz2",
                axum::routing::get(|| async { RECOVERY_BYTES }),
            );
        let addr = spawn_test_server(app).await;
        let provider = NoaaTgftpLevel2Provider::with_base_url(format!("http://{addr}"));
        provider.latest_complete_volume("KFTG", None).await.unwrap();
        let original = provider.cached_for("KFTG").unwrap();
        let LatestVolume::New { name, time, scan } =
            provider.latest_complete_volume("KFTG", None).await.unwrap()
        else {
            panic!("must retain the latest accepted source data");
        };
        assert_eq!(name, RECOVERY_NAME);
        assert_eq!(time, original.time);
        assert_eq!(scan.sweeps(), original.scan.sweeps());
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn fallback_listing_names_are_exact_site_qualified_basenames() {
        for bad in [
            "../KFTG_20170508_203405.bz2",
            "KFTG_201758_203405.bz2",
            "KFTG_20170508_203405.bz2?other",
            "KFTG_20170508_203405_extra.bz2",
            "KFTG_20170508_20340é.bz2",
        ] {
            assert!(parse_filename_time(bad).is_none(), "{bad}");
        }
        let entries = vec![
            ("KTLX_20261012_000000.bz2".into(), 1),
            (RECOVERY_NAME.into(), 1),
        ];
        assert_eq!(
            newest_entry_for_site(&entries, "kftg").unwrap().0,
            RECOVERY_NAME
        );
        assert!(newest_entry_for_site(&entries, "KPAH").is_none());
    }

    fn stalled_body() -> axum::body::Body {
        axum::body::Body::from_stream(futures_util::stream::once(std::future::pending::<
            Result<Vec<u8>, std::io::Error>,
        >()))
    }

    #[tokio::test]
    async fn a_subscription_retries_after_timeout_and_delivers_the_recovered_scan() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        let app = axum::Router::new()
            .route(
                "/KFTG/dir.list",
                axum::routing::get(move || {
                    let count = count.clone();
                    async move {
                        if count.fetch_add(1, Ordering::Relaxed) == 0 {
                            std::future::pending::<()>().await;
                        }
                        format!("{} {RECOVERY_NAME}\n", RECOVERY_BYTES.len())
                    }
                }),
            )
            .route(
                &format!("/KFTG/{RECOVERY_NAME}"),
                axum::routing::get(|| async { RECOVERY_BYTES }),
            );
        let addr = spawn_test_server(app).await;
        let mut provider = NoaaTgftpLevel2Provider::with_base_url(format!("http://{addr}"));
        provider.list_timeout = std::time::Duration::from_millis(100);
        provider.poll_interval = std::time::Duration::from_millis(150);
        let expected = wxdata::level2::decode_volume(RECOVERY_BYTES.to_vec()).unwrap();
        let base = Arc::new(Scan::new(expected.coverage_pattern().clone(), Vec::new()));
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let flag = active.clone();
        let stop = active.clone();
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let started = std::time::Instant::now();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            provider.subscribe(
                "KFTG".into(),
                base,
                Box::new(move || flag.load(Ordering::Relaxed)),
                Box::new(move |update| {
                    tx.try_send(update).unwrap();
                    stop.store(false, Ordering::Relaxed);
                }),
                Box::new(|_| panic!("completed fallback must not fabricate progress")),
            ),
        )
        .await
        .expect("the stream must recover after its failed poll")
        .unwrap();
        let update = rx.recv().await.unwrap();
        assert_eq!(requests.load(Ordering::Relaxed), 2, "no hot retry loop");
        assert_eq!(update.name, RECOVERY_NAME);
        assert_eq!(update.time, parse_filename_time(RECOVERY_NAME).unwrap());
        assert_eq!(update.scan.sweeps(), expected.sweeps());
        assert!(update.received_at.is_none() && update.radial_coverage.is_none());
        assert!(
            update.decode_time.is_none(),
            "unobserved fallback decode must not report zero"
        );
        assert!(started.elapsed() >= provider.list_timeout + provider.poll_interval);
        println!(
            "TGFTP timeout -> retry -> recovered update: {} ms, 2 polls",
            started.elapsed().as_millis()
        );
    }

    #[tokio::test]
    async fn a_stalled_listing_times_out_then_recovers_without_changing_source_data() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let stalled = Arc::new(AtomicBool::new(true));
        let flag = stalled.clone();
        let app = axum::Router::new()
            .route(
                "/KFTG/dir.list",
                axum::routing::get(move || {
                    let flag = flag.clone();
                    async move {
                        if flag.load(Ordering::Relaxed) {
                            std::future::pending::<()>().await;
                        }
                        format!("{} {RECOVERY_NAME}\n", RECOVERY_BYTES.len())
                    }
                }),
            )
            .route(
                &format!("/KFTG/{RECOVERY_NAME}"),
                axum::routing::get(|| async { RECOVERY_BYTES }),
            );
        let addr = spawn_test_server(app).await;
        let mut provider = NoaaTgftpLevel2Provider::with_base_url(format!("http://{addr}"));
        provider.list_timeout = std::time::Duration::from_millis(100);
        let failed = match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            provider.latest_complete_volume("KFTG", None),
        )
        .await
        .expect("the listing deadline must release the poll")
        {
            Err(error) => error,
            Ok(_) => panic!("a stalled listing must fail"),
        };
        assert!(failed.to_string().contains("timed out"));
        assert!(provider.last_known.lock().unwrap().is_none());

        stalled.store(false, Ordering::Relaxed);
        let LatestVolume::New { name, time, scan } =
            provider.latest_complete_volume("KFTG", None).await.unwrap()
        else {
            panic!("a successful next poll must recover");
        };
        assert_eq!(name, RECOVERY_NAME);
        assert_eq!(time, parse_filename_time(RECOVERY_NAME).unwrap());
        let expected = wxdata::level2::decode_volume(RECOVERY_BYTES.to_vec()).unwrap();
        let actual =
            wxdata::level2::bin_scan(&scan, wxdata::level2::Moment::Reflectivity, 0).unwrap();
        let expected =
            wxdata::level2::bin_scan(&expected, wxdata::level2::Moment::Reflectivity, 0).unwrap();
        assert_eq!(actual.data, expected.data);
        assert_eq!(actual.bin_time_ms, expected.bin_time_ms);

        // A new outage retains only the original clock and gates, never "now".
        stalled.store(true, Ordering::Relaxed);
        assert!(matches!(
            provider
                .latest_complete_volume("KFTG", Some(RECOVERY_NAME))
                .await
                .unwrap(),
            LatestVolume::UpToDate
        ));
        let cached = provider.last_known.lock().unwrap();
        assert_eq!(cached.as_ref().unwrap().time, time);
        assert_eq!(cached.as_ref().unwrap().scan.sweeps(), scan.sweeps());
    }

    #[tokio::test]
    async fn deadlines_cover_stalled_listing_and_volume_bodies_after_headers() {
        let app = axum::Router::new()
            .route(
                "/KTLX/dir.list",
                axum::routing::get(|| async { stalled_body() }),
            )
            .route(
                "/KTLX/volume.bz2",
                axum::routing::get(|| async { stalled_body() }),
            );
        let addr = spawn_test_server(app).await;
        // Negative control: the baseline's unbounded headers+body expression cannot finish
        // against this server. This demonstrates the fault independently of the timeout helper.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), async {
                reqwest::get(format!("http://{addr}/KTLX/dir.list"))
                    .await
                    .unwrap()
                    .error_for_status()
                    .unwrap()
                    .text()
                    .await
            })
            .await
            .is_err()
        );
        let mut provider = NoaaTgftpLevel2Provider::with_base_url(format!("http://{addr}"));
        provider.list_timeout = std::time::Duration::from_millis(100);
        provider.volume_timeout = std::time::Duration::from_millis(100);
        assert!(tokio::time::timeout(
            std::time::Duration::from_secs(3),
            provider.fetch_dir_list("KTLX")
        )
        .await
        .expect("listing body must time out")
        .is_err());
        assert!(tokio::time::timeout(
            std::time::Duration::from_secs(3),
            provider.fetch_volume_bytes("KTLX", "volume.bz2")
        )
        .await
        .expect("volume body must time out")
        .is_err());
    }

    #[tokio::test]
    async fn cancelling_a_stalled_subscription_releases_headers_and_body_reads() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        for body_stall in [false, true] {
            let requested = Arc::new(tokio::sync::Notify::new());
            let signal = requested.clone();
            let body_signal = requested.clone();
            let app = axum::Router::new()
                .route(
                    "/KFTG/dir.list",
                    axum::routing::get(move || {
                        let signal = signal.clone();
                        async move {
                            if !body_stall {
                                signal.notify_one();
                                std::future::pending::<()>().await;
                            }
                            format!("{} {RECOVERY_NAME}\n", RECOVERY_BYTES.len())
                        }
                    }),
                )
                .route(
                    &format!("/KFTG/{RECOVERY_NAME}"),
                    axum::routing::get(move || {
                        let signal = body_signal.clone();
                        async move {
                            signal.notify_one();
                            stalled_body()
                        }
                    }),
                );
            let addr = spawn_test_server(app).await;
            let provider = NoaaTgftpLevel2Provider::with_base_url(format!("http://{addr}"));
            let active = Arc::new(AtomicBool::new(true));
            let flag = active.clone();
            let updates = Arc::new(AtomicUsize::new(0));
            let count = updates.clone();
            let base = Arc::new(wxdata::level2::decode_volume(RECOVERY_BYTES.to_vec()).unwrap());
            let handle = tokio::spawn(async move {
                provider
                    .subscribe(
                        "KFTG".into(),
                        base,
                        Box::new(move || flag.load(Ordering::Relaxed)),
                        Box::new(move |_| {
                            count.fetch_add(1, Ordering::Relaxed);
                        }),
                        Box::new(|_| panic!("TGFTP has no progressive coverage")),
                    )
                    .await
            });
            tokio::time::timeout(std::time::Duration::from_secs(3), requested.notified())
                .await
                .expect("server must see the in-flight request");
            let cancelled_at = std::time::Instant::now();
            active.store(false, Ordering::Relaxed);
            tokio::time::timeout(std::time::Duration::from_secs(2), handle)
                .await
                .expect("cancel must not wait for the HTTP deadline")
                .unwrap()
                .unwrap();
            assert_eq!(updates.load(Ordering::Relaxed), 0);
            println!(
                "TGFTP cancellation body_stall={body_stall}: {} ms",
                cancelled_at.elapsed().as_millis()
            );
        }
    }
}
