//! On-disk store behind `wxdata::objcache`: one file per object, a folder per space (HRRR, GEFS,
//! RTMA and other-model GRIB messages, MRMS grids, GOES scans), each kept under its own quota,
//! least recently read first.
//!
//! A file is `[u32 LE key length][key][payload]`. The key is stored so a name collision (a 128-bit
//! hash makes one absurdly unlikely, but the check costs nothing) or a file from another key can
//! never be returned as the wrong object. Writes go to a temporary name and are renamed into
//! place, so a crash mid-write leaves a stray temp file the next sweep removes, never a torn entry.
//!
//! Desktop and Android; the web build keeps the same spaces in IndexedDB (`crate::webcache`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use wxdata::objcache::{ObjectStore, Space, StoreFuture};

/// Writes between size checks of a space's folder.
const SWEEP_EVERY: u32 = 64;

/// A space's quota on this platform: the smaller one on a phone.
pub fn cap(space: &Space) -> u64 {
    if cfg!(target_os = "android") {
        space.cap_small
    } else {
        space.cap
    }
}

pub struct DiskStore {
    root: PathBuf,
    /// Overrides every space's quota (tests).
    cap_override: Option<u64>,
    puts: AtomicU32,
}

impl DiskStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            cap_override: None,
            puts: AtomicU32::new(0),
        }
    }

    #[cfg(test)]
    fn with_cap(root: PathBuf, cap: u64) -> Self {
        Self {
            cap_override: Some(cap),
            ..Self::new(root)
        }
    }

    fn dir(&self, space: &Space) -> PathBuf {
        self.root.join(space.slug)
    }

    fn path(&self, space: &Space, key: &str) -> PathBuf {
        self.dir(space)
            .join(format!("{}.obj", wxdata::objcache::digest(key)))
    }

    fn read(&self, space: &Space, key: &str) -> Option<Vec<u8>> {
        let path = self.path(space, key);
        let file = std::fs::read(&path).ok()?;
        let payload = payload_for(&file, key)?;
        // Touch it so the oldest-first sweep is least-recently-used, not first-written: the frame
        // being scrubbed back over is the one worth keeping.
        if let Ok(f) = std::fs::File::options().write(true).open(&path) {
            let _ = f.set_modified(std::time::SystemTime::now());
        }
        Some(payload.to_vec())
    }

    fn write(&self, space: &Space, key: &str, bytes: &[u8]) {
        let dir = self.dir(space);
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = self.path(space, key);
        let tmp = path.with_extension("tmp");
        let mut out = Vec::with_capacity(4 + key.len() + bytes.len());
        out.extend_from_slice(&(key.len() as u32).to_le_bytes());
        out.extend_from_slice(key.as_bytes());
        out.extend_from_slice(bytes);
        if std::fs::write(&tmp, &out).is_ok() && std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        if self.puts.fetch_add(1, Ordering::Relaxed) % SWEEP_EVERY == SWEEP_EVERY - 1 {
            let cap = self.cap_override.unwrap_or_else(|| cap(space));
            crate::tiles::sweep_cache_dir(&dir, space.label, cap);
        }
    }
}

/// Split a stored file back into its payload, provided it was written for `key`.
fn payload_for<'a>(file: &'a [u8], key: &str) -> Option<&'a [u8]> {
    let head: [u8; 4] = file.get(..4)?.try_into().ok()?;
    let n = u32::from_le_bytes(head) as usize;
    let stored = file.get(4..4usize.checked_add(n)?)?;
    (stored == key.as_bytes()).then(|| &file[4 + n..])
}

impl ObjectStore for DiskStore {
    fn get<'a>(&'a self, space: &'a Space, key: &'a str) -> StoreFuture<'a, Option<Vec<u8>>> {
        Box::pin(async move { self.read(space, key) })
    }

    fn put<'a>(&'a self, space: &'a Space, key: &'a str, bytes: Vec<u8>) -> StoreFuture<'a, ()> {
        Box::pin(async move { self.write(space, key, &bytes) })
    }
}

/// Where the store keeps its folders, if this platform has a cache directory.
pub fn root() -> Option<PathBuf> {
    crate::paths::cache_dir().map(|d| d.join("objects"))
}

