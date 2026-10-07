//! Immutable GOES selection and accepted-image identity. Decoded storage is still shared;
//! panes with a different selection wait rather than borrowing the active pane's imagery.
use super::*;
use wxdata::goes_abi::{Footprint, Satellite, Sector};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GoesRequest {
    pub layer: Option<crate::render::FieldLayer>,
    pub west: bool,
    pub sector: Sector,
    pub at: Option<DateTime<Utc>>,
    pub tolerance_minutes: u16,
    pub recipe: Option<&'static str>,
}

impl GoesRequest {
    pub(super) fn from_source(source: &OverlaySource, tolerance_minutes: u16) -> Option<Self> {
        let (layer, satellite, sector, at, recipe) = match source {
            OverlaySource::Goes(layer, satellite, sector, at) => {
                (Some(*layer), *satellite, *sector, *at, None)
            }
            OverlaySource::GoesDiff(layer, satellite)
            | OverlaySource::GoesCoolingRate(layer, satellite) => {
                (Some(*layer), *satellite, Sector::Conus, None, None)
            }
            OverlaySource::GoesRgb(recipe, satellite, sector, at) => (
                Some(crate::render::FieldLayer::GoesRgb),
                *satellite,
                *sector,
                *at,
                Some(recipe.slug),
            ),
            OverlaySource::GoesFootprint(satellite, sector, at) => {
                (None, *satellite, *sector, *at, None)
            }
            _ => return None,
        };
        Some(Self {
            layer,
            west: satellite == Satellite::West,
            sector,
            at,
            tolerance_minutes,
            recipe,
        })
    }
    fn satellite(self) -> Satellite {
        if self.west {
            Satellite::West
        } else {
            Satellite::East
        }
    }
    pub(super) fn source(self) -> OverlaySource {
        use crate::render::FieldLayer as L;
        match self.layer {
            None => OverlaySource::GoesFootprint(self.satellite(), self.sector, self.at),
            Some(L::GoesRgb) => OverlaySource::GoesRgb(
                wxdata::goes_rgb::by_slug(self.recipe.unwrap_or("air-mass"))
                    .unwrap_or(&wxdata::goes_rgb::AIR_MASS),
                self.satellite(),
                self.sector,
                self.at,
            ),
            Some(layer @ L::GoesDustDiff) => OverlaySource::GoesDiff(layer, self.satellite()),
            Some(layer @ L::GoesCoolingRate) => {
                OverlaySource::GoesCoolingRate(layer, self.satellite())
            }
            Some(layer) => OverlaySource::Goes(layer, self.satellite(), self.sector, self.at),
        }
    }
    pub(super) fn footprint_request(self) -> Self {
        Self {
            layer: None,
            recipe: None,
            ..self
        }
    }
    pub(super) fn cadence(self) -> std::time::Duration {
        std::time::Duration::from_secs(match self.layer {
            Some(layer) if !self.sector.is_meso() => field_refresh_secs(layer),
            _ => self.sector.cadence_secs(),
        })
    }
    pub(super) fn accepts_time(self, time: DateTime<Utc>) -> bool {
        self.at.is_none_or(|at| {
            (time - at).abs()
                <= chrono::Duration::minutes(self.tolerance_minutes as i64)
                    .max(chrono::Duration::seconds(self.sector.cadence_secs() as i64))
        })
    }
    pub(super) fn description(self) -> String {
        format!(
            "GOES-{} / {} / {}; {}",
            if self.west { "West" } else { "East" },
            self.sector.label(),
            self.recipe
                .unwrap_or_else(|| self.layer.map_or("sector footprint", |l| l.slug())),
            self.at
                .map(|at| format!(
                    "analysis {} UTC ±{} min",
                    at.format("%Y-%m-%d %H:%M:%S"),
                    (self.tolerance_minutes as u64).max(self.sector.cadence_secs() / 60)
                ))
                .unwrap_or_else(|| "latest scan requested".into())
        )
    }
    pub(super) fn stamp(
        self,
        data: wxdata::mrms::MrmsField,
    ) -> wxdata::field::Stamped<wxdata::mrms::MrmsField> {
        let mut stamp = field_state::model_stamp(
            if self.west { "GOES-West" } else { "GOES-East" },
            self.recipe
                .unwrap_or_else(|| self.layer.map_or("sector footprint", |l| l.slug())),
            &data,
            None,
            self.recipe.is_some()
                || matches!(
                    self.layer,
                    Some(
                        crate::render::FieldLayer::GoesColdTop
                            | crate::render::FieldLayer::GoesDustDiff
                            | crate::render::FieldLayer::GoesCoolingRate
                    )
                ),
        );
        stamp.is_forecast = false;
        wxdata::field::Stamped { data, stamp }
    }
    pub(super) fn accepts_field(
        self,
        field: &wxdata::field::Stamped<wxdata::mrms::MrmsField>,
    ) -> bool {
        self.layer.is_some()
            && field.data.time == field.stamp.valid_time
            && self.accepts_time(field.data.time)
            && field.stamp.source_id == if self.west { "GOES-West" } else { "GOES-East" }
            && field.stamp.product_id == self.recipe.unwrap_or_else(|| self.layer.unwrap().slug())
    }
    pub(super) fn accepts_message(self, msg: &OverlayMsg) -> bool {
        match msg {
            OverlayMsg::GoesField(request, field) => *request == self && self.accepts_field(field),
            OverlayMsg::GoesFootprintFor(request, fp) => {
                *request == self && self.layer.is_none() && self.accepts_time(fp.time)
            }
            _ => false,
        }
    }
}

