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

    /// Points of an imported GIS file.
    pub(crate) fn paint_imported_marks(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        imported_shown: bool,
    ) {
        if imported_shown && !self.imported_marks.is_empty() {
            let style = self.settings.imported_gis_style;
            let c = style.stroke_rgba();
            let layer_color = egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
            // A mark coloured by attribute keeps the layer's opacity.
            let colors = self.imported_colors.as_ref().map(|(_, c, _)| c);
            let color_of = |src: Option<&usize>| {
                colors
                    .and_then(|c| *c.get(*src?)?)
                    .map_or(layer_color, |[r, g, b]| {
                        egui::Color32::from_rgba_unmultiplied(r, g, b, c[3])
                    })
            };
            let width = style.rendered_stroke_width();
            let screen = |ll: &[f64; 2]| {
                let w = crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
                let (sx, sy) = cam.world_to_screen(w, vp);
                egui::pos2(prect.left() + sx, prect.top() + sy)
            };
            let marks = &self.imported_marks;
            for (i, line) in marks.lines.iter().enumerate() {
                if !self.imported_valid(marks.line_src.get(i)) {
                    continue;
                }
                let pts: Vec<egui::Pos2> = line.iter().map(screen).collect();
                let color = color_of(marks.line_src.get(i));
                painter.add(egui::Shape::line(pts, egui::Stroke::new(width, color)));
            }
            for (i, point) in marks.points.iter().enumerate() {
                if !self.imported_valid(marks.point_src.get(i)) {
                    continue;
                }
                let p = screen(point);
                if !prect.contains(p) {
                    continue;
                }
                let color = color_of(marks.point_src.get(i));
                // Outlined rather than a plain dot: an imported site has to stay visible over both
                // a bright radar core and a dark basemap, which one flat color cannot manage.
                // The outline-width control also scales point symbols so a mixed-geometry file
                // keeps one coherent visual weight. The default 1.6 px remains the old 3.5 px dot.
                let radius = 2.5 + width * 0.625;
                painter.circle_filled(p, radius, color);
                painter.circle_stroke(
                    p,
                    radius,
                    egui::Stroke::new(1.0, egui::Color32::from_black_alpha(180)),
                );
            }
        }
    }

    /// Labels of an imported GIS file, thinned to one per grid cell.
    pub(crate) fn paint_imported_labels(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        imported_shown: bool,
    ) {
        if let Some(key) = self
            .settings
            .imported_gis_label
            .as_deref()
            .filter(|_| imported_shown)
        {
            let c = self.settings.imported_gis_style.stroke_rgba();
            let text_color = egui::Color32::from_rgb(
                c[0].saturating_add(90),
                c[1].saturating_add(90),
                c[2].saturating_add(90),
            );
            let font = egui::FontId::proportional(11.5);
            let (cell_w, cell_h) = (90.0_f32, 18.0_f32);
            let mut taken = std::collections::HashSet::new();
            let mut drawn = 0;
            for &(at, src) in &self.imported_marks.anchors {
                if !self.imported_valid(Some(&src)) {
                    continue;
                }
                if drawn >= 600 {
                    break;
                }
                let w = crate::render::mercator::lonlat_to_world(at[0], at[1]);
                let (sx, sy) = cam.world_to_screen(w, vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.shrink(4.0).contains(p) {
                    continue;
                }
                let cell = ((p.x / cell_w) as i32, (p.y / cell_h) as i32);
                if !taken.insert(cell) {
                    continue;
                }
                let Some(text) = self
                    .imported_marks
                    .props
                    .get(src)
                    .and_then(|props| crate::gis_import::label_text(props, key))
                else {
                    continue;
                };
                let galley = painter.layout_no_wrap(text, font.clone(), text_color);
                // Beside a point's dot, centred on a line or polygon's anchor; a dark halo keeps
                // it legible over radar and basemap alike.
                let pos = p + egui::vec2(6.0, -galley.size().y * 0.5);
                for d in [
                    egui::vec2(-1.0, 0.0),
                    egui::vec2(1.0, 0.0),
                    egui::vec2(0.0, -1.0),
                    egui::vec2(0.0, 1.0),
                ] {
                    painter.galley_with_override_text_color(
                        pos + d,
                        galley.clone(),
                        egui::Color32::from_black_alpha(200),
                    );
                }
                painter.galley(pos, galley, text_color);
                drawn += 1;
            }
        }
    }
}
