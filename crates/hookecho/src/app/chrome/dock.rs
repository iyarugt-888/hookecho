//! The docked-panel layout ("Dock (ImGui)" under Appearance → Theme), for a tablet or a desktop
//! that wants every control on screen at once: a tab row across the top, a layers tree on the left,
//! info panels on the right, and a timeline panel under the map.
//!
//! It is a second *presentation* of things that already exist, not a second set of features. The
//! layers tree is the command registry (`palette_entries`) grouped by category with a checkbox per
//! row; the checkboxes and buttons go through the same palette actions as every other surface, and
//! the timeline drives the same `Timeline`. So a layer switched on here is the layer switched on in
//! the ribbon, the palette and the phone sheet.

use super::*;
use crate::ui::a11y::Named as _;
use egui::{Color32, RichText, Stroke};

const BG: Color32 = Color32::from_rgb(11, 16, 24);
const PANEL: Color32 = Color32::from_rgb(15, 22, 33);
const TITLE: Color32 = Color32::from_rgb(24, 36, 54);
const BORDER: Color32 = Color32::from_rgb(44, 58, 84);
const TAB_ON: Color32 = Color32::from_rgb(38, 102, 214);
const TEXT: Color32 = Color32::from_rgb(214, 222, 235);
const DIM: Color32 = Color32::from_rgb(140, 152, 172);
const SELECT: Color32 = Color32::from_rgb(28, 78, 168);

/// Which slice of the registry the left panel's tab row shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum DockTab {
    #[default]
    Layers,
    Radar,
    Models,
    Weather,
    /// Only what is switched on right now.
    Active,
}

impl DockTab {
    pub(crate) const ALL: [DockTab; 5] = [
        DockTab::Layers,
        DockTab::Radar,
        DockTab::Models,
        DockTab::Weather,
        DockTab::Active,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            DockTab::Layers => "Layers",
            DockTab::Radar => "Radar",
            DockTab::Models => "Models",
            DockTab::Weather => "Weather",
            DockTab::Active => "Active",
        }
    }

    /// Whether a registry row belongs on this tab.
    fn includes(self, category: &str, on: bool) -> bool {
        match self {
            DockTab::Layers => true,
            DockTab::Radar => matches!(category, "Radar" | "Sites"),
            DockTab::Models => category == "Models",
            DockTab::Weather => matches!(category, "National" | "Severe" | "Obs"),
            DockTab::Active => on,
        }
    }
}

/// Everything the dock remembers between frames.
pub(crate) struct DockState {
    pub tab: DockTab,
    pub query: String,
    pub left_open: bool,
    pub right_open: bool,
    pub timeline_open: bool,
    pub quick_open: bool,
    pub selected_open: bool,
    pub info_open: bool,
}

impl Default for DockState {
    fn default() -> Self {
        Self {
            tab: DockTab::Layers,
            query: String::new(),
            left_open: true,
            right_open: true,
            timeline_open: true,
            quick_open: true,
            selected_open: true,
            info_open: true,
        }
    }
}

/// One category's rows, as indexes into the registry slice, with how many are switched on.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Group {
    pub category: &'static str,
    pub rows: Vec<usize>,
    pub on: usize,
}

/// Group registry rows by category for one tab and one search, in the panel's category order.
///
/// A row shows when it is on the tab and matches the search; a category with no rows left is
/// dropped rather than shown empty. `on` counts the rows *shown* that are switched on, which is the
/// "(2/28)" beside a category's name.
pub(crate) fn group_entries(entries: &[PaletteEntry], tab: DockTab, query: &str) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for cat in crate::ui::layers_panel::CATEGORIES {
        let mut rows = Vec::new();
        let mut on = 0;
        for (i, e) in entries.iter().enumerate() {
            if e.category != cat {
                continue;
            }
            let is_on = e.on == Some(true);
            if !tab.includes(e.category, is_on) {
                continue;
            }
            if !query.is_empty()
                && crate::ui::layers_panel::fuzzy(query, &e.label).is_none()
                && crate::ui::layers_panel::fuzzy(query, e.desc).is_none()
            {
                continue;
            }
            on += usize::from(is_on);
            rows.push(i);
        }
        if !rows.is_empty() {
            groups.push(Group {
                category: cat,
                rows,
                on,
            });
        }
    }
    groups
}

