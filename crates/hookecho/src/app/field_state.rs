//! Field delivery state, shared by pane rendering and the provenance inspector.
use super::{HookEchoApp, Instant, PrecipGrid};
use crate::render::{FieldLayer, MrmsUpload};
use wxdata::time_align::TimeOffset;
use wxdata::{
    field::{DataStamp, QualitySummary, Stamped},
    mrms::MrmsField,
};

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
    pub last_fetch: Option<Instant>,
    /// Since when no pane has drawn this layer; drives GPU texture eviction.
    pub off_since: Option<Instant>,
    pub stamp: Option<DataStamp>,
}

impl HookEchoApp {
    /// Enabled stamped fields whose valid times do not match the radar scan on screen.
    pub(crate) fn field_time_mismatches(&self) -> Vec<(FieldLayer, chrono::Duration)> {
        let view = &self.views[self.active];
        let Some(volume) = view.volume.as_ref() else {
            return Vec::new();
        };
        let tolerance = chrono::Duration::minutes(self.settings.time_mismatch_minutes as i64);
        let mut mismatches: Vec<_> = view
            .fields_on
            .iter()
            .filter_map(|layer| {
                let stamp = self.fields.get(layer)?.stamp.as_ref()?;
                let comparison = TimeOffset::between(stamp.valid_time, volume.time, tolerance);
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
            state.pending = Some(upload);
            state.stamp = stamp;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
