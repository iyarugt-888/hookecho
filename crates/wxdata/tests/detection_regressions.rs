//! detectionplan.md's non-negotiable regression tests, end to end: synthetic three-tilt volumes
//! through exactly the pipeline the app's Tornado ID runs (LLSD columns, tracking, debris
//! classification, fusion, `llsd_analyst::identify`), asserting on the verdict a person would see.
//! The pieces have their own unit tests; these pin the whole.

use wxdata::confirm::Confirmation;
use wxdata::level2::{BinnedSweep, Moment};
use wxdata::llsd_analyst::{analyse, identify, Analysed, LIKELY_SCORE, MIN_SCORE};
use wxdata::rotation_tracks::{TrackParams, Tracker};
use wxdata::tornado_id::{Tier, TornadoId};

const AZ: usize = 720;
const GATE_KM: f32 = 0.25;
const FIRST_KM: f32 = 2.125;
const GATES: usize = 600; // to 152 km
const TILTS: [f32; 3] = [0.5, 0.9, 1.3];
/// Where the storm is: azimuth (deg) and range (km) of its centre, and its radius (km).
const STORM: (f64, f64, f64) = (200.0, 50.0, 30.0);

/// Planar (east, north) km of a polar position.
fn xy(deg: f64, r: f64) -> (f64, f64) {
    let t = deg.to_radians();
    (r * t.sin(), r * t.cos())
}

fn binned(
    moment: Moment,
    elev: f32,
    lo: f32,
    hi: f32,
    f: &dyn Fn(f64, f64) -> Option<f32>,
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
        elevation_deg: elev,
        value_min: lo,
        value_max: hi,
        ..Default::default()
    }
}

/// One volume. `vel` is radial velocity (m/s) by (azimuth, range); reflectivity is 45 dBZ inside
/// the storm and nothing outside; `ball` puts a debris ball (55 dBZ, CC 0.4) of 1.5 km radius at
/// a polar position, CC 0.98 elsewhere in the storm.
struct Volume {
    vel_pairs: Vec<(BinnedSweep, BinnedSweep)>,
    cc_pairs: Vec<(BinnedSweep, BinnedSweep)>,
}

fn volume(vel: &dyn Fn(f64, f64) -> f32, ball: Option<(f64, f64)>, nyquist: f32) -> Volume {
    volume_at(vel, ball, nyquist, STORM.1)
}

/// [`volume`] with the storm centred `storm_r` km out instead.
fn volume_at(
    vel: &dyn Fn(f64, f64) -> f32,
    ball: Option<(f64, f64)>,
    nyquist: f32,
    storm_r: f64,
) -> Volume {
    let centre = xy(STORM.0, storm_r);
    let in_storm = move |d: f64, r: f64| {
        let p = xy(d, r);
        (p.0 - centre.0).hypot(p.1 - centre.1) <= STORM.2
    };
    let in_ball = move |d: f64, r: f64| {
        ball.is_some_and(|(bd, br)| {
            let (p, q) = (xy(d, r), xy(bd, br));
            (p.0 - q.0).hypot(p.1 - q.1) <= 1.5
        })
    };
    let (mut vel_pairs, mut cc_pairs) = (Vec::new(), Vec::new());
    for elev in TILTS {
        let z = binned(Moment::Reflectivity, elev, -32.0, 94.5, &|d, r| {
            if in_ball(d, r) {
                Some(55.0)
            } else {
                in_storm(d, r).then_some(45.0)
            }
        });
        let cc = binned(Moment::CorrelationCoefficient, elev, 0.2, 1.05, &|d, r| {
            if in_ball(d, r) {
                Some(0.4)
            } else {
                in_storm(d, r).then_some(0.98)
            }
        });
        let mut v = binned(Moment::Velocity, elev, -64.0, 64.0, &|d, r| Some(vel(d, r)));
        v.nyquist_ms = nyquist;
        vel_pairs.push((v, z.clone()));
        cc_pairs.push((z, cc));
    }
    Volume {
        vel_pairs,
        cc_pairs,
    }
}

