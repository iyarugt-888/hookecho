//! Environmental isotherm heights for one radar site and one analysis time: 0 through −40 °C,
//! only where the matched source provides them, with where they came from.
//!
//! A reading is filed under the `(site, epoch)` it was requested for — `epoch` `None` for the live
//! HRRR analysis while a pane follows the feed, the requested synoptic epoch
//! for an archived volume — and handed only to a pane wanting that same pair, so a live analysis
//! never stands in for a past storm's and one site's never for another's.

/// Isotherm heights, metres MSL (recorded RAOB geopotential height for archived scans).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EnvLevels {
    pub site: String,
    pub epoch: Option<chrono::DateTime<chrono::Utc>>,
    pub h0_m: Option<f64>,
    pub hm20_m: Option<f64>,
    /// `None` when the source had no −10 °C level (never estimated from the other two).
    pub hm10_m: Option<f64>,
    pub hm30_m: Option<f64>,
    pub hm40_m: Option<f64>,
    /// "HRRR analysis 06 00Z" or "Norman, OK 20 12Z sounding": what a reader checks.
    pub source: String,
    /// The observed ascent's −10 °C crossings (the lowest is used); `None` for a model analysis,
    /// whose single field reports one height.
    pub hm10_crossings: Option<usize>,
    pub hm30_crossings: Option<usize>,
    pub hm40_crossings: Option<usize>,
}

impl EnvLevels {
    /// The heights a column product reads, as `f32` metres above sea level.
    pub(crate) fn column_levels(&self) -> wxdata::udp_column::Levels {
        wxdata::udp_column::Levels {
            h0_m: self.h0_m.map(|h| h as f32),
            hm10_m: self.hm10_m.map(|h| h as f32),
            hm20_m: self.hm20_m.map(|h| h as f32),
            hm30_m: self.hm30_m.map(|h| h as f32),
            hm40_m: self.hm40_m.map(|h| h as f32),
        }
    }

    /// No radar elevation enters this conversion: source HGHT already has the MSL datum.
    pub(crate) fn observed(
        site: String,
        epoch: chrono::DateTime<chrono::Utc>,
        reading: wxdata::raob::EnvironmentalLevels,
    ) -> Self {
        let l = &reading.isotherms;
        Self {
            site,
            epoch: Some(epoch),
            h0_m: l[0].highest_cooling_height_m.or(l[0].height_m),
            hm20_m: l[2].highest_cooling_height_m.or(l[2].height_m),
            hm10_m: l[1].height_m,
            hm30_m: l[3].height_m,
            hm40_m: l[4].height_m,
            hm10_crossings: Some(l[1].crossings_m.len()),
            hm30_crossings: Some(l[3].crossings_m.len()),
            hm40_crossings: Some(l[4].crossings_m.len()),
            source: format!(
                "{}; 0/-20 C highest cooling crossing (else lowest recorded), -10/-30/-40 C lowest crossing",
                reading.describe()
            ),
        }
    }

    /// Whether this reading answers `site` at `epoch`.
    pub(crate) fn answers(&self, site: &str, epoch: Option<chrono::DateTime<chrono::Utc>>) -> bool {
        self.site == site && self.epoch == epoch
    }

    /// A short provenance line for an inspector or a status row.
    pub(crate) fn describe(&self) -> String {
        let hm10 = self.hm10_m.map_or_else(
            || "−10 °C unavailable".into(),
            |h| format!("−10 °C {:.1} km", h / 1000.0),
        );
        let multiple = match self.hm10_crossings {
            Some(n) if n > 1 => format!(" (lowest of {n} crossings)"),
            _ => String::new(),
        };
        let level = |name: &str, h: Option<f64>, crossings: Option<usize>| {
            let height = h.map_or_else(
                || format!("{name} unavailable"),
                |h| format!("{name} {:.1} km", h / 1000.0),
            );
            match crossings {
                Some(n) if n > 1 => format!("{height} (lowest of {n} crossings)"),
                _ => height,
            }
        };
        format!(
            "{}: {}, {hm10}{multiple}, {}, {}, {} MSL",
            self.source,
            level("0 °C", self.h0_m, None),
            level("−20 °C", self.hm20_m, None),
            level("−30 °C", self.hm30_m, self.hm30_crossings),
            level("−40 °C", self.hm40_m, self.hm40_crossings)
        )
    }
}

