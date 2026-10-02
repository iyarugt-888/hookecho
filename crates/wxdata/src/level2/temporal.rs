//! Acquisition coverage for binned integrations and native observed radar sweeps.
//!
//! This records angular acquisition, not valid-echo coverage or proof a volume is complete.
//! Pass boundaries use the same source-time gap inference as the 2D sweep display. Native cut
//! indices are scan-local; persistent pass identities and proven transport gaps need live inventory.

use super::{mask_previous_pass_rows, previous_pass_cutoff, BinnedSweep, Moment};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TemporalPolicy {
    #[default]
    Continuous,
    StrictCurrent,
}

/// Coverage of native observed radials. Absent radials cannot be counted without an acquisition
/// inventory; these counts describe recorded input rows, not synthesized azimuth bins.
#[derive(Clone, Debug, PartialEq)]
pub struct ObservedCutCoverage {
    /// Index in this decoded scan's sweep vector, not a globally persisted pass identifier.
    pub source_cut: usize,
    pub elevation_number: u8,
    pub elevation_deg: f32,
    pub recorded_radials: usize,
    pub excluded_radials: usize,
    pub older_pass_radials: usize,
    /// Another timed cut is selected for this moment/elevation by the 2D tilt-list selector.
    pub unselected_cut: bool,
    pub unknown_time_radials: usize,
    pub used_start_ms: Option<i64>,
    pub used_end_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservedCoverage {
    pub policy: TemporalPolicy,
    pub cuts: Vec<ObservedCutCoverage>,
}

impl ObservedCoverage {
    pub fn acquisition_range_ms(&self) -> Option<(i64, i64)> {
        Some((
            self.cuts.iter().filter_map(|cut| cut.used_start_ms).min()?,
            self.cuts.iter().filter_map(|cut| cut.used_end_ms).max()?,
        ))
    }

    pub fn excluded_radials(&self) -> usize {
        self.cuts.iter().map(|cut| cut.excluded_radials).sum()
    }

    pub fn retained_older_radials(&self) -> usize {
        if self.policy == TemporalPolicy::Continuous {
            self.cuts.iter().map(|cut| cut.older_pass_radials).sum()
        } else {
            0
        }
    }

