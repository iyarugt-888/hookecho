//! Offline packs (the web build's saved scenes, and the desktop's loop kept on disk) and soundings
//! (a point's profile, refetched and compared with the one before). Moved out of `app.rs`
//! (ROADMAP_2 §7).

use super::*;

/// The desktop's "keep this loop" progress, for the timeline's menu.
#[cfg(not(target_arch = "wasm32"))]
static KEEP_STATUS: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(not(target_arch = "wasm32"))]
fn set_keep_status(s: String) {
    if let Ok(mut g) = KEEP_STATUS.lock() {
        *g = Some(s);
    }
}

/// The frames a kept loop holds: the last `n` up to and including the playhead.
#[cfg(not(target_arch = "wasm32"))]
fn loop_to_keep(frames: &[Identifier], playhead: usize, n: usize) -> Vec<Identifier> {
    let end = (playhead + 1).min(frames.len());
    frames[end.saturating_sub(n)..end].to_vec()
}

#[cfg(not(target_arch = "wasm32"))]
impl HookEchoApp {
    /// How keeping a loop on this device is going, if one was asked for.
    pub(crate) fn keep_status(&self) -> Option<String> {
        KEEP_STATUS.lock().ok().and_then(|s| s.clone())
    }

    /// Download the active timeline's loop (its last `frames` volumes up to the playhead) into
    /// the disk cache, two at a time, skipping any already there. With no signal the day's
    /// listing then falls back to them (`level2::cached_volumes`), so the loop still plays
    /// (ROADMAP_NEW L5, 1010.md V9).
    pub(crate) fn keep_loop_offline(&mut self, ctx: &egui::Context, frames: usize) {
        let Some(cache) = crate::paths::cache_dir() else {
            set_keep_status("This build keeps no files on disk.".into());
            return;
        };
        let tl = &self.views[self.active].timeline;
        let ids = loop_to_keep(&tl.frames, tl.playhead, frames);
        if ids.is_empty() {
            set_keep_status("No volumes in this loop yet.".into());
            return;
        }
        let total = ids.len();
        let missing: Vec<Identifier> = ids
            .into_iter()
            .filter(|id| !cache.join("volumes").join(id.name()).exists())
            .collect();
        let already = total - missing.len();
        if missing.is_empty() {
            set_keep_status(format!(
                "This loop's {total} volumes are already on this device; it plays with no signal."
            ));
            return;
        }
        set_keep_status(format!(
            "Keeping this loop on this device: 0 of {} volumes to fetch\u{2026}",
            missing.len()
        ));
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            let (mut done, mut failed) = (0usize, 0usize);
            let todo = missing.len();
            for pair in missing.chunks(2) {
                let jobs = pair
                    .iter()
                    .map(|id| crate::volume::fetch(id.clone(), true, Some(cache.clone())));
                for r in futures_util::future::join_all(jobs).await {
                    match r {
                        Ok(_) => done += 1,
                        Err(e) => {
                            failed += 1;
                            log::warn!("keep loop offline: {e:#}");
                        }
                    }
                }
                set_keep_status(format!(
                    "Keeping this loop on this device: {} of {todo} volumes fetched\u{2026}",
                    done + failed
                ));
                ctx.request_repaint();
            }
            let kept = already + done;
            set_keep_status(if failed == 0 {
                format!("This loop's {kept} volumes are on this device; it plays with no signal.")
            } else {
                format!(
                    "{kept} of this loop's {total} volumes are on this device; {failed} could not \
                     be fetched."
                )
            });
            ctx.request_repaint();
        });
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod keep_tests {
    use super::*;

    /// A kept loop is the last `n` frames up to the playhead, fewer near the start, none of
    /// an empty timeline.
    #[test]
    fn a_kept_loop_is_the_frames_up_to_the_playhead() {
        let frames: Vec<Identifier> = (0..10)
            .map(|i| Identifier::new(format!("KTLX20130520_20{i:02}00_V06")))
            .collect();
        let names = |v: Vec<Identifier>| -> Vec<String> {
            v.iter().map(|id| id.name().to_string()).collect()
        };
        assert_eq!(
            names(loop_to_keep(&frames, 6, 3)),
            [
                "KTLX20130520_200400_V06",
                "KTLX20130520_200500_V06",
                "KTLX20130520_200600_V06"
            ]
        );
        assert_eq!(loop_to_keep(&frames, 1, 5).len(), 2);
        assert_eq!(loop_to_keep(&frames, 40, 4).len(), 4);
        assert!(loop_to_keep(&[], 0, 4).is_empty());
    }
}

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

    /// The GEFS members' envelope at the current sounding's point and valid time (ROADMAP_NEW F8),
    /// the members counted into the window as they arrive.
    pub(crate) fn fetch_sounding_envelope(&mut self, ctx: &egui::Context) {
        let Some(current) = self.sounding_window.sounding.as_ref() else {
            return;
        };
        let (lon, lat) = (current.lon, current.lat);
        let valid = current.run + chrono::Duration::hours(i64::from(current.fh));
        self.sounding_window.envelope_error = None;
        let progress = std::sync::Arc::clone(&self.sounding_window.envelope_progress);
        progress.store(0, std::sync::atomic::Ordering::Relaxed);
        let (tx, rx) = std::sync::mpsc::channel();
        self.envelope_rx = Some(rx);
        let (http, ctx) = (self.http.clone(), ctx.clone());
        self.spawner.spawn(async move {
            let repaint = ctx.clone();
            let res = wxdata::ensemble_sounding::fetch_envelope(&http, lon, lat, valid, move |n| {
                progress.store(n, std::sync::atomic::Ordering::Relaxed);
                repaint.request_repaint();
            })
            .await
            .map_err(|e| e.to_string());
            let _ = tx.send(res);
            ctx.request_repaint();
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
