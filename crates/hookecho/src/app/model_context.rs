//! Request-owned model identity. Displayed metadata is never reconstructed from a later picker.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModelRequest {
    Reflectivity(wxdata::hrrr::Model, u8, Option<DateTime<Utc>>),
    Subhourly(u16, Option<DateTime<Utc>>),
    Environment(
        crate::render::FieldLayer,
        wxdata::hrrr::Model,
        bool,
        u8,
        u8,
        Option<DateTime<Utc>>,
    ),
    RegionalProduct(crate::render::FieldLayer, u8, Option<DateTime<Utc>>),
    Global(
        crate::render::FieldLayer,
        wxdata::global::GlobalModel,
        wxdata::global::GlobalField,
        u16,
        Option<DateTime<Utc>>,
    ),
    Analysis(crate::render::FieldLayer, Option<DateTime<Utc>>),
}

pub(super) const MODEL_LAYERS: [crate::render::FieldLayer; 21] = {
    use crate::render::FieldLayer as L;
    [
        L::Hrrr,
        L::Cape,
        L::Srh,
        L::UpdraftHelicity,
        L::Smoke,
        L::Snowfall,
        L::ThunderProb,
        L::GlobalMslp,
        L::GlobalHeight500,
        L::GlobalTemp2m,
        L::GlobalDewpoint2m,
        L::GlobalWind10m,
        L::GlobalPrecip,
        L::RtmaTemp2m,
        L::RtmaDewpoint2m,
        L::RtmaWind10m,
        L::RtmaGust10m,
        L::RtmaVisibility,
        L::RtmaCeiling,
        L::RtmaMslp,
        L::RtmaPrecip1h,
    ]
};

impl ModelRequest {
    fn description(self) -> String {
        use crate::render::FieldLayer as L;
        let (source, product, minutes, run) = match self {
            Self::Reflectivity(model, hour, run) => (
                model.label(),
                "Composite reflectivity".into(),
                i64::from(hour) * 60,
                run,
            ),
            Self::Subhourly(minutes, run) => (
                crate::model_browser::BModel::Hrrr15.label(),
                "Composite reflectivity".into(),
                i64::from(minutes),
                run,
            ),
            Self::Environment(layer, model, mixed, depth, hour, run) => (
                model.label(),
                match layer {
                    L::Cape if mixed => "Mixed-layer CAPE".into(),
                    L::Cape => "Surface CAPE".into(),
                    _ => format!("0–{depth} km SRH"),
                },
                i64::from(hour) * 60,
                run,
            ),
            Self::RegionalProduct(layer, hour, run) => (
                if layer == L::ThunderProb {
                    "NBM"
                } else {
                    "HRRR"
                },
                match layer {
                    L::UpdraftHelicity => "Updraft helicity swath",
                    L::Smoke => "Near-surface smoke",
                    L::Snowfall => "Accumulated snowfall",
                    L::ThunderProb => "Thunder probability",
                    _ => "Regional product",
                }
                .into(),
                i64::from(if matches!(layer, L::UpdraftHelicity | L::ThunderProb) {
                    hour.max(1)
                } else {
                    hour
                }) * 60,
                run,
            ),
            Self::Global(_, model, field, hour, run) => (
                model.label(),
                field.label().into(),
                i64::from(hour) * 60,
                run,
            ),
            Self::Analysis(layer, hour) => {
                return format!(
                    "RTMA/URMA / {}; {}",
                    rtma_field(layer).map_or("Analysis", |field| field.label()),
                    hour.map(|time| format!("analysis {} UTC", time.format("%Y-%m-%d %H:%M")))
                        .unwrap_or_else(|| "latest analysis requested".into())
                )
            }
        };
        format!(
            "{source} / {product}; lead {minutes} min; {}",
            run.map(|time| format!("run {} UTC", time.format("%Y-%m-%d %H:%M")))
                .unwrap_or_else(|| "latest available run requested".into())
        )
    }
    pub(super) fn current(self, selected: Option<Self>, requested: Option<Self>) -> bool {
        selected == Some(self) && requested == Some(self)
    }
    pub(super) fn from_source(source: &OverlaySource) -> Option<Self> {
        Some(match *source {
            OverlaySource::Hrrr(model, lead, run) => Self::Reflectivity(model, lead, run),
            OverlaySource::HrrrSub(lead, run) => Self::Subhourly(lead, run),
            OverlaySource::Env(layer, model, ml, depth, lead, run) => {
                Self::Environment(layer, model, ml, depth, lead, run)
            }
            OverlaySource::HrrrLayer(layer, lead, run) => Self::RegionalProduct(layer, lead, run),
            OverlaySource::Global(layer, model, field, lead, run) => {
                Self::Global(layer, model, field, lead, run)
            }
            OverlaySource::Rtma(layer, hour) => Self::Analysis(layer, hour),
            _ => return None,
        })
    }

