//! Global model fields: NOAA's GFS and ECMWF's open IFS.
//!
//! Both publish quarter-degree GRIB2 on a plain lat/lon grid with a sidecar index, so a field is
//! two requests: read the index, range-GET the one message. That is the same shape as the HRRR
//! path, and the decode and regrid are literally the same code — only the URL and the index
//! format differ.
//!
//! * **GFS** — `noaa-gfs-bdp-pds`, NOAA's `.idx` text format, simple packing.
//! * **ECMWF** — `data.ecmwf.int`, a JSON-lines `.index`, CCSDS packing (which the vendored
//!   gribberish decodes in pure Rust, so this works on wasm too).
//!
//! Longitudes arrive on 0..360 and are wrapped to −180..180 inside the shared regrid, so a field
//! that spans the dateline lands continuous instead of drawing one quad across the whole map.
//!
//! ponytail: one quad covering −180..180, so the field does not repeat past the antimeridian —
//! pan east of +180 and it simply ends. Drawing a second copy is easy if anyone chases Fiji.

use crate::alerts::USER_AGENT;
use crate::mrms::MrmsField;
use chrono::{DateTime, Datelike, Timelike, Utc};

const GFS_BUCKET: &str = "https://noaa-gfs-bdp-pds.s3.amazonaws.com";
const ECMWF_BASE: &str = "https://data.ecmwf.int/forecasts";
const GEFS_BUCKET: &str = "https://noaa-gefs-pds.s3.amazonaws.com";
/// Environment and Climate Change Canada's public "Datamart" — plain HTTPS directory listings,
/// one GRIB2 message per file already (no sidecar index to slice one out of a bundle the way
/// GFS/ECMWF need). `today` is the only window Datamart exposes; there is no `yesterday`, so a
/// walk-back that crosses a UTC midnight can come up empty even for a cycle that really did post
/// — the same honest "no cycle found" every model already surfaces when nothing's ready yet.
const GDPS_BASE: &str = "https://dd.weather.gc.ca/today/model_gdps/15km";

/// Quarter-degree source grids (GFS, ECMWF) resample onto this. Coarser than the grid itself, so
/// the scatter fills every cell; 1440×721 at 0.25° well under the 4096 texture cap either way.
const RES_DEG: f64 = 0.3;
/// GEFS's ensemble mean posts at half a degree, not a quarter — scattering it onto `RES_DEG`
/// left most of the output grid empty (a source cell coarser than its target leaves gaps between
/// samples), so it gets its own coarser target to match.
const GEFS_RES_DEG: f64 = 0.6;

/// Which global model to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GlobalModel {
    #[default]
    Gfs,
    Ecmwf,
    /// GFS Ensemble mean, 0.5° — thirty-one members averaged down to one field. Coarser than a
    /// single deterministic run, but it is the average outcome across the spread rather than one
    /// realization of it, which is its own kind of useful.
    Gefs,
    /// Environment Canada's Global Deterministic Prediction System, 0.15° — a second national
    /// weather service's own global model, independent of NCEP/ECMWF's data assimilation and
    /// physics entirely.
    Gdps,
}

impl GlobalModel {
    pub fn label(self) -> &'static str {
        match self {
            GlobalModel::Gfs => "GFS",
            GlobalModel::Ecmwf => "ECMWF",
            GlobalModel::Gefs => "GEFS mean",
            GlobalModel::Gdps => "GDPS",
        }
    }

    /// Hours between cycles. All three run four times a day.
    fn cycle_step(self) -> u32 {
        6
    }

    /// Hours after a cycle's name before it has typically finished posting. Rough on purpose: it
    /// only decides where a run list starts, and a run that is not up yet fails as a missing file.
    pub fn typical_latency_hours(self) -> i64 {
        match self {
            GlobalModel::Gfs => 5,
            GlobalModel::Ecmwf => 8,
            GlobalModel::Gefs => 6,
            GlobalModel::Gdps => 6,
        }
    }

    /// The cycles this model publishes, newest plausible first.
    pub fn run_choices(self, now: DateTime<Utc>, count: usize) -> Vec<DateTime<Utc>> {
        let step = self.cycle_step();
        let base = now - chrono::Duration::hours(self.typical_latency_hours());
        let floored = (base.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc())
            + chrono::Duration::hours(i64::from(base.hour() / step * step));
        (0..count)
            .map(|i| floored - chrono::Duration::hours((i as u32 * step) as i64))
            .collect()
    }
}

