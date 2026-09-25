//! The tool rail: every map tool as a square button against the map's left edge, in groups, with
//! the 3D volume explorer at its foot. Each arms its tool through the palette action, so it is the
//! same tool the ribbon and the phone rail arm.

use super::*;
use crate::ui::a11y::Named as _;
use egui_phosphor::regular as ph;

/// The rail's width: one button and its margin.
const RAIL_W: f32 = ws::RAIL_BTN + 8.0;

/// The tools, grouped: looking, measuring, the atmosphere, marking up the map.
const GROUPS: [&[(MapTool, &str, &str)]; 4] = [
    &[
        (MapTool::Interrogate, ph::CURSOR, "Explore the map"),
        (
            MapTool::GateInspector,
            ph::CROSSHAIR,
            "Inspect a radar gate",
        ),
    ],
    &[
        (MapTool::Measure, ph::RULER, "Measure distance"),
        (MapTool::CrossSection, ph::CHART_LINE_UP, "Cross-section"),
        (MapTool::RegionStats, ph::CHART_SCATTER, "Region statistics"),
    ],
    &[
        (MapTool::Sounding, ph::THERMOMETER, "Sounding"),
        (MapTool::Forecast, ph::CLOUD_SUN, "Point forecast"),
    ],
    &[
        (MapTool::Marker, ph::MAP_PIN, "Drop a marker"),
        (MapTool::Draw, ph::PENCIL_SIMPLE, "Draw on the map"),
        (MapTool::AlertZone, ph::WARNING, "Draw a watch zone"),
    ],
];

impl HookEchoApp {
    pub(super) fn dock_rail(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        use crate::app::PaletteAction as A;
        let t = self.ws_tokens();
        let armed = self.tool;
        let mut pick = None;
        egui::Panel::left("dock_rail")
            .exact_size(RAIL_W)
            .resizable(false)
            .frame(
                egui::Frame::NONE
                    .fill(t.bg)
                    .stroke(egui::Stroke::new(1.0, t.line))
                    .inner_margin(egui::Margin::symmetric(4, 8)),
            )
            .show(root, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                egui::ScrollArea::vertical()
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .auto_shrink([true, false])
                    .show(ui, |ui| {
                        for (gi, group) in GROUPS.iter().enumerate() {
                            if gi > 0 {
                                let (r, _) = ui.allocate_exact_size(
                                    egui::vec2(ws::RAIL_BTN, 9.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().line_segment(
                                    [
                                        egui::pos2(r.left() + 6.0, r.center().y),
                                        egui::pos2(r.right() - 6.0, r.center().y),
                                    ],
                                    egui::Stroke::new(1.0, t.line),
                                );
                            }
                            for &(tool, glyph, name) in group.iter() {
                                let on = armed == tool;
                                if ws::rail_button(ui, &t, glyph, on)
                                    .named_toggle(name, on)
                                    .clicked()
                                {
                                    pick = Some(A::Tool(tool));
                                }
                            }
                        }
                    });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    if ws::rail_button(ui, &t, ph::CUBE, false)
                        .named("Open the 3D volume explorer")
                        .clicked()
                    {
                        pick = Some(A::OpenWindow(AppWindow::Volume3d));
                    }
                });
            });
        if let Some(a) = pick {
            self.apply_palette(a, ctx);
        }
    }
}
