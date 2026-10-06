//! Vertical cross-section window: a distance×height reflectivity panel reconstructed from the
//! volume's stacked tilts, colored with the reflectivity palette.

use crate::colormap::ColorTable;
use wxdata::level2::Moment;
use wxdata::xsection::CrossSection;

/// Turn a cross-section into an egui image (row 0 = top of panel), colored via `table`.
pub fn to_image(xs: &CrossSection, table: &ColorTable) -> egui::ColorImage {
    let mut buf = vec![0u8; xs.cols * xs.rows * 4];
    for r in 0..xs.rows {
        for c in 0..xs.cols {
            let rgba = xs
                .at(c, r)
                .and_then(|v| table.sample(v))
                .unwrap_or([18, 18, 18, 255]);
            buf[(r * xs.cols + c) * 4..(r * xs.cols + c) * 4 + 4].copy_from_slice(&rgba);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([xs.cols, xs.rows], &buf)
}

/// What to show for the pixel under `pos` within the drawn panel `rect`: its position (distance
/// along the cut, height), its value if any, and — ROADMAP_NEW C3's "warn when a sampled feature
/// is below/above sampled beam coverage" — an explicit note when that value is only there because
/// [`wxdata::xsection::sample_profile`] held the nearest real beam sample over past the true
/// coverage boundary, which otherwise reads indistinguishably from a real sample at a glance.
/// `None` when `pos` isn't over the panel at all.
fn hover_readout(
    xs: &CrossSection,
    moment: Moment,
    rect: egui::Rect,
    pos: egui::Pos2,
) -> Option<String> {
    if !rect.contains(pos) || xs.cols < 2 || xs.rows < 2 {
        return None;
    }
    let tx = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    let ty = ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
    let col = (tx * (xs.cols - 1) as f32).round() as usize;
    let row = (ty * (xs.rows - 1) as f32).round() as usize;
    let dist_km = xs.length_km * col as f64 / (xs.cols - 1) as f64;
    let height_km = xs.max_height_km * (1.0 - row as f32 / (xs.rows - 1) as f32);
    let mut s = format!("{dist_km:.0} km along \u{b7} {height_km:.1} km above the radar");
    match xs.at(col, row) {
        Some(v) => {
            let units = moment.units();
            if units.is_empty() {
                s.push_str(&format!("\n{v:.2}"));
            } else {
                s.push_str(&format!("\n{v:.1} {units}"));
            }
            if !xs.is_covered(col, row) {
                s.push_str(
                    "\n\u{26a0} outside beam coverage \u{2014} nearest tilt held over across \
                     the gap, not an actual sample here",
                );
            }
        }
        None => s.push_str("\nNo beam coverage here"),
    }
    Some(s)
}

/// Draw each tilt's beam-centre curve over the already-rendered panel `rect` (ROADMAP_NEW C3 /
/// suggestions.md §3.2). `xs.beam_lines` is radar-relative geometry already computed alongside the
/// reflectivity grid — this only maps it onto the same pixels the image was stretched to.
fn draw_beam_rise(ui: &egui::Ui, rect: egui::Rect, xs: &CrossSection) {
    let painter = ui.painter();
    // A handful of fixed hues cycled by tilt index rather than one flat color: with several tilts
    // in frame (a full VCP can be a dozen-plus), telling "which line is which" apart needs some
    // differentiation, but a full rainbow would fight the reflectivity palette underneath it for
    // attention. Muted and desaturated on purpose.
    const HUES: [egui::Color32; 6] = [
        egui::Color32::from_rgb(255, 255, 255),
        egui::Color32::from_rgb(255, 210, 120),
        egui::Color32::from_rgb(140, 210, 255),
        egui::Color32::from_rgb(200, 160, 255),
        egui::Color32::from_rgb(160, 255, 200),
        egui::Color32::from_rgb(255, 160, 190),
    ];
    let cols = xs.cols.max(2) as f32;
    for (i, line) in xs.beam_lines.iter().enumerate() {
        let color = HUES[i % HUES.len()].gamma_multiply(0.8);
        let stroke = egui::Stroke::new(1.3, color);
        // Break the polyline at every gap (the beam left the panel, or hasn't reached it yet)
        // rather than joining across one — a straight line spanning a None run would draw a false
        // segment that never corresponds to this tilt's actual geometry in that gap.
        let mut segment: Vec<egui::Pos2> = Vec::new();
        let flush = |segment: &mut Vec<egui::Pos2>| {
            if segment.len() >= 2 {
                painter.line(segment.clone(), stroke);
            }
            segment.clear();
        };
        for (col, h) in line.height_km.iter().enumerate() {
            match h {
                Some(h) => {
                    let x = rect.left() + rect.width() * col as f32 / (cols - 1.0);
                    let frac_from_top =
                        1.0 - (h / xs.max_height_km.max(f32::EPSILON)).clamp(0.0, 1.0);
                    let y = rect.top() + rect.height() * frac_from_top;
                    segment.push(egui::pos2(x, y));
                }
                None => flush(&mut segment),
            }
        }
        flush(&mut segment);
    }
    if !xs.beam_lines.is_empty() {
        // A small color-coded legend along the panel's top-right — one label per tilt, in the
        // same color as its line, rather than plain text that would need its own key.
        let mut x = rect.right() - 6.0;
        let y = rect.top() + 4.0;
        for (i, line) in xs.beam_lines.iter().enumerate().rev() {
            let color = HUES[i % HUES.len()];
            let label = format!("{:.1}°", line.elevation_deg);
            let galley = ui
                .painter()
                .layout_no_wrap(label, egui::FontId::proportional(9.5), color);
            x -= galley.size().x;
            painter.galley(egui::pos2(x, y), galley, color);
            x -= 6.0;
        }
    }
}

/// The line's exact controls and what the window shows about its sampling. The app fills the
/// current values in; the window writes back an edited bearing/length and one-shot requests.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XsControls {
    pub bearing_deg: f64,
    pub length_km: f64,
    /// Requested slide this frame, km to the right of A→B.
    pub slide_km: f64,
    pub radial: bool,
    pub swap: bool,
    pub cut_3d: bool,
    /// Site, tilts and the span their radials were scanned over.
    pub info: String,
    /// The radar antenna's altitude above mean sea level, km, when the site is known: the
    /// panel's heights are above the antenna, and this turns them into MSL.
    pub antenna_msl_km: Option<f64>,
    /// The ground along the cut, from A to B, when the app has terrain for it.
    pub ground: GroundProfile,
}

/// Ground height along the cut (ROADMAP_PARITY M3.6): evenly spaced samples from A to B, metres
/// MSL, `None` where the terrain is not here; the grid's resolution; and whether more is on its
/// way.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroundProfile {
    pub msl_m: Vec<Option<f32>>,
    pub resolution_m: Option<f64>,
    pub loading: bool,
}