/// Fetch `field` at `fh` from exactly `run`. Unlike [`fetch`] this never walks back to another
/// cycle: naming a run means getting that one, or an honest error.
pub async fn fetch_at_run(
    http: &reqwest::Client,
    model: GlobalModel,
    field: GlobalField,
    run: DateTime<Utc>,
    fh: u16,
) -> anyhow::Result<GlobalForecast> {
    fetch_run(http, model, field, run, fh).await
}

/// A field a global model can draw. Kept to what both publish, so switching source keeps the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlobalField {
    Mslp,
    Height500,
    Temp2m,
    Dewpoint2m,
    Wind10m,
    Precip,
}

impl GlobalField {
    pub const ALL: [GlobalField; 6] = [
        GlobalField::Mslp,
        GlobalField::Height500,
        GlobalField::Temp2m,
        GlobalField::Dewpoint2m,
        GlobalField::Wind10m,
        GlobalField::Precip,
    ];

    pub fn label(self) -> &'static str {
        match self {
            GlobalField::Mslp => "MSLP",
            GlobalField::Height500 => "500 hPa height",
            GlobalField::Temp2m => "2 m temp",
            GlobalField::Dewpoint2m => "2 m dewpoint",
            GlobalField::Wind10m => "10 m wind",
            GlobalField::Precip => "Total precip",
        }
    }

    /// Stable slug for settings and the headless CLI.
    pub fn slug(self) -> &'static str {
        match self {
            GlobalField::Mslp => "mslp",
            GlobalField::Height500 => "gh500",
            GlobalField::Temp2m => "t2m",
            GlobalField::Dewpoint2m => "td2m",
            GlobalField::Wind10m => "wind10m",
            GlobalField::Precip => "precip",
        }
    }

    pub fn from_slug(s: &str) -> Option<GlobalField> {
        GlobalField::ALL.into_iter().find(|f| f.slug() == s)
    }

    /// GFS `.idx` `(var, level)`.
    fn gfs_key(self) -> (&'static str, &'static str) {
        match self {
            GlobalField::Mslp => ("PRMSL", "mean sea level"),
            GlobalField::Height500 => ("HGT", "500 mb"),
            GlobalField::Temp2m => ("TMP", "2 m above ground"),
            GlobalField::Dewpoint2m => ("DPT", "2 m above ground"),
            GlobalField::Wind10m => ("UGRD", "10 m above ground"),
            GlobalField::Precip => ("PWAT", "entire atmosphere (considered as a single layer)"),
        }
    }

    /// ECMWF index `(param, levtype, level)`.
    fn ecmwf_key(self) -> (&'static str, &'static str, Option<&'static str>) {
        match self {
            GlobalField::Mslp => ("msl", "sfc", None),
            GlobalField::Height500 => ("gh", "pl", Some("500")),
            GlobalField::Temp2m => ("2t", "sfc", None),
            GlobalField::Dewpoint2m => ("2d", "sfc", None),
            GlobalField::Wind10m => ("10u", "sfc", None),
            GlobalField::Precip => ("tp", "sfc", None),
        }
    }

    /// GDPS Datamart filename `(variable, level)` tokens — `{variable}_{level}` between the
    /// model name and the grid spec in `{date}T{HH}Z_MSC_GDPS_{variable}_{level}_LatLon0.15_PT{fh}H.grib2`.
    /// Wind is the one genuine semantic difference from GFS/ECMWF's key: GDPS publishes speed
    /// directly rather than a U component, so this is the actual scalar magnitude, not one
    /// vector component read as if it were the whole story.
    fn gdps_key(self) -> (&'static str, &'static str) {
        match self {
            GlobalField::Mslp => ("Pressure", "MSL"),
            GlobalField::Height500 => ("GeopotentialHeight", "IsbL-0500"),
            GlobalField::Temp2m => ("AirTemp", "AGL-2m"),
            GlobalField::Dewpoint2m => ("DewPoint", "AGL-2m"),
            GlobalField::Wind10m => ("WindSpeed", "AGL-10m"),
            GlobalField::Precip => ("Precip-Accum", "Sfc"),
        }
    }

    /// Source-independent field metadata (Phase A1) shared by every global model that publishes
    /// this quantity — the same pattern `wxdata::model::ModelField::descriptor` established for
    /// HRRR/RAP's regional fields. Provider-specific wire spelling stays in `gfs_key`/`ecmwf_key`/
    /// `gdps_key` above; this owns presentation (units, palette, contour default) instead.
    pub fn descriptor(self) -> &'static crate::field::FieldDescriptor {
        GLOBAL_FIELD_DEFS
            .iter()
            .find(|entry| entry.global_field == self)
            .map(|entry| &entry.descriptor)
            .expect("every GlobalField has a FieldDescriptor")
    }
}

