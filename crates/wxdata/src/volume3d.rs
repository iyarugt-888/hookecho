//! 3D reflectivity volume: resample the stacked radar tilts onto a regular Cartesian grid
//! (radar-relative km) for GPU raymarching. Reuses the same 4/3-earth beam geometry and
//! per-column vertical interpolation as the cross-section reconstruction.

use crate::level2::BinnedSweep;
use crate::xsection::{beam_height_km, sample_profile};

/// A regular Cartesian reflectivity grid centered on the radar. `data` is the normalized
/// reflectivity index (`0` = empty, `2..=255` = dBZ over the REF range), laid out
/// `x + n*y + n*n*z` so it uploads directly as a `width=n, height=n, depth=nz` 3D texture.
/// x = east, y = north, z = up.
pub struct Volume3d {
    pub data: Vec<u8>,
    pub n: usize,
    pub nz: usize,
    /// Half-width of the horizontal box (km); x,y span `[-half_km, +half_km]`.
    pub half_km: f32,
    /// Top of the box (km); z spans `[0, top_km]`.
    pub top_km: f32,
    pub value_min: f32,
    pub value_max: f32,
}

/// The farthest ground range any of these tilts' *actual reported echo* reaches — not the gate
/// array's raw instrument capacity. Intended as `build`'s `half_km`, so the 3D volume covers real
/// echo instead of clipping it (superres reflectivity's declared capacity commonly reaches
/// 300-460 km, well past where any real storm's echo stops) — but also so it does NOT stretch the
/// volume out to that full theoretical reach for an ordinary nearby storm with nothing beyond it,
/// which would silently coarsen every close-in storm's cell size (the fixed `n`/`nz` grid spans
/// `half_km`, so a bigger box means bigger cells) for no benefit: a first version of this function
/// used the array's declared capacity directly and, because most VCPs' reflectivity tilt reports
/// hundreds of km of *capacity* regardless of where the echo actually is, ended up stretching
/// nearly every volume out to that near-maximum reach and washing out real detail (hail cores in
/// particular) that used to be visible at the old fixed-150-km resolution.
///
/// `0`/`1` are the binner's own below-threshold/range-folded sentinels (see [`BinnedSweep::data`]'s
/// own doc comment); anything `>= 2` is a real reported value.
///
/// A single stray gate does not count: ground-clutter/anomalous-propagation breakthrough under a
/// temperature inversion classically shows up as an isolated speckle far past a storm's real
/// echo, on one or two azimuths at one or two gates — exactly the "surprisingly large half_km on
/// an otherwise unremarkable volume" case that motivated requiring a real, spatially coherent run
/// of echo (see [`MIN_CONTIGUOUS_KM`]) rather than trusting the single farthest reported gate.
pub fn max_sample_range_km(sweeps: &[BinnedSweep]) -> f32 {
    let mut max_range = 0.0f32;
    for s in sweeps {
        if s.gate_count == 0 || s.gate_interval_km <= 0.0 {
            continue;
        }
        let min_run = ((MIN_CONTIGUOUS_KM / s.gate_interval_km).ceil() as usize).max(1);
        let farthest_gate = s
            .data
            .chunks_exact(s.gate_count)
            .filter_map(|row| farthest_gate_ending_a_run(row, min_run))
            .max();
        if let Some(gate) = farthest_gate {
            let range = s.first_gate_km + (gate + 1) as f32 * s.gate_interval_km;
            max_range = max_range.max(range);
        }
    }
    max_range
}

/// How much contiguous real echo, radially, counts as a genuine return rather than a speckle —
/// a real storm's echo is essentially always far larger than this in every dimension.
const MIN_CONTIGUOUS_KM: f32 = 1.5;

/// The farthest index in `row` that ends an unbroken run of at least `min_run` gates `>= 2`,
/// scanning from the far end inward. `None` if no such run exists.
fn farthest_gate_ending_a_run(row: &[u8], min_run: usize) -> Option<usize> {
    let mut run = 0usize;
    for (i, &v) in row.iter().enumerate().rev() {
        if v >= 2 {
            run += 1;
            if run >= min_run {
                return Some(i + min_run - 1);
            }
        } else {
            run = 0;
        }
    }
    None
}

