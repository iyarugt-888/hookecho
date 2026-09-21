//! Temporal extrema trails: the strongest (or weakest) value each gate has held over a moving
//! time window — ROADMAP_NEW C2.
//!
//! A rotation track, a hail-core path, a reflectivity core path and a CC-minimum path are all the
//! same operation with a different moment: hold a running extremum per gate, feed it the frames
//! inside the window, and draw the result. The analyst question it answers is the one a single
//! frame cannot — *where has this been*, not *where is it now*.
//!
//! ## Why this accumulates in polar space
//!
//! Every sweep from one site at one tilt shares its polar geometry, so the accumulator is the
//! same `[az_bin][gate]` grid as the sweeps feeding it and the merge is an element-wise `max`
//! (or `min`) over `u8`. That buys three things:
//!
//! 1. **No resampling.** Projecting to a lat/lon grid first — the way [`crate::derived`] must,
//!    because it integrates *across* tilts — would smear a sharp couplet over a cell and make
//!    the extremum depend on grid spacing. Radar is drawn in polar space on the GPU
//!    ([`crate::level2::BinnedSweep`] uploads directly), so the output of this module is a
//!    `BinnedSweep` and the existing radar layer draws it with no new pipeline.
//! 2. **Comparing codes is comparing values.** The `2..=255` band is a *monotonic* linear map
//!    onto the moment's physical range, so `max` over codes is `max` over dBZ (or m/s, or CC)
//!    without decoding and re-encoding each gate. [`accumulate`] refuses to merge two sweeps
//!    whose `value_min`/`value_max` differ, which is the assumption that makes this sound —
//!    dealiased velocity carries a widened range and must not be folded in with raw velocity.
//! 3. **It costs one pass.** A window of frames is `O(frames × az_bins × gate_count)` of `u8`
//!    comparisons with no allocation per frame, which is what lets it run on the phone.
//!
//! ## What it deliberately does not do
//!
//! ponytail: one tilt, one site. A trail spanning tilts would average over beam heights that are
//! kilometres apart at range, and a trail spanning sites would need both reprojected to common
//! ground first — that is [`crate::mosaic`]'s problem, not this one. [`accumulate`] returns
//! [`Merge::Reset`] rather than silently producing a blend, and the caller starts a fresh trail.
//!
//! Sentinel codes never win. `0` (below threshold) and `1` (range folded) are not measurements:
//! a folded velocity gate is an unknown velocity, and letting it seed a minimum trail would paint
//! a permanent false couplet wherever the Nyquist interval was exceeded once.

use crate::level2::BinnedSweep;

/// Which end of the range the trail keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extremum {
    /// Strongest value seen — reflectivity cores, hail, azimuthal shear, ZDR columns.
    Max,
    /// Weakest value seen — the CC-minimum path a debris signature leaves.
    Min,
}

/// What [`accumulate`] did with the frame it was handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Merge {
    /// Folded into the running trail.
    Merged,
    /// The sweep does not describe the same beam as the accumulator, so nothing was merged and
    /// the caller should start over from this frame. Carries why, for the layer's status line.
    Reset(Mismatch),
}

/// Why two sweeps could not share a trail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mismatch {
    /// Different moment — a reflectivity trail cannot absorb a velocity sweep.
    Moment,
    /// Different physical range behind the same `u8` band. Raw and dealiased velocity are the
    /// live case: both are `Moment::Velocity`, and their codes mean different m/s.
    ValueRange,
    /// Different azimuth/gate grid.
    Geometry,
    /// Different elevation cut — kilometres of beam height apart at range.
    Elevation,
    /// Different radar.
    Site,
}

/// How far two elevation angles may differ and still count as the same cut, in degrees.
///
/// Not zero: a VCP's reported angle for "the 0.5° cut" wanders by a few hundredths between
/// volumes as the pedestal settles, and demanding exact equality would reset the trail on every
/// frame. Well under the gap between adjacent cuts (the tightest in VCP 212 is 0.4°).
const ELEV_TOLERANCE_DEG: f32 = 0.05;

