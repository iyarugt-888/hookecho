//! Driving routes for chasing (ROADMAP_NEW L1) and what weather a route runs into (L3).
//!
//! **Providers.** Routing comes from a server the user points the app at — an OSRM or Valhalla
//! instance, their own or a public one — never from a hard-wired paid service. Both are asked with
//! a plain GET (Valhalla's `?json=` form), so a browser can call a CORS-enabled server directly.
//! The public OSRM and Valhalla demo servers are offered only as an explicit choice with their
//! light-use terms stated, not as a silent default.
//!
//! **Exposure.** [`exposure`] walks a route against polygons (warnings, watches) and reports where
//! along it each one is first entered, and roughly when at the route's own pace. It says where
//! the route meets a hazard; it never says a route is safe, and nothing here should be read that
//! way — a polygon that does not cross the line today can be issued over it in five minutes.

use serde_json::Value;

/// Which kind of routing server `base_url` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Engine {
    #[default]
    Osrm,
    Valhalla,
}

impl Engine {
    pub fn label(self) -> &'static str {
        match self {
            Self::Osrm => "OSRM",
            Self::Valhalla => "Valhalla",
        }
    }

    /// The public demo server, for an explicit opt-in. Both ask for light use only.
    pub fn demo_url(self) -> &'static str {
        match self {
            Self::Osrm => "https://router.project-osrm.org",
            Self::Valhalla => "https://valhalla1.openstreetmap.de",
        }
    }
}

/// One driving route.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    /// `[lon, lat]` along the road, start to end.
    pub coords: Vec<[f64; 2]>,
    pub distance_m: f64,
    pub duration_s: f64,
    /// The provider's own short description (major roads), when it gives one.
    pub summary: String,
}

/// The request URL for `waypoints` (`[lon, lat]`, at least two), with alternatives when the
/// engine can offer them (OSRM only between two points).
pub fn request_url(engine: Engine, base_url: &str, waypoints: &[[f64; 2]]) -> String {
    let base = base_url.trim_end_matches('/');
    match engine {
        Engine::Osrm => {
            let pts: Vec<String> = waypoints
                .iter()
                .map(|p| format!("{:.6},{:.6}", p[0], p[1]))
                .collect();
            let alternatives = if waypoints.len() == 2 {
                "true"
            } else {
                "false"
            };
            format!(
                "{base}/route/v1/driving/{}?overview=full&geometries=geojson&alternatives={alternatives}&steps=false",
                pts.join(";")
            )
        }
        Engine::Valhalla => {
            let locations: Vec<Value> = waypoints
                .iter()
                .map(|p| serde_json::json!({ "lat": p[1], "lon": p[0] }))
                .collect();
            let body = serde_json::json!({
                "locations": locations,
                "costing": "auto",
                "alternates": if waypoints.len() == 2 { 2 } else { 0 },
                "directions_options": { "units": "kilometers" },
                "directions_type": "none",
            });
            let encoded: String = percent_encoding::utf8_percent_encode(
                &body.to_string(),
                percent_encoding::NON_ALPHANUMERIC,
            )
            .collect();
            format!("{base}/route?json={encoded}")
        }
    }
}

/// Parse an OSRM `route` response.
pub fn parse_osrm(json: &str) -> anyhow::Result<Vec<Route>> {
    let v: Value = serde_json::from_str(json)?;
    let code = v.get("code").and_then(Value::as_str).unwrap_or("");
    anyhow::ensure!(
        code == "Ok",
        "OSRM: {}",
        v.get("message").and_then(Value::as_str).unwrap_or(code)
    );
    let routes = v
        .get("routes")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("OSRM: no routes"))?;
    Ok(routes
        .iter()
        .filter_map(|r| {
            let coords: Vec<[f64; 2]> = r
                .get("geometry")?
                .get("coordinates")?
                .as_array()?
                .iter()
                .filter_map(|p| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?]))
                .collect();
            let summary = r
                .get("legs")
                .and_then(Value::as_array)
                .map(|legs| {
                    legs.iter()
                        .filter_map(|l| l.get("summary").and_then(Value::as_str))
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .unwrap_or_default();
            (coords.len() >= 2).then(|| Route {
                coords,
                distance_m: r.get("distance").and_then(Value::as_f64).unwrap_or(0.0),
                duration_s: r.get("duration").and_then(Value::as_f64).unwrap_or(0.0),
                summary,
            })
        })
        .collect())
}

/// Decode a Google-style encoded polyline at `precision` decimal places (Valhalla uses 6) into
/// `[lon, lat]`.
pub fn decode_polyline(encoded: &str, precision: u32) -> Vec<[f64; 2]> {
    let factor = 10f64.powi(precision as i32);
    let bytes = encoded.as_bytes();
    let (mut i, mut lat, mut lon) = (0usize, 0i64, 0i64);
    let mut out = Vec::new();
    let next = |i: &mut usize| -> Option<i64> {
        let (mut result, mut shift) = (0i64, 0u32);
        loop {
            let b = (*bytes.get(*i)? as i64) - 63;
            *i += 1;
            result |= (b & 0x1f) << shift;
            shift += 5;
            if b < 0x20 || shift > 60 {
                break;
            }
        }
        Some(if result & 1 != 0 {
            !(result >> 1)
        } else {
            result >> 1
        })
    };
    while i < bytes.len() {
        let (Some(dlat), Some(dlon)) = (next(&mut i), next(&mut i)) else {
            break;
        };
        lat += dlat;
        lon += dlon;
        out.push([lon as f64 / factor, lat as f64 / factor]);
    }
    out
}

/// Parse a Valhalla `route` response: the main trip, then any `alternates`.
pub fn parse_valhalla(json: &str) -> anyhow::Result<Vec<Route>> {
    let v: Value = serde_json::from_str(json)?;
    if let Some(err) = v.get("error").and_then(Value::as_str) {
        anyhow::bail!("Valhalla: {err}");
    }
    let trip = |t: &Value| -> Option<Route> {
        let t = t.get("trip")?;
        let mut coords = Vec::new();
        for leg in t.get("legs")?.as_array()? {
            let shape = decode_polyline(leg.get("shape")?.as_str()?, 6);
            // Consecutive legs share their joining point.
            let skip = usize::from(!coords.is_empty());
            coords.extend(shape.into_iter().skip(skip));
        }
        let s = t.get("summary")?;
        (coords.len() >= 2).then(|| Route {
            coords,
            distance_m: s.get("length").and_then(Value::as_f64).unwrap_or(0.0) * 1_000.0,
            duration_s: s.get("time").and_then(Value::as_f64).unwrap_or(0.0),
            summary: String::new(),
        })
    };
    let mut out: Vec<Route> = trip(&v).into_iter().collect();
    anyhow::ensure!(!out.is_empty(), "Valhalla: no trip");
    if let Some(alts) = v.get("alternates").and_then(Value::as_array) {
        out.extend(alts.iter().filter_map(trip));
    }
    Ok(out)
}

