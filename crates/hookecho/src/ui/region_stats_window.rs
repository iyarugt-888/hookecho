//! Region statistics window (ROADMAP_NEW C4): every gate of the displayed tilt inside a box drawn
//! on the map, across every moment — a summary table, a histogram of one moment, a scatter plot of
//! two against each other, and the gates themselves as CSV. The numbers come from
//! [`wxdata::regionstats`]; this only draws them.

use crate::ui::workstation::{self as ws, Tokens};
use egui::{Rect, Sense, Stroke};
use std::collections::HashMap;
use std::sync::Arc;
use wxdata::level2::Moment;
use wxdata::regionstats::{GateRow, Histogram, RegionSamples, Summary};

/// A scatter pair's values and their correlation.
pub struct Pairs {
    pub v: Vec<(f32, f32)>,
    pub r: Option<f32>,
}

/// What the workstation charts derive from one box, computed once rather than every frame: a
/// big box is 150 000 gates, and sorting each moment of it per frame costs more than the map.
/// It notices a new box itself (by size, extent and tilt) and starts over.
#[derive(Default)]
pub struct RegionCache {
    key: Option<(usize, [u64; 4], u32)>,
    summaries: HashMap<usize, Option<Summary>>,
    hists: HashMap<usize, Option<Histogram>>,
    pairs: HashMap<(usize, usize), Arc<Pairs>>,
}

impl RegionCache {
    fn check(&mut self, s: &RegionSamples) {
        let (w, so, e, n) = s.bbox;
        let key = (
            s.rows.len(),
            [w.to_bits(), so.to_bits(), e.to_bits(), n.to_bits()],
            s.elevation_deg.to_bits(),
        );
        if self.key != Some(key) {
            *self = RegionCache {
                key: Some(key),
                ..Default::default()
            };
        }
    }

    pub fn summary(&mut self, s: &RegionSamples, i: usize) -> Option<Summary> {
        self.check(s);
        *self.summaries.entry(i).or_insert_with(|| s.summary(i))
    }

    pub fn histogram(&mut self, s: &RegionSamples, i: usize) -> Option<Histogram> {
        self.check(s);
        self.hists
            .entry(i)
            .or_insert_with(|| s.histogram(i, 40))
            .clone()
    }

    pub fn pairs(&mut self, s: &RegionSamples, x: usize, y: usize) -> Arc<Pairs> {
        self.check(s);
        self.pairs
            .entry((x, y))
            .or_insert_with(|| {
                Arc::new(Pairs {
                    v: s.pairs(x, y),
                    r: s.correlation(x, y),
                })
            })
            .clone()
    }
}

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

/// The gates a hovered histogram bar or scatter cell stands for, which the map outlines.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Highlight {
    /// `(moment index, lo, hi)`: a gate is in when each named moment lies in `lo..=hi`.
    pub conds: Vec<(usize, f32, f32)>,
}

impl Highlight {
    pub fn matches(&self, row: &GateRow) -> bool {
        self.conds.iter().all(|&(i, lo, hi)| {
            row.values
                .get(i)
                .copied()
                .flatten()
                .is_some_and(|v| v >= lo && v <= hi)
        })
    }
}

