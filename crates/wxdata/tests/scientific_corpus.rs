//! Small real partial Archive II inputs run offline on every PR. Large inputs are opt-in,
//! cached, and still fail if required files are missing; see scripts/corpus/README.md.
#[path = "support/corpus.rs"]
pub mod corpus;

use wxdata::level2::{self, Moment};

#[test]
fn manifest_and_required_offline_checksums_are_valid() {
    let m = corpus::manifest();
    let offline: Vec<_> = m.fixtures.iter().filter(|f| f.tier == "offline").collect();
    assert_eq!(
        offline.len(),
        3,
        "required small subset must not silently shrink"
    );
    for f in offline {
        corpus::read(f, &corpus::cache_dir()).unwrap();
    }
}

#[test]
fn damaged_fixtures_and_unsafe_manifests_fail_explicitly() {
    let mut m = corpus::manifest();
    let f = m.fixtures.iter().find(|f| f.tier == "offline").unwrap();
    let mut data = corpus::read(f, &corpus::cache_dir()).unwrap();
    data[24] ^= 1;
    assert!(corpus::verify(f, &data)
        .unwrap_err()
        .to_string()
        .contains("SHA-256"));
    assert!(corpus::verify(f, &data[..20]).is_err());
    assert!(corpus::read(
        &m.fixtures[0],
        &std::path::PathBuf::from("a-required-cache-that-does-not-exist")
    )
    .unwrap_err()
    .to_string()
    .contains("required fixture"));
    m.fixtures[0].path = "../escape.ar2".into();
    assert!(corpus::validate(&m).is_err());
    m.schema_version = 999;
    assert!(corpus::validate(&m).is_err());
}

#[test]
fn pinned_truth_preserves_archive_membership_and_shape() {
    let m = corpus::manifest();
    assert_eq!(
        m.truth_snapshots.len(),
        8,
        "required truth subset must not shrink"
    );
    for f in &m.truth_snapshots {
        let text = corpus::read_truth(f).unwrap();
        let mut damaged = text.as_bytes().to_vec();
        damaged[0] ^= 1;
        assert!(corpus::verify_bytes(&f.id, f.bytes, &f.sha256, &damaged).is_err());
        if let corpus::TruthRequest::Reports { start, end } = f.request {
            let reports = corpus::reports_between(start, end);
            assert_eq!(
                reports.len(),
                f.expected_features,
                "every pinned report retains its coordinates"
            );
            assert!(
                reports
                    .iter()
                    .any(|r| r.kind == wxdata::spc::ReportKind::Tornado),
                "required tornado truth"
            );
        }
    }
    let point = (-97.491, 35.332);
    let covers = |features: &[wxdata::overlay::GeoFeature]| {
        features.iter().any(|f| {
            f.alert
                .as_ref()
                .is_some_and(|a| a.event == "Tornado Warning")
                && f.rings
                    .iter()
                    .any(|r| wxdata::overlay::point_in_ring(r, point.0, point.1))
        })
    };
    use chrono::TimeZone;
    let during = corpus::warnings_at(
        chrono::Utc
            .with_ymd_and_hms(2013, 5, 20, 20, 12, 0)
            .unwrap(),
    );
    let before = corpus::warnings_at(chrono::Utc.with_ymd_and_hms(2013, 5, 20, 18, 0, 0).unwrap());
    assert!(covers(&during));
    assert!(
        !covers(&before),
        "warning must not exist before it was issued"
    );
}

#[test]
fn pinned_damage_tracks_preserve_geometry_and_source_evidence() {
    let mut m = corpus::manifest();
    assert_eq!(
        m.track_snapshots.len(),
        2,
        "required track subset must not shrink"
    );
    for f in &m.track_snapshots {
        let line = corpus::read_track(f).unwrap();
        assert!(line.windows(2).any(|w| w[0] != w[1]));
    }
    let f = m
        .track_snapshots
        .iter()
        .find(|f| f.id == "somers-2021-damage-track")
        .unwrap();
    assert!(
        f.evidence.starts_with("Not surveyed."),
        "source limitations must survive"
    );
    m.track_snapshots[0].event_name = "invented tornado".into();
    assert!(corpus::read_track(&m.track_snapshots[0]).is_err());
    m.track_snapshots[0].path = "../escape.kmz".into();
    assert!(corpus::validate(&m).is_err());
    assert!(corpus::track_properties("<tr><td><b>x</b></td><td>y</td>").is_err());
    assert!(corpus::track_properties(
        "<tr><td><b>x</b></td><td>y</td></tr><tr><td><b>x</b></td><td>z</td></tr>"
    )
    .is_err());
}

#[test]
fn offline_radar_inputs_preserve_values_time_and_missing_coverage() {
    for f in corpus::manifest()
        .fixtures
        .iter()
        .filter(|f| f.tier == "offline")
    {
        let bytes = corpus::read(f, &corpus::cache_dir()).unwrap();
        let scan = level2::decode_volume(bytes.clone()).expect("real partial volume");
        let again = level2::decode_volume(bytes).unwrap();
        let a = level2::bin_scan(&scan, Moment::Reflectivity, 0).unwrap();
        let b = level2::bin_scan(&again, Moment::Reflectivity, 0).unwrap();
        assert_eq!(
            a.data, b.data,
            "independent decodes must produce identical values"
        );
        assert_eq!(a.bin_time_ms, b.bin_time_ms);
        let populated = a.bin_time_ms.iter().filter(|&&t| t > 0).count();
        let max_code = a.data.iter().copied().max().unwrap();
        assert!(max_code >= 2);
        let max_dbz = a.value_min + (max_code - 2) as f32 / 253.0 * (a.value_max - a.value_min);
        eprintln!("{}: elevations={:?}, az_bins={}, gate_count={}, populated_bins={}, reflectivity_sha256={}, max_dbz={max_dbz}", f.id, level2::elevation_angles(&scan), a.az_bins, a.gate_count, populated, corpus::digest(&a.data));
        let e = f.expected.as_ref().expect("required scientific baseline");
        assert_eq!(level2::elevation_angles(&scan), e.elevations);
        assert_eq!(
            (a.az_bins, a.gate_count, populated),
            (e.az_bins, e.gate_count, e.populated_bins)
        );
        assert_eq!(corpus::digest(&a.data), e.reflectivity_sha256);
        assert!((max_dbz - e.max_dbz).abs() < 0.01);
        assert!(populated > 0 && populated < a.az_bins);
        for (row, &time) in a.data.chunks_exact(a.gate_count).zip(&a.bin_time_ms) {
            if time == 0 {
                assert!(
                    row.iter().all(|&v| v == 0),
                    "unobserved azimuth must remain missing"
                );
            } else {
                assert!(time >= f.source.acquisition_time.timestamp_millis());
                assert!(
                    time < (f.source.acquisition_time + chrono::Duration::minutes(10))
                        .timestamp_millis()
                );
            }
        }
        // Missing upper elevations remain unavailable rather than being inserted as
        // observations into a column profile.
        assert_eq!(level2::elevation_angles(&scan).len(), 1);
        assert!(level2::bin_scan(&scan, Moment::Reflectivity, 1).is_err());
    }
}

