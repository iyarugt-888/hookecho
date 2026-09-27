//! The dock's design system ("Analyst Workstation", `docs/WSV3_IMGUI_MODERN_DESIGN_PLAN.md`):
//! one [`Tokens`] value for every colour and size, and small stateless painters that the dock's
//! panels compose. Dock code never sets a colour inline; it asks for a component.
//!
//! Dark only, like the dock before it, but the accent is the theme's (with the user's accent
//! override folded in by `theme::accent`), so the one "your colour" setting still applies.

use egui::{Color32, CornerRadius, FontId, Frame, Margin, Rect, Response, Sense, Stroke, Vec2};

/// App bar height.
pub const APP_BAR_H: f32 = 40.0;
/// Context toolbar height.
pub const TOOLBAR_H: f32 = 36.0;
/// Standard control height (buttons, combos, fields).
pub const CONTROL_H: f32 = 24.0;
/// Tool-rail button edge.
pub const RAIL_BTN: f32 = 36.0;
/// Panel header height.
pub const HEADER_H: f32 = 28.0;

/// Every colour the workstation look uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    pub bg: Color32,
    pub panel: Color32,
    pub panel_hi: Color32,
    pub field: Color32,
    pub field_hi: Color32,
    pub line: Color32,
    pub line_soft: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub live: Color32,
    pub warn: Color32,
    pub danger: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Tokens {
    /// The workstation palette around `accent`.
    pub fn new(accent: Color32) -> Tokens {
        Tokens {
            bg: rgb(0x0B111A),
            panel: rgb(0x0F1722),
            panel_hi: rgb(0x141E2C),
            field: rgb(0x18222F),
            field_hi: rgb(0x1F2B3B),
            line: rgb(0x233147),
            line_soft: rgb(0x1A2536),
            text: rgb(0xD8DFEA),
            text_dim: rgb(0x8A97AB),
            text_faint: rgb(0x5D6A7E),
            accent,
            live: rgb(0x3DD68C),
            warn: rgb(0xF2B84B),
            danger: rgb(0xF25555),
        }
    }

    /// The accent as a translucent wash, for a selected row or an "on" button's fill.
    pub fn accent_soft(&self) -> Color32 {
        let [r, g, b, _] = self.accent.to_array();
        Color32::from_rgba_unmultiplied(r, g, b, 46)
    }
}

/// A docked panel's body.
pub fn panel_frame(t: &Tokens) -> Frame {
    Frame::NONE
        .fill(t.panel)
        .stroke(Stroke::new(1.0, t.line))
        .inner_margin(Margin::same(0))
}

/// A floating card over the map: the panel look, rounded, with a shadow to lift it off the data.
pub fn card_frame(t: &Tokens) -> Frame {
    Frame::NONE
        .fill(t.panel)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(CornerRadius::same(4))
        .shadow(egui::epaint::Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(140),
        })
}

/// Stock egui widgets in the workstation look, for the controls the dock embeds rather than
/// paints (text fields, combo boxes and their menus, sliders, the shared model and layer-option
/// panels).
/// Whether `ui` is inside a [`style_scope`]: shared widgets drawn in both the phone-sized
/// panels and the workstation (the settings switch) take the compact metrics here. Read from
/// the two metrics the scope sets together, which no app theme uses as a pair.
pub fn in_scope(ui: &egui::Ui) -> bool {
    ui.spacing().interact_size.y == CONTROL_H
        && ui
            .style()
            .text_styles
            .get(&egui::TextStyle::Body)
            .is_some_and(|f| f.size == 13.0)
}

pub fn style_scope(ui: &mut egui::Ui, t: &Tokens) {
    let style = ui.style_mut();
    style.override_text_style = None;
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(13.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(12.5));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(11.0));
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(8.0, 3.0);
    style.spacing.interact_size.y = CONTROL_H;
    let v = &mut style.visuals;
    v.override_text_color = Some(t.text);
    v.extreme_bg_color = t.field;
    v.faint_bg_color = t.panel_hi;
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.window_stroke = Stroke::new(1.0, t.line);
    v.selection.bg_fill = t.accent_soft();
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.hyperlink_color = t.accent;
    let r = CornerRadius::same(4);
    for (w, fill, stroke) in [
        (&mut v.widgets.noninteractive, t.panel, t.line_soft),
        (&mut v.widgets.inactive, t.field, t.line),
        (
            &mut v.widgets.hovered,
            t.field_hi,
            t.accent.gamma_multiply(0.6),
        ),
        (
            &mut v.widgets.active,
            t.accent.gamma_multiply(0.85),
            t.accent,
        ),
        (&mut v.widgets.open, t.field_hi, t.accent),
    ] {
        w.corner_radius = r;
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, stroke);
    }
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text_dim);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
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

