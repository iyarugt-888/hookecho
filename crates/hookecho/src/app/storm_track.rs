//! The manual storm-motion tool (ROADMAP_2 §2.2): drag from a storm to where it will be in an
//! hour, and the projection follows the pointer — configurable time marks, a swath that widens
//! with a cone of uncertainty, and the ETA and closest approach at every saved marker, recomputed
//! while the handle moves. Either end drags to edit it afterwards; the card edits speed, heading,
//! width and cone by number. Session-only, like the drawing tool: a motion estimate is stale an
//! hour later.
//!
//! Manual tracks are drawn in magenta and labelled as manual, so they are never taken for the
//! automatic (SCIT) motion drawn beside them.

use super::{HookEchoApp, MapTool};
use crate::geo::{destination_point, great_circle, KM_PER_MILE};
use chrono::{DateTime, Utc};

#[path = "storm_track_geometry.rs"]
mod geometry;

/// How far ahead a track projects, and what the dragged vector stands for.
pub(crate) const HORIZON_MIN: f64 = 60.0;
const DEFAULT_MARK_INTERVAL_MIN: u32 = 15;
/// Beyond this, a marker is too far ahead for the motion to say anything about it.
const ETA_MAX_MIN: f64 = 120.0;
const KMH_PER_KT: f64 = 1.852;
/// A pointer this close (points) to a handle grabs it rather than starting a new track.
const GRAB_PT: f32 = 12.0;
/// A fingertip covers far more than a cursor tip: on a touch screen a handle is grabbed from twice
/// as far (§2.6, storm tools by mouse, pen and touch).
const GRAB_PT_TOUCH: f32 = 24.0;

/// How near a handle a press must land to grab it.
fn grab_radius(touch: bool) -> f32 {
    if touch {
        GRAB_PT_TOUCH
    } else {
        GRAB_PT
    }
}

pub(crate) fn color() -> egui::Color32 {
    egui::Color32::from_rgb(255, 92, 214)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ManualTrack {
    /// Where the storm is at `t0`, `[lon, lat]`.
    pub origin: [f64; 2],
    /// Toward which it moves, degrees clockwise from north.
    pub bearing_deg: f64,
    pub speed_kmh: f64,
    /// The analysis time the origin was placed at.
    pub t0: DateTime<Utc>,
    /// Left and right uncertainty widths, facing along the motion, at the origin.
    pub left_width_km: f64,
    pub right_width_km: f64,
    /// Half-angle of the cone the swath widens by; zero keeps it a straight band.
    pub cone_deg: f64,
    /// Display spacing only: editing it does not change the motion or the one-hour footprint.
    mark_interval_min: u32,
    /// A line track's storm edge at `t0` (a QLCS, a gust front), moving as one with the
    /// motion; empty for a single storm. The origin is its middle.
    pub edge: Vec<[f64; 2]>,
    /// The storm it was started from, when it was (ROADMAP_PARITY M2.1: manual objects may stay
    /// unassociated). Kept as the source reference, never updated: the storm history says which
    /// storm that is now.
    pub source: Option<TrackSource>,
    /// Reopened from a saved case: an estimate for its own time, drawn faded and left out of a
    /// storm's arrivals until the analyst reactivates it (ROADMAP_PARITY M2.4).
    pub historical: bool,
}

/// Where a manual track was seeded from.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TrackSource {
    /// The SCIT cell ID at seeding, and the time of that table.
    pub cell_id: String,
    pub scan: Option<DateTime<Utc>>,
    /// SCIT's motion then, toward degrees and km/h, so an adjustment can be read against it.
    pub scit_bearing_deg: f64,
    pub scit_speed_kmh: f64,
}

impl ManualTrack {
    fn new(origin: [f64; 2], t0: DateTime<Utc>) -> Self {
        Self {
            origin,
            bearing_deg: 0.0,
            speed_kmh: 0.0,
            t0,
            left_width_km: 3.0,
            right_width_km: 3.0,
            cone_deg: 10.0,
            mark_interval_min: DEFAULT_MARK_INTERVAL_MIN,
            edge: Vec::new(),
            source: None,
            historical: false,
        }
    }

    /// This track as GeoJSON features (ROADMAP_PARITY M4.4): the projected path of its storm (or
    /// a line's middle) over the hour and the uncertainty swath around it, each carrying the
    /// motion, the analysis time it is projected from, its source storm, and whether it is a
    /// historical estimate. Coordinates are `[lon, lat]` WGS84; speeds km/h, angles degrees.
    pub(crate) fn to_features(&self) -> Vec<wxdata::gis::GisFeature> {
        use serde_json::{json, Value};
        use wxdata::gis::{Geometry, GisFeature};
        let base = |kind: &str| {
            let mut p = json!({
                "hookecho": kind,
                "t0": self.t0.to_rfc3339(),
                "horizon_min": HORIZON_MIN,
                "bearing_deg": (self.bearing_deg * 10.0).round() / 10.0,
                "speed_kmh": (self.speed_kmh * 10.0).round() / 10.0,
                "left_width_km": self.left_width_km,
                "right_width_km": self.right_width_km,
                "cone_deg": self.cone_deg,
                "historical": self.historical,
            });
            if let Some(src) = &self.source {
                p["source_cell"] = Value::from(src.cell_id.clone());
                if let Some(scan) = src.scan {
                    p["source_scan"] = Value::from(scan.to_rfc3339());
                }
            }
            p.as_object().cloned().unwrap_or_default()
        };
        let path: Vec<[f64; 2]> = (0..=HORIZON_MIN as usize)
            .step_by(5)
            .map(|m| self.at(m as f64))
            .collect();
        let mut ring = self.swath();
        if ring.first() != ring.last() {
            ring.push(ring[0]);
        }
        vec![
            GisFeature {
                geometry: Geometry::LineString(path),
                properties: base("manual-track-path"),
            },
            GisFeature {
                geometry: Geometry::Polygon(vec![ring]),
                properties: base("manual-track-swath"),
            },
        ]
    }

    /// This track as a case keeps it.
    pub(crate) fn to_case(&self) -> crate::case::CaseTrack {
        crate::case::CaseTrack {
            origin: self.origin,
            bearing_deg: self.bearing_deg,
            speed_kmh: self.speed_kmh,
            t0: self.t0,
            left_width_km: self.left_width_km,
            right_width_km: self.right_width_km,
            cone_deg: self.cone_deg,
            mark_interval_min: self.mark_interval_min,
            edge: self.edge.clone(),
            source_cell: self.source.as_ref().map(|s| s.cell_id.clone()),
            source_scan: self.source.as_ref().and_then(|s| s.scan),
            scit_motion: self
                .source
                .as_ref()
                .map(|s| (s.scit_bearing_deg, s.scit_speed_kmh)),
        }
    }

    /// A track reopened from a case: as saved, and historical. Values a hand-edited file could
    /// make nonsensical are brought back into the ranges the editor allows.
    pub(crate) fn from_case(c: &crate::case::CaseTrack) -> Self {
        let finite = |v: f64, d: f64| if v.is_finite() { v } else { d };
        ManualTrack {
            origin: c.origin,
            bearing_deg: finite(c.bearing_deg, 0.0).rem_euclid(360.0),
            speed_kmh: finite(c.speed_kmh, 0.0).clamp(0.0, 150.0 * KMH_PER_KT),
            t0: c.t0,
            left_width_km: finite(c.left_width_km, 3.0).max(0.0),
            right_width_km: finite(c.right_width_km, 3.0).max(0.0),
            cone_deg: finite(c.cone_deg, 10.0).clamp(0.0, 45.0),
            mark_interval_min: if c.mark_interval_min == 0 {
                DEFAULT_MARK_INTERVAL_MIN
            } else {
                c.mark_interval_min
            },
            edge: c.edge.clone(),
            source: c.source_cell.as_ref().map(|cell| TrackSource {
                cell_id: cell.clone(),
                scan: c.source_scan,
                scit_bearing_deg: c.scit_motion.map_or(0.0, |m| m.0),
                scit_speed_kmh: c.scit_motion.map_or(0.0, |m| m.1),
            }),
            historical: true,
        }
    }

