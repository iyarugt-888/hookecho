//! Live (network): GOES with no signal (ROADMAP_NEW L5). With a store registered, the last
//! hour of GOES-East CONUS band 13 scans is listed and the newest scan read once with the
//! network; then, with this run's listings forgotten as a new start would, the same listing and
//! scan are asked for through a client whose every request fails (a dead proxy). Its own test
//! binary: the store is process-wide.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wxdata::goes_abi::{Satellite, Sector};
use wxdata::objcache::{ObjectStore, Space, StoreFuture};

#[derive(Default)]
struct MemStore(Mutex<HashMap<(String, String), Vec<u8>>>);

impl ObjectStore for MemStore {
    fn get<'a>(&'a self, space: &'a Space, key: &'a str) -> StoreFuture<'a, Option<Vec<u8>>> {
        Box::pin(async move {
            self.0
                .lock()
                .unwrap()
                .get(&(space.slug.to_string(), key.to_string()))
                .cloned()
        })
    }
    fn put<'a>(&'a self, space: &'a Space, key: &'a str, bytes: Vec<u8>) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            self.0
                .lock()
                .unwrap()
                .insert((space.slug.to_string(), key.to_string()), bytes);
        })
    }
}

#[tokio::test]
#[ignore = "network: GOES bucket, then the same reads with every request failing"]
async fn goes_scans_read_before_still_read_with_no_signal() {
    wxdata::objcache::set_store(Arc::new(MemStore::default()));
    let online = reqwest::Client::new();
    let offline = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
        .build()
        .unwrap();
    let (sat, sector, band) = (Satellite::East, Sector::Conus, 13u8);
    let to = chrono::Utc::now();
    let from = to - chrono::Duration::minutes(60);

    let frames = wxdata::goes_abi::scan_frames(&online, sat, sector, band, from, to)
        .await
        .expect("the hour's scans, online");
    let newest = frames.last().expect("a scan in the last hour").start;
    let field = wxdata::goes_abi::fetch_at(&online, sat, sector, band, Some(newest), 600, 350)
        .await
        .expect("the newest scan, online");

    // As a new process would start: no listings in memory, only what the store kept.
    wxdata::goes_abi::forget_listings();
    let frames_again = wxdata::goes_abi::scan_frames(&offline, sat, sector, band, from, to)
        .await
        .expect("the hour's scans, no signal");
    assert_eq!(
        frames_again.iter().map(|f| f.start).collect::<Vec<_>>(),
        frames.iter().map(|f| f.start).collect::<Vec<_>>()
    );
    let again = wxdata::goes_abi::fetch_at(&offline, sat, sector, band, Some(newest), 600, 350)
        .await
        .expect("the newest scan, no signal");
    assert_eq!(again.time, field.time);
    assert_eq!(again.values.len(), field.values.len());
    // A scan never read is not invented: the oldest of the hour was listed but not fetched.
    let oldest = frames.first().unwrap().start;
    if oldest != newest {
        assert!(
            wxdata::goes_abi::fetch_at(&offline, sat, sector, band, Some(oldest), 600, 350)
                .await
                .is_err(),
            "a scan never fetched cannot be read with no signal"
        );
    }
    eprintln!(
        "{} scans listed again and the {} scan read again with no signal",
        frames.len(),
        newest
    );
}
