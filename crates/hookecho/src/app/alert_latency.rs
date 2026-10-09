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

    /// The 30 s polygon poll's warnings (1008.md A1): any the map does not hold yet are added
    /// and announced at once, rather than at the next full refresh, and deduplicated against the
    /// wire by VTEC event. Their latency is noted once the full feed has seeded the log, so a
    /// zone alert the full feed brings later is never mistaken for a new message.
    pub(crate) fn accept_warning_polygons(&mut self, feats: Vec<GeoFeature>) {
        let new = unheld(&self.alert_features, feats);
        if new.is_empty() {
            return;
        }
        if self.alert_latency.is_seeded() {
            self.alert_latency.observe(
                new.iter().filter_map(|f| f.alert.as_ref()),
                chrono::Utc::now(),
                "api.weather.gov (30 s warning poll)",
            );
        }
        self.detect_new_warnings(&new);
        self.alert_features.extend(new);
        self.remerge_wire_alerts();
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

/// The features of `feats` whose alert message the map does not hold yet (every polygon part of
/// a new message, none of a held one's).
fn unheld(held: &[GeoFeature], feats: Vec<GeoFeature>) -> Vec<GeoFeature> {
    let ids: std::collections::HashSet<&str> = held
        .iter()
        .filter_map(|f| f.alert.as_ref().map(|a| a.id.as_str()))
        .collect();
    feats
        .into_iter()
        .filter(|f| {
            f.alert
                .as_ref()
                .is_some_and(|a| !ids.contains(a.id.as_str()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_messages_the_map_does_not_hold_are_new() {
        let json = r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"MultiPolygon","coordinates":[
                [[[-98,35],[-97,35],[-97,36],[-98,35]]],[[[-96,35],[-95,35],[-95,36],[-96,35]]]]},
             "properties":{"id":"urn:new","event":"Tornado Warning","sent":"2026-10-09T05:06:00Z"}},
            {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[-98,35],[-97,35],[-97,36],[-98,35]]]},
             "properties":{"id":"urn:held","event":"Severe Thunderstorm Warning"}}]}"#;
        let feats = wxdata::alerts::parse_alerts(json).unwrap();
        let held: Vec<GeoFeature> = feats
            .iter()
            .filter(|f| f.alert.as_ref().is_some_and(|a| a.id == "urn:held"))
            .cloned()
            .collect();
        let new = unheld(&held, feats);
        assert_eq!(new.len(), 2, "both parts of the new message");
        assert!(new
            .iter()
            .all(|f| f.alert.as_ref().unwrap().id == "urn:new"));
        assert!(unheld(&new, new.clone()).is_empty());
    }
}
