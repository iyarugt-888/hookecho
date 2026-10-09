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
    /// The original pipeline's circulations in Tornado ID's comparison mode (else empty): drawn
    /// hollow under the fused markers, labelled Original, never opening a card or alerting.
    pub original_circulations: &'a [wxdata::tornado_id::Circulation],
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
            original_circulations,
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
        // together — with a card of the verdict, what it tied in and, for the point picked (on
        // the map or in the card), that detection's factors. Nothing here needs a hover: a
        // finger has none. A mouse still gets the same readings as tooltips, and a press-and-hold
        // pins them on a touch screen (`touch_hover`).
        if !circulations.is_empty() {
            use wxdata::tornado_id::{Evidence, MERGE_KM};
            let ctx = ui.ctx().clone();
            let touch = ctx.input(|i| i.has_touch_screen() || i.any_touches());
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
            // No hover tooltip on the frame a tap opens or closes something: on a touch screen it
            // flashed up under the finger and the card then opened over it.
            let hover = response.hover_pos().filter(|_| tap.is_none());
            // A fingertip is wider than a cursor: so are the targets.
            let (marker_hit, member_hit) = if touch { (48.0, 40.0) } else { (36.0, 28.0) };
            let kt = |ms: f32| ms * 1.943_844;
            let near = |a: (f64, f64), b: (f64, f64)| {
                crate::geo::great_circle([a.0, a.1], [b.0, b.1]).0 <= 0.5
            };
            // A detection's glyph colour, one-line label, factor lines, score history and
            // evidence (0..1).
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
                        h.confidence,
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
                        h.confidence,
                    )
                }
            };
            // The HRRR hour the verdicts read (the environment gate), for each card's air.
            let environment = self.views[idx].volume.as_ref().and_then(|v| {
                let valid = near_storm::valid_hour(v.time);
                self.near_storm
                    .source(valid)
                    .map(|(run, hour)| (run, hour, (valid - run.timestamp()) / 3600))
            });
            // Comparison mode: the original Tornado ID's verdicts, under the fused markers. A
            // hollow triangle pointing down, so the two read apart where they sit together, and
            // a label above saying whose verdict it is.
            let mut original_labels: Vec<egui::Rect> = Vec::new();
            for o in original_circulations {
                let t = &o.id;
                let p = to_screen(t.lon, t.lat);
                if !prect.contains(p) {
                    continue;
                }
                let col = tier_colour(t.tier);
                let tri = vec![
                    p + egui::vec2(0.0, 14.0),
                    p + egui::vec2(12.5, -7.5),
                    p + egui::vec2(-12.5, -7.5),
                ];
                painter.add(egui::Shape::convex_polygon(
                    tri,
                    egui::Color32::from_black_alpha(150),
                    egui::Stroke::new(2.0, col),
                ));
                painter.text(
                    p + egui::vec2(0.0, -1.0),
                    egui::Align2::CENTER_CENTER,
                    t.tier.glyph(),
                    egui::FontId::proportional(11.0),
                    col,
                );
                halo_label(
                    painter,
                    &mut original_labels,
                    false,
                    p + egui::vec2(0.0, -10.0),
                    egui::Align2::CENTER_BOTTOM,
                    format!(
                        "Original \u{b7} {} \u{b7} {}",
                        t.tier.label(),
                        wxdata::evidence::out_of_100(t.score)
                    ),
                    egui::FontId::proportional(if touch { 12.5 } else { 11.0 }),
                    col,
                );
                let hit = egui::Rect::from_center_size(p, egui::vec2(marker_hit, marker_hit));
                if hover.is_some_and(|hp| hit.contains(hp)) {
                    response.clone().show_tooltip_ui(|ui| {
                        ui.strong(format!(
                            "Original Tornado ID \u{b7} {} \u{b7} {}",
                            t.tier.label(),
                            wxdata::evidence::out_of_100(t.score)
                        ));
                        for r in &t.reasons {
                            ui.label(r);
                        }
                        ui.weak(
                            "Comparison mode: the original pipeline's verdict, beside the fused \
                             one. It never alerts.",
                        );
                    });
                }
            }
            for (ci, c) in circulations.iter().enumerate() {
                let t = &c.id;
                let p = to_screen(t.lon, t.lat);
                let air = environment
                    .as_ref()
                    .and_then(|(run, hour, lead)| Some((run, hour.sample(t.lon, t.lat)?, lead)));
                if !prect.contains(p) {
                    continue;
                }
                let col = tier_colour(t.tier);
                // Open survives the next scan, whose centre sits a little further along.
                let is_open = open.is_some_and(|(lon, lat)| {
                    crate::geo::great_circle([lon, lat], [t.lon, t.lat]).0 <= MERGE_KM
                });
                let hit = egui::Rect::from_center_size(p, egui::vec2(marker_hit, marker_hit));
                hits.push(hit);
                let hovered = hover.is_some_and(|hp| hit.contains(hp));
                if tap.is_some_and(|tp| hit.contains(tp)) {
                    set_open = Some((!is_open).then_some((t.lon, t.lat)));
                    set_pick = Some(None);
                }
                let seed = c.members.first().map(|m| m.evidence);
                if is_open || hovered {
                    // How far it reached for what it tied in: a faint dashed ring.
                    let edge = {
                        let e = crate::geo::destination_point([t.lon, t.lat], 90.0, MERGE_KM);
                        to_screen(e[0], e[1])
                    };
                    let r = (edge - p).length();
                    let ring: Vec<egui::Pos2> = (0..=72)
                        .map(|k| {
                            let a = k as f32 / 72.0 * std::f32::consts::TAU;
                            p + egui::vec2(a.cos(), a.sin()) * r
                        })
                        .collect();
                    painter.extend(egui::Shape::dashed_line(
                        &ring,
                        egui::Stroke::new(1.0, col.gamma_multiply(0.45)),
                        6.0,
                        5.0,
                    ));
                }
                // One line: the verdict, then the strongest numbers behind it. Measured now so
                // the web's labels keep off it; drawn last, on top.
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
                let verdict_label = painter.layout_no_wrap(
                    label,
                    egui::FontId::proportional(if touch { 13.0 } else { 11.5 }),
                    col,
                );
                let verdict_rect = egui::Align2::CENTER_TOP.anchor_size(
                    p + egui::vec2(0.0, if is_open { 21.0 } else { 10.0 }),
                    verdict_label.size(),
                );
                // Room the web's labels must leave: the verdict's label and its triangle.
                let mut placed = vec![
                    verdict_rect,
                    egui::Rect::from_center_size(p, egui::vec2(40.0, 40.0)),
                ];
                let mut picked_member = None;
                if is_open {
                    // Everything it tied together is listed in the card; on the map only the
                    // picked one is labelled once there are more than four, so the labels do not
                    // pile up on the storm.
                    let label_all = c.members.len() <= 4;
                    // Spokes first, under every glyph: a dark underlay so they read over any
                    // colour of radar, weighted by how strong each detection is.
                    for m in &c.members {
                        if Some(m.evidence) == seed {
                            continue;
                        }
                        let q = to_screen(m.lon, m.lat);
                        let (_, _, _, _, ev) = describe(m.evidence);
                        let w = 1.2 + 2.0 * ev.clamp(0.0, 1.0);
                        painter.line_segment(
                            [p, q],
                            egui::Stroke::new(w + 2.5, egui::Color32::from_black_alpha(150)),
                        );
                        painter.line_segment(
                            [p, q],
                            egui::Stroke::new(
                                w,
                                col.gamma_multiply(0.55 + 0.4 * ev.clamp(0.0, 1.0)),
                            ),
                        );
                    }
                    for m in &c.members {
                        let centre = Some(m.evidence) == seed;
                        // The centre's own detection sits under the verdict; its glyph goes
                        // just below it so it can be reached.
                        let q = if centre {
                            p + egui::vec2(0.0, 40.0)
                        } else {
                            to_screen(m.lon, m.lat)
                        };
                        let (glyph_col, label, lines, track, ev) = describe(m.evidence);
                        let is_picked = picked.is_some_and(|at| near(at, (m.lon, m.lat)));
                        if is_picked {
                            picked_member = Some(*m);
                            painter.circle_filled(q, 13.0, glyph_col.gamma_multiply(0.35));
                            painter.circle_stroke(q, 13.0, egui::Stroke::new(1.5, glyph_col));
                        }
                        let s = 5.5 + 3.0 * ev.clamp(0.0, 1.0);
                        match m.evidence {
                            Evidence::Rotation(_) => {
                                painter.circle_stroke(
                                    q,
                                    s + 1.0,
                                    egui::Stroke::new(4.0, egui::Color32::from_black_alpha(150)),
                                );
                                painter.circle_stroke(
                                    q,
                                    s + 1.0,
                                    egui::Stroke::new(2.0, glyph_col),
                                );
                            }
                            Evidence::Debris(_) => {
                                painter.add(egui::Shape::convex_polygon(
                                    vec![
                                        q + egui::vec2(-s, -s),
                                        q + egui::vec2(s, -s),
                                        q + egui::vec2(0.0, s),
                                    ],
                                    glyph_col.gamma_multiply(0.35),
                                    egui::Stroke::new(1.5, glyph_col),
                                ));
                            }
                        }
                        // Labelled only where the label fits: never over the verdict's own
                        // label or another, whatever the zoom. Every one is in the card; the
                        // picked one is always labelled.
                        if is_picked || (label_all && !centre) {
                            halo_label(
                                painter,
                                &mut placed,
                                is_picked,
                                q + egui::vec2(s + 6.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                if centre {
                                    format!("{label} (centre)")
                                } else {
                                    format!("{label} \u{b7} {:.1} km", m.km)
                                },
                                egui::FontId::proportional(if touch { 12.0 } else { 10.5 }),
                                glyph_col,
                            );
                        }
                        let mhit =
                            egui::Rect::from_center_size(q, egui::vec2(member_hit, member_hit));
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
                // The verdict: a warning triangle, a dark rim when open so it stands out from
                // its own web.
                let tri = vec![
                    p + egui::vec2(0.0, -14.0),
                    p + egui::vec2(12.5, 7.5),
                    p + egui::vec2(-12.5, 7.5),
                ];
                if is_open {
                    painter.circle_filled(p, 19.0, egui::Color32::from_black_alpha(110));
                    painter.circle_stroke(p, 19.0, egui::Stroke::new(2.0, col));
                }
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
                halo_galley(painter, verdict_rect.min, verdict_label, col);
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
                        if let Some((_, s, _)) = &air {
                            ui.label(format!("Environment: STP {:.1}", s.stp));
                        }
                        ui.weak(format!(
                            "{} detections tied together. Click or tap to open the web.",
                            c.members.len()
                        ));
                        lineage_lines(ui, tornado_lineage);
                    });
                }
                if !is_open {
                    continue;
                }
                // The card, away from the storm: on the side of the pane the marker is not on,
                // below the alert banners and clear of the colour legend; on a narrow pane a
                // sheet along the bottom. A thin line ties it to its marker.
                let place = card_placement(prect, p);
                let card = egui::Area::new(egui::Id::new(("circulation_card", idx, ci)))
                    .order(egui::Order::Foreground)
                    .pivot(place.pivot)
                    .fixed_pos(place.at)
                    .constrain_to(prect)
                    .show(&ctx, |ui| {
                        crate::ui::style::glass(ui, 240).show(ui, |ui| {
                            ui.set_width(place.width);
                            let body = if touch { 13.5 } else { 12.0 };
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} \u{b7} {}",
                                        t.tier.label(),
                                        wxdata::evidence::out_of_100(t.score)
                                    ))
                                    .strong()
                                    .size(body + 1.5)
                                    .color(col),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let side = if touch { 40.0 } else { 24.0 };
                                        let close = ui
                                            .add(
                                                egui::Button::new(
                                                    egui::RichText::new(egui_phosphor::regular::X)
                                                        .size(if touch { 18.0 } else { 14.0 }),
                                                )
                                                .frame(false)
                                                .min_size(egui::vec2(side, side)),
                                            )
                                            .on_hover_text("Close");
                                        if close.clicked() {
                                            set_open = Some(None);
                                            set_pick = Some(None);
                                        }
                                    },
                                );
                            });
                            // Tall with a factor breakdown open: it scrolls inside the map.
                            egui::ScrollArea::vertical()
                                .max_height(place.max_height)
                                .show(ui, |ui| {
                                    // The verdict in a few lines; the full working (every
                                    // tilt's numbers, the track, each term) one tap away.
                                    let (summary, details) =
                                        t.reasons.split_at(t.reasons.len().min(SUMMARY_REASONS));
                                    for r in summary {
                                        ui.label(egui::RichText::new(r).size(body));
                                    }
                                    if !details.is_empty() {
                                        egui::CollapsingHeader::new(
                                            egui::RichText::new(format!(
                                                "Radar details ({} lines)",
                                                details.len()
                                            ))
                                            .size(body - 1.0),
                                        )
                                        .id_salt(("circulation_details", idx))
                                        .show(ui, |ui| {
                                            for r in details {
                                                ui.label(
                                                    egui::RichText::new(r.trim_start())
                                                        .size(body - 1.5),
                                                );
                                            }
                                        });
                                    }
                                    // The air feeding it, from the HRRR hour the gate read.
                                    if let Some((run, s, lead)) = &air {
                                        ui.add_space(4.0);
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "Environment \u{b7} HRRR {}Z +{lead} h",
                                                run.format("%H")
                                            ))
                                            .weak()
                                            .size(body - 1.0),
                                        );
                                        ui.label(egui::RichText::new(s.summary()).size(body - 0.5));
                                        if s.stp < wxdata::near_storm::GATE_STP {
                                            ui.label(
                                                egui::RichText::new(
                                                    "The model's air here cannot support a \
                                                     tornado (STP under 0.25): shown on its \
                                                     radar evidence alone.",
                                                )
                                                .weak()
                                                .size(body - 1.5),
                                            );
                                        }
                                    }
                                    ui.add_space(6.0);
                                    ui.label(
                                        egui::RichText::new("Tied together")
                                            .weak()
                                            .size(body - 1.0),
                                    );
                                    let row_h = if touch { 36.0 } else { 22.0 };
                                    for m in &c.members {
                                        let (mc, what, _, _, _) = describe(m.evidence);
                                        let where_ = if Some(m.evidence) == seed {
                                            "centre".to_string()
                                        } else {
                                            format!("{:.1} km off", m.km)
                                        };
                                        let glyph = match m.evidence {
                                            Evidence::Rotation(_) => "\u{25ef}",
                                            Evidence::Debris(_) => "\u{25bd}",
                                        };
                                        let sel = picked_member
                                            .is_some_and(|pm| pm.evidence == m.evidence);
                                        let row = ui.add(
                                            egui::Button::selectable(
                                                sel,
                                                egui::RichText::new(format!(
                                                    "{glyph}  {what} \u{b7} {where_}"
                                                ))
                                                .color(mc)
                                                .size(body),
                                            )
                                            .min_size(egui::vec2(ui.available_width(), row_h)),
                                        );
                                        if row.clicked() {
                                            set_pick = Some((!sel).then_some((m.lon, m.lat)));
                                        }
                                    }
                                    match picked_member {
                                        Some(m) => {
                                            let (mc, _, lines, track, _) = describe(m.evidence);
                                            ui.separator();
                                            score_tooltip(ui, lines, track, mc);
                                        }
                                        None => {
                                            ui.label(
                                                egui::RichText::new(if touch {
                                                    "Tap a detection, here or on the map, for \
                                                     the factors behind it."
                                                } else {
                                                    "Pick a detection, here or on the map, for \
                                                     the factors behind it."
                                                })
                                                .weak()
                                                .size(body - 1.5),
                                            );
                                        }
                                    }
                                    // Where the verdict came from: a finger has no hover, so the
                                    // card is where a phone reads it.
                                    lineage_lines(ui, tornado_lineage);
                                });
                        });
                    });
                // The tie from card to marker, from the card's nearest edge.
                let r = card.response.rect;
                let from = egui::pos2(
                    p.x.clamp(r.left(), r.right()),
                    p.y.clamp(r.top(), r.bottom()),
                );
                if from.distance(p) > 24.0 {
                    let to = p + (from - p).normalized() * 20.0;
                    painter.line_segment(
                        [from, to],
                        egui::Stroke::new(3.0, egui::Color32::from_black_alpha(120)),
                    );
                    painter
                        .line_segment([from, to], egui::Stroke::new(1.25, col.gamma_multiply(0.8)));
                }
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

