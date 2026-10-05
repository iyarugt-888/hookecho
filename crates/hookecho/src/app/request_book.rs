//! Request identity and source health: which lane each background fetch belongs to, whether its
//! reply is still the latest, and how each source is doing (the Layers rows, the Sources window
//! and the diagnostics bundle all read this). Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::{field_refresh_secs, FeedSource};
use chrono::{DateTime, Utc};
use wxdata::clock::Instant;

/// One independently-refreshing result lane.
///
/// A second request in the same lane makes the first one's eventual reply stale. Field layers
/// need separate lanes because they fetch concurrently; placefiles need one per URL; everything
/// else has one stable feed name. This is request identity only — the health UI builds on the
/// same book later rather than inventing a parallel tracker.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum RequestLane {
    Field(crate::render::FieldLayer),
    Placefile(String),
    Feed(FeedSource),
}

impl RequestLane {
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Field(layer) => format!("field {}", layer.slug()),
            Self::Placefile(source) => format!("Placefile {source}"),
            Self::Feed(source) => source.label().into(),
        }
    }

    pub(crate) fn cadence(&self) -> std::time::Duration {
        let secs = match self {
            Self::Field(layer) => field_refresh_secs(*layer),
            Self::Placefile(_) => 120,
            Self::Feed(source) => source.cadence_secs(),
        };
        std::time::Duration::from_secs(secs)
    }

    pub(crate) fn severity(&self) -> crate::source_health::Severity {
        match self {
            Self::Feed(source) => source.severity(),
            Self::Field(_) | Self::Placefile(_) => crate::source_health::Severity::Routine,
        }
    }

    pub(crate) fn endpoint_family(&self) -> crate::source_health::EndpointFamily {
        match self {
            Self::Field(layer) => crate::source_health::field_endpoint_family(*layer),
            Self::Placefile(_) => crate::source_health::EndpointFamily::UserConfigured,
            Self::Feed(source) => source.endpoint_family(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HealthState {
    Fetching,
    Fresh,
    /// ROADMAP_NEW N1's "Delayed": past the expected cadence but not by much — the gap between
    /// "still on schedule" and "genuinely stopped updating" `Fresh`/`Stale` used to jump straight
    /// across. See [`SourceHealth::state`] for the exact threshold and why.
    Delayed,
    Stale,
    /// The newest refresh failed, but a previously successful value is still resident and is
    /// what the map is showing. This is deliberately distinct from `Failed`, which means there
    /// is no usable value to fall back to.
    Cached,
    Failed,
    Waiting,
}

/// Whether the value represented by a source-health row is actually resident in the app.
///
/// This is intentionally small and truthful: HookEcho can currently prove that a decoded value
/// is in memory (including an uploaded field texture), or that no value is resident. It does not
/// pretend to know which upstream HTTP objects a browser/service-worker or OS cache may retain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheState {
    Empty,
    Memory,
}

impl CacheState {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Empty => "Not cached",
            Self::Memory => "In memory",
        }
    }

    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Memory => "memory",
        }
    }
}

