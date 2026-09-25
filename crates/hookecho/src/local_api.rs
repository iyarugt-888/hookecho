//! The running app's local API (ROADMAP_NEW M4): what the app is showing right now, over HTTP on
//! the loopback interface, for a stream overlay, a Stream Deck button, a home-automation rule or a
//! script. Off unless turned on in settings, and only ever bound to 127.0.0.1.
//!
//! `--serve` answers the same kinds of question for a machine with no app running; this answers
//! them about *this* session: which radars the panes show, which volume and tilt, what the
//! detectors found on it, which warnings are up, how the feeds are doing, and what the displayed
//! volume reads at a point.
//!
//! Endpoints (all `GET`, JSON unless noted):
//!
//! | path | what |
//! |---|---|
//! | `/api/v1` | this list |
//! | `/api/v1/state` | panes: site, product, tilt, volume and its time, camera, live or not |
//! | `/api/v1/detections` | debris signatures and couplets on the active pane's volume |
//! | `/api/v1/warnings` | the warnings shown on the map |
//! | `/api/v1/health` | each data source's health |
//! | `/api/v1/products` | the active volume's moments and tilts, and the field layers on |
//! | `/api/v1/frames` | the active pane's volume list, newest last |
//! | `/api/v1/sample?lat=&lon=` | every moment of the displayed tilt at a point |
//! | `/api/v1/snapshot.png` | the window as a PNG |
//! | `/api/v1/events` | Server-Sent Events: `state` whenever the displayed volume changes |
//!
//! Two guards, because anything on this machine — including a web page in a browser — can reach
//! 127.0.0.1: requests must name this server in their `Host` header (a DNS-rebinding page names
//! its own host), and no CORS header is ever sent, so another origin's script cannot read an
//! answer. The server holds no app state of its own: the app publishes a [`Snapshot`] each second,
//! and the two questions only the app can answer (a sample, a screenshot) are forwarded to it as
//! [`Request`]s and waited on with a timeout.
//
// ponytail: thread-per-connection over std::net, like `crate::serve`; a handful of local pollers.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// What the app last published, each part already serialised (single-line JSON).
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub state: String,
    pub detections: String,
    pub warnings: String,
    pub health: String,
    pub products: String,
    pub frames: String,
}

/// A question only the app can answer, with where to send the answer.
pub enum Request {
    /// Every moment of the active pane's displayed tilt at a point, as JSON.
    Sample {
        lat: f64,
        lon: f64,
        reply: mpsc::Sender<String>,
    },
    /// The window as a PNG, or why not.
    Snapshot {
        reply: mpsc::Sender<Result<Vec<u8>, String>>,
    },
}

struct Shared {
    snapshot: Mutex<Snapshot>,
    /// Bumped when the displayed volume changes, which is what an event stream reports.
    version: AtomicU64,
    stop: AtomicBool,
    /// Wake the app so a forwarded request is seen now rather than at its next repaint.
    wake: Box<dyn Fn() + Send + Sync>,
    port: u16,
}

/// A running server. Dropping it stops the listener; open event streams end within a second.
pub struct Handle {
    shared: Arc<Shared>,
    /// Forwarded requests for the app to answer (see [`Request`]).
    pub requests: mpsc::Receiver<Request>,
    pub port: u16,
}

impl Handle {
    /// Replace the published snapshot; `changed` bumps the event-stream version.
    pub fn publish(&self, snapshot: Snapshot, changed: bool) {
        if let Ok(mut s) = self.shared.snapshot.lock() {
            *s = snapshot;
        }
        if changed {
            self.shared.version.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
    }
}

/// Start listening on `127.0.0.1:port` (0 picks a free port). `wake` is called whenever a request
/// needs the app's attention.
pub fn start(port: u16, wake: Box<dyn Fn() + Send + Sync>) -> std::io::Result<Handle> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;
    let (tx, rx) = mpsc::channel();
    let shared = Arc::new(Shared {
        snapshot: Mutex::new(Snapshot::default()),
        version: AtomicU64::new(0),
        stop: AtomicBool::new(false),
        wake,
        port,
    });
    let accept = Arc::clone(&shared);
    std::thread::Builder::new()
        .name("local-api".into())
        .spawn(move || {
            while !accept.stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (shared, tx) = (Arc::clone(&accept), tx.clone());
                        let _ = std::thread::Builder::new()
                            .name("local-api-conn".into())
                            .spawn(move || {
                                let _ = stream.set_nonblocking(false);
                                handle(stream, &shared, &tx);
                            });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(200)),
                }
            }
        })?;
    Ok(Handle {
        shared,
        requests: rx,
        port,
    })
}

/// The endpoint list `/api/v1` answers with.
fn index() -> String {
    serde_json::json!({
        "api": "hookecho-local",
        "version": 1,
        "endpoints": [
            "/api/v1/state", "/api/v1/detections", "/api/v1/warnings", "/api/v1/health",
            "/api/v1/products", "/api/v1/frames", "/api/v1/sample?lat=&lon=",
            "/api/v1/snapshot.png", "/api/v1/events",
        ],
    })
    .to_string()
}

