//! The Preferences window: the floating panel's two settings pages, which hold things the Settings
//! window does not — *Map* (background style, crisp or smooth radar, the live sweep indicator,
//! where to open on launch, offline map packs) and *App* (display and streaming mode, location,
//! weather radio, sharing and image export, backup, help). The pages are the panel's own
//! `map_rows`/`app_rows`, so the two layouts can never offer different settings.

use super::*;
use crate::ui::a11y::Named as _;
use egui_phosphor::regular as ph;

/// The window's width, docked or floating.
const PREFS_W: f32 = 300.0;

impl HookEchoApp {
    pub(super) fn dock_prefs(&mut self, host: Host<'_>, ctx: &egui::Context) {
        if !self.dock.prefs.open {
            return;
        }
        let t = self.ws_tokens();
        let map_rect = self.chrome_rect;
        let place = self.dock.prefs.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.prefs.collapsed;
        let body_h = (map_rect.height() - 110.0).clamp(160.0, 560.0);
        let section_id = egui::Id::new("preferences_section");
        let mut header = ws::HeaderAction::None;
        let mut opts = crate::ui::layer_options::UiActions::default();
        tool_window(
            host,
            ToolWindow {
                id: "dock_prefs",
                place,
                width: PREFS_W,
                // Mid-map: the corners are where Layers, Alerts and the Inspector start.
                float_at: egui::pos2(map_rect.center().x - PREFS_W / 2.0, map_rect.top() + 12.0),
            },
            map_rect,
            &t,
            |ui| {
                header = ws::window_header(
                    ui,
                    &t,
                    ph::SLIDERS_HORIZONTAL,
                    "Preferences",
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        let page = self.dock.prefs_page;
                        let section = ui
                            .ctx()
                            .data_mut(|d| d.get_temp::<&'static str>(section_id));
                        ui.horizontal(|ui| {
                            let pick = ws::segmented(
                                ui,
                                &t,
                                &["Map", "App"],
                                usize::from(page == PrefsPage::App),
                            );
                            match pick {
                                Some(0) => self.dock.prefs_page = PrefsPage::Map,
                                Some(_) => self.dock.prefs_page = PrefsPage::App,
                                None => {}
                            }
                            // Inside an App section, a way back to the list of sections.
                            if page == PrefsPage::App && section.is_some() {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ws::icon_button(ui, &t, ph::CARET_LEFT, "Back", false)
                                            .named("Back to the list of preferences")
                                            .clicked()
                                        {
                                            ui.ctx()
                                                .data_mut(|d| d.remove::<&'static str>(section_id));
                                        }
                                    },
                                );
                            }
                        });
                        ui.add_space(6.0);
                        let scroll = egui::ScrollArea::vertical().auto_shrink([false, floating]);
                        let scroll = if floating {
                            scroll.max_height(body_h)
                        } else {
                            scroll
                        };
                        scroll.show(ui, |ui| match self.dock.prefs_page {
                            PrefsPage::Map => self.map_rows(ui, &mut opts),
                            PrefsPage::App => self.app_rows(ui),
                        });
                    });
            },
        );
        apply_header(header, &mut self.dock.prefs);
        self.apply_ui_actions(opts, ctx);
    }
}
