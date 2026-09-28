//! NWS county and forecast-zone shapes (UGCs) and the forecast office each belongs to, for the
//! "Forecast zones" and "CWA boundaries" map layers.
//!
//! The Iowa Environmental Mesonet serves the NWS UGC database as GeoJSON, one state at a time
//! (`/api/1/nws/ugcs.geojson?state=OK`, about 250 KB), with each shape's code, name, state and WFO.
//! The whole country in one request is 24 MB, so the app asks for the states the view covers
//! ([`states_in_view`]) and keeps what it has fetched.
//!
//! County warning areas are not published as polygons of their own; [`cwa_outlines`] dissolves
//! the counties by office: an edge two counties share is drawn only where their offices differ,
//! and an edge only one county has (the coast, the border) is drawn always.

use crate::alerts::USER_AGENT;
use crate::overlay::{for_each_feature, polygons_of};

const URL: &str = "https://mesonet.agron.iastate.edu/api/1/nws/ugcs.geojson?state=";

/// One county (`OKC109`) or forecast zone (`OKZ025`).
#[derive(Debug, Clone, PartialEq)]
pub struct Ugc {
    pub code: String,
    pub name: String,
    pub state: String,
    /// The forecast office whose county warning area it is (`OUN`).
    pub wfo: String,
    /// Every ring of every polygon, `[lon, lat]`.
    pub rings: Vec<Vec<[f64; 2]>>,
}

impl Ugc {
    /// A county (the `C` in `OKC109`), as against a forecast zone.
    pub fn is_county(&self) -> bool {
        self.code.as_bytes().get(2) == Some(&b'C')
    }
}

/// Parse IEM's UGC GeoJSON.
pub fn parse(json: &str) -> anyhow::Result<Vec<Ugc>> {
    let mut out = Vec::new();
    for_each_feature(json, |geom, props| {
        let s = |k: &str| {
            props
                .get(k)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let rings: Vec<Vec<[f64; 2]>> = polygons_of(geom).into_iter().flatten().collect();
        if rings.is_empty() {
            return;
        }
        out.push(Ugc {
            code: s("ugc"),
            name: s("name"),
            state: s("state"),
            wfo: s("wfo"),
            rings,
        });
    })?;
    Ok(out)
}

/// Fetch one state's counties and zones.
pub async fn fetch_state(client: &reqwest::Client, state: &str) -> anyhow::Result<Vec<Ugc>> {
    parse(&fetch_state_text(client, state).await?)
}

/// One state's file as served, for a caller that keeps it on disk and [`parse`]s it.
pub async fn fetch_state_text(client: &reqwest::Client, state: &str) -> anyhow::Result<String> {
    Ok(client
        .get(crate::net::fetch_url(&format!("{URL}{state}")))
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?)
}

/// The states (and territories) whose shapes reach into the view `[west, south, east, north]`.
/// Alaska's box runs past -180, so each box is also tried shifted a turn east.
pub fn states_in_view(view: [f64; 4]) -> Vec<&'static str> {
    let overlaps =
        |b: [f64; 4]| b[0] < view[2] && b[2] > view[0] && b[1] < view[3] && b[3] > view[1];
    STATE_BOXES
        .iter()
        .filter(|(_, b)| overlaps(*b) || overlaps([b[0] + 360.0, b[1], b[2] + 360.0, b[3]]))
        .map(|(s, _)| *s)
        .collect()
}

/// The county warning area outlines of `ugcs` (counties are used; zones are ignored), as
/// polylines: every county edge that has no neighbour, or whose neighbour belongs to another
/// office. Shared vertices are matched to about a metre.
pub fn cwa_outlines(ugcs: &[Ugc]) -> Vec<Vec<[f64; 2]>> {
    use std::collections::HashMap;
    type Edge = ((i64, i64), (i64, i64));
    // Undirected edge -> the offices on its sides, and the segment itself.
    let mut edges: HashMap<Edge, (Vec<&str>, [[f64; 2]; 2])> = HashMap::new();
    for u in ugcs.iter().filter(|u| u.is_county()) {
        for ring in &u.rings {
            for w in ring.windows(2) {
                let (a, b) = (key(w[0]), key(w[1]));
                if a == b {
                    continue;
                }
                let k = if a < b { (a, b) } else { (b, a) };
                edges
                    .entry(k)
                    .or_insert_with(|| (Vec::new(), [w[0], w[1]]))
                    .0
                    .push(&u.wfo);
            }
        }
    }
    let mut kept: Vec<[[f64; 2]; 2]> = edges
        .into_values()
        .filter(|(sides, _)| sides.len() == 1 || sides.iter().any(|w| *w != sides[0]))
        .map(|(_, seg)| seg)
        .collect();
    // A fixed order, so the same counties always chain into the same polylines.
    kept.sort_by(|a, b| (key(a[0]), key(a[1])).cmp(&(key(b[0]), key(b[1]))));
    chain(kept)
}

