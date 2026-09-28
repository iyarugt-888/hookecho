//! Y'all mode: the severe weather picture for one spot, said plainly. It reads the products the
//! app already has (warnings, watches, the SPC outlook, SCIT storm cells) at a single point (your
//! GPS fix, else the map's centre) and answers the question a person actually has: how worried
//! should y'all be, and why.
//!
//! - the Y'all-O-Meter: one level, 0 (all quiet) to 5 (take cover now), with the reasons behind it
//! - Y'all Watches: each watch over the spot, with what it means
//! - Y'all Outlook: the Day 1-3 SPC category over the spot, in words
//! - Y'all Tracks: storms whose forecast motion brings them near the spot, with when and how close
//!
//! Pure: no app state, no network, so every rule is tested here.

use wxdata::overlay::{FeatureKind, GeoFeature};

/// SPC categorical risk, lowest to highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Risk {
    Tstm,
    Mrgl,
    Slgt,
    Enh,
    Mdt,
    High,
}

impl Risk {
    /// The short code (`SLGT`) or the long name SPC's `LABEL2` carries ("Slight Risk",
    /// "General Thunderstorms Risk").
    fn parse(label: &str) -> Option<Risk> {
        let l = label.trim().to_ascii_uppercase();
        Some(match l.as_str() {
            "TSTM" => Risk::Tstm,
            "MRGL" => Risk::Mrgl,
            "SLGT" => Risk::Slgt,
            "ENH" => Risk::Enh,
            "MDT" => Risk::Mdt,
            "HIGH" => Risk::High,
            _ if l.starts_with("GENERAL THUNDER") => Risk::Tstm,
            _ if l.starts_with("MARGINAL") => Risk::Mrgl,
            _ if l.starts_with("SLIGHT") => Risk::Slgt,
            _ if l.starts_with("ENHANCED") => Risk::Enh,
            _ if l.starts_with("MODERATE") => Risk::Mdt,
            _ if l.starts_with("HIGH") => Risk::High,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Risk::Tstm => "General thunder",
            Risk::Mrgl => "Marginal risk (1 of 5)",
            Risk::Slgt => "Slight risk (2 of 5)",
            Risk::Enh => "Enhanced risk (3 of 5)",
            Risk::Mdt => "Moderate risk (4 of 5)",
            Risk::High => "High risk (5 of 5)",
        }
    }

    /// What the category means, plainly.
    pub fn plain(self) -> &'static str {
        match self {
            Risk::Tstm => "Just regular old thunderstorms. Lightning, but nothing severe expected.",
            Risk::Mrgl => "A storm or two could get rowdy: a little hail or a gusty wind.",
            Risk::Slgt => "A few storms could turn severe. Keep your phone charged.",
            Risk::Enh => "Several strong storms are likely. Know where you'd shelter.",
            Risk::Mdt => "Widespread severe storms, some of them bad. Plan your day around it.",
            Risk::High => "A big severe outbreak is expected. Take this one real serious.",
        }
    }

    pub fn rgb(self) -> [u8; 3] {
        match self {
            Risk::Tstm => [192, 232, 192],
            Risk::Mrgl => [127, 197, 127],
            Risk::Slgt => [246, 246, 127],
            Risk::Enh => [230, 194, 127],
            Risk::Mdt => [230, 127, 127],
            Risk::High => [255, 127, 255],
        }
    }
}

/// The highest SPC categorical risk over `(lon, lat)` in one day's categorical outlook, whose
/// features are titled `Day N: LABEL`.
pub fn outlook_at(features: &[GeoFeature], lon: f64, lat: f64) -> Option<Risk> {
    features
        .iter()
        .filter(|f| f.kind == FeatureKind::Outlook && f.contains(lon, lat))
        .filter_map(|f| Risk::parse(f.title.rsplit(':').next()?))
        .max()
}

/// A watch over the spot, plainly.
#[derive(Debug, Clone, PartialEq)]
pub struct YallWatch {
    pub title: String,
    pub tornado: bool,
    pub pds: bool,
    pub plain: &'static str,
    pub expires: Option<chrono::DateTime<chrono::Utc>>,
}

