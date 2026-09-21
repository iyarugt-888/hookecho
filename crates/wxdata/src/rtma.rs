//! RTMA: NCEP's Real-Time Mesoscale Analysis (ROADMAP_NEW G1).
//!
//! An analysis is not a forecast: it is the model's best estimate of what the surface looks like
//! *now*, pulled toward every observation that arrived that hour. That makes it the reference the
//! forecasts get judged against, and the honest answer to "what is the temperature field doing
//! right now" between stations. It is hourly on a 2.5 km CONUS grid and published a little under an
//! hour after the time it describes.
//!
//! Each file is a bundle of fields with a standard `.idx` sidecar, so a field is two requests: read
//! the index, range-GET the one message. The decode and regrid are the HRRR path's own.
//!
//! This is the *real-time* analysis. URMA, the retrospective "unrestricted" analysis that waits for
//! late observations, is a separate archive this module does not read.

use crate::alerts::USER_AGENT;
use crate::mrms::MrmsField;
use chrono::{DateTime, Datelike, Timelike, Utc};

const BUCKET: &str = "https://noaa-rtma-pds.s3.amazonaws.com";
/// Target cell size. A shade coarser than the 2.5 km native spacing (about 0.0225°) so the scatter
/// fills every cell rather than leaving a grid of holes.
const RES_DEG: f64 = 0.03;
/// Minutes after the hour before that hour's analysis has typically posted.
const LATENCY_MIN: i64 = 45;

/// A surface field the analysis carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RtmaField {
    Temp2m,
    Dewpoint2m,
    /// 10 m wind *speed*, published directly (not a single component).
    Wind10m,
    Gust10m,
}

impl RtmaField {
    pub const ALL: [RtmaField; 4] = [
        RtmaField::Temp2m,
        RtmaField::Dewpoint2m,
        RtmaField::Wind10m,
        RtmaField::Gust10m,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RtmaField::Temp2m => "2 m temperature",
            RtmaField::Dewpoint2m => "2 m dewpoint",
            RtmaField::Wind10m => "10 m wind speed",
            RtmaField::Gust10m => "10 m wind gust",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            RtmaField::Temp2m => "t2m",
            RtmaField::Dewpoint2m => "td2m",
            RtmaField::Wind10m => "wind10m",
            RtmaField::Gust10m => "gust10m",
        }
    }

    pub fn from_slug(s: &str) -> Option<RtmaField> {
        Self::ALL.into_iter().find(|f| f.slug() == s)
    }

    /// The `.idx` `(var, level)` for this field.
    pub fn key(self) -> (&'static str, &'static str) {
        match self {
            RtmaField::Temp2m => ("TMP", "2 m above ground"),
            RtmaField::Dewpoint2m => ("DPT", "2 m above ground"),
            RtmaField::Wind10m => ("WIND", "10 m above ground"),
            RtmaField::Gust10m => ("GUST", "10 m above ground"),
        }
    }
}

/// One hour's analysis of one field.
pub struct RtmaAnalysis {
    pub field: MrmsField,
    /// The hour this analyzes (UTC) — its valid time. An analysis has no lead.
    pub hour: DateTime<Utc>,
}

/// The analysis hours worth offering, newest first, starting at the newest that has plausibly
/// posted. The newest may still be mid-upload, which a fetch reports as a missing file.
pub fn run_choices(now: DateTime<Utc>, count: usize) -> Vec<DateTime<Utc>> {
    let base = now - chrono::Duration::minutes(LATENCY_MIN);
    let floored = base
        .with_minute(0)
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(base);
    (0..count)
        .map(|i| floored - chrono::Duration::hours(i as i64))
        .collect()
}

fn base_url(hour: DateTime<Utc>) -> String {
    format!(
        "{BUCKET}/rtma2p5.{:04}{:02}{:02}/rtma2p5.t{:02}z.2dvaranl_ndfd.grb2_wexp",
        hour.year(),
        hour.month(),
        hour.day(),
        hour.hour()
    )
}

