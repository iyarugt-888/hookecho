//! "What's new": after an update, a phone or tablet opens on a sheet of the release's big
//! changes ([`crate::ui::changelog::Release::highlights`]) — large cards, one per change, in the
//! words of `CHANGELOG.md`'s `### Highlights` list — with "See everything" for the full changelog
//! and "Got it" to close. It comes up once per set of highlights ([`key`], remembered in
//! `Settings::whats_new_seen`), not on a new install (nothing changed for them), and not over
//! the first-run card or the tour.
//!
//! Touch devices only: on a desktop the same highlights head the Help hub's "What's new"
//! section, where a pop-up at launch would be in the way of work.

use crate::ui::changelog::Release;
use egui::{Color32, RichText};

/// What the sheet was closed with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Choice {
    /// "Got it".
    Done,
    /// "See everything": open the changelog.
    SeeAll,
}

/// Which set of highlights a sheet showed: the release and its highlights' titles, so a new
/// highlight brings the sheet back and a reworded body does not.
pub(crate) fn key(release: &Release) -> String {
    let mut k = release.name.clone();
    for h in &release.highlights {
        k.push('|');
        k.push_str(&h.title);
    }
    // ponytail: FNV-1a, so the setting stays a short token rather than every title.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in k.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{}-{hash:016x}", release.name)
}

/// A touch device: an Android phone or tablet (or the debug phone emulation), or a browser with
/// a touch screen.
pub(crate) fn touch_device() -> bool {
    if crate::platform::phone_layout() || crate::platform::form_factor::touch_tablet() {
        return true;
    }
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window().is_some_and(|w| w.navigator().max_touch_points() > 0)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

/// Whether to open the sheet at launch: on a touch device, after setup, with highlights to show
/// that have not been seen.
pub(crate) fn wanted(release: Option<&Release>, seen: &str, setup_done: bool, touch: bool) -> bool {
    touch && setup_done && release.is_some_and(|r| !r.highlights.is_empty() && key(r) != seen)
}

/// Draw the sheet over everything. `phone` sizes it to the screen; a tablet gets a centred
/// card. Returns how it was closed, if it was.
pub(crate) fn show(
    ctx: &egui::Context,
    release: &Release,
    phone: bool,
    accent: Color32,
) -> Option<Choice> {
    let screen = ctx.content_rect();
    // Dim what is behind, and take its clicks: the sheet is the one thing to answer.
    egui::Area::new(egui::Id::new("whats_new_backdrop"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(170));
        });
    // The content's width: the frame's margins (and its stroke) go outside it, and a phone keeps
    // an 8-point gutter each side.
    let margin = if phone { 18.0 } else { 24.0 };
    let width = if phone {
        (screen.width() - 16.0 - 2.0 * margin - 2.0).max(120.0)
    } else {
        560.0f32.min(screen.width() - 48.0 - 2.0 * margin)
    };
    let max_height = screen.height() - if phone { 32.0 } else { 96.0 };
    let mut choice = None;
    egui::Area::new(egui::Id::new("whats_new_sheet"))
        .order(egui::Order::Tooltip)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            egui::Frame::window(ui.style())
                .corner_radius(16)
                .inner_margin(egui::Margin::same(margin as i8))
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.set_max_height(max_height);
                    ui.label(RichText::new("What's new").size(28.0).strong());
                    ui.label(
                        RichText::new(format!(
                            "The big changes in HookEcho {}",
                            if release.name.eq_ignore_ascii_case("unreleased") {
                                "since the last release".to_string()
                            } else {
                                release.name.clone()
                            }
                        ))
                        .size(15.0)
                        .weak(),
                    );
                    ui.add_space(12.0);
                    // Room for the buttons below whatever the cards take.
                    let cards_height = (max_height - 170.0).max(120.0);
                    egui::ScrollArea::vertical()
                        .max_height(cards_height)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for h in &release.highlights {
                                card(ui, &h.title, &h.text, accent);
                                ui.add_space(10.0);
                            }
                        });
                    ui.add_space(8.0);
                    // Finger-sized buttons, the answer on the right.
                    let button_h = 48.0;
                    ui.horizontal(|ui| {
                        let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                        if ui
                            .add_sized(
                                [half, button_h],
                                egui::Button::new(RichText::new("See everything").size(17.0)),
                            )
                            .clicked()
                        {
                            choice = Some(Choice::SeeAll);
                        }
                        if ui
                            .add_sized(
                                [half, button_h],
                                egui::Button::new(
                                    RichText::new("Got it")
                                        .size(17.0)
                                        .strong()
                                        .color(Color32::BLACK),
                                )
                                .fill(accent),
                            )
                            .clicked()
                        {
                            choice = Some(Choice::Done);
                        }
                    });
                });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        choice = Some(Choice::Done);
    }
    choice
}

