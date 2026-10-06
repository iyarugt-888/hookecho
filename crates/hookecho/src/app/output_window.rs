//! The output window (ROADMAP_2 §6.1): a clean picture of the active pane in a window of its own,
//! apart from the operator's controls, for OBS or a second screen. It carries the same broadcast
//! dressing as streaming mode (clock, caption, warning crawl, logo), plus an optional title strap,
//! and none of the operator chrome. Its title stays "HookEcho Output" so a capture source finds it
//! again after a restart.
//!
//! Sized in physical pixels (1080p, 1440p, 4K), so what OBS captures is the size asked for
//! whatever the display scaling, or left free to size by hand; fullscreen fills the monitor the
//! window is on. Native only: a browser tab has one window.

use super::*;

/// What size the output is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum OutputSize {
    #[default]
    Hd1080,
    Qhd1440,
    Uhd2160,
    /// Whatever the window is dragged to.
    Free,
}

impl OutputSize {
    const ALL: [OutputSize; 4] = [Self::Hd1080, Self::Qhd1440, Self::Uhd2160, Self::Free];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Hd1080 => "1920 × 1080",
            Self::Qhd1440 => "2560 × 1440",
            Self::Uhd2160 => "3840 × 2160",
            Self::Free => "Free size",
        }
    }

    /// Physical pixels, for the fixed sizes.
    pub(crate) fn pixels(self) -> Option<[f32; 2]> {
        match self {
            Self::Hd1080 => Some([1920.0, 1080.0]),
            Self::Qhd1440 => Some([2560.0, 1440.0]),
            Self::Uhd2160 => Some([3840.0, 2160.0]),
            Self::Free => None,
        }
    }
}

/// Which pane the program output shows (ROADMAP_PARITY M6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ProgramSource {
    /// Not chosen yet: the pane active when the window opens is pinned.
    #[default]
    Unset,
    /// Whichever pane the operator is working in, as before M6.1 — chosen explicitly.
    FollowActive,
    /// This pane (0-based), whatever the operator does in the others.
    Pane(usize),
}

/// The pane program output shows, or why there is none: a pinned pane that has been closed is
/// not quietly replaced by another.
pub(crate) fn program_pane(
    source: ProgramSource,
    active: usize,
    panes: usize,
) -> Result<usize, String> {
    match source {
        ProgramSource::Unset | ProgramSource::FollowActive => {
            Ok(active.min(panes.saturating_sub(1)))
        }
        ProgramSource::Pane(i) if i < panes => Ok(i),
        ProgramSource::Pane(i) => Err(format!("pane {} is not open", i + 1)),
    }
}

/// The window's state; session-only.
#[derive(Default)]
pub(crate) struct OutputWindow {
    pub open: bool,
    /// The pane program shows.
    pub source: ProgramSource,
    /// The scene cued in preview, waiting for Take.
    pub cued: Option<crate::broadcast::Scene>,
    /// Program keeps its own camera: the operator panning or zooming the program pane does not
    /// move the output, and Take sets it.
    pub hold: bool,
    /// The held camera, once taken.
    pub held: Option<crate::render::mercator::Camera>,
    pub size: OutputSize,
    pub fullscreen: bool,
    /// A title strap along the top, when not empty.
    pub strap: String,
    /// The size the window was last asked to be, so a change is sent once.
    sent: Option<(OutputSize, bool)>,
}

/// Points to ask for so the window is `px` physical pixels at `ppp` pixels per point.
pub(crate) fn points_for(px: [f32; 2], ppp: f32) -> [f32; 2] {
    let ppp = if ppp.is_finite() && ppp > 0.0 {
        ppp
    } else {
        1.0
    };
    [px[0] / ppp, px[1] / ppp]
}

