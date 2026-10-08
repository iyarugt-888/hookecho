//! Opt-in valid-time coordination for multi-radar archive panes.
use super::HookEchoApp;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AnalysisCursor {
    Live,
    Archive(DateTime<Utc>),
}

/// The selected analysis instant must survive pane focus changes. A linked pane can only land on
/// its own nearest scan; adopting that scan as a new target on focus would drift every pane.
#[derive(Default)]
pub(super) struct LinkedTimeState {
    pub cursor: Option<AnalysisCursor>,
    active: Option<usize>,
    site: Option<String>,
    observed: Option<DateTime<Utc>>,
    following: bool,
    settling: bool,
    live_time: Option<DateTime<Utc>>,
}

impl LinkedTimeState {
    /// A timed source such as GOES can become the analysis clock. The next sync must align the
    /// focused radar too, so clear the remembered pane identity to force one retarget pass.
    pub fn select_external(&mut self, time: Option<DateTime<Utc>>) {
        self.cursor = Some(time.map_or(AnalysisCursor::Live, AnalysisCursor::Archive));
        self.active = None;
        self.site = None;
        self.observed = time;
        self.following = time.is_none();
        self.settling = true;
        if time.is_none() {
            self.live_time = None;
        }
    }

    pub fn select_explicit(
        &mut self,
        active: usize,
        site: Option<&str>,
        time: Option<DateTime<Utc>>,
    ) {
        self.cursor = Some(time.map_or(AnalysisCursor::Live, AnalysisCursor::Archive));
        self.active = Some(active);
        self.site = site.map(str::to_owned);
        self.observed = time;
        self.following = time.is_none();
        self.settling = time.is_some();
    }

    /// Returns true when the newly focused/site-changed pane needs to be aligned to the retained
    /// cursor. Ordinary changes on the same active pane are user scrubs or playback and own it.
    fn observe(
        &mut self,
        active: usize,
        site: Option<&str>,
        observed: Option<DateTime<Utc>>,
        following: bool,
        pending_seek: bool,
    ) -> bool {
        if self.cursor.is_none() {
            self.cursor = if following {
                Some(AnalysisCursor::Live)
            } else {
                observed.map(AnalysisCursor::Archive)
            };
            self.active = Some(active);
            self.site = site.map(str::to_owned);
            self.observed = observed;
            self.following = following;
            self.settling = pending_seek;
            if following {
                self.live_time = observed.or(self.live_time);
            }
            return false;
        }
        let switched = self.active != Some(active) || self.site.as_deref() != site;
        self.active = Some(active);
        self.site = site.map(str::to_owned);
        if switched {
            self.settling = true;
            return site.is_some();
        }
        if self.settling {
            if site.is_some() && !pending_seek && (observed.is_some() || following) {
                self.settling = false;
                self.observed = observed;
                self.following = following;
                if following {
                    self.live_time = observed.or(self.live_time);
                }
            }
            return false;
        }
        if following != self.following
            || (!following && observed.is_some() && observed != self.observed)
        {
            if following {
                self.cursor = Some(AnalysisCursor::Live);
            } else if let Some(target) = observed {
                self.cursor = Some(AnalysisCursor::Archive(target));
            }
            self.settling = pending_seek;
        }
        self.observed = observed;
        self.following = following;
        if following {
            self.live_time = observed.or(self.live_time);
        }
        false
    }

    fn aligned_active(&mut self, observed: Option<DateTime<Utc>>, following: bool, pending: bool) {
        self.observed = observed;
        self.following = following;
        self.settling = pending;
        if following {
            self.live_time = observed.or(self.live_time);
        }
    }

    pub fn selected_time(&self) -> Option<DateTime<Utc>> {
        match self.cursor? {
            AnalysisCursor::Archive(target) => Some(target),
            AnalysisCursor::Live => self.live_time,
        }
    }

    /// Make a settled radar frame authoritative. This is deliberately separate from
    /// `select_explicit`: the frame has already loaded, so marking it as settling would cause the
    /// next UI pass to treat the same scan as another pending seek.
    fn lock_to_source(&mut self, active: usize, site: Option<&str>, observed: DateTime<Utc>) {
        self.cursor = Some(AnalysisCursor::Archive(observed));
        self.active = Some(active);
        self.site = site.map(str::to_owned);
        self.observed = Some(observed);
        self.following = false;
        self.settling = false;
    }
}

fn pane_selected_time(view: &crate::view::MapView) -> Option<DateTime<Utc>> {
    let timeline = &view.timeline;
    if timeline.following && !timeline.live_looping() {
        view.volume
            .as_ref()
            .map(|volume| volume.time)
            .or_else(|| timeline.current().and_then(|id| id.date_time()))
    } else {
        timeline
            .seek_target
            .or_else(|| timeline.current().and_then(|id| id.date_time()))
            .or_else(|| view.volume.as_ref().map(|volume| volume.time))
    }
}