impl GroundProfile {
    /// The ground `along_km` from A on a cut `length_km` long, metres MSL, from the nearest
    /// sample.
    pub fn at(&self, along_km: f64, length_km: f64) -> Option<f64> {
        let n = self.msl_m.len();
        if n == 0 || length_km <= 0.0 {
            return None;
        }
        let i = ((along_km / length_km).clamp(0.0, 1.0) * (n - 1) as f64).round() as usize;
        self.msl_m[i].map(f64::from)
    }
}

/// One end of the cross-section ruler: where it is on the panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RulerEnd {
    /// Distance along the cut from A, km.
    pub along_km: f64,
    /// Height above the radar antenna, km (the panel's own axis).
    pub above_antenna_km: f64,
}

/// What the ruler measures (ROADMAP_PARITY M3.6).
#[derive(Clone, Debug, PartialEq)]
pub struct RulerReading {
    /// Ground distance between the ends along the cut, km.
    pub horizontal_km: f64,
    /// Height of the second end over the first, km.
    pub vertical_km: f64,
    /// Straight-line distance between them, km (flat over the span; the panel's heights already
    /// carry the earth's curvature through the beam model).
    pub slant_km: f64,
    /// Per end: height above the antenna, height above MSL when the antenna's is known, and the
    /// sampled value with whether a beam actually passes there.
    pub ends: [(f64, Option<f64>, Option<f32>, bool); 2],
    /// Per end: the ground there, km MSL, when known; and the terrain grid and whether it is
    /// still loading, for the text.
    pub ground_km: [Option<f64>; 2],
    pub ground_resolution_m: Option<f64>,
    pub ground_loading: bool,
}

