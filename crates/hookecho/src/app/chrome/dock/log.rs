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
    provider: Option<&'static str>,
    streaming: bool,
    progress: Option<wxdata::live::ScanProgress>,
    /// Seconds behind the radar at the last arrival.
    lag_s: Option<f32>,
    decode_ms: Option<f32>,
    retries: u32,
    frame_age_s: Option<i64>,
    /// `(ingest lag s, decode ms)` of recent arrivals, oldest first.
    history: Vec<(f32, f32)>,
    couplets: Option<usize>,
    debris: Option<usize>,
}

impl HookEchoApp {
    fn live_stats(&self) -> LiveStats {
        let v = &self.views[self.active];
        let streaming = self
            .live_stream
            .as_ref()
            .is_some_and(|(i, ..)| *i == self.active);
        let key = self.volume_key(self.active);
        LiveStats {
            site: v.site.clone().unwrap_or_default(),
            provider: self.live_stream.as_ref().and_then(|s| s.3),
            streaming,
            progress: v.live_progress,
            lag_s: v.live_history.back().map(|h| h.1),
            decode_ms: v.last_decode_time.map(|d| d.as_secs_f32() * 1000.0),
            retries: v.live_retries,
            frame_age_s: v
                .timeline
                .current()
                .and_then(|id| id.date_time())
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
        if let Some(p) = s.provider {
            ui.label(ws::text(p, 11.0, t.text_dim));
        }
    });
    if let Some(p) = s.progress {
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
    if s.retries > 0 {
        ws::kv(ui, t, "Retries", &s.retries.to_string(), Some(t.warn));
    }
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
}
