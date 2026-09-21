//! Keyboard shortcuts.
//!
//! One flat table maps a [`egui::KeyboardShortcut`] to a [`BindableAction`]. Most actions are
//! just registry entries ([`PaletteAction`]), so a hotkey, a drawer row and a command-palette hit
//! all run the same code; the handful that aren't (tilt stepping, the OBS toggles, fullscreen)
//! are app-level variants. [`defaults`] is the shipped table; [`active`] swaps in the user's
//! overrides from settings without touching call sites.

use crate::app::{AppWindow, MapTool, PaletteAction};
use crate::settings::Settings;
use std::borrow::Cow;
use wxdata::level2::Moment;

/// A thing a key can trigger. Everything the registry already knows how to do rides in
/// `Palette`; the rest are app-level and have no drawer row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum BindableAction {
    Palette(PaletteAction),
    TiltUp,
    TiltDown,
    /// Nudge the active pane's map-pitch 3D camera tilt/rotation — a keyboard alternative to the
    /// right-drag gesture, and the only way to adjust it at all without a mouse. No-op outside 3D
    /// map mode, same as `TiltUp`/`TiltDown` are no-ops with no volume loaded. Off the arrow keys
    /// on purpose: those are fully the timeline's, so a camera nudge can never eat a scrub.
    Camera3dPitchUp,
    Camera3dPitchDown,
    Camera3dBearingLeft,
    Camera3dBearingRight,
    OpenSiteDialog,
    ToggleAlertPanel,
    ToggleObs,
    ToggleObsTour,
    ToggleDrawer,
    StepBack,
    StepForward,
    /// The coarse timeline jump beside `StepBack`/`StepForward`'s one-frame step: about an hour of
    /// archive time, so finding "around 22Z" doesn't mean stepping through every 4-6 minute volume
    /// between here and there by hand.
    StepHourBack,
    StepHourForward,
    Fullscreen,
    CommandSearch,
    CheatSheet,
    ToggleMute,
    /// ROADMAP_NEW J6's "pane focus": move which pane is active without a click, wrapping at
    /// either end. A no-op with one pane, the same way `TiltUp`/`TiltDown` no-op with no volume.
    FocusPrevPane,
    FocusNextPane,
    /// ROADMAP_NEW J6's "product next/previous": step through `Moment::ALL` in order, wrapping.
    /// Distinct from the `1`-`7` keys, which jump straight to one specific moment — cycling is
    /// what you want with a hand on the mouse, stepping REF → VEL → CC across one storm.
    ProductPrev,
    ProductNext,
}

/// A key (with modifiers) bound to an action.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Binding {
    pub shortcut: egui::KeyboardShortcut,
    pub action: BindableAction,
}

const fn plain(key: egui::Key, action: BindableAction) -> Binding {
    Binding {
        shortcut: egui::KeyboardShortcut::new(egui::Modifiers::NONE, key),
        action,
    }
}