/// Radial velocity of a Rankine vortex at (`az0`°, `r0` km), core radius `rc` km, peak `vmax` m/s
/// (positive: counterclockwise, cyclonic here).
fn rankine(az0: f64, r0: f64, rc: f64, vmax: f64) -> impl Fn(f64, f64) -> f32 {
    move |d, r| {
        let (x, y) = xy(d, r);
        let (xc, yc) = xy(az0, r0);
        let (dx, dy) = (x - xc, y - yc);
        let rho = dx.hypot(dy);
        if rho < 1e-9 {
            return 0.0;
        }
        let vt = if rho < rc {
            vmax * rho / rc
        } else {
            vmax * rc / rho
        };
        ((-vt * dy / rho * x + vt * dx / rho * y) / r) as f32
    }
}

/// The volume through the app's pipeline: analysed columns, and Tornado ID's verdicts with nobody
/// confirming anything.
fn run(tracker: &mut Tracker, time: i64, v: &Volume) -> (Vec<Analysed>, Vec<TornadoId>) {
    let columns = wxdata::rotation_columns::from_sweeps(&v.vel_pairs);
    let tracked = tracker.update(time, columns);
    let debris = wxdata::tds::detect_volume(&v.cc_pairs, 0.80, 40.0, 150.0, 4);
    let analysed = analyse(tracked, &debris, &[]);
    let ids = identify(&analysed, |_, _| Confirmation::default());
    (analysed, ids)
}

fn tracker() -> Tracker {
    Tracker::new(TrackParams::default())
}

fn max_score(a: &[Analysed]) -> f32 {
    a.iter().map(|a| a.fused.score).fold(0.0, f32::max)
}

#[test]
fn a_vortex_with_debris_on_it_is_a_debris_tier_tornado_where_it_is() {
    let v = volume(
        &rankine(STORM.0, STORM.1, 1.5, 40.0),
        Some((STORM.0, STORM.1)),
        0.0,
    );
    let (_, ids) = run(&mut tracker(), 0, &v);
    let top = ids.first().expect("a detection");
    assert_eq!(top.tier, Tier::Debris, "{top:#?}");
    assert!(top.score >= LIKELY_SCORE, "{}", top.score);
    let here = {
        let (x, y) = xy(STORM.0, STORM.1);
        let lat0 = 35.33f64;
        (
            -97.28 + x / (111.32 * lat0.to_radians().cos()),
            lat0 + y / 110.57,
        )
    };
    let km = ((top.lon - here.0) * 111.32 * 35.33f64.to_radians().cos())
        .hypot((top.lat - here.1) * 110.57);
    assert!(km < 3.0, "{km} km from the vortex");
}

#[test]
fn a_short_lived_extreme_circulation_is_not_held_back_for_lack_of_persistence() {
    // A violent vortex seen once, with no debris yet: it must already read as a tornado.
    let v = volume(&rankine(STORM.0, STORM.1, 1.0, 75.0), None, 0.0);
    let (a, ids) = run(&mut tracker(), 0, &v);
    assert!(
        ids.iter().any(|t| t.tier >= Tier::Likely),
        "best evidence {} with no persistence: {ids:#?}",
        max_score(&a)
    );
}

#[test]
fn a_persistent_coherent_circulation_becomes_more_credible() {
    // The same moderate vortex, moving ~15 m/s, seen once and then three volumes running.
    let mut tr = tracker();
    let mut scores = Vec::new();
    for k in 0..3 {
        let az = STORM.0 + 5.0 * k as f64; // ~4.4 km of arc at 50 km per 5 minutes
        let v = volume(&rankine(az, STORM.1, 1.5, 30.0), None, 0.0);
        let (a, _) = run(&mut tr, k * 300, &v);
        scores.push(max_score(&a));
    }
    assert!(scores[2] > scores[0], "{scores:?}");
}

#[test]
fn a_strong_persisting_circulation_without_debris_still_shows() {
    // 40 m/s winds in a 3 km core, rooted through three tilts, three volumes running, moving
    // ~15 m/s, and no debris: the legacy Tornado ID calls this Likely. It must at least show.
    let mut tr = tracker();
    let mut last = Vec::new();
    for k in 0..3 {
        let az = STORM.0 + 5.0 * k as f64;
        let v = volume(&rankine(az, STORM.1, 1.5, 40.0), None, 0.0);
        last = run(&mut tr, k * 300, &v).1;
    }
    assert!(last.iter().any(|t| t.tier >= Tier::Possible), "{last:#?}");
}

