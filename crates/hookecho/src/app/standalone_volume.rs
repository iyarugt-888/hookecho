//! Ownership of the standalone reflectivity grid. One worker stays alive through selection
//! changes; its answer is accepted only if the actual source and controls still match.
use super::*;
use wxdata::level2::temporal::{TemporalCoverage, TemporalPolicy};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct VolumeKey {
    site: Option<String>,
    volume: String,
    revision: u64,
    scan: radar_products::ScanIdentity,
    policy: TemporalPolicy,
    chosen: Vec<u32>,
    palette: u64,
    high_contrast: bool,
}

impl VolumeKey {
    fn for_view(view: &MapView, settings: &Settings, chosen: &[f32], palette: u64) -> Option<Self> {
        let vol = view.volume.as_ref()?;
        let mut chosen: Vec<_> = chosen.iter().map(|e| e.to_bits()).collect();
        chosen.sort_unstable();
        chosen.dedup();
        Some(Self {
            site: view.site.clone(),
            volume: vol.name.clone(),
            revision: vol.revision(),
            scan: radar_products::ScanIdentity::new(&vol.scan),
            policy: radar_products::policy(view, settings),
            chosen,
            palette,
            high_contrast: crate::theme::is_high_contrast(settings.theme),
        })
    }

    fn includes(&self, elevation: f32) -> bool {
        self.chosen.is_empty()
            || self
                .chosen
                .iter()
                .any(|&bits| (f32::from_bits(bits) - elevation).abs() < 0.05)
    }
}

pub(super) struct VolumeBuilt {
    upload: crate::render3d::Volume3dUpload,
    range: (f32, f32),
    layers: Vec<level2::ObservedLayer>,
    coverage: TemporalCoverage,
}

pub(super) struct VolumeDelivery {
    key: VolumeKey,
    result: Result<VolumeBuilt, String>,
}

#[derive(Default)]
pub(super) struct VolumeBuildState {
    attempted: Option<VolumeKey>,
    accepted: Option<VolumeKey>,
    rx: Option<std::sync::mpsc::Receiver<VolumeDelivery>>,
}

impl VolumeBuildState {
    fn ready(&self, actual: Option<&VolumeKey>) -> bool {
        actual.is_some() && self.accepted.as_ref() == actual && self.rx.is_none()
    }

    fn disconnected(&mut self, actual: Option<&VolumeKey>) -> Option<String> {
        self.rx = None;
        if actual.is_some() && self.attempted.as_ref() == actual {
            Some("3D worker stopped before delivering a grid".into())
        } else {
            self.attempted = None;
            None
        }
    }

    fn wants(&self, key: &VolumeKey) -> bool {
        self.rx.is_none() && self.attempted.as_ref() != Some(key)
    }

    fn accept(
        &mut self,
        delivery: VolumeDelivery,
        actual: Option<&VolumeKey>,
    ) -> Option<Result<VolumeBuilt, String>> {
        if actual != Some(&delivery.key) || self.attempted.as_ref() != Some(&delivery.key) {
            // Superseded successes and failures are equally neutral. The next frame can build
            // the latest selection, even if it happens to be the accepted selection again.
            self.attempted = None;
            return None;
        }
        if delivery.result.is_ok() {
            self.accepted = Some(delivery.key);
        }
        // Remember an empty/failed build; retry is explicit rather than a full CPU job per frame.
        Some(delivery.result)
    }
}

