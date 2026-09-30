//! Network-layer failure injection (ROADMAP_2 §3.2): the feeds pointed at a local server that
//! fails the ways real ones do — a 500, an HTML error page where JSON belongs, an empty body, a
//! body cut off mid-transfer — and at a closed port, standing in for being offline. Every feed
//! must come back with an error or an honestly empty result, quickly, never a panic and never
//! something made up. No network needed: `net::redirect_feeds` sends every request here.
//!
//! One test, run in order: the redirect is process-wide, and the scenarios would trip over each
//! other in parallel.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
enum Fault {
    ServerError = 0,
    HtmlPage = 1,
    EmptyBody = 2,
    CutOff = 3,
}

/// A server on a free local port answering every request with the current fault.
fn serve(mode: Arc<AtomicU8>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            // Read the request head; its contents do not matter.
            let mut buf = [0u8; 8192];
            let mut head = Vec::new();
            while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => head.extend_from_slice(&buf[..n]),
                }
            }
            let reply: Vec<u8> = match mode.load(Ordering::SeqCst) {
                0 => b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 5\r\nConnection: close\r\n\r\nerror".to_vec(),
                1 => {
                    let body = "<html><body>503 Service Unavailable</body></html>";
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .into_bytes()
                }
                2 => b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                // Promises 100 kB and sends the start of a GeoJSON document, then hangs up.
                _ => b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100000\r\nConnection: close\r\n\r\n{\"type\":\"FeatureCollection\",\"features\":[{\"type\":\"Feat".to_vec(),
            };
            let _ = s.write_all(&reply);
            let _ = s.flush();
        }
    });
    port
}

/// A feed's answer: an error, or how many items it claimed to find.
type Answer = Result<usize, String>;

async fn every_feed(http: &reqwest::Client) -> Vec<(&'static str, Answer)> {
    let n = |r: anyhow::Result<usize>| r.map_err(|e| format!("{e:#}"));
    vec![
        (
            "alerts",
            n(wxdata::alerts::fetch_polygon_alerts(http)
                .await
                .map(|v| v.len())),
        ),
        (
            "watches",
            n(wxdata::spc::fetch_watches(http).await.map(|v| v.len())),
        ),
        (
            "SPC outlook",
            n(wxdata::spc::fetch_outlook(http, 1).await.map(|v| v.len())),
        ),
        (
            "ProbSevere",
            n(wxdata::probsevere::fetch_probsevere(http)
                .await
                .map(|v| v.len())),
        ),
        (
            "archived warnings",
            n(
                wxdata::archive_warnings::fetch(http, "2013-05-20T20:12:00Z")
                    .await
                    .map(|v| v.len()),
            ),
        ),
        (
            "storm reports",
            n(wxdata::lsr::fetch(http, None).await.map(|v| v.len())),
        ),
        (
            "METARs",
            n(wxdata::metar::fetch_bbox(http, 34.0, -99.0, 36.0, -96.0)
                .await
                .map(|v| v.len())),
        ),
    ]
}

#[tokio::test(flavor = "multi_thread")]
async fn every_feed_fails_honestly_when_its_server_does() {
    let mode = Arc::new(AtomicU8::new(0));
    let port = serve(mode.clone());
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let mut scenarios: Vec<(String, Option<Fault>)> = [
        Fault::ServerError,
        Fault::HtmlPage,
        Fault::EmptyBody,
        Fault::CutOff,
    ]
    .into_iter()
    .map(|f| (format!("{f:?}"), Some(f)))
    .collect();
    // Offline: nothing listens on the discard port.
    scenarios.push(("Offline".to_string(), None));

    for (name, fault) in scenarios {
        let base = match fault {
            Some(f) => {
                mode.store(f as u8, Ordering::SeqCst);
                format!("http://127.0.0.1:{port}")
            }
            None => "http://127.0.0.1:9".to_string(),
        };
        wxdata::net::redirect_feeds(Some(base));
        let start = Instant::now();
        let answers = every_feed(&http).await;
        let took = start.elapsed();
        for (feed, answer) in &answers {
            eprintln!("{name:<12} {feed:<18} {answer:?}");
            assert!(
                !matches!(answer, Ok(n) if *n > 0),
                "{name}: {feed} made up {answer:?} from a failing server"
            );
        }
        assert!(
            took < Duration::from_secs(20),
            "{name}: the feeds took {took:?} to give up"
        );
        // Every one of these is a failed fetch, and must say so: an empty answer would read as a
        // quiet sky from a healthy feed.
        for (feed, answer) in &answers {
            assert!(
                answer.is_err(),
                "{name}: {feed} said {answer:?}, not an error"
            );
        }
    }
    wxdata::net::redirect_feeds(None);
}

/// The real services still pass the stricter checks: live storm reports and METARs come back as
/// data, not as the error a malformed reply now is.
#[tokio::test]
#[ignore = "network"]
async fn the_real_feeds_still_parse() {
    let http = reqwest::Client::new();
    let reports = wxdata::lsr::fetch(&http, Some(("2013-05-20T19:40Z", "2013-05-20T20:40Z")))
        .await
        .expect("storm reports");
    assert!(!reports.is_empty(), "Moore's hour had reports");
    let obs = wxdata::metar::fetch_bbox(&http, 34.0, -99.0, 36.0, -96.0)
        .await
        .expect("METARs");
    eprintln!("{} reports, {} METARs", reports.len(), obs.len());
}
