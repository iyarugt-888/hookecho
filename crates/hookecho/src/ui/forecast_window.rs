//! Point forecast window: tap the map, get the plain forecast for that spot.
//!
//! The hourly strip is hand-painted rather than pulled in as a plotting dependency — it's one
//! polyline and a row of bars, and the app already draws its own hodographs and cross-sections.

use chrono::{DateTime, Utc};
use egui::{Align2, Color32, FontId, RichText, Sense, Stroke, Vec2};
use wxdata::forecast::PointForecast;

/// What the app is currently holding for the tapped point.
pub enum State {
    Loading,
    Ready(Box<PointForecast>),
    Failed(String),
}

/// The model-meteogram picker: which model, which field, how far out. A second data source from
/// the "This week" NWS blend above — that is one forecaster-reconciled outlook; this is one
/// specific model's own raw run, the same numbers the map's Global-model layers draw, sampled at
/// a point and strung into a line instead of painted as a grid.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModelSeriesUi {
    pub model: wxdata::global::GlobalModel,
    pub field: wxdata::global::GlobalField,
    pub period: Period,
}

impl Default for ModelSeriesUi {
    fn default() -> Self {
        Self {
            model: wxdata::global::GlobalModel::Gfs,
            field: wxdata::global::GlobalField::Temp2m,
            period: Period::Day3,
        }
    }
}

/// How far the meteogram reaches, in 3-hourly steps — the same step the map's own forecast-hour
/// slider uses.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Period {
    Day1,
    Day3,
    Day5,
}

impl Period {
    /// The forecast hours to fetch, ascending — the first one is what
    /// [`wxdata::global::fetch_point_series`] pins the whole series' cycle to.
    pub fn hours(self) -> Vec<u16> {
        let max = match self {
            Period::Day1 => 24,
            Period::Day3 => 72,
            Period::Day5 => 120,
        };
        (0..=max).step_by(3).collect()
    }

    fn label(self) -> &'static str {
        match self {
            Period::Day1 => "24h",
            Period::Day3 => "3 day",
            Period::Day5 => "5 day",
        }
    }
}

/// One point's worth of one model's one field, across the tapped period — `None` per hour where
/// that particular request failed (not yet posted, past the model's own range), so a gap breaks
/// the line rather than the whole series erroring over one missing hour.
pub enum SeriesState {
    Idle,
    Loading,
    Ready(Vec<(DateTime<Utc>, Option<f32>)>),
    Failed(String),
}

/// The ensemble-plume picker: which field, how far out. The plume is GEFS's mean and spread at the
/// tapped point (ROADMAP_NEW F7's "point plume"): where the deterministic meteogram above shows one
/// model's one answer, this shows how much the 31 members disagree about it.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlumeUi {
    pub field: wxdata::ensemble::EnsembleField,
    pub period: Period,
}

impl Default for PlumeUi {
    fn default() -> Self {
        Self {
            field: wxdata::ensemble::EnsembleField::Temp2m,
            period: Period::Day3,
        }
    }
}

impl Period {
    /// Plume leads: six-hourly, since each lead is two reads and a plume is about the envelope,
    /// not the diurnal wiggle.
    pub fn plume_hours(self) -> Vec<u16> {
        let max = match self {
            Period::Day1 => 24,
            Period::Day3 => 72,
            Period::Day5 => 120,
        };
        (0..=max).step_by(6).collect()
    }
}

/// The plume for the tapped point. Gaps (`None`) break the band rather than the whole plume.
pub enum PlumeState {
    Idle,
    Loading,
    Ready(Vec<wxdata::ensemble::PlumePoint>),
    Failed(String),
}

/// What [`show`] found this frame: whether the window is still open, and whether a picker
/// changed — the caller owns fetching (this module has no network access of its own), so it
/// needs to know when to kick one off.
pub struct ForecastResult {
    pub open: bool,
    pub series_changed: bool,
    pub plume_changed: bool,
}

