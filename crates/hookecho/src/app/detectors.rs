//! Radar signature detectors on the active pane's volume: debris (TDS), rotation couplets, hail
//! spikes (TBSS), ZDR columns, local cell tracks and score histories, with their caches, and the
//! alert rules that watch them. Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Run the user's scan rules over one volume's detections.
    ///
    /// Once per volume, not per frame: the same reason `check_rain_arrival` keys on the volume.
    /// Called with every scan detector's hits already computed — including detectors whose layer
    /// is off, which is what makes a rule independent of what happens to be drawn.
    pub(crate) fn evaluate_scan_rules(
        &mut self,
        idx: usize,
        tds: &[wxdata::tds::TdsHit],
        tbss: &[wxdata::dualpol::TbssHit],
        zdr: &[wxdata::dualpol::ZdrColumnHit],
        couplets: &[wxdata::rotation::CoupletHit],
    ) {
        use crate::rules::Detection;
        use crate::settings::RuleTrigger as T;
        if self.settings.alert_rules.iter().all(|r| !r.enabled) {
            return;
        }
        let key = self.volume_key(idx);
        if self.rules_key.as_ref() == Some(&key) {
            return;
        }
        self.rules_key = Some(key);
        // Strengths in the units the rule is written in: knots for rotation, and nothing for the
        // signatures that are their own answer.
        let hits = |t: &T| -> Vec<Detection> {
            match t {
                T::Tds => tds.iter().map(|h| Detection::at(h.lon, h.lat)).collect(),
                T::Tbss => tbss.iter().map(|h| Detection::at(h.lon, h.lat)).collect(),
                T::ZdrColumn => zdr.iter().map(|h| Detection::at(h.lon, h.lat)).collect(),
                T::Rotation => couplets
                    .iter()
                    .map(|h| Detection::with_strength(h.lon, h.lat, h.vrot_ms as f64 * 1.943_844))
                    .collect(),
                _ => Vec::new(),
            }
        };
        // Every scan detector's hits are remembered, not only the armed ones: a compound rule
        // asks about a trigger no rule is armed on all the time ("rotation and also a TDS").
        for t in [T::Tds, T::Tbss, T::ZdrColumn, T::Rotation] {
            let h = hits(&t);
            self.note_hits(&t, &h);
        }
        let recent = self.recent_for_rules();
        for rule in self.settings.alert_rules.clone() {
            if !rule.enabled || !rule.trigger.is_scan() {
                continue;
            }
            // The closest qualifying detection is the one worth naming.
            let hit = hits(&rule.trigger)
                .into_iter()
                .filter(|h| crate::rules::matches(&rule, h, &self.settings))
                .min_by(|a, b| {
                    let d = |h: &Detection| self.distance_from_radar_km(idx, h.lon, h.lat);
                    d(a).total_cmp(&d(b))
                });
            let Some(hit) = hit else { continue };
            if !crate::rules::compound_ok(&rule, &hit, &recent) {
                continue;
            }
            self.fire_rule(&rule, &hit);
        }
    }

    /// Hail spikes (TBSS) for the active pane's lowest tilt, cached per volume like the TDS
    /// detector. Display-only: a spike is context about hail, not a reason to make noise.
    pub(crate) fn compute_tbss(&mut self, idx: usize) -> Vec<wxdata::dualpol::TbssHit> {
        let key = self.tuned_key(idx);
        let core_dbz = self.settings.detectors.tbss_core_dbz;
        if let Some((k, v)) = &self.tbss_cache {
            if *k == key {
                return v.clone();
            }
        }
        let z = self.views[idx]
            .volume
            .as_mut()
            .and_then(|v| v.binned(Moment::Reflectivity, 0, false).ok())
            .cloned();
        let cc = self.views[idx]
            .volume
            .as_mut()
            .and_then(|v| v.binned(Moment::CorrelationCoefficient, 0, false).ok())
            .cloned();
        let out = match (z, cc) {
            (Some(z), Some(cc)) => wxdata::dualpol::tbss(&z, &cc, core_dbz, 20.0, 0.8, 4.0, 150.0),
            _ => Vec::new(),
        };
        self.tbss_cache = Some((key, out.clone()));
        out
    }

    /// ZDR columns and the bright band, both from a full pass over the active pane's tilts.
    ///
    /// The freezing level is the same one the hail grids use (`freezing_for`: the live analysis,
    /// or the observed sounding for an archived volume), so this needs that fetch already done;
    /// without it there is nothing to be "above" and the answer is empty.
    pub(crate) fn compute_zdr_columns(
        &mut self,
        idx: usize,
        ctx: &egui::Context,
    ) -> Vec<wxdata::dualpol::ZdrColumnHit> {
        let key = self.tuned_key(idx);
        let (min_zdr, min_depth) = (
            self.settings.detectors.zdr_min_db,
            self.settings.detectors.zdr_min_depth_km,
        );
        if let Some((k, v, _)) = &self.zdr_cache {
            if *k == key {
                return v.clone();
            }
        }
        // Model heights are above sea level; beam heights are above the radar.
        let site = self.views[idx].site.clone();
        let radar_km = site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
            .map_or(0.0, |s| s.elevation_meters as f64 / 1000.0);
        let h0_km = match self.freezing_for(idx) {
            Some((h0, _)) => h0 / 1000.0 - radar_km,
            None => {
                self.fetch_freezing_levels(ctx, idx);
                return Vec::new();
            }
        };
        let Some(vol) = self.views[idx].volume.as_mut() else {
            return Vec::new();
        };
        let zdr = vol.moment_tilts(Moment::DifferentialReflectivity);
        let z = vol.moment_tilts(Moment::Reflectivity);
        let cc = vol.moment_tilts(Moment::CorrelationCoefficient);
        let hits = wxdata::dualpol::zdr_columns(&zdr, &z, h0_km, min_zdr, min_depth, 40.0, 100.0);
        // The mid tilts are the ones that cut the melting layer at a range where the beam is
        // still narrow enough to mean something.
        let mid = |v: &[wxdata::level2::BinnedSweep]| -> Vec<wxdata::level2::BinnedSweep> {
            v.iter()
                .filter(|s| (2.0..=10.0).contains(&s.elevation_deg))
                .cloned()
                .collect()
        };
        let bb = wxdata::dualpol::bright_band(&mid(&cc), &mid(&z), 6.0);
        self.zdr_cache = Some((key, hits.clone(), bb));
        hits
    }

    /// Auto TDS detection for the active pane's lowest tilt: bin reflectivity + CC and flag debris
    /// signatures (low CC in high Z), then cross-corroborate with rotation. Fires a chime + banner
    /// on the rising edge of a new detection.
    pub(crate) fn compute_tds(&mut self, idx: usize) -> Vec<wxdata::tds::TdsHit> {
        let mut hits = self.tds_raw(idx);
        // Cross-corroborate from each side's own raw evidence: a debris signature beside a
        // couplet, and (symmetrically) a couplet beside a debris signature. `couplets_raw`
        // reads the rotation cache without chiming — a TDS layer must not sound the rotation
        // alarm for rotation the user never asked about, only use it to corroborate its own hits.
        // `rot` itself is discarded once corroborated: what it gained here is not this cache's to
        // keep, or the couplet layer would see an already-boosted couplet and double-count it.
        // `scanned` (velocity tilts read) lets a signature with no couplet beside it be held to
        // `NO_ROTATION_FACTOR` only when there was velocity to find one in.
        let (mut rot, _, scanned) = self.couplets_raw(idx);
        wxdata::tds::cross_corroborate(&mut hits, &mut rot, scanned > 0);
        // By volume name, so a score-timeline sparkline can replay several volumes' worth of
        // history using the exact confidence the marker itself shows, not `tds_raw`'s earlier,
        // pre-corroboration one — see `tds_shown_cache`'s own doc comment.
        if let Some(name) = self.views[idx].volume.as_ref().map(|v| v.name.clone()) {
            self.tds_shown_cache.put(name, hits.clone());
        }

        // No alert of its own: debris is evidence for Tornado detection, which alerts on its verdicts
        // (`Self::tornado_alert`). A user's rule on debris signatures still fires from these hits.

        // The user's confidence threshold is applied on the way out (after corroboration, not
        // baked into the cache), so lowering it shows the hidden hits at once instead of at the
        // next scan.
        let min = self.settings.detectors.tds_min_confidence;
        let evidence = self.confirm_evidence(idx);
        let minute = self.volume_minute(idx);
        let mut shown: Vec<_> = hits
            .into_iter()
            .map(|mut h| {
                h.confirmation = wxdata::confirm::confirm(h.lon, h.lat, minute, &evidence);
                h
            })
            // Human confirmation outranks the radar score, so the slider never hides it.
            .filter(|h| h.confidence >= min || h.confirmation.level().is_some())
            .collect();
        shown.sort_by_key(|h| std::cmp::Reverse(h.confirmation.level()));
        // Whatever the detector makes of a bad sweep, the map is never buried under markers: the
        // strongest and the confirmed are kept.
        shown.truncate(MAX_COUPLET_MARKERS);
        shown
    }

    /// This volume's debris signatures without side effects and never corroborated: the cached
    /// raw hits if this volume has already been scanned, otherwise a fresh (expensive, per-tilt)
    /// detection, cached before returning. Kept raw so the rotation layer reading this to
    /// corroborate its own couplets never sees a confidence corroboration already raised — see
    /// `compute_tds` and `wxdata::tds::cross_corroborate`.
    pub(crate) fn tds_raw(&mut self, idx: usize) -> Vec<wxdata::tds::TdsHit> {
        let key = self.volume_key(idx);
        if let Some((k, v)) = &self.tds_cache {
            if *k == key {
                return v.clone();
            }
        }
        let hits = self.compute_tds_uncached(idx);
        self.tds_cache = Some((key, hits.clone()));
        hits
    }

    /// Same as [`Self::tds_raw`] — the name a caller reads when it specifically wants raw hits
    /// without side effects (i.e. it must not chime), mirroring [`Self::couplets_raw`].
    pub(crate) fn tds_quiet(&mut self, idx: usize) -> Vec<wxdata::tds::TdsHit> {
        self.tds_raw(idx)
    }

    /// Tornado ID for this volume, from the source the settings name: the fusion (default) or the
    /// legacy couplets and debris. `merged` asks for one detection per tornado
    /// ([`wxdata::tornado_id::Circulation`]s, with `couplets` and `tds` tied in) instead of the
    /// separate verdicts.
    pub(crate) fn tornado_identifications(
        &mut self,
        idx: usize,
        ctx: &egui::Context,
        couplets: &[wxdata::rotation::CoupletHit],
        tds: &[wxdata::tds::TdsHit],
        merged: bool,
    ) -> (
        Vec<wxdata::tornado_id::TornadoId>,
        Vec<wxdata::tornado_id::Circulation>,
        wxdata::detection_lineage::DetectionLineage,
    ) {
        use crate::settings::TornadoIdSource;
        use wxdata::detection_lineage::{DetectionLineage, Pipeline};
        // The fusion's verdict once this volume's columns are ready; until then, and for a light
        // loop frame (one tilt), the original's, so the markers never blink out.
        let fused = self.settings.detectors.tornado_id_source == TornadoIdSource::Fusion;
        let environment = if fused {
            self.near_storm_hour(idx, ctx)
        } else {
            None
        };
        let analysed = if fused {
            self.compute_llsd(idx, ctx)
        } else {
            None
        };
        let vol = self.views[idx].volume.as_ref();
        let mut lineage = DetectionLineage {
            pipeline: Pipeline::Original,
            algorithms: wxdata::detection_lineage::original_algorithms(),
            site: self.views[idx].site.clone(),
            volume: vol.map(|v| v.name.clone()).unwrap_or_default(),
            volume_time: vol.map(|v| v.time),
            // The couplets were read from these sweeps; see `detect_couplets`.
            inputs: self
                .couplet_inputs
                .as_ref()
                .filter(|(k, _)| *k == self.volume_key(idx))
                .and_then(|(_, inputs)| inputs.clone()),
            stand_in: None,
        };
        match analysed {
            None => {
                if fused {
                    lineage.stand_in = Some(if vol.is_some_and(|v| v.light) {
                        "a loop frame carries one tilt, and the fused evidence is a column \
                         through several"
                    } else {
                        "the fused verdict for this volume is still being computed"
                    });
                }
                if merged {
                    (
                        Vec::new(),
                        wxdata::tornado_id::circulations(couplets, tds),
                        lineage,
                    )
                } else {
                    (
                        wxdata::tornado_id::identify(couplets, tds),
                        Vec::new(),
                        lineage,
                    )
                }
            }
            Some(analysed) => {
                lineage.pipeline = Pipeline::Fused;
                lineage.algorithms = wxdata::detection_lineage::fused_algorithms();
                lineage.inputs = self.llsd_inputs(idx);
                let evidence = self.confirm_evidence(idx);
                let minute = self.volume_minute(idx);
                let confirm =
                    |lon: f64, lat: f64| wxdata::confirm::confirm(lon, lat, minute, &evidence);
                let options = self.verdict_options(idx, environment.as_deref());
                let likely = self.llsd_tracker.as_ref().map(|t| &t.likely);
                if merged {
                    let mut c = wxdata::llsd_analyst::circulations_with(
                        &analysed, couplets, tds, confirm, options,
                    );
                    if let Some(l) = likely {
                        l.apply(&analysed, c.iter_mut().map(|c| &mut c.id));
                    }
                    (Vec::new(), c, lineage)
                } else {
                    let mut ids = wxdata::llsd_analyst::identify_with(&analysed, confirm, options);
                    if let Some(l) = likely {
                        l.apply(&analysed, ids.iter_mut());
                    }
                    (ids, Vec::new(), lineage)
                }
            }
        }
    }

    /// The same circulations for a volume already computed, read-only (the Cell dock): the
    /// fusion's from its cache when that holds this volume, else the legacy ones.
    pub(crate) fn cached_circulations(
        &self,
        volume_name: &str,
        couplets: &[wxdata::rotation::CoupletHit],
        tds: &[wxdata::tds::TdsHit],
    ) -> Vec<wxdata::tornado_id::Circulation> {
        use crate::settings::TornadoIdSource;
        if self.settings.detectors.tornado_id_source == TornadoIdSource::Fusion {
            if let Some((key, analysed, _)) = &self.llsd_cache {
                if key.1 == volume_name {
                    let idx = key.0;
                    let evidence = self.confirm_evidence(idx);
                    let minute = self.volume_minute(idx);
                    let environment = self.views[idx]
                        .volume
                        .as_ref()
                        .and_then(|v| self.near_storm.hour(near_storm::valid_hour(v.time)));
                    let mut c = wxdata::llsd_analyst::circulations_with(
                        analysed,
                        couplets,
                        tds,
                        |lon, lat| wxdata::confirm::confirm(lon, lat, minute, &evidence),
                        self.verdict_options(idx, environment.as_deref()),
                    );
                    if let Some(t) = &self.llsd_tracker {
                        t.likely.apply(analysed, c.iter_mut().map(|c| &mut c.id));
                    }
                    return c;
                }
            }
        }
        wxdata::tornado_id::circulations(couplets, tds)
    }

    /// The fused pipeline for this volume, analysed (detectionplan.md Phases 12-13), or `None`
    /// while it is still being computed or cannot be: rotation columns from the same four lowest
    /// tilts the couplet detector reads, tracked from the previous volume, with the volume's debris
    /// signatures classified beside them and every column fused. Cached per volume.
    ///
    /// The tracker is fed once per low-level pass ([`wxdata::low_passes`]): under SAILS or MRLE
    /// the lowest tilt is revisited partway through the volume, and each pass is tracked at its
    /// own time, under the volume's upper tilts, so a live tornado is marked at the pass that
    /// shows it rather than when the volume ends. Each pass also counts toward the two passes at
    /// *likely* a verdict needs before it is shown so ([`LlsdTracking::likely`]). A volume's first
    /// pass waits for the four lowest velocity tilts; a pass already fed is not fed again, so the
    /// sweeps of a live volume arriving one by one re-analyse the last pass (its debris and other
    /// tilts) without moving the tracker.
    ///
    /// The columns cost about a third of a second a pass (four LLSD fields and their objects;
    /// the legacy couplets take about 12 ms), so off the browser build they are computed on a
    /// background thread: the first call for a volume starts it and returns `None`, and when it
    /// is done it asks `ctx` for a repaint and the next call tracks, classifies and fuses (well
    /// under a millisecond). Only the newest volume's job is kept. A light loop frame carries one
    /// tilt, and the fusion's evidence is a column through several, so it is not computed for one
    /// (`None`). The tracker starts over when the site changes or a volume comes before the passes
    /// it has seen (stepping back through a loop), so a track is only built forward in time.
    pub(crate) fn compute_llsd(
        &mut self,
        idx: usize,
        ctx: &egui::Context,
    ) -> Option<Vec<wxdata::llsd_analyst::Analysed>> {
        const TILTS: usize = 4;
        let key = self.volume_key(idx);
        if let Some((k, v, _)) = &self.llsd_cache {
            if *k == key {
                return Some(v.clone());
            }
        }
        if self.views[idx].volume.as_ref().is_none_or(|v| v.light) {
            return None;
        }
        let site = self.views[idx].site.clone().unwrap_or_default();
        let (scan, name, time) = {
            let v = self.views[idx].volume.as_ref()?;
            (v.scan.clone(), v.name.clone(), v.time)
        };
        // The passes to feed: those after the last one fed, or all of them on a new track.
        let mut passes = wxdata::low_passes::passes(&scan, time);
        if passes.is_empty() {
            // No velocity timestamps: the volume is one step, at its own time.
            passes.push(wxdata::low_passes::Pass {
                sweep: usize::MAX,
                time,
            });
        }
        let newest = passes.last().map(|p| p.time).unwrap_or(time);
        let tracking = self.llsd_tracker.as_ref().filter(|t| t.site == site);
        let fed = tracking.and_then(|t| t.fed.clone());
        let restart = match &fed {
            None => true,
            // Stepping back: a different volume that begins before the passes already tracked.
            Some((v, at)) => *v != name && passes[0].time.timestamp() <= *at,
        };
        let after = if restart {
            i64::MIN
        } else {
            fed.map_or(i64::MIN, |(_, at)| at)
        };
        let todo: Vec<wxdata::low_passes::Pass> = passes
            .iter()
            .copied()
            .filter(|p| p.time.timestamp() > after)
            .collect();

        let results: Option<LlsdJob> = if todo.is_empty() {
            None
        } else {
            match self.llsd_job.take() {
                Some((k, rx)) if k == key => match rx.try_recv() {
                    Ok(done) => Some(done),
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        self.llsd_job = Some((k, rx));
                        return None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return None,
                },
                // A job for another volume (or none): this one is what is wanted now.
                _ => {
                    let vol = self.views[idx].volume.as_mut()?;
                    let low = vol.elevations.first().copied();
                    let is_low = |s: &wxdata::level2::BinnedSweep| {
                        low.is_some_and(|e| (s.elevation_deg - e).abs() < 0.15)
                    };
                    let velocity: Vec<_> = vol
                        .velocity_tilts_dealiased()
                        .into_iter()
                        .zip(vol.moment_tilts(Moment::Reflectivity))
                        .take(TILTS)
                        .collect();
                    // A volume's first pass waits for the upper tilts its columns are built from;
                    // until they arrive the last pass fed stands, the newest look at low levels.
                    if velocity.len() < TILTS {
                        None
                    } else {
                        // Earlier passes read their own debris; the volume's newest pass reads the
                        // app's (`tds_quiet`), as it always has.
                        let earlier = todo.iter().any(|p| p.time < newest);
                        let (dual_pol, zdr) = if earlier {
                            (
                                vol.moment_tilts(Moment::Reflectivity)
                                    .into_iter()
                                    .zip(vol.moment_tilts(Moment::CorrelationCoefficient))
                                    .take(TILTS)
                                    .collect::<Vec<_>>(),
                                vol.moment_tilts(Moment::DifferentialReflectivity)
                                    .into_iter()
                                    .take(TILTS)
                                    .collect::<Vec<_>>(),
                            )
                        } else {
                            (Vec::new(), Vec::new())
                        };
                        let lowest = wxdata::low_passes::Lowest {
                            velocity: velocity.first().is_some_and(|(v, _)| is_low(v)),
                            dual_pol: dual_pol.first().is_some_and(|(z, _)| is_low(z)),
                            zdr: zdr.first().is_some_and(is_low),
                        };
                        let job = move || {
                            pass_columns(scan, todo, newest, velocity, dual_pol, zdr, lowest)
                        };
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            let (tx, rx) = std::sync::mpsc::channel();
                            let ctx = ctx.clone();
                            std::thread::spawn(move || {
                                let _ = tx.send(job());
                                ctx.request_repaint();
                            });
                            self.llsd_job = Some((key, rx));
                            return None;
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            let _ = ctx;
                            Some(job())
                        }
                    }
                }
            }
        };

        let debris = self.tds_quiet(idx);
        let bar = self.settings.detectors.rotation_only_possible;
        if restart && results.is_some() {
            self.llsd_tracker = Some(LlsdTracking::new(site.clone()));
        }
        let tracking = self.llsd_tracker.as_mut().filter(|t| t.site == site)?;
        let mut inputs = self.llsd_cache.as_ref().and_then(|(_, _, i)| i.clone());
        if let Some((steps, coverage)) = results {
            inputs = coverage;
            for step in steps {
                let tracked = tracking.tracker.update(step.time, step.columns);
                let analysed = wxdata::llsd_analyst::analyse(
                    tracked.clone(),
                    step.debris.as_deref().unwrap_or(&debris),
                    &[],
                );
                tracking.likely.record(&analysed, bar);
                tracking.fed = Some((name.clone(), step.time));
                tracking.last = tracked;
            }
        }
        // The last pass fed, re-analysed with the debris this volume has now.
        let out = wxdata::llsd_analyst::analyse(tracking.last.clone(), &debris, &[]);
        self.llsd_cache = Some((key, out.clone(), inputs));
        Some(out)
    }

    /// When the sweeps behind this volume's fused columns were scanned, once they are computed;
    /// `None` when they are not (yet), or were not recorded.
    pub(crate) fn llsd_inputs(
        &self,
        idx: usize,
    ) -> Option<wxdata::level2::temporal::TemporalCoverage> {
        let key = self.volume_key(idx);
        self.llsd_cache
            .as_ref()
            .filter(|(k, _, _)| *k == key)
            .and_then(|(_, _, inputs)| inputs.clone())
    }

    /// The tornado reports and tornado warnings a detection can be confirmed by right now: the live
    /// ones, or the archived ones while the playhead is off live. See [`wxdata::confirm`].
    pub(crate) fn confirm_evidence(&self, idx: usize) -> wxdata::confirm::Evidence {
        use wxdata::confirm::{is_observed, report_minute, TornadoReport, TornadoWarning};
        let minute = self.volume_minute(idx);
        let warnings = self
            .active_alert_features()
            .iter()
            .filter_map(|f| {
                let a = f.alert.as_ref()?;
                if !a.event.eq_ignore_ascii_case("Tornado Warning") {
                    return None;
                }
                Some(TornadoWarning {
                    rings: f.rings.clone(),
                    observed: is_observed(
                        a.tornado_detection.as_deref(),
                        wxdata::alerts::escalation(a) >= 3,
                    ),
                })
            })
            .collect();
        let reports = self
            .active_storm_reports()
            .iter()
            .filter(|r| r.kind == wxdata::spc::ReportKind::Tornado)
            .filter_map(|r| {
                Some(TornadoReport {
                    lon: r.lon,
                    lat: r.lat,
                    minute: report_minute(&r.time, minute)?,
                })
            })
            .collect();
        wxdata::confirm::Evidence { reports, warnings }
    }

    /// Minutes since the Unix epoch of the pane's displayed volume, the moment its detections are
    /// about. Zero with no volume, when there are no detections to place anyway.
    /// How the fused Tornado ID draws verdicts for pane `idx`: the rotation-only bar from settings,
    /// the wind-turbine mask for the turbines standing in its volume's year, and the environment
    /// gate in `environment`, its HRRR hour ([`Self::near_storm_hour`]).
    pub(crate) fn verdict_options<'a>(
        &self,
        idx: usize,
        environment: Option<&'a wxdata::near_storm::EnvHour>,
    ) -> wxdata::llsd_analyst::VerdictOptions<'a> {
        use chrono::Datelike;
        wxdata::llsd_analyst::VerdictOptions {
            rotation_only_possible: self.settings.detectors.rotation_only_possible,
            turbines_in_year: self.views[idx].volume.as_ref().map(|v| v.time.year()),
            environment,
        }
    }

    pub(crate) fn volume_minute(&self, idx: usize) -> i64 {
        self.views[idx]
            .volume
            .as_ref()
            .map_or(0, |v| v.time.timestamp().div_euclid(60))
    }

    /// The real (expensive) per-tilt gate scan behind [`Self::tds_raw`]: bin reflectivity + CC on
    /// the lowest few tilts, flag debris signatures, and discount by ZDR. No corroboration and no
    /// alerting — see [`Self::compute_tds`], which layers both on afterward.
    pub(crate) fn compute_tds_uncached(&mut self, idx: usize) -> Vec<wxdata::tds::TdsHit> {
        // The lowest few tilts, not just the lowest one: a debris ball that repeats up through
        // them is real vertical evidence a single tilt cannot offer at all (see
        // `tds::detect_volume`). Capped at 4 tilts' worth of gate scanning rather than the whole
        // volume — this only runs once per new volume (cached on `volume_key`), but there is no
        // reason to pay for tilts high enough that lofted-debris relevance has already dropped off.
        const TILTS: usize = 4;
        let Some(vol) = self.views[idx].volume.as_mut() else {
            return Vec::new();
        };
        let z_tilts = vol.moment_tilts(Moment::Reflectivity);
        let cc_tilts = vol.moment_tilts(Moment::CorrelationCoefficient);
        // Differential reflectivity separates debris (near 0 dB) from the rain and large drops
        // that also lower CC; absent on a volume without it, which is then simply not discounted.
        let zdr_tilts = vol.moment_tilts(Moment::DifferentialReflectivity);
        let pairs: Vec<_> = z_tilts.into_iter().zip(cc_tilts).take(TILTS).collect();
        if pairs.is_empty() {
            return Vec::new(); // no dual-pol CC on this volume (legacy pre-dual-pol, or TDWR)
        }
        let mut hits = wxdata::tds::detect_volume(&pairs, 0.80, 40.0, 150.0, 4);
        wxdata::tds::apply_zdr(&mut hits, &zdr_tilts);
        let site = self.views[idx].site.as_deref().unwrap_or("?");
        log::debug!(
            target: "wxdata::tds",
            "{site}: {} tilt(s) scanned, {} debris signature(s)",
            pairs.len(),
            hits.len(),
        );
        hits
    }

    /// Client-side rotation detection for the active pane's lowest tilt: bin the dealiased
    /// velocity sweep and flag gate-to-gate couplets, then cross-corroborate with debris. Fires a
    /// chime + banner on the rising edge, like the TDS detector (they're complementary: rotation
    /// aloft precedes debris at the ground).
    pub(crate) fn compute_couplets(&mut self, idx: usize) -> Vec<wxdata::rotation::CoupletHit> {
        let (mut hits, _, scanned) = self.couplets_raw(idx);
        // Cross-corroborate from each side's own raw evidence (see `compute_tds`'s matching
        // comment). `tds_quiet` reads the debris cache without chiming — the rotation layer must
        // not sound the TDS alarm for debris the user never asked about, only use it to
        // corroborate its own couplets. `debris` itself is discarded once corroborated.
        let mut debris = self.tds_quiet(idx);
        wxdata::tds::cross_corroborate(&mut debris, &mut hits, scanned > 0);
        // By volume name, so a score-timeline sparkline can replay several volumes' worth of
        // history using the exact confidence the marker itself shows — see `rot_shown_cache`'s
        // own doc comment.
        if let Some(name) = self.views[idx].volume.as_ref().map(|v| v.name.clone()) {
            self.rot_shown_cache.put(name, hits.clone());
        }

        let site = self.views[idx].site.clone().unwrap_or_default();
        log::debug!(
            target: "wxdata::rotation",
            "{site}: {} tilt(s) scanned, {} couplet(s)",
            scanned,
            hits.len(),
        );

        // No alert of its own: rotation is evidence for Tornado detection, which alerts on its
        // verdicts (`Self::tornado_alert`). A user's rule on rotation still fires from these hits,
        // and the near-you check still reads the couplets that clear the user's floor.
        let min = self.settings.detectors.rotation_min_confidence;
        let alertable: Vec<_> = hits
            .iter()
            .copied()
            .filter(|h| h.confidence >= min)
            .collect();
        self.rotation_near_you(&alertable);

        // The user's confidence threshold is applied on the way out (after corroboration, not
        // baked into the cache), so lowering it shows the hidden ones at once instead of at the
        // next scan.
        let evidence = self.confirm_evidence(idx);
        let minute = self.volume_minute(idx);
        let mut shown: Vec<_> = hits
            .into_iter()
            .map(|mut h| {
                h.confirmation = wxdata::confirm::confirm(h.lon, h.lat, minute, &evidence);
                h
            })
            .filter(|h| h.confidence >= min || h.confirmation.level().is_some())
            .collect();
        shown.sort_by_key(|h| std::cmp::Reverse(h.confirmation.level()));
        shown
    }

    /// Cell tracks for the active pane, computed here from reflectivity rather than read from a
    /// Level 3 storm-cell table — the point of the layer is the sites that have no such table.
    ///
    /// Built by folding [`wxdata::celltrack::associate`] over the timeline frames still in the
    /// decode cache, so it needs no extra downloads and silently produces nothing on a fresh boot
    /// with one volume in hand.
    pub(crate) fn compute_local_tracks(&mut self) -> Vec<wxdata::celltrack::Track> {
        // A bounded trailing window, not "everything from frame 0 to the playhead": association
        // only ever looks at a track's *last* point and `fit_motion` only ever fits the last
        // `FIT_POINTS` (6) of them, so frames older than this window contribute nothing to the
        // answer — only cost. Recomputing the full history on every volume tick made this scale
        // with how long the session (or the scrubbed-to archive day) had been running, freezing
        // the UI thread for whole seconds once a live session or a deep timeline scrub had
        // accumulated a few dozen volumes, and it never got cheaper again for the rest of the
        // session. 16 volumes is over an hour at a typical VCP, comfortably more than
        // `FIT_POINTS` needs.
        const WINDOW_FRAMES: usize = 16;
        let key = self.volume_key(self.active);
        if let Some((k, v)) = &self.tracks_cache {
            if *k == key {
                return v.clone();
            }
        }
        let playhead = self.views[self.active].timeline.playhead;
        let frames: Vec<_> = self.views[self.active]
            .timeline
            .frames
            .iter()
            .take(playhead + 1)
            .rev()
            .take(WINDOW_FRAMES)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .filter_map(|id| id.date_time().map(|t| (id.name().to_string(), t)))
            .filter(|(name, _)| self.scan_cache.contains(name))
            .collect();
        let mut tracks: Vec<wxdata::celltrack::Track> = Vec::new();
        for (name, at) in frames {
            let cells = match self.celltrack_cache.get(&name) {
                Some(c) => c.clone(),
                None => {
                    let Some(scan) = self.scan_cache.get(&name).map(Arc::clone) else {
                        continue;
                    };
                    // Lowest tilt: the cell a chaser is driving toward is the one at the ground.
                    let cells = match level2::bin_scan(&scan, Moment::Reflectivity, 0) {
                        Ok(sweep) => wxdata::celltrack::find_cells(&sweep, 45.0),
                        Err(e) => {
                            log::debug!("celltrack: skipping {name}: {e}");
                            Vec::new()
                        }
                    };
                    self.celltrack_cache.put(name.clone(), cells.clone());
                    cells
                }
            };
            tracks = wxdata::celltrack::associate(&tracks, &cells, at, 30.0);
        }
        // A track whose last point is older than the newest frame stopped being seen; drop it
        // rather than leave a stale arrow pointing at empty sky.
        if let Some(newest) = tracks
            .iter()
            .filter_map(|t| t.points.last())
            .map(|p| p.2)
            .max()
        {
            tracks
                .retain(|t| t.points.last().is_some_and(|p| p.2 == newest) && t.points.len() >= 2);
        }
        self.tracks_cache = Some((key, tracks.clone()));
        tracks
    }

    /// Confidence-over-time for the active pane's debris signatures — C5's "timeline of score
    /// changes" (`wxdata::scoretrack`), built the same bounded-trailing-window way
    /// [`Self::compute_local_tracks`] builds cell tracks, and for the same reason: replaying a
    /// whole session's history on every tick does not scale. Folds `wxdata::scoretrack::associate`
    /// over `tds_shown_cache` (the *corroborated* hits, the same confidence the marker itself
    /// shows), so it costs nothing beyond what `compute_tds` already paid to decode and
    /// corroborate each volume once — a frame not yet in that cache (never viewed, or aged out of
    /// it) is silently skipped rather than triggering a fresh per-tilt gate scan just to backfill
    /// a history nobody may ever look at.
    pub(crate) fn compute_tds_score_track(
        &mut self,
        idx: usize,
    ) -> Vec<wxdata::scoretrack::ScoreTrack> {
        const WINDOW_FRAMES: usize = 16;
        let key = self.volume_key(idx);
        if let Some((k, v)) = &self.tds_tracks_cache {
            if *k == key {
                return v.clone();
            }
        }
        let playhead = self.views[idx].timeline.playhead;
        let frames: Vec<_> = self.views[idx]
            .timeline
            .frames
            .iter()
            .take(playhead + 1)
            .rev()
            .take(WINDOW_FRAMES)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .filter_map(|id| id.date_time().map(|t| (id.name().to_string(), t)))
            .collect();
        let mut tracks: Vec<wxdata::scoretrack::ScoreTrack> = Vec::new();
        for (name, at) in frames {
            let Some(hits) = self.tds_shown_cache.get(&name) else {
                continue;
            };
            let points: Vec<_> = hits
                .iter()
                .map(|h| wxdata::scoretrack::ScorePoint {
                    lon: h.lon,
                    lat: h.lat,
                    confidence: h.confidence,
                    time: at,
                })
                .collect();
            tracks = wxdata::scoretrack::associate(&tracks, &points);
        }
        self.tds_tracks_cache = Some((key, tracks.clone()));
        tracks
    }

    /// Same as [`Self::compute_tds_score_track`], for rotation couplets, folded over the
    /// corroborated `rot_shown_cache`.
    pub(crate) fn compute_rot_score_track(
        &mut self,
        idx: usize,
    ) -> Vec<wxdata::scoretrack::ScoreTrack> {
        const WINDOW_FRAMES: usize = 16;
        let key = self.volume_key(idx);
        if let Some((k, v)) = &self.rot_tracks_cache {
            if *k == key {
                return v.clone();
            }
        }
        let playhead = self.views[idx].timeline.playhead;
        let frames: Vec<_> = self.views[idx]
            .timeline
            .frames
            .iter()
            .take(playhead + 1)
            .rev()
            .take(WINDOW_FRAMES)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .filter_map(|id| id.date_time().map(|t| (id.name().to_string(), t)))
            .collect();
        let mut tracks: Vec<wxdata::scoretrack::ScoreTrack> = Vec::new();
        for (name, at) in frames {
            let Some(hits) = self.rot_shown_cache.get(&name) else {
                continue;
            };
            let points: Vec<_> = hits
                .iter()
                .map(|h| wxdata::scoretrack::ScorePoint {
                    lon: h.lon,
                    lat: h.lat,
                    confidence: h.confidence,
                    time: at,
                })
                .collect();
            tracks = wxdata::scoretrack::associate(&tracks, &points);
        }
        self.rot_tracks_cache = Some((key, tracks.clone()));
        tracks
    }
}