/// Fetch routes through `waypoints` from the configured server.
pub async fn fetch(
    http: &reqwest::Client,
    engine: Engine,
    base_url: &str,
    waypoints: &[[f64; 2]],
) -> anyhow::Result<Vec<Route>> {
    anyhow::ensure!(
        waypoints.len() >= 2,
        "a route needs a start and a destination"
    );
    anyhow::ensure!(
        base_url.starts_with("http://") || base_url.starts_with("https://"),
        "set a routing server (OSRM or Valhalla) in the route window first"
    );
    let url = request_url(engine, base_url, waypoints);
    let body = http
        .get(url)
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", crate::alerts::USER_AGENT)
        .send()
        .await?
        .text()
        .await?;
    match engine {
        Engine::Osrm => parse_osrm(&body),
        Engine::Valhalla => parse_valhalla(&body),
    }
}

// ---- along the route ---------------------------------------------------------------------------

/// Great-circle distance (m).
pub fn haversine_m(a: [f64; 2], b: [f64; 2]) -> f64 {
    let r = 6_371_008.8;
    let (la1, la2) = (a[1].to_radians(), b[1].to_radians());
    let dla = la2 - la1;
    let dlo = (b[0] - a[0]).to_radians();
    let h = (dla / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlo / 2.0).sin().powi(2);
    2.0 * r * h.sqrt().asin()
}

/// Cumulative distance (m) at each route point.
pub fn cumulative_m(coords: &[[f64; 2]]) -> Vec<f64> {
    let mut out = Vec::with_capacity(coords.len());
    let mut d = 0.0;
    for (i, p) in coords.iter().enumerate() {
        if i > 0 {
            d += haversine_m(coords[i - 1], *p);
        }
        out.push(d);
    }
    out
}

/// Where `pos` is along the route: distance (m) from the start of the nearest point on it, and
/// how far off the route `pos` is (m). Flat-earth within each segment, which is metres at road
/// scale.
pub fn progress(coords: &[[f64; 2]], pos: [f64; 2]) -> Option<(f64, f64)> {
    let cum = cumulative_m(coords);
    let k = pos[1].to_radians().cos();
    let mut best: Option<(f64, f64)> = None;
    for i in 1..coords.len() {
        let (a, b) = (coords[i - 1], coords[i]);
        let (ax, ay, bx, by) = (a[0] * k, a[1], b[0] * k, b[1]);
        let (px, py) = (pos[0] * k, pos[1]);
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 {
            (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let foot = [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
        let off = haversine_m(foot, pos);
        if best.is_none_or(|(_, o)| off < o) {
            best = Some((cum[i - 1] + t * (cum[i] - cum[i - 1]), off));
        }
    }
    best
}

fn point_in_ring(ring: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut inside = false;
    let mut j = ring.len().wrapping_sub(1);
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Where along the route (from `from_m` on) a condition first holds: metres ahead and seconds
/// ahead at the route's pace, sampling every ~200 m. For gridded fields (heavy reflectivity) and
/// point data (lightning near the road).
pub fn first_along(
    route: &Route,
    from_m: f64,
    mut hit: impl FnMut([f64; 2]) -> bool,
) -> Option<(f64, f64)> {
    let cum = cumulative_m(&route.coords);
    let total = cum.last().copied().unwrap_or(0.0).max(1.0);
    let pace = route.duration_s / total;
    let mut d = from_m.max(0.0);
    while d <= total {
        if hit(point_along(&route.coords, &cum, d)) {
            return Some((d - from_m, (d - from_m) * pace));
        }
        d += 200.0;
    }
    None
}

/// A regular grid's nearest cell to `p` (row 0 northernmost), or `None` outside it or missing.
pub fn grid_value(grid: &crate::mrms::MrmsField, p: [f64; 2]) -> Option<f32> {
    if grid.nx == 0 || grid.ny == 0 || grid.values.len() != grid.nx * grid.ny {
        return None;
    }
    let fx = (p[0] - grid.lon_west) / (grid.lon_east - grid.lon_west);
    let fy = (grid.lat_north - p[1]) / (grid.lat_north - grid.lat_south);
    if !(0.0..1.0).contains(&fx) || !(0.0..1.0).contains(&fy) {
        return None;
    }
    let v = grid.values[(fy * grid.ny as f64) as usize * grid.nx + (fx * grid.nx as f64) as usize];
    v.is_finite().then_some(v)
}

/// A polygon the route crosses: where it first enters it along the route.
#[derive(Debug, Clone, PartialEq)]
pub struct Encounter {
    /// Index into the `polygons` passed to [`exposure`].
    pub polygon: usize,
    /// Distance along the route (m) where it is first inside the polygon; 0 when the route
    /// starts inside it.
    pub at_m: f64,
    /// Time along the route (s) to get there, at the route's own average pace.
    pub at_s: f64,
}

/// Where the route (from `from_m` metres along it, e.g. the vehicle's progress) first enters
/// each polygon, nearest first. Polygons are rings (`[lon, lat]`, outer ring first); the route is
/// sampled every ~200 m, so a crossing narrower than that can be missed.
pub fn exposure(route: &Route, polygons: &[Vec<Vec<[f64; 2]>>], from_m: f64) -> Vec<Encounter> {
    let cum = cumulative_m(&route.coords);
    let total = cum.last().copied().unwrap_or(0.0).max(1.0);
    let pace = route.duration_s / total;
    // Samples along the route: every vertex, plus extra points on long segments.
    let mut samples: Vec<([f64; 2], f64)> = Vec::new();
    for i in 0..route.coords.len() {
        if i > 0 {
            let (a, b) = (route.coords[i - 1], route.coords[i]);
            let seg = cum[i] - cum[i - 1];
            let n = (seg / 200.0).ceil() as usize;
            for s in 1..n {
                let t = s as f64 / n as f64;
                samples.push((
                    [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])],
                    cum[i - 1] + t * seg,
                ));
            }
        }
        samples.push((route.coords[i], cum[i]));
    }
    // The exact point `from_m` along, so a route that starts (or a vehicle that already is)
    // inside a polygon reads as 0 m, not the next sample's distance.
    if let Some(i) = cum.iter().position(|&d| d >= from_m).filter(|&i| i > 0) {
        let (a, b) = (route.coords[i - 1], route.coords[i]);
        let seg = (cum[i] - cum[i - 1]).max(f64::EPSILON);
        let t = ((from_m - cum[i - 1]) / seg).clamp(0.0, 1.0);
        samples.push(([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])], from_m));
        samples.sort_by(|x, y| x.1.total_cmp(&y.1));
    }
    let mut out = Vec::new();
    for (pi, rings) in polygons.iter().enumerate() {
        let Some(outer) = rings.first() else { continue };
        let (mut w, mut s, mut e, mut n) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in outer {
            (w, s, e, n) = (w.min(p[0]), s.min(p[1]), e.max(p[0]), n.max(p[1]));
        }
        let hit = samples.iter().find(|(p, d)| {
            *d >= from_m
                && (w..=e).contains(&p[0])
                && (s..=n).contains(&p[1])
                && point_in_ring(outer, *p)
                && !rings[1..].iter().any(|h| point_in_ring(h, *p))
        });
        if let Some(&(_, d)) = hit {
            let at_m = d - from_m;
            out.push(Encounter {
                polygon: pi,
                at_m,
                at_s: at_m * pace,
            });
        }
    }
    out.sort_by(|a, b| a.at_m.total_cmp(&b.at_m));
    out
}

