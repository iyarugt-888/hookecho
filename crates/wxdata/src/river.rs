//! River flood gauges from NOAA's National Water Prediction Service (NWPS), for chase-relevant
//! flood awareness. A bounding-box query returns the gauges in view; each becomes a [`Gauge`] the
//! app draws as a flood-category colored droplet with a stage / forecast tooltip.
//!
//! Clicking one asks for two more documents: the gauge's metadata ([`GaugeDetail`]: its flood
//! stages, crest history, impact statements and seasonal outlook) and its hydrograph
//! ([`Hydrograph`]: about 30 days of observed stage and flow plus the river forecast, when the
//! forecast center issues one). [`peak`] and [`trend`] read the crest and the rate of rise out of a
//! stretch of readings.
//!
//! Endpoint (no API key): `GET .../nwps/v1/gauges?bbox.xmin=<lon0>&bbox.ymin=<lat0>&
//! bbox.xmax=<lon1>&bbox.ymax=<lat1>&srid=EPSG_4326`. Stage/forecast come back as `-999` when
//! missing; flood category is a string (`action|minor|moderate|major|no_flooding|not_defined|
//! obs_not_current|fcst_not_current`).

use crate::alerts::USER_AGENT;
use chrono::{DateTime, Utc};

const GAUGES_URL: &str = "https://api.water.noaa.gov/nwps/v1/gauges";

/// The gauge's page on the NWPS website, for "open in browser".
pub fn page_url(lid: &str) -> String {
    format!("https://water.noaa.gov/gauges/{}", lid.to_ascii_lowercase())
}

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
    pub fn severity(self) -> u8 {
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
    arr.iter().filter_map(gauge_of).collect()
}

/// One gauge's summary, from a list entry or the gauge's own metadata document (both carry the
/// same id, name, position and `status` block).
fn gauge_of(g: &serde_json::Value) -> Option<Gauge> {
    let (lat, lon) = (num(g, "latitude")?, num(g, "longitude")?);
    let obs = g.get("status").and_then(|s| s.get("observed"));
    let fcst = g.get("status").and_then(|s| s.get("forecast"));
    Some(Gauge {
        lid: str_of(g, "lid"),
        name: str_of(g, "name"),
        lat,
        lon,
        cat: cat_of(obs),
        stage_ft: stage(obs.and_then(|o| num(o, "primary"))),
        forecast_ft: stage(fcst.and_then(|f| num(f, "primary"))),
        forecast_cat: cat_of(fcst),
    })
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

/// The stages at which a gauge's river reaches each NWS flood category. A gauge may define only
/// some of them (or none: many gauges exist for water supply, not flood warning).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Thresholds {
    pub action: Option<f64>,
    pub minor: Option<f64>,
    pub moderate: Option<f64>,
    pub major: Option<f64>,
}

impl Thresholds {
    /// Each defined stage with its category, lowest first.
    pub fn levels(&self) -> Vec<(FloodCat, f64)> {
        let mut v: Vec<(FloodCat, f64)> = [
            (FloodCat::Action, self.action),
            (FloodCat::Minor, self.minor),
            (FloodCat::Moderate, self.moderate),
            (FloodCat::Major, self.major),
        ]
        .into_iter()
        .filter_map(|(c, s)| s.map(|s| (c, s)))
        .collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        v
    }

    /// The category a stage falls in: the highest one it reaches, `NoFlooding` below all of them,
    /// `Unknown` at a gauge that defines none.
    pub fn category(&self, stage_ft: f64) -> FloodCat {
        let levels = self.levels();
        if levels.is_empty() {
            return FloodCat::Unknown;
        }
        levels
            .iter()
            .rev()
            .find(|(_, s)| stage_ft >= *s)
            .map_or(FloodCat::NoFlooding, |(c, _)| *c)
    }

    /// The next category the river would reach from `stage_ft`, and at what stage.
    pub fn next_above(&self, stage_ft: f64) -> Option<(FloodCat, f64)> {
        self.levels().into_iter().find(|(_, s)| *s > stage_ft)
    }
}

/// A notable past crest from the gauge's record.
#[derive(Debug, Clone, PartialEq)]
pub struct Crest {
    pub time: DateTime<Utc>,
    pub stage_ft: f64,
    /// In cubic feet per second; `None` where the record has no flow.
    pub flow_cfs: Option<f64>,
    /// Not yet reviewed (`P` in the record).
    pub preliminary: bool,
}