/// The Region window's body in the workstation's style: the box, a summary table whose rows
/// pick the histogram, the histogram, and the scatter plot. Each section folds, each chart
/// expands on click, and hovering a bar or a scatter cell sets `hl` so the map outlines those
/// gates.
pub fn dock_body(
    ui: &mut egui::Ui,
    t: &Tokens,
    s: &RegionSamples,
    cache: &mut RegionCache,
    st: &mut RegionStatsUi,
    hl: &mut Option<Highlight>,
) {
    st.clamp_to(s.moments.len());
    let (w, so, e, n) = s.bbox;
    let mid = ((so + n) / 2.0).to_radians().cos();
    ui.label(ws::text(
        format!(
            "{} gates \u{b7} {:.1}\u{b0} tilt \u{b7} {:.0} \u{d7} {:.0} km",
            s.rows.len(),
            s.elevation_deg,
            (e - w) * 111.32 * mid,
            (n - so) * 110.57
        ),
        11.5,
        t.text_dim,
    ));
    ui.horizontal(|ui| {
        crate::ui::csv_buttons(
            ui,
            "region.csv",
            "Every gate in the box: position, then each moment",
            || s.to_csv(),
        );
    });
    let count = s.moments.len().to_string();
    ws::fold_section(
        ui,
        t,
        "region_summary",
        "Summary",
        Some(&count),
        true,
        |ui| {
            summary_ws(ui, t, s, cache, st);
        },
    );
    let m = label(s.moments[st.hist]);
    ws::fold_section(ui, t, "region_hist", "Histogram", Some(m), true, |ui| {
        let labels: Vec<&str> = s.moments.iter().map(|m| label(*m)).collect();
        if let Some(i) = ws::chips(ui, t, &labels, Some(st.hist)) {
            st.hist = i;
        }
        ui.add_space(4.0);
        ws_histogram(ui, t, "region", s, cache, st.hist, hl);
    });
    let both = cache.pairs(s, st.x, st.y);
    let pairs = both.v.len();
    let r = both
        .r
        .map_or_else(|| format!("{pairs}"), |r| format!("r {r:.2}"));
    ws::fold_section(ui, t, "region_scatter", "Scatter", Some(&r), true, |ui| {
        let quick: Vec<(usize, usize, String)> = QUICK_PAIRS
            .iter()
            .filter_map(|&(a, b)| {
                Some((
                    s.index_of(a)?,
                    s.index_of(b)?,
                    format!("{}\u{2013}{}", label(a), label(b)),
                ))
            })
            .collect();
        let names: Vec<&str> = quick.iter().map(|q| q.2.as_str()).collect();
        let on = quick.iter().position(|q| (q.0, q.1) == (st.x, st.y));
        if let Some(i) = ws::chips(ui, t, &names, on) {
            (st.x, st.y) = (quick[i].0, quick[i].1);
        }
        ui.horizontal(|ui| {
            moment_combo(ui, "region_x", "X", s, &mut st.x);
            moment_combo(ui, "region_y", "Y", s, &mut st.y);
            if ws::button(ui, t, egui_phosphor::regular::ARROWS_LEFT_RIGHT, 0.0)
                .on_hover_text("Swap the axes")
                .clicked()
            {
                (st.x, st.y) = (st.y, st.x);
            }
        });
        ui.add_space(4.0);
        ws_scatter(ui, t, "region", s, cache, (st.x, st.y), hl);
        let both = cache.pairs(s, st.x, st.y);
        let pairs = both.v.len();
        let text = match both.r {
            Some(r) => format!("r = {r:.2} over {pairs} gates with both"),
            None => format!("{pairs} gates with both"),
        };
        ui.label(ws::text(text, 10.5, t.text_faint));
    });
}

/// The summary as the workstation draws tables: a row per moment (median, the 10–90% spread,
/// the max), the rest on hover. Clicking a row shows its histogram.
fn summary_ws(
    ui: &mut egui::Ui,
    t: &Tokens,
    s: &RegionSamples,
    cache: &mut RegionCache,
    st: &mut RegionStatsUi,
) {
    let w = ui.available_width();
    let cols = [6.0, w * 0.22, w * 0.46, w * 0.80];
    let font = egui::FontId::monospace(11.5);
    let (head, _) = ui.allocate_exact_size(egui::vec2(w, 16.0), Sense::hover());
    for (x, h) in cols.iter().zip(["", "MEDIAN", "10\u{2013}90%", "MAX"]) {
        ui.painter().text(
            egui::pos2(head.left() + x, head.center().y),
            egui::Align2::LEFT_CENTER,
            h,
            egui::FontId::proportional(9.5),
            t.text_faint,
        );
    }
    for (i, m) in s.moments.iter().enumerate() {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 20.0), Sense::click());
        let on = st.hist == i;
        if on {
            ui.painter().rect_filled(rect, 3.0, t.accent_soft());
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, 3.0, t.field_hi);
        }
        let sum = cache.summary(s, i);
        let cells = match &sum {
            Some(v) => [
                label(*m).to_string(),
                fmt(v.median, *m),
                format!("{} \u{2013} {}", fmt(v.p10, *m), fmt(v.p90, *m)),
                fmt(v.max, *m),
            ],
            None => [
                label(*m).to_string(),
                "\u{2014}".into(),
                "no gates".into(),
                String::new(),
            ],
        };
        for (k, (x, c)) in cols.iter().zip(cells).enumerate() {
            let color = match (k, on, sum.is_some()) {
                (0, true, _) => t.accent,
                (_, _, false) => t.text_faint,
                (0, false, _) => t.text_dim,
                _ => t.text,
            };
            ui.painter().text(
                egui::pos2(rect.left() + x, rect.center().y),
                egui::Align2::LEFT_CENTER,
                c,
                font.clone(),
                color,
            );
        }
        let info = crate::products::info(*m);
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let resp = match &sum {
            Some(v) => resp.on_hover_text(format!(
                "{} ({})\n{} gates \u{b7} min {} \u{b7} mean {} \u{b7} max {}\nClick for its histogram",
                info.name,
                m.units(),
                v.n,
                fmt(v.min, *m),
                fmt(v.mean, *m),
                fmt(v.max, *m)
            )),
            None => resp.on_hover_text(format!("No {} gate in this box", info.name)),
        };
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, info.name)
        });
        if resp.clicked() {
            st.hist = i;
        }
    }
}

