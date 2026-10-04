//! The workstation on a phone: the Station phone design.
//!
//! The dock's windows, laid out for one hand. Top to bottom: an app bar (the name, search, the
//! alerts bell and a menu holding every menu the desktop app bar has), a row of big Site / Product
//! / Tilt fields with the 2D / 3D switch and the Layers button, the map with a tool rail down its
//! left edge and the colour scale down its right, a bottom sheet whose tabs are the workstation's
//! windows (Inspector, Layers, Storms, Alerts, then whatever else is open: storm details, the
//! sounding, the gauges, preferences), and the timeline under everything.
//!
//! It is the dock's own windows, not copies: the sheet draws the front one through
//! [`HookEchoApp::dock_window`], the same code a desktop dock runs, so everything the workstation
//! can do the phone can. What cannot be reached from the bars is in the Layers tab, which is the
//! whole command registry and searches it, and in the menu behind the gear, which lists every
//! window (`menus::window_home` is exhaustive).

use super::app_bar::{pane_items, share_rows, table_items, Follow};
use super::menus::{window_rows, Menu, MenuPick};
use super::*;
use crate::ui::a11y::Named as _;
use egui::{Color32, FontId, Rect, Sense, Stroke};
use egui_phosphor::regular as ph;
use wxdata::level2::Moment;

/// App bar height.
const BAR_H: f32 = 54.0;
/// The Site / Product / Tilt row's height.
const ROW_H: f32 = 62.0;
/// The sheet's drag handle strip.
const HANDLE_H: f32 = 18.0;
/// A tool rail button's edge.
const RAIL_BTN: f32 = 46.0;
/// The map keeps at least this much height however far the sheet is pulled up.
const MAP_MIN_H: f32 = 96.0;
/// The least body a sheet needs above its tabs to draw its window rather than fold.
const UNFOLD_MIN: f32 = 80.0;

/// What a rail button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rail {
    Locate,
    Center,
    Display,
    Measure,
    Tools,
    FullScreen,
}

/// The rail, top to bottom. When the sheet leaves too little map for all of them, the ones at
/// the end are left off (each is also in a menu).
const RAIL: [(Rail, &str, &str); 6] = [
    (Rail::Locate, ph::NAVIGATION_ARROW, "Center on my location"),
    (Rail::Center, ph::CROSSHAIR, "Center on the radar"),
    (
        Rail::Display,
        ph::STACK_SIMPLE,
        "Display: smoothing, colours, overlays, map",
    ),
    (Rail::Measure, ph::RULER, "Measure distance"),
    (Rail::Tools, ph::WRENCH, "Map tools"),
    (Rail::FullScreen, ph::ARROWS_OUT, "Full-screen map"),
];

/// How many rail buttons fit down a map this tall.
fn rail_fit(map_h: f32) -> usize {
    (((map_h - 12.0) / (RAIL_BTN + 8.0)).floor().max(0.0) as usize).min(RAIL.len())
}

/// The sheet's heights for the room between the control row and the timeline: folded to its
/// tabs, about two fifths of the room, or all but a strip of map.
fn sheet_heights(room: f32, peek: f32) -> [f32; 3] {
    let full = (room - MAP_MIN_H).max(peek);
    let half = (room * 0.42).clamp(peek, full);
    [peek, half, full]
}

/// The sheet's height and whether it is folded to its tabs, given the height its snap and drag
/// ask for. Below `peek + UNFOLD_MIN` there is no room for a window's body, so it folds, and a
/// folded sheet does not draw the window. While a text field has focus that would be fatal: the
/// soft keyboard takes a third of a phone's height, the room shrinks, the half-height sheet drops
/// under the line and folds, the field it held stops being drawn, egui drops its focus, and the
/// app (which shows the keyboard for exactly as long as a field is focused) hides the keyboard
/// again the instant it opened. So while typing the sheet keeps at least an unfolded height,
/// even past the room it was given: with the keyboard up a phone can have almost no room left,
/// and a field kept focused but drawn a few points tall cannot be read. The sheet then rides
/// over the map and the control row until the keyboard goes. A tablet's room never got that
/// small.
fn sheet_fit(h: f32, peek: f32, typing: bool) -> (f32, bool) {
    let unfolded = peek + UNFOLD_MIN;
    if typing {
        (h.max(unfolded), false)
    } else {
        (h, h < unfolded)
    }
}

