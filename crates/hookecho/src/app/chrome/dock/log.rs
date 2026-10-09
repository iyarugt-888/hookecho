//! The Analyst log window: Analyst Mode's live sweep, provider-health and failover log
//! (`ui::analyst_log_window`) as a workstation tool window. It is present while Analyst Mode is
//! on and docks right by default, joining the other right-hand windows as a tab. Closing it turns
//! Analyst Mode off, exactly as closing the floating window does in the other layouts, so the
//! setting and the window cannot disagree.

use super::*;
use egui_phosphor::regular as ph;

/// The window's width, docked or floating.
pub(super) const LOG_W: f32 = 300.0;

/// What the log's header shows: the live feed's state for the active pane and what the detectors
/// found in its current volume.
pub(super) struct LiveStats {
    site: String,
    streaming: bool,
    scan: crate::live_scan::LiveScan,
    /// Seconds behind the radar at the last arrival.
    lag_s: Option<f32>,
    decode_ms: Option<f32>,
    /// Client transport receipt to completed GPU queue writes, p50/p95 and sample count.
    queue_ms: Option<(f32, f32, usize)>,
    /// Receipt → GPU finished the frame that drew it: p50, p95, samples.
    gpu_done_ms: Option<(f32, f32, usize)>,
    /// Live uploads whose GPU completion was not observed (excluded, not counted as zero).
    gpu_unobserved: u64,
    retries: u32,
    /// New warnings: their `sent` → the app accepted them → the first frame built after.
    alerts: wxdata::alert_latency::Summary,
    /// The same for warnings pushed from the NWWS-OI relay, when one is configured.
    wire: wxdata::alert_latency::Summary,
    frame_age_s: Option<i64>,
    /// `(ingest lag s, decode ms)` of recent arrivals, oldest first.
    history: Vec<(f32, f32)>,
    couplets: Option<usize>,
    debris: Option<usize>,
    frames: Option<crate::app::telemetry::FrameSummary>,
    panes: usize,
    /// Radar volumes held across every pane, the one on screen included.
    volumes: usize,
    tiles: crate::tiles::TileStats,
}

impl HookEchoApp {
    fn live_stats(&self) -> LiveStats {
        let v = &self.views[self.active];
        let streaming = self.live_session.streaming_for(self.active);
        let key = self.volume_key(self.active);
        LiveStats {
            site: v.site.clone().unwrap_or_default(),
            streaming,
            scan: v.live_scan.clone(),
            lag_s: v.live_history.back().map(|h| h.1),
            decode_ms: v.last_decode_time.map(|d| d.as_secs_f32() * 1000.0),
            queue_ms: queue_percentiles(&v.live_queue_timings.samples_micros()),
            gpu_done_ms: queue_percentiles(&v.live_queue_timings.gpu_done_samples_micros()),
            gpu_unobserved: v.live_queue_timings.unobserved(),
            retries: v.live_retries,
            alerts: self.alert_latency_summary(),
            wire: self.wire_latency_summary(),
            frame_age_s: v
                .displayed_radar_time()
                .map(|d| (chrono::Utc::now() - d).num_seconds()),
            history: v.live_history.iter().map(|h| (h.1, h.2)).collect(),
            couplets: self
                .couplet_cache
                .as_ref()
                .filter(|(k, _)| *k == key)
                .map(|(_, (hits, ..))| hits.len()),
            debris: self
                .tds_cache
                .as_ref()
                .filter(|(k, _)| *k == key)
                .map(|(_, hits)| hits.len()),
            frames: self.frame_times.summary(),
            tiles: self.tiles.stats(),
            panes: self.views.len(),
            volumes: self
                .views
                .iter()
                .map(|v| usize::from(v.volume.is_some()) + v.recent_len())
                .sum(),
        }
    }

    pub(super) fn dock_log(&mut self, host: Host<'_>) {
        if !self.dock.log.open || !self.dock.log_available {
            return;
        }
        let t = self.ws_tokens();
        let map_rect = self.chrome_rect;
        let place = self.dock.log.place;
        let floating = place == Place::Float;
        let collapsed = floating && self.dock.log.collapsed;
        let float_h = (map_rect.height() - 320.0).clamp(140.0, 360.0);
        let stats = self.live_stats();
        let mut header = ws::HeaderAction::None;
        tool_window(
            host,
            ToolWindow {
                id: "dock_log",
                place,
                width: LOG_W,
                float_at: map_rect.right_top() + egui::vec2(-LOG_W - 24.0, 90.0),
            },
            map_rect,
            &t,
            |ui| {
                header = ws::window_header(
                    ui,
                    &t,
                    ph::TERMINAL_WINDOW,
                    "Analyst log",
                    Some(place),
                    floating.then_some(collapsed),
                );
                if collapsed {
                    return;
                }
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        live_stats(ui, &t, &stats);
                        let h = if floating {
                            float_h
                        } else {
                            (ui.available_height() - 40.0).max(120.0)
                        };
                        crate::ui::analyst_log_window::body(ui, h);
                    });
            },
        );
        if header == ws::HeaderAction::Close {
            // The log is Analyst Mode's window: closing it is how the mode is switched off here,
            // with the log level restored the same way the Settings checkbox does it.
            self.settings.analyst_mode = false;
            crate::devlog::set_analyst_mode(false);
        }
        self.dock.apply_header(DockWin::Log, header);
    }
}

