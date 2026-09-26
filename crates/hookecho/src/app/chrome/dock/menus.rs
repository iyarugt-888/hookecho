//! The app bar's menus — Tools, Share, Settings, Help — and where every window lives in them.
//!
//! [`window_home`] is an exhaustive match on purpose: a new `AppWindow` does not compile until it
//! has a place in the workstation, which is how "every feature is reachable from the Dock" stays
//! true rather than being true once.

use super::*;

/// The app-bar menus (and the one window with a button of its own).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Menu {
    Tools,
    Settings,
    Help,
    /// Its own app-bar button: the forecast discussion.
    Discussion,
}

/// Every window the palette can open, in menu order. Kept beside [`window_home`], whose
/// exhaustive match is what forces a new window to be added here too.
pub(super) const ALL_WINDOWS: [AppWindow; 24] = [
    AppWindow::StormTable,
    AppWindow::Digest,
    AppWindow::Cappi,
    AppWindow::Volume3d,
    AppWindow::UdpProducts,
    AppWindow::Climatology,
    AppWindow::Verify,
    AppWindow::ModelVerify,
    AppWindow::Site,
    AppWindow::LayerManager,
    AppWindow::Placefiles,
    AppWindow::Palettes,
    AppWindow::Markers,
    AppWindow::DataHealth,
    AppWindow::Events,
    AppWindow::ChaseReplay,
    AppWindow::AlertRules,
    AppWindow::Afd,
    AppWindow::Tropical,
    AppWindow::Settings,
    AppWindow::Help,
    AppWindow::Tour,
    AppWindow::Setup,
    AppWindow::About,
];

/// Where the workstation offers a window: which menu, under which group heading, and its label.
pub(super) fn window_home(w: AppWindow) -> (Menu, &'static str, &'static str) {
    use AppWindow as W;
    const ANALYSIS: &str = "Analysis";
    const DATA: &str = "Data & map";
    const EVENTS: &str = "Events & alerts";
    match w {
        W::StormTable => (Menu::Tools, ANALYSIS, "Storm attributes"),
        W::Digest => (Menu::Tools, ANALYSIS, "Storm digest"),
        W::Cappi => (Menu::Tools, ANALYSIS, "Constant-height slice (CAPPI)"),
        W::Volume3d => (Menu::Tools, ANALYSIS, "3D volume"),
        W::UdpProducts => (Menu::Tools, ANALYSIS, "User-defined products"),
        W::Climatology => (Menu::Tools, ANALYSIS, "Tornado climatology"),
        W::Verify => (Menu::Tools, ANALYSIS, "Warning verification"),
        W::ModelVerify => (Menu::Tools, ANALYSIS, "Model verification"),
        W::Site => (Menu::Tools, DATA, "Radar site"),
        W::LayerManager => (Menu::Tools, DATA, "Layer manager"),
        W::Placefiles => (Menu::Tools, DATA, "Placefile manager"),
        W::Palettes => (Menu::Tools, DATA, "Color-table editor"),
        W::Markers => (Menu::Tools, DATA, "Location markers"),
        W::DataHealth => (Menu::Tools, DATA, "Data source health"),
        W::Events => (Menu::Tools, EVENTS, "Event library"),
        W::ChaseReplay => (Menu::Tools, EVENTS, "Chase replay"),
        W::AlertRules => (Menu::Tools, EVENTS, "Alert rules"),
        W::Afd => (Menu::Discussion, "", "Forecast discussion"),
        W::Tropical => (Menu::Discussion, "", "Tropical models & advisories"),
        W::Settings => (Menu::Settings, "", "Settings\u{2026}"),
        W::Help => (Menu::Help, "", "Help, glossary and shortcuts"),
        W::Tour => (Menu::Help, "", "Take the tour"),
        W::Setup => (Menu::Help, "", "Set up again"),
        W::About => (Menu::Help, "", "About HookEcho"),
    }
}

/// What a menu row asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum MenuPick {
    Palette(crate::app::PaletteAction),
    /// Open the Preferences window on a page, optionally inside one of its App sections.
    Prefs(PrefsPage, Option<&'static str>),
    /// The keyboard cheat sheet.
    Shortcuts,
}

/// A menu's windows, grouped under their headings in [`ALL_WINDOWS`] order.
pub(super) fn windows_in(menu: Menu) -> Vec<(&'static str, &'static str, AppWindow)> {
    ALL_WINDOWS
        .iter()
        .map(|&w| (w, window_home(w)))
        .filter(|(_, (m, _, _))| *m == menu)
        .map(|(w, (_, group, label))| (group, label, w))
        .collect()
}

/// Draw a menu's window rows (with a heading whenever the group changes); the row picked, if any.
pub(super) fn window_rows(ui: &mut egui::Ui, t: &ws::Tokens, menu: Menu) -> Option<MenuPick> {
    let mut pick = None;
    let mut group = "";
    for (g, label, w) in windows_in(menu) {
        if g != group && !g.is_empty() {
            if !group.is_empty() {
                ui.separator();
            }
            ui.label(ws::text(g.to_uppercase(), 10.5, t.text_faint));
            group = g;
        }
        if ui.button(label).clicked() {
            pick = Some(MenuPick::Palette(crate::app::PaletteAction::OpenWindow(w)));
        }
    }
    pick
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_window_has_one_home_and_no_window_is_listed_twice() {
        let mut seen = std::collections::HashSet::new();
        for w in ALL_WINDOWS {
            assert!(seen.insert(format!("{w:?}")), "{w:?} listed twice");
            let (_, _, label) = window_home(w);
            assert!(!label.is_empty(), "{w:?} has no label");
        }
        // The menus between them list every window exactly once.
        let listed: usize = [Menu::Tools, Menu::Settings, Menu::Help, Menu::Discussion]
            .into_iter()
            .map(|m| windows_in(m).len())
            .sum();
        assert_eq!(listed, ALL_WINDOWS.len());
    }

    #[test]
    fn the_tools_menu_groups_stay_together() {
        let groups: Vec<&str> = windows_in(Menu::Tools).iter().map(|(g, _, _)| *g).collect();
        let mut runs = groups.clone();
        runs.dedup();
        let mut unique = runs.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(runs.len(), unique.len(), "a group split in two: {groups:?}");
    }
}
