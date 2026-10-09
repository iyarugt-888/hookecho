//! How long new alerts take to reach the map (1008.md A1): each alert reply the app accepts is
//! noted on the wall clock against the messages' own `sent` times, and the first frame built
//! after it marks them drawn ([`wxdata::alert_latency`]). The Analyst log shows the stages.

use super::*;

impl HookEchoApp {
    /// A live alert reply was accepted now.
    pub(crate) fn note_alert_receipt(&mut self, feats: &[GeoFeature]) {
        self.alert_latency.observe(
            feats.iter().filter_map(|f| f.alert.as_ref()),
            chrono::Utc::now(),
            "api.weather.gov",
        );
    }

    /// A frame finished building: the messages accepted before it are on the map if the warnings
    /// layer is on and the view shows live alerts rather than an archived bucket.
    pub(crate) fn note_alert_frame(&mut self) {
        let shown = self.filters.show_alerts && self.arch_warn_shown.is_none();
        for log in [&mut self.alert_latency, &mut self.wire.latency] {
            if log.awaiting_draw() {
                log.frame_built(chrono::Utc::now(), shown);
            }
        }
    }

    /// Warnings' sent → in the app and in the app → frame built, for the Analyst log.
    pub(crate) fn alert_latency_summary(&self) -> wxdata::alert_latency::Summary {
        self.alert_latency
            .summary(|s| wxdata::alert_latency::is_warning(&s.event))
    }

    /// The same for warnings pushed from the user's NWWS-OI relay.
    pub(crate) fn wire_latency_summary(&self) -> wxdata::alert_latency::Summary {
        self.wire
            .latency
            .summary(|s| wxdata::alert_latency::is_warning(&s.event))
    }
}
