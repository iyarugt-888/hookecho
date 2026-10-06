//! Persistent storm identity (ROADMAP_PARITY M2.1, increment 1): one history per radar, built
//! volume by volume from whatever storm objects a source reports (SCIT cells, locally tracked
//! reflectivity cells), giving each storm a stable local ID and keeping how every observation was
//! tied to it.
//!
//! What it keeps and why:
//!
//! * **Observations by reference.** A storm holds [`ObservationRef`]s (source, site, provider ID,
//!   time, position) to the original source objects, not copies of their attributes, so the
//!   Inspector can always go back to what the source said.
//! * **The evidence for every link.** Each association records the distance from where the storm
//!   was predicted to be, the time gap, which motion predicted it (the storm's own track, the
//!   provider's declared motion, or none), the competing storms within reach, and whether it is
//!   confirmed or tentative.
//! * **Provider IDs are not identities.** SCIT recycles its two-character IDs; a reused ID far
//!   from where its old storm could be is recorded as a recycled ID, not a match.
//! * **Lineage.** A new storm born beside one that was continued is recorded as split from it; a
//!   storm that vanishes beside one that continued is recorded as merged into it. Ambiguity stays
//!   ambiguous: alternatives are kept, never resolved by forcing a merge.
//! * **Determinism.** The same updates replay to the same identities: candidates are ordered by
//!   distance, then storm ID, then source order. An update older than the history (a backward
//!   archive seek) restarts it, so seeking back and replaying forward rebuilds the same storms.
//!
//! The limits ([`AssociationParams`]) are versioned by [`ASSOCIATION_VERSION`] and replayed on two
//! pinned real days of SCIT in `tests/scit_history.rs`, which records how often the history keeps
//! SCIT's own continued cells together and whether it ever makes two of SCIT's cells one storm.

/// Version of the association rules and limits, recorded with anything derived from them.
pub const ASSOCIATION_VERSION: &str = "storm-history-2";

/// Where a storm observation came from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// A NEXRAD Level 3 SCIT storm cell.
    Scit,
    /// A cell tracked locally from reflectivity (`celltrack`), for radars without SCIT.
    LocalCell,
}

/// One storm object as a source reported it, by reference: enough to find it again and place it.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservationRef {
    pub source: Source,
    pub site: String,
    /// The provider's own ID (SCIT's "O7"); `None` when the source has none.
    pub provider_id: Option<String>,
    /// The observation time, seconds since the Unix epoch.
    pub time: i64,
    pub lon: f64,
    pub lat: f64,
    /// Motion the provider declares (m/s east, north), if any.
    pub provider_motion_ms: Option<(f64, f64)>,
}

/// A stable local storm ID: unique within one history, never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StormId(pub u64);

/// What predicted where a storm would be for an association.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionSource {
    /// The storm's own last two observations.
    Track,
    /// The motion the provider declared with its last observation.
    Provider,
    /// No motion known: its last position.
    Stationary,
}

/// How sure an association is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// The nearest storm, clearly nearer than any other.
    Confirmed,
    /// Another storm was within [`AssociationParams::ambiguity_km`] of the same distance, or the
    /// link rests on a long gap.
    Tentative,
}

/// The evidence for tying one observation to one storm.
#[derive(Debug, Clone, PartialEq)]
pub struct Association {
    pub storm: StormId,
    /// Index of the observation in the update it came with.
    pub observation: usize,
    /// Distance (km) from where the storm was predicted to be.
    pub distance_km: f64,
    /// Seconds since the storm was last seen.
    pub gap_s: i64,
    pub motion: MotionSource,
    pub confidence: Confidence,
    /// Other storms within reach of this observation, with their distances (km), nearest first.
    pub alternatives: Vec<(StormId, f64)>,
    /// What else is worth knowing: a provider ID change, a recycled ID declined.
    pub notes: Vec<String>,
}

/// How a storm began or ended relative to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lineage {
    /// Born beside a storm that was continued in the same update.
    SplitFrom(StormId),
    /// Vanished beside a storm that continued in the same update.
    MergedInto(StormId),
}

