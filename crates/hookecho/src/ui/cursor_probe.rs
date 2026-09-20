//! ROADMAP_NEW J3: the compact "Pane | Source | Product | Time | Value" table shown while
//! `link_cursor` is on — one row per pane, each independently sampled at the same shared
//! geographic point (`HookEchoApp::linked_probe`) so panes at different zooms/products still read
//! as answering the same question.
//!
//! Presentation only, following `gate_inspector.rs`'s split: the math (per-pane sampling) stays in
//! `app.rs`'s `probe_row`. Radar reuses the same `inspect_gate` a click already uses; gridded
//! layers sample the retained decoded field represented by the top visible texture.

/// One pane's reading at the shared probe point, or the reasons it has none — a pane with no
/// resident field/radar volume or a point outside coverage reads as `value: None` here rather
/// than being dropped from the table, so a probed pane is never silently missing.
#[derive(Debug, Clone)]
pub struct ProbeRow {
    pub pane: usize,
    pub source: String,
    pub product: String,
    pub time: Option<chrono::DateTime<chrono::Utc>>,
    /// Already formatted in the displayed layer's own units/category vocabulary.
    pub value: Option<String>,
    pub folded: bool,
}

fn value_text(row: &ProbeRow) -> String {
    if row.folded {
        return "Range folded".into();
    }
    row.value.clone().unwrap_or_else(|| "—".into())
}

pub fn show(ctx: &egui::Context, rows: &[ProbeRow], tz: Option<wxdata::tz::Tz>) {
    if rows.is_empty() {
        return;
    }
    egui::Window::new("Cursor probe")
        .id(egui::Id::new("cursor_probe"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -84.0))
        .resizable(false)
        .collapsible(false)
        .title_bar(false)
        .frame(
            egui::Frame::window(&ctx.style_of(ctx.theme()))
                .fill(egui::Color32::from_black_alpha(225))
                .corner_radius(10)
                .inner_margin(10),
        )
        .show(ctx, |ui| table(ui, rows, tz));
}

fn table(ui: &mut egui::Ui, rows: &[ProbeRow], tz: Option<wxdata::tz::Tz>) {
    egui::Grid::new("cursor_probe_grid")
        .num_columns(5)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            for h in ["Pane", "Source", "Product", "Time", "Value"] {
                ui.label(egui::RichText::new(h).size(11.0).weak());
            }
            ui.end_row();
            for row in rows {
                ui.label(format!("{}", row.pane + 1));
                ui.label(&row.source);
                ui.label(&row.product);
                ui.label(
                    row.time
                        .map(|t| crate::timefmt::fmt_clock(t, tz, false))
                        .unwrap_or_else(|| "—".into()),
                );
                ui.label(egui::RichText::new(value_text(row)).strong());
                ui.end_row();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pane: usize, value: Option<&str>, folded: bool) -> ProbeRow {
        ProbeRow {
            pane,
            source: "KTLX".into(),
            product: "REF".into(),
            time: chrono::DateTime::from_timestamp(1_000, 0),
            value: value.map(str::to_string),
            folded,
        }
    }

    #[test]
    fn a_sampled_value_carries_its_units() {
        assert_eq!(value_text(&row(0, Some("42.5 dBZ"), false)), "42.5 dBZ");
    }

    #[test]
    fn a_folded_gate_reads_folded_not_a_number() {
        assert_eq!(value_text(&row(0, None, true)), "Range folded");
    }

    #[test]
    fn a_pane_with_nothing_sampled_reads_as_a_dash() {
        assert_eq!(value_text(&row(0, None, false)), "—");
    }
}
