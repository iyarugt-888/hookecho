//! A source-independent inspector; timestamps always include their UTC date.
use chrono::{DateTime, Duration, Utc};
use wxdata::field::DataStamp;
use wxdata::time_align::TimeOffset;

pub(crate) fn offset_label(offset: Duration) -> String {
    let seconds = offset.num_seconds();
    let sign = if seconds >= 0 { '+' } else { '-' };
    let seconds = seconds.unsigned_abs();
    if seconds >= 3600 {
        format!("{sign}{}h {:02}m", seconds / 3600, seconds % 3600 / 60)
    } else if seconds >= 60 {
        format!("{sign}{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{sign}{seconds}s")
    }
}

pub(crate) fn show(
    ui: &mut egui::Ui,
    stamp: &DataStamp,
    analysis_time: Option<DateTime<Utc>>,
    tolerance: Duration,
) {
    let now = chrono::Utc::now();
    ui.label(format!("Source: {}", stamp.source_id));
    ui.label(format!("Product: {}", stamp.product_id));
    ui.label(format!("Valid: {}", stamp.valid_time));
    if let Some(analysis_time) = analysis_time {
        let comparison = TimeOffset::between(stamp.valid_time, analysis_time, tolerance);
        let prefix = if comparison.outside_tolerance {
            "⚠ "
        } else {
            ""
        };
        let relation = if comparison.offset < Duration::zero() {
            "older"
        } else if comparison.offset > Duration::zero() {
            "newer"
        } else {
            "aligned"
        };
        ui.label(format!(
            "{prefix}Radar scan: {analysis_time} · source offset {} ({relation})",
            offset_label(comparison.offset)
        ));
        if comparison.outside_tolerance {
            ui.label("Source valid time is outside the configured layer time tolerance.");
        }
    }
    ui.label(format!("Received: {}", stamp.received_time));
    ui.label(format!(
        "Valid-time age: {} s · receipt age: {} s",
        stamp.age_at(now).num_seconds(),
        stamp.receipt_age_at(now).num_seconds()
    ));
    for (name, time) in [("Issue", stamp.issue_time), ("Run", stamp.run_time)] {
        ui.label(format!(
            "{name}: {}",
            time.map(|t| t.to_string())
                .unwrap_or_else(|| "Unknown".into())
        ));
    }
    ui.label(format!(
        "Provider ingest latency: {}",
        stamp
            .source_latency
            .map(|d| format!("{} s", d.num_seconds()))
            .unwrap_or_else(|| "Unknown".into())
    ));
    ui.label(format!(
        "Forecast: {} · Derived: {}",
        stamp.is_forecast, stamp.is_derived
    ));
    ui.label(format!("Quality: {:?}", stamp.quality));
    if let Some(grid) = &stamp.grid {
        ui.label(format!(
            "Native grid: {} × {} (longitude/latitude)",
            grid.native.nx, grid.native.ny
        ));
        ui.label(format!(
            "Native bounds [W, S, E, N]: {:?}",
            grid.native.bounds
        ));
        ui.label(format!(
            "Display grid: {} × {}",
            grid.displayed.nx, grid.displayed.ny
        ));
        let transform = match grid.transform {
            wxdata::field::DisplayTransform::Native => "Native grid values".to_string(),
            wxdata::field::DisplayTransform::MaximumPool { factor } => {
                format!("Maximum pooling, {factor} × {factor} cells")
            }
            wxdata::field::DisplayTransform::NearestCell => {
                "Nearest-cell resampling (categorical)".to_string()
            }
        };
        ui.label(format!("Display transform: {transform}"));
    }
}