/// Every watch over `(lon, lat)`: the SPC watch boxes and NWS watch counties both, one row per
/// watch (the county rows of one watch share a title).
pub fn watches_at(
    watch_boxes: &[GeoFeature],
    alerts: &[GeoFeature],
    lon: f64,
    lat: f64,
) -> Vec<YallWatch> {
    let mut out: Vec<YallWatch> = Vec::new();
    let candidates = watch_boxes.iter().chain(
        alerts
            .iter()
            .filter(|f| matches!(f.kind, FeatureKind::Watch | FeatureKind::WatchBox)),
    );
    for f in candidates {
        let event = f
            .alert
            .as_ref()
            .map(|a| a.event.as_str())
            .unwrap_or(f.title.as_str());
        let lower = event.to_ascii_lowercase();
        let tornado = lower.contains("tornado");
        if !(tornado || lower.contains("severe thunderstorm")) || !f.contains(lon, lat) {
            continue;
        }
        let text = format!(
            "{} {}",
            f.detail,
            f.alert
                .as_ref()
                .map(|a| a.description.as_str())
                .unwrap_or("")
        )
        .to_ascii_lowercase();
        let pds = text.contains("particularly dangerous situation");
        let kind = if tornado {
            "Tornado Watch"
        } else {
            "Severe Thunderstorm Watch"
        };
        if out.iter().any(|w| w.tornado == tornado) {
            continue; // county rows of the same watch, or a box and its counties
        }
        out.push(YallWatch {
            title: if pds {
                format!("PDS {kind}")
            } else {
                kind.to_string()
            },
            tornado,
            pds,
            plain: match (tornado, pds) {
                (true, true) => {
                    "Strong, long-track tornadoes are possible. Be ready to shelter at a moment's notice."
                }
                (true, false) => {
                    "Tornadoes could form around y'all. Know where you'd shelter and keep alerts on."
                }
                (false, true) => {
                    "Destructive wind and very large hail are possible. Get the car under cover."
                }
                (false, false) => {
                    "Storms could bring damaging wind and hail. Keep an eye on the radar."
                }
            },
            expires: f.alert.as_ref().and_then(|a| a.expires),
        });
    }
    out.sort_by_key(|w| (!w.tornado, !w.pds));
    out
}

/// A storm whose forecast motion brings it near the spot.
#[derive(Debug, Clone, PartialEq)]
pub struct YallTrack {
    pub id: String,
    /// Where it is now and where it passes closest, `[lon, lat]`.
    pub from: [f64; 2],
    pub closest: [f64; 2],
    /// Minutes until it passes closest, and how far off it passes (km).
    pub eta_min: f64,
    pub pass_km: f64,
    pub hail_in: Option<f32>,
    pub rotation: bool,
    pub max_dbz: Option<f32>,
}

impl YallTrack {
    /// Severe by its own numbers: rotation, inch hail, or a 60 dBZ core.
    pub fn severe(&self) -> bool {
        self.rotation
            || self.hail_in.is_some_and(|h| h >= 1.0)
            || self.max_dbz.is_some_and(|d| d >= 60.0)
    }

    /// "Storm K4 gets to y'all in about 25 min, passing 2 mi away, with 1.5 in hail"
    pub fn plain(&self) -> String {
        let mi = self.pass_km * 0.621_371;
        let pass = if mi < 1.5 {
            "right over y'all".to_string()
        } else {
            format!("passing {mi:.0} mi away")
        };
        let when = if self.eta_min < 2.0 {
            "is on y'all now".to_string()
        } else {
            format!("gets to y'all in about {:.0} min", self.eta_min)
        };
        let mut s = format!("Storm {} {when}, {pass}", self.id);
        if self.rotation {
            s.push_str(", and it's rotating");
        } else if let Some(h) = self.hail_in.filter(|h| *h >= 0.75) {
            s.push_str(&format!(", with {h:.2} in hail"));
        }
        s
    }
}

/// Storms passing within this far of the spot are Y'all Tracks.
pub const TRACK_PASS_KM: f64 = 16.0;
/// ...within this many minutes.
pub const TRACK_HORIZON_MIN: f64 = 60.0;

/// The storms whose forecast motion brings them within [`TRACK_PASS_KM`] of `spot` in the next
/// [`TRACK_HORIZON_MIN`] minutes, soonest first.
pub fn tracks_toward(cells: &[wxdata::level3::Cell], spot: [f64; 2]) -> Vec<YallTrack> {
    let mut out: Vec<YallTrack> = cells
        .iter()
        .filter(|c| c.kind == wxdata::level3::CellKind::Storm)
        .filter_map(|c| {
            let from = [c.lon, c.lat];
            let (dir, kt) = (
                c.mvt_deg.unwrap_or(0.0) as f64,
                c.mvt_kt.unwrap_or(0.0) as f64,
            );
            let (pass_km, eta_min) =
                crate::geo::closest_approach(from, dir, kt, spot, TRACK_HORIZON_MIN);
            if pass_km > TRACK_PASS_KM {
                return None;
            }
            let closest = if kt > 1.0 {
                crate::geo::destination_point(from, dir, kt * 1.852 / 60.0 * eta_min)
            } else {
                from
            };
            Some(YallTrack {
                id: c.id.clone(),
                from,
                closest,
                eta_min,
                pass_km,
                hail_in: c.hail_in,
                rotation: c.tvs.is_some() || c.meso.is_some(),
                max_dbz: c.max_dbz,
            })
        })
        .collect();
    out.sort_by(|a, b| a.eta_min.total_cmp(&b.eta_min));
    out
}

