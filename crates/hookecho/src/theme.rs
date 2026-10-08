//! The app's one look: the Dear ImGui style the workstation dock was designed in — a tuned dark
//! palette around ImGui's blue accent, ImGui's square, compact widget geometry, and a subtle window
//! border and shadow.
//!
//! There used to be eight colour schemes here (Dark, Light, Synthwave, Aurora, High contrast,
//! OLED black…). They were retired so every surface draws the same way; `settings::Theme` keeps
//! their names as aliases of this one.
//!
//! Applied every frame from [`crate::app`] (a cheap style clone). The workstation's own painters
//! (`ui::workstation`) draw their navy panels around the same accent.

use crate::settings::Theme;
use egui::{vec2, Color32, CornerRadius, Margin, Stroke, Style, Visuals};

/// The accent where no theme is in scope: Dear ImGui's `CheckMark` blue. Live UI reads [`accent`]
/// (which folds in the user's own pick) or `ui.visuals().hyperlink_color`.
pub const ACCENT: Color32 = Color32::from_rgb(0x42, 0x96, 0xfa); // #4296fa

fn c(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// The palette's roles.
#[derive(Clone, Copy)]
struct Palette {
    /// Panel + window fill.
    bg: Color32,
    /// Deepest surface (plots, extreme_bg).
    extreme: Color32,
    /// Faint raised surface (stat cards).
    faint: Color32,
    /// Window / separator stroke.
    stroke: Color32,
    /// Inactive widget fill.
    widget: Color32,
    /// Hovered widget fill.
    widget_hover: Color32,
    /// Body text.
    text: Color32,
    /// Primary accent (selection, active widgets, links, map markers).
    accent: Color32,
}

/// Dear ImGui's own StyleColorsDark(), within this app's single-accent abstraction. `widget` and
/// `widget_hover` are ImGui's FrameBg/FrameBgHovered (its translucent navy-blue accent washes,
/// `(0.16,0.29,0.48,0.54)` and `(0.26,0.59,0.98,0.40)`) alpha-composited over ImGui's own WindowBg
/// `(0.06,0.06,0.06)` — the exact math a real ImGui frame does, baked to opaque colors since this
/// palette has no separate alpha channel per role. `accent` is ImGui's literal
/// CheckMark/Header/SliderGrabActive blue, `(0.26,0.59,0.98)`.
fn palette() -> Palette {
    Palette {
        bg: c(0x0f0f0f),
        extreme: c(0x000000),
        faint: c(0x1a1a1a),
        stroke: c(0x3a3d46),
        widget: c(0x1d2f49),
        widget_hover: c(0x24456d),
        text: c(0xffffff),
        accent: accent_override().unwrap_or(ACCENT),
    }
}

/// The accent the app marks things with (map markers, the active pane's outline, selection):
/// ImGui's blue, or the user's own colour.
pub fn accent(_theme: Theme) -> Color32 {
    accent_override().unwrap_or(ACCENT)
}

/// User accent override, packed as `0xFF_RR_GG_BB` (0 = none).
///
/// ponytail: a process-global instead of threading `&Settings` through 19 `accent()` call sites —
/// the accent is one app-wide value and `apply()` is the only writer. If accent ever becomes
/// per-window, pass it explicitly instead.
static ACCENT_OVERRIDE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn set_accent_override(rgb: Option<[u8; 3]>) {
    let packed = rgb.map_or(0, |[r, g, b]| {
        0xFF00_0000 | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
    });
    ACCENT_OVERRIDE.store(packed, std::sync::atomic::Ordering::Relaxed);
}

fn accent_override() -> Option<Color32> {
    let p = ACCENT_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed);
    (p != 0).then(|| Color32::from_rgb((p >> 16) as u8, (p >> 8) as u8, p as u8))
}

