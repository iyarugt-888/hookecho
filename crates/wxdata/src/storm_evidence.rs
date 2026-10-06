//! What each tracked storm has been linked to over time (ROADMAP_PARITY M2.1): warnings,
//! ProbSevere objects, fused tornado detections and hail attributes, by reference to the source
//! object, beside the storm's identity history ([`crate::storm_history`]).
//!
//! Every link says how it was made — the storm's position inside a polygon (coverage: a warning can
//! cover several storms and that is not a shared identity), the nearest storm within reach of a
//! point signature, or the storm's own attribute — and a point signature nearly equally close to two
//! storms is recorded on both as ambiguous, naming the other, instead of being given to either.
//! One source object seen on several scans is one track with its first and last scan and a sample
//! per scan, so a ProbSevere probability or a detection tier can be read as it changed.
//!
//! Like the history, an update older than the last one restarts the record, so a backward seek
//! replays to the same evidence.

use crate::storm_history::StormId;
use std::collections::BTreeMap;

/// What kind of source object a link is to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvidenceKind {
    Warning,
    ProbSevere,
    /// A fused tornado detection (circulation).
    TornadoDetection,
    /// Hail attributes the storm's own SCIT cell carried.
    Hail,
}

impl EvidenceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::ProbSevere => "ProbSevere",
            Self::TornadoDetection => "tornado detection",
            Self::Hail => "hail",
        }
    }
}

/// One source object at one scan.
#[derive(Debug, Clone, PartialEq)]
pub struct EvidenceObject {
    pub kind: EvidenceKind,
    /// The source's own identity: an alert's event key, a ProbSevere object's title, a
    /// detection's rounded position, a SCIT cell ID. Not a storm identity.
    pub source_id: String,
    /// What it said at this scan, as the source said it ("Tornado Warning", "PROB 82%").
    pub detail: String,
    /// When the source says this reading was valid (a detection's volume time), if it says.
    pub valid: Option<i64>,
    pub shape: Shape,
}

/// Where a source object is.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// A polygon (rings `[lon, lat]`, ring 0 outer, others holes): links by containment.
    Polygon(Vec<Vec<[f64; 2]>>),
    /// A point signature, as its points (a circulation's centre and members): links to the
    /// storm nearest any of them, within reach.
    Points(Vec<[f64; 2]>),
    /// An attribute of one storm's own observation (by its index in the update's storm list).
    Own(usize),
}

/// How a link was made.
#[derive(Debug, Clone, PartialEq)]
pub enum Relation {
    /// The storm's position is inside the polygon.
    Covers,
    /// The nearest storm to a point, `km` away.
    Nearest { km: f64 },
    /// Equally close to this storm and the others named (`km` each): not attributed to any.
    Ambiguous {
        km: f64,
        others: Vec<(StormId, f64)>,
    },
    /// The storm's own observation carried it.
    Own,
}

/// One scan's reading of a track.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// The storm scan this was linked at.
    pub time: i64,
    /// The source's own valid time, when it gives one.
    pub valid: Option<i64>,
    pub detail: String,
    pub relation: Relation,
}

/// One source object's links to one storm across scans.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub kind: EvidenceKind,
    pub source_id: String,
    pub samples: Vec<Sample>,
}

impl Track {
    pub fn first(&self) -> i64 {
        self.samples.first().map_or(0, |s| s.time)
    }

    pub fn last(&self) -> i64 {
        self.samples.last().map_or(0, |s| s.time)
    }

    /// Whether every sample so far is ambiguous.
    pub fn only_ambiguous(&self) -> bool {
        self.samples
            .iter()
            .all(|s| matches!(s.relation, Relation::Ambiguous { .. }))
    }
}

/// Limits, versioned with the evidence they produce.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvidenceParams {
    /// Farthest a point signature may be from a storm and still be linked to it, km.
    pub reach_km: f64,
    /// Two storms within this many km of each other's distance to a point are equally close.
    pub ambiguity_km: f64,
}

impl Default for EvidenceParams {
    fn default() -> Self {
        // The spatial-association limits the Cell window already uses for circulations.
        EvidenceParams {
            reach_km: 10.0,
            ambiguity_km: 1.0,
        }
    }
}

pub const VERSION: &str = "storm-evidence-1";

/// The evidence record for one radar's storms.
#[derive(Debug, Clone, Default)]
pub struct StormEvidence {
    pub params: EvidenceParams,
    tracks: BTreeMap<StormId, Vec<Track>>,
    last: Option<i64>,
    pub restarts: usize,
}

