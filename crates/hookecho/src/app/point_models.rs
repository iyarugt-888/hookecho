//! The forecast window's model forecast at the tapped point, from every model the app reads and
//! for any product they publish. Pick a product (a common one, or any field from a model's own
//! file inventory), tick the models, and each ticked model's run is read at its own leads over
//! the chosen span and graphed beside the others on one time axis.
//!
//! Each model's line is one run, pinned when its inventory is read: the newest run with lead 12
//! posted. A product is matched in each model's own inventory (the same quantity, level and timing
//! kind), so a model without it says so instead of standing in another field. A lead a run has not
//! posted is a gap in its line. Regional models outside their grid are not asked.
//!
//! Values are in the field's vetted display unit (`wxdata::model_inventory::vetted`), wind as
//! speed in knots from both components, temperatures in the user's unit.

use super::*;
use wxdata::global::{GlobalField, GlobalModel};
use wxdata::hrrr::Model as Regional;
use wxdata::model_inventory::{Field, Level, Stat, TimingKind};

/// Leads fetched at once for one model.
const AT_ONCE: usize = 4;
/// The lead whose inventory picks a model's run and matches the product: one every model has,
/// and late enough that a total since the run began is told from a first bucket (the GEFS's
/// 0–6 h precipitation is both at lead 6, and only 6 h buckets after).
const REF_LEAD: u16 = 12;
/// How long a finished line is reused for the same point, product and span.
const CACHE_SECS: u64 = 900;
/// How far before now the graph starts.
const BACK_H: i64 = 3;
/// At most this many models are read at once.
pub(crate) const MAX_MODELS: usize = 6;
const MS_TO_KT: f32 = 1.943_844;

/// A model the point forecast can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PointModel {
    Regional(Regional),
    Global(GlobalModel),
}

/// Every model, in the order they are offered: convection-allowing regional models, the coarser
/// regional ones and the blend, then the global models.
pub(crate) const ALL: [PointModel; 16] = [
    PointModel::Regional(Regional::Hrrr),
    PointModel::Regional(Regional::Rap),
    PointModel::Regional(Regional::NamNest),
    PointModel::Regional(Regional::HireswArw),
    PointModel::Regional(Regional::HireswFv3),
    PointModel::Regional(Regional::Rrfs),
    PointModel::Regional(Regional::HrefMean),
    PointModel::Regional(Regional::HrefPmm),
    PointModel::Regional(Regional::HrefProb),
    PointModel::Regional(Regional::Nam),
    PointModel::Regional(Regional::Nbm),
    PointModel::Regional(Regional::HrrrPressure),
    PointModel::Global(GlobalModel::Gfs),
    PointModel::Global(GlobalModel::Ecmwf),
    PointModel::Global(GlobalModel::Gefs),
    PointModel::Global(GlobalModel::Gdps),
];

/// Ticked until the user picks their own.
const DEFAULT_MODELS: [PointModel; 4] = [
    PointModel::Regional(Regional::Hrrr),
    PointModel::Regional(Regional::NamNest),
    PointModel::Global(GlobalModel::Gfs),
    PointModel::Global(GlobalModel::Ecmwf),
];

impl PointModel {
    pub(crate) fn label(self) -> &'static str {
        match self {
            PointModel::Regional(Regional::HrrrPressure) => "HRRR pressure levels",
            PointModel::Regional(m) => m.label(),
            PointModel::Global(GlobalModel::Gefs) => "GEFS mean",
            PointModel::Global(g) => g.label(),
        }
    }

    /// The id a saved choice carries.
    pub(crate) fn id(self) -> &'static str {
        match self {
            PointModel::Regional(m) => m.def().id,
            PointModel::Global(GlobalModel::Gfs) => "gfs",
            PointModel::Global(GlobalModel::Ecmwf) => "ecmwf",
            PointModel::Global(GlobalModel::Gefs) => "gefs-mean",
            PointModel::Global(GlobalModel::Gdps) => "gdps",
        }
    }

    pub(crate) fn from_id(id: &str) -> Option<Self> {
        ALL.into_iter().find(|m| m.id() == id)
    }

    /// Whether the model's grid covers the point.
    pub(crate) fn covers(self, lon: f64, lat: f64) -> bool {
        match self {
            PointModel::Regional(m) => m.def().domain.contains(lon, lat),
            PointModel::Global(_) => true,
        }
    }

    /// Whether its file inventory can be read (every model but the GDPS, whose files are one
    /// field each with no index).
    pub(crate) fn has_inventory(self) -> bool {
        self != PointModel::Global(GlobalModel::Gdps)
    }

    /// Why this build cannot read the model, if it cannot. The browser build leaves out the
    /// JPEG 2000 codec, which the RAP's and the GDPS's files are packed with, and NOMADS (the
    /// HiresW, RRFS and HREF) sends no CORS headers and limits how often one address may ask, so
    /// it is not put behind the shared proxy.
    pub(crate) fn unreadable_here(self) -> Option<&'static str> {
        if !cfg!(target_arch = "wasm32") {
            return None;
        }
        match self {
            PointModel::Regional(Regional::Rap) | PointModel::Global(GlobalModel::Gdps) => {
                Some("desktop and Android only: the browser cannot decode its JPEG 2000 files")
            }
            PointModel::Regional(m) if from_nomads(m) => {
                Some("desktop and Android only: its server does not let a browser read it")
            }
            _ => None,
        }
    }

    /// The fewest hours between its points: NOMADS limits how often one address may ask, so its
    /// models are read every third hour (the HiresW has 2 m temperature only then anyway).
    fn min_step(self) -> u16 {
        match self {
            PointModel::Regional(m) if from_nomads(m) => 3,
            _ => 1,
        }
    }

    /// The longest lead `run` publishes.
    fn max_lead(self, run: DateTime<Utc>) -> u16 {
        use chrono::Timelike;
        match self {
            PointModel::Regional(m) => m.max_lead_for_cycle(run.hour()),
            PointModel::Global(GlobalModel::Gfs | GlobalModel::Gefs) => 384,
            PointModel::Global(GlobalModel::Ecmwf) if run.hour().is_multiple_of(12) => 360,
            PointModel::Global(GlobalModel::Ecmwf) => 144,
            PointModel::Global(GlobalModel::Gdps) => 240,
        }
    }

    /// Whether the model publishes a file for `lead` (before its run's end).
    fn publishes(self, lead: u16) -> bool {
        match self {
            PointModel::Regional(Regional::Nam) => lead <= 36 || lead.is_multiple_of(3),
            PointModel::Regional(Regional::Nbm) => {
                lead >= 1
                    && (lead <= 36
                        || (lead <= 192 && lead.is_multiple_of(3))
                        || lead.is_multiple_of(6))
            }
            PointModel::Regional(_) => true,
            PointModel::Global(GlobalModel::Gdps) => lead.is_multiple_of(3),
            PointModel::Global(g) => g.inventory_lead(lead) == lead,
        }
    }

    /// The leads of `run` valid within `from..=to`, on whole multiples of `step` hours of valid
    /// time (so every model's points line up), that the model publishes.
    pub(crate) fn leads(
        self,
        run: DateTime<Utc>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        step: u16,
    ) -> Vec<u16> {
        use chrono::Timelike;
        let max = self.max_lead(run);
        let step = step.max(self.min_step());
        (0..=max)
            .filter(|&lead| {
                let valid = run + chrono::Duration::hours(i64::from(lead));
                valid >= from
                    && valid <= to
                    && valid.hour().is_multiple_of(u32::from(step))
                    && self.publishes(lead)
            })
            .collect()
    }
}

