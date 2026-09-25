//! Caption, color bar and city labels painted into a finished render, on the CPU.
//!
//! A shared image has to say what it is looking at and when — a radar picture with no site, no
//! time and no scale is a pretty texture. The app draws all three with egui over the map; this
//! does the same thing for the off-screen renders the server serves, without a window.
//!
//! Why the CPU and not a second `egui_wgpu` pass over the same target: the renders are produced on
//! whatever adapter the box has, including lavapipe, and a text pass through the GPU is the part
//! most likely to differ between them. Laying glyphs out with epaint and blitting the atlas by
//! hand is a few dozen lines, has no adapter in it at all, and is byte-identical everywhere.
//!
//! Native only — nothing here is reachable from the wasm build, which draws its chrome the normal
//! egui way.

use crate::colormap::ColorTable;
use egui::epaint::text::{FontId, Fonts, TextOptions};
use egui::Color32;

/// Distance from the image edge to anything drawn here.
const INSET: f32 = 14.0;
/// Caption size in pixels, at the reference width below. Everything scales off this.
const CAPTION_PX: f32 = 15.0;
/// The width the sizes above are tuned for; a bigger render gets proportionally bigger text.
const REFERENCE_W: f32 = 1000.0;

/// The color bar to draw, if the render has a moment to explain.
#[derive(Clone)]
pub struct Bar {
    pub table: ColorTable,
    /// e.g. `"dBZ"`.
    pub unit: &'static str,
}

/// Everything painted on top of a finished render.
#[derive(Default, Clone)]
pub struct Stamp {
    /// One line, bottom left: `KTLX · REF 0.5° · 2026-08-29 20:32Z · hookecho.io`.
    pub caption: String,
    pub bar: Option<Bar>,
    /// City labels, already projected to pixels by the caller — this module knows nothing about
    /// cameras, and the caller is the only place the projection exists.
    pub labels: Vec<(f32, f32, String)>,
    /// Title-safe margin, px: nothing framing the picture comes nearer the edge (0 keeps the
    /// ordinary inset). See `crate::broadcast`.
    pub margin: f32,
    /// A clock for the top-right corner: the time large, a second line (date, zone) under it.
    pub clock: Option<(String, String)>,
    /// A warning crawl: one line in a band along the bottom.
    pub crawl: Option<String>,
    /// A logo for the top-left corner, drawn at up to 9 % of the frame's height.
    pub logo: Option<image::RgbaImage>,
}

/// The buffer being painted into: an RGBA8 image and its dimensions, kept together so the
/// drawing helpers take one argument for "where" instead of three.
struct Canvas<'a> {
    rgba: &'a mut [u8],
    w: u32,
    h: u32,
}

