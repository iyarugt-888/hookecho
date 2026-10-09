//! `hookecho --headless-alert-latency <minutes> [interval_s] --export DIR`: watch the NWS
//! active-alerts feed live and measure how long each new message took from its `sent` time to a
//! poll that saw it (1008.md A1).
//!
//! Each poll's reply time is recorded on the local wall clock beside the server's own `Date`
//! header, so a skewed local clock is visible in the report rather than hidden in the latency.
//! A message's "received" time is the poll that first saw it, so the report's numbers include
//! waiting for the poll: it states the poll interval it ran at, and the app's own interval
//! (`fetch_schedule`, 120 s, 240 s metered), and never calls either on-screen latency.

use std::io::Write as _;
use wxdata::alert_latency::{self, LatencyLog, Stage};

/// The app's overlay refresh (`fetch_schedule.rs`), for the report's arithmetic.
const APP_POLL_S: f64 = 120.0;

pub fn run(minutes: u64, interval_s: u64, export: &str) -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    std::fs::create_dir_all(export)?;
    let http = reqwest::Client::new();
    let mut log = LatencyLog::default();
    let mut polls = std::fs::File::create(format!("{export}/polls.csv"))?;
    writeln!(
        polls,
        "local_utc,server_date_utc,local_minus_server_s,messages,new,error"
    )?;
    let end = wxdata::clock::Instant::now() + std::time::Duration::from_secs(minutes * 60);
    let started = chrono::Utc::now();
    let mut skews: Vec<f64> = Vec::new();
    let mut failures = 0usize;
    while wxdata::clock::Instant::now() < end {
        let tick = wxdata::clock::Instant::now();
        match rt.block_on(wxdata::alerts::fetch_active_raw(&http)) {
            Ok((body, server)) => {
                let received = chrono::Utc::now();
                let infos = wxdata::alerts::parse_alert_infos(&body).unwrap_or_default();
                let new = log.observe(&infos, received, "api.weather.gov");
                // Nothing is drawn headless; the stage is left unmeasured, not zero.
                let skew = server.map(|s| (received - s).num_milliseconds() as f64 / 1000.0);
                skews.extend(skew);
                writeln!(
                    polls,
                    "{},{},{},{},{},",
                    received.to_rfc3339(),
                    server.map(|s| s.to_rfc3339()).unwrap_or_default(),
                    skew.map(|s| format!("{s:.3}")).unwrap_or_default(),
                    infos.len(),
                    new
                )?;
            }
            Err(e) => {
                failures += 1;
                writeln!(
                    polls,
                    "{},,,,,{}",
                    chrono::Utc::now().to_rfc3339(),
                    csv(&e.to_string())
                )?;
            }
        }
        polls.flush()?;
        let next = tick + std::time::Duration::from_secs(interval_s);
        if let Some(wait) = next.checked_duration_since(wxdata::clock::Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    let ended = chrono::Utc::now();

    let mut samples = std::fs::File::create(format!("{export}/samples.csv"))?;
    writeln!(
        samples,
        "id,event,event_key,sent_utc,received_utc,sent_to_received_s,source"
    )?;
    for s in log.samples() {
        writeln!(
            samples,
            "{},{},{},{},{},{:.3},{}",
            csv(&s.id),
            csv(&s.event),
            csv(&s.event_key),
            s.sent.to_rfc3339(),
            s.received.to_rfc3339(),
            s.sent_to_received_s(),
            s.source
        )?;
    }
    let report = report(&log, started, ended, interval_s, &skews, failures);
    std::fs::write(format!("{export}/report.md"), &report)?;
    print!("{report}");
    Ok(())
}

fn csv(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn line(name: &str, st: Option<Stage>) -> String {
    match st {
        Some(st) => format!(
            "| {name} | {} | {:.0} s | {:.0} s |\n",
            st.count, st.p50_s, st.p95_s
        ),
        None => format!("| {name} | 0 | — | — |\n"),
    }
}

/// The readable report: what was measured, how, and what it does not cover.
pub fn report(
    log: &LatencyLog,
    started: chrono::DateTime<chrono::Utc>,
    ended: chrono::DateTime<chrono::Utc>,
    interval_s: u64,
    skews: &[f64],
    failures: usize,
) -> String {
    let all = log.summary(|_| true);
    let warnings = log.summary(|s| alert_latency::is_warning(&s.event));
    let skew = alert_latency::stage(skews.iter().map(|&s| Some(s)));
    let mut out = String::new();
    out.push_str("# NWS alert latency, live\n\n");
    out.push_str(&format!(
        "Watched `api.weather.gov/alerts/active` from {} to {}, polling every {interval_s} s \
         ({} failed polls).\n\n",
        started.format("%Y-%m-%d %H:%M:%SZ"),
        ended.format("%Y-%m-%d %H:%M:%SZ"),
        failures
    ));
    out.push_str("| Messages | Count | p50 | p95 |\n| --- | --- | --- | --- |\n");
    out.push_str(&line(
        "All new, sent → seen by a poll",
        all.sent_to_received,
    ));
    out.push_str(&line(
        "Warnings, sent → seen by a poll",
        warnings.sent_to_received,
    ));
    out.push_str(&format!(
        "\nExcluded: {} already active at the first poll (issued before watching), {} without a \
         `sent` time.\n\n",
        all.excluded_already_active, all.excluded_no_sent
    ));
    match skew {
        Some(s) => out.push_str(&format!(
            "Local clock minus the server's `Date` header (1 s resolution, includes the reply's \
             transfer): p50 {:.1} s, p95 {:.1} s over {} polls. A latency below is shifted by \
             about this much.\n\n",
            s.p50_s, s.p95_s, s.count
        )),
        None => out.push_str("No reply carried a `Date` header; the local clock is unchecked.\n\n"),
    }
    out.push_str(&format!(
        "\"Seen by a poll\" includes waiting for the next poll, up to {interval_s} s here. The \
         app polls alerts with the other overlays every {APP_POLL_S:.0} s (240 s on a metered \
         connection), so in the app a message waits on average about {:.0} s longer than the \
         feed itself took to publish it. Nothing here is drawn: the app's own \"received → \
         drawn\" stage is measured in the running app, not by this watch.\n",
        (APP_POLL_S - interval_s as f64).max(0.0) / 2.0
    ));
    out
}
