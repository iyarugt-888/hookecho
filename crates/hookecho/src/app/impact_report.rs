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
    /// The SCIT storm id when this is the radar's own automatic motion for a storm nobody set a
    /// manual motion on; `None` for an analyst's motion.
    pub scit_cell: Option<&'a str>,
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

/// Keep every analyst motion, and only the SCIT storms that reach something: the radar names
/// dozens of storms, and the report is for the ones ahead of a place someone saved. Rows are
/// renumbered to the kept tracks. Returns the kept tracks, their rows, and how many SCIT storms
/// with a motion had nothing ahead.
pub(crate) fn keep_reaching<'a>(
    tracks: Vec<TrackContext<'a>>,
    rows: Vec<ImpactRow>,
) -> (Vec<TrackContext<'a>>, Vec<ImpactRow>, usize) {
    let mut kept = Vec::new();
    let mut number = vec![None; tracks.len()];
    let mut quiet = 0;
    for (i, ctx) in tracks.into_iter().enumerate() {
        if ctx.scit_cell.is_some() && !rows.iter().any(|r| r.track == i + 1) {
            quiet += 1;
            continue;
        }
        kept.push(ctx);
        number[i] = Some(kept.len());
    }
    let rows = rows
        .into_iter()
        .filter_map(|mut r| {
            r.track = number[r.track - 1]?;
            Some(r)
        })
        .collect();
    (kept, rows, quiet)
}

/// `impacts.csv`: one row per arrival, times UTC, distances km. `motion` says whose motion the
/// arrival is projected from ("manual", or "scit" for the radar's automatic storm motion), and
/// `storm` names the SCIT storm when it is one.
pub(crate) fn impacts_csv(tracks: &[TrackContext<'_>], rows: &[ImpactRow]) -> String {
    let mut s = String::from(
        "track,target,kind,source,arrival_utc,minutes_after_motion,closest_km,passes_side,\
         in_path,inside_now,motion,storm\n",
    );
    for r in rows {
        let scit = tracks.get(r.track - 1).and_then(|t| t.scit_cell);
        s.push_str(&format!(
            "{},{},{},{},{},{:.1},{},{},{},{},{},{}\n",
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
            if scit.is_some() { "scit" } else { "manual" },
            csv_field(scit.unwrap_or("")),
        ));
    }
    s
}

/// `impacts.md`: per motion, its parameters, the people in its hour's swath, then each arrival.
/// One warning in effect, for the report's warnings section.
pub(crate) struct WarningLine<'a> {
    pub event: &'a str,
    pub area: &'a str,
    pub expires: Option<DateTime<Utc>>,
    /// The people inside its polygon, when its card asked the Census.
    pub people: Option<&'a ImpactState>,
}

/// The warnings section: each warning in effect over the radar's area with the people inside
/// its polygon where that was counted, and said plainly where it was not.
fn warnings_markdown(warnings: &[WarningLine<'_>]) -> String {
    let mut s = String::from("\n## Warnings in effect\n\n");
    if warnings.is_empty() {
        s.push_str("No warning was in effect over the radar's area.\n");
        return s;
    }
    s.push_str(
        "Populations are 2020 Census counts inside each warning's polygon, made when its card \
         was opened; the rest were not counted.\n\n| Warning | Area | Until (UTC) | People inside |\n\
         |---|---|---|---|\n",
    );
    for w in warnings {
        let people = match w.people {
            Some(ImpactState::Ready(imp)) => summary(imp),
            Some(ImpactState::Pending) => "count still running".into(),
            Some(ImpactState::Failed) => "count failed".into(),
            None => "not counted".into(),
        };
        s.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            w.event.replace('|', "/"),
            w.area.replace('|', "/"),
            w.expires
                .map_or_else(|| "—".into(), |e| e.format("%H:%M").to_string()),
            people
        ));
    }
    s
}

