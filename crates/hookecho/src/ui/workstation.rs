//! The dock's design system ("Analyst Workstation", `docs/WSV3_IMGUI_MODERN_DESIGN_PLAN.md`), drawn
//! in Dear ImGui's manner: one [`Tokens`] value for every colour and size, and small stateless
//! painters that the dock's panels compose. Dock code never sets a colour inline; it asks for a
//! component.
//!
//! The colours are the active Dear ImGui style's (`theme::current`, with the user's accent override
//! folded in), and every painter draws what ImGui draws: 19px frames with `FramePadding` 4 x 3,
//! square corners (tabs alone round their top corners), no frame borders, the one 13px font,
//! buttons in the `Button` roles, selected rows in the `Header` roles, ticks and grabs in
//! `CheckMark` and `SliderGrab`.

use crate::theme::{Palette, FONT, FRAME_H, TAB_ROUNDING};
use egui::{
    Color32, CornerRadius, FontId, Frame, InnerResponse, Margin, Rect, Response, Sense, Stroke,
    Vec2,
};

/// App bar height: one frame (tabs, buttons, the clock) with the menu bar's margin round it.
pub const APP_BAR_H: f32 = FRAME_H + 6.0;
/// Context toolbar height: a frame with `ItemSpacing.y` above and below.
pub const TOOLBAR_H: f32 = FRAME_H + 8.0;
/// Standard control height (buttons, combos, fields): ImGui's frame.
pub const CONTROL_H: f32 = FRAME_H;
/// Tool-rail button edge: an 18px glyph plus `FramePadding` either side (ImGui's ImageButton).
pub const RAIL_BTN: f32 = 26.0;
/// Panel header height: ImGui's title bar, one frame.
pub const HEADER_H: f32 = FRAME_H;

/// Every colour the workstation look uses: the active ImGui style's roles under the names the dock
/// has always asked for, and the whole [`Palette`] in `im` for the roles those names do not cover.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    /// The map's ground, behind every window.
    pub bg: Color32,
    /// `WindowBg`: a docked panel, a floating card, a sheet.
    pub panel: Color32,
    /// `TableHeaderBg`: a header row inside a panel.
    pub panel_hi: Color32,
    /// `FrameBg`: checkboxes, sliders, fields, tracks.
    pub field: Color32,
    /// `FrameBgHovered`.
    pub field_hi: Color32,
    /// `Border`: window and popup edges.
    pub line: Color32,
    /// `Separator`: rules and dividers.
    pub line_soft: Color32,
    /// `Text`.
    pub text: Color32,
    /// Secondary text. ImGui has one text colour, so this is `Text` too: hierarchy comes from
    /// columns, headers and the mono face, not from a dimmer grey.
    pub text_dim: Color32,
    /// `TextDisabled`: disabled labels, hints in empty fields, shortcuts.
    pub text_faint: Color32,
    /// `TextLink` (or the user's accent): the colour that marks a thing as chosen, and coloured
    /// text. (Ticks and grabs use `im.check_mark` and `im.slider_grab` themselves.)
    pub accent: Color32,
    pub live: Color32,
    pub warn: Color32,
    pub danger: Color32,
    /// Every role of the active style.
    pub im: Palette,
}

impl Tokens {
    /// The workstation colours of the style on screen now, with `accent` as the accent and check
    /// mark (the caller's accent — `theme::accent` — or a test's own).
    pub fn new(accent: Color32) -> Tokens {
        Tokens::of(crate::theme::current(), Some(accent))
    }

    /// The workstation colours of `p`, optionally with `accent` as its accent and check mark.
    pub fn of(mut p: Palette, accent: Option<Color32>) -> Tokens {
        // The style's own accent (what `theme::accent` hands the dock) leaves its check mark
        // alone: Classic keeps its grey ticks. Any other colour takes over both.
        if let Some(a) = accent.filter(|a| *a != p.text_link) {
            p.check_mark = a;
            p.text_link = a;
        }
        Tokens {
            bg: crate::theme::VIEWPORT_BG,
            panel: p.window_bg,
            panel_hi: p.table_header_bg,
            field: p.frame_bg,
            field_hi: p.frame_bg_hovered,
            line: p.border,
            line_soft: p.separator,
            text: p.text,
            text_dim: p.text,
            text_faint: p.text_disabled,
            accent: p.text_link,
            live: p.live,
            warn: p.warn,
            danger: p.danger,
            im: p,
        }
    }

    /// The selected-row wash: ImGui's `Header`, for a selected row or an "on" list item.
    pub fn accent_soft(&self) -> Color32 {
        self.im.header
    }

    /// The same as [`Tokens::new`]: since the Dear ImGui restyle every layout draws in the active
    /// ImGui style, so a panel shared with a layout outside the workstation (the floating "3D map"
    /// window) needs nothing from its host's visuals.
    pub fn from_visuals(_v: &egui::Visuals, accent: Color32) -> Tokens {
        Tokens::new(accent)
    }
}

/// A docked panel's body: `WindowBg` with its 1px `Border`.
pub fn panel_frame(t: &Tokens) -> Frame {
    Frame::NONE
        .fill(t.panel)
        .stroke(Stroke::new(1.0, t.line))
        .inner_margin(Margin::same(0))
}

/// A floating card over the map: an ImGui window, the same as a docked one — square, bordered,
/// no shadow.
pub fn card_frame(t: &Tokens) -> Frame {
    Frame::NONE
        .fill(t.panel)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(CornerRadius::ZERO)
}

/// Stock egui widgets in the workstation look, for the controls the dock embeds rather than
/// paints (text fields, combo boxes and their menus, sliders, the shared model and layer-option
/// panels).
/// Whether `ui` is inside a [`style_scope`]: shared widgets drawn in both the phone-sized
/// panels and the workstation (the settings switch) take the compact metrics here. Read from
/// the two metrics the scope sets together.
pub fn in_scope(ui: &egui::Ui) -> bool {
    ui.spacing().interact_size.y == CONTROL_H
        && ui
            .style()
            .text_styles
            .get(&egui::TextStyle::Body)
            .is_some_and(|f| f.size == FONT)
}

/// [`style_scope`] for a popup menu: the same tokens, but items are flat rows that light up in
/// `HeaderHovered`, as Dear ImGui's menus are, rather than a stack of framed buttons.
pub fn menu_scope(ui: &mut egui::Ui, t: &Tokens) {
    style_scope(ui, t);
    let style = ui.style_mut();
    style.spacing.item_spacing.y = 0.0;
    style.spacing.button_padding = egui::vec2(4.0, 2.0);
    let v = &mut style.visuals;
    for w in [&mut v.widgets.inactive, &mut v.widgets.noninteractive] {
        w.weak_bg_fill = Color32::TRANSPARENT;
    }
    for w in [
        &mut v.widgets.hovered,
        &mut v.widgets.open,
        &mut v.widgets.active,
    ] {
        w.weak_bg_fill = t.im.header_hovered;
    }
}

/// ImGui's style for the stock widgets inside a workstation panel: the desktop geometry (even on a
/// phone, whose sheet draws the desktop windows) and the active style's roles.
pub fn style_scope(ui: &mut egui::Ui, t: &Tokens) {
    let style = ui.style_mut();
    style.override_text_style = None;
    crate::theme::geometry(style, false);
    let v = &mut style.visuals;
    crate::theme::fill_widgets(v, &t.im);
    v.override_text_color = Some(t.text);
    v.extreme_bg_color = t.field;
    v.text_edit_bg_color = Some(t.field);
    v.faint_bg_color = t.im.table_row_bg_alt;
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.window_stroke = Stroke::new(1.0, t.line);
    v.hyperlink_color = t.im.text_link;
    v.weak_text_color = Some(t.text_faint);
}

/// Monospace text, for the things read as data rather than as words: values, times,
/// coordinates, VCP and frame counts, so digits line up and one reading is told from the next.
pub fn mono(s: impl Into<String>, size: f32, color: Color32) -> egui::RichText {
    egui::RichText::new(s.into())
        .monospace()
        .size(size)
        .color(color)
}

/// Text in the workstation's proportional face.
pub fn text(s: impl Into<String>, size: f32, color: Color32) -> egui::RichText {
    egui::RichText::new(s.into()).size(size).color(color)
}

/// ImGui's check mark in a `size` square at `min` (`RenderCheckMark`).
fn check_mark(p: &egui::Painter, min: egui::Pos2, size: f32, color: Color32) {
    crate::ui::wsv3::check_mark(p, min, size, color);
}

/// ImGui's window close cross (`CloseButton`), `size` across, centred on `c`.
fn close_cross(p: &egui::Painter, c: egui::Pos2, size: f32, color: Color32) {
    let e = size * 0.5 * std::f32::consts::FRAC_1_SQRT_2 - 1.0;
    let s = Stroke::new(1.0, color);
    p.line_segment([c + egui::vec2(-e, -e), c + egui::vec2(e, e)], s);
    p.line_segment([c + egui::vec2(e, -e), c + egui::vec2(-e, e)], s);
}