/// The header: feed state, the sweep in progress, lag and decode time, a graph of recent
/// arrivals, and the current volume's detections.
fn live_stats(ui: &mut egui::Ui, t: &ws::Tokens, s: &LiveStats) {
    let (word, color) = if s.streaming {
        ("Streaming", t.live)
    } else {
        ("Polling", t.warn)
    };
    ui.horizontal(|ui| {
        ws::badge(ui, t, word, color);
        ui.label(ws::mono(&s.site, 12.0, egui::Color32::WHITE));
        if let Some(p) = &s.scan.provider {
            ui.label(ws::text(p, 11.0, t.text_dim));
        }
    });
    if let Some(mode) = s.scan.source_mode {
        ws::kv(ui, t, "Source mode", mode.label(), None);
    }
    if let Some(reason) = &s.scan.switch_reason {
        ws::kv(ui, t, "Last switch", reason, Some(t.warn));
    }
    if let Some(error) = &s.scan.last_stream_error {
        ui.add(
            egui::Label::new(ws::text(
                format!("Last live stream error: {error}"),
                11.0,
                t.warn,
            ))
            .wrap(),
        );
    }
    if let Some(p) = s.scan.progress {
        ws::kv(
            ui,
            t,
            "Sweep",
            &format!(
                "{:.1}\u{b0}  {}/{}  \u{b7}  chunk {}/{}",
                p.elevation_angle_deg,
                p.elevation_number,
                p.total_elevations,
                p.chunk_index,
                p.chunks_in_sweep
            ),
            None,
        );
    }
    scan_progression(ui, t, &s.scan);
    // While streaming, the timeline's frame is the last archived volume, not what is on screen.
    if let Some(a) = s.frame_age_s.filter(|_| !s.streaming) {
        let stale = a > 20 * 60;
        ws::kv(
            ui,
            t,
            "Frame age",
            &humanize(a.max(0)),
            stale.then_some(t.warn),
        );
    }
    if let Some(l) = s.lag_s {
        ws::kv(
            ui,
            t,
            "Ingest lag",
            &format!("{l:.0} s"),
            (l > 120.0).then_some(t.warn),
        );
    }
    if let Some(d) = s.decode_ms {
        ws::kv(ui, t, "Decode", &format!("{d:.0} ms"), None);
    }
    if let Some((p50, p95, count)) = s.queue_ms {
        ws::kv(
            ui,
            t,
            "To GPU queue",
            &format!("p50 {p50:.0} / p95 {p95:.0} ms · {count}"),
            (p50 >= 100.0 || p95 >= 250.0).then_some(t.warn),
        );
        ui.label(ws::text(
            "Client receipt → GPU queue writes",
            10.0,
            t.text_dim,
        ));
    }
    if let Some((p50, p95, count)) = s.gpu_done_ms {
        ws::kv(
            ui,
            t,
            "GPU done",
            &if s.gpu_unobserved > 0 {
                format!(
                    "p50 {p50:.0} / p95 {p95:.0} ms · {count} ({} not observed)",
                    s.gpu_unobserved
                )
            } else {
                format!("p50 {p50:.0} / p95 {p95:.0} ms · {count}")
            },
            (p50 >= 150.0 || p95 >= 400.0).then_some(t.warn),
        );
        ui.label(ws::text(
            "Client receipt → GPU finished the frame that drew it (seen by the next frame at \
             the latest; screen scan-out not measured)",
            10.0,
            t.text_dim,
        ));
    }
    if s.retries > 0 {
        ws::kv(ui, t, "Retries", &s.retries.to_string(), Some(t.warn));
    }
    alert_latency_rows(ui, t, &s.alerts, "Warning arrival");
    alert_latency_rows(ui, t, &s.wire, "Wire arrival");
    // This app's own cost, measured here and kept here (ROADMAP_2 §14.1).
    if let Some(f) = s.frames {
        use crate::app::telemetry::{BUDGET_MS, STALL_MS};
        ws::kv(
            ui,
            t,
            "Frame build",
            &format!("p50 {:.1} / p95 {:.1} / max {:.0} ms", f.p50, f.p95, f.max),
            (f.p95 > BUDGET_MS).then_some(t.warn),
        );
        ws::kv(
            ui,
            t,
            "Over budget",
            &format!(
                "{} of the last {} over {BUDGET_MS:.1} ms · {} stall{} over {STALL_MS:.0} ms of {}",
                f.over_budget,
                f.kept,
                f.stalls,
                if f.stalls == 1 { "" } else { "s" },
                f.total
            ),
            (f.stalls > 0).then_some(t.warn),
        );
    }
    ws::kv(
        ui,
        t,
        "Held",
        &format!(
            "{} pane{} · {} radar volume{}",
            s.panes,
            if s.panes == 1 { "" } else { "s" },
            s.volumes,
            if s.volumes == 1 { "" } else { "s" }
        ),
        None,
    );
    // Basemap coverage (ROADMAP_2 §4.5): a missing tile is drawn from a coarser one until it
    // arrives, so gaps here show as blur on the map rather than holes.
    let tl = s.tiles;
    ws::kv(
        ui,
        t,
        "Map tiles",
        &format!(
            "{} loaded · {} loading · {} failed{}",
            tl.resident,
            tl.loading,
            tl.failed,
            if tl.stubborn > 0 {
                format!(" ({} backing off)", tl.stubborn)
            } else {
                String::new()
            }
        ),
        (tl.failed > 0).then_some(t.warn),
    );
    // The detectors run only while their layers are on; "off" says that, not "none found".
    let det = |n: Option<usize>| n.map_or_else(|| "off".to_string(), |n| n.to_string());
    ws::kv(
        ui,
        t,
        "Detections",
        &format!(
            "rotation {} \u{b7} debris {}",
            det(s.couplets),
            det(s.debris)
        ),
        (s.couplets.unwrap_or(0) + s.debris.unwrap_or(0) > 0).then_some(t.warn),
    );
    lag_graph(ui, t, &s.history);
    ui.add_space(4.0);
}

