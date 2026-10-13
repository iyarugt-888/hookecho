//! NWS damage surveys from the Damage Assessment Toolkit (DAT).
//!
//! After a tornado, NWS survey crews walk the path and rate what they find: one point per damage
//! indicator, each with an EF rating, a degree-of-damage description and an estimated wind speed;
//! the fitted path is published as a line. That is ground truth — the only layer in this app that
//! says what the storm actually did, rather than what the radar or a warning thought it would.
//!
//! Source: the public ArcGIS Feature Server the DAT's own viewer calls. No key. Queried as GeoJSON
//! with a bbox and a storm-date window, which is why the layer is archive-aware: scrub the timeline
//! to 2013-05-20 over Moore and the survey lands on top of the hook.
//!
//! Two service quirks, both handled below: the server caps a response at `maxRecordCount` (2000)
//! and sets `exceededTransferLimit`, so pages are pulled with `resultOffset`; and a date comparison
//! against an epoch-millisecond literal is rejected — the `where` clause has to use SQL `timestamp`
//! literals.

use crate::alerts::USER_AGENT;

const SERVER: &str =
    "https://services.dat.noaa.gov/arcgis/rest/services/nws_damageassessmenttoolkit/DamageViewer/FeatureServer";
/// The server's own cap. Asking for more per page is silently clamped to it.
const PAGE: usize = 2000;
/// Stop after this many pages (~20k features) — a continental multi-day query would otherwise
/// page for a long time over a slow link.
const MAX_PAGES: usize = 10;

/// One surveyed damage indicator.
#[derive(Debug, Clone, PartialEq)]
pub struct DamagePoint {
    pub lon: f64,
    pub lat: f64,
    /// "EF0".."EF5", or "EFU" for unknown. Empty when the survey left it blank.
    pub efscale: String,
    /// The damage indicator surveyed, e.g. "One- or Two-Family Residence (FR12)".
    pub damage: String,
    /// The degree of damage observed on that indicator.
    pub dod: String,
    /// Estimated wind speed, mph.
    pub windspeed: Option<i32>,
    pub deaths: i32,
    pub injuries: i32,
    /// Issuing office (WFO) that did the survey.
    pub office: String,
    pub comments: Option<String>,
    /// URL of a survey photo, when one was attached.
    pub image: Option<String>,
    pub storm: Option<chrono::DateTime<chrono::Utc>>,
    /// The surveyed path this indicator belongs to (the path's `globalid`), when the survey
    /// linked it; many points carry an all-zero id instead, and are matched by distance.
    pub path_id: Option<String>,
}

/// A surveyed tornado path.
#[derive(Debug, Clone, PartialEq)]
pub struct DamageTrack {
    /// Path vertices as `[lon, lat]`, in order.
    pub path: Vec<[f64; 2]>,
    pub efscale: String,
    /// Path length in miles and max width in yards, as the survey recorded them.
    pub length_mi: Option<f64>,
    pub width_yd: Option<f64>,
    pub max_wind: Option<i32>,
    pub fatalities: i32,
    pub injuries: i32,
    pub wfo: String,
    pub comments: Option<String>,
    pub storm: Option<chrono::DateTime<chrono::Utc>>,
    /// The path's `globalid`, which its damage points name in `path_guid`.
    pub id: String,
}

/// The rating as a 0–5 number, or `None` when the survey didn't rate it. Surveys write the scale
/// field freehand, so besides "EF3" the real data carries "EF3+" (at least EF3), "EFU", "UNKNOWN",
/// "N/A" and "TSTM/Wind" (straight-line wind, not a tornado) — everything unrated draws grey.
pub fn ef_number(efscale: &str) -> Option<u8> {
    let digits: String = efscale
        .trim()
        .strip_prefix("EF")?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse::<u8>().ok().filter(|n| *n <= 5)
}

fn text(p: &serde_json::Value, k: &str) -> String {
    match p.get(k).and_then(|v| v.as_str()) {
        // The service writes the string "null" into empty text columns.
        Some(s) if !s.is_empty() && s != "null" => s.to_string(),
        _ => String::new(),
    }
}

