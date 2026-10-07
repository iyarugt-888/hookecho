//! Dear ImGui's look for the whole app: its three colour styles and `ImGuiStyle`'s default
//! geometry, installed into egui's `Style`.
//!
//! The colours are `ImGui::StyleColorsDark()`, `StyleColorsLight()` and `StyleColorsClassic()`
//! from Dear ImGui v1.91.5 (`imgui_draw.cpp`), kept here as ImGui writes them: straight RGBA
//! floats per `ImGuiCol_*` role, the `Tab*` roles already evaluated through the same `ImLerp`.
//! [`Palette`] composites the translucent ones the way ImGui draws them: windows, title bars and
//! popups over the map's ground ([`VIEWPORT_BG`]), everything inside a window over the window. So
//! every colour a painter gets is opaque, and Classic's 85%-black windows still let the map show
//! through. The only colours that are HookEcho's rather than ImGui's are the ground and the three
//! status colours (`live`, `warn`, `danger`), which ImGui has no role for.
//!
//! Geometry is `ImGuiStyle`'s defaults: a 13px font everywhere, `FramePadding` 4 x 3 (so every
//! frame is 19px), `ItemSpacing` 8 x 4, `WindowPadding` 8, `IndentSpacing` 21, no rounding, no
//! frame borders, no shadows. Touch scales it up for a finger.
//!
//! Applied every frame from [`crate::app`] (a cheap style clone) so it survives runtime theme
//! switches. The workstation's painters (`ui::workstation::Tokens`) read the same palette.

use crate::settings::Theme;
use egui::{vec2, Color32, CornerRadius, Margin, Shadow, Stroke, Style, Visuals};
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};

/// What every window sits on: the radar map's dark ground and the app background behind the
/// docks. The same in every theme, because the map stays dark.
pub const VIEWPORT_BG: Color32 = Color32::from_rgb(0x0b, 0x11, 0x1a);

/// ImGui Dark's `CheckMark` blue: the accent where no theme is in scope. Live UI reads
/// [`accent`] or the palette.
pub const ACCENT: Color32 = Color32::from_rgb(0x42, 0x96, 0xfa);

/// The frame height every desktop control shares: the 13px font plus `FramePadding.y` twice.
pub const FRAME_H: f32 = 19.0;
/// The one desktop font size.
pub const FONT: f32 = 13.0;
/// `TabRounding`: tabs are the one thing ImGui rounds (top corners only).
pub const TAB_ROUNDING: u8 = 4;

/// One Dear ImGui colour style: `ImGuiCol_*` as straight (unpremultiplied) RGBA floats.
#[derive(Clone, Copy)]
struct ImStyle {
    text: [f32; 4],
    text_disabled: [f32; 4],
    window_bg: [f32; 4],
    popup_bg: [f32; 4],
    border: [f32; 4],
    frame_bg: [f32; 4],
    frame_bg_hovered: [f32; 4],
    frame_bg_active: [f32; 4],
    title_bg: [f32; 4],
    title_bg_active: [f32; 4],
    title_bg_collapsed: [f32; 4],
    menu_bar_bg: [f32; 4],
    scrollbar_bg: [f32; 4],
    scrollbar_grab: [f32; 4],
    scrollbar_grab_hovered: [f32; 4],
    scrollbar_grab_active: [f32; 4],
    check_mark: [f32; 4],
    slider_grab: [f32; 4],
    slider_grab_active: [f32; 4],
    button: [f32; 4],
    button_hovered: [f32; 4],
    button_active: [f32; 4],
    header: [f32; 4],
    header_hovered: [f32; 4],
    header_active: [f32; 4],
    separator: [f32; 4],
    separator_hovered: [f32; 4],
    separator_active: [f32; 4],
    tab_hovered: [f32; 4],
    tab: [f32; 4],
    tab_selected: [f32; 4],
    tab_selected_overline: [f32; 4],
    tab_dimmed: [f32; 4],
    tab_dimmed_selected: [f32; 4],
    plot_lines: [f32; 4],
    plot_histogram: [f32; 4],
    table_header_bg: [f32; 4],
    table_border_strong: [f32; 4],
    table_border_light: [f32; 4],
    table_row_bg_alt: [f32; 4],
    text_link: [f32; 4],
    text_selected_bg: [f32; 4],
    nav_cursor: [f32; 4],
    modal_window_dim_bg: [f32; 4],
}

