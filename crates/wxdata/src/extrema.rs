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
use crate::mrms::MrmsField;

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
    /// The frame is older than the newest held and does not describe the same beam: it was left
    /// out and the trail kept. The newest beam decides what the trail is, so the same frames give
    /// the same trail whatever order they arrive in. ([`SlidingTrail::push`] only.)
    Skipped(Mismatch),
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

/// Age the trail by `codes` before the next frame is folded in — ROADMAP_NEW C2's decay view. A
/// kept maximum drops by that many codes and a kept minimum rises by them, so the newest part of
/// the path stays at full strength and the older part fades toward the unremarkable end; a value
/// that decays past the end of the scale leaves the trail. Sentinels are left alone. The codes
/// are a linear map onto the moment's physical range, so a fixed code step is a fixed physical
/// step (so many dBZ) everywhere on the sweep.
pub fn decay(acc: &mut BinnedSweep, keep: Extremum, codes: u8) {
    if codes == 0 {
        return;
    }
    for slot in &mut acc.data {
        if *slot < FIRST_VALUE_CODE {
            continue;
        }
        *slot = match keep {
            Extremum::Max => match slot.checked_sub(codes) {
                Some(c) if c >= FIRST_VALUE_CODE => c,
                _ => 0,
            },
            Extremum::Min => slot.saturating_add(codes),
        };
    }
}

/// The decay step for `elapsed_min` minutes on a trail whose window is `window_min` long: a
/// quarter of the code scale over the whole window, so a core from the start of the window shows
/// about a quarter of the scale weaker than it was (for reflectivity, roughly 30 dBZ).
pub fn decay_codes(elapsed_min: f32, window_min: u16) -> u8 {
    let span = f32::from(u8::MAX - FIRST_VALUE_CODE);
    (0.25 * span * elapsed_min.max(0.0) / f32::from(window_min.max(1)))
        .round()
        .min(255.0) as u8
}

/// The outline of where a gridded trail reaches `level` — at or above it for a kept maximum, at or
/// below it for a kept minimum — as closed `(lon, lat)` polylines: the "core path" an analyst
/// draws by hand. Contoured from a 0/1 mask at 0.5, so exactly one level comes back.
pub fn outline(grid: &crate::mrms::MrmsField, level: f32, keep: Extremum) -> Vec<Vec<(f64, f64)>> {
    let mask = crate::mrms::MrmsField {
        values: grid
            .values
            .iter()
            .map(|v| {
                let inside = match keep {
                    Extremum::Max => *v >= level,
                    Extremum::Min => *v <= level,
                };
                if v.is_finite() && inside {
                    1.0
                } else {
                    0.0
                }
            })
            .collect(),
        ..grid.clone()
    };
    crate::contour::contour_lines(&mask, 0.5)
        .into_iter()
        .map(|l| l.pts)
        .collect()
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

/// What a [`SlidingTrail`] covers as of a moment: the window asked for and what its frames span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Coverage {
    /// The window asked for, seconds.
    pub requested_s: i64,
    /// The oldest and newest frame inside it (seconds since the epoch).
    pub from: i64,
    pub to: i64,
    pub frames: usize,
    /// Frames the covered span should hold at the trail's own cadence (the median interval
    /// between its frames) but does not: missed volumes.
    pub missing: usize,
    /// Seconds at the old end of the window with no frame at all: a history shorter than asked
    /// for, which must not be shown as the full window.
    pub short_s: i64,
}

/// A trail from a [`SlidingTrail`]: physical extrema, untouched by age.
#[derive(Debug, Clone)]
pub struct WindowTrail {
    /// The extremum at every gate, as codes of the frames' own moment and range.
    pub sweep: BinnedSweep,
    /// The time (seconds since the epoch) of the frame that supplied each gate's value, the
    /// newest when several tie; `None` where no frame held a measurement. Age is for display
    /// opacity only, computed from this, never by changing a value.
    pub contributor: Vec<Option<i64>>,
    pub coverage: Coverage,
}

