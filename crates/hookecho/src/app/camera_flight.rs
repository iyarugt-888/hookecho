//! Fly-to easing (ROADMAP_2 §4.4): a camera that jumps — a search result, a shared link, "Center"
//! on a storm, a new radar site, an alert's "zoom to" — travels there instead, easing in and out
//! and, on a long hop, rising partway so the way there is visible, then settles on the target.
//!
//! Generic rather than per call site: each pane remembers the camera it last drew with, and a
//! change bigger than a pan or zoom step (half a view away, or 1.5 zoom levels) that the user's own
//! drag, pinch or wheel did not make becomes a flight from there to the new camera. So every
//! existing jump flies, and so will the next one anyone adds. What the user moves by hand never
//! animates, a flight gives way the moment they touch the map, and reduced motion keeps every jump
//! instant. A pane following a storm moves a little each scan, below the threshold, so it stays
//! put rather than drifting: the roadmap's rule that camera motion must not change what an analyst
//! reads while scrubbing.

use super::HookEchoApp;
use crate::render::mercator::Camera;

/// A flight in progress.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Flight {
    from: Camera,
    to: Camera,
    /// egui time it began, and how long it takes, in seconds.
    started: f64,
    duration: f64,
    /// The camera this flight last set, so a move by anything else cancels it.
    last: Camera,
}

fn same(a: &Camera, b: &Camera) -> bool {
    a.center == b.center && a.zoom == b.zoom && a.pitch == b.pitch && a.bearing == b.bearing
}

/// How many view widths apart two cameras' centres are, at the nearer zoom.
fn views_apart(a: &Camera, b: &Camera, view_px: f32) -> f64 {
    let dx = a.center.0 - b.center.0;
    let dy = a.center.1 - b.center.1;
    let world = (dx * dx + dy * dy).sqrt();
    let span = view_px.max(1.0) as f64 * a.world_per_pixel().min(b.world_per_pixel());
    world / span
}

/// Whether `from` to `to` is a jump rather than a step.
pub(crate) fn is_jump(from: &Camera, to: &Camera, view_px: f32) -> bool {
    views_apart(from, to, view_px) > 0.5 || (to.zoom - from.zoom).abs() >= 1.5
}

/// Longer for a longer hop, but never long enough to wait on.
pub(crate) fn duration(from: &Camera, to: &Camera, view_px: f32) -> f64 {
    let hop = views_apart(from, to, view_px) + (to.zoom - from.zoom).abs();
    (0.45 + 0.2 * (1.0 + hop).ln()).clamp(0.45, 1.2)
}

fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// The camera `t` (0..1) of the way along a flight.
pub(crate) fn along(from: &Camera, to: &Camera, t: f64, view_px: f32) -> Camera {
    let e = ease(t);
    let lerp = |a: f64, b: f64| a + (b - a) * e;
    // Rise on a long hop, most at the middle, back down by the end: far enough to have both ends
    // in view at the top (about log2 of the distance in view widths, plus one).
    let far = views_apart(from, to, view_px);
    let rise = if far > 0.5 {
        (far.log2() + 1.0).clamp(0.0, 4.0)
    } else {
        0.0
    };
    let zoom = lerp(from.zoom, to.zoom) - rise * (std::f64::consts::PI * t.clamp(0.0, 1.0)).sin();
    // Bearing the short way round.
    let mut db = to.bearing - from.bearing;
    if db > 180.0 {
        db -= 360.0;
    } else if db < -180.0 {
        db += 360.0;
    }
    Camera {
        center: (
            lerp(from.center.0, to.center.0),
            lerp(from.center.1, to.center.1),
        ),
        zoom,
        pitch: from.pitch + (to.pitch - from.pitch) * e as f32,
        bearing: from.bearing + db * e as f32,
    }
}

impl HookEchoApp {
    /// [`Self::step_camera_flight`] for every pane, by its rect.
    pub(crate) fn step_camera_flights(&mut self, rects: &[egui::Rect], ctx: &egui::Context) {
        for (i, r) in rects.iter().enumerate() {
            self.step_camera_flight(i, r.width(), ctx);
        }
    }

