//! A cache for immutable remote objects, the same on every platform (ROADMAP_NEW A3).
//!
//! Much of what the app downloads never changes once it exists: a GRIB2 message of a published
//! model run, an MRMS file named for the minute it was made, a GOES scan named for its start time.
//! Their URLs (and byte ranges) name them for good, so a copy kept once can be handed back on
//! every later read — scrubbing back, flipping products, re-opening the app — without a request.
//!
//! The store is pluggable and async ([`ObjectStore`]): the desktop and phone apps register a disk
//! store, the web build registers IndexedDB, and a build with none (headless tools, tests) simply
//! fetches every time. Entries live in a [`Space`] per source family, each with its own byte quota
//! (the store enforces it, least recently used first).
//!
//! Nothing is trusted on the way in or out: every entry is checked by the caller's `valid` test
//! when it is stored and again when it is read, so a truncated download or a damaged entry is a
//! miss and a refetch, never a wrong picture.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

/// A store operation in flight. `Send` natively, where fetches run on a multi-threaded runtime;
/// not on the web, where IndexedDB's futures live on the page's one thread.
#[cfg(not(target_arch = "wasm32"))]
pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
#[cfg(target_arch = "wasm32")]
pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Where cached objects are kept. Implementations must never panic: a store that fails is a
/// cache miss, not an error.
pub trait ObjectStore: Send + Sync {
    /// The bytes kept under `key` in `space`, if any.
    fn get<'a>(&'a self, space: &'a Space, key: &'a str) -> StoreFuture<'a, Option<Vec<u8>>>;
    /// Keep `bytes` under `key` in `space`, then trim the space back under its quota. Best effort.
    fn put<'a>(&'a self, space: &'a Space, key: &'a str, bytes: Vec<u8>) -> StoreFuture<'a, ()>;
}

/// A namespace with its own quota: one source family.
#[derive(Debug, PartialEq, Eq)]
pub struct Space {
    /// Stable on disk and in IndexedDB.
    pub slug: &'static str,
    /// A person-facing name for the Storage tab.
    pub label: &'static str,
    /// Byte quota on a desktop.
    pub cap: u64,
    /// Byte quota where storage is scarcer: Android and the browser.
    pub cap_small: u64,
}

const MB: u64 = 1024 * 1024;

pub const GRIB_HRRR: Space = Space {
    slug: "hrrr",
    label: "HRRR GRIB",
    cap: 256 * MB,
    cap_small: 64 * MB,
};
pub const GRIB_GEFS: Space = Space {
    slug: "gefs",
    label: "GEFS GRIB",
    cap: 256 * MB,
    cap_small: 64 * MB,
};
pub const GRIB_RTMA: Space = Space {
    slug: "rtma",
    label: "RTMA GRIB",
    cap: 256 * MB,
    cap_small: 64 * MB,
};
pub const GRIB_MODELS: Space = Space {
    slug: "models",
    label: "Other model GRIB",
    cap: 256 * MB,
    cap_small: 64 * MB,
};
/// MRMS national grids: each file is a few MB gzipped, named for its minute.
pub const MRMS: Space = Space {
    slug: "mrms",
    label: "MRMS grids",
    cap: 256 * MB,
    cap_small: 64 * MB,
};
/// GOES ABI scans: the CONUS channels are tens of MB each, so this holds an animation's worth.
pub const GOES: Space = Space {
    slug: "goes",
    label: "GOES satellite scans",
    cap: 512 * MB,
    cap_small: 96 * MB,
};

/// Every space, for sweeps and the Storage tab.
pub const SPACES: [&Space; 6] = [
    &GRIB_HRRR,
    &GRIB_GEFS,
    &GRIB_RTMA,
    &GRIB_MODELS,
    &MRMS,
    &GOES,
];

static STORE: OnceLock<Arc<dyn ObjectStore>> = OnceLock::new();

/// Register the store. One-shot: later calls are ignored, like the other process-wide hooks.
pub fn set_store(store: Arc<dyn ObjectStore>) {
    let _ = STORE.set(store);
}

/// The registered store, if any.
pub fn store() -> Option<&'static Arc<dyn ObjectStore>> {
    STORE.get()
}

/// The object under `key` in `space`: from the store when it holds a copy that passes `valid`,
/// otherwise from `fetch` — whose answer is stored only if it passes `valid` too, and returned
/// either way, so a caller sees exactly what an uncached fetch would have given it.
pub async fn cached<F>(
    space: &Space,
    key: &str,
    valid: impl Fn(&[u8]) -> bool,
    fetch: F,
) -> anyhow::Result<Vec<u8>>
where
    F: Future<Output = anyhow::Result<Vec<u8>>>,
{
    cached_in(store().map(|s| s.as_ref()), space, key, valid, fetch).await
}

