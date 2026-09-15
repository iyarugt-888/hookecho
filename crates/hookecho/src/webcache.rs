//! Offline chase packs, and an automatic archive-volume cache, both in IndexedDB.
//!
//! The service worker already caches basemap tiles (`web/sw.src.js`, `tiles-v1`) and deliberately
//! excludes radar — a volume is tens of megabytes and the newest one changes every few minutes,
//! which is exactly what a cache-first HTTP cache handles worst. But a chaser about to lose
//! signal wants the opposite trade: pin *these specific* archived volumes, which never change,
//! and read them back with no request at all. That is a pack.
//!
//! Raw Archive II bytes are stored rather than decoded scans, for the same reason the native disk
//! cache stores them: they are smaller, they need no serialization of their own, and they go back
//! through the same decode path either way.
//!
//! ponytail: no eviction inside a pack and no sharing accounting between packs — a volume in two
//! packs is stored once and freed when the last pack referencing it goes. Packs are deleted whole,
//! oldest first, once the store passes [`BYTE_CAP`].
//!
//! The auto-cache (`auto_cached_volume`/`spawn_auto_cache_put`) is the unrelated, unglamorous
//! other half: native builds already keep every archived volume on disk indefinitely
//! (`wxdata::level2::download_scan`'s `cache_dir`), so re-scrubbing to an hour already visited
//! this session is a file read, not a download — but the browser build only got that for a volume
//! someone explicitly saved as a pack. Every other archived volume was refetched from S3 on every
//! visit, including a plain page reload. This mirrors the native behavior for the browser: any
//! archived (never-live) volume is cached the first time it is decoded and read back on every
//! later visit, evicted oldest-`last_used`-first once the store passes [`AUTO_CACHE_BYTE_CAP`] —
//! independently of packs, so background scrubbing can never evict something a chaser pinned on
//! purpose.

#[cfg(target_arch = "wasm32")]
use anyhow::anyhow;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::JsFuture;
#[cfg(target_arch = "wasm32")]
use web_sys::{IdbDatabase, IdbObjectStore, IdbRequest, IdbTransactionMode};

#[cfg(target_arch = "wasm32")]
const DB_NAME: &str = "hookecho-packs";
#[cfg(target_arch = "wasm32")]
const VOLUMES: &str = "volumes";
#[cfg(target_arch = "wasm32")]
const PACKS: &str = "packs";
#[cfg(target_arch = "wasm32")]
const AUTO_VOLUMES: &str = "auto_volumes";
#[cfg(target_arch = "wasm32")]
const AUTO_VOLUMES_META: &str = "auto_volumes_meta";

/// How much radar may sit in IndexedDB before saving a pack evicts the oldest one.
///
/// A browser's quota is a fraction of free disk and is not knowable up front, so this is a
/// self-imposed ceiling well under any plausible quota: about ten loops of a dozen volumes.
#[cfg(target_arch = "wasm32")]
const BYTE_CAP: f64 = 250.0 * 1024.0 * 1024.0;

/// How much the automatic archive-volume cache may hold, independent of [`BYTE_CAP`] — a
/// background cache should never be able to evict a pack a chaser deliberately saved, or vice
/// versa. About 200-500 volumes depending on site/mode, which comfortably covers a session's
/// worth of scrubbing back through one storm.
#[cfg(target_arch = "wasm32")]
const AUTO_CACHE_BYTE_CAP: f64 = 150.0 * 1024.0 * 1024.0;

/// One auto-cached volume's bookkeeping: enough to evict the least-recently-read entry first.
///
/// Kept available (not just `#[cfg(target_arch = "wasm32")]`) on `test` too — the eviction order
/// this and [`entries_over_cap`] decide is worth testing without a browser's IndexedDB, and native
/// is where `cargo test` actually runs.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
struct AutoCacheMeta {
    /// Unix milliseconds of the last read (or write) — the LRU clock.
    last_used: i64,
    bytes: f64,
}

