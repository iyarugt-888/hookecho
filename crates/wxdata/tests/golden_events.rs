//! Golden scientific checks on historic events (ROADMAP_2 §8.2): real archived volumes from the
//! NOAA Level II archive, run through the same pipeline the app and `--headless-backtest` use
//! (four tilts, dealiased velocity, debris and rotation detection, cross-corroboration, and the
//! one-detection-per-tornado merge), with what each must find pinned.
//!
//! Exact source objects and SHA-256 checksums live in tests/data/corpus/manifest.json.
//! The full volumes are fetched, not committed (§8.1: "publicly available archive references
//! rather than committing huge raw files"), so these need the network by default:
//!
//!     cargo test -p wxdata --test golden_events -- --ignored
//!
//! Set HOOKECHO_CORPUS_CACHE after provisioning scripts/corpus/provision.py to read verified
//! local radar inputs. Warning/report truth snapshots are committed and checksum-verified,
//! so every check in this file runs offline when its radar inputs are provisioned.
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

#[path = "support/corpus.rs"]
pub mod corpus;
use corpus::historic_volume as volume;

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
    // 0.85, not 0.9: Mayfield's old 0.9+ came partly from counting the debris and the couplet
    // twice (each boosted by the other, then combined). Fused once (detectionplan.md Phase 1) it
    // scores 0.89; the Debris tier above is what says "tornado".
    assert!(here[0].id.score >= 0.85, "score {}", here[0].id.score);
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
            // The newest 0.5° reflectivity cut has raw peak 70.5 dBZ, quantized to
            // 70.4. The former 68.4 golden described the first cut (raw 68.5).
            max_dbz: 70.4,
            vrot_ms: 45.0,
        },
    );
}

/// Archive replay is deterministic (ROADMAP_2 §0.2): the same volume, downloaded and decoded
/// twice, gives byte-identical tilts (reflectivity and dealiased velocity, every tilt) and exactly
/// the same detections, scores and merged circulations. Two independent runs rather than one scan
/// run twice, so state carried in the decoder, the dealiaser or a cache would show up as a
/// difference.
#[tokio::test]
#[ignore = "network"]
async fn archive_replay_is_deterministic() {
    let when = Utc.with_ymd_and_hms(2013, 5, 20, 20, 16, 0).unwrap();
    let run = |scan: &level2::Scan| {
        let tilts = level2::elevation_angles(scan).len();
        let mut sweeps = Vec::new();
        for tilt in 0..tilts {
            for (moment, dealias) in [(Moment::Reflectivity, false), (Moment::Velocity, true)] {
                if let Ok(s) = level2::bin_scan_opts(scan, moment, tilt, dealias) {
                    sweeps.push((tilt, moment, s.data, s.az_bins, s.gate_count));
                }
            }
        }
        let f = detect(scan);
        let merged = format!("{:?}", circulations(&f.couplets, &f.debris));
        (sweeps, format!("{:?} {:?}", f.debris, f.couplets), merged)
    };
    let a = run(&volume("KTLX", when).await);
    let b = run(&volume("KTLX", when).await);
    assert!(!a.0.is_empty(), "no tilts binned");
    assert_eq!(a.0.len(), b.0.len(), "a different number of tilts binned");
    for (x, y) in a.0.iter().zip(&b.0) {
        assert_eq!(
            (x.0, x.1, x.3, x.4),
            (y.0, y.1, y.3, y.4),
            "tilt inventory differs"
        );
        assert!(x.2 == y.2, "tilt {} {:?} differs between runs", x.0, x.1);
    }
    assert_eq!(a.1, b.1, "detections differ between runs");
    assert_eq!(a.2, b.2, "merged circulations differ between runs");
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
            // The newest SAILS repeat has raw peak 68.0 dBZ, quantized to 67.9.
            // The former 66.4 golden described the first cut (raw 66.5).
            max_dbz: 67.9,
            vrot_ms: 25.0,
        },
    );
}

