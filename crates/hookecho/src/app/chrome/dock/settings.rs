//! Main Settings inside the workstation, sharing every control with the legacy drawer.

use super::*;
use crate::ui::settings_window::{SyncAction, SyncView};

pub(super) const SETTINGS_W: f32 = 620.0;

impl HookEchoApp {
    pub(crate) fn open_settings(&mut self) {
        self.settings_window.open = true;
        if self.workstation_chrome() {
            self.dock.bring_forward(DockWin::Settings);
        }
    }

    pub(super) fn dock_settings(&mut self, host: Host<'_>) {
        if !self.dock.settings.open {
            return;
        }
        self.settings_window.open = true;
        self.settings_window.prepare();
        let t = self.ws_tokens();
        let map = self.chrome_rect;
        let place = self.dock.settings.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.settings.collapsed;
        let entries = self.palette_entries();
        let mut header = ws::HeaderAction::None;
        let mut action = None;
        tool_window(
            host,
            ToolWindow {
                id: "dock_settings",
                place,
                width: SETTINGS_W.min((map.width() - 24.0).max(200.0)),
                float_at: egui::pos2(map.center().x - SETTINGS_W / 2.0, map.top() + 12.0),
            },
            map,
            &t,
            |ui| {
                header = ws::window_header(
                    ui,
                    &t,
                    egui_phosphor::regular::GEAR,
                    "Settings",
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                let height = if floating {
                    (map.height() - 70.0).clamp(100.0, 640.0)
                } else {
                    ui.available_height()
                };
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::Frame::NONE
                            .inner_margin(egui::Margin::same(10))
                            .show(ui, |ui| {
                                ws::style_scope(ui, &t);
                                action = self.settings_window.show_body(
                                    ui,
                                    &mut self.settings,
                                    &self.palettes,
                                    SyncView {
                                        signed_in: self.sync_tokens.is_some(),
                                        status: &self.sync_status,
                                        login_url: self.sync_login.as_ref().map(|p| p.url.as_str()),
                                        last_sync: self.sync_state.last_sync,
                                    },
                                    &entries,
                                    true,
                                );
                            });
                    },
                );
            },
        );
        self.dock.settings_action = action;
        self.dock.apply_header(DockWin::Settings, header);
    }

    /// Drain Settings actions once after the chrome has drawn, whichever host drew the editor.
    pub(crate) fn settings_frame(&mut self, ctx: &egui::Context, workstation: bool) {
        let action = if workstation {
            self.settings_window.open = self.dock.settings.open;
            self.settings_window.prepare();
            if !self.dock.shown(DockWin::Settings) || self.dock.settings.collapsed {
                self.settings_window.capturing = false;
            }
            self.dock.settings_action.take()
        } else {
            let entries = if self.settings_window.open {
                self.palette_entries()
            } else {
                std::sync::Arc::from(Vec::new())
            };
            let action = self.settings_window.show(
                ctx,
                &mut self.settings,
                &self.palettes,
                SyncView {
                    signed_in: self.sync_tokens.is_some(),
                    status: &self.sync_status,
                    login_url: self.sync_login.as_ref().map(|p| p.url.as_str()),
                    last_sync: self.sync_state.last_sync,
                },
                &entries,
                &mut self.drawer,
            );
            // Switching to a workstation from Appearance carries the open editor with it.
            if self.settings_window.open && self.workstation_chrome() {
                self.dock.bring_forward(DockWin::Settings);
            }
            action
        };
        self.capture_key = self.settings_window.capturing;
        if std::mem::take(&mut self.settings_window.run_setup) {
            self.firstrun.start();
        }
        if std::mem::take(&mut self.settings_window.run_tour) {
            self.tour.start();
        }
        match action {
            Some(SyncAction::SignIn) => self.sync_sign_in(),
            Some(SyncAction::SignOut) => self.sync_sign_out(),
            Some(SyncAction::SyncNow) => self.sync_now(),
            None => {}
        }
    }
}