fn contains(rings: &[Vec<[f64; 2]>], lon: f64, lat: f64) -> bool {
    use crate::overlay::point_in_ring;
    match rings.split_first() {
        Some((outer, holes)) => {
            point_in_ring(outer, lon, lat) && !holes.iter().any(|h| point_in_ring(h, lon, lat))
        }
        None => false,
    }
}

/// Great-circle distance, km (haversine, mean earth radius).
fn km(a: [f64; 2], b: [f64; 2]) -> f64 {
    let (p0, p1) = (a[1].to_radians(), b[1].to_radians());
    let (dp, dl) = (p1 - p0, (b[0] - a[0]).to_radians());
    let h = (dp / 2.0).sin().powi(2) + p0.cos() * p1.cos() * (dl / 2.0).sin().powi(2);
    2.0 * 6371.0088 * h.sqrt().asin()
}

impl StormEvidence {
    pub fn new(params: EvidenceParams) -> Self {
        StormEvidence {
            params,
            ..Default::default()
        }
    }

    /// The tracks linked to `storm`, most recently seen first.
    pub fn of(&self, storm: StormId) -> Vec<&Track> {
        let mut t: Vec<&Track> = self
            .tracks
            .get(&storm)
            .map(|v| v.iter().collect())
            .unwrap_or_default();
        t.sort_by(|a, b| {
            b.last()
                .cmp(&a.last())
                .then(a.kind.cmp(&b.kind))
                .then(a.source_id.cmp(&b.source_id))
        });
        t
    }

    /// Record one scan: `storms` are this scan's storms and positions (as the history assigned
    /// them), `objects` the source objects current at `time`. An object linked to a storm twice
    /// in one scan (the same source ID) keeps the first reading, so callers order objects
    /// strongest first.
    pub fn update(
        &mut self,
        time: i64,
        storms: &[(StormId, [f64; 2])],
        objects: &[EvidenceObject],
    ) {
        if self.last.is_some_and(|l| time < l) {
            *self = StormEvidence {
                params: self.params,
                restarts: self.restarts + 1,
                ..Default::default()
            };
        }
        if self.last == Some(time) {
            // The same scan again: replace what it recorded rather than doubling it.
            for tracks in self.tracks.values_mut() {
                for t in tracks.iter_mut() {
                    t.samples.retain(|s| s.time != time);
                }
                tracks.retain(|t| !t.samples.is_empty());
            }
        }
        self.last = Some(time);
        for o in objects {
            for (storm, relation) in self.links(storms, o) {
                let tracks = self.tracks.entry(storm).or_default();
                let sample = Sample {
                    time,
                    valid: o.valid,
                    detail: o.detail.clone(),
                    relation,
                };
                match tracks
                    .iter_mut()
                    .find(|t| t.kind == o.kind && t.source_id == o.source_id)
                {
                    // One reading per object per scan: the first given (a multi-part polygon, or
                    // several detections in one track) wins.
                    Some(t) if t.samples.last().is_some_and(|s| s.time == time) => {}
                    Some(t) => t.samples.push(sample),
                    None => tracks.push(Track {
                        kind: o.kind,
                        source_id: o.source_id.clone(),
                        samples: vec![sample],
                    }),
                }
            }
        }
    }

    fn links(
        &self,
        storms: &[(StormId, [f64; 2])],
        o: &EvidenceObject,
    ) -> Vec<(StormId, Relation)> {
        match &o.shape {
            Shape::Polygon(rings) => storms
                .iter()
                .filter(|(_, at)| contains(rings, at[0], at[1]))
                .map(|(s, _)| (*s, Relation::Covers))
                .collect(),
            Shape::Own(i) => storms
                .get(*i)
                .map(|(s, _)| vec![(*s, Relation::Own)])
                .unwrap_or_default(),
            Shape::Points(p) => point_links(self.params, storms, p),
        }
    }
}

