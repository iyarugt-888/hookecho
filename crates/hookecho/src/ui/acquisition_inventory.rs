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

/// Native keys describe policy-retained input rows; they do not identify interpolated output cells.
pub(crate) fn contributor_rows<'a>(
    inputs: impl Iterator<Item = Option<&'a wxdata::level2::temporal::PassContributors>>,
    unit: &str,
) -> Vec<(&'static str, String)> {
    let mut total = 0;
    let mut known = 0;
    let mut passes = std::collections::BTreeMap::new();
    let (mut unanchored, mut unknown, mut unavailable) = (0, 0, 0);
    for input in inputs {
        total += 1;
        if let Some(input) = input {
            known += 1;
            for &(key, count) in &input.passes {
                *passes.entry(key).or_insert(0usize) += count;
            }
            unanchored += input.unanchored_rows;
            unknown += input.unknown_clock_rows;
            unavailable += input.unavailable_rows;
        }
    }
    if known == 0 {
        return vec![(
            "Native passes",
            "Exact input row association unavailable".into(),
        )];
    }
    let mut rows = vec![(
        "Native passes",
        format!(
            "{} recorded boundaries; {known} / {total} input sweeps with row evidence",
            passes.len()
        ),
    )];
    for (key, count) in passes.iter().take(8) {
        rows.push((
            "Recorded pass",
            format!(
                "Elevation {} · {} · {count} {unit}",
                key.elevation_number,
                source_clock(key.start_ms)
            ),
        ));
    }
    if passes.len() > 8 {
        rows.push((
            "More passes",
            format!("{} additional recorded boundaries", passes.len() - 8),
        ));
    }
    rows.push(("Unanchored inputs", format!("{unanchored} {unit}")));
    rows.push((
        "Untimed inputs",
        format!("{unknown} {unit}; pass identity unestablished"),
    ));
    rows.push((
        "Unmatched inputs",
        format!("{unavailable} {unit}; no matching retained source evidence"),
    ));
    if known < total {
        rows.push((
            "Unavailable inputs",
            format!(
                "{} input sweep{} lack{} exact row association",
                total - known,
                if total - known == 1 { "" } else { "s" },
                if total - known == 1 { "s" } else { "" }
            ),
        ));
    }
    rows.push(("Pass scope","Recorded source rows retained by the policy. Spatial/range transforms and individual output-cell lineage are separate; pass completeness remains unestablished. Source-time mixed-pass inference is independent.".into()));
    rows
}

pub(crate) fn gate_pass_rows(
    pass: wxdata::live_pass::RowPass,
    site: Option<&str>,
) -> Vec<(&'static str, String)> {
    use wxdata::live_pass::RowPass;
    let mut rows = vec![("Pass radar", site.unwrap_or("Unknown").into())];
    rows.push((
        "Native pass",
        match pass {
            RowPass::Anchored(key) => format!(
                "Elevation {} · recorded start {}",
                key.elevation_number,
                source_clock(key.start_ms)
            ),
            RowPass::Unanchored => "Unanchored; no established native boundary".into(),
            RowPass::UnknownClock => "Unknown radial clock; unique identity unestablished".into(),
            RowPass::Unavailable => "Exact source row association unavailable".into(),
        },
    ));
    rows
}

/// An overlapping plain row's actual writer clock may differ from the unchanged bin maximum.
pub(crate) fn gate_clock_rows(row: Option<i64>, bin: Option<i64>) -> Vec<(&'static str, String)> {
    row.filter(|&ms| {
        ms > 0 && chrono::DateTime::from_timestamp_millis(ms).is_some() && Some(ms) != bin
    })
    .map(|ms| {
        vec![(
            "Native row time",
            format!(
                "{} · recorded writer; bin maximum differs",
                source_clock(ms)
            ),
        )]
    })
    .unwrap_or_default()
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
    paint_inventory(ui, t, inventory, "Live receiver inventory, independent of the playhead. Known clocks cover timed arrivals only. Ordinal coverage does not establish pass identity, transport loss or complete scans. Native source-marked passes are listed separately when available.");
}