/// One volume's rotation columns and, from the same sweeps once the columns are done, when those
/// sweeps were scanned (the fused Tornado ID's input clocks).
#[allow(clippy::type_complexity)]
/// The fused pipeline's tracking on one site ([`HookEchoApp::compute_llsd`]).
pub(crate) struct LlsdTracking {
    site: String,
    tracker: wxdata::rotation_tracks::Tracker,
    /// The volume and the low-level pass (seconds since the epoch) last fed to the tracker.
    fed: Option<(String, i64)>,
    /// What the tracker made of that pass, re-analysed as the volume's other sweeps arrive.
    last: Vec<wxdata::rotation_tracks::Tracked>,
    /// The passes each track has read *likely* or stronger on.
    pub(crate) likely: wxdata::llsd_analyst::LikelyConfirmation,
}

impl LlsdTracking {
    fn new(site: String) -> Self {
        LlsdTracking {
            site,
            tracker: wxdata::rotation_tracks::Tracker::new(
                wxdata::rotation_tracks::TrackParams::default(),
            ),
            fed: None,
            last: Vec::new(),
            likely: Default::default(),
        }
    }
}

/// One low-level pass's columns, oldest first, from the background job: when (seconds since the
/// epoch), the columns, and, for a pass before the volume's newest, its own debris signatures.
pub(crate) struct PassColumns {
    time: i64,
    columns: Vec<wxdata::rotation_columns::RotationColumn>,
    debris: Option<Vec<wxdata::tds::TdsHit>>,
}