/// What a tool window's header asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderAction {
    None,
    Close,
    /// Fold a floating window to its title bar, or unfold it.
    Collapse,
    /// Move the window: dock it to a side or float it over the map.
    Place(crate::workspace::Place),
    /// Bring this tab of the header's tab group to the front (see [`HeaderTabs`]).
    Tab(usize),
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

/// A tool window's header: glyph and title, then (right-aligned) a placement menu, a collapse
/// button while floating, and close. `place` is `None` for a window that cannot move;
/// `collapsed` is `Some` only while the window floats. Double-clicking a floating window's
/// header folds it too, as in Dear ImGui.
pub fn window_header(
    ui: &mut egui::Ui,
    t: &Tokens,
    glyph: &str,
    title: &str,
    place: Option<crate::workspace::Place>,
    collapsed: Option<bool>,
) -> HeaderAction {
    use crate::workspace::Place;
    // Taken, not read: the tabs belong to this one header, not to any drawn after it.
    let tabs = ui.ctx().data_mut(|d| {
        let tabs = d.get_temp::<HeaderTabs>(header_tabs_id());
        d.remove::<HeaderTabs>(header_tabs_id());
        tabs
    });
    let (rect, bar) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEADER_H), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, t.panel_hi);
    p.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, t.line),
    );
    let mut action = HeaderAction::None;
    let buttons = 1 + usize::from(collapsed.is_some()) + usize::from(place.is_some());
    if let Some(group) = &tabs {
        let font = FontId::proportional(12.5);
        let full: Vec<f32> = group
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let name = if i == group.front { title } else { tab.title };
                let text = ui
                    .painter()
                    .layout_no_wrap(name.into(), font.clone(), t.text);
                text.size().x + 44.0
            })
            .collect();
        let avail = rect.width() - 8.0 - 24.0 * buttons as f32;
        let widths = tab_widths(&full, group.front, avail);
        let mut x = rect.left();
        for (i, (tab, w)) in group.tabs.iter().zip(&widths).enumerate() {
            let r = Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(*w, rect.height()));
            x += w;
            let front = i == group.front;
            let name = if front { title } else { tab.title };
            let resp = ui.interact(r, ui.id().with(("header_tab", tab.title)), Sense::click());
            let p = ui.painter();
            if front {
                // The front tab is cut from the body's colour, so it reads as the top of the page
                // below it; the accent edge says which one it is without relying on that.
                p.rect_filled(r.with_min_y(r.top() + 1.0), 0.0, t.panel);
                p.rect_filled(
                    Rect::from_min_size(r.min, egui::vec2(r.width(), 2.0)),
                    0.0,
                    t.accent,
                );
            } else if resp.hovered() {
                p.rect_filled(r, 0.0, t.field_hi);
            }
            let ink = if front || resp.hovered() {
                t.text
            } else {
                t.text_dim
            };
            let compact = *w < full[i];
            p.text(
                if compact {
                    r.center()
                } else {
                    r.left_center() + egui::vec2(18.0, 0.0)
                },
                egui::Align2::CENTER_CENTER,
                tab.glyph,
                FontId::proportional(14.0),
                if front { t.accent } else { ink },
            );
            if !compact {
                p.text(
                    r.left_center() + egui::vec2(32.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    name,
                    font.clone(),
                    ink,
                );
            }
            // The front tab's own header already says it (a count in its title); a tab behind it
            // has only this dot to say it wants a look.
            if let (Some(c), false) = (tab.dot, front) {
                let at = if compact {
                    r.center() + egui::vec2(7.0, -6.0)
                } else {
                    r.left_center() + egui::vec2(25.0, -6.0)
                };
                p.circle(at, 3.5, c, Stroke::new(1.5, t.panel_hi));
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
            if resp.clicked() && !front {
                action = HeaderAction::Tab(i);
            }
        }
    } else {
        p.text(
            rect.left_center() + egui::vec2(10.0, 0.0),
            egui::Align2::LEFT_CENTER,
            glyph,
            FontId::proportional(14.0),
            t.text_dim,
        );
        p.text(
            rect.left_center() + egui::vec2(30.0, 0.0),
            egui::Align2::LEFT_CENTER,
            title,
            FontId::proportional(13.0),
            t.text,
        );
    }
    if collapsed.is_some() && bar.double_clicked() {
        action = HeaderAction::Collapse;
    }
    let mut x = rect.right() - 16.0;
    let mut button = |ui: &mut egui::Ui, glyph: &str, hint: &str| -> Response {
        let r = Rect::from_center_size(egui::pos2(x, rect.center().y), egui::vec2(22.0, 22.0));
        x -= 24.0;
        // Keyed on the hint, not the title: a title that carries a count ("Alerts (8)") would
        // otherwise give the same button a new id whenever the count changes.
        let resp = ui.interact(r, ui.id().with(("window_header", hint)), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 4.0, t.field_hi);
        }
        ui.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            FontId::proportional(13.0),
            if resp.hovered() { t.text } else { t.text_dim },
        );
        let enabled = resp.enabled();
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, hint));
        resp.on_hover_text(hint)
    };
    if button(ui, egui_phosphor::regular::X, "Close").clicked() {
        action = HeaderAction::Close;
    }
    if let Some(folded) = collapsed {
        let (glyph, hint) = if folded {
            (egui_phosphor::regular::CARET_DOWN, "Unfold")
        } else {
            (egui_phosphor::regular::CARET_UP, "Fold to the title bar")
        };
        if button(ui, glyph, hint).clicked() {
            action = HeaderAction::Collapse;
        }
    }
    if let Some(now) = place {
        let menu = button(ui, egui_phosphor::regular::DOTS_THREE, "Move this window");
        egui::Popup::menu(&menu).show(|ui| {
            for (p, label) in [
                (Place::Left, "Dock left"),
                (Place::Right, "Dock right"),
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

/// An app-bar tab: plain text, with an accent underline when selected.
pub fn tab(ui: &mut egui::Ui, t: &Tokens, label: &str, selected: bool, height: f32) -> Response {
    let font = FontId::proportional(13.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), t.text);
    let size = egui::vec2(galley.size().x + 24.0, height);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let color = if selected {
        Color32::WHITE
    } else if resp.hovered() {
        t.text
    } else {
        t.text_dim
    };
    if resp.hovered() && !selected {
        ui.painter()
            .rect_filled(rect.shrink2(egui::vec2(2.0, 6.0)), 4.0, t.field);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        color,
    );
    if selected {
        let bar = Rect::from_min_max(
            egui::pos2(rect.left() + 8.0, rect.bottom() - 2.5),
            egui::pos2(rect.right() - 8.0, rect.bottom()),
        );
        ui.painter().rect_filled(bar, 1.0, t.accent);
    }
    resp
}

/// A glyph-and-label button (app bar, panel footers): an "on" one takes the accent.
pub fn icon_button(ui: &mut egui::Ui, t: &Tokens, glyph: &str, label: &str, on: bool) -> Response {
    let font = FontId::proportional(12.5);
    let text = if label.is_empty() {
        glyph.to_string()
    } else {
        format!("{glyph}  {label}")
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text.clone(), font.clone(), t.text);
    let size = egui::vec2(galley.size().x + 18.0, CONTROL_H + 4.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if on {
        t.accent_soft()
    } else if resp.hovered() {
        t.field_hi
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 4.0, fill);
    let color = if on {
        t.accent
    } else if resp.hovered() {
        Color32::WHITE
    } else {
        t.text
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        font,
        color,
    );
    resp
}

/// A plain bordered button in the field style (panel footers, card actions).
pub fn button(ui: &mut egui::Ui, t: &Tokens, label: &str, width: f32) -> Response {
    let font = FontId::proportional(12.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), t.text);
    let size = egui::vec2(width.max(galley.size().x + 20.0), CONTROL_H + 4.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    ui.painter().rect(
        rect,
        4.0,
        if resp.hovered() { t.field_hi } else { t.field },
        Stroke::new(
            1.0,
            if resp.hovered() {
                t.accent.gamma_multiply(0.6)
            } else {
                t.line
            },
        ),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        t.text,
    );
    resp
}

/// A square tool-rail button: the glyph alone, the armed tool filled with the accent.
pub fn rail_button(ui: &mut egui::Ui, t: &Tokens, glyph: &str, on: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(RAIL_BTN, RAIL_BTN), Sense::click());
    let fill = if on {
        t.accent
    } else if resp.hovered() {
        t.field_hi
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 4.0, fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(18.0),
        if on { Color32::WHITE } else { t.text },
    );
    resp
}

/// Joined segment buttons; returns the segment clicked, if any. The selected one is filled with
/// the accent.
pub fn segmented(ui: &mut egui::Ui, t: &Tokens, labels: &[&str], selected: usize) -> Option<usize> {
    let font = FontId::proportional(12.5);
    let widths: Vec<f32> = labels
        .iter()
        .map(|l| {
            ui.painter()
                .layout_no_wrap(l.to_string(), font.clone(), t.text)
                .size()
                .x
                + 22.0
        })
        .collect();
    let total: f32 = widths.iter().sum();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total, CONTROL_H), Sense::hover());
    ui.painter().rect(
        rect,
        4.0,
        t.field,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
    let mut clicked = None;
    let mut x = rect.left();
    for (i, (label, w)) in labels.iter().zip(&widths).enumerate() {
        let r = Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(*w, rect.height()));
        x += w;
        let resp = ui.interact(r, ui.id().with(("seg", i, *label)), Sense::click());
        let on = i == selected;
        if on {
            ui.painter().rect_filled(r.shrink(1.0), 3.0, t.accent);
        } else if resp.hovered() {
            ui.painter().rect_filled(r.shrink(1.0), 3.0, t.field_hi);
        }
        if i > 0 && !on && i != selected + 1 {
            ui.painter().line_segment(
                [
                    r.left_top() + egui::vec2(0.0, 5.0),
                    r.left_bottom() - egui::vec2(0.0, 5.0),
                ],
                Stroke::new(1.0, t.line),
            );
        }
        ui.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            font.clone(),
            if on { Color32::WHITE } else { t.text },
        );
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

/// A compact checkbox: an accent-filled box with a tick when on, then the label.
pub fn check(ui: &mut egui::Ui, t: &Tokens, on: &mut bool, label: &str) -> Response {
    let font = FontId::proportional(12.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), t.text);
    let size = egui::vec2(galley.size().x + 24.0, CONTROL_H);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let bx = Rect::from_center_size(
        egui::pos2(rect.left() + 7.0, rect.center().y),
        egui::vec2(14.0, 14.0),
    );
    if *on {
        ui.painter().rect_filled(bx, 3.0, t.accent);
        ui.painter().text(
            bx.center(),
            egui::Align2::CENTER_CENTER,
            egui_phosphor::regular::CHECK,
            FontId::proportional(11.0),
            Color32::WHITE,
        );
    } else {
        ui.painter().rect(
            bx,
            3.0,
            t.field,
            Stroke::new(1.0, if resp.hovered() { t.accent } else { t.line }),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().text(
        egui::pos2(bx.right() + 7.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        if resp.hovered() {
            Color32::WHITE
        } else {
            t.text
        },
    );
    resp
}

/// A flat fraction slider for a dense row: a 3 px track filled with the accent up to `value`, and
/// a small thumb. Drag or click to set it; with focus, the arrow keys step by 5 %. `value` stays
/// within `min..=1`, and the response is marked changed only when it moved.
pub fn fader(
    ui: &mut egui::Ui,
    t: &Tokens,
    value: &mut f32,
    min: f32,
    width: f32,
    label: &str,
) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(width, 18.0), Sense::click_and_drag());
    let track = rect.shrink2(egui::vec2(5.0, 0.0));
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
    let y = rect.center().y;
    let p = ui.painter();
    let bar =
        |x0: f32, x1: f32| Rect::from_min_max(egui::pos2(x0, y - 1.5), egui::pos2(x1, y + 1.5));
    p.rect_filled(bar(track.left(), track.right()), 1.5, t.field_hi);
    let at = track.left() + track.width() * v;
    p.rect_filled(bar(track.left(), at), 1.5, t.accent);
    let hot = resp.hovered() || resp.dragged() || resp.has_focus();
    p.circle(
        egui::pos2(at, y),
        if hot { 5.5 } else { 4.5 },
        if hot { Color32::WHITE } else { t.text },
        Stroke::new(1.0, t.accent),
    );
    resp
}

/// A small caption before a control ("Site:").
pub fn caption(ui: &mut egui::Ui, t: &Tokens, label: &str) {
    ui.label(text(label, 12.0, t.text_dim));
}

/// An inspector row: the key dim in a fixed column, the value beside it in monospace.
pub fn kv(ui: &mut egui::Ui, t: &Tokens, key: &str, value: &str, color: Option<Color32>) {
    kv_row(ui, t, key, mono(value, 12.0, color.unwrap_or(t.text)));
}

/// An inspector row whose value is words rather than data (a place name), in the text face.
pub fn kv_text(ui: &mut egui::Ui, t: &Tokens, key: &str, value: &str) {
    kv_row(ui, t, key, text(value, 12.5, t.text));
}

fn kv_row(ui: &mut egui::Ui, t: &Tokens, key: &str, value: egui::RichText) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(92.0, 18.0), Sense::hover());
        ui.painter().text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            key,
            FontId::proportional(12.0),
            t.text_dim,
        );
        // A long value (a VCP's full name, a volume file) is cut with an ellipsis and shown whole
        // on hover, rather than widening the card or the docked column it sits in.
        ui.add(egui::Label::new(value).truncate());
    });
}

