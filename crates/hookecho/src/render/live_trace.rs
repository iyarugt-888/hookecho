//! Bounded accepted-update evidence. Monotonic client stages are never compared with
//! source UTC, and GPU completion is never substituted for visible presentation.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Clone, Debug, serde::Serialize)]
pub struct RevisionIdentity {
    pub receiver_pane: usize,
    pub site: Option<String>,
    /// Adapter label only, never an endpoint or credentials.
    pub provider: Option<String>,
    pub volume: String,
    /// Source volume start, not the acquisition time of every gate.
    pub volume_start_unix_ms: i64,
    pub accepted_revision: u64,
    pub raw_acquisition: Option<RawAcquisition>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct RawAcquisition {
    pub elevation_number: usize,
    pub observed_radials: usize,
    pub unknown_clock_radials: usize,
    /// Bounds of valid clocks in this update only; not every gate in the merged scan.
    pub known_interval_unix_ms: Option<[i64; 2]>,
}

impl RawAcquisition {
    pub fn from_coverage(coverage: &wxdata::live::RadialCoverage) -> Self {
        Self::from_radials(coverage.progress.elevation_number, &coverage.radials)
    }

    fn from_radials(elevation_number: usize, radials: &[(u16, i64)]) -> Self {
        let mut known_interval_unix_ms: Option<[i64; 2]> = None;
        let mut unknown_clock_radials = 0;
        for &(_, time) in radials {
            if time <= 0 || chrono::DateTime::from_timestamp_millis(time).is_none() {
                unknown_clock_radials += 1;
            } else {
                known_interval_unix_ms = Some(match known_interval_unix_ms {
                    None => [time, time],
                    Some([first, last]) => [first.min(time), last.max(time)],
                });
            }
        }
        Self {
            elevation_number,
            observed_radials: radials.len(),
            unknown_clock_radials,
            known_interval_unix_ms,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct DrawIdentity {
    pub pane: usize,
    pub tilt: usize,
    pub moment: String,
    /// Derived products keep their identity; a REF input does not make their output raw REF.
    pub product_key: Option<u64>,
    pub column_max: bool,
    pub clean_reflectivity: bool,
    pub previous_visit: Option<usize>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct RevisionTrace {
    pub identity: RevisionIdentity,
    /// Elapsed from the provider's client receipt boundary. This is not necessarily
    /// first byte, and completed-volume providers can omit it altogether.
    pub receipt_to_accept_us: Option<u64>,
    pub decode_work_us: u64,
    pub receipt_to_queue_writes_us: Option<u64>,
    pub receipt_to_gpu_callback_us: Option<u64>,
    pub first_2d_upload: Option<DrawIdentity>,
    pub gpu_observation: &'static str,
    pub first_byte_unix_ms: Option<i64>,
    pub first_byte_unavailable_reason: &'static str,
    pub receipt_to_visible_presentation_us: Option<u64>,
    pub presentation_unavailable_reason: &'static str,
}

#[derive(Default)]
pub(super) struct RevisionTraces(Mutex<VecDeque<RevisionTrace>>);

fn micros(d: Duration) -> u64 {
    d.as_micros().min(u64::MAX as u128) as u64
}

impl RevisionTraces {
    pub(super) const CAPACITY: usize = 128;

    pub(super) fn accept(
        &self,
        identity: RevisionIdentity,
        receipt_to_accept: Option<Duration>,
        decode_work: Duration,
    ) {
        let mut rows = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rows.push_back(RevisionTrace {
            identity,
            receipt_to_accept_us: receipt_to_accept.map(micros),
            decode_work_us: micros(decode_work),
            receipt_to_queue_writes_us: None,
            receipt_to_gpu_callback_us: None,
            first_2d_upload: None,
            gpu_observation: "no_observed_2d_upload",
            first_byte_unix_ms: None,
            first_byte_unavailable_reason:
                "provider receipt does not expose a first-byte UTC clock",
            receipt_to_visible_presentation_us: None,
            presentation_unavailable_reason: "no compositor or external display observer attached",
        });
        if rows.len() > Self::CAPACITY {
            rows.pop_front();
        }
    }

    fn update(&self, revision: Option<u64>, update: impl FnOnce(&mut RevisionTrace)) {
        let Some(revision) = revision else { return };
        let mut rows = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(row) = rows
            .iter_mut()
            .find(|r| r.identity.accepted_revision == revision)
        {
            update(row);
        }
        // A late callback for an evicted revision is discarded, never attached to the newest.
    }

    pub(super) fn queued(
        &self,
        revision: Option<u64>,
        elapsed: Duration,
        draw: Option<&DrawIdentity>,
    ) {
        self.update(revision, |row| {
            if row.receipt_to_queue_writes_us.is_none() {
                row.receipt_to_queue_writes_us = Some(micros(elapsed));
                row.first_2d_upload = draw.cloned();
                row.gpu_observation = "awaiting_callback";
            }
        });
    }

    pub(super) fn gpu_done(&self, revision: Option<u64>, elapsed: Duration) {
        self.update(revision, |row| {
            if row.receipt_to_queue_writes_us.is_some() && row.receipt_to_gpu_callback_us.is_none()
            {
                row.receipt_to_gpu_callback_us = Some(micros(elapsed));
                row.gpu_observation = "callback_observed_not_presentation";
            }
        });
    }

    pub(super) fn unobserved(&self, revision: Option<u64>) {
        self.update(revision, |row| {
            row.gpu_observation = "callback_registration_too_late"
        });
    }

    pub(super) fn snapshot(&self) -> Vec<RevisionTrace> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_bounds_exclude_unknown_clocks_without_replacing_them_with_volume_start() {
        let source =
            RawAcquisition::from_radials(3, &[(1, 0), (2, 2000), (3, 1000), (4, i64::MAX)]);
        assert_eq!(source.elevation_number, 3);
        assert_eq!(source.observed_radials, 4);
        assert_eq!(source.unknown_clock_radials, 2);
        assert_eq!(source.known_interval_unix_ms, Some([1000, 2000]));
        assert_eq!(
            RawAcquisition::from_radials(3, &[(1, 0)]).known_interval_unix_ms,
            None
        );
    }

    pub(super) fn identity(revision: u64) -> RevisionIdentity {
        RevisionIdentity {
            receiver_pane: 1,
            site: Some("KTLX".into()),
            provider: Some("test adapter".into()),
            volume: "KTLX20261013_000000".into(),
            volume_start_unix_ms: 1791849600000,
            accepted_revision: revision,
            raw_acquisition: None,
        }
    }

    #[test]
    fn callbacks_follow_their_revision_and_unuploaded_updates_remain_visible() {
        let trace = RevisionTraces::default();
        for revision in 1..=3 {
            trace.accept(
                identity(revision),
                Some(Duration::from_micros(10)),
                Duration::from_micros(3),
            );
        }
        let draw = DrawIdentity {
            pane: 2,
            tilt: 1,
            moment: "REF".into(),
            product_key: None,
            column_max: false,
            clean_reflectivity: false,
            previous_visit: None,
        };
        trace.queued(Some(1), Duration::from_micros(20), Some(&draw));
        trace.queued(Some(3), Duration::from_micros(30), Some(&draw));
        trace.gpu_done(Some(3), Duration::from_micros(50));
        trace.gpu_done(Some(1), Duration::from_micros(70));
        trace.gpu_done(Some(1), Duration::from_micros(90)); // repeated observation keeps the first
        let rows = trace.snapshot();
        assert_eq!(rows[0].receipt_to_gpu_callback_us, Some(70));
        assert_eq!(rows[2].receipt_to_gpu_callback_us, Some(50));
        assert_eq!(rows[1].receipt_to_queue_writes_us, None);
        assert_eq!(rows[0].first_2d_upload.as_ref().unwrap().pane, 2);
        assert!(rows
            .iter()
            .all(|r| r.receipt_to_visible_presentation_us.is_none()));
        let json = serde_json::to_value(&rows[1]).unwrap();
        assert!(json["first_byte_unix_ms"].is_null());
        assert!(json["receipt_to_visible_presentation_us"].is_null());
        assert!(json["presentation_unavailable_reason"]
            .as_str()
            .unwrap()
            .contains("no compositor"));
    }

    #[test]
    fn bounded_history_and_missing_receipts_cannot_acquire_foreign_callbacks() {
        let trace = RevisionTraces::default();
        for revision in 0..=RevisionTraces::CAPACITY as u64 {
            trace.accept(identity(revision), None, Duration::ZERO);
        }
        trace.gpu_done(Some(0), Duration::from_secs(1));
        trace.gpu_done(None, Duration::from_secs(2));
        let rows = trace.snapshot();
        assert_eq!(rows.len(), RevisionTraces::CAPACITY);
        assert_eq!(rows[0].identity.accepted_revision, 1);
        assert!(rows
            .iter()
            .all(|r| r.receipt_to_accept_us.is_none() && r.receipt_to_gpu_callback_us.is_none()));
        trace.queued(Some(2), Duration::from_micros(20), None);
        trace.unobserved(Some(2));
        assert_eq!(
            trace.snapshot()[1].gpu_observation,
            "callback_registration_too_late"
        );
        assert_eq!(trace.snapshot()[1].receipt_to_gpu_callback_us, None);
        assert!(
            RevisionTraces::default().snapshot().is_empty(),
            "a new subscription starts empty"
        );
    }
}