/// The models read from NOMADS rather than a cloud bucket.
fn from_nomads(m: Regional) -> bool {
    matches!(
        m,
        Regional::HireswArw
            | Regional::HireswFv3
            | Regional::Rrfs
            | Regional::HrefMean
            | Regional::HrefPmm
            | Regional::HrefProb
    )
}

/// Leads read at once for `model`: fewer from NOMADS, which limits how often one address asks.
fn at_once(model: PointModel) -> usize {
    match model {
        PointModel::Regional(m) if from_nomads(m) => 2,
        _ => AT_ONCE,
    }
}

/// A common product, matched in each model's inventory by quantity, level and timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Quick {
    Temp2m,
    Dewpoint2m,
    Rh2m,
    Wind10m,
    Gust,
    Mslp,
    Precip,
    Cape,
    Reflectivity,
    Cloud,
    Visibility,
    Pwat,
    Temp850,
    Height500,
    Snow,
}

/// How a common product is found in an inventory.
struct Spec {
    /// The abbreviations that mean it, in preference order (the HRRR names its sea-level pressure
    /// `MSLMA`).
    vars: &'static [&'static str],
    level: Level,
    kind: TimingKind,
    vector: bool,
    /// The same quantity on the GDPS, which has no inventory.
    gdps: Option<GlobalField>,
}

impl Quick {
    pub(crate) const ALL: [Quick; 15] = [
        Quick::Temp2m,
        Quick::Dewpoint2m,
        Quick::Rh2m,
        Quick::Wind10m,
        Quick::Gust,
        Quick::Mslp,
        Quick::Precip,
        Quick::Cape,
        Quick::Reflectivity,
        Quick::Cloud,
        Quick::Visibility,
        Quick::Pwat,
        Quick::Temp850,
        Quick::Height500,
        Quick::Snow,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Quick::Temp2m => "Temperature (2 m)",
            Quick::Dewpoint2m => "Dew point (2 m)",
            Quick::Rh2m => "Relative humidity (2 m)",
            Quick::Wind10m => "Wind speed (10 m)",
            Quick::Gust => "Wind gust",
            Quick::Mslp => "Sea-level pressure",
            Quick::Precip => "Precipitation since the run began",
            Quick::Cape => "Surface CAPE",
            Quick::Reflectivity => "Composite reflectivity",
            Quick::Cloud => "Total cloud cover",
            Quick::Visibility => "Visibility",
            Quick::Pwat => "Precipitable water",
            Quick::Temp850 => "Temperature (850 hPa)",
            Quick::Height500 => "500 hPa height",
            Quick::Snow => "Snow depth",
        }
    }

    pub(crate) fn slug(self) -> &'static str {
        match self {
            Quick::Temp2m => "t2m",
            Quick::Dewpoint2m => "td2m",
            Quick::Rh2m => "rh2m",
            Quick::Wind10m => "wind10m",
            Quick::Gust => "gust",
            Quick::Mslp => "mslp",
            Quick::Precip => "precip",
            Quick::Cape => "cape",
            Quick::Reflectivity => "refc",
            Quick::Cloud => "tcdc",
            Quick::Visibility => "vis",
            Quick::Pwat => "pwat",
            Quick::Temp850 => "t850",
            Quick::Height500 => "z500",
            Quick::Snow => "snod",
        }
    }

    fn spec(self) -> Spec {
        fn at(vars: &'static [&'static str], level: Level, gdps: Option<GlobalField>) -> Spec {
            Spec {
                vars,
                level,
                kind: TimingKind::Instant,
                vector: false,
                gdps,
            }
        }
        match self {
            Quick::Temp2m => at(&["TMP"], Level::AboveGround(2.0), Some(GlobalField::Temp2m)),
            Quick::Dewpoint2m => at(
                &["DPT"],
                Level::AboveGround(2.0),
                Some(GlobalField::Dewpoint2m),
            ),
            Quick::Rh2m => at(&["RH"], Level::AboveGround(2.0), None),
            Quick::Wind10m => Spec {
                vector: true,
                // The GDPS publishes the speed itself.
                ..at(
                    &["UGRD"],
                    Level::AboveGround(10.0),
                    Some(GlobalField::Wind10m),
                )
            },
            Quick::Gust => at(&["GUST"], Level::Surface, None),
            Quick::Mslp => at(
                &["PRMSL", "MSLMA"],
                Level::MeanSeaLevel,
                Some(GlobalField::Mslp),
            ),
            Quick::Precip => Spec {
                kind: TimingKind::SinceRun(Stat::Accumulation),
                ..at(&["APCP"], Level::Surface, Some(GlobalField::Precip))
            },
            Quick::Cape => at(&["CAPE"], Level::Surface, None),
            Quick::Reflectivity => at(&["REFC"], Level::Column, None),
            Quick::Cloud => at(&["TCDC"], Level::Column, None),
            Quick::Visibility => at(&["VIS"], Level::Surface, None),
            Quick::Pwat => at(&["PWAT"], Level::Column, None),
            Quick::Temp850 => at(&["TMP"], Level::Pressure(850.0), None),
            Quick::Height500 => at(
                &["HGT"],
                Level::Pressure(500.0),
                Some(GlobalField::Height500),
            ),
            Quick::Snow => at(&["SNOD"], Level::Surface, None),
        }
    }
}

