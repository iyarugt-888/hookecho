//! The ribbon layout's chrome primitives, in Dear ImGui's manner.
//!
//! The ribbon is a docked strip of labelled control groups, a docked horizontal colour scale
//! directly under it, and a thin status bar along the bottom edge — the opposite trade from the
//! minimal chrome: the map gives back a strip of height in exchange for every control being
//! visible at once. It used to be painted as a navy→black gradient with glossy stadium pills; it
//! now draws what every other surface draws: an ImGui window strip, flat 19px buttons and frames,
//! 1px separators, the active ImGui style's colours (`theme::current`).
//!
//! Everything here is a free function: no `self`, no app state. `app/chrome/ribbon.rs` composes
//! these into the actual ribbon and wires them to `PaletteAction`s.

use crate::colormap::ColorTable;
use crate::theme::{self, FONT, FRAME_H};
use egui::{
    pos2, vec2, Align2, Color32, FontId, Mesh, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui,
};

/// Body text on the ribbon: ImGui's `Text`.
pub fn ink() -> Color32 {
    theme::current().text
}
/// A resting control fill: ImGui's `Button`.
pub fn pill_bg() -> Color32 {
    theme::current().button
}
/// The status bar: ImGui's `MenuBarBg`, the bar along an edge.
pub fn status_bg() -> Color32 {
    theme::current().menu_bar_bg
}
/// Status bar text and colour-scale labels: ImGui's `Text` (ImGui has no secondary grey).
pub fn status_fg() -> Color32 {
    theme::current().text
}

/// Ribbon control-row height.
pub const RIBBON_H: f32 = 130.0;
/// Docked colour scale height: a frame-high bar, its ticks, and a line of labels under them.
pub const COLORBAR_H: f32 = 40.0;
/// Bottom status bar height: one frame.
pub const STATUS_H: f32 = FRAME_H + 3.0;
/// Right-edge keep-out for the window min/max/close buttons in this layout (they anchor 70 px in
/// from the edge and are ~100 px wide).
pub const WINDOW_BTN_KEEPOUT: f32 = 178.0;

/// Paint the ribbon's background across `rect`: an ImGui window strip, its border along the
/// bottom edge. No gradient: ImGui fills are flat.
pub fn ribbon_background(painter: &egui::Painter, rect: Rect) {
    let p = theme::current();
    painter.rect_filled(rect, 0.0, p.window_bg);
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );
}

/// A ribbon button: ImGui's `Button`, held in `ButtonActive` while `selected`. The returned
/// [`Response`] senses clicks. (`accent` is the caller's theme accent, kept for the call sites;
/// the selected fill is the style's own `ButtonActive`.)
pub fn pill(ui: &mut Ui, label: &str, selected: bool, accent: Color32) -> Response {
    pill_sized(ui, label, selected, accent, 0.0)
}

/// [`pill`] with a minimum width (so a row of related buttons lines up): `FramePadding` either
/// side of the label, one frame high, square, no border, no gloss.
pub fn pill_sized(
    ui: &mut Ui,
    label: &str,
    selected: bool,
    _accent: Color32,
    min_w: f32,
) -> Response {
    let p = theme::current();
    let font = FontId::proportional(FONT);
    let text_w = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), p.text)
        .size()
        .x;
    let w = (text_w + 8.0).max(min_w);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, FRAME_H), Sense::click());
    let fill = if selected || resp.is_pointer_button_down_on() {
        p.button_active
    } else if resp.hovered() {
        p.button_hovered
    } else {
        p.button
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, fill);
    painter.text(rect.center(), Align2::CENTER_CENTER, label, font, p.text);
    resp
}

/// The label that titles a ribbon group: plain text at the one size, as ImGui labels are.
pub fn group_label(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(FONT).color(ink()));
    ui.add_space(2.0);
}

