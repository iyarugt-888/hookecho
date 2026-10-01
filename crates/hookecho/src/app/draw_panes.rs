//! The panes themselves: the shared label set, the central panel, the pane grid and each pane's
//! render.
//! Moved out of `ui_frame` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    pub(crate) fn draw_panes(&mut self, ctx: &egui::Context, root: &mut egui::Ui) {
        let placefile_labels = self.placefile_labels_cached();
        // One occupancy set for the whole frame, across every pane and every label layer. Panes
        // occupy disjoint screen rects, so sharing it between them costs nothing and saves
        // resetting it per pane.
        self.labels.begin();
        egui::CentralPanel::default().show(root, |ui| {
            let full = ui.available_rect_before_wrap();
            let n = self.views.len();
            // A phone shows one pane at a time. Two 400x400 pt panes stacked is two views of
            // nothing; the pane strip above the scrubber is how you get to the others.
            // Only where one pane is all that fits: a tablet shows the split.
            let solo = cfg!(target_os = "android") && n > 1 && chrome::compact(ctx);
            let rects = if solo {
                vec![full; n]
            } else {
                arranged_pane_rects(full, n, self.pane_layout)
            };

            self.step_camera_flights(&rects, ctx); // jumps fly there (ROADMAP_2 §4.4)
                                                   // If cameras are linked, mirror the active pane's camera to the others.
            if self.link_cameras {
                let cam = self.views[self.active.min(n - 1)].camera;
                for v in &mut self.views {
                    v.camera = cam;
                }
            }

            // Each pane fetches and draws its own `View::basemap` (see `render_pane`). Two things
            // stay global for now: the GOES frame cursor and the vector palette, both driven by
            // the active pane.
            // ponytail: one GOES cursor + one vector palette for all panes; split them when
            // someone actually wants two satellite times or two vector palettes side by side.
            use crate::tiles::BasemapStyle;
            let style = self.views[self.active.min(n - 1)]
                .basemap
                .resolve(ctx.theme() == egui::Theme::Dark);
            let is_vector = style.vector_palette().is_some();
            let raster_style = if style.is_raster() {
                style
            } else {
                BasemapStyle::None
            };
            self.tiles
                .set_keys(&self.settings.mapbox_key, &self.settings.maptiler_key);
            self.tiles
                .set_custom_template(&self.settings.custom_tile_url);
            self.tiles.set_custom_max_z(self.settings.custom_tile_max_z);
            // Ask for `@2x` tiles where the provider serves them: same tile count, twice the
            // pixels, labels drawn for the density instead of magnified. Off on a metered link —
            // a double-resolution tile is roughly double the bytes.
            let mut clear_tiles = self
                .tiles
                .set_retina(ctx.pixels_per_point() > 1.0 && !crate::platform::is_metered());
            // GOES sub-hourly scrub: fetch the available frame times when a GOES style becomes
            // active, and apply the selected frame (None = latest).
            if raster_style.timed() {
                // Which hour of imagery to ask for: the pane's own clock when it is replaying an
                // archive, otherwise the live window ending now. GIBS keeps GeoColor about two
                // weeks and Band 13 several months, so a replayed event usually has satellite.
                let radar_time = self.linked_analysis_time().or_else(|| {
                    self.views[self.active.min(n - 1)]
                        .volume
                        .as_ref()
                        .map(|v| v.time)
                });
                let (hour, from, to) = crate::tiles::goes_window(chrono::Utc::now(), radar_time);
                if self.goes_times_style != Some(raster_style) || self.goes_hour != hour {
                    self.goes_times_style = Some(raster_style);
                    self.goes_hour = hour;
                    self.goes_times.clear();
                    self.goes_time_idx = None;
                    let (tx, rx) = std::sync::mpsc::channel();
                    self.goes_times_rx = Some(rx);
                    let http = self.http.clone();
                    self.spawner.spawn(async move {
                        let times =
                            crate::tiles::fetch_frame_times(&http, raster_style, from, to, 48)
                                .await;
                        let _ = tx.send(times);
                    });
                }
                // Following the radar is the default: scrubbing back through an event should
                // take the satellite back with it, which is the whole reason to look at both.
                // Stepping the GOES arrows by hand drops out of it.
                let selected = if self.goes_follow_radar {
                    radar_time.and_then(|t| nearest_goes(&self.goes_times, t))
                } else {
                    self.goes_time_idx
                        .and_then(|i| self.goes_times.get(i).copied())
                };
                // `None` means GIBS's own default, which is the newest imagery there is — right
                // for a live pane, three days wrong for one replaying an archive. In an archive
                // window, fall back to the newest frame *in that window* instead.
                let selected = match (selected, self.goes_hour) {
                    (None, Some(_)) => self.goes_times.last().copied(),
                    (s, _) => s,
                };
                clear_tiles |= self.tiles.set_goes_time(selected);
            } else if self.goes_times_style.is_some() {
                self.goes_times_style = None;
                self.goes_hour = None;
                self.goes_times.clear();
                clear_tiles |= self.tiles.set_goes_time(None);
            }
            let mut clear_vector = false;
            if is_vector {
                clear_vector |= self
                    .vtiles
                    .set_style(style.vector_palette().unwrap_or_default());
                clear_vector |= self.vtiles.set_theme(self.settings.theme);
            }
            self.last_viewport = rects
                .get(self.active)
                .map_or((full.width(), full.height()), |r| (r.width(), r.height()));

            // Which pane carries the once-per-frame work (tile-cache clears, the shared label
            // pass): the first one actually drawn, which under `solo` is the active one.
            let head = if solo { self.active.min(n - 1) } else { 0 };
            // Cleared before every pane gets a chance to re-set it this frame (`render_pane`'s
            // hover block does, when `link_cursor` is on): leaving the map area with no pane
            // hovered must drop the shared point rather than leave the last one drawn everywhere.
            self.linked_probe = None;
            self.follow_linked_storm();
            for (i, prect) in rects.iter().enumerate() {
                if solo && i != head {
                    continue;
                }
                let first = i == head;
                self.render_pane(
                    ui,
                    ctx,
                    i,
                    *prect,
                    clear_tiles && first,
                    clear_vector && first,
                    first,
                    solo || i + 1 == n,
                    &placefile_labels,
                );
            }

            self.paint_storm_tracks(ui, &rects);
            self.paint_scale_bar(ui, &rects);
            self.paint_linked_time_badges(ui, &rects, solo);
            self.paint_linked_cursor(ui, &rects, solo);
            self.paint_layer_probe(ui, &rects);
            self.paint_selected_storm(ui, &rects, solo);

            // Pane borders; the active pane gets an accent outline. Nothing to outline under
            // `solo` — there is one pane on screen and the strip says which.
            if n > 1 && !solo {
                for (i, prect) in rects.iter().enumerate() {
                    let (w, col) = if i == self.active {
                        (2.0, crate::theme::accent(self.settings.theme))
                    } else {
                        (1.0, egui::Color32::from_gray(60))
                    };
                    ui.painter().rect_stroke(
                        *prect,
                        0.0,
                        egui::Stroke::new(w, col),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        });
    }
}
