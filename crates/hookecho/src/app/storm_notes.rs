//! Notes on storms (ROADMAP_PARITY M2.4, 1008.md B3): a line of text attached to one storm with
//! its source association (radar, SCIT cell ID and the table time it was written against), kept
//! in analysis cases. A note follows its storm through SCIT renumbering via the storm history
//! (`StormIdentity::resolve`); a note reopened from a case is historical and is listed, never
//! attached to whatever storm carries its old cell ID today.

use chrono::{DateTime, Utc};

/// One note on one storm.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StormNote {
    pub text: String,
    /// The radar whose storm table it was written against.
    pub site: String,
    /// SCIT's cell ID for the storm when written.
    pub cell: String,
    /// That table's time; `None` when the table carried none.
    pub scan: Option<DateTime<Utc>>,
    /// Where the storm was, `[lon, lat]`.
    pub at: [f64; 2],
    /// When it was written (this computer's clock).
    pub written: DateTime<Utc>,
    /// Reopened from a case: listed with its own time, not attached to a current storm.
    pub historical: bool,
}

impl StormNote {
    /// A note to be written on `cell` of `site`'s table.
    pub(crate) fn draft(site: &str, cell: &wxdata::level3::Cell, now: DateTime<Utc>) -> Self {
        StormNote {
            text: String::new(),
            site: site.to_string(),
            cell: cell.id.clone(),
            scan: cell.time,
            at: [cell.lon, cell.lat],
            written: now,
            historical: false,
        }
    }

    pub(crate) fn to_case(&self) -> crate::case::CaseStormNote {
        crate::case::CaseStormNote {
            text: self.text.clone(),
            site: self.site.clone(),
            cell: self.cell.clone(),
            scan: self.scan,
            at: self.at,
            written: self.written,
        }
    }

    /// A note reopened from a case: as saved, and historical.
    pub(crate) fn from_case(c: &crate::case::CaseStormNote) -> Self {
        StormNote {
            text: c.text.clone(),
            site: c.site.clone(),
            cell: c.cell.clone(),
            scan: c.scan,
            at: c.at,
            written: c.written,
            historical: true,
        }
    }

    /// The note as one line in the notes list: "20:12Z KTLX K3 — text (from a case)".
    pub(crate) fn line(&self) -> String {
        format!(
            "{} {} {} \u{2014} {}{}",
            self.scan.map_or_else(
                || "time unknown".to_string(),
                |t| t.format("%H:%MZ").to_string()
            ),
            self.site,
            self.cell,
            self.text.trim(),
            if self.historical {
                " (from a case)"
            } else {
                ""
            }
        )
    }
}

/// The live notes on the storm shown as `row_cell` in `site`'s current table: a note follows its
/// storm through renumbering (`resolve` maps the note's cell and table time to the cell that
/// storm is now), and a historical note is never attached.
pub(crate) fn notes_for_row<'a>(
    notes: &'a [StormNote],
    site: Option<&str>,
    row_cell: &str,
    resolve: impl Fn(&str, Option<i64>) -> Option<String>,
) -> Vec<&'a StormNote> {
    notes
        .iter()
        .filter(|n| !n.historical && Some(n.site.as_str()) == site)
        .filter(|n| resolve(&n.cell, n.scan.map(|t| t.timestamp())).as_deref() == Some(row_cell))
        .collect()
}

/// One note as the map draws it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MapNote {
    /// `[lon, lat]`.
    pub at: [f64; 2],
    pub text: String,
    /// On its storm as the table has it now; otherwise drawn dimmed where it was written.
    pub current: bool,
}

/// Longest note text the map draws; the full text is in the storm table.
const MAP_NOTE_CHARS: usize = 40;

/// Where each note goes on the map. A live note whose storm is in `site`'s current table
/// (`cells`, reached through `resolve` as in [`notes_for_row`]) sits on the storm where it is now.
/// Every other note, reopened from a case, on another radar, or whose storm has left the table,
/// sits where it was written, with its time, so it never lands on an unrelated storm.
pub(crate) fn map_notes(
    notes: &[StormNote],
    site: Option<&str>,
    cells: &[wxdata::level3::Cell],
    resolve: impl Fn(&str, Option<i64>) -> Option<String>,
) -> Vec<MapNote> {
    let short = |s: &str| {
        let s = s.trim();
        if s.chars().count() > MAP_NOTE_CHARS {
            let cut: String = s.chars().take(MAP_NOTE_CHARS - 1).collect();
            format!("{cut}\u{2026}")
        } else {
            s.to_string()
        }
    };
    notes
        .iter()
        .map(|n| {
            let live = (!n.historical && Some(n.site.as_str()) == site)
                .then(|| resolve(&n.cell, n.scan.map(|t| t.timestamp())))
                .flatten()
                .and_then(|id| cells.iter().find(|c| c.id == id));
            match live {
                Some(c) => MapNote {
                    at: [c.lon, c.lat],
                    text: short(&n.text),
                    current: true,
                },
                None => MapNote {
                    at: n.at,
                    text: format!(
                        "{} {}",
                        n.scan
                            .map_or_else(|| "?".to_string(), |t| t.format("%H:%MZ").to_string()),
                        short(&n.text)
                    ),
                    current: false,
                },
            }
        })
        .collect()
}