/// `ImGui::StyleColorsDark()`.
const DARK: ImStyle = ImStyle {
    text: [1.0, 1.0, 1.0, 1.0],
    text_disabled: [0.5, 0.5, 0.5, 1.0],
    window_bg: [0.06, 0.06, 0.06, 0.94],
    popup_bg: [0.08, 0.08, 0.08, 0.94],
    border: [0.43, 0.43, 0.5, 0.5],
    frame_bg: [0.16, 0.29, 0.48, 0.54],
    frame_bg_hovered: [0.26, 0.59, 0.98, 0.4],
    frame_bg_active: [0.26, 0.59, 0.98, 0.67],
    title_bg: [0.04, 0.04, 0.04, 1.0],
    title_bg_active: [0.16, 0.29, 0.48, 1.0],
    title_bg_collapsed: [0.0, 0.0, 0.0, 0.51],
    menu_bar_bg: [0.14, 0.14, 0.14, 1.0],
    scrollbar_bg: [0.02, 0.02, 0.02, 0.53],
    scrollbar_grab: [0.31, 0.31, 0.31, 1.0],
    scrollbar_grab_hovered: [0.41, 0.41, 0.41, 1.0],
    scrollbar_grab_active: [0.51, 0.51, 0.51, 1.0],
    check_mark: [0.26, 0.59, 0.98, 1.0],
    slider_grab: [0.24, 0.52, 0.88, 1.0],
    slider_grab_active: [0.26, 0.59, 0.98, 1.0],
    button: [0.26, 0.59, 0.98, 0.4],
    button_hovered: [0.26, 0.59, 0.98, 1.0],
    button_active: [0.06, 0.53, 0.98, 1.0],
    header: [0.26, 0.59, 0.98, 0.31],
    header_hovered: [0.26, 0.59, 0.98, 0.8],
    header_active: [0.26, 0.59, 0.98, 1.0],
    separator: [0.43, 0.43, 0.5, 0.5], // = Border
    separator_hovered: [0.1, 0.4, 0.75, 0.78],
    separator_active: [0.1, 0.4, 0.75, 1.0],
    tab_hovered: [0.26, 0.59, 0.98, 0.8], // = HeaderHovered
    tab: [0.18, 0.35, 0.58, 0.862],       // ImLerp(Header, TitleBgActive, 0.80)
    tab_selected: [0.2, 0.41, 0.68, 1.0], // ImLerp(HeaderActive, TitleBgActive, 0.60)
    tab_selected_overline: [0.26, 0.59, 0.98, 1.0], // = HeaderActive
    tab_dimmed: [0.068, 0.102, 0.148, 0.9724], // ImLerp(Tab, TitleBg, 0.80)
    tab_dimmed_selected: [0.136, 0.262, 0.424, 1.0], // ImLerp(TabSelected, TitleBg, 0.40)
    plot_lines: [0.61, 0.61, 0.61, 1.0],
    plot_histogram: [0.9, 0.7, 0.0, 1.0],
    table_header_bg: [0.19, 0.19, 0.2, 1.0],
    table_border_strong: [0.31, 0.31, 0.35, 1.0],
    table_border_light: [0.23, 0.23, 0.25, 1.0],
    table_row_bg_alt: [1.0, 1.0, 1.0, 0.06],
    text_link: [0.26, 0.59, 0.98, 1.0], // = HeaderActive
    text_selected_bg: [0.26, 0.59, 0.98, 0.35],
    nav_cursor: [0.26, 0.59, 0.98, 1.0],
    modal_window_dim_bg: [0.8, 0.8, 0.8, 0.35],
};

/// `ImGui::StyleColorsLight()`.
const LIGHT: ImStyle = ImStyle {
    text: [0.0, 0.0, 0.0, 1.0],
    text_disabled: [0.6, 0.6, 0.6, 1.0],
    window_bg: [0.94, 0.94, 0.94, 1.0],
    popup_bg: [1.0, 1.0, 1.0, 0.98],
    border: [0.0, 0.0, 0.0, 0.3],
    frame_bg: [1.0, 1.0, 1.0, 1.0],
    frame_bg_hovered: [0.26, 0.59, 0.98, 0.4],
    frame_bg_active: [0.26, 0.59, 0.98, 0.67],
    title_bg: [0.96, 0.96, 0.96, 1.0],
    title_bg_active: [0.82, 0.82, 0.82, 1.0],
    title_bg_collapsed: [1.0, 1.0, 1.0, 0.51],
    menu_bar_bg: [0.86, 0.86, 0.86, 1.0],
    scrollbar_bg: [0.98, 0.98, 0.98, 0.53],
    scrollbar_grab: [0.69, 0.69, 0.69, 0.8],
    scrollbar_grab_hovered: [0.49, 0.49, 0.49, 0.8],
    scrollbar_grab_active: [0.49, 0.49, 0.49, 1.0],
    check_mark: [0.26, 0.59, 0.98, 1.0],
    slider_grab: [0.26, 0.59, 0.98, 0.78],
    slider_grab_active: [0.46, 0.54, 0.8, 0.6],
    button: [0.26, 0.59, 0.98, 0.4],
    button_hovered: [0.26, 0.59, 0.98, 1.0],
    button_active: [0.06, 0.53, 0.98, 1.0],
    header: [0.26, 0.59, 0.98, 0.31],
    header_hovered: [0.26, 0.59, 0.98, 0.8],
    header_active: [0.26, 0.59, 0.98, 1.0],
    separator: [0.39, 0.39, 0.39, 0.62],
    separator_hovered: [0.14, 0.44, 0.8, 0.78],
    separator_active: [0.14, 0.44, 0.8, 1.0],
    tab_hovered: [0.26, 0.59, 0.98, 0.8],     // = HeaderHovered
    tab: [0.764, 0.797, 0.836, 0.931],        // ImLerp(Header, TitleBgActive, 0.90)
    tab_selected: [0.596, 0.728, 0.884, 1.0], // ImLerp(HeaderActive, TitleBgActive, 0.60)
    tab_selected_overline: [0.26, 0.59, 0.98, 1.0], // = HeaderActive
    tab_dimmed: [0.9208, 0.9274, 0.9352, 0.9862], // ImLerp(Tab, TitleBg, 0.80)
    tab_dimmed_selected: [0.7416, 0.8208, 0.9144, 1.0], // ImLerp(TabSelected, TitleBg, 0.40)
    plot_lines: [0.39, 0.39, 0.39, 1.0],
    plot_histogram: [0.9, 0.7, 0.0, 1.0],
    table_header_bg: [0.78, 0.87, 0.98, 1.0],
    table_border_strong: [0.57, 0.57, 0.64, 1.0],
    table_border_light: [0.68, 0.68, 0.74, 1.0],
    table_row_bg_alt: [0.3, 0.3, 0.3, 0.09],
    text_link: [0.26, 0.59, 0.98, 1.0], // = HeaderActive
    text_selected_bg: [0.26, 0.59, 0.98, 0.35],
    nav_cursor: [0.26, 0.59, 0.98, 0.8], // = HeaderHovered
    modal_window_dim_bg: [0.2, 0.2, 0.2, 0.35],
};