fn opt_text(p: &serde_json::Value, k: &str) -> Option<String> {
    let s = text(p, k);
    (!s.is_empty() && s != "None").then_some(s)
}

fn num(p: &serde_json::Value, k: &str) -> Option<f64> {
    let v = p.get(k)?;
    // Numeric columns come back as numbers; `windspeed` is a string of digits. The service
    // writes -99 for a value the survey left out (a path's width, its peak wind).
    v.as_f64()
        .or_else(|| v.as_str()?.trim().parse().ok())
        .filter(|x| *x > -98.0)
}

/// A GUID column, normalised for comparison; `None` for the all-zero GUID the service writes
/// for "no link".
fn guid(p: &serde_json::Value, k: &str) -> Option<String> {
    let g = text(p, k)
        .trim_matches(|c| c == '{' || c == '}')
        .to_ascii_uppercase();
    (!g.is_empty() && g.chars().any(|c| c != '0' && c != '-')).then_some(g)
}

/// Epoch-millisecond columns → UTC.
fn stamp(p: &serde_json::Value, k: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::from_timestamp_millis(p.get(k)?.as_i64()?)
}

/// Parse a GeoJSON page of the damage-point layer.
pub fn parse_points(json: &str) -> anyhow::Result<Vec<DamagePoint>> {
    let mut out = Vec::new();
    crate::overlay::for_each_feature(json, |geom, props| {
        let geojson::GeometryValue::Point { coordinates: c } = geom else {
            return;
        };
        let (Some(&lon), Some(&lat)) = (c.as_slice().first(), c.as_slice().get(1)) else {
            return;
        };
        let props = serde_json::Value::Object(props.clone());
        out.push(DamagePoint {
            lon,
            lat,
            efscale: text(&props, "efscale"),
            damage: text(&props, "damage_txt"),
            dod: text(&props, "dod_txt"),
            windspeed: num(&props, "windspeed").map(|w| w as i32),
            deaths: num(&props, "deaths").unwrap_or(0.0) as i32,
            injuries: num(&props, "injuries").unwrap_or(0.0) as i32,
            office: text(&props, "office"),
            comments: opt_text(&props, "comments"),
            image: opt_text(&props, "image"),
            storm: stamp(&props, "stormdate"),
            path_id: guid(&props, "path_guid"),
        });
    })?;
    Ok(out)
}

/// Parse a GeoJSON page of the damage-line layer. Handles both `LineString` and `MultiLineString`
/// (the service returns either depending on how the path was digitised).
pub fn parse_tracks(json: &str) -> anyhow::Result<Vec<DamageTrack>> {
    let mut out = Vec::new();
    crate::overlay::for_each_feature(json, |geom, props| {
        let lines: Vec<&Vec<geojson::Position>> = match geom {
            geojson::GeometryValue::LineString { coordinates: l } => vec![l],
            geojson::GeometryValue::MultiLineString { coordinates: ls } => ls.iter().collect(),
            _ => return,
        };
        let props = serde_json::Value::Object(props.clone());
        for line in lines {
            let path: Vec<[f64; 2]> = line
                .iter()
                .filter_map(|p| Some([*p.as_slice().first()?, *p.as_slice().get(1)?]))
                .collect();
            if path.len() < 2 {
                continue;
            }
            out.push(DamageTrack {
                path,
                efscale: text(&props, "efscale"),
                length_mi: num(&props, "length"),
                width_yd: num(&props, "width"),
                max_wind: num(&props, "maxwind").map(|w| w as i32),
                fatalities: num(&props, "fatalities").unwrap_or(0.0) as i32,
                injuries: num(&props, "injuries").unwrap_or(0.0) as i32,
                wfo: text(&props, "wfo"),
                comments: opt_text(&props, "comments"),
                storm: stamp(&props, "stormdate"),
                id: guid(&props, "globalid").unwrap_or_default(),
            });
        }
    })?;
    Ok(out)
}

