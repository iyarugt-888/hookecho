//! What "the models" are to a person: pick a **model**, then a **product** it publishes, then a
//! **lead**. Reflectivity is just one product, offered by every model that has it; the HRRR's
//! 15-minute output is a model of its own rather than a switch on another layer.
//!
//! This is only the catalogue: which products each model can show, the layer that draws each, and
//! how far out and in what steps it can be scrubbed. The app maps a choice onto the renderer's
//! existing field layers, so nothing here fetches or draws.

use crate::render::FieldLayer;
use chrono::{DateTime, Timelike, Utc};
use wxdata::global::GlobalModel;
use wxdata::hrrr::Model as Regional;

/// A forecast source a person can pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BModel {
    Hrrr,
    /// The HRRR's `wrfsubhf` output: the same model at 15-minute steps.
    Hrrr15,
    Rap,
    NamNest,
    Nam,
    Nbm,
    /// The RTMA real-time surface analysis: an estimate of now, hourly, with no lead.
    Rtma,
    Gfs,
    Ecmwf,
    GefsMean,
    Gdps,
}

/// What a model can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Product {
    Reflectivity,
    Cape,
    Srh,
    UpdraftHelicity,
    Snowfall,
    Smoke,
    ThunderChance,
    Temp2m,
    Dewpoint2m,
    Wind10m,
    Mslp,
    Height500,
    /// Precipitable water (GFS, GEFS) or total precipitation (ECMWF, GDPS).
    Moisture,
    /// The RTMA analysis's own surface fields: what the temperature, dewpoint and wind are doing
    /// now, as opposed to a forecast of them.
    AnalysisTemp2m,
    AnalysisDewpoint2m,
    AnalysisWind10m,
    AnalysisGust10m,
}

/// How models group in a picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Convection-allowing and regional forecasts.
    StormScale,
    /// Analyses of the present rather than forecasts.
    Analysis,
    Global,
}

impl Family {
    pub const ALL: [Family; 3] = [Family::StormScale, Family::Analysis, Family::Global];

    pub fn label(self) -> &'static str {
        match self {
            Family::StormScale => "Storm scale",
            Family::Analysis => "Analysis",
            Family::Global => "Global",
        }
    }
}

/// How a model is fetched, which decides which of the app's clocks its lead lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// A regional NCEP model read through the shared GRIB path.
    Regional(Regional),
    /// The HRRR sub-hourly product.
    Sub15,
    Global(GlobalModel),
    /// An hourly analysis (RTMA): valid at its own hour, so it has no run/lead pair.
    Analysis,
}

/// How far out a model can be scrubbed, in minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeadRange {
    pub min: u16,
    pub max: u16,
    pub step: u16,
    /// Past this lead the model publishes less often: `(after, step)`, in minutes. The GFS family
    /// goes from 3-hourly to 6-hourly, the NAM 12 km from hourly to 3-hourly, and so on.
    pub coarse: Option<(u16, u16)>,
}

impl LeadRange {
    const fn fixed(min: u16, max: u16, step: u16) -> Self {
        Self {
            min,
            max,
            step,
            coarse: None,
        }
    }

    /// The step in force just after `lead`, when moving later.
    fn step_up(self, lead: u16) -> u16 {
        match self.coarse {
            Some((after, coarse)) if lead >= after => coarse,
            _ => self.step,
        }
    }

    /// The step in force just before `lead`, when moving earlier.
    fn step_down(self, lead: u16) -> u16 {
        match self.coarse {
            Some((after, coarse)) if lead > after => coarse,
            _ => self.step,
        }
    }

    /// Snap a lead into this range, on the grid the model actually publishes.
    pub fn clamp(self, minutes: u16) -> u16 {
        let m = minutes.clamp(self.min, self.max);
        let snapped = match self.coarse {
            Some((after, coarse)) if m > after => after + (m - after) / coarse * coarse,
            _ => self.min + (m - self.min) / self.step * self.step,
        };
        snapped.min(self.max)
    }

    /// The next published lead after (`later`) or before `from`, staying inside the range.
    pub fn neighbour(self, from: u16, later: bool) -> u16 {
        let from = self.clamp(from);
        if later {
            self.clamp(from.saturating_add(self.step_up(from)))
        } else {
            self.clamp(from.saturating_sub(self.step_down(from)))
        }
    }
}

impl BModel {
    pub const ALL: [BModel; 11] = [
        BModel::Hrrr,
        BModel::Hrrr15,
        BModel::Rap,
        BModel::NamNest,
        BModel::Nam,
        BModel::Nbm,
        BModel::Rtma,
        BModel::Gfs,
        BModel::Ecmwf,
        BModel::GefsMean,
        BModel::Gdps,
    ];

