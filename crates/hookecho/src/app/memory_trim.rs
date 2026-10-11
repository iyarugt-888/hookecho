//! Giving memory back when Android asks for it (1010.md V4, expansion.md §6.3).
//!
//! Android ranks cached processes by how much they hold and kills from the top. Everything here
//! can be rebuilt (3D frames from the volumes, scans and terrain from the disk cache or the
//! network), so holding it while hidden only moves the app up that list. The trim reaches the app
//! on its own thread through `android_main`'s event-loop wrapper; see `lib.rs`.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use super::*;

/// How hard to trim, from `ComponentCallbacks2.onTrimMemory`'s level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Trim {
    /// Keep everything.
    Keep,
    /// Still on screen, and the system is short: drop what is built ahead of the picture.
    Visible,
    /// Hidden and on the system's list of processes to kill: drop everything that can be rebuilt.
    Background,
}

impl Trim {
    pub(crate) fn from_level(level: i32) -> Self {
        match level {
            // RUNNING_MODERATE, RUNNING_LOW, RUNNING_CRITICAL. Android 14 stopped sending these.
            5..=15 => Trim::Visible,
            // UI_HIDDEN: the user just switched away and may well be straight back. Rebuilding a
            // 3D loop for a glance at a text message costs more than it saves.
            20 => Trim::Keep,
            // BACKGROUND, MODERATE, COMPLETE: what is held now decides how soon the app is killed.
            l if l >= 40 => Trim::Background,
            _ => Trim::Keep,
        }
    }
}

impl HookEchoApp {
    /// Drop the rebuildable caches as far as `trim` says. What each pane shows is held outside
    /// these caches, so the picture survives; what goes is built or fetched again when wanted.
    /// Returns a line for the log.
    pub(crate) fn trim_memory(&mut self, trim: Trim) -> String {
        if trim == Trim::Keep {
            return "kept everything".to_owned();
        }
        self.loop3d_jobs.drain(&mut self.loop3d);
        let built: usize = self.loop3d.iter().map(Loop3dCache::bytes).sum();
        for cache in &mut self.loop3d {
            cache.clear();
        }
        let light = self.light_frames.clear();
        let mut line = format!(
            "dropped {} MB of built 3D frames and {light} loop frames",
            built >> 20
        );
        if trim == Trim::Background {
            let scans = self.scan_cache.len();
            self.scan_cache.clear();
            let tiles = self.terrain.trim();
            line.push_str(&format!(
                ", {scans} decoded scans and {tiles} terrain tiles"
            ));
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use super::Trim;

    #[test]
    fn android_levels_map_to_how_hard_to_trim() {
        assert_eq!(Trim::from_level(5), Trim::Visible);
        assert_eq!(Trim::from_level(15), Trim::Visible);
        assert_eq!(
            Trim::from_level(20),
            Trim::Keep,
            "a quick app switch keeps the 3D loop"
        );
        assert_eq!(Trim::from_level(40), Trim::Background);
        assert_eq!(Trim::from_level(80), Trim::Background);
        assert_eq!(Trim::from_level(0), Trim::Keep);
    }

    /// The `fn` names in the block that starts at `header`, at its first level of indentation.
    fn methods(src: &str, header: &str) -> Vec<String> {
        let start = src
            .find(header)
            .unwrap_or_else(|| panic!("{header} not found"));
        let indent = src[..start].rsplit('\n').next().unwrap().len();
        let close = format!("\n{}}}", " ".repeat(indent));
        let body = &src[start..start + src[start..].find(&close).unwrap()];
        let fn_at = format!("\n{}    fn ", " ".repeat(indent));
        let mut names: Vec<String> = body
            .split(&fn_at)
            .skip(1)
            .map(|rest| rest.split(['(', '<']).next().unwrap().to_owned())
            .collect();
        names.sort();
        names
    }

    /// On Android eframe sees `SharedApp` in `lib.rs`, not `HookEchoApp`. A method added to the
    /// app's `eframe::App` impl and not passed through there would silently fall back to
    /// eframe's default on Android alone.
    #[test]
    fn the_android_wrapper_passes_on_every_app_method() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let read = |p: &str| std::fs::read_to_string(root.join(p)).unwrap();
        let app = methods(&read("src/app.rs"), "impl eframe::App for HookEchoApp {");
        let shared = methods(&read("src/lib.rs"), "impl eframe::App for SharedApp {");
        assert!(app.contains(&"ui".to_owned()), "parsed {app:?}");
        assert_eq!(
            app, shared,
            "SharedApp in lib.rs must pass on each of these"
        );
    }
}
