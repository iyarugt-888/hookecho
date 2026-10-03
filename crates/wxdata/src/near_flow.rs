//! How fast the air moves near the ground around a circulation (detectionplan.md, round three).
//!
//! A tornado in a line (QLCS) typically forms where a surge of the rear-inflow jet meets the
//! line's leading edge: the "three ingredients" view of QLCS tornadogenesis (line-normal low-level
//! shear, a rear-inflow surge, a mesovortex). That predicts strong near-ground flow right beside
//! the vortex. A hail storm's mesocyclone, whose rotation can measure just as strong, need not
//! have it. [`near_flow`] measures it on the lowest velocity tilt: every valid gate within a
//! radius of the circulation, as the radar sees it (radial velocity, dealiased by the caller).
//!
//! Radial velocity is only the component along the beam, so a flow across the beam reads weak.
//! These numbers are therefore lower bounds on the wind, and the backtest, not this module,
//! decides whether they carry evidence.

use crate::level2::BinnedSweep;

/// How far from a circulation to measure, km.
pub const RADIUS_KM: f32 = 5.0;

/// The near-ground flow around one position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NearFlow {
    /// 90th percentile of |radial velocity|, m/s.
    pub p90_speed_ms: f32,
    /// Strongest flow toward the radar (as a positive speed), m/s; 0 with none.
    pub max_inbound_ms: f32,
    /// Strongest flow away from the radar, m/s; 0 with none.
    pub max_outbound_ms: f32,
    /// Valid gates measured.
    pub gates: usize,
}

/// The flow on `vel` within `radius_km` of (`lon`, `lat`); `None` when the position is off the
/// sweep or fewer than 9 valid gates are that near.
pub fn near_flow(vel: &BinnedSweep, lon: f64, lat: f64, radius_km: f32) -> Option<NearFlow> {
    let at = vel.sample_at(lon, lat)?;
    let (na, ng) = (vel.az_bins, vel.gate_count);
    if na == 0 || ng == 0 || vel.data.len() != na * ng {
        return None;
    }
    let bin = ((at.azimuth_deg as f64 / 360.0 * na as f64) as usize) % na;
    let arc_km = (at.range_km.max(1.0) * std::f32::consts::TAU / na as f32).max(1e-3);
    let dg = (radius_km / vel.gate_interval_km.max(1e-3)).ceil() as isize;
    let da = ((radius_km / arc_km).ceil() as isize).min(na as isize / 2);
    let span = vel.value_max - vel.value_min;
    let mut speeds = Vec::new();
    let (mut inbound, mut outbound) = (0.0f32, 0.0f32);
    for oa in -da..=da {
        let a = (bin as isize + oa).rem_euclid(na as isize) as usize;
        for og in -dg..=dg {
            let g = at.gate as isize + og;
            if g < 0 || g >= ng as isize {
                continue;
            }
            if (oa as f32 * arc_km).hypot(og as f32 * vel.gate_interval_km) > radius_km {
                continue;
            }
            // 0 is no data and 1 range-folded: neither is a velocity.
            let code = vel.data[a * ng + g as usize];
            if code < 2 {
                continue;
            }
            let v = vel.value_min + (code - 2) as f32 / 253.0 * span;
            speeds.push(v.abs());
            if v < 0.0 {
                inbound = inbound.max(-v);
            } else {
                outbound = outbound.max(v);
            }
        }
    }
    if speeds.len() < 9 {
        return None;
    }
    speeds.sort_by(f32::total_cmp);
    let p90 = speeds[((speeds.len() - 1) as f32 * 0.9).round() as usize];
    Some(NearFlow {
        p90_speed_ms: p90,
        max_inbound_ms: inbound,
        max_outbound_ms: outbound,
        gates: speeds.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sweep(f: impl Fn(f64, f64) -> Option<f32>) -> BinnedSweep {
        let (na, ng, gate) = (720usize, 600usize, 0.25f32);
        let mut data = vec![0u8; na * ng];
        for a in 0..na {
            let th = (a as f64 + 0.5) * std::f64::consts::TAU / na as f64;
            for g in 0..ng {
                let r = 2.125 + g as f64 * gate as f64;
                if let Some(v) = f(r * th.sin(), r * th.cos()) {
                    data[a * ng + g] = (2.0 + ((v + 64.0) / 128.0 * 253.0).round()) as u8;
                }
            }
        }
        BinnedSweep {
            moment: crate::level2::Moment::Velocity,
            az_bins: na,
            gate_count: ng,
            data,
            first_gate_km: 2.125,
            gate_interval_km: gate,
            radar_lat: 35.0,
            radar_lon: -97.0,
            elevation_deg: 0.5,
            value_min: -64.0,
            value_max: 64.0,
            ..Default::default()
        }
    }

    fn lonlat(x: f64, y: f64) -> (f64, f64) {
        (
            -97.0 + x / (111.32 * 35f64.to_radians().cos()),
            35.0 + y / 110.57,
        )
    }

    #[test]
    fn a_surge_beside_the_vortex_reads_strong_and_calm_air_weak() {
        // A 35 m/s inbound surge in a 4 km strip just west of (60, 0) km; 5 m/s elsewhere.
        let s = sweep(|x, y| {
            Some(if (x - 57.0).abs() <= 2.0 && y.abs() <= 6.0 {
                -35.0
            } else {
                5.0
            })
        });
        let (lon, lat) = lonlat(60.0, 0.0);
        let f = near_flow(&s, lon, lat, RADIUS_KM).unwrap();
        assert!((f.max_inbound_ms - 35.0).abs() < 0.5, "{f:?}");
        assert!((f.max_outbound_ms - 5.0).abs() < 0.5, "{f:?}");
        assert!(
            f.p90_speed_ms > 30.0,
            "a fifth of the disk is in the surge: {f:?}"
        );
        let (lon, lat) = lonlat(-60.0, 0.0);
        let calm = near_flow(&s, lon, lat, RADIUS_KM).unwrap();
        assert!((calm.p90_speed_ms - 5.0).abs() < 0.5 && calm.max_inbound_ms == 0.0);
        // No data near the position: nothing measured.
        let empty = sweep(|_, _| None);
        assert_eq!(near_flow(&empty, lon, lat, RADIUS_KM), None);
    }
}