/// What is graphed: a common product, or one field picked from a model's inventory (matched in
/// the others by the same quantity, level and timing kind).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Product {
    Quick(Quick),
    Field {
        var: String,
        level: String,
        kind: TimingKind,
        vector: bool,
    },
}

impl Default for Product {
    fn default() -> Self {
        Product::Quick(Quick::Temp2m)
    }
}

impl Product {
    pub(crate) fn label(&self) -> String {
        match self {
            Product::Quick(q) => q.label().to_string(),
            Product::Field {
                var,
                level,
                kind,
                vector,
            } => {
                let level = Level::parse(level).label();
                let name = if *vector {
                    "Wind speed".to_string()
                } else {
                    wxdata::model_inventory::name(var)
                };
                format!("{name} \u{b7} {level} \u{b7} {}", kind.label())
            }
        }
    }

    /// The display unit of the values as fetched.
    pub(crate) fn unit(&self) -> &'static str {
        let (var, vector) = match self {
            Product::Quick(q) => {
                let s = q.spec();
                (s.vars[0], s.vector)
            }
            Product::Field { var, vector, .. } => (var.as_str(), *vector),
        };
        if vector {
            return "kt";
        }
        wxdata::model_inventory::vetted(var).map_or("", |q| q.unit)
    }
}

/// One model's message for the product, as its inventory names it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Resolved {
    pub var: String,
    pub level: String,
    pub kind: TimingKind,
    pub vector: bool,
}

/// Find `product` in an inventory: the same quantity at the same level with the same timing kind.
/// A wind is a pair of components at the level.
pub(crate) fn resolve(product: &Product, fields: &[Field]) -> Option<Resolved> {
    let (vars, level, kind, vector): (Vec<&str>, Level, TimingKind, bool) = match product {
        Product::Quick(q) => {
            let s = q.spec();
            (s.vars.to_vec(), s.level, s.kind, s.vector)
        }
        Product::Field {
            var,
            level,
            kind,
            vector,
        } => (vec![var.as_str()], Level::parse(level), *kind, *vector),
    };
    let level_text = match product {
        Product::Field { level, .. } => Some(level.as_str()),
        Product::Quick(_) => None,
    };
    let matches = |f: &Field| {
        f.entry.timing.kind() == Some(kind)
            && (level_text == Some(f.entry.level_text.as_str()) || f.entry.level == level)
    };
    let found = |f: &Field| Resolved {
        var: f.entry.var.clone(),
        level: f.entry.level_text.clone(),
        kind,
        vector,
    };
    if vector {
        return wxdata::model_inventory::vector_pairs(fields)
            .into_iter()
            .find(|(u, _)| matches(u))
            .map(|(u, _)| found(u));
    }
    vars.iter().find_map(|var| {
        fields
            .iter()
            .find(|f| f.supported() && f.entry.var == *var && matches(f))
            .map(found)
    })
}

/// How far ahead the graph reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum Span {
    H24,
    #[default]
    H48,
    D5,
    D10,
}

impl Span {
    pub(crate) const ALL: [Span; 4] = [Span::H24, Span::H48, Span::D5, Span::D10];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Span::H24 => "24 h",
            Span::H48 => "48 h",
            Span::D5 => "5 days",
            Span::D10 => "10 days",
        }
    }

    pub(crate) fn hours(self) -> i64 {
        match self {
            Span::H24 => 24,
            Span::H48 => 48,
            Span::D5 => 120,
            Span::D10 => 240,
        }
    }

    /// Hours between points: hourly over two days, then coarser so a line stays a few dozen reads.
    pub(crate) fn step(self) -> u16 {
        match self {
            Span::H24 | Span::H48 => 1,
            Span::D5 => 3,
            Span::D10 => 6,
        }
    }
}

/// A value (or a gap) per valid time.
type Series = Vec<(DateTime<Utc>, Option<f32>)>;

/// A fetch's progress, as it sends it.
enum LineMsg {
    Run(DateTime<Utc>, usize),
    Points(Series),
    /// The model has no such product (or does not cover the point).
    NotOffered(String),
    Failed(String),
    Done,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LineStatus {
    Loading,
    Done,
    NotOffered(String),
    Failed(String),
}

/// One model's line.
pub(crate) struct Line {
    pub run: Option<DateTime<Utc>>,
    /// Leads asked for, once the run is known.
    pub expected: usize,
    pub points: Series,
    pub status: LineStatus,
    rx: Option<std::sync::mpsc::Receiver<LineMsg>>,
}

impl Line {
    fn settled(status: LineStatus) -> Self {
        Line {
            run: None,
            expected: 0,
            points: Vec::new(),
            status,
            rx: None,
        }
    }