/// `ImGui::StyleColorsClassic()`.
const CLASSIC: ImStyle = ImStyle {
    text: [0.9, 0.9, 0.9, 1.0],
    text_disabled: [0.6, 0.6, 0.6, 1.0],
    window_bg: [0.0, 0.0, 0.0, 0.85],
    popup_bg: [0.11, 0.11, 0.14, 0.92],
    border: [0.5, 0.5, 0.5, 0.5],
    frame_bg: [0.43, 0.43, 0.43, 0.39],
    frame_bg_hovered: [0.47, 0.47, 0.69, 0.4],
    frame_bg_active: [0.42, 0.41, 0.64, 0.69],
    title_bg: [0.27, 0.27, 0.54, 0.83],
    title_bg_active: [0.32, 0.32, 0.63, 0.87],
    title_bg_collapsed: [0.4, 0.4, 0.8, 0.2],
    menu_bar_bg: [0.4, 0.4, 0.55, 0.8],
    scrollbar_bg: [0.2, 0.25, 0.3, 0.6],
    scrollbar_grab: [0.4, 0.4, 0.8, 0.3],
    scrollbar_grab_hovered: [0.4, 0.4, 0.8, 0.4],
    scrollbar_grab_active: [0.41, 0.39, 0.8, 0.6],
    check_mark: [0.9, 0.9, 0.9, 0.5],
    slider_grab: [1.0, 1.0, 1.0, 0.3],
    slider_grab_active: [0.41, 0.39, 0.8, 0.6],
    button: [0.35, 0.4, 0.61, 0.62],
    button_hovered: [0.4, 0.48, 0.71, 0.79],
    button_active: [0.46, 0.54, 0.8, 1.0],
    header: [0.4, 0.4, 0.9, 0.45],
    header_hovered: [0.45, 0.45, 0.9, 0.8],
    header_active: [0.53, 0.53, 0.87, 0.8],
    separator: [0.5, 0.5, 0.5, 0.6],
    separator_hovered: [0.6, 0.6, 0.7, 1.0],
    separator_active: [0.7, 0.7, 0.9, 1.0],
    tab_hovered: [0.45, 0.45, 0.9, 0.8],        // = HeaderHovered
    tab: [0.336, 0.336, 0.684, 0.786],          // ImLerp(Header, TitleBgActive, 0.80)
    tab_selected: [0.404, 0.404, 0.726, 0.842], // ImLerp(HeaderActive, TitleBgActive, 0.60)
    tab_selected_overline: [0.53, 0.53, 0.87, 0.8], // = HeaderActive
    tab_dimmed: [0.2832, 0.2832, 0.5688, 0.8212], // ImLerp(Tab, TitleBg, 0.80)
    tab_dimmed_selected: [0.3504, 0.3504, 0.6516, 0.8372], // ImLerp(TabSelected, TitleBg, 0.40)
    plot_lines: [1.0, 1.0, 1.0, 1.0],
    plot_histogram: [0.9, 0.7, 0.0, 1.0],
    table_header_bg: [0.27, 0.27, 0.38, 1.0],
    table_border_strong: [0.31, 0.31, 0.45, 1.0],
    table_border_light: [0.26, 0.26, 0.28, 1.0],
    table_row_bg_alt: [1.0, 1.0, 1.0, 0.07],
    text_link: [0.53, 0.53, 0.87, 0.8], // = HeaderActive
    text_selected_bg: [0.0, 0.0, 1.0, 0.35],
    nav_cursor: [0.45, 0.45, 0.9, 0.8], // = HeaderHovered
    modal_window_dim_bg: [0.2, 0.2, 0.2, 0.35],
};
/// One theme's colours, ready to paint: every `ImGuiCol_*` role this app uses, composited to an
/// opaque colour (see the module doc), plus HookEcho's status colours.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub is_dark: bool,
    pub text: Color32,
    pub text_disabled: Color32,
    pub window_bg: Color32,
    pub popup_bg: Color32,
    pub border: Color32,
    pub frame_bg: Color32,
    pub frame_bg_hovered: Color32,
    pub frame_bg_active: Color32,
    pub title_bg: Color32,
    pub title_bg_active: Color32,
    pub title_bg_collapsed: Color32,
    pub menu_bar_bg: Color32,
    pub scrollbar_bg: Color32,
    pub scrollbar_grab: Color32,
    pub scrollbar_grab_hovered: Color32,
    pub scrollbar_grab_active: Color32,
    pub check_mark: Color32,
    pub slider_grab: Color32,
    pub slider_grab_active: Color32,
    pub button: Color32,
    pub button_hovered: Color32,
    pub button_active: Color32,
    pub header: Color32,
    pub header_hovered: Color32,
    pub header_active: Color32,
    pub separator: Color32,
    pub separator_hovered: Color32,
    pub separator_active: Color32,
    pub tab_hovered: Color32,
    pub tab: Color32,
    pub tab_selected: Color32,
    pub tab_selected_overline: Color32,
    pub tab_dimmed: Color32,
    pub tab_dimmed_selected: Color32,
    pub plot_lines: Color32,
    pub plot_histogram: Color32,
    pub table_header_bg: Color32,
    pub table_border_strong: Color32,
    pub table_border_light: Color32,
    pub table_row_bg_alt: Color32,
    pub text_link: Color32,
    pub text_selected_bg: Color32,
    pub nav_cursor: Color32,
    pub modal_window_dim_bg: Color32,
    /// Live, fresh, sweeping now.
    pub live: Color32,
    /// Archive, aging, stale, needs a look.
    pub warn: Color32,
    /// Errors, offline feeds, alert counts.
    pub danger: Color32,
}

