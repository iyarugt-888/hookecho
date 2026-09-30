//! Chasing: your position and its HUD, offline chase packs, and the max-value trail with its
//! export. Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Chase-mode follow-me: when the tracked position changes, hand the active pane off to the
    /// nearest NEXRAD site and recenter on the position. Applied once per position change.
    pub(crate) fn apply_chase(&mut self) {
        // Drain any live gpsd fixes into the tracked position (newest wins).
        if let Some(rx) = &self.gps_rx {
            let mut latest = None;
            while let Ok(pos) = rx.try_recv() {
                latest = Some(pos);
            }
            if let Some((lon, lat)) = latest {
                self.chase_pos = Some((lon, lat));
                // The chase log records every fix the app receives, whether or not follow-me is
                // driving the camera: the track is what you did, not what the map did.
                if self.settings.chase_log {
                    self.chase_track.push(lon, lat, crate::share::now());
                }
            }
        }
        if !self.chase_mode {
            self.chase_applied = None;
            return;
        }
        let Some((lon, lat)) = self.chase_pos else {
            return;
        };
        if self.chase_applied == Some((lon, lat)) {
            return;
        }
        if let Some(site) = crate::geo::nearest_site_id(lon, lat) {
            let zoom = self.views[self.active].camera.zoom.max(8.0);
            self.goto_view(&site, lon, lat, zoom, None);
        }
        self.chase_applied = Some((lon, lat));
        self.warm_next_site();
    }

    /// Fixed chase-pack zoom span for the current view: `z_lo = floor(zoom)`, four levels deeper,
    /// both capped to the active basemap's max (zooming past the style's deepest level packs that
    /// deepest level instead of an empty range).
    pub(crate) fn chasepack_zoom(&self) -> (u8, u8) {
        use crate::tiles::BasemapStyle;
        let style = self.views[self.active].basemap.resolve(true);
        let z_lo = (self.views[self.active].camera.zoom.floor() as i64).clamp(2, 18) as u8;
        let max_z = if style.is_raster() {
            self.tiles.max_pack_z(style)
        } else if matches!(style, BasemapStyle::Dark | BasemapStyle::Light) {
            self.vtiles.max_pack_z()
        } else {
            z_lo
        };
        let z_lo = z_lo.min(max_z);
        (z_lo, (z_lo + 4).min(max_z))
    }

    /// Per-frame chase-pack estimate + progress [`map_rows`](Self::map_rows) renders.
    pub(crate) fn chasepack_ui(&self) -> ui::layer_options::ChasePackUi {
        use crate::tiles::BasemapStyle;
        // Dark and Light pack the same vector tiles (the `.pbf` cache is palette-agnostic), so
        // resolving `Auto` either way gives the same pack.
        let style = self.views[self.active].basemap.resolve(true);
        let packable = !cfg!(target_arch = "wasm32")
            && if style.is_raster() {
                self.tiles.packable(style)
            } else if matches!(style, BasemapStyle::Dark | BasemapStyle::Light) {
                self.vtiles.packable()
            } else {
                false
            };
        let (z_lo, z_hi) = self.chasepack_zoom();
        let tiles = if packable {
            let (min_lon, min_lat, max_lon, max_lat) = self.view_bounds();
            let mut n =
                crate::tiles::pack_tile_count(min_lon, min_lat, max_lon, max_lat, z_lo, z_hi);
            // The extras the pack quietly adds, so the estimate matches what downloads: terrain
            // at whichever DEM zoom is set, and streets beside raster imagery.
            let dem_z = if self.settings.pack_hires_dem {
                crate::elevation::DEM_ZOOM_HIRES
            } else {
                crate::elevation::DEM_ZOOM
            };
            n += crate::tiles::pack_tile_count(min_lon, min_lat, max_lon, max_lat, dem_z, dem_z);
            if style.is_raster() && self.settings.pack_include_vector {
                let vz_hi = z_hi.min(self.vtiles.max_pack_z());
                n += crate::tiles::pack_tile_count(
                    min_lon,
                    min_lat,
                    max_lon,
                    max_lat,
                    z_lo.min(vz_hi),
                    vz_hi,
                );
            }
            if !style.is_raster() && self.settings.pack_include_satellite {
                let sz_hi = z_hi.min(self.tiles.max_pack_z(BasemapStyle::HybridSatellite));
                n += crate::tiles::pack_tile_count(
                    min_lon,
                    min_lat,
                    max_lon,
                    max_lat,
                    z_lo.min(sz_hi),
                    sz_hi,
                );
            }
            n
        } else {
            0
        };
        // ponytail: 25 KB/tile average across raster + vector; only used for the "≈ MB" hint.
        let mb = tiles as f64 * 25_000.0 / 1e6;
        let progress = self
            .chasepack
            .as_ref()
            .map(|p| (p.done, p.total, p.errors, p.bytes as f64 / 1e6));
        ui::layer_options::ChasePackUi {
            tiles,
            mb,
            packable,
            z_lo,
            z_hi,
            progress,
        }
    }

    /// Kick off an offline chase-pack download of the current view's basemap tiles (4 workers).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn start_chasepack(&mut self) {
        use crate::tiles::BasemapStyle;
        if self.chasepack.is_some() {
            return;
        }
        let style = self.views[self.active].basemap.resolve(true);
        let (z_lo, z_hi) = self.chasepack_zoom();
        let (min_lon, min_lat, max_lon, max_lat) = self.view_bounds();
        // The DEM resolution is a per-session choice; make sure the pack fetches what the sampler
        // will later read.
        crate::elevation::set_hires(self.settings.pack_hires_dem);
        let mut jobs = if style.is_raster() {
            self.tiles
                .pack_jobs(style, min_lon, min_lat, max_lon, max_lat, z_lo, z_hi)
        } else if matches!(style, BasemapStyle::Dark | BasemapStyle::Light) {
            self.vtiles
                .pack_jobs(min_lon, min_lat, max_lon, max_lat, z_lo, z_hi)
        } else {
            Vec::new()
        };
        // Streets alongside the imagery: a raster pack used to be either/or, which left an
        // offline chaser with satellite pictures and no road names. Vector tiles cap at their own
        // max zoom, so this asks for what exists rather than the raster range.
        if style.is_raster() && self.settings.pack_include_vector {
            let vz_hi = z_hi.min(self.vtiles.max_pack_z());
            let vz_lo = z_lo.min(vz_hi);
            jobs.extend(
                self.vtiles
                    .pack_jobs(min_lon, min_lat, max_lon, max_lat, vz_lo, vz_hi),
            );
        }
        // And imagery alongside the streets, for the packs that went the other way round.
        if !style.is_raster() && self.settings.pack_include_satellite {
            let sat = BasemapStyle::HybridSatellite;
            let sz_hi = z_hi.min(self.tiles.max_pack_z(sat));
            jobs.extend(self.tiles.pack_jobs(
                sat,
                min_lon,
                min_lat,
                max_lon,
                max_lat,
                z_lo.min(sz_hi),
                sz_hi,
            ));
        }
        // The DEM rides along with every pack, whatever the basemap: offline chase mode wants the
        // blockage overlay as much as it wants the map under it.
        jobs.extend(self.tiles.dem_pack_jobs(min_lon, min_lat, max_lon, max_lat));
        if jobs.is_empty() {
            return;
        }
        let total = jobs.len() as u64;
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        crate::tiles::start_pack_download(self._rt.handle(), jobs, cancel.clone(), tx);
        self.chasepack = Some(ChasePack {
            rx,
            cancel,
            total,
            done: 0,
            errors: 0,
            bytes: 0,
        });
    }

    /// Chase HUD: the storm-relative numbers a chaser in motion actually needs — where the storm
    /// is, how close it will come and when, and which way to drive to get off its path. Display
    /// only; the arrival cones and NWS warnings already own the alarms.
    pub(crate) fn chase_hud(&mut self, ctx: &egui::Context) {
        let metric = self.metric();
        if !self.chase_mode {
            return;
        }
        let Some((lon, lat)) = self.chase_pos else {
            return;
        };
        let me = [lon, lat];
        // Prefer the cell the camera is following, else the nearest tracked cell within 300 km.
        let cell = match &self.follow_cell {
            Some((_, c, _)) => Some(c.clone()),
            None => nearest_cell(self.active_storm_cells(), lon, lat, 300.0).cloned(),
        };
        let Some(c) = cell else { return };
        let (km, bearing) = crate::geo::great_circle(me, [c.lon, c.lat]);
        let dir = c.mvt_deg.unwrap_or(0.0) as f64;
        let kt = c.mvt_kt.unwrap_or(0.0) as f64;
        let (close_km, close_min) =
            crate::geo::closest_approach([c.lon, c.lat], dir, kt, me, 120.0);
        let escape = crate::geo::escape_bearing([c.lon, c.lat], dir, me);
        // Spoken position updates: only while chasing, only when the picture has actually
        // changed, and never more than once a minute — a voice that repeats itself is a voice the
        // user turns off. Reported in whole miles, so a storm parked in place says nothing.
        if self.settings.speak_position && !self.settings.mute_alerts {
            let whole = if metric {
                km.round() as i32
            } else {
                (km / crate::geo::KM_PER_MILE).round() as i32
            };
            let fresh = self.spoke_pos.is_none_or(|(t, m)| {
                m != whole && t.elapsed() >= std::time::Duration::from_secs(60)
            });
            if fresh {
                self.spoke_pos = Some((Instant::now(), whole));
                crate::speech::speak(&wxdata::spoken::position_script(
                    // "Cell O7", not the bare id — a synthesizer reading "O7" alone is a noise.
                    &if c.id.is_empty() {
                        c.title.clone()
                    } else {
                        format!("Cell {}", c.id)
                    },
                    bearing as f32,
                    km,
                    c.mvt_deg,
                    metric,
                ));
            }
        }
        // Urgent when the storm will be on top of you soon. The threshold stays in kilometres
        // whatever the card reads in: five miles of warning is five miles of warning in Bavaria.
        let urgent = close_km < 8.05 && close_min < 20.0;
        let accent = crate::theme::accent(self.settings.theme);
        let red = egui::Color32::from_rgb(230, 70, 70);
        let inset_bottom = (ctx.viewport_rect().bottom() - ctx.content_rect().bottom()).max(0.0);
        // Android floats the bottom card at the same edge; sit above it.
        let dy = if crate::platform::phone_layout() {
            -(inset_bottom + 190.0)
        } else {
            // Above the product pill, which owns the bottom-left corner.
            crate::ui::style::LANE_BOTTOM_CHASE
        };
        egui::Area::new(egui::Id::new("chase_hud"))
            .constrain_to(self.chrome_rect)
            // Pure readout — never take input, or it kills pinch over its corner of the map.
            .interactable(false)
            .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(14.0, dy))
            .show(ctx, |ui| {
                let frame = mobile::glass(ui, 244).stroke(egui::Stroke::new(
                    if urgent { 2.0 } else { 1.0 },
                    if urgent {
                        red
                    } else {
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 22)
                    },
                ));
                frame.show(ui, |ui| {
                    ui.set_width(226.0);
                    let head = if c.id.is_empty() {
                        "Storm".to_string()
                    } else {
                        format!("Storm {}", c.id)
                    };
                    ui.label(
                        egui::RichText::new(head)
                            .size(14.0)
                            .strong()
                            .color(if urgent { red } else { accent }),
                    );
                    let row = |ui: &mut egui::Ui, k: &str, v: String| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(k)
                                    .size(12.0)
                                    .color(egui::Color32::from_gray(160)),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(v)
                                            .size(13.0)
                                            .strong()
                                            .color(egui::Color32::from_gray(235)),
                                    );
                                },
                            );
                        });
                    };
                    row(
                        ui,
                        "Now",
                        format!(
                            "{} {} ({:.0}°)",
                            crate::geo::fmt_distance(km, metric, 1),
                            cardinal(bearing),
                            bearing
                        ),
                    );
                    if kt > 1.0 {
                        row(
                            ui,
                            "Closest",
                            format!(
                                "{} in {close_min:.0} min",
                                crate::geo::fmt_distance(close_km, metric, 1)
                            ),
                        );
                        row(
                            ui,
                            "Escape",
                            format!("{} ({:.0}°)", cardinal(escape), escape),
                        );
                        row(ui, "Motion", format!("{} at {kt:.0} kt", cardinal(dir)));
                    } else {
                        row(ui, "Motion", "stationary".into());
                    }
                    if let Some(site) = crate::geo::nearest_site_id(lon, lat) {
                        row(ui, "Radar", site);
                    }
                });
            });
        if urgent {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    /// C2: bring the active pane's extremum trail up to date and return the tag that makes the
    /// shown-image key change as it grows, or `None` when there is nothing to draw.
    ///
    /// The trail is built from volumes already in the decode cache, oldest first, and at most
    /// [`Self::TRAIL_FOLDS_PER_FRAME`] are folded per UI frame: binning a sweep is real work, and a
    /// two-hour window is two dozen of them. The image grows over a few frames instead of
    /// stalling one. A window that slides (a live arrival, a scrub) cannot un-fold its oldest
    /// frame, so a change to the oldest wanted volume starts the trail over.
    pub(crate) fn advance_trail(
        &mut self,
        data: usize,
        moment: Moment,
        tilt: usize,
    ) -> Option<String> {
        use wxdata::extrema::{self, Extremum, Merge};
        let window = self.filters.trail_window_min;
        let keep = if self.filters.trail_keep_min {
            Extremum::Min
        } else {
            Extremum::Max
        };
        // This runs every UI frame while the layer is on, so names are cloned only for the frames
        // inside the window rather than for the whole day's timeline.
        let (newest, in_window): (Option<DateTime<Utc>>, Vec<String>) = {
            let tl = &self.views[data].timeline;
            let upto = &tl.frames[..(tl.playhead + 1).min(tl.frames.len())];
            let newest = upto.iter().rev().find_map(|id| id.date_time());
            let cutoff = newest.map(|n| n - chrono::Duration::minutes(i64::from(window)));
            let names = upto
                .iter()
                .filter(|id| id.date_time().zip(cutoff).is_some_and(|(t, c)| t >= c))
                .map(|id| id.name().to_string())
                .collect();
            (newest, names)
        };
        if newest.is_none() {
            self.trail = None;
            self.trail_more = false;
            return None;
        }
        let wanted: Vec<String> = in_window
            .into_iter()
            .filter(|name| self.scan_cache.contains(name))
            .collect();
        let Some(oldest) = wanted.first().cloned() else {
            self.trail = None;
            self.trail_more = false;
            return None;
        };
        let decaying = self.filters.trail_decay;
        let key: TrailKey = (data, moment, tilt, keep, window, decaying, oldest);
        let stale = self
            .trail
            .as_ref()
            .is_none_or(|t| t.key != key || !wanted.starts_with(&t.folded));
        if stale {
            self.trail = Some(TrailState {
                key,
                folded: Vec::new(),
                acc: None,
                generation: self
                    .trail
                    .as_ref()
                    .map_or(0, |t| t.generation.wrapping_add(1)),
                restarted: None,
                last_time: None,
            });
        }
        let state = self.trail.as_mut()?;
        let start = state.folded.len();
        for name in wanted.iter().skip(start).take(Self::TRAIL_FOLDS_PER_FRAME) {
            // Recorded as folded even when it cannot be binned, so one bad volume is skipped
            // once rather than retried every frame.
            state.folded.push(name.clone());
            state.generation = state.generation.wrapping_add(1);
            let Some(scan) = self.scan_cache.peek(name).map(Arc::clone) else {
                continue;
            };
            let sweep = match level2::bin_scan(&scan, moment, tilt) {
                Ok(s) => s,
                Err(e) => {
                    log::debug!("trail: skipping {name}: {e}");
                    continue;
                }
            };
            let taken = self.views[data]
                .timeline
                .frames
                .iter()
                .find(|id| id.name() == name.as_str())
                .and_then(|id| id.date_time());
            state.last_time = taken.or(state.last_time);
            match state.acc.as_mut() {
                None => state.acc = Some(extrema::start(&sweep)),
                Some(acc) => {
                    if decaying {
                        if let (Some(then), Some(now)) = (state.last_time, taken) {
                            let min = (now - then).num_seconds() as f32 / 60.0;
                            extrema::decay(acc, keep, extrema::decay_codes(min, window));
                        }
                    }
                    if let Merge::Reset(why) = extrema::accumulate(acc, &sweep, keep) {
                        state.restarted = Some(why);
                        *acc = extrema::start(&sweep);
                    }
                }
            }
        }
        self.trail_more = state.folded.len() < wanted.len();
        let folded = state.folded.len();
        let restarted = state.restarted;
        let generation = state.generation;
        let has_image = state.acc.is_some();
        self.filters.trail_status =
            trail_status_line(folded, wanted.len(), window, keep, restarted);
        if decaying {
            self.filters.trail_status.push_str(", older part faded");
        }
        has_image.then(|| format!("trail{generation}"))
    }

    /// Write the current max/min trail out (ROADMAP_NEW C2): as a GeoTIFF of its values on a
    /// lat/lon grid, or as GeoJSON outlines of the path at the pane's value threshold — or, with
    /// none set, at [`trail_outline_level`] for the product.
    pub(crate) fn export_trail(&mut self, raster: bool) {
        use wxdata::extrema::Extremum;
        let v = &self.views[self.active];
        let time = v
            .timeline
            .current()
            .and_then(|id| id.date_time())
            .unwrap_or_else(Utc::now);
        let (moment, threshold) = (v.moment, v.active_threshold());
        let Some(state) = self.trail.as_ref() else {
            self.toast(
                ToastKind::Error,
                "No trail yet \u{2014} turn on the Max/min trail layer",
            );
            return;
        };
        let keep = state.key.3;
        let window = state.key.4;
        let Some(grid) = state
            .acc
            .as_ref()
            .and_then(|acc| wxdata::derived::sweep_grid(acc, time))
        else {
            self.toast(ToastKind::Error, "The trail has nothing to export yet");
            return;
        };
        let what = match keep {
            Extremum::Max => "max",
            Extremum::Min => "min",
        };
        let slug = format!(
            "trail-{}-{what}-{window}min-{}",
            moment.short_name().to_lowercase(),
            time.format("%Y%m%dT%H%MZ")
        );
        let (bytes, ext, count) = if raster {
            let desc = format!(
                "HookEcho {what} {} trail, {window} min ending {}",
                moment.short_name(),
                time.to_rfc3339()
            );
            match wxdata::geotiff::write(&grid, &desc) {
                Some(b) => (b, "tif", None),
                None => {
                    self.toast(ToastKind::Error, "Could not encode the trail");
                    return;
                }
            }
        } else {
            let level = threshold.unwrap_or_else(|| trail_outline_level(moment, keep));
            let rings = wxdata::extrema::outline(&grid, level, keep);
            let features: Vec<wxdata::gis::GisFeature> = rings
                .into_iter()
                .map(|r| wxdata::gis::GisFeature {
                    geometry: wxdata::gis::Geometry::LineString(
                        r.into_iter().map(|(x, y)| [x, y]).collect(),
                    ),
                    properties: serde_json::json!({
                        "hookecho": "trail-outline",
                        "product": moment.short_name(),
                        "keep": what,
                        "level": level,
                        "window_min": window,
                        "ending_utc": time.to_rfc3339(),
                    })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
                })
                .collect();
            let n = features.len();
            if n == 0 {
                self.toast(
                    ToastKind::Error,
                    format!("Nothing in the trail reaches {level:.2} \u{2014} lower the threshold"),
                );
                return;
            }
            (
                wxdata::gis::to_geojson(&features).into_bytes(),
                "geojson",
                Some(n),
            )
        };
        match crate::dialog::save_bytes(&format!("{slug}.{ext}"), ext, &bytes) {
            crate::dialog::Saved::Where(w) => self.toast(
                ToastKind::Success,
                match count {
                    Some(n) => format!("Trail outline ({n} shapes) saved to {w}"),
                    None => format!("Trail saved to {w}"),
                },
            ),
            crate::dialog::Saved::Failed(e) => {
                self.toast(ToastKind::Error, format!("Trail export failed: {e}"))
            }
            crate::dialog::Saved::Cancelled => {}
        }
    }
}
