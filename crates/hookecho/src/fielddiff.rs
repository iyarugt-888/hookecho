//! Subtract one model's field from another's.
//!
//! Two models disagreeing is the forecast information — a 6 °C spread over the warm sector says
//! more about tomorrow than either model's own number does. Everything the difference layer needs
//! beyond the ordinary field path lives here: resample the two grids onto one lattice, subtract,
//! and hand back an [`MrmsField`] the existing upload/draw code cannot tell from any other.
//!
//! ponytail: the coarser grid wins, rather than interpolating both onto something finer. A
//! difference is never sharper than its blurriest input, and pretending otherwise costs memory to
//! draw detail that is not there. GFS and ECMWF already share one lattice, so that pair does not
//! resample at all.

use chrono::{DateTime, Duration, Utc};
use wxdata::global::GlobalField;
use wxdata::mrms::MrmsField;

/// One exact valid time, with the source run and lead retained for each model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComparisonTimes {
    pub valid: DateTime<Utc>,
    pub a_run: DateTime<Utc>,
    pub a_lead_hours: u16,
    pub b_run: DateTime<Utc>,
    pub b_lead_hours: u16,
}

impl ComparisonTimes {
    pub fn new(
        a_run: DateTime<Utc>,
        a_lead_hours: u16,
        b_run: DateTime<Utc>,
        b_lead_hours: u16,
    ) -> anyhow::Result<Self> {
        let a_valid = a_run + Duration::hours(i64::from(a_lead_hours));
        let b_valid = b_run + Duration::hours(i64::from(b_lead_hours));
        anyhow::ensure!(
            a_valid == b_valid,
            "model valid times differ: {a_valid} vs {b_valid}"
        );
        Ok(Self {
            valid: a_valid,
            a_run,
            a_lead_hours,
            b_run,
            b_lead_hours,
        })
    }

    pub fn label(self, a: &str, b: &str) -> String {
        format!(
            "Both valid {} · {a} run {} +{}h · {b} run {} +{}h",
            self.valid.format("%Y-%m-%d %H:%MZ"),
            self.a_run.format("%Y-%m-%d %H:%MZ"),
            self.a_lead_hours,
            self.b_run.format("%Y-%m-%d %H:%MZ"),
            self.b_lead_hours
        )
    }
}

pub struct ComparisonPair {
    pub a: MrmsField,
    pub b: MrmsField,
    pub times: ComparisonTimes,
}

/// How the already-computed `A - B` field is presented. Switching modes is a display operation:
/// the signed CPU grid remains authoritative, so the app can rebuild the upload without fetching
/// either model again and cursor readouts can still explain exactly what was transformed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DiffMode {
    #[default]
    Signed,
    Absolute,
    /// Binary analyst scan: transparent within the field's deadband, one color outside it.
    Disagreement,
}

impl DiffMode {
    pub const ALL: [Self; 3] = [Self::Signed, Self::Absolute, Self::Disagreement];

    pub fn label(self) -> &'static str {
        match self {
            Self::Signed => "Signed (A − B)",
            Self::Absolute => "Absolute |A − B|",
            Self::Disagreement => "Disagreement mask",
        }
    }

    pub fn apply(self, value: f32) -> f32 {
        match self {
            Self::Signed => value,
            Self::Absolute | Self::Disagreement => value.abs(),
        }
    }

    pub fn expression(self, a: &str, b: &str) -> String {
        match self {
            Self::Signed => format!("{a} − {b}"),
            Self::Absolute => format!("|{a} − {b}|"),
            Self::Disagreement => format!("{a} ≉ {b}"),
        }
    }
}

/// A resident GPU grid is drawable only while its metadata belongs to the current selection.
pub fn layer_ready(
    layer: crate::render::FieldLayer,
    diff: Option<ComparisonTimes>,
    compare: Option<ComparisonTimes>,
) -> bool {
    use crate::render::FieldLayer as FL;
    match layer {
        FL::ModelDiff => diff.is_some(),
        FL::CompareA | FL::CompareB => compare.is_some(),
        _ => true,
    }
}

fn verify_field_valid(
    model: &str,
    field: &MrmsField,
    expected: DateTime<Utc>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        field.time == expected,
        "{model} GRIB valid time {} differs from requested {expected}",
        field.time
    );
    Ok(())
}

