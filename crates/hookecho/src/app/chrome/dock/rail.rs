//! The tool rail against the map's left edge: the Layers window and "center on the radar" first,
//! then every map tool as a square button in groups, with the 3D volume explorer at its foot.
//! Each tool arms through the palette action, so it is the same tool the ribbon and the phone
//! rail arm. Only one tool is armed at a time.

use super::*;
use crate::ui::a11y::Named as _;
use egui_phosphor::regular as ph;

/// The rail's width: one button and its margin.
const RAIL_W: f32 = ws::RAIL_BTN + 8.0;

/// The zoom "center on the radar" frames the site at: a radar's useful range fills the map.
const RADAR_ZOOM: f64 = 8.0;

/// The tools, grouped: looking, measuring, the atmosphere and the ground, marking up the map.
/// Every `MapTool` is here — [`rail_group`]'s exhaustive match will not compile otherwise.
const GROUPS: [&[(MapTool, &str, &str)]; 4] = [
    &[
        (MapTool::Interrogate, ph::CURSOR, "Explore the map"),
        (
            MapTool::GateInspector,
            ph::CROSSHAIR,
            "Inspect a radar gate",
        ),
        (
            MapTool::RadarSuitability,
            ph::CELL_TOWER,
            "Radar suitability: which radar sees a point best",
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
        (MapTool::Climatology, ph::TORNADO, "Tornado climatology"),
        (
            MapTool::Chase,
            ph::NAVIGATION_ARROW,
            "Set your chase location",
        ),
    ],
    &[
        (MapTool::Marker, ph::MAP_PIN, "Drop a marker"),
        (MapTool::Draw, ph::PENCIL_SIMPLE, "Draw on the map"),
        (MapTool::AlertZone, ph::WARNING, "Draw a watch zone"),
    ],
];

/// Which rail group a tool sits in. Exhaustive, so a new tool has to be given a button.
fn rail_group(tool: MapTool) -> usize {
    match tool {
        MapTool::Interrogate | MapTool::GateInspector | MapTool::RadarSuitability => 0,
        MapTool::Measure | MapTool::CrossSection | MapTool::RegionStats => 1,
        MapTool::Sounding | MapTool::Forecast | MapTool::Climatology | MapTool::Chase => 2,
        MapTool::Marker | MapTool::Draw | MapTool::AlertZone => 3,
    }
}

impl HookEchoApp {
    pub(super) fn dock_rail(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        use crate::app::PaletteAction as A;
        let t = self.ws_tokens();
        let armed = self.tool;
        let layers_open = self.dock.layers.open;
        let has_site = self.views[self.active].site.is_some();
        let mut pick = None;
        let mut toggle_layers = false;
        let mut center = false;
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
                        if ws::rail_button(ui, &t, ph::STACK, layers_open)
                            .named_toggle("Layers", layers_open)
                            .clicked()
                        {
                            toggle_layers = true;
                        }
                        let r = ui.add_enabled_ui(has_site, |ui| {
                            ws::rail_button(ui, &t, ph::CROSSHAIR_SIMPLE, false)
                        });
                        if r.inner.named("Center on the radar").clicked() {
                            center = true;
                        }
                        for (gi, group) in GROUPS.iter().enumerate() {
                            separator(ui, &t);
                            for &(tool, glyph, name) in group.iter() {
                                debug_assert_eq!(rail_group(tool), gi, "{name}");
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
        if toggle_layers {
            self.dock.layers.open = !self.dock.layers.open;
        }
        if center {
            self.dock_center_on_radar();
        }
        if let Some(a) = pick {
            self.apply_palette(a, ctx);
        }
    }

    /// Put the active pane's radar in the middle of the map at a storm-scale zoom, keeping a 3D
    /// view's pitch and bearing.
    fn dock_center_on_radar(&mut self) {
        let v = &mut self.views[self.active];
        let Some(site) = v.site.as_deref().and_then(wxdata::sites::site_by_id) else {
            return;
        };
        let old = v.camera;
        let mut cam = crate::render::mercator::Camera::at_lonlat(
            f64::from(site.longitude),
            f64::from(site.latitude),
            RADAR_ZOOM,
        );
        cam.pitch = old.pitch;
        cam.bearing = old.bearing;
        v.camera = cam;
    }
}

/// The hairline between the rail's groups.
fn separator(ui: &mut egui::Ui, t: &ws::Tokens) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ws::RAIL_BTN, 9.0), egui::Sense::hover());
    ui.painter().line_segment(
        [
            egui::pos2(r.left() + 6.0, r.center().y),
            egui::pos2(r.right() - 6.0, r.center().y),
        ],
        egui::Stroke::new(1.0, t.line),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_is_on_the_rail_once_in_its_group() {
        let mut seen = Vec::new();
        for (gi, group) in GROUPS.iter().enumerate() {
            for &(tool, _, name) in group.iter() {
                assert_eq!(rail_group(tool), gi, "{name} is in the wrong group");
                assert!(!seen.contains(&tool), "{name} is on the rail twice");
                seen.push(tool);
            }
        }
        // `rail_group` names every variant, so its arms count the tools.
        assert_eq!(seen.len(), 13);
    }
}