    fn copy(&self) -> Self {
        Line {
            run: self.run,
            expected: self.expected,
            points: self.points.clone(),
            status: self.status.clone(),
            rx: None,
        }
    }

    /// Take what the fetch sent since last frame.
    fn drain(&mut self) {
        let Some(rx) = &self.rx else { return };
        loop {
            match rx.try_recv() {
                Ok(LineMsg::Run(run, n)) => {
                    self.run = Some(run);
                    self.expected = n;
                }
                Ok(LineMsg::Points(p)) => {
                    self.points.extend(p);
                    self.points.sort_by_key(|(t, _)| *t);
                }
                Ok(LineMsg::NotOffered(why)) => self.status = LineStatus::NotOffered(why),
                Ok(LineMsg::Failed(e)) => self.status = LineStatus::Failed(e),
                Ok(LineMsg::Done) => {
                    if self.status == LineStatus::Loading {
                        self.status = LineStatus::Done;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if self.status == LineStatus::Loading {
                        self.status = LineStatus::Failed("the fetch stopped".into());
                    }
                    self.rx = None;
                    return;
                }
            }
        }
    }
}

/// What a set of lines is for: the point's ~0.05° cell, the product and the span.
type LinesKey = ((i32, i32), Product, Span);

/// The "Any field" picker: one model's whole inventory, searchable.
#[derive(Default)]
pub(crate) struct FieldPicker {
    pub open: bool,
    pub model: Option<PointModel>,
    pub search: String,
    pub state: Option<(PointModel, super::model_field::InventoryState)>,
    rx: Option<std::sync::mpsc::Receiver<(PointModel, super::model_field::InventoryState)>>,
}

pub(crate) struct PointModels {
    pub product: Product,
    pub span: Span,
    /// Ticked models, in [`ALL`]'s order.
    pub models: Vec<PointModel>,
    pub picker: FieldPicker,
    key: Option<LinesKey>,
    lines: std::collections::HashMap<PointModel, Line>,
    cache: std::collections::HashMap<(LinesKey, PointModel), (Instant, Line)>,
    /// The choice changed this frame (for saving).
    pub changed: bool,
}

impl Default for PointModels {
    fn default() -> Self {
        PointModels {
            product: Product::default(),
            span: Span::default(),
            models: DEFAULT_MODELS.to_vec(),
            picker: FieldPicker::default(),
            key: None,
            lines: std::collections::HashMap::new(),
            cache: std::collections::HashMap::new(),
            changed: false,
        }
    }
}

impl PointModels {
    /// The saved choice of models and common product, where one was saved.
    pub(crate) fn restore(&mut self, models: &[String], product: &str) {
        let mut picked: Vec<PointModel> = models
            .iter()
            .filter_map(|id| PointModel::from_id(id))
            .collect();
        picked.sort_by_key(|m| ALL.iter().position(|x| x == m));
        picked.dedup();
        picked.truncate(MAX_MODELS);
        if !picked.is_empty() {
            self.models = picked;
        }
        if let Some(q) = Quick::ALL.into_iter().find(|q| q.slug() == product) {
            self.product = Product::Quick(q);
        }
    }
}

/// Read one model's run at the point: its inventory, the product in it, then each lead.
#[allow(clippy::too_many_arguments)]
async fn fetch_line(
    http: reqwest::Client,
    model: PointModel,
    product: Product,
    (lon, lat): (f64, f64),
    (from, to): (DateTime<Utc>, DateTime<Utc>),
    step: u16,
    tx: std::sync::mpsc::Sender<LineMsg>,
    ctx: egui::Context,
) {
    let send = |m: LineMsg| {
        let _ = tx.send(m);
        ctx.request_repaint();
    };
    // The GDPS: the common products it publishes, one file each.
    if let PointModel::Global(GlobalModel::Gdps) = model {
        let Product::Quick(q) = &product else {
            return send(LineMsg::NotOffered(
                "fields picked from an inventory are not on the GDPS".into(),
            ));
        };
        let Some(gf) = q.spec().gdps else {
            return send(LineMsg::NotOffered("not among the GDPS's fields".into()));
        };
        let g = GlobalModel::Gdps;
        let first = match wxdata::global::fetch(&http, g, gf, REF_LEAD).await {
            Ok(f) => f,
            Err(e) => return send(LineMsg::Failed(format!("{e:#}"))),
        };
        let run = first.run;
        let leads = model.leads(run, from, to, step);
        send(LineMsg::Run(run, leads.len()));
        for group in leads.chunks(at_once(model)) {
            let jobs = group.iter().map(|&lead| {
                let http = http.clone();
                async move {
                    let valid = run + chrono::Duration::hours(i64::from(lead));
                    let value = wxdata::global::fetch_at_run(&http, g, gf, run, lead)
                        .await
                        .ok()
                        .and_then(|f| f.field.sample_bilinear(lon, lat))
                        .map(|v| gdps_display(gf, v));
                    (valid, value)
                }
            });
            send(LineMsg::Points(futures_util::future::join_all(jobs).await));
        }
        return send(LineMsg::Done);
    }
    let inventory = match model {
        PointModel::Regional(m) => wxdata::hrrr::fetch_inventory(&http, m, None, REF_LEAD as u8)
            .await
            .map(|(run, _, fields)| (run, fields)),
        PointModel::Global(g) => {
            wxdata::global::fetch_global_inventory(&http, g, None, REF_LEAD).await
        }
    };
    let (mut run, mut fields) = match inventory {
        Ok(got) => got,
        Err(e) => return send(LineMsg::Failed(format!("{e:#}"))),
    };
    // The HRRR's and RAP's newest run is often a short one: when it ends before the span does,
    // the newest long run reads further.
    if let PointModel::Regional(m) = model {
        if run + chrono::Duration::hours(i64::from(model.max_lead(run))) < to {
            if let Some(long) = longer_run(m, run) {
                if let Ok((r, _, f)) =
                    wxdata::hrrr::fetch_inventory(&http, m, Some(long), REF_LEAD as u8).await
                {
                    (run, fields) = (r, f);
                }
            }
        }
    }
    let Some(r) = resolve(&product, &fields) else {
        return send(LineMsg::NotOffered("not in this model's file".into()));
    };
    let leads = model.leads(run, from, to, step);
    send(LineMsg::Run(run, leads.len()));
    for group in leads.chunks(at_once(model)) {
        let jobs = group.iter().map(|&lead| {
            let (http, r) = (http.clone(), &r);
            async move {
                let valid = run + chrono::Duration::hours(i64::from(lead));
                (
                    valid,
                    read_lead(&http, model, run, lead, r, (lon, lat)).await,
                )
            }
        });
        send(LineMsg::Points(futures_util::future::join_all(jobs).await));
    }
    send(LineMsg::Done);
}

/// The newest run before `run` that runs longer than it, for a model with extended cycles.
fn longer_run(m: Regional, run: DateTime<Utc>) -> Option<DateTime<Utc>> {
    use chrono::Timelike;
    let own = m.max_lead_for_cycle(run.hour());
    (1..24)
        .map(|back| run - chrono::Duration::hours(back))
        .find(|r| m.max_lead_for_cycle(r.hour()) > own)
}

/// One lead's value at the point, in display units; `None` where the run does not have it.
async fn read_lead(
    http: &reqwest::Client,
    model: PointModel,
    run: DateTime<Utc>,
    lead: u16,
    r: &Resolved,
    (lon, lat): (f64, f64),
) -> Option<f32> {
    let speed = |u: &wxdata::mrms::MrmsField, v: &wxdata::mrms::MrmsField| {
        let (u, v) = (u.sample_bilinear(lon, lat)?, v.sample_bilinear(lon, lat)?);
        Some(u.hypot(v) * MS_TO_KT)
    };
    match (model, r.vector) {
        (PointModel::Regional(m), false) => wxdata::hrrr::fetch_inventory_field(
            http,
            m,
            Some(run),
            u8::try_from(lead).ok()?,
            &r.var,
            &r.level,
            r.kind,
        )
        .await
        .ok()?
        .field
        .sample_bilinear(lon, lat),
        (PointModel::Regional(m), true) => {
            let (_, _, u, v) = wxdata::hrrr::fetch_inventory_vector(
                http,
                m,
                Some(run),
                u8::try_from(lead).ok()?,
                &r.level,
                r.kind,
            )
            .await
            .ok()?;
            speed(&u, &v)
        }
        (PointModel::Global(g), false) => wxdata::global::fetch_global_inventory_field(
            http,
            g,
            Some(run),
            lead,
            &r.var,
            &r.level,
            r.kind,
        )
        .await
        .ok()?
        .field
        .sample_bilinear(lon, lat),
        (PointModel::Global(g), true) => {
            let (_, u, v) = wxdata::global::fetch_global_inventory_vector(
                http,
                g,
                Some(run),
                lead,
                &r.level,
                r.kind,
            )
            .await
            .ok()?;
            speed(&u, &v)
        }
    }
}

/// A GDPS value in the unit the inventory's vetted quantity shows it in.
fn gdps_display(field: GlobalField, raw: f32) -> f32 {
    match field {
        GlobalField::Temp2m | GlobalField::Dewpoint2m => raw - 273.15,
        GlobalField::Mslp => raw * 0.01,
        GlobalField::Height500 => raw * 0.1,
        GlobalField::Wind10m => raw * MS_TO_KT,
        GlobalField::Precip => raw,
    }
}

impl HookEchoApp {
    /// Keep the point forecast's model lines current while the forecast window is open: drain
    /// what arrived, and start a fetch for each ticked model without a line for this point,
    /// product and span.
    pub(crate) fn sync_point_models(&mut self, ctx: &egui::Context) {
        let pm = &mut self.point_models;
        if pm.changed {
            pm.changed = false;
            self.settings.point_models = pm.models.iter().map(|m| m.id().to_string()).collect();
            self.settings.point_product = match &pm.product {
                Product::Quick(q) => q.slug().to_string(),
                Product::Field { .. } => String::new(),
            };
            self.settings.save();
        }
        self.sync_field_picker(ctx);
        let Some((lon, lat)) = self.forecast_at else {
            return;
        };
        let pm = &mut self.point_models;
        for line in pm.lines.values_mut() {
            line.drain();
        }
        let cell = ((lat * 20.0).round() as i32, (lon * 20.0).round() as i32);
        let key = (cell, pm.product.clone(), pm.span);
        if pm.key.as_ref() != Some(&key) {
            if let Some(old) = pm.key.take() {
                for (m, line) in pm.lines.drain() {
                    if line.status != LineStatus::Loading {
                        pm.cache.insert((old.clone(), m), (Instant::now(), line));
                    }
                }
            }
            pm.cache
                .retain(|_, (when, _)| when.elapsed().as_secs() < CACHE_SECS);
            pm.key = Some(key.clone());
        }
        pm.lines.retain(|m, _| pm.models.contains(m));
        let now = Utc::now();
        let from = now - chrono::Duration::hours(BACK_H);
        let to = now + chrono::Duration::hours(pm.span.hours());
        for m in pm.models.clone() {
            if pm.lines.contains_key(&m) {
                continue;
            }
            if !m.covers(lon, lat) {
                pm.lines.insert(
                    m,
                    Line::settled(LineStatus::NotOffered("outside its grid".into())),
                );
                continue;
            }
            if let Some(why) = m.unreadable_here() {
                pm.lines
                    .insert(m, Line::settled(LineStatus::NotOffered(why.into())));
                continue;
            }
            if let Some((when, line)) = pm.cache.get(&(key.clone(), m)) {
                if when.elapsed().as_secs() < CACHE_SECS {
                    pm.lines.insert(m, line.copy());
                    continue;
                }
            }
            let (tx, rx) = std::sync::mpsc::channel();
            pm.lines.insert(
                m,
                Line {
                    rx: Some(rx),
                    ..Line::settled(LineStatus::Loading)
                },
            );
            self.spawner.spawn(fetch_line(
                self.http.clone(),
                m,
                pm.product.clone(),
                (lon, lat),
                (from, to),
                pm.span.step(),
                tx,
                ctx.clone(),
            ));
        }
    }

