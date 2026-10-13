//! The map's measure tool: great-circle distance and bearing between two clicks, and how high the
//! pane's beam is over the far end — above the radar antenna, which is what the beam model gives,
//! and above sea level from the site's antenna altitude, each named so neither is read as the
//! other (ROADMAP_PARITY M3.6) — and above the ground there, from the terrain tiles
//! (`terrain_cache`), with the grid's resolution, once they are here; until then it says so.
//! Moved out of `app.rs`.
use super::*;

/// One end of the measure line: where it is on the ground, and, in the 3D Observed view, the tilt
/// it was picked on (`render3d::pick_observed_tilt`), which puts it in the air on that beam.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MeasurePoint {
    /// `[lon, lat]` of the ground under it.
    pub ll: [f64; 2],
    pub tilt: Option<usize>,
}

/// An end of the ruler held by a drag: which pane, which end, and the ruler as it was (Esc puts it
/// back).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EndDrag {
    pub pane: usize,
    pub end: usize,
    pub before: [MeasurePoint; 2],
}

/// The first hit, nearest first, where `measured` says a gate was measured; `None` when none was.
pub(crate) fn prefer_measured<T: Copy>(
    hits: Vec<T>,
    mut measured: impl FnMut(T) -> bool,
) -> Option<T> {
    hits.into_iter().find(|h| measured(*h))
}

/// How far from an end a press may land and still take it, in points: a finger is wider.
const END_GRAB_PT: f32 = 12.0;
const END_GRAB_PT_TOUCH: f32 = 24.0;

/// The end nearest `p` within `radius`, given the ends' screen positions.
pub(crate) fn end_at(ends: [egui::Pos2; 2], p: egui::Pos2, radius: f32) -> Option<usize> {
    ends.into_iter()
        .enumerate()
        .map(|(i, e)| (i, e.distance(p)))
        .filter(|(_, d)| *d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// An end picked on a tilt: the tilt's elevation and the beam centre's height there, feet above
/// mean sea level (true geometry, not the 3D view's exaggeration), the ground under it, and what
/// the pane's product reads at that gate.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AirEnd {
    pub elevation_deg: f32,
    pub msl_ft: f64,
    pub ground: super::terrain_cache::Ground,
    /// The gate's value as shown ("52 dBZ", "range folded", "below threshold"); `None` when the
    /// point is outside the tilt's coverage.
    pub reading: Option<String>,
}

/// What the pane's product reads at a measure end on its tilt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EndReading {
    pub moment: Moment,
    pub value: Option<f32>,
    pub folded: bool,
}

/// An end's reading in the pane's display units: velocity and spectrum width follow the Units
/// setting, and velocity is the raw (aliased) value the sweep carries, said so.
pub(crate) fn reading_text(r: EndReading, factor: f32, unit: &str) -> String {
    if r.folded {
        return "range folded".into();
    }
    let Some(v) = r.value else {
        return "below threshold".into();
    };
    let decimals = match r.moment {
        Moment::CorrelationCoefficient => 2,
        Moment::DifferentialReflectivity | Moment::SpecificDifferentialPhase => 1,
        _ => 0,
    };
    let raw = if r.moment == Moment::Velocity {
        " raw"
    } else {
        ""
    };
    format!("{:.*} {unit}{raw}", decimals, v * factor)
        .trim_end()
        .to_string()
}

/// One end of the 3D ruler: its tilt, its height above sea level and above the ground, and the
/// gate's value.
fn air_end_text(e: &AirEnd) -> String {
    use super::terrain_cache::Ground;
    let mut t = format!("{:.1}°  {:.0} ft MSL", e.elevation_deg, e.msl_ft);
    match e.ground {
        Ground::Known { msl_m, .. } => {
            t.push_str(&format!(", {:.0} ft AGL", e.msl_ft - msl_m * 3.280_84))
        }
        Ground::Loading => t.push_str(", ground loading"),
        Ground::Unknown => {}
    }
    if let Some(r) = &e.reading {
        t.push_str(&format!(", {r}"));
    }
    t
}

