//! The frame's fetch scheduling: each feed, overlay, field and model layer asks whether its
//! cadence, the view or a setting says it is due, and starts a fetch if so. Moved out of
//! `ui_frame` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    pub(crate) fn schedule_fetches(&mut self, ctx: &egui::Context) {
        // Surface obs (METAR station plots).
        self.sync_metar(ctx);
        self.sync_webcams(ctx);
        self.sync_fires(ctx);
        self.sync_aqi(ctx);
        self.sync_stations(ctx);
        self.sync_dat(ctx);
        self.sync_mosaic(ctx);
        // River flood gauges (NWPS).
        self.sync_gauges(ctx);
        // HRRR model contours.
        self.sync_contours(ctx);
        // Hurricane-hunter observations: a mission transmits every 30 s, so 10 min is plenty.
        if self.show_recon
            && self
                .recon_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 600)
        {
            self.recon_last_fetch = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::Recon);
        }
        // County outages: ODIN's upstream updates about every 15 min; 5 min keeps a fast-moving
        // event current without asking much of a free, keyless API.
        if self.show_outages
            && self
                .outages_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 300)
        {
            self.outages_last_fetch = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::Outages);
        }
        // Tropical model guidance: its own half-hourly clock, only while it is on.
        self.spaghetti.poll();
        if self.show_tropical && self.spaghetti.due() {
            let (rt, http) = (self.spawner.clone(), self.http.clone());
            self.spaghetti.fetch(&rt, &http, ctx);
        }
        // NHC tropical suite: refresh every 15 min while enabled.
        if self.show_tropical
            && self
                .tropical_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 900)
        {
            self.tropical_last_fetch = Some(Instant::now());
            self.spawn_overlay(
                ctx,
                OverlaySource::Tropical(self.tropical_wind_kt, self.tropical_surge),
            );
        }
        // Periodic overlay refresh (~2 min), honoring live weather cadence. Skipped entirely
        // while backgrounded — see `platform::activity`.
        let overlay_secs = if crate::platform::is_metered() {
            240
        } else {
            120
        };
        // On the web the first pass through here *is* the boot fetch (App::new skips it), so hold
        // it until there is radar on screen — or five seconds have gone by and the radar is
        // evidently not coming, in which case the overlays are the only thing left to draw.
        let overlays_may_start = !cfg!(target_arch = "wasm32")
            || self.views[self.active].volume.is_some()
            || self.boot_at.elapsed().as_secs() >= 5;
        if overlays_may_start
            && crate::platform::activity::is_active()
            && self
                .overlay_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= overlay_secs)
        {
            self.fetch_overlays(ctx);
        }
        // MRMS national mosaic: fetch when enabled, refresh at the ~2-min product cadence.
        // National field layers: fetch each enabled layer at its product cadence.
        use crate::render::FieldLayer as FL;
        for layer in FL::DRAW_ORDER {
            // Layers with a fetch block of their own answer `None` and are skipped here.
            let Some(request) = self.mrms_request(layer) else {
                continue;
            };
            // The reflectivity tint reads the precipitation-type grid whether or not that
            // layer is being drawn, so wanting the tint counts as wanting the layer's data.
            let wanted =
                self.field_wanted(layer) || (layer == FL::PrecipType && self.settings.precip_tint);
            let selection_changed = self
                .fields
                .get(&layer)
                .is_none_or(|s| s.mrms_request.as_ref() != Some(&request));
            let stale = wanted
                && self.fields.get(&layer).is_none_or(|s| {
                    selection_changed
                        || s.last_fetch
                            .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            if stale {
                let state = self.fields.entry(layer).or_default();
                if selection_changed {
                    state.pending = None;
                    state.stamp = None;
                    state.mrms_request = Some(request.clone());
                    if layer == FL::PrecipType {
                        self.precip_flag_grid = None;
                        self.precip_flag_gen = self.precip_flag_gen.wrapping_add(1);
                    }
                }
                state.last_fetch = Some(Instant::now());
                self.spawn_overlay(ctx, OverlaySource::Field(layer, request));
            }
        }
        // Snow bands: the mosaic and the precipitation-type grid, cut to the banded snow.
        {
            let layer = FL::SnowBands;
            let stale = self.view_target_time().is_none()
                && self.field_wanted(layer)
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            if stale {
                if let Some(s) = self.fields.get_mut(&layer) {
                    s.last_fetch = Some(Instant::now());
                }
                self.spawn_overlay(ctx, OverlaySource::SnowBands);
            }
        }
        // GOES bands: no forecast hour, no product path — read straight from S3. Which satellite
        // is a setting, not a per-layer choice, so flipping it has to refetch every band at once
        // rather than waiting out the normal cadence.
        let west = self.settings.goes_satellite_west;
        let satellite = if west {
            wxdata::goes_abi::Satellite::West
        } else {
            wxdata::goes_abi::Satellite::East
        };
        // The ABI sector to read: the chosen one, unless it is a mesoscale box that has moved
        // away from the view, which reads CONUS until it covers the view again (ROADMAP_NEW E5).
        let sector = self.goes_sector_now();
        // Scrubbed back (or linked to an archive instant), the frame nearest that time; live, the
        // newest, refreshed on the scan cadence.
        let at = self.view_target_time();
        let slot = goes_slot(at, sector);
        let refresh = |layer| {
            if sector.is_meso() {
                sector.cadence_secs()
            } else {
                field_refresh_secs(layer)
            }
        };
        for layer in [
            FL::GoesIr,
            FL::GoesVisible,
            FL::GoesWaterVapor,
            FL::GoesShortwaveIr,
            FL::GoesMidWaterVapor,
            FL::GoesLowWaterVapor,
            FL::GoesDirtyIr,
            FL::GoesLongwaveIr,
            FL::GoesColdTop,
        ] {
            let on = self.field_wanted(layer);
            // An archive frame never changes: only the newest is refreshed.
            let stale = on
                && at.is_none()
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= refresh(layer))
                });
            let changed = on
                && (self.goes_west_key.get(&layer) != Some(&west)
                    || self.goes_fetched_sector.get(&layer) != Some(&sector)
                    || self.goes_fetched_slot.get(&layer) != Some(&slot));
            if stale || changed {
                self.fields.entry(layer).or_default().last_fetch = Some(Instant::now());
                self.goes_west_key.insert(layer, west);
                self.goes_fetched_sector.insert(layer, sector);
                self.goes_fetched_slot.insert(layer, slot);
                self.spawn_overlay(ctx, OverlaySource::Goes(layer, satellite, sector, at));
            }
        }
        // Falling back to CONUS: keep an eye on where the chosen box goes, so the view gets it
        // back the minute it covers the view again.
        let chosen = self.settings.goes_sector;
        if chosen.is_meso()
            && sector != chosen
            && self.goes_layers_on()
            && self
                .goes_footprint_probe
                .is_none_or(|t| t.elapsed().as_secs() >= chosen.cadence_secs())
        {
            self.goes_footprint_probe = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::GoesFootprint(satellite, chosen, at));
        }
        // GOES channel-difference products: same staleness/satellite-flip rules as the single-band
        // layers above, but a distinct `OverlaySource` variant since each one fetches two bands.
        // Kept as a list with one entry rather than unrolled: a second difference product only
        // needs a new `FieldLayer` here, and the shape matches the multi-layer block above it.
        #[allow(clippy::single_element_loop)]
        for layer in [FL::GoesDustDiff] {
            let on = self.field_wanted(layer);
            let stale = on
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            let changed = on && self.goes_west_key.get(&layer) != Some(&west);
            if stale || changed {
                self.fields.entry(layer).or_default().last_fetch = Some(Instant::now());
                self.goes_west_key.insert(layer, west);
                self.spawn_overlay(ctx, OverlaySource::GoesDiff(layer, satellite));
            }
        }
        // GOES time-difference products: same staleness/satellite-flip rules as the two blocks
        // above, but a distinct `OverlaySource` variant since each one fetches the same band at
        // two different times instead of two bands at the same time. Same reason as above for
        // keeping the one-entry list rather than unrolling it.
        #[allow(clippy::single_element_loop)]
        for layer in [FL::GoesCoolingRate] {
            let on = self.field_wanted(layer);
            let stale = on
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            let changed = on && self.goes_west_key.get(&layer) != Some(&west);
            if stale || changed {
                self.fields.entry(layer).or_default().last_fetch = Some(Instant::now());
                self.goes_west_key.insert(layer, west);
                self.spawn_overlay(ctx, OverlaySource::GoesCoolingRate(layer, satellite));
            }
        }
        // The GOES RGB composite: the same five-minute cadence and satellite flip, and a new
        // recipe refetches at once (its bands differ).
        {
            let layer = FL::GoesRgb;
            let recipe = wxdata::goes_rgb::by_slug(&self.settings.goes_rgb_recipe)
                .unwrap_or(&wxdata::goes_rgb::AIR_MASS);
            let on = self.field_wanted(layer);
            let stale = on
                && at.is_none()
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= refresh(layer))
                });
            let changed = on
                && (self.goes_west_key.get(&layer) != Some(&west)
                    || self.goes_rgb_fetched != Some(recipe.slug)
                    || self.goes_fetched_sector.get(&layer) != Some(&sector)
                    || self.goes_fetched_slot.get(&layer) != Some(&slot));
            if stale || changed {
                self.fields.entry(layer).or_default().last_fetch = Some(Instant::now());
                self.goes_west_key.insert(layer, west);
                self.goes_rgb_fetched = Some(recipe.slug);
                self.goes_fetched_sector.insert(layer, sector);
                self.goes_fetched_slot.insert(layer, slot);
                self.spawn_overlay(ctx, OverlaySource::GoesRgb(recipe, satellite, sector, at));
            }
        }
        // NDFD elements: also no forecast hour to scrub — each fetch is the whole short-range
        // bundle and this always shows the message valid nearest to now.
        for layer in [
            FL::NdfdTemp2m,
            FL::NdfdWind10m,
            FL::NdfdGust10m,
            FL::NdfdSnow,
        ] {
            let stale = self.field_wanted(layer)
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            if stale {
                self.fields.entry(layer).or_default().last_fetch = Some(Instant::now());
                self.spawn_overlay(ctx, OverlaySource::Ndfd(layer));
            }
        }
        // RTMA analysis: a new hour posts about 45 minutes after it. Naming an analysis hour in the
        // browser refetches at once; the pin only applies while RTMA is the selected model, so a
        // run picked for the HRRR is never read as an analysis hour.
        for layer in [
            FL::RtmaTemp2m,
            FL::RtmaDewpoint2m,
            FL::RtmaWind10m,
            FL::RtmaGust10m,
            FL::RtmaVisibility,
            FL::RtmaCeiling,
            FL::RtmaMslp,
            FL::RtmaPrecip1h,
        ] {
            let hour = if self.model_sel.model == crate::model_browser::BModel::Rtma {
                self.model_run
            } else {
                None
            };
            let on = self.field_wanted(layer);
            let changed = on && self.rtma_key.get(&layer) != Some(&hour);
            let stale = on
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            if stale || changed {
                self.fields.entry(layer).or_default().last_fetch = Some(Instant::now());
                self.rtma_key.insert(layer, hour);
                self.spawn_overlay(ctx, OverlaySource::Rtma(layer, hour));
            }
        }
        // Environment suite (CAPE/SRH): the model browser's model at the scrubbed forecast hour.
        // Changing the model or the hour refetches now rather than on the slow cadence.
        for layer in [FL::Cape, FL::Srh] {
            let run = self.pinned_regional_run(self.env_model);
            let key = (self.env_model, self.hrrr_fcst_hour, run);
            let changed = self.field_wanted(layer) && self.env_fetch_key.get(&layer) != Some(&key);
            let stale = self.field_wanted(layer)
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            if stale || changed {
                if let Some(s) = self.fields.get_mut(&layer) {
                    s.last_fetch = Some(Instant::now());
                }
                self.env_fetch_key.insert(layer, key);
                self.spawn_overlay(
                    ctx,
                    OverlaySource::Env(
                        layer,
                        self.env_model,
                        self.env_cape_ml,
                        self.env_srh_km,
                        self.hrrr_fcst_hour,
                        run,
                    ),
                );
            }
        }
        // Global models: whichever source and forecast hour the user picked.
        for (layer, gfield) in [
            (FL::GlobalMslp, wxdata::global::GlobalField::Mslp),
            (FL::GlobalHeight500, wxdata::global::GlobalField::Height500),
            (FL::GlobalTemp2m, wxdata::global::GlobalField::Temp2m),
            (
                FL::GlobalDewpoint2m,
                wxdata::global::GlobalField::Dewpoint2m,
            ),
            (FL::GlobalWind10m, wxdata::global::GlobalField::Wind10m),
            (FL::GlobalPrecip, wxdata::global::GlobalField::Precip),
        ] {
            let fh = self.global_fcst_hour;
            let model = self.global_model;
            let on = self.field_wanted(layer);
            let stale = on
                && self.fields.get(&layer).is_some_and(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            // Changing the source or the hour has to refetch now, not on the next slow cadence.
            let run = self.pinned_global_run();
            let changed = on && self.global_layer_key.get(&layer) != Some(&(model, fh, run));
            if stale || changed {
                if let Some(s) = self.fields.get_mut(&layer) {
                    s.last_fetch = Some(Instant::now());
                }
                self.global_layer_key.insert(layer, (model, fh, run));
                self.spawn_overlay(ctx, OverlaySource::Global(layer, model, gfield, fh, run));
            }
        }
        // Model difference: same cadence as a global layer, and the same refetch-on-change rule.
        {
            let layer = FL::ModelDiff;
            let fh = self.global_fcst_hour;
            let on = self.field_wanted(layer);
            let stale = on
                && self.fields.get(&layer).is_some_and(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            let changed = on && self.diff_key != Some((self.diff_field, fh));
            // A field with no meaningful ratio falls back from the percent view.
            if !self.diff_mode.offered_for(self.diff_field) {
                self.diff_mode = crate::fielddiff::DiffMode::Signed;
            }
            let display_key = (self.diff_field, self.diff_mode);
            // Remembered across restarts; written only when it changes.
            if self.settings.compare_view != Some(display_key) {
                self.settings.compare_view = Some(display_key);
            }
            let display_changed = on
                && !changed
                && self.diff_display_key != Some(display_key)
                && self.diff_valid.is_some();
            if display_changed {
                if let Some(grid) = self.diff_grid.clone() {
                    let upload = self.diff_upload(&grid);
                    if let Some(state) = self.fields.get_mut(&layer) {
                        state.pending = Some(upload);
                        if let Some(stamp) = state.stamp.as_mut() {
                            let (a, b) = self.diff_field.pair();
                            stamp.source_id = self.diff_mode.expression(a, b);
                        }
                    }
                    self.diff_display_key = Some(display_key);
                }
            }
            if stale || changed {
                if changed {
                    self.diff_valid = None;
                    self.diff_grid = None;
                    self.diff_pct = None;
                    self.diff_display_key = None;
                    if let Some(s) = self.fields.get_mut(&layer) {
                        s.stamp = None;
                    }
                }
                self.diff_error = None;
                if let Some(s) = self.fields.get_mut(&layer) {
                    s.last_fetch = Some(Instant::now());
                }
                self.diff_key = Some((self.diff_field, fh));
                self.spawn_overlay(ctx, OverlaySource::ModelDiff(self.diff_field, fh));
            }
        }
        // Ensemble layer: the member fetch is keyed on (field, hour); the statistic and threshold
        // only rebuild the display from the members already held.
        {
            let layer = FL::Ensemble;
            let fh = self.ensemble_lead_hour();
            let on = self.field_wanted(layer);
            let stale = on
                && self.fields.get(&layer).is_some_and(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            let changed = on && self.ensemble_key != Some((self.ensemble.field, fh));
            if on
                && !changed
                && self.ensemble_run.is_some()
                && self.ensemble_display_key != Some(self.ensemble.display_key())
            {
                self.rebuild_ensemble_display();
            }
            if stale || changed {
                if changed {
                    self.ensemble_run = None;
                    self.ensemble_grid = None;
                    self.ensemble_display_key = None;
                    if let Some(s) = self.fields.get_mut(&layer) {
                        s.stamp = None;
                    }
                }
                self.ensemble_error = None;
                if let Some(s) = self.fields.get_mut(&layer) {
                    s.last_fetch = Some(Instant::now());
                }
                self.ensemble_key = Some((self.ensemble.field, fh));
                self.spawn_overlay(ctx, OverlaySource::Ensemble(self.ensemble.field, fh));
            }
        }
        // Model comparison: the same two grids the difference layer fetches, shown side by side
        // instead of subtracted — one fetch feeds both `CompareA`/`CompareB`, so either wanting it
        // is enough to trigger it, and both get the same staleness stamp.
        {
            let fh = self.global_fcst_hour;
            let on = self.field_wanted(FL::CompareA) || self.field_wanted(FL::CompareB);
            let stale = on
                && self.fields.get(&FL::CompareA).is_some_and(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(FL::CompareA))
                });
            let changed = on && self.compare_key != Some((self.diff_field, fh));
            if stale || changed {
                if changed {
                    self.compare_valid = None;
                    self.compare_grid = None;
                    for layer in [FL::CompareA, FL::CompareB] {
                        if let Some(s) = self.fields.get_mut(&layer) {
                            s.stamp = None;
                        }
                    }
                }
                self.compare_error = None;
                for layer in [FL::CompareA, FL::CompareB] {
                    if let Some(s) = self.fields.get_mut(&layer) {
                        s.last_fetch = Some(Instant::now());
                    }
                }
                self.compare_key = Some((self.diff_field, fh));
                self.spawn_overlay(ctx, OverlaySource::Compare(self.diff_field, fh));
            }
        }
        // HRRR rotation tracks + smoke: same forecast-hour scrub as future radar, own cadences.
        for layer in [
            FL::UpdraftHelicity,
            FL::Smoke,
            FL::Snowfall,
            FL::ThunderProb,
        ] {
            let fh = self.hrrr_fcst_hour;
            let stale = self.field_wanted(layer)
                && self.fields.get(&layer).is_none_or(|s| {
                    s.last_fetch
                        .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
                });
            // Scrubbing the forecast tail must refetch immediately, not wait out the cadence.
            // Naming a different run counts as a change for the same reason.
            let run = self.pinned_regional_run(if layer == FL::ThunderProb {
                wxdata::hrrr::Model::Nbm
            } else {
                wxdata::hrrr::Model::Hrrr
            });
            let hour_changed =
                self.field_wanted(layer) && self.hrrr_layer_hour.get(&layer) != Some(&(fh, run));
            if stale || hour_changed {
                if let Some(s) = self.fields.get_mut(&layer) {
                    s.last_fetch = Some(Instant::now());
                }
                self.hrrr_layer_hour.insert(layer, (fh, run));
                self.spawn_overlay(ctx, OverlaySource::HrrrLayer(layer, fh, run));
            }
        }
        // Quiet hours just ended: replay what it held back as one push, so waking up to a silent
        // night does not mean waking up to no idea what happened during it.
        let now_quiet = self.in_quiet_hours();
        if self.was_quiet && !now_quiet {
            let held = match self.quiet_queue.lock() {
                Ok(mut q) => std::mem::take(&mut *q),
                Err(_) => Vec::new(),
            };
            if !held.is_empty() {
                let (title, body) = quiet_summary(&held);
                self.notify_alert(&title, &body, false);
            }
        }
        self.was_quiet = now_quiet;
        // Hold the queue on disk as it changes, not only on a clean exit. A crash or a kill
        // during quiet hours used to lose the whole night's held alerts; the queue is a handful
        // of short strings, so comparing it every tick and writing only on a real change costs
        // nothing worth measuring.
        if let Ok(q) = self.quiet_queue.lock() {
            if *q != self.settings.quiet_pending {
                self.settings.quiet_pending = q.clone();
                drop(q);
                self.settings.save();
            }
        }

        // GOES lightning: granules land every 20 s, so poll about that often. One in flight at a
        // time — a slow fetch must not queue up behind itself.
        let glm_fed_on = self.field_wanted(FL::GlmFed);
        // A lightning-density rule needs the flashes polled and the grid built even with the
        // layer off, exactly like the scan signatures.
        let glm_rule_armed = self.settings.alert_rules.iter().any(|r| {
            r.enabled
                && matches!(
                    r.trigger,
                    crate::settings::RuleTrigger::GlmFed | crate::settings::RuleTrigger::GlmJump
                )
        });
        if (self.show_glm || glm_fed_on || glm_rule_armed)
            && self
                .glm_last_poll
                .is_none_or(|t| t.elapsed().as_secs() >= 20)
            && !self.glm_polling.load(std::sync::atomic::Ordering::Relaxed)
        {
            self.glm_last_poll = Some(Instant::now());
            self.glm_polling
                .store(true, std::sync::atomic::Ordering::Relaxed);
            let feed = self.glm.clone();
            let west = self.settings.glm_goes_west;
            let busy = self.glm_polling.clone();
            let http = self.http.clone();
            let ctx2 = ctx.clone();
            self.spawner.spawn(async move {
                // Decode outside the lock: holding it across an await would stall the painter.
                let mut local = wxdata::glm::GlmFeed::new(15);
                local.set_west(west);
                if let Ok(f) = feed.lock() {
                    local.set_last_keys(f.last_keys().clone());
                }
                let added = local.poll(&http).await.unwrap_or(0);
                if let Ok(mut f) = feed.lock() {
                    f.absorb(local);
                }
                if added > 0 {
                    ctx2.request_repaint();
                }
                busy.store(false, std::sync::atomic::Ordering::Relaxed);
            });
        }

        // Scrubbed back: the flashes of the window ending at the view's time, once per minute of
        // scrubbing (ROADMAP_NEW E6).
        if let Some(target) = self.view_target_time() {
            if (self.show_glm || glm_fed_on) && self.glm_archive_slot != Some(glm_slot(target)) {
                self.glm_archive_slot = Some(glm_slot(target));
                self.spawn_overlay(
                    ctx,
                    OverlaySource::GlmWindow(target, self.settings.glm_goes_west),
                );
            }
        } else {
            self.glm_archive_slot = None;
        }

        // GLM flash-extent density: the same flashes the dots come from, gridded. Cheap enough
        // (one pass over a few thousand points) to do inline on the field-layer cadence rather
        // than spawning for it.
        if (glm_fed_on || glm_rule_armed)
            && self
                .glm_fed_last
                .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(FL::GlmFed))
        {
            self.glm_fed_last = Some(Instant::now());
            if let Some(s) = self.fields.get_mut(&FL::GlmFed) {
                s.last_fetch = Some(Instant::now());
            }
            let target = self.view_target_time();
            let field = self.glm.lock().ok().and_then(|f| {
                let (flashes, end) =
                    glm_flashes_for(target, f.flashes(), self.glm_archive.as_ref(), Utc::now());
                let flashes: std::collections::VecDeque<wxdata::glm::Flash> =
                    flashes.into_iter().copied().collect();
                wxdata::glm::flash_density(
                    &flashes,
                    self.settings.detectors.glm_fed_cell_deg,
                    chrono::Duration::minutes(self.settings.detectors.glm_fed_window_min),
                    end,
                )
            });
            // Alert rules watch what is happening now, never a scrubbed-back view.
            if let Some(field) = field.as_ref().filter(|_| target.is_none()) {
                self.evaluate_grid_rules(crate::settings::RuleTrigger::GlmFed, field);
                // The jump is the difference between this grid and the one before it, so it can
                // only be asked for once there is a previous one — the first grid after launch
                // has no rate.
                if let Some(prev) = &self.glm_fed_prev {
                    if let Some(jump) = wxdata::glm::flash_jump(prev, field) {
                        self.evaluate_grid_rules(crate::settings::RuleTrigger::GlmJump, &jump);
                    }
                }
                self.glm_fed_prev = Some(field.clone());
            }
            if let (Some(field), true) = (field, glm_fed_on) {
                let cap = self.field_texture_cap();
                let _ = self
                    .overlay_tx
                    .send(OverlayDelivery::Immediate(OverlayMsg::Field(
                        FL::GlmFed,
                        field.decimated(cap),
                    )));
            }
        }

        // Wind particles. HRRR posts hourly, so this gets its own 15-minute clock rather than
        // riding the 120 s overlay block — that would re-download 4.5 MB about thirty times per
        // useful update. Doubled on a metered connection.
        if self.show_wind {
            // Advection timestep, shared by every pane so they stay in step. Clamped because a
            // stalled frame or a resume from background would otherwise teleport the whole field.
            let now = Instant::now();
            self.wind_dt = self
                .wind_last_frame
                .map_or(0.0, |t| now.duration_since(t).as_secs_f32())
                // Headroom above the 100 ms Android cadence, so a normal phone frame is never
                // itself treated as a hitch and quietly slowed down.
                .clamp(0.0, 0.15);
            self.wind_last_frame = Some(now);
            // The app is otherwise idle-driven; this is its first always-on animation, and the
            // cost is not the particle mesh — it is re-rendering the whole map (radar warp,
            // vector basemap) every frame instead of sitting idle. Measured on an S24 Ultra:
            // 7% CPU idle, 78% animating at 20 fps, so the cadence is the battery knob. 10 fps on
            // a phone reads fine because the trail is itself the motion blur.
            // An unfocused window animating particles nobody is looking at is the whole cost
            // for none of the value; the idle heartbeat carries it until focus returns, and the
            // `wind_dt` clamp above absorbs the jump.
            if crate::platform::activity::is_active() && ctx.input(|i| i.focused) {
                let ms = if cfg!(target_os = "android") || ui::motion::degraded() {
                    100
                } else {
                    33
                };
                ctx.request_repaint_after(std::time::Duration::from_millis(ms));
            }

            // Panes come and go with the layout; their particle sets should not outlive them.
            self.wind_particles.retain(|k, _| *k < self.views.len());

            let want = (self.wind_level, self.hrrr_fcst_hour);
            let interval = if crate::platform::is_metered() {
                1800
            } else {
                900
            };
            let stale = self
                .wind_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= interval);
            // A level or forecast-hour change refetches at once — but the ~200 ms floor keeps a
            // fast drag across the forecast tail from firing a request per frame.
            let changed = self.wind_fetched != Some(want)
                && self
                    .wind_last_fetch
                    .is_none_or(|t| t.elapsed().as_millis() >= 200);
            // A dropped fetch (spawn_overlay only logs errors) expires rather than wedging.
            let free = self
                .wind_inflight
                .is_none_or(|t| t.elapsed().as_secs() >= 60);
            // Radar winds drive the particles on their own (`sync_radar_wind`).
            if (stale || changed) && free && !self.radar_wind.on {
                self.wind_last_fetch = Some(Instant::now());
                self.wind_inflight = Some(Instant::now());
                self.wind_fetched = Some(want);
                self.spawn_overlay(ctx, OverlaySource::Wind(want.0, want.1));
            }
        }

        // Observed snowfall analysis: its own block because the accumulation window is a knob,
        // and changing it must refetch at once rather than wait out the cadence.
        {
            let on = self.field_wanted(FL::SnowAnalysis);
            let stale = self.fields.get(&FL::SnowAnalysis).is_some_and(|s| {
                s.last_fetch
                    .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(FL::SnowAnalysis))
            });
            let window_changed = self.snow_fetched != Some(self.snow_hours);
            if on && (stale || window_changed) {
                if let Some(s) = self.fields.get_mut(&FL::SnowAnalysis) {
                    s.last_fetch = Some(Instant::now());
                }
                self.snow_fetched = Some(self.snow_hours);
                self.spawn_overlay(ctx, OverlaySource::Snow(self.snow_hours));
            }
        }
        // Crowd precip-type reports. Skipped entirely without a key — the layer is opt-in twice
        // over: you turn it on, and you supply your own mPING key.
        if self.show_mping
            && !self.settings.mping_key.trim().is_empty()
            && self
                .mping_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 300)
        {
            self.mping_last_fetch = Some(Instant::now());
            let key = self.settings.mping_key.trim().to_string();
            self.spawn_overlay(ctx, OverlaySource::Mping(key));
        }
        // Surface analysis: WPC reissues it a few times an hour.
        if self.show_fronts
            && self
                .fronts_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 1800)
        {
            self.fronts_last_fetch = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::Fronts);
        }
        // Locally derived products: no fetch, just a recompute when the volume or threshold moves.
        self.recompute_derived(ctx);
        // Beam-blockage raster: rebuilt when the camera, site, or tilt moves (DEM tiles are cached).
        self.update_blockage(ctx);
        self.update_lowest_tilt(ctx);
        self.update_coverage_compare(ctx);
        // Gridded L3 products (DVL/EET): per-site, refetch on the L3 cadence or a site change.
        let l3_site = self.views[self.active].site.clone();
        let site_changed = self.l3grid_site != l3_site;
        for layer in [FL::Vil, FL::EchoTops, FL::Hca] {
            let on = self.field_wanted(layer);
            if !on {
                continue;
            }
            let stale = self.fields.get(&layer).is_some_and(|s| {
                s.last_fetch
                    .is_none_or(|t| t.elapsed().as_secs() >= field_refresh_secs(layer))
            });
            if let Some(site) = &l3_site {
                if stale || site_changed {
                    if let Some(s) = self.fields.get_mut(&layer) {
                        s.last_fetch = Some(Instant::now());
                    }
                    self.spawn_overlay(ctx, OverlaySource::L3Grid(layer, site.clone()));
                }
            }
        }
        if site_changed
            && [FL::Vil, FL::EchoTops, FL::Hca]
                .iter()
                .any(|l| self.field_wanted(*l))
        {
            self.l3grid_site = l3_site;
        }
        // Forecast reflectivity: fetch when enabled and the model, forecast hour or run changed
        // (~10-min throttle; a new run posts hourly).
        let hrrr_on = self.field_wanted(FL::Hrrr);
        if hrrr_on {
            let stale = self
                .hrrr_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 600);
            // Sub-hourly and hourly are the same layer on one lane; the selected lead is the
            // 15-minute value in sub-hourly mode and the whole-hour value otherwise. Switching
            // modes counts as a change so the tail refetches at the new resolution.
            let run = self.pinned_regional_run(self.refl_model);
            let model_changed = self.hrrr_fetched_key != Some((self.refl_model, run));
            let (changed, source) = if self.hrrr_subhourly {
                (
                    model_changed || self.hrrr_fetched_min != Some(self.hrrr_fcst_min),
                    OverlaySource::HrrrSub(self.hrrr_fcst_min, run),
                )
            } else {
                (
                    model_changed || self.hrrr_fetched_hour != Some(self.hrrr_fcst_hour),
                    OverlaySource::Hrrr(self.refl_model, self.hrrr_fcst_hour, run),
                )
            };
            if changed || stale {
                self.hrrr_fetched_key = Some((self.refl_model, run));
                self.hrrr_fetched_hour = Some(self.hrrr_fcst_hour);
                self.hrrr_fetched_min = Some(self.hrrr_fcst_min);
                self.hrrr_last_fetch = Some(Instant::now());
                self.spawn_overlay(ctx, source);
            }
        }
        // Live LSR refresh (~2-min cadence; the IEM feed is minutes-fresh).
        // The reports layer, or a detector that confirms itself against them.
        if (self.show_storm_reports || self.filters.show_tds || self.filters.show_couplets)
            && self
                .reports_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 120)
        {
            self.reports_last_fetch = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::StormReports(None));
        }
        // Aviation SIGMET/AIRMET refresh (10-min cadence).
        if self.show_aviation
            && self
                .aviation_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 600)
        {
            self.aviation_last_fetch = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::Aviation);
        }
        // TFR refresh. Slow, because a restriction's shape never changes once issued — the
        // cadence is really about noticing new ones. While the first load is still filling in,
        // the next batch is asked for promptly instead.
        if self.show_tfr {
            let due = if self.tfr_pending > 0 { 5 } else { 900 };
            if self
                .tfr_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= due)
            {
                self.tfr_last_fetch = Some(Instant::now());
                let have: Vec<String> = self.tfr_features.keys().cloned().collect();
                self.spawn_overlay(ctx, OverlaySource::Tfr(have));
            }
        }
        // Spotter Network refresh (feed's own 1-min cadence).
        if self.show_spotters
            && self
                .spotters_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 60)
        {
            self.spotters_last_fetch = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::Spotters);
        }
        // ProbSevere refresh (~2-min product cadence).
        if self.show_probsevere
            && self
                .probsevere_last_fetch
                .is_none_or(|t| t.elapsed().as_secs() >= 120)
        {
            self.probsevere_last_fetch = Some(Instant::now());
            self.spawn_overlay(ctx, OverlaySource::ProbSevere);
        }
        // Sensors: fetch when the window is open and the site changed or the 10-min clock elapsed.
        if self.show_sensors {
            if let Some(site) = self.views[self.active].site.clone() {
                let stale = self
                    .sensor_last_fetch
                    .is_none_or(|t| t.elapsed().as_secs() >= 600);
                let site_changed = self.sensor_site.as_deref() != Some(site.as_str());
                if stale || site_changed {
                    if let Some(s) = wxdata::sites::site_by_id(&site) {
                        if site_changed {
                            self.sensor_data = None; // show "loading" until the new site returns
                        }
                        self.sensor_last_fetch = Some(Instant::now());
                        self.spawn_overlay(
                            ctx,
                            OverlaySource::Obs {
                                site: site.clone(),
                                lat: s.latitude as f64,
                                lon: s.longitude as f64,
                            },
                        );
                    }
                }
            }
        }
        // VAD hodograph: fetch when open and the site changed or the 5-min clock elapsed.
        if self.show_hodo {
            if let Some(site) = self.views[self.active].site.clone() {
                let stale = self
                    .hodo_last_fetch
                    .is_none_or(|t| t.elapsed().as_secs() >= 300);
                let site_changed = self.hodo_site.as_deref() != Some(site.as_str());
                if stale || site_changed {
                    if site_changed {
                        self.hodo_data.clear();
                    }
                    self.hodo_last_fetch = Some(Instant::now());
                    self.spawn_overlay(ctx, OverlaySource::Vwp(site));
                }
            }
        }
        self.sync_placefiles(ctx);
        self.sync_pf_icons(ctx);
    }
}