/// The snap nearest a dragged height.
fn nearest_snap(h: f32, heights: [f32; 3]) -> Sheet {
    let snaps = [Sheet::Peek, Sheet::Half, Sheet::Full];
    let mut best = 0;
    for i in 1..3 {
        if (heights[i] - h).abs() < (heights[best] - h).abs() {
            best = i;
        }
    }
    snaps[best]
}

/// A field in the control row: its name small above, its value large below, a caret at the end.
fn field(ui: &mut egui::Ui, t: &ws::Tokens, label: &str, value: &str, w: f32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, ROW_H - 14.0), Sense::click());
    let hot = resp.hovered() || resp.is_pointer_button_down_on();
    ui.painter().rect(
        r,
        8.0,
        if hot { t.field_hi } else { t.field },
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
    let p = ui.painter_at(r.shrink(1.0));
    p.text(
        r.left_top() + egui::vec2(10.0, 7.0),
        egui::Align2::LEFT_TOP,
        label,
        FontId::proportional(11.5),
        t.text_dim,
    );
    p.text(
        r.right_center() + egui::vec2(-9.0, 7.0),
        egui::Align2::RIGHT_CENTER,
        ph::CARET_DOWN,
        FontId::proportional(13.0),
        t.text,
    );
    // The value, cut to what fits before the caret.
    let room = r.width() - 34.0;
    let font = FontId::proportional(16.0);
    let wide = |s: &str| {
        p.layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE)
            .size()
            .x
            > room
    };
    let mut shown = value.to_string();
    if wide(&shown) {
        let mut base: Vec<char> = value.chars().collect();
        while base.len() > 1 {
            base.pop();
            shown = base.iter().collect::<String>() + "\u{2026}";
            if !wide(&shown) {
                break;
            }
        }
    }
    p.text(
        r.left_bottom() + egui::vec2(10.0, -8.0),
        egui::Align2::LEFT_BOTTOM,
        shown,
        font,
        Color32::WHITE,
    );
    resp.named(&format!("{label}: {value}"))
}

/// An app-bar icon: a large glyph with a round press state.
fn bar_icon(ui: &mut egui::Ui, t: &ws::Tokens, glyph: &str, on: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), Sense::click());
    if on || resp.hovered() || resp.is_pointer_button_down_on() {
        ui.painter().circle_filled(
            r.center(),
            20.0,
            if on { t.accent_soft() } else { t.field_hi },
        );
    }
    ui.painter().text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(24.0),
        if on { t.accent } else { t.text },
    );
    resp
}

/// A rail button over the map: a rounded square on the panel colour, the accent when armed.
fn rail_button(ui: &mut egui::Ui, t: &ws::Tokens, glyph: &str, on: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(RAIL_BTN, RAIL_BTN), Sense::click());
    let fill = if on {
        t.accent
    } else if resp.hovered() || resp.is_pointer_button_down_on() {
        t.field_hi
    } else {
        t.panel.gamma_multiply(0.94)
    };
    ui.painter().rect(
        r,
        10.0,
        fill,
        Stroke::new(1.0, if on { t.accent } else { t.line }),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(22.0),
        if on { Color32::WHITE } else { t.text },
    );
    resp
}

impl HookEchoApp {
    /// Whether this frame draws the Station phone chrome: on a phone, with that design picked.
    pub(crate) fn phone_station(&self) -> bool {
        crate::platform::phone_layout()
            && self.settings.phone_design == crate::settings::PhoneDesign::Station
    }

