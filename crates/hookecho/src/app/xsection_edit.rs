//! Direct editing of the cross-section line (ROADMAP_PARITY M3.6 / gap 3): drag its endpoints,
//! slide it by its middle, swing it about its middle by a rotation handle, set its length and
//! bearing exactly, snap it to a radial through the radar or to 15° steps, and carry it into the
//! 3D view's vertical cut. Mouse, touch and pen share one path (egui's primary pointer), with a
//! touch-sized grab radius, and Esc or a cancelled touch puts the line back as it was.
//!
//! The geometry is spherical (`crate::geo`), the same as the 3D plane's ground track, so the line
//! drawn on the map, the section sampled and the 3D cut all name the same ground. Editing never
//! resamples anything itself: the section is rebuilt from the volume's own gates
//! (`wxdata::xsection::build`) whenever the line or the volume changes.
use super::*;

/// Handle grab radius, points: a fingertip needs twice a cursor's.
const GRAB_PT: f32 = 12.0;
const GRAB_PT_TOUCH: f32 = 24.0;
/// The rotation handle sits this far off the line's middle, as a fraction of its length, at a
/// right angle to it — far enough not to sit on the middle handle on a short line.
const ROTATE_ARM: f64 = 0.25;
/// Shift-snapping step for bearings, degrees.
const SNAP_DEG: f64 = 15.0;
/// The 3D cut's slab half-width around the section, km: a band of the volume about the width the
/// section's nearest-gate sampling draws from at mid range.
const CUT_SLAB_KM: f64 = 2.0;

/// The section's ground line, `a` → `b`, both `[lon, lat]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SectionLine {
    pub a: [f64; 2],
    pub b: [f64; 2],
}

/// Which handle a drag holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Grab {
    A,
    B,
    /// The middle: the whole line slides, keeping length and bearing.
    Middle,
    /// The rotation handle: the line swings about its middle.
    Rotate,
}

/// A drag in progress, with the line it started from for a cancel.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SectionDrag {
    pub grab: Grab,
    pub before: SectionLine,
    pub pane: usize,
}

impl SectionLine {
    pub fn length_km(&self) -> f64 {
        crate::geo::great_circle(self.a, self.b).0
    }

    pub fn midpoint(&self) -> [f64; 2] {
        let (len, brg) = crate::geo::great_circle(self.a, self.b);
        crate::geo::destination_point(self.a, brg, len / 2.0)
    }

    /// Bearing of the line at its middle, A toward B, degrees from north.
    pub fn bearing(&self) -> f64 {
        crate::geo::bearing_deg(self.midpoint(), self.b)
    }

    /// The line of `length_km` centred on `mid`, heading `bearing` from A to B.
    pub fn centred(mid: [f64; 2], bearing: f64, length_km: f64) -> Self {
        let half = length_km.max(0.1) / 2.0;
        SectionLine {
            a: crate::geo::destination_point(mid, bearing + 180.0, half),
            b: crate::geo::destination_point(mid, bearing, half),
        }
    }

    pub fn with_bearing(&self, bearing: f64) -> Self {
        Self::centred(self.midpoint(), bearing.rem_euclid(360.0), self.length_km())
    }

    pub fn with_length(&self, length_km: f64) -> Self {
        Self::centred(self.midpoint(), self.bearing(), length_km)
    }

    /// Slid sideways by `km`: positive to the right of A→B.
    pub fn slid(&self, km: f64) -> Self {
        let mid = crate::geo::destination_point(self.midpoint(), self.bearing() + 90.0, km);
        Self::centred(mid, self.bearing(), self.length_km())
    }

    /// Moved so its middle is at `mid`, keeping length and bearing.
    pub fn moved_to(&self, mid: [f64; 2]) -> Self {
        Self::centred(mid, self.bearing(), self.length_km())
    }

