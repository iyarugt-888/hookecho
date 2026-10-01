//! Per-pane acquisition state for a live Level II session.
//!
//! This describes the feed, not the timeline playhead. Archive replay must never make a
//! live source look healthy. All transitions use accepted messages from the current stream.

use chrono::{DateTime, Utc};
use wxdata::live::{CutKind, RadialCoverage, ScanProgress};

#[derive(Clone, Debug)]
struct CutObservation {
    kind: CutKind,
    chunks: Vec<bool>,
    /// Newest source acquisition time seen at each one-based azimuth position.
    radial_times_ms: Vec<i64>,
}

impl CutObservation {
    fn new(progress: ScanProgress) -> Self {
        Self {
            kind: progress.cut_kind,
            chunks: vec![false; progress.chunks_in_sweep],
            radial_times_ms: vec![0; progress.chunks_in_sweep * 120],
        }
    }

    fn complete(&self) -> bool {
        !self.chunks.is_empty()
            && self.chunks.iter().all(|seen| *seen)
            && self.radial_gaps().is_empty()
    }

    fn radial_gaps(&self) -> Vec<(u16, u16)> {
        let mut gaps = Vec::new();
        for (chunk, observed) in self.chunks.iter().enumerate() {
            if !observed {
                continue;
            }
            let start = chunk * 120;
            let sector = &self.radial_times_ms[start..start + 120];
            let Some(first) = sector.iter().position(|time| *time > 0) else {
                continue; // no radial bounds in this chunk yet
            };
            let last = sector.iter().rposition(|time| *time > 0).unwrap_or(first);
            let mut index = first;
            while index <= last {
                if sector[index] > 0 {
                    index += 1;
                    continue;
                }
                let gap_start = index;
                while index <= last && sector[index] == 0 {
                    index += 1;
                }
                gaps.push(((start + gap_start + 1) as u16, (start + index) as u16));
            }
        }
        gaps
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Progression {
    pub vcp_number: Option<u16>,
    pub expected_cuts: usize,
    pub observed_cuts: usize,
    pub completed_cuts: usize,
    pub volume_complete: bool,
    pub expected_next_cut: Option<usize>,
    /// Seconds since the source volume start, if that timestamp is available.
    pub elapsed_secs: Option<i64>,
    /// Rough projection at the current cut's rotation rate; later cuts may run faster or slower.
    pub projected_remaining_secs: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutCoverage {
    Unobserved,
    Partial,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceMode {
    ProgressiveRadials,
    CompletedVolumes,
}

impl SourceMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::ProgressiveRadials => "progressive radials",
            Self::CompletedVolumes => "completed volumes only",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    AwaitingVolume,
    AcquiringSweep,
    SweepComplete,
    VolumeComplete,
    Recovering,
    FallbackSource,
    Aging,
    Stale,
    Offline,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::AwaitingVolume => "Awaiting volume",
            Self::AcquiringSweep => "Acquiring sweep",
            Self::SweepComplete => "Sweep complete",
            Self::VolumeComplete => "Volume complete",
            Self::Recovering => "Recovering",
            Self::FallbackSource => "Fallback source",
            Self::Aging => "Aging",
            Self::Stale => "Stale",
            Self::Offline => "Offline",
        }
    }
}

/// The provider name a pane's scan carries while it follows completed volumes by polling.
pub const COMPLETED_POLL_LABEL: &str = "Completed-volume poll";

#[derive(Clone, Debug, Default)]
pub struct LiveScan {
    pub site: Option<String>,
    pub provider: Option<String>,
    pub volume: Option<String>,
    /// The source's volume timestamp. It is not the age of every radial in a mixed sweep.
    pub volume_time: Option<DateTime<Utc>>,
    pub last_received: Option<DateTime<Utc>>,
    pub progress: Option<ScanProgress>,
    pub source_mode: Option<SourceMode>,
    pub switch_reason: Option<String>,
    /// Cut positions, not unique angles: supplemental low-level cuts keep their own slot.
    observed_cuts: Vec<Option<CutObservation>>,
    /// Retained across stream reconnects to reject delayed progress from an old volume.
    latest_progress_volume_start_ms: Option<i64>,
    progress_vcp_number: Option<u16>,
    streaming: bool,
    fallback: bool,
    recovering: bool,
}

impl LiveScan {
    pub fn reset(&mut self, site: Option<String>) {
        *self = Self {
            site,
            ..Self::default()
        };
    }