fn clear_stale_volume(stale_axis: bool, selected: Option<&str>, shown: Option<&str>) -> bool {
    stale_axis || shown.is_some_and(|name| selected != Some(name))
}

fn settled_source_lock(
    enabled: bool,
    retarget_active: bool,
    pending: bool,
    following: bool,
    is_nexrad: bool,
    observed: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    (enabled && !retarget_active && !pending && !following && is_nexrad)
        .then_some(observed)
        .flatten()
}

impl HookEchoApp {
    /// Keep one selected analysis instant while each NEXRAD pane lands on its own nearest scan.
    /// Focus changes retain that instant; scrubbing or playing the focused pane moves it.
    /// Returns whether the focused pane was retargeted and needs another frame to fetch it.
    pub(super) fn sync_linked_pane_times(&mut self) -> bool {
        if !self.link_times || self.views.is_empty() {
            self.linked_analysis = LinkedTimeState::default();
            return false;
        }
        let active = self.active.min(self.views.len() - 1);
        let source = &self.views[active];
        let observed = pane_selected_time(source);
        let pending = source.timeline.seek_target.is_some() || source.timeline.listing;
        let retarget_active = self.linked_analysis.observe(
            active,
            source.site.as_deref(),
            observed,
            source.timeline.following,
            pending,
        );
        if let Some(source_time) = settled_source_lock(
            self.lock_source_time,
            retarget_active,
            pending,
            source.timeline.following,
            source.site.as_deref().is_some_and(wxdata::sites::is_nexrad),
            observed,
        ) {
            self.linked_analysis
                .lock_to_source(active, source.site.as_deref(), source_time);
        }
        let Some(cursor) = self.linked_analysis.cursor else {
            return false;
        };
        for (idx, view) in self.views.iter_mut().enumerate() {
            if (idx == active && !retarget_active)
                || !view.site.as_deref().is_some_and(wxdata::sites::is_nexrad)
            {
                continue;
            }
            if cursor == AnalysisCursor::Live {
                let day = chrono::Utc::now().date_naive();
                if !view.timeline.following || view.timeline.date != day {
                    view.timeline.follow_day(day);
                    view.volume = None;
                    view.loading = false;
                    view.last_poll = None;
                }
            } else if let AnalysisCursor::Archive(target) = cursor {
                let site = view.site.as_deref().unwrap_or_default();
                let stale_axis = view.timeline.seek_to_valid_time(site, target);
                let selected = view.timeline.current().map(|id| id.name());
                let shown = view.volume.as_ref().map(|volume| volume.name.as_str());
                if clear_stale_volume(stale_axis, selected, shown) {
                    view.volume = None;
                    view.loading = false;
                }
            }
        }
        if retarget_active {
            let source = &self.views[active];
            self.linked_analysis.aligned_active(
                pane_selected_time(source),
                source.timeline.following,
                source.timeline.seek_target.is_some() || source.timeline.listing,
            );
        }
        retarget_active
    }

    pub(super) fn linked_analysis_time(&self) -> Option<DateTime<Utc>> {
        self.link_times
            .then(|| self.linked_analysis.selected_time())
            .flatten()
    }

