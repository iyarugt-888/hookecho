//! [`OverlayToggle`]: every boolean overlay and panel toggle, addressable by name so the layers
//! panel, the command palette and workspaces flip any of them without a match arm per surface.
//! Moved out of `app.rs` unchanged (ROADMAP_2 §7).

/// A boolean overlay/panel toggle addressable by name, so the layers panel and command palette
/// can flip any of them without a match arm per surface (see [`HookEchoApp::overlay_flag`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum OverlayToggle {
    AlertPanel,
    StormReports,
    Spotters,
    RadarSites,
    Metar,
    Webcams,
    Fires,
    Aqi,
    Stations,
    Dat,
    Gauges,
    Tropical,
    /// County power outages (ODIN).
    Outages,
    /// NWS forecast-zone outlines.
    ForecastZones,
    /// Each forecast office's county warning area.
    CwaBoundaries,
    ProbSevere,
    Aviation,
    Tfr,
    RangeRings,
    Sensors,
    Hodo,
    Cells,
    Tracks,
    ArrivalCones,
    Nowcast,
    /// Per-gate max/min over a trailing window of cached volumes (ROADMAP_NEW C2).
    Trail,
    /// A ring at the sweep edge coloured by how long before the newest data each azimuth was collected.
    ScanAge,
    /// Tornado detection: the rotation and debris detectors and Tornado ID's verdicts, drawn as one
    /// marker per tornado that opens into the web of detections it ties together
    /// (`wxdata::tornado_id::circulations`). Its slug stays `TornadoId`; the separate rotation
    /// (`Couplets`) and debris (`Tds`) layers it replaced restore as it ([`Self::from_slug`]).
    TornadoId,
    /// Y'all mode (`crate::yall`): the Y'all-O-Meter card and Y'all Tracks.
    YallMode,
    /// The layer probe (`app::layer_probe`): every visible layer's reading under the pointer.
    LayerProbe,
    /// The zoomed-out map as a globe (`render::mercator::set_globe`).
    Globe,
    /// A distance ruler in the corner of each flat pane (`app::scale_bar`).
    ScaleBar,
    Tbss,
    ZdrColumns,
    Alerts,
    Mds,
    Mping,
    Pireps,
    Recon,
    Fronts,
    /// SPC tornado and severe thunderstorm watches in effect.
    Watches,
    /// Cell tracks computed here from reflectivity, for sites with no Level 3 SCIT product.
    LocalTracks,
    GlmLightning,
    /// Ground strikes republished onto the user's own MQTT broker (see `strikes_topic`).
    Strikes,
    Wind,
    /// Wind particles from the active radar's Doppler velocity (and its VAD), not the model.
    RadarWind,
    LinkCameras,
    /// Align archive radar panes by valid time, using each site's nearest volume.
    LinkTimes,
    /// Snap the shared analysis cursor to the active radar's exact source frame after seeking.
    LockSourceTime,
    /// ROADMAP_NEW J2: picking a new radar site in one pane sets it in every other pane too —
    /// each pane keeps its own product/tilt, so this is for comparing several products of one
    /// storm rather than making every pane identical.
    LinkSite,
    /// ROADMAP_NEW J3: hovering any pane shows the same geographic point on every other pane, plus
    /// a compact per-pane value table — a shared crosshair rather than each pane's own independent
    /// cursor.
    LinkCursor,
    /// ROADMAP_NEW J2's storm-selection link: the selected storm cell is marked in every pane,
    /// and every pane recenters on it when the selection changes or the storm moves.
    LinkStorm,
    /// The always-on-top mini-loop window (desktop only).
    MiniLoop,
    /// Beam-vs-terrain blockage shading for the displayed tilt (chase mode).
    Blockage,
    /// ROADMAP_NEW C3's "lowest usable beam map": which of the volume's tilts is the lowest one
    /// that actually clears the terrain, shaded green (low tilt reaches) to red (needs a high
    /// one) — "which tilt do I need here", distinct from `Blockage`'s "is my current tilt good
    /// here".
    LowestTilt,
    /// Day/night shading, the terminator line, and the lat/lon graticule.
    DayNight,
    /// ROADMAP_NEW I1: shapes from a user-imported GeoJSON or Shapefile.
    ImportedGis,
}

impl OverlayToggle {
    /// Every toggle, for the persistence sweep. A new variant belongs here too, or it silently
    /// stops being remembered across restarts.
    pub(crate) const ALL: [OverlayToggle; 57] = [
        Self::AlertPanel,
        Self::StormReports,
        Self::Spotters,
        Self::RadarSites,
        Self::Metar,
        Self::Webcams,
        Self::Fires,
        Self::Aqi,
        Self::Stations,
        Self::Dat,
        Self::Gauges,
        Self::Tropical,
        Self::Outages,
        Self::ForecastZones,
        Self::CwaBoundaries,
        Self::ProbSevere,
        Self::Aviation,
        Self::Tfr,
        Self::RangeRings,
        Self::Sensors,
        Self::Hodo,
        Self::Cells,
        Self::Tracks,
        Self::ArrivalCones,
        Self::Nowcast,
        Self::Trail,
        Self::ScanAge,
        Self::TornadoId,
        Self::YallMode,
        Self::LayerProbe,
        Self::Globe,
        Self::ScaleBar,
        Self::Tbss,
        Self::ZdrColumns,
        Self::Alerts,
        Self::Mds,
        Self::Mping,
        Self::Pireps,
        Self::Recon,
        Self::Fronts,
        Self::Watches,
        Self::LocalTracks,
        Self::GlmLightning,
        Self::Strikes,
        Self::Wind,
        Self::RadarWind,
        Self::LinkCameras,
        Self::LinkTimes,
        Self::LockSourceTime,
        Self::LinkSite,
        Self::LinkCursor,
        Self::LinkStorm,
        Self::MiniLoop,
        Self::Blockage,
        Self::LowestTilt,
        Self::DayNight,
        Self::ImportedGis,
    ];

    /// The links between panes (roadmap J2), in the order the Panes menu lists them. The time
    /// lock is left out: it refines linked times rather than being a link of its own.
    pub(crate) const PANE_LINKS: [OverlayToggle; 5] = [
        Self::LinkCameras,
        Self::LinkTimes,
        Self::LinkSite,
        Self::LinkCursor,
        Self::LinkStorm,
    ];

    /// Toggles that describe this session's window arrangement rather than a layer. Pane links
    /// are captured by a saved workspace but are not global layer preferences; the mini loop is
    /// a window and is not persisted. `ImportedGis` restores from its own remembered file
    /// reference and turns itself on only after that reload succeeds, so it does not need a
    /// second persisted toggle that could disagree with the file.
    pub(crate) fn session_only(self) -> bool {
        matches!(
            self,
            Self::LinkCameras
                | Self::LinkTimes
                | Self::LockSourceTime
                | Self::LinkSite
                | Self::LinkCursor
                | Self::LinkStorm
                | Self::MiniLoop
                | Self::ImportedGis
        )
    }

    /// Stable name used in the settings file. Persisted as a string, not as the enum: an unknown
    /// name written by a newer build has to be skippable, and a failed `Settings` parse takes the
    /// whole file down with it.
    pub(crate) fn slug(self) -> String {
        // The variant name, which is also the serde name — one list of names, not two.
        format!("{self:?}")
    }

    pub(crate) fn from_slug(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.slug() == s).or(match s {
            // The rotation-couplet and debris-signature layers, merged into Tornado detection: a
            // settings file, workspace or scene that had either on turns it on.
            "Couplets" | "Tds" => Some(Self::TornadoId),
            _ => None,
        })
    }
}