    /// The phone's docked parts, drawn before the map's rect is read (as [`Self::dock_layout`] is
    /// on a desktop): the app bar and control row across the top, the timeline across the bottom
    /// and the sheet above it. A full-screen map draws none of them.
    pub(crate) fn phone_layout(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        self.dock.phone = true;
        ws::set_touch(ctx, true);
        // Every window is a tab of the one sheet: none floats or folds here. (The arrangement a
        // desktop saved is not touched: the phone never writes it back.)
        for w in DockWin::ALL {
            let c = self.dock.chrome_mut(w);
            c.place = Place::Right;
            c.collapsed = false;
        }
        for w in PHONE_CORE {
            self.dock.chrome_mut(w).open = true;
        }
        self.dock.timeline_open = true;
        self.dock_sync_bulletin();
        self.dock_sync_available();
        if self.mobile_chrome_hidden {
            return;
        }
        self.phone_app_bar(root, ctx);
        self.phone_controls(root, ctx);
        // The timeline first, so it is the lowest of the bottom panels, under the sheet.
        self.dock_timeline(root, true);
        self.phone_sheet(root, ctx);
    }

    /// Over the map: the tool rail.
    pub(crate) fn phone_overlay(&mut self, ctx: &egui::Context) {
        if self.mobile_chrome_hidden {
            return;
        }
        let t = self.ws_tokens();
        let map = self.chrome_rect;
        let n = rail_fit(map.height());
        if n == 0 {
            return;
        }
        let armed = self.tool;
        let has_site = self.views[self.active].site.is_some();
        let mut hit = None;
        let mut tool_pick = None;
        egui::Area::new(egui::Id::new("phone_rail"))
            .order(egui::Order::Middle)
            .fixed_pos(map.left_top() + egui::vec2(10.0, 10.0))
            .show(ctx, |ui| {
                ws::style_scope(ui, &t);
                ui.spacing_mut().item_spacing.y = 8.0;
                for &(what, glyph, name) in RAIL.iter().take(n) {
                    let on = match what {
                        Rail::Measure => armed == MapTool::Measure,
                        Rail::Tools => !matches!(armed, MapTool::Interrogate | MapTool::Measure),
                        _ => false,
                    };
                    // The tools button wears the armed tool's glyph, so the rail says what a tap
                    // on the map will do.
                    let glyph = if what == Rail::Tools && on {
                        super::rail::GROUPS
                            .iter()
                            .flat_map(|g| g.iter())
                            .find(|(tool, ..)| *tool == armed)
                            .map_or(glyph, |(_, g, _)| *g)
                    } else {
                        glyph
                    };
                    let resp = ui
                        .add_enabled_ui(what != Rail::Center || has_site, |ui| {
                            rail_button(ui, &t, glyph, on)
                        })
                        .inner
                        .named_toggle(name, on);
                    match what {
                        Rail::Display => {
                            egui::Popup::menu(&resp)
                                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                .show(|ui| {
                                    ws::menu_scope(ui, &t);
                                    ui.set_min_width(250.0);
                                    ui.spacing_mut().item_spacing.y = 6.0;
                                    ui.spacing_mut().button_padding = egui::vec2(10.0, 7.0);
                                    egui::ScrollArea::vertical()
                                        .max_height(map.height().max(260.0))
                                        .show(ui, |ui| self.phone_display_items(ui, &t, ctx));
                                });
                        }
                        Rail::Tools => {
                            egui::Popup::menu(&resp).show(|ui| {
                                ws::menu_scope(ui, &t);
                                ui.set_min_width(240.0);
                                ui.style_mut().spacing.button_padding = egui::vec2(10.0, 9.0);
                                for (gi, group) in super::rail::GROUPS.iter().enumerate() {
                                    if gi > 0 {
                                        ui.separator();
                                    }
                                    for &(tool, g, name) in group.iter() {
                                        if ui
                                            .selectable_label(
                                                armed == tool,
                                                format!("{g}   {name}"),
                                            )
                                            .clicked()
                                        {
                                            tool_pick = Some(tool);
                                        }
                                    }
                                }
                            });
                        }
                        _ => {
                            if resp.clicked() {
                                hit = Some(what);
                            }
                        }
                    }
                }
            });
        match hit {
            Some(Rail::Locate) => self.locate_me(),
            Some(Rail::Center) => self.dock_center_on_radar(),
            Some(Rail::Measure) => {
                let tool = if armed == MapTool::Measure {
                    MapTool::Interrogate
                } else {
                    MapTool::Measure
                };
                self.apply_palette(crate::app::PaletteAction::Tool(tool), ctx);
            }
            Some(Rail::FullScreen) => {
                self.mobile_chrome_hidden = true;
            }
            _ => {}
        }
        if let Some(tool) = tool_pick {
            self.apply_palette(crate::app::PaletteAction::Tool(tool), ctx);
        }
    }

