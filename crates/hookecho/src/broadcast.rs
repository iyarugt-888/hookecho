//! Broadcast styling (ROADMAP_NEW M1): what a picture meant for air or a stream carries beyond the
//! map, and where — a title-safe margin, the colour scale, the caption, a clock, a warning crawl
//! and a logo. One [`Broadcast`] drives both the off-screen renders (`--watch`, painted by
//! `crate::chrome`) and the app's streaming mode (drawn with egui), so a stream and a rendered
//! file look alike.

use chrono::{DateTime, Utc};

/// How a broadcast frame is dressed.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Broadcast {
    /// Title-safe margin, percent of the shorter edge (0..=15). Nothing drawn here comes nearer
    /// the edge than this; 5 % is the broadcast convention for graphics.
    pub safe_margin_pct: f32,
    /// The product's colour scale.
    pub legend: bool,
    /// The one-line source caption: site, product, tilt, time, whose render.
    pub caption: bool,
    /// The valid time, large, top right, with the date and zone under it.
    pub clock: bool,
    /// A band along the bottom naming the warnings in force.
    pub crawl: bool,
    /// An image for the top-left corner (a station or channel logo), by path.
    pub logo: Option<String>,
}

impl Default for Broadcast {
    /// Streaming mode's defaults: safe margins, the scale, the caption, the clock and the crawl.
    fn default() -> Self {
        Broadcast {
            safe_margin_pct: 5.0,
            legend: true,
            caption: true,
            clock: true,
            crawl: true,
            logo: None,
        }
    }
}

impl Broadcast {
    /// A plain render: the scale and the caption at the ordinary inset, nothing else — what
    /// `--watch` and the server draw unless asked for more.
    pub fn plain() -> Self {
        Broadcast {
            safe_margin_pct: 0.0,
            legend: true,
            caption: true,
            clock: false,
            crawl: false,
            logo: None,
        }
    }

    /// The safe margin in pixels for a `w`x`h` frame (the percentage clamped to 0..=15).
    pub fn margin_px(&self, w: f32, h: f32) -> f32 {
        self.safe_margin_pct.clamp(0.0, 15.0) / 100.0 * w.min(h)
    }
}

/// The clock's two lines for a frame valid at `time`: the time in the radar's own zone, and the
/// site and date under it.
pub fn clock_lines(site: &str, time: DateTime<Utc>) -> (String, String) {
    let tz = wxdata::tz::site_tz(site);
    let date = match tz {
        Some(tz) => time.with_timezone(&tz).format("%b %-d, %Y").to_string(),
        None => time.format("%b %-d, %Y").to_string(),
    };
    (
        crate::timefmt::fmt_clock(time, tz, false),
        format!("{site} \u{b7} {date}"),
    )
}

/// The warnings the crawl names: the event and its area, soonest-expiring first, one per event
/// (a warning drawn as several polygons is still one warning). Only those still in force at
/// `valid` — an expired one says nothing about the picture.
pub fn crawl_items(features: &[wxdata::overlay::GeoFeature], valid: DateTime<Utc>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut items: Vec<(Option<DateTime<Utc>>, String)> = features
        .iter()
        .filter_map(|f| f.alert.as_ref().map(|a| (f, a)))
        .filter(|(_, a)| a.expires.is_none_or(|e| e > valid))
        .filter(|(_, a)| seen.insert(a.id.clone()))
        .map(|(f, a)| {
            let area = a.area.split(';').next().unwrap_or("").trim();
            let text = if area.is_empty() {
                f.title.clone()
            } else {
                format!("{} \u{2014} {area}", f.title)
            };
            (a.expires, text)
        })
        .collect();
    items.sort_by_key(|(expires, _)| expires.unwrap_or(DateTime::<Utc>::MAX_UTC));
    items.into_iter().map(|(_, t)| t).collect()
}

/// The crawl's one line: the items joined, or `None` when nothing is in force.
pub fn crawl_line(items: &[String]) -> Option<String> {
    (!items.is_empty()).then(|| items.join("   \u{2022}   "))
}