fn frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER))
        .inner_margin(egui::Margin::same(6))
}

fn mono(text: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(text.into()).monospace().size(size).color(color)
}

/// A panel's title bar: a small accent tick, the name, and a close button on the right. Returns
/// whether close was pressed.
fn title_bar(ui: &mut egui::Ui, name: &str) -> bool {
    let mut close = false;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, TITLE);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
        0.0,
        TAB_ON,
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(10.0, 0.0),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::monospace(13.0),
        TEXT,
    );
    let x = egui::Rect::from_center_size(
        rect.right_center() - egui::vec2(14.0, 0.0),
        egui::vec2(22.0, 22.0),
    );
    let resp = ui.interact(x, ui.id().with(("close", name)), egui::Sense::click());
    ui.painter().text(
        x.center(),
        egui::Align2::CENTER_CENTER,
        egui_phosphor::regular::X,
        egui::FontId::proportional(13.0),
        if resp.hovered() { Color32::WHITE } else { DIM },
    );
    if resp.on_hover_text("Close").clicked() {
        close = true;
    }
    close
}

impl HookEchoApp {
    /// Draw the whole dock. Called once per frame before the map's own rect is read, so the map
    /// gets whatever the panels leave.
    pub(crate) fn dock_layout(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        self.dock_menu(root, ctx);
        self.dock_left(root, ctx);
        self.dock_right(root, ctx);
        self.dock_timeline(root);
    }

