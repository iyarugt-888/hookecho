//! Phase F1: model definitions and field mappings as *data*, not as code paths.
//!
//! The app already fetches from six NWP sources, but everything that distinguishes them lived in
//! `match` arms: the cycle spacing in one, the grid resolution in another, and — the expensive
//! one — the GRIB variable and level strings as literals inside the UI's own layer dispatch, in
//! `app.rs`, `fielddiff.rs`, `severe.rs` and `headless.rs` independently. Adding a model meant
//! editing every one of those, and asking "does the NBM publish updraft helicity?" had no answer
//! short of firing a request and reading the error.
//!
//! This module is the table those questions should be asked of. `ROADMAP_NEW.md` §F's acceptance
//! criterion is that adding a model with an already-supported GRIB format should be a definition
//! plus field mappings and not a new renderer; [`ModelDef`] is the definition and
//! [`ModelField::grib`] is the mapping.
//!
//! **What this is not.** It does not add a model, a field, or a renderer, and it is not Phase F.
//! The fetch/regrid machinery still lives in [`crate::hrrr`] and is unchanged; this is the
//! metadata layer F1 asks for, which F2–F8 would build on.

use crate::field::{
    DataSource, FieldDescriptor, FieldFamily, FieldId, GeographicBounds, PaletteId, Unit, ValueKind,
};
use crate::hrrr::Model;

/// Where a model's grid covers. Used to reject a point before spending a request on it, and to
/// answer "which models can I even ask about this location".
pub use crate::field::GeographicBounds as Domain;

/// A model's role in an ensemble, for F7. Every model wired up today is deterministic; the field
/// exists so an ensemble member can be added as a definition rather than as a parallel code path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ensemble {
    Deterministic,
    /// One member of a named ensemble, with its member number.
    Member(&'static str, u8),
    /// Statistically post-processed guidance derived from an ensemble of models — the NBM. Not a
    /// member and not a raw deterministic run, and the distinction matters: its fields are
    /// calibrated probabilities and percentiles, so differencing it against a raw model is
    /// comparing two different kinds of number.
    PostProcessed,
}

/// Everything about a model that is metadata rather than behaviour.
///
/// The URL layout and regrid resolution stay in [`crate::hrrr::Model`], which already owns them;
/// duplicating them here would just create two places to be wrong.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelDef {
    /// Stable machine id — what a saved workspace or a URL parameter should carry, unlike
    /// [`Self::label`], which is prose and may be reworded.
    pub id: &'static str,
    pub label: &'static str,
    /// Hours between cycles.
    pub cycle_hours: u32,
    /// Longest forecast lead published on an ordinary cycle, hours.
    pub max_lead_h: u16,
    /// Longest lead on an *extended* cycle, and which cycles those are. HRRR is the reason this
    /// exists: it runs to 18 h four times a day and to 48 h on 00/06/12/18Z, and the app has
    /// been clamping every request to 18 regardless — silently truncating three quarters of the
    /// longest HRRR runs. Empty means every cycle is the same length.
    pub extended_lead_h: u16,
    pub extended_cycles: &'static [u32],
    /// Native horizontal grid spacing, km. What a "is this resolving the storm or smoothing it"
    /// question is actually about.
    pub grid_km: f32,
    pub domain: Domain,
    /// Typical minutes from cycle time to the file being on the wire. An estimate from observed
    /// posting behaviour, used to decide how far back to start looking — not a guarantee, and
    /// not a substitute for actually trying the fetch.
    pub typical_latency_min: u16,
    pub ensemble: Ensemble,
}

impl Model {
    /// This model's definition.
    pub fn def(self) -> &'static ModelDef {
        DEFS.iter()
            .find(|d| d.model == self)
            .map(|d| &d.def)
            // Unreachable by construction: `every_model_has_a_definition` proves the table is
            // total over the enum, so this cannot fire without that test failing first.
            .expect("every Model has a ModelDef")
    }

    /// Look a model up by its stable [`ModelDef::id`].
    pub fn from_id(id: &str) -> Option<Self> {
        DEFS.iter().find(|e| e.def.id == id).map(|e| e.model)
    }

    /// Longest forecast hour this model publishes from `cycle_hour` (UTC hour of the run).
    ///
    /// Replaces the hard-coded `.min(18)` the fetch path applied to every model.
    pub fn max_lead_for_cycle(self, cycle_hour: u32) -> u16 {
        let d = self.def();
        if d.extended_cycles.contains(&cycle_hour) {
            d.extended_lead_h
        } else {
            d.max_lead_h
        }
    }
}