impl WindowTrail {
    /// The trail at `(lon, lat)`: the gate sample (value, geometry) and the time of the frame
    /// that supplied it. `None` outside the sweep.
    pub fn at_point(&self, lon: f64, lat: f64) -> Option<(crate::level2::GateSample, Option<i64>)> {
        let s = self.sweep.sample_at(lon, lat)?;
        let who = self
            .sweep
            .index_at(lon, lat)
            .and_then(|i| self.contributor.get(i).copied().flatten());
        Some((s, who))
    }
}

/// An exact sliding-window trail (ROADMAP_PARITY M3.4): the frames of the last `window_s`
/// seconds are kept (at most `max_frames`, oldest dropped first), and the trail as of any moment
/// is recomputed from those inside the window. So advancing past the strongest old frame removes
/// its contribution, where the running accumulator ([`accumulate`] with [`decay`]) could only
/// fade it by rewriting its values. Frames may arrive out of order (a backward seek, a late
/// download): each is placed by time, and the same frames give the same trail in any order. A
/// frame that does not share the beam with those held resets the trail to it, as [`trail`] does.
#[derive(Debug, Clone)]
pub struct SlidingTrail {
    keep: Extremum,
    window_s: i64,
    max_frames: usize,
    /// Held frames, oldest first, one per time.
    frames: Vec<(i64, BinnedSweep)>,
    /// Why the trail last restarted, for the layer's status line.
    pub last_reset: Option<Mismatch>,
}

impl SlidingTrail {
    pub fn new(keep: Extremum, window_s: i64, max_frames: usize) -> Self {
        SlidingTrail {
            keep,
            window_s: window_s.max(0),
            max_frames: max_frames.max(1),
            frames: Vec::new(),
            last_reset: None,
        }
    }

    /// How many frames are held.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// The times of the frames held, oldest first.
    pub fn times(&self) -> impl Iterator<Item = i64> + '_ {
        self.frames.iter().map(|(t, _)| *t)
    }

    /// Keep only frames inside the window ending at `now`. A trail anchored to a scrubbed
    /// playhead calls this before adding the frames it lacks, so a backward seek drops the frames
    /// after the playhead rather than letting them push the older ones it now needs out of
    /// [`Self::push`]'s window-behind-the-newest bound.
    pub fn retain_window(&mut self, now: i64) {
        let from = now - self.window_s;
        self.frames.retain(|(t, _)| *t >= from && *t <= now);
    }

    /// Add the frame scanned at `time` (seconds since the epoch). A frame at a time already held
    /// replaces it. One that does not describe the same beam resets the trail to it when it is
    /// the newest, and is skipped when it is older (a backward scrub across a VCP change).
    pub fn push(&mut self, time: i64, sweep: &BinnedSweep) -> Merge {
        if let Some((_, held)) = self.frames.first() {
            if let Some(why) = mismatch(held, sweep) {
                if self.frames.last().is_some_and(|(newest, _)| time < *newest) {
                    return Merge::Skipped(why);
                }
                self.frames.clear();
                self.frames.push((time, start(sweep)));
                self.last_reset = Some(why);
                return Merge::Reset(why);
            }
        }
        match self.frames.binary_search_by_key(&time, |(t, _)| *t) {
            Ok(i) => self.frames[i].1 = start(sweep),
            Err(i) => self.frames.insert(i, (time, start(sweep))),
        }
        // Bounded: frames older than the window behind the newest go, then the oldest past the cap.
        let newest = self.frames.last().map_or(time, |f| f.0);
        self.frames.retain(|(t, _)| *t >= newest - self.window_s);
        let over = self.frames.len().saturating_sub(self.max_frames);
        self.frames.drain(..over);
        Merge::Merged
    }

    /// The trail as of `now`: the exact extremum over the held frames with
    /// `now - window_s <= time <= now`, with each gate's contributing time and the coverage.
    /// `None` when no frame falls in the window.
    pub fn at(&self, now: i64) -> Option<WindowTrail> {
        let inside: Vec<&(i64, BinnedSweep)> = self
            .frames
            .iter()
            .filter(|(t, _)| *t <= now && *t >= now - self.window_s)
            .collect();
        let (first_t, first) = inside.first().map(|f| (f.0, &f.1))?;
        let mut sweep = start(first);
        let mut contributor: Vec<Option<i64>> = first
            .data
            .iter()
            .map(|&c| (c >= FIRST_VALUE_CODE).then_some(first_t))
            .collect();
        for (t, frame) in &inside[1..] {
            for ((slot, who), &code) in sweep
                .data
                .iter_mut()
                .zip(contributor.iter_mut())
                .zip(frame.data.iter())
            {
                if code < FIRST_VALUE_CODE {
                    continue;
                }
                let better = *slot < FIRST_VALUE_CODE
                    || match self.keep {
                        Extremum::Max => code >= *slot,
                        Extremum::Min => code <= *slot,
                    };
                if better {
                    *slot = code;
                    *who = Some(*t);
                }
            }
        }
        let times: Vec<i64> = inside.iter().map(|f| f.0).collect();
        Some(WindowTrail {
            sweep,
            contributor,
            coverage: coverage_of(&times, now, self.window_s),
        })
    }
}

