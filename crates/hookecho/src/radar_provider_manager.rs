//! ROADMAP_NEW B6.11 step 11: wires the previously dormant B6 pieces — [`crate::failover_arbiter`],
//! [`crate::provider_health`], [`crate::relay_provider`] and [`crate::tgftp_provider`] — into one
//! thing a call site can actually use: "which [`Level2LiveProvider`] should I subscribe with right
//! now for this site, and what should the health popup say about it?"
//!
//! Native only, like `relay_provider`/`provider_health`: the second progressive path
//! (`HookEchoRelayLevel2Provider`) is itself native-only, and a dual-feed comparison is only
//! meaningful once a second provider exists to compare against.
//!
//! Three tiers, not the two-sided arbiter's raw [`ActiveSide`]:
//!
//! - **Primary**: the existing Unidata/AWS chunk feed. Always present.
//! - **Backup**: [`HookEchoRelayLevel2Provider`], only once the user has configured a relay URL
//!   (ROADMAP_NEW's own "opt-in ... never the default" for that provider).
//! - **Degraded**: [`NoaaTgftpLevel2Provider`] (B6.8), selected only once whichever side the
//!   arbiter currently prefers has *itself* gone stale past [`DEGRADED_AFTER`] — the roadmap's own
//!   topology diagram places TGFTP behind *both* progressive paths as the universal last resort,
//!   not a candidate the two-sided arbiter would ever choose on its own. This falls out naturally
//!   even with no relay configured at all: a lone, stalled Unidata feed still degrades to TGFTP
//!   rather than leaving the pane stuck.
//!
//! [`SiteProviders`] owns one [`SiteArbiter`] and one dual-feed [`HealthBoard`] for one radar site;
//! a caller keeps one instance per pane for as long as that pane is following a NEXRAD site live,
//! same lifetime discipline the live stream itself already follows.

use crate::failover_arbiter::{ActiveSide, ArbiterConfig, ArbiterInput, SiteArbiter, Transition};
use crate::provider_health::{spawn_dual_feed_monitor, HealthBoard, ProviderHealth};
use crate::relay_provider::HookEchoRelayLevel2Provider;
use crate::tgftp_provider::NoaaTgftpLevel2Provider;
use crate::volume::{Level2LiveProvider, UnidataLevel2Provider};
use chrono::{DateTime, Duration, Utc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use wxdata::level2::Scan;
use wxdata::live_block::ProviderSwitchReason;

/// [`ProviderHealth`]/[`HealthBoard`] key for the Unidata path — matches
/// `UnidataLevel2Provider::label`.
pub const PRIMARY_LABEL: &str = "Unidata Level II (AWS S3)";
/// [`ProviderHealth`]/[`HealthBoard`] key for the relay path — matches
/// `HookEchoRelayLevel2Provider::label`.
pub const BACKUP_LABEL: &str = "HookEcho Relay";
/// Matches `NoaaTgftpLevel2Provider::label`. Not itself a [`HealthBoard`] key: TGFTP is never
/// dual-fed/monitored the way primary/backup are, it is only ever selected outright.
pub const DEGRADED_LABEL: &str = "NOAA TGFTP (degraded)";

/// How stale the arbiter's own active side must get before this manager gives up on both
/// progressive paths and drops to the completed-volume TGFTP fallback. Deliberately looser than
/// [`ArbiterConfig::staleness_threshold`] (90s default): that threshold only asks "should the
/// *other* progressive side take over," which requires that side to actually be fresher — dropping
/// all the way to a completed-volume-only source is a bigger step, worth a longer grace period
/// first. Not yet exposed as a setting — see ROADMAP_NEW B6.6's "thresholds should be configurable"
/// note; this is a starting point, not a claim about real scan cadence.
pub const DEGRADED_AFTER: Duration = Duration::seconds(180);

/// Which of the three tiers is currently the one to subscribe with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedTier {
    Primary,
    Backup,
    Degraded,
}

/// Stable label for [`SelectedTier`], for UI/diagnostics — mirrors each provider's own `label()`.
pub fn label_for_tier(tier: SelectedTier) -> &'static str {
    match tier {
        SelectedTier::Primary => PRIMARY_LABEL,
        SelectedTier::Backup => BACKUP_LABEL,
        SelectedTier::Degraded => DEGRADED_LABEL,
    }
}

/// A snapshot of the manager's current decision — cheap to compute on demand for the health popup
/// and the diagnostics bundle, so nothing needs to cache or invalidate one.
#[derive(Debug, Clone)]
pub struct FailoverSnapshot {
    pub selected: SelectedTier,
    /// Whether a relay URL is configured at all — without one, "Backup" never has anything to
    /// report beyond "not configured."
    pub has_backup: bool,
    pub primary: Option<ProviderHealth>,
    pub backup: Option<ProviderHealth>,
    pub manual_override: bool,
    pub last_transition: Option<(DateTime<Utc>, ProviderSwitchReason, SelectedTier)>,
}