#[test]
#[ignore = "large cached fixtures: provision explicitly before running"]
fn cached_clear_air_control() {
    let m = corpus::manifest();
    let f = m
        .fixtures
        .iter()
        .find(|f| f.id == "clear-air-2019")
        .unwrap();
    let scan = level2::decode_volume(corpus::read(f, &corpus::cache_dir()).unwrap()).unwrap();
    let mut pairs = Vec::new();
    let mut velocity = Vec::new();
    let mut zdr = Vec::new();
    for tilt in 0..4 {
        let z = level2::bin_scan(&scan, Moment::Reflectivity, tilt).unwrap();
        if let Ok(cc) = level2::bin_scan(&scan, Moment::CorrelationCoefficient, tilt) {
            pairs.push((z.clone(), cc));
        }
        if let Ok(v) = level2::bin_scan_opts(&scan, Moment::Velocity, tilt, true) {
            velocity.push((v, z));
        }
        if let Ok(d) = level2::bin_scan(&scan, Moment::DifferentialReflectivity, tilt) {
            zdr.push(d);
        }
    }
    assert!(!pairs.is_empty());
    assert!(
        !velocity.is_empty(),
        "classification needs velocity evidence"
    );
    let mut hits = wxdata::tds::detect_volume(&pairs, 0.80, 40.0, 150.0, 4);
    wxdata::tds::apply_zdr(&mut hits, &zdr);
    let mut couplets = wxdata::rotation::detect_volume(&velocity, 25.0, 20.0, 15.0, 150.0, 3);
    wxdata::tds::cross_corroborate(&mut hits, &mut couplets, true);
    let circulations = wxdata::tornado_id::circulations(&couplets, &hits);
    eprintln!("clear-air control: {} debris candidates", hits.len());
    assert!(
        circulations
            .iter()
            .all(|c| c.id.tier < wxdata::tornado_id::Tier::Debris),
        "weak-echo fixture must not claim a debris-tier tornado: {circulations:?}"
    );
}

#[test]
#[ignore = "large cached fixtures: provision explicitly before running"]
fn cached_repeated_cut_selection_matches_raw_observations() {
    for (id, first_peak, latest_peak) in [("moore-2013", 68.5, 70.5), ("mayfield-2021", 66.5, 68.0)]
    {
        let m = corpus::manifest();
        let f = m.fixtures.iter().find(|f| f.id == id).unwrap();
        let scan = level2::decode_volume(corpus::read(f, &corpus::cache_dir()).unwrap()).unwrap();
        let angle = level2::elevation_angles(&scan)[0];
        let mut cuts = Vec::new();
        for sweep in scan.sweeps().iter().filter(|s| {
            s.elevation_angle_degrees()
                .is_some_and(|e| (e - angle).abs() < 0.15)
        }) {
            let radials: Vec<_> = sweep
                .radials()
                .iter()
                .filter(|r| r.reflectivity().is_some())
                .collect();
            if radials.is_empty() {
                continue;
            }
            let time = radials
                .iter()
                .map(|r| r.collection_timestamp())
                .max()
                .unwrap();
            let peak = radials
                .iter()
                .flat_map(|r| r.reflectivity().unwrap().iter())
                .filter_map(|v| match v {
                    nexrad_model::data::MomentValue::Value(v) => Some(v),
                    _ => None,
                })
                .fold(f32::NEG_INFINITY, f32::max);
            cuts.push((time, peak));
        }
        eprintln!("{id} reflectivity cuts (latest radial ms, raw peak dBZ): {cuts:?}");
        assert!(
            cuts.len() > 1,
            "real input must include repeated reflectivity cuts"
        );
        let latest = cuts.iter().max_by_key(|(time, _)| time).unwrap();
        assert!(latest.0 > cuts[0].0);
        assert_eq!(cuts[0].1, first_peak, "first-cut raw baseline");
        assert_eq!(latest.1, latest_peak, "newest-cut raw baseline");
        let selected = level2::bin_scan(&scan, Moment::Reflectivity, 0).unwrap();
        let top = selected.data.iter().copied().max().unwrap();
        let peak = selected.value_min
            + (top - 2) as f32 / 253.0 * (selected.value_max - selected.value_min);
        assert!(
            (peak - latest.1).abs() <= (selected.value_max - selected.value_min) / 253.0,
            "selected peak must match latest raw observations within one quantization step"
        );
    }
}

#[test]
#[ignore = "large cached fixtures: provision explicitly before running"]
fn cached_derived_products_repeat_with_source_clock() {
    for id in ["mayfield-2021", "denver-hail-2017"] {
        let m = corpus::manifest();
        let f = m.fixtures.iter().find(|f| f.id == id).unwrap();
        let run = || {
            let scan =
                level2::decode_volume(corpus::read(f, &corpus::cache_dir()).unwrap()).unwrap();
            let sweeps: Vec<_> = (0..4)
                .map(|tilt| {
                    let mut s = level2::bin_scan(&scan, Moment::Reflectivity, tilt).unwrap();
                    // Explicit 60 km range crop bounds the scheduled check, preserving original
                    // gate values and missing rows. No new observations are interpolated.
                    let gates = s.gate_count.min(240);
                    s.data = s
                        .data
                        .chunks_exact(s.gate_count)
                        .flat_map(|row| row[..gates].iter().copied())
                        .collect();
                    s.gate_count = gates;
                    s
                })
                .collect();
            wxdata::derived::derive(
                &sweeps,
                &wxdata::derived::DerivedOpts {
                    time: f.source.acquisition_time,
                    ..Default::default()
                },
            )
            .expect("four real low tilts derive products")
        };
        let a = run();
        let b = run();
        for (name, x, y) in [
            ("composite", &a.composite, &b.composite),
            ("VIL", &a.vil, &b.vil),
            ("VIL density", &a.vild, &b.vild),
            ("echo top", &a.etop, &b.etop),
        ] {
            assert_eq!(
                x.time, f.source.acquisition_time,
                "derived time must remain the source clock"
            );
            assert_eq!((x.nx, x.ny), (y.nx, y.ny));
            assert!(
                x.values.iter().any(|v| v.is_finite()),
                "{id} {name}: no valid derived cells"
            );
            assert!(
                x.values
                    .iter()
                    .map(|v| v.to_bits())
                    .eq(y.values.iter().map(|v| v.to_bits())),
                "{id} {name}: repeatability including missing cells"
            );
        }
    }
}

/// The lowest tilt that carries velocity, dealiased.
fn lowest_velocity(scan: &level2::Scan) -> level2::BinnedSweep {
    lowest_velocity_tilt(scan).1
}

/// The lowest tilt that carries velocity, its index and its dealiased velocity.
fn lowest_velocity_tilt(scan: &level2::Scan) -> (usize, level2::BinnedSweep) {
    (0..level2::elevation_angles(scan).len())
        .find_map(|tilt| {
            level2::bin_scan_opts(scan, Moment::Velocity, tilt, true)
                .ok()
                .map(|v| (tilt, v))
        })
        .expect("a velocity tilt")
}

