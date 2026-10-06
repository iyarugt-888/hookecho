//! The map's measure tool: great-circle distance and bearing between two clicks, and how high the
//! pane's beam is over the far end — above the radar antenna, which is what the beam model gives,
//! and above sea level from the site's antenna altitude, each named so neither is read as the
//! other (ROADMAP_PARITY M3.6) — and above the ground there, from the terrain tiles
//! (`terrain_cache`), with the grid's resolution, once they are here; until then it says so.
//! Moved out of `app.rs`.
use super::*;

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
        let screen = |ll: [f64; 2]| {
            let w = crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
            let (sx, sy) = cam.world_to_screen(w, vp);
            egui::pos2(prect.left() + sx, prect.top() + sy)
        };
        for &pt in &self.measure {
            painter.circle_filled(screen(pt), 3.5, col);
        }
        if self.measure.len() == 2 {
            let (a, b) = (screen(self.measure[0]), screen(self.measure[1]));
            painter.line_segment([a, b], egui::Stroke::new(2.0, col));
            let (km, brg) = crate::geo::great_circle(self.measure[0], self.measure[1]);
            let txt = measure_label(
                km,
                brg,
                self.metric_in(idx),
                self.beam_over(idx, self.measure[1]),
            );
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
