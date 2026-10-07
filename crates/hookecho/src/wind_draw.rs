//! Animated wind particles: the "streaming" wind map, driven by our own HRRR grids.
//!
//! Particles are advected on the CPU and emitted as one `egui::Mesh` per frame, the same technique
//! [`crate::fronts_draw`] uses for its pips. That is not a fallback for a compute shader — it is
//! the only portable option here. When eframe picks a GL adapter it builds the device with
//! `wgpu::Limits::downlevel_webgl2_defaults()`, which zeroes `max_storage_buffers_per_shader_stage`
//! and the compute workgroup sizes outright, so compute is forbidden by the device regardless of
//! what the driver supports. The app has no compute and no storage buffers today; this layer was
//! not going to be the first thing constraining backend choice.
//!
//! The budget makes it a non-issue anyway. Readable density is about one trail per 40x40 px —
//! ~1,500 particles on a phone, ~900 in a desktop pane, not 100,000. At the 3,000 ceiling with a
//! 12-point trail that is ~130k vertices a frame, which `egui-wgpu` binds as `Uint32` indices in
//! a single draw call.
//!
//! `// ponytail:` the ceiling is the mesh rebuild, and the upgrade path if it ever bites is NOT
//! compute — it is the webgl-wind trick (advect positions in a fragment shader, ping-ponging two
//! textures), which runs on GLES 3.0 with no storage buffers at all.

use crate::render::field_ramps::{bake_ramp_lut, FieldScale, WIND};
use crate::render::mercator::{world_to_lonlat, Camera};
use egui::{Color32, Mesh, Pos2, Vec2};
use wxdata::hrrr::WindLevel;
use wxdata::mrms::MrmsField;

/// Positions per trail. What the eye reads is the streak's length in *pixels*, and at the rate
/// below a 10 m/s wind draws roughly 12 of these across ~30 px — about what Windy shows, which
/// gets there with 30-60 much shorter steps.
const TRAIL: usize = 12;

/// **The calibration knob**: on-screen pixels per second, per m/s of wind. A 10 m/s wind moves
/// 160 px/s, drawing a ~35 px streak; a 25 m/s jet core hits the clamp below.
///
/// This is deliberately calibrated in SCREEN space rather than as a model-time multiplier, and
/// that was a correction made against a real screen. Model time cannot work: on-screen speed
/// would scale with zoom, so any multiplier fast enough to be visible across CONUS (a 10 m/s wind
/// at z=5 moves 0.09 px per frame at 900x, i.e. a half-pixel trail) pins every particle against
/// the step clamp two zoom levels later. Fixed pixel speed is also what every wind map does, and
/// the legend still carries the real knots.
///
/// Direction is unaffected by the change: web mercator's scale factor is isotropic, so the
/// `cos(lat)` that converts m/s to world units cancels out of the normalised heading and only
/// ever set the magnitude — which is now set here instead.
pub(crate) const PX_PER_SEC_PER_MS: f64 = 16.0;

/// Longest single step, in pixels, so a hitch or a jet core cannot fling a particle across the
/// pane. At low zoom this is several grid cells, which makes the trajectory coarse where the
/// whole country is on screen and nobody is tracing a parcel; at the zooms where the flow is
/// being read at all, a step is a small fraction of a cell and forward Euler is plenty.
pub(crate) const MAX_STEP_PX: f64 = 20.0;

/// Particles per pixel of pane area, and the range to clamp the result into. Deriving the count
/// from area rather than fixing it means a four-pane layout costs about what one pane does.
const PX_PER_PARTICLE: f32 = 1200.0;
const COUNT_RANGE: (f32, f32) = (400.0, 3000.0);

/// A matched pair of wind component grids plus the run they came from.
pub struct WindField {
    pub u: MrmsField,
    pub v: MrmsField,
    pub level: WindLevel,
    pub run: chrono::DateTime<chrono::Utc>,
    pub fcst_hour: u8,
}

impl WindField {
    pub fn valid(&self) -> chrono::DateTime<chrono::Utc> {
        self.run + chrono::Duration::hours(self.fcst_hour as i64)
    }