/// detectionplan.md Phase 3: rotation objects on the lowest tilt find each violent tornado as one
/// credible cyclonic object, and the clear-air control as none.
#[test]
#[ignore = "large cached fixtures: provision explicitly before running"]
fn cached_rotation_objects_find_tornadoes_not_clear_air() {
    use wxdata::azshear::{llsd, LlsdParams};
    use wxdata::rotation_objects::{objects, ObjectParams};
    let m = corpus::manifest();
    for (id, tornado) in [
        ("moore-2013", Some((-97.491, 35.332))),
        ("mayfield-2021", Some((-88.636, 36.742))),
        ("clear-air-2019", None),
    ] {
        let f = m.fixtures.iter().find(|f| f.id == id).unwrap();
        let scan = level2::decode_volume(corpus::read(f, &corpus::cache_dir()).unwrap()).unwrap();
        let (tilt, vel) = lowest_velocity_tilt(&scan);
        let z = level2::bin_scan(&scan, Moment::Reflectivity, tilt).unwrap();
        let t0 = std::time::Instant::now();
        let field = llsd(&vel, &LlsdParams::default());
        let objs = objects(&field, &vel, &z, &ObjectParams::default());
        let took = t0.elapsed();
        let credible: Vec<_> = objs.iter().filter(|o| o.credible()).collect();
        eprintln!(
            "{id}: {} objects, {} credible, field + objects {took:?}",
            objs.len(),
            credible.len()
        );
        let mut ranges: Vec<i32> = credible.iter().map(|o| o.range_km.round() as i32).collect();
        ranges.sort_unstable();
        eprintln!("   credible ranges (km): {ranges:?}");
        for o in credible.iter().take(6) {
            let d = tornado.map_or(f64::NAN, |t| ground_km((o.lon, o.lat), t));
            eprintln!(
                "   {:?} max {:.4} sig {:.1} {:.1} km², {:.1}x{:.1} km, dV {:.0} m/s, {:.1} km range, {d:.1} km from the tornado",
                o.sense, o.max_azshear, o.significance, o.area_km2, o.length_km, o.width_km,
                o.max_delta_v_ms, o.range_km
            );
        }
        if let Some(t) = tornado {
            for o in objs.iter().filter(|o| ground_km((o.lon, o.lat), t) <= 10.0) {
                eprintln!(
                    "   near: {:?} max {:.4} sig {:.1} {:.1} km², {:.1}x{:.1} km, {} gates, fold {:.2} x{:.2}, rmse {:.1}, tex {:.1} {:?}",
                    o.sense, o.max_azshear, o.significance, o.area_km2, o.length_km, o.width_km,
                    o.gates, o.fold_share, o.fold_crossings, o.fit_rmse_ms, o.mean_texture_ms, o.artifacts
                );
            }
        }
        match tornado {
            Some(t) => {
                let here: Vec<_> = credible
                    .iter()
                    .filter(|o| ground_km((o.lon, o.lat), t) <= 8.0)
                    .collect();
                // Twice the 0.006 s⁻¹ TORP builds objects from. Mayfield's single strongest
                // lowest-tilt gate (0.028 s⁻¹) is not in an object: it sits at the storm's edge
                // over 3-12 dBZ, with none of its kernel at 20 dBZ, so the storm-context rule
                // leaves it out. Its objects of 0.019 and 0.017 s⁻¹ beside it are found.
                assert!(
                    here.iter()
                        .any(|o| o.sense == wxdata::rotation::Sense::Cyclonic
                            && o.max_azshear >= 0.012),
                    "{id}: no strong credible cyclonic object at the tornado: {here:#?}"
                );
            }
            None => assert!(credible.is_empty(), "{id}: {credible:#?}"),
        }
    }
}

fn ground_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dlat = (b.1 - a.1) * 111.32;
    let dlon = (b.0 - a.0) * 111.32 * a.1.to_radians().cos();
    dlat.hypot(dlon)
}

/// detectionplan.md Phase 2: the LLSD AzShear field reads both violent tornadoes as rotation
/// clearly past the 0.006 s⁻¹ TORP builds its objects from, cyclonic, at the lowest tilt.
#[test]
#[ignore = "large cached fixtures: provision explicitly before running"]
fn cached_llsd_azshear_reads_violent_tornadoes() {
    use wxdata::azshear::{llsd, LlsdParams};
    let m = corpus::manifest();
    for (id, tornado) in [
        ("moore-2013", (-97.491, 35.332)),
        ("mayfield-2021", (-88.636, 36.742)),
    ] {
        let f = m.fixtures.iter().find(|f| f.id == id).unwrap();
        let scan = level2::decode_volume(corpus::read(f, &corpus::cache_dir()).unwrap()).unwrap();
        let vel = lowest_velocity(&scan);
        let t0 = std::time::Instant::now();
        let field = llsd(&vel, &LlsdParams::default());
        let took = t0.elapsed();
        let sign = field.cyclonic_sign();
        let (peak, at) = field
            .iter()
            .filter(|&(az, g, _)| ground_km(field.lonlat(az, g), tornado) <= 8.0)
            .map(|(az, g, s)| (s.shear_s * sign, (az, g, s)))
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .expect("shear near the tornado");
        eprintln!(
            "{id}: {:.1}° tilt, {} gates computed in {took:?}; peak cyclonic {peak:.4} s⁻¹ at {:.1} km, {:?}",
            vel.elevation_deg,
            field.iter().count(),
            field.range_km(at.1),
            at.2.quality
        );
        assert!(peak >= 0.01, "{id}: peak cyclonic shear {peak}");
    }
}

/// The clear-air control, for comparison: how much of a weak-echo field reaches the TORP
/// threshold before any object, reflectivity or quality screening (Phase 3's job).
#[test]
#[ignore = "large cached fixtures: provision explicitly before running"]
fn cached_llsd_azshear_clear_air_report() {
    use wxdata::azshear::{llsd, LlsdParams};
    let m = corpus::manifest();
    let f = m
        .fixtures
        .iter()
        .find(|f| f.id == "clear-air-2019")
        .unwrap();
    let scan = level2::decode_volume(corpus::read(f, &corpus::cache_dir()).unwrap()).unwrap();
    let field = llsd(&lowest_velocity(&scan), &LlsdParams::default());
    let all: Vec<_> = field.iter().collect();
    let strong = all
        .iter()
        .filter(|(_, _, s)| s.shear_s.abs() >= 0.006)
        .count();
    let peak = all
        .iter()
        .map(|(_, _, s)| s.shear_s.abs())
        .fold(0.0f32, f32::max);
    eprintln!(
        "clear-air-2019: {} gates, {strong} at |shear| >= 0.006 ({:.3}%), peak {peak:.4}",
        all.len(),
        100.0 * strong as f64 / all.len().max(1) as f64
    );
}

/// detectionplan.md Phase 4: each violent tornado is a rooted cyclonic column several tilts deep
/// with strong low-level shear, from objects on the lowest four velocity tilts (and, for its root,
/// objects found without the echo screen: Mayfield's lowest-tilt shear sits over 3-12 dBZ).
#[test]
#[ignore = "large cached fixtures: provision explicitly before running"]
fn cached_rotation_columns_root_the_violent_tornadoes() {
    use wxdata::azshear::{llsd, LlsdParams};
    use wxdata::rotation_columns::{columns_with_support, ColumnParams};
    use wxdata::rotation_objects::{objects, Artifact, ObjectParams};
    let m = corpus::manifest();
    for (id, tornado) in [
        ("moore-2013", (-97.491, 35.332)),
        ("mayfield-2021", (-88.636, 36.742)),
    ] {
        let f = m.fixtures.iter().find(|f| f.id == id).unwrap();
        let scan = level2::decode_volume(corpus::read(f, &corpus::cache_dir()).unwrap()).unwrap();
        let mut tilts = Vec::new();
        let mut support = Vec::new();
        let relaxed = ObjectParams {
            require_echo: false,
            ..ObjectParams::default()
        };
        for tilt in 0..level2::elevation_angles(&scan).len() {
            let (Ok(vel), Ok(z)) = (
                level2::bin_scan_opts(&scan, Moment::Velocity, tilt, true),
                level2::bin_scan(&scan, Moment::Reflectivity, tilt),
            ) else {
                continue;
            };
            let field = llsd(&vel, &LlsdParams::default());
            tilts.push(
                objects(&field, &vel, &z, &ObjectParams::default())
                    .into_iter()
                    .filter(|o| o.credible())
                    .collect::<Vec<_>>(),
            );
            support.push(
                objects(&field, &vel, &z, &relaxed)
                    .into_iter()
                    .filter(|o| o.artifacts.iter().all(|a| *a == Artifact::NoEcho))
                    .collect::<Vec<_>>(),
            );
            if tilts.len() == 4 {
                break;
            }
        }
        let cols = columns_with_support(&tilts, &support, &ColumnParams::default());
        let here: Vec<_> = cols
            .iter()
            .filter(|c| ground_km((c.lon, c.lat), tornado) <= 8.0)
            .collect();
        for c in &here {
            eprintln!(
                "{id}: {:?} {} tilts {:.1}-{:.1} km, rooted {} (weak echo {:?}), low {:?} mid {:?} max {:.4}, lean {:?} km/km",
                c.sense, c.tilts(), c.base_km, c.top_km, c.rooted,
                c.members.iter().map(|m| m.weak_echo).collect::<Vec<_>>(), c.low_level_azshear,
                c.mid_level_azshear, c.max_azshear, c.lean_km_per_km
            );
        }
        assert!(
            here.iter()
                .any(|c| c.sense == wxdata::rotation::Sense::Cyclonic
                    && c.rooted
                    && c.tilts() >= 2
                    && c.low_level_azshear.is_some_and(|s| s >= 0.012)),
            "{id}: no rooted multi-tilt cyclonic column at the tornado"
        );
    }
}

