//! Phase B4's radar metadata inspector: every geometry/value fact about one clicked gate.
//!
//! The math lives in `wxdata::level2` (`BinnedSweep::inspect`, `sweep_time_range`) — this module
//! is presentation only, following `cell_window.rs`'s grouped-columns layout.

use wxdata::level2::{GateInspection, Moment};

/// Everything a click with the Gate inspector tool needs to show.
#[derive(Debug, Clone)]
pub struct GateInspectorPopup {
    pub site: Option<String>,
    /// Human-readable VCP label, e.g. "VCP 212 (Precipitation, SZ-2)" — `Volume::vcp` verbatim.
    pub vcp: String,
    pub moment: Moment,
    /// The wall-clock span this tilt's radials were actually collected over
    /// (`wxdata::level2::sweep_time_range`); `None` when the source carries no per-radial
    /// timestamps.
    pub time_range: Option<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>,
    pub inspection: GateInspection,
    /// Phase C1: every moment/geometry input a user-defined product can read, sampled at this
    /// same point — kept separate from `inspection` so the products shown here update if the
    /// saved list changes without needing another click.
    pub gate_inputs: wxdata::udp::GateInputs,
    /// Every tilt's own `gate_inputs` at this same point, low to high — what a vertical/layer
    /// user-defined-product formula (`max_vertical`, `max_layer`, …; ROADMAP_NEW C1) reduces
    /// over. A tilt this point falls outside of is simply absent, not a placeholder entry.
    pub column_inputs: Vec<wxdata::udp::GateInputs>,
}

pub fn show(
    ctx: &egui::Context,
    popup: &GateInspectorPopup,
    tz: Option<wxdata::tz::Tz>,
    udp_products: &[wxdata::udp::ProductDef],
    popovers: &mut crate::ui::popover::Popovers,
) -> bool {
    let mut open = true;
    popovers
        .card(
            ctx,
            "gate_inspector",
            egui::Window::new("Gate inspector").id(egui::Id::new("gate_inspector")),
        )
        .open(&mut open)
        .default_width((ctx.content_rect().width() - 48.0).clamp(320.0, 560.0))
        .max_height((ctx.content_rect().height() - 160.0).max(260.0))
        .vscroll(true)
        .resizable(false)
        .collapsible(false)
        .frame(
            egui::Frame::window(&ctx.style_of(ctx.theme()))
                .fill(egui::Color32::from_rgb(17, 23, 31))
                .corner_radius(16)
                .inner_margin(18),
        )
        .show(ctx, |ui| attributes(ui, popup, tz, udp_products));
    open
}

fn opt(v: Option<f32>, unit: &str, decimals: usize) -> String {
    v.map(|x| format!("{x:.*}{unit}", decimals))
        .unwrap_or_else(|| "—".into())
}

fn value(ui: &mut egui::Ui, label: &str, value: String) {
    ui.label(egui::RichText::new(label).size(11.0).weak());
    ui.label(egui::RichText::new(value).size(15.0).strong());
    ui.add_space(5.0);
}

