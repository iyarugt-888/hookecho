//! Feed layers a pane draws as marks: pilot reports, ProbSevere objects, placefile labels and
//! planned routes. Moved out of `render_pane` unchanged (ROADMAP_2 §7); each method takes the
//! locals its block read, under the same names.

use super::*;

impl HookEchoApp {
    /// Pilot reports (turbulence, icing), with hover cards.
    pub(crate) fn paint_pireps(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        response: &egui::Response,
    ) {
        if self.show_pireps {
            for r in &self.pireps {
                let w = crate::render::mercator::lonlat_to_world(r.lon, r.lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(p) {
                    continue;
                }
                let col = if r.urgent {
                    egui::Color32::from_rgb(235, 70, 70)
                } else if r.hazard.is_empty() {
                    egui::Color32::from_rgb(150, 165, 185)
                } else {
                    egui::Color32::from_rgb(240, 190, 50)
                };
                let d = 5.0;
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        p + egui::vec2(0.0, -d),
                        p + egui::vec2(d, d * 0.8),
                        p + egui::vec2(-d, d * 0.8),
                    ],
                    col,
                    egui::Stroke::new(1.0, egui::Color32::from_black_alpha(160)),
                ));
                // Hover → altitude, aircraft and the raw report, which is what pilots read.
                let hit = egui::Rect::from_center_size(p, egui::vec2(16.0, 16.0));
                if response.hover_pos().is_some_and(|hp| hit.contains(hp)) {
                    let alt = r
                        .alt_ft
                        .map_or_else(|| "—".to_string(), |a| format!("{a} ft"));
                    response.clone().show_tooltip_text(format!(
                        "{alt}  {}\n{}\n{}",
                        r.ac_type, r.hazard, r.raw
                    ));
                }
            }
        }
    }

    /// ProbSevere objects, ringed by probability.
    pub(crate) fn paint_probsevere(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
    ) {
        if self.show_probsevere {
            for f in &self.probsevere {
                let Some(ring) = f.rings.first() else {
                    continue;
                };
                if ring.is_empty() {
                    continue;
                }
                let (mut clon, mut clat) = (0.0, 0.0);
                for p in ring {
                    clon += p[0];
                    clat += p[1];
                }
                let cw = crate::render::mercator::lonlat_to_world(
                    clon / ring.len() as f64,
                    clat / ring.len() as f64,
                );
                let (sx, sy) = cam.world_to_screen(cw, vp);
                let c = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(c) {
                    continue;
                }
                let color = egui::Color32::from_rgb(f.stroke[0], f.stroke[1], f.stroke[2]);
                let font = egui::FontId::proportional(11.0);
                let galley =
                    painter.layout_no_wrap(f.title.clone(), font.clone(), egui::Color32::BLACK);
                let rect = egui::Rect::from_center_size(c, galley.size() + egui::vec2(8.0, 4.0));
                painter.rect_filled(rect, 3.0, color);
                painter.text(
                    c,
                    egui::Align2::CENTER_CENTER,
                    &f.title,
                    font,
                    egui::Color32::BLACK,
                );
            }
        }
    }

    /// Text items from loaded placefiles, with hover text.
    pub(crate) fn paint_placefile_labels(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        response: &egui::Response,
        placefile_labels: &[PlaceLabel],
    ) {
        for label in placefile_labels {
            // An anchored label is placed by projecting its object's anchor and then stepping the
            // stated pixels from it (y up), so it holds its offset as the map zooms.
            let (base, off) = match label.anchor {
                Some(a) => (a, egui::vec2(label.pos[0] as f32, -label.pos[1] as f32)),
                None => (label.pos, egui::Vec2::ZERO),
            };
            let w = crate::render::mercator::lonlat_to_world(base[0], base[1]);
            let (sx, sy) = cam.world_to_screen(w, vp);
            let p = egui::pos2(prect.left() + sx, prect.top() + sy) + off;
            if !prect.contains(p) {
                continue;
            }
            let mut hit_size = egui::vec2(16.0, 16.0);
            match &label.kind {
                PlaceLabelKind::Text(text) => {
                    painter.text(
                        p,
                        egui::Align2::CENTER_CENTER,
                        text,
                        egui::FontId::proportional(12.0),
                        label.color,
                    );
                }
                PlaceLabelKind::Marker => {
                    painter.circle_stroke(p, 5.0, egui::Stroke::new(1.5, label.color));
                    painter.circle_filled(p, 1.5, label.color);
                }
                PlaceLabelKind::Sprite {
                    tex,
                    uv,
                    size,
                    hot,
                    angle,
                } => {
                    draw_sprite(painter, *tex, *uv, p, *size, *hot, *angle, label.color);
                    hit_size = *size;
                }
            }
            if !label.hover.is_empty() {
                let hit = egui::Rect::from_center_size(p, hit_size);
                if response.hover_pos().is_some_and(|hp| hit.contains(hp)) {
                    response.clone().show_tooltip_text(&label.hover);
                }
            }
        }
    }

    /// Planned routes and their waypoints.
    pub(crate) fn paint_routes(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
    ) {
        if !self.route_window.routes.is_empty() || !self.route_window.waypoints.is_empty() {
            let screen = |p: &[f64; 2]| {
                let w = crate::render::mercator::lonlat_to_world(p[0], p[1]);
                let (sx, sy) = cam.world_to_screen(w, vp);
                egui::pos2(prect.left() + sx, prect.top() + sy)
            };
            let chosen = self.route_window.selected;
            for (i, r) in self.route_window.routes.iter().enumerate() {
                if i == chosen {
                    continue;
                }
                let pts: Vec<egui::Pos2> = r.coords.iter().map(screen).collect();
                painter.add(egui::Shape::line(
                    pts,
                    egui::Stroke::new(
                        3.0,
                        egui::Color32::from_rgba_unmultiplied(170, 170, 185, 170),
                    ),
                ));
            }
            if let Some(r) = self.route_window.routes.get(chosen) {
                let pts: Vec<egui::Pos2> = r.coords.iter().map(screen).collect();
                painter.add(egui::Shape::line(
                    pts.clone(),
                    egui::Stroke::new(7.0, egui::Color32::from_black_alpha(170)),
                ));
                painter.add(egui::Shape::line(
                    pts,
                    egui::Stroke::new(4.0, egui::Color32::from_rgb(70, 150, 255)),
                ));
            }
            for (i, p) in self.route_window.waypoints.iter().enumerate() {
                let at = screen(p);
                painter.circle(
                    at,
                    8.0,
                    egui::Color32::from_rgb(70, 150, 255),
                    egui::Stroke::new(1.5, egui::Color32::WHITE),
                );
                painter.text(
                    at,
                    egui::Align2::CENTER_CENTER,
                    ((b'A' + (i as u8).min(25)) as char).to_string(),
                    egui::FontId::proportional(10.0),
                    egui::Color32::WHITE,
                );
            }
        }
    }
}
