//! Confirming an automatic detection with what people and forecasters said about it.
//!
//! A detector's confidence is a claim made from radar alone, and it stops at 100%. Independent
//! human evidence is a different kind of thing, so it is kept apart rather than blended into that
//! number: a detection near a tornado a spotter reported, or inside a tornado warning the NWS
//! tagged `OBSERVED`, is **confirmed**, a tier above any radar-only score.
//!
//! What counts, and what deliberately does not:
//!
//! * a local storm report of a tornado near the detection and close in time: **reported**;
//! * a tornado warning polygon over it that says the tornado is `OBSERVED` (or a Tornado
//!   Emergency): **observed**;
//! * both together read as fully confirmed.
//!
//! A plain tornado warning does not count. Most are issued on the same radar signatures the
//! detector sees, so agreeing with one would be the detector confirming itself.
//!
//! Confirmation never changes the detector's own score, and it is not used to tune it: a detector
//! scored against the reports that also confirm it would only be measuring its agreement with them.

use crate::overlay::point_in_ring;

/// How far from a detection a tornado report may be and still be about it (km).
pub const REPORT_RADIUS_KM: f64 = 10.0;

/// How far in time from the detection (minutes). Reports are logged when seen or surveyed, often
/// a little after the fact, so this is wider than a scan interval.
pub const REPORT_WINDOW_MIN: i64 = 30;

/// A tornado local storm report.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TornadoReport {
    pub lon: f64,
    pub lat: f64,
    /// Minutes since any fixed origin; only differences are used.
    pub minute: i64,
}

/// A tornado warning polygon.
#[derive(Debug, Clone, PartialEq)]
pub struct TornadoWarning {
    /// Rings in `[lon, lat]`; the first is the outer boundary and any others are holes.
    pub rings: Vec<Vec<[f64; 2]>>,
    /// The warning says the tornado is observed (`tornadoDetection: OBSERVED`), or it is a Tornado
    /// Emergency.
    pub observed: bool,
}

/// The human evidence available for one moment.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Evidence {
    pub reports: Vec<TornadoReport>,
    pub warnings: Vec<TornadoWarning>,
}

/// What corroborates a detection. `Confirmation::NONE` is the ordinary radar-only case.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Confirmation {
    /// Inside a tornado warning marked observed.
    pub observed_warning: bool,
    /// The nearest tornado report in range, as `(km away, minutes from the detection)`, the
    /// minutes positive when the report came after.
    pub report: Option<(f32, i32)>,
}

/// The tier a [`Confirmation`] amounts to, weakest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Only an observed warning covers it.
    Observed,
    /// Only a tornado report is near it.
    Reported,
    /// Both.
    Confirmed,
}

impl Level {
    /// The badge text.
    pub fn label(self) -> &'static str {
        match self {
            Level::Observed => "OBSERVED",
            Level::Reported => "REPORTED",
            Level::Confirmed => "CONFIRMED",
        }
    }
}

impl Confirmation {
    pub const NONE: Confirmation = Confirmation {
        observed_warning: false,
        report: None,
    };

    /// The tier, or `None` when nothing corroborates it.
    pub fn level(&self) -> Option<Level> {
        match (self.observed_warning, self.report.is_some()) {
            (true, true) => Some(Level::Confirmed),
            (false, true) => Some(Level::Reported),
            (true, false) => Some(Level::Observed),
            (false, false) => None,
        }
    }

    /// A line saying what backs it, for a tooltip; `None` when nothing does.
    pub fn describe(&self) -> Option<String> {
        let level = self.level()?;
        let mut parts = Vec::new();
        if let Some((km, min)) = self.report {
            let when = match min {
                0 => "at the same time".to_string(),
                m if m > 0 => format!("{m} min later"),
                m => format!("{} min earlier", -m),
            };
            parts.push(format!("tornado report {km:.0} km away, {when}"));
        }
        if self.observed_warning {
            parts.push("inside a tornado warning marked observed".to_string());
        }
        Some(format!("{}: {}", level.label(), parts.join("; ")))
    }
}