use super::*;

/// Isotherm readings filed by `(site, epoch)`, so two panes on different radars or times each keep
/// their own instead of the newest reply standing in for both (roadmaps/radar-analysis R2), and
/// the throttle on asking for them.
#[derive(Default)]
pub(crate) struct EnvCache {
    /// Oldest filed first; at most [`EnvCache::CAPACITY`].
    readings: Vec<EnvLevels>,
    /// When each `(site, epoch)` was last asked for.
    asked: Vec<(Instant, String, Option<chrono::DateTime<chrono::Utc>>)>,
    /// The request in flight. One lane carries one request: a second sent while it is out would
    /// supersede it and its reply would be dropped, so another pair waits for it.
    in_flight: Option<(Instant, String, Option<chrono::DateTime<chrono::Utc>>)>,
}

impl EnvCache {
    /// Readings kept: more than the panes of a full workstation layout, each on its own radar.
    pub(crate) const CAPACITY: usize = 12;
    /// A request not answered in this long no longer holds the lane.
    const IN_FLIGHT_S: u64 = 60;

    /// The reading for `site` at `epoch`, if one was filed.
    pub(crate) fn get(
        &self,
        site: &str,
        epoch: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Option<&EnvLevels> {
        self.readings.iter().find(|l| l.answers(site, epoch))
    }

    /// File a reply under the pair it answers, replacing that pair's older reading; the oldest
    /// other reading goes once the cache is full.
    pub(crate) fn file(&mut self, levels: EnvLevels) {
        if self
            .in_flight
            .as_ref()
            .is_some_and(|(_, s, e)| levels.answers(s, *e))
        {
            self.in_flight = None;
        }
        self.readings
            .retain(|l| !l.answers(&levels.site, levels.epoch));
        self.readings.push(levels);
        if self.readings.len() > Self::CAPACITY {
            self.readings.remove(0);
        }
    }

    /// Whether a request for `site` at `epoch` may go out now: not asked within `cadence`, and no
    /// other pair's request still out on the lane (`lane_busy`, and asked under a minute ago).
    pub(crate) fn may_ask(
        &self,
        site: &str,
        epoch: Option<chrono::DateTime<chrono::Utc>>,
        lane_busy: bool,
        cadence: std::time::Duration,
    ) -> bool {
        let recent = self
            .asked
            .iter()
            .any(|(t, s, e)| s == site && *e == epoch && t.elapsed() < cadence);
        let blocked = lane_busy
            && self.in_flight.as_ref().is_some_and(|(t, s, e)| {
                !(s == site && *e == epoch)
                    && t.elapsed() < std::time::Duration::from_secs(Self::IN_FLIGHT_S)
            });
        !recent && !blocked
    }

    /// Record that a request for `site` at `epoch` went out.
    pub(crate) fn asking(&mut self, site: &str, epoch: Option<chrono::DateTime<chrono::Utc>>) {
        let now = Instant::now();
        self.asked.retain(|(_, s, e)| !(s == site && *e == epoch));
        self.asked.push((now, site.to_string(), epoch));
        if self.asked.len() > Self::CAPACITY {
            self.asked.remove(0);
        }
        self.in_flight = Some((now, site.to_string(), epoch));
    }
}

impl HookEchoApp {
    /// Refresh the melting-level heights the hail grids need, on the environment cadence.
    /// Which melting level view `idx` wants: `None` for the live HRRR analysis while it follows
    /// the feed, or the synoptic launch at or before its volume's scan time when it is scrubbing
    /// the archive — the observed ascent that day, not today's model.
    pub(super) fn freezing_epoch(&self, idx: usize) -> Option<chrono::DateTime<chrono::Utc>> {
        let v = &self.views[idx];
        if v.timeline.following {
            return None;
        }
        v.volume
            .as_ref()
            .map(|vol| wxdata::raob::synoptic_before(vol.time))
    }

