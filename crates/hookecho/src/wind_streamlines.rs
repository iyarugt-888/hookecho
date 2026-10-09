//! Wind streamlines (ROADMAP_PARITY M5.3): the wind as evenly spaced lines that follow it, with
//! arrowheads downstream, coloured by speed on the same ramp as the barbs and particles.
//!
//! Barbs give a value at a point and particles give motion; neither shows the flow's structure
//! at a glance — where air converges into a line, curls round a low or fans out — which is what
//! streamlines are for. They are laid out in screen space by the evenly spaced method of Jobard
//! and Lefer (1997): seeds on a lattice; each line traced both ways from its seed with
//! midpoint (RK2) steps until it leaves the pane, the wind goes calm or missing, it comes within
//! half the spacing of a line already drawn (or of its own earlier course, so a vortex does not
//! spiral forever), or it reaches its length limit. A seed within the full spacing of a drawn
//! line is skipped. The direction at each step is the wind's own bearing projected through the
//! camera, as the barbs do, so a rotated, pitched or globe view still reads true.

use crate::render::mercator::{lonlat_to_world, world_to_lonlat, Camera};
use egui::{Color32, Pos2, Vec2};

/// Screen spacing between neighbouring streamlines, px.
pub const STREAM_SPACING_PX: f32 = 32.0;

/// Integration step, px.
const STEP_PX: f32 = 3.0;

/// The most steps one direction of one line takes.
const MAX_STEPS: usize = 220;

/// Below this the air is calm enough that a direction means little, m/s.
const CALM_MS: f32 = 0.5;

/// Distance between arrowheads along a line, px.
const ARROW_EVERY_PX: f32 = 140.0;

/// One streamline: its points in pane pixels, downstream order, and the wind speed at each, kt.
#[derive(Debug, Clone, PartialEq)]
pub struct Streamline {
    pub pts: Vec<Pos2>,
    pub kt: Vec<f32>,
}

/// The wind at a screen point: the unit screen vector it blows toward and its speed (m/s).
fn flow_at(
    sample: &impl Fn(f64, f64) -> Option<(f32, f32)>,
    cam: &Camera,
    vp: (f32, f32),
    p: Pos2,
) -> Option<(Vec2, f32)> {
    if !(0.0..=vp.0).contains(&p.x) || !(0.0..=vp.1).contains(&p.y) {
        return None;
    }
    let w = cam.screen_to_world((p.x, p.y), vp);
    let (lon, lat) = world_to_lonlat(w.0, w.1);
    let (u, v) = sample(lon, lat).filter(|(u, v)| u.is_finite() && v.is_finite())?;
    let speed = u.hypot(v);
    if speed < CALM_MS {
        return None;
    }
    // The bearing the air moves toward, projected: a short step that way on the ground, seen
    // through the camera.
    let toward = f64::from(u).atan2(f64::from(v)).to_degrees();
    let to = crate::geo::destination_point([lon, lat], toward, 10.0);
    let s = cam.world_to_screen(lonlat_to_world(to[0], to[1]), vp);
    let d = Vec2::new(s.0 - p.x, s.1 - p.y);
    (d.length() > 1e-4).then(|| (d.normalized(), speed))
}

/// Points already drawn, bucketed by screen cell, to ask "is anything within `r` of here?".
struct Occupancy {
    cell: f32,
    cols: usize,
    rows: usize,
    /// Per cell, its points and the line (and step) each belongs to.
    cells: Vec<Vec<(Pos2, usize, usize)>>,
}

impl Occupancy {
    fn new(vp: (f32, f32), cell: f32) -> Self {
        let cols = (vp.0 / cell).ceil().max(1.0) as usize;
        let rows = (vp.1 / cell).ceil().max(1.0) as usize;
        Occupancy {
            cell,
            cols,
            rows,
            cells: vec![Vec::new(); cols * rows],
        }
    }

