//! Model verification: how far off was a forecast run, scored against the RTMA analysis
//! (ROADMAP_NEW K1).
//!
//! The scoring lives in `wxdata::gridverify`; this window is the choosing and the reading. It is a
//! table rather than a dashboard because the story is the *shape* of the errors: how they grow with
//! lead, and whether the bias leans warm or cold.

use crate::settings::TempUnit;
use chrono::{DateTime, Utc};
use wxdata::gridverify::{GridScore, LeadResult, VerifyField};

/// The models that can be scored: the regional runs that publish the surface fields.
pub const MODELS: [crate::model_browser::BModel; 4] = [
    crate::model_browser::BModel::Hrrr,
    crate::model_browser::BModel::Rap,
    crate::model_browser::BModel::NamNest,
    crate::model_browser::BModel::Nam,
];

/// Leads offered, in hours. Whole hours every model publishes.
pub const LEADS: [u8; 5] = [1, 3, 6, 12, 18];

/// What one finished run scored, so the table can say what it is a table *of*.
#[derive(Clone)]
pub struct Meta {
    pub model: crate::model_browser::BModel,
    pub field: VerifyField,
    pub run: DateTime<Utc>,
    pub threshold_k: Option<f32>,
    pub region_is_view: bool,
}

/// What the window wants the app to do this frame.
#[derive(Default)]
pub struct Actions {
    pub run: bool,
}

pub struct ModelVerifyWindow {
    pub open: bool,
    pub busy: bool,
    pub error: Option<String>,
    pub results: Option<(Meta, Vec<LeadResult>)>,
    pub model: crate::model_browser::BModel,
    pub field: VerifyField,
    /// `None` picks a run far enough back that its leads all have an analysis.
    pub run: Option<DateTime<Utc>>,
    pub leads_on: [bool; 5],
    /// Score only what is on screen, rather than the whole domain.
    pub region_is_view: bool,
    pub threshold_on: bool,
    /// The event threshold in Kelvin, so a change of temperature unit never changes what it means.
    pub threshold_k: f32,
}

impl Default for ModelVerifyWindow {
    fn default() -> Self {
        Self {
            open: false,
            busy: false,
            error: None,
            results: None,
            model: crate::model_browser::BModel::Hrrr,
            field: VerifyField::Temp2m,
            run: None,
            leads_on: [true, true, true, false, false],
            region_is_view: false,
            threshold_on: false,
            // Freezing.
            threshold_k: 273.15,
        }
    }
}

/// A temperature *difference* in the reader's unit: the size of the degree, not the zero point.
pub fn diff_in(unit: TempUnit, kelvin_diff: f64) -> f64 {
    (unit.from_c(kelvin_diff as f32) - unit.from_c(0.0)) as f64
}

/// The event threshold in Kelvin, from what the reader typed in their own unit.
pub fn threshold_kelvin(unit: TempUnit, shown: f32) -> f32 {
    let c = match unit {
        TempUnit::Fahrenheit => (shown - 32.0) * 5.0 / 9.0,
        TempUnit::Celsius => shown,
    };
    c + 273.15
}

/// A run that is far enough back for its short leads to have an analysis, for "auto". Twelve hours
/// covers the RTMA's posting delay for every lead offered up to 6 h with room to spare, and 18 h
/// leads still fall back to "no analysis yet" if the run is too fresh.
pub fn auto_run(model: crate::model_browser::BModel, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let cutoff = now - chrono::Duration::hours(12);
    model
        .run_choices(now, 96)
        .into_iter()
        .find(|run| *run <= cutoff)
}

/// A score line in words: which way it leans and how big the typical miss is.
pub fn headline(score: &GridScore, unit: TempUnit) -> String {
    let s = score.scores;
    let bias = diff_in(unit, s.bias);
    let lean = if bias.abs() < 0.1 {
        "no lean".to_string()
    } else if bias > 0.0 {
        format!("runs {:.1}{} too high", bias, unit.label())
    } else {
        format!("runs {:.1}{} too low", -bias, unit.label())
    };
    format!(
        "typically off by {:.1}{} ({lean})",
        diff_in(unit, s.mae),
        unit.label()
    )
}