pub(super) fn is_goes(layer: crate::render::FieldLayer) -> bool {
    is_goes_frame_layer(layer)
        || matches!(
            layer,
            crate::render::FieldLayer::GoesDustDiff | crate::render::FieldLayer::GoesCoolingRate
        )
}

#[derive(Default)]
pub(super) struct GoesSlot {
    pub requested: Option<GoesRequest>,
    pub accepted: Option<GoesRequest>,
    last_fetch: Option<Instant>,
}
impl GoesSlot {
    pub(super) fn due(&self, request: GoesRequest, resident: bool, now: Instant) -> bool {
        self.requested != Some(request)
            || (!(request.at.is_some() && self.accepted == Some(request) && resident)
                && self
                    .last_fetch
                    .is_none_or(|last| now.saturating_duration_since(last) >= request.cadence()))
    }
    pub(super) fn begin(&mut self, request: GoesRequest, now: Instant) -> Option<GoesRequest> {
        let retired = self.requested.filter(|old| *old != request);
        if self.requested != Some(request) {
            self.accepted = None;
        }
        self.requested = Some(request);
        self.last_fetch = Some(now);
        retired
    }
    pub(super) fn accept(&mut self, request: GoesRequest) -> bool {
        if self.requested != Some(request) {
            return false;
        }
        self.accepted = Some(request);
        true
    }
    pub(super) fn ready(&self, request: GoesRequest, time: Option<DateTime<Utc>>) -> bool {
        self.requested == Some(request)
            && self.accepted == Some(request)
            && time.is_some_and(|time| request.accepts_time(time))
    }
}