struct Entry {
    model: Model,
    def: ModelDef,
}

/// The definition table. One row per model; adding a row is the whole cost of teaching the rest
/// of the app that a model exists.
static DEFS: &[Entry] = &[
    Entry {
        model: Model::Hrrr,
        def: ModelDef {
            id: "hrrr",
            label: "HRRR",
            cycle_hours: 1,
            max_lead_h: 18,
            extended_lead_h: 48,
            extended_cycles: &[0, 6, 12, 18],
            grid_km: 3.0,
            domain: GeographicBounds::CONUS,
            typical_latency_min: 50,
            ensemble: Ensemble::Deterministic,
        },
    },
    Entry {
        model: Model::HrrrPressure,
        def: ModelDef {
            id: "hrrr-prs",
            label: "HRRR pressure",
            cycle_hours: 1,
            max_lead_h: 18,
            extended_lead_h: 48,
            extended_cycles: &[0, 6, 12, 18],
            grid_km: 3.0,
            domain: GeographicBounds::CONUS,
            typical_latency_min: 55,
            ensemble: Ensemble::Deterministic,
        },
    },
    Entry {
        model: Model::Rap,
        def: ModelDef {
            id: "rap",
            label: "RAP",
            cycle_hours: 1,
            max_lead_h: 21,
            extended_lead_h: 51,
            extended_cycles: &[3, 9, 15, 21],
            grid_km: 13.0,
            domain: GeographicBounds::CONUS,
            typical_latency_min: 55,
            ensemble: Ensemble::Deterministic,
        },
    },
    Entry {
        model: Model::NamNest,
        def: ModelDef {
            id: "nam-nest",
            label: "NAM 3 km nest",
            cycle_hours: 6,
            max_lead_h: 60,
            extended_lead_h: 60,
            extended_cycles: &[],
            grid_km: 3.0,
            domain: GeographicBounds::CONUS,
            typical_latency_min: 85,
            ensemble: Ensemble::Deterministic,
        },
    },
    Entry {
        model: Model::Nam,
        def: ModelDef {
            id: "nam",
            label: "NAM 12 km",
            cycle_hours: 6,
            max_lead_h: 84,
            extended_lead_h: 84,
            extended_cycles: &[],
            grid_km: 12.0,
            domain: GeographicBounds::CONUS,
            typical_latency_min: 80,
            ensemble: Ensemble::Deterministic,
        },
    },
    Entry {
        model: Model::Nbm,
        def: ModelDef {
            id: "nbm",
            label: "NBM",
            cycle_hours: 1,
            max_lead_h: 264,
            extended_lead_h: 264,
            extended_cycles: &[],
            grid_km: 2.5,
            domain: GeographicBounds::CONUS,
            typical_latency_min: 75,
            ensemble: Ensemble::PostProcessed,
        },
    },
];

/// A field by meaning, independent of how any one model spells it in GRIB.
///
/// The variants are the fields the app fetches today. The point is not the list's length — it is
/// that [`Self::grib`] is now the single place a GRIB name appears, so a second model publishing
/// the same field costs one table row instead of a new branch in every caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelField {
    /// Composite reflectivity — "future radar".
    CompositeReflectivity,
    SurfaceCape,
    MixedLayerCape,
    /// Storm-relative helicity, 0–1 km.
    Srh1km,
    /// Storm-relative helicity, 0–3 km.
    Srh3km,
    /// Hourly maximum updraft helicity, 2–5 km — the rotation-track proxy.
    UpdraftHelicity,
    /// Accumulated snowfall since the run started.
    Snowfall,
    /// Calibrated probability of thunder over the hour ending at the lead.
    ThunderProbability,
    /// Near-surface smoke mass density.
    Smoke,
    MeanSeaLevelPressure,
    Temperature2m,
    Dewpoint2m,
}