/// One storm's history.
#[derive(Debug, Clone, PartialEq)]
pub struct Storm {
    pub id: StormId,
    /// Every observation tied to it, oldest first.
    pub observations: Vec<ObservationRef>,
    /// The evidence for each observation after the first, in the same order.
    pub associations: Vec<Association>,
    pub lineage: Vec<Lineage>,
    /// What is worth knowing about how it began: a recycled provider ID declined.
    pub notes: Vec<String>,
    /// Closed: not seen for longer than [`AssociationParams::max_gap_s`], or merged.
    pub closed: bool,
}

impl Storm {
    fn last(&self) -> &ObservationRef {
        self.observations
            .last()
            .expect("a storm has at least one observation")
    }

    /// Where it would be at `time`, and which motion said so.
    fn predict(&self, time: i64) -> ((f64, f64), MotionSource) {
        let last = self.last();
        let dt = (time - last.time) as f64;
        let (motion, source) = if let [.., a, b] = self.observations.as_slice() {
            let span = (b.time - a.time) as f64;
            if span > 0.0 {
                let (dx, dy) = km_offset((a.lon, a.lat), (b.lon, b.lat));
                (
                    (dx * 1000.0 / span, dy * 1000.0 / span),
                    MotionSource::Track,
                )
            } else {
                ((0.0, 0.0), MotionSource::Stationary)
            }
        } else if let Some(m) = last.provider_motion_ms {
            (m, MotionSource::Provider)
        } else {
            ((0.0, 0.0), MotionSource::Stationary)
        };
        let lat_rad = last.lat.to_radians();
        let lon = last.lon + motion.0 * dt / 1000.0 / (111.32 * lat_rad.cos());
        let lat = last.lat + motion.1 * dt / 1000.0 / 110.57;
        ((lon, lat), source)
    }
}

/// The association limits. A first guess for 5-minute volumes; see [`ASSOCIATION_VERSION`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AssociationParams {
    /// Reach (km) around a prediction at no gap.
    pub base_km: f64,
    /// How far (m/s) a storm may stray from its prediction per second of gap.
    pub deviation_ms: f64,
    /// Longest gap (s) a storm survives unseen.
    pub max_gap_s: i64,
    /// Two storms this close (km) to the same distance from an observation make it ambiguous.
    pub ambiguity_km: f64,
    /// A gap (s) past which a link is only tentative.
    pub tentative_gap_s: i64,
    /// The fastest a storm is taken to move (m/s): how far a provider may say its cell went and
    /// still be believed that it is the same cell, whatever this history predicted.
    pub provider_speed_ms: f64,
}

impl Default for AssociationParams {
    fn default() -> Self {
        AssociationParams {
            base_km: 5.0,
            deviation_ms: 10.0,
            max_gap_s: 15 * 60,
            ambiguity_km: 2.0,
            tentative_gap_s: 10 * 60,
            provider_speed_ms: 40.0,
        }
    }
}

impl AssociationParams {
    fn reach_km(&self, gap_s: i64) -> f64 {
        self.base_km + self.deviation_ms * gap_s.max(0) as f64 / 1000.0
    }

    /// How far from its last position a storm's own provider ID may reappear and still be it.
    fn provider_reach_km(&self, gap_s: i64) -> f64 {
        self.base_km + self.provider_speed_ms * gap_s.max(0) as f64 / 1000.0
    }
}

/// One radar's storm history.
#[derive(Debug, Clone, Default)]
pub struct StormHistory {
    pub params: AssociationParams,
    storms: Vec<Storm>,
    next_id: u64,
    latest: Option<i64>,
    /// How many times an update older than the history restarted it.
    pub restarts: usize,
}

/// What one update did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UpdateReport {
    /// The storm each observation was tied to or started, in observation order.
    pub storms: Vec<StormId>,
    /// Observations that started a storm.
    pub new: Vec<usize>,
    /// Whether the update restarted the history (it was older than the latest one).
    pub restarted: bool,
}

impl StormHistory {
    pub fn new(params: AssociationParams) -> Self {
        StormHistory {
            params,
            ..Default::default()
        }
    }

    /// Every storm, open and closed, in ID order.
    pub fn storms(&self) -> &[Storm] {
        &self.storms
    }

    /// The storm with this ID.
    pub fn storm(&self, id: StormId) -> Option<&Storm> {
        self.storms.iter().find(|s| s.id == id)
    }

