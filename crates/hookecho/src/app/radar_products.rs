//! Local radar product identity and delivery. A worker answers a specific accepted scan revision;
//! source timing travels with that answer rather than being reconstructed from its volume label.
use super::*;
pub(super) use crate::volume::ScanIdentity;
use wxdata::level2::temporal::{TemporalCoverage, TemporalPolicy};

pub(super) const LAYERS: [crate::render::FieldLayer; 6] = {
    use crate::render::FieldLayer as F;
    [
        F::CompositeLocal,
        F::VilLocal,
        F::VilDensity,
        F::EtopLocal,
        F::HailMehs,
        F::HailPosh,
    ]
};
pub(super) const HAIL_BITS: u8 = 0b110000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DerivedKey {
    pub site: Option<String>,
    pub volume: String,
    pub revision: u64,
    scan: ScanIdentity,
    acquisition: Option<crate::live_scan::AcquisitionSnapshot>,
    pub policy: TemporalPolicy,
    pub etop_bits: u32,
    pub layers: u8,
    pub freezing_bits: Option<(u64, u64)>,
}

pub(super) fn policy(view: &MapView, settings: &Settings) -> TemporalPolicy {
    if settings.live_sweep_mode == crate::settings::LiveSweepMode::StrictCurrentSweep
        && view.timeline.following
        && !view.timeline.playing
        && view.volume.as_ref().is_some_and(Volume::is_live_partial)
    {
        TemporalPolicy::StrictCurrent
    } else {
        TemporalPolicy::Continuous
    }
}

pub(crate) struct DerivedDelivery {
    pub(super) key: DerivedKey,
    pub fields: Result<
        (
            TemporalCoverage,
            Vec<(crate::render::FieldLayer, wxdata::mrms::MrmsField)>,
        ),
        String,
    >,
}

#[derive(Clone, Debug)]
pub(crate) struct RadarMetadata {
    pub(super) key: DerivedKey,
    pub coverage: TemporalCoverage,
}

impl RadarMetadata {
    pub(crate) fn acquisition(&self) -> Option<&crate::live_scan::AcquisitionSnapshot> {
        self.key.acquisition.as_ref()
    }
}

impl HookEchoApp {
    pub(super) fn current_derived_key(&self) -> Option<DerivedKey> {
        let view = &self.views[self.active];
        let layers = LAYERS.iter().enumerate().fold(0, |mask, (i, layer)| {
            mask | (u8::from(self.field_wanted(*layer)) << i)
        });
        DerivedKey::for_view(view, &self.settings, layers, self.freezing_for(self.active))
    }

    /// Shared fields can only draw in a pane showing the source volume and accepted revision.
    pub(crate) fn radar_field_ready(&self, idx: usize, layer: crate::render::FieldLayer) -> bool {
        if !LAYERS.contains(&layer) {
            return true;
        }
        let Some(metadata) = self
            .fields
            .get(&layer)
            .and_then(|state| state.radar.as_ref())
        else {
            return false;
        };
        let view = &self.views[idx];
        let key = &metadata.key;
        key.matches_view(view, &self.settings)
            && key.freezing_bits == freezing_bits(key.layers, self.freezing_for(idx))
    }

    pub(super) fn accept_derived_fields(&mut self, delivery: DerivedDelivery) {
        if !self.derived_key_current(&delivery.key) {
            return;
        }
        let Ok((coverage, fields)) = delivery.fields else {
            return;
        };
        let metadata = Arc::new(RadarMetadata {
            key: delivery.key,
            coverage,
        });
        for (layer, field) in fields {
            // The build key, exact source coverage, grid and upload commit as one result.
            self.accept_field(layer, field, None);
            if let Some(state) = self.fields.get_mut(&layer) {
                state.radar = Some(Arc::clone(&metadata));
            }
        }
    }

    pub(super) fn derived_key_current(&self, key: &DerivedKey) -> bool {
        key.is_current(
            self.derived_key.as_ref(),
            self.current_derived_key().as_ref(),
        )
    }
}