/// Fetch two models at one exact valid time. If the first model's newest cycle cannot be paired,
/// try the second model's newest cycle as the anchor. Never subtract grids from different times.
pub async fn fetch_pair(
    http: &reqwest::Client,
    field: DiffField,
    fh: u16,
) -> anyhow::Result<ComparisonPair> {
    use wxdata::global::{GlobalField, GlobalModel};
    match field {
        DiffField::Global(GlobalFieldKind::Precip) => {
            anyhow::bail!(
                "GFS precipitable water and ECMWF total precipitation are different quantities"
            )
        }
        DiffField::Global(kind) => {
            let g: GlobalField = kind.into();
            let (gfs, ecmwf) = futures_util::future::try_join(
                wxdata::global::fetch(http, GlobalModel::Gfs, g, fh),
                wxdata::global::fetch(http, GlobalModel::Ecmwf, g, fh),
            )
            .await?;
            let (gfs, ecmwf) = if gfs.valid() == ecmwf.valid() {
                (gfs, ecmwf)
            } else {
                match wxdata::global::fetch_aligned(http, GlobalModel::Ecmwf, g, gfs.valid()).await
                {
                    Ok(aligned) => (gfs, aligned),
                    Err(_) => {
                        let target = ecmwf.valid();
                        let aligned =
                            wxdata::global::fetch_aligned(http, GlobalModel::Gfs, g, target)
                                .await
                                .map_err(|err| {
                                    anyhow::anyhow!(
                                        "no GFS/ECMWF pair shares valid time {target}: {err}"
                                    )
                                })?;
                        (aligned, ecmwf)
                    }
                }
            };
            let times = ComparisonTimes::new(gfs.run, gfs.fcst_hour, ecmwf.run, ecmwf.fcst_hour)?;
            verify_field_valid("GFS", &gfs.field, times.valid)?;
            verify_field_valid("ECMWF", &ecmwf.field, times.valid)?;
            Ok(ComparisonPair {
                a: gfs.field,
                b: ecmwf.field,
                times,
            })
        }
        DiffField::Cape | DiffField::Srh => {
            use wxdata::hrrr::Model;
            use wxdata::model::ModelField;
            // Phase F1: HRRR and RAP spell these identically, so one key serves both — but that
            // is a fact the catalogue now records rather than an assumption this call site makes.
            // A third model in this comparison would need no change here.
            let mf = match field {
                DiffField::Srh => ModelField::Srh3km,
                _ => ModelField::SurfaceCape,
            };
            let key_for = |m: Model| {
                mf.grib(m)
                    .ok_or_else(|| anyhow::anyhow!("{} does not publish {}", m.label(), mf.label()))
            };
            let (hrrr_key, rap_key) = (key_for(Model::Hrrr)?, key_for(Model::Rap)?);
            anyhow::ensure!(
                hrrr_key == rap_key,
                "HRRR and RAP spell {} differently; this comparison assumes one key",
                mf.label()
            );
            let (var, level, min_valid) = (hrrr_key.var, hrrr_key.level, hrrr_key.min_valid);
            let (hrrr, rap) = futures_util::future::try_join(
                wxdata::hrrr::fetch_field(http, Model::Hrrr, var, level, 0, min_valid),
                wxdata::hrrr::fetch_field(http, Model::Rap, var, level, 0, min_valid),
            )
            .await?;
            let (hrrr, rap) = if hrrr.valid() == rap.valid() {
                (hrrr, rap)
            } else {
                match wxdata::hrrr::fetch_field_aligned(
                    http,
                    Model::Rap,
                    var,
                    level,
                    hrrr.valid(),
                    min_valid,
                )
                .await
                {
                    Ok(aligned) => (hrrr, aligned),
                    Err(_) => {
                        let target = rap.valid();
                        let aligned = wxdata::hrrr::fetch_field_aligned(
                            http,
                            Model::Hrrr,
                            var,
                            level,
                            target,
                            min_valid,
                        )
                        .await
                        .map_err(|err| {
                            anyhow::anyhow!("no HRRR/RAP pair shares valid time {target}: {err}")
                        })?;
                        (aligned, rap)
                    }
                }
            };
            let times = ComparisonTimes::new(
                hrrr.run,
                u16::from(hrrr.fcst_hour),
                rap.run,
                u16::from(rap.fcst_hour),
            )?;
            verify_field_valid("HRRR", &hrrr.field, times.valid)?;
            verify_field_valid("RAP", &rap.field, times.valid)?;
            Ok(ComparisonPair {
                a: hrrr.field,
                b: rap.field,
                times,
            })
        }
        DiffField::RunToRunCape => {
            use wxdata::hrrr::Model;
            use wxdata::model::ModelField;
            // ROADMAP_NEW F5: current HRRR run minus the previous one, at the analysis hour
            // (lead 0) — the same fixed-lead choice the HRRR/RAP comparison above makes, and for
            // the same reason: a larger lead would need the previous cycle to still be within its
            // own 18 h publish window for the same valid time, which isn't always true.
            let key = ModelField::SurfaceCape
                .grib(Model::Hrrr)
                .ok_or_else(|| anyhow::anyhow!("HRRR does not publish surface CAPE"))?;
            let (var, level, min_valid) = (key.var, key.level, key.min_valid);
            let current = wxdata::hrrr::fetch_field(http, Model::Hrrr, var, level, 0, min_valid)
                .await
                .map_err(|err| anyhow::anyhow!("no current HRRR cycle: {err}"))?;
            let previous = wxdata::hrrr::fetch_field_previous_run(
                http,
                Model::Hrrr,
                var,
                level,
                current.run,
                current.valid(),
                min_valid,
            )
            .await
            .map_err(|err| anyhow::anyhow!("no earlier HRRR cycle to compare against: {err}"))?;
            let times = ComparisonTimes::new(
                current.run,
                u16::from(current.fcst_hour),
                previous.run,
                u16::from(previous.fcst_hour),
            )?;
            verify_field_valid("current HRRR run", &current.field, times.valid)?;
            verify_field_valid("previous HRRR run", &previous.field, times.valid)?;
            Ok(ComparisonPair {
                a: current.field,
                b: previous.field,
                times,
            })
        }
    }
}

