//! Rotation objects: connected regions of one sweep's LLSD azimuthal shear
//! ([`crate::azshear`]), each with the physical measurements later stages score
//! (detectionplan.md Phase 3).
//!
//! [`crate::rotation`] clusters gate-pair candidates on a fixed 0.04° grid, so one circulation can
//! split across cells and two can share one. Here an object is exactly the set of connected gates
//! whose shear clears a threshold, found on the sweep's own polar grid:
//!
//! * **Hysteresis.** A gate joins at [`ObjectParams::grow_s`]; an object exists only if some gate
//!   in it reaches [`ObjectParams::seed_s`] (0.006 s⁻¹, the threshold TORP builds its objects
//!   from). Weak isolated noise never seeds one, and a real circulation keeps its whole footprint.
//! * **Storm context.** At least [`ObjectParams::min_echo_share`] of a gate's kernel needs echo
//!   (≥ [`ObjectParams::z_min_dbz`]) at the same tilt. Clear air holds scattered gates past
//!   0.006 s⁻¹ (0.19% of the 2019 clear-air control). The kernel, not the gate: a tornado can
//!   centrifuge the echo out of its core, and a couplet sits at the edge of a hook (Mayfield's
//!   strongest lowest-tilt shear was over 6 dBZ). A share, not any one gate: near the radar a
//!   kernel spans dozens of gates, and a speck of ground clutter in clear air would otherwise
//!   qualify it. With [`ObjectParams::require_echo`] off, gates are not screened and an object
//!   whose gates' kernels are mostly echo-free is [`Artifact::NoEcho`] instead: how
//!   [`crate::rotation_columns`] finds a circulation's root in weak echo below a column that has
//!   storm context higher up.
//! * **Fit relative to the shear.** A gate's fit RMSE is compared with half the velocity change its
//!   shear makes across the kernel (plus [`ObjectParams::noise_floor_ms`] for quantization). Not
//!   an absolute limit: Moore and Mayfield have 11–14 m/s RMSE at their peaks, because a violent
//!   tornado is smaller and sharper than any plane, but that is well under what their shear
//!   implies. A gate more than twice over never joins an object; an object whose median gate is
//!   over is [`Artifact::PoorFit`].
//! * **Significance.** Far out the kernel is nine samples, and noisy velocity alone makes slopes of
//!   0.01–0.017 s⁻¹ there. The velocity noise around an object is the median texture of every gate
//!   in a fixed window about its peak (three kernel widths each side across the beam, 3 km along
//!   it: a few hundred gates), turned into the standard error a plane's slope has at that range;
//!   the peak shear must be [`ObjectParams::min_significance`] of those
//!   ([`Artifact::NotSignificant`]). The window, not the object: a small object's own few gates
//!   are where the texture happened to be low (that is partly why its shear cleared the threshold),
//!   and a tornado's own gates carry its gradients in their texture.
//! * **Filled, not scattered.** A circulation fills its footprint; velocity noise past the growth
//!   threshold percolates into sprawling, ragged regions (230 × 60 km, under 3% filled, in ±12 m/s
//!   noise). An object filling less than [`ObjectParams::min_fill`] of its length × width is
//!   [`Artifact::Ragged`].
//! * **Flanks.** Outside a vortex's core the wind falls off, so its radial velocity falls back
//!   across azimuth on both flanks: shear of the other sense without rotation of its own (AzShear
//!   is not vorticity). An object beside a much stronger one of the other sense is that flank
//!   ([`Artifact::Flank`]).
//! * **Each sense separately.** Cyclonic and anticyclonic shear never join one object.
//!
//! Every object is kept, but [`RotationObject::artifacts`] names any shape that is not a compact
//! circulation: too few gates, a single radial, a long thin line (a radial seam, a gust front, an
//! outflow boundary), a fold dealiasing left behind, a poor fit overall. [`RotationObject::credible`]
//! is the absence of all of them. Keeping the rejected ones lets an analyst see why a signature is
//! not counted (Phase 12). The thresholds are provisional, for the backtest to tune.

use crate::azshear::{radials_each_side, AzShearField};
use crate::level2::BinnedSweep;
use crate::rotation::{decode, Sense};

/// Version of the object rules, recorded with anything derived from them.
pub const ALGORITHM_VERSION: &str = "llsd-objects-1";

