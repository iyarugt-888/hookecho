//! Does a TDWR see line tornadoes the WSR-88D cannot? (detectionplan.md round four: line tornadoes
//! at range are too small and too shallow for the WSR-88D's lowest tilt and the LLSD kernel.)
//!
//! For the Chicago derecho of 2020-08-10, the same LLSD field on the lowest velocity tilt of
//! O'Hare's TDWR (TORD) and of KLOT, at each tornado report: the strongest cyclonic shear within
//! 3 km. A measurement for the plan, not a regression: network only, run explicitly.

use wxdata::azshear::{llsd, LlsdParams};
use wxdata::level2::{self, Moment};

/// Reports from the IEM LSR archive: (minute after 20:00Z, lat, lon, place).
const REPORTS: [(i64, f64, f64, &str); 6] = [
    (15, 41.60, -88.31, "5 W Plainfield"),
    (35, 41.87, -88.10, "1 ENE Wheaton"),
    (39, 41.89, -88.00, "1 NNE Lombard"),
    (54, 41.62, -87.73, "1 SW Midlothian"),
    (57, 41.48, -87.69, "Park Forest"),
    (59, 42.01, -87.70, "West Ridge, Chicago"),
];

fn km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dy = (b.1 - a.1) * 110.57;
    let dx = (b.0 - a.0) * 111.32 * ((a.1 + b.1) * 0.5).to_radians().cos();
    dx.hypot(dy)
}

/// What the sweep holds within `r_km`: gates with a velocity, range-folded, and empty.
fn coverage_near(vel: &level2::BinnedSweep, lon: f64, lat: f64, r_km: f64) -> String {
    let (mut valid, mut folded, mut empty) = (0, 0, 0);
    for az in 0..vel.az_bins {
        let th = (az as f64 + 0.5) * 360.0 / vel.az_bins as f64;
        for g in 0..vel.gate_count {
            let r = (vel.first_gate_km + g as f32 * vel.gate_interval_km) as f64;
            // Flat-earth position of the gate: close enough within a few km of a report.
            let (rlon, rlat) = (vel.radar_lon as f64, vel.radar_lat as f64);
            let p = (
                rlon + r * th.to_radians().sin() / (111.32 * rlat.to_radians().cos()),
                rlat + r * th.to_radians().cos() / 110.57,
            );
            if km(p, (lon, lat)) > r_km {
                continue;
            }
            match vel.data[az * vel.gate_count + g] {
                0 => empty += 1,
                1 => folded += 1,
                _ => valid += 1,
            }
        }
    }
    format!(
        "{} radials x {} gates of {:.3} km, elev {:.2}, valid {valid} folded {folded} empty {empty}",
        vel.az_bins, vel.gate_count, vel.gate_interval_km, vel.elevation_deg
    )
}

/// Strongest cyclonic shear (s⁻¹) within `r_km` of (lon, lat), and the gate count searched.
fn peak_near(vel: &level2::BinnedSweep, lon: f64, lat: f64, r_km: f64) -> (f32, usize) {
    let field = llsd(vel, &LlsdParams::default());
    let mut best = 0.0f32;
    let mut n = 0;
    for (az, g, s) in field.iter() {
        if km(field.lonlat(az, g), (lon, lat)) <= r_km {
            n += 1;
            best = best.max(s.shear_s);
        }
    }
    (best, n)
}

#[tokio::test]
#[ignore = "network: TDWR and Level II archives"]
async fn tdwr_against_wsr88d_on_the_chicago_line_tornadoes() {
    let http = reqwest::Client::new();
    let day = chrono::NaiveDate::from_ymd_opt(2020, 8, 10).unwrap();
    let klot = level2::list_volumes("KLOT", day).await.unwrap();
    let base = day.and_hms_opt(20, 0, 0).unwrap().and_utc();
    let (tlon, tlat) = (-87.858, 41.797);
    let (klon, klat) = (-88.085, 41.604);
    for (minute, lat, lon, place) in REPORTS {
        let at = base + chrono::Duration::minutes(minute);
        // TDWR: the volume as it stood at the report.
        let tdwr = match wxdata::tdwr::fetch_volume_at(&http, "TORD", at).await {
            Ok((name, _, scan)) => {
                let vel = (0..level2::elevation_angles(&scan).len())
                    .find_map(|t| level2::bin_scan_opts(&scan, Moment::Velocity, t, true).ok());
                vel.map(|v| (name, v))
            }
            Err(e) => {
                eprintln!("{place}: TDWR unavailable: {e}");
                None
            }
        };
        // KLOT: the volume that began nearest before the report.
        let id = klot
            .iter()
            .filter_map(|id| id.date_time().map(|t| (t, id.clone())))
            .filter(|(t, _)| *t <= at)
            .max_by_key(|(t, _)| *t)
            .map(|(_, id)| id)
            .unwrap();
        let scan = level2::download_scan(id.clone(), None).await.unwrap();
        let kvel = level2::bin_scan_opts(&scan, Moment::Velocity, 0, true).unwrap();
        let (kpeak, kn) = peak_near(&kvel, lon, lat, 3.0);
        eprintln!(
            "    KLOT near {place}: {}",
            coverage_near(&kvel, lon, lat, 3.0)
        );
        let line = match &tdwr {
            Some((name, v)) => {
                let (tpeak, tn) = peak_near(v, lon, lat, 3.0);
                eprintln!("    TDWR near {place}: {}", coverage_near(v, lon, lat, 3.0));
                format!(
                    "TDWR {name} {:.0} km: {tpeak:.4} s-1 ({tn} gates)",
                    km((tlon, tlat), (lon, lat))
                )
            }
            None => "TDWR: none".into(),
        };
        eprintln!(
            "{place} 20:{minute:02}Z | {line} | KLOT {:.0} km: {kpeak:.4} s-1 ({kn} gates)",
            km((klon, klat), (lon, lat))
        );
    }
}