    pub(super) fn linked_archive_time(&self) -> Option<DateTime<Utc>> {
        if !self.link_times {
            return None;
        }
        match self.linked_analysis.cursor {
            Some(AnalysisCursor::Archive(target)) => Some(target),
            _ => None,
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
        let selected = self.linked_analysis_time();
        let tolerance = chrono::Duration::minutes(self.settings.time_mismatch_minutes as i64);
        for (idx, rect) in rects.iter().enumerate() {
            if solo && idx != active {
                continue;
            }
            let view = &self.views[idx];
            let Some(site) = view.site.as_deref() else {
                continue;
            };
            let site_label = if idx == active {
                selected.map_or_else(
                    || site.to_string(),
                    |target| format!("Target {}Z · {site}", target.format("%m-%d %H:%M")),
                )
            } else {
                site.to_string()
            };
            let (caption, warning) = match (&view.volume, selected) {
                (Some(volume), Some(selected)) => {
                    let comparison =
                        wxdata::time_align::TimeOffset::between(volume.time, selected, tolerance);
                    let offset = crate::ui::data_inspector::offset_label(comparison.offset);
                    (
                        format!(
                            "{site_label} · {}Z · Δ{offset}",
                            volume.time.format("%Y-%m-%d %H:%M:%S")
                        ),
                        comparison.outside_tolerance,
                    )
                }
                (Some(volume), None) => (
                    format!(
                        "{site_label} · {}Z",
                        volume.time.format("%Y-%m-%d %H:%M:%S")
                    ),
                    false,
                ),
                (None, _) if view.timeline.listing || view.loading => {
                    (format!("{site_label} · loading scan"), false)
                }
                (None, _) => (format!("{site_label} · no scan for selected time"), false),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn t(minutes: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(minutes * 60, 0).unwrap()
    }

    #[test]
    fn changing_focus_does_not_replace_the_selected_analysis_instant() {
        let mut state = LinkedTimeState::default();
        assert!(!state.observe(0, Some("KTLX"), Some(t(100)), false, false));
        assert_eq!(state.cursor, Some(AnalysisCursor::Archive(t(100))));
        assert!(state.observe(1, Some("KDMX"), Some(t(103)), false, false));
        assert_eq!(state.selected_time(), Some(t(100)));
        state.aligned_active(Some(t(103)), false, false);
        state.observe(1, Some("KDMX"), Some(t(103)), false, false);
        assert_eq!(state.selected_time(), Some(t(100)));
        state.observe(1, Some("KDMX"), Some(t(110)), false, false);
        assert_eq!(state.selected_time(), Some(t(110)));
    }

    #[test]
    fn pending_site_listing_cannot_shift_the_analysis_cursor() {
        let mut state = LinkedTimeState::default();
        state.observe(0, Some("KTLX"), Some(t(100)), false, false);
        assert!(state.observe(0, Some("KDMX"), None, false, true));
        state.aligned_active(None, false, true);
        state.observe(0, Some("KDMX"), None, false, true);
        state.observe(0, Some("KDMX"), Some(t(103)), false, false);
        assert_eq!(state.selected_time(), Some(t(100)));
    }

    #[test]
    fn live_cursor_updates_its_clock_without_pin_drift() {
        let mut state = LinkedTimeState::default();
        state.observe(0, Some("KTLX"), Some(t(100)), true, false);
        assert_eq!(state.cursor, Some(AnalysisCursor::Live));
        state.observe(0, Some("KTLX"), Some(t(105)), true, false);
        assert_eq!(state.selected_time(), Some(t(105)));
        state.observe(0, Some("KTLX"), Some(t(95)), false, false);
        assert_eq!(state.cursor, Some(AnalysisCursor::Archive(t(95))));
    }

    #[test]
    fn explicit_event_jump_replaces_the_target_even_when_the_site_changes() {
        let mut state = LinkedTimeState::default();
        state.observe(0, Some("KTLX"), Some(t(100)), false, false);
        state.select_explicit(0, Some("KDMX"), Some(t(800)));
        state.observe(0, Some("KDMX"), Some(t(800)), false, true);
        state.observe(0, Some("KDMX"), Some(t(803)), false, false);
        assert_eq!(state.selected_time(), Some(t(800)));
    }

    #[test]
    fn satellite_frame_can_drive_radar_alignment_and_return_to_live() {
        let mut state = LinkedTimeState::default();
        state.observe(0, Some("KTLX"), Some(t(100)), false, false);
        state.select_external(Some(t(200)));
        assert!(state.observe(0, Some("KTLX"), Some(t(100)), false, false));
        state.aligned_active(Some(t(203)), false, false);
        assert_eq!(state.selected_time(), Some(t(200)));
        state.select_external(None);
        assert!(state.observe(0, Some("KTLX"), Some(t(203)), false, false));
        state.aligned_active(Some(t(210)), true, false);
        assert_eq!(state.cursor, Some(AnalysisCursor::Live));
        assert_eq!(state.selected_time(), Some(t(210)));
    }

    #[test]
    fn pending_new_frame_is_not_cancelled_each_ui_frame() {
        assert!(!clear_stale_volume(false, Some("new"), None));
        assert!(clear_stale_volume(false, Some("new"), Some("old")));
        assert!(clear_stale_volume(false, None, Some("old")));
        assert!(clear_stale_volume(true, None, None));
    }

    #[test]
    fn source_lock_replaces_an_external_request_with_the_settled_radar_frame() {
        let mut state = LinkedTimeState::default();
        state.select_external(Some(t(200)));
        state.lock_to_source(0, Some("KTLX"), t(203));
        assert_eq!(state.cursor, Some(AnalysisCursor::Archive(t(203))));
        assert_eq!(state.selected_time(), Some(t(203)));
        assert!(!state.settling);
    }

    #[test]
    fn source_lock_waits_for_the_requested_radar_frame_to_settle() {
        assert_eq!(
            settled_source_lock(true, true, false, false, true, Some(t(100))),
            None,
            "the old source frame must not cancel a new external request"
        );
        assert_eq!(
            settled_source_lock(true, false, true, false, true, Some(t(200))),
            None,
            "the requested frame is still loading"
        );
        assert_eq!(
            settled_source_lock(true, false, false, false, true, Some(t(203))),
            Some(t(203))
        );
    }
}
