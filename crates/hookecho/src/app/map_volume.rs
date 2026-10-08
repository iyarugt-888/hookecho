//! Source ownership and build specifications for cached smooth/isosurface map frames.

use super::*;
use crate::loop3d::SourceKey;
use wxdata::level2::temporal::TemporalPolicy;

impl HookEchoApp {
    /// Give back what 3D holds for panes that no longer show it, and share the 3D cache budget
    /// among those that do ([`crate::loop3d::rebalance`]). Without this every volume and mesh a
    /// pane ever built stayed resident after 3D was turned off, the site changed or the pane
    /// closed, and a long session ratcheted memory up until it ran out.
    pub(super) fn release_3d(&mut self) {
        self.loop3d_jobs.drain(&mut self.loop3d);
        let panes: [Option<Option<&str>>; crate::view::MAX_PANES] = std::array::from_fn(|i| {
            self.views
                .get(i)
                .filter(|v| v.map_3d.enabled)
                .map(|v| v.site.as_deref())
        });
        crate::loop3d::rebalance(&mut self.loop3d, &panes);
        for (idx, pane) in panes.iter().enumerate() {
            if pane.is_some() {
                continue;
            }
            self.iso_mesh[idx] = None;
            self.smooth_vol_pending[idx] = None;
            self.smooth_vol_coverage[idx] = None;
            if self.smooth_vol_key[idx].take().is_some() {
                // The pane's GPU volume goes with it (see `pane_gpu`).
                self.smooth_vol_release[idx] = true;
            }
        }
    }

    pub(super) fn sync_isosurface(&mut self, idx: usize, ctx: &egui::Context) {
        self.loop3d_jobs.drain(&mut self.loop3d);
        let Some(key) = self.current_iso_key(idx) else {
            return;
        };
        if self.iso_mesh[idx].as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        if let Some(shells) = self.loop3d[idx].iso.get(&key) {
            self.iso_mesh[idx] = Some((key, Arc::clone(shells)));
            return;
        }
        let job = JobKey::Iso(idx, key.clone());
        if self.loop3d_jobs.came_up_empty(&job) {
            // Nothing crosses the threshold in this volume: show nothing, not the last one's.
            self.iso_mesh[idx] = None;
            return;
        }
        if !self.loop3d_jobs.wants(&job) {
            return;
        }
        let spec = self.iso_spec(idx);
        let policy = key.source.policy;
        let Some(volume) = self.views[idx].volume.as_ref() else {
            return;
        };
        let scan = Arc::clone(&volume.scan);
        let inputs = crate::loop3d::Sweeps::captured(scan, key.source.acquisition());
        self.loop3d_jobs.start(job, &self.spawner, ctx, move || {
            crate::loop3d::Built::Iso(crate::loop3d::build_iso_covered(inputs, &spec, policy))
        });
    }

    pub(super) fn shown_volume_key(&self, idx: usize) -> Option<SourceKey> {
        SourceKey::for_view(
            &self.views[idx],
            super::radar_products::policy(&self.views[idx], &self.settings),
        )
    }

    pub(super) fn cached_volume_key(&self, idx: usize, name: &str) -> Option<SourceKey> {
        // The currently displayed complete frame can outlive its download-cache entry.
        let view = &self.views[idx];
        if let Some(volume) = &view.volume {
            if volume.name == name && !volume.is_live_partial() {
                return SourceKey::for_view(view, TemporalPolicy::Continuous);
            }
        }
        let scan = self.scan_cache.peek(&name.to_string())?;
        Some(SourceKey::new(
            view.site.clone(),
            name.into(),
            0,
            scan,
            TemporalPolicy::Continuous,
        ))
    }

    pub(super) fn current_smooth_key(&self, idx: usize) -> Option<SmoothKey> {
        self.smooth_key_for(
            idx,
            self.shown_volume_key(idx)?,
            self.views[idx].timeline.playing,
        )
    }

    pub(super) fn current_iso_key(&self, idx: usize) -> Option<IsoKey> {
        self.iso_key_for(idx, self.shown_volume_key(idx)?)
    }