/// What the difference layer is differencing, and therefore which two models it asks for.
///
/// ponytail: a fixed pair per field rather than two free model pickers. These are the two
/// comparisons forecasters actually make — global against global for the synoptic pattern, and
/// the two convection-scale models against each other — and each extra picker is a way to ask
/// for a pair that has no shared valid time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DiffField {
    /// GFS − ECMWF, on the shared 0.3° lattice.
    Global(GlobalFieldKind),
    /// HRRR − RAP surface CAPE.
    Cape,
    /// HRRR − RAP storm-relative helicity.
    Srh,
    /// ROADMAP_NEW F5: HRRR's current run minus its own previous run, both at the analysis hour
    /// (lead 0) for the same valid time — how much the model's own initial state has changed
    /// cycle to cycle, not a disagreement between two different models.
    RunToRunCape,
}

/// `GlobalField` again, because that one is not `Hash`/`Serialize` and this is used as a settings
/// value and a map key. Converts both ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum GlobalFieldKind {
    Mslp,
    Height500,
    Temp2m,
    Dewpoint2m,
    Wind10m,
    Precip,
}

impl From<GlobalFieldKind> for GlobalField {
    fn from(k: GlobalFieldKind) -> GlobalField {
        match k {
            GlobalFieldKind::Mslp => GlobalField::Mslp,
            GlobalFieldKind::Height500 => GlobalField::Height500,
            GlobalFieldKind::Temp2m => GlobalField::Temp2m,
            GlobalFieldKind::Dewpoint2m => GlobalField::Dewpoint2m,
            GlobalFieldKind::Wind10m => GlobalField::Wind10m,
            GlobalFieldKind::Precip => GlobalField::Precip,
        }
    }
}

impl Default for DiffField {
    fn default() -> Self {
        DiffField::Global(GlobalFieldKind::Mslp)
    }
}

impl DiffField {
    /// Moisture is here as 2 m dewpoint: both models publish it as the same quantity in the same
    /// units, so the subtraction means something. Column moisture still is not — GFS publishes
    /// precipitable water and ECMWF total precipitation, and subtracting them subtracts two
    /// different things. A plausible looking map of nonsense is worse than no map.
    pub const ALL: [DiffField; 8] = [
        DiffField::Global(GlobalFieldKind::Mslp),
        DiffField::Global(GlobalFieldKind::Height500),
        DiffField::Global(GlobalFieldKind::Temp2m),
        DiffField::Global(GlobalFieldKind::Dewpoint2m),
        DiffField::Global(GlobalFieldKind::Wind10m),
        DiffField::Cape,
        DiffField::Srh,
        DiffField::RunToRunCape,
    ];

    /// Native units → display units, the same conversion the single-model ramps apply. Grids
    /// arrive as the model published them: pressure in Pa, height in m, wind in m/s.
    pub fn input_scale(self) -> f32 {
        match self {
            DiffField::Global(GlobalFieldKind::Mslp) => 0.01, // Pa → hPa
            DiffField::Global(GlobalFieldKind::Height500) => 0.1, // m → dam
            DiffField::Global(GlobalFieldKind::Wind10m) => 1.943_844, // m/s → kt
            // A difference of two Kelvin fields is already a difference in °C.
            _ => 1.0,
        }
    }