/// Frame/window geometry: Dear ImGui's default style rounds nothing (`FrameRounding`/
/// `WindowRounding` both `0.0`) and uses its own tight, fixed `FramePadding`/`ItemSpacing`/
/// `WindowPadding` — square corners and compact proportions are as much a part of its identity as
/// the blue accent. A plain value so the numbers are unit-tested without a live `egui::Context`.
struct Geometry {
    corner_radius: u8,
    window_corner_radius: u8,
    item_spacing: egui::Vec2,
    button_padding: egui::Vec2,
    interact_h: f32,
    window_margin: i8,
    menu_margin: i8,
}

const GEOMETRY: Geometry = Geometry {
    corner_radius: 0,
    window_corner_radius: 0,
    item_spacing: vec2(8.0, 4.0),   // ImGuiStyle::ItemSpacing
    button_padding: vec2(4.0, 3.0), // ImGuiStyle::FramePadding
    interact_h: 19.0,               // ~FramePadding.y*2 + a 13px line
    window_margin: 8,               // ImGuiStyle::WindowPadding
    menu_margin: 8,
};

pub fn apply(ctx: &egui::Context, accent_rgb: Option<[u8; 3]>) {
    set_accent_override(accent_rgb);
    let pal = palette();
    let mut visuals = Visuals::dark();
    tune(&mut visuals, &pal);

    let mut style = Style {
        visuals,
        ..Default::default()
    };

    let g = &GEOMETRY;
    style.spacing.item_spacing = g.item_spacing;
    style.spacing.button_padding = g.button_padding;
    style.spacing.interact_size.y = g.interact_h;
    style.spacing.window_margin = Margin::same(g.window_margin);
    style.spacing.menu_margin = Margin::same(g.menu_margin);

    // Rounding on every widget state.
    let r = CornerRadius::same(g.corner_radius);
    for w in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        w.corner_radius = r;
    }
    let window_r = CornerRadius::same(g.window_corner_radius);
    style.visuals.window_corner_radius = window_r;
    style.visuals.menu_corner_radius = window_r;

    // Type scale (egui's default face; sizes only): the compact desktop sizes, or the M3 touch
    // scale on a phone.
    use egui::{FontFamily::Proportional, FontId, TextStyle};
    let (heading, body, button, small, mono) = if cfg!(target_os = "android") {
        (
            crate::ui::m3::T_TITLE,
            crate::ui::m3::T_BODY,
            crate::ui::m3::T_LABEL_LG,
            crate::ui::m3::T_LABEL_SM,
            crate::ui::m3::T_LABEL,
        )
    } else {
        (14.0, 12.5, 12.5, 11.0, 12.0)
    };
    style.text_styles = [
        (TextStyle::Heading, FontId::new(heading, Proportional)),
        (TextStyle::Body, FontId::new(body, Proportional)),
        (TextStyle::Button, FontId::new(button, Proportional)),
        (TextStyle::Small, FontId::new(small, Proportional)),
        (
            TextStyle::Monospace,
            FontId::new(mono, egui::FontFamily::Monospace),
        ),
    ]
    .into();

    ctx.set_style_of(egui::Theme::Dark, style);
    ctx.options_mut(|o| o.theme_preference = egui::ThemePreference::Dark);
}

/// An egui slider drawn the way the workstation's own fader is: the track filled with the accent up
/// to the value, and a solid accent grab. egui paints the grab in the widget's `bg_fill` — the
/// same colour as the rail it rides on — ringed by `fg_stroke`; scoped to this one slider, a
/// stroke as thick as the grab is narrow fills it in. Scoped because that stroke also draws every
/// checkbox's tick.
pub fn slider(ui: &mut egui::Ui, slider: egui::Slider<'_>) -> egui::Response {
    let accent = ui.visuals().hyperlink_color;
    let width = ui.spacing().interact_size.y * 0.5;
    ui.scope(|ui| {
        let v = ui.visuals_mut();
        v.slider_trailing_fill = true;
        v.selection.bg_fill = accent.gamma_multiply(0.45);
        v.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.6 };
        for (state, grab) in [
            (&mut v.widgets.inactive, accent.gamma_multiply(0.85)),
            (&mut v.widgets.hovered, accent),
            (&mut v.widgets.active, accent),
        ] {
            state.fg_stroke = Stroke::new(width, grab);
        }
        ui.add(slider)
    })
    .inner
}