impl Palette {
    fn build(s: &ImStyle, status: [Color32; 3], is_dark: bool) -> Palette {
        let window = over(s.window_bg, VIEWPORT_BG);
        Palette {
            is_dark,
            text: over(s.text, window),
            text_disabled: over(s.text_disabled, window),
            window_bg: window,
            popup_bg: over(s.popup_bg, VIEWPORT_BG),
            border: over(s.border, window),
            frame_bg: over(s.frame_bg, window),
            frame_bg_hovered: over(s.frame_bg_hovered, window),
            frame_bg_active: over(s.frame_bg_active, window),
            title_bg: over(s.title_bg, VIEWPORT_BG),
            title_bg_active: over(s.title_bg_active, VIEWPORT_BG),
            title_bg_collapsed: over(s.title_bg_collapsed, VIEWPORT_BG),
            menu_bar_bg: over(s.menu_bar_bg, window),
            scrollbar_bg: over(s.scrollbar_bg, window),
            scrollbar_grab: over(s.scrollbar_grab, window),
            scrollbar_grab_hovered: over(s.scrollbar_grab_hovered, window),
            scrollbar_grab_active: over(s.scrollbar_grab_active, window),
            check_mark: over(s.check_mark, window),
            slider_grab: over(s.slider_grab, window),
            slider_grab_active: over(s.slider_grab_active, window),
            button: over(s.button, window),
            button_hovered: over(s.button_hovered, window),
            button_active: over(s.button_active, window),
            header: over(s.header, window),
            header_hovered: over(s.header_hovered, window),
            header_active: over(s.header_active, window),
            separator: over(s.separator, window),
            separator_hovered: over(s.separator_hovered, window),
            separator_active: over(s.separator_active, window),
            tab_hovered: over(s.tab_hovered, window),
            tab: over(s.tab, window),
            tab_selected: over(s.tab_selected, window),
            tab_selected_overline: over(s.tab_selected_overline, window),
            tab_dimmed: over(s.tab_dimmed, window),
            tab_dimmed_selected: over(s.tab_dimmed_selected, window),
            plot_lines: over(s.plot_lines, window),
            plot_histogram: over(s.plot_histogram, window),
            table_header_bg: over(s.table_header_bg, window),
            table_border_strong: over(s.table_border_strong, window),
            table_border_light: over(s.table_border_light, window),
            table_row_bg_alt: over(s.table_row_bg_alt, window),
            text_link: over(s.text_link, window),
            text_selected_bg: over(s.text_selected_bg, window),
            nav_cursor: over(s.nav_cursor, window),
            modal_window_dim_bg: straight(s.modal_window_dim_bg),
            live: status[0],
            warn: status[1],
            danger: status[2],
        }
    }
}

