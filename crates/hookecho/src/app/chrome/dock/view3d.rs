//! The 3D view window: the active pane's 3D controls (representation, camera, beam rise, floors,
//! quality, slicing) as a workstation tool window. It exists only while that pane is in 3D, and by
//! default docks right, where it joins the Inspector as a tab instead of floating over the map
//! and its colour scale.

use super::*;
use egui_phosphor::regular as ph;

/// The window's width, docked or floating: the Inspector's, so the two share a dock evenly.
pub(super) const VIEW3D_W: f32 = inspector::CARD_W;

impl HookEchoApp {
    pub(super) fn dock_view3d(&mut self, host: Host<'_>) {
        if !self.dock.view3d.open || !self.dock.view3d_available {
            return;
        }
        let t = self.ws_tokens();
        let idx = self.active;
        let map_rect = self.chrome_rect;
        let place = self.dock.view3d.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.view3d.collapsed;
        let body_h = (map_rect.height() - 60.0).clamp(160.0, 520.0);
        let mut header = ws::HeaderAction::None;
        tool_window(
            host,
            ToolWindow {
                id: "dock_view3d",
                place,
                width: VIEW3D_W,
                float_at: map_rect.right_top() + egui::vec2(-VIEW3D_W - 24.0, 12.0),
            },
            map_rect,
            &t,
            |ui| {
                header = ws::window_header(
                    ui,
                    &t,
                    ph::CUBE,
                    "3D view",
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                let scroll = egui::ScrollArea::vertical().auto_shrink([false, floating]);
                let scroll = if floating {
                    scroll.max_height(body_h)
                } else {
                    scroll
                };
                scroll.show(ui, |ui| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| self.map_3d_controls_body(idx, ui));
                });
            },
        );
        self.dock.apply_header(DockWin::View3d, header);
    }
}
