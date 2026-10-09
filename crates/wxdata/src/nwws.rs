//! NWS warning text products, as the NOAA Weather Wire Service (NWWS-OI) pushes them, read into
//! the same [`GeoFeature`]s as the alerts feed (1008.md A1, increment 2).
//!
//! The alerts feed (`api.weather.gov`) is polled; NWWS-OI pushes each product as it is issued, but
//! needs the user's own NWWS account and a relay to hold the connection. This module is the part
//! that does not depend on how the text arrives: it parses a warning product (TOR, SVR, FFW, SMW,
//! SQW, EWW, DSW) or its follow-up statement (SVS, FFS, MWS) into one feature per segment, from
//! the segment's own P-VTEC line, `LAT...LON` polygon, `TIME...MOT...LOC` line and threat tags,
//! and [`merge`] adds to the polled alerts only the events the feed has not published yet, keyed by
//! VTEC event, so nothing is drawn twice.
//!
//! Each feature's alert id starts with [`ID_PREFIX`], so a card can say it came from the wire
//! ([`from_wire`]). Its `issued` time is the product's WMO header time (day, hour and minute, read
//! against the time the text was received to supply the month and year); a header that cannot be
//! read leaves it unknown. A segment without a VTEC line or a polygon is skipped: without either
//! there is no event to dedupe or nothing to draw.

use crate::alerts;
use crate::overlay::{AlertInfo, GeoFeature, StormMotion};
use crate::vtec::{Action, Vtec};
use chrono::{DateTime, Datelike, TimeZone, Utc};

/// The start of every wire-sourced alert id.
pub const ID_PREFIX: &str = "nwws-oi:";

/// Whether an alert came from the wire rather than the polled feed.
pub fn from_wire(a: &AlertInfo) -> bool {
    a.id.starts_with(ID_PREFIX)
}

/// The phenomenon a VTEC code names, as NWS event strings write it.
fn phenomenon(code: &str) -> Option<&'static str> {
    Some(match code {
        "TO" => "Tornado",
        "SV" => "Severe Thunderstorm",
        "FF" => "Flash Flood",
        "MA" => "Special Marine",
        "EW" => "Extreme Wind",
        "SQ" => "Snow Squall",
        "DS" => "Dust Storm",
        _ => return None,
    })
}

fn significance(code: &str) -> Option<&'static str> {
    Some(match code {
        "W" => "Warning",
        "A" => "Watch",
        "Y" => "Advisory",
        _ => return None,
    })
}

/// The WMO header's `DDHHMM` against `received`: the day of the month nearest it, in the month
/// before, of or after the receipt.
fn header_time(text: &str, received: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let stamp = text.lines().find_map(|l| {
        let mut f = l.split_whitespace();
        let (tt, cccc, ddhhmm) = (f.next()?, f.next()?, f.next()?);
        (tt.len() == 6 && cccc.len() == 4 && ddhhmm.len() == 6)
            .then_some(ddhhmm)
            .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
    })?;
    let n = |r: std::ops::Range<usize>| stamp[r].parse::<u32>().ok();
    let (day, hour, minute) = (n(0..2)?, n(2..4)?, n(4..6)?);
    let (y, m) = (received.year(), received.month());
    [(-1i32), 0, 1]
        .iter()
        .filter_map(|dm| {
            let months = y * 12 + m as i32 - 1 + dm;
            Utc.with_ymd_and_hms(
                months.div_euclid(12),
                months.rem_euclid(12) as u32 + 1,
                day,
                hour,
                minute,
                0,
            )
            .single()
        })
        .min_by_key(|t| (*t - received).num_seconds().abs())
}

/// `LAT...LON 4256 9492 4273 9491 ...`, continued on indented lines, as `[lon, lat]` (west
/// longitudes are written positive, and 5-digit ones past 100 W).
fn polygon(segment: &str) -> Option<Vec<[f64; 2]>> {
    let mut lines = segment.lines().skip_while(|l| !l.starts_with("LAT...LON"));
    let first = lines.next()?.trim_start_matches("LAT...LON");
    let mut numbers: Vec<f64> = Vec::new();
    for chunk in std::iter::once(first).chain(lines.take_while(|l| l.starts_with(' '))) {
        for w in chunk.split_whitespace() {
            numbers.push(w.parse::<f64>().ok()? / 100.0);
        }
    }
    let mut ring: Vec<[f64; 2]> = numbers
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| [-p[1], p[0]])
        .collect();
    if ring.len() < 3 {
        return None;
    }
    if ring.first() != ring.last() {
        ring.push(ring[0]);
    }
    Some(ring)
}

