//! Storm Digest window: a briefing of the storms in view from the app's own radar analysis. The
//! app fills `text` (the built-in summary instantly; Claude's or Gemini's analysis when the chosen
//! provider has a key) and `facts`, the data the model was given, which the window can show.

#[derive(Default)]
pub struct DigestWindow {
    pub open: bool,
    pub text: String,
    pub busy: bool,
    /// True once a model-written version replaced the templated text.
    pub enhanced: bool,
    /// The model asked for this digest ("Claude", "Gemini").
    pub provider: &'static str,
    /// Why the model's rewrite failed, when it did; the built-in summary stands meanwhile.
    pub error: Option<String>,
    /// The fact sheet the briefing was written from.
    pub facts: String,
}

pub enum DigestAction {
    Generate,
}

impl DigestWindow {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        drawer: &mut crate::ui::drawer::Drawer,
    ) -> Option<DigestAction> {
        if !self.open {
            return None;
        }
        let mut open = self.open;
        let mut action = None;
        let Some(window) = drawer.page(
            ctx,
            "Storm Digest",
            &mut open,
            false,
            egui::Window::new("Storm Digest"),
        ) else {
            self.open = open;
            return None;
        };
        window.show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.add_enabled(!self.busy, egui::Button::new("↻ Generate")).clicked() {
                        action = Some(DigestAction::Generate);
                    }
                    if self.busy {
                        ui.spinner();
                        ui.weak(format!("asking {}…", self.provider));
                    } else if self.enhanced {
                        ui.weak(format!("· written by {}", self.provider));
                    }
                });
                ui.separator();
                if let Some(e) = &self.error {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        format!("{} could not write this one: {e}", self.provider),
                    );
                }
                if self.text.is_empty() {
                    ui.weak("Click Generate for a briefing of the storms in view, from the radar's rotation, debris, hail and storm-cell analysis.");
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("digest_text")
                        .max_height(360.0)
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(&self.text).size(14.0));
                        });
                }
                if !self.facts.is_empty() {
                    egui::CollapsingHeader::new("Radar data used")
                        .id_salt("digest_facts")
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("digest_facts_scroll")
                                .max_height(260.0)
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new(&self.facts).monospace().size(11.0));
                                });
                        });
                }
                ui.add_space(6.0);
                ui.weak("Set an Anthropic or Google AI Studio key in Settings ▸ General ▸ AI to have Claude or Gemini analyse this data; otherwise a built-in summary is used.");
            });
        self.open = open;
        action
    }
}