    /// Add one volume's observations, all at `time` (seconds since the epoch).
    pub fn update(&mut self, time: i64, observations: &[ObservationRef]) -> UpdateReport {
        let mut report = UpdateReport::default();
        if self.latest.is_some_and(|t| time < t) {
            // A backward seek: what was built from later volumes does not describe this one.
            let params = self.params;
            let restarts = self.restarts + 1;
            *self = StormHistory::new(params);
            self.restarts = restarts;
            report.restarted = true;
        }
        self.latest = Some(time);
        let p = self.params;

        // Close storms unseen for too long; they take no part in matching.
        for s in &mut self.storms {
            if !s.closed && time - s.last().time > p.max_gap_s {
                s.closed = true;
            }
        }
        let open: Vec<usize> = (0..self.storms.len())
            .filter(|&i| !self.storms[i].closed && self.storms[i].last().time < time)
            .collect();
        let predicted: Vec<((f64, f64), MotionSource)> =
            open.iter().map(|&i| self.storms[i].predict(time)).collect();
        // The provider IDs the open storms carried before this update, to tell a recycled ID.
        let previous_ids: Vec<(String, StormId)> = open
            .iter()
            .filter_map(|&i| {
                let s = &self.storms[i];
                s.last().provider_id.clone().map(|p| (p, s.id))
            })
            .collect();

        // Every observation-storm pair within reach. The distance is from the storm's prediction;
        // for a storm with no motion of its own yet, also from the observation carried back along
        // the motion its provider now declares for it, whichever is nearer — a new cell moving at
        // 25 m/s is otherwise out of reach of its own first position by the next volume.
        let mut pairs: Vec<(f64, StormId, usize, usize)> = Vec::new();
        for (o, obs) in observations.iter().enumerate() {
            for (k, &i) in open.iter().enumerate() {
                let s = &self.storms[i];
                let last = s.last();
                let gap = time - last.time;
                let mut d = ground_km(predicted[k].0, (obs.lon, obs.lat));
                if predicted[k].1 == MotionSource::Stationary {
                    if let Some((u, v)) = obs.provider_motion_ms {
                        let lat_rad = obs.lat.to_radians();
                        let back = (
                            obs.lon - u * gap as f64 / 1000.0 / (111.32 * lat_rad.cos()),
                            obs.lat - v * gap as f64 / 1000.0 / 110.57,
                        );
                        d = d.min(ground_km((last.lon, last.lat), back));
                    }
                }
                if d <= p.reach_km(gap) {
                    pairs.push((d, s.id, k, o));
                }
            }
        }
        // Nearest first; ties by storm ID, then observation order, so replay is deterministic.
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.3.cmp(&b.3)));
        let candidates = |o: usize| -> Vec<(StormId, f64)> {
            pairs
                .iter()
                .filter(|q| q.3 == o)
                .map(|q| (q.1, q.0))
                .collect()
        };

        let mut taken_obs = vec![false; observations.len()];
        let mut taken_storm = vec![false; open.len()];
        let mut links: Vec<(usize, usize, f64)> = Vec::new();
        // The provider's own continuity first: an observation carrying the ID its storm last
        // had, from the same source and site, is that storm while it is no farther from the
        // storm's last position than the fastest storm moves in the gap — the provider tracked
        // it with more than positions. Only then are the rest linked nearest first, so a dense
        // field does not hand a storm to a neighbour that happened to land closer to its
        // prediction. An ID beyond that is refused as recycled, below.
        let mut continued: Vec<(f64, usize, usize)> = Vec::new();
        for (o, obs) in observations.iter().enumerate() {
            let Some(pid) = obs.provider_id.as_ref() else {
                continue;
            };
            for (k, &i) in open.iter().enumerate() {
                let last = self.storms[i].last();
                if last.provider_id.as_ref() != Some(pid)
                    || last.source != obs.source
                    || last.site != obs.site
                {
                    continue;
                }
                let moved = ground_km((last.lon, last.lat), (obs.lon, obs.lat));
                if moved <= p.provider_reach_km(time - last.time) {
                    let d = ground_km(predicted[k].0, (obs.lon, obs.lat));
                    continued.push((d, k, o));
                }
            }
        }
        continued.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        for &(d, k, o) in &continued {
            if taken_obs[o] || taken_storm[k] {
                continue;
            }
            taken_obs[o] = true;
            taken_storm[k] = true;
            links.push((k, o, d));
        }
        for &(d, _, k, o) in &pairs {
            if taken_obs[o] || taken_storm[k] {
                continue;
            }
            taken_obs[o] = true;
            taken_storm[k] = true;
            links.push((k, o, d));
        }

        report.storms = vec![StormId(0); observations.len()];
        for &(k, o, d) in &links {
            let i = open[k];
            let obs = &observations[o];
            let gap = time - self.storms[i].last().time;
            let id = self.storms[i].id;
            let alternatives: Vec<(StormId, f64)> = candidates(o)
                .into_iter()
                .filter(|(s, _)| *s != id)
                .collect();
            let ambiguous = alternatives
                .first()
                .is_some_and(|(_, other)| other - d <= p.ambiguity_km);
            let mut notes = Vec::new();
            if let (Some(was), Some(now)) = (&self.storms[i].last().provider_id, &obs.provider_id) {
                if was != now {
                    notes.push(format!("provider ID changed from {was} to {now}"));
                }
            }
            if ambiguous {
                notes.push(format!(
                    "{} other storm(s) within {:.1} km of the same distance",
                    alternatives
                        .iter()
                        .filter(|(_, x)| x - d <= p.ambiguity_km)
                        .count(),
                    p.ambiguity_km
                ));
            }
            let confidence = if ambiguous || gap > p.tentative_gap_s {
                Confidence::Tentative
            } else {
                Confidence::Confirmed
            };
            let association = Association {
                storm: id,
                observation: o,
                distance_km: d,
                gap_s: gap,
                motion: predicted[k].1,
                confidence,
                alternatives,
                notes,
            };
            let s = &mut self.storms[i];
            s.observations.push(obs.clone());
            s.associations.push(association);
            report.storms[o] = id;
        }

        // Unmatched observations start storms. One within reach of a storm that was continued
        // this update split from it; a reused provider ID is recorded as recycled.
        for (o, obs) in observations.iter().enumerate() {
            if taken_obs[o] {
                continue;
            }
            // IDs count from 1: a person reads "#1" as the first storm.
            self.next_id += 1;
            let id = StormId(self.next_id);
            let mut lineage = Vec::new();
            if let Some((parent, _)) = candidates(o).first() {
                lineage.push(Lineage::SplitFrom(*parent));
            }
            let mut notes = Vec::new();
            if let Some((pid, was)) = obs
                .provider_id
                .as_ref()
                .and_then(|pid| previous_ids.iter().find(|(p, _)| p == pid))
            {
                notes.push(format!(
                    "provider ID {pid} was storm {}'s, out of reach: recycled, not the same storm",
                    was.0
                ));
            }
            self.storms.push(Storm {
                id,
                observations: vec![obs.clone()],
                associations: Vec::new(),
                lineage,
                notes,
                closed: false,
            });
            report.storms[o] = id;
            report.new.push(o);
        }

        // Unmatched open storms: one whose prediction was within reach of an observation another
        // storm took merged into that storm.
        for (k, &i) in open.iter().enumerate() {
            if taken_storm[k] {
                continue;
            }
            let id = self.storms[i].id;
            if let Some(&(_, _, _, o)) = pairs.iter().find(|q| q.1 == id) {
                let into = report.storms[o];
                if into != id {
                    let s = &mut self.storms[i];
                    s.lineage.push(Lineage::MergedInto(into));
                    s.closed = true;
                }
            }
        }
        report
    }
}

