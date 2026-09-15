//! `--devlog-serve`: a small standalone admin panel for the developer log — the same `log::`
//! calls every native and web instance already makes (see [`crate::devlog`]), aggregated in one
//! place so a live-sweep stall, a TDS/TVS false trip, or a browser-only error can be filtered and
//! searched without asking whoever hit it to paste a console.
//!
//! Hand-rolled over `std::net::TcpListener`, the same shape as [`crate::serve`] — a handful of
//! local instances posting a few log lines a second do not justify an async HTTP stack. Unlike
//! `serve.rs` this one also has to read a request body (`POST /ingest`), which is the one thing
//! the other server never needed.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

/// How many entries the ring keeps before the oldest are dropped. A dev session generating a
/// sustained few lines a second for hours would still fit; this exists so a runaway `trace!` loop
/// caught by an over-broad `HOOKECHO_DEVLOG`/`?devlog=` can't grow the admin process without
/// bound.
const MAX_ENTRIES: usize = 50_000;
/// A request this large is not a log batch; refused outright rather than read into memory.
const MAX_BODY: usize = 8 * 1024 * 1024;

struct Stored {
    id: u64,
    ts_ms: i64,
    level: String,
    target: String,
    message: String,
    instance: String,
    source: String,
}

struct InstanceMeta {
    source: String,
    version: String,
    os: String,
    first_seen_ms: i64,
    last_seen_ms: i64,
    entries: u64,
}

#[derive(Default)]
struct Store {
    entries: VecDeque<Stored>,
    instances: HashMap<String, InstanceMeta>,
    next_id: u64,
}

impl Store {
    fn ingest(&mut self, batch: crate::devlog::IngestBatch) {
        let now = chrono::Utc::now().timestamp_millis();
        let meta = self
            .instances
            .entry(batch.instance.clone())
            .or_insert_with(|| InstanceMeta {
                source: batch.source.clone(),
                version: batch.version.clone(),
                os: batch.os.clone(),
                first_seen_ms: now,
                last_seen_ms: now,
                entries: 0,
            });
        meta.last_seen_ms = now;
        meta.entries += batch.entries.len() as u64;
        for entry in batch.entries {
            // Ids start at 1, not 0: `since_id=0` (a client's "I've seen nothing yet") has to
            // include the very first entry, which `id > since_id` can't do if that entry's id
            // were also 0.
            self.next_id += 1;
            let id = self.next_id;
            self.entries.push_back(Stored {
                id,
                ts_ms: entry.ts_ms,
                level: entry.level,
                target: entry.target,
                message: entry.message,
                instance: batch.instance.clone(),
                source: batch.source.clone(),
            });
        }
        while self.entries.len() > MAX_ENTRIES {
            self.entries.pop_front();
        }
    }
}

/// Serve until killed. `bind` is an address like `127.0.0.1` or `0.0.0.0` — loopback by default,
/// the same deliberate-act-to-open-up posture as [`crate::serve::run`]. `token`, if non-empty, is
/// the bearer every request must carry (header or `?token=`) — the same all-or-nothing scheme
/// `serve.rs` uses, because this panel can read (and, via `/api/clear`, erase) every log line any
/// instance has ever shipped it, which is worse to leave open on a public deploy than `--serve`'s
/// own read-only status routes.
pub fn run(bind: &str, port: u16, token: String) -> anyhow::Result<()> {
    let listener = TcpListener::bind((bind, port))?;
    let store: Arc<Mutex<Store>> = Arc::new(Mutex::new(Store::default()));
    let token = Arc::new(token);
    log::info!("devlog admin serving on http://{bind}:{port}");
    if bind == "0.0.0.0" {
        if token.is_empty() {
            log::warn!(
                "devlog admin bound to all interfaces with no token — anyone on this network can \
                 read every log line any instance ships here, and clear them"
            );
        } else {
            log::info!("devlog admin bound to all interfaces, bearer token required");
        }
    }
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let store = Arc::clone(&store);
                let token = Arc::clone(&token);
                std::thread::spawn(move || {
                    if let Err(e) = handle(&store, &token, stream) {
                        log::debug!("devlog connection ended: {e}");
                    }
                });
            }
            Err(e) => log::warn!("devlog admin accept failed: {e}"),
        }
    }
    Ok(())
}

