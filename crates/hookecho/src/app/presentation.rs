//! Streaming mode on a touch screen (ROADMAP_PARITY M6.4): the clean view hides every control,
//! and its only ways out were F8 and F9, keys a tablet does not have. A tap now brings up a small
//! control strip for a few seconds — exit, the previous and next scene, save a still — and the
//! system Back gesture leaves streaming mode before it leaves the app. With nothing touched the
//! picture stays as clean as it was.

use super::*;

/// How long the strip stays up after the last touch, seconds.
const SHOW_SECS: f32 = 4.0;

#[derive(Default)]
pub(crate) struct Presentation {
    /// When the strip was last asked for.
    shown_at: Option<Instant>,
    /// Whether this session has seen a touch, so the hint names a tap rather than F8.
    touched: bool,
    /// The scene the strip last put on, for previous/next.
    scene: Option<usize>,
}

impl Presentation {
    fn visible(&self) -> bool {
        self.shown_at
            .is_some_and(|t| t.elapsed().as_secs_f32() < SHOW_SECS)
    }
}

/// The scene previous/next goes to among `count`: from the one last put on, wrapping; the first
/// (or last) when none has been. `None` without scenes.
pub(crate) fn step_scene(current: Option<usize>, count: usize, forward: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match (current.filter(|&i| i < count), forward) {
        (None, true) => 0,
        (None, false) => count - 1,
        (Some(i), true) => (i + 1) % count,
        (Some(i), false) => (i + count - 1) % count,
    })
}

/// What the strip asks for this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    Exit,
    Scene(bool),
    Still,
}

impl HookEchoApp {
    /// Leave streaming mode (and its warning tour).
    pub(crate) fn exit_presentation(&mut self) {
        self.obs_mode = false;
        self.obs_tour = false;
        self.presentation.shown_at = None;
    }

    /// While streaming mode is on: a tap anywhere shows the strip; the strip's buttons act.
    pub(crate) fn presentation_controls(&mut self, ctx: &egui::Context) {
        if !self.obs_mode {
            return;
        }
        let (touch, pressed) = ctx.input(|i| (i.any_touches(), i.pointer.any_pressed()));
        if touch {
            self.presentation.touched = true;
        }
        if touch || (pressed && self.presentation.touched) {
            self.presentation.shown_at = Some(Instant::now());
        }
        if !self.presentation.visible() {
            return;
        }
        // Keep counting down while nothing else asks for a frame.
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        let scenes = self.settings.scenes.len();
        let label = self
            .presentation
            .scene
            .and_then(|i| self.settings.scenes.get(i).map(|s| (i, s)))
            .map_or_else(
                || format!("{scenes} scene{}", if scenes == 1 { "" } else { "s" }),
                |(i, s)| format!("{} / {scenes} \u{b7} {}", i + 1, s.name),
            );
        let mut ask = None;
        // Top centre, under the hint: the bottom is the warning crawl's and the corners the
        // clock's and logo's. Above the dressing, which paints on the foreground layer.
        egui::Area::new("presentation_controls".into())
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 48.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_black_alpha(190))
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.spacing_mut().interact_size.y = 44.0;
                        ui.spacing_mut().button_padding = egui::vec2(14.0, 10.0);
                        ui.horizontal(|ui| {
                            if ui.button("\u{d7}  Exit").clicked() {
                                ask = Some(Ask::Exit);
                            }
                            ui.add_enabled_ui(scenes > 0, |ui| {
                                if ui
                                    .button("\u{25c0}")
                                    .on_hover_text("Previous scene")
                                    .clicked()
                                {
                                    ask = Some(Ask::Scene(false));
                                }
                                ui.label(egui::RichText::new(&label).color(egui::Color32::WHITE));
                                if ui.button("\u{25b6}").on_hover_text("Next scene").clicked() {
                                    ask = Some(Ask::Scene(true));
                                }
                            });
                            if ui.button("Save still").clicked() {
                                ask = Some(Ask::Still);
                            }
                        });
                    });
            });
        match ask {
            Some(Ask::Exit) => self.exit_presentation(),
            Some(Ask::Scene(forward)) => {
                let next = step_scene(self.presentation.scene, scenes, forward);
                if let Some(scene) = next.and_then(|i| self.settings.scenes.get(i).cloned()) {
                    self.presentation.scene = next;
                    self.take_scene(&scene);
                }
                self.presentation.shown_at = Some(Instant::now());
            }
            Some(Ask::Still) => {
                // The strip would be in the picture: hide it, then capture the clean view.
                self.presentation.shown_at = None;
                if let Some(path) = crate::dialog::save_path("hookecho.png", "png") {
                    self.request_capture(ctx, ShotDest::File(path));
                }
            }
            None => {}
        }
    }

    /// The hint over the clean view: how to get out with the input this device has.
    pub(crate) fn presentation_hint(&self) -> &'static str {
        match (
            self.presentation.touched || cfg!(target_os = "android"),
            self.obs_tour,
        ) {
            (true, true) => "Streaming \u{b7} tour (tap for controls \u{b7} Back exits)",
            (true, false) => "Streaming (tap for controls \u{b7} Back exits)",
            (false, true) => "OBS \u{b7} tour (F8 exit \u{b7} F9 stop tour)",
            (false, false) => "OBS mode (F8 exit \u{b7} F9 tour)",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previous_and_next_walk_the_scenes_and_wrap() {
        assert_eq!(step_scene(None, 0, true), None);
        assert_eq!(step_scene(None, 3, true), Some(0));
        assert_eq!(step_scene(None, 3, false), Some(2));
        assert_eq!(step_scene(Some(2), 3, true), Some(0));
        assert_eq!(step_scene(Some(0), 3, false), Some(2));
        assert_eq!(step_scene(Some(1), 3, true), Some(2));
        // A scene deleted since: start again from the ends.
        assert_eq!(step_scene(Some(7), 3, true), Some(0));
    }
}