/// Fetch `field` for `hour`, or for the newest analysis that has posted when `hour` is `None`
/// (walking back a few hours, since the newest may not be up yet). Naming an hour gets that hour or
/// an honest error; it never substitutes another.
pub async fn fetch(
    http: &reqwest::Client,
    field: RtmaField,
    hour: Option<DateTime<Utc>>,
) -> anyhow::Result<RtmaAnalysis> {
    let candidates = match hour {
        Some(hour) => vec![hour],
        None => run_choices(Utc::now(), 6),
    };
    let mut last_err = None;
    for hour in candidates {
        // Twice per hour: a dropped connection on a reused socket is common and says nothing
        // about whether the hour is posted, and walking back would quietly serve a staler
        // analysis for what was only a blip.
        for _ in 0..2 {
            match fetch_hour(http, field, hour).await {
                Ok(field) => return Ok(RtmaAnalysis { field, hour }),
                Err(e) => last_err = Some(e),
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no RTMA analysis found")))
}

async fn fetch_hour(
    http: &reqwest::Client,
    field: RtmaField,
    hour: DateTime<Utc>,
) -> anyhow::Result<MrmsField> {
    let base = base_url(hour);
    let idx = http
        .get(crate::net::fetch_url(&format!("{base}.idx")))
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let (var, level) = field.key();
    let (start, end) = crate::hrrr::field_byte_range(&idx, var, level)
        .ok_or_else(|| anyhow::anyhow!("no {var}:{level} in the RTMA index"))?;
    let bytes = crate::gribcache::fetch_range(http, &base, (start, end), USER_AGENT).await?;
    // gribberish can panic on some packings; contain it (see mrms::fetch_latest).
    let mut grid =
        crate::task::guarded(|| crate::hrrr::decode_regrid_at(&bytes, RES_DEG, f64::NEG_INFINITY))
            .unwrap_or_else(|_| anyhow::bail!("RTMA grib decode panicked"))?;
    fill_scatter_holes(&mut grid, 2);
    anyhow::ensure!(
        grid.time == hour,
        "the RTMA message is valid {}, not the requested {hour}",
        grid.time
    );
    Ok(grid)
}

/// Close the one-cell gaps the scatter regrid leaves where the source grid is sparser than the
/// target lattice. A fixed distance spans more degrees of longitude the farther north you go, so in
/// Canada the native samples are wider apart than a target cell and whole columns between them stay
/// empty. Those are artifacts of resampling, not missing data, so a cell with at least five finite
/// neighbours (of eight) takes their mean. A real edge, or a hole too big to be a gap, has too few
/// neighbours and is left alone.
fn fill_scatter_holes(field: &mut MrmsField, passes: usize) {
    let (nx, ny) = (field.nx, field.ny);
    if nx < 3 || ny < 3 || field.values.len() != nx * ny {
        return;
    }
    for _ in 0..passes {
        let before = field.values.clone();
        for y in 1..ny - 1 {
            for x in 1..nx - 1 {
                if before[y * nx + x].is_finite() {
                    continue;
                }
                let (mut sum, mut count) = (0.0f32, 0u8);
                for (dy, dx) in [
                    (-1isize, -1isize),
                    (-1, 0),
                    (-1, 1),
                    (0, -1),
                    (0, 1),
                    (1, -1),
                    (1, 0),
                    (1, 1),
                ] {
                    let i = (y as isize + dy) as usize * nx + (x as isize + dx) as usize;
                    if before[i].is_finite() {
                        sum += before[i];
                        count += 1;
                    }
                }
                if count >= 5 {
                    field.values[y * nx + x] = sum / f32::from(count);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn grid(nx: usize, ny: usize, v: f32) -> MrmsField {
        MrmsField {
            values: vec![v; nx * ny],
            nx,
            ny,
            lon_west: -100.0,
            lon_east: -90.0,
            lat_north: 50.0,
            lat_south: 40.0,
            time: Utc.with_ymd_and_hms(2026, 9, 20, 6, 0, 0).unwrap(),
        }
    }

    #[test]
    fn resampling_gaps_are_closed_but_real_holes_and_edges_are_not() {
        // A gradient with a missing column, like the gaps a sparse source leaves.
        let mut g = grid(8, 8, 0.0);
        for y in 0..8 {
            for x in 0..8 {
                g.values[y * 8 + x] = x as f32;
            }
        }
        for y in 0..8 {
            g.values[y * 8 + 4] = f32::NAN;
        }
        // An isolated single missing cell too.
        g.values[2 * 8 + 1] = f32::NAN;
        fill_scatter_holes(&mut g, 2);
        // The column interior is bridged with a value between its neighbours (3 and 5).
        for y in 1..7 {
            let v = g.values[y * 8 + 4];
            assert!(v.is_finite() && (3.0..=5.0).contains(&v), "row {y}: {v}");
        }
        assert!(g.values[2 * 8 + 1].is_finite(), "isolated gap filled");
        // The top and bottom edge of the gap column have too few neighbours and stay missing:
        // an edge is not a gap.
        assert!(g.values[4].is_nan() && g.values[7 * 8 + 4].is_nan());

        // A large hole keeps its middle: nothing there is a plausible resampling artifact.
        let mut big = grid(12, 12, 10.0);
        for y in 3..9 {
            for x in 3..9 {
                big.values[y * 12 + x] = f32::NAN;
            }
        }
        fill_scatter_holes(&mut big, 2);
        assert!(
            big.values[6 * 12 + 6].is_nan(),
            "the centre of a real hole survives"
        );
        // A grid too small to have an interior is untouched, as is one whose shape is inconsistent.
        let mut tiny = grid(2, 2, f32::NAN);
        fill_scatter_holes(&mut tiny, 2);
        assert!(tiny.values.iter().all(|v| v.is_nan()));
    }

    #[test]
    fn analysis_hours_are_hourly_newest_first_and_start_behind_the_clock() {
        let now = Utc.with_ymd_and_hms(2026, 9, 20, 21, 30, 0).unwrap();
        let runs = run_choices(now, 4);
        // 21:30 minus the 45-minute posting latency is 20:45, so 20Z is the newest offered.
        assert_eq!(
            runs,
            [
                Utc.with_ymd_and_hms(2026, 9, 20, 20, 0, 0).unwrap(),
                Utc.with_ymd_and_hms(2026, 9, 20, 19, 0, 0).unwrap(),
                Utc.with_ymd_and_hms(2026, 9, 20, 18, 0, 0).unwrap(),
                Utc.with_ymd_and_hms(2026, 9, 20, 17, 0, 0).unwrap(),
            ]
        );
        // Early in the hour the newest is the hour before last.
        let early = Utc.with_ymd_and_hms(2026, 9, 20, 21, 10, 0).unwrap();
        assert_eq!(run_choices(early, 1)[0].hour(), 20);
    }

    #[test]
    fn the_file_name_follows_the_bucket_layout() {
        let hour = Utc.with_ymd_and_hms(2026, 9, 21, 1, 0, 0).unwrap();
        assert_eq!(
            base_url(hour),
            "https://noaa-rtma-pds.s3.amazonaws.com/rtma2p5.20260921/rtma2p5.t01z.2dvaranl_ndfd.grb2_wexp"
        );
    }

    #[test]
    fn every_field_has_a_unique_slug_and_key() {
        let mut slugs: Vec<_> = RtmaField::ALL.iter().map(|f| f.slug()).collect();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), RtmaField::ALL.len());
        let mut keys: Vec<_> = RtmaField::ALL.iter().map(|f| f.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), RtmaField::ALL.len());
        for f in RtmaField::ALL {
            assert_eq!(RtmaField::from_slug(f.slug()), Some(f));
        }
    }

    /// Live: every field decodes for the newest posted hour, on a plausible CONUS grid, with values
    /// in physical range, and a named hour returns exactly that hour.
    /// Network test: `--ignored rtma_fields_decode`.
    #[tokio::test]
    #[ignore = "network"]
    async fn rtma_fields_decode_for_the_newest_hour() {
        let http = reqwest::Client::new();
        for field in RtmaField::ALL {
            let a = fetch(&http, field, None)
                .await
                .unwrap_or_else(|e| panic!("{}: {e}", field.label()));
            let finite: Vec<f32> = a
                .field
                .values
                .iter()
                .copied()
                .filter(|v| v.is_finite())
                .collect();
            assert!(
                finite.len() > 50_000,
                "{}: only {} cells",
                field.label(),
                finite.len()
            );
            let (lo, hi) = finite
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(l, h), &v| {
                    (l.min(v), h.max(v))
                });
            println!(
                "{} {} · {}x{} · {} cells · {lo:.1}..{hi:.1}",
                field.label(),
                a.hour,
                a.field.nx,
                a.field.ny,
                finite.len()
            );
            match field {
                // Kelvin: no CONUS surface is outside roughly -50..60 °C.
                RtmaField::Temp2m | RtmaField::Dewpoint2m => {
                    assert!(lo > 200.0 && hi < 340.0, "{}: {lo}..{hi} K", field.label())
                }
                RtmaField::Wind10m | RtmaField::Gust10m => {
                    assert!(lo >= 0.0 && hi < 90.0, "{}: {lo}..{hi} m/s", field.label())
                }
            }
        }
        // A named hour is that hour, and a wildly old one is an error rather than a substitute.
        let newest = fetch(&http, RtmaField::Temp2m, None).await.unwrap().hour;
        let earlier = newest - chrono::Duration::hours(3);
        let named = fetch(&http, RtmaField::Temp2m, Some(earlier))
            .await
            .unwrap();
        assert_eq!(named.hour, earlier);
        let ancient = Utc.with_ymd_and_hms(2001, 1, 1, 0, 0, 0).unwrap();
        assert!(fetch(&http, RtmaField::Temp2m, Some(ancient))
            .await
            .is_err());
    }
}
