//! Opt-in valid-time coordination for multi-radar archive panes.
use super::HookEchoApp;

impl HookEchoApp {
    /// The active pane owns the analysis cursor. Other NEXRAD panes seek their own nearest
    /// volume when it is scrubbed; while it follows live, they follow their own live heads so
    /// each source can keep polling. GOES has its own frame cursor; MRMS currently stays live
    /// and warns when its valid time differs.
    pub(super) fn sync_linked_pane_times(&mut self) {
        if !self.link_times || self.views.len() < 2 {
            return;
        }
        let active = self.active.min(self.views.len() - 1);
        let source = &self.views[active].timeline;
        let live_day = source.following.then_some(source.date);
        let target = (!source.following)
            .then(|| source.current().and_then(|id| id.date_time()))
            .flatten();
        for (idx, view) in self.views.iter_mut().enumerate() {
            if idx == active || !view.site.as_deref().is_some_and(wxdata::sites::is_nexrad) {
                continue;
            }
            if let Some(day) = live_day {
                if !view.timeline.following || view.timeline.date != day {
                    view.timeline.follow_day(day);
                    view.volume = None;
                    view.loading = false;
                    view.last_poll = None;
                }
            } else if let Some(target) = target {
                let site = view.site.as_deref().unwrap_or_default();
                if view.timeline.seek_to_valid_time(site, target) {
                    view.volume = None;
                    view.loading = false;
                }
            }
        }
    }

    /// Show the scan actually painted in each pane and its offset from the selected analysis
    /// instant. This matters most when sites scan at different cadences or a frame is still
    /// downloading: linked cursors must never imply identical source times.
    pub(super) fn paint_linked_time_badges(&self, ui: &egui::Ui, rects: &[egui::Rect], solo: bool) {
        if !self.link_times || rects.len() < 2 {
            return;
        }
        let active = self.active.min(rects.len() - 1);
        let source = &self.views[active];
        let selected = if source.timeline.following && !source.timeline.live_looping() {
            source
                .volume
                .as_ref()
                .map(|volume| volume.time)
                .or_else(|| source.timeline.current().and_then(|id| id.date_time()))
        } else {
            source
                .timeline
                .current()
                .and_then(|id| id.date_time())
                .or_else(|| source.volume.as_ref().map(|volume| volume.time))
        };
        let tolerance = chrono::Duration::minutes(self.settings.time_mismatch_minutes as i64);
        for (idx, rect) in rects.iter().enumerate() {
            if solo && idx != active {
                continue;
            }
            let view = &self.views[idx];
            let Some(site) = view.site.as_deref() else {
                continue;
            };
            let (caption, warning) = match (&view.volume, selected) {
                (Some(volume), Some(selected)) => {
                    let comparison =
                        wxdata::time_align::TimeOffset::between(volume.time, selected, tolerance);
                    let offset = crate::ui::data_inspector::offset_label(comparison.offset);
                    (
                        format!(
                            "{site} · {}Z · Δ{offset}",
                            volume.time.format("%Y-%m-%d %H:%M:%S")
                        ),
                        comparison.outside_tolerance,
                    )
                }
                (Some(volume), None) => (
                    format!("{site} · {}Z", volume.time.format("%Y-%m-%d %H:%M:%S")),
                    false,
                ),
                (None, _) if view.timeline.listing || view.loading => {
                    (format!("{site} · loading scan"), false)
                }
                (None, _) => (format!("{site} · no scan for selected time"), false),
            };
            let caption = if warning {
                format!("⚠ {caption}")
            } else {
                caption
            };
            let color = if warning {
                egui::Color32::from_rgb(255, 197, 92)
            } else {
                egui::Color32::WHITE
            };
            let painter = ui.painter_at(*rect);
            let font = egui::FontId::proportional(11.0);
            let galley = painter.layout_no_wrap(caption.clone(), font.clone(), color);
            let origin = egui::pos2(rect.left() + 8.0, rect.bottom() - galley.size().y - 31.0);
            let background = egui::Rect::from_min_size(
                origin - egui::vec2(5.0, 3.0),
                galley.size() + egui::vec2(10.0, 6.0),
            );
            painter.rect_filled(background, 3.0, egui::Color32::from_black_alpha(210));
            painter.text(origin, egui::Align2::LEFT_TOP, caption, font, color);
        }
    }
}
