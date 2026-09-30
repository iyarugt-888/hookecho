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

    fn label(self) -> &'static str {
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

/// The window's state; session-only.
#[derive(Default)]
pub(crate) struct OutputWindow {
    pub open: bool,
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
        if !self.output.open || cfg!(target_arch = "wasm32") {
            return;
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
        let idx = self.active.min(self.views.len() - 1);
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
                        // A passenger like the mini-loop: the main window's own pane loop owns
                        // the tile caches.
                        self.render_pane(ui, &octx, idx, rect, false, false, false, false, &[]);
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
        if cfg!(target_arch = "wasm32") {
            return;
        }
        ui.collapsing("Output window", |ui| {
            let o = &mut self.output;
            toggle(ui, &mut o.open, "Open the output window").on_hover_text(
                "The active pane, clean, in a window of its own for OBS or a second screen; \
                 capture the window titled \u{201c}HookEcho Output\u{201d}",
            );
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