fn build(
    mut sweeps: Vec<BinnedSweep>,
    key: &VolumeKey,
    table: &crate::colormap::ColorTable,
    n: usize,
    nz: usize,
) -> Result<VolumeBuilt, String> {
    let mut coverage =
        level2::temporal::prepare(&mut sweeps, key.policy).map_err(|error| error.to_string())?;
    // The extent is shared across tilt selections, but only retained policy inputs can set it.
    let half_km = wxdata::volume3d::max_sample_range_km(&sweeps).max(50.0);
    let layers = sweeps
        .iter()
        .zip(&coverage.contributors)
        .map(|(sweep, source)| {
            let mut layer = level2::layer_summary(sweep);
            // Original bin clocks stay intact; summarize the clocks actually allowed to
            // contribute, so excluded rows cannot leak an older interval into the Layers UI.
            layer.scan_start = source
                .used_start_ms
                .and_then(DateTime::from_timestamp_millis);
            layer.scan_end = source.used_end_ms.and_then(DateTime::from_timestamp_millis);
            layer
        })
        .collect();
    sweeps.retain(|sweep| key.includes(sweep.elevation_deg));
    coverage
        .contributors
        .retain(|source| key.includes(source.elevation_deg));
    let volume = if key.chosen.is_empty() {
        wxdata::volume3d::build(&sweeps, n, nz, half_km, VOL3D_TOP_KM)
    } else {
        wxdata::volume3d::build_shells(
            &sweeps,
            n,
            nz,
            half_km,
            VOL3D_TOP_KM,
            crate::render3d::BEAMWIDTH_DEG,
        )
    }
    .ok_or_else(|| "No reflectivity tilts match this selection".to_string())?;
    let range = (volume.value_min, volume.value_max);
    let lut = crate::colormap::bake_lut(table, range, None).to_vec();
    Ok(VolumeBuilt {
        upload: crate::render3d::Volume3dUpload {
            data: crate::render3d::pack_rg8(&volume.data),
            n: volume.n as u32,
            nz: volume.nz as u32,
            lut,
            half_km: volume.half_km,
            top_km: volume.top_km,
            outside: 0.0,
            value_range: None,
        },
        range,
        layers,
        coverage,
    })
}

impl HookEchoApp {
    fn current_volume3d_key(&self) -> Option<VolumeKey> {
        VolumeKey::for_view(
            &self.views[self.active],
            &self.settings,
            &self.vol3d.selected_elevs,
            self.palettes.gen,
        )
    }

    pub(super) fn sync_volume3d_status(&mut self) {
        let actual = self.current_volume3d_key();
        self.vol3d.current = self.vol3d_build.ready(actual.as_ref());
        self.vol3d.building = self.vol3d_build.rx.is_some();
        if self.vol3d_build.attempted != actual {
            self.vol3d.error = None;
        }
    }

    pub(crate) fn build_volume3d(&mut self) {
        if !self.volume3d_supported {
            self.toast(
                ToastKind::Error,
                "3D needs WebGPU — this browser fell back to WebGL, which has no 3D textures",
            );
            return;
        }
        self.show_3d = true;
        if std::mem::take(&mut self.vol3d.retry) && self.vol3d_build.rx.is_none() {
            self.vol3d_build.attempted = None;
            self.vol3d.error = None;
        }
        self.sync_volume3d_status();
        let Some(key) = self.current_volume3d_key() else {
            return;
        };
        if !self.vol3d_build.wants(&key) {
            return;
        }
        let sweeps = self.views[self.active]
            .volume
            .as_mut()
            .expect("key has a source volume")
            .reflectivity_tilts();
        let table = crate::colormap::effective_table(
            &self.palettes,
            Moment::Reflectivity,
            self.settings.theme,
        );
        let (tx, rx) = std::sync::mpsc::channel();
        self.vol3d_build.attempted = Some(key.clone());
        self.vol3d_build.rx = Some(rx);
        self.vol3d.current = false;
        self.vol3d.building = true;
        self.vol3d.error = None;
        self.spawner.spawn(async move {
            let worker_key = key.clone();
            let result = wxdata::task::blocking(move || {
                build(sweeps, &worker_key, &table, VOL3D_N, VOL3D_NZ)
            })
            .await
            .map_err(|error| format!("3D worker failed: {error}"))
            .and_then(|result| result);
            let _ = tx.send(VolumeDelivery { key, result });
        });
    }