/// ImGui's solid triangle (`RenderArrow`): pointing down when `open`, right when not.
fn arrow(p: &egui::Painter, c: egui::Pos2, size: f32, open: bool, color: Color32) {
    let r = size * 0.4;
    let pts = if open {
        vec![
            c + egui::vec2(-r, -r * 0.5),
            c + egui::vec2(r, -r * 0.5),
            c + egui::vec2(0.0, r * 0.75),
        ]
    } else {
        vec![
            c + egui::vec2(-r * 0.5, -r),
            c + egui::vec2(r * 0.75, 0.0),
            c + egui::vec2(-r * 0.5, r),
        ]
    };
    p.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
}

/// The fill an ImGui button shows for `resp`: held, hovered, at rest; `on` holds it down.
fn button_fill(t: &Tokens, resp: &Response, on: bool) -> Color32 {
    if on || resp.is_pointer_button_down_on() {
        t.im.button_active
    } else if resp.hovered() {
        t.im.button_hovered
    } else {
        t.im.button
    }
}

/// What a tool window's header asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeaderAction {
    None,
    Close,
    /// Fold a floating window to its title bar, or unfold it.
    Collapse,
    /// Move the window: dock it to a side or float it over the map.
    Place(crate::workspace::Place),
    /// Bring this tab of the header's tab group to the front (see [`HeaderTabs`]).
    Tab(usize),
    /// Dragged out of its dock: float it (a group's tab `Some(i)`, or the window itself) with its
    /// header under this point, as Dear ImGui tears a tab out of a dock node.
    TearOff(Option<usize>, egui::Pos2),
    /// A group's tab dragged along the strip over another: move tab `.0` to where tab `.1` is.
    Reorder(usize, usize),
}

/// What dragging tab `i` of a strip to `at` asks for: out of the strip (above or below it by more
/// than [`TEAR_DISTANCE`]) tears the tab off; along it, over another tab, moves it there.
pub fn tab_drag(i: usize, at: egui::Pos2, strip: Rect, tabs: &[Rect]) -> HeaderAction {
    if at.y < strip.top() - TEAR_DISTANCE || at.y > strip.bottom() + TEAR_DISTANCE {
        return HeaderAction::TearOff(Some(i), at);
    }
    match tabs.iter().position(|r| r.x_range().contains(at.x)) {
        Some(j) if j != i => HeaderAction::Reorder(i, j),
        _ => HeaderAction::None,
    }
}

/// How far a docked header or tab has to be dragged before it tears off, so a click that wobbles
/// is still a click.
const TEAR_DISTANCE: f32 = 14.0;

/// Whether a drag of `delta` from a docked header has gone far enough to tear it off.
pub fn tears_off(delta: egui::Vec2) -> bool {
    delta.length() >= TEAR_DISTANCE
}

/// One tab of a dock's tab group: the window's glyph and plain title, and a status dot for a
/// window that wants a look while it is behind another tab (alerts in view, a failing feed).
#[derive(Clone, Debug, PartialEq)]
pub struct HeaderTab {
    pub glyph: &'static str,
    pub title: &'static str,
    pub dot: Option<Color32>,
}

/// The windows sharing one dock, as Dear ImGui's docking draws them: the front window's header
/// becomes a strip of every window's tab, with the front window's own move and close buttons at
/// its end. Handed to the next [`window_header`] through [`set_header_tabs`], so a window's body
/// draws the same way whether it is alone or in a group.
#[derive(Clone, Debug, PartialEq)]
pub struct HeaderTabs {
    pub tabs: Vec<HeaderTab>,
    pub front: usize,
}

fn header_tabs_id() -> egui::Id {
    egui::Id::new("ws_header_tabs")
}

fn touch_id() -> egui::Id {
    egui::Id::new("ws_touch")
}

/// Say whether the workstation is on a phone (the Station design): window headers are taller,
/// their tabs are words a finger can hit, and a window cannot be moved or folded (it lives in the
/// bottom sheet).
pub fn set_touch(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(touch_id(), on));
}

/// Whether [`set_touch`] said the workstation is on a phone.
pub fn touch(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(touch_id()))
        .unwrap_or(false)
}

/// The header height: taller for a finger.
pub fn header_h(ctx: &egui::Context) -> f32 {
    if touch(ctx) {
        40.0
    } else {
        HEADER_H
    }
}

/// Make the next [`window_header`] drawn a tab strip (or, with `None`, stop that).
pub fn set_header_tabs(ctx: &egui::Context, tabs: Option<HeaderTabs>) {
    ctx.data_mut(|d| match tabs {
        Some(t) => {
            d.insert_temp(header_tabs_id(), t);
        }
        None => d.remove::<HeaderTabs>(header_tabs_id()),
    });
}

/// A group's tab widths: every tab with its title if they all fit in `avail`; otherwise the front
/// one keeps its title and the rest shrink to their glyph (the title moves to the hover).
fn tab_widths(full: &[f32], front: usize, avail: f32) -> Vec<f32> {
    const GLYPH_ONLY: f32 = 32.0;
    if full.iter().sum::<f32>() <= avail {
        return full.to_vec();
    }
    full.iter()
        .enumerate()
        .map(|(i, w)| if i == front { *w } else { GLYPH_ONLY })
        .collect()
}