    pub(super) fn accepts_message(self, msg: &OverlayMsg) -> bool {
        matches!(msg, OverlayMsg::StampedField(layer, field)
            if *layer == self.layer() && self.accepts(&field.stamp) && field.data.time == field.stamp.valid_time)
    }

    pub(super) fn source(self) -> OverlaySource {
        match self {
            Self::Reflectivity(model, lead, run) => OverlaySource::Hrrr(model, lead, run),
            Self::Subhourly(lead, run) => OverlaySource::HrrrSub(lead, run),
            Self::Environment(layer, model, ml, depth, lead, run) => {
                OverlaySource::Env(layer, model, ml, depth, lead, run)
            }
            Self::RegionalProduct(layer, lead, run) => OverlaySource::HrrrLayer(layer, lead, run),
            Self::Global(layer, model, field, lead, run) => {
                OverlaySource::Global(layer, model, field, lead, run)
            }
            Self::Analysis(layer, hour) => OverlaySource::Rtma(layer, hour),
        }
    }

    pub(super) fn layer(self) -> crate::render::FieldLayer {
        match self {
            Self::Reflectivity(..) | Self::Subhourly(..) => crate::render::FieldLayer::Hrrr,
            Self::Environment(layer, ..)
            | Self::RegionalProduct(layer, ..)
            | Self::Global(layer, ..)
            | Self::Analysis(layer, ..) => layer,
        }
    }

    pub(super) fn accepts(self, stamp: &wxdata::field::DataStamp) -> bool {
        use crate::render::FieldLayer as L;
        use wxdata::model::ModelField as F;
        let (source, product, lead, requested_run) = match self {
            Self::Reflectivity(model, hour, run) => (
                model.label(),
                "Composite reflectivity".into(),
                i64::from(hour) * 60,
                run,
            ),
            Self::Subhourly(minutes, run) => (
                crate::model_browser::BModel::Hrrr15.label(),
                "Composite reflectivity".into(),
                i64::from(minutes),
                run,
            ),
            Self::Environment(layer, model, mixed, depth, hour, run) => {
                let field = match layer {
                    L::Cape if mixed => F::MixedLayerCape,
                    L::Cape => F::SurfaceCape,
                    L::Srh if depth == 1 => F::Srh1km,
                    L::Srh => F::Srh3km,
                    _ => return false,
                };
                let Some(key) = field.grib(model) else {
                    return false;
                };
                (
                    model.label(),
                    format!("{}:{}", key.var, key.level),
                    i64::from(hour) * 60,
                    run,
                )
            }
            Self::RegionalProduct(layer, hour, run) => {
                let (source, hour) = match layer {
                    L::ThunderProb => ("NBM", hour.max(1)),
                    L::UpdraftHelicity => ("HRRR", hour.max(1)),
                    L::Smoke | L::Snowfall => ("HRRR", hour),
                    _ => return false,
                };
                (source, layer.slug().into(), i64::from(hour) * 60, run)
            }
            Self::Global(_, model, field, hour, run) => (
                model.label(),
                field.slug().into(),
                i64::from(hour) * 60,
                run,
            ),
            Self::Analysis(layer, hour) => {
                let Some(field) = rtma_field(layer) else {
                    return false;
                };
                if ![
                    wxdata::rtma::AnalysisKind::Rtma.label(),
                    wxdata::rtma::AnalysisKind::Urma.label(),
                ]
                .contains(&stamp.source_id.as_str())
                    || stamp.product_id != field.slug()
                {
                    return false;
                }
                return stamp.run_time.is_some_and(|actual| {
                    hour.is_none_or(|selected| selected == actual) && stamp.valid_time == actual
                });
            }
        };
        stamp.source_id == source
            && stamp.product_id == product
            && stamp.run_time.is_some_and(|run| {
                requested_run.is_none_or(|selected| selected == run)
                    && run
                        .checked_add_signed(chrono::Duration::minutes(lead))
                        .is_some_and(|valid| stamp.valid_time == valid)
            })
    }
}

