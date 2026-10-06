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

/// Share of the echo the auto-cropped box must contain. The rest is reported, not silently lost.
pub const ECHO_COVERAGE: f64 = 0.99;

/// How far the box extends and how much echo that leaves outside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extent {
    pub half_km: f32,
    /// Fraction of all echo gates (0..=1) beyond `half_km`, i.e. not in the volume.
    pub outside: f64,
}

/// Ground range holding [`ECHO_COVERAGE`] of the echo gates in `sweeps`, rounded up to 25 km and
/// floored at 50 km, never beyond `full_km` (normally [`max_sample_range_km`]).
///
/// [`max_sample_range_km`] only asks for a 1.5 km contiguous run, which AP and clutter satisfy,
/// so a few percent of far speckle stretched the box (and every cell in it) 2-3x. This weighs by
/// how much echo is actually there instead, and reports what it leaves out.
pub fn echo_extent_km(sweeps: &[BinnedSweep], full_km: f32) -> Extent {
    const BIN_KM: f32 = 5.0;
    let full_km = full_km.max(50.0);
    let bins = (full_km / BIN_KM).ceil() as usize + 2;
    let mut hist = vec![0u64; bins];
    let mut total = 0u64;
    for s in sweeps {
        if s.gate_count == 0 {
            continue;
        }
        let cos_e = s.elevation_deg.to_radians().cos().max(0.05);
        for row in s.data.chunks_exact(s.gate_count) {
            for (g, &v) in row.iter().enumerate() {
                if v >= 2 {
                    let ground = (s.first_gate_km + (g as f32 + 0.5) * s.gate_interval_km) * cos_e;
                    let b = ((ground / BIN_KM) as usize).min(bins - 1);
                    hist[b] += 1;
                    total += 1;
                }
            }
        }
    }
    if total == 0 {
        return Extent {
            half_km: 50.0f32.min(full_km),
            outside: 0.0,
        };
    }
    let want = (total as f64 * ECHO_COVERAGE).ceil() as u64;
    let (mut acc, mut edge_km) = (0u64, BIN_KM);
    for (b, &c) in hist.iter().enumerate() {
        acc += c;
        edge_km = (b + 1) as f32 * BIN_KM;
        if acc >= want {
            break;
        }
    }
    let half_km = ((edge_km / 25.0).ceil() * 25.0).clamp(50.0, full_km);
    let inside: u64 = hist
        .iter()
        .enumerate()
        .filter(|(b, _)| ((*b + 1) as f32 * BIN_KM) <= half_km)
        .map(|(_, &c)| c)
        .sum();
    Extent {
        half_km,
        outside: 1.0 - inside as f64 / total as f64,
    }
}

/// Grid size for a box: `(n, nz)`. Aims for `target_cell_km` horizontally (the native gate
/// spacing), 250 m vertically, then shrinks to fit `max_voxels` and the device's 3D texture
/// limit `max_dim`. Never below 64 x 64 x 16.
pub fn plan_grid(
    half_km: f32,
    top_km: f32,
    target_cell_km: f32,
    max_voxels: usize,
    max_dim: usize,
) -> (usize, usize) {
    let max_dim = max_dim.max(64);
    let nz_want = ((top_km / 0.25).ceil() as usize + 1).clamp(16, 96);
    let n_want = ((2.0 * half_km / target_cell_km.max(0.01)).ceil() as usize + 1).max(64);
    let mut nz = nz_want.min(max_dim);
    let mut n = n_want.min(max_dim);
    if n * n * nz > max_voxels {
        // Horizontal detail is what the data has, so trade vertical levels away first.
        nz = nz.clamp(16, 48);
        n = ((max_voxels / nz) as f64).sqrt() as usize;
        n = n.clamp(64, max_dim);
    }
    (n, nz)
}