/// Which auto-cached entries to delete to bring `entries`' total back under `cap`, oldest
/// `last_used` first. Pure and target-independent so eviction order is tested without a browser.
#[cfg(any(target_arch = "wasm32", test))]
fn entries_over_cap(entries: &[(String, AutoCacheMeta)], cap: f64) -> Vec<String> {
    let mut ordered: Vec<&(String, AutoCacheMeta)> = entries.iter().collect();
    ordered.sort_by_key(|(_, meta)| meta.last_used);
    let mut total: f64 = entries.iter().map(|(_, meta)| meta.bytes).sum();
    let mut evict = Vec::new();
    for (name, meta) in ordered {
        if total <= cap {
            break;
        }
        total -= meta.bytes;
        evict.push(name.clone());
    }
    evict
}

/// One saved loop.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Pack {
    /// Radar site the loop was saved from.
    pub site: String,
    /// UTC day of the loop, `YYYY-MM-DD`.
    pub date: String,
    /// Volume object names, oldest first — the timeline as it stood when saved.
    pub volumes: Vec<String>,
    /// Unix seconds when it was saved.
    pub saved_at: i64,
    /// Total size of the volumes, for the eviction accounting and the picker's readout.
    pub bytes: f64,
}

impl Pack {
    /// Stable key: one pack per site and day, so re-saving a loop replaces it.
    pub fn key(&self) -> String {
        format!("{}-{}", self.site, self.date)
    }

    /// One line for the picker.
    pub fn label(&self) -> String {
        format!(
            "{} {} \u{2014} {} volume{}, {:.0} MB",
            self.site,
            self.date,
            self.volumes.len(),
            if self.volumes.len() == 1 { "" } else { "s" },
            self.bytes / 1024.0 / 1024.0
        )
    }
}

#[cfg(target_arch = "wasm32")]
/// Await an `IDBRequest`, resolving to its result.
#[cfg(target_arch = "wasm32")]
async fn await_request(req: IdbRequest) -> anyhow::Result<JsValue> {
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let r = req.clone();
        let ok = Closure::once_into_js(move |_: JsValue| {
            let _ = resolve.call1(&JsValue::NULL, &r.result().unwrap_or(JsValue::NULL));
        });
        req.set_onsuccess(Some(ok.unchecked_ref()));
        let err = Closure::once_into_js(move |_: JsValue| {
            let _ = reject.call1(&JsValue::NULL, &"indexeddb request failed".into());
        });
        req.set_onerror(Some(err.unchecked_ref()));
    });
    JsFuture::from(promise).await.map_err(|e| anyhow!("{e:?}"))
}

/// Open the database, creating any store this build knows about but this browser's copy does not
/// yet have. Version 2 added [`AUTO_VOLUMES`]/[`AUTO_VOLUMES_META`] to a database that visitors
/// from before that change already have at version 1 — `IndexedDB` fires `onupgradeneeded` for
/// exactly that gap, and each store is only created if missing, so both a fresh visitor and one
/// upgrading from version 1 land in the same place without a "store already exists" exception.
#[cfg(target_arch = "wasm32")]
async fn open() -> anyhow::Result<IdbDatabase> {
    let factory = web_sys::window()
        .and_then(|w| w.indexed_db().ok().flatten())
        .ok_or_else(|| anyhow!("no IndexedDB in this browser"))?;
    let req = factory
        .open_with_u32(DB_NAME, 2)
        .map_err(|e| anyhow!("{e:?}"))?;
    let upgrade = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
        let Some(req) = ev.target().and_then(|t| t.dyn_into::<IdbRequest>().ok()) else {
            return;
        };
        let Ok(db) = req.result().and_then(|v| v.dyn_into::<IdbDatabase>()) else {
            return;
        };
        let existing = db.object_store_names();
        for name in [VOLUMES, PACKS, AUTO_VOLUMES, AUTO_VOLUMES_META] {
            if !existing.contains(name) {
                let _ = db.create_object_store(name);
            }
        }
    });
    req.set_onupgradeneeded(Some(upgrade.as_ref().unchecked_ref()));
    let db = await_request(req.clone().unchecked_into()).await?;
    // The closure has to outlive the open, and the database outlives everything: leaking one
    // closure per open beats a lifetime dance for an object that lives as long as the page.
    upgrade.forget();
    db.dyn_into::<IdbDatabase>().map_err(|e| anyhow!("{e:?}"))
}