/// What [`HookEchoApp::compute_llsd`]'s job hands back: each pass's columns, and when the sweeps
/// behind the newest were scanned.
pub(crate) type LlsdJob = (
    Vec<PassColumns>,
    Option<wxdata::level2::temporal::TemporalCoverage>,
);

/// The columns of each pass in `todo`: the newest from the volume's own sweeps (`velocity`, its
/// newest cuts), each earlier one from its own lowest-tilt sweeps under them
/// ([`wxdata::low_passes::at_pass`]), with its own debris signatures.
fn pass_columns(
    scan: std::sync::Arc<wxdata::level2::Scan>,
    todo: Vec<wxdata::low_passes::Pass>,
    newest: chrono::DateTime<chrono::Utc>,
    velocity: Vec<(wxdata::level2::BinnedSweep, wxdata::level2::BinnedSweep)>,
    dual_pol: Vec<(wxdata::level2::BinnedSweep, wxdata::level2::BinnedSweep)>,
    zdr: Vec<wxdata::level2::BinnedSweep>,
    lowest: wxdata::low_passes::Lowest,
) -> LlsdJob {
    let mut steps = Vec::new();
    for pass in todo.iter().filter(|p| p.time < newest) {
        let Some(inputs) =
            wxdata::low_passes::at_pass(&scan, pass, &velocity, &dual_pol, &zdr, lowest)
        else {
            continue;
        };
        let mut debris = wxdata::tds::detect_volume(&inputs.dual_pol, 0.80, 40.0, 150.0, 4);
        wxdata::tds::apply_zdr(&mut debris, &inputs.zdr);
        steps.push(PassColumns {
            time: pass.time.timestamp(),
            columns: wxdata::rotation_columns::from_sweeps(&inputs.velocity),
            debris: Some(debris),
        });
    }
    steps.push(PassColumns {
        time: newest.timestamp(),
        columns: wxdata::rotation_columns::from_sweeps(&velocity),
        debris: None,
    });
    let sweeps = velocity.into_iter().flat_map(|(v, z)| [v, z]).collect();
    (steps, wxdata::detection_lineage::input_coverage(sweeps))
}

