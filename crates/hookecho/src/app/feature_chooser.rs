//! "What's here" (ROADMAP_PARITY M4.3): where a click lands on more than one feature — imported
//! points or lines from several layers, or overlapping polygons that are not alerts — a short
//! list of them opens instead of the topmost one alone, so a feature underneath another is still
//! reachable. Alerts keep their own stack (`ui::warning_window`), which already lists overlaps.

use super::*;

/// One feature under the click.
#[derive(Clone)]
pub(crate) struct ChoiceItem {
    pub detail: Detail,
    /// The imported layer and source feature, so picking it also picks its table row.
    pub source: Option<(u64, usize)>,
    /// A discussion or watch: the key and outline its people count is computed for.
    pub impact: Option<(String, Vec<Vec<[f64; 2]>>)>,
}

impl HookEchoApp {
    /// Open what `item` is: its popup, its table row, its people count.
    pub(crate) fn open_choice(&mut self, item: ChoiceItem, ctx: &egui::Context) {
        if let Some((layer, src)) = item.source {
            self.note_gis_pick(layer, src);
        }
        self.warning_popup = None;
        self.gate_popup = None;
        self.detail_impact = item.impact.as_ref().map(|(k, _)| k.clone());
        if let Some((key, rings)) = item.impact {
            if !self.impacts.by_id.contains_key(&key) {
                self.request_impact(key, rings, ctx);
            }
        }
        self.detail = Some(item.detail);
    }

    /// The list, while one is open. Rows are buttons (a scrolled list does not deliver row clicks
    /// on a touch screen) and close it when one is chosen.
    pub(crate) fn feature_chooser_window(&mut self, ctx: &egui::Context) {
        let Some(items) = self.feature_chooser.take() else {
            return;
        };
        let mut open = true;
        let mut chosen = None;
        egui::Window::new("What's here")
            .id(egui::Id::new("feature_chooser"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(300.0)
            .show(ctx, |ui| {
                ui.weak(format!("{} features under that spot", items.len()));
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for (i, item) in items.iter().enumerate() {
                            let [r, g, b, _] = item.detail.color;
                            // The layer's own name, from the popup body's last line.
                            let layer = item
                                .detail
                                .body
                                .rsplit_once("Layer: ")
                                .map(|(_, l)| format!("  \u{b7} {l}"))
                                .unwrap_or_default();
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("\u{25a0}")
                                        .color(egui::Color32::from_rgb(r, g, b)),
                                );
                                if ui
                                    .add(
                                        egui::Button::new(format!("{}{layer}", item.detail.title))
                                            .min_size(egui::vec2(ui.available_width(), 32.0)),
                                    )
                                    .clicked()
                                {
                                    chosen = Some(i);
                                }
                            });
                        }
                    });
            });
        match chosen {
            Some(i) => {
                if let Some(item) = items.into_iter().nth(i) {
                    self.open_choice(item, ctx);
                }
            }
            None if open => self.feature_chooser = Some(items),
            None => {}
        }
    }
}

impl HookEchoApp {
    /// What a click with no alert under it could open: the imported points and lines (topmost
    /// layer first), then the polygons that are not alerts (in their paint order, top first).
    pub(crate) fn click_choices(
        &self,
        marks: &[(Detail, u64, usize)],
        hits: &[&wxdata::overlay::GeoFeature],
    ) -> Vec<ChoiceItem> {
        let mut out: Vec<ChoiceItem> = marks
            .iter()
            .map(|(detail, layer, src)| ChoiceItem {
                detail: detail.clone(),
                source: Some((*layer, *src)),
                impact: None,
            })
            .collect();
        for f in hits.iter().filter(|f| f.alert.is_none()) {
            let impact = matches!(
                f.kind,
                wxdata::overlay::FeatureKind::MesoDiscussion
                    | wxdata::overlay::FeatureKind::Watch
                    | wxdata::overlay::FeatureKind::WatchBox
            )
            .then(|| (format!("feature:{}", f.title), f.rings.clone()));
            out.push(ChoiceItem {
                detail: Detail {
                    title: f.title.clone(),
                    body: f.detail.clone(),
                    color: f.stroke,
                    image: None,
                    link: None,
                },
                source: self.overlay_source_of(f),
                impact,
            });
        }
        out
    }
}