/// Shared with the tests below so layout changes cannot silently drop a field.
pub(crate) fn attributes(
    ui: &mut egui::Ui,
    popup: &GateInspectorPopup,
    tz: Option<wxdata::tz::Tz>,
    udp_products: &[wxdata::udp::ProductDef],
) {
    let i = &popup.inspection;
    let raw_value = if i.sample.folded {
        "Range folded".to_string()
    } else {
        opt(i.sample.value, &format!(" {}", popup.moment.units()), 1)
    };
    let sweep_time = popup.time_range.map_or_else(
        || "—".to_string(),
        |(start, end)| {
            if (end - start).num_seconds().abs() < 30 {
                crate::timefmt::fmt_clock(start, tz, true)
            } else {
                format!(
                    "{}–{}",
                    crate::timefmt::fmt_clock(start, tz, false),
                    crate::timefmt::fmt_clock(end, tz, false)
                )
            }
        },
    );
    // The gate's *own* acquisition time, which on a live partially-swept volume is not the
    // volume's time: the wedge the antenna has not come back to yet is carrying radials from the
    // previous rotation. "Sweep time" above spans the whole tilt and so hides that; this row is
    // the per-gate half of suggestions.md §21's requirement that retained data never read as
    // newly scanned. The lag is measured against the newest radial in the tilt, because that is
    // what the user is implicitly comparing it to when they read the sweep time.
    let gate_time = i.sample.collected_ms.map_or_else(
        || "—".to_string(),
        |ms| {
            let t = chrono::DateTime::from_timestamp_millis(ms);
            let clock = t.map_or_else(|| "—".into(), |t| crate::timefmt::fmt_clock(t, tz, true));
            match (t, popup.time_range) {
                // 30 s is under one rotation in any VCP, so a lag that clears it is a real
                // generation boundary rather than the seconds a single pass takes to sweep by.
                (Some(t), Some((_, end))) if (end - t).num_seconds() >= 30 => {
                    format!("{clock} ({} s older)", (end - t).num_seconds())
                }
                _ => clock,
            }
        },
    );
    // Phase C1: each saved formula, re-evaluated fresh against this same gate every frame — so
    // editing a product in the manager updates this without needing another click.
    let udp_rows: Vec<(&str, String)> = udp_products
        .iter()
        .map(|def| {
            let value = match def.compile() {
                Ok(expr) => match wxdata::udp::evaluate_at_column(
                    &expr,
                    &popup.gate_inputs,
                    &popup.column_inputs,
                ) {
                    Some(v) => format!("{v:.2} {}", def.units).trim_end().to_string(),
                    None => "—".to_string(),
                },
                Err(e) => format!("Error: {e}"),
            };
            (def.name.as_str(), value)
        })
        .collect();
    let mut groups: Vec<(&str, Vec<(&str, String)>)> = vec![
        (
            "POSITION",
            vec![
                (
                    "Radar site",
                    popup.site.clone().unwrap_or_else(|| "—".into()),
                ),
                ("VCP", popup.vcp.clone()),
                ("Elevation angle", format!("{:.2}°", i.elevation_deg)),
                ("Azimuth", format!("{:.1}°", i.sample.azimuth_deg)),
                ("Sweep time", sweep_time),
                ("Gate collected", gate_time),
            ],
        ),
        (
            "GEOMETRY",
            vec![
                ("Slant range", format!("{:.2} km", i.sample.range_km)),
                ("Ground range", format!("{:.2} km", i.ground_range_km)),
                ("Beam height", format!("{:.0} ft", i.beam_height_ft)),
                (
                    "Beam top/bottom",
                    format!(
                        "{:.0} / {:.0} ft",
                        i.beam_top_bottom_ft.0, i.beam_top_bottom_ft.1
                    ),
                ),
                ("Beam width", format!("{:.2} km", i.beam_width_km)),
                ("Gate spacing", format!("{:.3} km", i.gate_interval_km)),
                ("Gate index", i.sample.gate.to_string()),
            ],
        ),
        (popup.moment.short_name(), {
            let mut rows = vec![("Raw value", raw_value)];
            if popup.moment == Moment::Velocity {
                rows.push(("Dealiased value", opt(i.dealiased_value, " m/s", 1)));
                rows.push(("Nyquist velocity (est.)", opt(i.nyquist_mps, " m/s", 1)));
            }
            rows
        }),
    ];
    if !udp_rows.is_empty() {
        groups.push(("USER-DEFINED", udp_rows));
    }
    let columns = if ui.available_width() >= 480.0 { 3 } else { 1 };
    for chunk in groups.chunks(columns) {
        ui.columns(columns, |cols| {
            for (col, (title, fields)) in cols.iter_mut().zip(chunk) {
                col.label(egui::RichText::new(*title).size(11.0).strong());
                col.separator();
                for (label, v) in fields {
                    value(col, label, v.clone());
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wxdata::level2::GateSample;

    fn sample_popup(moment: Moment, folded: bool, value: Option<f32>) -> GateInspectorPopup {
        GateInspectorPopup {
            site: Some("KTLX".into()),
            vcp: "VCP 212 (Precipitation, SZ-2)".into(),
            moment,
            time_range: Some((
                chrono::DateTime::from_timestamp(1_000, 0).unwrap(),
                chrono::DateTime::from_timestamp(1_060, 0).unwrap(),
            )),
            inspection: GateInspection {
                sample: GateSample {
                    value,
                    folded,
                    azimuth_deg: 123.4,
                    range_km: 45.6,
                    gate: 182,
                    collected_ms: Some(1_050_000),
                },
                dealiased_value: (moment == Moment::Velocity).then_some(28.0),
                ground_range_km: 45.0,
                beam_height_ft: 6200.0,
                beam_top_bottom_ft: (7100.0, 5300.0),
                beam_width_km: 0.73,
                gate_interval_km: 0.25,
                elevation_deg: 0.5,
                nyquist_mps: (moment == Moment::Velocity).then_some(32.0),
            },
            gate_inputs: wxdata::udp::GateInputs::default(),
            column_inputs: Vec::new(),
        }
    }

    /// Every documented B4 field must actually reach the screen — a label present in the layout
    /// but never rendered would defeat the whole point of an inspector.
    #[test]
    fn every_geometry_and_value_field_renders() {
        let ctx = egui::Context::default();
        let popup = sample_popup(Moment::Reflectivity, false, Some(42.5));
        let mut labels = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 800.0),
                )),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| attributes(ui, &popup, None, &[]));
            labels = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) if s.clip_rect.contains(t.pos) => {
                        Some(t.galley.job.text.clone())
                    }
                    _ => None,
                })
                .collect();
        }
        assert!(labels.iter().any(|s| s == "KTLX"), "{labels:?}");
        assert!(labels.iter().any(|s| s == "0.50°"), "{labels:?}");
        assert!(labels.iter().any(|s| s == "123.4°"), "{labels:?}");
        assert!(labels.iter().any(|s| s == "45.60 km"), "{labels:?}");
        assert!(labels.iter().any(|s| s == "45.00 km"), "{labels:?}");
        assert!(labels.iter().any(|s| s == "6200 ft"), "{labels:?}");
        assert!(labels.iter().any(|s| s == "7100 / 5300 ft"), "{labels:?}");
        assert!(labels.iter().any(|s| s == "0.73 km"), "{labels:?}");
        assert!(labels.iter().any(|s| s == "182"), "{labels:?}");
        assert!(labels.iter().any(|s| s == "42.5 dBZ"), "{labels:?}");
    }

    /// Velocity's extra rows (dealiased value, Nyquist) must not appear for a moment they have
    /// no meaning for — showing "Dealiased value —" on a reflectivity gate would just be noise.
    #[test]
    fn dealiasing_and_nyquist_rows_are_velocity_only() {
        let ctx = egui::Context::default();
        let refl = sample_popup(Moment::Reflectivity, false, Some(42.5));
        let vel = sample_popup(Moment::Velocity, false, Some(-12.0));
        for (popup, expect_extra_rows) in [(&refl, false), (&vel, true)] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 800.0),
                )),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| attributes(ui, popup, None, &[]));
            let labels: Vec<String> = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) if s.clip_rect.contains(t.pos) => {
                        Some(t.galley.job.text.clone())
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                labels.iter().any(|s| s == "Dealiased value"),
                expect_extra_rows,
                "{popup:?} -> {labels:?}"
            );
        }
    }

    /// A gate carried over from the previous rotation must say how far behind it is. Without
    /// this the inspector reports the volume's time for a reading that could be a minute older,
    /// which is a position error presented as a measurement.
    #[test]
    fn a_carried_over_gate_reports_its_own_age() {
        let mut popup = sample_popup(Moment::Reflectivity, false, Some(42.5));
        // Sweep ends at t=1_060_000 ms; this gate was collected a rotation earlier.
        popup.inspection.sample.collected_ms = Some(1_060_000 - 180_000);
        let labels = labels_for(&popup, &[]);
        assert!(labels.iter().any(|s| s == "Gate collected"), "{labels:?}");
        assert!(
            labels.iter().any(|s| s.contains("(180 s older)")),
            "{labels:?}"
        );

        // A gate from the current pass must not be labelled old at all.
        popup.inspection.sample.collected_ms = Some(1_055_000);
        let fresh = labels_for(&popup, &[]);
        assert!(
            !fresh.iter().any(|s| s.contains("older")),
            "current-pass gate marked stale: {fresh:?}"
        );
    }

    /// A range-folded gate must say so instead of showing a bogus numeric reading.
    #[test]
    fn a_folded_gate_reads_folded_not_a_number() {
        let ctx = egui::Context::default();
        let popup = sample_popup(Moment::Velocity, true, None);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 800.0),
            )),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| attributes(ui, &popup, None, &[]));
        let labels: Vec<String> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) if s.clip_rect.contains(t.pos) => {
                    Some(t.galley.job.text.clone())
                }
                _ => None,
            })
            .collect();
        assert!(labels.iter().any(|s| s == "Range folded"), "{labels:?}");
    }

    fn labels_for(popup: &GateInspectorPopup, udp: &[wxdata::udp::ProductDef]) -> Vec<String> {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 800.0),
            )),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| attributes(ui, popup, None, udp));
        output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) if s.clip_rect.contains(t.pos) => {
                    Some(t.galley.job.text.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// Phase C1: a saved product shows its name and live-evaluated value, and the whole section
    /// is simply absent when nothing is saved — no empty "USER-DEFINED" heading for nothing.
    #[test]
    fn a_saved_product_evaluates_against_this_gate() {
        let mut popup = sample_popup(Moment::Reflectivity, false, Some(42.5));
        popup.gate_inputs.reflectivity = Some(42.5);
        let none: Vec<String> = labels_for(&popup, &[]);
        assert!(!none.iter().any(|s| s == "USER-DEFINED"), "{none:?}");

        let products = [wxdata::udp::ProductDef {
            name: "Boosted REF".into(),
            units: "dBZ".into(),
            expression: "REF + 10".into(),
        }];
        let with = labels_for(&popup, &products);
        assert!(with.iter().any(|s| s == "USER-DEFINED"), "{with:?}");
        assert!(with.iter().any(|s| s == "Boosted REF"), "{with:?}");
        assert!(with.iter().any(|s| s == "52.50 dBZ"), "{with:?}");
    }

    /// A formula that fails to compile shows the error inline rather than silently dropping the
    /// row or panicking the popup.
    #[test]
    fn a_broken_product_shows_its_error_instead_of_a_value() {
        let popup = sample_popup(Moment::Reflectivity, false, Some(42.5));
        let products = [wxdata::udp::ProductDef {
            name: "Broken".into(),
            units: "".into(),
            expression: "REF +".into(),
        }];
        let labels = labels_for(&popup, &products);
        assert!(labels.iter().any(|s| s.starts_with("Error:")), "{labels:?}");
    }

    /// A formula referencing an input this gate doesn't have reads "—", the same convention
    /// every other missing value in this popup already uses. This popup's reflectivity gate has
    /// a real value (not folded), so the only "—" `attributes` can produce here is this row's.
    #[test]
    fn a_missing_input_reads_as_a_dash() {
        let popup = sample_popup(Moment::Reflectivity, false, Some(42.5));
        // sample_popup's gate_inputs is all-None by default (see `Default::default()` above).
        let products = [wxdata::udp::ProductDef {
            name: "Needs velocity".into(),
            units: "m/s".into(),
            expression: "VEL".into(),
        }];
        let labels = labels_for(&popup, &products);
        assert!(labels.iter().any(|s| s == "Needs velocity"), "{labels:?}");
        assert!(labels.iter().any(|s| s == "—"), "{labels:?}");
    }
}