    pub fn unknown_time_radials(&self) -> usize {
        self.cuts.iter().map(|cut| cut.unknown_time_radials).sum()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SweepCoverage {
    pub moment: Moment,
    pub elevation_deg: f32,
    pub azimuth_rows: usize,
    /// Source interval of the rows allowed to contribute after applying the policy.
    pub used_start_ms: Option<i64>,
    pub used_end_ms: Option<i64>,
    /// Rows timed to the older side of an inferred pass boundary, before masking.
    pub older_pass_rows: usize,
    /// All rows excluded by the strict mask, including untimed rows at a detected boundary.
    pub excluded_rows: usize,
    /// Empty, untimed rows. Binned data cannot distinguish a not-yet-scanned sector from a gap.
    pub unobserved_rows: usize,
    /// Rows with data but without a usable source clock. These are never called current.
    pub unknown_time_rows: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TemporalCoverage {
    pub policy: TemporalPolicy,
    pub contributors: Vec<SweepCoverage>,
}

impl TemporalCoverage {
    pub fn acquisition_range_ms(&self) -> Option<(i64, i64)> {
        Some((
            self.contributors
                .iter()
                .filter_map(|s| s.used_start_ms)
                .min()?,
            self.contributors
                .iter()
                .filter_map(|s| s.used_end_ms)
                .max()?,
        ))
    }

    pub fn retained_older_rows(&self) -> usize {
        if self.policy == TemporalPolicy::Continuous {
            self.contributors.iter().map(|s| s.older_pass_rows).sum()
        } else {
            0
        }
    }

    pub fn excluded_rows(&self) -> usize {
        self.contributors.iter().map(|s| s.excluded_rows).sum()
    }

    pub fn unobserved_rows(&self) -> usize {
        self.contributors.iter().map(|s| s.unobserved_rows).sum()
    }

    pub fn unknown_time_rows(&self) -> usize {
        self.contributors.iter().map(|s| s.unknown_time_rows).sum()
    }
}

/// Prepare owned build inputs; cached sweeps and their original source clocks stay intact.
/// Unknown clocks remain unknown. Strict masking agrees with the existing 2D behavior: without
/// a distinguishable pass boundary it does not guess one from a client clock or elevation.
pub fn prepare(
    sweeps: &mut [BinnedSweep],
    policy: TemporalPolicy,
) -> anyhow::Result<TemporalCoverage> {
    // Reject the whole build before changing any input when a later contributor is malformed.
    for sweep in sweeps.iter() {
        anyhow::ensure!(
            sweep.az_bins > 0
                && sweep.gate_count > 0
                && sweep.az_bins.checked_mul(sweep.gate_count) == Some(sweep.data.len()),
            "invalid radar sweep dimensions for temporal coverage"
        );
    }
    let mut contributors = Vec::with_capacity(sweeps.len());
    for sweep in sweeps {
        let cutoff = previous_pass_cutoff(&sweep.bin_time_ms, sweep.az_bins);
        let timed = sweep.bin_time_ms.len() == sweep.az_bins;
        let strict = policy == TemporalPolicy::StrictCurrent;
        let mut coverage = SweepCoverage {
            moment: sweep.moment,
            elevation_deg: sweep.elevation_deg,
            azimuth_rows: sweep.az_bins,
            used_start_ms: None,
            used_end_ms: None,
            older_pass_rows: 0,
            excluded_rows: 0,
            unobserved_rows: 0,
            unknown_time_rows: 0,
        };
        for row in 0..sweep.az_bins {
            let time = if timed { sweep.bin_time_ms[row] } else { 0 };
            let excluded = strict && cutoff.is_some_and(|old| time <= old);
            if time > 0 {
                coverage.older_pass_rows += usize::from(cutoff.is_some_and(|old| time <= old));
                if !excluded {
                    coverage.used_start_ms =
                        Some(coverage.used_start_ms.map_or(time, |t| t.min(time)));
                    coverage.used_end_ms = Some(coverage.used_end_ms.map_or(time, |t| t.max(time)));
                }
            } else {
                let data = &sweep.data[row * sweep.gate_count..(row + 1) * sweep.gate_count];
                if data.iter().any(|&v| v != 0) {
                    coverage.unknown_time_rows += 1;
                } else {
                    coverage.unobserved_rows += 1;
                }
            }
        }
        if strict {
            let mut data = std::mem::take(&mut sweep.data);
            coverage.excluded_rows = mask_previous_pass_rows(sweep, &mut data);
            sweep.data = data;
        }
        contributors.push(coverage);
    }
    Ok(TemporalCoverage {
        policy,
        contributors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mixed() -> BinnedSweep {
        BinnedSweep {
            az_bins: 8,
            gate_count: 4,
            data: vec![120; 32],
            first_gate_km: 5.0,
            gate_interval_km: 5.0,
            radar_lat: 35.0,
            radar_lon: -97.0,
            elevation_deg: 0.5,
            value_min: -20.0,
            value_max: 80.0,
            bin_time_ms: vec![1_700_000_120_000; 4]
                .into_iter()
                .chain([1_700_000_000_000; 4])
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn continuous_retains_mixed_passes_and_strict_matches_the_2d_mask() {
        let original = mixed();
        let mut continuous = [original.clone()];
        let coverage = prepare(&mut continuous, TemporalPolicy::Continuous).unwrap();
        assert_eq!(continuous[0].data, original.data);
        assert_eq!(coverage.retained_older_rows(), 4);
        assert_eq!(
            coverage.acquisition_range_ms(),
            Some((1_700_000_000_000, 1_700_000_120_000))
        );
        let mut expected = original.data.clone();
        mask_previous_pass_rows(&original, &mut expected);
        let mut strict = [original.clone()];
        let coverage = prepare(&mut strict, TemporalPolicy::StrictCurrent).unwrap();
        assert_eq!(strict[0].data, expected);
        assert_eq!(strict[0].bin_time_ms, original.bin_time_ms);
        assert_eq!(coverage.retained_older_rows(), 0);
        assert_eq!(coverage.excluded_rows(), 4);
        assert_eq!(
            coverage.acquisition_range_ms(),
            Some((1_700_000_120_000, 1_700_000_120_000))
        );
    }

    #[test]
    fn unknown_clocks_and_unobserved_sectors_are_not_called_complete_or_current() {
        for clocks in [vec![], vec![1_700_000_000_000]] {
            let mut input = mixed();
            input.bin_time_ms = clocks;
            input.data[4..8].fill(0);
            let original = input.data.clone();
            let coverage = prepare(&mut [input.clone()], TemporalPolicy::StrictCurrent).unwrap();
            assert_eq!(coverage.unknown_time_rows(), 7);
            assert_eq!(coverage.unobserved_rows(), 1);
            assert_eq!(coverage.acquisition_range_ms(), None);
            let mut sweeps = [input];
            prepare(&mut sweeps, TemporalPolicy::StrictCurrent).unwrap();
            assert_eq!(sweeps[0].data, original);
        }
        let mut input = mixed();
        input.bin_time_ms[1] = 0;
        input.data[4..8].fill(0);
        let coverage = prepare(&mut [input], TemporalPolicy::StrictCurrent).unwrap();
        assert_eq!(coverage.unobserved_rows(), 1);
        assert_eq!(coverage.excluded_rows(), 5);
    }

    #[test]
    fn strict_integration_preserves_empty_sectors_in_all_six_derived_fields() {
        let mut inputs = vec![mixed(), mixed()];
        inputs[1].elevation_deg = 1.5;
        let opts = crate::derived::DerivedOpts {
            time: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            ..Default::default()
        };
        let continuous = crate::derived::derive(&inputs, &opts).unwrap();
        prepare(&mut inputs, TemporalPolicy::StrictCurrent).unwrap();
        let strict = crate::derived::derive(&inputs, &opts).unwrap();
        let hail = crate::derived::hail(&inputs, 200.0, 400.0, &opts).unwrap();
        let missing: Vec<_> = continuous
            .composite
            .values
            .iter()
            .zip(&strict.composite.values)
            .enumerate()
            .filter_map(|(i, (a, b))| (a.is_finite() && b.is_nan()).then_some(i))
            .collect();
        assert!(
            missing.len() > 100,
            "older sectors must remain missing after integration"
        );
        assert!(strict.composite.values.iter().any(|v| v.is_finite()));
        for grid in [
            &strict.vil,
            &strict.vild,
            &strict.etop,
            &hail.mehs,
            &hail.posh,
        ] {
            assert!(missing.iter().all(|&i| grid.values[i].is_nan()));
            assert_eq!(grid.time, opts.time);
        }
    }

    #[test]
    fn invalid_geometry_cannot_create_a_coverage_record() {
        let mut input = mixed();
        input.data.pop();
        let mut inputs = [mixed(), input];
        let original = inputs[0].data.clone();
        assert!(prepare(&mut inputs, TemporalPolicy::StrictCurrent).is_err());
        assert_eq!(inputs[0].data, original);
        assert!(prepare(&mut [BinnedSweep::default()], TemporalPolicy::Continuous).is_err());
    }

    #[test]
    fn late_gap_fill_and_reordered_rows_use_source_times_without_creating_missing_upper_cuts() {
        let mut input = mixed();
        input.bin_time_ms[2] = 0;
        input.data[8..12].fill(0);
        let initial = prepare(&mut [input.clone()], TemporalPolicy::Continuous).unwrap();
        assert_eq!(initial.unobserved_rows(), 1);
        input.bin_time_ms[2] = 1_700_000_121_000;
        input.data[8..12].fill(120);
        let filled = prepare(&mut [input.clone()], TemporalPolicy::Continuous).unwrap();
        assert_eq!(filled.unobserved_rows(), 0);
        assert_eq!(
            filled.contributors.len(),
            1,
            "unseen upper cuts are not synthesized"
        );
        input.bin_time_ms.reverse();
        input.data = input
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .rev()
            .flatten()
            .copied()
            .collect();
        let reordered = prepare(&mut [input.clone()], TemporalPolicy::Continuous).unwrap();
        assert_eq!(filled, reordered);
        for (i, time) in input.bin_time_ms.iter_mut().enumerate() {
            *time = 1_700_000_120_000 + i as i64 * 1000;
        }
        let complete_pass = prepare(&mut [input], TemporalPolicy::StrictCurrent).unwrap();
        assert_eq!(complete_pass.excluded_rows(), 0);
        assert_eq!(complete_pass.retained_older_rows(), 0);
    }
}
