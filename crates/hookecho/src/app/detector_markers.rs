//! The detector markers on a pane: debris signatures, hail spikes, ZDR columns and the melting
//! layer, rotation couplets, Tornado ID's triangles and the one-per-tornado detections with the web
//! they open into. Moved out of `render_pane` unchanged (ROADMAP_2 §7): the locals it read arrive
//! in [`Markers`] under the same names, so the drawing code itself did not change.

use super::*;

/// What `render_pane` had worked out for this pane's markers.
pub(crate) struct Markers<'a> {
    pub painter: &'a egui::Painter,
    pub prect: egui::Rect,
    pub cam: crate::render::mercator::Camera,
    pub vp: (f32, f32),
    pub response: &'a egui::Response,
    pub tds_hits: &'a [wxdata::tds::TdsHit],
    pub tbss_hits: &'a [wxdata::dualpol::TbssHit],
    pub zdr_hits: &'a [wxdata::dualpol::ZdrColumnHit],
    pub couplets: &'a [wxdata::rotation::CoupletHit],
    pub tornado_ids: &'a [wxdata::tornado_id::TornadoId],
    pub circulations: &'a [wxdata::tornado_id::Circulation],
    /// Where the Tornado ID verdicts came from: pipeline, versions, volume and input scan times.
    pub tornado_lineage: Option<&'a wxdata::detection_lineage::DetectionLineage>,
    pub tied_tds: &'a [bool],
    pub tied_couplet: &'a [bool],
    pub all_couplets: &'a [wxdata::rotation::CoupletHit],
    pub all_tds: &'a [wxdata::tds::TdsHit],
    pub tds_score_tracks: &'a [wxdata::scoretrack::ScoreTrack],
    pub rot_score_tracks: &'a [wxdata::scoretrack::ScoreTrack],
    /// The experimental LLSD pipeline's columns (empty unless `detectors.llsd_preview`).
    pub llsd: &'a [wxdata::llsd_analyst::Analysed],
}

