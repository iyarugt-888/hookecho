//! The workstation on a phone: the Station phone design.
//!
//! The dock's windows, laid out for one hand around a full-height map. Over the map's top left a
//! floating pill holds the menu (behind the logo: every menu the desktop app bar has), search, the
//! alerts bell and one view chip for Site / Product / Tilt / 2D-3D; over its bottom left a round
//! menu button expands the map tools (center, Layers, 3D, display, measure, tools, full screen)
//! beside the always-shown locate button. Under the map, a bottom sheet whose tabs are the
//! workstation's windows (Inspector, Layers, Storms, Alerts, then whatever else is open), and the
//! timeline under everything. The full-width app bar and control row this replaced took a sixth
//! of a phone's height.
//!
//! It is the dock's own windows, not copies: the sheet draws the front one through
//! [`HookEchoApp::dock_window`], the same code a desktop dock runs, so everything the workstation
//! can do the phone can. What cannot be reached from the pill and the tools is in the Layers tab,
//! which is the whole command registry and searches it, and in the menu behind the logo, which
//! lists every window (`menus::window_home` is exhaustive).

use super::app_bar::{pane_items, share_rows, table_items, Follow};
use super::menus::{window_rows, Menu, MenuPick};
use super::*;
use crate::ui::a11y::Named as _;
use egui::{Color32, FontId, Rect, Sense, Stroke};
use egui_phosphor::regular as ph;
use wxdata::level2::Moment;

/// The sheet's drag handle strip.
const HANDLE_H: f32 = 18.0;
/// A round map button's diameter.
const RAIL_BTN: f32 = 46.0;
/// The floating pill's height.
const PILL_H: f32 = 48.0;
/// Room kept clear above the map's bottom edge for the basemap attribution line.
const ATTRIBUTION_H: f32 = 26.0;
/// Room kept clear at the map's right edge for the colour scale.
const LEGEND_W: f32 = 56.0;
/// The map keeps at least this much height however far the sheet is pulled up.
const MAP_MIN_H: f32 = 96.0;
/// The least body a sheet needs above its tabs to draw its window rather than fold.
const UNFOLD_MIN: f32 = 80.0;

/// What a map tool button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rail {
    Center,
    Layers,
    Dim,
    Display,
    Measure,
    Tools,
    FullScreen,
}

/// The expanded tool menu, nearest the thumb first (see [`rail_slots`]).
const RAIL: [(Rail, &str, &str); 7] = [
    (Rail::Layers, ph::STACK, "Layers"),
    (
        Rail::Display,
        ph::STACK_SIMPLE,
        "Display: smoothing, colours, overlays, map",
    ),
    (Rail::Dim, ph::CUBE, "3D map"),
    (Rail::Center, ph::CROSSHAIR, "Center on the radar"),
    (Rail::Measure, ph::RULER, "Measure distance"),
    (Rail::Tools, ph::WRENCH, "Map tools"),
    (Rail::FullScreen, ph::ARROWS_OUT, "Full-screen map"),
];

/// Where each of the expanded menu's buttons goes on a map this tall, as (column, row) from the
/// bottom left: up the first column above the locate and menu buttons that always show, then up
/// the next column from the bottom, and so on, staying under the pill and above the attribution
/// line. So a map shortened by the sheet still reaches every tool; past three columns the last
/// ones are left off (each is also in a menu).
fn rail_slots(map_h: f32) -> Vec<(usize, usize)> {
    let room = map_h - 10.0 - PILL_H - 10.0 - ATTRIBUTION_H - 4.0 + 8.0;
    let rows = ((room / (RAIL_BTN + 8.0)).floor().max(0.0) as usize).max(2);
    (0..3)
        .flat_map(|col| (if col == 0 { 2 } else { 0 }..rows).map(move |row| (col, row)))
        .take(RAIL.len())
        .collect()
}

/// A product's name short enough for the view chip: what its name says in brackets
/// ("Rain intensity (reflectivity)" is "Reflectivity"), or the name itself.
fn compact_product(name: &str) -> String {
    let inner = name
        .find('(')
        .zip(name.rfind(')'))
        .filter(|(a, b)| a < b)
        .map(|(a, b)| &name[a + 1..b]);
    match inner {
        Some(x) if !x.is_empty() => {
            let mut c = x.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        }
        _ => name.to_string(),
    }
}

