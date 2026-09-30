//! The manual storm-motion tool (ROADMAP_2 §2.2): drag from a storm to where it will be in an
//! hour, and the projection follows the pointer — 15/30/45/60-minute marks, a swath that widens
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

/// How far ahead a track projects, and what the dragged vector stands for.
pub(crate) const HORIZON_MIN: f64 = 60.0;
const MARK_EVERY_MIN: f64 = 15.0;
/// Beyond this, a marker is too far ahead for the motion to say anything about it.
const ETA_MAX_MIN: f64 = 120.0;
const KMH_PER_KT: f64 = 1.852;
/// A pointer this close (points) to a handle grabs it rather than starting a new track.
const GRAB_PT: f32 = 12.0;

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
    /// Half-width of the swath at the origin.
    pub half_width_km: f64,
    /// Half-angle of the cone the swath widens by; zero keeps it a straight band.
    pub cone_deg: f64,
    /// A line track's storm edge at `t0` (a QLCS, a gust front), moving as one with the
    /// motion; empty for a single storm. The origin is its middle.
    pub edge: Vec<[f64; 2]>,
}

impl ManualTrack {
    fn new(origin: [f64; 2], t0: DateTime<Utc>) -> Self {
        Self {
            origin,
            bearing_deg: 0.0,
            speed_kmh: 0.0,
            t0,
            half_width_km: 3.0,
            cone_deg: 10.0,
            edge: Vec::new(),
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
        Some(t)
    }

    /// Where the storm is projected `min` minutes after `t0`.
    pub fn at(&self, min: f64) -> [f64; 2] {
        destination_point(self.origin, self.bearing_deg, self.speed_kmh * min / 60.0)
    }

    pub fn head(&self) -> [f64; 2] {
        self.at(HORIZON_MIN)
    }

    /// Point the hour's end at `head`. `snap` holds the heading to 5° steps.
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

    /// Half-width of the swath `km` along the track.
    fn half_width_at(&self, km: f64) -> f64 {
        self.half_width_km + km * self.cone_deg.to_radians().tan()
    }

    /// The swath's outline to the horizon, left edge out and right edge back.
    fn swath(&self) -> Vec<[f64; 2]> {
        const STEPS: usize = 12;
        let km = self.speed_kmh * HORIZON_MIN / 60.0;
        let edge = |i: usize, side: f64| {
            let d = km * i as f64 / STEPS as f64;
            let c = destination_point(self.origin, self.bearing_deg, d);
            destination_point(c, self.bearing_deg + 90.0 * side, self.half_width_at(d))
        };
        let mut out: Vec<[f64; 2]> = (0..=STEPS).map(|i| edge(i, -1.0)).collect();
        out.extend((0..=STEPS).rev().map(|i| edge(i, 1.0)));
        out
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
            // Past the line's ends: measured from the nearer end, whose swath widens as a
            // single storm's does.
            let (first, last) = (v[0], v[v.len() - 1]);
            let end = if (pc - first.1).abs() <= (pc - last.1).abs() {
                first
            } else {
                last
            };
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
            in_path: cross.abs() <= self.half_width_at(along),
        })
    }
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

#[derive(Default)]
pub(crate) struct StormTracks {
    pub tracks: Vec<ManualTrack>,
    pub selected: Option<usize>,
    drag: Option<Grab>,
    /// A line being clicked out, point by point, before its motion is dragged.
    pub pending: Vec<[f64; 2]>,
}

impl StormTracks {
    fn remove(&mut self, i: usize) {
        if i < self.tracks.len() {
            self.tracks.remove(i);
        }
        self.selected = None;
        self.drag = None;
    }
}

/// Compass point for a heading.
fn compass(deg: f64) -> &'static str {
    const P: [&str; 16] = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW",
        "NW", "NNW",
    ];
    P[((deg.rem_euclid(360.0) + 11.25) / 22.5) as usize % 16]
}

fn distance(km: f64, metric: bool) -> String {
    if metric {
        format!("{km:.0} km")
    } else {
        format!("{:.0} mi", km / KM_PER_MILE)
    }
}

