//! The layers that follow the view's time rather than their own: GOES (sector, frame readiness)
//! and MRMS (product, request, readiness), and the time they follow. Moved out of `app.rs`
//! unchanged (ROADMAP_2 §7).

use super::*;

impl HookEchoApp {
    /// The ABI sector the GOES layers read this frame (see [`goes_sector_for`]).
    pub(crate) fn goes_sector_now(&self) -> wxdata::goes_abi::Sector {
        self.goes_sector_for_pane(self.active)
    }

    /// The time the time-following layers (MRMS, GOES, GLM) should show: the linked archive
    /// instant, or the active pane's scan when its timeline is scrubbed back or playing rather
    /// than following live; `None` for live.
    pub(crate) fn view_target_time(&self) -> Option<DateTime<Utc>> {
        self.model_target_time(self.active)
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
        let Some((sector, fp)) = self
            .goes_footprint_for(self.active)
            .filter(|(s, _)| *s == chosen)
        else {
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

    pub(crate) fn mrms_ready(&self, layer: crate::render::FieldLayer) -> bool {
        self.mrms_ready_for(self.active, layer)
    }
    pub(crate) fn mrms_ready_for(&self, idx: usize, layer: crate::render::FieldLayer) -> bool {
        if !self.model_field_ready_for(idx, layer) || !self.radar_field_ready(idx, layer) {
            return false;
        }
        if mrms_context::is_mrms(layer) {
            return self.mrms_context_ready_for(idx, layer);
        }
        // GOES layers follow the view's time the same way (ROADMAP_NEW A2/E7).
        if !self.goes_ready_for(idx, layer) {
            return false;
        }
        // These two current-only composites do not have archive selection yet. A previous live
        // texture must not be painted over an archive scan.
        if self.model_target_time(idx).is_some()
            && matches!(
                layer,
                crate::render::FieldLayer::Mosaic | crate::render::FieldLayer::SnowBands
            )
        {
            return false;
        }
        true
    }
}
