//! ROADMAP_NEW N1's "Data Source Health panel": every active source's fetch health in one place,
//! rather than hunting row by row through the Layers panel's own per-entry hover popups.
//!
//! Adds no new health tracking of its own — `HookEchoApp::palette_entries` already computes a
//! `SourceHealth` for every entry that has one (see `app::registry`'s `palette_health`); this
//! window just lists the ones that are currently active, using the exact `active_layer` filter
//! the Layers panel itself uses to decide which rows earn a warning icon, so the two can never
//! disagree about what counts as "active".
//!
//! ROADMAP_NEW N1's "rolling success/failure count" is the one item that DOES have tracking now:
//! `SourceHealth.recent_outcomes` — successes out of the last `RequestBook::OUTCOME_WINDOW`
//! finished requests, `None` for a source (radar) that doesn't track a request-outcome history at
//! all. Endpoint family is explicit typed metadata on each `SourceHealth`, so rows sharing one
//! upstream failure domain remain recognizable even when their layer-specific names differ.
//! `latest_valid_time` is equally explicit: the newest authoritative data time retained by the
//! request book, never the local request-completion clock; untimed feeds remain visibly unknown.
//! Radar's configured provider alternatives are structured metadata too; sources with no runtime
//! fallback leave the list empty. Cache residency remains genuinely open rather than inferred.

use crate::app::{HealthState, PaletteEntry, SourceHealth};
use crate::ui::layers_panel::{active_layer, age_line, compact_age, health_look, valid_time_line};
use egui::{Color32, RichText};

/// Worse first. `HealthState`'s own declaration order isn't a severity ranking (`Fetching`
/// happens to sort before `Fresh` there), so this is its own explicit scale rather than reusing
/// the enum's discriminant.
fn severity_rank(state: HealthState) -> u8 {
    match state {
        HealthState::Failed => 0,
        HealthState::Stale => 1,
        HealthState::Delayed => 2,
        HealthState::Waiting => 3,
        HealthState::Fetching => 4,
        HealthState::Fresh => 5,
    }
}

/// Every active, health-tracked source from this frame's registry, worst-first — the pure half of
/// [`show`], kept separate so the ordering is testable without an `egui::Context`. Also the source
/// ROADMAP_NEW N4's diagnostics bundle reads, so the two features can never disagree about what
/// counts as an active source.
pub(crate) fn active_health_rows(entries: &[PaletteEntry]) -> Vec<&SourceHealth> {
    let mut rows: Vec<&SourceHealth> = entries
        .iter()
        .filter(|e| active_layer(e))
        .filter_map(|e| e.health.as_ref())
        .collect();
    rows.sort_by_key(|h| severity_rank(h.state()));
    rows
}