pub(crate) fn show_frame(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    name: &str,
    revision: u64,
    receipt: Option<&crate::live_scan::AcquisitionSnapshot>,
) {
    egui::CollapsingHeader::new("Selected frame acquisition")
        .id_salt("accepted-frame-acquisition")
        .show(ui, |ui| paint_frame(ui, t, name, revision, receipt));
}

fn paint_frame(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    name: &str,
    revision: u64,
    receipt: Option<&crate::live_scan::AcquisitionSnapshot>,
) {
    ui.add(
        egui::Label::new(ws::text(
            format!("Frame: {name} · revision {revision}"),
            11.0,
            t.text,
        ))
        .wrap(),
    );
    if let Some(receipt) = receipt {
        paint_inventory(ui, t, receipt.inventory(), "Arrivals retained with this decoded frame. Product coverage describes contributors separately. Known clocks cover timed arrivals only; complete scans and transport loss remain unestablished. Native pass evidence is inspected separately.");
    } else {
        ui.add(egui::Label::new(ws::text("Raw acquisition receipt unavailable for this frame. Completed/archive inputs do not establish progressive arrivals.", 11.0, t.text_dim)).wrap());
    }
}

/// Source arrival evidence is separate from the product's retained/interpolated contributors.
pub(crate) fn receipt_rows(
    receipt: Option<&crate::live_scan::AcquisitionSnapshot>,
) -> Vec<(&'static str, String)> {
    let Some(receipt) = receipt else {
        return vec![("Raw receipt", "Unavailable for this accepted source".into())];
    };
    let inventory = receipt.inventory();
    let raw_cuts = inventory
        .cuts
        .iter()
        .filter(|cut| cut.observed_radials.is_some())
        .count();
    let positions: usize = inventory
        .cuts
        .iter()
        .filter_map(|cut| cut.observed_radials)
        .sum();
    let unknown: usize = inventory
        .cuts
        .iter()
        .map(|cut| cut.unknown_clock_radials)
        .sum();
    let mut rows = vec![
        ("Raw receipt", format!("{raw_cuts} / {} cuts with raw evidence", inventory.cuts.len())),
        ("Raw positions", positions.to_string()),
        ("Untimed raw", unknown.to_string()),
        ("Receipt scope", "Source arrivals; contributor coverage is separate. Native pass evidence is inspected separately; transport loss unestablished.".into()),
    ];
    if let Some(history) = &inventory.source_passes {
        rows.push((
            "Source passes",
            format!(
                "{} boundary-anchored; {} unanchored positions",
                history.passes.len(),
                history.unanchored_positions
            ),
        ));
        if history.unclassified_updates > 0 {
            rows.push((
                "Pass evidence",
                format!(
                    "Unavailable on {} subsequent raw updates",
                    history.unclassified_updates
                ),
            ));
        }
        if history.discontinuous_assemblies > 0 {
            rows.push((
                "Source continuity",
                format!("{} discontinuous assemblies; pass association unestablished across uncertain boundaries", history.discontinuous_assemblies),
            ));
        }
    }
    if let Some(history) = &inventory.source_sequences {
        rows.push(("Source sequence", sequence_origin(&history.origin)));
        rows.push(("Message holes", sequence_spans(&history.bounded_holes)));
        if history.unavailable_updates > 0 {
            rows.push((
                "Sequence evidence",
                format!(
                    "Unavailable on {} subsequent raw updates",
                    history.unavailable_updates
                ),
            ));
        }
    }
    rows
}

