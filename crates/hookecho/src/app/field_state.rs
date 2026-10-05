//! Field delivery state, shared by pane rendering and the provenance inspector.
use super::{HookEchoApp, PrecipGrid};
use crate::render::{FieldLayer, MrmsUpload};
use wxdata::clock::Instant;
use wxdata::time_align::TimeOffset;
use wxdata::{
    field::{DataStamp, QualitySummary, Stamped},
    mrms::MrmsField,
};

/// The selected cursor travels with an MRMS reply so late frames cannot replace a new choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MrmsRequest {
    pub(super) product: String,
    pub(super) archive: Option<(chrono::DateTime<chrono::Utc>, u16)>,
}

impl MrmsRequest {
    pub(super) fn accepts(&self, stamp: &wxdata::field::DataStamp) -> bool {
        stamp.product_id == self.product
            && self.archive.is_none_or(|(target, minutes)| {
                !wxdata::time_align::TimeOffset::between(
                    stamp.valid_time,
                    target,
                    chrono::Duration::minutes(minutes as i64),
                )
                .outside_tolerance
            })
    }
}

/// Attach the decoded model field's own valid time and source cycle before display decimation.
/// The app sees the completed decode here, so `received_time` is local delivery time; provider
/// ingest latency remains unknown rather than being inferred from a forecast's future valid time.
pub(super) fn model_field(
    source: &str,
    product: &str,
    field: MrmsField,
    run: Option<chrono::DateTime<chrono::Utc>>,
    expected_valid: chrono::DateTime<chrono::Utc>,
    is_derived: bool,
) -> anyhow::Result<Stamped<MrmsField>> {
    anyhow::ensure!(
        field.time == expected_valid,
        "{source} {product} decoded valid time {} differs from run/lead time {expected_valid}",
        field.time
    );
    let stamp = model_stamp(source, product, &field, run, is_derived);
    Ok(Stamped { data: field, stamp })
}

pub(super) fn model_stamp(
    source: &str,
    product: &str,
    field: &MrmsField,
    run: Option<chrono::DateTime<chrono::Utc>>,
    is_derived: bool,
) -> DataStamp {
    DataStamp {
        source_id: source.into(),
        product_id: product.into(),
        issue_time: None,
        run_time: run,
        valid_time: field.time,
        received_time: chrono::Utc::now(),
        source_latency: None,
        is_forecast: run.is_none_or(|cycle| field.time > cycle),
        is_derived,
        quality: QualitySummary::Unknown,
        grid: None,
    }
}

#[derive(Default)]
pub(crate) struct FieldState {
    pub pending: Option<MrmsUpload>,
    /// The same decoded/decimated grid represented by the resident texture. Retained so linked
    /// probes and future scientific exports sample the displayed data rather than reverse-
    /// engineering an 8-bit GPU index. Cleared with the texture by the existing field eviction.
    pub grid: Option<MrmsField>,
    pub last_fetch: Option<Instant>,
    /// Since when no pane has drawn this layer; drives GPU texture eviction.
    pub off_since: Option<Instant>,
    pub stamp: Option<DataStamp>,
    /// Exact local radar contributors represented by the resident texture.
    pub radar: Option<std::sync::Arc<super::radar_products::RadarMetadata>>,
    mrms_request: Option<MrmsRequest>,
    pub(super) model_requested: Option<super::ModelRequest>,
    pub(super) model_accepted: Option<super::ModelRequest>,
}

impl FieldState {
    pub(super) fn model_due(
        &self,
        request: super::ModelRequest,
        cadence: std::time::Duration,
        now: Instant,
    ) -> bool {
        self.model_requested != Some(request)
            || self
                .last_fetch
                .is_none_or(|last| now.saturating_duration_since(last) >= cadence)
    }

    pub(super) fn begin_model(&mut self, request: super::ModelRequest, now: Instant) -> bool {
        let changed = self.model_requested != Some(request);
        if changed {
            self.pending = None;
            self.model_accepted = None;
            self.model_requested = Some(request);
        }
        self.last_fetch = Some(now);
        changed
    }

    pub(super) fn model_ready(&self, request: super::ModelRequest) -> bool {
        self.model_requested == Some(request)
            && self.model_accepted == Some(request)
            && self.grid.is_some()
            && self
                .stamp
                .as_ref()
                .is_some_and(|stamp| request.accepts(stamp))
    }

    /// A hidden old context can retain its bounded queued upload. Only an exact selected
    /// context may consume it; beginning a replacement request clears it and its admission.
    pub(super) fn take_upload(
        &mut self,
        selected: Option<super::ModelRequest>,
    ) -> Option<MrmsUpload> {
        if selected.is_some_and(|request| !self.model_ready(request)) {
            return None;
        }
        self.pending.take()
    }

