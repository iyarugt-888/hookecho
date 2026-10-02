//! Source-labeled AP/ground-clutter control. HCA labels are operational classification,
//! not an absolute non-tornado ground-truth mask; preserve that limitation with the input.
use wxdata::level2::{self, Moment};

#[path = "support/corpus.rs"]
pub mod corpus;

/// Query the original polar categorical mask; do not interpolate neighboring classes.
fn class_at(p: &nexrad_level3::Level3Product, spacing_km: f64, lon: f64, lat: f64) -> Option<u8> {
    let ra = p.radial.as_ref()?;
    let (phi1, phi2) = (f64::from(p.lat).to_radians(), lat.to_radians());
    let dlambda = (lon - f64::from(p.lon)).to_radians();
    let h = ((phi2 - phi1) * 0.5).sin().powi(2)
        + phi1.cos() * phi2.cos() * (dlambda * 0.5).sin().powi(2);
    let ground =
        2.0 * wxdata::beam_geometry::EARTH_RADIUS_M / 1_000.0 * h.clamp(0.0, 1.0).sqrt().asin();
    let azimuth = (dlambda.sin() * phi2.cos())
        .atan2(phi1.cos() * phi2.sin() - phi1.sin() * phi2.cos() * dlambda.cos())
        .to_degrees()
        .rem_euclid(360.0);
    let slant = wxdata::xsection::slant_from_ground_km(ground, f64::from(p.elevation_deg?));
    let gate = (slant / spacing_km).floor() as i64 - i64::from(ra.first_bin);
    let gate = usize::try_from(gate).ok()?;
    ra.radials
        .iter()
        .find(|r| (azimuth - f64::from(r.start_deg)).rem_euclid(360.0) < f64::from(r.delta_deg))?
        .levels
        .get(gate)
        .copied()
        .filter(|&code| code != 0)
}

#[test]
fn pinned_classification_retains_source_clocks_and_labels() {
    let m = corpus::manifest();
    assert_eq!(m.classification_snapshots.len(), 1);
    let f = &m.classification_snapshots[0];
    let p = corpus::read_classification(f).unwrap();
    assert_eq!(p.times.data_start_unix, Some(1_594_814_650));
    assert_eq!(p.times.generation_unix, Some(1_594_814_694));
    assert_eq!(f.expected_classes[&20], 2_363);
    assert!(f.evidence.contains("operational classifier"));
    assert!(f.evidence.contains("not a field survey"));
    assert!(f.evidence.contains("not per-gate label times"));
    let field = wxdata::level3::radial_to_field(&p, 0.25, |code, _| {
        (code >= 10).then_some(f32::from(code))
    })
    .unwrap();
    assert_eq!(field.time, f.source.acquisition_time);
    assert!(
        field.values.contains(&20.0),
        "clutter survives grid conversion"
    );
    assert!(
        field.values.iter().any(|v| v.is_nan()),
        "missing stays missing"
    );
    // Verify the spatial mask against every real clutter gate center. An empty or
    // unavailable mask cannot silently pass the detector control below.
    let ra = p.radial.as_ref().unwrap();
    for r in &ra.radials {
        for (gate, &class) in r.levels.iter().enumerate().filter(|&(_, &c)| c == 20) {
            let slant = (gate as f64 + 0.5) * f64::from(f.gate_spacing_km);
            let ground = wxdata::xsection::ground_from_slant_km(slant, f64::from(f.elevation_deg));
            let (lon, lat) = wxdata::beam_geometry::destination_lonlat(
                f64::from(p.lon),
                f64::from(p.lat),
                f64::from(r.start_deg + r.delta_deg * 0.5),
                ground * 1_000.0,
            );
            assert_eq!(class_at(&p, 0.25, lon, lat), Some(class));
        }
    }
}

#[test]
fn classification_contract_rejects_bad_context_and_missing_metadata() {
    let mut m = corpus::manifest();
    m.classification_snapshots[0].radar_fixture = "missing-radar".into();
    assert!(corpus::validate(&m).is_err());
    let mut m = corpus::manifest();
    m.classification_snapshots[0].source.acquisition_time += chrono::Duration::seconds(1);
    assert!(corpus::validate(&m).is_err());
    assert!(corpus::read_classification(&m.classification_snapshots[0]).is_err());
    let mut m = corpus::manifest();
    m.classification_snapshots[0].generation_time += chrono::Duration::seconds(1);
    assert!(corpus::read_classification(&m.classification_snapshots[0]).is_err());
    let mut m = corpus::manifest();
    m.classification_snapshots[0].expected_classes.insert(20, 0);
    assert!(corpus::validate(&m).is_err());
    assert!(corpus::read_classification(&m.classification_snapshots[0]).is_err());
    let mut m = corpus::manifest();
    m.classification_snapshots[0].path = "../escape.l3".into();
    assert!(corpus::validate(&m).is_err());
    assert!(corpus::read_classification(&m.classification_snapshots[0]).is_err());
    let mut m = corpus::manifest();
    m.classification_snapshots[0].path = "missing-classification.l3".into();
    assert!(corpus::read_classification(&m.classification_snapshots[0]).is_err());
    m.classification_snapshots.clear();
    assert!(corpus::validate(&m).is_err());
}