/// A tool window's header, drawn as Dear ImGui's title bar: glyph and title, then (right-aligned) a
/// placement menu, a fold triangle while floating, and the close cross. `place` is `None` for a
/// window that cannot move; `collapsed` is `Some` only while the window floats. Double-clicking a
/// floating window's header folds it too, as in Dear ImGui. Given [`HeaderTabs`], the bar is a
/// docking node's tab bar instead.
pub fn window_header(
    ui: &mut egui::Ui,
    t: &Tokens,
    glyph: &str,
    title: &str,
    place: Option<crate::workspace::Place>,
    collapsed: Option<bool>,
) -> HeaderAction {
    use crate::workspace::Place;
    // On a phone a window lives in the bottom sheet: it cannot be moved or folded.
    let touch = touch(ui.ctx());
    let (place, collapsed) = if touch {
        (None, None)
    } else {
        (place, collapsed)
    };
    // Taken, not read: the tabs belong to this one header, not to any drawn after it.
    let tabs = ui.ctx().data_mut(|d| {
        let tabs = d.get_temp::<HeaderTabs>(header_tabs_id());
        d.remove::<HeaderTabs>(header_tabs_id());
        tabs
    });
    // A docked header can be dragged out of its dock; a floating one is how its window is moved,
    // so it must leave the drag to the window.
    let docked = place.is_some_and(|p| p != Place::Float);
    let (rect, bar) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), header_h(ui.ctx())),
        if docked {
            Sense::click_and_drag()
        } else {
            Sense::click()
        },
    );
    let p = ui.painter();
    // The focused window's title bar colour: a docked window is always the one its dock shows.
    p.rect_filled(rect, 0.0, t.im.title_bg_active);
    let mut action = HeaderAction::None;
    let buttons = 1 + usize::from(collapsed.is_some()) + usize::from(place.is_some());
    let font_size = if touch { 15.0 } else { FONT };
    if let Some(group) = &tabs {
        // ImGui's tab bar: the bar's bottom edge in the selected tab's colour.
        p.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(1.0, t.im.tab_selected),
        );
        let font = FontId::proportional(font_size);
        // A touch tab is its word alone, as a phone's tabs are; the glyph comes back when it has
        // to shrink to one.
        let pad = if touch { 20.0 } else { 34.0 };
        let full: Vec<f32> = group
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let name = if i == group.front && !touch {
                    title
                } else {
                    tab.title
                };
                let text = ui
                    .painter()
                    .layout_no_wrap(name.into(), font.clone(), t.text);
                text.size().x + pad
            })
            .collect();
        let per_button = if touch { 44.0 } else { 20.0 };
        let avail = rect.width() - 8.0 - per_button * buttons as f32;
        let widths = tab_widths(&full, group.front, avail);
        let tab_rects: Vec<Rect> = widths
            .iter()
            .scan(rect.left() + 4.0, |x, w| {
                let r =
                    Rect::from_min_size(egui::pos2(*x, rect.top()), egui::vec2(*w, rect.height()));
                *x += w;
                Some(r)
            })
            .collect();
        for (i, (tab, r)) in group.tabs.iter().zip(&tab_rects).enumerate() {
            let r = *r;
            let resp = ui.interact(
                r,
                ui.id().with(("header_tab", tab.title)),
                if docked {
                    Sense::click_and_drag()
                } else {
                    Sense::click()
                },
            );
            if resp.dragged() && tears_off(resp.total_drag_delta().unwrap_or_default()) {
                if let Some(at) = resp.interact_pointer_pos() {
                    let asked = tab_drag(i, at, rect, &tab_rects);
                    if asked != HeaderAction::None {
                        action = asked;
                    }
                }
            }
            let front = i == group.front;
            let name = if front && !touch { title } else { tab.title };
            let p = ui.painter();
            // ImGui's tab: `ItemInnerSpacing` between tabs, top corners rounded, the front one in
            // `TabSelected` with its overline.
            let body = Rect::from_min_max(
                egui::pos2(r.left(), r.top() + if touch { 3.0 } else { 0.0 }),
                egui::pos2(r.right() - 4.0, r.bottom()),
            );
            let fill = if front {
                t.im.tab_selected
            } else if resp.hovered() {
                t.im.tab_hovered
            } else {
                t.im.tab
            };
            let top = CornerRadius {
                nw: TAB_ROUNDING,
                ne: TAB_ROUNDING,
                sw: 0,
                se: 0,
            };
            p.rect_filled(body, top, fill);
            if front {
                p.rect_filled(
                    Rect::from_min_size(body.min, egui::vec2(body.width(), 2.0)),
                    top,
                    t.im.tab_selected_overline,
                );
            }
            let compact = r.width() < full[i];
            if touch && !compact {
                p.text(
                    body.center(),
                    egui::Align2::CENTER_CENTER,
                    name,
                    font.clone(),
                    t.text,
                );
            } else {
                p.text(
                    if compact {
                        body.center()
                    } else {
                        body.left_center() + egui::vec2(11.0, 0.0)
                    },
                    egui::Align2::CENTER_CENTER,
                    tab.glyph,
                    FontId::proportional(if touch { 18.0 } else { FONT }),
                    t.text,
                );
            }
            if !compact && !touch {
                p.text(
                    body.left_center() + egui::vec2(22.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    name,
                    font.clone(),
                    t.text,
                );
            }
            // The front tab's own header already says it (a count in its title); a tab behind it
            // has only this dot to say it wants a look.
            if let (Some(c), false) = (tab.dot, front) {
                let at = if compact {
                    body.center() + egui::vec2(7.0, -5.0)
                } else {
                    body.left_center() + egui::vec2(17.0, -5.0)
                };
                p.circle(at, 3.0, c, Stroke::new(1.0, fill));
            }
            // The dot's meaning in words, for the hover and a screen reader.
            let said = if tab.dot.is_some() && !front {
                format!("{name}, needs a look")
            } else {
                name.to_string()
            };
            resp.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, front, &said)
            });
            let resp = if compact || tab.dot.is_some() {
                resp.on_hover_text(&said)
            } else {
                resp
            };
            // On a phone the front tab answers too: it is how a folded sheet is opened again.
            if resp.clicked() && (!front || touch) {
                action = HeaderAction::Tab(i);
            }
        }
    } else {
        p.text(
            rect.left_center() + egui::vec2(4.0 + FONT * 0.5, 0.0),
            egui::Align2::CENTER_CENTER,
            glyph,
            FontId::proportional(font_size),
            t.text,
        );
        p.text(
            rect.left_center() + egui::vec2(4.0 + FONT + 4.0, 0.0),
            egui::Align2::LEFT_CENTER,
            title,
            FontId::proportional(font_size),
            t.text,
        );
    }
    if collapsed.is_some() && bar.double_clicked() {
        action = HeaderAction::Collapse;
    }
    if docked && bar.dragged() && tears_off(bar.total_drag_delta().unwrap_or_default()) {
        if let Some(at) = bar.interact_pointer_pos() {
            action = HeaderAction::TearOff(None, at);
        }
    }
    // ImGui's title-bar buttons: a font-sized mark with a round `ButtonHovered` behind it on
    // hover. A finger gets a bigger target round the same mark.
    let hit = if touch { 36.0 } else { FRAME_H };
    let mut x = rect.right() - hit * 0.5 - if touch { 6.0 } else { 2.0 };
    #[derive(Clone, Copy)]
    enum Mark<'a> {
        Cross,
        Arrow(bool),
        Glyph(&'a str),
    }
    let mut button = |ui: &mut egui::Ui, mark: Mark<'_>, hint: &str| -> Response {
        let r = Rect::from_center_size(egui::pos2(x, rect.center().y), egui::vec2(hit, hit));
        x -= hit;
        // Keyed on the hint, not the title: a title that carries a count ("Alerts (8)") would
        // otherwise give the same button a new id whenever the count changes.
        let resp = ui.interact(r, ui.id().with(("window_header", hint)), Sense::click());
        let size = if touch { 18.0 } else { FONT };
        let p = ui.painter();
        if resp.hovered() || resp.is_pointer_button_down_on() {
            let fill = if resp.is_pointer_button_down_on() {
                t.im.button_active
            } else {
                t.im.button_hovered
            };
            p.circle_filled(r.center(), size * 0.5 + 1.0, fill);
        }
        match mark {
            Mark::Cross => close_cross(p, r.center(), size, t.text),
            Mark::Arrow(open) => arrow(p, r.center(), size, open, t.text),
            Mark::Glyph(g) => {
                p.text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    g,
                    FontId::proportional(size),
                    t.text,
                );
            }
        }
        let enabled = resp.enabled();
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, hint));
        resp.on_hover_text(hint)
    };
    if button(ui, Mark::Cross, "Close").clicked() {
        action = HeaderAction::Close;
    }
    if let Some(folded) = collapsed {
        // ImGui's collapse triangle: pointing down while open, right while folded.
        let hint = if folded {
            "Unfold"
        } else {
            "Fold to the title bar"
        };
        if button(ui, Mark::Arrow(!folded), hint).clicked() {
            action = HeaderAction::Collapse;
        }
    }
    if let Some(now) = place {
        let menu = button(
            ui,
            Mark::Glyph(egui_phosphor::regular::DOTS_THREE),
            "Move this window",
        );
        egui::Popup::menu(&menu).show(|ui| {
            for (p, label) in [
                (Place::Left, "Dock left"),
                (Place::Right, "Dock right"),
                (Place::Bottom, "Dock bottom"),
                (Place::Float, "Float over the map"),
            ] {
                if ui.selectable_label(now == p, label).clicked() {
                    action = HeaderAction::Place(p);
                }
            }
        });
    }
    action
}

/// An app-bar tab: ImGui's tab, a frame high at the bottom of `height`, its top corners
/// rounded, `TabSelected` with a 2px overline when selected.
pub fn tab(ui: &mut egui::Ui, t: &Tokens, label: &str, selected: bool, height: f32) -> Response {
    let font = FontId::proportional(FONT);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), t.text);
    let size = egui::vec2(galley.size().x + 8.0 + 4.0, height);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let body = Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - CONTROL_H),
        egui::pos2(rect.right() - 4.0, rect.bottom()),
    );
    let fill = if selected {
        t.im.tab_selected
    } else if resp.hovered() {
        t.im.tab_hovered
    } else {
        t.im.tab
    };
    let top = CornerRadius {
        nw: TAB_ROUNDING,
        ne: TAB_ROUNDING,
        sw: 0,
        se: 0,
    };
    ui.painter().rect_filled(body, top, fill);
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(body.min, egui::vec2(body.width(), 2.0)),
            top,
            t.im.tab_selected_overline,
        );
    }
    ui.painter().text(
        body.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        t.text,
    );
    resp
}

/// A glyph-and-label toolbar button (app bar, panel footers): ImGui's toolbar idiom, a `Button`
/// with its resting fill pushed transparent, held in `ButtonActive` while it is on.
pub fn icon_button(ui: &mut egui::Ui, t: &Tokens, glyph: &str, label: &str, on: bool) -> Response {
    let font = FontId::proportional(FONT);
    let text = if label.is_empty() {
        glyph.to_string()
    } else {
        format!("{glyph} {label}")
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text.clone(), font.clone(), t.text);
    let w = if label.is_empty() {
        CONTROL_H.max(galley.size().x + 8.0)
    } else {
        galley.size().x + 8.0
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, CONTROL_H), Sense::click());
    let fill = if on || resp.is_pointer_button_down_on() {
        t.im.button_active
    } else if resp.hovered() {
        t.im.button_hovered
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 0.0, fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        font,
        t.text,
    );
    resp
}

/// ImGui's button (panel footers, card actions): a frame in `Button`, `ButtonHovered` under the
/// pointer, `ButtonActive` while held; no border, no rounding.
pub fn button(ui: &mut egui::Ui, t: &Tokens, label: &str, width: f32) -> Response {
    let font = FontId::proportional(FONT);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), t.text);
    let size = egui::vec2(width.max(galley.size().x + 8.0), CONTROL_H);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    ui.painter()
        .rect_filled(rect, 0.0, button_fill(t, &resp, false));
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        t.text,
    );
    resp
}

/// A tool-rail button: an 18px glyph in an ImGui ImageButton-sized frame, the armed tool held in
/// `ButtonActive`.
pub fn rail_button(ui: &mut egui::Ui, t: &Tokens, glyph: &str, on: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(RAIL_BTN, RAIL_BTN), Sense::click());
    ui.painter()
        .rect_filled(rect, 0.0, button_fill(t, &resp, on));
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(18.0),
        t.text,
    );
    resp
}

/// Joined segment buttons; returns the segment clicked, if any. The selected one is filled with
/// the accent.
pub fn segmented(ui: &mut egui::Ui, t: &Tokens, labels: &[&str], selected: usize) -> Option<usize> {
    let segments: Vec<Segment<'_>> = labels.iter().map(|l| Segment::new(l)).collect();
    segmented_full(ui, t, &segments, Some(selected), false)
}

/// One segment of [`segmented_full`].
#[derive(Debug, Clone, Copy)]
pub struct Segment<'a> {
    pub label: &'a str,
    /// A disabled segment is drawn dimmed and does not click; its `hover` says what brings it.
    pub enabled: bool,
    pub hover: &'a str,
}

impl<'a> Segment<'a> {
    pub fn new(label: &'a str) -> Self {
        Segment {
            label,
            enabled: true,
            hover: "",
        }
    }
}

