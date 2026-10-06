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
            .as_ref()
            .filter(|l| l.answers(site, epoch))
            .and_then(|l| l.h0_m.zip(l.hm20_m))
            .filter(|(h0, hm20)| hm20 > h0)
    }

    /// The full isotherm reading for view `idx`, under the same site-and-epoch rule as
    /// [`Self::freezing_for`].
    pub(crate) fn env_levels_for(&self, idx: usize) -> Option<&env_levels::EnvLevels> {
        let site = self.views[idx].site.as_deref()?;
        let epoch = self.freezing_epoch(idx);
        self.freezing.as_ref().filter(|l| l.answers(site, epoch))
    }

    /// Request the melting level view `idx` wants (see [`Self::freezing_epoch`]). Throttled per
    /// `(site, epoch)`: the live analysis refreshes on the 15-minute environment cadence, and an
    /// archived ascent never changes, so the same cadence only paces retries after a failure.
    pub(super) fn fetch_freezing_levels(&mut self, ctx: &egui::Context, idx: usize) {
        let Some(site) = self.views[idx]
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
        else {
            return;
        };
        let epoch = self.freezing_epoch(idx);
        if self
            .freezing_last_fetch
            .as_ref()
            .is_some_and(|(t, s, e)| s == site.id && *e == epoch && t.elapsed().as_secs() < 900)
        {
            return;
        }
        self.freezing_last_fetch = Some((Instant::now(), site.id.to_string(), epoch));
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