/// Whether the chart `key` is expanded, flipping it when `resp` was clicked.
fn expanded_toggle(ui: &egui::Ui, key: egui::Id, resp: &egui::Response) -> bool {
    let mut on = ui.ctx().data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    if resp.clicked() {
        on = !on;
        ui.ctx().data_mut(|d| d.insert_temp(key, on));
    }
    on
}

/// Moment `i`'s histogram, workstation-styled and interactive: hovering a bar reads its range,
/// count and share and sets `hl` to its gates; dashed lines mark the 10th, 50th and 90th
/// percentiles; a click expands it. Also the Cell window's core distribution.
pub fn ws_histogram(
    ui: &mut egui::Ui,
    t: &Tokens,
    id: impl std::hash::Hash + std::fmt::Debug,
    s: &RegionSamples,
    cache: &mut RegionCache,
    i: usize,
    hl: &mut Option<Highlight>,
) {
    let m = s.moments[i];
    let Some(h) = cache.histogram(s, i) else {
        ui.label(ws::text(
            format!("No {} in this box", label(m)),
            11.0,
            t.text_faint,
        ));
        return;
    };
    let key = ui.make_persistent_id(("ws_hist", id));
    let size = egui::vec2(ui.available_width(), 96.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let expanded = expanded_toggle(ui, key, &resp);
    // The expanded chart continues below the clickable strip; hover reads the whole of it.
    let rect = if expanded {
        let extra = ui
            .allocate_exact_size(egui::vec2(size.x, 104.0), Sense::hover())
            .0;
        rect.union(extra)
    } else {
        rect
    };
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, t.field);
    let hovered = ui.rect_contains_pointer(rect);
    if hovered {
        p.rect_stroke(
            rect,
            3.0,
            Stroke::new(1.0, t.line),
            egui::StrokeKind::Inside,
        );
    }
    let plot = Rect::from_min_max(
        rect.left_top() + egui::vec2(6.0, 18.0),
        rect.right_bottom() - egui::vec2(6.0, 14.0),
    );
    let total: usize = h.counts.iter().sum();
    let peak = h.counts.iter().copied().max().unwrap_or(1).max(1) as f32;
    let n = h.counts.len();
    let bw = plot.width() / n as f32;
    let pointer = ui.ctx().pointer_hover_pos().filter(|_| hovered);
    let hover_bin = pointer
        .filter(|q| q.x >= plot.left() && q.x <= plot.right())
        .map(|q| (((q.x - plot.left()) / bw) as usize).min(n - 1));
    for (b, c) in h.counts.iter().enumerate() {
        if *c == 0 {
            continue;
        }
        let top = plot.bottom() - plot.height() * (*c as f32 / peak);
        let col = if Some(b) == hover_bin {
            t.accent
        } else {
            t.accent.gamma_multiply(0.55)
        };
        p.rect_filled(
            Rect::from_min_max(
                egui::pos2(plot.left() + b as f32 * bw, top),
                egui::pos2(plot.left() + (b + 1) as f32 * bw - 1.0, plot.bottom()),
            ),
            0.0,
            col,
        );
    }
    if let Some(v) = cache.summary(s, i).filter(|_| h.hi > h.lo) {
        for (q, name) in [(v.p10, "p10"), (v.median, "med"), (v.p90, "p90")] {
            let x = plot.left() + plot.width() * (q - h.lo) / (h.hi - h.lo);
            p.extend(egui::Shape::dashed_line(
                &[egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
                Stroke::new(1.0, t.text_dim),
                3.0,
                3.0,
            ));
            if expanded {
                p.text(
                    egui::pos2(x + 2.0, plot.top()),
                    egui::Align2::LEFT_TOP,
                    format!("{name} {}", fmt(q, m)),
                    egui::FontId::monospace(9.5),
                    t.text_dim,
                );
            }
        }
    }
    let small = egui::FontId::monospace(9.5);
    p.text(
        rect.left_top() + egui::vec2(6.0, 3.0),
        egui::Align2::LEFT_TOP,
        format!("{} ({})", label(m), m.units()),
        egui::FontId::proportional(10.5),
        t.text_dim,
    );
    p.text(
        rect.right_top() + egui::vec2(-6.0, 3.0),
        egui::Align2::RIGHT_TOP,
        format!("{total} gates \u{b7} peak {peak:.0}"),
        small.clone(),
        t.text_faint,
    );
    p.text(
        rect.left_bottom() + egui::vec2(6.0, -2.0),
        egui::Align2::LEFT_BOTTOM,
        fmt(h.lo, m),
        small.clone(),
        t.text_faint,
    );
    p.text(
        rect.right_bottom() + egui::vec2(-6.0, -2.0),
        egui::Align2::RIGHT_BOTTOM,
        fmt(h.hi, m),
        small,
        t.text_faint,
    );
    let mut resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if let Some(b) = hover_bin {
        let lo = h.lo + b as f32 * h.bin_width();
        let hi = lo + h.bin_width();
        let c = h.counts[b];
        resp = resp.on_hover_text(format!(
            "{} \u{2013} {} {}\n{c} gates ({:.1}%), outlined on the map\n{}",
            fmt(lo, m),
            fmt(hi, m),
            m.units(),
            100.0 * c as f32 / total.max(1) as f32,
            if expanded {
                "Click to fold"
            } else {
                "Click to expand"
            }
        ));
        if c > 0 {
            *hl = Some(Highlight {
                conds: vec![(i, lo, hi)],
            });
        }
    }
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, expanded, "Histogram")
    });
}

