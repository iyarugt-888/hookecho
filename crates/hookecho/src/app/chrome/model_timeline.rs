//! Shared forecast controls for the dock and floating timeline. The slider indexes the model's
//! published leads, including quarter-hour steps and changes to coarser output.

use super::*;
use crate::model_browser::{format_lead, BModel};
use crate::ui::{a11y::Named as _, workstation as ws};
use egui_phosphor::regular as ph;

#[derive(Default)]
struct Intent {
    action: Option<PaletteAction>,
    radar: bool,
    toggle_play: bool,
}

impl HookEchoApp {
    pub(crate) fn model_timeline_ui(&mut self, ui: &mut egui::Ui, phone: bool) {
        let input = self.model_panel_input();
        let t = self.ws_tokens();
        let tz = self.active_tz();
        let playback = &mut self.views[self.active].model_playback;
        let intent = forecast_rows(
            ui,
            &t,
            &input,
            playback.playing,
            &mut playback.speed,
            tz,
            phone,
        );
        if intent.radar {
            self.radar_timeline();
        }
        if intent.toggle_play {
            self.toggle_model_playback();
        }
        if let Some(action) = intent.action {
            self.apply_palette(action, &ui.ctx().clone());
        }
    }

    pub(crate) fn floating_model_timeline(&mut self, ctx: &egui::Context) {
        let phone = crate::platform::phone_layout();
        let t = self.ws_tokens();
        let width = (self.chrome_rect.width() - 24.0).clamp(260.0, 820.0);
        egui::Area::new(egui::Id::new("floating_model_timeline"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -12.0))
            .show(ctx, |ui| {
                ws::panel_frame(&t).show(ui, |ui| {
                    ui.set_width(width);
                    ws::style_scope(ui, &t);
                    self.model_timeline_ui(ui, phone);
                });
            });
    }
}

