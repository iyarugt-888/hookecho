//! Background overlay acquisition owns request generations and health independently of the UI.
//! Callers provide a source selection and display size; delivery remains on the existing channel.

use super::{OverlayDelivery, OverlayMsg, OverlaySource, RequestBook, RequestLane, SourceHealth};
use crate::rt::Spawner;
use chrono::{DateTime, Utc};
use futures_util::future::{AbortHandle, Abortable};
use std::sync::{mpsc::Sender, Mutex, MutexGuard};

/// Shorter than the fastest field cadence, but longer than the HTTP deadline so the browser
/// aborts its request before we drop the future. A hung feed cannot accumulate work every tick.
const OVERLAY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(55);

pub(super) struct OverlayAcquisition {
    http: reqwest::Client,
    spawner: Spawner,
    sender: Sender<OverlayDelivery>,
    requests: Mutex<RequestBook>,
    model_jobs: Mutex<std::collections::HashMap<super::ModelRequest, (u64, AbortHandle)>>,
    goes_jobs: Mutex<std::collections::HashMap<super::GoesRequest, (u64, AbortHandle)>>,
    mrms_jobs: Mutex<std::collections::HashMap<super::MrmsContext, (u64, AbortHandle)>>,
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
            model_jobs: Mutex::new(Default::default()),
            goes_jobs: Mutex::new(Default::default()),
            mrms_jobs: Mutex::new(Default::default()),
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
        if let RequestLane::Model(request) = lane {
            let mut jobs = self
                .model_jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if jobs.get(request).is_some_and(|(g, _)| *g == generation) {
                jobs.remove(request);
            }
        }
        if let RequestLane::Mrms(request) = lane {
            let mut jobs = self
                .mrms_jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if jobs.get(request).is_some_and(|(g, _)| *g == generation) {
                jobs.remove(request);
            }
        }
        if let RequestLane::Goes(request) = lane {
            let mut jobs = self
                .goes_jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if jobs.get(request).is_some_and(|(g, _)| *g == generation) {
                jobs.remove(request);
            }
        }
        self.requests().finish(lane, generation, error, valid_time)
    }

    pub(super) fn health(&self, lane: &RequestLane) -> SourceHealth {
        self.requests().health(lane)
    }

    pub(super) fn discard(&self, lane: &RequestLane, generation: u64) {
        if let RequestLane::Goes(request) = lane {
            let mut jobs = self
                .goes_jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if jobs.get(request).is_some_and(|(g, _)| *g == generation) {
                if let Some((_, handle)) = jobs.remove(request) {
                    handle.abort();
                }
            }
        }
        self.requests().discard(lane, generation);
    }

    pub(super) fn reset(&self, lane: &RequestLane) {
        if let RequestLane::Model(request) = lane {
            if let Some((_, handle)) = self
                .model_jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(request)
            {
                handle.abort();
            }
        }
        if let RequestLane::Mrms(request) = lane {
            if let Some((_, handle)) = self
                .mrms_jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(request)
            {
                handle.abort();
            }
        }
        if let RequestLane::Goes(request) = lane {
            if let Some((_, handle)) = self
                .goes_jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(request)
            {
                handle.abort();
            }
        }
        self.requests().reset(lane);
    }

    /// Retiring the last subscriber cancels transport work without inventing a source failure.
    pub(super) fn cancel_unwanted_models(
        &self,
        wanted: &std::collections::HashSet<super::ModelRequest>,
    ) -> Vec<super::ModelRequest> {
        let mut jobs = self
            .model_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let retired: Vec<_> = jobs
            .keys()
            .filter(|request| !wanted.contains(request))
            .copied()
            .collect();
        for request in &retired {
            if let Some((generation, handle)) = jobs.remove(request) {
                handle.abort();
                self.requests()
                    .discard(&RequestLane::Model(*request), generation);
            }
        }
        retired
    }

    /// Retiring the last subscriber cancels transport work without inventing a source failure.
    pub(super) fn cancel_unwanted_mrms(
        &self,
        wanted: &std::collections::HashSet<super::MrmsContext>,
    ) -> Vec<super::MrmsContext> {
        let mut jobs = self
            .mrms_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let retired: Vec<_> = jobs
            .keys()
            .filter(|request| !wanted.contains(request))
            .copied()
            .collect();
        for request in &retired {
            if let Some((generation, handle)) = jobs.remove(request) {
                handle.abort();
                self.requests()
                    .discard(&RequestLane::Mrms(*request), generation);
            }
        }
        retired
    }

    pub(super) fn cancel_unwanted_goes(
        &self,
        wanted: &std::collections::HashSet<super::GoesRequest>,
    ) -> Vec<super::GoesRequest> {
        let mut jobs = self
            .goes_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let retired: Vec<_> = jobs
            .keys()
            .filter(|r| !wanted.contains(r))
            .copied()
            .collect();
        for request in &retired {
            if let Some((generation, handle)) = jobs.remove(request) {
                handle.abort();
                self.requests()
                    .discard(&RequestLane::Goes(*request), generation);
            }
        }
        retired
    }
    pub(super) fn spawn_goes(&self, ctx: &egui::Context, request: super::GoesRequest, cap: usize) {
        let source = request.source();
        debug_assert_eq!(
            super::GoesRequest::from_source(&source, request.tolerance_minutes),
            Some(request)
        );
        self.spawn_lane(ctx, source, cap, RequestLane::Goes(request));
    }

    pub(super) fn set_cache_resident(&self, lane: &RequestLane, resident: bool) {
        self.requests().set_cache_resident(lane, resident);
    }

    pub(super) fn spawn(&self, ctx: &egui::Context, source: OverlaySource, cap: usize) {
        let lane = source.lane();
        self.spawn_lane(ctx, source, cap, lane);
    }
    pub(super) fn spawn_model(
        &self,
        ctx: &egui::Context,
        request: super::ModelRequest,
        cap: usize,
    ) {
        self.spawn_lane(ctx, request.source(), cap, RequestLane::Model(request));
    }
    pub(super) fn spawn_mrms(&self, ctx: &egui::Context, request: super::MrmsContext, cap: usize) {
        self.spawn_lane(ctx, request.source(), cap, RequestLane::Mrms(request));
    }
    fn spawn_lane(
        &self,
        ctx: &egui::Context,
        source: OverlaySource,
        cap: usize,
        lane: RequestLane,
    ) {
        let generation = self.start(lane.clone());
        let model_request = super::ModelRequest::from_source(&source);
        let mrms_context = match lane {
            RequestLane::Mrms(context) => Some(context),
            _ => None,
        };
        let goes_request = match lane {
            RequestLane::Goes(request) => Some(request),
            _ => None,
        };
        let registration = if let Some(request) = model_request {
            let (handle, registration) = AbortHandle::new_pair();
            if let Some((_, old)) = self
                .model_jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(request, (generation, handle))
            {
                old.abort();
            }
            Some(registration)
        } else if let Some(context) = mrms_context {
            let (handle, registration) = AbortHandle::new_pair();
            if let Some((_, old)) = self
                .mrms_jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(context, (generation, handle))
            {
                old.abort();
            }
            Some(registration)
        } else if let Some(request) = goes_request {
            let (handle, registration) = AbortHandle::new_pair();
            if let Some((_, old)) = self
                .goes_jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(request, (generation, handle))
            {
                old.abort();
            }
            Some(registration)
        } else {
            None
        };
        let http = self.http.clone();
        let tx = self.sender.clone();
        let ctx = ctx.clone();
        self.spawner.spawn(async move {
            let fetch = async {
                match wxdata::task::timeout(OVERLAY_TIMEOUT, source.fetch(&http))
                    .await
                    .unwrap_or_else(Err)
                {
                    // Oversized grids are prepared off-frame, before their delivery to the UI.
                    Ok(msg) => {
                        let msg = if let Some(request) = goes_request {
                            tag_goes_message(request, msg)?
                        } else {
                            msg
                        };
                        Ok(prepare_message(msg, cap))
                    }
                    Err(e) => Err(e.to_string()),
                }
            };
            let result = if let Some(registration) = registration {
                let Ok(result) = Abortable::new(fetch, registration).await else {
                    return;
                };
                result
            } else {
                fetch.await
            };
            let _ = tx.send(OverlayDelivery::Fetched {
                lane,
                generation,
                model_request,
                mrms_context,
                result,
            });
            ctx.request_repaint();
        });
    }
}