/// Show the window. `entries` is the current frame's full action registry
/// (`HookEchoApp::palette_entries`); only active, health-tracked rows are listed.
pub(crate) fn show(
    ctx: &egui::Context,
    entries: &[PaletteEntry],
    open: &mut bool,
    drawer: &mut crate::ui::drawer::Drawer,
) {
    let mut keep = *open;
    let Some(window) = drawer.page_sized(
        ctx,
        "Data source health",
        &mut keep,
        false,
        700.0,
        egui::Window::new("Data source health"),
    ) else {
        *open = keep;
        return;
    };
    window.show(ctx, |ui| {
        let rows = active_health_rows(entries);
        if rows.is_empty() {
            ui.weak("No active source is being tracked right now — enable a layer to see it here.");
            *open = keep;
            return;
        }
        ui.weak("Every active layer's fetch health, worst first. Hover a row in the Layers panel for the same detail one source at a time.");
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("source_health_grid")
                .num_columns(6)
                .spacing([12.0, 6.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.weak("Status");
                    ui.weak("Source / family");
                    ui.weak("Latest valid data");
                    ui.weak("Fetch health");
                    ui.weak("Recent")
                        .on_hover_text("Successes out of the last 20 finished requests");
                    ui.weak("Last error");
                    ui.end_row();
                    for h in rows {
                        let state = h.state();
                        let (label, color) = health_look(state);
                        ui.colored_label(color, label);
                        ui.vertical(|ui| {
                            ui.label(&h.source);
                            ui.weak(h.endpoint_family.label());
                            if !h.fallback_providers.is_empty() {
                                ui.weak(format!(
                                    "Alternates: {}",
                                    h.fallback_providers.join(" → ")
                                ));
                            }
                        });
                        ui.label(valid_time_line(h.latest_valid_time)).on_hover_text(
                            "Newest authoritative valid/observation time seen for this source; \
                             not the local fetch-completion time",
                        );
                        ui.vertical(|ui| {
                            ui.label(age_line(h.last_success));
                            ui.weak(format!("{} cadence", compact_age(h.cadence)));
                        });
                        match h.recent_outcomes {
                            Some((successes, failures)) => {
                                ui.label(format!("{successes}/{}", successes + failures));
                            }
                            None => {
                                ui.weak("—");
                            }
                        }
                        match &h.error {
                            Some(e) => {
                                ui.label(RichText::new(e).color(Color32::from_rgb(230, 120, 120)))
                                    .on_hover_text(e);
                            }
                            None => {
                                ui.weak("—");
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    });
    *open = keep;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::PaletteAction;
    use crate::render::FieldLayer;

    fn entry(source: &str, state: HealthState, active: bool) -> PaletteEntry {
        let (last_success, last_failure, fetching) = match state {
            HealthState::Fresh => (Some(std::time::Duration::from_secs(1)), None, false),
            HealthState::Fetching => (None, None, true),
            // Between the 120 s cadence this fixture's `SourceHealth` uses below and its
            // `DELAYED_CADENCE_MULTIPLIER` (2x) — past due, not yet stale.
            HealthState::Delayed => (Some(std::time::Duration::from_secs(180)), None, false),
            HealthState::Stale => (Some(std::time::Duration::from_secs(9_999)), None, false),
            HealthState::Failed => (None, Some(std::time::Duration::ZERO), false),
            HealthState::Waiting => (None, None, false),
        };
        PaletteEntry {
            label: source.to_string(),
            category: "National",
            action: PaletteAction::ToggleField(FieldLayer::Mrms),
            on: Some(active),
            desc: "",
            common: false,
            key: None,
            health: Some(SourceHealth {
                source: source.to_string(),
                endpoint_family: crate::source_health::EndpointFamily::NoaaMrms,
                latest_valid_time: None,
                fallback_providers: Vec::new(),
                fetching,
                last_attempt: None,
                last_success,
                last_failure,
                error: (state == HealthState::Failed).then(|| "boom".to_string()),
                cadence: std::time::Duration::from_secs(120),
                recent_outcomes: None,
                details: Vec::new(),
            }),
        }
    }

    #[test]
    fn severity_rank_puts_failed_first_and_fresh_last() {
        let all = [
            HealthState::Fetching,
            HealthState::Fresh,
            HealthState::Delayed,
            HealthState::Stale,
            HealthState::Failed,
            HealthState::Waiting,
        ];
        let mut ranks: Vec<u8> = all.iter().map(|&s| severity_rank(s)).collect();
        ranks.sort_unstable();
        ranks.dedup();
        assert_eq!(ranks.len(), all.len(), "every state gets its own rank");
        assert_eq!(
            severity_rank(HealthState::Failed),
            0,
            "Failed must sort first"
        );
        assert!(severity_rank(HealthState::Fresh) > severity_rank(HealthState::Stale));
        assert!(severity_rank(HealthState::Fresh) > severity_rank(HealthState::Waiting));
        // Delayed sits strictly between "on schedule" and "genuinely stopped updating".
        assert!(severity_rank(HealthState::Fresh) > severity_rank(HealthState::Delayed));
        assert!(severity_rank(HealthState::Delayed) > severity_rank(HealthState::Stale));
    }

    #[test]
    fn inactive_and_healthless_entries_are_excluded() {
        let entries = vec![
            entry("Fresh source", HealthState::Fresh, true),
            entry("Inactive source", HealthState::Failed, false),
            PaletteEntry {
                health: None,
                ..entry("No health tracked", HealthState::Fresh, true)
            },
        ];
        let rows = active_health_rows(&entries);
        assert_eq!(
            rows.len(),
            1,
            "{:?}",
            rows.iter().map(|h| &h.source).collect::<Vec<_>>()
        );
        assert_eq!(rows[0].source, "Fresh source");
    }

    #[test]
    fn rows_come_back_worst_first() {
        let entries = vec![
            entry("Healthy", HealthState::Fresh, true),
            entry("Broken", HealthState::Failed, true),
            entry("Aging", HealthState::Stale, true),
        ];
        let rows = active_health_rows(&entries);
        let order: Vec<&str> = rows.iter().map(|h| h.source.as_str()).collect();
        assert_eq!(order, ["Broken", "Aging", "Healthy"]);
    }
}