/// The coverage of frames at `times` (ascending, non-empty) for the window of `window_s` ending at
/// `now`: span, missed frames at their own median cadence, and the unfilled old end.
fn coverage_of(times: &[i64], now: i64, window_s: i64) -> Coverage {
    let first_t = times[0];
    let to = *times.last().unwrap_or(&first_t);
    let mut steps: Vec<i64> = times.windows(2).map(|w| w[1] - w[0]).collect();
    steps.sort_unstable();
    let cadence = steps.get(steps.len() / 2).copied().filter(|c| *c > 0);
    let missing = cadence.map_or(0, |c| {
        let expected = ((to - first_t) as f64 / c as f64).round() as usize + 1;
        expected.saturating_sub(times.len())
    });
    let short_s = (first_t - (now - window_s) - cadence.unwrap_or(0)).max(0);
    Coverage {
        requested_s: window_s,
        from: first_t,
        to,
        frames: times.len(),
        missing,
        short_s,
    }
}

/// Why two grids could not share a [`GridTrail`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridMismatch {
    /// Different cell count or extent: another grid, not the same cells at another time.
    Grid,
}

/// A gridded trail as of a moment: physical extrema of finite cells, untouched by age.
#[derive(Clone)]
pub struct GridWindowTrail {
    /// The extremum per cell (NaN where no frame in the window had a value), stamped with the
    /// newest frame's time.
    pub field: MrmsField,
    /// Time (seconds) of the frame that supplied each cell, the newest on a tie.
    pub contributor: Vec<Option<i64>>,
    pub coverage: Coverage,
}

impl GridWindowTrail {
    /// `(value, contributor time)` of the cell `(lon, lat)` falls in. `None` off the grid.
    pub fn at_point(&self, lon: f64, lat: f64) -> Option<(Option<f32>, Option<i64>)> {
        let f = &self.field;
        if f.nx == 0 || f.ny == 0 {
            return None;
        }
        let fx = (lon - f.lon_west) / (f.lon_east - f.lon_west) * f.nx as f64;
        let fy = (f.lat_north - lat) / (f.lat_north - f.lat_south) * f.ny as f64;
        if !(0.0..f.nx as f64).contains(&fx) || !(0.0..f.ny as f64).contains(&fy) {
            return None;
        }
        let i = fy as usize * f.nx + fx as usize;
        let v = f.values[i];
        Some((v.is_finite().then_some(v), self.contributor[i]))
    }
}

/// [`SlidingTrail`] for lat/lon grids — a column user product or another field evaluated per
/// volume on one fixed grid. Frames are kept by time; the trail as of a moment is the exact
/// extremum of the finite cells of the frames inside the window ending then. NaN never wins.
/// Frames on another grid reset the trail when newest and are skipped when older, so arrival
/// order cannot change the answer. The caller owns product identity: a different product or
/// definition is a new trail, not a frame of this one.
#[derive(Clone)]
pub struct GridTrail {
    keep: Extremum,
    window_s: i64,
    max_frames: usize,
    frames: Vec<(i64, MrmsField)>,
    pub last_reset: Option<GridMismatch>,
}

