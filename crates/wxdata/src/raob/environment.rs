//! Recorded-height environmental queries. UWyo HGHT is geopotential height in metres
//! above mean sea level (https://weather.uwyo.edu/upperair/columns.html). It is never
//! reconstructed from pressure or shifted by a different site's terrain elevation.

use super::{fetch_table, field, stations_within, synoptic_before, RaobStation};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

/// One requested isotherm, using only recorded HGHT/TEMP brackets. Missing stays missing.
#[derive(Clone, Debug, PartialEq)]
pub struct Isotherm {
    pub temperature_c: f64,
    /// Lowest bracketed crossing in recorded geopotential metres MSL.
    pub height_m: Option<f64>,
    /// Highest cooling-through crossing, for existing hail melting/growth-level semantics.
    pub highest_cooling_height_m: Option<f64>,
    /// Distinct crossing heights, including exact-temperature recorded samples.
    pub crossings_m: Vec<f64>,
}

/// A time-selected observed environment, with immutable raw-table identity.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvironmentalLevels {
    pub station: RaobStation,
    /// Synoptic launch selected by the request/cache key, not a receipt timestamp. The table
    /// cache does not retain the HTML observation-time metadata; do not call this independently
    /// verified reported launch time.
    pub launch: DateTime<Utc>,
    pub table_sha256: String,
    /// 0, -10, -20, -30, -40 C, respectively. A short/cold profile can lack individual levels.
    pub isotherms: [Isotherm; 5],
}

impl EnvironmentalLevels {
    /// Parse one immutable table, retaining incomplete rows to break interpolation adjacency.
    pub fn from_table(station: RaobStation, launch: DateTime<Utc>, table: &str) -> Self {
        let profile: Vec<(Option<f64>, Option<f64>)> = table
            .lines()
            .filter_map(|line| {
                let p = field(line, 0)?;
                (p.is_finite() && (1.0..=1100.0).contains(&p)).then(|| {
                    (
                        field(line, 1).filter(|v| v.is_finite()),
                        field(line, 2).filter(|v| v.is_finite()),
                    )
                })
            })
            .collect();
        let isotherms = [0.0, -10.0, -20.0, -30.0, -40.0].map(|temperature_c| {
            let mut crossings_m = Vec::new();
            let mut highest_cooling_height_m: Option<f64> = None;
            for &(height, temp) in &profile {
                if temp == Some(temperature_c) {
                    if let Some(h) = height {
                        crossings_m.push(h);
                    }
                }
            }
            for pair in profile.windows(2) {
                let [(Some(h0), Some(t0)), (Some(h1), Some(t1))] = pair else {
                    continue;
                };
                if h1 <= h0 {
                    continue;
                }
                if *t0 > temperature_c && *t1 <= temperature_c {
                    let h = h0 + (temperature_c - t0) / (t1 - t0) * (h1 - h0);
                    highest_cooling_height_m =
                        Some(highest_cooling_height_m.map_or(h, |old| old.max(h)));
                }
                if (*t0 < temperature_c && *t1 > temperature_c)
                    || (*t0 > temperature_c && *t1 < temperature_c)
                {
                    let fraction = (temperature_c - t0) / (t1 - t0);
                    crossings_m.push(h0 + fraction * (h1 - h0));
                }
            }
            crossings_m.sort_by(f64::total_cmp);
            crossings_m.dedup();
            Isotherm {
                temperature_c,
                height_m: crossings_m.first().copied(),
                highest_cooling_height_m,
                crossings_m,
            }
        });
        Self {
            station,
            launch,
            table_sha256: format!("{:x}", Sha256::digest(table.as_bytes())),
            isotherms,
        }
    }

    /// Carries complete date, station, datum, interpolation and content identity into existing
    /// column cache keys, probes and scientific export provenance strings.
    pub fn describe(&self) -> String {
        format!("{} WMO {} sounding (launch selection {}), recorded HGHT geopotential m MSL; bracketed crossings, linear in HGHT; table SHA256 {}",
            self.station.name, self.station.id, self.launch.format("%Y-%m-%d %H:%MZ"), self.table_sha256)
    }
}