    /// Swung about its middle so it runs along the radial through `radar` — the cut a radial
    /// velocity section reads straight — keeping whichever direction is nearer its current one.
    pub fn radial_through(&self, radar: [f64; 2]) -> Self {
        let mid = self.midpoint();
        if crate::geo::great_circle(mid, radar).0 < 0.01 {
            return *self;
        }
        let toward = crate::geo::bearing_deg(mid, radar);
        let away = (toward + 180.0).rem_euclid(360.0);
        let cur = self.bearing();
        let diff = |x: f64| ((x - cur + 540.0).rem_euclid(360.0) - 180.0).abs();
        self.with_bearing(if diff(toward) <= diff(away) {
            toward
        } else {
            away
        })
    }

    /// A ↔ B.
    pub fn reversed(&self) -> Self {
        SectionLine {
            a: self.b,
            b: self.a,
        }
    }

    /// Where the rotation handle sits.
    pub fn rotate_handle(&self) -> [f64; 2] {
        crate::geo::destination_point(
            self.midpoint(),
            self.bearing() + 90.0,
            self.length_km() * ROTATE_ARM,
        )
    }

    /// The line after dragging `grab` to `to` (`[lon, lat]`); `snap` rounds a swing to 15°.
    pub fn dragged(&self, grab: Grab, to: [f64; 2], snap: bool) -> Self {
        let snapped = |b: f64| {
            if snap {
                (b / SNAP_DEG).round() * SNAP_DEG
            } else {
                b
            }
        };
        match grab {
            Grab::A => {
                if snap {
                    let (len, brg) = crate::geo::great_circle(self.b, to);
                    SectionLine {
                        a: crate::geo::destination_point(self.b, snapped(brg), len),
                        b: self.b,
                    }
                } else {
                    SectionLine { a: to, b: self.b }
                }
            }
            Grab::B => {
                if snap {
                    let (len, brg) = crate::geo::great_circle(self.a, to);
                    SectionLine {
                        a: self.a,
                        b: crate::geo::destination_point(self.a, snapped(brg), len),
                    }
                } else {
                    SectionLine { a: self.a, b: to }
                }
            }
            Grab::Middle => self.moved_to(to),
            Grab::Rotate => {
                let mid = self.midpoint();
                if crate::geo::great_circle(mid, to).0 < 1e-3 {
                    return *self;
                }
                self.with_bearing(snapped(crate::geo::bearing_deg(mid, to) - 90.0))
            }
        }
    }

    /// The 3D view's vertical cut along this line, for a box centred on `radar` with half-width
    /// `half_km`: the plane's normal is the line's right-hand side, its offset the line's signed
    /// distance from the radar along it, and a thin slab keeps a band around the section.
    pub fn plane(&self, radar: [f64; 2], half_km: f32) -> crate::render3d::VerticalPlane {
        let normal = (self.bearing() + 90.0).rem_euclid(360.0);
        let (d, brg) = crate::geo::great_circle(radar, self.midpoint());
        let along_normal = d * (brg - normal).to_radians().cos();
        let half = f64::from(half_km.max(1e-3));
        crate::render3d::VerticalPlane {
            bearing_deg: normal as f32,
            offset: (along_normal / half).clamp(-1.0, 1.0) as f32,
            thickness: Some((CUT_SLAB_KM / half) as f32),
        }
    }
}

/// `line` after the window's edits: swing, length, slide, then the one-shot snaps.
pub(crate) fn edited(
    line: SectionLine,
    before: &crate::ui::xsection_window::XsControls,
    ctl: &crate::ui::xsection_window::XsControls,
    site: Option<&str>,
) -> SectionLine {
    let mut next = line;
    if (ctl.bearing_deg - before.bearing_deg).abs() > 1e-9 {
        next = next.with_bearing(ctl.bearing_deg);
    }
    if (ctl.length_km - before.length_km).abs() > 1e-9 {
        next = next.with_length(ctl.length_km);
    }
    if ctl.slide_km != 0.0 {
        next = next.slid(ctl.slide_km);
    }
    if ctl.radial {
        if let Some(site) = site.and_then(wxdata::sites::site_by_id) {
            next = next.radial_through([f64::from(site.longitude), f64::from(site.latitude)]);
        }
    }
    if ctl.swap {
        next = next.reversed();
    }
    next
}