impl super::HookEchoApp {
    /// Draw the storm notes on every pane (1008.md B3): a pin and the text, on the storm's
    /// present position when it is in the active radar's table, else dimmed where written.
    pub(crate) fn paint_storm_notes(&self, ui: &egui::Ui, rects: &[egui::Rect]) {
        if self.storm_tracks.notes.is_empty() {
            return;
        }
        let site = self.views[self.active].site.clone();
        let cells = self.active_storm_cells();
        let resolve = |cell: &str, scan: Option<i64>| self.dock.storm_ids.current_id(cell, scan);
        let notes = map_notes(&self.storm_tracks.notes, site.as_deref(), cells, resolve);
        for (idx, rect) in rects.iter().enumerate() {
            let cam = self.views[idx].camera;
            let vp = (rect.width(), rect.height());
            let painter = ui.painter_at(*rect);
            for n in &notes {
                let w = crate::render::mercator::lonlat_to_world(n.at[0], n.at[1]);
                let (x, y) = cam.world_to_screen(w, vp);
                let p = egui::pos2(rect.left() + x, rect.top() + y);
                if rect.expand(40.0).contains(p) {
                    paint_map_note(&painter, p, n);
                }
            }
        }
    }
}

/// One note on the map at `p`: a pin and its text, amber on its storm, grey where it was written.
pub(crate) fn paint_map_note(painter: &egui::Painter, p: egui::Pos2, n: &MapNote) {
    let halo = egui::Color32::from_black_alpha(200);
    let font = egui::FontId::proportional(12.0);
    let col = if n.current {
        egui::Color32::from_rgb(255, 214, 102)
    } else {
        egui::Color32::from_rgb(170, 170, 180)
    };
    painter.circle(p, 4.0, col, egui::Stroke::new(1.5, halo));
    let at = p + egui::vec2(8.0, -16.0);
    painter.text(
        at + egui::vec2(1.0, 1.0),
        egui::Align2::LEFT_TOP,
        &n.text,
        font.clone(),
        halo,
    );
    painter.text(at, egui::Align2::LEFT_TOP, &n.text, font, col);
}

