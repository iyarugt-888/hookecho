//! Model contours over the radar: which fields can be contoured ([`ContourKind`]) and each
//! active one's fetched state ([`ContourEntry`]). Moved out of `app.rs` (ROADMAP_2 §7).

use super::*;

/// HRRR model field drawn as contour lines over the radar (surface `f00`). SB-CAPE / 0-3 km SRH
/// are fixed here — `// ponytail: not wired to the env suite's env_cape_ml / env_srh_km toggles.`
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
pub(crate) enum ContourKind {
    #[default]
    Off,
    Mslp,
    T2m,
    Td2m,
    Cape,
    Srh,
    /// Significant Tornado Parameter (composite of several HRRR fields — see `wxdata::severe`).
    Stp,
    /// Supercell Composite Parameter.
    Scp,
    /// Energy-Helicity Index, 0-1 km.
    Ehi,
    /// 700–500 hPa lapse rate (°C/km).
    Lapse700500,
    /// 850–500 hPa lapse rate (°C/km).
    Lapse850500,
    /// Effective bulk wind difference (kt).
    EffShear,
    /// Effective storm-relative helicity (m²/s²).
    EffSrh,
    /// STP in its effective-layer form — the one SPC mesoanalysis draws.
    StpEff,
}

impl ContourKind {
    pub(crate) const ALL: [ContourKind; 14] = [
        ContourKind::Off,
        ContourKind::Mslp,
        ContourKind::T2m,
        ContourKind::Td2m,
        ContourKind::Cape,
        ContourKind::Srh,
        ContourKind::Stp,
        ContourKind::Scp,
        ContourKind::Ehi,
        ContourKind::Lapse700500,
        ContourKind::Lapse850500,
        ContourKind::EffShear,
        ContourKind::EffSrh,
        ContourKind::StpEff,
    ];

    /// The composite parameters, which combine several GRIB fields instead of drawing one.
    pub(crate) fn severe(self) -> Option<wxdata::severe::SevereKind> {
        use wxdata::severe::SevereKind as S;
        Some(match self {
            ContourKind::Stp => S::Stp,
            ContourKind::Scp => S::Scp,
            ContourKind::Ehi => S::Ehi,
            ContourKind::Lapse700500 => S::Lapse700500,
            ContourKind::Lapse850500 => S::Lapse850500,
            ContourKind::EffShear => S::EffShear,
            ContourKind::EffSrh => S::EffSrh,
            ContourKind::StpEff => S::StpEff,
            _ => return None,
        })
    }

    /// Contour interval in display units (composites only; single fields carry theirs in `params`).
    pub(crate) fn severe_interval(self) -> f32 {
        match self {
            ContourKind::Stp | ContourKind::StpEff => 0.5,
            ContourKind::Scp => 2.0,
            // °C/km: 0.5 resolves the 7-8 °C/km band steep-lapse-rate plumes live in.
            ContourKind::Lapse700500 | ContourKind::Lapse850500 => 0.5,
            ContourKind::EffShear => 10.0, // kt
            ContourKind::EffSrh => 100.0,  // m²/s²
            _ => 1.0,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            ContourKind::Off => "Off",
            ContourKind::Mslp => "MSLP",
            ContourKind::T2m => "2 m temp",
            ContourKind::Td2m => "2 m dewpoint",
            ContourKind::Cape => "SB-CAPE",
            ContourKind::Srh => "0-3 km SRH",
            ContourKind::Stp => "STP (fixed)",
            ContourKind::Scp => "SCP",
            ContourKind::Ehi => "EHI 0-1 km",
            ContourKind::Lapse700500 => "700-500 lapse",
            ContourKind::Lapse850500 => "850-500 lapse",
            ContourKind::EffShear => "Eff. bulk shear",
            ContourKind::EffSrh => "Eff. SRH",
            ContourKind::StpEff => "STP (effective)",
        }
    }

