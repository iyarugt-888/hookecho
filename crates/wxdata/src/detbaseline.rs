//! Detector baseline export (detectionplan.md Phase 0): every candidate a detector produced on the
//! archived backtest corpus, one row each with the physical measurements behind it, plus the
//! aggregate statistics later detector changes are compared against.
//!
//! This only records. It changes no detector and no score, so the same corpus and the same code
//! give the same rows and the same summary, run after run: rows are sorted, maps are ordered, and
//! nothing here reads a clock.
//!
//! Scores are written as the detectors report them (0..1). They are *evidence scores*, not
//! calibrated probabilities; the reliability table is how far each score bin actually verified,
//! which is the first thing a later calibration needs.

use crate::detverify::{score_with_paths, Detection, PathTruth, Truth};
use serde::Serialize;
use std::collections::BTreeMap;

/// Which detector made a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectorKind {
    /// A velocity couplet (`rotation`).
    Rotation,
    /// A debris signature (`tds`).
    Debris,
    /// A Tornado ID circulation (`tornado_id::circulations`), fused from the two above.
    TornadoId,
    /// A MEHS/POSH hail core (`derived::hail_cores`).
    Hail,
    /// An LLSD rotation column (`rotation_columns`): credible `rotation_objects` on each velocity
    /// tilt, associated up through the tilts, run beside the legacy couplets for comparison
    /// (detectionplan.md Phase 13). Its position is its lowest member's. It has no evidence score
    /// yet (Phase 7): its score is a strength index, peak AzShear in units of 0.02 s⁻¹, capped at
    /// 1, so the threshold table reads 0.002 s⁻¹ steps.
    RotationLlsd,
    /// Each debris signature classified (`debris_class`, detectionplan.md Phase 6): `tier` is
    /// its class, the score its own polarimetric evidence, `members` how many hail signs it carries
    /// and `azshear_s` the low-level shear of the column that promoted it, if one did.
    DebrisClass,
    /// Each LLSD column fused (`tornado_fusion`, detectionplan.md Phase 7): the score is its
    /// fused evidence score from the current weights, and `features` the inputs, for fitting.
    TornadoFusion,
}

impl DetectorKind {
    pub const ALL: [DetectorKind; 7] = [
        DetectorKind::Rotation,
        DetectorKind::Debris,
        DetectorKind::TornadoId,
        DetectorKind::Hail,
        DetectorKind::RotationLlsd,
        DetectorKind::DebrisClass,
        DetectorKind::TornadoFusion,
    ];

    pub fn name(self) -> &'static str {
        match self {
            DetectorKind::Rotation => "rotation",
            DetectorKind::Debris => "debris",
            DetectorKind::TornadoId => "tornado_id",
            DetectorKind::Hail => "hail",
            DetectorKind::RotationLlsd => "rotation_llsd",
            DetectorKind::DebrisClass => "debris_class",
            DetectorKind::TornadoFusion => "tornado_fusion",
        }
    }

    /// Whether this detector is scored against tornado truth (reports and surveys) rather than
    /// hail reports.
    pub fn is_tornado(self) -> bool {
        !matches!(self, DetectorKind::Hail)
    }
}

