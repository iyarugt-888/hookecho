//! Placefiles: fetching each configured placefile on its own refresh and keeping its icons.
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Reconcile loaded placefiles with `settings.placefiles`: fetch new/enabled URLs, drop
    /// removed ones, mirror the enabled flag, and refetch on each file's `RefreshSeconds`.
    pub(crate) fn sync_placefiles(&mut self, ctx: &egui::Context) {
        // Plugins ride the same pipeline as placefiles — they produce the same format — keyed by
        // a synthetic `plugin:<name>` instead of a URL.
        let plugin_keys: Vec<(String, bool)> = self
            .settings
            .plugins
            .iter()
            .map(|p| (format!("plugin:{}", p.name), p.enabled))
            .collect();
        // Drop entries no longer configured.
        let before = self.placefiles.len();
        self.placefiles.retain(|lp| {
            self.settings.placefiles.iter().any(|c| c.url == lp.url)
                || plugin_keys.iter().any(|(k, _)| *k == lp.url)
        });
        let mut changed = self.placefiles.len() != before;
        for (key, enabled) in &plugin_keys {
            match self.placefiles.iter_mut().find(|lp| lp.url == *key) {
                Some(lp) => {
                    if lp.enabled != *enabled {
                        lp.enabled = *enabled;
                        changed = true;
                    }
                }
                None => {
                    changed = true;
                    self.placefiles.push(LoadedPlacefile {
                        url: key.clone(),
                        enabled: *enabled,
                        pf: Default::default(),
                        last_fetch: None,
                        loaded: false,
                        error: None,
                    });
                }
            }
        }
        for cfg in &self.settings.placefiles {
            match self.placefiles.iter_mut().find(|lp| lp.url == cfg.url) {
                Some(lp) => {
                    if lp.enabled != cfg.enabled {
                        lp.enabled = cfg.enabled;
                        changed = true;
                    }
                }
                None => {
                    changed = true;
                    self.placefiles.push(LoadedPlacefile {
                        url: cfg.url.clone(),
                        enabled: cfg.enabled,
                        pf: Default::default(),
                        last_fetch: None,
                        loaded: false,
                        error: None,
                    });
                }
            }
        }
        // Fetch never-loaded and refresh stale (min 15s cadence).
        let mut to_fetch = Vec::new();
        for lp in &self.placefiles {
            if !lp.enabled {
                continue;
            }
            // A plugin's cadence is the user's setting, not the placefile's own RefreshSeconds:
            // a plugin sampling something live should be asked again on a schedule they control.
            let plugin_secs = self
                .settings
                .plugins
                .iter()
                .find(|p| lp.url == format!("plugin:{}", p.name))
                .map(|p| p.refresh_secs);
            let stale = match (lp.last_fetch, plugin_secs) {
                (None, _) => true,
                // A failed plugin retries on its cadence rather than every frame.
                (Some(t), Some(secs)) => t.elapsed().as_secs() >= secs.max(5) as u64,
                (Some(t), None) => {
                    lp.loaded
                        && lp.pf.refresh_secs > 0
                        && t.elapsed().as_secs() >= lp.pf.refresh_secs.max(15) as u64
                }
            };
            if stale {
                to_fetch.push(lp.url.clone());
            }
        }
        for url in to_fetch {
            if let Some(lp) = self.placefiles.iter_mut().find(|lp| lp.url == url) {
                lp.last_fetch = Some(Instant::now());
            }
            let source = match self
                .settings
                .plugins
                .iter()
                .find(|p| url == format!("plugin:{}", p.name))
            {
                #[cfg(not(target_arch = "wasm32"))]
                Some(p) => OverlaySource::Plugin(
                    url.clone(),
                    p.command.clone(),
                    p.args.clone(),
                    self.plugin_context(),
                ),
                #[cfg(target_arch = "wasm32")]
                Some(_) => OverlaySource::Placefile(url),
                None => OverlaySource::Placefile(url),
            };
            self.spawn_overlay(ctx, source);
        }
        if changed {
            self.overlay_gen = self.overlay_gen.wrapping_add(1);
        }
    }
}