/// Build an `n × n × nz` reflectivity volume out to `half_km` horizontally and `top_km` up.
/// Returns `None` if there are no sweeps.
pub fn build(
    sweeps: &[BinnedSweep],
    n: usize,
    nz: usize,
    half_km: f32,
    top_km: f32,
) -> Option<Volume3d> {
    let s0 = sweeps.first()?;
    let (value_min, value_max) = (s0.value_min, s0.value_max);
    let span = (value_max - value_min).max(f32::EPSILON);
    let n = n.max(2);
    let nz = nz.max(2);
    let mut data = vec![0u8; n * n * nz];

    for j in 0..n {
        let y = -half_km as f64 + 2.0 * half_km as f64 * j as f64 / (n - 1) as f64;
        for i in 0..n {
            let x = -half_km as f64 + 2.0 * half_km as f64 * i as f64 / (n - 1) as f64;
            let ground = (x * x + y * y).sqrt();
            if ground < 0.5 {
                continue; // cone of silence at the radar
            }
            let az = x.atan2(y).to_degrees().rem_euclid(360.0);

            // Vertical profile of (beam_height, dBZ) from every tilt at this ground range/azimuth.
            let mut samples: Vec<(f64, f32)> = Vec::with_capacity(sweeps.len());
            for s in sweeps {
                let e = s.elevation_deg as f64;
                let slant = ground / e.to_radians().cos();
                let gate = ((slant - s.first_gate_km as f64)
                    / s.gate_interval_km.max(f32::EPSILON) as f64)
                    .round();
                if gate < 0.0 || gate as usize >= s.gate_count {
                    continue;
                }
                let bin = ((az / 360.0 * s.az_bins as f64) as usize) % s.az_bins;
                let idx = s.data[bin * s.gate_count + gate as usize];
                if idx < 2 {
                    continue;
                }
                let h = beam_height_km(slant, e);
                let v = value_min + (idx as f32 - 2.0) / 253.0 * span;
                samples.push((h, v));
            }
            if samples.is_empty() {
                continue;
            }
            samples.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap_or(std::cmp::Ordering::Equal));

            for k in 0..nz {
                let z = top_km as f64 * k as f64 / (nz - 1) as f64;
                if let (Some(v), _) = sample_profile(&samples, z) {
                    let t = ((v - value_min) / span).clamp(0.0, 1.0);
                    data[i + n * j + n * n * k] = 2 + (t * 253.0) as u8;
                }
            }
        }
    }

    Some(Volume3d {
        data,
        n,
        nz,
        half_km,
        top_km,
        value_min,
        value_max,
    })
}

/// Flip the volume's index mapping end for end: the voxel that held `value_min` now holds
/// `value_max`'s index and vice versa, with everything between mirrored the same way. `value_min`/
/// `value_max` themselves are left untouched — they still describe the true physical range, just
/// no longer the direction the index counts toward.
///
/// Built for correlation coefficient. [`build`]'s index is directly proportional to the physical
/// value, which is right for reflectivity: the raymarch takes a maximum, so raising its floor
/// carves away weak echo and leaves the storm cores standing alone. CC is the opposite ask — a
/// tornado debris signature is a *lofted low-CC pocket*, and a maximum-of-CC raymarch only ever
/// finds the ordinary high-CC rain around it. Inverting first makes "low CC" the voxel that wins
/// the maximum, so the same floor/opacity controls that isolate a reflectivity core isolate a
/// debris signature instead. The caller must pair this with a matching LUT permutation
/// (`colormap::invert_lut` in the `hookecho` crate) so a voxel's color still comes from its true
/// physical value, not its flipped index.
pub fn invert_in_place(v3: &mut Volume3d) {
    for b in v3.data.iter_mut() {
        if *b >= 2 {
            *b = (257 - *b as u16) as u8;
        }
    }
}

