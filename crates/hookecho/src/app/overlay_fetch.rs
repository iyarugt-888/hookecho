//! Background fetches: [`OverlaySource`] (what to fetch), [`OverlayMsg`] (what came back) and
//! [`OverlayDelivery`] (a reply with the request identity that decides whether it is still the
//! latest). Moved out of `app.rs` unchanged (ROADMAP_2 §7).

use super::*;

pub(crate) enum OverlayMsg {
    Alerts(Vec<GeoFeature>),
    /// Last run's alert overlay, read from disk off the launch path. Applied only if no live
    /// fetch has landed yet; it seeds the known-warning ids either way, so a restart mid-event
    /// doesn't re-banner and re-speak warnings already on the map.
    AlertSeed(Vec<GeoFeature>),
    /// WPC coded surface analysis (fronts + pressure centers).
    Fronts(wxdata::fronts::SurfaceAnalysis),
    Outlook(u8, Vec<GeoFeature>),
    Mds(Vec<GeoFeature>),
    Watches(Vec<GeoFeature>),
    /// WPC Winter Storm Severity Index polygons for a day.
    Wssi(u8, Vec<GeoFeature>),
    /// WPC Excessive Rainfall Outlook polygons for a day.
    Ero(u8, Vec<GeoFeature>),
    /// SPC Fire Weather Outlook polygons (risk + dry thunderstorm) for a day.
    FireWx(u8, Vec<GeoFeature>),
    /// mPING crowd precipitation-type reports.
    Mping(Vec<wxdata::mping::Report>),
    /// Pilot reports within the fetched bbox.
    Pireps(Vec<wxdata::aviation::Pirep>),
    /// Hurricane-hunter flight-track observations.
    Recon(Vec<wxdata::recon::HdobOb>),
    /// Storm cells for a specific site (dropped if the active site changed meanwhile), and when
    /// asked for, the cells of its earlier scans (oldest first) to start the trends with.
    Cells(String, Vec<Cell>, CellHistory),
    /// A fetched placefile keyed by its URL.
    Placefile(String, wxdata::placefile::Placefile),
    /// The latest grid for a national field layer (mosaic, rotation, MESH, AzShear, lightning).
    Field(crate::render::FieldLayer, wxdata::mrms::MrmsField),
    /// Atomic local build with its accepted frame identity and source coverage.
    DerivedFields(Box<radar_products::DerivedDelivery>),
    StampedField(
        crate::render::FieldLayer,
        wxdata::field::Stamped<wxdata::mrms::MrmsField>,
    ),
    MrmsField(
        crate::render::FieldLayer,
        wxdata::field::Stamped<wxdata::mrms::MrmsField>,
        MrmsRequest,
    ),
    /// A model-difference grid with exact shared valid time and both source runs.
    /// The signed difference, and the percent grid where the field offers one.
    ModelDiff(
        crate::fielddiff::DiffField,
        u16,
        wxdata::mrms::MrmsField,
        Option<wxdata::mrms::MrmsField>,
        crate::fielddiff::ComparisonTimes,
    ),
    /// Both sides of a comparison, unsubtracted, plus their shared valid time — the field is included
    /// so a selection change in flight is easy to detect as stale (see the handler).
    Compare(
        crate::fielddiff::DiffField,
        u16,
        wxdata::mrms::MrmsField,
        wxdata::mrms::MrmsField,
        crate::fielddiff::ComparisonTimes,
    ),
    /// Every GEFS member of one field at a forecast hour (ROADMAP_NEW F7). The statistic is
    /// computed from these in the handler, so changing it never refetches.
    Ensemble(
        wxdata::ensemble::EnsembleField,
        u16,
        Box<wxdata::ensemble::EnsembleRun>,
    ),
    /// `(0 °C, −20 °C)` level heights above sea level, in metres, at `site`'s radar — for the
    /// melting-level `epoch` they were requested for (see [`OverlaySource::FreezingLevels`]), so
    /// a reply that lands after the user moved to another site or time is filed under the one it
    /// actually answers.
    FreezingLevels {
        site: String,
        epoch: Option<chrono::DateTime<chrono::Utc>>,
        h0: f64,
        hm20: f64,
    },
    /// Local storm reports: live trailing window (`None`) or an archive bucket (feature CC).
    StormReports(Option<i64>, Vec<wxdata::spc::StormReport>),
    /// Live Spotter Network positions (CONUS-wide; filtered to the active site at draw time).
    Spotters(Vec<wxdata::spotters::Spotter>),
    /// ProbSevere per-storm probability polygons.
    ProbSevere(Vec<GeoFeature>),
    /// An HRRR composite-reflectivity forecast (regridded + run/valid metadata).
    Hrrr(wxdata::hrrr::HrrrForecast),
    /// HRRR wind components for the particle layer.
    ///
    /// Deliberately not an [`OverlayMsg::Field`]: `spawn_overlay` runs `decimated` on every
    /// `Field` message, and `decimated` **max-pools**. That is right for reflectivity and wrong
    /// for a signed vector component — it would bias u and v independently toward positive, i.e.
    /// a phantom northeasterly drift. Boxed because a pair of CONUS grids is ~11 MB and this enum
    /// is moved by value through the channel.
    Wind(Box<crate::wind_draw::WindField>),
    /// Nearest-station observations for a site (or an error string).
    Obs(String, Result<wxdata::obs::StationObs, String>),
    /// VAD wind profile for a site.
    Vwp(String, Vec<wxdata::level3::VwpLevel>),
    /// Archived storm-based warnings for a 5-min UTC bucket (feature W).
    ArchiveWarnings(i64, Vec<GeoFeature>),
    /// Mesoscale discussions in effect at a 5-minute bucket's time.
    ArchiveMds(i64, Vec<GeoFeature>),
    /// Surface observations (METAR station plots) for the requested bbox (feature U).
    Metar(
        Vec<wxdata::metar::SurfaceOb>,
        std::collections::HashMap<String, String>,
    ),
    /// FAA camera sites for the requested bbox.
    Webcams(Vec<wxdata::webcams::CamSite>),
    /// Wildfire perimeters + incident points for the requested bbox.
    Fires(Vec<GeoFeature>, Vec<wxdata::wfigs::FireIncident>),
    /// AirNow AQI observations for the requested bbox.
    Aqi(Vec<wxdata::airnow::AqiOb>),
    /// Live surface stations for the telemetry cards.
    Stations(Vec<wxdata::stations::StationOb>),
    /// The current PPEF electric-field table (ionospheric, mV/m).
    Ppef(wxdata::efield::Ppef),
    /// Highway cameras for the requested bbox.
    DotCams(Vec<wxdata::dotcams::DotCam>),
    /// Newest reading from a configured ground field mill (kV/m).
    Mill(f32),
    /// NWS damage-survey points and surveyed tracks for the requested bbox + storm day.
    Dat(Vec<wxdata::dat::DamagePoint>, Vec<wxdata::dat::DamageTrack>),
    /// A plugin or placefile that failed to load, with why (shown in the manager window).
    PlacefileError(String, String),
    /// A finished multi-radar reflectivity composite: the grid, its contributing sites, and the
    /// oldest contributing scan time.
    Mosaic(
        wxdata::mrms::MrmsField,
        Vec<String>,
        chrono::DateTime<chrono::Utc>,
    ),
    /// River flood gauges (NWPS) for the requested bbox.
    Gauges(Vec<wxdata::river::Gauge>),
    /// Where a GOES mesoscale sector is pointed now.
    GoesFootprint(wxdata::goes_abi::Sector, wxdata::goes_abi::Footprint),
    /// GLM flashes in the window before a past time.
    GlmWindow(DateTime<Utc>, Vec<wxdata::glm::Flash>),
    /// HRRR model contour polylines for a kind, plus the forecast valid time.
    Contours(
        ContourKind,
        Vec<wxdata::contour::ContourLine>,
        // The model run, then the valid time.
        DateTime<Utc>,
        DateTime<Utc>,
        // The grid the lines were drawn from, in display units, for the layer probe.
        Arc<wxdata::mrms::MrmsField>,
    ),
    /// NHC tropical cyclones: cones + per-storm tracks (feature V).
    Tropical(wxdata::tropical::TropicalData),
    /// County power outages from ODIN.
    Outages(Vec<overlay::GeoFeature>),
    /// Aviation SIGMET/AIRMET hazard polygons (feature GG).
    Aviation(Vec<GeoFeature>),
    /// Newly-fetched TFR shapes keyed by NOTAM id, and how many are still unfetched.
    Tfr(Vec<(String, GeoFeature)>, usize),
}

