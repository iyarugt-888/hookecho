//! The impact report (WeatherWise-class "impact/analysis report"; ROADMAP_PARITY M2.3): for every
//! manual storm motion, when it reaches each saved place, watch zone, imported impact target and
//! town already found in its path, and how many people its hour's swath holds when that was
//! counted. Written into the analysis export as `impacts.csv` (one row per arrival) and
//! `impacts.md` (the same, readable, motion by motion).
//!
//! Built only from what the session already has: nothing is looked up while exporting, and a
//! lookup that was never asked for or failed is said as such rather than left out silently.
//! Arrivals are estimates from the analyst's own motion and uncertainty swath, stated from the
//! motion's analysis time, never presented as observations.

use super::community_targets::TownsState;
use super::gis_layers::{Target, TargetShape};
use super::impact::{summary, towns, ImpactState};
use super::storm_track::{compass, ManualTrack};
use chrono::{DateTime, Duration, Utc};

/// One arrival: a track, a target and when the projection reaches it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ImpactRow {
    /// 1-based, as the motion card numbers tracks.
    pub track: usize,
    pub target: String,
    /// "marker", "zone", "gis-point", "gis-area" or "town".
    pub kind: &'static str,
    /// The layer or list it came from.
    pub source: String,
    pub arrival: DateTime<Utc>,
    /// Minutes after the motion's analysis time.
    pub minutes: f64,
    /// Closest approach of the storm's centre (or line), km; `None` for areas.
    pub closest_km: Option<f64>,
    /// "N", "SE"…: where the storm passes relative to the target, as the motion card says it
    /// ("passes 3 mi S"); `None` for a direct hit or an area.
    pub side: Option<&'static str>,
    /// Inside the projected path (a point) or entered by it (an area), rather than reached
    /// only by the swath's edge or passed beside.
    pub in_path: bool,
    /// The target is already inside the path at the motion's time.
    pub inside_now: bool,
}

/// What the report knows about one track beyond its arrivals.
pub(crate) struct TrackContext<'a> {
    pub track: &'a ManualTrack,
    /// The towns lookup for its swath, if one was made.
    pub towns: Option<&'a TownsState>,
    /// The population count for its hour's swath, if one was made.
    pub people: Option<&'a ImpactState>,
}