    /// A line track along `edge`, its origin at the edge's middle.
    fn line(edge: Vec<[f64; 2]>, t0: DateTime<Utc>) -> Self {
        let n = edge.len();
        let origin = if n % 2 == 1 {
            edge[n / 2]
        } else {
            let (a, b) = (edge[n / 2 - 1], edge[n / 2]);
            [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
        };
        Self {
            edge,
            ..Self::new(origin, t0)
        }
    }

    pub fn is_line(&self) -> bool {
        self.edge.len() >= 2
    }

    /// The edge `min` minutes after `t0`.
    pub fn edge_at(&self, min: f64) -> Vec<[f64; 2]> {
        let km = self.speed_kmh * min / 60.0;
        self.edge
            .iter()
            .map(|v| destination_point(*v, self.bearing_deg, km))
            .collect()
    }

    /// Move the track (and its edge) so its origin sits at `to`; the motion stays as it was.
    pub fn move_to(&mut self, to: [f64; 2]) {
        let d = [to[0] - self.origin[0], to[1] - self.origin[1]];
        for v in &mut self.edge {
            v[0] += d[0];
            v[1] += d[1];
        }
        self.origin = to;
    }

    /// `point` in a local plane at the origin, in km: along the motion, and across it (positive
    /// to the right). Good to well past the hour's reach.
    fn plane(&self, point: [f64; 2]) -> (f64, f64) {
        let lat0 = self.origin[1].to_radians();
        let dx = (point[0] - self.origin[0]) * 111.32 * lat0.cos();
        let dy = (point[1] - self.origin[1]) * 110.57;
        let b = self.bearing_deg.to_radians();
        let (ux, uy) = (b.sin(), b.cos());
        (dx * ux + dy * uy, dx * uy - dy * ux)
    }

    /// A track seeded from a SCIT cell's automatic motion, to adjust by hand. `None` without
    /// one.
    pub fn from_cell(cell: &wxdata::level3::Cell, t0: DateTime<Utc>) -> Option<Self> {
        let mut t = Self::new([cell.lon, cell.lat], t0);
        t.bearing_deg = f64::from(cell.mvt_deg?).rem_euclid(360.0);
        t.speed_kmh = f64::from(cell.mvt_kt?) * KMH_PER_KT;
        t.source = Some(TrackSource {
            cell_id: cell.id.clone(),
            scan: cell.time,
            scit_bearing_deg: t.bearing_deg,
            scit_speed_kmh: t.speed_kmh,
        });
        Some(t)
    }

    /// Where the storm is projected `min` minutes after `t0`.
    pub fn at(&self, min: f64) -> [f64; 2] {
        destination_point(self.origin, self.bearing_deg, self.speed_kmh * min / 60.0)
    }

    pub fn head(&self) -> [f64; 2] {
        self.at(HORIZON_MIN)
    }

    /// Always include the hour's end, even when a chosen spacing does not divide the horizon.
    fn projection_marks(&self) -> impl Iterator<Item = f64> {
        let step = self.mark_interval_min.clamp(5, HORIZON_MIN as u32);
        (step..HORIZON_MIN as u32)
            .step_by(step as usize)
            .chain(std::iter::once(HORIZON_MIN as u32))
            .map(f64::from)
    }

    /// Point the hour's end at `head`. `snap` holds the heading to 5° steps.
    #[cfg(test)]
    pub fn aim(&mut self, head: [f64; 2], snap: bool) {
        self.aim_from(self.origin, head, snap);
    }

    /// Set the motion to an hour's travel from `from` to `to`, wherever the drag began (a
    /// line's motion can be dragged from any point along it).
    pub fn aim_from(&mut self, from: [f64; 2], to: [f64; 2], snap: bool) {
        let (km, bearing) = great_circle(from, to);
        self.speed_kmh = km * 60.0 / HORIZON_MIN;
        self.bearing_deg = if snap {
            (bearing / 5.0).round() * 5.0
        } else {
            bearing
        }
        .rem_euclid(360.0);
    }

    /// Uncertainty width on one side, `km` along the track.
    fn width_at(&self, km: f64, right: bool) -> f64 {
        let base = if right {
            self.right_width_km
        } else {
            self.left_width_km
        };
        base + km * self.cone_deg.to_radians().tan()
    }

    /// The swath's outline to the horizon, left edge out and right edge back.
    pub(crate) fn swath(&self) -> Vec<[f64; 2]> {
        const STEPS: usize = 12;
        let km = self.speed_kmh * HORIZON_MIN / 60.0;
        let edge = |i: usize, side: f64| {
            let d = km * i as f64 / STEPS as f64;
            let c = destination_point(self.origin, self.bearing_deg, d);
            destination_point(
                c,
                self.bearing_deg + 90.0 * side,
                self.width_at(d, side > 0.0),
            )
        };
        let mut out: Vec<[f64; 2]> = (0..=STEPS).map(|i| edge(i, -1.0)).collect();
        out.extend((0..=STEPS).rev().map(|i| edge(i, 1.0)));
        out
    }

    /// Cache identity includes every vertex and geometric control. Display spacing and source
    /// time do not change population coverage.
    pub fn impact_id(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut key = std::collections::hash_map::DefaultHasher::new();
        for value in [
            self.origin[0],
            self.origin[1],
            self.bearing_deg,
            self.speed_kmh,
            self.left_width_km,
            self.right_width_km,
            self.cone_deg,
        ] {
            value.to_bits().hash(&mut key);
        }
        self.edge.len().hash(&mut key);
        for point in &self.edge {
            for value in point {
                value.to_bits().hash(&mut key);
            }
        }
        format!("track:{:016x}", key.finish())
    }

    /// A line's uncertainty at one instant, one convex part per segment. Degenerate parts stay
    /// as segments for zone intersection when the line is perpendicular to its motion.
    fn line_envelopes_at(&self, minutes: f64) -> Vec<Vec<[f64; 2]>> {
        let edge = self.edge_at(minutes);
        let km = self.speed_kmh * minutes / 60.0;
        edge.windows(2)
            .map(|w| {
                geometry::hull(
                    w.iter()
                        .flat_map(|&p| {
                            [
                                destination_point(
                                    p,
                                    self.bearing_deg - 90.0,
                                    self.width_at(km, false),
                                ),
                                destination_point(
                                    p,
                                    self.bearing_deg + 90.0,
                                    self.width_at(km, true),
                                ),
                            ]
                        })
                        .collect(),
                )
            })
            .collect()
    }

    /// The union of convex segment envelopes is the full swept area. Keeping the parts avoids
    /// filling empty space inside a bent or folded line with a single global hull.
    fn footprints(&self) -> Vec<Vec<[f64; 2]>> {
        if !self.is_line() {
            return vec![self.swath()];
        }
        self.edge
            .windows(2)
            .map(|segment| {
                let points = (0..=12)
                    .flat_map(|step| {
                        let km = self.speed_kmh * HORIZON_MIN * f64::from(step) / (12.0 * 60.0);
                        segment.iter().flat_map(move |&p| {
                            let p = destination_point(p, self.bearing_deg, km);
                            [
                                destination_point(
                                    p,
                                    self.bearing_deg - 90.0,
                                    self.width_at(km, false),
                                ),
                                destination_point(
                                    p,
                                    self.bearing_deg + 90.0,
                                    self.width_at(km, true),
                                ),
                            ]
                        })
                    })
                    .collect();
                geometry::hull(points)
            })
            .filter(|ring| ring.len() >= 3)
            .collect()
    }

    fn cached_footprints(
        &self,
        ctx: &egui::Context,
        slot: usize,
    ) -> std::sync::Arc<Vec<Vec<[f64; 2]>>> {
        type Cached = (String, std::sync::Arc<Vec<Vec<[f64; 2]>>>);
        let id = egui::Id::new(("storm_uncertainty_parts", slot));
        let key = self.impact_id();
        ctx.data_mut(|data| {
            if let Some((cached, parts)) = data.get_temp::<Cached>(id) {
                if cached == key {
                    return parts;
                }
            }
            let parts = std::sync::Arc::new(self.footprints());
            data.insert_temp(id, (key, parts.clone()));
            parts
        })
    }

    /// When the storm reaches `point`'s closest approach, and how close that is. `None` when
    /// the point is behind the storm or further ahead than the motion can speak to.
    pub fn eta(&self, point: [f64; 2]) -> Option<Eta> {
        if self.speed_kmh < 1.0 {
            return None;
        }
        let (pa, pc) = self.plane(point);
        let (mut along, mut cross) = (pa, pc);
        if self.is_line() {
            let v: Vec<(f64, f64)> = self.edge.iter().map(|p| self.plane(*p)).collect();
            // Where the moving line crosses the point: the soonest segment spanning it.
            let hit = v
                .windows(2)
                .filter_map(|w| {
                    let ((a0, c0), (a1, c1)) = (w[0], w[1]);
                    if pc < c0.min(c1) || pc > c0.max(c1) || (c1 - c0).abs() < 1e-9 {
                        return None;
                    }
                    Some(pa - (a0 + (a1 - a0) * (pc - c0) / (c1 - c0)))
                })
                .filter(|d| *d > 0.0)
                .min_by(f64::total_cmp);
            if let Some(d) = hit {
                let minutes = d / self.speed_kmh * 60.0;
                return (minutes <= ETA_MAX_MIN).then_some(Eta {
                    minutes,
                    closest_km: 0.0,
                    right: false,
                    in_path: true,
                });
            }
            // Beyond its cross-track extent, uncertainty comes from the nearest eligible
            // vertex, which may be an interior bend rather than either endpoint.
            let end = v.iter().copied().filter(|p| pa > p.0).min_by(|a, b| {
                (pc - a.1)
                    .abs()
                    .total_cmp(&(pc - b.1).abs())
                    .then((pa - a.0).total_cmp(&(pa - b.0)))
            })?;
            along = pa - end.0;
            cross = pc - end.1;
        }
        // Positive `cross`: the point lies to the right of the motion.
        if along <= 0.0 {
            return None;
        }
        let minutes = along / self.speed_kmh * 60.0;
        (minutes <= ETA_MAX_MIN).then(|| Eta {
            minutes,
            closest_km: cross.abs(),
            right: cross > 0.0,
            in_path: cross.abs() <= self.width_at(along, cross > 0.0),
        })
    }
}

impl ManualTrack {
    /// When the storm first enters `ring` (a watch zone), checked minute by minute to two hours
    /// ahead: its centre, or for a line any point along it. Failing that, when the swath's edge
    /// first touches it (`grazes`). `None` when neither happens.
    pub fn zone_eta(&self, ring: &[[f64; 2]]) -> Option<ZoneEta> {
        if ring.len() < 3 {
            return None;
        }
        let inside = |p: [f64; 2]| wxdata::overlay::point_in_ring(ring, p[0], p[1]);
        let core = |m: f64| -> Vec<[f64; 2]> {
            if self.is_line() {
                self.edge_at(m)
            } else {
                vec![self.at(m)]
            }
        };
        let flanks = |m: f64| -> [[f64; 2]; 2] {
            let km = self.speed_kmh * m / 60.0;
            let ends = if self.is_line() {
                let e = self.edge_at(m);
                [e[0], e[e.len() - 1]]
            } else {
                let c = self.at(m);
                [c, c]
            };
            [
                destination_point(ends[0], self.bearing_deg - 90.0, self.width_at(km, false)),
                destination_point(ends[1], self.bearing_deg + 90.0, self.width_at(km, true)),
            ]
        };
        let mut graze = None;
        for m in 0..=ETA_MAX_MIN as usize {
            let m = m as f64;
            let positions = core(m);
            if positions.iter().copied().any(inside)
                || positions
                    .windows(2)
                    .any(|w| wxdata::overlay::segment_intersects_ring(w[0], w[1], ring))
            {
                return Some(ZoneEta {
                    minutes: m,
                    grazes: false,
                });
            }
            if graze.is_none() {
                let sides = flanks(m);
                if sides.into_iter().any(inside)
                    || (!self.is_line()
                        && wxdata::overlay::segment_intersects_ring(sides[0], sides[1], ring))
                    || (self.is_line()
                        && self
                            .line_envelopes_at(m)
                            .iter()
                            .any(|part| geometry::touches(part, ring)))
                {
                    graze = Some(m);
                }
            }
            if self.speed_kmh < 1.0 {
                break;
            }
        }
        graze.map(|minutes| ZoneEta {
            minutes,
            grazes: true,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ZoneEta {
    /// Zero when it is already inside.
    pub minutes: f64,
    /// Only the swath's edge reaches the zone, not the storm itself.
    pub grazes: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Eta {
    pub minutes: f64,
    pub closest_km: f64,
    /// Which side of the track the point passes on, facing along the motion.
    pub right: bool,
    /// Inside the swath at that time.
    pub in_path: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    Origin(usize),
    Head(usize),
    /// A new line's motion, dragged from this point.
    Motion(usize, [f64; 2]),
}

#[derive(Clone)]
struct TrackDrag {
    grab: Grab,
    pane: usize,
    created: bool,
    before: ManualTrack,
}

impl TrackDrag {
    fn aim(&self, track: &mut ManualTrack, from: [f64; 2], to: [f64; 2], keys: egui::Modifiers) {
        track.aim_from(from, to, keys.shift);
        // A new vector has no motion to hold. Existing vectors use the drag-start values,
        // so a constraint stays stable even after several frames or a modifier change.
        if !self.created {
            if keys.ctrl || keys.command {
                track.bearing_deg = self.before.bearing_deg;
            }
            if keys.alt {
                track.speed_kmh = self.before.speed_kmh;
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct StormTracks {
    pub tracks: Vec<ManualTrack>,
    pub selected: Option<usize>,
    drag: Option<TrackDrag>,
    /// A line being clicked out, point by point, before its motion is dragged.
    pub pending: Vec<[f64; 2]>,
}

impl StormTracks {
    /// Local tool keys run before global bindings. A focused text editor or key-binding
    /// capture keeps its events; a button in the card still permits track commands.
    fn keys(&mut self, ctx: &egui::Context, armed: bool, capturing: bool) {
        let card_focused = ctx
            .memory(|m| m.focused())
            .and_then(|id| ctx.read_response(id))
            .is_some_and(|r| {
                r.layer_id
                    == egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("storm_track_card"),
                    )
            });
        if capturing
            || ctx.text_edit_focused()
            || !ctx.input(|i| i.focused)
            || !(armed || card_focused)
        {
            return;
        }
        if !self.pending.is_empty()
            && ctx.input_mut(|i| consume_track_key(i, egui::Modifiers::NONE, egui::Key::Backspace))
        {
            self.pending.pop();
        }
        self.selected = self.selected.filter(|&i| i < self.tracks.len());
        let Some(index) = self.selected else { return };
        let (del, dup, narrow, widen) = ctx.input_mut(|i| {
            (
                consume_track_key(i, egui::Modifiers::NONE, egui::Key::Delete),
                consume_track_key(i, egui::Modifiers::COMMAND, egui::Key::D),
                consume_track_key(i, egui::Modifiers::NONE, egui::Key::OpenBracket),
                consume_track_key(i, egui::Modifiers::NONE, egui::Key::CloseBracket),
            )
        });
        if del {
            self.remove(index);
        } else if dup {
            self.tracks.push(self.tracks[index].clone());
            self.selected = Some(self.tracks.len() - 1);
        } else if narrow || widen {
            let track = &mut self.tracks[index];
            track.cone_deg = (track.cone_deg + if widen { 2.0 } else { -2.0 }).clamp(0.0, 45.0);
        }
    }

    fn owns_drag(&self, pane: usize) -> bool {
        self.drag.as_ref().is_some_and(|drag| drag.pane == pane)
    }

    fn accepts_pointer(&mut self, pane: usize, allowed: bool, down: bool, released: bool) -> bool {
        if self.drag.is_some() && !self.owns_drag(pane) {
            return false;
        }
        if !allowed || (self.owns_drag(pane) && !down && !released) {
            self.cancel_drag();
            return false;
        }
        true
    }

    /// A gesture takeover or lost pointer cancels the transaction instead of leaving a handle
    /// attached to the next press. Existing edits roll back; new lines return to construction.
    fn cancel_drag(&mut self) {
        let Some(drag) = self.drag.take() else { return };
        let i = match drag.grab {
            Grab::Head(i) | Grab::Origin(i) | Grab::Motion(i, _) => i,
        };
        if drag.created {
            self.remove(i);
            self.pending = drag.before.edge;
        } else if let Some(track) = self.tracks.get_mut(i) {
            *track = drag.before;
            self.selected = Some(i);
        }
    }

    fn finish_drag(&mut self) {
        if let Some(drag) = self.drag.take().filter(|d| d.created) {
            let i = match drag.grab {
                Grab::Head(i) | Grab::Motion(i, _) => i,
                Grab::Origin(_) => return,
            };
            if self.tracks.get(i).is_some_and(|t| t.speed_kmh < 2.0) {
                let edge = self.tracks[i].edge.clone();
                self.remove(i);
                self.pending = edge;
            }
        }
    }

    fn remove(&mut self, i: usize) {
        if i < self.tracks.len() {
            self.tracks.remove(i);
        }
        self.selected = None;
        self.drag = None;
    }
}

/// A consumed key may also have a companion Text event. Remove that companion so the global
/// Android text fallback cannot fire a second action; text-only plain keys work here too.
fn consume_track_key(
    input: &mut egui::InputState,
    modifiers: egui::Modifiers,
    key: egui::Key,
) -> bool {
    let matches_text = |event: &egui::Event| {
        matches!(event,
        egui::Event::Text(text) if text.chars().count() == 1
            && text.eq_ignore_ascii_case(key.symbol_or_name()))
    };
    let text_only =
        modifiers.is_none() && input.modifiers.is_none() && input.events.iter().any(matches_text);
    if input.consume_key(modifiers, key) || text_only {
        input.events.retain(|event| !matches_text(event));
        true
    } else {
        false
    }
}

/// Compass point for a heading.
pub(crate) fn compass(deg: f64) -> &'static str {
    const P: [&str; 16] = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW",
        "NW", "NNW",
    ];
    P[((deg.rem_euclid(360.0) + 11.25) / 22.5) as usize % 16]
}

pub(crate) fn distance(km: f64, metric: bool) -> String {
    if metric {
        format!("{km:.0} km")
    } else {
        format!("{:.0} mi", km / KM_PER_MILE)
    }
}

fn width_control(ui: &mut egui::Ui, label: &str, width_km: &mut f64, metric: bool) -> bool {
    ui.weak(label);
    let mut value = if metric {
        *width_km
    } else {
        *width_km / KM_PER_MILE
    };
    let changed = ui
        .add(
            egui::DragValue::new(&mut value)
                .range(0.0..=50.0)
                .speed(0.2)
                .max_decimals(1)
                .suffix(if metric { " km" } else { " mi" }),
        )
        .on_hover_text("Uncertainty width, facing along the motion")
        .changed();
    if changed {
        *width_km = if metric { value } else { value * KM_PER_MILE };
    }
    changed
}

/// One line on a storm's manual motion for the storm's own card: the motion, when it was set, and
/// how it compares with the SCIT motion it was started from.
pub(crate) fn manual_motion_line(
    track: &ManualTrack,
    now: Option<DateTime<Utc>>,
    metric: bool,
    fmt_time: impl Fn(DateTime<Utc>) -> String,
) -> String {
    let speed = |kmh: f64| {
        if metric {
            format!("{kmh:.0} km/h")
        } else {
            format!("{:.0} mph", kmh / 1.609_344)
        }
    };
    let mut line = format!(
        "{} at {} ({:03.0}°), set for {}",
        compass(track.bearing_deg),
        speed(track.speed_kmh),
        track.bearing_deg,
        fmt_time(track.t0)
    );
    if let Some(age) = now.map(|n| (n - track.t0).num_minutes()).filter(|m| *m > 0) {
        line.push_str(&format!(", {age} min before this scan"));
    }
    if let Some(src) = &track.source {
        let turned = (track.bearing_deg - src.scit_bearing_deg + 540.0).rem_euclid(360.0) - 180.0;
        let faster = track.speed_kmh - src.scit_speed_kmh;
        if turned.abs() < 0.5 && faster.abs() < 0.5 {
            line.push_str(&format!(
                "; SCIT's motion for cell {}, unadjusted",
                src.cell_id
            ));
        } else {
            line.push_str(&format!(
                "; adjusted from SCIT's {} at {} for cell {}",
                compass(src.scit_bearing_deg),
                speed(src.scit_speed_kmh),
                src.cell_id
            ));
        }
    }
    line
}

impl HookEchoApp {
    pub(crate) fn storm_track_keys(&mut self, ctx: &egui::Context) {
        self.storm_tracks
            .keys(ctx, self.tool == MapTool::StormTrack, self.capture_key);
    }

    /// The manual tracks started from `cell`'s storm, newest first: the storm history decides
    /// which storm a track's seeding cell was, so a track follows its storm through a SCIT
    /// renumbering and is never shown for another storm that took the old ID.
    pub(crate) fn manual_tracks_for(&self, cell: &wxdata::level3::Cell) -> Vec<&ManualTrack> {
        use super::chrome::Resolved;
        self.storm_tracks
            .tracks
            .iter()
            .rev()
            .filter(|t| {
                // A reopened estimate is not this storm's motion now until reactivated.
                if t.historical {
                    return false;
                }
                let Some(src) = &t.source else {
                    return false;
                };
                match self
                    .dock
                    .storm_ids
                    .resolve(&src.cell_id, src.scan.map(|s| s.timestamp()))
                {
                    Resolved::Current(id) => id == cell.id,
                    Resolved::Gone(_) => false,
                    Resolved::Unknown => !cell.id.is_empty() && src.cell_id == cell.id,
                }
            })
            .collect()
    }

    /// Seed the manual tool from the same SCIT motion and source time in every storm UI.
    pub(crate) fn track_cell_manually(&mut self, c: &wxdata::level3::Cell) {
        let t0 = c
            .time
            .or_else(|| self.views[self.active].volume.as_ref().map(|v| v.time))
            .unwrap_or_else(Utc::now);
        if let Some(track) = ManualTrack::from_cell(c, t0) {
            self.storm_tracks.tracks.push(track);
            self.storm_tracks.selected = Some(self.storm_tracks.tracks.len() - 1);
            self.tool = super::MapTool::StormTrack;
        }
    }

    /// The tool's pointer handling on one pane: a drag from open map starts a track, a drag from
    /// a handle moves that end, a click picks the track under it. Returns whether it took the
    /// drag, so the map does not also pan.
    pub(crate) fn storm_track_input(
        &mut self,
        idx: usize,
        prect: egui::Rect,
        response: &egui::Response,
        ui: &egui::Ui,
        allow_pointer: bool,
    ) -> bool {
        let interrupted = ui.input(|i| {
            !i.focused
                || i.key_pressed(egui::Key::Escape)
                || i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Touch {
                            phase: egui::TouchPhase::Cancel,
                            ..
                        }
                    )
                })
        });
        if self.tool != MapTool::StormTrack || interrupted {
            self.storm_tracks.cancel_drag();
            if self.tool != MapTool::StormTrack || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.storm_tracks.pending.clear();
            }
            return false;
        }
        if !self.storm_tracks.accepts_pointer(
            idx,
            allow_pointer,
            ui.input(|i| i.pointer.primary_down()),
            response.drag_stopped_by(egui::PointerButton::Primary),
        ) {
            return false;
        }
        let vp = (prect.width(), prect.height());
        let cam = self.views[idx].camera;
        let to_ll = |p: egui::Pos2| {
            let w = cam.screen_to_world((p.x - prect.left(), p.y - prect.top()), vp);
            let (lon, lat) = crate::render::mercator::world_to_lonlat(w.0, w.1);
            [lon, lat]
        };
        let to_px = |ll: [f64; 2]| {
            let w = crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
            let (x, y) = cam.world_to_screen(w, vp);
            egui::pos2(prect.left() + x, prect.top() + y)
        };
        let radius = grab_radius(ui.input(|i| i.any_touches() || i.has_touch_screen()));
        let grab_at = |p: egui::Pos2, st: &StormTracks| {
            st.tracks
                .iter()
                .enumerate()
                .flat_map(|(i, t)| {
                    [
                        (Grab::Head(i), to_px(t.head())),
                        (Grab::Origin(i), to_px(t.origin)),
                    ]
                })
                .map(|(g, at)| (g, at.distance(p)))
                .filter(|(_, d)| *d <= radius)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(g, _)| g)
        };
        let keys = ui.input(|i| i.modifiers);
        if response.drag_started_by(egui::PointerButton::Primary) {
            if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
                self.active = idx;
                let mut created = false;
                let grab = grab_at(p, &self.storm_tracks).unwrap_or_else(|| {
                    created = true;
                    let t0 = self.views[idx]
                        .displayed_radar_time()
                        .unwrap_or_else(Utc::now);
                    let st = &mut self.storm_tracks;
                    let pending = std::mem::take(&mut st.pending);
                    if pending.len() >= 2 {
                        // A clicked-out line: this drag is its motion, from wherever it began.
                        st.tracks.push(ManualTrack::line(pending, t0));
                        Grab::Motion(st.tracks.len() - 1, to_ll(p))
                    } else {
                        st.tracks.push(ManualTrack::new(to_ll(p), t0));
                        Grab::Head(st.tracks.len() - 1)
                    }
                });
                let i = match grab {
                    Grab::Head(i) | Grab::Origin(i) | Grab::Motion(i, _) => i,
                };
                let track = &self.storm_tracks.tracks[i];
                self.storm_tracks.drag = Some(TrackDrag {
                    grab,
                    pane: idx,
                    created,
                    before: track.clone(),
                });
            }
        }
        let st = &mut self.storm_tracks;
        if let (Some(drag), Some(p)) = (st.drag.as_ref(), response.interact_pointer_pos()) {
            let ll = to_ll(p);
            match drag.grab {
                Grab::Head(i) => {
                    if let Some(t) = st.tracks.get_mut(i) {
                        drag.aim(t, t.origin, ll, keys);
                    }
                    st.selected = Some(i);
                }
                Grab::Origin(i) => {
                    if let Some(t) = st.tracks.get_mut(i) {
                        // The origin moves the whole track; the motion stays as it was.
                        t.move_to(ll);
                    }
                    st.selected = Some(i);
                }
                Grab::Motion(i, from) => {
                    if let Some(t) = st.tracks.get_mut(i) {
                        drag.aim(t, from, ll, keys);
                    }
                    st.selected = Some(i);
                }
            }
        }
        if response.drag_stopped_by(egui::PointerButton::Primary) {
            self.storm_tracks.finish_drag();
        }
        if response.clicked_by(egui::PointerButton::Primary) {
            if let Some(p) = response.interact_pointer_pos() {
                // A click on a track picks it; on open map it adds a point to a line.
                match grab_at(p, &self.storm_tracks) {
                    Some(Grab::Head(i) | Grab::Origin(i) | Grab::Motion(i, _)) => {
                        self.storm_tracks.selected = Some(i);
                    }
                    None => {
                        self.storm_tracks.selected = None;
                        self.storm_tracks.pending.push(to_ll(p));
                    }
                }
            }
        }
        response.dragged_by(egui::PointerButton::Primary) || self.storm_tracks.owns_drag(idx)
    }