// ---- storm intercept geometry (L4) -------------------------------------------------------------

/// Where `start` ends up after `m` metres on initial bearing `bearing_deg` (spherical earth).
fn destination(start: [f64; 2], bearing_deg: f64, m: f64) -> [f64; 2] {
    let d = m / 6_371_008.8;
    let brg = bearing_deg.to_radians();
    let (lon1, lat1) = (start[0].to_radians(), start[1].to_radians());
    let lat2 = (lat1.sin() * d.cos() + lat1.cos() * d.sin() * brg.cos()).asin();
    let lon2 = lon1 + (brg.sin() * d.sin() * lat1.cos()).atan2(d.cos() - lat1.sin() * lat2.sin());
    [lon2.to_degrees(), lat2.to_degrees()]
}

/// Initial bearing (degrees from north) from `a` to `b`.
fn bearing(a: [f64; 2], b: [f64; 2]) -> f64 {
    let (la1, la2) = (a[1].to_radians(), b[1].to_radians());
    let dlo = (b[0] - a[0]).to_radians();
    let y = dlo.sin() * la2.cos();
    let x = la1.cos() * la2.sin() - la1.sin() * la2.cos() * dlo.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

// ---- the forecast along the route (L3) ---------------------------------------------------------

/// A stretch of the route where a forecast field meets a condition at the time the vehicle would
/// be there, driving at the route's average pace.
#[derive(Debug, Clone, PartialEq)]
pub struct ForecastSpan {
    /// Metres ahead of the vehicle's progress where the stretch starts and ends.
    pub from_m: f64,
    pub to_m: f64,
    /// When the vehicle would be at each end.
    pub start: chrono::DateTime<chrono::Utc>,
    pub end: chrono::DateTime<chrono::Utc>,
    /// The highest value met along the stretch.
    pub peak: f32,
}

/// What [`forecast_along`] found, and how much of the route the frames reached.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ForecastCheck {
    pub spans: Vec<ForecastSpan>,
    /// How far ahead (m) and until when the frames stood for the vehicle's time; the route past
    /// that was not checked. `None` when no sample had a frame near its time.
    pub covered: Option<(f64, chrono::DateTime<chrono::Utc>)>,
}

/// How far a sample's time may be from a forecast frame's valid time for that frame to stand for
/// it: half the HRRR's 15-minute sub-hourly step, and a minute more.
pub const FORECAST_FRAME_SLACK_S: i64 = 8 * 60 + 30;

/// Walk the route from `from_m` at its average pace, starting `now`, and test each ~200 m sample
/// against the forecast frame (`time` = valid time) nearest the vehicle's time there, within
/// `FORECAST_FRAME_SLACK_S`. Contiguous samples where `hit` holds become one [`ForecastSpan`].
/// A sample with no frame near its time is a gap (it ends a stretch and is not counted as
/// clear); the walk stops once the vehicle's time is past the last frame.
pub fn forecast_along(
    route: &Route,
    from_m: f64,
    now: chrono::DateTime<chrono::Utc>,
    frames: &[crate::mrms::MrmsField],
    hit: impl FnMut(f32) -> bool,
) -> ForecastCheck {
    forecast_along_within(route, from_m, now, frames, FORECAST_FRAME_SLACK_S, hit)
}

/// How far a sample's time may be from an hourly window frame's middle: half the hour, and a
/// minute more (the HRRR's max fields, such as updraft helicity, each cover one hour).
pub const HOURLY_FRAME_SLACK_S: i64 = 31 * 60;

/// [`forecast_along`] with frames that stand for `slack_s` seconds either side of their `time`.
pub fn forecast_along_within(
    route: &Route,
    from_m: f64,
    now: chrono::DateTime<chrono::Utc>,
    frames: &[crate::mrms::MrmsField],
    slack_s: i64,
    mut hit: impl FnMut(f32) -> bool,
) -> ForecastCheck {
    let mut out = ForecastCheck::default();
    let Some(last) = frames.iter().map(|f| f.time).max() else {
        return out;
    };
    let slack = chrono::Duration::seconds(slack_s);
    let cum = cumulative_m(&route.coords);
    let total = cum.last().copied().unwrap_or(0.0);
    if total <= 0.0 {
        return out;
    }
    let pace = route.duration_s / total;
    let from = from_m.clamp(0.0, total);
    let mut open: Option<ForecastSpan> = None;
    let mut d = from;
    loop {
        let ahead = d - from;
        let t = now + chrono::Duration::milliseconds((ahead * pace * 1000.0) as i64);
        if t > last + slack {
            break;
        }
        let frame = frames
            .iter()
            .min_by_key(|f| (f.time - t).num_seconds().abs())
            .filter(|f| (f.time - t).num_seconds().abs() <= slack_s);
        let value = frame.and_then(|f| {
            out.covered = Some((ahead, t));
            grid_value(f, point_along(&route.coords, &cum, d))
        });
        match value.filter(|v| hit(*v)) {
            Some(v) => match open.as_mut() {
                Some(s) => {
                    s.to_m = ahead;
                    s.end = t;
                    s.peak = s.peak.max(v);
                }
                None => {
                    open = Some(ForecastSpan {
                        from_m: ahead,
                        to_m: ahead,
                        start: t,
                        end: t,
                        peak: v,
                    })
                }
            },
            None => out.spans.extend(open.take()),
        }
        if d >= total {
            break;
        }
        d = (d + 200.0).min(total);
    }
    out.spans.extend(open);
    out
}