/// `over` (straight RGBA) composited onto opaque `under`, as ImGui's renderer blends it.
fn over(c: [f32; 4], under: Color32) -> Color32 {
    let a = c[3];
    let mix = |v: f32, u: u8| (v * 255.0 * a + f32::from(u) * (1.0 - a)).round() as u8;
    Color32::from_rgb(
        mix(c[0], under.r()),
        mix(c[1], under.g()),
        mix(c[2], under.b()),
    )
}

/// A straight RGBA role kept translucent.
fn straight(c: [f32; 4]) -> Color32 {
    let b = |v: f32| (v * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(b(c[0]), b(c[1]), b(c[2]), b(c[3]))
}

/// `live`, `warn`, `danger` on ImGui's dark windows (Dark and Classic).
const STATUS_DARK: [Color32; 3] = [
    Color32::from_rgb(0x3d, 0xd6, 0x8c),
    Color32::from_rgb(0xf2, 0xb8, 0x4b),
    Color32::from_rgb(0xf2, 0x55, 0x55),
];
/// The same three darkened to read on Light's grey windows (at least 4.5:1 on `WindowBg`).
const STATUS_LIGHT: [Color32; 3] = [
    Color32::from_rgb(0x13, 0x70, 0x3f),
    Color32::from_rgb(0x8a, 0x53, 0x00),
    Color32::from_rgb(0xb4, 0x23, 0x18),
];

/// The three styles a [`Theme`] resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    Dark,
    Light,
    Classic,
}

fn look(theme: Theme, system_dark: bool) -> Look {
    match theme {
        Theme::Dark => Look::Dark,
        Theme::Light => Look::Light,
        Theme::Classic => Look::Classic,
        Theme::System if system_dark => Look::Dark,
        Theme::System => Look::Light,
    }
}

fn palette_of(look: Look) -> Palette {
    match look {
        Look::Dark => Palette::build(&DARK, STATUS_DARK, true),
        Look::Light => Palette::build(&LIGHT, STATUS_LIGHT, false),
        Look::Classic => Palette::build(&CLASSIC, STATUS_DARK, true),
    }
}

/// A theme's palette, with the user's accent override (if any) folded in.
pub fn palette(theme: Theme, system_dark: bool) -> Palette {
    with_accent(palette_of(look(theme, system_dark)))
}

/// The look `apply` last installed, as a [`Look`] discriminant: the workstation's tokens are built
/// in dozens of places with no `&Settings` in scope, and `apply` is the only writer.
static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The palette of the theme on screen now (Dark until `apply` has run, as in tests).
pub fn current() -> Palette {
    let look = match CURRENT.load(Ordering::Relaxed) {
        1 => Look::Light,
        2 => Look::Classic,
        _ => Look::Dark,
    };
    with_accent(palette_of(look))
}

/// The accent a theme marks things with (map markers, the active pane's outline, coloured
/// headings): ImGui's `TextLink`, or the user's own colour. `TextLink` rather than `CheckMark`
/// because Classic's check mark is a half-transparent grey that goes dull as a colour of its own;
/// in Dark and Light the two are the same blue.
pub fn accent(theme: Theme) -> Color32 {
    palette(theme, true).text_link
}

/// User accent override, packed as `0xFF_RR_GG_BB` (0 = none).
///
/// ponytail: a process-global instead of threading `&Settings` through every `accent()` call site —
/// the accent is one app-wide value and `apply()` is the only writer. If accent ever becomes
/// per-window, pass it explicitly instead.
static ACCENT_OVERRIDE: AtomicU32 = AtomicU32::new(0);

pub fn set_accent_override(rgb: Option<[u8; 3]>) {
    let packed = rgb.map_or(0, |[r, g, b]| {
        0xFF00_0000 | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
    });
    ACCENT_OVERRIDE.store(packed, Ordering::Relaxed);
}

fn accent_override() -> Option<Color32> {
    let p = ACCENT_OVERRIDE.load(Ordering::Relaxed);
    (p != 0).then(|| Color32::from_rgb((p >> 16) as u8, (p >> 8) as u8, p as u8))
}

/// A custom accent takes over the roles that mark a thing as chosen or grabbed: the check mark,
/// the slider grab, links and the keyboard focus. Buttons and headers keep the style's own.
fn with_accent(mut p: Palette) -> Palette {
    if let Some(a) = accent_override() {
        p.check_mark = a;
        p.slider_grab = a;
        p.slider_grab_active = a;
        p.text_link = a;
        p.nav_cursor = a;
    }
    p
}

/// The window fill for a theme, for the settings swatch preview.
pub fn preview_bg(theme: Theme) -> Color32 {
    palette(theme, true).window_bg
}