fn respond(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

fn error(stream: &mut TcpStream, status: &str, message: &str) {
    let body = serde_json::json!({ "error": message }).to_string();
    respond(stream, status, "application/json", body.as_bytes());
}

/// Whether `host` (the request's `Host` header) names this server.
fn host_ok(host: Option<&str>, port: u16) -> bool {
    let Some(host) = host else {
        return false;
    };
    let host = host.trim().to_ascii_lowercase();
    [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ]
    .contains(&host)
}

fn handle(mut stream: TcpStream, shared: &Shared, tx: &mpsc::Sender<Request>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok(reader_stream) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(reader_stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let mut host = None;
    let mut header_bytes = 0usize;
    loop {
        let mut h = String::new();
        match reader.read_line(&mut h) {
            Ok(0) | Err(_) => break,
            Ok(n) => header_bytes += n,
        }
        if h == "\r\n" || h == "\n" || header_bytes > 16 * 1024 {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.trim().eq_ignore_ascii_case("host") {
                host = Some(v.trim().to_string());
            }
        }
    }
    if !host_ok(host.as_deref(), shared.port) {
        return error(
            &mut stream,
            "403 Forbidden",
            "requests must be addressed to 127.0.0.1",
        );
    }
    if method != "GET" {
        return error(&mut stream, "405 Method Not Allowed", "GET only");
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let snap = |f: fn(&Snapshot) -> &String| {
        shared
            .snapshot
            .lock()
            .map(|s| f(&s).clone())
            .unwrap_or_default()
    };
    let json = |stream: &mut TcpStream, body: String| {
        if body.is_empty() {
            error(
                stream,
                "503 Service Unavailable",
                "the app has not published yet",
            );
        } else {
            respond(stream, "200 OK", "application/json", body.as_bytes());
        }
    };
    match path {
        "/api/v1" | "/api/v1/" => json(&mut stream, index()),
        "/api/v1/state" => json(&mut stream, snap(|s| &s.state)),
        "/api/v1/detections" => json(&mut stream, snap(|s| &s.detections)),
        "/api/v1/warnings" => json(&mut stream, snap(|s| &s.warnings)),
        "/api/v1/health" => json(&mut stream, snap(|s| &s.health)),
        "/api/v1/products" => json(&mut stream, snap(|s| &s.products)),
        "/api/v1/frames" => json(&mut stream, snap(|s| &s.frames)),
        "/api/v1/sample" => {
            let num = |k| crate::cloud::param(query, k).and_then(|v| v.parse::<f64>().ok());
            let (Some(lat), Some(lon)) = (num("lat"), num("lon")) else {
                return error(&mut stream, "400 Bad Request", "lat and lon are required");
            };
            if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
                return error(&mut stream, "400 Bad Request", "lat/lon out of range");
            }
            let (reply, answer) = mpsc::channel();
            if tx.send(Request::Sample { lat, lon, reply }).is_err() {
                return error(&mut stream, "503 Service Unavailable", "the app is closing");
            }
            (shared.wake)();
            match answer.recv_timeout(Duration::from_secs(3)) {
                Ok(body) => json(&mut stream, body),
                Err(_) => error(&mut stream, "504 Gateway Timeout", "the app did not answer"),
            }
        }
        "/api/v1/snapshot.png" => {
            let (reply, answer) = mpsc::channel();
            if tx.send(Request::Snapshot { reply }).is_err() {
                return error(&mut stream, "503 Service Unavailable", "the app is closing");
            }
            (shared.wake)();
            match answer.recv_timeout(Duration::from_secs(8)) {
                Ok(Ok(png)) => respond(&mut stream, "200 OK", "image/png", &png),
                Ok(Err(e)) => error(&mut stream, "500 Internal Server Error", &e),
                Err(_) => error(&mut stream, "504 Gateway Timeout", "the app did not answer"),
            }
        }
        "/api/v1/events" => events(stream, shared),
        _ => error(
            &mut stream,
            "404 Not Found",
            "no such endpoint; see /api/v1",
        ),
    }
}

/// Server-Sent Events: the state now, then again whenever the displayed volume changes, with a
/// comment every 15 s so a proxy or client does not time the stream out. Ends when the client
/// goes away or the server stops.
fn events(mut stream: TcpStream, shared: &Shared) {
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                Cache-Control: no-store\r\nConnection: keep-alive\r\n\r\n";
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    let mut seen = u64::MAX;
    let mut last_write = Instant::now();
    while !shared.stop.load(Ordering::SeqCst) {
        let version = shared.version.load(Ordering::SeqCst);
        let result = if version != seen {
            seen = version;
            let state = shared
                .snapshot
                .lock()
                .map(|s| s.state.clone())
                .unwrap_or_default();
            last_write = Instant::now();
            stream.write_all(format!("event: state\ndata: {state}\n\n").as_bytes())
        } else if last_write.elapsed() > Duration::from_secs(15) {
            last_write = Instant::now();
            stream.write_all(b": keep-alive\n\n")
        } else {
            Ok(())
        };
        if result.and_then(|()| stream.flush()).is_err() {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn get(port: u16, path: &str, host: Option<&str>) -> (String, String) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let host = host.map_or(String::new(), |h| format!("Host: {h}\r\n"));
        write!(s, "GET {path} HTTP/1.1\r\n{host}\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        let (head, body) = out.split_once("\r\n\r\n").unwrap();
        (head.lines().next().unwrap().to_string(), body.to_string())
    }

    fn local(port: u16) -> String {
        format!("127.0.0.1:{port}")
    }

    #[test]
    fn serves_the_published_snapshot_and_refuses_foreign_hosts() {
        let h = start(0, Box::new(|| {})).unwrap();
        let host = local(h.port);
        let (status, _) = get(h.port, "/api/v1/state", Some(&host));
        assert!(status.contains("503"), "{status}: nothing published yet");
        h.publish(
            Snapshot {
                state: r#"{"panes":[]}"#.into(),
                ..Default::default()
            },
            true,
        );
        let (status, body) = get(h.port, "/api/v1/state", Some(&host));
        assert!(status.contains("200"), "{status}");
        assert_eq!(body, r#"{"panes":[]}"#);
        let (_, body) = get(h.port, "/api/v1", Some(&format!("localhost:{}", h.port)));
        assert!(body.contains("/api/v1/sample"), "{body}");
        // A DNS-rebinding page names its own host; no Host at all is refused too.
        assert!(get(h.port, "/api/v1/state", Some("evil.example:80"))
            .0
            .contains("403"));
        assert!(get(h.port, "/api/v1/state", None).0.contains("403"));
        assert!(get(h.port, "/nope", Some(&host)).0.contains("404"));
        // No CORS header, ever.
        let mut s = TcpStream::connect(("127.0.0.1", h.port)).unwrap();
        write!(
            s,
            "GET /api/v1 HTTP/1.1\r\nHost: {host}\r\nOrigin: http://x\r\n\r\n"
        )
        .unwrap();
        let mut all = String::new();
        s.read_to_string(&mut all).unwrap();
        assert!(
            !all.to_ascii_lowercase().contains("access-control"),
            "{all}"
        );
    }

    #[test]
    fn a_sample_is_forwarded_to_the_app_and_its_answer_returned() {
        let h = start(0, Box::new(|| {})).unwrap();
        let port = h.port;
        let host = local(port);
        let client =
            std::thread::spawn(move || get(port, "/api/v1/sample?lat=35.3&lon=-97.5", Some(&host)));
        // The app's side: answer the forwarded request.
        match h.requests.recv_timeout(Duration::from_secs(3)).unwrap() {
            Request::Sample { lat, lon, reply } => {
                assert_eq!((lat, lon), (35.3, -97.5));
                reply.send(r#"{"REF":42.5}"#.into()).unwrap();
            }
            Request::Snapshot { .. } => panic!("expected a sample"),
        }
        let (status, body) = client.join().unwrap();
        assert!(status.contains("200"), "{status}");
        assert_eq!(body, r#"{"REF":42.5}"#);
        let host = local(port);
        assert!(get(port, "/api/v1/sample?lat=95&lon=0", Some(&host))
            .0
            .contains("400"));
        assert!(get(port, "/api/v1/sample", Some(&host)).0.contains("400"));
    }

    #[test]
    fn events_stream_the_state_and_again_when_it_changes() {
        let h = start(0, Box::new(|| {})).unwrap();
        h.publish(
            Snapshot {
                state: r#"{"v":1}"#.into(),
                ..Default::default()
            },
            true,
        );
        let mut s = TcpStream::connect(("127.0.0.1", h.port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        write!(
            s,
            "GET /api/v1/events HTTP/1.1\r\nHost: {}\r\n\r\n",
            local(h.port)
        )
        .unwrap();
        let mut reader = BufReader::new(s);
        let mut seen = String::new();
        let read_until = |reader: &mut BufReader<TcpStream>, seen: &mut String, want: &str| {
            while !seen.contains(want) {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                seen.push_str(&line);
            }
        };
        read_until(&mut reader, &mut seen, "data: {\"v\":1}");
        assert!(seen.contains("text/event-stream") && seen.contains("event: state"));
        h.publish(
            Snapshot {
                state: r#"{"v":2}"#.into(),
                ..Default::default()
            },
            true,
        );
        read_until(&mut reader, &mut seen, "data: {\"v\":2}");
        assert!(seen.contains("data: {\"v\":2}"), "{seen}");
    }

    #[test]
    fn the_host_check_takes_only_this_port_on_loopback_names() {
        assert!(host_ok(Some("127.0.0.1:47914"), 47_914));
        assert!(host_ok(Some("LOCALHOST:47914"), 47_914));
        assert!(host_ok(Some("[::1]:47914"), 47_914));
        assert!(!host_ok(Some("127.0.0.1:80"), 47_914));
        assert!(!host_ok(Some("127.0.0.1"), 47_914));
        assert!(!host_ok(Some("attacker.test:47914"), 47_914));
        assert!(!host_ok(None, 47_914));
    }
}