    /// The token [`Self::from_token`] reads, also the name a kind is saved under; `None` for Off.
    pub(crate) fn token(self) -> Option<&'static str> {
        Some(match self {
            ContourKind::Off => return None,
            ContourKind::Mslp => "mslp",
            ContourKind::T2m => "t2m",
            ContourKind::Td2m => "td2m",
            ContourKind::Cape => "cape",
            ContourKind::Srh => "srh",
            ContourKind::Stp => "stp",
            ContourKind::Scp => "scp",
            ContourKind::Ehi => "ehi",
            ContourKind::Lapse700500 => "lapse700",
            ContourKind::Lapse850500 => "lapse850",
            ContourKind::EffShear => "ebwd",
            ContourKind::EffSrh => "esrh",
            ContourKind::StpEff => "stpeff",
        })
    }

    /// Parse a headless CLI token (`mslp|t2m|td2m|cape|srh`) into a kind.
    pub(crate) fn from_token(s: &str) -> Option<ContourKind> {
        Some(match s {
            "mslp" => ContourKind::Mslp,
            "t2m" => ContourKind::T2m,
            "td2m" => ContourKind::Td2m,
            "cape" => ContourKind::Cape,
            "srh" => ContourKind::Srh,
            "stp" => ContourKind::Stp,
            "scp" => ContourKind::Scp,
            "ehi" => ContourKind::Ehi,
            "lapse700" => ContourKind::Lapse700500,
            "lapse850" => ContourKind::Lapse850500,
            "ebwd" => ContourKind::EffShear,
            "esrh" => ContourKind::EffSrh,
            "stpeff" => ContourKind::StpEff,
            _ => return None,
        })
    }

    pub(crate) fn model_field(self) -> Option<wxdata::model::ModelField> {
        use wxdata::model::ModelField as MF;
        Some(match self {
            ContourKind::Mslp => MF::MeanSeaLevelPressure,
            ContourKind::T2m => MF::Temperature2m,
            ContourKind::Td2m => MF::Dewpoint2m,
            ContourKind::Cape => MF::SurfaceCape,
            ContourKind::Srh => MF::Srh3km,
            _ => return None,
        })
    }

    /// GRIB `(var, level, native contour interval)`, or `None` for `Off` and derived composites.
    /// HRRR and RAP intentionally share these spellings; the model-catalog contract test protects
    /// that invariant because this contour UI can switch between them without changing fields.
    pub(crate) fn params(self) -> Option<(&'static str, &'static str, f32)> {
        let field = self.model_field()?;
        let key = field.grib(wxdata::hrrr::Model::Hrrr)?;
        Some((
            key.var,
            key.level,
            field.descriptor().default_contour_interval?,
        ))
    }

    pub(crate) fn interval(self, temp_unit: crate::settings::TempUnit) -> f32 {
        match (self, temp_unit) {
            // Two kelvin is a useful metric interval; five Fahrenheit is the conventional rounded
            // chart interval rather than the awkward exact conversion (3.6 °F).
            (ContourKind::T2m | ContourKind::Td2m, crate::settings::TempUnit::Fahrenheit) => 5.0,
            (ContourKind::Mslp, _) => self.params().map_or(2.0, |(_, _, pa)| pa / 100.0),
            _ => self
                .params()
                .map_or_else(|| self.severe_interval(), |(_, _, interval)| interval),
        }
    }

    /// Convert a raw GRIB value to the display unit the interval is expressed in.
    pub(crate) fn to_display(self, raw: f32, temp_unit: crate::settings::TempUnit) -> f32 {
        match self {
            ContourKind::Mslp => raw / 100.0, // Pa → hPa
            ContourKind::T2m | ContourKind::Td2m => temp_unit.from_c(raw - 273.15), // K → selected unit
            _ => raw, // CAPE / SRH as-is
        }
    }

    pub(crate) fn unit(self, temp_unit: crate::settings::TempUnit) -> Option<&'static str> {
        matches!(self, ContourKind::T2m | ContourKind::Td2m).then(|| temp_unit.label())
    }

    pub(crate) fn display_label(self, temp_unit: crate::settings::TempUnit) -> String {
        match self.unit(temp_unit) {
            Some(unit) => format!("{} {unit}", self.label()),
            None => self.label().into(),
        }
    }

    pub(crate) fn color(self) -> egui::Color32 {
        match self {
            ContourKind::Mslp => egui::Color32::from_rgb(235, 235, 235),
            ContourKind::T2m => egui::Color32::from_rgb(240, 120, 60),
            ContourKind::Td2m => egui::Color32::from_rgb(90, 200, 120),
            ContourKind::Cape => egui::Color32::from_rgb(240, 160, 40),
            ContourKind::Srh => egui::Color32::from_rgb(190, 110, 230),
            ContourKind::Stp => egui::Color32::from_rgb(230, 60, 90),
            ContourKind::Scp => egui::Color32::from_rgb(250, 120, 50),
            ContourKind::Ehi => egui::Color32::from_rgb(150, 110, 235),
            ContourKind::Lapse700500 => egui::Color32::from_rgb(240, 200, 90),
            ContourKind::Lapse850500 => egui::Color32::from_rgb(210, 170, 70),
            ContourKind::EffShear => egui::Color32::from_rgb(120, 190, 250),
            ContourKind::EffSrh => egui::Color32::from_rgb(200, 130, 240),
            ContourKind::StpEff => egui::Color32::from_rgb(250, 70, 110),
            ContourKind::Off => egui::Color32::WHITE,
        }
    }
}

