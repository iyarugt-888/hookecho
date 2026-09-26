//! The dock's design system ("Analyst Workstation", `docs/WSV3_IMGUI_MODERN_DESIGN_PLAN.md`):
//! one [`Tokens`] value for every colour and size, and small stateless painters that the dock's
//! panels compose. Dock code never sets a colour inline; it asks for a component.
//!
//! Dark only, like the dock before it, but the accent is the theme's (with the user's accent
//! override folded in by `theme::accent`), so the one "your colour" setting still applies.

use egui::{Color32, CornerRadius, FontId, Frame, Margin, Rect, Response, Sense, Stroke, Vec2};

/// App bar height.
pub const APP_BAR_H: f32 = 40.0;
/// Context toolbar height.
pub const TOOLBAR_H: f32 = 36.0;
/// Standard control height (buttons, combos, fields).
pub const CONTROL_H: f32 = 24.0;
/// Tool-rail button edge.
pub const RAIL_BTN: f32 = 36.0;
/// Panel header height.
pub const HEADER_H: f32 = 28.0;

/// Every colour the workstation look uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    pub bg: Color32,
    pub panel: Color32,
    pub panel_hi: Color32,
    pub field: Color32,
    pub field_hi: Color32,
    pub line: Color32,
    pub line_soft: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub live: Color32,
    pub warn: Color32,
    pub danger: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Tokens {
    /// The workstation palette around `accent`.
    pub fn new(accent: Color32) -> Tokens {
        Tokens {
            bg: rgb(0x0B111A),
            panel: rgb(0x0F1722),
            panel_hi: rgb(0x141E2C),
            field: rgb(0x18222F),
            field_hi: rgb(0x1F2B3B),
            line: rgb(0x233147),
            line_soft: rgb(0x1A2536),
            text: rgb(0xD8DFEA),
            text_dim: rgb(0x8A97AB),
            text_faint: rgb(0x5D6A7E),
            accent,
            live: rgb(0x3DD68C),
            warn: rgb(0xF2B84B),
            danger: rgb(0xF25555),
        }
    }

    /// The accent as a translucent wash, for a selected row or an "on" button's fill.
    pub fn accent_soft(&self) -> Color32 {
        let [r, g, b, _] = self.accent.to_array();
        Color32::from_rgba_unmultiplied(r, g, b, 46)
    }
}

/// A docked panel's body.
pub fn panel_frame(t: &Tokens) -> Frame {
    Frame::NONE
        .fill(t.panel)
        .stroke(Stroke::new(1.0, t.line))
        .inner_margin(Margin::same(0))
}

/// A floating card over the map: the panel look, rounded, with a shadow to lift it off the data.
pub fn card_frame(t: &Tokens) -> Frame {
    Frame::NONE
        .fill(t.panel)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(CornerRadius::same(4))
        .shadow(egui::epaint::Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(140),
        })
}

/// Stock egui widgets in the workstation look, for the controls the dock embeds rather than
/// paints (text fields, combo boxes and their menus, sliders, the shared model and layer-option
/// panels).
pub fn style_scope(ui: &mut egui::Ui, t: &Tokens) {
    let style = ui.style_mut();
    style.override_text_style = None;
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(13.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(12.5));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(11.0));
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(8.0, 3.0);
    style.spacing.interact_size.y = CONTROL_H;
    let v = &mut style.visuals;
    v.override_text_color = Some(t.text);
    v.extreme_bg_color = t.field;
    v.faint_bg_color = t.panel_hi;
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.window_stroke = Stroke::new(1.0, t.line);
    v.selection.bg_fill = t.accent_soft();
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.hyperlink_color = t.accent;
    let r = CornerRadius::same(4);
    for (w, fill, stroke) in [
        (&mut v.widgets.noninteractive, t.panel, t.line_soft),
        (&mut v.widgets.inactive, t.field, t.line),
        (
            &mut v.widgets.hovered,
            t.field_hi,
            t.accent.gamma_multiply(0.6),
        ),
        (
            &mut v.widgets.active,
            t.accent.gamma_multiply(0.85),
            t.accent,
        ),
        (&mut v.widgets.open, t.field_hi, t.accent),
    ] {
        w.corner_radius = r;
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, stroke);
    }
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text_dim);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
}

