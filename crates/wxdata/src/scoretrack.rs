//! Confidence-over-time for one recurring detection (TDS or rotation), tracked the same greedy
//! nearest-neighbour way [`crate::celltrack`] tracks storm cells from one volume to the next.
//!
//! This is the "timeline of score changes" the algorithm-lab roadmap item (C5) asks for: not a
//! new detector, but a history of what an existing one already said about the same feature as it
//! persisted across a volume's worth of scans. A confidence that climbed steadily over four
//! volumes and a confidence that spiked once and vanished are the same single-volume number with
//! very different stories behind them, and only a tracked history can tell them apart.
//!
//! Deliberately the same shape as `celltrack::Track`/`associate`, and not built on top of it:
//! `celltrack::Track` carries motion (direction, speed, extrapolation) that a score timeline has
//! no use for, and its `Blob` has no confidence field to carry. Duplicating the (small) greedy
//! nearest-neighbour loop keeps each module's job legible on its own rather than threading a
//! shared abstraction through two things that only resemble each other by coincidence of both
//! being "the same physical feature, tracked frame to frame".

use chrono::{DateTime, Utc};

/// One volume's reading of a tracked detection: where it was and how confident the detector was,
/// at the instant that volume was scanned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScorePoint {
    pub lon: f64,
    pub lat: f64,
    pub confidence: f32,
    pub time: DateTime<Utc>,
}

/// One detection followed across volumes, oldest point first.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScoreTrack {
    pub points: Vec<ScorePoint>,
}

impl ScoreTrack {
    /// The confidence at the most recent volume. `None` only for a track with no points, which
    /// [`associate`] never produces.
    pub fn latest(&self) -> Option<f32> {
        self.points.last().map(|p| p.confidence)
    }

    /// The strongest confidence this detection has reached over its whole history.
    pub fn peak(&self) -> Option<f32> {
        self.points
            .iter()
            .map(|p| p.confidence)
            .fold(None, |m: Option<f32>, c| Some(m.map_or(c, |m| m.max(c))))
    }

    /// How the confidence moved from the previous volume to the latest one; positive is rising,
    /// negative fading. `None` before a second point exists to compare against.
    pub fn trend(&self) -> Option<f32> {
        let n = self.points.len();
        (n >= 2).then(|| self.points[n - 1].confidence - self.points[n - 2].confidence)
    }
}

/// Great-circle distance in km.
fn haversine_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dlon, dlat) = ((b.0 - a.0).to_radians(), (b.1 - a.1).to_radians());
    let h = (dlat / 2.0).sin().powi(2)
        + a.1.to_radians().cos() * b.1.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    2.0 * 6371.0 * h.sqrt().asin()
}

/// A detection can move at most this fast (km/min) and still be read as the same one from one
/// volume to the next. A tornadic circulation translates with its parent storm, rarely much past
/// highway speed (~30-40 mph); this is a generous ceiling above the fastest documented supercell
/// motion, wide enough that an ordinary storm's own centroid jitter never breaks the track.
const GATE_KM_PER_MIN: f64 = 3.0;

/// A gap wider than this (km) is never bridged, however long the interval between volumes —
/// otherwise a couplet that went quiet for an hour and an unrelated new one two counties over
/// would read as one implausibly fast-moving track.
const MAX_GATE_KM: f64 = 30.0;

