//! What a click (or a long press) on the map means: your own markers first, then a peer's
//! stream, a watch zone, a webcam, a live station, a surveyed damage point, a river gauge, and
//! otherwise whatever the armed tool does there. Moved out of `render_pane` unchanged
//! (ROADMAP_2 §7). It returns true where the code used to leave `render_pane` early, so the caller
//! still does.

use super::*;

impl HookEchoApp {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn handle_map_click(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        idx: usize,
        prect: egui::Rect,
        vp: (f32, f32),
        response: &egui::Response,
        long_press: bool,
    ) -> bool {
        self.active = idx;
        // What this press means. A long press is always an interrogation; anything else is
        // whatever the toolbar says.
        let tool = if long_press {
            MapTool::Interrogate
        } else {
            self.tool
        };
        if let Some(pos) = response.interact_pointer_pos() {
            // Remember the tap for the double-tap-drag zoom below.
            self.last_tap = Some((ui.input(|i| i.time), pos));
            let cam = self.views[idx].camera;
            let px = (pos.x - prect.left(), pos.y - prect.top());
            let w = cam.screen_to_world(px, vp);
            let (lon, lat) = crate::render::mercator::world_to_lonlat(w.0, w.1);
            // Your own markers win over everything else on the map: you put them at a place you
            // chose, so a tap there means that pin, not whatever the radar drew underneath.
            // Nearest wins, so clustered markers stay individually reachable.
            let marker_hit = matches!(tool, MapTool::Interrogate | MapTool::Marker)
                .then(|| {
                    self.settings
                        .markers
                        .iter()
                        .enumerate()
                        .filter_map(|(i, m)| {
                            let w = crate::render::mercator::lonlat_to_world(m.lon, m.lat);
                            let (sx, sy) = cam.world_to_screen(w, vp);
                            let (dx, dy) = (prect.left() + sx - pos.x, prect.top() + sy - pos.y);
                            let d2 = dx * dx + dy * dy;
                            (d2 <= tap_r2(14.0)).then_some((i, d2))
                        })
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(i, _)| i)
                })
                .flatten();
            // A chase partner's dot, if they published a stream with their position.
            let peer_hit = (marker_hit.is_none() && tool == MapTool::Interrogate)
                .then(|| {
                    self.peers
                        .values()
                        .filter(|p| !p.video_url.trim().is_empty())
                        .find(|p| {
                            let w = crate::render::mercator::lonlat_to_world(p.lon, p.lat);
                            let (sx, sy) = cam.world_to_screen(w, vp);
                            let (dx, dy) = (prect.left() + sx - pos.x, prect.top() + sy - pos.y);
                            dx * dx + dy * dy <= tap_r2(12.0)
                        })
                        .map(|p| (p.name.clone(), p.video_url.trim().to_string()))
                })
                .flatten();
            // A watch zone under the click, when nothing more specific is there.
            let zone_hit =
                (marker_hit.is_none() && peer_hit.is_none() && tool == MapTool::Interrogate)
                    .then(|| {
                        self.settings
                            .alert_polygons
                            .iter()
                            .position(|z| point_in_ring_ll(&z.ring, lon, lat))
                    })
                    .flatten();
            // Interrogate + a click on a radar-site ring switches radars (storm features win,
            // handled inside try_pick_site). Consumes the click so no popup opens underneath.
            let picked_site = marker_hit.is_none()
                && tool == MapTool::Interrogate
                && self.show_radar_sites
                && self.try_pick_site(idx, pos, cam, prect, vp);
            // A camera site under an interrogate click wins over everything below it: the
            // markers are sparse, so a tap on one is never ambiguous.
            let cam_site = (marker_hit.is_none()
                && !picked_site
                && tool == MapTool::Interrogate
                && self.show_webcams)
                .then(|| {
                    self.webcams
                        .iter()
                        .find(|s| {
                            let w = crate::render::mercator::lonlat_to_world(s.lon, s.lat);
                            let (sx, sy) = cam.world_to_screen(w, vp);
                            let (dx, dy) = (prect.left() + sx - pos.x, prect.top() + sy - pos.y);
                            dx * dx + dy * dy <= tap_r2(12.0)
                        })
                        .cloned()
                })
                .flatten();
            // A live station under an interrogate click. Ranked above the FAA cameras: where
            // both sit on one airport, the card carries the camera and the telemetry.
            let station_hit = (marker_hit.is_none()
                && !picked_site
                && tool == MapTool::Interrogate
                && self.show_stations)
                .then(|| {
                    let to_screen = |lon: f64, lat: f64| {
                        let w = crate::render::mercator::lonlat_to_world(lon, lat);
                        let (sx, sy) = cam.world_to_screen(w, vp);
                        egui::pos2(prect.left() + sx, prect.top() + sy)
                    };
                    self.stations.hit(pos, tap_r2(12.0), to_screen)
                })
                .flatten();
            let cam_site = if station_hit.is_some() {
                None
            } else {
                cam_site
            };
            // A surveyed damage point under an interrogate click, same rule as the cameras.
            let dat_hit = (marker_hit.is_none()
                && !picked_site
                && cam_site.is_none()
                && tool == MapTool::Interrogate
                && self.show_dat)
                .then(|| {
                    self.dat_points
                        .iter()
                        .find(|p| {
                            let w = crate::render::mercator::lonlat_to_world(p.lon, p.lat);
                            let (sx, sy) = cam.world_to_screen(w, vp);
                            let (dx, dy) = (prect.left() + sx - pos.x, prect.top() + sy - pos.y);
                            dx * dx + dy * dy <= tap_r2(10.0)
                        })
                        .cloned()
                })
                .flatten();
            // A river gauge under an interrogate click, among the ones actually drawn (the
            // declutter hides some, and a click must not open one the map does not show).
            let gauge_hit = (marker_hit.is_none()
                && !picked_site
                && cam_site.is_none()
                && dat_hit.is_none()
                && tool == MapTool::Interrogate
                && self.show_gauges
                && cam.zoom >= 6.0)
                .then(|| {
                    self.gauges
                        .iter()
                        .filter(|g| self.labels.was_shown(crate::labelplace::key(&g.lid)))
                        .map(|g| {
                            let w = crate::render::mercator::lonlat_to_world(g.lon, g.lat);
                            let (sx, sy) = cam.world_to_screen(w, vp);
                            let (dx, dy) = (prect.left() + sx - pos.x, prect.top() + sy - pos.y);
                            (g, dx * dx + dy * dy)
                        })
                        .filter(|(_, d2)| *d2 <= tap_r2(10.0))
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(g, _)| g.clone())
                })
                .flatten();
            if let Some(g) = gauge_hit.filter(|_| station_hit.is_none()) {
                self.cell_popup = None;
                self.warning_popup = None;
                self.gate_popup = None;
                let (rt, http) = (self.spawner.clone(), self.http.clone());
                self.gauge_cards
                    .open(&g.lid, &g.name, (g.lat, g.lon), &rt, &http, ctx);
                return true;
            }
            if let Some(ob) = station_hit {
                self.cell_popup = None;
                self.warning_popup = None;
                self.gate_popup = None;
                let (rt, http) = (self.spawner.clone(), self.http.clone());
                self.stations.open_card(ob, &rt, &http, ctx);
                return true;
            }
            match tool {
                // Also catches the drop tool: a second marker within a finger's width of an
                // existing one is never what someone meant, and this makes a stray drop undoable.
                _ if marker_hit.is_some() => {
                    self.marker_popup = marker_hit;
                    self.cell_popup = None;
                    self.detail = None;
                    self.gate_popup = None;
                }
                _ if peer_hit.is_some() => {
                    let (name, url) = peer_hit.expect("checked Some");
                    self.cell_popup = None;
                    self.gate_popup = None;
                    self.watch_stream(name, url);
                }
                _ if zone_hit.is_some() => {
                    self.zone_popup = zone_hit;
                    self.cell_popup = None;
                    self.gate_popup = None;
                }
                _ if picked_site => {}
                _ if cam_site.is_some() => {
                    self.cell_popup = None;
                    self.warning_popup = None;
                    self.gate_popup = None;
                    let site = cam_site.expect("checked Some");
                    self.open_webcam(&site, ctx);
                }
                _ if dat_hit.is_some() => {
                    self.cell_popup = None;
                    self.warning_popup = None;
                    self.gate_popup = None;
                    let p = dat_hit.expect("checked Some");
                    self.open_damage_point(&p, ctx);
                }
                MapTool::Measure => {
                    if self.measure.len() >= 2 {
                        self.measure.clear();
                    }
                    self.measure.push([lon, lat]);
                    if self.measure.len() == 2 {
                        // The ground under the far end, for the beam's height above it.
                        self.request_ground(lon, lat, ctx);
                    }
                }
                MapTool::Marker => {
                    let n = self.settings.markers.len() + 1;
                    self.settings.markers.push(crate::settings::Marker {
                        id: crate::settings::new_marker_id(),
                        name: format!("Marker {n}"),
                        lat,
                        lon,
                        icon: None,
                        alert_radius_mi: crate::settings::default_alert_radius_mi(),
                        video_url: String::new(),
                        home: false,
                    });
                }
                MapTool::CrossSection => {
                    // A tap on one of the line's handles is a handle, not a new endpoint.
                    if let (Some(line), Some(p)) =
                        (self.xsection_line(), response.interact_pointer_pos())
                    {
                        let cam = self.views[idx].camera;
                        let to_px = |ll: [f64; 2]| {
                            let w = crate::render::mercator::lonlat_to_world(ll[0], ll[1]);
                            let (x, y) = cam.world_to_screen(w, vp);
                            egui::pos2(prect.left() + x, prect.top() + y)
                        };
                        let touch = ui.input(|i| i.any_touches() || i.has_touch_screen());
                        let radius = if touch { 24.0 } else { 12.0 };
                        if xsection_edit::grab_at(&line, p, radius, to_px).is_some() {
                            return true;
                        }
                    }
                    if self.xsection_pts.len() >= 2 {
                        self.xsection_pts.clear();
                    }
                    self.xsection_pts.push([lon, lat]);
                    if self.xsection_pts.len() == 2 {
                        self.build_xsection(idx, ctx);
                    }
                }
                MapTool::RegionStats => self.region_click(idx, lon, lat),
                MapTool::Sounding => self.fetch_sounding(lon, lat),
                MapTool::Forecast => self.fetch_point_forecast(lon, lat),
                MapTool::Chase => {
                    self.chase_mode = true;
                    self.chase_pos = Some((lon, lat));
                }
                MapTool::Climatology => self.query_climatology(lon, lat),
                // Drawing happens on drag, not on click; a bare click leaves no mark.
                MapTool::Draw | MapTool::StormTrack => {}
                MapTool::AlertZone => self.zone_pts.push([lon, lat]),
                MapTool::Route => {
                    self.route_window.open = true;
                    self.route_window.waypoints.push([lon, lat]);
                    self.fetch_route();
                }
                MapTool::GateInspector => {
                    self.cell_popup = None;
                    self.warning_popup = None;
                    self.detail = None;
                    // The map-pitch 3D view can show several tilts stacked in one frame, so a
                    // click there is a real 3D pick against each tilt's own beam-height
                    // surface — see `render3d::pick_observed_tilt`'s doc comment for why a
                    // flat ground-plane click (what `lon`/`lat` above already are) cannot
                    // tell those tilts apart. `SmoothVolume`/`SmoothDebris`/
                    // `SmoothSpectrumWidth` are a resampled Cartesian grid, not discrete
                    // tilts, so this only applies to `ObservedSweeps`.
                    let picked_3d = (self.views[idx].map_3d.enabled
                        && self.views[idx].map_3d.representation
                            == crate::view::Map3dRepresentation::ObservedSweeps)
                        .then(|| {
                            let v = &self.views[idx];
                            let site = v.site.as_deref().and_then(wxdata::sites::site_by_id)?;
                            let elevations = &v.volume.as_ref()?.elevations;
                            crate::render3d::pick_observed_tilt(
                                &cam,
                                px,
                                vp,
                                site.longitude as f64,
                                site.latitude as f64,
                                site.elevation_meters as f64 + wxdata::towers::tower_m(site.id),
                                v.map_3d.vertical_exaggeration as f64,
                                v.map_3d.beam_rise as f64,
                                elevations,
                            )
                        })
                        .flatten();
                    let (gate_lon, gate_lat, tilt) = match picked_3d {
                        Some((tilt, plon, plat)) => (plon, plat, Some(tilt)),
                        None => (lon, lat, None),
                    };
                    self.gate_popup = self.inspect_gate(ctx, idx, gate_lon, gate_lat, tilt);
                    // The same point across the loop this pane holds (ROADMAP_NEW C4).
                    let display = self.map_display(idx);
                    if let Some(p) = self.gate_popup.as_mut() {
                        p.display = display;
                        p.series = self.views[idx].point_series(
                            p.moment,
                            p.inspection.elevation_deg,
                            gate_lon,
                            gate_lat,
                        );
                    }
                }
                MapTool::RadarSuitability => {
                    self.cell_popup = None;
                    self.warning_popup = None;
                    self.detail = None;
                    self.suitability_popup = Some(ui::suitability_popup::SuitabilityPopup {
                        lon,
                        lat,
                        candidates: wxdata::suitability::rank(lon, lat, 0.5, 6),
                    });
                }
                // Labelled so the storm-marker shortcut below can bail out of the hit-test
                // chain without returning from `render_pane` and costing this pane its
                // tiles and radar for the frame.
                MapTool::Interrogate => 'interrogate: {
                    // A tropical cyclone's own marker sits above everything else: it is the
                    // one feature whose words matter more than its geometry, and the cone
                    // polygon underneath would otherwise swallow the click.
                    let storm_hit = self
                        .show_tropical
                        .then(|| {
                            self.tropical.as_ref().and_then(|t| {
                                t.storms
                                    .iter()
                                    .find(|s| {
                                        let w =
                                            crate::render::mercator::lonlat_to_world(s.lon, s.lat);
                                        let (sx, sy) = cam.world_to_screen(w, vp);
                                        let (dx, dy) =
                                            (prect.left() + sx - pos.x, prect.top() + sy - pos.y);
                                        dx * dx + dy * dy <= tap_r2(14.0)
                                    })
                                    .map(|s| s.id.clone())
                            })
                        })
                        .flatten();
                    if let Some(id) = storm_hit {
                        self.tropical_window.open = true;
                        self.tropical_window.storm_id = Some(id.clone());
                        let product = self.tropical_window.product;
                        self.fetch_tropical_text(&id, product);
                        // With the models loaded, the storm's own guidance is what a click
                        // on it is asking for.
                        if self.spaghetti.find(&id).is_some() {
                            self.tropical_window.tab = ui::tropical_window::Tab::Models;
                        }
                        self.gate_popup = None;
                        break 'interrogate;
                    }
                    // An invest: no advisory to read, but its models are the point.
                    let invest_hit = self
                        .show_tropical
                        .then(|| {
                            self.spaghetti.hit(pos, tap_r2(14.0), |lon, lat| {
                                let w = crate::render::mercator::lonlat_to_world(lon, lat);
                                let (sx, sy) = cam.world_to_screen(w, vp);
                                egui::pos2(prect.left() + sx, prect.top() + sy)
                            })
                        })
                        .flatten();
                    if let Some(id) = invest_hit {
                        self.tropical_window.open = true;
                        self.tropical_window.tab = ui::tropical_window::Tab::Models;
                        self.tropical_window.storm_id = Some(id);
                        self.gate_popup = None;
                        break 'interrogate;
                    }
                    // Storm reports sit on top: a click near a report dot opens its detail.
                    let report = self
                        .show_storm_reports
                        .then(|| {
                            self.active_storm_reports().iter().find(|r| {
                                let w = crate::render::mercator::lonlat_to_world(r.lon, r.lat);
                                let (sx, sy) = cam.world_to_screen(w, vp);
                                let (dx, dy) =
                                    (prect.left() + sx - pos.x, prect.top() + sy - pos.y);
                                dx * dx + dy * dy <= tap_r2(12.0)
                            })
                        })
                        .flatten()
                        .cloned();
                    // Fire incident points sit alongside the reports in the same hit test.
                    let fire = self
                        .show_fires
                        .then(|| {
                            self.fire_incidents.iter().find(|f| {
                                let w = crate::render::mercator::lonlat_to_world(f.lon, f.lat);
                                let (sx, sy) = cam.world_to_screen(w, vp);
                                let (dx, dy) =
                                    (prect.left() + sx - pos.x, prect.top() + sy - pos.y);
                                dx * dx + dy * dy <= tap_r2(12.0)
                            })
                        })
                        .flatten()
                        .cloned();
                    let air = self
                        .show_aqi
                        .then(|| {
                            self.aqi.iter().find(|o| {
                                let w = crate::render::mercator::lonlat_to_world(o.lon, o.lat);
                                let (sx, sy) = cam.world_to_screen(w, vp);
                                let (dx, dy) =
                                    (prect.left() + sx - pos.x, prect.top() + sy - pos.y);
                                dx * dx + dy * dy <= tap_r2(12.0)
                            })
                        })
                        .flatten()
                        .cloned();
                    if let Some(o) = air {
                        self.cell_popup = None;
                        self.warning_popup = None;
                        self.gate_popup = None;
                        let c = o.color();
                        self.detail = Some(Detail {
                            title: format!("AQI {} — {}", o.aqi, o.category_name()),
                            body: format!("{}\n{}", o.site, o.param),
                            color: [c[0], c[1], c[2], 255],
                            image: None,
                            link: None,
                        });
                    } else if let Some(f) = fire {
                        self.cell_popup = None;
                        self.warning_popup = None;
                        self.gate_popup = None;
                        self.detail = Some(Detail {
                            title: format!("{} Fire", f.name),
                            body: format!(
                                "{}\n{}",
                                f.acres
                                    .map(|a| format!("{a:.0} acres"))
                                    .unwrap_or_else(|| "size not reported".to_string()),
                                f.containment
                                    .map(|c| format!("{c:.0}% contained"))
                                    .unwrap_or_else(|| "containment not reported".to_string()),
                            ),
                            color: [235, 110, 40, 255],
                            image: None,
                            link: None,
                        });
                    } else if let Some(r) = report {
                        self.cell_popup = None;
                        self.warning_popup = None;
                        self.gate_popup = None;
                        self.detail = Some(Detail {
                            title: format!("{} Report — {}", r.kind.label(), r.magnitude),
                            body: format!(
                                "{}, {}\nCounty: {}\nTime: {}Z\n\n{}",
                                r.location, r.state, r.county, r.time, r.comments
                            ),
                            color: report_color(r.kind),
                            image: None,
                            link: None,
                        });
                    } else {
                        let cell_hit = self.filters.show_cells
                            && self.cells_site.as_deref() == self.views[idx].site.as_deref()
                            && !self.active_storm_cells().is_empty();
                        let picked = cell_hit
                            .then(|| {
                                self.active_storm_cells().iter().find(|c| {
                                    let w = crate::render::mercator::lonlat_to_world(c.lon, c.lat);
                                    let (sx, sy) = cam.world_to_screen(w, vp);
                                    let (dx, dy) =
                                        (prect.left() + sx - pos.x, prect.top() + sy - pos.y);
                                    dx * dx + dy * dy <= tap_r2(14.0)
                                })
                            })
                            .flatten()
                            .cloned();
                        match picked {
                            // A storm cell with an id opens the attributes window; a standalone
                            // detection (empty id) falls back to a generic detail popup.
                            Some(c) if !c.id.is_empty() => {
                                self.detail = None;
                                self.gate_popup = None;
                                self.select_storm(c);
                            }
                            Some(c) => {
                                self.cell_popup = None;
                                self.gate_popup = None;
                                self.detail = Some(Detail {
                                    title: c.title.clone(),
                                    body: c.summary(),
                                    color: cell_color(c.kind),
                                    image: None,
                                    link: None,
                                });
                            }
                            None => {
                                // Warnings/watches open the warning window (deduped by alert id
                                // across MultiPolygon parts); other features use the generic popup.
                                // An imported layer hidden below its minimum zoom is not
                                // there to click either.
                                self.feature_chooser = None;
                                let hits = self.overlay_hits(
                                    lon,
                                    lat,
                                    self.views[self.active].camera.zoom,
                                );
                                let mut seen = std::collections::HashSet::new();
                                let mut cards: Vec<ui::warning_window::WarnCard> = hits
                                    .iter()
                                    .filter_map(|f| f.alert.as_ref().map(|a| (a, f.stroke)))
                                    .filter(|(a, _)| seen.insert(a.id.clone()))
                                    .map(|(a, color)| ui::warning_window::WarnCard {
                                        info: a.clone(),
                                        color,
                                    })
                                    .collect();
                                // The bulletin that opens is the one that matters most where
                                // polygons overlap: an emergency before a plain warning.
                                cards.sort_by_key(|c| {
                                    std::cmp::Reverse((
                                        wxdata::alerts::escalation(&c.info),
                                        ui::alert_panel::severity_rank(&c.info.event),
                                    ))
                                });
                                // An imported point or line under the click (drawn over the
                                // polygons) opens its attributes, unless an alert is there.
                                let marks = if cards.is_empty() {
                                    let cam = self.views[self.active].camera;
                                    let touch =
                                        ctx.input(|i| i.any_touches() || i.has_touch_screen());
                                    self.gis_mark_hits(lon, lat, &cam, touch)
                                } else {
                                    Vec::new()
                                };
                                // More than one feature (and no alert) under the click: list
                                // them rather than open only the topmost.
                                let choices = if cards.is_empty() {
                                    self.click_choices(&marks, &hits)
                                } else {
                                    Vec::new()
                                };
                                let mark = marks.into_iter().next();
                                if choices.len() > 1 {
                                    self.detail = None;
                                    self.warning_popup = None;
                                    self.gate_popup = None;
                                    self.feature_chooser = Some(choices);
                                } else if let Some((detail, layer, src)) = mark {
                                    self.note_gis_pick(layer, src);
                                    self.warning_popup = None;
                                    self.gate_popup = None;
                                    self.detail_impact = None;
                                    self.detail = Some(detail);
                                } else if !cards.is_empty() {
                                    self.detail = None;
                                    self.gate_popup = None;
                                    // Open straight to the full bulletin of the top alert; the
                                    // Back button reveals the stack when polygons overlap.
                                    self.warning_popup = Some(ui::warning_window::WarningPopup {
                                        cards,
                                        selected: Some(0),
                                        at: self.alerts_at(),
                                    });
                                } else if let Some(f) = hits.first().map(|f| (*f).clone()) {
                                    let zoom = self.views[self.active].camera.zoom;
                                    if let Some((layer, src)) =
                                        self.overlay_hit_source(lon, lat, zoom)
                                    {
                                        self.note_gis_pick(layer, src);
                                    }
                                    self.warning_popup = None;
                                    self.gate_popup = None;
                                    // Discussions and watches get the same people count as
                                    // an alert card.
                                    self.detail_impact = matches!(
                                        f.kind,
                                        overlay::FeatureKind::MesoDiscussion
                                            | overlay::FeatureKind::Watch
                                            | overlay::FeatureKind::WatchBox
                                    )
                                    .then(|| format!("feature:{}", f.title));
                                    if let Some(key) = self.detail_impact.clone() {
                                        if !self.impacts.by_id.contains_key(&key) {
                                            let rings = f.rings.clone();
                                            self.request_impact(key, rings, ctx);
                                        }
                                    }
                                    self.detail = Some(Detail {
                                        title: f.title.clone(),
                                        body: f.detail.clone(),
                                        color: f.stroke,
                                        image: None,
                                        link: None,
                                    });
                                } else {
                                    // Explore only opens features. Gate sampling is an explicit
                                    // tool, so a normal map click does not open the inspector.
                                    self.warning_popup = None;
                                    self.detail = None;
                                    self.gate_popup = None;
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    }
}

fn point_in_ring_ll(ring: &[[f64; 2]], lon: f64, lat: f64) -> bool {
    wxdata::overlay::rings_intersect(
        ring,
        // A tiny square around the click: reuses the one geometry primitive rather than adding a
        // second point-in-polygon implementation here.
        &[
            [lon - 1e-6, lat - 1e-6],
            [lon + 1e-6, lat - 1e-6],
            [lon + 1e-6, lat + 1e-6],
            [lon - 1e-6, lat + 1e-6],
        ],
    )
}