/// The 3D ruler's label (ROADMAP_PARITY M3.6), on two lines: the horizontal distance and bearing,
/// the rise from the first end to the second and the straight-line distance between the two points
/// in the air; then each end's tilt, heights and the gate's value.
pub(crate) fn ruler_3d_label(
    km: f64,
    bearing_deg: f64,
    metric: bool,
    a: &AirEnd,
    b: &AirEnd,
) -> String {
    let rise_ft = b.msl_ft - a.msl_ft;
    let straight_km = km.hypot(rise_ft / 3_280.84);
    format!(
        "{}  @ {bearing_deg:.0}°  ·  rise {rise_ft:+.0} ft  ·  straight line {}\n{}  →  {}",
        crate::geo::fmt_distance(km, metric, 1),
        crate::geo::fmt_distance(straight_km, metric, 1),
        air_end_text(a),
        air_end_text(b),
    )
}

/// The beam over a point: height above the radar antenna, and above mean sea level when the
/// antenna's altitude is known; feet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BeamOver {
    pub above_radar_ft: f64,
    pub msl_ft: Option<f64>,
    /// The ground under the point, as far as is known.
    pub ground: super::terrain_cache::Ground,
}

/// The measure line's label: distance and bearing, then the beam over the far end.
pub(crate) fn measure_label(
    km: f64,
    bearing_deg: f64,
    metric: bool,
    beam: Option<BeamOver>,
) -> String {
    let mut txt = format!(
        "{}  @ {bearing_deg:.0}°",
        crate::geo::fmt_distance(km, metric, 1)
    );
    // How high the beam is over the far end of the line: the number that decides whether
    // "there's nothing on radar there" means the storm is weak or the scan is over its head.
    if let Some(b) = beam {
        txt.push_str(&format!(
            "  ·  beam {:.0} ft above the radar",
            b.above_radar_ft
        ));
        use super::terrain_cache::Ground;
        if let Some(msl) = b.msl_ft {
            let ground = match b.ground {
                Ground::Known {
                    msl_m,
                    resolution_m,
                } => format!(
                    ", {:.0} ft above the ground (terrain on a {resolution_m:.0} m grid)",
                    msl - msl_m * 3.280_84
                ),
                Ground::Loading => ", ground loading".into(),
                Ground::Unknown => ", ground unknown".into(),
            };
            txt.push_str(&format!(" ({msl:.0} ft MSL{ground})"));
        }
    }
    txt
}

impl HookEchoApp {
    /// Approximate map view range in nautical miles (viewport height), for placefile thresholds.
    /// `// ponytail: coarse mercator estimate; fine for zoom-gating, not for measuring.`
    pub(crate) fn view_range_nmi(&self) -> f32 {
        let cam = &self.views[self.active].camera;
        let world_h = self.last_viewport.1 as f64 * cam.world_per_pixel();
        let s = (cam.center.1 * 2.0 - 1.0) * std::f64::consts::PI;
        let coslat = (1.0 / s.cosh()).max(0.05); // cos(lat) = sech(mercator y)
        (world_h * 40075.017 * coslat / 1.852) as f32
    }

    /// Pane `idx`'s beam centre over the point `ll` (`[lon, lat]`) on its displayed tilt. `None`
    /// when the pane has no site or no loaded tilt.
    ///
    /// Ground range is close enough to slant range for the shallow tilts this is read at, and the
    /// 4/3-earth model is the same one the cross-section draws with
    /// ([`wxdata::xsection::beam_height_km`]), so the two agree.
    pub(crate) fn beam_over(&self, idx: usize, ll: [f64; 2]) -> Option<BeamOver> {
        let v = &self.views[idx];
        let site = wxdata::sites::site_by_id(v.site.as_deref()?)?;
        let elev = *v.volume.as_ref()?.elevations.get(v.tilt)? as f64;
        let (km, _) = crate::geo::great_circle([site.longitude as f64, site.latitude as f64], ll);
        let above_radar_ft = wxdata::xsection::beam_height_km(km, elev) * 3280.84;
        let antenna_m = f64::from(site.elevation_meters) + wxdata::towers::tower_m(site.id);
        Some(BeamOver {
            above_radar_ft,
            msl_ft: Some(above_radar_ft + antenna_m * 3.280_84),
            ground: self.terrain.ground(ll[0], ll[1]),
        })
    }