/// Every arrival, track by track, in-path first then soonest within a track.
pub(crate) fn impact_rows(
    tracks: &[TrackContext<'_>],
    markers: &[(String, [f64; 2])],
    zones: &[(String, Vec<[f64; 2]>)],
    targets: &[Target],
) -> Vec<ImpactRow> {
    let mut out = Vec::new();
    for (i, ctx) in tracks.iter().enumerate() {
        let t = ctx.track;
        let mut rows = Vec::new();
        let mut point = |name: &str, kind: &'static str, source: &str, p: [f64; 2]| {
            if let Some(e) = t.eta(p) {
                let direct = e.closest_km < 0.5;
                rows.push(ImpactRow {
                    track: i + 1,
                    target: name.to_string(),
                    kind,
                    source: source.to_string(),
                    arrival: t.t0 + Duration::seconds((e.minutes * 60.0) as i64),
                    minutes: e.minutes,
                    closest_km: Some(e.closest_km),
                    side: (!direct)
                        .then(|| compass(t.bearing_deg + if e.right { -90.0 } else { 90.0 })),
                    in_path: e.in_path,
                    inside_now: false,
                });
            }
        };
        for (name, p) in markers {
            point(name, "marker", "Saved places", *p);
        }
        let town_list: Vec<Target> = match ctx.towns {
            Some(TownsState::Ready(places)) => places
                .iter()
                .map(super::community_targets::place_target)
                .collect(),
            _ => Vec::new(),
        };
        for (list, kind) in [(targets, "gis-point"), (&town_list[..], "town")] {
            for target in list {
                if let TargetShape::Point(p) = target.shape {
                    point(&target.name, kind, &target.layer, p);
                }
            }
        }
        let mut area = |name: &str, kind: &'static str, source: &str, ring: &[[f64; 2]]| {
            if let Some(z) = t.zone_eta(ring) {
                rows.push(ImpactRow {
                    track: i + 1,
                    target: name.to_string(),
                    kind,
                    source: source.to_string(),
                    arrival: t.t0 + Duration::seconds((z.minutes * 60.0) as i64),
                    minutes: z.minutes,
                    closest_km: None,
                    side: None,
                    in_path: !z.grazes,
                    inside_now: z.minutes == 0.0 && !z.grazes,
                });
            }
        };
        for (name, ring) in zones {
            area(name, "zone", "Watch zones", ring);
        }
        for target in targets {
            if let TargetShape::Area(ring) = &target.shape {
                area(&target.name, "gis-area", &target.layer, ring);
            }
        }
        rows.sort_by(|a, b| {
            b.in_path
                .cmp(&a.in_path)
                .then(a.minutes.total_cmp(&b.minutes))
        });
        out.extend(rows);
    }
    out
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// `impacts.csv`: one row per arrival, times UTC, distances km.
pub(crate) fn impacts_csv(rows: &[ImpactRow]) -> String {
    let mut s = String::from(
        "track,target,kind,source,arrival_utc,minutes_after_motion,closest_km,passes_side,\
         in_path,inside_now\n",
    );
    for r in rows {
        s.push_str(&format!(
            "{},{},{},{},{},{:.1},{},{},{},{}\n",
            r.track,
            csv_field(&r.target),
            r.kind,
            csv_field(&r.source),
            r.arrival.format("%Y-%m-%dT%H:%M:%SZ"),
            r.minutes,
            r.closest_km.map_or_else(String::new, |k| format!("{k:.2}")),
            r.side.unwrap_or(""),
            r.in_path,
            r.inside_now,
        ));
    }
    s
}