#[test]
fn one_wild_gate_in_a_storm_is_not_a_tornado() {
    let (a0, g0) = (400usize, 190usize); // ~200°, ~50 km: inside the storm
    let v = volume(
        &move |d, r| {
            let a = (d / 360.0 * AZ as f64).floor() as usize;
            let g = ((r as f32 - FIRST_KM) / GATE_KM).round() as usize;
            if (a, g) == (a0, g0) {
                45.0
            } else {
                0.0
            }
        },
        None,
        0.0,
    );
    let (a, ids) = run(&mut tracker(), 0, &v);
    assert!(ids.is_empty(), "{ids:#?}");
    assert!(max_score(&a) < MIN_SCORE);
}

#[test]
fn a_fold_dealiasing_left_behind_is_not_a_tornado() {
    // +24 / -24 m/s either side of the 200° radial through the storm, on a 26 m/s Nyquist.
    let v = volume(&|d, _| if d < 200.0 { 24.0 } else { -24.0 }, None, 26.0);
    let (_, ids) = run(&mut tracker(), 0, &v);
    assert!(ids.is_empty(), "{ids:#?}");
}

#[test]
fn a_radial_seam_is_not_a_circulation() {
    // Two radials that disagree along their whole length (a bad radial pair, a partial sweep's
    // edge): a 20 m/s step across the 200° radial.
    let v = volume(&|d, _| if d < 200.0 { 10.0 } else { -10.0 }, None, 0.0);
    let (_, ids) = run(&mut tracker(), 0, &v);
    assert!(ids.is_empty(), "{ids:#?}");
}

#[test]
fn a_gust_front_is_not_a_compact_tornado() {
    // Wind across a straight line through the storm flips from +15 to -15 m/s over 1 km.
    let line = xy(STORM.0, STORM.1);
    let v = volume(
        &move |d, r| {
            let (x, y) = xy(d, r);
            let north = 15.0 * ((x - line.0) / 0.5).tanh();
            (north * y / r) as f32
        },
        None,
        0.0,
    );
    let (_, ids) = run(&mut tracker(), 0, &v);
    assert!(ids.iter().all(|t| t.tier < Tier::Likely), "{ids:#?}");
}

#[test]
fn the_same_debris_twice_does_not_raise_the_score() {
    let v = volume(
        &rankine(STORM.0, STORM.1, 1.5, 40.0),
        Some((STORM.0, STORM.1)),
        0.0,
    );
    let columns = wxdata::rotation_columns::from_sweeps(&v.vel_pairs);
    let debris = wxdata::tds::detect_volume(&v.cc_pairs, 0.80, 40.0, 150.0, 4);
    let once = analyse(tracker().update(0, columns.clone()), &debris, &[]);
    let twice_debris: Vec<_> = debris.iter().chain(&debris).copied().collect();
    let twice = analyse(tracker().update(0, columns), &twice_debris, &[]);
    assert_eq!(max_score(&once), max_score(&twice));
}

#[test]
fn the_verdict_is_the_same_every_run() {
    let v = volume(
        &rankine(STORM.0, STORM.1, 1.5, 40.0),
        Some((STORM.0, STORM.1)),
        0.0,
    );
    let (_, a) = run(&mut tracker(), 0, &v);
    let (_, b) = run(&mut tracker(), 0, &v);
    assert_eq!(a, b);
}

/// A repeatable uniform number in [0, 1) for a gate, so noise is the same every run.
fn noise(d: f64, r: f64, seed: u64) -> f64 {
    let a = (d / 360.0 * AZ as f64).floor() as u64;
    let g = ((r - FIRST_KM as f64) / GATE_KM as f64).round() as u64;
    let mut z = (a << 32 | g) ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)) as f64 / u64::MAX as f64
}

/// A volume from its three fields directly: velocity, reflectivity and CC by (azimuth, range),
/// `None` where there is no echo (velocity is masked wherever reflectivity is).
fn volume_fields(
    vel: &dyn Fn(f64, f64) -> f32,
    z: &dyn Fn(f64, f64) -> Option<f32>,
    cc: &dyn Fn(f64, f64) -> Option<f32>,
) -> Volume {
    let (mut vel_pairs, mut cc_pairs) = (Vec::new(), Vec::new());
    for elev in TILTS {
        let zs = binned(Moment::Reflectivity, elev, -32.0, 94.5, z);
        let ccs = binned(Moment::CorrelationCoefficient, elev, 0.2, 1.05, cc);
        let v = binned(Moment::Velocity, elev, -64.0, 64.0, &|d, r| {
            z(d, r).map(|_| vel(d, r))
        });
        vel_pairs.push((v, zs.clone()));
        cc_pairs.push((zs, ccs));
    }
    Volume {
        vel_pairs,
        cc_pairs,
    }
}