    pub(crate) fn drain_volume3d(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.vol3d_build.rx else {
            return;
        };
        let delivery = match rx.try_recv() {
            Ok(delivery) => delivery,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // Retire the one worker without repeatedly respawning a disconnected job.
                let actual = self.current_volume3d_key();
                self.vol3d.error = self.vol3d_build.disconnected(actual.as_ref());
                self.sync_volume3d_status();
                ctx.request_repaint();
                return;
            }
        };
        self.vol3d_build.rx = None;
        let actual = self.current_volume3d_key();
        let label = ui::volume3d_window::VolumeFrameLabel {
            site: delivery.key.site.clone(),
            volume: delivery.key.volume.clone(),
            revision: delivery.key.revision,
        };
        match self.vol3d_build.accept(delivery, actual.as_ref()) {
            Some(Ok(built)) => {
                // Publish source, coverage, layer summaries and GPU staging together.
                self.vol3d_pending = Some(built.upload);
                self.vol3d_range = built.range;
                self.vol3d.layers = built.layers;
                self.vol3d.frame = Some((label, built.coverage));
                self.vol3d.error = None;
            }
            Some(Err(error)) => self.vol3d.error = Some(error),
            None => {}
        }
        self.sync_volume3d_status();
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_view() -> MapView {
        let scan = level2::decode_volume(
            include_bytes!("../../../wxdata/tests/data/corpus/mayfield-2021-first-records.ar2")
                .to_vec(),
        )
        .unwrap();
        let mut view = MapView::new(Some("KPAH".into()), Camera::at_lonlat(-88.0, 37.0, 8.0));
        view.timeline.following = true;
        view.volume = Some(Volume::from_live(
            Arc::new(scan),
            "KPAH20211211_032349_V06".into(),
            DateTime::from_timestamp(1_639_193_029, 0).unwrap(),
        ));
        view
    }

    fn key(view: &MapView) -> VolumeKey {
        VolumeKey::for_view(view, &Settings::default(), &[], 0).unwrap()
    }

    #[test]
    fn same_elevation_updates_and_independent_panes_change_the_volume_key() {
        let mut view = fixture_view();
        let old = key(&view);
        assert_ne!(old, key(&fixture_view()));
        let volume = view.volume.as_mut().unwrap();
        let count = volume.elevations.len();
        volume.apply_live(
            Arc::clone(&volume.scan),
            volume.name.clone(),
            volume.time,
            &[0.5],
        );
        assert_eq!(volume.elevations.len(), count);
        assert_ne!(
            old,
            key(&view),
            "tilt count alone misses a supplemental pass"
        );
        view.site = Some("KTLX".into());
        assert_ne!(old, key(&view));
        view.volume = None;
        assert!(VolumeKey::for_view(&view, &Settings::default(), &[], 0).is_none());
    }

    #[test]
    fn policy_palette_and_beam_selection_are_inputs_but_camera_is_not() {
        let mut view = fixture_view();
        let original = key(&view);
        let mut settings = Settings::default();
        assert_ne!(
            original,
            VolumeKey::for_view(&view, &settings, &[], 1).unwrap()
        );
        settings.theme = crate::settings::Theme::HighContrast;
        assert_ne!(
            original,
            VolumeKey::for_view(&view, &settings, &[], 0).unwrap()
        );
        settings.theme = crate::settings::Theme::DearImGui;
        assert_eq!(
            original,
            VolumeKey::for_view(&view, &settings, &[], 0).unwrap()
        );
        settings.live_sweep_mode = crate::settings::LiveSweepMode::StrictCurrentSweep;
        assert_ne!(
            original,
            VolumeKey::for_view(&view, &settings, &[], 0).unwrap()
        );
        view.timeline.playing = true;
        assert_eq!(
            original,
            VolumeKey::for_view(&view, &settings, &[], 0).unwrap()
        );
        view.timeline.playing = false;
        view.timeline.following = false;
        assert_eq!(
            original,
            VolumeKey::for_view(&view, &settings, &[], 0).unwrap()
        );
        let a = VolumeKey::for_view(&view, &settings, &[0.5, 1.5], 0).unwrap();
        assert_ne!(original, a);
        assert_eq!(
            a,
            VolumeKey::for_view(&view, &settings, &[1.5, 0.5, 0.5], 0).unwrap()
        );
    }

