//! Source-marked elevation passes, before stitching merges repeated cuts by azimuth.
//! A pass key comes from a recorded boundary radial and its radar clock, never a local counter,
//! angle, rotation duration or provider receipt clock. A mid-cut join remains unanchored.

use crate::level2::Scan;
use nexrad_model::data::RadialStatus;
use std::collections::{BTreeMap, HashSet, VecDeque};

/// Scoped to the source radar by the enclosing acquisition receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PassKey {
    pub elevation_number: u16,
    pub start_ms: i64,
}

/// Raw positions from one source-marked segment in the arriving decoded input.
#[derive(Clone, Debug)]
pub struct PassArrival {
    pub elevation_number: u16,
    pub key: Option<PassKey>,
    pub start_marker: bool,
    pub end_marker: bool,
    pub radials: Vec<(u16, i64)>,
}

#[derive(Clone, Copy)]
struct ActivePass {
    key: Option<PassKey>,
    closed: bool,
}

/// Bounded duplicate tracking prevents the repeated metadata/backfill prefix of an assembly
/// from reopening an older pass. No gates are held. Reset at a source volume rollover.
#[derive(Default)]
pub struct PassTracker {
    active: BTreeMap<u16, ActivePass>,
    seen: HashSet<(u16, u16, i64, RadialStatus)>,
    order: VecDeque<(u16, u16, i64, RadialStatus)>,
    last_elevation: Option<u16>,
    ledger: PassLedger,
}

const MAX_FINGERPRINTS: usize = 128 * 720;
pub const MAX_RETAINED_PASSES: usize = 128;

impl PassTracker {
    /// `continuous` is true only for consecutive source transport sequences in this volume.
    /// If bytes were skipped or decoding failed, no earlier boundary is lent to the new input.
    pub fn observe(&mut self, scan: &Scan, continuous: bool) -> Vec<PassArrival> {
        self.observe_inner(scan, continuous, true)
    }

    /// Coalesced bytes contain a known source discontinuity, but decoded radials do not identify
    /// which side of it they came from. Keep native start IDs, without assigning ordinary positions
    /// or end markers across a potentially missing boundary. Do not lend these starts to later input.
    pub fn observe_discontinuous_assembly(&mut self, scan: &Scan) -> Vec<PassArrival> {
        let arrivals = self.observe_inner(scan, false, false);
        self.active.clear();
        self.last_elevation = None;
        self.ledger.record_discontinuous_assembly();
        arrivals
    }