pub fn apply(ctx: &egui::Context, theme: Theme, system_dark: bool, accent_rgb: Option<[u8; 3]>) {
    set_accent_override(accent_rgb);
    let look = look(theme, system_dark);
    CURRENT.store(look as u8, Ordering::Relaxed);
    let p = palette(theme, system_dark);
    let mut style = Style {
        visuals: visuals(&p),
        ..Default::default()
    };
    geometry(&mut style, cfg!(target_os = "android"));
    let egui_theme = if p.is_dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_style_of(egui_theme, style);
    ctx.options_mut(|o| {
        o.theme_preference = if p.is_dark {
            egui::ThemePreference::Dark
        } else {
            egui::ThemePreference::Light
        }
    });
}

/// `ImGuiStyle`'s geometry, or its touch-scaled version on a phone or tablet: spacing, frame
/// height, the type scale (one size, as ImGui has one font), and no rounding anywhere.
pub fn geometry(style: &mut Style, touch: bool) {
    use egui::{FontFamily, FontId, TextStyle};
    let s = &mut style.spacing;
    let (text, mono) = if touch {
        s.item_spacing = vec2(8.0, 8.0);
        s.button_padding = vec2(12.0, 8.0);
        // 44, not 48: `interact_size` is the *minimum* egui pads every widget to, and 48 there
        // bloats inline rows. Real tap targets get `m3::MIN_TARGET` explicitly.
        s.interact_size.y = 44.0;
        s.window_margin = Margin::same(10);
        (15.0, 14.0)
    } else {
        s.item_spacing = vec2(8.0, 4.0); // ImGuiStyle::ItemSpacing
        s.button_padding = vec2(4.0, 3.0); // ImGuiStyle::FramePadding
        s.interact_size.y = FRAME_H;
        s.window_margin = Margin::same(8); // ImGuiStyle::WindowPadding
        (FONT, FONT)
    };
    s.menu_margin = Margin::same(8);
    s.indent = 21.0; // ImGuiStyle::IndentSpacing
                     // A checkbox, radio or combo arrow is a frame-height square, as ImGui's are.
    s.icon_width = if touch { 24.0 } else { FRAME_H };
    s.icon_width_inner = s.icon_width * 0.68;
    s.icon_spacing = 4.0; // ImGuiStyle::ItemInnerSpacing
                          // A slider is a whole frame with a grab in it, not a thin rail with a knob.
    s.slider_rail_height = s.interact_size.y;
    s.scroll = egui::style::ScrollStyle::solid();
    s.scroll.bar_width = 14.0; // ImGuiStyle::ScrollbarSize
    s.scroll.handle_min_length = 12.0; // ImGuiStyle::GrabMinSize
    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(text, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(text, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(text, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(text, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(mono, FontFamily::Monospace),
        ),
    ]
    .into();
}

/// egui's visuals filled from ImGui's roles. Buttons take `Button*` (egui's `weak_bg_fill`),
/// framed widgets `FrameBg*` (`bg_fill`); nothing has a border, a rounded corner, a shadow or a
/// hover expansion.
pub fn visuals(p: &Palette) -> Visuals {
    let mut v = if p.is_dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    fill_widgets(&mut v, p);
    v.panel_fill = p.window_bg;
    v.window_fill = p.window_bg;
    v.extreme_bg_color = p.frame_bg;
    v.text_edit_bg_color = Some(p.frame_bg);
    v.code_bg_color = p.frame_bg;
    v.faint_bg_color = p.table_row_bg_alt;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::ZERO;
    v.window_highlight_topmost = false;
    v.hyperlink_color = p.text_link;
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.danger;
    v.weak_text_color = Some(p.text_disabled);
    v.override_text_color = Some(p.text);
    v.collapsing_header_frame = true;
    v.indent_has_left_vline = false;
    v.slider_trailing_fill = false;
    v.handle_shape = egui::style::HandleShape::Rect {
        aspect_ratio: 12.0 / FRAME_H, // ImGuiStyle::GrabMinSize across a frame
    };
    v.disabled_alpha = 0.6; // ImGuiStyle::DisabledAlpha
    v.text_cursor.stroke = Stroke::new(2.0, p.text);
    v
}

/// The per-state widget visuals and the selection, shared with the workstation's own scope.
pub fn fill_widgets(v: &mut Visuals, p: &Palette) {
    let w = &mut v.widgets;
    for (wv, weak, frame) in [
        (&mut w.noninteractive, p.window_bg, p.window_bg),
        (&mut w.inactive, p.button, p.frame_bg),
        (&mut w.hovered, p.button_hovered, p.frame_bg_hovered),
        (&mut w.active, p.button_active, p.frame_bg_active),
        (&mut w.open, p.button_active, p.frame_bg_active),
    ] {
        wv.weak_bg_fill = weak;
        wv.bg_fill = frame;
        wv.bg_stroke = Stroke::NONE;
        wv.fg_stroke = Stroke::new(1.0, p.text);
        wv.corner_radius = CornerRadius::ZERO;
        wv.expansion = 0.0;
    }
    // A separator is the one stroke a noninteractive widget draws.
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.separator);
    v.selection.bg_fill = p.header;
    v.selection.stroke = Stroke::new(1.0, p.text);
}

/// An egui slider with ImGui's solid grab. egui paints the grab with the widget's `bg_fill` —
/// the same `FrameBg` as the rail it rides on — ringed by `fg_stroke`; scoped to this one
/// slider, a stroke as thick as the grab is narrow fills it in `SliderGrab` (`SliderGrabActive`
/// while dragged). Scoped because the same stroke draws every checkbox's tick.
pub fn slider(ui: &mut egui::Ui, slider: egui::Slider<'_>) -> egui::Response {
    let p = current();
    let width = ui.spacing().interact_size.y * 0.5;
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        for (state, grab) in [
            (&mut w.inactive, p.slider_grab),
            (&mut w.hovered, p.slider_grab),
            (&mut w.active, p.slider_grab_active),
        ] {
            state.fg_stroke = Stroke::new(width, grab);
        }
        ui.add(slider)
    })
    .inner
}

