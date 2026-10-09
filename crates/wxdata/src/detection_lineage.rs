//! Where a Tornado ID verdict came from (ROADMAP_PARITY M1.4): which pipeline made it and at which
//! algorithm versions, from which radar volume, and when the sweeps its rotation evidence was
//! measured on were scanned.
//!
//! Tornado ID has two pipelines. The fused one (LLSD rotation columns, tracked, with debris
//! classified beside them) is the default. The original (gate-to-gate couplets and debris
//! signatures) stands in while the fused verdict for a volume is still being computed and on a
//! light loop frame, and is chosen when the reader picks it. A marker looks the same either way,
//! so the record says which made it, and why the original stood in when it did.
//!
//! Input clocks are the sweeps' own per-radial acquisition times, gathered by
//! [`crate::level2::temporal::prepare`] under the continuous policy the detectors read with: rows
//! from the previous antenna pass are counted, never relabelled, and a row without a clock stays
//! unknown. When the inputs were not recorded the record says so instead of borrowing the volume's
//! nominal time. The rotation evidence's sweeps and the debris evidence's (reflectivity, CC and
//! ZDR) are recorded apart: they are different tilts, and can be from different antenna passes.

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::level2::temporal::{self, TemporalCoverage, TemporalPolicy};
use crate::level2::BinnedSweep;

/// Which Tornado ID pipeline made a verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pipeline {
    Fused,
    Original,
}

/// The provenance of one volume's Tornado ID verdicts. Every verdict from a volume shares it.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectionLineage {
    pub pipeline: Pipeline,
    /// (stage, version), in the order the pipeline runs.
    pub algorithms: Vec<(&'static str, &'static str)>,
    pub site: Option<String>,
    /// The decoded volume's name.
    pub volume: String,
    /// The volume's nominal time, as the volume states it.
    pub volume_time: Option<DateTime<Utc>>,
    /// The sweeps the rotation evidence was measured on and when they were scanned; `None` when
    /// they were not recorded.
    pub inputs: Option<TemporalCoverage>,
    /// The sweeps the debris signatures beside the verdict were read from (reflectivity and CC
    /// pairs, and the lowest ZDR sweep that discounts them) and when they were scanned; `None`
    /// when not recorded, including a volume without dual-pol. Both pipelines read the same
    /// debris for a volume's newest pass, so the record is the same for either.
    pub debris_inputs: Option<TemporalCoverage>,
    /// The fused pipeline's earlier low-level passes in this volume (a SAILS or MRLE rescan of
    /// the lowest tilt), oldest first. Each was tracked with its own sweeps and debris, and a
    /// *likely* verdict needs its track to have read *likely* on two passes, so these are inputs
    /// to the tier as well. Empty for the original pipeline and for a volume with one pass.
    pub earlier_passes: Vec<EarlierPass>,
    /// Why the original pipeline stands in for the fused one, when it does.
    pub stand_in: Option<&'static str>,
}

/// One earlier low-level pass the fused tracker read: the pass's own time (when its lowest-tilt
/// velocity sweep finished, [`crate::low_passes::Pass`]), and when the sweeps behind its rotation
/// columns and its debris signatures were scanned. A pass reads its own lowest tilt under the
/// volume's upper tilts ([`crate::low_passes::at_pass`]), so its inputs can end after its time.
#[derive(Debug, Clone, PartialEq)]
pub struct EarlierPass {
    pub time: DateTime<Utc>,
    pub inputs: Option<TemporalCoverage>,
    pub debris_inputs: Option<TemporalCoverage>,
}

/// The fused pipeline's stages and versions.
pub fn fused_algorithms() -> Vec<(&'static str, &'static str)> {
    vec![
        ("azshear", crate::azshear::ALGORITHM_VERSION),
        (
            "rotation_objects",
            crate::rotation_objects::ALGORITHM_VERSION,
        ),
        (
            "rotation_columns",
            crate::rotation_columns::ALGORITHM_VERSION,
        ),
        ("rotation_tracks", crate::rotation_tracks::ALGORITHM_VERSION),
        ("debris_signature", crate::tds::ALGORITHM_VERSION),
        ("debris_class", crate::debris_class::ALGORITHM_VERSION),
        ("tornado_fusion", crate::tornado_fusion::ALGORITHM_VERSION),
    ]
}

