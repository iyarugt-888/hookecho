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
                | FL::Ensemble
                | FL::RtmaTemp2m
                | FL::RtmaDewpoint2m
                | FL::RtmaWind10m
                | FL::RtmaGust10m
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
            format!(
                "{retries} chunk fetch retr{}",
                if retries == 1 { "y" } else { "ies" }
            ),
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

fn render_queue_detail(micros: u64) -> Option<(&'static str, String)> {
    (micros > 0).then(|| {
        (
            "Render queue",
            format_millis(std::time::Duration::from_micros(micros)),
        )
    })
}

/// ROADMAP_NEW B6.9's source-health additions: which of the three failover tiers is active, the
/// standby side's own freshness, and the last transition — when this pane has a
/// `radar_provider_manager::SiteProviders` running for its site. Appended to `radar_health()`'s
/// `details` rather than replacing the B3 latency lines above, which describe the *active*
/// provider's own performance regardless of which one that is.
#[cfg(not(target_arch = "wasm32"))]
fn failover_details(
    snap: &crate::radar_provider_manager::FailoverSnapshot,
) -> Vec<(&'static str, String)> {
    use crate::radar_provider_manager::SelectedTier;
    let mut out = vec![(
        "Active provider",
        crate::radar_provider_manager::label_for_tier(snap.selected).to_string(),
    )];
    let state = if snap.manual_override {
        "MANUAL"
    } else {
        match snap.selected {
            SelectedTier::Primary => "PRIMARY",
            SelectedTier::Backup => "BACKUP",
            SelectedTier::Degraded => "DEGRADED_VOLUME",
        }
    };
    out.push(("Failover state", state.to_string()));
    out.push((
        "Standby provider",
        if snap.has_backup {
            let standby = match snap.selected {
                SelectedTier::Primary => snap.backup.as_ref(),
                _ => snap.primary.as_ref(),
            };
            standby
                .map(standby_freshness_detail)
                .unwrap_or_else(|| "not yet reporting".to_string())
        } else {
            "no relay configured".to_string()
        },
    ));
    if let Some((at, reason, tier)) = snap.last_transition {
        let ago = (chrono::Utc::now() - at).num_seconds().max(0);
        out.push((
            "Last transition",
            format!(
                "{} ({}), {} ago",
                crate::radar_provider_manager::label_for_tier(tier),
                switch_reason_label(reason),
                humanize(ago)
            ),
        ));
    }
    out
}