/// [`segmented`] with per-segment enabled state and hover text, optionally stretched across the
/// row (`fill`), each segment widened in proportion to its label. `selected` may be `None`.
///
/// ImGui's idiom for it: buttons on one line with no spacing between them, the chosen one held in
/// `ButtonActive`; a disabled one stays in the row at `DisabledAlpha`.
pub fn segmented_full(
    ui: &mut egui::Ui,
    t: &Tokens,
    segments: &[Segment<'_>],
    selected: Option<usize>,
    fill: bool,
) -> Option<usize> {
    let font = FontId::proportional(FONT);
    let mut widths: Vec<f32> = segments
        .iter()
        .map(|s| {
            ui.painter()
                .layout_no_wrap(s.label.to_string(), font.clone(), t.text)
                .size()
                .x
                + 8.0
        })
        .collect();
    let natural: f32 = widths.iter().sum();
    if fill && natural > 0.0 {
        let k = ui.available_width() / natural;
        widths.iter_mut().for_each(|w| *w *= k);
    }
    let total: f32 = widths.iter().sum();
    let enabled = ui.is_enabled();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total, CONTROL_H), Sense::hover());
    let mut clicked = None;
    let mut x = rect.left();
    for (i, (seg, w)) in segments.iter().zip(&widths).enumerate() {
        let r = Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(*w, rect.height()));
        x += w;
        let live = enabled && seg.enabled;
        let mut resp = ui.interact(
            r,
            ui.id().with(("seg", i, seg.label)),
            if live { Sense::click() } else { Sense::hover() },
        );
        let on = selected == Some(i);
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, live, on, seg.label)
        });
        let fill = if live {
            button_fill(t, &resp, on)
        } else {
            t.im.button.gamma_multiply(0.6)
        };
        ui.painter().rect_filled(r, 0.0, fill);
        ui.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            seg.label,
            font.clone(),
            if live {
                t.text
            } else {
                t.text.gamma_multiply(0.6)
            },
        );
        if !seg.hover.is_empty() {
            resp = resp.on_hover_text(seg.hover);
        }
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

/// A compact checkbox: an accent-filled box with a tick when on, then the label.
pub fn check(ui: &mut egui::Ui, t: &Tokens, on: &mut bool, label: &str) -> Response {
    check_sized(ui, t, on, label, None)
}

/// [`check`] in a box `width` wide (the label column of a [`prop_toggle`]), or as wide as its
/// label. Dimmed, and inert, inside a disabled `ui`.
///
/// ImGui's checkbox: a frame-high `FrameBg` square, the three-point tick in `CheckMark` when on,
/// the label `ItemInnerSpacing` after it.
fn check_sized(
    ui: &mut egui::Ui,
    t: &Tokens,
    on: &mut bool,
    label: &str,
    width: Option<f32>,
) -> Response {
    let font = FontId::proportional(FONT);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), t.text);
    let size = egui::vec2(
        width.unwrap_or(CONTROL_H + 4.0 + galley.size().x),
        CONTROL_H,
    );
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let checked = *on;
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, checked, label)
    });
    let dim = |c: Color32| if enabled { c } else { c.gamma_multiply(0.6) };
    let bx = Rect::from_min_size(rect.min, egui::vec2(CONTROL_H, CONTROL_H));
    let p = ui.painter_at(rect);
    let fill = if enabled && resp.is_pointer_button_down_on() {
        t.im.frame_bg_active
    } else if enabled && resp.hovered() {
        t.im.frame_bg_hovered
    } else {
        t.im.frame_bg
    };
    p.rect_filled(bx, 0.0, dim(fill));
    if checked {
        check_mark(&p, bx.min, CONTROL_H, dim(t.im.check_mark));
    }
    p.text(
        egui::pos2(bx.right() + 4.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        if enabled { t.text } else { t.text_faint },
    );
    resp
}

/// The label column of a property row ([`prop_row`]): wide enough for "North–south" or a box and "Anomaly".
pub const PROP_LABEL_W: f32 = 88.0;
/// The value box closing a [`prop_slider`] row, the same width on every row so they line up.
pub const PROP_VALUE_W: f32 = 60.0;

/// Lay a property row out in a child exactly as wide as the column, clipped to it, and take only
/// that width from `ui`. A value that comes out wider than its box ("43.4 dBZ") is cut at the
/// column's edge instead of widening it: egui grows a layout's room to fit an over-wide row, so
/// one such row had widened every row after it.
fn prop_container<R>(
    ui: &mut egui::Ui,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let room = Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), CONTROL_H));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(room)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let clip = ui.clip_rect();
    child.set_clip_rect(Rect::from_x_y_ranges(
        room.x_range().intersection(clip.x_range()),
        clip.y_range(),
    ));
    let inner = add(&mut child);
    let used = Rect::from_min_size(
        room.min,
        egui::vec2(room.width(), child.min_rect().height().max(CONTROL_H)),
    );
    InnerResponse::new(inner, ui.allocate_rect(used, Sense::hover()))
}

/// A property row in the Dear ImGui editor manner: the label dim in a fixed column, the control
/// filling the rest, so a panel of rows reads as two aligned columns.
pub fn prop_row<R>(
    ui: &mut egui::Ui,
    t: &Tokens,
    label: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    prop_container(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(PROP_LABEL_W, CONTROL_H), Sense::hover());
        let color = if ui.is_enabled() {
            t.text_dim
        } else {
            t.text_faint
        };
        ui.painter_at(rect).text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::proportional(FONT),
            color,
        );
        ui.spacing_mut().interact_size.x = PROP_VALUE_W;
        add(ui)
    })
}

/// The slider width that leaves `trailing` points, and the value box, at the end of the row.
pub fn prop_slider_width(ui: &egui::Ui, trailing: f32) -> f32 {
    let gap = ui.spacing().item_spacing.x;
    (ui.available_width() - PROP_VALUE_W - gap - trailing).max(20.0)
}

/// A slider in a [`prop_row`]: the track fills the row and the typeable value box closes it.
/// Give the slider no `.text()`; the row's label is its name.
pub fn prop_slider(
    ui: &mut egui::Ui,
    t: &Tokens,
    label: &str,
    slider: egui::Slider<'_>,
) -> Response {
    prop_row(ui, t, label, |ui| {
        ui.spacing_mut().slider_width = prop_slider_width(ui, 0.0);
        self::slider(ui, t, slider)
    })
    .inner
}

/// An egui slider with ImGui's solid grab: [`crate::theme::slider`], in this panel's style.
pub fn slider(ui: &mut egui::Ui, _t: &Tokens, slider: egui::Slider<'_>) -> Response {
    crate::theme::slider(ui, slider)
}

/// A property row whose label is a checkbox: the control beside it is dimmed and inert while it
/// is off, rather than hidden, so the panel does not jump when it is ticked. Returns the
/// checkbox's response.
pub fn prop_toggle(
    ui: &mut egui::Ui,
    t: &Tokens,
    on: &mut bool,
    label: &str,
    add: impl FnOnce(&mut egui::Ui),
) -> Response {
    prop_container(ui, |ui| {
        let resp = check_sized(ui, t, on, label, Some(PROP_LABEL_W));
        let live = *on;
        ui.add_enabled_ui(live, |ui| {
            ui.spacing_mut().interact_size.x = PROP_VALUE_W;
            add(ui)
        });
        resp
    })
    .inner
}

/// A square glyph button for the end of a property row (reset, face north): ImGui's button, one
/// frame square.
pub fn glyph_button(ui: &mut egui::Ui, t: &Tokens, glyph: &str, label: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(CONTROL_H, CONTROL_H), Sense::click());
    let enabled = ui.is_enabled();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let fill = if enabled {
        button_fill(t, &resp, false)
    } else {
        t.im.button.gamma_multiply(0.6)
    };
    ui.painter().rect_filled(rect, 0.0, fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(FONT),
        if enabled { t.text } else { t.text_faint },
    );
    resp
}

/// A hint or status line in a panel: the one text size, wrapped to the column rather than
/// widening it.
pub fn note(ui: &mut egui::Ui, t: &Tokens, s: impl Into<String>) -> Response {
    ui.add(egui::Label::new(text(s, FONT, t.text_dim)).wrap())
}