/// Moment `xi` against `yi` as a density plot (gates binned to cells, brighter where more fall,
/// so every gate counts however many there are), workstation-styled: hovering a cell reads its
/// ranges and count and sets `hl` to its gates; a click expands it to a square.
pub fn ws_scatter(
    ui: &mut egui::Ui,
    t: &Tokens,
    id: impl std::hash::Hash + std::fmt::Debug,
    s: &RegionSamples,
    cache: &mut RegionCache,
    (xi, yi): (usize, usize),
    hl: &mut Option<Highlight>,
) {
    let (mx, my) = (s.moments[xi], s.moments[yi]);
    let both = cache.pairs(s, xi, yi);
    let pairs = &both.v;
    if pairs.is_empty() {
        ui.label(ws::text(
            format!("No gate here has both {} and {}", label(mx), label(my)),
            11.0,
            t.text_faint,
        ));
        return;
    }
    let (xlo, xhi) = extent(pairs.iter().map(|p| p.0));
    let (ylo, yhi) = extent(pairs.iter().map(|p| p.1));
    let key = ui.make_persistent_id(("ws_scatter", id));
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 180.0), Sense::click());
    let expanded = expanded_toggle(ui, key, &resp);
    let rect = if expanded {
        let extra = ui
            .allocate_exact_size(egui::vec2(w, (w - 180.0).max(60.0)), Sense::hover())
            .0;
        rect.union(extra)
    } else {
        rect
    };
    let g = if expanded { 64 } else { 40 };
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, t.field);
    let plot = Rect::from_min_max(
        rect.left_top() + egui::vec2(38.0, 8.0),
        rect.right_bottom() - egui::vec2(8.0, 18.0),
    );
    let cell_of = |x: f32, y: f32| {
        let f = |v: f32, lo: f32, hi: f32| {
            (((v - lo) / (hi - lo)) * g as f32)
                .floor()
                .clamp(0.0, g as f32 - 1.0) as usize
        };
        (f(x, xlo, xhi), f(y, ylo, yhi))
    };
    let mut grid = vec![0u32; g * g];
    for (x, y) in pairs {
        let (cx, cy) = cell_of(*x, *y);
        grid[cy * g + cx] += 1;
    }
    let max = grid.iter().copied().max().unwrap_or(1).max(1) as f32;
    let (cw, ch) = (plot.width() / g as f32, plot.height() / g as f32);
    let cell_rect = |cx: usize, cy: usize| {
        Rect::from_min_size(
            egui::pos2(
                plot.left() + cx as f32 * cw,
                plot.bottom() - (cy + 1) as f32 * ch,
            ),
            egui::vec2(cw, ch),
        )
    };
    let grid_line = Stroke::new(1.0, t.line_soft);
    for k in 1..4 {
        let f = k as f32 / 4.0;
        let x = plot.left() + plot.width() * f;
        let y = plot.top() + plot.height() * f;
        p.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            grid_line,
        );
        p.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            grid_line,
        );
    }
    for cy in 0..g {
        for cx in 0..g {
            let c = grid[cy * g + cx];
            if c > 0 {
                let a = (c as f32 / max).sqrt();
                p.rect_filled(
                    cell_rect(cx, cy),
                    0.0,
                    t.accent.gamma_multiply(0.18 + 0.82 * a),
                );
            }
        }
    }
    p.rect_stroke(
        plot,
        0.0,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Outside,
    );
    let small = egui::FontId::monospace(9.5);
    p.text(
        egui::pos2(plot.left(), rect.bottom() - 2.0),
        egui::Align2::LEFT_BOTTOM,
        fmt(xlo, mx),
        small.clone(),
        t.text_faint,
    );
    p.text(
        egui::pos2(plot.right(), rect.bottom() - 2.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("{} {}", fmt(xhi, mx), label(mx)),
        small.clone(),
        t.text_dim,
    );
    p.text(
        egui::pos2(rect.left() + 4.0, plot.bottom()),
        egui::Align2::LEFT_BOTTOM,
        fmt(ylo, my),
        small.clone(),
        t.text_faint,
    );
    p.text(
        egui::pos2(rect.left() + 4.0, plot.top()),
        egui::Align2::LEFT_TOP,
        format!("{}\n{}", fmt(yhi, my), label(my)),
        small,
        t.text_dim,
    );
    let mut resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let pointer = ui
        .ctx()
        .pointer_hover_pos()
        .filter(|q| plot.contains(*q) && ui.rect_contains_pointer(rect));
    if let Some(q) = pointer {
        let x = xlo + (q.x - plot.left()) / plot.width() * (xhi - xlo);
        let y = ylo + (plot.bottom() - q.y) / plot.height() * (yhi - ylo);
        let (cx, cy) = cell_of(x, y);
        p.rect_stroke(
            cell_rect(cx, cy).expand(1.0),
            0.0,
            Stroke::new(1.0, egui::Color32::WHITE),
            egui::StrokeKind::Outside,
        );
        let (dx, dy) = ((xhi - xlo) / g as f32, (yhi - ylo) / g as f32);
        let (x0, y0) = (xlo + cx as f32 * dx, ylo + cy as f32 * dy);
        let c = grid[cy * g + cx];
        resp = resp.on_hover_text(format!(
            "{} {} \u{2013} {}\n{} {} \u{2013} {}\n{c} gates, outlined on the map\n{}",
            label(mx),
            fmt(x0, mx),
            fmt(x0 + dx, mx),
            label(my),
            fmt(y0, my),
            fmt(y0 + dy, my),
            if expanded {
                "Click to fold"
            } else {
                "Click to expand"
            }
        ));
        if c > 0 {
            *hl = Some(Highlight {
                conds: vec![(xi, x0, x0 + dx), (yi, y0, y0 + dy)],
            });
        }
    }
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, expanded, "Scatter plot")
    });
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
    fn the_dock_body_draws_the_table_the_charts_and_the_correlation() {
        let s = samples();
        let mut st = RegionStatsUi::default();
        let ctx = egui::Context::default();
        let t = ws::Tokens::new(egui::Color32::from_rgb(0x2F, 0x81, 0xF7));
        let mut cache = RegionCache::default();
        let mut got = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(360.0, 1400.0),
                )),
                ..Default::default()
            };
            let out = ctx.run_ui(input, |ui| {
                let mut hl = None;
                dock_body(ui, &t, &s, &mut cache, &mut st, &mut hl);
                assert_eq!(hl, None, "nothing hovered");
            });
            got = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
                    _ => None,
                })
                .collect();
        }
        assert!(got.iter().any(|x| x.starts_with("50 gates")), "{got:?}");
        assert!(got.iter().any(|x| x == "MEDIAN"), "{got:?}");
        assert!(
            got.iter().any(|x| x.starts_with("50 gates \u{b7} peak")),
            "{got:?}"
        );
        assert!(got.iter().any(|x| x.starts_with("r = 1.00")), "{got:?}");
    }

    #[test]
    fn a_highlight_keeps_gates_inside_every_range() {
        let s = samples();
        let h = Highlight {
            conds: vec![(0, 30.0, 40.0), (2, 0.0, 1.0)],
        };
        // REF 30..=40 is gates 10..=20; CC is missing on gates 10 and 20.
        let n = s.rows.iter().filter(|r| h.matches(r)).count();
        assert_eq!(n, 9);
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