/// The panel cell under a position, clamped to the panel.
fn cell_at(xs: &CrossSection, along_km: f64, above_km: f64) -> (usize, usize) {
    let cols = xs.cols.max(2) - 1;
    let rows = xs.rows.max(2) - 1;
    let col = (along_km / xs.length_km.max(1e-9) * cols as f64)
        .round()
        .clamp(0.0, cols as f64) as usize;
    let row = ((1.0 - above_km / f64::from(xs.max_height_km).max(1e-9)) * rows as f64)
        .round()
        .clamp(0.0, rows as f64) as usize;
    (col, row)
}

/// Measure between two points on the panel.
pub fn ruler_reading(
    xs: &CrossSection,
    ends: [RulerEnd; 2],
    antenna_msl_km: Option<f64>,
    ground: &GroundProfile,
) -> RulerReading {
    let ground_at = |e: RulerEnd| ground.at(e.along_km, xs.length_km).map(|m| m / 1000.0);
    let horizontal_km = (ends[1].along_km - ends[0].along_km).abs();
    let vertical_km = ends[1].above_antenna_km - ends[0].above_antenna_km;
    let end = |e: RulerEnd| {
        let (c, r) = cell_at(xs, e.along_km, e.above_antenna_km);
        (
            e.above_antenna_km,
            antenna_msl_km.map(|a| a + e.above_antenna_km),
            xs.at(c, r),
            xs.is_covered(c, r),
        )
    };
    RulerReading {
        horizontal_km,
        vertical_km,
        slant_km: horizontal_km.hypot(vertical_km),
        ends: [end(ends[0]), end(ends[1])],
        ground_km: [ground_at(ends[0]), ground_at(ends[1])],
        ground_resolution_m: ground.resolution_m,
        ground_loading: ground.loading,
    }
}

/// The ruler's reading as lines a person reads.
pub fn ruler_text(r: &RulerReading, moment: Moment) -> Vec<String> {
    let mut lines = vec![format!(
        "Ruler: {:.1} km across \u{b7} {:+.1} km up \u{b7} {:.1} km straight",
        r.horizontal_km, r.vertical_km, r.slant_km
    )];
    for ((name, (above, msl, value, covered)), ground) in
        ["1", "2"].iter().zip(&r.ends).zip(&r.ground_km)
    {
        let mut l = format!("{name}: {above:.1} km above the radar");
        match msl {
            Some(m) => l.push_str(&format!(" ({m:.1} km MSL)")),
            None => l.push_str(" (MSL unknown: no site altitude)"),
        }
        match (msl, ground) {
            (Some(m), Some(g)) => l.push_str(&format!(
                " \u{b7} {:.1} km above the ground (terrain on a {:.0} m grid)",
                m - g,
                r.ground_resolution_m.unwrap_or(f64::NAN)
            )),
            _ if r.ground_loading => l.push_str(" \u{b7} ground loading"),
            _ => l.push_str(" \u{b7} ground level here unknown: no terrain data"),
        }
        match value {
            Some(v) if *covered => l.push_str(&format!(" \u{b7} {v:.1} {}", moment.units())),
            Some(v) => l.push_str(&format!(
                " \u{b7} {v:.1} {} (held over from the nearest beam, not sampled here)",
                moment.units()
            )),
            None => l.push_str(" \u{b7} no beam here"),
        }
        lines.push(l);
    }
    lines
}

/// The share of the panel's cells inside real beam coverage.
pub fn covered_fraction(xs: &CrossSection) -> f32 {
    let total = xs.cols * xs.rows;
    if total == 0 {
        return 0.0;
    }
    let covered = (0..xs.rows)
        .flat_map(|r| (0..xs.cols).map(move |c| (c, r)))
        .filter(|&(c, r)| xs.is_covered(c, r))
        .count();
    covered as f32 / total as f32
}