fn same_grid(a: &MrmsField, b: &MrmsField) -> bool {
    a.nx == b.nx
        && a.ny == b.ny
        && a.values.len() == b.values.len()
        && (a.lon_west - b.lon_west).abs() < 1e-9
        && (a.lon_east - b.lon_east).abs() < 1e-9
        && (a.lat_north - b.lat_north).abs() < 1e-9
        && (a.lat_south - b.lat_south).abs() < 1e-9
}

/// What [`GridTrail::push`] did with a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridMerge {
    Merged,
    Reset(GridMismatch),
    Skipped(GridMismatch),
}

impl GridTrail {
    pub fn new(keep: Extremum, window_s: i64, max_frames: usize) -> Self {
        GridTrail {
            keep,
            window_s: window_s.max(0),
            max_frames: max_frames.max(1),
            frames: Vec::new(),
            last_reset: None,
        }
    }

    pub fn times(&self) -> impl Iterator<Item = i64> + '_ {
        self.frames.iter().map(|(t, _)| *t)
    }

    /// Keep only frames inside the window ending at `now` (see [`SlidingTrail::retain_window`]).
    pub fn retain_window(&mut self, now: i64) {
        let from = now - self.window_s;
        self.frames.retain(|(t, _)| *t >= from && *t <= now);
    }

    pub fn push(&mut self, time: i64, field: MrmsField) -> GridMerge {
        if let Some((_, held)) = self.frames.first() {
            if !same_grid(held, &field) {
                if self.frames.last().is_some_and(|(newest, _)| time < *newest) {
                    return GridMerge::Skipped(GridMismatch::Grid);
                }
                self.frames.clear();
                self.frames.push((time, field));
                self.last_reset = Some(GridMismatch::Grid);
                return GridMerge::Reset(GridMismatch::Grid);
            }
        }
        match self.frames.binary_search_by_key(&time, |(t, _)| *t) {
            Ok(i) => self.frames[i].1 = field,
            Err(i) => self.frames.insert(i, (time, field)),
        }
        let newest = self.frames.last().map_or(time, |f| f.0);
        self.frames.retain(|(t, _)| *t >= newest - self.window_s);
        let over = self.frames.len().saturating_sub(self.max_frames);
        self.frames.drain(..over);
        GridMerge::Merged
    }

    /// The trail as of `now` over the held frames with `now - window_s <= time <= now`.
    pub fn at(&self, now: i64) -> Option<GridWindowTrail> {
        let inside: Vec<&(i64, MrmsField)> = self
            .frames
            .iter()
            .filter(|(t, _)| *t <= now && *t >= now - self.window_s)
            .collect();
        let first = &inside.first()?.1;
        let mut values = vec![f32::NAN; first.values.len()];
        let mut contributor = vec![None; first.values.len()];
        for (t, f) in &inside {
            for ((slot, who), &v) in values.iter_mut().zip(contributor.iter_mut()).zip(&f.values) {
                if !v.is_finite() {
                    continue;
                }
                let better = !slot.is_finite()
                    || match self.keep {
                        Extremum::Max => v >= *slot,
                        Extremum::Min => v <= *slot,
                    };
                if better {
                    *slot = v;
                    *who = Some(*t);
                }
            }
        }
        let newest = inside.last().map(|f| &f.1).unwrap_or(first);
        let times: Vec<i64> = inside.iter().map(|f| f.0).collect();
        Some(GridWindowTrail {
            field: MrmsField {
                values,
                time: newest.time,
                ..newest.clone_meta()
            },
            contributor,
            coverage: coverage_of(&times, now, self.window_s),
        })
    }
}

impl MrmsField {
    /// The grid without its values.
    fn clone_meta(&self) -> MrmsField {
        MrmsField {
            values: Vec::new(),
            nx: self.nx,
            ny: self.ny,
            lon_west: self.lon_west,
            lon_east: self.lon_east,
            lat_north: self.lat_north,
            lat_south: self.lat_south,
            time: self.time,
        }
    }
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

