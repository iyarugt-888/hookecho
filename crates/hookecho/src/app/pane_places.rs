//! Place names over a pane's map: city and town labels from the vector tiles, drawn with a halo
//! and placed through the shared label placer so they give way to warnings and cells. Moved out
//! of `render_pane` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// City and town labels, biggest places first.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_place_labels(
        &self,
        painter: &egui::Painter,
        prect: egui::Rect,
        cam: crate::render::mercator::Camera,
        vp: (f32, f32),
        is_vector: bool,
        basemap: crate::tiles::BasemapStyle,
        vlabels: &[crate::vector_tiles::PlaceLabel],
        placer: &mut crate::labelplace::Placer,
    ) {
        if !vlabels.is_empty() {
            let (text_col, halo_col, big) = if is_vector {
                let st = crate::basemap_style::style(basemap.vector_palette().unwrap_or_default());
                (
                    egui::Color32::from_rgb(st.label[0], st.label[1], st.label[2]),
                    egui::Color32::from_rgb(st.label_halo[0], st.label_halo[1], st.label_halo[2]),
                    13.0,
                )
            } else {
                (
                    egui::Color32::WHITE,
                    egui::Color32::from_black_alpha(235),
                    14.5,
                )
            };
            let z = cam.zoom;
            // Repeat route shields from regional zoom; collision placement still prevents overlap.
            let repeat_shields = z >= 5.0;
            let mut labels: Vec<&crate::vector_tiles::PlaceLabel> =
                vlabels.iter().filter(|l| l.visible_at(z)).collect();
            let label_key = |l: &crate::vector_tiles::PlaceLabel| {
                let key = crate::labelplace::key(&l.name);
                if repeat_shields && l.shield != crate::vector_tiles::RoadShield::None {
                    key ^ ((l.world[0].to_bits() as u64) << 32) ^ l.world[1].to_bits() as u64
                } else {
                    key
                }
            };
            // Labels already on screen are offered their slot before newcomers of the same
            // importance; without that a name at the edge of a collision wins and loses on
            // alternate frames, which is exactly the flicker you see while panning.
            labels.sort_by_key(|l| (l.priority(), !placer.was_shown(label_key(l)), l.rank));
            let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
            // 8-way halo (cardinals + diagonals) for a solid, readable outline.
            const HALO: [egui::Vec2; 8] = [
                egui::vec2(1.2, 0.0),
                egui::vec2(-1.2, 0.0),
                egui::vec2(0.0, 1.2),
                egui::vec2(0.0, -1.2),
                egui::vec2(1.0, 1.0),
                egui::vec2(1.0, -1.0),
                egui::vec2(-1.0, 1.0),
                egui::vec2(-1.0, -1.0),
            ];
            let mut placed_shields: Vec<(&str, crate::vector_tiles::RoadShield, egui::Pos2)> =
                Vec::new();
            for l in labels {
                if (l.shield == crate::vector_tiles::RoadShield::None || !repeat_shields)
                    && !seen.insert(l.name.as_str())
                {
                    continue;
                }
                let (sx, sy) = cam.world_to_screen((l.world[0] as f64, l.world[1] as f64), vp);
                let p = egui::pos2(prect.left() + sx, prect.top() + sy);
                if !prect.contains(p) {
                    continue;
                }
                if l.shield != crate::vector_tiles::RoadShield::None {
                    use crate::vector_tiles::RoadShield;
                    let (height, pad, text_color) = match l.shield {
                        RoadShield::Interstate => (25.0, 10.0, egui::Color32::WHITE),
                        RoadShield::Us => (22.0, 11.0, egui::Color32::BLACK),
                        RoadShield::State => (19.0, 9.0, egui::Color32::BLACK),
                        RoadShield::Other => (17.0, 7.0, egui::Color32::BLACK),
                        RoadShield::None => unreachable!(),
                    };
                    let galley = painter.layout_no_wrap(
                        l.name.clone(),
                        egui::FontId::proportional(big - 2.5),
                        text_color,
                    );
                    let r = egui::Rect::from_center_size(
                        p,
                        egui::vec2((galley.size().x + pad).max(height), height),
                    );
                    if placed_shields.iter().any(|(name, shield, position)| {
                        *name == l.name
                            && *shield == l.shield
                            && position.distance(p) < if z < 8.0 { 160.0 } else { 220.0 }
                    }) {
                        continue;
                    }
                    if !placer.place(
                        label_key(l),
                        r.expand(3.0),
                        crate::labelplace::Priority::Place,
                    ) {
                        continue;
                    }
                    placed_shields.push((&l.name, l.shield, p));
                    match l.shield {
                        RoadShield::Interstate => {
                            let shield = |rect: egui::Rect| {
                                vec![
                                    egui::pos2(rect.left() + 3.0, rect.top() + 2.0),
                                    egui::pos2(rect.center().x, rect.top()),
                                    egui::pos2(rect.right() - 3.0, rect.top() + 2.0),
                                    egui::pos2(rect.right(), rect.top() + 7.0),
                                    egui::pos2(rect.right() - 1.0, rect.bottom() - 7.0),
                                    egui::pos2(rect.right() - 4.0, rect.bottom() - 3.0),
                                    egui::pos2(rect.center().x, rect.bottom()),
                                    egui::pos2(rect.left() + 4.0, rect.bottom() - 3.0),
                                    egui::pos2(rect.left() + 1.0, rect.bottom() - 7.0),
                                    egui::pos2(rect.left(), rect.top() + 7.0),
                                ]
                            };
                            painter.add(egui::Shape::convex_polygon(
                                shield(r),
                                egui::Color32::WHITE,
                                egui::Stroke::NONE,
                            ));
                            let inner = r.shrink(1.2);
                            painter.add(egui::Shape::convex_polygon(
                                shield(inner),
                                egui::Color32::from_rgb(38, 67, 145),
                                egui::Stroke::NONE,
                            ));
                            painter.add(egui::Shape::convex_polygon(
                                vec![
                                    egui::pos2(inner.left() + 1.0, inner.top() + 6.5),
                                    egui::pos2(inner.left() + 3.0, inner.top() + 2.0),
                                    egui::pos2(inner.center().x, inner.top()),
                                    egui::pos2(inner.right() - 3.0, inner.top() + 2.0),
                                    egui::pos2(inner.right() - 1.0, inner.top() + 6.5),
                                ],
                                egui::Color32::from_rgb(190, 37, 48),
                                egui::Stroke::NONE,
                            ));
                            painter.line_segment(
                                [
                                    egui::pos2(inner.left() + 1.0, inner.top() + 7.0),
                                    egui::pos2(inner.right() - 1.0, inner.top() + 7.0),
                                ],
                                egui::Stroke::new(1.2, egui::Color32::WHITE),
                            );
                        }
                        RoadShield::Us => {
                            let badge = |rect: egui::Rect| {
                                vec![
                                    egui::pos2(rect.left() + 4.0, rect.top()),
                                    egui::pos2(rect.right() - 4.0, rect.top()),
                                    egui::pos2(rect.right(), rect.top() + 5.0),
                                    egui::pos2(rect.right() - 2.0, rect.bottom() - 4.0),
                                    egui::pos2(rect.center().x, rect.bottom()),
                                    egui::pos2(rect.left() + 2.0, rect.bottom() - 4.0),
                                    egui::pos2(rect.left(), rect.top() + 5.0),
                                ]
                            };
                            painter.add(egui::Shape::convex_polygon(
                                badge(r),
                                egui::Color32::BLACK,
                                egui::Stroke::NONE,
                            ));
                            painter.add(egui::Shape::convex_polygon(
                                badge(r.shrink(1.3)),
                                egui::Color32::WHITE,
                                egui::Stroke::NONE,
                            ));
                        }
                        RoadShield::State => {
                            painter.rect_filled(r, height * 0.5, egui::Color32::BLACK);
                            painter.rect_filled(r.shrink(1.2), height * 0.5, egui::Color32::WHITE);
                        }
                        RoadShield::Other => {
                            painter.rect_filled(r, 2.0, egui::Color32::BLACK);
                            painter.rect_filled(r.shrink(1.0), 1.5, egui::Color32::WHITE);
                        }
                        RoadShield::None => unreachable!(),
                    }
                    painter.galley_with_override_text_color(
                        egui::pos2(
                            r.center().x - galley.size().x * 0.5,
                            r.center().y - galley.size().y * 0.5
                                + if l.shield == RoadShield::Interstate {
                                    2.8
                                } else {
                                    0.0
                                },
                        ),
                        galley,
                        text_color,
                    );
                    continue;
                }
                let font = egui::FontId::proportional(if l.city { big } else { big - 2.5 });
                let galley = painter.layout_no_wrap(l.name.clone(), font, text_col);
                let r = egui::Rect::from_min_size(p, galley.size()).expand(4.0);
                if !placer.place(label_key(l), r, crate::labelplace::Priority::Place) {
                    continue;
                }
                // One layout per label, reused for all nine draws. `painter.text` would lay the
                // string out again every time, which at eight halo offsets meant ten text
                // layouts per visible place name, every frame.
                for off in HALO {
                    painter.galley_with_override_text_color(p + off, galley.clone(), halo_col);
                }
                painter.galley_with_override_text_color(p, galley, text_col);
            }
            // OpenMapTiles/OpenStreetMap credit for the label data (raster imagery is credited below).
            painter.text(
                egui::pos2(prect.left() + 6.0, prect.bottom() - 18.0),
                egui::Align2::LEFT_BOTTOM,
                "© OpenMapTiles © OpenStreetMap",
                egui::FontId::proportional(10.0),
                egui::Color32::from_gray(200).gamma_multiply(0.55),
            );
        }
    }
}
