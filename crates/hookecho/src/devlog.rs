//! The developer log: every `log::` record this process emits, captured into a small buffer and,
//! opt-in, shipped to a separate admin viewer (`hookecho --devlog-serve`, see
//! [`crate::devlog_admin`]) — so a live-sweep stall, a TDS/TVS false trip, or a browser-only error
//! can be filtered and searched without bloating the app's own UI or asking whoever hit it to
//! paste a console.
//!
//! Every existing `log::warn!`/`info!`/`debug!` call site is captured automatically: this wraps
//! whichever logger already prints to a terminal or the browser console, so nothing here changes
//! what a call site says, only where else it ends up. `record.target()` — the module path, unless
//! a call site overrides it — is the category the admin panel filters on: `wxdata::tds`,
//! `wxdata::rotation`, `hookecho::app` and so on need no registration here.
//!
//! Off by default and loopback-shaped like [`crate::serve`]: a normal run never opens a socket or
//! reads an env var for this. `HOOKECHO_DEVLOG=http://127.0.0.1:8884/ingest` (native — or just
//! `=1` for that default) or `?devlog=http://host:8884/ingest` in the page URL (web) turns
//! shipping on for that one launch.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// One record, exactly as it goes over the wire to the admin server.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct LogEntry {
    pub ts_ms: i64,
    pub level: String,
    pub target: String,
    pub message: String,
}

/// A batch posted to `/ingest` — instance identity travels once per batch, not once per entry.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct IngestBatch {
    pub instance: String,
    pub source: String,
    pub version: String,
    pub os: String,
    pub entries: Vec<LogEntry>,
}

/// How many unshipped records are kept before the oldest are dropped. Protects memory when
/// nothing is listening on the configured endpoint (or shipping is off entirely and nobody ever
/// drains this) — not a limit that matters in ordinary operation, where the shipper empties it
/// every few seconds.
const CAPACITY: usize = 8_000;

static BUFFER: OnceLock<Mutex<VecDeque<LogEntry>>> = OnceLock::new();

fn buffer() -> &'static Mutex<VecDeque<LogEntry>> {
    BUFFER.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn capture(record: &log::Record) {
    let entry = LogEntry {
        ts_ms: chrono::Utc::now().timestamp_millis(),
        level: record.level().to_string(),
        target: record.target().to_string(),
        message: format!("{}", record.args()),
    };
    let Ok(mut buf) = buffer().lock() else { return };
    buf.push_back(entry);
    while buf.len() > CAPACITY {
        buf.pop_front();
    }
}

/// Take everything buffered since the last call.
///
/// Best-effort delivery: a batch a dead endpoint never accepts is simply gone. That is the right
/// trade for a debug tool over holding an unbounded retry queue for a viewer that may never be
/// listening in the first place.
fn drain() -> Vec<LogEntry> {
    let Ok(mut buf) = buffer().lock() else {
        return Vec::new();
    };
    buf.drain(..).collect()
}

/// The most recent `limit` warning/error records, oldest first, *without* removing them — unlike
/// [`drain`], which the shipper depends on to hand a batch off exactly once. ROADMAP_NEW N4's
/// local diagnostics bundle is the one caller: a snapshot for a one-off export must not silently
/// steal entries the shipper (if it's running) still needs to send.
pub fn recent_warnings(limit: usize) -> Vec<LogEntry> {
    let Ok(buf) = buffer().lock() else {
        return Vec::new();
    };
    let mut out: Vec<LogEntry> = buf
        .iter()
        .rev()
        .filter(|e| e.level == "WARN" || e.level == "ERROR")
        .take(limit)
        .cloned()
        .collect();
    out.reverse();
    out
}

/// The most recent `limit` records at *any* level whose target starts with one of
/// `target_prefixes`, oldest first, without removing them (same non-destructive contract as
/// [`recent_warnings`] — a live viewer reading this must not steal entries the shipper, if
/// running, still needs to send). theme_plan.md §4's Analyst Mode is the one caller: `debug!`-
/// level detail (live-sweep chunk arrival, provider/failover health) only ever reaches this
/// buffer at all once something has raised the ambient level past the default `info` filter —
/// see [`raise_level_for_analyst_mode`] — this function only reads what's already there.
pub fn recent(limit: usize, target_prefixes: &[&str]) -> Vec<LogEntry> {
    let Ok(buf) = buffer().lock() else {
        return Vec::new();
    };
    let mut out: Vec<LogEntry> = buf
        .iter()
        .rev()
        .filter(|e| target_prefixes.iter().any(|p| e.target.starts_with(p)))
        .take(limit)
        .cloned()
        .collect();
    out.reverse();
    out
}