    fn observe_inner(
        &mut self,
        scan: &Scan,
        continuous: bool,
        associate_positions: bool,
    ) -> Vec<PassArrival> {
        if !continuous {
            self.active.clear();
            self.last_elevation = None;
        }
        let mut arrivals: Vec<PassArrival> = Vec::new();
        let mut local = BTreeMap::<u16, PassKey>::new();
        for radial in scan.sweeps().iter().flat_map(|sweep| sweep.radials()) {
            let elevation = u16::from(radial.elevation_number());
            let number = radial.azimuth_number();
            if !(1..=64).contains(&elevation) || !(1..=720).contains(&number) {
                continue;
            }
            let raw_time = radial.collection_timestamp();
            let time =
                if raw_time > 0 && chrono::DateTime::from_timestamp_millis(raw_time).is_some() {
                    raw_time
                } else {
                    0
                };
            let status = radial.radial_status();
            let fingerprint = (elevation, number, time, status);
            if !self.seen.insert(fingerprint) {
                // An untimed boundary is not uniquely identified by this fingerprint. It might
                // be repeated backfill or a new boundary with another unknown clock. Fail closed
                // instead of lending a timed pass's anchor across it.
                if time == 0
                    && matches!(
                        status,
                        RadialStatus::ScanStart
                            | RadialStatus::ElevationStart
                            | RadialStatus::ElevationStartVCPFinal
                            | RadialStatus::ElevationEnd
                            | RadialStatus::ScanEnd
                    )
                {
                    self.active.clear();
                    local.clear();
                    self.last_elevation = None;
                }
                continue;
            }
            self.order.push_back(fingerprint);
            if self.order.len() > MAX_FINGERPRINTS {
                self.seen
                    .remove(&self.order.pop_front().expect("bounded duplicate history"));
            }
            if self.last_elevation != Some(elevation) {
                if let Some(previous) = self
                    .last_elevation
                    .and_then(|elevation| self.active.get_mut(&elevation))
                {
                    previous.closed = true;
                }
                self.last_elevation = Some(elevation);
            }
            let starts = matches!(
                status,
                RadialStatus::ScanStart
                    | RadialStatus::ElevationStart
                    | RadialStatus::ElevationStartVCPFinal
            );
            let ends = matches!(status, RadialStatus::ElevationEnd | RadialStatus::ScanEnd);
            let declared =
                (starts && time > 0 && chrono::DateTime::from_timestamp_millis(time).is_some())
                    .then_some(PassKey {
                        elevation_number: elevation,
                        start_ms: time,
                    });
            if starts {
                if let Some(key) = declared {
                    local.insert(elevation, key);
                } else {
                    local.remove(&elevation);
                }
                let newer =
                    self.active
                        .get(&elevation)
                        .is_none_or(|active| match (active.key, declared) {
                            (Some(old), Some(new)) => new.start_ms > old.start_ms,
                            _ => true,
                        });
                if newer {
                    self.active.insert(
                        elevation,
                        ActivePass {
                            key: declared,
                            closed: false,
                        },
                    );
                }
            }
            let active = self.active.get(&elevation).copied();
            let key = if starts {
                declared
            } else if !associate_positions {
                None
            } else {
                let current = active
                    .filter(|active| !active.closed)
                    .and_then(|active| active.key);
                match (
                    local.get(&elevation).copied(),
                    active.and_then(|active| active.key),
                ) {
                    (Some(local), Some(latest))
                        if latest.start_ms > local.start_ms && time >= latest.start_ms =>
                    {
                        current
                    }
                    (Some(local), _) => Some(local),
                    _ => current,
                }
                .filter(|key| time <= 0 || time >= key.start_ms)
            };
            let same_segment = arrivals.last().is_some_and(|last| {
                last.elevation_number == elevation && last.key == key && !starts && !last.end_marker
            });
            if !same_segment {
                arrivals.push(PassArrival {
                    elevation_number: elevation,
                    key,
                    start_marker: starts,
                    end_marker: false,
                    radials: Vec::new(),
                });
            }
            let arrival = arrivals.last_mut().expect("source segment created");
            arrival.radials.push((number, time));
            arrival.end_marker |= ends;
            if ends {
                local.remove(&elevation);
                if let Some(active) = self.active.get_mut(&elevation) {
                    if key == active.key {
                        active.closed = true;
                    }
                }
            }
        }
        self.ledger.observe(&arrivals);
        arrivals
    }