impl HookEchoApp {
    /// Tornado detection's one alert: a banner, notification and sound when a verdict reaches
    /// Likely or higher (Likely, Debris, Confirmed), or rises to a higher tier, on the active pane.
    /// Possible does not alert: on a random sample of ordinary severe days it was wrong about once
    /// per radar-hour, where Likely was wrong once in forty (detectionplan.md). While the original
    /// pipeline stands in for a fused verdict still being computed, nothing is decided, so an alert
    /// cannot flip when the fused one lands.
    pub(crate) fn tornado_alert(
        &mut self,
        idx: usize,
        circulations: &[wxdata::tornado_id::Circulation],
        lineage: Option<&wxdata::detection_lineage::DetectionLineage>,
    ) {
        use wxdata::tornado_id::Tier;
        let ids: Vec<_> = circulations.iter().map(|c| c.id.clone()).collect();
        let standing_in = lineage.is_some_and(|l| l.stand_in.is_some());
        let (fire, state) = tornado_alert_decision(self.tornado_alerted, &ids, standing_in);
        self.tornado_alerted = state;
        if let Some(t) = fire {
            let site = self.views[idx].site.clone().unwrap_or_default();
            let where_ = wxdata::sites::site_by_id(&site)
                .map(|s| {
                    let (km, bearing) = crate::geo::great_circle(
                        [s.longitude as f64, s.latitude as f64],
                        [t.lon, t.lat],
                    );
                    format!("{km:.0} km {} of {site}", cardinal(bearing))
                })
                .unwrap_or_else(|| format!("{:.2}, {:.2}", t.lat, t.lon));
            let mut detail = vec![format!(
                "evidence {}",
                wxdata::evidence::out_of_100(t.score)
            )];
            if let Some(v) = t.vrot_ms {
                detail.push(format!("{:.0} kt rotation", v * 1.943_844));
            }
            if let Some(cc) = t.min_cc {
                detail.push(format!("debris CC down to {cc:.2}"));
            }
            log::info!(
                target: "wxdata::tornado_id",
                "{site}: {} — {where_} ({})",
                t.tier.label(),
                detail.join(", ")
            );
            let mark = if t.tier >= Tier::Debris {
                "\u{26a0}"
            } else {
                "\u{27f3}"
            };
            // One banner per tornado: a rise (Likely to Debris, or to Confirmed once the reports
            // load) replaces the card it raised before instead of stacking a second one about the
            // same storm.
            self.warning_banners
                .retain(|(event, _, _)| !is_tornado_detection_banner(event));
            self.banner(
                format!("{mark} {}", t.tier.label()),
                format!("{where_} ({})", detail.join(", ")),
            );
            self.notify_alert(&format!("{mark} {}", t.tier.label()), &where_, true);
            if self.settings.alert_sound {
                let sound = if t.tier >= Tier::Debris {
                    self.settings.tds_sound.clone()
                } else {
                    self.settings.rotation_sound.clone()
                };
                self.play_alert_urgent(&sound);
            }
        }
    }
}