#[cfg(not(target_arch = "wasm32"))]
fn standby_freshness_detail(h: &crate::provider_health::ProviderHealth) -> String {
    match h.newest_radar_time {
        Some(t) => {
            let age = (chrono::Utc::now() - t).num_seconds().max(0);
            format!("{} ({} behind)", h.label, humanize(age))
        }
        None => format!("{} (no data yet)", h.label),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn switch_reason_label(reason: wxdata::live_block::ProviderSwitchReason) -> &'static str {
    use wxdata::live_block::ProviderSwitchReason as R;
    match reason {
        R::TransportError => "transport error",
        R::StaleData => "stale data",
        R::SequenceGap => "sequence gap",
        R::ManualOverride => "manual override",
        R::Recovery => "recovery",
    }
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
        let age = v
            .timeline
            .newest()
            .and_then(|id| id.date_time())
            .map(|t| (chrono::Utc::now() - t).to_std().unwrap_or_default());
        // Phase B3's provider-ingest-lag reading; never set from an archive scrub or a loop's
        // replayed frame, only a genuine live arrival — see `last_live_arrival`'s own doc comment.
        let base_details: Vec<(&'static str, String)> = [
            ingest_lag_detail(v.last_live_arrival),
            decode_time_detail(v.last_decode_time),
            render_queue_detail(
                v.live_gpu_queue_micros
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
            retry_detail(v.live_retries),
        ]
        .into_iter()
        .flatten()
        .collect();
        #[cfg(not(target_arch = "wasm32"))]
        let (details, fallback_providers) = match v.radar_providers.as_ref() {
            Some(providers) => {
                let snapshot = providers.snapshot();
                let fallback_providers = snapshot
                    .alternate_provider_labels()
                    .into_iter()
                    .map(str::to_string)
                    .collect();
                let mut details = base_details;
                details.extend(failover_details(&snapshot));
                (details, fallback_providers)
            }
            None => (base_details, Vec::new()),
        };
        #[cfg(target_arch = "wasm32")]
        let (details, fallback_providers) = (base_details, Vec::<String>::new());
        SourceHealth {
            source: v
                .site
                .as_deref()
                .map_or_else(|| "Radar".to_string(), |site| format!("{site} radar")),
            endpoint_family: crate::source_health::EndpointFamily::RadarLevel2,
            latest_valid_time: v.timeline.newest().and_then(|id| id.date_time()),
            fallback_providers,
            cache_state: if v.volume.is_some() {
                CacheState::Memory
            } else {
                CacheState::Empty
            },
            fetching: v.loading,
            last_attempt: v.last_poll.map(|t| t.elapsed()),
            last_success: age,
            last_failure: v.error.as_ref().map(|_| std::time::Duration::ZERO),
            error: v.error.clone(),
            // Shared with the scrubber's own Live/Stale badge (`RADAR_FRESH_SECS`) so the two
            // can never disagree about what counts as fresh — see that constant's doc comment.
            cadence: std::time::Duration::from_secs(RADAR_FRESH_SECS as u64),
            // Radar's health is built from `MapView` fields directly, not `RequestBook`, so
            // there is no rolling outcome history to report here — see `recent_outcomes`'s own
            // doc comment.
            recent_outcomes: None,
            details,
        }
    }

    fn palette_health(&self, action: PaletteAction) -> Option<SourceHealth> {
        use crate::render::FieldLayer as FL;
        use OverlayToggle as T;
        let lane = match action {
            PaletteAction::SetMoment(..) => return Some(self.radar_health()),
            PaletteAction::SetContours(k) if k != ContourKind::Off => {
                RequestLane::Feed(FeedSource::ModelContours)
            }
            PaletteAction::ToggleModelProduct(product)
                if field_layer_is_health_tracked(product.layer()) =>
            {
                RequestLane::Field(product.layer())
            }
            PaletteAction::ToggleField(layer) if field_layer_is_health_tracked(layer) => {
                // Compare's one fetch feeds both layers at once and is filed under CompareA
                // (see `OverlaySource::Compare`'s own `lane()`); ask for that lane regardless of
                // which of the two the toggle is for, so CompareB's health isn't perpetually
                // "unknown" just because nothing was ever filed under its own name.
                let layer = if layer == FL::CompareB {
                    FL::CompareA
                } else {
                    layer
                };
                RequestLane::Field(layer)
            }
            PaletteAction::ToggleOverlay(toggle) => match toggle {
                T::AlertPanel | T::Alerts => RequestLane::Feed(FeedSource::WeatherAlerts),
                T::StormReports => RequestLane::Feed(FeedSource::StormReports),
                T::Spotters => RequestLane::Feed(FeedSource::SpotterNetwork),
                T::Metar => RequestLane::Feed(FeedSource::SurfaceObservations),
                T::Webcams => RequestLane::Feed(FeedSource::Webcams),
                T::Fires => RequestLane::Feed(FeedSource::Wildfires),
                T::Aqi => RequestLane::Feed(FeedSource::AirQuality),
                T::Stations => RequestLane::Feed(FeedSource::LiveStations),
                T::Dat => RequestLane::Feed(FeedSource::DamageSurveys),
                T::Gauges => RequestLane::Feed(FeedSource::RiverGauges),
                T::Tropical => RequestLane::Feed(FeedSource::TropicalCyclones),
                T::Outages => RequestLane::Feed(FeedSource::PowerOutages),
                T::ProbSevere => RequestLane::Feed(FeedSource::ProbSevere),
                T::Aviation => RequestLane::Feed(FeedSource::AviationAdvisories),
                T::Tfr => RequestLane::Feed(FeedSource::TemporaryFlightRestrictions),
                T::Sensors => RequestLane::Feed(FeedSource::RadarObservations),
                T::Hodo => RequestLane::Feed(FeedSource::VadProfile),
                T::Cells | T::Tracks | T::ArrivalCones => RequestLane::Feed(FeedSource::StormCells),
                T::Mds => RequestLane::Feed(FeedSource::MesoscaleDiscussions),
                T::Mping => RequestLane::Feed(FeedSource::MpingReports),
                T::Pireps => RequestLane::Feed(FeedSource::PilotReports),
                T::Recon => RequestLane::Feed(FeedSource::HurricaneReconnaissance),
                T::Fronts => RequestLane::Feed(FeedSource::SurfaceAnalysis),
                T::Watches => RequestLane::Feed(FeedSource::WatchBoxes),
                T::Wind => RequestLane::Feed(FeedSource::WindParticles),
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

        // --- Radar sites: type a station id or a city and switch straight to it. ---
        //
        // `common: false` — with ~200 of these across four networks, showing them all in the
        // default (unsearched) list would bury everything else; they still surface the moment
        // a query matches one. `label` carries the id and the city/state a query might name (the
        // panel's own search only reads `label`, not `desc` — see `layers_panel::matches`), so
        // `desc` is one shared, static hint instead of a per-site format!() this struct's field
        // type couldn't hold anyway.
        let cur_site = self.views[self.active].site.as_deref();
        for s in wxdata::sites::all() {
            push(
                &format!("{} \u{2014} {}, {}", s.id, s.city, s.state),
                "Sites",
                "Switch this pane to this radar",
                false,
                PaletteAction::SetSite(crate::app::encode_site_id(s.id)),
                Some(cur_site == Some(s.id)),
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
                FL::GoesMidWaterVapor,
                "National",
                "GOES mid-level water vapor",
                "Mid-tropospheric moisture (Band 9) — pairs with the upper- and lower-level \
                 water vapor channels for the full three-level loop",
                false,
            ),
            (
                FL::GoesLowWaterVapor,
                "National",
                "GOES low-level water vapor",
                "Lower-tropospheric moisture (Band 10) — the water vapor channel most sensitive \
                 to boundary-layer moisture the upper two can't see",
                false,
            ),
            (
                FL::GoesShortwaveIr,
                "National",
                "GOES shortwave IR (fire detection)",
                "Band 7 — a sub-pixel fire raises this channel's brightness temperature far \
                 above anything a cloud or clear sky reaches, day or night",
                false,
            ),
            (
                FL::GoesDirtyIr,
                "National",
                "GOES split-window IR",
                "Band 15 — reads like clean IR on its own; the other half of the split-window \
                 dust/ash detection technique",
                false,
            ),
            (
                FL::GoesDustDiff,
                "National",
                "GOES dust/ash detection",
                "Split-window technique (Band 13 minus Band 15) — highlights airborne dust and \
                 volcanic ash the way an ordinary IR or visible loop can't",
                false,
            ),
            (
                FL::GoesColdTop,
                "National",
                "GOES cold cloud tops",
                "Highlights cloud tops colder than -63\u{b0}C (210 K) — a spotting aid for \
                 overshooting tops and rapidly intensifying convection",
                false,
            ),
            (
                FL::GoesCoolingRate,
                "National",
                "GOES cooling rate",
                "Band 13 brightness temperature 15 minutes ago minus now — a rapidly cooling \
                 cloud top can flag an intensifying updraft a single IR frame can't show",
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
                "Largest hail this storm can be making, from the volume aloft and that day's melting level",
                false,
            ),
            (
                FL::HailPosh,
                "Radar",
                "Severe-hail chance (POSH, derived)",
                "Odds this storm is producing hail three-quarters of an inch or larger",
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
                FL::Ensemble,
                "Models",
                "GEFS ensemble",
                "What 31 forecast runs say together: mean, spread, percentiles or the chance of crossing a threshold \u{2014} pick in layer options",
                false,
            ),
            (
                FL::SnowAnalysis,
                "National",
                "Snowfall analysis",
                "How much snow actually fell \u{2014} pick the window in layer options",
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
        // The model browser's products: one row each, whichever model is picked. Reflectivity is
        // one of them, offered by every model that publishes it, not a layer of its own.
        for product in crate::model_browser::Product::ALL {
            use crate::model_browser::Product as P;
            let on = self.views[self.active].fields_on.contains(&product.layer());
            push(
                product.row_label(),
                "Models",
                product.blurb(),
                matches!(product, P::Reflectivity | P::UpdraftHelicity),
                PaletteAction::ToggleModelProduct(product),
                Some(on),
            );
        }
        // Each real kind toggles independently — several can be active at once (e.g. MSLP and
        // CAPE together) — while "Off" is the one exclusive action, clearing every active kind.
        for k in ContourKind::ALL {
            let label = format!("Contours: {}", k.display_label(self.settings.temp_unit));
            let on = if k == ContourKind::Off {
                self.active_contours.is_empty()
            } else {
                self.active_contours.contains(&k)
            };
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
                T::Trail,
                "Severe",
                "Max/min trail (temporal extrema)",
                "Replace the radar with the strongest (or weakest) value each gate held over the \
                 last 15\u{2013}120 minutes of cached volumes \u{2014} a rotation, hail or \
                 reflectivity-core path. Pick the product and tilt as usual; the trail is built \
                 from volumes already in the loop.",
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
                T::LowestTilt,
                "Reference",
                "Lowest usable tilt (terrain)",
                "Shade by which tilt is the lowest that actually clears the terrain here \u{2014} \
                 green needs a low angle, red needs a high one",
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
                T::ScanAge,
                "Reference",
                "Scan-age ring",
                "A ring at the edge of the sweep, green where the data is newest and red where \
                 it was collected longest before, so you can see which side of the picture is \
                 a rotation old",
                false,
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
                T::LockSourceTime,
                "Reference",
                "Lock analysis to radar frame",
                "After a linked seek, make the active radar's exact scan time the analysis time so satellite and MRMS align to that source frame",
                false,
            ),
            (
                T::LinkSite,
                "Reference",
                "Link pane radar site",
                "Picking a new site in one pane sets it in every pane; each keeps its own product and tilt",
                false,
            ),
            (
                T::LinkCursor,
                "Reference",
                "Link pane crosshair",
                "Hovering one pane shows the same point on every other pane, plus a compact probe table of each pane's value there",
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
            (
                T::ImportedGis,
                "Reference",
                "Imported GIS shapes",
                "Polygons, lines, and points from the GeoJSON or Shapefile you imported \u{2014} \
                 style the layer in Layer Manager",
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
        let workstation = self.workstation_chrome();
        push(
            "Layers panel",
            "Reference",
            "The floating panel \u{2014} close it and nothing covers the map",
            false,
            PaletteAction::TogglePanel,
            Some(if workstation {
                self.dock.shown(super::DockWin::Layers)
            } else {
                self.panel_open
            }),
        );
        // The workstation's other tool windows. The 3D view and the Analyst log come with 3D and
        // Analyst Mode, which have their own switches, so they are not rows of their own.
        if workstation {
            use super::DockWin as D;
            for (w, label, desc) in [
                (
                    D::Inspector,
                    "Inspector window",
                    "The reading under the pointer, the product's provenance and the 3D view's state",
                ),
                (
                    D::Alerts,
                    "Alerts window",
                    "The warnings, watches and advisories in view",
                ),
                (
                    D::Sources,
                    "Sources window",
                    "Every active data feed's health, worst first",
                ),
                (
                    D::Prefs,
                    "Preferences window",
                    "Map, display and app settings",
                ),
            ] {
                push(
                    label,
                    "Reference",
                    desc,
                    false,
                    PaletteAction::DockWindow(w),
                    Some(self.dock.shown(w)),
                );
            }
        }
        push(
            "Top bar",
            "Reference",
            "The top bars (the ribbon, or the workstation's app bar and toolbar) \u{2014} hide them for a full-window map view",
            false,
            PaletteAction::ToggleRibbon,
            Some(!self.ribbon_collapsed),
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
                MapTool::RadarSuitability,
                "Tool: Radar suitability",
                "Click a point to rank nearby radars by beam height there, not just distance",
                false,
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
                MapTool::RegionStats,
                "Tool: Region statistics",
                "Box a storm for every moment's spread, histograms and scatter plots, and CSV",
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
                W::Tropical,
                "Tropical models & advisories…",
                "Hurricane model tracks (spaghetti), intensity guidance, invests, and NHC advisories",
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
                W::ModelVerify,
                "Model verification…",
                "Score a forecast run against the RTMA analysis: bias, error and hits by lead",
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
            (
                W::DataHealth,
                "Data source health…",
                "Every active source's status in one place — provider, freshness, backoff, last error",
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
        if self.diff_field.supports_side_by_side() {
            push(
                "Compare models in 2 panes",
                "Models",
                "One model's own field in each pane, cameras linked, instead of subtracting them",
                false,
                PaletteAction::CompareInPanes,
                None,
            );
            push(
                "Blink between compared models",
                "Models",
                "Alternate this pane between each model's own field on a timer, instead of two panes",
                false,
                PaletteAction::ToggleBlinkCompare,
                Some(self.views[self.active].blink_compare),
            );
            push(
                "Overlay compared models",
                "Models",
                "Draw model A normally with model B at 50% opacity in this pane",
                false,
                PaletteAction::ToggleCompareOverlay,
                Some(self.views[self.active].overlay_compare),
            );
            push(
                "Swipe between compared models",
                "Models",
                "Split this pane between model A and B with a draggable divider",
                false,
                PaletteAction::ToggleCompareSwipe,
                Some(self.views[self.active].swipe_compare),
            );
        }
        let panes = self.views.len();
        for n in [1usize, 2, 3, 4, 6, 9]
            .into_iter()
            .filter(|n| *n <= crate::view::MAX_PANES)
        {
            push(
                &format!("{n} pane{}", if n == 1 { "" } else { "s" }),
                "Tools",
                "Split the window to watch several radars or products at once",
                false,
                PaletteAction::SetPanes(n),
                Some(panes == n),
            );
        }
        for layout in crate::workspace::PaneLayout::ALL {
            push(
                &format!("{} pane layout", layout.label()),
                "Tools",
                layout.description(),
                false,
                PaletteAction::SetPaneLayout(layout),
                Some(self.pane_layout == layout),
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
        push(
            "Follow live sweep",
            "Radar",
            "While live, change tilt as each new sweep starts, showing the elevation being scanned",
            false,
            PaletteAction::ToggleFollowSweep,
            Some(self.views[self.active].follow_live_sweep),
        );
        push(
            "Follow lowest tilt",
            "Radar",
            "While live, jump to the lowest tilt each time it is rescanned (SAILS/MRLE)",
            false,
            PaletteAction::ToggleFollowLowest,
            Some(self.views[self.active].follow_lowest_cut),
        );
        push(
            "3D map view",
            "Tools",
            "Pitch this pane over into the map-pitch 3D view and back",
            true,
            PaletteAction::ToggleMap3d,
            Some(self.views[self.active].map_3d.enabled),
        );
        push(
            "Zoom to imported shapes",
            "Tools",
            "Frame the map on the GeoJSON file you imported, wherever it covers",
            false,
            PaletteAction::ZoomToGis,
            None,
        );
        push(
            "Export map as GeoJSON\u{2026}",
            "Tools",
            "Save what's drawn right now \u{2014} annotations, markers, watch zones, storm cells \
             and every displayed polygon \u{2014} for QGIS, ArcGIS or a briefing",
            false,
            PaletteAction::ExportGis,
            None,
        );
        push(
            "Import GIS file\u{2026}",
            "Tools",
            "Load GeoJSON or an ESRI Shapefile as a styled reference overlay; polygon attributes \
             remain clickable",
            false,
            PaletteAction::ImportGis,
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
    use super::{
        decode_time_detail, field_layer_is_health_tracked, ingest_lag_detail, render_queue_detail,
        retry_detail,
    };

    /// Every MRMS catalog product must be health-tracked without being named here — that is the
    /// whole point of checking `descriptor().is_some()` first. A product added to the catalog
    /// with no matching entry in this test would still pass it, which is the intended shape: the
    /// catalog is the source of truth, not this list.
    #[test]
    fn every_catalog_product_is_health_tracked() {
        for product in wxdata::mrms::catalog::PRODUCTS {
            let layer =
                crate::render::FieldLayer::from_slug(product.field.id.0).unwrap_or_else(|| {
                    panic!(
                        "catalog product {} has no FieldLayer slug",
                        product.field.id.0
                    )
                });
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
        let (_, value) = decode_time_detail(Some(std::time::Duration::from_millis(1_500))).unwrap();
        assert_eq!(value, "1.5s");
    }

    #[test]
    fn render_queue_detail_is_hidden_until_a_live_upload_commits() {
        assert!(render_queue_detail(0).is_none());
        let (label, value) = render_queue_detail(18_250).unwrap();
        assert_eq!(label, "Render queue");
        assert_eq!(value, "18ms");
    }
}
