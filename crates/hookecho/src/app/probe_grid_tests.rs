use super::{format_probe_field_value, HookEchoApp};
use crate::render::FieldLayer as FL;
use crate::settings::TempUnit;

#[test]
fn probe_values_follow_legend_units_and_categories() {
    assert_eq!(
        format_probe_field_value(FL::GlobalTemp2m, 273.15, TempUnit::Fahrenheit).as_deref(),
        Some("32.0 °F")
    );
    assert_eq!(
        format_probe_field_value(FL::Hca, 90.0, TempUnit::Celsius).as_deref(),
        Some("Graupel")
    );
    assert_eq!(
        format_probe_field_value(FL::Mrms, 42.25, TempUnit::Celsius).as_deref(),
        Some("42.2 dBZ")
    );
    assert!(format_probe_field_value(FL::Mrms, f32::NAN, TempUnit::Celsius).is_none());
}

/// ROADMAP_NEW E6's "brightness-temperature sample": a satellite pixel has to read as a
/// temperature in the user's own unit, not as the raw Kelvin the grid holds.
#[test]
fn a_goes_channel_samples_as_a_brightness_temperature() {
    assert_eq!(
        format_probe_field_value(FL::GoesIr, 273.15, TempUnit::Celsius).as_deref(),
        Some("0.0 °C")
    );
    assert_eq!(
        format_probe_field_value(FL::GoesIr, 273.15, TempUnit::Fahrenheit).as_deref(),
        Some("32.0 °F")
    );
    // Every water-vapor channel shares one ramp, so all three have to read this way too.
    assert_eq!(
        format_probe_field_value(FL::GoesMidWaterVapor, 273.15, TempUnit::Celsius).as_deref(),
        Some("0.0 °C")
    );
}

