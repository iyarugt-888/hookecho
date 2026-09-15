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
            if self.inner.matches(record) {
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
                let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build()
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
            if self.enabled(record.metadata()) {
                console_log::log(record);
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
    }

    #[test]
    fn instance_id_is_stable_within_the_process() {
        assert_eq!(instance_id(), instance_id());
    }
}
