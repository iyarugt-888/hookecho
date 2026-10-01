//! Plain-language storm digest window: a "what does this mean for me" briefing of the in-view
//! weather. The app fills `text` (templated instantly; rewritten by Claude or Gemini if the
//! chosen provider has a key).

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
                    ui.weak("Click Generate for a plain-language briefing of the in-view weather.");
                } else {
                    ui.label(egui::RichText::new(&self.text).size(14.0));
                }
                ui.add_space(6.0);
                ui.weak("Set an Anthropic or Google AI Studio key in Settings ▸ General ▸ AI for friendlier prose; otherwise a built-in summary is used.");
            });
        self.open = open;
        action
    }
}