/// A fraction slider for a dense row, drawn as ImGui's SliderFloat without its text: a frame in
/// `FrameBg` with a `GrabMinSize` grab in `SliderGrab` (`SliderGrabActive` while dragged). Drag
/// or click to set it; with focus, the arrow keys step by 5 %. `value` stays within `min..=1`,
/// and the response is marked changed only when it moved.
pub fn fader(
    ui: &mut egui::Ui,
    t: &Tokens,
    value: &mut f32,
    min: f32,
    width: f32,
    label: &str,
) -> Response {
    const GRAB: f32 = 12.0; // ImGuiStyle::GrabMinSize
    let (rect, mut resp) =
        ui.allocate_exact_size(egui::vec2(width, CONTROL_H), Sense::click_and_drag());
    let track = rect.shrink2(egui::vec2(2.0 + GRAB * 0.5, 0.0));
    let before = *value;
    if let Some(p) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() {
            *value = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
        }
    }
    if resp.has_focus() {
        let step = ui.input(|i| {
            i.num_presses(egui::Key::ArrowRight) as f32 + i.num_presses(egui::Key::ArrowUp) as f32
                - i.num_presses(egui::Key::ArrowLeft) as f32
                - i.num_presses(egui::Key::ArrowDown) as f32
        });
        *value += step * 0.05;
    }
    *value = value.clamp(min, 1.0);
    if (*value - before).abs() > f32::EPSILON {
        resp.mark_changed();
    }
    let v = *value;
    resp.widget_info(|| egui::WidgetInfo::slider(true, f64::from(v), label));
    let active = resp.dragged() || resp.is_pointer_button_down_on();
    let p = ui.painter();
    let frame = if active {
        t.im.frame_bg_active
    } else if resp.hovered() || resp.has_focus() {
        t.im.frame_bg_hovered
    } else {
        t.im.frame_bg
    };
    p.rect_filled(rect, 0.0, frame);
    let at = track.left() + track.width() * v;
    let grab = Rect::from_center_size(
        egui::pos2(at, rect.center().y),
        egui::vec2(GRAB, rect.height() - 4.0),
    );
    p.rect_filled(
        grab,
        0.0,
        if active {
            t.im.slider_grab_active
        } else {
            t.im.slider_grab
        },
    );
    resp
}

/// A label before a control ("Site:"): plain text at the one size.
pub fn caption(ui: &mut egui::Ui, t: &Tokens, label: &str) {
    ui.label(text(label, FONT, t.text_dim));
}

/// An inspector row: the key in a fixed column, the value beside it in monospace.
pub fn kv(ui: &mut egui::Ui, t: &Tokens, key: &str, value: &str, color: Option<Color32>) {
    kv_row(ui, t, key, mono(value, FONT, color.unwrap_or(t.text)));
}

/// An inspector row whose value is words rather than data (a place name), in the text face.
pub fn kv_text(ui: &mut egui::Ui, t: &Tokens, key: &str, value: &str) {
    kv_row(ui, t, key, text(value, FONT, t.text));
}

fn kv_row(ui: &mut egui::Ui, t: &Tokens, key: &str, value: egui::RichText) {
    ui.horizontal(|ui| {
        // Wide enough for the longest key ("Beam height", "Track error"), no wider: the value
        // beside it is what gets cut short when the column is too generous. A row is a table
        // row's height: the font plus `CellPadding.y` twice.
        let (rect, _) = ui.allocate_exact_size(egui::vec2(76.0, FONT + 4.0), Sense::hover());
        ui.painter().text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            key,
            FontId::proportional(FONT),
            t.text_dim,
        );
        // A long value (a VCP's full name, a volume file) is cut with an ellipsis and shown whole
        // on hover, rather than widening the card or the docked column it sits in.
        ui.add(egui::Label::new(value).truncate());
    });
}

/// A section's heading inside a card or panel: ImGui's SeparatorText, the caption inset 20px with
/// a 3px `Separator` rule either side of it.
pub fn section_rule(ui: &mut egui::Ui, t: &Tokens, label: &str) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), CONTROL_H), Sense::hover());
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), FontId::proportional(FONT), t.text);
    let tx = rect.left() + 20.0; // ImGuiStyle::SeparatorTextPadding.x
    let rule = |p: &egui::Painter, x0: f32, x1: f32| {
        if x1 > x0 {
            p.rect_filled(
                Rect::from_min_max(
                    egui::pos2(x0, rect.center().y - 1.5),
                    egui::pos2(x1, rect.center().y + 1.5),
                ),
                0.0,
                t.line_soft,
            );
        }
    };
    let p = ui.painter();
    rule(p, rect.left(), tx - 8.0);
    rule(p, tx + galley.size().x + 8.0, rect.right());
    p.galley(
        egui::pos2(tx, rect.center().y - galley.size().y / 2.0),
        galley,
        t.text,
    );
}

/// A section that folds: ImGui's CollapsingHeader, a frame-high bar in the `Header` roles with
/// the fold triangle, open state remembered per `id`. `count` (when given) sits at the right end,
/// e.g. how many rows are inside.
pub fn fold_section<R>(
    ui: &mut egui::Ui,
    t: &Tokens,
    id: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    count: Option<&str>,
    default_open: bool,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    let id = ui.make_persistent_id(("ws_fold", id));
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        id,
        default_open,
    );
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), CONTROL_H), Sense::click());
    let open = state.is_open();
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, true, open, label)
    });
    if resp.clicked() {
        state.toggle(ui);
    }
    let p = ui.painter();
    let fill = if resp.is_pointer_button_down_on() {
        t.im.header_active
    } else if resp.hovered() {
        t.im.header_hovered
    } else {
        t.im.header
    };
    p.rect_filled(rect, 0.0, fill);
    arrow(
        p,
        egui::pos2(rect.left() + 4.0 + FONT * 0.5, rect.center().y),
        FONT,
        open,
        t.text,
    );
    p.text(
        egui::pos2(rect.left() + 4.0 + FONT + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(FONT),
        t.text,
    );
    if let Some(c) = count {
        p.text(
            egui::pos2(rect.right() - 4.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            c,
            FontId::monospace(FONT),
            t.text,
        );
    }
    state.store(ui.ctx());
    state.show_body_unindented(ui, body).map(|r| r.inner)
}

/// One point of a [`trend_chart`]: its value and the label the hover shows for it (a time).
#[derive(Clone, Debug, PartialEq)]
pub struct TrendPoint {
    pub label: String,
    pub value: f32,
}

/// The nearest point to `x` in a chart `n` points wide spread over `left..right`.
pub fn nearest_index(x: f32, left: f32, right: f32, n: usize) -> usize {
    if n < 2 || right <= left {
        return 0;
    }
    (((x - left) / (right - left)) * (n - 1) as f32)
        .round()
        .clamp(0.0, (n - 1) as f32) as usize
}

/// An interactive trend: compact (a sparkline with the latest value and its change) until
/// clicked, then expanded with gridlines, the range, and a point per sample. Hovering either
/// form marks the nearest sample and reads its label, value and change from the one before.
/// `expanded` is remembered per `id`.
pub fn trend_chart(
    ui: &mut egui::Ui,
    t: &Tokens,
    id: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    unit: &str,
    points: &[TrendPoint],
    color: Color32,
) -> Response {
    let key = ui.make_persistent_id(("ws_trend", id));
    let mut expanded = ui.ctx().data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    let h = if expanded { 140.0 } else { 48.0 };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() {
        expanded = !expanded;
        ui.ctx().data_mut(|d| d.insert_temp(key, expanded));
    }
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, t.field);
    if resp.hovered() {
        p.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, t.line),
            egui::StrokeKind::Inside,
        );
    }
    let last = points.last();
    let head = match (last, points.len().checked_sub(2).map(|i| &points[i])) {
        (Some(l), Some(prev)) => {
            let d = l.value - prev.value;
            let arrow = if d > 0.0 {
                "\u{25b2}"
            } else if d < 0.0 {
                "\u{25bc}"
            } else {
                "="
            };
            format!("{:.1} {unit}  {arrow}{:.1}", l.value, d.abs())
        }
        (Some(l), None) => format!("{:.1} {unit}", l.value),
        _ => "\u{2014}".into(),
    };
    p.text(
        rect.left_top() + egui::vec2(6.0, 3.0),
        egui::Align2::LEFT_TOP,
        title,
        FontId::proportional(FONT),
        t.text_dim,
    );
    p.text(
        rect.right_top() + egui::vec2(-6.0, 3.0),
        egui::Align2::RIGHT_TOP,
        head,
        FontId::monospace(FONT),
        t.text,
    );
    if points.len() < 2 {
        p.text(
            rect.center() + egui::vec2(0.0, 6.0),
            egui::Align2::CENTER_CENTER,
            "needs two volumes",
            FontId::proportional(FONT),
            t.text_faint,
        );
        return resp.named_toggle_info(title, expanded);
    }
    let plot = Rect::from_min_max(
        rect.left_top()
            + egui::vec2(
                if expanded { 34.0 } else { 6.0 },
                if expanded { 28.0 } else { 20.0 },
            ),
        rect.right_bottom() - egui::vec2(6.0, if expanded { 18.0 } else { 5.0 }),
    );
    let (lo, hi) = points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), q| {
        (lo.min(q.value), hi.max(q.value))
    });
    let (lo, hi) = if hi > lo {
        (lo, hi)
    } else {
        (lo - 1.0, hi + 1.0)
    };
    let n = points.len();
    let at = |i: usize, v: f32| {
        egui::pos2(
            plot.left() + plot.width() * i as f32 / (n - 1) as f32,
            plot.bottom() - plot.height() * (v - lo) / (hi - lo),
        )
    };
    if expanded {
        for k in 0..=3 {
            let v = lo + (hi - lo) * k as f32 / 3.0;
            let y = plot.bottom() - plot.height() * k as f32 / 3.0;
            p.line_segment(
                [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
                Stroke::new(1.0, t.line_soft),
            );
            p.text(
                egui::pos2(plot.left() - 4.0, y),
                egui::Align2::RIGHT_CENTER,
                format!("{v:.0}"),
                FontId::monospace(FONT),
                t.text_faint,
            );
        }
        for (i, label) in [(0, &points[0].label), (n - 1, &points[n - 1].label)] {
            p.text(
                egui::pos2(at(i, lo).x, rect.bottom() - 2.0),
                if i == 0 {
                    egui::Align2::LEFT_BOTTOM
                } else {
                    egui::Align2::RIGHT_BOTTOM
                },
                label,
                FontId::monospace(FONT),
                t.text_faint,
            );
        }
    }
    let line: Vec<egui::Pos2> = points
        .iter()
        .enumerate()
        .map(|(i, q)| at(i, q.value))
        .collect();
    p.add(egui::Shape::line(line.clone(), Stroke::new(1.6, color)));
    if expanded {
        for q in &line {
            p.circle_filled(*q, 2.2, color);
        }
    }
    let mut resp = resp;
    if let Some(pos) = resp.hover_pos() {
        let i = nearest_index(pos.x, plot.left(), plot.right(), n);
        let q = line[i];
        p.line_segment(
            [egui::pos2(q.x, plot.top()), egui::pos2(q.x, plot.bottom())],
            Stroke::new(1.0, t.text_faint),
        );
        p.circle_filled(q, 3.5, t.text);
        let d = if i > 0 {
            format!(
                " ({:+.1} from the scan before)",
                points[i].value - points[i - 1].value
            )
        } else {
            String::new()
        };
        resp = resp.on_hover_text(format!(
            "{}: {:.1} {unit}{d}\n{}",
            points[i].label,
            points[i].value,
            if expanded {
                "Click to fold"
            } else {
                "Click to expand"
            }
        ));
    }
    resp.named_toggle_info(title, expanded)
}

