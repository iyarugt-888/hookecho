//! Offline packs (the web build's saved scenes) and soundings (a point's profile, refetched and
//! compared with the one before). Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    pub(crate) fn fetch_sounding(&mut self, lon: f64, lat: f64) {
        self.sounding_window.fh = 0;
        self.sounding_at = Some((lon, lat));
        self.refetch_sounding();
        self.fetch_raob(lon, lat);
    }

    /// Re-pull the sounding at the remembered point for the window's current forecast hour. The
    /// observed ascent is left alone — a radiosonde has no forecast hours.
    pub(crate) fn refetch_sounding(&mut self) {
        let Some((lon, lat)) = self.sounding_at else {
            return;
        };
        let fh = self.sounding_window.fh;
        let model = self.sounding_window.model;
        let (tx, rx) = std::sync::mpsc::channel();
        self.sounding_rx = Some(rx);
        self.sounding_window.open = true;
        self.sounding_window.busy = true;
        self.sounding_window.sounding = None;
        self.sounding_window.error = None;
        let http = self.http.clone();
        self.spawner.spawn(async move {
            let res = wxdata::sounding::fetch_model_at(&http, model, lon, lat, fh)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
    }

    /// The previous HRRR run at the current sounding's valid time.
    pub(crate) fn fetch_previous_sounding(&mut self) {
        let Some(current) = self.sounding_window.sounding.as_ref() else {
            return;
        };
        let current = wxdata::sounding::Sounding {
            lon: current.lon,
            lat: current.lat,
            run: current.run,
            fh: current.fh,
            levels: Vec::new(),
        };
        self.sounding_window.previous_error = None;
        let model = self.sounding_window.model;
        let (tx, rx) = std::sync::mpsc::channel();
        self.previous_sounding_rx = Some(rx);
        let http = self.http.clone();
        self.spawner.spawn(async move {
            let res = wxdata::sounding::fetch_previous_model_run(&http, model, &current)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
    }

    /// Packs saved in this browser, refreshed in the background whenever one is written.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn packs(&self) -> Vec<crate::webcache::Pack> {
        crate::webcache::known_packs()
    }

    /// The last thing the pack machinery has to say — a progress line or an error.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn pack_status(&self) -> Option<String> {
        crate::webcache::status()
    }

    /// Save the active timeline's archived frames into an offline pack.
    ///
    /// The live head is skipped on purpose: the newest object can still be uploading, and half a
    /// volume kept forever is worse than one frame missing from a saved loop.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn save_offline_pack(&mut self, ctx: &egui::Context) {
        let tl = &self.views[self.active].timeline;
        let ids: Vec<_> = tl.frames.iter().take(tl.playhead + 1).cloned().collect();
        let site = self.views[self.active].site.clone().unwrap_or_default();
        let date = tl.date.format("%Y-%m-%d").to_string();
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            crate::webcache::save_timeline(site, date, ids).await;
            ctx.request_repaint();
        });
    }

    /// Point the active pane at a saved pack: its site and day, and its frames, without listing
    /// anything over the network. Playback then reads each volume back out of IndexedDB.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn load_offline_pack(&mut self, pack: &crate::webcache::Pack) {
        let Ok(date) = chrono::NaiveDate::parse_from_str(&pack.date, "%Y-%m-%d") else {
            return;
        };
        self.views[self.active].site = Some(pack.site.clone());
        let tl = &mut self.views[self.active].timeline;
        tl.date = date;
        tl.following = false;
        tl.listing = false;
        tl.frames = pack
            .volumes
            .iter()
            .map(|n| Identifier::new(n.clone()))
            .collect();
        tl.playhead = 0;
        tl.playing = true;
        tl.loop_enabled = true;
    }
}