/// How one model spells one field in its GRIB `.idx`, plus the floor below which decoded values
/// are not real data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GribKey {
    pub var: &'static str,
    pub level: &'static str,
    /// Values below this are dropped as missing rather than plotted. `NEG_INFINITY` keeps
    /// everything, which is right for a signed field like helicity where negative is a physical
    /// reading and not an absence.
    pub min_valid: f64,
}

impl ModelField {
    pub fn label(self) -> &'static str {
        self.descriptor().name
    }

    /// Source-independent field metadata shared by every model that publishes this quantity.
    /// Provider-specific availability and GRIB spelling remain in [`Self::grib`].
    pub fn descriptor(self) -> &'static FieldDescriptor {
        FIELD_DEFS
            .iter()
            .find(|entry| entry.model_field == self)
            .map(|entry| &entry.descriptor)
            .expect("every ModelField has a FieldDescriptor")
    }

    /// Look up a model field by the descriptor's stable product ID.
    pub fn from_id(id: &str) -> Option<Self> {
        FIELD_DEFS
            .iter()
            .find(|entry| entry.descriptor.id.0 == id)
            .map(|entry| entry.model_field)
    }

    /// The GRIB key for this field in `model`, or `None` when that model does not publish it.
    ///
    /// `None` is a real answer, not a gap in the table: the NBM carries no updraft helicity
    /// because it is post-processed guidance rather than a convection-allowing run, and the RAP
    /// carries no hourly-max UH because it is not convection-allowing at all. A caller that gets
    /// `None` should grey the field out rather than fire a request it knows will 404.
    pub fn grib(self, model: Model) -> Option<GribKey> {
        let key = |var, level, min_valid| {
            Some(GribKey {
                var,
                level,
                min_valid,
            })
        };
        use Model::*;
        // Every entry below was read off a real `.idx` rather than assumed — the contract test
        // at the bottom of this file is what keeps it that way. Several of these exceptions are
        // not guessable: the NAM spells the composite-reflectivity level differently from the
        // HRRR, and the app's matcher compares the level string exactly.
        match (self, model) {
            // "entire atmosphere" vs "entire atmosphere (considered as a single layer)" — same
            // field, two spellings, and the NAM family uses the long one.
            (Self::CompositeReflectivity, NamNest | Nam) => key(
                "REFC",
                "entire atmosphere (considered as a single layer)",
                -30.0,
            ),
            // The NBM publishes MAXREF at 1000 m, which is a different quantity, not a
            // differently-named REFC; claiming it here would silently compare unlike fields.
            (Self::CompositeReflectivity, Nbm) => None,
            (Self::CompositeReflectivity, _) => key("REFC", "entire atmosphere", -30.0),

            // The NBM's CAPE is blended, calibrated guidance rather than a raw model's own
            // parcel calculation — genuinely published, so it is in the table, but `Ensemble::
            // PostProcessed` on its definition is what tells a caller not to difference it
            // against a raw run as though the two were the same kind of number.
            (Self::SurfaceCape, _) => key("CAPE", "surface", 0.0),

            (Self::MixedLayerCape, Nbm) => None,
            (Self::MixedLayerCape, _) => key("CAPE", "90-0 mb above ground", 0.0),

            // Helicity is signed: a negative value is anticyclonic rotation, not missing data.
            (Self::Srh1km, Nbm) => None,
            (Self::Srh1km, _) => key("HLCY", "1000-0 m above ground", f64::NEG_INFINITY),
            (Self::Srh3km, Nbm) => None,
            (Self::Srh3km, _) => key("HLCY", "3000-0 m above ground", f64::NEG_INFINITY),

            // Hourly-max UH exists only in the convection-allowing runs. The NAM's own 12 km
            // parent grid does not carry it even though its 3 km nest does.
            (Self::UpdraftHelicity, Hrrr | HrrrPressure | NamNest) => {
                key("MXUPHL", "5000-2000 m above ground", 0.0)
            }
            (Self::UpdraftHelicity, _) => None,

            // Caveat worth stating: at leads past the first hour these files carry *two* ASNOW
            // messages — the run-total accumulation and the trailing one-hour window — and the
            // matcher takes whichever the `.idx` lists first. The window is not pinned here, so
            // do not read this key as a promise of which one you get.
            (Self::Snowfall, Hrrr | HrrrPressure | Rap | Nbm) => key("ASNOW", "surface", 0.0),
            (Self::Snowfall, _) => None,

            // The NBM's calibrated thunder probability is the thing nobody else publishes.
            (Self::ThunderProbability, Nbm) => key("TSTM", "surface", 0.0),
            (Self::ThunderProbability, _) => None,

            // Near-surface aerosol mass density. The RAP carries it as well as the HRRR — found
            // by the contract test's negative check, not by reading documentation.
            (Self::Smoke, Hrrr | HrrrPressure | Rap) => key("MASSDEN", "8 m above ground", 0.0),
            (Self::Smoke, _) => None,

            // MSLMA (MAPS/analysis reduction) in the HRRR/RAP family, MSLET (Eta reduction) in
            // the NAM family. The NBM CONUS core file carries no mean-sea-level pressure at all.
            (Self::MeanSeaLevelPressure, Hrrr | HrrrPressure | Rap) => {
                key("MSLMA", "mean sea level", 0.0)
            }
            (Self::MeanSeaLevelPressure, NamNest | Nam) => key("MSLET", "mean sea level", 0.0),
            (Self::MeanSeaLevelPressure, Nbm) => None,

            (Self::Temperature2m, _) => key("TMP", "2 m above ground", f64::NEG_INFINITY),
            (Self::Dewpoint2m, _) => key("DPT", "2 m above ground", f64::NEG_INFINITY),
        }
    }

    /// Which of the wired-up models publish this field, in table order.
    pub fn models(self) -> Vec<Model> {
        ALL_MODELS
            .iter()
            .copied()
            .filter(|&m| self.grib(m).is_some())
            .collect()
    }
}