    /// Which two models, in the order they are subtracted.
    pub fn pair(self) -> (&'static str, &'static str) {
        match self {
            DiffField::Global(_) => ("GFS", "ECMWF"),
            DiffField::Cape | DiffField::Srh => ("HRRR", "RAP"),
            // Distinct labels even though it's one model: "HRRR minus HRRR" would read as a
            // typo, not "the same model's own initial state one cycle apart".
            DiffField::RunToRunCape => ("HRRR (latest)", "HRRR (previous)"),
        }
    }

    /// The "view side by side" compare-panes mode shows each side's own single-model layer
    /// unsubtracted — meaningful for two different models (`FieldLayer::GlobalMslp` genuinely is
    /// "GFS's own MSLP" and "ECMWF's own MSLP" in the two panes), but a run-to-run field has no
    /// distinct "previous run" layer of its own yet, so both panes would show today's current-run
    /// CAPE with nothing to tell them apart. Hidden rather than shipped half-working.
    pub fn supports_side_by_side(self) -> bool {
        !matches!(self, DiffField::RunToRunCape)
    }

    /// The single-model layer whose color scale represents this field's own physical units.
    ///
    /// The compare-panes mode shows each side's raw field, unsubtracted — one model's own MSLP
    /// is exactly what `FieldLayer::GlobalMslp` already draws, so it reuses that ramp rather than
    /// tabulating a second copy of it under a `CompareA`/`CompareB` key. Both panes read this same
    /// layer regardless of which side they're drawing: the field is identical, only the model
    /// providing the grid differs.
    pub fn source_layer(self) -> crate::render::FieldLayer {
        use crate::render::FieldLayer as FL;
        match self {
            DiffField::Global(GlobalFieldKind::Mslp) => FL::GlobalMslp,
            DiffField::Global(GlobalFieldKind::Height500) => FL::GlobalHeight500,
            DiffField::Global(GlobalFieldKind::Temp2m) => FL::GlobalTemp2m,
            DiffField::Global(GlobalFieldKind::Dewpoint2m) => FL::GlobalDewpoint2m,
            DiffField::Global(GlobalFieldKind::Wind10m) => FL::GlobalWind10m,
            DiffField::Global(GlobalFieldKind::Precip) => FL::GlobalPrecip,
            DiffField::Cape => FL::Cape,
            DiffField::Srh => FL::Srh,
            // Reused rather than a distinct layer: see `supports_side_by_side`'s doc comment on
            // why the side-by-side mode this feeds is hidden for this field anyway.
            DiffField::RunToRunCape => FL::Cape,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DiffField::Global(k) => GlobalField::from(k).label(),
            DiffField::Cape => "Surface CAPE",
            DiffField::Srh => "Storm-relative helicity",
            DiffField::RunToRunCape => "Surface CAPE (run to run)",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            DiffField::Global(k) => GlobalField::from(k).slug(),
            DiffField::Cape => "cape",
            DiffField::Srh => "srh",
            DiffField::RunToRunCape => "cape-run-to-run",
        }
    }

    /// Full-scale difference and the deadband inside which the two models count as agreeing, in
    /// the field's own units. Both are eyeballed from what a meaningful spread looks like — an
    /// 8 hPa MSLP split is a different low, a 200 J/kg CAPE split is noise.
    pub fn range(self) -> (f32, f32) {
        match self {
            DiffField::Global(GlobalFieldKind::Mslp) => (8.0, 0.5),
            DiffField::Global(GlobalFieldKind::Height500) => (12.0, 1.0),
            DiffField::Global(GlobalFieldKind::Temp2m) => (6.0, 0.5),
            DiffField::Global(GlobalFieldKind::Dewpoint2m) => (6.0, 0.5),
            DiffField::Global(GlobalFieldKind::Wind10m) => (20.0, 2.0),
            DiffField::Global(GlobalFieldKind::Precip) => (20.0, 1.0),
            DiffField::Cape => (1500.0, 200.0),
            DiffField::Srh => (150.0, 25.0),
            // Tighter than the cross-model CAPE range above: this is the same model's own
            // analysis one cycle apart, not two independent physics packages, so agreement is
            // the common case and a smaller swing is already worth flagging.
            DiffField::RunToRunCape => (800.0, 100.0),
        }
    }

    /// Units, for the legend and the hover text.
    pub fn units(self) -> &'static str {
        match self {
            DiffField::Global(GlobalFieldKind::Mslp) => "hPa",
            DiffField::Global(GlobalFieldKind::Height500) => "dam",
            DiffField::Global(GlobalFieldKind::Temp2m) => "°C",
            DiffField::Global(GlobalFieldKind::Dewpoint2m) => "°C",
            DiffField::Global(GlobalFieldKind::Wind10m) => "kt",
            DiffField::Global(GlobalFieldKind::Precip) => "mm",

            DiffField::Cape => "J/kg",
            DiffField::Srh => "m²/s²",
            DiffField::RunToRunCape => "J/kg",
        }
    }
}