    /// The Display menu: what the desktop toolbar carries past Site, Product and Tilt.
    fn phone_display_items(&mut self, ui: &mut egui::Ui, t: &ws::Tokens, ctx: &egui::Context) {
        use crate::app::{OverlayToggle as T, PaletteAction as A};
        let mut action = None;
        let v = &mut self.views[self.active];
        ui.label(ws::text("VIEW", 10.5, t.text_faint));
        ws::check(ui, t, &mut v.smooth, "Smoothing");
        ws::check(ui, t, &mut v.show_legend, "Colour scale");
        let follow = Follow::of(v.follow_lowest_cut, v.follow_live_sweep);
        ui.label(ws::text("FOLLOW WHILE LIVE", 10.5, t.text_faint));
        ui.horizontal(|ui| {
            for f in Follow::ALL {
                if ui
                    .selectable_label(f == follow, f.label())
                    .on_hover_text(f.hint())
                    .clicked()
                {
                    v.follow_lowest_cut = f == Follow::Lowest;
                    v.follow_live_sweep = f == Follow::Sweep;
                    v.followed_sweep = None;
                }
            }
        });
        let moment = v.moment;
        let key = moment.short_name();
        let table_now = self
            .settings
            .palettes
            .get(key)
            .map(|s| super::app_bar::table_label(s))
            .unwrap_or_else(|| "Default".to_string());
        let mut table_pick = None;
        ui.menu_button(format!("Colour table: {table_now}"), |ui| {
            ws::menu_scope(ui, t);
            table_items(ui, moment, &table_now, &mut table_pick);
        });
        match table_pick {
            Some(Some(v)) => {
                self.settings.palettes.insert(key.to_string(), v);
            }
            Some(None) => {
                self.settings.palettes.remove(key);
            }
            None => {}
        }
        ui.separator();
        ui.label(ws::text("OVERLAYS", 10.5, t.text_faint));
        for (tg, name) in [
            (T::RangeRings, "Range rings"),
            (T::Alerts, "Warnings"),
            (T::Tracks, "Storm tracks"),
            (T::GlmLightning, "Lightning"),
            (T::Gauges, "River gauges"),
        ] {
            let mut on = *self.overlay_flag(tg);
            if ws::check(ui, t, &mut on, name).changed() {
                action = Some(A::ToggleOverlay(tg));
            }
        }
        ui.separator();
        ui.label(ws::text("MAP", 10.5, t.text_faint));
        let basemap = self.views[self.active].basemap.label();
        if ui.button(format!("Map style: {basemap}\u{2026}")).clicked() {
            self.basemap_open = true;
            ui.close();
        }
        let panes = self.views.len();
        let links: Vec<(T, bool)> = T::PANE_LINKS
            .into_iter()
            .map(|t| (t, *self.overlay_flag(t)))
            .collect();
        let pane_layout = self.pane_layout;
        ui.menu_button(
            format!("Panes: {panes} \u{b7} {}", pane_layout.label()),
            |ui| {
                ws::menu_scope(ui, t);
                pane_items(ui, panes, pane_layout, &links, &mut action);
            },
        );
        if ui.button("3D volume explorer\u{2026}").clicked() {
            action = Some(A::OpenWindow(AppWindow::Volume3d));
        }
        if let Some(a) = action {
            self.apply_palette(a, ctx);
        }
    }

    /// Carry out a pick from the gear menu.
    fn phone_menu_pick(&mut self, pick: MenuPick, ctx: &egui::Context) {
        match pick {
            MenuPick::Palette(a) => self.apply_palette(a, ctx),
            MenuPick::Prefs(page, section) => {
                self.dock.prefs_page = page;
                self.dock.bring_forward(DockWin::Prefs);
                ctx.data_mut(|d| {
                    let id = egui::Id::new("preferences_section");
                    match section {
                        Some(s) => {
                            d.insert_temp(id, s);
                        }
                        None => d.remove::<&'static str>(id),
                    }
                });
            }
            MenuPick::Shortcuts => self.show_cheatsheet = true,
        }
    }

