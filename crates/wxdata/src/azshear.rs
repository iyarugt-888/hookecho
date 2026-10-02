//! Azimuthal shear from one velocity sweep by local linear least squares (LLSD), with the quality
//! of every estimate kept beside it (detectionplan.md Phase 2).
//!
//! [`crate::rotation`] finds couplets from the velocity difference between two adjacent gates. One
//! bad gate, one bad radial or one fold that dealiasing missed is enough to make that difference
//! large. Here every gate instead gets the slope of a plane fitted to the radial velocities around
//! it — `V ≈ c + a·s + b·r`, with `s` the distance across the beam (arc length) and `r` the distance
//! along it — and the azimuthal shear is `a = ∂V/∂s`. That is the LLSD technique of Smith and
//! Elmore (2004), documented in full by Mahalik et al. (2019, *Wea. Forecasting* 34, 415–434), and
//! the basis of MRMS AzShear.
//!
//! * **The kernel is physical, not a count of bins.** It spans [`LlsdParams::azimuthal_km`] across
//!   the beam (2.5 km) and [`LlsdParams::radial_km`] along it (750 m), the sizes in the literature
//!   above, so how many radials it takes grows as the beam narrows toward the radar. Far out a
//!   single radial each side is already wider than 2.5 km; the kernel never takes fewer, so there
//!   it is as wide as the beam spacing makes it, and the shear of a small circulation is
//!   underestimated (for a 5 km vortex, within ~20% out to ~140 km).
//! * **The fit is robust.** Samples far from the plane (by a multiple of the residuals' median
//!   absolute deviation) lose their weight under Tukey's biweight and the plane is refitted, so
//!   one wild gate or one bad radial in the kernel does not move the slope ([`LlsdParams`]).
//! * **Every estimate carries its quality** ([`ShearQuality`]): how much of the kernel had data,
//!   how far the samples sit from the plane, how noisy the velocity is along each radial, and what
//!   share of neighbouring samples look like a fold dealiasing left behind. Nothing is thrown away
//!   for poor quality here: deciding what is too poor is the rotation objects' job (Phase 3).
//!
//! Units are s⁻¹. Positive shear is radial velocity rising clockwise (with azimuth), which is
//! counterclockwise rotation: cyclonic north of the equator ([`AzShearField::cyclonic_sign`]). For
//! solid-body rotation the shear is `Vrot / R`, half the vorticity. TORP builds its rotation objects
//! from AzShear of at least 0.006 s⁻¹.
//!
//! Each radial's row is computed independently and collected in order, so the parallel computation
//! gives the same field every run.

use crate::level2::{BinnedSweep, Moment};
use crate::rotation::{decode, is_leftover_fold, scanned_together};
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

/// Version of the field computation, recorded with anything derived from it.
pub const ALGORITHM_VERSION: &str = "llsd-azshear-1";

/// How the field is computed. [`Default`] is the literature's kernel and conservative QC.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LlsdParams {
    /// Kernel width across the beam, km (Smith and Elmore 2004: 2.5 km).
    pub azimuthal_km: f32,
    /// Kernel depth along the beam, km (750 m).
    pub radial_km: f32,
    /// Gates nearer than this are left empty: clutter, and too narrow a beam to fill the kernel.
    pub min_range_km: f32,
    /// Gates beyond this are left empty (and not stored).
    pub max_range_km: f32,
    /// At most this many radials each side. Within ~11 km of the radar a 2.5 km kernel would take
    /// dozens of 0.5° radials; the cap keeps the cost bounded and leaves the kernel narrower there.
    pub max_radials_each_side: usize,
    /// Fewest valid samples a fit may use.
    pub min_samples: usize,
    /// Least share of the kernel that must hold valid velocity.
    pub min_valid_fraction: f32,
    /// Tukey biweight cutoff, in units of the residuals' robust spread (4.685: 95% efficient on
    /// clean Gaussian data). Samples beyond it get no weight.
    pub biweight_c: f32,
    /// Biweighted refits, starting from a flat plane at the median velocity (0: ordinary least
    /// squares instead).
    pub robust_iterations: usize,
    /// Floor on the residuals' robust spread, m/s: velocity is quantized at about 0.5 m/s, so a
    /// near-perfect fit must not make every quantization step an outlier.
    pub min_sigma_ms: f32,
}

