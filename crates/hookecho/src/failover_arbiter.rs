//! ROADMAP_NEW B6.11 step 8: per-site failover arbiter.
//!
//! [`SiteArbiter`] decides which of two providers ("primary" and "backup") is *active* — i.e.
//! whose data should be rendered — for one radar site. It is a pure decision-making state
//! machine: given a freshness/failure snapshot for each side at one instant
//! ([`SiteArbiter::evaluate`]), it says whether the active side should change and why. It runs no
//! network I/O, decodes nothing, and never touches a `Scan` — the actual switching of *which
//! `Level2LiveProvider`'s updates reach `MapView`* is the caller's job (not yet wired in; see
//! ROADMAP_NEW B6.11 step 11), same separation [`crate::provider_health`] keeps between observing
//! health and rendering it.
//!
//! Every rule below traces to a specific ROADMAP_NEW B6.6 bullet:
//!
//! - **Never use reachability alone as "healthy."** [`ArbiterInput`] only carries data freshness
//!   and a transport failure count — there is no "is it responding to a ping" signal to even
//!   consult.
//! - **Never fail over to an even older stream.** A backup is only worth switching to if it is
//!   demonstrably fresher than the primary by [`ArbiterConfig::min_freshness_margin`] — see
//!   [`is_fresher`]. This is also what keeps the visible newest-radar timestamp from ever moving
//!   backwards on a switch.
//! - **Hysteresis before failback.** [`ArbiterConfig::failback_consecutive_healthy`] requires
//!   multiple *consecutive* healthy primary observations, not just one, before switching back —
//!   any unhealthy observation resets the streak to zero, so a flapping primary cannot bounce the
//!   active side back and forth.
//! - **Explicit switch reason, always.** Every [`Transition`] carries a
//!   [`wxdata::live_block::ProviderSwitchReason`] — `TransportError`, `StaleData`,
//!   `ManualOverride`, or `Recovery`. `SequenceGap` is not produced by this arbiter: detecting a
//!   sequence gap needs the cross-provider radial identity comparison B6.5/B6.7 add, not just the
//!   per-provider freshness/failure counts this one reasons about.

use chrono::{DateTime, Duration, Utc};
use wxdata::live_block::ProviderSwitchReason;

/// The arbiter's only input for one provider at one instant — deliberately smaller than
/// [`crate::provider_health::ProviderHealth`]: the arbiter reasons about staleness and failure,
/// not the full diagnostic detail a human-facing health display wants.
#[derive(Debug, Clone, Copy, Default)]
pub struct ArbiterInput {
    pub newest_radar_time: Option<DateTime<Utc>>,
    /// Consecutive transport failures (connection errors, exhausted retries) since the last
    /// success — not a lifetime total, so a provider that recovers and later fails again is
    /// judged on its current run of failures, not ancient history.
    pub consecutive_failures: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveSide {
    Primary,
    Backup,
}

/// Tunable thresholds. A deployment/user configures these, not code — ROADMAP_NEW B6.6:
/// "Thresholds should be configurable and benchmarked against real scan cadence. Do not hard-code
/// a marketing latency target as the health rule." The defaults here are starting points, not a
/// claim about real scan cadence.
#[derive(Debug, Clone)]
pub struct ArbiterConfig {
    /// Consecutive transport failures on the active side before failover is even considered.
    pub max_consecutive_failures: u32,
    /// How old the active side's newest radar time may get (relative to the evaluation instant)
    /// before it counts as stale.
    pub staleness_threshold: Duration,
    /// The candidate side must be fresher than the current active side by at least this much to
    /// be worth switching to.
    pub min_freshness_margin: Duration,
    /// Consecutive healthy primary observations required, once on the backup, before failing back.
    pub failback_consecutive_healthy: u32,
}

impl Default for ArbiterConfig {
    fn default() -> Self {
        Self {
            max_consecutive_failures: 3,
            staleness_threshold: Duration::seconds(90),
            min_freshness_margin: Duration::seconds(5),
            failback_consecutive_healthy: 3,
        }
    }
}

/// One transition the arbiter has decided on, with its reason for provenance/diagnostics
/// (ROADMAP_NEW B6.6: "explicit switch reason ... recorded in provenance and diagnostics").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    pub to: ActiveSide,
    pub reason: ProviderSwitchReason,
}