/// [`cached`] against a given store (`None`: fetch every time).
async fn cached_in<F>(
    store: Option<&dyn ObjectStore>,
    space: &Space,
    key: &str,
    valid: impl Fn(&[u8]) -> bool,
    fetch: F,
) -> anyhow::Result<Vec<u8>>
where
    F: Future<Output = anyhow::Result<Vec<u8>>>,
{
    if let Some(store) = store {
        if let Some(hit) = store.get(space, key).await {
            if valid(&hit) {
                return Ok(hit);
            }
        }
    }
    let bytes = fetch.await?;
    if valid(&bytes) {
        if let Some(store) = store {
            store.put(space, key, bytes.clone()).await;
        }
    }
    Ok(bytes)
}

/// A stable file or record name for a key: 128 bits of its SHA-256, hex.
pub fn digest(key: &str) -> String {
    use sha2::Digest;
    let sum = sha2::Sha256::digest(key.as_bytes());
    sum[..16].iter().map(|b| format!("{b:02x}")).collect()
}

/// A gzip stream that is whole: it decompresses to the end and its CRC-32 and length trailer
/// match. An MRMS file cut short by a dropped connection fails this; decompressing a few MB costs
/// milliseconds, a fraction of fetching it.
pub fn is_whole_gzip(bytes: &[u8]) -> bool {
    use std::io::Read;
    if bytes.len() < 18 || bytes[..2] != [0x1f, 0x8b] {
        return false;
    }
    let mut sink = [0u8; 64 * 1024];
    let mut dec = flate2::read::GzDecoder::new(bytes);
    loop {
        match dec.read(&mut sink) {
            Ok(0) => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
}

/// An HDF5 file (what a GOES ABI netCDF-4 scan is) that is whole: its length is exactly the
/// end-of-file address its superblock records, past its base address. Superblock versions 0-3.
pub fn is_whole_hdf5(bytes: &[u8]) -> bool {
    hdf5_extent(bytes).is_some_and(|end| end == bytes.len() as u64)
}

/// Where an HDF5 file says it ends: its base address plus its end-of-file address.
fn hdf5_extent(b: &[u8]) -> Option<u64> {
    if !b.starts_with(b"\x89HDF\r\n\x1a\n") {
        return None;
    }
    let read = |at: usize, n: usize| -> Option<u64> {
        let s = b.get(at..at + n)?;
        Some(
            s.iter()
                .rev()
                .fold(0u64, |acc, &x| (acc << 8) | u64::from(x)),
        )
    };
    let version = *b.get(8)?;
    let (offsets, base_at) = match version {
        // sig 8, versions 5, offset/length sizes 2, reserved 1, node Ks 4, flags 4 (+ 4 in v1).
        0 => (usize::from(*b.get(13)?), 24),
        1 => (usize::from(*b.get(13)?), 28),
        // sig 8, version 1, offset/length sizes 2, flags 1.
        2 | 3 => (usize::from(*b.get(9)?), 12),
        _ => return None,
    };
    if !matches!(offsets, 2 | 4 | 8) {
        return None;
    }
    // Base address, then one address (free-space info in v0/1, the superblock extension in v2/3),
    // then the end-of-file address.
    let base = read(base_at, offsets)?;
    base.checked_add(read(base_at + 2 * offsets, offsets)?)
}

#[cfg(test)]
pub mod testing {
    //! An in-memory store for tests, counting its traffic.
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct MemStore {
        pub entries: Mutex<HashMap<(String, String), Vec<u8>>>,
        pub gets: Mutex<usize>,
    }

    impl ObjectStore for MemStore {
        fn get<'a>(&'a self, space: &'a Space, key: &'a str) -> StoreFuture<'a, Option<Vec<u8>>> {
            Box::pin(async move {
                *self.gets.lock().unwrap() += 1;
                self.entries
                    .lock()
                    .unwrap()
                    .get(&(space.slug.to_string(), key.to_string()))
                    .cloned()
            })
        }
        fn put<'a>(
            &'a self,
            space: &'a Space,
            key: &'a str,
            bytes: Vec<u8>,
        ) -> StoreFuture<'a, ()> {
            Box::pin(async move {
                self.entries
                    .lock()
                    .unwrap()
                    .insert((space.slug.to_string(), key.to_string()), bytes);
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spaces_are_distinct_and_small_quotas_are_smaller() {
        let mut slugs: Vec<_> = SPACES.iter().map(|s| s.slug).collect();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), SPACES.len());
        assert!(SPACES
            .iter()
            .all(|s| s.cap_small < s.cap && s.cap_small > 0));
    }

    fn ok(b: &[u8]) -> impl Future<Output = anyhow::Result<Vec<u8>>> {
        let b = b.to_vec();
        async move { Ok(b) }
    }

    fn starts_ok(b: &[u8]) -> bool {
        b.starts_with(b"OK")
    }

    #[tokio::test]
    async fn a_valid_object_is_fetched_once_then_served_from_the_store() {
        let mem = testing::MemStore::default();
        let a = cached_in(Some(&mem), &MRMS, "k", starts_ok, ok(b"OK-1"))
            .await
            .unwrap();
        assert_eq!(a, b"OK-1");
        // The second read never reaches its fetch: it would have answered something else.
        let b = cached_in(Some(&mem), &MRMS, "k", starts_ok, ok(b"OK-2"))
            .await
            .unwrap();
        assert_eq!(b, b"OK-1");
        // Spaces are separate namespaces.
        let c = cached_in(Some(&mem), &GOES, "k", starts_ok, ok(b"OK-3"))
            .await
            .unwrap();
        assert_eq!(c, b"OK-3");
    }

    #[tokio::test]
    async fn an_invalid_answer_is_returned_but_never_kept_and_a_bad_entry_is_refetched() {
        let mem = testing::MemStore::default();
        let a = cached_in(Some(&mem), &MRMS, "k", starts_ok, ok(b"truncated"))
            .await
            .unwrap();
        assert_eq!(a, b"truncated", "the caller still sees what the fetch gave");
        assert!(mem.entries.lock().unwrap().is_empty(), "nothing kept");
        mem.entries
            .lock()
            .unwrap()
            .insert(("mrms".into(), "k".into()), b"damaged".to_vec());
        let b = cached_in(Some(&mem), &MRMS, "k", starts_ok, ok(b"OK-fresh"))
            .await
            .unwrap();
        assert_eq!(b, b"OK-fresh");
        assert_eq!(
            mem.entries.lock().unwrap()[&("mrms".to_string(), "k".to_string())],
            b"OK-fresh"
        );
        // No store: straight through.
        let c = cached_in(None, &MRMS, "k", starts_ok, ok(b"OK-none"))
            .await
            .unwrap();
        assert_eq!(c, b"OK-none");
    }

    #[test]
    fn a_gzip_is_whole_only_when_its_trailer_checks_out() {
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(&[7u8; 50_000]).unwrap();
        let gz = enc.finish().unwrap();
        assert!(is_whole_gzip(&gz));
        assert!(!is_whole_gzip(&gz[..gz.len() - 3]), "cut short");
        let mut flipped = gz.clone();
        let n = flipped.len();
        flipped[n - 6] ^= 0xff;
        assert!(!is_whole_gzip(&flipped), "bad CRC");
        assert!(!is_whole_gzip(b"<Error>SlowDown</Error> padded to length"));
    }

    /// A superblock of `version` with 8-byte offsets, declaring the file `eof` bytes long.
    fn hdf5(version: u8, eof: u64, len: usize) -> Vec<u8> {
        let mut b = b"\x89HDF\r\n\x1a\n".to_vec();
        match version {
            0 | 1 => {
                b.extend_from_slice(&[version, 0, 0, 0, 0, 8, 8, 0, 4, 0, 16, 0, 0, 0, 0, 0]);
                if version == 1 {
                    b.extend_from_slice(&[0; 4]);
                }
                b.extend_from_slice(&0u64.to_le_bytes()); // base
                b.extend_from_slice(&u64::MAX.to_le_bytes()); // free space
                b.extend_from_slice(&eof.to_le_bytes());
            }
            _ => {
                b.extend_from_slice(&[version, 8, 8, 0]);
                b.extend_from_slice(&0u64.to_le_bytes()); // base
                b.extend_from_slice(&u64::MAX.to_le_bytes()); // extension
                b.extend_from_slice(&eof.to_le_bytes());
            }
        }
        b.resize(len, 0);
        b
    }

    #[test]
    fn an_hdf5_file_is_whole_only_at_the_length_its_superblock_declares() {
        for v in [0u8, 1, 2, 3] {
            assert!(is_whole_hdf5(&hdf5(v, 4096, 4096)), "v{v}");
            assert!(!is_whole_hdf5(&hdf5(v, 4096, 4000)), "v{v} cut short");
        }
        assert!(!is_whole_hdf5(b"GRIB...7777"));
        assert!(!is_whole_hdf5(b"\x89HDF\r\n\x1a\n\x09"), "unknown version");
    }
}