impl HookEchoApp {
    pub(crate) fn paint_detector_markers(&self, ui: &egui::Ui, m: Markers<'_>, idx: usize) {
        let Markers {
            painter,
            prect,
            cam,
            vp,
            response,
            tds_hits,
            tbss_hits,
            zdr_hits,
            couplets,
            tornado_ids,
            circulations,
            tornado_lineage,
            tied_tds,
            tied_couplet,
            all_couplets,
            all_tds,
            tds_score_tracks,
            rot_score_tracks,
            llsd,
        } = m;
        let to_screen = |lon: f64, lat: f64| {
            let w = crate::render::mercator::lonlat_to_world(lon, lat);
            let (sx, sy) = cam.world_to_screen(w, vp);
            egui::pos2(prect.left() + sx, prect.top() + sy)
        };

        // The experimental LLSD columns (analyst preview): a cyan ring at each column's base, its
        // headline under it, and on hover everything that made it. Drawn first, so the regular
        // detectors' markers sit on top. The strongest dozen only: weak columns are many.
        let cyan = egui::Color32::from_rgb(40, 210, 230);
        for a in llsd.iter().take(12) {
            let c = &a.tracked.column;
            let p = to_screen(c.lon, c.lat);
            if !prect.contains(p) {
                continue;
            }
            let ring = if c.sense == wxdata::rotation::Sense::Cyclonic {
                egui::Stroke::new(2.0, cyan)
            } else {
                egui::Stroke::new(1.0, cyan)
            };
            painter.circle_stroke(p, 10.0, ring);
            painter.text(
                p + egui::vec2(0.0, 12.0),
                egui::Align2::CENTER_TOP,
                a.headline(),
                egui::FontId::proportional(10.5),
                cyan,
            );
            let hit = egui::Rect::from_center_size(p, egui::vec2(24.0, 24.0));
            if response.hover_pos().is_some_and(|hp| hit.contains(hp)) {
                response.clone().show_tooltip_ui(|ui| {
                    ui.set_max_width(520.0);
                    ui.strong(a.headline());
                    for line in a.lines() {
                        ui.label(egui::RichText::new(line).small());
                    }
                });
            }
        }

        // TDS markers: a magenta inverted triangle + label at each debris-signature cluster.
        for (i, h) in tds_hits.iter().enumerate() {
            let p = to_screen(h.lon, h.lat);
            if !prect.contains(p) || tied_tds.get(i).copied().unwrap_or(false) {
                continue;
            }
            let m = egui::Color32::from_rgb(240, 40, 210);
            let s = 8.0;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    p + egui::vec2(-s, -s),
                    p + egui::vec2(s, -s),
                    p + egui::vec2(0.0, s),
                ],
                egui::Color32::from_rgba_unmultiplied(240, 40, 210, 60),
                egui::Stroke::new(2.0, m),
            ));
            // Rotation beside the signature, when there is some: the corroboration that
            // separates a debris ball from hail.
            let rot = h
                .rotation_ms
                .map_or(String::new(), |v| format!(" · rot {:.0}kt", v * 1.943_844));
            // Confirmed by people, above the radar score: a gold ring and the badge.
            let badge = h
                .confirmation
                .level()
                .map_or(String::new(), |l| format!(" · {}", l.label()));
            if h.confirmation.level().is_some() {
                painter.circle_stroke(p, 15.0, egui::Stroke::new(2.5, CONFIRMED_GOLD));
            }
            painter.text(
                p + egui::vec2(0.0, -s - 2.0),
                egui::Align2::CENTER_BOTTOM,
                // Height only earns its place on the label once there's more than one tilt of
                // it to report — a bare "0.5 km" off a single low tilt is just its range, not
                // evidence of anything lofted.
                if h.tilts > 1 {
                    format!(
                        "TDS ρ{:.2} · {}t {:.1}km{} · {}{rot}{badge}",
                        h.min_cc,
                        h.tilts,
                        h.top_km,
                        // Only the exception is labelled, so the ordinary case stays short.
                        if h.rooted == Some(false) {
                            " aloft"
                        } else {
                            ""
                        },
                        wxdata::evidence::out_of_100(h.confidence)
                    )
                } else {
                    format!(
                        "TDS ρ{:.2} · {}{rot}{badge}",
                        h.min_cc,
                        wxdata::evidence::out_of_100(h.confidence)
                    )
                },
                egui::FontId::proportional(11.0),
                m,
            );
            // Hover for the working: every term, its measurement and what each stage added,
            // plus a sparkline of how the score got here if this signature has been seen
            // before.
            let hit = egui::Rect::from_center_size(p, egui::vec2(26.0, 26.0));
            if response.hover_pos().is_some_and(|hp| hit.contains(hp)) {
                let lines = h.explain().lines(h);
                let track = nearest_score_track(tds_score_tracks, h.lon, h.lat);
                response
                    .clone()
                    .show_tooltip_ui(|ui| score_tooltip(ui, lines, track, m));
            }
        }

        // Hail spikes: a hollow triangle at the core the spike points away from. Yellow, not
        // magenta — this is a hail flag, and nothing here should read like a debris signature.
        for h in tbss_hits {
            let p = to_screen(h.lon, h.lat);
            if !prect.contains(p) {
                continue;
            }
            let col = egui::Color32::from_rgb(250, 210, 60);
            let s = 7.0;
            painter.add(egui::Shape::closed_line(
                vec![
                    p + egui::vec2(0.0, -s),
                    p + egui::vec2(s, s),
                    p + egui::vec2(-s, s),
                ],
                egui::Stroke::new(2.0, col),
            ));
            painter.text(
                p + egui::vec2(0.0, -s - 2.0),
                egui::Align2::CENTER_BOTTOM,
                format!("TBSS {:.0} dBZ", h.core_dbz),
                egui::FontId::proportional(11.0),
                col,
            );
        }

        // ZDR columns: an upward arrow with the depth above the freezing level.
        for h in zdr_hits {
            let p = to_screen(h.lon, h.lat);
            if !prect.contains(p) {
                continue;
            }
            let col = egui::Color32::from_rgb(120, 230, 160);
            let s = 8.0;
            painter.line_segment(
                [p + egui::vec2(0.0, s), p + egui::vec2(0.0, -s)],
                egui::Stroke::new(2.0, col),
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    p + egui::vec2(0.0, -s - 4.0),
                    p + egui::vec2(-4.0, -s + 1.0),
                    p + egui::vec2(4.0, -s + 1.0),
                ],
                col,
                egui::Stroke::NONE,
            ));
            painter.text(
                p + egui::vec2(6.0, 0.0),
                egui::Align2::LEFT_CENTER,
                format!("ZDR +{:.1} km", h.depth_km),
                egui::FontId::proportional(11.0),
                col,
            );
        }

        // Where the melting layer is, read off the same volume's CC. One line, because the
        // number is the whole product: it tells you which "heavy rain" is a bright band.
        if self.filters.show_zdr_columns && idx == self.active {
            if let Some((_, _, Some(bb))) = &self.zdr_cache {
                let text = format!("melting layer ~{:.1} km (bright band)", bb.height_km);
                let font = egui::FontId::proportional(12.0);
                painter.text(
                    egui::pos2(prect.left() + 8.0, prect.bottom() - 8.0),
                    egui::Align2::LEFT_BOTTOM,
                    text,
                    font,
                    egui::Color32::from_rgb(160, 200, 230),
                );
            }
        }

        // Rotation couplets: a ring at each cluster — solid red at strong-TVS strength
        // (≥36 m/s rotational velocity), hollow orange below it.
        for (i, h) in couplets.iter().enumerate() {
            let p = to_screen(h.lon, h.lat);
            if !prect.contains(p) || tied_couplet.get(i).copied().unwrap_or(false) {
                continue;
            }
            let strong = h.vrot_ms >= 36.0;
            let col = if strong {
                egui::Color32::from_rgb(240, 60, 60)
            } else {
                egui::Color32::from_rgb(245, 160, 50)
            };
            painter.circle_stroke(p, 11.0, egui::Stroke::new(2.0, col));
            let badge = h
                .confirmation
                .level()
                .map_or(String::new(), |l| format!(" · {}", l.label()));
            if h.confirmation.level().is_some() {
                painter.circle_stroke(p, 16.0, egui::Stroke::new(2.5, CONFIRMED_GOLD));
            }
            if strong {
                painter.circle_filled(p, 3.5, col);
            }
            painter.text(
                p + egui::vec2(0.0, 13.0),
                egui::Align2::CENTER_TOP,
                // Same reasoning as the TDS label: height/tilt-count only means something once
                // there's more than one tilt behind it.
                {
                    // Only the exceptions are labelled, so the ordinary cyclonic, ground-
                    // rooted couplet reads exactly as short as it did before.
                    let anti = if h.sense == wxdata::rotation::Sense::Anticyclonic {
                        " · anticyc"
                    } else {
                        ""
                    };
                    if h.tilts > 1 {
                        format!(
                            "ROT {:.0} kt · {}t {:.1}km{} · {}{anti}{badge}",
                            h.vrot_ms * 1.943_844,
                            h.tilts,
                            h.top_km,
                            if h.rooted == Some(false) {
                                " aloft"
                            } else {
                                ""
                            },
                            wxdata::evidence::out_of_100(h.confidence)
                        )
                    } else {
                        format!(
                            "ROT {:.0} kt · {}{anti}{badge}",
                            h.vrot_ms * 1.943_844,
                            wxdata::evidence::out_of_100(h.confidence)
                        )
                    }
                },
                egui::FontId::proportional(11.0),
                col,
            );
            // Hover for the working, as on the debris signatures.
            let hit = egui::Rect::from_center_size(p, egui::vec2(26.0, 26.0));
            if response.hover_pos().is_some_and(|hp| hit.contains(hp)) {
                let lines = h.explain().lines(h);
                let track = nearest_score_track(rot_score_tracks, h.lon, h.lat);
                response
                    .clone()
                    .show_tooltip_ui(|ui| score_tooltip(ui, lines, track, col));
            }
        }

        // Tornado ID: a warning triangle per identification, coloured by tier, the reasons on
        // hover. Drawn over the couplet and debris markers it was made from.
        for t in tornado_ids {
            use wxdata::tornado_id::Tier;
            let p = to_screen(t.lon, t.lat);
            if !prect.contains(p) {
                continue;
            }
            let col = match t.tier {
                Tier::Possible => egui::Color32::from_rgb(245, 210, 60),
                Tier::Likely => egui::Color32::from_rgb(245, 110, 40),
                Tier::Debris => egui::Color32::from_rgb(225, 70, 225),
                Tier::Confirmed => CONFIRMED_GOLD,
            };
            let tri = vec![
                p + egui::vec2(0.0, -13.0),
                p + egui::vec2(11.5, 7.0),
                p + egui::vec2(-11.5, 7.0),
            ];
            painter.add(egui::Shape::convex_polygon(
                tri,
                col,
                egui::Stroke::new(1.5, egui::Color32::BLACK),
            ));
            painter.text(
                p + egui::vec2(0.0, 1.0),
                egui::Align2::CENTER_CENTER,
                t.tier.glyph(),
                egui::FontId::proportional(12.0),
                egui::Color32::BLACK,
            );
            painter.text(
                p + egui::vec2(0.0, 10.0),
                egui::Align2::CENTER_TOP,
                format!(
                    "{} \u{b7} {}",
                    t.tier.label(),
                    wxdata::evidence::out_of_100(t.score)
                ),
                egui::FontId::proportional(11.5),
                col,
            );
            let hit = egui::Rect::from_center_size(p, egui::vec2(28.0, 28.0));
            if response.hover_pos().is_some_and(|hp| hit.contains(hp)) {
                response.clone().show_tooltip_ui(|ui| {
                    ui.strong(t.tier.label());
                    for r in &t.reasons {
                        ui.label(r);
                    }
                    lineage_lines(ui, tornado_lineage);
                });
            }
        }

        // One detection per tornado: the verdict at the most likely rotation. A click or tap
        // opens it into a web — a spoke to every rotation and debris detection it ties
        // together — with a pinned card of the verdict, what it tied in and, for the point
        // picked (on the map or in the card), that detection's factors. Nothing here needs a
        // hover: a finger has none. A mouse still gets the same readings as tooltips.
        if !circulations.is_empty() {
            use wxdata::tornado_id::{Evidence, Tier, MERGE_KM};
            let ctx = ui.ctx().clone();
            let open_id = egui::Id::new(("circulation_open", idx));
            let pick_id = egui::Id::new(("circulation_pick", idx));
            let open: Option<(f64, f64)> = ctx.data(|d| d.get_temp(open_id)).flatten();
            // The picked detection, by where it is: indices change every scan.
            let picked: Option<(f64, f64)> = ctx.data(|d| d.get_temp(pick_id)).flatten();
            let mut set_open: Option<Option<(f64, f64)>> = None;
            let mut set_pick: Option<Option<(f64, f64)>> = None;
            let mut hits = Vec::new();
            let tap = response
                .clicked()
                .then(|| response.interact_pointer_pos())
                .flatten();
            let hover = response.hover_pos();
            let kt = |ms: f32| ms * 1.943_844;
            let near = |a: (f64, f64), b: (f64, f64)| {
                crate::geo::great_circle([a.0, a.1], [b.0, b.1]).0 <= 0.5
            };
            // A detection's glyph colour, one-line label, factor lines and score history.
            let describe = |e: Evidence| match e {
                Evidence::Rotation(i) => {
                    let h = &all_couplets[i];
                    let rc = if h.vrot_ms >= 36.0 {
                        egui::Color32::from_rgb(240, 60, 60)
                    } else {
                        egui::Color32::from_rgb(245, 160, 50)
                    };
                    (
                        rc,
                        format!(
                            "Rotation {:.0} kt \u{b7} {}",
                            kt(h.vrot_ms),
                            wxdata::evidence::out_of_100(h.confidence)
                        ),
                        h.explain().lines(h),
                        nearest_score_track(rot_score_tracks, h.lon, h.lat),
                    )
                }
                Evidence::Debris(i) => {
                    let h = &all_tds[i];
                    (
                        egui::Color32::from_rgb(240, 40, 210),
                        format!(
                            "Debris \u{3c1}{:.2} \u{b7} {}",
                            h.min_cc,
                            wxdata::evidence::out_of_100(h.confidence)
                        ),
                        h.explain().lines(h),
                        nearest_score_track(tds_score_tracks, h.lon, h.lat),
                    )
                }
            };
            for (ci, c) in circulations.iter().enumerate() {
                let t = &c.id;
                let p = to_screen(t.lon, t.lat);
                if !prect.contains(p) {
                    continue;
                }
                let col = match t.tier {
                    Tier::Possible => egui::Color32::from_rgb(245, 210, 60),
                    Tier::Likely => egui::Color32::from_rgb(245, 110, 40),
                    Tier::Debris => egui::Color32::from_rgb(225, 70, 225),
                    Tier::Confirmed => CONFIRMED_GOLD,
                };
                // Open survives the next scan, whose centre sits a little further along.
                let is_open = open.is_some_and(|(lon, lat)| {
                    crate::geo::great_circle([lon, lat], [t.lon, t.lat]).0 <= MERGE_KM
                });
                // A finger is wider than a cursor: the target is too.
                let hit = egui::Rect::from_center_size(p, egui::vec2(36.0, 36.0));
                hits.push(hit);
                let hovered = hover.is_some_and(|hp| hit.contains(hp));
                if tap.is_some_and(|tp| hit.contains(tp)) {
                    set_open = Some((!is_open).then_some((t.lon, t.lat)));
                    set_pick = Some(None);
                }
                let seed = c.members.first().map(|m| m.evidence);
                if is_open || hovered {
                    // How far it reached for what it tied in.
                    let edge = {
                        let e = crate::geo::destination_point([t.lon, t.lat], 90.0, MERGE_KM);
                        to_screen(e[0], e[1])
                    };
                    painter.circle_stroke(
                        p,
                        (edge - p).length(),
                        egui::Stroke::new(1.0, col.gamma_multiply(0.35)),
                    );
                }
                let mut picked_member = None;
                if is_open {
                    for m in &c.members {
                        let centre = Some(m.evidence) == seed;
                        let q = to_screen(m.lon, m.lat);
                        if !centre {
                            painter.line_segment(
                                [p, q],
                                egui::Stroke::new(1.5, col.gamma_multiply(0.75)),
                            );
                        }
                        // The centre's own detection sits under the verdict; its glyph goes
                        // just below it so it can be reached.
                        let q = if centre { p + egui::vec2(0.0, 38.0) } else { q };
                        let (glyph_col, label, lines, track) = describe(m.evidence);
                        let is_picked = picked.is_some_and(|at| near(at, (m.lon, m.lat)));
                        if is_picked {
                            picked_member = Some(*m);
                            painter.circle_filled(q, 11.0, glyph_col.gamma_multiply(0.35));
                        }
                        match m.evidence {
                            Evidence::Rotation(_) => {
                                painter.circle_stroke(q, 7.0, egui::Stroke::new(2.0, glyph_col));
                            }
                            Evidence::Debris(_) => {
                                let s = 6.0;
                                painter.add(egui::Shape::convex_polygon(
                                    vec![
                                        q + egui::vec2(-s, -s),
                                        q + egui::vec2(s, -s),
                                        q + egui::vec2(0.0, s),
                                    ],
                                    glyph_col.gamma_multiply(0.3),
                                    egui::Stroke::new(1.5, glyph_col),
                                ));
                            }
                        }
                        painter.text(
                            q + egui::vec2(10.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            if centre {
                                format!("{label} (centre)")
                            } else {
                                format!("{label} \u{b7} {:.1} km", m.km)
                            },
                            egui::FontId::proportional(10.5),
                            glyph_col,
                        );
                        let mhit = egui::Rect::from_center_size(q, egui::vec2(28.0, 28.0));
                        hits.push(mhit);
                        if tap.is_some_and(|tp| mhit.contains(tp)) {
                            set_pick = Some((!is_picked).then_some((m.lon, m.lat)));
                        }
                        if !hovered && hover.is_some_and(|hp| mhit.contains(hp)) {
                            response
                                .clone()
                                .show_tooltip_ui(|ui| score_tooltip(ui, lines, track, glyph_col));
                        }
                    }
                }
                let tri = vec![
                    p + egui::vec2(0.0, -14.0),
                    p + egui::vec2(12.5, 7.5),
                    p + egui::vec2(-12.5, 7.5),
                ];
                painter.add(egui::Shape::convex_polygon(
                    tri,
                    col,
                    egui::Stroke::new(1.5, egui::Color32::BLACK),
                ));
                painter.text(
                    p + egui::vec2(0.0, 1.0),
                    egui::Align2::CENTER_CENTER,
                    t.tier.glyph(),
                    egui::FontId::proportional(12.0),
                    egui::Color32::BLACK,
                );
                // One line: the verdict, then the strongest numbers behind it.
                let mut label = format!(
                    "{} \u{b7} {}",
                    t.tier.label(),
                    wxdata::evidence::out_of_100(t.score)
                );
                if let Some(v) = t.vrot_ms {
                    label.push_str(&format!(" \u{b7} ROT {:.0} kt", kt(v)));
                }
                if let Some(cc) = t.min_cc {
                    label.push_str(&format!(" \u{b7} \u{3c1}{cc:.2}"));
                }
                if c.members.len() > 1 && !is_open {
                    label.push_str(&format!(" \u{b7} {} signals", c.members.len()));
                }
                painter.text(
                    p + egui::vec2(0.0, 10.0),
                    egui::Align2::CENTER_TOP,
                    label,
                    egui::FontId::proportional(11.5),
                    col,
                );
                if hovered && !is_open {
                    response.clone().show_tooltip_ui(|ui| {
                        ui.strong(format!(
                            "{} \u{b7} {}",
                            t.tier.label(),
                            wxdata::evidence::out_of_100(t.score)
                        ));
                        for r in &t.reasons {
                            ui.label(r);
                        }
                        ui.weak(format!(
                            "{} detections tied together. Click to open the web.",
                            c.members.len()
                        ));
                        lineage_lines(ui, tornado_lineage);
                    });
                }
                if !is_open {
                    continue;
                }
                // The pinned card: beside the marker, on whichever side has room.
                let right = p.x + 330.0 < prect.right();
                egui::Area::new(egui::Id::new(("circulation_card", idx, ci)))
                    .order(egui::Order::Foreground)
                    .pivot(if right {
                        egui::Align2::LEFT_TOP
                    } else {
                        egui::Align2::RIGHT_TOP
                    })
                    .fixed_pos(p + egui::vec2(if right { 24.0 } else { -24.0 }, -16.0))
                    .constrain_to(prect)
                    .show(&ctx, |ui| {
                        crate::ui::style::glass(ui, 240).show(ui, |ui| {
                            // Tall with a factor breakdown open: it scrolls inside the map.
                            egui::ScrollArea::vertical()
                                .max_height((prect.height() - 32.0).max(120.0))
                                .show(ui, |ui| {
                                    ui.set_max_width(300.0);
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{} \u{b7} {}",
                                                t.tier.label(),
                                                wxdata::evidence::out_of_100(t.score)
                                            ))
                                            .strong()
                                            .color(col),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui.small_button("\u{d7}").clicked() {
                                                    set_open = Some(None);
                                                    set_pick = Some(None);
                                                }
                                            },
                                        );
                                    });
                                    for r in &t.reasons {
                                        ui.label(egui::RichText::new(r).size(11.5));
                                    }
                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new("Tied together").weak().size(11.0),
                                    );
                                    for m in &c.members {
                                        let (mc, what, _, _) = describe(m.evidence);
                                        let where_ = if Some(m.evidence) == seed {
                                            "centre".to_string()
                                        } else {
                                            format!("{:.1} km off", m.km)
                                        };
                                        let sel = picked_member
                                            .is_some_and(|pm| pm.evidence == m.evidence);
                                        let row = ui.selectable_label(
                                            sel,
                                            egui::RichText::new(format!("{what} \u{b7} {where_}"))
                                                .color(mc)
                                                .size(11.5),
                                        );
                                        if row.clicked() {
                                            set_pick = Some((!sel).then_some((m.lon, m.lat)));
                                        }
                                    }
                                    match picked_member {
                                        Some(m) => {
                                            let (mc, _, lines, track) = describe(m.evidence);
                                            ui.separator();
                                            score_tooltip(ui, lines, track, mc);
                                        }
                                        None => {
                                            ui.label(
                                        egui::RichText::new(
                                            "Pick a detection, here or on the map, for the \
                                             factors behind it.",
                                        )
                                        .weak()
                                        .size(10.5),
                                    );
                                        }
                                    }
                                    // Where the verdict came from: a finger has no hover, so the
                                    // card is where a phone reads it.
                                    lineage_lines(ui, tornado_lineage);
                                });
                        });
                    });
            }
            if let Some(v) = set_open {
                ctx.data_mut(|d| d.insert_temp(open_id, v));
            }
            if let Some(v) = set_pick {
                ctx.data_mut(|d| d.insert_temp(pick_id, v));
            }
            ctx.data_mut(|d| d.insert_temp(egui::Id::new(("circulation_hits", idx)), hits));
        }
    }
}