fn handle(store: &Mutex<Store>, token: &str, mut stream: TcpStream) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    // "POST /ingest HTTP/1.1"
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let target = parts.next().unwrap_or("/");
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target.to_string(), String::new()),
    };

    let mut bearer: Option<String> = None;
    let mut content_length: usize = 0;
    for _ in 0..64 {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h.trim().is_empty() {
            break;
        }
        let Some((name, value)) = h.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse().unwrap_or(0);
        } else if name.eq_ignore_ascii_case("authorization") {
            if let Some(b) = value.trim().strip_prefix("Bearer ") {
                bearer = Some(b.trim().to_string());
            }
        }
    }
    let authorized = authorize(token, bearer.as_deref(), &query);

    let (status, ctype, body) = if method == "OPTIONS" {
        // The preflight itself never carries the token — it's the browser asking "may I even send
        // this method/these headers", not the real request — so it has to pass regardless of
        // `authorized`, or a token'd server could never be shipped to from another origin at all.
        route(store, &method, &path, &query, &[])
    } else if !authorized {
        (
            "401 Unauthorized",
            "application/json",
            br#"{"error":"missing or bad bearer token"}"#.to_vec(),
        )
    } else if content_length > MAX_BODY {
        (
            "413 Payload Too Large",
            "application/json",
            br#"{"error":"body too large"}"#.to_vec(),
        )
    } else {
        let mut request_body = vec![0u8; content_length];
        reader.read_exact(&mut request_body)?;
        route(store, &method, &path, &query, &request_body)
    };

    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\
         Cache-Control: no-cache\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST\r\n\
         Access-Control-Allow-Headers: Content-Type, Authorization\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&body)?;
    Ok(())
}

/// Whether a request may proceed: no token configured (fully open, the default), or a supplied
/// bearer header or `?token=` query parameter exactly matches it. Split out of `handle` so the
/// decision itself is testable without a socket.
fn authorize(token: &str, bearer: Option<&str>, query: &str) -> bool {
    if token.is_empty() {
        return true;
    }
    if bearer.is_some_and(|b| crate::serve::constant_time_eq(b, token)) {
        return true;
    }
    // The admin page itself has no header to set on a bookmark, and neither does a native/web
    // instance's shipper if an operator would rather bake the token into the one URL it already
    // POSTs to — `?token=` on that same endpoint just works, no extra plumbing.
    crate::cloud::param(query, "token").is_some_and(|t| crate::serve::constant_time_eq(&t, token))
}

fn route(
    store: &Mutex<Store>,
    method: &str,
    path: &str,
    query: &str,
    body: &[u8],
) -> (&'static str, &'static str, Vec<u8>) {
    match (method, path) {
        // The web build ships its logs from whatever origin it's running on — a self-hosted
        // `--serve` on a different port at the very least, a deployed page at worst — so this is
        // always cross-origin from the browser's point of view. A JSON POST is not a "simple
        // request", so the browser sends this preflight before the real one; `handle` already
        // attaches the `Access-Control-Allow-*` headers to every response, this just needs to
        // answer with a successful status for the preflight itself to pass.
        ("OPTIONS", _) => ("204 No Content", "text/plain", Vec::new()),
        ("POST", "/ingest") => ingest(store, body),
        ("POST", "/api/clear") => {
            *store.lock().unwrap() = Store::default();
            ("200 OK", "application/json", b"{\"ok\":true}".to_vec())
        }
        ("GET", "/") => (
            "200 OK",
            "text/html; charset=utf-8",
            ADMIN_HTML.as_bytes().to_vec(),
        ),
        ("GET", "/api/logs") => logs_json(store, query),
        ("GET", "/api/meta") => meta_json(store),
        _ => (
            "404 Not Found",
            "application/json",
            br#"{"error":"no such endpoint"}"#.to_vec(),
        ),
    }
}