/// Whether a banner is one Tornado detection raised: "⚠ Tornado debris", "⟳ Tornado likely" and
/// so on, as `tornado_alert` titles them.
pub(crate) fn is_tornado_detection_banner(event: &str) -> bool {
    use wxdata::tornado_id::Tier;
    [Tier::Possible, Tier::Likely, Tier::Debris, Tier::Confirmed]
        .iter()
        .any(|tier| {
            ["\u{26a0}", "\u{27f3}"]
                .iter()
                .any(|mark| event == format!("{mark} {}", tier.label()))
        })
}

/// What Tornado detection alerts on, given the tier it last alerted on (`previous`) and this
/// volume's verdicts: the verdict to alert on, if any, and the state to keep. It alerts when the
/// best verdict reaches Likely or higher, or rises above what it alerted on; it stays quiet while
/// that holds; it resets once nothing is Likely or higher. While the original pipeline stands in
/// for a fused verdict (`standing_in`), nothing changes.
pub(crate) fn tornado_alert_decision(
    previous: Option<wxdata::tornado_id::Tier>,
    ids: &[wxdata::tornado_id::TornadoId],
    standing_in: bool,
) -> (
    Option<wxdata::tornado_id::TornadoId>,
    Option<wxdata::tornado_id::Tier>,
) {
    use wxdata::tornado_id::Tier;
    if standing_in {
        return (None, previous);
    }
    let best = ids
        .iter()
        .filter(|t| t.tier >= Tier::Likely)
        .max_by(|a, b| a.tier.cmp(&b.tier).then(a.score.total_cmp(&b.score)))
        .cloned();
    let state = best.as_ref().map(|t| t.tier);
    let fire = best.filter(|t| previous.is_none_or(|was| t.tier > was));
    (fire, state)
}