/// The shipped table: 1–6 select products, PageUp/Down change tilt, F3 site dialog, F5 reload,
/// and so on. Escape is deliberately absent — it means "close/cancel whatever is in front of
/// you", which is per-widget and not something one global binding can own.
pub(crate) fn defaults() -> Vec<Binding> {
    use egui::Key as K;
    use BindableAction as A;
    use PaletteAction as P;
    vec![
        plain(
            K::Num1,
            A::Palette(P::SetMoment(Moment::Reflectivity, false)),
        ),
        plain(K::Num2, A::Palette(P::SetMoment(Moment::Velocity, false))),
        plain(
            K::Num3,
            A::Palette(P::SetMoment(Moment::SpectrumWidth, false)),
        ),
        plain(
            K::Num4,
            A::Palette(P::SetMoment(Moment::DifferentialReflectivity, false)),
        ),
        plain(
            K::Num5,
            A::Palette(P::SetMoment(Moment::DifferentialPhase, false)),
        ),
        plain(
            K::Num6,
            A::Palette(P::SetMoment(Moment::SpecificDifferentialPhase, false)),
        ),
        plain(
            K::Num7,
            A::Palette(P::SetMoment(Moment::CorrelationCoefficient, false)),
        ),
        plain(K::PageUp, A::TiltUp),
        plain(K::PageDown, A::TiltDown),
        plain(K::F3, A::OpenSiteDialog),
        plain(K::F5, A::Palette(P::Reload)),
        plain(K::Z, A::Palette(P::CycleBasemap)),
        plain(K::A, A::ToggleAlertPanel),
        // F7 opened the Advanced toolbox before it was dissolved into the drawer; it keeps
        // working, pointed at the drawer, so the muscle memory still lands somewhere.
        plain(K::F7, A::ToggleDrawer),
        plain(K::L, A::ToggleDrawer),
        // All four arrow keys are the timeline's, full stop: Left/Right step one volume (the
        // transport buttons were the only way to do that, which made every scripted capture
        // depend on hitting them by pixel), Up/Down jump about an hour so finding "around 22Z" in
        // a day of 4-6 minute volumes doesn't mean stepping through every one of them by hand.
        plain(K::ArrowLeft, A::StepBack),
        plain(K::ArrowRight, A::StepForward),
        plain(K::ArrowUp, A::StepHourForward),
        plain(K::ArrowDown, A::StepHourBack),
        // The map-pitch 3D camera takes W/S (tilt) and Q/E (rotate) instead — a flight-sim-style
        // pairing that reads as "look up/down, turn left/right" without touching a single arrow
        // key or any letter another binding already owns.
        plain(K::W, A::Camera3dPitchUp),
        plain(K::S, A::Camera3dPitchDown),
        plain(K::Q, A::Camera3dBearingLeft),
        plain(K::E, A::Camera3dBearingRight),
        plain(K::F8, A::ToggleObs),
        plain(K::F9, A::ToggleObsTour),
        plain(K::R, A::Palette(P::InstantReplay)),
        plain(K::M, A::ToggleMute),
        plain(K::F11, A::Fullscreen),
        plain(K::T, A::Palette(P::ToggleRibbon)),
        // ROADMAP_NEW J6: previously reachable only through the command palette or a click.
        // `End` matches the usual media-timeline convention (`Home` = oldest, `End` = most
        // recent) rather than reusing an arrow key, all four of which are already the timeline's.
        plain(K::End, A::Palette(P::GoLive)),
        // `G`ate inspector — reads an exact value/geometry at a click, the closest existing tool
        // to J6's "sample tool" ask. `Interrogate` (the other click tool) is already one tap away
        // from anything else, since tapping the active tool again returns to it (see `poll`'s own
        // dispatch), so it doesn't need a dedicated key of its own.
        plain(K::G, A::Palette(P::Tool(MapTool::GateInspector))),
        // `X` for cross-section (crossed lines), `V` for the vertical profile a sounding shows.
        plain(K::X, A::Palette(P::Tool(MapTool::CrossSection))),
        plain(K::V, A::Palette(P::Tool(MapTool::Sounding))),
        // `[`/`]` cycle which pane is active (ROADMAP_NEW J6's "pane focus") — off `Tab` on
        // purpose: egui already owns `Tab` for widget-to-widget focus, and a global shortcut
        // consuming it first would break that keyboard navigation Q3 already confirmed working.
        plain(K::OpenBracket, A::FocusPrevPane),
        plain(K::CloseBracket, A::FocusNextPane),
        // `N`ext/`P`revious product. Letters rather than a punctuation pair like the brackets
        // above: a single-character key name is what `steals_typing` uses to decide a shortcut
        // must yield to a focused text field, so these stay out of the way while someone is
        // typing a site id or a marker name.
        plain(K::P, A::ProductPrev),
        plain(K::N, A::ProductNext),
        // `D` for the 3D map view, which until now had no shortcut and no palette entry at all —
        // it was reachable only from a dropdown inside the 3D options panel.
        plain(K::D, A::Palette(P::ToggleMap3d)),
        plain(K::Questionmark, A::CheatSheet),
        // Every action above that lives only on a function key or Page Up/Down gets a second,
        // ordinary key too. A tablet's cover keyboard has no F row and no Page keys, and a
        // shortcut nobody can press is not a shortcut: `,` and `.` are the tilt pair (they read as
        // `<` and `>`), `F` finds a site, `U` updates (reloads), `K` opens command search where
        // Ctrl+K cannot be typed (Android reports no modifier state), `/` and `H` are help.
        plain(K::Comma, A::TiltDown),
        plain(K::Period, A::TiltUp),
        plain(K::F, A::OpenSiteDialog),
        plain(K::U, A::Palette(P::Reload)),
        plain(K::K, A::CommandSearch),
        plain(K::Slash, A::CheatSheet),
        plain(K::H, A::Palette(P::OpenWindow(AppWindow::Help))),
        plain(K::O, A::ToggleObs),
        plain(K::B, A::ToggleObsTour),
        plain(K::Backslash, A::Fullscreen),
        // `?` stays the shortcut overlay; F1 is the searchable hub the overlay points at.
        plain(K::F1, A::Palette(P::OpenWindow(AppWindow::Help))),
        Binding {
            shortcut: egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::K),
            action: A::CommandSearch,
        },
    ]
}