impl Default for LlsdParams {
    fn default() -> Self {
        LlsdParams {
            azimuthal_km: 2.5,
            radial_km: 0.75,
            min_range_km: 5.0,
            max_range_km: 230.0,
            max_radials_each_side: 12,
            min_samples: 6,
            min_valid_fraction: 0.5,
            biweight_c: 4.685,
            robust_iterations: 4,
            min_sigma_ms: 1.0,
        }
    }
}

/// How far one shear estimate can be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ShearQuality {
    /// Valid velocity samples the fit used.
    pub samples: u16,
    /// Share of the kernel (radials scanned with this one, gates in range) that held velocity.
    pub valid_fraction: f32,
    /// RMS of gate-to-gate velocity differences along each radial in the kernel, m/s: noise.
    pub velocity_texture_ms: f32,
    /// RMS distance of the samples from the fitted plane, m/s, outliers included.
    pub fit_rmse_ms: f32,
    /// Share of azimuthally neighbouring sample pairs that look like a fold dealiasing left
    /// behind (both near the Nyquist velocity, about two Nyquists apart).
    pub dealias_suspect_fraction: f32,
}

/// One gate's shear and its quality.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GateShear {
    /// ∂V/∂s, s⁻¹. Positive: velocity rising clockwise.
    pub shear_s: f32,
    pub quality: ShearQuality,
}

/// The shear field of one sweep, on the sweep's own polar grid (up to the maximum range). Not
/// `PartialEq`: gates without an estimate hold NaN.
#[derive(Debug, Clone)]
pub struct AzShearField {
    pub az_bins: usize,
    /// Gates stored per radial: the sweep's, up to [`LlsdParams::max_range_km`].
    pub gate_count: usize,
    pub first_gate_km: f32,
    pub gate_interval_km: f32,
    pub radar_lat: f32,
    pub radar_lon: f32,
    pub elevation_deg: f32,
    pub params: LlsdParams,
    // Struct of arrays, azimuth-major, ~16 bytes a gate: a whole sweep is ~10 MB, not ~30.
    shear: Vec<f32>,
    fit_rmse: Vec<f32>,
    texture: Vec<f32>,
    samples: Vec<u16>,
    /// Share × 255.
    valid: Vec<u8>,
    /// Share × 255.
    fold: Vec<u8>,
}

impl AzShearField {
    /// The shear at a gate, or `None` where it was not computed (no velocity, too little of the
    /// kernel filled, the kernel only on one side, or out of range).
    pub fn at(&self, az: usize, gate: usize) -> Option<GateShear> {
        if az >= self.az_bins || gate >= self.gate_count {
            return None;
        }
        let i = az * self.gate_count + gate;
        let shear_s = self.shear[i];
        if !shear_s.is_finite() {
            return None;
        }
        Some(GateShear {
            shear_s,
            quality: ShearQuality {
                samples: self.samples[i],
                valid_fraction: self.valid[i] as f32 / 255.0,
                velocity_texture_ms: self.texture[i],
                fit_rmse_ms: self.fit_rmse[i],
                dealias_suspect_fraction: self.fold[i] as f32 / 255.0,
            },
        })
    }

    /// `+1` where positive shear is cyclonic (north of the equator), `-1` where it is anticyclonic.
    pub fn cyclonic_sign(&self) -> f32 {
        if self.radar_lat >= 0.0 {
            1.0
        } else {
            -1.0
        }
    }

    /// Range of a gate's centre, km.
    pub fn range_km(&self, gate: usize) -> f32 {
        self.first_gate_km + gate as f32 * self.gate_interval_km
    }

    /// Azimuth of a bin's centre, degrees clockwise from north.
    pub fn azimuth_deg(&self, az: usize) -> f64 {
        (az as f64 + 0.5) * 360.0 / self.az_bins as f64
    }

    /// Where a gate is on the ground (lon, lat).
    pub fn lonlat(&self, az: usize, gate: usize) -> (f64, f64) {
        crate::rotation::dest(
            self.radar_lon as f64,
            self.radar_lat as f64,
            self.azimuth_deg(az),
            self.range_km(gate) as f64,
        )
    }

