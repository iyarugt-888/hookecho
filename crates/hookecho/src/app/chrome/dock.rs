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
const LEFT_WIDTH: f32 = 264.0;
const RIGHT_WIDTH: f32 = 240.0;
/// Below this width, two sidebars leave too little useful map. Keep one edge panel at a time.
const SINGLE_SIDEBAR_WIDTH: f32 = 1_100.0;

fn single_sidebar(width: f32) -> bool {
    width < SINGLE_SIDEBAR_WIDTH
}

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
    /// The settings of the layers that are on: ensemble, comparison, lightning, satellite and the
    /// rest. Not a slice of the registry, so it has no rows of its own.
    Options,
}

impl DockTab {
    pub(crate) const ALL: [DockTab; 6] = [
        DockTab::Layers,
        DockTab::Radar,
        DockTab::Models,
        DockTab::Weather,
        DockTab::Active,
        DockTab::Options,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            DockTab::Layers => "Layers",
            DockTab::Radar => "Radar",
            DockTab::Models => "Models",
            DockTab::Weather => "Weather",
            DockTab::Active => "Active",
            DockTab::Options => "Options",
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
            DockTab::Options => false,
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
    /// The inspector's model-forecast card (shown only while a model layer is on the map).
    pub model_open: bool,
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
            model_open: true,
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
    RichText::new(text.into())
        .monospace()
        .size(size)
        .color(color)
}

/// Give egui's stock widgets the dock's square, monospace look for one block of controls, so the
/// shared model controls do not look imported into a panel they were not drawn for.
fn dock_style(ui: &mut egui::Ui) {
    let style = ui.style_mut();
    style.override_text_style = Some(egui::TextStyle::Monospace);
    style.spacing.item_spacing = egui::vec2(4.0, 3.0);
    style.spacing.slider_width = 112.0;
    let v = &mut style.visuals;
    v.selection.bg_fill = SELECT;
    v.selection.stroke = Stroke::new(1.0, TAB_ON);
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = egui::CornerRadius::same(2);
    }
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.fg_stroke.color = TEXT;
}