/// A draft's editor: the text, Save (only with text) and Cancel; Escape cancels. Returns `Some(true)`
/// to save, `Some(false)` to cancel, `None` to keep editing.
pub(crate) fn editor(ui: &mut egui::Ui, draft: &mut StormNote) -> Option<bool> {
    let mut out = None;
    ui.label(format!("Note on storm {} ({})", draft.cell, draft.site));
    let edit = ui.add(
        egui::TextEdit::multiline(&mut draft.text)
            .desired_rows(2)
            .desired_width(f32::INFINITY)
            .hint_text("What you see in this storm"),
    );
    if edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        out = Some(false);
    }
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!draft.text.trim().is_empty(), egui::Button::new("Save"))
            .clicked()
        {
            out = Some(true);
        }
        if ui.button("Cancel").clicked() {
            out = Some(false);
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(min: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + min * 60, 0).unwrap()
    }

    fn cell(id: &str, min: i64) -> wxdata::level3::Cell {
        wxdata::level3::Cell {
            id: id.into(),
            lon: -97.5,
            lat: 35.3,
            time: Some(t(min)),
            ..Default::default()
        }
    }

    /// The editor with a draft, and the notes list with a live and a reopened note, for review.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes the storm note captures"]
    fn gpu_storm_note_snapshot() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for the editor");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m2.4");
        std::fs::create_dir_all(&destination).unwrap();
        let mut draft = StormNote::draft("KTLX", &cell("K3", 0), t(1));
        draft.text = "Wall cloud on the rear flank, rotating".into();
        let mut reopened = draft.clone();
        reopened.historical = true;
        reopened.cell = "Q7".into();
        reopened.text = "Debris signature with the couplet".into();
        let lines = [draft.line(), reopened.line()];
        gpu.save(&destination.join("storm-note.png"), 360, 230, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(340.0);
                editor(ui, &mut draft);
                ui.separator();
                egui::CollapsingHeader::new(format!("Notes ({})", lines.len()))
                    .default_open(true)
                    .show(ui, |ui| {
                        for line in &lines {
                            ui.horizontal_wrapped(|ui| {
                                let _ = ui.small_button("\u{d7}");
                                ui.label(line);
                            });
                        }
                    });
            });
        })
        .unwrap();
    }

    /// Notes as the map draws them (`paint_map_note`, the code the app paints with): one on
    /// its storm, one reopened from a case where it was written.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes the map notes capture"]
    fn gpu_map_notes_snapshot() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for the map notes");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m2.4");
        std::fs::create_dir_all(&destination).unwrap();
        let notes = [
            MapNote {
                at: [0.0, 0.0],
                text: "Wall cloud on the rear flank, rotating".into(),
                current: true,
            },
            MapNote {
                at: [0.0, 0.0],
                text: "20:12Z Debris signature with the couplet".into(),
                current: false,
            },
        ];
        gpu.save(&destination.join("map-notes.png"), 420, 160, |ui| {
            let rect = ui.max_rect();
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(24, 28, 34));
            paint_map_note(
                &painter,
                rect.left_top() + egui::vec2(40.0, 60.0),
                &notes[0],
            );
            paint_map_note(
                &painter,
                rect.left_top() + egui::vec2(60.0, 125.0),
                &notes[1],
            );
        })
        .unwrap();
    }

    #[test]
    fn a_note_round_trips_through_a_case_and_reopens_historical() {
        let mut n = StormNote::draft("KTLX", &cell("K3", 0), t(1));
        n.text = "wall cloud, rotating".into();
        let saved = n.to_case();
        let json = serde_json::to_string(&saved).unwrap();
        let back = StormNote::from_case(&serde_json::from_str(&json).unwrap());
        assert!(back.historical);
        assert_eq!(
            StormNote {
                historical: false,
                ..back.clone()
            },
            n
        );
        assert_eq!(
            back.line(),
            "22:13Z KTLX K3 \u{2014} wall cloud, rotating (from a case)"
        );
        // A case written before notes were kept opens with none.
        let old: crate::case::CaseManifest = serde_json::from_value(serde_json::json!({
            "format": 1, "name": "x", "app_version": "0", "created_utc": "2026-01-01T00:00:00Z",
            "time_utc": null, "span_min": 0, "sites": [],
            "workspace": {"name": "x", "panes": []}
        }))
        .unwrap();
        assert!(old.storm_notes.is_empty());
    }

    #[test]
    fn a_note_follows_its_storm_and_a_historical_or_other_radar_note_does_not_attach() {
        let mut n = StormNote::draft("KTLX", &cell("K3", 0), t(1));
        n.text = "hook".into();
        let mut historical = n.clone();
        historical.historical = true;
        let mut elsewhere = n.clone();
        elsewhere.site = "KINX".into();
        let notes = vec![n, historical, elsewhere];
        // The storm the note was written on is now cell Q7 (SCIT renumbered it); K3 is another storm.
        let resolve = |cell: &str, scan: Option<i64>| {
            (cell == "K3" && scan == Some(t(0).timestamp())).then(|| "Q7".to_string())
        };
        let on_q7 = notes_for_row(&notes, Some("KTLX"), "Q7", resolve);
        assert_eq!(on_q7.len(), 1, "only the live note on this radar");
        assert_eq!(on_q7[0].text, "hook");
        assert!(notes_for_row(&notes, Some("KTLX"), "K3", resolve).is_empty());
        assert!(notes_for_row(&notes, Some("KDMX"), "Q7", resolve).is_empty());
    }

    /// On the map, a live note rides on its storm where the current table has it; a note from a
    /// case, from another radar or whose storm is gone stays dimmed where it was written, with
    /// its time; long text is shortened.
    #[test]
    fn a_note_on_the_map_sits_on_its_storm_or_where_it_was_written() {
        let mut n = StormNote::draft("KTLX", &cell("K3", 0), t(1));
        n.text = "hook".into();
        let mut historical = n.clone();
        historical.historical = true;
        let mut long = n.clone();
        long.text = "x".repeat(60);
        long.cell = "Z9".into();
        let notes = vec![n, historical, long];
        let mut moved = cell("Q7", 5);
        moved.lon = -97.2;
        moved.lat = 35.6;
        let resolve = |cell: &str, scan: Option<i64>| {
            (cell == "K3" && scan == Some(t(0).timestamp())).then(|| "Q7".to_string())
        };
        let drawn = map_notes(&notes, Some("KTLX"), std::slice::from_ref(&moved), resolve);
        assert_eq!(drawn[0].at, [-97.2, 35.6], "on the storm where it is now");
        assert!(drawn[0].current && drawn[0].text == "hook");
        assert_eq!(
            drawn[1].at,
            [-97.5, 35.3],
            "a case's note stays where it was written"
        );
        assert!(!drawn[1].current && drawn[1].text.ends_with(" hook"));
        assert!(!drawn[2].current, "its storm is not in the table");
        assert!(drawn[2].text.chars().count() <= 47 && drawn[2].text.ends_with('\u{2026}'));
        // Another radar's table: nothing attaches.
        let other = map_notes(&notes, Some("KINX"), std::slice::from_ref(&moved), resolve);
        assert!(other.iter().all(|m| !m.current));
    }
}
