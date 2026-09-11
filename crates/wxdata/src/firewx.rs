//! SPC Fire Weather Outlook (Day 1–2): categorical risk plus the dry-thunderstorm hazard, from
//! the same ArcGIS map service SPC's own web viewer reads.
//!
//! Days 3–8 split into "dry thunderstorm" and "winds and low humidity" layers instead of a single
//! categorical risk (a coarser, longer-range method — see the service's own layer list), so this
//! stays Day 1–2 only: the pair every fire-weather viewer leads with, and the one every other
//! "outlook" layer in this app (SPC convective, ERO, WSSI) already follows the same Day-N shape
//! for.
//!
//! Unlike the convective outlook's static `.lyr.geojson` files, there is no plain download here —
//! SPC's own site only links a KMZ, and the GeoJSON lives behind this ArcGIS `query` endpoint
//! instead (same family of service as the mesoscale-discussion and watch-box layers this crate
//! already reads).

use crate::alerts::USER_AGENT;
use crate::overlay::{for_each_feature, polygons_of, FeatureKind, GeoFeature};

const BASE: &str =
    "https://mapservices.weather.noaa.gov/vector/rest/services/fire_weather/SPC_firewx/MapServer";

/// The categorical-risk sublayer id for `day` (1 or 2); `None` otherwise.
fn risk_layer(day: u8) -> Option<u32> {
    match day {
        1 => Some(1),
        2 => Some(4),
        _ => None,
    }
}

/// The dry-thunderstorm-hazard sublayer id for `day` (1 or 2); `None` otherwise.
fn dryt_layer(day: u8) -> Option<u32> {
    match day {
        1 => Some(2),
        2 => Some(5),
        _ => None,
    }
}

/// Categorical risk label/color for the service's own `dn` code — its ArcGIS renderer's break
/// points (`MapServer/1?f=json`'s `drawingInfo.renderer.uniqueValueInfos`), since the query
/// endpoint returns the raw code and not the label or fill the KMZ export carries.
fn risk_color(dn: i64) -> (&'static str, [u8; 3]) {
    match dn {
        8 => ("Critical", [255, 0, 0]),
        10 => ("Extreme", [230, 0, 169]),
        _ => ("Elevated", [230, 152, 0]), // 5, and anything the service adds later
    }
}

/// Dry-thunderstorm hazard label/color, same idea as [`risk_color`] but the layer's own two-tier
/// scale (isolated vs. scattered) rather than the three-tier categorical one.
fn dryt_color(dn: i64) -> (&'static str, [u8; 3]) {
    match dn {
        8 => ("Scattered dry thunderstorms", [255, 0, 0]),
        _ => ("Isolated dry thunderstorms", [115, 38, 0]), // 5
    }
}

/// `"202609101700"` (the service's own `valid`/`expire` format) → `"10 17:00Z"`.
fn fmt_stamp(s: &str) -> String {
    chrono::NaiveDateTime::parse_from_str(s, "%Y%m%d%H%M")
        .map(|t| t.format("%d %H:%MZ").to_string())
        .unwrap_or_else(|_| s.to_string())
}

async fn fetch_layer(client: &reqwest::Client, layer: u32) -> anyhow::Result<String> {
    let url = format!("{BASE}/{layer}/query?where=1%3D1&outFields=dn,valid,expire&f=geojson");
    Ok(client
        .get(crate::net::fetch_url(&url))
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?)
}

