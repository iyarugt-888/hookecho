//! Sunrise/sunset and moon phase — the two almanac facts a storm chaser actually plans around
//! (how much daylight is left, and how much moon there is once it's gone).
//!
//! Pure math, no network and no ephemeris tables: the NOAA sunrise equation for solar events and
//! the leading terms of the Meeus lunar series for the moon. Both are good to a few minutes,
//! which is all the forecast window prints.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};

/// Earth's obliquity (°) — good enough for a sunrise clock at this precision.
const OBLIQUITY: f64 = 23.4397;
/// Solar zenith at sunrise/sunset: 90° plus refraction and the sun's apparent radius.
const ZENITH: f64 = 90.833;
/// Julian day of the Unix epoch.
const JD_UNIX_EPOCH: f64 = 2_440_587.5;
/// Julian day of J2000.0.
const JD_J2000: f64 = 2_451_545.0;

/// Sunrise and sunset (UTC) for `date` at `lat`/`lon` (degrees, east-positive), or `None` during
/// polar day or polar night when the sun never crosses the horizon.
pub fn sun_times(lat: f64, lon: f64, date: NaiveDate) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let midnight = date.and_hms_opt(0, 0, 0)?.and_utc().timestamp() as f64;
    let jd = midnight / 86_400.0 + JD_UNIX_EPOCH;

    // Days since J2000 for the solar-noon nearest this date at this longitude.
    let n = (jd - JD_J2000 + 0.0008).round();
    let j_star = n - lon / 360.0;

    // Solar mean anomaly, equation of the center, ecliptic longitude.
    let m = (357.5291 + 0.985_600_28 * j_star).rem_euclid(360.0);
    let m_rad = m.to_radians();
    let c = 1.9148 * m_rad.sin() + 0.02 * (2.0 * m_rad).sin() + 0.0003 * (3.0 * m_rad).sin();
    let lambda = (m + c + 180.0 + 102.9372).rem_euclid(360.0);
    let lambda_rad = lambda.to_radians();

    // Solar transit (local solar noon) as a Julian day.
    let j_transit = JD_J2000 + j_star + 0.0053 * m_rad.sin() - 0.0069 * (2.0 * lambda_rad).sin();

    // Declination of the sun, then the hour angle at which it hits the sunrise zenith.
    let sin_decl = lambda_rad.sin() * OBLIQUITY.to_radians().sin();
    let decl = sin_decl.asin();
    let lat_rad = lat.to_radians();
    let cos_omega =
        (ZENITH.to_radians().cos() - lat_rad.sin() * decl.sin()) / (lat_rad.cos() * decl.cos());
    if !(-1.0..=1.0).contains(&cos_omega) {
        return None; // sun stays up (or down) all day at this latitude and season
    }
    let omega = cos_omega.acos().to_degrees();

    let rise = jd_to_utc(j_transit - omega / 360.0)?;
    let set = jd_to_utc(j_transit + omega / 360.0)?;
    Some((rise, set))
}

fn jd_to_utc(jd: f64) -> Option<DateTime<Utc>> {
    let secs = (jd - JD_UNIX_EPOCH) * 86_400.0;
    Utc.timestamp_opt(secs.round() as i64, 0).single()
}

/// Where the sun is directly overhead right now: `(latitude, longitude)` in degrees, east-positive
/// longitude to match [`crate::render::mercator::lonlat_to_world`]. The day/night map draws from
/// this — the terminator is the great circle 90° from this point, and a location is in daylight
/// exactly where [`solar_zenith_cos`] comes out positive.
///
/// Same mean-anomaly/ecliptic-longitude/declination formula [`sun_times`] uses (so both share one
/// tested solar position), evaluated at a continuous instant instead of a day-quantized one, plus
/// Greenwich Mean Sidereal Time for the longitude half `sun_times` never needed.
pub fn subsolar_point(t: DateTime<Utc>) -> (f64, f64) {
    let d = t.timestamp() as f64 / 86_400.0 + JD_UNIX_EPOCH - JD_J2000;

    let m = (357.5291 + 0.985_600_28 * d).rem_euclid(360.0);
    let m_rad = m.to_radians();
    let c = 1.9148 * m_rad.sin() + 0.02 * (2.0 * m_rad).sin() + 0.0003 * (3.0 * m_rad).sin();
    let lambda = (m + c + 180.0 + 102.9372).rem_euclid(360.0);
    let lambda_rad = lambda.to_radians();
    let obliquity_rad = OBLIQUITY.to_radians();

    let decl = (lambda_rad.sin() * obliquity_rad.sin()).asin();
    let ra = (obliquity_rad.cos() * lambda_rad.sin()).atan2(lambda_rad.cos());

    // Simplified GMST (good to a few arcseconds, plenty for a day/night line): right ascension
    // minus sidereal time gives the hour angle at Greenwich, and the sun's own longitude is where
    // that hour angle is zero.
    let gmst = (280.460_618_37 + 360.985_647_366_29 * d).rem_euclid(360.0);
    let lon = (ra.to_degrees() - gmst + 180.0).rem_euclid(360.0) - 180.0;

    (decl.to_degrees(), lon)
}

