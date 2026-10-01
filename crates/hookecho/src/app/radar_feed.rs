//! The radar feed: live streams and completed-volume fetches, which provider serves them, and
//! picking a site. Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

/// Whether a live-head poll may start now (ROADMAP_2 §1.3: provider loss must not freeze the
/// display). Polling is the floor under the live stream, so it must never stop for good: one is
/// started when none is in flight and the interval has passed (or the site changed), and also
/// when the one in flight has outlived every deadline it could have. Nothing in the app bounds a
/// poll as a whole, and a browser fetch has no timeout of its own, so a single hung request used
/// to leave `loading` set and the pane frozen on its last volume. A late answer from the
/// abandoned poll is harmless: `LiveScan::accept_volume` never lets an older volume back in.
pub(crate) fn poll_may_start(
    loading: bool,
    since_poll: Option<std::time::Duration>,
    interval: std::time::Duration,
    site_changed: bool,
) -> bool {
    let stuck = loading && since_poll.is_some_and(|d| d >= POLL_STUCK_AFTER);
    let due = site_changed || since_poll.is_none_or(|d| d >= interval);
    (!loading || stuck) && due
}

/// How long a live poll may be in flight before it counts as hung: past the volume download's
/// own deadline, with room for the listing before it.
pub(crate) const POLL_STUCK_AFTER: std::time::Duration =
    std::time::Duration::from_secs(VOLUME_TIMEOUT.as_secs() + 15);

impl HookEchoApp {
    /// Reconstruct a vertical reflectivity cross-section along the two clicked endpoints from
    /// pane `idx`'s volume, upload it as a texture, and open the cross-section window.
    /// If the click landed on a radar-site ring (and not on a storm report/cell, which take
    /// precedence), switch pane `idx` to that site and return true. `sync_pane` reacts to the
    /// changed site — no extra plumbing here.
    pub(crate) fn try_pick_site(
        &mut self,
        idx: usize,
        pos: egui::Pos2,
        cam: crate::render::mercator::Camera,
        prect: egui::Rect,
        vp: (f32, f32),
    ) -> bool {
        let to_screen_hit = |lon: f64, lat: f64| {
            let w = crate::render::mercator::lonlat_to_world(lon, lat);
            let (sx, sy) = cam.world_to_screen(w, vp);
            let (dx, dy) = (prect.left() + sx - pos.x, prect.top() + sy - pos.y);
            dx * dx + dy * dy
        };
        // Storm features win: bail if a report or cell dot sits under the cursor.
        let near_storm = (self.show_storm_reports
            && self
                .active_storm_reports()
                .iter()
                .any(|r| to_screen_hit(r.lon, r.lat) <= tap_r2(12.0)))
            || (self.cells_site.as_deref() == self.views[idx].site.as_deref()
                && self
                    .active_storm_cells()
                    .iter()
                    .any(|c| to_screen_hit(c.lon, c.lat) <= tap_r2(14.0)));
        if near_storm {
            return false;
        }
        let hit = wxdata::sites::all()
            .filter(|s| to_screen_hit(s.longitude as f64, s.latitude as f64) <= tap_r2(12.0))
            .min_by(|a, b| {
                to_screen_hit(a.longitude as f64, a.latitude as f64)
                    .total_cmp(&to_screen_hit(b.longitude as f64, b.latitude as f64))
            });
        match hit {
            Some(s) if self.views[idx].site.as_deref() != Some(s.id) => {
                self.views[idx].site = Some(s.id.to_string());
                self.cell_popup = None;
                self.warning_popup = None;
                self.detail = None;
                true
            }
            _ => false,
        }
    }

    /// Volume poll cadence, doubled on a metered link. A phone on mobile data pulls a multi-MB
    /// volume every interval; halving that rate costs at most a couple of minutes of latency on
    /// the live head, which the chunk stream covers anyway when it is running.
    pub(crate) fn poll_interval_secs(&self) -> u64 {
        let base = self.settings.poll_interval_secs;
        let base = if crate::platform::is_metered() {
            base * 2
        } else {
            base
        };
        // Battery saver stacks with metering: both are "spend less", and a chaser who has turned
        // both on has said so twice.
        if self.settings.battery_saver {
            base * 2
        } else {
            base
        }
    }

