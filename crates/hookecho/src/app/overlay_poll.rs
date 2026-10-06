//! Delivery of background overlay fetches: each OverlayMsg folded into the app's state.
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

/// How long a SCIT ID's trend samples outlive the ID leaving the table, so a storm that took a
/// new ID keeps its earlier samples (two hours; `CELL_TREND_MAX` volumes is a little more).
const CELL_TREND_KEEP_S: i64 = 2 * 3600;

impl HookEchoApp {
    pub(crate) fn poll_overlays(&mut self) {
        crate::prof_scope!("poll_overlays");
        let mut changed = false;
        while let Ok(delivery) = self.overlay_rx.try_recv() {
            let mut model_context = None;
            let msg = match delivery {
                OverlayDelivery::Immediate(msg) => msg,
                OverlayDelivery::Fetched {
                    lane,
                    generation,
                    model_request,
                    mrms_context,
                    mut result,
                } => {
                    if matches!(&lane, RequestLane::Mrms(context) if Some(*context) != mrms_context)
                    {
                        self.acquisition.discard(&lane, generation);
                        continue;
                    }
                    if let Some(request) = model_request {
                        if lane != RequestLane::Model(request)
                            || !self.wanted_model_requests().contains(&request)
                            || self
                                .model_fields
                                .get(&request)
                                .is_none_or(|slot| slot.state.model_requested != Some(request))
                        {
                            self.acquisition.discard(&lane, generation);
                            continue;
                        }
                        if result
                            .as_ref()
                            .is_ok_and(|msg| !request.accepts_message(msg))
                        {
                            result = Err("Model reply does not match its requested source, product, run and lead".into());
                        }
                        model_context = Some(request);
                    }
                    if let Some(context) = mrms_context {
                        if lane != RequestLane::Mrms(context)
                            || !self.wanted_mrms_contexts().contains(&context)
                            || self.mrms_fields.get(&context).is_none()
                        {
                            self.acquisition.discard(&lane, generation);
                            continue;
                        }
                        if result
                            .as_ref()
                            .is_ok_and(|msg| !context.accepts_message(msg))
                        {
                            result = Err("MRMS reply does not match its product, selected analysis or tolerance".into());
                        }
                    }
                    if let RequestLane::Goes(request) = lane {
                        if !self.goes_request_current(request) {
                            self.acquisition.discard(&lane, generation);
                            continue;
                        }
                        if result
                            .as_ref()
                            .is_ok_and(|msg| !request.accepts_message(msg))
                        {
                            result = Err("GOES reply does not match its satellite, sector, product or selected analysis".into());
                        }
                    }
                    if let Ok(OverlayMsg::DerivedFields(delivery)) = &result {
                        if !self.derived_key_current(&delivery.key) {
                            self.acquisition.discard(&lane, generation);
                            continue;
                        }
                    }
                    if let Ok(OverlayMsg::ColumnProduct(delivery)) = &result {
                        if !self.column_delivery_current(delivery) {
                            self.acquisition.discard(&lane, generation);
                            continue;
                        }
                    }
                    let valid_time = result.as_ref().ok().and_then(OverlayMsg::health_valid_time);
                    // Plugin failures deliberately arrive as a message so the placefile manager
                    // can show them, but they are still failures for source health. Treating the
                    // message as a successful cached value would make both the status and cache
                    // residency lie.
                    let embedded_error = result.as_ref().ok().and_then(OverlayMsg::health_error);
                    let health_error = result.as_ref().err().map(String::as_str).or(embedded_error);
                    let current =
                        self.acquisition
                            .finish(&lane, generation, health_error, valid_time);
                    if !current {
                        log::debug!("discarding stale {} reply", lane.label());
                        continue;
                    }
                    match result {
                        Ok(msg) => msg,
                        Err(err) => {
                            use crate::render::FieldLayer as FL;
                            match &lane {
                                RequestLane::Field(FL::ModelDiff) => {
                                    self.diff_valid = None;
                                    self.diff_grid = None;
                                    self.diff_display_key = None;
                                    self.diff_error = Some(err.clone());
                                    if let Some(state) = self.fields.get_mut(&FL::ModelDiff) {
                                        state.pending = None;
                                        state.stamp = None;
                                    }
                                    self.acquisition.set_cache_resident(&lane, false);
                                }
                                RequestLane::Field(FL::Ensemble) => {
                                    self.ensemble_run = None;
                                    self.ensemble_grid = None;
                                    self.ensemble_display_key = None;
                                    self.ensemble_error = Some(err.clone());
                                    if let Some(state) = self.fields.get_mut(&FL::Ensemble) {
                                        state.pending = None;
                                        state.stamp = None;
                                    }
                                    self.acquisition.set_cache_resident(&lane, false);
                                }
                                RequestLane::Field(FL::CompareA) => {
                                    self.compare_valid = None;
                                    self.compare_grid = None;
                                    self.compare_error = Some(err.clone());
                                    for layer in [FL::CompareA, FL::CompareB] {
                                        if let Some(state) = self.fields.get_mut(&layer) {
                                            state.pending = None;
                                            state.stamp = None;
                                        }
                                    }
                                    self.acquisition.set_cache_resident(&lane, false);
                                }
                                _ => {}
                            }
                            note_feed_error(lane.label(), err);
                            continue;
                        }
                    }
                }
            };
            match msg {
                OverlayMsg::AlertSeed(f) => {
                    for id in f
                        .iter()
                        .filter_map(|f| f.alert.as_ref().map(|a| a.dedupe_key()))
                    {
                        self.known_warning_ids.insert(id);
                    }
                    if self.alert_features.is_empty() {
                        self.alert_features = f;
                    }
                }
                OverlayMsg::Alerts(f) => {
                    self.detect_new_warnings(&f);
                    crate::alert_snapshot::save(&f);
                    self.alert_features = f;
                }
                OverlayMsg::Mds(f) => self.md_features = f,
                OverlayMsg::Watches(f) => self.watch_features = f,
                OverlayMsg::Mping(r) => self.mping_reports = r,
                OverlayMsg::Pireps(p) => self.pireps = p,
                OverlayMsg::Recon(o) => self.recon = o,
                OverlayMsg::Ero(day, f) => {
                    if day == self.filters.ero_day {
                        self.ero_features = f;
                    }
                }
                OverlayMsg::FireWx(day, f) => {
                    if day == self.filters.fire_day {
                        self.fire_features = f;
                    }
                }
                OverlayMsg::Wssi(day, f) => {
                    // A day change in flight must not overwrite the day now selected.
                    if day == self.filters.wssi_day {
                        self.wssi_features = f;
                    }
                }
                OverlayMsg::Outlook(day, f) => {
                    if (1..=3).contains(&day) {
                        self.outlook_features[(day - 1) as usize] = f;
                    }
                }
                OverlayMsg::Cells(site, cells, past) => {
                    // Keep only if still the active site.
                    if self.views[self.active].site.as_deref() == Some(site.as_str()) {
                        // Reset trend history on a site change; append this volume's samples.
                        if self.cells_site.as_deref() != Some(site.as_str()) {
                            self.cell_trends.clear();
                        }
                        // The same score the storm-cells table ranks by, from the same cached
                        // couplets, so the trend line and the table's number never disagree. Read
                        // from the cache only: a cell-product arrival must not kick off a
                        // rotation detection pass of its own.
                        let couplets: &[wxdata::rotation::CoupletHit] = match &self.couplet_cache {
                            Some((_, (hits, ..))) => hits,
                            None => &[],
                        };
                        let scores: Vec<u8> =
                            wxdata::cellscore::score_all(&cells, &self.probsevere, couplets);
                        for (c, &score) in cells.iter().zip(&scores) {
                            if c.id.is_empty() {
                                continue;
                            }
                            let hist = self.cell_trends.entry(c.id.clone()).or_default();
                            let sample = ui::cell_window::CellSample {
                                vil: c.vil,
                                top: c.top_kft,
                                dbz: c.max_dbz,
                                severity: Some(score),
                                time: c.time,
                                dbz_hgt: c.max_dbz_hgt_kft,
                            };
                            // Skip a duplicate of the last sample (same volume re-fetched).
                            if hist.last().is_none_or(|s| {
                                (s.vil, s.top, s.dbz, s.severity)
                                    != (sample.vil, sample.top, sample.dbz, sample.severity)
                            }) {
                                hist.push(sample);
                                if hist.len() > CELL_TREND_MAX {
                                    hist.remove(0);
                                }
                            }
                        }
                        // Cell ids churn every volume, and the map only ever grew — an entry
                        // per cell the radar has ever named, for as long as the site is the same.
                        // Keep the ones this volume still has, and those sampled within the last
                        // two hours: a storm that took a new ID keeps its samples under the old
                        // one, read back through its history (`StormIdentity::trend`).
                        let newest = cells.iter().filter_map(|c| c.time).max();
                        self.cell_trends.retain(|id, hist| {
                            cells.iter().any(|c| &c.id == id)
                                || hist.last().and_then(|s| s.time).zip(newest).is_some_and(
                                    |(t, n)| (n - t).num_seconds() <= CELL_TREND_KEEP_S,
                                )
                        });
                        if !past.is_empty() {
                            merge_cell_history(&mut self.cell_trends, &past);
                            self.cells_history_site = Some(site.clone());
                        }
                        // The storm history first, so following and selection read this table
                        // through it (`StormIdentity::resolve`).
                        self.dock.storm_ids.feed(Some(site.as_str()), &cells);
                        self.storm_cells = cells;
                        self.cells_site = Some(site);
                        self.update_follow();
                    }
                }
                OverlayMsg::Placefile(url, pf) => {
                    if let Some(lp) = self.placefiles.iter_mut().find(|lp| lp.url == url) {
                        lp.pf = pf;
                        lp.loaded = true;
                        lp.error = None;
                        lp.last_fetch = Some(Instant::now());
                        self.overlay_gen = self.overlay_gen.wrapping_add(1);
                    }
                }
                OverlayMsg::PlacefileError(url, err) => {
                    log::warn!("{url}: {err}");
                    if let Some(lp) = self.placefiles.iter_mut().find(|lp| lp.url == url) {
                        lp.error = Some(err);
                        lp.last_fetch = Some(Instant::now());
                    }
                }
                OverlayMsg::Field(layer, field) => {
                    // GOES grids must arrive with their original immutable request.
                    if !goes_context::is_goes(layer) {
                        self.accept_field(layer, field, None);
                    }
                }
                OverlayMsg::GoesFootprint(..) => {}
                OverlayMsg::GoesField(request, field) => {
                    if self.goes_request_current(request) && request.accepts_field(&field) {
                        let layer = request.layer.unwrap();
                        if self
                            .goes_fields
                            .get_mut(&layer)
                            .is_some_and(|slot| slot.accept(request))
                        {
                            if request.sector.is_meso() {
                                self.goes_footprint = Some((
                                    request.footprint_request(),
                                    wxdata::goes_abi::Footprint::of(&field.data),
                                ));
                            }
                            self.accept_field(layer, field.data, Some(field.stamp));
                        }
                    }
                }
                OverlayMsg::GoesFootprintFor(request, fp) => {
                    if self.goes_request_current(request)
                        && request.accepts_time(fp.time)
                        && self.goes_footprint_slot.accept(request)
                    {
                        self.goes_footprint = Some((request, fp));
                    }
                }
                OverlayMsg::GlmWindow(end, flashes) => {
                    self.glm_archive = Some((end, flashes));
                    // The density grid is rebuilt from these now, not on its next tick.
                    self.glm_fed_last = None;
                }
                OverlayMsg::StampedField(layer, field) => {
                    if let Some(request) = model_context {
                        let upload = self.field_upload(layer, &field.data);
                        if let Some(slot) = self.model_fields.get_mut(&request) {
                            if slot.state.stage_model(request, field, upload) {
                                for idx in 0..self.views.len() {
                                    if self.views[idx].fields_on.contains(&layer)
                                        && self.selected_model_request_for(idx, layer)
                                            == Some(request)
                                    {
                                        self.views[idx].last_model_fields.insert(layer, request);
                                    }
                                }
                            }
                        }
                    } else if self.selected_model_request(layer).is_none() {
                        self.accept_field(layer, field.data, Some(field.stamp));
                    }
                }
                OverlayMsg::DerivedFields(delivery) => self.accept_derived_fields(*delivery),
                OverlayMsg::ColumnProduct(delivery) => self.accept_column_product(*delivery),
                OverlayMsg::ColumnTrail(delivery) => self.accept_column_trail(*delivery),
                OverlayMsg::MrmsField(layer, field, request) => {
                    // The field's original request resolves to a currently wanted immutable slot.
                    if let Some(context) = self
                        .wanted_mrms_contexts()
                        .into_iter()
                        .find(|context| context.layer == layer && context.request() == request)
                    {
                        self.accept_mrms_field(context, field);
                    }
                }
                OverlayMsg::ModelDiff(kind, fh, field, pct, valid)
                    if kind == self.diff_field && fh == self.comparison_fcst_hour =>
                {
                    let layer = crate::render::FieldLayer::ModelDiff;
                    self.diff_pct = pct;
                    let upload = self.diff_upload(&field);
                    if let Some(s) = self.fields.get_mut(&layer) {
                        s.pending = Some(upload);
                        let (a, b) = self.diff_field.pair();
                        s.stamp = Some(field_state::model_stamp(
                            &self.diff_mode.expression(a, b),
                            self.diff_field.slug(),
                            &field,
                            None,
                            true,
                        ));
                    }
                    self.diff_valid = Some(valid);
                    self.diff_error = None;
                    self.diff_grid = Some(field);
                    self.diff_display_key = Some((self.diff_field, self.diff_mode));
                }
                OverlayMsg::ModelDiff(..) => {}
                OverlayMsg::Compare(field, fh, a, b, valid) => {
                    // A selection change in flight must not overwrite the field now selected.
                    if field == self.diff_field && fh == self.comparison_fcst_hour {
                        use crate::render::FieldLayer as FL;
                        let source = field.source_layer();
                        let upload_a = self.field_upload(source, &a);
                        let upload_b = self.field_upload(source, &b);
                        let (model_a, model_b) = field.pair();
                        if let Some(s) = self.fields.get_mut(&FL::CompareA) {
                            s.pending = Some(upload_a);
                            s.stamp = Some(field_state::model_stamp(
                                model_a,
                                field.slug(),
                                &a,
                                Some(valid.a_run),
                                false,
                            ));
                        }
                        if let Some(s) = self.fields.get_mut(&FL::CompareB) {
                            s.pending = Some(upload_b);
                            s.stamp = Some(field_state::model_stamp(
                                model_b,
                                field.slug(),
                                &b,
                                Some(valid.b_run),
                                false,
                            ));
                        }
                        self.compare_valid = Some(valid);
                        self.compare_error = None;
                        self.compare_grid = Some((a, b));
                    }
                }
                OverlayMsg::Ensemble(field, fh, run) => {
                    // A selection change in flight must not overwrite the field now selected.
                    if field == self.ensemble.field && fh == self.ensemble_lead_hour() {
                        self.ensemble_run = Some(*run);
                        self.ensemble_error = None;
                        self.ensemble_display_key = None;
                        self.rebuild_ensemble_display();
                    }
                }
                OverlayMsg::StormReports(bucket, reports) => match bucket {
                    None => self.storm_reports = reports,
                    Some(b) => {
                        self.arch_lsr.put(b, reports);
                        if self.arch_lsr_inflight == Some(b) {
                            self.arch_lsr_inflight = None;
                        }
                    }
                },
                OverlayMsg::Aviation(f) => self.aviation_features = f,
                OverlayMsg::Tfr(new, remaining) => {
                    self.tfr_features.extend(new);
                    self.tfr_pending = remaining;
                }
                OverlayMsg::Spotters(spotters) => self.spotters = spotters,
                OverlayMsg::Fronts(a) => self.fronts = Some(a),
                OverlayMsg::FreezingLevels(levels) => self.freezing = Some(*levels),
                OverlayMsg::ProbSevere(f) => {
                    self.evaluate_probsevere_rules(&f);
                    self.probsevere = f;
                }
                OverlayMsg::Obs(site, res) => {
                    // Keep only if still the active site.
                    if self.views[self.active].site.as_deref() == Some(site.as_str()) {
                        self.sensor_data = Some(res);
                        self.sensor_site = Some(site);
                    }
                }
                OverlayMsg::Vwp(site, levels) => {
                    if self.views[self.active].site.as_deref() == Some(site.as_str()) {
                        // A site change starts a new time series; mixing radars on one axis would
                        // be nonsense.
                        if self.hodo_site.as_deref() != Some(site.as_str()) {
                            self.hodo_history.clear();
                        }
                        // The product carries no timestamp, so identical profiles mean "same scan
                        // refetched" — dedupe on content rather than stamping duplicates.
                        let dup = self
                            .hodo_history
                            .back()
                            .is_some_and(|(_, prev)| *prev == levels);
                        if !dup && !levels.is_empty() {
                            self.hodo_history.push_back((Utc::now(), levels.clone()));
                            // ~2 hours at the 5-minute refetch cadence.
                            while self.hodo_history.len() > 24 {
                                self.hodo_history.pop_front();
                            }
                        }
                        self.hodo_data = levels;
                        self.hodo_site = Some(site);
                    }
                }
                OverlayMsg::ArchiveWarnings(bucket, feats) => {
                    self.arch_warns.put(bucket, feats);
                    if self.arch_warn_inflight == Some(bucket) {
                        self.arch_warn_inflight = None;
                    }
                }
                OverlayMsg::ArchiveMds(bucket, feats) => {
                    self.arch_mds.put(bucket, feats);
                    if self.arch_md_inflight == Some(bucket) {
                        self.arch_md_inflight = None;
                    }
                }
                OverlayMsg::Metar(obs, tafs) => {
                    self.metars = obs;
                    self.tafs = tafs;
                }
                OverlayMsg::Webcams(sites) => {
                    // Drop the cached stills with the list they belonged to. Windy's free-tier
                    // image URLs expire after ten minutes, and a kept texture would otherwise
                    // show the same frame until the layer was toggled off and on.
                    self.pf_icon_tex.retain(|k, _| !k.starts_with("cam:"));
                    self.webcams = sites;
                }
                OverlayMsg::Aqi(obs) => self.aqi = obs,
                OverlayMsg::Fires(perims, incidents) => {
                    self.fire_perims = perims;
                    self.fire_incidents = incidents;
                    // Perimeters ride the tessellated overlay layer, so the assembled feature
                    // set has to be rebuilt — bumping the generation alone re-tessellates the
                    // old list and the perimeters never appear.
                    self.rebuild_overlays();
                }
                OverlayMsg::Stations(obs) => self.stations.ingest(obs),
                OverlayMsg::Ppef(p) => self.stations.ppef = Some(p),
                OverlayMsg::DotCams(cams) => self.stations.cams = cams,
                OverlayMsg::Mill(kv) => self.stations.mill_kv_per_m = Some(kv),
                OverlayMsg::Dat(mut points, tracks) => {
                    // Weakest first, so the EF4/EF5 points end up painted on top of the EF0 and
                    // straight-line-wind ones that outnumber them ten to one.
                    points.sort_by_key(|p| wxdata::dat::ef_number(&p.efscale).unwrap_or(0));
                    self.dat_points = points;
                    self.dat_tracks = tracks;
                }
                OverlayMsg::Mosaic(field, sites, oldest) => {
                    self.mosaic_sites = sites;
                    self.mosaic_oldest = Some(oldest);
                    let layer = crate::render::FieldLayer::Mosaic;
                    let upload = self.field_upload(layer, &field);
                    if let Some(s) = self.fields.get_mut(&layer) {
                        s.pending = Some(upload);
                        s.grid = Some(field);
                    }
                }
                OverlayMsg::Gauges(g) => self.gauges = g,
                OverlayMsg::Contours(kind, lines, run, valid, grid) => {
                    self.contours_arrived(kind, lines, run, valid, grid)
                }
                OverlayMsg::Tropical(data) => self.tropical = Some(data),
                OverlayMsg::Outages(f) => self.outage_features = f,
                OverlayMsg::Wind(w) => {
                    self.wind_inflight = None;
                    // Keep only if the selection didn't change while the fetch was in flight, and
                    // radar winds have not taken over the particles meanwhile.
                    if self.wind_fetched == Some((w.level, w.fcst_hour)) && !self.radar_wind.on {
                        self.wind = Some(*w);
                    }
                }
            }
            changed = true;
        }
        if changed {
            // One rebuild covers every message kind (ProbSevere/Tropical included).
            self.rebuild_overlays();
        }
    }
}
