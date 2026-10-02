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
