//! Per-pane model controls. Link groups share source/run/lead while retaining pane products.
use crate::model_browser::{Engine, Selection};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelControls {
    pub model_sel: Selection,
    pub model_run: Option<DateTime<Utc>>,
    pub refl_model: wxdata::hrrr::Model,
    pub env_model: wxdata::hrrr::Model,
    pub global_model: wxdata::global::GlobalModel,
    pub global_fcst_hour: u16,
    pub hrrr_fcst_hour: u8,
    pub hrrr_fcst_min: u16,
    pub hrrr_subhourly: bool,
    pub env_cape_ml: bool,
    pub env_srh_km: u8,
    #[serde(skip)]
    pub hrrr_by_timeline: bool,
}

impl Default for ModelControls {
    fn default() -> Self {
        Self {
            model_sel: Selection::default(),
            model_run: None,
            refl_model: wxdata::hrrr::Model::Hrrr,
            env_model: wxdata::hrrr::Model::Hrrr,
            global_model: wxdata::global::GlobalModel::default(),
            global_fcst_hour: 0,
            hrrr_fcst_hour: 1,
            hrrr_fcst_min: 15,
            hrrr_subhourly: false,
            env_cape_ml: false,
            env_srh_km: 3,
            hrrr_by_timeline: false,
        }
    }
}
impl ModelControls {
    pub fn lead_min(&self) -> u16 {
        match self.model_sel.model.engine() {
            Engine::Sub15 => self.hrrr_fcst_min,
            Engine::Regional(_) => u16::from(self.hrrr_fcst_hour) * 60,
            Engine::Global(_) => self.global_fcst_hour * 60,
            Engine::Analysis => 0,
        }
    }
    pub fn set_lead(&mut self, minutes: u16, now: DateTime<Utc>) {
        let m = self
            .model_sel
            .model
            .leads_for(self.model_run, now)
            .clamp(minutes);
        match self.model_sel.model.engine() {
            Engine::Sub15 => {
                self.hrrr_fcst_min = m;
                self.hrrr_fcst_hour = (m / 60) as u8;
            }
            Engine::Regional(_) => {
                self.hrrr_fcst_hour = (m / 60) as u8;
                self.hrrr_fcst_min = m.max(15);
            }
            Engine::Global(_) => self.global_fcst_hour = m / 60,
            Engine::Analysis => {}
        }
    }
    pub fn apply_engine(&mut self, selection: Selection) {
        use crate::model_browser::Product;
        match (selection.model.engine(), selection.product) {
            (Engine::Sub15, _) => {
                self.refl_model = wxdata::hrrr::Model::Hrrr;
                self.hrrr_subhourly = true;
            }
            (Engine::Regional(model), Product::Reflectivity) => {
                self.refl_model = model;
                self.hrrr_subhourly = false;
            }
            (Engine::Regional(model), Product::Cape | Product::Srh) => self.env_model = model,
            (Engine::Global(model), _) => self.global_model = model,
            _ => {}
        }
    }
    pub fn linked_from(&self, source: &Self, now: DateTime<Utc>) -> Self {
        let selection = self.model_sel.with_model(source.model_sel.model);
        let mut next = source.clone();
        next.model_sel = selection;
        next.env_cape_ml = self.env_cape_ml;
        next.env_srh_km = self.env_srh_km;
        next.hrrr_by_timeline = false;
        next.apply_engine(selection);
        next.set_lead(source.lead_min(), now);
        next
    }
    pub fn valid(&self) -> bool {
        use chrono::Timelike;
        let mut canonical = self.clone();
        canonical.apply_engine(self.model_sel);
        let engine_consistent = match self.model_sel.model.engine() {
            Engine::Sub15 => self.hrrr_subhourly && self.refl_model == canonical.refl_model,
            Engine::Regional(_) => match self.model_sel.product {
                crate::model_browser::Product::Reflectivity => {
                    !self.hrrr_subhourly && self.refl_model == canonical.refl_model
                }
                crate::model_browser::Product::Cape | crate::model_browser::Product::Srh => {
                    self.env_model == canonical.env_model
                }
                _ => true,
            },
            Engine::Global(_) => self.global_model == canonical.global_model,
            Engine::Analysis => true,
        };
        let cycle = match self.model_sel.model.engine() {
            Engine::Regional(model) => model.def().cycle_hours,
            Engine::Global(_) => 6,
            _ => 1,
        };
        engine_consistent
            && self.model_run.is_none_or(|run| run.hour() % cycle == 0)
            && self
                .model_sel
                .model
                .products()
                .contains(&self.model_sel.product)
            && matches!(self.env_srh_km, 1 | 3)
            && self.global_fcst_hour <= 384
            && self.hrrr_fcst_hour <= 84
            && self.hrrr_fcst_min <= 5040
            && self
                .model_run
                .map_or_else(
                    || self.model_sel.model.leads(),
                    |run| self.model_sel.model.leads_for(Some(run), run),
                )
                .clamp(self.lead_min())
                == self.lead_min()
            && self
                .model_run
                .is_none_or(|run| run.minute() == 0 && run.second() == 0 && run.nanosecond() == 0)
    }
}

