//! Pane-owned MRMS requests. The selected product/time also owns delivery, health and textures.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct MrmsContext {
    pub layer: crate::render::FieldLayer,
    pub product: &'static str,
    pub archive: Option<(DateTime<Utc>, u16)>,
    /// Interpolated between the archived frames either side (1008.md E2); see [`Self::blended`].
    pub blend: bool,
}

impl MrmsContext {
    pub(super) fn resolve(
        layer: crate::render::FieldLayer,
        rotation: u16,
        lightning: u16,
        hail: u16,
        target: Option<DateTime<Utc>>,
        tolerance: u16,
    ) -> Option<Self> {
        Some(Self {
            layer,
            product: wxdata::mrms::catalog::find(layer.slug())?.path(rotation, lightning, hail),
            archive: target.map(|time| (time, tolerance)),
            blend: false,
        })
    }
    /// Blend between frames when `on`, the request names an archive time, and the layer's values
    /// may be interpolated (scalar or probability; never categories or accumulations).
    pub(super) fn blended(self, on: bool) -> Self {
        let kind = self
            .layer
            .descriptor()
            .map_or(wxdata::field::ValueKind::Scalar, |d| d.value_kind);
        Self {
            blend: on
                && self.archive.is_some()
                && matches!(
                    kind,
                    wxdata::field::ValueKind::Scalar | wxdata::field::ValueKind::Probability
                ),
            ..self
        }
    }
    pub(super) fn request(self) -> MrmsRequest {
        MrmsRequest {
            product: self.product.into(),
            archive: self.archive,
            blend: self.blend,
        }
    }
    pub(super) fn source(self) -> OverlaySource {
        OverlaySource::Field(self.layer, self.request())
    }
    pub(super) fn description(self) -> String {
        format!(
            "MRMS / {}; {}",
            self.product,
            self.archive
                .map(|(time, tolerance)| format!(
                    "analysis {} UTC ±{tolerance} min{}",
                    time.format("%Y-%m-%d %H:%M:%S"),
                    if self.blend {
                        ", interpolated between frames"
                    } else {
                        ""
                    }
                ))
                .unwrap_or_else(|| "latest analysis requested".into())
        )
    }
    pub(super) fn accepts_message(self, msg: &OverlayMsg) -> bool {
        matches!(msg, OverlayMsg::MrmsField(layer, field, request)
            if *layer == self.layer && *request == self.request()
                && self.accepts_field(field))
    }
    pub(super) fn accepts_field(
        self,
        field: &wxdata::field::Stamped<wxdata::mrms::MrmsField>,
    ) -> bool {
        field.data.time == field.stamp.valid_time && self.request().accepts(&field.stamp)
    }
}

pub(super) fn is_mrms(layer: crate::render::FieldLayer) -> bool {
    wxdata::mrms::catalog::find(layer.slug()).is_some()
}