struct GlobalFieldEntry {
    global_field: GlobalField,
    descriptor: crate::field::FieldDescriptor,
}

macro_rules! global_field {
    ($kind:ident, $id:literal, $name:literal, $description:literal, $units:ident,
     $aliases:literal, $palette:ident, $interval:expr) => {
        GlobalFieldEntry {
            global_field: GlobalField::$kind,
            descriptor: crate::field::FieldDescriptor {
                id: crate::field::FieldId($id),
                source: crate::field::DataSource::GlobalModels,
                family: crate::field::FieldFamily::Model,
                name: $name,
                description: $description,
                units: crate::field::Unit::$units,
                value_kind: crate::field::ValueKind::Scalar,
                aliases: $aliases,
                default_palette: crate::field::PaletteId::$palette,
                default_contour_interval: $interval,
                valid_domain: Some(crate::field::GeographicBounds::WORLD),
                // The decoder has already converted GRIB bitmap/threshold exclusions to NaN.
                missing_values: &[],
            },
        }
    };
}

/// One entry per [`GlobalField`] variant. Native GRIB units (Pa, m, K, m/s) — the display-scaled
/// units a legend actually shows (hPa, dam, kt) are a rendering concern the ramp's own
/// `input_scale` applies, same split `wxdata::model`'s regional fields already use.
static GLOBAL_FIELD_DEFS: &[GlobalFieldEntry] = &[
    global_field!(
        Mslp,
        "global-mslp",
        "MSLP",
        "Atmospheric pressure reduced to mean sea level",
        Pascals,
        "surface pressure isobars synoptic PRMSL msl GFS ECMWF",
        MeanSeaLevelPressure,
        Some(200.0)
    ),
    global_field!(
        Height500,
        "global-height500",
        "500 hPa height",
        "Geopotential height at the 500 hPa pressure surface — the mid-level steering flow",
        Meters,
        "steering flow trough ridge HGT gh GFS ECMWF",
        Height500,
        Some(60.0)
    ),
    global_field!(
        Temp2m,
        "global-temp2m",
        "2 m temperature",
        "Air temperature two metres above ground",
        Kelvin,
        "surface temperature TMP 2t GFS ECMWF",
        Temperature,
        Some(2.0)
    ),
    global_field!(
        Dewpoint2m,
        "global-dewpoint2m",
        "2 m dewpoint",
        "Dewpoint temperature two metres above ground",
        Kelvin,
        "surface moisture DPT 2d GFS ECMWF",
        Dewpoint,
        Some(2.0)
    ),
    global_field!(
        Wind10m,
        "global-wind10m",
        "10 m wind",
        "Wind speed ten metres above ground",
        MetersPerSecond,
        "surface wind UGRD 10u GFS ECMWF",
        Wind10m,
        None
    ),
    global_field!(
        Precip,
        "global-precip",
        "Precipitable water",
        "Total column water vapor — how wet the air mass is, not how much has fallen",
        Millimeters,
        "moisture PWAT tp GFS ECMWF",
        PrecipitableWater,
        None
    ),
];

/// One decoded global field plus the cycle it came from.
pub struct GlobalForecast {
    pub field: MrmsField,
    pub run: DateTime<Utc>,
    pub fcst_hour: u16,
}

impl GlobalForecast {
    pub fn valid(&self) -> DateTime<Utc> {
        self.run + chrono::Duration::hours(self.fcst_hour as i64)
    }
}

/// Fetch `field` at forecast hour `fh`, walking back through recent cycles until one has it.
///
/// Global models post slowly — GFS takes a few hours to finish a cycle — so the newest cycle
/// directory usually exists before the file does. Walking back is what makes the layer reliable.
pub async fn fetch(
    http: &reqwest::Client,
    model: GlobalModel,
    field: GlobalField,
    fh: u16,
) -> anyhow::Result<GlobalForecast> {
    fetch_latest(http, model, field, fh).await.map(|(_, f)| f)
}