    /// Read the picker's model inventory once per model.
    fn sync_field_picker(&mut self, ctx: &egui::Context) {
        use super::model_field::InventoryState;
        let p = &mut self.point_models.picker;
        if let Some(rx) = &p.rx {
            if let Ok(got) = rx.try_recv() {
                p.state = Some(got);
                p.rx = None;
            }
        }
        let Some(model) = p.model.filter(|_| p.open) else {
            return;
        };
        if p.state.as_ref().is_some_and(|(m, _)| *m == model) {
            return;
        }
        p.state = Some((model, InventoryState::Pending));
        let (tx, rx) = std::sync::mpsc::channel();
        p.rx = Some(rx);
        let http = self.http.clone();
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            let got = match model {
                PointModel::Regional(m) => {
                    wxdata::hrrr::fetch_inventory(&http, m, None, REF_LEAD as u8)
                        .await
                        .map(|(run, lead, fields)| (run, u16::from(lead), fields))
                }
                PointModel::Global(g) => {
                    wxdata::global::fetch_global_inventory(&http, g, None, REF_LEAD)
                        .await
                        .map(|(run, fields)| (run, REF_LEAD, fields))
                }
            };
            let state = match got {
                Ok((run, lead_h, fields)) => InventoryState::Ready {
                    run,
                    lead_h,
                    fields,
                },
                Err(e) => InventoryState::Failed(format!("{e:#}")),
            };
            let _ = tx.send((model, state));
            ctx.request_repaint();
        });
    }
}

