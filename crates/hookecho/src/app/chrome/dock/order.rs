//! The Layers panel's paint-order list (design plan §3.1 "drag reorder"): in the Active filter,
//! the field layers that are on, top-painted first, with a handle to drag each one above or below
//! another. A line marks the radar between the two bands; a layer stays in its own band, because
//! the radar paints between them and moving a satellite wash over the radar is a different
//! control (the layer's band), not a reorder.
//!
//! The order lives in `Settings::field_order` and reaches the map through
//! `FieldLayer::paint_order`, which the pane's draw list is sorted by.

use super::*;
use crate::render::FieldLayer;
use crate::ui::a11y::Named as _;
use egui::{FontId, Sense, Stroke};
use egui_phosphor::regular as ph;

const ROW_H: f32 = 24.0;

/// What the list asked for.
pub(super) enum OrderHit {
    /// Put `moved` directly above (`above: true`) or below `target` in the list.
    Move {
        moved: FieldLayer,
        target: FieldLayer,
        above: bool,
    },
    Reset,
}

/// The new `field_order` after dragging `moved` next to `target`. `shown` is the band's active
/// layers as listed, top first. Every active layer of that band is written in its new order, so
/// they all hold their places from then on; other layers in `custom` keep theirs. A move across
/// the radar, or onto itself, changes nothing.
pub(super) fn move_layer(
    custom: &[FieldLayer],
    shown: &[FieldLayer],
    moved: FieldLayer,
    target: FieldLayer,
    above: bool,
) -> Option<Vec<FieldLayer>> {
    if moved == target || moved.below_radar() != target.below_radar() {
        return None;
    }
    let mut top_first: Vec<FieldLayer> = shown
        .iter()
        .copied()
        .filter(|l| l.below_radar() == moved.below_radar() && *l != moved)
        .collect();
    let at = top_first.iter().position(|l| *l == target)?;
    top_first.insert(if above { at } else { at + 1 }, moved);
    let mut out: Vec<FieldLayer> = custom
        .iter()
        .copied()
        .filter(|l| !top_first.contains(l))
        .collect();
    out.extend(top_first.iter().rev());
    Some(out)
}

/// Draw the list. `active` is the pane's field layers that are on; `names` gives each its row
/// label and glyph.
pub(super) fn paint_order_group(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    active: &[FieldLayer],
    custom: &[FieldLayer],
    names: &dyn Fn(FieldLayer) -> (String, &'static str),
) -> Option<OrderHit> {
    let order = FieldLayer::paint_order(custom);
    // Top first: the reverse of paint order.
    let shown: Vec<FieldLayer> = order
        .iter()
        .rev()
        .copied()
        .filter(|l| active.contains(l))
        .collect();
    if shown.len() < 2 {
        return None;
    }
    let mut hit = None;
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(ws::text(ph::STACK_SIMPLE, 13.0, t.text_dim));
        ui.label(ws::text("Paint order", 12.5, t.text));
        ui.label(ws::text("top first", 11.0, t.text_faint));
        if !custom.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(10.0);
                if ui
                    .add(egui::Button::new(ws::text("Reset", 11.5, t.text_dim)).frame(false))
                    .named("Reset the paint order to the built-in one")
                    .clicked()
                {
                    hit = Some(OrderHit::Reset);
                }
            });
        }
    });
    let mut radar_drawn = false;
    for (i, &layer) in shown.iter().enumerate() {
        // The radar sits between the last layer above it and the first below it.
        if layer.below_radar() && !radar_drawn {
            radar_drawn = true;
            if i > 0 {
                radar_line(ui, t);
            }
        }
        if let Some(h) = order_row(ui, t, layer, names) {
            hit = Some(h);
        }
    }
    if !radar_drawn {
        radar_line(ui, t);
    }
    ui.add_space(6.0);
    hit
}

fn radar_line(ui: &mut egui::Ui, t: &ws::Tokens) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 16.0), Sense::hover());
    let y = rect.center().y;
    let p = ui.painter();
    let label = p.layout_no_wrap(
        "radar".to_string(),
        FontId::proportional(10.5),
        t.text_faint,
    );
    let mid = rect.left() + 34.0;
    p.line_segment(
        [egui::pos2(rect.left() + 14.0, y), egui::pos2(mid - 4.0, y)],
        Stroke::new(1.0, t.line),
    );
    let after = mid + label.size().x + 4.0;
    p.galley(
        egui::pos2(mid, y - label.size().y / 2.0),
        label,
        t.text_faint,
    );
    p.line_segment(
        [egui::pos2(after, y), egui::pos2(rect.right() - 12.0, y)],
        Stroke::new(1.0, t.line),
    );
}