/// The ambient log level from just before Analyst Mode last raised it, so turning it back off
/// restores exactly what was there — a user who already set `RUST_LOG=trace` themselves shouldn't
/// have Analyst Mode quietly demote them back to `info` on exit. `None` means Analyst Mode isn't
/// currently the reason the level is raised (either it was never turned on, or it's already been
/// restored).
static ANALYST_MODE_PREV_LEVEL: Mutex<Option<log::LevelFilter>> = Mutex::new(None);

/// An *additional* capture threshold, independent of whatever [`native::NativeLogger`]'s wrapped
/// `env_logger::Logger` or [`web::WebLogger`]'s own `level` field allow through to the terminal or
/// console. Both of those are built once, from `RUST_LOG`/a fixed level, and have no public API to
/// change their filter afterward — so raising [`set_analyst_mode`]'s global `log::max_level` alone
/// is not enough: a `debug!` record would clear that first gate but then still be rejected by the
/// wrapped logger's own unchanged filter before `capture()` is ever called. This is the second,
/// actually-adjustable gate each logger's `log()` also checks. `Off` (the default) adds nothing —
/// capture then depends only on the wrapped logger's own filter, exactly as before Analyst Mode
/// existed.
static CAPTURE_LEVEL: Mutex<log::LevelFilter> = Mutex::new(log::LevelFilter::Off);

fn capture_level() -> log::LevelFilter {
    CAPTURE_LEVEL
        .lock()
        .map(|g| *g)
        .unwrap_or(log::LevelFilter::Off)
}

/// Raise (or restore) the process-wide log level for Analyst Mode. Two things happen together,
/// both necessary: `log::set_max_level` gates `log::debug!`/`trace!` call sites *before* a
/// `Record` is even constructed, and [`CAPTURE_LEVEL`] is the second gate each logger's `log()`
/// checks once a `Record` does exist — see that constant's own doc comment for why both are
/// needed. Idempotent: calling with the same `on` value twice in a row is a no-op the second time.
pub fn set_analyst_mode(on: bool) {
    let Ok(mut prev) = ANALYST_MODE_PREV_LEVEL.lock() else {
        return;
    };
    match (on, *prev) {
        (true, None) => {
            let before = log::max_level();
            *prev = Some(before);
            log::set_max_level(log::LevelFilter::Debug.max(before));
            if let Ok(mut cap) = CAPTURE_LEVEL.lock() {
                *cap = log::LevelFilter::Debug;
            }
        }
        (false, Some(before)) => {
            log::set_max_level(before);
            if let Ok(mut cap) = CAPTURE_LEVEL.lock() {
                *cap = log::LevelFilter::Off;
            }
            *prev = None;
        }
        _ => {} // already in the requested state
    }
}

/// A per-launch id, stable for the process (native) or page load (web), so the admin panel can
/// tell one instance's chatter from another's without either naming itself.
pub fn instance_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        format!(
            "{:x}-{:x}",
            chrono::Utc::now().timestamp_millis(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    })
}

fn take_batch(source: &'static str, os: &'static str) -> Option<IngestBatch> {
    let entries = drain();
    if entries.is_empty() {
        return None;
    }
    Some(IngestBatch {
        instance: instance_id().to_string(),
        source: source.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        os: os.to_string(),
        entries,
    })
}