    fn dock_menu(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        use crate::app::PaletteAction as A;
        let mut action = None;
        egui::Panel::top("dock_menu")
            .exact_size(76.0)
            .frame(egui::Frame::NONE.fill(BG).inner_margin(egui::Margin::symmetric(8, 4)))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(mono(
                            format!("HookEcho v{}", crate::ui::about_window::VERSION),
                            15.0,
                            Color32::WHITE,
                        ));
                        ui.label(mono("Real-time weather radar", 10.0, DIM));
                    });
                    ui.add_space(24.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let ok = self.views[self.active].error.is_none();
                        ui.label(mono(
                            if ok { "Online" } else { "Trouble" },
                            12.0,
                            TEXT,
                        ));
                        ui.label(mono(
                            "\u{25cf}",
                            14.0,
                            if ok {
                                Color32::from_rgb(70, 200, 90)
                            } else {
                                Color32::from_rgb(220, 70, 60)
                            },
                        ));
                        let now = chrono::Local::now();
                        ui.label(mono(now.format("%b %d, %Y  %-I:%M %p").to_string(), 12.0, DIM));
                    });
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let tabs: [(&str, bool); 7] = [
                        ("Map", true),
                        ("Layers", self.dock.left_open),
                        ("Playback", self.dock.timeline_open),
                        ("Forecast", false),
                        ("Tools", false),
                        ("Settings", false),
                        ("Help", false),
                    ];
                    for (name, on) in tabs {
                        let b = ui
                            .add(
                                egui::Button::new(mono(name, 13.0, TEXT))
                                    .fill(if on { SELECT } else { Color32::TRANSPARENT })
                                    .stroke(Stroke::new(1.0, if on { TAB_ON } else { BORDER }))
                                    .corner_radius(2.0)
                                    .min_size(egui::vec2(76.0, 24.0)),
                            )
                            .named(name);
                        if b.clicked() {
                            match name {
                                "Layers" => self.dock.left_open = !self.dock.left_open,
                                "Playback" => self.dock.timeline_open = !self.dock.timeline_open,
                                "Forecast" => action = Some(A::OpenWindow(AppWindow::Afd)),
                                "Tools" => action = Some(A::OpenWindow(AppWindow::LayerManager)),
                                "Settings" => action = Some(A::OpenWindow(AppWindow::Settings)),
                                "Help" => action = Some(A::OpenWindow(AppWindow::Help)),
                                _ => {}
                            }
                        }
                    }
                });
            });
        if let Some(a) = action {
            self.apply_palette(a, ctx);
        }
    }

    fn dock_left(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        if !self.dock.left_open {
            return;
        }
        let entries = self.palette_entries();
        let groups = group_entries(&entries, self.dock.tab, &self.dock.query);
        let mut chosen = None;
        let mut close = false;
        let mut open_manager = false;
        egui::Panel::left("dock_layers")
            .exact_size(300.0)
            .resizable(false)
            .frame(frame())
            .show(root, |ui| {
                close = title_bar(ui, "Layers & Tools");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for t in DockTab::ALL {
                        let on = self.dock.tab == t;
                        if ui
                            .add(
                                egui::Button::new(mono(t.label(), 12.0, TEXT))
                                    .fill(if on { SELECT } else { Color32::TRANSPARENT })
                                    .stroke(Stroke::new(1.0, if on { TAB_ON } else { BORDER }))
                                    .corner_radius(2.0),
                            )
                            .named_toggle(t.label(), on)
                            .clicked()
                        {
                            self.dock.tab = t;
                        }
                    }
                });
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.dock.query)
                        .hint_text("Search layers (e.g. reflectivity, HRRR...)")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(4.0);
                let list_h = (ui.available_height() - 36.0).max(80.0);
                egui::ScrollArea::vertical()
                    .max_height(list_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if groups.is_empty() {
                            ui.label(mono("Nothing matches.", 12.0, DIM));
                        }
                        for g in &groups {
                            let name = crate::ui::layers_panel::category_name(g.category);
                            egui::CollapsingHeader::new(mono(
                                format!("{name}  ({}/{})", g.on, g.rows.len()),
                                13.0,
                                TEXT,
                            ))
                            .id_salt(("dock_group", g.category))
                            .default_open(matches!(g.category, "Radar" | "Models"))
                            .show(ui, |ui| {
                                for &i in &g.rows {
                                    let e = &entries[i];
                                    let mut on = e.on == Some(true);
                                    let toggleable = e.on.is_some();
                                    let row = if toggleable {
                                        ui.checkbox(&mut on, mono(&e.label, 12.0, TEXT))
                                    } else {
                                        ui.add(
                                            egui::Button::new(mono(&e.label, 12.0, TEXT))
                                                .fill(Color32::TRANSPARENT)
                                                .stroke(Stroke::NONE),
                                        )
                                    };
                                    let row = if e.desc.is_empty() {
                                        row
                                    } else {
                                        row.on_hover_text(e.desc)
                                    };
                                    if row.clicked() || row.changed() {
                                        chosen = Some(e.action);
                                    }
                                }
                            });
                        }
                    });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(mono("Import layer", 12.0, TEXT)).min_size(egui::vec2(136.0, 26.0)))
                        .on_hover_text("Import a GeoJSON or Shapefile as an overlay")
                        .clicked()
                    {
                        chosen = Some(crate::app::PaletteAction::ImportGis);
                    }
                    if ui
                        .add(egui::Button::new(mono("Manage layers", 12.0, TEXT)).min_size(egui::vec2(136.0, 26.0)))
                        .clicked()
                    {
                        open_manager = true;
                    }
                });
            });
        if close {
            self.dock.left_open = false;
        }
        if open_manager {
            self.apply_palette(
                crate::app::PaletteAction::OpenWindow(AppWindow::LayerManager),
                ctx,
            );
        }
        if let Some(a) = chosen {
            self.apply_palette(a, ctx);
        }
    }

    fn dock_right(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        use crate::app::OverlayToggle as T;
        if !self.dock.right_open {
            return;
        }
        // Read before the panel closure takes `&mut self`.
        let toggles: Vec<(T, &str, bool)> = [
            (T::RangeRings, "Range rings"),
            (T::RadarSites, "Radar sites"),
            (T::Alerts, "Warnings"),
            (T::Watches, "Watches"),
            (T::GlmLightning, "Lightning"),
            (T::Tracks, "Storm tracks"),
        ]
        .into_iter()
        .map(|(t, n)| (t, n, *self.overlay_flag(t)))
        .collect();
        let basemap = if self.settings.basemap.is_empty() {
            "Default".to_string()
        } else {
            self.settings.basemap.clone()
        };
        let (site, vcp, moment_name, tilt_deg, valid, age) = {
            let v = &self.views[self.active];
            let site = v.site.clone().unwrap_or_else(|| "\u{2014}".to_string());
            let vcp = v
                .volume
                .as_ref()
                .map(|x| x.vcp.split(" (").next().unwrap_or_default().to_string())
                .unwrap_or_default();
            let tilt_deg = v
                .volume
                .as_ref()
                .and_then(|x| x.elevations.get(v.tilt).copied());
            let valid = v.timeline.current().and_then(|id| id.date_time());
            let age = v.timeline.newest().and_then(|id| id.date_time()).map(|t| {
                humanize((chrono::Utc::now() - t).num_seconds().max(0))
            });
            (
                site,
                vcp,
                crate::products::name(v.moment, v.srv),
                tilt_deg,
                valid,
                age,
            )
        };
        let tz = self.active_tz();
        let cam = self.views[self.active].camera;
        let (lon, lat) = crate::render::mercator::world_to_lonlat(cam.center.0, cam.center.1);
        let map_rect = self.chrome_rect;
        let mouse = ctx
            .input(|i| i.pointer.hover_pos())
            .filter(|p| map_rect.contains(*p) && self.views.len() == 1)
            .map(|p| {
                let w = cam.screen_to_world(
                    (p.x - map_rect.left(), p.y - map_rect.top()),
                    self.last_viewport,
                );
                crate::render::mercator::world_to_lonlat(w.0, w.1)
            });
        let frames = {
            let t = &self.views[self.active].timeline;
            (t.playhead + 1, t.slot_count())
        };
        let forecast = self.views[self.active].timeline.forecast_hour();
        let mut smoothing = self.views[self.active].smooth;
        let mut flip = None;
        let mut cycle_basemap = false;
        let mut show_layers = false;
        egui::Panel::right("dock_right")
            .exact_size(258.0)
            .resizable(false)
            .frame(frame())
            .show(root, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if self.dock.quick_open {
                        if title_bar(ui, "Quick Controls") {
                            self.dock.quick_open = false;
                        }
                        ui.horizontal(|ui| {
                            ui.label(mono("Map style", 12.0, DIM));
                            if ui
                                .add(egui::Button::new(mono(&basemap, 12.0, TEXT)).min_size(egui::vec2(120.0, 22.0)))
                                .on_hover_text("Next map style")
                                .clicked()
                            {
                                cycle_basemap = true;
                            }
                        });
                        for (t, name, on) in &toggles {
                            let mut v = *on;
                            if ui.checkbox(&mut v, mono(*name, 12.0, TEXT)).changed() {
                                flip = Some(*t);
                            }
                        }
                        ui.checkbox(&mut smoothing, mono("Smoothing", 12.0, TEXT));
                        ui.add_space(6.0);
                    }
                    if self.dock.selected_open {
                        if title_bar(ui, "Selected Layer") {
                            self.dock.selected_open = false;
                        }
                        ui.label(mono(moment_name, 14.0, Color32::WHITE));
                        let row = |ui: &mut egui::Ui, k: &str, v: String| {
                            ui.horizontal(|ui| {
                                ui.label(mono(format!("{k:<8}"), 11.0, DIM));
                                ui.label(mono(v, 11.0, TEXT));
                            });
                        };
                        row(ui, "Radar:", format!("{site}  {vcp}"));
                        if let Some(d) = tilt_deg {
                            row(ui, "Tilt:", format!("{d:.1}\u{b0}"));
                        }
                        if let Some(d) = valid {
                            row(ui, "Valid:", crate::timefmt::fmt_clock(d, tz, false));
                        }
                        if let Some(a) = &age {
                            row(ui, "Age:", format!("{a} ago"));
                        }
                        if ui
                            .add(egui::Button::new(mono("Layers", 12.0, TEXT)).min_size(egui::vec2(100.0, 24.0)))
                            .clicked()
                        {
                            show_layers = true;
                        }
                        ui.add_space(6.0);
                    }
                    if self.dock.info_open {
                        if title_bar(ui, "Map Information") {
                            self.dock.info_open = false;
                        }
                        let row = |ui: &mut egui::Ui, k: &str, v: String| {
                            ui.horizontal(|ui| {
                                ui.label(mono(format!("{k:<12}"), 11.0, DIM));
                                ui.label(mono(v, 11.0, TEXT));
                            });
                        };
                        row(ui, "Center:", format!("{lat:.2}, {lon:.2}"));
                        row(ui, "Zoom:", format!("{:.1}", cam.zoom));
                        row(ui, "Projection:", "Web Mercator".to_string());
                        row(
                            ui,
                            "Mouse:",
                            mouse.map_or_else(
                                || "\u{2014}".to_string(),
                                |(lo, la)| format!("{la:.2}, {lo:.2}"),
                            ),
                        );
                        row(ui, "Radar:", format!("{site} ({vcp})"));
                        if let Some(h) = forecast {
                            row(ui, "Forecast:", format!("F+{h}h"));
                        }
                        row(ui, "Frames:", format!("{} / {}", frames.0, frames.1));
                    }
                });
            });
        self.views[self.active].smooth = smoothing;
        if show_layers {
            self.dock.left_open = true;
        }
        if cycle_basemap {
            self.apply_palette(crate::app::PaletteAction::CycleBasemap, ctx);
        }
        if let Some(t) = flip {
            self.apply_palette(crate::app::PaletteAction::ToggleOverlay(t), ctx);
        }
    }

    fn dock_timeline(&mut self, root: &mut egui::Ui) {
        if !self.dock.timeline_open {
            return;
        }
        let tz = self.active_tz();
        let site = self.views[self.active]
            .site
            .clone()
            .unwrap_or_else(|| "no site".to_string());
        let mut close = false;
        let mut go_head = false;
        egui::Panel::bottom("dock_timeline")
            .exact_size(112.0)
            .resizable(false)
            .frame(frame())
            .show(root, |ui| {
                close = title_bar(ui, "Timeline & Playback");
                let t = &mut self.views[self.active].timeline;
                let slots = t.slot_count();
                let observed = t.frames.len();
                ui.horizontal(|ui| {
                    let btn = |ui: &mut egui::Ui, glyph: &str, name: &str| {
                        ui.add(
                            egui::Button::new(RichText::new(glyph).size(15.0).color(TEXT))
                                .min_size(egui::vec2(34.0, 28.0)),
                        )
                        .named(name)
                        .clicked()
                    };
                    if btn(ui, egui_phosphor::regular::SKIP_BACK, "Jump to start") {
                        t.go_begin();
                    }
                    if btn(ui, egui_phosphor::regular::REWIND, "Previous frame") {
                        t.step(-1);
                    }
                    let playing = t.playing;
                    if btn(
                        ui,
                        if playing {
                            egui_phosphor::regular::PAUSE
                        } else {
                            egui_phosphor::regular::PLAY
                        },
                        if playing { "Pause" } else { "Play" },
                    ) {
                        t.toggle_play();
                    }
                    if btn(ui, egui_phosphor::regular::FAST_FORWARD, "Next frame") {
                        t.step(1);
                    }
                    if btn(ui, egui_phosphor::regular::SKIP_FORWARD, "Jump to newest") {
                        go_head = true;
                    }
                    if slots > 1 {
                        let mut at = t.playhead as f64;
                        // A slider is as wide as the style says, not as wide as the space it is
                        // given, so the width is set on the style for this one.
                        ui.spacing_mut().slider_width = (ui.available_width() - 210.0).max(120.0);
                        let slider = ui.add(
                            egui::Slider::new(&mut at, 0.0..=(slots - 1) as f64)
                                .show_value(false)
                                .trailing_fill(true),
                        );
                        if slider.changed() {
                            let idx = (at as usize).min(slots - 1);
                            if idx != t.playhead {
                                t.playhead = idx;
                                t.playing = false;
                                t.following = idx + 1 == observed;
                            }
                        }
                    }
                    let label = match t.forecast_hour() {
                        Some(h) => format!("F +{h}h"),
                        None => t
                            .current()
                            .and_then(|id| id.date_time())
                            .map(|d| crate::timefmt::fmt_clock(d, tz, false))
                            .unwrap_or_default(),
                    };
                    ui.label(mono(label, 13.0, Color32::WHITE));
                    egui::ComboBox::from_id_salt("dock_speed")
                        .width(64.0)
                        .selected_text(mono(format!("{:.0} fps", t.speed), 12.0, TEXT))
                        .show_ui(ui, |ui| {
                            for s in [2.0f32, 4.0, 6.0, 8.0, 12.0, 16.0] {
                                ui.selectable_value(&mut t.speed, s, format!("{s:.0} fps"));
                            }
                        });
                    // The archive lives behind the date: without this the dock could only ever
                    // scrub today's frames. Same day-seek path as the desktop scrubber's menu.
                    let cal = ui
                        .add(
                            egui::Button::new(
                                RichText::new(format!(
                                    "{}  {}",
                                    egui_phosphor::regular::CALENDAR_BLANK,
                                    t.date.format("%b %-d, %Y")
                                ))
                                .size(12.0)
                                .color(TEXT),
                            )
                            .min_size(egui::vec2(0.0, 28.0)),
                        )
                        .named("Archive calendar");
                    egui::Popup::menu(&cal)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            ui.set_min_width(240.0);
                            ui.horizontal(|ui| {
                                if ui
                                    .button(egui_phosphor::regular::CARET_LEFT)
                                    .named("Previous day")
                                    .clicked()
                                {
                                    if let Some(d) = t.date.pred_opt() {
                                        super::scrubber::seek_to_day(t, &site, d);
                                    }
                                }
                                if let Some(d) = archive_day_input(ui, t.date) {
                                    super::scrubber::seek_to_day(t, &site, d);
                                }
                                let is_today = t.date >= chrono::Utc::now().date_naive();
                                if ui
                                    .add_enabled(
                                        !is_today,
                                        egui::Button::new(egui_phosphor::regular::CARET_RIGHT),
                                    )
                                    .named("Next day")
                                    .clicked()
                                {
                                    if let Some(d) = t.date.succ_opt() {
                                        super::scrubber::seek_to_day(t, &site, d);
                                    }
                                }
                            });
                            if let Some(d) = archive_day_calendar(ui, t.date) {
                                super::scrubber::seek_to_day(t, &site, d);
                            }
                        });
                });
                // One dot per frame, the current one filled: a click picks it.
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 20.0),
                    egui::Sense::click(),
                );
                if slots > 0 {
                    let step = rect.width() / slots as f32;
                    for i in 0..slots {
                        let c = egui::pos2(rect.left() + step * (i as f32 + 0.5), rect.center().y);
                        let now = i == t.playhead;
                        ui.painter().circle_filled(
                            c,
                            if now { 4.0 } else { 2.0 },
                            if now { TAB_ON } else { DIM },
                        );
                    }
                    if resp.clicked() {
                        if let Some(p) = resp.interact_pointer_pos() {
                            let idx = (((p.x - rect.left()) / step) as usize).min(slots - 1);
                            t.playhead = idx;
                            t.playing = false;
                            t.following = idx + 1 == observed;
                        }
                    }
                }
                ui.horizontal(|ui| {
                    let first = t.frames.first().and_then(|id| id.date_time());
                    let cur = t.current().and_then(|id| id.date_time());
                    if let Some(d) = first {
                        ui.label(mono(
                            format!("Start: {}", crate::timefmt::fmt_clock(d, tz, false)),
                            11.0,
                            DIM,
                        ));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(d) = cur {
                            ui.label(mono(
                                format!("Valid: {}", crate::timefmt::fmt_clock(d, tz, false)),
                                11.0,
                                DIM,
                            ));
                        }
                    });
                });
            });
        if go_head {
            self.views[self.active].timeline.go_head();
        }
        if close {
            self.dock.timeline_open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::PaletteAction;

    fn entry(label: &str, category: &'static str, on: Option<bool>) -> PaletteEntry {
        PaletteEntry {
            label: label.to_string(),
            category,
            action: PaletteAction::Reload,
            on,
            desc: "",
            common: false,
            key: None,
            health: None,
        }
    }

    fn sample() -> Vec<PaletteEntry> {
        vec![
            entry("Reflectivity", "Radar", Some(true)),
            entry("Velocity", "Radar", Some(false)),
            entry("KTLX", "Sites", Some(false)),
            entry("HRRR future radar", "Models", Some(true)),
            entry("GFS precipitation", "Models", Some(false)),
            entry("Watches", "Severe", Some(false)),
            entry("Measure", "Tools", None),
        ]
    }

    #[test]
    fn the_layers_tab_shows_every_category_in_the_panels_order() {
        let g = group_entries(&sample(), DockTab::Layers, "");
        let cats: Vec<_> = g.iter().map(|g| g.category).collect();
        assert_eq!(cats, ["Radar", "Sites", "Severe", "Models", "Tools"]);
    }

    #[test]
    fn each_group_counts_the_rows_that_are_on() {
        let g = group_entries(&sample(), DockTab::Layers, "");
        let radar = g.iter().find(|g| g.category == "Radar").unwrap();
        assert_eq!((radar.on, radar.rows.len()), (1, 2));
        let models = g.iter().find(|g| g.category == "Models").unwrap();
        assert_eq!((models.on, models.rows.len()), (1, 2));
    }

    #[test]
    fn a_tab_keeps_only_its_categories_and_drops_the_empty_ones() {
        let g = group_entries(&sample(), DockTab::Models, "");
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].category, "Models");
        let weather = group_entries(&sample(), DockTab::Weather, "");
        assert_eq!(weather.len(), 1);
        assert_eq!(weather[0].category, "Severe");
    }

    #[test]
    fn the_active_tab_shows_only_what_is_switched_on() {
        let g = group_entries(&sample(), DockTab::Active, "");
        let labels: Vec<_> = g
            .iter()
            .flat_map(|g| g.rows.iter().map(|&i| sample()[i].label.clone()))
            .collect();
        assert_eq!(labels, ["Reflectivity", "HRRR future radar"]);
    }

    #[test]
    fn a_search_narrows_rows_and_a_category_with_none_left_disappears() {
        let g = group_entries(&sample(), DockTab::Layers, "hrrr");
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].category, "Models");
        assert_eq!(g[0].rows, vec![3]);
        assert!(group_entries(&sample(), DockTab::Layers, "zzzzqq").is_empty());
    }

    #[test]
    fn a_row_that_is_not_a_toggle_counts_as_off() {
        // "Measure" is a tool with no on/off state: it is listed, and never counted as switched on.
        let g = group_entries(&sample(), DockTab::Layers, "");
        let tools = g.iter().find(|g| g.category == "Tools").unwrap();
        assert_eq!((tools.on, tools.rows.len()), (0, 1));
    }
}

