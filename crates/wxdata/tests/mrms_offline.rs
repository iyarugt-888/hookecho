//! Live (network): MRMS with no signal (ROADMAP_NEW L5). With a store registered, the newest
//! mosaic and an archived frame are read once with the network, then again through a client
//! whose every request fails (a dead proxy). The second reads must give the same frames, from
//! the listings and files kept the first time, with their own valid times. Its own test binary:
//! the store is process-wide.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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
#[ignore = "network: MRMS bucket, then the same reads with every request failing"]
async fn mrms_frames_read_before_still_read_with_no_signal() {
    wxdata::objcache::set_store(Arc::new(MemStore::default()));
    let online = reqwest::Client::new();
    let offline = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
        .build()
        .unwrap();
    let product = wxdata::mrms::REFLECTIVITY;

    // Nothing read yet, no signal: nothing to show.
    assert!(wxdata::mrms::fetch_latest_stamped(&offline, product)
        .await
        .is_err());

    let live = wxdata::mrms::fetch_latest_stamped(&online, product)
        .await
        .expect("the newest mosaic, online");
    // As a new process would start: no memo of the newest key, only what the store kept.
    wxdata::mrms::forget_newest_keys();
    let again = wxdata::mrms::fetch_latest_stamped(&offline, product)
        .await
        .expect("the newest mosaic, no signal");
    assert_eq!(again.stamp.valid_time, live.stamp.valid_time);
    assert_eq!(again.data.values.len(), live.data.values.len());

    // An archived frame half an hour back, read once, then with no signal.
    let target = live.stamp.valid_time - chrono::Duration::minutes(30);
    let tol = chrono::Duration::minutes(5);
    let past = wxdata::mrms::fetch_nearest_stamped(&online, product, target, tol)
        .await
        .expect("an archived frame, online");
    let past_again = wxdata::mrms::fetch_nearest_stamped(&offline, product, target, tol)
        .await
        .expect("the same archived frame, no signal");
    assert_eq!(past_again.stamp.valid_time, past.stamp.valid_time);
    // A frame never read is not invented.
    let unseen = wxdata::mrms::fetch_nearest_stamped(
        &offline,
        product,
        target - chrono::Duration::hours(3),
        tol,
    )
    .await;
    assert!(unseen.is_err(), "{:?}", unseen.map(|f| f.stamp.valid_time));
    eprintln!(
        "newest {} and archived {} read again with no signal",
        live.stamp.valid_time, past.stamp.valid_time
    );
}
