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
use crate::ui::layers_panel::{age_line, compact_age, health_look, valid_time_line_at};
use egui::{FontId, Rect, Sense};
use egui_phosphor::regular as ph;

/// The window's width, docked or floating.
pub(super) const SOURCES_W: f32 = 300.0;

/// Session-only presentation state, independent of layer enablement and saved workspaces.
#[derive(Default)]
pub(super) struct SourceListState {
    query: String,
    attention_only: bool,
    expanded: Option<String>,
}

impl SourceListState {
    fn matches(&self, h: &SourceHealth) -> bool {
        if self.attention_only && !needs_attention(h.state()) {
            return false;
        }
        let query = self.query.trim().to_lowercase();
        query.is_empty()
            || h.source.to_lowercase().contains(&query)
            || h.endpoint_family.label().to_lowercase().contains(&query)
            || crate::ui::source_health_window::provider(h)
                .is_some_and(|p| p.to_lowercase().contains(&query))
    }
}

/// A state that asks for the analyst's attention (the summary line counts these).
fn needs_attention(state: HealthState) -> bool {
    matches!(
        state,
        HealthState::Failed | HealthState::Stale | HealthState::Cached | HealthState::Delayed
    )
}

/// Past data age, or a signed future valid-time offset. A forecast is never labeled "now".
fn data_age(
    latest: Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    match latest {
        None => "\u{2014}".into(),
        Some(t) => {
            let secs = (now - t).num_seconds();
            if t > now {
                format!(
                    "+{}",
                    compact_age(std::time::Duration::from_secs(secs.unsigned_abs().max(1)))
                )
            } else if secs < 5 {
                "now".into()
            } else {
                compact_age(std::time::Duration::from_secs(secs as u64))
            }
        }
    }
}

impl HookEchoApp {
    /// How many active feeds need a look: the Sources window's title count and its tab's dot.
    pub(super) fn sources_attention(&mut self) -> usize {
        let entries = self.palette_entries();
        crate::ui::source_health_window::active_health_rows(&entries)
            .iter()
            // The red dot is for what misleads when missing; a routine feed failing is listed,
            // amber, in the window without it.
            .filter(|h| {
                needs_attention(h.state()) && h.severity == crate::source_health::Severity::Critical
            })
            .count()
    }

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
        let list_state = &mut self.dock.sources_list;
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
                sources_list(ui, &t, &rows, now, list_state, floating.then_some(list_h));
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

fn sources_list(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    rows: &[&SourceHealth],
    now: chrono::DateTime<chrono::Utc>,
    state: &mut SourceListState,
    max_height: Option<f32>,
) {
    let attention = rows.iter().filter(|h| needs_attention(h.state())).count();
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            let summary = match (rows.len(), attention) {
                (0, _) => "No active source is being tracked.".to_string(),
                (n, 0) => format!("{n} active sources"),
                (n, k) => format!("{n} active \u{b7} {k} need attention"),
            };
            ui.label(ws::text(
                summary,
                12.0,
                if attention > 0 { t.warn } else { t.text_dim },
            ));
            ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .hint_text("Search sources or providers")
                    .desired_width(f32::INFINITY),
            )
            .on_hover_text("Search active sources and providers");
            if let Some(i) = ws::segmented(
                ui,
                t,
                &["All", "Attention"],
                usize::from(state.attention_only),
            ) {
                state.attention_only = i == 1;
            }
        });
    let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, max_height.is_some()]);
    scroll =
        scroll.max_height(max_height.unwrap_or_else(|| (ui.available_height() - 80.0).max(60.0)));
    scroll.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let visible: Vec<_> = rows.iter().copied().filter(|h| state.matches(h)).collect();
        for h in &visible {
            // Stable source identity keeps expanded details/focus attached while severity reorders.
            let id = ui.id().with(("source_health_row", &h.source));
            let expanded = state.expanded.as_deref() == Some(h.source.as_str());
            let response = source_row(ui, t, h, now, id, expanded);
            if response.clicked() {
                state.expanded = (!expanded).then(|| h.source.clone());
            }
        }
        if visible.is_empty() && !rows.is_empty() {
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.label(ws::text(
                        "No sources match these filters.",
                        12.0,
                        t.text_dim,
                    ));
                    if ws::button(ui, t, "Reset filters", 0.0).clicked() {
                        state.query.clear();
                        state.attention_only = false;
                    }
                });
        }
    });
    ui.add_space(6.0);
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(10, 0))
        .show(ui, |ui| {
            ui.label(ws::text(
                "Data age \u{b7} + means future valid time",
                10.5,
                t.text_faint,
            ));
            ui.label(ws::text(
                "Tap a source or press Enter for details",
                10.5,
                t.text_faint,
            ));
        });
}