/// A storm-date `where` clause. The service rejects epoch-millisecond comparisons on date columns,
/// so the window is written with SQL `timestamp` literals (UTC, as the column is stored).
fn date_where(start: chrono::DateTime<chrono::Utc>, end: chrono::DateTime<chrono::Utc>) -> String {
    format!(
        "stormdate BETWEEN timestamp '{}' AND timestamp '{}'",
        start.format("%Y-%m-%d %H:%M:%S"),
        end.format("%Y-%m-%d %H:%M:%S")
    )
}

/// Pull every page of one layer as GeoJSON, following `exceededTransferLimit`.
async fn fetch_layer(
    client: &reqwest::Client,
    layer: u8,
    bbox: (f64, f64, f64, f64),
    start: chrono::DateTime<chrono::Utc>,
    end: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<Vec<String>> {
    let (min_lon, min_lat, max_lon, max_lat) = bbox;
    let envelope = format!("{min_lon},{min_lat},{max_lon},{max_lat}");
    let where_clause = date_where(start, end);
    let mut pages = Vec::new();
    for page in 0..MAX_PAGES {
        let offset = (page * PAGE).to_string();
        let body = client
            .get(crate::net::fetch_url(&format!("{SERVER}/{layer}/query")))
            .timeout(crate::net::FEED_TIMEOUT)
            .query(&[
                ("where", where_clause.as_str()),
                ("geometry", envelope.as_str()),
                ("geometryType", "esriGeometryEnvelope"),
                ("inSR", "4326"),
                ("outSR", "4326"),
                ("outFields", "*"),
                ("returnGeometry", "true"),
                ("resultOffset", offset.as_str()),
                ("resultRecordCount", "2000"),
                ("f", "geojson"),
            ])
            .header("User-Agent", USER_AGENT)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let more = exceeded_limit(&body);
        pages.push(body);
        if !more {
            break;
        }
    }
    Ok(pages)
}

/// Whether a GeoJSON page was truncated by the server's record cap.
pub(crate) fn exceeded_limit(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("properties")?.get("exceededTransferLimit")?.as_bool())
        .unwrap_or(false)
}

/// Fetch the damage points and surveyed tracks for storms in `[start, end]` inside `bbox`.
pub async fn fetch(
    client: &reqwest::Client,
    bbox: (f64, f64, f64, f64),
    start: chrono::DateTime<chrono::Utc>,
    end: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<(Vec<DamagePoint>, Vec<DamageTrack>)> {
    fetch_cached(client, bbox, start, end, None).await
}

/// [`fetch`], with the service's raw pages kept in `cache_dir` for a query that succeeded, and
/// read back from it after. A past window's surveys are effectively fixed, and the backtest must
/// give the same truth on every run. A failed fetch is never cached, so a later run retries
/// rather than keeping an outage. (On 2026-10-03 the service failed three events' queries with
/// HTTP 400, which silently dropped their surveyed paths from that run's truth.)
pub async fn fetch_cached(
    client: &reqwest::Client,
    bbox: (f64, f64, f64, f64),
    start: chrono::DateTime<chrono::Utc>,
    end: chrono::DateTime<chrono::Utc>,
    cache_dir: Option<std::path::PathBuf>,
) -> anyhow::Result<(Vec<DamagePoint>, Vec<DamageTrack>)> {
    let (min_lon, min_lat, max_lon, max_lat) = bbox;
    let cache_file = cache_dir.map(|d| {
        d.join("dat").join(format!(
            "{min_lon:.3}_{min_lat:.3}_{max_lon:.3}_{max_lat:.3}_{}_{}.json",
            start.format("%Y%m%d%H%M"),
            end.format("%Y%m%d%H%M")
        ))
    });
    let cached: Option<(Vec<String>, Vec<String>)> = cache_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok());
    let (pt_pages, ln_pages) = match cached {
        Some(pages) => pages,
        None => {
            let pages = futures_util::try_join!(
                fetch_layer(client, 0, bbox, start, end),
                fetch_layer(client, 1, bbox, start, end),
            )?;
            if let Some(p) = &cache_file {
                if let Some(dir) = p.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                if let Ok(s) = serde_json::to_string(&pages) {
                    let _ = std::fs::write(p, s);
                }
            }
            pages
        }
    };
    let mut points = Vec::new();
    for p in &pt_pages {
        points.extend(parse_points(p)?);
    }
    let mut tracks = Vec::new();
    for p in &ln_pages {
        tracks.extend(parse_tracks(p)?);
    }
    Ok((points, tracks))
}

