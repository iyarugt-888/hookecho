//! Progressive cut acquisition details shared by desktop docks and phone Analyst Mode.

use crate::ui::workstation as ws;

pub(crate) fn show(ui: &mut egui::Ui, t: &ws::Tokens, scan: &crate::live_scan::LiveScan) {
    // Summarize positions only when the operator opens the section.
    egui::CollapsingHeader::new("Cut acquisition details")
        .id_salt("raw-cut-inventory")
        .show(ui, |ui| {
            if let Some(inventory) = scan.acquisition_inventory() {
                paint_acquisition_inventory(ui, t, &inventory);
            } else {
                ui.add(
                    egui::Label::new(ws::text(
                        "No progressive cut inventory received",
                        11.0,
                        t.text_dim,
                    ))
                    .wrap(),
                );
            }
        });
}

fn cut_acquisition_rows(cut: &crate::live_scan::CutAcquisition) -> Vec<(&'static str, String)> {
    let Some(expected) = cut.expected_chunks else {
        return vec![(
            "Coverage",
            "Cut not observed; angle and clocks unknown".into(),
        )];
    };
    let mut rows = vec![
        (
            "Chunks",
            format!("{} / {expected} received", cut.received_chunks),
        ),
        (
            "Raw positions",
            cut.observed_radials.map_or_else(
                || "Unavailable; progress metadata only".into(),
                |count| {
                    format!(
                        "{count} unique positions in {} / {expected} chunks",
                        cut.raw_chunks
                    )
                },
            ),
        ),
    ];
    if !cut.unobserved_chunks.is_empty() {
        rows.push(("Unobserved chunks", format!("{:?}", cut.unobserved_chunks)));
    }
    if cut.observed_radials.is_some() {
        rows.push((
            "Unknown clocks",
            format!("{} observed positions", cut.unknown_clock_radials),
        ));
        let clock = |ms| {
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms).map_or_else(
                || "Unknown".into(),
                |time| time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            )
        };
        if let Some((start, end)) = cut.known_interval_ms {
            rows.push(("Known clock start", clock(start)));
            rows.push(("Known clock end", clock(end)));
        } else {
            rows.push(("Known clocks", "Unavailable".into()));
        }
        if let Some(spans) = crate::live_scan::gap_summary(&cut.internal_unobserved_spans) {
            rows.push(("Bounded holes", spans.replace("missing", "unobserved")));
        }
    }
    rows
}