/// How objects are found and screened.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectParams {
    /// |shear| (s⁻¹) some gate of an object must reach.
    pub seed_s: f32,
    /// |shear| (s⁻¹) at which a gate joins an object it touches.
    pub grow_s: f32,
    /// Reflectivity (dBZ) that counts as echo.
    pub z_min_dbz: f32,
    /// Screen gates by echo (the default). Off, objects form in weak echo too and are flagged
    /// [`Artifact::NoEcho`].
    pub require_echo: bool,
    /// Least share of a gate's kernel that must be echo.
    pub min_echo_share: f32,
    /// Noise (m/s) every gate is allowed, however weak its shear: about two quantization steps.
    pub noise_floor_ms: f32,
    /// Fewest gates a credible object has.
    pub min_gates: usize,
    /// Least area of a credible object, as a share of the kernel's footprint (azimuthal × radial
    /// width): LLSD spreads any real shear over at least about the kernel, so a much smaller
    /// object is a speck that only just cleared the seed.
    pub min_kernel_share: f32,
    /// A credible object is no more than this many times longer than it is wide...
    pub max_aspect: f32,
    /// ...once it is at least this long (km). A short blob can be thin and still be a circulation.
    pub elongated_min_km: f32,
    /// Largest median, over the object's gates, of fit RMSE relative to what the shear implies.
    pub max_relative_noise: f32,
    /// Least peak shear, in standard errors of a slope at the object's noise and range.
    pub min_significance: f32,
    /// Least share of its length × width an object covers.
    pub min_fill: f32,
    /// Leftover-fold pairs per kernel row, on average over the object, that make it a fold seam.
    /// A seam crosses every row once (1.0) and still 0.66 where it ends; the Moore tornado's
    /// lowest-tilt object has 0.31.
    pub min_fold_crossings: f32,
    /// An object is a flank when one of the other sense at least this many times stronger...
    pub flank_ratio: f32,
    /// ...is within the two objects' radii plus this (km) of it.
    pub flank_gap_km: f32,
}

impl Default for ObjectParams {
    fn default() -> Self {
        ObjectParams {
            seed_s: 0.006,
            grow_s: 0.004,
            z_min_dbz: 20.0,
            require_echo: true,
            min_echo_share: 0.25,
            noise_floor_ms: 1.0,
            min_gates: 4,
            min_kernel_share: 0.5,
            max_aspect: 4.0,
            elongated_min_km: 6.0,
            max_relative_noise: 1.0,
            min_significance: 6.0,
            min_fill: 0.3,
            min_fold_crossings: 0.5,
            flank_ratio: 1.5,
            flank_gap_km: 2.5,
        }
    }
}

/// Why an object is not a compact circulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Artifact {
    /// Fewer than [`ObjectParams::min_gates`] gates, or smaller than
    /// [`ObjectParams::min_kernel_share`] of the kernel's footprint.
    TooSmall,
    /// One radial wide: LLSD spreads real rotation over the kernel, so this is one bad radial.
    SingleRadial,
    /// A long thin line: a radial seam, a gust front, an outflow boundary.
    Elongated,
    /// The two-Nyquist step of a fold dealiasing left behind: every velocity within the Nyquist
    /// limit, a spread of 1.6–2.2 Nyquists, and a leftover-fold pair in most rows of the kernel
    /// ([`ObjectParams::min_fold_crossings`]). The velocities alone are not enough: a strong
    /// couplet can sit right at the limit (the Moore tornado's lowest-tilt object does), but it
    /// turns through intermediate velocities, so only some of its neighbour pairs look like folds.
    FoldSeam,
    /// The planes fit the velocities poorly for the shear they claim, across the object.
    PoorFit,
    /// Its peak shear is within what the object's own velocity noise could make.
    NotSignificant,
    /// Scattered through its footprint rather than filling it: noise percolating, not a circulation.
    Ragged,
    /// The outer flank of a stronger circulation of the other sense beside it.
    Flank,
    /// Its gates' kernels average less than [`ObjectParams::min_echo_share`] echo: shear with no
    /// storm behind it at this tilt. Only with [`ObjectParams::require_echo`] off; otherwise such
    /// gates never join an object.
    NoEcho,
}

impl Artifact {
    pub fn label(self) -> &'static str {
        match self {
            Artifact::TooSmall => "too few gates",
            Artifact::SingleRadial => "a single radial",
            Artifact::Elongated => "a long thin line, not a compact circulation",
            Artifact::FoldSeam => "a dealiasing fold",
            Artifact::PoorFit => "velocities fit poorly for the shear",
            Artifact::NotSignificant => "within what velocity noise alone makes",
            Artifact::Ragged => "scattered, not one filled circulation",
            Artifact::Flank => "the flank of a stronger circulation beside it",
            Artifact::NoEcho => "no storm echo around it at this tilt",
        }
    }
}

