//! The one place the floating chrome gets its look and its position from.
//!
//! Desktop and mobile share the same card language, so the frame builders live here rather than
//! in `app::mobile` (where desktop used to import them from). Since the Dear ImGui restyle a
//! "glass" card is an ImGui window floated over the map: the style's `WindowBg`, a 1px border,
//! square corners, no blur or shadow. The `LANE_*` constants are the
//! other half: every floating surface anchors to a named lane instead of a hand-picked offset, so
//! two panels can't quietly land on top of each other.
//!
//! Two window idioms exist and both are fine:
//! - **Floating chrome** — `egui::Area` + [`glass`], anchored to a `LANE_*` offset. Map-first, no
//!   title bar, no drag. Everything in the always-on-screen layer uses this.
//! - **Tool window** — `crate::ui::phone_surface(egui::Window::new(..))`, titled, draggable, closable.
//!   Everything summoned on demand (soundings, settings, palette editor) uses this.

use egui::{vec2, Color32, Frame, Margin, RichText, Stroke};

/// RadarOmega's signature accents: orange for the active/live radar chrome, blue for actions.
pub const OMEGA_ORANGE: Color32 = Color32::from_rgb(0xF2, 0xA0, 0x33);
pub const OMEGA_BLUE: Color32 = Color32::from_rgb(0x2D, 0x9C, 0xDB);
/// Reserved for live/recording semantics only — the theme accent is the accent.
pub const OMEGA_GREEN: Color32 = Color32::from_rgb(0x3D, 0xD5, 0x6B);

/// Type scale for chrome text: ImGui has one font size, so the three text steps are the same;
/// `FONT_TITLE` is the glyph size on a map button.
pub const FONT_SM: f32 = crate::theme::FONT;
pub const FONT_BASE: f32 = crate::theme::FONT;
pub const FONT_LG: f32 = crate::theme::FONT;
pub const FONT_TITLE: f32 = 20.0;

/// Corner radii for chips and buttons (`SM`) and cards and panels (`LG`): ImGui rounds neither.
pub const RADIUS_SM: f32 = 0.0;
pub const RADIUS_LG: f32 = 0.0;

/// A dark card's fill before the per-call alpha: Dear ImGui Dark's `WindowBg` (0.06 at 0.94)
/// over the map's ground.
pub const CARD_FILL: (u8, u8, u8) = (15, 15, 16);

// ---------- Anchor lanes ----------
// Top edge, CENTER_TOP: banners sit above the search pill.
pub const LANE_TOP_BANNER: f32 = 8.0;
// Right edge, RIGHT_TOP: the control column is outermost, panels open inboard of it, and
// status badges stack below both.
pub const LANE_RIGHT_CONTROLS: egui::Vec2 = vec2(-14.0, 44.0);
pub const LANE_RIGHT_PANEL: egui::Vec2 = vec2(-74.0, 44.0);
pub const LANE_RIGHT_BADGE_X: f32 = -14.0;
/// First free y below a control column of `buttons` 44 px buttons.
pub fn lane_right_badge_y(buttons: usize) -> f32 {
    LANE_RIGHT_CONTROLS.y + buttons as f32 * 50.0 + 8.0
}
// Bottom edge.
pub const LANE_BOTTOM_CHIP: f32 = -8.0;
/// The chase HUD sits above the bottom edge.
pub const LANE_BOTTOM_CHASE: f32 = -92.0;

/// A card floated over the map: an ImGui window in the current style (`WindowBg` at `alpha`, its
/// 1px `Border`, square, `WindowPadding` across and `FramePadding` down).
pub fn glass(_ui: &egui::Ui, alpha: u8) -> Frame {
    let p = crate::theme::current();
    let [r, g, b, _] = p.window_bg.to_array();
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(r, g, b, alpha))
        .corner_radius(0)
        .inner_margin(Margin::symmetric(8, 3))
        .stroke(Stroke::new(1.0, p.border))
}

/// A ~44px square chrome button holding one Phosphor glyph: ImGui's `Button` colours on a
/// `WindowBg` square (so it reads over any basemap), `ButtonActive` while on.
pub fn square_btn(
    ui: &mut egui::Ui,
    glyph: &str,
    active: bool,
    _accent: Color32,
) -> egui::Response {
    let p = crate::theme::current();
    // The resting fill is the window's, not the translucent `Button`: these float over the map,
    // and a pale glyph on a see-through fill disappears over the light and satellite basemaps.
    let bg = if active { p.button_active } else { p.window_bg };
    ui.add(
        egui::Button::new(RichText::new(glyph).size(FONT_TITLE).color(p.text))
            .min_size(vec2(44.0, 44.0))
            .fill(bg)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(0.0),
    )
}

/// Full-width, keyboard-accessible settings switch.
pub(crate) fn toggle(ui: &mut egui::Ui, value: &mut bool, label: &str) -> egui::Response {
    use crate::ui::a11y::Named;
    // Touch-sized in the panels; the workstation's denser rows inside its style scope.
    let (h, text, glyph) = if crate::ui::workstation::in_scope(ui) {
        (26.0, 12.5, 20.0)
    } else {
        (38.0, 14.0, 28.0)
    };
    let mut response = ui
        .add_sized(
            egui::vec2(ui.available_width(), h),
            egui::Button::new("").frame(false),
        )
        .named_toggle(label, *value);
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    let rect = response.rect;
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(text),
        ui.visuals().text_color(),
    );
    ui.painter().text(
        rect.right_center(),
        egui::Align2::RIGHT_CENTER,
        if *value {
            egui_phosphor::regular::TOGGLE_RIGHT
        } else {
            egui_phosphor::regular::TOGGLE_LEFT
        },
        egui::FontId::proportional(glyph),
        if *value {
            ui.visuals().selection.stroke.color
        } else {
            ui.visuals().weak_text_color()
        },
    );
    response
}

#[cfg(test)]
mod switch_tests {
    #[test]
    fn settings_switch_toggles_and_reports_changes() {
        let ctx = egui::Context::default();
        let mut value = false;
        let mut pos = egui::Pos2::ZERO;
        let mut frame = |events| {
            let mut changed = false;
            let _ = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response = super::toggle(ui, &mut value, "Radar");
                    pos = response.rect.center();
                    changed = response.changed();
                },
            );
            (value, changed, pos)
        };
        let (_, changed, pos) = frame(vec![]);
        assert!(!changed);
        for expected in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ]);
            let (value, changed, _) = frame(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }]);
            assert_eq!(value, expected);
            assert!(changed);
        }
    }
}
