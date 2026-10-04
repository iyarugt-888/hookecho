//! One live subscription's ownership, retry scope and cancellation generation.
//! No clocks or volume identity are inferred here: this controls transport lifecycle only.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;
use wxdata::clock::Instant;

pub(super) const RETRY_INTERVAL: Duration = Duration::from_secs(60);

/// Shared by subscription ownership and monitor configuration, matching provider construction.
pub(super) fn relay_endpoint_key(url: &str) -> &str {
    url.trim().trim_end_matches('/')
}

#[derive(Clone, Copy)]
pub(super) struct StreamRequest<'a> {
    pub view: usize,
    pub site: &'a str,
    pub provider: &'static str,
    /// Only the selected relay's configuration; an inactive backup cannot change the primary.
    pub relay_endpoint: Option<&'a str>,
}

#[derive(Clone)]
struct Scope {
    view: usize,
    site: String,
    provider: &'static str,
    endpoint: Option<String>,
}

impl Scope {
    fn new(request: StreamRequest<'_>) -> Self {
        Self {
            view: request.view,
            site: request.site.into(),
            provider: request.provider,
            endpoint: request
                .relay_endpoint
                .map(|url| relay_endpoint_key(url).into()),
        }
    }
    fn matches(&self, request: StreamRequest<'_>) -> bool {
        self.view == request.view
            && self.site == request.site
            && self.provider == request.provider
            && self.endpoint.as_deref() == request.relay_endpoint.map(relay_endpoint_key)
    }
}

struct Running {
    scope: Scope,
    generation: u64,
}
struct Attempt {
    scope: Scope,
    at: Instant,
}

pub(super) struct StreamStart {
    pub view: usize,
    pub site: String,
    pub provider: &'static str,
    pub generation: u64,
    pub context_changed: bool,
}

#[derive(Default)]
pub(super) struct StreamDecision {
    pub stopped_view: Option<usize>,
    pub start: Option<StreamStart>,
    pub retry_after: Option<Duration>,
}

#[derive(Default)]
pub(super) struct LiveSession {
    running: Option<Running>,
    last_attempt: Option<Attempt>,
    generation: Arc<AtomicU64>,
}