/// The links a point signature makes: to the storm nearest any of its points when that is within
/// reach and no other storm is within [`EvidenceParams::ambiguity_km`] of as close; otherwise to
/// every such equally close storm within reach, each as ambiguous. Equal closeness is judged
/// against every storm, in reach or not, so a storm just past the reach still keeps a signature
/// from being given to its neighbour.
pub fn point_links(
    params: EvidenceParams,
    storms: &[(StormId, [f64; 2])],
    points: &[[f64; 2]],
) -> Vec<(StormId, Relation)> {
    let mut near: Vec<(StormId, f64)> = storms
        .iter()
        .map(|(s, at)| {
            let d = points
                .iter()
                .map(|p| km(*p, *at))
                .filter(|d| d.is_finite())
                .fold(f64::INFINITY, f64::min);
            (*s, d)
        })
        .filter(|(_, d)| d.is_finite())
        .collect();
    near.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let Some(&(best, d0)) = near.first() else {
        return Vec::new();
    };
    if d0 > params.reach_km {
        return Vec::new();
    }
    let tied: Vec<(StormId, f64)> = near
        .iter()
        .copied()
        .filter(|(_, d)| d - d0 <= params.ambiguity_km)
        .collect();
    if tied.len() == 1 {
        return vec![(best, Relation::Nearest { km: d0 })];
    }
    tied.iter()
        .filter(|(_, d)| *d <= params.reach_km)
        .map(|&(s, d)| {
            let others = tied.iter().copied().filter(|(o, _)| *o != s).collect();
            (s, Relation::Ambiguous { km: d, others })
        })
        .collect()
}