/// One connected region of same-sense azimuthal shear on one sweep.
#[derive(Debug, Clone, PartialEq)]
pub struct RotationObject {
    /// Shear-weighted centre.
    pub lon: f64,
    pub lat: f64,
    pub area_km2: f32,
    /// Diameter of a circle of the same area.
    pub diameter_km: f32,
    /// Length and width (km) of the object's footprint, from its spatial spread.
    pub length_km: f32,
    pub width_km: f32,
    /// Shear magnitude, sense-adjusted (always positive), s⁻¹.
    pub max_azshear: f32,
    pub p90_azshear: f32,
    pub median_azshear: f32,
    /// Spread of radial velocity over the object and the kernel around it: p95 − p5, and max − min.
    pub robust_delta_v_ms: f32,
    pub max_delta_v_ms: f32,
    /// Means over the object's gates.
    pub mean_texture_ms: f32,
    pub fit_rmse_ms: f32,
    pub valid_fraction: f32,
    pub fold_share: f32,
    /// Leftover-fold neighbour pairs per kernel row, averaged over the object: about 1 along a
    /// fold seam, which crosses every row once.
    pub fold_crossings: f32,
    /// Peak shear in standard errors of a slope at the velocity noise around it and its range.
    pub significance: f32,
    pub range_km: f32,
    /// Beam-centre height at the object's range, km above radar level.
    pub beam_height_km: f32,
    pub elevation_deg: f32,
    pub sense: Sense,
    pub gates: usize,
    /// Distinct radials the object spans.
    pub radials: usize,
    /// What makes it not a compact circulation; empty if nothing does.
    pub artifacts: Vec<Artifact>,
}

impl RotationObject {
    /// No artifact found.
    pub fn credible(&self) -> bool {
        self.artifacts.is_empty()
    }
}

/// A gate's fit RMSE over what its shear implies: half the velocity change the shear makes
/// across the kernel, plus the floor.
fn relative_noise(s: &crate::azshear::GateShear, kernel_km: f32, p: &ObjectParams) -> f32 {
    s.quality.fit_rmse_ms / (p.noise_floor_ms + 0.5 * s.shear_s.abs() * kernel_km * 1000.0)
}

/// Standard error (s⁻¹) of a kernel's slope at `range_km` when each velocity carries independent
/// noise of `sigma_ms`: `σ / √Σs²`, over the kernel's offsets across the beam.
fn slope_stderr(sigma_ms: f32, range_km: f32, f: &AzShearField) -> f32 {
    let side = radials_each_side(range_km, f.az_bins, &f.params) as f64;
    let rows = (2 * crate::azshear::gates_each_side(f.gate_interval_km, &f.params) + 1) as f64;
    let ds = range_km as f64 * std::f64::consts::TAU / f.az_bins as f64;
    // Σ over rows and k = -side..=side of (k·ds)².
    let sum_s2 = rows * ds * ds * side * (side + 1.0) * (2.0 * side + 1.0) / 3.0;
    (sigma_ms as f64 / sum_s2.sqrt() / 1000.0) as f32
}

/// Reflectivity (dBZ) at a velocity gate, read from the same tilt's reflectivity sweep by azimuth
/// and range, whatever its gate spacing.
fn dbz_at(z: &BinnedSweep, f: &AzShearField, az: usize, gate: usize) -> Option<f32> {
    if z.az_bins == 0 || z.gate_count == 0 || z.gate_interval_km <= 0.0 {
        return None;
    }
    let za = (az * z.az_bins / f.az_bins.max(1)).min(z.az_bins - 1);
    let zi = ((f.range_km(gate) - z.first_gate_km) / z.gate_interval_km).round();
    if zi < 0.0 || zi as usize >= z.gate_count {
        return None;
    }
    decode(z, z.data[za * z.gate_count + zi as usize])
}

/// The share of a gate's kernel (the radials and gates its shear was fitted over) with echo of at
/// least `z_min` at the same tilt.
fn echo_share(z: &BinnedSweep, f: &AzShearField, az: usize, gate: usize, z_min: f32) -> f32 {
    let side = radials_each_side(f.range_km(gate), f.az_bins, &f.params) as i64;
    let rows = crate::azshear::gates_each_side(f.gate_interval_km, &f.params) as i64;
    let (mut echo, mut all) = (0usize, 0usize);
    for k in -side..=side {
        let a = (az as i64 + k).rem_euclid(f.az_bins as i64) as usize;
        for j in -rows..=rows {
            let g = gate as i64 + j;
            if g < 0 || g as usize >= f.gate_count {
                continue;
            }
            all += 1;
            echo += usize::from(dbz_at(z, f, a, g as usize).is_some_and(|d| d >= z_min));
        }
    }
    echo as f32 / all.max(1) as f32
}

/// p-th quantile (0..1) of a sorted slice.
fn quantile(sorted: &[f32], q: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f32 * q).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