impl HookEchoApp {
    /// Draw the output window, while it is open.
    pub(crate) fn output_window(&mut self, ctx: &egui::Context) {
        self.scene_hotkeys(ctx);
        if !self.output.open || cfg!(target_arch = "wasm32") {
            return;
        }
        // Program is pinned to the pane active when the window opened, unless chosen otherwise:
        // the operator moving to another pane does not move the picture on air.
        if self.output.source == ProgramSource::Unset {
            self.output.source = ProgramSource::Pane(self.active);
        }
        let ppp = ctx.pixels_per_point();
        let mut builder = egui::ViewportBuilder::default()
            .with_title("HookEcho Output")
            .with_fullscreen(self.output.fullscreen);
        if let Some(px) = self.output.size.pixels() {
            builder = builder.with_inner_size(points_for(px, ppp));
        }
        let want = (self.output.size, self.output.fullscreen);
        let resend = self.output.sent != Some(want);
        let program = program_pane(self.output.source, self.active, self.views.len());
        let mut close = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("hookecho-output"),
            builder,
            |vctx, _class| {
                if resend {
                    // A size or fullscreen change after the window exists.
                    vctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(want.1));
                    if let (Some(px), false) = (want.0.pixels(), want.1) {
                        vctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                            points_for(px, vctx.pixels_per_point()).into(),
                        ));
                    }
                }
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.fill(egui::Color32::BLACK))
                    .show(vctx, |ui| {
                        let rect = ui.max_rect();
                        let octx = ui.ctx().clone();
                        let idx = match &program {
                            Ok(idx) => *idx,
                            Err(why) => {
                                // Said on the output, not swapped for another pane's picture.
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    format!("No program: {why}"),
                                    egui::FontId::proportional(24.0),
                                    egui::Color32::from_gray(160),
                                );
                                return;
                            }
                        };
                        // A passenger like the mini-loop: the main window's own pane loop owns
                        // the tile caches. A held program view swaps its camera in around the
                        // render, exactly as the mini-loop does, and the pane gets its own back.
                        let held = self.output.hold.then(|| {
                            let cam = *self.output.held.get_or_insert(self.views[idx].camera);
                            std::mem::replace(&mut self.views[idx].camera, cam)
                        });
                        self.render_pane(ui, &octx, idx, rect, false, false, false, false, &[]);
                        if let Some(pane_cam) = held {
                            self.output.held =
                                Some(std::mem::replace(&mut self.views[idx].camera, pane_cam));
                        }
                        let painter = octx.layer_painter(egui::LayerId::new(
                            egui::Order::Foreground,
                            egui::Id::new("output_dressing"),
                        ));
                        self.paint_broadcast(&octx, &painter, rect);
                        paint_strap(&painter, rect, &self.output.strap, &self.settings.broadcast);
                    });
                if vctx.input(|i| i.key_pressed(egui::Key::Escape)) && want.1 {
                    self.output.fullscreen = false;
                }
                if vctx.input(|i| i.viewport().close_requested()) {
                    close = true;
                }
            },
        );
        self.output.sent = Some(want);
        if close {
            self.output.open = false;
            self.output.sent = None;
        }
    }

    /// The streaming overlay's settings and the output window's, where streaming mode is set.
    pub(crate) fn streaming_rows(&mut self, ui: &mut egui::Ui) {
        let toggle = |ui: &mut egui::Ui, v: &mut bool, label: &str| ui.checkbox(v, label);
        ui.collapsing("Streaming overlay", |ui| {
            let b = &mut self.settings.broadcast;
            ui.add(
                egui::Slider::new(&mut b.safe_margin_pct, 0.0..=15.0)
                    .suffix(" %")
                    .text("Safe margin"),
            )
            .on_hover_text(
                "Keep the clock, caption, crawl and logo this far in from the edge — \
                 5 % is the broadcast convention",
            );
            toggle(ui, &mut b.clock, "Clock");
            toggle(ui, &mut b.caption, "Source caption");
            toggle(ui, &mut b.crawl, "Warning crawl");
            toggle(ui, &mut b.legend, "Colour scale");
            let mut logo = b.logo.clone().unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label("Logo");
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut logo)
                            .hint_text("path to a PNG")
                            .desired_width(160.0),
                    )
                    .changed()
                {
                    b.logo = (!logo.trim().is_empty()).then(|| logo.trim().to_string());
                }
            });
            if b.logo
                .as_deref()
                .is_some_and(|p| !std::path::Path::new(p).is_file())
            {
                ui.weak("No image at that path");
            }
        });
        // Scenes drive streaming mode on the main map too, so they are on every target.
        self.scene_rows(ui);
        if cfg!(target_arch = "wasm32") {
            return;
        }
        ui.collapsing("Output window", |ui| {
            let o = &mut self.output;
            toggle(ui, &mut o.open, "Open the output window").on_hover_text(
                "A pane, clean, in a window of its own for OBS or a second screen; capture the \
                 window titled \u{201c}HookEcho Output\u{201d}",
            );
            let panes = self.views.len();
            let shown = match o.source {
                ProgramSource::Unset => "The pane active when it opens".to_string(),
                ProgramSource::FollowActive => "Follow the active pane".to_string(),
                ProgramSource::Pane(i) if i < panes => format!("Pane {}", i + 1),
                ProgramSource::Pane(i) => format!("Pane {} (not open)", i + 1),
            };
            egui::ComboBox::from_label("Program shows")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for i in 0..panes {
                        ui.selectable_value(
                            &mut o.source,
                            ProgramSource::Pane(i),
                            format!("Pane {}", i + 1),
                        );
                    }
                    ui.selectable_value(
                        &mut o.source,
                        ProgramSource::FollowActive,
                        "Follow the active pane",
                    );
                })
                .response
                .on_hover_text(
                    "Pin program to one pane so working in the others never changes what is on \
                     air; scenes are taken into this pane",
                );
            if toggle(ui, &mut o.hold, "Hold the program view")
                .on_hover_text(
                    "Program keeps its own camera: panning or zooming that pane no longer moves \
                     the output, and Take sets it",
                )
                .changed()
            {
                // Held from where the pane is looking now; released, the output follows it again.
                o.held = None;
            }
            egui::ComboBox::from_label("Size")
                .selected_text(o.size.label())
                .show_ui(ui, |ui| {
                    for s in OutputSize::ALL {
                        ui.selectable_value(&mut o.size, s, s.label());
                    }
                })
                .response
                .on_hover_text("Physical pixels, whatever the display scaling");
            toggle(ui, &mut o.fullscreen, "Fullscreen")
                .on_hover_text("On the monitor the window is on; Esc there leaves it");
            ui.horizontal(|ui| {
                ui.label("Title strap");
                ui.add(
                    egui::TextEdit::singleline(&mut o.strap)
                        .hint_text("e.g. Severe weather coverage")
                        .desired_width(180.0),
                );
            });
        });
    }
}