    /// Where a click on pane `idx` lands in the 3D Observed view: the nearest tilt along the ray
    /// with a measured gate there in the pane's product (a value, or range folded: what is
    /// drawn), else the nearest tilt the ray meets at all; and the ground under that hit. An
    /// empty gate is drawn transparent, so the echo behind it is what the click is on
    /// (roadmaps/radar-analysis R5). `None` outside that view or past every tilt. Shared by the
    /// gate inspector and the 3D ruler.
    pub(crate) fn pick_3d(
        &mut self,
        idx: usize,
        cam: &crate::render::mercator::Camera,
        px: (f32, f32),
        vp: (f32, f32),
    ) -> Option<(usize, f64, f64)> {
        let v = &self.views[idx];
        if !v.map_3d.enabled
            || v.map_3d.representation != crate::view::Map3dRepresentation::ObservedSweeps
        {
            return None;
        }
        let site = v.site.as_deref().and_then(wxdata::sites::site_by_id)?;
        let elevations = &v.volume.as_ref()?.elevations;
        let hits = crate::render3d::pick_observed_tilts(
            cam,
            px,
            vp,
            site.longitude as f64,
            site.latitude as f64,
            site.elevation_meters as f64 + wxdata::towers::tower_m(site.id),
            v.map_3d.vertical_exaggeration as f64,
            v.map_3d.beam_rise as f64,
            elevations,
        );
        let first = *hits.first()?;
        let moment = v.moment;
        let v = &mut self.views[idx];
        let measured = |v: &mut crate::view::MapView, (tilt, lon, lat): (usize, f64, f64)| {
            v.volume
                .as_mut()
                .and_then(|vol| vol.binned(moment, tilt, false).ok())
                .and_then(|sweep| sweep.sample_at(lon, lat))
                .is_some_and(|s| s.value.is_some() || s.folded)
        };
        Some(prefer_measured(hits, |hit| measured(v, hit)).unwrap_or(first))
    }

    /// A measure end picked on a tilt, in the air: its tilt, height above sea level, the ground
    /// under it as far as the terrain tiles know, and `reading` (from [`Self::measure_readings`]).
    pub(crate) fn air_end(
        &self,
        idx: usize,
        p: MeasurePoint,
        reading: Option<EndReading>,
    ) -> Option<AirEnd> {
        let v = &self.views[idx];
        let site = wxdata::sites::site_by_id(v.site.as_deref()?)?;
        let elevation_deg = *v.volume.as_ref()?.elevations.get(p.tilt?)?;
        let (km, _) = crate::geo::great_circle([site.longitude as f64, site.latitude as f64], p.ll);
        let antenna_m = f64::from(site.elevation_meters) + wxdata::towers::tower_m(site.id);
        let above_radar_m = wxdata::xsection::beam_height_km(km, elevation_deg as f64) * 1_000.0;
        let reading = reading.map(|r| {
            let (factor, unit) = super::display_units(r.moment, &self.settings);
            reading_text(r, factor, unit)
        });
        Some(AirEnd {
            elevation_deg,
            msl_ft: (above_radar_m + antenna_m) * 3.280_84,
            ground: self.terrain.ground(p.ll[0], p.ll[1]),
            reading,
        })
    }

    /// What pane `idx`'s product reads at each measure end on its own tilt, from that tilt's
    /// binned sweep (cached: the 3D view bins every tilt it draws). `None` for an end on the
    /// ground or outside the tilt's coverage.
    pub(crate) fn measure_readings(&mut self, idx: usize) -> Vec<Option<EndReading>> {
        let points = self.measure.clone();
        let Some(v) = self.views.get_mut(idx) else {
            return Vec::new();
        };
        let moment = v.moment;
        points
            .iter()
            .map(|p| {
                let sweep = v.volume.as_mut()?.binned(moment, p.tilt?, false).ok()?;
                let s = sweep.sample_at(p.ll[0], p.ll[1])?;
                Some(EndReading {
                    moment,
                    value: s.value,
                    folded: s.folded,
                })
            })
            .collect()
    }

    /// PgUp/PgDn with the Measure tool: move the last end placed in the 3D Observed view up or
    /// down a tilt over the same ground (ROADMAP_PARITY M3.6), so the ruler spans the tilts the
    /// analyst means rather than whichever beam a click met first. Untouched otherwise.
    pub(crate) fn measure_tilt_keys(&mut self, ctx: &egui::Context) {
        if self.tool != MapTool::Measure || ctx.egui_wants_keyboard_input() {
            return;
        }
        let Some(tilt) = self.measure.last().and_then(|p| p.tilt) else {
            return;
        };
        let tilts = self.views[self.active]
            .volume
            .as_ref()
            .map_or(0, |v| v.elevations.len());
        // Counted, not just seen: presses that land in one frame each move a tilt.
        let (up, down) = ctx.input_mut(|i| {
            (
                i.count_and_consume_key(egui::Modifiers::NONE, egui::Key::PageUp),
                i.count_and_consume_key(egui::Modifiers::NONE, egui::Key::PageDown),
            )
        });
        let next = step_tilt(tilt, tilts, up, down);
        if let Some(p) = self.measure.last_mut() {
            p.tilt = Some(next);
        }
    }