/// Cosine of the solar zenith angle at `(lat, lon)` given the current subsolar point — positive in
/// daylight, negative at night, zero exactly on the terminator. The standard spherical-astronomy
/// formula (equivalent to `sin(solar elevation)`), so a location's own zenith angle needs no
/// separate elevation calculation.
pub fn solar_zenith_cos(lat_deg: f64, lon_deg: f64, subsolar: (f64, f64)) -> f64 {
    let (lat, decl) = (lat_deg.to_radians(), subsolar.0.to_radians());
    let hour_angle = (lon_deg - subsolar.1).to_radians();
    lat.sin() * decl.sin() + lat.cos() * decl.cos() * hour_angle.cos()
}

/// Latitude (degrees) of the day/night terminator at `lon_deg`, for the given subsolar point.
/// `None` only at the instant the sun sits exactly over the equator (declination zero, twice a
/// year): the terminator is then two meridians rather than a function of longitude, and every
/// caller already has to decide what "the terminator's latitude" even means there.
pub fn terminator_lat_deg(lon_deg: f64, subsolar: (f64, f64)) -> Option<f64> {
    let decl = subsolar.0.to_radians();
    if decl.abs() < 1e-6 {
        return None;
    }
    let hour_angle = (lon_deg - subsolar.1).to_radians();
    // Zenith = 90° solved for latitude: sin(lat)sin(decl) + cos(lat)cos(decl)cos(H) = 0
    // => tan(lat) = -cos(decl)cos(H) / sin(decl). `atan` (not `atan2`) is deliberate: the result
    // is always a plain latitude in (-90°, 90°), never a point needing the extra quadrant atan2
    // resolves.
    let lat = (-decl.cos() * hour_angle.cos() / decl.sin()).atan();
    Some(lat.to_degrees())
}

/// Moon phase as a fraction of the synodic cycle: 0.0 = new, 0.25 = first quarter, 0.5 = full.
///
/// The mean synodic month this used to divide by drifts up to about half a day either side of the
/// true phase, because the moon's orbit is eccentric and the sun tugs on it. These are the largest
/// terms of the phase-angle series in Meeus, *Astronomical Algorithms* ch. 49 — the moon's own
/// anomaly first, then the sun's, then evection and variation. Truncated there, it is good to a
/// few minutes of arc, which is far past what an eight-bucket label can show.
pub fn moon_phase(t: DateTime<Utc>) -> f64 {
    let jd = t.timestamp() as f64 / 86_400.0 + JD_UNIX_EPOCH;
    let tc = (jd - JD_J2000) / 36_525.0; // Julian centuries since J2000.0

    // Mean elongation of the moon from the sun, and the two mean anomalies the corrections ride on.
    let d = (297.850_2 + 445_267.111_5 * tc).to_radians();
    let m = (357.529_1 + 35_999.050_3 * tc).to_radians();
    let mp = (134.963_4 + 477_198.867_6 * tc).to_radians();

    // Elongation corrected to the true one: 180° minus the phase angle of Meeus (49.4).
    let elong = d.to_degrees() + 6.289 * mp.sin() - 2.100 * m.sin()
        + 1.274 * (2.0 * d - mp).sin()
        + 0.658 * (2.0 * d).sin()
        + 0.214 * (2.0 * mp).sin()
        + 0.110 * d.sin();
    (elong / 360.0).rem_euclid(1.0)
}