/// The bindings in force: the user's table if they have edited one, otherwise [`defaults`].
pub(crate) fn active(settings: &Settings) -> Cow<'_, [Binding]> {
    if settings.keybinds.is_empty() {
        Cow::Owned(defaults())
    } else {
        Cow::Borrowed(&settings.keybinds)
    }
}

/// An existing binding that already owns `shortcut` (ignoring the row being edited).
pub(crate) fn conflict(
    bindings: &[Binding],
    shortcut: egui::KeyboardShortcut,
    editing: BindableAction,
) -> Option<&Binding> {
    bindings
        .iter()
        .find(|b| b.shortcut == shortcut && b.action != editing)
}

/// Actions triggered this frame.
///
/// A bare printable key does nothing while a text field has focus, so typing a site id doesn't
/// fire product shortcuts. Anything with a modifier, or a function key, still fires — Ctrl+K from
/// inside the search box and F5 while typing are what every other desktop app does.
pub(crate) fn poll(ctx: &egui::Context, bindings: &[Binding]) -> Vec<BindableAction> {
    // A text field, not merely any focused widget: tapping a checkbox or button focuses it too, and
    // treating that as typing left the single-key shortcuts dead until focus was cleared.
    let typing = ctx.text_edit_focused();
    ctx.input_mut(|i| {
        let pressed: Vec<egui::Key> = i
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key {
                    key, pressed: true, ..
                } => Some(*key),
                _ => None,
            })
            .collect();
        let typed: Vec<String> = i
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Text(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        if let Some(what) = i.events.iter().rev().find_map(|e| match e {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => Some(format!(
                "key {}",
                pretty(&egui::KeyboardShortcut::new(*modifiers, *key))
            )),
            egui::Event::Text(t) => Some(format!("text {t:?}")),
            _ => None,
        }) {
            if let Ok(mut last) = LAST_INPUT.lock() {
                *last = what;
            }
        }
        let mut out: Vec<BindableAction> = bindings
            .iter()
            .filter(|b| !(typing && steals_typing(b.shortcut)))
            .filter(|b| i.consume_shortcut(&b.shortcut))
            .map(|b| b.action)
            .collect();
        if !typing {
            out.extend(text_fallback(&pressed, &typed, bindings));
        }
        out
    })
}

/// Whether `key` is one an ordinary keyboard has without a function row or a Fn layer: a letter,
/// digit or punctuation key, the arrows, Home and End. Page Up/Down, Insert, Delete and the F keys
/// are what a tablet's cover keyboard leaves out.
pub(crate) fn on_a_plain_keyboard(key: egui::Key) -> bool {
    key.symbol_or_name().chars().count() == 1
        || matches!(
            key,
            egui::Key::ArrowUp
                | egui::Key::ArrowDown
                | egui::Key::ArrowLeft
                | egui::Key::ArrowRight
                | egui::Key::Home
                | egui::Key::End
        )
}

