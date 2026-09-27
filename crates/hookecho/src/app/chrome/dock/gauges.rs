//! The Flood gauges window: the flood-gauge dashboard (`ui::gauge_dashboard`) as a workstation
//! tool window — every river gauge in view with its category, stage, forecast, a week's
//! sparkline and 24-hour change, and the selected gauge's hydrograph and history — docked or
//! floating like the other tool windows. The classic layouts show the same body in a plain
//! floating window.

use super::*;
use egui_phosphor::regular as ph;

pub(super) const GAUGES_W: f32 = 440.0;

impl HookEchoApp {
    pub(super) fn dock_gauges(&mut self, host: Host<'_>, ctx: &egui::Context) {
        if !self.dock.gauges.open {
            return;
        }
        let t = self.ws_tokens();
        let map_rect = self.chrome_rect;
        let place = self.dock.gauges.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.gauges.collapsed;
        let body_h = (map_rect.height() - 60.0).clamp(240.0, 760.0);
        let tz = self.active_tz();
        let (rt, http) = (self.spawner.clone(), self.http.clone());
        let zoomed_out = self.gauges_zoomed_out();
        let title = if self.show_gauges && !self.gauges.is_empty() {
            format!("Flood gauges ({})", self.gauges.len())
        } else {
            "Flood gauges".to_string()
        };
        let mut header = ws::HeaderAction::None;
        let mut outs = Vec::new();
        let (dash, gauges, cards, layer_on) = (
            &mut self.gauge_dash,
            &self.gauges,
            &mut self.gauge_cards,
            self.show_gauges,
        );
        tool_window(
            host,
            ToolWindow {
                id: "dock_gauges",
                place,
                width: GAUGES_W,
                float_at: map_rect.right_top() + egui::vec2(-GAUGES_W - 24.0, 12.0),
            },
            map_rect,
            &t,
            |ui| {
                header = ws::window_header(
                    ui,
                    &t,
                    ph::DROP,
                    &title,
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                let scroll = egui::ScrollArea::vertical()
                    .id_salt("dock_gauges_scroll")
                    .auto_shrink([false, floating]);
                let scroll = if floating {
                    scroll.max_height(body_h)
                } else {
                    scroll
                };
                // Docked, the table takes a good part of the column and the selected gauge the
                // rest; floating, it keeps to a height so the card below stays in reach.
                let table_h = if floating {
                    240.0
                } else {
                    (ui.available_height() * 0.38).clamp(160.0, 420.0)
                };
                scroll.show(ui, |ui| {
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            outs = crate::ui::gauge_dashboard::body(
                                ui,
                                &t,
                                dash,
                                gauges,
                                cards,
                                crate::ui::gauge_dashboard::Env {
                                    tz,
                                    spawner: &rt,
                                    http: &http,
                                    layer_on,
                                    zoomed_out,
                                    max_table_h: table_h,
                                },
                            );
                        });
                });
            },
        );
        self.dock.apply_header(DockWin::Gauges, header);
        for o in outs {
            self.gauge_dash_out(o, ctx);
        }
    }
}
