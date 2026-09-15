//! GOES frame-time selection and the shared radar/satellite scrub control.
use super::*;
use crate::ui::a11y::Named as _;

/// The GOES frame closest to `t`, or `None` when the nearest one is too far off to be the same
/// weather (or there are no frames yet).
///
/// GIBS geostationary layers are 10-minute imagery; half an hour of tolerance covers a gap in the
/// index or a radar volume from a slow VCP without ever pairing a scan with imagery from another
/// part of the day. `None` means "leave it on the latest", which is the pre-existing behaviour.
pub(super) fn nearest_goes(
    times: &[chrono::DateTime<chrono::Utc>],
    t: chrono::DateTime<chrono::Utc>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    const TOLERANCE_MIN: i64 = 30;
    let frames: Vec<_> = times
        .iter()
        .copied()
        .map(|valid| wxdata::time_align::FrameTime { valid, run: None })
        .collect();
    let index = wxdata::time_align::select(
        &frames,
        t,
        wxdata::time_align::TimePolicy::Nearest,
        Some(chrono::Duration::minutes(TOLERANCE_MIN)),
        wxdata::field::ValueKind::Scalar,
    )?
    .single_index()?;
    Some(times[index])
}

fn offset_label(
    analysis: chrono::DateTime<chrono::Utc>,
    imagery: chrono::DateTime<chrono::Utc>,
    tolerance: chrono::Duration,
) -> String {
    let comparison = wxdata::time_align::TimeOffset::between(imagery, analysis, tolerance);
    let offset = crate::ui::data_inspector::offset_label(comparison.offset);
    if comparison.outside_tolerance {
        format!("⚠ {offset}")
    } else {
        format!("Δ{offset}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mismatch_readout_names_the_exact_source_offset() {
        let radar = chrono::DateTime::from_timestamp(1_000_000, 0).unwrap();
        let tolerance = chrono::Duration::minutes(10);
        assert_eq!(
            offset_label(radar, radar + tolerance, tolerance),
            "Δ+10m 00s"
        );
        assert_eq!(
            offset_label(
                radar,
                radar + tolerance + chrono::Duration::seconds(1),
                tolerance
            ),
            "⚠ +10m 01s"
        );
        assert_eq!(
            offset_label(
                radar,
                radar - tolerance - chrono::Duration::seconds(1),
                tolerance
            ),
            "⚠ -10m 01s"
        );
    }
}

impl HookEchoApp {
    /// Sub-hourly frame scrub bar: shown when the active basemap has a time dimension (GOES
    /// imagery, a WMS radar composite) and its frame times are loaded. Steps through the recent
    /// frames; "Latest" pins to the newest.
    pub(super) fn goes_time_bar(&mut self, ctx: &egui::Context) {
        let active_is_timed = self.views[self.active].basemap.timed();
        if !active_is_timed || self.goes_times.is_empty() {
            return;
        }
        // While following, the readout is whichever frame the selected analysis clock picked.
        let followed = self.goes_follow_radar.then(|| {
            self.linked_analysis_time()
                .or_else(|| self.views[self.active].volume.as_ref().map(|v| v.time))
                .and_then(|t| nearest_goes(&self.goes_times, t))
        });
        egui::Area::new(egui::Id::new("goes_time_bar"))
            .constrain_to(self.chrome_rect)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -34.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let n = self.goes_times.len();
                        // Effective index (None = latest = n-1).
                        let cur = match followed.flatten() {
                            Some(t) => self
                                .goes_times
                                .iter()
                                .position(|x| *x == t)
                                .unwrap_or(n - 1),
                            None if self.goes_follow_radar => n - 1,
                            None => self.goes_time_idx.unwrap_or(n - 1),
                        };
                        ui.label("🛰 GOES:");
                        if ui
                            .selectable_label(
                                self.goes_follow_radar,
                                if self.link_times { "⟲ analysis time" } else { "⟲ radar time" },
                            )
                            .on_hover_text(if self.link_times {
                                "Keep satellite imagery on the selected analysis time. While selected, the arrows step that shared time and linked radar panes follow. If no frame is within 30 minutes, the latest indexed frame is shown with a time-offset warning."
                            } else {
                                "Keep the satellite on the radar's clock — scrub the timeline and the imagery follows. If no frame is within 30 minutes, the latest indexed frame is shown with a time-offset warning."
                            })
                            .clicked()
                        {
                            self.goes_follow_radar = !self.goes_follow_radar;
                            self.goes_time_idx = None;
                        }
                        if ui
                            .add_enabled(
                                cur > 0,
                                egui::Button::new(egui_phosphor::regular::CARET_LEFT),
                            )
                            .named("Previous satellite frame")
                            .clicked()
                        {
                            let previous = cur.saturating_sub(1);
                            if self.link_times && self.goes_follow_radar {
                                self.linked_analysis
                                    .select_external(Some(self.goes_times[previous]));
                            } else {
                                self.goes_follow_radar = false;
                                self.goes_time_idx = Some(previous);
                            }
                        }
                        let label = crate::timefmt::fmt_clock(
                            self.goes_times[cur],
                            self.active_tz(),
                            false,
                        );
                        ui.monospace(label);
                        if let Some(analysis_time) = self.linked_analysis_time().or_else(|| self.views[self.active].volume.as_ref().map(|volume| volume.time)) {
                            let tolerance = chrono::Duration::minutes(self.settings.time_mismatch_minutes as i64);
                            ui.label(offset_label(analysis_time, self.goes_times[cur], tolerance))
                                .on_hover_text(format!("GOES image valid time minus the selected analysis time; ⚠ means outside your {}-minute layer time warning threshold.", self.settings.time_mismatch_minutes));
                        }
                        if ui
                            .add_enabled(
                                cur + 1 < n,
                                egui::Button::new(egui_phosphor::regular::CARET_RIGHT),
                            )
                            .named("Next satellite frame")
                            .clicked()
                        {
                            let ni = cur + 1;
                            if self.link_times && self.goes_follow_radar {
                                self.linked_analysis.select_external(Some(self.goes_times[ni]));
                            } else {
                                self.goes_follow_radar = false;
                                self.goes_time_idx = if ni >= n - 1 { None } else { Some(ni) };
                            }
                        }
                        if ui
                            .add_enabled(
                                self.goes_time_idx.is_some() || self.goes_follow_radar,
                                egui::Button::new("Latest"),
                            )
                            .clicked()
                        {
                            if self.link_times && self.goes_follow_radar {
                                self.linked_analysis.select_external(None);
                            } else {
                                self.goes_follow_radar = false;
                                self.goes_time_idx = None;
                            }
                        }
                    });
                });
            });
    }
}