    fn index(&self, p: Pos2) -> Option<(usize, usize)> {
        let (c, r) = ((p.x / self.cell).floor(), (p.y / self.cell).floor());
        (c >= 0.0 && r >= 0.0 && (c as usize) < self.cols && (r as usize) < self.rows)
            .then_some((c as usize, r as usize))
    }

    fn insert(&mut self, p: Pos2, line: usize, step: usize) {
        if let Some((c, r)) = self.index(p) {
            self.cells[r * self.cols + c].push((p, line, step));
        }
    }

    /// Whether any point within `r` of `p` passes `counts`.
    fn near(&self, p: Pos2, r: f32, counts: impl Fn(usize, usize) -> bool) -> bool {
        let reach = (r / self.cell).ceil() as isize;
        let Some((c, row)) = self.index(p) else {
            return false;
        };
        for dr in -reach..=reach {
            for dc in -reach..=reach {
                let (cc, rr) = (c as isize + dc, row as isize + dr);
                if cc < 0 || rr < 0 || cc as usize >= self.cols || rr as usize >= self.rows {
                    continue;
                }
                for &(q, line, step) in &self.cells[rr as usize * self.cols + cc as usize] {
                    if q.distance(p) < r && counts(line, step) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Evenly spaced streamlines `spacing` px apart over a `vp` pane, from east/north wind
/// components (m/s) given by `sample` at a longitude and latitude.
pub fn streamlines(
    sample: impl Fn(f64, f64) -> Option<(f32, f32)>,
    cam: &Camera,
    vp: (f32, f32),
    spacing: f32,
) -> Vec<Streamline> {
    let spacing = spacing.max(8.0);
    let test = spacing * 0.5;
    let mut grid = Occupancy::new(vp, test);
    let mut out: Vec<Streamline> = Vec::new();
    // How many steps back a line's own course counts as something to stop at: far enough that
    // its neighbouring points never do.
    let own_gap = (2.0 * spacing / STEP_PX).ceil() as usize;
    let mut y = spacing * 0.5;
    while y < vp.1 {
        let mut x = spacing * 0.5;
        while x < vp.0 {
            let seed = Pos2::new(x, y);
            x += spacing;
            if grid.near(seed, spacing, |_, _| true) {
                continue;
            }
            let id = out.len();
            // Trace one way, keeping this line's own points in the grid as it goes (each with a
            // step number counting outward from the seed, both ways).
            let trace = |dir: f32, grid: &mut Occupancy| -> Vec<(Pos2, f32)> {
                let mut pts = Vec::new();
                let mut p = seed;
                for step in 1..=MAX_STEPS {
                    let Some((d0, _)) = flow_at(&sample, cam, vp, p) else {
                        break;
                    };
                    let mid = p + d0 * (dir * STEP_PX * 0.5);
                    let Some((d1, _)) = flow_at(&sample, cam, vp, mid) else {
                        break;
                    };
                    let next = p + d1 * (dir * STEP_PX);
                    let Some((_, speed)) = flow_at(&sample, cam, vp, next) else {
                        break;
                    };
                    let tag = if dir > 0.0 { step } else { MAX_STEPS + step };
                    let blocked = grid.near(next, test, |line, s| {
                        line != id || {
                            // Its own points count only well behind on the same side.
                            let same_side = (s > MAX_STEPS) == (tag > MAX_STEPS);
                            let (a, b) = (s % (MAX_STEPS + 1), tag % (MAX_STEPS + 1));
                            !same_side && a + b > own_gap || same_side && b > a + own_gap
                        }
                    });
                    if blocked {
                        break;
                    }
                    grid.insert(next, id, tag);
                    pts.push((next, speed));
                    p = next;
                }
                pts
            };
            let Some((_, seed_speed)) = flow_at(&sample, cam, vp, seed) else {
                continue;
            };
            grid.insert(seed, id, 0);
            let ahead = trace(1.0, &mut grid);
            let behind = trace(-1.0, &mut grid);
            let n = ahead.len() + behind.len() + 1;
            if (n as f32) * STEP_PX < spacing {
                // Too short to read as a line: take its points back out.
                for cell in &mut grid.cells {
                    cell.retain(|&(_, line, _)| line != id);
                }
                continue;
            }
            let mut pts = Vec::with_capacity(n);
            let mut kt = Vec::with_capacity(n);
            for &(p, s) in behind.iter().rev() {
                pts.push(p);
                kt.push(s * 1.943_844);
            }
            pts.push(seed);
            kt.push(seed_speed * 1.943_844);
            for &(p, s) in &ahead {
                pts.push(p);
                kt.push(s * 1.943_844);
            }
            out.push(Streamline { pts, kt });
        }
        y += spacing;
    }
    out
}

/// How often a moving camera re-traces, at most. Between traces the lines stay pinned to the
/// ground (they are kept in world coordinates), so a pan carries them along and only the newly
/// uncovered edge waits for the next trace.
const RETRACE_EVERY: std::time::Duration = std::time::Duration::from_millis(200);

/// A traced line's points in world coordinates and its speeds, kt.
type WorldLine = (Vec<(f64, f64)>, Vec<f32>);

/// One pane's streamlines, kept in world coordinates until the camera, pane size or wind
/// changes: tracing walks tens of thousands of samples, too many to repeat on every frame of a
/// pan.
#[derive(Default)]
pub struct StreamCache {
    key: Option<u64>,
    traced: Option<std::time::Instant>,
    last_source: Option<(usize, i64)>,
    lines: Vec<WorldLine>,
}

impl StreamCache {
    /// The lines for this camera, pane and wind `source` (anything that changes with the wind's
    /// data, such as its buffer's address and valid time), in pane pixels, and whether they are
    /// waiting on a re-trace (ask for another frame). A new wind traces at once; a camera still
    /// moving re-traces at most every [`RETRACE_EVERY`].
    pub fn get(
        &mut self,
        cam: &Camera,
        vp: (f32, f32),
        source: (usize, i64),
        trace: impl FnOnce() -> Vec<Streamline>,
    ) -> (Vec<Streamline>, bool) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (
            cam.center.0.to_bits(),
            cam.center.1.to_bits(),
            cam.zoom.to_bits(),
        )
            .hash(&mut h);
        (cam.pitch.to_bits(), cam.bearing.to_bits()).hash(&mut h);
        (vp.0.to_bits(), vp.1.to_bits(), source).hash(&mut h);
        let key = h.finish();
        let fresh_source = self.last_source != Some(source);
        let mut stale = false;
        if self.key != Some(key) {
            if fresh_source || self.traced.is_none_or(|t| t.elapsed() >= RETRACE_EVERY) {
                self.lines = trace()
                    .into_iter()
                    .map(|l| {
                        let pts = l
                            .pts
                            .iter()
                            .map(|p| cam.screen_to_world((p.x, p.y), vp))
                            .collect();
                        (pts, l.kt)
                    })
                    .collect();
                self.key = Some(key);
                self.last_source = Some(source);
                self.traced = Some(std::time::Instant::now());
            } else {
                stale = true;
            }
        }
        let lines = self
            .lines
            .iter()
            .map(|(pts, kt)| Streamline {
                pts: pts
                    .iter()
                    .map(|&w| {
                        let (x, y) = cam.world_to_screen(w, vp);
                        Pos2::new(x, y)
                    })
                    .collect(),
                kt: kt.clone(),
            })
            .collect();
        (lines, stale)
    }
}

/// Where along a line its arrowheads go: one per [`ARROW_EVERY_PX`], the first half that far
/// in, each as the point and the unit direction downstream. A line shorter than that gets one
/// at its middle.
pub fn arrowheads(line: &Streamline) -> Vec<(Pos2, Vec2)> {
    let mut out = Vec::new();
    let mut travelled = 0.0;
    let mut next = ARROW_EVERY_PX * 0.5;
    for w in line.pts.windows(2) {
        let d = w[1] - w[0];
        let len = d.length();
        if len <= 0.0 {
            continue;
        }
        while travelled + len >= next {
            let t = (next - travelled) / len;
            out.push((w[0] + d * t, d / len));
            next += ARROW_EVERY_PX;
        }
        travelled += len;
    }
    if out.is_empty() {
        let mid = line.pts.len() / 2;
        if let (Some(a), Some(b)) = (line.pts.get(mid.saturating_sub(1)), line.pts.get(mid)) {
            let d = *b - *a;
            if d.length() > 0.0 {
                out.push((*b, d.normalized()));
            }
        }
    }
    out
}

/// Draw `lines` (pane pixels from `origin`) with a dark halo, each stretch coloured by its speed,
/// and their arrowheads.
pub fn paint(painter: &egui::Painter, origin: Pos2, lines: &[Streamline], alpha: f32) {
    let halo = egui::Stroke::new(3.0, Color32::from_black_alpha((120.0 * alpha) as u8));
    // Colour changes along a line in stretches of this many steps: smooth enough for a ramp,
    // few enough shapes to stay cheap.
    const RUN: usize = 6;
    for l in lines {
        let pts: Vec<Pos2> = l.pts.iter().map(|p| origin + p.to_vec2()).collect();
        painter.add(egui::Shape::line(pts.clone(), halo));
        let mut i = 0;
        while i + 1 < pts.len() {
            let end = (i + RUN).min(pts.len() - 1);
            let kt = l.kt[i..=end].iter().sum::<f32>() / (end - i + 1) as f32;
            let col = crate::wind_draw::barb_color(kt).gamma_multiply(alpha);
            painter.add(egui::Shape::line(
                pts[i..=end].to_vec(),
                egui::Stroke::new(1.4, col),
            ));
            i = end;
        }
        for (at, dir) in arrowheads(l) {
            let at = origin + at.to_vec2();
            let back = -dir * 7.0;
            let side = Vec2::new(-dir.y, dir.x) * 3.5;
            let tri = vec![at + dir * 2.0, at + back + side, at + back - side];
            let kt = l.kt[l.kt.len() / 2];
            let col = crate::wind_draw::barb_color(kt).gamma_multiply(alpha);
            painter.add(egui::Shape::convex_polygon(tri, col, halo_thin(alpha)));
        }
    }
}

fn halo_thin(alpha: f32) -> egui::Stroke {
    egui::Stroke::new(1.0, Color32::from_black_alpha((140.0 * alpha) as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> Camera {
        Camera::at_lonlat(-97.5, 35.3, 7.0)
    }

    #[test]
    fn a_uniform_westerly_gives_parallel_lines_flowing_east() {
        let vp = (640.0, 480.0);
        let lines = streamlines(|_, _| Some((10.0, 0.0)), &cam(), vp, 32.0);
        assert!(lines.len() >= 10, "{}", lines.len());
        for l in &lines {
            let (a, b) = (l.pts[0], *l.pts.last().unwrap());
            assert!(b.x > a.x, "downstream is east");
            assert!((b.y - a.y).abs() < 2.0, "a westerly runs along a row");
            assert!(l.kt.iter().all(|k| (k - 19.44).abs() < 0.1));
        }
        // Even spacing: neighbouring rows no closer than half the spacing anywhere.
        let mut ys: Vec<f32> = lines.iter().map(|l| l.pts[0].y).collect();
        ys.sort_by(f32::total_cmp);
        for w in ys.windows(2) {
            assert!(w[1] - w[0] >= 16.0 - 0.5, "{ys:?}");
        }
        // Each crosses most of the pane.
        assert!(lines
            .iter()
            .any(|l| l.pts.last().unwrap().x - l.pts[0].x > 500.0));
        // Arrowheads point east.
        for (_, d) in lines.iter().flat_map(arrowheads) {
            assert!(d.x > 0.99);
        }
    }

    #[test]
    fn the_cache_keeps_lines_on_the_ground_and_retraces_a_new_wind_at_once() {
        let mut cache = StreamCache::default();
        let vp = (400.0, 300.0);
        let mut c = cam();
        let wind = |lon: f64, lat: f64| {
            Some((
                (lat * 3.0).sin() as f32 * 9.0 + 4.0,
                (lon * 2.0).cos() as f32 * 7.0,
            ))
        };
        let (first, stale) = cache.get(&c, vp, (1, 0), || streamlines(wind, &c, vp, 32.0));
        assert!(!stale && !first.is_empty());
        // Pan 50 px right at once: no re-trace yet, the same lines moved 50 px left on screen.
        let w = c.screen_to_world((250.0, 150.0), vp);
        c.center = w;
        let (moved, stale) = cache.get(&c, vp, (1, 0), || panic!("throttled"));
        assert!(stale);
        let (a, b) = (first[0].pts[0], moved[0].pts[0]);
        assert!(
            (a.x - b.x - 50.0).abs() < 0.01 && (a.y - b.y).abs() < 0.01,
            "{a:?} {b:?}"
        );
        // A new wind traces at once, camera moving or not.
        let mut traced = false;
        let (_, stale) = cache.get(&c, vp, (2, 0), || {
            traced = true;
            Vec::new()
        });
        assert!(traced && !stale);
    }

    #[test]
    fn a_southerly_flows_up_the_screen_and_calm_or_missing_air_draws_nothing() {
        let vp = (400.0, 400.0);
        let lines = streamlines(|_, _| Some((0.0, 12.0)), &cam(), vp, 32.0);
        assert!(!lines.is_empty());
        for l in &lines {
            assert!(
                l.pts.last().unwrap().y < l.pts[0].y,
                "north is up the screen"
            );
        }
        assert!(streamlines(|_, _| Some((0.1, 0.1)), &cam(), vp, 32.0).is_empty());
        assert!(streamlines(|_, _| None, &cam(), vp, 32.0).is_empty());
        assert!(streamlines(|_, _| Some((f32::NAN, 3.0)), &cam(), vp, 32.0).is_empty());
    }

    #[test]
    fn a_vortex_closes_its_rings_rather_than_spiralling_over_itself() {
        // Solid-body rotation about the pane's centre: every streamline is a circle.
        let c = cam();
        let vp = (480.0, 480.0);
        let centre = world_to_lonlat(c.center.0, c.center.1);
        let lines = streamlines(
            |lon, lat| {
                let (dx, dy) = (lon - centre.0, lat - centre.1);
                Some(((-dy * 20.0) as f32, (dx * 20.0) as f32))
            },
            &c,
            vp,
            32.0,
        );
        assert!(!lines.is_empty());
        // No point of any line lies closer than half the spacing to another line's point, nor
        // does a line run on round on top of itself.
        let mut all: Vec<(Pos2, usize, usize)> = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            for (k, p) in l.pts.iter().enumerate() {
                all.push((*p, i, k));
            }
        }
        for (i, &(p, li, ki)) in all.iter().enumerate() {
            for &(q, lj, kj) in &all[i + 1..] {
                let apart = if li == lj { ki.abs_diff(kj) > 30 } else { true };
                if apart {
                    assert!(
                        p.distance(q) > 16.0 - STEP_PX,
                        "{p:?} {q:?} lines {li} {lj}"
                    );
                }
            }
        }
        // The rings away from the calm centre come round to meet themselves.
        assert!(lines
            .iter()
            .any(|l| l.pts[0].distance(*l.pts.last().unwrap()) < 32.0
                && l.pts.len() as f32 * STEP_PX > 200.0));
    }
}