/// One candidate, with everything the detector measured. A field a detector does not have is
/// `None`, never zero.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Candidate {
    /// The backtest event ("KTLX 2013-05-20 19:50").
    pub event: String,
    pub site: String,
    /// The volume's scan time, RFC 3339.
    pub volume: String,
    /// The volume's minute (Unix minutes), the unit truth matching uses.
    pub minute: i64,
    pub detector: DetectorKind,
    pub lon: f64,
    pub lat: f64,
    pub range_km: f32,
    /// Beam-centre heights (km above radar level) of the lowest and highest contributing tilt.
    pub beam_base_km: Option<f32>,
    pub beam_top_km: Option<f32>,
    /// The detector's own score before any corroboration from another detector.
    pub raw_score: f32,
    /// The score the app shows: after corroboration (rotation, debris) or fusion (Tornado ID).
    pub final_score: f32,
    pub gates: Option<usize>,
    pub tilts: Option<usize>,
    pub vrot_ms: Option<f32>,
    pub g2g_ms: Option<f32>,
    pub min_cc: Option<f32>,
    pub mean_cc: Option<f32>,
    pub mean_z: Option<f32>,
    pub max_z: Option<f32>,
    pub zdr_db: Option<f32>,
    /// Vertical extent of the contributing tilts, `beam_top_km - beam_base_km`.
    pub depth_km: Option<f32>,
    pub rooted: Option<bool>,
    /// "cyclonic" / "anticyclonic", for rotation.
    pub sense: Option<String>,
    /// Tornado ID's tier, and how many rotation / debris detections it fused.
    pub tier: Option<String>,
    pub members: Option<usize>,
    /// Peak sense-adjusted LLSD azimuthal shear, s⁻¹ (LLSD rotation objects).
    pub azshear_s: Option<f32>,
    /// The LLSD column's track (`rotation_tracks`), volumes it has been seen in (1 for a new
    /// track), and its peak AzShear trend (s⁻¹ per 10 minutes, `None` for a new track).
    pub track_id: Option<u64>,
    pub track_age_volumes: Option<usize>,
    pub azshear_trend: Option<f32>,
    /// The fusion's features, in `tornado_fusion::FEATURE_NAMES` order (fused candidates only).
    pub features: Option<Vec<f32>>,
    /// Inside a tornado warning marked observed at its own volume. Recorded, never scored: an
    /// ordinary warning is issued from the same radar signatures.
    pub observed_warning: bool,
    /// Near a local storm report (tornado, or severe hail for the hail detector).
    pub matched_report: bool,
    /// Near a Damage Assessment Toolkit surveyed track (tornado detectors only).
    pub matched_survey: bool,
}

impl Candidate {
    /// Matched to any truth.
    pub fn verified(&self) -> bool {
        self.matched_report || self.matched_survey
    }

    fn detection(&self) -> Detection {
        Detection {
            lon: self.lon,
            lat: self.lat,
            confidence: self.final_score,
            minute: self.minute,
            range_km: self.range_km,
        }
    }
}

/// A detector's truth in one event: point reports, and surveyed paths.
type TruthSet = (Vec<Truth>, Vec<PathTruth>);

/// One event's candidates and truth, as the backtest gathered them.
#[derive(Debug, Clone, Default)]
pub struct EventRun {
    pub label: String,
    pub candidates: Vec<Candidate>,
    /// Tornado truth: local storm reports, and surveyed paths (matched along the path, not at a
    /// point; see `detverify::PathTruth`).
    pub tornado_reports: Vec<Truth>,
    pub tornado_surveys: Vec<PathTruth>,
    pub hail_reports: Vec<Truth>,
    pub volumes: usize,
    /// Radar time covered: from the first volume to the last, plus one volume's gap.
    pub radar_hours: f64,
    /// The sounding the hail algorithm's freezing levels came from ("Nashville, TN 03 00Z"), or
    /// `None` without one. Recorded because a failed download falls back to an older sounding,
    /// which changes every hail candidate.
    pub sounding: Option<String>,
}

/// Mark each candidate matched or not against its event's truth.
pub fn mark_matches(run: &mut EventRun, radius_km: f64, window_min: i64) {
    let near = |c: &Candidate, truths: &[Truth], paths: &[PathTruth]| {
        let d = c.detection();
        score_with_paths(&[d], truths, paths, radius_km, window_min, &[0.0])[0].verified > 0
    };
    for c in &mut run.candidates {
        if c.detector.is_tornado() {
            c.matched_report = near(c, &run.tornado_reports, &[]);
            c.matched_survey = near(c, &[], &run.tornado_surveys);
        } else {
            c.matched_report = near(c, &run.hail_reports, &[]);
            c.matched_survey = false;
        }
    }
}