/// Name and glyph for a phase fraction from [`moon_phase`], in the usual eight buckets.
pub fn moon_label(frac: f64) -> (&'static str, &'static str) {
    // Each named phase is centered on its eighth, so shift by half a bucket before bucketing.
    let bucket = ((frac.rem_euclid(1.0) * 8.0 + 0.5).floor() as usize) % 8;
    match bucket {
        0 => ("New moon", "🌑"),
        1 => ("Waxing crescent", "🌒"),
        2 => ("First quarter", "🌓"),
        3 => ("Waxing gibbous", "🌔"),
        4 => ("Full moon", "🌕"),
        5 => ("Waning gibbous", "🌖"),
        6 => ("Last quarter", "🌗"),
        _ => ("Waning crescent", "🌘"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hhmm(t: DateTime<Utc>) -> (u32, u32) {
        use chrono::Timelike;
        (t.hour(), t.minute())
    }

    fn minutes(t: DateTime<Utc>) -> i64 {
        let (h, m) = hhmm(t);
        h as i64 * 60 + m as i64
    }

    #[test]
    fn okc_summer_solstice() {
        // Oklahoma City, 2024-06-20: sunrise 6:17 AM CDT (11:17Z), sunset 8:51 PM CDT (01:51Z+1).
        let d = NaiveDate::from_ymd_opt(2024, 6, 20).unwrap();
        let (rise, set) = sun_times(35.47, -97.52, d).expect("sun rises in Oklahoma");
        assert!(
            (minutes(rise) - (11 * 60 + 17)).abs() <= 5,
            "sunrise ~11:17Z, got {rise}"
        );
        // Sunset falls after 00Z, so compare against 01:51 on the following UTC day.
        assert!((minutes(set) - 111).abs() <= 5, "sunset ~01:51Z, got {set}");
    }

    #[test]
    fn london_new_years_day() {
        // London, 2024-01-01: sunrise 08:06 GMT, sunset 16:02 GMT (GMT == UTC in January).
        let d = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let (rise, set) = sun_times(51.5074, -0.1278, d).expect("sun rises in London");
        assert!(
            (minutes(rise) - (8 * 60 + 6)).abs() <= 5,
            "sunrise ~08:06Z, got {rise}"
        );
        assert!(
            (minutes(set) - (16 * 60 + 2)).abs() <= 5,
            "sunset ~16:02Z, got {set}"
        );
    }

    #[test]
    fn equator_equinox_day_is_twelve_hours_around_noon() {
        // On the equator at an equinox the day is a few minutes longer than 12 h (refraction plus
        // the sun's radius) and is centered on local solar noon — 12:00Z at 0° longitude.
        let d = NaiveDate::from_ymd_opt(2024, 3, 20).unwrap();
        let (rise, set) = sun_times(0.0, 0.0, d).unwrap();
        let length = (set - rise).num_minutes();
        assert!((725..=730).contains(&length), "≈12h07m, got {length} min");
        let midpoint = (minutes(rise) + minutes(set)) / 2;
        assert!(
            (midpoint - 12 * 60).abs() <= 10,
            "noon-centered, got {rise}/{set}"
        );
    }

    #[test]
    fn southern_hemisphere_seasons_flip() {
        // Sydney in June is winter: a short day (<11 h) unlike OKC's ~14 h.
        let d = NaiveDate::from_ymd_opt(2024, 6, 20).unwrap();
        let (rise, set) = sun_times(-33.87, 151.21, d).unwrap();
        let hours = (set - rise).num_minutes() as f64 / 60.0;
        assert!(
            (9.5..10.5).contains(&hours),
            "short winter day, got {hours}"
        );
    }

    #[test]
    fn polar_night_and_polar_day_have_no_events() {
        let winter = NaiveDate::from_ymd_opt(2024, 12, 21).unwrap();
        let summer = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();
        assert!(sun_times(78.0, 15.0, winter).is_none(), "polar night");
        assert!(sun_times(78.0, 15.0, summer).is_none(), "polar day");
    }

    #[test]
    fn known_new_moon_is_near_phase_zero() {
        // New moon 2024-01-11 11:57 UTC.
        let t = Utc.with_ymd_and_hms(2024, 1, 11, 11, 57, 0).unwrap();
        let p = moon_phase(t);
        // Within an hour of exact new: 1 h is 0.0014 of a synodic month.
        assert!(!(0.002..=0.998).contains(&p), "expected ~new, got {p}");
        assert_eq!(moon_label(p).0, "New moon");
    }

    #[test]
    fn known_full_moon_is_near_phase_half() {
        // Full moon 2024-01-25 17:54 UTC.
        let t = Utc.with_ymd_and_hms(2024, 1, 25, 17, 54, 0).unwrap();
        let p = moon_phase(t);
        assert!((p - 0.5).abs() < 0.002, "expected ~full, got {p}");
        assert_eq!(moon_label(p).0, "Full moon");
    }

    #[test]
    fn labels_cover_every_bucket() {
        let names: Vec<_> = (0..8)
            .map(|i| moon_label(i as f64 / 8.0).0)
            .collect::<Vec<_>>();
        assert_eq!(names[0], "New moon");
        assert_eq!(names[2], "First quarter");
        assert_eq!(names[4], "Full moon");
        assert_eq!(names[6], "Last quarter");
        // Every bucket distinct — no accidental double-mapping in the shift-and-floor.
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 8);
    }

    #[test]
    fn subsolar_declination_matches_the_seasons() {
        // Near the June solstice the subsolar point sits over the Tropic of Cancer; near the
        // December solstice, the Tropic of Capricorn. Declination barely moves right at a
        // solstice, which is what makes it a good instant to check against a fixed tolerance.
        let june = Utc.with_ymd_and_hms(2024, 6, 21, 0, 0, 0).unwrap();
        let december = Utc.with_ymd_and_hms(2024, 12, 21, 12, 0, 0).unwrap();
        assert!(
            (subsolar_point(june).0 - 23.44).abs() < 0.5,
            "got {:?}",
            subsolar_point(june)
        );
        assert!(
            (subsolar_point(december).0 + 23.44).abs() < 0.5,
            "got {:?}",
            subsolar_point(december)
        );
    }

    #[test]
    fn subsolar_longitude_tracks_local_solar_noon() {
        // At 12:00 UTC the sun is within the equation of time's reach (<= ~17 minutes, ~4.3° of
        // longitude) of standing over the Greenwich meridian, on any date of the year — this is
        // what "UTC" being mean solar time at 0° longitude actually means.
        for md in [(3, 20), (6, 21), (9, 22), (12, 21)] {
            let t = Utc.with_ymd_and_hms(2024, md.0, md.1, 12, 0, 0).unwrap();
            let (_, lon) = subsolar_point(t);
            assert!(lon.abs() < 4.5, "{md:?}: subsolar lon {lon}");
        }
    }

    #[test]
    fn zenith_cosine_is_one_overhead_and_minus_one_at_the_antipode() {
        let sub = (10.0, -80.0);
        assert!((solar_zenith_cos(sub.0, sub.1, sub) - 1.0).abs() < 1e-9);
        let antipode = (-sub.0, sub.1 + 180.0);
        assert!((solar_zenith_cos(antipode.0, antipode.1, sub) - (-1.0)).abs() < 1e-9);
    }

    #[test]
    fn terminator_crosses_the_equator_a_quarter_turn_from_the_subsolar_meridian() {
        // True regardless of season: 90° of hour angle from local solar noon is always the
        // equator crossing, because sin(elevation) at the equator only ever depends on cos(H).
        let sub = (23.4, 40.0);
        let lat = terminator_lat_deg(sub.1 + 90.0, sub).unwrap();
        assert!(lat.abs() < 1e-6, "expected the equator, got {lat}");
    }

    #[test]
    fn terminator_matches_the_polar_circle_under_the_subsolar_meridian() {
        // At the subsolar meridian itself (H = 0) the terminator sits at ±(90° - |declination|) —
        // the Arctic/Antarctic Circle, the classic solstice polar-day/polar-night boundary.
        let sub = (23.44, 0.0);
        let lat = terminator_lat_deg(sub.1, sub).unwrap();
        assert!(
            (lat + (90.0 - sub.0)).abs() < 0.1,
            "expected the antarctic circle, got {lat}"
        );
    }

    #[test]
    fn terminator_is_none_exactly_at_zero_declination() {
        assert!(terminator_lat_deg(50.0, (0.0, 10.0)).is_none());
    }
}