    pub(super) fn smooth_key_for(
        &self,
        idx: usize,
        source: SourceKey,
        loop_quality: bool,
    ) -> Option<SmoothKey> {
        let v = &self.views[idx];
        let state = &v.map_3d;
        if !state.enabled {
            return None;
        }
        let (moment, _) = state
            .representation
            .smooth_moment()
            .filter(|(m, _)| *m == v.moment)?;
        Some(SmoothKey {
            source,
            moment,
            full_range: state.smooth_full_range,
            loop_quality,
            storm_uv: v.storm_motion_uv().map(|(u, n)| (u.to_bits(), n.to_bits())),
            product: match state.representation {
                Map3dRepresentation::SmoothProduct => Some(self.product_spec_key(idx)?),
                _ => None,
            },
            palette: self.palettes.gen,
            high_contrast: crate::theme::is_high_contrast(self.settings.theme),
            max_dim: self.vol3d_max_dim,
            roi: self.volume_roi_km(idx).map(crate::loop3d::Roi::key),
        })
    }

    pub(super) fn iso_key_for(&self, idx: usize, source: SourceKey) -> Option<IsoKey> {
        let v = &self.views[idx];
        let state = &v.map_3d;
        if !state.enabled || !state.iso_enabled {
            return None;
        }
        let spec = self.iso_spec(idx);
        Some(IsoKey {
            source,
            moment: spec.moment,
            value: spec.value.to_bits(),
            smooth: spec.smooth,
            step: spec.step.map(f32::to_bits),
            storm_uv: spec.storm_uv.map(|(u, n)| (u.to_bits(), n.to_bits())),
            max_dim: self.vol3d_max_dim,
        })
    }

    pub(super) fn iso_spec(&self, idx: usize) -> crate::loop3d::IsoSpec {
        let v = &self.views[idx];
        let state = &v.map_3d;
        let mi = Moment::ALL.iter().position(|m| *m == v.moment).unwrap_or(0);
        crate::loop3d::IsoSpec {
            moment: v.moment,
            value: state.iso_values[mi],
            step: state.iso_nested.then_some(state.iso_steps[mi]),
            smooth: state.iso_smooth,
            max_dim: self.vol3d_max_dim,
            top_km: VOL3D_TOP_KM,
            storm_uv: v.storm_motion_uv(),
        }
    }

    pub(super) fn smooth_spec(
        &self,
        idx: usize,
        loop_quality: bool,
    ) -> Option<crate::loop3d::SmoothSpec> {
        let state = &self.views[idx].map_3d;
        let (moment, invert) = state.representation.smooth_moment()?;
        Some(crate::loop3d::SmoothSpec {
            moment,
            invert,
            full_range: state.smooth_full_range,
            table: crate::colormap::effective_table(&self.palettes, moment, self.settings.theme),
            max_dim: self.vol3d_max_dim,
            max_voxels: if loop_quality {
                crate::loop3d::SMOOTH_LOOP_MAX_VOXELS
            } else {
                crate::loop3d::SMOOTH_MAX_VOXELS
            },
            top_km: VOL3D_TOP_KM,
            storm_uv: self.views[idx].storm_motion_uv(),
            product: match state.representation {
                Map3dRepresentation::SmoothProduct => Some(self.product_spec(idx)?),
                _ => None,
            },
            roi: self.volume_roi_km(idx),
        })
    }

    /// The pane's region of interest in the radar's frame: km east and north of the site, and
    /// its half-width. `None` without one (the whole radar) or without a site.
    pub(crate) fn volume_roi_km(&self, idx: usize) -> Option<crate::loop3d::Roi> {
        let v = &self.views[idx];
        let roi = v.map_3d.roi.as_ref()?;
        let site = v.site.as_deref().and_then(wxdata::sites::site_by_id)?;
        Some(roi_in_radar_frame(
            [f64::from(site.longitude), f64::from(site.latitude)],
            roi,
        ))
    }