/// The rows of the inspector's model card: what is on the map from the models, when it is valid,
/// and how fresh it is. Pure, so what the card says is checked without a window.
pub(crate) fn model_card_rows(
    input: &crate::ui::model_panel::Input,
    tz: Option<wxdata::tz::Tz>,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<(&'static str, String)> {
    let sel = input.sel;
    let run = match (&input.stamp, input.run) {
        (Some(stamp), _) => stamp
            .run_time
            .map(|r| r.format("%d %HZ").to_string())
            .unwrap_or_else(|| "unknown".into()),
        (None, Some(run)) => run.format("%d %HZ").to_string(),
        (None, None) => "latest".into(),
    };
    let mut rows = vec![
        ("Model:", sel.model.label().to_string()),
        ("Product:", sel.product.label().to_string()),
        ("Run:", run),
        (
            "Lead:",
            if input.range.min == input.range.max {
                "analysis".to_string()
            } else {
                crate::model_browser::format_lead(input.lead_min)
            },
        ),
    ];
    match &input.stamp {
        Some(stamp) => {
            rows.push((
                "Valid:",
                crate::timefmt::fmt_date_clock(stamp.valid_time, tz),
            ));
            rows.push((
                "Fetched:",
                crate::ui::model_panel::ago(stamp.received_time, now),
            ));
        }
        None => rows.push(("Valid:", "loading{2026}".into())),
    }
    rows
}

/// A panel's title bar: a small accent tick, the name, and a close button on the right. Returns
/// whether close was pressed.
fn title_bar(ui: &mut egui::Ui, name: &str) -> bool {
    let mut close = false;
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
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
        // When a window is narrowed after both panels were opened, favor the Layers panel. Its
        // contents are the primary navigation and the Inspector has an explicit tab to reopen it.
        // This keeps the map from becoming a thin strip in the middle of the screen.
        if single_sidebar(root.available_width()) && self.dock.left_open && self.dock.right_open {
            self.dock.right_open = false;
        }
        self.dock_menu(root, ctx);
        self.dock_left(root, ctx);
        self.dock_right(root, ctx);
        self.dock_timeline(root);
        // After the timeline so it stacks just above it, directly under the map it controls.
        self.dock_tilts(root, ctx);
        // Last of the side panels, so it sits against the map rather than against the layers
        // panel: docked, not floating over the data.
        self.dock_tools(root, ctx);
    }

    fn dock_menu(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        use crate::app::PaletteAction as A;
        let mut action = None;
        // The clock in the corner is only right if something repaints it; the map does not while
        // it sits idle.
        ctx.request_repaint_after(std::time::Duration::from_secs(20));
        let models_open = self.dock.left_open && self.dock.tab == DockTab::Models;
        let options_open = self.dock.left_open && self.dock.tab == DockTab::Options;
        egui::Panel::top("dock_menu")
            .exact_size(76.0)
            .frame(
                egui::Frame::NONE
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
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
                        ui.label(mono(if ok { "Online" } else { "Trouble" }, 12.0, TEXT));
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
                        ui.label(mono(
                            now.format("%b %d, %Y  %-I:%M %p").to_string(),
                            12.0,
                            DIM,
                        ));
                    });
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let tabs: [(&str, bool); 10] = [
                        ("Map", true),
                        (
                            "Layers",
                            self.dock.left_open && !models_open && !options_open,
                        ),
                        ("Models", models_open),
                        ("Options", options_open),
                        ("Inspector", self.dock.right_open),
                        ("Playback", self.dock.timeline_open),
                        ("Discussion", false),
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
                                    .min_size(egui::vec2(68.0, 24.0)),
                            )
                            .named(name);
                        if b.clicked() {
                            match name {
                                "Models" => {
                                    // Straight to the model controls: open the left panel on its Models
                                    // tab, or close it if that is what it already shows.
                                    if models_open {
                                        self.dock.left_open = false;
                                    } else {
                                        self.dock.tab = DockTab::Models;
                                        self.dock.left_open = true;
                                        if single_sidebar(ui.ctx().content_rect().width()) {
                                            self.dock.right_open = false;
                                        }
                                    }
                                }
                                "Options" => {
                                    // The settings of the layers that are on — where the ensemble
                                    // and the comparison modes are configured.
                                    if options_open {
                                        self.dock.left_open = false;
                                    } else {
                                        self.dock.tab = DockTab::Options;
                                        self.dock.left_open = true;
                                        if single_sidebar(ui.ctx().content_rect().width()) {
                                            self.dock.right_open = false;
                                        }
                                    }
                                }
                                "Layers" => {
                                    // From the Models or Options view this goes back to the layer
                                    // list rather than closing the panel.
                                    if models_open || options_open {
                                        self.dock.tab = DockTab::Layers;
                                        self.dock.left_open = true;
                                    } else {
                                        self.dock.left_open = !self.dock.left_open;
                                    }
                                    if self.dock.left_open
                                        && single_sidebar(ui.ctx().content_rect().width())
                                    {
                                        self.dock.right_open = false;
                                    }
                                }
                                "Inspector" => {
                                    self.dock.right_open = !self.dock.right_open;
                                    if self.dock.right_open
                                        && single_sidebar(ui.ctx().content_rect().width())
                                    {
                                        self.dock.left_open = false;
                                    }
                                }
                                "Playback" => self.dock.timeline_open = !self.dock.timeline_open,
                                // The forecast *discussion* (AFD); model forecasts live under Models.
                                "Discussion" => action = Some(A::OpenWindow(AppWindow::Afd)),
                                "Tools" => action = Some(A::OpenWindow(AppWindow::LayerManager)),
                                "Settings" => action = Some(A::OpenWindow(AppWindow::Settings)),
                                "Help" => action = Some(A::OpenWindow(AppWindow::Help)),
                                _ => {}
                            }
                        }
                    }
                    ui.separator();
                    let map_3d = self.views[self.active].map_3d.enabled;
                    for (label, on, want_3d) in [("2D", !map_3d, false), ("3D map", map_3d, true)] {
                        if ui
                            .add(
                                egui::Button::new(mono(label, 12.0, TEXT))
                                    .fill(if on { SELECT } else { Color32::TRANSPARENT })
                                    .stroke(Stroke::new(1.0, if on { TAB_ON } else { BORDER }))
                                    .corner_radius(2.0)
                                    .min_size(egui::vec2(54.0, 24.0)),
                            )
                            .named_toggle(label, on)
                            .on_hover_text("Switch the live map between plan and tilted 3D view")
                            .clicked()
                        {
                            self.views[self.active].set_map_3d(want_3d);
                        }
                    }
                    if ui
                        .add(
                            egui::Button::new(mono("Volume", 12.0, TEXT))
                                .stroke(Stroke::new(1.0, BORDER))
                                .corner_radius(2.0)
                                .min_size(egui::vec2(58.0, 24.0)),
                        )
                        .on_hover_text("Open the standalone 3D volume explorer")
                        .clicked()
                    {
                        action = Some(A::OpenWindow(AppWindow::Volume3d));
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
        // The model browser lives at the top of the Models tab, so the dock has the same model,
        // product, run and lead controls as every other layout.
        let model_input = self.model_panel_input();
        let model_on = self.views[self.active].fields_on.clone();
        let model_tz = self.active_tz();
        let mut model_actions = crate::ui::layer_options::UiActions::default();
        egui::Panel::left("dock_layers")
            .exact_size(LEFT_WIDTH)
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
                // Options are settings, not rows, so there is nothing for a search to filter.
                if self.dock.tab != DockTab::Options {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.dock.query)
                            .hint_text("Search layers (e.g. reflectivity, HRRR...)")
                            .desired_width(f32::INFINITY),
                    );
                }
                ui.add_space(4.0);
                let list_h = (ui.available_height() - 36.0).max(80.0);
                egui::ScrollArea::vertical()
                    .max_height(list_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.dock.tab == DockTab::Options {
                            ui.scope(|ui| {
                                dock_style(ui);
                                self.layer_options_body(ui, &mut model_actions);
                            });
                            return;
                        }
                        if self.dock.tab == DockTab::Models {
                            ui.scope(|ui| {
                                dock_style(ui);
                                crate::ui::model_panel::show(
                                    ui,
                                    &model_input,
                                    &model_on,
                                    model_tz,
                                    &mut self.env_cape_ml,
                                    &mut self.env_srh_km,
                                    &mut self.fields,
                                    &mut model_actions,
                                );
                            });
                            ui.separator();
                        }
                        if groups.is_empty() {
                            ui.label(mono("Nothing matches.", 12.0, DIM));
                        }
                        // While searching, every category left standing already has a match in
                        // it (`group_entries` drops the rest), so it opens regardless of whatever
                        // collapsed state the user left it in — a match hidden behind a closed
                        // header would need a second search just to see the row it found. An
                        // empty search leaves each category's own open/closed state alone.
                        let searching = !self.dock.query.is_empty();
                        for g in &groups {
                            let name = crate::ui::layers_panel::category_name(g.category);
                            let mut header = egui::CollapsingHeader::new(mono(
                                format!("{name}  ({}/{})", g.on, g.rows.len()),
                                13.0,
                                TEXT,
                            ))
                            .id_salt(("dock_group", g.category))
                            .default_open(matches!(g.category, "Radar" | "Models"));
                            if searching {
                                header = header.open(Some(true));
                            }
                            header.show(ui, |ui| {
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
                        .add(
                            egui::Button::new(mono("Import layer", 12.0, TEXT))
                                .min_size(egui::vec2(118.0, 26.0)),
                        )
                        .on_hover_text("Import a GeoJSON or Shapefile as an overlay")
                        .clicked()
                    {
                        chosen = Some(crate::app::PaletteAction::ImportGis);
                    }
                    if ui
                        .add(
                            egui::Button::new(mono("Manage layers", 12.0, TEXT))
                                .min_size(egui::vec2(118.0, 26.0)),
                        )
                        .clicked()
                    {
                        open_manager = true;
                    }
                });
            });
        // The model controls and the layer options both report through one actions struct.
        let model_palette = model_actions.palette.take();
        if chosen.is_none() {
            chosen = model_palette;
        }
        self.apply_ui_actions(model_actions, ctx);
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
            let age = v
                .timeline
                .newest()
                .and_then(|id| id.date_time())
                .map(|t| humanize((chrono::Utc::now() - t).num_seconds().max(0)));
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
        // The model forecast card appears only while something from the models is on the map.
        let model_shown = crate::model_browser::model_layers()
            .any(|layer| self.views[self.active].fields_on.contains(&layer));
        let model_rows = model_card_rows(&self.model_panel_input(), tz, chrono::Utc::now());
        let mut model_action = None;
        let mut open_models = false;
        egui::Panel::right("dock_right")
            .exact_size(RIGHT_WIDTH)
            .resizable(false)
            .frame(frame())
            .show(root, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.dock.quick_open {
                            if title_bar(ui, "Quick Controls") {
                                self.dock.quick_open = false;
                            }
                            ui.horizontal(|ui| {
                                ui.label(mono("Map style", 12.0, DIM));
                                if ui
                                    .add(
                                        egui::Button::new(mono(&basemap, 12.0, TEXT))
                                            .min_size(egui::vec2(120.0, 22.0)),
                                    )
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
                                .add(
                                    egui::Button::new(mono("Layers", 12.0, TEXT))
                                        .min_size(egui::vec2(100.0, 24.0)),
                                )
                                .clicked()
                            {
                                show_layers = true;
                            }
                            ui.add_space(6.0);
                        }
                        if model_shown && self.dock.model_open {
                            if title_bar(ui, "Model Forecast") {
                                self.dock.model_open = false;
                            }
                            for (k, v) in &model_rows {
                                ui.horizontal(|ui| {
                                    ui.label(mono(format!("{k:<9}"), 11.0, DIM));
                                    ui.label(mono(v, 11.0, TEXT));
                                });
                            }
                            ui.horizontal(|ui| {
                                for (label, hint, step) in [
                                    ("{2039}", "One step earlier", -1i8),
                                    ("{203a}", "One step later", 1),
                                ] {
                                    if ui
                                        .add(
                                            egui::Button::new(mono(label, 13.0, TEXT))
                                                .min_size(egui::vec2(28.0, 24.0)),
                                        )
                                        .on_hover_text(hint)
                                        .clicked()
                                    {
                                        model_action =
                                            Some(crate::app::PaletteAction::StepModelLead(step));
                                    }
                                }
                                if ui
                                    .add(
                                        egui::Button::new(mono("Models", 12.0, TEXT))
                                            .min_size(egui::vec2(72.0, 24.0)),
                                    )
                                    .on_hover_text("Change model, product, run or lead")
                                    .clicked()
                                {
                                    open_models = true;
                                }
                            });
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
        if let Some(action) = model_action {
            self.apply_palette(action, ctx);
        }
        if open_models {
            self.dock.tab = DockTab::Models;
            self.dock.left_open = true;
            if single_sidebar(ctx.content_rect().width()) {
                self.dock.right_open = false;
            }
        }
        if show_layers {
            self.dock.left_open = true;
            if single_sidebar(ctx.content_rect().width()) {
                self.dock.right_open = false;
            }
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

    #[test]
    fn narrow_docks_keep_only_one_sidebar_beside_the_map() {
        assert!(single_sidebar(SINGLE_SIDEBAR_WIDTH - 1.0));
        assert!(!single_sidebar(SINGLE_SIDEBAR_WIDTH));
        assert!(!single_sidebar(1_920.0));
    }

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

    /// Options are the settings of the layers that are on, drawn by their own body; the tab must
    /// not also list registry rows, or every layer would appear twice.
    #[test]
    fn the_options_tab_lists_no_rows_of_its_own() {
        assert!(group_entries(&sample(), DockTab::Options, "").is_empty());
        assert!(DockTab::ALL.contains(&DockTab::Options));
        assert_eq!(DockTab::Options.label(), "Options");
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
    let mut rows = vec![("Lat", format!("{lat:.2}")), ("Lon", format!("{lon:.2}"))];
    if let Some((rlon, rlat)) = radar {
        let (km, bearing) = crate::geo::great_circle([rlon, rlat], [lon, lat]);
        rows.push(("Range", crate::geo::fmt_distance(km, metric, 1)));
        rows.push(("Az", format!("{:.1}\u{b0}", bearing.rem_euclid(360.0))));
    }
    rows
}

impl HookEchoApp {
    /// The cursor readout over the map's top-left corner, where there is a pointer to read. The
    /// tool strip is not here: it is docked beside the map (`dock_tools`).
    pub(crate) fn dock_map_overlay(&mut self, ctx: &egui::Context) {
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
        // Only the readout floats, and only where there is a pointer to read: a tablet has none,
        // so it gets nothing over its map at all. The tools are docked (`dock_tools`).
        let Some(at) = mouse else {
            return;
        };
        egui::Area::new(egui::Id::new("dock_map_overlay"))
            .constrain_to(map_rect)
            .movable(false)
            .interactable(false)
            .anchor(
                egui::Align2::LEFT_TOP,
                egui::vec2(map_rect.left() + 8.0, map_rect.top() + 8.0),
            )
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(Color32::from_rgba_unmultiplied(11, 16, 24, 225))
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        for (k, v) in cursor_readout(at, radar, metric) {
                            ui.label(mono(format!("{k:<6}{v}"), 11.0, TEXT));
                        }
                    });
            });
    }

    /// The map's tool strip, docked as a narrow panel against the map's left edge. Every button
    /// arms a tool through the palette action, so it is the same tool the ribbon and the phone rail
    /// arm. It used to float over the map, where a touch that meant to pan dragged it around and
    /// it sat on top of the data.
    fn dock_tools(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        use crate::app::PaletteAction as A;
        use egui_phosphor::regular as ph;
        if !self.dock.left_open && !self.dock.right_open && !self.dock.timeline_open {
            return;
        }
        let armed = self.tool;
        let mut pick = None;
        egui::Panel::left("dock_tools")
            .exact_size(TOUCH + 16.0)
            .resizable(false)
            .frame(frame().inner_margin(egui::Margin::same(6)))
            .show(root, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let tools = [
                    (MapTool::Interrogate, ph::CURSOR, "Explore the map"),
                    (
                        MapTool::GateInspector,
                        ph::CROSSHAIR,
                        "Inspect a radar gate",
                    ),
                    (MapTool::Measure, ph::RULER, "Measure distance"),
                    (MapTool::CrossSection, ph::CHART_LINE_UP, "Cross-section"),
                    (MapTool::RegionStats, ph::CHART_SCATTER, "Region statistics"),
                    (MapTool::Sounding, ph::THERMOMETER, "Sounding"),
                    (MapTool::Marker, ph::MAP_PIN, "Drop a marker"),
                    (MapTool::AlertZone, ph::WARNING, "Draw a watch zone"),
                    (MapTool::Draw, ph::PENCIL_SIMPLE, "Draw on the map"),
                ];
                egui::ScrollArea::vertical()
                    .auto_shrink([true, false])
                    .show(ui, |ui| {
                        for (tool, glyph, name) in tools {
                            let on = armed == tool;
                            let b = ui
                                .add(
                                    egui::Button::new(RichText::new(glyph).size(19.0).color(TEXT))
                                        .min_size(egui::vec2(TOUCH, TOUCH))
                                        .fill(if on { SELECT } else { Color32::TRANSPARENT })
                                        .stroke(Stroke::new(
                                            1.0,
                                            if on { TAB_ON } else { Color32::TRANSPARENT },
                                        ))
                                        .corner_radius(3.0),
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

    /// The tilt bar under the map: every tilt of the current volume as a finger-sized button, the
    /// one on screen highlighted, and the one the live stream is sweeping right now marked apart
    /// from it with a progress strip. The ribbon has had this row for a while; the dock had no way
    /// to choose a tilt at all short of the layer options, and no way to see what was being swept.
    fn dock_tilts(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        let v = &self.views[self.active];
        if v.volume.is_none() && v.site.is_none() {
            return;
        }
        let tilt = v.tilt;
        let elevations = v
            .volume
            .as_ref()
            .map(|x| x.elevations.clone())
            .unwrap_or_default();
        let cuts = v
            .volume
            .as_ref()
            .map(|x| wxdata::level2::tilt_cuts(&x.scan))
            .unwrap_or_default();
        let streaming = self
            .live_stream
            .as_ref()
            .is_some_and(|(view, _, _, _)| *view == self.active);
        let progress = v.live_progress;
        let indicator = self.settings.live_scan_indicator;
        let sweeping = sweeping_tilt(progress, streaming, indicator);
        let follow_low = v.follow_lowest_cut;
        let mut pick = None;
        let mut all = false;
        let mut follow = false;
        egui::Panel::bottom("dock_tilts")
            .exact_size(TOUCH + 14.0)
            .resizable(false)
            .frame(frame().inner_margin(egui::Margin::symmetric(6, 6)))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(mono("Tilt", 12.0, DIM));
                    if elevations.is_empty() {
                        ui.label(mono("loading\u{2026}", 12.0, DIM));
                        return;
                    }
                    let status = live_status(progress, streaming, indicator);
                    let status_w = if status.is_empty() { 0.0 } else { 250.0 };
                    let room = (ui.available_width() - status_w - 170.0).max(120.0);
                    egui::ScrollArea::horizontal()
                        .id_salt("dock_tilt_scroll")
                        .max_width(room)
                        .auto_shrink([true, false])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                for (i, a) in elevations.iter().enumerate() {
                                    let repeated = cuts
                                        .get(i)
                                        .is_some_and(|c| c.sails_cuts > 0 || c.mrle_cuts > 0);
                                    let label = if repeated {
                                        format!("{a:.1}\u{b0}\u{2022}")
                                    } else {
                                        format!("{a:.1}\u{b0}")
                                    };
                                    let on = i == tilt;
                                    let live = sweeping == Some(i);
                                    let edge = if live {
                                        LIVE
                                    } else if on {
                                        TAB_ON
                                    } else {
                                        BORDER
                                    };
                                    let r = ui
                                        .add(
                                            egui::Button::new(mono(label.clone(), 13.0, TEXT))
                                                .min_size(egui::vec2(58.0, TOUCH))
                                                .fill(if on {
                                                    SELECT
                                                } else {
                                                    Color32::TRANSPARENT
                                                })
                                                .stroke(Stroke::new(
                                                    if live { 2.0 } else { 1.0 },
                                                    edge,
                                                ))
                                                .corner_radius(3.0),
                                        )
                                        .named_toggle(&format!("Tilt {label}"), on);
                                    if live {
                                        if let Some(p) = progress {
                                            super::ribbon::live_sweep_strip(ui, r.rect, p, LIVE);
                                        }
                                    }
                                    let r = if live {
                                        r.on_hover_text("The radar is sweeping this tilt now")
                                    } else if repeated {
                                        r.on_hover_text("Rescanned mid-volume (SAILS/MRLE)")
                                    } else {
                                        r
                                    };
                                    if r.clicked() {
                                        pick = Some(i);
                                    }
                                }
                            });
                        });
                    if ui
                        .add(
                            egui::Button::new(mono("All", 12.0, TEXT))
                                .min_size(egui::vec2(44.0, TOUCH))
                                .stroke(Stroke::new(1.0, BORDER)),
                        )
                        .on_hover_text("Four panes, one product, four tilts, cameras linked")
                        .named("All tilts")
                        .clicked()
                    {
                        all = true;
                    }
                    if ui
                        .add(
                            egui::Button::new(mono("Follow low", 12.0, TEXT))
                                .min_size(egui::vec2(44.0, TOUCH))
                                .fill(if follow_low {
                                    SELECT
                                } else {
                                    Color32::TRANSPARENT
                                })
                                .stroke(Stroke::new(1.0, if follow_low { TAB_ON } else { BORDER })),
                        )
                        .on_hover_text(
                            "While following live, jump to the lowest tilt the instant it is \
                             rescanned (SAILS/MRLE).",
                        )
                        .named_toggle("Follow lowest tilt", follow_low)
                        .clicked()
                    {
                        follow = true;
                    }
                    if !status.is_empty() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(mono(status, 12.0, LIVE));
                        });
                    }
                });
            });
        if let Some(i) = pick {
            self.views[self.active].tilt = i;
        }
        if all {
            self.apply_palette(crate::app::PaletteAction::AllTilts, ctx);
        }
        if follow {
            let f = &mut self.views[self.active].follow_lowest_cut;
            *f = !*f;
        }
    }
}

