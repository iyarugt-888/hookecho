//! Spatial evidence for a SCIT storm. Source objects stay separate: containment is coverage,
//! and a nearby radar signature is a tentative association rather than a shared identity.

use wxdata::storm_evidence::{point_links, EvidenceParams, Relation};
use wxdata::storm_history::StormId;
use wxdata::{level3::Cell, overlay::GeoFeature, tornado_id::Circulation};

pub(super) const MAX_CIRCULATION_KM: f64 = 10.0;
/// Two cores nearly equally close to one signature do not establish which storm owns it.
const AMBIGUITY_KM: f64 = 1.0;

pub(super) fn params() -> EvidenceParams {
    EvidenceParams {
        reach_km: MAX_CIRCULATION_KM,
        ambiguity_km: AMBIGUITY_KM,
    }
}

/// A circulation's position as the storm evidence reads it: its centre and every member.
pub(super) fn circulation_points(circulation: &Circulation) -> Vec<[f64; 2]> {
    std::iter::once([circulation.id.lon, circulation.id.lat])
        .chain(circulation.members.iter().map(|m| [m.lon, m.lat]))
        .collect()
}

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
    // The same rule the storm evidence record uses (`wxdata::storm_evidence::point_links`), with
    // this cell first and every other current core after it.
    let storms: Vec<(StormId, [f64; 2])> = std::iter::once([cell.lon, cell.lat])
        .chain(
            evidence
                .cells
                .iter()
                .filter(|c| c.id != cell.id)
                .map(|c| [c.lon, c.lat]),
        )
        .enumerate()
        .map(|(i, at)| (StormId(i as u64), at))
        .collect();
    for (i, circulation) in evidence.circulations.iter().enumerate() {
        let links = point_links(params(), &storms, &circulation_points(circulation));
        match links.iter().find(|(s, _)| *s == StormId(0)).map(|l| &l.1) {
            Some(Relation::Nearest { km }) => result.circulations.push((i, *km)),
            Some(Relation::Ambiguous { .. }) => result.ambiguous += 1,
            _ => {}
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

    #[test]
    fn the_card_and_the_evidence_record_link_by_the_same_limits() {
        assert_eq!(params(), wxdata::storm_evidence::EvidenceParams::default());
    }
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
                terms: vec![],
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