/// The whole country, for a query by date alone: the lower 48, Alaska, Hawaii and Puerto Rico.
pub const NATION: (f64, f64, f64, f64) = (-180.0, 15.0, -60.0, 72.0);

/// SPC's convective day `day`: 12 UTC that day to 12 UTC the next, so an evening outbreak's
/// after-midnight tornadoes stay with the day they belong to.
pub fn convective_day(
    day: chrono::NaiveDate,
) -> (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) {
    let start = day.and_hms_opt(12, 0, 0).expect("noon exists").and_utc();
    (start, start + chrono::Duration::hours(24))
}

/// One surveyed event: a path and the damage indicators along it, or indicators no path claims.
#[derive(Debug, Clone, PartialEq)]
pub struct SurveyEvent {
    pub track: Option<DamageTrack>,
    /// Strongest first.
    pub points: Vec<DamagePoint>,
}

impl SurveyEvent {
    /// The strongest rating: the path's, or its strongest indicator's when the path was left
    /// unrated (a re-analysed path can carry "N/A" while its points carry the EF5).
    pub fn rating(&self) -> Option<u8> {
        let track = self.track.as_ref().and_then(|t| ef_number(&t.efscale));
        let points = self
            .points
            .iter()
            .filter_map(|p| ef_number(&p.efscale))
            .max();
        track.max(points)
    }

    /// When it happened: the path's storm time, else its earliest indicator's.
    pub fn time(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.track
            .as_ref()
            .and_then(|t| t.storm)
            .or_else(|| self.points.iter().filter_map(|p| p.storm).min())
    }

    /// The surveying office.
    pub fn office(&self) -> &str {
        match &self.track {
            Some(t) if !t.wfo.is_empty() => &t.wfo,
            _ => self.points.first().map_or("", |p| p.office.as_str()),
        }
    }

    /// `(west, south, east, north)` around the path and its indicators.
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let coords = self
            .track
            .iter()
            .flat_map(|t| t.path.iter().copied())
            .chain(self.points.iter().map(|p| [p.lon, p.lat]));
        coords.fold(None, |acc, [lon, lat]| {
            Some(match acc {
                None => (lon, lat, lon, lat),
                Some((w, s, e, n)) => (w.min(lon), s.min(lat), e.max(lon), n.max(lat)),
            })
        })
    }

    /// Deaths: the path's count, or its indicators' sum without a path.
    pub fn deaths(&self) -> i32 {
        match &self.track {
            Some(t) => t.fatalities,
            None => self.points.iter().map(|p| p.deaths).sum(),
        }
    }
}

/// How far from a path an unlinked indicator may be and still be its damage.
const MATCH_KM: f64 = 3.0;
/// How far apart in time an unlinked indicator and a path may be.
const MATCH_HOURS: i64 = 12;

