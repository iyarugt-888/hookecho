//! Spatial evidence for a SCIT storm. Source objects stay separate: containment is coverage,
//! and a nearby radar signature is a tentative association rather than a shared identity.

use wxdata::{level3::Cell, overlay::GeoFeature, tornado_id::Circulation};

pub(super) const MAX_CIRCULATION_KM: f64 = 10.0;
/// Two cores nearly equally close to one signature do not establish which storm owns it.
const AMBIGUITY_KM: f64 = 1.0;

pub(super) struct Evidence<'a> {
    pub cells: &'a [Cell],
    pub circulations: &'a [Circulation],
    pub warnings: &'a [GeoFeature],
    pub probsevere: &'a [GeoFeature],
}

#[derive(Debug, Default)]
pub(super) struct Associations {
    pub warnings: Vec<usize>,
    pub probsevere: Vec<usize>,
    /// Source circulation index and separation from the nearest member (km).
    pub circulations: Vec<(usize, f64)>,
    pub ambiguous: usize,
}

fn separation(cell: &Cell, circulation: &Circulation) -> f64 {
    std::iter::once([circulation.id.lon, circulation.id.lat])
        .chain(circulation.members.iter().map(|m| [m.lon, m.lat]))
        .map(|at| crate::geo::great_circle([cell.lon, cell.lat], at).0)
        .filter(|d| d.is_finite())
        .fold(f64::INFINITY, f64::min)
}

/// Associations for one selected storm, evaluated against every current SCIT core so a
/// circulation cannot be independently claimed by whichever detail cards happen to be open.
pub(super) fn associate(cell: &Cell, evidence: &Evidence<'_>) -> Associations {
    let covering = |features: &[GeoFeature]| {
        features
            .iter()
            .enumerate()
            .filter(|(_, f)| f.contains(cell.lon, cell.lat))
            .map(|(i, _)| i)
            .collect()
    };
    let mut result = Associations {
        warnings: covering(evidence.warnings),
        probsevere: covering(evidence.probsevere),
        ..Default::default()
    };
    for (i, circulation) in evidence.circulations.iter().enumerate() {
        let d = separation(cell, circulation);
        if d > MAX_CIRCULATION_KM {
            continue;
        }
        let other = evidence
            .cells
            .iter()
            .filter(|c| c.id != cell.id)
            .map(|c| separation(c, circulation))
            .fold(f64::INFINITY, f64::min);
        if (other - d).abs() <= AMBIGUITY_KM {
            result.ambiguous += 1;
        } else if d < other {
            result.circulations.push((i, d));
        }
    }
    result
        .circulations
        .sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use wxdata::{
        overlay::FeatureKind,
        tornado_id::{Tier, TornadoId},
    };

    fn cell(id: &str, lon: f64) -> Cell {
        Cell {
            id: id.into(),
            lon,
            lat: 35.0,
            ..Default::default()
        }
    }

    fn circulation(lon: f64) -> Circulation {
        Circulation {
            id: TornadoId {
                lon,
                lat: 35.0,
                tier: Tier::Possible,
                score: 0.5,
                vrot_ms: None,
                min_cc: None,
                reasons: vec![],
            },
            members: vec![],
        }
    }

    fn evidence<'a>(cells: &'a [Cell], circulations: &'a [Circulation]) -> Evidence<'a> {
        Evidence {
            cells,
            circulations,
            warnings: &[],
            probsevere: &[],
        }
    }

    #[test]
    fn a_signature_belongs_only_to_the_nearest_core() {
        let cells = [cell("A", -97.0), cell("B", -96.96)];
        let circulations = [circulation(-96.96)];
        let sources = evidence(&cells, &circulations);
        assert!(associate(&cells[0], &sources).circulations.is_empty());
        assert_eq!(associate(&cells[1], &sources).circulations, [(0, 0.0)]);
    }

    #[test]
    fn nearly_equal_matches_remain_ambiguous() {
        let cells = [cell("A", -97.005), cell("B", -96.995)];
        let circulations = [circulation(-97.0)];
        let sources = evidence(&cells, &circulations);
        for c in &cells {
            let result = associate(c, &sources);
            assert!(result.circulations.is_empty());
            assert_eq!(result.ambiguous, 1);
        }
    }

    #[test]
    fn distant_circulations_stay_unassociated() {
        let cells = [cell("A", -97.0)];
        let circulations = [circulation(-96.0)];
        let result = associate(&cells[0], &evidence(&cells, &circulations));
        assert!(result.circulations.is_empty());
        assert_eq!(result.ambiguous, 0);
    }

    #[test]
    fn warning_and_probsevere_coverage_respect_holes_and_preserve_every_source() {
        let outer = vec![[-97.2, 34.8], [-96.8, 34.8], [-96.8, 35.2], [-97.2, 35.2]];
        let hole = vec![[-97.1, 34.9], [-96.9, 34.9], [-96.9, 35.1], [-97.1, 35.1]];
        let polygon = GeoFeature {
            rings: vec![outer],
            fill: [0; 4],
            stroke: [0; 4],
            kind: FeatureKind::Warning,
            title: "source".into(),
            detail: "provider record".into(),
            alert: None,
        };
        let mut excluded = polygon.clone();
        excluded.rings.push(hole);
        let features = [excluded, polygon.clone(), polygon];
        let c = cell("A", -97.0);
        let sources = Evidence {
            cells: std::slice::from_ref(&c),
            circulations: &[],
            warnings: &features,
            probsevere: &features,
        };
        let result = associate(&c, &sources);
        assert_eq!(result.warnings, [1, 2]);
        assert_eq!(result.probsevere, [1, 2]);
    }
}