/// Read-only request state copied into an enabled Layers row.
#[derive(Clone)]
pub(crate) struct SourceHealth {
    pub source: String,
    /// The shared upstream failure domain, separate from `source`'s layer-specific display name.
    pub endpoint_family: crate::source_health::EndpointFamily,
    /// Newest authoritative product/observation valid time seen for this source lane. This is
    /// deliberately separate from `last_success`, which is the local HTTP completion clock.
    pub latest_valid_time: Option<DateTime<Utc>>,
    /// Configured alternate providers for this source, empty when no runtime fallback exists.
    pub fallback_providers: Vec<String>,
    /// Proven residency of the value this source last delivered. A failed refresh only becomes
    /// [`HealthState::Cached`] when this says the prior value still exists.
    pub cache_state: CacheState,
    pub fetching: bool,
    pub last_attempt: Option<std::time::Duration>,
    pub last_success: Option<std::time::Duration>,
    pub last_failure: Option<std::time::Duration>,
    pub error: Option<String>,
    pub cadence: std::time::Duration,
    /// ROADMAP_NEW N1's "rolling success/failure count": `(successes, failures)` over the last
    /// `RequestBook::OUTCOME_WINDOW` finished requests. `None` for a source with no rolling
    /// tally to report — radar's own health is built from `MapView` fields directly (see
    /// `HookEchoApp::radar_health`), which track ingest lag and a live-stream retry count but not
    /// a request-outcome history the way `RequestBook`-tracked lanes do; showing `Some((0, 0))`
    /// there would read as "always healthy" rather than "not tracked".
    pub recent_outcomes: Option<(u32, u32)>,
    /// Extra labeled lines the popup shows verbatim, for a source with facts worth a permanent
    /// line of their own rather than a hover aside — radar's provider ingest lag and live-stream
    /// retry count are the only ones that currently set this. Empty for every other source.
    pub details: Vec<(&'static str, String)>,
    /// Whether this source failing is worth the red dot (`source_health::Severity`).
    pub severity: crate::source_health::Severity,
}

impl SourceHealth {
    /// A source that has missed its expected cadence by more than this multiplier is genuinely
    /// stale, not just running a little behind — a single slow poll or a source with naturally
    /// jittery timing (a feed that lands "every ~5 minutes" plus or minus) shouldn't flip straight
    /// to the same alarming state as one that has stopped updating outright.
    pub(crate) const DELAYED_CADENCE_MULTIPLIER: u32 = 2;

    pub(crate) fn state(&self) -> HealthState {
        if self.fetching {
            HealthState::Fetching
        } else if self
            .last_failure
            .is_some_and(|failed| self.last_success.is_none_or(|success| failed <= success))
        {
            if self.cache_state == CacheState::Memory {
                HealthState::Cached
            } else {
                HealthState::Failed
            }
        } else if self.last_success.is_some_and(|age| age <= self.cadence) {
            HealthState::Fresh
        } else if self
            .last_success
            .is_some_and(|age| age <= self.cadence * Self::DELAYED_CADENCE_MULTIPLIER)
        {
            HealthState::Delayed
        } else if self.last_success.is_some() {
            HealthState::Stale
        } else {
            HealthState::Waiting
        }
    }

    /// How this source recovers, as the code does it (ROADMAP_2 §3.4): the retry interval, when
    /// it reads as delayed and stale, and what happens to its last good data meanwhile.
    pub(crate) fn recovery(&self) -> String {
        use crate::ui::layers_panel::compact_age;
        let retention = match self.cache_state {
            CacheState::Memory => "If a refresh fails, the resident data remain available, marked Cached, until a refresh succeeds.",
            CacheState::Empty => "No usable data are resident for this request. Its value remains unavailable until delivery.",
        };
        format!(
            "Retried every {} (no backoff); delayed after {}, stale after {}. {retention} Severity: {}.",
            compact_age(self.cadence),
            compact_age(self.cadence),
            compact_age(self.cadence * Self::DELAYED_CADENCE_MULTIPLIER),
            self.severity.label()
        )
    }

    pub(crate) fn next_retry(&self) -> Option<std::time::Duration> {
        self.last_attempt
            .map(|age| self.cadence.saturating_sub(age))
    }
}

/// One [`SourceHealth`] row as it goes into ROADMAP_NEW N4's diagnostics bundle — durations become
/// plain seconds so the export needs no serde duration helper, and `status` is the same label the
/// Layers panel's own popup shows rather than the bare `HealthState` variant name.
#[derive(serde::Serialize)]
pub(crate) struct DiagnosticsSourceHealth {
    pub(crate) source: String,
    pub(crate) endpoint_family: &'static str,
    pub(crate) latest_valid_time: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) fallback_providers: Vec<String>,
    pub(crate) cache_state: &'static str,
    pub(crate) status: &'static str,
    pub(crate) last_success_secs: Option<u64>,
    pub(crate) cadence_secs: u64,
    /// ROADMAP_NEW N1's rolling success/failure count, `None` for a source that doesn't track
    /// one — see `SourceHealth.recent_outcomes`'s own doc comment.
    pub(crate) recent_successes: Option<u32>,
    pub(crate) recent_failures: Option<u32>,
    pub(crate) error: Option<String>,
    /// `SourceHealth.details` verbatim — for radar, this is where ROADMAP_NEW B6.9's active/
    /// standby provider, failover state and last-transition lines land once a
    /// `radar_provider_manager::SiteProviders` is running (see `registry.rs::failover_details`);
    /// every other source's own B3-style detail lines ride along the same way.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) details: Vec<(&'static str, String)>,
}