/// A section's caption inside a card or panel, with a hairline running out to the right edge.
pub fn section_rule(ui: &mut egui::Ui, t: &Tokens, label: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), Sense::hover());
    let galley = ui.painter().layout_no_wrap(
        label.to_uppercase(),
        FontId::proportional(10.5),
        t.text_faint,
    );
    let x = rect.left() + galley.size().x + 8.0;
    ui.painter().galley(
        egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        t.text_faint,
    );
    if x < rect.right() {
        ui.painter().line_segment(
            [
                egui::pos2(x, rect.center().y),
                egui::pos2(rect.right(), rect.center().y),
            ],
            Stroke::new(1.0, t.line_soft),
        );
    }
}

/// A section that folds: the [`section_rule`] look with a caret, open state remembered per `id`.
/// `count` (when given) sits at the right end of the rule, e.g. how many rows are inside.
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
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), Sense::click());
    let open = state.is_open();
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, true, open, label)
    });
    if resp.clicked() {
        state.toggle(ui);
    }
    let p = ui.painter();
    let ink = if resp.hovered() { t.text } else { t.text_dim };
    p.text(
        egui::pos2(rect.left() + 5.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        if open {
            egui_phosphor::regular::CARET_DOWN
        } else {
            egui_phosphor::regular::CARET_RIGHT
        },
        FontId::proportional(10.0),
        ink,
    );
    let galley = p.layout_no_wrap(label.to_uppercase(), FontId::proportional(10.5), ink);
    let x = rect.left() + 16.0;
    let gx = x + galley.size().x + 8.0;
    p.galley(
        egui::pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    let mut right = rect.right();
    if let Some(c) = count {
        let cg = p.layout_no_wrap(c.to_string(), FontId::monospace(10.5), t.text_faint);
        right -= cg.size().x + 6.0;
        p.galley(
            egui::pos2(right + 6.0, rect.center().y - cg.size().y / 2.0),
            cg,
            t.text_faint,
        );
    }
    if gx < right {
        p.line_segment(
            [
                egui::pos2(gx, rect.center().y),
                egui::pos2(right, rect.center().y),
            ],
            Stroke::new(1.0, t.line_soft),
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
    let h = if expanded { 132.0 } else { 44.0 };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() {
        expanded = !expanded;
        ui.ctx().data_mut(|d| d.insert_temp(key, expanded));
    }
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, t.field);
    if resp.hovered() {
        p.rect_stroke(
            rect,
            3.0,
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
        FontId::proportional(10.5),
        t.text_dim,
    );
    p.text(
        rect.right_top() + egui::vec2(-6.0, 3.0),
        egui::Align2::RIGHT_TOP,
        head,
        FontId::monospace(10.5),
        t.text,
    );
    if points.len() < 2 {
        p.text(
            rect.center() + egui::vec2(0.0, 6.0),
            egui::Align2::CENTER_CENTER,
            "needs two volumes",
            FontId::proportional(10.0),
            t.text_faint,
        );
        return resp.named_toggle_info(title, expanded);
    }
    let plot = Rect::from_min_max(
        rect.left_top()
            + egui::vec2(
                if expanded { 34.0 } else { 6.0 },
                if expanded { 26.0 } else { 18.0 },
            ),
        rect.right_bottom() - egui::vec2(6.0, if expanded { 16.0 } else { 5.0 }),
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
                FontId::monospace(9.5),
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
                FontId::monospace(9.5),
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
        p.circle_filled(q, 3.5, Color32::WHITE);
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

trait ToggleInfo {
    fn named_toggle_info(self, name: &str, on: bool) -> Response;
}
impl ToggleInfo for Response {
    fn named_toggle_info(self, name: &str, on: bool) -> Response {
        self.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, name));
        self
    }
}

/// A row of small pill choices that wraps to the width (moment pickers, quick pairs); returns
/// the one clicked. `selected` None highlights none.
pub fn chips(
    ui: &mut egui::Ui,
    t: &Tokens,
    labels: &[&str],
    selected: Option<usize>,
) -> Option<usize> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        let font = FontId::proportional(11.5);
        for (i, label) in labels.iter().enumerate() {
            let galley = ui
                .painter()
                .layout_no_wrap(label.to_string(), font.clone(), t.text);
            let (rect, resp) =
                ui.allocate_exact_size(egui::vec2(galley.size().x + 14.0, 20.0), Sense::click());
            let on = selected == Some(i);
            let fill = if on {
                t.accent_soft()
            } else if resp.hovered() {
                t.field_hi
            } else {
                t.field
            };
            ui.painter().rect(
                rect,
                10.0,
                fill,
                Stroke::new(1.0, if on { t.accent } else { t.line_soft }),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                *label,
                font.clone(),
                if on {
                    t.accent
                } else if resp.hovered() {
                    t.text
                } else {
                    t.text_dim
                },
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

/// A small pill: a dot and a word ("● Live").
pub fn badge(ui: &mut egui::Ui, t: &Tokens, label: &str, color: Color32) -> Response {
    let font = FontId::proportional(11.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), t.text);
    let size = egui::vec2(galley.size().x + 22.0, 18.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let [r, g, b, _] = color.to_array();
    ui.painter()
        .rect_filled(rect, 9.0, Color32::from_rgba_unmultiplied(r, g, b, 30));
    ui.painter()
        .circle_filled(egui::pos2(rect.left() + 9.0, rect.center().y), 3.5, color);
    ui.painter().text(
        egui::pos2(rect.left() + 16.0, rect.center().y),
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

/// A vertical hairline between control groups in a horizontal row.
pub fn divider(ui: &mut egui::Ui, t: &Tokens, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, height), Sense::hover());
    ui.painter().line_segment(
        [
            egui::pos2(rect.center().x, rect.top() + 4.0),
            egui::pos2(rect.center().x, rect.bottom() - 4.0),
        ],
        Stroke::new(1.0, t.line),
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
        Tokens::new(rgb(0x2F81F7))
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
        assert!(got.iter().any(|s| s == "CORE") && got.iter().any(|s| s == "inside"));
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

    #[test]
    fn the_accent_wash_is_the_accent_translucent() {
        let t = t();
        let [r, g, b, a] = t.accent_soft().to_array();
        assert!(a > 0 && a < 255, "{a}");
        // Premultiplied, so each channel is at most the accent's own.
        let [ar, ag, ab, _] = t.accent.to_array();
        assert!(r <= ar && g <= ag && b <= ab);
        assert_ne!(
            t.panel, t.panel_hi,
            "a header must stand apart from its body"
        );
    }
}
