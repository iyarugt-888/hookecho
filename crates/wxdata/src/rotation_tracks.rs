//! Rotation columns tracked from volume to volume (detectionplan.md Phase 5).
//!
//! Persistence is evidence. A circulation seen in one volume is as often a passing artifact as a
//! real one, and one that persists and moves with its storm is credible in a way no single volume
//! can be. [`Tracker::update`] takes each new volume's [`RotationColumn`]s and returns them
//! [`Tracked`]: which track each continues, how long it has lasted, how it moves, and how its
//! strength and depth are trending.
//!
//! Association is gated nearest-neighbour, greedy by cost (the plan's acceptable first step before
//! Hungarian matching). A track predicts where it will be from its own motion; a column may
//! continue it only if it turns the same way and is within reach: [`TrackParams::reach_km`] of the
//! prediction, widened to [`TrackParams::max_speed_ms`] of travel for a track with no motion yet.
//! The cost is distance from the prediction, plus how different the shear and depth are, so a
//! strong deep column is not handed a weak shallow neighbour's history.
//!
//! Everything is bounded: a track keeps at most [`TrackParams::max_history`] points and ends after
//! [`TrackParams::max_gap_s`] without a match, so work per volume never grows with the length of
//! the run.

use crate::rotation::Sense;
use crate::rotation_columns::RotationColumn;

/// Version of the tracking rules, recorded with anything derived from them.
pub const ALGORITHM_VERSION: &str = "llsd-tracks-1";

/// How columns are tracked. Provisional, for the backtest to tune.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackParams {
    /// Distance (km) from a track's predicted position a column may be and still continue it.
    pub reach_km: f32,
    /// Fastest a circulation is taken to move (m/s), for a track with no motion estimate yet.
    pub max_speed_ms: f32,
    /// A track with no match for longer than this (s) ends.
    pub max_gap_s: i64,
    /// Points a track keeps.
    pub max_history: usize,
    /// Weight of shear dissimilarity in the cost (km per factor-of-e difference in peak shear).
    pub shear_cost_km: f32,
    /// Weight of depth difference in the cost (km of distance per km of depth).
    pub depth_cost_km: f32,
}

impl Default for TrackParams {
    fn default() -> Self {
        TrackParams {
            reach_km: 3.0,
            max_speed_ms: 35.0,
            max_gap_s: 12 * 60,
            max_history: 12,
            shear_cost_km: 1.0,
            depth_cost_km: 1.0,
        }
    }
}

/// One volume of a track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackPoint {
    /// Volume time, Unix seconds.
    pub time: i64,
    pub lon: f64,
    pub lat: f64,
    pub max_azshear: f32,
    pub low_level_azshear: Option<f32>,
    pub depth_km: f32,
    pub tilts: usize,
    pub rooted: bool,
}

/// A circulation followed through time.
#[derive(Debug, Clone, PartialEq)]
pub struct RotationTrack {
    pub id: u64,
    pub sense: Sense,
    /// Oldest first, at most [`TrackParams::max_history`].
    pub history: Vec<TrackPoint>,
    /// Volumes matched since the track began (not capped like `history`).
    pub volumes: usize,
    /// When the track began, Unix seconds.
    pub started: i64,
}

/// A column with what its track says about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Tracked {
    pub column: RotationColumn,
    pub track_id: u64,
    /// Volumes the track has been seen in, this one included: 1 for a new track.
    pub age_volumes: usize,
    /// Seconds since the track began.
    pub age_seconds: i64,
    /// Motion (m/s, east and north) from the track's recent positions; `None` for a new track.
    pub motion_ms: Option<(f32, f32)>,
    /// How far (km) this column is from where the track predicted it; `None` for a new track.
    pub position_jump_km: Option<f32>,
    /// Least-squares trends over the track's history including this volume, per 10 minutes:
    /// peak AzShear (s⁻¹), low-level AzShear where every point has one (s⁻¹), and depth (km).
    /// `None` with fewer than two points.
    pub azshear_trend: Option<f32>,
    pub low_level_trend: Option<f32>,
    pub depth_trend: Option<f32>,
}

/// Local east/north offsets (km) of `b` from `a`.
fn offset_km(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let lat = ((a.1 + b.1) * 0.5).to_radians();
    ((b.0 - a.0) * 111.32 * lat.cos(), (b.1 - a.1) * 110.57)
}