    /// The name, then search, the alerts bell and the menu.
    fn phone_app_bar(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        let t = self.ws_tokens();
        let (alerts, _) = self.alert_badge();
        let alerts_on = self.dock.shown(DockWin::Alerts);
        let workspaces: Vec<String> = self
            .settings
            .workspaces
            .iter()
            .map(|w| w.name.clone())
            .collect();
        let menu_h = ctx.content_rect().height() * 0.7;
        let mut pick = None;
        let mut search = false;
        let mut bell = false;
        egui::Panel::top("phone_app_bar")
            .exact_size(BAR_H)
            .frame(
                egui::Frame::NONE
                    .fill(t.bg)
                    .inner_margin(egui::Margin::symmetric(12, 0)),
            )
            .show(root, |ui| {
                ws::style_scope(ui, &t);
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(ws::text(ph::BROADCAST, 28.0, t.accent));
                    ui.add_space(8.0);
                    ui.label(ws::text("Hook", 22.0, Color32::WHITE).strong());
                    ui.label(ws::text("Echo", 22.0, t.accent).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let gear = bar_icon(ui, &t, ph::GEAR_SIX, false)
                            .named("Settings, tools, share and help");
                        egui::Popup::menu(&gear).show(|ui| {
                            ws::menu_scope(ui, &t);
                            ui.set_min_width(260.0);
                            ui.style_mut().spacing.button_padding = egui::vec2(10.0, 8.0);
                            egui::ScrollArea::vertical()
                                .max_height(menu_h)
                                .show(ui, |ui| {
                                    let head = |ui: &mut egui::Ui, s: &str| {
                                        ui.label(ws::text(s, 10.5, t.text_faint));
                                    };
                                    head(ui, "SETTINGS");
                                    if ui.button("Preferences").clicked() {
                                        pick = Some(MenuPick::Prefs(PrefsPage::App, None));
                                    }
                                    if ui.button("Map settings").clicked() {
                                        pick = Some(MenuPick::Prefs(PrefsPage::Map, None));
                                    }
                                    if let Some(p) = window_rows(ui, &t, Menu::Settings) {
                                        pick = Some(p);
                                    }
                                    ui.separator();
                                    if let Some(p) = window_rows(ui, &t, Menu::Tools) {
                                        pick = Some(p);
                                    }
                                    ui.separator();
                                    head(ui, "FORECASTS");
                                    if let Some(p) = window_rows(ui, &t, Menu::Discussion) {
                                        pick = Some(p);
                                    }
                                    ui.separator();
                                    head(ui, "SHARE");
                                    if let Some(p) = share_rows(ui, &t, &workspaces) {
                                        pick = Some(p);
                                    }
                                    ui.separator();
                                    head(ui, "HELP");
                                    if let Some(p) = window_rows(ui, &t, Menu::Help) {
                                        pick = Some(p);
                                    }
                                });
                        });
                        let b = bar_icon(ui, &t, ph::BELL, alerts_on)
                            .named_toggle(&format!("Alerts in view: {alerts}"), alerts_on);
                        if alerts > 0 {
                            let at = b.rect.center() + egui::vec2(9.0, -10.0);
                            let text = if alerts > 99 {
                                "99+".to_string()
                            } else {
                                alerts.to_string()
                            };
                            let g = ui.painter().layout_no_wrap(
                                text,
                                FontId::proportional(10.5),
                                Color32::WHITE,
                            );
                            let r = Rect::from_center_size(
                                at,
                                egui::vec2((g.size().x + 8.0).max(16.0), 16.0),
                            );
                            ui.painter().rect_filled(r, 8.0, t.danger);
                            ui.painter()
                                .galley(r.center() - g.size() / 2.0, g, Color32::WHITE);
                        }
                        bell = b.clicked();
                        search = bar_icon(ui, &t, ph::MAGNIFYING_GLASS, false)
                            .named("Search layers, sites, places and tools")
                            .clicked();
                    });
                });
            });
        if search {
            // A search wants the room to show its results.
            self.dock.open_search();
            self.dock.bring_forward(DockWin::Layers);
            self.dock.sheet = Sheet::Full;
        }
        if bell {
            self.dock.toggle(DockWin::Alerts);
        }
        if let Some(p) = pick {
            self.phone_menu_pick(p, ctx);
        }
    }

    /// Site, Product and Tilt as big fields, the 2D / 3D switch, and the Layers button.
    fn phone_controls(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        use crate::app::PaletteAction as A;
        let t = self.ws_tokens();
        let streaming = self.live_session.streaming_for(self.active);
        let (site, moment, srv, tilt, elevations, map_3d, progress) = {
            let v = &self.views[self.active];
            (
                v.site.clone(),
                v.moment,
                v.srv,
                v.tilt,
                v.volume
                    .as_ref()
                    .map(|x| x.elevations.clone())
                    .unwrap_or_default(),
                v.map_3d.enabled,
                v.live_progress,
            )
        };
        let sweeping = sweeping_tilt(
            progress,
            &elevations,
            streaming,
            self.settings.live_scan_indicator,
        );
        let layers_on = self.dock.shown(DockWin::Layers);
        let mut action = None;
        let mut open_sites = false;
        let mut pick_tilt = None;
        let mut want_3d = None;
        let mut layers = false;
        egui::Panel::top("phone_controls")
            .exact_size(ROW_H)
            .frame(
                egui::Frame::NONE
                    .fill(t.bg)
                    .stroke(Stroke::new(1.0, t.line_soft))
                    .inner_margin(egui::Margin {
                        left: 10,
                        right: 10,
                        top: 4,
                        bottom: 10,
                    }),
            )
            .show(root, |ui| {
                ws::style_scope(ui, &t);
                let gap = 6.0;
                let (site_w, tilt_w, seg_w, lay_w) = (76.0, 66.0, 88.0, 46.0);
                let prod_w =
                    (ui.available_width() - site_w - tilt_w - seg_w - lay_w - 4.0 * gap).max(84.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    if field(ui, &t, "Site", site.as_deref().unwrap_or("None"), site_w).clicked() {
                        open_sites = true;
                    }
                    let prod = field(
                        ui,
                        &t,
                        "Product",
                        crate::products::name(moment, srv),
                        prod_w,
                    );
                    egui::Popup::menu(&prod).show(|ui| {
                        ws::menu_scope(ui, &t);
                        ui.set_min_width(220.0);
                        ui.style_mut().spacing.button_padding = egui::vec2(10.0, 8.0);
                        for m in Moment::ALL {
                            let on = m == moment && !(srv && m == Moment::Velocity);
                            if ui
                                .selectable_label(on, crate::products::info(m).name)
                                .clicked()
                            {
                                action = Some(A::SetMoment(m, false));
                            }
                            if m == Moment::Velocity
                                && ui
                                    .selectable_label(
                                        moment == m && srv,
                                        crate::products::name(m, true),
                                    )
                                    .clicked()
                            {
                                action = Some(A::SetMoment(m, true));
                            }
                        }
                    });
                    let tilt_text = elevations
                        .get(tilt)
                        .map_or_else(|| "\u{2014}".to_string(), |a| format!("{a:.1}\u{b0}"));
                    let tr = field(ui, &t, "Tilt", &tilt_text, tilt_w);
                    egui::Popup::menu(&tr).show(|ui| {
                        ws::menu_scope(ui, &t);
                        ui.set_min_width(160.0);
                        ui.style_mut().spacing.button_padding = egui::vec2(10.0, 8.0);
                        for (i, a) in elevations.iter().enumerate() {
                            let mut label = format!("{a:.1}\u{b0}");
                            if sweeping == Some(i) {
                                label.push_str("  \u{25cf} live");
                            }
                            if ui.selectable_label(i == tilt, label).clicked() {
                                pick_tilt = Some(i);
                            }
                        }
                        ui.separator();
                        if ui
                            .selectable_label(false, "All tilts (four panes)")
                            .clicked()
                        {
                            action = Some(A::AllTilts);
                        }
                    });
                    // 2D / 3D: two halves of one control, the chosen one in the accent.
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(seg_w, ROW_H - 14.0), Sense::hover());
                    ui.painter().rect(
                        r,
                        8.0,
                        t.field,
                        Stroke::new(1.0, t.line),
                        egui::StrokeKind::Inside,
                    );
                    for (i, label) in ["2D", "3D"].into_iter().enumerate() {
                        let half = Rect::from_min_size(
                            egui::pos2(r.left() + r.width() / 2.0 * i as f32, r.top()),
                            egui::vec2(r.width() / 2.0, r.height()),
                        );
                        let on = (i == 1) == map_3d;
                        let resp = ui
                            .interact(half, ui.id().with(("phone_dim", i)), Sense::click())
                            .named_toggle(label, on);
                        if on {
                            ui.painter().rect_filled(half.shrink(3.0), 6.0, t.accent);
                        }
                        ui.painter().text(
                            half.center(),
                            egui::Align2::CENTER_CENTER,
                            label,
                            FontId::proportional(16.0),
                            if on { Color32::WHITE } else { t.text },
                        );
                        if resp.clicked() && !on {
                            want_3d = Some(i == 1);
                        }
                    }
                    let (r, resp) =
                        ui.allocate_exact_size(egui::vec2(lay_w, ROW_H - 14.0), Sense::click());
                    ui.painter().rect(
                        r,
                        8.0,
                        if layers_on { t.accent_soft() } else { t.field },
                        Stroke::new(1.0, if layers_on { t.accent } else { t.line }),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        r.center(),
                        egui::Align2::CENTER_CENTER,
                        ph::STACK,
                        FontId::proportional(24.0),
                        t.accent,
                    );
                    layers = resp.named_toggle("Layers", layers_on).clicked();
                });
            });
        if open_sites {
            self.site_dialog = Some(Default::default());
        }
        if let Some(i) = pick_tilt {
            self.views[self.active].tilt = i;
        }
        if let Some(on) = want_3d {
            self.views[self.active].set_map_3d(on);
        }
        if layers {
            self.dock.toggle(DockWin::Layers);
        }
        if let Some(a) = action {
            self.apply_palette(a, ctx);
        }
    }

    /// The bottom sheet: a handle to drag it between its heights, then the front window with
    /// every open window as a tab in its header.
    fn phone_sheet(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        let t = self.ws_tokens();
        let room = root.available_rect_before_wrap().height();
        let peek = HANDLE_H + ws::header_h(ctx) + 2.0;
        let heights = sheet_heights(room, peek);
        let base = heights[self.dock.sheet as usize];
        let drag_id = egui::Id::new("phone_sheet_drag");
        let drag: f32 = ctx.data(|d| d.get_temp(drag_id)).unwrap_or(0.0);
        let (h, folded) = sheet_fit(
            (base + drag).clamp(peek, heights[2]),
            peek,
            ctx.text_edit_focused(),
        );
        let stack = self.dock.phone_stack();
        let Some(front) = self.dock.phone_front.or(stack.first().copied()) else {
            return;
        };
        let mut tabs = ws::HeaderTabs {
            tabs: stack.iter().map(|w| w.tab()).collect(),
            front: stack.iter().position(|w| *w == front).unwrap_or(0),
        };
        for (w, tab) in stack.iter().zip(&mut tabs.tabs) {
            tab.dot = match w {
                DockWin::Alerts if self.alert_badge().0 > 0 => Some(t.warn),
                DockWin::Sources if self.sources_attention() > 0 => Some(t.danger),
                DockWin::Gauges if self.gauges_in_flood() > 0 => Some(t.warn),
                _ => None,
            };
        }
        let mut snap = None;
        let mut new_drag = drag;
        // Too short for a window's body (folded, or being pulled up from folded): the tabs alone.
        // Never while a field in it has the keyboard (`sheet_fit`).
        let mut header = ws::HeaderAction::None;
        egui::Panel::bottom("phone_sheet")
            .exact_size(h)
            .resizable(false)
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .stroke(Stroke::new(1.0, t.line))
                    .corner_radius(egui::CornerRadius {
                        nw: 16,
                        ne: 16,
                        sw: 0,
                        se: 0,
                    }),
            )
            .show(root, |ui| {
                ws::style_scope(ui, &t);
                ui.spacing_mut().item_spacing.y = 0.0;
                let (hr, handle) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), HANDLE_H),
                    Sense::click_and_drag(),
                );
                let pill = Rect::from_center_size(hr.center(), egui::vec2(40.0, 5.0));
                ui.painter().rect_filled(
                    pill,
                    3.0,
                    if handle.dragged() {
                        t.text
                    } else {
                        t.text_faint
                    },
                );
                let handle = handle.named(match self.dock.sheet {
                    Sheet::Peek => "Pull up the panel",
                    _ => "Fold the panel down",
                });
                if handle.dragged() {
                    // Up the screen is a taller sheet.
                    new_drag = drag - handle.drag_delta().y;
                }
                if handle.drag_stopped() {
                    snap = Some(nearest_snap(h, heights));
                    new_drag = 0.0;
                } else if handle.clicked() {
                    snap = Some(if self.dock.sheet == Sheet::Peek {
                        Sheet::Half
                    } else {
                        Sheet::Peek
                    });
                }
                ws::set_header_tabs(ctx, Some(tabs));
                if folded {
                    // Folded, the sheet is its tabs alone: the window under them is not drawn
                    // (it would not fit, and a panel whose contents overflow it loses its room).
                    let tab = front.tab();
                    header = ws::window_header(ui, &t, tab.glyph, tab.title, None, None);
                } else {
                    self.dock_window(front, Host::Docked(ui), ctx);
                }
                ws::set_header_tabs(ctx, None);
            });
        if folded {
            self.dock.apply_header(front, header);
        }
        ctx.data_mut(|d| d.insert_temp(drag_id, new_drag));
        if let Some(s) = snap {
            self.dock.sheet = s;
        }
        // The standing tabs do not close: their close button folds the sheet instead.
        for w in PHONE_CORE {
            if !self.dock.chrome(w).open {
                self.dock.chrome_mut(w).open = true;
                self.dock.sheet = Sheet::Peek;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sheet_snaps_to_the_nearest_height_and_always_leaves_some_map() {
        let h = sheet_heights(600.0, 60.0);
        assert_eq!(h[0], 60.0);
        assert!(h[1] > h[0] && h[1] < h[2]);
        assert_eq!(h[2], 600.0 - MAP_MIN_H);
        assert_eq!(nearest_snap(70.0, h), Sheet::Peek);
        assert_eq!(nearest_snap(h[1] + 10.0, h), Sheet::Half);
        assert_eq!(nearest_snap(590.0, h), Sheet::Full);
        // A tiny room still has a sheet that shows its tabs.
        let tiny = sheet_heights(100.0, 60.0);
        assert!(tiny.iter().all(|x| *x >= 60.0));
    }

    #[test]
    fn the_keyboard_cannot_fold_the_sheet_out_from_under_its_field() {
        // A portrait phone with the keyboard up: ~320 pt between the control row and the
        // timeline. The half sheet is 42% of that, under the unfold line.
        let (room, peek) = (320.0, 58.0);
        let half = sheet_heights(room, peek)[1];
        assert!(half < peek + UNFOLD_MIN, "the case this guards: {half}");
        // Not typing, it folds as before.
        assert_eq!(sheet_fit(half, peek, false), (half, true));
        // Typing, it stays open, tall enough for its body.
        assert_eq!(sheet_fit(half, peek, true), (peek + UNFOLD_MIN, false));
        // A taller sheet is left alone.
        assert_eq!(sheet_fit(250.0, peek, true), (250.0, false));
        // Even folded to its tabs with almost no room, a field being typed in keeps its body.
        assert_eq!(sheet_fit(peek, peek, true), (peek + UNFOLD_MIN, false));
    }

    #[test]
    fn the_rail_keeps_what_fits() {
        assert_eq!(rail_fit(1000.0), RAIL.len());
        assert_eq!(rail_fit(12.0), 0);
        assert_eq!(rail_fit(12.0 + 2.0 * (RAIL_BTN + 8.0)), 2);
        // Every map tool can still be armed from the Tools button's menu.
        let tools: usize = super::super::rail::GROUPS.iter().map(|g| g.len()).sum();
        assert!(tools >= 13);
    }
}
