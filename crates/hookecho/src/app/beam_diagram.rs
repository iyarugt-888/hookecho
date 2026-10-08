//! The beam diagram (WeatherWise-class "explicit beam-rise visualization"): every distinct tilt of
//! the active pane's scan drawn against range, as the 4/3-earth beam model puts it — centre and
//! half-power edges, metres above sea level — with the range under the cursor marked and each
//! beam's height read out there, plus the heights no beam samples at that range.
//!
//! Geometry only (`wxdata::beam_geometry`): it says where the radar looks, not what it saw, and
//! the beam model is the standard-atmosphere approximation, which real refraction departs from.
use super::*;

/// Ranges the diagram can span, km.
pub(crate) const SPANS_KM: [f32; 3] = [100.0, 230.0, 460.0];

pub(crate) struct BeamDiagram {
    pub open: bool,
    pub span_km: f32,
    /// The range read out when the cursor is not over the map (touch), km.
    pub range_km: f32,
}

impl Default for BeamDiagram {
    fn default() -> Self {
        Self {
            open: false,
            span_km: 230.0,
            range_km: 100.0,
        }
    }
}

/// A tilt's colour: low tilts cyan through high tilts magenta, as the 3D beam guides draw them.
fn tilt_color(i: usize, n: usize) -> egui::Color32 {
    let t = if n > 1 {
        i as f32 / (n - 1) as f32
    } else {
        0.0
    };
    let lerp = |a: f32, b: f32| (a + (b - a) * t) as u8;
    egui::Color32::from_rgb(lerp(60.0, 230.0), lerp(210.0, 80.0), lerp(230.0, 220.0))
}

/// The readout at one range: each tilt's bottom–top and centre, km MSL, and the unsampled
/// heights. Pure, so the words can be tested.
pub(crate) fn readout(
    elevations: &[f32],
    antenna_m: f64,
    ground_m: f64,
    range_km: f64,
) -> Vec<String> {
    use wxdata::beam_geometry::{beam_at_ground, distinct_tilts, unsampled_at};
    let km = |m: f64| m / 1000.0;
    let mut lines: Vec<String> = distinct_tilts(elevations)
        .into_iter()
        .filter_map(|e| {
            let b = beam_at_ground(range_km * 1000.0, e, antenna_m)?;
            Some(format!(
                "{e:.1}\u{b0}: {:.2}\u{2013}{:.2} km MSL, centre {:.2} km ({:.0} ft)",
                km(b.bottom.altitude_msl_m),
                km(b.top.altitude_msl_m),
                km(b.center.altitude_msl_m),
                b.center.altitude_msl_m * 3.280_84
            ))
        })
        .collect();
    let gaps = unsampled_at(elevations, antenna_m, range_km * 1000.0, ground_m);
    if gaps.is_empty() {
        lines.push("No gap between the ground and the highest tilt".into());
    } else {
        let parts: Vec<String> = gaps
            .iter()
            .map(|(a, b)| format!("{:.2}\u{2013}{:.2}", km(*a), km(*b)))
            .collect();
        lines.push(format!("Not sampled: {} km MSL", parts.join(", ")));
    }
    lines
}