fn scan_progression(ui: &mut egui::Ui, t: &ws::Tokens, scan: &crate::live_scan::LiveScan) {
    use crate::live_scan::CutCoverage;

    let Some(state) = scan.progression(chrono::Utc::now()) else {
        return;
    };
    ui.add_space(4.0);
    ui.label(ws::text("SCAN PROGRESSION", 10.0, t.text_dim));
    ws::kv(
        ui,
        t,
        "VCP",
        &state
            .vcp_number
            .map_or_else(|| "unknown".to_owned(), |n| n.to_string()),
        None,
    );
    ws::kv(
        ui,
        t,
        "Cuts",
        &format!(
            "{} observed / {} expected · {} chunk inventories received",
            state.observed_cuts, state.expected_cuts, state.completed_cuts
        ),
        None,
    );
    ws::kv(
        ui,
        t,
        "Next",
        &state.expected_next_cut.map_or_else(
            || {
                if state.volume_complete {
                    "volume complete".to_owned()
                } else {
                    "awaiting missing cuts".to_owned()
                }
            },
            |n| format!("cut {n}"),
        ),
        None,
    );
    if let Some(elapsed) = state.elapsed_secs {
        ws::kv(ui, t, "Elapsed", &humanize(elapsed), None);
    }
    if let Some(remaining) = state
        .projected_remaining_secs
        .filter(|_| !state.volume_complete)
    {
        ws::kv(
            ui,
            t,
            "Estimate",
            &format!("~{} at current cut rate", humanize(remaining.ceil() as i64)),
            None,
        );
    }
    // Raw positions not observed inside the chunks received for the current cut.
    if let Some(gaps) = crate::live_scan::gap_summary(&scan.radial_gaps()) {
        ws::kv(ui, t, "Gaps", &gaps, Some(t.warn));
    }
    ui.horizontal_wrapped(|ui| {
        for number in 1..=state.expected_cuts.min(64) {
            let (color, status) = match scan.cut_coverage(number) {
                CutCoverage::Unobserved => (t.text_faint, "unobserved"),
                CutCoverage::Partial => (t.warn, "partial"),
                CutCoverage::Complete => (t.live, "chunk inventory received"),
            };
            let kind = scan.cut_kind(number).map_or("", |kind| kind.label());
            ws::badge(ui, t, &number.to_string(), color)
                .on_hover_text(format!("Cut {number}: {status} {kind}"));
        }
    });
    crate::ui::acquisition_inventory::show(ui, t, scan);
}