/// A constant-altitude PPI (CAPPI): an `n × n` horizontal slice of reflectivity (dBZ) at a fixed
/// altitude, radar-centered. `dbz[x + n*y]` with row 0 = north (y inverted for image display).
pub struct Cappi {
    pub n: usize,
    pub half_km: f32,
    pub alt_km: f32,
    /// dBZ per cell, `None` where no beam sampled that altitude.
    pub dbz: Vec<Option<f32>>,
}

/// Re-slice the stacked tilts at a single altitude `alt_km` into an `n × n` CAPPI out to
/// `half_km`. Mirrors [`build`]'s inner column loop but samples one height. `None` if no sweeps.
pub fn cappi(sweeps: &[BinnedSweep], alt_km: f32, n: usize, half_km: f32) -> Option<Cappi> {
    let s0 = sweeps.first()?;
    let (value_min, value_max) = (s0.value_min, s0.value_max);
    let span = (value_max - value_min).max(f32::EPSILON);
    let n = n.max(2);
    let mut dbz = vec![None; n * n];

    for j in 0..n {
        // Row 0 = north: invert y so the image displays north-up.
        let y = half_km as f64 - 2.0 * half_km as f64 * j as f64 / (n - 1) as f64;
        for i in 0..n {
            let x = -half_km as f64 + 2.0 * half_km as f64 * i as f64 / (n - 1) as f64;
            let ground = (x * x + y * y).sqrt();
            if ground < 0.5 {
                continue;
            }
            let az = x.atan2(y).to_degrees().rem_euclid(360.0);
            let mut samples: Vec<(f64, f32)> = Vec::with_capacity(sweeps.len());
            for s in sweeps {
                let e = s.elevation_deg as f64;
                let slant = ground / e.to_radians().cos();
                let gate = ((slant - s.first_gate_km as f64)
                    / s.gate_interval_km.max(f32::EPSILON) as f64)
                    .round();
                if gate < 0.0 || gate as usize >= s.gate_count {
                    continue;
                }
                let bin = ((az / 360.0 * s.az_bins as f64) as usize) % s.az_bins;
                let idx = s.data[bin * s.gate_count + gate as usize];
                if idx < 2 {
                    continue;
                }
                let h = beam_height_km(slant, e);
                let v = value_min + (idx as f32 - 2.0) / 253.0 * span;
                samples.push((h, v));
            }
            if samples.is_empty() {
                continue;
            }
            samples.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap_or(std::cmp::Ordering::Equal));
            dbz[i + n * j] = sample_profile(&samples, alt_km as f64).0;
        }
    }

    Some(Cappi {
        n,
        half_km,
        alt_km,
        dbz,
    })
}

