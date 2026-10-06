//! The near-storm environment of every backtest candidate (detectionplan.md, "Environment"), from
//! the HRRR archive, so it can be tested against the verified detections without rerunning the
//! radar:
//!
//!     cargo run --release -p wxdata --example env_sample -- [--dry-run] OUT.csv DIR/candidates.csv ...
//!
//! Reads the `tornado_fusion`, `tornado_marker` and `tornado_id` rows of each export and, for each
//! hour they fall in, fetches the [`wxdata::near_storm::HRRR_SPECS`] fields as the app's
//! environment gate does ([`wxdata::near_storm::fetch_hour`]: the run an hour before at F+1),
//! then writes one line per distinct candidate point: `minute,lon,lat` exactly as the export wrote
//! them, then the [`wxdata::near_storm::EnvSample`]. A run missing from the archive is stood in for
//! by the on-hour analysis, then the run two hours before (`run` says which was used). An hour whose
//! fields cannot be had at all (before the archive begins on 2014-07-30, or no source) is written
//! as `#missing`. Lines are appended an hour at a time, and a rerun skips the hours already sampled
//! (and those before the archive), so a stopped run resumes and a failed hour is tried again.
//! `--dry-run` only counts hours and points.
//!
//! The GRIB itself goes through the HRRR cache, which is capped (`objcache::GRIB_HRRR`): a long
//! run leaves only OUT behind.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use chrono::DateTime;
use futures_util::StreamExt;
use wxdata::near_storm::ARCHIVE_START;

/// Hours fetched at once; each is eight field requests.
const HOURS_AT_ONCE: usize = 3;
const DETECTORS: [&str; 3] = ["tornado_fusion", "tornado_marker", "tornado_id"];
/// What `near_storm::fetch_hour` says of an hour before the archive.
const BEFORE_ARCHIVE: &str = "before the HRRR archive";

/// One candidate point: the export's own text for its minute, longitude and latitude.
type Point = (i64, String, String);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let dry = args.first().is_some_and(|a| a == "--dry-run");
    if dry {
        args.remove(0);
    }
    anyhow::ensure!(
        args.len() >= 2,
        "usage: env_sample [--dry-run] OUT.csv DIR/candidates.csv ..."
    );
    let out = args.remove(0);

    // Every distinct point, by the hour it falls in.
    let mut by_hour: BTreeMap<i64, BTreeSet<Point>> = BTreeMap::new();
    for path in &args {
        let text = std::fs::read_to_string(path)?;
        let mut lines = text.lines();
        let header: Vec<&str> = lines.next().unwrap_or_default().split(',').collect();
        let col = |n: &str| {
            header
                .iter()
                .position(|h| *h == n)
                .ok_or_else(|| anyhow::anyhow!("{path}: no {n} column"))
        };
        let (det, minute, lon, lat) = (col("detector")?, col("minute")?, col("lon")?, col("lat")?);
        for line in lines {
            let f: Vec<&str> = line.split(',').collect();
            if f.len() <= det.max(minute).max(lon).max(lat) || !DETECTORS.contains(&f[det]) {
                continue;
            }
            let Ok(m) = f[minute].parse::<i64>() else {
                continue;
            };
            by_hour.entry(m.div_euclid(60) * 60).or_default().insert((
                m,
                f[lon].to_string(),
                f[lat].to_string(),
            ));
        }
    }

    // Hours already in OUT (a resumed run): sampled ones, and those before the archive. Other
    // missing hours are tried again.
    let mut done = BTreeSet::new();
    if let Ok(text) = std::fs::read_to_string(&out) {
        for line in text.lines() {
            let mut f = line.split(',');
            let first = f.next().unwrap_or_default();
            let valid = if first == "#missing" {
                let v = f.next();
                v.filter(|_| line.contains(BEFORE_ARCHIVE))
            } else {
                f.nth(2)
            };
            if let Some(v) = valid.and_then(|v| v.parse::<i64>().ok()) {
                done.insert(v);
            }
        }
    }
    let todo: Vec<(i64, BTreeSet<Point>)> = by_hour
        .into_iter()
        .filter(|(h, _)| !done.contains(h))
        .collect();
    let points: usize = todo.iter().map(|(_, p)| p.len()).sum();
    let archived = todo
        .iter()
        .filter(|(h, _)| *h * 60 >= ARCHIVE_START)
        .count();
    eprintln!(
        "{} hours to sample ({archived} in the archive, {} already done), {points} points",
        todo.len(),
        done.len()
    );
    if dry {
        return Ok(());
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&out)?;
    if done.is_empty() {
        writeln!(
            file,
            "minute,lon,lat,valid,run,sbcape,mlcape,mlcin,srh1,srh3,shear6,lcl_m,stp,stp_point,cells"
        )?;
    }
    let http = reqwest::Client::new();
    let total = todo.len();
    let mut results = futures_util::stream::iter(todo.into_iter().map(|(hour, pts)| {
        let http = http.clone();
        // Spawned, so the regrids of different hours use different cores.
        tokio::spawn(async move { (hour, sample_hour(&http, hour, &pts).await) })
    }))
    .buffer_unordered(HOURS_AT_ONCE);
    let (mut n, mut missing) = (0usize, 0usize);
    while let Some(r) = results.next().await {
        let (hour, lines) = r?;
        n += 1;
        match lines {
            Ok(lines) => {
                for l in lines {
                    writeln!(file, "{l}")?;
                }
            }
            Err(e) => {
                missing += 1;
                let why = format!("{e:#}").replace([',', '\n'], ";");
                writeln!(file, "#missing,{hour},{why}")?;
            }
        }
        file.flush()?;
        if n % 25 == 0 || n == total {
            eprintln!("{n} of {total} hours, {missing} missing");
        }
    }
    Ok(())
}

/// The sampled lines of one hour (valid at `hour`, epoch minutes).
async fn sample_hour(
    http: &reqwest::Client,
    hour: i64,
    pts: &BTreeSet<Point>,
) -> anyhow::Result<Vec<String>> {
    let valid =
        DateTime::from_timestamp(hour * 60, 0).ok_or_else(|| anyhow::anyhow!("bad hour {hour}"))?;
    let (run, env) = wxdata::near_storm::fetch_hour(http, valid).await?;
    let run_min = run.timestamp() / 60;
    Ok(pts
        .iter()
        .map(|(m, lon, lat)| {
            let s = lon
                .parse()
                .ok()
                .zip(lat.parse().ok())
                .and_then(|(x, y)| env.sample(x, y));
            match s {
                Some(s) => format!(
                    "{m},{lon},{lat},{hour},{run_min},{:.0},{:.0},{:.0},{:.0},{:.0},{:.1},{:.0},{:.3},{:.3},{}",
                    s.sbcape, s.mlcape, s.mlcin, s.srh1, s.srh3, s.shear6, s.lcl_m, s.stp, s.stp_point, s.cells
                ),
                None => format!("{m},{lon},{lat},{hour},{run_min},,,,,,,,,,0"),
            }
        })
        .collect())
}