/// The original pipeline's stages and versions.
pub fn original_algorithms() -> Vec<(&'static str, &'static str)> {
    vec![
        ("rotation", crate::rotation::ALGORITHM_VERSION),
        ("debris_signature", crate::tds::ALGORITHM_VERSION),
        ("tornado_id", crate::tornado_id::ALGORITHM_VERSION),
    ]
}

/// The acquisition coverage of detector inputs, read as the detectors read them (continuous: no
/// row is masked). `None` when a sweep is malformed, rather than a partial record.
pub fn input_coverage(mut sweeps: Vec<BinnedSweep>) -> Option<TemporalCoverage> {
    temporal::prepare(&mut sweeps, TemporalPolicy::Continuous).ok()
}

/// The acquisition coverage of the debris signatures' inputs: the (reflectivity, CC) pairs given
/// to [`crate::tds::detect_volume`], and the lowest ZDR sweep, the one [`crate::tds::apply_zdr`]
/// discounts by. `None` with no pairs (no dual-pol on the volume) or a malformed sweep.
pub fn debris_input_coverage(
    pairs: Vec<(BinnedSweep, BinnedSweep)>,
    zdr: Vec<BinnedSweep>,
) -> Option<TemporalCoverage> {
    if pairs.is_empty() {
        return None;
    }
    let lowest_zdr = zdr
        .into_iter()
        .filter(|s| s.moment == crate::level2::Moment::DifferentialReflectivity)
        .min_by(|a, b| a.elevation_deg.total_cmp(&b.elevation_deg));
    input_coverage(
        pairs
            .into_iter()
            .flat_map(|(z, cc)| [z, cc])
            .chain(lowest_zdr)
            .collect(),
    )
}

fn utc(ms: i64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp_millis(ms)
}

fn clock(ms: i64) -> String {
    utc(ms).map_or_else(|| "unknown".into(), |t| t.format("%H:%M:%SZ").to_string())
}

/// One evidence's input lines: its tilts and when they were scanned, and the rows that are from
/// the previous pass or carry no clock. `what` names the evidence ("Rotation", "Debris"). The
/// debris and rotation layers' own hovers show these too, from the same records.
pub fn input_lines(what: &str, inputs: Option<&TemporalCoverage>) -> Vec<String> {
    let mut out = Vec::new();
    coverage_lines(&mut out, what, inputs);
    out
}

fn coverage_lines(out: &mut Vec<String>, what: &str, inputs: Option<&TemporalCoverage>) {
    let Some(c) = inputs else {
        out.push(format!("{what} input scan times: not recorded"));
        return;
    };
    let lower = what.to_lowercase();
    let mut tilts: Vec<f32> = c.contributors.iter().map(|s| s.elevation_deg).collect();
    tilts.sort_by(f32::total_cmp);
    tilts.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    let tilts = tilts
        .iter()
        .map(|e| format!("{e:.1}\u{b0}"))
        .collect::<Vec<_>>()
        .join(", ");
    match c.acquisition_range_ms() {
        Some((a, b)) => out.push(format!(
            "{what} inputs scanned {}\u{2013}{} on {tilts}",
            clock(a),
            clock(b)
        )),
        None => out.push(format!("{what} inputs on {tilts}; scan times unknown")),
    }
    let older = c.retained_older_rows();
    if older > 0 {
        out.push(format!(
            "{older} {lower} input rows from the previous antenna pass (mixed times)"
        ));
    }
    let unknown = c.unknown_time_rows();
    if unknown > 0 {
        out.push(format!("{unknown} {lower} input rows without a scan time"));
    }
}

/// An input record's scan interval in a few words: "20:10:02Z–20:11:40Z", or why there is none.
fn span(inputs: Option<&TemporalCoverage>) -> String {
    match inputs.map(TemporalCoverage::acquisition_range_ms) {
        None => "not recorded".into(),
        Some(None) => "scan times unknown".into(),
        Some(Some((a, b))) => format!("{}\u{2013}{}", clock(a), clock(b)),
    }
}