/// The order rows are written in: event, time, detector, then place.
pub fn sort_candidates(c: &mut [Candidate]) {
    c.sort_by(|a, b| {
        (a.event.as_str(), a.minute, a.detector)
            .cmp(&(b.event.as_str(), b.minute, b.detector))
            .then(a.lon.total_cmp(&b.lon))
            .then(a.lat.total_cmp(&b.lat))
            .then(b.final_score.total_cmp(&a.final_score))
    });
}

/// One row of the by-threshold table.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThresholdRow {
    pub threshold: f32,
    pub detections: usize,
    pub verified: usize,
    pub events: usize,
    pub found: usize,
    pub pod: Option<f32>,
    pub far: Option<f32>,
    pub csi: Option<f32>,
    /// Verified detections over detections (1 - FAR).
    pub precision: Option<f32>,
    /// Found events over events (the POD).
    pub recall: Option<f32>,
    /// Harmonic mean of precision and recall.
    pub f1: Option<f32>,
    pub false_per_radar_hour: Option<f64>,
    pub false_per_volume: Option<f64>,
    /// Median minutes from the earliest detection near each found event to the event (negative:
    /// the detection came after the report), and how many found events it is over.
    pub median_lead_min: Option<f32>,
    pub leads: usize,
}

/// How often candidates in one score bin verified.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReliabilityBin {
    pub lo: f32,
    pub hi: f32,
    pub candidates: usize,
    pub verified: usize,
    pub verified_fraction: Option<f32>,
}

/// Candidates in one range or beam-height band.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Band {
    pub band: String,
    pub candidates: usize,
    pub verified: usize,
    pub verified_fraction: Option<f32>,
    /// The same, at or above an evidence score of 0.5.
    pub candidates_at_half: usize,
    pub verified_at_half: usize,
    /// Events (all of them, unbanded) and how many this band's detections at or above 0.5 found:
    /// would this band alone have caught them (`detverify::score_in_range`).
    pub events: usize,
    pub found_at_half: usize,
    pub pod_at_half: Option<f32>,
    pub far_at_half: Option<f32>,
    pub csi_at_half: Option<f32>,
}

/// One detector's baseline.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DetectorSummary {
    pub candidates: usize,
    pub volumes: usize,
    pub radar_hours: f64,
    pub detections_per_volume: Option<f64>,
    /// Against reports and surveys together, for the tornado detectors; reports for hail.
    pub by_threshold: Vec<ThresholdRow>,
    pub reliability: Vec<ReliabilityBin>,
    pub by_range: Vec<Band>,
    pub by_beam_height: Vec<Band>,
}

/// The whole baseline.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    /// The detector algorithm versions that produced it.
    pub versions: BTreeMap<String, String>,
    pub events: Vec<String>,
    /// Each event's sounding (see [`EventRun::sounding`]); "none" without one.
    pub soundings: BTreeMap<String, String>,
    pub radius_km: f64,
    pub window_min: i64,
    pub detectors: BTreeMap<String, DetectorSummary>,
}

pub const THRESHOLDS: [f32; 10] = [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];
/// The range bands detectionplan.md Phase 11 asks every revision to be broken down by.
const RANGE_BANDS: [(f32, f32, &str); 5] = [
    (0.0, 30.0, "0-30 km"),
    (30.0, 60.0, "30-60 km"),
    (60.0, 100.0, "60-100 km"),
    (100.0, 150.0, "100-150 km"),
    (150.0, f32::INFINITY, "150+ km"),
];
const HEIGHT_BANDS: [(f32, f32, &str); 4] = [
    (0.0, 1.0, "0-1 km"),
    (1.0, 2.0, "1-2 km"),
    (2.0, 3.0, "2-3 km"),
    (3.0, f32::INFINITY, "3+ km"),
];

fn ratio(n: usize, d: usize) -> Option<f32> {
    (d > 0).then(|| n as f32 / d as f32)
}

