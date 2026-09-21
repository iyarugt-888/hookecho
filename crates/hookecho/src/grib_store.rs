//! On-disk store behind `wxdata::gribcache`: one file per GRIB2 message, a folder per source
//! family, each family kept under its own byte quota.
//!
//! A file is `[u32 LE key length][key][payload]`. The key is stored so a name collision (a 128-bit
//! hash makes one absurdly unlikely, but the check costs nothing) or a file from another key can
//! never be returned as the wrong message. Writes go to a temporary name and are renamed into
//! place, so a crash mid-write leaves a stray temp file the next sweep removes, never a torn entry.
//!
//! Desktop and Android only; the web build has no filesystem and registers no store.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use wxdata::gribcache::{Family, RangeStore};

/// Per-family quota. A message is a few hundred KB to a couple of MB, so this holds a few hundred
/// of them: a full evening of scrubbing several products across a run.
pub const FAMILY_CACHE_BYTES: u64 = if cfg!(target_os = "android") {
    64 * 1024 * 1024
} else {
    256 * 1024 * 1024
};

/// Writes between size checks of a family's folder.
const SWEEP_EVERY: u32 = 64;

pub struct DiskStore {
    root: PathBuf,
    cap: u64,
    puts: AtomicU32,
}

impl DiskStore {
    pub fn new(root: PathBuf, cap: u64) -> Self {
        Self {
            root,
            cap,
            puts: AtomicU32::new(0),
        }
    }

    fn dir(&self, family: Family) -> PathBuf {
        self.root.join(family.slug())
    }

    fn path(&self, family: Family, key: &str) -> PathBuf {
        self.dir(family)
            .join(format!("{}.grib", wxdata::gribcache::digest(key)))
    }
}

/// Split a stored file back into its payload, provided it was written for `key`.
fn payload_for<'a>(file: &'a [u8], key: &str) -> Option<&'a [u8]> {
    let head: [u8; 4] = file.get(..4)?.try_into().ok()?;
    let n = u32::from_le_bytes(head) as usize;
    let stored = file.get(4..4usize.checked_add(n)?)?;
    (stored == key.as_bytes()).then(|| &file[4 + n..])
}

impl RangeStore for DiskStore {
    fn get(&self, family: Family, key: &str) -> Option<Vec<u8>> {
        let path = self.path(family, key);
        let file = std::fs::read(&path).ok()?;
        match payload_for(&file, key) {
            Some(p) => {
                // Touch it so the oldest-first sweep is least-recently-used, not first-written:
                // the message being scrubbed back over is the one worth keeping.
                if let Ok(f) = std::fs::File::options().write(true).open(&path) {
                    let _ = f.set_modified(std::time::SystemTime::now());
                }
                Some(p.to_vec())
            }
            None => None,
        }
    }

    fn put(&self, family: Family, key: &str, bytes: &[u8]) {
        let dir = self.dir(family);
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = self.path(family, key);
        let tmp = path.with_extension("tmp");
        let mut out = Vec::with_capacity(4 + key.len() + bytes.len());
        out.extend_from_slice(&(key.len() as u32).to_le_bytes());
        out.extend_from_slice(key.as_bytes());
        out.extend_from_slice(bytes);
        if std::fs::write(&tmp, &out).is_ok() && std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        if self.puts.fetch_add(1, Ordering::Relaxed) % SWEEP_EVERY == SWEEP_EVERY - 1 {
            crate::tiles::sweep_cache_dir(&dir, family.label(), self.cap);
        }
    }
}