    /// Eastward/northward components (m/s) at a point, or `None` off the HRRR domain. Both
    /// components must resolve — half a vector points somewhere the wind isn't going.
    pub fn sample(&self, lon: f64, lat: f64) -> Option<(f32, f32)> {
        Some((
            self.u.sample_bilinear(lon, lat)?,
            self.v.sample_bilinear(lon, lat)?,
        ))
    }
}

/// Spacing of wind barbs on screen, px.
pub const BARB_SPACING_PX: f32 = 56.0;

/// One barb to draw: where (pane pixels), the unit screen vector from there toward where the wind
/// comes from, and the speed in knots.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Barb {
    pub at: Pos2,
    pub toward_source: Vec2,
    pub kt: f32,
}

/// Barbs on a screen lattice `spacing` px apart over a `vp` pane: each samples `field` at its
/// point and points at the wind's source as the camera projects it (so a rotated or pitched map
/// still reads true). Points off the grid, or with either component missing, get none.
pub fn barbs(field: &WindField, cam: &Camera, vp: (f32, f32), spacing: f32) -> Vec<Barb> {
    barbs_uv(&field.u, &field.v, cam, vp, spacing)
}

/// [`barbs`] from any east/north component grids (m/s), such as a wind picked in the model
/// field browser.
pub fn barbs_uv(
    u: &MrmsField,
    v: &MrmsField,
    cam: &Camera,
    vp: (f32, f32),
    spacing: f32,
) -> Vec<Barb> {
    let spacing = spacing.max(8.0);
    let mut out = Vec::new();
    let mut y = spacing / 2.0;
    while y < vp.1 {
        let mut x = spacing / 2.0;
        while x < vp.0 {
            let w = cam.screen_to_world((x, y), vp);
            let (lon, lat) = world_to_lonlat(w.0, w.1);
            let sample = u.sample_bilinear(lon, lat).zip(v.sample_bilinear(lon, lat));
            if let Some((u, v)) = sample.filter(|(u, v)| u.is_finite() && v.is_finite()) {
                // Meteorological "from" bearing: the wind blows from there toward (u, v).
                let from = f64::from(-u).atan2(f64::from(-v)).to_degrees();
                let src = crate::geo::destination_point([lon, lat], from, 10.0);
                let s = cam
                    .world_to_screen(crate::render::mercator::lonlat_to_world(src[0], src[1]), vp);
                let d = Vec2::new(s.0 - x, s.1 - y);
                if d.length() > 1e-3 {
                    out.push(Barb {
                        at: Pos2::new(x, y),
                        toward_source: d.normalized(),
                        kt: u.hypot(v) * 1.943_844,
                    });
                }
            }
            x += spacing;
        }
        y += spacing;
    }
    out
}

/// Draw `barbs` (pane pixels from `origin`) with a dark halo, coloured by speed; calm points
/// get a circle.
pub fn paint_barbs(painter: &egui::Painter, origin: Pos2, barbs: &[Barb], alpha: f32) {
    for b in barbs {
        let at = Barb {
            at: origin + b.at.to_vec2(),
            ..*b
        };
        let col = barb_color(b.kt).gamma_multiply(alpha);
        let halo = Color32::from_black_alpha((140.0 * alpha) as u8);
        let lines = barb_lines(&at, 20.0);
        for l in &lines {
            painter.line_segment(*l, egui::Stroke::new(3.0, halo));
        }
        for l in &lines {
            painter.line_segment(*l, egui::Stroke::new(1.4, col));
        }
        if b.kt < 2.5 {
            painter.circle_stroke(at.at, 3.0, egui::Stroke::new(1.2, col));
        }
    }
}

/// A barb's line segments in pane pixels, `len` px long: the shaft toward the source and the
/// METAR barb/pennant convention along it ([`wxdata::metar::barb_segments`]).
pub fn barb_lines(b: &Barb, len: f32) -> Vec<[Pos2; 2]> {
    let up = b.toward_source;
    let right = Vec2::new(-up.y, up.x);
    let map = |p: [f32; 2]| b.at + (right * p[0] + up * p[1]) * len;
    wxdata::metar::barb_segments(b.kt)
        .into_iter()
        .map(|(a, c)| [map(a), map(c)])
        .collect()
}