    pub fn engine(self) -> Engine {
        match self {
            BModel::Hrrr => Engine::Regional(Regional::Hrrr),
            BModel::Hrrr15 => Engine::Sub15,
            BModel::Rap => Engine::Regional(Regional::Rap),
            BModel::NamNest => Engine::Regional(Regional::NamNest),
            BModel::Nam => Engine::Regional(Regional::Nam),
            BModel::Nbm => Engine::Regional(Regional::Nbm),
            BModel::Rtma => Engine::Analysis,
            BModel::Gfs => Engine::Global(GlobalModel::Gfs),
            BModel::Ecmwf => Engine::Global(GlobalModel::Ecmwf),
            BModel::GefsMean => Engine::Global(GlobalModel::Gefs),
            BModel::Gdps => Engine::Global(GlobalModel::Gdps),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BModel::Hrrr => "HRRR",
            BModel::Hrrr15 => "HRRR 15-min",
            BModel::Rap => "RAP",
            BModel::NamNest => "NAM 3 km",
            BModel::Nam => "NAM 12 km",
            BModel::Nbm => "NBM",
            BModel::Rtma => "RTMA",
            BModel::Gfs => "GFS",
            BModel::Ecmwf => "ECMWF",
            BModel::GefsMean => "GEFS mean",
            BModel::Gdps => "GDPS",
        }
    }

