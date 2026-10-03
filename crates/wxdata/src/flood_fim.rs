//! Flood inundation maps for a river gauge: what the river covers at a given stage, from NOAA's
//! stage-based Categorical Flood Inundation Mapping (CatFIM) library.
//!
//! The library maps about 2,500 forecast gauges. For each it holds one polygon per mapped stage,
//! in five layers of thresholds (action, minor, moderate, major, record) and four of
//! one-foot intervals between them. [`fetch_library`] lists every mapped stage of a gauge in one
//! request, without geometry. [`pick`] chooses the polygon that answers "what does the river cover
//! at this stage": the highest mapped stage at or below it, so the map never shows more water than
//! the stage reaches. [`fetch_inundation`] then fetches that one polygon, simplified to about ten
//! meters, in longitude and latitude.
//!
//! Endpoint (no API key; it sends CORS headers):
//! `https://maps.water.noaa.gov/server/rest/services/fim_libs/static_stage_based_catfim/FeatureServer`.
//! Gauges are keyed by their NWS location id in lower case (`ahps_lid`). The threshold layers name
//! their stage `stage` and the interval layers `interval_stage`.

use crate::alerts::USER_AGENT;

const CATFIM_URL: &str =
    "https://maps.water.noaa.gov/server/rest/services/fim_libs/static_stage_based_catfim/FeatureServer";

/// The layers holding polygons, and whether each is an interval layer (`interval_stage`) rather
/// than a threshold one (`stage`): action, minor, moderate and major intervals and thresholds,
/// then the record threshold.
const LAYERS: [(u32, bool); 9] = [
    (2, true),
    (3, false),
    (5, true),
    (6, false),
    (8, true),
    (9, false),
    (11, true),
    (12, false),
    (13, false),
];

/// How close two stages are to count as the same, in feet. The library stores some thresholds
/// with float noise (`14.220000000000001`).
const SAME_FT: f64 = 0.005;

/// One mapped stage of a gauge: the layer its polygon is in, and the stage in feet.
#[derive(Debug, Clone, PartialEq)]
pub struct MappedStage {
    pub layer: u32,
    pub stage_ft: f64,
    /// `action`, `minor`, `moderate`, `major` or `record`, as the library names it.
    pub magnitude: String,
}

impl MappedStage {
    fn stage_field(&self) -> &'static str {
        if LAYERS
            .iter()
            .any(|&(l, interval)| l == self.layer && interval)
        {
            "interval_stage"
        } else {
            "stage"
        }
    }
}

/// Which polygon answers a stage.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    /// The highest mapped stage at or below the one asked for. `capped` when the stage asked for
    /// is above everything mapped, so the polygon is the most the library shows, not the stage.
    Mapped { stage: MappedStage, capped: bool },
    /// The stage is below the lowest mapped one: the library shows no flooding there.
    Below { lowest_ft: f64 },
    /// The gauge has no inundation maps.
    Unmapped,
}

/// Choose the polygon for `want_ft` from a gauge's mapped stages (any order).
pub fn pick(stages: &[MappedStage], want_ft: f64) -> Pick {
    let Some(highest) = stages
        .iter()
        .max_by(|a, b| a.stage_ft.total_cmp(&b.stage_ft))
    else {
        return Pick::Unmapped;
    };
    match stages
        .iter()
        .filter(|s| s.stage_ft <= want_ft + SAME_FT)
        .max_by(|a, b| a.stage_ft.total_cmp(&b.stage_ft))
    {
        Some(s) => Pick::Mapped {
            stage: s.clone(),
            capped: want_ft > highest.stage_ft + SAME_FT,
        },
        None => Pick::Below {
            lowest_ft: stages
                .iter()
                .map(|s| s.stage_ft)
                .fold(f64::INFINITY, f64::min),
        },
    }
}

/// Read the stage listing [`fetch_library`] asks for: every layer's features, attributes only.
/// Sorted by stage, with a stage mapped in two layers (an interval that is also a threshold) kept
/// once, from the layer listed first.
pub fn parse_library(json: &str) -> anyhow::Result<Vec<MappedStage>> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    if let Some(e) = v.get("error") {
        anyhow::bail!(
            "inundation library error: {}",
            e["message"].as_str().unwrap_or("unknown")
        );
    }
    let mut out: Vec<MappedStage> = Vec::new();
    for layer in v["layers"].as_array().into_iter().flatten() {
        let Some(id) = layer["id"].as_u64() else {
            continue;
        };
        for f in layer["features"].as_array().into_iter().flatten() {
            let a = &f["attributes"];
            let Some(stage_ft) = a["interval_stage"].as_f64().or(a["stage"].as_f64()) else {
                continue;
            };
            if !stage_ft.is_finite() || out.iter().any(|s| (s.stage_ft - stage_ft).abs() < SAME_FT)
            {
                continue;
            }
            out.push(MappedStage {
                layer: id as u32,
                stage_ft,
                magnitude: a["magnitude"].as_str().unwrap_or("").to_ascii_lowercase(),
            });
        }
    }
    out.sort_by(|a, b| a.stage_ft.total_cmp(&b.stage_ft));
    Ok(out)
}