/// A distinct colour per model, stable across products.
pub(crate) fn model_color(m: PointModel) -> egui::Color32 {
    const COLORS: [[u8; 3]; 16] = [
        [102, 194, 255],
        [255, 160, 80],
        [120, 220, 130],
        [235, 110, 150],
        [190, 150, 255],
        [255, 220, 90],
        [90, 220, 210],
        [200, 200, 120],
        [255, 120, 100],
        [170, 230, 255],
        [150, 160, 255],
        [240, 240, 240],
        [255, 110, 220],
        [130, 200, 90],
        [230, 180, 140],
        [180, 120, 90],
    ];
    let i = ALL.iter().position(|x| *x == m).unwrap_or(0);
    let [r, g, b] = COLORS[i % COLORS.len()];
    egui::Color32::from_rgb(r, g, b)
}

impl PointModels {
    /// The section in the forecast window: product, span and model pickers, the graph and each
    /// model's line in words.
    pub(crate) fn section(
        &mut self,
        ui: &mut egui::Ui,
        at: (f64, f64),
        tz: Option<wxdata::tz::Tz>,
        temp: crate::settings::TempUnit,
    ) {
        use egui::RichText;
        ui.label(RichText::new("Models at this point").strong());
        ui.horizontal_wrapped(|ui| {
            ui.label("Product");
            let mut product = self.product.clone();
            egui::ComboBox::from_id_salt("point_models_product")
                .selected_text(product.label())
                .width(220.0)
                .show_ui(ui, |ui| {
                    for q in Quick::ALL {
                        ui.selectable_value(&mut product, Product::Quick(q), q.label());
                    }
                });
            if product != self.product {
                self.product = product;
                self.changed = true;
            }
            if ui
                .selectable_label(self.picker.open, "Any field\u{2026}")
                .on_hover_text("Pick any field from one model's own file inventory")
                .clicked()
            {
                self.picker.open = !self.picker.open;
                if self.picker.model.is_none() {
                    self.picker.model = self.models.iter().copied().find(|m| m.has_inventory());
                }
            }
        });
        if self.picker.open {
            self.field_picker(ui);
        }
        ui.horizontal_wrapped(|ui| {
            for s in Span::ALL {
                if ui.selectable_value(&mut self.span, s, s.label()).changed() {
                    self.changed = true;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for m in ALL {
                let on = self.models.contains(&m);
                let covers = m.covers(at.0, at.1);
                let text = RichText::new(m.label()).color(if on {
                    model_color(m)
                } else {
                    ui.visuals().weak_text_color()
                });
                let mut resp = ui.selectable_label(on, text);
                if !covers {
                    resp = resp.on_hover_text("This point is outside its grid");
                } else if let Some(why) = m.unreadable_here() {
                    resp = resp.on_hover_text(format!("Not here: {why}"));
                }
                if resp.clicked() {
                    if on {
                        self.models.retain(|x| *x != m);
                    } else if self.models.len() < MAX_MODELS {
                        self.models.push(m);
                        self.models.sort_by_key(|x| ALL.iter().position(|y| y == x));
                    }
                    self.changed = true;
                }
            }
        });
        if self.models.len() >= MAX_MODELS {
            ui.weak(format!("Up to {MAX_MODELS} models at once"));
        }
        let unit = self.product.unit();
        let to_f = unit == "°C" && temp == crate::settings::TempUnit::Fahrenheit;
        let shown_unit = if to_f { "°F" } else { unit };
        let convert = |v: f32| if to_f { temp.from_c(v) } else { v };
        let lines: Vec<(PointModel, Series)> = self
            .models
            .iter()
            .filter_map(|m| {
                let line = self.lines.get(m)?;
                Some((
                    *m,
                    line.points
                        .iter()
                        .map(|(t, v)| (*t, v.map(convert)))
                        .collect(),
                ))
            })
            .collect();
        let chart: Vec<crate::ui::forecast_window::ChartLine> = lines
            .iter()
            .map(|(m, points)| crate::ui::forecast_window::ChartLine {
                label: m.label(),
                color: model_color(*m),
                points,
            })
            .collect();
        let decimals = decimals(shown_unit);
        crate::ui::forecast_window::paint_lines(ui, &chart, shown_unit, decimals, tz, Utc::now());
        for (m, points) in &lines {
            let Some(line) = self.lines.get(m) else {
                continue;
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("\u{25cf}").color(model_color(*m)));
                ui.label(m.label());
                if let Some(run) = line.run {
                    ui.weak(run.format("%d %HZ run").to_string());
                }
                let values: Vec<f32> = points.iter().filter_map(|(_, v)| *v).collect();
                match &line.status {
                    LineStatus::Loading => {
                        ui.spinner();
                        if line.expected > 0 {
                            ui.weak(format!("{}/{} leads", line.points.len(), line.expected));
                        }
                    }
                    LineStatus::NotOffered(why) => {
                        ui.weak(format!("\u{2014} {why}"));
                    }
                    LineStatus::Failed(e) => {
                        ui.colored_label(egui::Color32::from_rgb(230, 120, 120), "failed")
                            .on_hover_text(e);
                    }
                    LineStatus::Done if values.is_empty() => {
                        ui.weak("\u{2014} no values for this span");
                    }
                    LineStatus::Done => {}
                }
                if !values.is_empty() {
                    let (lo, hi) = values
                        .iter()
                        .fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
                    ui.weak(format!(
                        "{lo:.decimals$}\u{2013}{hi:.decimals$} {shown_unit}"
                    ));
                    let gaps = points.iter().filter(|(_, v)| v.is_none()).count();
                    if gaps > 0 && line.status != LineStatus::Loading {
                        ui.weak(if gaps == 1 {
                            "(1 lead missing)".to_string()
                        } else {
                            format!("({gaps} leads missing)")
                        });
                    }
                }
            });
        }
        ui.weak(
            "Each line is one model run read at this point, hourly over two days and every 3 or \
             6 hours beyond. A model without this product says so rather than showing another.",
        );
    }