/// HRRR sub-hourly composite-reflectivity frames for the next `horizon_min` minutes, every 15
/// minutes from the quarter-hour at or before `now`, all from one run and cropped to `bbox`
/// (`[west, south, east, north]`). Each frame's `time` is its valid time. The newest run that
/// has posted the farthest frame is used (a run posts its leads in order); older runs are tried
/// when it has not.
pub async fn fetch_forecast_frames(
    http: &reqwest::Client,
    now: chrono::DateTime<chrono::Utc>,
    bbox: [f64; 4],
    horizon_min: u16,
) -> anyhow::Result<(chrono::DateTime<chrono::Utc>, Vec<crate::mrms::MrmsField>)> {
    use crate::hrrr::{self, Model};
    use chrono::Timelike;
    use futures_util::StreamExt;
    let start = now
        .with_minute(now.minute() / 15 * 15)
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(now);
    let steps = i64::from(horizon_min.div_ceil(15).max(1));
    let valids: Vec<_> = (0..=steps)
        .map(|i| start + chrono::Duration::minutes(15 * i))
        .collect();
    let mut last_err = None;
    for run in hrrr::recent_cycles(Model::Hrrr, now) {
        let max_min = i64::from(Model::Hrrr.max_lead_for_cycle(run.hour())) * 60;
        let leads: Vec<u16> = valids
            .iter()
            .map(|v| (*v - run).num_minutes())
            .filter(|m| (15..=max_min).contains(m))
            .map(|m| m as u16)
            .collect();
        let Some(&farthest) = leads.last().filter(|_| leads.len() == valids.len()) else {
            last_err = Some(anyhow::anyhow!(
                "the HRRR {:02}Z run does not reach that far",
                run.hour()
            ));
            continue;
        };
        let last = match hrrr::fetch_forecast_subhourly_at_run(http, run, farthest).await {
            Ok(fc) => crop_frame(fc, bbox)?,
            Err(e) => {
                last_err = Some(e);
                continue;
            }
        };
        // Owned, not borrowed: a borrow held across the await stops the future being `Send`.
        let earlier: Vec<u16> = leads[..leads.len() - 1].to_vec();
        let rest: Vec<anyhow::Result<crate::mrms::MrmsField>> =
            futures_util::stream::iter(earlier.into_iter().map(|m| {
                let http = http.clone();
                async move {
                    let fc = hrrr::fetch_forecast_subhourly_at_run(&http, run, m).await?;
                    crop_frame(fc, bbox)
                }
            }))
            .buffered(4)
            .collect()
            .await;
        let mut frames = rest.into_iter().collect::<anyhow::Result<Vec<_>>>()?;
        frames.push(last);
        return Ok((run, frames));
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no HRRR run found")))
}

/// The HRRR's hourly maximum 2-5 km updraft helicity (its rotation tracks) for the hour windows
/// that cover the next `horizon_min` minutes, from one run, cropped to `bbox`. Each frame's `time`
/// is the middle of its hour (read it with [`HOURLY_FRAME_SLACK_S`]), since the value is the
/// strongest anywhere in that hour, not at an instant. The newest run that has posted the last
/// window is used.
pub async fn fetch_uh_frames(
    http: &reqwest::Client,
    now: chrono::DateTime<chrono::Utc>,
    bbox: [f64; 4],
    horizon_min: u16,
) -> anyhow::Result<(chrono::DateTime<chrono::Utc>, Vec<crate::mrms::MrmsField>)> {
    use crate::hrrr::{self, Model};
    use chrono::Timelike;
    use futures_util::StreamExt;
    let end = now + chrono::Duration::minutes(i64::from(horizon_min));
    let hours_to = |t: chrono::DateTime<chrono::Utc>, run: chrono::DateTime<chrono::Utc>| {
        ((t - run).num_seconds() as f64 / 3600.0).ceil() as i64
    };
    let mut last_err = None;
    for run in hrrr::recent_cycles(Model::Hrrr, now) {
        // The window (h-1, h] that holds now, through the one that holds the end.
        let (first, last) = (hours_to(now, run).max(1), hours_to(end, run));
        if last > i64::from(Model::Hrrr.max_lead_for_cycle(run.hour())) {
            last_err = Some(anyhow::anyhow!(
                "the HRRR {:02}Z run does not reach that far",
                run.hour()
            ));
            continue;
        }
        let fetch = |h: i64| {
            let http = http.clone();
            async move {
                let fc = hrrr::fetch_field_at_run(
                    &http,
                    Model::Hrrr,
                    run,
                    "MXUPHL",
                    "5000-2000 m above ground",
                    h as u8,
                    0.0,
                )
                .await?;
                let mut f = crop_frame(fc, bbox)?;
                f.time -= chrono::Duration::minutes(30);
                anyhow::Ok(f)
            }
        };
        let newest = match fetch(last).await {
            Ok(f) => f,
            Err(e) => {
                last_err = Some(e);
                continue;
            }
        };
        let rest: Vec<anyhow::Result<crate::mrms::MrmsField>> =
            futures_util::stream::iter((first..last).map(fetch))
                .buffered(3)
                .collect()
                .await;
        let mut frames = rest.into_iter().collect::<anyhow::Result<Vec<_>>>()?;
        frames.push(newest);
        return Ok((run, frames));
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no HRRR run found")))
}

/// One forecast cropped to `bbox`, its `time` set to its valid time.
fn crop_frame(
    fc: crate::hrrr::HrrrForecast,
    bbox: [f64; 4],
) -> anyhow::Result<crate::mrms::MrmsField> {
    let valid = fc.valid();
    let mut f = fc
        .field
        .cropped(bbox[0], bbox[1], bbox[2], bbox[3])
        .ok_or_else(|| anyhow::anyhow!("the route is outside the HRRR's grid"))?;
    f.time = valid;
    Ok(f)
}

/// The point `m` metres along the route (clamped to its ends).
fn point_along(coords: &[[f64; 2]], cum: &[f64], m: f64) -> [f64; 2] {
    let Some(i) = cum.iter().position(|&d| d >= m) else {
        return *coords.last().unwrap_or(&[0.0, 0.0]);
    };
    if i == 0 {
        return coords[0];
    }
    let t = ((m - cum[i - 1]) / (cum[i] - cum[i - 1]).max(f64::EPSILON)).clamp(0.0, 1.0);
    let (a, b) = (coords[i - 1], coords[i]);
    [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
}

/// Which way along the route gets off a storm's path (ROADMAP_NEW L4): the route's heading at the
/// vehicle, ahead and back, against the escape bearing (perpendicular to the storm's motion, on
/// the vehicle's side of it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RouteEscape {
    /// Driving on along the route heads within [`ESCAPE_TOLERANCE_DEG`] of the escape; the
    /// route's heading there.
    Ahead(f64),
    /// Turning back along the route does; the heading back.
    Back(f64),
    /// Neither does: the escape is off this route. The route's heading ahead.
    Off(f64),
}

/// How far the route's heading may be from the escape bearing and still count as an escape.
pub const ESCAPE_TOLERANCE_DEG: f64 = 60.0;

/// [`RouteEscape`] at `from_m` along the route, judged over the next (and previous) 2 km so a
/// kink at the vehicle does not decide it. `None` for a route with no length.
pub fn route_escape(coords: &[[f64; 2]], from_m: f64, escape_deg: f64) -> Option<RouteEscape> {
    let cum = cumulative_m(coords);
    let total = *cum.last()?;
    if total <= 0.0 {
        return None;
    }
    let here = point_along(coords, &cum, from_m.clamp(0.0, total));
    let heading = |m: f64| {
        let there = point_along(coords, &cum, m.clamp(0.0, total));
        (haversine_m(here, there) > 1.0).then(|| initial_bearing(here, there))
    };
    let off = |h: f64| {
        let d = (h - escape_deg).rem_euclid(360.0);
        d.min(360.0 - d)
    };
    let ahead = heading(from_m + 2_000.0);
    let back = heading(from_m - 2_000.0);
    Some(match (ahead, back) {
        (Some(a), _) if off(a) <= ESCAPE_TOLERANCE_DEG => RouteEscape::Ahead(a),
        (_, Some(b)) if off(b) <= ESCAPE_TOLERANCE_DEG => RouteEscape::Back(b),
        (Some(a), _) => RouteEscape::Off(a),
        (None, Some(b)) => RouteEscape::Off((b + 180.0) % 360.0),
        (None, None) => return None,
    })
}

/// The initial great-circle bearing from `a` to `b`, degrees clockwise from north.
fn initial_bearing(a: [f64; 2], b: [f64; 2]) -> f64 {
    let (la1, la2) = (a[1].to_radians(), b[1].to_radians());
    let dlo = (b[0] - a[0]).to_radians();
    let y = dlo.sin() * la2.cos();
    let x = la1.cos() * la2.sin() - la1.sin() * la2.cos() * dlo.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

/// A storm against a route (ROADMAP_NEW L4), from the vehicle's current progress on it.
#[derive(Debug, Clone, PartialEq)]
pub struct Intercept {
    /// The closest the storm and the vehicle come (m) while the vehicle drives the route at its
    /// own pace and the storm keeps its motion, and when (s from now).
    pub closest_m: f64,
    pub closest_s: f64,
    /// Where the storm is, seen from the vehicle, at that moment (degrees from north).
    pub closest_bearing_deg: f64,
    /// Where the storm's track crosses the route ahead: metres ahead along the route, when the
    /// storm gets there, and when the vehicle does (both s from now). `None` when the track does
    /// not cross the route within the horizon.
    pub crossing: Option<(f64, f64, f64)>,
}

/// Intercept geometry for a storm at `storm` moving toward `mvt_deg` at `mvt_kt`, against the
/// vehicle `from_m` metres along `route`, looking `horizon_s` ahead. Straight-line storm motion
/// and the route's average pace: an estimate to plan by, not a forecast.
pub fn intercept(
    route: &Route,
    from_m: f64,
    storm: [f64; 2],
    mvt_deg: f64,
    mvt_kt: f64,
    horizon_s: f64,
) -> Option<Intercept> {
    let cum = cumulative_m(&route.coords);
    let total = *cum.last()?;
    if total <= 0.0 {
        return None;
    }
    let speed = mvt_kt * 0.514_444; // m/s
    let pace = route.duration_s / total; // s per metre
    let vehicle_at = |t: f64| point_along(&route.coords, &cum, from_m + t / pace.max(1e-9));
    let storm_at = |t: f64| destination(storm, mvt_deg, speed * t);
    let mut best = (f64::MAX, 0.0);
    let mut t = 0.0;
    while t <= horizon_s {
        let d = haversine_m(vehicle_at(t), storm_at(t));
        if d < best.0 {
            best = (d, t);
        }
        t += 30.0;
    }
    // The storm's track against each route segment ahead, in a local flat frame.
    let k = storm[1].to_radians().cos();
    let flat = |p: [f64; 2]| (p[0] * k, p[1]);
    let track_end = storm_at(horizon_s);
    let (s0, s1) = (flat(storm), flat(track_end));
    let mut crossing = None;
    for i in 1..route.coords.len() {
        if cum[i] < from_m {
            continue;
        }
        let (a, b) = (flat(route.coords[i - 1]), flat(route.coords[i]));
        let (r, q) = ((b.0 - a.0, b.1 - a.1), (s1.0 - s0.0, s1.1 - s0.1));
        let den = r.0 * q.1 - r.1 * q.0;
        if den.abs() < 1e-15 {
            continue;
        }
        let (ax, ay) = (s0.0 - a.0, s0.1 - a.1);
        let u = (ax * q.1 - ay * q.0) / den; // along the route segment
        let v = (ax * r.1 - ay * r.0) / den; // along the storm track
        if (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v) {
            let at_m = cum[i - 1] + u * (cum[i] - cum[i - 1]);
            if at_m < from_m {
                continue;
            }
            let x = [
                route.coords[i - 1][0] + u * (route.coords[i][0] - route.coords[i - 1][0]),
                route.coords[i - 1][1] + u * (route.coords[i][1] - route.coords[i - 1][1]),
            ];
            let storm_s = if speed > 0.0 {
                haversine_m(storm, x) / speed
            } else {
                f64::INFINITY
            };
            crossing = Some((at_m - from_m, storm_s, (at_m - from_m) * pace));
            break;
        }
    }
    Some(Intercept {
        closest_m: best.0,
        closest_s: best.1,
        closest_bearing_deg: bearing(vehicle_at(best.1), storm_at(best.1)),
        crossing,
    })
}

#[cfg(test)]
mod forecast_tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    /// A 1-degree grid frame over 34-36 N, 99-95 W at `time`, 0 dBZ except where `wet` says.
    fn frame(
        time: chrono::DateTime<Utc>,
        wet: impl Fn(f64) -> Option<f32>,
    ) -> crate::mrms::MrmsField {
        let (nx, ny) = (400, 200);
        let mut values = vec![0.0f32; nx * ny];
        for r in 0..ny {
            for c in 0..nx {
                let lon = -99.0 + (c as f64 + 0.5) * 4.0 / nx as f64;
                if let Some(v) = wet(lon) {
                    values[r * nx + c] = v;
                }
            }
        }
        crate::mrms::MrmsField {
            values,
            nx,
            ny,
            lon_west: -99.0,
            lon_east: -95.0,
            lat_north: 36.0,
            lat_south: 34.0,
            time,
        }
    }

    /// An eastbound road along 35 N from 98 W to 96 W, about 182 km, driven in two hours.
    fn road() -> Route {
        let coords: Vec<[f64; 2]> = (0..=20).map(|i| [-98.0 + 0.1 * i as f64, 35.0]).collect();
        Route {
            distance_m: *cumulative_m(&coords).last().unwrap(),
            coords,
            duration_s: 7200.0,
            summary: String::new(),
        }
    }

    /// A storm forecast over 97.2-97.0 W only in the frame valid an hour from now is met where the
    /// vehicle is then (it reaches 97 W at the hour), and not where it is earlier or later; the
    /// same storm a frame too early is missed, since the vehicle is not there yet.
    #[test]
    fn a_forecast_storm_is_met_only_where_the_vehicle_is_at_its_time() {
        let now = Utc.with_ymd_and_hms(2026, 5, 20, 21, 0, 0).unwrap();
        let storm = |lon: f64| (-97.2..-97.0).contains(&lon).then_some(52.0);
        let frames: Vec<_> = (0..=8)
            .map(|i| {
                let t = now + chrono::Duration::minutes(15 * i);
                if i == 3 || i == 4 {
                    frame(t, storm)
                } else {
                    frame(t, |_| None)
                }
            })
            .collect();
        let check = forecast_along(&road(), 0.0, now, &frames, |v| v >= 40.0);
        assert_eq!(check.spans.len(), 1, "{check:?}");
        let s = &check.spans[0];
        // 97.2 W is 0.8 degree along (~73 km, ~48 min): the 45-min frame covers 37.5-52.5 min.
        assert!((70_000.0..76_000.0).contains(&s.from_m), "{s:?}");
        assert!(s.start >= now + chrono::Duration::minutes(46));
        assert!(s.end <= now + chrono::Duration::minutes(60));
        assert_eq!(s.peak, 52.0);
        // The whole road was covered, to its end two hours on.
        let (m, t) = check.covered.unwrap();
        assert!((m - road().distance_m).abs() < 1.0);
        assert!((t - (now + chrono::Duration::hours(2))).num_seconds().abs() <= 1);

        // Only the frame valid at 21:15 has the storm: the vehicle is still far west of it then.
        let early: Vec<_> = (0..=8)
            .map(|i| {
                let t = now + chrono::Duration::minutes(15 * i);
                if i == 1 {
                    frame(t, storm)
                } else {
                    frame(t, |_| None)
                }
            })
            .collect();
        assert!(forecast_along(&road(), 0.0, now, &early, |v| v >= 40.0)
            .spans
            .is_empty());
    }

    /// An hourly window frame, read with its half-hour slack, covers the whole hour: a track in
    /// the 21-22Z window is met by a vehicle there at 21:10, which the 8.5-minute slack would miss.
    #[test]
    fn an_hourly_window_frame_covers_its_whole_hour() {
        let now = Utc.with_ymd_and_hms(2026, 5, 20, 21, 0, 0).unwrap();
        // UH over 97.9-97.7 W in the 21-22Z window (its middle 21:30), reached 6 to 18 min out.
        let window = frame(now + chrono::Duration::minutes(30), |lon| {
            (-97.9..-97.7).contains(&lon).then_some(120.0)
        });
        let frames = [window];
        let hourly = forecast_along_within(&road(), 0.0, now, &frames, HOURLY_FRAME_SLACK_S, |v| {
            v >= 75.0
        });
        assert_eq!(hourly.spans.len(), 1, "{hourly:?}");
        assert_eq!(hourly.spans[0].peak, 120.0);
        let quarter = forecast_along(&road(), 0.0, now, &frames, |v| v >= 75.0);
        // With the short slack the frame stands only for 21:21:30-21:38:30: the track is missed.
        assert!(quarter.spans.is_empty(), "{quarter:?}");
    }

    /// Frames that stop short cover only part of the route, and say how far; no frames, nothing.
    #[test]
    fn the_check_says_how_far_the_frames_reach() {
        let now = Utc.with_ymd_and_hms(2026, 5, 20, 21, 0, 0).unwrap();
        let frames: Vec<_> = (0..=2)
            .map(|i| frame(now + chrono::Duration::minutes(15 * i), |_| None))
            .collect();
        let check = forecast_along(&road(), 0.0, now, &frames, |v| v >= 40.0);
        let (m, t) = check.covered.unwrap();
        // The last frame is valid at 21:30 and stands until 21:38:30: ~58 km of a 182 km road.
        assert!(t <= now + chrono::Duration::seconds(30 * 60 + FORECAST_FRAME_SLACK_S));
        assert!((55_000.0..60_000.0).contains(&m), "{m}");
        assert!(check.spans.is_empty());
        assert_eq!(
            forecast_along(&road(), 0.0, now, &[], |_| true),
            ForecastCheck::default()
        );
    }

    /// A missing frame mid-way is a gap: it splits a stretch rather than joining across it, and
    /// missing values (NaN, outside the grid) never count as a hit.
    #[test]
    fn a_missing_frame_or_value_is_a_gap_not_a_hit() {
        let now = Utc.with_ymd_and_hms(2026, 5, 20, 21, 0, 0).unwrap();
        let everywhere = |_: f64| Some(45.0);
        let frames: Vec<_> = [0, 1, 3, 4]
            .into_iter()
            .map(|i| frame(now + chrono::Duration::minutes(15 * i), everywhere))
            .collect();
        let check = forecast_along(&road(), 0.0, now, &frames, |v| v >= 40.0);
        assert_eq!(check.spans.len(), 2, "{check:?}");
        assert!(check.spans[0].end < now + chrono::Duration::minutes(24));
        assert!(check.spans[1].start > now + chrono::Duration::minutes(36));

        let nan = vec![frame(now, |_| Some(f32::NAN))];
        assert!(forecast_along(&road(), 0.0, now, &nan, |_| true)
            .spans
            .is_empty());
    }

    /// A crop keeps whole cells in place: a value reads the same through the crop as before it,
    /// and a box off the grid gives nothing.
    #[test]
    fn a_cropped_grid_reads_the_same_values() {
        let now = Utc.with_ymd_and_hms(2026, 5, 20, 21, 0, 0).unwrap();
        let f = frame(now, |lon| Some(lon as f32));
        let c = f.cropped(-97.534, 34.605, -96.413, 35.396).unwrap();
        assert!(c.nx < f.nx && c.ny < f.ny);
        for p in [[-97.495, 34.625], [-97.005, 35.005], [-96.425, 35.385]] {
            assert_eq!(grid_value(&c, p), grid_value(&f, p), "{p:?}");
        }
        assert!(c.lon_west <= -97.534 && c.lon_east >= -96.413);
        assert!(f.cropped(-90.0, 34.0, -89.0, 35.0).is_none());
        assert!(f.cropped(-98.0, 40.0, -97.0, 41.0).is_none());
    }

    /// The live HRRR rotation tracks: the hour windows over the next 90 minutes, from one run,
    /// each timed at its window's middle, cropped to the box.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_hrrr_uh_windows_cover_the_next_ninety_minutes() {
        let http = reqwest::Client::new();
        let now = Utc::now();
        let bbox = [-98.5, 34.5, -96.5, 36.0];
        let (run, frames) = fetch_uh_frames(&http, now, bbox, 90).await.unwrap();
        let strongest = frames
            .iter()
            .flat_map(|f| f.values.iter().copied().filter(|v| v.is_finite()))
            .fold(0.0f32, f32::max);
        eprintln!(
            "HRRR {run}: {} UH windows {:?}, {}x{} cells, strongest {strongest:.0} m2/s2",
            frames.len(),
            frames
                .iter()
                .map(|f| f.time.format("%H:%M").to_string())
                .collect::<Vec<_>>(),
            frames[0].nx,
            frames[0].ny
        );
        assert!((2..=4).contains(&frames.len()));
        assert!(frames
            .iter()
            .all(|f| f.time.format("%M").to_string() == "30"));
        let first = frames.iter().map(|f| f.time).min().unwrap();
        assert!((first - now).num_minutes().abs() <= 31);
    }

    /// The live HRRR: an hour of sub-hourly frames over central Oklahoma, from one run, every 15
    /// minutes, cropped to the box, and checked along a real road.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_hrrr_frames_cover_the_next_hour_from_one_run() {
        let http = reqwest::Client::new();
        let now = Utc::now();
        let bbox = [-98.5, 34.5, -96.5, 36.0];
        let (run, frames) = fetch_forecast_frames(&http, now, bbox, 60).await.unwrap();
        assert_eq!(frames.len(), 5);
        let mut times: Vec<_> = frames.iter().map(|f| f.time).collect();
        times.sort();
        for w in times.windows(2) {
            assert_eq!((w[1] - w[0]).num_minutes(), 15);
        }
        assert!(times[0] <= now && now - times[0] < chrono::Duration::minutes(15));
        let wettest = frames
            .iter()
            .flat_map(|f| f.values.iter().copied().filter(|v| v.is_finite()))
            .fold(f32::MIN, f32::max);
        for f in &frames {
            assert!(
                f.lon_west <= bbox[0] && f.lon_east >= bbox[2],
                "{:?}",
                f.lon_west
            );
            assert!(f.lat_south <= bbox[1] && f.lat_north >= bbox[3]);
            assert!(f.nx < 200 && f.ny < 200, "{}x{}", f.nx, f.ny);
        }
        let road = road();
        let check = forecast_along(&road, 0.0, now, &frames, |v| v >= 40.0);
        eprintln!(
            "HRRR {run} ({} frames {}..{}, {}x{} cells, wettest {wettest:.0} dBZ): {} stretch(es) \
             of 40+ dBZ, covered {:?}",
            frames.len(),
            times[0],
            times[4],
            frames[0].nx,
            frames[0].ny,
            check.spans.len(),
            check.covered.map(|(m, t)| (m.round(), t))
        );
        assert!(wettest < 90.0);
        assert!(check.covered.is_some());
    }
}