/// A barb's colour: the wind ramp, as the particles and legend use.
pub fn barb_color(kt: f32) -> Color32 {
    speed_color(kt / 1.943_844)
}

/// Speed (m/s) → color, through the shared [`WIND`] ramp so the legend and the particles cannot
/// disagree. The LUT is baked once; `FieldRamp::index` already applies the m/s→kt conversion.
fn speed_color(ms: f32) -> Color32 {
    static LUT: std::sync::LazyLock<Vec<u8>> = std::sync::LazyLock::new(|| match &WIND.scale {
        FieldScale::Ramp { stops, .. } => bake_ramp_lut(stops, 255),
        FieldScale::Categorical(_) => vec![255; 256 * 4],
    });
    let i = WIND.index(ms) as usize * 4;
    Color32::from_rgb(LUT[i], LUT[i + 1], LUT[i + 2])
}

struct Particle {
    /// Trail head first. **World space**, not screen space: world coordinates stay valid through
    /// any camera change, so pan and zoom need no reprojection pass and no resize fix-up, and the
    /// trails stay pinned to the ground — which is what makes this read as a map layer.
    trail: [(f64, f64); TRAIL],
    /// How many entries of `trail` are real yet (a fresh particle has one).
    n: usize,
    age: u32,
    /// Randomised per particle, redrawn on every respawn. **This is the whole anti-clumping
    /// mechanism.** With a shared lifetime everything piles into the convergence lines within
    /// about ten seconds and the rest of the map empties; decorrelated deaths keep the field even
    /// without any density-feedback bookkeeping.
    max_age: u32,
    /// Speed at the head, m/s, kept for coloring.
    speed: f32,
}

/// One pane's particles. Each pane has its own camera, so each needs its own set.
pub struct Particles {
    p: Vec<Particle>,
    rng: u64,
    /// Viewport the population was sized for, so a resize can be noticed.
    vp: (f32, f32),
}

impl Default for Particles {
    fn default() -> Self {
        Self {
            p: Vec::new(),
            // `// ponytail:` a four-line xorshift, not the `rand` crate, for jittering particles.
            // Seeded off the clock: nothing here is security-sensitive or needs reproducibility.
            rng: wxdata::clock::SystemTime::now()
                .duration_since(wxdata::clock::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as u64 | 1)
                .unwrap_or(0x2545_F491_4F6C_DD1D),
            vp: (0.0, 0.0),
        }
    }
}