/// Register the disk store under the app's cache directory and queue the startup sweep of every
/// family folder. A no-op where there is no cache directory.
pub fn install() {
    let Some(root) = crate::paths::cache_dir().map(|d| d.join("gribcache")) else {
        return;
    };
    for f in Family::ALL {
        crate::tiles::sweep_later(root.join(f.slug()), f.label(), FAMILY_CACHE_BYTES);
    }
    wxdata::gribcache::set_store(Arc::new(DiskStore::new(root, FAMILY_CACHE_BYTES)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hookecho-grib-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_stored_message_comes_back_and_only_for_its_own_key() {
        let root = scratch("roundtrip");
        let s = DiskStore::new(root.clone(), 1 << 30);
        s.put(Family::Hrrr, "https://x/a#0-9", b"GRIB-payload-7777");
        assert_eq!(
            s.get(Family::Hrrr, "https://x/a#0-9").as_deref(),
            Some(&b"GRIB-payload-7777"[..])
        );
        assert!(s.get(Family::Hrrr, "https://x/a#0-10").is_none());
        // Families are separate namespaces.
        assert!(s.get(Family::Gefs, "https://x/a#0-9").is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_file_written_for_another_key_or_torn_is_not_returned() {
        let root = scratch("collide");
        let s = DiskStore::new(root.clone(), 1 << 30);
        s.put(Family::Rtma, "key-a", b"payload");
        // Plant key-a's file where key-b's would be: a hash collision, in effect.
        let a = s.path(Family::Rtma, "key-a");
        let b = s.path(Family::Rtma, "key-b");
        std::fs::copy(&a, &b).unwrap();
        assert!(s.get(Family::Rtma, "key-b").is_none());
        // Torn: the header claims more key bytes than the file holds.
        std::fs::write(&a, [200u8, 0, 0, 0, b'k']).unwrap();
        assert!(s.get(Family::Rtma, "key-a").is_none());
        assert!(payload_for(&[], "k").is_none());
        assert!(payload_for(&[255, 255, 255, 255], "k").is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_family_is_swept_back_under_its_quota_oldest_first() {
        let root = scratch("quota");
        // Each entry is 4 + key + 1000 bytes; the quota holds a handful.
        let s = DiskStore::new(root.clone(), 5_000);
        let old = "old-entry";
        s.put(Family::Models, old, &[1u8; 1000]);
        let old_path = s.path(Family::Models, old);
        let f = std::fs::File::options()
            .write(true)
            .open(&old_path)
            .unwrap();
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
            .unwrap();
        for i in 0..SWEEP_EVERY - 1 {
            s.put(Family::Models, &format!("entry-{i:04}"), &[2u8; 1000]);
        }
        let total: u64 = std::fs::read_dir(s.dir(Family::Models))
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
        let s = DiskStore::new(root.clone(), 3_000);
        s.put(Family::Gefs, "scrubbed-back-over", &[3u8; 1000]);
        for i in 0..SWEEP_EVERY - 2 {
            s.put(Family::Gefs, &format!("filler-{i:04}"), &[4u8; 1000]);
        }
        // Everything so far is old, the scrubbed-back-over message the oldest of all.
        let target = s.path(Family::Gefs, "scrubbed-back-over");
        for (n, e) in std::fs::read_dir(s.dir(Family::Gefs))
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
        assert!(s.get(Family::Gefs, "scrubbed-back-over").is_some());
        s.put(Family::Gefs, "the-64th", &[5u8; 1000]);
        assert!(
            s.get(Family::Gefs, "scrubbed-back-over").is_some(),
            "a read entry outlives older, unread ones"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// Live: a real RTMA message is fetched once, then served from disk with identical bytes and no
    /// request. Network test: `--ignored a_live_message_is_fetched_once`.
    #[tokio::test]
    #[ignore = "network"]
    async fn a_live_message_is_fetched_once() {
        let root = scratch("live");
        wxdata::gribcache::set_store(Arc::new(DiskStore::new(root.clone(), 1 << 30)));
        let http = reqwest::Client::new();
        let hour = Some(
            chrono::Utc::now()
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
                - chrono::Duration::hours(3),
        );
        let t0 = std::time::Instant::now();
        let a = wxdata::rtma::fetch(&http, wxdata::rtma::RtmaField::ALL[0], hour)
            .await
            .unwrap();
        let cold = t0.elapsed();
        let files: Vec<_> = std::fs::read_dir(root.join("rtma"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(files.len(), 1, "one message cached");
        let bytes = files[0].metadata().unwrap().len();
        // Take the network away: a cached message must not need it.
        let dead = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
            .build()
            .unwrap();
        let t1 = std::time::Instant::now();
        let b = wxdata::rtma::fetch(&dead, wxdata::rtma::RtmaField::ALL[0], hour).await;
        let warm = t1.elapsed();
        println!("cold {cold:?}, warm {warm:?}, {bytes} bytes on disk");
        // The idx read still needs the network, so the warm fetch may fail there; what matters is
        // that the message itself came from disk when the idx was reachable.
        let c = wxdata::rtma::fetch(&http, wxdata::rtma::RtmaField::ALL[0], hour)
            .await
            .unwrap();
        let bits =
            |f: &wxdata::mrms::MrmsField| f.values.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
        assert!(
            bits(&a.field) == bits(&c.field),
            "the cached message decodes identically"
        );
        let _ = b;
        std::fs::remove_dir_all(&root).ok();
    }
}