/// Show the window. `minute` is the per-minute radar-advection profile over the point (dBZ per
/// minute from now); `None` in archive or without a volume, which hides that section. `now` is the
/// nearest station's latest observation, if one arrived.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    state: &State,
    at: (f64, f64),
    tz: Option<wxdata::tz::Tz>,
    minute: Option<&[Option<f32>]>,
    now: Option<(&str, &wxdata::obs::Observation)>,
    popovers: &mut crate::ui::popover::Popovers,
    series_ui: &mut ModelSeriesUi,
    series_state: &SeriesState,
    plume_ui: &mut PlumeUi,
    plume_state: &PlumeState,
) -> ForecastResult {
    let mut open = true;
    let mut series_changed = false;
    let mut plume_changed = false;
    popovers
        .card(ctx, "forecast", egui::Window::new("Forecast"))
        .open(&mut open)
        .default_size([460.0, 520.0])
        .show(ctx, |ui| {
            // The window is a fixed default size but the content is not — the daily list, and
            // now the model-forecast section below it, both vary with what came back. An outer
            // scroll area means a tall render never clips instead of the window having to grow
            // to fit whatever the longest possible content is.
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong(format!("{:.3}, {:.3}", at.1, at.0));
                    if let State::Ready(f) = state {
                        if !f.office.is_empty() {
                            ui.weak(format!("· {}", f.office));
                        }
                    }
                });
                if let Some((station, o)) = now {
                    ui.label(conditions_line(o, station));
                }
                ui.weak(almanac_line(at, tz, Utc::now()));
                ui.separator();
                if let Some(m) = minute {
                    minute_strip(ui, m);
                    ui.add_space(6.0);
                }
                match state {
                    State::Loading => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.weak("Fetching forecast…");
                        });
                    }
                    State::Failed(e) => {
                        ui.colored_label(Color32::from_rgb(230, 120, 120), e);
                        ui.small("Forecast services go down; tap the map again to retry.");
                    }
                    State::Ready(f) => body(ui, f, tz),
                }
                ui.separator();
                series_changed = model_series_section(ui, series_ui, series_state, tz);
                ui.separator();
                plume_changed = plume_section(ui, plume_ui, plume_state, tz);
            });
        });
    ForecastResult {
        open,
        series_changed,
        plume_changed,
    }
}

fn body(ui: &mut egui::Ui, f: &PointForecast, tz: Option<wxdata::tz::Tz>) {
    if !f.hourly.is_empty() {
        ui.label(RichText::new("Next 24 hours").strong());
        hourly_strip(ui, &f.hourly, tz);
        wind_strip(ui, &f.hourly);
        ui.add_space(6.0);
    }
    ui.label(RichText::new("This week").strong());
    ui.add_space(2.0);
    // Capped, not left to fill whatever room the window has: an uncapped scroll area here ate
    // the entire rest of the window, leaving nothing for the model-forecast section below it.
    egui::ScrollArea::vertical()
        .max_height(150.0)
        .show(ui, |ui| {
            for p in &f.daily {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [104.0, 18.0],
                        egui::Label::new(RichText::new(&p.name).strong()).selectable(false),
                    );
                    let temp =
                        RichText::new(format!("{:.0}°", p.temp_f))
                            .strong()
                            .color(if p.is_day {
                                Color32::from_rgb(245, 190, 90)
                            } else {
                                Color32::from_rgb(150, 180, 240)
                            });
                    ui.add_sized([44.0, 18.0], egui::Label::new(temp).selectable(false));
                    if let Some(pc) = p.precip_pct {
                        ui.add_sized(
                            [42.0, 18.0],
                            egui::Label::new(
                                RichText::new(format!("{pc}%"))
                                    .color(Color32::from_rgb(110, 180, 240)),
                            )
                            .selectable(false),
                        );
                    } else {
                        ui.add_sized([42.0, 18.0], egui::Label::new("").selectable(false));
                    }
                    ui.label(&p.short);
                })
                .response
                .on_hover_text(if p.wind.is_empty() {
                    p.short.clone()
                } else {
                    format!("{}\nWind {}", p.short, p.wind)
                });
            }
        });
}

/// Model/field/period pickers, the graph, and a min/max/avg line. Returns whether any picker
/// changed this frame — the caller owns fetching, so it needs to know when to kick one off.
fn model_series_section(
    ui: &mut egui::Ui,
    series_ui: &mut ModelSeriesUi,
    series_state: &SeriesState,
    tz: Option<wxdata::tz::Tz>,
) -> bool {
    use wxdata::global::GlobalModel as GM;
    let mut changed = false;
    ui.label(RichText::new("Model forecast").strong());
    ui.horizontal_wrapped(|ui| {
        for m in [GM::Gfs, GM::Ecmwf, GM::Gefs, GM::Gdps] {
            changed |= ui
                .selectable_value(&mut series_ui.model, m, m.label())
                .changed();
        }
    });
    ui.horizontal_wrapped(|ui| {
        for f in field_choices() {
            changed |= ui
                .selectable_value(&mut series_ui.field, f, f.label())
                .changed();
        }
    });
    ui.horizontal(|ui| {
        for p in [Period::Day1, Period::Day3, Period::Day5] {
            changed |= ui
                .selectable_value(&mut series_ui.period, p, p.label())
                .changed();
        }
    });
    ui.add_space(4.0);
    match series_state {
        SeriesState::Idle => {}
        SeriesState::Loading => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(format!("Fetching {}…", series_ui.model.label()));
            });
        }
        SeriesState::Failed(e) => {
            ui.colored_label(Color32::from_rgb(230, 120, 120), e);
        }
        SeriesState::Ready(points) => {
            series_chart(ui, points, series_ui.field, tz);
            series_stats(ui, points, series_ui.field);
        }
    }
    changed
}

