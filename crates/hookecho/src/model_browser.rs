//! What "the models" are to a person: pick a **model**, then a **product** it publishes, then a
//! **lead**. Reflectivity is just one product, offered by every model that has it; the HRRR's
//! 15-minute output is a model of its own rather than a switch on another layer.
//!
//! This is only the catalogue: which products each model can show, the layer that draws each, and
//! how far out and in what steps it can be scrubbed. The app maps a choice onto the renderer's
//! existing field layers, so nothing here fetches or draws.

use crate::render::FieldLayer;
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
}

/// How a model is fetched, which decides which of the app's clocks its lead lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// A regional NCEP model read through the shared GRIB path.
    Regional(Regional),
    /// The HRRR sub-hourly product.
    Sub15,
    Global(GlobalModel),
}

/// How far out a model can be scrubbed, in minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeadRange {
    pub min: u16,
    pub max: u16,
    pub step: u16,
}

impl LeadRange {
    /// Snap a lead into this range, on its step grid.
    pub fn clamp(self, minutes: u16) -> u16 {
        let m = minutes.clamp(self.min, self.max);
        let snapped = self.min + (m - self.min) / self.step * self.step;
        snapped.min(self.max)
    }
}

impl BModel {
    pub const ALL: [BModel; 10] = [
        BModel::Hrrr,
        BModel::Hrrr15,
        BModel::Rap,
        BModel::NamNest,
        BModel::Nam,
        BModel::Nbm,
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

    /// Lead range in minutes. Limits are what the fetch path can reach today, not the models'
    /// full extended runs.
    pub fn leads(self) -> LeadRange {
        let hours = |min: u16, max: u16, step: u16| LeadRange {
            min: min * 60,
            max: max * 60,
            step: step * 60,
        };
        match self {
            BModel::Hrrr => hours(0, 18, 1),
            BModel::Hrrr15 => LeadRange {
                min: 15,
                max: 18 * 60,
                step: 15,
            },
            BModel::Rap => hours(0, 21, 1),
            BModel::NamNest => hours(0, 60, 1),
            // The 12 km grid is hourly only through hour 36.
            BModel::Nam => hours(0, 36, 1),
            BModel::Nbm => hours(1, 36, 1),
            BModel::Gfs | BModel::Ecmwf | BModel::GefsMean | BModel::Gdps => hours(0, 120, 3),
        }
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

    /// Model family for grouping in a picker.
    pub fn regional(self) -> bool {
        !matches!(self.engine(), Engine::Global(_))
    }

    /// The model whose lead and fetch clocks a regional pick drives, if any.
    pub fn regional_model(self) -> Option<Regional> {
        match self.engine() {
            Engine::Regional(model) => Some(model),
            Engine::Sub15 => Some(Regional::Hrrr),
            Engine::Global(_) => None,
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
    pub const ALL: [Product; 13] = [
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
    fn lead_snaps_to_the_models_own_steps() {
        let r = BModel::Hrrr15.leads();
        assert_eq!(r.clamp(0), 15);
        assert_eq!(r.clamp(50), 45);
        let g = BModel::Gfs.leads();
        assert_eq!(g.clamp(4 * 60), 3 * 60);
        assert_eq!(g.clamp(999 * 60), 120 * 60);
    }

    #[test]
    fn leads_read_naturally() {
        assert_eq!(format_lead(0), "F+0h");
        assert_eq!(format_lead(45), "F+45m");
        assert_eq!(format_lead(180), "F+3h");
        assert_eq!(format_lead(75), "F+1h15m");
    }
}