fn ingest(store: &Mutex<Store>, body: &[u8]) -> (&'static str, &'static str, Vec<u8>) {
    match serde_json::from_slice::<crate::devlog::IngestBatch>(body) {
        Ok(batch) => {
            let n = batch.entries.len();
            store.lock().unwrap().ingest(batch);
            (
                "200 OK",
                "application/json",
                format!(r#"{{"accepted":{n}}}"#).into_bytes(),
            )
        }
        Err(e) => (
            "400 Bad Request",
            "application/json",
            serde_json::json!({ "error": e.to_string() })
                .to_string()
                .into_bytes(),
        ),
    }
}

/// `ERROR` < `WARN` < `INFO` < `DEBUG` < `TRACE`, most severe first — matching `log::Level`'s own
/// ordering. A `level=` filter means "this severity or worse", the usual meaning in a log viewer.
fn level_rank(level: &str) -> Option<u8> {
    match level.to_ascii_uppercase().as_str() {
        "ERROR" => Some(1),
        "WARN" => Some(2),
        "INFO" => Some(3),
        "DEBUG" => Some(4),
        "TRACE" => Some(5),
        _ => None,
    }
}

fn logs_json(store: &Mutex<Store>, query: &str) -> (&'static str, &'static str, Vec<u8>) {
    let min_level = crate::cloud::param(query, "level").and_then(|l| level_rank(&l));
    let target_prefix = crate::cloud::param(query, "target").filter(|s| !s.is_empty());
    let source = crate::cloud::param(query, "source").filter(|s| !s.is_empty());
    let instance = crate::cloud::param(query, "instance").filter(|s| !s.is_empty());
    let q = crate::cloud::param(query, "q")
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase());
    let since_id: u64 = crate::cloud::param(query, "since_id")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let limit: usize = crate::cloud::param(query, "limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(500)
        .clamp(1, 2000);

    let store = store.lock().unwrap();
    let matches: Vec<&Stored> = store
        .entries
        .iter()
        .filter(|s| s.id > since_id)
        .filter(|s| min_level.is_none_or(|min| level_rank(&s.level).is_some_and(|r| r <= min)))
        .filter(|s| target_prefix.as_deref().is_none_or(|t| s.target.starts_with(t)))
        .filter(|s| source.as_deref().is_none_or(|src| s.source == src))
        .filter(|s| instance.as_deref().is_none_or(|inst| s.instance == inst))
        .filter(|s| {
            q.as_deref()
                .is_none_or(|needle| s.message.to_lowercase().contains(needle))
        })
        .collect();
    // Entries are id-ascending in the ring; keep the newest `limit` of what matched, oldest first,
    // so a live-tailing client can just append what comes back.
    let start = matches.len().saturating_sub(limit);
    let body: Vec<serde_json::Value> = matches[start..]
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "ts_ms": s.ts_ms,
                "level": s.level,
                "target": s.target,
                "message": s.message,
                "instance": s.instance,
                "source": s.source,
            })
        })
        .collect();
    (
        "200 OK",
        "application/json",
        serde_json::to_vec(&body).unwrap_or_default(),
    )
}

fn meta_json(store: &Mutex<Store>) -> (&'static str, &'static str, Vec<u8>) {
    let store = store.lock().unwrap();
    let mut targets: Vec<&str> = store
        .entries
        .iter()
        .map(|s| s.target.as_str())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    targets.sort_unstable();
    let mut instances: Vec<serde_json::Value> = store
        .instances
        .iter()
        .map(|(id, m)| {
            serde_json::json!({
                "id": id,
                "source": m.source,
                "version": m.version,
                "os": m.os,
                "first_seen_ms": m.first_seen_ms,
                "last_seen_ms": m.last_seen_ms,
                "entries": m.entries,
            })
        })
        .collect();
    instances.sort_by(|a, b| {
        b["last_seen_ms"]
            .as_i64()
            .unwrap_or(0)
            .cmp(&a["last_seen_ms"].as_i64().unwrap_or(0))
    });
    let body = serde_json::json!({
        "targets": targets,
        "instances": instances,
        "total_entries": store.entries.len(),
    });
    (
        "200 OK",
        "application/json",
        serde_json::to_vec(&body).unwrap_or_default(),
    )
}

