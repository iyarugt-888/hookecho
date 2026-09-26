//! The Sounding window: the point sounding (`ui::sounding_window`) — indices, the Skew-T and the
//! hodograph — as a workstation tool window, present once a point has been sounded and docked
//! right by default, where it joins the Inspector as a tab instead of a 560 px window over the
//! map. The plots stack in a dock; floating, it is as wide as the standalone window and they sit
//! side by side. Closing the tab closes the sounding, as closing its window does elsewhere.

use super::*;
use egui_phosphor::regular as ph;

/// The docked width: one plot's width plus margins. Drag the dock wider for more room.
pub(super) const SOUNDING_W: f32 = 320.0;
/// Floating, it has room for the two plots side by side.
const FLOAT_W: f32 = 560.0;

impl HookEchoApp {
    pub(super) fn dock_sounding(&mut self, host: Host<'_>) {
        if !self.dock.sounding.open || !self.dock.sounding_available {
            return;
        }
        let t = self.ws_tokens();
        let tz = self.active_tz();
        let map_rect = self.chrome_rect;
        let place = self.dock.sounding.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.sounding.collapsed;
        let mut header = ws::HeaderAction::None;
        tool_window(
            host,
            ToolWindow {
                id: "dock_sounding",
                place,
                width: if floating { FLOAT_W } else { SOUNDING_W },
                float_at: map_rect.right_top() + egui::vec2(-FLOAT_W - 24.0, 12.0),
            },
            map_rect,
            &t,
            |ui| {
                header = ws::window_header(
                    ui,
                    &t,
                    ph::THERMOMETER,
                    "Sounding",
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        // Side by side only when there is room for both plots.
                        let stacked = ui.available_width() < 540.0;
                        if stacked {
                            egui::ScrollArea::vertical()
                                .id_salt("dock_sounding_scroll")
                                .show(ui, |ui| self.sounding_window.body(ui, tz, true));
                        } else {
                            self.sounding_window.body(ui, tz, stacked);
                        }
                    });
            },
        );
        if header == ws::HeaderAction::Close {
            // The tab is the sounding: closing it closes the sounding itself, as elsewhere.
            self.sounding_window.open = false;
        }
        // A forecast-hour step sets `refetch`; the app answers it after the frame's windows.
        self.dock.apply_header(DockWin::Sounding, header);
    }
}
