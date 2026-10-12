# What's new on phones and tablets (2026-10-12)

The changelog used to reach a phone or tablet only through the Help page: its "What's new"
section listed the newest release's first six entry titles, and the Changelog tab browsed the
whole file. The Unreleased section alone holds 488 entries, written as release notes, so nothing
said which of them mattered.

**The changelog.** A release now opens with `### Highlights`: one bullet per big change, a bold
title and a sentence or two in plain words (`- **Title** — what it means`). The parser reads them
apart from the entries (`ui::changelog::Release::highlights`; `highlights_are_read_apart_from_the_entries`),
so the entry count and the filters are unchanged, and the release job prints them first, so a
release page opens with them too. Unreleased has ten, chosen from its 488 entries: following a
storm through the loop, several radars at once, the weather along a route, working with no
signal, every model field, ensembles, satellite at its own pace, reading at a glance, tablets,
and cleaner velocity. A test holds the shipped changelog to 4 to 12 highlights, each short
enough for a card (`the_changelog_opens_with_highlights`).

**The sheet.** After an update, a touch device opens on "What's new" (`ui/whats_new.rs`): the
highlights as large cards (20-point titles, 15.5-point text, an accent edge), the map dimmed
behind, and two 48-point buttons, "See everything" (the Help page on the full changelog) and
"Got it". A phone gets a sheet the width of the screen with an 8-point gutter; a tablet a card
up to 560 points wide. It comes up once per set of highlights (`whats_new::key`, the release
and its highlights' titles, remembered in `Settings::whats_new_seen`): a new highlight brings it
back, a reworded one does not. A new install, still in setup, takes the current set as seen,
since nothing changed for them, and the sheet never opens over the first-run card or the tour
(`the_sheet_opens_once_per_set_of_highlights`). Touch devices are Android phones and tablets
(and the debug phone emulation) and browsers whose `navigator.maxTouchPoints` is above zero. On
a desktop the same highlights head the Help page's "What's new" section instead.

The Changelog tab's release row now wraps, so on a phone its Newer/Older buttons go to a second
line rather than off the edge.

| Capture | What it shows |
| --- | --- |
| `whats-new-tablet-browser.jpg` | Browser build on WebGPU, one machine, `?phone` in an 800 by 600 pane (a tablet): the sheet over the dimmed map, the first four highlights, "See everything" and "Got it". |
| `whats-new-phone-desktop-emulation.png` | The Windows debug build's phone emulation (`HOOKECHO_PHONE=1`, 411 by 780): the same sheet the width of the screen. |
| `see-everything-changelog-browser.jpg` | After "See everything": the Help page on the Changelog tab, "Unreleased · 488 changes", newest first. Reloading afterwards did not bring the sheet back. |

| File | SHA-256 |
| --- | --- |
| whats-new-tablet-browser.jpg | `1e9a9a9bdb693230aaee013a29a163575e2bab123ec5ed4ac0b8e1e5b9bd9ecc` |
| whats-new-phone-desktop-emulation.png | `50de32e54d4f4eff7cfed019e40999cc980905af4b03c980b6f49d1a3bd0d346` |
| see-everything-changelog-browser.jpg | `4eb56a15f233ce850f429f685b9c67da6fcb5583559f38c0b8a771d07b6fa356` |

The browser pane's own phone emulation (375 by 812 at device-pixel ratio 2) drew a canvas at
half resolution and showed only its corner, so the phone capture is the desktop debug build's
phone mode instead. Not established: an Android phone or tablet, an iPad or a phone browser, and
the sheet after a real update from one release to the next.