/// Historical warning verification (ROADMAP_2 §8.4) on the Moore case: the archived warning is
/// there at the instant it was in effect and not before it was issued, the tornado report is where
/// the tornado was, and the verification scorer credits the merged detection with it.
#[tokio::test]
#[ignore = "network"]
async fn moore_2013_warnings_reports_and_verification() {
    use wxdata::detverify::{score, Detection, Truth};
    use wxdata::overlay::point_in_ring;
    let covers = |feats: &[wxdata::overlay::GeoFeature]| {
        feats.iter().any(|f| {
            f.alert
                .as_ref()
                .is_some_and(|a| a.event == "Tornado Warning")
                && f.rings.iter().any(|r| point_in_ring(r, MOORE.0, MOORE.1))
        })
    };
    // In effect over Moore at the golden volume's time.
    let during = corpus::warnings_at(Utc.with_ymd_and_hms(2013, 5, 20, 20, 12, 0).unwrap());
    assert!(covers(&during), "no tornado warning over Moore at 20:12Z");
    // Not yet issued two hours earlier, before the storm formed.
    let before = corpus::warnings_at(Utc.with_ymd_and_hms(2013, 5, 20, 18, 0, 0).unwrap());
    assert!(
        !covers(&before),
        "a tornado warning over Moore before it was issued"
    );

    // The tornado reports in the hour around the volume, near Moore.
    let reports = corpus::reports_between(
        Utc.with_ymd_and_hms(2013, 5, 20, 19, 40, 0).unwrap(),
        Utc.with_ymd_and_hms(2013, 5, 20, 20, 40, 0).unwrap(),
    );
    let tornadoes: Vec<_> = reports
        .iter()
        .filter(|r| r.kind == wxdata::spc::ReportKind::Tornado)
        .filter(|r| km((r.lon, r.lat), MOORE) <= 15.0)
        .collect();
    assert!(!tornadoes.is_empty(), "no tornado report near Moore");

    // The merged detection at 20:12Z verifies against them.
    let scan = volume(
        "KTLX",
        Utc.with_ymd_and_hms(2013, 5, 20, 20, 16, 0).unwrap(),
    )
    .await;
    let f = detect(&scan);
    let minute = |hhmm: &str| {
        let h: i64 = hhmm.get(..2).and_then(|h| h.parse().ok()).unwrap_or(0);
        let m: i64 = hhmm.get(2..4).and_then(|m| m.parse().ok()).unwrap_or(0);
        h * 60 + m
    };
    let detections: Vec<Detection> = circulations(&f.couplets, &f.debris)
        .iter()
        .map(|z| Detection {
            lon: z.id.lon,
            lat: z.id.lat,
            confidence: z.id.score,
            minute: 20 * 60 + 12,
            range_km: 0.0,
        })
        .collect();
    let truths: Vec<Truth> = tornadoes
        .iter()
        .map(|r| Truth {
            lon: r.lon,
            lat: r.lat,
            minute: minute(&r.time),
        })
        .collect();
    let s = score(&detections, &truths, 15.0, 30, &[0.9]);
    eprintln!("{s:?} from {} reports", truths.len());
    assert!(
        s[0].verified >= 1,
        "the Moore detection did not verify: {s:?}"
    );
    assert!(s[0].found >= 1, "the Moore tornado was not found: {s:?}");
}

/// A corpus event, checked against the tornado reports themselves (ROADMAP_2 §8.1): the reported
/// positions are the truth, so no location is typed in by hand.
struct Case {
    site: &'static str,
    /// When a tornado was on the ground (a few minutes after the corpus line's time).
    when: (i32, u32, u32, u32, u32),
}

/// The tornado reports within 20 minutes and 150 km of the case's scan, and what the app found.
async fn corpus_case(c: &Case) -> (Vec<(f64, f64)>, Vec<wxdata::tornado_id::Circulation>) {
    let (y, mo, d, h, mi) = c.when;
    let when = Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap();
    let scan = volume(c.site, when).await;
    let f = detect(&scan);
    let site = wxdata::sites::site_by_id(c.site).expect("a known radar");
    let radar = (f64::from(site.longitude), f64::from(site.latitude));
    let reports = corpus::reports_between(
        when - chrono::Duration::minutes(20),
        when + chrono::Duration::minutes(20),
    );
    let tornadoes: Vec<(f64, f64)> = reports
        .iter()
        .filter(|r| r.kind == wxdata::spc::ReportKind::Tornado)
        .map(|r| (r.lon, r.lat))
        .filter(|p| km(*p, radar) <= 150.0)
        .collect();
    (tornadoes, circulations(&f.couplets, &f.debris))
}

const CORPUS: [Case; 4] = [
    Case {
        site: "KTLX",
        when: (2013, 5, 31, 23, 10),
    }, // El Reno
    Case {
        site: "KILX",
        when: (2013, 11, 17, 17, 5),
    }, // Washington, IL
    Case {
        site: "KLZK",
        when: (2014, 4, 28, 0, 35),
    }, // Mayflower and Vilonia
    Case {
        site: "KOHX",
        when: (2020, 3, 3, 6, 45),
    }, // Nashville
];

