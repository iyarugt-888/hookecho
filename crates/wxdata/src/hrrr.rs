//! HRRR "future radar": composite reflectivity (REFC) forecast grids from the NOAA HRRR AWS PDS.
//!
//! Fetches only the REFC message of a `wrfsfcf{HH}` file via an `.idx` byte-range request
//! (~0.4 MB instead of the ~130 MB full file), decodes it with `gribberish` (Lambert-conformal
//! grid), then scatter-regrids the native grid onto a regular lat/lon grid so it can reuse the
//! MRMS field-layer render pipeline (a plate-carrée→mercator warp).

use crate::alerts::USER_AGENT;
use crate::mrms::MrmsField;
use chrono::{DateTime, Datelike, Timelike, Utc};
use futures_util::StreamExt;

const BUCKET: &str = "https://noaa-hrrr-bdp-pds.s3.amazonaws.com";
const RAP_BUCKET: &str = "https://noaa-rap-pds.s3.amazonaws.com";
const NAM_BUCKET: &str = "https://noaa-nam-pds.s3.amazonaws.com";
/// The National Blend of Models, in GRIB2 with `.idx` sidecars. Not `noaa-nbm-pds`: that bucket
/// republished as per-element GeoTIFF, which would need a TIFF decoder to read one field.
const NBM_BUCKET: &str = "https://noaa-nbm-grib2-pds.s3.amazonaws.com";

/// Which model to pull a field from.
///
/// HRRR is the forecast model this module was written for. RAP is here for its **f00 analysis**:
/// the same fields, assimilated from observations rather than projected forward, which is what
/// people mean when they ask for "mesoanalysis" (SPC's own surface objective analysis is RAP plus
/// surface obs). It costs one URL and one grid spacing — everything downstream is identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Model {
    #[default]
    Hrrr,
    Rap,
    /// HRRR's pressure-level file. Not offered as a user-facing source — it exists for the
    /// effective-layer parameters, which need real columns.
    HrrrPressure,
    /// NAM 3 km CONUS nest. A second convection-allowing opinion at HRRR's resolution, on its own
    /// dynamical core and its own 6-hourly cycle — which is the point of having it.
    NamNest,
    /// The NAM's own parent 12 km CONUS grid (AWIPS grid 212) — coarser than the nest, but the
    /// nest is itself downscaled from this, so it is a third genuinely independent opinion (its
    /// own dynamical core, its own cycle) rather than the same nest at a different crop.
    Nam,
    /// National Blend of Models, CONUS domain. Statistically post-processed guidance rather than
    /// a raw model: no updraft helicity, but the calibrated probabilities nobody else publishes.
    Nbm,
}

impl Model {
    /// The GRIB2 file for a cycle + forecast hour.
    fn url(self, date: &str, cycle_hour: u32, fh: u8) -> String {
        match self {
            Model::Hrrr => {
                format!("{BUCKET}/hrrr.{date}/conus/hrrr.t{cycle_hour:02}z.wrfsfcf{fh:02}.grib2")
            }
            // The pressure-level file: full mandatory levels with dewpoint and with U and V as
            // separate messages, which the surface file and RAP's awp130 both lack.
            Model::HrrrPressure => {
                format!("{BUCKET}/hrrr.{date}/conus/hrrr.t{cycle_hour:02}z.wrfprsf{fh:02}.grib2")
            }
            // awp130 is the 13 km CONUS pressure/surface product — the one with CAPE and helicity.
            Model::Rap => {
                format!("{RAP_BUCKET}/rap.{date}/rap.t{cycle_hour:02}z.awp130pgrbf{fh:02}.grib2")
            }
            Model::NamNest => format!(
                "{NAM_BUCKET}/nam.{date}/nam.t{cycle_hour:02}z.conusnest.hiresf{fh:02}.tm00.grib2"
            ),
            // `awip12` is AWIPS grid 212, the NAM's own parent 12 km CONUS grid.
            Model::Nam => {
                format!("{NAM_BUCKET}/nam.{date}/nam.t{cycle_hour:02}z.awip12{fh:02}.tm00.grib2")
            }
            // `co` is the CONUS domain; the forecast hour is three digits here, not two.
            Model::Nbm => format!(
                "{NBM_BUCKET}/blend.{date}/{cycle_hour:02}/core/blend.t{cycle_hour:02}z.core.f{fh:03}.co.grib2"
            ),
        }
    }

    /// Hours between cycles. Walking back an hour at a time past a model that runs every six only
    /// ever finds 404s.
    fn cycle_hours(self) -> u32 {
        match self {
            Model::NamNest | Model::Nam => 6,
            _ => 1,
        }
    }

    /// Regular-grid cell size (degrees) for the regrid: a shade coarser than the model's native
    /// spacing, so the scatter fills every target cell instead of leaving a grid of holes.
    /// HRRR is ~3 km, RAP ~13 km.
    fn res_deg(self) -> f64 {
        match self {
            Model::Hrrr | Model::HrrrPressure => 0.04,
            Model::Rap => 0.15,
            // The NAM nest is 3 km like the HRRR; the NBM CONUS grid is 2.5 km.
            Model::NamNest => 0.04,
            // AWIPS grid 212 is 12 km native.
            Model::Nam => 0.13,
            Model::Nbm => 0.035,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Model::Hrrr => "HRRR",
            Model::HrrrPressure => "HRRR pressure",
            Model::Rap => "RAP",
            Model::NamNest => "NAM 3 km nest",
            Model::Nam => "NAM 12 km",
            Model::Nbm => "NBM",
        }
    }
}

/// A decoded HRRR forecast field plus its run/valid times.
pub struct HrrrForecast {
    pub field: MrmsField,
    /// Model cycle (run) initialization time (UTC).
    pub run: DateTime<Utc>,
    /// Forecast hour past the run.
    pub fcst_hour: u8,
    /// Forecast lead in minutes, set only for the sub-hourly (`wrfsubhf`) product where the lead
    /// is not a whole hour. `None` means the lead is exactly `fcst_hour` hours.
    pub fcst_minutes: Option<u16>,
}

impl HrrrForecast {
    /// Valid time = run + forecast lead.
    pub fn valid(&self) -> DateTime<Utc> {
        match self.fcst_minutes {
            Some(m) => self.run + chrono::Duration::minutes(i64::from(m)),
            None => self.run + chrono::Duration::hours(i64::from(self.fcst_hour)),
        }
    }
}

