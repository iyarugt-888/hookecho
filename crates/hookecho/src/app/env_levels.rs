//! Environmental isotherm heights for one radar site and one analysis time: the 0 °C, −10 °C and
//! −20 °C levels the hail grids and user-defined products read, with where they came from.
//!
//! A reading is filed under the `(site, epoch)` it was requested for — `epoch` `None` for the live
//! HRRR analysis while a pane follows the feed, the synoptic launch time of the observed sounding
//! for an archived volume — and handed only to a pane wanting that same pair, so a live analysis
//! never stands in for a past storm's and one site's never for another's.

/// Isotherm heights, metres above sea level, at one radar for one analysis time.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EnvLevels {
    pub site: String,
    pub epoch: Option<chrono::DateTime<chrono::Utc>>,
    pub h0_m: f64,
    pub hm20_m: f64,
    /// `None` when the source had no −10 °C level (never estimated from the other two).
    pub hm10_m: Option<f64>,
    /// "HRRR analysis 06 00Z" or "Norman, OK 20 12Z sounding": what a reader checks.
    pub source: String,
    /// The observed ascent's −10 °C crossings (the lowest is used); `None` for a model analysis,
    /// whose single field reports one height.
    pub hm10_crossings: Option<usize>,
}

impl EnvLevels {
    /// The heights a column product reads, as `f32` metres above sea level.
    pub(crate) fn column_levels(&self) -> wxdata::udp_column::Levels {
        wxdata::udp_column::Levels {
            h0_m: Some(self.h0_m as f32),
            hm10_m: self.hm10_m.map(|h| h as f32),
            hm20_m: Some(self.hm20_m as f32),
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
        format!(
            "{}: 0 °C {:.1} km, {hm10}{multiple}, −20 °C {:.1} km MSL",
            self.source,
            self.h0_m / 1000.0,
            self.hm20_m / 1000.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live() -> EnvLevels {
        EnvLevels {
            site: "KTLX".into(),
            epoch: None,
            h0_m: 4200.0,
            hm20_m: 7600.0,
            hm10_m: None,
            source: "HRRR analysis 06 00Z".into(),
            hm10_crossings: None,
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
}
