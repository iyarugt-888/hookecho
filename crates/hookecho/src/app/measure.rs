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

/// An end picked on a tilt: the tilt's elevation and the beam centre's height there, feet above
/// mean sea level (true geometry, not the 3D view's exaggeration).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AirEnd {
    pub elevation_deg: f32,
    pub msl_ft: f64,
}

/// The 3D ruler's label (ROADMAP_PARITY M3.6): the horizontal distance and bearing, each end's
/// tilt and height above sea level, the rise from the first end to the second, and the
/// straight-line distance between the two points in the air.
pub(crate) fn ruler_3d_label(
    km: f64,
    bearing_deg: f64,
    metric: bool,
    a: AirEnd,
    b: AirEnd,
) -> String {
    let rise_ft = b.msl_ft - a.msl_ft;
    let straight_km = km.hypot(rise_ft / 3_280.84);
    format!(
        "{}  @ {bearing_deg:.0}°  ·  {:.1}° {:.0} ft → {:.1}° {:.0} ft MSL  ·  rise {rise_ft:+.0} ft  ·  straight line {}",
        crate::geo::fmt_distance(km, metric, 1),
        a.elevation_deg,
        a.msl_ft,
        b.elevation_deg,
        b.msl_ft,
        crate::geo::fmt_distance(straight_km, metric, 1),
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

    /// Where a click on pane `idx` lands in the 3D Observed view: the tilt whose beam surface the
    /// ray meets first, and the ground under that hit. `None` outside that view or past every
    /// tilt. Shared by the gate inspector and the 3D ruler.
    pub(crate) fn pick_3d(
        &self,
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
        crate::render3d::pick_observed_tilt(
            cam,
            px,
            vp,
            site.longitude as f64,
            site.latitude as f64,
            site.elevation_meters as f64 + wxdata::towers::tower_m(site.id),
            v.map_3d.vertical_exaggeration as f64,
            v.map_3d.beam_rise as f64,
            elevations,
        )
    }

    /// A measure end picked on a tilt, in the air: its tilt and height above sea level.
    pub(crate) fn air_end(&self, idx: usize, p: MeasurePoint) -> Option<AirEnd> {
        let v = &self.views[idx];
        let site = wxdata::sites::site_by_id(v.site.as_deref()?)?;
        let elevation_deg = *v.volume.as_ref()?.elevations.get(p.tilt?)?;
        let (km, _) = crate::geo::great_circle([site.longitude as f64, site.latitude as f64], p.ll);
        let antenna_m = f64::from(site.elevation_meters) + wxdata::towers::tower_m(site.id);
        let above_radar_m = wxdata::xsection::beam_height_km(km, elevation_deg as f64) * 1_000.0;
        Some(AirEnd {
            elevation_deg,
            msl_ft: (above_radar_m + antenna_m) * 3.280_84,
        })
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

    /// Draw the measure tool's points, line and label on pane `idx`.
    pub(crate) fn paint_measure(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        idx: usize,
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
        for &pt in &self.measure {
            painter.circle_filled(screen(pt), 3.5, col);
        }
        if self.measure.len() == 2 {
            let (a, b) = (screen(self.measure[0]), screen(self.measure[1]));
            painter.line_segment([a, b], egui::Stroke::new(2.0, col));
            let (km, brg) = crate::geo::great_circle(self.measure[0].ll, self.measure[1].ll);
            // Both ends on tilts in the 3D view: a ruler in the air. Otherwise the map's ruler.
            let txt = match (
                self.air_end(idx, self.measure[0]),
                self.air_end(idx, self.measure[1]),
            ) {
                (Some(ea), Some(eb)) => ruler_3d_label(km, brg, self.metric_in(idx), ea, eb),
                _ => measure_label(
                    km,
                    brg,
                    self.metric_in(idx),
                    self.beam_over(idx, self.measure[1].ll),
                ),
            };
            let mid = a + (b - a) * 0.5;
            painter.text(
                mid + egui::vec2(0.0, -10.0),
                egui::Align2::CENTER_BOTTOM,
                txt,
                egui::FontId::proportional(12.0),
                col,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 3D ruler between two points in the air: 10 km apart over the ground, from 2,000 ft on
    /// the 0.5° tilt to 12,000 ft on the 3.1° tilt, rises 10,000 ft over a straight line of
    /// hypot(10 km, 3.048 km) = 10.45 km.
    #[test]
    fn the_3d_ruler_reads_rise_and_straight_line() {
        let a = AirEnd {
            elevation_deg: 0.5,
            msl_ft: 2_000.0,
        };
        let b = AirEnd {
            elevation_deg: 3.1,
            msl_ft: 12_000.0,
        };
        let l = ruler_3d_label(10.0, 45.0, true, a, b);
        assert!(l.contains("0.5° 2000 ft → 3.1° 12000 ft MSL"), "{l}");
        assert!(l.contains("rise +10000 ft"), "{l}");
        let straight = crate::geo::fmt_distance(10.0f64.hypot(10_000.0 / 3_280.84), true, 1);
        assert!(l.ends_with(&format!("straight line {straight}")), "{l}");
        assert!(straight.starts_with("10.5"), "{straight}");
        // Downhill reads as a negative rise.
        assert!(ruler_3d_label(10.0, 45.0, true, b, a).contains("rise -10000 ft"));
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