/// One source: glyph, name and data age on one line, and its last error under it when failing.
fn source_row(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    h: &crate::app::SourceHealth,
    now: chrono::DateTime<chrono::Utc>,
    id: egui::Id,
    expanded: bool,
) -> egui::Response {
    let state = h.state();
    let (word, color) = health_look(state);
    let error = h.error.as_deref().filter(|_| needs_attention(state));
    let row_h = if ws::touch(ui.ctx()) { 44.0 } else { 28.0 };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::hover());
    let resp = ui.interact(rect, id, Sense::click());
    if resp.has_focus() {
        crate::hotkeys::reserve_navigation(ui.ctx(), id);
    }
    let p = ui.painter();
    if expanded || resp.hovered() || resp.has_focus() {
        p.rect_filled(rect, 0.0, t.panel_hi);
    }
    if resp.has_focus() {
        p.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(2.0, t.accent),
        );
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
    let age_w = 58.0;
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
        valid_time_line_at(h.latest_valid_time, now),
        age_line(h.last_success),
        compact_age(h.cadence),
        h.cache_state.label(),
    );
    if let Some(p) = crate::ui::source_health_window::provider(h) {
        detail.push_str(&format!("\nProvider: {p}"));
    }
    if !h.fallback_providers.is_empty() {
        detail.push_str(&format!(
            "\nAlternates: {}",
            h.fallback_providers.join(" → ")
        ));
    }
    for (label, value) in h
        .details
        .iter()
        .filter(|(label, _)| *label != "Active provider")
    {
        detail.push_str(&format!("\n{label}: {value}"));
    }
    detail.push_str(&format!(
        "\nNext try: {}",
        crate::ui::source_health_window::retry_line(h)
    ));
    if let Some((ok, bad)) = h.recent_outcomes {
        detail.push_str(&format!("\nRecent: {ok}/{} succeeded", ok + bad));
    }
    detail.push('\n');
    detail.push_str(&h.recovery());
    if let Some(e) = &h.error {
        detail.push_str(&format!("\nLast error: {e}"));
    }
    let resp = resp.named_toggle(
        &format!("{}: {word}. Show source details", h.source),
        expanded,
    );
    if expanded {
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.add(egui::Label::new(ws::text(&detail, 11.5, t.text_dim)).wrap());
            });
    } else if let Some(e) = error {
        let mut job = egui::text::LayoutJob::simple(
            e.to_string(),
            FontId::proportional(11.0),
            color,
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
            color,
        );
    }
    resp.on_hover_text(detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn health(name: &str, now: chrono::DateTime<chrono::Utc>) -> SourceHealth {
        let book = crate::app::RequestBook::default();
        let mut h = book.health(&crate::app::RequestLane::Field(
            crate::render::FieldLayer::Cape,
        ));
        h.source = name.into();
        h.last_success = Some(std::time::Duration::from_secs(30));
        h.latest_valid_time = Some(now - chrono::Duration::minutes(4));
        h.cache_state = crate::app::CacheState::Memory;
        h
    }

    #[test]
    fn a_rows_age_is_its_newest_datas_not_the_fetch_clock() {
        let now = chrono::Utc::now();
        assert_eq!(data_age(None, now), "\u{2014}");
        assert_eq!(data_age(Some(now), now), "now");
        let four_min = data_age(Some(now - chrono::Duration::minutes(4)), now);
        assert!(four_min.starts_with('4'), "{four_min}");
        assert_eq!(data_age(Some(now + chrono::Duration::hours(3)), now), "+3h");
        assert_ne!(
            data_age(Some(now + chrono::Duration::milliseconds(500)), now),
            "now"
        );
        assert!(needs_attention(HealthState::Failed) && !needs_attention(HealthState::Fresh));
    }

    #[test]
    fn attention_and_search_filters_keep_source_health_and_enablement_unchanged() {
        let now = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let good = health("CAPE", now);
        let mut cached = health("Low-level rotation", now);
        cached.last_failure = Some(std::time::Duration::from_secs(5));
        cached.error = Some("upstream timeout".into());
        cached
            .details
            .push(("Active provider", "Radar relay".into()));
        let mut filter = SourceListState {
            attention_only: true,
            ..Default::default()
        };
        assert!(!filter.matches(&good));
        assert!(filter.matches(&cached));
        filter.query = "  RELAY  ".into();
        assert!(filter.matches(&cached));
        filter.query = "cape".into();
        assert!(!filter.matches(&good));
        filter.attention_only = false;
        assert!(filter.matches(&good));
        assert_eq!(cached.state(), HealthState::Cached);
        assert_eq!(good.state(), HealthState::Fresh);
    }

    #[test]
    fn source_row_supports_keyboard_activation_and_touch_target_height() {
        let ctx = egui::Context::default();
        let now = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let h = health("MRMS rotation", now);
        let t = ws::Tokens::new(egui::Color32::LIGHT_BLUE);
        let id = egui::Id::new("keyboard_source_row");
        ws::set_touch(&ctx, true);
        let mut clicked = false;
        let mut draw = |ui: &mut egui::Ui| {
            let response = source_row(ui, &t, &h, now, id, false);
            assert_eq!(response.rect.height(), 44.0);
            clicked = response.clicked();
        };
        let raw = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(300.0, 300.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run_ui(raw(), &mut draw);
        ctx.memory_mut(|m| m.request_focus(id));
        let mut input = raw();
        input.events.push(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let _ = ctx.run_ui(input, &mut draw);
        assert!(
            clicked,
            "focused source details must open without hover or a pointer"
        );
    }

    #[test]
    fn long_source_errors_and_details_fit_a_narrow_host() {
        let ctx = egui::Context::default();
        let now = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let mut h = health("A provider with a very long display name", now);
        h.error = Some(
            "upstream connection timed out while requesting a missing archive scan ".repeat(6),
        );
        h.last_failure = Some(std::time::Duration::from_secs(5));
        let t = ws::Tokens::new(egui::Color32::LIGHT_BLUE);
        for width in [240.0, 300.0] {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 800.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    source_row(ui, &t, &h, now, egui::Id::new("narrow_source"), true);
                    assert!(
                        ui.min_rect().width() <= width,
                        "source details must wrap inside the dock"
                    );
                },
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn dependency_health(case: &str, now: chrono::DateTime<chrono::Utc>) -> SourceHealth {
        use crate::{
            provider_health::ProviderHealth,
            radar_provider_manager::{FailoverSnapshot, SelectedTier},
        };
        use wxdata::{
            live_block::ProviderCapabilities,
            provider_topology::{
                DeclarationUnavailable, InputMode, ProviderTopology, UpstreamDeclaration,
                UNIDATA_AWS_DOMAIN,
            },
        };
        let mut primary = ProviderHealth::new("Unidata/AWS", ProviderCapabilities::unidata());
        primary.record_topology(ProviderTopology::adapter(UNIDATA_AWS_DOMAIN), now);
        let mut backup = ProviderHealth::new("HookEcho Relay", ProviderCapabilities::relay());
        if case != "unknown" {
            backup.record_topology(
                ProviderTopology::relay(
                    UpstreamDeclaration::new(
                        if case == "shared" {
                            InputMode::Replay
                        } else {
                            InputMode::Live
                        },
                        if case == "distinct" {
                            vec!["operator-idd-peer-with-an-explicit-network-dependency".into()]
                        } else {
                            vec![
                                UNIDATA_AWS_DOMAIN.into(),
                                "deployment-network-with-a-long-declared-failure-domain-identifier"
                                    .into(),
                            ]
                        },
                    )
                    .unwrap(),
                ),
                now - chrono::Duration::minutes(10),
            );
        }
        if case == "refresh-failed" {
            backup.record_topology(
                ProviderTopology::unknown(DeclarationUnavailable::HttpFailure),
                now,
            );
        }
        let snapshot = FailoverSnapshot {
            selected: SelectedTier::Primary,
            has_backup: true,
            primary: Some(primary),
            backup: Some(backup),
            manual_override: false,
            last_transition: None,
        };
        let mut h = health("Level II radar", now);
        h.endpoint_family = crate::source_health::EndpointFamily::RadarLevel2;
        h.fallback_providers = snapshot
            .alternate_provider_labels()
            .into_iter()
            .map(str::to_string)
            .collect();
        h.details = crate::app::chrome::registry::failover_details(&snapshot);
        h
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn dependency_disclosure_wraps_and_diagnostics_retain_current_unknown_and_dated_history() {
        let now = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        for case in ["shared", "distinct", "unknown", "refresh-failed"] {
            let h = dependency_health(case, now);
            let json = serde_json::to_value(crate::app::DiagnosticsSourceHealth::from(&h)).unwrap();
            assert_eq!(
                json["latest_valid_time"],
                h.latest_valid_time.unwrap().to_rfc3339()
            );
            assert_eq!(json["details"], serde_json::to_value(&h.details).unwrap());
            let details = h
                .details
                .iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join("\n");
            match case {
                "shared" => {
                    assert!(details.contains("shared declared upstream: unidata-level2-aws"));
                    assert!(details.contains("not a live upstream backup"));
                }
                "distinct" => assert!(details
                    .contains("declared domains differ; independent redundancy not established")),
                "unknown" => assert!(
                    details.contains("Relay upstream: unknown")
                        && !details.contains("Previous relay")
                ),
                _ => {
                    assert!(
                        details.contains("Relay upstream: unknown (metadata endpoint unavailable)")
                    );
                    assert!(details.contains("Previous relay declaration:"));
                    assert!(details.contains("2023-11-14T22:03:20+00:00; current upstream unknown"));
                    assert!(!details.contains("shared declared upstream:"));
                }
            }
            let ctx = egui::Context::default();
            let t = ws::Tokens::new(egui::Color32::LIGHT_BLUE);
            for width in [240.0, 300.0] {
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 1800.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        source_row(ui, &t, &h, now, egui::Id::new("dependency_source"), true);
                        assert!(
                            ui.min_rect().width() <= width,
                            "{case} overflows {width}px dock"
                        );
                    },
                );
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes declared upstream Source details for visual review"]
    fn gpu_upstream_dependency_sources_snapshots() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for upstream review");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m1.3/upstream-domains/ui");
        std::fs::create_dir_all(&destination).unwrap();
        let now = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let t = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        for case in ["shared", "distinct", "unknown", "refresh-failed"] {
            let h = dependency_health(case, now);
            std::fs::write(
                destination.join(format!("{case}.json")),
                serde_json::to_vec_pretty(&crate::app::DiagnosticsSourceHealth::from(&h)).unwrap(),
            )
            .unwrap();
            for width in [240, 300] {
                gpu.save(
                    &destination.join(format!("{case}-{width}.png")),
                    width,
                    1600,
                    |ui| {
                        ws::set_touch(ui.ctx(), true);
                        ws::panel_frame(&t).show(ui, |ui| {
                            ws::style_scope(ui, &t);
                            ws::window_header(ui, &t, ph::PULSE, "Sources", None, None);
                            source_row(ui, &t, &h, now, egui::Id::new("dependency_source"), true);
                        });
                    },
                )
                .unwrap();
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: captures model context transitions in production Sources rows"]
    fn gpu_model_context_sources_snapshots() {
        let gpu =
            crate::headless::ui::Snapshot::new().expect("GPU adapter for model context review");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/model-context/ui");
        std::fs::create_dir_all(&destination).unwrap();
        let now = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let t = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        for case in ["waiting", "fetching", "failed", "loaded"] {
            let h = crate::app::model_context::tests::review_health(case);
            std::fs::write(
                destination.join(format!("{case}.json")),
                serde_json::to_vec_pretty(&crate::app::DiagnosticsSourceHealth::from(&h)).unwrap(),
            )
            .unwrap();
            for width in [240, 300] {
                gpu.save(
                    &destination.join(format!("{case}-{width}.png")),
                    width,
                    760,
                    |ui| {
                        ws::set_touch(ui.ctx(), true);
                        ws::panel_frame(&t).show(ui, |ui| {
                            ws::style_scope(ui, &t);
                            ws::window_header(ui, &t, ph::PULSE, "Sources", None, None);
                            source_row(
                                ui,
                                &t,
                                &h,
                                now,
                                egui::Id::new("model_context_source"),
                                true,
                            );
                            assert!(
                                ui.min_rect().right() <= width as f32 + 0.5,
                                "{case}: horizontal overflow"
                            );
                            assert!(ui.min_rect().bottom() <= 760.0, "{case}: vertical overflow");
                        });
                    },
                )
                .unwrap();
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes Sources dock captures for visual review"]
    fn gpu_sources_dock_snapshots() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for Sources review");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/sources");
        std::fs::create_dir_all(&destination).unwrap();
        let now = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let mut cached = health("MRMS low-level rotation", now);
        cached.endpoint_family =
            crate::source_health::field_endpoint_family(crate::render::FieldLayer::Rotation);
        cached.cadence = std::time::Duration::from_secs(120);
        cached.last_failure = Some(std::time::Duration::from_secs(5));
        cached.error = Some("Upstream timeout. Retaining the previous scan.".into());
        let mut forecast = health("HRRR mixed-layer CAPE", now);
        forecast.latest_valid_time = Some(now + chrono::Duration::hours(3));
        let rows = [&cached, &forecast];
        let t = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        for width in [240, 300, 400] {
            for touch in [false, true] {
                let mut state = SourceListState {
                    expanded: Some(cached.source.clone()),
                    ..Default::default()
                };
                gpu.save(
                    &destination.join(format!("sources-{width}-touch-{touch}.png")),
                    width,
                    700,
                    |ui| {
                        ws::set_touch(ui.ctx(), touch);
                        ws::panel_frame(&t).show(ui, |ui| {
                            ws::style_scope(ui, &t);
                            ws::window_header(ui, &t, ph::PULSE, "Sources (1)", None, None);
                            sources_list(ui, &t, &rows, now, &mut state, Some(520.0));
                        });
                    },
                )
                .unwrap();
            }
        }
    }
}