/// The object store for `name`, in a transaction of `mode`.
#[cfg(target_arch = "wasm32")]
fn store(db: &IdbDatabase, name: &str, mode: IdbTransactionMode) -> anyhow::Result<IdbObjectStore> {
    db.transaction_with_str_and_mode(name, mode)
        .and_then(|tx| tx.object_store(name))
        .map_err(|e| anyhow!("{e:?}"))
}

/// Raw bytes of one volume, if a pack holds it.
#[cfg(target_arch = "wasm32")]
pub async fn volume(name: &str) -> Option<Vec<u8>> {
    let db = open().await.ok()?;
    let s = store(&db, VOLUMES, IdbTransactionMode::Readonly).ok()?;
    let v = await_request(s.get(&name.into()).ok()?).await.ok()?;
    let arr = v.dyn_into::<js_sys::Uint8Array>().ok()?;
    Some(arr.to_vec())
}

/// Store one volume's bytes.
#[cfg(target_arch = "wasm32")]
async fn put_volume(db: &IdbDatabase, name: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let s = store(db, VOLUMES, IdbTransactionMode::Readwrite)?;
    let arr = js_sys::Uint8Array::from(bytes);
    let req = s
        .put_with_key(&arr, &name.into())
        .map_err(|e| anyhow!("{e:?}"))?;
    await_request(req).await.map(|_| ())
}

/// Every saved pack, newest first.
#[cfg(target_arch = "wasm32")]
pub async fn packs() -> Vec<Pack> {
    let Ok(db) = open().await else {
        return Vec::new();
    };
    let Ok(s) = store(&db, PACKS, IdbTransactionMode::Readonly) else {
        return Vec::new();
    };
    let Ok(req) = s.get_all() else {
        return Vec::new();
    };
    let Ok(v) = await_request(req).await else {
        return Vec::new();
    };
    let mut out: Vec<Pack> = js_sys::Array::from(&v)
        .iter()
        .filter_map(|e| e.as_string())
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect();
    out.sort_by_key(|p: &Pack| std::cmp::Reverse(p.saved_at));
    out
}

/// Save a pack: store every volume's bytes, then the manifest, then evict oldest packs until the
/// store is back under [`BYTE_CAP`].
///
/// `volumes` is `(object name, raw bytes)` in timeline order. Returns the pack as stored.
#[cfg(target_arch = "wasm32")]
pub async fn save_pack(
    site: &str,
    date: &str,
    volumes: Vec<(String, Vec<u8>)>,
) -> anyhow::Result<Pack> {
    if volumes.is_empty() {
        anyhow::bail!("nothing in the loop to save");
    }
    let db = open().await?;
    let mut bytes = 0.0;
    let mut names = Vec::new();
    for (name, data) in &volumes {
        put_volume(&db, name, data).await?;
        bytes += data.len() as f64;
        names.push(name.clone());
    }
    let pack = Pack {
        site: site.to_string(),
        date: date.to_string(),
        volumes: names,
        saved_at: chrono::Utc::now().timestamp(),
        bytes,
    };
    let s = store(&db, PACKS, IdbTransactionMode::Readwrite)?;
    let json = serde_json::to_string(&pack)?;
    let req = s
        .put_with_key(&json.as_str().into(), &pack.key().into())
        .map_err(|e| anyhow!("{e:?}"))?;
    await_request(req).await?;
    evict(&db).await;
    Ok(pack)
}

/// Delete oldest packs until the store fits under the cap.
#[cfg(target_arch = "wasm32")]
async fn evict(db: &IdbDatabase) {
    let mut all = packs().await;
    let mut total: f64 = all.iter().map(|p| p.bytes).sum();
    all.sort_by_key(|p| p.saved_at);
    for p in all {
        if total <= BYTE_CAP {
            break;
        }
        total -= p.bytes;
        let _ = delete_pack(db, &p).await;
    }
}

