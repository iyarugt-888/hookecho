//! Golden scientific checks on historic events (ROADMAP_2 §8.2): real archived volumes from the
//! NOAA Level II archive, run through the same pipeline the app and `--headless-backtest` use
//! (four tilts, dealiased velocity, debris and rotation detection, cross-corroboration, and the
//! one-detection-per-tornado merge), with what each must find pinned.
//!
//! The volumes are fetched, not committed (§8.1: "publicly available archive references rather
//! than committing huge raw files"), so these need the network:
//!
//!     cargo test -p wxdata --test golden_events -- --ignored
//!
//! What is pinned is what the science should give, with tolerances where the algorithm is
//! continuous: the tilt inventory exactly, peak reflectivity within 1 dBZ, and the detections by
//! where they are (within a few km of the documented tornado) rather than by exact score, so a
//! tuning change that keeps the answer passes and one that loses the tornado fails.

use chrono::{TimeZone, Utc};
use wxdata::level2::{self, BinnedSweep, Moment};
use wxdata::rotation::CoupletHit;
use wxdata::tds::TdsHit;
use wxdata::tornado_id::{circulations, Tier};

/// The volume that started at or just before `when` on `site`.
async fn volume(site: &str, when: chrono::DateTime<Utc>) -> level2::Scan {
    let ids = level2::list_volumes(site, when.date_naive())
        .await
        .expect("archive listing");
    let id = ids
        .into_iter()
        .filter(|id| id.date_time().is_some_and(|t| t <= when))
        .filter(|id| !id.name().ends_with("_MDM"))
        .max_by_key(|id| id.date_time())
        .expect("a volume before the time");
    eprintln!("{site} {when}: {}", id.name());
    level2::download_scan(id, None).await.expect("download")
}

struct Found {
    angles: Vec<f32>,
    max_dbz: f32,
    debris: Vec<TdsHit>,
    couplets: Vec<CoupletHit>,
}

/// The app's detector pipeline, as `--headless-backtest` runs it.
fn detect(scan: &level2::Scan) -> Found {
    let angles = level2::elevation_angles(scan);
    let mut pairs: Vec<(BinnedSweep, BinnedSweep)> = Vec::new();
    let mut vel_pairs: Vec<(BinnedSweep, BinnedSweep)> = Vec::new();
    let mut zdr = Vec::new();
    let mut max_dbz = f32::MIN;
    for tilt in 0..4 {
        let z = level2::bin_scan(scan, Moment::Reflectivity, tilt);
        let cc = level2::bin_scan(scan, Moment::CorrelationCoefficient, tilt);
        let vel = level2::bin_scan_opts(scan, Moment::Velocity, tilt, true);
        if let Ok(zd) = level2::bin_scan(scan, Moment::DifferentialReflectivity, tilt) {
            zdr.push(zd);
        }
        if tilt == 0 {
            if let Ok(z) = &z {
                // Codes 2..=255 span value_min..value_max, as `BinnedSweep::sample_at` reads them.
                let top = z
                    .data
                    .iter()
                    .copied()
                    .filter(|c| *c >= 2)
                    .max()
                    .unwrap_or(2);
                max_dbz = z.value_min + (top - 2) as f32 / 253.0 * (z.value_max - z.value_min);
            }
        }
        if let (Ok(z), Ok(cc)) = (&z, cc) {
            pairs.push((z.clone(), cc));
        }
        if let (Ok(z), Ok(vel)) = (z, vel) {
            vel_pairs.push((vel, z));
        }
    }
    let mut debris = wxdata::tds::detect_volume(&pairs, 0.80, 40.0, 150.0, 4);
    wxdata::tds::apply_zdr(&mut debris, &zdr);
    let mut couplets = wxdata::rotation::detect_volume(&vel_pairs, 25.0, 20.0, 15.0, 150.0, 3);
    wxdata::tds::cross_corroborate(&mut debris, &mut couplets, !vel_pairs.is_empty());
    Found {
        angles,
        max_dbz,
        debris,
        couplets,
    }
}

fn km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dlat = (b.1 - a.1) * 111.32;
    let dlon = (b.0 - a.0) * 111.32 * a.1.to_radians().cos();
    (dlat * dlat + dlon * dlon).sqrt()
}

