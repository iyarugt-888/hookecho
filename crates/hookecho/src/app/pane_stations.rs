//! Fixed stations a pane draws, placed with the shared label placer: river gauges and radar
//! sites. Moved out of `render_pane` unchanged (ROADMAP_2 §7); each method takes the locals its
//! block read, under the same names, and borrows the placer rather than reaching for
//! `self.labels`, since `render_pane` holds the pane's view meanwhile.

use super::*;

impl HookEchoApp {
    /// River gauges, coloured by forecast flood category, with hover cards.
    pub(crate) fn paint_gauges(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        response: &egui::Response,
        labels: &mut crate::labelplace::Placer,
    ) {
        if self.show_gauges && cam.zoom >= 6.0 {
            let gcolor = crate::ui::gauge_card::cat_color;
            let glabel = crate::ui::gauge_card::cat_label;
            // Already-drawn gauges get their slot back before a newcomer takes it, the same way
            // the METAR and place-name layers already do. Without it a gauge at the edge of a
            // collision wins and loses on alternate frames, which reads as flicker while panning.
            //
            // Two passes — returning labels, then the rest — rather than sorting into a vector
            // that would be allocated and thrown away on every frame.
            for returning in [true, false] {
                crate::prof_scope!("river_gauges");
                for g in &self.gauges {
                    if labels.was_shown(crate::labelplace::key(&g.lid)) != returning {
                        continue;
                    }
                    let w = crate::render::mercator::lonlat_to_world(g.lon, g.lat);
                    let (sx, sy) = cam.world_to_screen(w, vp);
                    let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                    if !prect.contains(p) {
                        continue;
                    }
                    // Shared declutter: gauges are the lowest tier, so they fill what is left.
                    let cell = egui::Rect::from_center_size(p, egui::vec2(15.0, 15.0));
                    if !labels.place(
                        crate::labelplace::key(&g.lid),
                        cell,
                        crate::labelplace::Priority::Minor,
                    ) {
                        continue;
                    }
                    let s = 6.0;
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            p + egui::vec2(-s * 0.85, -s * 0.6),
                            p + egui::vec2(s * 0.85, -s * 0.6),
                            p + egui::vec2(0.0, s),
                        ],
                        gcolor(g.cat).gamma_multiply(0.85),
                        egui::Stroke::new(1.2, egui::Color32::from_gray(20)),
                    ));
                    if g.forecast_ft.is_some() && g.forecast_cat.severity() < g.cat.severity() {
                        painter.circle_stroke(
                            p + egui::vec2(0.0, -0.5),
                            s + 3.0,
                            egui::Stroke::new(1.8, gcolor(g.forecast_cat)),
                        );
                    }
                    if self.gauge_cards.is_open(&g.lid) {
                        painter.circle_stroke(
                            p + egui::vec2(0.0, -0.5),
                            s + 5.5,
                            egui::Stroke::new(1.5, egui::Color32::WHITE),
                        );
                    }
                    let hit = egui::Rect::from_center_size(p, egui::vec2(16.0, 16.0));
                    if response.hover_pos().is_some_and(|hp| hit.contains(hp)) {
                        let stage = g
                            .stage_ft
                            .map_or_else(|| "n/a".to_string(), |v| format!("{v:.1} ft"));
                        let mut tip =
                            format!("{} ({})\n{stage} — {}", g.name, g.lid, glabel(g.cat));
                        if let Some(f) = g.forecast_ft {
                            tip.push_str(&format!(
                                "\nFcst: {f:.1} ft ({})",
                                glabel(g.forecast_cat)
                            ));
                        }
                        if self.tool == MapTool::Interrogate {
                            tip.push_str("\nClick for the hydrograph and crests");
                        }
                        response.clone().show_tooltip_text(tip);
                    }
                }
            }
        }
    }

    /// Radar site markers and their ids.
    pub(crate) fn paint_radar_sites(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        idx: usize,
        labels: &mut crate::labelplace::Placer,
    ) {
        if self.show_radar_sites {
            let accent = crate::theme::accent(self.settings.theme);
            let current = self.views[idx].site.as_deref();
            // Sticky, for the same reason as the gauges above: a site id that wins and loses the
            // same collision on alternate frames is the flicker, not the collision.
            for returning in [true, false] {
                let show_labels = cam.zoom >= 5.0;
                for (s, w) in sites_in_world() {
                    if labels.was_shown(crate::labelplace::key(s.id)) != returning {
                        continue;
                    }
                    let (sx, sy) = cam.world_to_screen(*w, vp);
                    let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                    if !prect.contains(p) {
                        continue;
                    }
                    let is_current = current == Some(s.id);
                    let col = if is_current {
                        accent
                    } else {
                        egui::Color32::from_rgb(120, 190, 255)
                    };
                    let r = if is_current { 5.0 } else { 3.5 };
                    painter.circle_stroke(p, r, egui::Stroke::new(1.5, col));
                    painter.circle_filled(p, 1.5, col);
                    // The dot always draws — it is the click target, and it is small enough not to
                    // matter. Only the four-letter id competes for space, and it loses to city names:
                    // "TDAL" sitting across "Grapevine" is the exact overlap this pass exists for.
                    let id_rect = egui::Rect::from_min_size(
                        p + egui::vec2(6.0, -6.0),
                        egui::vec2(s.id.len() as f32 * 6.5, 12.0),
                    )
                    .expand(1.0);
                    if show_labels
                        && labels.place(
                            crate::labelplace::key(s.id),
                            id_rect,
                            crate::labelplace::Priority::Minor,
                        )
                    {
                        painter.text(
                            p + egui::vec2(6.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            s.id,
                            egui::FontId::monospace(10.0),
                            col,
                        );
                    }
                }
            }
        }
    }
}