/// Read a polygon query's rings, as (longitude, latitude) points, from every feature returned.
/// The library's rings follow Esri's winding (outer rings clockwise, holes counterclockwise), so a
/// non-zero fill draws the holes.
pub fn parse_rings(json: &str) -> anyhow::Result<Vec<Vec<(f64, f64)>>> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    if let Some(e) = v.get("error") {
        anyhow::bail!(
            "inundation library error: {}",
            e["message"].as_str().unwrap_or("unknown")
        );
    }
    let mut rings = Vec::new();
    for f in v["features"].as_array().into_iter().flatten() {
        for ring in f["geometry"]["rings"].as_array().into_iter().flatten() {
            let pts: Vec<(f64, f64)> = ring
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| Some((p.get(0)?.as_f64()?, p.get(1)?.as_f64()?)))
                .filter(|(x, y)| x.is_finite() && y.is_finite())
                .collect();
            // Simplification leaves slivers collapsed to a point or two; they have no area.
            let distinct = pts.windows(2).filter(|w| w[0] != w[1]).count();
            if pts.len() >= 3 && distinct >= 2 {
                rings.push(pts);
            }
        }
    }
    Ok(rings)
}

/// A gauge id as the library keys it, refusing anything that would change the query it goes in.
fn library_lid(lid: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        !lid.is_empty() && lid.len() <= 8 && lid.chars().all(|c| c.is_ascii_alphanumeric()),
        "not a gauge id: {lid:?}"
    );
    Ok(lid.to_ascii_lowercase())
}

async fn get_text(client: &reqwest::Client, url: reqwest::Url) -> anyhow::Result<String> {
    let response = client
        .get(crate::net::fetch_url(url.as_str()))
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    anyhow::ensure!(
        status.is_success(),
        "inundation library answered {}",
        status.as_u16()
    );
    Ok(body)
}

/// Every mapped stage of a gauge, lowest first (empty when the gauge has no inundation maps).
pub async fn fetch_library(
    client: &reqwest::Client,
    lid: &str,
) -> anyhow::Result<Vec<MappedStage>> {
    let lid = library_lid(lid)?;
    let defs = LAYERS
        .iter()
        .map(|(l, _)| format!("\"{l}\":\"ahps_lid='{lid}'\""))
        .collect::<Vec<_>>()
        .join(",");
    let url = reqwest::Url::parse_with_params(
        &format!("{CATFIM_URL}/query"),
        &[
            ("layerDefs", format!("{{{defs}}}")),
            ("returnGeometry", "false".into()),
            ("f", "json".into()),
        ],
    )?;
    parse_library(&get_text(client, url).await?)
}