/// One overlay data source to fetch.
#[derive(Clone)]
pub(crate) enum OverlaySource {
    /// NWS alerts; the `(lat, lon)` list scopes zone-only alert resolution to the active radar and
    /// every saved marker. The bounds are the active pane's viewport, which is what decides
    /// whether European warnings are worth fetching alongside them.
    Alerts(Vec<(f64, f64)>, (f64, f64, f64, f64)),
    Mds,
    /// Tornado and severe thunderstorm watch polygons in effect.
    Watches,
    /// Winter Storm Severity Index for a day (1-3).
    Wssi(u8),
    /// Excessive Rainfall Outlook for a day (1-3).
    Ero(u8),
    /// SPC Fire Weather Outlook (categorical risk + dry thunderstorm) for a day (1-2).
    FireWx(u8),
    /// mPING crowd reports from the last hour, with the user's API key.
    Mping(String),
    /// Pilot reports within a lat/lon bbox `(lat0, lon0, lat1, lon1)`.
    Pireps(f64, f64, f64, f64),
    /// Hurricane-hunter HDOBs from the last few hours.
    Recon,
    Outlook(u8, wxdata::spc::OutlookKind),
    /// A site's storm cells; `true` also fetches its earlier scans ([`CELL_HISTORY_SCANS`]).
    Cells(String, bool),
    Placefile(String),
    /// A national field layer plus the MRMS S3 product path to fetch it from.
    Field(crate::render::FieldLayer, MrmsRequest),
    /// Local storm reports: live (`None`) or a 30-min archive bucket (Unix secs / 1800).
    StormReports(Option<i64>),
    Spotters,
    ProbSevere,
    /// WPC coded surface analysis (fronts + pressure centers).
    Fronts,
    /// Forecast reflectivity from a regional model at a whole forecast hour, from a pinned run
    /// (`None` = the newest that has posted).
    Hrrr(wxdata::hrrr::Model, u8, Option<DateTime<Utc>>),
    /// HRRR sub-hourly (`wrfsubhf`) composite reflectivity, forecast lead in minutes (15..=1080).
    HrrrSub(u16, Option<DateTime<Utc>>),
    /// CAPE or SRH from a regional model: `(layer, model, mixed-layer parcel, SRH depth km, hour,
    /// pinned run)`.
    Env(
        crate::render::FieldLayer,
        wxdata::hrrr::Model,
        bool,
        u8,
        u8,
        Option<DateTime<Utc>>,
    ),
    /// HRRR-backed field layer (rotation tracks, smoke) at a forecast hour, from a pinned run.
    HrrrLayer(crate::render::FieldLayer, u8, Option<DateTime<Utc>>),
    /// A global-model field (GFS or ECMWF) at a forecast hour, from a pinned run.
    Global(
        crate::render::FieldLayer,
        wxdata::global::GlobalModel,
        wxdata::global::GlobalField,
        u16,
        Option<DateTime<Utc>>,
    ),
    /// One model's field minus another's, at a forecast hour. Which two models is implied by the
    /// field (see `fielddiff::DiffField::pair`).
    ModelDiff(crate::fielddiff::DiffField, u16),
    /// Both models' own field, unsubtracted, at a forecast hour — for the compare-panes mode
    /// (`CompareA`/`CompareB`). Same two grids `ModelDiff` fetches, shown side by side instead of
    /// subtracted.
    Compare(crate::fielddiff::DiffField, u16),
    /// All GEFS members of a field at a forecast hour, for the ensemble layer.
    Ensemble(wxdata::ensemble::EnsembleField, u16),
    /// Gridded L3 product (DVL/EET) for a site, projected to a lat/lon field (feature X).
    L3Grid(crate::render::FieldLayer, String),
    /// Melting-level and −20 °C heights at `site`'s radar (`lon`, `lat`, `elev_m` above sea
    /// level), for the derived hail grids. `epoch: None` is the live HRRR analysis; `Some(t)` is
    /// the observed sounding at synoptic time `t`, for an archived volume — so a storm from 2013
    /// is integrated with 2013's melting level, not today's.
    FreezingLevels {
        site: String,
        lon: f64,
        lat: f64,
        elev_m: f64,
        epoch: Option<chrono::DateTime<chrono::Utc>>,
    },
    /// NOHRSC observed snowfall analysis over an accumulation window (hours).
    Snow(u16),
    /// Banded snow: the MRMS mosaic cut to elongated echo and masked to snow.
    SnowBands,
    /// A GOES ABI band, CONUS sector, read directly from S3 rather than GIBS' pre-rendered
    /// tiles — which band is `GoesIr`/`GoesVisible`/`GoesWaterVapor`/`GoesShortwaveIr`/
    /// `GoesMidWaterVapor`/`GoesLowWaterVapor`/`GoesDirtyIr`/`GoesLongwaveIr`/`GoesColdTop`, which satellite is
    /// the second field (`settings.goes_satellite_west`, resolved at spawn time). `GoesColdTop`
    /// reuses Band 13 but transforms the fetched value before it reaches the field cache — see
    /// the handler's own comment.
    Goes(
        crate::render::FieldLayer,
        wxdata::goes_abi::Satellite,
        wxdata::goes_abi::Sector,
        Option<DateTime<Utc>>,
    ),
    /// A two-band GOES ABI channel-difference product, CONUS sector — today only
    /// `FieldLayer::GoesDustDiff` (ROADMAP_NEW E6's split-window dust/ash technique, Band 13
    /// minus Band 15), but the variant carries the layer rather than being hardcoded so a second
    /// difference product only needs a new match arm, not a new `OverlaySource` case.
    GoesDiff(crate::render::FieldLayer, wxdata::goes_abi::Satellite),
    /// A GOES ABI same-band time-difference product, CONUS sector — today only
    /// `FieldLayer::GoesCoolingRate` (ROADMAP_NEW E6's cooling-rate/time-change product, Band 13
    /// 15 minutes ago minus now), same "carry the layer, not a hardcoded band" shape as
    /// `GoesDiff` above so a second lookback-window product only needs a new match arm.
    GoesCoolingRate(crate::render::FieldLayer, wxdata::goes_abi::Satellite),
    /// A GOES RGB composite (`FieldLayer::GoesRgb`): every band of the recipe from one scan,
    /// composed and packed into one grid (`wxdata::goes_rgb`).
    GoesRgb(
        &'static wxdata::goes_rgb::Recipe,
        wxdata::goes_abi::Satellite,
        wxdata::goes_abi::Sector,
        Option<DateTime<Utc>>,
    ),
    /// GLM flashes in the minutes before a past time, for a scrubbed-back view (ROADMAP_NEW E6).
    GlmWindow(DateTime<Utc>, bool),
    /// Where a GOES mesoscale sector is pointed now, or at a past time (ROADMAP_NEW E5).
    GoesFootprint(
        wxdata::goes_abi::Satellite,
        wxdata::goes_abi::Sector,
        Option<DateTime<Utc>>,
    ),
    /// An NDFD element (the NWS's own forecaster-blended grid), CONUS short range — which
    /// element is `NdfdTemp2m`/`NdfdWind10m`/`NdfdGust10m`/`NdfdSnow`.
    Ndfd(crate::render::FieldLayer),
    /// An RTMA analysis field for one analysis hour (`None` = the newest that has posted).
    Rtma(crate::render::FieldLayer, Option<DateTime<Utc>>),
    /// Nearest-station observations for `site` at `(lat, lon)`.
    Obs {
        site: String,
        lat: f64,
        lon: f64,
    },
    /// VAD wind profile for `site`.
    Vwp(String),
    /// Archived storm-based warnings valid at a 5-min UTC bucket (Unix seconds, feature W).
    ArchiveWarnings(i64),
    ArchiveMds(i64),
    /// Aviation SIGMET/AIRMET polygons (feature GG).
    Aviation,
    /// FAA Temporary Flight Restrictions; carries the NOTAM ids already held, so a refresh only
    /// fetches shapes that are new.
    Tfr(Vec<String>),
    /// Surface observations within a lat/lon bbox `(lat0, lon0, lat1, lon1)` (feature U).
    Metar(f64, f64, f64, f64),
    /// Run an external-process plugin: `(key, command, args, context)`. The key is the synthetic
    /// `plugin:<name>` id it shares with the placefile pipeline it feeds.
    #[cfg(not(target_arch = "wasm32"))]
    Plugin(String, String, Vec<String>, crate::plugins::Context),
    /// Camera sites within a lon/lat bbox `(min_lon, min_lat, max_lon, max_lat)`, plus the user's
    /// Windy API key — empty for FAA-only, which is the keyless default.
    Webcams(f64, f64, f64, f64, String),
    /// Wildfire perimeters + incidents within a lon/lat bbox `(west, south, east, north)`.
    Fires(f64, f64, f64, f64),
    /// AirNow AQI within a lon/lat bbox, plus the user's key (never fetched without one).
    Aqi(f64, f64, f64, f64, String),
    /// Live stations in a lat/lon bbox, plus the view centre the keyed networks are asked around
    /// and the keys themselves (empty = that network stays off).
    Stations {
        bbox: (f64, f64, f64, f64),
        center: (f64, f64),
        tempest: String,
        wu: String,
        synoptic: String,
    },
    /// NOAA's PPEF electric-field table.
    Ppef,
    /// Highway cameras within a lon/lat bbox.
    DotCams(f64, f64, f64, f64),
    /// A user-configured field-mill endpoint.
    Mill(String),
    /// Damage surveys: a lon/lat bbox plus the UTC day whose storms to ask for.
    Dat((f64, f64, f64, f64), chrono::NaiveDate),
    /// Multi-radar mosaic over a named set of sites (chosen from the view before spawning, so the
    /// fetch task needs no camera state).
    Mosaic(Vec<String>),
    /// River flood gauges within a lat/lon bbox `(lat0, lon0, lat1, lon1)`.
    Gauges(f64, f64, f64, f64),
    /// Model contours for a field kind (surface f00, contoured off-thread), from HRRR or the RAP
    /// analysis.
    Contours(ContourKind, wxdata::hrrr::Model, crate::settings::TempUnit),
    /// County power outages by county, from ODIN (DOE/ORNL).
    Outages,
    /// NHC tropical cyclones (feature V).
    /// NHC tropical suite: `(wind-field threshold in kt, include storm surge)`.
    Tropical(Option<u8>, bool),
    /// HRRR wind components for the particle layer, at a level and forecast hour.
    Wind(wxdata::hrrr::WindLevel, u8),
}

