//! The changelog as something to browse rather than a wall of text: releases to pick from, the
//! kinds of change to filter by, and each entry a row that opens to its full text.
//!
//! `CHANGELOG.md` is written for the release job (each `## ` section is a release body), in two
//! styles: newer entries are `### Added: title` (or Fixed, Changed, Improved, …) with a body;
//! older releases group bullets under a themed `### Heading`, or are bare bullets. [`parse`]
//! reads both into [`Release`]s of [`Item`]s, so neither style needs rewriting.

use egui::text::LayoutJob;
use egui::{Color32, FontId, RichText, TextFormat};

/// What an entry did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    Added,
    Improved,
    Changed,
    Fixed,
    Removed,
    /// A themed group or a bare bullet: no kind was written.
    Other,
}

impl Kind {
    const FILTERS: [Kind; 6] = [
        Kind::Added,
        Kind::Improved,
        Kind::Changed,
        Kind::Fixed,
        Kind::Removed,
        Kind::Other,
    ];

    /// The kind a heading names before its colon ("Added: globe"), if it names one.
    fn from_word(w: &str) -> Option<Kind> {
        Some(match w.trim().to_ascii_lowercase().as_str() {
            "added" | "new" => Kind::Added,
            "improved" | "faster" => Kind::Improved,
            "changed" => Kind::Changed,
            "fixed" | "fixes" => Kind::Fixed,
            "removed" => Kind::Removed,
            _ => return None,
        })
    }

    fn label(self) -> &'static str {
        match self {
            Kind::Added => "Added",
            Kind::Improved => "Improved",
            Kind::Changed => "Changed",
            Kind::Fixed => "Fixed",
            Kind::Removed => "Removed",
            Kind::Other => "Notes",
        }
    }

    fn color(self) -> Color32 {
        match self {
            Kind::Added => Color32::from_rgb(70, 190, 120),
            Kind::Improved => Color32::from_rgb(90, 160, 240),
            Kind::Changed => Color32::from_rgb(235, 175, 70),
            Kind::Fixed => Color32::from_rgb(200, 120, 230),
            Kind::Removed => Color32::from_rgb(225, 95, 95),
            Kind::Other => Color32::from_gray(150),
        }
    }
}

/// One change.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Item {
    pub kind: Kind,
    pub title: String,
    /// Markdown: paragraphs and `- ` bullets.
    pub body: String,
}

/// One `## ` section.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Release {
    /// "0.12.0-beta.2", or "Unreleased".
    pub name: String,
    pub date: Option<String>,
    /// The text before its first entry.
    pub intro: String,
    pub items: Vec<Item>,
}

/// Read every release out of the changelog, newest first as written.
pub(crate) fn parse(md: &str) -> Vec<Release> {
    let mut out: Vec<Release> = Vec::new();
    // Bullets under a themed heading belong to it; bare bullets are entries of their own.
    let mut in_group = false;
    for line in md.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            let (name, date) = match h.split_once(" - ") {
                Some((n, d)) => (n.trim().to_string(), Some(d.trim().to_string())),
                None => (h.trim().to_string(), None),
            };
            out.push(Release {
                name,
                date,
                intro: String::new(),
                items: Vec::new(),
            });
            in_group = false;
            continue;
        }
        let Some(rel) = out.last_mut() else {
            continue; // the file's own preamble
        };
        if let Some(h) = line.strip_prefix("### ") {
            let (kind, title) = match h.split_once(':') {
                Some((w, rest)) if Kind::from_word(w).is_some() => (
                    Kind::from_word(w).expect("checked"),
                    capitalized(rest.trim()),
                ),
                _ => (Kind::Other, h.trim().to_string()),
            };
            rel.items.push(Item {
                kind,
                title,
                body: String::new(),
            });
            in_group = true;
            continue;
        }
        if !in_group {
            if let Some(b) = line.strip_prefix("- ") {
                // A bare bullet: its first sentence is the title, all of it the body.
                rel.items.push(Item {
                    kind: guess_kind(b),
                    title: first_sentence(b),
                    body: b.to_string(),
                });
                continue;
            }
            match rel.items.last_mut() {
                // A bullet's continuation lines.
                Some(item) if line.starts_with("  ") && !item.body.is_empty() => {
                    item.body.push(' ');
                    item.body.push_str(line.trim());
                }
                _ => {
                    rel.intro.push_str(line);
                    rel.intro.push('\n');
                }
            }
            continue;
        }
        if let Some(item) = rel.items.last_mut() {
            item.body.push_str(line);
            item.body.push('\n');
        }
    }
    for r in &mut out {
        r.intro = r.intro.trim().to_string();
        for i in &mut r.items {
            i.body = i.body.trim().to_string();
        }
    }
    out
}