/// Every rotation object on one sweep: `field` from [`crate::azshear::llsd`] over `vel`, and `z`
/// the same tilt's reflectivity. Ordered strongest first (by max shear, then position), so the
/// result does not depend on where the scan happened to start.
pub fn objects(
    field: &AzShearField,
    vel: &BinnedSweep,
    z: &BinnedSweep,
    p: &ObjectParams,
) -> Vec<RotationObject> {
    let (n_az, ng) = (field.az_bins, field.gate_count);
    if n_az == 0 || ng == 0 {
        return Vec::new();
    }
    let kernel_km = field.params.azimuthal_km;
    // Signed shear where a gate may join an object (echo behind it, fit not grossly poor), else 0.
    let usable: Vec<f32> = (0..n_az * ng)
        .map(|i| {
            let (az, g) = (i / ng, i % ng);
            match field.at(az, g) {
                Some(s)
                    if relative_noise(&s, kernel_km, p) <= 2.0 * p.max_relative_noise
                        && s.shear_s.abs() >= p.grow_s
                        && (!p.require_echo
                            || echo_share(z, field, az, g, p.z_min_dbz) >= p.min_echo_share) =>
                {
                    s.shear_s
                }
                _ => 0.0,
            }
        })
        .collect();
    let dtheta = std::f64::consts::TAU / n_az as f64;
    let mut out = Vec::new();
    let mut seen = vec![false; n_az * ng];
    let mut stack = Vec::new();
    for polarity in [1.0f32, -1.0] {
        seen.iter_mut().for_each(|s| *s = false);
        for start in 0..n_az * ng {
            if seen[start] || usable[start] * polarity < p.grow_s {
                continue;
            }
            // Flood fill, 8-connected, azimuth wrapping through north.
            let mut members = Vec::new();
            seen[start] = true;
            stack.push(start);
            while let Some(i) = stack.pop() {
                members.push(i);
                let (az, g) = ((i / ng) as i64, (i % ng) as i64);
                for da in -1..=1i64 {
                    for dg in -1..=1i64 {
                        let g2 = g + dg;
                        if (da == 0 && dg == 0) || g2 < 0 || g2 >= ng as i64 {
                            continue;
                        }
                        let a2 = (az + da).rem_euclid(n_az as i64) as usize;
                        let j = a2 * ng + g2 as usize;
                        if !seen[j] && usable[j] * polarity >= p.grow_s {
                            seen[j] = true;
                            stack.push(j);
                        }
                    }
                }
            }
            if !members.iter().any(|&i| usable[i] * polarity >= p.seed_s) {
                continue;
            }
            members.sort_unstable();
            out.push(describe(field, vel, z, &members, polarity, dtheta, p));
        }
    }
    mark_flanks(&mut out, p);
    out.sort_by(|a, b| {
        b.max_azshear
            .total_cmp(&a.max_azshear)
            .then(a.lon.total_cmp(&b.lon))
            .then(a.lat.total_cmp(&b.lat))
    });
    out
}

/// Mark each object that sits beside a much stronger one of the other sense as its flank.
fn mark_flanks(objs: &mut [RotationObject], p: &ObjectParams) {
    let flanks: Vec<bool> = objs
        .iter()
        .map(|o| {
            objs.iter().any(|big| {
                big.sense != o.sense
                    && big.max_azshear >= p.flank_ratio * o.max_azshear
                    && crate::tds::ground_km((o.lon, o.lat), (big.lon, big.lat))
                        <= ((o.diameter_km + big.diameter_km) / 2.0 + p.flank_gap_km) as f64
            })
        })
        .collect();
    for (o, flank) in objs.iter_mut().zip(flanks) {
        if flank {
            o.artifacts.push(Artifact::Flank);
        }
    }
}

