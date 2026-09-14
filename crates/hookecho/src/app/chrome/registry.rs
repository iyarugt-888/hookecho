//! The one action registry every surface searches: palette entries and their build.

use super::*;

/// Whether `layer`'s fetch health belongs in the request book: every MRMS catalog product
/// automatically qualifies, so a new one is tracked the day it is added rather than needing this
/// list remembered too. What follows is only the layers the catalog does not cover yet (HRRR/RAP,
/// the global suite, local-radar-derived fields) — see `docs/field-registry.md`'s "Remaining
/// registry work". A free function rather than a `palette_health` match arm so the mapping is
/// testable without an `HookEchoApp` to hang it off of.
fn field_layer_is_health_tracked(layer: crate::render::FieldLayer) -> bool {
    use crate::render::FieldLayer as FL;
    layer.descriptor().is_some()
        || matches!(
            layer,
            FL::Mosaic
                | FL::SnowBands
                | FL::Vil
                | FL::EchoTops
                | FL::Hca
                | FL::Hrrr
                | FL::UpdraftHelicity
                | FL::SnowAnalysis
                | FL::Snowfall
                | FL::Smoke
                | FL::Cape
                | FL::Srh
                | FL::GlobalMslp
                | FL::GlobalHeight500
                | FL::GlobalTemp2m
                | FL::GlobalDewpoint2m
                | FL::GlobalWind10m
                | FL::GlobalPrecip
                | FL::ThunderProb
                | FL::GlmFed
                | FL::ModelDiff
                | FL::CompareA
                | FL::CompareB
        )
}

/// The ingest-lag reading `radar_health`'s detail lines show: how far behind wall clock the
/// data already was the moment this client actually received it (the radar's own timestamp
/// against this client's receipt) — not the local network/decode time on top of that. `None`
/// until the first live arrival lands. A free function, like `field_layer_is_health_tracked`
/// above, so it is testable without an `HookEchoApp`.
fn ingest_lag_detail(
    last_live_arrival: Option<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>,
) -> Option<(&'static str, String)> {
    last_live_arrival.map(|(received_at, valid_time)| {
        let lag = (received_at - valid_time).num_seconds().max(0);
        ("Provider lag", humanize(lag))
    })
}

/// The retry-count detail line: only shown once the current live stream connection has actually
/// had to retry a chunk fetch — a perfectly healthy connection (the common case) doesn't earn a
/// permanent "0 retries" line taking up space for nothing. A free function for the same reason as
/// `ingest_lag_detail` above.
fn retry_detail(retries: u32) -> Option<(&'static str, String)> {
    (retries > 0).then(|| {
        (
            "Stream retries",
            format!("{retries} chunk fetch retr{}", if retries == 1 { "y" } else { "ies" }),
        )
    })
}

/// The local half of live latency, alongside `ingest_lag_detail`'s provider-side half: how long
/// the last live-stream sweep spent assembling and merging on this client, once one has actually
/// landed. `None` before the first live-stream update (as opposed to an interval poll, which
/// doesn't measure this) arrives.
fn decode_time_detail(last: Option<std::time::Duration>) -> Option<(&'static str, String)> {
    last.map(|d| ("Decode time", format_millis(d)))
}

/// Sub-second precision below 1s (decode times are normally tens to low hundreds of ms, where
/// `humanize`'s whole-second granularity would round everything down to a useless "0s"); whole
/// tenths of a second above that, since a decode slow enough to reach a second is already
/// noteworthy without needing millisecond precision on top.
fn format_millis(d: std::time::Duration) -> String {
    if d < std::time::Duration::from_secs(1) {
        format!("{}ms", d.as_millis())
    } else {
        format!("{:.1}s", d.as_secs_f64())
    }
}

impl HookEchoApp {
    fn request_health(&self, lane: RequestLane) -> SourceHealth {
        self.overlay_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .health(&lane)
    }

    pub(in crate::app) fn radar_health(&self) -> SourceHealth {
        let v = &self.views[self.active];
        // The timeline's own newest known frame, not the displayed volume: a rolling live loop
        // deliberately keeps its playhead frame on screen while a genuinely new head is appended
        // behind the scenes (see the `DataMsg::Volume` handler's `looping && new_head` case), so
        // `v.volume.time` can be well behind the feed's actual cadence while looping plays.
        let age = v.timeline.newest().and_then(|id| id.date_time()).map(|t| {
            (chrono::Utc::now() - t).to_std().unwrap_or_default()
        });
        // Phase B3's provider-ingest-lag reading; never set from an archive scrub or a loop's
        // replayed frame, only a genuine live arrival — see `last_live_arrival`'s own doc comment.
        let details = [
            ingest_lag_detail(v.last_live_arrival),
            decode_time_detail(v.last_decode_time),
            retry_detail(v.live_retries),
        ]
        .into_iter()
        .flatten()
        .collect();
        SourceHealth {
            source: v
                .site
                .as_deref()
                .map_or_else(|| "Radar".to_string(), |site| format!("{site} radar")),
            fetching: v.loading,
            last_attempt: v.last_poll.map(|t| t.elapsed()),
            last_success: age,
            last_failure: v.error.as_ref().map(|_| std::time::Duration::ZERO),
            error: v.error.clone(),
            // Shared with the scrubber's own Live/Stale badge (`RADAR_FRESH_SECS`) so the two
            // can never disagree about what counts as fresh — see that constant's doc comment.
            cadence: std::time::Duration::from_secs(RADAR_FRESH_SECS as u64),
            details,
        }
    }