fn controls(ui: &mut egui::Ui, ctl: &mut XsControls) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Bearing");
        ui.add(
            egui::DragValue::new(&mut ctl.bearing_deg)
                .range(0.0..=359.9)
                .speed(0.5)
                .suffix("°"),
        )
        .on_hover_text("A→B, degrees from north; the line swings about its middle");
        ui.label("Length");
        ui.add(
            egui::DragValue::new(&mut ctl.length_km)
                .range(5.0..=460.0)
                .speed(0.5)
                .suffix(" km"),
        )
        .on_hover_text("Keeps the middle where it is");
        ui.separator();
        ui.label("Slide");
        for (label, km, tip) in [
            ("\u{2190}5", -5.0, "5 km to the left of A→B"),
            ("\u{2190}1", -1.0, "1 km to the left of A→B"),
            ("1\u{2192}", 1.0, "1 km to the right of A→B"),
            ("5\u{2192}", 5.0, "5 km to the right of A→B"),
        ] {
            if ui.small_button(label).on_hover_text(tip).clicked() {
                ctl.slide_km += km;
            }
        }
        ui.separator();
        if ui
            .small_button("Radial")
            .on_hover_text(
                "Swing it about its middle onto the radar's beam, so radial velocity along it \
                 reads straight",
            )
            .clicked()
        {
            ctl.radial = true;
        }
        if ui
            .small_button("A\u{21c4}B")
            .on_hover_text("Swap the ends")
            .clicked()
        {
            ctl.swap = true;
        }
        ui.checkbox(&mut ctl.cut_3d, "3D cut").on_hover_text(
            "Cut the 3D view's smooth volume along this section, a 4 km slab, as it moves",
        );
    });
    if !ctl.info.is_empty() {
        ui.weak(&ctl.info);
    }
    ui.weak(
        "With the cross-section tool: drag A, B or the middle on the map, swing it by the \u{21bb} \
         handle (Shift snaps to 15°), Esc puts it back.",
    );
}