fn order_row(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    layer: FieldLayer,
    names: &dyn Fn(FieldLayer) -> (String, &'static str),
) -> Option<OrderHit> {
    let (name, glyph) = names(layer);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::hover());
    // The whole row is the drag source; the handle glyph only says so.
    let resp = ui
        .interact(
            rect,
            ui.id().with(("paint_order", layer.slug())),
            Sense::drag(),
        )
        .on_hover_cursor(egui::CursorIcon::Grab)
        .named(&format!("{name}: drag to change its paint order"));
    resp.dnd_set_drag_payload(layer);
    let dragging = resp.dragged();
    let p = ui.painter();
    if dragging {
        p.rect_filled(rect, 0.0, t.accent_soft());
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, t.panel_hi);
    }
    let y = rect.center().y;
    p.text(
        egui::pos2(rect.left() + 20.0, y),
        egui::Align2::CENTER_CENTER,
        ph::DOTS_SIX_VERTICAL,
        FontId::proportional(13.0),
        if resp.hovered() || dragging {
            t.text
        } else {
            t.text_faint
        },
    );
    p.text(
        egui::pos2(rect.left() + 40.0, y),
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(13.0),
        t.text_dim,
    );
    let mut job =
        egui::text::LayoutJob::simple_singleline(name, FontId::proportional(12.5), t.text);
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 64.0).max(20.0));
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    ui.painter().galley(
        egui::pos2(rect.left() + 54.0, y - galley.size().y / 2.0),
        galley,
        t.text,
    );
    // A drop target: the line shows where the dragged layer would land. Rows in the other band
    // show nothing, since the move would do nothing.
    let pointer = ui.ctx().pointer_interact_pos();
    let above = pointer.is_some_and(|p| p.y < y);
    let mut hit = None;
    let fits = |moved: FieldLayer| moved != layer && moved.below_radar() == layer.below_radar();
    if let Some(moved) = resp.dnd_hover_payload::<FieldLayer>() {
        if fits(*moved) {
            let ly = if above { rect.top() } else { rect.bottom() };
            ui.painter().line_segment(
                [
                    egui::pos2(rect.left() + 12.0, ly),
                    egui::pos2(rect.right() - 12.0, ly),
                ],
                Stroke::new(2.0, t.accent),
            );
        }
    }
    if let Some(moved) = resp.dnd_release_payload::<FieldLayer>() {
        if fits(*moved) {
            hit = Some(OrderHit::Move {
                moved: *moved,
                target: layer,
                above,
            });
        }
    }
    hit
}

#[cfg(test)]
mod tests {
    use super::*;
    use FieldLayer as FL;

    fn above(order: &[FieldLayer], a: FieldLayer, b: FieldLayer) -> bool {
        let at = |l| order.iter().position(|x| *x == l).unwrap();
        at(a) > at(b)
    }

    #[test]
    fn dragging_a_layer_over_another_paints_it_over_that_one() {
        // Two layers in one band, in their built-in order.
        let band: Vec<FieldLayer> = FL::DRAW_ORDER
            .into_iter()
            .filter(|l| !l.below_radar())
            .take(2)
            .collect();
        let (low, high) = (band[0], band[1]);
        let order = FL::paint_order(&[]);
        assert!(above(&order, high, low), "built-in: {high:?} over {low:?}");
        let shown = [high, low]; // top first
        let custom = move_layer(&[], &shown, low, high, true).unwrap();
        let order = FL::paint_order(&custom);
        assert!(above(&order, low, high));
        // Everything else keeps its place.
        let untouched: Vec<_> = FL::paint_order(&[])
            .into_iter()
            .filter(|l| *l != low && *l != high)
            .collect();
        let now: Vec<_> = order
            .into_iter()
            .filter(|l| *l != low && *l != high)
            .collect();
        assert_eq!(untouched, now);
    }

    #[test]
    fn a_layer_never_crosses_the_radar_and_the_default_is_the_built_in_order() {
        let below = FL::DRAW_ORDER
            .into_iter()
            .find(|l| l.below_radar())
            .unwrap();
        let over = FL::DRAW_ORDER
            .into_iter()
            .find(|l| !l.below_radar())
            .unwrap();
        assert!(move_layer(&[], &[over, below], below, over, true).is_none());
        assert!(move_layer(&[], &[over], over, over, true).is_none());
        let bands: Vec<bool> = FL::paint_order(&[])
            .iter()
            .map(|l| l.below_radar())
            .collect();
        let first_above = bands.iter().position(|b| !b).unwrap();
        assert!(
            bands[first_above..].iter().all(|b| !b),
            "below band first, then above"
        );
        assert_eq!(FL::paint_order(&[]).len(), FL::DRAW_ORDER.len());
    }
}
