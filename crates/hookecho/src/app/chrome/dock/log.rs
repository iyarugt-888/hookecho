//! The Analyst log window: Analyst Mode's live sweep, provider-health and failover log
//! (`ui::analyst_log_window`) as a workstation tool window. It is present while Analyst Mode is
//! on and docks right by default, joining the other right-hand windows as a tab. Closing it turns
//! Analyst Mode off, exactly as closing the floating window does in the other layouts, so the
//! setting and the window cannot disagree.

use super::*;
use egui_phosphor::regular as ph;

/// The window's width, docked or floating.
pub(super) const LOG_W: f32 = 300.0;

impl HookEchoApp {
    pub(super) fn dock_log(&mut self, host: Host<'_>) {
        if !self.dock.log.open || !self.dock.log_available {
            return;
        }
        let t = self.ws_tokens();
        let map_rect = self.chrome_rect;
        let place = self.dock.log.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.log.collapsed;
        let float_h = (map_rect.height() - 120.0).clamp(160.0, 420.0);
        let mut header = ws::HeaderAction::None;
        tool_window(
            host,
            ToolWindow {
                id: "dock_log",
                place,
                width: LOG_W,
                float_at: map_rect.right_top() + egui::vec2(-LOG_W - 24.0, 90.0),
            },
            map_rect,
            &t,
            |ui| {
                header = ws::window_header(
                    ui,
                    &t,
                    ph::TERMINAL_WINDOW,
                    "Analyst log",
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        let h = if floating {
                            float_h
                        } else {
                            (ui.available_height() - 40.0).max(120.0)
                        };
                        crate::ui::analyst_log_window::body(ui, h);
                    });
            },
        );
        if header == ws::HeaderAction::Close {
            // The log is Analyst Mode's window: closing it is how the mode is switched off here,
            // with the log level restored the same way the Settings checkbox does it.
            self.settings.analyst_mode = false;
            crate::devlog::set_analyst_mode(false);
        }
        self.dock.apply_header(DockWin::Log, header);
    }
}
