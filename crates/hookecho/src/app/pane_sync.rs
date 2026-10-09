//! Keeping each pane in step: its timeline, its site and product, and the mini-loop viewport.
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Per-frame per-pane: react to site changes, keep the timeline current, and (for the active
    /// pane) manage the live stream. Each pane fetches its own volume via its view index.
    pub(crate) fn sync_pane(&mut self, idx: usize, ctx: &egui::Context) {
        // Site change: clear the old volume, recenter, and (if a real site) refetch.
        let site_changed = self.views[idx].site != self.views[idx].loaded_site;
        if site_changed {
            let v = &mut self.views[idx];
            v.loaded_site = v.site.clone();
            v.volume = None;
            v.forget_recent();
            v.moments_seen = [false; Moment::ALL.len()];
            v.live_progress = None;
            v.live_progress_at = None;
            v.live_scan.reset(v.site.clone());
            v.live_render_started = None;
            v.live_queue_timings = Arc::new(crate::render::LiveQueueTimings::default());
            v.error = None;
            // Clear a stuck in-flight flag: if the previous site's fetch is still running when the
            // site changes, its result is dropped on arrival (site mismatch) without clearing
            // `loading`, which would then block the new site's fetch forever ("no volume").
            v.loading = false;
            // The old site's frame list is not this site's. Left in place it is both what the
            // scrub path displays and what it downloads until the new listing lands — i.e. the
            // wrong radar's volumes at an index that means nothing here. A scrubbed pane keeps
            // the time it was looking at; a live one just goes back to the head.
            // ...unless a deep link, an event or a replay bundle already asked for an instant:
            // that target is the whole point of the jump, and overwriting it with the frame the
            // pane happened to be showing sent every Event Library entry to the wrong day.
            if !v.timeline.following && v.timeline.seek_target.is_none() {
                v.timeline.seek_target = v.timeline.current().and_then(|id| id.date_time());
            }
            v.timeline.frames.clear();
            v.timeline.playhead = 0;
            match &v.site {
                // ...unless a deep link already aimed the camera at something specific.
                Some(_) if std::mem::take(&mut v.camera_placed) => {}
                Some(s) => ui::site_dialog::center_on_site(&mut v.camera, s),
                None => {
                    self.pane_shown.remove(&idx);
                }
            }
            // Storm cells follow the active pane's site: drop the old ones (and any open old-site
            // storm popup / trend history — the ring-click path in try_pick_site does the same) and
            // refetch.
            if idx == self.active {
                self.storm_cells.clear();
                self.cells_site = None;
                self.cell_trends.clear();
                self.cell_popup = None;
                if let Some(site) = self.views[idx].site.clone() {
                    let history = self.cells_history_site.as_deref() != Some(site.as_str());
                    self.spawn_overlay(ctx, OverlaySource::Cells(site, history));
                }
            }
        }

        // Advance playback (if playing) then reconcile the displayed volume with the timeline.
        // Past the scan cache a loop plays light frames (`app::long_loop`), a few percent of a
        // volume each, so the phone and the browser loop as long as the desktop's memory allows.
        self.views[idx].timeline.live_window = self
            .settings
            .live_loop_frames
            .clamp(1, long_loop::MAX_LOOP_FRAMES);

        // The browser demo opens playing. A single frozen frame is indistinguishable from a broken
        // map to someone who has never seen this app, and the last fifteen minutes is what makes
        // radar readable — you cannot tell which way a storm is moving from a still.
        //
        // Waits for two frames rather than starting on one, so the first thing the visitor sees
        // move is an actual loop and not a one-frame stutter. `backfill_loop_frames` is what
        // fetches the tail; once playing, ordinary prefetch takes over.
        if self.autoplay_pending && !(idx == self.active && self.model_timeline_active()) {
            let tl = &self.views[idx].timeline;
            if tl.following && !tl.playing {
                let window = tl.live_window.max(1);
                let start = tl.frames.len().saturating_sub(window);
                let ready = tl.frames[start..]
                    .iter()
                    .filter(|id| self.scan_cache.contains(&id.name().to_string()))
                    .count();
                if ready >= 2 {
                    log::info!("loop: playing {ready}/{window} frames");
                    self.views[idx].timeline.toggle_play();
                    self.autoplay_pending = false;
                } else {
                    self.backfill_loop_frames(idx, ctx);
                }
            }
        }
        // Hold the playhead while the next frame is still downloading. Advancing on the wall clock
        // regardless meant playback skipped frames it hadn't got yet and the loop read as juddery;
        // waiting reads as buffering, which is what it is. Only while there's a fetch to wait for,
        // so a permanently-failed frame can't stall the loop.
        let next_pending = {
            let tl = &self.views[idx].timeline;
            tl.playing
                && tl
                    .frames
                    .get(tl.playhead + 1)
                    .map(|id| id.name().to_string())
                    .is_some_and(|n| {
                        !self.scan_cache.contains(&n)
                            && !(self.light_ok(idx) && self.light_frame(idx, &n).is_some())
                            && book(&self.prefetching).get(&n).is_some_and(|at| {
                                // Bounded: a fetch that never answers must not park the loop.
                                at.elapsed() < std::time::Duration::from_secs(8)
                            })
                    })
        };
        // Likewise while its 3D is still being built, so the 3D view plays frame for frame
        // rather than showing one scan's volume under the next scan's time (ROADMAP_NEW H8).
        if !next_pending && !self.next_frame_3d_pending(idx) {
            self.views[idx].timeline.tick();
        }
        // Playback paces itself rather than riding whatever the idle heartbeat happens to give
        // it: ask for a repaint exactly when the next frame is due.
        if let Some(dt) = self.views[idx].timeline.time_to_next_frame() {
            ctx.request_repaint_after(dt);
        }
        self.sync_timeline(idx, ctx, site_changed);

        // Live streaming is limited to the active pane; others poll their head.
        if idx == self.active {
            self.manage_stream(ctx);
        }
    }

    /// Reconcile the frame listing and the displayed volume with the timeline: keep the
    /// listing current, poll the live head while following, or load the scrubbed frame.
    pub(crate) fn sync_timeline(&mut self, idx: usize, ctx: &egui::Context, site_changed: bool) {
        if !crate::platform::activity::is_active() {
            return; // backgrounded: no listings, no head polls, no downloads
        }
        // (Re)list volumes when the site or selected date changed.
        let (site, date, following, need_list, listing) = {
            let v = &self.views[idx];
            let key = v.site.clone().map(|s| (s, v.timeline.date));
            // TDWRs and DWD radars have no archive to list; their timeline stays empty and
            // always live.
            let need = v.site.as_deref().is_some_and(wxdata::sites::is_nexrad)
                && v.timeline.frames_key != key;
            (
                v.site.clone(),
                v.timeline.date,
                v.timeline.following,
                need,
                v.timeline.listing,
            )
        };
        if let Some(s) = &site {
            if need_list && !listing {
                self.views[idx].timeline.listing = true;
                self.spawn_list_frames(idx, s.clone(), date, ctx.clone());
            }
        }

        let looping = self.views[idx].timeline.live_looping();
        if following {
            self.poll_live_head(idx, looping, site_changed, ctx);
        }
        // A playing live loop owns its display at the head frame too: live chunks are not
        // merged while it plays, so without this the newest frame kept showing the one before it.
        if !following || looping || self.views[idx].timeline.playing {
            // Archive / loop: display the volume at the playhead (cache hit is synchronous).
            let target = self.views[idx].timeline.current().map(|id| {
                (
                    id.name().to_string(),
                    id.date_time().unwrap_or_else(Utc::now),
                    id.clone(),
                )
            });
            if let Some((name, time, id)) = target {
                let light_ok = self.light_ok(idx);
                // A light loop frame on screen stands until the loop stops (or needs the whole
                // volume for 3D, Max or Clean); then the full volume replaces it.
                let need = match self.views[idx].volume.as_ref() {
                    Some(v) if v.name == name => v.light && !light_ok,
                    _ => true,
                };
                if need {
                    if let Some(scan) = self.scan_cache.get(&name).map(Arc::clone) {
                        wxdata::stats::bump(wxdata::stats::Counter::ScanCacheHits);
                        self.remember_light(idx, &name, &scan);
                        let v = &mut self.views[idx];
                        v.show_volume(scan, name, time);
                        v.loading = false;
                        v.error = None;
                        v.clamp_tilt();
                        v.clamp_moment();
                        self.pane_shown.remove(&idx);
                    } else if let Some(light) = self.light_frame(idx, &name).filter(|_| light_ok) {
                        let v = &mut self.views[idx];
                        v.show_light_volume(light, name, time);
                        v.loading = false;
                        v.error = None;
                        self.pane_shown.remove(&idx);
                    } else if !self.views[idx].loading {
                        wxdata::stats::bump(wxdata::stats::Counter::ScanCacheMisses);
                        let s = self.views[idx].site.clone().unwrap_or_default();
                        self.views[idx].loading = true;
                        self.spawn_frame_fetch(idx, s, id, ctx.clone());
                    }
                }
                // Pull neighbouring frames in behind the playhead, on their own in-flight book so
                // they never compete with the frame being shown or with the head poll. Without
                // this, playback is a serial download per frame with the loop stalled between —
                // and scrubbing, which runs this same path paused, was a cold download per step.
                self.prefetch_frames(idx, ctx);
                // The trail's window can reach past what the loop keeps; fetch it, bounded.
                self.prefetch_trail_window(idx, ctx);
            }
        }
    }

    /// The always-on-top mini loop: a small undecorated window showing the active pane, so the
    /// radar stays visible over whatever else is on screen.
    ///
    /// An *immediate* viewport, not a deferred one: deferred viewports run their closure on the
    /// egui side and demand `'static + Send + Sync`, which `HookEchoApp` is not (wgpu handles,
    /// `Rc`s in the tile caches). Immediate renders inline on this thread, which is exactly what
    /// reusing [`Self::render_pane`] needs.
    /// The loop keeps its own camera, cloned from the active pane when the window opens and
    /// swapped in for the duration of the render — panning the little window is how you look
    /// somewhere else while the main map stays where it was.
    // ponytail: not persisted; it is a window, not a layer (see `OverlayToggle::session_only`).
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    pub(crate) fn mini_loop_viewport(&mut self, ctx: &egui::Context) {
        if !self.mini_loop {
            return;
        }
        let idx = self.active.min(self.views.len() - 1);
        let caption = {
            let v = &self.views[idx];
            let time = v.volume.as_ref().map_or_else(
                || "—".to_string(),
                |vol| vol.time.format("%H:%MZ").to_string(),
            );
            format!(
                "{} {} {time}",
                v.site.as_deref().unwrap_or("—"),
                v.moment.short_name()
            )
        };
        let builder = egui::ViewportBuilder::default()
            .with_title("HookEcho — mini loop")
            .with_inner_size([340.0, 260.0])
            .with_decorations(false)
            // Honoured on X11 only. The comment here used to blame GNOME's policy and say KDE
            // was fine — it is not a policy question: winit's Wayland backend implements
            // `set_window_level` as an empty function (winit 0.30, `platform_impl/linux/wayland/
            // window/mod.rs`), so no Wayland compositor is ever asked. Nothing to fix here
            // without going around winit to `xdg-foreign`/layer-shell, which is not worth it for
            // one optional window. The tool's own description says so under Wayland rather than
            // leaving the user to wonder why their window keeps disappearing behind the browser.
            .with_always_on_top();
        let mut close = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("mini-loop"),
            builder,
            |ctx, _class| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        let full = ui.max_rect();
                        // Undecorated, so we draw our own caption strip: label, drag handle, close.
                        let (bar, prect) = (
                            egui::Rect::from_min_max(
                                full.min,
                                egui::pos2(full.max.x, full.min.y + 18.0),
                            ),
                            egui::Rect::from_min_max(
                                egui::pos2(full.min.x, full.min.y + 18.0),
                                full.max,
                            ),
                        );
                        ui.painter()
                            .rect_filled(bar, 0.0, egui::Color32::from_gray(24));
                        let handle = ui.interact(
                            bar,
                            egui::Id::new("mini-loop-bar"),
                            egui::Sense::click_and_drag(),
                        );
                        if handle.drag_started() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }
                        ui.painter().text(
                            bar.left_center() + egui::vec2(6.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            &caption,
                            egui::FontId::proportional(11.0),
                            egui::Color32::from_gray(190),
                        );
                        let x = egui::Rect::from_center_size(
                            bar.right_center() - egui::vec2(10.0, 0.0),
                            egui::vec2(16.0, 16.0),
                        );
                        if ui
                            .interact(x, egui::Id::new("mini-loop-close"), egui::Sense::click())
                            .clicked()
                        {
                            close = true;
                        }
                        ui.painter().text(
                            x.center(),
                            egui::Align2::CENTER_CENTER,
                            "✕",
                            egui::FontId::proportional(11.0),
                            egui::Color32::from_gray(190),
                        );
                        let vctx = ui.ctx().clone();
                        // Swap this window's camera in around the render, and take back whatever
                        // the pointer did to it. Nothing returns early in between, so the pane
                        // always gets its own camera back.
                        let mine = self.mini_cam.take().unwrap_or(self.views[idx].camera);
                        let pane_cam = std::mem::replace(&mut self.views[idx].camera, mine);
                        self.render_pane(
                            // `first`/`last` false: the mini-loop viewport is a passenger — the
                            // main window's pane loop owns draining and evicting the tile caches.
                            ui,
                            &vctx,
                            idx,
                            prect,
                            false,
                            false,
                            false,
                            false,
                            &[],
                        );
                        self.mini_cam =
                            Some(std::mem::replace(&mut self.views[idx].camera, pane_cam));
                    });
                if ctx.input(|i| i.viewport().close_requested()) {
                    close = true;
                }
            },
        );
        if close {
            self.mini_loop = false;
            // Reopening should frame what the main map is looking at now, not where the loop was
            // pointed an hour ago.
            self.mini_cam = None;
        }
    }
}
