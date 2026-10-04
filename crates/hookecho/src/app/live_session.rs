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
}
