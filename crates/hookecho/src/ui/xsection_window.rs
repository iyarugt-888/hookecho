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
    let mut s = format!("{dist_km:.0} km along \u{b7} {height_km:.1} km up");
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

/// Show the cross-section window. Returns `false` when it should close.
pub fn show(
    ctx: &egui::Context,
    xs: &CrossSection,
    tex: &egui::TextureHandle,
    moment: &mut wxdata::level2::Moment,
    beam_rise: &mut bool,
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
        ui.horizontal(|ui| {
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
        let resp = ui.add(img);
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
            resp.on_hover_text(text);
        }
        if *beam_rise {
            draw_beam_rise(ui, rect, xs);
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
        ui.weak("A→B left to right; height increases upward. Gaps = no beam coverage.");
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
            .chunks_exact(4)
            .zip(actual.chunks_exact(4))
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