/// One object's measurements and artifacts from its gates (sorted indices into the field).
fn describe(
    field: &AzShearField,
    vel: &BinnedSweep,
    z: &BinnedSweep,
    members: &[usize],
    polarity: f32,
    dtheta: f64,
    p: &ObjectParams,
) -> RotationObject {
    let (n_az, ng) = (field.az_bins, field.gate_count);
    let gi = field.gate_interval_km as f64;
    let kernel_km = field.params.azimuthal_km;
    let mut shears = Vec::with_capacity(members.len());
    let (mut sw, mut sx, mut sy, mut sr, mut area) = (0.0f64, 0.0, 0.0, 0.0, 0.0);
    let mut pts = Vec::with_capacity(members.len());
    let (mut tex, mut rmse, mut valid, mut fold) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    // Leftover-fold pairs per kernel row: the per-pair share times the 2·side pairs in a row at
    // this gate's range (the kernel's width in radials changes along a long object).
    let mut crossings = 0.0f32;
    let mut misfit = Vec::with_capacity(members.len());
    let mut radials = std::collections::BTreeSet::new();
    let mut vs: Vec<f32> = Vec::new();
    let mut vel_seen = std::collections::BTreeSet::new();
    for &i in members {
        let (az, g) = (i / ng, i % ng);
        let s = field.at(az, g).expect("object gates have shear");
        let mag = s.shear_s * polarity;
        shears.push(mag);
        let r = field.range_km(g) as f64;
        let th = field.azimuth_deg(az).to_radians();
        let (x, y) = (r * th.sin(), r * th.cos());
        let w = mag as f64;
        sw += w;
        sx += w * x;
        sy += w * y;
        sr += w * r;
        area += r * dtheta * gi;
        pts.push((x, y));
        tex += s.quality.velocity_texture_ms;
        rmse += s.quality.fit_rmse_ms;
        valid += s.quality.valid_fraction;
        fold += s.quality.dealias_suspect_fraction;
        crossings += s.quality.dealias_suspect_fraction
            * 2.0
            * radials_each_side(r as f32, n_az, &field.params) as f32;
        misfit.push(relative_noise(&s, kernel_km, p));
        radials.insert(az);
        // The velocities the shear was measured from: this gate and the kernel's radials around it.
        let side = radials_each_side(r as f32, n_az, &field.params) as i64;
        for k in -side..=side {
            let a2 = (az as i64 + k).rem_euclid(n_az as i64) as usize;
            if a2 < vel.az_bins && g < vel.gate_count && vel_seen.insert((a2, g)) {
                if let Some(v) = decode(vel, vel.data[a2 * vel.gate_count + g]) {
                    vs.push(v);
                }
            }
        }
    }
    let n = members.len() as f32;
    shears.sort_by(f32::total_cmp);
    vs.sort_by(f32::total_cmp);
    misfit.sort_by(f32::total_cmp);
    let (cx, cy) = (sx / sw, sy / sw);
    let range_km = (sr / sw) as f32;
    let (lon, lat) = crate::rotation::dest(
        field.radar_lon as f64,
        field.radar_lat as f64,
        cx.atan2(cy).to_degrees().rem_euclid(360.0),
        cx.hypot(cy),
    );
    // Footprint length and width from the spatial covariance: a uniform bar of length L has
    // variance L²/12. The width never reads below one gate.
    let (mx, my) = (
        pts.iter().map(|p| p.0).sum::<f64>() / pts.len() as f64,
        pts.iter().map(|p| p.1).sum::<f64>() / pts.len() as f64,
    );
    let (mut cxx, mut cyy, mut cxy) = (0.0, 0.0, 0.0);
    for (x, y) in &pts {
        cxx += (x - mx).powi(2);
        cyy += (y - my).powi(2);
        cxy += (x - mx) * (y - my);
    }
    let m = pts.len() as f64;
    let (cxx, cyy, cxy) = (cxx / m, cyy / m, cxy / m);
    let half_tr = 0.5 * (cxx + cyy);
    let det = (0.25 * (cxx - cyy).powi(2) + cxy * cxy).sqrt();
    let length_km = ((12.0 * (half_tr + det)).sqrt() as f32).max(gi as f32);
    let width_km = ((12.0 * (half_tr - det).max(0.0)).sqrt() as f32).max(gi as f32);
    let max_azshear = *shears.last().unwrap_or(&0.0);
    let fold_share = fold / n;

    let mut artifacts = Vec::new();
    let kernel_area = field.params.azimuthal_km * field.params.radial_km;
    if members.len() < p.min_gates || (area as f32) < p.min_kernel_share * kernel_area {
        artifacts.push(Artifact::TooSmall);
    }
    if radials.len() == 1 {
        artifacts.push(Artifact::SingleRadial);
    }
    if (area as f32) < p.min_fill * length_km * width_km {
        artifacts.push(Artifact::Ragged);
    }
    if length_km >= p.elongated_min_km && length_km >= p.max_aspect * width_km {
        artifacts.push(Artifact::Elongated);
    }
    let nyq = vel.nyquist_ms;
    let spread = vs.last().copied().unwrap_or(0.0) - vs.first().copied().unwrap_or(0.0);
    let peak_v = vs.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let fold_crossings = crossings / n;
    if nyq > 5.0
        && fold_crossings >= p.min_fold_crossings
        && peak_v <= 1.1 * nyq
        && (1.6 * nyq..=2.2 * nyq).contains(&spread)
    {
        artifacts.push(Artifact::FoldSeam);
    }
    if quantile(&misfit, 0.5) > p.max_relative_noise {
        artifacts.push(Artifact::PoorFit);
    }
    if !p.require_echo {
        let share = members
            .iter()
            .map(|&i| echo_share(z, field, i / ng, i % ng, p.z_min_dbz))
            .sum::<f32>()
            / n;
        if share < p.min_echo_share {
            artifacts.push(Artifact::NoEcho);
        }
    }
    // Velocity noise from a fixed window about the peak: for white noise the RMS of gate-to-gate
    // differences is √2 σ. The peak's range sets the kernel the slope was fitted over.
    let peak = members
        .iter()
        .copied()
        .max_by(|&a, &b| {
            let s = |i: usize| {
                field
                    .at(i / ng, i % ng)
                    .map_or(0.0, |s| s.shear_s * polarity)
            };
            s(a).total_cmp(&s(b)).then(b.cmp(&a))
        })
        .unwrap_or(members[0]);
    let (peak_az, peak_g) = (peak / ng, peak % ng);
    let peak_range = field.range_km(peak_g);
    let across = 3 * radials_each_side(peak_range, n_az, &field.params) as i64;
    let along = (3.0 / gi).round() as i64;
    let mut window: Vec<f32> = Vec::new();
    for da in -across..=across {
        let a = (peak_az as i64 + da).rem_euclid(n_az as i64) as usize;
        for dg in -along..=along {
            let g = peak_g as i64 + dg;
            if g < 0 || g >= ng as i64 {
                continue;
            }
            if let Some(s) = field.at(a, g as usize) {
                window.push(s.quality.velocity_texture_ms);
            }
        }
    }
    window.sort_by(f32::total_cmp);
    let sigma = (quantile(&window, 0.5) / std::f32::consts::SQRT_2).max(p.noise_floor_ms);
    let significance = max_azshear / slope_stderr(sigma, peak_range, field);
    if significance < p.min_significance {
        artifacts.push(Artifact::NotSignificant);
    }
    RotationObject {
        lon,
        lat,
        area_km2: area as f32,
        diameter_km: 2.0 * (area as f32 / std::f32::consts::PI).sqrt(),
        length_km,
        width_km,
        max_azshear,
        p90_azshear: quantile(&shears, 0.9),
        median_azshear: quantile(&shears, 0.5),
        robust_delta_v_ms: quantile(&vs, 0.95) - quantile(&vs, 0.05),
        max_delta_v_ms: spread,
        mean_texture_ms: tex / n,
        fit_rmse_ms: rmse / n,
        valid_fraction: valid / n,
        fold_share,
        fold_crossings,
        significance,
        range_km,
        beam_height_km: crate::xsection::beam_height_km(range_km as f64, field.elevation_deg as f64)
            as f32,
        elevation_deg: field.elevation_deg,
        // Positive polarity is velocity rising with azimuth; `Sense::of` allows for the hemisphere.
        sense: Sense::of(polarity as f64, field.radar_lat),
        gates: members.len(),
        radials: radials.len(),
        artifacts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::azshear::{llsd, LlsdParams};
    use crate::level2::Moment;

    const AZ: usize = 720;
    const GATE_KM: f32 = 0.25;
    const FIRST_KM: f32 = 2.125;
    const GATES: usize = 920;

    fn binned(
        moment: Moment,
        lo: f32,
        hi: f32,
        f: impl Fn(f64, f64) -> Option<f32>,
    ) -> BinnedSweep {
        let mut data = vec![0u8; AZ * GATES];
        for az in 0..AZ {
            let deg = (az as f64 + 0.5) * 360.0 / AZ as f64;
            for g in 0..GATES {
                let r = (FIRST_KM + g as f32 * GATE_KM) as f64;
                if let Some(v) = f(deg, r) {
                    let idx = 2.0 + ((v.clamp(lo, hi) - lo) / (hi - lo) * 253.0).round();
                    data[az * GATES + g] = idx as u8;
                }
            }
        }
        BinnedSweep {
            moment,
            az_bins: AZ,
            gate_count: GATES,
            data,
            first_gate_km: FIRST_KM,
            gate_interval_km: GATE_KM,
            radar_lat: 35.33,
            radar_lon: -97.28,
            elevation_deg: 0.5,
            value_min: lo,
            value_max: hi,
            ..Default::default()
        }
    }

    fn vel(f: impl Fn(f64, f64) -> Option<f32>) -> BinnedSweep {
        binned(Moment::Velocity, -64.0, 64.0, f)
    }

    /// Echo everywhere, or nowhere.
    fn echo(dbz: f32) -> BinnedSweep {
        binned(Moment::Reflectivity, -32.0, 94.5, move |_, _| Some(dbz))
    }

    /// Radial velocity of a Rankine vortex (as in `azshear`'s tests), plus a uniform `u`, `v` flow.
    fn rankine(az0: f64, r0: f64, rc: f64, vmax: f64) -> impl Fn(f64, f64) -> f64 {
        move |deg, r| {
            let (t, t0) = (deg.to_radians(), az0.to_radians());
            let (x, y) = (r * t.sin(), r * t.cos());
            let (dx, dy) = (x - r0 * t0.sin(), y - r0 * t0.cos());
            let rho = dx.hypot(dy);
            let vt = if rho < rc {
                vmax * rho / rc
            } else {
                vmax * rc / rho
            };
            if rho < 1e-9 {
                return 0.0;
            }
            (-vt * dy / rho * x + vt * dx / rho * y) / r
        }
    }

    fn run(v: &BinnedSweep, z: &BinnedSweep) -> Vec<RotationObject> {
        objects(
            &llsd(v, &LlsdParams::default()),
            v,
            z,
            &ObjectParams::default(),
        )
    }

    /// Uniform white noise in ±`amp` m/s, independent from gate to gate (splitmix64 of the gate).
    fn white(d: f64, r: f64, amp: f32) -> f32 {
        let a = (d / 360.0 * AZ as f64).floor() as u64;
        let g = ((r as f32 - FIRST_KM) / GATE_KM).round() as u64;
        let mut z = (a * 1_000_003 + g).wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        let u = (z >> 11) as f64 / (1u64 << 53) as f64;
        ((u * 2.0 - 1.0) * amp as f64) as f32
    }

    fn km(a: (f64, f64), b: (f64, f64)) -> f64 {
        crate::tds::ground_km(a, b)
    }

    fn at(deg: f64, r: f64) -> (f64, f64) {
        crate::rotation::dest(-97.28, 35.33, deg, r)
    }

    #[test]
    fn a_vortex_is_one_credible_cyclonic_object_where_it_is() {
        let vortex = rankine(200.0, 50.0, 1.5, 35.0);
        let objs = run(&vel(|d, r| Some(vortex(d, r) as f32)), &echo(45.0));
        let credible: Vec<_> = objs.iter().filter(|o| o.credible()).collect();
        assert_eq!(credible.len(), 1, "{objs:#?}");
        let o = credible[0];
        assert_eq!(o.sense, Sense::Cyclonic);
        assert!(km((o.lon, o.lat), at(200.0, 50.0)) < 1.0, "{o:#?}");
        assert!(o.max_azshear > 0.015, "{}", o.max_azshear);
        assert!(o.max_azshear >= o.p90_azshear && o.p90_azshear >= o.median_azshear);
        // Both sides of the couplet are in the spread: about twice the peak wind.
        assert!(
            o.max_delta_v_ms > 55.0 && o.robust_delta_v_ms > 40.0,
            "{o:#?}"
        );
        assert!(
            (0.3..0.8).contains(&o.beam_height_km),
            "{}",
            o.beam_height_km
        );
        assert!(o.length_km < 4.0 * o.width_km);
    }

    #[test]
    fn clockwise_rotation_is_anticyclonic_and_separate() {
        // A cyclonic and an anticyclonic vortex 15 km apart: two objects, one of each sense.
        let (a, b) = (
            rankine(200.0, 50.0, 1.5, 35.0),
            rankine(217.0, 50.0, 1.5, -35.0),
        );
        let objs = run(&vel(|d, r| Some((a(d, r) + b(d, r)) as f32)), &echo(45.0));
        let credible: Vec<_> = objs.iter().filter(|o| o.credible()).collect();
        assert_eq!(credible.len(), 2, "{objs:#?}");
        assert!(credible.iter().any(|o| o.sense == Sense::Cyclonic));
        assert!(credible.iter().any(|o| o.sense == Sense::Anticyclonic));
    }

    #[test]
    fn rotation_in_clear_air_makes_no_object() {
        let vortex = rankine(200.0, 50.0, 1.5, 35.0);
        let v = vel(|d, r| Some(vortex(d, r) as f32));
        assert!(run(&v, &echo(5.0)).is_empty());
        // Without the echo screen it is found, and flagged as having no storm behind it.
        let relaxed = ObjectParams {
            require_echo: false,
            ..ObjectParams::default()
        };
        let objs = objects(&llsd(&v, &LlsdParams::default()), &v, &echo(5.0), &relaxed);
        assert!(!objs.is_empty());
        assert!(objs.iter().all(|o| o.artifacts.contains(&Artifact::NoEcho)));
        let main = &objs[0];
        assert_eq!(main.artifacts, vec![Artifact::NoEcho], "{main:#?}");
    }

    #[test]
    fn one_wild_gate_makes_no_object() {
        let (a0, g0) = (180usize, 200usize);
        let v = vel(move |d, r| {
            let a = (d / 360.0 * AZ as f64).floor() as usize;
            let g = ((r as f32 - FIRST_KM) / GATE_KM).round() as usize;
            Some(if (a, g) == (a0, g0) { 45.0 } else { 0.0 })
        });
        assert!(run(&v, &echo(45.0)).is_empty());
    }

    #[test]
    fn a_long_shear_line_is_not_a_compact_circulation() {
        // A gust front: northward wind flips from +15 to -15 m/s across a north-south line 20 km
        // east of the radar, over 1 km, for 100 km. Strong shear, and nothing like a vortex.
        let v = vel(|d, r| {
            let t = d.to_radians();
            let (x, y) = (r * t.sin(), r * t.cos());
            // Faded in and out over ~5 km at each end, so the ends are not shear lines of their own.
            let fade = 0.5 * (((y - 20.0) / 5.0).tanh() - ((y - 120.0) / 5.0).tanh());
            let north = 15.0 * ((x - 20.0) / 0.5).tanh() * fade;
            Some((north * y / r) as f32)
        });
        let objs = run(&v, &echo(35.0));
        // The line itself is found and is not a circulation. Where it fades out at its ends the
        // shear can leave a small blob at the seed threshold; that is all that may remain.
        let line = objs.first().expect("the line should be found");
        assert!(line.artifacts.contains(&Artifact::Elongated), "{line:#?}");
        assert!(line.length_km > 50.0);
        for o in objs.iter().filter(|o| o.credible()) {
            assert!(o.max_azshear < 0.0075, "{o:#?}");
        }
    }

    #[test]
    fn a_fold_dealiasing_left_behind_is_not_rotation() {
        // +24 / -24 m/s either side of the 90° radial from 30 to 60 km, on a 26 m/s Nyquist.
        let mut v = vel(|d, r| {
            Some(if (30.0..60.0).contains(&r) {
                if d < 90.0 {
                    24.0
                } else {
                    -24.0
                }
            } else {
                0.0
            })
        });
        v.nyquist_ms = 26.0;
        let objs = run(&v, &echo(35.0));
        assert!(!objs.is_empty());
        assert!(objs.iter().all(|o| !o.credible()), "{objs:#?}");
        assert!(objs
            .iter()
            .any(|o| o.artifacts.contains(&Artifact::FoldSeam)));
    }

    #[test]
    fn a_vortex_across_north_is_one_object() {
        let vortex = rankine(0.0, 40.0, 1.5, 35.0);
        let objs = run(&vel(|d, r| Some(vortex(d, r) as f32)), &echo(45.0));
        let cyclonic: Vec<_> = objs
            .iter()
            .filter(|o| o.credible() && o.sense == Sense::Cyclonic)
            .collect();
        assert_eq!(cyclonic.len(), 1, "{objs:#?}");
        assert!(km((cyclonic[0].lon, cyclonic[0].lat), at(0.0, 40.0)) < 1.0);
    }

    #[test]
    fn same_sign_rotation_in_strong_flow_is_still_an_object() {
        let vortex = rankine(200.0, 50.0, 1.5, 25.0);
        let objs = run(&vel(|d, r| Some(vortex(d, r) as f32 + 25.0)), &echo(45.0));
        assert_eq!(objs.iter().filter(|o| o.credible()).count(), 1, "{objs:#?}");
    }

    #[test]
    fn noisy_velocity_is_too_noisy_for_its_shear() {
        // Pseudo-random ±12 m/s noise over a weak vortex: shear clears the threshold here and
        // there, but the planes do not fit what they claim.
        let vortex = rankine(200.0, 50.0, 3.0, 12.0);
        let objs = run(
            &vel(|d, r| Some(vortex(d, r) as f32 + white(d, r, 12.0))),
            &echo(40.0),
        );
        let credible = objs.iter().filter(|o| o.credible()).count();
        eprintln!("noise: {credible} credible of {} objects", objs.len());
        assert_eq!(
            credible,
            0,
            "{:#?}",
            objs.iter().filter(|o| o.credible()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_weak_vortex_in_ordinary_storm_noise_is_still_found() {
        // 20 m/s winds in a 3 km core at 90 km (shear 0.013 s⁻¹, about TORP's weak end), in
        // ±3 m/s velocity noise: what good echo actually has.
        let vortex = rankine(200.0, 90.0, 1.5, 20.0);
        let objs = run(
            &vel(|d, r| Some(vortex(d, r) as f32 + white(d, r, 3.0))),
            &echo(40.0),
        );
        let credible: Vec<_> = objs.iter().filter(|o| o.credible()).collect();
        assert_eq!(credible.len(), 1, "{credible:#?}");
        assert!(km((credible[0].lon, credible[0].lat), at(200.0, 90.0)) < 1.5);
    }

    #[test]
    fn the_flanks_of_a_vortex_belong_to_it() {
        let vortex = rankine(200.0, 50.0, 1.5, 35.0);
        let objs = run(&vel(|d, r| Some(vortex(d, r) as f32)), &echo(45.0));
        let flanks: Vec<_> = objs
            .iter()
            .filter(|o| o.artifacts.contains(&Artifact::Flank))
            .collect();
        assert!(!flanks.is_empty(), "{objs:#?}");
        assert!(flanks.iter().all(|o| o.sense == Sense::Anticyclonic));
    }

    #[test]
    fn objects_are_the_same_every_run() {
        let vortex = rankine(200.0, 50.0, 1.5, 35.0);
        let v = vel(|d, r| Some(vortex(d, r) as f32));
        assert_eq!(run(&v, &echo(45.0)), run(&v, &echo(45.0)));
    }
}