    /// Every computed gate, azimuth-major: `(az, gate, shear)`.
    pub fn iter(&self) -> impl Iterator<Item = (usize, usize, GateShear)> + '_ {
        (0..self.az_bins).flat_map(move |az| {
            (0..self.gate_count).filter_map(move |g| self.at(az, g).map(|s| (az, g, s)))
        })
    }
}

/// Radials each side of the centre for a kernel `azimuthal_km` wide at `range_km`, on a sweep of
/// `az_bins` radials: the count whose span is nearest the width, at least one, at most the cap.
pub fn radials_each_side(range_km: f32, az_bins: usize, p: &LlsdParams) -> usize {
    let spacing_km = range_km * std::f32::consts::TAU / az_bins as f32;
    if spacing_km.is_nan() || spacing_km <= 0.0 {
        return p.max_radials_each_side.max(1);
    }
    let across = (p.azimuthal_km / spacing_km).round() as usize;
    (across.saturating_sub(1) / 2).clamp(1, p.max_radials_each_side.max(1))
}

/// Gates each side of the centre for a kernel `radial_km` deep, at least one.
pub fn gates_each_side(gate_interval_km: f32, p: &LlsdParams) -> usize {
    if gate_interval_km.is_nan() || gate_interval_km <= 0.0 {
        return 1;
    }
    let along = (p.radial_km / gate_interval_km).round() as usize;
    (along.saturating_sub(1) / 2).max(1)
}

/// One sample in a kernel: offsets across (`s`) and along (`r`) the beam, km, and velocity.
#[derive(Clone, Copy)]
struct Sample {
    s: f64,
    r: f64,
    v: f64,
}

/// Weighted least-squares plane `v = c + a·s + b·r`; `None` if the samples do not span both
/// directions.
fn fit_plane(xs: &[Sample], w: &[f64]) -> Option<(f64, f64, f64)> {
    let (mut sw, mut ss, mut sr, mut sss, mut srr, mut ssr) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let (mut sv, mut ssv, mut srv) = (0.0, 0.0, 0.0);
    for (x, &w) in xs.iter().zip(w) {
        sw += w;
        ss += w * x.s;
        sr += w * x.r;
        sss += w * x.s * x.s;
        srr += w * x.r * x.r;
        ssr += w * x.s * x.r;
        sv += w * x.v;
        ssv += w * x.s * x.v;
        srv += w * x.r * x.v;
    }
    // Normal equations [sw ss sr; ss sss ssr; sr ssr srr] [c a b]' = [sv ssv srv]', by Cramer.
    let det3 = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let m = [[sw, ss, sr], [ss, sss, ssr], [sr, ssr, srr]];
    let d = det3(m);
    // Relative to the scale of the matrix, so tiny kernels near the radar are not rejected.
    let scale = sw * sss.max(1e-12) * srr.max(1e-12);
    if d.is_nan() || d.abs() <= 1e-9 * scale {
        return None;
    }
    let rhs = [sv, ssv, srv];
    let col = |k: usize| {
        let mut mk = m;
        for (row, v) in mk.iter_mut().zip(rhs) {
            row[k] = v;
        }
        det3(mk) / d
    };
    Some((col(0), col(1), col(2)))
}

/// The median of `x`, reordering it.
fn median(x: &mut [f64]) -> f64 {
    let n = x.len();
    let mid = n / 2;
    x.select_nth_unstable_by(mid, f64::total_cmp);
    let hi = x[mid];
    if n % 2 == 1 {
        hi
    } else {
        let lo = x[..mid].iter().copied().fold(f64::NEG_INFINITY, f64::max);
        0.5 * (lo + hi)
    }
}

/// Buffers reused from gate to gate, so the gate loop does not allocate.
#[derive(Default)]
struct Scratch {
    weights: Vec<f64>,
    residuals: Vec<f64>,
    sorted: Vec<f64>,
}

/// Tukey's biweight: full weight near the plane, falling smoothly to none at `c`.
fn biweight(r: f64, c: f64) -> f64 {
    let u = r / c;
    if u.abs() < 1.0 {
        (1.0 - u * u).powi(2)
    } else {
        0.0
    }
}