/// Health describes the selected request; retained previous data is explicitly historical.
pub(super) fn model_health(
    request: ModelRequest,
    state: Option<&FieldState>,
    mut health: SourceHealth,
) -> SourceHealth {
    let description = request.description();
    health.source = description.split(';').next().unwrap_or(&description).into();
    health.details.push(("Selected model request", description));
    let requested = state.is_some_and(|state| state.model_requested == Some(request));
    let ready = state.is_some_and(|state| state.model_ready(request));
    if !requested {
        health.fetching = false;
        health.last_attempt = None;
        health.last_success = None;
        health.last_failure = None;
        health.error = None;
        health.recent_outcomes = None;
    }
    if ready {
        health.latest_valid_time = state
            .and_then(|state| state.stamp.as_ref())
            .map(|stamp| stamp.valid_time);
    } else {
        health.latest_valid_time = None;
        health.cache_state = CacheState::Empty;
        if let Some(stamp) = state.and_then(|state| state.stamp.as_ref()) {
            health.details.push((
                "Previous model field",
                format!(
                    "{} / {}; run {}; valid {} UTC. Unavailable for the selected request.",
                    stamp.source_id,
                    stamp.product_id,
                    stamp
                        .run_time
                        .map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string())
                        .unwrap_or_else(|| "unknown".into()),
                    stamp.valid_time.format("%Y-%m-%d %H:%M")
                ),
            ));
        }
    }
    health
}

pub(super) fn rtma_field(layer: crate::render::FieldLayer) -> Option<wxdata::rtma::RtmaField> {
    use crate::render::FieldLayer as L;
    use wxdata::rtma::RtmaField as F;
    Some(match layer {
        L::RtmaTemp2m => F::Temp2m,
        L::RtmaDewpoint2m => F::Dewpoint2m,
        L::RtmaWind10m => F::Wind10m,
        L::RtmaGust10m => F::Gust10m,
        L::RtmaVisibility => F::Visibility,
        L::RtmaCeiling => F::Ceiling,
        L::RtmaMslp => F::Mslp,
        L::RtmaPrecip1h => F::Precip1h,
        _ => return None,
    })
}

