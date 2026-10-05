//! Explicit source/run/lead links. Products and their variants remain pane choices.
use super::*;

pub(super) fn propagate(views: &mut [MapView], source: usize, now: DateTime<Utc>) {
    let Some(group) = views[source].model_group else {
        views[source].model_link_snapshot = views[source].models.clone();
        return;
    };
    let controls = views[source].models.clone();
    for (idx, view) in views.iter_mut().enumerate() {
        if view.model_group != Some(group) {
            continue;
        }
        if idx != source {
            let previous = view.models.model_sel.layer();
            view.model_restore_raw = None;
            view.models = view.models.linked_from(&controls, now);
            if view.fields_on.remove(&previous) {
                view.fields_on.insert(view.models.model_sel.layer());
            }
        }
        view.model_link_snapshot = view.models.clone();
    }
}

pub(super) fn join(views: &mut [MapView], idx: usize, group: Option<u8>, now: DateTime<Utc>) {
    let group = group.filter(|g| (1..=crate::view::MAX_PANES as u8).contains(g));
    let source = group.and_then(|group| {
        views
            .iter()
            .enumerate()
            .find(|(i, view)| *i != idx && view.model_group == Some(group))
            .map(|(i, _)| i)
    });
    views[idx].model_playback.pause();
    views[idx].model_group = group;
    if let Some(source) = source {
        propagate(views, source, now);
    } else {
        views[idx].model_link_snapshot = views[idx].models.clone();
    }
}

impl HookEchoApp {
    /// All input surfaces can edit controls; reconcile before intake and before painting maps.
    pub(super) fn sync_model_groups(&mut self) {
        let mut changed: Vec<_> = self
            .views
            .iter()
            .enumerate()
            .filter(|(_, v)| v.models != v.model_link_snapshot)
            .map(|(idx, _)| idx)
            .collect();
        // The focused pane owns simultaneous UI changes in a group.
        changed.sort_by_key(|idx| *idx != self.active);
        for idx in changed {
            if self.views[idx].models == self.views[idx].model_link_snapshot {
                continue;
            }
            let group = self.views[idx].model_group;
            for (i, view) in self.views.iter_mut().enumerate() {
                if i == idx || group.is_some() && view.model_group == group {
                    view.model_playback.pause();
                }
            }
            self.views[idx].model_restore_raw = None;
            propagate(&mut self.views, idx, Utc::now());
        }
    }

    pub(crate) fn model_group_ui(&mut self, ui: &mut egui::Ui) {
        let idx = self.active;
        if self.views[idx].model_restore_raw.is_some() {
            ui.colored_label(
                egui::Color32::YELLOW,
                "Saved model controls unavailable. Choose a model to replace them.",
            );
        }
        let mut group = self.views[idx].model_group;
        picker(ui, idx, &mut group);
        if group != self.views[idx].model_group {
            join(&mut self.views, idx, group, Utc::now());
        }
    }
}