/// The two derived GOES layers deliberately store a *transformed* quantity rather than an
/// absolute brightness temperature (`GoesColdTop` holds degrees colder than its 210 K
/// threshold; `GoesDustDiff` holds a band difference). Presenting either as a temperature
/// would be a wrong number with a plausible-looking unit beside it, so this pins that they
/// carry their own units instead of going through the Kelvin conversion.
#[test]
fn derived_goes_layers_are_not_reported_as_absolute_temperatures() {
    let cold_top = format_probe_field_value(FL::GoesColdTop, 20.0, TempUnit::Celsius)
        .expect("a finite sample formats");
    assert!(
        !cold_top.contains("°C"),
        "an offset from a threshold is not a temperature: {cold_top}"
    );
    assert!(cold_top.starts_with("20.0"), "{cold_top}");

    assert_eq!(
        format_probe_field_value(FL::GoesRgb, 0x80_40_20_u32 as f32, TempUnit::Celsius).as_deref(),
        Some("RGB 128 64 32"),
        "a composite probes as its colour, not its packed number"
    );
    {
        use crate::app::{goes_grid, goes_sector_for};
        use wxdata::goes_abi::{Footprint, Sector};
        let fp = Footprint {
            lon_west: -82.0,
            lon_east: -68.0,
            lat_south: 32.0,
            lat_north: 46.0,
            time: chrono::DateTime::UNIX_EPOCH,
        };
        let boston = (-71.0, 42.3);
        let okc = (-97.5, 35.5);
        // CONUS stays CONUS; a box not yet seen is tried; a box over the view is read.
        assert_eq!(
            goes_sector_for(Sector::Conus, Some((Sector::Meso1, fp)), okc),
            Sector::Conus
        );
        assert_eq!(goes_sector_for(Sector::Meso1, None, okc), Sector::Meso1);
        assert_eq!(
            goes_sector_for(Sector::Meso1, Some((Sector::Meso1, fp)), boston),
            Sector::Meso1
        );
        // The box moved away from the view: CONUS until it covers it again.
        assert_eq!(
            goes_sector_for(Sector::Meso1, Some((Sector::Meso1, fp)), okc),
            Sector::Conus
        );
        // Where Meso 1 is says nothing about Meso 2.
        assert_eq!(
            goes_sector_for(Sector::Meso2, Some((Sector::Meso1, fp)), okc),
            Sector::Meso2
        );
        assert_eq!(goes_grid(Sector::Meso2), (480, 480));
    }
    {
        use crate::app::{goes_frame_ready, goes_slot};
        use wxdata::goes_abi::Sector;
        let t = |m: i64| chrono::DateTime::from_timestamp(1_790_000_000 + m * 60, 0).unwrap();
        let tol = chrono::Duration::minutes(10);
        // Live: the slot is None and a live fetch is ready whatever its time.
        assert_eq!(goes_slot(None, Sector::Conus), None);
        assert!(goes_frame_ready(Some(None), None, Some(t(-3)), None, tol));
        // Scrubbed within one 5-minute CONUS slot: no refetch; into the next: refetch.
        assert_eq!(
            goes_slot(Some(t(0)), Sector::Conus),
            goes_slot(Some(t(0)), Sector::Conus)
        );
        assert_ne!(
            goes_slot(Some(t(0)), Sector::Conus),
            goes_slot(Some(t(6)), Sector::Conus)
        );
        // A mesoscale slot is a minute.
        assert_ne!(
            goes_slot(Some(t(0)), Sector::Meso1),
            goes_slot(Some(t(1)), Sector::Meso1)
        );
        let s = goes_slot(Some(t(0)), Sector::Conus);
        // The archive frame for this slot, close to the target: painted.
        assert!(goes_frame_ready(Some(s), s, Some(t(2)), Some(t(0)), tol));
        // A live frame still held when the view is scrubbed back: not painted.
        assert!(!goes_frame_ready(
            Some(None),
            s,
            Some(t(0)),
            Some(t(0)),
            tol
        ));
        // An archive frame still held after going live: not painted until the newest lands.
        assert!(!goes_frame_ready(Some(s), None, Some(t(0)), None, tol));
        // The nearest frame the bucket had was far off (a gap in the archive): not painted.
        assert!(!goes_frame_ready(Some(s), s, Some(t(40)), Some(t(0)), tol));
    }
    {
        use crate::app::glm_flashes_for;
        let t = |m: i64| chrono::DateTime::from_timestamp(1_790_000_000 + m * 60, 0).unwrap();
        let flash = |m: i64| wxdata::glm::Flash {
            lon: -97.0,
            lat: 35.0,
            energy: 1.0,
            time: t(m),
        };
        let live: std::collections::VecDeque<_> = [flash(100), flash(101)].into();
        let archive = (t(0), vec![flash(-3), flash(-1)]);
        // Live: the feed, aged against now.
        let (f, clock) = glm_flashes_for(None, &live, Some(&archive), t(102));
        assert_eq!((f.len(), clock), (2, t(102)));
        // Scrubbed to the archive window's minute: its flashes, aged against the view's time.
        let (f, clock) = glm_flashes_for(Some(t(0)), &live, Some(&archive), t(102));
        assert_eq!((f.len(), clock), (2, t(0)));
        // Scrubbed elsewhere before its window arrives: nothing, not today's flashes.
        let (f, _) = glm_flashes_for(Some(t(30)), &live, Some(&archive), t(102));
        assert!(f.is_empty());
    }
    let dust = format_probe_field_value(FL::GoesDustDiff, 3.0, TempUnit::Fahrenheit)
        .expect("a finite sample formats");
    assert!(
        !dust.contains("°F"),
        "a band difference is not a temperature: {dust}"
    );
    assert!(dust.starts_with("3.0"), "{dust}");
}

#[test]
fn legacy_grid_products_get_analyst_facing_names() {
    assert_eq!(
        HookEchoApp::probe_field_product(FL::Mosaic),
        "Multi-radar reflectivity"
    );
    assert_eq!(
        HookEchoApp::probe_field_product(FL::CompositeLocal),
        "Local composite reflectivity"
    );
}