#[tokio::test]
#[ignore = "network"]
async fn record_corpus() {
    for c in &CORPUS {
        let (tornadoes, circs) = corpus_case(c).await;
        eprintln!(
            "== {} {:?}: {} tornado reports",
            c.site,
            c.when,
            tornadoes.len()
        );
        for z in &circs {
            let near = tornadoes
                .iter()
                .map(|t| km(*t, (z.id.lon, z.id.lat)))
                .fold(f64::INFINITY, f64::min);
            eprintln!(
                "   {:?} {:.2} ({} members) nearest report {:.1} km",
                z.id.tier,
                z.id.score,
                z.members.len(),
                near
            );
        }
    }
}

/// Each corpus tornado is found where it was reported, at Likely or higher, as one detection.
/// Recorded 2026-09-30: El Reno Debris 0.91 at 1.4 km, Washington Likely 0.70 at 1.7 km,
/// Vilonia Debris 0.64 at 1.1 km, Nashville Debris 0.66 at 5.3 km from a report.
#[tokio::test]
#[ignore = "network"]
async fn corpus_tornadoes_are_found_once_where_reported() {
    for c in &CORPUS {
        let (tornadoes, circs) = corpus_case(c).await;
        assert!(!tornadoes.is_empty(), "{}: no tornado reports", c.site);
        let found = tornadoes.iter().find(|t| {
            circs
                .iter()
                .any(|z| z.id.tier >= Tier::Likely && km(**t, (z.id.lon, z.id.lat)) <= 8.0)
        });
        let Some(t) = found else {
            panic!(
                "{} {:?}: no Likely detection within 8 km of a report",
                c.site, c.when
            );
        };
        let here = circs
            .iter()
            .filter(|z| km(*t, (z.id.lon, z.id.lat)) <= 8.0)
            .count();
        assert_eq!(here, 1, "{}: one tornado, one detection", c.site);
    }
}