/// Field/period pickers, the plume, and one honest sentence about what the shading means. Returns
/// whether a picker changed, like [`model_series_section`].
fn plume_section(
    ui: &mut egui::Ui,
    plume_ui: &mut PlumeUi,
    state: &PlumeState,
    tz: Option<wxdata::tz::Tz>,
) -> bool {
    let mut changed = false;
    ui.label(RichText::new("Ensemble plume (GEFS)").strong());
    ui.horizontal_wrapped(|ui| {
        for f in wxdata::ensemble::EnsembleField::ALL {
            changed |= ui
                .selectable_value(&mut plume_ui.field, f, f.label())
                .changed();
        }
    });
    ui.horizontal(|ui| {
        for p in [Period::Day1, Period::Day3, Period::Day5] {
            changed |= ui
                .selectable_value(&mut plume_ui.period, p, p.label())
                .changed();
        }
    });
    ui.add_space(4.0);
    match state {
        PlumeState::Idle => {}
        PlumeState::Loading => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Fetching the GEFS mean and spread…");
            });
        }
        PlumeState::Failed(e) => {
            ui.colored_label(Color32::from_rgb(230, 120, 120), e);
        }
        PlumeState::Ready(points) => {
            plume_chart(ui, points, plume_ui.field, tz);
            ui.weak(
                "Line: ensemble mean. Shading: one standard deviation either side — a range of \
                 plausible outcomes across the 31 members, not a limit on what can happen.",
            );
        }
    }
    changed
}

/// A native value in the units this window shows (°F for temperature, like the deterministic
/// meteogram; the field's usual unit otherwise).
fn plume_value(field: wxdata::ensemble::EnsembleField, native: f32) -> f32 {
    use wxdata::ensemble::EnsembleField as EF;
    match field {
        EF::Temp2m => crate::ui::station_card::c_to_f(native - 273.15),
        _ => field.to_display(native),
    }
}

/// A spread is a difference: convert its size, not its zero point.
fn plume_spread(field: wxdata::ensemble::EnsembleField, native: f32) -> f32 {
    use wxdata::ensemble::EnsembleField as EF;
    match field {
        EF::Temp2m => native * 1.8,
        _ => field.spread_to_display(native),
    }
}

fn plume_unit(field: wxdata::ensemble::EnsembleField) -> &'static str {
    match field {
        wxdata::ensemble::EnsembleField::Temp2m => "°F",
        _ => field.display().0,
    }
}

/// The mean as a line with a translucent band of one standard deviation around it, hand-painted
/// like [`series_chart`]. A lead with no data breaks both, rather than drawing across it.
fn plume_chart(
    ui: &mut egui::Ui,
    points: &[wxdata::ensemble::PlumePoint],
    field: wxdata::ensemble::EnsembleField,
    tz: Option<wxdata::tz::Tz>,
) {
    if points.len() < 2 {
        ui.weak("Not enough leads to draw a plume.");
        return;
    }
    // (mean, spread) per lead in display units.
    let vals: Vec<Option<(f32, f32)>> = points
        .iter()
        .map(|p| Some((plume_value(field, p.mean?), plume_spread(field, p.spread?))))
        .collect();
    let extent: Vec<f32> = vals
        .iter()
        .flatten()
        .flat_map(|(m, s)| [m - s, m + s])
        .collect();
    if extent.is_empty() {
        ui.weak("No data for this period.");
        return;
    }
    let w = ui.available_width().max(220.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 120.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, Color32::from_black_alpha(90));
    let plot = rect.shrink2(Vec2::new(6.0, 4.0));
    let axis_h = 12.0;
    let body = egui::Rect::from_min_max(
        plot.left_top(),
        egui::pos2(plot.right(), plot.bottom() - axis_h),
    );
    let (lo, hi) = extent
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    let (lo, hi) = if (hi - lo).abs() < 1.0 {
        (lo - 1.0, hi + 1.0)
    } else {
        (lo, hi)
    };
    let x_of = |i: usize| body.left() + (i as f32 + 0.5) / points.len() as f32 * body.width();
    let y_of = |v: f32| body.bottom() - ((v - lo) / (hi - lo).max(f32::EPSILON)) * body.height();

    // The band: one convex quad between each pair of neighbouring leads that both have data.
    let band = Color32::from_rgba_unmultiplied(120, 190, 230, 60);
    for i in 0..vals.len() - 1 {
        if let (Some((m0, s0)), Some((m1, s1))) = (vals[i], vals[i + 1]) {
            p.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x_of(i), y_of(m0 + s0)),
                    egui::pos2(x_of(i + 1), y_of(m1 + s1)),
                    egui::pos2(x_of(i + 1), y_of(m1 - s1)),
                    egui::pos2(x_of(i), y_of(m0 - s0)),
                ],
                band,
                Stroke::NONE,
            ));
        }
    }
    // The mean, broken at gaps.
    let color = Color32::from_rgb(120, 190, 230);
    let mut run: Vec<egui::Pos2> = Vec::new();
    for (i, v) in vals.iter().enumerate() {
        match v {
            Some((m, _)) => run.push(egui::pos2(x_of(i), y_of(*m))),
            None => {
                if run.len() > 1 {
                    p.add(egui::Shape::line(run.clone(), Stroke::new(1.8, color)));
                }
                run.clear();
            }
        }
    }
    if run.len() > 1 {
        p.add(egui::Shape::line(run, Stroke::new(1.8, color)));
    }
    // Label the range at both ends of the vertical axis, and the time along the bottom.
    let font = FontId::proportional(9.0);
    let unit = plume_unit(field);
    p.text(
        body.left_top() + Vec2::new(2.0, 0.0),
        Align2::LEFT_TOP,
        format!("{hi:.0}{unit}"),
        font.clone(),
        Color32::from_gray(190),
    );
    p.text(
        body.left_bottom() - Vec2::new(-2.0, 1.0),
        Align2::LEFT_BOTTOM,
        format!("{lo:.0}{unit}"),
        font.clone(),
        Color32::from_gray(190),
    );
    let step = (points.len() / 4).max(1);
    for (i, pt) in points.iter().enumerate() {
        if i % step != 0 {
            continue;
        }
        p.text(
            egui::pos2(x_of(i), plot.bottom() - axis_h + 1.0),
            Align2::CENTER_TOP,
            short_hour(pt.valid, tz),
            font.clone(),
            Color32::from_gray(170),
        );
    }
    // The last lead's spread, in words: how uncertain the far end is is the point of a plume.
    if let Some((m, s)) = vals.iter().rev().flatten().next() {
        ui.weak(format!("Latest lead: {m:.0}{unit} ± {s:.1}{unit}"));
    }
}

