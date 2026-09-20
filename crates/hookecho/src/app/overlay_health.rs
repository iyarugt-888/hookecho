//! Valid-time extraction for the source-health request book (ROADMAP_NEW N1).
//!
//! Fetch completion time is not data valid time. This module only returns timestamps explicitly
//! carried by a decoded product or observation. Feeds whose current app payload discards or never
//! reports a representative time return `None`; the health UI says so instead of presenting the
//! local HTTP completion clock as meteorological provenance.

use super::OverlayMsg;
use chrono::{DateTime, TimeZone, Utc};

fn latest(times: impl Iterator<Item = DateTime<Utc>>) -> Option<DateTime<Utc>> {
    times.max()
}

impl OverlayMsg {
    /// Errors represented as messages because another UI surface needs to consume them. These
    /// still count as request failures and must not establish cache residency merely because the
    /// transport-level `Result` is `Ok`.
    pub(super) fn health_error(&self) -> Option<&str> {
        match self {
            Self::PlacefileError(_, error) => Some(error),
            _ => None,
        }
    }

    /// Newest authoritative valid/observation time carried by this successful result.
    pub(super) fn health_valid_time(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::Fronts(analysis) => analysis.valid,
            Self::Mping(reports) => latest(reports.iter().map(|r| r.time)),
            Self::Pireps(reports) => latest(reports.iter().map(|r| r.time)),
            Self::Recon(observations) => latest(observations.iter().map(|o| o.time)),
            Self::Cells(_, cells) => latest(cells.iter().filter_map(|c| c.time)),
            Self::Field(_, field) => Some(field.time),
            Self::StampedField(_, field) | Self::MrmsField(_, field, _) => {
                Some(field.stamp.valid_time)
            }
            Self::ModelDiff(_, _, _, times) | Self::Compare(_, _, _, _, times) => Some(times.valid),
            Self::Spotters(spotters) => latest(spotters.iter().map(|s| s.time)),
            Self::Hrrr(forecast) => Some(forecast.valid()),
            Self::Wind(field) => Some(field.valid()),
            Self::Obs(_, Ok(station)) => latest(
                station
                    .obs
                    .iter()
                    .filter_map(|observation| observation.time),
            ),
            Self::ArchiveWarnings(bucket, _) => Utc.timestamp_opt(bucket * 300, 0).single(),
            Self::Metar(observations, _) => latest(
                observations
                    .iter()
                    .filter_map(|ob| ob.obs_time)
                    .filter_map(|seconds| Utc.timestamp_opt(seconds, 0).single()),
            ),
            Self::Stations(stations) => latest(stations.iter().filter_map(|station| station.time)),
            Self::Ppef(model) => latest(model.rows.iter().map(|row| row.time)),
            Self::Dat(points, tracks) => latest(
                points
                    .iter()
                    .filter_map(|point| point.storm)
                    .chain(tracks.iter().filter_map(|track| track.storm)),
            ),
            Self::Mosaic(field, _, _) => Some(field.time),
            Self::Contours(_, _, valid) => Some(*valid),

            // These payloads either have no representative timestamp, carry only display text,
            // or expose an expiry/forecast-window end that must not be mislabeled as observation
            // valid time. Preserve `None` until their decoders carry explicit provenance.
            Self::Alerts(_)
            | Self::AlertSeed(_)
            | Self::Outlook(_, _)
            | Self::Mds(_)
            | Self::Watches(_)
            | Self::Wssi(_, _)
            | Self::Ero(_, _)
            | Self::FireWx(_, _)
            | Self::Placefile(_, _)
            | Self::FreezingLevels(_, _)
            | Self::StormReports(_, _)
            | Self::ProbSevere(_)
            | Self::Obs(_, Err(_))
            | Self::Vwp(_, _)
            | Self::Webcams(_)
            | Self::Fires(_, _)
            | Self::Aqi(_)
            | Self::DotCams(_)
            | Self::Mill(_)
            | Self::PlacefileError(_, _)
            | Self::Gauges(_)
            | Self::Tropical(_)
            | Self::Outages(_)
            | Self::Aviation(_)
            | Self::Tfr(_, _) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::FieldLayer;
    use wxdata::mrms::MrmsField;

    fn field(time: DateTime<Utc>) -> MrmsField {
        MrmsField {
            values: vec![1.0],
            nx: 1,
            ny: 1,
            lon_west: -98.0,
            lon_east: -97.0,
            lat_north: 36.0,
            lat_south: 35.0,
            time,
        }
    }

    #[test]
    fn grid_uses_its_data_time_not_the_local_fetch_clock() {
        let valid = Utc.with_ymd_and_hms(2026, 9, 19, 12, 30, 0).unwrap();
        let msg = OverlayMsg::Field(FieldLayer::Mrms, field(valid));
        assert_eq!(msg.health_valid_time(), Some(valid));
    }

    #[test]
    fn observations_report_the_newest_time_in_the_payload() {
        let old = Utc.with_ymd_and_hms(2026, 9, 19, 12, 0, 0).unwrap();
        let new = Utc.with_ymd_and_hms(2026, 9, 19, 12, 5, 0).unwrap();
        let report = |time| wxdata::mping::Report {
            lat: 35.0,
            lon: -97.0,
            time,
            precip: wxdata::mping::Precip::Rain,
            description: String::new(),
        };
        let msg = OverlayMsg::Mping(vec![report(new), report(old)]);
        assert_eq!(msg.health_valid_time(), Some(new));
    }

    #[test]
    fn untimed_feed_does_not_fabricate_a_valid_time() {
        assert_eq!(OverlayMsg::Webcams(Vec::new()).health_valid_time(), None);
    }

    #[test]
    fn plugin_error_message_is_still_a_health_failure() {
        let msg = OverlayMsg::PlacefileError("plugin:test".into(), "command failed".into());
        assert_eq!(msg.health_error(), Some("command failed"));
        assert_eq!(OverlayMsg::Webcams(Vec::new()).health_error(), None);
    }
}
