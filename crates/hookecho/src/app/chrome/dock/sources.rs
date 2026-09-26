//! The Sources window: every active feed's health in a dock-width list, worst first — the
//! workstation's compact home for ROADMAP_NEW N1's source health, where the full seven-column
//! Data source health window is too wide to dock. Each row is a status glyph (a shape as well as
//! a colour), the source, and how old its newest data is; a failure adds its last error under
//! it. The full table is one click away.
//!
//! It reads the same rows as that window (`source_health_window::active_health_rows`), so the two
//! cannot disagree about which sources are active or how healthy they are.

use super::*;
use crate::ui::a11y::Named as _;
use crate::ui::layers_panel::{age_line, compact_age, health_look, valid_time_line};
use egui::{FontId, Rect, Sense};
use egui_phosphor::regular as ph;

/// The window's width, docked or floating.
pub(super) const SOURCES_W: f32 = 300.0;

/// A state that asks for the analyst's attention (the summary line counts these).
fn needs_attention(state: HealthState) -> bool {
    matches!(
        state,
        HealthState::Failed | HealthState::Stale | HealthState::Cached | HealthState::Delayed
    )
}

/// How old a source's newest data is, as a row's right-hand column: "4m", "now", or a dash when
/// the feed does not report a valid time.
fn data_age(
    latest: Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    match latest {
        None => "\u{2014}".into(),
        Some(t) => {
            let secs = (now - t).num_seconds();
            if secs < 5 {
                "now".into()
            } else {
                compact_age(std::time::Duration::from_secs(secs as u64))
            }
        }
    }
}

impl HookEchoApp {
    pub(super) fn dock_sources(&mut self, host: Host<'_>) {
        if !self.dock.sources.open {
            return;
        }
        let t = self.ws_tokens();
        let entries = self.palette_entries();
        let rows = crate::ui::source_health_window::active_health_rows(&entries);
        let now = chrono::Utc::now();
        let map_rect = self.chrome_rect;
        let place = self.dock.sources.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.sources.collapsed;
        let list_h = (map_rect.height() - 110.0).clamp(140.0, 520.0);
        let attention = rows.iter().filter(|h| needs_attention(h.state())).count();
        let title = if attention == 0 {
            "Sources".to_string()
        } else {
            format!("Sources ({attention})")
        };
        let mut header = ws::HeaderAction::None;
        let mut full_table = false;
        tool_window(
            host,
            ToolWindow {
                id: "dock_sources",
                place,
                width: SOURCES_W,
                float_at: map_rect.right_top() + egui::vec2(-SOURCES_W - 24.0, 60.0),
            },
            map_rect,
            &t,
            |ui| {
                header = ws::window_header(
                    ui,
                    &t,
                    ph::PULSE,
                    &title,
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        let summary = match (rows.len(), attention) {
                            (0, _) => "No active source is being tracked.".to_string(),
                            (n, 0) => format!("{n} active, all healthy"),
                            (n, k) => format!("{n} active \u{b7} {k} need attention"),
                        };
                        ui.label(ws::text(
                            summary,
                            12.0,
                            if attention > 0 { t.warn } else { t.text_dim },
                        ));
                    });
                let scroll = egui::ScrollArea::vertical().auto_shrink([false, floating]);
                let scroll = if floating {
                    scroll.max_height(list_h)
                } else {
                    scroll.max_height((ui.available_height() - 44.0).max(80.0))
                };
                scroll.show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for h in &rows {
                        source_row(ui, &t, h, now);
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    if ws::button(ui, &t, "Full table\u{2026}", 0.0)
                        .named("Open the full data source health table")
                        .clicked()
                    {
                        full_table = true;
                    }
                });
                ui.add_space(6.0);
            },
        );
        self.dock.apply_header(DockWin::Sources, header);
        if full_table {
            self.show_data_health = true;
        }
    }
}

/// One source: glyph, name and data age on one line, and its last error under it when failing.
fn source_row(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    h: &crate::app::SourceHealth,
    now: chrono::DateTime<chrono::Utc>,
) {
    let state = h.state();
    let (word, color) = health_look(state);
    let error = h.error.as_deref().filter(|_| needs_attention(state));
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), Sense::hover());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 0.0, t.panel_hi);
    }
    let y = rect.center().y;
    p.text(
        egui::pos2(rect.left() + 18.0, y),
        egui::Align2::CENTER_CENTER,
        health_glyph(state),
        FontId::proportional(13.0),
        color,
    );
    let age = data_age(h.latest_valid_time, now);
    let age_w = 44.0;
    p.text(
        egui::pos2(rect.right() - 12.0, y),
        egui::Align2::RIGHT_CENTER,
        &age,
        FontId::monospace(11.5),
        t.text_dim,
    );
    let mut job = egui::text::LayoutJob::simple_singleline(
        h.source.clone(),
        FontId::proportional(12.5),
        t.text,
    );
    let x = rect.left() + 34.0;
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - age_w - x).max(20.0));
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    ui.painter()
        .galley(egui::pos2(x, y - galley.size().y / 2.0), galley, t.text);
    let mut detail = format!(
        "{}: {word}\n{}\nNewest data: {}\nLast success: {} ({} cadence)\nCache: {}",
        h.source,
        h.endpoint_family.label(),
        valid_time_line(h.latest_valid_time),
        age_line(h.last_success),
        compact_age(h.cadence),
        h.cache_state.label(),
    );
    if let Some((ok, bad)) = h.recent_outcomes {
        detail.push_str(&format!("\nRecent: {ok}/{} succeeded", ok + bad));
    }
    if let Some(e) = &h.error {
        detail.push_str(&format!("\nLast error: {e}"));
    }
    resp.on_hover_text(detail).widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Label,
            true,
            format!("{}: {word}", h.source),
        )
    });
    if let Some(e) = error {
        let mut job = egui::text::LayoutJob::simple(
            e.to_string(),
            FontId::proportional(11.0),
            t.danger,
            (ui.available_width() - 44.0).max(40.0),
        );
        job.wrap.max_rows = 2;
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let (r, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), galley.size().y + 6.0),
            Sense::hover(),
        );
        ui.painter().galley(
            Rect::from_min_size(r.min + egui::vec2(34.0, 0.0), galley.size()).min,
            galley,
            t.danger,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rows_age_is_its_newest_datas_not_the_fetch_clock() {
        let now = chrono::Utc::now();
        assert_eq!(data_age(None, now), "\u{2014}");
        assert_eq!(data_age(Some(now), now), "now");
        let four_min = data_age(Some(now - chrono::Duration::minutes(4)), now);
        assert!(four_min.starts_with('4'), "{four_min}");
        assert!(needs_attention(HealthState::Failed) && !needs_attention(HealthState::Fresh));
    }
}
