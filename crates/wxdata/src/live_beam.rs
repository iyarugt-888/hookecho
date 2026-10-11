//! Geometry of the newest received radial, never a prediction of antenna position.

use crate::level2::{Moment, Scan};
use crate::live::RadialCoverage;
use nexrad_model::data::DataMoment;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub bearing_deg: f32,
    /// Far edge of the last native gate, in the same range convention as the 2D polar display.
    pub range_km: f32,
    pub acquired_ms: i64,
}

/// Bounded numeric metadata only; no gate buffers or fabricated positions/times.
#[derive(Clone, Debug)]
pub struct Observation {
    pub elevation_deg: f32,
    pub edges: [Option<Edge>; 7],
}

/// Match raw arriving radial identities to the accepted merged scan. Retained old sectors,
/// unavailable moments and unknown source clocks cannot contribute an edge.
pub fn observe(scan: &Scan, coverage: &RadialCoverage) -> Option<Observation> {
    let arriving: HashSet<_> = coverage.radials.iter().copied().collect();
    let cut = coverage.progress.elevation_number;
    let mut observation = Observation {
        elevation_deg: coverage.progress.elevation_angle_deg as f32,
        edges: [None; 7],
    };
    if !observation.elevation_deg.is_finite() {
        return None;
    }
    for sweep in scan
        .sweeps()
        .iter()
        .filter(|s| usize::from(s.elevation_number()) == cut)
    {
        for radial in sweep.radials() {
            let acquired_ms = radial.collection_timestamp();
            let bearing = radial.azimuth_angle_degrees();
            if acquired_ms <= 0
                || !bearing.is_finite()
                || !arriving.contains(&(radial.azimuth_number(), acquired_ms))
            {
                continue;
            }
            for moment in Moment::ALL {
                let Some(data) = moment.select(radial) else {
                    continue;
                };
                let count = data.gate_count();
                let spacing = data.gate_interval_km();
                let first = data.first_gate_range_km();
                let range_km = first + (f64::from(count) - 0.5) * spacing;
                if count == 0
                    || !spacing.is_finite()
                    || spacing <= 0.0
                    || !first.is_finite()
                    || first < 0.0
                    || !range_km.is_finite()
                    || range_km <= 0.0
                {
                    continue;
                }
                let edge = &mut observation.edges[moment.index()];
                if edge.is_none_or(|previous| acquired_ms > previous.acquired_ms) {
                    *edge = Some(Edge {
                        bearing_deg: bearing.rem_euclid(360.0),
                        range_km: range_km as f32,
                        acquired_ms,
                    });
                }
            }
        }
    }
    observation
        .edges
        .iter()
        .any(Option::is_some)
        .then_some(observation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::{CutKind, ScanProgress};
    use nexrad_model::data::{MomentData, Radial, RadialStatus, Sweep};

    fn radial(number: u16, bearing: f32, time: i64, gates: u16, velocity: bool) -> Radial {
        let data =
            MomentData::from_fixed_point(gates, 1000, 250, 8, 2.0, 66.0, vec![120; gates as usize]);
        Radial::new(
            time,
            number,
            bearing,
            0.5,
            RadialStatus::IntermediateRadialData,
            1,
            0.5,
            Some(data.clone()),
            velocity.then_some(data),
            None,
            None,
            None,
            None,
            None,
        )
    }

    fn input(radials: Vec<Radial>, arrivals: Vec<(u16, i64)>) -> (Scan, RadialCoverage) {
        let source = crate::level2::decode_volume(
            include_bytes!("../tests/data/corpus/denver-hail-2017-first-records.ar2").to_vec(),
        )
        .unwrap();
        let scan = Scan::new(
            source.coverage_pattern().clone(),
            vec![Sweep::new(1, radials)],
        );
        let coverage = RadialCoverage {
            progress: ScanProgress {
                volume_start_ms: None,
                vcp_number: Some(scan.coverage_pattern_number().number()),
                cut_kind: CutKind::Standard,
                elevation_number: 1,
                total_elevations: 1,
                elevation_angle_deg: 0.5,
                azimuth_rate_dps: 90.0,
                azimuth_start_deg: 0.0,
                azimuth_end_deg: 60.0,
                chunk_index: 1,
                chunks_in_sweep: 6,
            },
            radials: arrivals,
            source_passes: None,
            source_sequences: None,
            source_attribution: None,
            source_scope: None,
        };
        (scan, coverage)
    }

    #[test]
    fn bearings_and_endpoints_come_from_the_arriving_moments_not_chunk_numbers() {
        let (scan, coverage) = input(
            vec![
                radial(2, 271.25, 100, 400, true),
                radial(1, 359.75, 200, 1000, false),
            ],
            vec![(1, 200), (2, 100)],
        );
        let observation = observe(&scan, &coverage).unwrap();
        let refl = observation.edges[Moment::Reflectivity.index()].unwrap();
        assert_eq!(refl.bearing_deg, 359.75); // chunk-number estimate says 0–60, entirely wrong
        assert_eq!(refl.acquired_ms, 200);
        assert!((refl.range_km - 250.875).abs() < 1e-5);
        let vel = observation.edges[Moment::Velocity.index()].unwrap();
        assert_eq!(vel.bearing_deg, 271.25);
        assert!((vel.range_km - 100.875).abs() < 1e-5);
        assert!(observation.edges[Moment::CorrelationCoefficient.index()].is_none());
        let mut reordered = coverage.clone();
        reordered.radials.reverse();
        assert_eq!(observe(&scan, &reordered).unwrap().edges, observation.edges);
    }

    #[test]
    fn retained_radials_unknown_clocks_and_unavailable_geometry_do_not_move_the_beam() {
        let (scan, mut coverage) = input(
            vec![
                radial(1, 10.0, 100, 100, false),
                radial(2, 40.0, 200, 100, false),
            ],
            vec![(1, 100)],
        );
        assert_eq!(
            observe(&scan, &coverage).unwrap().edges[0]
                .unwrap()
                .bearing_deg,
            10.0
        );
        coverage.radials = vec![(1, 99)]; // same ordinal, foreign pass clock
        assert!(observe(&scan, &coverage).is_none());
        coverage.progress.elevation_number = 2;
        coverage.radials = vec![(1, 100)];
        assert!(observe(&scan, &coverage).is_none());
        for invalid in [
            radial(1, 10.0, 0, 100, false),
            radial(1, f32::NAN, 100, 100, false),
            radial(1, 10.0, 100, 0, false),
        ] {
            let time = invalid.collection_timestamp();
            let (scan, coverage) = input(vec![invalid], vec![(1, time)]);
            assert!(observe(&scan, &coverage).is_none());
        }
    }
}
