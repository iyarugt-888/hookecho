//! Compact valid-time or unavailable badge for model comparison panes.

use crate::fielddiff::{ComparisonTimes, DiffField};
use crate::render::FieldLayer;
use std::collections::HashSet;

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_for_view(
    painter: &egui::Painter,
    map: egui::Rect,
    layers: &HashSet<FieldLayer>,
    field: DiffField,
    diff_times: Option<ComparisonTimes>,
    compare_times: Option<ComparisonTimes>,
    diff_error: Option<&str>,
    compare_error: Option<&str>,
) {
    let (a, b) = field.pair();
    let compare_side = if layers.contains(&FieldLayer::CompareB) {
        Some((b, false))
    } else if layers.contains(&FieldLayer::CompareA) {
        Some((a, true))
    } else {
        None
    };
    let (caption, failed) = if let Some((model, side_a)) = compare_side {
        match compare_times {
            Some(times) => {
                let lead = if side_a {
                    times.a_lead_hours
                } else {
                    times.b_lead_hours
                };
                (
                    format!(
                        "{model} +{lead}h · valid {}",
                        times.valid.format("%Y-%m-%d %H:%MZ")
                    ),
                    false,
                )
            }
            None => unavailable(compare_error),
        }
    } else if layers.contains(&FieldLayer::ModelDiff) {
        match diff_times {
            Some(times) => (
                format!(
                    "{a} − {b} {} · valid {}",
                    field.label(),
                    times.valid.format("%Y-%m-%d %H:%MZ")
                ),
                false,
            ),
            None => unavailable(diff_error),
        }
    } else {
        return;
    };
    paint(painter, map, &caption, failed);
}

fn unavailable(error: Option<&str>) -> (String, bool) {
    match error {
        Some(_) => (
            "⚠ Model comparison unavailable · see Layer settings".into(),
            true,
        ),
        None => ("Aligning model valid times…".into(), false),
    }
}

fn paint(painter: &egui::Painter, map: egui::Rect, caption: &str, failed: bool) {
    let color = if failed {
        egui::Color32::from_rgb(255, 197, 92)
    } else {
        egui::Color32::WHITE
    };
    let font = egui::FontId::proportional(12.0);
    let galley = painter.layout_no_wrap(caption.to_string(), font.clone(), color);
    let center = egui::pos2(map.center().x, map.top() + 53.0);
    let background = egui::Rect::from_center_size(center, galley.size() + egui::vec2(14.0, 8.0));
    painter.rect_filled(background, 4.0, egui::Color32::from_black_alpha(215));
    painter.text(center, egui::Align2::CENTER_CENTER, caption, font, color);
}