    #[test]
    fn the_outline_encloses_the_cells_past_the_level() {
        let t = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let mut values = vec![0.0f32; 8 * 8];
        for y in 3..5 {
            for x in 3..5 {
                values[y * 8 + x] = 60.0;
            }
        }
        let grid = crate::mrms::MrmsField {
            values,
            nx: 8,
            ny: 8,
            lon_west: -98.0,
            lon_east: -97.0,
            lat_north: 36.0,
            lat_south: 35.0,
            time: t,
        };
        let rings = outline(&grid, 50.0, Extremum::Max);
        // Marching squares may hand the ring back in more than one stitched piece; every piece
        // lies on the core's edge.
        assert!(!rings.is_empty());
        for p in rings.iter().flatten() {
            assert!(
                (p.0 + 97.5).abs() < 0.2 && (p.1 - 35.5).abs() < 0.2,
                "{p:?}"
            );
        }
        assert!(outline(&grid, 70.0, Extremum::Max).is_empty());
    }

    #[test]
    fn decay_fades_a_kept_maximum_and_drops_it_off_the_bottom() {
        let mut acc = sweep(Moment::Reflectivity, &[0, 1, 2, 10, 200]);
        decay(&mut acc, Extremum::Max, 5);
        assert_eq!(
            acc.data,
            [0, 1, 0, 5, 195],
            "sentinels kept; too weak leaves the trail"
        );
        decay(&mut acc, Extremum::Min, 100);
        assert_eq!(
            acc.data,
            [0, 1, 0, 105, 255],
            "a minimum rises toward the top"
        );
        assert_eq!(decay_codes(60.0, 60), 63);
        assert_eq!(decay_codes(0.0, 60), 0);
    }
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

    /// A frame whose gates are `codes(gate)`.
    fn frame(f: impl Fn(usize) -> u8) -> BinnedSweep {
        sweep(
            Moment::Reflectivity,
            &(0..AZ * GATES).map(f).collect::<Vec<_>>(),
        )
    }