impl HookEchoApp {
    pub(super) fn selected_model_request(
        &self,
        layer: crate::render::FieldLayer,
    ) -> Option<ModelRequest> {
        use crate::render::FieldLayer as L;
        use wxdata::global::GlobalField as G;
        Some(match layer {
            L::Hrrr => {
                let run = self.pinned_regional_run(self.refl_model);
                if self.hrrr_subhourly {
                    ModelRequest::Subhourly(self.hrrr_fcst_min, run)
                } else {
                    ModelRequest::Reflectivity(self.refl_model, self.hrrr_fcst_hour, run)
                }
            }
            L::Cape | L::Srh => ModelRequest::Environment(
                layer,
                self.env_model,
                self.env_cape_ml,
                self.env_srh_km,
                self.hrrr_fcst_hour,
                self.pinned_regional_run(self.env_model),
            ),
            L::UpdraftHelicity | L::Smoke | L::Snowfall | L::ThunderProb => {
                ModelRequest::RegionalProduct(
                    layer,
                    self.hrrr_fcst_hour,
                    self.pinned_regional_run(if layer == L::ThunderProb {
                        wxdata::hrrr::Model::Nbm
                    } else {
                        wxdata::hrrr::Model::Hrrr
                    }),
                )
            }
            L::GlobalMslp
            | L::GlobalHeight500
            | L::GlobalTemp2m
            | L::GlobalDewpoint2m
            | L::GlobalWind10m
            | L::GlobalPrecip => {
                let field = match layer {
                    L::GlobalMslp => G::Mslp,
                    L::GlobalHeight500 => G::Height500,
                    L::GlobalTemp2m => G::Temp2m,
                    L::GlobalDewpoint2m => G::Dewpoint2m,
                    L::GlobalWind10m => G::Wind10m,
                    _ => G::Precip,
                };
                ModelRequest::Global(
                    layer,
                    self.global_model,
                    field,
                    self.global_fcst_hour,
                    self.pinned_global_run(),
                )
            }
            L::RtmaTemp2m
            | L::RtmaDewpoint2m
            | L::RtmaWind10m
            | L::RtmaGust10m
            | L::RtmaVisibility
            | L::RtmaCeiling
            | L::RtmaMslp
            | L::RtmaPrecip1h => ModelRequest::Analysis(
                layer,
                if self.model_sel.model == crate::model_browser::BModel::Rtma {
                    self.model_run
                } else {
                    None
                },
            ),
            _ => return None,
        })
    }

    pub(crate) fn model_field_ready(&self, layer: crate::render::FieldLayer) -> bool {
        self.selected_model_request(layer).is_none_or(|request| {
            self.fields
                .get(&layer)
                .is_some_and(|state| state.model_ready(request))
        })
    }