/// What the fused Tornado ID costs per volume against the legacy detectors, on the Moore volume
/// (release build for meaningful numbers): the app runs it on the UI thread for each new volume.
#[test]
#[ignore = "large cached fixtures: provision explicitly before running"]
fn cached_fused_pipeline_cost_against_legacy() {
    let m = corpus::manifest();
    let f = m.fixtures.iter().find(|f| f.id == "moore-2013").unwrap();
    let scan = level2::decode_volume(corpus::read(f, &corpus::cache_dir()).unwrap()).unwrap();
    let (mut vel_pairs, mut cc_pairs) = (Vec::new(), Vec::new());
    for tilt in 0..level2::elevation_angles(&scan).len() {
        let z = level2::bin_scan(&scan, Moment::Reflectivity, tilt);
        if let (Ok(z), Ok(cc)) = (
            &z,
            level2::bin_scan(&scan, Moment::CorrelationCoefficient, tilt),
        ) {
            cc_pairs.push((z.clone(), cc));
        }
        if let (Ok(z), Ok(v)) = (
            z,
            level2::bin_scan_opts(&scan, Moment::Velocity, tilt, true),
        ) {
            vel_pairs.push((v, z));
        }
        if vel_pairs.len() == 4 {
            break;
        }
    }
    let t0 = std::time::Instant::now();
    let couplets = wxdata::rotation::detect_volume(&vel_pairs, 25.0, 20.0, 15.0, 150.0, 3);
    let legacy_rot = t0.elapsed();
    let t1 = std::time::Instant::now();
    let debris = wxdata::tds::detect_volume(&cc_pairs, 0.80, 40.0, 150.0, 4);
    let legacy_tds = t1.elapsed();
    let t2 = std::time::Instant::now();
    let columns = wxdata::rotation_columns::from_sweeps(&vel_pairs);
    let fused_cols = t2.elapsed();
    let t3 = std::time::Instant::now();
    let mut tracker =
        wxdata::rotation_tracks::Tracker::new(wxdata::rotation_tracks::TrackParams::default());
    let analysed = wxdata::llsd_analyst::analyse(tracker.update(0, columns), &debris, &[]);
    let fused_rest = t3.elapsed();
    let t4 = std::time::Instant::now();
    let one = wxdata::rotation_columns::from_sweeps(&vel_pairs[..1]);
    let one_tilt = t4.elapsed();
    eprintln!(
        "legacy couplets {legacy_rot:?} ({} hits), legacy debris {legacy_tds:?}; fused columns {fused_cols:?} \
         + track/classify/fuse {fused_rest:?} ({} analysed); one tilt alone {one_tilt:?} ({} columns)",
        couplets.len(),
        analysed.len(),
        one.len()
    );
}

/// Diagnostic for detectionplan.md round three (`--ignored --nocapture`): the echo the strongest
/// rooted cyclonic columns sit in (`storm_mode`, lowest tilt, as `from_sweeps` measures it), and
/// the same at other reflectivity thresholds and search radii. Recorded rather than asserted: on
/// these storms connected-echo aspect does not separate a line from a cell (Moore reads 5.4 at
/// 40 dBZ; the derecho's echo is one 140 km blob of aspect 2).
#[test]
#[ignore = "diagnostic; large cached fixtures: provision explicitly before running"]
fn cached_echo_shape_tells_a_line_from_a_cell() {
    let m = corpus::manifest();
    let mut aspect_at = std::collections::HashMap::new();
    for id in [
        "moore-2013",
        "mayfield-2021",
        "iowa-qlcs-2021",
        "derecho-2020",
        "denver-hail-2017",
    ] {
        let f = m.fixtures.iter().find(|f| f.id == id).unwrap();
        let scan = level2::decode_volume(corpus::read(f, &corpus::cache_dir()).unwrap()).unwrap();
        let mut pairs = Vec::new();
        for tilt in 0..level2::elevation_angles(&scan).len() {
            if let (Ok(v), Ok(z)) = (
                level2::bin_scan_opts(&scan, Moment::Velocity, tilt, true),
                level2::bin_scan(&scan, Moment::Reflectivity, tilt),
            ) {
                pairs.push((v, z));
            }
            if pairs.len() == 4 {
                break;
            }
        }
        let mut cols: Vec<_> = wxdata::rotation_columns::from_sweeps(&pairs)
            .into_iter()
            .filter(|c| c.rooted && c.sense == wxdata::rotation::Sense::Cyclonic)
            .collect();
        cols.sort_by(|a, b| b.max_azshear.total_cmp(&a.max_azshear));
        for c in cols.iter().take(4) {
            eprintln!(
                "{id}: column {:.3},{:.3} max {:.4} s-1, {} tilts: echo {:?} aspect {:?}",
                c.lon,
                c.lat,
                c.max_azshear,
                c.tilts(),
                c.echo,
                c.echo.map(|e| e.aspect())
            );
        }
        aspect_at.insert(id, cols.first().and_then(|c| c.echo).map(|e| e.aspect()));
        let t0 = std::time::Instant::now();
        let labelled =
            wxdata::storm_mode::EchoObjects::label(&pairs[0].1, wxdata::storm_mode::CORE_DBZ);
        eprintln!(
            "{id}: labelled {} objects in {:?}",
            labelled.len(),
            t0.elapsed()
        );
        // Other thresholds and search radii, for the strongest two columns.
        for dbz in [30.0, 35.0, 45.0, 50.0] {
            let objs = wxdata::storm_mode::EchoObjects::label(&pairs[0].1, dbz);
            for search in [5.0, 10.0] {
                let shapes: Vec<String> = cols
                    .iter()
                    .take(2)
                    .map(|c| match objs.shape_at(c.lon, c.lat, search) {
                        Some(e) => format!(
                            "{:.0}x{:.0} km ({:.1})",
                            e.length_km,
                            e.width_km,
                            e.aspect()
                        ),
                        None => "none".into(),
                    })
                    .collect();
                eprintln!("{id}: {dbz} dBZ within {search} km: {shapes:?}");
            }
        }
    }
    eprintln!("{aspect_at:?}");
}