/// Saved session source context, kept in PaneSnap's forward-compatible extension map.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SavedModelContext {
    pub schema: u8,
    pub group: Option<u8>,
    pub controls: ModelControls,
}
impl SavedModelContext {
    pub fn valid(&self) -> bool {
        self.schema == 1
            && self
                .group
                .is_none_or(|group| (1..=crate::view::MAX_PANES as u8).contains(&group))
            && self.controls.valid()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_browser::{BModel, Product};
    use chrono::TimeZone;

    #[test]
    fn model_pane_published_choices_round_trip_with_their_engine_and_native_leads() {
        let now = Utc.with_ymd_and_hms(2026, 10, 4, 20, 0, 0).unwrap();
        for model in BModel::ALL {
            for product in model.products() {
                let selection = Selection {
                    model,
                    product: *product,
                };
                let mut controls = ModelControls {
                    model_sel: selection,
                    model_run: Some(Utc.with_ymd_and_hms(2026, 10, 4, 12, 0, 0).unwrap()),
                    ..Default::default()
                };
                controls.apply_engine(selection);
                controls.set_lead(246 * 60, now);
                assert!(controls.valid(), "{selection:?}");
                let saved = SavedModelContext {
                    schema: 1,
                    group: Some(2),
                    controls: controls.clone(),
                };
                let restored: SavedModelContext =
                    serde_json::from_value(serde_json::to_value(saved).unwrap()).unwrap();
                assert!(restored.valid());
                assert_eq!(restored.controls, controls);
            }
        }
    }

    #[test]
    fn model_pane_rejects_wrong_engine_cycle_invalid_variants_and_missing_identity() {
        let mut controls = ModelControls {
            model_sel: Selection {
                model: BModel::Nam,
                product: Product::Reflectivity,
            },
            ..Default::default()
        };
        assert!(
            !controls.valid(),
            "NAM picker cannot silently restore an HRRR grid"
        );
        controls.apply_engine(controls.model_sel);
        assert!(controls.valid());
        controls.model_run = Some(Utc.with_ymd_and_hms(2026, 10, 4, 13, 0, 0).unwrap());
        assert!(!controls.valid());
        controls.model_run = None;
        controls.env_srh_km = 2;
        assert!(!controls.valid());
        assert!(
            serde_json::from_value::<ModelControls>(serde_json::json!({"hrrr_fcst_hour": 6}))
                .is_err()
        );
        let saved = SavedModelContext {
            schema: 2,
            group: Some(1),
            controls: Default::default(),
        };
        assert!(!saved.valid());
    }
}

#[cfg(test)]
mod native_lead_tests {
    use super::*;
    use crate::model_browser::{BModel, Product};
    #[test]
    fn model_pane_saved_leads_reject_unsupported_steps_and_overflow() {
        let selection = Selection {
            model: BModel::Gfs,
            product: Product::Temp2m,
        };
        let mut controls = ModelControls {
            model_sel: selection,
            global_fcst_hour: 4,
            ..Default::default()
        };
        controls.apply_engine(selection);
        assert!(!controls.valid());
        controls.global_fcst_hour = u16::MAX;
        assert!(!controls.valid());
        controls.global_fcst_hour = 6;
        assert!(controls.valid());
        controls.model_sel = Selection {
            model: BModel::Hrrr15,
            product: Product::Reflectivity,
        };
        controls.apply_engine(controls.model_sel);
        controls.hrrr_fcst_min = 20;
        assert!(!controls.valid());
        controls.hrrr_fcst_min = 75;
        assert!(controls.valid());
    }
}
