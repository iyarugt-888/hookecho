//! Scoring an automatic detector against what actually happened.
//!
//! A confidence number is a claim about how often a detection is right, and this is where it is
//! tested: given the detections a detector made and the tornado reports for the same time, how many
//! of the detections were near a real report (and so were not false alarms), and how many of the
//! reports did the detector find? Both are counted at each confidence threshold, so the table shows
//! what a minimum-confidence filter would cost and what it would buy.
//!
//! Pure functions over plain values, so it applies to any detector (debris signatures, rotation
//! couplets) and to any truth set (local storm reports, damage-survey points, warning centroids).
//!
//! Local storm reports are not ground truth for the *instant* of a detection: they are logged when
//! seen or surveyed, and a tornado can be on the ground for a long time. The matching window and
//! radius are therefore arguments, and any score is only as good as those choices and the
//! completeness of the reports. A missed tornado nobody reported counts as a false alarm here.

/// One thing a detector reported.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Detection {
    pub lon: f64,
    pub lat: f64,
    /// The detector's own 0..1 confidence.
    pub confidence: f32,
    /// When it was detected, in minutes since any fixed origin (only differences are used).
    pub minute: i64,
    /// Range from the radar that made this detection, km. Only used by [`score_in_range`], to
    /// ask whether a detection criterion performs consistently by range independent of whatever
    /// the confidence score itself already discounts for range.
    pub range_km: f32,
}

/// One real event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Truth {
    pub lon: f64,
    pub lat: f64,
    /// When it was reported, in the same minutes as [`Detection::minute`].
    pub minute: i64,
}

/// How detections at or above one confidence threshold fared.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    pub threshold: f32,
    /// Detections at or above the threshold.
    pub detections: usize,
    /// Of those, the ones near a real event in space and time.
    pub verified: usize,
    /// Real events.
    pub events: usize,
    /// Real events with a detection at or above the threshold near them.
    pub found: usize,
}

impl Score {
    /// Probability of detection: the share of real events found. `None` with no events.
    pub fn pod(&self) -> Option<f32> {
        (self.events > 0).then(|| self.found as f32 / self.events as f32)
    }

    /// False alarm ratio: the share of detections near no real event. `None` with no detections.
    pub fn far(&self) -> Option<f32> {
        (self.detections > 0)
            .then(|| (self.detections - self.verified) as f32 / self.detections as f32)
    }

    /// Critical success index: found / (found + missed + false alarms). `None` when there is nothing
    /// to count at all.
    pub fn csi(&self) -> Option<f32> {
        let missed = self.events - self.found;
        let false_alarms = self.detections - self.verified;
        let denom = self.found + missed + false_alarms;
        (denom > 0).then(|| self.found as f32 / denom as f32)
    }
}

/// Great-circle distance in km between two lon/lat points.
fn km_between(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, la2) = (a.1.to_radians(), b.1.to_radians());
    let dlat = la2 - la1;
    let dlon = (b.0 - a.0).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * 6371.0 * h.sqrt().asin()
}

/// Slowest and fastest a tornado is taken to move along its surveyed path, km per minute (8 and
/// 30 m/s). A survey records where the path is and when the tornado began, not when it was where,
/// so these bound where on the path it can have been at a given time.
pub const PATH_MIN_SPEED_KM_MIN: f64 = 0.48;
pub const PATH_MAX_SPEED_KM_MIN: f64 = 1.8;

/// A surveyed tornado path as truth (detectionplan.md Phase 10).
///
/// A point report says where a tornado was seen once; a damage survey says everywhere it went.
/// Matching a detection only to a path's midpoint, as a point, throws that away: a radar detection
/// right on a 200 km path, far from its middle, would read as a false alarm. Here a detection
/// verifies when it is within the radius of the stretch of path the tornado can have been on at
/// that time: from where it would be moving at [`PATH_MIN_SPEED_KM_MIN`] to where it would be at
/// [`PATH_MAX_SPEED_KM_MIN`], each widened by the matching window. The path is one event however
/// long it is.
#[derive(Debug, Clone, PartialEq)]
pub struct PathTruth {
    /// Vertices as `(lon, lat)`, in the direction the tornado moved.
    pub path: Vec<(f64, f64)>,
    /// When the tornado began, in the same minutes as [`Detection::minute`].
    pub start_minute: i64,
}