    /// One model's whole inventory, searchable; a click graphs that field for every ticked model.
    fn field_picker(&mut self, ui: &mut egui::Ui) {
        use super::model_field::InventoryState;
        let p = &mut self.picker;
        ui.group(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("From");
                let current = p.model.map_or("a model", |m| m.label());
                egui::ComboBox::from_id_salt("point_models_picker_model")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for m in ALL.into_iter().filter(|m| m.has_inventory()) {
                            ui.selectable_value(&mut p.model, Some(m), m.label());
                        }
                    });
            });
            ui.add(
                egui::TextEdit::singleline(&mut p.search)
                    .hint_text("Search: temperature, 700, helicity\u{2026}"),
            );
            let fields = match (&p.state, p.model) {
                (Some((m, InventoryState::Ready { fields, run, .. })), Some(want))
                    if *m == want =>
                {
                    ui.weak(format!(
                        "{} {} run: {} fields with vetted units",
                        m.label(),
                        run.format("%d %HZ"),
                        fields.iter().filter(|f| f.supported()).count()
                    ));
                    fields
                }
                (Some((_, InventoryState::Failed(e))), _) => {
                    ui.colored_label(egui::Color32::from_rgb(230, 120, 80), e);
                    return;
                }
                _ => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.weak("Reading the inventory\u{2026}");
                    });
                    return;
                }
            };
            let needle = p.search.trim().to_lowercase();
            let mut chosen = None;
            egui::ScrollArea::vertical()
                .id_salt("point_models_picker")
                .max_height(200.0)
                .show(ui, |ui| {
                    let winds = wxdata::model_inventory::vector_pairs(fields);
                    for (u, _) in winds {
                        let Some(kind) = u.entry.timing.kind() else {
                            continue;
                        };
                        let product = Product::Field {
                            var: u.entry.var.clone(),
                            level: u.entry.level_text.clone(),
                            kind,
                            vector: true,
                        };
                        let label = product.label();
                        if (needle.is_empty() || label.to_lowercase().contains(&needle))
                            && ui
                                .selectable_label(self.product == product, &label)
                                .clicked()
                        {
                            chosen = Some(product);
                        }
                    }
                    for f in fields.iter().filter(|f| f.supported()) {
                        let Some(kind) = f.entry.timing.kind() else {
                            continue;
                        };
                        if f.quantity.and_then(|q| q.component).is_some() {
                            continue;
                        }
                        let label = f.label();
                        if !needle.is_empty() && !label.to_lowercase().contains(&needle) {
                            continue;
                        }
                        let product = Product::Field {
                            var: f.entry.var.clone(),
                            level: f.entry.level_text.clone(),
                            kind,
                            vector: false,
                        };
                        if ui
                            .selectable_label(self.product == product, label)
                            .clicked()
                        {
                            chosen = Some(product);
                        }
                    }
                });
            if let Some(product) = chosen {
                self.product = product;
                self.changed = true;
            }
        });
    }
}