/// Paint `stamp` into an RGBA8 buffer of `w`x`h` pixels, in place.
pub fn draw(rgba: &mut [u8], w: u32, h: u32, stamp: &Stamp) {
    if rgba.len() < (w as usize * h as usize * 4) {
        return;
    }
    let canvas = &mut Canvas { rgba, w, h };
    let mut fonts = Fonts::new(
        TextOptions {
            max_texture_side: 2048,
            ..Default::default()
        },
        crate::fonts::base(),
    );
    let scale = (w as f32 / REFERENCE_W).clamp(0.7, 2.0);

    // Labels first, around the frame: the caption, the bar, the clock, the crawl and the logo are
    // the frame, so their places are taken before any city name is placed — a name is dropped
    // rather than drawn under or over them.
    let label_px = 12.0 * scale;
    let mut taken: Vec<[f32; 4]> = reserved(&mut fonts, w as f32, h as f32, stamp, scale);
    for (x, y, name) in &stamp.labels {
        let size = text_size(&mut fonts, name, label_px);
        let (x0, y0) = (x - size.0 / 2.0, y - size.1 / 2.0);
        let box_ = [x0, y0, x0 + size.0, y0 + size.1];
        // Greedy: first label in wins its space, later ones that would collide are dropped. A
        // proper label solver is a different project; ten names on a radar picture is the job.
        // A name the canvas cuts in half ("Lafayett") reads as a rendering fault rather than a
        // place, so a label that does not fit whole is not drawn at all.
        if x0 < 0.0 || y0 < 0.0 || box_[2] > w as f32 || box_[3] > h as f32 {
            continue;
        }
        if taken.iter().any(|t| overlaps(t, &box_)) {
            continue;
        }
        taken.push(box_);
        text(
            canvas,
            &mut fonts,
            name,
            label_px,
            x0,
            y0,
            Color32::from_rgb(235, 235, 240),
        );
    }

    let caption_px = CAPTION_PX * scale;
    let inset = (INSET * scale).max(stamp.margin);
    // The crawl owns the bottom edge; the caption and the bar sit on top of it.
    let bottom = match &stamp.crawl {
        Some(line) => {
            h as f32 - inset - draw_crawl(canvas, &mut fonts, line, inset, scale) - 6.0 * scale
        }
        None => h as f32 - inset,
    };
    if !stamp.caption.is_empty() {
        let size = text_size(&mut fonts, &stamp.caption, caption_px);
        text(
            canvas,
            &mut fonts,
            &stamp.caption,
            caption_px,
            inset,
            bottom - size.1,
            Color32::from_rgb(240, 240, 245),
        );
    }

    if let Some(bar) = &stamp.bar {
        draw_bar(canvas, &mut fonts, bar, scale, inset, bottom);
    }
    if let Some((time, sub)) = &stamp.clock {
        draw_clock(canvas, &mut fonts, time, sub, inset, scale);
    }
    if let Some(logo) = &stamp.logo {
        draw_logo(canvas, logo, inset);
    }
}

/// The boxes the frame's own furniture will occupy, padded, so city labels keep clear of them.
fn reserved(fonts: &mut Fonts, w: f32, h: f32, stamp: &Stamp, scale: f32) -> Vec<[f32; 4]> {
    let inset = (INSET * scale).max(stamp.margin);
    let pad = 4.0 * scale;
    let mut boxes = Vec::new();
    let mut bottom = h - inset;
    if stamp.crawl.is_some() {
        let band_h = 32.0 * scale;
        boxes.push([inset, bottom - band_h, w - inset, bottom]);
        bottom -= band_h + 6.0 * scale;
    }
    if !stamp.caption.is_empty() {
        let (cw, ch) = text_size(fonts, &stamp.caption, CAPTION_PX * scale);
        boxes.push([inset, bottom - ch, inset + cw, bottom]);
    }
    if stamp.bar.is_some() {
        let bar_w = (w * 0.32).min(360.0 * scale);
        let tick_h = text_size(fonts, "0", 11.0 * scale).1;
        boxes.push([
            w - inset - bar_w,
            bottom - 12.0 * scale - tick_h,
            w - inset,
            bottom,
        ]);
    }
    if let Some((time, sub)) = &stamp.clock {
        let t = text_size(fonts, time, 30.0 * scale);
        let s = text_size(fonts, sub, 13.0 * scale);
        let tw = t.0.max(s.0);
        boxes.push([
            w - inset - tw,
            inset,
            w - inset,
            inset + t.1 + 2.0 * scale + s.1,
        ]);
    }
    if let Some(logo) = &stamp.logo {
        if logo.width() > 0 && logo.height() > 0 {
            let fit = (h * 0.09 / logo.height() as f32)
                .min(w * 0.30 / logo.width() as f32)
                .min(1.0);
            boxes.push([
                inset,
                inset,
                inset + logo.width() as f32 * fit,
                inset + logo.height() as f32 * fit,
            ]);
        }
    }
    boxes
        .into_iter()
        .map(|[x0, y0, x1, y1]| [x0 - pad, y0 - pad, x1 + pad, y1 + pad])
        .collect()
}