    pub fn stream_started(&mut self, provider: &str, fallback: bool) {
        self.provider = Some(provider.to_owned());
        self.streaming = true;
        self.fallback = fallback;
        self.recovering = false;
        self.progress = None;
    }

    pub fn set_source_mode(&mut self, mode: SourceMode) {
        self.source_mode = Some(mode);
    }

    pub fn set_switch_reason(&mut self, reason: impl Into<String>) {
        self.switch_reason = Some(reason.into());
    }

    pub fn progress(&mut self, progress: ScanProgress, received: DateTime<Utc>) {
        // A recovered source can still deliver progress for an older volume. Never let it
        // replace the current cut's inventory before its matching Update is rejected.
        if matches!((self.latest_progress_volume_start_ms, progress.volume_start_ms), (Some(old), Some(new)) if new < old)
        {
            return;
        }
        if !(1..=64).contains(&progress.total_elevations)
            || !(1..=progress.total_elevations).contains(&progress.elevation_number)
            || !(1..=64).contains(&progress.chunks_in_sweep)
            || !(1..=progress.chunks_in_sweep).contains(&progress.chunk_index)
        {
            return;
        }
        let next_volume = matches!((self.latest_progress_volume_start_ms, progress.volume_start_ms), (Some(old), Some(new)) if new > old);
        let next_vcp = self.progress_vcp_number.is_some()
            && progress.vcp_number.is_some()
            && self.progress_vcp_number != progress.vcp_number;
        let changed_cut_count =
            !self.observed_cuts.is_empty() && self.observed_cuts.len() != progress.total_elevations;
        if next_volume || next_vcp || changed_cut_count {
            self.observed_cuts.clear();
            self.progress = None;
        }
        if let Some(start) = progress.volume_start_ms {
            self.latest_progress_volume_start_ms = Some(start);
        }
        if let Some(vcp) = progress.vcp_number {
            self.progress_vcp_number = Some(vcp);
        }
        self.observed_cuts
            .resize_with(progress.total_elevations, || None);
        let cut = &mut self.observed_cuts[progress.elevation_number - 1];
        if cut.as_ref().is_none_or(|seen| {
            seen.kind != progress.cut_kind || seen.chunks.len() != progress.chunks_in_sweep
        }) {
            *cut = Some(CutObservation::new(progress));
        }
        cut.as_mut()
            .expect("cut observation was initialized")
            .chunks[progress.chunk_index - 1] = true;
        // Delayed earlier cuts can fill inventory gaps, but cannot move the marker backwards.
        if self.progress.is_none_or(|old| {
            progress.elevation_number > old.elevation_number
                || (progress.elevation_number == old.elevation_number
                    && (progress.cut_kind != old.cut_kind
                        || progress.chunk_index >= old.chunk_index))
        }) {
            self.progress = Some(progress);
        }
        self.last_received = Some(received);
        self.recovering = false;
    }

    pub fn unobserved_chunks(&self) -> Vec<usize> {
        let Some(p) = self.progress else {
            return Vec::new();
        };
        self.cut(p.elevation_number)
            .into_iter()
            .flat_map(|cut| cut.chunks.iter().take(p.chunk_index).enumerate())
            .filter_map(|(index, observed)| (!observed).then_some(index + 1))
            .collect()
    }

    /// Missing one-based radial-number spans bounded by observed radials within received chunks.
    /// Unscanned chunk sectors are reported separately by [`Self::unobserved_chunks`].
    pub fn radial_gaps(&self) -> Vec<(u16, u16)> {
        self.progress
            .and_then(|p| self.cut(p.elevation_number))
            .map_or_else(Vec::new, CutObservation::radial_gaps)
    }