/// Monospace text, for the things read as data rather than as words: values, times,
/// coordinates, VCP and frame counts, so digits line up and one reading is told from the next.
pub fn mono(s: impl Into<String>, size: f32, color: Color32) -> egui::RichText {
    egui::RichText::new(s.into())
        .monospace()
        .size(size)
        .color(color)
}

/// Text in the workstation's proportional face.
pub fn text(s: impl Into<String>, size: f32, color: Color32) -> egui::RichText {
    egui::RichText::new(s.into()).size(size).color(color)
}

/// What a tool window's header asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderAction {
    None,
    Close,
    /// Fold a floating window to its title bar, or unfold it.
    Collapse,
    /// Move the window: dock it to a side or float it over the map.
    Place(crate::workspace::Place),
}

/// A tool window's header: glyph and title, then (right-aligned) a placement menu, a collapse
/// button while floating, and close. `place` is `None` for a window that cannot move;
/// `collapsed` is `Some` only while the window floats. Double-clicking a floating window's
/// header folds it too, as in Dear ImGui.
pub fn window_header(
    ui: &mut egui::Ui,
    t: &Tokens,
    glyph: &str,
    title: &str,
    place: Option<crate::workspace::Place>,
    collapsed: Option<bool>,
) -> HeaderAction {
    use crate::workspace::Place;
    let (rect, bar) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEADER_H), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, t.panel_hi);
    p.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, t.line),
    );
    p.text(
        rect.left_center() + egui::vec2(10.0, 0.0),
        egui::Align2::LEFT_CENTER,
        glyph,
        FontId::proportional(14.0),
        t.text_dim,
    );
    p.text(
        rect.left_center() + egui::vec2(30.0, 0.0),
        egui::Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.0),
        t.text,
    );
    let mut action = HeaderAction::None;
    if collapsed.is_some() && bar.double_clicked() {
        action = HeaderAction::Collapse;
    }
    let mut x = rect.right() - 16.0;
    let mut button = |ui: &mut egui::Ui, glyph: &str, hint: &str| -> Response {
        let r = Rect::from_center_size(egui::pos2(x, rect.center().y), egui::vec2(22.0, 22.0));
        x -= 24.0;
        // Keyed on the hint, not the title: a title that carries a count ("Alerts (8)") would
        // otherwise give the same button a new id whenever the count changes.
        let resp = ui.interact(r, ui.id().with(("window_header", hint)), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 4.0, t.field_hi);
        }
        ui.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            FontId::proportional(13.0),
            if resp.hovered() { t.text } else { t.text_dim },
        );
        let enabled = resp.enabled();
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, hint));
        resp.on_hover_text(hint)
    };
    if button(ui, egui_phosphor::regular::X, "Close").clicked() {
        action = HeaderAction::Close;
    }
    if let Some(folded) = collapsed {
        let (glyph, hint) = if folded {
            (egui_phosphor::regular::CARET_DOWN, "Unfold")
        } else {
            (egui_phosphor::regular::CARET_UP, "Fold to the title bar")
        };
        if button(ui, glyph, hint).clicked() {
            action = HeaderAction::Collapse;
        }
    }
    if let Some(now) = place {
        let menu = button(ui, egui_phosphor::regular::DOTS_THREE, "Move this window");
        egui::Popup::menu(&menu).show(|ui| {
            for (p, label) in [
                (Place::Left, "Dock left"),
                (Place::Right, "Dock right"),
                (Place::Float, "Float over the map"),
            ] {
                if ui.selectable_label(now == p, label).clicked() {
                    action = HeaderAction::Place(p);
                }
            }
        });
    }
    action
}

/// An app-bar tab: plain text, with an accent underline when selected.
pub fn tab(ui: &mut egui::Ui, t: &Tokens, label: &str, selected: bool, height: f32) -> Response {
    let font = FontId::proportional(13.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), t.text);
    let size = egui::vec2(galley.size().x + 24.0, height);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let color = if selected {
        Color32::WHITE
    } else if resp.hovered() {
        t.text
    } else {
        t.text_dim
    };
    if resp.hovered() && !selected {
        ui.painter()
            .rect_filled(rect.shrink2(egui::vec2(2.0, 6.0)), 4.0, t.field);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        color,
    );
    if selected {
        let bar = Rect::from_min_max(
            egui::pos2(rect.left() + 8.0, rect.bottom() - 2.5),
            egui::pos2(rect.right() - 8.0, rect.bottom()),
        );
        ui.painter().rect_filled(bar, 1.0, t.accent);
    }
    resp
}