pub(crate) fn impacts_markdown(
    tracks: &[TrackContext<'_>],
    rows: &[ImpactRow],
    quiet_scit: usize,
    warnings: &[WarningLine<'_>],
    exported: DateTime<Utc>,
) -> String {
    let mut s = format!(
        "# Storm impacts\n\nExported {}. Arrival times are estimates from the storm motions \
         below and their uncertainty swaths, counted from each motion's own analysis time: the \
         analyst's manual motions, and for storms nobody set one on, the radar's automatic storm \
         motion (SCIT) with the default swath. Town populations are whole-town 2020 Census \
         figures at the town's centre point; a town's edge can be reached before its centre.\n",
        exported.format("%Y-%m-%d %H:%MZ")
    );
    if tracks.is_empty() {
        s.push_str(if quiet_scit > 0 {
            "\nNo manual storm motion was set, and no storm the radar tracks has anything saved \
             ahead of it within two hours.\n"
        } else {
            "\nNo storm motion was set or tracked, so there are no arrivals to report.\n"
        });
        s.push_str(&warnings_markdown(warnings));
        return s;
    }
    for (i, ctx) in tracks.iter().enumerate() {
        let t = ctx.track;
        let kt = t.speed_kmh / super::storm_track::KMH_PER_KT;
        let heading = match ctx.scit_cell {
            Some(id) => format!("Storm {id}, radar's automatic motion"),
            None => format!("Motion #{}", i + 1),
        };
        s.push_str(&format!(
            "\n## {heading}{}\n\nFrom {:.3}, {:.3} at {}, toward {:03.0}\u{b0} ({}) at {:.0} kt; \
             swath {:.1} km left, {:.1} km right, widening {:.0}\u{b0}.{}\n\n",
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
            None if ctx.scit_cell.is_some() => "People in path: not counted.\n".into(),
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
    if quiet_scit > 0 {
        s.push_str(&format!(
            "\n{quiet_scit} more storm{} the radar tracks had nothing saved ahead within two \
             hours.\n",
            if quiet_scit == 1 { "" } else { "s" }
        ));
    }
    s.push_str(&warnings_markdown(warnings));
    s
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `impacts.html`: the readable report as a page that prints cleanly (1008.md B2), rendered from
/// `impacts.md` itself so the two never disagree. Handles exactly what that report writes:
/// `#`/`##` headings, paragraphs and pipe tables; everything is escaped. Each motion starts on a
/// new printed page after the first.
pub(crate) fn impacts_html(md: &str) -> String {
    let mut body = String::new();
    let mut para: Vec<&str> = Vec::new();
    let mut table: Vec<Vec<String>> = Vec::new();
    let mut sections = 0;
    let flush_para = |para: &mut Vec<&str>, body: &mut String| {
        if !para.is_empty() {
            body.push_str(&format!("<p>{}</p>\n", html_escape(&para.join(" "))));
            para.clear();
        }
    };
    let flush_table = |table: &mut Vec<Vec<String>>, body: &mut String| {
        if table.is_empty() {
            return;
        }
        body.push_str("<table>\n");
        for (i, row) in table.iter().enumerate() {
            let cell = if i == 0 { "th" } else { "td" };
            body.push_str("<tr>");
            for c in row {
                body.push_str(&format!("<{cell}>{}</{cell}>", html_escape(c)));
            }
            body.push_str("</tr>\n");
        }
        body.push_str("</table>\n");
        table.clear();
    };
    for line in md.lines() {
        let t = line.trim();
        if t.starts_with('|') {
            flush_para(&mut para, &mut body);
            let cells: Vec<String> = t
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_string())
                .collect();
            // The `|---|---|` separator row carries nothing to show.
            if !cells
                .iter()
                .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-'))
            {
                table.push(cells);
            }
            continue;
        }
        flush_table(&mut table, &mut body);
        if let Some(h) = t.strip_prefix("## ") {
            flush_para(&mut para, &mut body);
            sections += 1;
            let class = if sections > 1 { " class=\"page\"" } else { "" };
            body.push_str(&format!("<h2{class}>{}</h2>\n", html_escape(h)));
        } else if let Some(h) = t.strip_prefix("# ") {
            flush_para(&mut para, &mut body);
            body.push_str(&format!("<h1>{}</h1>\n", html_escape(h)));
        } else if t.is_empty() {
            flush_para(&mut para, &mut body);
        } else {
            para.push(t);
        }
    }
    flush_table(&mut table, &mut body);
    flush_para(&mut para, &mut body);
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>Storm impacts</title>\n<style>\n\
         body {{ font: 11pt/1.45 system-ui, sans-serif; color: #111; background: #fff; \
         max-width: 60rem; margin: 1.5rem auto; padding: 0 1rem; }}\n\
         h1 {{ font-size: 18pt; margin: 0 0 .5rem; }}\n\
         h2 {{ font-size: 13pt; margin: 1.5rem 0 .25rem; border-bottom: 1px solid #999; }}\n\
         table {{ border-collapse: collapse; width: 100%; margin: .5rem 0 1rem; }}\n\
         th, td {{ border: 1px solid #bbb; padding: .2rem .4rem; text-align: left; \
         vertical-align: top; }}\n\
         th {{ background: #eee; }}\n\
         @media print {{ body {{ margin: 0; max-width: none; }} \
         h2.page {{ break-before: page; }} tr {{ break-inside: avoid; }} }}\n\
         </style>\n</head>\n<body>\n{body}</body>\n</html>\n"
    )
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
        // The radar's own motion for each storm on the active site that has one and that nobody
        // set a manual motion on, from its own scan time.
        let site = self.views[self.active].site.as_deref();
        let volume_time = self.views[self.active].volume.as_ref().map(|v| v.time);
        let scit: Vec<(&str, ManualTrack)> = if site.is_some() && self.cells_site.as_deref() == site
        {
            self.storm_cells
                .iter()
                .filter(|c| !c.id.is_empty() && self.manual_tracks_for(c).is_empty())
                .filter_map(|c| {
                    let t0 = c.time.or(volume_time)?;
                    Some((c.id.as_str(), ManualTrack::from_cell(c, t0)?))
                })
                .collect()
        } else {
            Vec::new()
        };
        let contexts: Vec<TrackContext<'_>> = self
            .storm_tracks
            .tracks
            .iter()
            .map(|t| self.track_context(t, None))
            .chain(scit.iter().map(|(id, t)| self.track_context(t, Some(*id))))
            .collect();
        let rows = impact_rows(&contexts, &markers, &zones, &targets);
        let (contexts, rows, quiet) = keep_reaching(contexts, rows);
        let warnings = self.report_warnings();
        (
            impacts_csv(&contexts, &rows),
            impacts_markdown(&contexts, &rows, quiet, &warnings, now),
        )
    }

    fn track_context<'a>(
        &'a self,
        track: &'a ManualTrack,
        scit_cell: Option<&'a str>,
    ) -> TrackContext<'a> {
        let id = track.impact_id();
        TrackContext {
            track,
            scit_cell,
            towns: self.towns.state(&id),
            people: self.impacts.by_id.get(&id),
        }
    }

    /// Warnings in effect within the active radar's 250 km, with the population counts their
    /// cards already made; one line per warning however many polygon parts it has.
    fn report_warnings(&self) -> Vec<WarningLine<'_>> {
        let near = self.active_site_bounds(250.0);
        let mut seen = std::collections::HashSet::new();
        self.alert_features
            .iter()
            .filter(|f| f.kind == wxdata::overlay::FeatureKind::Warning)
            .filter(|f| match (near, f.bbox()) {
                (Some((x0, y0, x1, y1)), Some((a0, b0, a1, b1))) => {
                    a0 <= x1 && a1 >= x0 && b0 <= y1 && b1 >= y0
                }
                (None, _) => true,
                (_, None) => false,
            })
            .filter_map(|f| f.alert.as_ref())
            .filter(|a| seen.insert(a.id.as_str()))
            .map(|a| WarningLine {
                event: &a.event,
                area: &a.area,
                expires: a.expires,
                people: self.impacts.by_id.get(&a.id),
            })
            .collect()
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
            scit_cell: None,
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

        let csv = impacts_csv(&ctx, &rows);
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
            scit_cell: None,
            track: &t,
            towns: None,
            people: Some(&people),
        }];
        let md = impacts_markdown(&ctx, &[], 0, &[], t0());
        assert!(md.contains("## Motion #1"), "{md}");
        assert!(md.contains("toward 090\u{b0} (E) at 32 kt"), "{md}");
        assert!(md.contains("About 20,860 people"), "{md}");
        assert!(md.contains("Towns in path: not looked up."), "{md}");
        assert!(md.contains("Nothing saved lies ahead"), "{md}");
        let ctx = [TrackContext {
            scit_cell: None,
            track: &t,
            towns: Some(&TownsState::Failed("timeout".into())),
            people: None,
        }];
        let md = impacts_markdown(&ctx, &[], 0, &[], t0());
        assert!(md.contains("People in path: not counted"), "{md}");
        assert!(md.contains("Towns in path: the lookup failed."), "{md}");
        assert!(impacts_markdown(&[], &[], 0, &[], t0()).contains("No storm motion was set"));
    }

    /// The printable page carries the report's headings, paragraphs and table rows, escaped,
    /// drops the Markdown separator row, and starts each motion after the first on a new page.
    #[test]
    fn the_printable_page_is_the_readable_report() {
        let t = track();
        let ctx = [
            TrackContext {
                track: &t,
                scit_cell: None,
                towns: None,
                people: None,
            },
            TrackContext {
                track: &t,
                scit_cell: Some("Q4"),
                towns: None,
                people: None,
            },
        ];
        let markers = vec![(
            "Smith & Sons <farm>".to_string(),
            destination_point([-97.0, 35.0], 90.0, 30.0),
        )];
        let rows = impact_rows(&ctx, &markers, &[], &[]);
        let md = impacts_markdown(&ctx, &rows, 0, &[], t0());
        let html = impacts_html(&md);
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<h1>Storm impacts</h1>"));
        assert!(
            html.contains("<h2>Motion #1</h2>"),
            "the first is not page-broken"
        );
        assert!(html.contains("<h2 class=\"page\">Storm Q4, radar's automatic motion</h2>"));
        assert!(
            html.contains("<td>Smith &amp; Sons &lt;farm&gt;</td>"),
            "{html}"
        );
        assert!(html.contains("<th>Target</th>"));
        assert!(!html.contains("---"), "the separator row is not drawn");
        assert!(html.contains("break-before: page"));
        assert!(html.contains("<h2 class=\"page\">Warnings in effect</h2>"));
    }

    /// A storm with only the radar's automatic motion is reported when it reaches a saved place,
    /// labelled as SCIT's in both files; one that reaches nothing is only counted. Warnings are
    /// listed with their counted populations, and an uncounted one says so.
    #[test]
    fn scit_storms_that_reach_something_are_reported_and_labelled() {
        let manual = track();
        let mut reaching = ManualTrack::new([-97.5, 35.0], t0());
        reaching.aim(destination_point([-97.5, 35.0], 90.0, 60.0), false);
        let mut away = ManualTrack::new([-97.0, 35.5], t0());
        away.aim(destination_point([-97.0, 35.5], 0.0, 60.0), false);
        let ctx = |t, scit_cell| TrackContext {
            track: t,
            scit_cell,
            towns: None,
            people: None,
        };
        let contexts = vec![
            ctx(&manual, None),
            ctx(&away, Some("K7")),
            ctx(&reaching, Some("Q4")),
        ];
        // 30 km ahead of the manual motion, and 30 km ahead of storm Q4.
        let markers = vec![
            (
                "School".to_string(),
                destination_point([-97.0, 35.0], 90.0, 30.0),
            ),
            (
                "Farm".to_string(),
                destination_point([-97.5, 35.0], 90.0, 20.0),
            ),
        ];
        let rows = impact_rows(&contexts, &markers, &[], &[]);
        let (kept, rows, quiet) = keep_reaching(contexts, rows);
        assert_eq!(quiet, 1, "K7 reaches nothing");
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[1].scit_cell, Some("Q4"));
        assert!(rows.iter().all(|r| r.track <= 2), "{rows:?}");
        let farm = rows.iter().find(|r| r.target == "Farm" && r.track == 2);
        assert!(farm.is_some(), "Q4's own arrival, renumbered: {rows:?}");

        let csv = impacts_csv(&kept, &rows);
        assert!(csv.lines().next().unwrap().ends_with(",motion,storm"));
        assert!(
            csv.lines()
                .any(|l| l.starts_with("2,Farm,") && l.ends_with(",scit,Q4")),
            "{csv}"
        );
        assert!(
            csv.lines()
                .any(|l| l.starts_with("1,School,") && l.ends_with(",manual,")),
            "{csv}"
        );

        let counted = ImpactState::Ready(Impact {
            population: 4_120,
            housing_units: 1_700,
            places: Vec::new(),
        });
        let warnings = [
            WarningLine {
                event: "Tornado Warning",
                area: "Cleveland, OK",
                expires: Some(t0() + Duration::minutes(45)),
                people: Some(&counted),
            },
            WarningLine {
                event: "Severe Thunderstorm Warning",
                area: "McClain, OK",
                expires: None,
                people: None,
            },
        ];
        let md = impacts_markdown(&kept, &rows, quiet, &warnings, t0());
        assert!(md.contains("## Storm Q4, radar's automatic motion"), "{md}");
        assert!(md.contains("## Motion #1"), "{md}");
        assert!(
            md.contains("1 more storm the radar tracks had nothing saved ahead"),
            "{md}"
        );
        assert!(
            md.contains("| Tornado Warning | Cleveland, OK | 20:25 | About 4,120 people"),
            "{md}"
        );
        assert!(
            md.contains("| Severe Thunderstorm Warning | McClain, OK | — | not counted |"),
            "{md}"
        );
        let none = impacts_markdown(&[], &[], 3, &[], t0());
        assert!(
            none.contains("no storm the radar tracks has anything saved"),
            "{none}"
        );
        assert!(none.contains("No warning was in effect"), "{none}");
    }
}