/// The alert feed only describes the present, so a frame older than this gets no crawl rather
/// than today's warnings over yesterday's radar.
pub const CRAWL_MAX_AGE_MIN: i64 = 30;

/// Whether a frame valid at `valid` is recent enough, at `now`, for the live feed to describe it.
pub fn crawl_applies(valid: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    (now - valid).num_minutes().abs() <= CRAWL_MAX_AGE_MIN
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use wxdata::overlay::{AlertInfo, FeatureKind, GeoFeature};

    fn warning(id: &str, title: &str, area: &str, expires_h: Option<u32>) -> GeoFeature {
        GeoFeature {
            rings: Vec::new(),
            fill: [0; 4],
            stroke: [0; 4],
            kind: FeatureKind::Warning,
            title: title.into(),
            detail: String::new(),
            alert: Some(AlertInfo {
                id: id.into(),
                event: title.into(),
                area: area.into(),
                expires: expires_h.map(|h| Utc.with_ymd_and_hms(2013, 5, 20, h, 0, 0).unwrap()),
                headline: String::new(),
                description: String::new(),
                instruction: String::new(),
                max_hail_in: None,
                max_wind: None,
                tornado_detection: None,
                damage_threat: None,
                source: None,
                motion: None,
                vtec: None,
            }),
        }
    }

    #[test]
    fn the_crawl_names_what_is_in_force_once_each_soonest_first() {
        let valid = Utc.with_ymd_and_hms(2013, 5, 20, 20, 30, 0).unwrap();
        let feats = vec![
            warning(
                "a",
                "Severe Thunderstorm Warning",
                "Tulsa, OK; Creek, OK",
                Some(22),
            ),
            warning("b", "Tornado Warning", "Cleveland, OK", Some(21)),
            warning("b", "Tornado Warning", "Cleveland, OK", Some(21)),
            warning("c", "Flash Flood Warning", "Oklahoma, OK", Some(20)),
        ];
        let items = crawl_items(&feats, valid);
        assert_eq!(
            items,
            [
                "Tornado Warning \u{2014} Cleveland, OK",
                "Severe Thunderstorm Warning \u{2014} Tulsa, OK",
            ]
        );
        assert!(crawl_line(&items).unwrap().contains("\u{2022}"));
        assert_eq!(crawl_line(&[]), None);
    }

    #[test]
    fn only_a_recent_frame_gets_the_live_feeds_crawl() {
        let now = Utc.with_ymd_and_hms(2026, 9, 25, 18, 0, 0).unwrap();
        assert!(crawl_applies(now - chrono::Duration::minutes(12), now));
        assert!(!crawl_applies(
            Utc.with_ymd_and_hms(2013, 5, 20, 20, 8, 0).unwrap(),
            now
        ));
    }

    #[test]
    fn the_margin_is_a_share_of_the_shorter_edge_and_capped() {
        let b = Broadcast::default();
        assert!((b.margin_px(1920.0, 1080.0) - 54.0).abs() < 0.01);
        let wide = Broadcast {
            safe_margin_pct: 40.0,
            ..b
        };
        assert!((wide.margin_px(1000.0, 1000.0) - 150.0).abs() < 0.01);
        assert_eq!(Broadcast::plain().margin_px(1920.0, 1080.0), 0.0);
    }

    #[test]
    fn the_clock_reads_in_the_radars_own_zone() {
        let t = Utc.with_ymd_and_hms(2013, 5, 20, 20, 8, 11).unwrap();
        assert_eq!(
            clock_lines("KTLX", t),
            (
                "3:08 PM CDT".to_string(),
                "KTLX \u{b7} May 20, 2013".to_string()
            )
        );
    }

    #[test]
    fn a_saved_style_missing_new_fields_still_loads() {
        let b: Broadcast = serde_json::from_str(r#"{"clock": false}"#).unwrap();
        assert!(!b.clock && b.legend && b.safe_margin_pct == 5.0);
    }
}