/// Bring a saved key table up to date without touching what the user chose: any action that has no
/// binding on a plain key gets its shipped plain-key binding, if that key is free. A table saved
/// before the plain-key alternatives existed would otherwise keep tilt, site and reload on keys a
/// tablet keyboard lacks, and opening the Hotkeys tab copies the whole shipped table into settings,
/// so that is most saved tables. Idempotent.
pub(crate) fn fill_plain_keys(table: &mut Vec<Binding>) {
    if table.is_empty() {
        return; // the shipped table is used as is
    }
    for d in defaults() {
        if !on_a_plain_keyboard(d.shortcut.logical_key) || !d.shortcut.modifiers.is_none() {
            continue;
        }
        let has_plain = table.iter().any(|b| {
            b.action == d.action
                && on_a_plain_keyboard(b.shortcut.logical_key)
                && b.shortcut.modifiers.is_none()
        });
        let key_free = !table.iter().any(|b| b.shortcut == d.shortcut);
        if !has_plain && key_free {
            table.push(d);
        }
    }
}

/// Keystrokes that arrived as typed text with no key event beside them, as bindings.
///
/// A desktop delivers a key event for every press, and a typed character rides along with it; the
/// key event is what shortcuts read. Some Android paths deliver the character only, so a shortcut
/// keyed off key events never fires there. This turns a lone typed character back into the key it
/// names, but only when no key event for that key came in the same frame, so a press that produced
/// both is never counted twice.
pub(crate) fn text_fallback(
    pressed: &[egui::Key],
    typed: &[String],
    bindings: &[Binding],
) -> Vec<BindableAction> {
    let mut out = Vec::new();
    for t in typed {
        let mut chars = t.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            continue;
        };
        let Some(key) = egui::Key::from_name(&c.to_string()) else {
            continue;
        };
        if pressed.contains(&key) {
            continue;
        }
        for b in bindings {
            if b.shortcut.logical_key == key && b.shortcut.modifiers.is_none() {
                out.push(b.action);
            }
        }
    }
    out
}

/// What the last key event or typed character was, for the Hotkeys tab's "does my keyboard reach
/// the app" line. Empty until something arrives.
static LAST_INPUT: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// The last input recorded by [`poll`], as text.
pub(crate) fn last_input() -> String {
    LAST_INPUT.lock().map(|s| s.clone()).unwrap_or_default()
}

/// Would this shortcut swallow a keystroke meant for a focused text field?
fn steals_typing(s: egui::KeyboardShortcut) -> bool {
    s.modifiers.is_none() && s.logical_key.symbol_or_name().chars().count() == 1
}

/// Compact shortcut text for painting on a drawer row ("Ctrl+K", "F5", "1"). egui's own
/// `format_shortcut` needs a Context; the registry is built without one.
pub(crate) fn pretty(s: &egui::KeyboardShortcut) -> String {
    let m = s.modifiers;
    let mut out = String::new();
    for (on, name) in [
        (m.ctrl || m.command, "Ctrl+"),
        (m.alt, "Alt+"),
        (m.shift, "Shift+"),
    ] {
        if on {
            out.push_str(name);
        }
    }
    out.push_str(s.logical_key.name());
    out
}

