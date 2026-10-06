//! Reference marks a pane draws: your saved locations, live weather stations, range rings, and
//! the points and labels of an imported GIS file. Moved out of `render_pane` unchanged
//! (ROADMAP_2 §7); each method takes the locals its block read, under the same names.

use super::*;

impl HookEchoApp {
    /// Live stations, coloured warm to cool by temperature.
    pub(crate) fn paint_live_stations(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
    ) {
        if self.show_stations {
            let show_labels = cam.zoom >= 8.0;
            let temp_unit = self.settings.temp_unit;
            for ob in &self.stations.obs {
                let w = crate::render::mercator::lonlat_to_world(ob.lon, ob.lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(p) {
                    continue;
                }
                let col = match ob.temp_c {
                    // Blue at freezing through red at 38 C, the span US surface weather lives in.
                    Some(t) => {
                        let f = ((t / 38.0).clamp(0.0, 1.0) * 255.0) as u8;
                        egui::Color32::from_rgb(f, 90, 255 - f)
                    }
                    None => egui::Color32::from_gray(150),
                };
                // A personal station usually sits within a mile of the airport METAR that already
                // has a dot here, so the networks get different shapes and opposite label sides —
                // otherwise the PWS is drawn, invisible, underneath the METAR.
                let stroke = egui::Stroke::new(1.0, egui::Color32::from_black_alpha(180));
                let metar = ob.network == wxdata::stations::Network::Metar;
                if metar {
                    painter.circle_filled(p, 5.0, col);
                    painter.circle_stroke(p, 5.0, stroke);
                } else {
                    let r = egui::Rect::from_center_size(p, egui::vec2(9.0, 9.0));
                    painter.rect_filled(r, 1.0, col);
                    painter.rect_stroke(r, 1.0, stroke, egui::StrokeKind::Middle);
                }
                if show_labels {
                    let label = match ob.temp_c {
                        Some(t) => format!("{:.0}{}", temp_unit.from_c(t), temp_unit.label()),
                        None => ob.id.clone(),
                    };
                    let (off, align) = if metar {
                        (7.0, egui::Align2::LEFT_CENTER)
                    } else {
                        (-7.0, egui::Align2::RIGHT_CENTER)
                    };
                    painter.text(
                        p + egui::vec2(off, 0.0),
                        align,
                        label,
                        egui::FontId::proportional(10.0),
                        egui::Color32::from_gray(230),
                    );
                }
            }
        }
    }

    /// Range rings around the pane's radar.
    pub(crate) fn paint_range_rings(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        idx: usize,
    ) {
        let view = &self.views[idx];
        if self.show_range_rings {
            if let Some(site) = view.site.as_deref().and_then(wxdata::sites::site_by_id) {
                let origin = [site.longitude as f64, site.latitude as f64];
                let col = egui::Color32::from_gray(150).gamma_multiply(0.55);
                let to_screen = |lon: f64, lat: f64| {
                    let w = crate::render::mercator::lonlat_to_world(lon, lat);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    egui::pos2(prect.left() + sx, prect.top() + sy)
                };
                let metric = self.metric_in(idx);
                let ring_values: [f64; 4] = if metric {
                    [50.0, 100.0, 150.0, 200.0]
                } else {
                    [25.0, 50.0, 75.0, 100.0]
                };
                let mut max_ring_km = 0.0f64;
                for value in ring_values {
                    let km = if metric {
                        value
                    } else {
                        value * crate::geo::KM_PER_MILE
                    };
                    max_ring_km = max_ring_km.max(km);
                    let pts: Vec<egui::Pos2> = (0..=72)
                        .map(|i| {
                            let p = crate::geo::destination_point(origin, i as f64 * 5.0, km);
                            to_screen(p[0], p[1])
                        })
                        .collect();
                    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.0, col)));
                    if cam.zoom >= 6.0 {
                        let top = crate::geo::destination_point(origin, 0.0, km);
                        painter.text(
                            to_screen(top[0], top[1]),
                            egui::Align2::CENTER_BOTTOM,
                            crate::geo::fmt_distance(km, metric, 0),
                            egui::FontId::proportional(10.0),
                            col,
                        );
                    }
                }
                for az in (0..360).step_by(45) {
                    let far = crate::geo::destination_point(origin, az as f64, max_ring_km);
                    painter.line_segment(
                        [to_screen(origin[0], origin[1]), to_screen(far[0], far[1])],
                        egui::Stroke::new(0.6, col.gamma_multiply(0.7)),
                    );
                }
            }
        }
    }

    /// Your saved location markers.
    pub(crate) fn paint_location_markers(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
    ) {
        for m in &self.settings.markers {
            let w = crate::render::mercator::lonlat_to_world(m.lon, m.lat);
            let (sx, sy) = cam.world_to_screen(w, vp);
            let p = egui::pos2(prect.left() + sx, prect.top() + sy);
            if !prect.contains(p) {
                continue;
            }
            let col = crate::theme::accent(self.settings.theme);
            // Home wears its watch radius: the ring is the ground truth for "within 20 miles",
            // and a circle you can see beats a number you have to trust.
            if m.home && m.alert_radius_mi > 0.0 {
                let km = m.alert_radius_mi * crate::geo::KM_PER_MILE;
                let edge = crate::geo::destination_point([m.lon, m.lat], 90.0, km);
                let ew = crate::render::mercator::lonlat_to_world(edge[0], edge[1]);
                let (ex, _) = cam.world_to_screen(ew, vp);
                let r = (prect.left() + ex - p.x).abs();
                if r > 4.0 && r < 4000.0 {
                    painter.circle_stroke(
                        p,
                        r,
                        egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 70),
                        ),
                    );
                }
            }
            // Uploaded icon if one is loaded; otherwise the default accent dot.
            let tex = m
                .icon
                .as_ref()
                .and_then(|n| self.marker_icon_tex.get(n))
                .and_then(|t| t.as_ref());
            let label_dx = if let Some(tex) = tex {
                // Round the icon into a disc with a white ring, so a marker reads as a map pin
                // rather than a photo pasted on the map. A corner radius of half the size is a
                // circle; the ring also separates a dark photo from a dark basemap.
                let d = crate::ui::marker_window::ICON_D;
                let r = egui::Rect::from_center_size(p, egui::vec2(d, d));
                painter.add(
                    egui::epaint::RectShape::filled(
                        r,
                        egui::CornerRadius::same((d / 2.0) as u8),
                        egui::Color32::WHITE,
                    )
                    .with_texture(
                        tex.id(),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    ),
                );
                painter.circle_stroke(
                    p,
                    d / 2.0,
                    egui::Stroke::new(1.5, egui::Color32::from_white_alpha(230)),
                );
                d / 2.0 + 2.0
            } else {
                painter.circle_filled(p, 4.0, col);
                painter.circle_stroke(p, 4.0, egui::Stroke::new(1.5, egui::Color32::WHITE));
                7.0
            };
            painter.text(
                p + egui::vec2(label_dx, 0.0),
                egui::Align2::LEFT_CENTER,
                &m.name,
                egui::FontId::proportional(12.0),
                col,
            );
        }
    }
}
