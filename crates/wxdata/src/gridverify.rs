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

    // Weighted sums for the moments.
    let (mut w_sum, mut sum_err, mut sum_abs, mut sum_sq) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut sum_f, mut sum_a, mut sum_ff, mut sum_aa, mut sum_fa) =
        (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut cells = 0usize;
    let mut table = threshold.map(|_| Contingency {
        hits: 0.0,
        misses: 0.0,
        false_alarms: 0.0,
        correct_negatives: 0.0,
    });

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
            if let Some((west, south, east, north)) = region {
                if lon < west || lon > east || lat < south || lat > north {
                    continue;
                }
            }
            let Some(a) = analysis.sample_bilinear(lon, lat).filter(|a| a.is_finite()) else {
                continue;
            };
            let (f, a) = (f64::from(f), f64::from(a));
            let err = f - a;
            cells += 1;
            w_sum += w;
            sum_err += w * err;
            sum_abs += w * err.abs();
            sum_sq += w * err * err;
            sum_f += w * f;
            sum_a += w * a;
            sum_ff += w * f * f;
            sum_aa += w * a * a;
            sum_fa += w * f * a;
            if let (Some(t), Some(c)) = (threshold, table.as_mut()) {
                let t = f64::from(t);
                match (f > t, a > t) {
                    (true, true) => c.hits += w,
                    (false, true) => c.misses += w,
                    (true, false) => c.false_alarms += w,
                    (false, false) => c.correct_negatives += w,
                }
            }
        }
    }
    anyhow::ensure!(
        cells > 0 && w_sum > 0.0,
        "the forecast and the analysis share no cells in that area"
    );

    let mean_f = sum_f / w_sum;
    let mean_a = sum_a / w_sum;
    let var_f = sum_ff / w_sum - mean_f * mean_f;
    let var_a = sum_aa / w_sum - mean_a * mean_a;
    let cov = sum_fa / w_sum - mean_f * mean_a;
    // Variances this small are rounding noise, not a pattern to correlate.
    let correlation =
        (var_f > 1e-9 && var_a > 1e-9).then(|| (cov / (var_f * var_a).sqrt()).clamp(-1.0, 1.0));
    Ok(GridScore {
        scores: Scores {
            cells,
            bias: sum_err / w_sum,
            mae: sum_abs / w_sum,
            rmse: (sum_sq / w_sum).sqrt(),
            correlation,
        },
        contingency: table,
    })
}

/// Which surface field to verify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VerifyField {
    Temp2m,
    Dewpoint2m,
}

impl VerifyField {
    pub const ALL: [VerifyField; 2] = [VerifyField::Temp2m, VerifyField::Dewpoint2m];

    pub fn label(self) -> &'static str {
        match self {
            VerifyField::Temp2m => "2 m temperature",
            VerifyField::Dewpoint2m => "2 m dewpoint",
        }
    }

    fn rtma(self) -> crate::rtma::RtmaField {
        match self {
            VerifyField::Temp2m => crate::rtma::RtmaField::Temp2m,
            VerifyField::Dewpoint2m => crate::rtma::RtmaField::Dewpoint2m,
        }
    }

    fn model_field(self) -> crate::model::ModelField {
        match self {
            VerifyField::Temp2m => crate::model::ModelField::Temperature2m,
            VerifyField::Dewpoint2m => crate::model::ModelField::Dewpoint2m,
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

/// Score one model run against the RTMA at each of `leads` (whole forecast hours).
///
/// A lead whose valid time has not yet got an analysis (it is in the future, or too recent to have
/// posted) is reported as such rather than fetched to fail, and one lead failing never stops the
/// rest. Thresholds are in the field's native units (Kelvin).
pub async fn verify_run(
    http: &reqwest::Client,
    model: crate::hrrr::Model,
    field: VerifyField,
    run: DateTime<Utc>,
    leads: &[u8],
    region: Option<Region>,
    threshold: Option<f32>,
) -> anyhow::Result<Vec<LeadResult>> {
    let key = field
        .model_field()
        .grib(model)
        .ok_or_else(|| anyhow::anyhow!("{} does not publish {}", model.label(), field.label()))?;
    let now = Utc::now();
    let mut out = Vec::with_capacity(leads.len());
    for &lead in leads {
        let valid = run + Duration::hours(i64::from(lead));
        let score = if valid > now - Duration::minutes(ANALYSIS_LATENCY_MIN) {
            Err("no analysis yet: this lead is not far enough in the past".to_string())
        } else {
            score_lead(http, model, field, key, run, lead, valid, region, threshold)
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
) -> anyhow::Result<GridScore> {
    let (forecast, analysis) = futures_util::future::try_join(
        crate::hrrr::fetch_field_at_run(http, model, run, key.var, key.level, lead, key.min_valid),
        crate::rtma::fetch(http, field.rtma(), Some(valid)),
    )
    .await?;
    compare(&forecast.field, &analysis.field, region, threshold)
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
}