/// The residuals' robust spread (1.4826 × MAD), floored at `min_sigma_ms`.
fn spread(xs: &[Sample], fit: (f64, f64, f64), p: &LlsdParams, b: &mut Scratch) -> f64 {
    let (c, a, rs) = fit;
    b.residuals.clear();
    b.residuals
        .extend(xs.iter().map(|x| x.v - (c + a * x.s + rs * x.r)));
    b.sorted.clear();
    b.sorted.extend_from_slice(&b.residuals);
    let med = median(&mut b.sorted);
    b.sorted.clear();
    b.sorted.extend(b.residuals.iter().map(|r| (r - med).abs()));
    (1.4826 * median(&mut b.sorted)).max(p.min_sigma_ms as f64)
}

/// Robust plane fit: Tukey-biweighted least squares (none: ordinary least squares).
///
/// Huber weights are not enough here. With three radials in the kernel (the kernel's width past
/// ~90 km), one bad radial is a third of the samples and sits at the kernel's edge, where it has
/// the most leverage: each refit tilts the plane toward it, the good samples' residuals grow, the
/// spread grows with them, and the bad samples win their weight back until the fit is ordinary
/// least squares again (half the bad radial's step, read as shear). Biweights fall to zero, so a
/// sample far from the plane drops out instead of pulling with bounded force.
///
/// The fit starts from a flat plane at the median velocity, so a bad radial is an outlier from the
/// first step instead of being half fitted. The spread is taken from that start, held while
/// `robust_iterations / 2` refits find the gradient, then taken once more from the residuals about
/// the fitted plane and held for the rest: never re-estimated from a fit it could have
/// contaminated. A real couplet is not thrown out: with two or more radials of it in the kernel the
/// spread is large and every sample keeps its weight. A genuine gradient loses nothing either: any
/// positive weights return the exact plane of data that lie on one.
fn robust_fit(xs: &[Sample], p: &LlsdParams, b: &mut Scratch) -> Option<(f64, f64, f64)> {
    b.weights.clear();
    b.weights.resize(xs.len(), 1.0);
    if p.robust_iterations == 0 {
        return fit_plane(xs, &b.weights);
    }
    b.sorted.clear();
    b.sorted.extend(xs.iter().map(|x| x.v));
    let mut fit = (median(&mut b.sorted), 0.0, 0.0);
    let mut sigma = spread(xs, fit, p, b);
    for i in 0..p.robust_iterations {
        if i > 0 && i == p.robust_iterations / 2 {
            sigma = spread(xs, fit, p, b);
        }
        let (c, a, rs) = fit;
        let cut = p.biweight_c as f64 * sigma;
        for (wi, x) in b.weights.iter_mut().zip(xs) {
            *wi = biweight(x.v - (c + a * x.s + rs * x.r), cut);
        }
        fit = fit_plane(xs, &b.weights)?;
    }
    Some(fit)
}

/// One gate's computed values, in [`AzShearField`]'s column order: shear, fit RMSE, texture,
/// samples, valid share and fold share (both × 255).
type GateOut = (f32, f32, f32, u16, u8, u8);