impl FailoverSnapshot {
    /// Every configured tier other than the one currently selected, in failover preference order.
    /// TGFTP is always available natively; the relay appears only when configured. When TGFTP is
    /// active, the progressive providers are recovery candidates rather than "fallbacks" in the
    /// chronological sense, but they are still the alternate providers source health must name.
    pub fn alternate_provider_labels(&self) -> Vec<&'static str> {
        match self.selected {
            SelectedTier::Primary => {
                let mut labels = Vec::with_capacity(2);
                if self.has_backup {
                    labels.push(BACKUP_LABEL);
                }
                labels.push(DEGRADED_LABEL);
                labels
            }
            SelectedTier::Backup => vec![PRIMARY_LABEL, DEGRADED_LABEL],
            SelectedTier::Degraded => {
                let mut labels = vec![PRIMARY_LABEL];
                if self.has_backup {
                    labels.push(BACKUP_LABEL);
                }
                labels
            }
        }
    }
}

/// One radar site's dual-feed health plus the failover decision built on top of it. See the module
/// doc comment for the three-tier model.
pub struct SiteProviders {
    site: String,
    relay_url: Option<String>,
    arbiter: SiteArbiter,
    board: HealthBoard,
    active_flag: Arc<AtomicBool>,
    degraded: bool,
    manual_override: Option<SelectedTier>,
    last_transition: Option<(DateTime<Utc>, ProviderSwitchReason, SelectedTier)>,
}

impl SiteProviders {
    /// Start monitoring `site`. `relay_url` is `None` when no self-hosted relay is configured —
    /// the manager still works, degrading a lone stalled Unidata feed to TGFTP rather than doing
    /// nothing. `base` seeds both monitored providers' merge state, same as the real live stream.
    pub fn start(
        spawner: &crate::rt::Spawner,
        site: String,
        relay_url: Option<String>,
        base: Arc<Scan>,
    ) -> Self {
        let mut providers: Vec<Arc<dyn Level2LiveProvider + Send + Sync>> =
            vec![Arc::new(UnidataLevel2Provider)];
        if let Some(url) = &relay_url {
            providers.push(Arc::new(HookEchoRelayLevel2Provider::new(url.clone())));
        }
        let active_flag = Arc::new(AtomicBool::new(true));
        let active: Arc<dyn Fn() -> bool + Send + Sync> = {
            let flag = active_flag.clone();
            Arc::new(move || flag.load(Ordering::Relaxed))
        };
        let board = spawn_dual_feed_monitor(spawner, site.clone(), providers, base, active);
        Self {
            site,
            relay_url,
            arbiter: SiteArbiter::new(ArbiterConfig::default()),
            board,
            active_flag,
            degraded: false,
            manual_override: None,
            last_transition: None,
        }
    }

    pub fn site(&self) -> &str {
        &self.site
    }

    pub fn relay_url(&self) -> Option<&str> {
        self.relay_url.as_deref()
    }

    fn health(&self, label: &str) -> Option<ProviderHealth> {
        self.board
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(label)
            .cloned()
    }

    /// Force `tier` active until [`Self::clear_manual_override`] — ROADMAP_NEW B6.1/B6.9's Advanced
    /// settings override. Unlike [`SiteArbiter::set_manual_override`] (binary), this also allows
    /// forcing the degraded TGFTP tier, since that choice lives above the two-sided arbiter here.
    pub fn set_manual_override(&mut self, tier: SelectedTier) {
        if self.manual_override == Some(tier) {
            return; // idempotent: no spurious transition on every settings-sync tick
        }
        self.manual_override = Some(tier);
        self.last_transition = Some((Utc::now(), ProviderSwitchReason::ManualOverride, tier));
    }

    pub fn clear_manual_override(&mut self) {
        self.manual_override = None;
    }

    pub fn is_manually_overridden(&self) -> bool {
        self.manual_override.is_some()
    }