/// detectionplan.md "Performance constraints": each stage of the fused pipeline timed on its own,
/// on the pinned Moore volume's lowest four velocity tilts (median of 5 runs). Run in release
/// with `--ignored --nocapture`; the numbers are recorded in the plan with the hardware.
#[test]
#[ignore = "large cached fixtures: provision explicitly before running (release build)"]
fn cached_fused_pipeline_stage_timings() {
    use std::time::{Duration, Instant};
    use wxdata::azshear::{llsd, LlsdParams};
    use wxdata::rotation_columns::{columns_with_support, ColumnParams};
    use wxdata::rotation_objects::{objects, Artifact, ObjectParams};
    let m = corpus::manifest();
    let f = m.fixtures.iter().find(|f| f.id == "moore-2013").unwrap();
    let scan = level2::decode_volume(corpus::read(f, &corpus::cache_dir()).unwrap()).unwrap();
    let (mut pairs, mut cc_pairs) = (Vec::new(), Vec::new());
    for tilt in 0..level2::elevation_angles(&scan).len() {
        let z = level2::bin_scan(&scan, Moment::Reflectivity, tilt);
        if let (Ok(z), Ok(cc)) = (
            &z,
            level2::bin_scan(&scan, Moment::CorrelationCoefficient, tilt),
        ) {
            cc_pairs.push((z.clone(), cc));
        }
        if let (Ok(z), Ok(v)) = (
            z,
            level2::bin_scan_opts(&scan, Moment::Velocity, tilt, true),
        ) {
            pairs.push((v, z));
        }
        if pairs.len() == 4 {
            break;
        }
    }
    let median = |mut d: Vec<Duration>| {
        d.sort();
        d[d.len() / 2]
    };
    let time = |f: &mut dyn FnMut()| {
        median(
            (0..5)
                .map(|_| {
                    let t = Instant::now();
                    f();
                    t.elapsed()
                })
                .collect(),
        )
    };
    let p = LlsdParams::default();
    let fields: Vec<_> = pairs.iter().map(|(v, _)| llsd(v, &p)).collect();
    let field_t = time(&mut || {
        for (v, _) in &pairs {
            std::hint::black_box(llsd(v, &p));
        }
    });
    let relaxed = ObjectParams {
        require_echo: false,
        ..ObjectParams::default()
    };
    let mut tilts = Vec::new();
    let mut support = Vec::new();
    let objects_t = time(&mut || {
        tilts.clear();
        support.clear();
        for (fld, (v, z)) in fields.iter().zip(&pairs) {
            tilts.push(
                objects(fld, v, z, &ObjectParams::default())
                    .into_iter()
                    .filter(|o| o.credible())
                    .collect::<Vec<_>>(),
            );
            support.push(
                objects(fld, v, z, &relaxed)
                    .into_iter()
                    .filter(|o| o.artifacts.iter().all(|a| *a == Artifact::NoEcho))
                    .collect::<Vec<_>>(),
            );
        }
    });
    let mut columns = Vec::new();
    let columns_t = time(&mut || {
        columns = columns_with_support(&tilts, &support, &ColumnParams::default());
    });
    let echo_t = time(&mut || {
        let e = wxdata::storm_mode::EchoObjects::label(&pairs[0].1, wxdata::storm_mode::CORE_DBZ);
        for c in &columns {
            std::hint::black_box(e.shape_at(c.lon, c.lat, wxdata::storm_mode::SEARCH_KM));
            std::hint::black_box(wxdata::near_flow::near_flow(
                &pairs[0].0,
                c.lon,
                c.lat,
                wxdata::near_flow::RADIUS_KM,
            ));
        }
    });
    let debris = wxdata::tds::detect_volume(&cc_pairs, 0.80, 40.0, 150.0, 4);
    let track_t = time(&mut || {
        let mut tracker =
            wxdata::rotation_tracks::Tracker::new(wxdata::rotation_tracks::TrackParams::default());
        std::hint::black_box(tracker.update(0, columns.clone()));
    });
    let mut tracker =
        wxdata::rotation_tracks::Tracker::new(wxdata::rotation_tracks::TrackParams::default());
    let tracked = tracker.update(0, columns.clone());
    let fuse_t = time(&mut || {
        std::hint::black_box(wxdata::llsd_analyst::analyse(tracked.clone(), &debris, &[]));
    });
    let whole_t = time(&mut || {
        std::hint::black_box(wxdata::rotation_columns::from_sweeps(&pairs));
    });
    eprintln!(
        "Moore 2013, 4 tilts, {} columns, {} debris signatures (median of 5):\n  \
         LLSD field {field_t:?} ({:?} per tilt)\n  objects (credible + weak-echo support) {objects_t:?}\n  \
         vertical association {columns_t:?}\n  echo shape + near flow {echo_t:?}\n  \
         tracking {track_t:?}\n  debris classification + fusion {fuse_t:?}\n  \
         from_sweeps end to end {whole_t:?}",
        columns.len(),
        debris.len(),
        field_t / pairs.len() as u32
    );
}

/// One marker per tornado on the pinned Mayfield 2021 volume: every fused verdict within 15 km of
/// the tornado folds into one circulation (it was 2-5 markers on every scan, with the
/// rotation-only Possible bar adding more). Fast: one cached volume, release build.
#[test]
#[ignore = "large cached fixtures: provision explicitly before running"]
fn cached_mayfield_is_one_tornado_marker() {
    let m = corpus::manifest();
    let f = m.fixtures.iter().find(|f| f.id == "mayfield-2021").unwrap();
    let scan = level2::decode_volume(corpus::read(f, &corpus::cache_dir()).unwrap()).unwrap();
    let (mut pairs, mut cc_pairs) = (Vec::new(), Vec::new());
    for tilt in 0..level2::elevation_angles(&scan).len() {
        let z = level2::bin_scan(&scan, Moment::Reflectivity, tilt);
        if let (Ok(z), Ok(cc)) = (
            &z,
            level2::bin_scan(&scan, Moment::CorrelationCoefficient, tilt),
        ) {
            cc_pairs.push((z.clone(), cc));
        }
        if let (Ok(z), Ok(v)) = (
            z,
            level2::bin_scan_opts(&scan, Moment::Velocity, tilt, true),
        ) {
            pairs.push((v, z));
        }
        if pairs.len() == 4 {
            break;
        }
    }
    let columns = wxdata::rotation_columns::from_sweeps(&pairs);
    let tracked = wxdata::rotation_tracks::Tracker::new(Default::default()).update(0, columns);
    let debris = wxdata::tds::detect_volume(&cc_pairs, 0.80, 40.0, 150.0, 4);
    let analysed = wxdata::llsd_analyst::analyse(tracked, &debris, &[]);
    let tornado = (-88.636, 36.742);
    let verdicts = wxdata::llsd_analyst::identify_with(
        &analysed,
        |_, _| Default::default(),
        wxdata::llsd_analyst::VerdictOptions::bar(Some(0.018)),
    );
    let circs = wxdata::llsd_analyst::circulations_with(
        &analysed,
        &[],
        &[],
        |_, _| Default::default(),
        wxdata::llsd_analyst::VerdictOptions::bar(Some(0.018)),
    );
    let near = |lon: f64, lat: f64| ground_km((lon, lat), tornado) <= 15.0;
    let before = verdicts.iter().filter(|t| near(t.lon, t.lat)).count();
    let after = circs.iter().filter(|c| near(c.id.lon, c.id.lat)).count();
    eprintln!(
        "Mayfield: {before} verdicts within 15 km of the tornado, {after} marker(s) after merging"
    );
    assert!(before >= 1, "the tornado is detected");
    assert_eq!(after, 1, "one tornado, one marker");
}