    /// Once a frame, per pane, before its camera is drawn: start a flight for a jump, advance one
    /// in progress, or cancel it for the user's own hand. `view_px` is the pane's width.
    pub(crate) fn step_camera_flight(&mut self, idx: usize, view_px: f32, ctx: &egui::Context) {
        let (now, touching) = ctx.input(|i| {
            (
                i.time,
                i.pointer.is_decidedly_dragging()
                    || i.zoom_delta() != 1.0
                    || i.smooth_scroll_delta != egui::Vec2::ZERO
                    || i.multi_touch().is_some(),
            )
        });
        let reduced = crate::ui::motion::reduced();
        let v = &mut self.views[idx];
        let current = v.camera;
        if let Some(f) = v.flight {
            if touching || reduced || !same(&current, &f.last) {
                // The user took over, or something else moved the camera: it is theirs now.
                v.flight = None;
            } else {
                let t = (now - f.started) / f.duration;
                if t >= 1.0 {
                    v.camera = f.to;
                    v.flight = None;
                } else {
                    let cam = along(&f.from, &f.to, t, view_px);
                    v.camera = cam;
                    v.flight = Some(Flight { last: cam, ..f });
                    ctx.request_repaint();
                }
                v.shown_camera = Some(v.camera);
                return;
            }
        }
        if let Some(shown) = v.shown_camera {
            if !touching && !reduced && is_jump(&shown, &current, view_px) {
                let f = Flight {
                    from: shown,
                    to: current,
                    started: now,
                    duration: duration(&shown, &current, view_px),
                    last: shown,
                };
                v.camera = shown;
                v.flight = Some(f);
                ctx.request_repaint();
            }
        }
        v.shown_camera = Some(v.camera);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam(lon: f64, lat: f64, zoom: f64) -> Camera {
        Camera::at_lonlat(lon, lat, zoom)
    }

    #[test]
    fn a_pan_or_a_zoom_step_is_not_a_jump_but_a_new_place_is() {
        let a = cam(-97.5, 35.3, 8.0);
        assert!(!is_jump(&a, &cam(-97.45, 35.3, 8.0), 1000.0), "a small pan");
        assert!(
            !is_jump(&a, &cam(-97.5, 35.3, 9.0), 1000.0),
            "one zoom step"
        );
        assert!(
            is_jump(&a, &cam(-88.6, 36.7, 8.0), 1000.0),
            "Moore to Mayfield"
        );
        assert!(
            is_jump(&a, &cam(-97.5, 35.3, 11.0), 1000.0),
            "three zoom levels"
        );
    }

    #[test]
    fn a_flight_starts_and_ends_exactly_and_rises_on_a_long_hop() {
        let (a, b) = (cam(-97.5, 35.3, 8.0), cam(-88.6, 36.7, 8.0));
        let start = along(&a, &b, 0.0, 1000.0);
        let end = along(&a, &b, 1.0, 1000.0);
        assert!(same(&start, &a));
        assert!((end.center.0 - b.center.0).abs() < 1e-12 && (end.zoom - b.zoom).abs() < 1e-9);
        let mid = along(&a, &b, 0.5, 1000.0);
        assert!(mid.zoom < 8.0 - 1.0, "rises mid-flight: {}", mid.zoom);
        // A short hop does not rise.
        let c = cam(-97.4, 35.3, 8.0);
        assert!((along(&a, &c, 0.5, 1000.0).zoom - 8.0).abs() < 1e-9);
    }

    #[test]
    fn bearing_turns_the_short_way_and_longer_hops_take_longer() {
        let mut a = cam(-97.5, 35.3, 8.0);
        let mut b = a;
        a.bearing = 170.0;
        b.bearing = -170.0;
        let mid = along(&a, &b, 0.5, 1000.0);
        assert!(
            mid.bearing > 170.0 || mid.bearing < -170.0,
            "{}",
            mid.bearing
        );
        let near = duration(&cam(-97.5, 35.3, 8.0), &cam(-97.2, 35.3, 8.0), 1000.0);
        let far = duration(&cam(-97.5, 35.3, 8.0), &cam(-80.0, 40.0, 8.0), 1000.0);
        assert!(near < far && far <= 1.2 && near >= 0.45, "{near} {far}");
    }
}
