//! Warnings pushed from the NWS Weather Wire (1008.md A1, increment 2): text products the user's
//! own NWWS-OI relay republishes onto their MQTT broker (`Settings::warnings_topic`), parsed by
//! [`wxdata::nwws`] and merged into the polled alerts by VTEC event, so a warning the feed has
//! not published yet is drawn and announced as soon as the wire carries it, and is drawn once
//! when the feed catches up. With no topic set nothing arrives here and the app behaves exactly
//! as before. Each wire message is noted in its own latency log ([`WireAlerts::latency`]): sent to
//! received here, then to the first frame built after.

use super::*;

/// Wire warnings kept: an outbreak's worth, newest kept when there are more.
#[cfg(not(target_arch = "wasm32"))] // MQTT, and so the wire, is native only.
const MAX_WIRE: usize = 200;

pub(crate) struct WireAlerts {
    features: Vec<GeoFeature>,
    pub(crate) latency: wxdata::alert_latency::LatencyLog,
}

impl Default for WireAlerts {
    fn default() -> Self {
        // Every pushed message is new by construction, so there is no first reply to seed with.
        let mut latency = wxdata::alert_latency::LatencyLog::default();
        latency.observe([], chrono::Utc::now(), "NWWS-OI");
        Self {
            features: Vec::new(),
            latency,
        }
    }
}

impl WireAlerts {
    /// Keep `feats` (parsed from one product), drop what has expired at `now`, and bound the rest.
    #[cfg(not(target_arch = "wasm32"))] // MQTT, and so the wire, is native only.
    fn keep(&mut self, feats: Vec<GeoFeature>, now: chrono::DateTime<chrono::Utc>) {
        self.features.extend(feats);
        self.features.retain(|f| {
            f.alert
                .as_ref()
                .and_then(|a| a.expires)
                .is_none_or(|e| e > now)
        });
        let excess = self.features.len().saturating_sub(MAX_WIRE);
        self.features.drain(..excess);
    }
}

impl HookEchoApp {
    /// One text product off the warnings topic, received at `received`.
    #[cfg(not(target_arch = "wasm32"))] // MQTT, and so the wire, is native only.
    pub(crate) fn accept_wire_warning(
        &mut self,
        text: &str,
        received: chrono::DateTime<chrono::Utc>,
    ) {
        let feats = wxdata::nwws::parse_product(text, received);
        if feats.is_empty() {
            log::debug!("nwws: a product with no warning segment");
            return;
        }
        self.wire.latency.observe(
            feats.iter().filter_map(|f| f.alert.as_ref()),
            received,
            "NWWS-OI",
        );
        self.wire.keep(feats, chrono::Utc::now());
        self.remerge_wire_alerts();
    }

    /// The polled alerts again with the wire's events the feed has not published, announcing
    /// any that are new. A no-op while nothing has come off the wire.
    pub(crate) fn remerge_wire_alerts(&mut self) {
        if self.wire.features.is_empty() {
            return;
        }
        let polled: Vec<GeoFeature> = std::mem::take(&mut self.alert_features)
            .into_iter()
            .filter(|f| !f.alert.as_ref().is_some_and(wxdata::nwws::from_wire))
            .collect();
        let merged = wxdata::nwws::merge(polled, &self.wire.features, chrono::Utc::now());
        self.detect_new_warnings(&merged);
        self.alert_features = merged;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVR: &str = include_str!("../../../wxdata/tests/data/nwws/svr_kdmx_0351.txt");

    #[test]
    fn wire_warnings_expire_and_are_bounded() {
        let mut w = WireAlerts::default();
        let received: chrono::DateTime<chrono::Utc> = "2026-10-09T05:06:30Z".parse().unwrap();
        let one = wxdata::nwws::parse_product(SVR, received);
        w.keep(one.clone(), received);
        assert_eq!(w.features.len(), 1);
        // Past its VTEC end time it is gone.
        w.keep(Vec::new(), "2026-10-09T05:46:00Z".parse().unwrap());
        assert!(w.features.is_empty());
        let many: Vec<GeoFeature> = std::iter::repeat_n(one[0].clone(), MAX_WIRE + 5).collect();
        w.keep(many, received);
        assert_eq!(w.features.len(), MAX_WIRE);
    }
}
