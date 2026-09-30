//! One place to look things up: shortcuts, vocabulary, the tour, and what changed.
//!
//! Help used to be scattered — a glossary window, a `?` overlay of key bindings, a tour you had
//! to know lived in Settings — and none of them knew about each other. Somebody who does not
//! know the word "TDS" also does not know which of three surfaces would have told them. So there
//! is one search box over all of it, and the sections underneath it are what that box matched.
//!
//! ponytail: no index, no fuzzy matcher — a substring pass over ~15 glossary entries and ~25
//! bindings is nothing per frame. The registry's fuzzy matcher is for the command palette, where
//! the user is typing an action name and expects typo tolerance; here they are reading.

use crate::app::PaletteEntry;
use crate::hotkeys::{self, BindableAction, Binding};

/// What's-new comes from the changelog itself: one file to write at release time, not two.
const CHANGELOG: &str = include_str!("../../../../CHANGELOG.md");

/// Small things people miss, shown one at a time and rotated so a second look is a second tip.
const TIPS: &[&str] = &[
    "Drag the scrubber to travel in time; the LIVE badge takes you back to now.",
    "Ctrl+K searches every command in the app, including ones with no button.",
    "Search examples: reflectivity, station KTLX, tool measure, or time 21:30Z.",
    "Long-press or right-click the map to interrogate a pixel: every moment at that point.",
    "Panes can each run their own site, product and tilt — set panes to 4 and compare tilts.",
    "Alert rules decide what is worth interrupting you for. Nothing else makes a sound.",
    "Save an arrangement as a workspace and it comes back exactly, panes and all.",
    "A hook in reflectivity is a reason to look at velocity, not a tornado by itself.",
];

/// Which half of the page is up.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Tab {
    #[default]
    Help,
    Changelog,
}

#[derive(Default)]
pub(crate) struct HelpHub {
    pub open: bool,
    query: String,
    tab: Tab,
    changelog: crate::ui::changelog::View,
}

/// The changelog, parsed once.
fn releases() -> &'static [crate::ui::changelog::Release] {
    static R: std::sync::OnceLock<Vec<crate::ui::changelog::Release>> = std::sync::OnceLock::new();
    R.get_or_init(|| crate::ui::changelog::parse(CHANGELOG))
}