fn forecast_rows(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    input: &crate::ui::model_panel::Input,
    playing: bool,
    speed: &mut f32,
    tz: Option<wxdata::tz::Tz>,
    phone: bool,
) -> Intent {
    let mut intent = Intent::default();
    ui.horizontal(|ui| {
        if ui
            .selectable_label(false, "Radar")
            .named("Use radar timeline")
            .clicked()
        {
            intent.radar = true;
        }
        egui::ComboBox::from_id_salt("timeline_model")
            .selected_text(input.sel.model.label())
            .width(105.0)
            .show_ui(ui, |ui| {
                for model in BModel::ALL {
                    if ui
                        .selectable_label(model == input.sel.model, model.label())
                        .clicked()
                    {
                        intent.action = Some(PaletteAction::SetModel(model));
                    }
                }
            })
            .response
            .named("Timeline model");
        if !phone {
            ui.label(ws::text(input.sel.product.label(), 12.0, t.text_dim));
        }
    });
    ui.horizontal(|ui| {
        ws::caption(
            ui,
            t,
            if input.sel.model.has_lead() {
                "Run"
            } else {
                "Analysis hour"
            },
        );
        egui::ComboBox::from_id_salt("timeline_run")
            .width(
                ui.available_width()
                    .clamp(100.0, if phone { 220.0 } else { 300.0 }),
            )
            .selected_text(input.run.map_or_else(
                || "Latest available".to_string(),
                |run| input.sel.model.run_label(run),
            ))
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(input.run.is_none(), "Latest available")
                    .clicked()
                {
                    intent.action = Some(PaletteAction::SetModelRun(None));
                }
                for run in &input.runs {
                    if ui
                        .selectable_label(input.run == Some(*run), input.sel.model.run_label(*run))
                        .clicked()
                    {
                        intent.action = Some(PaletteAction::SetModelRun(Some(run.timestamp())));
                    }
                }
            })
            .response
            .named("Forecast run or analysis hour");
    });
    let positions = input.range.positions();
    let lead = input.range.clamp(input.lead_min);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = if phone { 6.0 } else { 2.0 };
        let button = |ui: &mut egui::Ui, glyph: &str, name: &str| {
            let response = if phone {
                ui.add_sized([44.0, 38.0], egui::Button::new(glyph))
            } else {
                ws::icon_button(ui, t, glyph, "", false)
            };
            response.named(name).clicked()
        };
        if button(
            ui,
            ph::SKIP_BACK,
            if input.sel.model.has_lead() {
                "First forecast hour"
            } else {
                "Oldest analysis hour"
            },
        ) {
            intent.action = Some(if input.sel.model.has_lead() {
                PaletteAction::SetModelLead(input.range.min)
            } else {
                PaletteAction::SetModelRun(input.runs.last().map(|run| run.timestamp()))
            });
        }
        if button(ui, ph::REWIND, "Previous forecast hour or analysis") {
            intent.action = Some(PaletteAction::StepModelLead(-1));
        }
        ui.add_enabled_ui(input.sel.model.has_lead(), |ui| {
            if button(
                ui,
                if playing { ph::PAUSE } else { ph::PLAY },
                if playing {
                    "Pause model forecast"
                } else {
                    "Play model forecast"
                },
            ) {
                intent.toggle_play = true;
            }
        });
        if button(ui, ph::FAST_FORWARD, "Next forecast hour or analysis") {
            intent.action = Some(PaletteAction::StepModelLead(1));
        }
        if button(
            ui,
            ph::SKIP_FORWARD,
            if input.sel.model.has_lead() {
                "Last forecast hour"
            } else {
                "Latest analysis hour"
            },
        ) {
            intent.action = Some(if input.sel.model.has_lead() {
                PaletteAction::SetModelLead(input.range.max)
            } else {
                PaletteAction::SetModelRun(None)
            });
        }
        if !phone {
            ui.add_space(8.0);
            egui::ComboBox::from_id_salt("model_timeline_speed")
                .width(75.0)
                .selected_text(format!("{speed:.0} fps"))
                .show_ui(ui, |ui| {
                    for fps in [1.0, 2.0, 4.0, 6.0] {
                        ui.selectable_value(speed, fps, format!("{fps:.0} fps"));
                    }
                });
        }
    });
    if input.sel.model.has_lead() {
        ui.horizontal(|ui| {
            let mut entered = lead;
            if ui
                .add(
                    egui::DragValue::new(&mut entered)
                        .range(input.range.min..=input.range.max)
                        .speed(f64::from(input.range.step))
                        .custom_formatter(|value, _| format_lead(value.round() as u16))
                        .custom_parser(parse_lead),
                )
                .named("Enter forecast hour")
                .changed()
            {
                intent.action = Some(PaletteAction::SetModelLead(input.range.clamp(entered)));
            }
            let mut index = positions.binary_search(&lead).unwrap_or(0);
            let width = ui.available_width().max(40.0);
            ui.spacing_mut().slider_width = width;
            if ui
                .add(
                    egui::Slider::new(&mut index, 0..=positions.len().saturating_sub(1))
                        .show_value(false)
                        .custom_formatter(|value, _| {
                            format_lead(
                                positions[(value.round() as usize).min(positions.len() - 1)],
                            )
                        }),
                )
                .named("Model forecast hour")
                .changed()
            {
                intent.action = Some(PaletteAction::SetModelLead(positions[index]));
            }
        });
    }
    let clock = forecast_clock(input, tz);
    ui.add(egui::Label::new(ws::mono(clock, 11.0, t.text_dim)).wrap());
    intent
}

fn parse_lead(text: &str) -> Option<f64> {
    let text = text
        .trim()
        .trim_start_matches(['F', 'f', '+'])
        .to_lowercase();
    if text.contains('-') {
        return None;
    }
    let minutes = if let Some((hours, minutes)) = text.split_once('h') {
        hours.trim().parse::<f64>().ok()? * 60.0
            + if minutes.is_empty() {
                0.0
            } else {
                minutes.trim_end_matches('m').trim().parse::<f64>().ok()?
            }
    } else if let Some(minutes) = text.strip_suffix('m') {
        minutes.trim().parse::<f64>().ok()?
    } else {
        text.parse::<f64>().ok()? * 60.0
    };
    (minutes.is_finite() && minutes >= 0.0).then_some(minutes)
}