/// Great-circle distance in km between two lon/lat points.
fn km_between(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, la2) = (a.1.to_radians(), b.1.to_radians());
    let dlat = la2 - la1;
    let dlon = (b.0 - a.0).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * 6371.0 * h.sqrt().asin()
}

/// Whether `(lon, lat)` is inside a warning: in the outer ring and in none of the holes.
fn inside(w: &TornadoWarning, lon: f64, lat: f64) -> bool {
    let Some((outer, holes)) = w.rings.split_first() else {
        return false;
    };
    point_in_ring(outer, lon, lat) && !holes.iter().any(|h| point_in_ring(h, lon, lat))
}

/// What corroborates a detection at `(lon, lat)` at `minute`.
pub fn confirm(lon: f64, lat: f64, minute: i64, evidence: &Evidence) -> Confirmation {
    let observed_warning = evidence
        .warnings
        .iter()
        .any(|w| w.observed && inside(w, lon, lat));
    let report = evidence
        .reports
        .iter()
        .filter(|r| (r.minute - minute).abs() <= REPORT_WINDOW_MIN)
        .map(|r| (km_between((lon, lat), (r.lon, r.lat)), r.minute - minute))
        .filter(|(km, _)| *km <= REPORT_RADIUS_KM)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(km, min)| (km as f32, min as i32));
    Confirmation {
        observed_warning,
        report,
    }
}

/// The minute of the day-and-time `hhmm` ("2001") nearest to `reference_minute`. Local storm
/// reports carry only a time of day, so this picks the day: the one that puts it closest to the
/// reference, which is what makes a report at 0005 belong to a 2358 detection. `None` for a
/// malformed time.
pub fn report_minute(hhmm: &str, reference_minute: i64) -> Option<i64> {
    let h: i64 = hhmm.get(0..2)?.parse().ok()?;
    let m: i64 = hhmm.get(2..4)?.parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    let of_day = h * 60 + m;
    let day = reference_minute.div_euclid(1440);
    [day - 1, day, day + 1]
        .into_iter()
        .map(|d| d * 1440 + of_day)
        .min_by_key(|t| (t - reference_minute).abs())
}