/// A title strap along the top of the picture, inside the safe margin: a dark band with the text.
fn paint_strap(
    painter: &egui::Painter,
    rect: egui::Rect,
    text: &str,
    style: &crate::broadcast::Broadcast,
) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let inner = rect.shrink(style.margin_px(rect.width(), rect.height()).max(12.0));
    let size = (rect.height() * 0.035).clamp(16.0, 64.0);
    let galley = painter.layout_no_wrap(
        text.to_string(),
        egui::FontId::proportional(size),
        egui::Color32::WHITE,
    );
    let pad = egui::vec2(size * 0.6, size * 0.3);
    let band = egui::Rect::from_min_size(
        egui::pos2(
            inner.left(),
            inner.bottom() - galley.size().y - pad.y * 2.0 - size * 3.0,
        ),
        galley.size() + pad * 2.0,
    );
    painter.rect_filled(band, 3.0, egui::Color32::from_black_alpha(200));
    painter.rect_filled(
        egui::Rect::from_min_size(band.min, egui::vec2(size * 0.18, band.height())),
        0.0,
        egui::Color32::from_rgb(220, 40, 40),
    );
    painter.galley(
        band.min + pad + egui::vec2(size * 0.2, 0.0),
        galley,
        egui::Color32::WHITE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_stays_on_its_pane_and_a_closed_one_is_not_replaced() {
        use ProgramSource::*;
        // The operator moves to pane 3: a pinned program stays on pane 1.
        assert_eq!(program_pane(Pane(0), 2, 4), Ok(0));
        assert_eq!(program_pane(FollowActive, 2, 4), Ok(2));
        assert_eq!(program_pane(Unset, 2, 4), Ok(2));
        // Pane 3 closed: no program, said, rather than whichever pane is now third or active.
        assert_eq!(
            program_pane(Pane(2), 0, 2),
            Err("pane 3 is not open".into())
        );
        assert_eq!(program_pane(FollowActive, 5, 2), Ok(1));
    }

    #[test]
    fn the_output_is_the_asked_for_pixels_at_any_scaling() {
        assert_eq!(points_for([1920.0, 1080.0], 1.0), [1920.0, 1080.0]);
        assert_eq!(points_for([1920.0, 1080.0], 1.5), [1280.0, 720.0]);
        assert_eq!(points_for([3840.0, 2160.0], 2.0), [1920.0, 1080.0]);
        assert_eq!(
            points_for([1920.0, 1080.0], 0.0),
            [1920.0, 1080.0],
            "a bad scale is 1"
        );
        assert_eq!(OutputSize::Free.pixels(), None);
    }
}