    /// Apply raw positions from the arriving chunk, before the merged sweep's older pass can
    /// fill holes. The progress event normally arrived first, but this also tolerates a missing
    /// progress message without guessing from elevation angle.
    pub fn observe_radials(&mut self, coverage: RadialCoverage, received: DateTime<Utc>) {
        let p = coverage.progress;
        if !(1..=64).contains(&p.total_elevations)
            || !(1..=p.total_elevations).contains(&p.elevation_number)
            || !(1..=64).contains(&p.chunks_in_sweep)
            || !(1..=p.chunks_in_sweep).contains(&p.chunk_index)
        {
            return;
        }
        self.progress(p, received);
        if (p.volume_start_ms.is_some()
            && self.latest_progress_volume_start_ms != p.volume_start_ms)
            || (p.vcp_number.is_some() && self.progress_vcp_number != p.vcp_number)
        {
            return;
        }
        let Some(cut) = self
            .observed_cuts
            .get_mut(p.elevation_number.saturating_sub(1))
            .and_then(Option::as_mut)
        else {
            return;
        };
        let newest_arrival = coverage
            .radials
            .iter()
            .map(|(_, time)| *time)
            .max()
            .unwrap_or(0);
        let newest_prior = cut.radial_times_ms.iter().copied().max().unwrap_or(0);
        let rotation_ms = (p.chunk_duration_secs() * p.chunks_in_sweep as f32 * 1000.0) as i64;
        let new_pass = p.chunk_index == 1
            && newest_prior > 0
            && newest_arrival > newest_prior + rotation_ms.saturating_mul(3) / 4;
        if new_pass {
            cut.chunks.fill(false);
            cut.radial_times_ms.fill(0);
            self.progress = Some(p);
        }
        cut.chunks[p.chunk_index - 1] = true;
        for (number, time) in coverage.radials {
            let index = if number == 0 { 0 } else { number as usize - 1 };
            if time > 0 {
                if let Some(slot) = cut.radial_times_ms.get_mut(index) {
                    *slot = (*slot).max(time);
                }
            }
        }
    }

    fn cut(&self, number: usize) -> Option<&CutObservation> {
        self.observed_cuts.get(number.checked_sub(1)?)?.as_ref()
    }

    pub fn cut_coverage(&self, number: usize) -> CutCoverage {
        match self.cut(number) {
            None => CutCoverage::Unobserved,
            Some(cut) if cut.complete() => CutCoverage::Complete,
            Some(_) => CutCoverage::Partial,
        }
    }

    pub fn cut_kind(&self, number: usize) -> Option<CutKind> {
        self.cut(number).map(|cut| cut.kind)
    }

    fn sweep_complete(&self) -> bool {
        self.progress
            .and_then(|p| self.cut(p.elevation_number))
            .is_some_and(CutObservation::complete)
    }

    fn volume_complete(&self) -> bool {
        self.progress.is_some_and(|p| {
            p.total_elevations > 0
                && p.total_elevations <= 64
                && (1..=p.total_elevations)
                    .all(|number| self.cut(number).is_some_and(CutObservation::complete))
        })
    }

    pub fn progression(&self, now: DateTime<Utc>) -> Option<Progression> {
        let p = self.progress?;
        let observed_cuts = self
            .observed_cuts
            .iter()
            .filter(|cut| cut.is_some())
            .count();
        let completed_cuts = self
            .observed_cuts
            .iter()
            .flatten()
            .filter(|cut| cut.complete())
            .count();
        let expected_next_cut = if self.sweep_complete() {
            (p.elevation_number < p.total_elevations).then_some(p.elevation_number + 1)
        } else {
            Some(p.elevation_number)
        };
        let remaining_chunks = p.chunks_in_sweep.saturating_sub(
            self.cut(p.elevation_number)
                .map_or(0, |cut| cut.chunks.iter().filter(|seen| **seen).count()),
        ) + p.total_elevations.saturating_sub(p.elevation_number)
            * p.chunks_in_sweep;
        let rate = p.chunk_duration_secs();
        let volume_complete = self.volume_complete();
        Some(Progression {
            vcp_number: p.vcp_number,
            expected_cuts: p.total_elevations,
            observed_cuts,
            completed_cuts,
            volume_complete,
            expected_next_cut,
            elapsed_secs: p
                .volume_start_ms
                .map(|ms| (now.timestamp_millis() - ms).max(0) / 1000),
            projected_remaining_secs: (rate.is_finite()
                && rate > 0.0
                && (remaining_chunks > 0 || volume_complete))
                .then_some(remaining_chunks as f32 * rate),
        })
    }

