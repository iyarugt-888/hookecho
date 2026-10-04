//! Each pane's radar GPU upload: binning the shared volume with the pane's own product and tilt, cached per pane.
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// Radar upload for pane `idx`, binning the shared volume in `data` (usually the active pane)
    /// with pane `idx`'s product/tilt. Returns `(upload_when_changed, draw_radar)`; the pane's GPU
    /// buffer persists, so `None` means "reuse what's uploaded". Caches per pane via `pane_shown`.
    pub(crate) fn pane_radar(&mut self, idx: usize, data: usize) -> (Option<RadarUpload>, bool) {
        crate::prof_scope!("pane_radar");
        let has_volume = self.views[data].volume.is_some();
        if !self.views[idx].show_radar || !has_volume {
            self.pane_shown.remove(&idx);
            self.pane_lut.remove(&idx);
            self.scan_age_rings.remove(&idx);
            return (None, false);
        }
        if !self.show_scan_age {
            self.scan_age_rings.clear();
        }
        let count = self.views[data].elevation_count();
        self.views[idx].clamp_tilt_to(&count);
        let (moment, tilt, threshold, smooth, storm_uv) = {
            let v = &self.views[idx];
            (
                v.moment,
                v.tilt,
                v.active_threshold(),
                v.smooth,
                v.storm_motion_uv(),
            )
        };
        // The pane's product list is the union over every volume from this site, so a single frame
        // in a loop can lack the selected moment (a legacy volume, a split cut that hasn't arrived).
        // Draw what this volume does have rather than blanking the radar for that frame.
        // Caller resolved `data` to a view that has a volume; a "wait" is the honest answer if
        // that stops being true rather than taking the frame down.
        let Some(have) = self.views[data].volume.as_ref().map(|v| v.moments) else {
            return (None, true);
        };
        let moment = if have[moment.index()] {
            moment
        } else {
            match Moment::ALL.into_iter().find(|m| have[m.index()]) {
                Some(m) => m,
                None => return (None, true), // nothing decodable yet: keep the last image up
            }
        };
        let storm_uv = if moment == Moment::Velocity {
            storm_uv
        } else {
            None
        };
        let Some(name) = self.views[data].volume.as_ref().map(|v| v.name.clone()) else {
            return (None, true);
        };
        // C2: on the pane being worked in, the extremum trail stands in for the sweep. The tag
        // rides in the shown-image key so the upload repeats as the trail grows.
        let trail_tag = if self.filters.show_trail && idx == self.active {
            self.advance_trail(data, moment, tilt)
        } else {
            if !self.filters.show_trail {
                self.trail = None;
                self.trail_more = false;
            }
            None
        };
        let name = match &trail_tag {
            Some(tag) => format!("{name}\u{1}{tag}"),
            None => name,
        };
        // Max reflectivity: the column maximum over every tilt stands in for the one tilt.
        let column_max = self.views[idx].column_max && moment == Moment::Reflectivity;
        // Reflectivity X: the same with non-weather echo removed.
        let clean = self.views[idx].clean_reflectivity && moment == Moment::Reflectivity;
        let name = match (column_max, clean) {
            (true, true) => format!("{name}\u{2}maxclean"),
            (true, false) => format!("{name}\u{2}max"),
            (false, true) => format!("{name}\u{2}clean"),
            (false, false) => name,
        };
        // A user product stands in for the moment, worked out on the shown tilt. Picking another
        // moment takes it off.
        if self.views[idx].user_product.is_some()
            && self.views[idx].moment != self.views[idx].product_moment
        {
            self.views[idx].user_product = None;
            self.views[idx].product_range = None;
        }
        let product = self.map_product(idx);
        let name = match &product {
            Some((_, key)) => format!("{name}\u{2}udp{key:x}"),
            None => name,
        };
        let (threshold, storm_uv) = if product.is_some() {
            (None, None) // both are in the moment's units, which a product does not share
        } else {
            (threshold, storm_uv)
        };
        let strict_current = self.settings.live_sweep_mode
            == crate::settings::LiveSweepMode::StrictCurrentSweep
            && self.views[data].timeline.following
            && !self.views[data].timeline.playing
            && self.views[data]
                .volume
                .as_ref()
                .is_some_and(Volume::is_live_partial)
            && trail_tag.is_none()
            && !column_max
            && product.is_none();
        // Bilinear sampling can pull a valid current-pass gate across an azimuth whose old
        // row was cleared. Strict mode uses nearest sampling so the masked sector stays empty.
        let smooth = smooth && !strict_current;
        let uv_key = storm_uv.map(|(e, n)| (e.to_bits(), n.to_bits()));
        // Dealiasing only applies to Doppler velocity, and only where it is actually folded:
        // a TDWR's Level 3 velocity is already unfolded before it leaves the radar.
        let dealias = self.settings.dealias_velocity
            && moment == Moment::Velocity
            && !self.views[idx]
                .site
                .as_deref()
                .is_some_and(wxdata::tdwr::is_tdwr);
        let key: ShownKey = (
            name,
            moment,
            tilt,
            threshold,
            smooth,
            uv_key,
            dealias,
            self.settings.precip_tint.then_some(self.precip_flag_gen),
            strict_current,
        );
        // Flash a range: on its bright half-beats the band is painted white. Only the colour table
        // changes, so the beat costs a 3 KB table write, not a sweep upload.
        let flash = self.views[idx].flash_ranges[moment.index()]
            .filter(|_| flash_beat_on() && product.is_none());
        let flash_tag = flash.map_or(0u64, |(lo, hi)| {
            (u64::from(lo.to_bits()) << 32 | u64::from(hi.to_bits())) | 1
        });
        let lut_gen = self
            .palettes
            .gen
            .wrapping_add(if crate::theme::is_high_contrast(self.settings.theme) {
                0x9e37_79b9_7f4a_7c15
            } else {
                0
            })
            .wrapping_add(flash_tag.wrapping_mul(0x2545_f491_4f6c_dd1d));
        // Same sweep already up: the only thing left that can differ is the color table, and
        // that is a 3 KB write into the texture already bound.
        let lut_only = self.pane_shown.get(&idx) == Some(&key);
        // Unless the scan-age ring was just switched on: it is read off the sweep as it is binned,
        // and this sweep was binned before anyone asked.
        let ring_owed = self.show_scan_age && !self.scan_age_rings.contains_key(&idx);
        if lut_only && self.pane_lut.get(&idx) == Some(&lut_gen) && !ring_owed {
            return (None, true);
        }
        let mut table_owned =
            crate::colormap::effective_table(&self.palettes, moment, self.settings.theme);
        if let Some((lo, hi)) = flash {
            table_owned = crate::colormap::highlight(&table_owned, lo, hi, FLASH_RGBA);
        }
        let table = &table_owned;
        // Cheap handle taken before the volume is borrowed mutably below.
        let precip = (self.settings.precip_tint
            && self.mrms_ready(crate::render::FieldLayer::PrecipType))
        .then(|| self.precip_flag_grid.clone())
        .flatten();
        let trail_acc = trail_tag
            .as_ref()
            .and(self.trail.as_ref())
            .and_then(|t| t.acc.as_ref());
        // Read off the sweep in hand rather than fetched later: the paint pass only has a shared
        // borrow of the volume, and binning needs a mutable one.
        let want_age = self.show_scan_age;
        let mut ring: Option<ScanAgeRing> = None;
        let mut product_range: Option<(f32, f32)> = None;
        let upload = if let Some(acc) = trail_acc {
            Ok::<_, anyhow::Error>(to_upload(
                acc,
                table,
                threshold,
                smooth,
                storm_uv,
                precip.as_deref(),
                lut_only,
                None,
            ))
        } else {
            let telemetry = self.views[data]
                .live_render_started
                .map(|started| (started, Arc::clone(&self.views[data].live_queue_timings)));
            let Some(vol) = self.views[data].volume.as_mut() else {
                return (None, true);
            };
            // No tilts yet (a volume that has only just started arriving) is a "wait", not a
            // failure: erroring here put "tilt 0 out of range" on the map once per volume.
            if vol.elevations.is_empty() {
                return (None, true);
            }
            let sweep = if let Some((spec, key)) = &product {
                vol.product_sweep(*key, &spec.expr, spec.range, spec.env, tilt)
            } else if column_max {
                vol.column_max(moment, clean)
            } else if clean {
                vol.clean_reflectivity(tilt)
            } else {
                vol.binned(moment, tilt, dealias)
            };
            sweep.map(|s| {
                if want_age {
                    ring = ScanAgeRing::from_sweep(s);
                }
                // A product's own colour table, else a ramp across the range it came out in.
                let ramp = product.as_ref().map(|(spec, _)| {
                    product_range = Some((s.value_min, s.value_max));
                    spec.table
                        .clone()
                        .unwrap_or_else(|| crate::colormap::ramp_table(s.value_min, s.value_max))
                });
                let mut upload = to_upload(
                    s,
                    ramp.as_ref().unwrap_or(table),
                    threshold,
                    smooth,
                    storm_uv,
                    precip.as_deref(),
                    lut_only,
                    telemetry,
                );
                if strict_current && !upload.data.is_empty() {
                    wxdata::level2::mask_previous_pass_rows(s, &mut upload.data);
                    // The old rows are transparent now; no dimming pass is needed.
                    upload.uniform[19] = 0.0;
                }
                upload
            })
        };
        match upload {
            Ok(up) => {
                self.views[data].live_render_started = None;
                if product_range.is_some() || product.is_none() {
                    self.views[idx].product_range = product_range;
                }
                self.pane_shown.insert(idx, key);
                self.pane_lut.insert(idx, lut_gen);
                // Recorded even when `None` (a trail has no single rotation to age, and some
                // sweeps carry no timing), so "looked, nothing to draw" is not re-asked every frame.
                if want_age {
                    self.scan_age_rings.insert(idx, ring);
                }
                (Some(up), true)
            }
            Err(e) => {
                self.views[idx].error = Some(e.to_string());
                (None, false)
            }
        }
    }

    /// The map-pitch "Smooth" and "Debris" 3D representations: a continuous, interpolated volume
    /// raymarched in place on the map, as against `pane_observed_radar`'s real (and therefore
    /// gappy-at-range) Level II gates. `None` when this pane isn't in one of those two modes.
    /// `Some` carries this frame's camera/radar uniform always, and a fresh
    /// [`crate::render3d::Volume3dUpload`] only on the frame a rebuild finishes — the resample is
    /// real CPU work (`build_volume3d`'s own comment: "would drop a second of frames"), so it runs
    /// off-thread and this drains it rather than blocking the render path.
    ///
    /// Debris shares every line of this with Smooth — same resample, same raymarch, same controls
    /// — except which moment it resamples and that its volume's index is inverted afterward
    /// ([`wxdata::volume3d::invert_in_place`]) before upload, with the LUT permuted
    /// ([`crate::colormap::invert_lut`]) to match. See that function's doc comment for why: a
    /// max-intensity raymarch over plain CC only ever finds ordinary high-CC rain, never the
    /// lofted low-CC pocket a tornado debris signature actually is.
    pub(crate) fn pane_smooth_volume(
        &mut self,
        idx: usize,
        data: usize,
        ctx: &egui::Context,
        cam: &crate::render::mercator::Camera,
        vp: (f32, f32),
    ) -> Option<(
        Option<Arc<crate::render3d::Volume3dUpload>>,
        crate::render3d::Uniforms,
    )> {
        // Cloned (not borrowed) up front: `Map3dState` isn't `Copy`, and every branch below needs
        // `&mut self` for the async-build bookkeeping, so holding a borrow of it across this
        // function would fight the borrow checker for no benefit — it's a few scalars and a small
        // `Option`, cheap to clone.
        let state = self.views[idx].map_3d.clone();
        if !state.enabled {
            return None;
        }
        let (resample_moment, _) = state.representation.smooth_moment()?;
        if self.views[idx].moment != resample_moment {
            // `map_3d_controls` already resets back to Observed the moment this stops being
            // true; this is a second, cheap guard against ever resampling the wrong moment.
            return None;
        }
        let site = self.views[idx]
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)?;

        // Cache/upload/coverage are one accepted frame. A different selected source never
        // draws the old GPU volume while its worker runs.
        self.loop3d_jobs.drain(&mut self.loop3d);
        let loop_quality = self.views[data].timeline.playing;
        let source = self.shown_volume_key(data)?;
        let key = self.smooth_key_for(idx, source, loop_quality)?;
        if self.smooth_vol_key[idx].as_ref() != Some(&key) {
            let job = JobKey::Smooth(idx, key.clone());
            if let Some(frame) = self.loop3d[idx].smooth.get(&key) {
                let up = &frame.upload;
                self.smooth_vol_dims[idx] = Some((up.n, up.nz, up.half_km, up.top_km));
                self.smooth_vol_info[idx] = Some((up.cell_km(), up.outside));
                self.smooth_vol_range[idx] = up.value_range;
                self.smooth_vol_coverage[idx] = Some(frame.coverage.clone());
                self.smooth_vol_pending[idx] = Some(Arc::clone(up));
                self.smooth_vol_key[idx] = Some(key.clone());
                ctx.request_repaint();
            } else if self.loop3d_jobs.came_up_empty(&job) {
                self.smooth_vol_dims[idx] = None;
                self.smooth_vol_info[idx] = None;
                self.smooth_vol_range[idx] = None;
                self.smooth_vol_coverage[idx] = None;
                self.smooth_vol_pending[idx] = None;
                self.smooth_vol_key[idx] = Some(key.clone());
            } else if self.loop3d_jobs.wants(&job) {
                if let (Some(spec), Some(vol)) = (
                    self.smooth_spec(idx, loop_quality),
                    self.views[data].volume.as_ref(),
                ) {
                    let scan = Arc::clone(&vol.scan);
                    let policy = key.source.policy;
                    let inputs = crate::loop3d::Sweeps::captured(scan, key.source.acquisition());
                    self.loop3d_jobs.start(job, &self.spawner, ctx, move || {
                        crate::loop3d::Built::Smooth(crate::loop3d::build_smooth_covered(
                            inputs, &spec, policy,
                        ))
                    });
                }
            }
        }
        if self.smooth_vol_key[idx].as_ref() != Some(&key) {
            return None;
        }

        let (n, nz, half_km, top_km) = self.smooth_vol_dims[idx]?;
        let antenna_altitude_m =
            (site.elevation_meters as f64 + wxdata::towers::tower_m(site.id)) as f32;
        // A geometry-only stand-in: `map_uniform` reads `n`/`nz`/`half_km`/`top_km` off it and
        // nothing else, so the (empty, non-allocating) `data`/`lut` never have to hold the actual
        // multi-megabyte volume just to recompute a camera matrix on a frame with no new upload.
        let dims_only = crate::render3d::Volume3dUpload {
            data: Vec::new(),
            n,
            nz,
            lut: Vec::new(),
            half_km,
            top_km,
            outside: 0.0,
            value_range: None,
        };
        // Denoising a plain "high is interesting" field makes sense for reflectivity and spectrum
        // width; `SmoothDebris`'s inverted-CC volume is the opposite sense (low is interesting)
        // and has no floor of its own here.
        let denoise_floor = match state.representation {
            Map3dRepresentation::SmoothVolume => Some(state.reflectivity_floor_dbz),
            Map3dRepresentation::SmoothSpectrumWidth => Some(state.sw_floor_ms),
            Map3dRepresentation::SmoothZdr => Some(state.zdr_floor_db),
            Map3dRepresentation::SmoothKdp => Some(state.kdp_floor_deg_km),
            Map3dRepresentation::SmoothVelocity => Some(state.velocity_floor_ms),
            // A product's floor is in its own units, inside the range its volume was drawn over.
            Map3dRepresentation::SmoothProduct => {
                self.smooth_vol_range[idx].map(|(lo, hi)| state.product_floor.clamp(lo, hi))
            }
            Map3dRepresentation::SmoothDebris | Map3dRepresentation::ObservedSweeps => None,
        };
        // The values the volume's indices span: a product's own range, else the moment's.
        let index_range = match state.representation {
            Map3dRepresentation::SmoothProduct => self.smooth_vol_range[idx],
            _ => None,
        }
        .unwrap_or(resample_moment.value_range());
        // Velocity's volume is indexed by speed (`fold_by_speed`), so its floor and ceiling are
        // speeds: the floor is the outbound index at that speed (inbound sits one above it) and
        // the ceiling the inbound one, so both directions are kept alike.
        let speed_scale =
            (resample_moment == Moment::Velocity).then(|| resample_moment.value_range().1);
        // Velocity's values are speeds, either direction; every other moment's are signed values
        // (a -10 dBZ floor, a negative ZDR) and go in as they are.
        let value_index = |v: f32, top: bool| match speed_scale {
            Some(vmax) => {
                let v = v.abs();
                (wxdata::volume3d::speed_index(v, vmax) + u8::from(top && v < vmax)) as f32
            }
            None => crate::render3d::threshold_index(v, index_range),
        };
        let threshold_idx = match denoise_floor {
            Some(floor) if state.denoise_enabled => value_index(floor, false),
            _ => 2.0,
        };
        // Debris is the CC representation, so it gets the anomaly ramp. `inverted: true` because
        // the volume it raymarches has already had its indices flipped (`invert_in_place`) — the
        // ramp has to run the other way round in that space to still mean "low CC draws solid".
        let cc = match state.representation {
            Map3dRepresentation::SmoothDebris => crate::render3d::cc_anomaly_uniform(
                state.cc_anomaly,
                resample_moment.value_range(),
                true,
            ),
            _ => [0.0; 4],
        };
        let ceiling_idx = match state.ceilings[state.representation as usize] {
            Some(top) if denoise_floor.is_some() && state.denoise_enabled => value_index(top, true),
            _ => 0.0,
        };
        // The opacity curve, its values into this volume's index space as the floor's are.
        let tf = state.tf_curves[state.representation as usize]
            .filter(|_| denoise_floor.is_some())
            .map(|pts| pts.map(|[v, a]| [value_index(v, false), a]));
        let view = crate::render3d::View3d {
            tf,
            threshold_idx,
            ceiling_idx,
            clip: state.clip,
            plane: state.plane,
            cc,
            cappi_km: state.cappi_marker.then_some(self.cappi_alt_km),
        };
        // Same visual-guard clamp as the standalone 3D Reflectivity window: a phone under thermal
        // or battery pressure gets the coarsest march regardless of what quality was chosen.
        let steps = if ui::motion::degraded() {
            state.quality_steps.min(64)
        } else {
            state.quality_steps
        }
        // The march takes about one sample per cell (`map_uniform`), so this is a ceiling, not
        // the count: the grid is now far finer than the old fixed 64-128 steps could cover.
        * 4;
        let uniform = crate::render3d::map_uniform(
            cam,
            vp,
            site.longitude as f64,
            site.latitude as f64,
            antenna_altitude_m,
            &dims_only,
            steps,
            view,
            state.vertical_exaggeration,
            state.opacity,
        );
        Some((self.smooth_vol_pending[idx].take(), uniform))
    }

    /// Build the static instance buffer for all real gates in a Level II volume. The cache key
    /// excludes camera state on purpose: pitch, bearing, pan and zoom only change the shared
    /// camera uniform.
    pub(crate) fn pane_observed_radar(
        &mut self,
        idx: usize,
        data: usize,
    ) -> (Option<ObservedSweepUpload>, bool) {
        let state = &self.views[idx].map_3d;
        if !state.enabled || state.representation != Map3dRepresentation::ObservedSweeps {
            return (None, false);
        }
        let moment = self.views[idx].moment;
        if moment == Moment::SpecificDifferentialPhase {
            // KDP is derived from PhiDP after binning; calling it an observed gate would be false.
            return (None, false);
        }
        let Some(vol) = self.views[data].volume.as_ref() else {
            return (None, false);
        };
        if !vol.moments[moment.index()] {
            return (None, false);
        }
        let scan = Arc::clone(&vol.scan);
        let (value_min, value_max) = moment.value_range();
        // CC anomaly replaces the floor for correlation coefficient rather than stacking with it.
        // Leaving both live would be self-defeating: the floor's whole effect on CC is to discard
        // the low values the anomaly ramp exists to bring forward, so a user who turned anomaly
        // on would still lose the debris to a threshold set for a different moment's logic.
        let cc_anomaly = self.views[idx].map_3d.cc_anomaly;
        let cc_on = moment == Moment::CorrelationCoefficient && cc_anomaly.enabled;
        let cc = if cc_on {
            crate::render3d::cc_anomaly_uniform(cc_anomaly, (value_min, value_max), false)
        } else {
            [0.0; 4]
        };
        let threshold = self.views[idx].active_threshold().filter(|_| !cc_on);
        let threshold_idx = threshold.map_or(2.0, |value| {
            (2.0 + (value - value_min) / (value_max - value_min).max(f32::EPSILON) * 253.0)
                .clamp(2.0, 255.0)
        });
        let (motion_e, motion_n, srv) = self.views[idx]
            .storm_motion_uv()
            .map(|(e, n)| {
                let per_ms = 253.0 / (value_max - value_min).max(f32::EPSILON);
                (e * per_ms, n * per_ms, 1.0f32)
            })
            .unwrap_or((0.0, 0.0, 0.0));
        // -inf in an unused slot reads as "nothing here" on the shader side (`> -900.0` is false
        // for it) and is exact under `.to_bits()` round-tripping, unlike NaN's multiple bit
        // patterns. Only the first `selected_layer_elevs.len()` slots are ever real; the UI caps
        // that at `MAX_HIGHLIGHTED_LAYERS` already, so no truncation happens here.
        let mut highlight_elevs = [f32::NEG_INFINITY; MAX_HIGHLIGHTED_LAYERS];
        for (slot, elev) in highlight_elevs
            .iter_mut()
            .zip(self.views[idx].map_3d.selected_layer_elevs.iter())
        {
            *slot = *elev;
        }
        let mut controls = [0u32; 12 + MAX_HIGHLIGHTED_LAYERS];
        controls[0] = self.views[idx].map_3d.vertical_exaggeration.to_bits();
        controls[1] = self.views[idx].map_3d.opacity.to_bits();
        controls[2] = threshold_idx.to_bits();
        controls[3] = motion_e.to_bits();
        controls[4] = motion_n.to_bits();
        controls[5] = srv.to_bits();
        // Rebuild when the volume gains a sweep (live chunk stream) so every available tilt
        // is in the buffer, not just the ones present when 3D was first enabled.
        controls[6] = scan.sweeps().len() as u32;
        // The uniform only reaches the GPU alongside a fresh instance buffer, so anything that
        // lives in it has to be part of the rebuild identity or moving the control silently does
        // nothing. That is why `threshold_idx` and friends are already here, why the CC ramp has
        // to join them, and why `beam_rise` — which rides in the uniform slot the lowest-tilt
        // elevation vacated — does too.
        controls[7] = self.views[idx].map_3d.beam_rise.to_bits();
        for (slot, v) in controls[8..12].iter_mut().zip(cc.iter()) {
            *slot = v.to_bits();
        }
        for (slot, elev) in controls[12..].iter_mut().zip(highlight_elevs.iter()) {
            *slot = elev.to_bits();
        }
        let policy = super::radar_products::policy(&self.views[data], &self.settings);
        let key = crate::view::ObservedKey::new(
            &self.views[data],
            moment,
            policy,
            self.palettes.gen,
            crate::theme::is_high_contrast(self.settings.theme),
            controls,
        )
        .expect("observed upload has a source volume");
        if self.views[idx].map_3d.observed_key.as_ref() == Some(&key) {
            return (None, true);
        }
        // Full native resolution: every gate of every radial goes to the GPU as a texel, and the
        // shader draws each radial as a strip on its own beam surface. `max_texture_dim` is only
        // a ceiling; a sweep wider than it is max-pooled and reported, never silently thinned.
        let observed = match level2::observed_volume_with_passes(
            &scan,
            moment,
            self.max_texture_dim as usize,
            policy,
            self.views[data]
                .volume
                .as_ref()
                .and_then(|volume| volume.acquisition_for(self.views[data].site.as_deref()))
                .and_then(crate::live_scan::AcquisitionSnapshot::pass_index),
        ) {
            Ok(volume) => volume,
            Err(err) => {
                self.views[idx].error = Some(err.to_string());
                return (None, false);
            }
        };
        self.views[idx].map_3d.observed_layers = observed.layers.clone();
        self.views[idx].map_3d.observed_coverage = Some(observed.coverage.clone());
        // A selection surviving a moment switch or a tilt dropping out of the volume would dim
        // every gate (the shader has a selection but nothing left to match it), which reads as
        // the whole layer vanishing rather than as nothing being selected.
        self.views[idx].map_3d.selected_layer_elevs.retain(|&sel| {
            observed
                .layers
                .iter()
                .any(|l| (l.elevation_deg - sel).abs() < 0.05)
        });
        let antenna_altitude_m = self.views[idx]
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)
            .map(|site| site.elevation_meters as f64 + wxdata::towers::tower_m(site.id))
            .unwrap_or(0.0) as f32;
        let table = crate::colormap::effective_table(&self.palettes, moment, self.settings.theme);
        let lut = crate::colormap::bake_lut(&table, (value_min, value_max), None).to_vec();
        #[cfg(debug_assertions)]
        log::debug!(
            "3D observed {}: {} sweeps, {} radials, {} layers, pooled {:?}",
            moment.short_name(),
            observed.sweep_count,
            observed.radial_count,
            observed.sweeps.len(),
            observed.sweeps.iter().map(|s| s.pool).max()
        );
        let (radar_lat, radar_lon) = (observed.radar_lat, observed.radar_lon);
        let mut instances = Vec::with_capacity(observed.radial_count);
        let mut layers = Vec::with_capacity(observed.sweeps.len());
        for (layer, sweep) in observed.sweeps.into_iter().enumerate() {
            for (row, r) in sweep.radials.iter().enumerate() {
                instances.push(ObservedRadialInstance {
                    polar: [
                        r.azimuth_deg,
                        r.spacing_deg,
                        r.first_gate_km,
                        r.gate_interval_km,
                    ],
                    data: [
                        r.elevation_deg,
                        layer as f32,
                        row as f32,
                        r.gate_count as f32,
                    ],
                });
            }
            layers.push(ObservedSweepLayer {
                width: sweep.width as u32,
                rows: sweep.radials.len() as u32,
                values: sweep.values,
            });
        }
        let uniform = observed_uniform(
            [
                radar_lat,
                radar_lon,
                antenna_altitude_m,
                self.views[idx].map_3d.vertical_exaggeration,
                self.views[idx].map_3d.opacity,
                threshold_idx,
                Camera::world_units_per_metre(radar_lat as f64) as f32,
                srv,
                motion_e,
                motion_n,
                // Slot 10: the volume's lowest-tilt elevation used to live here and stopped being
                // read; `beam_rise` takes it over rather than growing the buffer. See the shader's
                // own `Radar3d` comment.
                self.views[idx].map_3d.beam_rise,
            ],
            cc,
            highlight_elevs,
        );
        self.views[idx].map_3d.observed_key = Some(key);
        (
            Some(ObservedSweepUpload {
                instances,
                layers,
                uniform,
                lut,
            }),
            true,
        )
    }
}