impl HookEchoApp {
    fn goes_clock_for(&self, idx: usize) -> Option<Option<DateTime<Utc>>> {
        let timeline = &self.views.get(idx)?.timeline;
        // A satellite loop drives the GOES layers' own clock: the scan on its playhead.
        if let Some(scan) = self.sat_loop.current() {
            return Some(Some(scan));
        }
        let target = self.model_target_time(idx);
        (target.is_some() || timeline.following || timeline.forecast_hour().is_some())
            .then_some(target)
    }
    pub(super) fn selected_goes_footprint_for(&self, idx: usize) -> Option<GoesRequest> {
        Some(GoesRequest {
            layer: None,
            west: self.settings.goes_satellite_west,
            sector: self.settings.goes_sector,
            at: self.goes_clock_for(idx)?,
            tolerance_minutes: self.settings.time_mismatch_minutes,
            recipe: None,
        })
    }
    pub(super) fn goes_footprint_for(&self, idx: usize) -> Option<(Sector, Footprint)> {
        let wanted = self.selected_goes_footprint_for(idx)?;
        self.goes_footprint
            .filter(|(request, fp)| *request == wanted && request.accepts_time(fp.time))
            .map(|(request, fp)| (request.sector, fp))
    }
    pub(super) fn goes_sector_for_pane(&self, idx: usize) -> Sector {
        let view = &self.views[idx];
        // A sector's coverage is geographic; camera focus must not select another pane's box.
        let center =
            crate::render::mercator::world_to_lonlat(view.camera.center.0, view.camera.center.1);
        goes_sector_for(
            self.settings.goes_sector,
            self.goes_footprint_for(idx),
            center,
        )
    }
    pub(super) fn selected_goes_request_for(
        &self,
        idx: usize,
        layer: crate::render::FieldLayer,
    ) -> Option<GoesRequest> {
        if !is_goes(layer) {
            return None;
        }
        let at = self.goes_clock_for(idx)?;
        let derived = matches!(
            layer,
            crate::render::FieldLayer::GoesDustDiff | crate::render::FieldLayer::GoesCoolingRate
        );
        if derived && at.is_some() {
            return None;
        }
        Some(GoesRequest {
            layer: Some(layer),
            west: self.settings.goes_satellite_west,
            sector: if derived {
                Sector::Conus
            } else {
                self.goes_sector_for_pane(idx)
            },
            at,
            tolerance_minutes: self.settings.time_mismatch_minutes,
            recipe: (layer == crate::render::FieldLayer::GoesRgb).then(|| {
                wxdata::goes_rgb::by_slug(&self.settings.goes_rgb_recipe)
                    .unwrap_or(&wxdata::goes_rgb::AIR_MASS)
                    .slug
            }),
        })
    }
    pub(super) fn goes_request_current(&self, request: GoesRequest) -> bool {
        match request.layer {
            Some(layer) => {
                self.field_wanted(layer)
                    && self.selected_goes_request_for(self.active, layer) == Some(request)
                    && self
                        .goes_fields
                        .get(&layer)
                        .is_some_and(|slot| slot.requested == Some(request))
            }
            None => {
                self.goes_layers_on()
                    && self.selected_goes_footprint_for(self.active) == Some(request)
                    && self.goes_footprint_slot.requested == Some(request)
            }
        }
    }
    pub(super) fn goes_ready_for(&self, idx: usize, layer: crate::render::FieldLayer) -> bool {
        if !is_goes(layer) {
            return true;
        }
        let Some(request) = self.selected_goes_request_for(idx, layer) else {
            return false;
        };
        self.goes_fields.get(&layer).is_some_and(|slot| {
            slot.ready(
                request,
                self.fields
                    .get(&layer)
                    .and_then(|state| state.grid.as_ref())
                    .map(|grid| grid.time),
            )
        })
    }
    pub(super) fn schedule_goes(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let mut wanted = std::collections::HashSet::new();
        for layer in GOES_FRAME_LAYERS.into_iter().chain([
            crate::render::FieldLayer::GoesDustDiff,
            crate::render::FieldLayer::GoesCoolingRate,
        ]) {
            if !self.field_wanted(layer) {
                continue;
            }
            let Some(request) = self.selected_goes_request_for(self.active, layer) else {
                continue;
            };
            wanted.insert(request);
            let resident = self.fields.get(&layer).is_some_and(|s| s.grid.is_some());
            let slot = self.goes_fields.entry(layer).or_default();
            if !slot.due(request, resident, now) {
                continue;
            }
            if let Some(retired) = slot.begin(request, now) {
                self.acquisition.reset(&RequestLane::Goes(retired));
                if let Some(state) = self.fields.get_mut(&layer) {
                    state.pending = None;
                    state.stamp = None;
                }
            }
            self.fields.entry(layer).or_default().last_fetch = Some(now);
            self.acquisition
                .spawn_goes(ctx, request, self.field_texture_cap());
        }
        if let Some(request) = self
            .selected_goes_footprint_for(self.active)
            .filter(|r| r.sector.is_meso() && self.goes_layers_on())
        {
            // Probe while reading CONUS, and also on a new context before trusting an old box.
            wanted.insert(request);
            let resident = self.goes_footprint.is_some_and(|(r, _)| r == request);
            if self.goes_footprint_slot.due(request, resident, now) {
                if let Some(retired) = self.goes_footprint_slot.begin(request, now) {
                    self.acquisition.reset(&RequestLane::Goes(retired));
                }
                self.acquisition
                    .spawn_goes(ctx, request, self.field_texture_cap());
            }
        }
        for retired in self.acquisition.cancel_unwanted_goes(&wanted) {
            if let Some(layer) = retired.layer {
                if let Some(slot) = self.goes_fields.get_mut(&layer) {
                    slot.last_fetch = None;
                }
            } else {
                self.goes_footprint_slot.last_fetch = None;
            }
        }
    }
    pub(super) fn goes_health(&self, request: GoesRequest) -> SourceHealth {
        let mut health = self.acquisition.health(&RequestLane::Goes(request));
        let time = if let Some(layer) = request.layer {
            self.fields
                .get(&layer)
                .and_then(|state| state.grid.as_ref())
                .map(|grid| grid.time)
                .filter(|time| {
                    self.goes_fields
                        .get(&layer)
                        .is_some_and(|slot| slot.ready(request, Some(*time)))
                })
        } else {
            self.goes_footprint
                .filter(|(r, _)| *r == request)
                .map(|(_, fp)| fp.time)
        };
        health.latest_valid_time = time;
        health.cache_state = if time.is_some() {
            CacheState::Memory
        } else {
            CacheState::Empty
        };
        health.selection_only = request.at.is_some() && time.is_some();
        health
            .details
            .push(("Requested satellite analysis", request.description()));
        if let Some(time) = time {
            health.details.push((
                "Loaded satellite analysis",
                format!("{} UTC", time.format("%Y-%m-%d %H:%M:%S")),
            ));
        }
        health
    }
}