/// The view chip's width on a map this wide: what the pill leaves beside the colour scale.
fn chip_width(map_w: f32) -> f32 {
    (map_w - 20.0 - 52.0 - 2.0 * 40.0 - 24.0 - LEGEND_W).clamp(112.0, 210.0)
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

/// `s`, cut with an ellipsis to fit `room` points in `font`.
fn fit_text(p: &egui::Painter, s: &str, font: &FontId, room: f32) -> String {
    let wide = |x: &str| {
        p.layout_no_wrap(x.to_string(), font.clone(), Color32::WHITE)
            .size()
            .x
            > room
    };
    if !wide(s) {
        return s.to_string();
    }
    let mut base: Vec<char> = s.chars().collect();
    while base.len() > 1 {
        base.pop();
        let shown = base.iter().collect::<String>() + "\u{2026}";
        if !wide(&shown) {
            return shown;
        }
    }
    "\u{2026}".to_string()
}

/// The view chip: the radar and tilt small above (in `TextDisabled`), the product below, a caret
/// at the end; a flat ImGui button that lights up while pressed.
fn view_chip(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    top: &str,
    product: &str,
    w: f32,
) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 40.0), Sense::click());
    if resp.hovered() || resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(r, 0.0, t.im.button_hovered);
    }
    let p = ui.painter_at(r);
    p.text(
        r.left_top() + egui::vec2(8.0, 3.0),
        egui::Align2::LEFT_TOP,
        top,
        FontId::proportional(11.5),
        t.text_faint,
    );
    let font = FontId::proportional(15.5);
    p.text(
        r.left_bottom() + egui::vec2(8.0, -3.0),
        egui::Align2::LEFT_BOTTOM,
        fit_text(&p, product, &font, r.width() - 30.0),
        font,
        t.text,
    );
    p.text(
        r.right_center() + egui::vec2(-8.0, 6.0),
        egui::Align2::RIGHT_CENTER,
        ph::CARET_DOWN,
        FontId::proportional(13.0),
        t.text,
    );
    resp.named(&format!("{top}, {product}: radar, product and tilt"))
}

/// A pill icon: a glyph on an ImGui button with its resting fill pushed transparent, held in
/// `ButtonActive` while on.
fn bar_icon(ui: &mut egui::Ui, t: &ws::Tokens, glyph: &str, on: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), Sense::click());
    if on || resp.hovered() || resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(
            r,
            0.0,
            if on || resp.is_pointer_button_down_on() {
                t.im.button_active
            } else {
                t.im.button_hovered
            },
        );
    }
    ui.painter().text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(22.0),
        t.text,
    );
    resp
}

/// The pill's menu button: the logo and a caret.
fn logo_button(ui: &mut egui::Ui, t: &ws::Tokens) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(52.0, 40.0), Sense::click());
    if resp.hovered() || resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(r, 0.0, t.im.button_hovered);
    }
    ui.painter().text(
        r.left_center() + egui::vec2(6.0, 0.0),
        egui::Align2::LEFT_CENTER,
        ph::BROADCAST,
        FontId::proportional(25.0),
        t.accent,
    );
    ui.painter().text(
        r.right_center() + egui::vec2(-5.0, 1.0),
        egui::Align2::RIGHT_CENTER,
        ph::CARET_DOWN,
        FontId::proportional(crate::theme::FONT),
        t.text,
    );
    resp
}

/// A thin upright line between the pill's groups.
fn pill_divider(ui: &mut egui::Ui, t: &ws::Tokens) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(9.0, 40.0), Sense::hover());
    ui.painter().line_segment(
        [
            r.center_top() + egui::vec2(0.0, 9.0),
            r.center_bottom() - egui::vec2(0.0, 9.0),
        ],
        Stroke::new(1.0, t.line_soft),
    );
}