/// The inundation polygon for one mapped stage, as rings of (longitude, latitude).
pub async fn fetch_inundation(
    client: &reqwest::Client,
    lid: &str,
    stage: &MappedStage,
) -> anyhow::Result<Vec<Vec<(f64, f64)>>> {
    let lid = library_lid(lid)?;
    let field = stage.stage_field();
    let (lo, hi) = (stage.stage_ft - SAME_FT, stage.stage_ft + SAME_FT);
    let url = reqwest::Url::parse_with_params(
        &format!("{CATFIM_URL}/{}/query", stage.layer),
        &[
            (
                "where",
                format!("ahps_lid='{lid}' AND {field} > {lo} AND {field} < {hi}"),
            ),
            ("outFields", "oid".into()),
            ("returnGeometry", "true".into()),
            ("outSR", "4326".into()),
            // About ten meters: the polygons are drawn at city scale, and unsimplified a major
            // stage can run to megabytes.
            ("maxAllowableOffset", "0.0001".into()),
            ("geometryPrecision", "5".into()),
            ("f", "json".into()),
        ],
    )?;
    parse_rings(&get_text(client, url).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(layer: u32, stage_ft: f64, m: &str) -> MappedStage {
        MappedStage {
            layer,
            stage_ft,
            magnitude: m.into(),
        }
    }

    #[test]
    fn the_library_listing_is_sorted_and_deduplicated() {
        // Trimmed from the live listing for MDVG1 (Ogeechee River at Midville, GA).
        let json = r#"{"layers":[
            {"id":2,"features":[]},
            {"id":3,"features":[{"attributes":{"magnitude":"action","stage":5}}]},
            {"id":5,"features":[{"attributes":{"magnitude":"minor","interval_stage":8}},
                                {"attributes":{"magnitude":"minor","interval_stage":7}}]},
            {"id":6,"features":[{"attributes":{"magnitude":"minor","stage":6}}]},
            {"id":11,"features":[{"attributes":{"magnitude":"major","interval_stage":14}}]},
            {"id":12,"features":[{"attributes":{"magnitude":"major","stage":14.000000000001}}]},
            {"id":13,"features":[{"attributes":{"magnitude":"record","stage":12.41}}]}
        ]}"#;
        let s = parse_library(json).unwrap();
        let stages: Vec<f64> = s.iter().map(|m| m.stage_ft).collect();
        assert_eq!(stages, vec![5.0, 6.0, 7.0, 8.0, 12.41, 14.0]);
        assert_eq!(
            s[5].layer, 11,
            "the interval layer is listed first and kept"
        );
        assert_eq!(s[4].magnitude, "record");
        assert_eq!(s[4].stage_field(), "stage");
        assert_eq!(s[3].stage_field(), "interval_stage");
        assert!(parse_library(r#"{"error":{"code":400,"message":"bad"}}"#).is_err());
        assert!(parse_library(r#"{"layers":[]}"#).unwrap().is_empty());
    }

    #[test]
    fn the_polygon_never_shows_more_water_than_the_stage() {
        let s = vec![
            st(3, 5.0, "action"),
            st(5, 7.0, "minor"),
            st(5, 8.0, "minor"),
            st(11, 18.0, "major"),
        ];
        let Pick::Mapped { stage, capped } = pick(&s, 7.9) else {
            panic!()
        };
        assert_eq!((stage.stage_ft, capped), (7.0, false));
        let Pick::Mapped { stage, .. } = pick(&s, 8.0) else {
            panic!()
        };
        assert_eq!(stage.stage_ft, 8.0, "an exact stage is its own polygon");
        let Pick::Mapped { stage, capped } = pick(&s, 21.3) else {
            panic!()
        };
        assert_eq!(
            (stage.stage_ft, capped),
            (18.0, true),
            "above the library: its most"
        );
        assert_eq!(pick(&s, 3.0), Pick::Below { lowest_ft: 5.0 });
        assert_eq!(pick(&[], 10.0), Pick::Unmapped);
    }

    #[test]
    fn rings_come_back_as_lon_lat() {
        let json = r#"{"features":[
            {"geometry":{"rings":[[[-84.03,32.30],[-84.02,32.30],[-84.02,32.31],[-84.03,32.30]],
                                  [[-84.0,32.0],[-84.0,32.1]],
                                  [[-84.0,32.0],[-84.0,32.0],[-84.0,32.0],[-84.0,32.0]]]}},
            {"geometry":{"rings":[[[-84.1,32.2],[-84.0,32.2],[-84.0,32.25],[-84.1,32.2]]]}}
        ]}"#;
        let rings = parse_rings(json).unwrap();
        assert_eq!(rings.len(), 2, "two-point and one-point rings have no area and are dropped");
        assert_eq!(rings[0][0], (-84.03, 32.30));
        assert!(library_lid("MDVG1").unwrap() == "mdvg1");
        assert!(library_lid("x' OR 1=1").is_err());
    }
    // Live network check: the library is public; one listing and one polygon.
    #[tokio::test]
    #[ignore = "network"]
    async fn fetches_a_real_inundation_map() {
        let client = reqwest::Client::new();
        let lib = fetch_library(&client, "MDVG1").await.unwrap();
        eprintln!(
            "{} mapped stages: {:?}",
            lib.len(),
            lib.iter().map(|s| s.stage_ft).collect::<Vec<_>>()
        );
        let Pick::Mapped { stage, capped } = pick(&lib, 15.5) else {
            panic!("MDVG1 is mapped through its major stages")
        };
        let rings = fetch_inundation(&client, "MDVG1", &stage).await.unwrap();
        let points: usize = rings.iter().map(Vec::len).sum();
        eprintln!(
            "{} ft (capped {capped}): {} rings, {points} points",
            stage.stage_ft,
            rings.len()
        );
        assert!(!rings.is_empty());
        assert!(rings[0]
            .iter()
            .all(|&(lon, lat)| (-83.0..-81.5).contains(&lon) && (32.0..33.5).contains(&lat)));
        assert!(fetch_library(&client, "ZZZZ9").await.unwrap().is_empty());
    }
}