/// Whether a warning's `tornadoDetection` value or headline marks the tornado as observed.
pub fn is_observed(detection: Option<&str>, emergency: bool) -> bool {
    emergency
        || detection.is_some_and(|d| {
            let d = d.to_ascii_uppercase();
            d.contains("OBSERVED") && !d.contains("NOT OBSERVED")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(lon: f64, lat: f64, half: f64) -> Vec<[f64; 2]> {
        vec![
            [lon - half, lat - half],
            [lon + half, lat - half],
            [lon + half, lat + half],
            [lon - half, lat + half],
            [lon - half, lat - half],
        ]
    }

    fn warning(observed: bool) -> TornadoWarning {
        TornadoWarning {
            rings: vec![square(-97.5, 35.3, 0.2)],
            observed,
        }
    }

    #[test]
    fn nothing_nearby_confirms_nothing() {
        let c = confirm(-97.5, 35.3, 100, &Evidence::default());
        assert_eq!(c, Confirmation::NONE);
        assert_eq!(c.level(), None);
        assert_eq!(c.describe(), None);
    }

    #[test]
    fn a_nearby_recent_report_is_reported() {
        let ev = Evidence {
            reports: vec![TornadoReport {
                lon: -97.5,
                lat: 35.32,
                minute: 110,
            }],
            warnings: vec![],
        };
        let c = confirm(-97.5, 35.3, 100, &ev);
        assert_eq!(c.level(), Some(Level::Reported));
        let (km, min) = c.report.unwrap();
        assert!((km - 2.2).abs() < 0.3, "{km}");
        assert_eq!(min, 10);
        assert!(c.describe().unwrap().contains("10 min later"));
    }

    #[test]
    fn a_report_too_far_or_too_long_ago_is_about_something_else() {
        let far = TornadoReport {
            lon: -97.5 + 0.3,
            lat: 35.3,
            minute: 100,
        };
        let stale = TornadoReport {
            lon: -97.5,
            lat: 35.3,
            minute: 100 - REPORT_WINDOW_MIN - 1,
        };
        let ev = Evidence {
            reports: vec![far, stale],
            warnings: vec![],
        };
        assert_eq!(confirm(-97.5, 35.3, 100, &ev), Confirmation::NONE);
    }

    #[test]
    fn the_nearest_report_is_the_one_named() {
        let ev = Evidence {
            reports: vec![
                TornadoReport {
                    lon: -97.5,
                    lat: 35.35,
                    minute: 100,
                },
                TornadoReport {
                    lon: -97.5,
                    lat: 35.31,
                    minute: 95,
                },
            ],
            warnings: vec![],
        };
        let (km, min) = confirm(-97.5, 35.3, 100, &ev).report.unwrap();
        assert!(km < 2.0);
        assert_eq!(min, -5);
    }

    #[test]
    fn only_an_observed_warning_counts_and_only_over_the_detection() {
        let ev = |observed| Evidence {
            reports: vec![],
            warnings: vec![warning(observed)],
        };
        // A plain (radar-indicated) tornado warning is not corroboration.
        assert_eq!(confirm(-97.5, 35.3, 0, &ev(false)), Confirmation::NONE);
        let c = confirm(-97.5, 35.3, 0, &ev(true));
        assert_eq!(c.level(), Some(Level::Observed));
        assert!(c.describe().unwrap().contains("observed"));
        // Outside the polygon.
        assert_eq!(confirm(-96.0, 35.3, 0, &ev(true)), Confirmation::NONE);
    }

    #[test]
    fn a_hole_in_the_warning_is_outside_it() {
        let mut w = warning(true);
        w.rings.push(square(-97.5, 35.3, 0.05));
        let ev = Evidence {
            reports: vec![],
            warnings: vec![w],
        };
        assert_eq!(confirm(-97.5, 35.3, 0, &ev), Confirmation::NONE);
        assert!(confirm(-97.5, 35.45, 0, &ev).observed_warning);
    }

    #[test]
    fn a_report_and_an_observed_warning_together_are_confirmed_and_outrank_either() {
        let ev = Evidence {
            reports: vec![TornadoReport {
                lon: -97.5,
                lat: 35.3,
                minute: 100,
            }],
            warnings: vec![warning(true)],
        };
        let c = confirm(-97.5, 35.3, 100, &ev);
        assert_eq!(c.level(), Some(Level::Confirmed));
        assert!(Level::Confirmed > Level::Reported && Level::Reported > Level::Observed);
        let text = c.describe().unwrap();
        assert!(
            text.starts_with("CONFIRMED") && text.contains("report") && text.contains("warning")
        );
    }

    #[test]
    fn a_time_of_day_picks_the_day_nearest_the_detection() {
        // 2001 UTC on day 10, and a report logged at 0005 just after midnight.
        let detection = 10 * 1440 + 23 * 60 + 58;
        assert_eq!(report_minute("0005", detection), Some(11 * 1440 + 5));
        assert_eq!(
            report_minute("2350", detection),
            Some(10 * 1440 + 23 * 60 + 50)
        );
        assert_eq!(report_minute("0005", 10 * 1440 + 3), Some(10 * 1440 + 5));
        assert_eq!(
            report_minute("2359", 10 * 1440 + 1),
            Some(9 * 1440 + 23 * 60 + 59)
        );
        assert_eq!(report_minute("2560", 0), None);
        assert_eq!(report_minute("12", 0), None);
        assert_eq!(report_minute("", 0), None);
    }

    #[test]
    fn observed_means_observed_and_not_the_absence_of_it() {
        assert!(is_observed(Some("OBSERVED"), false));
        assert!(is_observed(Some("observed"), false));
        assert!(!is_observed(Some("RADAR INDICATED"), false));
        assert!(!is_observed(None, false));
        assert!(
            is_observed(None, true),
            "a Tornado Emergency is an observed tornado"
        );
    }
}