    /// Whether a Measure click on the active pane lands on a tilt, in the air: the 3D Observed
    /// view with a volume.
    pub(crate) fn measure_in_air(&self) -> bool {
        let v = &self.views[self.active];
        v.map_3d.enabled
            && v.map_3d.representation == crate::view::Map3dRepresentation::ObservedSweeps
            && v.volume.is_some()
    }

    /// Where a measure end is drawn: on its beam in the 3D Observed view, the way the sweeps
    /// themselves are drawn (exaggeration and beam rise included), else on the ground.
    fn measure_screen(
        &self,
        idx: usize,
        cam: &crate::render::mercator::Camera,
        vp: (f32, f32),
        p: MeasurePoint,
    ) -> Option<(f32, f32)> {
        let v = &self.views[idx];
        let in_air = v.map_3d.enabled
            && v.map_3d.representation == crate::view::Map3dRepresentation::ObservedSweeps;
        if let (true, Some(tilt)) = (in_air, p.tilt) {
            let site = wxdata::sites::site_by_id(v.site.as_deref()?)?;
            let elevation_deg = *v.volume.as_ref()?.elevations.get(tilt)?;
            let radar = [site.longitude as f64, site.latitude as f64];
            let (km, bearing) = crate::geo::great_circle(radar, p.ll);
            let local = crate::render3d::beam_local(
                cam,
                radar[0],
                radar[1],
                f64::from(site.elevation_meters) + wxdata::towers::tower_m(site.id),
                v.map_3d.vertical_exaggeration as f64,
                v.map_3d.beam_rise as f64,
                bearing,
                km,
                elevation_deg,
            );
            return crate::render3d::project_local(cam, vp, local);
        }
        let w = crate::render::mercator::lonlat_to_world(p.ll[0], p.ll[1]);
        Some(cam.world_to_screen(w, vp))
    }