fn paint_acquisition_inventory(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    inventory: &crate::live_scan::AcquisitionInventory,
) {
    let source_time = inventory
        .volume_start_ms
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
        .map_or_else(
            || "unknown".into(),
            |time| time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        );
    let vcp = inventory
        .vcp_number
        .map_or_else(|| "unknown".into(), |n| n.to_string());
    ui.add(
        egui::Label::new(ws::text(
            format!("Source volume: {source_time} · VCP {vcp}"),
            11.0,
            t.text,
        ))
        .wrap(),
    );
    ui.add(egui::Label::new(ws::text(
        "Live receiver inventory, independent of the playhead. Known clocks cover timed arrivals only. Unobserved positions do not prove transport loss or a complete scan; repeated-pass identity is not established.",
        11.0, t.text_dim,
    )).wrap());
    for cut in &inventory.cuts {
        let angle = cut
            .angle_deg
            .map_or_else(|| "angle unknown".into(), |a| format!("{a:.1}°"));
        let kind = cut.kind.map_or("unobserved", |kind| kind.label());
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(ws::text(
                format!("Cut {} · {angle} · {kind}", cut.number),
                12.0,
                t.text,
            ))
            .wrap(),
        );
        for (label, value) in cut_acquisition_rows(cut) {
            // The complete evidence must remain readable on touch without requiring a hover.
            ui.add(
                egui::Label::new(ws::text(format!("{label}: {value}"), 11.0, t.text_dim)).wrap(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_phosphor::regular as ph;

    fn inventory(raw: bool) -> crate::live_scan::AcquisitionInventory {
        let mut scan = crate::live_scan::LiveScan::default();
        let start = 1_700_000_000_000;
        let p = wxdata::live::ScanProgress {
            volume_start_ms: Some(start),
            vcp_number: Some(212),
            cut_kind: wxdata::live::CutKind::Standard,
            elevation_number: 1,
            total_elevations: 4,
            elevation_angle_deg: 0.5,
            azimuth_rate_dps: 18.0,
            azimuth_start_deg: 0.0,
            azimuth_end_deg: 120.0,
            chunk_index: 1,
            chunks_in_sweep: 3,
        };
        scan.progress(p, chrono::Utc::now());
        if raw {
            scan.observe_radials(
                wxdata::live::RadialCoverage {
                    progress: p,
                    radials: vec![(1, start + 1000), (2, 0), (4, start + 3000)],
                },
                chrono::Utc::now(),
            );
            scan.observe_radials(
                wxdata::live::RadialCoverage {
                    progress: wxdata::live::ScanProgress {
                        elevation_number: 2,
                        cut_kind: wxdata::live::CutKind::Sails,
                        ..p
                    },
                    radials: vec![(1, start + 10000), (2, start + 11000)],
                },
                chrono::Utc::now(),
            );
            scan.progress(
                wxdata::live::ScanProgress {
                    elevation_number: 3,
                    cut_kind: wxdata::live::CutKind::Mrle,
                    ..p
                },
                chrono::Utc::now(),
            );
        }
        scan.acquisition_inventory().unwrap()
    }

    #[test]
    fn raw_acquisition_rows_preserve_unknowns_and_explain_bounded_holes() {
        let source = inventory(true);
        let rows = cut_acquisition_rows(&source.cuts[0]);
        assert!(rows.contains(&("Raw positions", "3 unique positions in 1 / 3 chunks".into())));
        assert!(rows.contains(&("Unknown clocks", "1 observed positions".into())));
        assert!(rows.contains(&("Bounded holes", "1 radial unobserved (#3)".into())));
        let metadata = cut_acquisition_rows(&source.cuts[2]);
        assert!(metadata.contains(&(
            "Raw positions",
            "Unavailable; progress metadata only".into()
        )));
        assert!(!metadata
            .iter()
            .any(|(label, _)| label.starts_with("Known clock")));
        assert_eq!(
            cut_acquisition_rows(&source.cuts[3]),
            [(
                "Coverage",
                "Cut not observed; angle and clocks unknown".into()
            )]
        );
    }

    #[test]
    fn raw_acquisition_details_wrap_at_phone_and_desktop_widths() {
        let ctx = egui::Context::default();
        let t = ws::Tokens::new(egui::Color32::LIGHT_BLUE);
        for width in [240.0, 300.0] {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 1200.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    ws::set_touch(ui.ctx(), width == 240.0);
                    ws::panel_frame(&t).show(ui, |ui| {
                        ws::style_scope(ui, &t);
                        paint_acquisition_inventory(ui, &t, &inventory(true));
                        assert!(
                            ui.min_rect().right() <= width + 1.0,
                            "inventory overflowed {width}px"
                        );
                        assert!(
                            ui.min_rect().bottom() < 1100.0,
                            "inventory exceeds capture height"
                        );
                    });
                },
            );
        }
    }

    #[test]
    #[ignore = "gpu: writes progressive raw cut acquisition captures"]
    fn gpu_raw_acquisition_snapshots() {
        let gpu =
            crate::headless::ui::Snapshot::new().expect("GPU adapter for raw acquisition review");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m1.1/raw-acquisition-ui");
        std::fs::create_dir_all(&destination).unwrap();
        let t = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        for (name, raw) in [("raw", true), ("metadata", false)] {
            for width in [240, 300] {
                gpu.save(
                    &destination.join(format!("{name}-{width}.png")),
                    width,
                    1100,
                    |ui| {
                        ws::set_touch(ui.ctx(), width == 240);
                        ws::panel_frame(&t).show(ui, |ui| {
                            ws::style_scope(ui, &t);
                            ws::window_header(
                                ui,
                                &t,
                                ph::INFO,
                                "Cut acquisition details",
                                None,
                                None,
                            );
                            paint_acquisition_inventory(ui, &t, &inventory(raw));
                        });
                    },
                )
                .unwrap();
            }
        }
    }
}