impl HelpHub {
    pub(crate) fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.query.clear();
        }
    }

    /// Open the hub with the search box already pointed at one glossary term, for the ⓘ links on
    /// the layer rows.
    pub(crate) fn explain(&mut self, entry: usize) {
        self.open = true;
        self.tab = Tab::Help;
        self.query = crate::ui::glossary::ENTRIES
            .get(entry)
            .map(|e| {
                e.term
                    .split([' ', '\u{2014}'])
                    .next()
                    .unwrap_or(e.term)
                    .to_string()
            })
            .unwrap_or_default();
    }

    /// Draw the page. Returns true when the user asked for the tour — the app owns the tour, and
    /// starting it from here would mean this module knowing about app state.
    pub(crate) fn show(
        &mut self,
        ctx: &egui::Context,
        drawer: &mut crate::ui::drawer::Drawer,
        bindings: &[Binding],
        entries: &[PaletteEntry],
    ) -> bool {
        let mut open = self.open;
        // The changelog reads better wider; the drawer still clamps it to the screen.
        let width = if self.tab == Tab::Changelog {
            560.0
        } else {
            380.0
        };
        let Some(window) = drawer.page_sized(
            ctx,
            "Help",
            &mut open,
            false,
            width,
            egui::Window::new("Help"),
        ) else {
            self.open = open;
            return false;
        };
        let mut tour = false;
        window.show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Help, "Help");
                ui.selectable_value(&mut self.tab, Tab::Changelog, "Changelog");
            });
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text(if self.tab == Tab::Changelog {
                            "Search every release\u{2026}"
                        } else {
                            "Search help\u{2026}"
                        })
                        .desired_width(f32::INFINITY),
                );
            });
            let q = self.query.trim().to_ascii_lowercase();
            if self.tab == Tab::Changelog {
                ui.add_space(4.0);
                self.changelog.show(ui, releases(), &q);
                return;
            }

            if q.is_empty() {
                ui.add_space(6.0);
                // Rotates on its own so the same card is not the only thing anyone ever reads.
                let i = (ctx.input(|i| i.time) / 12.0) as usize % TIPS.len();
                ui.label(egui::RichText::new(TIPS[i]).italics());
                ctx.request_repaint_after(std::time::Duration::from_secs(1));
            }
            ui.add_space(4.0);
            if ui
                .button(format!(
                    "{} Take the 60-second tour",
                    egui_phosphor::regular::PLAY_CIRCLE
                ))
                .on_hover_text("Four stops on the live map: the timeline, the products, where everything lives, and how to read a storm")
                .clicked()
            {
                tour = true;
            }
            ui.separator();

            let keys = shortcut_rows(bindings, entries, &q);
            if !keys.is_empty() {
                crate::theme::section(ui, "Keyboard shortcuts", |ui| {
                    for (key, label) in &keys {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(key)
                                    .monospace()
                                    .size(crate::ui::style::FONT_SM)
                                    .background_color(egui::Color32::from_white_alpha(18)),
                            );
                            ui.label(label);
                        });
                    }
                });
            }

            let terms: Vec<&crate::ui::glossary::Entry> = crate::ui::glossary::ENTRIES
                .iter()
                .filter(|e| {
                    q.is_empty()
                        || e.term.to_ascii_lowercase().contains(&q)
                        || e.body.to_ascii_lowercase().contains(&q)
                })
                .collect();
            if !terms.is_empty() {
                crate::theme::section(ui, "What the map is saying", |ui| {
                    for e in terms {
                        ui.label(egui::RichText::new(e.term).strong());
                        ui.label(e.body);
                        ui.add_space(6.0);
                    }
                });
            }

            // What's new: the newest few entry titles, each opening the changelog; the full text
            // lives on the Changelog tab rather than as a wall here.
            let newest = releases().first();
            let news: Vec<&crate::ui::changelog::Item> = newest
                .map(|r| {
                    r.items
                        .iter()
                        .filter(|i| {
                            q.is_empty()
                                || i.title.to_ascii_lowercase().contains(&q)
                                || i.body.to_ascii_lowercase().contains(&q)
                        })
                        .take(if q.is_empty() { 6 } else { 20 })
                        .collect()
                })
                .unwrap_or_default();
            if let (Some(r), false) = (newest, news.is_empty()) {
                crate::theme::section(ui, "What's new", |ui| {
                    ui.weak(&r.name);
                    for i in &news {
                        let title = crate::ui::changelog::plain(&i.title);
                        if ui
                            .link(format!("\u{2022} {title}"))
                            .on_hover_text("Open in the changelog")
                            .clicked()
                        {
                            self.tab = Tab::Changelog;
                        }
                    }
                    if ui.button("Open the full changelog").clicked() {
                        self.tab = Tab::Changelog;
                    }
                });
            }

            if !q.is_empty() && keys.is_empty() && terms_empty(&q) && news.is_empty() {
                ui.weak("Nothing matches that.");
            }
        });
        self.open = open;
        tour
    }
}

/// Did the glossary match nothing? Separate so the "nothing matches" check reads as one line.
fn terms_empty(q: &str) -> bool {
    !crate::ui::glossary::ENTRIES
        .iter()
        .any(|e| e.term.to_ascii_lowercase().contains(q) || e.body.to_ascii_lowercase().contains(q))
}

/// Live bindings as `(key, what it does)`, labelled from the registry so a renamed action renames
/// its shortcut row too.
fn shortcut_rows(bindings: &[Binding], entries: &[PaletteEntry], q: &str) -> Vec<(String, String)> {
    bindings
        .iter()
        .filter_map(|b| {
            let label = match b.action {
                BindableAction::Palette(p) => entries.iter().find(|e| e.action == p)?.label.clone(),
                other => hotkeys::label(other)?.to_string(),
            };
            let key = hotkeys::pretty(&b.shortcut);
            (q.is_empty()
                || label.to_ascii_lowercase().contains(q)
                || key.to_ascii_lowercase().contains(q))
            .then_some((key, label))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_changelog_has_a_release_to_show() {
        assert!(!releases().is_empty(), "no release section");
        assert!(!releases()[0].items.is_empty());
    }

    #[test]
    fn shortcuts_filter_by_label_and_by_key() {
        let bindings = crate::hotkeys::defaults();
        let all = shortcut_rows(&bindings, &[], "");
        assert!(
            !all.is_empty(),
            "app-level bindings always label themselves"
        );
        // No registry passed, so every palette-bound row drops out and only `hotkeys::label` rows
        // remain — which is exactly what a caller with no entries should see.
        assert!(all.iter().any(|(_, l)| l == "Fullscreen"));
        // One row per key: fullscreen has F11 and a plain key for keyboards with no F row.
        let fs = shortcut_rows(&bindings, &[], "fullscreen");
        assert!(!fs.is_empty() && fs.iter().all(|(_, l)| l == "Fullscreen"));
        assert!(shortcut_rows(&bindings, &[], "zzzz").is_empty());
    }
}