/// `a - b`, on the coarser of the two lattices, over the part of the world both cover.
///
/// Rejects inputs with different valid times before sampling. `fetch_pair` also records both
/// source runs and leads; this guard protects direct callers of the pure subtraction.
pub fn diff(a: &MrmsField, b: &MrmsField) -> Option<MrmsField> {
    if a.time != b.time {
        return None;
    }
    let lon_west = a.lon_west.max(b.lon_west);
    let lon_east = a.lon_east.min(b.lon_east);
    let lat_south = a.lat_south.max(b.lat_south);
    let lat_north = a.lat_north.min(b.lat_north);
    if lon_east <= lon_west || lat_north <= lat_south {
        return None; // disjoint domains: HRRR over CONUS against a regional model elsewhere
    }

    // Cell size of each input, then the coarser one, then how many of those fit in the overlap.
    let step = |f: &MrmsField| {
        (
            (f.lon_east - f.lon_west) / f.nx.max(2).saturating_sub(1) as f64,
            (f.lat_north - f.lat_south) / f.ny.max(2).saturating_sub(1) as f64,
        )
    };
    let (adx, ady) = step(a);
    let (bdx, bdy) = step(b);
    let (dx, dy) = (adx.max(bdx), ady.max(bdy));
    if dx <= 0.0 || dy <= 0.0 {
        return None;
    }
    let nx = ((lon_east - lon_west) / dx).round() as usize + 1;
    let ny = ((lat_north - lat_south) / dy).round() as usize + 1;
    if nx < 2 || ny < 2 {
        return None;
    }

    let mut values = Vec::with_capacity(nx * ny);
    for row in 0..ny {
        let lat = lat_north - row as f64 * dy;
        for col in 0..nx {
            let lon = lon_west + col as f64 * dx;
            values.push(match (sample(a, lon, lat), sample(b, lon, lat)) {
                (Some(x), Some(y)) => x - y,
                // Either model missing here means there is no difference to state. NaN is what
                // the rest of the field pipeline already reads as "no data".
                _ => f32::NAN,
            });
        }
    }
    Some(MrmsField {
        values,
        nx,
        ny,
        lon_west,
        lon_east: lon_west + (nx - 1) as f64 * dx,
        lat_north,
        lat_south: lat_north - (ny - 1) as f64 * dy,
        time: a.time,
    })
}

/// Bilinear sample at a lat/lon, or `None` outside the grid or against missing data.
fn sample(f: &MrmsField, lon: f64, lat: f64) -> Option<f32> {
    if f.nx < 2 || f.ny < 2 {
        return None;
    }
    let dx = (f.lon_east - f.lon_west) / (f.nx - 1) as f64;
    let dy = (f.lat_north - f.lat_south) / (f.ny - 1) as f64;
    if dx <= 0.0 || dy <= 0.0 {
        return None;
    }
    // Row 0 is the northernmost latitude, so y counts downward from lat_north.
    let x = (lon - f.lon_west) / dx;
    let y = (f.lat_north - lat) / dy;
    if x < 0.0 || y < 0.0 || x > (f.nx - 1) as f64 || y > (f.ny - 1) as f64 {
        return None;
    }
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(f.nx - 1), (y0 + 1).min(f.ny - 1));
    let (tx, ty) = ((x - x0 as f64) as f32, (y - y0 as f64) as f32);
    let at = |r: usize, c: usize| {
        let v = f.values[r * f.nx + c];
        if v.is_finite() {
            Some(v)
        } else {
            None
        }
    };
    // One missing corner poisons the cell rather than being treated as zero — a hole in a model
    // field is not a value of zero, and a difference against zero is a fabricated gradient.
    let (v00, v01, v10, v11) = (at(y0, x0)?, at(y0, x1)?, at(y1, x0)?, at(y1, x1)?);
    let top = v00 + (v01 - v00) * tx;
    let bottom = v10 + (v11 - v10) * tx;
    Some(top + (bottom - top) * ty)
}