impl ModelVerifyWindow {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        tz: Option<wxdata::tz::Tz>,
        unit: TempUnit,
        drawer: &mut crate::ui::drawer::Drawer,
    ) -> Actions {
        let mut act = Actions::default();
        if !self.open {
            return act;
        }
        let mut open = self.open;
        let Some(window) = drawer.page_sized(
            ctx,
            "Model Verification",
            &mut open,
            false,
            600.0,
            egui::Window::new("Model Verification"),
        ) else {
            self.open = open;
            return act;
        };
        window.show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Model:");
                for m in MODELS {
                    if ui.selectable_label(self.model == m, m.label()).clicked() {
                        self.model = m;
                        // A run is one model's cycle; another model has its own.
                        self.run = None;
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Field:");
                for f in VerifyField::ALL {
                    ui.selectable_value(&mut self.field, f, f.label());
                }
            });
            ui.horizontal(|ui| {
                ui.label("Run:");
                let now = Utc::now();
                let current = match self.run {
                    Some(r) => self.model.run_label(r),
                    None => "Auto (about 12 h ago)".to_string(),
                };
                egui::ComboBox::from_id_salt("model_verify_run")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(self.run.is_none(), "Auto (about 12 h ago)").clicked() {
                            self.run = None;
                        }
                        for r in self.model.run_choices(now, self.model.run_list_len()) {
                            if ui
                                .selectable_label(self.run == Some(r), self.model.run_label(r))
                                .clicked()
                            {
                                self.run = Some(r);
                            }
                        }
                    });
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Leads:");
                for (on, lead) in self.leads_on.iter_mut().zip(LEADS) {
                    ui.checkbox(on, format!("F+{lead}h"));
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.region_is_view, "Only what is on screen");
                ui.checkbox(&mut self.threshold_on, "Score an event above");
                // Shown in the reader's unit, kept in Kelvin.
                let mut shown = unit.from_c(self.threshold_k - 273.15);
                if ui
                    .add_enabled(
                        self.threshold_on,
                        egui::DragValue::new(&mut shown)
                            .speed(0.5)
                            .suffix(unit.label()),
                    )
                    .changed()
                {
                    self.threshold_k = threshold_kelvin(unit, shown);
                }
            });
            ui.horizontal(|ui| {
                let any_lead = self.leads_on.iter().any(|on| *on);
                if ui
                    .add_enabled(!self.busy && any_lead, egui::Button::new("Verify"))
                    .clicked()
                {
                    act.run = true;
                }
                if self.busy {
                    crate::ui::loading(ui, "Scoring against the RTMA…");
                }
            });
            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(240, 120, 120), e);
            }
            let Some((meta, rows)) = &self.results else {
                ui.weak(
                    "Scores a forecast run against the RTMA analysis for the same hour, over about a \
                     million grid cells weighted by the ground they cover. The RTMA is itself an \
                     estimate, so this measures agreement with it, not with every station.",
                );
                return;
            };
            ui.separator();
            ui.strong(format!(
                "{} {} · {} run · {}",
                meta.model.label(),
                meta.field.label(),
                meta.run.format("%d %b %HZ"),
                if meta.region_is_view {
                    "map view"
                } else {
                    "whole domain"
                }
            ));
            results_table(ui, meta, rows, tz, unit);
        });
        self.open = open;
        act
    }
}

