//! The route planner's per-frame work: the route window, its fetches and the warnings it crosses.
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// The route window's per-frame work: take a finished fetch, work out progress and exposure
    /// along the chosen route, draw the window, and act on what it asks.
    pub(crate) fn route_frame(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.route_rx {
            if let Ok(res) = rx.try_recv() {
                self.route_rx = None;
                let w = &mut self.route_window;
                w.busy = false;
                match res {
                    Ok(routes) => {
                        w.routes = routes;
                        w.selected = 0;
                    }
                    Err(e) => {
                        w.routes.clear();
                        w.error = Some(e);
                    }
                }
                w.generation += 1;
            }
        }
        if !self.route_window.open {
            return;
        }
        let route = self
            .route_window
            .routes
            .get(self.route_window.selected)
            .cloned();
        // Progress from the chase position, when it is on (or within 2 km of) the route.
        let along = route
            .as_ref()
            .zip(self.chase_pos)
            .and_then(|(r, (lon, lat))| {
                let (m, off) = wxdata::route::progress(&r.coords, [lon, lat])?;
                (off < 2_000.0).then_some(m)
            });
        let remaining = route.as_ref().zip(along).map(|(r, m)| {
            let total = wxdata::route::cumulative_m(&r.coords)
                .last()
                .copied()
                .unwrap_or(1.0);
            let left = (total - m).max(0.0);
            (left / 1000.0, r.duration_s * left / total.max(1.0))
        });
        let key = (
            self.route_window.generation,
            self.overlay_gen,
            (along.unwrap_or(0.0) / 100.0) as i64,
            self.active_storm_cells().len(),
            // Lightning and the fields refresh by the minute.
            Utc::now().timestamp() / 60,
        );
        if self.route_exposure.0 != key {
            // Warnings and watches in effect now: every overlay polygon that carries an alert.
            let alerts: Vec<(&str, Vec<Vec<[f64; 2]>>)> = self
                .overlays
                .iter()
                .filter_map(|f| Some((f.alert.as_ref()?.event.as_str(), f.rings.clone())))
                .collect();
            let polys: Vec<Vec<Vec<[f64; 2]>>> = alerts.iter().map(|(_, r)| r.clone()).collect();
            let hits = route
                .as_ref()
                .map(|r| wxdata::route::exposure(r, &polys, along.unwrap_or(0.0)))
                .unwrap_or_default();
            // One line per kind of alert: the nearest of each.
            let mut lines: Vec<(String, f64, f64, bool)> = Vec::new();
            for h in hits {
                let what = alerts[h.polygon].0.to_string();
                if !lines.iter().any(|(w, ..)| *w == what) {
                    lines.push((what, h.at_m / 1000.0, h.at_s, true));
                }
            }
            if let Some(r) = route.as_ref() {
                let from = along.unwrap_or(0.0);
                // Heavy echo from a displayed MRMS reflectivity grid that matches the view's time.
                use crate::render::FieldLayer as FL;
                let grid = [FL::Mosaic, FL::ReflLowestAlt].into_iter().find_map(|l| {
                    self.mrms_ready(l)
                        .then(|| self.fields.get(&l)?.grid.as_ref())
                        .flatten()
                });
                if let Some(g) = grid {
                    if let Some((m, s)) = wxdata::route::first_along(r, from, |p| {
                        wxdata::route::grid_value(g, p).is_some_and(|v| v >= 50.0)
                    }) {
                        lines.push(("Heavy echo (50+ dBZ, MRMS)".into(), m / 1000.0, s, false));
                    }
                }
                // Hail, heavy rain and rare rainfall from the MRMS layers that are displayed.
                for (layer, at_least, what) in [
                    (FL::Mesh, 25.4, "Hail 1 in or larger (MESH)"),
                    (
                        FL::Qpe1h,
                        50.8,
                        "2 in or more of rain in the last hour (MRMS)",
                    ),
                    (
                        FL::FlashFlood,
                        10.0,
                        "30-min rainfall rarer than 1-in-10-year (MRMS FLASH)",
                    ),
                ] {
                    let Some(g) = self
                        .mrms_ready(layer)
                        .then(|| self.fields.get(&layer)?.grid.as_ref())
                        .flatten()
                    else {
                        continue;
                    };
                    if let Some((m, sec)) = wxdata::route::first_along(r, from, |p| {
                        wxdata::route::grid_value(g, p).is_some_and(|v| v >= at_least)
                    }) {
                        lines.push((what.into(), m / 1000.0, sec, false));
                    }
                }
                // Lightning within 8 km of the road in the last 15 minutes (live only).
                if self.view_target_time().is_none() {
                    let cutoff = Utc::now() - chrono::Duration::minutes(15);
                    let (mut w, mut s, mut e, mut n) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                    for p in &r.coords {
                        (w, s, e, n) = (w.min(p[0]), s.min(p[1]), e.max(p[0]), n.max(p[1]));
                    }
                    let near: Vec<[f64; 2]> = self
                        .glm
                        .lock()
                        .map(|f| {
                            f.flashes()
                                .iter()
                                .filter(|fl| {
                                    fl.time >= cutoff
                                        && (w - 0.1..=e + 0.1).contains(&fl.lon)
                                        && (s - 0.1..=n + 0.1).contains(&fl.lat)
                                })
                                .map(|fl| [fl.lon, fl.lat])
                                .collect()
                        })
                        .unwrap_or_default();
                    if !near.is_empty() {
                        if let Some((m, sec)) = wxdata::route::first_along(r, from, |p| {
                            near.iter()
                                .any(|f| wxdata::route::haversine_m(p, *f) < 8_000.0)
                        }) {
                            lines.push((
                                "Lightning within 5 mi in the last 15 min".into(),
                                m / 1000.0,
                                sec,
                                false,
                            ));
                        }
                    }
                }
            }
            lines.sort_by(|a, b| a.1.total_cmp(&b.1));
            // Tracked storms with a motion, within 150 km of the route (L4).
            let metric = self.metric();
            let mut storms: Vec<(f64, String)> = Vec::new();
            if let Some(r) = route.as_ref() {
                let from = along.unwrap_or(0.0);
                for c in self.active_storm_cells() {
                    let (Some(deg), Some(kt)) = (c.mvt_deg, c.mvt_kt) else {
                        continue;
                    };
                    let near = wxdata::route::progress(&r.coords, [c.lon, c.lat])
                        .is_some_and(|(_, off)| off < 150_000.0);
                    if !near {
                        continue;
                    }
                    let Some(i) = wxdata::route::intercept(
                        r,
                        from,
                        [c.lon, c.lat],
                        deg as f64,
                        kt as f64,
                        2.0 * 3600.0,
                    ) else {
                        continue;
                    };
                    let name = match c.max_dbz {
                        Some(z) => format!("{} ({z:.0} dBZ)", c.title),
                        None => c.title.clone(),
                    };
                    let mut line = format!(
                        "{name}: closest {} in {} min, to your {}",
                        crate::geo::fmt_distance(i.closest_m / 1000.0, metric, 0),
                        (i.closest_s / 60.0).round(),
                        compass8(i.closest_bearing_deg)
                    );
                    if let Some((ahead, storm_s, you_s)) = i.crossing {
                        line.push_str(&format!(
                            "; crosses the route {} ahead: storm in {} min, you in {} min",
                            crate::geo::fmt_distance(ahead / 1000.0, metric, 0),
                            (storm_s / 60.0).round(),
                            (you_s / 60.0).round()
                        ));
                    }
                    storms.push((i.closest_m, line));
                }
            }
            storms.sort_by(|a, b| a.0.total_cmp(&b.0));
            let intercepts = storms.into_iter().take(6).map(|(_, l)| l).collect();
            self.route_exposure = (key, lines, intercepts);
        }
        let readout = ui::route_window::RouteReadout {
            metric: self.metric(),
            have_position: self.chase_pos.is_some(),
            remaining,
            exposure: &self.route_exposure.1,
            intercepts: &self.route_exposure.2,
        };
        let before = (self.settings.route_engine, self.settings.route_url.clone());
        let action = self.route_window.show(
            ctx,
            &mut self.settings.route_engine,
            &mut self.settings.route_url,
            &readout,
            &mut self.drawer,
        );
        match action {
            ui::route_window::RouteAction::StartHere => {
                if let Some((lon, lat)) = self.chase_pos {
                    self.route_window.waypoints.insert(0, [lon, lat]);
                    self.fetch_route();
                }
            }
            ui::route_window::RouteAction::Fetch => self.fetch_route(),
            ui::route_window::RouteAction::None => {
                if before != (self.settings.route_engine, self.settings.route_url.clone()) {
                    self.settings.save();
                }
            }
        }
    }
}