/// A full-height 1px separator between two ribbon groups.
pub fn vsep(ui: &mut Ui) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, RIBBON_H - 14.0), Sense::hover());
    let yr = egui::Rangef::new(rect.top() + 6.0, rect.bottom() - 6.0);
    ui.painter().vline(
        rect.center().x,
        yr,
        Stroke::new(1.0, theme::current().separator),
    );
    ui.add_space(4.0);
}

/// ImGui's check mark in a `size` square at `min`: the three-point tick `RenderCheckMark` draws,
/// inset by a sixth of the box.
pub fn check_mark(painter: &egui::Painter, min: egui::Pos2, size: f32, color: Color32) {
    let pad = (size / 6.0).floor().max(1.0);
    let sz = size - pad * 2.0;
    let thickness = (sz / 5.0).max(1.0);
    let third = sz / 3.0;
    let bx = min.x + pad + third;
    let by = min.y + pad + sz - third * 0.5;
    painter.add(Shape::line(
        vec![
            pos2(bx - third, by - third),
            pos2(bx, by),
            pos2(bx + third * 2.0, by - third * 2.0),
        ],
        Stroke::new(thickness, color),
    ));
}

/// ImGui's checkbox for the ribbon's options (Smoothing, Map legend…): a frame-high
/// `FrameBg` box, the tick in `CheckMark` when on, then the label. Returns `true` on the frame
/// it is clicked.
pub fn check(ui: &mut Ui, label: &str, on: &mut bool) -> bool {
    let p = theme::current();
    let font = FontId::proportional(FONT);
    let tw = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), p.text)
        .size()
        .x;
    let (rect, resp) = ui.allocate_exact_size(vec2(FRAME_H + 4.0 + tw, FRAME_H), Sense::click());
    if resp.clicked() {
        *on = !*on;
    }
    let painter = ui.painter();
    let box_r = Rect::from_min_size(rect.min, vec2(FRAME_H, FRAME_H));
    let fill = if resp.is_pointer_button_down_on() {
        p.frame_bg_active
    } else if resp.hovered() {
        p.frame_bg_hovered
    } else {
        p.frame_bg
    };
    painter.rect_filled(box_r, 0.0, fill);
    if *on {
        check_mark(painter, box_r.min, FRAME_H, p.check_mark);
    }
    painter.text(
        pos2(box_r.right() + 4.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        p.text,
    );
    resp.clicked()
}

/// The colour bar's own strip inside the docked scale's `rect`, and the value range it spans
/// (the table's first to last stop), shared by the drawing and by the pointer.
fn colorbar_geometry(rect: Rect, table: &ColorTable) -> Option<(Rect, f32, f32)> {
    let (vmin, vmax) = match (table.stops.first(), table.stops.last()) {
        (Some(a), Some(b)) if b.value > a.value => (a.value, b.value),
        _ => return None,
    };
    // A frame-high bar, as the design system's colour scale is: the data is the one gradient.
    let bar = Rect::from_min_max(
        pos2(rect.left() + 6.0, rect.top() + 2.0),
        pos2(rect.right() - 6.0, rect.top() + 2.0 + FRAME_H),
    );
    Some((bar, vmin, vmax))
}

/// The value (the table's internal units) under screen x on the docked colour scale.
pub fn colorbar_value_at(rect: Rect, table: &ColorTable, x: f32) -> Option<f32> {
    let (bar, vmin, vmax) = colorbar_geometry(rect, table)?;
    let t = ((x - bar.left()) / bar.width().max(1.0)).clamp(0.0, 1.0);
    Some(vmin + t * (vmax - vmin))
}

/// Outline the band `lo..=hi` on the docked colour scale: the range flashing on the map.
pub fn colorbar_band(painter: &egui::Painter, rect: Rect, table: &ColorTable, lo: f32, hi: f32) {
    let Some((bar, vmin, vmax)) = colorbar_geometry(rect, table) else {
        return;
    };
    let span = (vmax - vmin).max(f32::EPSILON);
    let x_of = |v: f32| bar.left() + ((v - vmin) / span).clamp(0.0, 1.0) * bar.width();
    let band = Rect::from_min_max(
        pos2(x_of(lo.min(hi)), bar.top() - 2.0),
        pos2(
            x_of(lo.max(hi)).max(x_of(lo.min(hi)) + 2.0),
            bar.bottom() + 2.0,
        ),
    );
    painter.rect_stroke(
        band,
        0.0,
        Stroke::new(2.0, Color32::WHITE),
        StrokeKind::Outside,
    );
}