    pub(super) fn stage_model(
        &mut self,
        request: super::ModelRequest,
        field: Stamped<MrmsField>,
        upload: MrmsUpload,
    ) -> bool {
        if self.model_requested != Some(request)
            || !request.accepts(&field.stamp)
            || field.data.time != field.stamp.valid_time
        {
            return false;
        }
        self.stage(field.data, Some(field.stamp), upload);
        self.model_accepted = Some(request);
        true
    }
    /// Selection changes bypass the cadence, but hidden layers never start a request.
    pub(super) fn mrms_due(
        &self,
        request: &MrmsRequest,
        wanted: bool,
        cadence: std::time::Duration,
        now: Instant,
    ) -> bool {
        wanted
            && (self.mrms_request.as_ref() != Some(request)
                || self
                    .last_fetch
                    .is_none_or(|last| now.saturating_duration_since(last) >= cadence))
    }

    /// Invalidate the old selection's pending upload/provenance before starting replacement work.
    /// The retained grid/texture cannot draw until `mrms_ready` confirms the selected request.
    pub(super) fn begin_mrms(&mut self, request: MrmsRequest, now: Instant) -> bool {
        let changed = self.mrms_request.as_ref() != Some(&request);
        if changed {
            self.pending = None;
            self.stamp = None;
            self.mrms_request = Some(request);
        }
        self.last_fetch = Some(now);
        changed
    }

    pub(super) fn mrms_ready(&self, request: &MrmsRequest) -> bool {
        self.mrms_request.as_ref() == Some(request)
            && self
                .stamp
                .as_ref()
                .is_some_and(|stamp| request.accepts(stamp))
    }

    pub(super) fn mrms_delivered(&mut self, request: MrmsRequest, now: Instant) {
        self.mrms_request = Some(request);
        self.last_fetch = Some(now);
    }

    /// Commit a decoded grid, its optional provenance and matching upload as one delivery.
    pub(super) fn stage(&mut self, field: MrmsField, stamp: Option<DataStamp>, upload: MrmsUpload) {
        self.pending = Some(upload);
        self.stamp = stamp;
        self.radar = None;
        self.model_accepted = None;
        self.grid = Some(field);
    }
}

impl HookEchoApp {
    /// Enabled stamped fields whose valid times exceed the selected analysis tolerance.
    pub(crate) fn field_time_mismatches(&self) -> Vec<(FieldLayer, chrono::Duration)> {
        let view = &self.views[self.active];
        let Some(analysis_time) = self
            .linked_analysis_time()
            .or_else(|| view.volume.as_ref().map(|volume| volume.time))
        else {
            return Vec::new();
        };
        let tolerance = chrono::Duration::minutes(self.settings.time_mismatch_minutes as i64);
        let mut mismatches: Vec<_> = view
            .fields_on
            .iter()
            .filter_map(|layer| {
                if !self.mrms_ready(*layer) {
                    return None;
                }
                let stamp = self.fields.get(layer)?.stamp.as_ref()?;
                let comparison = TimeOffset::between(stamp.valid_time, analysis_time, tolerance);
                comparison
                    .outside_tolerance
                    .then_some((*layer, comparison.offset))
            })
            .collect();
        mismatches.sort_by_key(|(layer, _)| layer.slug());
        mismatches
    }

