//! The model browser's controls: pick a model, then a product it publishes, then scrub its lead.
//!
//! One panel serves every surface that offers it (layer options today), so a model behaves the
//! same wherever it is chosen. It only reads what it is handed and reports intent as
//! [`PaletteAction`]s; the app owns the state and applies them.

use crate::app::{FieldState, PaletteAction};
use crate::model_browser::{format_lead, BModel, Product, Selection};
use crate::render::FieldLayer as FL;
use crate::ui::layer_options::UiActions;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use wxdata::field::DataStamp;

/// What the panel needs to draw itself.
pub struct Input {
    pub sel: Selection,
    /// The current lead in minutes.
    pub lead_min: u16,
    /// Stamp of the layer the selection draws, once it has arrived.
    pub stamp: Option<DataStamp>,
    /// The run chosen, or `None` for the newest that has posted.
    pub run: Option<DateTime<Utc>>,
    /// The runs on offer for this model, newest first.
    pub runs: Vec<DateTime<Utc>>,
    /// How far this model (this run) can be scrubbed, and in what steps.
    pub range: crate::model_browser::LeadRange,
}

/// "3 min ago", "2 h ago" — coarse on purpose; a forecast's age is not a to-the-second matter.
pub fn ago(from: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = (now - from).num_seconds();
    if secs < 90 {
        "just now".into()
    } else if secs < 90 * 60 {
        format!("{} min ago", (secs + 30) / 60)
    } else {
        format!("{:.1} h ago", secs as f64 / 3600.0)
    }
}

/// The one-line provenance under the slider: which run, what it is valid for, how fresh.
pub fn status_line(
    stamp: &DataStamp,
    lead_min: u16,
    tz: Option<wxdata::tz::Tz>,
    now: DateTime<Utc>,
) -> String {
    let valid = crate::timefmt::fmt_date_clock(stamp.valid_time, tz);
    let run = stamp
        .run_time
        .map(|r| format!("{} run", r.format("%d %HZ")))
        .unwrap_or_else(|| "run unknown".into());
    let analysis = if lead_min == 0 { " · analysis" } else { "" };
    format!(
        "{run} · valid {valid}{analysis} · fetched {}",
        ago(stamp.received_time, now)
    )
}