/// A background result plus the identity of the request that produced it.
pub(crate) enum OverlayDelivery {
    /// Work that is inline or already carries enough identity to reject itself at the receiver.
    Immediate(OverlayMsg),
    Fetched {
        lane: RequestLane,
        generation: u64,
        result: Result<OverlayMsg, String>,
    },
}

impl OverlaySource {
    pub(crate) fn lane(&self) -> RequestLane {
        use crate::render::FieldLayer as FL;
        match self {
            Self::Alerts(..) => RequestLane::Feed(FeedSource::WeatherAlerts),
            Self::Mds => RequestLane::Feed(FeedSource::MesoscaleDiscussions),
            Self::Watches => RequestLane::Feed(FeedSource::WatchBoxes),
            Self::Wssi(..) => RequestLane::Feed(FeedSource::WinterStormSeverity),
            Self::Ero(..) => RequestLane::Feed(FeedSource::ExcessiveRainfallOutlook),
            Self::FireWx(..) => RequestLane::Feed(FeedSource::FireWeatherOutlook),
            Self::Mping(..) => RequestLane::Feed(FeedSource::MpingReports),
            Self::Pireps(..) => RequestLane::Feed(FeedSource::PilotReports),
            Self::Recon => RequestLane::Feed(FeedSource::HurricaneReconnaissance),
            Self::Outlook(..) => RequestLane::Feed(FeedSource::SpcOutlook),
            Self::Cells(..) => RequestLane::Feed(FeedSource::StormCells),
            Self::Placefile(url) => RequestLane::Placefile(url.clone()),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Plugin(key, ..) => RequestLane::Placefile(key.clone()),
            Self::Field(layer, ..)
            | Self::Env(layer, ..)
            | Self::HrrrLayer(layer, ..)
            | Self::Global(layer, ..)
            | Self::L3Grid(layer, ..)
            | Self::Goes(layer, ..)
            | Self::GoesDiff(layer, ..)
            | Self::GoesCoolingRate(layer, ..)
            | Self::Ndfd(layer)
            | Self::Rtma(layer, _) => RequestLane::Field(*layer),
            Self::GoesRgb(..) => RequestLane::Field(FL::GoesRgb),
            Self::GoesFootprint(..) => RequestLane::Feed(FeedSource::GoesMesoSector),
            Self::GlmWindow(..) => RequestLane::Feed(FeedSource::GlmArchive),
            Self::ModelDiff(..) => RequestLane::Field(FL::ModelDiff),
            // Both compare panes ride one fetch (see `fetch_diff_pair`); either layer name works
            // as the dedup key, so it just picks the first.
            Self::Compare(..) => RequestLane::Field(FL::CompareA),
            Self::Ensemble(..) => RequestLane::Field(FL::Ensemble),
            Self::Mosaic(..) => RequestLane::Field(FL::Mosaic),
            Self::Hrrr(..) | Self::HrrrSub(..) => RequestLane::Field(FL::Hrrr),
            Self::Snow(..) => RequestLane::Field(FL::SnowAnalysis),
            Self::SnowBands => RequestLane::Field(FL::SnowBands),
            Self::StormReports(Some(_)) => RequestLane::Feed(FeedSource::ArchivedStormReports),
            Self::StormReports(None) => RequestLane::Feed(FeedSource::StormReports),
            Self::Spotters => RequestLane::Feed(FeedSource::SpotterNetwork),
            Self::ProbSevere => RequestLane::Feed(FeedSource::ProbSevere),
            Self::Fronts => RequestLane::Feed(FeedSource::SurfaceAnalysis),
            Self::FreezingLevels { .. } => RequestLane::Feed(FeedSource::FreezingLevels),
            Self::Obs { .. } => RequestLane::Feed(FeedSource::RadarObservations),
            Self::Vwp(..) => RequestLane::Feed(FeedSource::VadProfile),
            Self::ArchiveWarnings(..) => RequestLane::Feed(FeedSource::ArchivedWarnings),
            // Its own lane: a lane keeps only its newest request's answer, and a warnings fetch
            // for the same frame would otherwise throw this one away.
            Self::ArchiveMds(..) => RequestLane::Feed(FeedSource::ArchivedDiscussions),
            Self::Aviation => RequestLane::Feed(FeedSource::AviationAdvisories),
            Self::Tfr(..) => RequestLane::Feed(FeedSource::TemporaryFlightRestrictions),
            Self::Metar(..) => RequestLane::Feed(FeedSource::SurfaceObservations),
            Self::Webcams(..) => RequestLane::Feed(FeedSource::Webcams),
            Self::Fires(..) => RequestLane::Feed(FeedSource::Wildfires),
            Self::Aqi(..) => RequestLane::Feed(FeedSource::AirQuality),
            Self::Stations { .. } => RequestLane::Feed(FeedSource::LiveStations),
            Self::Ppef => RequestLane::Feed(FeedSource::ElectricField),
            Self::DotCams(..) => RequestLane::Feed(FeedSource::HighwayCameras),
            Self::Mill(..) => RequestLane::Feed(FeedSource::FieldMill),
            Self::Dat(..) => RequestLane::Feed(FeedSource::DamageSurveys),
            Self::Gauges(..) => RequestLane::Feed(FeedSource::RiverGauges),
            Self::Contours(..) => RequestLane::Feed(FeedSource::ModelContours),
            Self::Outages => RequestLane::Feed(FeedSource::PowerOutages),
            Self::Tropical(..) => RequestLane::Feed(FeedSource::TropicalCyclones),
            Self::Wind(..) => RequestLane::Feed(FeedSource::WindParticles),
        }
    }