/// "globe" reads as a title once it is "Globe".
fn capitalized(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// A bare bullet's kind, read from how it opens ("Fixed …", "A crash when …" stays a note).
fn guess_kind(text: &str) -> Kind {
    let first = text
        .split(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or_default();
    Kind::from_word(first).unwrap_or(Kind::Other)
}

/// The first sentence of `text`, markup removed, at most about 90 characters.
fn first_sentence(text: &str) -> String {
    let plain = plain(text);
    let end = plain
        .char_indices()
        .find(|(i, c)| {
            matches!(c, '.' | ':' | ';')
                && plain[i + c.len_utf8()..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
        })
        .map_or(plain.len(), |(i, _)| i);
    let s = plain[..end].trim();
    if s.chars().count() > 90 {
        let cut: String = s.chars().take(88).collect();
        format!("{}\u{2026}", cut.trim_end())
    } else {
        s.to_string()
    }
}

/// `text` without its inline markup: `**bold**`, `` `code` `` and `[links](url)` read as words.
pub(crate) fn plain(text: &str) -> String {
    spans(text).into_iter().map(|(s, _)| s).collect()
}

/// Inline style of a run of text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    Plain,
    Bold,
    Code,
    Link,
}

/// `text` split into styled runs.
fn spans(text: &str) -> Vec<(String, Style)> {
    let mut out: Vec<(String, Style)> = Vec::new();
    let mut rest = text;
    let push = |s: &str, st: Style, out: &mut Vec<(String, Style)>| {
        if !s.is_empty() {
            out.push((s.to_string(), st));
        }
    };
    while !rest.is_empty() {
        let next = [
            rest.find("**").map(|i| (i, 0)),
            rest.find('`').map(|i| (i, 1)),
            rest.find('[').map(|i| (i, 2)),
        ]
        .into_iter()
        .flatten()
        .min_by_key(|(i, _)| *i);
        let Some((i, which)) = next else {
            push(rest, Style::Plain, &mut out);
            break;
        };
        push(&rest[..i], Style::Plain, &mut out);
        let after = &rest[i..];
        let consumed = match which {
            0 => after[2..].find("**").map(|j| {
                push(&after[2..2 + j], Style::Bold, &mut out);
                j + 4
            }),
            1 => after[1..].find('`').map(|j| {
                push(&after[1..1 + j], Style::Code, &mut out);
                j + 2
            }),
            _ => after.find("](").and_then(|j| {
                let close = after[j + 2..].find(')')?;
                push(&after[1..j], Style::Link, &mut out);
                Some(j + 2 + close + 1)
            }),
        };
        match consumed {
            Some(n) => rest = &after[n..],
            // An unmatched marker is just a character.
            None => {
                let n = if which == 0 { 2 } else { 1 };
                push(&after[..n], Style::Plain, &mut out);
                rest = &after[n..];
            }
        }
    }
    out
}

/// A paragraph or bullet laid out with its inline styles.
fn paragraph(ui: &mut egui::Ui, text: &str) {
    let size = 12.5;
    let color = ui.visuals().text_color();
    let strong = ui.visuals().strong_text_color();
    let mut job = LayoutJob::default();
    job.wrap.max_width = ui.available_width();
    for (s, st) in spans(text) {
        let fmt = match st {
            Style::Plain => TextFormat::simple(FontId::proportional(size), color),
            Style::Bold => TextFormat::simple(FontId::proportional(size), strong),
            Style::Code => TextFormat {
                font_id: FontId::monospace(size - 1.0),
                color,
                background: Color32::from_white_alpha(16),
                ..Default::default()
            },
            Style::Link => TextFormat {
                font_id: FontId::proportional(size),
                color: ui.visuals().hyperlink_color,
                ..Default::default()
            },
        };
        job.append(&s, 0.0, fmt);
    }
    ui.label(job);
}

/// An entry's body: paragraphs, and bullets (with their continuation lines) indented.
fn body(ui: &mut egui::Ui, md: &str) {
    let mut blocks: Vec<(bool, String)> = Vec::new();
    for line in md.lines() {
        let t = line.trim();
        if t.is_empty() {
            blocks.push((false, String::new()));
        } else if let Some(b) = t.strip_prefix("- ") {
            blocks.push((true, b.to_string()));
        } else {
            match blocks.last_mut() {
                Some((_, s)) if !s.is_empty() => {
                    s.push(' ');
                    s.push_str(t);
                }
                _ => blocks.push((false, t.to_string())),
            }
        }
    }
    for (bullet, text) in blocks.into_iter().filter(|(_, t)| !t.is_empty()) {
        if bullet {
            ui.horizontal_top(|ui| {
                ui.add_space(6.0);
                ui.label(RichText::new("\u{2022}").weak());
                ui.vertical(|ui| paragraph(ui, &text));
            });
        } else {
            paragraph(ui, &text);
        }
        ui.add_space(3.0);
    }
}

/// What the view remembers between frames.
#[derive(Default)]
pub(crate) struct View {
    release: usize,
    kind: Option<Kind>,
    /// Entries opened by hand, as `(release, item)`.
    open: std::collections::HashSet<(usize, usize)>,
}

impl View {
    /// Draw the changelog. `query` (lowercase, from the Help search box) narrows every release
    /// at once: while it is set, the release picker gives way to the matches across all of them.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, releases: &[Release], query: &str) {
        if releases.is_empty() {
            ui.weak("No changelog in this build.");
            return;
        }
        self.release = self.release.min(releases.len() - 1);
        let searching = !query.is_empty();
        let matches = |i: &Item| {
            !searching
                || i.title.to_ascii_lowercase().contains(query)
                || i.body.to_ascii_lowercase().contains(query)
        };

        if !searching {
            // Release picker: newest first, each with how much it changed.
            ui.horizontal(|ui| {
                ui.label(RichText::new("Release").weak());
                let r = &releases[self.release];
                egui::ComboBox::from_id_salt("changelog_release")
                    .width(220.0)
                    .selected_text(release_label(r))
                    .show_ui(ui, |ui| {
                        for (i, r) in releases.iter().enumerate() {
                            ui.selectable_value(&mut self.release, i, release_label(r));
                        }
                    });
                if ui
                    .add_enabled(
                        self.release > 0,
                        egui::Button::new("\u{2190} Newer").small(),
                    )
                    .clicked()
                {
                    self.release -= 1;
                }
                if ui
                    .add_enabled(
                        self.release + 1 < releases.len(),
                        egui::Button::new("Older \u{2192}").small(),
                    )
                    .clicked()
                {
                    self.release += 1;
                }
            });
        }
        let scope: Vec<usize> = if searching {
            (0..releases.len()).collect()
        } else {
            vec![self.release]
        };

        // Kind chips, counted over what is in scope.
        let count = |k: Option<Kind>| {
            scope
                .iter()
                .flat_map(|r| releases[*r].items.iter())
                .filter(|i| matches(i) && k.is_none_or(|k| i.kind == k))
                .count()
        };
        ui.horizontal_wrapped(|ui| {
            let all = count(None);
            if ui
                .selectable_label(self.kind.is_none(), format!("All {all}"))
                .clicked()
            {
                self.kind = None;
            }
            for k in Kind::FILTERS {
                let n = count(Some(k));
                if n == 0 {
                    continue;
                }
                let on = self.kind == Some(k);
                let text = RichText::new(format!("{} {n}", k.label())).color(if on {
                    Color32::WHITE
                } else {
                    k.color()
                });
                if ui.selectable_label(on, text).clicked() {
                    self.kind = if on { None } else { Some(k) };
                }
            }
            ui.separator();
            if ui.small_button("Expand all").clicked() {
                for r in &scope {
                    for (j, i) in releases[*r].items.iter().enumerate() {
                        if matches(i) && self.kind.is_none_or(|k| i.kind == k) {
                            self.open.insert((*r, j));
                        }
                    }
                }
            }
            if ui.small_button("Collapse all").clicked() {
                self.open.clear();
            }
        });
        ui.add_space(4.0);

        let mut shown = 0;
        for &r in &scope {
            let rel = &releases[r];
            let items: Vec<(usize, &Item)> = rel
                .items
                .iter()
                .enumerate()
                .filter(|(_, i)| matches(i) && self.kind.is_none_or(|k| i.kind == k))
                .collect();
            if items.is_empty() && (searching || !rel.intro.is_empty() && self.kind.is_some()) {
                continue;
            }
            if searching {
                ui.add_space(4.0);
                ui.label(RichText::new(release_label(rel)).strong());
            } else if !rel.intro.is_empty() && self.kind.is_none() {
                body(ui, &rel.intro);
                ui.add_space(4.0);
            }
            for (j, item) in items {
                shown += 1;
                self.row(ui, (r, j), item, searching);
            }
        }
        if shown == 0 {
            ui.weak(if searching {
                "No change mentions that."
            } else {
                "Nothing of that kind in this release."
            });
        }
    }

    /// One entry: a badge, its title and a line of preview; open, its full text.
    fn row(&mut self, ui: &mut egui::Ui, key: (usize, usize), item: &Item, searching: bool) {
        // A search opens what it found, so the match is visible without a click.
        let is_open = self.open.contains(&key) || (searching && !item.body.is_empty());
        let frame = egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(6, 4))
            .corner_radius(4)
            .fill(if is_open {
                Color32::from_white_alpha(8)
            } else {
                Color32::TRANSPARENT
            });
        let resp = frame
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let c = item.kind.color();
                    ui.label(
                        RichText::new(item.kind.label())
                            .size(10.5)
                            .strong()
                            .color(Color32::BLACK)
                            .background_color(c),
                    );
                    ui.label(
                        RichText::new(if is_open {
                            egui_phosphor::regular::CARET_DOWN
                        } else {
                            egui_phosphor::regular::CARET_RIGHT
                        })
                        .weak(),
                    );
                    ui.add(egui::Label::new(RichText::new(plain(&item.title)).strong()).wrap());
                });
                if is_open {
                    ui.add_space(2.0);
                    body(ui, &item.body);
                } else if !item.body.is_empty() {
                    let preview = first_sentence(&item.body);
                    if preview != plain(&item.title) {
                        ui.add(
                            egui::Label::new(RichText::new(preview).weak().size(11.5)).truncate(),
                        );
                    }
                }
            })
            .response;
        let click = ui.interact(
            resp.rect,
            ui.id().with(("changelog_row", key)),
            egui::Sense::click(),
        );
        if click.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if click.clicked() && !item.body.is_empty() && !self.open.remove(&key) {
            self.open.insert(key);
        }
    }
}