pub(super) fn picker(ui: &mut egui::Ui, idx: usize, group: &mut Option<u8>) {
    use crate::ui::a11y::Named as _;
    ui.horizontal_wrapped(|ui| {
            ui.weak(format!("Pane {} · Model/run", idx + 1));
            egui::ComboBox::from_id_salt(("model_group", idx))
                .selected_text((*group).map_or_else(|| "Independent".into(), |g| format!("Group {g}")))
                .show_ui(ui, |ui| {
                    ui.selectable_value(group, None, "Independent");
                    for g in 1..=crate::view::MAX_PANES as u8 {
                        ui.selectable_value(group, Some(g), format!("Group {g}"));
                    }
                }).response.named("Pane model and run link group")
                .on_hover_text("Group members share model, run and lead. Each pane keeps a compatible product, otherwise the model's published default. Independent keeps the current selection.");
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_browser::{BModel, Product, Selection};
    use crate::render::mercator::Camera;
    use chrono::TimeZone;

    fn pane(model: BModel, product: Product, group: Option<u8>) -> MapView {
        let mut view = MapView::new(None, Camera::at_lonlat(-97.0, 35.0, 8.0));
        view.models.model_sel = Selection { model, product };
        view.models.apply_engine(view.models.model_sel);
        view.models.model_run = Some(Utc.with_ymd_and_hms(2026, 10, 4, 12, 0, 0).unwrap());
        view.fields_on.insert(view.models.model_sel.layer());
        view.model_group = group;
        view.model_link_snapshot = view.models.clone();
        view
    }
    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 4, 20, 0, 0).unwrap()
    }

    #[test]
    fn model_groups_share_source_run_lead_and_keep_products_and_variants() {
        let mut views = vec![
            pane(BModel::Rap, Product::Cape, Some(1)),
            pane(BModel::Hrrr, Product::Srh, Some(1)),
            pane(BModel::Gfs, Product::Temp2m, None),
        ];
        views[0].models.set_lead(360, now());
        views[1].models.env_srh_km = 1;
        let independent = views[2].models.clone();
        propagate(&mut views, 0, now());
        assert_eq!(
            views[1].models.model_sel,
            Selection {
                model: BModel::Rap,
                product: Product::Srh
            }
        );
        assert_eq!(views[1].models.env_model, wxdata::hrrr::Model::Rap);
        assert_eq!(views[1].models.env_srh_km, 1);
        assert_eq!(views[1].models.lead_min(), 360);
        assert_eq!(views[1].models.model_run, views[0].models.model_run);
        assert_eq!(views[2].models, independent);
    }

    #[test]
    fn model_groups_unlink_retains_context_and_join_adopts_existing_owner() {
        let mut views = vec![
            pane(BModel::Gfs, Product::Temp2m, Some(2)),
            pane(BModel::Ecmwf, Product::Mslp, None),
        ];
        views[0].models.set_lead(540, now());
        join(&mut views, 1, Some(2), now());
        assert_eq!(views[1].models.model_sel.model, BModel::Gfs);
        assert_eq!(views[1].models.model_sel.product, Product::Mslp);
        assert_eq!(views[1].models.lead_min(), 540);
        let unlinked = views[1].models.clone();
        join(&mut views, 1, None, now());
        views[0].models.set_lead(720, now());
        propagate(&mut views, 0, now());
        assert_eq!(views[1].models, unlinked);
    }

    #[test]
    fn model_groups_incompatible_products_move_to_a_published_product() {
        let mut views = vec![
            pane(BModel::Rtma, Product::AnalysisTemp2m, Some(1)),
            pane(BModel::Hrrr, Product::Reflectivity, Some(1)),
        ];
        propagate(&mut views, 0, now());
        assert_eq!(views[1].models.model_sel.model, BModel::Rtma);
        assert_eq!(
            views[1].models.model_sel.product,
            BModel::Rtma.default_product()
        );
        assert!(!views[1]
            .fields_on
            .contains(&crate::render::FieldLayer::Hrrr));
        assert!(views[1]
            .fields_on
            .contains(&views[1].models.model_sel.layer()));
        assert_eq!(views[1].models.lead_min(), 0);
    }

    #[test]
    fn model_groups_reordering_panes_and_reusing_groups_keeps_attached_contexts() {
        let mut views = vec![
            pane(BModel::Gfs, Product::Temp2m, Some(1)),
            pane(BModel::Rap, Product::Cape, Some(2)),
            pane(BModel::Gfs, Product::Mslp, Some(1)),
        ];
        views.swap(0, 1);
        views[1].models.set_lead(900, now());
        propagate(&mut views, 1, now());
        assert_eq!(views[0].models.model_sel.model, BModel::Rap);
        assert_eq!(views[2].models.lead_min(), 900);
        views.remove(1);
        join(&mut views, 0, Some(3), now());
        assert_eq!(views[0].models.model_sel.model, BModel::Rap);
        assert_eq!(views[1].model_group, Some(1));
    }
}