    /// One line on what the model is good for.
    pub fn blurb(self) -> &'static str {
        match self {
            BModel::Hrrr => "3 km, hourly. The go-to for storm-scale detail out to 18 hours.",
            BModel::Hrrr15 => "The HRRR's own 15-minute output — same model, finer time steps.",
            BModel::Rap => "13 km, hourly. Coarser, but its 0-hour is a true observed analysis.",
            BModel::NamNest => {
                "3 km every 6 hours, on its own dynamics — a second storm-scale opinion."
            }
            BModel::Nam => "12 km every 6 hours — the parent the nest is downscaled from.",
            BModel::Nbm => "Calibrated blend of many models. Probabilities, not a raw forecast.",
            BModel::Rtma => {
                "The real-time surface analysis: what temperature, dewpoint and wind are doing now, 2.5 km, hourly. An analysis, not a forecast."
            }
            BModel::Gfs => "NOAA's global model, every 6 hours.",
            BModel::Ecmwf => "The European centre's open global model, every 6 hours.",
            BModel::GefsMean => "Average of the 31 GEFS members, 0.5°. Smooth by design.",
            BModel::Gdps => "Environment Canada's global model, independent of NCEP and ECMWF.",
        }
    }

    /// Products this model can draw, in the order they read best. Each one is checked against the
    /// model catalogue by a test, so a product cannot be offered for a model that lacks it.
    pub fn products(self) -> &'static [Product] {
        use Product::*;
        match self {
            BModel::Hrrr => &[Reflectivity, Cape, Srh, UpdraftHelicity, Snowfall, Smoke],
            BModel::Hrrr15 => &[Reflectivity],
            BModel::Rap | BModel::NamNest | BModel::Nam => &[Reflectivity, Cape, Srh],
            BModel::Nbm => &[ThunderChance],
            BModel::Rtma => &[
                AnalysisTemp2m,
                AnalysisDewpoint2m,
                AnalysisWind10m,
                AnalysisGust10m,
            ],
            BModel::Gfs | BModel::Ecmwf | BModel::Gdps => {
                &[Temp2m, Dewpoint2m, Wind10m, Mslp, Height500, Moisture]
            }
            // The ensemble mean files carry no dewpoint.
            BModel::GefsMean => &[Temp2m, Wind10m, Mslp, Height500, Moisture],
        }
    }

    pub fn has(self, product: Product) -> bool {
        self.products().contains(&product)
    }

    /// The best product to land on when switching to this model.
    pub fn default_product(self) -> Product {
        self.products()[0]
    }

    /// The widest lead range any run of this model offers, in minutes.
    pub fn leads(self) -> LeadRange {
        self.leads_at_hour(None)
    }

    /// The lead range of the run picked (`None` = the newest that has plausibly posted).
    pub fn leads_for(self, run: Option<DateTime<Utc>>, now: DateTime<Utc>) -> LeadRange {
        let run = run.or_else(|| self.run_choices(now, 1).first().copied());
        self.leads_at_hour(run.map(|r| r.hour()))
    }

    /// Lead range for a run starting at `run_hour` (UTC), or the widest across runs when `None`.
    ///
    /// The extended cycles are where the long leads live: HRRR's 00/06/12/18Z runs go to 48 h and
    /// the rest stop at 18. The steps and limits below were each checked against the servers.
    fn leads_at_hour(self, run_hour: Option<u32>) -> LeadRange {
        let h = |hours: u16| hours * 60;
        // A regional model's limit for this run, from the model catalogue's own schedule.
        let regional = |model: Regional, cap_h: u16| -> u16 {
            let hours = match run_hour {
                Some(hour) => model.max_lead_for_cycle(hour),
                None => model.def().extended_lead_h,
            };
            h(hours.min(cap_h))
        };
        match self {
            BModel::Hrrr => LeadRange::fixed(0, regional(Regional::Hrrr, 48), h(1)),
            // The sub-hourly files stop at 18 h on every cycle.
            BModel::Hrrr15 => LeadRange::fixed(15, h(18), 15),
            BModel::Rap => LeadRange::fixed(0, regional(Regional::Rap, 51), h(1)),
            BModel::NamNest => LeadRange::fixed(0, h(60), h(1)),
            // The 12 km grid is hourly through hour 36, then every three hours to 84.
            BModel::Nam => LeadRange {
                coarse: Some((h(36), h(3))),
                ..LeadRange::fixed(0, h(84), h(1))
            },
            BModel::Nbm => LeadRange::fixed(h(1), h(36), h(1)),
            // An analysis is valid at its own hour: one "lead", zero.
            BModel::Rtma => LeadRange::fixed(0, 0, h(1)),
            BModel::Gfs => LeadRange::fixed(0, h(384), h(3)),
            // Three-hourly to 240 h, then six-hourly to 384 h.
            BModel::GefsMean => LeadRange {
                coarse: Some((h(240), h(6))),
                ..LeadRange::fixed(0, h(384), h(3))
            },
            // Three-hourly to 144 h, then six-hourly; the 00/12Z runs reach 240 h, the 06/18Z runs
            // stop at 144 h.
            BModel::Ecmwf => {
                let max = match run_hour {
                    Some(hour) if hour % 12 != 0 => 144,
                    _ => 240,
                };
                LeadRange {
                    coarse: Some((h(144), h(6))),
                    ..LeadRange::fixed(0, h(max), h(3))
                }
            }
            // Unverified past five days: the Canadian server was not answering when this was
            // checked, so it keeps its original range.
            BModel::Gdps => LeadRange::fixed(0, h(120), h(3)),
        }
    }

    /// The cycles a person can pick, newest plausible first. Hourly models list a day of runs; the
    /// six-hourly ones list two days.
    pub fn run_choices(self, now: DateTime<Utc>, count: usize) -> Vec<DateTime<Utc>> {
        match self.engine() {
            Engine::Regional(model) => wxdata::hrrr::run_choices(model, now, count),
            Engine::Sub15 => wxdata::hrrr::run_choices(Regional::Hrrr, now, count),
            Engine::Global(model) => model.run_choices(now, count),
            Engine::Analysis => wxdata::rtma::run_choices(now, count),
        }
    }

    /// How many runs the picker lists for this model.
    pub fn run_list_len(self) -> usize {
        match self.engine() {
            Engine::Regional(model) if model.def().cycle_hours == 1 => 24,
            Engine::Sub15 | Engine::Analysis => 24,
            _ => 8,
        }
    }

    /// A run in a picker: "18Z Sun 20 Sep · to F+48h".
    pub fn run_label(self, run: DateTime<Utc>) -> String {
        let reach = self.leads_at_hour(Some(run.hour())).max / 60;
        if reach == 0 {
            // An analysis is the picture at its own hour; there is nothing to reach toward.
            return format!("{:02}Z {} · analysis", run.hour(), run.format("%a %d %b"));
        }
        format!(
            "{:02}Z {} · to F+{reach}h",
            run.hour(),
            run.format("%a %d %b")
        )
    }

    /// The browser model for a regional model, for models with a reflectivity forecast; anything
    /// else (which has none) maps to the HRRR.
    pub fn from_regional(model: Regional) -> BModel {
        match model {
            Regional::Rap => BModel::Rap,
            Regional::NamNest => BModel::NamNest,
            Regional::Nam => BModel::Nam,
            _ => BModel::Hrrr,
        }
    }

    /// Which group this model sits in, in a picker.
    pub fn family(self) -> Family {
        match self.engine() {
            Engine::Global(_) => Family::Global,
            Engine::Analysis => Family::Analysis,
            Engine::Regional(_) | Engine::Sub15 => Family::StormScale,
        }
    }

    /// Whether this model has a forecast lead to scrub. An analysis does not.
    pub fn has_lead(self) -> bool {
        let r = self.leads();
        r.min != r.max
    }

    /// The model whose lead and fetch clocks a regional pick drives, if any.
    pub fn regional_model(self) -> Option<Regional> {
        match self.engine() {
            Engine::Regional(model) => Some(model),
            Engine::Sub15 => Some(Regional::Hrrr),
            Engine::Global(_) | Engine::Analysis => None,
        }
    }

    pub fn global_model(self) -> Option<GlobalModel> {
        match self.engine() {
            Engine::Global(model) => Some(model),
            _ => None,
        }
    }
}