/// Same bounded historical source search as RAOB melting levels, but uses recorded heights.
/// A missing isotherm does not discard other valid ones. Never requests a launch after `when`.
pub async fn environment_levels(
    client: &reqwest::Client,
    lon: f64,
    lat: f64,
    when: DateTime<Utc>,
    cache_dir: Option<std::path::PathBuf>,
) -> anyhow::Result<EnvironmentalLevels> {
    let mut last_error = anyhow::anyhow!("no sounding site within 400 km");
    for (station, _) in stations_within(lon, lat, 400.0).into_iter().take(2) {
        for back_h in [0, 12] {
            let launch = synoptic_before(when - chrono::Duration::hours(back_h));
            match fetch_table(client, station, launch, cache_dir.clone()).await {
                Ok(table) => {
                    let levels = EnvironmentalLevels::from_table(*station, launch, &table);
                    if levels.isotherms.iter().any(|l| l.height_m.is_some()) {
                        return Ok(levels);
                    }
                    last_error = anyhow::anyhow!(
                        "{} at {} has no recorded-height isotherm brackets",
                        station.name,
                        launch
                    );
                }
                Err(error) => last_error = error,
            }
        }
    }
    Err(last_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[(Option<f64>, Option<f64>)]) -> String {
        rows.iter()
            .enumerate()
            .map(|(i, (h, t))| {
                let f = |v: Option<f64>| v.map_or("       ".into(), |v| format!("{v:7.1}"));
                format!("{}{}{}\n", f(Some(1000.0 - i as f64 * 50.0)), f(*h), f(*t))
            })
            .collect()
    }

    fn levels(rows: &[(Option<f64>, Option<f64>)]) -> EnvironmentalLevels {
        EnvironmentalLevels::from_table(
            super::super::STATIONS[0],
            DateTime::from_timestamp(0, 0).unwrap(),
            &table(rows),
        )
    }

    #[test]
    fn recorded_heights_bracket_both_temperature_gradients_and_preserve_holes() {
        let l = levels(&[
            (Some(1000.0), Some(-15.0)),
            (Some(2000.0), Some(-5.0)),
            (Some(4000.0), Some(-25.0)),
        ]);
        assert_eq!(l.isotherms[1].crossings_m, [1500.0, 2500.0]);
        assert_eq!(l.isotherms[1].height_m, Some(1500.0));
        assert_eq!(
            l.isotherms[0].height_m, None,
            "no extrapolated freezing level"
        );
        let gap = levels(&[
            (Some(1000.0), Some(-5.0)),
            (None, Some(-10.0)),
            (Some(4000.0), Some(-25.0)),
        ]);
        assert_eq!(
            gap.isotherms[1].height_m, None,
            "a missing height breaks adjacency"
        );
        let no_temp = levels(&[
            (Some(1000.0), Some(-5.0)),
            (Some(2000.0), None),
            (Some(4000.0), Some(-25.0)),
        ]);
        assert_eq!(no_temp.isotherms[1].height_m, None);
    }

    #[test]
    fn additional_levels_and_exact_samples_need_no_dewpoint_or_wind() {
        let l = levels(&[(Some(1611.0), Some(10.0)), (Some(6611.0), Some(-40.0))]);
        assert_eq!(l.isotherms[3].height_m, Some(5611.0));
        assert_eq!(l.isotherms[4].height_m, Some(6611.0));
        assert_eq!(l.isotherms[4].crossings_m.len(), 1);
        assert!(l.describe().contains("geopotential m MSL"));
        assert!(l.describe().contains("1970-01-01 00:00Z"));
        assert_eq!(l.table_sha256.len(), 64);
    }

    #[test]
    fn pinned_observed_profiles_match_independent_reference_and_raw_identity() {
        let records: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../docs/certification/m3.3/recorded-environment/reference.json"
        ))
        .unwrap();
        let tables = [
            include_str!(
                "../../../../docs/certification/m3.3/recorded-environment/72357-2013052012.txt"
            ),
            include_str!(
                "../../../../docs/certification/m3.3/recorded-environment/72469-2017050812.txt"
            ),
        ];
        for (record, table) in records.as_array().unwrap().iter().zip(tables) {
            let id = &record["file"].as_str().unwrap()[..5];
            let station = *super::super::STATIONS.iter().find(|s| s.id == id).unwrap();
            let launch = record["launch_selection_utc"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap();
            let got = EnvironmentalLevels::from_table(station, launch, table);
            assert_eq!(got.table_sha256, record["sha256"].as_str().unwrap());
            for (got, want) in got
                .isotherms
                .iter()
                .zip(record["isotherms"].as_array().unwrap())
            {
                assert_eq!(
                    got.crossings_m.len(),
                    want["crossings_m"].as_array().unwrap().len()
                );
                for (a, b) in got
                    .crossings_m
                    .iter()
                    .zip(want["crossings_m"].as_array().unwrap())
                {
                    assert!((a - b.as_f64().unwrap()).abs() < 1e-8);
                }
                assert!((got.height_m.unwrap() - want["lowest_m"].as_f64().unwrap()).abs() < 1e-8);
                assert!(
                    (got.highest_cooling_height_m.unwrap()
                        - want["highest_cooling_m"].as_f64().unwrap())
                    .abs()
                        < 1e-8
                );
            }
            let altered = EnvironmentalLevels::from_table(station, launch, &format!("{table}\n"));
            assert_eq!(got.isotherms, altered.isotherms);
            assert_ne!(
                got.table_sha256, altered.table_sha256,
                "identity includes raw source bytes"
            );
        }
    }

    #[tokio::test]
    async fn historical_environment_selection_reads_its_requested_cache_not_a_live_profile() {
        let station = super::super::STATIONS
            .iter()
            .find(|s| s.id == "72469")
            .unwrap();
        let when = "2017-05-08T21:00:00Z".parse().unwrap();
        let cache = std::env::temp_dir().join(format!("hookecho-raob-env-{}", std::process::id()));
        std::fs::create_dir_all(cache.join("raob")).unwrap();
        std::fs::write(
            cache.join("raob/72469-2017050812.txt"),
            include_str!(
                "../../../../docs/certification/m3.3/recorded-environment/72469-2017050812.txt"
            ),
        )
        .unwrap();
        // No successful network response is possible. The selected historical table must suffice.
        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
            .build()
            .unwrap();
        let got = environment_levels(&client, station.lon, station.lat, when, Some(cache.clone()))
            .await
            .unwrap();
        assert_eq!(got.station.id, "72469");
        assert_eq!(got.launch.to_rfc3339(), "2017-05-08T12:00:00+00:00");
        assert_eq!(got.isotherms[1].height_m, Some(5411.0));
        std::fs::remove_file(cache.join("raob/72469-2017050812.txt")).unwrap();
        std::fs::remove_dir(cache.join("raob")).unwrap();
        std::fs::remove_dir(cache).unwrap();
    }
}
