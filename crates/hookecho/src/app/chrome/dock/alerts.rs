//! The Alerts window: the warnings, watches and advisories in view, newest first, with the mute
//! switch — the same list (`ui::alert_panel`) the floating panel's Alerts tab and the phone sheet
//! draw. A row flies the map to the alert and opens its bulletin.

use super::*;
use egui_phosphor::regular as ph;

/// The window's width, docked or floating.
const ALERTS_W: f32 = 300.0;

impl HookEchoApp {
    pub(super) fn dock_alerts(&mut self, host: Host<'_>) {
        if !self.dock.alerts.open {
            return;
        }
        let t = self.ws_tokens();
        let map_rect = self.chrome_rect;
        let (count, _) = self.alert_badge();
        let bounds = self.view_bounds();
        let feats = self.active_alert_features().to_vec();
        let mut muted = self.settings.mute_alerts;
        let place = self.dock.alerts.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.alerts.collapsed;
        let list_h = (map_rect.height() - 60.0).clamp(160.0, 560.0);
        let title = if count == 0 {
            "Alerts".to_string()
        } else {
            format!("Alerts ({count})")
        };
        let mut header = ws::HeaderAction::None;
        let mut hit = None;
        tool_window(
            host,
            ToolWindow {
                id: "dock_alerts",
                place,
                width: ALERTS_W,
                // Beside where a floating Layers window starts, clear of the Inspector's corner.
                float_at: map_rect.left_top() + egui::vec2(12.0 + LEFT_WIDTH + 12.0, 12.0),
            },
            map_rect,
            &t,
            |ui| {
                header = ws::window_header(
                    ui,
                    &t,
                    ph::BELL,
                    &title,
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                let scroll = egui::ScrollArea::vertical().auto_shrink([false, floating]);
                let scroll = if floating {
                    scroll.max_height(list_h)
                } else {
                    scroll
                };
                scroll.show(ui, |ui| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            hit = crate::ui::alert_panel::body(ui, &feats, bounds, &mut muted);
                        });
                });
            },
        );
        apply_header(header, &mut self.dock.alerts);
        self.settings.mute_alerts = muted;
        if let Some((id, lon, lat)) = hit {
            // Fly the active camera to the alert and open its bulletin, as the panel does.
            let cam = &mut self.views[self.active].camera;
            cam.center = crate::render::mercator::lonlat_to_world(lon, lat);
            cam.zoom = cam.zoom.max(8.0);
            self.open_alert_popup(&id);
        }
    }
}