impl DerivedKey {
    fn for_view(
        view: &MapView,
        settings: &Settings,
        layers: u8,
        levels: Option<(f64, f64)>,
    ) -> Option<Self> {
        let vol = view.volume.as_ref()?;
        (layers != 0).then(|| Self {
            site: view.site.clone(),
            volume: vol.name.clone(),
            revision: vol.revision(),
            scan: ScanIdentity::new(&vol.scan),
            acquisition: vol.acquisition_for(view.site.as_deref()).cloned(),
            policy: policy(view, settings),
            etop_bits: settings.etop_dbz.to_bits(),
            layers,
            freezing_bits: freezing_bits(layers, levels),
        })
    }

    fn is_current(&self, requested: Option<&Self>, actual: Option<&Self>) -> bool {
        requested == Some(self) && actual == Some(self)
    }

    fn matches_view(&self, view: &MapView, settings: &Settings) -> bool {
        self.site == view.site
            && view.volume.as_ref().is_some_and(|v| {
                self.volume == v.name
                    && self.revision == v.revision()
                    && self.scan.matches(&v.scan)
                    && self.acquisition.as_ref() == v.acquisition_for(view.site.as_deref())
            })
            && self.policy == policy(view, settings)
            && self.etop_bits == settings.etop_dbz.to_bits()
    }
}