/// The crawl: a dark band across the bottom inside the margin, a red WARNINGS tag, and the line —
/// cut with an ellipsis where it would run past the band (a still cannot scroll). Returns the
/// band's height.
fn draw_crawl(c: &mut Canvas<'_>, fonts: &mut Fonts, line: &str, inset: f32, scale: f32) -> f32 {
    let (w, h) = (c.w as f32, c.h as f32);
    let band_h = 32.0 * scale;
    let (x0, x1) = (inset, w - inset);
    let (y0, y1) = (h - inset - band_h, h - inset);
    c.fill(x0, y0, x1, y1, [10, 12, 18], 225);
    let px = 14.0 * scale;
    let tag = "WARNINGS";
    let tag_size = text_size(fonts, tag, px);
    let pad = 10.0 * scale;
    let tag_x1 = x0 + tag_size.0 + 2.0 * pad;
    c.fill(x0, y0, tag_x1, y1, [196, 30, 36], 255);
    let ty = y0 + (band_h - tag_size.1) / 2.0;
    text(c, fonts, tag, px, x0 + pad, ty, Color32::WHITE);
    let room = x1 - tag_x1 - 2.0 * pad;
    let fitted = fit_line(fonts, line, px, room);
    text(
        c,
        fonts,
        &fitted,
        px,
        tag_x1 + pad,
        ty,
        Color32::from_rgb(240, 240, 245),
    );
    band_h
}

/// `line`, cut to what fits in `room` px with an ellipsis, or whole if it fits.
fn fit_line(fonts: &mut Fonts, line: &str, px: f32, room: f32) -> String {
    if text_size(fonts, line, px).0 <= room {
        return line.to_string();
    }
    let chars: Vec<char> = line.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>() + "\u{2026}";
        if text_size(fonts, &candidate, px).0 <= room {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo]
        .iter()
        .collect::<String>()
        .trim_end()
        .to_string()
        + "\u{2026}"
}

/// The clock, right-aligned inside the top-right margin: the time large, the second line small.
fn draw_clock(
    c: &mut Canvas<'_>,
    fonts: &mut Fonts,
    time: &str,
    sub: &str,
    inset: f32,
    scale: f32,
) {
    // One pixel in: every glyph carries a one-pixel shadow down and to the right.
    let right = c.w as f32 - inset - 1.0;
    let big = 30.0 * scale;
    let small = 13.0 * scale;
    let t = text_size(fonts, time, big);
    text(c, fonts, time, big, right - t.0, inset, Color32::WHITE);
    let s = text_size(fonts, sub, small);
    let dim = Color32::from_rgb(215, 215, 222);
    text(
        c,
        fonts,
        sub,
        small,
        right - s.0,
        inset + t.1 + 2.0 * scale,
        dim,
    );
}

/// The logo in the top-left corner, scaled (never up) to at most 9 % of the height and 30 % of the
/// width, composited with its own transparency.
fn draw_logo(c: &mut Canvas<'_>, logo: &image::RgbaImage, inset: f32) {
    let (w, h) = (c.w as f32, c.h as f32);
    if logo.width() == 0 || logo.height() == 0 {
        return;
    }
    let fit = (h * 0.09 / logo.height() as f32)
        .min(w * 0.30 / logo.width() as f32)
        .min(1.0);
    let (lw, lh) = (
        ((logo.width() as f32 * fit).round() as u32).max(1),
        ((logo.height() as f32 * fit).round() as u32).max(1),
    );
    let scaled = image::imageops::resize(logo, lw, lh, image::imageops::FilterType::Triangle);
    let (ox, oy) = (inset as u32, inset as u32);
    for (x, y, p) in scaled.enumerate_pixels() {
        c.blend(ox + x, oy + y, [p[0], p[1], p[2]], p[3]);
    }
}

