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
}
