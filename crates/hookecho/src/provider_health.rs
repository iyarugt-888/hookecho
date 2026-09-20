//! ROADMAP_NEW B6.11 step 7: run more than one [`Level2LiveProvider`] for a site concurrently and
//! track each one's health independently, **without** changing which provider's data is actually
//! rendered — that decision stays with the existing single-provider live pipeline
//! (`MapView`/`view.rs`) until the failover arbiter (B6.11 step 8) exists to make it safely.
//!
//! This module answers "how healthy is the backup path right now, compared to the primary?" It
//! does not answer "which one should be active?" — mixing those two questions is exactly what
//! B6.7's "never mix sources when identity is uncertain" rule warns against, and a health monitor
//! has no business making a switching decision on its own.
//!
//! Native only, like [`crate::relay_provider`]: a dual-feed comparison is only meaningful once a
//! second provider exists to compare against, and the only second provider implemented so far
//! (`HookEchoRelayLevel2Provider`) is itself native-only.

use crate::volume::Level2LiveProvider;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wxdata::level2::Scan;
use wxdata::live_block::ProviderCapabilities;

/// Per-provider health for one radar site — the subset of ROADMAP_NEW B6.6's full tracked-state
/// list achievable from a single provider's own callbacks, without the cross-provider radial
/// identity comparison B6.5/B6.7 add later (so no sequence-gap/duplicate/conflict counts yet —
/// those describe a *relationship* between two providers' data, not one provider's own behavior).
#[derive(Debug, Clone)]
pub struct ProviderHealth {
    pub label: &'static str,
    pub capabilities: ProviderCapabilities,
    /// The most recent radar-time timestamp this provider has delivered, if any.
    pub newest_radar_time: Option<DateTime<Utc>>,
    /// Wall-clock time this provider last successfully delivered an update.
    pub last_receipt_at: Option<DateTime<Utc>>,
    pub successes: u32,
    pub failures: u32,
    /// Failed/errored `subscribe` attempts in a row since the last successful `on_update`, reset
    /// to zero the instant one arrives — [`crate::failover_arbiter::ArbiterInput`]'s own field of
    /// the same name wants exactly this, not the lifetime `failures` total below it (an old
    /// failure hours ago shouldn't still count against a provider that has since recovered).
    pub consecutive_failures: u32,
    /// How many times `subscribe` has ended (cleanly or with an error) and been restarted.
    pub reconnects: u32,
    pub last_error: Option<String>,
}

impl ProviderHealth {
    fn new(label: &'static str, capabilities: ProviderCapabilities) -> Self {
        Self {
            label,
            capabilities,
            newest_radar_time: None,
            last_receipt_at: None,
            successes: 0,
            failures: 0,
            consecutive_failures: 0,
            reconnects: 0,
            last_error: None,
        }
    }
}

/// Shared, thread-safe snapshot of every monitored provider's health for one site, keyed by
/// [`Level2LiveProvider::label`]. Cheap to clone (an `Arc`) and safe to read from the UI thread at
/// any time — a monitor only ever holds the lock for the duration of one field update.
pub type HealthBoard = Arc<Mutex<HashMap<&'static str, ProviderHealth>>>;