/// An egui button held down the ImGui way: a toggle that is on keeps `ButtonActive`, where
/// `Button::selected` alone would paint it in the selected-row colour (`Header`).
pub trait Hold {
    fn held(self, on: bool) -> Self;
}

impl Hold for egui::Button<'_> {
    fn held(self, on: bool) -> Self {
        let b = self.selected(on);
        if on {
            b.fill(current().button_active)
        } else {
            b
        }
    }
}

/// Does `theme` request the high-contrast information layers (thicker strokes, high-contrast
/// colour tables)?
///
/// ponytail: no theme does since the Dear ImGui restyle retired the High contrast theme. The hook
/// stays (with [`overlay_stroke_scale`], [`vector_stroke_scale`], [`warning_alpha_for`] and
/// [`high_contrast_alt_name`]) so the map's information layers can respond again when an
/// accessibility theme returns; the `-HC` colour tables are still pickable by hand meanwhile.
pub fn is_high_contrast(_theme: Theme) -> bool {
    false
}

/// Stroke-width multiplier for geographic overlay polygons (warnings, outlooks, etc.).
pub fn overlay_stroke_scale(theme: Theme) -> f32 {
    if is_high_contrast(theme) {
        2.2
    } else {
        1.0
    }
}

/// Stroke-width multiplier for vector basemap strokes (roads, boundaries, waterways).
pub fn vector_stroke_scale(theme: Theme) -> f32 {
    if is_high_contrast(theme) {
        1.8
    } else {
        1.0
    }
}

/// Fill/stroke alphas for warning polygons, `(fill_alpha, stroke_alpha)` 0..255.
pub fn warning_alpha_for(theme: Theme) -> (u8, u8) {
    if is_high_contrast(theme) {
        (90, 255)
    } else {
        (45, 235)
    }
}

/// Which built-in colormap alternate a high-contrast theme prefers for `moment`. The alternates
/// live in `colormap.rs` (`REF-HC.pal`, `VEL-HC.pal`).
pub fn high_contrast_alt_name(moment: wxdata::level2::Moment) -> Option<&'static str> {
    if moment == wxdata::level2::Moment::Reflectivity {
        Some("High contrast (reflectivity)")
    } else if moment == wxdata::level2::Moment::Velocity {
        Some("High contrast (velocity)")
    } else {
        None
    }
}

/// A compact stat card: a frame-coloured box with the label over the value, both at the one
/// font size. Sized to a fixed width so several tile neatly in a `horizontal_wrapped` row.
pub fn stat_card(ui: &mut egui::Ui, label: &str, value: &str) {
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .inner_margin(Margin::symmetric(4, 3))
        .show(ui, |ui| {
            ui.set_width(108.0);
            ui.vertical(|ui| {
                ui.add(egui::Label::new(egui::RichText::new(label).weak()).truncate());
                ui.label(egui::RichText::new(value).monospace());
            });
        });
}

/// A min-max normalized sparkline of `vals` (oldest→newest) in a fixed-height row.
pub fn sparkline(ui: &mut egui::Ui, vals: &[f32], color: Color32) {
    let size = egui::vec2(ui.available_width().min(300.0), 34.0);
    sparkline_sized(ui, vals, color, size);
}

