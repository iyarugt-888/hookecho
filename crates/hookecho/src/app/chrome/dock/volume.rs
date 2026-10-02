//! The 3D volume window: the orbitable reflectivity raymarch (`ui::volume3d_window`) as a
//! workstation tool window, so it docks, floats and folds like the others instead of being the
//! one plain egui window over the map. Floating by default (the view wants room); docked, it takes
//! the dock's width and the height left under its controls. "View in 3D" on a storm opens it
//! cropped to that storm, as before.

use super::*;
use egui_phosphor::regular as ph;

pub(super) const VOLUME_W: f32 = 460.0;

impl HookEchoApp {
    pub(super) fn dock_volume(&mut self, host: Host<'_>) {
        if !self.dock.volume.open || !self.dock.volume_available {
            return;
        }
        // Dock panels paint before floating_windows polls workers. Validate at this paint
        // boundary too, so a freshly accepted radar update cannot draw the prior grid once.
        self.sync_volume3d_status();
        let t = self.ws_tokens();
        let map_rect = self.chrome_rect;
        let place = self.dock.volume.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.volume.collapsed;
        let degraded = crate::ui::motion::degraded();
        let (range, cappi) = (self.vol3d_range, self.cappi_alt_km);
        let mut header = ws::HeaderAction::None;
        tool_window(
            host,
            ToolWindow {
                id: "dock_volume",
                place,
                width: VOLUME_W,
                float_at: map_rect.center_top() + egui::vec2(-VOLUME_W / 2.0, 12.0),
            },
            map_rect,
            &t,
            |ui| {
                header = ws::window_header(
                    ui,
                    &t,
                    ph::CUBE,
                    "3D volume",
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        // Floating: a fixed view under the controls. Docked: whatever the dock
                        // column has left, so the view grows with the window.
                        let view = if floating {
                            egui::vec2(ui.available_width(), 380.0)
                        } else {
                            egui::vec2(
                                ui.available_width(),
                                (ui.available_height() - 110.0).max(240.0),
                            )
                        };
                        crate::ui::volume3d_window::body(
                            ui,
                            &mut self.vol3d,
                            &mut self.vol3d_pending,
                            crate::app::VOL3D_N as u32,
                            crate::app::VOL3D_NZ as u32,
                            crate::app::VOL3D_TOP_KM,
                            range,
                            cappi,
                            degraded,
                            view,
                        );
                    });
            },
        );
        if header == ws::HeaderAction::Close {
            self.show_3d = false;
        }
        self.dock.apply_header(DockWin::Volume, header);
    }
}
