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

    /// Each shown warning, watch and special weather statement's kind in words on its polygon
    /// (`alerts::map_tag`), so it reads without telling colours apart (1010.md V11). Most severe
    /// first; none over another; only on a polygon big enough on screen to hold its tag, at a
    /// point inside it (`tag_anchor`). Each tag reserves its place in the frame's shared `labels`
    /// at warning priority, before city names and every lesser label, so none of those covers it
    /// (1008.md E4).
    pub(crate) fn paint_alert_tags(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        labels: &mut crate::labelplace::Placer,
    ) {
        use wxdata::overlay::FeatureKind;
        let rank = |f: &wxdata::overlay::GeoFeature, a: &wxdata::overlay::AlertInfo| {
            let kind = match (f.kind, a.event.to_ascii_lowercase().as_str()) {
                (FeatureKind::Warning, "tornado warning") => 30,
                (FeatureKind::Warning, _) => 20,
                (FeatureKind::Watch, _) => 10,
                _ => 0,
            };
            kind + u32::from(wxdata::alerts::escalation(a))
        };
        let mut tags: Vec<(u32, &wxdata::overlay::GeoFeature, String, String)> = self
            .overlays
            .iter()
            .filter_map(|f| {
                let a = f.alert.as_ref()?;
                let tag = wxdata::alerts::map_tag(a)?;
                let short = wxdata::alerts::short_tag(a).unwrap_or_else(|| tag.clone());
                Some((rank(f, a), f, tag, short))
            })
            .collect();
        tags.sort_by_key(|t| std::cmp::Reverse(t.0));
        let font = egui::FontId::proportional(11.0);
        for (_, f, tag, short) in tags {
            let key = crate::labelplace::key(&format!(
                "alert-tag:{}:{tag}",
                f.alert.as_ref().map_or("", |a| a.id.as_str())
            ));
            let Some(ring) = f.rings.first() else {
                continue;
            };
            let pts: Vec<egui::Pos2> = ring
                .iter()
                .map(|p| {
                    let (sx, sy) = cam
                        .world_to_screen(crate::render::mercator::lonlat_to_world(p[0], p[1]), vp);
                    egui::pos2(prect.left() + sx, prect.top() + sy)
                })
                .collect();
            // The full tag where it fits, else the short one: a small emergency polygon still
            // says "TOR!!" rather than nothing.
            let fit = |text: String| {
                let galley = painter.layout_no_wrap(text, font.clone(), egui::Color32::WHITE);
                let size = galley.size() + egui::vec2(8.0, 4.0);
                tag_anchor(&pts, prect, size).map(|at| (galley, size, at))
            };
            let Some((galley, size, at)) = fit(tag).or_else(|| fit(short)) else {
                continue;
            };
            let rect = egui::Rect::from_center_size(at, size);
            if !labels.place(key, rect, crate::labelplace::Priority::Warning) {
                continue;
            }
            let edge = egui::Color32::from_rgb(f.stroke[0], f.stroke[1], f.stroke[2]);
            painter.rect_filled(rect, 3.0, egui::Color32::from_black_alpha(210));
            painter.rect_stroke(
                rect,
                3.0,
                egui::Stroke::new(1.5, edge),
                egui::StrokeKind::Inside,
            );
            painter.galley(
                rect.min + egui::vec2(4.0, 2.0),
                galley,
                egui::Color32::WHITE,
            );
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

/// Where a polygon's tag goes, in screen pixels: its area centroid when that is inside it and on
/// the pane, else the middle of its on-screen part when that is; `None` when the polygon is too
/// small on screen to hold `size`, or neither point is inside it (a thin or hooked shape).
pub(crate) fn tag_anchor(
    pts: &[egui::Pos2],
    view: egui::Rect,
    size: egui::Vec2,
) -> Option<egui::Pos2> {
    if pts.len() < 3 {
        return None;
    }
    let bbox = egui::Rect::from_points(pts);
    if bbox.width() < size.x * 1.5 || bbox.height() < size.y * 1.8 {
        return None;
    }
    let inside = |p: egui::Pos2| {
        let mut c = false;
        let mut j = pts.len() - 1;
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[j]);
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                c = !c;
            }
            j = i;
        }
        c
    };
    let (mut area, mut cx, mut cy) = (0.0f32, 0.0f32, 0.0f32);
    for w in pts.windows(2) {
        let cross = w[0].x * w[1].y - w[1].x * w[0].y;
        area += cross;
        cx += (w[0].x + w[1].x) * cross;
        cy += (w[0].y + w[1].y) * cross;
    }
    let centroid = (area.abs() > 1e-3).then(|| egui::pos2(cx / (3.0 * area), cy / (3.0 * area)));
    let room = view.shrink2(size * 0.5);
    let visible = bbox.intersect(view);
    [centroid, visible.is_positive().then(|| visible.center())]
        .into_iter()
        .flatten()
        .find(|&p| room.contains(p) && inside(p))
}

#[cfg(test)]
mod tag_anchor_tests {
    use super::tag_anchor;
    use egui::{pos2, vec2, Rect};

    fn view() -> Rect {
        Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 600.0))
    }

    /// A box gets its tag in the middle; a box too small for the tag gets none; an L-shape whose
    /// centroid falls outside it still gets one inside; a polygon half off the pane puts it in
    /// the visible half.
    #[test]
    fn a_tag_goes_inside_its_polygon_on_the_pane() {
        let tag = vec2(40.0, 16.0);
        let square = [
            pos2(100.0, 100.0),
            pos2(300.0, 100.0),
            pos2(300.0, 300.0),
            pos2(100.0, 300.0),
            pos2(100.0, 100.0),
        ];
        let at = tag_anchor(&square, view(), tag).unwrap();
        assert!(
            (at.x - 200.0).abs() < 0.5 && (at.y - 200.0).abs() < 0.5,
            "{at:?}"
        );
        let tiny = square.map(|p| pos2(100.0 + (p.x - 100.0) * 0.1, 100.0 + (p.y - 100.0) * 0.1));
        assert_eq!(tag_anchor(&tiny, view(), tag), None);
        // A thin L: the centroid sits in the notch, outside; the visible middle is inside.
        let ell = [
            pos2(100.0, 100.0),
            pos2(400.0, 100.0),
            pos2(400.0, 140.0),
            pos2(140.0, 140.0),
            pos2(140.0, 400.0),
            pos2(100.0, 400.0),
            pos2(100.0, 100.0),
        ];
        if let Some(at) = tag_anchor(&ell, view(), tag) {
            assert!(
                (100.0..=400.0).contains(&at.x) && (at.y <= 140.0 || at.x <= 140.0),
                "{at:?}"
            );
        }
        // Half off the pane to the left: the tag stays on the pane.
        let off = square.map(|p| pos2(p.x - 250.0, p.y));
        let at = tag_anchor(&off, view(), tag).unwrap();
        assert!(at.x >= 20.0 && at.x <= 50.0, "{at:?}");
    }
}
