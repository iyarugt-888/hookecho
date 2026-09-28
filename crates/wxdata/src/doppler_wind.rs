//! Wind from one radar's Doppler velocity, for the wind particles.
//!
//! A single radar measures only the part of the wind along its beam. The rest comes from the
//! same sweep's VAD (velocity-azimuth display): on each range ring the radial velocity traces a
//! sine with azimuth, and a least-squares fit of `vr = a0 + a·cos(az) + b·sin(az)` gives the mean
//! wind at that ring's height (`u = b / cos(e)`, `v = a / cos(e)`). Each gate then keeps its own
//! measured radial speed, and borrows only its crosswise component from its ring's VAD. So the
//! field matches the radar exactly along every beam, and is an average only across it.

use crate::level2::BinnedSweep;
use crate::mrms::MrmsField;

/// Gates pooled into one VAD ring: 4 x 250 m = 1 km, enough samples to fit and still following
/// the wind's change with height.
const RING_GATES: usize = 4;
/// Of 12 azimuth sectors, how many must hold data for a ring's fit to count: a ring seen only on
/// one side cannot tell a sine from an offset.
const MIN_SECTORS: usize = 8;

/// The VAD wind `(u, v)` (m/s, toward east and north) for each ring of [`RING_GATES`] gates of a
/// velocity sweep, `None` where the ring is too empty to fit.
pub fn vad(sweep: &BinnedSweep) -> Vec<Option<(f32, f32)>> {
    let (az_bins, gates) = (sweep.az_bins, sweep.gate_count);
    let span = sweep.value_max - sweep.value_min;
    let cos_e = (sweep.elevation_deg as f64).to_radians().cos().max(0.2);
    (0..gates.div_ceil(RING_GATES))
        .map(|ring| {
            // Normal equations of the three-term fit.
            let mut m = [[0f64; 3]; 3];
            let mut rhs = [0f64; 3];
            let mut sectors = [false; 12];
            for a in 0..az_bins {
                let az = (a as f64 + 0.5) / az_bins as f64 * std::f64::consts::TAU;
                let basis = [1.0, az.cos(), az.sin()];
                for g in ring * RING_GATES..((ring + 1) * RING_GATES).min(gates) {
                    let idx = sweep.data[a * gates + g];
                    if idx < 2 {
                        continue;
                    }
                    let vr = (sweep.value_min + (idx as f32 - 2.0) / 253.0 * span) as f64;
                    sectors[a * 12 / az_bins.max(1)] = true;
                    for i in 0..3 {
                        rhs[i] += basis[i] * vr;
                        for j in 0..3 {
                            m[i][j] += basis[i] * basis[j];
                        }
                    }
                }
            }
            if sectors.iter().filter(|s| **s).count() < MIN_SECTORS {
                return None;
            }
            let [_, a, b] = solve3(m, rhs)?;
            Some(((b / cos_e) as f32, (a / cos_e) as f32))
        })
        .collect()
}

/// Solve a 3x3 system by Cramer's rule; `None` when it is singular.
fn solve3(m: [[f64; 3]; 3], r: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(m);
    if d.abs() < 1e-9 {
        return None;
    }
    let mut out = [0f64; 3];
    for (c, o) in out.iter_mut().enumerate() {
        let mut mc = m;
        for (row, rv) in mc.iter_mut().zip(r) {
            row[c] = rv;
        }
        *o = det(mc) / d;
    }
    Some(out)
}

