//! Grid verification: score a forecast field against an analysis of the same valid time
//! (ROADMAP_NEW K1).
//!
//! The question is "how far off was the model?", answered by comparing its forecast to what the
//! RTMA says the surface actually did at that hour. The scores are the standard ones:
//!
//! * **bias**: the average signed error, forecast minus analysis. Positive means the model ran
//!   high (too warm, too windy).
//! * **MAE** and **RMSE**: the average size of the error. RMSE punishes large misses harder.
//! * **correlation**: whether the pattern is right even when the level is off.
//! * for an event threshold, a **contingency table** with hits, misses, false alarms and the usual
//!   ratios (POD, FAR, CSI, frequency bias).
//!
//! Two things keep the numbers honest. Grid cells on a latitude/longitude lattice cover less ground
//! toward the poles, so every cell is weighted by the cosine of its latitude rather than counted
//! equally, and a forecast is only ever compared with an analysis of *exactly* the same valid time.
//! The analysis is itself an estimate, so these measure agreement with the RTMA, not with every
//! station.

use crate::mrms::MrmsField;
use chrono::{DateTime, Duration, Utc};

/// A rectangle in degrees, `(west, south, east, north)`, to score inside.
pub type Region = (f64, f64, f64, f64);

/// Error statistics for forecast minus analysis, in the field's own units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scores {
    /// Grid cells compared (unweighted count).
    pub cells: usize,
    /// Mean of `forecast - analysis`.
    pub bias: f64,
    /// Mean of `|forecast - analysis|`.
    pub mae: f64,
    pub rmse: f64,
    /// Pearson correlation between forecast and analysis, `None` when either has no variation.
    pub correlation: Option<f64>,
}

/// Forecast/observed event counts for one threshold, area-weighted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contingency {
    pub hits: f64,
    pub misses: f64,
    pub false_alarms: f64,
    pub correct_negatives: f64,
}

impl Contingency {
    /// Probability of detection: of the events that happened, the share forecast.
    pub fn pod(&self) -> Option<f64> {
        ratio(self.hits, self.hits + self.misses)
    }

    /// False alarm ratio: of the events forecast, the share that did not happen.
    pub fn far(&self) -> Option<f64> {
        ratio(self.false_alarms, self.hits + self.false_alarms)
    }

    /// Critical success index: hits over everything that was forecast or observed.
    pub fn csi(&self) -> Option<f64> {
        ratio(self.hits, self.hits + self.misses + self.false_alarms)
    }

    /// Frequency bias: how often the event was forecast relative to how often it happened. Above 1
    /// the model over-forecasts the event.
    pub fn frequency_bias(&self) -> Option<f64> {
        ratio(self.hits + self.false_alarms, self.hits + self.misses)
    }
}

fn ratio(n: f64, d: f64) -> Option<f64> {
    (d > 0.0).then(|| n / d)
}

/// Everything one comparison produces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridScore {
    pub scores: Scores,
    /// Present when a threshold was given.
    pub contingency: Option<Contingency>,
}

/// Compare `forecast` with `analysis` over the forecast's cells, sampling the analysis at each
/// cell centre, optionally inside `region`, and optionally scoring an event `value > threshold`.
///
/// Fails when the two are not valid at the same instant, or when nothing overlaps.
pub fn compare(
    forecast: &MrmsField,
    analysis: &MrmsField,
    region: Option<Region>,
    threshold: Option<f32>,
) -> anyhow::Result<GridScore> {
    anyhow::ensure!(
        forecast.time == analysis.time,
        "the forecast is valid {} but the analysis is valid {}; they cannot be compared",
        forecast.time,
        analysis.time
    );
    anyhow::ensure!(
        forecast.nx >= 2 && forecast.ny >= 2 && forecast.values.len() == forecast.nx * forecast.ny,
        "the forecast grid has no cells"
    );
    // Grid values are cell *centres*: `nx` cells span west to east, so a cell is a full step wide
    // and its value sits half a step in. This is the convention `MrmsField::sample_bilinear` reads
    // the analysis with, so forecast and analysis land on the same ground.
    let dx = (forecast.lon_east - forecast.lon_west) / forecast.nx as f64;
    let dy = (forecast.lat_north - forecast.lat_south) / forecast.ny as f64;

    let mut acc = Accum::new(threshold);
    for row in 0..forecast.ny {
        let lat = forecast.lat_north - (row as f64 + 0.5) * dy;
        // Cells shrink toward the poles: a degree of longitude is cos(lat) as wide.
        let w = lat.to_radians().cos().max(0.0);
        if w <= 0.0 {
            continue;
        }
        for col in 0..forecast.nx {
            let f = forecast.values[row * forecast.nx + col];
            if !f.is_finite() {
                continue;
            }
            let lon = forecast.lon_west + (col as f64 + 0.5) * dx;
            if !inside(region, lon, lat) {
                continue;
            }
            let Some(a) = analysis.sample_bilinear(lon, lat).filter(|a| a.is_finite()) else {
                continue;
            };
            acc.add(f64::from(f), f64::from(a), w);
        }
    }
    acc.finish("the forecast and the analysis share no cells in that area")
}