impl Product {
    pub const ALL: [Product; 17] = [
        Product::Reflectivity,
        Product::Cape,
        Product::Srh,
        Product::UpdraftHelicity,
        Product::Snowfall,
        Product::Smoke,
        Product::ThunderChance,
        Product::Temp2m,
        Product::Dewpoint2m,
        Product::Wind10m,
        Product::Mslp,
        Product::Height500,
        Product::Moisture,
        Product::AnalysisTemp2m,
        Product::AnalysisDewpoint2m,
        Product::AnalysisWind10m,
        Product::AnalysisGust10m,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Product::Reflectivity => "Reflectivity",
            Product::Cape => "CAPE",
            Product::Srh => "Helicity (SRH)",
            Product::UpdraftHelicity => "Rotation tracks",
            Product::Snowfall => "Snowfall",
            Product::Smoke => "Smoke",
            Product::ThunderChance => "Thunder chance",
            Product::Temp2m => "Temperature",
            Product::Dewpoint2m => "Dewpoint",
            Product::Wind10m => "Wind",
            Product::Mslp => "Pressure",
            Product::Height500 => "500 hPa height",
            Product::Moisture => "Moisture",
            // The model already says "RTMA", so the chip only needs the quantity.
            Product::AnalysisTemp2m => "Temperature",
            Product::AnalysisDewpoint2m => "Dewpoint",
            Product::AnalysisWind10m => "Wind",
            Product::AnalysisGust10m => "Gusts",
        }
    }

    /// The name in the layers list, where the model is not part of the row: "Storm fuel (CAPE)"
    /// says what it is, and the model is whichever one is picked.
    pub fn row_label(self) -> &'static str {
        match self {
            Product::Reflectivity => "Reflectivity forecast",
            Product::Cape => "Storm fuel (CAPE)",
            Product::Srh => "Storm spin (SRH)",
            Product::UpdraftHelicity => "Future rotation tracks",
            Product::Snowfall => "Forecast snowfall",
            Product::Smoke => "Wildfire smoke",
            Product::ThunderChance => "Chance of thunder",
            Product::Temp2m => "Surface temperature (2 m)",
            Product::Dewpoint2m => "Surface dewpoint (2 m)",
            Product::Wind10m => "Surface wind (10 m)",
            Product::Mslp => "Surface pressure (MSLP)",
            Product::Height500 => "Upper-level pattern (500 hPa height)",
            Product::Moisture => "Moisture in the air column",
            Product::AnalysisTemp2m => "Surface temperature (RTMA analysis)",
            Product::AnalysisDewpoint2m => "Surface dewpoint (RTMA analysis)",
            Product::AnalysisWind10m => "Surface wind (RTMA analysis)",
            Product::AnalysisGust10m => "Wind gusts (RTMA analysis)",
        }
    }

    /// Plain-language one-liner, for tooltips and the layers list.
    pub fn blurb(self) -> &'static str {
        match self {
            Product::Reflectivity => {
                "Forecast radar picture — what the model thinks radar will show"
            }
            Product::Cape => "Storm fuel: how much energy the atmosphere has for updrafts",
            Product::Srh => "Storm spin: how much rotation the wind profile can feed a storm",
            Product::UpdraftHelicity => {
                "Where storms are forecast to rotate, as a swath through the hour"
            }
            Product::Snowfall => "Snow forecast to pile up by this lead",
            Product::Smoke => "Forecast smoke near the ground, from active fires",
            Product::ThunderChance => "Chance of a thunderstorm in the hour ending at this lead",
            Product::Temp2m => "Air temperature at 2 m",
            Product::Dewpoint2m => "How much moisture the air is carrying, at 2 m",
            Product::Wind10m => "Surface wind at 10 m",
            Product::Mslp => "Surface pressure reduced to sea level — highs and lows",
            Product::Height500 => "The steering flow: troughs and ridges at 500 hPa",
            Product::Moisture => {
                "Column moisture: precipitable water (GFS, GEFS) or precipitation (ECMWF, GDPS)"
            }
            Product::AnalysisTemp2m => {
                "What the surface temperature is right now, between stations"
            }
            Product::AnalysisDewpoint2m => {
                "What the surface dewpoint is right now — where the moisture sits"
            }
            Product::AnalysisWind10m => "Surface wind speed right now, analyzed from observations",
            Product::AnalysisGust10m => "Analyzed wind gusts at 10 m",
        }
    }

    /// The renderer layer that draws this product.
    pub fn layer(self) -> FieldLayer {
        match self {
            Product::Reflectivity => FieldLayer::Hrrr,
            Product::Cape => FieldLayer::Cape,
            Product::Srh => FieldLayer::Srh,
            Product::UpdraftHelicity => FieldLayer::UpdraftHelicity,
            Product::Snowfall => FieldLayer::Snowfall,
            Product::Smoke => FieldLayer::Smoke,
            Product::ThunderChance => FieldLayer::ThunderProb,
            Product::Temp2m => FieldLayer::GlobalTemp2m,
            Product::Dewpoint2m => FieldLayer::GlobalDewpoint2m,
            Product::Wind10m => FieldLayer::GlobalWind10m,
            Product::Mslp => FieldLayer::GlobalMslp,
            Product::Height500 => FieldLayer::GlobalHeight500,
            Product::Moisture => FieldLayer::GlobalPrecip,
            Product::AnalysisTemp2m => FieldLayer::RtmaTemp2m,
            Product::AnalysisDewpoint2m => FieldLayer::RtmaDewpoint2m,
            Product::AnalysisWind10m => FieldLayer::RtmaWind10m,
            Product::AnalysisGust10m => FieldLayer::RtmaGust10m,
        }
    }

    /// Inverse of [`Self::layer`].
    pub fn from_layer(layer: FieldLayer) -> Option<Product> {
        Product::ALL.into_iter().find(|p| p.layer() == layer)
    }

    /// The catalogue field behind a regional product, for checking availability.
    #[cfg(test)]
    fn model_field(self) -> Option<wxdata::model::ModelField> {
        use wxdata::model::ModelField;
        Some(match self {
            Product::Reflectivity => ModelField::CompositeReflectivity,
            Product::Cape => ModelField::SurfaceCape,
            Product::Srh => ModelField::Srh3km,
            Product::UpdraftHelicity => ModelField::UpdraftHelicity,
            Product::Snowfall => ModelField::Snowfall,
            Product::Smoke => ModelField::Smoke,
            Product::ThunderChance => ModelField::ThunderProbability,
            _ => return None,
        })
    }
}