/// How many of a verdict's reasons its card shows before folding the rest under "Radar details":
/// the verdict lines (folded columns, why it is shown, confirmation) and the column summary.
const SUMMARY_REASONS: usize = 3;

/// A Tornado ID tier's marker colour.
fn tier_colour(tier: wxdata::tornado_id::Tier) -> egui::Color32 {
    use wxdata::tornado_id::Tier;
    match tier {
        Tier::Possible => egui::Color32::from_rgb(245, 210, 60),
        Tier::Likely => egui::Color32::from_rgb(245, 110, 40),
        Tier::Debris => egui::Color32::from_rgb(225, 70, 225),
        Tier::Confirmed => CONFIRMED_GOLD,
    }
}

/// A map label with a dark halo, placed only where it overlaps nothing in `placed` (unless
/// `force`), which it then joins. Whether it was drawn.
#[allow(clippy::too_many_arguments)]
fn halo_label(
    painter: &egui::Painter,
    placed: &mut Vec<egui::Rect>,
    force: bool,
    at: egui::Pos2,
    align: egui::Align2,
    text: String,
    font: egui::FontId,
    colour: egui::Color32,
) -> bool {
    let galley = painter.layout_no_wrap(text, font, colour);
    let rect = align.anchor_size(at, galley.size());
    if !force && placed.iter().any(|r| r.expand(2.0).intersects(rect)) {
        return false;
    }
    placed.push(rect);
    halo_galley(painter, rect.min, galley, colour);
    true
}

