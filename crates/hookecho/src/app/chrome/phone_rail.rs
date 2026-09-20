//! The phone chrome that the five designs (`ui::phone_design`) draw differently: the tool rail and
//! the 2D / 3D / Tilt / Dual bar. One routine, parameterised by the design's `Spec`, so a design is
//! a row of data and switching between them never changes what a button does.

use super::*;
use crate::ui::a11y::Named as _;
use crate::ui::phone_design::{Mode, RailItem, Side};
use egui_phosphor::regular as ph;

/// Where the mode bar sits under the pill, and how tall it is with its gap. The rail and the
/// windows that open on the left start below it.
pub(crate) const MODE_BAR_H: f32 = 44.0;

impl HookEchoApp {
    /// The accent the touch chrome wears: the person's own pick if they made one, otherwise the
    /// design's. Desktop and tablet keep the theme's accent.
    pub(crate) fn chrome_accent(&self) -> egui::Color32 {
        if !crate::platform::phone_layout() {
            return crate::theme::accent(self.settings.theme);
        }
        let [r, g, b] = self
            .settings
            .accent
            .unwrap_or(self.settings.phone_design.spec().accent);
        egui::Color32::from_rgb(r, g, b)
    }

    /// Which mode the active pane is in. Tilt wins over everything (the tilted map is its own
    /// camera), then the 3D volume window, then two panes; otherwise the flat map.
    fn phone_mode(&self) -> Mode {
        if self.views[self.active].map_3d.enabled {
            Mode::Tilt
        } else if self.show_3d {
            Mode::Volume
        } else if self.views.len() == 2 {
            Mode::Dual
        } else {
            Mode::Flat
        }
    }

    /// Put the app in `mode`, leaving the others. Each goes through the existing palette action
    /// rather than poking state, so a button here and the same command from the palette cannot
    /// disagree about what a mode means.
    fn set_phone_mode(&mut self, mode: Mode, ctx: &egui::Context) {
        use crate::app::PaletteAction as A;
        if self.phone_mode() == mode {
            return;
        }
        // Leave whatever is on first, so modes do not stack.
        if self.views[self.active].map_3d.enabled && mode != Mode::Tilt {
            self.apply_palette(A::ToggleMap3d, ctx);
        }
        if self.show_3d && mode != Mode::Volume {
            self.show_3d = false;
        }
        if self.views.len() == 2 && mode != Mode::Dual {
            self.apply_palette(A::SetPanes(1), ctx);
        }
        match mode {
            Mode::Flat => {}
            Mode::Tilt => self.apply_palette(A::ToggleMap3d, ctx),
            Mode::Volume => self.apply_palette(A::OpenWindow(AppWindow::Volume3d), ctx),
            Mode::Dual => self.apply_palette(A::SetPanes(2), ctx),
        }
    }