    /// Take a drag that starts on an end of a finished ruler, with the measure tool armed: that end
    /// follows the pointer (onto the tilt under it in the 3D Observed view, as a click would put
    /// it) and the ground under it is read again when it is let go. Esc puts the ruler back.
    /// Returns whether it owns the pointer, so the map does not pan under it.
    pub(crate) fn measure_input(
        &mut self,
        idx: usize,
        prect: egui::Rect,
        response: &egui::Response,
        ui: &egui::Ui,
        allow_pointer: bool,
        ctx: &egui::Context,
    ) -> bool {
        let cancelled = ui.input(|i| !i.focused || i.key_pressed(egui::Key::Escape));
        if self.tool != MapTool::Measure || cancelled || !allow_pointer || self.measure.len() != 2 {
            if let Some(drag) = self.measure_drag.take() {
                if cancelled {
                    self.measure = drag.before.to_vec();
                }
            }
            return false;
        }
        if self.measure_drag.is_some_and(|d| d.pane != idx) {
            return false;
        }
        let vp = (prect.width(), prect.height());
        let cam = self.views[idx].camera;
        let to_px = |s: Option<(f32, f32)>| {
            let (x, y) = s.unwrap_or((-1.0e6, -1.0e6));
            egui::pos2(prect.left() + x, prect.top() + y)
        };
        if response.drag_started_by(egui::PointerButton::Primary) {
            let Some(p) = ui.input(|i| i.pointer.press_origin()) else {
                return false;
            };
            let ends = [
                to_px(self.measure_screen(idx, &cam, vp, self.measure[0])),
                to_px(self.measure_screen(idx, &cam, vp, self.measure[1])),
            ];
            let touch = ui.input(|i| i.any_touches() || i.has_touch_screen());
            let radius = if touch {
                END_GRAB_PT_TOUCH
            } else {
                END_GRAB_PT
            };
            let Some(end) = end_at(ends, p, radius) else {
                return false;
            };
            self.active = idx;
            self.measure_drag = Some(EndDrag {
                pane: idx,
                end,
                before: [self.measure[0], self.measure[1]],
            });
        }
        let Some(drag) = self.measure_drag else {
            return false;
        };
        if let Some(p) = response.interact_pointer_pos() {
            let px = (p.x - prect.left(), p.y - prect.top());
            let point = match self.pick_3d(idx, &cam, px, vp) {
                Some((tilt, lon, lat)) => MeasurePoint {
                    ll: [lon, lat],
                    tilt: Some(tilt),
                },
                None => {
                    let w = cam.screen_to_world(px, vp);
                    let (lon, lat) = crate::render::mercator::world_to_lonlat(w.0, w.1);
                    MeasurePoint {
                        ll: [lon, lat],
                        tilt: None,
                    }
                }
            };
            self.measure[drag.end] = point;
        }
        if response.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
            self.measure_drag = None;
            let [lon, lat] = self.measure[drag.end].ll;
            self.request_ground(lon, lat, ctx);
        }
        true
    }

    /// Draw the measure tool's points, line and label on pane `idx`; `readings` are each end's
    /// gate values on its tilt ([`Self::measure_readings`]).
    pub(crate) fn paint_measure(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        idx: usize,
        readings: &[Option<EndReading>],
    ) {
        if self.measure.is_empty() {
            return;
        }
        let col = egui::Color32::from_rgb(255, 210, 80);
        let screen = |p: MeasurePoint| {
            let (sx, sy) = self
                .measure_screen(idx, &cam, vp, p)
                .unwrap_or((-1.0e6, -1.0e6));
            egui::pos2(prect.left() + sx, prect.top() + sy)
        };
        // With the tool armed, a finished ruler's ends are handles: ringed, the held one larger.
        let handles = self.tool == MapTool::Measure && self.measure.len() == 2;
        let held = self.measure_drag.map(|d| d.end);
        for (i, &pt) in self.measure.iter().enumerate() {
            let at = screen(pt);
            if handles {
                let r = if held == Some(i) { 8.0 } else { 6.0 };
                painter.circle_filled(at, r, egui::Color32::from_black_alpha(160));
                painter.circle_stroke(at, r, egui::Stroke::new(2.0, col));
            }
            painter.circle_filled(at, 3.5, col);
        }
        if self.measure.len() == 2 {
            let (a, b) = (screen(self.measure[0]), screen(self.measure[1]));
            painter.line_segment([a, b], egui::Stroke::new(2.0, col));
            let (km, brg) = crate::geo::great_circle(self.measure[0].ll, self.measure[1].ll);
            // Both ends on tilts in the 3D view: a ruler in the air. Otherwise the map's ruler.
            let reading = |i: usize| readings.get(i).copied().flatten();
            let txt = match (
                self.air_end(idx, self.measure[0], reading(0)),
                self.air_end(idx, self.measure[1], reading(1)),
            ) {
                (Some(ea), Some(eb)) => ruler_3d_label(km, brg, self.metric_in(idx), &ea, &eb),
                _ => measure_label(
                    km,
                    brg,
                    self.metric_in(idx),
                    self.beam_over(idx, self.measure[1].ll),
                ),
            };
            // On a dark card: over radar, roads and warnings, bare yellow text is unreadable.
            let mid = a + (b - a) * 0.5;
            let galley = painter.layout_no_wrap(txt, egui::FontId::proportional(12.0), col);
            let rect = keep_inside(
                egui::Align2::CENTER_BOTTOM
                    .anchor_size(mid + egui::vec2(0.0, -10.0), galley.size())
                    .expand2(egui::vec2(6.0, 3.0)),
                painter.clip_rect(),
            );
            painter.rect_filled(rect, 4.0, egui::Color32::from_black_alpha(200));
            painter.galley(rect.min + egui::vec2(6.0, 3.0), galley, col);
        }
    }
}

/// `card` moved in from whichever edge of `bounds` it crosses. One wider or taller than the bounds
/// is pinned to the left or top edge, so the reading's start stays on screen: a ruler near the
/// pane's edge, or a 3D ruler's two-line reading in a narrow pane, otherwise loses its first words.
fn keep_inside(card: egui::Rect, bounds: egui::Rect) -> egui::Rect {
    let shift = |lo: f32, hi: f32, min: f32, max: f32| {
        let mut d = 0.0;
        if hi > max {
            d = max - hi;
        }
        if lo + d < min {
            d = min - lo;
        }
        d
    };
    card.translate(egui::vec2(
        shift(card.left(), card.right(), bounds.left(), bounds.right()),
        shift(card.top(), card.bottom(), bounds.top(), bounds.bottom()),
    ))
}

