//! Region statistics window (ROADMAP_NEW C4): every gate of the displayed tilt inside a box drawn
//! on the map, across every moment — a summary table, a histogram of one moment, a scatter plot of
//! two against each other, and the gates themselves as CSV. The numbers come from
//! [`wxdata::regionstats`]; this only draws them.

use wxdata::level2::Moment;
use wxdata::regionstats::RegionSamples;

/// Which moments the histogram and the scatter plot are showing, by index into the samples'
/// moments. Session state: a new box keeps the choice where the moment is still there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionStatsUi {
    pub hist: usize,
    pub x: usize,
    pub y: usize,
}

impl Default for RegionStatsUi {
    fn default() -> Self {
        Self {
            hist: 0,
            x: 0,
            y: 1,
        }
    }
}

impl RegionStatsUi {
    /// Pull every index back inside `n` moments, for a new box whose volume had fewer.
    pub fn clamp_to(&mut self, n: usize) {
        let last = n.saturating_sub(1);
        self.hist = self.hist.min(last);
        self.x = self.x.min(last);
        self.y = self.y.min(last);
    }
}

/// The pairs analysts reach for first: rain against hail and big drops, meteorological against
/// non-meteorological echo, drop size against liquid water, and rotation against debris.
const QUICK_PAIRS: [(Moment, Moment); 4] = [
    (Moment::Reflectivity, Moment::DifferentialReflectivity),
    (Moment::Reflectivity, Moment::CorrelationCoefficient),
    (
        Moment::DifferentialReflectivity,
        Moment::SpecificDifferentialPhase,
    ),
    (Moment::Velocity, Moment::CorrelationCoefficient),
];

/// Most points the scatter plot draws; past this every n-th pair stands in for the rest, which
/// shows the same shape at a fraction of the painting.
const MAX_POINTS: usize = 12_000;

fn fmt(v: f32, m: Moment) -> String {
    match m {
        Moment::CorrelationCoefficient => format!("{v:.3}"),
        Moment::DifferentialReflectivity | Moment::SpecificDifferentialPhase => format!("{v:.2}"),
        _ => format!("{v:.1}"),
    }
}

fn label(m: Moment) -> &'static str {
    crate::products::info(m).short
}

/// Show the window. Returns `false` when it should close.
pub fn show(
    ctx: &egui::Context,
    s: &RegionSamples,
    st: &mut RegionStatsUi,
    drawer: &mut crate::ui::drawer::Drawer,
) -> bool {
    let mut open = true;
    let Some(window) = drawer.page_sized(
        ctx,
        "Region statistics",
        &mut open,
        false,
        520.0,
        egui::Window::new("Region statistics"),
    ) else {
        return open;
    };
    st.clamp_to(s.moments.len());
    window.show(ctx, |ui| {
        let (w, so, e, n) = s.bbox;
        let mid = ((so + n) / 2.0).to_radians().cos();
        ui.horizontal(|ui| {
            ui.label(format!(
                "{} gates · {:.1}° tilt · {:.0} × {:.0} km",
                s.rows.len(),
                s.elevation_deg,
                (e - w) * 111.32 * mid,
                (n - so) * 110.57
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                crate::ui::csv_buttons(
                    ui,
                    "region.csv",
                    "Every gate in the box: position, then each moment",
                    || s.to_csv(),
                );
            });
        });
        ui.separator();
        summary_table(ui, s);
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.strong("Histogram");
            for (i, m) in s.moments.iter().enumerate() {
                if ui.selectable_label(st.hist == i, label(*m)).clicked() {
                    st.hist = i;
                }
            }
        });
        histogram(ui, s, st.hist);
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.strong("Scatter");
            for (a, b) in QUICK_PAIRS {
                if let (Some(x), Some(y)) = (s.index_of(a), s.index_of(b)) {
                    let text = format!("{}–{}", label(a), label(b));
                    if ui.selectable_label((st.x, st.y) == (x, y), text).clicked() {
                        (st.x, st.y) = (x, y);
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            moment_combo(ui, "region_x", "X", s, &mut st.x);
            moment_combo(ui, "region_y", "Y", s, &mut st.y);
            let pairs = s.pairs(st.x, st.y).len();
            match s.correlation(st.x, st.y) {
                Some(r) => ui.weak(format!("r = {r:.2} over {pairs} gates")),
                None => ui.weak(format!("{pairs} gates with both")),
            };
        });
        scatter(ui, s, st.x, st.y);
    });
    open
}