    pub(super) fn schedule_model_fields(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        for layer in MODEL_LAYERS {
            if !self.field_wanted(layer) {
                continue;
            }
            let Some(request) = self.selected_model_request(layer) else {
                continue;
            };
            let state = self.fields.entry(layer).or_default();
            if state.model_due(
                request,
                std::time::Duration::from_secs(field_refresh_secs(layer)),
                now,
            ) {
                if state.begin_model(request, now) {
                    self.acquisition.reset(&RequestLane::Field(layer));
                }
                self.spawn_overlay(ctx, request.source());
            }
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::render::{FieldLayer as L, MrmsUpload};
    use wxdata::global::{GlobalField as F, GlobalModel as G};
    use wxdata::hrrr::Model as M;

    fn run() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }
    fn field(
        source: &str,
        product: &str,
        cycle: DateTime<Utc>,
        minutes: i64,
        value: f32,
    ) -> wxdata::field::Stamped<wxdata::mrms::MrmsField> {
        let valid = cycle + chrono::Duration::minutes(minutes);
        let grid = wxdata::mrms::MrmsField {
            values: vec![value],
            nx: 1,
            ny: 1,
            lon_west: -100.0,
            lon_east: -100.0,
            lat_north: 35.0,
            lat_south: 35.0,
            time: valid,
        };
        let mut stamped =
            field_state::model_field(source, product, grid, Some(cycle), valid, false).unwrap();
        stamped.stamp.received_time = cycle + chrono::Duration::minutes(10);
        stamped
    }
    fn upload(value: u8) -> MrmsUpload {
        MrmsUpload {
            data: vec![value],
            nx: 1,
            ny: 1,
            world_min: [0.0; 2],
            world_max: [1.0; 2],
            uniform: [0.0; 12],
            lut: vec![0; 1024],
        }
    }
    fn stage(state: &mut FieldState, request: ModelRequest, value: f32) -> bool {
        state.stage_model(
            request,
            field(M::Hrrr.label(), "Composite reflectivity", run(), 60, value),
            upload(value as u8),
        )
    }

    #[test]
    fn model_context_reflectivity_keeps_original_source_and_exact_clocks() {
        let request = ModelRequest::Reflectivity(M::Hrrr, 1, Some(run()));
        let stamped = field(M::Hrrr.label(), "Composite reflectivity", run(), 60, 30.0);
        assert!(request.accepts(&stamped.stamp));
        for other in [
            ModelRequest::Reflectivity(M::Rap, 1, Some(run())),
            ModelRequest::Reflectivity(M::Hrrr, 2, Some(run())),
            ModelRequest::Reflectivity(M::Hrrr, 1, Some(run() - chrono::Duration::hours(1))),
            ModelRequest::Subhourly(60, Some(run())),
        ] {
            assert!(!other.accepts(&stamped.stamp));
            assert!(!request.current(Some(other), Some(request)));
        }
        let mut wrong = stamped.stamp.clone();
        wrong.product_id = "CAPE".into();
        assert!(!request.accepts(&wrong));
        wrong = stamped.stamp.clone();
        wrong.run_time = None;
        assert!(!request.accepts(&wrong));
        wrong = stamped.stamp.clone();
        wrong.valid_time += chrono::Duration::seconds(1);
        assert!(!request.accepts(&wrong));
        let mut msg = OverlayMsg::StampedField(L::Hrrr, stamped);
        assert!(request.accepts_message(&msg));
        if let OverlayMsg::StampedField(_, ref mut field) = msg {
            field.data.time += chrono::Duration::seconds(1);
        }
        assert!(!request.accepts_message(&msg));
    }

    #[test]
    fn model_context_latest_is_request_identity_not_a_rewritten_provider_run() {
        let request = ModelRequest::Reflectivity(M::Hrrr, 1, None);
        let original = field(M::Hrrr.label(), "Composite reflectivity", run(), 60, 30.0);
        assert!(request.accepts(&original.stamp));
        assert!(!request.current(
            Some(ModelRequest::Reflectivity(M::Hrrr, 1, Some(run()))),
            Some(request)
        ));
        let expected = serde_json::to_value(&original.stamp).unwrap();
        let mut state = FieldState::default();
        state.begin_model(request, Instant::now());
        assert!(state.stage_model(request, original, upload(30)));
        assert_eq!(
            serde_json::to_value(state.stamp.as_ref().unwrap()).unwrap(),
            expected
        );
        assert!(matches!(
            state.stamp.as_ref().unwrap().quality,
            wxdata::field::QualitySummary::Unknown
        ));
        assert!(state.stamp.as_ref().unwrap().source_latency.is_none());
    }

    #[test]
    fn model_context_selection_change_rejects_old_delivery_and_switch_back_needs_upload() {
        let a = ModelRequest::Reflectivity(M::Hrrr, 1, Some(run()));
        let b = ModelRequest::Reflectivity(M::Rap, 1, Some(run()));
        let now = Instant::now();
        let mut state = FieldState::default();
        assert!(state.begin_model(a, now));
        assert!(stage(&mut state, a, 30.0));
        assert!(state.model_ready(a));
        let old_stamp = serde_json::to_value(state.stamp.as_ref().unwrap()).unwrap();
        assert!(state.begin_model(b, now));
        assert!(state.pending.is_none());
        assert!(!state.model_ready(a));
        assert!(!state.model_ready(b));
        assert!(!stage(&mut state, a, 80.0));
        assert!(
            !stage(&mut state, b, 90.0),
            "a reply stamped HRRR cannot fulfill RAP"
        );
        assert_eq!(state.grid.as_ref().unwrap().values, [30.0]);
        assert_eq!(
            serde_json::to_value(state.stamp.as_ref().unwrap()).unwrap(),
            old_stamp
        );
        assert!(state.pending.is_none());
        assert!(state.begin_model(a, now));
        assert!(
            !state.model_ready(a),
            "a discarded pending upload must not resurrect as a resident texture"
        );
        assert!(stage(&mut state, a, 40.0));
        assert!(state.model_ready(a));
        assert!(!state.begin_model(a, now));
        assert!(state.model_ready(a));
        assert_eq!(state.pending.as_ref().unwrap().data, [40]);
    }

    #[test]
    fn model_context_variants_bypass_cadence_and_dispatch_roundtrips() {
        let now = Instant::now();
        let cadence = std::time::Duration::from_secs(600);
        let variants = [
            ModelRequest::Environment(L::Cape, M::Hrrr, false, 1, 1, Some(run())),
            ModelRequest::Environment(L::Cape, M::Hrrr, true, 1, 1, Some(run())),
            ModelRequest::Environment(L::Srh, M::Hrrr, false, 1, 1, Some(run())),
            ModelRequest::Environment(L::Srh, M::Hrrr, false, 3, 1, Some(run())),
        ];
        let mut state = FieldState::default();
        for request in variants {
            assert!(state.model_due(request, cadence, now));
            state.begin_model(request, now);
            assert!(!state.model_due(request, cadence, now));
            assert!(state.model_due(request, cadence, now + cadence));
            assert_eq!(ModelRequest::from_source(&request.source()), Some(request));
        }
        for request in [
            ModelRequest::Reflectivity(M::Rap, 12, None),
            ModelRequest::Subhourly(75, None),
            ModelRequest::RegionalProduct(L::ThunderProb, 1, None),
            ModelRequest::Global(L::GlobalMslp, G::Gfs, F::Mslp, 384, None),
            ModelRequest::Analysis(L::RtmaTemp2m, None),
        ] {
            assert_eq!(ModelRequest::from_source(&request.source()), Some(request));
        }
        assert_eq!(
            MODEL_LAYERS
                .into_iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            21
        );
        assert!(ModelRequest::from_source(&OverlaySource::Fronts).is_none());
    }

    #[test]
    fn model_context_hidden_pending_upload_is_consumed_only_by_its_exact_context() {
        let a = ModelRequest::Reflectivity(M::Hrrr, 1, Some(run()));
        let b = ModelRequest::Reflectivity(M::Hrrr, 2, Some(run()));
        let mut state = FieldState::default();
        state.begin_model(a, Instant::now());
        assert!(stage(&mut state, a, 30.0));
        // A different product changes the shared lead while this layer is hidden, so no
        // replacement request for this layer has begun. The original upload must survive.
        assert!(state.take_upload(Some(b)).is_none());
        assert_eq!(state.pending.as_ref().unwrap().data, [30]);
        assert_eq!(state.take_upload(Some(a)).unwrap().data, [30]);
        assert!(state.pending.is_none());
        assert!(state.model_ready(a));
        assert!(stage(&mut state, a, 40.0));
        state.begin_model(b, Instant::now());
        assert!(state.take_upload(Some(a)).is_none());
        assert!(!state.model_ready(a));
    }

    #[test]
    fn model_context_environment_products_global_leads_and_analysis_fallback_are_exact() {
        let surface = ModelRequest::Environment(L::Cape, M::Hrrr, false, 1, 1, Some(run()));
        let mixed = ModelRequest::Environment(L::Cape, M::Hrrr, true, 1, 1, Some(run()));
        let key = wxdata::model::ModelField::SurfaceCape
            .grib(M::Hrrr)
            .unwrap();
        let stamped = field(
            M::Hrrr.label(),
            &format!("{}:{}", key.var, key.level),
            run(),
            60,
            1000.0,
        );
        assert!(surface.accepts(&stamped.stamp));
        assert!(!mixed.accepts(&stamped.stamp));
        let global = ModelRequest::Global(L::GlobalMslp, G::Gfs, F::Mslp, 384, Some(run()));
        assert!(
            global.accepts(&field(G::Gfs.label(), F::Mslp.slug(), run(), 384 * 60, 1010.0).stamp)
        );
        let extreme = ModelRequest::Global(L::GlobalMslp, G::Gfs, F::Mslp, u16::MAX, Some(run()));
        assert!(
            !extreme.accepts(&stamped.stamp),
            "untrusted leads cannot overflow arithmetic"
        );
        for layer in MODEL_LAYERS
            .into_iter()
            .filter(|layer| rtma_field(*layer).is_some())
        {
            let request = ModelRequest::Analysis(layer, Some(run()));
            for kind in [
                wxdata::rtma::AnalysisKind::Rtma,
                wxdata::rtma::AnalysisKind::Urma,
            ] {
                let stamped = field(
                    kind.label(),
                    rtma_field(layer).unwrap().slug(),
                    run(),
                    0,
                    1.0,
                );
                assert!(request.accepts(&stamped.stamp));
                assert!(
                    !ModelRequest::Analysis(layer, Some(run() - chrono::Duration::hours(1)))
                        .accepts(&stamped.stamp)
                );
            }
        }
    }

    #[test]
    fn model_context_obsolete_success_and_failure_cannot_change_selected_health() {
        let a = ModelRequest::Reflectivity(M::Hrrr, 1, Some(run()));
        let b = ModelRequest::Reflectivity(M::Rap, 1, Some(run()));
        let lane = RequestLane::Field(L::Hrrr);
        let mut book = RequestBook::default();
        let old = book.start(lane.clone());
        // Selection changes before a replacement request starts. Both result types are discarded.
        assert!(!a.current(Some(b), Some(a)));
        book.discard(&lane, old);
        assert!(!book.finish(&lane, old, None, Some(run())));
        assert!(!book.finish(&lane, old, Some("obsolete failure"), None));
        assert!(book.health(&lane).last_success.is_none());
        assert!(book.health(&lane).error.is_none());
        book.reset(&lane);
        let current = book.start(lane.clone());
        assert_ne!(old, current);
        assert!(!book.finish(&lane, old, Some("old failure"), None));
        assert!(book.health(&lane).fetching);
        assert!(book.finish(
            &lane,
            current,
            None,
            Some(run() + chrono::Duration::hours(1))
        ));
        assert_eq!(book.health(&lane).recent_outcomes, Some((1, 0)));
    }

    #[test]
    fn model_context_health_discloses_previous_field_without_current_cache_or_clock() {
        let a = ModelRequest::Reflectivity(M::Hrrr, 1, Some(run()));
        let b = ModelRequest::Reflectivity(M::Rap, 1, Some(run()));
        let lane = RequestLane::Field(L::Hrrr);
        let mut book = RequestBook::default();
        let generation = book.start(lane.clone());
        book.finish(
            &lane,
            generation,
            None,
            Some(run() + chrono::Duration::hours(1)),
        );
        let mut state = FieldState::default();
        state.begin_model(a, Instant::now());
        assert!(stage(&mut state, a, 30.0));
        let pending = model_health(b, Some(&state), book.health(&lane));
        assert_eq!(pending.state(), HealthState::Waiting);
        assert_eq!(pending.cache_state, CacheState::Empty);
        assert!(pending.latest_valid_time.is_none());
        assert!(pending.last_success.is_none());
        assert!(pending.recent_outcomes.is_none());
        assert!(pending.recovery().contains("No usable data"));
        assert!(!pending.recovery().contains("marked Cached"));
        assert_eq!(pending.source, "RAP / Composite reflectivity");
        assert_eq!(pending.details[1].0, "Previous model field");
        assert!(pending.details[1].1.contains(M::Hrrr.label()));
        state.begin_model(b, Instant::now());
        book.reset(&lane);
        let generation = book.start(lane.clone());
        book.finish(&lane, generation, Some("current failure"), None);
        let failed = model_health(b, Some(&state), book.health(&lane));
        assert_eq!(failed.state(), HealthState::Failed);
        assert_eq!(failed.error.as_deref(), Some("current failure"));
        assert_eq!(failed.recent_outcomes, Some((0, 1)));
        assert!(failed.latest_valid_time.is_none());
        let rap = field(M::Rap.label(), "Composite reflectivity", run(), 60, 45.0);
        assert!(state.stage_model(b, rap, upload(45)));
        let generation = book.start(lane.clone());
        book.finish(
            &lane,
            generation,
            None,
            Some(run() + chrono::Duration::hours(1)),
        );
        let loaded = model_health(b, Some(&state), book.health(&lane));
        assert_eq!(loaded.cache_state, CacheState::Memory);
        assert!(loaded.recovery().contains("If a refresh fails"));
        assert!(loaded
            .details
            .iter()
            .all(|(key, _)| *key != "Previous model field"));
        let generation = book.start(lane.clone());
        book.finish(&lane, generation, Some("retry failed"), None);
        assert_eq!(
            model_health(b, Some(&state), book.health(&lane)).state(),
            HealthState::Cached
        );
    }
    #[test]
    fn model_context_latest_health_uses_the_resident_field_after_older_run_fallback() {
        let request = ModelRequest::Reflectivity(M::Hrrr, 1, None);
        let lane = RequestLane::Field(L::Hrrr);
        let mut book = RequestBook::default();
        let generation = book.start(lane.clone());
        book.finish(
            &lane,
            generation,
            None,
            Some(run() + chrono::Duration::hours(1)),
        );
        let older = field(
            M::Hrrr.label(),
            "Composite reflectivity",
            run() - chrono::Duration::hours(6),
            60,
            20.0,
        );
        let actual = older.stamp.valid_time;
        let mut state = FieldState::default();
        state.begin_model(request, Instant::now());
        assert!(state.stage_model(request, older, upload(20)));
        assert_ne!(book.health(&lane).latest_valid_time, Some(actual));
        assert_eq!(
            model_health(request, Some(&state), book.health(&lane)).latest_valid_time,
            Some(actual)
        );
    }

    /// Fixtures pass through the real request book and field admission before Sources renders them.
    pub(in crate::app) fn review_health(case: &str) -> SourceHealth {
        let a = ModelRequest::Reflectivity(M::Hrrr, 1, Some(run()));
        let b = ModelRequest::Reflectivity(M::Rap, 1, Some(run()));
        let lane = RequestLane::Field(L::Hrrr);
        let mut book = RequestBook::default();
        let generation = book.start(lane.clone());
        book.finish(
            &lane,
            generation,
            None,
            Some(run() + chrono::Duration::hours(1)),
        );
        let mut state = FieldState::default();
        state.begin_model(a, Instant::now());
        assert!(stage(&mut state, a, 30.0));
        if case != "waiting" {
            state.begin_model(b, Instant::now());
            book.reset(&lane);
            let generation = book.start(lane.clone());
            match case {
                "failed" => {
                    book.finish(&lane, generation, Some("RAP request timed out"), None);
                }
                "loaded" => {
                    assert!(state.stage_model(
                        b,
                        field(M::Rap.label(), "Composite reflectivity", run(), 60, 45.0),
                        upload(45)
                    ));
                    book.finish(
                        &lane,
                        generation,
                        None,
                        Some(run() + chrono::Duration::hours(1)),
                    );
                }
                "fetching" => {}
                _ => panic!("unknown review case"),
            }
        }
        model_health(b, Some(&state), book.health(&lane))
    }
}
