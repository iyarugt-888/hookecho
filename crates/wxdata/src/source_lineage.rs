//! Lineage for warnings and surface observations (1008.md A4 item 2, ROADMAP_PARITY M1.4): the
//! same kind of record Tornado ID verdicts carry, for the other layers a reader acts on — each
//! with its own clocks, kept apart, and never filled from another.
//!
//! - **Warnings:** the message (its id, and whether it came from the alerts feed or the NWS
//!   Weather Wire), the VTEC event and action, the event's own begin and end as VTEC states them,
//!   the message's `sent`, `effective` and `expires`, and when this app received it — known only
//!   for messages that arrived while it was running ([`crate::alert_latency`]); otherwise
//!   `null`, not the time of the export.
//! - **Observations:** each station's own observation time (its clock, to the minute), how many
//!   carried none, the oldest and newest, and their ages when the reply was received.

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::overlay::AlertInfo;
use crate::vtec::Vtec;

/// A VTEC string's event begin and end (`YYMMDDTHHMMZ-YYMMDDTHHMMZ`); `000000T0000Z` is "not
/// given" and reads as `None`. `None` overall when the string has no time range.
pub fn vtec_times(vtec: &str) -> Option<(Option<DateTime<Utc>>, Option<DateTime<Utc>>)> {
    let inner = vtec.trim().trim_matches('/');
    let range = inner.split('.').nth(6)?;
    let (a, b) = range.split_once('-')?;
    let t = |s: &str| {
        if s.starts_with("000000") {
            return None;
        }
        chrono::NaiveDateTime::parse_from_str(s, "%y%m%dT%H%MZ")
            .ok()
            .map(|n| n.and_utc())
    };
    Some((t(a), t(b)))
}

fn rfc(t: Option<DateTime<Utc>>) -> Value {
    t.map_or(Value::Null, |t| Value::from(t.to_rfc3339()))
}

/// One warning message's lineage; `received` when this app saw it arrive.
pub fn warning_record(a: &AlertInfo, received: Option<DateTime<Utc>>) -> Value {
    let parsed = a.vtec.as_deref().and_then(Vtec::parse);
    let times = a.vtec.as_deref().and_then(vtec_times);
    json!({
        "id": a.id,
        "event": a.event,
        "feed": if crate::nwws::from_wire(a) { "NWWS-OI" } else { "api.weather.gov" },
        "vtec": a.vtec,
        "vtec_event": parsed.as_ref().map(Vtec::event_key),
        "vtec_action": parsed.as_ref().map(|v| format!("{:?}", v.action)),
        "event_begin_utc": rfc(times.and_then(|t| t.0)),
        "event_end_utc": rfc(times.and_then(|t| t.1)),
        "sent_utc": rfc(a.issued),
        "effective_utc": rfc(a.effective),
        "expires_utc": rfc(a.expires),
        "received_utc": rfc(received),
        "sent_clock": "NWS, stated to the minute",
    })
}

/// A set of station observations' lineage: their own observation times (`obs_time`, seconds
/// since the epoch), and their ages when the reply was `received` (when known).
pub fn observations_record(
    source: &str,
    obs_times: &[Option<i64>],
    received: Option<DateTime<Utc>>,
) -> Value {
    let times: Vec<DateTime<Utc>> = obs_times
        .iter()
        .flatten()
        .filter_map(|&s| DateTime::from_timestamp(s, 0))
        .collect();
    let untimed = obs_times.len() - times.len();
    let oldest = times.iter().min().copied();
    let newest = times.iter().max().copied();
    let mut ages: Vec<i64> = match received {
        Some(r) => times.iter().map(|t| (r - *t).num_seconds()).collect(),
        None => Vec::new(),
    };
    ages.sort_unstable();
    let median = (!ages.is_empty()).then(|| ages[ages.len() / 2]);
    json!({
        "source": source,
        "stations": obs_times.len(),
        "without_observation_time": untimed,
        "oldest_observation_utc": rfc(oldest),
        "newest_observation_utc": rfc(newest),
        "received_utc": rfc(received),
        "median_age_at_receipt_s": median,
        "oldest_age_at_receipt_s": ages.last().copied(),
        "observation_clock": "each station's own, to the minute",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn vtec_times_read_begin_and_end_and_not_given() {
        let (b, e) = vtec_times("/O.NEW.KDMX.SV.W.0351.261009T0506Z-261009T0545Z/").unwrap();
        assert_eq!(b, Some(at("2026-10-09T05:06:00Z")));
        assert_eq!(e, Some(at("2026-10-09T05:45:00Z")));
        let (b, e) = vtec_times("/O.CON.KDMX.SV.W.0349.000000T0000Z-261009T0500Z/").unwrap();
        assert_eq!((b, e), (None, Some(at("2026-10-09T05:00:00Z"))));
        assert_eq!(vtec_times("not a vtec"), None);
    }

    #[test]
    fn a_warning_keeps_its_own_clocks_and_unknown_receipt_stays_null() {
        let text = include_str!("../tests/data/nwws/svs_kdmx_0349.txt");
        let f = crate::nwws::parse_product(text, at("2026-10-09T04:45:20Z"));
        let a = f[0].alert.as_ref().unwrap();
        let r = warning_record(a, None);
        assert_eq!(r["feed"], "NWWS-OI");
        assert_eq!(r["vtec_event"], "KDMX.SV.W.0349.2026");
        assert_eq!(r["vtec_action"], "Continued");
        assert_eq!(
            r["event_begin_utc"],
            Value::Null,
            "a continuation gives no begin"
        );
        assert_eq!(r["event_end_utc"], "2026-10-09T05:00:00+00:00");
        assert_eq!(r["sent_utc"], "2026-10-09T04:45:00+00:00");
        assert_eq!(r["received_utc"], Value::Null);
        let r = warning_record(a, Some(at("2026-10-09T04:45:21Z")));
        assert_eq!(r["received_utc"], "2026-10-09T04:45:21+00:00");
    }

    #[test]
    fn observations_say_their_own_times_and_what_had_none() {
        let t = |s: &str| Some(at(s).timestamp());
        let obs = [
            t("2026-10-09T05:53:00Z"),
            None,
            t("2026-10-09T04:15:00Z"),
            t("2026-10-09T05:56:00Z"),
        ];
        let r = observations_record("METAR", &obs, Some(at("2026-10-09T06:01:00Z")));
        assert_eq!(r["stations"], 4);
        assert_eq!(r["without_observation_time"], 1);
        assert_eq!(r["oldest_observation_utc"], "2026-10-09T04:15:00+00:00");
        assert_eq!(r["median_age_at_receipt_s"], 480);
        assert_eq!(r["oldest_age_at_receipt_s"], 6360);
        let unknown = observations_record("METAR", &obs, None);
        assert_eq!(unknown["median_age_at_receipt_s"], Value::Null);
    }
}