/// Where a Tornado ID verdict came from, small and weak under its reasons: which pipeline and
/// version, the volume, and when its inputs were scanned.
fn lineage_lines(ui: &mut egui::Ui, lineage: Option<&wxdata::detection_lineage::DetectionLineage>) {
    let Some(l) = lineage else {
        return;
    };
    ui.add_space(2.0);
    for line in l.lines() {
        ui.label(egui::RichText::new(line).small().weak());
    }
}

#[cfg(test)]
mod llsd_preview_snapshots {
    /// The analyst hover for the experimental LLSD layer, on the real Moore 2013 volume (the
    /// scientific corpus's cached copy), rendered for visual review.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu + cached corpus volume: writes the LLSD analyst hover for visual review"]
    fn gpu_llsd_analyst_hover() {
        use wxdata::level2::{self, Moment};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let bytes = std::fs::read(root.join("scientific-corpus/KTLX20130520_201229_V06.gz"))
            .expect("cached Moore volume (provision the scientific corpus first)");
        let scan = level2::decode_volume(bytes).unwrap();
        let (mut vel_pairs, mut cc_pairs) = (Vec::new(), Vec::new());
        for tilt in 0..level2::elevation_angles(&scan).len() {
            let z = level2::bin_scan(&scan, Moment::Reflectivity, tilt);
            if let (Ok(z), Ok(cc)) = (
                &z,
                level2::bin_scan(&scan, Moment::CorrelationCoefficient, tilt),
            ) {
                cc_pairs.push((z.clone(), cc));
            }
            if let (Ok(z), Ok(v)) = (
                z,
                level2::bin_scan_opts(&scan, Moment::Velocity, tilt, true),
            ) {
                vel_pairs.push((v, z));
            }
            if vel_pairs.len() == 4 {
                break;
            }
        }
        let columns = wxdata::rotation_columns::from_sweeps(&vel_pairs);
        let mut tracker =
            wxdata::rotation_tracks::Tracker::new(wxdata::rotation_tracks::TrackParams::default());
        let tracked = tracker.update(0, columns);
        let debris = wxdata::tds::detect_volume(&cc_pairs, 0.80, 40.0, 150.0, 4);
        let analysed = wxdata::llsd_analyst::analyse(tracked, &debris, &[]);
        let a = analysed.first().expect("a column");
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for UI review");
        let destination = root.join("ui-review");
        std::fs::create_dir_all(&destination).unwrap();
        gpu.save(
            &destination.join("llsd-analyst-hover.png"),
            560,
            520,
            |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(520.0);
                    ui.strong(a.headline());
                    for line in a.lines() {
                        ui.label(egui::RichText::new(line).small());
                    }
                });
            },
        )
        .unwrap();
    }

    /// A Tornado ID hover on the same volume, with where the verdict came from under its reasons:
    /// the fused pipeline's version, the volume, and when its four input tilts were scanned.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu + cached corpus volume: writes a Tornado ID hover with its lineage"]
    fn gpu_tornado_id_lineage_hover() {
        use wxdata::detection_lineage::{
            fused_algorithms, input_coverage, DetectionLineage, Pipeline,
        };
        use wxdata::level2::{self, Moment};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let bytes = std::fs::read(root.join("scientific-corpus/KTLX20130520_201229_V06.gz"))
            .expect("cached Moore volume (provision the scientific corpus first)");
        let scan = level2::decode_volume(bytes).unwrap();
        let (mut vel_pairs, mut cc_pairs) = (Vec::new(), Vec::new());
        for tilt in 0..level2::elevation_angles(&scan).len() {
            let z = level2::bin_scan(&scan, Moment::Reflectivity, tilt);
            if let (Ok(z), Ok(cc)) = (
                &z,
                level2::bin_scan(&scan, Moment::CorrelationCoefficient, tilt),
            ) {
                cc_pairs.push((z.clone(), cc));
            }
            if let (Ok(z), Ok(v)) = (
                z,
                level2::bin_scan_opts(&scan, Moment::Velocity, tilt, true),
            ) {
                vel_pairs.push((v, z));
            }
            if vel_pairs.len() == 4 {
                break;
            }
        }
        let columns = wxdata::rotation_columns::from_sweeps(&vel_pairs);
        let tracked =
            wxdata::rotation_tracks::Tracker::new(wxdata::rotation_tracks::TrackParams::default())
                .update(0, columns);
        let debris = wxdata::tds::detect_volume(&cc_pairs, 0.80, 40.0, 150.0, 4);
        let analysed = wxdata::llsd_analyst::analyse(tracked, &debris, &[]);
        let ids = wxdata::llsd_analyst::identify(&analysed, |_, _| Default::default());
        let t = ids.first().expect("a Tornado ID verdict on Moore");
        let lineage = DetectionLineage {
            pipeline: Pipeline::Fused,
            algorithms: fused_algorithms(),
            site: Some("KTLX".into()),
            volume: "KTLX20130520_201229_V06".into(),
            volume_time: "2013-05-20T20:12:29Z".parse().ok(),
            inputs: input_coverage(vel_pairs.into_iter().flat_map(|(v, z)| [v, z]).collect()),
            stand_in: None,
        };
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for UI review");
        let destination = root.join("parity-review/m1.4");
        std::fs::create_dir_all(&destination).unwrap();
        gpu.save(
            &destination.join("tornado-id-lineage-hover.png"),
            520,
            250,
            |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(480.0);
                    ui.strong(t.tier.label());
                    // The first reasons only: the capture is for the lineage under them.
                    for r in t.reasons.iter().take(2) {
                        ui.label(r);
                    }
                    ui.weak("\u{2026}");
                    super::lineage_lines(ui, Some(&lineage));
                });
            },
        )
        .unwrap();
        std::fs::write(
            destination.join("tornado-id-lineage.json"),
            serde_json::to_string_pretty(&lineage.to_json()).unwrap(),
        )
        .unwrap();
    }
}