/// Requested time and the loaded field remain distinct during a download. A latest run is
/// resolved by the provider: its precise valid time remains unconfirmed before delivery.
fn forecast_clock(input: &crate::ui::model_panel::Input, tz: Option<wxdata::tz::Tz>) -> String {
    let requested = input
        .run
        .map(|run| run + chrono::Duration::minutes(i64::from(input.range.clamp(input.lead_min))));
    let wanted = requested.map_or_else(
        || format!("Latest run requested {}", format_lead(input.lead_min)),
        |valid| format!("Requested {}", crate::timefmt::fmt_date_clock(valid, tz)),
    );
    match &input.stamp {
        Some(stamp) => format!(
            "{wanted} · loaded {} valid {}",
            stamp.source_id,
            crate::timefmt::fmt_date_clock(stamp.valid_time, tz)
        ),
        None => format!("{wanted} · awaiting field"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn input(model: BModel, lead_min: u16) -> crate::ui::model_panel::Input {
        let run = Utc.with_ymd_and_hms(2026, 10, 4, 12, 0, 0).unwrap();
        crate::ui::model_panel::Input {
            sel: crate::model_browser::Selection {
                model,
                product: model.default_product(),
            },
            lead_min,
            stamp: None,
            run: Some(run),
            runs: model.runs_around(Some(run), run + chrono::Duration::hours(8), 6),
            range: model.leads_for(Some(run), run + chrono::Duration::hours(8)),
        }
    }

    fn loaded(input: &crate::ui::model_panel::Input, lead: u16) -> wxdata::field::DataStamp {
        let run = input.run.unwrap();
        wxdata::field::DataStamp {
            source_id: input.sel.model.label().into(),
            product_id: input.sel.product.label().into(),
            issue_time: None,
            run_time: Some(run),
            valid_time: run + chrono::Duration::minutes(i64::from(lead)),
            received_time: run,
            source_latency: None,
            is_forecast: true,
            is_derived: false,
            quality: wxdata::field::QualitySummary::Unknown,
            grid: None,
        }
    }

    #[test]
    fn model_timeline_hour_entry_round_trips_native_and_decimal_leads() {
        for model in BModel::ALL {
            for minutes in model.leads().positions() {
                assert_eq!(parse_lead(&format_lead(minutes)), Some(f64::from(minutes)));
            }
        }
        assert_eq!(parse_lead("1.25"), Some(75.0));
        assert_eq!(parse_lead("F+15m"), Some(15.0));
        assert_eq!(parse_lead("2h30m"), Some(150.0));
        for bad in ["NaN", "inf", "-1", "hour", "1h-15m"] {
            assert!(parse_lead(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn model_timeline_clock_discloses_requested_and_loaded_times_separately() {
        let mut request = input(BModel::Gfs, 6 * 60);
        let clock = forecast_clock(&request, None);
        assert!(clock.contains("18:00"));
        assert!(clock.contains("awaiting field"));
        request.stamp = Some(loaded(&request, 0));
        let clock = forecast_clock(&request, None);
        assert!(clock.contains("Requested 2026-10-04 18:00Z"));
        assert!(clock.contains("loaded GFS valid 2026-10-04 12:00Z"));
        request.stamp = None;
        request.run = None;
        let clock = forecast_clock(&request, None);
        assert!(clock.contains("Latest run requested F+6h"));
        assert!(
            !clock.contains("18:00"),
            "unknown latest cycle must not get a guessed valid clock"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: captures production model timeline at desktop and phone widths"]
    fn gpu_model_timeline_snapshots() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU for model timeline review");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/model-timeline/ui");
        std::fs::create_dir_all(&destination).unwrap();
        let t = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        for (name, model, lead) in [
            ("subhourly", BModel::Hrrr15, 75),
            ("regional", BModel::Nam, 39 * 60),
            ("global", BModel::GefsMean, 246 * 60),
            ("analysis", BModel::Rtma, 0),
        ] {
            let mut input = input(model, lead);
            if name == "global" {
                input.stamp = Some(loaded(&input, lead));
            }
            for (width, phone, height) in [(320, true, 180), (1000, false, 148)] {
                gpu.save(
                    &destination.join(format!("{name}-{width}.png")),
                    width,
                    height,
                    |ui| {
                        ws::set_touch(ui.ctx(), phone);
                        ws::panel_frame(&t)
                            .inner_margin(egui::Margin::symmetric(12, 6))
                            .show(ui, |ui| {
                                ws::style_scope(ui, &t);
                                let mut speed = 2.0;
                                forecast_rows(ui, &t, &input, false, &mut speed, None, phone);
                                assert!(
                                    ui.min_rect().right() <= width as f32 + 0.5,
                                    "{name}: horizontal overflow"
                                );
                                assert!(
                                    ui.min_rect().bottom() <= height as f32,
                                    "{name}: vertical overflow"
                                );
                            });
                    },
                )
                .unwrap();
            }
        }
    }
}