/// A vertex matched to about a metre.
fn key(p: [f64; 2]) -> (i64, i64) {
    ((p[0] * 1e5).round() as i64, (p[1] * 1e5).round() as i64)
}

/// Join segments that meet end to end into polylines.
fn chain(segs: Vec<[[f64; 2]; 2]>) -> Vec<Vec<[f64; 2]>> {
    use std::collections::HashMap;
    let mut at: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        at.entry(key(s[0])).or_default().push(i);
        at.entry(key(s[1])).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    for start in 0..segs.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut line = std::collections::VecDeque::from([segs[start][0], segs[start][1]]);
        // Grow from the tail, then from the head.
        for forward in [true, false] {
            loop {
                let end = if forward {
                    line[line.len() - 1]
                } else {
                    line[0]
                };
                let next = at
                    .get(&key(end))
                    .and_then(|v| v.iter().copied().find(|&i| !used[i]));
                let Some(i) = next else { break };
                used[i] = true;
                let s = segs[i];
                let p = if key(s[0]) == key(end) { s[1] } else { s[0] };
                if forward {
                    line.push_back(p);
                } else {
                    line.push_front(p);
                }
            }
        }
        out.push(line.into_iter().collect());
    }
    out
}

/// Each state's and territory's bounding box `[west, south, east, north]`, from the UGC database
/// itself (computed 2026-09-27, padded a tenth of a degree).
const STATE_BOXES: &[(&str, [f64; 4])] = &[
    ("AK", [-187.7, 51.1, -129.9, 71.5]),
    ("AL", [-88.6, 30.1, -84.8, 35.1]),
    ("AR", [-94.7, 32.9, -89.5, 36.6]),
    ("AS", [-171.2, -14.7, -168.0, -11.0]),
    ("AZ", [-114.9, 31.2, -109.0, 37.1]),
    ("CA", [-124.5, 32.4, -114.0, 42.1]),
    ("CO", [-109.2, 36.9, -101.9, 41.1]),
    ("CT", [-73.8, 40.9, -71.7, 42.1]),
    ("DC", [-77.2, 38.7, -76.8, 39.1]),
    ("DE", [-75.9, 38.4, -75.0, 39.9]),
    ("FL", [-87.7, 24.4, -79.9, 31.1]),
    ("FM", [-222.0, 0.9, -196.9, 10.2]),
    ("GA", [-85.7, 30.3, -80.7, 35.1]),
    ("GU", [-215.5, 13.1, -214.9, 13.8]),
    ("HI", [-160.3, 18.8, -154.7, 22.3]),
    ("IA", [-96.7, 40.3, -90.0, 43.6]),
    ("ID", [-117.3, 41.9, -110.9, 49.1]),
    ("IL", [-91.6, 36.9, -87.4, 42.6]),
    ("IN", [-88.2, 37.7, -84.7, 41.9]),
    ("KS", [-102.1, 36.9, -94.5, 40.1]),
    ("KY", [-89.7, 36.4, -81.9, 39.2]),
    ("LA", [-94.1, 28.8, -89.0, 33.1]),
    ("MA", [-73.6, 41.1, -69.8, 43.0]),
    ("MD", [-79.6, 37.8, -75.0, 39.8]),
    ("ME", [-71.2, 43.0, -66.9, 47.6]),
    ("MH", [-197.9, 5.7, -187.7, 11.8]),
    ("MI", [-90.5, 41.6, -82.3, 48.3]),
    ("MN", [-97.3, 43.4, -89.4, 49.5]),
    ("MO", [-95.9, 35.9, -89.0, 40.7]),
    ("MP", [-215.0, 14.0, -214.1, 18.9]),
    ("MS", [-91.8, 30.1, -88.0, 35.1]),
    ("MT", [-116.1, 44.3, -103.9, 49.1]),
    ("NC", [-84.4, 33.7, -75.4, 36.7]),
    ("ND", [-104.1, 45.8, -96.5, 49.1]),
    ("NE", [-104.1, 39.9, -95.2, 43.1]),
    ("NH", [-72.7, 42.6, -70.6, 45.4]),
    ("NJ", [-75.7, 38.8, -73.8, 41.5]),
    ("NM", [-109.1, 31.2, -102.9, 37.1]),
    ("NV", [-120.1, 34.9, -113.9, 42.1]),
    ("NY", [-79.9, 40.4, -71.8, 45.1]),
    ("OH", [-84.9, 38.3, -80.4, 42.1]),
    ("OK", [-103.1, 33.5, -94.3, 37.1]),
    ("OR", [-124.7, 41.9, -116.4, 46.4]),
    ("PA", [-80.6, 39.6, -74.6, 42.4]),
    ("PR", [-68.0, 17.8, -65.1, 18.6]),
    ("PW", [-229.0, 2.9, -225.2, 8.2]),
    ("RI", [-72.0, 41.0, -71.0, 42.1]),
    ("SC", [-83.4, 31.9, -78.4, 35.3]),
    ("SD", [-104.2, 42.4, -96.3, 46.1]),
    ("TN", [-90.4, 34.9, -81.6, 36.8]),
    ("TX", [-106.8, 25.7, -93.4, 36.6]),
    ("UT", [-114.1, 36.9, -108.9, 42.1]),
    ("VA", [-83.8, 36.4, -75.2, 39.6]),
    ("VI", [-65.2, 17.6, -64.5, 18.5]),
    ("VT", [-73.5, 42.6, -71.4, 45.1]),
    ("WA", [-124.9, 45.4, -116.8, 49.1]),
    ("WI", [-93.0, 42.4, -86.7, 47.2]),
    ("WV", [-82.7, 37.1, -77.6, 40.7]),
    ("WY", [-111.2, 40.9, -104.0, 45.2]),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn county(code: &str, wfo: &str, ring: &[[f64; 2]]) -> Ugc {
        Ugc {
            code: code.into(),
            name: code.into(),
            state: "OK".into(),
            wfo: wfo.into(),
            rings: vec![ring.to_vec()],
        }
    }

    #[test]
    fn a_cwa_outline_skips_edges_inside_one_office() {
        // Three unit squares in a row: A and B belong to OUN, C to TSA.
        let sq = |x: f64| [[x, 0.0], [x + 1.0, 0.0], [x + 1.0, 1.0], [x, 1.0], [x, 0.0]];
        let ugcs = [
            county("OKC001", "OUN", &sq(0.0)),
            county("OKC003", "OUN", &sq(1.0)),
            county("OKC005", "TSA", &sq(2.0)),
            // A forecast zone over them is not a county, and does not count.
            county("OKZ001", "OUN", &sq(0.5)),
        ];
        let lines = cwa_outlines(&ugcs);
        let has = |p: [f64; 2], q: [f64; 2]| {
            lines.iter().any(|l| {
                l.windows(2)
                    .any(|w| (w[0] == p && w[1] == q) || (w[0] == q && w[1] == p))
            })
        };
        assert!(!has([1.0, 0.0], [1.0, 1.0]), "A|B share an office: no line");
        assert!(has([2.0, 0.0], [2.0, 1.0]), "B|C differ: a line");
        assert!(has([0.0, 0.0], [1.0, 0.0]), "an outer edge is drawn");
        // The outlines chain into a few polylines, not one per segment.
        assert!(lines.len() <= 3, "{} polylines", lines.len());
    }

    #[test]
    fn states_in_view_include_alaska_across_the_antimeridian() {
        let ok = states_in_view([-98.0, 35.0, -97.0, 36.0]);
        assert!(ok.contains(&"OK") && !ok.contains(&"CA"), "{ok:?}");
        assert!(states_in_view([175.0, 50.0, 179.0, 53.0]).contains(&"AK"));
    }

    /// Oklahoma live: the dissolve keeps a fraction of the county edges (neighbouring counties
    /// share vertices in the source), and draws the outlines of every office with counties here.
    /// `cargo test -p wxdata ugc_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "network"]
    async fn ugc_live() {
        let ugcs = fetch_state(&reqwest::Client::new(), "OK").await.unwrap();
        let counties = ugcs.iter().filter(|u| u.is_county()).count();
        let zones = ugcs.len() - counties;
        let all_edges: usize = ugcs
            .iter()
            .filter(|u| u.is_county())
            .flat_map(|u| &u.rings)
            .map(|r| r.len().saturating_sub(1))
            .sum();
        let lines = cwa_outlines(&ugcs);
        let kept: usize = lines.iter().map(|l| l.len() - 1).sum();
        println!(
            "OK: {counties} counties, {zones} zones; {kept} of {all_edges} county edges kept in {} polylines",
            lines.len()
        );
        assert!(counties >= 77 && zones > 0);
        assert!(kept * 2 < all_edges, "interior edges are dropped");
    }

    #[test]
    fn parses_iem_ugcs() {
        let json = r#"{"type":"FeatureCollection","features":[{"type":"Feature",
            "properties":{"ugc":"OKC001","name":"Adair","state":"OK","wfo":"TSA"},
            "geometry":{"type":"MultiPolygon","coordinates":[[[[-94.56,36.13],[-94.55,36.11],
            [-94.54,36.05],[-94.56,36.13]]]]}}]}"#;
        let u = parse(json).unwrap();
        assert_eq!(u.len(), 1);
        assert_eq!((u[0].code.as_str(), u[0].wfo.as_str()), ("OKC001", "TSA"));
        assert!(u[0].is_county());
        assert_eq!(u[0].rings[0].len(), 4);
    }
}