/// Build an `n × n × nz` reflectivity volume out to `half_km` horizontally and `top_km` up.
/// Returns `None` if there are no sweeps.
/// The chosen tilts as they were actually scanned: each one's beam, at its true height at every
/// range and `beam_deg` thick (the half-power beamwidth, 0.95° for a WSR-88D), on the same grid as
/// [`build`]. Nothing is interpolated between tilts, so the gaps a VCP leaves between its cones
/// stay gaps — the layer-by-layer view the 3D map's Observed mode gives, in the orbit window.
/// Where two cones overlap, the stronger value shows. A shell is never thinner than one grid
/// cell, so a narrow beam near the radar does not vanish between voxel centres.
pub fn build_shells(
    sweeps: &[BinnedSweep],
    n: usize,
    nz: usize,
    half_km: f32,
    top_km: f32,
    beam_deg: f32,
) -> Option<Volume3d> {
    let s0 = sweeps.first()?;
    let (value_min, value_max) = (s0.value_min, s0.value_max);
    let span = (value_max - value_min).max(f32::EPSILON);
    let (n, nz) = (n.max(2), nz.max(2));
    let dz = top_km as f64 / (nz - 1) as f64;
    let half_beam = (beam_deg as f64 / 2.0).to_radians();
    let mut data = vec![0u8; n * n * nz];
    for j in 0..n {
        let y = -half_km as f64 + 2.0 * half_km as f64 * j as f64 / (n - 1) as f64;
        for i in 0..n {
            let x = -half_km as f64 + 2.0 * half_km as f64 * i as f64 / (n - 1) as f64;
            let ground = (x * x + y * y).sqrt();
            if ground < 0.5 {
                continue;
            }
            let az = x.atan2(y).to_degrees().rem_euclid(360.0);
            for s in sweeps {
                // The gate under this ground point by the one 4/3-earth nearest-gate rule every
                // vertical profile in this crate uses (`xsection::gate_over_ground`), so a voxel,
                // the cross-section and the derived grids read the same gate for the same point.
                let e = s.elevation_deg as f64;
                let Some((cell, slant)) = crate::xsection::gate_over_ground(s, ground, az) else {
                    continue;
                };
                let idx = s.data[cell];
                if idx < 2 {
                    continue;
                }
                // Re-normalise to this grid's range (every sweep of one moment shares it).
                let v = s.value_min + (idx as f32 - 2.0) / 253.0 * (s.value_max - s.value_min);
                let out = 2 + (((v - value_min) / span).clamp(0.0, 1.0) * 253.0) as u8;
                let h = beam_height_km(slant, e);
                let thick = (slant * half_beam.tan()).max(dz * 0.6);
                let k0 = ((h - thick) / dz).ceil().max(0.0) as usize;
                let k1 = (((h + thick) / dz).floor() as usize).min(nz - 1);
                for k in k0..=k1 {
                    let cell = &mut data[i + n * j + n * n * k];
                    *cell = (*cell).max(out);
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

    // One row of the grid (fixed `j`), laid out `k * n + i` so a row is a run of `nz` contiguous
    // x-spans that copy straight into the volume. Rows are independent, which is what lets the
    // native build spread them across threads.
    let row = |j: usize| -> Vec<u8> {
        let mut out = vec![0u8; n * nz];
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
                // The gate under this ground point by the one 4/3-earth nearest-gate rule every
                // vertical profile in this crate uses (`xsection::gate_over_ground`), so a voxel,
                // the cross-section and the derived grids read the same gate for the same point.
                let e = s.elevation_deg as f64;
                let Some((cell, slant)) = crate::xsection::gate_over_ground(s, ground, az) else {
                    continue;
                };
                let idx = s.data[cell];
                if idx < 2 {
                    continue;
                }
                let h = beam_height_km(slant, e);
                // Each sweep decodes with its own range: velocity's Nyquist differs by tilt.
                let v = s.value_min + (idx as f32 - 2.0) / 253.0 * (s.value_max - s.value_min);
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
                    out[k * n + i] = 2 + (t * 253.0) as u8;
                }
            }
        }
        out
    };

    // A column solve per cell: worth every thread on native. wasm has none.
    #[cfg(not(target_arch = "wasm32"))]
    let rows: Vec<Vec<u8>> = {
        use rayon::prelude::*;
        (0..n).into_par_iter().map(row).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let rows: Vec<Vec<u8>> = (0..n).map(row).collect();

    let mut data = vec![0u8; n * n * nz];
    for (j, r) in rows.iter().enumerate() {
        for k in 0..nz {
            let dst = n * j + n * n * k;
            data[dst..dst + n].copy_from_slice(&r[k * n..(k + 1) * n]);
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

/// Clear every voxel of `v3` where `by` — a volume on the same grid, usually reflectivity — is
/// empty or below `min_value`. The polarimetric fields are only trustworthy where there is
/// signal: in weak echo ZDR and KDP are noise, and a maximum-intensity raymarch finds that noise
/// first, painting the whole volume one colour. Masking at ~20 dBZ keeps a ZDR column or a KDP
/// core and drops the speckle. Returns `false` (and leaves `v3` alone) when the grids differ.
pub fn mask_by(v3: &mut Volume3d, by: &Volume3d, min_value: f32) -> bool {
    if v3.n != by.n || v3.nz != by.nz || (v3.half_km - by.half_km).abs() > 1e-3 {
        return false;
    }
    let span = (by.value_max - by.value_min).max(f32::EPSILON);
    let floor = (2.0 + ((min_value - by.value_min) / span).clamp(0.0, 1.0) * 253.0).ceil() as u8;
    for (v, &b) in v3.data.iter_mut().zip(&by.data) {
        if b < 2 || b < floor {
            *v = 0;
        }
    }
    true
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

/// Speed steps in a speed-ordered index: `2 + 2·step + inbound` fills `2..=255` exactly.
pub const SPEED_STEPS: u16 = 126;

/// The speed-ordered index of radial velocity `v` (m/s) on a `±vmax` scale: twice the speed step,
/// plus one for inbound (negative), so a larger index always means a faster wind and a maximum
/// over indices finds the fastest wind either way.
pub fn speed_index(v: f32, vmax: f32) -> u8 {
    let step = ((v.abs() / vmax.max(f32::EPSILON)).clamp(0.0, 1.0) * SPEED_STEPS as f32).round();
    (2 + 2 * step as u16 + u16::from(v < 0.0)) as u8
}

/// The radial velocity (m/s) a speed-ordered index stands for; the inverse of [`speed_index`].
pub fn speed_value(idx: u8, vmax: f32) -> f32 {
    let i = idx.max(2) as u16 - 2;
    let speed = (i / 2) as f32 / SPEED_STEPS as f32 * vmax;
    if i % 2 == 1 {
        -speed
    } else {
        speed
    }
}

/// Re-index a velocity volume built by [`build`] (index linear in `value_min..value_max`) by
/// speed, keeping the sign ([`speed_index`], on a `±max(|value_min|, |value_max|)` scale).
///
/// The raymarch keeps the largest index along each ray. On a plain velocity volume that is the
/// fastest *outbound* wind, and the inbound half of every couplet disappears; ordered by speed,
/// a ray finds the fastest wind in either direction, and the colour table (permuted to match)
/// still paints it inbound or outbound. `value_min`/`value_max` become `-vmax`/`+vmax`.
///
/// The GPU's trilinear filtering blends neighbouring indices. Where strong inbound meets strong
/// outbound (a couplet's gate-to-gate shear) a blended sample can land on either sign, so the
/// boundary is drawn one colour or the other there, never a false weak value in between.
pub fn fold_by_speed(v3: &mut Volume3d) {
    let vmax = v3.value_min.abs().max(v3.value_max.abs());
    let span = (v3.value_max - v3.value_min).max(f32::EPSILON);
    for b in v3.data.iter_mut() {
        if *b >= 2 {
            let v = v3.value_min + (*b as f32 - 2.0) / 253.0 * span;
            *b = speed_index(v, vmax);
        }
    }
    v3.value_min = -vmax;
    v3.value_max = vmax;
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
    sweeps.first()?;
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
                // The gate under this ground point by the one 4/3-earth nearest-gate rule every
                // vertical profile in this crate uses (`xsection::gate_over_ground`), so a voxel,
                // the cross-section and the derived grids read the same gate for the same point.
                let e = s.elevation_deg as f64;
                let Some((cell, slant)) = crate::xsection::gate_over_ground(s, ground, az) else {
                    continue;
                };
                let idx = s.data[cell];
                if idx < 2 {
                    continue;
                }
                let h = beam_height_km(slant, e);
                // Each sweep decodes with its own range: velocity's Nyquist differs by tilt.
                let v = s.value_min + (idx as f32 - 2.0) / 253.0 * (s.value_max - s.value_min);
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

    #[test]
    fn speed_index_orders_by_speed_and_keeps_the_sign() {
        let vmax = 127.0;
        // Faster wins either way, and the sign survives the round trip.
        assert!(speed_index(-40.0, vmax) > speed_index(30.0, vmax));
        assert!(speed_index(40.0, vmax) > speed_index(-30.0, vmax));
        for v in [-127.0, -40.0, -0.6, 0.0, 12.0, 127.0] {
            let back = speed_value(speed_index(v, vmax), vmax);
            assert!((back - v).abs() <= 0.51, "{v} -> {back}");
            assert_eq!(back < 0.0, v < -0.51, "{v}");
        }
        // The full scale fills the index space and never reaches the empty/folded sentinels.
        assert_eq!(speed_index(127.0, vmax), 254);
        assert_eq!(speed_index(-127.0, vmax), 255);
        assert_eq!(speed_index(0.0, vmax), 2);
    }

    #[test]
    fn fold_by_speed_lets_a_maximum_find_the_inbound_half() {
        let (lo, hi) = Moment::Velocity.value_range();
        let enc = |v: f32| 2 + (((v - lo) / (hi - lo)) * 253.0) as u8;
        let mut v3 = Volume3d {
            data: vec![0, enc(20.0), enc(-35.0)],
            n: 1,
            nz: 3,
            half_km: 1.0,
            top_km: 1.0,
            value_min: lo,
            value_max: hi,
        };
        fold_by_speed(&mut v3);
        assert_eq!(v3.data[0], 0, "empty stays empty");
        let strongest = *v3.data.iter().max().unwrap();
        let v = speed_value(strongest, v3.value_max);
        assert!((v + 35.0).abs() < 1.5, "the inbound 35 m/s wins, got {v}");
    }

    /// Every voxel column reads exactly the gates the cross-section reads at the same ground
    /// point, out to long range where a flat-earth slant lands on the neighbouring gate.
    #[test]
    fn a_voxel_column_reads_the_cross_sections_gates() {
        let (az_bins, gate_count) = (360usize, 1400usize);
        // A different value on every gate, so a one-gate shift is a different value.
        let sweeps: Vec<BinnedSweep> = [0.5f32, 1.5, 3.0, 6.0]
            .iter()
            .map(|&e| {
                let mut data = vec![0u8; az_bins * gate_count];
                for b in 0..az_bins {
                    for g in 0..gate_count {
                        data[b * gate_count + g] = 2 + ((g * 7 + b) % 250) as u8;
                    }
                }
                BinnedSweep {
                    moment: Moment::Reflectivity,
                    az_bins,
                    gate_count,
                    data,
                    first_gate_km: 2.0,
                    gate_interval_km: 0.25,
                    radar_lat: 35.0,
                    radar_lon: -97.0,
                    elevation_deg: e,
                    value_min: -32.0,
                    value_max: 95.0,
                    ..Default::default()
                }
            })
            .collect();
        let (n, nz, half_km, top_km) = (121usize, 40usize, 330.0f32, 18.0f32);
        let v3 = build(&sweeps, n, nz, half_km, top_km).unwrap();
        let span = v3.value_max - v3.value_min;
        let mut samples = Vec::new();
        let mut checked = 0;
        for j in (0..n).step_by(9) {
            for i in (0..n).step_by(7) {
                let x = -half_km as f64 + 2.0 * half_km as f64 * i as f64 / (n - 1) as f64;
                let y = -half_km as f64 + 2.0 * half_km as f64 * j as f64 / (n - 1) as f64;
                let ground = (x * x + y * y).sqrt();
                if ground < 0.5 {
                    continue;
                }
                let az = x.atan2(y).to_degrees().rem_euclid(360.0);
                crate::xsection::column_samples(&sweeps, ground, az, &mut samples);
                for k in 0..nz {
                    let z = top_km as f64 * k as f64 / (nz - 1) as f64;
                    let want = match sample_profile(&samples, z).0 {
                        Some(v) => 2 + (((v - v3.value_min) / span).clamp(0.0, 1.0) * 253.0) as u8,
                        None => 0,
                    };
                    assert_eq!(
                        v3.data[i + n * j + n * n * k],
                        want,
                        "voxel {i},{j},{k} at {ground:.0} km"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 5_000, "{checked}");
    }

    #[test]
    fn extent_ignores_a_thin_far_speckle_and_reports_it() {
        // 0.25 km gates: dense echo out to 100 km, plus a few far gates at ~300 km.
        let (az_bins, gate_count) = (360usize, 1300usize);
        let mut data = vec![0u8; az_bins * gate_count];
        for bin in 0..az_bins {
            for g in 0..400 {
                data[bin * gate_count + g] = 100;
            }
        }
        for g in 1180..1190 {
            data[10 * gate_count + g] = 100;
        }
        let s = BinnedSweep {
            az_bins,
            gate_count,
            data,
            first_gate_km: 0.0,
            gate_interval_km: 0.25,
            elevation_deg: 0.0,
            ..Default::default()
        };
        let e = echo_extent_km(&[s], 350.0);
        assert_eq!(e.half_km, 100.0);
        assert!(e.outside > 0.0 && e.outside < 0.005, "{e:?}");
    }

    #[test]
    fn plan_grid_respects_budget_and_device_limit() {
        let (n, nz) = plan_grid(150.0, 18.0, 0.25, 40_000_000, 2048);
        assert!(n * n * nz <= 40_000_000);
        assert!(n > 500);
        let (n, _) = plan_grid(150.0, 18.0, 0.25, 400_000_000, 256);
        assert_eq!(n, 256);
        // A small box reaches native spacing.
        let (n, _) = plan_grid(50.0, 18.0, 0.25, 40_000_000, 2048);
        assert_eq!(n, 401);
    }

    #[test]
    fn build_places_echo_where_the_sweep_has_it() {
        let v = build(&[sweep(0.5)], 96, 12, 100.0, 10.0).unwrap();
        // East of the radar (x > 0, y ~ 0) at ~50 km carries echo; due west does not.
        let (j, k) = (48usize, 0usize);
        let east = v.data[(48 + 24) + 96 * j + 96 * 96 * k];
        let west = v.data[(48 - 24) + 96 * j + 96 * 96 * k];
        assert!(east >= 2, "east {east}");
        assert_eq!(west, 0);
    }

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

    #[test]
    fn a_volume_is_masked_where_the_other_is_weak_or_empty() {
        use super::{mask_by, Volume3d};
        let grid = |data: Vec<u8>, lo: f32, hi: f32| Volume3d {
            data,
            n: 2,
            nz: 1,
            half_km: 10.0,
            top_km: 5.0,
            value_min: lo,
            value_max: hi,
        };
        // Reflectivity over -30..80 dBZ: empty, ~0 dBZ, ~25 dBZ, ~60 dBZ.
        let dbz = |v: f32| 2 + ((v + 30.0) / 110.0 * 253.0) as u8;
        let refl = grid(vec![0, dbz(0.0), dbz(25.0), dbz(60.0)], -30.0, 80.0);
        let mut zdr = grid(vec![200, 200, 200, 200], -8.0, 8.0);
        assert!(mask_by(&mut zdr, &refl, 20.0));
        assert_eq!(zdr.data, [0, 0, 200, 200]);
        let mut other = grid(vec![1; 8], -8.0, 8.0);
        other.n = 4;
        assert!(
            !mask_by(&mut other, &refl, 20.0),
            "different grids are refused"
        );
    }

    #[test]
    fn a_single_tilt_is_a_cone_shell_at_its_beam_height() {
        use super::build_shells;
        use crate::level2::{BinnedSweep, Moment};
        // One 2.0° tilt, every gate 40-ish dBZ, 1 km gates out to 150 km.
        let sweep = BinnedSweep {
            moment: Moment::Reflectivity,
            az_bins: 360,
            gate_count: 150,
            data: vec![150; 360 * 150],
            first_gate_km: 0.0,
            gate_interval_km: 1.0,
            radar_lat: 35.0,
            radar_lon: -97.0,
            elevation_deg: 2.0,
            value_min: -32.0,
            value_max: 95.0,
            ..Default::default()
        };
        let (n, nz) = (61, 41);
        let v = build_shells(&[sweep], n, nz, 120.0, 10.0, 0.95).unwrap();
        // At 100 km east the beam centre is ~4.1 km up (2° plus earth curvature); the shell there
        // covers that height and not the ground or 9 km.
        let i = ((100.0 + 120.0) / 240.0 * (n - 1) as f64).round() as usize;
        let j = (n - 1) / 2;
        let at = |z_km: f64| {
            v.data[i + n * j + n * n * (z_km / 10.0 * (nz - 1) as f64).round() as usize]
        };
        assert!(at(4.25) >= 2, "shell at the beam height");
        assert_eq!(at(0.0), 0, "nothing on the ground");
        assert_eq!(at(9.0), 0, "nothing above the beam");
    }
}