    /// The segmented 2D / 3D / Tilt / Dual control under the site pill.
    pub(crate) fn phone_mode_bar(&mut self, ctx: &egui::Context) {
        let spec = self.settings.phone_design.spec();
        let accent = self.chrome_accent();
        let current = self.phone_mode();
        let mut pick = None;
        egui::Area::new(egui::Id::new("phone_mode_bar"))
            .constrain_to(self.chrome_rect)
            .anchor(
                egui::Align2::LEFT_TOP,
                egui::vec2(crate::ui::m3::SP_3, chrome::phone_top(ctx) + 56.0),
            )
            .show(ctx, |ui| {
                crate::ui::style::glass(ui, spec.panel_alpha)
                    .corner_radius(spec.corner)
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
                        ui.horizontal(|ui| {
                            for &m in spec.modes {
                                let on = m == current;
                                let text = egui::RichText::new(m.label())
                                    .size(crate::ui::m3::T_LABEL_LG)
                                    .strong()
                                    .color(if on {
                                        egui::Color32::BLACK
                                    } else {
                                        egui::Color32::from_gray(225)
                                    });
                                let b = ui
                                    .add(
                                        egui::Button::new(text)
                                            .min_size(egui::vec2(58.0, 36.0))
                                            .fill(if on { accent } else { egui::Color32::TRANSPARENT })
                                            .corner_radius((spec.corner - 4.0).max(6.0)),
                                    )
                                    .named_toggle(&format!("{} view", m.label()), on);
                                if b.clicked() {
                                    pick = Some(m);
                                }
                            }
                        });
                    });
            });
        if let Some(m) = pick {
            self.set_phone_mode(m, ctx);
        }
    }

    /// Centre the map on the device. The first press starts the location feed (which asks for the
    /// permission); once there is a position, a press recentres on it.
    fn locate_me(&mut self) {
        if self.gps_rx.is_none() {
            let rx = if cfg!(target_os = "android") {
                crate::platform::start_location()
            } else {
                #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
                {
                    crate::gps::spawn()
                }
                #[cfg(any(target_os = "android", target_arch = "wasm32"))]
                {
                    None
                }
            };
            match rx {
                Some(rx) => {
                    self.gps_rx = Some(rx);
                    self.chase_mode = true;
                }
                None => log::warn!("no position source available"),
            }
        } else if let Some((lon, lat)) = self.chase_pos {
            let cam = &mut self.views[self.active].camera;
            *cam = crate::render::mercator::Camera::at_lonlat(lon, lat, cam.zoom.max(8.0));
        }
    }

    /// The tool rail: a column of buttons on the design's chosen edge, in its chosen order.
    pub(crate) fn phone_rail(&mut self, ctx: &egui::Context) {
        use crate::app::PaletteAction as A;
        let spec = self.settings.phone_design.spec();
        let accent = self.chrome_accent();
        let (alert_count, esc) = self.alert_badge();
        let layers_on = self.panel_open && !self.show_alert_panel;
        let alerts_on = self.panel_open && self.show_alert_panel;
        let measuring = self.tool == MapTool::Measure;
        let sectioning = self.tool == MapTool::CrossSection;
        let (align, x) = match spec.rail_side {
            Side::Right => (egui::Align2::RIGHT_TOP, -crate::ui::m3::SP_3),
            Side::Left => (egui::Align2::LEFT_TOP, crate::ui::m3::SP_3),
        };
        let mut alerts_anchor = None;
        let mut action: Option<A> = None;
        let mut toggle_layers = None;
        let mut locate = false;
        egui::Area::new(egui::Id::new("control_column"))
            .constrain_to(self.chrome_rect)
            .anchor(
                align,
                egui::vec2(x, chrome::phone_top(ctx) + 56.0 + MODE_BAR_H + 8.0),
            )
            .show(ctx, |ui| {
                crate::ui::style::glass(ui, spec.panel_alpha)
                    .corner_radius(spec.corner)
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 4.0);
                        for &item in spec.rail {
                            let (glyph, label, on) = match item {
                                RailItem::Locate => (ph::NAVIGATION_ARROW, "Locate", false),
                                RailItem::Layers => (ph::STACK, "Layers", layers_on),
                                RailItem::Basemaps => (ph::MAP_TRIFOLD, "Maps", self.basemap_open),
                                RailItem::Alerts => (ph::BELL, "Alerts", alerts_on),
                                RailItem::Share => (ph::SHARE_NETWORK, "Share", false),
                                RailItem::Measure => (ph::RULER, "Measure", measuring),
                                RailItem::Analysis => {
                                    (ph::CHART_LINE_UP, "Section", sectioning)
                                }
                                RailItem::Settings => (ph::GEAR, "Settings", false),
                            };
                            let resp = rail_button(ui, glyph, label, on, accent, &spec);
                            let name = match item {
                                RailItem::Layers => "Layers, products and tools",
                                RailItem::Basemaps => "Background map",
                                RailItem::Alerts => "Active alerts in view",
                                RailItem::Share => "Share this view",
                                RailItem::Locate => "Center on my location",
                                RailItem::Measure => "Measure distance",
                                RailItem::Analysis => "Cross-section",
                                RailItem::Settings => "Settings",
                            };
                            let resp = resp.named_toggle(name, on);
                            if item == RailItem::Alerts {
                                alerts_anchor = Some(resp.rect);
                            }
                            if resp.clicked() {
                                match item {
                                    RailItem::Locate => locate = true,
                                    RailItem::Layers => toggle_layers = Some((layers_on, false)),
                                    RailItem::Alerts => toggle_layers = Some((alerts_on, true)),
                                    RailItem::Basemaps => self.basemap_open = !self.basemap_open,
                                    RailItem::Share => action = Some(A::CopyViewLink),
                                    RailItem::Measure => action = Some(A::Tool(MapTool::Measure)),
                                    RailItem::Analysis => {
                                        action = Some(A::Tool(MapTool::CrossSection))
                                    }
                                    RailItem::Settings => {
                                        action = Some(A::OpenWindow(AppWindow::Settings))
                                    }
                                }
                            }
                            if item == RailItem::Alerts && alert_count > 0 {
                                let c = match esc {
                                    0 => crate::ui::style::OMEGA_ORANGE,
                                    1 => egui::Color32::from_rgb(230, 120, 60),
                                    _ => egui::Color32::from_rgb(200, 20, 20),
                                };
                                let at = resp.rect.right_top() + egui::vec2(-4.0, 4.0);
                                ui.painter().circle_filled(at, 8.0, c);
                                ui.painter().text(
                                    at,
                                    egui::Align2::CENTER_CENTER,
                                    alert_count.min(99).to_string(),
                                    egui::FontId::proportional(10.0),
                                    egui::Color32::BLACK,
                                );
                            }
                        }
                    });
            });
        self.tour_anchors.alerts = alerts_anchor;
        if locate {
            self.locate_me();
        }
        if let Some((was_on, alerts)) = toggle_layers {
            self.panel_open = !was_on;
            self.show_alert_panel = alerts;
        }
        if let Some(a) = action {
            self.apply_palette(a, ctx);
        }
    }
}