/// Point-to-segment distance in the same local-km approximation as the report checks.
fn track_km(point: (f64, f64), line: &[[f64; 2]]) -> f64 {
    let x = 111.32 * point.1.to_radians().cos();
    line.windows(2)
        .map(|w| {
            let a = ((w[0][0] - point.0) * x, (w[0][1] - point.1) * 111.32);
            let b = ((w[1][0] - point.0) * x, (w[1][1] - point.1) * 111.32);
            let delta = (b.0 - a.0, b.1 - a.1);
            let norm = delta.0 * delta.0 + delta.1 * delta.1;
            let t = if norm > 0.0 {
                (-(a.0 * delta.0 + a.1 * delta.1) / norm).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (a.0 + t * delta.0).hypot(a.1 + t * delta.1)
        })
        .fold(f64::INFINITY, f64::min)
}

/// Iowa's December 2021 QLCS: independently analyzed NWS paths, not sparse LSR
/// point locations. Knierim must retain a Likely-or-higher detection, and no single
/// detection may stand in for both tornadoes. Somers has no candidate in this baseline,
/// which is an open science gap. Its earlier Possible candidate (score 0.59, 7.87 km from
/// the path) stood on mis-unfolded velocity: within 10 km of it, 3–21% of the 0.5–1.8°
/// gates sat a fold away from Py-ART 2.3.0's `dealias_region_based`, showing inbound
/// values down to −27 m/s where Py-ART has −12 to −15. The current dealiaser agrees with
/// Py-ART on every one of those gates, and the shear the candidate scored on is gone
/// (ROADMAP_PARITY M3.1). Path vertices have no individual clocks, so this is proximity to
/// an active damage path, not exact tornado localization.
#[tokio::test]
#[ignore = "large cached fixture or pinned archive download"]
async fn iowa_qlcs_2021_retains_distinct_track_candidates() {
    let when = Utc.with_ymd_and_hms(2021, 12, 15, 23, 44, 0).unwrap();
    let scan = volume("KDMX", when).await;
    let f = detect(&scan);
    let circs = circulations(&f.couplets, &f.debris);
    let manifest = corpus::manifest();
    let tracks: Vec<_> = ["knierim-2021-damage-track", "somers-2021-damage-track"]
        .into_iter()
        .map(|id| {
            let track = manifest
                .track_snapshots
                .iter()
                .find(|t| t.id == id)
                .expect("required QLCS track");
            assert!(
                track.start <= when && when <= track.end,
                "track must be active at the case time"
            );
            (
                track,
                corpus::read_track(track).expect("verified NWS damage track"),
            )
        })
        .collect();
    let mut assigned: [Vec<_>; 2] = [Vec::new(), Vec::new()];
    // Assign each circulation only to its nearest of these paths. A single
    // merged detection cannot satisfy both neighboring tornadoes' expectations.
    for z in &circs {
        let nearest = tracks
            .iter()
            .enumerate()
            .map(|(i, (_, line))| (i, track_km((z.id.lon, z.id.lat), line)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        eprintln!(
            "QLCS {:.6},{:.6} {:?} score {:.2}: {:.2} km from {}",
            z.id.lon, z.id.lat, z.id.tier, z.id.score, nearest.1, tracks[nearest.0].0.event_name
        );
        if nearest.1 <= 8.0 {
            assigned[nearest.0].push(z);
        }
    }
    let [knierim, somers] = &assigned;
    assert_eq!(
        knierim.len(),
        1,
        "{}: one distinct candidate",
        tracks[0].0.event_name
    );
    assert!(
        somers.len() <= 1,
        "{}: at most one distinct candidate",
        tracks[1].0.event_name
    );
    assert!(
        assigned[0][0].id.tier >= Tier::Likely,
        "Knierim lost its stronger detection"
    );
}

/// The corpus's false-alarm cases (ROADMAP_2 §8.1): what the detectors must not claim.
const QUIET: [Case; 3] = [
    // Joplin, 22 May 2011: KSGF was not yet dual-pol, so a debris signature is impossible.
    Case {
        site: "KSGF",
        when: (2011, 5, 22, 22, 40),
    },
    // The Iowa derecho, 10 August 2020: this scan is a debris false-positive control.
    // This assertion does not claim that no tornadoes occurred anywhere in the event.
    Case {
        site: "KDVN",
        when: (2020, 8, 10, 17, 45),
    },
    // The Denver hailstorm, 8 May 2017: large hail lowers CC too.
    Case {
        site: "KFTG",
        when: (2017, 5, 8, 20, 35),
    },
];

/// The false-alarm cases claim no debris they cannot have: none at all on Joplin's pre-dual-pol
/// radar, and no Debris-tier detection in the derecho or the hailstorm (the hailstorm's low-CC
/// hail core, with no rotation near it, read as a tornado until unrotated debris was capped at
/// Possible).
#[tokio::test]
#[ignore = "network"]
async fn quiet_cases_claim_no_debris_tornado() {
    for c in &QUIET {
        let (y, mo, d, h, mi) = c.when;
        let scan = volume(c.site, Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()).await;
        let f = detect(&scan);
        let circs = circulations(&f.couplets, &f.debris);
        eprintln!(
            "== {}: {} debris, {} couplets, circulations {:?}",
            c.site,
            f.debris.len(),
            f.couplets.len(),
            circs
                .iter()
                .map(|z| (z.id.tier, (z.id.score * 100.0).round()))
                .collect::<Vec<_>>()
        );
        if c.site == "KSGF" {
            assert!(f.debris.is_empty(), "debris on a radar with no CC");
        }
        assert!(
            circs.iter().all(|z| z.id.tier < Tier::Debris),
            "{}: a debris-tier tornado claimed",
            c.site
        );
    }
}

#[tokio::test]
#[ignore = "network"]
async fn record_denver_hail_detail() {
    let scan = volume("KFTG", Utc.with_ymd_and_hms(2017, 5, 8, 20, 35, 0).unwrap()).await;
    let f = detect(&scan);
    for z in circulations(&f.couplets, &f.debris) {
        eprintln!(
            "{:?} {:.2} reasons {:?}",
            z.id.tier, z.id.score, z.id.reasons
        );
        for m in &z.members {
            match m.evidence {
                wxdata::tornado_id::Evidence::Debris(i) => {
                    let d = &f.debris[i];
                    eprintln!(
                        "  debris conf {:.2} cc {:.2} z {:.0} zdr {:?} tilts {} unrotated {} rot {:?}",
                        d.confidence, d.min_cc, d.max_z, d.zdr_db, d.tilts, d.unrotated, d.rotation_ms
                    );
                }
                wxdata::tornado_id::Evidence::Rotation(i) => {
                    let c = &f.couplets[i];
                    eprintln!(
                        "  couplet conf {:.2} vrot {:.0} {:.1} km",
                        c.confidence, c.vrot_ms, m.km
                    );
                }
            }
        }
    }
}
