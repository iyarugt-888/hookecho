//! The layers that follow the view's time rather than their own: GOES (sector, frame readiness)
//! and MRMS (product, request, readiness), and the time they follow. Moved out of `app.rs`
//! unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// The ABI sector the GOES layers read this frame (see [`goes_sector_for`]).
    pub(crate) fn goes_sector_now(&self) -> wxdata::goes_abi::Sector {
        let (min_lon, min_lat, max_lon, max_lat) = self.view_bounds();
        let center = ((min_lon + max_lon) * 0.5, (min_lat + max_lat) * 0.5);
        goes_sector_for(self.settings.goes_sector, self.goes_footprint, center)
    }

    /// The time the time-following layers (MRMS, GOES, GLM) should show: the linked archive
    /// instant, or the active pane's scan when its timeline is scrubbed back or playing rather
    /// than following live; `None` for live.
    pub(crate) fn view_target_time(&self) -> Option<DateTime<Utc>> {
        self.linked_archive_time().or_else(|| {
            let tl = &self.views[self.active].timeline;
            if tl.following || tl.forecast_hour().is_some() {
                return None;
            }
            tl.current().and_then(|id| id.date_time())
        })
    }

    /// Whether a GOES layer's grid matches the time the view shows (see [`goes_frame_ready`]).
    /// The two derived products that only exist live (the band difference and the cooling rate)
    /// are not painted over a scrubbed view at all. Any other layer answers true.
    pub(crate) fn goes_ready(&self, layer: crate::render::FieldLayer) -> bool {
        use crate::render::FieldLayer as FL;
        let target = self.view_target_time();
        if matches!(layer, FL::GoesDustDiff | FL::GoesCoolingRate) {
            return target.is_none();
        }
        if !is_goes_frame_layer(layer) {
            return true;
        }
        let sector = self
            .goes_fetched_sector
            .get(&layer)
            .copied()
            .unwrap_or_default();
        goes_frame_ready(
            self.goes_fetched_slot.get(&layer).copied(),
            goes_slot(target, sector),
            self.fields
                .get(&layer)
                .and_then(|s| s.grid.as_ref())
                .map(|g| g.time),
            target,
            chrono::Duration::minutes(self.settings.time_mismatch_minutes as i64)
                .max(chrono::Duration::seconds(sector.cadence_secs() as i64)),
        )
    }

    /// Whether any layer that reads the chosen GOES sector is on.
    pub(crate) fn goes_layers_on(&self) -> bool {
        GOES_FRAME_LAYERS.into_iter().any(|l| self.field_wanted(l))
    }

    /// A line for the Satellite settings on where the chosen mesoscale box is, and whether the
    /// view is reading it or falling back to CONUS.
    pub(crate) fn goes_sector_note(&self) -> Option<String> {
        let chosen = self.settings.goes_sector;
        if !chosen.is_meso() {
            return None;
        }
        let Some((sector, fp)) = self.goes_footprint.filter(|(s, _)| *s == chosen) else {
            return Some("Finding where the sector is pointed\u{2026}".into());
        };
        let place = format!(
            "{:.0}\u{2013}{:.0}\u{b0}N, {:.0}\u{2013}{:.0}\u{b0}W at {}",
            fp.lat_south,
            fp.lat_north,
            -fp.lon_east,
            -fp.lon_west,
            fp.time.format("%H:%MZ")
        );
        Some(if self.goes_sector_now() == sector {
            format!("{} is over {place}.", sector.label())
        } else {
            format!(
                "{} is over {place}, away from this view: showing CONUS until it covers the view \
                 again.",
                sector.label()
            )
        })
    }

    /// Resolve existing layer IDs through the MRMS catalog; other sources have their own fetches.
    pub(crate) fn mrms_product(&self, layer: crate::render::FieldLayer) -> Option<String> {
        let product = wxdata::mrms::catalog::find(layer.slug())?;
        Some(
            product
                .path(
                    self.rotation_minutes,
                    self.settings.lightning_minutes,
                    self.hail_minutes,
                )
                .to_string(),
        )
    }

    pub(crate) fn mrms_request(&self, layer: crate::render::FieldLayer) -> Option<MrmsRequest> {
        Some(MrmsRequest {
            product: self.mrms_product(layer)?,
            archive: self
                .view_target_time()
                .map(|target| (target, self.settings.time_mismatch_minutes)),
        })
    }

    pub(crate) fn mrms_ready(&self, layer: crate::render::FieldLayer) -> bool {
        if !self.radar_field_ready(self.active, layer) {
            return false;
        }
        // GOES layers follow the view's time the same way (ROADMAP_NEW A2/E7).
        if !self.goes_ready(layer) {
            return false;
        }
        // These two current-only composites do not have archive selection yet. A previous live
        // texture must not be painted over an archive scan.
        if self.view_target_time().is_some()
            && matches!(
                layer,
                crate::render::FieldLayer::Mosaic | crate::render::FieldLayer::SnowBands
            )
        {
            return false;
        }
        let Some(request) = self.mrms_request(layer) else {
            return true;
        };
        self.fields
            .get(&layer)
            .is_some_and(|state| state.mrms_ready(&request))
    }
}