/// Every layer a model choice can put on the map, so turning the model off (or changing product)
/// clears exactly these and nothing else.
pub fn model_layers() -> impl Iterator<Item = FieldLayer> {
    Product::ALL.into_iter().map(Product::layer)
}

/// The comparison that puts the selected model against its natural counterpart, if there is one:
/// HRRR against RAP for the regional products both publish, GFS against ECMWF for the global
/// fields both publish. Anything else has no comparison to offer.
pub fn compare_field(sel: Selection) -> Option<(crate::fielddiff::DiffField, BModel)> {
    use crate::fielddiff::{DiffField, GlobalFieldKind};
    let regional = match sel.product {
        Product::Reflectivity => Some(DiffField::Reflectivity),
        Product::Cape => Some(DiffField::Cape),
        Product::Srh => Some(DiffField::Srh),
        _ => None,
    };
    if let Some(field) = regional {
        return match sel.model {
            BModel::Hrrr => Some((field, BModel::Rap)),
            BModel::Rap => Some((field, BModel::Hrrr)),
            _ => None,
        };
    }
    let kind = match sel.product {
        Product::Mslp => GlobalFieldKind::Mslp,
        Product::Height500 => GlobalFieldKind::Height500,
        Product::Temp2m => GlobalFieldKind::Temp2m,
        Product::Dewpoint2m => GlobalFieldKind::Dewpoint2m,
        Product::Wind10m => GlobalFieldKind::Wind10m,
        _ => return None,
    };
    match sel.model {
        BModel::Gfs => Some((DiffField::Global(kind), BModel::Ecmwf)),
        BModel::Ecmwf => Some((DiffField::Global(kind), BModel::Gfs)),
        _ => None,
    }
}

/// The current choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub model: BModel,
    pub product: Product,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            model: BModel::Hrrr,
            product: Product::Reflectivity,
        }
    }
}

impl Selection {
    /// Choose a model, keeping the current product if the model has it.
    pub fn with_model(self, model: BModel) -> Selection {
        Selection {
            model,
            product: if model.has(self.product) {
                self.product
            } else {
                model.default_product()
            },
        }
    }

    /// Choose a product. If the current model lacks it, move to the first model that has it,
    /// preferring the models a forecaster would reach for first.
    pub fn with_product(self, product: Product) -> Selection {
        if self.model.has(product) {
            return Selection {
                model: self.model,
                product,
            };
        }
        let model = BModel::ALL
            .into_iter()
            .find(|m| m.has(product))
            .unwrap_or(self.model);
        Selection { model, product }
    }

    pub fn layer(self) -> FieldLayer {
        self.product.layer()
    }

    /// Stable text for saving: `model/product` slugs.
    pub fn slug(self) -> String {
        format!("{}/{}", model_slug(self.model), product_slug(self.product))
    }

    pub fn from_slug(s: &str) -> Option<Selection> {
        let (m, p) = s.split_once('/')?;
        let model = BModel::ALL.into_iter().find(|x| model_slug(*x) == m)?;
        let product = Product::ALL.into_iter().find(|x| product_slug(*x) == p)?;
        model.has(product).then_some(Selection { model, product })
    }
}