fn release_label(r: &Release) -> String {
    let n = r.items.len();
    match &r.date {
        Some(d) => format!("{} \u{b7} {d} \u{b7} {n}", r.name),
        None => format!("{} \u{b7} {n} changes", r.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD: &str = "# Changelog\n\npreamble\n\n## Unreleased\n\n### Added: globe\n\nTurn on **Globe** and spin `it`.\n\n### Fixed: a crash\n\nGone.\n\n## 0.9.0 - 2026-08-11\n\nThe intro.\n\n### Faster\n\n- One thing.\n- Two things,\n  wrapped.\n\n## 0.8.0 - 2026-08-09\n\n- Fixed the site list. It works.\n- A new [page](https://x.y) for sites.\n";

    #[test]
    fn both_changelog_styles_read_as_releases_of_entries() {
        let r = parse(MD);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].name, "Unreleased");
        assert_eq!(r[0].date, None);
        assert_eq!(
            r[0].items
                .iter()
                .map(|i| (i.kind, i.title.as_str()))
                .collect::<Vec<_>>(),
            [(Kind::Added, "Globe"), (Kind::Fixed, "A crash")]
        );
        assert_eq!(r[1].date.as_deref(), Some("2026-08-11"));
        assert_eq!(r[1].intro, "The intro.");
        // A themed heading keeps its bullets as its body.
        assert_eq!(r[1].items.len(), 1);
        assert_eq!(r[1].items[0].kind, Kind::Other);
        assert!(r[1].items[0].body.contains("wrapped"));
        // Bare bullets are entries, titled by their first sentence, kind read from the opening.
        assert_eq!(r[2].items.len(), 2);
        assert_eq!(r[2].items[0].kind, Kind::Fixed);
        assert_eq!(r[2].items[0].title, "Fixed the site list");
        assert_eq!(r[2].items[1].title, "A new page for sites");
    }

    #[test]
    fn inline_markup_is_read_not_shown() {
        assert_eq!(
            plain("Turn on **Globe** and spin `it`."),
            "Turn on Globe and spin it."
        );
        assert_eq!(plain("see [the docs](https://x.y)."), "see the docs.");
        assert_eq!(plain("a lone ` and ** stay"), "a lone ` and ** stay");
    }

    #[test]
    fn the_shipped_changelog_parses_into_every_release() {
        let md = include_str!("../../../../CHANGELOG.md");
        let r = parse(md);
        let headings = md.lines().filter(|l| l.starts_with("## ")).count();
        assert_eq!(r.len(), headings);
        assert!(r.iter().all(|r| !r.items.is_empty() || !r.intro.is_empty()));
        assert!(
            r[0].items.iter().filter(|i| i.kind == Kind::Added).count() > 10,
            "the typed headings are read"
        );
    }
}