/// The diagram for `site`'s `elevations`: span picker, the plot, and the readout at the hovered
/// range (or the slider's, without a pointer).
pub(crate) fn diagram_ui(
    ui: &mut egui::Ui,
    d: &mut BeamDiagram,
    site: &wxdata::sites::SiteEntry,
    elevations: &[f32],
    hovered: Option<(f64, f64)>,
) {
    let ground_m = f64::from(site.elevation_meters);
    let antenna_m = ground_m + wxdata::towers::tower_m(site.id);
    ui.horizontal_wrapped(|ui| {
        ui.label(format!(
            "{} \u{b7} {} tilts \u{b7} antenna {:.0} m MSL",
            site.id,
            wxdata::beam_geometry::distinct_tilts(elevations).len(),
            antenna_m
        ));
        egui::ComboBox::from_id_salt("beam_span")
            .selected_text(format!("{:.0} km", d.span_km))
            .width(80.0)
            .show_ui(ui, |ui| {
                for s in SPANS_KM {
                    ui.selectable_value(&mut d.span_km, s, format!("{s:.0} km"));
                }
            });
    });
    let range_km = match hovered {
        Some((km, _)) => km.min(f64::from(d.span_km)),
        None => {
            ui.add(
                egui::Slider::new(&mut d.range_km, 0.0..=d.span_km)
                    .text("range")
                    .suffix(" km"),
            );
            f64::from(d.range_km)
        }
    };
    // The plot: range across, height MSL up to 20 km.
    let w = ui.available_width().max(240.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 220.0), egui::Sense::hover());
    let plot = rect.shrink2(egui::vec2(34.0, 14.0));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    let top_m = 20_000.0;
    let span = f64::from(d.span_km);
    let to_screen = |km: f64, m: f64| {
        egui::pos2(
            plot.left() + (km / span) as f32 * plot.width(),
            plot.bottom() - ((m / top_m) as f32) * plot.height(),
        )
    };
    let weak = ui.visuals().weak_text_color();
    for h in [0.0, 5_000.0, 10_000.0, 15_000.0, 20_000.0] {
        let p = to_screen(0.0, h);
        painter.hline(
            plot.x_range(),
            p.y,
            egui::Stroke::new(0.5, weak.gamma_multiply(0.4)),
        );
        painter.text(
            egui::pos2(rect.left() + 2.0, p.y),
            egui::Align2::LEFT_CENTER,
            format!("{:.0} km", h / 1000.0),
            egui::FontId::proportional(9.0),
            weak,
        );
    }
    for k in 0..=4 {
        let km = span * k as f64 / 4.0;
        painter.text(
            to_screen(km, 0.0) + egui::vec2(0.0, 2.0),
            egui::Align2::CENTER_TOP,
            format!("{km:.0}"),
            egui::FontId::proportional(9.0),
            weak,
        );
    }
    // The radar's ground level.
    painter.hline(
        plot.x_range(),
        to_screen(0.0, ground_m).y,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(140, 110, 70)),
    );
    // Beams leave through the top of the plot: clip them there rather than pile them up on it.
    let beams = ui.painter_at(plot.expand(1.0));
    let profiles = wxdata::beam_geometry::tilt_profiles(elevations, antenna_m, span, 64);
    let n = profiles.len();
    for (i, t) in profiles.iter().enumerate() {
        let c = tilt_color(i, n);
        // Edges and centre as lines (the band is not convex, so no filled polygon).
        let line = |sel: fn(&(f64, f64, f64, f64)) -> f64| -> Vec<egui::Pos2> {
            t.points.iter().map(|p| to_screen(p.0, sel(p))).collect()
        };
        beams.add(egui::Shape::line(
            line(|p| p.1),
            egui::Stroke::new(0.8, c.gamma_multiply(0.6)),
        ));
        beams.add(egui::Shape::line(
            line(|p| p.3),
            egui::Stroke::new(0.8, c.gamma_multiply(0.6)),
        ));
        beams.add(egui::Shape::line(line(|p| p.2), egui::Stroke::new(1.6, c)));
        // Only beams that leave through the right edge are labelled there; the steep ones are
        // named in the readout.
        if let Some(last) = t.points.last().filter(|p| p.2 <= top_m) {
            painter.text(
                to_screen(last.0, last.2) + egui::vec2(-2.0, -2.0),
                egui::Align2::RIGHT_BOTTOM,
                format!("{:.1}\u{b0}", t.elevation_deg),
                egui::FontId::proportional(9.0),
                c,
            );
        }
    }
    // The range read out: a vertical line, red where no beam samples.
    let x = to_screen(range_km, 0.0).x;
    painter.vline(
        x,
        plot.y_range(),
        egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 214, 92)),
    );
    for (a, b) in
        wxdata::beam_geometry::unsampled_at(elevations, antenna_m, range_km * 1000.0, ground_m)
    {
        painter.line_segment(
            [to_screen(range_km, a), to_screen(range_km, b)],
            egui::Stroke::new(3.0, egui::Color32::from_rgb(230, 80, 70)),
        );
    }
    ui.label(
        egui::RichText::new(match hovered {
            Some((km, bearing)) => format!(
                "At the cursor: {km:.1} km from {}, bearing {bearing:.0}\u{b0}",
                site.id
            ),
            None => format!("At {range_km:.0} km (hover the map to follow the cursor)"),
        })
        .strong(),
    );
    egui::ScrollArea::vertical()
        .id_salt("beam_readout")
        .max_height(180.0)
        .show(ui, |ui| {
            for line in readout(elevations, antenna_m, ground_m, range_km) {
                ui.label(egui::RichText::new(line).small());
            }
        });
    ui.weak(
        "4/3-earth standard refraction and the WSR-88D 0.925\u{b0} half-power beam: \
             where the radar looks, not what it saw; heights above the radar's own \
             ground, not the terrain under the cursor.",
    );
}