/// `impacts.md`: per motion, its parameters, the people in its hour's swath, then each arrival.
pub(crate) fn impacts_markdown(
    tracks: &[TrackContext<'_>],
    rows: &[ImpactRow],
    exported: DateTime<Utc>,
) -> String {
    let mut s = format!(
        "# Storm impacts\n\nExported {}. Arrival times are estimates from the manual storm \
         motions below and their uncertainty swaths, counted from each motion's own analysis \
         time. Town populations are whole-town 2020 Census figures at the town's centre point; \
         a town's edge can be reached before its centre.\n",
        exported.format("%Y-%m-%d %H:%MZ")
    );
    if tracks.is_empty() {
        s.push_str("\nNo storm motion was set, so there are no arrivals to report.\n");
        return s;
    }
    for (i, ctx) in tracks.iter().enumerate() {
        let t = ctx.track;
        let kt = t.speed_kmh / super::storm_track::KMH_PER_KT;
        s.push_str(&format!(
            "\n## Motion #{}{}\n\nFrom {:.3}, {:.3} at {}, toward {:03.0}\u{b0} ({}) at {:.0} kt; \
             swath {:.1} km left, {:.1} km right, widening {:.0}\u{b0}.{}\n\n",
            i + 1,
            if t.is_line() { " (line)" } else { "" },
            t.origin[1],
            t.origin[0],
            t.t0.format("%Y-%m-%d %H:%MZ"),
            t.bearing_deg,
            compass(t.bearing_deg),
            kt,
            t.left_width_km,
            t.right_width_km,
            t.cone_deg,
            if t.historical {
                " Reopened from a case: an estimate for its own time."
            } else {
                ""
            }
        ));
        s.push_str(&match ctx.people {
            Some(ImpactState::Ready(imp)) => {
                let mut line = format!("People in the next hour's swath: {}.", summary(imp));
                if !imp.places.is_empty() {
                    line.push_str(&format!(" Largest towns: {}.", towns(imp)));
                }
                line + "\n"
            }
            Some(ImpactState::Pending) => "People in path: the count was still running.\n".into(),
            Some(ImpactState::Failed) => "People in path: the count failed.\n".into(),
            None => {
                "People in path: not counted (\u{201c}People in path\u{201d} on the motion card).\n"
                    .into()
            }
        });
        s.push_str(match ctx.towns {
            Some(TownsState::Ready(_)) => "",
            Some(TownsState::Pending) => "Towns in path: the lookup was still running.\n",
            Some(TownsState::Failed(_)) => "Towns in path: the lookup failed.\n",
            None => "Towns in path: not looked up.\n",
        });
        let mine: Vec<&ImpactRow> = rows.iter().filter(|r| r.track == i + 1).collect();
        if mine.is_empty() {
            s.push_str("\nNothing saved lies ahead of this motion within two hours.\n");
            continue;
        }
        s.push_str("\n| | Target | From | Arrives (UTC) | | How |\n|---|---|---|---|---|---|\n");
        for r in mine {
            let how = match (r.closest_km, r.side, r.inside_now, r.in_path) {
                (_, _, true, _) => "inside at the motion's time".to_string(),
                (None, _, _, true) => "path enters".to_string(),
                (None, _, _, false) => "only the swath's edge reaches it".to_string(),
                (Some(_), None, _, _) => "direct hit".to_string(),
                (Some(km), Some(side), _, _) => format!("passes {km:.1} km {side}"),
            };
            s.push_str(&format!(
                "| {} | {} | {} | {} | +{:.0} min | {} |\n",
                if r.in_path { "\u{25cf}" } else { "\u{25cb}" },
                r.target.replace('|', "/"),
                r.source.replace('|', "/"),
                r.arrival.format("%H:%M"),
                r.minutes,
                how
            ));
        }
    }
    s
}

impl super::HookEchoApp {
    /// The impact report's two files for the analysis export, from the session's tracks,
    /// places, zones, impact targets and finished lookups.
    pub(crate) fn impact_report(&self, now: DateTime<Utc>) -> (String, String) {
        let markers: Vec<(String, [f64; 2])> = self
            .settings
            .markers
            .iter()
            .map(|m| (m.name.clone(), [m.lon, m.lat]))
            .collect();
        let zones: Vec<(String, Vec<[f64; 2]>)> = self
            .settings
            .alert_polygons
            .iter()
            .map(|z| (z.name.clone(), z.ring.clone()))
            .collect();
        let (targets, _) = self.impact_targets();
        let contexts: Vec<TrackContext<'_>> = self
            .storm_tracks
            .tracks
            .iter()
            .map(|t| {
                let id = t.impact_id();
                TrackContext {
                    track: t,
                    towns: self.towns.state(&id),
                    people: self.impacts.by_id.get(&id),
                }
            })
            .collect();
        let rows = impact_rows(&contexts, &markers, &zones, &targets);
        (impacts_csv(&rows), impacts_markdown(&contexts, &rows, now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::destination_point;
    use chrono::TimeZone;
    use wxdata::census::{Impact, Place};

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2013, 5, 20, 19, 40, 0).unwrap()
    }

    /// Due east at 60 km/h from 35N 97W, a 3 km band each side.
    fn track() -> ManualTrack {
        let mut t = ManualTrack::new([-97.0, 35.0], t0());
        t.aim(destination_point([-97.0, 35.0], 90.0, 60.0), false);
        t.cone_deg = 0.0;
        t
    }

    fn square(center: [f64; 2], half_km: f64) -> Vec<[f64; 2]> {
        [45.0f64, 135.0, 225.0, 315.0]
            .iter()
            .map(|b| destination_point(center, *b, half_km * std::f64::consts::SQRT_2))
            .collect()
    }

    #[test]
    fn every_kind_of_target_gets_its_arrival_in_path_first() {
        let t = track();
        let on_path = destination_point([-97.0, 35.0], 90.0, 30.0);
        let beside = destination_point(destination_point([-97.0, 35.0], 90.0, 15.0), 0.0, 10.0);
        let behind = destination_point([-97.0, 35.0], 270.0, 20.0);
        let markers = vec![
            ("Beside".to_string(), beside),
            ("Behind".to_string(), behind),
            ("On path".to_string(), on_path),
        ];
        let zones = vec![(
            "Ahead zone".to_string(),
            square(destination_point([-97.0, 35.0], 90.0, 45.0), 2.0),
        )];
        let targets = vec![Target {
            name: "Siren 4".into(),
            layer: "sirens.geojson".into(),
            shape: TargetShape::Point(destination_point([-97.0, 35.0], 90.0, 50.0)),
        }];
        let towns = TownsState::Ready(vec![Place {
            geoid: "1".into(),
            name: "Moore city".into(),
            population: 55_081,
            lon: destination_point([-97.0, 35.0], 90.0, 20.0)[0],
            lat: destination_point([-97.0, 35.0], 90.0, 20.0)[1],
        }]);
        let ctx = [TrackContext {
            track: &t,
            towns: Some(&towns),
            people: None,
        }];
        let rows = impact_rows(&ctx, &markers, &zones, &targets);
        let names: Vec<&str> = rows.iter().map(|r| r.target.as_str()).collect();
        assert!(!names.contains(&"Behind"), "{names:?}");
        assert_eq!(
            names,
            [
                "Moore, pop. 55,081",
                "On path",
                "Ahead zone",
                "Siren 4",
                "Beside"
            ],
            "in-path soonest first, then beside"
        );
        let on = &rows[1];
        assert!((on.minutes - 30.0).abs() < 0.5, "{on:?}");
        // Spherical geometry: within seconds of the flat 30 minutes.
        assert!(
            (on.arrival - (t0() + Duration::minutes(30)))
                .num_seconds()
                .abs()
                <= 5
        );
        assert_eq!((on.kind, on.side, on.in_path), ("marker", None, true));
        assert_eq!(rows[0].kind, "town");
        assert_eq!(rows[2].kind, "zone");
        assert_eq!(rows[3].kind, "gis-point");
        let b = &rows[4];
        // The place is north of the track, so the storm passes south of it.
        assert_eq!((b.side, b.in_path), (Some("S"), false));
        assert!((b.closest_km.unwrap() - 10.0).abs() < 0.3, "{b:?}");

        let csv = impacts_csv(&rows);
        assert_eq!(csv.lines().count(), rows.len() + 1);
        assert!(
            csv.contains("1,\"Moore, pop. 55,081\",town,\"Census 2020, town centre\","),
            "{csv}"
        );
        assert!(
            csv.contains("1,On path,marker,Saved places,2013-05-20T20:10:"),
            "{csv}"
        );
    }

    #[test]
    fn the_readable_report_says_what_was_and_was_not_looked_up() {
        let t = track();
        let people = ImpactState::Ready(Impact {
            population: 20_860,
            housing_units: 8_052,
            places: vec![("Moore city".into(), 55_081)],
        });
        let ctx = [TrackContext {
            track: &t,
            towns: None,
            people: Some(&people),
        }];
        let md = impacts_markdown(&ctx, &[], t0());
        assert!(md.contains("## Motion #1"), "{md}");
        assert!(md.contains("toward 090\u{b0} (E) at 32 kt"), "{md}");
        assert!(md.contains("About 20,860 people"), "{md}");
        assert!(md.contains("Towns in path: not looked up."), "{md}");
        assert!(md.contains("Nothing saved lies ahead"), "{md}");
        let ctx = [TrackContext {
            track: &t,
            towns: Some(&TownsState::Failed("timeout".into())),
            people: None,
        }];
        let md = impacts_markdown(&ctx, &[], t0());
        assert!(md.contains("People in path: not counted"), "{md}");
        assert!(md.contains("Towns in path: the lookup failed."), "{md}");
        assert!(impacts_markdown(&[], &[], t0()).contains("No storm motion was set"));
    }
}