/// The LLSD azimuthal shear of a dealiased velocity sweep. Feed it the dealiased sweep: a fold
/// makes a two-Nyquist step that no robust fit can tell from shear (it is flagged in
/// [`ShearQuality::dealias_suspect_fraction`], not removed).
pub fn llsd(vel: &BinnedSweep, p: &LlsdParams) -> AzShearField {
    debug_assert_eq!(vel.moment, Moment::Velocity);
    let n_az = vel.az_bins;
    let gi = vel.gate_interval_km;
    let stored = if n_az == 0 || vel.gate_count == 0 || gi.is_nan() || gi <= 0.0 {
        0
    } else {
        let last = ((p.max_range_km - vel.first_gate_km) / gi).floor();
        if last < 0.0 {
            0
        } else {
            (last as usize + 1).min(vel.gate_count)
        }
    };
    let ng = vel.gate_count;
    // Decoded once: NaN where there is no velocity.
    let v: Vec<f32> = vel
        .data
        .iter()
        .map(|&b| decode(vel, b).unwrap_or(f32::NAN))
        .collect();
    let dtheta = std::f64::consts::TAU / n_az.max(1) as f64;
    let g_side = gates_each_side(gi, p) as i64;
    let nyq = vel.nyquist_ms;

    // Rayon is a native-only dependency. Preserve the identical row calculation and order
    // in browser builds, which execute it sequentially without a thread pool.
    #[cfg(not(target_arch = "wasm32"))]
    let azimuths = (0..n_az).into_par_iter();
    #[cfg(target_arch = "wasm32")]
    let azimuths = 0..n_az;
    let rows: Vec<Vec<GateOut>> = azimuths
        .map(|az| {
            let empty = (f32::NAN, 0.0, 0.0, 0u16, 0u8, 0u8);
            let mut row = vec![empty; stored];
            // Which radials around this one were scanned with it, by offset.
            let max_side = p.max_radials_each_side.max(1) as i64;
            let together: Vec<bool> = (-max_side..=max_side)
                .map(|k| {
                    scanned_together(vel, az, (az as i64 + k).rem_euclid(n_az as i64) as usize)
                })
                .collect();
            let mut xs: Vec<Sample> = Vec::new();
            let mut scratch = Scratch::default();
            for (g, out) in row.iter_mut().enumerate() {
                let range = vel.first_gate_km + g as f32 * gi;
                if range < p.min_range_km || !v[az * ng + g].is_finite() {
                    continue;
                }
                let a_side = radials_each_side(range, n_az, p) as i64;
                xs.clear();
                let (mut total, mut left, mut right) = (0usize, false, false);
                let (mut tex_sum, mut tex_n) = (0.0f64, 0usize);
                let (mut fold_pairs, mut adjacent_pairs) = (0usize, 0usize);
                for k in -a_side..=a_side {
                    let a2 = (az as i64 + k).rem_euclid(n_az as i64) as usize;
                    let with_us = together[(k + max_side) as usize];
                    let mut prev: Option<f32> = None;
                    for j in -g_side..=g_side {
                        total += 1;
                        let g2 = g as i64 + j;
                        if !with_us || g2 < 0 || g2 as usize >= ng {
                            prev = None;
                            continue;
                        }
                        let val = v[a2 * ng + g2 as usize];
                        if !val.is_finite() {
                            prev = None;
                            continue;
                        }
                        if let Some(pv) = prev {
                            tex_sum += ((val - pv) as f64).powi(2);
                            tex_n += 1;
                        }
                        prev = Some(val);
                        // The same gate on the next radial clockwise, for the fold check.
                        if k < a_side && together[(k + 1 + max_side) as usize] {
                            let a3 = (az as i64 + k + 1).rem_euclid(n_az as i64) as usize;
                            let nv = v[a3 * ng + g2 as usize];
                            if nv.is_finite() {
                                adjacent_pairs += 1;
                                if is_leftover_fold(val, nv, nyq) {
                                    fold_pairs += 1;
                                }
                            }
                        }
                        let r2 = (vel.first_gate_km + g2 as f32 * gi) as f64;
                        left |= k < 0;
                        right |= k > 0;
                        xs.push(Sample {
                            s: r2 * k as f64 * dtheta,
                            r: r2 - range as f64,
                            v: val as f64,
                        });
                    }
                }
                let valid_fraction = xs.len() as f32 / total.max(1) as f32;
                if xs.len() < p.min_samples
                    || valid_fraction < p.min_valid_fraction
                    || !(left && right)
                {
                    continue;
                }
                let Some((c, a, b)) = robust_fit(&xs, p, &mut scratch) else {
                    continue;
                };
                let rmse = (xs
                    .iter()
                    .map(|x| (x.v - (c + a * x.s + b * x.r)).powi(2))
                    .sum::<f64>()
                    / xs.len() as f64)
                    .sqrt();
                let texture = if tex_n > 0 {
                    (tex_sum / tex_n as f64).sqrt()
                } else {
                    0.0
                };
                let fold = if adjacent_pairs > 0 {
                    fold_pairs as f32 / adjacent_pairs as f32
                } else {
                    0.0
                };
                // m/s per km to s⁻¹.
                *out = (
                    (a / 1000.0) as f32,
                    rmse as f32,
                    texture as f32,
                    xs.len().min(u16::MAX as usize) as u16,
                    (valid_fraction.clamp(0.0, 1.0) * 255.0).round() as u8,
                    (fold.clamp(0.0, 1.0) * 255.0).round() as u8,
                );
            }
            row
        })
        .collect();

    let n = n_az * stored;
    let mut f = AzShearField {
        az_bins: n_az,
        gate_count: stored,
        first_gate_km: vel.first_gate_km,
        gate_interval_km: gi,
        radar_lat: vel.radar_lat,
        radar_lon: vel.radar_lon,
        elevation_deg: vel.elevation_deg,
        params: *p,
        shear: Vec::with_capacity(n),
        fit_rmse: Vec::with_capacity(n),
        texture: Vec::with_capacity(n),
        samples: Vec::with_capacity(n),
        valid: Vec::with_capacity(n),
        fold: Vec::with_capacity(n),
    };
    for row in rows {
        for (s, e, t, c, vf, fd) in row {
            f.shear.push(s);
            f.fit_rmse.push(e);
            f.texture.push(t);
            f.samples.push(c);
            f.valid.push(vf);
            f.fold.push(fd);
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    const AZ: usize = 720;
    const GATE_KM: f32 = 0.25;
    const FIRST_KM: f32 = 2.125;
    const GATES: usize = 920;
    const LO: f32 = -64.0;
    const HI: f32 = 64.0;

    /// A velocity sweep from `f(azimuth_deg, range_km)`, quantized the way Level 2 bins are.
    fn sweep(f: impl Fn(f64, f64) -> Option<f32>) -> BinnedSweep {
        let mut data = vec![0u8; AZ * GATES];
        for az in 0..AZ {
            let deg = (az as f64 + 0.5) * 360.0 / AZ as f64;
            for g in 0..GATES {
                let r = (FIRST_KM + g as f32 * GATE_KM) as f64;
                if let Some(v) = f(deg, r) {
                    let idx = 2.0 + ((v.clamp(LO, HI) - LO) / (HI - LO) * 253.0).round();
                    data[az * GATES + g] = idx as u8;
                }
            }
        }
        BinnedSweep {
            moment: Moment::Velocity,
            az_bins: AZ,
            gate_count: GATES,
            data,
            first_gate_km: FIRST_KM,
            gate_interval_km: GATE_KM,
            radar_lat: 35.33,
            radar_lon: -97.28,
            elevation_deg: 0.5,
            value_min: LO,
            value_max: HI,
            ..Default::default()
        }
    }

    /// Radial velocity of a Rankine vortex centred at (`az0`°, `r0` km), core radius `rc` km and
    /// peak tangential speed `vmax` m/s; positive `vmax` turns counterclockwise.
    fn rankine(az0: f64, r0: f64, rc: f64, vmax: f64) -> impl Fn(f64, f64) -> Option<f32> {
        move |deg, r| {
            let (t, t0) = (deg.to_radians(), az0.to_radians());
            let (x, y) = (r * t.sin(), r * t.cos());
            let (xc, yc) = (r0 * t0.sin(), r0 * t0.cos());
            let (dx, dy) = (x - xc, y - yc);
            let rho = dx.hypot(dy);
            let vt = if rho < rc {
                vmax * rho / rc
            } else {
                vmax * rc / rho
            };
            let (u, v) = if rho > 1e-9 {
                (-vt * dy / rho, vt * dx / rho)
            } else {
                (0.0, 0.0)
            };
            Some(((u * x + v * y) / r) as f32)
        }
    }

    fn bin_of(deg: f64) -> usize {
        (deg / 360.0 * AZ as f64).floor() as usize % AZ
    }

    fn gate_of(r: f64) -> usize {
        ((r as f32 - FIRST_KM) / GATE_KM).round() as usize
    }

    /// The strongest |shear| within `half` bins and gates of a point.
    fn peak_near(f: &AzShearField, deg: f64, r: f64, half: i64) -> f32 {
        let (a0, g0) = (bin_of(deg) as i64, gate_of(r) as i64);
        let mut best = 0.0f32;
        for da in -half..=half {
            for dg in -half..=half {
                let a = (a0 + da).rem_euclid(AZ as i64) as usize;
                if let Some(s) = f.at(a, (g0 + dg) as usize) {
                    if s.shear_s.abs() > best.abs() {
                        best = s.shear_s;
                    }
                }
            }
        }
        best
    }

    #[test]
    fn a_rankine_vortex_reads_its_rotation_rate_and_sense() {
        // A 5 km vortex with 40 m/s winds: solid-body shear inside the core is 40 / 2.5 km.
        let truth = 40.0 / 2500.0;
        for r0 in [40.0, 100.0] {
            let f = llsd(
                &sweep(rankine(200.0, r0, 2.5, 40.0)),
                &LlsdParams::default(),
            );
            let s = f
                .at(bin_of(200.0), gate_of(r0))
                .expect("shear at the centre");
            assert!(
                (s.shear_s - truth).abs() < 0.2 * truth,
                "{r0} km: {} vs {truth}",
                s.shear_s
            );
            assert!(s.quality.fit_rmse_ms < 2.0, "{:?}", s.quality);
            assert_eq!(s.quality.dealias_suspect_fraction, 0.0);
        }
        // Clockwise is negative, and of the same size.
        let f = llsd(
            &sweep(rankine(200.0, 40.0, 2.5, -40.0)),
            &LlsdParams::default(),
        );
        let s = f.at(bin_of(200.0), gate_of(40.0)).unwrap();
        assert!((s.shear_s + truth).abs() < 0.2 * truth, "{}", s.shear_s);
        assert_eq!(f.cyclonic_sign(), 1.0);
    }

    #[test]
    fn one_wild_gate_does_not_make_shear() {
        // Calm air with one gate reading 40 m/s: a 40 m/s gate-to-gate "couplet" to the old
        // detector, nothing here.
        let (deg, r) = (90.0, 50.0);
        let (a0, g0) = (bin_of(deg), gate_of(r));
        let f = llsd(
            &sweep(move |d, rr| {
                Some(if bin_of(d) == a0 && gate_of(rr) == g0 {
                    40.0
                } else {
                    0.0
                })
            }),
            &LlsdParams::default(),
        );
        let peak = peak_near(&f, deg, r, 4);
        assert!(peak.abs() < 0.001, "{peak}");
        // The wild gate shows in the fit, though.
        assert!(f.at(a0, g0).unwrap().quality.fit_rmse_ms > 5.0);
    }

    #[test]
    fn one_bad_radial_does_not_make_shear() {
        let deg = 90.0;
        let a0 = bin_of(deg);
        let f = llsd(
            &sweep(move |d, _| Some(if bin_of(d) == a0 { 30.0 } else { 0.0 })),
            &LlsdParams::default(),
        );
        // Including past ~90 km, where the kernel is three radials and the bad one a third of it:
        // ordinary least squares reads half its step as 0.014 s⁻¹ there, over twice the 0.006 s⁻¹
        // TORP builds rotation objects from.
        for r in [30.0, 60.0, 120.0, 180.0] {
            let peak = peak_near(&f, deg, r, 3);
            assert!(peak.abs() < 0.001, "{r} km: {peak}");
        }
    }

    #[test]
    fn same_sign_rotation_in_strong_flow_reads_the_same() {
        // The whole vortex moving away at 25 m/s: every gate outbound, no inbound side at all,
        // which the old opposite-sign rule could not see. The plane's offset takes the flow.
        let p = LlsdParams::default();
        let still = llsd(&sweep(rankine(200.0, 40.0, 2.5, 20.0)), &p);
        let vortex = rankine(200.0, 40.0, 2.5, 20.0);
        let moving = llsd(&sweep(move |d, r| vortex(d, r).map(|v| v + 25.0)), &p);
        let (a, g) = (bin_of(200.0), gate_of(40.0));
        let (s0, s1) = (
            still.at(a, g).unwrap().shear_s,
            moving.at(a, g).unwrap().shear_s,
        );
        assert!(s1 > 0.006, "{s1}");
        assert!((s1 - s0).abs() < 0.05 * s0, "{s0} vs {s1}");
    }

    #[test]
    fn a_fold_dealiasing_left_behind_is_flagged() {
        // +24 and -24 m/s either side of a line on a 26 m/s Nyquist: two Nyquists apart, both at
        // the limit. The fit sees a step it cannot tell from shear, so the quality has to say.
        let deg = 90.0;
        let a0 = bin_of(deg);
        let mut s = sweep(move |d, _| Some(if bin_of(d) <= a0 { 24.0 } else { -24.0 }));
        s.nyquist_ms = 26.0;
        let f = llsd(&s, &LlsdParams::default());
        let at_seam = f.at(a0, gate_of(60.0)).unwrap();
        assert!(
            at_seam.quality.dealias_suspect_fraction > 0.0,
            "{at_seam:?}"
        );
        // Away from the seam there is nothing to flag.
        let away = f.at(bin_of(120.0), gate_of(60.0)).unwrap();
        assert_eq!(away.quality.dealias_suspect_fraction, 0.0);
        assert!(away.shear_s.abs() < 1e-4);
    }

    #[test]
    fn the_kernel_is_physical_not_a_count_of_bins() {
        let p = LlsdParams::default();
        // 0.5° radials are ~0.17 km apart at 20 km and ~0.87 km at 100 km.
        assert_eq!(radials_each_side(20.0, AZ, &p), 6);
        assert_eq!(radials_each_side(100.0, AZ, &p), 1);
        // Never fewer than one each side, never more than the cap.
        assert_eq!(radials_each_side(220.0, AZ, &p), 1);
        assert_eq!(radials_each_side(3.0, AZ, &p), p.max_radials_each_side);
        assert_eq!(gates_each_side(0.25, &p), 1);
        assert_eq!(gates_each_side(1.0, &p), 1);
    }

    #[test]
    fn too_little_data_or_one_side_only_gives_nothing() {
        // Velocity only east of 90°: the gates on the edge have no kernel to their west.
        let f = llsd(
            &sweep(|d, _| (d >= 90.0).then_some(5.0)),
            &LlsdParams::default(),
        );
        assert!(f.at(bin_of(90.1), gate_of(50.0)).is_none());
        assert!(f.at(bin_of(100.0), gate_of(50.0)).is_some());
        // And none at all inside the minimum range or where there is no velocity.
        assert!(f.at(bin_of(100.0), gate_of(3.0)).is_none());
        assert!(f.at(bin_of(45.0), gate_of(50.0)).is_none());
    }

    #[test]
    fn radials_from_another_pass_are_not_neighbours() {
        // A partial live sweep: radials up to 90° are from the previous rotation, ten seconds
        // older. The step between passes must not read as shear on the boundary.
        let a0 = bin_of(90.0);
        let mut s = sweep(move |d, _| Some(if bin_of(d) <= a0 { 15.0 } else { -15.0 }));
        s.bin_time_ms = (0..AZ)
            .map(|a| {
                if a <= a0 {
                    1_000 + a as i64 * 14
                } else {
                    11_000 + a as i64 * 14
                }
            })
            .collect();
        let f = llsd(&s, &LlsdParams::default());
        // The boundary radials see only one side, so there is no estimate there at all.
        assert!(f.at(a0, gate_of(60.0)).is_none());
        assert!(f.at(a0 + 1, gate_of(60.0)).is_none());
        assert!(f.at(bin_of(120.0), gate_of(60.0)).is_some());
    }

    #[test]
    fn the_field_is_the_same_every_run() {
        let s = sweep(rankine(10.0, 70.0, 1.5, 35.0));
        let p = LlsdParams::default();
        let (a, b) = (llsd(&s, &p), llsd(&s, &p));
        let bits = |f: &AzShearField| f.shear.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&a), bits(&b));
        assert_eq!(
            (a.fit_rmse, a.texture, a.samples),
            (b.fit_rmse, b.texture, b.samples)
        );
    }

    #[test]
    fn a_sweep_with_no_radials_is_an_empty_field() {
        let f = llsd(
            &BinnedSweep {
                moment: Moment::Velocity,
                ..Default::default()
            },
            &LlsdParams::default(),
        );
        assert_eq!((f.az_bins, f.gate_count), (0, 0));
        assert_eq!(f.iter().count(), 0);
    }
}