#[allow(clippy::too_many_arguments)] // one flat call per frame, like layer_options::show
pub(crate) fn show(
    ui: &mut egui::Ui,
    input: &Input,
    on: &HashSet<FL>,
    tz: Option<wxdata::tz::Tz>,
    env_cape_ml: &mut bool,
    env_srh_km: &mut u8,
    fields: &mut HashMap<FL, FieldState>,
    actions: &mut UiActions,
) -> bool {
    let Input {
        sel,
        lead_min,
        stamp,
        run,
        runs,
        range,
    } = input;
    let range = *range;
    let mut changed = false;
    let showing = on.contains(&sel.layer());

    // Models, regional first: those are the ones worth reading a storm from.
    for family in crate::model_browser::Family::ALL {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(family.label()).small().weak());
            for m in BModel::ALL.into_iter().filter(|m| m.family() == family) {
                if ui
                    .selectable_label(sel.model == m, m.label())
                    .on_hover_text(m.blurb())
                    .clicked()
                {
                    actions.palette = Some(PaletteAction::SetModel(m));
                }
            }
        });
    }
    ui.add_space(2.0);

    // Products this model publishes. A dot marks the ones already on the map.
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Show").small().weak());
        for p in sel.model.products() {
            let drawn = on.contains(&p.layer());
            let text = if drawn {
                format!("● {}", p.label())
            } else {
                p.label().to_string()
            };
            if ui
                .selectable_label(sel.product == *p, text)
                .on_hover_text(p.blurb())
                .clicked()
            {
                actions.palette = Some(PaletteAction::SetModelProduct(*p));
            }
        }
    });

    // Which cycle of the model to read. "Latest" walks back to the newest one that has posted;
    // naming a run pins it, so two people looking at "the 12Z HRRR" see the same thing.
    ui.horizontal(|ui| {
        ui.label(if sel.model.has_lead() { "Run" } else { "Hour" });
        let current = match run {
            Some(r) => sel.model.run_label(*r),
            None => "Latest".to_string(),
        };
        egui::ComboBox::from_id_salt("model_run")
            .selected_text(current)
            // Fits a phone: the label takes what is left after the word "Run" and its gap.
            .width((ui.available_width() - 8.0).clamp(120.0, 260.0))
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(run.is_none(), "Latest available")
                    .on_hover_text("The newest run that has finished posting")
                    .clicked()
                {
                    actions.palette = Some(PaletteAction::SetModelRun(None));
                }
                for r in runs {
                    if ui
                        .selectable_label(*run == Some(*r), sel.model.run_label(*r))
                        .clicked()
                    {
                        actions.palette = Some(PaletteAction::SetModelRun(Some(r.timestamp())));
                    }
                }
            })
            .response
            .on_hover_text("Older runs stay available for a day or two");
    });

    if sel.model.has_lead() {
        // Lead: the model's own range and step, shown as a time from the run.
        ui.horizontal(|ui| {
            ui.label("Lead");
            if ui
                .small_button("‹")
                .on_hover_text("One step earlier")
                .clicked()
            {
                actions.palette = Some(PaletteAction::StepModelLead(-1));
            }
            let mut lead = *lead_min;
            let response = ui.add(
                egui::Slider::new(&mut lead, range.min..=range.max)
                    .step_by(f64::from(range.step))
                    .show_value(true)
                    .custom_formatter(|v, _| format_lead(v as u16))
                    .custom_parser(|s| {
                        let s = s.trim().trim_start_matches(['F', 'f', '+']);
                        s.trim_end_matches(['h', 'H'])
                            .parse::<f64>()
                            .ok()
                            .map(|h| h * 60.0)
                    }),
            );
            if response.changed() {
                actions.palette = Some(PaletteAction::SetModelLead(lead));
            }
            if ui
                .small_button("›")
                .on_hover_text("One step later")
                .clicked()
            {
                actions.palette = Some(PaletteAction::StepModelLead(1));
            }
        });

        // Jumps for getting well out without dragging: only the ones this run can reach.
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Jump").small().weak());
            for (label, add) in [
                ("+3h", 3u16),
                ("+6h", 6),
                ("+12h", 12),
                ("+24h", 24),
                ("+48h", 48),
                ("+5d", 120),
            ] {
                let target = lead_min.saturating_add(add * 60);
                if target > range.max && *lead_min >= range.max {
                    continue;
                }
                if ui.small_button(label).clicked() {
                    actions.palette = Some(PaletteAction::SetModelLead(target.min(range.max)));
                }
            }
            if ui
                .small_button("Start")
                .on_hover_text("Back to the first lead")
                .clicked()
            {
                actions.palette = Some(PaletteAction::SetModelLead(range.min));
            }
        });
    } else {
        // An analysis is valid at its own hour, so it has nothing to scrub: the Run menu above
        // is the time control.
        ui.weak("Analysis: valid at its own hour, so there is no lead. Pick the hour above.");
    }

    // One click to see how this model differs from its natural counterpart.
    if let Some((_, other)) = crate::model_browser::compare_field(*sel) {
        if ui
            .button(format!(
                "Swipe {} \u{21c4} {}",
                sel.model.label(),
                other.label()
            ))
            .on_hover_text(
                "Split the map between the two models at this lead, with a draggable divider",
            )
            .clicked()
        {
            actions.palette = Some(PaletteAction::CompareSelected);
        }
    }

    // Provenance, or why there is none yet.
    if showing {
        match stamp {
            Some(stamp) => {
                ui.weak(status_line(stamp, *lead_min, tz, Utc::now()));
            }
            None => {
                ui.weak("Loading forecast…");
            }
        }
        if sel.product == Product::Reflectivity {
            // The radar look is deliberate, so the label has to be too.
            ui.colored_label(
                egui::Color32::from_rgb(255, 170, 60),
                format!("{} forecast — not observed", sel.model.label()),
            );
        }
    } else {
        ui.weak("Not on the map — pick a product above to show it.");
    }

    // Options that belong to a product, right where the product is chosen.
    if on.contains(&FL::Cape) {
        ui.horizontal(|ui| {
            ui.label("CAPE parcel");
            let mut c = ui.selectable_value(env_cape_ml, false, "Surface").changed();
            c |= ui
                .selectable_value(env_cape_ml, true, "Mixed-layer")
                .changed();
            if c {
                if let Some(s) = fields.get_mut(&FL::Cape) {
                    s.last_fetch = None;
                }
                changed = true;
            }
        });
    }
    if on.contains(&FL::Srh) {
        ui.horizontal(|ui| {
            ui.label("Helicity depth");
            let mut c = ui.selectable_value(env_srh_km, 1u8, "0–1 km").changed();
            c |= ui.selectable_value(env_srh_km, 3u8, "0–3 km").changed();
            if c {
                if let Some(s) = fields.get_mut(&FL::Srh) {
                    s.last_fetch = None;
                }
                changed = true;
            }
        });
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn stamp(run_h: u32, valid_h: u32, received: DateTime<Utc>) -> DataStamp {
        DataStamp {
            source_id: "HRRR".into(),
            product_id: "REFC".into(),
            issue_time: None,
            run_time: Utc.with_ymd_and_hms(2026, 9, 20, run_h, 0, 0).single(),
            valid_time: Utc.with_ymd_and_hms(2026, 9, 20, valid_h, 0, 0).unwrap(),
            received_time: received,
            source_latency: None,
            is_forecast: true,
            is_derived: false,
            quality: wxdata::field::QualitySummary::Unknown,
            grid: None,
        }
    }

    #[test]
    fn age_is_coarse_and_never_negative_sounding() {
        let now = Utc.with_ymd_and_hms(2026, 9, 20, 12, 0, 0).unwrap();
        assert_eq!(ago(now, now), "just now");
        // A clock a little ahead of the server is "just now", not "-3 min ago".
        assert_eq!(ago(now + chrono::Duration::seconds(30), now), "just now");
        assert_eq!(ago(now - chrono::Duration::minutes(5), now), "5 min ago");
        assert_eq!(ago(now - chrono::Duration::minutes(150), now), "2.5 h ago");
    }

    #[test]
    fn the_status_line_names_the_run_the_valid_time_and_the_fetch_age() {
        let now = Utc.with_ymd_and_hms(2026, 9, 20, 15, 30, 0).unwrap();
        let s = stamp(12, 15, now - chrono::Duration::minutes(4));
        let text = status_line(&s, 180, None, now);
        assert!(text.contains("20 12Z run"), "{text}");
        assert!(text.contains("fetched 4 min ago"), "{text}");
        assert!(!text.contains("analysis"), "{text}");
        // Lead zero is an analysis, and says so.
        assert!(status_line(&s, 0, None, now).contains("analysis"));
    }
}