#[cfg(test)]
mod tornado_alert_tests {
    use super::{is_tornado_detection_banner, tornado_alert_decision};

    #[test]
    fn a_rise_replaces_the_tornado_banner_and_leaves_the_others() {
        assert!(is_tornado_detection_banner("\u{26a0} Tornado debris"));
        assert!(is_tornado_detection_banner("\u{26a0} Tornado confirmed"));
        assert!(is_tornado_detection_banner("\u{27f3} Tornado likely"));
        // An NWS warning banner, or anything else, is not one.
        assert!(!is_tornado_detection_banner("Tornado Warning"));
        assert!(!is_tornado_detection_banner("\u{26a0} Tornado Emergency"));
    }
    use wxdata::tornado_id::{Tier, TornadoId};

    fn id(tier: Tier, score: f32) -> TornadoId {
        TornadoId {
            lon: -97.5,
            lat: 35.3,
            tier,
            score,
            terms: Vec::new(),
            vrot_ms: None,
            min_cc: None,
            reasons: Vec::new(),
        }
    }

    #[test]
    fn it_alerts_on_likely_or_higher_once_and_again_only_when_it_rises() {
        // Possible alone never alerts.
        let (fire, state) = tornado_alert_decision(None, &[id(Tier::Possible, 0.4)], false);
        assert!(fire.is_none() && state.is_none());
        // Likely alerts, once.
        let (fire, state) = tornado_alert_decision(None, &[id(Tier::Likely, 0.65)], false);
        assert_eq!(fire.map(|t| t.tier), Some(Tier::Likely));
        let (fire, state) = tornado_alert_decision(state, &[id(Tier::Likely, 0.70)], false);
        assert!(fire.is_none(), "the same tier again stays quiet");
        // Debris beside it raises it: alert again.
        let (fire, state) = tornado_alert_decision(
            state,
            &[id(Tier::Likely, 0.7), id(Tier::Debris, 0.8)],
            false,
        );
        assert_eq!(fire.map(|t| t.tier), Some(Tier::Debris));
        // A stand-in volume decides nothing.
        let (fire, held) = tornado_alert_decision(state, &[], true);
        assert!(fire.is_none());
        assert_eq!(held, Some(Tier::Debris));
        // It ends, and a new one alerts afresh.
        let (_, state) = tornado_alert_decision(held, &[id(Tier::Possible, 0.35)], false);
        assert!(state.is_none());
        let (fire, _) = tornado_alert_decision(state, &[id(Tier::Likely, 0.62)], false);
        assert!(fire.is_some());
    }
}