/// A glyph-and-label button (app bar, panel footers): an "on" one takes the accent.
pub fn icon_button(ui: &mut egui::Ui, t: &Tokens, glyph: &str, label: &str, on: bool) -> Response {
    let font = FontId::proportional(12.5);
    let text = if label.is_empty() {
        glyph.to_string()
    } else {
        format!("{glyph}  {label}")
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text.clone(), font.clone(), t.text);
    let size = egui::vec2(galley.size().x + 18.0, CONTROL_H + 4.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if on {
        t.accent_soft()
    } else if resp.hovered() {
        t.field_hi
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 4.0, fill);
    let color = if on {
        t.accent
    } else if resp.hovered() {
        Color32::WHITE
    } else {
        t.text
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        font,
        color,
    );
    resp
}

/// A plain bordered button in the field style (panel footers, card actions).
pub fn button(ui: &mut egui::Ui, t: &Tokens, label: &str, width: f32) -> Response {
    let font = FontId::proportional(12.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), t.text);
    let size = egui::vec2(width.max(galley.size().x + 20.0), CONTROL_H + 4.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    ui.painter().rect(
        rect,
        4.0,
        if resp.hovered() { t.field_hi } else { t.field },
        Stroke::new(
            1.0,
            if resp.hovered() {
                t.accent.gamma_multiply(0.6)
            } else {
                t.line
            },
        ),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        t.text,
    );
    resp
}

/// A square tool-rail button: the glyph alone, the armed tool filled with the accent.
pub fn rail_button(ui: &mut egui::Ui, t: &Tokens, glyph: &str, on: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(RAIL_BTN, RAIL_BTN), Sense::click());
    let fill = if on {
        t.accent
    } else if resp.hovered() {
        t.field_hi
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 4.0, fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(18.0),
        if on { Color32::WHITE } else { t.text },
    );
    resp
}

/// Joined segment buttons; returns the segment clicked, if any. The selected one is filled with
/// the accent.
pub fn segmented(ui: &mut egui::Ui, t: &Tokens, labels: &[&str], selected: usize) -> Option<usize> {
    let font = FontId::proportional(12.5);
    let widths: Vec<f32> = labels
        .iter()
        .map(|l| {
            ui.painter()
                .layout_no_wrap(l.to_string(), font.clone(), t.text)
                .size()
                .x
                + 22.0
        })
        .collect();
    let total: f32 = widths.iter().sum();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total, CONTROL_H), Sense::hover());
    ui.painter().rect(
        rect,
        4.0,
        t.field,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
    let mut clicked = None;
    let mut x = rect.left();
    for (i, (label, w)) in labels.iter().zip(&widths).enumerate() {
        let r = Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(*w, rect.height()));
        x += w;
        let resp = ui.interact(r, ui.id().with(("seg", i, *label)), Sense::click());
        let on = i == selected;
        if on {
            ui.painter().rect_filled(r.shrink(1.0), 3.0, t.accent);
        } else if resp.hovered() {
            ui.painter().rect_filled(r.shrink(1.0), 3.0, t.field_hi);
        }
        if i > 0 && !on && i != selected + 1 {
            ui.painter().line_segment(
                [
                    r.left_top() + egui::vec2(0.0, 5.0),
                    r.left_bottom() - egui::vec2(0.0, 5.0),
                ],
                Stroke::new(1.0, t.line),
            );
        }
        ui.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            font.clone(),
            if on { Color32::WHITE } else { t.text },
        );
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

