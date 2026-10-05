//! Background overlay acquisition owns request generations and health independently of the UI.
//! Callers provide a source selection and display size; delivery remains on the existing channel.

use super::{OverlayDelivery, OverlayMsg, OverlaySource, RequestBook, RequestLane, SourceHealth};
use crate::rt::Spawner;
use chrono::{DateTime, Utc};
use std::sync::{mpsc::Sender, Mutex, MutexGuard};

/// Shorter than the fastest field cadence, but longer than the HTTP deadline so the browser
/// aborts its request before we drop the future. A hung feed cannot accumulate work every tick.
const OVERLAY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(55);

pub(super) struct OverlayAcquisition {
    http: reqwest::Client,
    spawner: Spawner,
    sender: Sender<OverlayDelivery>,
    requests: Mutex<RequestBook>,
}

impl OverlayAcquisition {
    pub(super) fn new(
        http: reqwest::Client,
        spawner: Spawner,
        sender: Sender<OverlayDelivery>,
    ) -> Self {
        Self {
            http,
            spawner,
            sender,
            requests: Mutex::new(RequestBook::default()),
        }
    }

    fn requests(&self) -> MutexGuard<'_, RequestBook> {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Local derived work uses the same generation/health contract as fetched overlays.
    pub(super) fn start(&self, lane: RequestLane) -> u64 {
        self.requests().start(lane)
    }

    pub(super) fn finish(
        &self,
        lane: &RequestLane,
        generation: u64,
        error: Option<&str>,
        valid_time: Option<DateTime<Utc>>,
    ) -> bool {
        self.requests().finish(lane, generation, error, valid_time)
    }

    pub(super) fn health(&self, lane: &RequestLane) -> SourceHealth {
        self.requests().health(lane)
    }

    pub(super) fn discard(&self, lane: &RequestLane, generation: u64) {
        self.requests().discard(lane, generation);
    }

    pub(super) fn reset(&self, lane: &RequestLane) {
        self.requests().reset(lane);
    }

    pub(super) fn set_cache_resident(&self, lane: &RequestLane, resident: bool) {
        self.requests().set_cache_resident(lane, resident);
    }

    pub(super) fn spawn(&self, ctx: &egui::Context, source: OverlaySource, cap: usize) {
        let lane = source.lane();
        let generation = self.start(lane.clone());
        let model_request = super::ModelRequest::from_source(&source);
        let http = self.http.clone();
        let tx = self.sender.clone();
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            let result = match wxdata::task::timeout(OVERLAY_TIMEOUT, source.fetch(&http))
                .await
                .unwrap_or_else(Err)
            {
                // Oversized grids are prepared off-frame, before their delivery to the UI.
                Ok(msg) => Ok(prepare_message(msg, cap)),
                Err(e) => Err(e.to_string()),
            };
            let _ = tx.send(OverlayDelivery::Fetched {
                lane,
                generation,
                model_request,
                result,
            });
            ctx.request_repaint();
        });
    }
}

fn prepare_message(msg: OverlayMsg, cap: usize) -> OverlayMsg {
    match msg {
        OverlayMsg::Field(layer, f) => OverlayMsg::Field(layer, f.decimated(cap)),
        OverlayMsg::StampedField(layer, f) => {
            let kind = layer
                .descriptor()
                .map_or(wxdata::field::ValueKind::Scalar, |d| d.value_kind);
            OverlayMsg::StampedField(layer, f.for_display(cap, kind))
        }
        OverlayMsg::MrmsField(layer, f, request) => {
            let kind = layer
                .descriptor()
                .map_or(wxdata::field::ValueKind::Scalar, |d| d.value_kind);
            OverlayMsg::MrmsField(layer, f.for_display(cap, kind), request)
        }
        other => other,
    }
}