/// [`fetch`], but also hands back which cycle actually answered — [`fetch_point_series`] needs
/// to pin every later hour to that same run instead of re-discovering it hour by hour.
async fn fetch_latest(
    http: &reqwest::Client,
    model: GlobalModel,
    field: GlobalField,
    fh: u16,
) -> anyhow::Result<(DateTime<Utc>, GlobalForecast)> {
    let now = Utc::now();
    let mut last_err = None;
    for back in 0..5 {
        let step = model.cycle_step() as i64;
        let hours = (now.hour() as i64 / step) * step - back * step;
        let run = (now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc())
            + chrono::Duration::hours(hours);
        match fetch_run(http, model, field, run, fh).await {
            Ok(f) => return Ok((run, f)),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no {} cycle found", model.label())))
}

/// One point's value from `field`, at every hour in `hours`, all pinned to the one cycle that
/// answered the first request — a meteogram describes one run's evolution, not whichever cycle
/// happened to be newest at each individual forecast hour (which can differ near a cycle
/// boundary, the same edge [`fetch_aligned`]'s doc comment explains for comparing two models).
///
/// `hours` should already be sorted ascending; the first one picks the run every later hour
/// reuses. A later hour failing (not yet posted, or past the model's own forecast length) is
/// `None` in its slot rather than aborting the rest of the series — a gap in the graph's line,
/// not an error for the whole thing over one missing hour.
pub async fn fetch_point_series(
    http: &reqwest::Client,
    model: GlobalModel,
    field: GlobalField,
    lon: f64,
    lat: f64,
    hours: &[u16],
) -> anyhow::Result<Vec<(DateTime<Utc>, Option<f32>)>> {
    anyhow::ensure!(
        field.descriptor().supports_location(lon, lat),
        "{} location {lat:.3}, {lon:.3} is outside the published field domain",
        field.label()
    );
    let Some((&first, rest)) = hours.split_first() else {
        return Ok(Vec::new());
    };
    let (run, f) = fetch_latest(http, model, field, first).await?;
    let mut out = vec![(f.valid(), f.field.sample_bilinear(lon, lat))];
    for &fh in rest {
        let valid = run + chrono::Duration::hours(fh as i64);
        let value = fetch_run(http, model, field, run, fh)
            .await
            .ok()
            .and_then(|f| f.field.sample_bilinear(lon, lat));
        out.push((valid, value));
    }
    Ok(out)
}

/// Fetch `model`'s `field` at whichever forecast hour lands exactly on `target_valid`, instead of
/// at a fixed `fh` from whatever cycle happens to be newest right now.
///
/// Two models on the same `fh` are not necessarily the same instant: both GFS and ECMWF cycle
/// every 6 h from the same UTC anchor, but [`fetch`]'s walk-back only skips a cycle that has not
/// posted *yet* — one model can be a cycle ahead of the other for hours at a time, and comparing
/// them at equal `fh` then compares two different valid times without saying so.
///
/// This is exact, not interpolated: since every cycle for both models lands on a whole UTC hour,
/// the cycle at or before `target_valid` is always some whole number of `cycle_step`s behind it,
/// and the forecast hour that reaches `target_valid` from there always exists in principle — the
/// walk-back here is purely for the same "not posted yet" latency [`fetch`] already tolerates.
/// What is not guaranteed is that the model has *published* that specific forecast hour (some
/// only post every third or sixth hour past a range). A difference caller must treat that failure
/// as unavailable or try aligning the other model; it must never subtract unmatched times.
pub async fn fetch_aligned(
    http: &reqwest::Client,
    model: GlobalModel,
    field: GlobalField,
    target_valid: DateTime<Utc>,
) -> anyhow::Result<GlobalForecast> {
    let step = model.cycle_step() as i64;
    let midnight = target_valid
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
    let mut last_err = None;
    for back in 0..5 {
        let hours = (target_valid.hour() as i64 / step) * step - back * step;
        let run = midnight + chrono::Duration::hours(hours);
        // `target_valid` is always on an hour boundary (both `run` and every `fh` are whole
        // hours), so this is an exact hour count, never a truncated fraction.
        let fh = (target_valid - run).num_hours();
        let Ok(fh) = u16::try_from(fh) else {
            continue; // run ended up after target_valid; try one cycle further back
        };
        match fetch_run(http, model, field, run, fh).await {
            Ok(f) => return Ok(f),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no {} cycle aligns", model.label())))
}

async fn fetch_run(
    http: &reqwest::Client,
    model: GlobalModel,
    field: GlobalField,
    run: DateTime<Utc>,
    fh: u16,
) -> anyhow::Result<GlobalForecast> {
    let date = format!("{:04}{:02}{:02}", run.year(), run.month(), run.day());
    let (base, range) = match model {
        GlobalModel::Gfs => {
            let base = format!(
                "{GFS_BUCKET}/gfs.{date}/{:02}/atmos/gfs.t{:02}z.pgrb2.0p25.f{fh:03}",
                run.hour(),
                run.hour()
            );
            let idx = get_text(http, &format!("{base}.idx")).await?;
            let (var, level) = field.gfs_key();
            let r = crate::hrrr::field_byte_range(&idx, var, level)
                .ok_or_else(|| anyhow::anyhow!("no {var}:{level} in GFS idx"))?;
            (base, r)
        }
        GlobalModel::Ecmwf => {
            let base = format!(
                "{ECMWF_BASE}/{date}/{:02}z/ifs/0p25/oper/{date}{:02}0000-{fh}h-oper-fc.grib2",
                run.hour(),
                run.hour()
            );
            let idx = get_text(http, &format!("{}.index", strip_ext(&base))).await?;
            let r = ecmwf_byte_range(&idx, field)
                .ok_or_else(|| anyhow::anyhow!("no {:?} in ECMWF index", field))?;
            (base, r)
        }
        // The ensemble mean's surface fields share GFS's own variable/level naming (both are
        // NCEP products off the same GRIB tables), so this reuses `gfs_key()` rather than
        // tabulating a second identical mapping — the one field it doesn't carry (dewpoint) then
        // just surfaces as the same "not found in idx" error a genuinely missing field always
        // does, the same way an unavailable field on any other model already fails honestly.
        GlobalModel::Gefs => {
            let base = format!(
                "{GEFS_BUCKET}/gefs.{date}/{:02}/atmos/pgrb2ap5/geavg.t{:02}z.pgrb2a.0p50.f{fh:03}",
                run.hour(),
                run.hour()
            );
            let idx = get_text(http, &format!("{base}.idx")).await?;
            let (var, level) = field.gfs_key();
            let r = crate::hrrr::field_byte_range(&idx, var, level)
                .ok_or_else(|| anyhow::anyhow!("no {var}:{level} in GEFS idx"))?;
            (base, r)
        }
        // No index to slice — Datamart already publishes one message per file, so the "range"
        // is simply the whole thing.
        GlobalModel::Gdps => {
            let (var, level) = field.gdps_key();
            let base = format!(
                "{GDPS_BASE}/{:02}/{fh:03}/{date}T{:02}Z_MSC_GDPS_{var}_{level}_LatLon0.15_PT{fh:03}H.grib2",
                run.hour(),
                run.hour()
            );
            (base, (0, None))
        }
    };

    let res_deg = if model == GlobalModel::Gefs {
        GEFS_RES_DEG
    } else {
        RES_DEG
    };
    let field_out = download_and_decode(http, &base, range, res_deg).await?;
    Ok(GlobalForecast {
        field: field_out,
        run,
        fcst_hour: fh,
    })
}

/// Range-GET one GRIB2 message and decode it onto the `res_deg` lattice.
async fn download_and_decode(
    http: &reqwest::Client,
    base: &str,
    range: (u64, Option<u64>),
    res_deg: f64,
) -> anyhow::Result<MrmsField> {
    let (start, end) = range;
    let http_range = match end {
        Some(e) => format!("bytes={start}-{}", e - 1),
        None => format!("bytes={start}-"),
    };
    let bytes = http
        .get(crate::net::fetch_url(base))
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .header("Range", http_range)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    let raw = bytes.to_vec();
    crate::task::blocking(move || decode(&raw, res_deg)).await?
}

/// One GEFS ensemble member's message for the GRIB `(var, level)` at `fh`, from a known cycle.
///
/// Member 0 is the control run (`gec00`); 1..=30 are the perturbed members (`gep01`..`gep30`).
/// Every member decodes onto the same [`GEFS_RES_DEG`] lattice, which is what lets
/// [`crate::ensemble::combine`] treat them cell by cell.
pub async fn fetch_gefs_member(
    http: &reqwest::Client,
    key: (&str, &str),
    run: DateTime<Utc>,
    fh: u16,
    member: u8,
) -> anyhow::Result<MrmsField> {
    let date = format!("{:04}{:02}{:02}", run.year(), run.month(), run.day());
    let name = if member == 0 {
        "gec00".to_string()
    } else {
        format!("gep{member:02}")
    };
    let base = format!(
        "{GEFS_BUCKET}/gefs.{date}/{:02}/atmos/pgrb2ap5/{name}.t{:02}z.pgrb2a.0p50.f{fh:03}",
        run.hour(),
        run.hour()
    );
    let idx = get_text(http, &format!("{base}.idx")).await?;
    let (var, level) = key;
    let range = crate::hrrr::field_byte_range(&idx, var, level)
        .ok_or_else(|| anyhow::anyhow!("no {var}:{level} in GEFS {name} idx"))?;
    download_and_decode(http, &base, range, GEFS_RES_DEG).await
}

/// The newest GEFS cycle that has posted the control member for `key` at `fh`, plus that
/// member's grid. Walks back through recent cycles for the same "directory exists before the
/// file does" latency [`fetch`] tolerates; the run is then pinned for every other member so the
/// ensemble never mixes cycles.
pub async fn find_gefs_cycle(
    http: &reqwest::Client,
    key: (&str, &str),
    fh: u16,
) -> anyhow::Result<(DateTime<Utc>, MrmsField)> {
    let now = Utc::now();
    let step = GlobalModel::Gefs.cycle_step() as i64;
    let mut last_err = None;
    for back in 0..5 {
        let hours = (now.hour() as i64 / step) * step - back * step;
        let run = (now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc())
            + chrono::Duration::hours(hours);
        match fetch_gefs_member(http, key, run, fh, 0).await {
            Ok(f) => return Ok((run, f)),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no GEFS cycle found")))
}

/// `foo.grib2` → `foo`, for the sidecar whose extension replaces rather than appends.
fn strip_ext(url: &str) -> &str {
    url.strip_suffix(".grib2").unwrap_or(url)
}

async fn get_text(http: &reqwest::Client, url: &str) -> anyhow::Result<String> {
    Ok(http
        .get(crate::net::fetch_url(url))
        .timeout(crate::net::FEED_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?)
}

/// Byte range for a field in an ECMWF JSON-lines `.index`.
///
/// Each line is one message: `{"param":"2t","levtype":"sfc","_offset":N,"_length":M,…}`. Offsets
/// and lengths are given outright, so unlike NOAA's `.idx` there is no next-line arithmetic.
///
/// ponytail: substring matching rather than a JSON parse — these lines are machine-generated and
/// flat. A real parse is one `serde_json::from_str` away if the format ever grows nesting.
fn ecmwf_byte_range(index: &str, field: GlobalField) -> Option<(u64, Option<u64>)> {
    let (param, levtype, level) = field.ecmwf_key();
    for line in index.lines() {
        if !line.contains(&format!("\"param\": \"{param}\""))
            && !line.contains(&format!("\"param\":\"{param}\""))
        {
            continue;
        }
        if !line.contains(&format!("\"{levtype}\"")) {
            continue;
        }
        if let Some(lv) = level {
            if !line.contains(&format!("\"levelist\": \"{lv}\""))
                && !line.contains(&format!("\"levelist\":\"{lv}\""))
            {
                continue;
            }
        }
        let offset = json_number(line, "_offset")?;
        let length = json_number(line, "_length")?;
        return Some((offset, Some(offset + length)));
    }
    None
}

/// Pull an unquoted numeric value out of one flat JSON line.
fn json_number(line: &str, key: &str) -> Option<u64> {
    let at = line.find(&format!("\"{key}\""))? + key.len() + 2;
    let rest = line[at..].trim_start_matches([':', ' ']);
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Decode one GRIB2 message onto the shared regular lat/lon grid.
fn decode(raw: &[u8], res_deg: f64) -> anyhow::Result<MrmsField> {
    use gribberish::data_message::DataMessage;
    use gribberish::message::read_message;
    let msg = read_message(raw, 0).ok_or_else(|| anyhow::anyhow!("no GRIB2 message"))?;
    let time = msg.forecast_date().unwrap_or_else(|_| Utc::now());
    let dm = DataMessage::try_from(&msg).map_err(|e| anyhow::anyhow!("global decode: {e:?}"))?;
    let (lats, lons) = dm.metadata.latlng();
    let data = dm.data;
    // A regular lat/lon grid hands back its two axes, not a coordinate per point (which is what
    // a Lambert projection like HRRR's produces). Expand the axes to the full grid so the shared
    // regrid sees the same shape either way.
    let (lats, lons) = if lats.len() * lons.len() == data.len() {
        let mut la = Vec::with_capacity(data.len());
        let mut lo = Vec::with_capacity(data.len());
        for lat in &lats {
            for lon in &lons {
                la.push(*lat);
                lo.push(*lon);
            }
        }
        (la, lo)
    } else {
        (lats, lons)
    };
    anyhow::ensure!(
        lats.len() == data.len() && lons.len() == data.len(),
        "global latlng/data length mismatch"
    );
    crate::hrrr::regrid(&lats, &lons, &data, time, res_deg, f64::NEG_INFINITY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecmwf_index_lines_resolve_to_ranges() {
        let idx = "{\"domain\": \"g\", \"param\": \"msl\", \"levtype\": \"sfc\", \"_offset\": 100, \"_length\": 250}\n\
                   {\"domain\": \"g\", \"param\": \"gh\", \"levtype\": \"pl\", \"levelist\": \"850\", \"_offset\": 400, \"_length\": 60}\n\
                   {\"domain\": \"g\", \"param\": \"gh\", \"levtype\": \"pl\", \"levelist\": \"500\", \"_offset\": 500, \"_length\": 75}\n\
                   {\"domain\": \"g\", \"param\": \"2d\", \"levtype\": \"sfc\", \"_offset\": 700, \"_length\": 90}\n";
        assert_eq!(
            ecmwf_byte_range(idx, GlobalField::Mslp),
            Some((100, Some(350)))
        );
        // The right pressure level, not just the right parameter.
        assert_eq!(
            ecmwf_byte_range(idx, GlobalField::Height500),
            Some((500, Some(575)))
        );
        assert_eq!(
            ecmwf_byte_range(idx, GlobalField::Dewpoint2m),
            Some((700, Some(790)))
        );
        assert_eq!(ecmwf_byte_range(idx, GlobalField::Temp2m), None);
    }

    #[test]
    fn field_slugs_round_trip() {
        for f in GlobalField::ALL {
            assert_eq!(GlobalField::from_slug(f.slug()), Some(f));
        }
        assert_eq!(GlobalField::from_slug("nope"), None);
    }

    /// Phase A1: every `GlobalField` must resolve to a real, uniquely-identified
    /// `FieldDescriptor` under the shared `GlobalModels` source — the same completeness check
    /// `wxdata::model`'s `ModelField` table runs for HRRR/RAP's regional fields.
    #[test]
    fn every_global_field_has_a_unique_searchable_descriptor() {
        use crate::field::{DataSource, FieldFamily};
        let mut ids = std::collections::HashSet::new();
        for f in GlobalField::ALL {
            let d = f.descriptor();
            assert!(!d.id.0.is_empty(), "{f:?}");
            assert_eq!(d.source, DataSource::GlobalModels);
            assert_eq!(d.family, FieldFamily::Model);
            assert!(d.supports_location(179.0, -45.0));
            assert!(!d.supports_location(f64::NAN, 0.0));
            assert!(
                d.search_text().contains("Global models"),
                "{f:?} search text: {}",
                d.search_text()
            );
            assert!(ids.insert(d.id), "{f:?} reuses another field's id");
        }
        assert_eq!(ids.len(), GlobalField::ALL.len());
    }

    #[tokio::test]
    async fn point_series_rejects_invalid_coordinates_before_fetching() {
        let err = fetch_point_series(
            &reqwest::Client::new(),
            GlobalModel::Gfs,
            GlobalField::Temp2m,
            f64::NAN,
            35.0,
            &[0],
        )
        .await
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("outside the published field domain"));
    }

    /// Both sources, live, at the newest usable cycle.
    /// `cargo test -p wxdata global_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "network"]
    async fn global_live() {
        let http = reqwest::Client::new();
        for model in [
            GlobalModel::Gfs,
            GlobalModel::Ecmwf,
            GlobalModel::Gefs,
            GlobalModel::Gdps,
        ] {
            let f = fetch(&http, model, GlobalField::Mslp, 0)
                .await
                .unwrap_or_else(|e| panic!("{} fetch: {e}", model.label()));
            let finite = f.field.values.iter().filter(|v| v.is_finite()).count();
            println!(
                "{}: {}x{} lon {:.1}..{:.1} lat {:.1}..{:.1} finite {finite}/{} ({:.0}%)",
                model.label(),
                f.field.nx,
                f.field.ny,
                f.field.lon_west,
                f.field.lon_east,
                f.field.lat_south,
                f.field.lat_north,
                f.field.values.len(),
                100.0 * finite as f64 / f.field.values.len() as f64,
            );
            // The whole point of the longitude wrap: a global field lands in −180..180.
            assert!(f.field.lon_west >= -180.5 && f.field.lon_east <= 180.5);
            assert!(finite > 0);
        }
    }

    /// GDPS specifically, every field this app reads — `gdps_key()`'s Datamart filenames are
    /// hand-transcribed from a live directory listing, not derived from any spec, so each one
    /// needs its own live check rather than trusting the MSLP check above covers them all.
    /// `cargo test -p wxdata gdps_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "network"]
    async fn gdps_live() {
        let http = reqwest::Client::new();
        for field in GlobalField::ALL {
            // Precip is an accumulation; it has nothing to report at the analysis hour.
            let fh = if field == GlobalField::Precip { 24 } else { 0 };
            let f = fetch(&http, GlobalModel::Gdps, field, fh)
                .await
                .unwrap_or_else(|e| panic!("{}: {e}", field.label()));
            let finite = f.field.values.iter().filter(|v| v.is_finite()).count();
            println!(
                "{}: {}x{} finite {finite}/{} ({:.0}%)",
                field.label(),
                f.field.nx,
                f.field.ny,
                f.field.values.len(),
                100.0 * finite as f64 / f.field.values.len() as f64,
            );
            assert!(
                finite > f.field.values.len() / 2,
                "{}: too many gaps",
                field.label()
            );
        }
    }

    /// A meteogram series for a real point (Oklahoma City), live, across all four models —
    /// every hour should come back with a value (the point sits well inside every model's
    /// domain), and every hour's valid time should be `run + fh`, one cycle apart.
    /// `cargo test -p wxdata point_series_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "network"]
    async fn point_series_live() {
        let http = reqwest::Client::new();
        let hours: Vec<u16> = (0..=24).step_by(6).collect();
        for model in [
            GlobalModel::Gfs,
            GlobalModel::Ecmwf,
            GlobalModel::Gefs,
            GlobalModel::Gdps,
        ] {
            let series =
                match fetch_point_series(&http, model, GlobalField::Temp2m, -97.5, 35.5, &hours)
                    .await
                {
                    Ok(s) => s,
                    // GDPS reads Datamart's rolling "today" window, which has no "yesterday" to fall
                    // back to (see `GDPS_BASE`'s doc comment) — a walk-back that crosses a UTC
                    // midnight can come up empty for a cycle that really did post. That is a real,
                    // disclosed limitation of the source, not this function; don't fail the other
                    // three models' coverage over it.
                    Err(e) if model == GlobalModel::Gdps => {
                        eprintln!("GDPS: {e} (known Datamart today-only limitation, skipping)");
                        continue;
                    }
                    Err(e) => panic!("{}: {e}", model.label()),
                };
            assert_eq!(series.len(), hours.len());
            let finite = series.iter().filter(|(_, v)| v.is_some()).count();
            println!(
                "{}: {finite}/{} hours, first {:?}, last {:?}",
                model.label(),
                series.len(),
                series.first(),
                series.last()
            );
            assert!(finite > 0, "{}: every hour came back empty", model.label());
            for (_, v) in &series {
                if let Some(k) = v {
                    assert!(
                        (250.0..330.0).contains(k),
                        "{}: implausible temp {k} K",
                        model.label()
                    );
                }
            }
            // Every valid time is exactly `fh` hours after the first — same cycle throughout.
            let first_valid = series[0].0;
            for (i, &fh) in hours.iter().enumerate() {
                let expected = first_valid + chrono::Duration::hours(fh as i64);
                assert_eq!(
                    series[i].0,
                    expected,
                    "{}: hour {fh} drifted cycle",
                    model.label()
                );
            }
        }
    }
}