/// One radar site's failover state. Call [`SiteArbiter::evaluate`] on every new health snapshot
/// for both sides (e.g. once per [`crate::provider_health::HealthBoard`] update); a real site
/// keeps one `SiteArbiter` for its lifetime, not one per evaluation.
pub struct SiteArbiter {
    config: ArbiterConfig,
    active: ActiveSide,
    /// Consecutive healthy primary observations since failing over to the backup, toward failback
    /// hysteresis. Reset to zero on any unhealthy primary observation or on failing back.
    primary_recovery_streak: u32,
    manual_override: Option<ActiveSide>,
}

impl SiteArbiter {
    pub fn new(config: ArbiterConfig) -> Self {
        Self {
            config,
            active: ActiveSide::Primary,
            primary_recovery_streak: 0,
            manual_override: None,
        }
    }

    pub fn active(&self) -> ActiveSide {
        self.active
    }

    pub fn is_manually_overridden(&self) -> bool {
        self.manual_override.is_some()
    }

    /// Force `side` active until [`SiteArbiter::clear_manual_override`] is called — ROADMAP_NEW
    /// B6.1's "preserve a manual provider override in Advanced settings for diagnostics." While a
    /// manual override is set, [`SiteArbiter::evaluate`] never changes the active side.
    pub fn set_manual_override(&mut self, side: ActiveSide) -> Option<Transition> {
        self.manual_override = Some(side);
        if self.active == side {
            return None;
        }
        self.active = side;
        self.primary_recovery_streak = 0;
        Some(Transition {
            to: side,
            reason: ProviderSwitchReason::ManualOverride,
        })
    }

    pub fn clear_manual_override(&mut self) {
        self.manual_override = None;
    }

    /// Evaluate one new snapshot for both sides, returning a [`Transition`] if the active side
    /// should change, or `None` if it should stay the same (including while a manual override is
    /// in effect).
    pub fn evaluate(
        &mut self,
        now: DateTime<Utc>,
        primary: ArbiterInput,
        backup: ArbiterInput,
    ) -> Option<Transition> {
        if self.manual_override.is_some() {
            return None;
        }

        match self.active {
            ActiveSide::Primary => {
                let transport_failed =
                    primary.consecutive_failures >= self.config.max_consecutive_failures;
                let stale = is_stale(
                    primary.newest_radar_time,
                    now,
                    self.config.staleness_threshold,
                );
                if !transport_failed && !stale {
                    return None;
                }
                // Never fail over to an even older (or equally uninformative) stream just because
                // it happens to be reachable right now.
                if !is_fresher(
                    backup.newest_radar_time,
                    primary.newest_radar_time,
                    self.config.min_freshness_margin,
                ) {
                    return None;
                }
                self.active = ActiveSide::Backup;
                self.primary_recovery_streak = 0;
                Some(Transition {
                    to: ActiveSide::Backup,
                    reason: if transport_failed {
                        ProviderSwitchReason::TransportError
                    } else {
                        ProviderSwitchReason::StaleData
                    },
                })
            }
            ActiveSide::Backup => {
                let primary_healthy = primary.consecutive_failures == 0
                    && !is_stale(
                        primary.newest_radar_time,
                        now,
                        self.config.staleness_threshold,
                    );
                if primary_healthy {
                    self.primary_recovery_streak += 1;
                } else {
                    self.primary_recovery_streak = 0;
                }
                if self.primary_recovery_streak < self.config.failback_consecutive_healthy {
                    return None;
                }
                self.active = ActiveSide::Primary;
                self.primary_recovery_streak = 0;
                Some(Transition {
                    to: ActiveSide::Primary,
                    reason: ProviderSwitchReason::Recovery,
                })
            }
        }
    }
}

fn is_stale(t: Option<DateTime<Utc>>, now: DateTime<Utc>, threshold: Duration) -> bool {
    match t {
        None => true,
        Some(t) => now.signed_duration_since(t) > threshold,
    }
}