/// One line of a [`series_chart`]: its name (for the hover), colour, and `(x, value)` points in
/// ascending x — x is seconds (a Unix time), so lines sampled at different scans still line up.
pub struct Series<'a> {
    pub name: &'a str,
    pub color: Color32,
    pub points: &'a [(f64, f32)],
}

/// The index of the point in `points` (ascending x) nearest to `x`.
pub fn nearest_by_x(points: &[(f64, f32)], x: f64) -> Option<usize> {
    if points.is_empty() {
        return None;
    }
    let i = points.partition_point(|p| p.0 < x);
    Some(match i {
        0 => 0,
        i if i >= points.len() => points.len() - 1,
        i if (points[i].0 - x) < (x - points[i - 1].0) => i,
        i => i - 1,
    })
}

/// A trend over time with one or more lines: a compact sparkline (latest value and its change
/// for one line, how many lines for several) until clicked, then expanded with gridlines, the
/// time range and a point per sample. Hovering marks the pointer's time and reads each line's
/// nearest sample and its change from the one before; `fmt_x` writes a time. Expansion is
/// remembered per `id`.
pub fn series_chart(
    ui: &mut egui::Ui,
    t: &Tokens,
    id: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    unit: &str,
    series: &[Series<'_>],
    fmt_x: &dyn Fn(f64) -> String,
) -> Response {
    let key = ui.make_persistent_id(("ws_series", id));
    let mut expanded = ui.ctx().data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    let h = if expanded { 156.0 } else { 52.0 };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() {
        expanded = !expanded;
        ui.ctx().data_mut(|d| d.insert_temp(key, expanded));
    }
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, t.field);
    if resp.hovered() {
        p.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, t.line),
            egui::StrokeKind::Inside,
        );
    }
    let lines: Vec<&Series> = series.iter().filter(|s| !s.points.is_empty()).collect();
    let head = match lines.as_slice() {
        [one] => {
            let n = one.points.len();
            let last = one.points[n - 1].1;
            match n.checked_sub(2).map(|i| one.points[i].1) {
                Some(prev) => {
                    let d = last - prev;
                    let arrow = if d > 0.0 {
                        "\u{25b2}"
                    } else if d < 0.0 {
                        "\u{25bc}"
                    } else {
                        "="
                    };
                    format!("{last:.1} {unit}  {arrow}{:.1}", d.abs())
                }
                None => format!("{last:.1} {unit}"),
            }
        }
        [] => "\u{2014}".into(),
        many => format!("{} lines \u{b7} {unit}", many.len()),
    };
    p.text(
        rect.left_top() + egui::vec2(6.0, 3.0),
        egui::Align2::LEFT_TOP,
        title,
        FontId::proportional(FONT),
        t.text_dim,
    );
    p.text(
        rect.right_top() + egui::vec2(-6.0, 3.0),
        egui::Align2::RIGHT_TOP,
        head,
        FontId::monospace(FONT),
        t.text,
    );
    let total: usize = lines.iter().map(|s| s.points.len()).sum();
    let (x0, x1) = lines
        .iter()
        .flat_map(|s| s.points.iter())
        .fold((f64::MAX, f64::MIN), |(a, b), q| (a.min(q.0), b.max(q.0)));
    if total < 2 || x1 <= x0 {
        p.text(
            rect.center() + egui::vec2(0.0, 6.0),
            egui::Align2::CENTER_CENTER,
            "needs two scans",
            FontId::proportional(FONT),
            t.text_faint,
        );
        return resp.named_toggle_info(title, expanded);
    }
    let (lo, hi) = lines
        .iter()
        .flat_map(|s| s.points.iter())
        .fold((f32::MAX, f32::MIN), |(a, b), q| (a.min(q.1), b.max(q.1)));
    let (lo, hi) = if hi > lo {
        (lo, hi)
    } else {
        (lo - 1.0, hi + 1.0)
    };
    let plot = Rect::from_min_max(
        rect.left_top()
            + egui::vec2(
                if expanded { 34.0 } else { 6.0 },
                if expanded { 28.0 } else { 20.0 },
            ),
        rect.right_bottom() - egui::vec2(6.0, if expanded { 18.0 } else { 5.0 }),
    );
    let at = |x: f64, v: f32| {
        egui::pos2(
            plot.left() + plot.width() * ((x - x0) / (x1 - x0)) as f32,
            plot.bottom() - plot.height() * (v - lo) / (hi - lo),
        )
    };
    if expanded {
        for k in 0..=3 {
            let v = lo + (hi - lo) * k as f32 / 3.0;
            let y = plot.bottom() - plot.height() * k as f32 / 3.0;
            p.line_segment(
                [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
                Stroke::new(1.0, t.line_soft),
            );
            p.text(
                egui::pos2(plot.left() - 4.0, y),
                egui::Align2::RIGHT_CENTER,
                format!("{v:.0}"),
                FontId::monospace(FONT),
                t.text_faint,
            );
        }
        for (x, align) in [
            (x0, egui::Align2::LEFT_BOTTOM),
            (x1, egui::Align2::RIGHT_BOTTOM),
        ] {
            p.text(
                egui::pos2(at(x, lo).x, rect.bottom() - 2.0),
                align,
                fmt_x(x),
                FontId::monospace(FONT),
                t.text_faint,
            );
        }
    }
    for s in &lines {
        let pts: Vec<egui::Pos2> = s.points.iter().map(|q| at(q.0, q.1)).collect();
        if pts.len() == 1 {
            p.circle_filled(pts[0], 2.5, s.color);
        } else {
            p.add(egui::Shape::line(pts.clone(), Stroke::new(1.6, s.color)));
        }
        if expanded {
            for q in &pts {
                p.circle_filled(*q, 2.0, s.color);
            }
        }
    }
    let mut resp = resp;
    if let Some(pos) = resp.hover_pos() {
        let x = x0 + (x1 - x0) * ((pos.x - plot.left()) / plot.width()).clamp(0.0, 1.0) as f64;
        let xp = at(x, lo).x;
        p.line_segment(
            [egui::pos2(xp, plot.top()), egui::pos2(xp, plot.bottom())],
            Stroke::new(1.0, t.text_faint),
        );
        let mut text = fmt_x(x);
        for s in &lines {
            let Some(i) = nearest_by_x(s.points, x) else {
                continue;
            };
            let (qx, qv) = s.points[i];
            p.circle_filled(at(qx, qv), 3.5, t.text);
            p.circle_filled(at(qx, qv), 2.5, s.color);
            let d = if i > 0 {
                format!(" ({:+.1})", qv - s.points[i - 1].1)
            } else {
                String::new()
            };
            let name = if lines.len() > 1 {
                format!("{}: ", s.name)
            } else {
                String::new()
            };
            text.push_str(&format!("\n{name}{qv:.1} {unit}{d} at {}", fmt_x(qx)));
        }
        text.push_str(if expanded {
            "\nClick to fold"
        } else {
            "\nClick to expand"
        });
        resp = resp.on_hover_text(text);
    }
    resp.named_toggle_info(title, expanded)
}

trait ToggleInfo {
    fn named_toggle_info(self, name: &str, on: bool) -> Response;
}
impl ToggleInfo for Response {
    fn named_toggle_info(self, name: &str, on: bool) -> Response {
        self.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, name));
        self
    }
}

/// A row of ImGui SmallButtons that wraps to the width (moment pickers, quick pairs); returns the
/// one clicked, which is held in `ButtonActive`. `selected` None highlights none.
pub fn chips(
    ui: &mut egui::Ui,
    t: &Tokens,
    labels: &[&str],
    selected: Option<usize>,
) -> Option<usize> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
        let font = FontId::proportional(FONT);
        for (i, label) in labels.iter().enumerate() {
            let galley = ui
                .painter()
                .layout_no_wrap(label.to_string(), font.clone(), t.text);
            // SmallButton: no vertical frame padding.
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(galley.size().x + 8.0, galley.size().y.max(FONT)),
                Sense::click(),
            );
            let on = selected == Some(i);
            ui.painter()
                .rect_filled(rect, 0.0, button_fill(t, &resp, on));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                *label,
                font.clone(),
                t.text,
            );
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            resp.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, *label)
            });
            if resp.clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