/// The moment's color scale as a horizontal ramp above the bottom-right corner, with its ends
/// labelled. Sampled from the same [`ColorTable`] the shader's LUT is baked from, so the picture
/// and its key cannot drift apart.
fn draw_bar(c: &mut Canvas<'_>, fonts: &mut Fonts, bar: &Bar, scale: f32, inset: f32, bottom: f32) {
    let (w, h) = (c.w, c.h);
    let (Some(first), Some(last)) = (bar.table.stops.first(), bar.table.stops.last()) else {
        return;
    };
    let (vmin, vmax) = (first.value, last.value);
    let span = (vmax - vmin).max(f32::EPSILON);

    let _ = h;
    let bar_h = 10.0 * scale;
    let bar_w = (w as f32 * 0.32).min(360.0 * scale);
    let x1 = w as f32 - inset;
    let x0 = x1 - bar_w;
    let y1 = bottom;
    let y0 = y1 - bar_h;

    for px in 0..bar_w as u32 {
        let value = vmin + (px as f32 / bar_w) * span;
        let Some(color) = bar.table.sample(value) else {
            continue;
        };
        for py in y0 as u32..y1 as u32 {
            c.blend(x0 as u32 + px, py, [color[0], color[1], color[2]], 255);
        }
    }

    let tick_px = 11.0 * scale;
    let low = format!("{vmin:.0}");
    let high = format!("{vmax:.0} {}", bar.unit);
    let low_size = text_size(fonts, &low, tick_px);
    let high_size = text_size(fonts, &high, tick_px);
    let ty = y0 - low_size.1 - 2.0 * scale;
    let dim = Color32::from_rgb(215, 215, 220);
    text(c, fonts, &low, tick_px, x0, ty, dim);
    text(c, fonts, &high, tick_px, x1 - high_size.0 - 1.0, ty, dim);
}