/// How far two radar positions may differ and still count as the same site, in degrees.
/// A site does not move; this only absorbs float noise in the position carried per sweep.
const SITE_TOLERANCE_DEG: f32 = 1e-4;

/// Lowest code that is a measurement rather than a sentinel. See [`BinnedSweep`]'s docs:
/// `0` is below threshold, `1` is range folded.
const FIRST_VALUE_CODE: u8 = 2;

/// Start a trail from one sweep.
///
/// The accumulator is a `BinnedSweep`, so whatever draws radar draws this. Its `bin_time_ms` and
/// `stale_arc_deg` are cleared: a trail is not a single rotation of the antenna, and carrying a
/// live sweep's timing would make the progressive-render path paint a sweep wedge across an
/// accumulation that has no such thing.
pub fn start(seed: &BinnedSweep) -> BinnedSweep {
    let mut acc = seed.clone();
    acc.bin_time_ms = Vec::new();
    acc.stale_arc_deg = None;
    acc
}

/// Fold `sweep` into `acc`, keeping the running [`Extremum`] at every gate.
///
/// Returns [`Merge::Reset`] without touching `acc` when the two do not describe the same beam;
/// see the module docs for why that is a refusal rather than a best effort.
pub fn accumulate(acc: &mut BinnedSweep, sweep: &BinnedSweep, keep: Extremum) -> Merge {
    if let Some(why) = mismatch(acc, sweep) {
        return Merge::Reset(why);
    }
    for (slot, &code) in acc.data.iter_mut().zip(sweep.data.iter()) {
        // A sentinel is not a measurement and never wins, in either direction.
        if code < FIRST_VALUE_CODE {
            continue;
        }
        // An empty slot takes the first real value it is offered, whichever end we are keeping —
        // otherwise a Min trail would stay empty forever, since nothing is below "no data".
        if *slot < FIRST_VALUE_CODE {
            *slot = code;
            continue;
        }
        *slot = match keep {
            Extremum::Max => (*slot).max(code),
            Extremum::Min => (*slot).min(code),
        };
    }
    Merge::Merged
}

/// Fold a whole window in one call, oldest frame first, starting a fresh trail whenever the
/// sequence changes beam.
///
/// Returns `None` for an empty window. The last [`Mismatch`] that forced a restart comes back
/// with the trail so a caller can say "trail restarted: site changed" rather than silently
/// showing a shorter history than the window asked for.
pub fn trail<'a, I>(frames: I, keep: Extremum) -> Option<(BinnedSweep, Option<Mismatch>)>
where
    I: IntoIterator<Item = &'a BinnedSweep>,
{
    let mut acc: Option<BinnedSweep> = None;
    let mut restarted = None;
    for sweep in frames {
        match acc.as_mut() {
            None => acc = Some(start(sweep)),
            Some(a) => {
                if let Merge::Reset(why) = accumulate(a, sweep, keep) {
                    restarted = Some(why);
                    *a = start(sweep);
                }
            }
        }
    }
    acc.map(|a| (a, restarted))
}