/// A compact checkbox: an accent-filled box with a tick when on, then the label.
pub fn check(ui: &mut egui::Ui, t: &Tokens, on: &mut bool, label: &str) -> Response {
    let font = FontId::proportional(12.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), t.text);
    let size = egui::vec2(galley.size().x + 24.0, CONTROL_H);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let bx = Rect::from_center_size(
        egui::pos2(rect.left() + 7.0, rect.center().y),
        egui::vec2(14.0, 14.0),
    );
    if *on {
        ui.painter().rect_filled(bx, 3.0, t.accent);
        ui.painter().text(
            bx.center(),
            egui::Align2::CENTER_CENTER,
            egui_phosphor::regular::CHECK,
            FontId::proportional(11.0),
            Color32::WHITE,
        );
    } else {
        ui.painter().rect(
            bx,
            3.0,
            t.field,
            Stroke::new(1.0, if resp.hovered() { t.accent } else { t.line }),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().text(
        egui::pos2(bx.right() + 7.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        if resp.hovered() {
            Color32::WHITE
        } else {
            t.text
        },
    );
    resp
}

/// A flat fraction slider for a dense row: a 3 px track filled with the accent up to `value`, and
/// a small thumb. Drag or click to set it; with focus, the arrow keys step by 5 %. `value` stays
/// within `min..=1`, and the response is marked changed only when it moved.
pub fn fader(
    ui: &mut egui::Ui,
    t: &Tokens,
    value: &mut f32,
    min: f32,
    width: f32,
    label: &str,
) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(width, 18.0), Sense::click_and_drag());
    let track = rect.shrink2(egui::vec2(5.0, 0.0));
    let before = *value;
    if let Some(p) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() {
            *value = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
        }
    }
    if resp.has_focus() {
        let step = ui.input(|i| {
            i.num_presses(egui::Key::ArrowRight) as f32 + i.num_presses(egui::Key::ArrowUp) as f32
                - i.num_presses(egui::Key::ArrowLeft) as f32
                - i.num_presses(egui::Key::ArrowDown) as f32
        });
        *value += step * 0.05;
    }
    *value = value.clamp(min, 1.0);
    if (*value - before).abs() > f32::EPSILON {
        resp.mark_changed();
    }
    let v = *value;
    resp.widget_info(|| egui::WidgetInfo::slider(true, f64::from(v), label));
    let y = rect.center().y;
    let p = ui.painter();
    let bar =
        |x0: f32, x1: f32| Rect::from_min_max(egui::pos2(x0, y - 1.5), egui::pos2(x1, y + 1.5));
    p.rect_filled(bar(track.left(), track.right()), 1.5, t.field_hi);
    let at = track.left() + track.width() * v;
    p.rect_filled(bar(track.left(), at), 1.5, t.accent);
    let hot = resp.hovered() || resp.dragged() || resp.has_focus();
    p.circle(
        egui::pos2(at, y),
        if hot { 5.5 } else { 4.5 },
        if hot { Color32::WHITE } else { t.text },
        Stroke::new(1.0, t.accent),
    );
    resp
}

/// A small caption before a control ("Site:").
pub fn caption(ui: &mut egui::Ui, t: &Tokens, label: &str) {
    ui.label(text(label, 12.0, t.text_dim));
}

/// An inspector row: the key dim in a fixed column, the value beside it in monospace.
pub fn kv(ui: &mut egui::Ui, t: &Tokens, key: &str, value: &str, color: Option<Color32>) {
    kv_row(ui, t, key, mono(value, 12.0, color.unwrap_or(t.text)));
}

/// An inspector row whose value is words rather than data (a place name), in the text face.
pub fn kv_text(ui: &mut egui::Ui, t: &Tokens, key: &str, value: &str) {
    kv_row(ui, t, key, text(value, 12.5, t.text));
}

fn kv_row(ui: &mut egui::Ui, t: &Tokens, key: &str, value: egui::RichText) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(92.0, 18.0), Sense::hover());
        ui.painter().text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            key,
            FontId::proportional(12.0),
            t.text_dim,
        );
        // A long value (a VCP's full name, a volume file) is cut with an ellipsis and shown whole
        // on hover, rather than widening the card or the docked column it sits in.
        ui.add(egui::Label::new(value).truncate());
    });
}