/// The handle under `p` (screen points), nearest first, within `radius`.
pub(crate) fn grab_at(
    line: &SectionLine,
    p: egui::Pos2,
    radius: f32,
    to_px: impl Fn([f64; 2]) -> egui::Pos2,
) -> Option<Grab> {
    [
        (Grab::A, line.a),
        (Grab::B, line.b),
        (Grab::Rotate, line.rotate_handle()),
        (Grab::Middle, line.midpoint()),
    ]
    .into_iter()
    .map(|(g, ll)| (g, to_px(ll).distance(p)))
    .filter(|(_, d)| *d <= radius)
    .min_by(|x, y| x.1.total_cmp(&y.1))
    .map(|(g, _)| g)
}

impl HookEchoApp {
    pub(crate) fn xsection_line(&self) -> Option<SectionLine> {
        match self.xsection_pts.as_slice() {
            [a, b] => Some(SectionLine { a: *a, b: *b }),
            _ => None,
        }
    }

    /// Replace the line and rebuild the section (and the 3D cut, when linked) from pane `idx`.
    pub(crate) fn set_xsection_line(&mut self, idx: usize, line: SectionLine, ctx: &egui::Context) {
        self.xsection_pts = vec![line.a, line.b];
        self.build_xsection(idx, ctx);
        self.sync_xsection_3d(idx);
    }

    /// Carry the section into pane `idx`'s 3D vertical cut, when linked and a smooth volume box is
    /// resident to place it in.
    pub(crate) fn sync_xsection_3d(&mut self, idx: usize) {
        if !self.xsection_cut_3d {
            return;
        }
        let Some(line) = self.xsection_line() else {
            return;
        };
        let Some((_, _, half_km, _, _)) = self.smooth_vol_dims.get(idx).copied().flatten() else {
            return;
        };
        // Relative to the box's centre, which a region of interest moves off the radar.
        let Some(center) = self.smooth_box_center(idx) else {
            return;
        };
        self.views[idx].map_3d.plane = Some(line.plane(center, half_km));
    }

    /// One line about what the section samples: site, tilts, the span their radials were scanned
    /// over, and how much of the panel is inside real beam coverage.
    pub(crate) fn xsection_info(&self, xs: &wxdata::xsection::CrossSection) -> String {
        let Some(src) = self.xsection_source.as_ref() else {
            return String::new();
        };
        let tz = self.active_tz();
        let site = self.views[src.pane].site.clone().unwrap_or_default();
        let span = src.span.map_or_else(
            || "scan times unknown".into(),
            |(s, e)| {
                format!(
                    "scanned {}–{}",
                    crate::timefmt::fmt_clock(s, tz, true),
                    crate::timefmt::fmt_clock(e, tz, true)
                )
            },
        );
        format!(
            "{site} {} · {} tilts {span} · {:.0}% of the panel inside beam coverage",
            src.moment.short_name(),
            src.tilts,
            crate::ui::xsection_window::covered_fraction(xs) * 100.0
        )
    }

    /// Apply the window's exact edits to the line, in the order a person would expect: swing,
    /// length, slide, then the one-shot snaps.
    pub(crate) fn apply_xsection_controls(
        &mut self,
        pane: usize,
        line: SectionLine,
        before: &crate::ui::xsection_window::XsControls,
        ctl: &crate::ui::xsection_window::XsControls,
        ctx: &egui::Context,
    ) {
        self.xsection_cut_3d = ctl.cut_3d;
        let next = edited(line, before, ctl, self.views[pane].site.as_deref());
        if next != line {
            self.set_xsection_line(pane, next, ctx);
        } else if ctl.cut_3d != before.cut_3d {
            self.sync_xsection_3d(pane);
        }
    }