/// A state word with a bullet in front ("● Live"), as ImGui's BulletText drawn in the state's
/// colour: no fill, the word always there, so colour is never the only signal.
pub fn badge(ui: &mut egui::Ui, t: &Tokens, label: &str, color: Color32) -> Response {
    let font = FontId::proportional(FONT);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), t.text);
    // BulletText: the bullet centred in a font-sized square after `FramePadding.x`, the text
    // after the square and `FramePadding.x` twice.
    let size = egui::vec2(FONT + 8.0 + galley.size().x + 4.0, CONTROL_H);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().circle_filled(
        egui::pos2(rect.left() + 4.0 + FONT * 0.5, rect.center().y),
        FONT * 0.2,
        color,
    );
    ui.painter().text(
        egui::pos2(rect.left() + FONT + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        color,
    );
    resp
}

/// A status dot of radius `r`, vertically centred in a control-height slot.
pub fn status_dot(ui: &mut egui::Ui, color: Color32, r: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(r * 2.0 + 4.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), r, color);
}

/// A vertical 1px `Separator` between control groups in a horizontal row.
pub fn divider(ui: &mut egui::Ui, t: &Tokens, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, height), Sense::hover());
    ui.painter().line_segment(
        [
            egui::pos2(rect.center().x, rect.top() + 4.0),
            egui::pos2(rect.center().x, rect.bottom() - 4.0),
        ],
        Stroke::new(1.0, t.line_soft),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `f` for three frames of a 900 x 600 screen and return every string drawn in the last.
    fn texts(f: impl Fn(&mut egui::Ui)) -> Vec<String> {
        let ctx = egui::Context::default();
        let mut out = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 600.0),
                )),
                ..Default::default()
            };
            let o = ctx.run_ui(input, |ui| f(ui));
            out = o
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
                    _ => None,
                })
                .collect();
        }
        out
    }

    fn t() -> Tokens {
        Tokens::new(Color32::from_rgb(0x2F, 0x81, 0xF7))
    }

    #[test]
    fn components_draw_their_labels() {
        let got = texts(|ui| {
            let t = t();
            window_header(
                ui,
                &t,
                egui_phosphor::regular::STACK,
                "Layers",
                Some(crate::workspace::Place::Left),
                Some(false),
            );
            tab(ui, &t, "Radar", true, APP_BAR_H);
            icon_button(ui, &t, egui_phosphor::regular::GEAR, "Settings", false);
            segmented(ui, &t, &["2D", "3D", "Volume"], 0);
            let mut on = true;
            check(ui, &t, &mut on, "Warnings");
            kv(ui, &t, "Site:", "KTLX", None);
            kv_text(ui, &t, "City:", "Oklahoma City");
            badge(ui, &t, "Live", t.live);
        });
        for want in [
            "Layers", "Radar", "Settings", "2D", "3D", "Volume", "Warnings", "Site:", "KTLX",
            "Live",
        ] {
            assert!(
                got.iter().any(|s| s.contains(want)),
                "{want} missing: {got:?}"
            );
        }
    }

    #[test]
    fn chips_draw_every_choice() {
        let got = texts(|ui| {
            let t = t();
            assert_eq!(chips(ui, &t, &["REF", "ZDR", "CC"], Some(1)), None);
        });
        for l in ["REF", "ZDR", "CC"] {
            assert!(got.iter().any(|s| s == l), "{got:?}");
        }
    }

    #[test]
    fn a_series_reads_the_sample_nearest_in_time() {
        let pts = [(0.0, 1.0), (300.0, 2.0), (900.0, 3.0)];
        assert_eq!(nearest_by_x(&pts, -50.0), Some(0));
        assert_eq!(nearest_by_x(&pts, 140.0), Some(0));
        assert_eq!(nearest_by_x(&pts, 160.0), Some(1));
        assert_eq!(nearest_by_x(&pts, 700.0), Some(2));
        assert_eq!(nearest_by_x(&pts, 5000.0), Some(2));
        assert_eq!(nearest_by_x(&[], 1.0), None);
    }

    #[test]
    fn a_series_chart_heads_one_line_with_its_change_and_several_with_a_count() {
        let a = [(0.0, 50.0), (300.0, 55.0)];
        let b = [(100.0, 40.0), (400.0, 42.0)];
        let got = texts(|ui| {
            let t = t();
            let one = [Series {
                name: "B2",
                color: t.accent,
                points: &a,
            }];
            series_chart(ui, &t, "one", "Peak", "dBZ", &one, &|x| format!("{x}"));
            let two = [
                Series {
                    name: "B2",
                    color: t.accent,
                    points: &a,
                },
                Series {
                    name: "S6",
                    color: t.warn,
                    points: &b,
                },
            ];
            series_chart(ui, &t, "two", "Peak", "dBZ", &two, &|x| format!("{x}"));
        });
        assert!(
            got.iter()
                .any(|s| s.starts_with("55.0 dBZ") && s.contains("5.0")),
            "{got:?}"
        );
        assert!(got.iter().any(|s| s.starts_with("2 lines")), "{got:?}");
    }

    #[test]
    fn a_chart_reads_the_nearest_sample() {
        assert_eq!(nearest_index(0.0, 0.0, 100.0, 5), 0);
        assert_eq!(nearest_index(49.0, 0.0, 100.0, 5), 2);
        assert_eq!(nearest_index(500.0, 0.0, 100.0, 5), 4);
        assert_eq!(nearest_index(10.0, 0.0, 100.0, 1), 0);
    }

    #[test]
    fn a_folded_section_hides_its_body_and_a_chart_draws_its_latest_change() {
        let got = texts(|ui| {
            let t = t();
            fold_section(ui, &t, "a", "Core", Some("3"), true, |ui| {
                ui.label("inside");
            });
            fold_section(ui, &t, "b", "Hidden", None, false, |ui| {
                ui.label("never drawn");
            });
            let pts: Vec<TrendPoint> = [50.0, 55.0, 52.0]
                .iter()
                .enumerate()
                .map(|(i, v)| TrendPoint {
                    label: format!("{i}"),
                    value: *v,
                })
                .collect();
            trend_chart(ui, &t, "dbz", "Reflectivity", "dBZ", &pts, t.accent);
        });
        // ImGui's collapsing header keeps the label as written: no uppercase.
        assert!(got.iter().any(|s| s == "Core") && got.iter().any(|s| s == "inside"));
        assert!(!got.iter().any(|s| s == "never drawn"));
        assert!(
            got.iter()
                .any(|s| s.starts_with("52.0 dBZ") && s.contains("3.0")),
            "{got:?}"
        );
    }

    #[test]
    fn a_crowded_tab_group_keeps_the_front_title_and_shrinks_the_rest() {
        assert_eq!(tab_widths(&[80.0, 90.0], 1, 200.0), [80.0, 90.0]);
        assert_eq!(
            tab_widths(&[80.0, 90.0, 100.0], 1, 200.0),
            [32.0, 90.0, 32.0]
        );
    }

    #[test]
    fn a_header_given_tabs_draws_every_title_once() {
        let got = texts(|ui| {
            set_header_tabs(
                ui.ctx(),
                Some(HeaderTabs {
                    tabs: vec![
                        HeaderTab {
                            glyph: egui_phosphor::regular::INFO,
                            title: "Inspector",
                            dot: None,
                        },
                        HeaderTab {
                            glyph: egui_phosphor::regular::WARNING,
                            title: "Alerts",
                            dot: None,
                        },
                    ],
                    front: 1,
                }),
            );
            window_header(
                ui,
                &t(),
                egui_phosphor::regular::WARNING,
                "Alerts (6)",
                Some(crate::workspace::Place::Right),
                None,
            );
            // The tabs were for that header alone: the next one is a plain title again.
            window_header(
                ui,
                &t(),
                egui_phosphor::regular::GEAR,
                "Preferences",
                None,
                None,
            );
        });
        for want in ["Inspector", "Alerts (6)", "Preferences"] {
            assert_eq!(
                got.iter().filter(|s| s.as_str() == want).count(),
                1,
                "{want}: {got:?}"
            );
        }
        assert!(
            !got.iter().any(|s| s == "Alerts"),
            "the front tab shows its live title"
        );
    }

    #[test]
    fn a_fader_never_leaves_its_range() {
        let ctx = egui::Context::default();
        let mut v = 1.4_f32;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            fader(ui, &t(), &mut v, 0.05, 90.0, "Opacity");
        });
        assert_eq!(v, 1.0);
        v = -3.0;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            fader(ui, &t(), &mut v, 0.05, 90.0, "Opacity");
        });
        assert_eq!(
            v, 0.05,
            "an invisible layer is a removed one, not a faded one"
        );
    }

    /// Property rows fill a narrow column without running past it: the 3D view's rows had pushed
    /// its docked column into scrolling sideways.
    #[test]
    fn property_rows_stay_inside_their_column() {
        let ctx = egui::Context::default();
        let column = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(240.0, 600.0));
        let mut widest = 0.0_f32;
        let mut v = 0.72_f32;
        let mut on = false;
        let _ = ctx.run_ui(egui::RawInput::default(), |root| {
            let t = t();
            let mut ui = root.new_child(egui::UiBuilder::new().max_rect(column));
            style_scope(&mut ui, &t);
            let rows = [
                prop_slider(&mut ui, &t, "Opacity", egui::Slider::new(&mut v, 0.1..=1.0)).rect,
                prop_toggle(&mut ui, &t, &mut on, "CC anomaly", |ui| {
                    ui.spacing_mut().slider_width = prop_slider_width(ui, 0.0);
                    crate::theme::slider(ui, egui::Slider::new(&mut v, 0.0..=1.0));
                })
                .rect,
            ];
            segmented_full(
                &mut ui,
                &t,
                &[
                    Segment::new("Observed"),
                    Segment::new("Volume"),
                    Segment::new("User"),
                ],
                Some(0),
                true,
            );
            widest = rows
                .iter()
                .map(|r| r.right())
                .fold(ui.min_rect().right(), f32::max);
        });
        assert!(
            widest <= column.right() + 0.5,
            "{widest} > {}",
            column.right()
        );
    }

    #[test]
    fn a_disabled_segment_does_not_click() {
        let ctx = egui::Context::default();
        let segs = [
            Segment::new("Observed"),
            Segment {
                label: "Volume",
                enabled: false,
                hover: "",
            },
        ];
        let at = egui::pos2(175.0, 16.0);
        let mut got = Vec::new();
        for events in [
            vec![egui::Event::PointerMoved(at)],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        ] {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                ui.set_width(200.0);
                got.push(segmented_full(ui, &t(), &segs, Some(0), true));
            });
        }
        assert_eq!(got, [None, None, None], "the greyed half clicked");
    }

    #[test]
    fn the_selected_row_wash_is_imgui_s_header_over_the_window() {
        let t = t();
        assert_eq!(t.accent_soft(), t.im.header);
        // Composited over the window, so opaque, and a different colour from the body it marks.
        assert_eq!(t.accent_soft().a(), 255);
        assert_ne!(t.accent_soft(), t.panel);
        assert_ne!(
            t.panel, t.panel_hi,
            "a header must stand apart from its body"
        );
    }

    /// Draw a header docked right for a few frames while the pointer presses on it at `from` and
    /// moves to each of `path`; what the header said on each frame.
    fn drag_header(
        place: crate::workspace::Place,
        from: egui::Pos2,
        path: &[egui::Pos2],
    ) -> Vec<HeaderAction> {
        let ctx = egui::Context::default();
        let t = Tokens::new(Color32::from_rgb(70, 130, 230));
        let mut out = Vec::new();
        let mut events: Vec<Vec<egui::Event>> = vec![
            vec![egui::Event::PointerMoved(from)],
            vec![egui::Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
        ];
        for p in path {
            events.push(vec![egui::Event::PointerMoved(*p)]);
        }
        for (i, ev) in events.into_iter().enumerate() {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 300.0),
                )),
                events: ev,
                time: Some(i as f64 * 0.05),
                ..Default::default()
            };
            let mut a = HeaderAction::None;
            let _ = ctx.run_ui(input, |ui| {
                a = window_header(ui, &t, "i", "Inspector", Some(place), None);
            });
            out.push(a);
        }
        out
    }

    #[test]
    fn a_docked_header_dragged_far_enough_tears_off() {
        use crate::workspace::Place;
        let from = egui::pos2(60.0, 10.0);
        let path = [
            egui::pos2(64.0, 14.0),
            egui::pos2(80.0, 40.0),
            egui::pos2(120.0, 90.0),
        ];
        let said = drag_header(Place::Right, from, &path);
        // A few points of wobble is still a click; past the threshold it tears off, under the
        // pointer.
        assert_eq!(said[2], HeaderAction::None, "{said:?}");
        assert!(
            said.iter()
                .any(|a| matches!(a, HeaderAction::TearOff(None, p) if p.y > 30.0)),
            "{said:?}"
        );
        // A floating window's header leaves the drag to the window: it never tears off.
        let floating = drag_header(Place::Float, from, &path);
        assert!(
            floating.iter().all(|a| *a == HeaderAction::None),
            "{floating:?}"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes the workstation components in each Dear ImGui style for visual review"]
    fn gpu_imgui_component_snapshots() {
        use crate::settings::Theme;
        use egui_phosphor::regular as ph;
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for UI review");
        let destination =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-review/imgui");
        std::fs::create_dir_all(&destination).unwrap();
        for theme in [Theme::Dark, Theme::Light, Theme::Classic] {
            let t = Tokens::of(crate::theme::palette(theme, true), None);
            gpu.save(
                &destination.join(format!("components-{}.png", theme.label().to_lowercase())),
                520,
                760,
                |ui| {
                    egui::Frame::NONE
                        .fill(crate::theme::VIEWPORT_BG)
                        .inner_margin(10)
                        .show(ui, |ui| {
                            panel_frame(&t).show(ui, |ui| {
                                style_scope(ui, &t);
                                set_header_tabs(
                                    ui.ctx(),
                                    Some(HeaderTabs {
                                        tabs: vec![
                                            HeaderTab {
                                                glyph: ph::CROSSHAIR,
                                                title: "Inspector",
                                                dot: None,
                                            },
                                            HeaderTab {
                                                glyph: ph::WARNING,
                                                title: "Alerts",
                                                dot: Some(t.warn),
                                            },
                                            HeaderTab {
                                                glyph: ph::NOTEBOOK,
                                                title: "Log",
                                                dot: None,
                                            },
                                        ],
                                        front: 0,
                                    }),
                                );
                                window_header(
                                    ui,
                                    &t,
                                    ph::CROSSHAIR,
                                    "Inspector",
                                    Some(crate::workspace::Place::Right),
                                    None,
                                );
                                egui::Frame::NONE.inner_margin(8).show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        for (i, l) in
                                            ["Radar", "Models", "Satellite"].iter().enumerate()
                                        {
                                            tab(ui, &t, l, i == 0, CONTROL_H);
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        button(ui, &t, "Product settings", 0.0);
                                        icon_button(ui, &t, ph::STACK, "Layers", false);
                                        icon_button(ui, &t, ph::PLAY, "", true);
                                        rail_button(ui, &t, ph::RULER, true);
                                        glyph_button(ui, &t, ph::ARROW_COUNTER_CLOCKWISE, "Reset");
                                    });
                                    segmented(ui, &t, &["2D", "3D", "Volume"], 0);
                                    let mut on = true;
                                    let mut off = false;
                                    ui.horizontal(|ui| {
                                        check(ui, &t, &mut on, "Smoothing");
                                        check(ui, &t, &mut off, "Range rings");
                                    });
                                    chips(
                                        ui,
                                        &t,
                                        &["Reflectivity", "Velocity", "SRV", "ZDR"],
                                        Some(1),
                                    );
                                    ui.horizontal(|ui| {
                                        badge(ui, &t, "Live", t.live);
                                        badge(ui, &t, "Archive", t.warn);
                                        badge(ui, &t, "Offline", t.danger);
                                    });
                                    let mut v = 0.6;
                                    fader(ui, &t, &mut v, 0.0, 160.0, "Opacity");
                                    let mut pitch = 36.0f32;
                                    prop_slider(
                                        ui,
                                        &t,
                                        "Pitch",
                                        egui::Slider::new(&mut pitch, 0.0..=80.0),
                                    );
                                    let mut sel = 1usize;
                                    prop_row(ui, &t, "Color table", |ui| {
                                        egui::ComboBox::from_id_salt("snap_combo")
                                            .selected_text("Default reflectivity")
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut sel, 1, "Default");
                                            });
                                    });
                                    section_rule(ui, &t, "Core statistics");
                                    kv(ui, &t, "Value", "58.5 dBZ", None);
                                    kv(ui, &t, "Beam height", "1,820 m", None);
                                    kv_text(ui, &t, "Place", "Moore, Oklahoma");
                                    fold_section(
                                        ui,
                                        &t,
                                        "snap",
                                        "Trends",
                                        Some("12"),
                                        true,
                                        |ui| {
                                            let pts: Vec<TrendPoint> = [50.0, 55.0, 52.0, 58.0]
                                                .iter()
                                                .enumerate()
                                                .map(|(i, v)| TrendPoint {
                                                    label: format!("{i}"),
                                                    value: *v,
                                                })
                                                .collect();
                                            trend_chart(
                                                ui,
                                                &t,
                                                "snap_trend",
                                                "Max dBZ",
                                                "dBZ",
                                                &pts,
                                                t.im.plot_lines,
                                            );
                                        },
                                    );
                                    note(
                                        ui,
                                        &t,
                                        "Sparklines for 2 of 5 gauges; one more a minute.",
                                    );
                                    let mut text = String::new();
                                    ui.add(
                                        egui::TextEdit::singleline(&mut text)
                                            .hint_text("HH:MM UTC"),
                                    );
                                    let _ = ui.button("egui button");
                                    let _ = ui.selectable_label(true, "Selected row");
                                });
                            });
                        });
                },
            )
            .unwrap();
        }
    }
}
