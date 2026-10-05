//! A pane's GPU work: which basemap tiles and vector tiles are visible and uploaded, field
//! layer uploads and eviction, the radar upload, the 3D volume and the paint callbacks. Moved
//! out of `render_pane` unchanged (ROADMAP_2 §7); it returns the city and town labels it
//! gathered for the overlay pass.

use super::*;

impl HookEchoApp {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn pane_gpu(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        idx: usize,
        prect: egui::Rect,
        vp: (f32, f32),
        (clear_tiles, clear_vector, first, last): (bool, bool, bool, bool),
        (pane_style, is_raster, is_vector): (crate::tiles::BasemapStyle, bool, bool),
    ) -> std::sync::Arc<[crate::vector_tiles::PlaceLabel]> {
        // --- Tiles (shared caches, per-pane visible list) ---
        let cam = self.views[idx].camera;
        // High-DPI screens render a 256-px raster tile across `ppp`× more physical pixels, which
        // looks blurry (bad on the S24's ~3.75× density). Fetch `round(log2(ppp))` levels deeper so
        // tiles land near 1:1. Desktop (ppp 1) → +0.
        //
        // Capped at +1, not +2: each level quadruples the tiles covering the same ground, so +2
        // asked a phone to fetch, decode, and hold 16× the imagery of a desktop for a screen that
        // is a few inches across. +1 is 4×, still sharper than the panel resolves.
        // A metered link drops to +0: the deeper level is a sharpness nicety, and it costs four
        // tile downloads for every one.
        let bias_cap = if crate::platform::is_metered() {
            0.0
        } else {
            1.0
        };
        let raster_bias = if pane_style.tiles_are_512() || self.tiles.is_retina(pane_style) {
            // 512-px and `@2x` providers already carry the extra detail in the tile itself;
            // biasing on top would fetch four of them per screen tile for nothing.
            0.0
        } else {
            ctx.pixels_per_point()
                .max(1.0)
                .log2()
                .round()
                .clamp(0.0, bias_cap) as f64
        };
        let visible = if is_raster {
            let vis = self.tiles.visible(pane_style, &cam, vp, raster_bias);
            self.tiles.request_missing(pane_style, &vis);
            self.tiles.promote_visible(pane_style, &vis);
            vis
        } else {
            Vec::new()
        };
        // Always run the vector-tile pipeline for its city/town labels — raster basemaps (satellite)
        // bake in faint labels that are hard to read, so we overlay crisp haloed ones. Only the
        // *geometry* is basemap-specific: vector basemaps draw it, raster keeps its own imagery.
        let (visible_vector, vlabels, visible_vector_tiles) = {
            let vis = self.vtiles.visible(&cam, vp);
            self.vtiles.request_missing(&vis);
            let ids: Vec<crate::render::TileId> = vis.iter().map(|v| v.id).collect();
            // Deep-copying every visible place name every frame (for every pane) showed up at the
            // 4-10 fps the phone runs at. The set only changes when the visible tiles do, or when
            // a tile's labels finish tessellating — both bump the key below.
            // Compared before it is built: the key holds a `Vec` of every visible tile id, and
            // cloning it to ask "did this change" was itself a per-pane, per-frame allocation.
            let gen = self.vtiles.label_generation();
            let stale = self
                .vlabel_cache
                .as_ref()
                .is_none_or(|((k_ids, k_gen), _)| *k_gen != gen || *k_ids != ids);
            if stale {
                let labels: std::sync::Arc<[crate::vector_tiles::PlaceLabel]> = self
                    .vtiles
                    .labels_for(ids.iter())
                    .into_iter()
                    .cloned()
                    .collect();
                self.vlabel_cache = Some(((ids.clone(), gen), labels));
            }
            // An `Arc` handle, not a deep copy of every visible place name — the cache existed to
            // stop the *lookup* running every frame, but the copy it returned survived it.
            let labels = self
                .vlabel_cache
                .as_ref()
                .map(|(_, l)| l.clone())
                .unwrap_or_else(|| Vec::new().into());
            (if is_vector { ids } else { Vec::new() }, labels, vis)
        };
        // Drain finished fetches once (on the first pane) — they upload into the shared cache.
        // Eviction lives with the tile manager (it also owns `requested`/`uploaded`), and only
        // the first pane runs it so a multi-pane frame doesn't evict what a later pane needs.
        // Eviction runs once per frame, on the last pane — after every pane has promoted its own
        // visible tiles, so a multi-pane frame can't evict what a pane it already drew still needs.
        let drop_tiles = if last {
            self.tiles.evict_excess()
        } else {
            Vec::new()
        };
        let drop_vector_tiles = if first {
            self.vtiles.touch_visible(&visible_vector_tiles);
            self.vtiles.take_evicted()
        } else {
            Vec::new()
        };
        let (new_tiles, new_vector_tiles) = if first {
            let nt = self.tiles.drain_ready();
            // Drain vtiles regardless of basemap (it populates the label cache); only upload the
            // geometry to the GPU when a vector basemap will actually draw it.
            let nv = self.vtiles.drain_ready();
            if !nt.is_empty() || !nv.is_empty() {
                ctx.request_repaint();
            }
            (nt, if is_vector { nv } else { Vec::new() })
        } else {
            (Vec::new(), Vec::new())
        };

        // --- Radar (this pane's product, its own volume) ---
        self.map_3d_controls(idx, prect, ctx);
        let (radar_upload, mut draw_radar) = self.pane_radar(idx, idx);
        // A flashing range needs a frame at its next beat, even with nothing else moving.
        if self.views[idx].flash_ranges.iter().any(Option::is_some) {
            ctx.request_repaint_after(std::time::Duration::from_millis(
                FLASH_BEAT_MS - (chrono::Utc::now().timestamp_millis() as u64 % FLASH_BEAT_MS),
            ));
        }
        if self.trail_more {
            ctx.request_repaint();
        }
        let (observed_upload, mut draw_observed) = self.pane_observed_radar(idx, idx);
        if draw_observed {
            draw_radar = false;
        }
        // In the forecast-scrub tail there's no observed volume — show the HRRR field instead.
        if self.views[idx].timeline.forecast_hour().is_some() {
            draw_radar = false;
            draw_observed = false;
        }

        // Field layers: upload freshly-fetched grids on the first pane; every pane draws the
        // currently-enabled layers.
        // Field textures are evicted the way tiles are: decided here, where the state that knows
        // whether a re-upload will follow lives, and handed to the renderer to free.
        let drop_fields: Vec<crate::render::FieldLayer> = if first {
            let now = Instant::now();
            let on: std::collections::HashSet<crate::render::FieldLayer> = self
                .views
                .iter()
                .flat_map(|v| v.fields_on.iter().copied())
                .collect();
            let mut drop = Vec::new();
            for (layer, st) in self.fields.iter_mut() {
                if on.contains(layer) {
                    st.off_since = None;
                } else if let Some(since) = st.off_since {
                    if now.duration_since(since) >= FIELD_EVICT {
                        st.off_since = None;
                        // The grid is gone from the GPU, so the next enable must re-fetch it
                        // rather than trust its refresh cadence.
                        st.last_fetch = None;
                        st.grid = None;
                        st.radar = None;
                        drop.push(*layer);
                    }
                } else {
                    st.off_since = Some(now);
                }
            }
            drop
        } else {
            Vec::new()
        };
        if drop_fields.contains(&crate::render::FieldLayer::Ensemble) {
            // Thirty-one grids are worth freeing along with the texture.
            self.ensemble_run = None;
            self.ensemble_grid = None;
            self.ensemble_key = None;
            self.ensemble_display_key = None;
        }
        if drop_fields.contains(&crate::render::FieldLayer::ModelDiff) {
            self.diff_grid = None;
            self.diff_valid = None;
            self.diff_display_key = None;
        }
        let compare_visible = self.views.iter().any(|view| {
            view.fields_on
                .contains(&crate::render::FieldLayer::CompareA)
                || view
                    .fields_on
                    .contains(&crate::render::FieldLayer::CompareB)
        });
        if !compare_visible
            && drop_fields.iter().any(|layer| {
                matches!(
                    layer,
                    crate::render::FieldLayer::CompareA | crate::render::FieldLayer::CompareB
                )
            })
        {
            self.compare_grid = None;
            self.compare_valid = None;
        }
        if !drop_fields.is_empty() {
            use crate::render::FieldLayer as FL;
            let requests = &self.acquisition;
            for &layer in &drop_fields {
                // CompareA/CompareB share one fetch and therefore one request-health lane.
                let health_layer = if layer == FL::CompareB {
                    FL::CompareA
                } else {
                    layer
                };
                requests.set_cache_resident(&RequestLane::Field(health_layer), false);
            }
        }
        let field_uploads: Vec<(crate::render::FieldLayer, crate::render::MrmsUpload)> = if first {
            self.fields
                .iter_mut()
                .filter(|(layer, _)| !model_context::MODEL_LAYERS.contains(layer))
                .filter_map(|(layer, state)| state.take_upload(None).map(|upload| (*layer, upload)))
                .collect()
        } else {
            Vec::new()
        };
        if first && self.model_palette_gen != self.palettes.gen {
            let uploads: Vec<_> = self
                .model_fields
                .iter()
                .filter_map(|(request, slot)| {
                    let grid = slot.state.grid.as_ref()?;
                    Some((*request, self.field_upload(request.layer(), grid)))
                })
                .collect();
            for (request, upload) in uploads {
                if let Some(slot) = self.model_fields.get_mut(&request) {
                    slot.state.pending = Some(upload);
                }
            }
            self.model_palette_gen = self.palettes.gen;
        }
        let model_uploads = if first {
            self.model_fields
                .iter_mut()
                .filter_map(|(request, slot)| {
                    slot.state
                        .take_upload(Some(*request))
                        .map(|upload| (slot.texture, upload))
                })
                .collect()
        } else {
            Vec::new()
        };
        let drop_model_fields = if first {
            std::mem::take(&mut self.model_drop_textures)
        } else {
            Vec::new()
        };
        let model_fields = self.views[idx]
            .fields_on
            .iter()
            .filter_map(|layer| {
                let request = self.selected_model_request_for(idx, *layer)?;
                let slot = self.model_fields.get(&request)?;
                slot.state
                    .model_ready(request)
                    .then_some((*layer, slot.texture))
            })
            .collect();
        // Bottom to top, in the user's paint order: the renderer paints this list as given.
        let order = crate::render::FieldLayer::paint_order(&self.settings.field_order);
        let mut on: Vec<crate::render::FieldLayer> =
            self.views[idx].fields_on.iter().copied().collect();
        on.sort_by_key(|l| order.iter().position(|o| o == l));
        let field_draws: Vec<(crate::render::FieldLayer, f32)> = on
            .iter()
            .filter(|layer| {
                crate::fielddiff::layer_ready(**layer, self.diff_valid, self.compare_valid)
                    && if model_context::MODEL_LAYERS.contains(layer) {
                        self.model_field_ready_for(idx, **layer)
                    } else {
                        self.mrms_ready(**layer)
                    }
                    && self.radar_field_ready(idx, **layer)
            })
            .map(|k| {
                let configured = self.settings.field_opacity.get(k).copied().unwrap_or(1.0);
                (
                    *k,
                    field_draw_opacity(*k, configured, self.views[idx].overlay_compare),
                )
            })
            .collect();

        let cam = self.views[idx].camera;
        // The pitched-map "Smooth" 3D volume takes over the pane entirely when it has something
        // resident to draw — it fully occludes the flat radar plane and the observed-gates cloud
        // underneath it, the same way `draw_observed` already wins over `draw_radar` above.
        let smooth_volume = self.pane_smooth_volume(idx, idx, ctx, &cam, vp);
        if smooth_volume.is_some() {
            draw_radar = false;
            draw_observed = false;
        }
        let (center, scale) = cam.world_to_clip_uniform(vp);
        let (wind_upload, wind) = if cam.is_3d() {
            // The particle compositor is a screen-space trail buffer and cannot be pitched without
            // smearing history across the map. Hide it in 3D until that buffer is reprojected.
            (None, None)
        } else {
            self.wind_gpu_frame(idx, &cam, vp)
        };
        let cb = MapCallback {
            pane: idx as u32,
            camera_center: center,
            camera_scale: scale,
            world_per_pixel: cam.world_per_pixel() as f32,
            camera_view_proj: cam.view_projection_uniform(vp),
            camera_3d: if cam.is_3d() { 1.0 } else { 0.0 },
            camera_globe: cam.globe_uniform(vp),
            new_tiles,
            visible,
            basemap_key: pane_style.key(),
            basemap_context: self.tiles.context(pane_style),
            vector_over_raster: pane_style == crate::tiles::BasemapStyle::HybridSatellite,
            radar_upload,
            draw_radar,
            observed_upload,
            draw_observed,
            overlay_upload: if first {
                self.pending_overlay.take()
            } else {
                None
            },
            draw_overlay: self.overlay_ready,
            field_uploads,
            model_uploads,
            model_fields,
            drop_model_fields,
            field_draws,
            field_swipe: self.views[idx]
                .swipe_compare
                .then_some(crate::render::FieldSwipe {
                    left: crate::render::FieldLayer::CompareA,
                    right: crate::render::FieldLayer::CompareB,
                    fraction: self.views[idx].swipe_fraction,
                }),
            drop_fields,
            clear_tiles,
            drop_tiles,
            new_vector_tiles,
            visible_vector,
            clear_vector,
            drop_vector_tiles,
            wind_upload,
            wind,
        };
        ui.painter()
            .add(egui_wgpu::Callback::new_paint_callback(prect, cb));
        // A second, independent paint callback rather than a field on `MapCallback`: it is its own
        // pipeline and its own per-pane GPU resources (`MapVolume3dResources`), keyed by a
        // different type than `RenderResources` in the same `CallbackResources` map, and it draws
        // strictly after (so on top of) the flat map `cb` just queued — the raymarched volume has
        // to composite over the basemap/tiles, never under them.
        if let Some((upload, uniform)) = smooth_volume {
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                prect,
                crate::render3d::MapVolume3dCallback {
                    pane: idx as u32,
                    upload,
                    uniform,
                },
            ));
        }
        vlabels
    }
}