fn tag_goes_message(request: super::GoesRequest, msg: OverlayMsg) -> Result<OverlayMsg, String> {
    let tagged = match msg {
        OverlayMsg::Field(layer, field) if request.layer == Some(layer) => {
            OverlayMsg::GoesField(request, request.stamp(field))
        }
        OverlayMsg::GoesFootprint(sector, fp)
            if request.layer.is_none() && sector == request.sector =>
        {
            OverlayMsg::GoesFootprintFor(request, fp)
        }
        _ => return Err("GOES payload does not match its original request".into()),
    };
    if !request.accepts_message(&tagged) {
        return Err("GOES scan is outside the selected analysis tolerance".into());
    }
    Ok(tagged)
}

fn prepare_message(msg: OverlayMsg, cap: usize) -> OverlayMsg {
    match msg {
        OverlayMsg::GoesField(request, field) => {
            let kind = request
                .layer
                .and_then(|layer| layer.descriptor())
                .map_or(wxdata::field::ValueKind::Scalar, |d| d.value_kind);
            OverlayMsg::GoesField(request, field.for_display(cap, kind))
        }
        OverlayMsg::Field(layer, f) => OverlayMsg::Field(layer, f.decimated(cap)),
        OverlayMsg::StampedField(layer, f) => {
            let kind = layer
                .descriptor()
                .map_or(wxdata::field::ValueKind::Scalar, |d| d.value_kind);
            OverlayMsg::StampedField(layer, f.for_display(cap, kind))
        }
        OverlayMsg::VectorField(speed, uv) => {
            let speed = speed.for_display(cap, wxdata::field::ValueKind::Scalar);
            let (u, v) = *uv;
            let uv = (
                super::model_field::resample_like(u, &speed.data),
                super::model_field::resample_like(v, &speed.data),
            );
            OverlayMsg::VectorField(speed, Box::new(uv))
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod cancellation_tests {
    use super::super::{CacheState, ModelRequest};
    use super::*;
    use crate::render::FieldLayer;
    use std::collections::HashSet;

    #[tokio::test]
    async fn mrms_jobs_cancel_only_after_the_last_owner_leaves_and_preserve_other_lanes() {
        let (sender, _receiver) = std::sync::mpsc::channel();
        let acquisition = OverlayAcquisition::new(
            reqwest::Client::new(),
            Spawner::new(tokio::runtime::Handle::current()),
            sender,
        );
        let a = super::super::MrmsContext::resolve(FieldLayer::Mesh, 30, 5, 60, None, 2).unwrap();
        let b = super::super::MrmsContext {
            archive: Some((Utc::now(), 2)),
            ..a
        };
        let lane = RequestLane::Mrms(a);
        let first = acquisition.start(lane.clone());
        assert!(acquisition.finish(&lane, first, None, None));
        let generation = acquisition.start(lane.clone());
        let (handle, registration) = AbortHandle::new_pair();
        acquisition
            .mrms_jobs
            .lock()
            .unwrap()
            .insert(a, (generation, handle));
        let other = acquisition.start(RequestLane::Mrms(b));
        assert!(acquisition
            .cancel_unwanted_mrms(&HashSet::from([a, b]))
            .is_empty());
        assert!(acquisition.health(&lane).fetching);
        assert_eq!(acquisition.cancel_unwanted_mrms(&HashSet::from([b])), [a]);
        assert!(Abortable::new(std::future::pending::<()>(), registration)
            .await
            .is_err());
        assert!(!acquisition.finish(&lane, generation, Some("obsolete failure"), None));
        let health = acquisition.health(&lane);
        assert!(!health.fetching);
        assert!(health.error.is_none());
        assert!(health.last_failure.is_none());
        assert_eq!(health.recent_outcomes, Some((1, 0)));
        assert!(acquisition.health(&RequestLane::Mrms(b)).fetching);
        assert!(acquisition.finish(&RequestLane::Mrms(b), other, Some("archive missing"), None));
        assert!(acquisition.health(&lane).error.is_none());
        let retry = acquisition.start(lane.clone());
        assert!(acquisition.finish(&lane, retry, None, None));
        assert!(acquisition.mrms_jobs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn model_jobs_cancel_only_after_the_last_subscriber_leaves_without_health_credit() {
        let (sender, _receiver) = std::sync::mpsc::channel();
        let acquisition = OverlayAcquisition::new(
            reqwest::Client::new(),
            Spawner::new(tokio::runtime::Handle::current()),
            sender,
        );
        let request = ModelRequest::Reflectivity(wxdata::hrrr::Model::Hrrr, 1, None);
        let lane = RequestLane::Model(request);
        let success = acquisition.start(lane.clone());
        assert!(acquisition.finish(&lane, success, None, None));
        let generation = acquisition.start(lane.clone());
        let (handle, registration) = AbortHandle::new_pair();
        acquisition
            .model_jobs
            .lock()
            .unwrap()
            .insert(request, (generation, handle));
        // One or several identical pane subscribers use the same wanted request.
        assert!(acquisition
            .cancel_unwanted_models(&HashSet::from([request]))
            .is_empty());
        assert!(acquisition.health(&lane).fetching);
        assert_eq!(
            acquisition.cancel_unwanted_models(&HashSet::new()),
            [request]
        );
        assert!(Abortable::new(std::future::pending::<()>(), registration)
            .await
            .is_err());
        let health = acquisition.health(&lane);
        assert!(!health.fetching);
        assert_eq!(health.cache_state, CacheState::Memory);
        assert!(health.last_failure.is_none());
        assert!(health.error.is_none());
        assert!(!acquisition.finish(&lane, generation, None, None));
        let retry = acquisition.start(lane.clone());
        assert!(acquisition.finish(&lane, retry, None, None));
        assert!(acquisition.model_jobs.lock().unwrap().is_empty());
        // Another pane's independent context has its own generation and history.
        let other = RequestLane::Model(ModelRequest::Environment(
            FieldLayer::Cape,
            wxdata::hrrr::Model::Rap,
            false,
            3,
            1,
            None,
        ));
        let generation = acquisition.start(other.clone());
        assert!(acquisition.finish(&other, generation, Some("offline"), None));
        assert!(acquisition.health(&lane).error.is_none());
    }
}