/// The tilt `up` PgUp and `down` PgDn presses move an end to: one tilt each, held at the lowest
/// and highest of `tilts`; unchanged when the volume has no tilts.
pub(crate) fn step_tilt(tilt: usize, tilts: usize, up: usize, down: usize) -> usize {
    if tilts == 0 {
        return tilt;
    }
    (tilt + up).saturating_sub(down).min(tilts - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reading's card stays in the pane: moved in from the edge it crosses, and pinned to the
    /// left/top edge when it is wider/taller than the pane, so its first words are always shown.
    #[test]
    fn the_reading_stays_inside_the_pane() {
        let pane = egui::Rect::from_min_max(egui::pos2(100.0, 50.0), egui::pos2(500.0, 400.0));
        let card = |x0: f32, y0: f32, w: f32, h: f32| {
            egui::Rect::from_min_size(egui::pos2(x0, y0), egui::vec2(w, h))
        };
        let inside = card(200.0, 100.0, 100.0, 30.0);
        assert_eq!(keep_inside(inside, pane), inside);
        assert_eq!(
            keep_inside(card(60.0, 100.0, 100.0, 30.0), pane).min,
            egui::pos2(100.0, 100.0)
        );
        assert_eq!(
            keep_inside(card(450.0, 30.0, 100.0, 30.0), pane).min,
            egui::pos2(400.0, 50.0)
        );
        assert_eq!(
            keep_inside(card(300.0, 390.0, 100.0, 30.0), pane).min,
            egui::pos2(300.0, 370.0)
        );
        let wide = keep_inside(card(20.0, 100.0, 600.0, 30.0), pane);
        assert_eq!(wide.left(), pane.left(), "too wide: its start is shown");
    }

    /// A pick passes over an empty nearer tilt to the measured gate behind it, and falls back to
    /// the nearest when none along the ray was measured.
    #[test]
    fn a_pick_prefers_the_nearest_measured_gate() {
        let hits = vec![(3usize, 0.0, 0.0), (1, 0.0, 0.0), (0, 0.0, 0.0)];
        assert_eq!(
            prefer_measured(hits.clone(), |(tilt, ..)| tilt <= 1).map(|h| h.0),
            Some(1)
        );
        assert_eq!(prefer_measured(hits, |_| false), None);
    }

    /// A press takes the nearer end within reach, and none beyond it.
    #[test]
    fn a_press_takes_the_nearer_end_within_reach() {
        let ends = [egui::pos2(100.0, 100.0), egui::pos2(110.0, 100.0)];
        assert_eq!(end_at(ends, egui::pos2(103.0, 101.0), 12.0), Some(0));
        assert_eq!(end_at(ends, egui::pos2(108.0, 99.0), 12.0), Some(1));
        assert_eq!(end_at(ends, egui::pos2(140.0, 100.0), 12.0), None);
        assert_eq!(end_at(ends, egui::pos2(140.0, 100.0), 32.0), Some(1));
    }

    /// The 3D ruler between two points in the air: 10 km apart over the ground, from 2,000 ft on
    /// the 0.5° tilt to 12,000 ft on the 3.1° tilt, rises 10,000 ft over a straight line of
    /// hypot(10 km, 3.048 km) = 10.45 km.
    #[test]
    fn the_3d_ruler_reads_rise_and_straight_line() {
        use crate::app::terrain_cache::Ground;
        let a = AirEnd {
            elevation_deg: 0.5,
            msl_ft: 2_000.0,
            // 1,200 ft of ground under it.
            ground: Ground::Known {
                msl_m: 365.76,
                resolution_m: 62.0,
            },
            reading: Some("52 dBZ".into()),
        };
        let b = AirEnd {
            elevation_deg: 3.1,
            msl_ft: 12_000.0,
            ground: Ground::Loading,
            reading: None,
        };
        let l = ruler_3d_label(10.0, 45.0, true, &a, &b);
        let (top, ends) = l.split_once('\n').expect("two lines");
        assert!(top.contains("rise +10000 ft"), "{l}");
        let straight = crate::geo::fmt_distance(10.0f64.hypot(10_000.0 / 3_280.84), true, 1);
        assert!(top.ends_with(&format!("straight line {straight}")), "{l}");
        assert!(straight.starts_with("10.5"), "{straight}");
        assert_eq!(
            ends,
            "0.5°  2000 ft MSL, 800 ft AGL, 52 dBZ  →  3.1°  12000 ft MSL, ground loading"
        );
        // Downhill reads as a negative rise; unknown ground is left out rather than guessed.
        let b_unknown = AirEnd {
            ground: Ground::Unknown,
            ..b.clone()
        };
        let down = ruler_3d_label(10.0, 45.0, true, &b_unknown, &a);
        assert!(down.contains("rise -10000 ft"), "{down}");
        assert!(down.contains("\n3.1°  12000 ft MSL  →  "), "{down}");
    }

    /// A gate's value as the ruler says it: in display units, folded and below-threshold gates in
    /// words, velocity marked raw.
    #[test]
    fn an_end_reading_says_what_the_gate_holds() {
        let r = |moment, value, folded| EndReading {
            moment,
            value,
            folded,
        };
        assert_eq!(
            reading_text(r(Moment::Reflectivity, Some(52.4), false), 1.0, "dBZ"),
            "52 dBZ"
        );
        assert_eq!(
            reading_text(
                r(Moment::CorrelationCoefficient, Some(0.873), false),
                1.0,
                ""
            ),
            "0.87"
        );
        assert_eq!(
            reading_text(r(Moment::Velocity, Some(-20.0), false), 1.943_844, "kt"),
            "-39 kt raw"
        );
        assert_eq!(
            reading_text(r(Moment::Velocity, None, true), 1.0, "m/s"),
            "range folded"
        );
        assert_eq!(
            reading_text(r(Moment::Reflectivity, None, false), 1.0, "dBZ"),
            "below threshold"
        );
    }

    /// Each PgUp/PgDn moves an end one tilt, several in a frame several tilts, stopping at the
    /// volume's lowest and highest.
    #[test]
    fn the_tilt_steps_one_press_at_a_time_within_the_volume() {
        assert_eq!(step_tilt(2, 14, 1, 0), 3);
        assert_eq!(step_tilt(2, 14, 0, 1), 1);
        assert_eq!(step_tilt(13, 14, 1, 0), 13);
        assert_eq!(step_tilt(0, 14, 0, 1), 0);
        assert_eq!(step_tilt(4, 14, 0, 0), 4);
        assert_eq!(step_tilt(4, 14, 1, 1), 4);
        assert_eq!(step_tilt(13, 14, 0, 20), 0);
        assert_eq!(step_tilt(3, 14, 5, 0), 8);
        assert_eq!(step_tilt(0, 0, 1, 0), 0);
    }

    #[test]
    fn the_beam_height_names_what_it_is_measured_from() {
        use crate::app::terrain_cache::Ground;
        let beam = BeamOver {
            above_radar_ft: 6200.0,
            msl_ft: Some(7450.0),
            ground: Ground::Unknown,
        };
        let l = measure_label(50.0, 87.4, true, Some(beam));
        assert!(l.contains("@ 87°"), "{l}");
        assert!(
            l.ends_with("beam 6200 ft above the radar (7450 ft MSL, ground unknown)"),
            "{l}"
        );
        // 7450 ft MSL over ground at 365.8 m (1200 ft): 6250 ft above it.
        let over = measure_label(
            50.0,
            87.4,
            true,
            Some(BeamOver {
                ground: Ground::Known {
                    msl_m: 365.76,
                    resolution_m: 62.0,
                },
                ..beam
            }),
        );
        assert!(
            over.ends_with("(7450 ft MSL, 6250 ft above the ground (terrain on a 62 m grid))"),
            "{over}"
        );
        let loading = measure_label(
            50.0,
            87.4,
            true,
            Some(BeamOver {
                ground: Ground::Loading,
                ..beam
            }),
        );
        assert!(loading.contains("ground loading"), "{loading}");
        let no_msl = measure_label(
            50.0,
            87.4,
            true,
            Some(BeamOver {
                msl_ft: None,
                ..beam
            }),
        );
        assert!(no_msl.ends_with("above the radar"), "{no_msl}");
        assert!(!measure_label(50.0, 87.4, true, None).contains("beam"));
    }
}