/// One big change: its title large, what it means below, an accent edge down the left.
fn card(ui: &mut egui::Ui, title: &str, text: &str, accent: Color32) {
    let frame = egui::Frame::NONE
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(12)
        .inner_margin(egui::Margin {
            left: 18,
            right: 14,
            top: 14,
            bottom: 14,
        });
    let resp = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add(
            egui::Label::new(
                RichText::new(crate::ui::changelog::plain(title))
                    .size(20.0)
                    .strong(),
            )
            .wrap(),
        );
        if !text.is_empty() {
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(RichText::new(crate::ui::changelog::plain(text)).size(15.5))
                    .wrap(),
            );
        }
    });
    let r = resp.response.rect;
    ui.painter().rect_filled(
        egui::Rect::from_min_max(r.min, egui::pos2(r.min.x + 5.0, r.max.y)),
        egui::CornerRadius {
            nw: 12,
            sw: 12,
            ne: 0,
            se: 0,
        },
        accent,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::changelog::{parse, Highlight};

    fn release(titles: &[&str]) -> Release {
        Release {
            name: "Unreleased".into(),
            date: None,
            intro: String::new(),
            items: Vec::new(),
            highlights: titles
                .iter()
                .map(|t| Highlight {
                    title: t.to_string(),
                    text: "x".into(),
                })
                .collect(),
        }
    }

    /// The sheet opens once per set of highlights, on a touch device after setup, and not for a
    /// release without highlights.
    #[test]
    fn the_sheet_opens_once_per_set_of_highlights() {
        let r = release(&["Follow a storm", "No signal"]);
        assert!(wanted(Some(&r), "", true, true));
        assert!(!wanted(Some(&r), &key(&r), true, true), "seen");
        assert!(
            !wanted(Some(&r), "", false, true),
            "a new install, still in setup"
        );
        assert!(!wanted(Some(&r), "", true, false), "a desktop");
        assert!(
            !wanted(Some(&release(&[])), "", true, true),
            "nothing to show"
        );
        assert!(!wanted(None, "", true, true));
        // A new highlight brings it back; rewording a body does not.
        let more = release(&["Follow a storm", "No signal", "Ensembles"]);
        assert_ne!(key(&more), key(&r));
        let mut reworded = r.clone();
        reworded.highlights[0].text = "other words".into();
        assert_eq!(key(&reworded), key(&r));
    }

    /// The shipped changelog's newest release has highlights for the sheet to show, each with a
    /// title and text short enough for a card.
    #[test]
    fn the_changelog_opens_with_highlights() {
        let releases = parse(include_str!("../../../../CHANGELOG.md"));
        let newest = releases.first().expect("a release");
        assert!(
            (4..=12).contains(&newest.highlights.len()),
            "{} highlights",
            newest.highlights.len()
        );
        for h in &newest.highlights {
            assert!(
                !h.title.is_empty() && h.title.chars().count() <= 48,
                "{h:?}"
            );
            assert!(!h.text.is_empty() && h.text.chars().count() <= 220, "{h:?}");
        }
    }
}
