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

        let site = self.views[idx].site.clone().unwrap_or_default();
        // Rising-edge alert, on the corroborated hits that clear the user's confidence threshold.
        // A threshold set to quiet doubtful detections must quiet their chime and banner too.
        let min_confidence = self.settings.detectors.tds_min_confidence;
        let alertable: Vec<_> = hits
            .iter()
            .filter(|h| h.confidence >= min_confidence)
            .collect();
        let now_active = !alertable.is_empty();
        if now_active && !self.tds_active {
            print!("\x07");
            use std::io::Write;
            let _ = std::io::stdout().flush();
            let best = *alertable[0]; // sorted strongest-first (by confidence)
                                      // Debris is lofted from the ground, so a column the lowest tilt does not see is the
                                      // one qualification worth carrying into the alert itself rather than the hover.
            let aloft = if best.rooted == Some(false) {
                ", aloft only"
            } else {
                ""
            };
            log::info!(
                target: "wxdata::tds",
                "{site}: TDS detected — evidence {}, {} tilt{}, {:.1}-{:.1} km{aloft}",
                wxdata::evidence::out_of_100(best.confidence),
                best.tilts,
                if best.tilts == 1 { "" } else { "s" },
                best.base_km,
                best.top_km,
            );
            self.banner(
                "⚠ TDS detected".to_string(),
                format!(
                    "{} debris signature(s) — possible tornado (evidence {}, \
                     {} tilt{}, lofted to {:.1} km{aloft}{})",
                    alertable.len(),
                    wxdata::evidence::out_of_100(best.confidence),
                    best.tilts,
                    if best.tilts == 1 { "" } else { "s" },
                    best.top_km,
                    best.rotation_ms.map_or(String::new(), |v| format!(
                        ", {:.0} kt rotation beside it",
                        v * 1.943_844
                    )),
                ),
            );
            self.notify_alert(
                "⚠ Tornado Debris Signature",
                "Low CC + high reflectivity detected on radar",
                true,
            );
            if self.settings.alert_sound {
                self.play_alert_urgent(&self.settings.tds_sound.clone());
            }
        } else if self.tds_active && !now_active {
            log::debug!(target: "wxdata::tds", "{site}: TDS cleared");
        }
        self.tds_active = now_active;

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
    ) {
        use crate::settings::TornadoIdSource;
        // The fusion's verdict once this volume's columns are ready; until then, and for a light
        // loop frame (one tilt), the original's, so the markers never blink out.
        let analysed = match self.settings.detectors.tornado_id_source {
            TornadoIdSource::Fusion => self.compute_llsd(idx, ctx),
            TornadoIdSource::Legacy => None,
        };
        match analysed {
            None => {
                if merged {
                    (Vec::new(), wxdata::tornado_id::circulations(couplets, tds))
                } else {
                    (wxdata::tornado_id::identify(couplets, tds), Vec::new())
                }
            }
            Some(analysed) => {
                let evidence = self.confirm_evidence(idx);
                let minute = self.volume_minute(idx);
                let confirm =
                    |lon: f64, lat: f64| wxdata::confirm::confirm(lon, lat, minute, &evidence);
                if merged {
                    let c = wxdata::llsd_analyst::circulations(&analysed, couplets, tds, confirm);
                    (Vec::new(), c)
                } else {
                    (
                        wxdata::llsd_analyst::identify(&analysed, confirm),
                        Vec::new(),
                    )
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
            if let Some((key, analysed)) = &self.llsd_cache {
                if key.1 == volume_name {
                    let idx = key.0;
                    let evidence = self.confirm_evidence(idx);
                    let minute = self.volume_minute(idx);
                    return wxdata::llsd_analyst::circulations(
                        analysed,
                        couplets,
                        tds,
                        |lon, lat| wxdata::confirm::confirm(lon, lat, minute, &evidence),
                    );
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
    /// The columns cost about a third of a second a volume (four LLSD fields and their objects;
    /// the legacy couplets take about 12 ms), so off the browser build they are computed on a
    /// background thread: the first call for a volume starts it and returns `None`, and when it
    /// is done it asks `ctx` for a repaint and the next call tracks, classifies and fuses (well
    /// under a millisecond). Only the newest volume's job is kept. A light loop frame carries one
    /// tilt, and the fusion's evidence is a column through several, so it is not computed for one
    /// (`None`). The tracker starts over when the site changes or the volume is not newer than the
    /// last one it saw (stepping back through a loop), so a track is only built forward in time.
    pub(crate) fn compute_llsd(
        &mut self,
        idx: usize,
        ctx: &egui::Context,
    ) -> Option<Vec<wxdata::llsd_analyst::Analysed>> {
        let key = self.volume_key(idx);
        if let Some((k, v)) = &self.llsd_cache {
            if *k == key {
                return Some(v.clone());
            }
        }
        if self.views[idx].volume.as_ref().is_none_or(|v| v.light) {
            return None;
        }
        let columns = match self.llsd_job.take() {
            Some((k, rx)) if k == key => match rx.try_recv() {
                Ok(columns) => columns,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.llsd_job = Some((k, rx));
                    return None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return None,
            },
            // A job for another volume (or none): this one is what is wanted now.
            _ => {
                const TILTS: usize = 4;
                let vol = self.views[idx].volume.as_mut()?;
                let pairs: Vec<_> = vol
                    .velocity_tilts_dealiased()
                    .into_iter()
                    .zip(vol.moment_tilts(Moment::Reflectivity))
                    .take(TILTS)
                    .collect();
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let (tx, rx) = std::sync::mpsc::channel();
                    let ctx = ctx.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(wxdata::rotation_columns::from_sweeps(&pairs));
                        ctx.request_repaint();
                    });
                    self.llsd_job = Some((key, rx));
                    return None;
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = ctx;
                    wxdata::rotation_columns::from_sweeps(&pairs)
                }
            }
        };
        let site = self.views[idx].site.clone().unwrap_or_default();
        let time = self.views[idx].volume.as_ref()?.time.timestamp();
        let fresh = match &self.llsd_tracker {
            Some((s, t)) => {
                *s != site
                    || t.tracks
                        .iter()
                        .filter_map(|tr| tr.history.last())
                        .any(|p| p.time >= time)
            }
            None => true,
        };
        if fresh {
            self.llsd_tracker = Some((
                site,
                wxdata::rotation_tracks::Tracker::new(
                    wxdata::rotation_tracks::TrackParams::default(),
                ),
            ));
        }
        let tracked = self
            .llsd_tracker
            .as_mut()
            .map(|(_, t)| t.update(time, columns))
            .unwrap_or_default();
        let debris = self.tds_quiet(idx);
        let out = wxdata::llsd_analyst::analyse(tracked, &debris, &[]);
        self.llsd_cache = Some((key, out.clone()));
        Some(out)
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
        let (mut hits, (radar_lon, radar_lat), scanned) = self.couplets_raw(idx);
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

        // Rising-edge alert, on the corroborated hits that clear the user's confidence threshold.
        let min = self.settings.detectors.rotation_min_confidence;
        // Alerts see only what the user would see; the return value keeps everything.
        let alertable: Vec<_> = hits
            .iter()
            .copied()
            .filter(|h| h.confidence >= min)
            .collect();
        let now_active = !alertable.is_empty();
        if now_active && !self.rot_active {
            let h = alertable[0]; // sorted strongest-first (by confidence, now that height/depth count)
            let kt = h.vrot_ms * 1.943_844;
            let (km, bearing) =
                crate::geo::great_circle([radar_lon as f64, radar_lat as f64], [h.lon, h.lat]);
            let where_ = format!("{:.0} km {} of {site}", km, cardinal(bearing));
            // The two things that change what the confidence means, and both belong in the alert
            // rather than only in the hover: rotation turning the way tornadoes essentially never
            // do, and rotation that never reaches the lowest tilt (a mid-level mesocyclone).
            let mut caveats: Vec<&str> = Vec::new();
            if h.sense == wxdata::rotation::Sense::Anticyclonic {
                caveats.push("anticyclonic");
            }
            if h.rooted == Some(false) {
                caveats.push("aloft only");
            }
            let caveat = if caveats.is_empty() {
                String::new()
            } else {
                format!(", {}", caveats.join(", "))
            };
            log::info!(
                target: "wxdata::rotation",
                "{site}: rotation detected — {kt:.0} kt, {where_}, evidence {}, {} tilt{}{caveat}",
                wxdata::evidence::out_of_100(h.confidence),
                h.tilts,
                if h.tilts == 1 { "" } else { "s" },
            );
            self.banner(
                "⟳ Rotation detected".to_string(),
                format!(
                    "{kt:.0} kt couplet — {where_} (evidence {}, {} tilt{}{caveat})",
                    wxdata::evidence::out_of_100(h.confidence),
                    h.tilts,
                    if h.tilts == 1 { "" } else { "s" },
                ),
            );
            self.notify_alert(
                "⟳ Rotation couplet",
                &format!("{kt:.0} kt rotational velocity — {where_}"),
                true,
            );
            if self.settings.alert_sound {
                self.play_alert_urgent(&self.settings.rotation_sound.clone());
            }
        } else if self.rot_active && !now_active {
            log::debug!(target: "wxdata::rotation", "{site}: rotation cleared");
        }
        self.rot_active = now_active;
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