#[test]
fn biological_scatter_is_not_a_tornado() {
    // A dense migration night: 12-28 dBZ (enough to pass the storm-context echo screen) with
    // low, scattered CC (0.3-0.6) everywhere out to 80 km, and birds flying a steady 10 m/s with
    // 6 m/s of scatter, for four volumes running.
    let mut tr = tracker();
    for k in 0..4u64 {
        let v = volume_fields(
            &move |d, r| {
                let toward = (d - 200.0).to_radians().cos() * 10.0;
                (toward + 12.0 * (noise(d, r, 3 * k) - 0.5)) as f32
            },
            &move |d, r| (r < 80.0).then(|| (12.0 + 16.0 * noise(d, r, 3 * k + 1)) as f32),
            &move |d, r| (r < 80.0).then(|| (0.3 + 0.3 * noise(d, r, 3 * k + 2)) as f32),
        );
        let (_, ids) = run(&mut tr, k as i64 * 300, &v);
        assert!(ids.is_empty(), "volume {k}: {ids:#?}");
    }
}

#[test]
fn wind_farm_clutter_is_not_a_tornado() {
    // A 5 km wind farm 40 km out on a quiet night: strong echo (50 dBZ), low CC (0.35-0.65) and
    // turbine blades scattering velocity by ±25 m/s, in the same place for four volumes.
    let farm = xy(STORM.0, 40.0);
    let on = move |d: f64, r: f64| {
        let p = xy(d, r);
        (p.0 - farm.0).hypot(p.1 - farm.1) <= 2.5
    };
    let mut tr = tracker();
    let mut worst = Tier::Possible;
    let mut shown = 0;
    for k in 0..4u64 {
        let v = volume_fields(
            &move |d, r| (50.0 * (noise(d, r, 3 * k) - 0.5)) as f32,
            &move |d, r| on(d, r).then_some(50.0),
            &move |d, r| on(d, r).then(|| (0.35 + 0.3 * noise(d, r, 3 * k + 2)) as f32),
        );
        let (_, ids) = run(&mut tr, k as i64 * 300, &v);
        shown += ids.len();
        worst = ids.iter().map(|t| t.tier).fold(worst, Tier::max);
        assert!(
            ids.iter().all(|t| t.tier < Tier::Likely),
            "volume {k}: {ids:#?}"
        );
    }
    assert_eq!(shown, 0, "worst tier {worst:?}");
}

#[test]
fn a_hail_core_with_no_rotation_is_not_a_tornado() {
    // A 65 dBZ core with CC down to 0.75 (big wet hail), inside a storm moving air at a steady
    // 10 m/s: low CC, no rotation, no tornado.
    let core = xy(STORM.0, STORM.1);
    let storm = xy(STORM.0, STORM.1);
    let in_storm = move |d: f64, r: f64| {
        let p = xy(d, r);
        (p.0 - storm.0).hypot(p.1 - storm.1) <= STORM.2
    };
    let in_core = move |d: f64, r: f64| {
        let p = xy(d, r);
        (p.0 - core.0).hypot(p.1 - core.1) <= 3.0
    };
    let v = volume_fields(
        &|_, _| 10.0,
        &move |d, r| {
            if in_core(d, r) {
                Some(65.0)
            } else {
                in_storm(d, r).then_some(45.0)
            }
        },
        &move |d, r| {
            if in_core(d, r) {
                Some(0.75)
            } else {
                in_storm(d, r).then_some(0.98)
            }
        },
    );
    let (a, ids) = run(&mut tracker(), 0, &v);
    assert!(ids.is_empty(), "{ids:#?}");
    assert!(max_score(&a) < MIN_SCORE);
}

