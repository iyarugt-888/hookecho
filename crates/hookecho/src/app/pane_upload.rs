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
}
