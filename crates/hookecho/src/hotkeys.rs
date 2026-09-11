//! Keyboard shortcuts.
//!
//! One flat table maps a [`egui::KeyboardShortcut`] to a [`BindableAction`]. Most actions are
//! just registry entries ([`PaletteAction`]), so a hotkey, a drawer row and a command-palette hit
//! all run the same code; the handful that aren't (tilt stepping, the OBS toggles, fullscreen)
//! are app-level variants. [`defaults`] is the shipped table; [`active`] swaps in the user's
//! overrides from settings without touching call sites.

use crate::app::{AppWindow, PaletteAction};
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
        plain(K::Questionmark, A::CheatSheet),
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
    let typing = ctx.memory(|m| m.focused().is_some());
    ctx.input_mut(|i| {
        bindings
            .iter()
            .filter(|b| !(typing && steals_typing(b.shortcut)))
            .filter(|b| i.consume_shortcut(&b.shortcut))
            .map(|b| b.action)
            .collect()
    })
}

/// Would this shortcut swallow a keystroke meant for a focused text field?
fn steals_typing(s: egui::KeyboardShortcut) -> bool {
    s.modifiers.is_none() && s.logical_key.name().len() == 1
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

    #[test]
    fn printable_keys_yield_to_text_fields_and_modifiers_dont() {
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
}