/// Finger-sized: the smallest square a control on this layout is drawn at.
const TOUCH: f32 = 42.0;

/// Green: "live", apart from the blue that means "selected".
const LIVE: Color32 = Color32::from_rgb(80, 220, 140);

/// Which tilt (an index into the volume's elevations) the live stream is sweeping right now.
/// `None` when nothing is streaming, the indicator is turned off, or no chunk has arrived.
pub(crate) fn sweeping_tilt(
    progress: Option<wxdata::live::ScanProgress>,
    streaming: bool,
    indicator_on: bool,
) -> Option<usize> {
    if !streaming || !indicator_on {
        return None;
    }
    // Sweeps count from 1; zero means the stream has not said, and must not wrap to a real index.
    progress.and_then(|p| p.elevation_number.checked_sub(1))
}

/// The one line that says what the radar is doing, for the tilt bar. Empty when not live.
pub(crate) fn live_status(
    progress: Option<wxdata::live::ScanProgress>,
    streaming: bool,
    indicator_on: bool,
) -> String {
    if !streaming {
        return String::new();
    }
    match progress {
        Some(p) if indicator_on => format!(
            "LIVE \u{b7} sweeping {:.1}\u{b0} ({}/{}) chunk {}/{}",
            p.elevation_angle_deg,
            p.elevation_number,
            p.total_elevations,
            p.chunk_index,
            p.chunks_in_sweep
        ),
        _ => "LIVE".to_string(),
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
        assert!(
            miles.iter().any(|r| r.0 == "Range" && r.1.ends_with(" mi")),
            "{miles:?}"
        );
    }
}