/// A square map button: an ImGui frame on the window colour with its border (so it reads over any
/// basemap), held in `ButtonActive` when on.
fn rail_button(ui: &mut egui::Ui, t: &ws::Tokens, glyph: &str, on: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(RAIL_BTN, RAIL_BTN), Sense::click());
    let fill = if on || resp.is_pointer_button_down_on() {
        t.im.button_active
    } else if resp.hovered() {
        t.im.button_hovered
    } else {
        t.panel
    };
    ui.painter().rect(
        r,
        0.0,
        fill,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(22.0),
        t.text,
    );
    resp
}

/// The menu behind the logo: settings, tools, forecasts, share and help.
fn menu_rows(ui: &mut egui::Ui, t: &ws::Tokens, workspaces: &[String]) -> Option<MenuPick> {
    let mut pick = None;
    let head = |ui: &mut egui::Ui, s: &str| ws::section_rule(ui, t, s);
    head(ui, "Settings");
    if ui.button("Preferences").clicked() {
        pick = Some(MenuPick::Prefs(PrefsPage::App, None));
    }
    if ui.button("Map settings").clicked() {
        pick = Some(MenuPick::Prefs(PrefsPage::Map, None));
    }
    if let Some(p) = window_rows(ui, t, Menu::Settings) {
        pick = Some(p);
    }
    ui.separator();
    if let Some(p) = window_rows(ui, t, Menu::Tools) {
        pick = Some(p);
    }
    ui.separator();
    head(ui, "Forecasts");
    if let Some(p) = window_rows(ui, t, Menu::Discussion) {
        pick = Some(p);
    }
    ui.separator();
    head(ui, "Share");
    if let Some(p) = share_rows(ui, t, workspaces) {
        pick = Some(p);
    }
    ui.separator();
    head(ui, "Help");
    if let Some(p) = window_rows(ui, t, Menu::Help) {
        pick = Some(p);
    }
    pick
}

impl HookEchoApp {
    /// Whether this frame draws the Station phone chrome: on a phone, with that design picked.
    pub(crate) fn phone_station(&self) -> bool {
        crate::platform::phone_layout()
            && self.settings.phone_design == crate::settings::PhoneDesign::Station
    }