/// Text for a multi-select contour picker's collapsed summary: "Off" for none active, the one
/// label when exactly one is, otherwise a joined list — so the picker always says what's actually
/// drawn without needing to open it.
pub(crate) fn summarize_contours(active: &std::collections::BTreeSet<ContourKind>) -> String {
    if active.is_empty() {
        return ContourKind::Off.label().to_string();
    }
    active
        .iter()
        .map(|k| k.label())
        .collect::<Vec<_>>()
        .join(", ")
}

/// One active [`ContourKind`]'s own fetched state — kept per kind so several contour overlays can
/// be in flight, cached, and stale-refreshed independently of each other.
#[derive(Default)]
pub(crate) struct ContourEntry {
    pub lines: Vec<wxdata::contour::ContourLine>,
    pub valid: Option<DateTime<Utc>>,
    /// The model run the grid is from, and when this app received it (the probe's stamp).
    pub run: Option<DateTime<Utc>>,
    pub received: Option<DateTime<Utc>>,
    /// The grid the lines were drawn from (display units), which the layer probe reads.
    pub grid: Option<Arc<wxdata::mrms::MrmsField>>,
    pub last_fetch: Option<Instant>,
    pub fetched_key: Option<(wxdata::hrrr::Model, crate::settings::TempUnit)>,
}

/// One contour layer's lines as GeoJSON features (ROADMAP_PARITY M4.4): each line at its level,
/// with the field, its display unit (absent when the field has none, never guessed), the model,
/// its run and valid time. Coordinates are WGS84 `[lon, lat]`.
pub(crate) fn contour_features(
    field: &str,
    unit: Option<&str>,
    model: Option<&str>,
    entry: &ContourEntry,
) -> Vec<wxdata::gis::GisFeature> {
    use serde_json::{Map, Value};
    let time = |t: Option<DateTime<Utc>>| t.map_or(Value::Null, |t| Value::from(t.to_rfc3339()));
    entry
        .lines
        .iter()
        .filter(|l| l.pts.len() >= 2)
        .map(|l| {
            let mut p = Map::new();
            p.insert("hookecho".into(), "contour".into());
            p.insert("field".into(), field.into());
            p.insert(
                "level".into(),
                serde_json::Number::from_f64(f64::from(l.level)).map_or(Value::Null, Value::Number),
            );
            p.insert("unit".into(), unit.map_or(Value::Null, Value::from));
            p.insert("model".into(), model.map_or(Value::Null, Value::from));
            p.insert("run".into(), time(entry.run));
            p.insert("valid".into(), time(entry.valid));
            wxdata::gis::GisFeature {
                geometry: wxdata::gis::Geometry::LineString(
                    l.pts.iter().map(|&(lon, lat)| [lon, lat]).collect(),
                ),
                properties: p,
            }
        })
        .collect()
}

impl HookEchoApp {
    /// Every active contour layer's lines, for the map's GeoJSON export.
    pub(crate) fn contour_features(&self) -> Vec<wxdata::gis::GisFeature> {
        let temp = self.settings.temp_unit;
        let mut out = Vec::new();
        // In the layers' own order, so the same map writes the same file.
        for kind in &self.active_contours {
            let Some(entry) = self.contours.get(kind) else {
                continue;
            };
            let model = entry.fetched_key.map(|(m, _)| m.label());
            out.extend(contour_features(
                kind.label(),
                kind.unit(temp),
                model,
                entry,
            ));
        }
        out
    }