/// [`sparkline`] at a caller-chosen size, for places with a row height to fit inside — a table
/// cell has ~20 px, where the default 34 would overlap its neighbours. Drawn as ImGui's
/// `PlotLines`: a frame with the line in it, and the range written in only where there is room.
///
/// Returns the response so a caller can hang a tooltip on it.
pub fn sparkline_sized(
    ui: &mut egui::Ui,
    vals: &[f32],
    color: Color32,
    size: egui::Vec2,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    let weak = ui.visuals().weak_text_color();
    if vals.len() < 2 {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "no data",
            egui::FontId::proportional(FONT),
            weak,
        );
        return response;
    }
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for &v in vals {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    let span = (hi - lo).max(1e-3);
    let pad = 4.0;
    let pts: Vec<egui::Pos2> = vals
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x =
                rect.left() + pad + (rect.width() - 2.0 * pad) * i as f32 / (vals.len() - 1) as f32;
            let y = rect.bottom() - pad - (rect.height() - 2.0 * pad) * (v - lo) / span;
            egui::pos2(x, y)
        })
        .collect();
    painter.add(egui::Shape::line(pts, Stroke::new(1.5, color)));
    // Two lines of the one font need 30 px; a table-cell sparkline goes without its range.
    if rect.height() >= 30.0 {
        let font = egui::FontId::monospace(FONT);
        painter.text(
            rect.right_top() + egui::vec2(-3.0, 1.0),
            egui::Align2::RIGHT_TOP,
            format!("{hi:.0}"),
            font.clone(),
            weak,
        );
        painter.text(
            rect.right_bottom() + egui::vec2(-3.0, -1.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("{lo:.0}"),
            font,
            weak,
        );
    }
    response
}

/// A collapsible section: ImGui's framed CollapsingHeader, open by default.
pub fn section<R>(
    ui: &mut egui::Ui,
    title: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    egui::CollapsingHeader::new(title)
        .default_open(true)
        .show_unindented(ui, |ui| {
            ui.add_space(2.0);
            let r = add(ui);
            ui.add_space(4.0);
            r
        })
        .body_returned
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Theme;

    #[test]
    fn the_three_looks_are_dear_imgui_s_own_colours() {
        let dark = palette(Theme::Dark, true);
        // StyleColorsDark: CheckMark (0.26, 0.59, 0.98), TitleBgActive (0.16, 0.29, 0.48).
        assert_eq!(dark.check_mark, Color32::from_rgb(0x42, 0x96, 0xfa));
        assert_eq!(dark.title_bg_active, Color32::from_rgb(0x29, 0x4a, 0x7a));
        assert_eq!(dark.text, Color32::WHITE);
        let light = palette(Theme::Light, true);
        // StyleColorsLight: WindowBg (0.94, 0.94, 0.94, 1.0), FrameBg white, Text black.
        assert_eq!(light.window_bg, Color32::from_rgb(240, 240, 240));
        assert_eq!(light.frame_bg, Color32::WHITE);
        assert_eq!(light.text, Color32::BLACK);
        assert!(!light.is_dark);
        let classic = palette(Theme::Classic, true);
        // StyleColorsClassic: TitleBgActive (0.32, 0.32, 0.63) at 0.87 over the ground — purple.
        assert!(classic.title_bg_active.b() > classic.title_bg_active.g() + 40);
    }

    #[test]
    fn a_translucent_window_lets_the_ground_through() {
        // Classic's WindowBg is black at 0.85: what is left of the ground shows through.
        let classic = palette(Theme::Classic, true);
        let b = classic.window_bg;
        assert!(b.b() > 0 && b.b() < VIEWPORT_BG.b(), "{b:?}");
        // Dark's frames are its translucent navy composited over the window, so opaque.
        assert_eq!(palette(Theme::Dark, true).frame_bg.a(), 255);
    }

    #[test]
    fn system_follows_the_os() {
        assert_eq!(palette(Theme::System, true), palette(Theme::Dark, true));
        assert_eq!(palette(Theme::System, false), palette(Theme::Light, false));
    }

    #[test]
    fn every_frame_is_nineteen_points_and_nothing_is_rounded() {
        let mut style = Style::default();
        geometry(&mut style, false);
        assert_eq!(style.spacing.interact_size.y, 19.0);
        assert_eq!(style.spacing.button_padding, vec2(4.0, 3.0));
        assert_eq!(style.spacing.item_spacing, vec2(8.0, 4.0));
        let v = visuals(&palette(Theme::Dark, true));
        assert_eq!(v.window_corner_radius, CornerRadius::ZERO);
        assert_eq!(v.widgets.inactive.corner_radius, CornerRadius::ZERO);
        assert_eq!(v.widgets.hovered.bg_stroke, Stroke::NONE);
        assert_eq!(v.window_shadow, Shadow::NONE);
    }

    #[test]
    fn status_colours_read_on_every_window() {
        fn lum(c: Color32) -> f32 {
            let f = |v: u8| {
                let v = f32::from(v) / 255.0;
                if v <= 0.039_28 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        }
        for theme in [Theme::Dark, Theme::Light, Theme::Classic] {
            let p = palette(theme, true);
            for c in [p.live, p.warn, p.danger] {
                let (a, b) = (lum(c), lum(p.window_bg));
                let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                assert!(ratio >= 4.5, "{theme:?} {c:?} is {ratio:.2}:1");
            }
        }
    }
}