/// Remove a pack and any volume of its that no surviving pack still lists.
#[cfg(target_arch = "wasm32")]
async fn delete_pack(db: &IdbDatabase, pack: &Pack) -> anyhow::Result<()> {
    let others: Vec<Pack> = packs()
        .await
        .into_iter()
        .filter(|p| p.key() != pack.key())
        .collect();
    let s = store(db, VOLUMES, IdbTransactionMode::Readwrite)?;
    for name in &pack.volumes {
        if others.iter().any(|p| p.volumes.contains(name)) {
            continue;
        }
        if let Ok(req) = s.delete(&name.as_str().into()) {
            let _ = await_request(req).await;
        }
    }
    let s = store(db, PACKS, IdbTransactionMode::Readwrite)?;
    let req = s
        .delete(&pack.key().as_str().into())
        .map_err(|e| anyhow!("{e:?}"))?;
    await_request(req).await.map(|_| ())
}

/// Delete one pack by key, for the picker's remove button.
#[cfg(target_arch = "wasm32")]
pub async fn remove(key: String) {
    let Ok(db) = open().await else { return };
    if let Some(p) = packs().await.into_iter().find(|p| p.key() == key) {
        let _ = delete_pack(&db, &p).await;
    }
}

/// Every entry's LRU bookkeeping, for eviction and for the read path's own timestamp bump.
#[cfg(target_arch = "wasm32")]
async fn auto_cache_entries(db: &IdbDatabase) -> Vec<(String, AutoCacheMeta)> {
    let Ok(s) = store(db, AUTO_VOLUMES_META, IdbTransactionMode::Readonly) else {
        return Vec::new();
    };
    let (Ok(keys_req), Ok(vals_req)) = (s.get_all_keys(), s.get_all()) else {
        return Vec::new();
    };
    let (Ok(keys), Ok(vals)) = (await_request(keys_req).await, await_request(vals_req).await)
    else {
        return Vec::new();
    };
    // `get_all`/`get_all_keys` both return their results in the same (ascending key) order, so
    // zipping the two arrays pairs each key with its own value without a second round trip.
    js_sys::Array::from(&keys)
        .iter()
        .filter_map(|k| k.as_string())
        .zip(
            js_sys::Array::from(&vals)
                .iter()
                .filter_map(|v| v.as_string())
                .filter_map(|s| serde_json::from_str::<AutoCacheMeta>(&s).ok()),
        )
        .collect()
}

/// Raw bytes of an archived volume from the automatic cache, if this browser has already fetched
/// it this way — never populated for the live head, which [`crate::volume::fetch`] never asks
/// this for. Touches the entry's `last_used` on a hit; a failed touch just makes that entry look
/// slightly older than it is at the next eviction, not a correctness problem.
#[cfg(target_arch = "wasm32")]
pub async fn auto_cached_volume(name: &str) -> Option<Vec<u8>> {
    let db = open().await.ok()?;
    let s = store(&db, AUTO_VOLUMES, IdbTransactionMode::Readonly).ok()?;
    let v = await_request(s.get(&name.into()).ok()?).await.ok()?;
    let bytes = v.dyn_into::<js_sys::Uint8Array>().ok()?.to_vec();
    if let Ok(s) = store(&db, AUTO_VOLUMES_META, IdbTransactionMode::Readwrite) {
        let meta = AutoCacheMeta {
            last_used: chrono::Utc::now().timestamp_millis(),
            bytes: bytes.len() as f64,
        };
        if let Ok(json) = serde_json::to_string(&meta) {
            if let Ok(req) = s.put_with_key(&json.as_str().into(), &name.into()) {
                let _ = await_request(req).await;
            }
        }
    }
    Some(bytes)
}

/// Store one archived volume's bytes in the automatic cache, then evict the least-recently-used
/// entries until the store is back under [`AUTO_CACHE_BYTE_CAP`].
#[cfg(target_arch = "wasm32")]
async fn auto_cache_put(name: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let db = open().await?;
    let s = store(&db, AUTO_VOLUMES, IdbTransactionMode::Readwrite)?;
    let arr = js_sys::Uint8Array::from(bytes);
    let req = s
        .put_with_key(&arr, &name.into())
        .map_err(|e| anyhow!("{e:?}"))?;
    await_request(req).await?;
    let meta = AutoCacheMeta {
        last_used: chrono::Utc::now().timestamp_millis(),
        bytes: bytes.len() as f64,
    };
    let s = store(&db, AUTO_VOLUMES_META, IdbTransactionMode::Readwrite)?;
    let json = serde_json::to_string(&meta)?;
    let req = s
        .put_with_key(&json.as_str().into(), &name.into())
        .map_err(|e| anyhow!("{e:?}"))?;
    await_request(req).await?;
    let victims = entries_over_cap(&auto_cache_entries(&db).await, AUTO_CACHE_BYTE_CAP);
    for name in victims {
        if let Ok(s) = store(&db, AUTO_VOLUMES, IdbTransactionMode::Readwrite) {
            if let Ok(req) = s.delete(&name.as_str().into()) {
                let _ = await_request(req).await;
            }
        }
        if let Ok(s) = store(&db, AUTO_VOLUMES_META, IdbTransactionMode::Readwrite) {
            if let Ok(req) = s.delete(&name.as_str().into()) {
                let _ = await_request(req).await;
            }
        }
    }
    Ok(())
}

