//! Per-pane acquisition state for a live Level II session.
//!
//! This describes the feed, not the timeline playhead. Archive replay must never make a
//! live source look healthy. All transitions use accepted messages from the current stream.

use chrono::{DateTime, Utc};
use wxdata::live::{CutKind, RadialCoverage, ScanProgress};

#[derive(Clone, Debug)]
struct CutObservation {
    kind: CutKind,
    angle_deg: f64,
    chunks: Vec<bool>,
    raw_chunks: Vec<bool>,
    /// Presence is independent of whether the source supplied a usable acquisition clock.
    radial_seen: Vec<bool>,
    /// Newest source acquisition time seen at each one-based azimuth position.
    radial_times_ms: Vec<i64>,
}

impl CutObservation {
    fn new(progress: ScanProgress) -> Self {
        Self {
            kind: progress.cut_kind,
            angle_deg: progress.elevation_angle_deg,
            chunks: vec![false; progress.chunks_in_sweep],
            raw_chunks: vec![false; progress.chunks_in_sweep],
            radial_seen: vec![false; progress.chunks_in_sweep * 120],
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
            let sector = &self.radial_seen[start..start + 120];
            let Some(first) = sector.iter().position(|seen| *seen) else {
                continue; // no radial bounds in this chunk yet
            };
            let last = sector.iter().rposition(|seen| *seen).unwrap_or(first);
            let mut index = first;
            while index <= last {
                if sector[index] {
                    index += 1;
                    continue;
                }
                let gap_start = index;
                while index <= last && !sector[index] {
                    index += 1;
                }
                gaps.push(((start + gap_start + 1) as u16, (start + index) as u16));
            }
        }
        gaps
    }
}

/// Raw acquisition evidence for one scan-local VCP position, before older sweep stitching.
/// Equal elevation angles remain separate cuts. This is not a persistent cut/pass identity.
#[derive(Clone, Debug, PartialEq)]
pub struct CutAcquisition {
    pub number: usize,
    pub kind: Option<CutKind>,
    pub angle_deg: Option<f64>,
    pub received_chunks: usize,
    pub expected_chunks: Option<usize>,
    pub unobserved_chunks: Vec<usize>,
    /// None when only progress metadata was received; Some(0) when raw input was empty.
    pub observed_radials: Option<usize>,
    pub raw_chunks: usize,
    pub unknown_clock_radials: usize,
    /// Bounds of known clocks only. An unknown-clock radial does not acquire these times.
    pub known_interval_ms: Option<(i64, i64)>,
    /// Unobserved positions bounded by raw arrivals within a received chunk. Not transport loss.
    pub internal_unobserved_spans: Vec<(u16, u16)>,
}

/// A small immutable summary of the receiver's current source volume, independent of playhead.
#[derive(Clone, Debug, PartialEq)]
pub struct AcquisitionInventory {
    pub volume_start_ms: Option<i64>,
    pub vcp_number: Option<u16>,
    pub cuts: Vec<CutAcquisition>,
    pub source_passes: Option<wxdata::live_pass::PassInventory>,
    pub source_sequences: Option<wxdata::live_sequence::SequenceInventory>,
}

/// An immutable accepted receipt. Equality is runtime receipt identity, never a persisted pass ID.
/// Clones share the small summary; they do not retain decoded gate buffers.
#[derive(Clone, Debug)]
pub struct AcquisitionSnapshot {
    site: Option<String>,
    inventory: std::sync::Arc<AcquisitionInventory>,
}

impl AcquisitionSnapshot {
    pub fn inventory(&self) -> &AcquisitionInventory {
        &self.inventory
    }

    pub fn matches_site(&self, site: Option<&str>) -> bool {
        self.site.as_deref() == site
    }

    /// Conservative summary capacity charge; shared receipts may be charged once per cache entry.
    pub fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.site.as_ref().map_or(0, String::capacity)
            + std::mem::size_of::<AcquisitionInventory>()
            + self.inventory.cuts.capacity() * std::mem::size_of::<CutAcquisition>()
            + self
                .inventory
                .source_sequences
                .as_ref()
                .map_or(0, |history| history.estimated_dynamic_bytes())
            + self.inventory.source_passes.as_ref().map_or(0, |history| {
                history.passes.capacity() * std::mem::size_of::<wxdata::live_pass::PassSummary>()
                    + history
                        .passes
                        .iter()
                        .map(|pass| {
                            pass.bounded_unobserved_spans.capacity()
                                * std::mem::size_of::<(u16, u16)>()
                        })
                        .sum::<usize>()
            })
            + self
                .inventory
                .cuts
                .iter()
                .map(|cut| {
                    cut.unobserved_chunks.capacity() * std::mem::size_of::<usize>()
                        + cut.internal_unobserved_spans.capacity()
                            * std::mem::size_of::<(u16, u16)>()
                })
                .sum::<usize>()
    }
}