    fn palette_health(&self, action: PaletteAction) -> Option<SourceHealth> {
        use crate::render::FieldLayer as FL;
        use OverlayToggle as T;
        let lane = match action {
            PaletteAction::SetMoment(..) => return Some(self.radar_health()),
            PaletteAction::SetContours(k) if k != ContourKind::Off => {
                RequestLane::Feed("Model contours")
            }
            PaletteAction::ToggleField(layer) if field_layer_is_health_tracked(layer) => {
                // Compare's one fetch feeds both layers at once and is filed under CompareA
                // (see `OverlaySource::Compare`'s own `lane()`); ask for that lane regardless of
                // which of the two the toggle is for, so CompareB's health isn't perpetually
                // "unknown" just because nothing was ever filed under its own name.
                let layer = if layer == FL::CompareB { FL::CompareA } else { layer };
                RequestLane::Field(layer)
            }
            PaletteAction::ToggleOverlay(toggle) => match toggle {
                T::AlertPanel | T::Alerts => RequestLane::Feed("Weather alerts"),
                T::StormReports => RequestLane::Feed("Storm reports"),
                T::Spotters => RequestLane::Feed("Spotter Network"),
                T::Metar => RequestLane::Feed("Surface observations"),
                T::Webcams => RequestLane::Feed("Webcams"),
                T::Fires => RequestLane::Feed("Wildfires"),
                T::Aqi => RequestLane::Feed("Air quality"),
                T::Stations => RequestLane::Feed("Live stations"),
                T::Dat => RequestLane::Feed("Damage surveys"),
                T::Gauges => RequestLane::Feed("River gauges"),
                T::Tropical => RequestLane::Feed("Tropical cyclones"),
                T::Outages => RequestLane::Feed("Power outages"),
                T::ProbSevere => RequestLane::Feed("ProbSevere"),
                T::Aviation => RequestLane::Feed("Aviation advisories"),
                T::Tfr => RequestLane::Feed("Temporary flight restrictions"),
                T::Sensors => RequestLane::Feed("Radar observations"),
                T::Hodo => RequestLane::Feed("VAD profile"),
                T::Cells | T::Tracks | T::ArrivalCones => {
                    RequestLane::Feed("Storm cells")
                }
                T::Mds => RequestLane::Feed("Mesoscale discussions"),
                T::Mping => RequestLane::Feed("mPING reports"),
                T::Pireps => RequestLane::Feed("Pilot reports"),
                T::Recon => RequestLane::Feed("Hurricane reconnaissance"),
                T::Fronts => RequestLane::Feed("Surface analysis"),
                T::Watches => RequestLane::Feed("Watch boxes"),
                T::Wind => RequestLane::Feed("Wind particles"),
                _ => return None,
            },
            _ => return None,
        };
        Some(self.request_health(lane))
    }

    /// Every layer/product/tool/window as a searchable, categorized row. Consumed by the layers
    /// panel (desktop slide-in + mobile sheet) and the Ctrl+K command palette.
    /// The action registry, rebuilt at most once a frame.
    ///
    /// Building it allocates ~150 owned strings, and up to five places render from it in the same
    /// frame (drawer, mobile sheets, legend, palette). Nothing can change it mid-frame — an
    /// action dispatched from one of those lists takes effect on the next one — so a per-frame
    /// memo is both free and honest.
    /// The command-palette action registry for this frame. Shared, not copied: the built list is
    /// a few hundred owned Strings, and several call sites want it in the same frame.
    pub(crate) fn palette_entries(&mut self) -> std::sync::Arc<[PaletteEntry]> {
        if let Some((frame, entries)) = &self.palette_cache {
            if *frame == self.frame_nr {
                return std::sync::Arc::clone(entries);
            }
        }
        let entries: std::sync::Arc<[PaletteEntry]> = self.palette_entries_build().into();
        self.palette_cache = Some((self.frame_nr, std::sync::Arc::clone(&entries)));
        entries
    }