impl From<&SourceHealth> for DiagnosticsSourceHealth {
    fn from(h: &SourceHealth) -> Self {
        Self {
            source: h.source.clone(),
            endpoint_family: h.endpoint_family.id(),
            latest_valid_time: h.latest_valid_time.map(|t| t.to_rfc3339()),
            fallback_providers: h.fallback_providers.clone(),
            cache_state: h.cache_state.id(),
            status: crate::ui::layers_panel::health_look(h.state()).0,
            last_success_secs: h.last_success.map(|d| d.as_secs()),
            cadence_secs: h.cadence.as_secs(),
            recent_successes: h.recent_outcomes.map(|(s, _)| s),
            recent_failures: h.recent_outcomes.map(|(_, f)| f),
            error: h.error.clone(),
            details: h.details.clone(),
        }
    }
}

/// ROADMAP_NEW N4's local diagnostics bundle. See `HookEchoApp::export_diagnostics_bundle` for
/// what deliberately isn't in here.
#[derive(serde::Serialize)]
pub(crate) struct DiagnosticsBundle {
    pub(crate) generated_at: String,
    pub(crate) version: &'static str,
    pub(crate) platform: &'static str,
    pub(crate) renderer: String,
    pub(crate) cache_bytes: u64,
    pub(crate) performance_counters: std::collections::BTreeMap<&'static str, u64>,
    pub(crate) source_health: Vec<DiagnosticsSourceHealth>,
    pub(crate) recent_warnings: Vec<crate::devlog::LogEntry>,
}

pub(crate) struct RequestStatus {
    pub(crate) fetching: bool,
    pub(crate) last_attempt: Instant,
    pub(crate) last_success: Option<Instant>,
    pub(crate) last_failure: Option<(Instant, String)>,
    /// Newest data timestamp reported by a successful payload, never a local fetch timestamp.
    pub(crate) latest_valid_time: Option<DateTime<Utc>>,
    /// True only while a value from this lane is known to remain resident. Successful delivery
    /// sets it; explicit field-texture eviction clears it; a failed refresh preserves it.
    pub(crate) cache_resident: bool,
    pub(crate) cadence: std::time::Duration,
    /// ROADMAP_NEW N1's rolling success/failure count: the outcome of each finished request,
    /// oldest first, capped at `RequestBook::OUTCOME_WINDOW` — `true` a success, `false` a
    /// failure. A `VecDeque` rather than just two running totals so the window actually rolls
    /// (an old failure eventually ages out) instead of a failure from hours ago permanently
    /// dragging down a source that has since recovered.
    pub(crate) outcomes: std::collections::VecDeque<bool>,
}

/// Latest generation and fetch health in each result lane.
#[derive(Default)]
pub(crate) struct RequestBook {
    next: u64,
    latest: std::collections::HashMap<RequestLane, u64>,
    status: std::collections::HashMap<RequestLane, RequestStatus>,
}

impl RequestBook {
    /// How many recent finished requests `SourceHealth.recent_outcomes` reports over. Large
    /// enough to smooth over a single transient blip, small enough that a source's tally reflects
    /// its current behavior rather than its whole session history.
    pub(crate) const OUTCOME_WINDOW: usize = 20;

    /// A changed source selection starts its own health history. The generation counter stays
    /// monotonic, so an old reply cannot become current after a reset.
    pub(crate) fn reset(&mut self, lane: &RequestLane) {
        self.latest.remove(lane);
        self.status.remove(lane);
    }

