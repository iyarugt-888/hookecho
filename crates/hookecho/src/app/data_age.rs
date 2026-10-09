//! When data counts as old (ROADMAP_2 §1.1, §9.3): the radar's stale threshold and the layer
//! time-mismatch warning, both the user's to set, and the Preferences rows that set them.

use super::*;

impl HookEchoApp {
    /// How old the newest known radar volume can be before the site counts as stale rather than
    /// "just between scans", in seconds. The Live/Stale timeline badge, the radar's source-health
    /// row and the live-scan phase all read this one number, so they can never disagree (two
    /// hand-picked numbers, 120 s and 900 s, for the same question was exactly that drift). The
    /// live scan reads Aging from 80% of it, so the warning shows before the line is crossed.
    /// The 15-minute default lets NEXRAD's slowest common VCP (clear air, ~10 min between
    /// volumes) read fresh, and a dead feed clears it inside two cycles of even that cadence.
    pub(crate) fn radar_fresh_secs(&self) -> i64 {
        i64::from(self.settings.radar_stale_minutes.clamp(3, 120)) * 60
    }

    /// Preferences → App → Data age.
    pub(crate) fn data_age_rows(&mut self, ui: &mut egui::Ui) {
        ui.label("Radar counts as stale after");
        crate::theme::slider(
            ui,
            egui::Slider::new(&mut self.settings.radar_stale_minutes, 3..=120)
                .suffix(" min")
                .logarithmic(true),
        )
        .on_hover_text(
            "How old the newest scan can be before the radar reads Stale. It reads Aging from \
             80% of this. Clear-air scans come about every 10 minutes, so below that a quiet \
             site will flicker to Aging between volumes.",
        );
        ui.add_space(6.0);
        ui.label("Warn when a layer's time is off the radar's by more than");
        crate::theme::slider(
            ui,
            egui::Slider::new(&mut self.settings.time_mismatch_minutes, 1..=180)
                .suffix(" min")
                .logarithmic(true),
        )
        .on_hover_text(
            "A satellite, model or other layer whose valid time is further than this from the \
             radar scan it is shown with is marked with a warning.",
        );
        ui.add_space(6.0);
        ui.checkbox(
            &mut self.settings.blend_frames,
            "Blend archived MRMS layers to the radar's time",
        )
        .on_hover_text(
            "Show a continuous MRMS layer (reflectivity, rotation, hail size) at the radar \
             scan's own time, interpolated between the frames either side within the threshold \
             above, instead of the nearest frame. Probes and exports say the frame is \
             interpolated. Categories (precipitation type) and accumulations are never blended.",
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_default_threshold_is_the_old_fifteen_minutes() {
        let s = crate::settings::Settings::default();
        assert_eq!(i64::from(s.radar_stale_minutes) * 60, 900);
    }
}