/// Parse one sublayer's GeoJSON into features, using `color_of` to turn its `dn` code into a
/// label and color and `heading` as the first line of every feature's detail text.
fn parse_layer(
    json: &str,
    day: u8,
    color_of: impl Fn(i64) -> (&'static str, [u8; 3]),
    heading: &str,
) -> anyhow::Result<Vec<GeoFeature>> {
    let mut out = Vec::new();
    for_each_feature(json, |geom, props| {
        let dn = props.get("dn").and_then(serde_json::Value::as_i64).unwrap_or(0);
        let (label, rgb) = color_of(dn);
        let valid = props
            .get("valid")
            .and_then(|v| v.as_str())
            .map(fmt_stamp)
            .unwrap_or_default();
        let expire = props
            .get("expire")
            .and_then(|v| v.as_str())
            .map(fmt_stamp)
            .unwrap_or_default();
        let title = format!("Day {day} Fire Weather: {label}");
        let detail = format!("{heading}\n{label}\nValid {valid} \u{2013} {expire}");
        for rings in polygons_of(geom) {
            out.push(GeoFeature {
                rings,
                fill: [rgb[0], rgb[1], rgb[2], 70],
                stroke: [rgb[0], rgb[1], rgb[2], 230],
                kind: FeatureKind::Outlook,
                title: title.clone(),
                detail: detail.clone(),
                alert: None,
            });
        }
    })?;
    Ok(out)
}

/// Fetch Day 1 or Day 2's fire weather outlook: the categorical risk and the dry-thunderstorm
/// hazard together, since the two hazards share nothing to pick between — a chaser watching fire
/// weather wants both on at once, the same way the app already draws SPC's SIG hatch alongside a
/// probabilistic hail/wind outlook rather than behind a second toggle.
pub async fn fetch(client: &reqwest::Client, day: u8) -> anyhow::Result<Vec<GeoFeature>> {
    let (Some(risk), Some(dryt)) = (risk_layer(day), dryt_layer(day)) else {
        anyhow::bail!("fire weather outlook is Day 1-2 only");
    };
    let risk_json = fetch_layer(client, risk).await?;
    let dryt_json = fetch_layer(client, dryt).await?;
    let mut out = parse_layer(&risk_json, day, risk_color, "SPC Fire Weather Outlook")?;
    out.extend(parse_layer(
        &dryt_json,
        day,
        dryt_color,
        "SPC Fire Weather Outlook \u{2014} dry thunderstorm risk",
    )?);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RISK_SAMPLE: &str = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature",
         "geometry":{"type":"Polygon","coordinates":[[[-100,35],[-98,35],[-98,37],[-100,35]]]},
         "properties":{"dn":8,"valid":"202609101700","expire":"202609111200"}},
        {"type":"Feature",
         "geometry":{"type":"Polygon","coordinates":[[[-90,35],[-89,35],[-89,36],[-90,35]]]},
         "properties":{"dn":5,"valid":"202609101700","expire":"202609111200"}}]}"#;

    #[test]
    fn risk_layer_picks_the_dn_coded_color_and_label() {
        let feats = parse_layer(RISK_SAMPLE, 1, risk_color, "SPC Fire Weather Outlook").unwrap();
        assert_eq!(feats.len(), 2);
        assert_eq!(feats[0].title, "Day 1 Fire Weather: Critical");
        assert_eq!(feats[0].stroke, [255, 0, 0, 230]);
        assert_eq!(feats[1].title, "Day 1 Fire Weather: Elevated");
        assert_eq!(feats[1].stroke, [230, 152, 0, 230]);
        assert!(feats[0].detail.contains("Valid 10 17:00Z"));
        assert!(feats[0].detail.contains("11 12:00Z"));
    }

    #[test]
    fn dry_thunderstorm_layer_uses_its_own_two_tier_scale() {
        let feats = parse_layer(RISK_SAMPLE, 2, dryt_color, "dry thunderstorm risk").unwrap();
        assert_eq!(feats[0].title, "Day 2 Fire Weather: Scattered dry thunderstorms");
        assert_eq!(feats[1].title, "Day 2 Fire Weather: Isolated dry thunderstorms");
    }

    #[test]
    fn only_day_1_and_2_are_supported() {
        assert_eq!(risk_layer(1), Some(1));
        assert_eq!(risk_layer(2), Some(4));
        assert_eq!(risk_layer(3), None);
        assert_eq!(dryt_layer(1), Some(2));
        assert_eq!(dryt_layer(2), Some(5));
    }

    #[test]
    fn a_malformed_stamp_is_passed_through_rather_than_dropped() {
        assert_eq!(fmt_stamp("not-a-date"), "not-a-date");
        assert_eq!(fmt_stamp("202609101700"), "10 17:00Z");
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_live_day1_outlook() {
        let c = reqwest::Client::new();
        // Not asserting non-empty: a quiet fire-weather day can legitimately have no polygons.
        fetch(&c, 1).await.expect("day 1 fire weather outlook");
    }
}