/// Every model with a definition, in table order.
pub const ALL_MODELS: &[Model] = &[
    Model::Hrrr,
    Model::HrrrPressure,
    Model::Rap,
    Model::NamNest,
    Model::Nam,
    Model::Nbm,
];

/// Every field in the catalogue, in declaration order.
pub const ALL_FIELDS: &[ModelField] = &[
    ModelField::CompositeReflectivity,
    ModelField::SurfaceCape,
    ModelField::MixedLayerCape,
    ModelField::Srh1km,
    ModelField::Srh3km,
    ModelField::UpdraftHelicity,
    ModelField::Snowfall,
    ModelField::ThunderProbability,
    ModelField::Smoke,
    ModelField::MeanSeaLevelPressure,
    ModelField::Temperature2m,
    ModelField::Dewpoint2m,
];

struct FieldEntry {
    model_field: ModelField,
    descriptor: FieldDescriptor,
}

macro_rules! model_field {
    ($kind:ident, $id:literal, $name:literal, $description:literal, $units:ident,
     $value_kind:ident, $aliases:literal, $palette:ident, $interval:expr) => {
        FieldEntry {
            model_field: ModelField::$kind,
            descriptor: FieldDescriptor {
                id: FieldId($id),
                source: DataSource::NoaaNcepModels,
                family: FieldFamily::Model,
                name: $name,
                description: $description,
                units: Unit::$units,
                value_kind: ValueKind::$value_kind,
                aliases: $aliases,
                default_palette: PaletteId::$palette,
                default_contour_interval: $interval,
                valid_domain: Some(GeographicBounds::CONUS),
                // The decoder has already converted GRIB bitmap and threshold exclusions to NaN.
                missing_values: &[],
            },
        }
    };
}