/// Decimals a value in `unit` is shown with.
fn decimals(unit: &str) -> usize {
    match unit {
        "g/kg" | "in" | "mm/h" | "km" | "mm" => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(var: &str, level: &str, timing: &str) -> Field {
        let line = format!("1:0:d=2026101212:{var}:{level}:{timing}:");
        wxdata::model_inventory::discover(&line)
            .into_iter()
            .next()
            .unwrap()
    }

    /// A common product is found by quantity, level and timing kind, under any of its names; a
    /// model without it is not given another field.
    #[test]
    fn a_product_is_matched_in_each_inventory_or_not_at_all() {
        let hrrr = vec![
            field("MSLMA", "mean sea level", "6 hour fcst"),
            field("TMP", "2 m above ground", "6 hour fcst"),
            field("APCP", "surface", "5-6 hour acc fcst"),
            field("APCP", "surface", "0-6 hour acc fcst"),
            field("UGRD", "10 m above ground", "6 hour fcst"),
            field("VGRD", "10 m above ground", "6 hour fcst"),
        ];
        let mslp = resolve(&Product::Quick(Quick::Mslp), &hrrr).unwrap();
        assert_eq!(mslp.var, "MSLMA");
        let precip = resolve(&Product::Quick(Quick::Precip), &hrrr).unwrap();
        assert_eq!(precip.kind, TimingKind::SinceRun(Stat::Accumulation));
        let wind = resolve(&Product::Quick(Quick::Wind10m), &hrrr).unwrap();
        assert!(wind.vector);
        assert_eq!(wind.level, "10 m above ground");
        assert!(resolve(&Product::Quick(Quick::Gust), &hrrr).is_none());
        // Only a 1 h bucket: not the total since the run began.
        let nbm = vec![field("APCP", "surface", "5-6 hour acc fcst")];
        assert!(resolve(&Product::Quick(Quick::Precip), &nbm).is_none());
        // A picked field matches the same quantity and level in another model.
        let picked = Product::Field {
            var: "TMP".into(),
            level: "2 m above ground".into(),
            kind: TimingKind::Instant,
            vector: false,
        };
        assert_eq!(resolve(&picked, &hrrr).unwrap().var, "TMP");
    }

    /// Leads land on the same valid hours in every model, within the span, on files it publishes.
    #[test]
    fn leads_line_up_on_valid_time_and_published_files() {
        let t = |h: i64| {
            DateTime::<Utc>::from_timestamp(1_791_763_200, 0).unwrap() + chrono::Duration::hours(h)
        };
        // A 12Z NAM run, read from 02Z the next day for three days, every 3 h of valid time.
        let nam = PointModel::Regional(Regional::Nam);
        let leads = nam.leads(t(12), t(38), t(38 + 72), 3);
        assert_eq!(leads.first(), Some(&27));
        assert!(leads.iter().all(|l| (12 + l) % 3 == 0));
        assert_eq!(leads.last(), Some(&84));
        // ECMWF past F+144 publishes every 6 h.
        let ec = PointModel::Global(GlobalModel::Ecmwf);
        let leads = ec.leads(t(0), t(0), t(240), 3);
        assert!(leads.contains(&144) && !leads.contains(&147) && leads.contains(&150));
        // The NBM has no analysis file.
        let nbm = PointModel::Regional(Regional::Nbm);
        assert_eq!(nbm.leads(t(0), t(0), t(3), 1), vec![1, 2, 3]);
        // NOMADS is asked every third hour at most.
        let hiresw = PointModel::Regional(Regional::HireswArw);
        assert_eq!(hiresw.leads(t(12), t(12), t(24), 1), vec![0, 3, 6, 9, 12]);
    }

    #[test]
    fn a_short_run_reaches_back_to_the_newest_long_one() {
        let t = |h: i64| {
            DateTime::<Utc>::from_timestamp(1_791_763_200, 0).unwrap() + chrono::Duration::hours(h)
        };
        assert_eq!(longer_run(Regional::Hrrr, t(19)), Some(t(18)));
        assert_eq!(longer_run(Regional::Hrrr, t(18)), None);
        assert_eq!(longer_run(Regional::Rap, t(1)), Some(t(-3)));
        assert_eq!(longer_run(Regional::Nam, t(18)), None);
    }

    #[test]
    fn a_saved_choice_comes_back() {
        let mut pm = PointModels::default();
        pm.restore(
            &["rap".into(), "gefs-mean".into(), "nonsense".into()],
            "gust",
        );
        assert_eq!(
            pm.models,
            vec![
                PointModel::Regional(Regional::Rap),
                PointModel::Global(GlobalModel::Gefs)
            ]
        );
        assert_eq!(pm.product, Product::Quick(Quick::Gust));
        for m in ALL {
            assert_eq!(PointModel::from_id(m.id()), Some(m), "{}", m.label());
        }
    }

    /// Every model's 2 m temperature at Oklahoma City over two days, live.
    /// `cargo test -p hookecho every_model_at_a_point_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "network"]
    async fn every_model_at_a_point_live() {
        let ctx = egui::Context::default();
        let now = Utc::now();
        let (from, to) = (
            now - chrono::Duration::hours(3),
            now + chrono::Duration::hours(48),
        );
        for product in [
            Product::Quick(Quick::Temp2m),
            Product::Quick(Quick::Wind10m),
            Product::Quick(Quick::Precip),
        ] {
            for m in ALL {
                let (tx, rx) = std::sync::mpsc::channel();
                fetch_line(
                    reqwest::Client::new(),
                    m,
                    product.clone(),
                    (-97.52, 35.47),
                    (from, to),
                    6,
                    tx,
                    ctx.clone(),
                )
                .await;
                let mut line = Line {
                    rx: Some(rx),
                    ..Line::settled(LineStatus::Loading)
                };
                line.drain();
                let got: Vec<String> = line
                    .points
                    .iter()
                    .map(|(t, v)| {
                        format!(
                            "{}={}",
                            t.format("%d/%H"),
                            v.map_or("-".into(), |v| format!("{v:.1}"))
                        )
                    })
                    .collect();
                println!(
                    "{:?} {:<22} run {:?} {:?}: {}",
                    product,
                    m.label(),
                    line.run.map(|r| r.format("%d %HZ").to_string()),
                    line.status,
                    got.join(" ")
                );
            }
        }
    }
}