fn paint_inventory(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    inventory: &crate::live_scan::AcquisitionInventory,
    scope: &str,
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
    ui.add(egui::Label::new(ws::text(scope, 11.0, t.text_dim)).wrap());
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
    let title = inventory.source_passes.as_ref().map_or_else(
        || "Source-marked passes".into(),
        |history| format!("Source-marked passes ({})", history.passes.len()),
    );
    egui::CollapsingHeader::new(title)
        .id_salt("native-pass-history")
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("native-pass-history-scroll")
                .max_height(280.0)
                .show(ui, |ui| {
                    paint_pass_inventory(ui, t, inventory.source_passes.as_ref());
                });
        });
    egui::CollapsingHeader::new("Source sequence receipts")
        .id_salt("source-sequence-receipts")
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("source-sequence-receipts-scroll")
                .max_height(280.0)
                .show(ui, |ui| {
                    paint_sequence_inventory(ui, t, inventory.source_sequences.as_ref())
                });
        });
}

fn sequence_origin(origin: &wxdata::live_sequence::SequenceOrigin) -> String {
    match origin {
        wxdata::live_sequence::SequenceOrigin::UnidataChunks => {
            "Direct chunks · current source volume".into()
        }
        wxdata::live_sequence::SequenceOrigin::RelayBlocks { upstream_id } => format!(
            "Relay blocks · current subscription and volume · upstream {}",
            upstream_id.as_deref().unwrap_or("unknown")
        ),
    }
}

