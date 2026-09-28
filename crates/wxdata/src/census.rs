//! Who is inside a polygon: the 2020 Census population, housing units and largest towns within
//! an alert or discussion area, from the Census Bureau's TIGERweb service.
//!
//! One query sums `POP100` and `HU100` over the census blocks the polygon touches (the server
//! does the sum, so a county-sized advisory is one small reply); another lists the incorporated
//! places and census-designated places it touches, largest first. Blocks that straddle the edge
//! count whole, so the figure is "about": at block size (a city block, a few rural square miles)
//! the overcount is small next to what a warning covers.

use crate::alerts::USER_AGENT;

const TIGERWEB: &str =
    "https://tigerweb.geo.census.gov/arcgis/rest/services/TIGERweb/tigerWMS_Census2020/MapServer";
/// Census blocks, incorporated places and census-designated places.
const BLOCKS: u32 = 10;
const PLACES: u32 = 26;
const CDPS: u32 = 28;
/// Most places listed.
pub const MAX_PLACES: usize = 8;

/// The people and places inside an area.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Impact {
    pub population: u64,
    pub housing_units: u64,
    /// `(name, population)`, largest first, at most [`MAX_PLACES`].
    pub places: Vec<(String, u64)>,
}

/// An Esri JSON polygon from `rings` (`[lon, lat]`), each ring taken as an outer boundary and
/// wound clockwise, which is what Esri reads as outer (GeoJSON winds outer rings the other way,
/// and a ring read as a hole would count nobody).
pub fn esri_polygon(rings: &[Vec<[f64; 2]>]) -> String {
    let rings: Vec<Vec<[f64; 2]>> = rings
        .iter()
        .filter(|r| r.len() >= 3)
        .map(|r| {
            let mut r = r.clone();
            if signed_area(&r) > 0.0 {
                r.reverse();
            }
            if r.first() != r.last() {
                r.push(r[0]);
            }
            r
        })
        .collect();
    serde_json::json!({ "rings": rings, "spatialReference": { "wkid": 4326 } }).to_string()
}

/// Twice the signed area (shoelace); positive is counter-clockwise with x = lon, y = lat.
fn signed_area(r: &[[f64; 2]]) -> f64 {
    r.iter()
        .zip(r.iter().cycle().skip(1))
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum()
}

async fn query(
    client: &reqwest::Client,
    layer: u32,
    geometry: &str,
    extra: &[(&str, &str)],
) -> anyhow::Result<serde_json::Value> {
    let mut form: Vec<(&str, &str)> = vec![
        ("geometry", geometry),
        ("geometryType", "esriGeometryPolygon"),
        ("inSR", "4326"),
        ("spatialRel", "esriSpatialRelIntersects"),
        ("returnGeometry", "false"),
        ("f", "json"),
    ];
    form.extend_from_slice(extra);
    // POST: an alert drawn from zone shapes can run to thousands of vertices, past a URL's length.
    let v: serde_json::Value = client
        .post(format!("{TIGERWEB}/{layer}/query"))
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .form(&form)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await
        .map(|t| serde_json::from_str(&t))??;
    if let Some(e) = v.get("error") {
        anyhow::bail!("TIGERweb: {e}");
    }
    Ok(v)
}

fn attrs(v: &serde_json::Value) -> impl Iterator<Item = &serde_json::Value> {
    v.get("features")
        .and_then(|f| f.as_array())
        .into_iter()
        .flatten()
        .filter_map(|f| f.get("attributes"))
}

/// Parse the block statistics reply: `(population, housing units)`.
pub fn parse_totals(v: &serde_json::Value) -> (u64, u64) {
    let a = attrs(v).next();
    let n = |k: &str| {
        a.and_then(|a| a.get(k))
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0)
            .max(0.0) as u64
    };
    (n("pop"), n("hu"))
}

/// Parse a places reply into `(name, population)`.
pub fn parse_places(v: &serde_json::Value) -> Vec<(String, u64)> {
    attrs(v)
        .filter_map(|a| {
            let name = a.get("NAME")?.as_str()?.to_string();
            let pop = a.get("POP100")?.as_f64()?.max(0.0) as u64;
            Some((name, pop))
        })
        .collect()
}

/// The population, housing units and largest places inside `rings`.
pub async fn impact(client: &reqwest::Client, rings: &[Vec<[f64; 2]>]) -> anyhow::Result<Impact> {
    let geometry = esri_polygon(rings);
    if geometry.contains("\"rings\":[]") {
        anyhow::bail!("no area");
    }
    let stats = r#"[{"statisticType":"sum","onStatisticField":"POP100","outStatisticFieldName":"pop"},{"statisticType":"sum","onStatisticField":"HU100","outStatisticFieldName":"hu"}]"#;
    let top = MAX_PLACES.to_string();
    let place_args = [
        ("outFields", "NAME,POP100"),
        ("orderByFields", "POP100 DESC"),
        ("resultRecordCount", top.as_str()),
    ];
    let block_args = [("outStatistics", stats)];
    let (totals, places, cdps) = futures_util::try_join!(
        query(client, BLOCKS, &geometry, &block_args),
        query(client, PLACES, &geometry, &place_args),
        query(client, CDPS, &geometry, &place_args),
    )?;
    let (population, housing_units) = parse_totals(&totals);
    let mut all = parse_places(&places);
    all.extend(parse_places(&cdps));
    all.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    all.truncate(MAX_PLACES);
    Ok(Impact {
        population,
        housing_units,
        places: all,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_are_wound_clockwise_and_closed() {
        // Counter-clockwise, open: reversed and closed.
        let ccw = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let g: serde_json::Value = serde_json::from_str(&esri_polygon(&[ccw])).unwrap();
        let ring: Vec<[f64; 2]> = serde_json::from_value(g["rings"][0].clone()).unwrap();
        assert!(signed_area(&ring) < 0.0, "clockwise");
        assert_eq!(ring.first(), ring.last());
        assert_eq!(g["spatialReference"]["wkid"], 4326);
        // A degenerate ring is dropped.
        assert!(esri_polygon(&[vec![[0.0, 0.0], [1.0, 1.0]]]).contains("\"rings\":[]"));
    }

    #[test]
    fn parses_totals_and_places() {
        let totals: serde_json::Value =
            serde_json::from_str(r#"{"features":[{"attributes":{"pop":281434,"hu":129785}}]}"#)
                .unwrap();
        assert_eq!(parse_totals(&totals), (281_434, 129_785));
        let places: serde_json::Value = serde_json::from_str(
            r#"{"features":[{"attributes":{"NAME":"Oklahoma City city","POP100":681054}},
               {"attributes":{"NAME":"Del City city","POP100":21822}}]}"#,
        )
        .unwrap();
        assert_eq!(
            parse_places(&places),
            vec![
                ("Oklahoma City city".to_string(), 681_054),
                ("Del City city".to_string(), 21_822)
            ]
        );
        assert_eq!(parse_totals(&serde_json::json!({})), (0, 0));
    }

    /// Central Oklahoma City, live.
    /// `cargo test -p wxdata census_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "network"]
    async fn census_live() {
        let ring = vec![
            [-97.60, 35.40],
            [-97.40, 35.40],
            [-97.40, 35.55],
            [-97.60, 35.55],
        ];
        let i = impact(&reqwest::Client::new(), &[ring]).await.unwrap();
        println!("{i:?}");
        assert!(i.population > 200_000 && i.housing_units > 100_000);
        assert_eq!(i.places[0].0, "Oklahoma City city");
    }
}