#[test]
#[ignore = "large cached fixtures: provision explicitly before running"]
fn cached_classified_clutter_does_not_promote_debris() {
    let m = corpus::manifest();
    let f = &m.classification_snapshots[0];
    let hca = corpus::read_classification(f).unwrap();
    let radar = m.fixtures.iter().find(|r| r.id == f.radar_fixture).unwrap();
    let scan = level2::decode_volume(corpus::read(radar, &corpus::cache_dir()).unwrap()).unwrap();
    assert!(
        scan.sweeps().iter().flat_map(|s| s.radials()).all(|r| {
            r.collection_timestamp() > 0
                && r.collection_timestamp() <= radar.case_time.timestamp_millis()
        }),
        "retrospective analysis time must follow every contributing observation"
    );
    let lowest = level2::bin_scan(&scan, Moment::Reflectivity, 0).unwrap();
    assert!((lowest.elevation_deg - f.elevation_deg).abs() < 0.15);
    assert!((lowest.radar_lat - hca.lat).abs() < 0.002);
    assert!((lowest.radar_lon - hca.lon).abs() < 0.002);
    let (start, generated) = (
        f.source.acquisition_time.timestamp_millis(),
        f.generation_time.timestamp_millis(),
    );
    assert!(
        lowest
            .bin_time_ms
            .iter()
            .all(|&ms| ms >= start && ms <= generated),
        "selected split-cut radials must belong to the classification's acquisition interval"
    );
    let ra = hca.radial.as_ref().unwrap();
    let eligible_clutter = ra
        .radials
        .iter()
        .flat_map(|r| r.levels.iter().enumerate())
        .filter(|&(g, &class)| {
            let range = (g as f32 + 0.5) * f.gate_spacing_km;
            class == 20 && (15.0..150.0).contains(&range)
        })
        .count();
    assert_eq!(
        eligible_clutter, 1_917,
        "operational labels cover the detector's tested range"
    );
    let analyze = || {
        let mut pairs = Vec::new();
        let mut velocity = Vec::new();
        let mut zdr = Vec::new();
        for tilt in 0..4 {
            let z = level2::bin_scan(&scan, Moment::Reflectivity, tilt).unwrap();
            let cc = level2::bin_scan(&scan, Moment::CorrelationCoefficient, tilt).unwrap();
            let v = level2::bin_scan_opts(&scan, Moment::Velocity, tilt, true).unwrap();
            let d = level2::bin_scan(&scan, Moment::DifferentialReflectivity, tilt).unwrap();
            pairs.push((z.clone(), cc));
            velocity.push((v, z));
            zdr.push(d);
        }
        let mut debris = wxdata::tds::detect_volume(&pairs, 0.80, 40.0, 150.0, 4);
        wxdata::tds::apply_zdr(&mut debris, &zdr);
        let mut couplets = wxdata::rotation::detect_volume(&velocity, 25.0, 20.0, 15.0, 150.0, 3);
        wxdata::tds::cross_corroborate(&mut debris, &mut couplets, true);
        let circulations = wxdata::tornado_id::circulations(&couplets, &debris);
        (debris, couplets, circulations)
    };
    let a = analyze();
    let b = analyze();
    assert_eq!(
        format!("{a:?}"),
        format!("{b:?}"),
        "raw measurements and fused results repeat"
    );
    eprintln!("classified clutter: {} eligible HCA gates, {} raw debris, {} couplets, {} fused circulations",
        eligible_clutter, a.0.len(), a.1.len(), a.2.len());
    assert!(
        a.2.iter()
            .all(|c| c.id.lon.is_finite() && c.id.lat.is_finite()),
        "invalid detector positions cannot escape mask verification"
    );
    let in_clutter: Vec<_> = a
        .2
        .iter()
        .filter(|c| class_at(&hca, f64::from(f.gate_spacing_km), c.id.lon, c.id.lat) == Some(20))
        .collect();
    assert!(
        in_clutter
            .iter()
            .all(|c| c.id.tier < wxdata::tornado_id::Tier::Debris),
        "operationally labeled clutter must not promote a debris-tier tornado: {in_clutter:?}"
    );
}