impl PartialEq for AcquisitionSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.site == other.site && std::sync::Arc::ptr_eq(&self.inventory, &other.inventory)
    }
}
impl Eq for AcquisitionSnapshot {}
impl std::hash::Hash for AcquisitionSnapshot {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.site, state);
        std::hash::Hash::hash(&(std::sync::Arc::as_ptr(&self.inventory) as usize), state);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Progression {
    pub vcp_number: Option<u16>,
    pub expected_cuts: usize,
    pub observed_cuts: usize,
    pub completed_cuts: usize,
    /// All reported chunks received with no bounded raw holes. Not full radial/VCP coverage.
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
    source_passes: Option<wxdata::live_pass::PassInventory>,
    source_sequences: Option<wxdata::live_sequence::SequenceInventory>,
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
            if next_volume {
                self.source_passes = None;
                self.source_sequences = None;
            }
            self.observed_cuts.clear();
            self.progress = None;
            self.progress_vcp_number = progress.vcp_number;
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
    pub fn observe_radials(&mut self, coverage: RadialCoverage, received: DateTime<Utc>) -> bool {
        let p = coverage.progress;
        if !(1..=64).contains(&p.total_elevations)
            || !(1..=p.total_elevations).contains(&p.elevation_number)
            || !(1..=64).contains(&p.chunks_in_sweep)
            || !(1..=p.chunks_in_sweep).contains(&p.chunk_index)
        {
            return false;
        }
        self.progress(p, received);
        if (p.volume_start_ms.is_some()
            && self.latest_progress_volume_start_ms != p.volume_start_ms)
            || (p.vcp_number.is_some() && self.progress_vcp_number != p.vcp_number)
        {
            return false;
        }
        let Some(cut) = self
            .observed_cuts
            .get_mut(p.elevation_number.saturating_sub(1))
            .and_then(Option::as_mut)
        else {
            return false;
        };
        if let Some(history) = &coverage.source_passes {
            self.source_passes = Some(history.clone());
        } else if let Some(history) = &mut self.source_passes {
            history.unclassified_updates = history.unclassified_updates.saturating_add(1);
        }
        if let Some(history) = &coverage.source_sequences {
            self.source_sequences = Some((**history).clone());
        } else if let Some(history) = &mut self.source_sequences {
            history.unavailable_updates = history.unavailable_updates.saturating_add(1);
        }
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
            cut.raw_chunks.fill(false);
            cut.radial_seen.fill(false);
            cut.radial_times_ms.fill(0);
            self.progress = Some(p);
        }
        cut.chunks[p.chunk_index - 1] = true;
        cut.raw_chunks[p.chunk_index - 1] = true;
        for (number, time) in coverage.radials {
            let Some(index) = number.checked_sub(1).map(usize::from) else {
                continue; // the provider contract is one-based; zero is not radial #1
            };
            if let Some(seen) = cut.radial_seen.get_mut(index) {
                *seen = true;
                if time > 0 {
                    let slot = &mut cut.radial_times_ms[index];
                    *slot = (*slot).max(time);
                }
            }
        }
        true
    }

    /// Called with the accepted Update's raw envelope, never an independently polled progress event.
    pub fn capture_acquisition(
        &mut self,
        coverage: RadialCoverage,
        received: DateTime<Utc>,
    ) -> Option<AcquisitionSnapshot> {
        if !self.observe_radials(coverage, received) {
            return None;
        }
        Some(AcquisitionSnapshot {
            site: self.site.clone(),
            inventory: std::sync::Arc::new(self.acquisition_inventory()?),
        })
    }

    /// Snapshot only the raw evidence accepted for this receiver volume. Angular zeros in a
    /// merged/rendered sweep are never used to infer arrivals, pass identity or transport gaps.
    pub fn acquisition_inventory(&self) -> Option<AcquisitionInventory> {
        if self.observed_cuts.is_empty() {
            return None;
        }
        let cuts = self
            .observed_cuts
            .iter()
            .enumerate()
            .map(|(index, cut)| {
                let Some(cut) = cut else {
                    return CutAcquisition {
                        number: index + 1,
                        kind: None,
                        angle_deg: None,
                        received_chunks: 0,
                        expected_chunks: None,
                        unobserved_chunks: Vec::new(),
                        observed_radials: None,
                        raw_chunks: 0,
                        unknown_clock_radials: 0,
                        known_interval_ms: None,
                        internal_unobserved_spans: Vec::new(),
                    };
                };
                let raw_chunks = cut.raw_chunks.iter().filter(|seen| **seen).count();
                let mut known = cut.radial_times_ms.iter().copied().filter(|time| *time > 0);
                let known_interval_ms = known.next().map(|first| {
                    known.fold((first, first), |(start, end), time| {
                        (start.min(time), end.max(time))
                    })
                });
                CutAcquisition {
                    number: index + 1,
                    kind: Some(cut.kind),
                    angle_deg: cut.angle_deg.is_finite().then_some(cut.angle_deg),
                    received_chunks: cut.chunks.iter().filter(|seen| **seen).count(),
                    expected_chunks: Some(cut.chunks.len()),
                    unobserved_chunks: cut
                        .chunks
                        .iter()
                        .enumerate()
                        .filter_map(|(i, seen)| (!seen).then_some(i + 1))
                        .collect(),
                    observed_radials: (raw_chunks > 0)
                        .then(|| cut.radial_seen.iter().filter(|seen| **seen).count()),
                    raw_chunks,
                    unknown_clock_radials: cut
                        .radial_seen
                        .iter()
                        .zip(&cut.radial_times_ms)
                        .filter(|(seen, time)| **seen && **time <= 0)
                        .count(),
                    known_interval_ms,
                    internal_unobserved_spans: cut.radial_gaps(),
                }
            })
            .collect();
        Some(AcquisitionInventory {
            volume_start_ms: self.latest_progress_volume_start_ms,
            vcp_number: self.progress_vcp_number,
            cuts,
            source_passes: self.source_passes.clone(),
            source_sequences: self.source_sequences.clone(),
        })
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

/// Controlled raw arrival envelope shared by accepted-source ownership and UI tests.
#[cfg(test)]
pub(crate) fn acquisition_fixture(site: &str) -> (LiveScan, AcquisitionSnapshot) {
    let mut receiver = LiveScan::default();
    receiver.reset(Some(site.into()));
    let start = 1_700_000_000_000;
    let receipt = receiver
        .capture_acquisition(
            RadialCoverage {
                progress: ScanProgress {
                    volume_start_ms: Some(start),
                    vcp_number: Some(212),
                    cut_kind: CutKind::Standard,
                    elevation_number: 1,
                    total_elevations: 3,
                    elevation_angle_deg: 0.5,
                    azimuth_rate_dps: 18.0,
                    azimuth_start_deg: 0.0,
                    azimuth_end_deg: 120.0,
                    chunk_index: 1,
                    chunks_in_sweep: 3,
                },
                source_passes: None,
                source_sequences: None,
                radials: vec![(1, start + 1000), (2, 0), (4, start + 3000)],
            },
            Utc::now(),
        )
        .unwrap();
    (receiver, receipt)
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

    fn raw(state: &mut LiveScan, p: ScanProgress, radials: &[(u16, i64)]) {
        state.observe_radials(
            RadialCoverage {
                progress: p,
                source_passes: None,
                source_sequences: None,
                radials: radials.to_vec(),
            },
            Utc::now(),
        );
    }

    #[test]
    fn sequence_receipts_survive_recovery_and_vcp_change_without_rewriting_accepted_frames() {
        use wxdata::live_sequence::{SequenceLedger, SequenceOrigin};
        let mut receiver = LiveScan::default();
        receiver.reset(Some("KTLX".into()));
        let mut ledger = SequenceLedger::new(SequenceOrigin::UnidataChunks);
        ledger.observe(1);
        ledger.download_failed(2);
        ledger.observe(3);
        let envelope =
            |p, sequence: Option<wxdata::live_sequence::SequenceInventory>| RadialCoverage {
                progress: p,
                radials: vec![(1, 2000)],
                source_passes: None,
                source_sequences: sequence.map(Box::new),
            };
        let first = receiver
            .capture_acquisition(
                envelope(progress(1, 1), Some(ledger.inventory())),
                Utc::now(),
            )
            .unwrap();
        ledger.observe(2);
        let mut p = progress(1, 1);
        p.vcp_number = Some(35);
        let recovered = receiver
            .capture_acquisition(envelope(p, Some(ledger.inventory())), Utc::now())
            .unwrap();
        let before = first.inventory().source_sequences.as_ref().unwrap();
        let after = recovered.inventory().source_sequences.as_ref().unwrap();
        assert_eq!(before.bounded_holes, [(2, 2)]);
        assert_eq!(before.failed_request_spans, [(2, 2)]);
        assert!(after.bounded_holes.is_empty());
        assert_eq!(after.recovered_spans, [(2, 2)]);
        assert!(
            first.estimated_bytes()
                >= std::mem::size_of::<AcquisitionInventory>() + before.estimated_dynamic_bytes()
        );
        let legacy = receiver
            .capture_acquisition(envelope(p, None), Utc::now())
            .unwrap();
        assert_eq!(
            legacy
                .inventory()
                .source_sequences
                .as_ref()
                .unwrap()
                .unavailable_updates,
            1
        );
        assert_eq!(after.unavailable_updates, 0);
        p.volume_start_ms = Some(5000);
        let rollover = receiver
            .capture_acquisition(envelope(p, None), Utc::now())
            .unwrap();
        assert!(rollover.inventory().source_sequences.is_none());
        receiver.reset(Some("KPAH".into()));
        assert!(receiver.acquisition_inventory().is_none());
        assert!(first.matches_site(Some("KTLX")));
        assert_eq!(before.bounded_holes, [(2, 2)]);
    }

    #[test]
    fn source_pass_history_survives_vcp_changes_and_inferred_revisits_with_the_accepted_receipt() {
        use wxdata::live_pass::{PassArrival, PassKey, PassLedger};
        let mut receiver = LiveScan::default();
        receiver.reset(Some("KTLX".into()));
        let arrival = |start, end| PassArrival {
            elevation_number: 1,
            key: Some(PassKey {
                elevation_number: 1,
                start_ms: start,
            }),
            start_marker: true,
            end_marker: end,
            radials: vec![(1, start), (3, 0)],
        };
        let mut provider = PassLedger::default();
        provider.observe(&[arrival(2000, true)]);
        let first = receiver
            .capture_acquisition(
                RadialCoverage {
                    progress: progress(1, 1),
                    radials: vec![(1, 2000), (3, 0)],
                    source_passes: Some(provider.inventory()),
                    source_sequences: None,
                },
                Utc::now(),
            )
            .unwrap();
        provider.observe(&[arrival(100000, false)]);
        let second = receiver
            .capture_acquisition(
                RadialCoverage {
                    progress: ScanProgress {
                        vcp_number: Some(35),
                        ..progress(1, 1)
                    },
                    radials: vec![(1, 100000)],
                    source_passes: Some(provider.inventory()),
                    source_sequences: None,
                },
                Utc::now(),
            )
            .unwrap();
        assert_eq!(
            first
                .inventory()
                .source_passes
                .as_ref()
                .unwrap()
                .passes
                .len(),
            1
        );
        let history = second.inventory().source_passes.as_ref().unwrap();
        assert_eq!(
            history.passes.len(),
            2,
            "VCP change does not erase source-marked history within this volume"
        );
        assert_eq!(history.passes[0].key.start_ms, 2000);
        assert!(history.passes[0].end_marker);
        assert!(!history.passes[1].end_marker);
        assert!(second.estimated_bytes() > first.estimated_bytes());
        let legacy = receiver
            .capture_acquisition(
                RadialCoverage {
                    progress: ScanProgress {
                        vcp_number: Some(35),
                        ..progress(1, 1)
                    },
                    radials: vec![(2, 100010)],
                    source_passes: None,
                    source_sequences: None,
                },
                Utc::now(),
            )
            .unwrap();
        assert_eq!(
            legacy
                .inventory()
                .source_passes
                .as_ref()
                .unwrap()
                .unclassified_updates,
            1
        );
        assert_eq!(
            history.unclassified_updates, 0,
            "accepted snapshots remain immutable"
        );
        let mut provider = PassLedger::default();
        provider.observe(&[arrival(201000, false)]);
        let next_volume = receiver
            .capture_acquisition(
                RadialCoverage {
                    progress: ScanProgress {
                        volume_start_ms: Some(200000),
                        ..progress(1, 1)
                    },
                    radials: vec![(1, 201000)],
                    source_passes: Some(provider.inventory()),
                    source_sequences: None,
                },
                Utc::now(),
            )
            .unwrap();
        assert_eq!(
            next_volume
                .inventory()
                .source_passes
                .as_ref()
                .unwrap()
                .passes
                .len(),
            1
        );
        assert_eq!(history.passes.len(), 2);
        receiver.reset(Some("KPAH".into()));
        assert_eq!(receiver.acquisition_inventory(), None);
        assert!(second.matches_site(Some("KTLX")));
    }

    #[test]
    fn accepted_receipt_survives_receiver_advance_reset_and_rejected_envelopes() {
        use std::collections::HashSet;
        let (mut receiver, receipt) = acquisition_fixture("KTLX");
        let mut identities = HashSet::new();
        identities.insert(receipt.clone());
        assert!(identities.contains(&receipt.clone()));
        assert!(receipt.matches_site(Some("KTLX")));
        assert!(!receipt.matches_site(Some("KPAH")));
        assert!(!receipt.matches_site(None));
        let original = receipt.inventory().clone();
        assert!(
            receipt.estimated_bytes()
                >= std::mem::size_of::<AcquisitionInventory>()
                    + original.cuts.len() * std::mem::size_of::<CutAcquisition>()
        );
        let mut p = receiver.progress.unwrap();
        let duplicate = receiver
            .capture_acquisition(
                RadialCoverage {
                    progress: p,
                    source_passes: None,
                    source_sequences: None,
                    radials: vec![(1, 1_700_000_001_000)],
                },
                Utc::now(),
            )
            .unwrap();
        assert_eq!(receipt.inventory(), duplicate.inventory());
        assert_ne!(
            receipt, duplicate,
            "equal summaries are separate accepted receipts"
        );
        p.elevation_number = 2;
        p.cut_kind = CutKind::Sails;
        raw(&mut receiver, p, &[(1, 1_700_000_010_000)]);
        assert_eq!(receipt.inventory(), &original);
        assert_ne!(receiver.acquisition_inventory().as_ref(), Some(&original));
        p.elevation_number = 0;
        assert!(receiver
            .capture_acquisition(
                RadialCoverage {
                    progress: p,
                    source_passes: None,
                    source_sequences: None,
                    radials: vec![],
                },
                Utc::now()
            )
            .is_none());
        p.elevation_number = 1;
        p.volume_start_ms = Some(1000);
        assert!(receiver
            .capture_acquisition(
                RadialCoverage {
                    progress: p,
                    source_passes: None,
                    source_sequences: None,
                    radials: vec![],
                },
                Utc::now()
            )
            .is_none());
        receiver.reset(Some("KPAH".into()));
        assert_eq!(receiver.acquisition_inventory(), None);
        assert_eq!(receipt.inventory(), &original);
    }

    #[test]
    fn raw_inventory_separates_unobserved_positions_from_unknown_clocks_and_progress_only() {
        let mut state = LiveScan::default();
        assert_eq!(state.acquisition_inventory(), None);
        state.progress(progress(1, 1), Utc::now());
        let metadata = state.acquisition_inventory().unwrap();
        assert_eq!(metadata.cuts[0].observed_radials, None);
        assert_eq!(metadata.cuts[1].expected_chunks, None);
        raw(
            &mut state,
            progress(1, 1),
            &[(1, 2000), (2, 0), (4, 4000), (0, 5000), (900, 5000)],
        );
        let snapshot = state.acquisition_inventory().unwrap();
        let cut = &snapshot.cuts[0];
        assert_eq!(cut.observed_radials, Some(3));
        assert_eq!(cut.unknown_clock_radials, 1);
        assert_eq!(cut.known_interval_ms, Some((2000, 4000)));
        assert_eq!(cut.internal_unobserved_spans, [(3, 3)]);
        assert_eq!(cut.unobserved_chunks, [2, 3]);
        assert_eq!(state.radial_gaps(), [(3, 3)]);
        raw(&mut state, progress(1, 1), &[(3, 0), (2, 3000), (1, 1900)]);
        let filled = state.acquisition_inventory().unwrap();
        assert_eq!(filled.cuts[0].observed_radials, Some(4));
        assert_eq!(filled.cuts[0].unknown_clock_radials, 1);
        assert_eq!(filled.cuts[0].known_interval_ms, Some((2000, 4000)));
        assert!(filled.cuts[0].internal_unobserved_spans.is_empty());
        assert_eq!(
            snapshot.cuts[0].internal_unobserved_spans,
            [(3, 3)],
            "snapshot is immutable across gap fill"
        );
    }

    #[test]
    fn raw_inventory_retains_supplemental_equal_angle_cuts_and_late_gap_fill() {
        let mut state = LiveScan::default();
        raw(&mut state, progress(1, 1), &[(1, 2000), (3, 2100)]);
        raw(
            &mut state,
            ScanProgress {
                cut_kind: CutKind::Sails,
                ..progress(2, 1)
            },
            &[(1, 8000)],
        );
        raw(
            &mut state,
            ScanProgress {
                cut_kind: CutKind::Mrle,
                ..progress(3, 1)
            },
            &[(1, 10000)],
        );
        raw(&mut state, progress(1, 1), &[(2, 2050)]);
        let inventory = state.acquisition_inventory().unwrap();
        assert_eq!(inventory.cuts.len(), 3);
        assert_eq!(inventory.cuts[0].observed_radials, Some(3));
        assert!(inventory.cuts[0].internal_unobserved_spans.is_empty());
        assert_eq!(inventory.cuts[1].kind, Some(CutKind::Sails));
        assert_eq!(inventory.cuts[2].kind, Some(CutKind::Mrle));
        assert!(inventory.cuts.iter().all(|cut| cut.angle_deg == Some(0.5)));
        assert_eq!(state.progress.unwrap().elevation_number, 3);
    }

    #[test]
    fn raw_inventory_resets_at_volume_vcp_and_inferred_revisit_boundaries() {
        let mut state = LiveScan::default();
        raw(&mut state, progress(1, 1), &[(1, 2000), (2, 0)]);
        // Existing rotation-time reset is an inference, not a persistent pass identifier.
        raw(&mut state, progress(1, 1), &[(4, 100000)]);
        assert_eq!(
            state.acquisition_inventory().unwrap().cuts[0].observed_radials,
            Some(1)
        );
        let next = ScanProgress {
            volume_start_ms: Some(200000),
            ..progress(2, 2)
        };
        raw(&mut state, next, &[(121, 201000), (123, 202000)]);
        let snapshot = state.acquisition_inventory().unwrap();
        assert_eq!(snapshot.volume_start_ms, Some(200000));
        assert_eq!(snapshot.cuts[0].kind, None);
        assert_eq!(
            snapshot.cuts[1].unobserved_chunks,
            [1, 3],
            "mid-volume join does not fill unseen sectors"
        );
        raw(&mut state, progress(1, 1), &[(1, 199000)]);
        assert_eq!(
            state.acquisition_inventory().unwrap(),
            snapshot,
            "late old volume is rejected"
        );
        raw(
            &mut state,
            ScanProgress {
                vcp_number: Some(35),
                ..next
            },
            &[],
        );
        let changed = state.acquisition_inventory().unwrap();
        assert_eq!(changed.vcp_number, Some(35));
        assert_eq!(changed.cuts[1].observed_radials, Some(0));
        assert_eq!(changed.cuts[1].known_interval_ms, None);
        state.reset(Some("KOUN".into()));
        assert_eq!(state.acquisition_inventory(), None);
    }

    #[test]
    fn raw_inventory_does_not_carry_a_vcp_into_a_new_volume_with_unknown_metadata() {
        let mut state = LiveScan::default();
        raw(&mut state, progress(1, 1), &[(1, 2000)]);
        raw(
            &mut state,
            ScanProgress {
                volume_start_ms: Some(3000),
                vcp_number: None,
                ..progress(1, 1)
            },
            &[(1, 4000)],
        );
        let inventory = state.acquisition_inventory().unwrap();
        assert_eq!(inventory.vcp_number, None);
        assert_eq!(inventory.volume_start_ms, Some(3000));
        assert_eq!(inventory.cuts[0].known_interval_ms, Some((4000, 4000)));
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
