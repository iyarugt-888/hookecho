//! Source ownership and build specifications for cached smooth/isosurface map frames.

use super::*;
use crate::loop3d::SourceKey;
use wxdata::level2::temporal::TemporalPolicy;

impl HookEchoApp {
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
        })
    }
}