fn median(mut v: Vec<i64>) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2] as f32
    } else {
        (v[n / 2 - 1] + v[n / 2]) as f32 / 2.0
    })
}

/// One band: `keep` picks its candidates. Events stay whole: a band is scored on whether its own
/// detections would have found them.
fn band(
    label: &str,
    runs: &[EventRun],
    mine: &dyn Fn(&EventRun) -> Vec<Candidate>,
    truths: &dyn Fn(&EventRun) -> TruthSet,
    keep: &dyn Fn(&Candidate) -> bool,
    radius_km: f64,
    window_min: i64,
) -> Band {
    let (mut candidates, mut verified, mut at_half, mut verified_at_half) = (0, 0, 0, 0);
    let (mut events, mut found, mut detections, mut matched) = (0, 0, 0, 0);
    for r in runs {
        let rows: Vec<Candidate> = mine(r).into_iter().filter(|c| keep(c)).collect();
        candidates += rows.len();
        verified += rows.iter().filter(|c| c.verified()).count();
        let half: Vec<&Candidate> = rows.iter().filter(|c| c.final_score >= 0.5).collect();
        at_half += half.len();
        verified_at_half += half.iter().filter(|c| c.verified()).count();
        let d: Vec<Detection> = rows.iter().map(Candidate::detection).collect();
        let (tr, paths) = truths(r);
        let s = score_with_paths(&d, &tr, &paths, radius_km, window_min, &[0.5])[0];
        events += s.events;
        found += s.found;
        detections += s.detections;
        matched += s.verified;
    }
    let s = crate::detverify::Score {
        threshold: 0.5,
        detections,
        verified: matched,
        events,
        found,
    };
    Band {
        band: label.to_string(),
        candidates,
        verified,
        verified_fraction: ratio(verified, candidates),
        candidates_at_half: at_half,
        verified_at_half,
        events,
        found_at_half: found,
        pod_at_half: s.pod(),
        far_at_half: s.far(),
        csi_at_half: s.csi(),
    }
}