/// The fields worth graphing at a point. Precip is left out on purpose: GFS publishes
/// precipitable water and ECMWF/GDPS publish accumulated precipitation, two different physical
/// quantities sharing one map legend already (see that legend's own doc comment) — fine as a
/// single always-on color scale, not fine as a number this window would state as one "precip" line
/// with no way to say which sense it's in.
fn field_choices() -> [wxdata::global::GlobalField; 5] {
    use wxdata::global::GlobalField as GF;
    [
        GF::Temp2m,
        GF::Dewpoint2m,
        GF::Wind10m,
        GF::Mslp,
        GF::Height500,
    ]
}

/// Kelvin / m·s⁻¹ / Pa / metres → the units this window already shows everywhere else (°F, mph,
/// hPa, dam) — the same conversions the map's own legend applies for these fields
/// (`render::field_ramps`), done locally rather than pulling a render-crate dependency into a UI
/// module for one multiply-and-maybe-subtract each.
fn display_value(field: wxdata::global::GlobalField, raw: f32) -> f32 {
    use crate::ui::station_card::c_to_f;
    use wxdata::global::GlobalField as GF;
    match field {
        GF::Temp2m | GF::Dewpoint2m => c_to_f(raw - 273.15),
        // GFS/ECMWF publish this as the U (east-west) *component* of the 10 m wind, not its
        // speed — a real vector quantity that is negative half the time, not a smaller wind.
        // `.abs()` turns it into the same "how hard, not which way" magnitude the map's own
        // `GLOBAL_WIND_10M` ramp already shows (`RampScale::Abs`) rather than a graph that
        // reads as calm every time the wind happens to blow from the east.
        GF::Wind10m => raw.abs() * 2.236_936, // m/s -> mph
        GF::Mslp => raw * 0.01,               // Pa -> hPa
        GF::Height500 => raw * 0.1,           // m -> dam
        GF::Precip => raw,
    }
}

fn field_unit(field: wxdata::global::GlobalField) -> &'static str {
    use wxdata::global::GlobalField as GF;
    match field {
        GF::Temp2m | GF::Dewpoint2m => "°F",
        GF::Wind10m => "mph",
        GF::Mslp => "hPa",
        GF::Height500 => "dam",
        GF::Precip => "mm",
    }
}

