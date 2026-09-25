//! Streaming mode's broadcast dressing (ROADMAP_NEW M1): the same [`crate::broadcast::Broadcast`]
//! style `--watch` stamps into its frames, drawn live with egui over the map — a clock, a caption,
//! a warning crawl and a logo, inside the title-safe margin. The colour scale's switch is honoured
//! where the map draws its legend.

use super::*;
use egui::{Align2, Color32, FontId, Pos2, Rect};

/// The crawl band's height and text size, px at the app's own scale.
const BAND_H: f32 = 34.0;
const BAND_PX: f32 = 15.0;
/// Room the clock leaves at the map's right edge for the vertical colour scale and its labels.
const LEGEND_CLEAR: f32 = 76.0;

impl HookEchoApp {
    /// Draw the dressing over the map while streaming mode is on.
    pub(crate) fn broadcast_dressing(&mut self, ctx: &egui::Context) {
        if !self.obs_mode {
            return;
        }
        let style = self.settings.broadcast.clone();
        let rect = self.chrome_rect;
        let inner = rect.shrink(style.margin_px(rect.width(), rect.height()).max(12.0));
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("broadcast_dressing"),
        ));
        let v = &self.views[self.active];
        let valid = v.timeline.current().and_then(|id| id.date_time());
        let site = v.site.clone().unwrap_or_default();
        let tz = self.active_tz();

        let mut bottom = inner.bottom();
        if style.crawl {
            if let Some(line) = valid.and_then(|t| self.crawl_line(t)) {
                let band = Rect::from_min_max(
                    Pos2::new(inner.left(), inner.bottom() - BAND_H),
                    inner.right_bottom(),
                );
                crawl_band(&painter, band, &line);
                bottom = band.top() - 6.0;
            }
        }
        if style.caption {
            let tilt = v
                .volume
                .as_ref()
                .and_then(|x| x.elevations.get(v.tilt))
                .map(|a| format!(" {a:.1}\u{b0}"))
                .unwrap_or_default();
            let when = valid
                .map(|t| format!(" \u{b7} {}", crate::timefmt::fmt_date_clock(t, tz)))
                .unwrap_or_default();
            let caption = format!(
                "{site} \u{b7} {}{tilt}{when} \u{b7} HookEcho",
                v.moment.short_name()
            );
            shadowed(
                &painter,
                Pos2::new(inner.left(), bottom),
                Align2::LEFT_BOTTOM,
                &caption,
                16.0,
                Color32::from_rgb(240, 240, 245),
            );
        }
        // The map draws the colour scale down its right edge (`ui::legend::draw_vertical`); the
        // clock stands clear of it rather than on it.
        let scale_shown = style.legend && v.show_legend && v.volume.is_some();
        let clock_right = if scale_shown {
            inner.right().min(rect.right() - LEGEND_CLEAR)
        } else {
            inner.right()
        };
        if style.clock {
            if let Some(t) = valid {
                let (time, sub) = crate::broadcast::clock_lines(&site, t);
                let big = shadowed(
                    &painter,
                    Pos2::new(clock_right, inner.top()),
                    Align2::RIGHT_TOP,
                    &time,
                    34.0,
                    Color32::WHITE,
                );
                shadowed(
                    &painter,
                    Pos2::new(clock_right, big.bottom() + 2.0),
                    Align2::RIGHT_TOP,
                    &sub,
                    15.0,
                    Color32::from_rgb(215, 215, 222),
                );
            }
        }
        if let Some(path) = style.logo.as_deref() {
            if let Some(tex) = self.broadcast_logo(ctx, path) {
                let size = tex.size_vec2();
                let fit = (rect.height() * 0.09 / size.y)
                    .min(rect.width() * 0.30 / size.x)
                    .min(1.0);
                let r = Rect::from_min_size(inner.left_top(), size * fit);
                painter.image(
                    tex.id(),
                    r,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
    }

    /// The crawl's line for a frame valid at `t`: the warnings in force then that touch the view.
    fn crawl_line(&self, t: chrono::DateTime<chrono::Utc>) -> Option<String> {
        let (x0, y0, x1, y1) = self.view_bounds();
        let in_view: Vec<_> = self
            .active_alert_features()
            .iter()
            .filter(|f| {
                f.bbox()
                    .is_some_and(|(a, b, c, d)| a <= x1 && x0 <= c && b <= y1 && y0 <= d)
            })
            .cloned()
            .collect();
        crate::broadcast::crawl_line(&crate::broadcast::crawl_items(&in_view, t))
    }

    /// The logo's texture, loaded once per path; `None` if the file cannot be read as an image.
    fn broadcast_logo(&mut self, ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
        if let Some((p, tex)) = &self.broadcast_logo {
            if p == path {
                return tex.clone();
            }
        }
        let tex = std::fs::read(path)
            .ok()
            .and_then(|bytes| image::load_from_memory(&bytes).ok())
            .map(|img| {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                ctx.load_texture(
                    "broadcast_logo",
                    egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
                    egui::TextureOptions::LINEAR,
                )
            });
        self.broadcast_logo = Some((path.to_string(), tex.clone()));
        tex
    }
}

/// The crawl band: dark, a red WARNINGS tag, and the line cut with an ellipsis to fit.
fn crawl_band(painter: &egui::Painter, band: Rect, line: &str) {
    painter.rect_filled(band, 0.0, Color32::from_rgba_unmultiplied(10, 12, 18, 225));
    let tag = painter.layout_no_wrap(
        "WARNINGS".to_string(),
        FontId::proportional(BAND_PX),
        Color32::WHITE,
    );
    let tag_rect = Rect::from_min_size(
        band.left_top(),
        egui::vec2(tag.size().x + 20.0, band.height()),
    );
    painter.rect_filled(tag_rect, 0.0, Color32::from_rgb(196, 30, 36));
    painter.galley(
        Pos2::new(tag_rect.left() + 10.0, band.center().y - tag.size().y / 2.0),
        tag,
        Color32::WHITE,
    );
    let mut job = egui::text::LayoutJob::simple_singleline(
        line.to_string(),
        FontId::proportional(BAND_PX),
        Color32::from_rgb(240, 240, 245),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(band.right() - tag_rect.right() - 20.0);
    let galley = painter.layout_job(job);
    painter.galley(
        Pos2::new(
            tag_rect.right() + 10.0,
            band.center().y - galley.size().y / 2.0,
        ),
        galley,
        Color32::WHITE,
    );
}

/// Text with a one-pixel black shadow, readable over bright echoes and dark map alike. Returns the
/// text's rect.
fn shadowed(
    painter: &egui::Painter,
    at: Pos2,
    anchor: Align2,
    text: &str,
    px: f32,
    color: Color32,
) -> Rect {
    painter.text(
        at + egui::vec2(1.0, 1.0),
        anchor,
        text,
        FontId::proportional(px),
        Color32::BLACK,
    );
    painter.text(at, anchor, text, FontId::proportional(px), color)
}