/// Clip slab that isolates one storm inside the volume box, in the `[x0, x1, y0, y1, z0, z1]`
/// fractions [`crate::volume3d`]'s consumers use.
///
/// `dx_km`/`dy_km` are the storm's offset east and north of the radar, `pad_km` the half-width to
/// keep around it. The vertical span is left alone: the whole point of looking at a storm in 3D is
/// its depth, so cropping height would throw away the answer.
pub fn clip_around(half_km: f32, dx_km: f32, dy_km: f32, pad_km: f32) -> [f32; 6] {
    // The box spans [-half_km, +half_km] in x and y, mapped onto 0..1.
    let frac = |km: f32| ((km + half_km) / (2.0 * half_km)).clamp(0.0, 1.0);
    let pad = (pad_km / (2.0 * half_km)).clamp(0.0, 0.5);
    let span = |c: f32| {
        let (mut lo, mut hi) = (c - pad, c + pad);
        // A storm near the edge of the box slides its window inward rather than shrinking it.
        if lo < 0.0 {
            (lo, hi) = (0.0, (2.0 * pad).min(1.0));
        } else if hi > 1.0 {
            (lo, hi) = ((1.0 - 2.0 * pad).max(0.0), 1.0);
        }
        (lo, hi)
    };
    let (x0, x1) = span(frac(dx_km));
    let (y0, y1) = span(frac(dy_km));
    [x0, x1, y0, y1, 0.0, 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level2::{BinnedSweep, Moment};

    /// A synthetic single-tilt sweep with one hot gate at a known azimuth/range.
    fn sweep(elev: f32) -> BinnedSweep {
        let (az_bins, gate_count) = (720usize, 200usize);
        let mut data = vec![0u8; az_bins * gate_count];
        // Strong echo in an east-facing wedge (az ~75°..105° → bins 150..210) at ~40..60 km.
        for bin in 150..210 {
            for g in 40..60 {
                data[bin * gate_count + g] = 200;
            }
        }
        let (value_min, value_max) = Moment::Reflectivity.value_range();
        BinnedSweep {
            moment: Moment::Reflectivity,
            az_bins,
            gate_count,
            data,
            first_gate_km: 0.0,
            gate_interval_km: 1.0,
            radar_lat: 35.0,
            radar_lon: -97.0,
            elevation_deg: elev,
            value_min,
            value_max,
            ..Default::default()
        }
    }

    /// A sweep whose gate array could hold `gate_count` gates out to a large declared capacity,
    /// but whose only real echo (index `>= 2`) is a genuine 10-gate-wide run ending at
    /// `echo_gate` — everything past it, and everything before the run, is the binner's own
    /// below-threshold sentinel (`0`), same as a real clear-air tail past a storm's actual echo.
    fn sweep_with_echo_at(
        gate_count: usize,
        gate_interval_km: f32,
        first_gate_km: f32,
        echo_gate: usize,
    ) -> BinnedSweep {
        let az_bins = 720usize;
        let mut data = vec![0u8; az_bins * gate_count];
        let run_start = echo_gate.saturating_sub(9);
        for bin in 150..210 {
            for g in run_start..=echo_gate {
                data[bin * gate_count + g] = 200;
            }
        }
        let (value_min, value_max) = Moment::Reflectivity.value_range();
        BinnedSweep {
            moment: Moment::Reflectivity,
            az_bins,
            gate_count,
            data,
            first_gate_km,
            gate_interval_km,
            radar_lat: 35.0,
            radar_lon: -97.0,
            elevation_deg: 0.5,
            value_min,
            value_max,
            ..Default::default()
        }
    }

    #[test]
    fn max_sample_range_km_follows_real_echo_not_the_gate_arrays_declared_capacity() {
        // Every synthetic tilt shares the same gate layout (200 gates, 1 km apart, from 0 km),
        // with real echo only out to gate 59 — a 60 km storm, not a 200 km one. Superres
        // reflectivity commonly *declares* 300-460 km of gate capacity regardless of where the
        // echo actually is; using that capacity directly (an earlier version of this function did)
        // stretched the volume out to nearly that full reach for every ordinary nearby storm,
        // coarsening cell size and washing out real detail like hail cores for no benefit.
        let sweeps = vec![sweep(0.5), sweep(1.5), sweep(2.4)];
        assert_eq!(max_sample_range_km(&sweeps), 60.0);

        // A tilt whose real echo reaches farther than the others must win, not be averaged away
        // or ignored in favor of the first one in the list — even with a different gate layout.
        let far = sweep_with_echo_at(800, 0.5, 0.25, 700);
        let with_far = vec![sweep(0.5), far, sweep(2.4)];
        assert_eq!(max_sample_range_km(&with_far), 0.25 + 701.0 * 0.5);

        // A tilt with no real echo anywhere (an empty gate array, or nothing above threshold)
        // contributes nothing — it must not win over a tilt that actually has echo, and a set of
        // only such tilts must not panic.
        let mut clear_air = sweep_with_echo_at(200, 1.0, 0.0, 0);
        clear_air.data.fill(0);
        assert_eq!(max_sample_range_km(&[clear_air.clone()]), 0.0);
        assert_eq!(max_sample_range_km(&[clear_air, sweep(0.5)]), 60.0);

        assert_eq!(max_sample_range_km(&[]), 0.0, "no tilts, no range");
    }

    #[test]
    fn an_isolated_speckle_far_past_the_real_echo_does_not_win() {
        // Ground-clutter/anomalous-propagation breakthrough under a temperature inversion is the
        // classic false-far-echo case: one or two stray gates, nowhere near a genuine storm's
        // spatial extent. A single bad reading here must not stretch the volume out to it.
        let mut with_speckle = sweep(0.5);
        // The real echo (a 20-gate run) is already at gates 40..60; add an isolated 1-gate blip
        // at gate 150 on a handful of azimuths — nowhere close to the 10-gate run
        // `farthest_gate_ending_a_run` requires.
        for bin in 150..156 {
            with_speckle.data[bin * 200 + 150] = 200;
        }
        assert_eq!(
            max_sample_range_km(std::slice::from_ref(&with_speckle)),
            60.0,
            "an isolated far speckle must not move the result past the real echo"
        );

        // The same far gates, but as a genuine run (not a speckle), DO win — this isn't "ignore
        // anything past 60 km", it's "ignore anything that isn't real echo".
        let mut with_real_far_echo = sweep(0.5);
        for bin in 150..156 {
            for g in 141..=150 {
                with_real_far_echo.data[bin * 200 + g] = 200;
            }
        }
        assert_eq!(
            max_sample_range_km(std::slice::from_ref(&with_real_far_echo)),
            151.0
        );
    }

    #[test]
    fn builds_nonempty_volume_with_echo() {
        let sweeps = vec![sweep(0.5), sweep(1.5), sweep(2.4)];
        let v = build(&sweeps, 64, 24, 120.0, 18.0).unwrap();
        assert_eq!(v.data.len(), 64 * 64 * 24);
        let filled = v.data.iter().filter(|&&b| b >= 2).count();
        assert!(filled > 0, "east-side echo should populate some voxels");
    }

    #[test]
    fn cappi_slices_east_echo_and_respects_row_order() {
        // The synthetic echo sits east (az ~90°) at ~40..60 km, low altitude.
        let sweeps = vec![sweep(0.5), sweep(1.5), sweep(2.4)];
        let c = cappi(&sweeps, 2.0, 128, 120.0).unwrap();
        assert_eq!(c.dbz.len(), 128 * 128);
        let filled = c.dbz.iter().filter(|v| v.is_some()).count();
        assert!(filled > 0, "2 km slice should catch the low echo");
        // Filled cells cluster on the east half (x > 0); the west half stays empty.
        let mid = 128 / 2;
        let mut east_filled = 0;
        let mut west_filled = 0;
        for j in 0..128 {
            for i in 0..128 {
                if c.dbz[i + 128 * j].is_some() {
                    if i > mid {
                        east_filled += 1
                    } else {
                        west_filled += 1
                    }
                }
            }
        }
        assert!(east_filled > 0, "east echo present");
        assert_eq!(west_filled, 0, "west empty");
        // Far above the echo there's nothing.
        let high = cappi(&sweeps, 14.0, 128, 120.0).unwrap();
        assert_eq!(
            high.dbz.iter().filter(|v| v.is_some()).count(),
            0,
            "14 km empty"
        );
    }
}

#[cfg(test)]
mod clip_tests {
    use super::clip_around;

    #[test]
    fn the_slab_brackets_the_storm_and_slides_in_at_the_edge() {
        // Dead center of a 150 km box, 25 km of padding: an eighth of the box either side.
        let c = clip_around(150.0, 0.0, 0.0, 25.0);
        assert!((c[0] - 0.4167).abs() < 1e-3 && (c[1] - 0.5833).abs() < 1e-3);
        // Height is never cropped.
        assert_eq!([c[4], c[5]], [0.0, 1.0]);
        // Hard against the west wall: the window slides inward instead of collapsing.
        let w = clip_around(150.0, -150.0, 0.0, 25.0);
        assert_eq!(w[0], 0.0);
        assert!((w[1] - w[0] - (c[1] - c[0])).abs() < 1e-3);
        // And against the north wall.
        let n = clip_around(150.0, 0.0, 150.0, 25.0);
        assert_eq!(n[3], 1.0);
        assert!((n[3] - n[2] - (c[1] - c[0])).abs() < 1e-3);
    }
}