/// Laid-out text with a dark halo, so it reads over any colour of radar.
fn halo_galley(
    painter: &egui::Painter,
    at: egui::Pos2,
    galley: std::sync::Arc<egui::Galley>,
    colour: egui::Color32,
) {
    let halo = egui::Color32::from_black_alpha(200);
    for d in [
        egui::vec2(-1.0, 0.0),
        egui::vec2(1.0, 0.0),
        egui::vec2(0.0, -1.0),
        egui::vec2(0.0, 1.0),
    ] {
        painter.galley_with_override_text_color(at + d, galley.clone(), halo);
    }
    painter.galley(at, galley, colour);
}

/// Where a circulation's card goes in a pane, given its marker.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CardPlacement {
    pivot: egui::Align2,
    at: egui::Pos2,
    width: f32,
    max_height: f32,
}

/// The card away from its marker: on a pane wide enough for it to sit beside the storm, at the
/// bottom of the half the marker is not in (the alert banners stack at the top; the colour
/// legend is on the right edge); on a narrow pane (a phone) a sheet along the bottom, or along
/// the top when the marker is in the lower half.
fn card_placement(prect: egui::Rect, marker: egui::Pos2) -> CardPlacement {
    const WIDTH: f32 = 340.0;
    /// Room left above the card for the alert banners.
    const TOP: f32 = 140.0;
    const BOTTOM: f32 = 36.0;
    const SIDE: f32 = 12.0;
    const LEGEND: f32 = 44.0;
    if prect.width() < 2.0 * WIDTH + 2.0 * LEGEND {
        let width = (prect.width() - 2.0 * SIDE).clamp(160.0, 520.0);
        let max_height = (prect.height() * 0.42).max(120.0);
        return if marker.y > prect.center().y {
            CardPlacement {
                pivot: egui::Align2::CENTER_TOP,
                at: egui::pos2(prect.center().x, prect.top() + SIDE),
                width,
                max_height,
            }
        } else {
            CardPlacement {
                pivot: egui::Align2::CENTER_BOTTOM,
                at: egui::pos2(prect.center().x, prect.bottom() - SIDE),
                width,
                max_height,
            }
        };
    }
    let max_height = (prect.height() - TOP - BOTTOM - 60.0).max(120.0);
    if marker.x > prect.center().x {
        CardPlacement {
            pivot: egui::Align2::LEFT_BOTTOM,
            at: egui::pos2(prect.left() + SIDE, prect.bottom() - BOTTOM),
            width: WIDTH,
            max_height,
        }
    } else {
        CardPlacement {
            pivot: egui::Align2::RIGHT_BOTTOM,
            at: egui::pos2(prect.right() - LEGEND, prect.bottom() - BOTTOM),
            width: WIDTH,
            max_height,
        }
    }
}