    /// Includes decoded source evidence that did not replace any displayed gates.
    pub fn inventory(&self) -> PassInventory {
        self.ledger.inventory()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PassSummary {
    pub key: PassKey,
    pub start_marker: bool,
    pub end_marker: bool,
    pub observed_positions: usize,
    pub unknown_clock_positions: usize,
    pub known_interval_ms: Option<(i64, i64)>,
    pub bounded_unobserved_spans: Vec<(u16, u16)>,
}

#[derive(Clone, Debug, Default)]
struct PassObservation {
    positions: BTreeMap<u16, i64>,
    start_marker: bool,
    end_marker: bool,
}

/// One volume's bounded pass history. Unanchored arrivals stay separate instead of acquiring
/// an invented base/repeat index. The UI's scan-local ordinal inventory remains independent.
#[derive(Clone, Debug, Default)]
pub struct PassLedger {
    passes: BTreeMap<PassKey, PassObservation>,
    unanchored: BTreeMap<u16, BTreeMap<u16, i64>>,
    retired: usize,
    unclassified_updates: usize,
    discontinuous_assemblies: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PassInventory {
    pub passes: Vec<PassSummary>,
    /// Positions deduplicated within each cut's unanchored arrivals, not a pass count.
    pub unanchored_positions: usize,
    pub unanchored_unknown_clocks: usize,
    pub retired_passes: usize,
    pub unclassified_updates: usize,
    /// Decoded assemblies whose known source discontinuity makes native pass association unsafe.
    /// This counts inspected assemblies, not missing messages or radials.
    pub discontinuous_assemblies: usize,
}

impl PassLedger {
    pub fn record_discontinuous_assembly(&mut self) {
        self.discontinuous_assemblies = self.discontinuous_assemblies.saturating_add(1);
    }
    pub fn record_unclassified(&mut self) {
        self.unclassified_updates = self.unclassified_updates.saturating_add(1);
    }
    pub fn observe(&mut self, arrivals: &[PassArrival]) {
        for arrival in arrivals {
            if !(1..=64).contains(&arrival.elevation_number) {
                continue;
            }
            let key = arrival.key.filter(|key| {
                key.elevation_number == arrival.elevation_number
                    && key.start_ms > 0
                    && chrono::DateTime::from_timestamp_millis(key.start_ms).is_some()
                    && !arrival
                        .radials
                        .iter()
                        .any(|(_, time)| *time > 0 && *time < key.start_ms)
            });
            let positions = if let Some(key) = key {
                // Keep the newest source passes; delayed input cannot cycle an evicted pass back.
                if self.passes.len() >= MAX_RETAINED_PASSES && !self.passes.contains_key(&key) {
                    let oldest = *self
                        .passes
                        .keys()
                        .min_by_key(|key| key.start_ms)
                        .expect("full pass history");
                    if key.start_ms <= oldest.start_ms {
                        continue;
                    }
                    self.passes.remove(&oldest);
                    self.retired = self.retired.saturating_add(1);
                }
                let pass = self.passes.entry(key).or_default();
                pass.start_marker |= arrival.start_marker;
                pass.end_marker |= arrival.end_marker;
                &mut pass.positions
            } else {
                self.unanchored.entry(arrival.elevation_number).or_default()
            };
            for &(number, time) in &arrival.radials {
                if (1..=720).contains(&number) {
                    let clock = positions.entry(number).or_default();
                    let time =
                        if time > 0 && chrono::DateTime::from_timestamp_millis(time).is_some() {
                            time
                        } else {
                            0
                        };
                    *clock = (*clock).max(time);
                }
            }
        }
    }

    pub fn inventory(&self) -> PassInventory {
        let mut passes: Vec<_> = self
            .passes
            .iter()
            .map(|(&key, pass)| {
                let times = || pass.positions.values().copied().filter(|time| *time > 0);
                let mut gaps = Vec::new();
                let mut previous = None;
                for &number in pass.positions.keys() {
                    if let Some(previous) = previous {
                        if number > previous + 1 {
                            gaps.push((previous + 1, number - 1));
                        }
                    }
                    previous = Some(number);
                }
                PassSummary {
                    key,
                    start_marker: pass.start_marker,
                    end_marker: pass.end_marker,
                    observed_positions: pass.positions.len(),
                    unknown_clock_positions: pass
                        .positions
                        .values()
                        .filter(|time| **time <= 0)
                        .count(),
                    known_interval_ms: times().min().zip(times().max()),
                    bounded_unobserved_spans: gaps,
                }
            })
            .collect();
        passes.sort_by_key(|pass| (pass.key.start_ms, pass.key.elevation_number));
        PassInventory {
            passes,
            unanchored_positions: self.unanchored.values().map(BTreeMap::len).sum(),
            unanchored_unknown_clocks: self
                .unanchored
                .values()
                .flat_map(|positions| positions.values())
                .filter(|time| **time <= 0)
                .count(),
            retired_passes: self.retired,
            unclassified_updates: self.unclassified_updates,
            discontinuous_assemblies: self.discontinuous_assemblies,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexrad_model::data::{Radial, Sweep};

    fn fixture() -> Scan {
        crate::level2::decode_volume(
            include_bytes!("../tests/data/corpus/mayfield-2021-first-records.ar2").to_vec(),
        )
        .unwrap()
    }

    fn input(samples: &[(u8, u16, i64, RadialStatus)]) -> Scan {
        let radials = samples
            .iter()
            .map(|&(elevation, number, time, status)| {
                Radial::new(
                    time,
                    number,
                    number as f32,
                    1.0,
                    status,
                    elevation,
                    0.5,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
            })
            .collect();
        Scan::new(
            fixture().coverage_pattern().clone(),
            Sweep::from_radials(radials),
        )
    }

    #[test]
    fn native_boundaries_separate_equal_angle_revisits_without_rotation_time_heuristics() {
        use RadialStatus::*;
        let mut tracker = PassTracker::default();
        let mut ledger = PassLedger::default();
        let base = tracker.observe(
            &input(&[(1, 1, 1000, ScanStart), (1, 3, 0, IntermediateRadialData)]),
            false,
        );
        ledger.observe(&base);
        let first_snapshot = ledger.inventory();
        let fill = tracker.observe(
            &input(&[
                (1, 2, 0, IntermediateRadialData),
                (1, 4, 1300, ElevationEnd),
            ]),
            true,
        );
        assert_eq!(
            fill[0].key, base[0].key,
            "consecutive source bytes retain their boundary anchor"
        );
        ledger.observe(&fill);
        ledger.observe(&tracker.observe(
            &input(&[(1, 1, 1400, ElevationStart), (1, 2, 1500, ElevationEnd)]),
            true,
        ));
        let inventory = ledger.inventory();
        assert_eq!(
            inventory
                .passes
                .iter()
                .map(|pass| pass.key.start_ms)
                .collect::<Vec<_>>(),
            [1000, 1400]
        );
        assert_eq!(inventory.passes[0].observed_positions, 4);
        assert_eq!(inventory.passes[0].unknown_clock_positions, 2);
        assert!(inventory.passes[0].end_marker);
        assert!(inventory.passes[0].bounded_unobserved_spans.is_empty());
        assert_eq!(first_snapshot.passes[0].bounded_unobserved_spans, [(2, 2)]);
        assert!(!first_snapshot.passes[0].end_marker);
    }

    #[test]
    fn joins_unknown_boundary_clocks_and_sequence_discontinuities_do_not_invent_identity() {
        use RadialStatus::*;
        let mut tracker = PassTracker::default();
        let mut ledger = PassLedger::default();
        ledger.observe(&tracker.observe(&input(&[(1, 121, 2000, IntermediateRadialData)]), false));
        ledger.observe(&tracker.observe(
            &input(&[
                (1, 1, 0, ElevationStart),
                (1, 2, 3000, IntermediateRadialData),
            ]),
            true,
        ));
        assert!(
            ledger.inventory().passes.is_empty(),
            "later clocks do not date an untimed boundary"
        );
        assert_eq!(ledger.inventory().unanchored_positions, 3);
        assert_eq!(ledger.inventory().unanchored_unknown_clocks, 1);
        ledger.observe(&tracker.observe(&input(&[(1, 1, 4000, ElevationStart)]), true));
        let interrupted = tracker.observe(&input(&[(1, 2, 4100, IntermediateRadialData)]), false);
        assert_eq!(
            interrupted[0].key, None,
            "missing source bytes cannot lend the earlier anchor"
        );
        ledger.observe(&interrupted);
        let after_end = tracker.observe(
            &input(&[
                (1, 1, 5000, ElevationStart),
                (1, 2, 5100, ElevationEnd),
                (1, 3, 5200, IntermediateRadialData),
            ]),
            true,
        );
        assert_eq!(after_end.last().unwrap().key, None);
        let mut switched = PassTracker::default();
        switched.observe(&input(&[(1, 1, 1000, ScanStart)]), false);
        switched.observe(&input(&[(2, 1, 2000, ElevationStartVCPFinal)]), true);
        assert_eq!(
            switched.observe(&input(&[(1, 2, 3000, IntermediateRadialData)]), true)[0].key,
            None
        );
    }

    #[test]
    fn duplicate_prefixes_do_not_reopen_old_passes_and_late_marked_fill_keeps_its_source() {
        use RadialStatus::*;
        let old = [(1, 1, 1000, ScanStart), (1, 3, 1200, ElevationEnd)];
        let new = [
            (1, 1, 2000, ElevationStart),
            (1, 2, 2100, IntermediateRadialData),
        ];
        let mut tracker = PassTracker::default();
        let mut ledger = PassLedger::default();
        ledger.observe(&tracker.observe(&input(&old), false));
        ledger.observe(&tracker.observe(&input(&new), true));
        let prefixed = [old.as_slice(), &[(1, 3, 2200, ElevationEnd)]].concat();
        let arrival = tracker.observe(&input(&prefixed), true);
        assert_eq!(arrival.len(), 1);
        assert_eq!(arrival[0].key.unwrap().start_ms, 2000);
        ledger.observe(&arrival);
        assert!(tracker
            .observe(&input(&[old.as_slice(), new.as_slice()].concat()), true)
            .is_empty());
        let mut replay = PassTracker::default();
        ledger.observe(&replay.observe(
            &input(&[
                (1, 1, 1000, ScanStart),
                (1, 2, 1100, IntermediateRadialData),
                (1, 3, 1200, ElevationEnd),
            ]),
            false,
        ));
        assert_eq!(ledger.inventory().passes.len(), 2);
        assert!(ledger.inventory().passes[0]
            .bounded_unobserved_spans
            .is_empty());
        assert_eq!(ledger.inventory().passes[1].key.start_ms, 2000);
    }

    #[test]
    fn pinned_native_boundary_ids_survive_independent_decodes_and_do_not_hold_gates() {
        let first = PassTracker::default().observe(&fixture(), false);
        let second = PassTracker::default().observe(&fixture(), false);
        let ids = |arrivals: &[PassArrival]| {
            arrivals
                .iter()
                .filter_map(|arrival| arrival.key)
                .collect::<Vec<_>>()
        };
        assert!(
            !ids(&first).is_empty(),
            "the pinned partial scan carries native starts"
        );
        assert_eq!(ids(&first), ids(&second));
        for key in ids(&first) {
            assert!((1_639_192_800_000..1_639_193_400_000).contains(&key.start_ms));
        }
        let mut ledger = PassLedger::default();
        ledger.observe(&first);
        let snapshot = ledger.inventory();
        assert!(snapshot.passes.iter().all(|pass| pass.start_marker));
        assert!(
            snapshot.passes.iter().any(|pass| !pass.end_marker),
            "trimmed fixture is not a complete pass"
        );
    }

    #[test]
    fn provider_history_retains_late_pass_evidence_that_does_not_replace_displayed_gates() {
        use RadialStatus::*;
        let newest = input(&[
            (1, 1, 2000, ScanStart),
            (1, 2, 2100, IntermediateRadialData),
            (1, 3, 2200, ElevationEnd),
        ]);
        let delayed = input(&[
            (1, 1, 1000, ElevationStart),
            (1, 2, 1100, IntermediateRadialData),
            (1, 3, 1200, ElevationEnd),
        ]);
        let mut tracker = PassTracker::default();
        tracker.observe(&newest, false);
        let accepted = tracker.inventory();
        tracker.observe(&delayed, true);
        let (_, changed) = crate::live::merge_scan(&newest, delayed);
        assert!(
            changed.is_empty(),
            "older gates cannot replace the accepted pass"
        );
        tracker.observe(&input(&[(1, 1, 3000, ElevationStart)]), true);
        assert_eq!(
            tracker
                .inventory()
                .passes
                .iter()
                .map(|pass| pass.key.start_ms)
                .collect::<Vec<_>>(),
            [1000, 2000, 3000],
            "the next emitted receipt retains non-rendering decoded evidence"
        );
        assert_eq!(accepted.passes.len(), 1, "earlier receipts stay immutable");
    }

    #[test]
    fn discontinuous_assembly_retains_starts_without_lending_them_across_missing_boundaries() {
        use RadialStatus::*;
        let mut tracker = PassTracker::default();
        tracker.observe_discontinuous_assembly(&input(&[
            (1, 1, 1000, ScanStart),
            (1, 2, 1100, IntermediateRadialData),
            (1, 3, 1200, ElevationEnd),
            (1, 1, 2000, ElevationStart),
            (1, 2, 0, IntermediateRadialData),
        ]));
        let before = tracker.inventory();
        assert_eq!(before.discontinuous_assemblies, 1);
        assert_eq!(before.passes.len(), 2);
        assert!(before
            .passes
            .iter()
            .all(|pass| pass.observed_positions == 1 && !pass.end_marker));
        assert_eq!(before.unanchored_positions, 2);
        assert_eq!(
            before.unanchored_unknown_clocks, 0,
            "a known clock survives an untimed duplicate"
        );
        let following = tracker.observe(&input(&[(1, 3, 2200, IntermediateRadialData)]), true);
        assert_eq!(
            following[0].key, None,
            "a questionable anchor cannot reach later input"
        );
        tracker.observe(
            &input(&[(1, 1, 3000, ElevationStart), (1, 2, 3100, ElevationEnd)]),
            true,
        );
        let after = tracker.inventory();
        assert_eq!(after.passes.last().unwrap().observed_positions, 2);
        assert!(
            after.passes.last().unwrap().end_marker,
            "a fresh contiguous marked pass recovers"
        );
        assert_eq!(before.passes.len(), 2, "earlier receipts remain immutable");
    }

    #[test]
    fn repeated_untimed_boundary_cannot_lend_the_intervening_timed_pass_anchor() {
        use RadialStatus::*;
        for boundary in [ElevationStart, ElevationEnd] {
            for current_elevation in [1, 2] {
                let mut tracker = PassTracker::default();
                tracker.observe(&input(&[(1, 1, 0, boundary)]), false);
                tracker.observe(
                    &input(&[(current_elevation, 1, 4000, ElevationStart)]),
                    true,
                );
                let next = tracker.observe(
                    &input(&[
                        (1, 1, 0, boundary),
                        (current_elevation, 2, 4100, IntermediateRadialData),
                    ]),
                    true,
                );
                assert_eq!(
                    next.last().unwrap().key,
                    None,
                    "a duplicate untimed boundary remains ambiguous"
                );
                assert_eq!(
                    tracker.inventory().passes.len(),
                    1,
                    "no invented new pass ID"
                );
            }
        }
    }

    #[test]
    fn pass_history_is_bounded_and_unknown_input_availability_stays_explicit() {
        let mut ledger = PassLedger::default();
        for start_ms in 1..=MAX_RETAINED_PASSES as i64 + 1 {
            ledger.observe(&[PassArrival {
                elevation_number: 1,
                key: Some(PassKey {
                    elevation_number: 1,
                    start_ms,
                }),
                start_marker: true,
                end_marker: false,
                radials: vec![(1, start_ms)],
            }]);
        }
        let before = ledger.inventory();
        assert_eq!(before.passes.len(), MAX_RETAINED_PASSES);
        assert_eq!(before.retired_passes, 1);
        assert_eq!(before.passes[0].key.start_ms, 2);
        ledger.observe(&[PassArrival {
            elevation_number: 1,
            key: Some(PassKey {
                elevation_number: 1,
                start_ms: 1,
            }),
            start_marker: true,
            end_marker: false,
            radials: vec![(2, 10)],
        }]);
        assert_eq!(
            ledger.inventory(),
            before,
            "retired delayed pass cannot evict a newer one"
        );
        ledger.record_unclassified();
        assert_eq!(ledger.inventory().unclassified_updates, 1);
        assert_eq!(before.unclassified_updates, 0);
        let mut invalid = PassLedger::default();
        invalid.observe(&[PassArrival {
            elevation_number: 1,
            key: Some(PassKey {
                elevation_number: 2,
                start_ms: 1,
            }),
            start_marker: true,
            end_marker: false,
            radials: vec![(0, 1), (721, 1), (2, i64::MAX)],
        }]);
        assert!(invalid.inventory().passes.is_empty());
        assert_eq!(invalid.inventory().unanchored_positions, 1);
        assert_eq!(invalid.inventory().unanchored_unknown_clocks, 1);
    }
}