/// Refresh cadence (seconds) for a national field layer's product.
pub(crate) fn field_refresh_secs(layer: crate::render::FieldLayer) -> u64 {
    use crate::render::FieldLayer as FL;
    match layer {
        FL::Lightning | FL::AzShear => 60,
        FL::Mrms | FL::Mesh | FL::Rotation | FL::RotationMidLevel | FL::Hrrr | FL::Mosaic => 120,
        // Same MRMS product cadence as MESH/rotation above.
        FL::Posh
        | FL::Shi
        | FL::MrmsVil
        | FL::MrmsEchoTop18
        | FL::MrmsEchoTop30
        | FL::MrmsEchoTop50
        | FL::MrmsEchoTop60
        | FL::ReflLowestAlt
        | FL::LowLevelReflectivity
        | FL::MrmsRefl0c
        | FL::MrmsReflM5c
        | FL::MrmsReflM10c
        | FL::MrmsReflM15c
        | FL::MrmsReflM20c => 120,
        // QPE accumulations update on a ~2-minute MRMS cadence.
        // The rate product lands every 2 minutes; the accumulations move far more slowly.
        FL::PrecipRate => 120,
        FL::Qpe1h | FL::Qpe3h | FL::Qpe6h | FL::Qpe12h | FL::Qpe24h => 120,
        // MRMS precip type / flash-flood ARI on the ~2-min cadence; L3 grids on the 120 s L3 cadence.
        FL::PrecipType
        | FL::FlashFlood
        | FL::FlashFlood1h
        | FL::FlashFlood3h
        | FL::FlashFlood6h
        | FL::FlashFlood12h
        | FL::FlashFlood24h
        | FL::FlashFloodMax
        | FL::Vil
        | FL::EchoTops
        | FL::Hca => 120,
        // Bands are cut from the ~2-min mosaic, so they are as fresh as it is.
        FL::SnowBands => 120,
        FL::UpdraftHelicity => 600,
        // Snowfall accumulates over a whole model run; it moves as slowly as the run does.
        FL::Snowfall => 600,
        // The analysis is reissued four times a day; half an hour is plenty.
        FL::SnowAnalysis => 1800,
        // Global cycles are six hours apart and take hours to post. Half an hour is generous.
        FL::GlobalMslp
        | FL::GlobalHeight500
        | FL::GlobalTemp2m
        | FL::GlobalDewpoint2m
        | FL::GlobalWind10m
        | FL::GlobalPrecip
        // Two global cycles behind it, so the same half hour.
        | FL::ModelDiff
        | FL::CompareA
        | FL::CompareB
        // GEFS also cycles every six hours; the 31-file fetch is worth doing no more often.
        | FL::Ensemble => 1800,
        FL::Smoke => 900,
        // NBM posts hourly; the blend moves no faster than that.
        FL::ThunderProb => 900,
        // CONUS ABI CMIP lands on S3 about every 5 minutes, whichever band.
        FL::GoesIr
        | FL::GoesVisible
        | FL::GoesWaterVapor
        | FL::GoesShortwaveIr
        | FL::GoesMidWaterVapor
        | FL::GoesLowWaterVapor
        | FL::GoesDirtyIr
        | FL::GoesLongwaveIr
        | FL::GoesDustDiff
        | FL::GoesColdTop
        | FL::GoesCoolingRate
        | FL::GoesRgb => 300,
        // NDFD elements update on a forecaster's schedule, not a fixed clock, and each fetch is
        // a whole multi-day CONUS grid (tens of MB) with no way to ask for just the new part —
        // half an hour balances staying current against re-downloading that for no reason.
        FL::NdfdTemp2m | FL::NdfdWind10m | FL::NdfdGust10m | FL::NdfdSnow => 1800,
        // A new analysis posts hourly, about 45 minutes after its hour; ten minutes catches it
        // soon after it lands without asking constantly.
        FL::RtmaTemp2m
        | FL::RtmaDewpoint2m
        | FL::RtmaWind10m
        | FL::RtmaGust10m
        | FL::RtmaVisibility
        | FL::RtmaCeiling
        | FL::RtmaMslp
        | FL::RtmaPrecip1h => 600,
        // An accumulation moves slower than the grid it accumulates, whatever the window.
        FL::HailSwath => 300,
        // Environment (HRRR CAPE/SRH) refreshes slowly — 15 min.
        FL::Cape | FL::Srh => 900,
        // Derived products cost no network: they recompute when the volume does, not on a clock.
        FL::CompositeLocal
        | FL::VilLocal
        | FL::VilDensity
        | FL::EtopLocal
        | FL::HailMehs
        | FL::HailPosh => 60,
        // Gridded from the GLM feed the app already polls every 20 s; regridding is local work.
        FL::GlmFed => 60,
    }
}