    pub(crate) async fn fetch(self, http: &reqwest::Client) -> anyhow::Result<OverlayMsg> {
        Ok(match self {
            OverlaySource::Alerts(points, bounds) => {
                let mut feats = alerts::fetch_active(http, &points).await?;
                // Europe's warnings come from a different publisher on a different continent, so
                // they are only asked for when the view is actually over one of the countries
                // that publishes them — otherwise this is a no-op with no request at all.
                if !wxdata::meteoalarm::countries_in_view(bounds).is_empty() {
                    match wxdata::meteoalarm::fetch_in_view(http, bounds).await {
                        Ok(eu) => feats.extend(eu),
                        // A European feed being down must not cost the US alerts already fetched.
                        Err(e) => log::warn!("meteoalarm fetch failed ({e})"),
                    }
                }
                // Canada's the same story one border north: gated on the view, and its own
                // failure, so an outage there costs neither the US nor the European alerts.
                if wxdata::eccc::in_view(bounds) {
                    match wxdata::eccc::fetch_in_view(http, bounds).await {
                        Ok(ca) => feats.extend(ca),
                        Err(e) => log::warn!("eccc alerts fetch failed ({e})"),
                    }
                }
                OverlayMsg::Alerts(feats)
            }
            OverlaySource::Mds => {
                OverlayMsg::Mds(wxdata::spc::fetch_mesoscale_discussions(http).await?)
            }
            OverlaySource::Watches => OverlayMsg::Watches(wxdata::spc::fetch_watches(http).await?),
            OverlaySource::Wssi(day) => {
                OverlayMsg::Wssi(day, wxdata::wssi::fetch(http, day).await?)
            }
            OverlaySource::Mping(key) => {
                OverlayMsg::Mping(wxdata::mping::fetch(http, &key, 60).await?)
            }
            OverlaySource::Pireps(lat0, lon0, lat1, lon1) => OverlayMsg::Pireps(
                wxdata::aviation::fetch_pireps(http, lat0, lon0, lat1, lon1).await?,
            ),
            OverlaySource::Ero(day) => OverlayMsg::Ero(day, wxdata::ero::fetch(http, day).await?),
            OverlaySource::FireWx(day) => {
                OverlayMsg::FireWx(day, wxdata::firewx::fetch(http, day).await?)
            }
            OverlaySource::Recon => OverlayMsg::Recon(wxdata::recon::fetch(http, 6).await?),
            OverlaySource::Outlook(day, kind) => {
                OverlayMsg::Outlook(day, wxdata::spc::fetch_outlook_kind(http, day, kind).await?)
            }
            OverlaySource::Cells(site, history) => {
                let (cells, past) = futures_util::join!(level3::fetch_cells(http, &site), async {
                    if history {
                        level3::fetch_cell_history(http, &site, CELL_HISTORY_SCANS).await
                    } else {
                        Vec::new()
                    }
                });
                OverlayMsg::Cells(site, cells, past)
            }
            OverlaySource::Placefile(url) => {
                let pf = wxdata::placefile::fetch(http, &url).await?;
                OverlayMsg::Placefile(url, pf)
            }
            #[cfg(not(target_arch = "wasm32"))]
            OverlaySource::Plugin(key, command, args, pctx) => {
                // A plugin failure is the user's own command misbehaving, so it has to reach the
                // manager window rather than only the log — hence a message either way.
                match crate::plugins::run(&command, &args, &pctx).await {
                    Ok(pf) => OverlayMsg::Placefile(key, pf),
                    Err(e) => OverlayMsg::PlacefileError(key, e.to_string()),
                }
            }
            OverlaySource::Field(layer, request) => {
                let field = if let Some((target, minutes)) = request.archive {
                    wxdata::mrms::fetch_nearest_stamped(
                        http,
                        &request.product,
                        target,
                        chrono::Duration::minutes(minutes as i64),
                    )
                    .await?
                } else {
                    wxdata::mrms::fetch_latest_stamped(http, &request.product).await?
                };
                OverlayMsg::MrmsField(layer, field, request)
            }
            OverlaySource::SnowBands => {
                // Both grids at once: the mask is useless without the echo and vice versa.
                let (mosaic, flags) = futures_util::future::try_join(
                    wxdata::mrms::fetch_latest(http, wxdata::mrms::REFLECTIVITY),
                    wxdata::mrms::fetch_latest(http, wxdata::mrms::PRECIP_TYPE),
                )
                .await?;
                // MRMS PrecipFlag: 3 is snow, 4 is wet snow. Everything else is rain, ice or
                // nothing, and a snow-squall layer that lit up over warm rain would be a liar.
                let bands = wxdata::banding::bands(&mosaic, 20.0, Some((&flags, &[3, 4])))
                    .ok_or_else(|| anyhow::anyhow!("the mosaic came back empty"))?;
                OverlayMsg::Field(crate::render::FieldLayer::SnowBands, bands)
            }
            OverlaySource::Global(layer, model, field, fh, run) => {
                let fc = match run {
                    Some(run) => wxdata::global::fetch_at_run(http, model, field, run, fh).await?,
                    None => wxdata::global::fetch(http, model, field, fh).await?,
                };
                let valid = fc.valid();
                OverlayMsg::StampedField(
                    layer,
                    field_state::model_field(
                        model.label(),
                        field.slug(),
                        fc.field,
                        Some(fc.run),
                        valid,
                        false,
                    )?,
                )
            }
            OverlaySource::ModelDiff(field, fh) => {
                let pair = crate::fielddiff::fetch_pair(http, field, fh).await?;
                let d = crate::fielddiff::diff(&pair.a, &pair.b)
                    .ok_or_else(|| anyhow::anyhow!("the two models cover nothing in common"))?;
                // The floor is in display units; the grids are in wire units.
                let pct = field.percent_floor().and_then(|floor| {
                    crate::fielddiff::percent(&pair.a, &pair.b, floor / field.input_scale())
                });
                OverlayMsg::ModelDiff(field, fh, d, pct, pair.times)
            }
            OverlaySource::Compare(field, fh) => {
                let pair = crate::fielddiff::fetch_pair(http, field, fh).await?;
                OverlayMsg::Compare(field, fh, pair.a, pair.b, pair.times)
            }
            OverlaySource::Ensemble(field, fh) => {
                let run = wxdata::ensemble::fetch_gefs(http, field, fh).await?;
                OverlayMsg::Ensemble(field, fh, Box::new(run))
            }
            OverlaySource::StormReports(bucket) => {
                // Archive bucket: the 6 h of reports ending at the bucket's close; live: last 6 h.
                let reports = match bucket {
                    Some(b) => {
                        let end =
                            chrono::DateTime::from_timestamp((b + 1) * 1800, 0).unwrap_or_default();
                        let start = end - chrono::Duration::hours(6);
                        let fmt = "%Y-%m-%dT%H:%MZ";
                        wxdata::lsr::fetch(
                            http,
                            Some((&start.format(fmt).to_string(), &end.format(fmt).to_string())),
                        )
                        .await?
                    }
                    None => wxdata::lsr::fetch(http, None).await?,
                };
                OverlayMsg::StormReports(bucket, reports)
            }
            OverlaySource::Fronts => OverlayMsg::Fronts(wxdata::fronts::fetch(http).await?),
            OverlaySource::Spotters => {
                OverlayMsg::Spotters(wxdata::spotters::fetch_spotters(http).await?)
            }
            OverlaySource::ProbSevere => {
                OverlayMsg::ProbSevere(wxdata::probsevere::fetch_probsevere(http).await?)
            }
            OverlaySource::Hrrr(model, fh, run) => {
                use wxdata::model::ModelField;
                // The GRIB spelling comes from the model catalogue, so RAP and the NAMs (whose
                // reflectivity level is spelled differently) use the same path as the HRRR.
                let key = ModelField::CompositeReflectivity
                    .grib(model)
                    .ok_or_else(|| {
                        anyhow::anyhow!("{} does not publish reflectivity", model.label())
                    })?;
                let fc = match run {
                    Some(run) => {
                        wxdata::hrrr::fetch_field_at_run(
                            http,
                            model,
                            run,
                            key.var,
                            key.level,
                            fh,
                            key.min_valid,
                        )
                        .await?
                    }
                    None => {
                        wxdata::hrrr::fetch_field(
                            http,
                            model,
                            key.var,
                            key.level,
                            fh,
                            key.min_valid,
                        )
                        .await?
                    }
                };
                OverlayMsg::Hrrr(fc)
            }
            OverlaySource::HrrrSub(mins, run) => OverlayMsg::Hrrr(match run {
                Some(run) => wxdata::hrrr::fetch_forecast_subhourly_at_run(http, run, mins).await?,
                None => wxdata::hrrr::fetch_forecast_subhourly(http, mins).await?,
            }),
            OverlaySource::HrrrLayer(layer, fh, run) => {
                use crate::render::FieldLayer as FL;
                use wxdata::hrrr::Model::{Hrrr as HRRR, Nbm as NBM};
                use wxdata::model::ModelField as MF;
                // Phase F1: each of these was a hand-written GRIB var/level pair. They now come
                // from the catalogue, which a live contract test checks against the real `.idx`
                // sidecars — so a NOAA rename surfaces as a failing test rather than as a layer
                // that quietly stops drawing.
                let uh_key = || {
                    MF::UpdraftHelicity
                        .grib(HRRR)
                        .expect("HRRR publishes updraft helicity")
                };
                async fn model_field(
                    http: &reqwest::Client,
                    field: MF,
                    model: wxdata::hrrr::Model,
                    fh: u8,
                    run: Option<DateTime<Utc>>,
                ) -> anyhow::Result<wxdata::hrrr::HrrrForecast> {
                    let k = field.grib(model).ok_or_else(|| {
                        anyhow::anyhow!("{} does not publish {}", model.label(), field.label())
                    })?;
                    match run {
                        Some(run) => {
                            wxdata::hrrr::fetch_field_at_run(
                                http,
                                model,
                                run,
                                k.var,
                                k.level,
                                fh,
                                k.min_valid,
                            )
                            .await
                        }
                        None => {
                            wxdata::hrrr::fetch_field(http, model, k.var, k.level, fh, k.min_valid)
                                .await
                        }
                    }
                }
                let fc = match layer {
                    // Rotation tracks read as a swath: the union of every hourly max window from
                    // now through the scrubbed hour, not just that one hour's slice.
                    FL::UpdraftHelicity => {
                        let k = uh_key();
                        match run {
                            Some(run) => {
                                wxdata::hrrr::fetch_field_swath_at_run(
                                    http,
                                    k.var,
                                    k.level,
                                    run,
                                    fh.max(1),
                                    k.min_valid,
                                )
                                .await?
                            }
                            None => {
                                wxdata::hrrr::fetch_field_swath(
                                    http,
                                    k.var,
                                    k.level,
                                    fh.max(1),
                                    k.min_valid,
                                )
                                .await?
                            }
                        }
                    }
                    // Accumulated snowfall through the scrubbed hour.
                    FL::Snowfall => model_field(http, MF::Snowfall, HRRR, fh, run).await?,
                    // NBM's calibrated probability of thunder over the hour ending at `fh`. The
                    // idx lists the trailing window first, so the plain var+level match already
                    // picks that one over the run-total windows beside it.
                    FL::ThunderProb => {
                        model_field(http, MF::ThunderProbability, NBM, fh.max(1), run).await?
                    }
                    _ => model_field(http, MF::Smoke, HRRR, fh, run).await?,
                };
                let source = if layer == FL::ThunderProb {
                    "NBM"
                } else {
                    "HRRR"
                };
                let valid = fc.valid();
                OverlayMsg::StampedField(
                    layer,
                    field_state::model_field(
                        source,
                        layer.slug(),
                        fc.field,
                        Some(fc.run),
                        valid,
                        layer == FL::UpdraftHelicity,
                    )?,
                )
            }
            OverlaySource::Env(layer, model, ml, srh_km, fh, run) => {
                use crate::render::FieldLayer as FL;
                use wxdata::model::ModelField;
                // Phase F1: the GRIB spelling is the model catalogue's business, not this
                // dispatch's. The old literals here were right for the HRRR and silently wrong
                // for the NAM, whose composite-reflectivity level string is spelled differently
                // — a bug this migration fixes rather than a refactor that preserves it.
                let field = match layer {
                    FL::Cape if ml => ModelField::MixedLayerCape,
                    FL::Cape => ModelField::SurfaceCape,
                    FL::Srh if srh_km == 1 => ModelField::Srh1km,
                    FL::Srh => ModelField::Srh3km,
                    _ => ModelField::CompositeReflectivity,
                };
                let key = field.grib(model).ok_or_else(|| {
                    anyhow::anyhow!("{} does not publish {}", model.label(), field.label())
                })?;
                let (var, level) = (key.var, key.level);
                let fc = match run {
                    Some(run) => {
                        wxdata::hrrr::fetch_field_at_run(
                            http,
                            model,
                            run,
                            var,
                            level,
                            fh,
                            key.min_valid,
                        )
                        .await?
                    }
                    None => {
                        wxdata::hrrr::fetch_field(http, model, var, level, fh, key.min_valid)
                            .await?
                    }
                };
                let valid = fc.valid();
                OverlayMsg::StampedField(
                    layer,
                    field_state::model_field(
                        model.label(),
                        &format!("{var}:{level}"),
                        fc.field,
                        Some(fc.run),
                        valid,
                        false,
                    )?,
                )
            }
            OverlaySource::L3Grid(layer, site) => {
                use crate::render::FieldLayer as FL;
                let field = match layer {
                    FL::Vil => wxdata::level3::fetch_dvl(http, &site).await,
                    FL::EchoTops => wxdata::level3::fetch_eet(http, &site).await,
                    FL::Hca => wxdata::level3::fetch_hhc(http, &site).await,
                    _ => None,
                };
                match field {
                    Some(f) => OverlayMsg::Field(layer, f),
                    None => anyhow::bail!("no L3 grid for {site}"),
                }
            }
            OverlaySource::Snow(hours) => OverlayMsg::Field(
                crate::render::FieldLayer::SnowAnalysis,
                wxdata::nohrsc::fetch(http, hours).await?,
            ),
            OverlaySource::Goes(layer, satellite, sector, at) => {
                use crate::render::FieldLayer as FL;
                // Band number for each channel's own S3 objects — see `wxdata::goes_abi`'s doc
                // comment for why CMIP CONUS is the product either way.
                let band = match layer {
                    FL::GoesIr | FL::GoesColdTop => 13,
                    FL::GoesVisible => 2,
                    FL::GoesWaterVapor => 8,
                    FL::GoesShortwaveIr => 7,
                    FL::GoesMidWaterVapor => 9,
                    FL::GoesLowWaterVapor => 10,
                    FL::GoesDirtyIr => 15,
                    FL::GoesLongwaveIr => 14,
                    _ => anyhow::bail!("{layer:?} is not a GOES band"),
                };
                // A mesoscale box is about 1000 km a side: square, and finer per degree.
                let (nx, ny) = goes_grid(sector);
                let mut field =
                    wxdata::goes_abi::fetch_at(http, satellite, sector, band, at, nx, ny).await?;
                // Cold-cloud-top threshold overlay (ROADMAP_NEW E6): the exact same Band 13 data
                // as GoesIr, re-expressed as "how many kelvin colder than the overshooting-top
                // threshold" so this layer's own ramp (`GOES_COLD_TOP`) can hide ordinary cloud
                // with a plain `lo` cutoff — the same value-inversion trick `GoesDustDiff` uses,
                // and for the same reason: this ramp system's cutoff only hides *low* values, so
                // the quantity has to be defined such that "not cold enough to matter" is the low
                // end.
                if layer == FL::GoesColdTop {
                    for v in &mut field.values {
                        *v = crate::render::field_ramps::COLD_TOP_THRESHOLD_K - *v;
                    }
                }
                OverlayMsg::Field(layer, field)
            }
            OverlaySource::GoesDiff(layer, satellite) => {
                use crate::render::FieldLayer as FL;
                let (band_a, band_b) = match layer {
                    // Band 13 minus Band 15 — see `render::field_ramps`'s `GOES_DUST_DIFF` doc
                    // comment for why the subtraction is this way around.
                    FL::GoesDustDiff => (13, 15),
                    _ => anyhow::bail!("{layer:?} is not a GOES difference product"),
                };
                OverlayMsg::Field(
                    layer,
                    wxdata::goes_abi::fetch_latest_conus_diff(
                        http, satellite, band_a, band_b, 1200, 700,
                    )
                    .await?,
                )
            }
            OverlaySource::GoesCoolingRate(layer, satellite) => {
                use crate::render::FieldLayer as FL;
                let (band, lookback_minutes) = match layer {
                    FL::GoesCoolingRate => (13, 15),
                    _ => anyhow::bail!("{layer:?} is not a GOES time-difference product"),
                };
                OverlayMsg::Field(
                    layer,
                    wxdata::goes_abi::fetch_cooling_rate(
                        http,
                        satellite,
                        band,
                        lookback_minutes,
                        1200,
                        700,
                    )
                    .await?,
                )
            }
            OverlaySource::GoesRgb(recipe, satellite, sector, at) => {
                let (nx, ny) = goes_grid(sector);
                let rgb =
                    wxdata::goes_rgb::fetch_recipe(http, satellite, sector, at, recipe, nx, ny)
                        .await?;
                OverlayMsg::Field(
                    crate::render::FieldLayer::GoesRgb,
                    wxdata::goes_rgb::pack(&rgb),
                )
            }
            OverlaySource::GlmWindow(end, west) => OverlayMsg::GlmWindow(
                end,
                wxdata::glm::fetch_window(http, west, end, GLM_ARCHIVE_MINUTES).await?,
            ),
            OverlaySource::GoesFootprint(satellite, sector, at) => OverlayMsg::GoesFootprint(
                sector,
                wxdata::goes_abi::footprint(http, satellite, sector, at).await?,
            ),
            OverlaySource::Ndfd(layer) => {
                use crate::render::FieldLayer as FL;
                let field = match layer {
                    FL::NdfdTemp2m => wxdata::ndfd::NdfdField::Temp,
                    FL::NdfdWind10m => wxdata::ndfd::NdfdField::WindSpeed,
                    FL::NdfdGust10m => wxdata::ndfd::NdfdField::WindGust,
                    FL::NdfdSnow => wxdata::ndfd::NdfdField::Snow,
                    _ => anyhow::bail!("{layer:?} is not an NDFD element"),
                };
                OverlayMsg::Field(layer, wxdata::ndfd::fetch(http, field).await?)
            }
            OverlaySource::Rtma(layer, hour) => {
                use crate::render::FieldLayer as FL;
                let field = match layer {
                    FL::RtmaTemp2m => wxdata::rtma::RtmaField::Temp2m,
                    FL::RtmaDewpoint2m => wxdata::rtma::RtmaField::Dewpoint2m,
                    FL::RtmaWind10m => wxdata::rtma::RtmaField::Wind10m,
                    FL::RtmaGust10m => wxdata::rtma::RtmaField::Gust10m,
                    FL::RtmaVisibility => wxdata::rtma::RtmaField::Visibility,
                    FL::RtmaCeiling => wxdata::rtma::RtmaField::Ceiling,
                    FL::RtmaMslp => wxdata::rtma::RtmaField::Mslp,
                    FL::RtmaPrecip1h => wxdata::rtma::RtmaField::Precip1h,
                    _ => anyhow::bail!("{layer:?} is not an RTMA field"),
                };
                let analysis = wxdata::rtma::fetch(http, field, hour).await?;
                // An analysis has no run and no lead: the hour it describes is both.
                OverlayMsg::StampedField(
                    layer,
                    field_state::model_field(
                        analysis.kind.label(),
                        field.slug(),
                        analysis.field,
                        Some(analysis.hour),
                        analysis.hour,
                        false,
                    )?,
                )
            }
            OverlaySource::FreezingLevels {
                site,
                lon,
                lat,
                elev_m,
                epoch: Some(t),
            } => {
                // An archived volume: the balloon that went up that day, not today's model.
                let m = wxdata::raob::melting_levels(http, lon, lat, t, crate::paths::cache_dir())
                    .await?;
                log::info!(
                    target: "wxdata::derived",
                    "{site}: melting level {:.1} km, −20 °C {:.1} km, from {}",
                    m.h0_m / 1000.0,
                    m.hm20_m / 1000.0,
                    m.label
                );
                // Above the launch site's surface, taken as above the radar (the two sit within a
                // few hundred metres of each other everywhere in CONUS — see `melting_levels`),
                // then put back on the sea-level datum the HRRR branch below reports in.
                OverlayMsg::FreezingLevels {
                    site,
                    epoch: Some(t),
                    h0: m.h0_m + elev_m,
                    hm20: m.hm20_m + elev_m,
                }
            }
            OverlaySource::FreezingLevels {
                site,
                lon,
                lat,
                epoch: None,
                ..
            } => {
                // HRRR carries both isotherm heights as analysis fields, so the hail algorithm
                // sources its own thermodynamics instead of asking the user for a freezing level.
                let h0 = wxdata::hrrr::fetch_field(
                    http,
                    wxdata::hrrr::Model::Hrrr,
                    "HGT",
                    "0C isotherm",
                    0,
                    f64::NEG_INFINITY,
                )
                .await?;
                // 253 K is −20.15 °C — the level Witt's hail weighting tops out at.
                let hm20 = wxdata::hrrr::fetch_field(
                    http,
                    wxdata::hrrr::Model::Hrrr,
                    "HGT",
                    "253 K level",
                    0,
                    f64::NEG_INFINITY,
                )
                .await?;
                match (
                    h0.field.sample_bilinear(lon, lat),
                    hm20.field.sample_bilinear(lon, lat),
                ) {
                    (Some(a), Some(b)) => OverlayMsg::FreezingLevels {
                        site,
                        epoch: None,
                        h0: a as f64,
                        hm20: b as f64,
                    },
                    _ => anyhow::bail!("no freezing levels at {lon},{lat}"),
                }
            }
            OverlaySource::Obs { site, lat, lon } => {
                let r = wxdata::obs::fetch_nearest(http, lat, lon)
                    .await
                    .map_err(|e| e.to_string());
                OverlayMsg::Obs(site, r)
            }
            OverlaySource::Vwp(site) => {
                let levels = wxdata::level3::fetch_vwp(http, &site).await;
                OverlayMsg::Vwp(site, levels)
            }
            OverlaySource::ArchiveWarnings(bucket) => {
                let ts = chrono::DateTime::from_timestamp(bucket * 300, 0)
                    .unwrap_or_default()
                    .to_rfc3339();
                // An IEM outage caches the bucket empty (self-heals via LRU); log it so a
                // silent "no warnings that day" isn't mistaken for truth.
                let feats = match wxdata::archive_warnings::fetch(http, &ts).await {
                    Ok(f) => f,
                    Err(e) => {
                        log::warn!("archive warnings fetch {ts}: {e} (bucket shown empty)");
                        Vec::new()
                    }
                };
                OverlayMsg::ArchiveWarnings(bucket, feats)
            }
            OverlaySource::ArchiveMds(bucket) => {
                let at = chrono::DateTime::from_timestamp(bucket * 300, 0).unwrap_or_default();
                let feats = match wxdata::archive_mds::fetch(http, at).await {
                    Ok(f) => f,
                    Err(e) => {
                        log::warn!("archive discussions fetch {at}: {e} (bucket shown empty)");
                        Vec::new()
                    }
                };
                OverlayMsg::ArchiveMds(bucket, feats)
            }
            OverlaySource::Metar(lat0, lon0, lat1, lon1) => {
                let mut obs = wxdata::metar::fetch_bbox(http, lat0, lon0, lat1, lon1).await?;
                // Buoys extend the same layer offshore and over the Great Lakes, where the
                // airport network simply has no stations. A buoy outage must not take the
                // METARs down with it.
                match wxdata::ndbc::fetch_bbox(http, lat0, lon0, lat1, lon1).await {
                    Ok(buoys) => obs.extend(buoys),
                    Err(e) => note_feed_error("Buoy observations", e),
                }
                // Terminal forecasts for the same box, riding along with the obs they belong
                // beside. A TAF outage costs the tooltips their forecast line, nothing more.
                let tafs = match wxdata::metar::fetch_tafs(http, lat0, lon0, lat1, lon1).await {
                    Ok(t) => t,
                    Err(e) => {
                        note_feed_error("Terminal forecasts (TAF)", e);
                        Default::default()
                    }
                };
                OverlayMsg::Metar(obs, tafs)
            }
            OverlaySource::Webcams(min_lon, min_lat, max_lon, max_lat, windy_key) => {
                // Both networks, merged: the FAA is keyless but US-only, Windy covers the rest of
                // the world for anyone who has supplied a key. Nothing for the user to choose.
                let mut sites =
                    wxdata::webcams::fetch_bbox(http, min_lon, min_lat, max_lon, max_lat)
                        .await
                        .unwrap_or_else(|e| {
                            note_feed_error("FAA webcams", e);
                            Vec::new()
                        });
                if !windy_key.is_empty() {
                    // A bad or throttled key must not take the FAA cameras down with it.
                    match wxdata::webcams::fetch_windy_bbox(
                        http, &windy_key, min_lon, min_lat, max_lon, max_lat,
                    )
                    .await
                    {
                        Ok(w) => sites.extend(w),
                        Err(e) => note_feed_error("Windy webcams", e),
                    }
                }
                OverlayMsg::Webcams(sites)
            }
            OverlaySource::Fires(w, s, e, n) => {
                // Two servers; either can be down without taking the other's layer with it.
                let bbox = [w, s, e, n];
                let perims = wxdata::wfigs::fetch_perimeters(http, bbox)
                    .await
                    .unwrap_or_else(|e| {
                        log::warn!("wfigs perimeters: {e}");
                        Vec::new()
                    });
                let incidents = wxdata::wfigs::fetch_incidents(http, bbox)
                    .await
                    .unwrap_or_else(|e| {
                        log::warn!("wfigs incidents: {e}");
                        Vec::new()
                    });
                OverlayMsg::Fires(perims, incidents)
            }
            OverlaySource::Aqi(w, s, e, n, key) => {
                OverlayMsg::Aqi(wxdata::airnow::fetch_bbox(http, &key, [w, s, e, n]).await?)
            }
            OverlaySource::Stations {
                bbox,
                center,
                tempest,
                wu,
                synoptic,
            } => {
                // METARs come first and cost one request; the keyed networks add themselves.
                let metars = wxdata::metar::fetch_bbox(http, bbox.0, bbox.1, bbox.2, bbox.3)
                    .await
                    .unwrap_or_default();
                OverlayMsg::Stations(
                    wxdata::stations::fetch_all(
                        http, &metars, &tempest, &wu, &synoptic, center.0, center.1,
                    )
                    .await,
                )
            }
            OverlaySource::Ppef => OverlayMsg::Ppef(wxdata::efield::fetch_ppef(http).await?),
            OverlaySource::DotCams(min_lon, min_lat, max_lon, max_lat) => OverlayMsg::DotCams(
                wxdata::dotcams::fetch_bbox(http, min_lon, min_lat, max_lon, max_lat).await?,
            ),
            OverlaySource::Mill(url) => {
                let mut r = wxdata::efield::fetch_mill(http, &url).await?;
                r.sort_by_key(|x| x.time);
                OverlayMsg::Mill(r.last().map(|x| x.kv_per_m).unwrap_or(0.0))
            }
            OverlaySource::Dat(bbox, day) => {
                // A survey is filed against the storm's local date, which can be either side of the
                // UTC one for an evening event — ask for the day either side and let the bbox and
                // the map do the rest.
                let start = day
                    .pred_opt()
                    .unwrap_or(day)
                    .and_hms_opt(0, 0, 0)
                    .unwrap_or_default()
                    .and_utc();
                let end = start + chrono::Duration::days(3);
                let (points, tracks) = wxdata::dat::fetch(http, bbox, start, end).await?;
                OverlayMsg::Dat(points, tracks)
            }
            OverlaySource::Mosaic(sites) => {
                let m = wxdata::mosaic::fetch(http, &sites)
                    .await
                    .ok_or_else(|| anyhow::anyhow!("no radar mosaic for {sites:?}"))?;
                OverlayMsg::Mosaic(m.field, m.sites, m.oldest)
            }
            OverlaySource::Gauges(lat0, lon0, lat1, lon1) => {
                OverlayMsg::Gauges(wxdata::river::fetch_bbox(http, lat0, lon0, lat1, lon1).await?)
            }
            OverlaySource::Contours(kind, model, temp_unit) => {
                // Composite parameters (STP/SCP/EHI) combine several same-run HRRR fields.
                if let Some(sk) = kind.severe() {
                    let fc = wxdata::severe::fetch_grid(http, model, sk).await?;
                    let (run, valid) = (fc.run, fc.valid());
                    let lines = wxdata::contour::contour_lines(&fc.field, kind.severe_interval());
                    return Ok(OverlayMsg::Contours(
                        kind,
                        lines,
                        run,
                        valid,
                        Arc::new(fc.field),
                    ));
                }
                let (var, level, _) = kind
                    .params()
                    .ok_or_else(|| anyhow::anyhow!("contour Off"))?;
                let interval = kind.interval(temp_unit);
                let mut fc =
                    wxdata::hrrr::fetch_field(http, model, var, level, 0, f64::NEG_INFINITY)
                        .await?;
                // Convert to display units so the interval is in hPa / °F / etc, then contour off-thread.
                for v in &mut fc.field.values {
                    if v.is_finite() {
                        *v = kind.to_display(*v, temp_unit);
                    }
                }
                let (run, valid) = (fc.run, fc.valid());
                OverlayMsg::Contours(
                    kind,
                    wxdata::contour::contour_lines(&fc.field, interval),
                    run,
                    valid,
                    Arc::new(fc.field),
                )
            }
            OverlaySource::Outages => OverlayMsg::Outages(wxdata::outages::fetch(http).await?),
            OverlaySource::Tropical(wind_kt, surge) => OverlayMsg::Tropical(
                wxdata::tropical::fetch_active_opts(http, wind_kt, surge).await?,
            ),
            OverlaySource::Wind(level, fh) => {
                let (run, u, v) = wxdata::hrrr::fetch_wind(http, level, fh).await?;
                OverlayMsg::Wind(Box::new(crate::wind_draw::WindField {
                    u,
                    v,
                    level,
                    run,
                    fcst_hour: fh,
                }))
            }
            OverlaySource::Aviation => {
                let mut f = wxdata::aviation::fetch_airsigmet(http).await?;
                // G-AIRMETs ride with the SIGMETs: same layer, same question, and a failure
                // fetching them must not cost the SIGMETs that already arrived.
                match wxdata::aviation::fetch_gairmet(http).await {
                    Ok(g) => f.extend(g),
                    Err(e) => log::warn!("g-airmet: {e}"),
                }
                OverlayMsg::Aviation(f)
            }
            OverlaySource::Tfr(have) => {
                let (new, remaining) = wxdata::tfr::fetch(http, &have, TFR_BATCH).await?;
                OverlayMsg::Tfr(new, remaining)
            }
        })
    }
}
