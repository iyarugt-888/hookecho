//! The model browser's side of the app: its panel input, the run and lead it reads (a past event
//! reads the run of its time), committing a selection, and model series, verification and
//! isotherms. Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    pub(crate) fn model_timeline_active(&self) -> bool {
        model_timeline_active(
            &self.views[self.active],
            self.views[self.active].models.model_sel,
        )
    }

    pub(crate) fn activate_model_timeline(&mut self) {
        if !self.views[self.active]
            .fields_on
            .contains(&self.views[self.active].models.model_sel.layer())
        {
            return;
        }
        activate_model_timeline(&mut self.views[self.active]);
        // The selected product now owns its layer, so the legacy radar forecast tail must not
        // clear it on its next observed frame or replace the selected model with reflectivity.
        self.views[self.active].models.hrrr_by_timeline = false;
    }

    pub(crate) fn radar_timeline(&mut self) {
        let playback = &mut self.views[self.active].model_playback;
        playback.active = false;
        playback.pause();
    }

    pub(crate) fn toggle_model_playback(&mut self) {
        if self.views[self.active].models.model_sel.model.has_lead() {
            let group = self.views[self.active].model_group;
            for (idx, view) in self.views.iter_mut().enumerate() {
                if idx != self.active && group.is_some() && view.model_group == group {
                    view.model_playback.pause();
                }
            }
            self.views[self.active].model_playback.toggle();
        }
    }

    pub(crate) fn drive_model_timeline(&mut self, ctx: &egui::Context) {
        // Every pane owns its playback; focus changes do not stop an independent pane.
        for idx in 0..self.views.len() {
            let selection = self.views[idx].models.model_sel;
            if !model_timeline_active(&self.views[idx], selection) {
                self.views[idx].model_playback.pause();
                continue;
            }
            let run = self.views[idx].models.model_run;
            let range = scrub_range(&self.views[idx], Utc::now());
            let lead = range.clamp(self.views[idx].models.lead_min());
            if lead != self.views[idx].models.lead_min() {
                self.views[idx].models.set_lead(lead, Utc::now());
                self.views[idx].model_playback.pause();
                model_groups::propagate(&mut self.views, idx, Utc::now());
            }
            let group = self.views[idx].model_group;
            let ready = crate::platform::activity::is_active()
                && self
                    .views
                    .iter()
                    .enumerate()
                    .filter(|(i, v)| *i == idx || group.is_some() && v.model_group == group)
                    .all(|(i, v)| {
                        !v.fields_on.contains(&v.models.model_sel.layer())
                            || self.model_field_ready_for(i, v.models.model_sel.layer())
                                && self
                                    .field_state_for(i, v.models.model_sel.layer())
                                    .and_then(|s| s.stamp.as_ref())
                                    .is_some_and(|stamp| {
                                        model_frame_matches(stamp, v.models.model_sel, lead, run)
                                    })
                    });
            if let Some(next) =
                self.views[idx]
                    .model_playback
                    .tick(lead, range, ready, Instant::now())
            {
                self.views[idx].models.set_lead(next, Utc::now());
                model_groups::propagate(&mut self.views, idx, Utc::now());
            }
            if self.views[idx].model_playback.playing {
                ctx.request_repaint_after(self.views[idx].model_playback.interval());
            }
        }
    }

    /// Everything the model controls need to draw themselves, whichever surface hosts them.
    pub(crate) fn model_panel_input(&self) -> crate::ui::model_panel::Input {
        let now = Utc::now();
        let model = self.views[self.active].models.model_sel.model;
        crate::ui::model_panel::Input {
            sel: self.views[self.active].models.model_sel,
            lead_min: self.model_lead_min(),
            stamp: self
                .field_state_for(
                    self.active,
                    self.views[self.active].models.model_sel.layer(),
                )
                .filter(|_| {
                    self.model_field_ready(self.views[self.active].models.model_sel.layer())
                })
                .and_then(|state| state.stamp.clone()),
            run: self.views[self.active].models.model_run,
            runs: model.runs_around(
                self.views[self.active].models.model_run,
                now,
                model.run_list_len(),
            ),
            range: scrub_range(&self.views[self.active], now),
        }
    }

    /// The forecast lead the model browser is scrubbed to, in minutes. Which clock that reads
    /// depends on the model: regional models share the HRRR-hour clock, the 15-minute product has
    /// its own, and global models read the global forecast hour.
    pub(crate) fn model_lead_min(&self) -> u16 {
        use crate::model_browser::Engine;
        match self.views[self.active].models.model_sel.model.engine() {
            Engine::Sub15 => self.views[self.active].models.hrrr_fcst_min,
            Engine::Regional(_) => u16::from(self.views[self.active].models.hrrr_fcst_hour) * 60,
            Engine::Global(_) => self.views[self.active].models.global_fcst_hour * 60,
            // An analysis is valid at its own hour: there is no lead.
            Engine::Analysis => 0,
        }
    }

    /// Scrub the selected model to `minutes`, snapped to that model's own range and steps.
    pub(crate) fn set_model_lead_min(&mut self, minutes: u16) {
        self.write_model_lead_min(minutes);
        self.activate_model_timeline();
    }

    fn write_model_lead_min(&mut self, minutes: u16) {
        use crate::model_browser::Engine;
        let m = scrub_range(&self.views[self.active], Utc::now()).clamp(minutes);
        match self.views[self.active].models.model_sel.model.engine() {
            Engine::Sub15 => {
                self.views[self.active].models.hrrr_fcst_min = m;
                self.views[self.active].models.hrrr_fcst_hour =
                    (m / 60).min(u16::from(u8::MAX)) as u8;
            }
            Engine::Regional(_) => {
                self.views[self.active].models.hrrr_fcst_hour =
                    (m / 60).min(u16::from(u8::MAX)) as u8;
                // Keep the 15-minute lead in step so switching to it lands on the same time.
                self.views[self.active].models.hrrr_fcst_min = m.max(15);
            }
            Engine::Global(_) => self.views[self.active].models.global_fcst_hour = m / 60,
            Engine::Analysis => {}
        }
    }

    /// Point each engine's state at `sel` without touching what is on the map.
    pub(crate) fn apply_model_engine(&mut self, sel: crate::model_browser::Selection) {
        use crate::model_browser::{Engine, Product};
        match (sel.model.engine(), sel.product) {
            (Engine::Sub15, _) => {
                self.views[self.active].models.refl_model = wxdata::hrrr::Model::Hrrr;
                self.views[self.active].models.hrrr_subhourly = true;
            }
            (Engine::Regional(model), Product::Reflectivity) => {
                self.views[self.active].models.refl_model = model;
                self.views[self.active].models.hrrr_subhourly = false;
            }
            (Engine::Regional(model), Product::Cape | Product::Srh) => {
                self.views[self.active].models.env_model = model;
                self.contour_model = model;
                // STP needs an LCL height that only the HRRR surface file carries; a source
                // without it cannot keep those contours.
                if !crate::ui::layer_options::stp_source(model) {
                    self.active_contours.remove(&ContourKind::Stp);
                    self.active_contours.remove(&ContourKind::StpEff);
                }
            }
            // Rotation tracks, snowfall, smoke and thunder chance are each tied to one model.
            (Engine::Regional(_), _) => {}
            (Engine::Global(model), _) => self.views[self.active].models.global_model = model,
            // RTMA layers read the pinned analysis hour directly; nothing to point.
            (Engine::Analysis, _) => {}
        }
    }

    /// Make `next` the model browser's choice: engine state, lead, and what shows on the map.
    /// `swap` replaces the previous choice's layer (a chip click); otherwise it only adds.
    pub(crate) fn commit_model_selection(
        &mut self,
        next: crate::model_browser::Selection,
        swap: bool,
    ) {
        self.views[self.active].model_restore_raw = None;
        let prev = self.views[self.active].models.model_sel;
        // The lead is a time, not a model's own number: carry it across and let the new model
        // snap it to its own range.
        let lead = self.model_lead_min();
        // A run is one model's cycle; another model has its own, so the pick does not carry over.
        if prev.model != next.model {
            self.views[self.active].models.model_run = None;
        }
        self.views[self.active].models.model_sel = next;
        self.apply_model_engine(next);
        self.set_model_lead_min(lead);
        let fields = &mut self.views[self.active].fields_on;
        if swap && prev.layer() != next.layer() {
            fields.remove(&prev.layer());
        }
        fields.insert(next.layer());
        self.activate_model_timeline();
        let slug = next.slug();
        if self.settings.model_pick != slug {
            self.settings.model_pick = slug;
            self.settings.save();
        }
    }

    /// One model's forecast for one field at the tapped point, over `self.model_series_ui`'s
    /// chosen period — the meteogram under the forecast window's "This week" NWS blend. Same
    /// ~0.05° cache cell and 15-minute TTL as the point forecast beside it, keyed additionally by
    /// the picker so switching model/field/period is a fresh fetch, not a stale hit.
    pub(crate) fn fetch_model_series(&mut self, lon: f64, lat: f64) {
        let key = (
            (lat * 20.0).round() as i32,
            (lon * 20.0).round() as i32,
            self.model_series_ui,
        );
        if let Some((when, series)) = self.model_series_cache.get(&key) {
            if when.elapsed().as_secs() < 900 {
                self.model_series_state = ui::forecast_window::SeriesState::Ready(series.clone());
                return;
            }
        }
        self.model_series_state = ui::forecast_window::SeriesState::Loading;
        let (tx, rx) = std::sync::mpsc::channel();
        self.model_series_rx = Some((key, rx));
        let http = self.http.clone();
        let ui = self.model_series_ui;
        let hours = ui.period.hours();
        self.spawner.spawn(async move {
            let res =
                wxdata::global::fetch_point_series(&http, ui.model, ui.field, lon, lat, &hours)
                    .await
                    .map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
    }

    /// Score the chosen run against the RTMA at each chosen lead, off the UI thread.
    pub(crate) fn fetch_model_verify(&mut self) {
        let w = &self.model_verify;
        let Some(regional) = w.model.regional_model() else {
            return;
        };
        let now = chrono::Utc::now();
        let Some(run) = w
            .run
            .or_else(|| ui::model_verify_window::auto_run(w.model, now))
        else {
            self.model_verify.error = Some("no run is old enough to verify yet".into());
            return;
        };
        let leads: Vec<u8> = ui::model_verify_window::LEADS
            .iter()
            .zip(w.leads_on)
            .filter_map(|(lead, on)| on.then_some(*lead))
            .collect();
        let threshold = w.threshold_on.then_some(if w.field.is_temperature() {
            w.threshold_k
        } else {
            w.threshold_dbz
        });
        let region = w.region_is_view.then(|| {
            let (west, south, east, north) = self.view_bounds();
            (west, south, east, north)
        });
        let meta = ui::model_verify_window::Meta {
            model: w.model,
            field: w.field,
            run,
            threshold,
            region_is_view: w.region_is_view,
            truth: w.truth,
        };
        let (field, truth) = (w.field, w.truth);
        self.model_verify.busy = true;
        self.model_verify.error = None;
        let (tx, rx) = std::sync::mpsc::channel();
        self.model_verify_rx = Some(rx);
        let http = self.http.clone();
        self.spawner.spawn(async move {
            let res = wxdata::gridverify::verify_run(
                &http, regional, field, run, &leads, region, threshold, truth,
            )
            .await
            .map(|rows| (meta, rows))
            .map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
    }

    /// Keep the HRRR isotherm heights for the 3D map's model surfaces current: the latest analysis,
    /// fetched only while some pane shows them and again each hour.
    pub(crate) fn sync_model_isotherms(&mut self) {
        if let Some((key, rx)) = &self.model_isotherms_rx {
            match rx.try_recv() {
                Ok(fields) => {
                    self.model_isotherms = Some((*key, fields));
                    self.model_isotherms_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.model_isotherms_rx = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
            }
        }
        let wanted = self
            .views
            .iter()
            .any(|v| v.map_3d.enabled && v.map_3d.model_isotherms);
        if !wanted {
            return;
        }
        let hour = Utc::now().timestamp().div_euclid(3600);
        if self
            .model_isotherms
            .as_ref()
            .is_some_and(|(k, _)| *k == hour)
        {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.model_isotherms_rx = Some((hour, rx));
        let http = self.http.clone();
        self.spawner.spawn(async move {
            let mut out = Vec::new();
            let mut run = None;
            for (level, label, color) in MODEL_ISOTHERMS {
                match wxdata::hrrr::fetch_field(
                    &http,
                    wxdata::hrrr::Model::Hrrr,
                    "HGT",
                    level,
                    0,
                    f64::NEG_INFINITY,
                )
                .await
                {
                    Ok(fc) => {
                        let mut field = fc.field;
                        // Geopotential metres; km like every other height layer here.
                        for v in &mut field.values {
                            *v /= 1000.0;
                        }
                        run = Some(fc.run);
                        out.push((label, color, field));
                    }
                    Err(e) => log::warn!("HRRR {level}: {e:#}"),
                }
            }
            if let Some(run) = run {
                let _ = tx.send((out, run));
            }
        });
    }
}

/// The leads a pane's model scrub steps through: the selected model's, narrowed to the browsed
/// field's own global models (the field's, and the one it is differenced against) when it is
/// shown under a global model. A browsed ECMWF field under the GFS clock then steps six-hourly
/// past F+144, as its files do, instead of repeating F+144 at F+147.
pub(crate) fn scrub_range(view: &MapView, now: DateTime<Utc>) -> crate::model_browser::LeadRange {
    use super::model_field::InventorySource;
    use crate::model_browser::{BModel, Engine};
    let models = &view.models;
    let mut range = models.model_sel.model.leads_for(models.model_run, now);
    let Some(field) = models.field.as_ref() else {
        return range;
    };
    if !matches!(models.model_sel.model.engine(), Engine::Global(_))
        || !view
            .fields_on
            .contains(&crate::render::FieldLayer::ModelField)
    {
        return range;
    }
    for source in std::iter::once(field.model).chain(field.minus) {
        let global = match source {
            InventorySource::Ecmwf => BModel::Ecmwf,
            InventorySource::Gfs => BModel::Gfs,
            InventorySource::Regional(_) => continue,
        };
        range = range.narrowed(global.leads_for(models.model_run, now));
    }
    range
}

pub(super) fn model_timeline_active(view: &MapView, sel: crate::model_browser::Selection) -> bool {
    view.model_playback.active && view.fields_on.contains(&sel.layer())
}

fn activate_model_timeline(view: &mut MapView) {
    view.timeline.playing = false;
    if view.timeline.forecast_hour().is_some() {
        view.timeline.go_head();
    }
    view.model_playback.activate();
}

fn model_frame_matches(
    stamp: &wxdata::field::DataStamp,
    sel: crate::model_browser::Selection,
    lead: u16,
    run: Option<DateTime<Utc>>,
) -> bool {
    let source = match sel.model.engine() {
        crate::model_browser::Engine::Regional(model) => model.label(),
        crate::model_browser::Engine::Global(model) => model.label(),
        _ => sel.model.label(),
    };
    if stamp.source_id != source {
        return false;
    }
    stamp.run_time.is_some_and(|cycle| {
        run.is_none_or(|selected| selected == cycle)
            && stamp.valid_time == cycle + chrono::Duration::minutes(i64::from(lead))
    })
}

/// The model run to read for a view scrubbed back to `target`: the newest cycle at or before it,
/// so a historical radar event is never shown under today's model. `None` within the few hours a
/// run takes to post, where the newest run is the right one anyway and the target's own may not
/// exist yet.
pub(crate) fn archive_run(
    target: DateTime<Utc>,
    now: DateTime<Utc>,
    cycle_hours: u32,
) -> Option<DateTime<Utc>> {
    use chrono::Timelike;
    const POSTING: chrono::Duration = chrono::Duration::hours(3);
    if now - target < POSTING || cycle_hours == 0 {
        return None;
    }
    let hour = target.hour() - target.hour() % cycle_hours;
    target
        .date_naive()
        .and_hms_opt(hour, 0, 0)
        .map(|t| t.and_utc())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_browser::{BModel, Product, Selection};

    #[test]
    fn model_timeline_activation_retires_radar_tail_without_needing_a_model_slider() {
        let mut view = MapView::new(
            None,
            crate::render::mercator::Camera::at_lonlat(-97.0, 35.0, 8.0),
        );
        let regional = Selection {
            model: BModel::NamNest,
            product: Product::Reflectivity,
        };
        view.fields_on.insert(regional.layer());
        view.timeline.frames.push(wxdata::level2::Identifier::new(
            "KTLX20260819_120000_V06".into(),
        ));
        view.timeline.playhead = 1;
        view.timeline.playing = true;
        assert_eq!(view.timeline.forecast_hour(), Some(1));
        activate_model_timeline(&mut view);
        assert!(model_timeline_active(&view, regional));
        assert!(!view.timeline.playing);
        assert_eq!(
            view.timeline.forecast_hour(),
            None,
            "old tail cannot overwrite the model"
        );
        assert!(view.fields_on.contains(&regional.layer()));
        let global = Selection {
            model: BModel::Gfs,
            product: Product::Temp2m,
        };
        view.fields_on.remove(&regional.layer());
        view.fields_on.insert(global.layer());
        activate_model_timeline(&mut view);
        assert!(model_timeline_active(&view, global));
        assert!(!model_timeline_active(&view, regional));
        view.fields_on.remove(&global.layer());
        assert!(!model_timeline_active(&view, global));
        let other = MapView::new(
            None,
            crate::render::mercator::Camera::at_lonlat(-97.0, 35.0, 8.0),
        );
        assert!(
            !model_timeline_active(&other, global),
            "mode belongs to its pane"
        );
    }

    /// A browsed ECMWF field under the GFS clock: the scrub steps three-hourly to F+144 and then
    /// six-hourly, as the ECMWF publishes, so F+147 is never a repeat of F+144. Hidden, or under a
    /// regional model, the selected model's own steps stand.
    #[test]
    fn a_browsed_ecmwf_field_scrubs_on_its_own_published_leads() {
        use super::super::model_field::{InventorySource, SavedFieldPick};
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 10, 10, 18, 0, 0).unwrap();
        let mut view = MapView::new(
            None,
            crate::render::mercator::Camera::at_lonlat(-97.0, 35.0, 8.0),
        );
        view.models.model_sel = Selection {
            model: BModel::Gfs,
            product: Product::Temp2m,
        };
        view.models.model_run = Some(Utc.with_ymd_and_hms(2026, 10, 10, 0, 0, 0).unwrap());
        let ecmwf = SavedFieldPick {
            model: InventorySource::Ecmwf,
            var: "TMP".into(),
            level: "500 mb".into(),
            kind: wxdata::model_inventory::TimingKind::Instant,
            vector: false,
            minus: None,
        };
        view.models.field = Some(ecmwf.clone());
        let gfs_own = view
            .models
            .model_sel
            .model
            .leads_for(view.models.model_run, now);
        assert_eq!(
            scrub_range(&view, now),
            gfs_own,
            "a hidden field changes nothing"
        );

        view.fields_on.insert(crate::render::FieldLayer::ModelField);
        let range = scrub_range(&view, now);
        let hours: Vec<u16> = range.positions().iter().map(|m| m / 60).collect();
        assert!(hours.windows(2).all(|w| w[0] < w[1]));
        assert!(hours.contains(&141) && hours.contains(&144) && hours.contains(&150));
        assert!(!hours.contains(&147), "F+147 would repeat F+144's file");
        assert_eq!(hours.last(), Some(&360), "the 00Z ECMWF run ends at F+360");
        // Every scrub position is a file the field's model publishes.
        let wx = wxdata::global::GlobalModel::Ecmwf;
        assert!(hours.iter().all(|&h| wx.inventory_lead(h) == h));
        assert_eq!(range.clamp(147 * 60), 144 * 60);

        // The GFS minus the ECMWF steps the same way: the ECMWF side has no F+147 to difference.
        view.models.field = Some(SavedFieldPick {
            model: InventorySource::Gfs,
            minus: Some(InventorySource::Ecmwf),
            ..ecmwf.clone()
        });
        assert_eq!(scrub_range(&view, now), range);

        // A GFS field under the GFS clock keeps the GFS's own steps.
        view.models.field = Some(SavedFieldPick {
            model: InventorySource::Gfs,
            ..ecmwf
        });
        assert_eq!(scrub_range(&view, now), gfs_own);
    }

    #[test]
    fn model_timeline_playback_waits_for_matching_source_run_and_lead() {
        use chrono::TimeZone;
        let run = Utc.with_ymd_and_hms(2026, 10, 4, 12, 0, 0).unwrap();
        let sel = Selection {
            model: BModel::NamNest,
            product: Product::Reflectivity,
        };
        let mut stamp = wxdata::field::DataStamp {
            source_id: "NAM 3 km nest".into(),
            product_id: "Composite reflectivity".into(),
            issue_time: None,
            run_time: Some(run),
            valid_time: run + chrono::Duration::hours(2),
            received_time: run,
            source_latency: None,
            is_forecast: true,
            is_derived: false,
            quality: wxdata::field::QualitySummary::Unknown,
            grid: None,
        };
        assert!(model_frame_matches(&stamp, sel, 120, Some(run)));
        assert!(model_frame_matches(&stamp, sel, 120, None));
        assert!(!model_frame_matches(&stamp, sel, 180, None));
        assert!(!model_frame_matches(
            &stamp,
            sel,
            120,
            Some(run - chrono::Duration::hours(6))
        ));
        stamp.source_id = "HRRR".into();
        assert!(!model_frame_matches(&stamp, sel, 120, None));
        stamp.run_time = None;
        assert!(!model_frame_matches(&stamp, sel, 120, None));
    }

    #[test]
    fn a_past_event_reads_the_model_run_of_its_time() {
        use chrono::TimeZone;
        let now = chrono::Utc.with_ymd_and_hms(2026, 9, 29, 18, 0, 0).unwrap();
        let may = chrono::Utc
            .with_ymd_and_hms(2013, 5, 20, 20, 47, 0)
            .unwrap();
        assert_eq!(
            super::archive_run(may, now, 1),
            Some(chrono::Utc.with_ymd_and_hms(2013, 5, 20, 20, 0, 0).unwrap()),
            "the hourly HRRR's own hour"
        );
        assert_eq!(
            super::archive_run(may, now, 6),
            Some(chrono::Utc.with_ymd_and_hms(2013, 5, 20, 18, 0, 0).unwrap()),
            "a six-hourly model's last cycle before it"
        );
        let recent = now - chrono::Duration::minutes(90);
        assert_eq!(
            super::archive_run(recent, now, 1),
            None,
            "recent: the newest run"
        );
    }
}