/// One line, hand-painted like `hourly_strip` — a value per fetched hour, gaps where a request
/// failed rather than a straight (and false) connector across a missing hour.
fn series_chart(
    ui: &mut egui::Ui,
    points: &[(DateTime<Utc>, Option<f32>)],
    field: wxdata::global::GlobalField,
    tz: Option<wxdata::tz::Tz>,
) {
    if points.len() < 2 {
        ui.weak("Not enough hours to draw a line.");
        return;
    }
    let values: Vec<Option<f32>> = points
        .iter()
        .map(|(_, v)| v.map(|v| display_value(field, v)))
        .collect();
    let finite: Vec<f32> = values.iter().filter_map(|v| *v).collect();
    if finite.is_empty() {
        ui.weak("No data for this period.");
        return;
    }

    let w = ui.available_width().max(220.0);
    let h = 110.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, Color32::from_black_alpha(90));

    let plot = rect.shrink2(Vec2::new(6.0, 4.0));
    let axis_h = 12.0;
    let body = egui::Rect::from_min_max(
        plot.left_top(),
        egui::pos2(plot.right(), plot.bottom() - axis_h),
    );

    let (lo, hi) = finite
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    // Always give the line some vertical room, even on a flat run.
    let (lo, hi) = if (hi - lo).abs() < 1.0 {
        (lo - 1.0, hi + 1.0)
    } else {
        (lo, hi)
    };
    let x_of = |i: usize| body.left() + (i as f32 + 0.5) / points.len() as f32 * body.width();
    let y_of =
        |v: f32| body.bottom() - ((v - lo) / (hi - lo).max(f32::EPSILON)) * body.height() * 0.82;

    // Contiguous runs of `Some` become separate polylines, so a gap breaks the line instead of
    // drawing a straight connector across an hour that failed to fetch.
    let color = Color32::from_rgb(120, 190, 230);
    let mut run: Vec<egui::Pos2> = Vec::new();
    for (i, v) in values.iter().enumerate() {
        match v {
            Some(v) => run.push(egui::pos2(x_of(i), y_of(*v))),
            None => {
                if run.len() > 1 {
                    p.add(egui::Shape::line(run.clone(), Stroke::new(1.6, color)));
                }
                run.clear();
            }
        }
    }
    if run.len() > 1 {
        p.add(egui::Shape::line(run, Stroke::new(1.6, color)));
    }

    // Label the ends and the extremes only — one value per fetched hour is noise past that.
    let font = FontId::proportional(9.0);
    let hi_i = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|v| (i, v)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i);
    let lo_i = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|v| (i, v)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i);
    let first_i = values.iter().position(|v| v.is_some());
    let last_i = values.iter().rposition(|v| v.is_some());
    for i in [first_i, hi_i, lo_i, last_i].into_iter().flatten() {
        let Some(v) = values[i] else { continue };
        p.text(
            egui::pos2(x_of(i), y_of(v)) - Vec2::new(0.0, 4.0),
            Align2::CENTER_BOTTOM,
            format!("{v:.0}"),
            font.clone(),
            Color32::from_gray(235),
        );
    }
    // Time axis at roughly quarter-width steps.
    let step = (points.len() / 4).max(1);
    for (i, (t, _)) in points.iter().enumerate() {
        if i % step != 0 {
            continue;
        }
        p.text(
            egui::pos2(x_of(i), plot.bottom() - axis_h + 1.0),
            Align2::CENTER_TOP,
            short_hour(*t, tz),
            font.clone(),
            Color32::from_gray(170),
        );
    }
}

fn series_stats(
    ui: &mut egui::Ui,
    points: &[(DateTime<Utc>, Option<f32>)],
    field: wxdata::global::GlobalField,
) {
    let values: Vec<f32> = points
        .iter()
        .filter_map(|(_, v)| v.map(|v| display_value(field, v)))
        .collect();
    if values.is_empty() {
        return;
    }
    let unit = field_unit(field);
    let (min, max) = values
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    ui.weak(format!(
        "min {min:.0}{unit} · max {max:.0}{unit} · avg {mean:.0}{unit}"
    ));
}

/// Minute-by-minute rain over the point for the next hour, advected from the current radar scan.
/// This is a nowcast off one volume, not a forecast product — hence the "~" in the caption.
fn minute_strip(ui: &mut egui::Ui, minute: &[Option<f32>]) {
    use crate::rain_arrival::intensity;
    if minute.len() < 2 {
        return;
    }
    ui.label(RichText::new("Next hour (radar)").strong());
    let w = ui.available_width().max(220.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, Color32::from_black_alpha(90));
    let bw = rect.width() / minute.len() as f32;
    for (i, v) in minute.iter().enumerate() {
        let lvl = v.map(intensity).unwrap_or(0);
        if lvl == 0 {
            continue;
        }
        let (color, frac) = match lvl {
            3 => (Color32::from_rgb(230, 90, 90), 1.0),
            2 => (Color32::from_rgb(80, 170, 230), 0.75),
            _ => (Color32::from_rgb(90, 130, 190), 0.45),
        };
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.left() + i as f32 * bw, rect.bottom() - 22.0 * frac),
            egui::pos2(rect.left() + (i + 1) as f32 * bw, rect.bottom() - 2.0),
        );
        p.rect_filled(bar, 0.0, color);
    }
    let wet: Vec<usize> = minute
        .iter()
        .enumerate()
        .filter(|(_, v)| v.is_some_and(|d| intensity(d) > 0))
        .map(|(i, _)| i)
        .collect();
    ui.small(match (wet.first(), wet.last()) {
        (Some(0), Some(e)) => format!("raining now · ends ~{e} min"),
        (Some(s), Some(e)) => format!("rain starts ~{s} min · ends ~{e} min"),
        _ => "no rain in the next hour".to_string(),
    });
}

