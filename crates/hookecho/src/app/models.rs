//! The model browser's side of the app: its panel input, the run and lead it reads (a past event
//! reads the run of its time), committing a selection, and model series, verification and
//! isotherms. Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Everything the model controls need to draw themselves, whichever surface hosts them.
    pub(crate) fn model_panel_input(&self) -> crate::ui::model_panel::Input {
        let now = Utc::now();
        let model = self.model_sel.model;
        crate::ui::model_panel::Input {
            sel: self.model_sel,
            lead_min: self.model_lead_min(),
            stamp: self
                .fields
                .get(&self.model_sel.layer())
                .and_then(|state| state.stamp.clone()),
            run: self.model_run,
            runs: model.runs_around(self.model_run, now, model.run_list_len()),
            range: model.leads_for(self.model_run, now),
        }
    }

    /// The run pinned in the browser, if it is one this regional model actually publishes. Runs
    /// are named by hour, so a 17Z pick means something to the hourly HRRR and nothing to the
    /// six-hourly NAM, which then simply reads its newest run.
    ///
    /// With nothing pinned, a view scrubbed back to a past event reads the run of that time
    /// ([`archive_run`]) rather than today's (ROADMAP_2 §10.3).
    pub(crate) fn pinned_regional_run(&self, model: wxdata::hrrr::Model) -> Option<DateTime<Utc>> {
        use chrono::Timelike;
        let cycle = model.def().cycle_hours;
        self.model_run
            .filter(|run| run.hour() % cycle == 0)
            .or_else(|| archive_run(self.view_target_time()?, Utc::now(), cycle))
    }

    /// The pinned run, if it lies on the global models' six-hourly cycles; else, scrubbed back,
    /// the run of that time.
    pub(crate) fn pinned_global_run(&self) -> Option<DateTime<Utc>> {
        use chrono::Timelike;
        self.model_run
            .filter(|run| run.hour() % 6 == 0)
            .or_else(|| archive_run(self.view_target_time()?, Utc::now(), 6))
    }

    /// The forecast lead the model browser is scrubbed to, in minutes. Which clock that reads
    /// depends on the model: regional models share the HRRR-hour clock, the 15-minute product has
    /// its own, and global models read the global forecast hour.
    pub(crate) fn model_lead_min(&self) -> u16 {
        use crate::model_browser::Engine;
        match self.model_sel.model.engine() {
            Engine::Sub15 => self.hrrr_fcst_min,
            Engine::Regional(_) => u16::from(self.hrrr_fcst_hour) * 60,
            Engine::Global(_) => self.global_fcst_hour * 60,
            // An analysis is valid at its own hour: there is no lead.
            Engine::Analysis => 0,
        }
    }

    /// Scrub the selected model to `minutes`, snapped to that model's own range and steps.
    pub(crate) fn set_model_lead_min(&mut self, minutes: u16) {
        use crate::model_browser::Engine;
        let m = self
            .model_sel
            .model
            .leads_for(self.model_run, Utc::now())
            .clamp(minutes);
        match self.model_sel.model.engine() {
            Engine::Sub15 => {
                self.hrrr_fcst_min = m;
                self.hrrr_fcst_hour = (m / 60).min(u16::from(u8::MAX)) as u8;
            }
            Engine::Regional(_) => {
                self.hrrr_fcst_hour = (m / 60).min(u16::from(u8::MAX)) as u8;
                // Keep the 15-minute lead in step so switching to it lands on the same time.
                self.hrrr_fcst_min = m.max(15);
            }
            Engine::Global(_) => self.global_fcst_hour = m / 60,
            Engine::Analysis => {}
        }
    }

    /// Point each engine's state at `sel` without touching what is on the map.
    pub(crate) fn apply_model_engine(&mut self, sel: crate::model_browser::Selection) {
        use crate::model_browser::{Engine, Product};
        match (sel.model.engine(), sel.product) {
            (Engine::Sub15, _) => {
                self.refl_model = wxdata::hrrr::Model::Hrrr;
                self.hrrr_subhourly = true;
            }
            (Engine::Regional(model), Product::Reflectivity) => {
                self.refl_model = model;
                self.hrrr_subhourly = false;
            }
            (Engine::Regional(model), Product::Cape | Product::Srh) => {
                self.env_model = model;
                // STP needs an LCL height that only the HRRR surface file carries; a source
                // without it cannot keep those contours.
                if !crate::ui::layer_options::stp_source(model) {
                    self.active_contours.remove(&ContourKind::Stp);
                    self.active_contours.remove(&ContourKind::StpEff);
                }
            }
            // Rotation tracks, snowfall, smoke and thunder chance are each tied to one model.
            (Engine::Regional(_), _) => {}
            (Engine::Global(model), _) => self.global_model = model,
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
        let prev = self.model_sel;
        // The lead is a time, not a model's own number: carry it across and let the new model
        // snap it to its own range.
        let lead = self.model_lead_min();
        // A run is one model's cycle; another model has its own, so the pick does not carry over.
        if prev.model != next.model {
            self.model_run = None;
        }
        self.model_sel = next;
        self.apply_model_engine(next);
        self.set_model_lead_min(lead);
        let fields = &mut self.views[self.active].fields_on;
        if swap && prev.layer() != next.layer() {
            fields.remove(&prev.layer());
        }
        fields.insert(next.layer());
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
        let threshold = w.threshold_on.then_some(w.threshold_k);
        let region = w.region_is_view.then(|| {
            let (west, south, east, north) = self.view_bounds();
            (west, south, east, north)
        });
        let meta = ui::model_verify_window::Meta {
            model: w.model,
            field: w.field,
            run,
            threshold_k: threshold,
            region_is_view: w.region_is_view,
        };
        let field = w.field;
        self.model_verify.busy = true;
        self.model_verify.error = None;
        let (tx, rx) = std::sync::mpsc::channel();
        self.model_verify_rx = Some(rx);
        let http = self.http.clone();
        self.spawner.spawn(async move {
            let res = wxdata::gridverify::verify_run(
                &http, regional, field, run, &leads, region, threshold,
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