/// The wind field `(u, v)` on an `n × n` lon/lat grid reaching `half_km` from the radar, from a
/// (dealiased) velocity sweep: each cell's own radial speed, plus the crosswise component of its
/// ring's VAD (the nearest ring with one, within 10 km). NaN where the radar saw nothing.
pub fn wind_field(
    sweep: &BinnedSweep,
    half_km: f64,
    n: usize,
    time: chrono::DateTime<chrono::Utc>,
) -> Option<(MrmsField, MrmsField)> {
    let (az_bins, gates) = (sweep.az_bins, sweep.gate_count);
    if az_bins == 0 || gates == 0 || n < 2 {
        return None;
    }
    let rings = vad(sweep);
    if rings.iter().all(Option::is_none) {
        return None;
    }
    // The nearest ring with a VAD, per ring, so a ring too empty to fit borrows a neighbour's.
    let reach = (10.0 / (sweep.gate_interval_km as f64 * RING_GATES as f64)).ceil() as usize;
    let ring_wind = |ring: usize| -> Option<(f32, f32)> {
        (0..=reach).find_map(|d| {
            rings
                .get(ring.wrapping_sub(d))
                .copied()
                .flatten()
                .or_else(|| rings.get(ring + d).copied().flatten())
        })
    };
    let span = sweep.value_max - sweep.value_min;
    let cos_e = (sweep.elevation_deg as f64).to_radians().cos().max(0.2);
    let (lat0, lon0) = (sweep.radar_lat as f64, sweep.radar_lon as f64);
    let dlat = half_km / 111.32;
    let dlon = half_km / (111.32 * lat0.to_radians().cos().max(0.05));
    let mut u = vec![f32::NAN; n * n];
    let mut v = vec![f32::NAN; n * n];
    for j in 0..n {
        // Row 0 is north.
        let y = half_km - 2.0 * half_km * j as f64 / (n - 1) as f64;
        for i in 0..n {
            let x = -half_km + 2.0 * half_km * i as f64 / (n - 1) as f64;
            let ground = (x * x + y * y).sqrt();
            let slant = ground / cos_e;
            let g = ((slant - sweep.first_gate_km as f64)
                / sweep.gate_interval_km.max(0.01) as f64)
                .round();
            if g < 0.0 || g as usize >= gates {
                continue;
            }
            let az = x.atan2(y).rem_euclid(std::f64::consts::TAU);
            let a = ((az / std::f64::consts::TAU * az_bins as f64) as usize) % az_bins;
            let idx = sweep.data[a * gates + g as usize];
            if idx < 2 {
                continue;
            }
            let Some((u0, v0)) = ring_wind(g as usize / RING_GATES) else {
                continue;
            };
            let vr = (sweep.value_min + (idx as f32 - 2.0) / 253.0 * span) as f64 / cos_e;
            let (s, c) = az.sin_cos();
            // Crosswise unit vector (clockwise from the beam): (cos az, -sin az).
            let vt = u0 as f64 * c - v0 as f64 * s;
            u[j * n + i] = (vr * s + vt * c) as f32;
            v[j * n + i] = (vr * c - vt * s) as f32;
        }
    }
    let field = |values: Vec<f32>| MrmsField {
        values,
        nx: n,
        ny: n,
        lon_west: lon0 - dlon,
        lon_east: lon0 + dlon,
        lat_north: lat0 + dlat,
        lat_south: lat0 - dlat,
        time,
    };
    Some((field(u), field(v)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level2::Moment;

    /// A 0.5° sweep of a uniform wind `(u, v)`, with a gap to the south-west.
    fn uniform(u: f64, v: f64) -> BinnedSweep {
        let (lo, hi) = Moment::Velocity.value_range();
        let (az_bins, gates) = (360usize, 400usize);
        let e = 0.5f64.to_radians();
        let mut data = vec![0u8; az_bins * gates];
        for a in 0..az_bins {
            if (200..240).contains(&a) {
                continue;
            }
            let az = (a as f64 + 0.5).to_radians();
            let vr = (u * az.sin() + v * az.cos()) * e.cos();
            let idx = 2 + (((vr as f32 - lo) / (hi - lo)) * 253.0).round() as u8;
            for g in 0..gates {
                data[a * gates + g] = idx;
            }
        }
        BinnedSweep {
            moment: Moment::Velocity,
            az_bins,
            gate_count: gates,
            data,
            first_gate_km: 2.0,
            gate_interval_km: 0.25,
            radar_lat: 35.0,
            radar_lon: -97.0,
            elevation_deg: 0.5,
            value_min: lo,
            value_max: hi,
            ..Default::default()
        }
    }

    #[test]
    fn vad_recovers_a_uniform_wind() {
        let rings = vad(&uniform(12.0, -5.0));
        let (u, v) = rings[10].expect("a full ring fits");
        assert!((u - 12.0).abs() < 0.6 && (v + 5.0).abs() < 0.6, "{u} {v}");
    }

    #[test]
    fn the_field_matches_the_wind_everywhere_the_radar_saw() {
        let t = chrono::Utc::now();
        let (fu, fv) = wind_field(&uniform(12.0, -5.0), 80.0, 60, t).unwrap();
        let mut checked = 0;
        for (u, v) in fu.values.iter().zip(&fv.values) {
            if u.is_nan() {
                continue;
            }
            assert!((u - 12.0).abs() < 1.0 && (v + 5.0).abs() < 1.0, "{u} {v}");
            checked += 1;
        }
        assert!(checked > 1000, "{checked} cells");
        // The gap stays empty rather than being filled with the average.
        assert!(fu.values.iter().any(|u| u.is_nan()));
    }
}