    /// Returns false for a delayed older volume, including one from a recovered provider.
    pub fn accept_volume(
        &mut self,
        name: &str,
        time: DateTime<Utc>,
        received: DateTime<Utc>,
    ) -> bool {
        if self.volume_time.is_some_and(|current| time < current) {
            return false;
        }
        // S3 may assign the same upload second to adjacent chunks. Their padded sequence
        // numbers provide the tie-breaker within a volume.
        if self.volume_time == Some(time)
            && self.volume.as_deref().is_some_and(|current| {
                match (chunk_sequence(name), chunk_sequence(current)) {
                    (Some((new_prefix, new_seq)), Some((old_prefix, old_seq))) => {
                        (new_prefix, new_seq) < (old_prefix, old_seq)
                    }
                    _ => false,
                }
            })
        {
            return false;
        }
        // Update names include the chunk sequence, so a new name does not imply a new
        // volume. Keep the immediately preceding progress event for the chunk being shown.
        self.volume = Some(name.to_owned());
        self.volume_time = Some(time);
        self.last_received = Some(received);
        self.recovering = false;
        if !self.streaming {
            self.provider = Some(COMPLETED_POLL_LABEL.to_owned());
            self.source_mode = Some(SourceMode::CompletedVolumes);
        }
        true
    }

    /// Why the pane is following completed volumes now, for the provider-switch log: the stream
    /// was lost, or the app stopped it on purpose (a loop, a scrub).
    pub fn poll_reason(&self) -> &'static str {
        if self.recovering {
            "live stream unavailable; completed-volume polling"
        } else {
            "live stream paused (loop or scrub); completed-volume polling"
        }
    }

    /// The stream died on its own: the feed is recovering until a volume arrives.
    pub fn stream_ended(&mut self) {
        self.streaming = false;
        self.progress = None;
        self.recovering = true;
    }

    /// The app stopped the stream on purpose: a loop started playing, the view was scrubbed off
    /// the live head, or the site or provider changed. Nothing failed, so nothing is recovering;
    /// the volume already shown stays what it was.
    pub fn stream_stopped(&mut self) {
        self.streaming = false;
        self.progress = None;
    }

    /// `stale_after` is shared with the visible radar-health threshold. Aging begins at 80%
    /// of it, so the operator sees a warning before the stale boundary.
    pub fn phase(&self, now: DateTime<Utc>, stale_after_secs: i64) -> Phase {
        if let Some(time) = self.volume_time {
            let age = (now - time).num_seconds();
            if age >= stale_after_secs {
                return Phase::Stale;
            }
            if age >= stale_after_secs * 4 / 5 {
                return Phase::Aging;
            }
        } else if !self.streaming && self.recovering {
            return Phase::Offline;
        }
        if self.recovering {
            return Phase::Recovering;
        }
        if !self.streaming && self.volume_time.is_some() {
            return Phase::VolumeComplete;
        }
        if self.fallback {
            return Phase::FallbackSource;
        }
        if self.progress.is_some() {
            if self.sweep_complete() {
                return if self.volume_complete() {
                    Phase::VolumeComplete
                } else {
                    Phase::SweepComplete
                };
            }
            return Phase::AcquiringSweep;
        }
        Phase::AwaitingVolume
    }

    pub fn description(&self, now: DateTime<Utc>, stale_after_secs: i64) -> String {
        let mut s = self.phase(now, stale_after_secs).label().to_owned();
        if let Some(provider) = &self.provider {
            s.push_str(&format!(" via {provider}"));
        }
        if let Some(mode) = self.source_mode {
            s.push_str(&format!(" · {}", mode.label()));
        }
        if let Some(reason) = &self.switch_reason {
            s.push_str(&format!(" · last switch: {reason}"));
        }
        if let Some(p) = self.progress {
            if let Some(vcp) = p.vcp_number {
                s.push_str(&format!(" · VCP {vcp}"));
            }
            s.push_str(&format!(
                " · cut {}/{} ({}) at {:.1}° · chunk {}/{}",
                p.elevation_number,
                p.total_elevations,
                p.cut_kind.label(),
                p.elevation_angle_deg,
                p.chunk_index,
                p.chunks_in_sweep
            ));
            let gaps = self.unobserved_chunks();
            if !gaps.is_empty() {
                s.push_str(&format!(" · unobserved chunks {gaps:?}"));
            }
            if let Some(gaps) = gap_summary(&self.radial_gaps()) {
                s.push_str(&format!(" · {gaps}"));
            }
        }
        if let Some(time) = self.volume_time {
            s.push_str(&format!(
                " · volume {} · {} s old",
                self.volume.as_deref().unwrap_or("?"),
                (now - time).num_seconds().max(0)
            ));
        }
        s
    }
}