/// Great-circle distance (km) and initial bearing (deg) from the radar, written here rather than
/// borrowed from the crate so the column check below does not share its geometry with the code
/// under test.
fn range_bearing(lat0: f64, lon0: f64, lat: f64, lon: f64) -> (f64, f64) {
    let (p0, p1) = (lat0.to_radians(), lat.to_radians());
    let dl = (lon - lon0).to_radians();
    let c = (p0.sin() * p1.sin() + p0.cos() * p1.cos() * dl.cos()).clamp(-1.0, 1.0);
    let y = dl.sin() * p1.cos();
    let x = p0.cos() * p1.sin() - p0.sin() * p1.cos() * dl.cos();
    (6371.0 * c.acos(), y.atan2(x).to_degrees().rem_euclid(360.0))
}

/// The value of `s` over ground range `r` / azimuth `az`, by searching for the gate whose ground
/// range is nearest rather than inverting the beam geometry, and that beam's height (m above the
/// radar) over the point.
fn sample_over(s: &level2::BinnedSweep, r: f64, az: f64) -> Option<(Option<f32>, f64)> {
    let e = s.elevation_deg as f64;
    let (mut best, mut best_d) = (0usize, f64::MAX);
    for g in 0..s.gate_count {
        let slant = s.first_gate_km as f64 + g as f64 * s.gate_interval_km as f64;
        let d = (wxdata::xsection::ground_from_slant_km(slant, e) - r).abs();
        if d < best_d {
            (best, best_d) = (g, d);
        }
    }
    if best_d > s.gate_interval_km as f64 * 0.75 {
        return None;
    }
    let bin = (az / 360.0 * s.az_bins as f64).floor() as usize % s.az_bins;
    let code = s.data[bin * s.gate_count + best];
    let v = (code >= 2)
        .then(|| s.value_min + (code as f32 - 2.0) / 253.0 * (s.value_max - s.value_min));
    let h = wxdata::xsection::beam_height_km(wxdata::xsection::slant_from_ground_km(r, e), e);
    Some((v, h * 1000.0))
}