    #[test]
    fn a_sliding_trail_is_the_exact_extremum_of_the_frames_in_its_window() {
        // Pseudo-random frames, sentinels included, checked gate by gate against brute force.
        let mut seed = 12345u64;
        let mut rnd = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as u8
        };
        let frames: Vec<(i64, BinnedSweep)> = (0..12)
            .map(|k| {
                let codes: Vec<u8> = (0..AZ * GATES).map(|_| rnd()).collect();
                (1_000 + 300 * k as i64, frame(|i| codes[i]))
            })
            .collect();
        for keep in [Extremum::Max, Extremum::Min] {
            let mut t = SlidingTrail::new(keep, 1_800, 64);
            // As it runs live: each frame arrives and the trail is read as of it.
            for (time, f) in &frames {
                t.push(*time, f);
                let now = *time;
                let got = t.at(now).expect("frames in the window");
                let inside: Vec<&(i64, BinnedSweep)> = frames
                    .iter()
                    .filter(|(ft, _)| *ft <= now && *ft >= now - 1_800)
                    .collect();
                for g in 0..AZ * GATES {
                    let real: Vec<(i64, u8)> = inside
                        .iter()
                        .map(|(ft, f)| (*ft, f.data[g]))
                        .filter(|(_, c)| *c >= 2)
                        .collect();
                    let want = match keep {
                        Extremum::Max => real.iter().map(|x| x.1).max(),
                        Extremum::Min => real.iter().map(|x| x.1).min(),
                    };
                    match want {
                        Some(v) => {
                            assert_eq!(got.sweep.data[g], v, "{keep:?} gate {g} at {now}");
                            let newest = real.iter().filter(|x| x.1 == v).map(|x| x.0).max();
                            assert_eq!(got.contributor[g], newest);
                        }
                        None => assert!(got.sweep.data[g] < 2 && got.contributor[g].is_none()),
                    }
                }
            }
        }
    }

    /// A scrub back past the newest frame, as the app drives it: retain the window ending at the
    /// playhead, add the frames it lacks, read it as of the playhead. The answer must be exactly
    /// the one a trail built fresh at that playhead gives, whatever was held before.
    #[test]
    fn a_backward_scrub_rebuilds_exactly_what_a_fresh_trail_gives() {
        let frames: Vec<(i64, BinnedSweep)> = (0..10)
            .map(|k| {
                (
                    300 * k as i64,
                    frame(move |i| ((k * 37 + i * 11) % 250) as u8 + 2),
                )
            })
            .collect();
        let window = 1_200;
        let drive = |t: &mut SlidingTrail, now: i64| {
            t.retain_window(now);
            for (ft, f) in frames
                .iter()
                .filter(|(ft, _)| *ft <= now && *ft >= now - window)
            {
                if !t.times().any(|h| h == *ft) {
                    t.push(*ft, f);
                }
            }
            t.at(now).unwrap()
        };
        let mut live = SlidingTrail::new(Extremum::Max, window, 64);
        drive(&mut live, 2_700); // played to the end
        for now in [1_800, 600, 2_100, 0, 2_700] {
            let scrubbed = drive(&mut live, now);
            let fresh = drive(&mut SlidingTrail::new(Extremum::Max, window, 64), now);
            assert_eq!(scrubbed.sweep.data, fresh.sweep.data, "values at {now}");
            assert_eq!(scrubbed.contributor, fresh.contributor, "times at {now}");
            assert_eq!(scrubbed.coverage, fresh.coverage, "coverage at {now}");
            assert!(live.times().all(|t| t <= now && t >= now - window));
        }
    }

    /// Frames from two different beams (a VCP change moved the cut) in any arrival order end in
    /// the same trail: the newest beam's frames, as the app's retry loop drives it.
    #[test]
    fn mixed_beams_converge_to_the_newest_beams_trail_in_any_order() {
        let mut other = frame(|_| 240);
        other.elevation_deg = 0.9;
        let frames: Vec<(i64, BinnedSweep)> = vec![
            (0, frame(|_| 100)),
            (300, other.clone()),
            (600, frame(|_| 120)),
            (900, frame(|_| 110)),
        ];
        let orders: [[usize; 4]; 4] = [[0, 1, 2, 3], [3, 2, 1, 0], [1, 3, 0, 2], [2, 0, 3, 1]];
        let mut results = Vec::new();
        for order in orders {
            let mut t = SlidingTrail::new(Extremum::Max, 3_600, 16);
            let mut skipped = std::collections::HashSet::new();
            // The app's loop: every frame not held and not skipped is offered again next round.
            for _ in 0..4 {
                for &k in &order {
                    let (ft, f) = &frames[k];
                    if t.times().any(|h| h == *ft) || skipped.contains(ft) {
                        continue;
                    }
                    match t.push(*ft, f) {
                        Merge::Skipped(_) => {
                            skipped.insert(*ft);
                        }
                        Merge::Reset(_) => skipped.clear(),
                        Merge::Merged => {}
                    }
                }
            }
            let w = t.at(900).unwrap();
            results.push((t.times().collect::<Vec<_>>(), w.sweep.data.clone()));
        }
        for r in &results[1..] {
            assert_eq!(r, &results[0]);
        }
        assert_eq!(
            results[0].0,
            vec![0, 600, 900],
            "the 0.9° frame is left out"
        );
        assert!(results[0].1.iter().all(|&c| c == 120));
    }

    fn grid(time: i64, f: impl Fn(usize) -> f32) -> crate::mrms::MrmsField {
        crate::mrms::MrmsField {
            values: (0..12).map(f).collect(),
            nx: 4,
            ny: 3,
            lon_west: -98.0,
            lon_east: -97.96,
            lat_north: 35.03,
            lat_south: 35.0,
            time: chrono::DateTime::from_timestamp(time, 0).unwrap(),
        }
    }

    #[test]
    fn a_grid_trail_is_exact_over_its_window_and_nan_never_wins() {
        let mut seed = 99u64;
        let mut rnd = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 40) as u32
        };
        let frames: Vec<(i64, crate::mrms::MrmsField)> = (0..10)
            .map(|k| {
                let vals: Vec<f32> = (0..12)
                    .map(|_| {
                        let r = rnd();
                        if r % 4 == 0 {
                            f32::NAN
                        } else {
                            (r % 1000) as f32 / 10.0 - 20.0
                        }
                    })
                    .collect();
                let t = 1_000 + 300 * k as i64;
                (t, grid(t, |i| vals[i]))
            })
            .collect();
        for keep in [Extremum::Max, Extremum::Min] {
            let mut g = GridTrail::new(keep, 1_200, 64);
            for (t, f) in &frames {
                g.push(*t, f.clone());
                let w = g.at(*t).unwrap();
                assert_eq!(w.field.time, f.time, "stamped with the newest frame");
                for cell in 0..12 {
                    let real: Vec<(i64, f32)> = frames
                        .iter()
                        .filter(|(ft, _)| *ft <= *t && *ft >= *t - 1_200)
                        .map(|(ft, f)| (*ft, f.values[cell]))
                        .filter(|(_, v)| v.is_finite())
                        .collect();
                    let want = match keep {
                        Extremum::Max => real
                            .iter()
                            .map(|x| x.1)
                            .fold(None, |a: Option<f32>, v| Some(a.map_or(v, |a| a.max(v)))),
                        Extremum::Min => real
                            .iter()
                            .map(|x| x.1)
                            .fold(None, |a: Option<f32>, v| Some(a.map_or(v, |a| a.min(v)))),
                    };
                    match want {
                        Some(v) => {
                            assert_eq!(w.field.values[cell], v);
                            let newest = real.iter().filter(|x| x.1 == v).map(|x| x.0).max();
                            assert_eq!(w.contributor[cell], newest);
                        }
                        None => {
                            assert!(w.field.values[cell].is_nan() && w.contributor[cell].is_none())
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_grid_trail_scrubs_back_exactly_and_skips_older_other_grids() {
        let frames: Vec<(i64, crate::mrms::MrmsField)> = (0..6)
            .map(|k| (300 * k, grid(300 * k, move |i| (k * 7 + i as i64) as f32)))
            .collect();
        let drive = |g: &mut GridTrail, now: i64| {
            g.retain_window(now);
            for (t, f) in frames.iter().filter(|(t, _)| *t <= now && *t >= now - 900) {
                if !g.times().any(|h| h == *t) {
                    g.push(*t, f.clone());
                }
            }
            g.at(now).unwrap()
        };
        let mut live = GridTrail::new(Extremum::Max, 900, 16);
        drive(&mut live, 1_500);
        for now in [600, 1_200, 0, 1_500] {
            let a = drive(&mut live, now);
            let b = drive(&mut GridTrail::new(Extremum::Max, 900, 16), now);
            assert!(a
                .field
                .values
                .iter()
                .zip(&b.field.values)
                .all(|(x, y)| x.to_bits() == y.to_bits()));
            assert_eq!(a.contributor, b.contributor);
            assert_eq!(a.coverage, b.coverage);
        }
        let mut other = grid(100, |_| 999.0);
        other.nx = 6;
        other.ny = 2;
        let mut g = GridTrail::new(Extremum::Max, 3_600, 16);
        g.push(600, frames[2].1.clone());
        assert_eq!(
            g.push(100, other.clone()),
            GridMerge::Skipped(GridMismatch::Grid)
        );
        assert_eq!(g.push(900, other), GridMerge::Reset(GridMismatch::Grid));
        let (v, who) = g.at(900).unwrap().at_point(-97.99, 35.02).unwrap();
        assert_eq!((v, who), (Some(999.0), Some(900)));
    }

    #[test]
    fn a_point_reads_its_gate_and_the_frame_that_supplied_it() {
        let mut t = SlidingTrail::new(Extremum::Max, 3_600, 8);
        t.push(100, &frame(|i| if i == 5 { 200 } else { 50 }));
        t.push(400, &frame(|_| 120));
        let w = t.at(400).unwrap();
        // Every gate's lookup agrees with the raster: the same index the value came from.
        let s = &w.sweep;
        let mut hits = 0;
        for bin in 0..AZ {
            for gate in 0..GATES {
                let az = (bin as f64 + 0.5) * 360.0 / AZ as f64;
                let ground = (gate as f64 + 0.5) * s.gate_interval_km as f64;
                let (lat0, lon0) = (s.radar_lat as f64, s.radar_lon as f64);
                let dlat = ground * az.to_radians().cos() / 111.2;
                let dlon = ground * az.to_radians().sin() / (111.2 * lat0.to_radians().cos());
                let Some(i) = s.index_at(lon0 + dlon, lat0 + dlat) else {
                    continue;
                };
                let (sample, who) = w.at_point(lon0 + dlon, lat0 + dlat).unwrap();
                assert_eq!(who, w.contributor[i]);
                let code = s.data[i];
                assert_eq!(sample.value.is_some(), code >= 2);
                assert_eq!(who, Some(if code == 200 { 100 } else { 400 }));
                hits += 1;
            }
        }
        assert!(hits >= AZ * GATES / 2, "only {hits} gates located");
    }

    #[test]
    fn advancing_past_the_strongest_old_frame_removes_it() {
        let mut t = SlidingTrail::new(Extremum::Max, 600, 16);
        t.push(0, &frame(|_| 200));
        t.push(300, &frame(|_| 100));
        t.push(600, &frame(|_| 90));
        assert_eq!(t.at(600).unwrap().sweep.data[0], 200);
        // At 900 the 0 s frame is out of the window: its 200 is gone, not faded.
        t.push(900, &frame(|_| 80));
        let later = t.at(900).unwrap();
        assert_eq!(later.sweep.data[0], 100);
        assert_eq!(later.contributor[0], Some(300));
        assert_eq!(later.coverage.from, 300);
    }

    #[test]
    fn the_same_frames_in_any_order_give_the_same_trail() {
        let frames: Vec<(i64, BinnedSweep)> = (0..5)
            .map(|k| (300 * k, frame(move |i| (10 * k as usize + i) as u8 + 2)))
            .collect();
        let mut ordered = SlidingTrail::new(Extremum::Max, 3_600, 16);
        for (t, f) in &frames {
            ordered.push(*t, f);
        }
        let mut shuffled = SlidingTrail::new(Extremum::Max, 3_600, 16);
        for &k in &[3, 0, 4, 1, 2, 4] {
            shuffled.push(frames[k].0, &frames[k].1);
        }
        let (a, b) = (ordered.at(1_200).unwrap(), shuffled.at(1_200).unwrap());
        assert_eq!(a.sweep.data, b.sweep.data);
        assert_eq!(a.contributor, b.contributor);
        assert_eq!(a.coverage, b.coverage);
    }

    #[test]
    fn coverage_says_what_is_missing_and_a_short_history_is_not_the_full_window() {
        let mut t = SlidingTrail::new(Extremum::Max, 3_600, 64);
        // Every 300 s from 2400 to 3600, the 3000 s volume missing.
        for time in [2_400, 2_700, 3_300, 3_600] {
            t.push(time, &frame(|_| 50));
        }
        let c = t.at(3_600).unwrap().coverage;
        assert_eq!((c.from, c.to, c.frames, c.missing), (2_400, 3_600, 4, 1));
        assert_eq!(c.requested_s, 3_600);
        assert!(c.short_s > 0, "an hour asked for, 20 minutes held: {c:?}");
    }

    #[test]
    fn a_sliding_trail_stays_bounded_and_resets_on_another_beam() {
        let mut t = SlidingTrail::new(Extremum::Max, 100_000, 3);
        for k in 0..6 {
            t.push(300 * k, &frame(|_| 10));
        }
        assert_eq!(t.len(), 3, "the oldest go past the cap");
        let velocity = sweep(Moment::Velocity, &[50; AZ * GATES]);
        assert_eq!(t.push(3_000, &velocity), Merge::Reset(Mismatch::Moment));
        assert_eq!(t.len(), 1);
        assert_eq!(t.last_reset, Some(Mismatch::Moment));
    }
}