/// Ingest lag of recent arrivals as a line (accent), decode time as bars (dim), on their own
/// scales, newest on the right.
fn lag_graph(ui: &mut egui::Ui, t: &ws::Tokens, h: &[(f32, f32)]) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, t.field);
    if h.len() < 2 {
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Live arrivals graph here once a stream runs",
            egui::FontId::proportional(10.5),
            t.text_faint,
        );
        return;
    }
    let inner = rect.shrink2(egui::vec2(4.0, 12.0));
    let n = h.len();
    let x = |i: usize| inner.left() + inner.width() * i as f32 / (n - 1) as f32;
    let max_d = robust_max(h.iter().map(|v| v.1));
    for (i, (_, d)) in h.iter().enumerate() {
        let bh = inner.height() * (d / max_d).clamp(0.0, 1.0);
        p.line_segment(
            [
                egui::pos2(x(i), inner.bottom()),
                egui::pos2(x(i), inner.bottom() - bh),
            ],
            egui::Stroke::new(2.0, t.text_faint),
        );
    }
    let max_l = robust_max(h.iter().map(|v| v.0));
    let pts: Vec<egui::Pos2> = h
        .iter()
        .enumerate()
        .map(|(i, (l, _))| {
            egui::pos2(
                x(i),
                inner.bottom() - inner.height() * (l / max_l).clamp(0.0, 1.0),
            )
        })
        .collect();
    p.add(egui::Shape::line(pts, egui::Stroke::new(1.5, t.accent)));
    p.text(
        rect.left_top() + egui::vec2(4.0, 1.0),
        egui::Align2::LEFT_TOP,
        format!("lag, to {max_l:.0} s"),
        egui::FontId::proportional(9.5),
        t.accent,
    );
    p.text(
        rect.right_top() + egui::vec2(-4.0, 1.0),
        egui::Align2::RIGHT_TOP,
        format!("decode, to {max_d:.0} ms"),
        egui::FontId::proportional(9.5),
        t.text_dim,
    );
}

/// The graph's top: the 90th percentile, padded. A stream's first update decodes the whole
/// backfilled volume and arrives minutes behind; scaled to it, every chunk after is a flat line.
/// Values above the top clip at it.
fn robust_max(values: impl Iterator<Item = f32>) -> f32 {
    let mut v: Vec<f32> = values.filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return 1.0;
    }
    v.sort_by(f32::total_cmp);
    let p90 = v[((v.len() - 1) as f32 * 0.9).round() as usize];
    (p90 * 1.25).max(1.0)
}

fn queue_percentiles(samples_micros: &[u64]) -> Option<(f32, f32, usize)> {
    if samples_micros.is_empty() {
        return None;
    }
    let mut sorted = samples_micros.to_vec();
    sorted.sort_unstable();
    let at = |percent: usize| {
        let rank = (sorted.len() * percent).div_ceil(100).saturating_sub(1);
        sorted[rank] as f32 / 1000.0
    };
    Some((at(50), at(95), sorted.len()))
}

/// New warnings' delivery: from the NWS `sent` time to the app, and on to the first frame built
/// with them. Nothing is shown before a new warning has arrived; messages active when the app
/// started are not measured (`wxdata::alert_latency`).
fn alert_latency_rows(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    a: &wxdata::alert_latency::Summary,
    label: &str,
) {
    let Some(sent) = a.sent_to_received else {
        return;
    };
    let wire = label == "Wire arrival";
    ws::kv(
        ui,
        t,
        label,
        &format!(
            "p50 {:.0} / p95 {:.0} s · {}",
            sent.p50_s, sent.p95_s, sent.count
        ),
        (sent.p95_s > 180.0).then_some(t.warn),
    );
    if let Some(drawn) = a.received_to_drawn {
        ws::kv(
            ui,
            t,
            "Then drawn",
            &format!(
                "p50 {:.2} / p95 {:.2} s · {}",
                drawn.p50_s, drawn.p95_s, drawn.count
            ),
            None,
        );
    }
    ui.label(ws::text(
        if wire {
            "NWS header time (to the minute) → this app received it from the NWWS-OI relay \
             → the first frame built after (not screen scan-out)"
        } else {
            "NWS sent time → this app accepted the poll that carried it (includes waiting \
             for the 2-minute poll; NWS clock against this computer's) → the first frame \
             built after (not screen scan-out)"
        },
        10.0,
        t.text_dim,
    ));
}

#[cfg(test)]
mod tests {
    #[test]
    fn one_backfill_outlier_does_not_flatten_the_graph() {
        let mut v = vec![4409.0];
        v.extend(std::iter::repeat_n(120.0, 20));
        let top = super::robust_max(v.into_iter());
        assert!((120.0..200.0).contains(&top), "{top}");
        assert_eq!(super::robust_max(std::iter::empty()), 1.0);
    }

    #[test]
    fn queue_percentiles_use_nearest_rank_and_keep_tail_latency_visible() {
        let samples: Vec<u64> = (1..=20).map(|ms| ms * 1000).collect();
        assert_eq!(super::queue_percentiles(&samples), Some((10.0, 19.0, 20)));
        assert_eq!(super::queue_percentiles(&[]), None);
    }
}