fn overlaps(a: &[f32; 4], b: &[f32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

fn text_size(fonts: &mut Fonts, s: &str, px: f32) -> (f32, f32) {
    let galley = fonts.with_pixels_per_point(1.0).layout_no_wrap(
        s.to_owned(),
        FontId::proportional(px),
        Color32::WHITE,
    );
    (galley.rect.width(), galley.rect.height())
}

/// Blit one line of text with its top-left at (`x`, `y`).
///
/// Drawn twice: once in black one pixel down-right, then in `color`. The halo is what makes a
/// caption readable over a bright echo and over an empty dark map both.
fn text(c: &mut Canvas<'_>, fonts: &mut Fonts, s: &str, px: f32, x: f32, y: f32, color: Color32) {
    let galley = fonts.with_pixels_per_point(1.0).layout_no_wrap(
        s.to_owned(),
        FontId::proportional(px),
        color,
    );
    // The atlas has to be read after layout: laying the text out is what allocates its glyphs.
    let atlas = fonts.image();
    let [aw, _ah] = atlas.size;
    for (dx, dy, ink) in [(1.0, 1.0, Color32::BLACK), (0.0, 0.0, color)] {
        for row in &galley.rows {
            for glyph in &row.row.glyphs {
                if glyph.uv_rect.is_nothing() {
                    continue;
                }
                let left = row.pos.x + glyph.pos.x + glyph.uv_rect.offset.x + x + dx;
                let top = row.pos.y + glyph.pos.y + glyph.uv_rect.offset.y + y + dy;
                let gw = (glyph.uv_rect.max[0] - glyph.uv_rect.min[0]) as u32;
                let gh = (glyph.uv_rect.max[1] - glyph.uv_rect.min[1]) as u32;
                for gy in 0..gh {
                    for gx in 0..gw {
                        let tx = glyph.uv_rect.min[0] as usize + gx as usize;
                        let ty = glyph.uv_rect.min[1] as usize + gy as usize;
                        let Some(texel) = atlas.pixels.get(ty * aw + tx) else {
                            continue;
                        };
                        let alpha = texel.a();
                        if alpha == 0 {
                            continue;
                        }
                        let (px_x, px_y) = (left + gx as f32, top + gy as f32);
                        if px_x < 0.0 || px_y < 0.0 {
                            continue;
                        }
                        c.blend(px_x as u32, px_y as u32, [ink.r(), ink.g(), ink.b()], alpha);
                    }
                }
            }
        }
    }
}

/// Source-over one pixel, in straight (unpremultiplied) alpha, so a transparent overlay frame
/// keeps its transparency around what is drawn — over an opaque pixel it is the ordinary blend.
/// Out-of-bounds writes are dropped rather than wrapped — a label near the edge is clipped, never
/// smeared onto the opposite side.
impl Canvas<'_> {
    fn blend(&mut self, x: u32, y: u32, color: [u8; 3], alpha: u8) {
        if x >= self.w || y >= self.h || alpha == 0 {
            return;
        }
        let i = (y as usize * self.w as usize + x as usize) * 4;
        let src_a = f32::from(alpha) / 255.0;
        let dst_a = f32::from(self.rgba[i + 3]) / 255.0;
        let out_a = src_a + dst_a * (1.0 - src_a);
        for (k, &c) in color.iter().enumerate() {
            let dst = f32::from(self.rgba[i + k]);
            let v = (f32::from(c) * src_a + dst * dst_a * (1.0 - src_a)) / out_a.max(f32::EPSILON);
            self.rgba[i + k] = v.round().clamp(0.0, 255.0) as u8;
        }
        self.rgba[i + 3] = (out_a * 255.0).round() as u8;
    }

    /// Source-over a rectangle of one colour.
    fn fill(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: [u8; 3], alpha: u8) {
        let (x0, y0) = (x0.max(0.0) as u32, y0.max(0.0) as u32);
        let (x1, y1) = (x1.max(0.0) as u32, y1.max(0.0) as u32);
        for y in y0..y1.min(self.h) {
            for x in x0..x1.min(self.w) {
                self.blend(x, y, color, alpha);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Text lands inside the buffer, changes pixels where it is drawn, and leaves the rest alone.
    /// This is the test that catches an epaint atlas API change — the only thing here that can
    /// silently start producing nothing.
    #[test]
    fn a_caption_marks_the_bottom_left_and_nothing_else() {
        let (w, h) = (400u32, 200u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        draw(
            &mut rgba,
            w,
            h,
            &Stamp {
                caption: "KTLX · REF · hookecho.io".to_string(),
                ..Default::default()
            },
        );
        let lit = |x0: u32, y0: u32, x1: u32, y1: u32| {
            let mut n = 0;
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * w + x) * 4) as usize;
                    if rgba[i] > 0 || rgba[i + 1] > 0 || rgba[i + 2] > 0 {
                        n += 1;
                    }
                }
            }
            n
        };
        assert!(lit(0, h - 40, w / 2, h) > 50, "no caption was drawn");
        assert_eq!(lit(0, 0, w, h / 2), 0, "something was drawn off-caption");
    }

    /// The broadcast dressing: a clock top right, a crawl band along the bottom with the caption
    /// lifted above it, a logo top left — all inside the safe margin.
    #[test]
    fn broadcast_dressing_stays_inside_the_margin() {
        let (w, h) = (640u32, 360u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        let margin = 18.0;
        draw(
            &mut rgba,
            w,
            h,
            &Stamp {
                caption: "KTLX · REF 0.5° · hookecho.io".into(),
                margin,
                clock: Some(("3:12 PM".into(), "May 20 · CDT".into())),
                crawl: Some("Tornado Warning — Cleveland, OK   •   ".repeat(8)),
                logo: Some(image::RgbaImage::from_pixel(
                    200,
                    100,
                    image::Rgba([255, 0, 0, 255]),
                )),
                ..Default::default()
            },
        );
        let px = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
        };
        // Nothing inside the margin band on any edge.
        for x in 0..w {
            for y in (0..margin as u32).chain(h - margin as u32..h) {
                assert_eq!(px(x, y)[3], 0, "drawn at ({x},{y}) inside the margin");
            }
        }
        for y in 0..h {
            for x in (0..margin as u32).chain(w - margin as u32..w) {
                assert_eq!(px(x, y)[3], 0, "drawn at ({x},{y}) inside the margin");
            }
        }
        // The logo is red, top left, scaled to 9 % of the height.
        assert_eq!(px(20, 20), [255, 0, 0, 255]);
        assert_eq!(px(20, 18 + (360.0f32 * 0.09) as u32 + 2)[3], 0);
        // The crawl's red tag sits at the bottom left, just inside the margin.
        let tag = px(22, h - margin as u32 - 3);
        assert!(tag[0] > 150 && tag[1] < 80, "no crawl tag: {tag:?}");
        // The clock draws something top right.
        let lit = (w - 140..w - margin as u32)
            .flat_map(|x| (margin as u32..60).map(move |y| (x, y)))
            .filter(|&(x, y)| px(x, y)[3] > 0)
            .count();
        assert!(lit > 50, "no clock");
    }

    /// A city whose name would land on the clock, the caption or the crawl is not drawn; one in
    /// open map still is.
    #[test]
    fn city_labels_keep_clear_of_the_frame() {
        let (w, h) = (800u32, 450u32);
        let stamp = Stamp {
            caption: "KTLX · REF 0.5°".into(),
            clock: Some(("3:08 PM CDT".into(), "KTLX · May 20, 2013".into())),
            crawl: Some("Tornado Warning — Cleveland, OK".into()),
            margin: 20.0,
            labels: vec![
                (740.0, 40.0, "Under the clock".into()),
                (400.0, 420.0, "Under the crawl".into()),
                (400.0, 200.0, "Open map".into()),
            ],
            ..Default::default()
        };
        let mut fonts = Fonts::new(TextOptions::default(), crate::fonts::base());
        let taken = reserved(&mut fonts, w as f32, h as f32, &stamp, 0.8);
        let hits = |x: f32, y: f32| {
            taken
                .iter()
                .any(|b| overlaps(b, &[x - 5.0, y - 5.0, x + 5.0, y + 5.0]))
        };
        assert!(hits(740.0, 40.0) && hits(400.0, 420.0));
        assert!(!hits(400.0, 200.0));
    }

    /// Straight-alpha blending: text on a transparent pixel leaves it partly transparent, and an
    /// opaque pixel stays opaque.
    #[test]
    fn blending_keeps_a_transparent_frame_transparent() {
        let mut rgba = vec![0u8; 8];
        rgba[4..8].copy_from_slice(&[0, 0, 255, 255]);
        let mut canvas = Canvas {
            rgba: &mut rgba,
            w: 2,
            h: 1,
        };
        canvas.blend(0, 0, [255, 255, 255], 128);
        canvas.blend(1, 0, [255, 0, 0], 128);
        assert_eq!(&rgba[0..4], &[255, 255, 255, 128]);
        assert_eq!(rgba[7], 255);
        assert!(rgba[4] > 120 && rgba[6] > 120, "{:?}", &rgba[4..8]);
    }

    /// Blending stays in bounds: a glyph hanging off the edge is clipped, not wrapped.
    #[test]
    fn out_of_bounds_pixels_are_dropped() {
        let mut rgba = vec![0u8; 4 * 4 * 4];
        let mut canvas = Canvas {
            rgba: &mut rgba,
            w: 4,
            h: 4,
        };
        canvas.blend(9, 1, [255, 255, 255], 255);
        canvas.blend(1, 9, [255, 255, 255], 255);
        assert!(rgba.iter().all(|&b| b == 0));
    }
}
