//! Valid-time alignment shared by radar, satellite, observations, and model grids.
use crate::field::ValueKind;
use chrono::{DateTime, Duration, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimePolicy {
    Exact,
    Nearest,
    NearestPast,
    HoldLast,
    InterpolateLinear,
    ForecastLead { run: DateTime<Utc>, lead: Duration },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameTime {
    pub valid: DateTime<Utc>,
    pub run: Option<DateTime<Utc>>,
}

/// Signed difference between a source frame and the analysis time shown on screen.
/// Positive means the source is newer; negative means it is older.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeOffset {
    pub offset: Duration,
    pub outside_tolerance: bool,
}

impl TimeOffset {
    pub fn between(
        source_valid: DateTime<Utc>,
        analysis_valid: DateTime<Utc>,
        tolerance: Duration,
    ) -> Self {
        let offset = source_valid - analysis_valid;
        Self {
            offset,
            outside_tolerance: tolerance < Duration::zero() || offset.abs() > tolerance,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FrameSelection {
    Single {
        index: usize,
        offset: Duration,
    },
    Blend {
        before: usize,
        after: usize,
        weight_after: f64,
    },
}

impl FrameSelection {
    pub fn single_index(self) -> Option<usize> {
        match self {
            Self::Single { index, .. } => Some(index),
            Self::Blend { .. } => None,
        }
    }
}

/// Select by valid time. `tolerance` is an absolute maximum age/difference; `None` is unlimited.
/// Categorical, vector, and mask fields never interpolate. Missing frames return `None`.
pub fn select(
    frames: &[FrameTime],
    target: DateTime<Utc>,
    policy: TimePolicy,
    tolerance: Option<Duration>,
    kind: ValueKind,
) -> Option<FrameSelection> {
    let within = |delta: Duration| {
        tolerance.is_none_or(|limit| {
            limit >= Duration::zero() && delta.num_milliseconds().abs() <= limit.num_milliseconds()
        })
    };
    let nearest = |filter: &dyn Fn(&FrameTime) -> bool| {
        frames
            .iter()
            .enumerate()
            .filter(|(_, frame)| filter(frame))
            .map(|(index, frame)| (index, frame.valid - target))
            .filter(|(_, delta)| within(*delta))
            .min_by_key(|(_, delta)| delta.num_milliseconds().abs())
            .map(|(index, offset)| FrameSelection::Single { index, offset })
    };
    match policy {
        TimePolicy::Exact => nearest(&|frame| frame.valid == target),
        TimePolicy::Nearest => nearest(&|_| true),
        TimePolicy::NearestPast | TimePolicy::HoldLast => nearest(&|frame| frame.valid <= target),
        TimePolicy::ForecastLead { run, lead } => {
            if lead < Duration::zero() {
                return None;
            }
            let valid = run.checked_add_signed(lead)?;
            if valid != target {
                return None;
            }
            nearest(&|frame| frame.run == Some(run) && frame.valid == valid)
        }
        TimePolicy::InterpolateLinear => {
            if !matches!(kind, ValueKind::Scalar | ValueKind::Probability) {
                return None;
            }
            if let Some(exact) = nearest(&|frame| frame.valid == target) {
                return Some(exact);
            }
            let before = frames
                .iter()
                .enumerate()
                .filter(|(_, f)| f.valid < target)
                .filter(|(_, f)| within(f.valid - target))
                .max_by_key(|(_, f)| f.valid)?;
            let after = frames
                .iter()
                .enumerate()
                .filter(|(_, f)| f.valid > target)
                .filter(|(_, f)| within(f.valid - target))
                .min_by_key(|(_, f)| f.valid)?;
            // Never blend two model runs under one unqualified valid-time label.
            if before.1.run != after.1.run {
                return None;
            }
            let elapsed = (target - before.1.valid).num_milliseconds() as f64;
            let span = (after.1.valid - before.1.valid).num_milliseconds() as f64;
            Some(FrameSelection::Blend {
                before: before.0,
                after: after.0,
                weight_after: elapsed / span,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn t(minutes: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(minutes * 60, 0).unwrap()
    }
    fn frame(minutes: i64) -> FrameTime {
        FrameTime {
            valid: t(minutes),
            run: None,
        }
    }

    #[test]
    fn source_offset_is_signed_and_tolerance_inclusive() {
        let tolerance = Duration::minutes(10);
        let ahead = TimeOffset::between(t(20), t(10), tolerance);
        assert_eq!(ahead.offset, tolerance);
        assert!(!ahead.outside_tolerance);
        let behind = TimeOffset::between(t(0), t(11), tolerance);
        assert_eq!(behind.offset, Duration::minutes(-11));
        assert!(behind.outside_tolerance);
    }

    #[test]
    fn nearest_and_past_are_distinct_and_tolerance_is_exact() {
        let frames = [frame(0), frame(10), frame(30)];
        assert_eq!(
            select(
                &frames,
                t(21),
                TimePolicy::Nearest,
                Some(Duration::minutes(10)),
                ValueKind::Scalar
            )
            .unwrap()
            .single_index(),
            Some(2)
        );
        assert_eq!(
            select(
                &frames,
                t(21),
                TimePolicy::NearestPast,
                Some(Duration::minutes(12)),
                ValueKind::Scalar
            )
            .unwrap()
            .single_index(),
            Some(1)
        );
        assert!(select(
            &frames,
            t(21),
            TimePolicy::HoldLast,
            Some(Duration::minutes(10)),
            ValueKind::Scalar
        )
        .is_none());
        assert!(select(&frames, t(21), TimePolicy::Exact, None, ValueKind::Scalar).is_none());
    }

    #[test]
    fn interpolation_checks_kind_tolerance_and_run() {
        let run = t(0);
        let frames = [
            FrameTime {
                valid: t(10),
                run: Some(run),
            },
            FrameTime {
                valid: t(20),
                run: Some(run),
            },
        ];
        assert_eq!(
            select(
                &frames,
                t(15),
                TimePolicy::InterpolateLinear,
                Some(Duration::minutes(5)),
                ValueKind::Scalar
            ),
            Some(FrameSelection::Blend {
                before: 0,
                after: 1,
                weight_after: 0.5
            })
        );
        assert!(select(
            &frames,
            t(15),
            TimePolicy::InterpolateLinear,
            None,
            ValueKind::Categorical
        )
        .is_none());
        assert!(select(
            &frames,
            t(15),
            TimePolicy::InterpolateLinear,
            Some(Duration::minutes(4)),
            ValueKind::Scalar
        )
        .is_none());
        let mismatch = [
            frames[0],
            FrameTime {
                valid: t(20),
                run: Some(t(1)),
            },
        ];
        assert!(select(
            &mismatch,
            t(15),
            TimePolicy::InterpolateLinear,
            None,
            ValueKind::Scalar
        )
        .is_none());
    }

    #[test]
    fn forecast_lead_requires_matching_run_and_valid_time() {
        let frames = [
            FrameTime {
                valid: t(60),
                run: Some(t(0)),
            },
            FrameTime {
                valid: t(60),
                run: Some(t(10)),
            },
        ];
        assert_eq!(
            select(
                &frames,
                t(60),
                TimePolicy::ForecastLead {
                    run: t(0),
                    lead: Duration::minutes(60)
                },
                None,
                ValueKind::Scalar
            )
            .unwrap()
            .single_index(),
            Some(0)
        );
        assert!(select(
            &frames,
            t(60),
            TimePolicy::ForecastLead {
                run: t(0),
                lead: Duration::minutes(30)
            },
            None,
            ValueKind::Scalar
        )
        .is_none());
    }
}