#[cfg(test)]
mod escape_tests {
    use super::*;

    /// An eastbound road: escaping east is on the route ahead, west is back along it, north is off
    /// it; a bearing 50 degrees off the road still counts, 70 does not.
    #[test]
    fn the_escape_is_judged_against_the_road_ahead_and_behind() {
        let road = vec![[-98.0, 35.0], [-97.0, 35.0], [-96.0, 35.0]];
        let at = progress(&road, [-97.0, 35.0]).unwrap().0;
        assert!(
            matches!(route_escape(&road, at, 90.0), Some(RouteEscape::Ahead(h)) if (h - 90.0).abs() < 1.0)
        );
        assert!(
            matches!(route_escape(&road, at, 270.0), Some(RouteEscape::Back(h)) if (h - 270.0).abs() < 1.0)
        );
        assert!(
            matches!(route_escape(&road, at, 0.0), Some(RouteEscape::Off(h)) if (h - 90.0).abs() < 1.0)
        );
        assert!(matches!(
            route_escape(&road, at, 140.0),
            Some(RouteEscape::Ahead(_))
        ));
        assert!(matches!(
            route_escape(&road, at, 160.0),
            Some(RouteEscape::Off(_))
        ));
        // At the end of the road there is only back.
        let end = progress(&road, [-96.0, 35.0]).unwrap().0;
        assert!(matches!(
            route_escape(&road, end, 270.0),
            Some(RouteEscape::Back(_))
        ));
        assert!(matches!(
            route_escape(&road, end, 90.0),
            Some(RouteEscape::Off(_))
        ));
        assert_eq!(route_escape(&[[-97.0, 35.0]], 0.0, 90.0), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osrm_routes_parse_with_alternatives() {
        let json = r#"{"code":"Ok","routes":[
            {"geometry":{"coordinates":[[-97.5,35.2],[-97.4,35.3],[-97.3,35.4]],"type":"LineString"},
             "legs":[{"summary":"I 35, OK 9"}],"distance":25000.5,"duration":1200.0},
            {"geometry":{"coordinates":[[-97.5,35.2],[-97.35,35.4]],"type":"LineString"},
             "legs":[{"summary":""}],"distance":27000.0,"duration":1500.0}],"waypoints":[]}"#;
        let r = parse_osrm(json).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].coords[1], [-97.4, 35.3]);
        assert_eq!(r[0].summary, "I 35, OK 9");
        assert_eq!(r[1].duration_s, 1500.0);
        let err = parse_osrm(r#"{"code":"NoRoute","message":"Impossible route"}"#).unwrap_err();
        assert!(err.to_string().contains("Impossible route"));
    }

    #[test]
    fn a_valhalla_polyline6_decodes_and_legs_join() {
        // The reference example from Google's polyline docs, at precision 5, and its points.
        let p5 = decode_polyline("_p~iF~ps|U_ulLnnqC_mqNvxq`@", 5);
        assert_eq!(p5.len(), 3);
        assert!((p5[0][1] - 38.5).abs() < 1e-9 && (p5[0][0] + 120.2).abs() < 1e-9);
        assert!((p5[2][1] - 43.252).abs() < 1e-9 && (p5[2][0] + 126.453).abs() < 1e-9);
        // The same shape written at precision 6 decodes to the same points.
        let enc6 = encode6(&p5);
        let back = decode_polyline(&enc6, 6);
        for (a, b) in p5.iter().zip(&back) {
            assert!((a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
        }
        let json = format!(
            r#"{{"trip":{{"legs":[{{"shape":"{enc6}"}},{{"shape":"{tail}"}}],
                "summary":{{"length":12.5,"time":900}}}},
                "alternates":[{{"trip":{{"legs":[{{"shape":"{enc6}"}}],"summary":{{"length":14,"time":1000}}}}}}]}}"#,
            tail = encode6(&[p5[2], [-127.0, 44.0]])
        );
        let r = parse_valhalla(&json).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(
            r[0].coords.len(),
            4,
            "the shared joining point appears once"
        );
        assert_eq!(r[0].distance_m, 12_500.0);
        assert!(parse_valhalla(r#"{"error":"No path could be found"}"#).is_err());
    }

    /// Encode `[lon, lat]` at precision 6 (for building test fixtures).
    fn encode6(pts: &[[f64; 2]]) -> String {
        let mut out = String::new();
        let (mut plat, mut plon) = (0i64, 0i64);
        let put = |v: i64, out: &mut String| {
            let mut v = if v < 0 { !(v << 1) } else { v << 1 };
            while v >= 0x20 {
                out.push((((v & 0x1f) | 0x20) as u8 + 63) as char);
                v >>= 5;
            }
            out.push((v as u8 + 63) as char);
        };
        for p in pts {
            let (lat, lon) = ((p[1] * 1e6).round() as i64, (p[0] * 1e6).round() as i64);
            put(lat - plat, &mut out);
            put(lon - plon, &mut out);
            (plat, plon) = (lat, lon);
        }
        out
    }

    fn straight_north() -> Route {
        // 1° of latitude north from 35°N: ~111 km, driven in an hour.
        Route {
            coords: vec![[-97.0, 35.0], [-97.0, 35.5], [-97.0, 36.0]],
            distance_m: 111_000.0,
            duration_s: 3_600.0,
            summary: String::new(),
        }
    }

    #[test]
    fn progress_finds_the_nearest_point_and_the_offset() {
        let r = straight_north();
        let (along, off) = progress(&r.coords, [-96.99, 35.25]).unwrap();
        assert!((along - 27_800.0).abs() < 300.0, "{along}");
        assert!((off - 910.0).abs() < 50.0, "{off}");
    }

    #[test]
    fn exposure_reports_where_each_polygon_is_first_entered() {
        let r = straight_north();
        let square = |s: f64, n: f64| {
            vec![vec![
                [-97.1, s],
                [-96.9, s],
                [-96.9, n],
                [-97.1, n],
                [-97.1, s],
            ]]
        };
        let polys = vec![
            square(35.6, 35.7), // ~66.7 km in
            square(35.2, 35.3), // ~22.2 km in
            vec![vec![
                [-90.0, 35.0],
                [-89.0, 35.0],
                [-89.0, 36.0],
                [-90.0, 35.0],
            ]], // off route
        ];
        let e = exposure(&r, &polys, 0.0);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].polygon, 1, "nearest first");
        assert!((e[0].at_m - 22_240.0).abs() < 300.0, "{}", e[0].at_m);
        assert!((e[0].at_s - e[0].at_m * 3_600.0 / 111_195.0).abs() < 30.0);
        // 30 km along is still inside the nearer square: it is reported at 0 m, "in it now".
        let now = exposure(&r, &polys, 30_000.0);
        assert_eq!((now[0].polygon, now[0].at_m), (1, 0.0));
        // From 40 km the nearer square is behind; the other is ~26.7 km ahead.
        let later = exposure(&r, &polys, 40_000.0);
        assert_eq!(later.len(), 1);
        assert!(
            (later[0].at_m - 26_700.0).abs() < 400.0,
            "{}",
            later[0].at_m
        );
    }

    #[test]
    fn a_storm_crossing_the_road_ahead_is_timed_against_the_vehicle() {
        // The road runs due north ~111 km in an hour. A storm 30 km east of its midpoint moves due
        // west at 30 kt (~15.4 m/s): it reaches the road in ~32 min, 55.6 km along; the vehicle,
        // starting at the south end, gets there in ~30 min.
        let r = straight_north();
        let storm = destination([-97.0, 35.5], 90.0, 30_000.0);
        let i = intercept(&r, 0.0, storm, 270.0, 30.0, 3_600.0).unwrap();
        let (ahead_m, storm_s, vehicle_s) = i.crossing.unwrap();
        assert!((ahead_m - 55_600.0).abs() < 500.0, "{ahead_m}");
        assert!((storm_s - 1_944.0).abs() < 30.0, "{storm_s}");
        assert!((vehicle_s - 1_800.0).abs() < 30.0, "{vehicle_s}");
        // They meet near there: the closest approach is a couple of km, about half an hour out.
        assert!(
            i.closest_m < 3_000.0 && (1_500.0..2_300.0).contains(&i.closest_s),
            "{i:?}"
        );
        // A storm moving away never crosses, and stays east of the vehicle.
        let away = intercept(&r, 0.0, storm, 90.0, 30.0, 3_600.0).unwrap();
        assert!(away.crossing.is_none());
        assert!(
            (30.0..150.0).contains(&away.closest_bearing_deg),
            "{away:?}"
        );
    }

    #[test]
    fn a_field_and_a_flash_are_found_along_the_road() {
        let r = straight_north();
        // Heavy echo north of 35.7°N on a 0.1° grid.
        let (nx, ny) = (20, 20);
        let grid = crate::mrms::MrmsField {
            values: (0..nx * ny)
                .map(|i| {
                    if 36.5 - (i / nx) as f64 * 0.1 > 35.7 {
                        55.0
                    } else {
                        20.0
                    }
                })
                .collect(),
            nx,
            ny,
            lon_west: -98.0,
            lon_east: -96.0,
            lat_north: 36.5,
            lat_south: 34.5,
            time: chrono::DateTime::UNIX_EPOCH,
        };
        let (m, s) =
            first_along(&r, 0.0, |p| grid_value(&grid, p).is_some_and(|v| v >= 50.0)).unwrap();
        assert!((m - 77_800.0).abs() < 1_500.0, "{m}");
        assert!((s - m * 3_600.0 / 111_195.0).abs() < 60.0);
        assert!(
            grid_value(&grid, [-90.0, 35.0]).is_none(),
            "outside the grid"
        );
        // A flash 3 km off the road at 35.3°N.
        let flash = destination([-97.0, 35.3], 90.0, 3_000.0);
        let (m, _) = first_along(&r, 0.0, |p| haversine_m(p, flash) < 8_000.0).unwrap();
        // Within 8 km from sqrt(8² - 3²) = 7.4 km before the nearest point, 33.3 km along.
        assert!((m - 25_900.0).abs() < 500.0, "{m}");
    }

    #[test]
    fn request_urls_are_well_formed() {
        let w = [[-97.5, 35.2], [-97.3, 35.4]];
        let u = request_url(Engine::Osrm, "https://router.example/", &w);
        assert!(u.starts_with("https://router.example/route/v1/driving/-97.500000,35.200000;"));
        assert!(u.contains("alternatives=true") && u.contains("geometries=geojson"));
        let v = request_url(Engine::Valhalla, "http://localhost:8002", &w);
        assert!(v.starts_with("http://localhost:8002/route?json=%7B"));
        let decoded = percent_encoding::percent_decode_str(v.split("json=").nth(1).unwrap())
            .decode_utf8()
            .unwrap();
        let body: Value = serde_json::from_str(&decoded).unwrap();
        assert_eq!(body["locations"][1]["lat"], 35.4);
        assert_eq!(body["costing"], "auto");
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn the_public_demo_servers_route_okc_to_norman() {
        let http = reqwest::Client::new();
        let w = [[-97.5164, 35.4676], [-97.4395, 35.2226]];
        for engine in [Engine::Osrm, Engine::Valhalla] {
            let r = fetch(&http, engine, engine.demo_url(), &w).await.unwrap();
            eprintln!(
                "{}: {} routes, first {:.1} km in {:.0} min, {} points, '{}'",
                engine.label(),
                r.len(),
                r[0].distance_m / 1000.0,
                r[0].duration_s / 60.0,
                r[0].coords.len(),
                r[0].summary
            );
            assert!((25_000.0..45_000.0).contains(&r[0].distance_m));
            assert!((900.0..3_600.0).contains(&r[0].duration_s));
            let end = *r[0].coords.last().unwrap();
            assert!(haversine_m(end, w[1]) < 2_000.0, "ends near Norman");
        }
    }
}
