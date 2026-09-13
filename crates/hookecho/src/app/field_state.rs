//! Field delivery state, shared by pane rendering and the provenance inspector.
use super::{HookEchoApp, Instant, PrecipGrid};
use crate::render::{FieldLayer, MrmsUpload};
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