    /// The phone's docked parts, drawn before the map's rect is read (as [`Self::dock_layout`] is
    /// on a desktop): the timeline across the bottom and the sheet above it. The rest floats over
    /// the map ([`Self::phone_overlay`]). A full-screen map draws none of them.
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
        // The timeline first, so it is the lowest of the bottom panels, under the sheet.
        self.dock_timeline(root, true);
        self.phone_sheet(root, ctx);
    }

    /// Over the map: the pill at the top left and the expandable tools at the bottom left.
    pub(crate) fn phone_overlay(&mut self, ctx: &egui::Context) {
        if self.mobile_chrome_hidden {
            return;
        }
        let t = self.ws_tokens();
        let map = self.chrome_rect;
        self.phone_pill(ctx, &t, map);
        let open_id = egui::Id::new("phone_tools_open");
        let mut open: bool = ctx.data(|d| d.get_temp(open_id)).unwrap_or(false);
        // The tools' places, and each column's height in buttons (the first column also holds
        // the two buttons that always show); shorter columns are padded at the top so every
        // column sits on the same bottom line.
        let slots = if open {
            rail_slots(map.height())
        } else {
            Vec::new()
        };
        let cols = slots.iter().map(|s| s.0 + 1).max().unwrap_or(1);
        let col_rows =
            |col: usize| slots.iter().filter(|s| s.0 == col).count() + if col == 0 { 2 } else { 0 };
        let tallest = (0..cols).map(col_rows).max().unwrap_or(2);
        let armed = self.tool;
        let has_site = self.views[self.active].site.is_some();
        let layers_on = self.dock.shown(DockWin::Layers);
        let map_3d = self.views[self.active].map_3d.enabled;
        let mut hit = None;
        let mut tool_pick = None;
        let mut locate = false;
        egui::Area::new(egui::Id::new("phone_rail"))
            .order(egui::Order::Middle)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(map.left_bottom() + egui::vec2(10.0, -(ATTRIBUTION_H + 4.0)))
            .show(ctx, |ui| {
                ws::style_scope(ui, &t);
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                ui.horizontal(|ui| {
                    for col in 0..cols {
                        ui.vertical(|ui| {
                            ui.add_space((tallest - col_rows(col)) as f32 * (RAIL_BTN + 8.0));
                            // This column's tools, top down.
                            let mut mine: Vec<(usize, usize)> = slots
                                .iter()
                                .enumerate()
                                .filter(|(_, s)| s.0 == col)
                                .map(|(i, s)| (s.1, i))
                                .collect();
                            mine.sort_by_key(|m| std::cmp::Reverse(m.0));
                            for (_, i) in mine {
                                let (what, glyph, name) = RAIL[i];
                                let on = match what {
                                    Rail::Measure => armed == MapTool::Measure,
                                    Rail::Tools => {
                                        !matches!(armed, MapTool::Interrogate | MapTool::Measure)
                                    }
                                    Rail::Layers => layers_on,
                                    Rail::Dim => map_3d,
                                    _ => false,
                                };
                                // The tools button wears the armed tool's glyph, so it says
                                // what a tap on the map will do.
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
                                            .align(egui::RectAlign::RIGHT_END)
                                            .close_behavior(
                                                egui::PopupCloseBehavior::CloseOnClickOutside,
                                            )
                                            .show(|ui| {
                                                ws::menu_scope(ui, &t);
                                                ui.set_min_width(250.0);
                                                ui.spacing_mut().item_spacing.y = 6.0;
                                                ui.spacing_mut().button_padding =
                                                    egui::vec2(10.0, 7.0);
                                                egui::ScrollArea::vertical()
                                                    .max_height(map.height().max(260.0))
                                                    .show(ui, |ui| {
                                                        self.phone_display_items(ui, &t, ctx)
                                                    });
                                            });
                                    }
                                    Rail::Tools => {
                                        egui::Popup::menu(&resp)
                                            .align(egui::RectAlign::RIGHT_END)
                                            .show(|ui| {
                                                ws::menu_scope(ui, &t);
                                                ui.set_min_width(240.0);
                                                ui.style_mut().spacing.button_padding =
                                                    egui::vec2(10.0, 9.0);
                                                for (gi, group) in
                                                    super::rail::GROUPS.iter().enumerate()
                                                {
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
                            if col == 0 {
                                locate = rail_button(ui, &t, ph::NAVIGATION_ARROW, false)
                                    .named("Center on my location")
                                    .clicked();
                                let toggle =
                                    rail_button(ui, &t, if open { ph::X } else { ph::LIST }, open)
                                        .named_toggle("Map tools", open);
                                if toggle.clicked() {
                                    open = !open;
                                }
                            }
                        });
                    }
                });
            });
        if locate {
            self.locate_me();
        }
        match hit {
            Some(Rail::Center) => self.dock_center_on_radar(),
            Some(Rail::Layers) => self.dock.toggle(DockWin::Layers),
            Some(Rail::Dim) => self.views[self.active].set_map_3d(!map_3d),
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
                open = false;
            }
            _ => {}
        }
        if let Some(tool) = tool_pick {
            self.apply_palette(crate::app::PaletteAction::Tool(tool), ctx);
        }
        ctx.data_mut(|d| d.insert_temp(open_id, open));
    }

    /// The Display menu: what the desktop toolbar carries past Site, Product and Tilt.
    fn phone_display_items(&mut self, ui: &mut egui::Ui, t: &ws::Tokens, ctx: &egui::Context) {
        use crate::app::{OverlayToggle as T, PaletteAction as A};
        let mut action = None;
        let v = &mut self.views[self.active];
        ws::section_rule(ui, t, "View");
        ws::check(ui, t, &mut v.smooth, "Smoothing");
        ws::check(ui, t, &mut v.show_legend, "Colour scale");
        let follow = Follow::of(v.follow_lowest_cut, v.follow_live_sweep);
        ws::section_rule(ui, t, "Follow while live");
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
        ws::section_rule(ui, t, "Overlays");
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
        ws::section_rule(ui, t, "Map");
        let basemap = self.views[self.active].basemap.label();
        if ui.button(format!("Map style: {basemap}\u{2026}")).clicked() {
            self.basemap_open = true;
            ui.close();
        }
        let panes = self.views.len();
        let all_panes_linked = self.all_pane_links_on();
        let links: Vec<(T, bool)> = T::PANE_LINKS
            .into_iter()
            .map(|t| (t, *self.overlay_flag(t)))
            .collect();
        let pane_layout = self.pane_layout;
        ui.menu_button(
            format!("Panes: {panes} \u{b7} {}", pane_layout.label()),
            |ui| {
                ws::menu_scope(ui, t);
                pane_items(
                    ui,
                    panes,
                    pane_layout,
                    &links,
                    all_panes_linked,
                    &mut action,
                );
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

    /// The floating pill over the map's top left: the menu behind the logo, search, the alerts
    /// bell, and the view chip that picks the radar, product, tilt and 2D / 3D.
    fn phone_pill(&mut self, ctx: &egui::Context, t: &ws::Tokens, map: Rect) {
        use crate::app::PaletteAction as A;
        let (alerts, _) = self.alert_badge();
        let alerts_on = self.dock.shown(DockWin::Alerts);
        let workspaces: Vec<String> = self
            .settings
            .workspaces
            .iter()
            .map(|w| w.name.clone())
            .collect();
        let menu_h = ctx.content_rect().height() * 0.7;
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
        let site_text = site.clone().unwrap_or_else(|| "No radar".to_string());
        let tilt_text = elevations
            .get(tilt)
            .map_or_else(|| "\u{2014}".to_string(), |a| format!("{a:.1}\u{b0}"));
        let top = format!(
            "{site_text} \u{b7} {tilt_text}{}",
            if map_3d { " \u{b7} 3D" } else { "" }
        );
        let product = compact_product(crate::products::name(moment, srv));
        let chip_w = chip_width(map.width());
        let mut pick = None;
        let mut search = false;
        let mut bell = false;
        let mut action = None;
        let mut open_sites = false;
        let mut pick_tilt = None;
        let mut want_3d = None;
        egui::Area::new(egui::Id::new("phone_pill"))
            .order(egui::Order::Middle)
            .fixed_pos(map.left_top() + egui::vec2(10.0, 10.0))
            .show(ctx, |ui| {
                ws::style_scope(ui, t);
                egui::Frame::NONE
                    .fill(t.panel)
                    .stroke(Stroke::new(1.0, t.line))
                    .corner_radius(0)
                    .inner_margin(egui::Margin::symmetric(4, 4))
                    .show(ui, |ui| {
                        ui.set_height(PILL_H - 8.0);
                        ui.horizontal_centered(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let menu = logo_button(ui, t).named("Settings, tools, share and help");
                            egui::Popup::menu(&menu).show(|ui| {
                                ws::menu_scope(ui, t);
                                ui.set_min_width(260.0);
                                ui.style_mut().spacing.button_padding = egui::vec2(10.0, 8.0);
                                egui::ScrollArea::vertical()
                                    .max_height(menu_h)
                                    .show(ui, |ui| {
                                        if let Some(p) = menu_rows(ui, t, &workspaces) {
                                            pick = Some(p);
                                        }
                                    });
                            });
                            pill_divider(ui, t);
                            search = bar_icon(ui, t, ph::MAGNIFYING_GLASS, false)
                                .named("Search layers, sites, places and tools")
                                .clicked();
                            let b = bar_icon(ui, t, ph::BELL, alerts_on)
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
                                    FontId::proportional(11.5),
                                    Color32::WHITE,
                                );
                                let r = Rect::from_center_size(
                                    at,
                                    egui::vec2((g.size().x + 8.0).max(16.0), 16.0),
                                );
                                ui.painter().rect_filled(r, 0.0, t.danger);
                                ui.painter()
                                    .galley(r.center() - g.size() / 2.0, g, Color32::WHITE);
                            }
                            bell = b.clicked();
                            pill_divider(ui, t);
                            let chip = view_chip(ui, t, &top, &product, chip_w);
                            egui::Popup::menu(&chip).show(|ui| {
                                ws::menu_scope(ui, t);
                                ui.set_min_width(240.0);
                                ui.style_mut().spacing.button_padding = egui::vec2(10.0, 8.0);
                                egui::ScrollArea::vertical()
                                    .max_height(menu_h)
                                    .show(ui, |ui| {
                                        let head = |ui: &mut egui::Ui, s: &str| ws::section_rule(ui, t, s);
                                        head(ui, "Radar");
                                        if ui
                                            .button(format!("{site_text}   Change radar\u{2026}"))
                                            .clicked()
                                        {
                                            open_sites = true;
                                            ui.close();
                                        }
                                        ui.separator();
                                        head(ui, "Product");
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
                                        ui.separator();
                                        head(ui, "Tilt");
                                        for (i, a) in elevations.iter().enumerate() {
                                            let mut label = format!("{a:.1}\u{b0}");
                                            if sweeping == Some(i) {
                                                label.push_str("  \u{25cf} live");
                                            }
                                            if ui.selectable_label(i == tilt, label).clicked() {
                                                pick_tilt = Some(i);
                                            }
                                        }
                                        if ui
                                            .selectable_label(false, "All tilts (four panes)")
                                            .clicked()
                                        {
                                            action = Some(A::AllTilts);
                                        }
                                        ui.separator();
                                        head(ui, "View");
                                        ui.horizontal(|ui| {
                                            for (label, three) in
                                                [("2D map", false), ("3D map", true)]
                                            {
                                                if ui
                                                    .selectable_label(map_3d == three, label)
                                                    .clicked()
                                                    && map_3d != three
                                                {
                                                    want_3d = Some(three);
                                                }
                                            }
                                        });
                                    });
                            });
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
        if open_sites {
            self.site_dialog = Some(Default::default());
        }
        if let Some(i) = pick_tilt {
            self.views[self.active].tilt = i;
        }
        if let Some(on) = want_3d {
            self.views[self.active].set_map_3d(on);
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
                    .corner_radius(0),
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
                    0.0,
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
                    // The window draws in exactly the body's room, clipped to it. A window whose
                    // fixed parts are taller than a half-height sheet (Layers' categories,
                    // search and filters) otherwise grew the panel upward past its own rect,
                    // and the handle and tabs went out of view under the map.
                    let body = ui.available_rect_before_wrap();
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(body)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    child.set_clip_rect(body.intersect(ui.clip_rect()));
                    self.dock_window(front, Host::Docked(&mut child), ctx);
                    ui.allocate_rect(body, Sense::hover());
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
        // A tall map: one column above locate and the menu button.
        let tall = rail_slots(2000.0);
        assert_eq!(tall.len(), RAIL.len());
        assert!(tall.iter().all(|s| s.0 == 0));
        assert_eq!(
            tall[0],
            (0, 2),
            "the first tool sits right above the two fixed buttons"
        );
        // A half sheet on a phone leaves about 410 points of map: three above, the rest beside.
        let half = rail_slots(410.0);
        assert_eq!(half.len(), RAIL.len());
        assert_eq!(half.iter().filter(|s| s.0 == 0).count(), 3);
        // Every slot stays under the pill.
        let rows = |h: f32| ((h - 90.0) / (RAIL_BTN + 8.0)).floor() as usize;
        assert!(half.iter().all(|s| s.1 < rows(410.0)));
        // The shortest map the sheet leaves holds four beside the two fixed buttons, and a
        // quarter of a phone every one.
        assert_eq!(rail_slots(MAP_MIN_H).len(), 4);
        assert_eq!(rail_slots(260.0).len(), RAIL.len());
        let tools: usize = super::super::rail::GROUPS.iter().map(|g| g.len()).sum();
        assert!(tools >= 13);
    }

    #[test]
    fn the_view_chip_reads_short_and_fits_beside_the_colour_scale() {
        assert_eq!(
            compact_product("Rain intensity (reflectivity)"),
            "Reflectivity"
        );
        assert_eq!(compact_product("Wind toward/away (velocity)"), "Velocity");
        assert_eq!(compact_product("Storm rotation (SRV)"), "SRV");
        assert_eq!(
            compact_product("Composite reflectivity"),
            "Composite reflectivity"
        );
        // A 390 pt phone leaves the chip room for "Reflectivity" with the pill's other buttons
        // and the colour scale; a narrow one keeps a usable minimum.
        let w = chip_width(390.0);
        assert!(
            w >= 140.0 && 10.0 + 52.0 + 80.0 + 24.0 + w + LEGEND_W <= 390.0,
            "{w}"
        );
        assert_eq!(chip_width(200.0), 112.0);
    }
}