fn moment_combo(ui: &mut egui::Ui, id: &str, text: &str, s: &RegionSamples, i: &mut usize) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(format!("{text}: {}", label(s.moments[*i])))
        .show_ui(ui, |ui| {
            for (j, m) in s.moments.iter().enumerate() {
                ui.selectable_value(i, j, crate::products::info(*m).name);
            }
        });
}

fn summary_table(ui: &mut egui::Ui, s: &RegionSamples) {
    egui::Grid::new("region_summary")
        .striped(true)
        .num_columns(8)
        .show(ui, |ui| {
            for h in ["", "gates", "min", "10%", "median", "90%", "max", "mean"] {
                ui.strong(h);
            }
            ui.end_row();
            for (i, m) in s.moments.iter().enumerate() {
                ui.label(label(*m)).on_hover_text(format!(
                    "{} ({})",
                    crate::products::info(*m).name,
                    m.units()
                ));
                match s.summary(i) {
                    Some(v) => {
                        ui.label(v.n.to_string());
                        for x in [v.min, v.p10, v.median, v.p90, v.max, v.mean] {
                            ui.label(fmt(x, *m));
                        }
                    }
                    None => {
                        ui.weak("0");
                        for _ in 0..6 {
                            ui.weak("—");
                        }
                    }
                }
                ui.end_row();
            }
        });
}

fn histogram(ui: &mut egui::Ui, s: &RegionSamples, i: usize) {
    let m = s.moments[i];
    let Some(h) = s.histogram(i, 40) else {
        ui.weak(format!("No {} in this box.", label(m)));
        return;
    };
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 110.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    let plot = rect.shrink2(egui::vec2(4.0, 14.0));
    let peak = h.counts.iter().copied().max().unwrap_or(1).max(1) as f32;
    let bw = plot.width() / h.counts.len() as f32;
    let fill = ui.visuals().selection.bg_fill;
    for (b, c) in h.counts.iter().enumerate() {
        if *c == 0 {
            continue;
        }
        let top = plot.bottom() - plot.height() * (*c as f32 / peak);
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(plot.left() + b as f32 * bw, top),
                egui::pos2(plot.left() + (b + 1) as f32 * bw - 1.0, plot.bottom()),
            ),
            0.0,
            fill,
        );
    }
    let text = ui.visuals().weak_text_color();
    let font = egui::FontId::proportional(10.0);
    painter.text(
        rect.left_bottom(),
        egui::Align2::LEFT_BOTTOM,
        fmt(h.lo, m),
        font.clone(),
        text,
    );
    painter.text(
        rect.right_bottom(),
        egui::Align2::RIGHT_BOTTOM,
        format!("{} {}", fmt(h.hi, m), m.units()),
        font.clone(),
        text,
    );
    painter.text(
        rect.left_top(),
        egui::Align2::LEFT_TOP,
        format!("{:.0} gates", peak),
        font,
        text,
    );
}

fn scatter(ui: &mut egui::Ui, s: &RegionSamples, xi: usize, yi: usize) {
    let (mx, my) = (s.moments[xi], s.moments[yi]);
    let pairs = s.pairs(xi, yi);
    if pairs.is_empty() {
        ui.weak(format!(
            "No gate in this box has both {} and {}.",
            label(mx),
            label(my)
        ));
        return;
    }
    let (xlo, xhi) = extent(pairs.iter().map(|p| p.0));
    let (ylo, yhi) = extent(pairs.iter().map(|p| p.1));
    let side = ui.available_width().min(360.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), side), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let plot = egui::Rect::from_min_max(
        rect.left_top() + egui::vec2(36.0, 6.0),
        rect.right_bottom() - egui::vec2(6.0, 18.0),
    );
    painter.rect_stroke(
        plot,
        0.0,
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Inside,
    );
    let to_screen = |x: f32, y: f32| {
        egui::pos2(
            plot.left() + plot.width() * (x - xlo) / (xhi - xlo),
            plot.bottom() - plot.height() * (y - ylo) / (yhi - ylo),
        )
    };
    let step = pairs.len().div_ceil(MAX_POINTS).max(1);
    let dot = ui.visuals().selection.bg_fill.gamma_multiply(0.55);
    for (x, y) in pairs.iter().step_by(step) {
        painter.circle_filled(to_screen(*x, *y), 1.4, dot);
    }
    let text = ui.visuals().weak_text_color();
    let font = egui::FontId::proportional(10.0);
    painter.text(
        egui::pos2(plot.left(), rect.bottom()),
        egui::Align2::LEFT_BOTTOM,
        fmt(xlo, mx),
        font.clone(),
        text,
    );
    painter.text(
        egui::pos2(plot.right(), rect.bottom()),
        egui::Align2::RIGHT_BOTTOM,
        format!("{} {} {}", fmt(xhi, mx), label(mx), mx.units()),
        font.clone(),
        text,
    );
    painter.text(
        egui::pos2(rect.left(), plot.bottom()),
        egui::Align2::LEFT_BOTTOM,
        fmt(ylo, my),
        font.clone(),
        text,
    );
    painter.text(
        egui::pos2(rect.left(), plot.top()),
        egui::Align2::LEFT_TOP,
        format!("{}\n{}", fmt(yhi, my), label(my)),
        font,
        text,
    );
    // Where the pointer is, in both moments' units — reading a cluster off the plot.
    if let Some(p) = response.hover_pos().filter(|p| plot.contains(*p)) {
        let x = xlo + (p.x - plot.left()) / plot.width() * (xhi - xlo);
        let y = ylo + (plot.bottom() - p.y) / plot.height() * (yhi - ylo);
        response.on_hover_text(format!(
            "{} {}, {} {}",
            label(mx),
            fmt(x, mx),
            label(my),
            fmt(y, my)
        ));
    }
}

