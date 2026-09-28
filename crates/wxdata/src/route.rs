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