/// Show the cross-section window. Returns `false` when it should close.
pub fn show(
    ctx: &egui::Context,
    xs: &CrossSection,
    tex: &egui::TextureHandle,
    moment: &mut wxdata::level2::Moment,
    beam_rise: &mut bool,
    ctl: &mut XsControls,
    drawer: &mut crate::ui::drawer::Drawer,
) -> bool {
    use wxdata::level2::Moment;
    let mut open = true;
    let mut changed = false;
    let Some(window) = drawer.page_sized(
        ctx,
        "Cross-section",
        &mut open,
        false,
        560.0,
        egui::Window::new("Cross-section"),
    ) else {
        return open;
    };
    window.show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(format!(
                "Length {:.0} km · top {:.0} km",
                xs.length_km, xs.max_height_km
            ));
            ui.separator();
            // Velocity shows how a couplet leans with height; CC shows how deep the debris
            // ball really is. Both were unreachable while this was hard-wired to REF.
            for m in [
                Moment::Reflectivity,
                Moment::Velocity,
                Moment::CorrelationCoefficient,
            ] {
                let info = crate::products::info(m);
                if ui
                    .selectable_label(*moment == m, info.short)
                    .on_hover_text(info.name)
                    .clicked()
                    && *moment != m
                {
                    *moment = m;
                    changed = true;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                crate::ui::csv_buttons(
                    ui,
                    "xsection.csv",
                    "The sampled grid, top row first",
                    || xs.to_csv(),
                );
            });
        });
        // Its own row rather than crammed into the one above: that row already holds the length
        // label, three moment buttons and two CSV buttons, and a narrow window (a laptop split
        // pane, this window's own minimum before the user resizes it) left no room for a sixth
        // control without the two competing layouts overlapping their text.
        controls(ui, ctl);
        // The ruler is the window's own: two ends on the panel, dropped when the cut changes.
        let ruler_on_id = egui::Id::new("xs_ruler_on");
        let ruler_id = egui::Id::new("xs_ruler");
        let mut ruler_on = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(ruler_on_id).unwrap_or(false));
        let mut ruler = ui.ctx().data(|d| {
            d.get_temp::<(f64, [RulerEnd; 2])>(ruler_id)
                .filter(|(len, _)| (*len - xs.length_km).abs() < 1e-6)
                .map(|(_, e)| e)
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut ruler_on, "Ruler").on_hover_text(
                "Drag across the panel to measure distance and height between two points; heights \
                 above the radar and above sea level, and the value at each end",
            );
            ui.checkbox(beam_rise, "Beam rise").on_hover_text(
                "Draw each tilt's beam-centre height across the panel, so a feature reading \
                 weaker higher up can be told apart from the beam simply climbing clear of it \
                 (4/3-earth model, approximate).",
            );
        });
        ui.separator();
        // Draw the panel stretched to a readable size (distance wide, height tall).
        let avail = ui.available_size();
        let w = avail.x.max(200.0);
        let h = (w * 0.4).clamp(120.0, 260.0);
        let img = egui::Image::new(tex)
            .fit_to_exact_size(egui::vec2(w, h))
            .texture_options(egui::TextureOptions::LINEAR);
        let resp = ui.add(img.sense(if ruler_on {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::hover()
        }));
        // Axis captions along the drawn rect.
        let rect = resp.rect;
        // ROADMAP_NEW C3: "warn when a sampled feature is below/above sampled beam coverage" — a
        // hovered pixel gets its position (distance, height), its value, and — when the pixel is
        // filled only because `sample_profile` held a beam sample over past the true coverage
        // boundary — an explicit warning that no beam actually passes through that point.
        let hover = resp
            .hover_pos()
            .and_then(|pos| hover_readout(xs, *moment, rect, pos));
        if let Some(text) = hover {
            resp.clone().on_hover_text(text);
        }
        if *beam_rise {
            draw_beam_rise(ui, rect, xs);
        }
        // The ground along the cut, where it rises above the radar antenna (the panel's zero):
        // terrain the low beams may run into. Drawn from the terrain tiles, on their grid.
        if let Some(antenna) = ctl.antenna_msl_km {
            let n = ctl.ground.msl_m.len();
            let painter = ui.painter_at(rect);
            let top = f64::from(xs.max_height_km).max(1e-9);
            let mut run: Vec<egui::Pos2> = Vec::new();
            let flush = |run: &mut Vec<egui::Pos2>| {
                if run.len() >= 2 {
                    painter.add(egui::Shape::line(
                        std::mem::take(run),
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(170, 120, 70)),
                    ));
                }
                run.clear();
            };
            for (i, g) in ctl.ground.msl_m.iter().enumerate() {
                let above = g
                    .map(|m| f64::from(m) / 1000.0 - antenna)
                    .filter(|h| *h > 0.0);
                match above {
                    Some(h) if n > 1 => {
                        let x = rect.left() + rect.width() * i as f32 / (n - 1) as f32;
                        let y = rect.bottom() - (h / top).min(1.0) as f32 * rect.height();
                        run.push(egui::pos2(x, y));
                    }
                    _ => flush(&mut run),
                }
            }
            flush(&mut run);
        }
        if ruler_on {
            let at = |p: egui::Pos2| RulerEnd {
                along_km: f64::from(((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0))
                    * xs.length_km,
                above_antenna_km: f64::from(
                    1.0 - ((p.y - rect.top()) / rect.height()).clamp(0.0, 1.0),
                ) * f64::from(xs.max_height_km),
            };
            if let Some(p) = resp.interact_pointer_pos() {
                if resp.drag_started() || resp.clicked() {
                    ruler = Some([at(p), at(p)]);
                } else if resp.dragged() {
                    if let Some(r) = ruler.as_mut() {
                        r[1] = at(p);
                    }
                }
            }
            if let Some(ends) = ruler {
                let to_screen = |e: RulerEnd| {
                    egui::pos2(
                        rect.left() + (e.along_km / xs.length_km.max(1e-9)) as f32 * rect.width(),
                        rect.bottom()
                            - (e.above_antenna_km / f64::from(xs.max_height_km).max(1e-9)) as f32
                                * rect.height(),
                    )
                };
                let (p, q) = (to_screen(ends[0]), to_screen(ends[1]));
                let painter = ui.painter_at(rect);
                painter.line_segment([p, q], egui::Stroke::new(3.0, egui::Color32::BLACK));
                painter.line_segment([p, q], egui::Stroke::new(1.5, egui::Color32::WHITE));
                for (pt, n) in [(p, "1"), (q, "2")] {
                    painter.circle_filled(pt, 4.0, egui::Color32::WHITE);
                    painter.text(
                        pt + egui::vec2(6.0, -6.0),
                        egui::Align2::LEFT_BOTTOM,
                        n,
                        egui::FontId::proportional(11.0),
                        egui::Color32::WHITE,
                    );
                }
            }
        }
        let cap = |ui: &egui::Ui, pos, anchor, txt: &str| {
            ui.painter().text(
                pos,
                anchor,
                txt,
                egui::FontId::proportional(10.0),
                egui::Color32::from_gray(200),
            );
        };
        cap(
            ui,
            rect.left_top() + egui::vec2(2.0, 2.0),
            egui::Align2::LEFT_TOP,
            &format!("{:.0} km", xs.max_height_km),
        );
        cap(
            ui,
            rect.left_bottom() + egui::vec2(2.0, -2.0),
            egui::Align2::LEFT_BOTTOM,
            "0 km",
        );
        cap(
            ui,
            rect.left_bottom() + egui::vec2(2.0, -14.0),
            egui::Align2::LEFT_BOTTOM,
            "A",
        );
        cap(
            ui,
            rect.right_bottom() + egui::vec2(-2.0, -14.0),
            egui::Align2::RIGHT_BOTTOM,
            "B",
        );
        ui.weak(
            "A→B left to right; height above the radar increases upward. Gaps = no beam coverage.",
        );
        if let (true, Some(ends)) = (ruler_on, ruler) {
            for line in ruler_text(
                &ruler_reading(xs, ends, ctl.antenna_msl_km, &ctl.ground),
                *moment,
            ) {
                ui.label(egui::RichText::new(line).monospace().size(11.0));
            }
        }
        ui.ctx().data_mut(|d| {
            d.insert_temp(ruler_on_id, ruler_on);
            match ruler {
                Some(e) => {
                    d.insert_temp(ruler_id, (xs.length_km, e));
                }
                None => d.remove::<(f64, [RulerEnd; 2])>(ruler_id),
            }
        });
    });
    if changed {
        // Force a rebuild on the next frame with the new moment.
        ctx.request_repaint();
    }
    open
}