fn sequence_spans(spans: &[(u64, u64)]) -> String {
    if spans.is_empty() {
        return "None retained".into();
    }
    let mut result = spans
        .iter()
        .take(8)
        .map(|(first, last)| {
            if first == last {
                first.to_string()
            } else {
                format!("{first}–{last}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    if spans.len() > 8 {
        result.push_str(&format!("; {} more spans", spans.len() - 8));
    }
    result
}

fn sequence_rows(
    history: &wxdata::live_sequence::SequenceInventory,
) -> Vec<(&'static str, String)> {
    vec![
        ("Origin", sequence_origin(&history.origin)),
        (
            "Received bounds",
            history.received_bounds.map_or_else(
                || "Unavailable".into(),
                |(first, last)| {
                    format!(
                        "{first}–{last}; {} positions retained",
                        history.retained_received
                    )
                },
            ),
        ),
        (
            "Bounded message holes",
            sequence_spans(&history.bounded_holes),
        ),
        (
            "Failed requests pending",
            sequence_spans(&history.failed_request_spans),
        ),
        (
            "Later arrivals retained",
            sequence_spans(&history.recovered_spans),
        ),
        (
            "Recovery total",
            format!(
                "{} sequence positions arrived after a bounded hole or failed request",
                history.recovered_positions
            ),
        ),
        (
            "Arrival order",
            format!(
                "{} duplicates; {} reversals",
                history.duplicate_arrivals, history.reordered_arrivals
            ),
        ),
        (
            "Download failures",
            format!(
                "{} attempts with known request positions; {} errors with unavailable positions",
                history.failed_download_attempts, history.unlocated_transport_errors
            ),
        ),
        (
            "Decode failures",
            format!(
                "{} assembly attempts; received bytes remain recorded",
                history.failed_decode_attempts
            ),
        ),
        (
            "Retained scope",
            format!(
                "{} older sequence positions retired; {} late arrivals outside retained scope",
                history.retired_positions, history.outside_retained_arrivals
            ),
        ),
        (
            "Unavailable updates",
            history.unavailable_updates.to_string(),
        ),
    ]
}

fn paint_sequence_inventory(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    history: Option<&wxdata::live_sequence::SequenceInventory>,
) {
    let Some(history) = history else {
        ui.add(egui::Label::new(ws::text("Source sequence evidence unavailable. Completed/archive inputs do not establish message arrivals.", 11.0, t.text_dim)).wrap());
        return;
    };
    ui.add(egui::Label::new(ws::text("Message positions describe received source bytes. Holes do not prove lost radials or incomplete scientific passes. An unrequested prefix is unknown. Relay labels do not establish emitter instance identity or independent redundancy. Counters cover this source context; listed spans cover retained positions only.", 11.0, t.text_dim)).wrap());
    for (label, value) in sequence_rows(history) {
        ui.add(egui::Label::new(ws::text(format!("{label}: {value}"), 11.0, t.text_dim)).wrap());
    }
}

fn pass_rows(pass: &wxdata::live_pass::PassSummary) -> Vec<(&'static str, String)> {
    let mut rows = vec![
        ("Native boundary", source_clock(pass.key.start_ms)),
        (
            "Positions",
            format!(
                "{} observed; {} clocks unknown",
                pass.observed_positions, pass.unknown_clock_positions
            ),
        ),
        (
            "Markers retained",
            format!(
                "start {}; end {}",
                if pass.start_marker { "yes" } else { "no" },
                if pass.end_marker { "yes" } else { "no" }
            ),
        ),
    ];
    if let Some((first, last)) = pass.known_interval_ms {
        rows.push(("Known clock start", source_clock(first)));
        rows.push(("Known clock end", source_clock(last)));
    }
    if let Some(gaps) = crate::live_scan::gap_summary(&pass.bounded_unobserved_spans) {
        rows.push(("Bounded holes", gaps.replace("missing", "unobserved")));
    }
    rows
}

fn source_clock(ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms).map_or_else(
        || "Unknown".into(),
        |time| time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    )
}

fn paint_pass_inventory(
    ui: &mut egui::Ui,
    t: &ws::Tokens,
    history: Option<&wxdata::live_pass::PassInventory>,
) {
    let Some(history) = history else {
        ui.add(
            egui::Label::new(ws::text(
                "Native pass-boundary evidence unavailable for this source.",
                11.0,
                t.text_dim,
            ))
            .wrap(),
        );
        return;
    };
    ui.add(egui::Label::new(ws::text("Passes use recorded radar boundary clocks and elevation numbers, scoped to this radar. End markers do not certify every radial. Unanchored joins and untimed starts stay separate; holes do not prove transport loss.", 11.0, t.text_dim)).wrap());
    ui.add(
        egui::Label::new(ws::text(
            format!(
                "Unanchored positions: {} ({} clocks unknown)",
                history.unanchored_positions, history.unanchored_unknown_clocks
            ),
            11.0,
            t.text_dim,
        ))
        .wrap(),
    );
    if history.retired_passes > 0 {
        ui.add(
            egui::Label::new(ws::text(
                format!(
                    "History limit: {} older passes retired",
                    history.retired_passes
                ),
                11.0,
                t.text_dim,
            ))
            .wrap(),
        );
    }
    if history.unclassified_updates > 0 {
        ui.add(
            egui::Label::new(ws::text(
                format!(
                    "Native pass evidence unavailable on {} subsequent raw updates",
                    history.unclassified_updates
                ),
                11.0,
                t.warn,
            ))
            .wrap(),
        );
    }
    if history.discontinuous_assemblies > 0 {
        ui.add(egui::Label::new(ws::text(
            format!("Discontinuous assemblies: {}. Ordinary positions remain unanchored across uncertain source boundaries; this does not prove radial loss.", history.discontinuous_assemblies),
            11.0,
            t.warn,
        )).wrap());
    }
    for pass in history.passes.iter().rev() {
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(ws::text(
                format!("Elevation {} · source pass", pass.key.elevation_number),
                12.0,
                t.text,
            ))
            .wrap(),
        );
        for (label, value) in pass_rows(pass) {
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

    fn sequence_fixture(recovered: bool) -> wxdata::live_sequence::SequenceInventory {
        use wxdata::live_sequence::{SequenceLedger, SequenceOrigin};
        let mut ledger = SequenceLedger::new(SequenceOrigin::RelayBlocks {
            upstream_id: Some("ldm".into()),
        });
        ledger.observe(500);
        ledger.download_failed(501);
        ledger.observe(503);
        ledger.decode_failed();
        ledger.transport_error_without_position();
        if recovered {
            ledger.observe(501);
            ledger.observe(502);
            ledger.observe(502);
        }
        ledger.inventory()
    }

    #[test]
    fn source_sequence_rows_keep_pending_requests_and_recovery_separate_from_native_positions() {
        let before = sequence_rows(&sequence_fixture(false));
        assert!(before.contains(&("Bounded message holes", "501–502".into())));
        assert!(before.contains(&("Failed requests pending", "501".into())));
        let after = sequence_rows(&sequence_fixture(true));
        assert!(after.contains(&("Bounded message holes", "None retained".into())));
        assert!(after.contains(&("Later arrivals retained", "501–502".into())));
        assert!(after.contains(&("Arrival order", "1 duplicates; 1 reversals".into())));
        assert_eq!(
            sequence_spans(&(0..20).map(|n| (n * 2, n * 2)).collect::<Vec<_>>()),
            "0, 2, 4, 6, 8, 10, 12, 14; 12 more spans"
        );
    }

    #[test]
    fn source_sequence_receipts_wrap_on_desktop_and_touch_with_unknown_or_recovered_evidence() {
        let before = sequence_fixture(false);
        let after = sequence_fixture(true);
        let ctx = egui::Context::default();
        let t = ws::Tokens::new(egui::Color32::LIGHT_BLUE);
        for width in [240.0, 300.0] {
            for history in [Some(&before), Some(&after), None] {
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        ws::set_touch(ui.ctx(), width == 240.0);
                        ws::panel_frame(&t).show(ui, |ui| {
                            ws::style_scope(ui, &t);
                            paint_sequence_inventory(ui, &t, history);
                            assert!(ui.min_rect().right() <= width + 1.0);
                            assert!(ui.min_rect().bottom() < 900.0);
                        });
                    },
                );
            }
        }
    }

    #[test]
    #[ignore = "gpu: writes source sequence receipt captures"]
    fn gpu_source_sequence_snapshots() {
        let gpu =
            crate::headless::ui::Snapshot::new().expect("GPU adapter for sequence receipt review");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m1.1/source-sequence-ui");
        std::fs::create_dir_all(&destination).unwrap();
        let t = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        let before = sequence_fixture(false);
        let after = sequence_fixture(true);
        for (name, history) in [
            ("holes", Some(&before)),
            ("recovered", Some(&after)),
            ("unavailable", None),
        ] {
            for width in [240, 300] {
                gpu.save(
                    &destination.join(format!("{name}-{width}.png")),
                    width,
                    900,
                    |ui| {
                        ws::set_touch(ui.ctx(), width == 240);
                        ws::panel_frame(&t).show(ui, |ui| {
                            ws::style_scope(ui, &t);
                            ws::window_header(
                                ui,
                                &t,
                                ph::INFO,
                                "Source sequence receipts",
                                None,
                                None,
                            );
                            paint_sequence_inventory(ui, &t, history);
                        });
                    },
                )
                .unwrap();
            }
        }
    }

    fn pass_fixture() -> wxdata::live_pass::PassInventory {
        use wxdata::live_pass::{PassArrival, PassKey, PassLedger};
        let start = 1_700_000_000_000;
        let mut ledger = PassLedger::default();
        ledger.observe(&[
            PassArrival {
                elevation_number: 1,
                key: Some(PassKey {
                    elevation_number: 1,
                    start_ms: start,
                }),
                start_marker: true,
                end_marker: true,
                radials: vec![(1, start), (2, 0), (4, start + 3000)],
            },
            PassArrival {
                elevation_number: 1,
                key: Some(PassKey {
                    elevation_number: 1,
                    start_ms: start + 20000,
                }),
                start_marker: true,
                end_marker: false,
                radials: vec![(1, start + 20000), (2, 0)],
            },
            PassArrival {
                elevation_number: 2,
                key: None,
                start_marker: false,
                end_marker: false,
                radials: vec![(121, start + 30000), (122, 0)],
            },
        ]);
        ledger.record_unclassified();
        ledger.record_discontinuous_assembly();
        ledger.inventory()
    }

    #[test]
    fn source_pass_details_separate_recorded_markers_from_complete_radial_coverage() {
        let history = pass_fixture();
        let base = pass_rows(&history.passes[0]);
        assert!(base.contains(&("Native boundary", "2023-11-14T22:13:20.000Z".into())));
        assert!(base.contains(&("Markers retained", "start yes; end yes".into())));
        assert!(base.contains(&("Bounded holes", "1 radial unobserved (#3)".into())));
        assert!(base.contains(&("Positions", "3 observed; 1 clocks unknown".into())));
        let revisit = pass_rows(&history.passes[1]);
        assert!(revisit.contains(&("Native boundary", "2023-11-14T22:13:40.000Z".into())));
        assert!(revisit.contains(&("Markers retained", "start yes; end no".into())));
        assert_eq!(history.unanchored_positions, 2);
        assert_eq!(history.unanchored_unknown_clocks, 1);
        assert_eq!(history.discontinuous_assemblies, 1);
    }

    #[test]
    fn source_pass_details_wrap_with_unanchored_and_unavailable_evidence() {
        let history = pass_fixture();
        let ctx = egui::Context::default();
        let t = ws::Tokens::new(egui::Color32::LIGHT_BLUE);
        for width in [240.0, 300.0] {
            for history in [Some(&history), None] {
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        ws::set_touch(ui.ctx(), width == 240.0);
                        ws::panel_frame(&t).show(ui, |ui| {
                            ws::style_scope(ui, &t);
                            paint_pass_inventory(ui, &t, history);
                            assert!(ui.min_rect().right() <= width + 1.0);
                            assert!(ui.min_rect().bottom() < 900.0);
                        });
                    },
                );
            }
        }
    }

    #[test]
    #[ignore = "gpu: writes native source pass history captures"]
    fn gpu_source_pass_snapshots() {
        let gpu = crate::headless::ui::Snapshot::new().expect("GPU adapter for native pass review");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m1.1/source-pass-continuity-ui");
        std::fs::create_dir_all(&destination).unwrap();
        let t = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        let history = pass_fixture();
        for (name, history) in [("history", Some(&history)), ("unavailable", None)] {
            for width in [240, 300] {
                gpu.save(
                    &destination.join(format!("{name}-{width}.png")),
                    width,
                    900,
                    |ui| {
                        ws::set_touch(ui.ctx(), width == 240);
                        ws::panel_frame(&t).show(ui, |ui| {
                            ws::style_scope(ui, &t);
                            ws::window_header(ui, &t, ph::INFO, "Source-marked passes", None, None);
                            paint_pass_inventory(ui, &t, history);
                        });
                    },
                )
                .unwrap();
            }
        }
    }

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
                    source_passes: None,
                    source_sequences: None,
                    source_attribution: None,
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
                    source_passes: None,
                    source_sequences: None,
                    source_attribution: None,
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
    fn frame_receipt_rows_keep_source_arrivals_distinct_from_product_coverage() {
        let (_, receipt) = crate::live_scan::acquisition_fixture("KTLX");
        let rows = receipt_rows(Some(&receipt));
        assert!(rows.contains(&("Raw receipt", "1 / 3 cuts with raw evidence".into())));
        assert!(rows.contains(&("Raw positions", "3".into())));
        assert!(rows.contains(&("Untimed raw", "1".into())));
        assert!(rows
            .last()
            .unwrap()
            .1
            .contains("contributor coverage is separate"));
        assert_eq!(
            receipt_rows(None),
            [("Raw receipt", "Unavailable for this accepted source".into())]
        );
    }

    #[test]
    fn selected_frame_acquisition_wraps_with_and_without_raw_evidence() {
        let (_, receipt) = crate::live_scan::acquisition_fixture("KTLX");
        let ctx = egui::Context::default();
        let t = ws::Tokens::new(egui::Color32::LIGHT_BLUE);
        for width in [240.0, 300.0] {
            for evidence in [Some(&receipt), None] {
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        ws::set_touch(ui.ctx(), width == 240.0);
                        ws::panel_frame(&t).show(ui, |ui| {
                            ws::style_scope(ui, &t);
                            paint_frame(ui, &t, "KTLX20231114_221320_V06", 7, evidence);
                            assert!(
                                ui.min_rect().right() <= width + 1.0,
                                "frame receipt overflowed {width}px"
                            );
                            assert!(ui.min_rect().bottom() < 900.0);
                        });
                    },
                );
            }
        }
    }

    #[test]
    #[ignore = "gpu: writes accepted frame acquisition captures"]
    fn gpu_frame_acquisition_snapshots() {
        let gpu =
            crate::headless::ui::Snapshot::new().expect("GPU adapter for frame acquisition review");
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-review/m1.1/frame-acquisition-ui");
        std::fs::create_dir_all(&destination).unwrap();
        let t = ws::Tokens::new(egui::Color32::from_rgb(72, 142, 226));
        let (mut receiver, receipt) = crate::live_scan::acquisition_fixture("KTLX");
        // The retained accepted receipt must still show #3 unobserved after the receiver fills it.
        receiver.observe_radials(
            wxdata::live::RadialCoverage {
                progress: receiver.progress.unwrap(),
                source_passes: None,
                source_sequences: None,
                source_attribution: None,
                radials: vec![(3, 0)],
            },
            chrono::Utc::now(),
        );
        assert!(receiver.acquisition_inventory().unwrap().cuts[0]
            .internal_unobserved_spans
            .is_empty());
        assert_eq!(
            receipt.inventory().cuts[0].internal_unobserved_spans,
            [(3, 3)]
        );
        for (name, evidence) in [("accepted", Some(&receipt)), ("unavailable", None)] {
            for width in [240, 300] {
                gpu.save(
                    &destination.join(format!("{name}-{width}.png")),
                    width,
                    900,
                    |ui| {
                        ws::set_touch(ui.ctx(), width == 240);
                        ws::panel_frame(&t).show(ui, |ui| {
                            ws::style_scope(ui, &t);
                            ws::window_header(
                                ui,
                                &t,
                                ph::INFO,
                                "Selected frame acquisition",
                                None,
                                None,
                            );
                            paint_frame(ui, &t, "KTLX20231114_221320_V06", 7, evidence);
                        });
                    },
                )
                .unwrap();
            }
        }
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
    #[test]
    fn contributor_rows_bound_detail_and_keep_unavailable_and_untimed_evidence_explicit() {
        use wxdata::level2::temporal::PassContributors;
        use wxdata::live_pass::{PassKey, RowPass};
        let summary = PassContributors {
            passes: (0..10)
                .map(|i| {
                    (
                        PassKey {
                            elevation_number: 1,
                            start_ms: 1_700_000_000_000 + i * 1000,
                        },
                        120,
                    )
                })
                .collect(),
            unanchored_rows: 3,
            unknown_clock_rows: 2,
            unavailable_rows: 1,
        };
        let rows = contributor_rows(
            [Some(&summary), Some(&summary), None].into_iter(),
            "input azimuth rows",
        );
        assert_eq!(
            rows.iter().filter(|(k, _)| *k == "Recorded pass").count(),
            8
        );
        assert!(rows.contains(&("More passes", "2 additional recorded boundaries".into())));
        assert!(rows.contains(&(
            "Untimed inputs",
            "4 input azimuth rows; pass identity unestablished".into()
        )));
        assert!(rows.contains(&("Unanchored inputs", "6 input azimuth rows".into())));
        for pass in [
            RowPass::Unanchored,
            RowPass::UnknownClock,
            RowPass::Unavailable,
        ] {
            let gate = gate_pass_rows(pass, None);
            assert!(gate.contains(&("Pass radar", "Unknown".into())));
            assert!(!gate.iter().any(|(_, v)| v.contains("recorded start")));
        }
    }
}
