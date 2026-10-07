//! Overlays drawn on a pane from `render_pane`: warning polygons, model contours, METAR
//! station plots, and the stale-scan badge. Moved out of `render_pane` unchanged (ROADMAP_2 §7); each method takes the
//! locals its block read, under the same names.

use super::*;

impl HookEchoApp {
    /// Warning, watch and advisory polygons, with their labels.
    pub(crate) fn paint_alert_polygons(
        &self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        idx: usize,
    ) {
        if self.filters.show_alerts {
            let to_screen = |lon: f64, lat: f64| {
                let w = crate::render::mercator::lonlat_to_world(lon, lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                egui::pos2(prect.left() + sx, prect.top() + sy)
            };
            let mut any_escalated = false;
            // Indices, not strings: the `take(6)` below drew six of them however many marker ×
            // warning pairs were formatted.
            let mut etas: Vec<(f64, usize, usize)> = Vec::new();
            let time = ctx.input(|i| i.time);
            // Viewport-center lon/lat: a polygon with every vertex off-screen can still fill the
            // whole pane (zoomed inside it) — the primary chase case for an escalated warning.
            let (center_lon, center_lat) = {
                let w = cam.screen_to_world((vp.0 * 0.5, vp.1 * 0.5), vp);
                crate::render::mercator::world_to_lonlat(w.0, w.1)
            };
            let features = self.active_alert_features();
            for (fi, f) in features.iter().enumerate() {
                let Some(a) = &f.alert else { continue };
                // Pulsing outline for escalated warnings only — watches can carry PDS wording,
                // but pulsing a state-sized watch polygon would drown the map (and `escalation`
                // uppercases the whole bulletin, too heavy to run for every alert every frame).
                if f.kind == overlay::FeatureKind::Warning && wxdata::alerts::escalation(a) >= 2 {
                    let visible =
                        f.rings.first().is_some_and(|r| {
                            r.iter().any(|p| prect.contains(to_screen(p[0], p[1])))
                        }) || f.contains(center_lon, center_lat);
                    if visible {
                        any_escalated = true;
                        let w = 2.0 + 2.0 * (time * 4.0).sin().abs() as f32;
                        let col = egui::Color32::from_rgb(255, 40, 40);
                        for ring in &f.rings {
                            let pts: Vec<egui::Pos2> =
                                ring.iter().map(|p| to_screen(p[0], p[1])).collect();
                            if pts.len() >= 2 {
                                painter.add(egui::Shape::line(pts, egui::Stroke::new(w, col)));
                            }
                        }
                    }
                }
                // Motion vector + projected path (heading = FROM + 180).
                let Some(m) = &a.motion else { continue };
                let Some(&origin) = m.points.first() else {
                    continue;
                };
                if m.kt < 1.0 {
                    continue;
                }
                let heading = ((m.deg + 180.0) % 360.0) as f64;
                let apex = to_screen(origin[0], origin[1]);
                let col = egui::Color32::from_rgb(255, 235, 90);
                painter.circle_filled(apex, 4.0, col);
                let mut prev = apex;
                for min in [15.0_f64, 30.0, 45.0, 60.0] {
                    let km = m.kt as f64 * 1.852 * (min / 60.0);
                    let tp = crate::geo::destination_point(origin, heading, km);
                    let p = to_screen(tp[0], tp[1]);
                    painter.line_segment([prev, p], egui::Stroke::new(1.5, col));
                    painter.circle_filled(p, 2.5, col);
                    if cam.zoom >= 7.0 {
                        painter.text(
                            p + egui::vec2(5.0, -2.0),
                            egui::Align2::LEFT_CENTER,
                            format!("+{min:.0}m"),
                            egui::FontId::proportional(10.0),
                            col,
                        );
                    }
                    prev = p;
                }
                // ETA to any watched marker along the storm's heading.
                for (mi, mk) in self.settings.markers.iter().enumerate() {
                    if let Some(t) = crate::geo::arrival_eta_min(
                        origin,
                        heading as f32,
                        m.kt,
                        [mk.lon, mk.lat],
                        22.5,
                        90.0,
                    ) {
                        etas.push((t, mi, fi));
                    }
                }
            }
            if idx == self.active && !etas.is_empty() {
                etas.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                let font = egui::FontId::proportional(12.0);
                let mut y = prect.top() + 64.0;
                for (t, mi, fi) in etas.iter().take(6) {
                    let mk = &self.settings.markers[*mi];
                    let event = features[*fi]
                        .alert
                        .as_ref()
                        .map(|a| a.event.as_str())
                        .unwrap_or_default();
                    let line = format!("⚠ {} — {} in {:.0} min", mk.name, event, t);
                    let galley = painter.layout_no_wrap(line, font.clone(), egui::Color32::WHITE);
                    let anchor = egui::pos2(prect.left() + 8.0, y);
                    let bg =
                        egui::Rect::from_min_size(anchor, galley.size() + egui::vec2(10.0, 4.0));
                    painter.rect_filled(
                        bg,
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(150, 30, 30, 210),
                    );
                    // The galley just measured, drawn — `painter.text` would lay the same string
                    // out a second time.
                    let size = galley.size();
                    painter.galley(anchor + egui::vec2(5.0, 2.0), galley, egui::Color32::WHITE);
                    y += size.y + 6.0;
                }
            }
            if any_escalated {
                ctx.request_repaint_after(std::time::Duration::from_millis(60));
            }
        }
    }

    /// Model contours (MSLP, 2 m temp, CAPE, SRH, ...): labelled isolines and their banners.
    pub(crate) fn paint_model_contours(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        idx: usize,
    ) {
        if !self.active_contours.is_empty() {
            let to_screen = |lon: f64, lat: f64| {
                let w = crate::render::mercator::lonlat_to_world(lon, lat);
                let (sx, sy) = cam.world_to_screen(w, vp);
                egui::pos2(prect.left() + sx, prect.top() + sy)
            };
            // This pane's lon/lat bounds, for culling lines by their precomputed bbox BEFORE
            // projecting any points — CAPE/SRH carry thousands of small rings.
            let (vmin_lon, vmin_lat, vmax_lon, vmax_lat) = {
                use crate::render::mercator::world_to_lonlat;
                let (wx0, wy0) = cam.screen_to_world((0.0, 0.0), vp);
                let (wx1, wy1) = cam.screen_to_world((vp.0, vp.1), vp);
                let (lon0, lat0) = world_to_lonlat(wx0, wy0);
                let (lon1, lat1) = world_to_lonlat(wx1, wy1);
                (
                    lon0.min(lon1),
                    lat0.min(lat1),
                    lon0.max(lon1),
                    lat0.max(lat1),
                )
            };
            let mut banner_row = 0;
            for kind in self.active_contours.clone() {
                let Some(entry) = self.contours.get(&kind) else {
                    continue;
                };
                if entry.lines.is_empty() {
                    continue;
                }
                let col = kind.color();
                for line in &entry.lines {
                    let (bx0, by0, bx1, by1) = line.bbox;
                    if bx1 < vmin_lon || bx0 > vmax_lon || by1 < vmin_lat || by0 > vmax_lat {
                        continue; // fully off-view
                    }
                    let pts: Vec<egui::Pos2> = line
                        .pts
                        .iter()
                        .map(|&(lon, lat)| to_screen(lon, lat))
                        .collect();
                    // Label the longest segment's midpoint when the line spans enough pixels.
                    let seg = longest_segment(&pts);
                    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.2, col)));
                    if let Some((a, b)) = seg {
                        if a.distance(b) > 60.0 {
                            let mid = a + (b - a) * 0.5;
                            let txt = match kind.unit(self.settings.temp_unit) {
                                Some(unit) => format!("{:.0}{unit}", line.level),
                                None => format!("{:.0}", line.level),
                            };
                            let font = egui::FontId::proportional(11.0);
                            for dx in [-1.0, 1.0] {
                                for dy in [-1.0, 1.0] {
                                    painter.text(
                                        mid + egui::vec2(dx, dy),
                                        egui::Align2::CENTER_CENTER,
                                        &txt,
                                        font.clone(),
                                        egui::Color32::from_black_alpha(200),
                                    );
                                }
                            }
                            painter.text(mid, egui::Align2::CENTER_CENTER, &txt, font, col);
                        }
                    }
                }
                if idx == self.active {
                    let vt = entry
                        .valid
                        .map(|t| crate::timefmt::fmt_clock(t, self.active_tz(), false))
                        .unwrap_or_default();
                    let text = format!(
                        "HRRR {} contours — valid {vt}",
                        kind.display_label(self.settings.temp_unit)
                    );
                    let font = egui::FontId::proportional(12.0);
                    // Stack this kind's banner below any already drawn this frame rather than
                    // overlapping them.
                    let anchor = egui::pos2(
                        prect.left() + 8.0,
                        prect.top() + 40.0 + banner_row as f32 * 22.0,
                    );
                    banner_row += 1;
                    let galley =
                        painter.layout_no_wrap(text.clone(), font.clone(), egui::Color32::WHITE);
                    let bg =
                        egui::Rect::from_min_size(anchor, galley.size() + egui::vec2(10.0, 4.0));
                    painter.rect_filled(
                        bg,
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(60, 90, 60, 200),
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
    }

    /// METAR station plots, decluttered windiest first.
    pub(crate) fn paint_metar_plots(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        response: &egui::Response,
        // Its own borrow, apart from `self`: `render_pane` holds the pane's view meanwhile.
        labels: &mut crate::labelplace::Placer,
    ) {
        if self.show_metar && cam.zoom >= 6.0 {
            crate::prof_scope!("metar_plots");
            let show_labels = cam.zoom >= 7.0;
            let flt_color = |c: &str| match c {
                "VFR" => egui::Color32::from_rgb(60, 200, 90),
                "MVFR" => egui::Color32::from_rgb(80, 150, 240),
                "IFR" => egui::Color32::from_rgb(230, 60, 60),
                "LIFR" => egui::Color32::from_rgb(220, 60, 200),
                _ => egui::Color32::from_gray(180),
            };
            // Projected and clipped before either sort: everything off screen is skipped by the
            // loop below anyway, and the whole national set was being sorted twice to get there.
            let mut obs: Vec<(egui::Pos2, &wxdata::metar::SurfaceOb)> = self
                .metars
                .iter()
                .filter_map(|ob| {
                    let w = crate::render::mercator::lonlat_to_world(ob.lon, ob.lat);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                    prect.contains(p).then_some((p, ob))
                })
                .collect();
            // Windiest-first so the strongest stations survive decluttering.
            obs.sort_by(|(_, a), (_, b)| {
                b.wspd_kt
                    .partial_cmp(&a.wspd_kt)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let temp_unit = self.settings.temp_unit;
            // Same stickiness rule as the place names: a station already plotted keeps its cell
            // ahead of a windier one that has only just come into view.
            obs.sort_by_key(|(_, ob)| !labels.was_shown(crate::labelplace::key(&ob.icao)));
            for (p, ob) in obs {
                // Shared declutter: this also sees the place names drawn above it.
                let cell = egui::Rect::from_center_size(p, egui::vec2(44.0, 34.0));
                if !labels.place(
                    crate::labelplace::key(&ob.icao),
                    cell,
                    crate::labelplace::Priority::Station,
                ) {
                    continue;
                }
                let col = flt_color(&ob.flt_cat);
                painter.circle_stroke(p, 3.0, egui::Stroke::new(1.5, col));
                // Wind barb, rotated so the shaft points toward the wind source (FROM bearing).
                if let Some(dir) = ob.wdir_deg {
                    let th = dir.to_radians();
                    let (up, right) = ([th.sin(), -th.cos()], [th.cos(), th.sin()]);
                    let map = |u: [f32; 2]| {
                        p + egui::vec2(
                            (u[0] * right[0] + u[1] * up[0]) * 22.0,
                            (u[0] * right[1] + u[1] * up[1]) * 22.0,
                        )
                    };
                    for (a, b) in wxdata::metar::barb_segments(ob.wspd_kt) {
                        painter.line_segment([map(a), map(b)], egui::Stroke::new(1.3, col));
                    }
                }
                // Temperature (red, upper-left) and dewpoint (green, lower-left) in °F.
                if show_labels {
                    let f = egui::FontId::proportional(11.0);
                    if let Some(t) = ob.temp_c {
                        painter.text(
                            p + egui::vec2(-6.0, -6.0),
                            egui::Align2::RIGHT_BOTTOM,
                            format!("{:.0}", temp_unit.from_c(t)),
                            f.clone(),
                            egui::Color32::from_rgb(240, 90, 90),
                        );
                    }
                    if let Some(d) = ob.dewp_c {
                        painter.text(
                            p + egui::vec2(-6.0, 6.0),
                            egui::Align2::RIGHT_TOP,
                            format!("{:.0}", temp_unit.from_c(d)),
                            f,
                            egui::Color32::from_rgb(90, 220, 120),
                        );
                    }
                    // Sea state, under the plot — buoys only, by construction.
                    if let Some(h) = ob.wvht_ft {
                        let period = ob
                            .dpd_s
                            .map(|s| format!(" {s:.0}s"))
                            .unwrap_or_else(String::new);
                        painter.text(
                            p + egui::vec2(0.0, 9.0),
                            egui::Align2::CENTER_TOP,
                            format!("{h:.1}ft{period}"),
                            egui::FontId::proportional(10.0),
                            egui::Color32::from_rgb(120, 200, 230),
                        );
                    }
                }
                // Hover → the raw METAR text.
                let hit = egui::Rect::from_center_size(p, egui::vec2(16.0, 16.0));
                if response.hover_pos().is_some_and(|hp| hit.contains(hp)) && !ob.raw.is_empty() {
                    // Observation first, then the terminal forecast where the station files one:
                    // what it is doing, then what it is expected to do.
                    let text = match self.tafs.get(&ob.icao) {
                        Some(taf) => format!("{}\n\n{taf}", ob.raw),
                        None => ob.raw.clone(),
                    };
                    response.clone().show_tooltip_text(text);
                }
            }
        }
    }

    /// A pane following live whose newest scan has gone old says so on the map itself (ROADMAP_2
    /// §0.2: no stale scan displayed as current without a visible indication). The app bar and
    /// timeline speak for the active pane only, and streaming mode and the output window hide
    /// both; this covers every pane, in every mode. Fresh panes stay clean.
    pub(crate) fn paint_stale_badge(&self, painter: &egui::Painter, prect: egui::Rect, idx: usize) {
        let view = &self.views[idx];
        let newest = view.timeline.newest().and_then(|id| id.date_time());
        let age = newest.map(|t| (Utc::now() - t).num_seconds());
        let Some((freshness, text)) =
            stale_badge(view.timeline.following, age, self.radar_fresh_secs())
        else {
            return;
        };
        let font = egui::FontId::proportional(12.0);
        let color = freshness.color();
        let galley = painter.layout_no_wrap(text, font, color);
        let at = prect.left_top() + egui::vec2(10.0, 10.0);
        let back = egui::Rect::from_min_size(at, galley.size() + egui::vec2(14.0, 8.0));
        painter.rect_filled(back, 0.0, egui::Color32::from_black_alpha(215));
        painter.rect_stroke(
            back,
            0.0,
            egui::Stroke::new(1.0, color),
            egui::StrokeKind::Inside,
        );
        painter.galley(at + egui::vec2(7.0, 4.0), galley, color);
    }
}

/// The badge a live pane shows for its newest scan's age: Aging from 80% of the stale threshold
/// (as the live scan reads it), Stale past it. `None` when not following live (an archive frame
/// is old by choice) or when the scan is fresh.
pub(crate) fn stale_badge(
    following: bool,
    age_secs: Option<i64>,
    fresh_secs: i64,
) -> Option<(crate::ui::freshness::Freshness, String)> {
    use crate::ui::freshness::Freshness;
    let age = age_secs.filter(|_| following)?;
    let class = if age >= fresh_secs {
        Freshness::Stale
    } else if age >= fresh_secs * 4 / 5 {
        Freshness::Aging
    } else {
        return None;
    };
    let old = if age < 3600 {
        format!("{}m", age / 60)
    } else {
        format!("{}h {:02}m", age / 3600, (age % 3600) / 60)
    };
    let word = if class == Freshness::Stale {
        "Stale"
    } else {
        "Aging"
    };
    Some((class, format!("{word} \u{b7} newest scan {old} old")))
}

#[cfg(test)]
mod stale_badge_tests {
    use super::stale_badge;
    use crate::ui::freshness::Freshness;

    #[test]
    fn a_live_pane_marks_an_old_scan_and_an_archive_frame_does_not() {
        let fresh = 900;
        assert_eq!(stale_badge(true, Some(300), fresh), None);
        let (class, text) = stale_badge(true, Some(760), fresh).unwrap();
        assert_eq!(class, Freshness::Aging);
        assert_eq!(text, "Aging \u{b7} newest scan 12m old");
        let (class, text) = stale_badge(true, Some(3_900), fresh).unwrap();
        assert_eq!(class, Freshness::Stale);
        assert_eq!(text, "Stale \u{b7} newest scan 1h 05m old");
        // An archive frame is old on purpose; no scan at all is the loading caption's business.
        assert_eq!(stale_badge(false, Some(3_900), fresh), None);
        assert_eq!(stale_badge(true, None, fresh), None);
    }
}