#[cfg(test)]
mod tests {
    use super::*;
    use wxdata::level2::BinnedSweep;

    /// A synthetic storm at eight tilts of a standard VCP: a 60 dBZ core 60 km east of the radar,
    /// falling off 1 dBZ per km from it and 4 dBZ per km of height above 8 km, so the section has
    /// a core, an anvil-shaped top and the beam gaps between the upper tilts.
    fn storm_volume() -> Vec<BinnedSweep> {
        let (az_bins, gate_count) = (360usize, 600usize);
        let (first_gate_km, gate_interval_km) = (2.125f32, 0.25f32);
        let (vmin, vmax) = (-32.0f32, 95.0f32);
        [0.5f32, 1.5, 2.4, 3.4, 4.3, 6.0, 9.9, 14.6]
            .into_iter()
            .map(|elevation_deg| {
                let mut data = vec![0u8; az_bins * gate_count];
                for az in 0..az_bins {
                    let th = (az as f32 + 0.5).to_radians();
                    for g in 0..gate_count {
                        let r = first_gate_km + g as f32 * gate_interval_km;
                        let (x, y) = (r * th.sin(), r * th.cos());
                        let h =
                            r * elevation_deg.to_radians().sin() + r * r / (2.0 * 1.21 * 6371.0);
                        let d = ((x - 60.0).powi(2) + y.powi(2)).sqrt();
                        let dbz = 60.0 - d - (h - 8.0).max(0.0) * 4.0;
                        if dbz >= 5.0 {
                            data[az * gate_count + g] =
                                (2.0 + (dbz - vmin) / (vmax - vmin) * 253.0).round() as u8;
                        }
                    }
                }
                BinnedSweep {
                    moment: Moment::Reflectivity,
                    az_bins,
                    gate_count,
                    data,
                    first_gate_km,
                    gate_interval_km,
                    radar_lat: 35.0,
                    radar_lon: -97.0,
                    elevation_deg,
                    value_min: vmin,
                    value_max: vmax,
                    ..Default::default()
                }
            })
            .collect()
    }