    /// A live stream ended. Only the current stream may clear the pane's acquisition state; a
    /// stale end (an older generation) is ignored. `lost` says it ended while still wanted, which
    /// reads as Recovering; one the app stopped (a new generation, a hidden tab, a backgrounded
    /// app) does not.
    pub(crate) fn live_ended(&mut self, view: usize, gen: u64, lost: bool) {
        let current = self
            .live_stream
            .as_ref()
            .is_some_and(|(v, _, g, _)| *v == view && *g == gen);
        if !current {
            return;
        }
        self.live_stream = None; // interval polling resumes automatically
        if let Some(v) = self.views.get_mut(view) {
            v.live_progress = None;
            v.live_progress_at = None;
            v.live_retries = 0;
            if lost {
                v.live_scan.stream_ended();
            } else {
                v.live_scan.stream_stopped();
            }
        }
    }

    /// A palette step (`NavStep`): the key action that already does it, or the loop's own
    /// play/pause, which had no key.
    pub(crate) fn apply_nav(&mut self, step: NavStep, ctx: &egui::Context) {
        match step.bindable() {
            Some(action) => self.apply_action(action, ctx),
            None => self.views[self.active].timeline.toggle_play(),
        }
    }

    /// The pane follows the live head: poll for the newest volume when one may start
    /// ([`poll_may_start`]).
    pub(crate) fn poll_live_head(
        &mut self,
        idx: usize,
        looping: bool,
        site_changed: bool,
        ctx: &egui::Context,
    ) {
        // Live head: poll for the newest volume. While looping, the displayed volume is a
        // middle loop frame, so compare against the newest *frame* (not the shown volume) to
        // decide whether the head advanced — otherwise every poll re-downloads the head.
        let (site, current_name, start) = {
            let v = &self.views[idx];
            let start = poll_may_start(
                v.loading,
                v.last_poll.map(|t| t.elapsed()),
                std::time::Duration::from_secs(self.poll_interval_secs()),
                site_changed,
            );
            let current_name = if looping {
                v.timeline.frames.last().map(|id| id.name().to_string())
            } else {
                v.volume.as_ref().map(|vol| vol.name.clone())
            };
            (v.site.clone(), current_name, start)
        };
        if start {
            if let Some(s) = site {
                if self.views[idx].loading {
                    log::warn!("{s}: live poll hung past its deadline; polling again");
                }
                self.views[idx].loading = true;
                self.views[idx].last_poll = Some(Instant::now());
                self.spawn_fetch(idx, s, current_name, ctx.clone());
            }
        }
    }

