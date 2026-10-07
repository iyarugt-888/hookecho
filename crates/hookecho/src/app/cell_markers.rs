//! Storm-cell drawing on a pane: arrival-time cones and the nowcast ghost, and the cell tracks
//! (locally computed, then SCIT's own) with the cell dots. Moved out of `render_pane` unchanged
//! (ROADMAP_2 §7); the locals they read are their parameters, under the same names.

use super::*;

impl HookEchoApp {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_cones_and_nowcast(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        idx: usize,
        cells_here: bool,
        nowcast_pts: &[(f64, f64, egui::Color32)],
    ) {
        let to_screen = |lon: f64, lat: f64| {
            let w = crate::render::mercator::lonlat_to_world(lon, lat);
            let (sx, sy) = cam.world_to_screen(w, vp);
            egui::pos2(prect.left() + sx, prect.top() + sy)
        };
        // Arrival-time cones: project each moving cell forward, shade the swept path, and
        // list ETAs to any watched marker the cone covers.
        if cells_here && self.filters.show_arrival_cones {
            const LEAD_MIN: f64 = 60.0;
            const HALF_ANGLE: f64 = 18.0;
            // Indices, not strings: every marker inside every cone used to be formatted and
            // then thrown away by the `take(6)` below.
            let mut etas: Vec<(f64, usize, usize)> = Vec::new();
            let cells = self.active_storm_cells();
            for (ci, c) in cells.iter().enumerate() {
                let (Some(dir), Some(kt)) = (c.mvt_deg, c.mvt_kt) else {
                    continue;
                };
                if kt <= 1.0 {
                    continue;
                }
                let lead_km = kt as f64 * 1.852 * (LEAD_MIN / 60.0);
                let left =
                    crate::geo::destination_point([c.lon, c.lat], dir as f64 - HALF_ANGLE, lead_km);
                let right =
                    crate::geo::destination_point([c.lon, c.lat], dir as f64 + HALF_ANGLE, lead_km);
                let apex = to_screen(c.lon, c.lat);
                let lp = to_screen(left[0], left[1]);
                let rp = to_screen(right[0], right[1]);
                let col = cell_color(c.kind);
                let fill = egui::Color32::from_rgba_unmultiplied(col[0], col[1], col[2], 40);
                painter.add(egui::Shape::convex_polygon(
                    vec![apex, lp, rp],
                    fill,
                    egui::Stroke::NONE,
                ));
                // Center line toward the projected 60-min position.
                let tip = crate::geo::destination_point([c.lon, c.lat], dir as f64, lead_km);
                painter.line_segment(
                    [apex, to_screen(tip[0], tip[1])],
                    egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_unmultiplied(col[0], col[1], col[2], 160),
                    ),
                );
                // ETA to each watched marker inside this cone.
                for (mi, m) in self.settings.markers.iter().enumerate() {
                    if let Some(min) = crate::geo::arrival_eta_min(
                        [c.lon, c.lat],
                        dir,
                        kt,
                        [m.lon, m.lat],
                        HALF_ANGLE,
                        LEAD_MIN,
                    ) {
                        etas.push((min, mi, ci));
                    }
                }
            }
            if idx == self.active && !etas.is_empty() {
                etas.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                let font = egui::FontId::proportional(12.0);
                let mut y = prect.top() + 40.0;
                for (min, mi, ci) in etas.iter().take(6) {
                    let (m, c) = (&self.settings.markers[*mi], &cells[*ci]);
                    let text = format!("⏱ {} — {} in {:.0} min", m.name, c.id, min);
                    let galley = painter.layout_no_wrap(text, font.clone(), egui::Color32::WHITE);
                    let anchor = egui::pos2(prect.left() + 8.0, y);
                    let size = galley.size();
                    let bg = egui::Rect::from_min_size(anchor, size + egui::vec2(10.0, 4.0));
                    painter.rect_filled(
                        bg,
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(150, 30, 30, 210),
                    );
                    // The galley just measured, drawn — one layout, not two.
                    painter.galley(anchor + egui::vec2(5.0, 2.0), galley, egui::Color32::WHITE);
                    y += size.y + 6.0;
                }
            }
        }