fn report(f: &Found, tornado: (f64, f64)) {
    eprintln!("tilts {:?}", f.angles);
    eprintln!("max dBZ at the lowest tilt {:.1}", f.max_dbz);
    for d in &f.debris {
        eprintln!(
            "debris {:.3},{:.3} conf {:.2} cc {:.2} {:.1} km from the tornado",
            d.lon,
            d.lat,
            d.confidence,
            d.min_cc,
            km((d.lon, d.lat), tornado)
        );
    }
    for c in &f.couplets {
        eprintln!(
            "couplet {:.3},{:.3} conf {:.2} vrot {:.0} m/s {:.1} km from the tornado",
            c.lon,
            c.lat,
            c.confidence,
            c.vrot_ms,
            km((c.lon, c.lat), tornado)
        );
    }
    for z in circulations(&f.couplets, &f.debris) {
        eprintln!(
            "circulation {:.3},{:.3} {:?} {:.2} ({} members) {:.1} km from the tornado",
            z.id.lon,
            z.id.lat,
            z.id.tier,
            z.id.score,
            z.members.len(),
            km((z.id.lon, z.id.lat), tornado)
        );
    }
}

/// Moore, OK, 20 May 2013: the EF5 at Plaza Towers Elementary, about 20:16Z.
const MOORE: (f64, f64) = (-97.491, 35.332);
/// Mayfield, KY, 11 December 2021: the Quad-State tornado through downtown, about 03:27Z.
const MAYFIELD: (f64, f64) = (-88.636, 36.742);

/// What a tornado's volume must show, around where the tornado was.
struct Golden {
    tilts: usize,
    max_dbz: f32,
    /// Rotational velocity the couplet at the tornado must reach, m/s.
    vrot_ms: f32,
}

fn check(f: &Found, tornado: (f64, f64), g: Golden) {
    report(f, tornado);
    // Tilt inventory: the VCP's cuts, lowest first, the lowest at 0.5°.
    assert_eq!(f.angles.len(), g.tilts, "tilt inventory {:?}", f.angles);
    assert!(
        (f.angles[0] - 0.5).abs() < 0.1,
        "lowest tilt {}",
        f.angles[0]
    );
    assert!(f.angles.windows(2).all(|w| w[0] < w[1]), "tilts ascend");
    // Reflectivity: the lowest tilt's peak, within a dBZ.
    assert!(
        (f.max_dbz - g.max_dbz).abs() <= 1.0,
        "peak {:.1} dBZ, golden {:.1}",
        f.max_dbz,
        g.max_dbz
    );
    // The debris ball: low CC in strong echo, confidently, near the tornado.
    let near = |lon: f64, lat: f64| km((lon, lat), tornado) <= 8.0;
    assert!(
        f.debris
            .iter()
            .any(|d| near(d.lon, d.lat) && d.min_cc <= 0.25 && d.confidence >= 0.8),
        "no debris signature at the tornado"
    );
    // Its couplet.
    assert!(
        f.couplets
            .iter()
            .any(|c| near(c.lon, c.lat) && c.vrot_ms >= g.vrot_ms && c.confidence >= 0.6),
        "no couplet of {} m/s at the tornado",
        g.vrot_ms
    );
    // And one detection for the one tornado, not a scatter of them.
    let here: Vec<_> = circulations(&f.couplets, &f.debris)
        .into_iter()
        .filter(|z| km((z.id.lon, z.id.lat), tornado) <= 15.0)
        .collect();
    assert_eq!(here.len(), 1, "one tornado, one detection");
    assert!(here[0].id.tier >= Tier::Debris, "{:?}", here[0].id.tier);
    assert!(here[0].id.score >= 0.9, "score {}", here[0].id.score);
    assert!(here[0].rotations() >= 1 && here[0].debris() >= 1);
}

/// KTLX 20:12:29Z, 20 May 2013: the Moore EF5 four minutes before Plaza Towers.
#[tokio::test]
#[ignore = "network"]
async fn moore_2013() {
    let scan = volume(
        "KTLX",
        Utc.with_ymd_and_hms(2013, 5, 20, 20, 16, 0).unwrap(),
    )
    .await;
    check(
        &detect(&scan),
        MOORE,
        Golden {
            tilts: 14,
            max_dbz: 68.4,
            vrot_ms: 45.0,
        },
    );
}

/// KPAH 03:23:49Z, 11 December 2021: the Quad-State tornado approaching downtown Mayfield.
#[tokio::test]
#[ignore = "network"]
async fn mayfield_2021() {
    let scan = volume(
        "KPAH",
        Utc.with_ymd_and_hms(2021, 12, 11, 3, 27, 0).unwrap(),
    )
    .await;
    check(
        &detect(&scan),
        MAYFIELD,
        Golden {
            tilts: 14,
            max_dbz: 66.4,
            vrot_ms: 25.0,
        },
    );
}