    pub(crate) fn start(&mut self, lane: RequestLane) -> u64 {
        self.next = self.next.wrapping_add(1);
        self.latest.insert(lane.clone(), self.next);
        let now = Instant::now();
        let cadence = lane.cadence();
        self.status
            .entry(lane)
            .and_modify(|s| {
                s.fetching = true;
                s.last_attempt = now;
                s.cadence = cadence;
            })
            .or_insert(RequestStatus {
                fetching: true,
                last_attempt: now,
                last_success: None,
                last_failure: None,
                latest_valid_time: None,
                cache_resident: false,
                cadence,
                outcomes: std::collections::VecDeque::new(),
            });
        self.next
    }

    pub(crate) fn is_current(&self, lane: &RequestLane, generation: u64) -> bool {
        self.latest.get(lane) == Some(&generation)
    }

    /// Retire an answer to a superseded selection without recording a source success/failure.
    /// An older generation cannot cancel a newer request or alter its health.
    pub(crate) fn discard(&mut self, lane: &RequestLane, generation: u64) {
        if self.is_current(lane, generation) {
            self.latest.remove(lane);
            if let Some(status) = self.status.get_mut(lane) {
                status.fetching = false;
            }
        }
    }

    /// Finish only the newest generation. An old failure cannot poison a newer success.
    pub(crate) fn finish(
        &mut self,
        lane: &RequestLane,
        generation: u64,
        error: Option<&str>,
        valid_time: Option<DateTime<Utc>>,
    ) -> bool {
        if !self.is_current(lane, generation) {
            return false;
        }
        if let Some(s) = self.status.get_mut(lane) {
            s.fetching = false;
            match error {
                Some(e) => s.last_failure = Some((Instant::now(), e.to_string())),
                None => {
                    s.last_success = Some(Instant::now());
                    s.cache_resident = true;
                    if let Some(valid) = valid_time {
                        s.latest_valid_time =
                            Some(s.latest_valid_time.map_or(valid, |old| old.max(valid)));
                    }
                }
            }
            s.outcomes.push_back(error.is_none());
            if s.outcomes.len() > Self::OUTCOME_WINDOW {
                s.outcomes.pop_front();
            }
        }
        true
    }

    /// Keep health cache state synchronized with the renderer's explicit residency changes.
    /// Missing lanes stay missing: an eviction before a source has ever requested data should not
    /// fabricate a health-history row.
    pub(crate) fn set_cache_resident(&mut self, lane: &RequestLane, resident: bool) {
        if let Some(status) = self.status.get_mut(lane) {
            status.cache_resident = resident;
        }
    }

    pub(crate) fn health(&self, lane: &RequestLane) -> SourceHealth {
        let now = Instant::now();
        let Some(s) = self.status.get(lane) else {
            return SourceHealth {
                source: lane.label(),
                endpoint_family: lane.endpoint_family(),
                latest_valid_time: None,
                fallback_providers: Vec::new(),
                cache_state: CacheState::Empty,
                fetching: false,
                last_attempt: None,
                last_success: None,
                last_failure: None,
                error: None,
                cadence: lane.cadence(),
                recent_outcomes: None,
                details: Vec::new(),
                severity: lane.severity(),
            };
        };
        let recent_outcomes = (!s.outcomes.is_empty()).then(|| {
            let successes = s.outcomes.iter().filter(|ok| **ok).count() as u32;
            (successes, s.outcomes.len() as u32 - successes)
        });
        SourceHealth {
            source: lane.label(),
            endpoint_family: lane.endpoint_family(),
            latest_valid_time: s.latest_valid_time,
            fallback_providers: Vec::new(),
            cache_state: if s.cache_resident {
                CacheState::Memory
            } else {
                CacheState::Empty
            },
            fetching: s.fetching,
            last_attempt: Some(now.saturating_duration_since(s.last_attempt)),
            last_success: s.last_success.map(|t| now.saturating_duration_since(t)),
            last_failure: s
                .last_failure
                .as_ref()
                .map(|(t, _)| now.saturating_duration_since(*t)),
            error: s.last_failure.as_ref().map(|(_, e)| e.clone()),
            cadence: s.cadence,
            recent_outcomes,
            details: Vec::new(),
            severity: lane.severity(),
        }
    }
}