    /// Re-evaluate the arbiter/degraded state from the current health board. Call this
    /// periodically (once per frame is plenty — it's a couple of `HashMap` lookups and pure
    /// arithmetic, no I/O). A no-op while a manual override is set.
    pub fn tick(&mut self) -> Option<Transition> {
        if self.manual_override.is_some() {
            return None;
        }
        let now = Utc::now();
        let primary = self.health(PRIMARY_LABEL);
        let backup = self.health(BACKUP_LABEL);
        let primary_input = to_arbiter_input(&primary);
        let backup_input = to_arbiter_input(&backup);

        let transition = self.arbiter.evaluate(now, primary_input, backup_input);
        if let Some(t) = transition {
            let tier = match t.to {
                ActiveSide::Primary => SelectedTier::Primary,
                ActiveSide::Backup => SelectedTier::Backup,
            };
            self.last_transition = Some((now, t.reason, tier));
        }

        let active_input = match self.arbiter.active() {
            ActiveSide::Primary => &primary_input,
            ActiveSide::Backup => &backup_input,
        };
        let now_degraded = is_older_than(active_input.newest_radar_time, now, DEGRADED_AFTER);
        if now_degraded != self.degraded {
            self.degraded = now_degraded;
            let tier = if now_degraded {
                SelectedTier::Degraded
            } else {
                match self.arbiter.active() {
                    ActiveSide::Primary => SelectedTier::Primary,
                    ActiveSide::Backup => SelectedTier::Backup,
                }
            };
            let reason = if now_degraded {
                ProviderSwitchReason::StaleData
            } else {
                ProviderSwitchReason::Recovery
            };
            self.last_transition = Some((now, reason, tier));
        }
        transition
    }

    pub fn selected_tier(&self) -> SelectedTier {
        if let Some(tier) = self.manual_override {
            return tier;
        }
        if self.degraded {
            return SelectedTier::Degraded;
        }
        match self.arbiter.active() {
            ActiveSide::Primary => SelectedTier::Primary,
            ActiveSide::Backup => SelectedTier::Backup,
        }
    }

    /// A freshly constructed provider for whichever tier is currently selected — cheap (each is a
    /// thin struct/string clone), so callers ask again each time they actually need to subscribe
    /// rather than holding one across a tier change.
    pub fn provider(&self) -> Arc<dyn Level2LiveProvider + Send + Sync> {
        match self.selected_tier() {
            SelectedTier::Primary => Arc::new(UnidataLevel2Provider),
            SelectedTier::Backup => Arc::new(HookEchoRelayLevel2Provider::new(
                self.relay_url.clone().unwrap_or_default(),
            )),
            SelectedTier::Degraded => Arc::new(NoaaTgftpLevel2Provider::new()),
        }
    }

    pub fn snapshot(&self) -> FailoverSnapshot {
        FailoverSnapshot {
            selected: self.selected_tier(),
            has_backup: self.relay_url.is_some(),
            primary: self.health(PRIMARY_LABEL),
            backup: self.health(BACKUP_LABEL),
            manual_override: self.manual_override.is_some(),
            last_transition: self.last_transition,
        }
    }

    /// Test-only constructor: builds the decision state around a caller-supplied [`HealthBoard`]
    /// instead of spawning real monitor tasks, so `tick()`/`selected_tier()` are exercisable with
    /// deterministic, network-free fixtures.
    #[cfg(test)]
    fn for_test(relay_url: Option<String>, board: HealthBoard) -> Self {
        Self {
            site: "KTLX".to_string(),
            relay_url,
            arbiter: SiteArbiter::new(ArbiterConfig::default()),
            board,
            active_flag: Arc::new(AtomicBool::new(false)),
            degraded: false,
            manual_override: None,
            last_transition: None,
        }
    }
}

impl Drop for SiteProviders {
    fn drop(&mut self) {
        // Stops every `monitor_provider` task spawned in `start()` — see `Level2LiveProvider::
        // subscribe`'s own `active` doc comment for why this is enough (each loop checks it and
        // returns; nothing here waits for that to happen, matching every other live subscription's
        // fire-and-forget cancellation in this codebase).
        self.active_flag.store(false, Ordering::Relaxed);
    }
}

fn to_arbiter_input(health: &Option<ProviderHealth>) -> ArbiterInput {
    match health {
        None => ArbiterInput::default(),
        Some(h) => ArbiterInput {
            newest_radar_time: h.newest_radar_time,
            consecutive_failures: h.consecutive_failures,
        },
    }
}