/// ROADMAP_PARITY M3.3: column user products on real volumes. `max_vertical(REF)` must be the local
/// composite cell for cell; a masked CC minimum and a ZDR-above-a-height maximum must match an
/// independent column calculation within one gate's sampling ambiguity.
#[test]
#[ignore = "large cached fixtures: provision explicitly before running"]
fn cached_column_products_match_independent_columns() {
    use wxdata::udp_column::{evaluate_grid, ColumnEnv, ColumnTilt, Levels};
    for id in ["denver-hail-2017", "mayfield-2021"] {
        let m = corpus::manifest();
        let f = m.fixtures.iter().find(|f| f.id == id).unwrap();
        let scan = level2::decode_volume(corpus::read(f, &corpus::cache_dir()).unwrap()).unwrap();
        // Every distinct tilt, cropped to 60 km as the derived-product check does: original gate
        // values and missing rows, nothing interpolated.
        let crop = |mut s: level2::BinnedSweep| {
            let gates = s.gate_count.min(240);
            s.data = s
                .data
                .chunks_exact(s.gate_count)
                .flat_map(|row| row[..gates].iter().copied())
                .collect();
            s.gate_count = gates;
            s
        };
        let tilts: Vec<ColumnTilt> = (0..level2::elevation_angles(&scan).len())
            .map(|t| {
                let get = |m| level2::bin_scan(&scan, m, t).ok().map(crop);
                [
                    get(Moment::Reflectivity),
                    None,
                    None,
                    get(Moment::DifferentialReflectivity),
                    None,
                    get(Moment::CorrelationCoefficient),
                ]
            })
            .filter(|t| t.iter().any(Option::is_some))
            .collect();
        assert!(tilts.len() >= 8, "{id}: {} tilts", tilts.len());
        let time = f.source.acquisition_time;
        let grid = |src: &str, env: &ColumnEnv| {
            evaluate_grid(&wxdata::udp::parse(src).unwrap(), &tilts, env, time)
                .unwrap_or_else(|e| panic!("{id} {src}: {e}"))
        };

        let composite = grid("max_vertical(REF)", &ColumnEnv::default());
        assert_eq!(composite.field.time, time, "the source clock, not now");
        let refl: Vec<_> = tilts.iter().filter_map(|t| t[0].clone()).collect();
        let derived = wxdata::derived::derive(
            &refl,
            &wxdata::derived::DerivedOpts {
                time,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            composite
                .field
                .values
                .iter()
                .map(|v| v.to_bits())
                .eq(derived.composite.values.iter().map(|v| v.to_bits())),
            "{id}: max_vertical(REF) differs from the local composite"
        );
        assert!(composite.cells_with_value > 1000, "{id}");

        // A height stated as a test parameter, not an observed melting level.
        let antenna = 1_700.0f32;
        let h0 = 3_600.0f32;
        let env = ColumnEnv {
            antenna_altitude_m: Some(antenna),
            levels: Levels {
                h0_m: Some(h0),
                ..Default::default()
            },
        };
        let cc = grid("min_vertical(CC, REF >= 45)", &ColumnEnv::default());
        let zdr = grid(
            "max_vertical(ZDR, BEAM_ALTITUDE_M > FREEZING_LEVEL_M)",
            &env,
        );
        let composable = [
            "min_vertical(REF, REF >= max_vertical(REF) - 5)",
            "mean_vertical(REF)",
            "fraction_vertical(REF >= 40)",
            "max_height(REF)",
        ]
        .map(|src| grid(src, &ColumnEnv::default()));
        let mut composable_bad = [0usize; 4];
        // Real archived Denver profile selected at/before this scan, using recorded MSL HGHT.
        // The independent layer bounds below are from the pinned stdlib Python reader.
        let cold_layer = (id == "denver-hail-2017").then(|| {
            let station = *wxdata::raob::STATIONS.iter().find(|s| s.id == "72469").unwrap();
            let launch = "2017-05-08T12:00:00Z".parse().unwrap();
            assert_eq!(wxdata::raob::synoptic_before(time), launch);
            let reading = wxdata::raob::EnvironmentalLevels::from_table(station, launch, include_str!(
                "../../../docs/certification/m3.3/recorded-environment/72469-2017050812.txt"
            ));
            let site = wxdata::sites::site_by_id("KFTG").unwrap();
            let antenna = site.elevation_meters as f32 + wxdata::towers::tower_m(site.id) as f32;
            let env = ColumnEnv { antenna_altitude_m: Some(antenna), levels: Levels {
                hm30_m: reading.isotherms[3].height_m.map(|h| h as f32),
                hm40_m: reading.isotherms[4].height_m.map(|h| h as f32),
                ..Default::default()
            }};
            eprintln!("{id}: {}", reading.describe());
            (antenna, [
                grid("max_vertical(REF, BEAM_ALTITUDE_M >= MINUS30C_HEIGHT_M && BEAM_ALTITUDE_M <= MINUS40C_HEIGHT_M)", &env),
                grid("mean_layer(REF, MINUS30C_HEIGHT_M - (BEAM_ALTITUDE_M - BEAM_HEIGHT_M), MINUS40C_HEIGHT_M - (BEAM_ALTITUDE_M - BEAM_HEIGHT_M))", &env),
            ])
        });
        let mut cold_bad = [0usize; 2];
        let mut cold_hits = 0usize;
        let (lat0, lon0) = (refl[0].radar_lat as f64, refl[0].radar_lon as f64);
        let fld = &cc.field;
        let (dlon, dlat) = (
            (fld.lon_east - fld.lon_west) / fld.nx as f64,
            (fld.lat_north - fld.lat_south) / fld.ny as f64,
        );
        let (mut checked, mut cc_bad, mut zdr_bad, mut cc_hits, mut zdr_hits) = (0, 0, 0, 0, 0);
        for gy in (0..fld.ny).step_by(3) {
            for gx in (0..fld.nx).step_by(3) {
                let (lat, lon) = (
                    fld.lat_north - (gy as f64 + 0.5) * dlat,
                    fld.lon_west + (gx as f64 + 0.5) * dlon,
                );
                let (r, az) = range_bearing(lat0, lon0, lat, lon);
                if !(5.0..55.0).contains(&r) {
                    continue;
                }
                let (mut cc_min, mut zdr_max): (Option<f32>, Option<f32>) = (None, None);
                let mut ref_column = Vec::new();
                for t in &tilts {
                    let z = t[0].as_ref().and_then(|s| sample_over(s, r, az));
                    if let Some((Some(v), h)) = z {
                        ref_column.push((v, h));
                    }
                    let c = t[5].as_ref().and_then(|s| sample_over(s, r, az));
                    if let (Some((Some(z), _)), Some((Some(c), _))) = (z, c) {
                        if z >= 45.0 {
                            cc_min = Some(cc_min.map_or(c, |m| m.min(c)));
                        }
                    }
                    let base = t[0].as_ref().or(t[3].as_ref()).unwrap();
                    let e = base.elevation_deg as f64;
                    let h = wxdata::xsection::beam_height_km(
                        wxdata::xsection::slant_from_ground_km(r, e),
                        e,
                    ) * 1000.0;
                    if let Some((Some(v), _)) = t[3].as_ref().and_then(|s| sample_over(s, r, az)) {
                        if antenna + h as f32 > h0 {
                            zdr_max = Some(zdr_max.map_or(v, |m| m.max(v)));
                        }
                    }
                }
                let i = gy * fld.nx + gx;
                let agree = |want: Option<f32>, got: f32| match want {
                    Some(w) => (got - w).abs() < 1e-4,
                    None => got.is_nan(),
                };
                checked += 1;
                cc_hits += usize::from(cc_min.is_some());
                zdr_hits += usize::from(zdr_max.is_some());
                cc_bad += usize::from(!agree(cc_min, cc.field.values[i]));
                zdr_bad += usize::from(!agree(zdr_max, zdr.field.values[i]));
                // Direct arithmetic on independently sampled gates; no DSL evaluation here.
                let peak = ref_column.iter().map(|(v, _)| *v).max_by(f32::total_cmp);
                let want = peak.map(|peak| {
                    let band_min = ref_column
                        .iter()
                        .map(|(v, _)| *v)
                        .filter(|v| *v >= peak - 5.0)
                        .min_by(f32::total_cmp)
                        .unwrap();
                    let mean = ref_column.iter().map(|(v, _)| f64::from(*v)).sum::<f64>()
                        / ref_column.len() as f64;
                    let fraction = ref_column.iter().filter(|(v, _)| *v >= 40.0).count() as f32
                        / ref_column.len() as f32;
                    let height = ref_column
                        .iter()
                        .filter(|(v, _)| *v == peak)
                        .map(|(_, h)| *h)
                        .min_by(f64::total_cmp)
                        .unwrap();
                    [band_min, mean as f32, fraction, height as f32]
                });
                for (k, product) in composable.iter().enumerate() {
                    let got = product.field.values[i];
                    let matches = want.map_or(got.is_nan(), |w| {
                        (got - w[k]).abs() < if k == 3 { 0.5 } else { 1e-4 }
                    });
                    composable_bad[k] += usize::from(!matches);
                }
                if let Some((antenna, products)) = &cold_layer {
                    let lo = 7922.473282442748_f64 as f32;
                    let hi = 9085.832061068702_f64 as f32;
                    let values: Vec<f32> = ref_column
                        .iter()
                        .filter(|(_, h)| {
                            let altitude = *antenna + *h as f32;
                            altitude >= lo && altitude <= hi
                        })
                        .map(|(v, _)| *v)
                        .collect();
                    cold_hits += usize::from(!values.is_empty());
                    let wanted = (!values.is_empty()).then(|| {
                        [
                            values.iter().copied().max_by(f32::total_cmp).unwrap(),
                            (values.iter().map(|v| f64::from(*v)).sum::<f64>()
                                / values.len() as f64) as f32,
                        ]
                    });
                    for (k, product) in products.iter().enumerate() {
                        cold_bad[k] +=
                            usize::from(!agree(wanted.map(|w| w[k]), product.field.values[i]));
                    }
                }
            }
        }
        eprintln!(
            "{id}: {checked} cells, CC-min {cc_hits} with a core level ({cc_bad} differ), \
             ZDR-above {zdr_hits} with a level above ({zdr_bad} differ); \
             nested-band/mean/fraction/peak-height mismatches {composable_bad:?}"
        );
        assert!(
            checked > 800 && cc_hits > 20 && zdr_hits > 200,
            "{id}: too few cells"
        );
        // A cell whose point sits on a gate boundary can resolve to the neighbouring gate under
        // the two (equivalent) nearest-gate rules; nothing else may differ.
        assert!(cc_bad * 100 <= checked, "{id}: {cc_bad} CC cells differ");
        assert!(zdr_bad * 100 <= checked, "{id}: {zdr_bad} ZDR cells differ");
        for (k, bad) in composable_bad.into_iter().enumerate() {
            assert!(
                bad * 100 <= checked,
                "{id}: composable product {k}: {bad}/{checked} cells differ"
            );
        }
        if cold_layer.is_some() {
            eprintln!("{id}: recorded -30/-40 C layer: {checked} sampled cells, {cold_hits} populated, masked-max/layer-mean mismatches {cold_bad:?}");
            assert!(cold_hits > 20, "insufficient sampled cold-layer echo");
            for bad in cold_bad {
                assert!(
                    bad * 100 <= checked,
                    "{bad}/{checked} cold-layer cells differ"
                );
            }
        }
    }
}

/// Cost of a column product over a whole real volume (every tilt, full range), printed for the
/// evidence ledger. No budget is asserted: CI machines differ, and this is not a device claim.
#[test]
#[ignore = "large cached fixtures: provision explicitly before running"]
fn cached_column_product_full_volume_cost() {
    use wxdata::udp_column::{evaluate_grid, ColumnEnv, ColumnTilt};
    let m = corpus::manifest();
    let f = m.fixtures.iter().find(|f| f.id == "mayfield-2021").unwrap();
    let scan = level2::decode_volume(corpus::read(f, &corpus::cache_dir()).unwrap()).unwrap();
    let tilts: Vec<ColumnTilt> = (0..level2::elevation_angles(&scan).len())
        .map(|t| {
            let get = |m| level2::bin_scan(&scan, m, t).ok();
            [
                get(Moment::Reflectivity),
                None,
                None,
                get(Moment::DifferentialReflectivity),
                None,
                get(Moment::CorrelationCoefficient),
            ]
        })
        .collect();
    for src in [
        "max_vertical(REF)",
        "min_vertical(CC, REF >= 45)",
        "first_height_above(REF, 50)",
        "max_layer(ZDR, 3000, 6000) * (count_above(REF, 40) >= 3)",
    ] {
        let t0 = std::time::Instant::now();
        let p = evaluate_grid(
            &wxdata::udp::parse(src).unwrap(),
            &tilts,
            &ColumnEnv::default(),
            f.source.acquisition_time,
        )
        .unwrap();
        eprintln!(
            "{src}: {}x{} cells, {} tilts, {} with value, {:.0} ms",
            p.field.nx,
            p.field.ny,
            p.tilts,
            p.cells_with_value,
            t0.elapsed().as_secs_f64() * 1000.0
        );
    }
}

/// Every Message 31 radial data block in `bytes`, read straight from the ICD layout rather than
/// through the decoder: block id "RRAD", then LRTUP, unambiguous range (0.1 km), two noise levels
/// (4 bytes each) and Nyquist velocity (0.01 m/s), big-endian. Returns `(nyquist, range)` raw.
fn radial_blocks_by_hand(bytes: Vec<u8>) -> Vec<(u16, u16)> {
    let file = nexrad_data::volume::File::new(bytes);
    let file = if file.compressed() {
        file.decompress().unwrap()
    } else {
        file
    };
    let mut out = Vec::new();
    for record in file.records().unwrap() {
        let record = if record.compressed() {
            record.decompress().unwrap()
        } else {
            nexrad_data::volume::Record::new(record.data().to_vec())
        };
        let data = record.data();
        let mut i = 0;
        while i + 18 <= data.len() {
            if &data[i..i + 4] == b"RRAD" {
                let be = |at: usize| u16::from_be_bytes([data[i + at], data[i + at + 1]]);
                let lrtup = be(4);
                if (20..=40).contains(&lrtup) {
                    out.push((be(16), be(6)));
                    i += lrtup as usize;
                    continue;
                }
            }
            i += 1;
        }
    }
    out
}

/// M3.1: the Nyquist velocity and unambiguous range each radial was collected with are decoded
/// from its own radial block, not estimated, and every binned row carries its writer's values.
#[test]
fn decoded_doppler_metadata_matches_an_independent_read_of_the_radial_blocks() {
    let m = corpus::manifest();
    for f in m.fixtures.iter().filter(|f| f.tier == "offline") {
        let bytes = corpus::read(f, &corpus::cache_dir()).unwrap();
        let mut by_hand = radial_blocks_by_hand(bytes.clone());
        let scan = level2::decode_volume(bytes).unwrap();
        let mut decoded: Vec<(u16, u16)> = scan
            .sweeps()
            .iter()
            .flat_map(|s| s.radials())
            .filter_map(|r| {
                Some((
                    (r.nyquist_velocity_mps()? * 100.0).round() as u16,
                    (r.unambiguous_range_km()? * 10.0).round() as u16,
                ))
            })
            .collect();
        let radials: usize = scan.sweeps().iter().map(|s| s.radials().len()).sum();
        assert!(
            !by_hand.is_empty(),
            "{}: no radial blocks found by hand",
            f.id
        );
        by_hand.retain(|(n, r)| *n > 0 && *r > 0);
        by_hand.sort_unstable();
        decoded.sort_unstable();
        assert_eq!(
            decoded.len(),
            radials,
            "{}: a radial lost its metadata",
            f.id
        );
        assert_eq!(
            decoded, by_hand,
            "{}: decoded values differ from the blocks",
            f.id
        );
        // Physically plausible WSR-88D values, read with the ICD's scales.
        for (n, r) in &decoded {
            let (n, r) = (f32::from(*n) * 0.01, f32::from(*r) * 0.1);
            assert!((5.0..=50.0).contains(&n), "{}: Nyquist {n} m/s", f.id);
            assert!(
                (80.0..=520.0).contains(&r),
                "{}: unambiguous range {r} km",
                f.id
            );
        }
        // Binned rows carry the writer's decoded values, unknown elsewhere.
        // The partial files keep only the first (surveillance) records, whose radials carry a
        // Nyquist too; the full volumes' velocity rows are checked in the cached test below.
        let v = [Moment::Velocity, Moment::Reflectivity]
            .into_iter()
            .find_map(|m| level2::bin_scan(&scan, m, 0).ok())
            .unwrap_or_else(|| panic!("{}: nothing to bin", f.id));
        {
            assert_eq!(v.row_nyquist_mps.len(), v.az_bins, "{}", f.id);
            let known: Vec<f32> = v
                .row_nyquist_mps
                .iter()
                .copied()
                .filter(|x| x.is_finite())
                .collect();
            assert!(!known.is_empty(), "{}: no row with a Nyquist", f.id);
            let allowed: Vec<f32> = decoded.iter().map(|(n, _)| f32::from(*n) * 0.01).collect();
            assert!(
                known
                    .iter()
                    .all(|k| allowed.iter().any(|a| (a - k).abs() < 1e-3)),
                "{}: a row's Nyquist is not one its radials carried",
                f.id
            );
            eprintln!(
                "{}: {} radials, Nyquist {:?} m/s on the lowest tilt",
                f.id,
                radials,
                {
                    let mut u: Vec<i32> =
                        known.iter().map(|k| (k * 100.0).round() as i32).collect();
                    u.sort_unstable();
                    u.dedup();
                    u.iter().map(|x| *x as f32 / 100.0).collect::<Vec<_>>()
                }
            );
        }
    }
}

/// The same on full volumes' velocity: every lowest-velocity-tilt row carries the Nyquist of the
/// radial that wrote it, and the decoded values are the ones the radial blocks hold.
#[test]
#[ignore = "large cached fixtures: provision explicitly before running"]
fn cached_velocity_rows_carry_their_decoded_nyquist() {
    let m = corpus::manifest();
    for id in ["mayfield-2021", "denver-hail-2017", "moore-2013"] {
        let f = m.fixtures.iter().find(|f| f.id == id).unwrap();
        let bytes = corpus::read(f, &corpus::cache_dir()).unwrap();
        let mut by_hand = radial_blocks_by_hand(bytes.clone());
        by_hand.retain(|(n, r)| *n > 0 && *r > 0);
        let scan = level2::decode_volume(bytes).unwrap();
        let (t, v) = (0..level2::elevation_angles(&scan).len())
            .find_map(|t| Some((t, level2::bin_scan(&scan, Moment::Velocity, t).ok()?)))
            .unwrap_or_else(|| panic!("{id}: no velocity tilt"));
        let known: Vec<f32> = v
            .row_nyquist_mps
            .iter()
            .copied()
            .filter(|x| x.is_finite())
            .collect();
        let filled = v
            .data
            .chunks(v.gate_count)
            .filter(|r| r.iter().any(|&c| c >= 2))
            .count();
        assert!(
            known.len() >= filled,
            "{id}: {} rows with data, {} with a Nyquist",
            filled,
            known.len()
        );
        let allowed: std::collections::BTreeSet<u16> = by_hand.iter().map(|(n, _)| *n).collect();
        let mut seen: Vec<u16> = known.iter().map(|k| (k * 100.0).round() as u16).collect();
        seen.sort_unstable();
        seen.dedup();
        assert!(
            seen.iter().all(|n| allowed.contains(n)),
            "{id}: {seen:?} not among the blocks"
        );
        eprintln!(
            "{id}: velocity tilt {t} ({:.1}°): {} rows, Nyquist {:?} m/s; estimate from values {:?}",
            v.elevation_deg,
            known.len(),
            seen.iter().map(|n| f32::from(*n) / 100.0).collect::<Vec<_>>(),
            v.estimated_nyquist_mps()
        );
    }
}