/// Whether `sweep` describes the same beam as `acc`, and if not, the first reason it does not.
fn mismatch(acc: &BinnedSweep, sweep: &BinnedSweep) -> Option<Mismatch> {
    if acc.moment != sweep.moment {
        return Some(Mismatch::Moment);
    }
    // Checked before geometry because it is the subtle one: raw and dealiased velocity agree on
    // every dimension here and disagree on what a code means.
    if acc.value_min != sweep.value_min || acc.value_max != sweep.value_max {
        return Some(Mismatch::ValueRange);
    }
    if acc.az_bins != sweep.az_bins
        || acc.gate_count != sweep.gate_count
        || acc.first_gate_km != sweep.first_gate_km
        || acc.gate_interval_km != sweep.gate_interval_km
        || acc.data.len() != sweep.data.len()
    {
        return Some(Mismatch::Geometry);
    }
    if (acc.radar_lat - sweep.radar_lat).abs() > SITE_TOLERANCE_DEG
        || (acc.radar_lon - sweep.radar_lon).abs() > SITE_TOLERANCE_DEG
    {
        return Some(Mismatch::Site);
    }
    if (acc.elevation_deg - sweep.elevation_deg).abs() > ELEV_TOLERANCE_DEG {
        return Some(Mismatch::Elevation);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level2::Moment;

    const AZ: usize = 8;
    const GATES: usize = 4;

    fn sweep(moment: Moment, codes: &[u8]) -> BinnedSweep {
        let (value_min, value_max) = moment.value_range();
        BinnedSweep {
            moment,
            az_bins: AZ,
            gate_count: GATES,
            data: codes.to_vec(),
            first_gate_km: 0.0,
            gate_interval_km: 0.25,
            radar_lat: 35.0,
            radar_lon: -97.0,
            elevation_deg: 0.5,
            value_min,
            value_max,
            ..Default::default()
        }
    }

    fn flat(moment: Moment, code: u8) -> BinnedSweep {
        sweep(moment, &[code; AZ * GATES])
    }

    #[test]
    fn max_keeps_the_strongest_gate_each_frame_offered() {
        let a = flat(Moment::Reflectivity, 100);
        let mut b = flat(Moment::Reflectivity, 50);
        b.data[3] = 200;
        let c = flat(Moment::Reflectivity, 60);

        let (trail, restarted) = trail([&a, &b, &c], Extremum::Max).expect("three frames");
        assert_eq!(restarted, None);
        assert_eq!(trail.data[3], 200, "the one strong gate survives");
        assert!(
            trail
                .data
                .iter()
                .enumerate()
                .all(|(i, &v)| i == 3 || v == 100),
            "every other gate keeps the strongest of 100/50/60"
        );
    }

    #[test]
    fn min_keeps_the_weakest_gate_but_never_a_sentinel() {
        let mut a = flat(Moment::CorrelationCoefficient, 200);
        let mut b = flat(Moment::CorrelationCoefficient, 150);
        // Gate 0: a real drop, which a CC-minimum trail exists to catch.
        b.data[0] = 40;
        // Gate 1: below threshold this frame. "No data" is not a low CC.
        b.data[1] = 0;
        // Gate 2: range folded. Also not a measurement.
        b.data[2] = 1;
        a.data[1] = 210;
        a.data[2] = 210;

        let (trail, _) = trail([&a, &b], Extremum::Min).expect("two frames");
        assert_eq!(trail.data[0], 40, "the real minimum is kept");
        assert_eq!(
            trail.data[1], 210,
            "a below-threshold gate does not win a minimum"
        );
        assert_eq!(
            trail.data[2], 210,
            "a range-folded gate does not win a minimum"
        );
    }

    #[test]
    fn an_empty_gate_takes_the_first_real_value_in_either_mode() {
        // Regression: seeding from a frame with no echo used to leave a Min trail empty forever,
        // because no code is ever less than the 0 sentinel it started from.
        for keep in [Extremum::Max, Extremum::Min] {
            let empty = flat(Moment::Reflectivity, 0);
            let echo = flat(Moment::Reflectivity, 90);
            let (trail, _) = trail([&empty, &echo], keep).expect("two frames");
            assert_eq!(
                trail.data[0], 90,
                "{keep:?} should adopt the first measurement"
            );
        }
    }

    #[test]
    fn a_trail_refuses_to_mix_beams_and_says_why() {
        let base = flat(Moment::Reflectivity, 100);

        let mut other_moment = flat(Moment::Velocity, 100);
        other_moment.elevation_deg = 0.5;
        let mut acc = start(&base);
        assert_eq!(
            accumulate(&mut acc, &other_moment, Extremum::Max),
            Merge::Reset(Mismatch::Moment)
        );

        let mut tilt = flat(Moment::Reflectivity, 100);
        tilt.elevation_deg = 1.5;
        let mut acc = start(&base);
        assert_eq!(
            accumulate(&mut acc, &tilt, Extremum::Max),
            Merge::Reset(Mismatch::Elevation)
        );

        let mut site = flat(Moment::Reflectivity, 100);
        site.radar_lat = 36.0;
        let mut acc = start(&base);
        assert_eq!(
            accumulate(&mut acc, &site, Extremum::Max),
            Merge::Reset(Mismatch::Site)
        );

        let mut geom = sweep(Moment::Reflectivity, &[100; AZ * (GATES + 1)]);
        geom.gate_count = GATES + 1;
        let mut acc = start(&base);
        assert_eq!(
            accumulate(&mut acc, &geom, Extremum::Max),
            Merge::Reset(Mismatch::Geometry)
        );
    }

    #[test]
    fn dealiased_velocity_is_not_folded_in_with_raw_velocity() {
        // Both are Moment::Velocity and agree on every dimension; only the range differs, and a
        // code means a different m/s in each. Merging them would report a speed that was never
        // measured.
        let raw = flat(Moment::Velocity, 200);
        let mut dealiased = flat(Moment::Velocity, 200);
        dealiased.value_min = -80.0;
        dealiased.value_max = 80.0;

        let mut acc = start(&raw);
        assert_eq!(
            accumulate(&mut acc, &dealiased, Extremum::Max),
            Merge::Reset(Mismatch::ValueRange)
        );
    }

    #[test]
    fn a_restart_keeps_the_frames_after_it_and_reports_the_reason() {
        let a = flat(Moment::Reflectivity, 100);
        let mut moved = flat(Moment::Reflectivity, 70);
        moved.radar_lat = 36.0;
        let mut after = flat(Moment::Reflectivity, 70);
        after.radar_lat = 36.0;
        after.data[5] = 180;

        let (trail, restarted) = trail([&a, &moved, &after], Extremum::Max).expect("frames");
        assert_eq!(restarted, Some(Mismatch::Site));
        assert_eq!(trail.radar_lat, 36.0, "the trail follows the new site");
        assert_eq!(trail.data[5], 180);
        assert_eq!(
            trail.data[0], 70,
            "nothing from before the restart survives into the new trail"
        );
    }

    #[test]
    fn the_trail_is_independent_of_frame_order_within_one_beam() {
        // C2's acceptance criterion is that a trail can be independently recomputed from cached
        // frames. An extremum is order-independent, so recomputing from a differently ordered
        // cache must land on the same raster.
        let mut a = flat(Moment::Reflectivity, 100);
        let mut b = flat(Moment::Reflectivity, 60);
        let mut c = flat(Moment::Reflectivity, 80);
        a.data[1] = 250;
        b.data[2] = 240;
        c.data[3] = 230;

        let (forward, _) = trail([&a, &b, &c], Extremum::Max).expect("frames");
        let (backward, _) = trail([&c, &b, &a], Extremum::Max).expect("frames");
        assert_eq!(forward.data, backward.data);
    }

    #[test]
    fn a_trail_never_carries_single_sweep_timing() {
        // `bin_time_ms` and `stale_arc_deg` describe one rotation of the antenna. An accumulation
        // is not one rotation, and the progressive-render path would otherwise paint a live sweep
        // wedge across it.
        let mut live = flat(Moment::Reflectivity, 100);
        live.bin_time_ms = vec![1_700_000_000_000; AZ];
        live.stale_arc_deg = Some((10.0, 40.0));

        let (trail, _) = trail([&live], Extremum::Max).expect("one frame");
        assert!(trail.bin_time_ms.is_empty());
        assert_eq!(trail.stale_arc_deg, None);
    }

    #[test]
    fn an_empty_window_is_none_not_a_blank_raster() {
        let empty: [&BinnedSweep; 0] = [];
        assert!(trail(empty, Extremum::Max).is_none());
    }
}