fn km_offset(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let lat = ((a.1 + b.1) / 2.0).to_radians();
    ((b.0 - a.0) * 111.32 * lat.cos(), (b.1 - a.1) * 110.57)
}

fn ground_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (x, y) = km_offset(a, b);
    x.hypot(y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_700_000_000;
    const VOL: i64 = 300;

    /// An observation `x`, `y` km east and north of a point in Oklahoma.
    fn obs(x: f64, y: f64, t: i64, id: Option<&str>) -> ObservationRef {
        let (lon0, lat0) = (-97.5_f64, 35.3_f64);
        ObservationRef {
            source: Source::Scit,
            site: "KTLX".into(),
            provider_id: id.map(str::to_string),
            time: t,
            lon: lon0 + x / (111.32 * lat0.to_radians().cos()),
            lat: lat0 + y / 110.57,
            provider_motion_ms: None,
        }
    }

    /// Two storms moving east at 15 m/s (4.5 km a volume), 20 km apart.
    fn two_storms(h: &mut StormHistory, volumes: i64) -> Vec<UpdateReport> {
        (0..volumes)
            .map(|v| {
                let t = T0 + v * VOL;
                let x = 4.5 * v as f64;
                h.update(
                    t,
                    &[obs(x, 0.0, t, Some("A1")), obs(x, 20.0, t, Some("B2"))],
                )
            })
            .collect()
    }

    #[test]
    fn moving_storms_keep_their_ids_and_record_the_motion_that_predicted_them() {
        let mut h = StormHistory::default();
        let r = two_storms(&mut h, 4);
        assert_eq!(h.storms().len(), 2);
        for later in &r[1..] {
            assert_eq!(later.storms, r[0].storms, "the same two IDs every volume");
            assert!(later.new.is_empty());
        }
        let s = h.storm(r[0].storms[0]).unwrap();
        assert_eq!(s.observations.len(), 4);
        // The second link had no track yet; from the third the storm's own motion predicts it.
        assert_eq!(s.associations[0].motion, MotionSource::Stationary);
        assert_eq!(s.associations[2].motion, MotionSource::Track);
        assert!(
            s.associations[2].distance_km < 0.5,
            "{}",
            s.associations[2].distance_km
        );
        assert!(s
            .associations
            .iter()
            .all(|a| a.confidence == Confidence::Confirmed));
    }

    #[test]
    fn the_same_updates_replay_to_the_same_identities() {
        let mut a = StormHistory::default();
        let mut b = StormHistory::default();
        assert_eq!(two_storms(&mut a, 5), two_storms(&mut b, 5));
        assert_eq!(a.storms(), b.storms());
    }

    #[test]
    fn crossing_storms_follow_their_tracks_not_the_nearest_last_position() {
        // One moves north, one south, crossing between volumes 2 and 3.
        let mut h = StormHistory::default();
        let mut ids = Vec::new();
        for v in 0..5 {
            let t = T0 + v * VOL;
            let y = -9.0 + 4.5 * v as f64;
            ids.push(
                h.update(t, &[obs(0.0, y, t, None), obs(3.0, -y, t, None)])
                    .storms,
            );
        }
        assert!(ids.iter().all(|s| *s == ids[0]), "{ids:?}");
    }

    #[test]
    fn a_missed_update_is_bridged_and_a_long_gap_closes_the_storm() {
        let mut h = StormHistory::default();
        let r0 = h.update(T0, &[obs(0.0, 0.0, T0, None)]);
        h.update(T0 + VOL, &[obs(4.5, 0.0, T0 + VOL, None)]);
        // Nothing for one volume, then seen where its motion said it would be.
        let t = T0 + 3 * VOL;
        let r = h.update(t, &[obs(13.5, 0.0, t, None)]);
        assert_eq!(r.storms, r0.storms);
        let s = h.storm(r0.storms[0]).unwrap();
        assert_eq!(s.associations.last().unwrap().gap_s, 2 * VOL);
        // Twenty minutes unseen: closed, and what appears next is a new storm.
        let t = t + 20 * 60;
        let r = h.update(t, &[obs(40.0, 0.0, t, None)]);
        assert_eq!(r.new, vec![0]);
        assert!(h.storm(r0.storms[0]).unwrap().closed);
    }

    #[test]
    fn a_recycled_provider_id_far_away_is_a_new_storm_not_the_old_one() {
        let mut h = StormHistory::default();
        let r0 = h.update(T0, &[obs(0.0, 0.0, T0, Some("O7"))]);
        let t = T0 + VOL;
        let r = h.update(
            t,
            &[obs(0.5, 0.0, t, Some("K3")), obs(80.0, 0.0, t, Some("O7"))],
        );
        assert_eq!(
            r.storms[0], r0.storms[0],
            "the storm kept its identity under a new ID"
        );
        let kept = h.storm(r0.storms[0]).unwrap();
        assert!(kept.associations[0].notes[0].contains("changed from O7 to K3"));
        let recycled = h.storm(r.storms[1]).unwrap();
        assert_ne!(recycled.id, kept.id);
        assert!(
            recycled.notes[0].contains("recycled"),
            "{:?}",
            recycled.notes
        );
    }

    /// In a dense field a neighbour can land nearer a storm's prediction than the storm's own
    /// cell; the provider's continuity (the same ID, within how far a storm can move) decides.
    #[test]
    fn a_storms_own_provider_id_wins_over_a_nearer_neighbour() {
        let mut h = StormHistory::default();
        let r0 = h.update(
            T0,
            &[obs(0.0, 0.0, T0, Some("A1")), obs(0.0, 6.0, T0, Some("B2"))],
        );
        // Next volume: A1 moved 3 km south, B2 moved 5 km south — to 1 km, nearer A1's
        // (stationary) prediction than A1 itself is.
        let t1 = T0 + VOL;
        let r1 = h.update(
            t1,
            &[
                obs(0.0, -3.0, t1, Some("A1")),
                obs(0.0, 1.0, t1, Some("B2")),
            ],
        );
        assert_eq!(r1.storms, r0.storms, "each storm keeps its own cell");
    }

    /// A new cell moving fast has no track of its own yet; the motion its provider now declares
    /// carries it back to where it was, so it is not lost after one volume.
    #[test]
    fn a_fast_new_cell_is_found_by_its_declared_motion() {
        let mut h = StormHistory::default();
        let r0 = h.update(T0, &[obs(0.0, 0.0, T0, None)]);
        // 25 m/s east for a 7-minute gap: 10.5 km, past the stationary reach.
        let t1 = T0 + 420;
        let mut moved = obs(10.5, 0.0, t1, None);
        moved.provider_motion_ms = Some((25.0, 0.0));
        let r1 = h.update(t1, &[moved.clone()]);
        assert_eq!(r1.storms, r0.storms);
        // Without the declared motion it would have been a new storm.
        let mut h2 = StormHistory::default();
        h2.update(T0, &[obs(0.0, 0.0, T0, None)]);
        moved.provider_motion_ms = None;
        assert_ne!(h2.update(t1, &[moved]).storms, r0.storms);
    }

    #[test]
    fn a_split_and_a_merge_are_recorded_as_lineage() {
        let mut h = StormHistory::default();
        let r0 = h.update(T0, &[obs(0.0, 0.0, T0, None)]);
        // It splits: two cells 3 km apart where one was.
        let t1 = T0 + VOL;
        let r1 = h.update(t1, &[obs(0.0, 0.5, t1, None), obs(0.0, -3.0, t1, None)]);
        assert_eq!(r1.storms[0], r0.storms[0]);
        let child = h.storm(r1.storms[1]).unwrap();
        assert_eq!(child.lineage, vec![Lineage::SplitFrom(r0.storms[0])]);
        // They merge back: one cell between them.
        let t2 = T0 + 2 * VOL;
        let r2 = h.update(t2, &[obs(0.0, 0.0, t2, None)]);
        let survivor = r2.storms[0];
        let other = if survivor == r0.storms[0] {
            r1.storms[1]
        } else {
            r0.storms[0]
        };
        let gone = h.storm(other).unwrap();
        assert!(gone.closed);
        assert_eq!(gone.lineage.last(), Some(&Lineage::MergedInto(survivor)));
    }

    #[test]
    fn two_storms_equally_near_leave_the_link_tentative_with_its_alternative() {
        let mut h = StormHistory::default();
        h.update(T0, &[obs(0.0, -2.0, T0, None), obs(0.0, 2.0, T0, None)]);
        let t = T0 + VOL;
        // One observation midway: whichever storm takes it, the other is a near-equal alternative.
        let r = h.update(t, &[obs(0.0, 0.2, t, None)]);
        let s = h.storm(r.storms[0]).unwrap();
        let a = s.associations.last().unwrap();
        assert_eq!(a.confidence, Confidence::Tentative);
        assert_eq!(a.alternatives.len(), 1);
    }

    #[test]
    fn a_backward_seek_restarts_the_history_and_replays_the_same_way() {
        let mut h = StormHistory::default();
        let first = two_storms(&mut h, 3);
        // Seek back to the start and play forward again.
        let again = two_storms(&mut h, 3);
        assert!(again[0].restarted);
        assert_eq!(h.restarts, 1);
        assert_eq!(first, {
            let mut r = again.clone();
            r[0].restarted = false;
            r
        });
    }
}