/// Temperature curve over precipitation-probability bars, 24 hours wide.
fn hourly_strip(ui: &mut egui::Ui, hours: &[wxdata::forecast::Period], tz: Option<wxdata::tz::Tz>) {
    let hours: Vec<_> = hours.iter().take(24).collect();
    if hours.len() < 2 {
        return;
    }
    let w = ui.available_width().max(220.0);
    let h = 96.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, Color32::from_black_alpha(90));

    let plot = rect.shrink2(Vec2::new(6.0, 4.0));
    let axis_h = 12.0;
    let body = egui::Rect::from_min_max(
        plot.left_top(),
        egui::pos2(plot.right(), plot.bottom() - axis_h),
    );

    let temps: Vec<f32> = hours.iter().map(|x| x.temp_f).collect();
    let (lo, hi) = temps
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &t| (a.min(t), b.max(t)));
    // Always give the curve some vertical room, even on a flat day.
    let (lo, hi) = if (hi - lo).abs() < 5.0 {
        (lo - 3.0, hi + 3.0)
    } else {
        (lo, hi)
    };
    let x_of = |i: usize| body.left() + (i as f32 + 0.5) / hours.len() as f32 * body.width();
    let y_of = |t: f32| body.bottom() - ((t - lo) / (hi - lo).max(1.0)) * body.height() * 0.78;

    // Precip bars first — they're the backdrop the temperature reads against.
    let bw = (body.width() / hours.len() as f32) * 0.7;
    for (i, x) in hours.iter().enumerate() {
        let Some(pc) = x.precip_pct.filter(|v| *v > 0) else {
            continue;
        };
        let frac = pc as f32 / 100.0;
        let bar = egui::Rect::from_min_max(
            egui::pos2(x_of(i) - bw / 2.0, body.bottom() - frac * body.height()),
            egui::pos2(x_of(i) + bw / 2.0, body.bottom()),
        );
        p.rect_filled(bar, 1.0, Color32::from_rgba_unmultiplied(70, 140, 230, 120));
    }

    let pts: Vec<egui::Pos2> = temps
        .iter()
        .enumerate()
        .map(|(i, &t)| egui::pos2(x_of(i), y_of(t)))
        .collect();
    p.add(egui::Shape::line(
        pts.clone(),
        Stroke::new(1.6, Color32::from_rgb(245, 190, 90)),
    ));

    // Label the ends and the extremes only — 24 numbers is noise.
    let font = FontId::proportional(9.0);
    let hottest = temps
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i);
    for i in [Some(0), hottest, Some(hours.len() - 1)]
        .into_iter()
        .flatten()
    {
        p.text(
            pts[i] - Vec2::new(0.0, 4.0),
            Align2::CENTER_BOTTOM,
            format!("{:.0}°", temps[i]),
            font.clone(),
            Color32::from_gray(235),
        );
    }
    // Time axis every 6 hours.
    for (i, x) in hours.iter().enumerate() {
        if i % 6 != 0 {
            continue;
        }
        p.text(
            egui::pos2(x_of(i), plot.bottom() - axis_h + 1.0),
            Align2::CENTER_TOP,
            short_hour(x.start, tz),
            font.clone(),
            Color32::from_gray(170),
        );
    }
}

