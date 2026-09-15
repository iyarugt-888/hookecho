//! App storage roots. Desktop resolves the OS config/data/cache dirs via `directories`; Android
//! has no such split, so `android_main` sets one app-private base dir and the three roots become
//! `<base>/config`, `<base>/data`, `<base>/cache`.
//!
//! Every persistent path in the app (settings, color tables, marker icons, tile + climatology
//! caches) goes through here, so the platform difference lives in exactly one place.

use std::path::PathBuf;
use std::sync::OnceLock;

/// Storage base override (Android sets this once from the activity's internal data path).
static BASE: OnceLock<PathBuf> = OnceLock::new();

/// Point every storage root at `base` (Android). One-shot: later calls are ignored.
pub fn set_base(base: PathBuf) {
    let _ = BASE.set(base);
}

/// Resolve a named root: the override's subfolder when set (Android), else the matching OS dir.
fn root(kind: &str) -> Option<PathBuf> {
    if let Some(base) = BASE.get() {
        return Some(base.join(kind));
    }
    // In a browser there is no filesystem. Every caller already treats `None` as "no cache", so
    // the web build keeps everything in memory. Settings are the exception: they persist through
    // `localStorage` instead, entirely inside `settings.rs` (nothing here returns a path for it).
    //
    // Tiles are the exception, and they are cached outside Rust entirely: the service worker
    // keeps them in Cache Storage (`web/sw.src.js`, the `tiles-v1` bucket). Radar volumes are
    // deliberately left out of that bucket — they are large, per-scan, and would evict the map.
    #[cfg(target_arch = "wasm32")]
    {
        let _ = kind;
        return None;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let pd = directories::ProjectDirs::from("", "", "hookecho")?;
    #[cfg(not(target_arch = "wasm32"))]
    Some(match kind {
        "config" => pd.config_dir().to_path_buf(),
        "data" => pd.data_dir().to_path_buf(),
        _ => pd.cache_dir().to_path_buf(),
    })
}

/// Config root (settings.json lives here).
pub fn config_dir() -> Option<PathBuf> {
    root("config")
}

/// Data root (color tables, marker icons).
pub fn data_dir() -> Option<PathBuf> {
    root("data")
}

/// Deep-link drop box: the Android alert service's notification tap writes `SITE,lon,lat,zoom`
/// here (see `MainActivity.kt`) and the app consumes it at startup and on resume. On desktop a
/// second launch carrying a `hookecho://` link writes the same file for the running instance
/// (see `main.rs`), so the handover shape is one thing on both platforms.
pub fn goto_file() -> Option<PathBuf> {
    match BASE.get() {
        Some(b) => Some(b.join("goto.txt")),
        None => cache_dir().map(|c| c.join("goto.txt")),
    }
}

/// Where the last panic's report is written, next to the settings so it survives a restart and
/// works the same on Android (see [`crate::crash`]).
pub fn crash_file() -> Option<PathBuf> {
    match BASE.get() {
        Some(b) => Some(b.join("last-panic.txt")),
        None => config_dir().map(|c| c.join("last-panic.txt")),
    }
}

/// Where the activity drops a file the user picked through the Storage Access Framework, in the
/// same handover shape as [`goto_file`]: the picker result arrives in Kotlin, and the app may not
/// be resumed to receive it.
pub fn import_file() -> Option<PathBuf> {
    BASE.get().map(|b| b.join("import.txt"))
}

/// Where the app writes the picture the home-screen radar widget shows (Android only). Sits at
/// the files-dir root, which is what `Context.filesDir` resolves to on the Kotlin side.
pub fn widget_snapshot() -> Option<PathBuf> {
    BASE.get().map(|b| b.join("widget-radar.png"))
}

/// Where the app writes the one-line storm caption the radar widget shows under its picture.
/// Beside the PNG, in the same files dir the Kotlin side reads.
pub fn widget_caption() -> Option<PathBuf> {
    BASE.get().map(|b| b.join("widget-radar.txt"))
}

/// Cache root (tiles, vector tiles, climatology CSV).
pub fn cache_dir() -> Option<PathBuf> {
    root("cache")
}

/// Total bytes on disk under [`cache_dir`], for ROADMAP_NEW N4's diagnostics bundle.
pub fn cache_dir_bytes() -> u64 {
    cache_dir().map(|d| dir_bytes(&d)).unwrap_or(0)
}

/// Total bytes of every regular file under `root`, walked iteratively (not recursively — the
/// cache tree is a handful of levels deep, but a stack avoids assuming that stays true).
/// Best-effort: a directory that can't be read (permissions, or it simply doesn't exist)
/// contributes 0 for that branch rather than failing the whole walk over one unreadable entry.
/// A free function, not inlined into [`cache_dir_bytes`], so it is testable against a real
/// temporary directory without touching the process-global cache root.
fn dir_bytes(root: &std::path::Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => stack.push(path),
                Ok(ft) if ft.is_file() => {
                    total += entry.metadata().map(|m| m.len()).unwrap_or(0);
                }
                _ => {}
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_roots_resolve_without_override() {
        // With no override set (the desktop path), all three resolve via `directories`.
        // (On CI's headless Linux these still return Some — ProjectDirs needs no real HOME write.)
        assert!(config_dir().is_some());
        assert!(data_dir().is_some());
        assert!(cache_dir().is_some());
    }

    /// A throwaway directory under the OS temp root, unique per test invocation (pid + a counter)
    /// so parallel test runs never collide — deliberately not the app's own `cache_dir()`, per
    /// `dir_bytes`'s own doc comment on why it takes an arbitrary path.
    fn scratch_dir(tag: &str) -> PathBuf {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "hookecho_paths_test_{tag}_{}_{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn dir_bytes_sums_files_at_every_depth() {
        let dir = scratch_dir("sums");
        std::fs::write(dir.join("a.txt"), b"12345").unwrap(); // 5 bytes
        let nested = dir.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("b.txt"), b"1234567890").unwrap(); // 10 bytes
        assert_eq!(dir_bytes(&dir), 15);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dir_bytes_of_a_missing_directory_is_zero_not_an_error() {
        let dir = std::env::temp_dir().join("hookecho_paths_test_does_not_exist_at_all");
        let _ = std::fs::remove_dir_all(&dir); // in case a previous run left it
        assert_eq!(dir_bytes(&dir), 0);
    }

    #[test]
    fn cache_dir_bytes_matches_a_manual_walk_of_the_real_cache_dir() {
        // Not a scratch dir: this exercises `cache_dir_bytes` end to end against whatever the
        // real cache root resolves to (see `desktop_roots_resolve_without_override` — it always
        // resolves on this platform, even if empty).
        let Some(root) = cache_dir() else {
            return;
        };
        assert_eq!(cache_dir_bytes(), dir_bytes(&root));
    }
}