    pub(super) fn accept_field(
        &mut self,
        layer: FieldLayer,
        field: MrmsField,
        stamp: Option<DataStamp>,
    ) {
        if layer == FieldLayer::Lightning {
            self.check_lightning_proximity(&field);
        }
        // Keep precipitation categories for the radar tint as well as their own layer.
        if layer == FieldLayer::PrecipType {
            self.precip_flag_grid = Some(std::sync::Arc::new(PrecipGrid::new(&field)));
            self.precip_flag_gen = self.precip_flag_gen.wrapping_add(1);
        }
        let upload = self.field_upload(layer, &field);
        if let Some(state) = self.fields.get_mut(&layer) {
            state.stage(field, stamp, upload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(time: chrono::DateTime<chrono::Utc>, value: f32) -> MrmsField {
        MrmsField {
            values: vec![value],
            nx: 1,
            ny: 1,
            lon_west: -100.0,
            lon_east: -100.0,
            lat_north: 35.0,
            lat_south: 35.0,
            time,
        }
    }

    fn upload(value: u8) -> MrmsUpload {
        MrmsUpload {
            data: vec![value],
            nx: 1,
            ny: 1,
            world_min: [0.0; 2],
            world_max: [1.0; 2],
            uniform: [0.0; 12],
            lut: vec![0; 256 * 4],
        }
    }

    #[test]
    fn changing_to_archive_cannot_upload_or_label_the_previous_live_grid() {
        let live_time = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let live = MrmsRequest {
            product: "CONUS/Test".into(),
            archive: None,
        };
        let archive = MrmsRequest {
            product: live.product.clone(),
            archive: Some((live_time - chrono::Duration::hours(2), 2)),
        };
        let now = Instant::now();
        let field = fixture(live_time, 30.0);
        let stamp = model_stamp("MRMS", &live.product, &field, None, false);
        let mut state = FieldState::default();
        state.begin_mrms(live.clone(), now);
        state.stage(field, Some(stamp), upload(30));
        assert!(state.mrms_ready(&live));
        assert!(state.begin_mrms(archive.clone(), now));
        assert!(
            state.pending.is_none(),
            "a queued live upload must not reach the archive view"
        );
        assert!(
            state.stamp.is_none(),
            "no live timestamp on a pending archive selection"
        );
        assert!(!state.mrms_ready(&archive));
        assert!(!state.mrms_ready(&live));
        assert_eq!(state.grid.as_ref().unwrap().time, live_time);

        let archive_time = archive.archive.unwrap().0;
        let field = fixture(archive_time, 15.0);
        let stamp = model_stamp("MRMS", &archive.product, &field, None, false);
        state.stage(field, Some(stamp), upload(15));
        state.mrms_delivered(archive.clone(), now);
        assert!(state.mrms_ready(&archive));
        assert!(!state.mrms_ready(&live));
        assert_eq!(state.grid.as_ref().unwrap().values, [15.0]);
    }

    #[test]
    fn scheduling_honors_hidden_state_cadence_and_immediate_selection_changes() {
        let live = MrmsRequest {
            product: "CONUS/Test".into(),
            archive: None,
        };
        let other = MrmsRequest {
            product: "CONUS/Other".into(),
            archive: None,
        };
        let now = Instant::now();
        let cadence = std::time::Duration::from_secs(120);
        let mut state = FieldState::default();
        assert!(!state.mrms_due(&live, false, cadence, now));
        assert!(state.mrms_due(&live, true, cadence, now));
        state.begin_mrms(live.clone(), now);
        assert!(!state.mrms_due(
            &live,
            true,
            cadence,
            now + cadence - std::time::Duration::from_nanos(1)
        ));
        assert!(state.mrms_due(&live, true, cadence, now + cadence));
        assert!(state.mrms_due(&other, true, cadence, now));
        assert!(!state.mrms_due(&other, false, cadence, now + cadence));
    }

    #[test]
    fn same_selection_refresh_keeps_last_good_data_and_unknown_delivery_clears_old_stamp() {
        let valid = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let request = MrmsRequest {
            product: "CONUS/Test".into(),
            archive: None,
        };
        let now = Instant::now();
        let mut state = FieldState::default();
        state.begin_mrms(request.clone(), now);
        let field = fixture(valid, 20.0);
        let stamp = model_stamp("MRMS", &request.product, &field, None, false);
        state.stage(field, Some(stamp), upload(20));
        assert!(!state.begin_mrms(request.clone(), now + std::time::Duration::from_secs(120)));
        assert!(state.mrms_ready(&request));
        assert_eq!(state.pending.as_ref().unwrap().data, [20]);
        state.stage(
            fixture(valid + chrono::Duration::minutes(2), 35.0),
            None,
            upload(35),
        );
        assert!(state.stamp.is_none());
        assert!(
            !state.mrms_ready(&request),
            "an untimed delivery cannot reuse old provenance"
        );
        assert_eq!(state.grid.as_ref().unwrap().values, [35.0]);
        assert_eq!(state.pending.as_ref().unwrap().data, [35]);
    }

    #[test]
    fn mrms_request_rejects_other_products_and_times_outside_archive_tolerance() {
        let target = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let field = MrmsField {
            values: vec![1.0],
            nx: 1,
            ny: 1,
            lon_west: -100.0,
            lon_east: -100.0,
            lat_north: 35.0,
            lat_south: 35.0,
            time: target,
        };
        let mut stamp = model_stamp("MRMS", "CONUS/Test", &field, None, true);
        let request = MrmsRequest {
            product: "CONUS/Test".into(),
            archive: Some((target, 2)),
        };
        assert!(request.accepts(&stamp));
        stamp.valid_time = target + chrono::Duration::minutes(2);
        assert!(request.accepts(&stamp));
        stamp.valid_time += chrono::Duration::seconds(1);
        assert!(!request.accepts(&stamp));
        stamp.valid_time = target;
        stamp.product_id = "CONUS/Other".into();
        assert!(!request.accepts(&stamp));
    }

    #[test]
    fn model_provenance_uses_decoded_valid_time_and_keeps_run() {
        let run = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let valid = run + chrono::Duration::hours(3);
        let field = MrmsField {
            values: vec![1.0],
            nx: 1,
            ny: 1,
            lon_west: -100.0,
            lon_east: -100.0,
            lat_north: 35.0,
            lat_south: 35.0,
            time: valid,
        };
        let stamped = model_field("GFS", "mslp", field.clone(), Some(run), valid, false).unwrap();
        assert_eq!(stamped.stamp.valid_time, valid);
        assert_eq!(stamped.stamp.run_time, Some(run));
        assert!(stamped.stamp.is_forecast);
        assert!(stamped.stamp.source_latency.is_none());
        assert!(model_field("GFS", "mslp", field, Some(run), run, false).is_err());
        let mut analysis = stamped.data;
        analysis.time = run;
        assert!(
            !model_field("GFS", "mslp", analysis, Some(run), run, false)
                .unwrap()
                .stamp
                .is_forecast
        );
    }
}