/// Human label for a non-registry action. `Palette` rows borrow the registry's own label, so
/// they're resolved by the caller (which has the entries) rather than duplicated here.
pub(crate) fn label(action: BindableAction) -> Option<&'static str> {
    Some(match action {
        BindableAction::Palette(_) => return None,
        BindableAction::TiltUp => "Tilt up",
        BindableAction::TiltDown => "Tilt down",
        BindableAction::Camera3dPitchUp => "3D camera: tilt more",
        BindableAction::Camera3dPitchDown => "3D camera: tilt less",
        BindableAction::Camera3dBearingLeft => "3D camera: rotate left",
        BindableAction::Camera3dBearingRight => "3D camera: rotate right",
        BindableAction::StepHourBack => "Jump back ~1 hour",
        BindableAction::StepHourForward => "Jump forward ~1 hour",
        BindableAction::OpenSiteDialog => "Change radar site",
        BindableAction::ToggleAlertPanel => "Alerts panel",
        BindableAction::ToggleObs => "Streamer (OBS) mode",
        BindableAction::ToggleObsTour => "Streamer auto-tour",
        BindableAction::ToggleDrawer => "Layers drawer",
        BindableAction::StepBack => "Previous frame",
        BindableAction::StepForward => "Next frame",
        BindableAction::Fullscreen => "Fullscreen",
        BindableAction::CommandSearch => "Search commands",
        BindableAction::CheatSheet => "Keyboard shortcuts",
        BindableAction::ToggleMute => "Mute audio alerts",
        BindableAction::FocusPrevPane => "Focus previous pane",
        BindableAction::FocusNextPane => "Focus next pane",
        BindableAction::ProductPrev => "Previous product",
        BindableAction::ProductNext => "Next product",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_no_conflicting_shortcuts() {
        let d = defaults();
        for (i, a) in d.iter().enumerate() {
            for b in &d[i + 1..] {
                // F7 and L both open the drawer on purpose; a repeat is only a bug when the two
                // rows disagree about what the key does.
                assert!(
                    a.shortcut != b.shortcut || a.action == b.action,
                    "{:?} bound to two actions",
                    a.shortcut
                );
            }
        }
    }

    /// ROADMAP_NEW J6: "live", "sample tool", "cross section", and "sounding" each reach an
    /// existing `PaletteAction` that a command-palette hit or a click already ran — this is only
    /// checking the new keyboard door in, not a second implementation of any of them.
    #[test]
    fn j6_shortcuts_reach_the_actions_the_roadmap_named() {
        use PaletteAction as P;
        let d = defaults();
        let has = |action: BindableAction| d.iter().any(|b| b.action == action);
        assert!(has(BindableAction::Palette(P::GoLive)), "jump to live");
        assert!(
            has(BindableAction::Palette(P::Tool(MapTool::GateInspector))),
            "sample tool"
        );
        assert!(
            has(BindableAction::Palette(P::Tool(MapTool::CrossSection))),
            "cross section"
        );
        assert!(
            has(BindableAction::Palette(P::Tool(MapTool::Sounding))),
            "sounding"
        );
        assert!(has(BindableAction::FocusPrevPane), "pane focus (previous)");
        assert!(has(BindableAction::FocusNextPane), "pane focus (next)");
        assert!(has(BindableAction::ProductPrev), "product previous");
        assert!(has(BindableAction::ProductNext), "product next");
        assert!(has(BindableAction::Palette(P::ToggleMap3d)), "3D");
    }

    /// The product-cycling keys have to yield to a focused text field the way every other
    /// single-letter shortcut does — otherwise typing a site id containing "n" would silently
    /// change the product out from under the person typing it.
    #[test]
    fn the_product_cycling_keys_yield_to_text_fields() {
        for binding in defaults() {
            if matches!(
                binding.action,
                BindableAction::ProductPrev | BindableAction::ProductNext
            ) {
                assert!(
                    steals_typing(binding.shortcut),
                    "{:?} must be a plain printable key so typing wins",
                    binding.shortcut
                );
            }
        }
    }

    #[test]
    fn printable_keys_yield_to_text_fields_and_modifiers_dont() {
        // Punctuation is typed into fields too: a comma in a marker name must not tilt the radar.
        for k in [
            egui::Key::Comma,
            egui::Key::Period,
            egui::Key::Slash,
            egui::Key::Backslash,
        ] {
            assert!(
                steals_typing(egui::KeyboardShortcut::new(egui::Modifiers::NONE, k)),
                "{k:?}"
            );
        }
        assert!(steals_typing(egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::A
        )));
        assert!(!steals_typing(egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::K
        )));
        assert!(!steals_typing(egui::KeyboardShortcut::new(
            egui::Modifiers::NONE,
            egui::Key::F5
        )));
    }

    #[test]
    fn every_action_has_a_key_on_a_keyboard_with_no_function_row() {
        let d = defaults();
        for b in &d {
            let reachable = d.iter().any(|o| {
                o.action == b.action
                    && o.shortcut.modifiers.is_none()
                    && on_a_plain_keyboard(o.shortcut.logical_key)
            });
            assert!(
                reachable,
                "{:?} can only be reached from a key a tablet keyboard lacks ({})",
                b.action,
                pretty(&b.shortcut)
            );
        }
    }

    #[test]
    fn the_plain_key_test_knows_what_a_cover_keyboard_lacks() {
        use egui::Key as K;
        for k in [
            K::A,
            K::Num5,
            K::Comma,
            K::Slash,
            K::ArrowUp,
            K::Home,
            K::End,
        ] {
            assert!(on_a_plain_keyboard(k), "{k:?}");
        }
        for k in [K::PageUp, K::PageDown, K::F1, K::F11, K::Insert, K::Delete] {
            assert!(!on_a_plain_keyboard(k), "{k:?}");
        }
    }

    #[test]
    fn a_table_saved_before_the_plain_keys_existed_gains_them_and_keeps_its_own_choices() {
        use egui::Key as K;
        let old_only: Vec<Binding> = defaults()
            .into_iter()
            .filter(|b| {
                !matches!(
                    b.shortcut.logical_key,
                    K::Comma
                        | K::Period
                        | K::F
                        | K::U
                        | K::K
                        | K::Slash
                        | K::H
                        | K::O
                        | K::B
                        | K::Backslash
                )
            })
            .collect();
        let mut table = old_only.clone();
        // The user gave tilt-up their own key, and it is a plain one: it must be left alone.
        for b in table
            .iter_mut()
            .filter(|b| b.action == BindableAction::TiltUp)
        {
            b.shortcut = egui::KeyboardShortcut::new(egui::Modifiers::NONE, K::Y);
        }
        fill_plain_keys(&mut table);
        let keys_for = |a: BindableAction| -> Vec<K> {
            table
                .iter()
                .filter(|b| b.action == a)
                .map(|b| b.shortcut.logical_key)
                .collect()
        };
        assert!(
            keys_for(BindableAction::TiltDown).contains(&K::Comma),
            "filled in"
        );
        assert!(keys_for(BindableAction::OpenSiteDialog).contains(&K::F));
        assert_eq!(
            keys_for(BindableAction::TiltUp)
                .iter()
                .filter(|k| **k == K::Period)
                .count(),
            0,
            "tilt up already had a plain key of the user's choosing"
        );
        // Idempotent, and an empty table (the shipped one) is left empty.
        let once = table.clone();
        fill_plain_keys(&mut table);
        assert_eq!(table, once);
        let mut empty = Vec::new();
        fill_plain_keys(&mut empty);
        assert!(empty.is_empty());
    }

    #[test]
    fn a_key_the_user_took_for_something_else_is_not_stolen_back() {
        use egui::Key as K;
        let mut table = vec![
            Binding {
                shortcut: egui::KeyboardShortcut::new(egui::Modifiers::NONE, K::Comma),
                action: BindableAction::ToggleMute,
            },
            plain(K::PageDown, BindableAction::TiltDown),
        ];
        fill_plain_keys(&mut table);
        assert_eq!(
            table
                .iter()
                .filter(|b| b.shortcut.logical_key == K::Comma)
                .count(),
            1,
            "the comma stays the user's mute key"
        );
    }

    #[test]
    fn a_typed_character_with_no_key_event_fires_its_binding_once() {
        use egui::Key as K;
        let b = defaults();
        let tilt_up = |v: Vec<BindableAction>| v.contains(&BindableAction::TiltUp);
        // Character only: fires.
        assert!(tilt_up(text_fallback(&[], &[".".into()], &b)));
        // The key event came too: the normal path owns it, so no second fire.
        assert!(!tilt_up(text_fallback(&[K::Period], &[".".into()], &b)));
        // Pasted or IME text is not a keystroke, and unknown characters do nothing.
        assert!(text_fallback(&[], &["hello".into()], &b).is_empty());
        assert!(text_fallback(&[], &["\u{e9}".into()], &b).is_empty());
        // Upper case (Caps Lock) names the same key.
        assert!(text_fallback(&[], &["N".into()], &b).contains(&BindableAction::ProductNext));
    }
}
