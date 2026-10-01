//! Bounded layer-probe presentation, including the shared provenance inspector.
use super::ProbeLine;

pub(super) fn card_width(pane_width: f32) -> f32 {
    (pane_width - 36.0).clamp(1.0, 300.0)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Presentation state supplied by the map owner"
)]
pub(super) fn show(
    ui: &mut egui::Ui,
    lines: &[ProbeLine],
    pinned: bool,
    point: [f64; 2],
    width: f32,
    pane_height: f32,
    analysis_time: Option<chrono::DateTime<chrono::Utc>>,
    tolerance: chrono::Duration,
) -> bool {
    ui.set_width(width);
    let mut unpin = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(format!("{:.3}, {:.3}", point[1], point[0]))
                .strong()
                .size(12.0),
        );
        if pinned {
            unpin = ui.small_button("Unpin").clicked();
        } else {
            ui.weak("click to pin");
        }
    });
    egui::ScrollArea::vertical()
        .id_salt("layer_probe_body")
        .max_height((pane_height - 68.0).max(1.0))
        .min_scrolled_height(0.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if lines.is_empty() {
                ui.weak("Nothing on the map here");
            }
            for (index, line) in lines.iter().enumerate() {
                if pinned {
                    ui.push_id(index, |ui| {
                        let id = ui.make_persistent_id("source_row");
                        let mut clicked = false;
                        let mut header =
                            egui::collapsing_header::CollapsingState::load_with_default_open(
                                ui.ctx(),
                                id,
                                false,
                            )
                            .show_header(ui, |ui| {
                                clicked = ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(format!(
                                                "{} · {}",
                                                line.layer, line.value
                                            ))
                                            .size(12.0),
                                        )
                                        .frame(false)
                                        .wrap(),
                                    )
                                    .clicked();
                            });
                        if clicked {
                            header.toggle();
                        }
                        header.body(|ui| show_source(ui, line, analysis_time, tolerance));
                    });
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(&line.layer).size(11.5).weak());
                        ui.label(egui::RichText::new(&line.value).size(12.0).strong());
                    });
                }
            }
            if pinned && !lines.is_empty() {
                ui.weak("Expand a layer to inspect sources and times");
            } else if !lines.is_empty() {
                ui.weak("Pin to inspect sources and times");
            }
        });
    unpin
}

fn show_source(
    ui: &mut egui::Ui,
    line: &ProbeLine,
    analysis_time: Option<chrono::DateTime<chrono::Utc>>,
    tolerance: chrono::Duration,
) {
    if let Some(detail) = &line.detail {
        ui.label(detail);
    }
    if let Some(field) = line.field.and_then(crate::render::FieldLayer::descriptor) {
        ui.label(format!(
            "Native units: {} · {:?}",
            field.units.symbol(),
            field.value_kind
        ));
        ui.label(format!(
            "Missing/no coverage codes: {:?} (masked)",
            field.missing_values
        ));
    }
    if let Some(stamp) = &line.stamp {
        crate::ui::data_inspector::show(ui, stamp, analysis_time, tolerance);
    } else {
        ui.weak("Full source stamp unavailable; only retained metadata is shown.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forecast() -> ProbeLine {
        let valid = chrono::DateTime::parse_from_rfc3339("2026-10-01T18:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let mut line = ProbeLine::new(
            "HRRR surface CAPE",
            "1234 J/kg",
            Some("HRRR · valid 2026-10-01 18:00 UTC".into()),
        );
        line.field = Some(crate::render::FieldLayer::Cape);
        line.stamp = Some(wxdata::field::DataStamp {
            source_id: "NOAA HRRR".into(),
            product_id: "surface-cape".into(),
            valid_time: valid,
            run_time: Some(valid - chrono::Duration::hours(3)),
            issue_time: None,
            received_time: valid - chrono::Duration::hours(2),
            source_latency: None,
            is_forecast: true,
            is_derived: false,
            quality: wxdata::field::QualitySummary::Unknown,
            grid: None,
        });
        line
    }

    #[test]
    fn expanded_source_inspector_fits_narrow_host_and_retains_the_stamp() {
        let line = forecast();
        let original = line.stamp.clone();
        let ctx = egui::Context::default();
        for width in [180.0, 300.0] {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 900.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    show_source(ui, &line, None, chrono::Duration::minutes(10));
                    assert!(ui.min_rect().width() <= width);
                },
            );
        }
        assert_eq!(line.stamp, original);
        assert!(original.unwrap().is_forecast);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "gpu: writes layer probe captures for visual review"]
    fn gpu_layer_probe_snapshots() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for probe review");
        let destination =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-review");
        std::fs::create_dir_all(&destination).unwrap();
        let tokens = crate::ui::workstation::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        for width in [220, 340] {
            for expanded in [false, true] {
                let name = if expanded { "source" } else { "card" };
                gpu.save(
                    &destination.join(format!("layer-probe-{name}-{width}.png")),
                    width,
                    640,
                    |ui| {
                        crate::ui::workstation::card_frame(&tokens)
                            .inner_margin(8)
                            .show(ui, |ui| {
                                crate::ui::workstation::style_scope(ui, &tokens);
                                ui.set_width(card_width(width as f32));
                                if expanded {
                                    show_source(
                                        ui,
                                        &forecast(),
                                        None,
                                        chrono::Duration::minutes(10),
                                    );
                                } else {
                                    let mut lines = vec![
                                        ProbeLine::new(
                                            "REF 0.5°",
                                            "52 dBZ",
                                            Some("KTLX, tilt acquired 2026-10-01 17:57 UTC".into()),
                                        ),
                                        forecast(),
                                    ];
                                    for index in 0..25 {
                                        lines.push(ProbeLine::new(
                                            format!("Additional active field {index}"),
                                            "—",
                                            None,
                                        ));
                                    }
                                    show(
                                        ui,
                                        &lines,
                                        true,
                                        [-97.0, 35.0],
                                        card_width(width as f32),
                                        640.0,
                                        None,
                                        chrono::Duration::minutes(10),
                                    );
                                }
                            });
                    },
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn card_width_stays_inside_narrow_panes() {
        for pane in [80.0, 180.0, 280.0, 640.0] {
            assert!(card_width(pane) + 36.0 <= pane);
            assert!(card_width(pane) <= 300.0);
        }
    }

    #[test]
    fn many_probe_rows_remain_inside_the_scrolling_host() {
        let ctx = egui::Context::default();
        let lines: Vec<_> = (0..40)
            .map(|i| {
                ProbeLine::new(
                    format!("Model forecast field {i}"),
                    "1234 J/kg",
                    Some("Retained source metadata".into()),
                )
            })
            .collect();
        for width in [180.0, 300.0] {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 240.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    show(
                        ui,
                        &lines,
                        true,
                        [-97.0, 35.0],
                        width,
                        240.0,
                        None,
                        chrono::Duration::minutes(10),
                    );
                    assert!(
                        ui.min_rect().width() <= width,
                        "{width} pt host widened to {:?}",
                        ui.min_rect()
                    );
                    assert!(
                        ui.min_rect().height() <= 240.0,
                        "host grew to {:?}",
                        ui.min_rect()
                    );
                },
            );
        }
    }
}