impl RotationTrack {
    fn last(&self) -> &TrackPoint {
        self.history.last().expect("a track has a point")
    }

    /// Motion (m/s east, north) from the oldest to the newest point of up to the last four.
    fn motion(&self) -> Option<(f32, f32)> {
        let n = self.history.len();
        if n < 2 {
            return None;
        }
        let a = &self.history[n.saturating_sub(4)];
        let b = &self.history[n - 1];
        let dt = (b.time - a.time) as f64;
        if dt <= 0.0 {
            return None;
        }
        let (dx, dy) = offset_km((a.lon, a.lat), (b.lon, b.lat));
        Some(((dx * 1000.0 / dt) as f32, (dy * 1000.0 / dt) as f32))
    }

    /// Where the track should be at `time` (lon, lat).
    fn predict(&self, time: i64) -> (f64, f64) {
        let p = self.last();
        let Some((u, v)) = self.motion() else {
            return (p.lon, p.lat);
        };
        let dt = (time - p.time) as f64;
        let lat = p.lat.to_radians();
        (
            p.lon + u as f64 * dt / 1000.0 / (111.32 * lat.cos()),
            p.lat + v as f64 * dt / 1000.0 / 110.57,
        )
    }
}

/// Least-squares slope of `ys` against `ts` (seconds), per 10 minutes.
fn trend(points: &[(i64, f32)]) -> Option<f32> {
    if points.len() < 2 {
        return None;
    }
    let n = points.len() as f64;
    let mt = points.iter().map(|p| p.0 as f64).sum::<f64>() / n;
    let my = points.iter().map(|p| p.1 as f64).sum::<f64>() / n;
    let (mut sty, mut stt) = (0.0, 0.0);
    for &(t, y) in points {
        sty += (t as f64 - mt) * (y as f64 - my);
        stt += (t as f64 - mt).powi(2);
    }
    (stt > 0.0).then(|| (sty / stt * 600.0) as f32)
}

/// Tracks rotation columns across volumes.
#[derive(Debug, Clone, Default)]
pub struct Tracker {
    pub params: TrackParams,
    pub tracks: Vec<RotationTrack>,
    next_id: u64,
}

impl Tracker {
    pub fn new(params: TrackParams) -> Self {
        Tracker {
            params,
            tracks: Vec::new(),
            next_id: 1,
        }
    }