    /// Where the pane's smooth volume box is centred, `[lon, lat]`: the radar, or the region's
    /// centre as the built grid has it.
    pub(crate) fn smooth_box_center(&self, idx: usize) -> Option<[f64; 2]> {
        let site = self.views[idx]
            .site
            .as_deref()
            .and_then(wxdata::sites::site_by_id)?;
        let radar = [f64::from(site.longitude), f64::from(site.latitude)];
        let [east, north] = self.smooth_vol_dims[idx].map_or([0.0, 0.0], |d| d.4);
        let km = f64::from(east).hypot(f64::from(north));
        Some(if km < 1e-6 {
            radar
        } else {
            crate::geo::destination_point(
                radar,
                f64::from(east).atan2(f64::from(north)).to_degrees(),
                km,
            )
        })
    }

    /// Put a region of interest around the selected storm (or clear it).
    pub(crate) fn set_volume_roi(&mut self, idx: usize, half_km: Option<f32>) {
        let Some(half_km) = half_km else {
            self.views[idx].map_3d.roi = None;
            return;
        };
        let Some((cell, _)) = self.selected_storm_live() else {
            return;
        };
        self.views[idx].map_3d.roi = Some(crate::view::VolumeRoi {
            center: [cell.lon, cell.lat],
            half_km,
            storm: self.cell_popup.as_ref().map(|c| c.id.clone()),
            follow: true,
            lost: false,
        });
    }

    /// A following region moves to its storm's newest position. It stops following, and stays
    /// put, when that storm leaves the table or another storm is selected: it never jumps to an
    /// unrelated storm.
    pub(crate) fn follow_volume_roi(&mut self, idx: usize) {
        let Some(roi) = self.views[idx].map_3d.roi.as_ref() else {
            return;
        };
        if !roi.follow {
            return;
        }
        let picked = self.cell_popup.as_ref().map(|c| c.id.clone());
        let live = self.selected_storm_live();
        let roi = self.views[idx].map_3d.roi.as_mut().expect("checked above");
        match (picked == roi.storm, live) {
            (true, Some((cell, true))) => {
                // Moves only when the storm does, so an unchanged table never rebuilds.
                roi.center = [cell.lon, cell.lat];
            }
            (true, _) => {
                roi.follow = false;
                roi.lost = true;
            }
            (false, _) => roi.follow = false,
        }
    }
}

/// A region's centre as km east and north of `radar` (`[lon, lat]`), by the great-circle range
/// and bearing every voxel's gate lookup also uses.
pub(crate) fn roi_in_radar_frame(
    radar: [f64; 2],
    roi: &crate::view::VolumeRoi,
) -> crate::loop3d::Roi {
    let (km, bearing) = crate::geo::great_circle(radar, roi.center);
    let b = bearing.to_radians();
    crate::loop3d::Roi {
        center_km: [(km * b.sin()) as f32, (km * b.cos()) as f32],
        half_km: roi.half_km,
    }
}

#[cfg(test)]
mod roi_tests {
    use super::roi_in_radar_frame;

    #[test]
    fn a_region_lands_where_its_storm_is_from_the_radar() {
        let radar = [-97.278, 35.333];
        let east = crate::geo::destination_point(radar, 90.0, 50.0);
        let roi = crate::view::VolumeRoi {
            center: east,
            half_km: 25.0,
            storm: None,
            follow: false,
            lost: false,
        };
        let r = roi_in_radar_frame(radar, &roi);
        assert!((r.center_km[0] - 50.0).abs() < 0.05, "{:?}", r.center_km);
        assert!(r.center_km[1].abs() < 0.5, "{:?}", r.center_km);
        let ne = crate::view::VolumeRoi {
            center: crate::geo::destination_point(radar, 45.0, 30.0),
            ..roi
        };
        let r = roi_in_radar_frame(radar, &ne);
        let s = 30.0 / 2f32.sqrt();
        assert!((r.center_km[0] - s).abs() < 0.1 && (r.center_km[1] - s).abs() < 0.1);
    }
}