/// The cursor readout's rows for a point on the map, given the active radar's position: latitude
/// and longitude, and (when there is a radar) how far and in which direction it is. Pure, so the
/// numbers can be tested without a window.
pub(crate) fn cursor_readout(
    at: (f64, f64),
    radar: Option<(f64, f64)>,
    metric: bool,
) -> Vec<(&'static str, String)> {
    let (lon, lat) = at;
    let mut rows = vec![
        ("Lat", format!("{lat:.2}")),
        ("Lon", format!("{lon:.2}")),
    ];
    if let Some((rlon, rlat)) = radar {
        let (km, bearing) = crate::geo::great_circle([rlon, rlat], [lon, lat]);
        rows.push(("Range", crate::geo::fmt_distance(km, metric, 1)));
        rows.push(("Az", format!("{:.1}\u{b0}", bearing.rem_euclid(360.0))));
    }
    rows
}

impl HookEchoApp {
    /// The map's own tool strip and a cursor readout, over its top-left corner: the mockup's
    /// vertical strip of tools beside the map. Every button arms a tool through the palette action,
    /// so it is the same tool the ribbon and the phone rail arm.
    pub(crate) fn dock_map_overlay(&mut self, ctx: &egui::Context) {
        use crate::app::PaletteAction as A;
        use egui_phosphor::regular as ph;
        if !self.dock.left_open && !self.dock.right_open && !self.dock.timeline_open {
            // A fully undocked map is the "hide everything" view; leave it clean.
            return;
        }
        let cam = self.views[self.active].camera;
        let map_rect = self.chrome_rect;
        let single = self.views.len() == 1;
        let radar = self.views[self.active]
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
            .map(|s| (f64::from(s.longitude), f64::from(s.latitude)));
        let metric = self.metric_in(self.active);
        let mouse = ctx
            .input(|i| i.pointer.hover_pos())
            .filter(|p| map_rect.contains(*p) && single)
            .map(|p| {
                let w = cam.screen_to_world(
                    (p.x - map_rect.left(), p.y - map_rect.top()),
                    self.last_viewport,
                );
                crate::render::mercator::world_to_lonlat(w.0, w.1)
            });
        let armed = self.tool;
        let mut pick = None;
        egui::Area::new(egui::Id::new("dock_map_overlay"))
            .constrain_to(map_rect)
            .anchor(egui::Align2::LEFT_TOP, egui::vec2(map_rect.left() + 8.0, map_rect.top() + 8.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                if let Some(at) = mouse {
                    egui::Frame::NONE
                        .fill(Color32::from_rgba_unmultiplied(11, 16, 24, 225))
                        .stroke(Stroke::new(1.0, BORDER))
                        .inner_margin(egui::Margin::same(6))
                        .show(ui, |ui| {
                            for (k, v) in cursor_readout(at, radar, metric) {
                                ui.label(mono(format!("{k:<6}{v}"), 11.0, TEXT));
                            }
                        });
                }
                egui::Frame::NONE
                    .fill(Color32::from_rgba_unmultiplied(11, 16, 24, 225))
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(egui::Margin::same(3))
                    .show(ui, |ui| {
                        let tools = [
                            (MapTool::Interrogate, ph::CURSOR, "Explore the map"),
                            (MapTool::GateInspector, ph::CROSSHAIR, "Inspect a radar gate"),
                            (MapTool::Measure, ph::RULER, "Measure distance"),
                            (MapTool::CrossSection, ph::CHART_LINE_UP, "Cross-section"),
                            (MapTool::Sounding, ph::THERMOMETER, "Sounding"),
                            (MapTool::Marker, ph::MAP_PIN, "Drop a marker"),
                            (MapTool::AlertZone, ph::WARNING, "Draw a watch zone"),
                            (MapTool::Draw, ph::PENCIL_SIMPLE, "Draw on the map"),
                        ];
                        for (tool, glyph, name) in tools {
                            let on = armed == tool;
                            let b = ui
                                .add(
                                    egui::Button::new(RichText::new(glyph).size(17.0).color(TEXT))
                                        .min_size(egui::vec2(32.0, 32.0))
                                        .fill(if on { SELECT } else { Color32::TRANSPARENT })
                                        .stroke(Stroke::new(1.0, if on { TAB_ON } else { Color32::TRANSPARENT }))
                                        .corner_radius(2.0),
                                )
                                .on_hover_text(name)
                                .named_toggle(name, on);
                            if b.clicked() {
                                pick = Some(tool);
                            }
                        }
                    });
            });
        if let Some(t) = pick {
            self.apply_palette(A::Tool(t), ctx);
        }
    }
}