    /// Take one volume's columns, at `time` (Unix seconds, after every earlier call's), and return
    /// them tracked, in the order given.
    pub fn update(&mut self, time: i64, columns: Vec<RotationColumn>) -> Vec<Tracked> {
        let p = self.params;
        // Tracks unmatched for too long end before this volume is considered.
        self.tracks
            .retain(|t| time - t.last().time <= p.max_gap_s && time > t.last().time);
        // Every admissible pairing and its cost.
        let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
        for (ti, t) in self.tracks.iter().enumerate() {
            let pred = t.predict(time);
            let dt = (time - t.last().time) as f64;
            let reach = if t.motion().is_some() {
                p.reach_km as f64
            } else {
                p.reach_km as f64 + p.max_speed_ms as f64 * dt / 1000.0
            };
            for (ci, c) in columns.iter().enumerate() {
                if c.sense != t.sense {
                    continue;
                }
                let (dx, dy) = offset_km(pred, (c.lon, c.lat));
                let d = dx.hypot(dy);
                if d > reach {
                    continue;
                }
                let last = t.last();
                let shear = (c.max_azshear.max(1e-6) / last.max_azshear.max(1e-6))
                    .ln()
                    .abs();
                let depth = (c.depth_km - last.depth_km).abs();
                let cost = d
                    + p.shear_cost_km as f64 * shear as f64
                    + p.depth_cost_km as f64 * depth as f64;
                pairs.push((cost, ti, ci));
            }
        }
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let mut track_of: Vec<Option<usize>> = vec![None; columns.len()];
        let mut taken = vec![false; self.tracks.len()];
        for (_, ti, ci) in pairs {
            if !taken[ti] && track_of[ci].is_none() {
                taken[ti] = true;
                track_of[ci] = Some(ti);
            }
        }
        let mut out = Vec::with_capacity(columns.len());
        for (ci, c) in columns.into_iter().enumerate() {
            let point = TrackPoint {
                time,
                lon: c.lon,
                lat: c.lat,
                max_azshear: c.max_azshear,
                low_level_azshear: c.low_level_azshear,
                depth_km: c.depth_km,
                tilts: c.tilts(),
                rooted: c.rooted,
            };
            let (ti, jump) = match track_of[ci] {
                Some(ti) => {
                    let pred = self.tracks[ti].predict(time);
                    let (dx, dy) = offset_km(pred, (c.lon, c.lat));
                    let had_motion = self.tracks[ti].motion().is_some();
                    let t = &mut self.tracks[ti];
                    t.history.push(point);
                    if t.history.len() > p.max_history {
                        t.history.remove(0);
                    }
                    t.volumes += 1;
                    (ti, had_motion.then_some(dx.hypot(dy) as f32))
                }
                None => {
                    self.tracks.push(RotationTrack {
                        id: self.next_id,
                        sense: c.sense,
                        history: vec![point],
                        volumes: 1,
                        started: time,
                    });
                    self.next_id += 1;
                    (self.tracks.len() - 1, None)
                }
            };
            let t = &self.tracks[ti];
            let series = |f: &dyn Fn(&TrackPoint) -> Option<f32>| -> Option<f32> {
                let pts: Option<Vec<(i64, f32)>> = t
                    .history
                    .iter()
                    .map(|h| f(h).map(|y| (h.time, y)))
                    .collect();
                pts.and_then(|p| trend(&p))
            };
            out.push(Tracked {
                track_id: t.id,
                age_volumes: t.volumes,
                age_seconds: time - t.started,
                motion_ms: t.motion(),
                position_jump_km: jump,
                azshear_trend: series(&|h| Some(h.max_azshear)),
                low_level_trend: series(&|h| h.low_level_azshear),
                depth_trend: series(&|h| Some(h.depth_km)),
                column: c,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rotation_columns::ColumnMember;

    /// A one-member column of `shear` at `(lon, lat)`, depth `depth` km.
    fn col(lon: f64, lat: f64, shear: f32, depth: f32, sense: Sense) -> RotationColumn {
        RotationColumn {
            lon,
            lat,
            sense,
            members: Vec::<ColumnMember>::new(),
            base_km: 0.5,
            top_km: 0.5 + depth,
            depth_km: depth,
            rooted: true,
            low_level_azshear: Some(shear),
            transition_azshear: None,
            mid_level_azshear: None,
            max_azshear: shear,
            integrated_azshear: 0.0,
            lean_km_per_km: None,
            lean_bearing_deg: None,
        }
    }

    /// Longitude `km` east of -97.0 at 35°N.
    fn east(km: f64) -> f64 {
        -97.0 + km / (111.32 * 35f64.to_radians().cos())
    }

    const VOL: i64 = 300;

    #[test]
    fn a_steadily_moving_circulation_is_one_track() {
        // 15 m/s east: 4.5 km per five-minute volume.
        let mut tr = Tracker::new(TrackParams::default());
        let mut last = None;
        for k in 0..6 {
            let x = 4.5 * k as f64;
            let out = tr.update(
                k * VOL,
                vec![col(
                    east(x),
                    35.0,
                    0.012 + 0.002 * k as f32,
                    1.0,
                    Sense::Cyclonic,
                )],
            );
            assert_eq!(out.len(), 1);
            let t = &out[0];
            if let Some(id) = last {
                assert_eq!(t.track_id, id, "volume {k}");
            }
            last = Some(t.track_id);
            assert_eq!(t.age_volumes, k as usize + 1);
            assert_eq!(t.age_seconds, k * VOL);
            if k >= 2 {
                let (u, v) = t.motion_ms.unwrap();
                assert!((u - 15.0).abs() < 0.5 && v.abs() < 0.5, "{u} {v}");
                assert!(t.position_jump_km.unwrap() < 0.2);
            }
        }
        // Strengthening by 0.002 s⁻¹ every five minutes is 0.004 per ten.
        let out = tr.update(
            6 * VOL,
            vec![col(east(27.0), 35.0, 0.024, 1.0, Sense::Cyclonic)],
        );
        assert!(
            (out[0].azshear_trend.unwrap() - 0.004).abs() < 1e-4,
            "{:?}",
            out[0].azshear_trend
        );
        assert_eq!(out[0].depth_trend, Some(0.0));
    }

    #[test]
    fn a_one_volume_blip_is_a_new_track_and_does_not_linger() {
        let mut tr = Tracker::new(TrackParams::default());
        let a = tr.update(0, vec![col(east(0.0), 35.0, 0.012, 0.5, Sense::Cyclonic)]);
        assert_eq!(
            (a[0].age_volumes, a[0].motion_ms, a[0].azshear_trend),
            (1, None, None)
        );
        tr.update(VOL, vec![]);
        tr.update(2 * VOL, vec![]);
        // Past the gap, something at the same place is a new track.
        let b = tr.update(
            3 * VOL,
            vec![col(east(0.0), 35.0, 0.012, 0.5, Sense::Cyclonic)],
        );
        assert_eq!(b[0].age_volumes, 1);
        assert_ne!(b[0].track_id, a[0].track_id);
        assert_eq!(tr.tracks.len(), 1, "the old track ended");
    }

    #[test]
    fn a_missed_volume_does_not_end_a_track() {
        let mut tr = Tracker::new(TrackParams::default());
        let a = tr.update(0, vec![col(east(0.0), 35.0, 0.015, 1.0, Sense::Cyclonic)]);
        tr.update(VOL, vec![]);
        let b = tr.update(
            2 * VOL,
            vec![col(east(2.0), 35.0, 0.015, 1.0, Sense::Cyclonic)],
        );
        assert_eq!(b[0].track_id, a[0].track_id);
        assert_eq!(b[0].age_volumes, 2);
    }

    #[test]
    fn an_implausible_jump_starts_a_new_track() {
        // 30 km in five minutes is 100 m/s.
        let mut tr = Tracker::new(TrackParams::default());
        let a = tr.update(0, vec![col(east(0.0), 35.0, 0.015, 1.0, Sense::Cyclonic)]);
        let b = tr.update(
            VOL,
            vec![col(east(30.0), 35.0, 0.015, 1.0, Sense::Cyclonic)],
        );
        assert_ne!(a[0].track_id, b[0].track_id);
    }

    #[test]
    fn opposite_senses_never_share_a_track() {
        let mut tr = Tracker::new(TrackParams::default());
        let a = tr.update(0, vec![col(east(0.0), 35.0, 0.015, 1.0, Sense::Cyclonic)]);
        let b = tr.update(
            VOL,
            vec![col(east(0.5), 35.0, 0.015, 1.0, Sense::Anticyclonic)],
        );
        assert_ne!(a[0].track_id, b[0].track_id);
    }

    #[test]
    fn two_circulations_side_by_side_keep_their_identities() {
        // Both moving east at 15 m/s, 6 km apart north-south; the cost favours staying in line.
        let mut tr = Tracker::new(TrackParams::default());
        let north = 35.0 + 6.0 / 110.57;
        let mut ids: Option<(u64, u64)> = None;
        for k in 0..5 {
            let x = 4.5 * k as f64;
            let out = tr.update(
                k * VOL,
                vec![
                    col(east(x), 35.0, 0.02, 2.0, Sense::Cyclonic),
                    col(east(x), north, 0.008, 0.5, Sense::Cyclonic),
                ],
            );
            let now = (out[0].track_id, out[1].track_id);
            if let Some(prev) = ids {
                assert_eq!(now, prev, "volume {k}");
            }
            ids = Some(now);
        }
    }

    #[test]
    fn history_is_bounded() {
        let p = TrackParams::default();
        let mut tr = Tracker::new(p);
        for k in 0..40 {
            tr.update(
                k * VOL,
                vec![col(east(1.0 * k as f64), 35.0, 0.015, 1.0, Sense::Cyclonic)],
            );
        }
        assert_eq!(tr.tracks.len(), 1);
        assert_eq!(tr.tracks[0].history.len(), p.max_history);
        assert_eq!(tr.tracks[0].volumes, 40);
    }

    #[test]
    fn tracking_is_the_same_every_run() {
        let run = || {
            let mut tr = Tracker::new(TrackParams::default());
            (0..5)
                .flat_map(|k| {
                    tr.update(
                        k * VOL,
                        vec![
                            col(east(4.0 * k as f64), 35.0, 0.015, 1.0, Sense::Cyclonic),
                            col(
                                east(4.0 * k as f64 + 2.0),
                                35.02,
                                0.015,
                                1.0,
                                Sense::Cyclonic,
                            ),
                        ],
                    )
                })
                .map(|t| (t.track_id, t.age_volumes))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }
}