/// The baseline for one detector over every event.
pub fn summarize_detector(
    runs: &[EventRun],
    kind: DetectorKind,
    radius_km: f64,
    window_min: i64,
) -> DetectorSummary {
    let mine = |r: &EventRun| -> Vec<Candidate> {
        r.candidates
            .iter()
            .filter(|c| c.detector == kind)
            .cloned()
            .collect()
    };
    let truths = |r: &EventRun| -> TruthSet {
        if kind.is_tornado() {
            (r.tornado_reports.clone(), r.tornado_surveys.clone())
        } else {
            (r.hail_reports.clone(), Vec::new())
        }
    };
    let volumes: usize = runs.iter().map(|r| r.volumes).sum();
    let radar_hours: f64 = runs.iter().map(|r| r.radar_hours).sum();
    // Each event is scored against its own truth only, and the counts are added.
    let by_threshold = THRESHOLDS
        .iter()
        .map(|&t| {
            let mut row = (0usize, 0usize, 0usize, 0usize);
            let mut leads = Vec::new();
            for r in runs {
                let d: Vec<Detection> = mine(r).iter().map(Candidate::detection).collect();
                let (tr, paths) = truths(r);
                let s = score_with_paths(&d, &tr, &paths, radius_km, window_min, &[t])[0];
                row.0 += s.detections;
                row.1 += s.verified;
                row.2 += s.events;
                row.3 += s.found;
                leads.extend(crate::detverify::lead_minutes_with_paths(
                    &d, &tr, &paths, radius_km, window_min, t,
                ));
            }
            let (detections, verified, events, found) = row;
            let s = crate::detverify::Score {
                threshold: t,
                detections,
                verified,
                events,
                found,
            };
            let precision = ratio(verified, detections);
            let recall = s.pod();
            let f1 = match (precision, recall) {
                (Some(p), Some(r)) if p + r > 0.0 => Some(2.0 * p * r / (p + r)),
                _ => None,
            };
            ThresholdRow {
                threshold: t,
                detections,
                verified,
                events,
                found,
                pod: s.pod(),
                far: s.far(),
                csi: s.csi(),
                precision,
                recall,
                f1,
                false_per_radar_hour: (radar_hours > 0.0)
                    .then(|| (detections - verified) as f64 / radar_hours),
                false_per_volume: (volumes > 0)
                    .then(|| (detections - verified) as f64 / volumes as f64),
                leads: leads.len(),
                median_lead_min: median(leads),
            }
        })
        .collect();
    let all: Vec<Candidate> = runs.iter().flat_map(mine).collect();
    let refs: Vec<&Candidate> = all.iter().collect();
    let reliability = (0..10)
        .map(|i| {
            let (lo, hi) = (i as f32 / 10.0, (i + 1) as f32 / 10.0);
            let rows: Vec<&&Candidate> = refs
                .iter()
                .filter(|c| {
                    c.final_score >= lo && (c.final_score < hi || (i == 9 && c.final_score <= 1.0))
                })
                .collect();
            let verified = rows.iter().filter(|c| c.verified()).count();
            ReliabilityBin {
                lo,
                hi,
                candidates: rows.len(),
                verified,
                verified_fraction: ratio(verified, rows.len()),
            }
        })
        .collect();
    let by_range = RANGE_BANDS
        .iter()
        .map(|&(lo, hi, label)| {
            let keep = |c: &Candidate| c.range_km >= lo && c.range_km < hi;
            band(label, runs, &mine, &truths, &keep, radius_km, window_min)
        })
        .collect();
    let mut by_beam_height: Vec<Band> = HEIGHT_BANDS
        .iter()
        .map(|&(lo, hi, label)| {
            let keep = |c: &Candidate| c.beam_base_km.is_some_and(|h| h >= lo && h < hi);
            band(label, runs, &mine, &truths, &keep, radius_km, window_min)
        })
        .collect();
    if refs.iter().any(|c| c.beam_base_km.is_none()) {
        let keep = |c: &Candidate| c.beam_base_km.is_none();
        by_beam_height.push(band(
            "unknown", runs, &mine, &truths, &keep, radius_km, window_min,
        ));
    }
    DetectorSummary {
        candidates: all.len(),
        volumes,
        radar_hours,
        detections_per_volume: (volumes > 0).then(|| all.len() as f64 / volumes as f64),
        by_threshold,
        reliability,
        by_range,
        by_beam_height,
    }
}

/// The whole baseline over every event.
pub fn summarize(runs: &[EventRun], radius_km: f64, window_min: i64) -> Summary {
    let mut versions = BTreeMap::new();
    versions.insert("rotation".into(), crate::rotation::ALGORITHM_VERSION.into());
    versions.insert("debris".into(), crate::tds::ALGORITHM_VERSION.into());
    versions.insert(
        "tornado_fusion".into(),
        crate::tornado_fusion::ALGORITHM_VERSION.into(),
    );
    versions.insert(
        "debris_class".into(),
        format!(
            "{} + {}",
            crate::tds::ALGORITHM_VERSION,
            crate::debris_class::ALGORITHM_VERSION
        ),
    );
    versions.insert(
        "rotation_llsd".into(),
        format!(
            "{} + {} + {} + {}",
            crate::azshear::ALGORITHM_VERSION,
            crate::rotation_objects::ALGORITHM_VERSION,
            crate::rotation_columns::ALGORITHM_VERSION,
            crate::rotation_tracks::ALGORITHM_VERSION
        ),
    );
    Summary {
        versions,
        events: runs.iter().map(|r| r.label.clone()).collect(),
        soundings: runs
            .iter()
            .map(|r| {
                let s = r.sounding.clone().unwrap_or_else(|| "none".into());
                (r.label.clone(), s)
            })
            .collect(),
        radius_km,
        window_min,
        detectors: DetectorKind::ALL
            .iter()
            .map(|&k| {
                (
                    k.name().to_string(),
                    summarize_detector(runs, k, radius_km, window_min),
                )
            })
            .collect(),
    }
}