/// Not a regression test: how much each negative case gives the pipeline to reject, so the tests
/// above cannot pass by having nothing reach it (`--ignored --nocapture`).
#[test]
#[ignore = "diagnostic"]
fn negative_cases_reach_the_pipeline() {
    use wxdata::azshear::{llsd, LlsdParams};
    use wxdata::rotation_objects::{objects, ObjectParams};
    let line = xy(STORM.0, STORM.1);
    let farm = xy(STORM.0, 40.0);
    let on = move |d: f64, r: f64| {
        let p = xy(d, r);
        (p.0 - farm.0).hypot(p.1 - farm.1) <= 2.5
    };
    let cases: Vec<(&str, Volume)> = vec![
        (
            "birds",
            volume_fields(
                &|d, r| {
                    ((d - 200.0).to_radians().cos() * 10.0 + 12.0 * (noise(d, r, 0) - 0.5)) as f32
                },
                &|d, r| (r < 80.0).then(|| (12.0 + 16.0 * noise(d, r, 1)) as f32),
                &|d, r| (r < 80.0).then(|| (0.3 + 0.3 * noise(d, r, 2)) as f32),
            ),
        ),
        (
            "wind farm",
            volume_fields(
                &|d, r| (50.0 * (noise(d, r, 0) - 0.5)) as f32,
                &move |d, r| on(d, r).then_some(50.0),
                &move |d, r| on(d, r).then(|| (0.35 + 0.3 * noise(d, r, 2)) as f32),
            ),
        ),
        (
            "fold",
            volume(&|d, _| if d < 200.0 { 24.0 } else { -24.0 }, None, 26.0),
        ),
        (
            "seam",
            volume(&|d, _| if d < 200.0 { 10.0 } else { -10.0 }, None, 0.0),
        ),
        (
            "gust",
            volume(
                &move |d, r| {
                    let (x, y) = xy(d, r);
                    ((15.0 * ((x - line.0) / 0.5).tanh()) * y / r) as f32
                },
                None,
                0.0,
            ),
        ),
    ];
    for (name, v) in cases {
        let (vel, z) = &v.vel_pairs[0];
        let objs = objects(
            &llsd(vel, &LlsdParams::default()),
            vel,
            z,
            &ObjectParams::default(),
        );
        let artifacts: Vec<_> = objs
            .iter()
            .map(|o| (o.max_azshear, o.artifacts.clone()))
            .take(4)
            .collect();
        let (a, ids) = run(&mut tracker(), 0, &v);
        eprintln!(
            "{name}: {} objects on the lowest tilt (strongest: {artifacts:?}), {} columns, best fused {:.2}, {} ids",
            objs.len(),
            a.len(),
            max_score(&a),
            ids.len()
        );
    }
}

/// Diagnostic: the field at the seam itself, and small (sub-beam-spacing) vortices.
#[test]
#[ignore = "diagnostic"]
fn seams_and_small_vortices_in_the_field() {
    use wxdata::azshear::{llsd, LlsdParams};
    let seam = volume(&|d, _| if d < 200.0 { 10.0 } else { -10.0 }, None, 0.0);
    let f = llsd(&seam.vel_pairs[0].0, &LlsdParams::default());
    let g = ((STORM.1 as f32 - FIRST_KM) / GATE_KM).round() as usize;
    let row: Vec<String> = (396..404)
        .map(|a| {
            f.at(a, g)
                .map_or("-".into(), |s| format!("{:.4}", s.shear_s))
        })
        .collect();
    eprintln!("seam shear across 198-202 deg at 50 km: {row:?}");
    for (rc, vmax, r0) in [
        (0.3, 50.0, 50.0),
        (0.3, 50.0, 90.0),
        (0.5, 40.0, 90.0),
        (1.5, 40.0, 50.0),
    ] {
        let v = volume_at(&rankine(STORM.0, r0, rc, vmax), None, 0.0, r0);
        let fld = llsd(&v.vel_pairs[0].0, &LlsdParams::default());
        let gg = ((r0 as f32 - FIRST_KM) / GATE_KM).round() as usize;
        let peak = (390..410)
            .flat_map(|a| (gg - 4..gg + 4).map(move |g| (a, g)))
            .filter_map(|(a, g)| fld.at(a, g).map(|s| s.shear_s))
            .fold(0.0f32, f32::max);
        let (an, ids) = run(&mut tracker(), 0, &v);
        eprintln!(
            "vortex rc {rc} km, {vmax} m/s at {r0} km: field peak {peak:.4} (true {:.4}), {} columns, best fused {:.2}, ids {:?}",
            vmax / (rc * 1000.0),
            an.len(),
            max_score(&an),
            ids.iter().map(|t| t.tier).collect::<Vec<_>>()
        );
    }
}
