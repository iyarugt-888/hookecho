//! Field delivery state, shared by pane rendering and the provenance inspector.
use super::{HookEchoApp, Instant, PrecipGrid};
use crate::render::{FieldLayer, MrmsUpload};
use wxdata::time_align::TimeOffset;
use wxdata::{field::DataStamp, mrms::MrmsField};

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
