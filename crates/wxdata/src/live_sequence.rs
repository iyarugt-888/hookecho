//! Byte-transport sequence evidence, independent of native radial presence or pass identity.
//! A hole between received sequence positions is not proof that any radar radial was lost.

use std::collections::BTreeSet;

pub const MAX_RETAINED_POSITIONS: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SequenceOrigin {
    /// Object positions within the source radar volume; numbering begins at one.
    UnidataChunks,
    /// Positions declared by the relay emitter, scoped to this subscription and radar volume.
    /// The upstream label does not establish emitter-instance identity or independent redundancy.
    RelayBlocks { upstream_id: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequenceInventory {
    pub origin: SequenceOrigin,
    pub received_bounds: Option<(u64, u64)>,
    pub retained_received: usize,
    pub bounded_holes: Vec<(u64, u64)>,
    pub failed_request_spans: Vec<(u64, u64)>,
    pub recovered_spans: Vec<(u64, u64)>,
    pub recovered_positions: u64,
    pub duplicate_arrivals: u64,
    pub reordered_arrivals: u64,
    pub failed_download_attempts: u64,
    /// Transport failures whose source position was not exposed by the provider API.
    pub unlocated_transport_errors: u64,
    /// Failed assembly attempts, often incomplete metadata; not a count of bad/lost blocks.
    pub failed_decode_attempts: u64,
    pub retired_positions: u64,
    pub outside_retained_arrivals: u64,
    pub unavailable_updates: u64,
}

impl SequenceInventory {
    pub fn estimated_dynamic_bytes(&self) -> usize {
        let origin = match &self.origin {
            SequenceOrigin::UnidataChunks => 0,
            SequenceOrigin::RelayBlocks { upstream_id } => {
                upstream_id.as_ref().map_or(0, String::capacity)
            }
        };
        origin
            + (self.bounded_holes.capacity()
                + self.failed_request_spans.capacity()
                + self.recovered_spans.capacity())
                * std::mem::size_of::<(u64, u64)>()
    }
}

/// One provider subscription/volume's bounded receipt ledger. No payloads or gate buffers.
pub struct SequenceLedger {
    origin: SequenceOrigin,
    tracked: BTreeSet<u64>,
    received: BTreeSet<u64>,
    failed_requests: BTreeSet<u64>,
    recovered: BTreeSet<u64>,
    floor: Option<u64>,
    last_arrival: Option<u64>,
    recovered_positions: u64,
    duplicates: u64,
    reordered: u64,
    failed_download_attempts: u64,
    unlocated_transport_errors: u64,
    failed_decode_attempts: u64,
    retired: u64,
    outside: u64,
}

impl SequenceLedger {
    pub fn origin(&self) -> &SequenceOrigin {
        &self.origin
    }

    pub fn new(origin: SequenceOrigin) -> Self {
        Self {
            origin,
            tracked: BTreeSet::new(),
            received: BTreeSet::new(),
            failed_requests: BTreeSet::new(),
            recovered: BTreeSet::new(),
            floor: None,
            last_arrival: None,
            recovered_positions: 0,
            duplicates: 0,
            reordered: 0,
            failed_download_attempts: 0,
            unlocated_transport_errors: 0,
            failed_decode_attempts: 0,
            retired: 0,
            outside: 0,
        }
    }

    pub fn observe(&mut self, position: u64) {
        if self.last_arrival.is_some_and(|last| position < last) {
            self.reordered = self.reordered.saturating_add(1);
        }
        self.last_arrival = Some(position);
        let duplicate = self.received.contains(&position);
        if duplicate {
            self.duplicates = self.duplicates.saturating_add(1);
        }
        let filled_hole = !duplicate
            && self.received.first().is_some_and(|first| position > *first)
            && self.received.last().is_some_and(|last| position < *last);
        if !self.retain(position) {
            self.outside = self.outside.saturating_add(1);
            return;
        }
        let filled_request = self.failed_requests.remove(&position);
        if self.received.insert(position) && (filled_hole || filled_request) {
            self.recovered.insert(position);
            self.recovered_positions = self.recovered_positions.saturating_add(1);
        }
    }

    /// Record only an actual failed request with an exposed source position, not a predicted next ID.
    pub fn download_failed(&mut self, position: u64) {
        self.failed_download_attempts = self.failed_download_attempts.saturating_add(1);
        if self.retain(position) && !self.received.contains(&position) {
            self.failed_requests.insert(position);
        }
    }

    pub fn transport_error_without_position(&mut self) {
        self.unlocated_transport_errors = self.unlocated_transport_errors.saturating_add(1);
    }

    pub fn decode_failed(&mut self) {
        self.failed_decode_attempts = self.failed_decode_attempts.saturating_add(1);
    }

    fn retain(&mut self, position: u64) -> bool {
        if self.floor.is_some_and(|floor| position < floor)
            || (self.tracked.len() == MAX_RETAINED_POSITIONS
                && !self.tracked.contains(&position)
                && self.tracked.first().is_some_and(|first| position < *first))
        {
            return false;
        }
        self.tracked.insert(position);
        if self.tracked.len() > MAX_RETAINED_POSITIONS {
            let oldest = self
                .tracked
                .pop_first()
                .expect("full bounded sequence history");
            self.received.remove(&oldest);
            self.failed_requests.remove(&oldest);
            self.recovered.remove(&oldest);
            self.floor = oldest.checked_add(1);
            self.retired = self.retired.saturating_add(1);
        }
        true
    }

    pub fn inventory(&self) -> SequenceInventory {
        let mut holes = Vec::new();
        let mut previous: Option<u64> = None;
        for &position in &self.received {
            if let Some(first) = previous.and_then(|previous| previous.checked_add(1)) {
                if first < position {
                    holes.push((first, position - 1));
                }
            }
            previous = Some(position);
        }
        SequenceInventory {
            origin: self.origin.clone(),
            received_bounds: self
                .received
                .first()
                .copied()
                .zip(self.received.last().copied()),
            retained_received: self.received.len(),
            bounded_holes: holes,
            failed_request_spans: spans(&self.failed_requests),
            recovered_spans: spans(&self.recovered),
            recovered_positions: self.recovered_positions,
            duplicate_arrivals: self.duplicates,
            reordered_arrivals: self.reordered,
            failed_download_attempts: self.failed_download_attempts,
            unlocated_transport_errors: self.unlocated_transport_errors,
            failed_decode_attempts: self.failed_decode_attempts,
            retired_positions: self.retired,
            outside_retained_arrivals: self.outside,
            unavailable_updates: 0,
        }
    }
}

fn spans(positions: &BTreeSet<u64>) -> Vec<(u64, u64)> {
    let mut spans: Vec<(u64, u64)> = Vec::new();
    for &position in positions {
        if let Some(last) = spans
            .last_mut()
            .filter(|last| last.1.checked_add(1) == Some(position))
        {
            last.1 = position;
        } else {
            spans.push((position, position));
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_holes_recover_without_inventing_a_join_prefix_or_recovery_on_duplicate() {
        let mut ledger = SequenceLedger::new(SequenceOrigin::RelayBlocks {
            upstream_id: Some("ldm".into()),
        });
        ledger.observe(500);
        ledger.observe(503);
        let before = ledger.inventory();
        assert_eq!(before.bounded_holes, [(501, 502)]);
        assert!(
            before.failed_request_spans.is_empty(),
            "a bounded hole is not a failed-request receipt"
        );
        ledger.observe(501);
        ledger.observe(501);
        ledger.observe(502);
        let after = ledger.inventory();
        assert!(after.bounded_holes.is_empty());
        assert_eq!(after.recovered_spans, [(501, 502)]);
        assert_eq!(after.recovered_positions, 2);
        assert_eq!(after.duplicate_arrivals, 1);
        assert_eq!(after.reordered_arrivals, 1);
        assert_eq!(
            before.bounded_holes,
            [(501, 502)],
            "earlier receipt is immutable"
        );
        assert_eq!(
            before.received_bounds,
            Some((500, 503)),
            "no invented 0..499 gap"
        );
    }

    #[test]
    fn actual_request_failure_and_decode_failure_remain_separate_from_byte_presence() {
        let mut ledger = SequenceLedger::new(SequenceOrigin::UnidataChunks);
        ledger.download_failed(1);
        ledger.download_failed(1);
        ledger.observe(2);
        ledger.decode_failed();
        ledger.transport_error_without_position();
        let before = ledger.inventory();
        assert_eq!(before.failed_request_spans, [(1, 1)]);
        assert!(before.bounded_holes.is_empty());
        assert_eq!(before.failed_download_attempts, 2);
        assert_eq!(before.failed_decode_attempts, 1);
        assert_eq!(before.unlocated_transport_errors, 1);
        ledger.observe(1);
        ledger.download_failed(2);
        let after = ledger.inventory();
        assert!(
            after.failed_request_spans.is_empty(),
            "a failed retry cannot erase received bytes"
        );
        assert_eq!(after.recovered_positions, 1);
        assert_eq!(after.retained_received, 2);
        assert_eq!(after.failed_download_attempts, 3);
    }

    #[test]
    fn sequence_limits_and_u64_boundaries_do_not_enumerate_large_missing_ranges() {
        let mut ledger = SequenceLedger::new(SequenceOrigin::UnidataChunks);
        ledger.observe(0);
        ledger.observe(u64::MAX);
        assert_eq!(ledger.inventory().bounded_holes, [(1, u64::MAX - 1)]);
        ledger.observe(u64::MAX - 1);
        assert_eq!(
            ledger.inventory().recovered_spans,
            [(u64::MAX - 1, u64::MAX - 1)]
        );
        let mut bounded = SequenceLedger::new(SequenceOrigin::UnidataChunks);
        for position in 0..=MAX_RETAINED_POSITIONS as u64 {
            bounded.observe(position);
        }
        let before = bounded.inventory();
        assert_eq!(before.retained_received, MAX_RETAINED_POSITIONS);
        assert_eq!(before.retired_positions, 1);
        bounded.observe(0);
        assert_eq!(bounded.inventory().received_bounds, before.received_bounds);
        assert_eq!(bounded.inventory().outside_retained_arrivals, 1);
        assert_eq!(before.outside_retained_arrivals, 0);
    }
}