fn results_table(
    ui: &mut egui::Ui,
    meta: &Meta,
    rows: &[LeadResult],
    tz: Option<wxdata::tz::Tz>,
    unit: TempUnit,
) {
    let has_event = meta.threshold_k.is_some();
    let unit_label = unit.label();
    egui::Grid::new("model_verify_table")
        .striped(true)
        .num_columns(if has_event { 11 } else { 7 })
        .show(ui, |ui| {
            for h in ["Lead", "Valid", "Cells"] {
                ui.strong(h);
            }
            ui.strong(format!("Bias {unit_label}"))
                .on_hover_text("Forecast minus analysis. Positive: the model ran high.");
            ui.strong(format!("MAE {unit_label}"))
                .on_hover_text("Average size of the error, ignoring direction.");
            ui.strong(format!("RMSE {unit_label}"))
                .on_hover_text("Like MAE but large misses count for more.");
            ui.strong("r")
                .on_hover_text("Correlation: is the pattern right even if the level is off?");
            if has_event {
                for (h, tip) in [
                    (
                        "POD",
                        "Of the events that happened, the share the model forecast.",
                    ),
                    (
                        "FAR",
                        "Of the events forecast, the share that did not happen.",
                    ),
                    (
                        "CSI",
                        "Hits over everything forecast or observed. 1 is perfect.",
                    ),
                    (
                        "Freq bias",
                        "Above 1 the model forecasts the event more often than it happened.",
                    ),
                ] {
                    ui.strong(h).on_hover_text(tip);
                }
            }
            ui.end_row();
            for r in rows {
                ui.label(crate::model_browser::format_lead(u16::from(r.lead_h) * 60));
                ui.label(crate::timefmt::fmt_date_clock(r.valid, tz));
                match &r.score {
                    Ok(g) => {
                        let s = g.scores;
                        ui.label(format!("{}", s.cells));
                        ui.label(format!("{:+.2}", diff_in(unit, s.bias)));
                        ui.label(format!("{:.2}", diff_in(unit, s.mae)));
                        ui.label(format!("{:.2}", diff_in(unit, s.rmse)));
                        ui.label(s.correlation.map_or("—".into(), |c| format!("{c:.3}")));
                        if has_event {
                            let pct = |v: Option<f64>| {
                                v.map_or("—".into(), |v| format!("{:.0}%", v * 100.0))
                            };
                            match g.contingency {
                                Some(c) => {
                                    ui.label(pct(c.pod()));
                                    ui.label(pct(c.far()));
                                    ui.label(pct(c.csi()));
                                    ui.label(
                                        c.frequency_bias()
                                            .map_or("—".into(), |v| format!("{v:.2}")),
                                    );
                                }
                                None => {
                                    for _ in 0..4 {
                                        ui.label("—");
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        ui.weak(e);
                    }
                }
                ui.end_row();
            }
        });
    // One plain sentence per scored lead, so the table has a reading.
    if let Some(g) = rows.iter().rev().find_map(|r| r.score.as_ref().ok()) {
        let lead = rows
            .iter()
            .rev()
            .find(|r| r.score.is_ok())
            .map(|r| r.lead_h)
            .unwrap_or(0);
        ui.add_space(4.0);
        ui.weak(format!("At F+{lead}h the model is {}.", headline(g, unit)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use wxdata::gridverify::Scores;

    fn score(bias: f64, mae: f64) -> GridScore {
        GridScore {
            scores: Scores {
                cells: 10,
                bias,
                mae,
                rmse: mae,
                correlation: Some(0.99),
            },
            contingency: None,
        }
    }

    #[test]
    fn a_difference_converts_by_the_size_of_the_degree_not_the_zero_point() {
        // 5 K apart is 9 °F apart, not the -450 a value conversion would give.
        assert!((diff_in(TempUnit::Fahrenheit, 5.0) - 9.0).abs() < 1e-4);
        assert!((diff_in(TempUnit::Celsius, 5.0) - 5.0).abs() < 1e-6);
        assert!((diff_in(TempUnit::Fahrenheit, -1.0) + 1.8).abs() < 1e-4);
    }

    #[test]
    fn an_entered_threshold_becomes_kelvin_in_either_unit() {
        assert!((threshold_kelvin(TempUnit::Fahrenheit, 32.0) - 273.15).abs() < 1e-3);
        assert!((threshold_kelvin(TempUnit::Celsius, 0.0) - 273.15).abs() < 1e-3);
        assert!((threshold_kelvin(TempUnit::Fahrenheit, 212.0) - 373.15).abs() < 1e-2);
    }

    #[test]
    fn the_headline_says_which_way_the_model_leans_in_words() {
        let warm = headline(&score(1.0, 2.0), TempUnit::Celsius);
        assert!(warm.contains("2.0") && warm.contains("too high"), "{warm}");
        let cold = headline(&score(-1.0, 2.0), TempUnit::Celsius);
        assert!(cold.contains("too low"), "{cold}");
        let flat = headline(&score(0.02, 0.5), TempUnit::Celsius);
        assert!(flat.contains("no lean"), "{flat}");
        // The size follows the reader's unit.
        assert!(headline(&score(0.0, 1.0), TempUnit::Fahrenheit).contains("1.8"));
    }

    #[test]
    fn auto_picks_a_run_old_enough_for_short_leads_to_have_an_analysis() {
        let now = Utc.with_ymd_and_hms(2026, 9, 21, 15, 30, 0).unwrap();
        for m in MODELS {
            let run = auto_run(m, now).expect("a run");
            assert!(run <= now - chrono::Duration::hours(12), "{m:?}: {run}");
            // ... but not absurdly old: the nearest such run on the model's own cycle.
            assert!(run > now - chrono::Duration::hours(12 + 7), "{m:?}: {run}");
        }
    }

    #[test]
    fn the_models_offered_are_the_ones_that_publish_the_surface_fields() {
        for m in MODELS {
            let regional = m.regional_model().expect("a regional model");
            for f in VerifyField::ALL {
                // The scorer looks the key up in the catalogue; it must exist for every pair offered.
                let mf = match f {
                    VerifyField::Temp2m => wxdata::model::ModelField::Temperature2m,
                    VerifyField::Dewpoint2m => wxdata::model::ModelField::Dewpoint2m,
                };
                assert!(mf.grib(regional).is_some(), "{m:?} {f:?}");
            }
        }
    }
}