        // Optical-flow nowcast: advected echo ghost + a lead-time banner.
        if !nowcast_pts.is_empty() {
            // The dots also grow with lead: a longer extrapolation is a blurrier claim
            // about where the echo will be, and a bigger, softer dot reads that way.
            let lead = self.filters.nowcast_lead_min;
            let radius = 2.5 + (1.0 - nowcast_confidence(lead)) * 3.0;
            for (lon, lat, col) in nowcast_pts {
                let p = to_screen(*lon, *lat);
                if prect.contains(p) {
                    painter.circle_filled(p, radius, *col);
                }
            }
            if idx == self.active {
                let text = if lead > 45 {
                    format!(
                        "\u{25c8} NOWCAST +{lead} min \u{2014} extrapolation only; try a \
                         model's reflectivity forecast for an hour or more"
                    )
                } else {
                    format!(
                        "\u{25c8} NOWCAST +{lead} min \u{2014} echo extrapolated from storm motion"
                    )
                };
                let font = egui::FontId::proportional(12.0);
                let anchor = egui::pos2(prect.left() + 8.0, prect.top() + 20.0);
                let galley =
                    painter.layout_no_wrap(text.clone(), font.clone(), egui::Color32::WHITE);
                let bg = egui::Rect::from_min_size(anchor, galley.size() + egui::vec2(10.0, 4.0));
                painter.rect_filled(
                    bg,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(60, 60, 150, 200),
                );
                painter.text(
                    anchor + egui::vec2(5.0, 2.0),
                    egui::Align2::LEFT_TOP,
                    &text,
                    font,
                    egui::Color32::WHITE,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_cell_tracks(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        idx: usize,
        cells_here: bool,
        local_tracks: &[wxdata::celltrack::Track],
        cell_labels_shown: &std::collections::HashSet<String>,
    ) {
        let view = &self.views[idx];
        let to_screen = |lon: f64, lat: f64| {
            let w = crate::render::mercator::lonlat_to_world(lon, lat);
            let (sx, sy) = cam.world_to_screen(w, vp);
            egui::pos2(prect.left() + sx, prect.top() + sy)
        };
        // Locally-computed cell tracks, in cyan so they never read as the Level 3 storm-cell
        // table drawn below in white and red. Same grammar: past polyline, dots, `T+NNm` labels.
        if !local_tracks.is_empty() {
            let cyan = egui::Color32::from_rgb(80, 220, 255);
            for tr in local_tracks {
                let pts: Vec<egui::Pos2> = tr
                    .points
                    .iter()
                    .map(|&(lon, lat, _)| to_screen(lon, lat))
                    .collect();
                painter.add(egui::Shape::line(
                    pts.clone(),
                    egui::Stroke::new(1.5, cyan.gamma_multiply(0.6)),
                ));
                let Some(&head) = pts.last() else { continue };
                if !prect.contains(head) {
                    continue;
                }
                painter.circle_stroke(head, 6.0, egui::Stroke::new(2.0, cyan));
                for minutes in [15.0, 30.0] {
                    let Some((lon, lat)) = tr.extrapolate(minutes) else {
                        continue;
                    };
                    let p = to_screen(lon, lat);
                    painter
                        .line_segment([head, p], egui::Stroke::new(1.0, cyan.gamma_multiply(0.5)));
                    painter.circle_filled(p, 3.0, cyan);
                    if cam.zoom >= 7.0 {
                        painter.text(
                            p + egui::vec2(5.0, -2.0),
                            egui::Align2::LEFT_CENTER,
                            format!("T+{minutes:.0}m"),
                            egui::FontId::proportional(10.0),
                            cyan,
                        );
                    }
                }
                // With a GPS fix, the number a chaser actually wants: how close this cell comes
                // to where they are standing, and in how many minutes.
                if let Some((lon, lat)) = self.chase_pos {
                    let last = tr.points[tr.points.len() - 1];
                    let (km, min) = crate::geo::closest_approach(
                        [last.0, last.1],
                        tr.dir_deg,
                        tr.speed_kt,
                        [lon, lat],
                        60.0,
                    );
                    if min >= 1.0 {
                        painter.text(
                            head + egui::vec2(0.0, 9.0),
                            egui::Align2::CENTER_TOP,
                            format!("\u{2248}{min:.0} min / {km:.0} km"),
                            egui::FontId::proportional(10.0),
                            cyan,
                        );
                    }
                }
            }
        }

        if cells_here {
            let label_tracks = self.filters.show_tracks && cam.zoom >= 7.0;
            // Forecast-time labels already drawn. Neighbouring cells' tracks run side by side, and
            // their times stacked into one unreadable pile. A label that would land on another, or
            // run off the pane's edge, is left off; its tick still marks the time.
            let mut eta_rects: Vec<egui::Rect> = Vec::new();
            for c in self.active_storm_cells() {
                let p = to_screen(c.lon, c.lat);
                // Past track (packet 23): faint gray polyline leading up to the current position.
                if self.filters.show_tracks && c.past_track.len() >= 2 {
                    let gray = egui::Color32::from_gray(150).gamma_multiply(0.7);
                    let pts: Vec<egui::Pos2> = c
                        .past_track
                        .iter()
                        .map(|&(lon, lat)| to_screen(lon, lat))
                        .collect();
                    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, gray)));
                }
                // SCIT positions retain their geometry; cross-ticks mark each forecast time.
                if self.filters.show_tracks && !c.track.is_empty() {
                    let white = egui::Color32::WHITE;
                    let mut prev = p;
                    for tp in &c.track {
                        let tpp = to_screen(tp.lon, tp.lat);
                        let direction = (tpp - prev).normalized();
                        let tick = egui::vec2(-direction.y, direction.x) * 12.0;
                        for (width, color) in [(4.0, egui::Color32::BLACK), (2.0, white)] {
                            painter.line_segment([prev, tpp], egui::Stroke::new(width, color));
                            if direction.length_sq() > 0.0 {
                                painter.line_segment(
                                    [tpp - tick, tpp + tick],
                                    egui::Stroke::new(width, color),
                                );
                            }
                        }
                        if label_tracks {
                            let txt = ui::cell_window::track_time(
                                c.time,
                                tp.minutes,
                                self.settings.tz_for(view.site.as_deref()),
                            );
                            let lp = tpp + egui::vec2(6.0, -16.0);
                            let size = painter
                                .layout_no_wrap(
                                    txt.clone(),
                                    egui::FontId::proportional(14.0),
                                    white,
                                )
                                .size();
                            let rect =
                                egui::Rect::from_min_size(lp - egui::vec2(0.0, size.y), size)
                                    .expand(2.0);
                            if !prect.contains_rect(rect)
                                || eta_rects.iter().any(|r| r.intersects(rect))
                            {
                                prev = tpp;
                                continue;
                            }
                            eta_rects.push(rect);
                            for off in [egui::vec2(1.0, 1.0), egui::vec2(-1.0, -1.0)] {
                                painter.text(
                                    lp + off,
                                    egui::Align2::LEFT_BOTTOM,
                                    &txt,
                                    egui::FontId::proportional(14.0),
                                    egui::Color32::BLACK,
                                );
                            }
                            painter.text(
                                lp,
                                egui::Align2::LEFT_BOTTOM,
                                &txt,
                                egui::FontId::proportional(14.0),
                                white,
                            );
                        }
                        prev = tpp;
                    }
                }
                if !prect.contains(p) {
                    continue;
                }
                let col = cell_color(c.kind);
                let color = egui::Color32::from_rgba_unmultiplied(col[0], col[1], col[2], 255);
                let marker_color = if c.kind == CellKind::Storm {
                    egui::Color32::WHITE
                } else {
                    color
                };
                painter.circle_filled(p, 7.0, egui::Color32::BLACK);
                painter.circle_stroke(p, 6.0, egui::Stroke::new(2.0, marker_color));
                painter.circle_filled(p, 2.0, marker_color);
                if c.kind == CellKind::Storm && cell_labels_shown.contains(&c.id) {
                    painter.text(
                        p + egui::vec2(8.0, -8.0),
                        egui::Align2::LEFT_BOTTOM,
                        &c.id,
                        egui::FontId::proportional(11.0),
                        color,
                    );
                }
            }
        }
    }
}