fn inside(region: Option<Region>, lon: f64, lat: f64) -> bool {
    region.is_none_or(|(west, south, east, north)| {
        (west..=east).contains(&lon) && (south..=north).contains(&lat)
    })
}

/// One station's observed value, in the forecast's units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub lon: f64,
    pub lat: f64,
    pub value: f32,
}

/// Compare `forecast` with station observations valid at its time: the forecast sampled
/// (bilinearly) at each station inside `region`, every station counted once. `Scores::cells` is
/// then the number of stations.
///
/// The forecast is a grid-box value at the model's terrain height and the station is a point at
/// its own: in rough terrain some of the difference is that, not forecast error.
pub fn compare_points(
    forecast: &MrmsField,
    points: &[Point],
    region: Option<Region>,
    threshold: Option<f32>,
) -> anyhow::Result<GridScore> {
    let mut acc = Accum::new(threshold);
    for p in points.iter().filter(|p| p.value.is_finite()) {
        if !inside(region, p.lon, p.lat) {
            continue;
        }
        let Some(f) = forecast
            .sample_bilinear(p.lon, p.lat)
            .filter(|f| f.is_finite())
        else {
            continue;
        };
        acc.add(f64::from(f), f64::from(p.value), 1.0);
    }
    acc.finish("no reporting station inside the forecast in that area")
}

/// Weighted sums for the scores.
struct Accum {
    cells: usize,
    w_sum: f64,
    sum_err: f64,
    sum_abs: f64,
    sum_sq: f64,
    sum_f: f64,
    sum_a: f64,
    sum_ff: f64,
    sum_aa: f64,
    sum_fa: f64,
    threshold: Option<f32>,
    table: Option<Contingency>,
}

impl Accum {
    fn new(threshold: Option<f32>) -> Self {
        Self {
            cells: 0,
            w_sum: 0.0,
            sum_err: 0.0,
            sum_abs: 0.0,
            sum_sq: 0.0,
            sum_f: 0.0,
            sum_a: 0.0,
            sum_ff: 0.0,
            sum_aa: 0.0,
            sum_fa: 0.0,
            threshold,
            table: threshold.map(|_| Contingency {
                hits: 0.0,
                misses: 0.0,
                false_alarms: 0.0,
                correct_negatives: 0.0,
            }),
        }
    }

    fn add(&mut self, f: f64, a: f64, w: f64) {
        let err = f - a;
        self.cells += 1;
        self.w_sum += w;
        self.sum_err += w * err;
        self.sum_abs += w * err.abs();
        self.sum_sq += w * err * err;
        self.sum_f += w * f;
        self.sum_a += w * a;
        self.sum_ff += w * f * f;
        self.sum_aa += w * a * a;
        self.sum_fa += w * f * a;
        if let (Some(t), Some(c)) = (self.threshold, self.table.as_mut()) {
            let t = f64::from(t);
            match (f > t, a > t) {
                (true, true) => c.hits += w,
                (false, true) => c.misses += w,
                (true, false) => c.false_alarms += w,
                (false, false) => c.correct_negatives += w,
            }
        }
    }