    /// Every manual track on every pane: swath, centre line, time marks and handles.
    pub(crate) fn paint_storm_tracks(&self, ui: &egui::Ui, rects: &[egui::Rect]) {
        if self.storm_tracks.tracks.is_empty() && self.storm_tracks.pending.is_empty() {
            return;
        }
        let col = color();
        let halo = egui::Color32::from_black_alpha(190);
        for (idx, rect) in rects.iter().enumerate() {
            let cam = self.views[idx].camera;
            let vp = (rect.width(), rect.height());
            let to_px = |ll: [f64; 2]| {
                let w = crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
                let (x, y) = cam.world_to_screen(w, vp);
                egui::pos2(rect.left() + x, rect.top() + y)
            };
            let painter = ui.painter_at(*rect);
            // A line being clicked out: dashed, with its points.
            let pending: Vec<egui::Pos2> = self
                .storm_tracks
                .pending
                .iter()
                .map(|p| to_px(*p))
                .collect();
            if pending.len() >= 2 {
                painter.extend(egui::Shape::dashed_line(
                    &pending,
                    egui::Stroke::new(2.0, col),
                    6.0,
                    4.0,
                ));
            }
            for p in &pending {
                painter.circle(*p, 4.0, col, egui::Stroke::new(1.5, halo));
            }
            for (i, t) in self.storm_tracks.tracks.iter().enumerate() {
                let selected = self.storm_tracks.selected == Some(i);
                // A reopened estimate reads as one: faded, never as a live motion.
                let col = if t.historical {
                    col.gamma_multiply(0.45)
                } else {
                    col
                };
                let fill = col.gamma_multiply(if selected { 0.16 } else { 0.09 });
                if t.is_line() {
                    // The swept area, a quad per segment (the whole is not convex); then the
                    // edge at the chosen interval, thin, and now, solid.
                    let now: Vec<egui::Pos2> = t.edge.iter().map(|p| to_px(*p)).collect();
                    for part in t.cached_footprints(ui.ctx(), i).iter() {
                        painter.add(egui::Shape::convex_polygon(
                            part.iter().copied().map(to_px).collect(),
                            fill,
                            egui::Stroke::NONE,
                        ));
                    }
                    for m in t.projection_marks() {
                        let at: Vec<egui::Pos2> = t.edge_at(m).into_iter().map(to_px).collect();
                        painter.add(egui::Shape::line(
                            at,
                            egui::Stroke::new(1.0, col.gamma_multiply(0.8)),
                        ));
                    }
                    painter.add(egui::Shape::line(now.clone(), egui::Stroke::new(5.0, halo)));
                    painter.add(egui::Shape::line(
                        now,
                        egui::Stroke::new(if selected { 3.0 } else { 2.2 }, col),
                    ));
                } else {
                    let swath: Vec<egui::Pos2> = t.swath().into_iter().map(to_px).collect();
                    painter.add(egui::Shape::convex_polygon(
                        swath.clone(),
                        fill,
                        egui::Stroke::NONE,
                    ));
                    painter.add(egui::Shape::closed_line(
                        swath,
                        egui::Stroke::new(1.0, col.gamma_multiply(0.7)),
                    ));
                }
                let (a, b) = (to_px(t.origin), to_px(t.head()));
                painter.line_segment([a, b], egui::Stroke::new(4.0, halo));
                painter.line_segment(
                    [a, b],
                    egui::Stroke::new(if selected { 2.5 } else { 1.8 }, col),
                );
                // Arrowhead at the hour's end.
                let dir = (b - a).normalized();
                if dir.is_finite() && a.distance(b) > 8.0 {
                    let n = dir.rot90();
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            b + dir * 3.0,
                            b - dir * 9.0 + n * 5.0,
                            b - dir * 9.0 - n * 5.0,
                        ],
                        col,
                        egui::Stroke::new(1.0, halo),
                    ));
                }
                let font = egui::FontId::monospace(10.5);
                for m in t.projection_marks() {
                    let p = to_px(t.at(m));
                    painter.circle_filled(p, 3.5, halo);
                    painter.circle_filled(p, 2.5, col);
                    let clock = (t.t0 + chrono::Duration::minutes(m as i64)).format("%H%MZ");
                    let label = format!("+{m:.0} {clock}");
                    let at = p + egui::vec2(6.0, -2.0);
                    painter.text(
                        at + egui::vec2(1.0, 1.0),
                        egui::Align2::LEFT_BOTTOM,
                        &label,
                        font.clone(),
                        halo,
                    );
                    painter.text(
                        at,
                        egui::Align2::LEFT_BOTTOM,
                        &label,
                        font.clone(),
                        egui::Color32::WHITE,
                    );
                }
                // Handles: the origin filled, the head ringed, both bigger when selected.
                let r = if selected { 6.0 } else { 4.5 };
                painter.circle(a, r, col, egui::Stroke::new(1.5, halo));
                painter.circle(b, r, halo, egui::Stroke::new(2.0, col));
                let kt = t.speed_kmh / KMH_PER_KT;
                let tag = format!(
                    "MANUAL{} {:03.0}° {kt:.0} kt",
                    if t.is_line() { " LINE" } else { "" },
                    t.bearing_deg
                );
                let at = a + egui::vec2(8.0, 6.0);
                painter.text(
                    at + egui::vec2(1.0, 1.0),
                    egui::Align2::LEFT_TOP,
                    &tag,
                    font.clone(),
                    halo,
                );
                painter.text(at, egui::Align2::LEFT_TOP, &tag, font, col);
            }
        }
    }

    /// The tool's card while it is armed or a track is selected: each track's numbers, editable,
    /// and the selected one's ETAs at the saved markers. Also its keys: Delete removes the
    /// selected track, Ctrl+D duplicates it, [ and ] narrow and widen its cone.
    pub(crate) fn storm_track_card(&mut self, ctx: &egui::Context) {
        let armed = self.tool == MapTool::StormTrack;
        if !armed
            || self
                .storm_tracks
                .drag
                .as_ref()
                .is_some_and(|drag| drag.pane >= self.views.len())
        {
            self.storm_tracks.cancel_drag();
        }
        if !armed {
            // A line half clicked out is dropped with the tool.
            self.storm_tracks.pending.clear();
            if self.storm_tracks.selected.is_none() {
                return;
            }
        }
        let metric = self.metric_in(self.active);
        let t = self.ws_tokens();
        let area = self.chrome_rect;
        let markers: Vec<(String, [f64; 2])> = self
            .settings
            .markers
            .iter()
            .map(|m| (m.name.clone(), [m.lon, m.lat]))
            .collect();
        let zones: Vec<(String, Vec<[f64; 2]>)> = self
            .settings
            .alert_polygons
            .iter()
            .map(|z| (z.name.clone(), z.ring.clone()))
            .collect();
        // The selected track's population lookup, if one was asked for this geometry.
        let impact = self
            .storm_tracks
            .selected
            .and_then(|i| self.storm_tracks.tracks.get(i))
            .map(|t| {
                let id = t.impact_id();
                let state = self.impacts.by_id.get(&id).cloned();
                (id, state)
            });
        let mut ask_impact = false;
        let st = &mut self.storm_tracks;
        let mut remove = None;
        let mut duplicate = None;
        let mut undo_point = false;
        let mut clear = false;
        egui::Area::new(egui::Id::new("storm_track_card"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(area.left_bottom() + egui::vec2(12.0, -40.0))
            .constrain_to(area)
            .show(ctx, |ui| {
                crate::ui::workstation::card_frame(&t).show(ui, |ui| {
                    crate::ui::workstation::style_scope(ui, &t);
                    ui.set_width((area.width() - 48.0).clamp(120.0, 300.0));
                    egui::ScrollArea::vertical()
                        .id_salt("storm_motion_body")
                        .max_height((area.height() - 96.0).max(80.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("Storm motion").strong().color(color()),
                                );
                                ui.label(egui::RichText::new("manual").small().weak());
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if !st.tracks.is_empty()
                                            && ui.small_button("Clear all").clicked()
                                        {
                                            clear = true;
                                        }
                                    },
                                );
                            });
                            if !st.pending.is_empty()
                                && ui
                                    .small_button("Undo point")
                                    .on_hover_text("Remove the last point of the line (Backspace)")
                                    .clicked()
                            {
                                undo_point = true;
                            }
                            match st.pending.len() {
                                0 => {}
                                1 => {
                                    ui.label(
                                egui::RichText::new(
                                    "Line: click more points along it (Backspace undoes one)",
                                )
                                .color(color()),
                            );
                                }
                                n => {
                                    ui.label(
                                        egui::RichText::new(format!(
                                    "Line of {n} points: now drag its motion from anywhere \
                                     (Backspace undoes a point)"
                                ))
                                        .color(color()),
                                    );
                                }
                            }
                            if st.tracks.is_empty() {
                                if st.pending.is_empty() {
                                    ui.weak(
                                        "Drag from a storm to where it will be in an hour. Shift \
                                 snaps the heading to 5°. For a line of storms, click points \
                                 along it first, then drag.",
                                    );
                                }
                                return;
                            }
                            for (i, track) in st.tracks.iter_mut().enumerate() {
                                let sel = st.selected == Some(i);
                                ui.horizontal_wrapped(|ui| {
                                    let name = format!(
                                        "#{}{} {:03.0}° {}",
                                        i + 1,
                                        if track.is_line() { " line" } else { "" },
                                        track.bearing_deg,
                                        compass(track.bearing_deg)
                                    );
                                    if ui.selectable_label(sel, name).clicked() {
                                        st.selected = if sel { None } else { Some(i) };
                                    }
                                    if track.historical {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "from a case, set for {}",
                                                track.t0.format("%Y-%m-%d %H:%MZ")
                                            ))
                                            .small()
                                            .weak(),
                                        );
                                        if ui
                                            .small_button("Reactivate")
                                            .on_hover_text(
                                                "Use this saved estimate as a current motion: it \
                                                 still projects from the time it was set for",
                                            )
                                            .clicked()
                                        {
                                            track.historical = false;
                                        }
                                    }
                                    let mut kt = track.speed_kmh / KMH_PER_KT;
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut kt)
                                                .range(0.0..=150.0)
                                                .speed(0.5)
                                                .max_decimals(0)
                                                .suffix(" kt"),
                                        )
                                        .on_hover_text("Speed")
                                        .changed()
                                    {
                                        track.speed_kmh = kt * KMH_PER_KT;
                                    }
                                    ui.add(
                                        egui::DragValue::new(&mut track.bearing_deg)
                                            .range(0.0..=359.9)
                                            .speed(1.0)
                                            .max_decimals(0)
                                            .suffix("°"),
                                    )
                                    .on_hover_text("Heading, toward");
                                    if ui
                                        .small_button("×")
                                        .on_hover_text("Remove (Delete)")
                                        .clicked()
                                    {
                                        remove = Some(i);
                                    }
                                    // The keyboard's Ctrl+D, for pen and touch.
                                    if ui
                                        .small_button(egui_phosphor::regular::COPY)
                                        .on_hover_text("Duplicate (Ctrl+D)")
                                        .clicked()
                                    {
                                        duplicate = Some(i);
                                    }
                                });
                                if !sel {
                                    continue;
                                }
                                ui.horizontal_wrapped(|ui| {
                                    ui.weak("Cone");
                                    ui.add(
                                        egui::DragValue::new(&mut track.cone_deg)
                                            .range(0.0..=45.0)
                                            .speed(0.5)
                                            .suffix("°"),
                                    )
                                    .on_hover_text("Half-angle the swath widens by ([ and ])");
                                    {
                                        for (label, width) in [
                                            ("Left width", &mut track.left_width_km),
                                            ("Right width", &mut track.right_width_km),
                                        ] {
                                            width_control(ui, label, width, metric);
                                        }
                                    }
                                    ui.weak(format!("from {}", track.t0.format("%H:%MZ")));
                                });
                                ui.horizontal(|ui| {
                                    ui.weak("Projection marks");
                                    egui::ComboBox::from_id_salt(("track_mark_interval", i))
                                        .selected_text(format!("{} min", track.mark_interval_min))
                                        .width(72.0)
                                        .show_ui(ui, |ui| {
                                            for minutes in [5, 10, 15, 20, 30, 60] {
                                                ui.selectable_value(
                                                    &mut track.mark_interval_min,
                                                    minutes,
                                                    format!("{minutes} min"),
                                                );
                                            }
                                        });
                                });
                                // Who lives in the hour's path (2020 Census), asked for on demand: a
                                // lookup per drag frame would hammer the service.
                                use super::impact::{summary, towns, ImpactState};
                                match impact.as_ref().and_then(|(_, s)| s.as_ref()) {
                                    None => {
                                        if ui
                                    .small_button("People in path")
                                    .on_hover_text(
                                        "Population and towns inside the next hour's swath \
                                         (2020 Census)",
                                    )
                                    .clicked()
                                {
                                    ask_impact = true;
                                }
                                    }
                                    Some(ImpactState::Pending) => {
                                        ui.weak("Counting people in the path…");
                                    }
                                    Some(ImpactState::Failed) => {
                                        if ui
                                            .small_button("Population lookup failed · retry")
                                            .clicked()
                                        {
                                            ask_impact = true;
                                        }
                                    }
                                    Some(ImpactState::Ready(i)) => {
                                        ui.label(egui::RichText::new(summary(i)).strong());
                                        if !i.places.is_empty() {
                                            ui.weak(towns(i));
                                        }
                                    }
                                }
                                let mut etas: Vec<(&str, Eta)> = markers
                                    .iter()
                                    .filter_map(|(n, p)| track.eta(*p).map(|e| (n.as_str(), e)))
                                    .collect();
                                etas.sort_by(|a, b| {
                                    b.1.in_path
                                        .cmp(&a.1.in_path)
                                        .then(a.1.minutes.total_cmp(&b.1.minutes))
                                });
                                if markers.is_empty() {
                                    ui.weak("Save markers for arrival times at them.");
                                } else if etas.is_empty() {
                                    ui.weak("No saved marker ahead within two hours.");
                                }
                                for (name, e) in etas.iter().take(6) {
                                    let when = track.t0
                                        + chrono::Duration::seconds((e.minutes * 60.0) as i64);
                                    let pass = if track.is_line() && e.closest_km == 0.0 {
                                        "line arrives".to_string()
                                    } else if e.closest_km < 0.5 {
                                        "direct hit".to_string()
                                    } else {
                                        format!(
                                            "passes {} {}",
                                            distance(e.closest_km, metric),
                                            compass(
                                                track.bearing_deg
                                                    + if e.right { -90.0 } else { 90.0 }
                                            )
                                        )
                                    };
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(if e.in_path {
                                                "●"
                                            } else {
                                                "○"
                                            })
                                            .color(if e.in_path { t.warn } else { t.text_dim }),
                                        );
                                        ui.label(egui::RichText::new(*name).strong());
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "~{} (+{:.0} min)",
                                                when.format("%H:%MZ"),
                                                e.minutes
                                            ))
                                            .monospace(),
                                        );
                                        ui.weak(pass);
                                    });
                                }
                                // Watch zones: when the storm (or line) gets in, soonest first.
                                let mut zone_etas: Vec<(&str, ZoneEta)> = zones
                                    .iter()
                                    .filter_map(|(n, r)| track.zone_eta(r).map(|e| (n.as_str(), e)))
                                    .collect();
                                zone_etas.sort_by(|a, b| {
                                    a.1.grazes
                                        .cmp(&b.1.grazes)
                                        .then(a.1.minutes.total_cmp(&b.1.minutes))
                                });
                                for (name, e) in zone_etas.iter().take(4) {
                                    let when = track.t0
                                        + chrono::Duration::seconds((e.minutes * 60.0) as i64);
                                    let what = match (e.minutes == 0.0, e.grazes) {
                                        (true, false) => "inside now".to_string(),
                                        (true, true) => "swath edge inside now".to_string(),
                                        (false, false) => format!(
                                            "enters ~{} (+{:.0} min)",
                                            when.format("%H:%MZ"),
                                            e.minutes
                                        ),
                                        (false, true) => format!(
                                            "swath edge ~{} (+{:.0} min)",
                                            when.format("%H:%MZ"),
                                            e.minutes
                                        ),
                                    };
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("▰").color(if e.grazes {
                                            t.text_dim
                                        } else {
                                            t.warn
                                        }));
                                        ui.label(egui::RichText::new(*name).strong());
                                        ui.label(egui::RichText::new(what).monospace());
                                    });
                                }
                            }
                            ui.weak("Drag either end to edit · Shift snaps heading · Ctrl-drag holds bearing · Alt-drag holds speed");
                            ui.weak("Ctrl+D duplicates · Delete removes · [ / ] adjust cone");
                        });
                });
            });
        if ask_impact {
            if let (Some((id, _)), Some(t)) = (impact, st.selected.and_then(|i| st.tracks.get(i))) {
                let parts = t.footprints();
                self.request_impact(id, parts, ctx);
            }
        }
        let st = &mut self.storm_tracks;
        if undo_point {
            st.pending.pop();
        }
        if clear {
            st.tracks.clear();
            st.selected = None;
        } else if let Some(i) = remove {
            st.remove(i);
        } else if let Some(i) = duplicate.filter(|&i| i < st.tracks.len()) {
            let copy = st.tracks[i].clone();
            st.tracks.push(copy);
            st.selected = Some(st.tracks.len() - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_input(key: egui::Key, modifiers: egui::Modifiers, text: Option<&str>) -> egui::RawInput {
        let mut events = vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }];
        if let Some(text) = text {
            events.push(egui::Event::Text(text.into()));
        }
        egui::RawInput {
            modifiers,
            events,
            ..Default::default()
        }
    }

    #[test]
    fn cone_keys_run_before_pane_focus_including_companion_and_text_only_input() {
        for text_only in [false, true] {
            let ctx = egui::Context::default();
            let mut st = StormTracks {
                tracks: vec![track()],
                selected: Some(0),
                ..Default::default()
            };
            let before = st.tracks[0].cone_deg;
            let input = if text_only {
                egui::RawInput {
                    events: vec![egui::Event::Text("]".into())],
                    ..Default::default()
                }
            } else {
                key_input(egui::Key::CloseBracket, egui::Modifiers::NONE, Some("]"))
            };
            let _ = ctx.run_ui(input, |_| {
                st.keys(&ctx, true, false);
                assert_eq!(st.tracks[0].cone_deg, before + 2.0);
                assert!(
                    crate::hotkeys::poll(&ctx, &crate::hotkeys::defaults()).is_empty(),
                    "local cone adjustment must not also focus another pane"
                );
            });
        }
    }

    #[test]
    fn duplicate_owns_its_companion_text_without_toggling_3d() {
        let ctx = egui::Context::default();
        let original = track();
        let mut st = StormTracks {
            tracks: vec![original.clone()],
            selected: Some(0),
            ..Default::default()
        };
        let _ = ctx.run_ui(
            key_input(egui::Key::D, egui::Modifiers::COMMAND, Some("d")),
            |_| {
                st.keys(&ctx, true, false);
                assert_eq!(st.tracks, vec![original.clone(), original.clone()]);
                assert_eq!(st.selected, Some(1));
                assert!(crate::hotkeys::poll(&ctx, &crate::hotkeys::defaults()).is_empty());
            },
        );
    }

    #[test]
    fn rebinding_keeps_track_edit_keys_unconsumed() {
        for (key, modifiers, text) in [
            (egui::Key::Delete, egui::Modifiers::NONE, None),
            (egui::Key::D, egui::Modifiers::COMMAND, Some("d")),
            (egui::Key::CloseBracket, egui::Modifiers::NONE, Some("]")),
            (egui::Key::Backspace, egui::Modifiers::NONE, None),
        ] {
            let ctx = egui::Context::default();
            let original = track();
            let pending = vec![[-97.0, 35.0], [-96.9, 35.0]];
            let mut st = StormTracks {
                tracks: vec![original.clone()],
                selected: Some(0),
                pending: pending.clone(),
                ..Default::default()
            };
            let _ = ctx.run_ui(key_input(key, modifiers, text), |_| {
                st.keys(&ctx, true, true);
                assert_eq!(st.tracks, vec![original.clone()]);
                assert_eq!(st.pending, pending);
                assert!(
                    ctx.input_mut(|i| i.consume_key(modifiers, key)),
                    "Settings key capture still receives {key:?}"
                );
            });
        }
    }

    #[test]
    fn focused_card_button_permits_shortcuts_after_tool_is_disarmed() {
        let ctx = egui::Context::default();
        let mut st = StormTracks {
            tracks: vec![track()],
            selected: Some(0),
            ..Default::default()
        };
        let draw = |ctx: &egui::Context| {
            egui::Area::new(egui::Id::new("storm_track_card"))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| ui.button("Track control"))
        };
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {
            draw(&ctx).inner.request_focus();
        });
        let before = st.tracks[0].cone_deg;
        let _ = ctx.run_ui(
            key_input(egui::Key::CloseBracket, egui::Modifiers::NONE, Some("]")),
            |_| {
                st.keys(&ctx, false, false);
                assert_eq!(st.tracks[0].cone_deg, before + 2.0);
                assert!(crate::hotkeys::poll(&ctx, &crate::hotkeys::defaults()).is_empty());
                draw(&ctx);
            },
        );
    }

    #[test]
    fn text_editor_retains_brackets_and_disarmed_tool_restores_pane_keys() {
        let ctx = egui::Context::default();
        let original = track();
        let mut st = StormTracks {
            tracks: vec![original.clone()],
            selected: Some(0),
            ..Default::default()
        };
        let mut value = String::new();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.text_edit_singleline(&mut value).request_focus();
        });
        let _ = ctx.run_ui(
            key_input(egui::Key::CloseBracket, egui::Modifiers::NONE, Some("]")),
            |ui| {
                assert!(ctx.text_edit_focused());
                st.keys(&ctx, true, false);
                assert_eq!(st.tracks, vec![original.clone()]);
                assert!(crate::hotkeys::poll(&ctx, &crate::hotkeys::defaults()).is_empty());
                ui.text_edit_singleline(&mut value).surrender_focus();
            },
        );
        assert_eq!(value, "]");
        let _ = ctx.run_ui(
            key_input(egui::Key::CloseBracket, egui::Modifiers::NONE, Some("]")),
            |_| {
                st.keys(&ctx, false, false);
                assert_eq!(st.tracks, vec![original.clone()]);
                assert_eq!(
                    crate::hotkeys::poll(&ctx, &crate::hotkeys::defaults()),
                    vec![crate::hotkeys::BindableAction::FocusNextPane]
                );
            },
        );
    }

    fn editing(line: bool, origin: bool, created: bool) -> StormTracks {
        let mut before = track();
        if line {
            before.edge = vec![[-97.1, 35.0], [-96.9, 35.0], [-96.8, 35.1]];
        }
        let mut changed = before.clone();
        if origin {
            changed.move_to([-98.0, 36.0]);
        } else {
            changed.aim(destination_point(changed.origin, 180.0, 30.0), false);
        }
        StormTracks {
            tracks: vec![changed],
            selected: Some(0),
            drag: Some(TrackDrag {
                grab: if origin {
                    Grab::Origin(0)
                } else {
                    Grab::Head(0)
                },
                pane: 1,
                created,
                before,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn other_panes_cannot_take_over_or_release_an_active_drag() {
        let mut st = editing(true, true, false);
        let edited = st.tracks[0].clone();
        assert!(!st.accepts_pointer(0, true, true, false));
        assert!(!st.accepts_pointer(0, false, false, true));
        assert!(st.owns_drag(1));
        assert_eq!(st.tracks, vec![edited]);
        assert!(st.accepts_pointer(1, true, false, true));
        st.finish_drag();
        assert!(st.drag.is_none());
    }

    #[test]
    fn gesture_takeover_and_lost_pointer_restore_existing_edits() {
        for line in [false, true] {
            for origin in [false, true] {
                for gesture in [false, true] {
                    let mut st = editing(line, origin, false);
                    let before = st.drag.as_ref().unwrap().before.clone();
                    assert!(!st.accepts_pointer(1, !gesture, gesture, false));
                    assert_eq!(st.tracks, vec![before]);
                    assert_eq!(st.selected, Some(0));
                    assert!(st.drag.is_none());
                    assert!(
                        st.accepts_pointer(0, true, true, false),
                        "a fresh press is not claimed by an abandoned handle"
                    );
                }
            }
        }
    }

    #[test]
    fn interrupted_new_vectors_disappear_and_lines_return_to_construction() {
        for line in [false, true] {
            let mut st = editing(line, false, true);
            let edge = st.drag.as_ref().unwrap().before.edge.clone();
            st.cancel_drag();
            assert!(st.tracks.is_empty() && st.drag.is_none());
            assert_eq!(st.pending, edge);
            assert!(st.selected.is_none());
        }
    }

    #[test]
    fn drag_constraints_preserve_initial_motion_across_frames() {
        for line in [false, true] {
            let mut t = track();
            if line {
                t.edge = vec![[-97.1, 35.0], [-96.9, 35.0]];
            }
            let from = if line { t.edge[0] } else { t.origin };
            let drag = TrackDrag {
                grab: Grab::Head(0),
                pane: 0,
                created: false,
                before: t.clone(),
            };
            let original = t.clone();
            for km in [30.0, 45.0] {
                drag.aim(
                    &mut t,
                    from,
                    destination_point(from, 170.0, km),
                    egui::Modifiers::CTRL,
                );
                assert_eq!(t.bearing_deg, original.bearing_deg);
                assert!((t.speed_kmh - km).abs() < 0.01);
            }
            drag.aim(
                &mut t,
                from,
                destination_point(from, 173.0, 20.0),
                egui::Modifiers {
                    alt: true,
                    shift: true,
                    ..Default::default()
                },
            );
            assert_eq!(t.speed_kmh, original.speed_kmh);
            assert_eq!(t.bearing_deg, 175.0);
            assert_eq!(t.origin, original.origin);
            assert_eq!(t.edge, original.edge);
            assert_eq!(t.t0, original.t0);
            assert_eq!(t.left_width_km, original.left_width_km);
            assert_eq!(t.right_width_km, original.right_width_km);
        }
    }

    #[test]
    fn slow_existing_edits_survive_but_empty_new_drags_are_discarded() {
        for line in [false, true] {
            for created in [false, true] {
                let mut t = track();
                t.speed_kmh = 1.0;
                if line {
                    t.edge = vec![[-97.1, 35.0], [-96.9, 35.0]];
                }
                let edge = t.edge.clone();
                let mut st = StormTracks {
                    tracks: vec![t.clone()],
                    selected: Some(0),
                    drag: Some(TrackDrag {
                        grab: if line {
                            Grab::Motion(0, t.origin)
                        } else {
                            Grab::Head(0)
                        },
                        pane: 0,
                        created,
                        before: t.clone(),
                    }),
                    ..Default::default()
                };
                st.finish_drag();
                assert!(st.drag.is_none());
                if created {
                    assert!(st.tracks.is_empty() && st.selected.is_none());
                    assert_eq!(st.pending, edge);
                } else {
                    assert_eq!(st.tracks, vec![t]);
                    assert_eq!(st.selected, Some(0));
                    assert!(st.pending.is_empty());
                }
            }
        }
    }

    #[test]
    fn new_vector_can_be_created_with_constraint_modifiers_held() {
        let mut t = ManualTrack::new([-97.0, 35.0], Utc::now());
        let from = t.origin;
        let drag = TrackDrag {
            grab: Grab::Head(0),
            pane: 0,
            created: true,
            before: t.clone(),
        };
        drag.aim(
            &mut t,
            from,
            destination_point(from, 93.0, 60.0),
            egui::Modifiers {
                ctrl: true,
                alt: true,
                shift: true,
                ..Default::default()
            },
        );
        assert_eq!(t.bearing_deg, 95.0);
        assert!((t.speed_kmh - 60.0).abs() < 0.01);
    }

    #[test]
    fn a_manual_motion_reads_against_the_scit_motion_it_started_from() {
        let t0 = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let cell = wxdata::level3::Cell {
            id: "O7".into(),
            lon: -97.0,
            lat: 35.0,
            time: Some(t0),
            mvt_deg: Some(240.0),
            mvt_kt: Some(30.0),
            ..Default::default()
        };
        let mut t = ManualTrack::from_cell(&cell, t0).unwrap();
        let src = t.source.clone().unwrap();
        assert_eq!((src.cell_id.as_str(), src.scan), ("O7", Some(t0)));
        let fmt = |d: DateTime<Utc>| format!("t{}", d.timestamp() - 1_700_000_000);
        assert_eq!(
            manual_motion_line(&t, Some(t0), true, fmt),
            "WSW at 56 km/h (240°), set for t0; SCIT's motion for cell O7, unadjusted"
        );
        t.bearing_deg = 260.0;
        t.speed_kmh = 40.0;
        let later = t0 + chrono::Duration::minutes(10);
        assert_eq!(
            manual_motion_line(&t, Some(later), false, fmt),
            "W at 25 mph (260°), set for t0, 10 min before this scan; adjusted from SCIT's WSW \
             at 35 mph for cell O7"
        );
        // A track drawn by hand belongs to no storm and says nothing about SCIT.
        let free = ManualTrack::new([-97.0, 35.0], t0);
        assert!(free.source.is_none());
        assert!(!manual_motion_line(&free, None, true, fmt).contains("SCIT"));
    }

    #[test]
    fn a_saved_track_reopens_as_it_was_and_historical() {
        let t0 = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let cell = wxdata::level3::Cell {
            id: "O7".into(),
            lon: -97.0,
            lat: 35.0,
            time: Some(t0),
            mvt_deg: Some(240.0),
            mvt_kt: Some(30.0),
            ..Default::default()
        };
        let mut t = ManualTrack::from_cell(&cell, t0).unwrap();
        t.bearing_deg = 255.0;
        t.left_width_km = 5.0;
        let saved = t.to_case();
        let json = serde_json::to_string(&saved).unwrap();
        let back = ManualTrack::from_case(&serde_json::from_str(&json).unwrap());
        assert!(back.historical, "never a live estimate on reopening");
        assert_eq!(
            ManualTrack {
                historical: false,
                ..back.clone()
            },
            t,
            "everything else as saved, its source storm included"
        );
        // A hand-edited file cannot make a nonsense motion.
        let mut bad = saved;
        bad.speed_kmh = f64::NAN;
        bad.bearing_deg = -30.0;
        bad.cone_deg = 400.0;
        let b = ManualTrack::from_case(&bad);
        assert_eq!((b.speed_kmh, b.bearing_deg, b.cone_deg), (0.0, 330.0, 45.0));
        // A case written before tracks were kept opens with none.
        let old: crate::case::CaseManifest = serde_json::from_str(
            &serde_json::to_string(&serde_json::json!({
                "format": 1, "name": "x", "app_version": "0", "created_utc": "2026-01-01T00:00:00Z",
                "time_utc": null, "span_min": 0, "sites": [],
                "workspace": {"name": "x", "panes": []}
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(old.manual_tracks.is_empty());
    }

    #[test]
    fn a_track_exports_its_path_and_swath_with_its_motion_and_time() {
        let t = track(); // due east at 60 km/h from 35N 97W
        let f = t.to_features();
        assert_eq!(f.len(), 2);
        let wxdata::gis::Geometry::LineString(path) = &f[0].geometry else {
            panic!("{:?}", f[0].geometry);
        };
        assert_eq!(path.len(), 13, "every 5 minutes over the hour");
        let (km, brg) = great_circle(path[0], path[12]);
        assert!(
            (km - 60.0).abs() < 0.1 && (brg - 90.0).abs() < 1.0,
            "{km} {brg}"
        );
        let wxdata::gis::Geometry::Polygon(rings) = &f[1].geometry else {
            panic!("{:?}", f[1].geometry);
        };
        assert_eq!(rings[0].first(), rings[0].last(), "a closed ring");
        let p = &f[1].properties;
        assert_eq!(p["hookecho"], "manual-track-swath");
        assert_eq!(p["speed_kmh"], 60.0);
        assert_eq!(p["historical"], false);
        assert_eq!(p["t0"], t.t0.to_rfc3339());
        // Read back through the app's own importer.
        let back = crate::gis_import::load_geojson(&wxdata::gis::to_geojson(&f)).unwrap();
        assert_eq!(back.features.len(), 2);
    }

    fn track() -> ManualTrack {
        // Due east at 60 km/h from 35N 97W.
        let mut t = ManualTrack::new([-97.0, 35.0], Utc::now());
        t.aim(destination_point([-97.0, 35.0], 90.0, 60.0), false);
        t
    }

    #[test]
    fn independent_widths_match_the_drawn_footprint_and_point_arrivals() {
        let mut t = track();
        t.cone_deg = 0.0;
        t.left_width_km = 9.0;
        t.right_width_km = 2.0;
        let center = t.at(30.0);
        let left = destination_point(center, t.bearing_deg - 90.0, 6.0);
        let right = destination_point(center, t.bearing_deg + 90.0, 6.0);
        let ring = t.footprints().pop().unwrap();
        for (point, expected) in [(left, true), (right, false)] {
            assert_eq!(t.eta(point).unwrap().in_path, expected);
            assert_eq!(
                wxdata::overlay::point_in_ring(&ring, point[0], point[1]),
                expected
            );
        }
        let before = t.impact_id();
        t.right_width_km = 9.0;
        assert_ne!(t.impact_id(), before);
        assert!(t.eta(right).unwrap().in_path);
        let before = t.impact_id();
        t.left_width_km = 2.0;
        assert_ne!(t.impact_id(), before);
        assert!(!t.eta(left).unwrap().in_path);
    }

    #[test]
    fn cone_expansion_preserves_the_selected_side_widths() {
        let mut t = track();
        t.left_width_km = 2.0;
        t.right_width_km = 8.0;
        assert_eq!(t.width_at(0.0, false), 2.0);
        assert_eq!(t.width_at(0.0, true), 8.0);
        let expansion = 30.0 * t.cone_deg.to_radians().tan();
        assert!((t.width_at(30.0, false) - 2.0 - expansion).abs() < 1e-9);
        assert!((t.width_at(30.0, true) - 8.0 - expansion).abs() < 1e-9);
    }

    #[test]
    fn zones_inside_the_uncertainty_band_are_found_between_center_and_flank() {
        let mut t = track();
        t.cone_deg = 0.0;
        t.left_width_km = 9.0;
        t.right_width_km = 2.0;
        let center = t.at(30.0);
        let small_zone = |p: [f64; 2]| {
            vec![
                [p[0] - 0.005, p[1] - 0.005],
                [p[0] + 0.005, p[1] - 0.005],
                [p[0] + 0.005, p[1] + 0.005],
                [p[0] - 0.005, p[1] + 0.005],
            ]
        };
        let left = small_zone(destination_point(center, t.bearing_deg - 90.0, 6.0));
        let right = small_zone(destination_point(center, t.bearing_deg + 90.0, 6.0));
        assert!(t.zone_eta(&left).unwrap().grazes);
        assert!(t.zone_eta(&right).is_none());
        t.left_width_km = 2.0;
        t.right_width_km = 9.0;
        assert!(t.zone_eta(&left).is_none());
        assert!(t.zone_eta(&right).unwrap().grazes);
    }

    #[test]
    fn configurable_projection_marks_keep_the_hour_endpoint() {
        let mut t = track();
        assert_eq!(
            t.projection_marks().collect::<Vec<_>>(),
            [15.0, 30.0, 45.0, 60.0]
        );
        t.mark_interval_min = 10;
        assert_eq!(
            t.projection_marks().collect::<Vec<_>>(),
            [10.0, 20.0, 30.0, 40.0, 50.0, 60.0]
        );
        t.mark_interval_min = 17;
        assert_eq!(
            t.projection_marks().collect::<Vec<_>>(),
            [17.0, 34.0, 51.0, 60.0]
        );
        t.mark_interval_min = 0;
        assert_eq!(t.projection_marks().count(), 12);
        t.mark_interval_min = u32::MAX;
        assert_eq!(t.projection_marks().collect::<Vec<_>>(), [60.0]);
    }

    #[test]
    fn changing_marker_spacing_preserves_motion_impacts_and_line_geometry() {
        let mut t = line();
        let point = destination_point(t.origin, t.bearing_deg, 30.0);
        let before = (t.head(), t.footprints(), t.impact_id(), t.eta(point));
        t.mark_interval_min = 5;
        assert_eq!(
            (t.head(), t.footprints(), t.impact_id(), t.eta(point)),
            before
        );
        assert_eq!(t.projection_marks().last(), Some(HORIZON_MIN));
    }

    #[test]
    fn the_drag_sets_an_hours_motion() {
        let t = track();
        assert!((t.speed_kmh - 60.0).abs() < 0.1, "{}", t.speed_kmh);
        assert!((t.bearing_deg - 90.0).abs() < 0.5, "{}", t.bearing_deg);
        let mut s = t.clone();
        s.aim(destination_point([-97.0, 35.0], 93.0, 60.0), true);
        assert_eq!(s.bearing_deg, 95.0, "shift snaps to 5°");
    }

    #[test]
    fn a_point_ahead_gets_an_eta_and_its_closest_approach() {
        let t = track();
        // 30 km east, 2 km north: in the swath (3 km + 30 km * tan 10°), passing on the left.
        let p = destination_point(destination_point(t.origin, 90.0, 30.0), 0.0, 2.0);
        let e = t.eta(p).unwrap();
        assert!((e.minutes - 30.0).abs() < 0.5, "{e:?}");
        assert!(
            (e.closest_km - 2.0).abs() < 0.1 && !e.right && e.in_path,
            "{e:?}"
        );
        // 10 km off at 10 km along: outside the band (3 + 1.8 km).
        let q = destination_point(destination_point(t.origin, 90.0, 10.0), 180.0, 10.0);
        let e = t.eta(q).unwrap();
        assert!(e.right && !e.in_path, "{e:?}");
    }

    #[test]
    fn behind_too_far_or_standing_still_has_no_eta() {
        let t = track();
        assert!(
            t.eta(destination_point(t.origin, 270.0, 10.0)).is_none(),
            "behind"
        );
        assert!(
            t.eta(destination_point(t.origin, 90.0, 150.0)).is_none(),
            "past two hours"
        );
        let mut still = t.clone();
        still.speed_kmh = 0.0;
        assert!(still.eta(destination_point(t.origin, 90.0, 10.0)).is_none());
    }

    #[test]
    fn the_cone_widens_the_swath_and_zero_keeps_it_a_band() {
        let mut t = track();
        let width = |t: &ManualTrack| {
            let s = t.swath();
            great_circle(s[12], s[13]).0
        };
        let wide = width(&t);
        t.cone_deg = 0.0;
        assert!((width(&t) - 6.0).abs() < 0.1, "{}", width(&t));
        assert!(wide > 6.0 + 2.0 * 60.0 * 0.17);
    }

    /// A north-south line moving east at 60 km/h, 20 km long.
    fn line() -> ManualTrack {
        let o = [-97.0, 35.0];
        let edge = vec![
            destination_point(o, 0.0, 10.0),
            o,
            destination_point(o, 180.0, 10.0),
        ];
        let mut t = ManualTrack::line(edge, Utc::now());
        t.aim_from(o, destination_point(o, 90.0, 60.0), false);
        t
    }

    #[test]
    fn a_line_arrives_where_it_crosses_a_point() {
        let t = line();
        assert_eq!(t.origin, [-97.0, 35.0], "the middle point");
        // 30 km east, 5 km north: within the line's span, crossed in 30 minutes.
        let p = destination_point(destination_point(t.origin, 90.0, 30.0), 0.0, 5.0);
        let e = t.eta(p).unwrap();
        assert!((e.minutes - 30.0).abs() < 0.6 && e.in_path, "{e:?}");
        assert_eq!(e.closest_km, 0.0);
        // 30 km east, 20 km north: 10 km past the north end, outside its widening swath.
        let q = destination_point(destination_point(t.origin, 90.0, 30.0), 0.0, 20.0);
        let e = t.eta(q).unwrap();
        assert!(
            !e.in_path && (e.closest_km - 10.0).abs() < 0.3 && !e.right,
            "{e:?}"
        );
        // Behind the line: nothing.
        assert!(t.eta(destination_point(t.origin, 270.0, 5.0)).is_none());
    }

    #[test]
    fn a_slanted_line_reaches_its_near_end_first() {
        // The north end sits 10 km further east: a point due east of it is reached sooner.
        let o = [-97.0, 35.0];
        let north = destination_point(destination_point(o, 0.0, 10.0), 90.0, 10.0);
        let mut t = ManualTrack::line(vec![north, destination_point(o, 180.0, 10.0)], Utc::now());
        t.aim_from(o, destination_point(o, 90.0, 60.0), false);
        let east_of = |km_north: f64| {
            let p = destination_point(destination_point(o, 0.0, km_north), 90.0, 40.0);
            t.eta(p).unwrap().minutes
        };
        assert!(east_of(8.0) < east_of(-8.0));
    }

    #[test]
    fn moving_a_line_takes_its_edge_along() {
        let mut t = line();
        let before = t.edge_at(30.0);
        t.move_to([-96.0, 35.0]);
        assert!(
            (t.edge[1][0] + 96.0).abs() < 1e-9,
            "the middle is the origin"
        );
        let after = t.edge_at(30.0);
        assert!((after[1][0] - before[1][0] - 1.0).abs() < 1e-6);
    }

    /// A 10 km box whose west side is `km_east` east of the origin and whose middle is
    /// `km_north` north of it.
    fn zone(o: [f64; 2], km_east: f64, km_north: f64) -> Vec<[f64; 2]> {
        let sw = destination_point(destination_point(o, 90.0, km_east), 180.0, 5.0 - km_north);
        let se = destination_point(sw, 90.0, 10.0);
        let ne = destination_point(se, 0.0, 10.0);
        let nw = destination_point(sw, 0.0, 10.0);
        vec![sw, se, ne, nw]
    }

    #[test]
    fn a_zone_ahead_is_entered_on_time_and_one_beside_is_grazed() {
        let t = track();
        let e = t.zone_eta(&zone(t.origin, 30.0, 0.0)).unwrap();
        assert!((e.minutes - 31.0).abs() <= 1.0 && !e.grazes, "{e:?}");
        // 12 km north of the path: the storm misses, the widening swath reaches it.
        let e = t.zone_eta(&zone(t.origin, 30.0, 12.0)).unwrap();
        assert!(e.grazes, "{e:?}");
        assert!(t.zone_eta(&zone(t.origin, 30.0, 60.0)).is_none(), "far off");
        let e = t.zone_eta(&zone(t.origin, -5.0, 0.0)).unwrap();
        assert_eq!(e.minutes, 0.0, "already inside");
    }

    #[test]
    fn a_line_enters_a_zone_with_any_part_of_it() {
        // The box is 8 km north of the line's middle: only the line's north half reaches it.
        let t = line();
        let e = t.zone_eta(&zone(t.origin, 30.0, 8.0)).unwrap();
        assert!(!e.grazes && (e.minutes - 31.0).abs() <= 1.0, "{e:?}");
    }

    #[test]
    fn a_line_crosses_small_zones_between_its_old_sample_points() {
        let t = line();
        let p = destination_point(t.edge_at(30.0)[1], 0.0, 2.5);
        let zone = [
            [p[0] - 0.003, p[1] - 0.003],
            [p[0] + 0.003, p[1] - 0.003],
            [p[0] + 0.003, p[1] + 0.003],
            [p[0] - 0.003, p[1] + 0.003],
        ];
        let e = t.zone_eta(&zone).expect("the segment crosses the zone");
        assert!(!e.grazes && (e.minutes - 30.0).abs() <= 1.0, "{e:?}");
    }

    #[test]
    fn line_uncertainty_is_in_drawn_parts_zone_checks_and_population_coverage() {
        let mut t = line();
        t.cone_deg = 0.0;
        t.left_width_km = 9.0;
        t.right_width_km = 2.0;
        let center = t.at(30.0);
        let left = destination_point(center, 0.0, 16.0);
        let right = destination_point(center, 180.0, 16.0);
        let covered = |t: &ManualTrack, p: [f64; 2]| {
            t.footprints()
                .iter()
                .any(|r| wxdata::overlay::point_in_ring(r, p[0], p[1]))
        };
        assert!(covered(&t, left));
        assert!(!covered(&t, right));
        assert!(t.eta(left).unwrap().in_path);
        assert!(!t.eta(right).unwrap().in_path);
        let zone = [
            [left[0] - 0.003, left[1] - 0.003],
            [left[0] + 0.003, left[1] - 0.003],
            [left[0] + 0.003, left[1] + 0.003],
            [left[0] - 0.003, left[1] + 0.003],
        ];
        assert!(t.zone_eta(&zone).unwrap().grazes);
        t.edge.reverse();
        assert!(covered(&t, left));
        assert!(t.zone_eta(&zone).unwrap().grazes);
    }

    #[test]
    fn impact_identity_follows_vertex_edits_even_without_a_vertex_count_change() {
        let t = line();
        let mut u = t.clone();
        u.edge[0][0] += 0.00001;
        assert_ne!(t.impact_id(), u.impact_id());
        u = t.clone();
        u.bearing_deg += 0.01;
        assert_ne!(t.impact_id(), u.impact_id());
    }

    #[test]
    fn a_bent_line_uses_interior_vertices_for_uncertainty_arrivals() {
        let mut t = line();
        let middle = destination_point(t.origin, 0.0, 10.0);
        t.edge = vec![
            destination_point(t.origin, 180.0, 10.0),
            middle,
            destination_point(t.origin, 180.0, 8.0),
        ];
        t.cone_deg = 0.0;
        t.left_width_km = 9.0;
        let p = destination_point(destination_point(middle, 90.0, 30.0), 0.0, 6.0);
        assert!(t.eta(p).unwrap().in_path);
    }

    #[test]
    fn envelopes_preserve_empty_space_inside_a_bent_line() {
        let mut t = line();
        let point =
            |east, north| destination_point(destination_point(t.origin, 90.0, east), 0.0, north);
        let edge = vec![point(0.0, 0.0), point(10.0, 0.0), point(10.0, 10.0)];
        let gap = point(8.0, 5.0);
        let covered = point(10.0, 5.0);
        t.edge = edge;
        t.speed_kmh = 1.0;
        t.bearing_deg = 0.0;
        t.cone_deg = 0.0;
        t.left_width_km = 0.2;
        t.right_width_km = 0.2;
        let parts = t.footprints();
        assert!(!parts
            .iter()
            .any(|r| wxdata::overlay::point_in_ring(r, gap[0], gap[1])));
        assert!(parts
            .iter()
            .any(|r| wxdata::overlay::point_in_ring(r, covered[0], covered[1])));
    }

    #[test]
    fn drawing_reuses_envelopes_until_the_geometry_changes() {
        let ctx = egui::Context::default();
        let mut t = line();
        let first = t.cached_footprints(&ctx, 0);
        t.mark_interval_min = 5;
        assert!(std::sync::Arc::ptr_eq(
            &first,
            &t.cached_footprints(&ctx, 0)
        ));
        t.left_width_km += 1.0;
        let changed = t.cached_footprints(&ctx, 0);
        assert!(!std::sync::Arc::ptr_eq(&first, &changed));
        assert_eq!(*changed, t.footprints());
    }

    #[test]
    fn the_footprint_covers_the_hour_and_its_key_follows_edits() {
        let t = line();
        let rings = t.footprints();
        assert_eq!(rings.len(), t.edge.len() - 1, "one swept part per segment");
        let inside = |p: [f64; 2]| {
            rings
                .iter()
                .any(|ring| wxdata::overlay::point_in_ring(ring, p[0], p[1]))
        };
        assert!(inside(destination_point(t.origin, 90.0, 30.0)));
        assert!(!inside(destination_point(t.origin, 90.0, 70.0)));
        let mut u = t.clone();
        assert_eq!(u.impact_id(), t.impact_id());
        u.speed_kmh += 5.0;
        assert_ne!(u.impact_id(), t.impact_id(), "an edit asks again");
        assert!(track().footprints()[0].len() > 4);
    }

    #[test]
    fn a_scit_cell_seeds_a_track_with_its_motion() {
        let cell = wxdata::level3::Cell {
            lon: -97.0,
            lat: 35.0,
            mvt_deg: Some(245.0),
            mvt_kt: Some(30.0),
            ..Default::default()
        };
        let t = ManualTrack::from_cell(&cell, Utc::now()).unwrap();
        assert_eq!(t.bearing_deg, 245.0);
        assert!((t.speed_kmh - 55.56).abs() < 0.01);
        assert!(ManualTrack::from_cell(&wxdata::level3::Cell::default(), Utc::now()).is_none());
    }

    #[test]
    fn a_fingertip_grabs_from_further_than_a_cursor() {
        assert!(grab_radius(true) >= 2.0 * grab_radius(false));
        assert_eq!(grab_radius(false), GRAB_PT);
    }

    #[test]
    fn headings_name_their_compass_point() {
        assert_eq!(compass(0.0), "N");
        assert_eq!(compass(359.0), "N");
        assert_eq!(compass(247.5), "WSW");
        assert_eq!(compass(90.0), "E");
    }
}