/// Runs one provider's `subscribe` in a reconnect loop (restarting on error or a clean end, while
/// `active` stays true) purely to observe its health — it never calls a caller-supplied
/// `on_update`, so nothing here can end up rendered. Intended to run alongside the site's real,
/// rendering subscription (already wired into `MapView`), not replace it.
///
/// Returns once `active()` reports false, so the caller controls its lifetime the same way every
/// other live subscription in this codebase does (see [`Level2LiveProvider::subscribe`]'s own
/// `active` doc comment).
pub async fn monitor_provider(
    provider: Arc<dyn Level2LiveProvider + Send + Sync>,
    site: String,
    base: Arc<Scan>,
    active: Arc<dyn Fn() -> bool + Send + Sync>,
    board: HealthBoard,
) {
    let label = provider.label();
    {
        let mut board = board.lock().unwrap();
        board
            .entry(label)
            .or_insert_with(|| ProviderHealth::new(label, provider.capabilities()));
    }

    while active() {
        let board_for_updates = board.clone();
        let active_for_subscribe = active.clone();
        let result = provider
            .subscribe(
                site.clone(),
                base.clone(),
                Box::new(move || active_for_subscribe()),
                Box::new(move |update: wxdata::live::Update| {
                    let mut board = board_for_updates.lock().unwrap();
                    if let Some(health) = board.get_mut(label) {
                        health.newest_radar_time = Some(update.time);
                        health.last_receipt_at = Some(Utc::now());
                        health.successes += 1;
                        health.consecutive_failures = 0;
                    }
                }),
                Box::new(|_progress| {}),
            )
            .await;

        {
            // Block-scoped rather than an explicit `drop()`: this reliably ends the guard's
            // borrow region before the `.await` below, in a way an async fn's generator lowering
            // always recognizes (a manual `drop()` call does not always narrow the region the
            // same way).
            let mut board_guard = board.lock().unwrap();
            if let Some(health) = board_guard.get_mut(label) {
                if let Err(e) = &result {
                    health.failures += 1;
                    health.consecutive_failures += 1;
                    health.last_error = Some(e.to_string());
                }
                if active() {
                    health.reconnects += 1;
                }
            }
        }

        if !active() {
            break;
        }
        // Brief backoff before reconnecting, so a persistently failing provider doesn't spin the
        // task hot — a lightweight stand-in for the hysteresis/cooldown policy B6.6 asks for at
        // the arbiter level; this alone doesn't decide anything, it just avoids busy-looping.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

/// Starts a [`monitor_provider`] task for every given provider and returns the shared
/// [`HealthBoard`] immediately — the ROADMAP_NEW B6.11 step 7 entry point: "run Unidata + relay
/// simultaneously and expose comparative health without switching." Each provider gets its own
/// `base` scan (they are independent acquisition paths; there is no shared merge state between
/// them at this stage) and shares one `active` gate, so stopping the comparison stops every
/// monitored provider together.
///
/// Takes the app's [`crate::rt::Spawner`] rather than calling `tokio::spawn`: this is called from
/// the UI thread, which has no runtime context, and a bare `tokio::spawn` there panics with "there
/// is no reactor running" (the Windows build died at startup on exactly that).
pub fn spawn_dual_feed_monitor(
    spawner: &crate::rt::Spawner,
    site: String,
    providers: Vec<Arc<dyn Level2LiveProvider + Send + Sync>>,
    base: Arc<Scan>,
    active: Arc<dyn Fn() -> bool + Send + Sync>,
) -> HealthBoard {
    let board: HealthBoard = Arc::new(Mutex::new(HashMap::new()));
    for provider in providers {
        let site = site.clone();
        let base = base.clone();
        let active = active.clone();
        let board = board.clone();
        spawner.spawn(async move {
            monitor_provider(provider, site, base, active, board).await;
        });
    }
    board
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volume::LatestVolume;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A provider that calls `on_update` a fixed number of times, each with a synthetic Update,
    /// then ends `subscribe` cleanly — deterministic and network-free, per this engagement's
    /// "add deterministic replay tests before depending on live network tests" rule.
    struct ScriptedProvider {
        label: &'static str,
        capabilities: ProviderCapabilities,
        updates_to_send: u32,
        sent: AtomicU32,
        fail: bool,
    }

    fn empty_scan() -> Scan {
        let vcp = nexrad_model::data::VolumeCoveragePattern::new(
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
        Scan::new(vcp, Vec::new())
    }

    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    impl Level2LiveProvider for ScriptedProvider {
        fn label(&self) -> &'static str {
            self.label
        }

        fn capabilities(&self) -> ProviderCapabilities {
            self.capabilities
        }

        async fn subscribe(
            &self,
            _site: String,
            base: Arc<Scan>,
            active: Box<dyn Fn() -> bool + Send + Sync>,
            mut on_update: Box<dyn FnMut(wxdata::live::Update) + Send>,
            _on_progress: Box<dyn FnMut(wxdata::live::ScanProgress) + Send>,
        ) -> anyhow::Result<()> {
            while active() && self.sent.load(Ordering::Relaxed) < self.updates_to_send {
                let n = self.sent.fetch_add(1, Ordering::Relaxed) + 1;
                on_update(wxdata::live::Update {
                    name: format!("scripted-{n}"),
                    time: Utc::now(),
                    scan: base.clone(),
                    changed: vec![0.5],
                    retries: 0,
                    decode_time: std::time::Duration::ZERO,
                });
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            if self.fail {
                anyhow::bail!("scripted failure");
            }
            Ok(())
        }

        async fn latest_complete_volume(
            &self,
            _site: &str,
            _current_name: Option<&str>,
        ) -> anyhow::Result<LatestVolume> {
            Ok(LatestVolume::UpToDate)
        }
    }

    #[tokio::test]
    async fn monitor_provider_records_successful_updates() {
        let provider = Arc::new(ScriptedProvider {
            label: "Scripted A",
            capabilities: ProviderCapabilities::unidata(),
            updates_to_send: 3,
            sent: AtomicU32::new(0),
            fail: false,
        });
        let board: HealthBoard = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let active_read = {
            let active = active.clone();
            move || active.load(Ordering::Relaxed)
        };

        let board_clone = board.clone();
        let handle = tokio::spawn(async move {
            monitor_provider(
                provider,
                "KTLX".to_string(),
                Arc::new(empty_scan()),
                Arc::new(active_read),
                board_clone,
            )
            .await;
        });

        // The scripted provider sends 3 updates then ends cleanly; give it time to finish one
        // full pass before stopping the loop from reconnecting further.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        active.store(false, Ordering::Relaxed);
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("monitor task did not stop after active() went false")
            .unwrap();

        let board = board.lock().unwrap();
        let health = board.get("Scripted A").expect("provider must be tracked");
        assert_eq!(health.successes, 3);
        assert!(health.last_receipt_at.is_some());
        assert!(health.newest_radar_time.is_some());
    }

    #[tokio::test]
    async fn monitor_provider_records_failures_and_reconnects() {
        let provider = Arc::new(ScriptedProvider {
            label: "Scripted B",
            capabilities: ProviderCapabilities::relay(),
            updates_to_send: 1,
            sent: AtomicU32::new(0),
            fail: true,
        });
        let board: HealthBoard = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let active_read = {
            let active = active.clone();
            move || active.load(Ordering::Relaxed)
        };

        let board_clone = board.clone();
        let handle = tokio::spawn(async move {
            monitor_provider(
                provider,
                "KTLX".to_string(),
                Arc::new(empty_scan()),
                Arc::new(active_read),
                board_clone,
            )
            .await;
        });

        // Let it fail and reconnect at least once (2s backoff between attempts).
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        active.store(false, Ordering::Relaxed);
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("monitor task did not stop after active() went false")
            .unwrap();

        let board = board.lock().unwrap();
        let health = board.get("Scripted B").expect("provider must be tracked");
        assert_eq!(health.failures, 1);
        assert_eq!(health.consecutive_failures, 1);
        assert_eq!(health.last_error.as_deref(), Some("scripted failure"));
    }

    /// Fails `fail_first_n` `subscribe` attempts, then sends one update and ends cleanly.
    struct FlakyThenHealthyProvider {
        label: &'static str,
        fail_first_n: u32,
        attempts: AtomicU32,
    }

    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    impl Level2LiveProvider for FlakyThenHealthyProvider {
        fn label(&self) -> &'static str {
            self.label
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::unidata()
        }

        async fn subscribe(
            &self,
            _site: String,
            base: Arc<Scan>,
            _active: Box<dyn Fn() -> bool + Send + Sync>,
            mut on_update: Box<dyn FnMut(wxdata::live::Update) + Send>,
            _on_progress: Box<dyn FnMut(wxdata::live::ScanProgress) + Send>,
        ) -> anyhow::Result<()> {
            let attempt = self.attempts.fetch_add(1, Ordering::Relaxed);
            if attempt < self.fail_first_n {
                anyhow::bail!("flaky attempt {attempt}");
            }
            on_update(wxdata::live::Update {
                name: "recovered".to_string(),
                time: Utc::now(),
                scan: base,
                changed: vec![0.5],
                retries: 0,
                decode_time: std::time::Duration::ZERO,
            });
            Ok(())
        }

        async fn latest_complete_volume(
            &self,
            _site: &str,
            _current_name: Option<&str>,
        ) -> anyhow::Result<LatestVolume> {
            Ok(LatestVolume::UpToDate)
        }
    }

    #[tokio::test]
    async fn a_success_resets_consecutive_failures_to_zero() {
        let provider = Arc::new(FlakyThenHealthyProvider {
            label: "Flaky",
            fail_first_n: 2,
            attempts: AtomicU32::new(0),
        });
        let board: HealthBoard = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let active_read = {
            let active = active.clone();
            move || active.load(Ordering::Relaxed)
        };

        let board_clone = board.clone();
        let handle = tokio::spawn(async move {
            monitor_provider(
                provider,
                "KTLX".to_string(),
                Arc::new(empty_scan()),
                Arc::new(active_read),
                board_clone,
            )
            .await;
        });

        // Two failures, each followed by the monitor's 2s reconnect backoff, then a success —
        // generous enough to clear both backoffs before the recovery is observed.
        tokio::time::sleep(std::time::Duration::from_millis(4500)).await;
        active.store(false, Ordering::Relaxed);
        tokio::time::timeout(std::time::Duration::from_secs(3), handle)
            .await
            .expect("monitor task did not stop after active() went false")
            .unwrap();

        let board = board.lock().unwrap();
        let health = board.get("Flaky").expect("provider must be tracked");
        assert_eq!(health.failures, 2);
        assert_eq!(
            health.consecutive_failures, 0,
            "a subsequent success must reset the consecutive-failure count the arbiter reads"
        );
        assert_eq!(health.successes, 1);
    }

    #[tokio::test]
    async fn spawn_dual_feed_monitor_tracks_every_given_provider_independently() {
        let unidata_like = Arc::new(ScriptedProvider {
            label: "Provider One",
            capabilities: ProviderCapabilities::unidata(),
            updates_to_send: 2,
            sent: AtomicU32::new(0),
            fail: false,
        });
        let relay_like = Arc::new(ScriptedProvider {
            label: "Provider Two",
            capabilities: ProviderCapabilities::relay(),
            updates_to_send: 2,
            sent: AtomicU32::new(0),
            fail: false,
        });
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let active_read = {
            let active = active.clone();
            move || active.load(Ordering::Relaxed)
        };

        let board = spawn_dual_feed_monitor(
            &crate::rt::Spawner::new(tokio::runtime::Handle::current()),
            "KTLX".to_string(),
            vec![unidata_like, relay_like],
            Arc::new(empty_scan()),
            Arc::new(active_read),
        );

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        active.store(false, Ordering::Relaxed);
        // Give both spawned tasks a moment to notice `active` went false and stop.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let board = board.lock().unwrap();
        assert!(board.contains_key("Provider One"));
        assert!(board.contains_key("Provider Two"));
        assert!(board["Provider One"].successes > 0);
        assert!(board["Provider Two"].successes > 0);
    }

    /// The UI thread has no tokio runtime context, and `SiteProviders::start` is called from it. A
    /// bare `tokio::spawn` there panics with "there is no reactor running" — which took the Windows
    /// build down at startup. The monitor must start from a plain thread when handed a Spawner.
    #[test]
    fn the_monitor_starts_from_a_thread_with_no_runtime() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let spawner = crate::rt::Spawner::new(rt.handle().clone());
        let provider = Arc::new(ScriptedProvider {
            label: "Provider One",
            capabilities: ProviderCapabilities::unidata(),
            updates_to_send: 1,
            sent: AtomicU32::new(0),
            fail: false,
        });
        assert!(
            tokio::runtime::Handle::try_current().is_err(),
            "this test must call from outside any runtime, or it proves nothing"
        );
        let board = spawn_dual_feed_monitor(
            &spawner,
            "KTLX".to_string(),
            vec![provider],
            Arc::new(empty_scan()),
            Arc::new(|| false),
        );
        drop(board);
    }
}