/// Distance in km from a point to a path, on a local flat projection (fine at path scale).
fn km_to_path(lon: f64, lat: f64, path: &[[f64; 2]]) -> f64 {
    let kx = 111.32 * lat.to_radians().cos();
    let ky = 110.57;
    let to = |p: &[f64; 2]| ((p[0] - lon) * kx, (p[1] - lat) * ky);
    path.windows(2)
        .map(|w| {
            let (a, b) = (to(&w[0]), to(&w[1]));
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 {
                (-(a.0 * dx + a.1 * dy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (a.0 + t * dx).hypot(a.1 + t * dy)
        })
        .fold(f64::INFINITY, f64::min)
}

impl DamageTrack {
    /// The path as it can be drawn: split wherever one segment is far longer than the whole
    /// recorded path (more than twice its length and over 10 km). A few digitised paths carry a
    /// stray vertex tens of miles off, which would draw a 1-mile EF0 across a state.
    pub fn drawable_runs(&self) -> Vec<&[[f64; 2]]> {
        let limit_km = self
            .length_mi
            .map_or(f64::INFINITY, |mi| (mi * 1.609_344 * 2.0).max(10.0));
        let seg_km = |a: &[f64; 2], b: &[f64; 2]| {
            let kx = 111.32 * ((a[1] + b[1]) / 2.0).to_radians().cos();
            ((b[0] - a[0]) * kx).hypot((b[1] - a[1]) * 110.57)
        };
        let mut runs = Vec::new();
        let mut start = 0;
        for i in 1..self.path.len() {
            if seg_km(&self.path[i - 1], &self.path[i]) > limit_km {
                if i - start >= 2 {
                    runs.push(&self.path[start..i]);
                }
                start = i;
            }
        }
        if self.path.len() - start >= 2 {
            runs.push(&self.path[start..]);
        }
        runs
    }
}

/// Group damage indicators with the paths they belong to: by the survey's own link when there
/// is one, else to the nearest path within 3 km and 12 hours. Indicators no path claims become
/// one event per office (straight-line wind, or a path not yet drawn). Events are in time order,
/// each one's indicators strongest first.
pub fn group(points: Vec<DamagePoint>, tracks: Vec<DamageTrack>) -> Vec<SurveyEvent> {
    let mut events: Vec<SurveyEvent> = tracks
        .into_iter()
        .map(|t| SurveyEvent {
            track: Some(t),
            points: Vec::new(),
        })
        .collect();
    let mut loose: std::collections::BTreeMap<String, Vec<DamagePoint>> = Default::default();
    for p in points {
        let linked = p.path_id.as_ref().and_then(|id| {
            events
                .iter()
                .position(|e| e.track.as_ref().is_some_and(|t| &t.id == id))
        });
        let near = || {
            events
                .iter()
                .enumerate()
                .filter_map(|(i, e)| {
                    let t = e.track.as_ref()?;
                    let close_in_time = match (p.storm, t.storm) {
                        (Some(a), Some(b)) => (a - b).num_hours().abs() <= MATCH_HOURS,
                        _ => true,
                    };
                    let d = km_to_path(p.lon, p.lat, &t.path);
                    (close_in_time && d <= MATCH_KM).then_some((i, d))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        };
        match linked.or_else(near) {
            Some(i) => events[i].points.push(p),
            None => loose.entry(p.office.clone()).or_default().push(p),
        }
    }
    events.extend(loose.into_values().map(|points| SurveyEvent {
        track: None,
        points,
    }));
    for e in &mut events {
        e.points
            .sort_by_key(|p| std::cmp::Reverse(ef_number(&p.efscale)));
    }
    events.sort_by_key(|e| e.time());
    events
}

/// Every survey for SPC's convective day `day`, nationwide, grouped into events.
pub async fn fetch_day(
    client: &reqwest::Client,
    day: chrono::NaiveDate,
) -> anyhow::Result<Vec<SurveyEvent>> {
    let (start, end) = convective_day(day);
    let (points, tracks) = fetch(client, NATION, start, end).await?;
    Ok(group(points, tracks))
}

/// The surveyed paths within `radius_km` of a point, over `[start, end]`, newest first, without
/// their indicators (see [`fetch_event`]).
pub async fn fetch_paths_near(
    client: &reqwest::Client,
    lon: f64,
    lat: f64,
    radius_km: f64,
    start: chrono::DateTime<chrono::Utc>,
    end: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<Vec<DamageTrack>> {
    let dlat = radius_km / 110.57;
    let dlon = radius_km / (111.32 * lat.to_radians().cos().max(0.1));
    let bbox = (lon - dlon, lat - dlat, lon + dlon, lat + dlat);
    let mut tracks = Vec::new();
    for page in fetch_layer(client, 1, bbox, start, end).await? {
        tracks.extend(parse_tracks(&page)?);
    }
    tracks.retain(|t| km_to_path(lon, lat, &t.path) <= radius_km);
    tracks.sort_by_key(|t| std::cmp::Reverse(t.storm));
    Ok(tracks)
}

/// A path's own event: the indicators around it on its day, grouped with it alone.
pub async fn fetch_event(
    client: &reqwest::Client,
    track: DamageTrack,
) -> anyhow::Result<SurveyEvent> {
    let probe = SurveyEvent {
        track: Some(track.clone()),
        points: Vec::new(),
    };
    let (w, s, e, n) = probe.bounds().unwrap_or(NATION);
    let pad = 0.05;
    let at = track.storm.unwrap_or_else(chrono::Utc::now);
    let (start, end) = (
        at - chrono::Duration::hours(MATCH_HOURS),
        at + chrono::Duration::hours(MATCH_HOURS),
    );
    let mut points = Vec::new();
    for page in fetch_layer(client, 0, (w - pad, s - pad, e + pad, n + pad), start, end).await? {
        points.extend(parse_points(&page)?);
    }
    let mut events = group(points, vec![track]);
    events.retain(|e| e.track.is_some());
    Ok(events.pop().unwrap_or(probe))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from a live query over Moore, OK for 2013-05-20.
    const POINTS: &str = r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","id":136273,"geometry":{"type":"Point","coordinates":[-97.4863,35.3391]},
       "properties":{"objectid":136273,"stormdate":1369079760000,"efscale":"EF5",
         "damage_txt":"One- or Two-Family Residence (FR12)","dod_txt":"Total destruction of building",
         "windspeed":"200","deaths":2,"injuries":11,"office":"OUN","comments":"None",
         "image":"null"}},
      {"type":"Feature","id":136274,"geometry":{"type":"Point","coordinates":[-97.51,35.33]},
       "properties":{"objectid":136274,"stormdate":1369079760000,"efscale":"EFU",
         "damage_txt":"Trees - Softwood (TS)","dod_txt":"Trees uprooted","windspeed":"85",
         "deaths":0,"injuries":0,"office":"OUN","comments":"Snapped at the base",
         "image":"https://apps.dat.noaa.gov/photo/1.jpg"}},
      {"type":"Feature","id":136275,"geometry":null,"properties":{}}]}"#;

    const TRACK: &str = r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","id":9,"geometry":{"type":"LineString",
        "coordinates":[[-97.6,35.30],[-97.4,35.35],[-97.2,35.38]]},
       "properties":{"objectid":9,"stormdate":1369079760000,"efscale":"EF5","efnum":5,
         "length":13.85,"width":1900,"maxwind":210,"fatalities":24,"injuries":212,"wfo":"OUN",
         "comments":"null"}}]}"#;

    #[tokio::test]
    async fn cached_surveys_are_read_back_without_the_service() {
        let dir = std::env::temp_dir().join(format!("hookecho-dat-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bbox = (-98.0, 35.0, -97.0, 36.0);
        let start = chrono::DateTime::from_timestamp(1_369_079_000, 0).unwrap();
        let end = start + chrono::Duration::hours(1);
        let file = dir.join("dat").join(format!(
            "-98.000_35.000_-97.000_36.000_{}_{}.json",
            start.format("%Y%m%d%H%M"),
            end.format("%Y%m%d%H%M")
        ));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let pages = (vec![POINTS.to_string()], vec![TRACK.to_string()]);
        std::fs::write(&file, serde_json::to_string(&pages).unwrap()).unwrap();
        // A client that cannot reach anything: the answer must come from the cache.
        let offline = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
            .build()
            .unwrap();
        let (points, tracks) = fetch_cached(&offline, bbox, start, end, Some(dir.clone()))
            .await
            .unwrap();
        assert_eq!((points.len(), tracks.len()), (2, 1));
        // A corrupt cache file is not trusted: it falls through to the (unreachable) service.
        std::fs::write(&file, "not json").unwrap();
        assert!(fetch_cached(&offline, bbox, start, end, Some(dir.clone()))
            .await
            .is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_damage_points() {
        let p = parse_points(POINTS).unwrap();
        assert_eq!(p.len(), 2, "the geometry-less feature must drop");
        assert_eq!(p[0].efscale, "EF5");
        assert_eq!(ef_number(&p[0].efscale), Some(5));
        assert_eq!(p[0].windspeed, Some(200));
        assert_eq!((p[0].deaths, p[0].injuries), (2, 11));
        // "None" comments and "null" images are the service's way of saying nothing.
        assert_eq!(p[0].comments, None);
        assert_eq!(p[0].image, None);
        assert_eq!(p[1].comments.as_deref(), Some("Snapped at the base"));
        assert!(p[1].image.is_some());
        // EFU is a real rating value meaning "unrateable", not a number.
        assert_eq!(ef_number(&p[1].efscale), None);
        // The other scale values the live service actually returns.
        assert_eq!(ef_number("EF3+"), Some(3));
        for unrated in ["TSTM/Wind", "UNKNOWN", "N/A", ""] {
            assert_eq!(ef_number(unrated), None, "{unrated}");
        }
        assert_eq!(
            p[0].storm.unwrap().to_rfc3339(),
            "2013-05-20T19:56:00+00:00"
        );
    }

    #[test]
    fn parses_damage_track() {
        let t = parse_tracks(TRACK).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].path.len(), 3);
        assert_eq!(t[0].length_mi, Some(13.85));
        assert_eq!(t[0].width_yd, Some(1900.0));
        assert_eq!(t[0].fatalities, 24);
        assert_eq!(t[0].comments, None);
    }

    #[test]
    fn date_window_uses_sql_timestamps() {
        let s = chrono::DateTime::from_timestamp(1369008000, 0).unwrap();
        let e = chrono::DateTime::from_timestamp(1369094400, 0).unwrap();
        assert_eq!(
            date_where(s, e),
            "stormdate BETWEEN timestamp '2013-05-20 00:00:00' AND timestamp '2013-05-21 00:00:00'"
        );
    }

    fn point(lon: f64, lat: f64, ef: &str, path_id: Option<&str>) -> DamagePoint {
        DamagePoint {
            lon,
            lat,
            efscale: ef.into(),
            damage: String::new(),
            dod: String::new(),
            windspeed: None,
            deaths: 0,
            injuries: 0,
            office: "OUN".into(),
            comments: None,
            image: None,
            storm: chrono::DateTime::from_timestamp(1_369_076_640, 0),
            path_id: path_id.map(str::to_string),
        }
    }

    fn track(id: &str, path: Vec<[f64; 2]>, ef: &str) -> DamageTrack {
        DamageTrack {
            path,
            efscale: ef.into(),
            length_mi: None,
            width_yd: None,
            max_wind: None,
            fatalities: 0,
            injuries: 0,
            wfo: "OUN".into(),
            comments: None,
            storm: chrono::DateTime::from_timestamp(1_369_076_640, 0),
            id: id.into(),
        }
    }

    /// Indicators join the path the survey linked them to, else the nearest path within 3 km;
    /// the rest are grouped apart. An unrated path takes its strongest indicator's rating.
    #[test]
    fn indicators_join_their_path() {
        let a = track("A", vec![[-97.60, 35.30], [-97.40, 35.34]], "N/A");
        let b = track("B", vec![[-97.00, 35.00], [-96.90, 35.05]], "EF1");
        let points = vec![
            point(-97.50, 35.321, "EF5", None),
            point(-97.45, 35.33, "EF2", None),
            point(-96.95, 35.03, "EF1", Some("A")),
            point(-96.20, 34.00, "EF0", None),
        ];
        let events = group(points, vec![a, b]);
        assert_eq!(events.len(), 3);
        let moore = events
            .iter()
            .find(|e| e.track.as_ref().is_some_and(|t| t.id == "A"))
            .unwrap();
        assert_eq!(moore.points.len(), 3, "two near it and one linked by id");
        assert_eq!(moore.points[0].efscale, "EF5", "strongest first");
        assert_eq!(moore.rating(), Some(5));
        let b = events
            .iter()
            .find(|e| e.track.as_ref().is_some_and(|t| t.id == "B"))
            .unwrap();
        assert!(b.points.is_empty(), "the linked point went to its own path");
        let loose = events.iter().find(|e| e.track.is_none()).unwrap();
        assert_eq!(loose.points.len(), 1);
        assert_eq!(loose.office(), "OUN");
    }

    /// A stray vertex far off a short path is not drawn as a line across the map.
    #[test]
    fn a_stray_vertex_breaks_the_drawn_path() {
        let mut t = track(
            "S",
            vec![
                [-97.50, 35.30],
                [-97.49, 35.30],
                [-96.00, 36.50],
                [-97.48, 35.31],
                [-97.47, 35.31],
            ],
            "EF0",
        );
        t.length_mi = Some(1.1);
        let runs = t.drawable_runs();
        assert_eq!(runs.len(), 2, "{runs:?}");
        assert!(runs.iter().all(|r| r.len() == 2));
        t.length_mi = None;
        assert_eq!(
            t.drawable_runs().len(),
            1,
            "without a recorded length, drawn as published"
        );
    }

    #[test]
    fn a_convective_day_runs_noon_to_noon_utc() {
        let (s, e) = convective_day(chrono::NaiveDate::from_ymd_opt(2013, 5, 20).unwrap());
        assert_eq!(s.to_rfc3339(), "2013-05-20T12:00:00+00:00");
        assert_eq!(e.to_rfc3339(), "2013-05-21T12:00:00+00:00");
    }

    #[test]
    fn missing_values_and_unlinked_guids_are_none() {
        let props = serde_json::json!({
            "width": -99.0, "maxwind": -99, "length": 17.51,
            "path_guid": "{00000000-0000-0000-0000-000000000000}",
            "globalid": "{86fce1a3-917e-4413-8b68-8efc23092575}"
        });
        assert_eq!(num(&props, "width"), None);
        assert_eq!(num(&props, "maxwind"), None);
        assert_eq!(num(&props, "length"), Some(17.51));
        assert_eq!(guid(&props, "path_guid"), None);
        assert_eq!(
            guid(&props, "globalid").as_deref(),
            Some("86FCE1A3-917E-4413-8B68-8EFC23092575")
        );
    }

    /// Moore, 2013-05-20, live: the day's surveys nationwide, and the Moore path with its
    /// indicators, found by distance to the path near Moore.
    /// `cargo test -p wxdata moore_survey_day_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "network (NWS DAT)"]
    async fn moore_survey_day_live() {
        let http = reqwest::Client::new();
        let day = chrono::NaiveDate::from_ymd_opt(2013, 5, 20).unwrap();
        let events = fetch_day(&http, day).await.unwrap();
        let paths = events.iter().filter(|e| e.track.is_some()).count();
        let points: usize = events.iter().map(|e| e.points.len()).sum();
        println!(
            "{day}: {} events, {paths} paths, {points} indicators",
            events.len()
        );
        for e in &events {
            println!(
                "  EF{:?} {} {:?} {} pts, {:.1} mi",
                e.rating(),
                e.office(),
                e.time(),
                e.points.len(),
                e.track.as_ref().and_then(|t| t.length_mi).unwrap_or(0.0)
            );
        }
        let near = fetch_paths_near(
            &http,
            -97.49,
            35.33,
            10.0,
            chrono::DateTime::from_timestamp(1_356_998_400, 0).unwrap(),
            chrono::Utc::now(),
        )
        .await
        .unwrap();
        println!("paths within 10 km of Moore since 2013: {}", near.len());
        let moore = near
            .iter()
            .find(|t| t.storm.is_some_and(|s| s.date_naive() == day));
        let event = fetch_event(&http, moore.expect("the 2013 path").clone())
            .await
            .unwrap();
        println!(
            "Moore: EF{:?}, {} indicators",
            event.rating(),
            event.points.len()
        );
        assert_eq!(event.rating(), Some(5));
    }

    #[test]
    fn detects_the_transfer_limit() {
        assert!(exceeded_limit(
            r#"{"type":"FeatureCollection","features":[],"properties":{"exceededTransferLimit":true}}"#
        ));
        assert!(!exceeded_limit(
            r#"{"type":"FeatureCollection","features":[]}"#
        ));
    }
}