/// Does `theme` request the high-contrast information layers (thicker strokes, high-contrast
/// colour tables)?
///
/// ponytail: none does since the High contrast scheme was retired with the others. The hook stays
/// (with [`overlay_stroke_scale`], [`vector_stroke_scale`], [`warning_alpha_for`] and
/// [`high_contrast_alt_name`]) so the map's information layers can respond again if an
/// accessibility scheme returns; the `-HC` colour tables are still pickable by hand meanwhile.
pub fn is_high_contrast(_theme: Theme) -> bool {
    false
}

/// Stroke-width multiplier for geographic overlay polygons (warnings, outlooks, etc.).
/// High contrast roughly doubles the outline so it stays legible in direct sunlight and for
/// low-vision users — the chrome uses `palette` instead, this is purely for map geometry.
pub fn overlay_stroke_scale(theme: Theme) -> f32 {
    if is_high_contrast(theme) {
        2.2
    } else {
        1.0
    }
}

/// Stroke-width multiplier for vector basemap strokes (roads, boundaries, waterways).
pub fn vector_stroke_scale(theme: Theme) -> f32 {
    if is_high_contrast(theme) {
        1.8
    } else {
        1.0
    }
}

/// Boost fill/stroke alphas for warning polygons under high contrast so the polygon remains
/// legible against both dark satellite imagery and bright radar. Returns `(fill_alpha, stroke_alpha)` 0..255.
pub fn warning_alpha_for(theme: Theme) -> (u8, u8) {
    if is_high_contrast(theme) {
        (90, 255)
    } else {
        (45, 235)
    }
}

/// Which built-in colormap alternate to prefer when `theme` is high contrast, if the user
/// hasn't chosen a custom `.pal`. `None` means keep the default. The alternates live in
/// `colormap.rs` (`REF-HC.pal`, `VEL-HC.pal`) and are exposed via `colormap::high_contrast_alt_name`.
pub fn high_contrast_alt_name(moment: wxdata::level2::Moment) -> Option<&'static str> {
    if moment == wxdata::level2::Moment::Reflectivity {
        Some("High contrast (reflectivity)")
    } else if moment == wxdata::level2::Moment::Velocity {
        Some("High contrast (velocity)")
    } else {
        None
    }
}

/// Backgrounds/strokes/text come from the palette; the accent drives active widgets, selection,
/// links, and a subtle glow on hover + window edges.
fn tune(v: &mut Visuals, p: &Palette) {
    let a = p.accent;
    v.panel_fill = p.bg;
    v.window_fill = p.bg;
    v.extreme_bg_color = p.extreme;
    v.faint_bg_color = p.faint;
    v.window_stroke = Stroke::new(1.0, p.stroke);
    v.window_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };
    v.popup_shadow = v.window_shadow;

    v.widgets.noninteractive.bg_fill = p.bg;
    v.widgets.noninteractive.weak_bg_fill = p.bg;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text.gamma_multiply(0.85));
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.stroke.gamma_multiply(0.8));

    v.widgets.inactive.bg_fill = p.widget;
    v.widgets.inactive.weak_bg_fill = p.widget;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, p.stroke);

    // Hover flashes the accent (the dopamine): a wash of accent over the hover fill.
    v.widgets.hovered.bg_fill = p.widget_hover;
    v.widgets.hovered.weak_bg_fill = p.widget_hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, a.gamma_multiply(0.55));

    v.widgets.active.bg_fill = a.gamma_multiply(0.85);
    v.widgets.active.weak_bg_fill = a.gamma_multiply(0.85);
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.active.bg_stroke = Stroke::new(1.0, a);

    v.selection.bg_fill = a.gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0, a);
    v.hyperlink_color = a;
    v.override_text_color = Some(p.text);
}