/// The admin page: one file, no framework, no build step, served as a string literal from the
/// binary — same convention as `serve.rs`'s `index()`. Log messages reach the DOM through
/// `textContent` only, never `innerHTML`: a message can be arbitrary text (a feed's own error
/// string, a URL, anything a call site formatted in), and this page is the one place all of it is
/// concentrated in one view.
const ADMIN_HTML: &str = r##"<!doctype html><html lang="en"><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>HookEcho — Developer Log</title>
<link rel="icon" href="data:,">
<style>
:root { color-scheme: dark; }
* { box-sizing: border-box; }
body { margin:0; font:14px/1.5 ui-monospace,SFMono-Regular,Consolas,monospace; background:#0e1116;
  color:#e6edf3; display:flex; flex-direction:column; height:100vh; }
header { padding:10px 14px; border-bottom:1px solid #21262d; display:flex; flex-wrap:wrap;
  gap:8px 12px; align-items:center; font-family:system-ui,sans-serif; }
h1 { font-size:14px; margin:0; font-weight:600; white-space:nowrap; }
select, input[type=text] { background:#161b22; color:#e6edf3; border:1px solid #30363d;
  border-radius:6px; padding:5px 8px; font:13px system-ui,sans-serif; }
input[type=text] { min-width:160px; }
label { display:flex; align-items:center; gap:5px; font:13px system-ui,sans-serif; color:#9aa7b4; }
button { background:#21262d; color:#e6edf3; border:1px solid #30363d; border-radius:6px;
  padding:5px 10px; font:13px system-ui,sans-serif; cursor:pointer; }
button:hover { background:#30363d; }
#status { margin-left:auto; color:#6e7681; font:12px system-ui,sans-serif; white-space:nowrap; }
#scroll { flex:1; overflow:auto; }
table { width:100%; border-collapse:collapse; }
td, th { padding:3px 10px; text-align:left; vertical-align:top; white-space:pre-wrap;
  word-break:break-word; border-bottom:1px solid #161b22; }
th { position:sticky; top:0; background:#0e1116; color:#6e7681; font-weight:600;
  font-family:system-ui,sans-serif; font-size:11px; text-transform:uppercase; letter-spacing:.04em; }
td.time, td.level, td.target, td.who { white-space:nowrap; }
td.time { color:#6e7681; }
td.target { color:#8b949e; }
td.who { color:#8b949e; }
tr.lv-error td.level { color:#f85149; font-weight:600; }
tr.lv-warn td.level { color:#d29922; font-weight:600; }
tr.lv-info td.level { color:#58a6ff; }
tr.lv-debug td.level { color:#8b949e; }
tr.lv-trace td.level { color:#484f58; }
tr.lv-error { background:#3d1d1d22; }
tr:hover { background:#161b22; }
#empty { padding:24px; color:#6e7681; font-family:system-ui,sans-serif; }
</style>
<header>
  <h1>Developer Log</h1>
  <select id="level">
    <option value="">all levels</option>
    <option value="ERROR">error+</option>
    <option value="WARN">warn+</option>
    <option value="INFO">info+</option>
    <option value="DEBUG">debug+</option>
    <option value="TRACE">trace+</option>
  </select>
  <input id="target" type="text" list="targets" placeholder="category (e.g. wxdata::tds)">
  <datalist id="targets"></datalist>
  <select id="source">
    <option value="">all sources</option>
    <option value="native">native</option>
    <option value="web">web</option>
  </select>
  <select id="instance"><option value="">all instances</option></select>
  <input id="q" type="text" placeholder="search message…">
  <label><input id="live" type="checkbox" checked> live tail</label>
  <button id="clear">Clear</button>
  <span id="status">—</span>
</header>
<div id="scroll">
  <table>
    <thead><tr><th>Time</th><th>Level</th><th>Category</th><th>Instance</th><th>Message</th></tr></thead>
    <tbody id="rows"></tbody>
  </table>
  <div id="empty" hidden>No log entries match these filters yet.</div>
</div>
<script>
const $ = (id) => document.getElementById(id);
const scroll = $("scroll");
// A token'd server is reached with `?token=`, same as `--serve`'s own dashboard; every fetch this
// page makes has to carry it too, since there's no header to set on a bookmark.
const token = new URLSearchParams(location.search).get("token");
function authed(path) {
  if (!token) return path;
  const [base, qs] = path.split("?");
  const params = new URLSearchParams(qs || "");
  params.set("token", token);
  return base + "?" + params.toString();
}
let sinceId = 0;
let rowCount = 0;
const MAX_ROWS = 5000; // client-side cap so a long session doesn't grow the DOM without bound
// Bumped by every resetView() and captured by pollLogs() before it awaits its fetch. Two filter
// changes close together start two overlapping requests; without this, whichever one's *response*
// lands last wins even if it was the *first* one sent — reappending rows a later reset already
// cleared, or duplicating ones the later request also fetched. A response checks this on the way
// back in and is dropped if a newer poll has since started.
let generation = 0;

function resetView() {
  generation++;
  sinceId = 0;
  rowCount = 0;
  $("rows").replaceChildren();
  $("empty").hidden = true;
}

function debounce(fn, ms) {
  let t;
  return (...a) => { clearTimeout(t); t = setTimeout(() => fn(...a), ms); };
}

function currentParams() {
  const p = new URLSearchParams();
  const level = $("level").value;
  const target = $("target").value.trim();
  const source = $("source").value;
  const instance = $("instance").value;
  const q = $("q").value.trim();
  if (level) p.set("level", level);
  if (target) p.set("target", target);
  if (source) p.set("source", source);
  if (instance) p.set("instance", instance);
  if (q) p.set("q", q);
  return p;
}

function addRow(it) {
  const tr = document.createElement("tr");
  tr.className = "lv-" + it.level.toLowerCase();
  const time = document.createElement("td");
  time.className = "time";
  time.textContent = new Date(it.ts_ms).toLocaleTimeString();
  const level = document.createElement("td");
  level.className = "level";
  level.textContent = it.level;
  const target = document.createElement("td");
  target.className = "target";
  target.textContent = it.target;
  const who = document.createElement("td");
  who.className = "who";
  who.textContent = it.source + " · " + it.instance.slice(0, 10);
  const msg = document.createElement("td");
  msg.textContent = it.message;
  tr.append(time, level, target, who, msg);
  $("rows").append(tr);
  rowCount++;
  while (rowCount > MAX_ROWS) {
    const first = $("rows").firstElementChild;
    if (!first) break;
    first.remove();
    rowCount--;
  }
}

async function pollLogs() {
  const myGeneration = generation;
  const p = currentParams();
  const live = $("live").checked;
  p.set("since_id", live ? String(sinceId) : "0");
  p.set("limit", "1000");
  try {
    const r = await fetch(authed("/api/logs?" + p.toString()));
    if (!r.ok) throw new Error("HTTP " + r.status);
    const items = await r.json();
    if (myGeneration !== generation) return; // a newer filter/reset has since started
    if (!live) resetView();
    const nearBottom = scroll.scrollHeight - scroll.scrollTop - scroll.clientHeight < 80;
    for (const it of items) {
      sinceId = Math.max(sinceId, it.id);
      addRow(it);
    }
    $("empty").hidden = rowCount > 0;
    if (items.length && live && nearBottom) {
      scroll.scrollTop = scroll.scrollHeight;
    }
    $("status").textContent = rowCount + " lines shown · " + new Date().toLocaleTimeString();
  } catch (e) {
    $("status").textContent = "offline (" + e.message + ")";
  }
}

async function pollMeta() {
  try {
    const r = await fetch(authed("/api/meta"));
    if (!r.ok) return;
    const meta = await r.json();
    const targets = $("targets");
    targets.replaceChildren(...meta.targets.map((t) => {
      const o = document.createElement("option");
      o.value = t;
      return o;
    }));
    const instSel = $("instance");
    const current = instSel.value;
    instSel.replaceChildren();
    const allOpt = document.createElement("option");
    allOpt.value = "";
    allOpt.textContent = "all instances";
    instSel.append(allOpt);
    for (const inst of meta.instances) {
      const o = document.createElement("option");
      o.value = inst.id;
      o.textContent = inst.source + " " + inst.version + " (" + inst.os + ") · " +
        inst.entries + " lines";
      instSel.append(o);
    }
    instSel.value = current;
  } catch (e) { /* a failed meta poll just leaves the dropdowns as they were */ }
}

for (const id of ["level", "source", "instance"]) {
  $(id).addEventListener("change", () => { resetView(); pollLogs(); });
}
$("target").addEventListener("input", debounce(() => { resetView(); pollLogs(); }, 300));
$("q").addEventListener("input", debounce(() => { resetView(); pollLogs(); }, 300));
$("live").addEventListener("change", () => { resetView(); pollLogs(); });
$("clear").addEventListener("click", async () => {
  if (!confirm("Clear every buffered log entry on this admin server? This can't be undone."))
    return;
  await fetch(authed("/api/clear"), { method: "POST" });
  resetView();
  pollLogs();
});

pollMeta();
pollLogs();
// Off, a filter change still fetches once (its own handler calls pollLogs directly) but the view
// otherwise stays put — there is no "current tail" to keep polling for.
setInterval(() => { if ($("live").checked) pollLogs(); }, 1500);
setInterval(pollMeta, 5000);
</script>
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devlog::{IngestBatch, LogEntry};

    fn entry(level: &str, target: &str, message: &str) -> LogEntry {
        LogEntry {
            ts_ms: 0,
            level: level.to_string(),
            target: target.to_string(),
            message: message.to_string(),
        }
    }

    fn batch(instance: &str, source: &str, entries: Vec<LogEntry>) -> IngestBatch {
        IngestBatch {
            instance: instance.to_string(),
            source: source.to_string(),
            version: "0.0.0-test".to_string(),
            os: "test".to_string(),
            entries,
        }
    }

    /// The whole point of `level=` is "this severe or worse", so the ranking has to put ERROR
    /// ahead of WARN ahead of INFO — the opposite of `log::Level`'s "how much detail" reading,
    /// which is easy to get backwards and would silently invert every severity filter in the UI.
    #[test]
    fn level_rank_orders_error_as_more_severe_than_trace() {
        assert!(level_rank("ERROR") < level_rank("WARN"));
        assert!(level_rank("WARN") < level_rank("INFO"));
        assert!(level_rank("INFO") < level_rank("DEBUG"));
        assert!(level_rank("DEBUG") < level_rank("TRACE"));
        assert_eq!(level_rank("error"), level_rank("ERROR")); // case-insensitive
        assert_eq!(level_rank("bogus"), None);
    }

    fn logs(store: &Mutex<Store>, query: &str) -> Vec<serde_json::Value> {
        let (status, _, body) = logs_json(store, query);
        assert_eq!(status, "200 OK");
        serde_json::from_slice(&body).unwrap()
    }

    #[test]
    fn ingest_then_query_round_trips_the_message_and_assigns_ids_in_order() {
        let store = Mutex::new(Store::default());
        store.lock().unwrap().ingest(batch(
            "inst-a",
            "native",
            vec![
                entry("INFO", "wxdata::tds", "first"),
                entry("WARN", "wxdata::rotation", "second"),
            ],
        ));
        let items = logs(&store, "");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["id"], 1);
        assert_eq!(items[0]["message"], "first");
        assert_eq!(items[1]["id"], 2);
        assert_eq!(items[1]["message"], "second");
    }

    #[test]
    fn level_filter_keeps_the_selected_severity_and_everything_worse() {
        let store = Mutex::new(Store::default());
        store.lock().unwrap().ingest(batch(
            "inst-a",
            "native",
            vec![
                entry("ERROR", "hookecho::app", "boom"),
                entry("WARN", "hookecho::app", "hmm"),
                entry("DEBUG", "hookecho::app", "chatter"),
            ],
        ));
        let items = logs(&store, "level=WARN");
        let messages: Vec<_> = items.iter().map(|v| v["message"].as_str().unwrap()).collect();
        assert_eq!(messages, ["boom", "hmm"]); // DEBUG is less severe than WARN, excluded
    }

    #[test]
    fn target_filter_is_a_prefix_match_so_a_whole_module_family_can_be_selected() {
        let store = Mutex::new(Store::default());
        store.lock().unwrap().ingest(batch(
            "inst-a",
            "native",
            vec![
                entry("INFO", "wxdata::tds", "debris"),
                entry("INFO", "wxdata::rotation", "couplet"),
                entry("INFO", "wxdata::tds::sub", "nested"),
            ],
        ));
        let items = logs(&store, "target=wxdata%3A%3Atds");
        assert_eq!(items.len(), 2); // "wxdata::tds" and "wxdata::tds::sub", not "wxdata::rotation"
    }

    #[test]
    fn search_matches_the_message_case_insensitively() {
        let store = Mutex::new(Store::default());
        store.lock().unwrap().ingest(batch(
            "inst-a",
            "native",
            vec![
                entry("INFO", "hookecho::app", "Live sweep loaded for KTLX"),
                entry("INFO", "hookecho::app", "unrelated line"),
            ],
        ));
        let items = logs(&store, "q=ktlx");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["message"], "Live sweep loaded for KTLX");
    }

    #[test]
    fn source_and_instance_filters_isolate_one_launch_from_another() {
        let store = Mutex::new(Store::default());
        {
            let mut s = store.lock().unwrap();
            s.ingest(batch("native-1", "native", vec![entry("INFO", "a", "from native")]));
            s.ingest(batch("web-1", "web", vec![entry("INFO", "a", "from web")]));
        }
        assert_eq!(logs(&store, "source=web").len(), 1);
        assert_eq!(logs(&store, "source=web")[0]["message"], "from web");
        assert_eq!(logs(&store, "instance=native-1").len(), 1);
        assert_eq!(logs(&store, "instance=native-1")[0]["message"], "from native");
    }

    #[test]
    fn since_id_returns_only_what_landed_after_the_cursor() {
        let store = Mutex::new(Store::default());
        store.lock().unwrap().ingest(batch(
            "inst-a",
            "native",
            vec![entry("INFO", "a", "one"), entry("INFO", "a", "two")],
        ));
        // A live-tailing client's next poll passes back the highest id it has already shown.
        let first_id = logs(&store, "")[0]["id"].as_u64().unwrap();
        store
            .lock()
            .unwrap()
            .ingest(batch("inst-a", "native", vec![entry("INFO", "a", "three")]));
        let items = logs(&store, &format!("since_id={first_id}"));
        let messages: Vec<_> = items.iter().map(|v| v["message"].as_str().unwrap()).collect();
        assert_eq!(messages, ["two", "three"]);
    }

    #[test]
    fn ingest_tracks_instance_metadata_across_batches_from_the_same_instance() {
        let store = Mutex::new(Store::default());
        {
            let mut s = store.lock().unwrap();
            s.ingest(batch("inst-a", "native", vec![entry("INFO", "a", "one")]));
            s.ingest(batch("inst-a", "native", vec![entry("INFO", "a", "two"), entry("INFO", "a", "three")]));
        }
        let (status, _, body) = meta_json(&store);
        assert_eq!(status, "200 OK");
        let meta: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let instances = meta["instances"].as_array().unwrap();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0]["id"], "inst-a");
        assert_eq!(instances[0]["entries"], 3); // one batch of 1 plus one batch of 2
    }

    /// A store over its cap drops the oldest entries rather than growing forever — the one thing
    /// standing between an admin panel left running for days and an out-of-memory process.
    #[test]
    fn the_ring_drops_the_oldest_entries_once_it_is_over_capacity() {
        let mut store = Store::default();
        // Ingest one over the cap; cheap to do one at a time since MAX_ENTRIES is a constant, not
        // a config the test has to plumb through.
        for i in 0..(MAX_ENTRIES + 1) {
            store.ingest(batch("inst-a", "native", vec![entry("INFO", "a", &i.to_string())]));
        }
        assert_eq!(store.entries.len(), MAX_ENTRIES);
        // The very first entry (id 0, message "0") should have been evicted; the newest survives.
        assert_eq!(store.entries.front().unwrap().message, "1");
        assert_eq!(store.entries.back().unwrap().message, MAX_ENTRIES.to_string());
    }

    /// The web build ships its logs from a different origin than this admin panel almost by
    /// definition (a self-hosted `--serve` on another port at best), so a JSON POST to `/ingest`
    /// always draws a CORS preflight first. A 404 here — the fallthrough for every path this
    /// server doesn't otherwise know, which `OPTIONS` would hit without its own arm — fails that
    /// preflight even with the right `Access-Control-Allow-*` headers, since the fetch spec
    /// requires a successful status; this is what stands between "works from the same machine"
    /// and "silently never ships a line the browser build ever logs".
    #[test]
    fn options_preflight_succeeds_for_any_path() {
        let store = Mutex::new(Store::default());
        let (status, _, body) = route(&store, "OPTIONS", "/ingest", "", &[]);
        assert_eq!(status, "204 No Content");
        assert!(body.is_empty());
    }

    #[test]
    fn no_token_configured_means_every_request_is_authorized() {
        assert!(authorize("", None, ""));
        assert!(authorize("", Some("literally anything"), ""));
    }

    #[test]
    fn a_configured_token_refuses_a_missing_or_wrong_credential() {
        assert!(!authorize("secret", None, ""));
        assert!(!authorize("secret", Some("wrong"), ""));
        assert!(!authorize("secret", None, "token=wrong"));
    }

    #[test]
    fn a_configured_token_accepts_either_the_bearer_header_or_the_query_parameter() {
        assert!(authorize("secret", Some("secret"), ""));
        assert!(authorize("secret", None, "token=secret"));
        // Whichever the caller could actually set — a native shipper's header, a bookmarked
        // admin-page URL's query string — either alone has to be enough.
        assert!(authorize("secret", Some("wrong"), "token=secret"));
    }
}