#[cfg(test)]
mod request_book_tests {
    use super::{CacheState, HealthState, RequestBook, RequestLane, SourceHealth};
    use crate::render::FieldLayer;

    #[test]
    fn superseded_selection_does_not_count_as_a_source_success_or_failure() {
        let mut book = RequestBook::default();
        let lane = RequestLane::Feed(crate::source_health::FeedSource::DerivedRadarFields);
        let first = book.start(lane.clone());
        book.discard(&lane, first);
        let health = book.health(&lane);
        assert!(!health.fetching);
        assert_eq!(health.recent_outcomes, None);
        assert_eq!(health.last_success, None);
        assert_eq!(health.last_failure, None);
        assert!(!book.finish(&lane, first, None, None));
        let old = book.start(lane.clone());
        let next = book.start(lane.clone());
        book.discard(&lane, old);
        assert!(book.health(&lane).fetching);
        assert!(book.finish(&lane, next, None, None));
        let before = book.health(&lane);
        let redundant = book.start(lane.clone());
        book.discard(&lane, redundant);
        let after = book.health(&lane);
        assert_eq!(after.recent_outcomes, before.recent_outcomes);
        assert_eq!(after.latest_valid_time, before.latest_valid_time);
        assert_eq!(after.cache_state, before.cache_state);
        assert!(after.last_success.is_some());
        assert!(after.last_failure.is_none());
    }

    #[test]
    fn only_the_latest_request_in_each_lane_is_current() {
        let mut book = RequestBook::default();
        let cape = RequestLane::Field(FieldLayer::Cape);
        let srh = RequestLane::Field(FieldLayer::Srh);
        let old_cape = book.start(cape.clone());
        let current_srh = book.start(srh.clone());
        let current_cape = book.start(cape.clone());

        // A late success or failure has the same identity check: neither may mutate state.
        assert!(!book.is_current(&cape, old_cape));
        assert!(book.is_current(&cape, current_cape));
        assert!(book.is_current(&srh, current_srh));
        assert!(book.finish(&cape, current_cape, None, None));
        assert!(!book.finish(&cape, old_cape, Some("old failure"), None));
        let health = book.health(&cape);
        assert_eq!(health.state(), HealthState::Fresh);
        assert!(health.error.is_none());
        // The rejected late failure must not count — only the one request that actually mutated
        // state does.
        assert_eq!(health.recent_outcomes, Some((1, 0)));
        assert_eq!(book.health(&srh).state(), HealthState::Fetching);
    }

    #[test]
    fn recent_outcomes_is_none_until_a_request_has_finished() {
        let mut book = RequestBook::default();
        let lane = RequestLane::Field(FieldLayer::Cape);
        assert_eq!(
            book.health(&lane).recent_outcomes,
            None,
            "no lane started yet"
        );
        let gen = book.start(lane.clone());
        assert_eq!(
            book.health(&lane).recent_outcomes,
            None,
            "started but not finished"
        );
        book.finish(&lane, gen, None, None);
        assert_eq!(book.health(&lane).recent_outcomes, Some((1, 0)));
    }

    #[test]
    fn recent_outcomes_rolls_off_the_oldest_result_past_the_window() {
        let mut book = RequestBook::default();
        let lane = RequestLane::Field(FieldLayer::Cape);
        // Fill the window with failures, then succeed enough times to push every failure out.
        for _ in 0..RequestBook::OUTCOME_WINDOW {
            let gen = book.start(lane.clone());
            book.finish(&lane, gen, Some("down"), None);
        }
        assert_eq!(
            book.health(&lane).recent_outcomes,
            Some((0, RequestBook::OUTCOME_WINDOW as u32)),
            "window full of failures"
        );
        for _ in 0..RequestBook::OUTCOME_WINDOW {
            let gen = book.start(lane.clone());
            book.finish(&lane, gen, None, None);
        }
        assert_eq!(
            book.health(&lane).recent_outcomes,
            Some((RequestBook::OUTCOME_WINDOW as u32, 0)),
            "every failure has aged out of the window, not just been outnumbered"
        );
    }