/// What happens at a stage, in the forecast office's words ("Water covers River Road...").
#[derive(Debug, Clone, PartialEq)]
pub struct Impact {
    pub stage_ft: f64,
    pub statement: String,
}

/// The seasonal (long-range) chance of the river reaching each category, as the service states
/// it: a probability, or a bound like `< 0.05`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outlook {
    pub minor: String,
    pub moderate: String,
    pub major: String,
    /// The months it covers, as three initials (`SON` is September to November).
    pub interval: String,
}

/// A probability as the service writes it, as a percentage: `0.1166` is `12%`, `< 0.05` is
/// `< 5%`. Anything else comes back as written.
pub fn chance_percent(s: &str) -> String {
    let t = s.trim();
    let (prefix, rest) = if let Some(r) = t.strip_prefix('<') {
        ("< ", r.trim())
    } else if let Some(r) = t.strip_prefix('>') {
        ("> ", r.trim())
    } else {
        ("", t)
    };
    match rest.parse::<f64>() {
        Ok(p) if (0.0..=1.0).contains(&p) => format!("{prefix}{:.0}%", p * 100.0),
        _ => t.to_string(),
    }
}

/// Everything the gauge's own metadata document says that the map list does not.
#[derive(Debug, Clone, PartialEq)]
pub struct GaugeDetail {
    /// Id, name, position and the current observed / forecast status.
    pub summary: Gauge,
    /// When the observed status reading was taken.
    pub observed_time: Option<DateTime<Utc>>,
    /// Flow at that reading, in [`Self::flow_units`].
    pub observed_flow: Option<f64>,
    pub flow_units: String,
    pub usgs_id: String,
    pub county: String,
    /// Two-letter state.
    pub state: String,
    /// The forecast office (e.g. `EWX`) and river forecast center (e.g. `WGRFC`).
    pub wfo: String,
    pub rfc: String,
    pub thresholds: Thresholds,
    /// The highest crests on record, highest first.
    pub historic: Vec<Crest>,
    /// The latest notable crests, newest first.
    pub recent: Vec<Crest>,
    /// Impact statements, highest stage first.
    pub impacts: Vec<Impact>,
    pub outlook: Option<Outlook>,
    /// How often forecasts are issued here ("issued routinely year-round", "during high water").
    pub reliability: String,
    pub upstream: Option<String>,
    pub downstream: Option<String>,
    /// False (with a reason in `service_note`) when the gauge is out of service.
    pub in_service: bool,
    pub service_note: String,
}

impl GaugeDetail {
    /// The impact statement for the highest listed stage at or below `stage_ft`: what is already
    /// happening at that level.
    pub fn impact_at(&self, stage_ft: f64) -> Option<&Impact> {
        self.impacts
            .iter()
            .filter(|i| i.stage_ft <= stage_ft)
            .max_by(|a, b| a.stage_ft.total_cmp(&b.stage_ft))
    }

    /// The record crest.
    pub fn record(&self) -> Option<&Crest> {
        self.historic
            .iter()
            .max_by(|a, b| a.stage_ft.total_cmp(&b.stage_ft))
    }
}

fn time_of(m: &serde_json::Value, k: &str) -> Option<DateTime<Utc>> {
    let t = DateTime::parse_from_rfc3339(m.get(k)?.as_str()?).ok()?;
    // The service writes 0001-01-01 for "no time"; nothing real predates 1970 on a live gauge,
    // but historic crests do (1869 at Austin), so only the sentinel year is refused.
    let t = t.with_timezone(&Utc);
    (chrono::Datelike::year(&t) > 1).then_some(t)
}

/// A category's flood stage, or `None` where the gauge leaves it undefined (the service writes
/// `-9999` or omits it).
fn threshold(cats: Option<&serde_json::Value>, name: &str) -> Option<f64> {
    cats?
        .get(name)
        .and_then(|c| num(c, "stage"))
        .filter(|s| *s > -900.0)
}