/// The Y'all-O-Meter reading.
#[derive(Debug, Clone, PartialEq)]
pub struct Meter {
    /// 0 (all quiet) to 5 (take cover now).
    pub level: u8,
    /// Why, most serious first.
    pub reasons: Vec<String>,
}

impl Meter {
    pub fn headline(&self) -> &'static str {
        match self.level {
            0 => "All quiet, y'all",
            1 => "Y'all keep an eye on the sky",
            2 => "Y'all pay attention today",
            3 => "Y'all get ready",
            4 => "Y'all take action now",
            _ => "Y'ALL TAKE COVER NOW",
        }
    }

    /// Green through yellow and red to magenta.
    pub fn rgb(level: u8) -> [u8; 3] {
        match level {
            0 => [90, 180, 110],
            1 => [150, 200, 90],
            2 => [240, 210, 70],
            3 => [245, 150, 50],
            4 => [230, 60, 50],
            _ => [220, 60, 220],
        }
    }
}

/// Read the meter at the spot from warnings (`alerts` already filtered to the spot), watches,
/// today's outlook and approaching storms.
pub fn meter(
    alerts_here: &[&GeoFeature],
    watches: &[YallWatch],
    outlook: Option<Risk>,
    tracks: &[YallTrack],
) -> Meter {
    let mut found: Vec<(u8, String)> = Vec::new();
    for f in alerts_here {
        let Some(a) = f.alert.as_ref() else { continue };
        let event = a.event.as_str();
        let lower = event.to_ascii_lowercase();
        let text = format!("{} {}", a.headline, a.description).to_ascii_lowercase();
        let emergency = lower.contains("emergency") || text.contains("emergency");
        let flash_flood_warning = lower.contains("flash flood") && lower.contains("warning");
        let level = if lower.contains("tornado warning")
            || (flash_flood_warning && emergency)
            || lower.contains("extreme wind warning")
        {
            5
        } else if lower.contains("severe thunderstorm warning") || flash_flood_warning {
            let destructive = a
                .damage_threat
                .as_deref()
                .is_some_and(|d| d.eq_ignore_ascii_case("destructive"));
            if destructive {
                5
            } else {
                4
            }
        } else if f.kind == FeatureKind::Warning {
            3
        } else if matches!(f.kind, FeatureKind::Advisory | FeatureKind::Statement) {
            1
        } else {
            continue;
        };
        found.push((level, format!("{event} for y'all")));
    }
    for w in watches {
        found.push((if w.pds { 4 } else { 3 }, format!("{} over y'all", w.title)));
    }
    if let Some(r) = outlook {
        let level = match r {
            Risk::Tstm | Risk::Mrgl => 1,
            Risk::Slgt | Risk::Enh => 2,
            Risk::Mdt | Risk::High => 3,
        };
        found.push((level, format!("Today's outlook: {}", r.name())));
    }
    for t in tracks {
        let level = if t.rotation && t.eta_min <= 30.0 && t.pass_km <= 10.0 {
            4
        } else if t.severe() {
            3
        } else {
            2
        };
        found.push((level, t.plain()));
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    Meter {
        level: found.first().map(|f| f.0).unwrap_or(0),
        reasons: found.into_iter().map(|f| f.1).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wxdata::overlay::AlertInfo;

    fn square(kind: FeatureKind, title: &str) -> GeoFeature {
        GeoFeature {
            rings: vec![vec![
                [-98.0, 35.0],
                [-97.0, 35.0],
                [-97.0, 36.0],
                [-98.0, 36.0],
                [-98.0, 35.0],
            ]],
            fill: [0; 4],
            stroke: [0; 4],
            kind,
            title: title.to_string(),
            detail: String::new(),
            alert: None,
        }
    }

    fn alert(event: &str) -> GeoFeature {
        let mut f = square(FeatureKind::Warning, event);
        f.alert = Some(AlertInfo {
            id: event.to_string(),
            event: event.to_string(),
            headline: String::new(),
            area: String::new(),
            description: String::new(),
            instruction: String::new(),
            expires: None,
            max_hail_in: None,
            max_wind: None,
            tornado_detection: None,
            damage_threat: None,
            source: None,
            motion: None,
            vtec: None,
        });
        f
    }

    fn cell(id: &str, lon: f64, lat: f64, toward: f32, kt: f32) -> wxdata::level3::Cell {
        let mut c = wxdata::level3::Cell::default();
        c.id = id.to_string();
        c.lon = lon;
        c.lat = lat;
        c.mvt_deg = Some(toward);
        c.mvt_kt = Some(kt);
        c
    }

    #[test]
    fn outlook_takes_the_highest_category_over_the_spot() {
        let feats = vec![
            square(FeatureKind::Outlook, "Day 1: MRGL"),
            square(FeatureKind::Outlook, "Day 1: ENH"),
            square(FeatureKind::Outlook, "Day 1: SLGT"),
        ];
        assert_eq!(outlook_at(&feats, -97.5, 35.5), Some(Risk::Enh));
        // The live feed's LABEL2 is the long name.
        let long = vec![
            square(FeatureKind::Outlook, "Day 1: General Thunderstorms Risk"),
            square(FeatureKind::Outlook, "Day 1: Slight Risk"),
        ];
        assert_eq!(outlook_at(&long, -97.5, 35.5), Some(Risk::Slgt));
        assert_eq!(
            outlook_at(&feats, -90.0, 35.5),
            None,
            "outside every polygon"
        );
    }

    #[test]
    fn watches_are_one_row_each_tornado_first() {
        let boxes = vec![
            square(FeatureKind::WatchBox, "Severe Thunderstorm Watch 0637"),
            square(FeatureKind::WatchBox, "Tornado Watch 0638"),
            square(FeatureKind::WatchBox, "Tornado Watch 0638"),
        ];
        let w = watches_at(&boxes, &[], -97.5, 35.5);
        assert_eq!(w.len(), 2);
        assert!(w[0].tornado && !w[1].tornado);
        assert!(watches_at(&boxes, &[], -80.0, 35.5).is_empty());
    }

    #[test]
    fn a_storm_headed_this_way_is_a_track_and_one_moving_off_is_not() {
        // 30 km west of the spot, moving east at 30 kt: over the spot in about half an hour.
        let spot = [-97.5, 35.5];
        let coming = cell("K4", -97.83, 35.5, 90.0, 30.0);
        let going = cell("J2", -97.83, 35.5, 270.0, 30.0);
        let t = tracks_toward(&[coming, going], spot);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].id, "K4");
        assert!(
            (25.0..=40.0).contains(&t[0].eta_min),
            "eta {}",
            t[0].eta_min
        );
        assert!(t[0].pass_km < 3.0, "passes over: {}", t[0].pass_km);
        assert!(
            t[0].plain().contains("right over y'all"),
            "{}",
            t[0].plain()
        );
    }

    #[test]
    fn the_meter_climbs_with_the_threat() {
        assert_eq!(meter(&[], &[], None, &[]).level, 0);
        assert_eq!(meter(&[], &[], Some(Risk::Slgt), &[]).level, 2);
        let boxes = vec![square(FeatureKind::WatchBox, "Tornado Watch 0638")];
        let w = watches_at(&boxes, &[], -97.5, 35.5);
        let m = meter(&[], &w, Some(Risk::Slgt), &[]);
        assert_eq!(m.level, 3);
        assert_eq!(m.reasons[0], "Tornado Watch over y'all", "the watch leads");
        let svr = alert("Severe Thunderstorm Warning");
        assert_eq!(meter(&[&svr], &w, None, &[]).level, 4);
        let tor = alert("Tornado Warning");
        let m = meter(&[&svr, &tor], &w, Some(Risk::Mdt), &[]);
        assert_eq!(m.level, 5);
        assert_eq!(m.headline(), "Y'ALL TAKE COVER NOW");
        assert_eq!(m.reasons.len(), 4);
    }

    #[test]
    fn a_rotating_storm_close_and_soon_is_take_action() {
        let spot = [-97.5, 35.5];
        let mut c = cell("K4", -97.7, 35.5, 90.0, 30.0);
        c.meso = Some("M3".into());
        let t = tracks_toward(&[c], spot);
        assert_eq!(meter(&[], &[], None, &t).level, 4);
        assert!(t[0].plain().ends_with("and it's rotating"));
    }
}