impl HookEchoApp {
    pub(super) fn selected_mrms_context_for(
        &self,
        idx: usize,
        layer: crate::render::FieldLayer,
    ) -> Option<MrmsContext> {
        let timeline = &self.views.get(idx)?.timeline;
        let target = self.model_target_time(idx);
        if target.is_none() && !timeline.following && timeline.forecast_hour().is_none() {
            return None;
        }
        MrmsContext::resolve(
            layer,
            self.rotation_minutes,
            self.settings.lightning_minutes,
            self.hail_minutes,
            target,
            self.settings.time_mismatch_minutes,
        )
        .map(|c| c.blended(self.settings.blend_frames))
    }
    pub(super) fn mrms_context_wanted_by(&self, idx: usize, request: MrmsContext) -> bool {
        self.views.get(idx).is_some_and(|view| {
            (view.fields_on.contains(&request.layer)
                || request.layer == crate::render::FieldLayer::PrecipType
                    && self.settings.precip_tint
                    && view.show_radar)
                && self.selected_mrms_context_for(idx, request.layer) == Some(request)
        })
    }
    pub(super) fn wanted_mrms_contexts(&self) -> std::collections::HashSet<MrmsContext> {
        self.views
            .iter()
            .enumerate()
            .flat_map(|(idx, view)| {
                view.fields_on
                    .iter()
                    .copied()
                    .chain(
                        (self.settings.precip_tint && view.show_radar)
                            .then_some(crate::render::FieldLayer::PrecipType),
                    )
                    .filter_map(move |layer| self.selected_mrms_context_for(idx, layer))
            })
            .collect()
    }
    pub(super) fn mrms_context_ready_for(
        &self,
        idx: usize,
        layer: crate::render::FieldLayer,
    ) -> bool {
        !is_mrms(layer)
            || self
                .selected_mrms_context_for(idx, layer)
                .is_some_and(|request| {
                    self.mrms_fields
                        .get(&request)
                        .is_some_and(|slot| slot.ready(request))
                })
    }
    pub(super) fn schedule_mrms_fields(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let wanted = self.wanted_mrms_contexts();
        let cap = self.field_texture_cap();
        for request in self.acquisition.cancel_unwanted_mrms(&wanted) {
            if let Some(slot) = self.mrms_fields.get_mut(&request) {
                slot.state.last_fetch = None;
            }
        }
        self.mrms_fields.reconcile(&wanted, now);
        for context in wanted {
            let slot = self.mrms_fields.ensure(context);
            // Accepted archive fields are immutable. Missing/failed archives still retry at a
            // bounded cadence; following latest keeps refreshing without discarding usable data.
            if slot.due(
                context,
                std::time::Duration::from_secs(field_refresh_secs(context.layer)),
                now,
            ) {
                slot.state.begin_mrms(context.request(), now);
                self.acquisition.spawn_mrms(ctx, context, cap);
            }
        }
        for (context, texture) in self.mrms_fields.take_dropped() {
            self.acquisition.reset(&RequestLane::Mrms(context));
            self.mrms_drop_textures.push(texture);
        }
    }
    pub(super) fn accept_mrms_field(
        &mut self,
        context: MrmsContext,
        field: wxdata::field::Stamped<wxdata::mrms::MrmsField>,
    ) {
        if !context.accepts_field(&field) || !self.wanted_mrms_contexts().contains(&context) {
            return;
        }
        if context.layer == crate::render::FieldLayer::Lightning && context.archive.is_none() {
            self.check_lightning_proximity(&field.data);
        }
        let upload = self.field_upload(context.layer, &field.data);
        if let Some(slot) = self.mrms_fields.get_mut(&context) {
            slot.stage(context, field, upload, Instant::now());
        }
    }
    pub(super) fn precip_for_pane(
        &self,
        idx: usize,
    ) -> Option<(
        (crate::render::MrmsTextureKey, u64),
        std::sync::Arc<PrecipGrid>,
    )> {
        if !self.settings.precip_tint {
            return None;
        }
        let context = self.selected_mrms_context_for(idx, crate::render::FieldLayer::PrecipType)?;
        let slot = self.mrms_fields.get(&context)?;
        if !slot.ready(context) {
            return None;
        }
        Some(((slot.texture, slot.generation), slot.precip.clone()?))
    }
    pub(super) fn mrms_context_health(&self, context: MrmsContext) -> SourceHealth {
        let state = self.mrms_fields.get(&context).map(|slot| &slot.state);
        mrms_health(
            context,
            state,
            self.acquisition.health(&RequestLane::Mrms(context)),
        )
    }
}