fn crests(list: Option<&serde_json::Value>) -> Vec<Crest> {
    list.and_then(|l| l.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    Some(Crest {
                        time: time_of(c, "occurredTime")?,
                        stage_ft: stage(num(c, "stage"))?,
                        flow_cfs: num(c, "flow").filter(|f| *f > 0.0),
                        preliminary: c.get("preliminary").and_then(|p| p.as_str()) == Some("P"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a gauge's metadata document (`/gauges/{lid}`).
pub fn parse_detail(json: &str) -> anyhow::Result<GaugeDetail> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    let summary = gauge_of(&v).ok_or_else(|| anyhow::anyhow!("the gauge has no position"))?;
    let obs = v.pointer("/status/observed");
    let flood = v.get("flood");
    let cats = flood.and_then(|f| f.get("categories"));
    let mut historic = crests(flood.and_then(|f| f.pointer("/crests/historic")));
    historic.sort_by(|a, b| b.stage_ft.total_cmp(&a.stage_ft));
    let mut recent = crests(flood.and_then(|f| f.pointer("/crests/recent")));
    recent.sort_by_key(|c| std::cmp::Reverse(c.time));
    let mut impacts: Vec<Impact> = flood
        .and_then(|f| f.get("impacts"))
        .and_then(|i| i.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|i| {
                    let statement = str_of(i, "statement").trim().to_string();
                    if statement.is_empty() {
                        return None;
                    }
                    Some(Impact {
                        stage_ft: stage(num(i, "stage"))?,
                        statement,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    impacts.sort_by(|a, b| b.stage_ft.total_cmp(&a.stage_ft));
    let outlook = flood.and_then(|f| f.get("lro")).and_then(|l| {
        let o = Outlook {
            minor: str_of(l, "minorCS"),
            moderate: str_of(l, "moderateCS"),
            major: str_of(l, "majorCS"),
            interval: str_of(l, "interval"),
        };
        (!(o.minor.is_empty() && o.moderate.is_empty() && o.major.is_empty())).then_some(o)
    });
    let abbrev = |k: &str| {
        v.get(k)
            .and_then(|o| o.get("abbreviation"))
            .and_then(|a| a.as_str())
            .unwrap_or("")
            .to_string()
    };
    let neighbor = |k: &str| {
        let l = str_of(&v, k);
        (!l.trim().is_empty()).then_some(l)
    };
    Ok(GaugeDetail {
        observed_time: obs.and_then(|o| time_of(o, "validTime")),
        observed_flow: obs.and_then(|o| num(o, "secondary")).filter(|f| *f >= 0.0),
        flow_units: obs.map(|o| str_of(o, "secondaryUnit")).unwrap_or_default(),
        usgs_id: str_of(&v, "usgsId"),
        county: str_of(&v, "county"),
        state: abbrev("state"),
        wfo: abbrev("wfo"),
        rfc: abbrev("rfc"),
        thresholds: Thresholds {
            action: threshold(cats, "action"),
            minor: threshold(cats, "minor"),
            moderate: threshold(cats, "moderate"),
            major: threshold(cats, "major"),
        },
        historic,
        recent,
        impacts,
        outlook,
        reliability: str_of(&v, "forecastReliability"),
        upstream: neighbor("upstreamLid"),
        downstream: neighbor("downstreamLid"),
        in_service: v
            .pointer("/inService/enabled")
            .and_then(|e| e.as_bool())
            .unwrap_or(true),
        service_note: v
            .pointer("/inService/message")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string(),
        summary,
    })
}

/// One point of a hydrograph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    pub time: DateTime<Utc>,
    pub stage_ft: Option<f64>,
    /// In the series' [`Series::flow_units`].
    pub flow: Option<f64>,
}

/// Observed or forecast readings, oldest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Series {
    /// When the forecast was issued (or the observations last collected).
    pub issued: Option<DateTime<Utc>>,
    pub readings: Vec<Reading>,
    pub flow_units: String,
}

impl Series {
    /// The readings at or after `t`.
    pub fn since(&self, t: DateTime<Utc>) -> &[Reading] {
        let i = self.readings.partition_point(|r| r.time < t);
        &self.readings[i..]
    }

    /// The newest reading with a stage.
    pub fn latest(&self) -> Option<&Reading> {
        self.readings.iter().rev().find(|r| r.stage_ft.is_some())
    }
}

/// A gauge's observed and forecast stage and flow.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hydrograph {
    pub observed: Series,
    /// Empty where no forecast is issued, or the last one has expired.
    pub forecast: Series,
}

fn series(node: Option<&serde_json::Value>) -> Series {
    let Some(n) = node else {
        return Series::default();
    };
    let mut readings: Vec<Reading> = n
        .get("data")
        .and_then(|d| d.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    Some(Reading {
                        time: time_of(r, "validTime")?,
                        stage_ft: stage(num(r, "primary")),
                        flow: num(r, "secondary").filter(|f| *f >= 0.0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    readings.sort_by_key(|r| r.time);
    readings.dedup_by_key(|r| r.time);
    Series {
        issued: time_of(n, "issuedTime"),
        readings,
        flow_units: str_of(n, "secondaryUnits"),
    }
}

/// Parse a gauge's hydrograph document (`/gauges/{lid}/stageflow`).
pub fn parse_hydrograph(json: &str) -> anyhow::Result<Hydrograph> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    Ok(Hydrograph {
        observed: series(v.get("observed")),
        forecast: series(v.get("forecast")),
    })
}

/// Where in a stretch of readings the highest stage falls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeakKind {
    /// Inside the stretch: the river came up to it and went back down. A crest.
    Crest,
    /// At the start: the river only fell from there.
    AtStart,
    /// At the end: the river is still at (or rising toward) its highest.
    AtEnd,
}

/// The highest stage in `readings` and where it falls. A flat top is reported where the river
/// first reached it. `None` without two stages to compare.
pub fn peak(readings: &[Reading]) -> Option<(Reading, PeakKind)> {
    let staged: Vec<(usize, f64)> = readings
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.stage_ft.map(|s| (i, s)))
        .collect();
    if staged.len() < 2 {
        return None;
    }
    let (mut bi, mut best) = staged[0];
    for &(i, s) in &staged[1..] {
        if s > best {
            (bi, best) = (i, s);
        }
    }
    // A top that holds to the end is still the river's current level, not a crest behind it.
    let holds = staged
        .iter()
        .filter(|(i, _)| *i > bi)
        .all(|(_, s)| *s >= best);
    let kind = if holds {
        PeakKind::AtEnd
    } else if bi == staged[0].0 {
        PeakKind::AtStart
    } else {
        PeakKind::Crest
    };
    Some((readings[bi], kind))
}

/// Which way the river is going, and how fast.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Trend {
    /// Feet per hour.
    Rising(f64),
    /// Feet per hour, as a positive number.
    Falling(f64),
    Steady,
}

/// The rate of rise over the last `hours` of readings: the newest stage against the one at the
/// start of that span. Changes under a tenth of a foot are steady (gauge noise and wind setup are
/// that large). `None` when the readings do not reach back that far.
pub fn trend(readings: &[Reading], hours: f64) -> Option<Trend> {
    let last = readings.iter().rev().find(|r| r.stage_ft.is_some())?;
    let since = last.time - chrono::Duration::seconds((hours * 3600.0) as i64);
    let first = readings
        .iter()
        .rev()
        .find(|r| r.stage_ft.is_some() && r.time <= since)?;
    let span_h = (last.time - first.time).num_seconds() as f64 / 3600.0;
    if span_h <= 0.0 {
        return None;
    }
    let d = last.stage_ft? - first.stage_ft?;
    Some(if d.abs() < 0.1 {
        Trend::Steady
    } else if d > 0.0 {
        Trend::Rising(d / span_h)
    } else {
        Trend::Falling(-d / span_h)
    })
}

/// A gauge id is a few letters and digits; anything else would change the URL it is put in.
fn check_lid(lid: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !lid.is_empty() && lid.len() <= 8 && lid.chars().all(|c| c.is_ascii_alphanumeric()),
        "not a gauge id: {lid:?}"
    );
    Ok(())
}

async fn get_text(client: &reqwest::Client, url: &str) -> anyhow::Result<String> {
    let response = client
        .get(crate::net::fetch_url(url))
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    anyhow::ensure!(status.is_success(), "{}", describe_failure(status, &body));
    Ok(body)
}

/// Fetch a gauge's metadata: flood stages, crest history, impacts, outlook.
pub async fn fetch_detail(client: &reqwest::Client, lid: &str) -> anyhow::Result<GaugeDetail> {
    check_lid(lid)?;
    parse_detail(&get_text(client, &format!("{GAUGES_URL}/{lid}")).await?)
}

/// Fetch a gauge's observed and forecast stage and flow.
pub async fn fetch_hydrograph(client: &reqwest::Client, lid: &str) -> anyhow::Result<Hydrograph> {
    check_lid(lid)?;
    parse_hydrograph(&get_text(client, &format!("{GAUGES_URL}/{lid}/stageflow")).await?)
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

    /// `/gauges/ACRT2`, trimmed: the Colorado at Austin, with a routine forecast.
    const ACRT2: &str = r#"{"lid":"ACRT2","usgsId":"08158000","reachId":"5781703",
        "name":"Colorado River (TX) at Austin","description":"",
        "rfc":{"abbreviation":"WGRFC","name":"West Gulf River Forecast Center"},
        "wfo":{"abbreviation":"EWX","name":"New Braunfels"},
        "state":{"abbreviation":"TX","name":"Texas"},"county":"Travis","timeZone":"CST6CDT",
        "latitude":30.244,"longitude":-97.694,
        "status":{"observed":{"primary":13.04,"primaryUnit":"ft","secondary":0.234,
            "secondaryUnit":"kcfs","floodCategory":"no_flooding","validTime":"2026-09-25T23:50:00Z"},
          "forecast":{"primary":16.7,"primaryUnit":"ft","secondary":1.87,"secondaryUnit":"kcfs",
            "floodCategory":"no_flooding","validTime":"2026-09-29T09:00:00Z"}},
        "flood":{"stageUnits":"ft","flowUnits":"cfs",
          "categories":{"major":{"stage":42,"flow":-9999},"moderate":{"stage":38,"flow":-9999},
            "minor":{"stage":33,"flow":-9999},"action":{"stage":25,"flow":-9999}},
          "lro":{"minorCS":"0.1166","moderateCS":"0.0689","majorCS":"< 0.05",
            "producedTime":"2026-08-28T17:12:09Z","interval":"SON"},
          "crests":{"historic":[
              {"occurredTime":"1935-06-15T00:00:00Z","stage":50,"flow":481000,"preliminary":"O","olddatum":false},
              {"occurredTime":"1869-07-07T00:00:00Z","stage":51,"flow":550000,"preliminary":"R","olddatum":false}],
            "recent":[
              {"occurredTime":"2015-05-26T00:40:00Z","stage":34.09,"flow":34400,"preliminary":"O","olddatum":false},
              {"occurredTime":"2015-10-30T18:00:00Z","stage":33.22,"flow":32129,"preliminary":"P","olddatum":false},
              {"occurredTime":"2010-09-08T00:00:00Z","stage":27.7,"flow":0,"preliminary":"O","olddatum":false}]},
          "impacts":[{"stage":33,"statement":"Minor lowland flooding begins."},
            {"stage":46,"statement":"Several homes flood."},{"stage":40,"statement":"  "}]},
        "forecastReliability":"Forecasts are issued routinely year-round.",
        "upstreamLid":"ALKT2","downstreamLid":"BRTT2","inService":{"enabled":true,"message":""}}"#;

    /// `/gauges/ACRT2/stageflow`, trimmed: a few observations and a forecast that crests.
    const ACRT2_SF: &str = r#"{"observed":{"pedts":"HGIRG","issuedTime":"2026-09-26T00:09:20Z",
          "primaryName":"Stage","primaryUnits":"ft","secondaryName":"Flow","secondaryUnits":"kcfs",
          "data":[
            {"validTime":"2026-09-25T20:00:00Z","primary":12.6,"secondary":0.2},
            {"validTime":"2026-09-25T18:00:00Z","primary":12.5,"secondary":0.2},
            {"validTime":"2026-09-25T22:00:00Z","primary":12.9,"secondary":0.22},
            {"validTime":"2026-09-25T23:50:00Z","primary":13.04,"secondary":0.234},
            {"validTime":"2026-09-25T21:00:00Z","primary":-999,"secondary":-999}]},
        "forecast":{"issuedTime":"2026-09-25T13:30:00Z","secondaryUnits":"kcfs","data":[
            {"validTime":"2026-09-28T00:00:00Z","primary":14.0,"secondary":0.5},
            {"validTime":"2026-09-29T09:00:00Z","primary":16.7,"secondary":1.87},
            {"validTime":"2026-09-30T13:00:00Z","primary":15.1,"secondary":1.0}]}}"#;

    #[test]
    fn gauge_detail_reads_stages_crests_impacts_and_outlook() {
        let d = parse_detail(ACRT2).unwrap();
        assert_eq!(d.summary.lid, "ACRT2");
        assert_eq!(d.summary.stage_ft, Some(13.04));
        assert_eq!(d.summary.forecast_ft, Some(16.7));
        assert_eq!(d.observed_flow, Some(0.234));
        assert_eq!(d.flow_units, "kcfs");
        assert_eq!((d.wfo.as_str(), d.rfc.as_str()), ("EWX", "WGRFC"));
        assert_eq!(d.state, "TX");
        assert_eq!(
            d.thresholds,
            Thresholds {
                action: Some(25.0),
                minor: Some(33.0),
                moderate: Some(38.0),
                major: Some(42.0)
            }
        );
        // Historic highest first, reaching back past 1970; recent newest first.
        assert_eq!(d.historic[0].stage_ft, 51.0);
        assert_eq!(chrono::Datelike::year(&d.historic[0].time), 1869);
        assert_eq!(d.record().unwrap().stage_ft, 51.0);
        let recent: Vec<f64> = d.recent.iter().map(|c| c.stage_ft).collect();
        assert_eq!(recent, [33.22, 34.09, 27.7]);
        assert!(d.recent[0].preliminary);
        assert_eq!(
            d.recent[2].flow_cfs, None,
            "a zero flow is no flow on record"
        );
        // The blank statement is dropped; the rest are highest first.
        assert_eq!(d.impacts.len(), 2);
        assert_eq!(d.impacts[0].stage_ft, 46.0);
        assert_eq!(d.impact_at(35.0).unwrap().stage_ft, 33.0);
        assert!(d.impact_at(20.0).is_none());
        let o = d.outlook.unwrap();
        assert_eq!(chance_percent(&o.minor), "12%");
        assert_eq!(chance_percent(&o.major), "< 5%");
        assert_eq!(chance_percent("n/a"), "n/a");
        assert_eq!(d.upstream.as_deref(), Some("ALKT2"));
        assert!(d.in_service);
    }

    #[test]
    fn a_gauge_without_flood_stages_or_forecast_still_parses() {
        let d = parse_detail(
            r#"{"lid":"DRYT2","name":"Dry Creek","latitude":32.5,"longitude":-96.8,
                "status":{"observed":{"primary":-999,"floodCategory":"obs_not_current",
                  "validTime":"0001-01-01T00:00:00Z"}},
                "flood":{"categories":{"major":{"stage":-9999},"minor":{"stage":null}}},
                "upstreamLid":"","inService":{"enabled":false,"message":"Gauge damaged"}}"#,
        )
        .unwrap();
        assert_eq!(d.thresholds, Thresholds::default());
        assert_eq!(d.thresholds.category(10.0), FloodCat::Unknown);
        assert_eq!(d.observed_time, None, "the 0001 sentinel is no time");
        assert!(d.historic.is_empty() && d.impacts.is_empty() && d.outlook.is_none());
        assert_eq!(d.upstream, None);
        assert!(!d.in_service);
        assert_eq!(d.service_note, "Gauge damaged");
        assert!(parse_detail(r#"{"lid":"X"}"#).is_err(), "no position");
    }

    #[test]
    fn thresholds_place_a_stage_in_its_category() {
        let t = Thresholds {
            action: Some(25.0),
            minor: Some(33.0),
            moderate: Some(38.0),
            major: None,
        };
        assert_eq!(t.category(10.0), FloodCat::NoFlooding);
        assert_eq!(t.category(25.0), FloodCat::Action);
        assert_eq!(t.category(37.9), FloodCat::Minor);
        assert_eq!(t.category(60.0), FloodCat::Moderate);
        assert_eq!(t.next_above(13.0), Some((FloodCat::Action, 25.0)));
        assert_eq!(t.next_above(38.0), None);
    }

    #[test]
    fn hydrograph_is_sorted_and_windowed() {
        let h = parse_hydrograph(ACRT2_SF).unwrap();
        let times: Vec<u32> = h
            .observed
            .readings
            .iter()
            .map(|r| chrono::Timelike::hour(&r.time))
            .collect();
        assert_eq!(times, [18, 20, 21, 22, 23], "oldest first");
        assert_eq!(
            h.observed.readings[2].stage_ft, None,
            "the -999 gap stays a gap"
        );
        assert_eq!(h.observed.flow_units, "kcfs");
        assert_eq!(h.observed.latest().unwrap().stage_ft, Some(13.04));
        let t = |s: &str| s.parse::<DateTime<Utc>>().unwrap();
        assert_eq!(h.observed.since(t("2026-09-25T21:30:00Z")).len(), 2);
        assert_eq!(h.forecast.issued, Some(t("2026-09-25T13:30:00Z")));
        assert_eq!(h.forecast.readings.len(), 3);
        // A gauge with no forecast answers with an empty series, not an error.
        let none = parse_hydrograph(r#"{"observed":{"data":[]},"forecast":{"data":[]}}"#).unwrap();
        assert!(none.forecast.readings.is_empty() && none.observed.latest().is_none());
    }

    fn readings(stages: &[f64]) -> Vec<Reading> {
        let t0 = "2026-09-25T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        stages
            .iter()
            .enumerate()
            .map(|(i, s)| Reading {
                time: t0 + chrono::Duration::hours(i as i64),
                stage_ft: (*s > -900.0).then_some(*s),
                flow: None,
            })
            .collect()
    }

    #[test]
    fn peak_tells_a_crest_from_a_rise_or_a_fall() {
        let crest = readings(&[10.0, 12.0, 14.0, 14.0, 13.0]);
        let (r, k) = peak(&crest).unwrap();
        assert_eq!((r.stage_ft, k), (Some(14.0), PeakKind::Crest));
        assert_eq!(
            r.time, crest[2].time,
            "a flat top is dated where the river reached it"
        );
        assert_eq!(
            peak(&readings(&[10.0, 11.0, 12.0])).unwrap().1,
            PeakKind::AtEnd
        );
        assert_eq!(
            peak(&readings(&[10.0, 12.0, 12.0])).unwrap().1,
            PeakKind::AtEnd,
            "holding at the top is still the current level"
        );
        assert_eq!(
            peak(&readings(&[12.0, 11.0, 10.0])).unwrap().1,
            PeakKind::AtStart
        );
        assert!(peak(&readings(&[12.0, -999.0])).is_none());
        // The forecast in the fixture crests in the middle.
        let h = parse_hydrograph(ACRT2_SF).unwrap();
        let (r, k) = peak(&h.forecast.readings).unwrap();
        assert_eq!((r.stage_ft, k), (Some(16.7), PeakKind::Crest));
    }

    #[test]
    fn trend_reads_the_rate_over_its_window() {
        // One foot over three hours.
        match trend(&readings(&[10.0, 10.2, 10.5, 11.0]), 3.0) {
            Some(Trend::Rising(r)) => assert!((r - 1.0 / 3.0).abs() < 1e-9),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            trend(&readings(&[11.0, 10.0, 9.0]), 2.0),
            Some(Trend::Falling(r)) if (r - 1.0).abs() < 1e-9
        ));
        assert_eq!(
            trend(&readings(&[10.0, 10.05, 10.02]), 2.0),
            Some(Trend::Steady)
        );
        assert_eq!(
            trend(&readings(&[10.0, 11.0]), 3.0),
            None,
            "too short a record"
        );
        assert!(check_lid("ACRT2").is_ok());
        assert!(check_lid("../x").is_err() && check_lid("").is_err());
        assert_eq!(page_url("ACRT2"), "https://water.noaa.gov/gauges/acrt2");
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_a_real_gauge_and_hydrograph() {
        let client = reqwest::Client::new();
        let d = fetch_detail(&client, "ACRT2").await.unwrap();
        let h = fetch_hydrograph(&client, "ACRT2").await.unwrap();
        eprintln!(
            "{}: {:?}, {} observed, {} forecast",
            d.summary.name,
            d.thresholds,
            h.observed.readings.len(),
            h.forecast.readings.len()
        );
        assert!(!h.observed.readings.is_empty());
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