/// Min and max of `v`, widened by half a unit when they are equal so a plot axis has length.
fn extent(v: impl Iterator<Item = f32>) -> (f32, f32) {
    let (lo, hi) = v.fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
    if hi > lo {
        (lo, hi)
    } else {
        (lo - 0.5, lo + 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wxdata::regionstats::GateRow;

    fn samples() -> RegionSamples {
        let rows = (0..50)
            .map(|i| GateRow {
                lon: -97.5,
                lat: 35.1 + i as f64 * 0.001,
                range_km: 10.0 + i as f32 * 0.25,
                az_deg: 0.0,
                values: vec![
                    Some(20.0 + i as f32),
                    Some(i as f32 * 0.05),
                    // CC missing on a few gates.
                    (i % 10 != 0).then_some(0.99 - i as f32 * 0.002),
                ],
            })
            .collect();
        RegionSamples {
            moments: vec![
                Moment::Reflectivity,
                Moment::DifferentialReflectivity,
                Moment::CorrelationCoefficient,
            ],
            rows,
            elevation_deg: 0.5,
            bbox: (-97.55, 35.1, -97.45, 35.15),
        }
    }

    /// Render one frame and return every string drawn.
    fn texts(s: &RegionSamples, st: &mut RegionStatsUi) -> Vec<String> {
        let ctx = egui::Context::default();
        let mut drawer = crate::ui::drawer::Drawer::default();
        let mut out = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 1000.0),
                )),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                assert!(show(ui.ctx(), s, st, &mut drawer));
            });
            out = output
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
                    _ => None,
                })
                .collect();
        }
        out
    }

    #[test]
    fn the_window_shows_the_table_the_pairs_that_exist_and_the_correlation() {
        let s = samples();
        let mut st = RegionStatsUi::default();
        let t = texts(&s, &mut st);
        assert!(t.iter().any(|x| x.starts_with("50 gates")), "{t:?}");
        assert!(t.iter().any(|x| x == "median"), "{t:?}");
        // Only the quick pairs whose moments are in the box are offered.
        let pair = |a: Moment, b: Moment| format!("{}–{}", label(a), label(b));
        assert!(t.contains(&pair(
            Moment::Reflectivity,
            Moment::DifferentialReflectivity
        )));
        assert!(t.contains(&pair(Moment::Reflectivity, Moment::CorrelationCoefficient)));
        assert!(!t.contains(&pair(Moment::Velocity, Moment::CorrelationCoefficient)));
        // REF against ZDR rises in lockstep here.
        assert!(t.iter().any(|x| x.starts_with("r = 1.00")), "{t:?}");
    }

    #[test]
    fn a_choice_past_a_smaller_boxs_moments_is_pulled_back() {
        let mut st = RegionStatsUi {
            hist: 6,
            x: 5,
            y: 4,
        };
        st.clamp_to(3);
        assert_eq!(
            st,
            RegionStatsUi {
                hist: 2,
                x: 2,
                y: 2
            }
        );
        assert_eq!(extent([3.0f32, 3.0].into_iter()), (2.5, 3.5));
        assert_eq!(extent([1.0f32, 4.0, 2.0].into_iter()), (1.0, 4.0));
    }
}