/// A compact stat card: a faint rounded panel with a small weak label over a strong value.
/// Sized to a fixed width so several tile neatly in a `horizontal_wrapped` row.
pub fn stat_card(ui: &mut egui::Ui, label: &str, value: &str) {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(108.0);
            ui.vertical(|ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(label.to_uppercase()).size(9.5).weak())
                        .truncate(),
                );
                ui.label(egui::RichText::new(value).size(15.0).strong());
            });
        });
}

/// A min-max normalized sparkline of `vals` (oldest→newest) in a fixed-height row.
pub fn sparkline(ui: &mut egui::Ui, vals: &[f32], color: Color32) {
    let size = egui::vec2(ui.available_width().min(300.0), 34.0);
    sparkline_sized(ui, vals, color, size);
}

/// [`sparkline`] at a caller-chosen size, for places with a row height to fit inside — a table
/// cell has ~20 px, where the default 34 would overlap its neighbours.
///
/// Returns the response so a caller can hang a tooltip on it.
pub fn sparkline_sized(
    ui: &mut egui::Ui,
    vals: &[f32],
    color: Color32,
    size: egui::Vec2,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    if vals.len() < 2 {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "no data",
            egui::FontId::proportional(10.0),
            ui.visuals().weak_text_color(),
        );
        return response;
    }
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for &v in vals {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    let span = (hi - lo).max(1e-3);
    let pad = 4.0;
    let pts: Vec<egui::Pos2> = vals
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x =
                rect.left() + pad + (rect.width() - 2.0 * pad) * i as f32 / (vals.len() - 1) as f32;
            let y = rect.bottom() - pad - (rect.height() - 2.0 * pad) * (v - lo) / span;
            egui::pos2(x, y)
        })
        .collect();
    painter.add(egui::Shape::line(pts, Stroke::new(1.5, color)));
    let weak = ui.visuals().weak_text_color();
    painter.text(
        rect.right_top() + egui::vec2(-3.0, 1.0),
        egui::Align2::RIGHT_TOP,
        format!("{hi:.0}"),
        egui::FontId::proportional(9.0),
        weak,
    );
    painter.text(
        rect.right_bottom() + egui::vec2(-3.0, -1.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("{lo:.0}"),
        egui::FontId::proportional(9.0),
        weak,
    );
    response
}

/// A collapsible, accent-labelled section with consistent inner spacing.
pub fn section<R>(
    ui: &mut egui::Ui,
    title: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    let heading = egui::RichText::new(title.to_uppercase())
        .color(ui.visuals().hyperlink_color)
        .size(11.5)
        .strong();
    egui::CollapsingHeader::new(heading)
        .default_open(true)
        .show_unindented(ui, |ui| {
            ui.add_space(2.0);
            let r = add(ui);
            ui.add_space(4.0);
            r
        })
        .body_returned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_geometry_is_dear_imgui_s_square_and_compact() {
        assert_eq!(GEOMETRY.corner_radius, 0, "Dear ImGui rounds nothing");
        assert_eq!(GEOMETRY.window_corner_radius, 0);
        assert_eq!(GEOMETRY.button_padding, vec2(4.0, 3.0), "ImGuiStyle::FramePadding");
        assert_eq!(GEOMETRY.item_spacing, vec2(8.0, 4.0), "ImGuiStyle::ItemSpacing");
    }

    #[test]
    fn the_accent_is_the_exact_imgui_blue_until_the_user_picks_one() {
        // ImGuiCol_CheckMark / Header / SliderGrabActive in StyleColorsDark(): (0.26, 0.59, 0.98).
        set_accent_override(None);
        assert_eq!(accent(Theme::DearImGui), Color32::from_rgb(0x42, 0x96, 0xfa));
        set_accent_override(Some([255, 0, 128]));
        assert_eq!(accent(Theme::DearImGui), Color32::from_rgb(255, 0, 128));
        set_accent_override(None);
    }

    #[test]
    fn the_window_fill_is_imgui_s_near_black() {
        let bg = palette().bg;
        // ImGui's own WindowBg, (0.06, 0.06, 0.06) — dark enough that no channel clears 0x20.
        assert!(bg.r() < 0x20 && bg.g() < 0x20 && bg.b() < 0x20, "{bg:?}");
    }
}
