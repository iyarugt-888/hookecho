//! A source-independent inspector; timestamps always include their UTC date.
use wxdata::field::DataStamp;

pub(crate) fn show(ui: &mut egui::Ui, stamp: &DataStamp) {
    let now = chrono::Utc::now();
    ui.label(format!("Source: {}", stamp.source_id));
    ui.label(format!("Product: {}", stamp.product_id));
    ui.label(format!("Valid: {}", stamp.valid_time));
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
}