    fn finish(self, empty: &str) -> anyhow::Result<GridScore> {
        anyhow::ensure!(self.cells > 0 && self.w_sum > 0.0, "{empty}");
        let w = self.w_sum;
        let mean_f = self.sum_f / w;
        let mean_a = self.sum_a / w;
        let var_f = self.sum_ff / w - mean_f * mean_f;
        let var_a = self.sum_aa / w - mean_a * mean_a;
        let cov = self.sum_fa / w - mean_f * mean_a;
        // Variances this small are rounding noise, not a pattern to correlate.
        let correlation =
            (var_f > 1e-9 && var_a > 1e-9).then(|| (cov / (var_f * var_a).sqrt()).clamp(-1.0, 1.0));
        Ok(GridScore {
            scores: Scores {
                cells: self.cells,
                bias: self.sum_err / w,
                mae: self.sum_abs / w,
                rmse: (self.sum_sq / w).sqrt(),
                correlation,
            },
            contingency: self.table,
        })
    }
}

/// What a forecast is scored against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Truth {
    /// The RTMA analysis grid, area-weighted.
    #[default]
    Rtma,
    /// METAR station reports nearest the valid time, each station once.
    Metar,
    /// The MRMS reflectivity mosaic nearest the valid time, area-weighted.
    Mrms,
}

impl Truth {
    pub const ALL: [Truth; 3] = [Truth::Rtma, Truth::Metar, Truth::Mrms];

    pub fn label(self) -> &'static str {
        match self {
            Truth::Rtma => "RTMA analysis",
            Truth::Metar => "METAR stations",
            Truth::Mrms => "MRMS radar mosaic",
        }
    }
}

/// Which surface field to verify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VerifyField {
    Temp2m,
    Dewpoint2m,
    /// Composite reflectivity, scored against the MRMS mosaic with both floored at
    /// [`NO_ECHO_DBZ`].
    CompositeReflectivity,
}

/// What "no echo" scores as, dBZ: MRMS's in-coverage no-echo cells and anything a model forecasts
/// below it. Without a floor the clear-air values a model writes (down to −20 or so) would count
/// as error against cells the radar mosaic leaves empty.
pub const NO_ECHO_DBZ: f32 = 0.0;

impl VerifyField {
    pub const ALL: [VerifyField; 3] = [
        VerifyField::Temp2m,
        VerifyField::Dewpoint2m,
        VerifyField::CompositeReflectivity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VerifyField::Temp2m => "2 m temperature",
            VerifyField::Dewpoint2m => "2 m dewpoint",
            VerifyField::CompositeReflectivity => "Composite reflectivity",
        }
    }

    /// What it can be scored against: the first is the default.
    pub fn truths(self) -> &'static [Truth] {
        match self {
            VerifyField::Temp2m | VerifyField::Dewpoint2m => &[Truth::Rtma, Truth::Metar],
            VerifyField::CompositeReflectivity => &[Truth::Mrms],
        }
    }

    /// Whether values are temperatures in Kelvin (else dBZ).
    pub fn is_temperature(self) -> bool {
        !matches!(self, VerifyField::CompositeReflectivity)
    }

    fn rtma(self) -> Option<crate::rtma::RtmaField> {
        match self {
            VerifyField::Temp2m => Some(crate::rtma::RtmaField::Temp2m),
            VerifyField::Dewpoint2m => Some(crate::rtma::RtmaField::Dewpoint2m),
            VerifyField::CompositeReflectivity => None,
        }
    }

    pub fn model_field(self) -> crate::model::ModelField {
        match self {
            VerifyField::Temp2m => crate::model::ModelField::Temperature2m,
            VerifyField::Dewpoint2m => crate::model::ModelField::Dewpoint2m,
            VerifyField::CompositeReflectivity => crate::model::ModelField::CompositeReflectivity,
        }
    }
}

/// One lead's result, or why it could not be scored.
#[derive(Debug, Clone)]
pub struct LeadResult {
    pub lead_h: u8,
    pub valid: DateTime<Utc>,
    pub score: Result<GridScore, String>,
}

/// How long after its hour an RTMA analysis is typically posted, in minutes, plus a margin.
const ANALYSIS_LATENCY_MIN: i64 = 60;

/// Reports up to [`crate::metar::NEAR_VALID_MIN`] after the hour count, and reach the feed within
/// minutes.
const METAR_LATENCY_MIN: i64 = 30;