/// The fields HookEcho already requests from HRRR/RAP/NAM/NBM. This table owns presentation and
/// contour defaults; the GRIB mapping above owns each provider's wire spelling.
static FIELD_DEFS: &[FieldEntry] = &[
    model_field!(
        CompositeReflectivity,
        "model-composite-reflectivity",
        "Composite reflectivity",
        "Forecast radar reflectivity through the full atmospheric column",
        Dbz,
        Scalar,
        "future radar REFC HRRR RAP NAM",
        Reflectivity,
        None
    ),
    model_field!(
        SurfaceCape,
        "surface-cape",
        "Surface CAPE",
        "Convective available potential energy for a surface parcel",
        JoulesPerKilogram,
        Scalar,
        "storm fuel instability SBCAPE HRRR RAP NAM NBM",
        Cape,
        Some(500.0)
    ),
    model_field!(
        MixedLayerCape,
        "mixed-layer-cape",
        "Mixed-layer CAPE",
        "Convective available potential energy for the lowest 90 hPa mixed layer",
        JoulesPerKilogram,
        Scalar,
        "storm fuel instability MLCAPE HRRR RAP NAM",
        Cape,
        Some(500.0)
    ),
    model_field!(
        Srh1km,
        "srh-1km",
        "0–1 km SRH",
        "Storm-relative helicity in the lowest kilometre",
        SquareMetersPerSquareSecond,
        Scalar,
        "storm spin helicity tornado HRRR RAP NAM",
        Helicity,
        Some(50.0)
    ),
    model_field!(
        Srh3km,
        "srh-3km",
        "0–3 km SRH",
        "Storm-relative helicity in the lowest three kilometres",
        SquareMetersPerSquareSecond,
        Scalar,
        "storm spin helicity supercell HRRR RAP NAM",
        Helicity,
        Some(100.0)
    ),
    model_field!(
        UpdraftHelicity,
        "updraft-helicity",
        "Updraft helicity (2–5 km max)",
        "Hourly maximum rotating-updraft proxy in the 2–5 km layer",
        SquareMetersPerSquareSecond,
        Scalar,
        "future rotation tracks UH MXUPHL HRRR NAM nest",
        UpdraftHelicity,
        Some(25.0)
    ),
    model_field!(
        Snowfall,
        "model-snowfall",
        "Accumulated snowfall",
        "Forecast snowfall accumulated since the model run began",
        Meters,
        Accumulation,
        "snow accumulation ASNOW HRRR RAP NBM",
        Snowfall,
        None
    ),
    model_field!(
        ThunderProbability,
        "thunder-probability",
        "Thunder probability",
        "Calibrated probability of thunder during the forecast hour",
        Percent,
        Probability,
        "chance thunderstorms lightning NBM TSTM",
        ThunderProbability,
        Some(10.0)
    ),
    model_field!(
        Smoke,
        "near-surface-smoke",
        "Near-surface smoke",
        "Smoke mass density eight metres above ground",
        KilogramsPerCubicMeter,
        Scalar,
        "wildfire air quality MASSDEN HRRR RAP",
        Smoke,
        None
    ),
    model_field!(
        MeanSeaLevelPressure,
        "regional-mslp",
        "MSLP",
        "Atmospheric pressure reduced to mean sea level",
        Pascals,
        Scalar,
        "surface pressure isobars MSLMA MSLET HRRR RAP NAM",
        MeanSeaLevelPressure,
        Some(200.0)
    ),
    model_field!(
        Temperature2m,
        "regional-temperature-2m",
        "2 m temperature",
        "Air temperature two metres above ground",
        Kelvin,
        Scalar,
        "surface temperature TMP HRRR RAP NAM NBM",
        Temperature,
        Some(2.0)
    ),
    model_field!(
        Dewpoint2m,
        "regional-dewpoint-2m",
        "2 m dewpoint",
        "Dewpoint temperature two metres above ground",
        Kelvin,
        Scalar,
        "surface moisture DPT HRRR RAP NAM NBM",
        Dewpoint,
        Some(2.0)
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// `Model::def` unwraps, so the table must cover the enum. A model added to the enum without
    /// a row would panic the first time anything asked about it — at runtime, in front of a user.
    #[test]
    fn every_model_has_a_definition() {
        for &m in ALL_MODELS {
            let d = m.def();
            assert_eq!(d.label, m.label(), "{m:?} label disagrees with hrrr::Model");
            assert!(!d.id.is_empty());
        }
        // And `ALL_MODELS` must itself be complete, or the check above is vacuous for whatever it
        // omits. `DEFS` is the table `def()` actually reads, so compare against that.
        assert_eq!(ALL_MODELS.len(), DEFS.len(), "ALL_MODELS misses a row");
    }

    /// The lead schedule is the thing the old `.min(18)` got wrong. An extended cycle must
    /// actually report its longer lead, and an ordinary one must not.
    #[test]
    fn extended_cycles_publish_longer_leads() {
        assert_eq!(Model::Hrrr.max_lead_for_cycle(12), 48);
        assert_eq!(Model::Hrrr.max_lead_for_cycle(13), 18);
        // A model with no extended cycles reports the same lead for every hour of the day.
        for h in 0..24 {
            assert_eq!(Model::NamNest.max_lead_for_cycle(h), 60);
        }
    }

    /// A definition whose numbers contradict themselves would send the fetch path looking for
    /// files that cannot exist.
    #[test]
    fn definitions_are_internally_consistent() {
        for &m in ALL_MODELS {
            let d = m.def();
            assert!(d.extended_lead_h >= d.max_lead_h, "{}", d.label);
            assert!(d.cycle_hours >= 1 && 24 % d.cycle_hours == 0, "{}", d.label);
            assert!(d.grid_km > 0.0, "{}", d.label);
            assert!(
                d.extended_cycles
                    .iter()
                    .all(|h| *h < 24 && h % d.cycle_hours == 0),
                "{} lists an extended cycle it never runs",
                d.label
            );
            assert!(d.domain.contains(-97.0, 35.0), "{} excludes CONUS", d.label);
        }
    }

    /// Every field must be published by at least one model, and a field claimed for a model must
    /// carry a non-empty GRIB key. An empty `var` would match the first line of any `.idx`.
    #[test]
    fn every_field_maps_to_at_least_one_model() {
        for &f in ALL_FIELDS {
            let descriptor = f.descriptor();
            assert!(!descriptor.id.0.is_empty(), "{f:?}");
            assert_eq!(descriptor.source, DataSource::NoaaNcepModels);
            assert_eq!(descriptor.family, FieldFamily::Model);
            assert!(descriptor.supports_location(-97.0, 35.0));
            assert!(!descriptor.supports_location(-150.0, 60.0));
            assert_eq!(ModelField::from_id(descriptor.id.0), Some(f));
            assert!(descriptor.search_text().contains("NOAA/NCEP models"));
            let models = f.models();
            assert!(!models.is_empty(), "{:?} maps to no model", f);
            for m in models {
                let k = f.grib(m).unwrap();
                assert!(!k.var.is_empty() && !k.level.is_empty(), "{f:?}/{m:?}");
            }
        }
        assert_eq!(FIELD_DEFS.len(), ALL_FIELDS.len());
        let unique_ids: std::collections::HashSet<_> =
            FIELD_DEFS.iter().map(|entry| entry.descriptor.id).collect();
        assert_eq!(unique_ids.len(), FIELD_DEFS.len());
    }

    #[test]
    fn contour_defaults_use_native_units() {
        assert_eq!(
            ModelField::MeanSeaLevelPressure
                .descriptor()
                .default_contour_interval,
            Some(200.0)
        );
        assert_eq!(
            ModelField::Temperature2m
                .descriptor()
                .default_contour_interval,
            Some(2.0)
        );
        assert_eq!(
            ModelField::CompositeReflectivity
                .descriptor()
                .default_contour_interval,
            None
        );
    }

    /// Signed fields must keep their negative half. Dropping values below zero from helicity
    /// would erase anticyclonic rotation — a real signal — as though it were missing data.
    #[test]
    fn signed_fields_are_not_floored_at_zero() {
        for f in [
            ModelField::Srh1km,
            ModelField::Srh3km,
            ModelField::Temperature2m,
            ModelField::Dewpoint2m,
        ] {
            for m in f.models() {
                assert_eq!(
                    f.grib(m).unwrap().min_valid,
                    f64::NEG_INFINITY,
                    "{f:?} on {m:?} would drop its negative values"
                );
            }
        }
    }

    /// The table claims things about real files. This asks the feeds themselves: for each model,
    /// pull one recent cycle's `.idx` and assert that every field the table says that model
    /// publishes is actually in it — and, just as important, that every field the table says it
    /// does *not* publish is genuinely absent, so a `None` is a fact rather than an oversight.
    ///
    /// Network-gated, like the rest of the live feed tests:
    /// `cargo test -p wxdata -- --ignored the_catalogue_matches`.
    #[tokio::test]
    #[ignore = "network"]
    async fn the_catalogue_matches_what_the_feeds_publish() {
        let http = reqwest::Client::new();
        let now = chrono::Utc::now();
        let mut checked = 0usize;
        for &m in ALL_MODELS {
            // Walk back until a cycle is actually posted; a just-named run is not on the wire.
            let mut idx = None;
            'cycles: for run in crate::hrrr::recent_cycles(m, now) {
                // The NBM's CONUS core has no f001, so try a couple of short leads before
                // giving up on a cycle that is in fact posted.
                for fh in [1u8, 2, 3] {
                    if let Ok(text) = crate::hrrr::fetch_idx(&http, m, run, fh).await {
                        idx = Some((run, fh, text));
                        break 'cycles;
                    }
                }
            }
            let Some((run, fh, idx)) = idx else {
                println!("SKIP {}: no cycle available", m.label());
                continue;
            };
            for &f in ALL_FIELDS {
                // `field_byte_range` is the same matcher the fetch path uses, so this tests each
                // key exactly as it will be used rather than an approximation of it.
                let found = |k: &GribKey| crate::hrrr::field_byte_range(&idx, k.var, k.level);
                match f.grib(m) {
                    Some(k) => {
                        assert!(
                            found(&k).is_some(),
                            "{} run {run} f{fh} claims {}:{} but its idx has no such message",
                            m.label(),
                            k.var,
                            k.level
                        );
                        checked += 1;
                    }
                    // A `None` is also a claim, and a wrong one costs the user a field the model
                    // really does publish. Check it against every spelling the field uses on any
                    // other model: if one of those is sitting in this idx, the table is wrong.
                    None => {
                        for other in ALL_MODELS.iter().filter_map(|&o| f.grib(o)) {
                            assert!(
                                found(&other).is_none(),
                                "{} run {run} f{fh} is marked as not publishing {:?}, but its \
                                 idx does carry {}:{}",
                                m.label(),
                                f,
                                other.var,
                                other.level
                            );
                        }
                        checked += 1;
                    }
                }
            }
            println!("{}: idx from {run} f{fh} agreed with the table", m.label());
        }
        assert!(checked > 0, "no model feed was reachable");
    }

    /// Forecast reflectivity is offered for every regional model that publishes it, at a lead
    /// beyond the analysis. This downloads and decodes one real forecast hour from each, so a
    /// model whose composite-reflectivity level is spelled differently (the NAMs) fails here
    /// rather than as an empty layer.
    ///
    /// Network-gated: `cargo test -p wxdata -- --ignored reflectivity_decodes`.
    #[tokio::test]
    #[ignore = "network"]
    async fn reflectivity_decodes_for_every_model_that_publishes_it() {
        let http = reqwest::Client::new();
        for (model, fh) in [
            (Model::Hrrr, 3u8),
            (Model::Rap, 3),
            (Model::NamNest, 6),
            (Model::Nam, 6),
        ] {
            let key = ModelField::CompositeReflectivity
                .grib(model)
                .expect("the catalogue says it publishes reflectivity");
            let fc = crate::hrrr::fetch_field(&http, model, key.var, key.level, fh, key.min_valid)
                .await
                .unwrap_or_else(|e| panic!("{} f{fh:02}: {e}", model.label()));
            let finite = fc.field.values.iter().filter(|v| v.is_finite()).count();
            // A CONUS field at these resolutions is thousands of cells, most of them no echo.
            assert!(
                finite > 1_000,
                "{}: only {finite} finite cells",
                model.label()
            );
            assert_eq!(
                fc.valid(),
                fc.field.time,
                "{}: valid time drifted",
                model.label()
            );
            println!(
                "{} f{fh:02}: run {} valid {} · {}x{} · {finite} cells",
                model.label(),
                fc.run,
                fc.valid(),
                fc.field.nx,
                fc.field.ny
            );
        }
    }
}