#[cfg(test)]
mod card_placement_tests {
    use super::card_placement;

    #[test]
    fn the_card_goes_where_the_storm_is_not() {
        let wide = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 800.0));
        // A marker on the right: the card on the left, and the other way round.
        let c = card_placement(wide, egui::pos2(900.0, 300.0));
        assert!(c.at.x < 600.0 && c.pivot == egui::Align2::LEFT_BOTTOM);
        let c = card_placement(wide, egui::pos2(200.0, 300.0));
        assert!(c.at.x > 600.0 && c.pivot == egui::Align2::RIGHT_BOTTOM);
        // Clear of the alert banners at the top.
        assert!(c.at.y - c.max_height > 100.0);
        // A phone-width pane: a sheet across it, on the half the marker is not in.
        let narrow = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 800.0));
        let c = card_placement(narrow, egui::pos2(200.0, 200.0));
        assert_eq!(c.pivot, egui::Align2::CENTER_BOTTOM);
        assert!(c.width <= 400.0 && c.max_height <= 800.0 * 0.42 + 0.1);
        let c = card_placement(narrow, egui::pos2(200.0, 700.0));
        assert_eq!(c.pivot, egui::Align2::CENTER_TOP);
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
        let (mut vel_pairs, mut cc_pairs, mut zdr) = (Vec::new(), Vec::new(), Vec::new());
        for tilt in 0..level2::elevation_angles(&scan).len() {
            // As `compute_tds_uncached` reads it: ZDR discounts the debris signatures.
            if let Ok(d) = level2::bin_scan(&scan, Moment::DifferentialReflectivity, tilt) {
                zdr.push(d);
            }
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
        let mut debris = wxdata::tds::detect_volume(&cc_pairs, 0.80, 40.0, 150.0, 4);
        wxdata::tds::apply_zdr(&mut debris, &zdr);
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
            debris_inputs: wxdata::detection_lineage::debris_input_coverage(cc_pairs, zdr),
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