/// Fire-and-forget wrapper for [`auto_cache_put`] — the caller (`volume::fetch`) already has the
/// decoded scan in hand and has no reason to wait on the store finishing.
#[cfg(target_arch = "wasm32")]
pub fn spawn_auto_cache_put(name: String, bytes: Vec<u8>) {
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = auto_cache_put(&name, &bytes).await {
            log::debug!("auto-cache put for {name} failed: {e}");
        }
    });
}

/// What the Storage tab reads for the automatic archive-volume cache: total bytes, entry count,
/// and whether a measurement has ever completed. Same "ask once, fill later" shape as `STATE`
/// below, kept separate because the auto-cache and the pack list refresh independently — clearing
/// one has no reason to re-measure the other.
#[cfg(target_arch = "wasm32")]
static AUTO_CACHE_STATE: std::sync::Mutex<(u64, usize, bool)> =
    std::sync::Mutex::new((0, 0, false));

#[cfg(target_arch = "wasm32")]
async fn refresh_auto_cache() {
    let Ok(db) = open().await else { return };
    let entries = auto_cache_entries(&db).await;
    let bytes: u64 = entries.iter().map(|(_, m)| m.bytes as u64).sum();
    if let Ok(mut s) = AUTO_CACHE_STATE.lock() {
        *s = (bytes, entries.len(), true);
    }
}

/// Bytes and entry count in the automatic archive-volume cache, for the Storage tab. The first
/// call kicks off the read that fills them; until it lands this returns `(0, 0)`.
#[cfg(target_arch = "wasm32")]
pub fn known_auto_cache() -> (u64, usize) {
    let asked = {
        let Ok(mut s) = AUTO_CACHE_STATE.lock() else {
            return (0, 0);
        };
        std::mem::replace(&mut s.2, true)
    };
    if !asked {
        wasm_bindgen_futures::spawn_local(refresh_auto_cache());
    }
    AUTO_CACHE_STATE
        .lock()
        .map(|s| (s.0, s.1))
        .unwrap_or((0, 0))
}

/// Force a fresh measurement of the automatic archive-volume cache, for the Storage tab's Refresh
/// button — unlike [`known_auto_cache`], this re-measures even if one already landed.
#[cfg(target_arch = "wasm32")]
pub fn spawn_refresh_auto_cache() {
    wasm_bindgen_futures::spawn_local(refresh_auto_cache());
}

/// Delete every entry in the automatic archive-volume cache, then re-measure it so the Storage tab
/// reflects the clear without a separate Refresh click.
#[cfg(target_arch = "wasm32")]
pub fn spawn_clear_auto_cache() {
    wasm_bindgen_futures::spawn_local(async {
        if let Ok(db) = open().await {
            for name in [AUTO_VOLUMES, AUTO_VOLUMES_META] {
                if let Ok(s) = store(&db, name, IdbTransactionMode::Readwrite) {
                    if let Ok(req) = s.clear() {
                        let _ = await_request(req).await;
                    }
                }
            }
        }
        refresh_auto_cache().await;
    });
}

/// What the UI reads: the packs on hand and the last progress line. Filled by the async saves and
/// loads, which have nowhere else to put a result — the picker is drawn from a `&mut self` the
/// spawned task cannot hold.
#[cfg(target_arch = "wasm32")]
static STATE: std::sync::Mutex<(Vec<Pack>, Option<String>, bool)> =
    std::sync::Mutex::new((Vec::new(), None, false));