/// Wind speed over the same 24 hours the temperature curve covers, with an arrow every third
/// hour showing where the wind is coming from.
///
/// The window showed wind only as prose on the current-conditions line, which answers "what is
/// it doing now" but not "when does it get windy" — the question that decides whether you tie
/// something down. Gusts are missing on purpose: neither feed publishes an hourly gust, so
/// there is nothing honest to draw.
fn wind_strip(ui: &mut egui::Ui, hours: &[wxdata::forecast::Period]) {
    let hours: Vec<_> = hours.iter().take(24).collect();
    // Nothing to say if the feed gave no numbers — the NWS publishes wind as prose, and a
    // string it cannot parse must not become a flat line at zero.
    if hours.len() < 2 || hours.iter().all(|h| h.wind_mph.is_none()) {
        return;
    }
    let w = ui.available_width().max(220.0);
    let h = 46.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, Color32::from_black_alpha(90));
    let body = rect.shrink2(Vec2::new(6.0, 5.0));

    let peak = hours
        .iter()
        .filter_map(|x| x.wind_mph)
        .fold(0.0f32, f32::max)
        .max(10.0); // a calm day still gets a sensible scale rather than a magnified wobble
    let x_of = |i: usize| body.left() + (i as f32 + 0.5) / hours.len() as f32 * body.width();
    let y_of = |v: f32| body.bottom() - (v / peak) * body.height() * 0.62;

    let pts: Vec<egui::Pos2> = hours
        .iter()
        .enumerate()
        .filter_map(|(i, x)| Some(egui::pos2(x_of(i), y_of(x.wind_mph?))))
        .collect();
    p.add(egui::Shape::line(
        pts,
        Stroke::new(1.6, Color32::from_rgb(120, 210, 190)),
    ));

    // Direction arrows: every third hour, pointing the way the wind is going (the compass
    // reading is where it comes *from*, so the arrow is the reverse).
    for (i, x) in hours.iter().enumerate() {
        if i % 3 != 0 {
            continue;
        }
        let (Some(v), Some(from_deg)) = (x.wind_mph, x.wind_deg) else {
            continue;
        };
        let to = (from_deg + 180.0).to_radians();
        // Screen y grows downward, so north is -y.
        let dir = Vec2::new(to.sin(), -to.cos());
        let c = egui::pos2(x_of(i), y_of(v) - 9.0);
        let tip = c + dir * 4.5;
        let tail = c - dir * 4.5;
        let wing = Vec2::new(-dir.y, dir.x) * 2.4;
        let grey = Color32::from_gray(180);
        p.line_segment([tail, tip], Stroke::new(1.0, grey));
        p.line_segment([tip, tip - dir * 3.0 + wing], Stroke::new(1.0, grey));
        p.line_segment([tip, tip - dir * 3.0 - wing], Stroke::new(1.0, grey));
    }

    p.text(
        rect.left_top() + Vec2::new(6.0, 2.0),
        Align2::LEFT_TOP,
        format!("wind · peak {peak:.0} mph"),
        FontId::proportional(9.0),
        Color32::from_gray(170),
    );
    resp.on_hover_text("Sustained wind over the next 24 hours; arrows show which way it blows");
}

/// One-line current conditions, skipping whatever the station didn't report:
/// `74°F · dew 62°F · 62% rh · SW 12 kt G18 · 29.92 inHg · KOKC`.
fn conditions_line(o: &wxdata::obs::Observation, station: &str) -> String {
    use crate::ui::station_card::c_to_f;
    let mut parts: Vec<String> = Vec::new();
    if let Some(t) = o.temp_c {
        parts.push(format!("{:.0}°F", c_to_f(t)));
    }
    if let Some(d) = o.dewpoint_c {
        parts.push(format!("dew {:.0}°F", c_to_f(d)));
    }
    if let Some(rh) = o.rh {
        parts.push(format!("{rh:.0}% rh"));
    }
    if let Some(kmh) = o.wind_kmh {
        let kt = kmh / 1.852;
        let dir = o
            .wind_dir_deg
            .map(|d| format!("{} ", crate::ui::sensor_window::compass(d)))
            .unwrap_or_default();
        let gust = match o.gust_kmh {
            Some(g) => format!(" G{:.0}", g / 1.852),
            None => String::new(),
        };
        parts.push(if kt < 1.0 {
            "calm".to_string()
        } else {
            format!("{dir}{kt:.0} kt{gust}")
        });
    }
    // Sea-level pressure is what people read off a barometer; fall back to the station value.
    if let Some(pa) = o.slp_pa.or(o.pressure_pa) {
        parts.push(format!("{:.2} inHg", pa / 3386.389));
    }
    if !station.is_empty() {
        parts.push(station.to_string());
    }
    parts.join(" · ")
}

/// `Sunrise 6:14 AM · Sunset 8:42 PM · Waxing gibbous`, or just the moon during polar day/night.
// ponytail: words, not ↑/↓ arrows — those render as tofu boxes in the Android font stack.
fn almanac_line(at: (f64, f64), tz: Option<wxdata::tz::Tz>, now: DateTime<Utc>) -> String {
    let date = match tz {
        Some(tz) => now.with_timezone(&tz).date_naive(),
        None => now.date_naive(),
    };
    let (moon, _) = crate::astro::moon_label(crate::astro::moon_phase(now));
    match crate::astro::sun_times(at.1, at.0, date) {
        Some((rise, set)) => {
            format!(
                "Sunrise {} · Sunset {} · {moon}",
                clock(rise, tz),
                clock(set, tz)
            )
        }
        None => moon.to_string(),
    }
}

fn clock(t: DateTime<Utc>, tz: Option<wxdata::tz::Tz>) -> String {
    match tz {
        Some(tz) => t.with_timezone(&tz).format("%-I:%M %p").to_string(),
        None => t.format("%H:%MZ").to_string(),
    }
}