impl LiveSession {
    pub fn token(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.generation)
    }
    pub fn streaming_for(&self, view: usize) -> bool {
        self.running
            .as_ref()
            .is_some_and(|session| session.scope.view == view)
    }
    pub fn accepts(&self, view: usize, generation: u64) -> bool {
        self.running
            .as_ref()
            .is_some_and(|session| session.scope.view == view && session.generation == generation)
    }
    fn retire(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.running = None;
    }
    /// Called even after a context change, but only the current subscription can finish itself.
    pub fn finish(&mut self, view: usize, generation: u64, lost: bool) -> bool {
        if !self.accepts(view, generation) {
            return false;
        }
        self.retire();
        if !lost {
            self.last_attempt = None;
        }
        true
    }
    /// Retire the subscription even if its pane was removed or selected another radar.
    /// Its end/error belongs only to the original receiver, never the new selection.
    pub fn finish_into(
        &mut self,
        view: usize,
        generation: u64,
        lost: bool,
        error: Option<String>,
        views: &mut [crate::view::MapView],
    ) {
        let Some(running) = self
            .running
            .as_ref()
            .filter(|_| self.accepts(view, generation))
        else {
            return;
        };
        let site = running.scope.site.clone();
        self.finish(view, generation, lost);
        if let Some(v) = views.get_mut(view).filter(|v| {
            v.site.as_deref() == Some(site.as_str())
                && v.live_scan.site.as_deref() == Some(site.as_str())
        }) {
            v.live_progress = None;
            v.live_progress_at = None;
            v.live_retries = 0;
            if lost {
                v.live_scan.stream_ended_with_error(error);
            } else {
                v.live_scan.stream_stopped();
            }
        }
    }
    /// The app's per-frame sync uses this decision before spawning or accepting work.
    /// At most one active scope and one last-attempt scope are retained; no payloads/URLs logged.
    pub fn reconcile(&mut self, wanted: Option<StreamRequest<'_>>, now: Instant) -> StreamDecision {
        let mut decision = StreamDecision::default();
        let cancel = self
            .running
            .as_ref()
            .is_some_and(|running| wanted.is_none_or(|request| !running.scope.matches(request)));
        if cancel {
            decision.stopped_view = self.running.as_ref().map(|running| running.scope.view);
            self.retire();
        }
        let Some(request) = wanted else {
            // Pausing/scrubbing or foreground suspension must not penalize the later resume.
            self.last_attempt = None;
            return decision;
        };
        if self.running.is_some() {
            return decision;
        }
        let context_changed = self
            .last_attempt
            .as_ref()
            .is_some_and(|attempt| !attempt.scope.matches(request));
        if let Some(attempt) = &self.last_attempt {
            if !cancel && attempt.scope.matches(request) {
                let elapsed = now.saturating_duration_since(attempt.at);
                if elapsed < RETRY_INTERVAL {
                    decision.retry_after = Some(RETRY_INTERVAL - elapsed);
                    return decision;
                }
            }
        }
        // Every attempt, including same-context restoration, gets a fresh cancellation generation.
        let generation = self
            .generation
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        let scope = Scope::new(request);
        decision.start = Some(StreamStart {
            view: scope.view,
            site: scope.site.clone(),
            provider: scope.provider,
            generation,
            context_changed,
        });
        self.last_attempt = Some(Attempt {
            scope: scope.clone(),
            at: now,
        });
        self.running = Some(Running { scope, generation });
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn primary(view: usize, site: &str) -> StreamRequest<'_> {
        StreamRequest {
            view,
            site,
            provider: "Unidata Level II (AWS S3)",
            relay_endpoint: None,
        }
    }
    fn relay(endpoint: &str) -> StreamRequest<'_> {
        StreamRequest {
            view: 0,
            site: "KTLX",
            provider: "HookEcho Relay",
            relay_endpoint: Some(endpoint),
        }
    }

    #[test]
    fn lost_subscription_retries_are_scoped_bounded_and_have_fresh_generations() {
        let now = Instant::now();
        let mut session = LiveSession::default();
        let token = session.token();
        let first = session
            .reconcile(Some(primary(0, "KTLX")), now)
            .start
            .unwrap();
        assert!(session.accepts(0, first.generation));
        assert!(session.finish(0, first.generation, true));
        assert_ne!(token.load(Ordering::Relaxed), first.generation);
        let waiting = session.reconcile(Some(primary(0, "KTLX")), now + Duration::from_secs(5));
        assert!(waiting.start.is_none());
        assert_eq!(waiting.retry_after, Some(Duration::from_secs(55)));
        let replacement = session
            .reconcile(
                Some(relay("http://127.0.0.1:8080/")),
                now + Duration::from_secs(6),
            )
            .start
            .unwrap();
        assert!(replacement.context_changed);
        assert_ne!(first.generation, replacement.generation);
        assert!(
            !session.finish(0, first.generation, true),
            "late End cannot clear the replacement"
        );
        assert!(session.accepts(0, replacement.generation));
        assert!(session.finish(0, replacement.generation, true));
        let retry = session
            .reconcile(
                Some(relay("http://127.0.0.1:8080")),
                now + Duration::from_secs(66),
            )
            .start
            .unwrap();
        assert_ne!(replacement.generation, retry.generation);
        assert!(!session.accepts(0, replacement.generation));
        assert!(!retry.context_changed);
    }

    #[test]
    fn active_endpoint_changes_cancel_and_unchanged_normalized_endpoints_do_not_churn() {
        let now = Instant::now();
        let mut session = LiveSession::default();
        let token = session.token();
        let old = session
            .reconcile(Some(relay("http://127.0.0.1:8080/")), now)
            .start
            .unwrap();
        assert!(session
            .reconcile(Some(relay("  http://127.0.0.1:8080  ")), now)
            .start
            .is_none());
        assert_eq!(token.load(Ordering::Relaxed), old.generation);
        let changed = session.reconcile(Some(relay("http://127.0.0.1:8081")), now);
        assert_eq!(changed.stopped_view, Some(0));
        let next = changed.start.unwrap();
        assert!(next.context_changed);
        assert!(!session.accepts(0, old.generation));
        assert!(session.accepts(0, next.generation));
        assert_eq!(token.load(Ordering::Relaxed), next.generation);
    }

    #[test]
    fn ended_stream_site_pane_and_configuration_changes_bypass_old_retry_delays() {
        let now = Instant::now();
        for wanted in [
            primary(0, "KOUN"),
            primary(1, "KTLX"),
            relay("http://127.0.0.1:8080"),
        ] {
            let mut session = LiveSession::default();
            let first = session
                .reconcile(Some(primary(0, "KTLX")), now)
                .start
                .unwrap();
            session.finish(0, first.generation, true);
            let next = session
                .reconcile(Some(wanted), now + Duration::from_secs(1))
                .start
                .unwrap();
            assert!(next.context_changed);
            assert!(session.accepts(wanted.view, next.generation));
        }
        let mut session = LiveSession::default();
        let first = session
            .reconcile(Some(relay("http://127.0.0.1:8080")), now)
            .start
            .unwrap();
        session.finish(0, first.generation, true);
        assert!(
            session
                .reconcile(Some(relay("http://127.0.0.1:8081")), now)
                .start
                .unwrap()
                .context_changed
        );
    }

    #[test]
    fn intentional_pause_and_background_end_resume_without_failure_throttle() {
        let now = Instant::now();
        let mut session = LiveSession::default();
        let first = session
            .reconcile(Some(primary(0, "KTLX")), now)
            .start
            .unwrap();
        let pause = session.reconcile(None, now + Duration::from_secs(1));
        assert_eq!(pause.stopped_view, Some(0));
        assert!(!session.accepts(0, first.generation));
        let resumed = session
            .reconcile(Some(primary(0, "KTLX")), now + Duration::from_secs(2))
            .start
            .unwrap();
        assert!(session.finish(0, resumed.generation, false));
        assert!(session
            .reconcile(Some(primary(0, "KTLX")), now + Duration::from_secs(3))
            .start
            .is_some());
    }

    fn pane(site: &str) -> (crate::view::MapView, crate::live_scan::AcquisitionSnapshot) {
        let mut view = crate::view::MapView::new(
            Some(site.into()),
            crate::render::mercator::Camera::at_lonlat(-97.0, 35.0, 8.0),
        );
        let (receiver, receipt) = crate::live_scan::acquisition_fixture(site);
        view.live_scan = receiver;
        (view, receipt)
    }

    #[test]
    fn receiver_recovery_poll_and_restoration_preserve_clocks_and_frozen_receipts() {
        let now = Instant::now();
        let clock = chrono::DateTime::from_timestamp_millis(1_700_000_000_000).unwrap();
        let (view, receipt) = pane("KTLX");
        let frozen = receipt.inventory().clone();
        let mut views = [view];
        let mut session = LiveSession::default();
        let first = session
            .reconcile(Some(primary(0, "KTLX")), now)
            .start
            .unwrap();
        views[0].live_scan.stream_started(first.provider, false);
        assert!(views[0].live_scan.accept_volume("control", clock, clock));
        session.finish_into(
            0,
            first.generation,
            true,
            Some("controlled loss".into()),
            &mut views,
        );
        assert_eq!(
            views[0].live_scan.last_stream_error.as_deref(),
            Some("controlled loss")
        );
        assert_eq!(
            views[0].live_scan.poll_reason(),
            "live stream unavailable; completed-volume polling"
        );
        assert_eq!(views[0].live_scan.volume_time, Some(clock));
        let poll_clock = clock + chrono::Duration::seconds(1);
        assert!(views[0]
            .live_scan
            .accept_volume("completed-control", poll_clock, poll_clock));
        assert_eq!(
            views[0].live_scan.provider.as_deref(),
            Some(crate::live_scan::COMPLETED_POLL_LABEL)
        );

        let next = session
            .reconcile(Some(primary(0, "KTLX")), now + RETRY_INTERVAL)
            .start
            .unwrap();
        views[0].live_scan.stream_started(next.provider, false);
        assert!(views[0].live_scan.last_stream_error.is_none());
        let marker = wxdata::live::ScanProgress {
            volume_start_ms: Some(1_700_000_000_000),
            vcp_number: Some(212),
            cut_kind: wxdata::live::CutKind::Standard,
            elevation_number: 1,
            total_elevations: 3,
            elevation_angle_deg: 0.5,
            azimuth_rate_dps: 18.0,
            azimuth_start_deg: 0.0,
            azimuth_end_deg: 120.0,
            chunk_index: 1,
            chunks_in_sweep: 3,
        };
        views[0].observe_live_progress(marker, poll_clock, now);
        views[0].live_retries = 2;
        session.finish_into(
            0,
            first.generation,
            true,
            Some("late old loss".into()),
            &mut views,
        );
        assert!(session.accepts(0, next.generation));
        assert_eq!(views[0].live_progress, Some(marker));
        assert_eq!(views[0].live_progress_at, Some(now));
        assert_eq!(views[0].live_retries, 2);
        assert!(views[0].live_scan.last_stream_error.is_none());
        assert!(!views[0]
            .live_scan
            .accept_volume("older-control", clock, poll_clock));
        assert_eq!(views[0].live_scan.volume_time, Some(poll_clock));
        session.finish_into(
            0,
            next.generation,
            true,
            Some("replacement loss".into()),
            &mut views,
        );
        assert!(views[0].live_progress.is_none());
        assert!(views[0].live_progress_at.is_none());
        assert_eq!(views[0].live_retries, 0);
        assert_eq!(receipt.inventory(), &frozen);
    }

    #[test]
    fn original_end_retires_work_without_writing_into_reselected_or_removed_panes() {
        for receiver_reset in [false, true] {
            let now = Instant::now();
            let (view, _) = pane("KTLX");
            let mut views = [view];
            let mut session = LiveSession::default();
            let first = session
                .reconcile(Some(primary(0, "KTLX")), now)
                .start
                .unwrap();
            views[0].site = Some("KOUN".into());
            if receiver_reset {
                views[0].live_scan.reset(Some("KOUN".into()));
            }
            views[0].live_scan.last_stream_error = Some("new selection status".into());
            session.finish_into(
                0,
                first.generation,
                true,
                Some("old radar loss".into()),
                &mut views,
            );
            assert!(!session.accepts(0, first.generation));
            assert_eq!(
                views[0].live_scan.last_stream_error.as_deref(),
                Some("new selection status")
            );
            assert!(session
                .reconcile(Some(primary(0, "KOUN")), now)
                .start
                .is_some());
        }
        let mut session = LiveSession::default();
        let first = session
            .reconcile(Some(primary(0, "KTLX")), Instant::now())
            .start
            .unwrap();
        session.finish_into(0, first.generation, true, None, &mut []);
        assert!(!session.accepts(0, first.generation));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn automatic_failover_completed_floor_and_preferred_restore_keep_receiver_ownership() {
        use crate::provider_health::{HealthBoard, ProviderHealth};
        use crate::radar_provider_manager::{
            label_for_reason, label_for_tier, SelectedTier, SiteProviders, BACKUP_LABEL,
            PRIMARY_LABEL,
        };
        use std::collections::HashMap;
        use std::sync::Mutex;
        let clock = chrono::DateTime::from_timestamp_millis(1_700_000_000_000).unwrap();
        let mono = Instant::now();
        let mut primary = ProviderHealth::new(
            PRIMARY_LABEL,
            wxdata::live_block::ProviderCapabilities::unidata(),
        );
        let mut backup = ProviderHealth::new(
            BACKUP_LABEL,
            wxdata::live_block::ProviderCapabilities::relay(),
        );
        primary.record_update(clock, clock);
        backup.record_update(clock, clock);
        let board: HealthBoard = Arc::new(Mutex::new(HashMap::from([
            (PRIMARY_LABEL, primary),
            (BACKUP_LABEL, backup),
        ])));
        let mut providers =
            SiteProviders::for_test(Some("http://relay.control".into()), board.clone());
        let (view, receipt) = pane("KTLX");
        let frozen = receipt.inventory().clone();
        let mut views = [view];
        let mut session = LiveSession::default();
        let mut events = Vec::new();
        let request = |tier| StreamRequest {
            view: 0,
            site: "KTLX",
            provider: label_for_tier(tier),
            relay_endpoint: (tier == SelectedTier::Backup).then_some("http://relay.control"),
        };

        providers.tick_at(clock, Duration::ZERO);
        assert_eq!(providers.selected_tier(), SelectedTier::Primary);
        let first = session
            .reconcile(Some(request(providers.selected_tier())), mono)
            .start
            .unwrap();
        views[0].live_scan.stream_started(first.provider, false);
        assert!(views[0]
            .live_scan
            .accept_volume("initial-control", clock, clock));
        events.push(serde_json::json!({"stage":"initial", "selected":label_for_tier(providers.selected_tier()), "generation":first.generation, "source_time_ms":views[0].live_scan.volume_time.unwrap().timestamp_millis()}));

        let backup_clock = clock + chrono::Duration::seconds(10);
        {
            let mut board = board.lock().unwrap();
            for _ in 0..3 {
                board
                    .get_mut(PRIMARY_LABEL)
                    .unwrap()
                    .record_failure("primary transport control");
            }
            board
                .get_mut(BACKUP_LABEL)
                .unwrap()
                .record_update(backup_clock, backup_clock);
        }
        providers.tick_at(backup_clock, Duration::from_secs(10));
        assert_eq!(providers.selected_tier(), SelectedTier::Backup);
        assert_eq!(
            providers.snapshot().last_transition.unwrap().1,
            wxdata::live_block::ProviderSwitchReason::TransportError
        );
        let decision = session.reconcile(
            Some(request(providers.selected_tier())),
            mono + Duration::from_secs(1),
        );
        assert_eq!(decision.stopped_view, Some(0));
        let relay = decision.start.unwrap();
        assert!(!session.accepts(0, first.generation));
        views[0].live_scan.stream_started(relay.provider, true);
        assert!(views[0]
            .live_scan
            .accept_volume("backup-control", backup_clock, backup_clock));
        session.finish_into(
            0,
            first.generation,
            true,
            Some("late primary end".into()),
            &mut views,
        );
        assert!(session.accepts(0, relay.generation));
        assert!(views[0].live_scan.last_stream_error.is_none());
        events.push(serde_json::json!({"stage":"primary loss", "selected":label_for_tier(providers.selected_tier()), "reason":label_for_reason(providers.snapshot().last_transition.unwrap().1), "generation":relay.generation, "source_time_ms":views[0].live_scan.volume_time.unwrap().timestamp_millis()}));

        for _ in 0..3 {
            board
                .lock()
                .unwrap()
                .get_mut(BACKUP_LABEL)
                .unwrap()
                .record_failure("backup transport control");
        }
        let floor_clock = clock + chrono::Duration::seconds(191);
        providers.tick_at(floor_clock, Duration::from_secs(191));
        assert_eq!(providers.selected_tier(), SelectedTier::Degraded);
        let completed = session
            .reconcile(
                Some(request(providers.selected_tier())),
                mono + Duration::from_secs(2),
            )
            .start
            .unwrap();
        views[0].live_scan.stream_started(completed.provider, true);
        views[0]
            .live_scan
            .set_source_mode(crate::live_scan::SourceMode::CompletedVolumes);
        assert!(views[0]
            .live_scan
            .accept_volume("completed-control", floor_clock, floor_clock));
        session.finish_into(
            0,
            relay.generation,
            true,
            Some("late relay end".into()),
            &mut views,
        );
        assert!(session.accepts(0, completed.generation));
        assert_eq!(
            views[0].live_scan.source_mode,
            Some(crate::live_scan::SourceMode::CompletedVolumes)
        );
        let floor_transition = providers.snapshot().last_transition;
        events.push(serde_json::json!({"stage":"both sources lost", "selected":label_for_tier(providers.selected_tier()), "reason":label_for_reason(floor_transition.unwrap().1), "generation":completed.generation, "source_time_ms":views[0].live_scan.volume_time.unwrap().timestamp_millis()}));

        // A failure occurs between UI samples, after two new primary observations.
        {
            let mut board = board.lock().unwrap();
            let primary = board.get_mut(PRIMARY_LABEL).unwrap();
            primary.record_update(floor_clock, floor_clock);
            primary.record_update(floor_clock + chrono::Duration::seconds(1), floor_clock);
            primary.record_failure("flap between frames");
        }
        for count in 1..=2 {
            let recovery_clock = floor_clock + chrono::Duration::seconds(count + 1);
            board
                .lock()
                .unwrap()
                .get_mut(PRIMARY_LABEL)
                .unwrap()
                .record_update(recovery_clock, recovery_clock);
            for _ in 0..1000 {
                providers.tick_at(recovery_clock, Duration::from_secs(192));
                assert_eq!(providers.selected_tier(), SelectedTier::Degraded);
                assert_eq!(providers.snapshot().last_transition, floor_transition);
                assert!(session
                    .reconcile(
                        Some(request(providers.selected_tier())),
                        mono + Duration::from_secs(3)
                    )
                    .start
                    .is_none());
            }
            events.push(serde_json::json!({"stage":format!("recovery observation {count}"), "selected":label_for_tier(providers.selected_tier()), "generation":completed.generation, "source_time_ms":views[0].live_scan.volume_time.unwrap().timestamp_millis()}));
        }
        let restored_clock = floor_clock + chrono::Duration::seconds(4);
        board
            .lock()
            .unwrap()
            .get_mut(PRIMARY_LABEL)
            .unwrap()
            .record_update(restored_clock, restored_clock);
        providers.tick_at(restored_clock, Duration::from_secs(195));
        assert_eq!(providers.selected_tier(), SelectedTier::Primary);
        assert_eq!(
            providers.snapshot().last_transition.unwrap().1,
            wxdata::live_block::ProviderSwitchReason::Recovery
        );
        let restored = session
            .reconcile(
                Some(request(providers.selected_tier())),
                mono + Duration::from_secs(4),
            )
            .start
            .unwrap();
        views[0].live_scan.stream_started(restored.provider, false);
        views[0]
            .live_scan
            .set_source_mode(crate::live_scan::SourceMode::ProgressiveRadials);
        session.finish_into(
            0,
            completed.generation,
            true,
            Some("late completed end".into()),
            &mut views,
        );
        assert!(session.accepts(0, restored.generation));
        assert!(views[0].live_scan.last_stream_error.is_none());
        assert!(!views[0]
            .live_scan
            .accept_volume("older-primary-control", clock, restored_clock));
        assert_eq!(views[0].live_scan.volume_time, Some(floor_clock));
        assert!(views[0].live_scan.accept_volume(
            "restored-control",
            restored_clock,
            restored_clock
        ));
        assert_eq!(receipt.inventory(), &frozen);
        events.push(serde_json::json!({"stage":"preferred restored", "selected":label_for_tier(providers.selected_tier()), "reason":label_for_reason(providers.snapshot().last_transition.unwrap().1), "generation":restored.generation, "source_time_ms":views[0].live_scan.volume_time.unwrap().timestamp_millis(), "frozen_receipt_preserved":true, "older_update_refused":true}));
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m1.3/restoration/fault-transitions.json");
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(destination, serde_json::to_vec_pretty(&events).unwrap()).unwrap();
    }
}