/// How often the shipper wakes up to flush whatever has buffered.
const SHIP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3);
/// How long one POST may take before it's abandoned — the same reasoning as
/// `wxdata::task::timeout`'s doc comment: an unreachable admin panel must not pile up requests.
const SHIP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Post buffered batches to `endpoint` forever, at [`SHIP_INTERVAL`]. Shared between native (run
/// on a dedicated thread's own tiny runtime) and web (run via `spawn_local`) — the only thing that
/// differs between them is how this future gets polled, which `wxdata::task` already abstracts.
async fn ship_loop(endpoint: String, source: &'static str, os: &'static str) {
    let http = reqwest::Client::new();
    loop {
        wxdata::task::sleep(SHIP_INTERVAL).await;
        let Some(batch) = take_batch(source, os) else {
            continue;
        };
        // `serde_json::to_vec` + a plain body rather than `RequestBuilder::json`: that needs
        // reqwest's `json` feature, which nothing else here pulls in for one POST a shipper makes
        // every few seconds.
        let Ok(body) = serde_json::to_vec(&batch) else {
            continue;
        };
        let send = http
            .post(&endpoint)
            .header("Content-Type", "application/json")
            .body(body)
            .send();
        // Errors (refused, timed out, 4xx/5xx) are swallowed on purpose: this is telemetry for a
        // viewer that may not be running, not a delivery guarantee anything else depends on.
        let _ = wxdata::task::timeout(SHIP_TIMEOUT, send).await;
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;

    /// Wraps whatever logger `main.rs` built (normally `env_logger`), forwarding every record to
    /// it unchanged and capturing the ones that pass its own filter — so `RUST_LOG` still governs
    /// exactly what used to reach the terminal, and the buffer never fills with noise the terminal
    /// itself would have suppressed.
    struct NativeLogger {
        inner: env_logger::Logger,
    }

    impl log::Log for NativeLogger {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            self.inner.enabled(metadata)
        }

        fn log(&self, record: &log::Record) {
            // `matches()` is `env_logger`'s own fixed filter (RUST_LOG at startup); `capture_level`
            // is Analyst Mode's independently-adjustable one — see that function's doc comment for
            // why both are checked rather than just one. Printing to the terminal stays governed
            // by `RUST_LOG` alone (`self.inner.log` below), unaffected by Analyst Mode.
            if self.inner.matches(record) || record.level() <= capture_level() {
                capture(record);
            }
            self.inner.log(record);
        }

        fn flush(&self) {
            self.inner.flush();
        }
    }

    /// Install the process-wide logger: everything `env_logger` already did (terminal output,
    /// `RUST_LOG` filtering), plus capture into the buffer this module drains for shipping.
    pub fn install(mut builder: env_logger::Builder) {
        let inner = builder.build();
        log::set_max_level(inner.filter());
        let _ = log::set_boxed_logger(Box::new(NativeLogger { inner }));
    }

    /// `HOOKECHO_DEVLOG=<url>` (or `=1`/`=true` for the local default) starts a background thread
    /// that posts buffered entries to that URL every few seconds. Unset — the default — this does
    /// nothing at all: no thread, no socket, no env var read anywhere else in the app.
    pub fn maybe_spawn_shipper() {
        let Ok(raw) = std::env::var("HOOKECHO_DEVLOG") else {
            return;
        };
        if raw.trim().is_empty() {
            return;
        }
        let endpoint = match raw.trim() {
            "1" | "true" => "http://127.0.0.1:8884/ingest".to_string(),
            other => other.to_string(),
        };
        let spawned = std::thread::Builder::new()
            .name("devlog-shipper".to_string())
            .spawn(move || {
                // A dedicated single-thread runtime rather than the app's own: this has no
                // relationship to the render loop or the feed fetchers, and starting it before
                // `HookEchoApp` exists (the `--serve` path never builds one at all) would
                // otherwise be a chicken-and-egg problem.
                let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                rt.block_on(ship_loop(endpoint, "native", std::env::consts::OS));
            });
        if let Err(e) = spawned {
            log::warn!("devlog shipper not started: {e}");
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub use native::{install as install_native, maybe_spawn_shipper as maybe_spawn_native_shipper};

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;

    /// Wraps `console_log`, forwarding every record to it unchanged (so the browser console
    /// output nobody looking at a phone or a friend's laptop can otherwise see is unaffected) and
    /// capturing the ones that pass the level filter.
    struct WebLogger {
        level: log::LevelFilter,
    }

    impl log::Log for WebLogger {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= self.level
        }

        fn log(&self, record: &log::Record) {
            // Console output stays governed by this logger's own fixed `level`, unaffected by
            // Analyst Mode. Capture additionally follows `capture_level` — see that function's
            // doc comment for why `self.enabled(...)` alone isn't enough once Analyst Mode is on.
            if self.enabled(record.metadata()) {
                console_log::log(record);
            }
            if self.enabled(record.metadata()) || record.level() <= capture_level() {
                capture(record);
            }
        }

        fn flush(&self) {}
    }

    /// Install the process-wide logger for the web build: everything `console_log` already did,
    /// plus capture into the buffer this module drains for shipping.
    pub fn install(level: log::Level) {
        let level = level.to_level_filter();
        log::set_max_level(level);
        let _ = log::set_boxed_logger(Box::new(WebLogger { level }));
    }

    /// `?devlog=http://host:8884/ingest` in the page's own URL starts a `spawn_local` loop that
    /// posts buffered entries to that address every few seconds. Absent — the default — nothing
    /// reads the URL for this at all, and no request is ever made.
    pub fn maybe_spawn_shipper() {
        let Some(endpoint) = devlog_endpoint() else {
            return;
        };
        wasm_bindgen_futures::spawn_local(ship_loop(endpoint, "web", "web"));
    }

    fn devlog_endpoint() -> Option<String> {
        let search = web_sys::window()?.location().search().ok()?;
        let query = search.strip_prefix('?').unwrap_or(&search);
        crate::cloud::param(query, "devlog")
    }
}
#[cfg(target_arch = "wasm32")]
pub use web::{install as install_wasm, maybe_spawn_shipper as maybe_spawn_wasm_shipper};

#[cfg(test)]
mod tests {
    use super::*;

    fn push(level: log::Level, target: &str, msg: &str) {
        let args = format_args!("{msg}");
        let record = log::Record::builder()
            .level(level)
            .target(target)
            .args(args)
            .build();
        capture(&record);
    }

    /// One test, not several, on purpose: `BUFFER` is a process-global the whole module shares,
    /// and cargo runs tests in parallel threads by default — splitting this into separate tests
    /// would let one test's pushes land in the middle of another's drain. `drain()` at the top
    /// leaves it clean of whatever ran (or is installed as the real logger) before this test.
    #[test]
    fn capture_and_drain_round_trip_and_the_ring_evicts_the_oldest_once_full() {
        drain();

        push(log::Level::Info, "wxdata::tds", "hello");
        push(log::Level::Warn, "hookecho::app", "world");
        let entries = drain();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].level, "INFO");
        assert_eq!(entries[0].target, "wxdata::tds");
        assert_eq!(entries[0].message, "hello");
        assert_eq!(entries[1].level, "WARN");
        assert_eq!(entries[1].message, "world");

        // A drain is a cursor, not a repeatable snapshot: nothing new pushed means nothing comes
        // back, and a batch with nothing in it should never be shipped.
        assert!(drain().is_empty());
        assert!(take_batch("native", "test-os").is_none());

        // Push more than the ring holds and confirm only the newest `CAPACITY` survive, oldest
        // dropped first — the guard against an unreachable endpoint growing this without bound.
        for i in 0..(CAPACITY + 5) {
            push(log::Level::Debug, "test", &i.to_string());
        }
        let entries = drain();
        assert_eq!(entries.len(), CAPACITY);
        assert_eq!(entries.first().unwrap().message, "5");
        assert_eq!(entries.last().unwrap().message, (CAPACITY + 4).to_string());

        // `recent_warnings`: unlike `drain`, a read that leaves the buffer alone — filtered to
        // warning/error severity, oldest-first, capped at the requested count.
        push(log::Level::Info, "wxdata::tds", "routine");
        push(log::Level::Warn, "hookecho::app", "first warning");
        push(log::Level::Error, "hookecho::app", "then an error");
        push(log::Level::Debug, "wxdata::tds", "noise");
        let warnings = recent_warnings(10);
        assert_eq!(warnings.len(), 2, "info/debug must be filtered out");
        assert_eq!(warnings[0].message, "first warning");
        assert_eq!(warnings[1].message, "then an error");
        assert_eq!(recent_warnings(1).len(), 1, "the limit is respected");
        // A read, not a drain: the shipper's own drain still sees everything just pushed.
        assert_eq!(drain().len(), 4);
    }

    #[test]
    fn instance_id_is_stable_within_the_process() {
        assert_eq!(instance_id(), instance_id());
    }

    /// theme_plan.md §4's Analyst Mode. `CAPTURE_LEVEL`/`ANALYST_MODE_PREV_LEVEL` are process
    /// globals distinct from `BUFFER` (untouched by the test above), but still worth keeping to
    /// one test for the same reason: this also mutates the genuinely global `log::max_level()`,
    /// and nothing else in this codebase reads or sets that (confirmed by grep before writing
    /// this), so one self-contained test that restores it exactly is the safe shape.
    #[test]
    fn analyst_mode_raises_and_restores_the_capture_level() {
        let starting_max = log::max_level();
        assert_eq!(
            capture_level(),
            log::LevelFilter::Off,
            "must start inert, adding nothing beyond each logger's own normal filter"
        );

        set_analyst_mode(true);
        assert_eq!(capture_level(), log::LevelFilter::Debug);
        assert!(
            log::max_level() >= log::LevelFilter::Debug,
            "debug!() call sites must actually be able to construct a Record now"
        );

        // Idempotent while already on: no double-raise, no losing track of the original level.
        set_analyst_mode(true);
        assert_eq!(capture_level(), log::LevelFilter::Debug);

        set_analyst_mode(false);
        assert_eq!(capture_level(), log::LevelFilter::Off);
        assert_eq!(
            log::max_level(),
            starting_max,
            "must restore exactly what was there before, not assume it was always Info"
        );

        // Idempotent while already off, too.
        set_analyst_mode(false);
        assert_eq!(capture_level(), log::LevelFilter::Off);
    }
}