impl HookEchoApp {
    /// The tool's pointer handling on one pane: a drag from open map starts a track, a drag from
    /// a handle moves that end, a click picks the track under it. Returns whether it took the
    /// drag, so the map does not also pan.
    pub(crate) fn storm_track_input(
        &mut self,
        idx: usize,
        prect: egui::Rect,
        response: &egui::Response,
        ui: &egui::Ui,
    ) -> bool {
        if self.tool != MapTool::StormTrack {
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
                .filter(|(_, d)| *d <= GRAB_PT)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(g, _)| g)
        };
        let shift = ui.input(|i| i.modifiers.shift);
        if response.drag_started() {
            if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
                self.active = idx;
                let grab = grab_at(p, &self.storm_tracks).unwrap_or_else(|| {
                    let t0 = self.views[idx]
                        .volume
                        .as_ref()
                        .map_or_else(Utc::now, |v| v.time);
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
                self.storm_tracks.drag = Some(grab);
            }
        }
        if let (Some(grab), Some(p)) = (self.storm_tracks.drag, response.interact_pointer_pos()) {
            let ll = to_ll(p);
            let st = &mut self.storm_tracks;
            match grab {
                Grab::Head(i) => {
                    if let Some(t) = st.tracks.get_mut(i) {
                        t.aim(ll, shift);
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
                        t.aim_from(from, ll, shift);
                    }
                    st.selected = Some(i);
                }
            }
        }
        if response.drag_stopped() {
            let st = &mut self.storm_tracks;
            // A press that barely moved is a click, not a track.
            if let Some(Grab::Head(i)) = st.drag {
                if st.tracks.get(i).is_some_and(|t| t.speed_kmh < 2.0) {
                    st.remove(i);
                }
            }
            // A line whose motion drag barely moved goes back to being clicked out.
            if let Some(Grab::Motion(i, _)) = st.drag {
                if st.tracks.get(i).is_some_and(|t| t.speed_kmh < 2.0) {
                    let edge = st.tracks[i].edge.clone();
                    st.remove(i);
                    st.pending = edge;
                }
            }
            st.drag = None;
        }
        if response.clicked() {
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
        response.dragged() || self.storm_tracks.drag.is_some()
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
                let fill = col.gamma_multiply(if selected { 0.16 } else { 0.09 });
                if t.is_line() {
                    // The swept area, a quad per segment (the whole is not convex); then the
                    // edge every 15 minutes, thin, and now, solid.
                    let now: Vec<egui::Pos2> = t.edge.iter().map(|p| to_px(*p)).collect();
                    let end: Vec<egui::Pos2> =
                        t.edge_at(HORIZON_MIN).into_iter().map(to_px).collect();
                    for k in 0..now.len() - 1 {
                        painter.add(egui::Shape::convex_polygon(
                            vec![now[k], now[k + 1], end[k + 1], end[k]],
                            fill,
                            egui::Stroke::NONE,
                        ));
                    }
                    let mut m = MARK_EVERY_MIN;
                    while m <= HORIZON_MIN {
                        let at: Vec<egui::Pos2> = t.edge_at(m).into_iter().map(to_px).collect();
                        painter.add(egui::Shape::line(
                            at,
                            egui::Stroke::new(1.0, col.gamma_multiply(0.8)),
                        ));
                        m += MARK_EVERY_MIN;
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
                let mut m = MARK_EVERY_MIN;
                while m <= HORIZON_MIN {
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
                    m += MARK_EVERY_MIN;
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
        if !armed {
            // A line half clicked out is dropped with the tool.
            self.storm_tracks.pending.clear();
            if self.storm_tracks.selected.is_none() {
                return;
            }
        }
        let typing = ctx.memory(|m| m.focused().is_some());
        if !self.storm_tracks.pending.is_empty()
            && !typing
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace))
        {
            self.storm_tracks.pending.pop();
        }
        if self
            .storm_tracks
            .selected
            .is_some_and(|i| i >= self.storm_tracks.tracks.len())
        {
            self.storm_tracks.selected = None;
        }
        if let (Some(i), false) = (self.storm_tracks.selected, typing) {
            let (del, dup, narrow, widen) = ctx.input_mut(|inp| {
                (
                    inp.consume_key(egui::Modifiers::NONE, egui::Key::Delete),
                    inp.consume_key(egui::Modifiers::COMMAND, egui::Key::D),
                    inp.consume_key(egui::Modifiers::NONE, egui::Key::OpenBracket),
                    inp.consume_key(egui::Modifiers::NONE, egui::Key::CloseBracket),
                )
            });
            if del {
                self.storm_tracks.remove(i);
            } else if dup {
                let copy = self.storm_tracks.tracks[i].clone();
                self.storm_tracks.tracks.push(copy);
                self.storm_tracks.selected = Some(self.storm_tracks.tracks.len() - 1);
            } else if narrow || widen {
                let t = &mut self.storm_tracks.tracks[i];
                t.cone_deg = (t.cone_deg + if widen { 2.0 } else { -2.0 }).clamp(0.0, 45.0);
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
        let st = &mut self.storm_tracks;
        let mut remove = None;
        let mut clear = false;
        egui::Area::new(egui::Id::new("storm_track_card"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(area.left_bottom() + egui::vec2(12.0, -40.0))
            .constrain_to(area)
            .show(ctx, |ui| {
                crate::ui::workstation::card_frame(&t).show(ui, |ui| {
                    crate::ui::workstation::style_scope(ui, &t);
                    ui.set_width(300.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Storm motion").strong().color(color()));
                        ui.label(egui::RichText::new("manual").small().weak());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !st.tracks.is_empty() && ui.small_button("Clear all").clicked() {
                                clear = true;
                            }
                        });
                    });
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
                        ui.horizontal(|ui| {
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
                            let mut kt = track.speed_kmh / KMH_PER_KT;
                            if ui
                                .add(
                                    egui::DragValue::new(&mut kt)
                                        .range(0.0..=150.0)
                                        .speed(0.5)
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
                        });
                        if !sel {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            ui.weak("Cone");
                            ui.add(
                                egui::DragValue::new(&mut track.cone_deg)
                                    .range(0.0..=45.0)
                                    .speed(0.5)
                                    .suffix("°"),
                            )
                            .on_hover_text("Half-angle the swath widens by ([ and ])");
                            ui.weak("Width");
                            let mut w = if metric {
                                track.half_width_km
                            } else {
                                track.half_width_km / KM_PER_MILE
                            };
                            if ui
                                .add(
                                    egui::DragValue::new(&mut w)
                                        .range(0.0..=50.0)
                                        .speed(0.2)
                                        .suffix(if metric { " km" } else { " mi" }),
                                )
                                .on_hover_text("Half-width of the swath at the storm")
                                .changed()
                            {
                                track.half_width_km = if metric { w } else { w * KM_PER_MILE };
                            }
                            ui.weak(format!("from {}", track.t0.format("%H:%MZ")));
                        });
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
                            let when =
                                track.t0 + chrono::Duration::seconds((e.minutes * 60.0) as i64);
                            let pass = if track.is_line() && e.closest_km == 0.0 {
                                "line arrives".to_string()
                            } else if e.closest_km < 0.5 {
                                "direct hit".to_string()
                            } else {
                                format!(
                                    "passes {} {}",
                                    distance(e.closest_km, metric),
                                    compass(track.bearing_deg + if e.right { -90.0 } else { 90.0 })
                                )
                            };
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(if e.in_path { "●" } else { "○" })
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
                    }
                    ui.weak("Drag either end to edit · Ctrl+D duplicates · Delete removes");
                });
            });
        if clear {
            st.tracks.clear();
            st.selected = None;
        } else if let Some(i) = remove {
            st.remove(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> ManualTrack {
        // Due east at 60 km/h from 35N 97W.
        let mut t = ManualTrack::new([-97.0, 35.0], Utc::now());
        t.aim(destination_point([-97.0, 35.0], 90.0, 60.0), false);
        t
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
    fn headings_name_their_compass_point() {
        assert_eq!(compass(0.0), "N");
        assert_eq!(compass(359.0), "N");
        assert_eq!(compass(247.5), "WSW");
        assert_eq!(compass(90.0), "E");
    }
}