fn model_slug(m: BModel) -> &'static str {
    match m {
        BModel::Hrrr => "hrrr",
        BModel::Hrrr15 => "hrrr-15",
        BModel::Rap => "rap",
        BModel::NamNest => "nam-nest",
        BModel::Nam => "nam",
        BModel::Nbm => "nbm",
        BModel::Rtma => "rtma",
        BModel::Gfs => "gfs",
        BModel::Ecmwf => "ecmwf",
        BModel::GefsMean => "gefs-mean",
        BModel::Gdps => "gdps",
    }
}

fn product_slug(p: Product) -> &'static str {
    match p {
        Product::Reflectivity => "reflectivity",
        Product::Cape => "cape",
        Product::Srh => "srh",
        Product::UpdraftHelicity => "uh",
        Product::Snowfall => "snowfall",
        Product::Smoke => "smoke",
        Product::ThunderChance => "thunder",
        Product::Temp2m => "t2m",
        Product::Dewpoint2m => "td2m",
        Product::Wind10m => "wind10m",
        Product::Mslp => "mslp",
        Product::Height500 => "gh500",
        Product::Moisture => "moisture",
        Product::AnalysisTemp2m => "rtma-t2m",
        Product::AnalysisDewpoint2m => "rtma-td2m",
        Product::AnalysisWind10m => "rtma-wind10m",
        Product::AnalysisGust10m => "rtma-gust10m",
    }
}

