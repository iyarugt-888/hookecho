//! One look for how current data is (ROADMAP_2 §9.3): fresh, aging, stale, unavailable. Every
//! surface that colours a source, a layer or the radar by its age takes the colour from here, so a
//! stale feed reads the same in the Layers list, the Sources window, the app bar and the timeline,
//! instead of each inventing its own amber.

use crate::app::HealthState;
use crate::live_scan::Phase;
use egui::Color32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Freshness {
    /// On schedule.
    Fresh,
    /// Behind its expected cadence, or recovering, but not yet given up on.
    Aging,
    /// Past the point where it can be read as current; what is shown is old.
    Stale,
    /// Nothing usable: the feed failed with nothing to fall back to, or is offline.
    Unavailable,
}

impl Freshness {
    pub(crate) fn color(self) -> Color32 {
        match self {
            Self::Fresh => Color32::from_rgb(70, 200, 120),
            // A paler, less saturated amber than Stale's: behind schedule, not yet alarming.
            Self::Aging => Color32::from_rgb(220, 200, 100),
            Self::Stale => Color32::from_rgb(235, 180, 70),
            Self::Unavailable => Color32::from_rgb(230, 90, 90),
        }
    }

    /// A source-health state's class. `None` for the states that say nothing about age:
    /// a first fetch still in flight, or a source not yet asked for.
    pub(crate) fn of_health(state: HealthState) -> Option<Self> {
        match state {
            HealthState::Fresh => Some(Self::Fresh),
            HealthState::Delayed => Some(Self::Aging),
            // A refresh failed and the last good value is shown: it is no longer being kept
            // current, so it reads as stale; its own glyph and word say why.
            HealthState::Stale | HealthState::Cached => Some(Self::Stale),
            HealthState::Failed => Some(Self::Unavailable),
            HealthState::Fetching | HealthState::Waiting => None,
        }
    }

    /// The live radar's class, from its scan phase. Acquiring and completing scans are fresh;
    /// a fallback provider or a reconnect is aging, since data still arrives but later.
    pub(crate) fn of_scan(phase: Phase) -> Self {
        match phase {
            Phase::AwaitingVolume
            | Phase::AcquiringSweep
            | Phase::SweepComplete
            | Phase::VolumeComplete => Self::Fresh,
            Phase::Recovering | Phase::FallbackSource | Phase::Aging => Self::Aging,
            Phase::Stale => Self::Stale,
            Phase::Offline => Self::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_classes_four_colours_ordered_by_alarm() {
        let all = [
            Freshness::Fresh,
            Freshness::Aging,
            Freshness::Stale,
            Freshness::Unavailable,
        ];
        let colours: std::collections::HashSet<_> = all.map(Freshness::color).into_iter().collect();
        assert_eq!(colours.len(), 4);
        // Redder as it gets worse: red minus green rises at each step.
        let warmth: Vec<i16> = all
            .iter()
            .map(|f| f.color().r() as i16 - f.color().g() as i16)
            .collect();
        assert!(warmth.windows(2).all(|w| w[0] < w[1]), "{warmth:?}");
    }

    #[test]
    fn health_and_scan_agree_on_what_stale_looks_like() {
        assert_eq!(
            Freshness::of_health(HealthState::Stale),
            Some(Freshness::of_scan(Phase::Stale))
        );
        assert_eq!(
            Freshness::of_health(HealthState::Delayed),
            Some(Freshness::of_scan(Phase::Aging))
        );
        assert_eq!(
            Freshness::of_health(HealthState::Failed),
            Some(Freshness::of_scan(Phase::Offline))
        );
        assert_eq!(Freshness::of_health(HealthState::Fetching), None);
        assert_eq!(
            Freshness::of_health(HealthState::Cached),
            Some(Freshness::Stale)
        );
    }
}