    /// Spawn a background fetch of the latest volume for `site`, routed back to `view_idx`.
    /// `current_name = None` forces a re-download even if the newest volume is unchanged.
    pub(crate) fn spawn_fetch(
        &self,
        view_idx: usize,
        site: String,
        current_name: Option<String>,
        ctx: egui::Context,
    ) {
        let tx = self.msg_tx.clone();
        let network = wxdata::sites::network(&site);
        if network != wxdata::sites::Network::Nexrad {
            // None of these has a Level 2 feed or an archive: one volume per poll, synthesized
            // from the newest Level 3 tilt products (TDWR) or assembled from the newest ODIM
            // files (DWD, OPERA).
            let http = self.http.clone();
            self.spawner.spawn(async move {
                use wxdata::sites::Network;
                // Each of these asks its feed what the newest volume is *before* downloading it,
                // and answers `None` when that is what we are already showing. The NEXRAD path
                // below has always worked this way — it lists, then compares, then downloads;
                // these three used to download the whole volume and compare afterwards, which on
                // DWD meant ~50 requests and ~2 MB discarded on nine polls out of ten.
                let cur = current_name.as_deref();
                let fetched = match network {
                    Network::Dwd => wxdata::dwd::fetch_volume(&http, &site, cur).await,
                    Network::Opera => wxdata::opera::fetch_volume(&http, &site, cur).await,
                    Network::Tdwr | Network::Nexrad => {
                        wxdata::tdwr::fetch_volume(&http, &site, cur).await
                    }
                };
                let msg = match fetched {
                    Ok(None) => DataMsg::UpToDate {
                        view: view_idx,
                        site,
                    },
                    // A feed that hands back the name we already hold anyway — a probe that could
                    // not decode, say — is still up to date.
                    Ok(Some((name, _, _))) if current_name.as_deref() == Some(name.as_str()) => {
                        DataMsg::UpToDate {
                            view: view_idx,
                            site,
                        }
                    }
                    Ok(Some((name, time, scan))) => DataMsg::Volume {
                        view: view_idx,
                        site,
                        name,
                        time,
                        scan,
                        live_poll: true,
                    },
                    Err(e) => DataMsg::Error {
                        view: view_idx,
                        site,
                        err: e.to_string(),
                    },
                };
                let _ = tx.send(msg);
                ctx.request_repaint();
            });
            return;
        }
        self.spawner.spawn(async move {
            use crate::volume::{LatestVolume, Level2LiveProvider, UnidataLevel2Provider};
            let msg = match UnidataLevel2Provider
                .latest_complete_volume(&site, current_name.as_deref())
                .await
            {
                Ok(LatestVolume::UpToDate) => DataMsg::UpToDate {
                    view: view_idx,
                    site,
                },
                Ok(LatestVolume::New { name, time, scan }) => DataMsg::Volume {
                    view: view_idx,
                    site,
                    name,
                    time,
                    scan,
                    live_poll: true,
                },
                Err(e) => DataMsg::Error {
                    view: view_idx,
                    site,
                    err: e.to_string(),
                },
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    /// Start/stop the live chunk stream for the active view. One stream at a time (the active
    /// view); a healthy stream starves interval polling, a dead one lets polling take over.
    /// Runs on the web too: the chunk objects are on a bucket that allows cross-origin reads, and
    /// the streamer's waits and backfill were already written without tokio.
    pub(crate) fn manage_stream(&mut self, ctx: &egui::Context) {
        let idx = self.active;
        let (want, site, base) = {
            let v = &self.views[idx];
            // Stream only while pinned to the live head; scrubbing pauses it. A live loop is
            // suppressed too — the loop shows past frames, so interval polling (not the sweep
            // stream) carries new-volume arrival. ponytail: stream resumes on pause / go_head.
            // Only WSR-88Ds have a Level 2 chunk stream to merge — asking for one downloads a
            // WSR-88D-shaped file that isn't there and decodes garbage.
            let want = v.timeline.following
                && !v.timeline.playing
                && v.site.as_deref().is_some_and(wxdata::sites::is_nexrad)
                && v.volume.is_some();
            (
                want,
                v.site.clone(),
                // Shared with the pane: the streamer merges into a new Scan rather than mutating
                // this one, so it only needs a refcount, not the tens of MB a deep copy cost on
                // the UI thread at every stream start.
                v.volume.as_ref().map(|vol| Arc::clone(&vol.scan)),
            )
        };

        // ROADMAP_NEW B6.11 step 11: keep the dual-feed health/failover decision for this pane's
        // site current, on the same lifetime as `want` above (no point monitoring a backup path
        // for a site nothing is actively following live), *before* the abort check below — a tier
        // change needs to actually abort a running stream on the old tier, not just be picked up
        // the next time one happens to restart on its own.
        #[cfg(not(target_arch = "wasm32"))]
        self.sync_radar_providers(idx, want, site.as_deref(), base.as_ref());
        #[cfg(not(target_arch = "wasm32"))]
        let desired_label = self.views[idx]
            .radar_providers
            .as_ref()
            .map(|p| crate::radar_provider_manager::label_for_tier(p.selected_tier()));
        #[cfg(target_arch = "wasm32")]
        let desired_label: Option<&'static str> = None;

        // Abort an existing stream if it no longer matches the active view/site/desired provider,
        // or isn't wanted.
        if let Some((sv, ss, _, sl)) = &self.live_stream {
            if !want || *sv != idx || Some(ss.as_str()) != site.as_deref() || *sl != desired_label {
                let ended_view = *sv;
                // ponytail: the cancelled stream notices within a second (its wait is sliced),
                // so a fast site switch overlaps two streams for about that long and at most
                // one in-flight chunk fetch. An abort channel if even that shows up.
                self.live_gen
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.live_stream = None;
                // Stopped on purpose (a loop, a scrub, a new site or provider), not lost: it must
                // not read as Recovering. A stream that dies on its own ends in `LiveEnded`.
                if ended_view < self.views.len() {
                    self.views[ended_view].live_scan.stream_stopped();
                }
                // A new site (or a failover switch) shouldn't inherit the old one's 60 s retry
                // gate — a switch away from a stalled/failing provider should reconnect promptly.
                self.last_stream_attempt = None;
            }
        }

        if want && self.live_stream.is_none() {
            let due = self
                .last_stream_attempt
                .is_none_or(|t| t.elapsed().as_secs() >= 60);
            // `want` already implies both, but the two are computed a screen away from here.
            if let (true, Some(site), Some(base)) = (due, site, base) {
                self.last_stream_attempt = Some(Instant::now());
                let provider = desired_label.unwrap_or("Unidata Level II (AWS S3)");
                let same_scan_site =
                    self.views[idx].live_scan.site.as_deref() == Some(site.as_str());
                if self.views[idx].live_scan.site.is_none() {
                    self.views[idx].live_scan.site = Some(site.clone());
                } else if !same_scan_site {
                    self.views[idx].live_scan.reset(Some(site.clone()));
                }
                // Every switch is logged (ROADMAP_2 §1.3), the web's too: there the only one is
                // the stream coming back after completed-volume polling.
                if same_scan_site {
                    if let Some(from) = self.views[idx].live_scan.provider.clone() {
                        if from != provider {
                            #[cfg(not(target_arch = "wasm32"))]
                            let manager_reason = self.views[idx]
                                .radar_providers
                                .as_ref()
                                .map(|p| p.snapshot())
                                .and_then(|snapshot| {
                                    snapshot.last_transition.and_then(|(_, reason, tier)| {
                                        (tier == snapshot.selected).then_some(
                                            crate::radar_provider_manager::label_for_reason(reason),
                                        )
                                    })
                                });
                            #[cfg(target_arch = "wasm32")]
                            let manager_reason: Option<&'static str> = None;
                            let reason = manager_reason.unwrap_or(
                                if from == crate::live_scan::COMPLETED_POLL_LABEL {
                                    "live stream reconnected"
                                } else {
                                    "provider configuration changed"
                                },
                            );
                            #[cfg(not(target_arch = "wasm32"))]
                            let degraded =
                                provider == crate::radar_provider_manager::DEGRADED_LABEL;
                            #[cfg(target_arch = "wasm32")]
                            let degraded = false;
                            let mode = if degraded {
                                "completed volumes only"
                            } else {
                                "progressive radials"
                            };
                            log::info!(
                                target: "hookecho::radar_provider_manager",
                                "{site}: provider switch {from} -> {provider}: {reason}; {mode}"
                            );
                            self.views[idx].live_scan.set_switch_reason(reason);
                            // Keep latency distributions tied to one source. In-flight uploads
                            // retain their old Arc and cannot enter the replacement's samples.
                            self.views[idx].live_render_started = None;
                            self.views[idx].live_queue_timings =
                                Arc::new(crate::render::LiveQueueTimings::default());
                        }
                    }
                }
                let gen = self.live_gen.load(std::sync::atomic::Ordering::Relaxed);
                self.spawn_stream(idx, site.clone(), base, ctx.clone(), gen);
                self.live_stream = Some((idx, site, gen, desired_label));
                self.views[idx].live_scan.stream_started(
                    provider,
                    desired_label.is_some_and(|label| label != "Unidata Level II (AWS S3)"),
                );
                #[cfg(not(target_arch = "wasm32"))]
                let completed_only =
                    desired_label == Some(crate::radar_provider_manager::DEGRADED_LABEL);
                #[cfg(target_arch = "wasm32")]
                let completed_only = false;
                self.views[idx]
                    .live_scan
                    .set_source_mode(if completed_only {
                        crate::live_scan::SourceMode::CompletedVolumes
                    } else {
                        crate::live_scan::SourceMode::ProgressiveRadials
                    });
            }
        }
    }

    /// Create, refresh or drop this pane's [`crate::radar_provider_manager::SiteProviders`] to
    /// match the current site and settings, then advance its arbiter one tick. Called every frame
    /// from `manage_stream`, same as that function's own site-change bookkeeping — a couple of
    /// `HashMap` lookups and pure arithmetic, not worth throttling separately.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn sync_radar_providers(
        &mut self,
        idx: usize,
        want: bool,
        site: Option<&str>,
        base: Option<&Arc<Scan>>,
    ) {
        use crate::radar_provider_manager::SiteProviders;

        let relay_url = (!self.settings.radar_relay_url.trim().is_empty())
            .then(|| self.settings.radar_relay_url.trim().to_string());
        let override_tier = match self.settings.radar_provider_override {
            crate::settings::RadarProviderOverride::Auto => None,
            crate::settings::RadarProviderOverride::Primary => {
                Some(crate::radar_provider_manager::SelectedTier::Primary)
            }
            crate::settings::RadarProviderOverride::Backup => {
                Some(crate::radar_provider_manager::SelectedTier::Backup)
            }
            crate::settings::RadarProviderOverride::Degraded => {
                Some(crate::radar_provider_manager::SelectedTier::Degraded)
            }
        };

        let spawner = self.spawner.clone();
        let v = &mut self.views[idx];
        if !want {
            v.radar_providers = None; // drops it, which stops its monitor tasks (see its `Drop`)
            return;
        }
        let (Some(site), Some(base)) = (site, base) else {
            v.radar_providers = None;
            return;
        };
        let stale = match &v.radar_providers {
            Some(p) => p.site() != site || p.relay_url() != relay_url.as_deref(),
            None => true,
        };
        if stale {
            v.radar_providers = Some(SiteProviders::start(
                &spawner,
                site.to_string(),
                relay_url,
                Arc::clone(base),
            ));
        }
        if let Some(p) = v.radar_providers.as_mut() {
            match override_tier {
                Some(tier) => p.set_manual_override(tier),
                None => p.clear_manual_override(),
            }
            p.tick();
        }
    }

    /// Spawn the live chunk streamer for `site`, routing merged volumes back to `view_idx`.
    ///
    /// `gen` is the generation this stream belongs to; it ends itself once the app has moved on.
    pub(crate) fn spawn_stream(
        &self,
        view_idx: usize,
        site: String,
        base: Arc<Scan>,
        ctx: egui::Context,
        gen: u64,
    ) {
        let tx = self.msg_tx.clone();
        let live_gen = Arc::clone(&self.live_gen);
        // Read again when the stream ends, to tell a stream the app stopped (a new generation, a
        // hidden tab or a backgrounded app) from one that was lost while still wanted.
        let end_gen = Arc::clone(&self.live_gen);
        let active = move || {
            live_gen.load(std::sync::atomic::Ordering::Relaxed) == gen
                && crate::platform::activity::is_active()
        };
        // ROADMAP_NEW B6.11 step 11: subscribe with whichever tier this pane's failover manager
        // currently selects (Unidata primary, the relay backup, or the NOAA TGFTP degraded
        // fallback) instead of always the hardcoded Unidata path. No manager (not native, or one
        // hasn't been created for this site yet) falls back to exactly the old behavior.
        #[cfg(not(target_arch = "wasm32"))]
        let provider: Arc<dyn crate::volume::Level2LiveProvider + Send + Sync> = self.views
            [view_idx]
            .radar_providers
            .as_ref()
            .map(|p| p.provider())
            .unwrap_or_else(|| Arc::new(crate::volume::UnidataLevel2Provider));
        self.spawner.spawn(async move {
            #[cfg(target_arch = "wasm32")]
            use crate::volume::{Level2LiveProvider, UnidataLevel2Provider};
            let end_site = site.clone();
            let cb_tx = tx.clone();
            let cb_ctx = ctx.clone();
            let cb_site = site.clone();
            let progress_tx = tx.clone();
            let progress_ctx = ctx.clone();
            let progress_site = site.clone();
            let active: Box<dyn Fn() -> bool + Send + Sync> = Box::new(active);
            let on_update: Box<dyn FnMut(wxdata::live::Update) + Send> = Box::new(move |u| {
                let _ = cb_tx.send(DataMsg::Live {
                    view: view_idx,
                    site: cb_site.clone(),
                    gen,
                    name: u.name,
                    time: u.time,
                    received_at: u.received_at,
                    radial_coverage: u.radial_coverage,
                    scan: u.scan,
                    changed: u.changed,
                    retries: u.retries,
                    decode_time: u.decode_time,
                });
                cb_ctx.request_repaint();
            });
            let on_progress: Box<dyn FnMut(wxdata::live::ScanProgress) + Send> =
                Box::new(move |progress| {
                    let _ = progress_tx.send(DataMsg::LiveProgress {
                        view: view_idx,
                        site: progress_site.clone(),
                        gen,
                        progress,
                    });
                    progress_ctx.request_repaint();
                });
            #[cfg(not(target_arch = "wasm32"))]
            log::info!(
                "live stream started for {end_site} via {}",
                provider.label()
            );
            #[cfg(target_arch = "wasm32")]
            log::info!("live stream started for {end_site}");
            #[cfg(not(target_arch = "wasm32"))]
            let res = provider
                .subscribe(site, base, active, on_update, on_progress)
                .await;
            #[cfg(target_arch = "wasm32")]
            let res = UnidataLevel2Provider
                .subscribe(site, base, active, on_update, on_progress)
                .await;
            if let Err(e) = &res {
                log::warn!("live stream for {end_site} ended: {e}");
            }
            // Still wanted when it ended means lost (an error, or the relay closing on us);
            // otherwise the app stopped it, which is not a recovery (`LiveScan::stream_stopped`).
            let lost = end_gen.load(std::sync::atomic::Ordering::Relaxed) == gen
                && crate::platform::activity::is_active();
            let _ = tx.send(DataMsg::LiveEnded {
                view: view_idx,
                site: end_site,
                gen,
                lost,
            });
            ctx.request_repaint();
        });
    }
}

#[cfg(test)]
mod poll_tests {
    use super::{poll_may_start, POLL_STUCK_AFTER};
    use std::time::Duration;

    #[test]
    fn a_hung_poll_cannot_stop_polling_for_good() {
        let every = Duration::from_secs(30);
        // Idle and due: poll. Idle, not due: wait. Site changed: poll now.
        assert!(poll_may_start(
            false,
            Some(Duration::from_secs(31)),
            every,
            false
        ));
        assert!(!poll_may_start(
            false,
            Some(Duration::from_secs(5)),
            every,
            false
        ));
        assert!(poll_may_start(
            false,
            Some(Duration::from_secs(5)),
            every,
            true
        ));
        assert!(poll_may_start(false, None, every, false));
        // One in flight: wait, however due, until it has outlived every deadline.
        assert!(!poll_may_start(
            true,
            Some(Duration::from_secs(60)),
            every,
            false
        ));
        assert!(poll_may_start(true, Some(POLL_STUCK_AFTER), every, false));
    }
}