    #[test]
    fn a_worker_stays_bounded_and_stale_success_or_failure_cannot_commit() {
        let mut view = fixture_view();
        let old = key(&view);
        let mut state = VolumeBuildState::default();
        let (tx, rx) = std::sync::mpsc::channel();
        state.attempted = Some(old.clone());
        state.rx = Some(rx);
        let volume = view.volume.as_mut().unwrap();
        volume.apply_live(
            Arc::clone(&volume.scan),
            volume.name.clone(),
            volume.time,
            &[0.5],
        );
        let newer = key(&view);
        assert!(
            !state.wants(&newer),
            "do not burst workers while the source changes"
        );
        tx.send(VolumeDelivery {
            key: old.clone(),
            result: build(
                mixed_inputs(),
                &old,
                crate::colormap::default_table(Moment::Reflectivity),
                24,
                16,
            ),
        })
        .unwrap();
        let delivery = state.rx.as_ref().unwrap().try_recv().unwrap();
        state.rx = None;
        assert!(state.accept(delivery, Some(&newer)).is_none());
        assert!(state.accepted.is_none());
        assert!(state.wants(&newer));
        state.attempted = Some(old.clone());
        assert!(state
            .accept(
                VolumeDelivery {
                    key: old,
                    result: Err("obsolete failure".into())
                },
                Some(&newer)
            )
            .is_none());
        assert!(state.accepted.is_none());
        assert!(state.wants(&newer));
        state.attempted = Some(newer.clone());
        let accepted = state
            .accept(
                VolumeDelivery {
                    key: newer.clone(),
                    result: build(
                        mixed_inputs(),
                        &newer,
                        crate::colormap::default_table(Moment::Reflectivity),
                        24,
                        16,
                    ),
                },
                Some(&newer),
            )
            .unwrap()
            .unwrap();
        assert_eq!(state.accepted, Some(newer.clone()));
        assert_eq!(accepted.coverage.contributors.len(), 2);
        assert!(
            !state.wants(&newer),
            "reopening an accepted grid does not resample"
        );
    }

    #[test]
    fn a_failed_current_selection_is_remembered_until_retry_or_input_change() {
        let view = fixture_view();
        let mut wanted = key(&view);
        wanted.chosen = vec![90.0_f32.to_bits()];
        let mut state = VolumeBuildState {
            attempted: Some(wanted.clone()),
            ..Default::default()
        };
        let result = build(
            mixed_inputs(),
            &wanted,
            crate::colormap::default_table(Moment::Reflectivity),
            24,
            16,
        );
        assert!(result.is_err());
        assert!(state
            .accept(
                VolumeDelivery {
                    key: wanted.clone(),
                    result
                },
                Some(&wanted)
            )
            .unwrap()
            .is_err());
        assert!(!state.wants(&wanted));
        assert!(state.accepted.is_none());
        state.attempted = None;
        assert!(
            state.wants(&wanted),
            "explicit retry permits exactly another attempt"
        );
    }