impl Particles {
    fn next_rand(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn unit(&mut self) -> f64 {
        (self.next_rand() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Place a particle at a uniform random point of the pane.
    ///
    /// Seeding in **screen** space rather than geographically is deliberate: uniform in lon/lat
    /// piles particles up at high latitude once mercator stretches the map, which is exactly the
    /// artifact this layer exists to avoid. It also means a fast drag refills the newly exposed
    /// band within one lifetime instead of leaving it bare.
    fn respawn(&mut self, i: usize, cam: &Camera, vp: (f32, f32)) {
        let (rx, ry) = (self.unit(), self.unit());
        let w = cam.screen_to_world((rx as f32 * vp.0, ry as f32 * vp.1), vp);
        let max_age = 30 + (self.next_rand() % 60) as u32;
        let p = &mut self.p[i];
        p.trail[0] = w;
        p.n = 1;
        p.age = 0;
        p.max_age = max_age;
        p.speed = 0.0;
    }

    /// Advect every particle one frame. `dt` is clamped by the caller's contract to a sane frame
    /// time — a hitch or a resume from background must not teleport the whole field.
    pub fn update(&mut self, field: &WindField, cam: &Camera, vp: (f32, f32), dt: f32) {
        let want = ((vp.0 * vp.1 / PX_PER_PARTICLE).clamp(COUNT_RANGE.0, COUNT_RANGE.1)) as usize;
        if self.p.len() != want || self.vp != vp {
            self.vp = vp;
            self.p.resize_with(want, || Particle {
                trail: [(0.0, 0.0); TRAIL],
                n: 0,
                age: 0,
                max_age: 0,
                speed: 0.0,
            });
            for i in 0..want {
                if self.p[i].n == 0 {
                    self.respawn(i, cam, vp);
                }
            }
        }

        let wpp = cam.world_per_pixel();
        let max_step = MAX_STEP_PX * wpp;
        // Off-pane by this much in world units, i.e. ~20 px of slack so a trail can leave cleanly.
        let margin = 20.0 * wpp;
        let (w0, h0) = cam.screen_to_world((0.0, 0.0), vp);
        let (w1, h1) = cam.screen_to_world((vp.0, vp.1), vp);
        let dt = dt as f64;

        for i in 0..self.p.len() {
            let (wx, wy) = self.p[i].trail[0];
            if wx < w0 - margin || wx > w1 + margin || wy < h0 - margin || wy > h1 + margin {
                self.respawn(i, cam, vp);
                continue;
            }
            let (lon, lat) = world_to_lonlat(wx, wy);
            let Some((u, v)) = field.sample(lon, lat) else {
                // Off the HRRR domain — CONUS-only, so this is most of the world.
                self.respawn(i, cam, vp);
                continue;
            };
            let speed = (u * u + v * v).sqrt();
            if speed < 0.2 {
                // Dead calm: the particle would sit still and burn a trail into the map.
                self.respawn(i, cam, vp);
                continue;
            }
            // Heading from the wind vector, magnitude from the screen calibration. y is negated
            // because v is northward while world y grows southward.
            let step = (speed as f64 * PX_PER_SEC_PER_MS * dt * wpp).min(max_step);
            let inv = step / speed as f64;
            let (dx, dy) = (u as f64 * inv, -(v as f64) * inv);

            let p = &mut self.p[i];
            p.trail.copy_within(0..TRAIL - 1, 1);
            p.trail[0] = (wx + dx, wy + dy);
            p.n = (p.n + 1).min(TRAIL);
            p.speed = speed;
            p.age += 1;
            if p.age > p.max_age {
                self.respawn(i, cam, vp);
            }
        }
    }

    /// Build this frame's mesh. `origin` is the pane's top-left; `alpha` scales the whole layer
    /// (the zoom fade and the dimming over reflectivity both ride on it).
    pub fn build_mesh(&self, cam: &Camera, vp: (f32, f32), origin: Pos2, alpha: f32) -> Mesh {
        let mut mesh = Mesh::default();
        if alpha <= 0.01 {
            return mesh;
        }
        let mut pts = [Pos2::ZERO; TRAIL];
        for p in &self.p {
            if p.n < 2 {
                continue;
            }
            for (k, slot) in pts.iter_mut().enumerate().take(p.n) {
                let (sx, sy) = cam.world_to_screen(p.trail[k], vp);
                *slot = origin + Vec2::new(sx, sy);
            }
            let col = speed_color(p.speed);
            for k in 0..p.n - 1 {
                let (a, b) = (pts[k], pts[k + 1]);
                let seg = b - a;
                let len = seg.length();
                if len < 0.01 {
                    continue;
                }
                // Taper and fade from head to tail; the fade IS the motion blur, which is why this
                // still looks right well below 60 fps.
                let t = k as f32 / (TRAIL - 1) as f32;
                let half = (2.0 - 1.3 * t) * 0.5;
                let a8 = (alpha * (1.0 - 0.85 * t) * 255.0) as u8;
                let c = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), a8);
                let n = Vec2::new(seg.y, -seg.x) / len * half;
                let i = mesh.vertices.len() as u32;
                for q in [a - n, a + n, b + n, b - n] {
                    mesh.colored_vertex(q, c);
                }
                mesh.add_triangle(i, i + 1, i + 2);
                mesh.add_triangle(i, i + 2, i + 3);
            }
        }
        mesh
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn barbs_point_where_the_wind_comes_from_on_the_screen() {
        let cam = Camera::at_lonlat(-100.0, 40.0, 6.0);
        let vp = (400.0, 300.0);
        // A 20 m/s westerly: from the west, so toward the source is screen-left.
        let w = super::barbs(&uniform(20.0, 0.0), &cam, vp, 56.0);
        assert!(!w.is_empty());
        for b in &w {
            assert!(b.toward_source.x < -0.99, "{:?}", b.toward_source);
            assert!((b.kt - 38.9).abs() < 0.1, "{}", b.kt);
        }
        // A northerly (blowing south): toward the source is screen-up.
        let n = super::barbs(&uniform(0.0, -10.0), &cam, vp, 56.0);
        assert!(n.iter().all(|b| b.toward_source.y < -0.99), "{:?}", n[0]);
        // Turn the map so east is up (bearing 90): the westerly's source is now screen-down.
        let mut turned = cam;
        turned.bearing = 90.0;
        let t = super::barbs(&uniform(20.0, 0.0), &turned, vp, 56.0);
        assert!(!t.is_empty());
        assert!(
            t.iter().all(|b| b.toward_source.y > 0.99),
            "{:?}",
            t[0].toward_source
        );
        // The glyph: a 39 kt barb is a shaft, three full barbs and a half.
        let lines = super::barb_lines(&w[0], 20.0);
        assert_eq!(lines.len(), 1 + 4);
        let shaft = lines[0];
        assert!(
            (shaft[1].x - shaft[0].x + 20.0).abs() < 0.01,
            "shaft drawn toward the source"
        );
    }

    /// Barbs over a synthetic speed gradient (5 to 75 kt westerly, south to north), written for
    /// review under `target/parity-review/wind-barbs`.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes the wind barb capture for review"]
    fn gpu_wind_barb_snapshot() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/wind-barbs");
        std::fs::create_dir_all(&dir).unwrap();
        let mut field = uniform(0.0, 0.0);
        for j in 0..4 {
            for i in 0..4 {
                // Row 0 is the north edge: fastest there, veering from southwesterly to westerly.
                let kt = 75.0 - 23.0 * j as f32;
                let ms = kt / 1.943_844;
                let (s, c) = (0.35 * j as f32).sin_cos();
                field.u.values[j * 4 + i] = ms * c;
                field.v.values[j * 4 + i] = ms * s;
            }
        }
        let cam = Camera::at_lonlat(-100.0, 40.0, 6.0);
        gpu.save(&dir.join("barbs.png"), 420, 320, |ui| {
            let rect = ui.max_rect();
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(28, 32, 40));
            for b in super::barbs(&field, &cam, (420.0, 320.0), super::BARB_SPACING_PX) {
                for l in super::barb_lines(&b, 20.0) {
                    painter.line_segment(l, egui::Stroke::new(1.4, super::barb_color(b.kt)));
                }
            }
        })
        .unwrap();
    }