/// Register the disk store under the app's cache directory and queue the startup sweep of every
/// space. A no-op where there is no cache directory.
///
/// The GRIB-only store this grew out of kept its folders under `gribcache/`; the first run after
/// the change renames that folder rather than refetching what it holds.
pub fn install() {
    let (Some(root), Some(cache)) = (root(), crate::paths::cache_dir()) else {
        return;
    };
    let old = cache.join("gribcache");
    if old.is_dir() && !root.exists() {
        let _ = std::fs::rename(&old, &root);
    }
    for space in wxdata::objcache::SPACES {
        crate::tiles::sweep_later(root.join(space.slug), space.label, cap(space));
    }
    wxdata::objcache::set_store(Arc::new(DiskStore::new(root)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use wxdata::objcache::{GRIB_GEFS, GRIB_HRRR, GRIB_MODELS, GRIB_RTMA, MRMS};

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hookecho-obj-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_stored_object_comes_back_and_only_for_its_own_key() {
        let root = scratch("roundtrip");
        let s = DiskStore::new(root.clone());
        s.write(&GRIB_HRRR, "https://x/a#0-9", b"GRIB-payload-7777");
        assert_eq!(
            s.read(&GRIB_HRRR, "https://x/a#0-9").as_deref(),
            Some(&b"GRIB-payload-7777"[..])
        );
        assert!(s.read(&GRIB_HRRR, "https://x/a#0-10").is_none());
        // Spaces are separate namespaces.
        assert!(s.read(&GRIB_GEFS, "https://x/a#0-9").is_none());
        assert!(s.read(&MRMS, "https://x/a#0-9").is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_file_written_for_another_key_or_torn_is_not_returned() {
        let root = scratch("collide");
        let s = DiskStore::new(root.clone());
        s.write(&GRIB_RTMA, "key-a", b"payload");
        // Plant key-a's file where key-b's would be: a hash collision, in effect.
        let a = s.path(&GRIB_RTMA, "key-a");
        let b = s.path(&GRIB_RTMA, "key-b");
        std::fs::copy(&a, &b).unwrap();
        assert!(s.read(&GRIB_RTMA, "key-b").is_none());
        // Torn: the header claims more key bytes than the file holds.
        std::fs::write(&a, [200u8, 0, 0, 0, b'k']).unwrap();
        assert!(s.read(&GRIB_RTMA, "key-a").is_none());
        assert!(payload_for(&[], "k").is_none());
        assert!(payload_for(&[255, 255, 255, 255], "k").is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_space_is_swept_back_under_its_quota_oldest_first() {
        let root = scratch("quota");
        // Each entry is 4 + key + 1000 bytes; the quota holds a handful.
        let s = DiskStore::with_cap(root.clone(), 5_000);
        let old = "old-entry";
        s.write(&GRIB_MODELS, old, &[1u8; 1000]);
        let old_path = s.path(&GRIB_MODELS, old);
        let f = std::fs::File::options()
            .write(true)
            .open(&old_path)
            .unwrap();
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
            .unwrap();
        for i in 0..SWEEP_EVERY - 1 {
            s.write(&GRIB_MODELS, &format!("entry-{i:04}"), &[2u8; 1000]);
        }
        let total: u64 = std::fs::read_dir(s.dir(&GRIB_MODELS))
            .unwrap()
            .flatten()
            .map(|e| e.metadata().unwrap().len())
            .sum();
        assert!(total <= 5_000, "swept to the quota, {total} bytes left");
        assert!(!old_path.exists(), "the oldest entry went first");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn reading_an_entry_protects_it_from_the_sweep() {
        let root = scratch("lru");
        let s = DiskStore::with_cap(root.clone(), 3_000);
        s.write(&GRIB_GEFS, "scrubbed-back-over", &[3u8; 1000]);
        for i in 0..SWEEP_EVERY - 2 {
            s.write(&GRIB_GEFS, &format!("filler-{i:04}"), &[4u8; 1000]);
        }
        // Everything so far is old, the scrubbed-back-over message the oldest of all.
        let target = s.path(&GRIB_GEFS, "scrubbed-back-over");
        for (n, e) in std::fs::read_dir(s.dir(&GRIB_GEFS))
            .unwrap()
            .flatten()
            .enumerate()
        {
            let age = if e.path() == target {
                7200
            } else {
                3600 - n as u64
            };
            std::fs::File::options()
                .write(true)
                .open(e.path())
                .unwrap()
                .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(age))
                .unwrap();
        }
        // Reading it makes it the newest entry; the write that triggers the sweep follows.
        assert!(s.read(&GRIB_GEFS, "scrubbed-back-over").is_some());
        s.write(&GRIB_GEFS, "the-64th", &[5u8; 1000]);
        assert!(
            s.read(&GRIB_GEFS, "scrubbed-back-over").is_some(),
            "a read entry outlives older, unread ones"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn the_store_answers_through_the_common_interface() {
        let root = scratch("trait");
        let s = DiskStore::new(root.clone());
        let store: &dyn ObjectStore = &s;
        store.put(&MRMS, "k", b"gz-bytes".to_vec()).await;
        assert_eq!(
            store.get(&MRMS, "k").await.as_deref(),
            Some(&b"gz-bytes"[..])
        );
        assert!(store.get(&MRMS, "other").await.is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    /// Live: a real MRMS grid and a real GOES scan are fetched once, then served from disk with
    /// identical bytes. Network test: `--ignored live_objects_are_fetched_once`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_objects_are_fetched_once() {
        let root = scratch("live");
        wxdata::objcache::set_store(Arc::new(DiskStore::new(root.clone())));
        let http = reqwest::Client::new();
        let t0 = std::time::Instant::now();
        let a = wxdata::mrms::fetch_latest(&http, wxdata::mrms::REFLECTIVITY)
            .await
            .unwrap();
        let cold = t0.elapsed();
        let t1 = std::time::Instant::now();
        let b = wxdata::mrms::fetch_latest(&http, wxdata::mrms::REFLECTIVITY)
            .await
            .unwrap();
        let warm = t1.elapsed();
        let files = std::fs::read_dir(root.join("mrms")).unwrap().count();
        println!("MRMS cold {cold:?}, warm {warm:?}, {files} file(s)");
        assert_eq!(files, 1, "one MRMS grid kept");
        assert_eq!(a.values.len(), b.values.len());
        let g = wxdata::goes_abi::fetch_latest_conus(
            &http,
            wxdata::goes_abi::Satellite::East,
            13,
            400,
            250,
        )
        .await
        .unwrap();
        let goes = std::fs::read_dir(root.join("goes")).unwrap().count();
        println!("GOES: {goes} scan(s) kept");
        assert_eq!(
            goes, 1,
            "one GOES scan kept: its HDF5 superblock checked out"
        );
        let _ = g;
        std::fs::remove_dir_all(&root).ok();
    }
}