impl HookEchoApp {
    pub(crate) fn beam_diagram_window(&mut self, ctx: &egui::Context) {
        if !self.beam_diagram.open {
            return;
        }
        let v = &self.views[self.active];
        let site = v.site.as_deref().and_then(wxdata::sites::site_by_id);
        let elevations = v.volume.as_ref().map(|vol| vol.elevations.clone());
        // The hovered point on the active pane, as range and bearing from its radar.
        let hovered = site
            .zip(self.hover_lonlat.filter(|(i, _)| *i == self.active))
            .map(|(s, (_, (lon, lat)))| {
                crate::geo::great_circle(
                    [f64::from(s.longitude), f64::from(s.latitude)],
                    [lon, lat],
                )
            });
        let mut open = true;
        let d = &mut self.beam_diagram;
        egui::Window::new("Beam diagram")
            .open(&mut open)
            .default_width(460.0)
            .show(ctx, |ui| match (site, elevations) {
                (Some(site), Some(elevations)) => diagram_ui(ui, d, site, &elevations, hovered),
                _ => {
                    ui.weak("Pick a radar and load a scan to see its beams.");
                }
            });
        self.beam_diagram.open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::readout;

    /// The diagram for KTLX with VCP 212's tilts, read at 120 km, written for review under
    /// `target/parity-review/beam-diagram`.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes the beam diagram capture for review"]
    fn gpu_beam_diagram_snapshot() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/beam-diagram");
        std::fs::create_dir_all(&dir).unwrap();
        let site = wxdata::sites::site_by_id("KTLX").expect("KTLX");
        let elevs = [
            0.5, 0.9, 1.3, 1.8, 2.4, 3.1, 4.0, 5.1, 6.4, 8.0, 10.0, 12.5, 15.6, 19.5, 0.5,
        ];
        let mut d = super::BeamDiagram::default();
        gpu.save(&dir.join("ktlx-vcp212-120km.png"), 520, 460, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    super::diagram_ui(ui, &mut d, site, &elevs, Some((120.0, 245.0)))
                });
        })
        .unwrap();
    }

    #[test]
    fn the_readout_lists_each_tilt_and_what_is_missed() {
        let elevs = [0.5, 0.9, 1.3, 1.8, 2.4, 0.5];
        let lines = readout(&elevs, 370.0, 360.0, 150.0);
        assert_eq!(
            lines.len(),
            6,
            "five distinct tilts and the gap line: {lines:?}"
        );
        assert!(lines[0].starts_with("0.5\u{b0}: "), "{}", lines[0]);
        assert!(
            lines[5].starts_with("Not sampled: 0.36\u{2013}"),
            "{}",
            lines[5]
        );
        // Ground above the lowest beam's bottom, and tilts closer than a beamwidth apart: the
        // column from the ground to the highest tilt is covered, and it says so.
        let covered = readout(&elevs, 370.0, 5_000.0, 150.0);
        assert!(covered.last().unwrap().starts_with("No gap"), "{covered:?}");
    }
}