/// A section's caption inside a card or panel, with a hairline running out to the right edge.
pub fn section_rule(ui: &mut egui::Ui, t: &Tokens, label: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), Sense::hover());
    let galley = ui.painter().layout_no_wrap(
        label.to_uppercase(),
        FontId::proportional(10.5),
        t.text_faint,
    );
    let x = rect.left() + galley.size().x + 8.0;
    ui.painter().galley(
        egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        t.text_faint,
    );
    if x < rect.right() {
        ui.painter().line_segment(
            [
                egui::pos2(x, rect.center().y),
                egui::pos2(rect.right(), rect.center().y),
            ],
            Stroke::new(1.0, t.line_soft),
        );
    }
}

/// A small pill: a dot and a word ("● Live").
pub fn badge(ui: &mut egui::Ui, t: &Tokens, label: &str, color: Color32) -> Response {
    let font = FontId::proportional(11.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), t.text);
    let size = egui::vec2(galley.size().x + 22.0, 18.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let [r, g, b, _] = color.to_array();
    ui.painter()
        .rect_filled(rect, 9.0, Color32::from_rgba_unmultiplied(r, g, b, 30));
    ui.painter()
        .circle_filled(egui::pos2(rect.left() + 9.0, rect.center().y), 3.5, color);
    ui.painter().text(
        egui::pos2(rect.left() + 16.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        color,
    );
    resp
}

/// A status dot of radius `r`, vertically centred in a control-height slot.
pub fn status_dot(ui: &mut egui::Ui, color: Color32, r: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(r * 2.0 + 4.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), r, color);
}

/// A vertical hairline between control groups in a horizontal row.
pub fn divider(ui: &mut egui::Ui, t: &Tokens, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, height), Sense::hover());
    ui.painter().line_segment(
        [
            egui::pos2(rect.center().x, rect.top() + 4.0),
            egui::pos2(rect.center().x, rect.bottom() - 4.0),
        ],
        Stroke::new(1.0, t.line),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `f` for three frames of a 900 x 600 screen and return every string drawn in the last.
    fn texts(f: impl Fn(&mut egui::Ui)) -> Vec<String> {
        let ctx = egui::Context::default();
        let mut out = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 600.0),
                )),
                ..Default::default()
            };
            let o = ctx.run_ui(input, |ui| f(ui));
            out = o
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
                    _ => None,
                })
                .collect();
        }
        out
    }

    fn t() -> Tokens {
        Tokens::new(rgb(0x2F81F7))
    }

    #[test]
    fn components_draw_their_labels() {
        let got = texts(|ui| {
            let t = t();
            window_header(
                ui,
                &t,
                egui_phosphor::regular::STACK,
                "Layers",
                Some(crate::workspace::Place::Left),
                Some(false),
            );
            tab(ui, &t, "Radar", true, APP_BAR_H);
            icon_button(ui, &t, egui_phosphor::regular::GEAR, "Settings", false);
            segmented(ui, &t, &["2D", "3D", "Volume"], 0);
            let mut on = true;
            check(ui, &t, &mut on, "Warnings");
            kv(ui, &t, "Site:", "KTLX", None);
            kv_text(ui, &t, "City:", "Oklahoma City");
            badge(ui, &t, "Live", t.live);
        });
        for want in [
            "Layers", "Radar", "Settings", "2D", "3D", "Volume", "Warnings", "Site:", "KTLX",
            "Live",
        ] {
            assert!(
                got.iter().any(|s| s.contains(want)),
                "{want} missing: {got:?}"
            );
        }
    }

    #[test]
    fn a_fader_never_leaves_its_range() {
        let ctx = egui::Context::default();
        let mut v = 1.4_f32;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            fader(ui, &t(), &mut v, 0.05, 90.0, "Opacity");
        });
        assert_eq!(v, 1.0);
        v = -3.0;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            fader(ui, &t(), &mut v, 0.05, 90.0, "Opacity");
        });
        assert_eq!(
            v, 0.05,
            "an invisible layer is a removed one, not a faded one"
        );
    }

    #[test]
    fn the_accent_wash_is_the_accent_translucent() {
        let t = t();
        let [r, g, b, a] = t.accent_soft().to_array();
        assert!(a > 0 && a < 255, "{a}");
        // Premultiplied, so each channel is at most the accent's own.
        let [ar, ag, ab, _] = t.accent.to_array();
        assert!(r <= ar && g <= ag && b <= ab);
        assert_ne!(
            t.panel, t.panel_hi,
            "a header must stand apart from its body"
        );
    }
}
