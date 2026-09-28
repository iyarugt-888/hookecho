//! Archived SPC mesoscale discussions from the Iowa Environmental Mesonet, so a scrubbed or
//! replayed radar frame shows the discussions that were in effect then, as it already does for
//! warnings ([`crate::archive_warnings`]).
//!
//! `/api/1/nws/spc_mcd.geojson?valid=T&hours=H` returns every discussion issued in the `H` hours
//! before `T`, with its polygon, issue and expiry; the ones whose window covers `T` are kept. Each
//! one's text comes from `/api/1/nwstext/{product_id}`, as the live layer's does from SPC.

use crate::alerts::USER_AGENT;
use crate::overlay::{for_each_feature, polygons_of, FeatureKind, GeoFeature};

const MCD_URL: &str = "https://mesonet.agron.iastate.edu/api/1/nws/spc_mcd.geojson";
const TEXT_URL: &str = "https://mesonet.agron.iastate.edu/api/1/nwstext/";
/// How far back to look for discussions still valid at the time: SPC's run up to a few hours.
const LOOKBACK_HOURS: u32 = 6;

/// One archived discussion before its text is fetched.
#[derive(Debug, Clone)]
pub struct ArchivedMd {
    pub feature: GeoFeature,
    pub product_id: String,
}

/// Parse the IEM discussion GeoJSON, keeping those valid at `at`.
pub fn parse(json: &str, at: chrono::DateTime<chrono::Utc>) -> anyhow::Result<Vec<ArchivedMd>> {
    let mut out = Vec::new();
    for_each_feature(json, |geom, props| {
        let s = |k: &str| -> String {
            match props.get(k) {
                Some(serde_json::Value::String(v)) => v.clone(),
                Some(serde_json::Value::Number(n)) => n.to_string(),
                _ => String::new(),
            }
        };
        let time = |k: &str| {
            chrono::DateTime::parse_from_rfc3339(&s(k))
                .ok()
                .map(|t| t.with_timezone(&chrono::Utc))
        };
        let (Some(issue), Some(expire)) = (time("issue"), time("expire")) else {
            return;
        };
        if at < issue || at > expire {
            return;
        }
        let num = s("num");
        let confidence = s("watch_confidence")
            .parse::<f32>()
            .ok()
            .map(|c| format!("\nProbability of a watch: {c:.0}%"))
            .unwrap_or_default();
        let detail = format!(
            "Issued {} UTC, valid to {} UTC{confidence}",
            issue.format("%Y-%m-%d %H:%M"),
            expire.format("%H:%M")
        );
        for rings in polygons_of(geom) {
            out.push(ArchivedMd {
                feature: GeoFeature {
                    rings,
                    fill: [255, 120, 0, 30],
                    stroke: [255, 140, 0, 235],
                    kind: FeatureKind::MesoDiscussion,
                    title: format!("Mesoscale Discussion {num}"),
                    detail: detail.clone(),
                    alert: None,
                },
                product_id: s("product_id"),
            });
        }
    })?;
    Ok(out)
}

/// The discussions in effect at `at`, each with its text appended to its details.
pub async fn fetch(
    client: &reqwest::Client,
    at: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<Vec<GeoFeature>> {
    let url = format!(
        "{MCD_URL}?valid={}&hours={LOOKBACK_HOURS}",
        at.format("%Y-%m-%dT%H:%M:%SZ")
    );
    let body = client
        .get(crate::net::fetch_url(&url))
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let mds = parse(&body, at)?;
    let mut out = Vec::with_capacity(mds.len());
    for md in mds {
        let mut f = md.feature;
        if !md.product_id.is_empty() {
            // The text is a bonus: a failure leaves the polygon and its times.
            if let Ok(r) = client
                .get(crate::net::fetch_url(&format!(
                    "{TEXT_URL}{}",
                    md.product_id
                )))
                .timeout(crate::net::FEED_TIMEOUT)
                .header("User-Agent", USER_AGENT)
                .send()
                .await
            {
                if let Ok(text) = r.text().await {
                    f.detail = format!("{}\n\n{}", f.detail, text.trim());
                }
            }
        }
        out.push(f);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_discussions_valid_at_the_time() {
        let json = r#"{"type":"FeatureCollection","features":[
          {"type":"Feature","properties":{"product_id":"201305201819-KWNS-ACUS11-SWOMCD","num":727,
           "issue":"2013-05-20T18:19:00Z","expire":"2013-05-20T20:15:00Z","watch_confidence":95.0},
           "geometry":{"type":"Polygon","coordinates":[[[-98,35],[-97,35],[-97,36],[-98,35]]]}},
          {"type":"Feature","properties":{"product_id":"x","num":"700",
           "issue":"2013-05-20T12:00:00Z","expire":"2013-05-20T14:00:00Z"},
           "geometry":{"type":"Polygon","coordinates":[[[-98,35],[-97,35],[-97,36],[-98,35]]]}}]}"#;
        let at = chrono::DateTime::parse_from_rfc3339("2013-05-20T20:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let mds = parse(json, at).unwrap();
        assert_eq!(mds.len(), 1, "the expired one is dropped");
        assert_eq!(mds[0].feature.title, "Mesoscale Discussion 727");
        assert_eq!(mds[0].feature.kind, FeatureKind::MesoDiscussion);
        assert!(mds[0].feature.detail.contains("95%"));
    }

    /// Moore, 20 May 2013, live.
    /// `cargo test -p wxdata archive_mds_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "network"]
    async fn archive_mds_live() {
        let at = chrono::DateTime::parse_from_rfc3339("2013-05-20T20:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let f = fetch(&reqwest::Client::new(), at).await.unwrap();
        for m in &f {
            println!("{} ({} chars)", m.title, m.detail.len());
        }
        assert!(f.iter().any(|m| m.detail.contains("MESOSCALE DISCUSSION")));
    }
}