#[cfg(test)]
mod overlay_tests {
    use super::cursor_readout;

    #[test]
    fn the_readout_gives_position_and_the_radars_range_and_bearing() {
        // Due east of the radar, one degree of longitude at 35 N: about 91 km.
        let rows = cursor_readout((-96.0, 35.0), Some((-97.0, 35.0)), true);
        let get = |k: &str| rows.iter().find(|r| r.0 == k).map(|r| r.1.clone()).unwrap();
        assert_eq!(get("Lat"), "35.00");
        assert_eq!(get("Lon"), "-96.00");
        let km: f64 = get("Range").trim_end_matches(" km").parse().unwrap();
        assert!((km - 91.0).abs() < 2.0, "{km}");
        let az: f64 = get("Az").trim_end_matches('\u{b0}').parse().unwrap();
        assert!((az - 90.0).abs() < 1.0, "{az}");
    }

    #[test]
    fn without_a_radar_there_is_no_range_or_bearing_to_invent() {
        let rows = cursor_readout((-96.0, 35.0), None, true);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn the_range_follows_the_units_setting() {
        let miles = cursor_readout((-96.0, 35.0), Some((-97.0, 35.0)), false);
        assert!(miles.iter().any(|r| r.0 == "Range" && r.1.ends_with(" mi")), "{miles:?}");
    }
}