    #[test]
    fn latest_valid_time_never_regresses_and_survives_a_failure() {
        use chrono::{TimeZone, Utc};

        let mut book = RequestBook::default();
        let lane = RequestLane::Field(FieldLayer::Cape);
        let older = Utc.with_ymd_and_hms(2026, 9, 19, 11, 0, 0).unwrap();
        let newer = Utc.with_ymd_and_hms(2026, 9, 19, 12, 0, 0).unwrap();
        for valid in [newer, older] {
            let generation = book.start(lane.clone());
            assert!(book.finish(&lane, generation, None, Some(valid)));
        }
        let generation = book.start(lane.clone());
        assert!(book.finish(&lane, generation, Some("temporary outage"), None));
        assert_eq!(book.health(&lane).latest_valid_time, Some(newer));
    }

    #[test]
    fn failed_refresh_is_cached_only_while_the_prior_value_is_resident() {
        let mut book = RequestBook::default();
        let lane = RequestLane::Field(FieldLayer::Cape);

        let first = book.start(lane.clone());
        assert!(book.finish(&lane, first, Some("offline"), None));
        assert_eq!(book.health(&lane).state(), HealthState::Failed);
        assert_eq!(book.health(&lane).cache_state, CacheState::Empty);

        let success = book.start(lane.clone());
        assert!(book.finish(&lane, success, None, None));
        assert_eq!(book.health(&lane).cache_state, CacheState::Memory);

        let refresh = book.start(lane.clone());
        assert!(book.finish(&lane, refresh, Some("temporary outage"), None));
        assert_eq!(book.health(&lane).state(), HealthState::Cached);

        book.set_cache_resident(&lane, false);
        assert_eq!(book.health(&lane).cache_state, CacheState::Empty);
        assert_eq!(book.health(&lane).state(), HealthState::Failed);
    }

    #[test]
    fn health_classifies_every_visible_state() {
        let cadence = std::time::Duration::from_secs(60);
        let health = |fetching: bool,
                      success: Option<u64>,
                      failure: Option<u64>,
                      error: Option<String>,
                      cache_state: CacheState| SourceHealth {
            source: "test".into(),
            endpoint_family: crate::source_health::EndpointFamily::LocalProcessing,
            latest_valid_time: None,
            fallback_providers: Vec::new(),
            cache_state,
            fetching,
            last_attempt: Some(std::time::Duration::from_secs(1)),
            last_success: success.map(std::time::Duration::from_secs),
            last_failure: failure.map(std::time::Duration::from_secs),
            error,
            cadence,
            recent_outcomes: None,
            details: Vec::new(),
            severity: Default::default(),
        };
        assert_eq!(
            health(true, None, None, None, CacheState::Empty).state(),
            HealthState::Fetching
        );
        assert_eq!(
            health(false, Some(5), None, None, CacheState::Memory).state(),
            HealthState::Fresh
        );
        // Past the 60 s cadence but within the 2x-cadence grace window: Delayed, not yet Stale.
        assert_eq!(
            health(false, Some(61), None, None, CacheState::Memory).state(),
            HealthState::Delayed
        );
        assert_eq!(
            health(false, Some(120), None, None, CacheState::Memory).state(),
            HealthState::Delayed,
            "exactly at the 2x boundary is still Delayed, not Stale"
        );
        assert_eq!(
            health(false, Some(121), None, None, CacheState::Memory).state(),
            HealthState::Stale,
            "past 2x cadence is genuinely stale"
        );
        assert_eq!(
            health(
                false,
                Some(20),
                Some(5),
                Some("offline".into()),
                CacheState::Empty,
            )
            .state(),
            HealthState::Failed
        );
        assert_eq!(
            health(
                false,
                Some(20),
                Some(5),
                Some("offline".into()),
                CacheState::Memory,
            )
            .state(),
            HealthState::Cached
        );
        assert_eq!(
            health(false, None, None, None, CacheState::Empty).state(),
            HealthState::Waiting
        );
    }
}