/// One evidence's inputs for an export: every clock as UTC RFC 3339, `null` where unknown.
fn coverage_json(c: &TemporalCoverage) -> Value {
    let rfc = |ms: Option<i64>| ms.and_then(utc).map(|t| t.to_rfc3339());
    let range = c.acquisition_range_ms();
    json!({
        "policy": match c.policy {
            TemporalPolicy::Continuous => "continuous",
            TemporalPolicy::StrictCurrent => "strict_current",
        },
        "acquisition_start_utc": rfc(range.map(|r| r.0)),
        "acquisition_end_utc": rfc(range.map(|r| r.1)),
        "older_pass_rows": c.retained_older_rows(),
        "unknown_time_rows": c.unknown_time_rows(),
        "unobserved_rows": c.unobserved_rows(),
        "sweeps": c.contributors.iter().map(|s| json!({
            "moment": s.moment.short_name(),
            "elevation_deg": s.elevation_deg,
            "azimuth_rows": s.azimuth_rows,
            "start_utc": rfc(s.used_start_ms),
            "end_utc": rfc(s.used_end_ms),
            "older_pass_rows": s.older_pass_rows,
            "unknown_time_rows": s.unknown_time_rows,
            "unobserved_rows": s.unobserved_rows,
        })).collect::<Vec<_>>(),
    })
}

