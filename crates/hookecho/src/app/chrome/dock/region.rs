//! The Region window: the region-statistics tool's results (ROADMAP_NEW C4) as a workstation tool
//! window, in place of the floating egui window the classic layouts keep. The body is
//! [`crate::ui::region_stats_window::dock_body`]; hovering a histogram bar or a scatter cell
//! outlines those gates on the map.

use super::*;
use egui_phosphor::regular as ph;

pub(super) const REGION_W: f32 = 360.0;

impl HookEchoApp {
    pub(super) fn dock_region(&mut self, host: Host<'_>) {
        if !self.dock.region.open || !self.dock.region_available {
            return;
        }
        let t = self.ws_tokens();
        let map_rect = self.chrome_rect;
        let place = self.dock.region.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.region.collapsed;
        let body_h = (map_rect.height() - 60.0).clamp(200.0, 720.0);
        let Some((s, cache, st)) = self.region.dock_parts() else {
            return;
        };
        let mut header = ws::HeaderAction::None;
        let mut hl = None;
        let mut pass = 0;
        tool_window(
            host,
            ToolWindow {
                id: "dock_region",
                place,
                width: REGION_W,
                float_at: map_rect.right_top() + egui::vec2(-REGION_W - 24.0, 12.0),
            },
            map_rect,
            &t,
            |ui| {
                pass = ui.ctx().cumulative_pass_nr();
                header = ws::window_header(
                    ui,
                    &t,
                    ph::CHART_SCATTER,
                    "Region",
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                let scroll = egui::ScrollArea::vertical()
                    .id_salt("dock_region_scroll")
                    .auto_shrink([false, floating]);
                let scroll = if floating {
                    scroll.max_height(body_h)
                } else {
                    scroll
                };
                scroll.show(ui, |ui| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            crate::ui::region_stats_window::dock_body(
                                ui, &t, s, cache, st, &mut hl,
                            );
                        });
                });
            },
        );
        if let Some(h) = hl {
            let pts = crate::app::region_stats::highlight_points(s, &h);
            self.region.set_highlight(pass, pts);
        }
        if header == ws::HeaderAction::Close {
            self.region.clear();
        }
        self.dock.apply_header(DockWin::Region, header);
    }
}
