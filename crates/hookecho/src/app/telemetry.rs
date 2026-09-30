//! Local-only frame telemetry (ROADMAP_2 §14.1): how long each frame took the app to build, kept
//! for the last [`KEEP`] frames and read in Analyst Mode. Nothing leaves the machine.
//!
//! This is the CPU time of `HookEchoApp::ui`, not the gap between frames: the app repaints on
//! demand, so the gap mostly measures how long nothing needed drawing. A frame that takes longer
//! than a display refresh to build is one the user can feel.

use std::collections::VecDeque;

/// Frames kept: about ten seconds of continuous animation at 60 Hz.
const KEEP: usize = 600;
/// One refresh at 60 Hz.
pub(crate) const BUDGET_MS: f32 = 1000.0 / 60.0;
/// A frame this slow is a visible stall, not a missed refresh.
pub(crate) const STALL_MS: f32 = 50.0;

#[derive(Default)]
pub(crate) struct FrameTimes {
    ms: VecDeque<f32>,
    /// Frames ever recorded, for the stall count's context.
    total: u64,
    stalls: u64,
}

/// The kept frames, summarised.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FrameSummary {
    pub p50: f32,
    pub p95: f32,
    pub max: f32,
    /// Of the kept frames, how many ran over [`BUDGET_MS`].
    pub over_budget: usize,
    pub kept: usize,
    /// Since launch: frames, and stalls over [`STALL_MS`].
    pub total: u64,
    pub stalls: u64,
}

impl FrameTimes {
    pub(crate) fn push(&mut self, ms: f32) {
        if !ms.is_finite() || ms < 0.0 {
            return;
        }
        if self.ms.len() == KEEP {
            self.ms.pop_front();
        }
        self.ms.push_back(ms);
        self.total += 1;
        if ms > STALL_MS {
            self.stalls += 1;
        }
    }

    pub(crate) fn summary(&self) -> Option<FrameSummary> {
        if self.ms.is_empty() {
            return None;
        }
        let mut v: Vec<f32> = self.ms.iter().copied().collect();
        v.sort_by(f32::total_cmp);
        let at = |q: f32| v[((v.len() - 1) as f32 * q).round() as usize];
        Some(FrameSummary {
            p50: at(0.5),
            p95: at(0.95),
            max: v[v.len() - 1],
            over_budget: v.iter().filter(|m| **m > BUDGET_MS).count(),
            kept: v.len(),
            total: self.total,
            stalls: self.stalls,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_summarise_to_percentiles_and_overruns() {
        let mut f = FrameTimes::default();
        assert!(f.summary().is_none());
        for i in 1..=100 {
            f.push(i as f32);
        }
        f.push(f32::NAN);
        let s = f.summary().unwrap();
        assert_eq!(s.kept, 100);
        assert!(
            (s.p50 - 50.0).abs() <= 1.0 && (s.p95 - 95.0).abs() <= 1.0,
            "{s:?}"
        );
        assert_eq!(s.max, 100.0);
        assert_eq!(s.over_budget, 84, "17..=100 ms");
        assert_eq!(s.stalls, 50, "51..=100 ms");
    }

    #[test]
    fn only_the_recent_frames_are_kept_but_stalls_are_counted_since_launch() {
        let mut f = FrameTimes::default();
        f.push(80.0);
        for _ in 0..KEEP {
            f.push(2.0);
        }
        let s = f.summary().unwrap();
        assert_eq!(
            (s.kept, s.max, s.stalls, s.total),
            (KEEP, 2.0, 1, KEEP as u64 + 1)
        );
    }
}
