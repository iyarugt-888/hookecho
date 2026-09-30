//! Point layers a pane draws: storm spotters, webcams and the scan-age stamps. Moved out of
//! `render_pane` unchanged (ROADMAP_2 §7); each method takes the locals its block read, under the
//! same names.

use super::*;

impl HookEchoApp {
    /// Webcam markers.
    pub(crate) fn paint_webcams(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
    ) {
        if self.show_webcams {
            let show_labels = cam.zoom >= 8.0;
            let col = egui::Color32::from_rgb(110, 180, 240);
            // A camera under a tornado or severe-thunderstorm warning is the one worth opening,
            // and it looks exactly like the other forty until you click them all. Ring it.
            // Polygons the alert layer already holds; no extra fetch and no extra geometry.
            let threat: Vec<&GeoFeature> = self
                .alert_features
                .iter()
                .filter(|f| {
                    f.kind == overlay::FeatureKind::Warning
                        && f.alert.as_ref().is_some_and(|a| {
                            let e = a.event.to_ascii_lowercase();
                            e.contains("tornado") || e.contains("severe thunderstorm")
                        })
                })
                .collect();
            for site in &self.webcams {
                let w = crate::render::mercator::lonlat_to_world(site.lon, site.lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(p) {
                    continue;
                }
                painter.circle_filled(p, 4.0, col);
                painter.circle_stroke(
                    p,
                    4.0,
                    egui::Stroke::new(1.0, egui::Color32::from_black_alpha(170)),
                );
                // `distance_km` is 0 inside the polygon, which is the test we want here.
                if threat
                    .iter()
                    .any(|f| f.distance_km(site.lon, site.lat) == 0.0)
                {
                    painter.circle_stroke(
                        p,
                        7.0,
                        egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 120, 60)),
                    );
                }
                if show_labels {
                    painter.text(
                        p + egui::vec2(6.0, -5.0),
                        egui::Align2::LEFT_BOTTOM,
                        &site.name,
                        egui::FontId::proportional(10.0),
                        col,
                    );
                }
            }
        }
    }

    /// Storm spotters near the pane's radar, with their hover cards. Returns the one clicked.
    pub(crate) fn paint_spotters(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        idx: usize,
        response: &egui::Response,
    ) -> Option<wxdata::spotters::Spotter> {
        if self.show_spotters {
            if let Some(site_pos) = self.views[idx]
                .site
                .as_deref()
                .and_then(wxdata::sites::site_by_id)
                .map(|s| [s.longitude as f64, s.latitude as f64])
            {
                let now = Utc::now();
                let show_labels = cam.zoom >= 9.0;
                // The range limit is at most `range/110` degrees of latitude and, at CONUS
                // latitudes, ~1.4x that in longitude — a cheap box rejects almost every spotter
                // before the haversine runs. 0 means the user asked for the whole feed.
                let range_km = self.settings.spotter_range_km.max(0.0);
                let (max_dlat, max_dlon) = if range_km <= 0.0 {
                    (f64::INFINITY, f64::INFINITY)
                } else {
                    let dlat = range_km / 110.0;
                    (dlat, dlat * 1.45)
                };
                let mut spotter_click: Option<wxdata::spotters::Spotter> = None;
                for sp in &self.spotters {
                    if (sp.lon - site_pos[0]).abs() > max_dlon
                        || (sp.lat - site_pos[1]).abs() > max_dlat
                    {
                        continue;
                    }
                    if range_km > 0.0
                        && crate::geo::great_circle(site_pos, [sp.lon, sp.lat]).0 > range_km
                    {
                        continue;
                    }
                    let w = crate::render::mercator::lonlat_to_world(sp.lon, sp.lat);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                    if !prect.contains(p) {
                        continue;
                    }
                    // Spotter Network green; faded when the report is stale (>30 min old).
                    let stale = (now - sp.time).num_minutes() > 30;
                    let color = {
                        let g = egui::Color32::from_rgb(0, 200, 80);
                        if stale {
                            g.gamma_multiply(0.35)
                        } else {
                            g
                        }
                    };
                    painter.circle_filled(p, 3.0, color);
                    painter.circle_stroke(
                        p,
                        3.0,
                        egui::Stroke::new(1.0, egui::Color32::from_black_alpha(160)),
                    );
                    // Movement arrow tick, heading clockwise from north.
                    if let Some(h) = sp.heading {
                        let r = h.to_radians();
                        let dir = egui::vec2(r.sin(), -r.cos());
                        painter.line_segment([p, p + dir * 8.0], egui::Stroke::new(1.5, color));
                    }
                    if show_labels {
                        painter.text(
                            p + egui::vec2(5.0, -5.0),
                            egui::Align2::LEFT_BOTTOM,
                            &sp.name,
                            egui::FontId::proportional(10.0),
                            color,
                        );
                    }
                    let hit = egui::Rect::from_center_size(p, egui::vec2(14.0, 14.0));
                    if response.hover_pos().is_some_and(|hp| hit.contains(hp)) {
                        let hover = format!(
                            "{}\n{}\n{}",
                            sp.name,
                            crate::timefmt::fmt_date_clock(sp.time, self.active_tz()),
                            sp.status
                        );
                        response.clone().show_tooltip_text(hover);
                    }
                    if response.clicked()
                        && response
                            .interact_pointer_pos()
                            .is_some_and(|hp| hit.contains(hp))
                    {
                        spotter_click = Some(sp.clone());
                    }
                }
                // Handed back, not stored: `render_pane` still holds the pane's view.
                return spotter_click;
            }
        }
        None
    }

    /// How old each radar's latest scan is, stamped at the site.
    pub(crate) fn paint_scan_age(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        idx: usize,
    ) {
        if self.show_scan_age {
            if let Some(Some(ring)) = self.scan_age_rings.get(&idx) {
                let to_screen = |p: [f64; 2]| {
                    let w = crate::render::mercator::lonlat_to_world(p[0], p[1]);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    egui::pos2(prect.left() + sx, prect.top() + sy)
                };
                let n = ring.wedges.len();
                for (i, age) in ring.wedges.iter().enumerate() {
                    let Some(age) = age else { continue };
                    let a0 = i as f64 / n as f64 * 360.0;
                    let a1 = (i + 1) as f64 / n as f64 * 360.0;
                    let pts: Vec<egui::Pos2> = [a0, (a0 + a1) / 2.0, a1]
                        .into_iter()
                        .map(|az| {
                            to_screen(crate::geo::destination_point(
                                ring.origin,
                                az,
                                ring.radius_km,
                            ))
                        })
                        .collect();
                    painter.add(egui::Shape::line(
                        pts,
                        egui::Stroke::new(5.0, scan_age_color(*age)),
                    ));
                }
                if cam.zoom >= 4.0 {
                    let top = to_screen(crate::geo::destination_point(
                        ring.origin,
                        0.0,
                        ring.radius_km,
                    ));
                    let mut text = format!(
                        "Sweep spans {}",
                        wxdata::scan_age::format_span(ring.summary.span_ms())
                    );
                    // Only meaningful on a live volume; on an archive replay the wall-clock age
                    // is years and says nothing about the picture.
                    let since = Utc::now().timestamp_millis() - ring.summary.newest_ms;
                    if (0..6 * 3_600_000).contains(&since) {
                        text.push_str(&format!(
                            " · newest {} ago",
                            wxdata::scan_age::format_span(since)
                        ));
                    }
                    if ring.summary.is_partial() {
                        text.push_str(" · partial");
                    }
                    painter.text(
                        top + egui::vec2(0.0, -8.0),
                        egui::Align2::CENTER_BOTTOM,
                        text,
                        egui::FontId::proportional(11.0),
                        egui::Color32::from_gray(225),
                    );
                }
            }
        }
    }
}