fn freezing_bits(layers: u8, levels: Option<(f64, f64)>) -> Option<(u64, u64)> {
    if layers & HAIL_BITS != 0 {
        levels.map(|(a, b)| (a.to_bits(), b.to_bits()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_hail_layers_and_neither_echo_top_layer_require_temperature_levels() {
        let hail: Vec<_> = LAYERS
            .iter()
            .enumerate()
            .filter_map(|(i, l)| (HAIL_BITS & (1 << i) != 0).then_some(*l))
            .collect();
        assert_eq!(
            hail,
            [
                crate::render::FieldLayer::HailMehs,
                crate::render::FieldLayer::HailPosh
            ]
        );
    }

    #[test]
    fn live_merge_and_selection_changes_reject_old_workers_before_a_new_build_starts() {
        let mut view = fixture_view();
        let mut settings = Settings::default();
        let key = DerivedKey::for_view(&view, &settings, 63, Some((3000.0, 6000.0))).unwrap();
        assert!(key.is_current(Some(&key), Some(&key)));
        let independently_acquired = fixture_view();
        assert!(
            !key.matches_view(&independently_acquired, &settings),
            "matching names and local counters do not prove two panes have identical inputs"
        );
        let volume = view.volume.as_mut().unwrap();
        volume.apply_live(
            Arc::clone(&volume.scan),
            volume.name.clone(),
            volume.time,
            &[0.5],
        );
        let next = DerivedKey::for_view(&view, &settings, 63, Some((3000.0, 6000.0))).unwrap();
        assert!(
            !key.is_current(Some(&key), Some(&next)),
            "arrival before recompute must be rejected"
        );
        assert!(
            !key.is_current(Some(&next), Some(&next)),
            "old completion cannot replace accepted revision"
        );
        assert!(!key.matches_view(&view, &settings));
        assert!(next.matches_view(&view, &settings));
        settings.live_sweep_mode = crate::settings::LiveSweepMode::StrictCurrentSweep;
        assert!(!next.matches_view(&view, &settings));
        let strict = DerivedKey::for_view(&view, &settings, 63, Some((3000.0, 6000.0))).unwrap();
        assert_eq!(strict.policy, TemporalPolicy::StrictCurrent);
        view.timeline.playing = true;
        assert_eq!(policy(&view, &settings), TemporalPolicy::Continuous);
        view.timeline.playing = false;
        view.site = Some("KTLX".into());
        assert!(!strict.matches_view(&view, &settings));
        view.volume = None;
        assert_eq!(DerivedKey::for_view(&view, &settings, 63, None), None);
    }

    #[test]
    fn hail_rebuilds_for_either_level_without_rounding_and_archive_keeps_all_passes() {
        let mut view = fixture_view();
        let settings = Settings {
            live_sweep_mode: crate::settings::LiveSweepMode::StrictCurrentSweep,
            ..Default::default()
        };
        let volume = view.volume.take().unwrap();
        view.volume = Some(Volume::new(volume.scan, volume.name, volume.time));
        let key =
            DerivedKey::for_view(&view, &settings, HAIL_BITS, Some((3000.0, 6000.0))).unwrap();
        assert_eq!(key.policy, TemporalPolicy::Continuous);
        for levels in [(3000.1, 6000.0), (3000.0, 6000.1)] {
            assert_ne!(
                key,
                DerivedKey::for_view(&view, &settings, HAIL_BITS, Some(levels)).unwrap()
            );
        }
        let etop = 1 << 3;
        assert_eq!(
            DerivedKey::for_view(&view, &settings, etop, None),
            DerivedKey::for_view(&view, &settings, etop, Some((3000.0, 6000.0)))
        );
    }

    fn fixture_view() -> MapView {
        let scan = level2::decode_volume(
            include_bytes!("../../../wxdata/tests/data/corpus/mayfield-2021-first-records.ar2")
                .to_vec(),
        )
        .unwrap();
        let time = DateTime::from_timestamp(1_639_193_029, 0).unwrap();
        let mut view = MapView::new(Some("KPAH".into()), Camera::at_lonlat(-88.0, 37.0, 8.0));
        view.timeline.following = true;
        view.volume = Some(Volume::from_live(
            Arc::new(scan),
            "KPAH20211211_032349_V06".into(),
            time,
        ));
        view
    }

    #[test]
    fn derived_delivery_retains_its_receipt_and_rejects_later_raw_context() {
        let mut view = fixture_view();
        let settings = Settings::default();
        let old = view.volume.take().unwrap();
        let (mut receiver, receipt) = crate::live_scan::acquisition_fixture("KPAH");
        let weak = Arc::downgrade(&old.scan);
        view.volume = Some(Volume::from_live_captured(
            old.scan,
            old.name,
            old.time,
            Some(receipt.clone()),
        ));
        let key = DerivedKey::for_view(&view, &settings, 63, None).unwrap();
        let metadata = RadarMetadata {
            key: key.clone(),
            coverage: TemporalCoverage {
                policy: TemporalPolicy::Continuous,
                contributors: Vec::new(),
            },
        };
        assert_eq!(metadata.acquisition(), Some(&receipt));
        assert!(key.matches_view(&view, &settings));
        let next_receipt = receiver
            .capture_acquisition(
                wxdata::live::RadialCoverage {
                    progress: receiver.progress.unwrap(),
                    source_passes: None,
                    source_sequences: None,
                    radials: vec![(3, 0)],
                },
                Utc::now(),
            )
            .unwrap();
        let vol = view.volume.as_mut().unwrap();
        vol.apply_live_captured(
            vol.scan.clone(),
            vol.name.clone(),
            vol.time,
            &[0.5],
            Some(next_receipt.clone()),
        );
        let next = DerivedKey::for_view(&view, &settings, 63, None).unwrap();
        assert!(!key.is_current(Some(&next), Some(&next)));
        assert!(!key.matches_view(&view, &settings));
        assert_eq!(next.acquisition.as_ref(), Some(&next_receipt));
        assert_eq!(metadata.acquisition(), Some(&receipt));
        view.site = Some("KTLX".into());
        assert!(!next.matches_view(&view, &settings));
        assert_eq!(
            DerivedKey::for_view(&view, &settings, 63, None)
                .unwrap()
                .acquisition,
            None
        );
        drop(view);
        assert!(
            weak.upgrade().is_none(),
            "metadata retains summaries rather than gate buffers"
        );
        assert_eq!(metadata.acquisition(), Some(&receipt));
    }
}