    #[test]
    fn no_barb_where_there_is_no_wind_grid() {
        let cam = Camera::at_lonlat(-60.0, 10.0, 6.0);
        assert!(super::barbs(&uniform(20.0, 0.0), &cam, (400.0, 300.0), 56.0).is_empty());
    }
    use super::*;
    use wxdata::mrms::MrmsField;

    /// A uniform field over CONUS: `u` m/s east, `v` m/s north, everywhere.
    fn uniform(u: f32, v: f32) -> WindField {
        let grid = |val: f32| MrmsField {
            values: vec![val; 16],
            nx: 4,
            ny: 4,
            lon_west: -110.0,
            lon_east: -90.0,
            lat_north: 45.0,
            lat_south: 35.0,
            time: chrono::Utc::now(),
        };
        WindField {
            u: grid(u),
            v: grid(v),
            level: WindLevel::Surface,
            run: chrono::Utc::now(),
            fcst_hour: 0,
        }
    }

    /// Per-particle screen-space displacement over one `update`, for particles that didn't respawn.
    fn step_pixels(field: &WindField, cam: &Camera, vp: (f32, f32), dt: f32) -> Vec<(f64, f64)> {
        let mut ps = Particles::default();
        ps.update(field, cam, vp, dt);
        let before: Vec<(f64, f64)> = ps.p.iter().map(|p| p.trail[0]).collect();
        ps.update(field, cam, vp, dt);
        let wpp = cam.world_per_pixel();
        ps.p.iter()
            .zip(&before)
            .filter(|(p, _)| p.age > 0)
            .map(|(p, b)| ((p.trail[0].0 - b.0) / wpp, (p.trail[0].1 - b.1) / wpp))
            .collect()
    }

