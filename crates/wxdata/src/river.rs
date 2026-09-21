//! River flood gauges from NOAA's National Water Prediction Service (NWPS), for chase-relevant
//! flood awareness. A bounding-box query returns the gauges in view; each becomes a [`Gauge`] the
//! app draws as a flood-category colored droplet with a stage / forecast tooltip.
//!
//! Endpoint (no API key): `GET .../nwps/v1/gauges?bbox.xmin=<lon0>&bbox.ymin=<lat0>&
//! bbox.xmax=<lon1>&bbox.ymax=<lat1>&srid=EPSG_4326`. Stage/forecast come back as `-999` when
//! missing; flood category is a string (`action|minor|moderate|major|no_flooding|not_defined|
//! obs_not_current|fcst_not_current`).

use crate::alerts::USER_AGENT;

const GAUGES_URL: &str = "https://api.water.noaa.gov/nwps/v1/gauges";

/// NWS flood severity for a gauge. Anything not-defined / stale maps to [`FloodCat::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloodCat {
    Major,
    Moderate,
    Minor,
    Action,
    NoFlooding,
    Unknown,
}

impl FloodCat {
    fn from_str(s: &str) -> Self {
        match s {
            "major" => FloodCat::Major,
            "moderate" => FloodCat::Moderate,
            "minor" => FloodCat::Minor,
            "action" => FloodCat::Action,
            "no_flooding" => FloodCat::NoFlooding,
            _ => FloodCat::Unknown, // not_defined, obs_not_current, fcst_not_current, ...
        }
    }
    /// Major-first sort key (0 = worst) so the app can prioritize / draw serious flooding on top.
    fn severity(self) -> u8 {
        match self {
            FloodCat::Major => 0,
            FloodCat::Moderate => 1,
            FloodCat::Minor => 2,
            FloodCat::Action => 3,
            FloodCat::NoFlooding => 4,
            FloodCat::Unknown => 5,
        }
    }
}

/// One river gauge with its current observed stage and (when available) forecast crest.
#[derive(Debug, Clone, PartialEq)]
pub struct Gauge {
    pub lid: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub cat: FloodCat,
    pub stage_ft: Option<f64>,
    pub forecast_ft: Option<f64>,
    pub forecast_cat: FloodCat,
}

/// `-999` is NWPS's missing sentinel; treat it (and any negative) as no reading.
fn stage(v: Option<f64>) -> Option<f64> {
    v.filter(|f| *f > -900.0)
}

/// Parse an NWPS `gauges` JSON payload. Tolerant: skips entries missing lat/lon.
pub fn parse(json: &str) -> Vec<Gauge> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(arr) = v.get("gauges").and_then(|g| g.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for g in arr {
        let (Some(lat), Some(lon)) = (num(g, "latitude"), num(g, "longitude")) else {
            continue;
        };
        let obs = g.get("status").and_then(|s| s.get("observed"));
        let fcst = g.get("status").and_then(|s| s.get("forecast"));
        out.push(Gauge {
            lid: str_of(g, "lid"),
            name: str_of(g, "name"),
            lat,
            lon,
            cat: cat_of(obs),
            stage_ft: stage(obs.and_then(|o| num(o, "primary"))),
            forecast_ft: stage(fcst.and_then(|f| num(f, "primary"))),
            forecast_cat: cat_of(fcst),
        });
    }
    out
}

fn cat_of(node: Option<&serde_json::Value>) -> FloodCat {
    node.and_then(|n| n.get("floodCategory"))
        .and_then(|c| c.as_str())
        .map_or(FloodCat::Unknown, FloodCat::from_str)
}

fn str_of(m: &serde_json::Value, k: &str) -> String {
    m.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn num(m: &serde_json::Value, k: &str) -> Option<f64> {
    m.get(k).and_then(|v| v.as_f64())
}

/// The widest area worth asking for, in degrees. A gauge map is unreadable past this, and the
/// service answers an unbounded box with the whole national dataset (about 13 MB, slowly).
pub const MAX_SPAN_DEG: f64 = 20.0;

/// Validate and normalize a bounding box into the service's query parameters.
///
/// The service accepts nonsense without complaint (NaN returns nothing, an unbounded box returns
/// everything), so a view that has not been laid out yet, or one zoomed far out, must be refused
/// here rather than sent. Coordinates are clamped to the valid range and put in min/max order.
pub fn bbox_params(
    lat0: f64,
    lon0: f64,
    lat1: f64,
    lon1: f64,
) -> anyhow::Result<[(&'static str, String); 5]> {
    anyhow::ensure!(
        [lat0, lon0, lat1, lon1].iter().all(|v| v.is_finite()),
        "the map view has no valid bounds yet"
    );
    let (lat_lo, lat_hi) = (
        lat0.min(lat1).clamp(-90.0, 90.0),
        lat0.max(lat1).clamp(-90.0, 90.0),
    );
    let (lon_lo, lon_hi) = (
        lon0.min(lon1).clamp(-180.0, 180.0),
        lon0.max(lon1).clamp(-180.0, 180.0),
    );
    anyhow::ensure!(
        lat_hi - lat_lo <= MAX_SPAN_DEG && lon_hi - lon_lo <= MAX_SPAN_DEG,
        "zoom in to see river gauges (the view spans more than {MAX_SPAN_DEG} degrees)"
    );
    // Four decimals is about 10 m, far finer than a gauge symbol, and keeps the URL short.
    let fmt = |v: f64| format!("{v:.4}");
    Ok([
        ("bbox.xmin", fmt(lon_lo)),
        ("bbox.ymin", fmt(lat_lo)),
        ("bbox.xmax", fmt(lon_hi)),
        ("bbox.ymax", fmt(lat_hi)),
        ("srid", "EPSG_4326".to_string()),
    ])
}

/// Turn a non-success answer into a message that says what the service said. NWPS answers with a
/// small JSON error body; showing it beats a bare "404" that hides why.
fn describe_failure(status: reqwest::StatusCode, body: &str) -> String {
    if status.as_u16() == 429 {
        return "river gauge service is rate limiting requests (10 per 5 minutes); it will retry"
            .into();
    }
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("message")
                .or_else(|| v.pointer("/error/message"))
                .and_then(|m| m.as_str().map(str::to_string))
        })
        .unwrap_or_else(|| body.chars().take(120).collect());
    format!(
        "river gauge service (NWPS) answered {status}: {}",
        detail.trim()
    )
}