    /// A contour fetch landed. Kept only if its kind is still active (it may have been turned
    /// off while the fetch was in flight), with its run and the time it arrived for the probe.
    pub(crate) fn contours_arrived(
        &mut self,
        kind: ContourKind,
        lines: Vec<wxdata::contour::ContourLine>,
        run: DateTime<Utc>,
        valid: DateTime<Utc>,
        grid: Arc<wxdata::mrms::MrmsField>,
    ) {
        if self.active_contours.contains(&kind) {
            let entry = self.contours.entry(kind).or_default();
            entry.lines = lines;
            entry.run = Some(run);
            entry.valid = Some(valid);
            entry.received = Some(Utc::now());
            entry.grid = Some(grid);
        }
    }
}

/// A model contour's stamp for the probe (ROADMAP_2 §9.1): the model and field, its run, the
/// time it is valid for, and when it arrived. A forecast, even at f00 (an analysis is the model's
/// own estimate). `None` until the grid has arrived with its times.
pub(crate) fn contour_stamp(
    model: &str,
    kind: ContourKind,
    entry: &ContourEntry,
) -> Option<wxdata::field::DataStamp> {
    Some(wxdata::field::DataStamp {
        source_id: model.to_string(),
        product_id: kind.label().to_string(),
        issue_time: None,
        run_time: entry.run,
        valid_time: entry.valid?,
        received_time: entry.received?,
        source_latency: None,
        is_forecast: true,
        is_derived: kind.severe().is_some(),
        quality: wxdata::field::QualitySummary::Unknown,
        grid: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contour_lines_export_with_field_level_unit_and_times() {
        let t = |h: u32| chrono::DateTime::from_timestamp(1_700_000_000 + i64::from(h) * 3600, 0);
        let entry = ContourEntry {
            lines: vec![
                wxdata::contour::ContourLine {
                    level: 1008.0,
                    pts: vec![(-97.0, 35.0), (-96.5, 35.2)],
                    bbox: (-97.0, 35.0, -96.5, 35.2),
                },
                wxdata::contour::ContourLine {
                    level: 1012.0,
                    pts: vec![(-97.0, 36.0)],
                    bbox: (-97.0, 36.0, -97.0, 36.0),
                },
            ],
            valid: t(1),
            run: t(0),
            received: None,
            grid: None,
            last_fetch: None,
            fetched_key: None,
        };
        let f = contour_features("MSLP", Some("hPa"), Some("HRRR"), &entry);
        assert_eq!(f.len(), 1, "a one-point line is left out");
        let p = &f[0].properties;
        assert_eq!(p["field"], "MSLP");
        assert_eq!(p["level"], 1008.0);
        assert_eq!(p["unit"], "hPa");
        assert_eq!(p["model"], "HRRR");
        assert_eq!(p["valid"], t(1).unwrap().to_rfc3339());
        let none = contour_features("STP", None, None, &entry);
        assert!(
            none[0].properties["unit"].is_null(),
            "no unit is said, not guessed"
        );
    }

    #[test]
    fn a_contour_is_stamped_once_its_grid_has_arrived() {
        let t = |m: i64| chrono::DateTime::from_timestamp(m * 60, 0).unwrap();
        let mut e = ContourEntry::default();
        assert!(contour_stamp("HRRR", ContourKind::Mslp, &e).is_none());
        e.run = Some(t(0));
        e.valid = Some(t(60));
        e.received = Some(t(75));
        let s = contour_stamp("HRRR", ContourKind::Mslp, &e).unwrap();
        assert_eq!(
            (s.run_time, s.valid_time, s.received_time),
            (Some(t(0)), t(60), t(75))
        );
        assert!(s.is_forecast && !s.is_derived);
        // A composite (STP) is computed from several fields: derived.
        assert!(
            contour_stamp("HRRR", ContourKind::Stp, &e)
                .unwrap()
                .is_derived
        );
    }
}