impl DetectionLineage {
    /// What a reader needs to know, one line each: the pipeline and its version, the volume, and
    /// when its inputs were scanned.
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let version = |stage: &str| {
            self.algorithms
                .iter()
                .find(|(s, _)| *s == stage)
                .map_or("?", |(_, v)| *v)
        };
        match self.pipeline {
            Pipeline::Fused => out.push(format!(
                "Fused Tornado ID ({}): LLSD rotation columns, tracked, with debris beside them",
                version("tornado_fusion")
            )),
            Pipeline::Original => out.push(format!(
                "Original Tornado ID ({}): couplets ({}) and debris signatures ({})",
                version("tornado_id"),
                version("rotation"),
                version("debris_signature")
            )),
        }
        if let Some(why) = self.stand_in {
            out.push(format!("Standing in for the fused verdict: {why}"));
        }
        let volume = match (&self.site, self.volume_time) {
            (Some(s), Some(t)) => format!("{s} volume {}", t.format("%Y-%m-%d %H:%M:%SZ")),
            (Some(s), None) => format!("{s} volume {}", self.volume),
            (None, _) => format!("Volume {}", self.volume),
        };
        out.push(volume);
        coverage_lines(&mut out, "Rotation", self.inputs.as_ref());
        coverage_lines(&mut out, "Debris", self.debris_inputs.as_ref());
        for p in &self.earlier_passes {
            out.push(format!(
                "Earlier low-level pass (lowest tilt done {}): rotation {}, debris {}",
                p.time.format("%H:%M:%SZ"),
                span(p.inputs.as_ref()),
                span(p.debris_inputs.as_ref())
            ));
        }
        out
    }

    /// The record for an export: every clock as UTC RFC 3339, `null` where unknown.
    pub fn to_json(&self) -> Value {
        let algorithms: serde_json::Map<String, Value> = self
            .algorithms
            .iter()
            .map(|(s, v)| ((*s).to_string(), Value::from(*v)))
            .collect();
        json!({
            "pipeline": match self.pipeline {
                Pipeline::Fused => "fused",
                Pipeline::Original => "original",
            },
            "algorithms": algorithms,
            "site": self.site,
            "volume": self.volume,
            "volume_time_utc": self.volume_time.map(|t| t.to_rfc3339()),
            "inputs": self.inputs.as_ref().map(coverage_json),
            "debris_inputs": self.debris_inputs.as_ref().map(coverage_json),
            "earlier_passes": self.earlier_passes.iter().map(|p| json!({
                "time_utc": p.time.to_rfc3339(),
                "inputs": p.inputs.as_ref().map(coverage_json),
                "debris_inputs": p.debris_inputs.as_ref().map(coverage_json),
            })).collect::<Vec<_>>(),
            "stand_in": self.stand_in,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level2::Moment;

    fn sweep(moment: Moment, elev: f32, times: Vec<i64>) -> BinnedSweep {
        let az = times.len();
        let mut data = vec![100u8; az * 2];
        // An empty, untimed row: not yet scanned (or a gap), not "unknown time".
        if let Some(i) = times.iter().position(|&t| t == -1) {
            data[i * 2] = 0;
            data[i * 2 + 1] = 0;
        }
        BinnedSweep {
            moment,
            az_bins: az,
            gate_count: 2,
            data,
            elevation_deg: elev,
            bin_time_ms: times.into_iter().map(|t| t.max(0)).collect(),
            ..Default::default()
        }
    }

    fn lineage(inputs: Option<TemporalCoverage>) -> DetectionLineage {
        DetectionLineage {
            pipeline: Pipeline::Fused,
            algorithms: fused_algorithms(),
            site: Some("KTLX".into()),
            volume: "KTLX20130520_201229_V06".into(),
            volume_time: "2013-05-20T20:12:29Z".parse().ok(),
            inputs,
            debris_inputs: None,
            earlier_passes: Vec::new(),
            stand_in: None,
        }
    }

    #[test]
    fn input_clocks_come_from_the_sweeps_and_unknown_stays_unknown() {
        let t0 = 1_369_080_749_000i64; // 20:12:29Z
        let c = input_coverage(vec![
            sweep(
                Moment::Velocity,
                0.5,
                vec![t0, t0 + 1_000, t0 + 2_000, t0 + 3_000],
            ),
            sweep(
                Moment::Reflectivity,
                0.5,
                vec![t0, t0 + 1_000, 0, t0 + 3_000],
            ),
            sweep(
                Moment::Velocity,
                0.9,
                vec![t0 + 20_000, t0 + 21_000, -1, t0 + 23_000],
            ),
        ])
        .unwrap();
        let l = lineage(Some(c));
        let lines = l.lines();
        assert!(lines[0].contains("fusion-3"), "{lines:?}");
        assert!(
            lines[1].contains("KTLX volume 2013-05-20 20:12:29Z"),
            "{lines:?}"
        );
        assert_eq!(
            lines[2],
            "Rotation inputs scanned 20:12:29Z\u{2013}20:12:52Z on 0.5\u{b0}, 0.9\u{b0}"
        );
        assert_eq!(
            lines[3], "1 rotation input rows without a scan time",
            "the timeless row with data"
        );
        assert_eq!(lines[4], "Debris input scan times: not recorded");
        let j = l.to_json();
        assert_eq!(j["pipeline"], "fused");
        assert_eq!(j["algorithms"]["tornado_fusion"], "fusion-3");
        assert_eq!(
            j["inputs"]["acquisition_start_utc"],
            "2013-05-20T20:12:29+00:00"
        );
        assert_eq!(j["inputs"]["unknown_time_rows"], 1);
        assert_eq!(
            j["inputs"]["unobserved_rows"], 1,
            "the empty untimed row is not 'unknown'"
        );
        assert_eq!(j["inputs"]["sweeps"][1]["moment"], "REF");
    }

    #[test]
    fn missing_inputs_are_said_to_be_missing_not_borrowed_from_the_volume() {
        let mut l = lineage(None);
        l.pipeline = Pipeline::Original;
        l.algorithms = original_algorithms();
        l.stand_in = Some("the fused verdict for this volume is still being computed");
        let lines = l.lines();
        assert!(lines[0].starts_with("Original Tornado ID"), "{lines:?}");
        assert!(lines[1].contains("still being computed"));
        assert_eq!(
            lines[lines.len() - 2],
            "Rotation input scan times: not recorded"
        );
        let j = l.to_json();
        assert!(j["inputs"].is_null());
        assert!(j["debris_inputs"].is_null());
        assert_eq!(
            j["algorithms"]["rotation"],
            crate::rotation::ALGORITHM_VERSION
        );
        // A sweep with no clocks at all keeps them unknown, with no interval invented.
        let c = input_coverage(vec![sweep(Moment::Velocity, 0.5, vec![0, 0])]).unwrap();
        assert_eq!(c.acquisition_range_ms(), None);
        let l = lineage(Some(c));
        assert!(
            l.lines()[2].ends_with("scan times unknown"),
            "{:?}",
            l.lines()
        );
        assert!(l.to_json()["inputs"]["acquisition_start_utc"].is_null());
        // Malformed inputs give no record rather than a partial one.
        let mut bad = sweep(Moment::Velocity, 0.5, vec![1, 2]);
        bad.data.pop();
        assert!(input_coverage(vec![bad]).is_none());
    }

    #[test]
    fn debris_inputs_are_their_own_sweeps_with_the_lowest_zdr() {
        let t0 = 1_369_080_749_000i64; // 20:12:29Z
        let pair = |elev: f32, t: i64| {
            (
                sweep(Moment::Reflectivity, elev, vec![t, t + 1_000]),
                sweep(Moment::CorrelationCoefficient, elev, vec![t, t + 1_000]),
            )
        };
        let c = debris_input_coverage(
            vec![pair(0.5, t0), pair(0.9, t0 + 40_000)],
            vec![
                // The higher ZDR sweep is not read by the discount, so its late clock is not an
                // input; the lowest is.
                sweep(Moment::DifferentialReflectivity, 0.9, vec![t0 + 90_000, 0]),
                sweep(Moment::DifferentialReflectivity, 0.5, vec![t0 + 2_000, 0]),
            ],
        )
        .unwrap();
        assert_eq!(c.contributors.len(), 5, "two pairs and one ZDR sweep");
        assert_eq!(c.acquisition_range_ms(), Some((t0, t0 + 41_000)));
        let mut l = lineage(None);
        l.debris_inputs = Some(c);
        let lines = l.lines();
        assert_eq!(lines[2], "Rotation input scan times: not recorded");
        assert_eq!(
            lines[3],
            "Debris inputs scanned 20:12:29Z\u{2013}20:13:10Z on 0.5\u{b0}, 0.9\u{b0}"
        );
        assert_eq!(lines[4], "1 debris input rows without a scan time");
        // The debris layer's own hover shows the same lines from the same record.
        assert_eq!(
            input_lines("Debris", l.debris_inputs.as_ref()),
            lines[3..5].to_vec()
        );
        assert_eq!(
            input_lines("Rotation", None),
            ["Rotation input scan times: not recorded"]
        );
        let j = l.to_json();
        assert_eq!(
            j["debris_inputs"]["acquisition_end_utc"],
            "2013-05-20T20:13:10+00:00"
        );
        assert_eq!(j["debris_inputs"]["sweeps"][1]["moment"], "CC");
        assert_eq!(j["debris_inputs"]["sweeps"][4]["moment"], "ZDR");
        // No dual-pol on the volume: nothing recorded, not an empty interval.
        assert!(debris_input_coverage(Vec::new(), Vec::new()).is_none());
    }

    #[test]
    fn earlier_passes_name_their_own_scan_times() {
        let t0 = 1_369_080_749_000i64; // 20:12:29Z
        let mut l = lineage(None);
        l.earlier_passes = vec![
            EarlierPass {
                time: utc(t0 - 120_000).unwrap(),
                inputs: input_coverage(vec![sweep(
                    Moment::Velocity,
                    0.5,
                    vec![t0 - 120_000, t0 - 106_000],
                )]),
                debris_inputs: None,
            },
            EarlierPass {
                time: utc(t0 - 60_000).unwrap(),
                // Clocks on no row: an interval is never invented.
                inputs: input_coverage(vec![sweep(Moment::Velocity, 0.5, vec![0, 0])]),
                debris_inputs: debris_input_coverage(
                    vec![(
                        sweep(Moment::Reflectivity, 0.5, vec![t0 - 78_000, t0 - 61_000]),
                        sweep(
                            Moment::CorrelationCoefficient,
                            0.5,
                            vec![t0 - 78_000, t0 - 61_000],
                        ),
                    )],
                    Vec::new(),
                ),
            },
        ];
        let lines = l.lines();
        let n = lines.len();
        assert_eq!(
            lines[n - 2],
            "Earlier low-level pass (lowest tilt done 20:10:29Z): rotation \
             20:10:29Z\u{2013}20:10:43Z, debris not recorded"
        );
        assert_eq!(
            lines[n - 1],
            "Earlier low-level pass (lowest tilt done 20:11:29Z): rotation scan times unknown, \
             debris 20:11:11Z\u{2013}20:11:28Z"
        );
        let j = l.to_json();
        assert_eq!(
            j["earlier_passes"][0]["time_utc"],
            "2013-05-20T20:10:29+00:00"
        );
        assert!(j["earlier_passes"][0]["debris_inputs"].is_null());
        assert_eq!(
            j["earlier_passes"][1]["debris_inputs"]["acquisition_start_utc"],
            "2013-05-20T20:11:11+00:00"
        );
        // One pass in the volume: nothing listed, an empty array exported.
        assert!(lineage(None).to_json()["earlier_passes"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}