/// Fetch the REFC forecast for `fcst_hour` (0..=18) from the most recent available HRRR run.
/// Tries recent cycles (allowing for the ~1–2 h data latency), newest first.
pub async fn fetch_forecast(http: &reqwest::Client, fcst_hour: u8) -> anyhow::Result<HrrrForecast> {
    fetch_field(
        http,
        Model::Hrrr,
        "REFC",
        "entire atmosphere",
        fcst_hour,
        -30.0,
    )
    .await
}

/// The HRRR sub-hourly (`wrfsubhf`) product: composite reflectivity at 15-minute steps.
///
/// `minutes` is the total forecast lead (15..=1080), snapped to the 15-minute grid. Each
/// `wrfsubhf{FF}` file holds the four steps ending at hour `FF` — `wrfsubhf01` carries the 15/30/
/// 45/60-minute forecasts — so the file index is `ceil(minutes / 60)` and the `.idx` message is
/// the one whose forecast designator (field 5) is exactly `"{minutes} min fcst"`.
///
/// The regrid and colour mapping are identical to [`fetch_forecast`]; only the file and the
/// index lookup differ, which is why this shares [`decode_regrid`] and [`recent_cycles`].
pub async fn fetch_forecast_subhourly(
    http: &reqwest::Client,
    minutes: u16,
) -> anyhow::Result<HrrrForecast> {
    let minutes = (minutes.clamp(15, 18 * 60) / 15) * 15;
    let ff = subhourly_file_index(minutes);
    let fcst = format!("{minutes} min fcst");
    let now = Utc::now();
    let mut last_err = None;
    for run in recent_cycles(Model::Hrrr, now) {
        match fetch_subhourly_run(http, run, ff, &fcst, minutes).await {
            Ok(fc) => return Ok(fc),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no HRRR sub-hourly run found")))
}

/// `wrfsubhf{ff}` for one cycle.
fn subhourly_url(date: &str, cycle_hour: u32, ff: u8) -> String {
    format!("{BUCKET}/hrrr.{date}/conus/hrrr.t{cycle_hour:02}z.wrfsubhf{ff:02}.grib2")
}

/// Which `wrfsubhf{FF}` file carries the `minutes`-lead step. Each file holds the four steps
/// ending at hour FF — `wrfsubhf01` → 15/30/45/60 min — so FF = ceil(minutes / 60), clamped to
/// the model's 18-hour range.
fn subhourly_file_index(minutes: u16) -> u8 {
    ((u32::from(minutes.max(1)) + 59) / 60).clamp(1, 18) as u8
}

async fn fetch_subhourly_run(
    http: &reqwest::Client,
    run: DateTime<Utc>,
    ff: u8,
    fcst: &str,
    minutes: u16,
) -> anyhow::Result<HrrrForecast> {
    let date = format!("{:04}{:02}{:02}", run.year(), run.month(), run.day());
    let base = subhourly_url(&date, run.hour(), ff);

    let idx = http
        .get(crate::net::fetch_url(&format!("{base}.idx")))
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let (start, end) = field_byte_range_fcst(&idx, "REFC", "entire atmosphere", fcst)
        .ok_or_else(|| anyhow::anyhow!("no REFC:{fcst} in sub-hourly idx"))?;

    let range = match end {
        Some(e) => format!("bytes={start}-{}", e - 1),
        None => format!("bytes={start}-"),
    };
    let bytes = http
        .get(crate::net::fetch_url(&base))
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .header("Range", range)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    let field = crate::task::guarded(|| decode_regrid(&bytes, Model::Hrrr, -30.0))
        .unwrap_or_else(|_| anyhow::bail!("HRRR sub-hourly grib decode panicked"))?;
    Ok(HrrrForecast {
        field,
        run,
        fcst_hour: (minutes / 60) as u8,
        fcst_minutes: Some(minutes),
    })
}

/// Fetch any single HRRR surface field for `fcst_hour` by variable + level idx strings, regridding
/// with `min_valid` as the drop threshold (REFC uses −30 dBZ; CAPE 0; SRH −∞ so negatives survive).
/// Walks back up to 6 cycles until a run has this forecast hour posted.
pub async fn fetch_field(
    http: &reqwest::Client,
    model: Model,
    var: &str,
    level: &str,
    fcst_hour: u8,
    min_valid: f64,
) -> anyhow::Result<HrrrForecast> {
    let now = Utc::now();
    let mut last_err = None;
    for run in recent_cycles(model, now) {
        // The lead cap is a property of the run, not a constant: this was `.min(18)` for every
        // model and every cycle, which silently truncated the NAM's 60 h nest and three quarters
        // of HRRR's 00/06/12/18Z runs. `crate::model` holds the real schedule.
        let fh = clamp_lead(model, run, fcst_hour);
        match fetch_run_field(http, model, run, fh, var, level, min_valid).await {
            Ok(field) => {
                return Ok(HrrrForecast {
                    field,
                    run,
                    fcst_hour: fh,
                    fcst_minutes: None,
                })
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no HRRR run found")))
}

/// Fetch the newest available cycle whose forecast lead lands exactly on `target_valid`.
/// This lets HRRR/RAP comparisons share a valid time even when one model's latest cycle is
/// still posting. A missing aligned field is an error, never an unmatched fallback.
pub async fn fetch_field_aligned(
    http: &reqwest::Client,
    model: Model,
    var: &str,
    level: &str,
    target_valid: DateTime<Utc>,
    min_valid: f64,
) -> anyhow::Result<HrrrForecast> {
    let mut last_err = None;
    for (run, fh) in aligned_run_hours(model, target_valid, Utc::now()) {
        match fetch_run_field(http, model, run, fh, var, level, min_valid).await {
            Ok(field) => {
                return Ok(HrrrForecast {
                    field,
                    run,
                    fcst_hour: fh,
                    fcst_minutes: None,
                })
            }
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        anyhow::anyhow!("no {} cycle aligns with {target_valid}", model.label())
    }))
}

/// Fetch `var`/`level` from a `model` cycle strictly earlier than `current_run`, at the same
/// `target_valid` — the "previous run" side of a run-to-run comparison (ROADMAP_NEW F5). Walks
/// back further if the immediately-previous cycle hasn't posted this field yet, the same
/// multi-candidate fallback [`fetch_field_aligned`] uses, just seeded one cycle further back so it
/// can never return `current_run` itself.
pub async fn fetch_field_previous_run(
    http: &reqwest::Client,
    model: Model,
    var: &str,
    level: &str,
    current_run: DateTime<Utc>,
    target_valid: DateTime<Utc>,
    min_valid: f64,
) -> anyhow::Result<HrrrForecast> {
    let mut last_err = None;
    for (run, fh) in aligned_run_hours(model, target_valid, current_run) {
        match fetch_run_field(http, model, run, fh, var, level, min_valid).await {
            Ok(field) => {
                return Ok(HrrrForecast {
                    field,
                    run,
                    fcst_hour: fh,
                    fcst_minutes: None,
                })
            }
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        anyhow::anyhow!(
            "no {} cycle before {current_run} aligns with {target_valid}",
            model.label()
        )
    }))
}

/// Newest cycles first, with only whole-hour leads in the published 0..=18 h range.
fn aligned_run_hours(
    model: Model,
    target_valid: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Vec<(DateTime<Utc>, u8)> {
    recent_cycles(model, now)
        .into_iter()
        .filter_map(|run| {
            let lead = target_valid - run;
            let fh = u8::try_from(lead.num_hours()).ok()?;
            (fh <= 18 && lead == chrono::Duration::hours(i64::from(fh))).then_some((run, fh))
        })
        .collect()
}

/// The six most recent cycles of `model` that could plausibly be posted, newest first.
///
/// Stepping back an hour at a time is right for the hourly models and useless for the NAM nest,
/// which runs every six: the run hour is floored onto the model's own cycle lattice first.
pub(crate) fn recent_cycles(model: Model, now: DateTime<Utc>) -> Vec<DateTime<Utc>> {
    let step = model.cycle_hours();
    // One step back before the first candidate: a cycle is not on the wire the moment it is named.
    let base = now - chrono::Duration::hours(step as i64);
    let floored = base
        .with_hour(base.hour() / step * step)
        .unwrap_or(base)
        .with_minute(0)
        .unwrap()
        .with_second(0)
        .unwrap()
        .with_nanosecond(0)
        .unwrap();
    (0..6)
        .map(|i| floored - chrono::Duration::hours((i * step) as i64))
        .collect()
}

/// How many HRRR field fetches run at once. NOMADS is a shared public service; six is brisk
/// without being rude, and past that the regrid work dominates anyway.
const HRRR_CONCURRENCY: usize = 6;

/// Fetch several surface fields from ONE model cycle: walks back up to 6 runs and only returns
/// when every `(var, level, min_valid)` spec resolves against the same run. Composite parameters
/// (STP/SCP/EHI) must not mix ingredients from different cycles, and per-field [`fetch_field`]
/// calls can land on different ones when a newer run is mid-upload.
pub async fn fetch_fields_one_run(
    http: &reqwest::Client,
    model: Model,
    fcst_hour: u8,
    specs: &[(&str, &str, f64)],
) -> anyhow::Result<(DateTime<Utc>, Vec<MrmsField>)> {
    fetch_fields_one_run_capped(http, model, fcst_hour, specs, None).await
}

/// [`fetch_fields_one_run`] with an optional grid cap: each field is subsampled to at most
/// `max_dim` cells on its longest side *as it arrives*, so a deep stack of levels never has to be
/// resident at full resolution. A hundred-odd full HRRR grids is most of a gigabyte; the same
/// stack at 500 cells wide is tens of megabytes, and the parameters that need a deep stack are
/// smooth enough to be worth far less resolution than they cost.
pub async fn fetch_fields_one_run_capped(
    http: &reqwest::Client,
    model: Model,
    fcst_hour: u8,
    specs: &[(&str, &str, f64)],
    max_dim: Option<usize>,
) -> anyhow::Result<(DateTime<Utc>, Vec<MrmsField>)> {
    // Owned up front: the concurrent stream below must not borrow `specs` across an await, or
    // the whole future stops being `Send` and the app can't spawn it.
    let owned_specs: Vec<(String, String, f64)> = specs
        .iter()
        .map(|(v, l, m)| (v.to_string(), l.to_string(), *m))
        .collect();
    let now = Utc::now();
    let mut last_err = None;
    for run in recent_cycles(model, now) {
        let fh = clamp_lead(model, run, fcst_hour);
        let results: Vec<_> = futures_util::stream::iter(owned_specs.clone().into_iter().map(
            |(var, level, mv): (String, String, f64)| {
                let http = http.clone();
                async move {
                    let f = fetch_run_field(&http, model, run, fh, &var, &level, mv).await?;
                    Ok(match max_dim {
                        Some(cap) => f.subsampled(cap),
                        None => f,
                    })
                }
            },
        ))
        .buffered(HRRR_CONCURRENCY)
        .collect()
        .await;
        let mut fields = Vec::with_capacity(specs.len());
        let mut failed = None;
        for r in results {
            match r {
                Ok(f) => fields.push(f),
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        match failed {
            None => return Ok((run, fields)),
            Some(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no HRRR run found")))
}

/// Which wind the particle layer flies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindLevel {
    /// 10 m above ground — the wind you stand in, and what surface obs report.
    #[default]
    Surface,
    /// 500 mb — mid-level flow, roughly what steers storms. A quarter the bytes of 10 m.
    Steering,
}

impl WindLevel {
    /// The `.idx` level string. Both live in `wrfsfc`, so neither needs the much larger `wrfprs`
    /// file that [`crate::sounding`] pulls for a full profile.
    fn idx_level(self) -> &'static str {
        match self {
            WindLevel::Surface => "10 m above ground",
            WindLevel::Steering => "500 mb",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WindLevel::Surface => "10 m",
            WindLevel::Steering => "500 mb",
        }
    }
}

/// Fetch the `u`/`v` wind components for `level` as a matched pair from one model cycle.
///
/// **HRRR only, deliberately — do not add a [`Model`] parameter.** RAP's `awp130pgrb` packs the two
/// components as *submessages of a single GRIB2 message*, so its `.idx` lists them at one shared
/// byte offset:
///
/// ```text
/// 92.1:4703714:d=2026073012:UGRD:500 mb:anl:
/// 92.2:4703714:d=2026073012:VGRD:500 mb:anl:      <- same offset
/// ```
///
/// [`field_byte_range`] hands back the same range for both, and `gribberish`'s `Message::data()`
/// decodes the *first* data section, so a RAP `VGRD` request quietly returns u-wind: every vector
/// at 45° with `|u| == |v|`, wrong everywhere and plausible-looking for about three seconds. HRRR's
/// `wrfsfc` stores them as separate messages, so this path is safe. The same trap waits for any
/// future RAP-sourced vector field.
///
/// `min_valid` is `-∞` for both: wind components are signed, and the regrid's threshold is a drop
/// test, so anything higher would silently delete every westward or southward vector.
pub async fn fetch_wind(
    http: &reqwest::Client,
    level: WindLevel,
    fcst_hour: u8,
) -> anyhow::Result<(DateTime<Utc>, MrmsField, MrmsField)> {
    let lvl = level.idx_level();
    let (run, mut fields) = fetch_fields_one_run(
        http,
        Model::Hrrr,
        fcst_hour,
        &[
            ("UGRD", lvl, f64::NEG_INFINITY),
            ("VGRD", lvl, f64::NEG_INFINITY),
        ],
    )
    .await?;
    anyhow::ensure!(fields.len() == 2, "expected u and v, got {}", fields.len());
    // `.buffered` preserves input order, but a wind field is unreadable if u and v ever swap, so
    // pop them back-to-front explicitly rather than trusting that at a distance.
    let v = fields.pop().unwrap();
    let u = fields.pop().unwrap();
    anyhow::ensure!(
        u.nx == v.nx && u.ny == v.ny,
        "u/v grid mismatch: {}x{} vs {}x{}",
        u.nx,
        u.ny,
        v.nx,
        v.ny
    );
    Ok((run, u, v))
}

/// Fetch one field across forecast hours `1..=through_hour` from a SINGLE model cycle and fold
/// them into one grid by elementwise max — the "swath" a max-per-hour field is meant to be read as.
///
/// HRRR's max fields (MXUPHL and friends) each cover one hour's window, so a single hour answers
/// "where was rotation strongest between F+2 and F+3" — useful, but the map chasers actually want
/// is the union: everywhere a rotating storm is forecast to pass between now and then. Hours come
/// from one run for the same reason [`fetch_fields_one_run`] exists.
pub async fn fetch_field_swath(
    http: &reqwest::Client,
    var: &str,
    level: &str,
    through_hour: u8,
    min_valid: f64,
) -> anyhow::Result<HrrrForecast> {
    let through = through_hour.clamp(1, 18);
    let now = Utc::now();
    let mut last_err = None;
    for back in 1..=6 {
        let run = (now - chrono::Duration::hours(back))
            .with_minute(0)
            .unwrap()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();
        // Up to 18 forecast hours, each its own ranged GRIB fetch. Sequentially that is 18
        // round trips stacked end to end; six at a time cuts the wall clock to roughly a third.
        // The fold is still ordered, and still all-or-nothing.
        // Each future owns its inputs (a `reqwest::Client` clone is a refcount bump): borrowed
        // ones make the combined future non-`Send`, which the app's tokio spawn requires.
        let results: Vec<_> =
            futures_util::stream::iter((1..=through).map(|fh| {
                let (http, var, level) = (http.clone(), var.to_string(), level.to_string());
                async move {
                    fetch_run_field(&http, Model::Hrrr, run, fh, &var, &level, min_valid).await
                }
            }))
            .buffered(HRRR_CONCURRENCY)
            .collect()
            .await;
        let mut acc: Option<MrmsField> = None;
        let mut failed = None;
        for r in results {
            match r {
                Ok(f) => match acc.as_mut() {
                    None => acc = Some(f),
                    Some(a) => merge_max(a, &f),
                },
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        match (failed, acc) {
            (None, Some(mut field)) => {
                // The max grid covers F+1 through F+through; timestamp it at the ending
                // valid time, not the first slice's F+1 time retained by the fold.
                field.time = run + chrono::Duration::hours(i64::from(through));
                return Ok(HrrrForecast {
                    field,
                    run,
                    fcst_hour: through,
                    fcst_minutes: None,
                });
            }
            (e, _) => last_err = e.or(last_err),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no HRRR run found")))
}

/// Fold `src` into `dst` by keeping the larger value per cell. Both come from the same run and
/// the same regrid parameters, so the grids are aligned; mismatched shapes are left alone rather
/// than producing a scrambled field.
fn merge_max(dst: &mut MrmsField, src: &MrmsField) {
    if dst.nx != src.nx || dst.ny != src.ny || dst.values.len() != src.values.len() {
        return;
    }
    for (d, s) in dst.values.iter_mut().zip(&src.values) {
        if s.is_nan() {
            continue;
        }
        if d.is_nan() || s > d {
            *d = *s;
        }
    }
}

/// Longest lead this particular run publishes, from [`crate::model`]'s schedule.
///
/// `u8` because every caller's forecast hour is one; the schedule is in `u16` because the NBM
/// runs to 264 h, which those callers cannot ask for anyway.
fn clamp_lead(model: Model, run: DateTime<Utc>, fcst_hour: u8) -> u8 {
    let max = model.max_lead_for_cycle(run.hour()).min(u8::MAX as u16) as u8;
    fcst_hour.min(max)
}

/// The `.idx` sidecar for one model cycle and forecast hour — the catalogue of what that file
/// actually contains. Exposed so [`crate::model`]'s contract test can ask the real feeds which
/// fields a model publishes, rather than trusting a hand-written table.
pub async fn fetch_idx(
    http: &reqwest::Client,
    model: Model,
    run: DateTime<Utc>,
    fh: u8,
) -> anyhow::Result<String> {
    let date = format!("{:04}{:02}{:02}", run.year(), run.month(), run.day());
    let base = model.url(&date, run.hour(), fh);
    Ok(http
        .get(crate::net::fetch_url(&format!("{base}.idx")))
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?)
}

async fn fetch_run_field(
    http: &reqwest::Client,
    model: Model,
    run: DateTime<Utc>,
    fh: u8,
    var: &str,
    level: &str,
    min_valid: f64,
) -> anyhow::Result<MrmsField> {
    let date = format!("{:04}{:02}{:02}", run.year(), run.month(), run.day());
    let base = model.url(&date, run.hour(), fh);

    let idx = fetch_idx(http, model, run, fh).await?;
    let (start, end) = field_byte_range(&idx, var, level)
        .ok_or_else(|| anyhow::anyhow!("no {var}:{level} in idx"))?;

    let range = match end {
        Some(e) => format!("bytes={start}-{}", e - 1),
        None => format!("bytes={start}-"),
    };
    let bytes = http
        .get(crate::net::fetch_url(&base))
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .header("Range", range)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    // gribberish can panic on some packings; contain it (see mrms::fetch_latest).
    crate::task::guarded(|| decode_regrid(&bytes, model, min_valid))
        .unwrap_or_else(|_| anyhow::bail!("{} grib decode panicked", model.label()))
}

/// Find the `[start, end)` byte range of the message matching `var` (field 3) and `level`
/// (field 4) in a GRIB2 `.idx`. `end` is `None` when it's the last message (read to EOF).
pub(crate) fn field_byte_range(idx: &str, var: &str, level: &str) -> Option<(u64, Option<u64>)> {
    field_byte_range_inner(idx, var, level, None)
}

/// [`field_byte_range`] that also pins the forecast designator (field 5) — needed for the
/// sub-hourly product, where one file holds four `REFC:entire atmosphere` messages that differ
/// only by `"15 min fcst"` / `"30 min fcst"` / `"45 min fcst"` / `"60 min fcst"`.
pub(crate) fn field_byte_range_fcst(
    idx: &str,
    var: &str,
    level: &str,
    fcst: &str,
) -> Option<(u64, Option<u64>)> {
    field_byte_range_inner(idx, var, level, Some(fcst))
}

fn field_byte_range_inner(
    idx: &str,
    var: &str,
    level: &str,
    fcst: Option<&str>,
) -> Option<(u64, Option<u64>)> {
    let lines: Vec<&str> = idx.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let f: Vec<&str> = line.split(':').collect();
        if f.len() < 5 {
            continue;
        }
        if f[3] == var && f[4] == level && fcst.is_none_or(|c| f.get(5) == Some(&c)) {
            let start: u64 = f[1].parse().ok()?;
            // The end is the next *distinct* offset. RAP's idx lists a message that packs several
            // fields (VUCSH/VVCSH) as sibling lines sharing one offset; taking the very next line
            // would build the empty byte range `start..start` and fetch nothing.
            let end = lines[i + 1..]
                .iter()
                .filter_map(|n| n.split(':').nth(1))
                .filter_map(|s| s.parse::<u64>().ok())
                .find(|&o| o > start);
            return Some((start, end));
        }
    }
    None
}

/// Decode a single-message HRRR GRIB2 (Lambert grid) and scatter-regrid onto a regular lat/lon
/// grid, keeping the max dBZ per target cell (reflectivity composites well under max).
fn decode_regrid(raw: &[u8], model: Model, min_valid: f64) -> anyhow::Result<MrmsField> {
    use gribberish::data_message::DataMessage;
    use gribberish::message::read_message;
    let msg = read_message(raw, 0).ok_or_else(|| anyhow::anyhow!("no GRIB2 message"))?;
    let time = msg.forecast_date().unwrap_or_else(|_| Utc::now());
    let dm = DataMessage::try_from(&msg).map_err(|e| anyhow::anyhow!("hrrr decode: {e:?}"))?;
    let (lats, lons) = dm.metadata.latlng();
    let data = dm.data;
    anyhow::ensure!(
        lats.len() == data.len() && lons.len() == data.len(),
        "hrrr latlng/data length mismatch"
    );

    regrid(&lats, &lons, &data, time, model.res_deg(), min_valid)
}

/// Scatter native (lat, lon, value) triples onto a regular lat/lon grid (max per cell).
/// Pure + fixture-testable. Non-finite or below-`min_valid` samples are ignored.
/// `// ponytail: max-per-cell — for SRH this keeps the strongest (most positive) value per cell;`
/// `// negative (anticyclonic) SRH is retained only where no positive sample shares the cell.`
pub(crate) fn regrid(
    lats: &[f64],
    lons: &[f64],
    data: &[f64],
    time: DateTime<Utc>,
    res_deg: f64,
    min_valid: f64,
) -> anyhow::Result<MrmsField> {
    // Extent pass. min/max are associative, so the parallel reduce lands on the same bits as the
    // serial fold. The 1799x1059 native HRRR grid is ~1.9 M points.
    // Global grids are published on 0..360 longitudes. Wrapping here rather than at each caller
    // means one quad never straddles the antimeridian, which the map shader has no way to draw.
    let wrap = |lon: f64| if lon > 180.0 { lon - 360.0 } else { lon };
    let init = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    let step = |acc: (f64, f64, f64, f64), k: usize| {
        if !lats[k].is_finite() || !lons[k].is_finite() {
            return acc;
        }
        (
            acc.0.min(wrap(lons[k])),
            acc.1.max(wrap(lons[k])),
            acc.2.min(lats[k]),
            acc.3.max(lats[k]),
        )
    };
    let join = |a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)| {
        (a.0.min(b.0), a.1.max(b.1), a.2.min(b.2), a.3.max(b.3))
    };
    #[cfg(not(target_arch = "wasm32"))]
    let (lonmin, lonmax, latmin, latmax) = {
        use rayon::prelude::*;
        (0..data.len())
            .into_par_iter()
            .fold(|| init, step)
            .reduce(|| init, join)
    };
    #[cfg(target_arch = "wasm32")]
    let (lonmin, lonmax, latmin, latmax) = {
        let _ = join;
        (0..data.len()).fold(init, step)
    };
    anyhow::ensure!(
        lonmax > lonmin && latmax > latmin,
        "hrrr grid has no finite extent"
    );

    let nx = (((lonmax - lonmin) / res_deg).ceil() as usize).max(1);
    let ny = (((latmax - latmin) / res_deg).ceil() as usize).max(1);
    let mut values = vec![f32::NAN; nx * ny];
    // Two phases: the per-sample cell index (the divisions, the expensive part) computes in
    // parallel, then a serial max-scatter. The scatter can't be split by output row without
    // rescanning the whole input per band, and max-per-cell is order-independent, so this is
    // bit-for-bit the serial result either way.
    let cell_of = |k: usize| -> Option<(usize, f32)> {
        let v = data[k];
        if !v.is_finite() || v < min_valid || !lats[k].is_finite() || !lons[k].is_finite() {
            return None;
        }
        let gx = (((wrap(lons[k]) - lonmin) / res_deg) as usize).min(nx - 1);
        // Row 0 is the northernmost latitude (matches MrmsField convention).
        let gy = (((latmax - lats[k]) / res_deg) as usize).min(ny - 1);
        Some((gy * nx + gx, v as f32))
    };
    #[cfg(not(target_arch = "wasm32"))]
    let cells: Vec<(usize, f32)> = {
        use rayon::prelude::*;
        (0..data.len())
            .into_par_iter()
            .filter_map(cell_of)
            .collect()
    };
    #[cfg(target_arch = "wasm32")]
    let cells: Vec<(usize, f32)> = (0..data.len()).filter_map(cell_of).collect();
    for (idx, v) in cells {
        let cell = &mut values[idx];
        *cell = if cell.is_nan() { v } else { cell.max(v) };
    }

    Ok(MrmsField {
        values,
        nx,
        ny,
        lon_west: lonmin,
        lon_east: lonmax,
        lat_north: latmax,
        lat_south: latmin,
        time,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned_candidates_use_exact_valid_time_and_newest_cycle_first() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 9, 14, 12, 30, 0).unwrap();
        let target = Utc.with_ymd_and_hms(2026, 9, 14, 11, 0, 0).unwrap();
        let candidates = aligned_run_hours(Model::Rap, target, now);
        assert_eq!(candidates[0], (target, 0));
        assert_eq!(candidates[1], (target - chrono::Duration::hours(1), 1));
        assert!(candidates
            .iter()
            .all(|(run, fh)| { *run + chrono::Duration::hours(i64::from(*fh)) == target }));
        assert!(
            aligned_run_hours(Model::Rap, target + chrono::Duration::minutes(15), now).is_empty()
        );
        assert!(aligned_run_hours(Model::Rap, target - chrono::Duration::days(2), now).is_empty());
    }

    /// `fetch_field_previous_run` (ROADMAP_NEW F5's run-to-run comparison) seeds its search with
    /// `current_run` itself rather than `Utc::now()`, precisely so it can never hand back the
    /// same run it is meant to be compared against. This checks that seed choice in isolation,
    /// without a network fetch.
    #[test]
    fn aligned_candidates_seeded_at_the_current_run_never_include_it() {
        use chrono::TimeZone;
        let current_run = Utc.with_ymd_and_hms(2026, 9, 14, 12, 0, 0).unwrap();
        let target_valid = current_run; // the analysis-hour (fh=0) case fetch_pair uses
        let candidates = aligned_run_hours(Model::Hrrr, target_valid, current_run);
        assert!(
            candidates.iter().all(|(run, _)| *run < current_run),
            "{candidates:?}"
        );
        // The immediately-previous cycle, one lead hour out, is the first (newest) candidate.
        assert_eq!(candidates[0], (current_run - chrono::Duration::hours(1), 1));
    }

    #[test]
    fn subhourly_idx_picks_the_right_15min_step() {
        // Real wrfsubhf01 layout: four REFC:entire atmosphere messages, one per 15-min step.
        let idx = "1:0:d=2024060112:REFC:entire atmosphere:15 min fcst:\n\
                   2:337244:d=2024060112:RETOP:cloud top:15 min fcst:\n\
                   50:50602329:d=2024060112:REFC:entire atmosphere:30 min fcst:\n\
                   51:50925519:d=2024060112:RETOP:cloud top:30 min fcst:\n\
                   99:102242691:d=2024060112:REFC:entire atmosphere:45 min fcst:\n\
                   148:154842502:d=2024060112:REFC:entire atmosphere:60 min fcst:\n\
                   149:155200986:d=2024060112:RETOP:cloud top:60 min fcst:\n";

        // Plain field_byte_range would return the first REFC (15 min); the fcst-pinned lookup
        // walks to the one asked for, and its end is the next distinct offset.
        assert_eq!(
            field_byte_range_fcst(idx, "REFC", "entire atmosphere", "15 min fcst"),
            Some((0, Some(337244)))
        );
        assert_eq!(
            field_byte_range_fcst(idx, "REFC", "entire atmosphere", "30 min fcst"),
            Some((50602329, Some(50925519)))
        );
        assert_eq!(
            field_byte_range_fcst(idx, "REFC", "entire atmosphere", "45 min fcst"),
            Some((102242691, Some(154842502)))
        );
        // Last REFC before EOF-ish: end is the next distinct offset (the trailing RETOP).
        assert_eq!(
            field_byte_range_fcst(idx, "REFC", "entire atmosphere", "60 min fcst"),
            Some((154842502, Some(155200986)))
        );
        // A step this file does not carry.
        assert_eq!(
            field_byte_range_fcst(idx, "REFC", "entire atmosphere", "75 min fcst"),
            None
        );
    }

    #[test]
    fn subhourly_url_and_file_index() {
        // wrfsubhf01 holds 15..60 min; wrfsubhf02 holds 75..120.
        assert_eq!(subhourly_file_index(15), 1);
        assert_eq!(subhourly_file_index(60), 1);
        assert_eq!(subhourly_file_index(75), 2);
        assert_eq!(subhourly_file_index(120), 2);
        assert_eq!(subhourly_file_index(18 * 60), 18);
        assert_eq!(subhourly_file_index(9999), 18, "clamped to the model range");
        assert!(subhourly_url("20240601", 12, 1)
            .ends_with("hrrr.20240601/conus/hrrr.t12z.wrfsubhf01.grib2"));
    }

    #[test]
    fn subhourly_valid_time_carries_minutes() {
        let run = "2024-06-01T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let fc = HrrrForecast {
            field: MrmsField {
                values: Vec::new(),
                nx: 0,
                ny: 0,
                lon_west: 0.0,
                lon_east: 0.0,
                lat_north: 0.0,
                lat_south: 0.0,
                time: run,
            },
            run,
            fcst_hour: 0,
            fcst_minutes: Some(45),
        };
        assert_eq!(
            fc.valid(),
            "2024-06-01T12:45:00Z".parse::<DateTime<Utc>>().unwrap()
        );
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn subhourly_refc_decodes() {
        let http = reqwest::Client::new();
        let fc = fetch_forecast_subhourly(&http, 45)
            .await
            .expect("HRRR sub-hourly F+45min REFC");
        let finite = fc.field.values.iter().filter(|v| v.is_finite()).count();
        eprintln!(
            "HRRR wrfsubhf F+45min — {}x{} grid, {finite} finite cells, run {}",
            fc.field.nx, fc.field.ny, fc.run
        );
        assert!(finite > fc.field.values.len() / 3, "coverage holes");
        assert_eq!(fc.fcst_minutes, Some(45));
        assert_eq!(fc.valid(), fc.run + chrono::Duration::minutes(45));
    }

    #[test]
    fn idx_range_finds_field() {
        let idx = "1:0:d=2026:REFC:entire atmosphere:1 hour fcst:\n\
                   2:396353:d=2026:RETOP:cloud top:1 hour fcst:\n\
                   3:500000:d=2026:ASNOW:surface:0-6 hour acc fcst:\n";
        assert_eq!(
            field_byte_range(idx, "REFC", "entire atmosphere"),
            Some((0, Some(396353)))
        );
        // Accumulations name a window in the forecast field, not a plain hour.
        assert_eq!(
            field_byte_range(idx, "ASNOW", "surface"),
            Some((500000, None))
        );
        // Last message → open-ended range; var+level disambiguates same-var different levels.
        let idx2 = "1:100:d=2026:CAPE:surface:\n\
                    2:5000:d=2026:CAPE:90-0 mb above ground:\n\
                    3:9000:d=2026:HLCY:3000-0 m above ground:\n";
        assert_eq!(
            field_byte_range(idx2, "CAPE", "surface"),
            Some((100, Some(5000)))
        );
        assert_eq!(
            field_byte_range(idx2, "CAPE", "90-0 mb above ground"),
            Some((5000, Some(9000)))
        );
        assert_eq!(
            field_byte_range(idx2, "HLCY", "3000-0 m above ground"),
            Some((9000, None))
        );
        // RAP packs some pairs into one message, listed as sibling lines at the same offset; the
        // range must run to the next distinct offset, not to the sibling.
        let rap = "241.1:100:d=2026:USTM:0-6000 m above ground:anl:\n\
                   241.2:100:d=2026:VSTM:0-6000 m above ground:anl:\n\
                   242.1:200:d=2026:VUCSH:0-6000 m above ground:anl:\n";
        assert_eq!(
            field_byte_range(rap, "USTM", "0-6000 m above ground"),
            Some((100, Some(200)))
        );
    }

    #[test]
    fn rap_and_hrrr_urls() {
        assert!(Model::Hrrr
            .url("20260728", 21, 0)
            .ends_with("hrrr.20260728/conus/hrrr.t21z.wrfsfcf00.grib2"));
        assert!(Model::Rap
            .url("20260728", 21, 0)
            .ends_with("rap.20260728/rap.t21z.awp130pgrbf00.grib2"));
        assert!(
            Model::Rap.res_deg() > Model::Hrrr.res_deg(),
            "13 km vs 3 km"
        );
        assert!(Model::Nam
            .url("20260728", 12, 6)
            .ends_with("nam.20260728/nam.t12z.awip1206.tm00.grib2"));
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn rap_analysis_cape_decodes() {
        let http = reqwest::Client::new();
        let fc = fetch_field(&http, Model::Rap, "CAPE", "surface", 0, 0.0)
            .await
            .expect("RAP f00 CAPE");
        let finite: Vec<f32> = fc
            .field
            .values
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .collect();
        let max = finite.iter().copied().fold(f32::MIN, f32::max);
        eprintln!(
            "RAP {}x{} run {} — {} finite cells, max {max:.0} J/kg",
            fc.field.nx,
            fc.field.ny,
            fc.run,
            finite.len()
        );
        // A 13 km CONUS grid regridded at 0.14° should cover most of its own box, and surface CAPE
        // anywhere in the country tops out well under 10000 J/kg.
        assert!(finite.len() > fc.field.values.len() / 2, "coverage holes");
        assert!((0.0..10_000.0).contains(&max), "implausible CAPE {max}");

        // Cross-check against the HRRR analysis for the same hour: different models on different
        // grids won't match cell for cell, but their CONUS-wide peak CAPE should be the same order.
        let hrrr = fetch_field(&http, Model::Hrrr, "CAPE", "surface", 0, 0.0)
            .await
            .expect("HRRR f00 CAPE");
        let hmax = hrrr
            .field
            .values
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .fold(f32::MIN, f32::max);
        eprintln!("peak CAPE — RAP {max:.0} vs HRRR {hmax:.0} J/kg");
        let ratio = (max as f64 / hmax.max(1.0) as f64).max(hmax as f64 / max.max(1.0) as f64);
        assert!(ratio < 3.0, "RAP {max} and HRRR {hmax} disagree wildly");
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn nam_parent_grid_cape_decodes() {
        let http = reqwest::Client::new();
        let fc = fetch_field(&http, Model::Nam, "CAPE", "surface", 6, 0.0)
            .await
            .expect("NAM f06 CAPE");
        let finite: Vec<f32> = fc
            .field
            .values
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .collect();
        let max = finite.iter().copied().fold(f32::MIN, f32::max);
        eprintln!(
            "NAM {}x{} run {} — {} finite cells, max {max:.0} J/kg",
            fc.field.nx,
            fc.field.ny,
            fc.run,
            finite.len()
        );
        assert!(finite.len() > fc.field.values.len() / 2, "coverage holes");
        assert!((0.0..10_000.0).contains(&max), "implausible CAPE {max}");

        // Same idea as the RAP/HRRR cross-check above: SRH from the NAM's own storm-relative
        // helicity field, sane range and a real CONUS-wide max.
        let srh = fetch_field(
            &http,
            Model::Nam,
            "HLCY",
            "3000-0 m above ground",
            6,
            f64::NEG_INFINITY,
        )
        .await
        .expect("NAM f06 0-3km SRH");
        let srh_max = srh
            .field
            .values
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .fold(f32::MIN, f32::max);
        eprintln!("NAM 0-3km SRH max {srh_max:.0} m2/s2");
        assert!(
            (-2000.0..2000.0).contains(&srh_max),
            "implausible SRH {srh_max}"
        );
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn hrrr_wind_decodes_as_a_matched_pair() {
        let http = reqwest::Client::new();
        let (run, u, v) = fetch_wind(&http, WindLevel::Surface, 1)
            .await
            .expect("HRRR wind");
        assert_eq!((u.nx, u.ny), (v.nx, v.ny));

        let finite = u
            .values
            .iter()
            .zip(&v.values)
            .filter(|(a, b)| a.is_finite() && b.is_finite());
        let (mut n, mut max_kt) = (0usize, 0.0f32);
        let (mut neg_u, mut neg_v) = (0usize, 0usize);
        for (a, b) in finite {
            n += 1;
            max_kt = max_kt.max((a * a + b * b).sqrt() * 1.943_844);
            neg_u += (*a < 0.0) as usize;
            neg_v += (*b < 0.0) as usize;
        }
        let holes = 1.0 - n as f64 / u.values.len() as f64;
        // The whole-grid figure is dominated by the Lambert domain's corners, which fall outside
        // the model entirely and are supposed to be empty. The middle third is all inside CONUS,
        // so its empty fraction is the one that says whether the regrid leaves real holes.
        let (mut inner, mut inner_empty) = (0usize, 0usize);
        for y in u.ny / 3..u.ny * 2 / 3 {
            for x in u.nx / 3..u.nx * 2 / 3 {
                inner += 1;
                inner_empty += !u.values[y * u.nx + x].is_finite() as usize;
            }
        }
        eprintln!(
            "interior (middle third) — {:.2}% empty of {inner} cells",
            inner_empty as f64 / inner as f64 * 100.0
        );
        eprintln!(
            "HRRR 10 m wind {}x{} run {run} — {n} paired cells, {:.1}% empty, max {max_kt:.0} kt, \
             {:.0}% easterly, {:.0}% southerly",
            u.nx,
            u.ny,
            holes * 100.0,
            neg_u as f64 / n as f64 * 100.0,
            neg_v as f64 / n as f64 * 100.0,
        );

        assert!(
            n > u.values.len() / 2,
            "coverage holes: {:.1}% empty",
            holes * 100.0
        );
        // Both signs must survive `min_valid`: a CONUS-wide field always has wind blowing every way.
        assert!(neg_u > n / 20 && neg_v > n / 20, "one sign got clipped");
        // Surface wind peaks somewhere in the country, but not at jet-stream speeds.
        assert!(
            (10.0..120.0).contains(&max_kt),
            "implausible peak {max_kt} kt"
        );
        // The RAP trap, checked from the outside: identical components mean one field read twice.
        assert!(
            u.values
                .iter()
                .zip(&v.values)
                .any(|(a, b)| (a - b).abs() > 0.5),
            "u and v are the same field — submessage aliasing"
        );

        // `fetch_wind` pops the pair off a `.buffered()` stream assuming input order. A silent
        // swap there would point every vector 90 degrees wrong and still look like weather, so
        // pin it against single-field fetches that cannot be reordered. Same run, same hour.
        let lvl = WindLevel::Surface.idx_level();
        let solo_u = fetch_run_field(&http, Model::Hrrr, run, 1, "UGRD", lvl, f64::NEG_INFINITY)
            .await
            .expect("solo UGRD");
        assert_eq!((solo_u.nx, solo_u.ny), (u.nx, u.ny));
        let diff = solo_u
            .values
            .iter()
            .zip(&u.values)
            .filter(|(a, b)| a.is_finite() && b.is_finite())
            .filter(|(a, b)| (*a - *b).abs() > 0.01)
            .count();
        assert_eq!(
            diff, 0,
            "fetch_wind's `u` is not UGRD — the pair is swapped"
        );
    }

    #[test]
    fn wind_levels_map_to_idx_strings() {
        assert_eq!(WindLevel::Surface.idx_level(), "10 m above ground");
        assert_eq!(WindLevel::Steering.idx_level(), "500 mb");
    }

    #[test]
    fn regrid_scatters_into_regular_grid() {
        // Two points ~0.1° apart land in distinct cells; the higher dBZ wins its cell.
        let lats = vec![40.0, 40.0, 40.001];
        let lons = vec![-100.0, -99.9, -100.0];
        let data = vec![25.0, 50.0, 45.0]; // first and third share a cell → keep max (45)
        let f = regrid(&lats, &lons, &data, Utc::now(), 0.04, -30.0).unwrap();
        assert!(f.nx >= 2 && f.ny >= 1);
        let north_west = f.values[0]; // row 0 = north, col 0 = west
        assert!(
            (north_west - 45.0).abs() < 1e-3,
            "max-per-cell kept: {north_west}"
        );
    }

    #[test]
    fn regrid_min_valid_keeps_negatives_for_srh() {
        // A −50 SRH sample survives with min_valid = −∞ but is dropped at the REFC −30 threshold.
        // Two spread points give the grid a finite extent; the NW cell (row 0, col 0) is the −50.
        let lats = vec![41.0, 40.0];
        let lons = vec![-100.0, -99.0];
        let data = vec![-50.0, 20.0];
        let kept = regrid(&lats, &lons, &data, Utc::now(), 0.04, f64::NEG_INFINITY).unwrap();
        assert!(
            (kept.values[0] - -50.0).abs() < 1e-3,
            "negative SRH kept in NW cell: {}",
            kept.values[0]
        );
        let dropped = regrid(&lats, &lons, &data, Utc::now(), 0.04, -30.0).unwrap();
        assert!(dropped.values[0].is_nan(), "below-threshold dropped");
    }

    #[test]
    fn cycle_walks_follow_each_model_s_own_run_schedule() {
        let now = "2026-08-25T14:37:00Z".parse::<DateTime<Utc>>().unwrap();

        let hrrr = recent_cycles(Model::Hrrr, now);
        assert_eq!(hrrr[0].hour(), 13, "one hour back, on the hour");
        assert_eq!(hrrr[1].hour(), 12);
        assert_eq!(hrrr.len(), 6);

        // The nest runs 00/06/12/18: 14:37 minus a cycle is 08:37, which floors to 06z.
        let nam = recent_cycles(Model::NamNest, now);
        assert_eq!(nam[0].hour(), 6);
        assert_eq!(nam[1].hour(), 0);
        assert_eq!(nam[2].hour(), 18, "and back into yesterday");
        assert_eq!(nam[2].day(), 24);
        assert!(nam.iter().all(|c| c.hour() % 6 == 0));
    }

    #[test]
    fn the_new_sources_point_at_their_own_buckets() {
        let nam = Model::NamNest.url("20260825", 12, 6);
        assert!(nam.contains("noaa-nam-pds"));
        assert!(nam.ends_with("nam.t12z.conusnest.hiresf06.tm00.grib2"));
        // NBM forecast hours are three digits, and CONUS is the `co` domain.
        let nbm = Model::Nbm.url("20260825", 12, 6);
        assert!(nbm.contains("noaa-nbm-grib2-pds"));
        assert!(nbm.ends_with("blend.t12z.core.f006.co.grib2"));
    }
}