#[cfg(test)]
mod model_card_tests {
    use super::model_card_rows;
    use crate::model_browser::{BModel, Product, Selection};
    use crate::ui::model_panel::Input;
    use chrono::{DateTime, TimeZone, Utc};
    use wxdata::field::{DataStamp, QualitySummary};

    fn input(stamp: Option<DataStamp>, run: Option<DateTime<Utc>>) -> Input {
        Input {
            sel: Selection {
                model: BModel::Hrrr,
                product: Product::Reflectivity,
            },
            lead_min: 180,
            stamp,
            run,
            runs: Vec::new(),
            range: BModel::Hrrr.leads(),
        }
    }

    fn stamp(now: DateTime<Utc>) -> DataStamp {
        DataStamp {
            source_id: "HRRR".into(),
            product_id: "Composite reflectivity".into(),
            issue_time: None,
            run_time: Utc.with_ymd_and_hms(2026, 9, 20, 18, 0, 0).single(),
            valid_time: Utc.with_ymd_and_hms(2026, 9, 20, 21, 0, 0).unwrap(),
            received_time: now - chrono::Duration::minutes(4),
            source_latency: None,
            is_forecast: true,
            is_derived: false,
            quality: QualitySummary::Unknown,
            grid: None,
        }
    }

    fn get<'a>(rows: &'a [(&'static str, String)], key: &str) -> &'a str {
        &rows
            .iter()
            .find(|(k, _)| *k == key)
            .unwrap_or_else(|| panic!("no {key} row in {rows:?}"))
            .1
    }

    #[test]
    fn the_card_names_the_model_run_lead_and_freshness() {
        let now = Utc.with_ymd_and_hms(2026, 9, 20, 21, 30, 0).unwrap();
        let rows = model_card_rows(&input(Some(stamp(now)), None), None, now);
        assert_eq!(get(&rows, "Model:"), "HRRR");
        assert_eq!(get(&rows, "Product:"), "Reflectivity");
        assert_eq!(get(&rows, "Run:"), "20 18Z");
        assert_eq!(get(&rows, "Lead:"), "F+3h");
        assert_eq!(get(&rows, "Fetched:"), "4 min ago");
        assert!(!get(&rows, "Valid:").is_empty());
    }

    #[test]
    fn before_the_data_arrives_it_says_so_instead_of_inventing_a_time() {
        let now = Utc.with_ymd_and_hms(2026, 9, 20, 21, 30, 0).unwrap();
        let rows = model_card_rows(&input(None, None), None, now);
        assert_eq!(get(&rows, "Run:"), "latest");
        assert!(get(&rows, "Valid:").contains("loading"));
        assert!(rows.iter().all(|(k, _)| *k != "Fetched:"));
        // A run the user pinned shows even before its data lands.
        let pinned = Utc.with_ymd_and_hms(2026, 9, 20, 12, 0, 0).single();
        let rows = model_card_rows(&input(None, pinned), None, now);
        assert_eq!(get(&rows, "Run:"), "20 12Z");
    }
}

#[cfg(test)]
mod tilt_bar_tests {
    use super::*;

    fn progress(n: usize) -> wxdata::live::ScanProgress {
        wxdata::live::ScanProgress {
            elevation_number: n,
            total_elevations: 14,
            elevation_angle_deg: 0.9,
            azimuth_rate_dps: 18.0,
            azimuth_start_deg: 0.0,
            azimuth_end_deg: 120.0,
            chunk_index: 2,
            chunks_in_sweep: 3,
        }
    }

    #[test]
    fn the_sweeping_tilt_is_the_streams_sweep_number_as_an_index() {
        assert_eq!(sweeping_tilt(Some(progress(1)), true, true), Some(0));
        assert_eq!(sweeping_tilt(Some(progress(5)), true, true), Some(4));
    }

    #[test]
    fn nothing_is_marked_when_not_streaming_switched_off_or_silent() {
        assert_eq!(sweeping_tilt(Some(progress(3)), false, true), None);
        assert_eq!(sweeping_tilt(Some(progress(3)), true, false), None);
        assert_eq!(sweeping_tilt(None, true, true), None);
        // Sweep zero is "not said yet", never the last tilt of the volume.
        assert_eq!(sweeping_tilt(Some(progress(0)), true, true), None);
    }

    #[test]
    fn the_status_line_names_the_sweep_and_is_empty_when_not_live() {
        assert_eq!(live_status(Some(progress(3)), false, true), "");
        let s = live_status(Some(progress(3)), true, true);
        assert!(
            s.contains("0.9\u{b0}") && s.contains("3/14") && s.contains("chunk 2/3"),
            "{s}"
        );
        // Live but the indicator is off, or no chunk yet: say live, invent no sweep.
        assert_eq!(live_status(Some(progress(3)), true, false), "LIVE");
        assert_eq!(live_status(None, true, true), "LIVE");
    }
}