    pub(crate) fn palette_entries_build(&mut self) -> Vec<PaletteEntry> {
        crate::prof_scope!("palette_entries");
        use crate::render::FieldLayer as FL;
        use AppWindow as W;
        use OverlayToggle as T;
        let mut out = Vec::new();
        // Reverse index from the live bindings, so a rebind relabels every row that shows a chip.
        let keys: Vec<(PaletteAction, String)> = crate::hotkeys::active(&self.settings)
            .iter()
            .filter_map(|b| match b.action {
                crate::hotkeys::BindableAction::Palette(p) => {
                    Some((p, crate::hotkeys::pretty(&b.shortcut)))
                }
                _ => None,
            })
            .collect();
        let mut push = |label: &str, category, desc, common, action, on| {
            // The panel groups rows by category, one collapsible per name in `CATEGORIES`. A row
            // with a category that isn't in that list draws nowhere: reachable from Ctrl+K, gone
            // from the panel. Cheaper to catch here than to notice a missing layer months later.
            debug_assert!(
                crate::ui::layers_panel::CATEGORIES.contains(&category),
                "registry row {label:?} has category {category:?}, which the panel does not draw"
            );
            out.push(PaletteEntry {
                label: label.to_string(),
                category,
                action,
                on,
                desc,
                common,
                key: keys
                    .iter()
                    .find(|(a, _)| *a == action)
                    .map(|(_, k)| k.clone()),
                health: None,
            })
        };

        // --- Radar products (the active pane's moment). ---
        let (cur_moment, cur_srv) = {
            let v = &self.views[self.active];
            (v.moment, v.srv)
        };
        // Every product, plus the storm-relative variant of velocity.
        let rows = crate::products::PRODUCTS
            .iter()
            .map(|p| (p.moment, false, p.name, p.blurb))
            .chain([(
                Moment::Velocity,
                true,
                "Storm rotation (SRV)",
                "Velocity with the storm's own motion subtracted out",
            )]);
        let have = self.available_moments();
        for (m, srv, label, desc) in rows {
            // A product this radar doesn't send is absent, not a row that paints nothing.
            if !have[m.index()] {
                continue;
            }
            let on = cur_moment == m && (m != Moment::Velocity || cur_srv == srv);
            push(
                label,
                "Radar",
                desc,
                true,
                PaletteAction::SetMoment(m, srv),
                Some(on),
            );
        }

        for product in wxdata::mrms::catalog::PRODUCTS {
            if let Some(layer) = FL::from_slug(product.field.id.0) {
                push(
                    product.field.name,
                    "National",
                    product.field.description,
                    product.common,
                    PaletteAction::ToggleField(layer),
                    Some(self.views[self.active].fields_on.contains(&layer)),
                );
            }
        }
        // --- National / model grids. ---
        for (layer, category, label, desc, common) in [
            (
                FL::Mosaic,
                "National",
                "Seamless mosaic (single-radar)",
                "Every nearby radar's own base reflectivity, stitched seamlessly",
                true,
            ),
            (
                FL::GoesIr,
                "National",
                "GOES IR satellite",
                "Cloud-top brightness temperature, read straight from the satellite over CONUS \
                 — East by default, West in Layer settings",
                true,
            ),
            (
                FL::GoesVisible,
                "National",
                "GOES visible satellite",
                "Daytime visible reflectance, read straight from the satellite over CONUS \
                 — East by default, West in Layer settings",
                true,
            ),
            (
                FL::GoesWaterVapor,
                "National",
                "GOES water vapor satellite",
                "Upper-level moisture, read straight from the satellite over CONUS \
                 — East by default, West in Layer settings",
                true,
            ),
            (
                FL::ThunderProb,
                "Models",
                "Chance of thunder (NBM)",
                "The National Blend's calibrated probability of a thunderstorm in the hour you \
                 have scrubbed to — a forecast, not a detection",
                false,
            ),
            (
                FL::SnowBands,
                "National",
                "Snow bands",
                "Snow organised into a line — the squall that whites out a road in two minutes, \
                 cut out of the national mosaic",
                false,
            ),
            (
                FL::Vil,
                "National",
                "Water aloft (VIL, L3)",
                "How much water the storm is holding aloft",
                false,
            ),
            (
                FL::EchoTops,
                "National",
                "Storm-top height (echo tops, L3)",
                "How tall the storm is",
                false,
            ),
            (
                FL::Hca,
                "National",
                "What the radar is seeing (hydrometeor class, L3)",
                "What the radar thinks it's seeing: rain, hail, debris",
                false,
            ),
            (
                FL::GlmFed,
                "National",
                "Lightning flashes (GLM)",
                "Where the satellite flashes are densest \u{2014} the total-lightning field                  behind the individual dots, and where a lightning jump shows up first.",
                false,
            ),
            (
                FL::CompositeLocal,
                "Radar",
                "Composite reflectivity",
                "The strongest echo anywhere above each point, not just what this tilt cuts through",
                false,
            ),
            (
                FL::VilLocal,
                "Radar",
                "Water aloft (VIL, derived)",
                "Water held aloft, computed from this volume \u{2014} works in archive replay",
                false,
            ),
            (
                FL::VilDensity,
                "Radar",
                "Hail signal (VIL density, derived)",
                "Water aloft per unit storm depth \u{2014} high values mean large hail",
                false,
            ),
            (
                FL::EtopLocal,
                "Radar",
                "Storm-top height (echo tops, derived)",
                "Storm-top height at a threshold you pick, from this volume",
                false,
            ),
            (
                FL::HailMehs,
                "Radar",
                "Max hail size (MEHS, derived)",
                "Largest hail this storm can be making, from the volume aloft (live only)",
                false,
            ),
            (
                FL::HailPosh,
                "Radar",
                "Severe-hail chance (POSH, derived)",
                "Odds this storm is producing hail an inch or larger (live only)",
                false,
            ),
            (
                FL::GlobalMslp,
                "Models",
                "Surface pressure (MSLP)",
                "Surface pressure worldwide — GFS or ECMWF, your pick in Layer options",
                false,
            ),
            (
                FL::GlobalHeight500,
                "Models",
                "Upper-level pattern (500 hPa height)",
                "The steering flow: where the troughs and ridges are, worldwide",
                false,
            ),
            (
                FL::GlobalTemp2m,
                "Models",
                "Surface temperature (2 m)",
                "Surface temperature worldwide",
                false,
            ),
            (
                FL::GlobalDewpoint2m,
                "Models",
                "Surface dewpoint (2 m)",
                "How much moisture the air is carrying, worldwide",
                false,
            ),
            (
                FL::GlobalWind10m,
                "Models",
                "Surface wind (10 m)",
                "Surface wind worldwide",
                false,
            ),
            (
                FL::GlobalPrecip,
                "Models",
                "Moisture in the air column",
                "Precipitable water (GFS) or total precipitation (ECMWF)",
                false,
            ),
            (
                FL::NdfdTemp2m,
                "Models",
                "NDFD temperature (2 m)",
                "The NWS's own forecaster-blended grid, CONUS \u{2014} not a raw model run",
                false,
            ),
            (
                FL::NdfdWind10m,
                "Models",
                "NDFD wind speed (10 m)",
                "The NWS's own forecaster-blended grid, CONUS \u{2014} not a raw model run",
                false,
            ),
            (
                FL::NdfdGust10m,
                "Models",
                "NDFD wind gust",
                "The NWS's own forecaster-blended grid, CONUS \u{2014} not a raw model run",
                false,
            ),
            (
                FL::NdfdSnow,
                "Models",
                "NDFD snowfall",
                "The NWS's own forecaster-blended grid, CONUS \u{2014} not a raw model run",
                false,
            ),
            (
                FL::ModelDiff,
                "Models",
                "Model difference",
                "Where two models disagree \u{2014} pick the field in layer options",
                false,
            ),
            (
                FL::CompareA,
                "Models",
                "Compare (pane A)",
                "One model's own field, meant for its own pane \u{2014} pick the field in layer options",
                false,
            ),
            (
                FL::CompareB,
                "Models",
                "Compare (pane B)",
                "The other model's own field, meant for its own pane",
                false,
            ),
            (
                FL::Hrrr,
                "Models",
                "HRRR future radar",
                "Forecast radar picture for the next 18 hours (not observed)",
                true,
            ),
            (
                FL::UpdraftHelicity,
                "Models",
                "Future rotation tracks",
                "Where storms are forecast to rotate \u{2014} scrub the timeline to extend the swath",
                true,
            ),
            (
                FL::SnowAnalysis,
                "National",
                "Snowfall analysis",
                "How much snow actually fell \u{2014} pick the window in layer options",
                false,
            ),
            (
                FL::Snowfall,
                "Models",
                "Forecast snowfall",
                "How much snow is forecast to pile up \u{2014} scrub the timeline to add hours",
                false,
            ),
            (
                FL::Smoke,
                "Models",
                "Wildfire smoke",
                "Forecast smoke near the ground, from active fires",
                false,
            ),
            (
                FL::Cape,
                "Models",
                "Storm fuel (CAPE)",
                "How much fuel the atmosphere has for storms",
                false,
            ),
            (
                FL::Srh,
                "Models",
                "Storm spin (SRH)",
                "How much spin the wind profile can feed a storm",
                false,
            ),
        ] {
            let on = self.views[self.active].fields_on.contains(&layer);
            push(
                label,
                category,
                desc,
                common,
                PaletteAction::ToggleField(layer),
                Some(on),
            );
        }
        for k in ContourKind::ALL {
            let label = format!("Contours: {}", k.display_label(self.settings.temp_unit));
            let on = self.contour_kind == k;
            push(
                &label,
                "Models",
                "Draw this forecast field as labeled contour lines",
                false,
                PaletteAction::SetContours(k),
                Some(on),
            );
        }

        // --- Severe / obs / reference toggles. ---
        for (t, category, label, desc, common) in [
            (
                T::Cells,
                "Severe",
                "Storm cells",
                "Mark each storm the radar is tracking",
                true,
            ),
            (
                T::Alerts,
                "Severe",
                "Alerts (NWS · EU · Canada)",
                "Official warning and watch polygons",
                true,
            ),
            (
                T::Couplets,
                "Severe",
                "Rotation couplets",
                "Flag tight rotation that could produce a tornado",
                true,
            ),
            (
                T::Tbss,
                "Severe",
                "Hail spikes (TBSS)",
                "Flag three-body scatter spikes \u{2014} near-proof of large hail in the core \
                 they point away from",
                false,
            ),
            (
                T::ZdrColumns,
                "Severe",
                "ZDR columns",
                "Flag rain carried above the freezing level \u{2014} an updraft proxy that \
                 deepens before a storm intensifies",
                false,
            ),
            (
                T::Tds,
                "Severe",
                "Debris detection (TDS)",
                "Flag lofted debris — a tornado is likely on the ground",
                true,
            ),
            (
                T::StormReports,
                "Severe",
                "Storm reports (LSR)",
                "What people on the ground actually reported today",
                true,
            ),
            (
                T::AlertPanel,
                "Severe",
                "Active alerts list",
                "Every alert in view, worst first (the sidebar's Alerts tab)",
                true,
            ),
            (
                T::Tracks,
                "Severe",
                "Projected storm tracks (SCIT)",
                "Where each tracked storm is projected to go",
                false,
            ),
            (
                T::ArrivalCones,
                "Severe",
                "Arrival-time cones",
                "When a storm is expected to reach points downstream",
                false,
            ),
            (
                T::Nowcast,
                "Severe",
                "Nowcast (echo extrapolation)",
                "Short-range radar forecast by sliding echoes forward",
                false,
            ),
            (
                T::LocalTracks,
                "Severe",
                "Local cell tracks (radar-derived)",
                "Cell motion computed here from reflectivity, with 15- and 30-minute \
                 extrapolation \u{2014} for sites and networks with no Level 3 storm-cell table. \
                 Needs a few volumes in the loop before it has motion to show.",
                false,
            ),
            (
                T::Watches,
                "Severe",
                "Watch boxes",
                "SPC tornado and severe thunderstorm watches in effect",
                false,
            ),
            (
                T::Mds,
                "Severe",
                "Mesoscale discussions",
                "SPC's notes on where watches may be issued next",
                false,
            ),
            (
                T::ProbSevere,
                "Severe",
                "Severe probability (ProbSevere)",
                "Per-storm probability of severe weather, from NOAA/CIMSS",
                false,
            ),
            (
                T::Metar,
                "Obs",
                "Surface obs (METAR)",
                "Temperature, dewpoint and wind at airports",
                true,
            ),
            (
                T::Webcams,
                "Obs",
                "Webcams (FAA + Windy)",
                "Look at the sky through a real camera \u{2014} FAA airports, plus the Windy \
                 network worldwide with a key in Settings",
                false,
            ),
            (
                T::Fires,
                "Severe",
                "Wildfires (WFIGS)",
                "Active fire perimeters and incident points from the interagency fire feed",
                false,
            ),
            (
                T::Aqi,
                "Obs",
                "Air quality (AirNow)",
                "EPA AQI at every monitor in view \u{2014} needs a free AirNow key in Settings",
                false,
            ),
            (
                T::Stations,
                "Obs",
                "Live station cards",
                "Cameras and live telemetry from surface stations, one floating card each",
                false,
            ),
            (
                T::Dat,
                "Obs",
                "Damage surveys (NWS DAT)",
                "What the survey crews found on the ground, rated point by point",
                false,
            ),
            (
                T::Spotters,
                "Obs",
                "Spotter Network",
                "Live positions of storm spotters near the radar",
                true,
            ),
            (
                T::Gauges,
                "Obs",
                "River gauges (NWPS)",
                "River levels and flood stage",
                false,
            ),
            (
                T::Sensors,
                "Obs",
                "Sensor dashboard",
                "Current conditions and 24-hour trends at the nearest station",
                false,
            ),
            (
                T::Hodo,
                "Obs",
                "Wind with height (VAD hodograph)",
                "How the wind turns with height above the radar",
                false,
            ),
            (
                T::Tropical,
                "Obs",
                "Tropical (NHC)",
                "Hurricane tracks and forecast cones",
                false,
            ),
            (
                T::Outages,
                "Severe",
                "Power outages (ODIN)",
                "Customers without power by county, from DOE/ORNL \u{2014} participating \
                 utilities only, so a blank county may just be an unreporting one",
                false,
            ),
            (
                T::Mping,
                "Obs",
                "Crowd reports (mPING)",
                "What people outside say is falling: rain, snow, sleet, freezing rain",
                false,
            ),
            (
                T::Pireps,
                "Obs",
                "Pilot reports (PIREPs)",
                "What pilots actually flew through: turbulence, icing, cloud tops",
                false,
            ),
            (
                T::Recon,
                "Obs",
                "Recon flight track",
                "Hurricane-hunter observations: flight-level and surface wind, measured",
                false,
            ),
            (
                T::Aviation,
                "Obs",
                "Aviation (SIGMET/AIRMET)",
                "Hazard areas for pilots: turbulence, icing, low ceilings",
                false,
            ),
            (
                T::Tfr,
                "Reference",
                "Flight restrictions (TFR)",
                "Airspace you may not fly through: fires, stadiums, VIP movements, launches",
                false,
            ),
            (
                T::Blockage,
                "Reference",
                "Beam blockage (terrain)",
                "Shade where terrain cuts into this tilt's beam \u{2014} the radar's blind spots",
                false,
            ),
            (
                T::RadarSites,
                "Reference",
                "Radar sites",
                "Show every NEXRAD site; click one to switch radars",
                true,
            ),
            (
                T::Wind,
                "Models",
                "Wind (animated)",
                "HRRR 10 m wind as drifting particles \u{2014} forecast output, CONUS only",
                true,
            ),
            (
                T::GlmLightning,
                "Severe",
                "Satellite lightning (GLM)",
                "Total lightning (optical) — every flash the GOES mapper sees, in-cloud included, \
                 fading as it ages. The CG density layer is the ground-strike half.",
                true,
            ),
            (
                T::Strikes,
                "Severe",
                "Lightning strikes (MQTT)",
                "Ground strikes republished onto your own MQTT broker \u{2014} needs a relay or a \
                 Home Assistant rebroadcast and the strikes topic set in Settings. Nothing is \
                 fetched from a strike network by the app itself.",
                true,
            ),
            (
                T::Fronts,
                "Reference",
                "Surface fronts (H/L)",
                "The cold, warm and stationary fronts from the national weather map",
                true,
            ),
            (
                T::RangeRings,
                "Reference",
                "Range rings",
                "Distance rings around the radar, every 50 km",
                true,
            ),
            (
                T::LinkCameras,
                "Reference",
                "Link pane cameras",
                "Pan and zoom every pane together",
                false,
            ),
            (
                T::LinkTimes,
                "Reference",
                "Link pane analysis time",
                "Radar scrubs and GOES follow-mode steps share one selected time; each NEXRAD pane seeks its nearest scan, while live panes follow their own feeds",
                false,
            ),
            (
                T::MiniLoop,
                "Reference",
                "Mini loop window",
                // Honest about the caveat rather than promising something the compositor will
                // not do — see `mini_loop_viewport`.
                if cfg!(unix) && std::env::var_os("WAYLAND_DISPLAY").is_some() {
                    "A small window showing the active pane (Wayland cannot keep it on top)"
                } else {
                    "A small always-on-top window showing the active pane"
                },
                false,
            ),
        ] {
            let on = *self.overlay_flag(t);
            push(
                label,
                category,
                desc,
                common,
                PaletteAction::ToggleOverlay(t),
                Some(on),
            );
        }
        push(
            "Cycle basemap",
            "Reference",
            "Switch the map underneath the radar",
            true,
            PaletteAction::CycleBasemap,
            None,
        );
        push(
            "Open this view in Windy",
            "Reference",
            "Open windy.com in your browser, looking at the same place",
            true,
            PaletteAction::OpenInWindy,
            None,
        );
        push(
            "Copy link to this view",
            "Reference",
            "A link to this site, place, zoom, time and product \u{2014} opens HookEcho here",
            true,
            PaletteAction::CopyViewLink,
            None,
        );
        push(
            "Save workspace",
            "Reference",
            "Remember this pane layout \u{2014} sites, products, tilts, overlays \u{2014} to restore later",
            true,
            PaletteAction::SaveWorkspace,
            None,
        );
        for (i, ws) in self.settings.workspaces.iter().enumerate() {
            push(
                &format!("Workspace: {}", ws.name),
                "Reference",
                "Restore this saved pane layout",
                true,
                PaletteAction::ApplyWorkspace(i),
                None,
            );
        }
        push(
            "Mute audio alerts",
            // "Tools", not "Alerts": there is no Alerts group in the panel, and a row filed under
            // one draws nowhere — reachable from Ctrl+K and invisible everywhere else. The debug
            // assertion in `push` caught it; a debug build panicked at startup.
            "Tools",
            "Silence every chime and spoken warning without changing your sound choices",
            true,
            PaletteAction::ToggleMute,
            Some(self.settings.mute_alerts),
        );
        push(
            "Layers panel",
            "Reference",
            "The floating panel \u{2014} close it and nothing covers the map",
            false,
            PaletteAction::TogglePanel,
            Some(self.panel_open),
        );
        push(
            "About HookEcho",
            "Reference",
            "Version, links, and whether a newer release is out",
            false,
            PaletteAction::OpenWindow(AppWindow::About),
            None,
        );

        // --- Tools, windows, panes. ---
        let tool = self.tool;
        for (t, label, desc, common) in [
            (
                MapTool::Interrogate,
                "Tool: Explore",
                "Inspect storm cells, alerts, markers, and other map features",
                true,
            ),
            (
                MapTool::GateInspector,
                "Tool: Gate inspector",
                "Click the radar to read the exact gate value and geometry",
                true,
            ),
            (
                MapTool::Measure,
                "Tool: Measure",
                "Drag to measure distance and bearing",
                true,
            ),
            (
                MapTool::Marker,
                "Tool: Drop marker",
                "Save a place — home, work, where you're headed",
                true,
            ),
            (
                MapTool::Forecast,
                "Tool: Point forecast",
                "Tap anywhere for that spot's 7-day and hourly forecast",
                true,
            ),
            (
                MapTool::Sounding,
                "Tool: Sounding",
                "Click a point for the model profile plus the nearest balloon sounding",
                false,
            ),
            (
                MapTool::CrossSection,
                "Tool: Cross-section",
                "Drag a line to slice the storm vertically",
                false,
            ),
            (
                MapTool::Chase,
                "Tool: Set chase location",
                "Tell the app where you are, for the chase readout",
                false,
            ),
            (
                MapTool::Climatology,
                "Tool: Tornado climatology",
                "How often tornadoes have hit this spot historically",
                false,
            ),
            (
                MapTool::AlertZone,
                "Tool: Watch zone",
                "Draw an area that alerts when a warning polygon touches it",
                false,
            ),
            (
                MapTool::Draw,
                "Tool: Draw",
                "Scribble on the map — circle the storm you're talking about",
                false,
            ),
        ] {
            push(
                label,
                "Tools",
                desc,
                common,
                PaletteAction::Tool(t),
                Some(tool == t),
            );
        }
        for (w, label, desc, common) in [
            (
                W::Site,
                "Radar site…",
                "Pick which radar you're watching",
                true,
            ),
            (
                W::Settings,
                "Settings…",
                "Theme, units, time display, alert sounds",
                true,
            ),
            (
                W::Markers,
                "Location markers…",
                "Manage your saved places and their alerts",
                false,
            ),
            (
                W::Events,
                "Event library…",
                "Jump to a famous storm and watch it replay",
                false,
            ),
            (
                W::ChaseReplay,
                "Chase replay\u{2026}",
                "Drive a recorded chase again, with the radar as it was",
                false,
            ),
            (
                W::Digest,
                "Storm digest…",
                "A plain-language summary of what's happening now",
                false,
            ),
            (
                W::Afd,
                "Forecast discussion (AFD)…",
                "What the local forecast office is writing",
                false,
            ),
            (
                W::Placefiles,
                "Placefile manager…",
                "Add GRLevelX placefile overlays",
                false,
            ),
            (
                W::LayerManager,
                "Layer manager…",
                "Reorder and set opacity for every layer",
                false,
            ),
            (
                W::Palettes,
                "Color-table editor…",
                "Change the colors a product is drawn with",
                false,
            ),
            (
                W::StormTable,
                "Storm attributes…",
                "Every tracked storm in one sortable table \u{2014} hail size, tops, rotation",
                true,
            ),
            (
                W::UdpProducts,
                "User-defined products…",
                "Write your own formula from REF/VEL/ZDR/etc.; see it evaluated live in the gate inspector",
                false,
            ),
            (
                W::AlertRules,
                "Alert rules\u{2026}",
                "Tell the app what is worth interrupting you for",
                false,
            ),
            (
                W::Help,
                "Help \u{2014} shortcuts, glossary, tour\u{2026}",
                "What a TDS, a hail spike or a ZDR column actually is, and every keyboard shortcut",
                false,
            ),
            (
                W::Verify,
                "Warning verification…",
                "Score an office's warnings against what actually happened",
                false,
            ),
            (
                W::Cappi,
                "Constant-height slice (CAPPI)…",
                "See the storm at one constant altitude",
                false,
            ),
            (
                W::Volume3d,
                "3D volume…",
                "Rotate the storm in three dimensions",
                false,
            ),
            (
                W::Climatology,
                "Tornado climatology…",
                "Historical tornado tracks for this area",
                false,
            ),
            (
                W::Setup,
                "Set up again…",
                "Pick your home radar again",
                false,
            ),
            (
                W::Tour,
                "Take the tour…",
                "A 60-second walk through the app's controls",
                false,
            ),
        ] {
            // The raymarch samples a `texture_3d<u32>`, which WebGL2 does not guarantee; on wasm
            // the entry would open a black window.
            // ponytail: revisit when the web build is webgpu-only.
            if cfg!(target_arch = "wasm32") && w == W::Volume3d {
                continue;
            }
            let on = None;
            push(
                label,
                "Tools",
                desc,
                common,
                PaletteAction::OpenWindow(w),
                on,
            );
        }
        push(
            "Compare 4 tilts",
            "Tools",
            "Four panes of this product at four heights, cameras linked",
            true,
            PaletteAction::AllTilts,
            None,
        );
        push(
            "Compare models in 2 panes",
            "Models",
            "One model's own field in each pane, cameras linked, instead of subtracting them",
            false,
            PaletteAction::CompareInPanes,
            None,
        );
        let panes = self.views.len();
        for n in [1usize, 2, 4] {
            push(
                &format!("{n} pane{}", if n == 1 { "" } else { "s" }),
                "Tools",
                "Split the window to watch several radars or products at once",
                false,
                PaletteAction::SetPanes(n),
                Some(panes == n),
            );
        }
        push(
            "Reload",
            "Tools",
            "Fetch the latest data again",
            true,
            PaletteAction::Reload,
            None,
        );
        push(
            "Jump to live",
            "Tools",
            "Snap back to the newest scan",
            true,
            PaletteAction::GoLive,
            None,
        );
        push(
            "Instant replay (DVR)",
            "Tools",
            "Replay the scans already in memory",
            false,
            PaletteAction::InstantReplay,
            None,
        );
        for entry in &mut out {
            if entry.on == Some(true) {
                entry.health = self.palette_health(entry.action);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_time_detail, field_layer_is_health_tracked, ingest_lag_detail, retry_detail};

    /// Every MRMS catalog product must be health-tracked without being named here — that is the
    /// whole point of checking `descriptor().is_some()` first. A product added to the catalog
    /// with no matching entry in this test would still pass it, which is the intended shape: the
    /// catalog is the source of truth, not this list.
    #[test]
    fn every_catalog_product_is_health_tracked() {
        for product in wxdata::mrms::catalog::PRODUCTS {
            let layer = crate::render::FieldLayer::from_slug(product.field.id.0)
                .unwrap_or_else(|| panic!("catalog product {} has no FieldLayer slug", product.field.id.0));
            assert!(
                field_layer_is_health_tracked(layer),
                "{} (catalog product {}) should be health-tracked",
                layer.slug(),
                product.field.id.0
            );
        }
    }

    /// A layer computed locally from the volume already on screen — no network request of its
    /// own — must not claim a health lane nothing ever fills in, which would just show
    /// "Waiting" forever instead of the honest "no network state" of `None`.
    #[test]
    fn a_locally_derived_layer_is_not_health_tracked() {
        assert!(!field_layer_is_health_tracked(
            crate::render::FieldLayer::CompositeLocal
        ));
    }

    #[test]
    fn no_live_arrival_yet_means_no_lag_detail() {
        assert!(ingest_lag_detail(None).is_none());
    }

    /// Received 41 seconds after the volume's own valid time — a plausible provider/network
    /// delay, not a clock skew edge case.
    #[test]
    fn lag_is_receipt_minus_valid_time() {
        let valid = chrono::DateTime::from_timestamp(1_000, 0).unwrap();
        let received = valid + chrono::Duration::seconds(41);
        let (label, value) = ingest_lag_detail(Some((received, valid))).unwrap();
        assert_eq!(label, "Provider lag");
        assert_eq!(value, "41s");
    }

    /// A clock skewed slightly ahead of the radar's own must not show a negative lag — that
    /// reads as nonsense ("-3s") rather than the honest "about zero" it actually is.
    #[test]
    fn a_negative_lag_from_clock_skew_clamps_to_zero() {
        let valid = chrono::DateTime::from_timestamp(1_000, 0).unwrap();
        let received = valid - chrono::Duration::seconds(3);
        let (_, value) = ingest_lag_detail(Some((received, valid))).unwrap();
        assert_eq!(value, "0s");
    }

    #[test]
    fn a_healthy_stream_with_no_retries_earns_no_detail_line() {
        assert!(retry_detail(0).is_none());
    }

    #[test]
    fn one_retry_is_singular_more_are_plural() {
        let (label, value) = retry_detail(1).unwrap();
        assert_eq!(label, "Stream retries");
        assert_eq!(value, "1 chunk fetch retry");
        let (_, value) = retry_detail(3).unwrap();
        assert_eq!(value, "3 chunk fetch retries");
    }

    #[test]
    fn no_live_stream_update_yet_means_no_decode_detail() {
        assert!(decode_time_detail(None).is_none());
    }

    /// Sub-second decode times are the normal case and need millisecond precision — `humanize`'s
    /// whole-second rounding would show "0s" for all of them, which is why this has its own
    /// formatter rather than reusing that one.
    #[test]
    fn a_sub_second_decode_shows_milliseconds() {
        let (label, value) =
            decode_time_detail(Some(std::time::Duration::from_millis(120))).unwrap();
        assert_eq!(label, "Decode time");
        assert_eq!(value, "120ms");
    }

    #[test]
    fn a_decode_at_or_past_one_second_shows_tenths() {
        let (_, value) =
            decode_time_detail(Some(std::time::Duration::from_millis(1_500))).unwrap();
        assert_eq!(value, "1.5s");
    }
}