/// The docked horizontal colour scale under the ribbon. Mirrors [`crate::ui::legend::draw_vertical`]
/// — same [`ColorTable`], so the bar and the radar LUT never diverge — but laid out left→right with
/// tick labels riding under the bar.
pub fn colorbar(
    painter: &egui::Painter,
    rect: Rect,
    table: &ColorTable,
    disp_factor: f32,
    disp_label: &str,
) {
    painter.rect_filled(rect, 0.0, theme::current().window_bg);
    let Some((bar, vmin, vmax)) = colorbar_geometry(rect, table) else {
        return;
    };
    let span = (vmax - vmin).max(f32::EPSILON);
    let x_of = |v: f32| bar.left() + ((v - vmin) / span).clamp(0.0, 1.0) * bar.width();
    let col = |c: [u8; 4]| Color32::from_rgb(c[0], c[1], c[2]);

    let mut mesh = Mesh::default();
    let mut quad = |x0: f32, x1: f32, c0: Color32, c1: Color32| {
        if x1 <= x0 {
            return;
        }
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(pos2(x0, bar.top()), c0);
        mesh.colored_vertex(pos2(x1, bar.top()), c1);
        mesh.colored_vertex(pos2(x1, bar.bottom()), c1);
        mesh.colored_vertex(pos2(x0, bar.bottom()), c0);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    };
    for (i, s) in table.stops.iter().enumerate() {
        let x0 = x_of(s.value);
        match table.stops.get(i + 1) {
            Some(n) => {
                let x1 = x_of(n.value);
                if s.solid {
                    quad(x0, x1, col(s.rgba), col(s.rgba));
                } else {
                    quad(x0, x1, col(s.rgba), col(s.end.unwrap_or(n.rgba)));
                }
            }
            None => quad(x0 - 1.0, x0 + 1.0, col(s.rgba), col(s.rgba)),
        }
    }
    painter.add(Shape::mesh(mesh));
    painter.rect_stroke(
        bar,
        0.0,
        Stroke::new(1.0, Color32::from_white_alpha(40)),
        StrokeKind::Outside,
    );

    // Ticks: the table's own Step if it declared one, else eight even divisions.
    let step = table
        .step
        .filter(|s| *s > 0.0 && (vmax - vmin) / *s <= 24.0);
    let mut ticks: Vec<f32> = Vec::new();
    match step {
        Some(st) => {
            let mut v = (vmin / st).ceil() * st;
            while v <= vmax + 1e-3 {
                ticks.push(v);
                v += st;
            }
        }
        None => {
            for k in 0..=8 {
                ticks.push(vmin + span * k as f32 / 8.0);
            }
        }
    }
    let font = FontId::monospace(FONT);
    let mut last_x = f32::NEG_INFINITY;
    for v in ticks {
        let x = x_of(v);
        if x - last_x < 34.0 {
            continue;
        }
        last_x = x;
        painter.vline(
            x,
            egui::Rangef::new(bar.bottom(), bar.bottom() + 4.0),
            Stroke::new(1.0, status_fg()),
        );
        painter.text(
            pos2(x, bar.bottom() + 5.0),
            Align2::CENTER_TOP,
            format!("{:.0}", v * disp_factor),
            font.clone(),
            status_fg(),
        );
    }
    if !disp_label.is_empty() {
        painter.text(
            pos2(rect.left() + 4.0, bar.center().y),
            Align2::LEFT_CENTER,
            disp_label.trim(),
            FontId::proportional(FONT),
            Color32::from_white_alpha(150),
        );
    }
}