/// Fetch the gauges within a lat/lon bounding box `(lat0, lon0, lat1, lon1)`, sorted worst-first
/// and capped at 300. `USER_AGENT` identifies the app to NOAA.
pub async fn fetch_bbox(
    client: &reqwest::Client,
    lat0: f64,
    lon0: f64,
    lat1: f64,
    lon1: f64,
) -> anyhow::Result<Vec<Gauge>> {
    let params = bbox_params(lat0, lon0, lat1, lon1)?;
    let response = client
        .get(crate::net::fetch_url(GAUGES_URL))
        .timeout(crate::net::FEED_TIMEOUT)
        .query(&params)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    anyhow::ensure!(status.is_success(), "{}", describe_failure(status, &body));
    let mut gauges = parse(&body);
    gauges.sort_by_key(|g| g.cat.severity());
    gauges.truncate(300);
    Ok(gauges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_drops_missing_sentinels() {
        // One flooding gauge with a real stage + forecast, one stale gauge with -999 sentinels.
        let json = r#"{"gauges":[
            {"lid":"MAJT2","name":"Big River","latitude":33.1,"longitude":-97.2,
             "status":{"observed":{"primary":28.4,"floodCategory":"major"},
                       "forecast":{"primary":31.0,"floodCategory":"moderate"}}},
            {"lid":"DRYT2","name":"Dry Creek","latitude":32.5,"longitude":-96.8,
             "status":{"observed":{"primary":-999,"floodCategory":"obs_not_current"},
                       "forecast":{"primary":-999,"floodCategory":"fcst_not_current"}}}
        ]}"#;
        let g = parse(json);
        assert_eq!(g.len(), 2);
        // Find them by id (parse order is preserved; fetch_bbox is what sorts).
        let maj = g.iter().find(|x| x.lid == "MAJT2").unwrap();
        assert_eq!(maj.cat, FloodCat::Major);
        assert_eq!(maj.stage_ft, Some(28.4));
        assert_eq!(maj.forecast_ft, Some(31.0));
        assert_eq!(maj.forecast_cat, FloodCat::Moderate);
        let dry = g.iter().find(|x| x.lid == "DRYT2").unwrap();
        assert_eq!(dry.cat, FloodCat::Unknown);
        assert_eq!(dry.stage_ft, None);
        assert_eq!(dry.forecast_ft, None);
    }

    #[test]
    fn bounds_are_validated_before_anything_is_sent() {
        let v = |p: [(&str, String); 5]| -> Vec<String> { p.into_iter().map(|(_, v)| v).collect() };
        // A normal view, in min/max order whichever way the corners were given.
        assert_eq!(
            v(bbox_params(34.0, -98.0, 36.0, -96.0).unwrap()),
            ["-98.0000", "34.0000", "-96.0000", "36.0000", "EPSG_4326"]
        );
        assert_eq!(
            v(bbox_params(36.0, -96.0, 34.0, -98.0).unwrap()),
            ["-98.0000", "34.0000", "-96.0000", "36.0000", "EPSG_4326"]
        );
        // A view with no layout yet (NaN or infinite corners) is refused, not sent.
        assert!(bbox_params(f64::NAN, -98.0, 36.0, -96.0).is_err());
        assert!(bbox_params(34.0, f64::NEG_INFINITY, 36.0, f64::INFINITY).is_err());
        // The whole country is refused: the service would answer with about 13 MB.
        assert!(bbox_params(24.0, -125.0, 50.0, -66.0).is_err());
        // Out-of-range coordinates are clamped rather than sent as-is.
        let clamped = v(bbox_params(89.0, 170.0, 95.0, 190.0).unwrap());
        assert_eq!(
            &clamped[..4],
            ["170.0000", "89.0000", "180.0000", "90.0000"]
        );
    }

    #[test]
    fn failures_say_what_the_service_said() {
        use reqwest::StatusCode;
        assert!(describe_failure(StatusCode::TOO_MANY_REQUESTS, "{}").contains("rate limiting"));
        let not_found = describe_failure(
            StatusCode::NOT_FOUND,
            r#"{"code":5,"message":"[] could not find unknown ID","details":[]}"#,
        );
        assert!(not_found.contains("404") && not_found.contains("could not find unknown ID"));
        // A body that is not JSON is shown truncated, never dumped whole.
        let html = describe_failure(StatusCode::BAD_GATEWAY, &"x".repeat(5000));
        assert!(html.len() < 250, "{}", html.len());
    }

    // Live network check (NWPS is public; be gentle).
    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_real_gauges() {
        let client = reqwest::Client::new();
        let g = fetch_bbox(&client, 32.0, -98.5, 33.5, -96.5).await.unwrap();
        eprintln!("{} gauges; first: {:?}", g.len(), g.first());
        assert!(!g.is_empty());
    }
}