/// Extend `prev` with the detections seen at `now`, and start new tracks for the ones that match
/// nothing. Same greedy nearest-neighbour shape as [`crate::celltrack::associate`]: each new
/// point claims the closest still-open track within a gate that widens with the elapsed time (so
/// a skipped volume does not break the track) but never past [`MAX_GATE_KM`].
///
/// Tracks that match nothing this round are returned unchanged, not dropped — a debris ball or a
/// couplet can vanish for one noisy volume and reappear on the next, and it is the caller's call
/// how long a quiet track stays worth keeping around before it counts as gone.
///
/// `now`'s points must share one volume's timestamp for this call to behave as documented; points
/// at or before an existing track's last time never extend it (`dt_min <= 0.0` below), so calling
/// this with an out-of-order or duplicate timestamp is silently a no-op for that point rather than
/// corrupting the track — it simply starts a new one.
pub fn associate(prev: &[ScoreTrack], now: &[ScorePoint]) -> Vec<ScoreTrack> {
    let mut out = prev.to_vec();
    let mut taken = vec![false; out.len()];
    // Only ever searched against the tracks `prev` already had, not ones this call itself starts:
    // two detections seen for the first time together in the same volume are two distinct
    // features, not one recurring track, however close together they landed. It also keeps
    // `taken`'s indices in bounds — see `celltrack::associate`'s matching comment, where this
    // exact shape (search the live, growing `out`, but size `taken` once up front) used to panic
    // on the third-plus new point in one call.
    let prev_len = out.len();
    for &point in now {
        let mut best: Option<(usize, f64)> = None;
        for (i, tr) in out[..prev_len].iter().enumerate() {
            if taken[i] {
                continue;
            }
            let Some(last) = tr.points.last() else {
                continue;
            };
            let dt_min = (point.time - last.time).num_seconds() as f64 / 60.0;
            if dt_min <= 0.0 {
                continue; // same volume as the track's last point, or arriving out of order
            }
            let gate = (dt_min * GATE_KM_PER_MIN).min(MAX_GATE_KM);
            let d = haversine_km((last.lon, last.lat), (point.lon, point.lat));
            if d <= gate && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        match best {
            Some((i, _)) => {
                taken[i] = true;
                out[i].points.push(point);
            }
            None => out.push(ScoreTrack {
                points: vec![point],
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t0: DateTime<Utc>, min: i64, lon: f64, lat: f64, confidence: f32) -> ScorePoint {
        ScorePoint {
            lon,
            lat,
            confidence,
            time: t0 + chrono::Duration::minutes(min),
        }
    }

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }

    #[test]
    fn two_volumes_close_in_space_and_time_extend_one_track() {
        let t0 = t0();
        let tracks = associate(&[], &[at(t0, 0, -97.5, 35.3, 0.4)]);
        let tracks = associate(&tracks, &[at(t0, 5, -97.48, 35.31, 0.6)]);
        assert_eq!(tracks.len(), 1, "the second point extended the first track");
        assert_eq!(tracks[0].points.len(), 2);
        assert_eq!(tracks[0].latest(), Some(0.6));
        assert_eq!(tracks[0].peak(), Some(0.6));
        assert!((tracks[0].trend().unwrap() - 0.2).abs() < 1e-6);
    }

    #[test]
    fn a_far_detection_starts_its_own_track() {
        let t0 = t0();
        let tracks = associate(&[], &[at(t0, 0, -97.5, 35.3, 0.5)]);
        let tracks = associate(&tracks, &[at(t0, 5, -95.0, 35.3, 0.5)]);
        assert_eq!(
            tracks.len(),
            2,
            "far enough away to be a different storm entirely"
        );
    }

    /// The very first volume of a session, with no `prev` tracks at all and several brand-new
    /// detections at once — every session's first tracked volume with more than two hits. See
    /// `celltrack::associate`'s matching regression test for the exact shape of the bug this
    /// guards against.
    #[test]
    fn three_brand_new_detections_in_one_call_do_not_panic() {
        let t0 = t0();
        let tracks = associate(
            &[],
            &[
                at(t0, 0, -97.5, 35.3, 0.4),
                at(t0, 0, -96.0, 35.3, 0.5),
                at(t0, 0, -94.5, 35.3, 0.6),
            ],
        );
        assert_eq!(
            tracks.len(),
            3,
            "three separate detections, none of them a match for another"
        );
    }

    #[test]
    fn a_gap_wider_than_the_cap_is_never_bridged_however_long_the_wait() {
        let t0 = t0();
        let tracks = associate(&[], &[at(t0, 0, -97.5, 35.3, 0.5)]);
        // An hour later, comfortably enough time under the per-minute gate alone to cover any
        // distance, but MAX_GATE_KM still refuses to call it the same detection.
        let far = at(t0, 60, -97.5 + 1.0, 35.3, 0.5); // ~91 km east
        let tracks = associate(&tracks, &[far]);
        assert_eq!(tracks.len(), 2, "too far even given the time to get there");
    }

    #[test]
    fn unmatched_tracks_are_kept_not_dropped() {
        let t0 = t0();
        let tracks = associate(&[], &[at(t0, 0, -97.5, 35.3, 0.5)]);
        // Nothing seen nearby this round -- the existing track must still come back unchanged,
        // so a caller can decide for itself whether a quiet volume means "gone".
        let tracks = associate(&tracks, &[]);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].points.len(), 1);
    }

    #[test]
    fn points_at_or_before_the_last_time_start_a_new_track_instead_of_corrupting_it() {
        let t0 = t0();
        let tracks = associate(&[], &[at(t0, 5, -97.5, 35.3, 0.5)]);
        // Same instant as the existing point: not a later volume, so it cannot extend the track.
        let tracks = associate(&tracks, &[at(t0, 5, -97.5, 35.3, 0.6)]);
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].points.len(), 1, "the original track is untouched");
    }

    #[test]
    fn a_wandering_multi_volume_history_stays_one_track() {
        let t0 = t0();
        let mut tracks = associate(&[], &[at(t0, 0, -97.50, 35.30, 0.35)]);
        for (min, lon, lat, conf) in [
            (5, -97.48, 35.31, 0.45),
            (10, -97.46, 35.33, 0.55),
            (15, -97.44, 35.34, 0.70),
        ] {
            tracks = associate(&tracks, &[at(t0, min, lon, lat, conf)]);
        }
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].points.len(), 4);
        assert_eq!(tracks[0].peak(), Some(0.70));
        assert!((tracks[0].trend().unwrap() - 0.15).abs() < 1e-6);
    }
}