/// `TIME...MOT...LOC 0506Z 300DEG 20KT 4261 9502`: direction from, speed and centroid points.
fn motion(segment: &str) -> Option<StormMotion> {
    let line = segment
        .lines()
        .find_map(|l| l.strip_prefix("TIME...MOT...LOC"))?;
    let f: Vec<&str> = line.split_whitespace().collect();
    let deg = f.get(1)?.strip_suffix("DEG")?.parse().ok()?;
    let kt = f.get(2)?.strip_suffix("KT")?.parse().ok()?;
    let coords: Vec<f64> = f[3..]
        .iter()
        .filter_map(|w| w.parse::<f64>().ok())
        .map(|v| v / 100.0)
        .collect();
    let points = coords
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| [-p[1], p[0]])
        .collect();
    Some(StormMotion { deg, kt, points })
}

/// The value after a `TAG...` line, e.g. `MAX HAIL SIZE...1.00 IN` -> `1.00 IN`.
fn tag(segment: &str, name: &str) -> Option<String> {
    segment.lines().find_map(|l| {
        l.strip_prefix(name)
            .and_then(|r| r.strip_prefix("..."))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

/// Where: the zone-name line of a statement (`Greene IA-Guthrie IA-`), else the bulleted
/// counties of a new warning, else the UGC codes.
fn area(segment: &str) -> String {
    let lines: Vec<&str> = segment.lines().map(str::trim_end).collect();
    if let Some(names) = lines.iter().find(|l| {
        l.ends_with('-') && l.chars().any(|c| c.is_ascii_lowercase()) && !l.starts_with('/')
    }) {
        return names
            .trim_end_matches('-')
            .split('-')
            .collect::<Vec<_>>()
            .join("; ");
    }
    if let Some(i) = lines
        .iter()
        .position(|l| l.starts_with("* ") && l.ends_with("for..."))
    {
        let parts: Vec<String> = lines[i + 1..]
            .iter()
            .take_while(|l| l.starts_with("  "))
            .map(|l| l.trim().trim_end_matches("...").to_string())
            .collect();
        if !parts.is_empty() {
            return parts.join("; ");
        }
    }
    lines
        .iter()
        .find(|l| l.len() > 6 && l.ends_with('-') && l[..2].chars().all(|c| c.is_ascii_uppercase()))
        .map_or_else(String::new, |l| l.to_string())
}

/// The VTEC end time `YYMMDDTHHMMZ`.
fn vtec_end(line: &str) -> Option<DateTime<Utc>> {
    let end = line.trim().trim_matches('/').rsplit('-').next()?;
    let t = chrono::NaiveDateTime::parse_from_str(end, "%y%m%dT%H%MZ").ok()?;
    Some(Utc.from_utc_datetime(&t))
}

/// Every warning segment in one product `text`, received at `received`. Ended events (CAN, EXP)
/// are returned too, so [`merge`] can drop them.
pub fn parse_product(text: &str, received: DateTime<Utc>) -> Vec<GeoFeature> {
    let text = text.replace("\r\n", "\n");
    let issued = header_time(&text, received);
    let header: String = text.lines().take(4).collect::<Vec<_>>().join(" ");
    let mut out = Vec::new();
    for (i, segment) in text.split("$$").enumerate() {
        let Some(vtec_line) = segment
            .lines()
            .find(|l| l.trim_start().starts_with("/O.") && l.trim_end().ends_with('/'))
        else {
            continue;
        };
        let Some(v) = Vtec::parse(vtec_line) else {
            continue;
        };
        let (Some(what), Some(sig)) = (phenomenon(&v.phenomenon), significance(&v.significance))
        else {
            continue;
        };
        let Some(ring) = polygon(segment) else {
            continue;
        };
        let event = format!("{what} {sig}");
        let (kind, rgb) = alerts::event_style(&event);
        let max_hail_in = tag(segment, "MAX HAIL SIZE")
            .and_then(|s| s.trim_end_matches(" IN").trim().parse::<f32>().ok());
        let damage = tag(segment, "TORNADO DAMAGE THREAT")
            .or_else(|| tag(segment, "THUNDERSTORM DAMAGE THREAT"))
            .or_else(|| tag(segment, "FLASH FLOOD DAMAGE THREAT"))
            .or_else(|| tag(segment, "SNOW SQUALL IMPACT"));
        let source = segment.lines().find_map(|l| {
            l.trim()
                .strip_prefix("SOURCE...")
                .map(|s| s.trim().trim_end_matches('.').to_string())
        });
        let area = area(segment);
        let headline = format!(
            "{event} ({}) issued by {} via NWWS-OI",
            match v.action {
                Action::New => "new",
                Action::Continued => "continued",
                Action::Extended => "extended",
                Action::Ended => "ended",
                Action::Upgraded => "upgraded",
                Action::Other => "update",
            },
            v.office
        );
        let alert = AlertInfo {
            id: format!("{ID_PREFIX}{header}#{i}"),
            event: event.clone(),
            headline: headline.clone(),
            area: area.clone(),
            description: segment.trim().to_string(),
            instruction: String::new(),
            expires: vtec_end(vtec_line),
            issued,
            effective: issued,
            max_hail_in,
            max_wind: tag(segment, "MAX WIND GUST"),
            tornado_detection: tag(segment, "TORNADO").filter(|s| !s.contains("DAMAGE")),
            damage_threat: damage,
            source,
            motion: motion(segment),
            vtec: Some(vtec_line.trim().to_string()),
        };
        out.push(GeoFeature {
            rings: vec![ring],
            fill: [rgb[0], rgb[1], rgb[2], 45],
            stroke: [rgb[0], rgb[1], rgb[2], 235],
            kind,
            title: event,
            detail: format!("{headline}\n\nArea: {area}\n\n{}", segment.trim()),
            alert: Some(alert),
        });
    }
    out
}

/// The polled alerts with the wire's events added: a wire event the feed already carries (same
/// VTEC event) is left to the feed's message, an ended one is dropped, as is one expired at
/// `now`, and of several wire messages for one event the newest is kept.
pub fn merge(polled: Vec<GeoFeature>, wire: &[GeoFeature], now: DateTime<Utc>) -> Vec<GeoFeature> {
    let key = |f: &GeoFeature| f.alert.as_ref().map(AlertInfo::event_key);
    let known: std::collections::HashSet<String> = polled.iter().filter_map(key).collect();
    let mut newest: std::collections::HashMap<String, &GeoFeature> = Default::default();
    for f in wire {
        let (Some(k), Some(a)) = (key(f), f.alert.as_ref()) else {
            continue;
        };
        if known.contains(&k) {
            continue;
        }
        let later = newest
            .get(&k)
            .and_then(|g| g.alert.as_ref())
            .is_none_or(|g| a.issued >= g.issued);
        if later {
            newest.insert(k, f);
        }
    }
    let mut out = polled;
    let mut added: Vec<&GeoFeature> = newest
        .into_values()
        .filter(|f| {
            let a = f.alert.as_ref().expect("keyed above");
            let ended = a
                .vtec
                .as_deref()
                .and_then(Vtec::parse)
                .is_some_and(|v| v.action == Action::Ended);
            !ended && a.expires.is_none_or(|e| e > now)
        })
        .collect();
    added.sort_by(|a, b| {
        a.alert
            .as_ref()
            .map(|x| &x.id)
            .cmp(&b.alert.as_ref().map(|x| &x.id))
    });
    out.extend(added.into_iter().cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::FeatureKind;

    const SVR: &str = include_str!("../tests/data/nwws/svr_kdmx_0351.txt");
    const TOR: &str = include_str!("../tests/data/nwws/tor_kmob_0039.txt");
    const SVS: &str = include_str!("../tests/data/nwws/svs_kdmx_0349.txt");

    fn at(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn a_new_severe_thunderstorm_warning_reads_like_the_feed() {
        let f = parse_product(SVR, at("2026-10-09T05:06:30Z"));
        assert_eq!(f.len(), 1);
        let a = f[0].alert.as_ref().unwrap();
        assert_eq!(a.event, "Severe Thunderstorm Warning");
        assert_eq!(f[0].kind, FeatureKind::Warning);
        assert_eq!(a.issued, Some(at("2026-10-09T05:06:00Z")));
        assert_eq!(a.expires, Some(at("2026-10-09T05:45:00Z")));
        assert_eq!(a.event_key(), "KDMX.SV.W.0351.2026");
        assert_eq!(a.max_hail_in, Some(1.0));
        assert_eq!(a.max_wind.as_deref(), Some("<50 MPH"));
        assert_eq!(a.source.as_deref(), Some("Radar indicated"));
        assert!(
            a.area.starts_with("Southwestern Pocahontas County"),
            "{}",
            a.area
        );
        assert!(from_wire(a));
        let ring = &f[0].rings[0];
        assert_eq!(ring.len(), 6, "five vertices, closed");
        assert_eq!(ring[0], [-94.92, 42.56]);
        assert_eq!(ring[4], [-95.17, 42.56]);
        let m = a.motion.as_ref().unwrap();
        assert_eq!((m.deg, m.kt), (300.0, 20.0));
        assert_eq!(m.points, vec![[-95.02, 42.61]]);
    }

    #[test]
    fn a_tornado_warning_carries_its_detection_tag() {
        let f = parse_product(TOR, at("2026-10-04T21:02:40Z"));
        let a = f[0].alert.as_ref().unwrap();
        assert_eq!(a.event, "Tornado Warning");
        assert_eq!(a.tornado_detection.as_deref(), Some("RADAR INDICATED"));
        assert_eq!(a.issued, Some(at("2026-10-04T21:02:00Z")));
        assert_eq!(
            a.area,
            "South central Baldwin County in southwestern Alabama"
        );
        assert_eq!(f[0].rings[0][2], [-87.74, 30.29]);
    }

    #[test]
    fn a_statement_continues_its_warning_and_names_its_zones() {
        let f = parse_product(SVS, at("2026-10-09T04:45:20Z"));
        let a = f[0].alert.as_ref().unwrap();
        assert_eq!(a.event, "Severe Thunderstorm Warning");
        assert_eq!(a.event_key(), "KDMX.SV.W.0349.2026");
        assert_eq!(a.area, "Greene IA; Guthrie IA; Boone IA; Dallas IA");
        assert!(a.headline.contains("(continued)"), "{}", a.headline);
        assert_eq!(a.expires, Some(at("2026-10-09T05:00:00Z")));
    }

    #[test]
    fn the_header_day_resolves_across_a_month_end() {
        let t = header_time("WUUS53 KDMX 312350\n", at("2026-11-01T00:00:10Z"));
        assert_eq!(t, Some(at("2026-10-31T23:50:00Z")));
        assert_eq!(
            header_time("no header here\n", at("2026-11-01T00:00:10Z")),
            None
        );
    }

    #[test]
    fn the_wire_only_adds_what_the_feed_has_not_published() {
        let now = at("2026-10-09T05:07:00Z");
        let wire_svr = parse_product(SVR, now);
        let wire_svs = parse_product(SVS, at("2026-10-09T04:45:20Z"));
        // The feed already has warning 0349 (as its own message); not 0351.
        let mut polled = wire_svs.clone();
        polled[0].alert.as_mut().unwrap().id = "urn:oid:feed-0349".into();
        let wire: Vec<GeoFeature> = wire_svr.iter().chain(&wire_svs).cloned().collect();
        let merged = merge(polled.clone(), &wire, at("2026-10-09T04:50:00Z"));
        let ids: Vec<&str> = merged
            .iter()
            .filter_map(|f| f.alert.as_ref().map(|a| a.id.as_str()))
            .collect();
        assert_eq!(ids.len(), 2, "{ids:?}");
        assert_eq!(ids[0], "urn:oid:feed-0349", "the feed's message stands");
        assert!(ids[1].starts_with(ID_PREFIX), "{ids:?}");
        // Expired by the time it is merged: not added.
        assert_eq!(
            merge(polled.clone(), &wire_svr, at("2026-10-09T06:00:00Z")).len(),
            1
        );
        // An ended event is not drawn.
        let ended = SVR.replace("/O.NEW.", "/O.CAN.");
        assert_eq!(merge(Vec::new(), &parse_product(&ended, now), now).len(), 0);
        // Two wire messages for one event: the newest.
        let mut older = wire_svr[0].clone();
        older.alert.as_mut().unwrap().issued = Some(at("2026-10-09T05:00:00Z"));
        older.alert.as_mut().unwrap().id = format!("{ID_PREFIX}older");
        let m = merge(Vec::new(), &[older, wire_svr[0].clone()], now);
        assert_eq!(m.len(), 1);
        assert_eq!(
            m[0].alert.as_ref().unwrap().issued,
            Some(at("2026-10-09T05:06:00Z"))
        );
    }
}