/// The candidates as CSV, header first. Fields are quoted only where they need it.
pub fn to_csv(candidates: &[Candidate]) -> String {
    const HEADER: &str = "event,site,volume,minute,detector,lon,lat,range_km,beam_base_km,\
        beam_top_km,raw_score,final_score,gates,tilts,vrot_ms,g2g_ms,min_cc,mean_cc,mean_z,max_z,\
        zdr_db,depth_km,rooted,sense,tier,members,observed_warning,matched_report,matched_survey,\
        azshear_s,track_id,track_age_volumes,azshear_trend";
    fn cell(s: &str) -> String {
        if s.contains([',', '"', '\n']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    }
    fn o<T: std::fmt::Display>(v: Option<T>) -> String {
        v.map_or_else(String::new, |v| v.to_string())
    }
    fn f(v: Option<f32>) -> String {
        v.map_or_else(String::new, |v| format!("{v:.4}"))
    }
    let mut out = String::from(HEADER);
    for name in crate::tornado_fusion::FEATURE_NAMES {
        out.push_str(",f_");
        out.push_str(name);
    }
    out.push('\n');
    for c in candidates {
        let mut row = vec![
            cell(&c.event),
            cell(&c.site),
            cell(&c.volume),
            c.minute.to_string(),
            c.detector.name().to_string(),
            format!("{:.5}", c.lon),
            format!("{:.5}", c.lat),
            format!("{:.2}", c.range_km),
            f(c.beam_base_km),
            f(c.beam_top_km),
            format!("{:.4}", c.raw_score),
            format!("{:.4}", c.final_score),
            o(c.gates),
            o(c.tilts),
            f(c.vrot_ms),
            f(c.g2g_ms),
            f(c.min_cc),
            f(c.mean_cc),
            f(c.mean_z),
            f(c.max_z),
            f(c.zdr_db),
            f(c.depth_km),
            o(c.rooted),
            cell(c.sense.as_deref().unwrap_or("")),
            cell(c.tier.as_deref().unwrap_or("")),
            o(c.members),
            c.observed_warning.to_string(),
            c.matched_report.to_string(),
            c.matched_survey.to_string(),
            f(c.azshear_s),
            o(c.track_id),
            o(c.track_age_volumes),
            f(c.azshear_trend),
        ];
        let features = c.features.as_deref().unwrap_or(&[]);
        row.extend(
            (0..crate::tornado_fusion::FEATURE_NAMES.len()).map(|i| f(features.get(i).copied())),
        );
        out.push_str(&row.join(","));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(kind: DetectorKind, lon: f64, score: f32, minute: i64) -> Candidate {
        Candidate {
            event: "KTLX test".into(),
            site: "KTLX".into(),
            volume: "2013-05-20T20:00:00Z".into(),
            minute,
            detector: kind,
            lon,
            lat: 35.3,
            range_km: 30.0,
            beam_base_km: Some(0.6),
            beam_top_km: Some(2.1),
            raw_score: score,
            final_score: score,
            gates: Some(12),
            tilts: Some(3),
            vrot_ms: None,
            g2g_ms: None,
            min_cc: None,
            mean_cc: None,
            mean_z: None,
            max_z: None,
            zdr_db: None,
            depth_km: Some(1.5),
            rooted: Some(true),
            sense: None,
            tier: None,
            members: None,
            observed_warning: false,
            matched_report: false,
            matched_survey: false,
            azshear_s: None,
            track_id: None,
            track_age_volumes: None,
            azshear_trend: None,
            features: None,
        }
    }

    fn run() -> EventRun {
        EventRun {
            label: "KTLX test".into(),
            candidates: vec![
                cand(DetectorKind::Rotation, -97.49, 0.8, 100),
                cand(DetectorKind::Rotation, -98.50, 0.6, 100),
                cand(DetectorKind::Debris, -97.49, 0.9, 105),
                cand(DetectorKind::Hail, -97.49, 0.7, 100),
            ],
            tornado_reports: vec![Truth {
                lon: -97.48,
                lat: 35.31,
                minute: 102,
            }],
            tornado_surveys: Vec::new(),
            hail_reports: Vec::new(),
            volumes: 2,
            radar_hours: 0.5,
            sounding: Some("Norman, OK 20 12Z".into()),
        }
    }

    #[test]
    fn candidates_are_matched_against_their_own_kind_of_truth() {
        let mut r = run();
        mark_matches(&mut r, 10.0, 15);
        let m: Vec<bool> = r.candidates.iter().map(|c| c.matched_report).collect();
        // The far couplet misses; the hail core is not matched by a tornado report.
        assert_eq!(m, [true, false, true, false]);
    }

    #[test]
    fn the_summary_counts_false_alarms_per_radar_hour_and_by_score_bin() {
        let mut r = run();
        mark_matches(&mut r, 10.0, 15);
        let s = summarize(&[r], 10.0, 15);
        let rot = &s.detectors["rotation"];
        assert_eq!(rot.candidates, 2);
        assert_eq!(rot.detections_per_volume, Some(1.0));
        let all = &rot.by_threshold[0];
        assert_eq!(
            (all.detections, all.verified, all.events, all.found),
            (2, 1, 1, 1)
        );
        assert_eq!(all.false_per_radar_hour, Some(2.0));
        let at_07 = &rot.by_threshold[7];
        assert_eq!((at_07.detections, at_07.verified), (1, 1));
        // 0.6 lands in the 0.6-0.7 bin, 0.8 in 0.8-0.9.
        assert_eq!(rot.reliability[6].candidates, 1);
        assert_eq!(rot.reliability[6].verified, 0);
        assert_eq!(rot.reliability[8].verified_fraction, Some(1.0));
        // 30 km is the start of the second band.
        assert_eq!(rot.by_range[0].candidates, 0);
        assert_eq!(rot.by_range[1].candidates, 2);
        assert_eq!(
            (rot.by_range[1].events, rot.by_range[1].found_at_half),
            (1, 1)
        );
        assert_eq!(rot.by_range[1].pod_at_half, Some(1.0));
        // Precision, recall and F1 at 0: one of two verified, the one event found.
        let all = &rot.by_threshold[0];
        assert_eq!((all.precision, all.recall), (Some(0.5), Some(1.0)));
        assert!((all.f1.unwrap() - 2.0 / 3.0).abs() < 1e-6);
        assert_eq!(all.false_per_volume, Some(0.5));
        // The detection at minute 100 came 2 minutes before the report at 102.
        assert_eq!((all.median_lead_min, all.leads), (Some(2.0), 1));
        assert_eq!(rot.by_beam_height[0].candidates, 2);
        assert_eq!(s.versions["rotation"], crate::rotation::ALGORITHM_VERSION);
        assert_eq!(s.soundings["KTLX test"], "Norman, OK 20 12Z");
    }

    #[test]
    fn a_score_of_one_lands_in_the_top_bin() {
        let mut r = run();
        r.candidates[0].final_score = 1.0;
        let s = summarize_detector(&[r], DetectorKind::Rotation, 10.0, 15);
        assert_eq!(s.reliability[9].candidates, 1);
    }

    #[test]
    fn the_export_is_the_same_whatever_order_candidates_arrive_in() {
        let mut a = run().candidates;
        let mut b = a.clone();
        b.reverse();
        sort_candidates(&mut a);
        sort_candidates(&mut b);
        assert_eq!(to_csv(&a), to_csv(&b));
        let csv = to_csv(&a);
        assert!(csv.starts_with("event,site,volume,minute,detector,"));
        assert_eq!(csv.lines().count(), 5);
        assert!(
            csv.contains(",rotation,-97.49000,35.30000,30.00,0.6000,2.1000,0.8000,0.8000,12,3,")
        );
    }
}
