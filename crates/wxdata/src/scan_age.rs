//! How old each part of a sweep is — the data behind the scan-age ring (ROADMAP_NEW, WSV3-class
//! gap "scan-age visualization").
//!
//! An antenna takes minutes to sweep a full circle, so the north edge of a picture and its south
//! edge can be a whole rotation apart. [`crate::level2::BinnedSweep::bin_time_ms`] already records
//! when every azimuth bin was collected (archive volumes as well as live ones); this module turns
//! that into something a person can read: how far apart the oldest and newest data are, and a
//! per-wedge age to colour a ring with.
//!
//! Ages here are measured **against the sweep's own newest bin**, not against the wall clock. On
//! an archive replay the wall clock says the whole picture is years old, which is true and
//! useless; the useful question is "which part of this picture is stale relative to the rest".
//! [`AgeSummary::newest_ms`] is exposed so a live caller can add the wall-clock age itself.

use crate::level2::BinnedSweep;

/// The spread of collection times across one sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgeSummary {
    /// Collection time of the most recently collected azimuth, ms since the Unix epoch.
    pub newest_ms: i64,
    /// Collection time of the least recently collected azimuth.
    pub oldest_ms: i64,
    /// Azimuth bins that carry a time.
    pub timed_bins: usize,
    /// Azimuth bins in the sweep.
    pub az_bins: usize,
}

impl AgeSummary {
    /// Time between the oldest and newest azimuth — roughly one antenna rotation for a complete
    /// sweep, less for one still being collected.
    pub fn span_ms(&self) -> i64 {
        self.newest_ms - self.oldest_ms
    }

    /// Whether some azimuths carry no time, so the ring has gaps rather than a colour there.
    pub fn is_partial(&self) -> bool {
        self.timed_bins < self.az_bins
    }
}

/// Summarise a sweep's collection times, or `None` when it carries none.
///
/// An empty `bin_time_ms` means "timing unknown" (every fixture, and any path that predates
/// partial-sweep rendering), never "all bins at time zero" — see [`BinnedSweep::bin_time_ms`].
pub fn summarize(sweep: &BinnedSweep) -> Option<AgeSummary> {
    if sweep.bin_time_ms.len() != sweep.az_bins || sweep.az_bins == 0 {
        return None;
    }
    let mut timed = sweep.bin_time_ms.iter().copied().filter(|&t| t > 0);
    let first = timed.next()?;
    let (mut newest, mut oldest, mut count) = (first, first, 1usize);
    for t in timed {
        newest = newest.max(t);
        oldest = oldest.min(t);
        count += 1;
    }
    Some(AgeSummary {
        newest_ms: newest,
        oldest_ms: oldest,
        timed_bins: count,
        az_bins: sweep.az_bins,
    })
}

/// Age of `segments` equal azimuth wedges, clockwise from north, as a fraction of the sweep's
/// span: `0.0` is as new as the newest data, `1.0` as old as the oldest.
///
/// A wedge is read at its centre bin — deterministic and cheap, and a wedge of a few degrees does
/// not change age meaningfully across its width. `None` marks a wedge whose centre bin carries no
/// time (no radial landed there), which a caller should draw as a gap and not as "newest".
///
/// A sweep whose bins are all the same instant (a synthetic frame, or a sub-second span) has no
/// spread to normalise by; every timed wedge is reported as `0.0` rather than dividing by zero.
pub fn ring(sweep: &BinnedSweep, segments: usize) -> Option<Vec<Option<f32>>> {
    let summary = summarize(sweep)?;
    if segments == 0 {
        return None;
    }
    let span = summary.span_ms();
    let out = (0..segments)
        .map(|s| {
            // Centre of wedge `s`, as a bin index.
            let centre = (s as f64 + 0.5) / segments as f64 * sweep.az_bins as f64;
            let bin = (centre as usize).min(sweep.az_bins - 1);
            let t = sweep.bin_time_ms[bin];
            (t > 0).then(|| {
                if span <= 0 {
                    0.0
                } else {
                    ((summary.newest_ms - t) as f64 / span as f64).clamp(0.0, 1.0) as f32
                }
            })
        })
        .collect();
    Some(out)
}

/// `"12s"`, `"4m 12s"`, `"1h 05m"` — an age or a span, for a readout.
pub fn format_span(ms: i64) -> String {
    let secs = (ms.max(0) + 500) / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timed(times: &[i64]) -> BinnedSweep {
        BinnedSweep {
            az_bins: times.len(),
            bin_time_ms: times.to_vec(),
            ..Default::default()
        }
    }

    #[test]
    fn no_timing_means_no_summary_not_a_span_from_epoch() {
        assert_eq!(summarize(&BinnedSweep::default()), None);
        // Timing present but for the wrong number of bins is not trusted either.
        let mut s = timed(&[10, 20, 30]);
        s.az_bins = 4;
        assert_eq!(summarize(&s), None);
    }

    #[test]
    fn the_span_is_newest_minus_oldest_ignoring_untimed_bins() {
        let s = timed(&[5_000, 0, 65_000, 35_000]);
        let sum = summarize(&s).expect("timed");
        assert_eq!((sum.oldest_ms, sum.newest_ms), (5_000, 65_000));
        assert_eq!(sum.span_ms(), 60_000);
        assert_eq!((sum.timed_bins, sum.az_bins), (3, 4));
        assert!(sum.is_partial());
    }

    #[test]
    fn a_wedge_reads_zero_at_the_newest_data_and_one_at_the_oldest() {
        // North bin oldest, then each bin a quarter-span newer.
        let s = timed(&[1_000, 16_000, 31_000, 61_000]);
        let r = ring(&s, 4).expect("timed");
        assert_eq!(r[0], Some(1.0), "oldest");
        assert_eq!(r[3], Some(0.0), "newest");
        let mid = r[1].expect("timed");
        assert!(
            (mid - 0.75).abs() < 1e-6,
            "16s is 45s of a 60s span behind: {mid}"
        );
    }

    #[test]
    fn an_untimed_wedge_is_a_gap_and_never_the_newest() {
        let s = timed(&[1_000, 0, 30_000, 60_000]);
        let r = ring(&s, 4).expect("timed");
        assert_eq!(r[1], None);
    }

    #[test]
    fn wedges_fewer_than_bins_read_their_centre_bin() {
        // Eight bins, two wedges: wedge 0 centres on bin 2, wedge 1 on bin 6.
        let s = timed(&[1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000, 9_000]);
        let r = ring(&s, 2).expect("timed");
        // span 8_000, newest 9_000. bin 2 = 3_000 -> 6/8; bin 6 = 7_000 -> 2/8.
        assert!((r[0].unwrap() - 0.75).abs() < 1e-6);
        assert!((r[1].unwrap() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn a_flat_sweep_has_no_spread_and_does_not_divide_by_zero() {
        let s = timed(&[7_000; 8]);
        let r = ring(&s, 4).expect("timed");
        assert!(r.iter().all(|w| *w == Some(0.0)));
    }

    #[test]
    fn zero_segments_is_none() {
        assert_eq!(ring(&timed(&[1_000, 2_000]), 0), None);
    }

    #[test]
    fn spans_read_as_seconds_minutes_and_hours() {
        assert_eq!(format_span(12_000), "12s");
        assert_eq!(format_span(252_000), "4m 12s");
        assert_eq!(format_span(3_900_000), "1h 05m");
        assert_eq!(format_span(-5), "0s");
    }
}
