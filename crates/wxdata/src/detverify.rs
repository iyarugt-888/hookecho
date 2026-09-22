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
}