/// Radial gaps said plainly: how many radials are missing and the first few spans by radial
/// number. `None` when there are none.
pub fn gap_summary(gaps: &[(u16, u16)]) -> Option<String> {
    if gaps.is_empty() {
        return None;
    }
    let missing: u32 = gaps
        .iter()
        .map(|(a, b)| u32::from(b.saturating_sub(*a)) + 1)
        .sum();
    let spans: Vec<String> = gaps
        .iter()
        .take(3)
        .map(|(a, b)| {
            if a == b {
                format!("#{a}")
            } else {
                format!("#{a}\u{2013}{b}")
            }
        })
        .collect();
    let more = if gaps.len() > 3 {
        format!(" +{} more", gaps.len() - 3)
    } else {
        String::new()
    };
    let radials = if missing == 1 { "radial" } else { "radials" };
    Some(format!(
        "{missing} {radials} missing ({}{more})",
        spans.join(", ")
    ))
}

fn chunk_sequence(name: &str) -> Option<(&str, u16)> {
    let prefix = name.get(..15)?;
    let sequence = name.get(16..19)?.parse().ok()?;
    Some((prefix, sequence))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn progress(cut: usize, chunk: usize) -> ScanProgress {
        ScanProgress {
            volume_start_ms: Some(1000),
            vcp_number: Some(212),
            cut_kind: wxdata::live::CutKind::Standard,
            elevation_number: cut,
            total_elevations: 3,
            elevation_angle_deg: 0.5,
            azimuth_rate_dps: 18.0,
            azimuth_start_deg: 0.0,
            azimuth_end_deg: 120.0,
            chunk_index: chunk,
            chunks_in_sweep: 3,
        }
    }

    #[test]
    fn radial_gaps_read_as_a_count_and_their_first_spans() {
        assert_eq!(gap_summary(&[]), None);
        assert_eq!(
            gap_summary(&[(7, 7)]).as_deref(),
            Some("1 radial missing (#7)")
        );
        assert_eq!(
            gap_summary(&[(1, 4), (10, 10), (20, 21), (30, 30)]).as_deref(),
            Some("8 radials missing (#1\u{2013}4, #10, #20\u{2013}21 +1 more)")
        );
    }

    #[test]
    fn sweep_completion_and_supplemental_cuts_are_distinct() {
        let now = Utc::now();
        let mut state = LiveScan::default();
        state.stream_started("chunks", false);
        state.progress(progress(1, 1), now);
        assert_eq!(state.phase(now, 900), Phase::AcquiringSweep);
        state.progress(progress(1, 2), now);
        state.progress(progress(1, 3), now);
        assert_eq!(state.phase(now, 900), Phase::SweepComplete);
        state.progress(progress(2, 1), now);
        state.progress(progress(2, 2), now);
        state.progress(progress(2, 3), now);
        // A repeated low cut can have the same angle but a different VCP position.
        state.progress(progress(3, 1), now);
        state.progress(progress(3, 2), now);
        state.progress(progress(3, 3), now);
        assert_eq!(state.phase(now, 900), Phase::VolumeComplete);
    }

    #[test]
    fn final_chunk_does_not_hide_gap_and_late_fill_does_not_regress_marker() {
        let now = Utc::now();
        let mut state = LiveScan::default();
        state.stream_started("chunks", false);
        state.progress(progress(1, 1), now);
        state.progress(progress(1, 3), now);
        assert_eq!(state.unobserved_chunks(), vec![2]);
        assert_eq!(state.phase(now, 900), Phase::AcquiringSweep);
        state.progress(progress(1, 2), now);
        assert_eq!(state.unobserved_chunks(), Vec::<usize>::new());
        assert_eq!(state.progress.unwrap().chunk_index, 3);
        assert_eq!(state.phase(now, 900), Phase::SweepComplete);
    }

    #[test]
    fn volume_and_supplemental_cut_reset_chunk_inventory() {
        let now = Utc::now();
        let mut state = LiveScan::default();
        state.stream_started("chunks", false);
        state.progress(progress(1, 1), now);
        state.progress(progress(1, 3), now);
        let supplemental = ScanProgress {
            cut_kind: wxdata::live::CutKind::Sails,
            ..progress(1, 1)
        };
        state.progress(supplemental, now);
        assert!(state.unobserved_chunks().is_empty());
        assert_eq!(
            state.progress.unwrap().cut_kind,
            wxdata::live::CutKind::Sails
        );
        let next_volume = ScanProgress {
            volume_start_ms: Some(2000),
            ..progress(1, 1)
        };
        state.progress(next_volume, now);
        state.stream_ended();
        state.stream_started("reconnected chunks", false);
        state.progress(
            ScanProgress {
                volume_start_ms: Some(1000),
                ..progress(1, 3)
            },
            now,
        );
        assert_eq!(state.progress, None);
        state.progress(next_volume, now);
        assert_eq!(state.progress.unwrap().volume_start_ms, Some(2000));
        assert!(state.unobserved_chunks().is_empty());
    }

    #[test]
    fn progression_preserves_prior_cut_coverage_and_ignores_late_cut_regression() {
        let now = Utc::now();
        let mut state = LiveScan::default();
        state.stream_started("chunks", false);
        state.progress(progress(1, 1), now);
        state.progress(progress(1, 3), now);
        state.progress(progress(2, 1), now);
        let summary = state.progression(now).unwrap();
        assert_eq!(
            (
                summary.expected_cuts,
                summary.observed_cuts,
                summary.completed_cuts
            ),
            (3, 2, 0)
        );
        assert_eq!(summary.expected_next_cut, Some(2));
        state.progress(progress(1, 2), now);
        assert_eq!(state.progress.unwrap().elevation_number, 2);
        assert_eq!(state.cut_coverage(1), CutCoverage::Complete);
        assert_eq!(state.progression(now).unwrap().completed_cuts, 1);
        assert_eq!(state.cut_coverage(3), CutCoverage::Unobserved);
    }

    #[test]
    fn vcp_change_resets_cut_progression() {
        let now = Utc::now();
        let mut state = LiveScan::default();
        state.stream_started("chunks", false);
        state.progress(progress(1, 1), now);
        state.progress(progress(2, 1), now);
        state.progress(
            ScanProgress {
                vcp_number: Some(35),
                ..progress(1, 1)
            },
            now,
        );
        let summary = state.progression(now).unwrap();
        assert_eq!(summary.vcp_number, Some(35));
        assert_eq!(summary.observed_cuts, 1);
        assert_eq!(state.progress.unwrap().elevation_number, 1);
    }

    #[test]
    fn last_cut_cannot_complete_volume_while_an_earlier_cut_has_a_gap() {
        let now = Utc::now();
        let mut state = LiveScan::default();
        state.stream_started("chunks", false);
        state.progress(progress(1, 1), now);
        state.progress(progress(1, 3), now);
        for cut in 2..=3 {
            for chunk in 1..=3 {
                state.progress(progress(cut, chunk), now);
            }
        }
        assert_eq!(state.phase(now, 900), Phase::SweepComplete);
        assert!(!state.progression(now).unwrap().volume_complete);
        state.progress(progress(1, 2), now);
        assert_eq!(state.progress.unwrap().elevation_number, 3);
        assert_eq!(state.phase(now, 900), Phase::VolumeComplete);
        assert!(state.progression(now).unwrap().volume_complete);
    }

    #[test]
    fn old_volume_cannot_reverse_time_and_staleness_wins_over_fallback() {
        let now = Utc::now();
        let mut state = LiveScan::default();
        state.stream_started("completed fallback", true);
        assert!(state.accept_volume("new", now - Duration::seconds(721), now));
        assert_eq!(state.phase(now, 900), Phase::Aging);
        assert!(!state.accept_volume("old", now - Duration::seconds(800), now));
        assert_eq!(state.volume.as_deref(), Some("new"));
        assert_eq!(state.phase(now + Duration::seconds(200), 900), Phase::Stale);
        state.stream_ended();
        assert_eq!(state.phase(now + Duration::seconds(200), 900), Phase::Stale);
    }

    #[test]
    fn recovery_and_site_reset_clear_old_progress() {
        let now = Utc::now();
        let mut state = LiveScan::default();
        state.stream_started("chunks", false);
        state.progress(progress(2, 2), now);
        state.stream_ended();
        assert_eq!(state.phase(now, 900), Phase::Offline);
        state.reset(Some("KTLX".into()));
        assert_eq!(state.progress, None);
        assert_eq!(state.phase(now, 900), Phase::AwaitingVolume);
    }

    #[test]
    fn chunk_names_do_not_erase_sweep_progress_and_poll_recovers() {
        let now = Utc::now();
        let mut state = LiveScan::default();
        state.stream_started("chunks", false);
        state.progress(progress(2, 1), now);
        assert!(state.accept_volume("volume_04", now, now));
        state.progress(progress(2, 2), now);
        assert!(state.accept_volume("volume_05", now, now));
        assert_eq!(state.phase(now, 900), Phase::AcquiringSweep);
        assert_eq!(state.progress.unwrap().chunk_index, 2);
        state.stream_ended();
        assert_eq!(state.phase(now, 900), Phase::Recovering);
        assert!(state.accept_volume("completed", now + Duration::seconds(1), now));
        assert_eq!(state.phase(now, 900), Phase::VolumeComplete);
        assert_eq!(state.provider.as_deref(), Some("Completed-volume poll"));
        assert_eq!(state.source_mode, Some(SourceMode::CompletedVolumes));
    }

    #[test]
    fn the_poll_reason_tells_a_lost_stream_from_a_paused_one() {
        let mut state = LiveScan::default();
        state.stream_started("chunks", false);
        state.stream_stopped();
        assert!(state.poll_reason().starts_with("live stream paused"));
        state.stream_started("chunks", false);
        state.stream_ended();
        assert!(state.poll_reason().starts_with("live stream unavailable"));
    }

    #[test]
    fn returning_to_the_stream_cannot_reverse_time() {
        // Live chunks, then the stream drops and polling delivers a newer completed volume.
        let t0 = Utc::now();
        let mut state = LiveScan::default();
        state.stream_started("chunks", false);
        assert!(state.accept_volume("20260520-190000-005-I", t0, t0));
        state.stream_ended();
        let t1 = t0 + Duration::seconds(300);
        assert!(state.accept_volume("KTLX20260520_190500_V06", t1, t1));
        assert_eq!(state.source_mode, Some(SourceMode::CompletedVolumes));
        // The preferred source comes back with a backlog: its older chunk must not win.
        state.stream_started("chunks", false);
        assert!(!state.accept_volume("20260520-190000-006-I", t0, t1));
        assert_eq!(state.volume_time, Some(t1));
        // Its first chunk of the current volume or a later one is accepted.
        assert!(state.accept_volume("20260520-191000-001-S", t1 + Duration::seconds(300), t1));
    }

    #[test]
    fn a_deliberate_stop_is_not_a_recovery() {
        let now = Utc::now();
        let mut state = LiveScan::default();
        state.stream_started("chunks", false);
        assert!(state.accept_volume("20260520-190000-005-I", now, now));
        state.stream_stopped();
        assert_eq!(state.phase(now, 900), Phase::VolumeComplete);
        state.stream_started("chunks", false);
        state.stream_ended();
        assert_eq!(state.phase(now, 900), Phase::Recovering);
    }

    #[test]
    fn equal_timestamp_uses_chunk_sequence_to_prevent_reversal() {
        let now = Utc::now();
        let mut state = LiveScan::default();
        state.stream_started("chunks", false);
        assert!(state.accept_volume("20260520-190000-005-I", now, now));
        assert!(!state.accept_volume("20260520-190000-004-I", now, now));
        assert_eq!(state.volume.as_deref(), Some("20260520-190000-005-I"));
        assert!(state.accept_volume("20260520-190000-006-I", now, now));
    }
}