impl PathTruth {
    /// Length along the path, km.
    pub fn length_km(&self) -> f64 {
        self.path.windows(2).map(|w| km_between(w[0], w[1])).sum()
    }

    /// Whether `d` is within `radius_km` of where on the path the tornado can have been at its
    /// time, allowing `window_min` either way.
    pub fn near(&self, d: &Detection, radius_km: f64, window_min: i64) -> bool {
        let Some(&first) = self.path.first() else {
            return false;
        };
        let (dt, w) = ((d.minute - self.start_minute) as f64, window_min as f64);
        if dt + w < 0.0 {
            return false;
        }
        if self.path.len() == 1 {
            return km_between((d.lon, d.lat), first) <= radius_km;
        }
        let len = self.length_km();
        let lo = (dt - w).max(0.0) * PATH_MIN_SPEED_KM_MIN;
        if lo > len {
            // Even moving at its slowest it had reached the end and was over.
            return false;
        }
        let hi = ((dt + w) * PATH_MAX_SPEED_KM_MIN).min(len);
        // Distance to the stretch [lo, hi], segment by segment, on a local flat projection about
        // the detection (paths are short against the earth's curvature).
        let lat0 = d.lat.to_radians();
        let xy = |p: (f64, f64)| ((p.0 - d.lon) * 111.32 * lat0.cos(), (p.1 - d.lat) * 110.57);
        let mut s = 0.0;
        for seg in self.path.windows(2) {
            let l = km_between(seg[0], seg[1]);
            let (a, b) = (s, s + l);
            s = b;
            if l <= 0.0 || b < lo || a > hi {
                continue;
            }
            // The part of this segment inside [lo, hi], as fractions along it.
            let (f0, f1) = (
                ((lo - a) / l).clamp(0.0, 1.0),
                ((hi - a) / l).clamp(0.0, 1.0),
            );
            let (p, q) = (xy(seg[0]), xy(seg[1]));
            let at = |f: f64| (p.0 + (q.0 - p.0) * f, p.1 + (q.1 - p.1) * f);
            let (u, v) = (at(f0), at(f1));
            // Distance from the origin (the detection) to the segment u-v.
            let (dx, dy) = (v.0 - u.0, v.1 - u.1);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 {
                (-(u.0 * dx + u.1 * dy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if (u.0 + dx * t).hypot(u.1 + dy * t) <= radius_km {
                return true;
            }
        }
        false
    }
}

/// [`score`] against point reports and surveyed paths together: each report and each path is one
/// event.
pub fn score_with_paths(
    detections: &[Detection],
    truths: &[Truth],
    paths: &[PathTruth],
    radius_km: f64,
    window_min: i64,
    thresholds: &[f32],
) -> Vec<Score> {
    thresholds
        .iter()
        .map(|&threshold| {
            let kept: Vec<&Detection> = detections
                .iter()
                .filter(|d| d.confidence >= threshold)
                .collect();
            let verified = kept
                .iter()
                .filter(|d| {
                    truths.iter().any(|t| near(d, t, radius_km, window_min))
                        || paths.iter().any(|p| p.near(d, radius_km, window_min))
                })
                .count();
            let found = truths
                .iter()
                .filter(|t| kept.iter().any(|d| near(d, t, radius_km, window_min)))
                .count()
                + paths
                    .iter()
                    .filter(|p| kept.iter().any(|d| p.near(d, radius_km, window_min)))
                    .count();
            Score {
                threshold,
                detections: kept.len(),
                verified,
                events: truths.len() + paths.len(),
                found,
            }
        })
        .collect()
}

/// [`lead_minutes`] against reports and paths together. A path's lead is from its start.
pub fn lead_minutes_with_paths(
    detections: &[Detection],
    truths: &[Truth],
    paths: &[PathTruth],
    radius_km: f64,
    window_min: i64,
    threshold: f32,
) -> Vec<i64> {
    let mut out = lead_minutes(detections, truths, radius_km, window_min, threshold);
    out.extend(paths.iter().filter_map(|p| {
        detections
            .iter()
            .filter(|d| d.confidence >= threshold && p.near(d, radius_km, window_min))
            .map(|d| d.minute)
            .min()
            .map(|first| p.start_minute - first)
    }));
    out
}

/// Whether `d` is within `radius_km` and `window_min` of the point truth `t`.
pub fn near_point(d: &Detection, t: &Truth, radius_km: f64, window_min: i64) -> bool {
    near(d, t, radius_km, window_min)
}

fn near(d: &Detection, t: &Truth, radius_km: f64, window_min: i64) -> bool {
    (d.minute - t.minute).abs() <= window_min
        && km_between((d.lon, d.lat), (t.lon, t.lat)) <= radius_km
}

/// Score `detections` against `truths` at each of `thresholds`. A detection matches an event within
/// `radius_km` and `window_min` minutes of it; one detection can vouch for several events and one
/// event for several detections, which is what a tornado seen in ten consecutive volumes should do.
pub fn score(
    detections: &[Detection],
    truths: &[Truth],
    radius_km: f64,
    window_min: i64,
    thresholds: &[f32],
) -> Vec<Score> {
    thresholds
        .iter()
        .map(|&threshold| {
            let kept: Vec<&Detection> = detections
                .iter()
                .filter(|d| d.confidence >= threshold)
                .collect();
            let verified = kept
                .iter()
                .filter(|d| truths.iter().any(|t| near(d, t, radius_km, window_min)))
                .count();
            let found = truths
                .iter()
                .filter(|t| kept.iter().any(|d| near(d, t, radius_km, window_min)))
                .count();
            Score {
                threshold,
                detections: kept.len(),
                verified,
                events: truths.len(),
                found,
            }
        })
        .collect()
}

/// [`score`], but only over the detections whose `range_km` falls in `[min_km, max_km)`.
///
/// The confidence score already discounts by range (every detector's `range_factor` fades past
/// 60 km), so scoring by confidence threshold alone cannot say whether the *underlying* detection
/// criterion -- a fixed gate-to-gate velocity difference, or a fixed CC/Z threshold -- performs
/// consistently near the radar and far from it, or is quietly miscalibrated by range in a way the
/// scoring then papers over. This asks that question directly, on the raw candidate set.
///
/// `truths` stay global and unbanded: a truth is "found" by any in-band detection near it,
/// wherever the rest of the detections (in or out of the band) happened to fall. One real tornado
/// seen by a near-range detection at one volume and a far-range one at the next is "found" in
/// both bands, which is the right answer to "would this band alone have caught it".
pub fn score_in_range(
    detections: &[Detection],
    truths: &[Truth],
    radius_km: f64,
    window_min: i64,
    min_km: f32,
    max_km: f32,
    thresholds: &[f32],
) -> Vec<Score> {
    let banded: Vec<Detection> = detections
        .iter()
        .copied()
        .filter(|d| d.range_km >= min_km && d.range_km < max_km)
        .collect();
    score(&banded, truths, radius_km, window_min, thresholds)
}

/// Lead time (minutes) for each truth that a detection at or above `threshold` found: the truth's
/// minute less the earliest such detection's near it. Negative when the first detection came after
/// the report. Truths nothing found are left out, so this says how early, not how often.
pub fn lead_minutes(
    detections: &[Detection],
    truths: &[Truth],
    radius_km: f64,
    window_min: i64,
    threshold: f32,
) -> Vec<i64> {
    truths
        .iter()
        .filter_map(|t| {
            detections
                .iter()
                .filter(|d| d.confidence >= threshold && near(d, t, radius_km, window_min))
                .map(|d| d.minute)
                .min()
                .map(|first| t.minute - first)
        })
        .collect()
}

/// The `truths` with no `detections` at or above `threshold` near them — the actual events behind
/// a [`Score`]'s `events - found`, not just the count. An aggregate table over several events
/// (`--headless-backtest-file`, say) can hide *which* report a change in the detector cost or
/// gained; this names it, so a change can be checked against the specific report rather than only
/// the totals moving.
pub fn unmatched(
    detections: &[Detection],
    truths: &[Truth],
    radius_km: f64,
    window_min: i64,
    threshold: f32,
) -> Vec<Truth> {
    let kept: Vec<&Detection> = detections
        .iter()
        .filter(|d| d.confidence >= threshold)
        .collect();
    truths
        .iter()
        .filter(|t| !kept.iter().any(|d| near(d, t, radius_km, window_min)))
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(lon: f64, lat: f64, confidence: f32, minute: i64) -> Detection {
        det_at_range(lon, lat, confidence, minute, 0.0)
    }

    fn det_at_range(lon: f64, lat: f64, confidence: f32, minute: i64, range_km: f32) -> Detection {
        Detection {
            lon,
            lat,
            confidence,
            minute,
            range_km,
        }
    }

    fn truth(lon: f64, lat: f64, minute: i64) -> Truth {
        Truth { lon, lat, minute }
    }

    #[test]
    fn a_detection_on_a_report_is_verified_and_finds_it() {
        let s = score(
            &[det(-97.5, 35.3, 0.8, 100)],
            &[truth(-97.5, 35.3, 100)],
            10.0,
            15,
            &[0.0],
        );
        assert_eq!(
            (s[0].detections, s[0].verified, s[0].events, s[0].found),
            (1, 1, 1, 1)
        );
        assert_eq!(s[0].pod(), Some(1.0));
        assert_eq!(s[0].far(), Some(0.0));
        assert_eq!(s[0].csi(), Some(1.0));
    }

    #[test]
    fn distance_and_time_both_have_to_match() {
        let t = [truth(-97.5, 35.3, 100)];
        // 30 km east: outside a 10 km radius. Same place, an hour later: outside a 15 minute window.
        let far_away = det(-97.5 + 0.33, 35.3, 0.9, 100);
        let too_late = det(-97.5, 35.3, 0.9, 160);
        let s = score(&[far_away, too_late], &t, 10.0, 15, &[0.0]);
        assert_eq!(s[0].verified, 0);
        assert_eq!(s[0].found, 0);
        assert_eq!(s[0].far(), Some(1.0));
        assert_eq!(s[0].pod(), Some(0.0));
        // A wider radius and window take both in.
        let wide = score(&[far_away, too_late], &t, 40.0, 90, &[0.0]);
        assert_eq!(wide[0].verified, 2);
    }

    #[test]
    fn a_higher_threshold_trades_false_alarms_for_missed_events() {
        // A confident hit on the tornado, and a marginal false alarm elsewhere.
        let dets = [det(-97.5, 35.3, 0.9, 100), det(-98.5, 36.0, 0.4, 100)];
        let events = [truth(-97.5, 35.3, 100)];
        let s = score(&dets, &events, 10.0, 15, &[0.0, 0.5, 0.95]);
        assert_eq!(s[0].far(), Some(0.5), "everything shown: half are false");
        assert_eq!(s[1].far(), Some(0.0), "the filter removed the false alarm");
        assert_eq!(s[1].pod(), Some(1.0), "and kept the tornado");
        assert_eq!(s[2].detections, 0, "too strict: nothing left");
        assert_eq!(s[2].pod(), Some(0.0));
        assert_eq!(s[2].far(), None, "no detections, so no ratio to state");
    }

    #[test]
    fn consecutive_detections_of_one_tornado_all_verify_it() {
        let dets: Vec<_> = (0..5).map(|i| det(-97.5, 35.3, 0.8, 100 + i * 5)).collect();
        let s = score(&dets, &[truth(-97.5, 35.3, 110)], 10.0, 15, &[0.0]);
        assert_eq!((s[0].verified, s[0].found), (5, 1));
    }

    #[test]
    fn no_events_or_nothing_detected_give_no_ratio_rather_than_a_fake_one() {
        let s = score(&[], &[], 10.0, 15, &[0.0]);
        assert_eq!((s[0].pod(), s[0].far(), s[0].csi()), (None, None, None));
        let only_events = score(&[], &[truth(0.0, 0.0, 0)], 10.0, 15, &[0.0]);
        assert_eq!(only_events[0].pod(), Some(0.0));
        assert_eq!(only_events[0].csi(), Some(0.0));
    }

    #[test]
    fn csi_counts_hits_misses_and_false_alarms_together() {
        // 1 of 2 events found, and 1 false alarm: 1 / (1 + 1 + 1).
        let dets = [det(-97.5, 35.3, 0.8, 0), det(-90.0, 30.0, 0.8, 0)];
        let events = [truth(-97.5, 35.3, 0), truth(-80.0, 40.0, 0)];
        let s = score(&dets, &events, 10.0, 15, &[0.0]);
        assert!((s[0].csi().unwrap() - 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn the_distance_is_right_across_a_degree() {
        // A degree of latitude is about 111 km.
        assert!((km_between((-97.0, 35.0), (-97.0, 36.0)) - 111.2).abs() < 0.5);
        assert_eq!(km_between((-97.0, 35.0), (-97.0, 35.0)), 0.0);
    }

    #[test]
    fn score_in_range_only_counts_detections_whose_own_range_falls_in_the_band() {
        // A verified near-range hit and a false-alarm far-range one, both at threshold 0.
        let dets = [
            det_at_range(-97.5, 35.3, 0.5, 0, 20.0),
            det_at_range(-90.0, 30.0, 0.5, 0, 120.0),
        ];
        let truths = [truth(-97.5, 35.3, 0)];
        let near = score_in_range(&dets, &truths, 10.0, 15, 0.0, 60.0, &[0.0]);
        assert_eq!(near[0].detections, 1);
        assert_eq!(near[0].far(), Some(0.0), "the near-range hit is real");
        let far = score_in_range(&dets, &truths, 10.0, 15, 60.0, 150.0, &[0.0]);
        assert_eq!(far[0].detections, 1);
        assert_eq!(far[0].far(), Some(1.0), "the far-range one matches nothing");
    }

    #[test]
    fn score_in_range_still_checks_every_truth_not_just_ones_in_the_band() {
        // The only detection near this truth happens to sit outside the queried band.
        let dets = [det_at_range(-97.5, 35.3, 0.5, 0, 120.0)];
        let truths = [truth(-97.5, 35.3, 0)];
        let near = score_in_range(&dets, &truths, 10.0, 15, 0.0, 60.0, &[0.0]);
        assert_eq!(near[0].detections, 0, "no near-range detections at all");
        assert_eq!(
            near[0].events, 1,
            "but the truth itself is not filtered out"
        );
        assert_eq!(near[0].found, 0, "and nothing in this band found it");
    }

    #[test]
    fn an_empty_band_or_no_detections_scores_as_nothing_rather_than_panicking() {
        let dets = [det_at_range(-97.5, 35.3, 0.5, 0, 20.0)];
        let empty = score_in_range(&dets, &[], 10.0, 15, 200.0, 300.0, &[0.0]);
        assert_eq!(empty[0].detections, 0);
        assert_eq!(empty[0].far(), None);
    }

    #[test]
    fn unmatched_names_exactly_the_truths_score_would_have_counted_as_missed() {
        let dets = [det(-97.5, 35.3, 0.8, 100)];
        let found = truth(-97.5, 35.3, 100);
        let missed = truth(-90.0, 30.0, 100);
        let m = unmatched(&dets, &[found, missed], 10.0, 15, 0.0);
        assert_eq!(m, vec![missed], "only the one no detection reached");
        // Agrees with `score`'s own count, not just in spirit.
        let s = score(&dets, &[found, missed], 10.0, 15, &[0.0]);
        assert_eq!(m.len(), s[0].events - s[0].found);
    }

    #[test]
    fn unmatched_respects_the_threshold_like_score_does() {
        let weak = det(-97.5, 35.3, 0.2, 100);
        let t = truth(-97.5, 35.3, 100);
        assert!(unmatched(&[weak], &[t], 10.0, 15, 0.0).is_empty());
        assert_eq!(
            unmatched(&[weak], &[t], 10.0, 15, 0.5),
            vec![t],
            "filtered below the threshold, so nothing is left to match it"
        );
    }

    #[test]
    fn nothing_missed_is_an_empty_list_not_a_placeholder() {
        let dets = [det(-97.5, 35.3, 0.8, 100)];
        assert!(unmatched(&dets, &[truth(-97.5, 35.3, 100)], 10.0, 15, 0.0).is_empty());
        assert!(unmatched(&dets, &[], 10.0, 15, 0.0).is_empty());
    }
}

#[cfg(test)]
mod lead_tests {
    use super::*;

    #[test]
    fn lead_time_is_from_the_earliest_detection_near_each_found_event() {
        let d = |minute: i64, confidence: f32| Detection {
            lon: -97.0,
            lat: 35.0,
            confidence,
            minute,
            range_km: 40.0,
        };
        let t = |minute: i64| Truth {
            lon: -97.01,
            lat: 35.0,
            minute,
        };
        // Detections at 90, 95 and 104 (the first weak); reports at 100 and 300 (nothing near).
        let dets = [d(90, 0.3), d(95, 0.8), d(104, 0.9)];
        let truths = [t(100), t(300)];
        assert_eq!(lead_minutes(&dets, &truths, 10.0, 15, 0.0), vec![10]);
        assert_eq!(lead_minutes(&dets, &truths, 10.0, 15, 0.5), vec![5]);
        // Only the late one qualifies: detected after the report.
        assert_eq!(lead_minutes(&dets, &truths, 10.0, 15, 0.85), vec![-4]);
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;

    /// A path due east from -97.0, 35.0 for `km`, starting at minute 100.
    fn east_path(km: f64) -> PathTruth {
        let dlon = km / (111.32 * 35f64.to_radians().cos());
        PathTruth {
            path: vec![
                (-97.0, 35.0),
                (-97.0 + dlon / 2.0, 35.0),
                (-97.0 + dlon, 35.0),
            ],
            start_minute: 100,
        }
    }

    fn det(km_east: f64, km_north: f64, minute: i64) -> Detection {
        Detection {
            lon: -97.0 + km_east / (111.32 * 35f64.to_radians().cos()),
            lat: 35.0 + km_north / 110.57,
            confidence: 0.9,
            minute,
            range_km: 50.0,
        }
    }

    #[test]
    fn a_detection_far_along_a_long_path_verifies_when_the_tornado_could_be_there() {
        let p = east_path(150.0);
        assert!((p.length_km() - 150.0).abs() < 0.5, "{}", p.length_km());
        // 120 km along, 80 minutes in: 1.5 km/min, inside 0.48..1.8.
        assert!(p.near(&det(120.0, 3.0, 180), 10.0, 15));
        // The midpoint, as the old matching used, is 45 km away from it.
        // Too early: it cannot have got 120 km in 10 minutes.
        assert!(!p.near(&det(120.0, 0.0, 110), 10.0, 15));
        // Too late at the start: moving at its slowest it had passed by long before.
        assert!(!p.near(&det(0.0, 0.0, 400), 10.0, 15));
        // Off to the side.
        assert!(!p.near(&det(60.0, 15.0, 150), 10.0, 15));
        // Before it began, only within the window.
        assert!(p.near(&det(0.0, 0.0, 90), 10.0, 15));
        assert!(!p.near(&det(0.0, 0.0, 80), 10.0, 15));
    }

    #[test]
    fn a_path_is_one_event_however_long() {
        let p = east_path(150.0);
        let dets: Vec<Detection> = (0..10)
            .map(|k| det(10.0 * k as f64, 0.0, 100 + 8 * k))
            .collect();
        let s = score_with_paths(&dets, &[], std::slice::from_ref(&p), 10.0, 15, &[0.0])[0];
        assert_eq!(
            (s.events, s.found, s.detections, s.verified),
            (1, 1, 10, 10)
        );
        assert_eq!(
            lead_minutes_with_paths(&dets, &[], &[p], 10.0, 15, 0.0),
            vec![0]
        );
    }
}