    #[test]
    fn a_westerly_pushes_particles_east_at_the_calibrated_screen_speed() {
        let field = uniform(10.0, 0.0);
        let cam = Camera::at_lonlat(-100.0, 40.0, 7.0);
        let dt = 0.033f32;
        let moved = step_pixels(&field, &cam, (800.0, 600.0), dt);
        assert!(!moved.is_empty());
        let expect = 10.0 * PX_PER_SEC_PER_MS * dt as f64;
        for (dx, dy) in moved {
            assert!(
                dy.abs() < 1e-6,
                "a westerly must not move north or south: {dy}"
            );
            // Relative: the speed is an f32 widened to f64, so the error scales with the answer.
            assert!(
                (dx - expect).abs() < expect * 1e-5,
                "stepped {dx} px, expected {expect}"
            );
        }
    }

    #[test]
    fn screen_speed_does_not_depend_on_zoom() {
        // The whole reason the rate is calibrated in pixels: the same wind must read the same
        // whether the pane shows the country or a county.
        let field = uniform(10.0, 0.0);
        let vp = (800.0, 600.0);
        let wide = step_pixels(&field, &Camera::at_lonlat(-100.0, 40.0, 5.0), vp, 0.033);
        let close = step_pixels(&field, &Camera::at_lonlat(-100.0, 40.0, 10.0), vp, 0.033);
        assert!(
            (wide[0].0 - close[0].0).abs() < wide[0].0.abs() * 1e-5,
            "{:?} vs {:?}",
            wide[0],
            close[0]
        );
    }

    #[test]
    fn the_step_clamp_caps_pixel_speed() {
        // A jet core and a long frame: the clamp, not the wind, must set the distance.
        let field = uniform(100.0, 0.0);
        let cam = Camera::at_lonlat(-100.0, 40.0, 10.0);
        for (dx, _) in step_pixels(&field, &cam, (800.0, 600.0), 0.1) {
            assert!(
                dx <= MAX_STEP_PX + 1e-6,
                "stepped {dx} px, cap is {MAX_STEP_PX}"
            );
        }
    }

    #[test]
    fn particles_off_the_domain_respawn_instead_of_freezing() {
        // Camera over the Atlantic: HRRR has nothing there, so every sample fails.
        let field = uniform(10.0, 0.0);
        let cam = Camera::at_lonlat(-40.0, 40.0, 7.0);
        let vp = (400.0, 300.0);
        let mut ps = Particles::default();
        ps.update(&field, &cam, vp, 0.02);
        ps.update(&field, &cam, vp, 0.02);
        assert!(
            ps.p.iter().all(|p| p.n == 1),
            "off-domain particles must keep respawning"
        );
        // Nothing to draw is better than a frozen field of stale streaks.
        let mesh = ps.build_mesh(&cam, vp, Pos2::ZERO, 1.0);
        assert!(mesh.is_empty());
    }

    #[test]
    fn respawn_lifetimes_are_decorrelated() {
        let field = uniform(10.0, 0.0);
        let cam = Camera::at_lonlat(-100.0, 40.0, 7.0);
        let mut ps = Particles::default();
        ps.update(&field, &cam, (800.0, 600.0), 0.02);
        let ages: std::collections::HashSet<u32> = ps.p.iter().map(|p| p.max_age).collect();
        // A shared lifetime is the clumping bug; anything near the full 60-value spread is fine.
        assert!(ages.len() > 30, "only {} distinct lifetimes", ages.len());
        assert!(ps.p.iter().all(|p| (30..90).contains(&p.max_age)));
    }

    #[test]
    fn speed_color_tracks_the_ramp() {
        // Calm and gale must not land on the same color, and the ramp is read in knots.
        assert_ne!(speed_color(1.0), speed_color(30.0));
        assert_eq!(speed_color(0.0), speed_color(-0.0));
    }
}