    /// Golden check of the cross-section panel (ROADMAP_2 §8.3): the reconstruction and its
    /// colouring, west to east through the storm core. CPU-only, so it runs everywhere; per-channel
    /// delta ≤ 8 on ≤ 0.5% of pixels absorbs floating-point differences between platforms. A
    /// missing golden is written in place and the test fails, so a new one is looked at before it
    /// is checked in.
    /// The ruler measures between two panel points: distance across and up, straight-line, each
    /// end's height above the radar and above sea level, and the value the panel holds there —
    /// saying when it is held over rather than sampled, and that ground level is not known.
    #[test]
    fn the_ruler_reads_distances_both_datums_and_the_panel_value() {
        let sweeps = storm_volume();
        let xs = wxdata::xsection::build(&sweeps, (-97.22, 35.0), (-95.8, 35.0), 300, 120, 18.0)
            .expect("sweeps");
        let a = RulerEnd {
            along_km: 20.0,
            above_antenna_km: 1.0,
        };
        let b = RulerEnd {
            along_km: 50.0,
            above_antenna_km: 5.0,
        };
        let r = ruler_reading(&xs, [a, b], Some(0.37), &GroundProfile::default());
        assert!((r.horizontal_km - 30.0).abs() < 1e-9);
        assert!((r.vertical_km - 4.0).abs() < 1e-9);
        assert!((r.slant_km - 30.265_5).abs() < 1e-3, "{}", r.slant_km);
        assert!(
            (r.ends[1].1.unwrap() - 5.37).abs() < 1e-9,
            "MSL = antenna + above it"
        );
        // The value is the panel's own at that cell.
        let (c, row) = cell_at(&xs, b.along_km, b.above_antenna_km);
        assert_eq!(r.ends[1].2, xs.at(c, row));
        assert_eq!(r.ends[1].3, xs.is_covered(c, row));
        // Measured either way round, the distances are the same and the climb flips.
        let back = ruler_reading(&xs, [b, a], Some(0.37), &GroundProfile::default());
        assert_eq!(back.horizontal_km, r.horizontal_km);
        assert_eq!(back.vertical_km, -r.vertical_km);
        let text = ruler_text(
            &ruler_reading(&xs, [a, b], None, &GroundProfile::default()),
            Moment::Reflectivity,
        );
        assert!(
            text[0].starts_with("Ruler: 30.0 km across \u{b7} +4.0 km up"),
            "{text:?}"
        );
        assert!(text[1].contains("MSL unknown"), "{text:?}");
        assert!(text[1].contains("no terrain data"), "{text:?}");
        // With terrain along the cut: the end 50 km along stands 5.37 km MSL over 0.45 km ground.
        let ground = GroundProfile {
            msl_m: vec![Some(450.0); 11],
            resolution_m: Some(62.0),
            loading: false,
        };
        let with = ruler_text(
            &ruler_reading(&xs, [a, b], Some(0.37), &ground),
            Moment::Reflectivity,
        );
        assert!(
            with[2].contains("4.9 km above the ground (terrain on a 62 m grid)"),
            "{with:?}"
        );
        let loading = GroundProfile {
            loading: true,
            ..Default::default()
        };
        let l = ruler_text(
            &ruler_reading(&xs, [a, b], Some(0.37), &loading),
            Moment::Reflectivity,
        );
        assert!(l[1].contains("ground loading"), "{l:?}");
    }

    #[test]
    fn cross_section_matches_its_golden() {
        let sweeps = storm_volume();
        // 1° of longitude at 35°N is ~91 km: from 20 km west of the radar to ~110 km east.
        let xs = wxdata::xsection::build(&sweeps, (-97.22, 35.0), (-95.8, 35.0), 300, 120, 18.0)
            .expect("sweeps");
        let img = to_image(&xs, crate::colormap::default_table(Moment::Reflectivity));
        let actual: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
        let (w, h) = (xs.cols as u32, xs.rows as u32);

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let golden = dir.join("tests/golden/xsection_storm.png");
        if !golden.exists() {
            image::save_buffer(&golden, &actual, w, h, image::ColorType::Rgba8).unwrap();
            panic!(
                "wrote a new golden to {}; look at it, then check it in",
                golden.display()
            );
        }
        let expected = image::open(&golden).expect("decode golden").to_rgba8();
        assert_eq!((expected.width(), expected.height()), (w, h));
        let bad = expected
            .as_raw()
            .as_chunks::<4>()
            .0
            .iter()
            .zip(actual.as_chunks::<4>().0)
            .filter(|(e, a)| e.iter().zip(a.iter()).any(|(x, y)| x.abs_diff(*y) > 8))
            .count();
        if bad * 200 > (w * h) as usize {
            let dump = dir.join("../../target/xsection_storm_actual.png");
            let _ = image::save_buffer(&dump, &actual, w, h, image::ColorType::Rgba8);
            panic!(
                "cross-section golden: {bad} pixels off; actual at {}",
                dump.display()
            );
        }
    }
}