/// Whether `candidate` is fresher than `current` by at least `margin` — `None` for `current`
/// (nothing has ever arrived) always loses to any real timestamp; `None` for `candidate` never
/// wins.
fn is_fresher(
    candidate: Option<DateTime<Utc>>,
    current: Option<DateTime<Utc>>,
    margin: Duration,
) -> bool {
    match (candidate, current) {
        (Some(c), Some(cur)) => c.signed_duration_since(cur) > margin,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(seconds_ago: i64, now: DateTime<Utc>) -> DateTime<Utc> {
        now - Duration::seconds(seconds_ago)
    }

    fn healthy(newest: DateTime<Utc>) -> ArbiterInput {
        ArbiterInput {
            newest_radar_time: Some(newest),
            consecutive_failures: 0,
        }
    }

    #[test]
    fn a_healthy_primary_never_triggers_a_switch() {
        let now = Utc::now();
        let mut arbiter = SiteArbiter::new(ArbiterConfig::default());
        let primary = healthy(t(1, now));
        let backup = healthy(t(1, now));
        assert!(arbiter.evaluate(now, primary, backup).is_none());
        assert_eq!(arbiter.active(), ActiveSide::Primary);
    }

    #[test]
    fn repeated_transport_failures_fail_over_to_a_fresher_backup() {
        let now = Utc::now();
        let mut arbiter = SiteArbiter::new(ArbiterConfig::default());
        let primary = ArbiterInput {
            newest_radar_time: Some(t(200, now)),
            consecutive_failures: 5,
        };
        let backup = healthy(t(1, now));
        let transition = arbiter.evaluate(now, primary, backup).unwrap();
        assert_eq!(transition.to, ActiveSide::Backup);
        assert_eq!(transition.reason, ProviderSwitchReason::TransportError);
        assert_eq!(arbiter.active(), ActiveSide::Backup);
    }

    #[test]
    fn stale_data_alone_without_transport_failure_still_fails_over_with_the_right_reason() {
        let now = Utc::now();
        let mut arbiter = SiteArbiter::new(ArbiterConfig::default());
        // No transport failures, but the primary's newest radar time is far outside the
        // staleness threshold (e.g. the connection is technically "up" but nothing new is coming
        // through it).
        let primary = ArbiterInput {
            newest_radar_time: Some(t(500, now)),
            consecutive_failures: 0,
        };
        let backup = healthy(t(1, now));
        let transition = arbiter.evaluate(now, primary, backup).unwrap();
        assert_eq!(transition.reason, ProviderSwitchReason::StaleData);
    }

    #[test]
    fn failure_never_fails_over_to_a_backup_that_is_not_actually_fresher() {
        let now = Utc::now();
        let mut arbiter = SiteArbiter::new(ArbiterConfig::default());
        let primary = ArbiterInput {
            newest_radar_time: Some(t(1, now)),
            consecutive_failures: 10,
        };
        // Backup is older than the primary's last known data — switching would move the visible
        // newest-radar timestamp backwards, which must never happen.
        let backup = healthy(t(60, now));
        assert!(
            arbiter.evaluate(now, primary, backup).is_none(),
            "must not fail over to an older stream even when the primary is failing"
        );
        assert_eq!(arbiter.active(), ActiveSide::Primary);
    }

    #[test]
    fn failure_with_no_backup_data_at_all_does_not_fail_over() {
        let now = Utc::now();
        let mut arbiter = SiteArbiter::new(ArbiterConfig::default());
        let primary = ArbiterInput {
            newest_radar_time: Some(t(1, now)),
            consecutive_failures: 10,
        };
        let backup = ArbiterInput {
            newest_radar_time: None,
            consecutive_failures: 0,
        };
        assert!(arbiter.evaluate(now, primary, backup).is_none());
    }

    #[test]
    fn failback_requires_consecutive_healthy_observations_not_just_one() {
        let now = Utc::now();
        let mut arbiter = SiteArbiter::new(ArbiterConfig::default());
        // Force onto the backup first.
        let failing_primary = ArbiterInput {
            newest_radar_time: Some(t(500, now)),
            consecutive_failures: 10,
        };
        arbiter
            .evaluate(now, failing_primary, healthy(t(1, now)))
            .unwrap();
        assert_eq!(arbiter.active(), ActiveSide::Backup);

        // Two healthy observations are not enough (default threshold is 3).
        for _ in 0..2 {
            let result = arbiter.evaluate(now, healthy(t(1, now)), healthy(t(1, now)));
            assert!(result.is_none());
            assert_eq!(arbiter.active(), ActiveSide::Backup);
        }
        // The third consecutive healthy observation triggers failback.
        let transition = arbiter
            .evaluate(now, healthy(t(1, now)), healthy(t(1, now)))
            .unwrap();
        assert_eq!(transition.to, ActiveSide::Primary);
        assert_eq!(transition.reason, ProviderSwitchReason::Recovery);
    }

    #[test]
    fn an_interrupted_recovery_streak_resets_and_does_not_fail_back_early() {
        let now = Utc::now();
        let mut arbiter = SiteArbiter::new(ArbiterConfig::default());
        let failing_primary = ArbiterInput {
            newest_radar_time: Some(t(500, now)),
            consecutive_failures: 10,
        };
        arbiter
            .evaluate(now, failing_primary, healthy(t(1, now)))
            .unwrap();

        // Two healthy observations, then one unhealthy one (a flapping primary), then two more
        // healthy ones — never three genuinely *consecutive* healthy observations.
        arbiter.evaluate(now, healthy(t(1, now)), healthy(t(1, now)));
        arbiter.evaluate(now, healthy(t(1, now)), healthy(t(1, now)));
        let flap = ArbiterInput {
            newest_radar_time: Some(t(1, now)),
            consecutive_failures: 1,
        };
        arbiter.evaluate(now, flap, healthy(t(1, now)));
        arbiter.evaluate(now, healthy(t(1, now)), healthy(t(1, now)));
        let still_backup = arbiter.evaluate(now, healthy(t(1, now)), healthy(t(1, now)));
        assert!(
            still_backup.is_none(),
            "a flapping primary must not accumulate a broken streak into a failback"
        );
        assert_eq!(arbiter.active(), ActiveSide::Backup);
    }

    #[test]
    fn manual_override_holds_regardless_of_health_until_cleared() {
        let now = Utc::now();
        let mut arbiter = SiteArbiter::new(ArbiterConfig::default());
        let transition = arbiter.set_manual_override(ActiveSide::Backup).unwrap();
        assert_eq!(transition.reason, ProviderSwitchReason::ManualOverride);

        // Even a perfectly healthy primary and a failing backup must not move the active side
        // while the override holds.
        let failing_backup = ArbiterInput {
            newest_radar_time: Some(t(500, now)),
            consecutive_failures: 10,
        };
        assert!(arbiter
            .evaluate(now, healthy(t(1, now)), failing_backup)
            .is_none());
        assert_eq!(arbiter.active(), ActiveSide::Backup);

        arbiter.clear_manual_override();
        // Automatic evaluation resumes: the backup is failing and stale, the primary is healthy
        // and fresher, so a normal failover-style decision (now from Backup toward Primary) can
        // proceed via the same hysteresis path as any other recovery.
        assert!(!arbiter.is_manually_overridden());
    }

    #[test]
    fn switching_never_moves_the_visible_newest_radar_time_backwards() {
        // Property check across a short scripted sequence of evaluations: after each `evaluate`,
        // whichever side is now active must have a newest_radar_time no older than what was
        // visible (the previously active side's newest_radar_time) just before that call.
        let now = Utc::now();
        let mut arbiter = SiteArbiter::new(ArbiterConfig::default());

        let steps: Vec<(ArbiterInput, ArbiterInput)> = vec![
            (
                ArbiterInput {
                    newest_radar_time: Some(t(1, now)),
                    consecutive_failures: 5,
                },
                healthy(t(2, now)), // backup is actually OLDER — must not switch
            ),
            (
                ArbiterInput {
                    newest_radar_time: Some(t(1, now)),
                    consecutive_failures: 5,
                },
                healthy(t(0, now)), // backup is fresher — may switch
            ),
        ];

        for (primary, backup) in steps {
            let visible_before = match arbiter.active() {
                ActiveSide::Primary => primary.newest_radar_time,
                ActiveSide::Backup => backup.newest_radar_time,
            };
            arbiter.evaluate(now, primary, backup);
            let visible_after = match arbiter.active() {
                ActiveSide::Primary => primary.newest_radar_time,
                ActiveSide::Backup => backup.newest_radar_time,
            };
            if let (Some(before), Some(after)) = (visible_before, visible_after) {
                assert!(
                    after >= before,
                    "the visible newest radar time must never move backwards on a switch"
                );
            }
        }
    }
}