/// `F+3h`, `F+45m`, `F+1h15m`.
pub fn format_lead(minutes: u16) -> String {
    let (h, m) = (minutes / 60, minutes % 60);
    match (h, m) {
        (0, 0) => "F+0h".into(),
        (0, m) => format!("F+{m}m"),
        (h, 0) => format!("F+{h}h"),
        (h, m) => format!("F+{h}h{m:02}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_model_has_products_and_a_sane_lead_range() {
        for m in BModel::ALL {
            assert!(!m.products().is_empty(), "{m:?}");
            let r = m.leads();
            assert!(r.min <= r.max && r.step > 0, "{m:?}");
            assert_eq!(
                (r.max - r.min) % r.step,
                0,
                "{m:?}: max is on the step grid"
            );
            assert_eq!(r.clamp(r.min), r.min);
            assert_eq!(r.clamp(u16::MAX), r.max);
        }
    }

    /// The point of the catalogue: a product is only offered where the model catalogue says the
    /// model publishes it, so the picker cannot offer something that will 404.
    #[test]
    fn regional_products_are_published_by_their_model() {
        for m in BModel::ALL {
            let Some(regional) = m.regional_model() else {
                continue;
            };
            for p in m.products() {
                let field = p
                    .model_field()
                    .unwrap_or_else(|| panic!("{m:?}/{p:?} is not a regional product"));
                assert!(
                    field.grib(regional).is_some(),
                    "{} does not publish {}",
                    regional.label(),
                    field.label()
                );
            }
        }
    }

    #[test]
    fn global_models_only_offer_global_products() {
        for m in BModel::ALL {
            if m.global_model().is_none() {
                continue;
            }
            for p in m.products() {
                assert!(p.model_field().is_none(), "{m:?} offers regional {p:?}");
            }
        }
    }

    #[test]
    fn reflectivity_is_offered_wherever_it_exists_and_only_there() {
        let with: Vec<_> = BModel::ALL
            .into_iter()
            .filter(|m| m.has(Product::Reflectivity))
            .collect();
        assert_eq!(
            with,
            [
                BModel::Hrrr,
                BModel::Hrrr15,
                BModel::Rap,
                BModel::NamNest,
                BModel::Nam
            ]
        );
    }

    #[test]
    fn products_map_to_distinct_layers_and_back() {
        let mut layers: Vec<_> = model_layers().collect();
        let n = layers.len();
        layers.sort_by_key(|l| l.slug());
        layers.dedup();
        assert_eq!(layers.len(), n);
        for p in Product::ALL {
            assert_eq!(Product::from_layer(p.layer()), Some(p));
        }
    }

    #[test]
    fn switching_model_keeps_the_product_when_it_can() {
        let s = Selection::default().with_model(BModel::Rap);
        assert_eq!(s.product, Product::Reflectivity);
        // The NBM has no reflectivity, so it lands on its own first product.
        let s = s.with_model(BModel::Nbm);
        assert_eq!(s.product, Product::ThunderChance);
        // And going back does not remember the old one; it takes the new model's default.
        assert_eq!(s.with_model(BModel::Hrrr).product, Product::Reflectivity);
    }

    #[test]
    fn asking_for_a_product_moves_to_a_model_that_has_it() {
        let s = Selection::default().with_product(Product::Mslp);
        assert!(s.model.has(Product::Mslp) && s.product == Product::Mslp);
        let s = Selection {
            model: BModel::Gfs,
            product: Product::Mslp,
        }
        .with_product(Product::Cape);
        assert!(s.model.has(Product::Cape));
        // A model that already has it stays put.
        let s = Selection {
            model: BModel::Rap,
            product: Product::Reflectivity,
        }
        .with_product(Product::Cape);
        assert_eq!(s.model, BModel::Rap);
    }

    #[test]
    fn selections_round_trip_through_their_slug_and_bad_ones_are_rejected() {
        for m in BModel::ALL {
            for p in m.products() {
                let s = Selection {
                    model: m,
                    product: *p,
                };
                assert_eq!(Selection::from_slug(&s.slug()), Some(s));
            }
        }
        assert_eq!(Selection::from_slug("nbm/reflectivity"), None);
        assert_eq!(Selection::from_slug("nonsense"), None);
    }

    #[test]
    fn only_real_pairs_get_a_comparison() {
        use crate::fielddiff::DiffField;
        let sel = |model, product| Selection { model, product };
        assert_eq!(
            compare_field(sel(BModel::Hrrr, Product::Reflectivity)),
            Some((DiffField::Reflectivity, BModel::Rap))
        );
        assert_eq!(
            compare_field(sel(BModel::Rap, Product::Cape)),
            Some((DiffField::Cape, BModel::Hrrr))
        );
        assert_eq!(
            compare_field(sel(BModel::Ecmwf, Product::Mslp)).map(|c| c.1),
            Some(BModel::Gfs)
        );
        // The NAMs have no comparison, and the ensemble mean is not a peer of the deterministic runs.
        assert_eq!(
            compare_field(sel(BModel::NamNest, Product::Reflectivity)),
            None
        );
        assert_eq!(compare_field(sel(BModel::GefsMean, Product::Mslp)), None);
        // Moisture is PWAT on one model and precipitation on the other: not comparable.
        assert_eq!(compare_field(sel(BModel::Gfs, Product::Moisture)), None);
        // And a comparison always names a model that really has that product.
        for m in BModel::ALL {
            for p in m.products() {
                if let Some((_, other)) = compare_field(sel(m, *p)) {
                    assert!(other.has(*p), "{m:?}/{p:?} compares against {other:?}");
                }
            }
        }
    }

    fn at(h: u32) -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.with_ymd_and_hms(2026, 9, 20, h, 0, 0).unwrap()
    }

    #[test]
    fn thinning_leads_snap_and_step_on_the_grid_the_model_publishes() {
        let h = |x: u16| x * 60;
        let nam = BModel::Nam.leads();
        // Hourly to 36 h, then every three hours: 37 and 38 h do not exist, 39 h does.
        assert_eq!(nam.clamp(h(37)), h(36));
        assert_eq!(nam.clamp(h(38)), h(36));
        assert_eq!(nam.clamp(h(39)), h(39));
        assert_eq!(nam.neighbour(h(36), true), h(39));
        assert_eq!(nam.neighbour(h(39), false), h(36));
        assert_eq!(nam.neighbour(h(35), true), h(36));
        assert_eq!(nam.neighbour(h(84), true), h(84), "stays inside the range");
        assert_eq!(nam.neighbour(0, false), 0);
        // GEFS: three-hourly to 240 h, six-hourly after.
        let gefs = BModel::GefsMean.leads();
        assert_eq!(gefs.neighbour(h(240), true), h(246));
        assert_eq!(gefs.neighbour(h(246), false), h(240));
        assert_eq!(gefs.clamp(h(243)), h(240));
        assert_eq!(gefs.clamp(h(999)), h(384));
    }

    #[test]
    fn every_step_from_the_start_lands_on_a_published_lead_and_reaches_the_end() {
        for m in BModel::ALL {
            let r = m.leads();
            let mut lead = r.min;
            let mut steps = 0;
            while lead < r.max {
                let next = r.neighbour(lead, true);
                assert!(next > lead, "{m:?} stuck at {lead}");
                assert_eq!(r.clamp(next), next, "{m:?}: {next} is off the grid");
                lead = next;
                steps += 1;
                assert!(steps < 1000);
            }
            assert_eq!(lead, r.max, "{m:?} did not reach its last lead");
        }
    }

    /// The long leads live on particular cycles, so the range has to follow the run.
    #[test]
    fn the_lead_range_follows_the_run() {
        let max = |m: BModel, hour: u32| m.leads_at_hour(Some(hour)).max / 60;
        assert_eq!(max(BModel::Hrrr, 12), 48, "an extended HRRR run");
        assert_eq!(max(BModel::Hrrr, 13), 18, "an ordinary HRRR run");
        assert_eq!(max(BModel::Rap, 15), 51);
        assert_eq!(max(BModel::Rap, 16), 21);
        assert_eq!(max(BModel::Ecmwf, 0), 240);
        assert_eq!(max(BModel::Ecmwf, 12), 240);
        assert_eq!(max(BModel::Ecmwf, 6), 144, "the 06/18Z runs stop earlier");
        // The widest answer, for a run not yet known, is the extended one.
        assert_eq!(BModel::Hrrr.leads().max / 60, 48);
        // The sub-hourly files never go past 18 h.
        assert_eq!(max(BModel::Hrrr15, 12), 18);
    }

    #[test]
    fn latest_uses_the_newest_runs_own_range() {
        use chrono::TimeZone;
        // 21:30Z: the newest plausible HRRR run is 20Z, which is not an extended cycle.
        let now = Utc.with_ymd_and_hms(2026, 9, 20, 21, 30, 0).unwrap();
        assert_eq!(BModel::Hrrr.leads_for(None, now).max / 60, 18);
        // Pinning the 18Z run opens up the long leads.
        assert_eq!(BModel::Hrrr.leads_for(Some(at(18)), now).max / 60, 48);
    }

    #[test]
    fn run_lists_sit_on_each_models_own_cycles_newest_first() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 9, 20, 21, 30, 0).unwrap();
        let hourly = BModel::Hrrr.run_choices(now, 4);
        assert_eq!(hourly, [at(20), at(19), at(18), at(17)]);
        for m in [BModel::NamNest, BModel::Nam] {
            let runs = m.run_choices(now, 4);
            assert!(runs.iter().all(|r| r.hour() % 6 == 0), "{m:?}: {runs:?}");
            assert!(runs
                .windows(2)
                .all(|w| w[0] - w[1] == chrono::Duration::hours(6)));
        }
        for m in [BModel::Gfs, BModel::Ecmwf, BModel::GefsMean, BModel::Gdps] {
            let runs = m.run_choices(now, 4);
            assert!(runs.iter().all(|r| r.hour() % 6 == 0), "{m:?}: {runs:?}");
            // Nothing newer than the model can plausibly have finished.
            assert!(runs[0] < now - chrono::Duration::hours(4), "{m:?}");
        }
        // A day of hourly runs, two days of six-hourly ones.
        assert_eq!(BModel::Hrrr.run_list_len(), 24);
        assert_eq!(BModel::Gfs.run_list_len(), 8);
    }

    #[test]
    fn a_run_label_says_when_and_how_far() {
        assert_eq!(BModel::Hrrr.run_label(at(18)), "18Z Sun 20 Sep · to F+48h");
        assert_eq!(BModel::Hrrr.run_label(at(17)), "17Z Sun 20 Sep · to F+18h");
    }

    #[test]
    fn rtma_is_an_analysis_with_no_lead_and_products_of_its_own() {
        use crate::render::FieldLayer as FL;
        let rtma = BModel::Rtma;
        assert_eq!(rtma.family(), Family::Analysis);
        assert!(!rtma.has_lead(), "an analysis is valid at its own hour");
        assert_eq!(rtma.leads().clamp(500), 0);
        assert_eq!(rtma.leads().neighbour(0, true), 0);
        // Every forecast model has a lead to scrub; only the analysis does not.
        for m in BModel::ALL {
            assert_eq!(m.has_lead(), m != BModel::Rtma, "{m:?}");
        }
        // Its products are the analysis layers, and nobody else offers them.
        for p in rtma.products() {
            assert!(matches!(
                p.layer(),
                FL::RtmaTemp2m | FL::RtmaDewpoint2m | FL::RtmaWind10m | FL::RtmaGust10m
            ));
            for m in BModel::ALL.into_iter().filter(|m| *m != rtma) {
                assert!(!m.has(*p), "{m:?} offers the analysis product {p:?}");
            }
        }
        // Its run picker is hourly analyses, labelled as such rather than as a forecast reach.
        assert!(
            rtma.run_label(at(20)).ends_with("analysis"),
            "{}",
            rtma.run_label(at(20))
        );
        assert_eq!(rtma.run_list_len(), 24);
        // The groups partition the models: each lands in exactly one.
        for m in BModel::ALL {
            let n = Family::ALL.iter().filter(|f| m.family() == **f).count();
            assert_eq!(n, 1, "{m:?}");
        }
    }

    #[test]
    fn analysis_hours_come_from_the_analysis_feed() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 9, 20, 21, 30, 0).unwrap();
        assert_eq!(
            BModel::Rtma.run_choices(now, 3),
            [at(20), at(19), at(18)],
            "hourly, newest first, behind the posting latency"
        );
    }

    #[test]
    fn lead_snaps_to_the_models_own_steps() {
        let r = BModel::Hrrr15.leads();
        assert_eq!(r.clamp(0), 15);
        assert_eq!(r.clamp(50), 45);
        let g = BModel::Gfs.leads();
        assert_eq!(g.clamp(4 * 60), 3 * 60);
        assert_eq!(g.clamp(999 * 60), 384 * 60);
    }

    #[test]
    fn leads_read_naturally() {
        assert_eq!(format_lead(0), "F+0h");
        assert_eq!(format_lead(45), "F+45m");
        assert_eq!(format_lead(180), "F+3h");
        assert_eq!(format_lead(75), "F+1h15m");
    }
}