fn mrms_health(
    context: MrmsContext,
    state: Option<&FieldState>,
    mut health: SourceHealth,
) -> SourceHealth {
    let ready =
        state.is_some_and(|state| state.mrms_ready(&context.request()) && state.grid.is_some());
    health.selection_only = context.archive.is_some() && ready;
    health.latest_valid_time = ready
        .then(|| state?.stamp.as_ref().map(|stamp| stamp.valid_time))
        .flatten();
    health.cache_state = if ready {
        CacheState::Memory
    } else {
        CacheState::Empty
    };
    health
        .details
        .push(("Requested analysis", context.description()));
    if let Some(stamp) = ready.then(|| state?.stamp.as_ref()).flatten() {
        health.details.push((
            "Loaded analysis",
            format!(
                "{} / {}; valid {} UTC",
                stamp.source_id,
                stamp.product_id,
                stamp.valid_time.format("%Y-%m-%d %H:%M:%S")
            ),
        ));
    }
    health
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::mrms_cache::MrmsFieldCache;
    use super::*;
    use crate::render::{FieldLayer as L, MrmsUpload};
    use std::{collections::HashSet, time::Duration};

    fn time() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }
    fn request(layer: L, seconds: i64) -> MrmsContext {
        MrmsContext::resolve(
            layer,
            30,
            5,
            60,
            Some(time() + chrono::Duration::seconds(seconds)),
            2,
        )
        .unwrap()
    }
    fn field(
        request: MrmsContext,
        valid: DateTime<Utc>,
        value: f32,
    ) -> wxdata::field::Stamped<wxdata::mrms::MrmsField> {
        let grid = wxdata::mrms::MrmsField {
            values: vec![value; 4],
            nx: 2,
            ny: 2,
            lon_west: -100.0,
            lon_east: -94.0,
            lat_north: 40.0,
            lat_south: 30.0,
            time: valid,
        };
        let mut stamp =
            field_state::model_stamp("MRMS fixture", request.product, &grid, None, false);
        stamp.is_forecast = false;
        wxdata::field::Stamped { data: grid, stamp }
    }
    fn upload() -> MrmsUpload {
        MrmsUpload {
            data: vec![2; 4],
            nx: 2,
            ny: 2,
            world_min: [0.0; 2],
            world_max: [1.0; 2],
            uniform: [0.0; 12],
            lut: vec![0; 1024],
        }
    }

    #[test]
    fn only_archived_continuous_layers_are_blended_and_the_request_says_so() {
        let archived = |layer| request(layer, 0);
        let mesh = archived(L::Mesh).blended(true);
        assert!(mesh.blend, "a scalar archived layer blends");
        assert!(mesh.request().blend);
        assert!(mesh.description().contains("interpolated between frames"));
        assert_ne!(mesh, archived(L::Mesh), "a blended frame is its own slot");
        assert!(!archived(L::Mesh).blended(false).blend, "off by default");
        for never in [L::Qpe1h, L::PrecipType] {
            assert!(
                !archived(never).blended(true).blend,
                "{never:?}: accumulations and categories are never interpolated"
            );
        }
        let live = MrmsContext::resolve(L::Mesh, 30, 5, 60, None, 2).unwrap();
        assert!(
            !live.blended(true).blend,
            "the latest frame is read as it is"
        );
        // A blended reply is not accepted by the nearest-frame request, nor the other way.
        let t = time() + chrono::Duration::seconds(30);
        let reply = field(mesh, t, 30.0);
        assert!(mesh.accepts_field(&reply));
        let msg = OverlayMsg::MrmsField(L::Mesh, reply, mesh.request());
        assert!(mesh.accepts_message(&msg));
        assert!(!archived(L::Mesh).accepts_message(&msg));
    }

    #[test]
    fn mrms_context_catalog_resolves_every_product_and_retains_full_analysis_identity() {
        for product in wxdata::mrms::catalog::PRODUCTS {
            let layer = L::from_slug(product.field.id.0).unwrap();
            let context = request(layer, 0);
            assert_eq!(context.product, product.path(30, 5, 60));
            assert!(context.description().contains("22:13:20"));
            assert!(context.description().contains("±2 min"));
            assert_ne!(context, request(layer, 1));
            let wider = MrmsContext {
                archive: Some((time(), 3)),
                ..context
            };
            assert_ne!(context, wider);
            assert_ne!(
                context,
                MrmsContext {
                    archive: None,
                    ..context
                }
            );
        }
        for layer in [L::Rotation, L::Lightning, L::HailSwath] {
            let a = MrmsContext::resolve(layer, 30, 5, 60, None, 2).unwrap();
            let b = MrmsContext::resolve(layer, 60, 15, 120, None, 2).unwrap();
            assert_ne!(a.product, b.product);
        }
        assert!(MrmsContext::resolve(L::Hrrr, 30, 5, 60, None, 2).is_none());
        assert!(MrmsContext::resolve(L::Mosaic, 30, 5, 60, None, 2).is_none());
    }

    #[test]
    fn mrms_context_delivery_rejects_wrong_layer_product_request_clock_and_tolerance() {
        let context = request(L::Mesh, 0);
        let good = field(context, time() + chrono::Duration::minutes(2), 25.4);
        assert!(context.accepts_message(&OverlayMsg::MrmsField(
            L::Mesh,
            good.clone(),
            context.request()
        )));
        assert!(!context.accepts_message(&OverlayMsg::MrmsField(
            L::Qpe1h,
            good.clone(),
            context.request()
        )));
        assert!(!context.accepts_message(&OverlayMsg::MrmsField(
            L::Mesh,
            good.clone(),
            request(L::Mesh, 1).request()
        )));
        let mut wrong = good.clone();
        wrong.stamp.product_id = "wrong product".into();
        assert!(!context.accepts_field(&wrong));
        wrong = good.clone();
        wrong.data.time += chrono::Duration::seconds(1);
        assert!(!context.accepts_field(&wrong));
        for seconds in [-121, 121] {
            assert!(!context.accepts_field(&field(
                context,
                time() + chrono::Duration::seconds(seconds),
                30.0
            )));
        }
        let outside = field(context, time() + chrono::Duration::hours(2), 30.0);
        assert!(MrmsContext {
            archive: None,
            ..context
        }
        .accepts_field(&outside));
    }

    #[test]
    fn mrms_context_independent_clocks_and_pending_archive_seeks_never_become_latest() {
        let mut a = crate::timeline::Timeline::default();
        let mut b = crate::timeline::Timeline::default();
        a.following = false;
        a.seek_target = Some(time());
        b.following = false;
        b.seek_target = Some(time() + chrono::Duration::hours(2));
        let resolve = |timeline: &crate::timeline::Timeline, linked| {
            MrmsContext::resolve(
                L::Mesh,
                30,
                5,
                60,
                model_context::analysis_target_time(timeline, linked),
                2,
            )
            .unwrap()
        };
        assert_eq!(resolve(&a, None), request(L::Mesh, 0));
        assert_ne!(resolve(&a, None), resolve(&b, None));
        assert_eq!(resolve(&a, Some(time())), resolve(&b, Some(time())));
        assert_eq!(
            model_context::analysis_target_time(&crate::timeline::Timeline::default(), None),
            None
        );
        a.frames = vec![wxdata::level2::Identifier::new(
            "KTLX20231114_221320_V06".into(),
        )];
        a.seek_target = None;
        assert_eq!(resolve(&a, None), request(L::Mesh, 0));
    }

    #[test]
    fn mrms_cache_protects_all_visible_contexts_and_expires_hidden_slots_without_reusing_ids() {
        let mut cache = MrmsFieldCache::new(2);
        let now = Instant::now();
        cache.ensure(request(L::Mesh, -1));
        cache.ensure(request(L::Mesh, -2));
        let visible: HashSet<_> = (0..12).map(|s| request(L::Mesh, s)).collect();
        cache.reconcile(&visible, now);
        for context in &visible {
            cache.ensure(*context);
        }
        assert!(visible.iter().all(|context| cache.get(context).is_some()));
        let remaining = HashSet::from([request(L::Mesh, 0), request(L::Mesh, 11)]);
        cache.reconcile(&remaining, now);
        assert_eq!(cache.iter().count(), 2);
        assert!(remaining.iter().all(|context| cache.get(context).is_some()));
        let old = cache.get(&request(L::Mesh, 0)).unwrap().texture;
        cache.reconcile(&HashSet::new(), now);
        cache.reconcile(&remaining, now + Duration::from_secs(100));
        assert_eq!(cache.get(&request(L::Mesh, 0)).unwrap().texture, old);
        cache.reconcile(&HashSet::new(), now + Duration::from_secs(100));
        cache.reconcile(&HashSet::new(), now + Duration::from_secs(159));
        assert!(cache.get(&request(L::Mesh, 0)).is_some());
        cache.reconcile(&HashSet::new(), now + Duration::from_secs(160));
        assert!(cache.get(&request(L::Mesh, 0)).is_none());
        assert_ne!(cache.ensure(request(L::Mesh, 0)).texture, old);
        assert!(cache.take_dropped().contains(&(request(L::Mesh, 0), old)));
    }

    #[test]
    fn mrms_slots_share_exact_requests_and_keep_probes_precip_tint_and_archive_cadence_separate() {
        let now = Instant::now();
        let cadence = Duration::from_secs(120);
        let mut cache = MrmsFieldCache::new(4);
        let a = request(L::PrecipType, 0);
        let b = request(L::PrecipType, 7200);
        let live = MrmsContext { archive: None, ..a };
        let key_a = cache.ensure(a).texture;
        assert_eq!(cache.ensure(a).texture, key_a);
        assert_ne!(cache.ensure(b).texture, key_a);
        let slot = cache.ensure(a);
        assert!(slot.due(a, cadence, now));
        slot.state.begin_mrms(a.request(), now);
        assert!(!slot.due(a, cadence, now));
        assert!(slot.due(a, cadence, now + cadence)); // Missing archive retries.
        let original = field(a, time(), 3.0);
        let stamp = serde_json::to_value(&original.stamp).unwrap();
        assert!(slot.stage(a, original, upload(), now));
        assert!(!slot.due(a, cadence, now + cadence * 10));
        assert_eq!(
            serde_json::to_value(slot.state.stamp.as_ref().unwrap()).unwrap(),
            stamp
        );
        assert_eq!(slot.precip.as_ref().unwrap().classes, [1; 4]);
        let generation = slot.generation;
        assert!(!slot.stage(
            a,
            field(b, time() + chrono::Duration::hours(2), 6.0),
            upload(),
            now
        ));
        assert_eq!(slot.generation, generation);
        assert!(cache.ensure(b).stage(
            b,
            field(b, time() + chrono::Duration::hours(2), 6.0),
            upload(),
            now
        ));
        assert_eq!(
            cache.get(&b).unwrap().precip.as_ref().unwrap().classes,
            [2; 4]
        );
        assert_eq!(
            cache
                .get(&a)
                .unwrap()
                .state
                .grid
                .as_ref()
                .unwrap()
                .sample_bilinear(-97.0, 35.0),
            Some(3.0)
        );
        assert_eq!(
            cache
                .get(&b)
                .unwrap()
                .state
                .grid
                .as_ref()
                .unwrap()
                .sample_bilinear(-97.0, 35.0),
            Some(6.0)
        );
        assert_eq!(cache.ensure(a).texture, key_a); // Cached context switch retains its own generation.
        let slot = cache.ensure(live);
        assert!(slot.stage(live, field(live, time(), 1.0), upload(), now));
        assert!(slot.due(live, cadence, now + cadence));
        assert!(slot.stage(
            live,
            field(live, time() + chrono::Duration::minutes(2), 3.0),
            upload(),
            now
        ));
        assert_eq!(slot.generation, 2);
        assert_ne!((slot.texture, slot.generation), (key_a, generation));
    }

    #[test]
    fn mrms_health_late_replies_and_failures_cannot_credit_another_analysis() {
        let a = request(L::Mesh, 0);
        let b = request(L::Mesh, 7200);
        let lane_a = RequestLane::Mrms(a);
        let lane_b = RequestLane::Mrms(b);
        let mut book = RequestBook::default();
        let mut cache = MrmsFieldCache::new(2);
        let old = book.start(lane_a.clone());
        book.discard(&lane_a, old);
        let current = book.start(lane_b.clone());
        assert!(!book.finish(&lane_a, old, None, Some(time())));
        assert!(!book.finish(&lane_a, old, Some("obsolete failure"), None));
        assert!(book.health(&lane_b).fetching);
        assert!(book.finish(&lane_b, current, Some("archive unavailable"), None));
        let failed = mrms_health(b, Some(&cache.ensure(b).state), book.health(&lane_b));
        assert_eq!(failed.state(), HealthState::Failed);
        assert_eq!(failed.cache_state, CacheState::Empty);
        assert!(failed.latest_valid_time.is_none());
        assert!(failed
            .details
            .iter()
            .all(|(key, _)| *key != "Loaded analysis"));
        let valid = time() + chrono::Duration::hours(2) + chrono::Duration::seconds(30);
        let current = book.start(lane_b.clone());
        assert!(cache
            .ensure(b)
            .stage(b, field(b, valid, 25.4), upload(), Instant::now()));
        assert!(book.finish(&lane_b, current, None, Some(valid)));
        let health = mrms_health(b, Some(&cache.get(&b).unwrap().state), book.health(&lane_b));
        assert_eq!(health.latest_valid_time, Some(valid));
        assert_eq!(health.cache_state, CacheState::Memory);
        assert!(health.selection_only);
        let mut retained = health.clone();
        retained.last_success = retained
            .last_success
            .map(|age| age + Duration::from_secs(7200));
        retained.last_attempt = retained
            .last_attempt
            .map(|age| age + Duration::from_secs(7200));
        retained.last_failure = retained
            .last_failure
            .map(|age| age + Duration::from_secs(7200));
        assert_eq!(retained.state(), HealthState::Fresh);
        assert!(retained.next_retry().is_none());
        assert_eq!(
            crate::ui::source_health_window::retry_line(&retained),
            "On selection change"
        );
        assert!(retained.recovery().contains("No periodic refresh"));
        assert!(DiagnosticsSourceHealth::from(&retained).selection_only);
        assert!(!failed.selection_only);
        assert!(failed.recovery().contains("Retried every"));
        assert!(health
            .details
            .iter()
            .any(|(key, value)| *key == "Loaded analysis" && value.contains("00:13:50")));
        let retry = book.start(lane_b.clone());
        book.finish(&lane_b, retry, Some("retry offline"), None);
        assert_eq!(
            mrms_health(b, Some(&cache.get(&b).unwrap().state), book.health(&lane_b)).state(),
            HealthState::Cached
        );
        assert!(mrms_health(a, None, book.health(&lane_a))
            .latest_valid_time
            .is_none());
    }

    /// The capture uses the production request book, display-slot admission and health builder.
    pub(in crate::app) fn review_health(case: &str) -> SourceHealth {
        let context = request(L::Rotation, 0);
        let lane = RequestLane::Mrms(context);
        let mut book = RequestBook::default();
        let mut cache = MrmsFieldCache::new(1);
        let generation = book.start(lane.clone());
        match case {
            "loaded" | "cached" => {
                cache.ensure(context).stage(
                    context,
                    field(context, time() + chrono::Duration::seconds(30), 0.01),
                    upload(),
                    Instant::now(),
                );
                book.finish(
                    &lane,
                    generation,
                    None,
                    Some(time() + chrono::Duration::seconds(30)),
                );
                if case == "cached" {
                    let retry = book.start(lane.clone());
                    book.finish(
                        &lane,
                        retry,
                        Some("MRMS refresh timed out; retaining this analysis only."),
                        None,
                    );
                }
            }
            "failed" => {
                book.finish(
                    &lane,
                    generation,
                    Some("No analysis within the selected time tolerance."),
                    None,
                );
            }
            "fetching" => {}
            _ => panic!("unknown MRMS review case"),
        }
        let mut health = mrms_health(
            context,
            cache.get(&context).map(|slot| &slot.state),
            book.health(&lane),
        );
        health.source = format!("Pane 1, 3 · {}", context.description());
        health.details.push(("Pane ownership", "Panes 1, 3; identical requests share this download and field. Includes precipitation-tint owners when enabled.".into()));
        health
    }
}