/// One line per track for a reader: what, which object, since when, and how it is linked.
pub fn describe(track: &Track, fmt_time: impl Fn(i64) -> String) -> String {
    let latest = track.samples.last();
    let how = match latest.map(|s| &s.relation) {
        Some(Relation::Covers) => "covers it".to_string(),
        Some(Relation::Nearest { km }) => format!("{km:.1} km away"),
        Some(Relation::Ambiguous { km, others }) => format!(
            "{km:.1} km away, as close to {}: not attributed",
            others
                .iter()
                .map(|(s, d)| format!("#{} ({d:.1} km)", s.0))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Some(Relation::Own) => "its own cell".to_string(),
        None => String::new(),
    };
    let span = if track.samples.len() > 1 {
        format!(
            "{}–{}, {} scans",
            fmt_time(track.first()),
            fmt_time(track.last()),
            track.samples.len()
        )
    } else {
        fmt_time(track.last())
    };
    let changed = {
        let mut details: Vec<&str> = Vec::new();
        for s in &track.samples {
            if details.last() != Some(&s.detail.as_str()) {
                details.push(&s.detail);
            }
        }
        if details.len() > 1 {
            details.join(" → ")
        } else {
            latest.map(|s| s.detail.clone()).unwrap_or_default()
        }
    };
    let valid = latest
        .and_then(|s| s.valid.filter(|v| *v != s.time))
        .map(|v| format!("; source valid {}", fmt_time(v)))
        .unwrap_or_default();
    format!(
        "{} {} — {changed} ({span}; {how}{valid})",
        track.kind.label(),
        track.source_id
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(lon: f64, lat: f64, half: f64) -> Vec<[f64; 2]> {
        vec![
            [lon - half, lat - half],
            [lon + half, lat - half],
            [lon + half, lat + half],
            [lon - half, lat + half],
        ]
    }

    fn obj(kind: EvidenceKind, id: &str, detail: &str, shape: Shape) -> EvidenceObject {
        EvidenceObject {
            kind,
            source_id: id.into(),
            detail: detail.into(),
            valid: None,
            shape,
        }
    }

    const A: StormId = StormId(1);
    const B: StormId = StormId(2);

    #[test]
    fn a_polygon_covers_every_storm_inside_it_and_none_in_its_hole() {
        let mut e = StormEvidence::new(EvidenceParams::default());
        let storms = [(A, [-97.0, 35.0]), (B, [-96.85, 35.0])];
        let rings = vec![square(-96.95, 35.0, 0.2), square(-97.0, 35.0, 0.02)];
        e.update(
            100,
            &storms,
            &[obj(
                EvidenceKind::Warning,
                "OUN.TO.W.0042",
                "Tornado Warning",
                Shape::Polygon(rings),
            )],
        );
        assert!(e.of(A).is_empty(), "A sits in the hole");
        let b = e.of(B);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].samples[0].relation, Relation::Covers);
    }

    #[test]
    fn a_signature_goes_to_the_nearest_storm_and_a_tie_to_neither() {
        let mut e = StormEvidence::new(EvidenceParams::default());
        let storms = [(A, [-97.0, 35.0]), (B, [-96.9, 35.0])];
        e.update(
            100,
            &storms,
            &[
                obj(
                    EvidenceKind::TornadoDetection,
                    "near-B",
                    "Likely",
                    Shape::Points(vec![[-96.91, 35.0]]),
                ),
                obj(
                    EvidenceKind::TornadoDetection,
                    "between",
                    "Possible",
                    Shape::Points(vec![[-96.95, 35.0]]),
                ),
                obj(
                    EvidenceKind::TornadoDetection,
                    "far",
                    "Possible",
                    Shape::Points(vec![[-95.0, 35.0]]),
                ),
            ],
        );
        let b = e.of(B);
        let near = b.iter().find(|t| t.source_id == "near-B").unwrap();
        assert!(matches!(near.samples[0].relation, Relation::Nearest { km } if km < 1.0));
        assert!(e.of(A).iter().all(|t| t.source_id != "near-B"));
        for (s, other) in [(A, B), (B, A)] {
            let t = e
                .of(s)
                .into_iter()
                .find(|t| t.source_id == "between")
                .unwrap();
            match &t.samples[0].relation {
                Relation::Ambiguous { others, .. } => assert_eq!(others[0].0, other),
                r => panic!("{r:?}"),
            }
            assert!(t.only_ambiguous());
        }
        assert!(e
            .of(A)
            .iter()
            .chain(e.of(B).iter())
            .all(|t| t.source_id != "far"));
    }

    #[test]
    fn a_storm_just_out_of_reach_still_makes_its_neighbour_ambiguous() {
        let p = EvidenceParams::default();
        // ~9.6 km and ~10.4 km east and west of the signature.
        let storms = [(A, [-97.106, 35.0]), (B, [-96.886, 35.0])];
        let links = point_links(p, &storms, &[[-97.0, 35.0]]);
        assert_eq!(links.len(), 1, "only A is in reach: {links:?}");
        match &links[0] {
            (s, Relation::Ambiguous { km, others }) => {
                assert_eq!(*s, A);
                assert!(*km < p.reach_km && others[0].1 > p.reach_km, "{links:?}");
            }
            l => panic!("{l:?}"),
        }
        // A circulation's members count: the nearest point decides.
        let links = point_links(p, &storms, &[[-97.0, 35.0], [-96.9, 35.0]]);
        assert!(
            matches!(links[..], [(s, Relation::Nearest { .. })] if s == B),
            "{links:?}"
        );
    }

    #[test]
    fn one_object_over_scans_is_one_track_with_its_changes() {
        let mut e = StormEvidence::new(EvidenceParams::default());
        let storms = [(A, [-97.0, 35.0])];
        let ps = |p: &str| {
            obj(
                EvidenceKind::ProbSevere,
                "PS 4321",
                p,
                Shape::Polygon(vec![square(-97.0, 35.0, 0.1)]),
            )
        };
        e.update(100, &storms, &[ps("PROB 62%")]);
        e.update(400, &storms, &[ps("PROB 82%")]);
        e.update(700, &storms, &[ps("PROB 82%")]);
        let t = &e.of(A)[0];
        assert_eq!((t.first(), t.last(), t.samples.len()), (100, 700, 3));
        let line = describe(t, |s| format!("t{s}"));
        assert_eq!(
            line,
            "ProbSevere PS 4321 — PROB 62% → PROB 82% (t100–t700, 3 scans; covers it)"
        );
        // The same scan fed twice is not two samples.
        e.update(700, &storms, &[ps("PROB 85%")]);
        let t = &e.of(A)[0];
        assert_eq!(t.samples.len(), 3);
        assert_eq!(t.samples[2].detail, "PROB 85%");
    }

    #[test]
    fn a_backward_seek_replays_to_the_same_record() {
        let storms = [(A, [-97.0, 35.0]), (B, [-96.9, 35.0])];
        let scans: Vec<(i64, Vec<EvidenceObject>)> = (0..4)
            .map(|k| {
                (
                    300 * k,
                    vec![
                        obj(
                            EvidenceKind::TornadoDetection,
                            "c",
                            &format!("t{k}"),
                            Shape::Points(vec![[-96.92, 35.0]]),
                        ),
                        obj(
                            EvidenceKind::Hail,
                            "O7",
                            &format!("POSH {}%", 10 * k),
                            Shape::Own(0),
                        ),
                    ],
                )
            })
            .collect();
        let run = |e: &mut StormEvidence| {
            for (t, o) in &scans {
                e.update(*t, &storms, o);
            }
        };
        let mut fresh = StormEvidence::new(EvidenceParams::default());
        run(&mut fresh);
        let mut seek = StormEvidence::new(EvidenceParams::default());
        run(&mut seek);
        seek.update(0, &storms, &scans[0].1); // back to the start
        assert_eq!(seek.restarts, 1);
        for (t, o) in &scans[1..] {
            seek.update(*t, &storms, o);
        }
        for s in [A, B] {
            assert_eq!(seek.of(s), fresh.of(s));
        }
        assert_eq!(
            fresh.of(A)[0].kind,
            EvidenceKind::Hail,
            "the own-cell hail goes to storm 0"
        );
    }
}