fn is_older_than(t: Option<DateTime<Utc>>, now: DateTime<Utc>, threshold: Duration) -> bool {
    match t {
        None => true,
        Some(t) => now.signed_duration_since(t) > threshold,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn board_with(entries: Vec<(&'static str, ProviderHealth)>) -> HealthBoard {
        Arc::new(Mutex::new(entries.into_iter().collect::<HashMap<_, _>>()))
    }

    fn health(newest: DateTime<Utc>, consecutive_failures: u32) -> ProviderHealth {
        ProviderHealth {
            label: "test",
            capabilities: wxdata::live_block::ProviderCapabilities::unidata(),
            newest_radar_time: Some(newest),
            last_receipt_at: Some(newest),
            successes: 1,
            failures: 0,
            consecutive_failures,
            reconnects: 0,
            last_error: None,
        }
    }

    #[test]
    fn alternate_provider_labels_follow_the_selected_tier_and_relay_configuration() {
        let snapshot = |selected, has_backup| FailoverSnapshot {
            selected,
            has_backup,
            primary: None,
            backup: None,
            manual_override: false,
            last_transition: None,
        };
        assert_eq!(
            snapshot(SelectedTier::Primary, false).alternate_provider_labels(),
            [DEGRADED_LABEL]
        );
        assert_eq!(
            snapshot(SelectedTier::Primary, true).alternate_provider_labels(),
            [BACKUP_LABEL, DEGRADED_LABEL]
        );
        assert_eq!(
            snapshot(SelectedTier::Backup, true).alternate_provider_labels(),
            [PRIMARY_LABEL, DEGRADED_LABEL]
        );
        assert_eq!(
            snapshot(SelectedTier::Degraded, true).alternate_provider_labels(),
            [PRIMARY_LABEL, BACKUP_LABEL]
        );
    }

    #[test]
    fn with_no_relay_configured_a_healthy_primary_stays_selected() {
        let now = Utc::now();
        let board = board_with(vec![(PRIMARY_LABEL, health(now, 0))]);
        let mut sp = SiteProviders::for_test(None, board);
        sp.tick();
        assert_eq!(sp.selected_tier(), SelectedTier::Primary);
        assert!(!sp.snapshot().has_backup);
    }

    #[test]
    fn a_stalled_primary_with_no_backup_degrades_to_tgftp() {
        let now = Utc::now();
        let stale = now - Duration::seconds(400);
        let board = board_with(vec![(PRIMARY_LABEL, health(stale, 0))]);
        let mut sp = SiteProviders::for_test(None, board);
        sp.tick();
        assert_eq!(
            sp.selected_tier(),
            SelectedTier::Degraded,
            "no backup exists to fail over to, so a long-stalled primary alone must still degrade"
        );
        assert_eq!(label_for_tier(sp.selected_tier()), DEGRADED_LABEL);
    }

    #[test]
    fn a_fresh_backup_takes_over_before_degrading_when_primary_fails() {
        let now = Utc::now();
        let board = board_with(vec![
            (
                PRIMARY_LABEL,
                health(now - Duration::seconds(400), 5), // failing and stale
            ),
            (BACKUP_LABEL, health(now, 0)), // fresh
        ]);
        let mut sp = SiteProviders::for_test(Some("http://relay.local".to_string()), board);
        sp.tick();
        assert_eq!(sp.selected_tier(), SelectedTier::Backup);
    }

    #[test]
    fn both_sides_stalling_degrades_even_with_a_backup_configured() {
        let now = Utc::now();
        let long_stale = now - Duration::seconds(400);
        let board = board_with(vec![
            (PRIMARY_LABEL, health(long_stale, 5)),
            (BACKUP_LABEL, health(long_stale, 0)),
        ]);
        let mut sp = SiteProviders::for_test(Some("http://relay.local".to_string()), board);
        // First tick: arbiter can't switch to backup (not fresher than primary), so primary stays
        // active, and it's the one that's long-stale -> degraded.
        sp.tick();
        assert_eq!(sp.selected_tier(), SelectedTier::Degraded);
    }

    #[test]
    fn manual_override_holds_regardless_of_health() {
        let now = Utc::now();
        let board = board_with(vec![(PRIMARY_LABEL, health(now, 0))]);
        let mut sp = SiteProviders::for_test(None, board);
        sp.set_manual_override(SelectedTier::Degraded);
        sp.tick(); // must be a no-op while overridden
        assert_eq!(sp.selected_tier(), SelectedTier::Degraded);
        assert!(sp.is_manually_overridden());
        assert!(sp.snapshot().manual_override);

        sp.clear_manual_override();
        sp.tick();
        assert_eq!(
            sp.selected_tier(),
            SelectedTier::Primary,
            "clearing the override resumes automatic evaluation against healthy data"
        );
    }

    #[test]
    fn setting_the_same_override_twice_does_not_record_a_new_transition() {
        let board = board_with(vec![]);
        let mut sp = SiteProviders::for_test(None, board);
        sp.set_manual_override(SelectedTier::Backup);
        let first = sp.snapshot().last_transition;
        sp.set_manual_override(SelectedTier::Backup);
        let second = sp.snapshot().last_transition;
        assert_eq!(
            first, second,
            "idempotent override must not spam transitions"
        );
    }

    #[test]
    fn provider_matches_the_selected_tier_labels() {
        let now = Utc::now();
        let board = board_with(vec![(PRIMARY_LABEL, health(now, 0))]);
        let sp = SiteProviders::for_test(None, board);
        assert_eq!(sp.provider().label(), PRIMARY_LABEL);
    }
}