/// The mosaic posts every two minutes, a few minutes behind.
const MRMS_LATENCY_MIN: i64 = 10;

/// How far from the valid time an MRMS mosaic may be and still stand for it.
const MRMS_TOLERANCE_MIN: i64 = 5;

/// Score one model run against the RTMA at each of `leads` (whole forecast hours).
///
/// A lead whose valid time has not yet got an analysis (it is in the future, or too recent to have
/// posted) is reported as such rather than fetched to fail, and one lead failing never stops the
/// rest. Thresholds are in the field's native units (Kelvin).
#[allow(clippy::too_many_arguments)]
pub async fn verify_run(
    http: &reqwest::Client,
    model: crate::hrrr::Model,
    field: VerifyField,
    run: DateTime<Utc>,
    leads: &[u8],
    region: Option<Region>,
    threshold: Option<f32>,
    truth: Truth,
) -> anyhow::Result<Vec<LeadResult>> {
    let key = field
        .model_field()
        .grib(model)
        .ok_or_else(|| anyhow::anyhow!("{} does not publish {}", model.label(), field.label()))?;
    anyhow::ensure!(
        field.truths().contains(&truth),
        "{} is not scored against the {}",
        field.label(),
        truth.label()
    );
    let now = Utc::now();
    let mut out = Vec::with_capacity(leads.len());
    for &lead in leads {
        let valid = run + Duration::hours(i64::from(lead));
        let latency = match truth {
            Truth::Rtma => ANALYSIS_LATENCY_MIN,
            Truth::Metar => METAR_LATENCY_MIN,
            Truth::Mrms => MRMS_LATENCY_MIN,
        };
        let score = if valid > now - Duration::minutes(latency) {
            Err(match truth {
                Truth::Rtma => "no analysis yet: this lead is not far enough in the past",
                Truth::Metar => "no reports yet: this lead is not far enough in the past",
                Truth::Mrms => "no mosaic yet: this lead is not far enough in the past",
            }
            .to_string())
        } else {
            score_lead(
                http, model, field, key, run, lead, valid, region, threshold, truth,
            )
            .await
            .map_err(|e| e.to_string())
        };
        out.push(LeadResult {
            lead_h: lead,
            valid,
            score,
        });
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
async fn score_lead(
    http: &reqwest::Client,
    model: crate::hrrr::Model,
    field: VerifyField,
    key: crate::model::GribKey,
    run: DateTime<Utc>,
    lead: u8,
    valid: DateTime<Utc>,
    region: Option<Region>,
    threshold: Option<f32>,
    truth: Truth,
) -> anyhow::Result<GridScore> {
    let forecast =
        crate::hrrr::fetch_field_at_run(http, model, run, key.var, key.level, lead, key.min_valid);
    match truth {
        Truth::Rtma => {
            let rtma = field
                .rtma()
                .ok_or_else(|| anyhow::anyhow!("the RTMA has no {}", field.label()))?;
            let (forecast, analysis) = futures_util::future::try_join(
                forecast,
                crate::rtma::fetch(http, rtma, Some(valid)),
            )
            .await?;
            compare(&forecast.field, &analysis.field, region, threshold)
        }
        Truth::Mrms => {
            let (forecast, mosaic) = futures_util::future::try_join(
                forecast,
                crate::mrms::fetch_nearest_for_scoring(
                    http,
                    crate::mrms::REFLECTIVITY,
                    valid,
                    Duration::minutes(MRMS_TOLERANCE_MIN),
                    NO_ECHO_DBZ,
                ),
            )
            .await?;
            let mut forecast = forecast.field;
            floor_echo(&mut forecast.values);
            // The nearest mosaic stands for the valid time (checked within the tolerance).
            let mut mosaic = mosaic.data;
            mosaic.time = forecast.time;
            compare(&forecast, &mosaic, region, threshold)
        }
        Truth::Metar => {
            let forecast = forecast.await?.field;
            anyhow::ensure!(
                forecast.time == valid,
                "the forecast is valid {}, not {valid}",
                forecast.time
            );
            let area = region.unwrap_or((
                forecast.lon_west,
                forecast.lat_south,
                forecast.lon_east,
                forecast.lat_north,
            ));
            let obs = crate::metar::fetch_near(http, area, valid).await?;
            compare_points(&forecast, &station_points(&obs, field), region, threshold)
        }
    }
}

/// Forecast reflectivity below [`NO_ECHO_DBZ`] is no echo, as the mosaic reads it.
pub fn floor_echo(values: &mut [f32]) {
    for v in values.iter_mut().filter(|v| v.is_finite()) {
        *v = v.max(NO_ECHO_DBZ);
    }
}

/// The stations' reports of `field`, in Kelvin like the forecast.
pub fn station_points(obs: &[crate::metar::SurfaceOb], field: VerifyField) -> Vec<Point> {
    obs.iter()
        .filter_map(|o| {
            let c = match field {
                VerifyField::Temp2m => o.temp_c,
                VerifyField::Dewpoint2m => o.dewp_c,
                VerifyField::CompositeReflectivity => None,
            }?;
            Some(Point {
                lon: o.lon,
                lat: o.lat,
                value: c + 273.15,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 20, 12, 0, 0).unwrap()
    }

    /// A small lat/lon grid of cell centres, values from `f(lon, lat)`.
    fn grid(
        nx: usize,
        ny: usize,
        lat_north: f64,
        lat_south: f64,
        f: impl Fn(f64, f64) -> f32,
    ) -> MrmsField {
        let (west, east) = (-100.0, -90.0);
        let mut values = Vec::with_capacity(nx * ny);
        for row in 0..ny {
            let lat = lat_north - (lat_north - lat_south) * (row as f64 + 0.5) / ny as f64;
            for col in 0..nx {
                let lon = west + (east - west) * (col as f64 + 0.5) / nx as f64;
                values.push(f(lon, lat));
            }
        }
        MrmsField {
            values,
            nx,
            ny,
            lon_west: west,
            lon_east: east,
            lat_north,
            lat_south,
            time: t(),
        }
    }

    #[test]
    fn stations_are_scored_once_each_at_their_own_point() {
        // The forecast is warmer by 2 K everywhere: a linear field sampled exactly at stations.
        let fc = grid(20, 20, 40.0, 30.0, |lon, lat| {
            (280.0 + lon + 100.0 + lat - 30.0) as f32
        });
        let truth = |lon: f64, lat: f64| (278.0 + lon + 100.0 + lat - 30.0) as f32;
        let points: Vec<Point> = [(-99.0, 31.0), (-95.0, 35.0), (-91.0, 39.0), (-80.0, 35.0)]
            .iter()
            .map(|&(lon, lat)| Point {
                lon,
                lat,
                value: truth(lon, lat),
            })
            .collect();
        let s = compare_points(&fc, &points, None, Some(281.0)).unwrap();
        // The station east of the grid is not counted.
        assert_eq!(s.scores.cells, 3);
        assert!((s.scores.bias - 2.0).abs() < 1e-4 && (s.scores.mae - 2.0).abs() < 1e-4);
        let c = s.contingency.unwrap();
        // Truths 280, 288, 296 against forecasts 282, 290, 298 over 281 K: one false alarm.
        assert_eq!(
            (c.hits, c.false_alarms, c.misses, c.correct_negatives),
            (2.0, 1.0, 0.0, 0.0)
        );
        let west = compare_points(&fc, &points, Some((-100.0, 30.0, -97.0, 40.0)), None).unwrap();
        assert_eq!(west.scores.cells, 1);
        assert!(compare_points(&fc, &points[3..], None, None).is_err());
    }

    #[test]
    fn a_station_reports_the_field_asked_for_in_kelvin() {
        let obs = crate::metar::parse(
            r#"[{"icaoId":"KOKC","lat":35.4,"lon":-97.6,"temp":20.0,"dewp":10.0},
                {"icaoId":"KTIK","lat":35.4,"lon":-97.4,"temp":21.0}]"#,
        );
        let t = station_points(&obs, VerifyField::Temp2m);
        assert_eq!(t.len(), 2);
        assert!((t[0].value - 293.15).abs() < 1e-3);
        let d = station_points(&obs, VerifyField::Dewpoint2m);
        assert_eq!(
            d.len(),
            1,
            "a station without a dew point is not scored for it"
        );
        assert!((d[0].value - 283.15).abs() < 1e-3);
    }

    #[test]
    fn a_constant_error_is_all_bias_and_no_spread_of_error() {
        let truth = grid(21, 21, 40.0, 30.0, |lon, lat| {
            280.0 + (lon as f32) * 0.1 + (lat as f32) * 0.2
        });
        let fcst = grid(21, 21, 40.0, 30.0, |lon, lat| {
            282.0 + (lon as f32) * 0.1 + (lat as f32) * 0.2
        });
        let s = compare(&fcst, &truth, None, None).unwrap().scores;
        assert!((s.bias - 2.0).abs() < 1e-3, "{s:?}");
        assert!((s.mae - 2.0).abs() < 1e-3);
        assert!((s.rmse - 2.0).abs() < 1e-3);
        // The pattern is exactly right even though the level is off.
        assert!(s.correlation.unwrap() > 0.999);
        assert_eq!(s.cells, 21 * 21);
    }

    #[test]
    fn errors_of_both_signs_cancel_in_bias_but_not_in_mae_or_rmse() {
        let truth = grid(21, 21, 40.0, 30.0, |_, _| 280.0);
        // Half the domain 3 too warm, half 3 too cold.
        let fcst = grid(
            21,
            21,
            40.0,
            30.0,
            |lon, _| if lon < -95.0 { 283.0 } else { 277.0 },
        );
        let s = compare(&fcst, &truth, None, None).unwrap().scores;
        assert!(
            s.bias.abs() < 0.3,
            "the halves should roughly cancel: {}",
            s.bias
        );
        assert!((s.mae - 3.0).abs() < 1e-3);
        assert!((s.rmse - 3.0).abs() < 1e-3);
    }

    #[test]
    fn cells_are_weighted_by_the_ground_they_cover_not_counted_equally() {
        // Wrong by 10 only in the northernmost rows, right elsewhere. Those rows are narrow on the
        // ground, so the area-weighted error must be smaller than the plain count share.
        let truth = grid(11, 41, 80.0, 0.0, |_, _| 280.0);
        let fcst = grid(
            11,
            41,
            80.0,
            0.0,
            |_, lat| if lat > 70.0 { 290.0 } else { 280.0 },
        );
        let s = compare(&fcst, &truth, None, None).unwrap().scores;
        let wrong_rows = (0..41).filter(|r| 80.0 - 2.0 * *r as f64 > 70.0).count() as f64;
        let plain_share = wrong_rows / 41.0 * 10.0;
        assert!(
            s.bias > 0.0 && s.bias < plain_share * 0.5,
            "weighted {} vs plain {plain_share}",
            s.bias
        );
    }

    #[test]
    fn a_region_restricts_what_is_scored() {
        let truth = grid(21, 21, 40.0, 30.0, |_, _| 280.0);
        let fcst = grid(
            21,
            21,
            40.0,
            30.0,
            |lon, _| if lon < -95.0 { 290.0 } else { 280.0 },
        );
        let west = compare(&fcst, &truth, Some((-100.0, 30.0, -96.0, 40.0)), None)
            .unwrap()
            .scores;
        let east = compare(&fcst, &truth, Some((-94.0, 30.0, -90.0, 40.0)), None)
            .unwrap()
            .scores;
        assert!((west.bias - 10.0).abs() < 1e-3);
        assert!(east.bias.abs() < 1e-3);
        assert!(west.cells < 21 * 21 && east.cells < 21 * 21);
        // An area with no overlap is an error, not a division by zero.
        assert!(compare(&fcst, &truth, Some((10.0, 10.0, 20.0, 20.0)), None).is_err());
    }

    #[test]
    fn different_valid_times_cannot_be_compared() {
        let a = grid(5, 5, 40.0, 30.0, |_, _| 1.0);
        let mut b = grid(5, 5, 40.0, 30.0, |_, _| 1.0);
        b.time += Duration::hours(1);
        assert!(compare(&a, &b, None, None).is_err());
    }

    #[test]
    fn missing_cells_are_skipped_not_counted_as_zero() {
        let truth = grid(11, 11, 40.0, 30.0, |_, _| 280.0);
        let mut fcst = grid(11, 11, 40.0, 30.0, |_, _| 281.0);
        for v in fcst.values.iter_mut().take(30) {
            *v = f32::NAN;
        }
        let s = compare(&fcst, &truth, None, None).unwrap().scores;
        assert!((s.bias - 1.0).abs() < 1e-3);
        assert_eq!(s.cells, 121 - 30);
    }

    #[test]
    fn the_contingency_table_counts_hits_misses_and_false_alarms() {
        // Freezing threshold 273.15: analysis is cold on the west half, forecast cold on the east
        // half plus a strip, so there are all four outcomes.
        let truth = grid(
            21,
            21,
            40.0,
            30.0,
            |lon, _| if lon < -95.0 { 270.0 } else { 285.0 },
        );
        let fcst = grid(
            21,
            21,
            40.0,
            30.0,
            |lon, _| if lon < -97.0 { 270.0 } else { 285.0 },
        );
        let c = compare(&fcst, &truth, None, Some(280.0))
            .unwrap()
            .contingency
            .unwrap();
        // "Event" is warm (> 280): forecast warm from -97 east, analysis warm from -95 east.
        assert!(c.hits > 0.0 && c.false_alarms > 0.0 && c.correct_negatives > 0.0);
        assert_eq!(
            c.misses, 0.0,
            "every observed-warm cell was also forecast warm"
        );
        assert!(
            c.frequency_bias().unwrap() > 1.0,
            "the event is over-forecast"
        );
        assert!(c.pod().unwrap() > 0.999);
        assert!(c.far().unwrap() > 0.0);
        let csi = c.csi().unwrap();
        assert!(csi > 0.0 && csi < 1.0);
        // No threshold, no table.
        assert!(compare(&fcst, &truth, None, None)
            .unwrap()
            .contingency
            .is_none());
    }

    #[test]
    fn ratios_are_absent_rather_than_infinite_when_nothing_happened() {
        let c = Contingency {
            hits: 0.0,
            misses: 0.0,
            false_alarms: 0.0,
            correct_negatives: 5.0,
        };
        assert_eq!(c.pod(), None);
        assert_eq!(c.far(), None);
        assert_eq!(c.csi(), None);
        assert_eq!(c.frequency_bias(), None);
    }

    /// Live: HRRR against the RTMA for the same valid hours. The forecast should be close (a few
    /// degrees, not tens), better at short leads than a persistence of nothing, and correlate
    /// strongly, since the diurnal and terrain pattern dominate.
    /// Network test: `--ignored live_hrrr_temperature`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_hrrr_temperature_scores_sensibly_against_the_rtma() {
        let http = reqwest::Client::new();
        let now = Utc::now();
        // A run old enough that its short leads all have analyses.
        let run = crate::hrrr::run_choices(crate::hrrr::Model::Hrrr, now, 12)
            .into_iter()
            .nth(8)
            .unwrap();
        let results = verify_run(
            &http,
            crate::hrrr::Model::Hrrr,
            VerifyField::Temp2m,
            run,
            &[1, 3, 6],
            None,
            Some(273.15),
            Truth::Rtma,
        )
        .await
        .expect("verification");
        assert_eq!(results.len(), 3);
        for r in &results {
            let g = r
                .score
                .as_ref()
                .unwrap_or_else(|e| panic!("F+{}: {e}", r.lead_h));
            let s = g.scores;
            println!(
                "F+{:02} valid {} · {} cells · bias {:+.2} K · MAE {:.2} · RMSE {:.2} · r {:.3}",
                r.lead_h,
                r.valid,
                s.cells,
                s.bias,
                s.mae,
                s.rmse,
                s.correlation.unwrap_or(f64::NAN)
            );
            assert!(s.cells > 100_000);
            assert!(
                s.mae < 6.0 && s.rmse < 8.0,
                "an hourly model should not be this far off"
            );
            assert!(s.correlation.unwrap() > 0.9, "the pattern should match");
            assert!(g.contingency.is_some());
        }
    }

    /// Live: the HRRR against METARs over the southern Plains, the same hours as above.
    /// Network test: `--ignored live_hrrr_temperature_against_stations`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_hrrr_temperature_against_stations() {
        let http = reqwest::Client::new();
        let run = crate::hrrr::run_choices(crate::hrrr::Model::Hrrr, Utc::now(), 12)
            .into_iter()
            .nth(8)
            .unwrap();
        let results = verify_run(
            &http,
            crate::hrrr::Model::Hrrr,
            VerifyField::Temp2m,
            run,
            &[1, 6],
            Some((-104.0, 33.0, -94.0, 40.0)),
            None,
            Truth::Metar,
        )
        .await
        .expect("verification");
        for r in &results {
            let s = r
                .score
                .as_ref()
                .unwrap_or_else(|e| panic!("F+{}: {e}", r.lead_h))
                .scores;
            println!(
                "F+{:02} valid {} · {} stations · bias {:+.2} K · MAE {:.2} · RMSE {:.2} · r {:.3}",
                r.lead_h,
                r.valid,
                s.cells,
                s.bias,
                s.mae,
                s.rmse,
                s.correlation.unwrap_or(f64::NAN)
            );
            // How many report within a quarter hour of the hour varies with the feed.
            assert!(s.cells > 50, "{}", s.cells);
            assert!(
                s.mae < 5.0,
                "an hourly model is not this far off its stations"
            );
        }
    }

    #[test]
    fn reflectivity_is_scored_only_against_the_mosaic_with_a_no_echo_floor() {
        assert_eq!(VerifyField::CompositeReflectivity.truths(), [Truth::Mrms]);
        assert!(!VerifyField::Temp2m.truths().contains(&Truth::Mrms));
        let mut v = vec![-15.0, 5.0, f32::NAN, 42.0];
        floor_echo(&mut v);
        assert_eq!(v[..2], [0.0, 5.0]);
        assert!(v[2].is_nan(), "a gap stays a gap");
        // A forecast echo over a no-echo cell is a false alarm, not skipped.
        let fc = grid(
            4,
            4,
            40.0,
            30.0,
            |lon, _| if lon < -95.0 { 40.0 } else { 0.0 },
        );
        let obs = grid(4, 4, 40.0, 30.0, |_, _| NO_ECHO_DBZ);
        let c = compare(&fc, &obs, None, Some(35.0))
            .unwrap()
            .contingency
            .unwrap();
        assert!(c.false_alarms > 0.0 && c.hits == 0.0);
    }

    /// Live: the HRRR's composite reflectivity against the MRMS mosaic.
    /// Network test: `--ignored live_hrrr_reflectivity_against_mrms`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_hrrr_reflectivity_against_mrms() {
        let http = reqwest::Client::new();
        let run = crate::hrrr::run_choices(crate::hrrr::Model::Hrrr, Utc::now(), 12)
            .into_iter()
            .nth(4)
            .unwrap();
        // The archive has gaps (whole missing hours); a lead whose hour is missing says so, and
        // at least one of three should land on frames.
        let results = verify_run(
            &http,
            crate::hrrr::Model::Hrrr,
            VerifyField::CompositeReflectivity,
            run,
            &[1, 2, 3],
            None,
            Some(35.0),
            Truth::Mrms,
        )
        .await
        .expect("verification");
        let mut scored = 0;
        for r in &results {
            let g = match &r.score {
                Ok(g) => g,
                Err(e) => {
                    println!("F+{:02}: {e}", r.lead_h);
                    continue;
                }
            };
            scored += 1;
            let (s, c) = (g.scores, g.contingency.unwrap());
            println!(
                "F+{:02} valid {} · {} cells · bias {:+.2} dBZ · MAE {:.2} · POD {:.2} FAR {:.2} CSI {:.2} freq bias {:.2}",
                r.lead_h,
                r.valid,
                s.cells,
                s.bias,
                s.mae,
                c.pod().unwrap_or(f64::NAN),
                c.far().unwrap_or(f64::NAN),
                c.csi().unwrap_or(f64::NAN),
                c.frequency_bias().unwrap_or(f64::NAN)
            );
            assert!(s.cells > 500_000, "no-echo cells are scored: {}", s.cells);
            assert!(s.mae < 15.0);
        }
        assert!(scored > 0, "no lead landed on a mosaic");
    }
}