fn short_hour(t: DateTime<Utc>, tz: Option<wxdata::tz::Tz>) -> String {
    match tz {
        Some(tz) => t.with_timezone(&tz).format("%-I%p").to_string(),
        None => t.format("%HZ").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wxdata::obs::Observation;

    fn blank() -> Observation {
        Observation {
            time: None,
            temp_c: None,
            dewpoint_c: None,
            rh: None,
            wind_kmh: None,
            gust_kmh: None,
            wind_dir_deg: None,
            pressure_pa: None,
            slp_pa: None,
        }
    }

    #[test]
    fn full_observation_reads_like_a_metar() {
        let o = Observation {
            temp_c: Some(23.3),
            dewpoint_c: Some(16.7),
            rh: Some(66.0),
            wind_kmh: Some(22.2),
            gust_kmh: Some(33.3),
            wind_dir_deg: Some(225.0),
            slp_pa: Some(101_320.0),
            ..blank()
        };
        assert_eq!(
            conditions_line(&o, "KOKC"),
            "74°F · dew 62°F · 66% rh · SW 12 kt G18 · 29.92 inHg · KOKC"
        );
    }

    #[test]
    fn missing_fields_are_dropped_not_blanked() {
        let o = Observation {
            temp_c: Some(10.0),
            ..blank()
        };
        assert_eq!(conditions_line(&o, "KXYZ"), "50°F · KXYZ");
        assert_eq!(conditions_line(&blank(), ""), "");
    }

    #[test]
    fn wind_without_gust_or_direction() {
        let o = Observation {
            wind_kmh: Some(18.5),
            ..blank()
        };
        assert_eq!(conditions_line(&o, ""), "10 kt");
        let calm = Observation {
            wind_kmh: Some(0.0),
            wind_dir_deg: Some(0.0),
            ..blank()
        };
        assert_eq!(conditions_line(&calm, ""), "calm");
    }

    #[test]
    fn station_pressure_backfills_sea_level() {
        let o = Observation {
            pressure_pa: Some(96_000.0),
            ..blank()
        };
        assert_eq!(conditions_line(&o, ""), "28.35 inHg");
    }

    #[test]
    fn almanac_line_has_both_events_in_the_tropics() {
        // A fixed date, not the wall clock: within a few days of either equinox the sun really
        // does rise and set even at 89 N, so a `Utc::now()` version of this failed every
        // September and March.
        let solstice: DateTime<Utc> = "2026-06-21T12:00:00Z".parse().unwrap();
        let s = almanac_line((-97.5, 35.5), None, solstice);
        assert!(s.contains("Sunrise") && s.contains("Sunset"), "got {s}");
        // Polar latitudes lose the sun (midnight sun here) but keep the moon.
        let polar = almanac_line((15.0, 89.0), None, solstice);
        assert!(!polar.contains("Sunrise"), "got {polar}");
        assert!(!polar.is_empty());
    }
}

#[cfg(test)]
mod plume_tests {
    use super::*;
    use wxdata::ensemble::EnsembleField as EF;

    #[test]
    fn a_plume_reads_in_the_windows_own_units_and_a_spread_ignores_the_zero_point() {
        // 273.15 K is freezing: 32 °F, not the -459 a spread-style conversion would give.
        assert!((plume_value(EF::Temp2m, 273.15) - 32.0).abs() < 1e-3);
        // A 5 K spread is 9 °F wide.
        assert!((plume_spread(EF::Temp2m, 5.0) - 9.0).abs() < 1e-4);
        assert_eq!(plume_unit(EF::Temp2m), "°F");
        // Pressure: Pa to hPa for both a value and a spread.
        assert!((plume_value(EF::Mslp, 101_325.0) - 1013.25).abs() < 1e-2);
        assert!((plume_spread(EF::Mslp, 500.0) - 5.0).abs() < 1e-4);
        assert_eq!(plume_unit(EF::Mslp), "hPa");
        // CAPE is already in its display unit.
        assert_eq!(plume_value(EF::Cape, 1500.0), 1500.0);
        assert_eq!(plume_unit(EF::Cape), "J/kg");
    }

    #[test]
    fn plume_leads_are_six_hourly_from_the_start_and_fit_the_period() {
        assert_eq!(Period::Day1.plume_hours(), [0, 6, 12, 18, 24]);
        assert_eq!(Period::Day3.plume_hours().last(), Some(&72));
        let five = Period::Day5.plume_hours();
        assert_eq!(five.len(), 21);
        assert!(five.iter().all(|h| h % 6 == 0));
        // Two reads per lead: even the longest plume stays a small, bounded number of requests.
        assert!(five.len() * 2 <= 42);
    }
}