    #[test]
    fn a_disconnected_worker_is_neutral_when_stale_and_requires_retry_when_current() {
        let view = fixture_view();
        let current = key(&view);
        let mut other = current.clone();
        other.palette += 1;
        let mut state = VolumeBuildState {
            attempted: Some(current.clone()),
            accepted: Some(current.clone()),
            ..Default::default()
        };
        assert!(state.ready(Some(&current)));
        assert!(!state.ready(Some(&other)));
        assert!(
            !state.ready(None),
            "removing the source hides the previous grid"
        );
        let (tx, rx) = std::sync::mpsc::channel();
        state.rx = Some(rx);
        assert!(
            !state.ready(Some(&current)),
            "a pending retry hides previous GPU content"
        );
        drop(tx);
        assert!(matches!(
            state.rx.as_ref().unwrap().try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ));
        assert!(state.disconnected(Some(&other)).is_none());
        assert!(state.wants(&other));
        state.attempted = Some(other.clone());
        assert!(state.disconnected(Some(&other)).is_some());
        assert!(
            !state.wants(&other),
            "a broken worker cannot respawn on every frame"
        );
    }

    fn mixed_inputs() -> Vec<BinnedSweep> {
        [0.5, 1.5]
            .into_iter()
            .map(|elevation_deg| BinnedSweep {
                moment: Moment::Reflectivity,
                az_bins: 8,
                gate_count: 8,
                data: vec![120; 64],
                first_gate_km: 5.0,
                gate_interval_km: 5.0,
                elevation_deg,
                value_min: -20.0,
                value_max: 80.0,
                bin_time_ms: [vec![1_700_000_120_000; 4], vec![1_700_000_000_000; 4]].concat(),
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn strict_resampling_and_selected_shells_keep_excluded_sectors_empty() {
        let view = fixture_view();
        let table = crate::colormap::default_table(Moment::Reflectivity);
        for chosen in [vec![], vec![0.5_f32.to_bits()]] {
            let mut wanted = key(&view);
            wanted.chosen = chosen;
            let inputs = mixed_inputs();
            let original = inputs[0].data.clone();
            let continuous = build(inputs.clone(), &wanted, table, 32, 16).unwrap();
            let reference = if wanted.chosen.is_empty() {
                wxdata::volume3d::build(&inputs, 32, 16, 50.0, VOL3D_TOP_KM)
            } else {
                wxdata::volume3d::build_shells(
                    &inputs[..1],
                    32,
                    16,
                    50.0,
                    VOL3D_TOP_KM,
                    crate::render3d::BEAMWIDTH_DEG,
                )
            }
            .unwrap();
            assert_eq!(
                continuous.upload.data,
                crate::render3d::pack_rg8(&reference.data),
                "continuous mode preserves existing samples"
            );
            wanted.policy = TemporalPolicy::StrictCurrent;
            let strict = build(inputs.clone(), &wanted, table, 32, 16).unwrap();
            let count = if wanted.chosen.is_empty() { 2 } else { 1 };
            assert_eq!(strict.coverage.contributors.len(), count);
            assert_eq!(strict.coverage.excluded_rows(), 4 * count);
            assert_eq!(
                strict.coverage.acquisition_range_ms(),
                Some((1_700_000_120_000, 1_700_000_120_000))
            );
            assert_eq!(
                strict.layers.len(),
                2,
                "available tilt selection remains accessible"
            );
            assert!(strict.layers.iter().all(
                |layer| layer.scan_start == DateTime::from_timestamp_millis(1_700_000_120_000)
            ));
            let mut removed = 0;
            let mut kept = 0;
            for k in 0..16 {
                for j in 0..32 {
                    for i in 0..32 {
                        let offset = 2 * (i + 32 * j + 32 * 32 * k);
                        if continuous.upload.data[offset] >= 2 {
                            if i < 16 {
                                assert_eq!(
                                    &strict.upload.data[offset..offset + 2],
                                    &[0, 0],
                                    "west/older sectors stay transparent at every height"
                                );
                                removed += 1;
                            } else {
                                assert_eq!(
                                    &strict.upload.data[offset..offset + 2],
                                    &continuous.upload.data[offset..offset + 2]
                                );
                                kept += 1;
                            }
                        }
                    }
                }
            }
            assert!(
                removed > 100 && kept > 100,
                "control must exercise both sides: {removed}/{kept}"
            );
            assert_eq!(inputs[0].data, original, "source cache stays unmasked");
        }
    }
}