/// Packs known to the UI. The first call kicks off the read that fills them.
#[cfg(target_arch = "wasm32")]
pub fn known_packs() -> Vec<Pack> {
    let asked = {
        let Ok(mut s) = STATE.lock() else {
            return Vec::new();
        };
        std::mem::replace(&mut s.2, true)
    };
    if !asked {
        wasm_bindgen_futures::spawn_local(async {
            refresh().await;
        });
    }
    STATE.lock().map(|s| s.0.clone()).unwrap_or_default()
}

/// The last progress or error line, for the picker.
#[cfg(target_arch = "wasm32")]
pub fn status() -> Option<String> {
    STATE.lock().ok().and_then(|s| s.1.clone())
}

#[cfg(target_arch = "wasm32")]
fn set_status(msg: Option<String>) {
    if let Ok(mut s) = STATE.lock() {
        s.1 = msg;
    }
}

#[cfg(target_arch = "wasm32")]
async fn refresh() {
    let list = packs().await;
    if let Ok(mut s) = STATE.lock() {
        s.0 = list;
    }
}

/// Fetch every volume in `ids` — from the pack store when it is already there, from the bucket
/// otherwise — and save them as one pack.
#[cfg(target_arch = "wasm32")]
pub async fn save_timeline(site: String, date: String, ids: Vec<wxdata::level2::Identifier>) {
    let total = ids.len();
    let mut out = Vec::new();
    for (i, id) in ids.into_iter().enumerate() {
        let name = id.name().to_string();
        set_status(Some(format!("saving {} of {total}\u{2026}", i + 1)));
        let bytes = match volume(&name).await {
            Some(b) => b,
            None => match wxdata::level2::volume_bytes(id).await {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("pack: skipping {name}: {e}");
                    continue;
                }
            },
        };
        out.push((name, bytes));
    }
    match save_pack(&site, &date, out).await {
        Ok(p) => set_status(Some(format!("saved {}", p.label()))),
        Err(e) => set_status(Some(format!("could not save the pack: {e}"))),
    }
    refresh().await;
}

/// Delete a pack from the picker.
#[cfg(target_arch = "wasm32")]
pub fn spawn_remove(key: String) {
    wasm_bindgen_futures::spawn_local(async move {
        remove(key).await;
        refresh().await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pack_round_trips_through_its_manifest() {
        let p = Pack {
            site: "KTLX".into(),
            date: "2026-05-20".into(),
            volumes: vec!["KTLX20260520_231502_V06".into()],
            saved_at: 1_780_000_000,
            bytes: 32.0 * 1024.0 * 1024.0,
        };
        let back: Pack = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        assert_eq!(back.key(), "KTLX-2026-05-20");
        assert_eq!(back.label(), "KTLX 2026-05-20 — 1 volume, 32 MB");
    }

    fn meta(last_used: i64, bytes: f64) -> AutoCacheMeta {
        AutoCacheMeta { last_used, bytes }
    }

    /// Under the cap, nothing is evicted — the common case on every write.
    #[test]
    fn nothing_is_evicted_under_the_cap() {
        let entries = vec![
            ("a".to_string(), meta(1, 10.0)),
            ("b".to_string(), meta(2, 10.0)),
        ];
        assert!(entries_over_cap(&entries, 100.0).is_empty());
    }

    /// Over the cap, the oldest `last_used` goes first, and only as many entries as it takes to
    /// get back under the cap — not every entry older than the newest one.
    #[test]
    fn the_least_recently_used_entries_go_first() {
        let entries = vec![
            ("newest".to_string(), meta(30, 10.0)),
            ("oldest".to_string(), meta(10, 10.0)),
            ("middle".to_string(), meta(20, 10.0)),
        ];
        assert_eq!(entries_over_cap(&entries, 25.0), vec!["oldest".to_string()]);
        assert_eq!(
            entries_over_cap(&entries, 5.0),
            vec![
                "oldest".to_string(),
                "middle".to_string(),
                "newest".to_string()
            ]
        );
    }

    /// A cache holding nothing has nothing to evict — must not panic on an empty slice.
    #[test]
    fn an_empty_cache_evicts_nothing() {
        assert!(entries_over_cap(&[], 0.0).is_empty());
    }
}