    /// Rebuild the section when the pane it was sampled from moved to another volume or revision
    /// (a live tilt landed, the playhead moved), so the panel never shows one scan under another's
    /// time.
    pub(crate) fn refresh_xsection(&mut self, ctx: &egui::Context) {
        let Some(src) = self.xsection_source.as_ref() else {
            return;
        };
        if self.xsection.is_none() || self.xsection_drag.is_some() {
            return;
        }
        let pane = src.pane;
        let current = self.views.get(pane).and_then(|v| v.volume.as_ref());
        if current.is_some_and(|v| v.name != src.volume || v.revision() != src.revision) {
            self.build_xsection(pane, ctx);
        }
    }

    /// Take a drag that starts on a section handle, with the cross-section tool armed. Returns
    /// whether it owns the pointer (so the map does not pan under it).
    pub(crate) fn xsection_input(
        &mut self,
        idx: usize,
        prect: egui::Rect,
        response: &egui::Response,
        ui: &egui::Ui,
        allow_pointer: bool,
        ctx: &egui::Context,
    ) -> bool {
        let cancelled = ui.input(|i| {
            !i.focused
                || i.key_pressed(egui::Key::Escape)
                || i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Touch {
                            phase: egui::TouchPhase::Cancel,
                            ..
                        }
                    )
                })
        });
        if self.tool != MapTool::CrossSection || cancelled || !allow_pointer {
            if let Some(drag) = self.xsection_drag.take() {
                // Put it back as it was, and the section with it.
                self.set_xsection_line(drag.pane, drag.before, ctx);
            }
            return false;
        }
        if self.xsection_drag.is_some_and(|d| d.pane != idx) {
            return false;
        }
        let Some(line) = self.xsection_line() else {
            return false;
        };
        let vp = (prect.width(), prect.height());
        let cam = self.views[idx].camera;
        let to_px = |ll: [f64; 2]| {
            let w = crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
            let (x, y) = cam.world_to_screen(w, vp);
            egui::pos2(prect.left() + x, prect.top() + y)
        };
        let to_ll = |p: egui::Pos2| {
            let w = cam.screen_to_world((p.x - prect.left(), p.y - prect.top()), vp);
            let (lon, lat) = crate::render::mercator::world_to_lonlat(w.0, w.1);
            [lon, lat]
        };
        let touch = ui.input(|i| i.any_touches() || i.has_touch_screen());
        let radius = if touch { GRAB_PT_TOUCH } else { GRAB_PT };
        if response.drag_started_by(egui::PointerButton::Primary) {
            let Some(p) = ui.input(|i| i.pointer.press_origin()) else {
                return false;
            };
            let Some(grab) = grab_at(&line, p, radius, to_px) else {
                return false;
            };
            self.active = idx;
            self.xsection_drag = Some(SectionDrag {
                grab,
                before: line,
                pane: idx,
            });
        }
        let Some(drag) = self.xsection_drag else {
            return false;
        };
        if let Some(p) = response.interact_pointer_pos() {
            let snap = ui.input(|i| i.modifiers.shift);
            let next = line.dragged(drag.grab, to_ll(p), snap);
            if next != line {
                self.set_xsection_line(idx, next, ctx);
            }
        }
        if response.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
            self.xsection_drag = None;
        }
        true
    }

    /// The edit handles over the line while the tool is armed: endpoints, the middle and the
    /// rotation handle on its arm.
    pub(crate) fn paint_xsection_handles(
        &self,
        painter: &egui::Painter,
        screen: impl Fn([f64; 2]) -> egui::Pos2,
    ) {
        if self.tool != MapTool::CrossSection {
            return;
        }
        let Some(line) = self.xsection_line() else {
            return;
        };
        let col = egui::Color32::from_rgb(90, 220, 255);
        let held = self.xsection_drag.map(|d| d.grab);
        let ring = |at: egui::Pos2, g: Grab| {
            let r = if held == Some(g) { 8.0 } else { 6.0 };
            painter.circle_filled(at, r, egui::Color32::from_black_alpha(160));
            painter.circle_stroke(at, r, egui::Stroke::new(2.0, col));
        };
        let mid = screen(line.midpoint());
        let rot = screen(line.rotate_handle());
        painter.line_segment([mid, rot], egui::Stroke::new(1.0, col.gamma_multiply(0.7)));
        ring(screen(line.a), Grab::A);
        ring(screen(line.b), Grab::B);
        ring(mid, Grab::Middle);
        ring(rot, Grab::Rotate);
        painter.text(
            rot + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "\u{21bb}",
            egui::FontId::proportional(13.0),
            col,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KTLX: [f64; 2] = [-97.278, 35.333];

    fn line() -> SectionLine {
        SectionLine::centred([-97.0, 35.4], 70.0, 80.0)
    }

    fn close(a: [f64; 2], b: [f64; 2], km: f64) -> bool {
        crate::geo::great_circle(a, b).0 < km
    }

    #[test]
    fn edits_keep_what_they_say_they_keep() {
        let l = line();
        assert!((l.length_km() - 80.0).abs() < 0.01);
        assert!((l.bearing() - 70.0).abs() < 0.01);
        let r = l.with_bearing(160.0);
        assert!(
            close(r.midpoint(), l.midpoint(), 0.01),
            "swings about its middle"
        );
        assert!((r.length_km() - 80.0).abs() < 0.01);
        let s = l.slid(10.0);
        assert!((crate::geo::great_circle(l.midpoint(), s.midpoint()).0 - 10.0).abs() < 0.01);
        assert!((s.bearing() - 70.0).abs() < 0.05 && (s.length_km() - 80.0).abs() < 0.01);
        // To the right of A→B (bearing 70) is 160.
        let side = crate::geo::bearing_deg(l.midpoint(), s.midpoint());
        assert!((side - 160.0).abs() < 0.1, "{side}");
        let n = l.with_length(30.0);
        assert!((n.length_km() - 30.0).abs() < 0.01 && close(n.midpoint(), l.midpoint(), 0.01));
        assert_eq!(l.reversed().reversed(), l);
    }

    #[test]
    fn dragging_each_handle_moves_only_what_it_holds() {
        let l = line();
        let to = [-96.8, 35.6];
        let a = l.dragged(Grab::A, to, false);
        assert_eq!((a.a, a.b), (to, l.b));
        let b = l.dragged(Grab::B, to, false);
        assert_eq!((b.a, b.b), (l.a, to));
        let m = l.dragged(Grab::Middle, to, false);
        assert!(close(m.midpoint(), to, 0.01));
        assert!((m.length_km() - l.length_km()).abs() < 0.01);
        // The rotation handle points the line's right-hand side at the pointer.
        let r = l.dragged(
            Grab::Rotate,
            crate::geo::destination_point(l.midpoint(), 0.0, 20.0),
            false,
        );
        assert!((r.bearing() - 270.0).abs() < 0.05, "{}", r.bearing());
        assert!(close(r.midpoint(), l.midpoint(), 0.01));
        // Shift snaps a swing and an endpoint's bearing to 15°.
        let s = l.dragged(
            Grab::Rotate,
            crate::geo::destination_point(l.midpoint(), 7.0, 20.0),
            true,
        );
        assert!(
            ((s.bearing() + 90.0).rem_euclid(15.0))
                .min(15.0 - (s.bearing() + 90.0).rem_euclid(15.0))
                < 0.05
        );
        let e = l.dragged(Grab::B, [-96.5, 35.47], true);
        let brg = crate::geo::bearing_deg(e.a, e.b);
        assert!((brg / 15.0 - (brg / 15.0).round()).abs() < 0.01, "{brg}");
    }

    #[test]
    fn a_radial_snap_runs_through_the_radar_and_keeps_its_middle_and_direction() {
        let l = line();
        let r = l.radial_through(KTLX);
        assert!(close(r.midpoint(), l.midpoint(), 0.01));
        // The radar sits on the great circle through the new line.
        let along = crate::geo::bearing_deg(r.midpoint(), KTLX);
        let diff = ((along - r.bearing() + 540.0).rem_euclid(360.0) - 180.0).abs();
        assert!(diff < 0.05 || (diff - 180.0).abs() < 0.05, "{diff}");
        // The nearer of the two directions: within 90° of where it pointed.
        let turn = ((r.bearing() - l.bearing() + 540.0).rem_euclid(360.0) - 180.0).abs();
        assert!(turn <= 90.0, "{turn}");
    }

    #[test]
    fn handles_are_found_within_their_radius_and_nearest_wins() {
        let l = line();
        // A flat test projection: one point per 0.01°.
        let to_px = |ll: [f64; 2]| egui::pos2((ll[0] * 100.0) as f32, (-ll[1] * 100.0) as f32);
        let at = |ll: [f64; 2]| to_px(ll);
        assert_eq!(grab_at(&l, at(l.a), 12.0, to_px), Some(Grab::A));
        assert_eq!(
            grab_at(&l, at(l.b) + egui::vec2(5.0, 5.0), 12.0, to_px),
            Some(Grab::B)
        );
        assert_eq!(
            grab_at(&l, at(l.midpoint()), 12.0, to_px),
            Some(Grab::Middle)
        );
        assert_eq!(
            grab_at(&l, at(l.rotate_handle()), 12.0, to_px),
            Some(Grab::Rotate)
        );
        // Off the side away from the rotation arm (which sits to the right of A→B).
        let off = at(l.midpoint()) + egui::vec2(0.0, -18.0);
        assert_eq!(grab_at(&l, off, 12.0, to_px), None, "a cursor misses");
        assert!(
            grab_at(&l, off, 24.0, to_px).is_some(),
            "a fingertip reaches"
        );
    }

    #[test]
    fn the_window_controls_apply_in_order_and_an_unchanged_window_changes_nothing() {
        use crate::ui::xsection_window::XsControls;
        let l = line();
        let before = XsControls {
            bearing_deg: l.bearing(),
            length_km: l.length_km(),
            ..Default::default()
        };
        assert_eq!(edited(l, &before, &before.clone(), Some("KTLX")), l);
        let ctl = XsControls {
            bearing_deg: 100.0,
            length_km: 40.0,
            slide_km: 2.0,
            swap: true,
            ..before.clone()
        };
        let e = edited(l, &before, &ctl, Some("KTLX"));
        assert!((e.length_km() - 40.0).abs() < 0.01);
        assert!(
            (e.bearing() - 280.0).abs() < 0.1,
            "swung to 100°, then swapped: {}",
            e.bearing()
        );
        assert!((crate::geo::great_circle(e.midpoint(), l.midpoint()).0 - 2.0).abs() < 0.01);
        let radial = XsControls {
            radial: true,
            ..before.clone()
        };
        let r = edited(l, &before, &radial, Some("KTLX"));
        assert_eq!(r, l.radial_through(KTLX_SITE()));
    }

    #[allow(non_snake_case)]
    fn KTLX_SITE() -> [f64; 2] {
        let s = wxdata::sites::site_by_id("KTLX").unwrap();
        [f64::from(s.longitude), f64::from(s.latitude)]
    }

    /// The 3D cut is the section's own ground: the existing ground-track inverse lands on it.
    #[test]
    fn the_3d_cut_runs_along_the_section() {
        for l in [line(), line().with_bearing(200.0), line().slid(-40.0)] {
            let half_km = 150.0;
            let p = l.plane(KTLX, half_km);
            assert!(p.thickness.is_some_and(|t| t > 0.0 && t < 0.1));
            let (a, b) = crate::render3d::plane_ground_track(p, KTLX, half_km);
            let track = SectionLine { a, b };
            // The section's middle lies on the cut's ground track (within a few hundred metres of
            // sphere-vs-plane difference over this distance).
            let to_mid = crate::geo::bearing_deg(track.a, l.midpoint());
            let d = crate::geo::great_circle(track.a, l.midpoint()).0;
            let off = d
                * (to_mid - crate::geo::bearing_deg(track.a, track.b))
                    .to_radians()
                    .sin();
            assert!(off.abs() < 0.5, "{off} km off the cut");
            // And it runs the same way (either direction).
            let turn = ((track.bearing() - l.bearing() + 540.0).rem_euclid(360.0) - 180.0).abs();
            assert!(turn < 1.0 || (turn - 180.0).abs() < 1.0, "{turn}");
        }
    }
}