    /// `(0 °C, −20 °C)` heights above sea level for view `idx`, only when the cached ones were
    /// fetched for its own site *and* epoch — a live reading must never stand in for an archived
    /// storm's, or one site's for another's.
    pub(super) fn freezing_for(&self, idx: usize) -> Option<(f64, f64)> {
        let site = self.views[idx].site.as_deref()?;
        let epoch = self.freezing_epoch(idx);
        self.freezing
            .get(site, epoch)
            .and_then(|l| l.h0_m.zip(l.hm20_m))
            .filter(|(h0, hm20)| hm20 > h0)
    }

    /// The full isotherm reading for view `idx`, under the same site-and-epoch rule as
    /// [`Self::freezing_for`].
    pub(crate) fn env_levels_for(&self, idx: usize) -> Option<&env_levels::EnvLevels> {
        let site = self.views[idx].site.as_deref()?;
        let epoch = self.freezing_epoch(idx);
        self.freezing.get(site, epoch)
    }

    /// Request the melting level view `idx` wants (see [`Self::freezing_epoch`]). Throttled per
    /// `(site, epoch)`: the live analysis refreshes on the 15-minute environment cadence, and an
    /// archived ascent never changes, so the same cadence only paces retries after a failure.
    /// Another pane's pair waits while one request is out, rather than superseding it.
    pub(super) fn fetch_freezing_levels(&mut self, ctx: &egui::Context, idx: usize) {
        let Some(site) = self.views[idx]
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
        else {
            return;
        };
        let epoch = self.freezing_epoch(idx);
        let lane_busy = self
            .acquisition
            .health(&RequestLane::Feed(FeedSource::FreezingLevels))
            .fetching;
        if !self.freezing.may_ask(
            site.id,
            epoch,
            lane_busy,
            std::time::Duration::from_secs(900),
        ) {
            return;
        }
        self.freezing.asking(site.id, epoch);
        self.spawn_overlay(
            ctx,
            OverlaySource::FreezingLevels {
                site: site.id.to_string(),
                lon: site.longitude as f64,
                lat: site.latitude as f64,
                epoch,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live() -> EnvLevels {
        EnvLevels {
            site: "KTLX".into(),
            epoch: None,
            h0_m: Some(4200.0),
            hm20_m: Some(7600.0),
            hm10_m: None,
            source: "HRRR analysis 06 00Z".into(),
            hm10_crossings: None,
            hm30_m: None,
            hm40_m: None,
            hm30_crossings: None,
            hm40_crossings: None,
        }
    }

    /// Two panes on different radars and times each keep their own reading, a refresh replaces
    /// only its own pair's, and a second pair waits while the first request is out.
    #[test]
    fn readings_are_filed_by_site_and_epoch() {
        let mut cache = EnvCache::default();
        let archived = chrono::DateTime::from_timestamp(1_368_993_600, 0);
        cache.file(live());
        cache.file(EnvLevels {
            site: "KFWS".into(),
            epoch: archived,
            source: "Fort Worth, TX 19 12Z sounding".into(),
            ..live()
        });
        assert_eq!(
            cache.get("KTLX", None).unwrap().source,
            "HRRR analysis 06 00Z"
        );
        assert_eq!(
            cache.get("KFWS", archived).unwrap().source,
            "Fort Worth, TX 19 12Z sounding"
        );
        assert!(cache.get("KFWS", None).is_none(), "live never stands in");
        cache.file(EnvLevels {
            source: "HRRR analysis 06 01Z".into(),
            ..live()
        });
        assert_eq!(
            cache.get("KTLX", None).unwrap().source,
            "HRRR analysis 06 01Z"
        );
        assert!(
            cache.get("KFWS", archived).is_some(),
            "the other pair is kept"
        );

        let quarter = std::time::Duration::from_secs(900);
        assert!(cache.may_ask("KTLX", None, false, quarter));
        cache.asking("KTLX", None);
        assert!(!cache.may_ask("KTLX", None, true, quarter), "throttled");
        assert!(
            !cache.may_ask("KFWS", archived, true, quarter),
            "waits for the request out"
        );
        assert!(
            cache.may_ask("KFWS", archived, false, quarter),
            "a failed request frees the lane"
        );
        cache.file(live());
        assert!(
            cache.may_ask("KFWS", archived, true, quarter),
            "an answered request frees it"
        );

        for i in 0..EnvCache::CAPACITY + 3 {
            cache.file(EnvLevels {
                site: format!("K{i:03}"),
                ..live()
            });
        }
        assert_eq!(cache.readings.len(), EnvCache::CAPACITY, "bounded");
        assert!(cache.get("K000", None).is_none(), "the oldest went first");
    }

    #[test]
    fn a_live_reading_answers_only_its_own_site_and_live_epoch() {
        let l = live();
        assert!(l.answers("KTLX", None));
        assert!(!l.answers("KFWS", None));
        let archived = chrono::DateTime::from_timestamp(1_368_993_600, 0);
        assert!(
            !l.answers("KTLX", archived),
            "never stands in for a past storm"
        );
    }

    #[test]
    fn an_absent_minus_10_level_stays_absent() {
        let l = live();
        assert_eq!(l.column_levels().hm10_m, None);
        assert_eq!(l.column_levels().h0_m, Some(4200.0));
        assert!(
            l.describe().contains("−10 °C unavailable"),
            "{}",
            l.describe()
        );
        let snd = EnvLevels {
            hm10_m: Some(5600.0),
            hm10_crossings: Some(2),
            ..l
        };
        assert!(snd.describe().contains("lowest of 2"), "{}", snd.describe());
    }

    #[test]
    fn observed_environment_retains_recorded_msl_datum_and_partial_levels() {
        let station = *wxdata::raob::STATIONS
            .iter()
            .find(|s| s.id == "72469")
            .unwrap();
        let epoch = "2017-05-08T12:00:00Z".parse().unwrap();
        let reading = wxdata::raob::EnvironmentalLevels::from_table(
            station,
            epoch,
            include_str!(
                "../../../../docs/certification/m3.3/recorded-environment/72469-2017050812.txt"
            ),
        );
        let l = EnvLevels::observed("KFTG".into(), epoch, reading);
        assert_eq!(
            l.h0_m,
            Some(4130.5),
            "recorded HGHT, no radar/launch elevation offset"
        );
        assert_eq!(l.hm10_m, Some(5411.0));
        assert!((l.hm30_m.unwrap() - 7922.473282442748).abs() < 1e-8);
        assert!(l.source.contains("table SHA256 bd47579b"));
        assert!(l.answers("KFTG", Some(epoch)));
        assert!(!l.answers("KFTG", None));
        assert!(!l.answers("KTLX", Some(epoch)));
        let table = format!(
            "{:7.1}{:7.1}{:7.1}\n{:7.1}{:7.1}{:7.1}\n",
            400.0, 7000.0, -25.0, 300.0, 9000.0, -45.0
        );
        let reading = wxdata::raob::EnvironmentalLevels::from_table(station, epoch, &table);
        let partial = EnvLevels::observed("KFTG".into(), epoch, reading);
        assert_eq!(
            (partial.h0_m, partial.hm10_m, partial.hm20_m),
            (None, None, None)
        );
        assert_eq!(
            (partial.hm30_m, partial.hm40_m),
            (Some(7500.0), Some(8500.0))
        );
        assert!(partial.describe().contains("0 °C unavailable"));
        assert!(partial.describe().contains("−30 °C 7.5 km"));
        assert_eq!((live().hm30_m, live().hm40_m), (None, None));
    }
}