/// One rail button: an icon, and its label beneath it when the design labels its tools.
fn rail_button(
    ui: &mut egui::Ui,
    glyph: &str,
    label: &str,
    on: bool,
    accent: egui::Color32,
    spec: &crate::ui::phone_design::Spec,
) -> egui::Response {
    let (fg, fill) = if on {
        (egui::Color32::BLACK, accent)
    } else {
        (
            egui::Color32::from_gray(238),
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 0),
        )
    };
    let size = if spec.rail_labels {
        egui::vec2(56.0, 60.0)
    } else {
        egui::vec2(48.0, 48.0)
    };
    let text = if spec.rail_labels {
        egui::RichText::new(format!("{glyph}\n{label}")).size(crate::ui::m3::T_LABEL_LG - 3.0)
    } else {
        egui::RichText::new(glyph).size(crate::ui::style::FONT_TITLE)
    };
    ui.add(
        egui::Button::new(text.color(fg))
            .min_size(size)
            .fill(fill)
            .stroke(egui::Stroke::NONE)
            .corner_radius((spec.corner - 4.0).max(6.0)),
    )
}

impl HookEchoApp {
    /// How much of the map's left and right edges the tool rail takes, so the banner and the
    /// windows that open beside it stay clear of whichever side the design put it on.
    pub(crate) fn phone_gutters(&self) -> (f32, f32) {
        let spec = self.settings.phone_design.spec();
        let rail_w = if spec.rail_labels { 76.0 } else { 68.0 };
        match spec.rail_side {
            Side::Left => (rail_w, 0.0),
            Side::Right => (0.0, rail_w),
        }
    }
}

impl HookEchoApp {
    /// A panel's opacity: the phone design's own on a phone, the surface's usual one elsewhere.
    pub(crate) fn chrome_alpha(&self, usual: u8) -> u8 {
        if crate::platform::phone_layout() {
            self.settings.phone_design.spec().panel_alpha
        } else {
            usual
        }
    }

    /// A panel's corner radius: the phone design's own on a phone, the surface's usual one elsewhere.
    pub(crate) fn chrome_corner(&self, usual: f32) -> f32 {
        if crate::platform::phone_layout() {
            self.settings.phone_design.spec().corner
        } else {
            usual
        }
    }
}