/// Blue-white-red across ±`range`, with everything inside `deadband` fully transparent.
///
/// The deadband is the point of the layer: models agreeing is the common case and drawing it
/// would bury the disagreement under a wash of near-white. 256 entries, RGBA, index 128 = zero —
/// the same 256×1 LUT shape every other field layer uploads.
pub fn diverging_lut(range: f32, deadband: f32) -> Vec<u8> {
    let mut lut = Vec::with_capacity(256 * 4);
    for i in 0..256 {
        let t = (i as f32 / 255.0) * 2.0 - 1.0; // −1..1
        let v = t * range;
        if v.abs() <= deadband {
            lut.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        // Ramp opacity in from the deadband edge so the field has no hard rim around agreement.
        let mag = ((v.abs() - deadband) / (range - deadband).max(1e-6)).clamp(0.0, 1.0);
        let alpha = (60.0 + 195.0 * mag) as u8;
        let (r, g, b) = if t < 0.0 {
            // b's value is higher: cool.
            (
                (255.0 * (1.0 - mag)) as u8,
                (255.0 * (1.0 - 0.45 * mag)) as u8,
                255,
            )
        } else {
            (
                255,
                (255.0 * (1.0 - 0.75 * mag)) as u8,
                (255.0 * (1.0 - mag)) as u8,
            )
        };
        lut.extend_from_slice(&[r, g, b, alpha]);
    }
    lut
}

/// Value → LUT index, symmetric about zero. `NaN` (no data on either side) maps to the deadband,
/// which the LUT draws as nothing.
pub fn diff_index(v: f32, range: f32) -> u8 {
    if !v.is_finite() {
        return 128;
    }
    (((v / range).clamp(-1.0, 1.0) + 1.0) * 127.5) as u8
}

/// Amber-to-red sequential scale for `|A - B|`, with the same transparent agreement deadband as
/// the signed view. Direction has intentionally disappeared, so opposite signed differences of
/// equal magnitude map to the exact same index and color.
pub fn magnitude_lut(range: f32, deadband: f32) -> Vec<u8> {
    let mut lut = Vec::with_capacity(256 * 4);
    for i in 0..256 {
        let value = i as f32 / 255.0 * range;
        if value <= deadband {
            lut.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let mag = ((value - deadband) / (range - deadband).max(1e-6)).clamp(0.0, 1.0);
        let alpha = (60.0 + 195.0 * mag) as u8;
        let r = (255.0 - 55.0 * mag) as u8;
        let g = (220.0 - 195.0 * mag) as u8;
        let b = (90.0 - 15.0 * mag) as u8;
        lut.extend_from_slice(&[r, g, b, alpha]);
    }
    lut
}

/// Absolute magnitude → sequential LUT index. Missing data maps to transparent index zero.
pub fn magnitude_index(v: f32, range: f32) -> u8 {
    if !v.is_finite() {
        return 0;
    }
    (v.abs() / range).clamp(0.0, 1.0).mul_add(255.0, 0.0) as u8
}

/// Two-slot categorical palette: index zero is agreement/missing, index 255 is disagreement.
/// Keeping classification in [`display_index`] avoids shifting a scientific threshold by half a
/// quantized LUT bin near the deadband edge.
pub fn disagreement_lut() -> Vec<u8> {
    let mut lut = vec![0u8; 256 * 4];
    for i in 1..256 {
        lut[i * 4..i * 4 + 4].copy_from_slice(&[236, 78, 188, 220]);
    }
    lut
}

pub fn display_lut(mode: DiffMode, range: f32, deadband: f32) -> Vec<u8> {
    match mode {
        DiffMode::Signed => diverging_lut(range, deadband),
        DiffMode::Absolute => magnitude_lut(range, deadband),
        DiffMode::Disagreement => disagreement_lut(),
    }
}

pub fn display_index(mode: DiffMode, value: f32, range: f32, deadband: f32) -> u8 {
    match mode {
        DiffMode::Signed => diff_index(value, range),
        DiffMode::Absolute => magnitude_index(value, range),
        DiffMode::Disagreement => u8::MAX * u8::from(value.is_finite() && value.abs() > deadband),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_requires_one_valid_time_and_retains_both_runs() {
        let a_run = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let b_run = a_run - Duration::hours(1);
        let times = ComparisonTimes::new(a_run, 0, b_run, 1).unwrap();
        assert_eq!(times.valid, a_run);
        assert_eq!(times.b_lead_hours, 1);
        assert!(times.label("HRRR", "RAP").contains("RAP run"));
        assert!(ComparisonTimes::new(a_run, 0, b_run, 0).is_err());
        assert!(!layer_ready(
            crate::render::FieldLayer::ModelDiff,
            None,
            None
        ));
        assert!(layer_ready(
            crate::render::FieldLayer::ModelDiff,
            Some(times),
            None
        ));
        assert!(!layer_ready(
            crate::render::FieldLayer::CompareA,
            Some(times),
            None
        ));

        let mut field = grid(2, 2, -100.0, -99.0, 30.0, 31.0, 1.0);
        field.time = times.valid;
        assert!(verify_field_valid("HRRR", &field, times.valid).is_ok());
        field.time -= Duration::hours(1);
        assert!(verify_field_valid("HRRR", &field, times.valid).is_err());
    }

    fn grid(
        nx: usize,
        ny: usize,
        west: f64,
        east: f64,
        south: f64,
        north: f64,
        v: f32,
    ) -> MrmsField {
        MrmsField {
            values: vec![v; nx * ny],
            nx,
            ny,
            lon_west: west,
            lon_east: east,
            lat_north: north,
            lat_south: south,
            time: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        }
    }

    #[test]
    fn identical_lattices_subtract_cell_by_cell() {
        let a = grid(11, 11, -100.0, -90.0, 30.0, 40.0, 8.0);
        let b = grid(11, 11, -100.0, -90.0, 30.0, 40.0, 5.0);
        let d = diff(&a, &b).expect("overlapping");
        assert_eq!((d.nx, d.ny), (11, 11));
        assert!(d.values.iter().all(|v| (v - 3.0).abs() < 1e-4));
    }

    #[test]
    fn different_valid_times_cannot_be_subtracted() {
        let a = grid(2, 2, -100.0, -99.0, 30.0, 31.0, 8.0);
        let mut b = grid(2, 2, -100.0, -99.0, 30.0, 31.0, 5.0);
        b.time += Duration::hours(1);
        assert!(diff(&a, &b).is_none());
    }

    #[test]
    fn the_coarser_lattice_wins_and_the_overlap_clips() {
        // Fine grid over half the domain of a coarse one.
        let fine = grid(101, 101, -100.0, -95.0, 30.0, 35.0, 10.0);
        let coarse = grid(11, 11, -100.0, -90.0, 30.0, 40.0, 4.0);
        let d = diff(&fine, &coarse).expect("overlapping");
        assert_eq!((d.lon_west, d.lon_east), (-100.0, -95.0));
        assert_eq!((d.lat_south, d.lat_north), (30.0, 35.0));
        // 1° cells from the coarse grid across a 5° overlap.
        assert_eq!((d.nx, d.ny), (6, 6));
        assert!(d.values.iter().all(|v| (v - 6.0).abs() < 1e-4));
    }

    #[test]
    fn disjoint_domains_produce_nothing() {
        let a = grid(11, 11, -100.0, -90.0, 30.0, 40.0, 1.0);
        let b = grid(11, 11, 10.0, 20.0, 30.0, 40.0, 1.0);
        assert!(diff(&a, &b).is_none());
    }

    #[test]
    fn a_hole_in_either_model_is_a_hole_in_the_difference() {
        let mut a = grid(11, 11, -100.0, -90.0, 30.0, 40.0, 8.0);
        a.values[0] = f32::NAN;
        let b = grid(11, 11, -100.0, -90.0, 30.0, 40.0, 5.0);
        let d = diff(&a, &b).unwrap();
        assert!(d.values[0].is_nan());
        assert!((d.values[d.values.len() - 1] - 3.0).abs() < 1e-4);
    }

    #[test]
    fn agreement_draws_nothing_and_disagreement_ramps() {
        let lut = diverging_lut(10.0, 1.0);
        let alpha = |v: f32| lut[diff_index(v, 10.0) as usize * 4 + 3];
        assert_eq!(alpha(0.0), 0, "models agreeing is invisible");
        assert_eq!(alpha(0.5), 0, "inside the deadband is invisible");
        assert!(
            alpha(5.0) > 0 && alpha(10.0) > alpha(5.0),
            "further apart, more opaque"
        );
        // Sign picks the side of the ramp: blue for negative, red for positive.
        let rgb = |v: f32| {
            let i = diff_index(v, 10.0) as usize * 4;
            (lut[i], lut[i + 2])
        };
        assert!(rgb(-9.0).1 > rgb(-9.0).0, "negative is blue");
        assert!(rgb(9.0).0 > rgb(9.0).1, "positive is red");
    }

    #[test]
    fn absolute_mode_erases_direction_but_preserves_missing_and_deadband() {
        let mode = DiffMode::Absolute;
        let lut = display_lut(mode, 10.0, 1.0);
        let rgba = |v: f32| {
            let i = display_index(mode, v, 10.0, 1.0) as usize * 4;
            [lut[i], lut[i + 1], lut[i + 2], lut[i + 3]]
        };
        assert_eq!(mode.apply(-6.0), 6.0);
        assert_eq!(mode.expression("A", "B"), "|A − B|");
        assert_eq!(rgba(-6.0), rgba(6.0), "equal magnitudes need one color");
        assert_eq!(rgba(0.5)[3], 0, "agreement stays transparent");
        assert_eq!(rgba(f32::NAN)[3], 0, "missing data stays transparent");
        assert!(rgba(10.0)[3] > rgba(5.0)[3], "larger split is more opaque");
    }

    #[test]
    fn disagreement_mask_is_binary_directionless_and_uses_the_field_deadband() {
        let mode = DiffMode::Disagreement;
        let lut = display_lut(mode, 10.0, 1.0);
        let rgba = |v: f32| {
            let i = display_index(mode, v, 10.0, 1.0) as usize * 4;
            [lut[i], lut[i + 1], lut[i + 2], lut[i + 3]]
        };
        assert_eq!(mode.expression("A", "B"), "A ≉ B");
        assert_eq!(rgba(0.5)[3], 0, "agreement stays transparent");
        assert_eq!(rgba(1.0)[3], 0, "the deadband boundary is agreement");
        assert_eq!(rgba(f32::NAN)[3], 0, "missing data stays transparent");
        assert_eq!(rgba(-5.0), rgba(5.0), "direction must not change the class");
        assert_eq!(
            rgba(2.0),
            rgba(10.0),
            "all material disagreement is one class"
        );
        assert!(
            rgba(1.0001)[3] > 0,
            "just outside the deadband is disagreement"
        );
    }

    #[test]
    fn every_field_maps_to_its_own_single_model_layer() {
        use crate::render::field_ramps::ramp_for;
        use crate::render::FieldLayer as FL;
        let expected = [
            (DiffField::Global(GlobalFieldKind::Mslp), FL::GlobalMslp),
            (
                DiffField::Global(GlobalFieldKind::Height500),
                FL::GlobalHeight500,
            ),
            (DiffField::Global(GlobalFieldKind::Temp2m), FL::GlobalTemp2m),
            (
                DiffField::Global(GlobalFieldKind::Dewpoint2m),
                FL::GlobalDewpoint2m,
            ),
            (
                DiffField::Global(GlobalFieldKind::Wind10m),
                FL::GlobalWind10m,
            ),
            (DiffField::Cape, FL::Cape),
            (DiffField::Srh, FL::Srh),
            (DiffField::RunToRunCape, FL::Cape),
        ];
        // Every field the UI actually offers (`DiffField::ALL`) is covered above — this catches a
        // new entry added to one list and not the other.
        assert_eq!(expected.len(), DiffField::ALL.len());
        for (f, layer) in expected {
            assert_eq!(f.source_layer(), layer, "{f:?} mapped to the wrong layer");
            assert!(
                ramp_for(layer).is_some(),
                "{layer:?} must have a ramp to borrow"
            );
        }
    }

    /// ROADMAP_NEW F5: a run-to-run field compares one model against itself, so `pair()` must not
    /// return the same label twice ("HRRR minus HRRR" reads as a typo, not "one cycle apart"),
    /// and the side-by-side compare mode — which has no distinct "previous run" layer to show —
    /// must be turned off rather than silently drawing the same current-run layer in both panes.
    #[test]
    fn run_to_run_fields_get_distinct_pair_labels_and_no_side_by_side_mode() {
        let (a, b) = DiffField::RunToRunCape.pair();
        assert_ne!(a, b, "pair: {a:?} vs {b:?}");
        assert!(!DiffField::RunToRunCape.supports_side_by_side());
        // Every other field keeps the mode it already had.
        assert!(DiffField::Cape.supports_side_by_side());
        assert!(DiffField::Srh.supports_side_by_side());
        assert!(DiffField::Global(GlobalFieldKind::Mslp).supports_side_by_side());
    }

    /// The HRRR-vs-RAP comparison fetches one GRIB key and uses it for both models. That is only
    /// sound while the catalogue says they spell the field identically — the fetch now checks it
    /// at runtime, and this checks it at build time so a future divergence (the NAM's
    /// reflectivity level is exactly such a case) fails here rather than in front of a user.
    #[test]
    fn the_compared_models_spell_their_shared_fields_the_same_way() {
        use wxdata::hrrr::Model;
        use wxdata::model::ModelField;
        for f in [ModelField::SurfaceCape, ModelField::Srh3km] {
            let hrrr = f.grib(Model::Hrrr).expect("HRRR publishes it");
            let rap = f.grib(Model::Rap).expect("RAP publishes it");
            assert_eq!(hrrr, rap, "{} diverged between HRRR and RAP", f.label());
        }
    }
}
