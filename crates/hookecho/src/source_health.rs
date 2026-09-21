//! Stable endpoint-family metadata for ROADMAP_NEW N1 source health.
//!
//! A source's display name answers "which layer is this?"; an endpoint family answers "which
//! upstream failure domain does it use?". Keep that distinction explicit: the health UI must not
//! reverse-engineer providers from labels such as `field mesh` or `Surface observations`.

use crate::render::FieldLayer;
use wxdata::field::DataSource;

/// A stable, coarse upstream family. This intentionally identifies a service family rather than
/// a single URL: hosts and paths change, while the useful diagnostic fact is that several failed
/// rows all depend on (for example) NOAA MRMS or AviationWeather.gov.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndpointFamily {
    RadarLevel2,
    RadarProducts,
    NoaaMrms,
    NoaaNcepModels,
    GlobalModels,
    MixedModels,
    GoesOpenData,
    NwsApi,
    NoaaMapServices,
    NoaaOperationalFiles,
    IowaMesonet,
    AviationWeather,
    CommunityFeed,
    PublicPartnerApi,
    MultiProvider,
    UserConfigured,
    LocalProcessing,
}

impl EndpointFamily {
    /// Stable machine-readable identifier used by the diagnostics bundle.
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::RadarLevel2 => "radar-level2",
            Self::RadarProducts => "radar-products",
            Self::NoaaMrms => "noaa-mrms",
            Self::NoaaNcepModels => "noaa-ncep-models",
            Self::GlobalModels => "global-models",
            Self::MixedModels => "mixed-models",
            Self::GoesOpenData => "goes-open-data",
            Self::NwsApi => "nws-api",
            Self::NoaaMapServices => "noaa-map-services",
            Self::NoaaOperationalFiles => "noaa-operational-files",
            Self::IowaMesonet => "iowa-mesonet",
            Self::AviationWeather => "aviation-weather",
            Self::CommunityFeed => "community-feed",
            Self::PublicPartnerApi => "public-partner-api",
            Self::MultiProvider => "multi-provider",
            Self::UserConfigured => "user-configured",
            Self::LocalProcessing => "local-processing",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::RadarLevel2 => "Level II radar providers",
            Self::RadarProducts => "NOAA radar products",
            Self::NoaaMrms => "NOAA MRMS",
            Self::NoaaNcepModels => "NOAA/NCEP model grids",
            Self::GlobalModels => "Global model providers",
            Self::MixedModels => "Multiple model providers",
            Self::GoesOpenData => "NOAA GOES Open Data",
            Self::NwsApi => "NWS API",
            Self::NoaaMapServices => "NOAA map services",
            Self::NoaaOperationalFiles => "NOAA operational files",
            Self::IowaMesonet => "Iowa Environmental Mesonet",
            Self::AviationWeather => "AviationWeather.gov",
            Self::CommunityFeed => "Community network",
            Self::PublicPartnerApi => "Public partner API",
            Self::MultiProvider => "Multiple providers",
            Self::UserConfigured => "User-configured endpoint",
            Self::LocalProcessing => "Local processing",
        }
    }
}

/// A non-grid request lane. Keeping the label, cadence and endpoint family on one enum makes a
/// newly added feed choose all three at compile time; source health never has to infer provider
/// metadata from a human-facing string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FeedSource {
    WeatherAlerts,
    MesoscaleDiscussions,
    WatchBoxes,
    WinterStormSeverity,
    ExcessiveRainfallOutlook,
    FireWeatherOutlook,
    MpingReports,
    PilotReports,
    HurricaneReconnaissance,
    SpcOutlook,
    StormCells,
    ArchivedStormReports,
    StormReports,
    SpotterNetwork,
    ProbSevere,
    SurfaceAnalysis,
    FreezingLevels,
    RadarObservations,
    VadProfile,
    ArchivedWarnings,
    AviationAdvisories,
    TemporaryFlightRestrictions,
    SurfaceObservations,
    Webcams,
    Wildfires,
    AirQuality,
    LiveStations,
    ElectricField,
    HighwayCameras,
    FieldMill,
    DamageSurveys,
    RiverGauges,
    ModelContours,
    PowerOutages,
    TropicalCyclones,
    WindParticles,
    DerivedRadarFields,
}

impl FeedSource {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::WeatherAlerts => "Weather alerts",
            Self::MesoscaleDiscussions => "Mesoscale discussions",
            Self::WatchBoxes => "Watch boxes",
            Self::WinterStormSeverity => "Winter storm severity",
            Self::ExcessiveRainfallOutlook => "Excessive rainfall outlook",
            Self::FireWeatherOutlook => "Fire weather outlook",
            Self::MpingReports => "mPING reports",
            Self::PilotReports => "Pilot reports",
            Self::HurricaneReconnaissance => "Hurricane reconnaissance",
            Self::SpcOutlook => "SPC outlook",
            Self::StormCells => "Storm cells",
            Self::ArchivedStormReports => "Archived storm reports",
            Self::StormReports => "Storm reports",
            Self::SpotterNetwork => "Spotter Network",
            Self::ProbSevere => "ProbSevere",
            Self::SurfaceAnalysis => "Surface analysis",
            Self::FreezingLevels => "Freezing levels",
            Self::RadarObservations => "Radar observations",
            Self::VadProfile => "VAD profile",
            Self::ArchivedWarnings => "Archived warnings",
            Self::AviationAdvisories => "Aviation advisories",
            Self::TemporaryFlightRestrictions => "Temporary flight restrictions",
            Self::SurfaceObservations => "Surface observations",
            Self::Webcams => "Webcams",
            Self::Wildfires => "Wildfires",
            Self::AirQuality => "Air quality",
            Self::LiveStations => "Live stations",
            Self::ElectricField => "Electric field",
            Self::HighwayCameras => "Highway cameras",
            Self::FieldMill => "Field mill",
            Self::DamageSurveys => "Damage surveys",
            Self::RiverGauges => "River gauges",
            Self::ModelContours => "Model contours",
            Self::PowerOutages => "Power outages",
            Self::TropicalCyclones => "Tropical cyclones",
            Self::WindParticles => "Wind particles",
            Self::DerivedRadarFields => "Derived radar fields",
        }
    }

    pub(crate) const fn cadence_secs(self) -> u64 {
        match self {
            Self::SpotterNetwork
            | Self::LiveStations
            | Self::FieldMill
            | Self::DerivedRadarFields => 60,
            Self::SurfaceObservations => 75,
            Self::MpingReports
            | Self::PowerOutages
            | Self::RiverGauges
            | Self::ElectricField
            | Self::VadProfile
            | Self::ArchivedWarnings => 300,
            Self::Webcams => 480,
            Self::HurricaneReconnaissance | Self::AviationAdvisories | Self::RadarObservations => {
                600
            }
            Self::TropicalCyclones
            | Self::Wildfires
            | Self::AirQuality
            | Self::TemporaryFlightRestrictions
            | Self::WindParticles
            | Self::FreezingLevels
            | Self::ModelContours => 900,
            Self::SurfaceAnalysis | Self::ArchivedStormReports => 1_800,
            Self::HighwayCameras | Self::DamageSurveys => 3_600,
            _ => 120,
        }
    }

    pub(crate) const fn endpoint_family(self) -> EndpointFamily {
        match self {
            Self::WeatherAlerts | Self::SurfaceAnalysis => EndpointFamily::NwsApi,
            Self::MesoscaleDiscussions
            | Self::WatchBoxes
            | Self::WinterStormSeverity
            | Self::ExcessiveRainfallOutlook
            | Self::FireWeatherOutlook
            | Self::DamageSurveys => EndpointFamily::NoaaMapServices,
            Self::SpcOutlook | Self::HurricaneReconnaissance | Self::TropicalCyclones => {
                EndpointFamily::NoaaOperationalFiles
            }
            Self::StormReports | Self::ArchivedStormReports | Self::ArchivedWarnings => {
                EndpointFamily::IowaMesonet
            }
            Self::PilotReports | Self::AviationAdvisories | Self::SurfaceObservations => {
                EndpointFamily::AviationWeather
            }
            Self::StormCells | Self::RadarObservations | Self::VadProfile => {
                EndpointFamily::RadarProducts
            }
            Self::ProbSevere => EndpointFamily::NoaaMrms,
            Self::FreezingLevels | Self::ModelContours | Self::WindParticles => {
                EndpointFamily::NoaaNcepModels
            }
            Self::SpotterNetwork => EndpointFamily::CommunityFeed,
            Self::Webcams | Self::LiveStations | Self::HighwayCameras => {
                EndpointFamily::MultiProvider
            }
            Self::MpingReports
            | Self::Wildfires
            | Self::AirQuality
            | Self::TemporaryFlightRestrictions
            | Self::ElectricField
            | Self::PowerOutages
            | Self::RiverGauges => EndpointFamily::PublicPartnerApi,
            Self::DerivedRadarFields => EndpointFamily::LocalProcessing,
            Self::FieldMill => EndpointFamily::UserConfigured,
        }
    }
}

/// Endpoint family for a gridded layer. Descriptor-backed layers inherit the registry's source,
/// so adding another MRMS/model product automatically classifies its health row too.
pub(crate) fn field_endpoint_family(layer: FieldLayer) -> EndpointFamily {
    if let Some(descriptor) = layer.descriptor() {
        return match descriptor.source {
            DataSource::NoaaMrms => EndpointFamily::NoaaMrms,
            DataSource::NoaaNcepModels => EndpointFamily::NoaaNcepModels,
            DataSource::GlobalModels => EndpointFamily::GlobalModels,
        };
    }

    use FieldLayer as FL;
    match layer {
        FL::Mosaic | FL::Vil | FL::EchoTops | FL::Hca => EndpointFamily::RadarProducts,
        FL::SnowBands => EndpointFamily::NoaaMrms,
        FL::GlmFed
        | FL::GoesIr
        | FL::GoesVisible
        | FL::GoesWaterVapor
        | FL::GoesShortwaveIr
        | FL::GoesMidWaterVapor
        | FL::GoesLowWaterVapor
        | FL::GoesDirtyIr
        | FL::GoesDustDiff
        | FL::GoesColdTop
        | FL::GoesCoolingRate => EndpointFamily::GoesOpenData,
        FL::ModelDiff | FL::CompareA | FL::CompareB => EndpointFamily::MixedModels,
        FL::Ensemble | FL::RtmaTemp2m | FL::RtmaDewpoint2m | FL::RtmaWind10m | FL::RtmaGust10m => {
            EndpointFamily::NoaaNcepModels
        }
        FL::SnowAnalysis => EndpointFamily::NoaaOperationalFiles,
        FL::NdfdTemp2m | FL::NdfdWind10m | FL::NdfdGust10m | FL::NdfdSnow => {
            EndpointFamily::NoaaNcepModels
        }
        // Locally-derived fields normally have no request-book row. Keeping the fallback honest
        // makes this helper safe if one is health-tracked later.
        _ => EndpointFamily::LocalProcessing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_backed_fields_inherit_the_registry_source() {
        assert_eq!(
            field_endpoint_family(FieldLayer::Mesh),
            EndpointFamily::NoaaMrms
        );
        assert_eq!(
            field_endpoint_family(FieldLayer::Cape),
            EndpointFamily::NoaaNcepModels
        );
        assert_eq!(
            field_endpoint_family(FieldLayer::GlobalMslp),
            EndpointFamily::GlobalModels
        );
    }

    #[test]
    fn shared_feed_failures_group_under_the_same_family() {
        assert_eq!(
            FeedSource::SurfaceObservations.endpoint_family(),
            FeedSource::AviationAdvisories.endpoint_family()
        );
        assert_eq!(
            FeedSource::StormReports.endpoint_family(),
            FeedSource::ArchivedWarnings.endpoint_family()
        );
    }

    #[test]
    fn typed_feed_metadata_preserves_source_specific_cadence() {
        assert_eq!(FeedSource::SurfaceObservations.cadence_secs(), 75);
        assert_eq!(FeedSource::FieldMill.cadence_secs(), 60);
        assert_eq!(FeedSource::DamageSurveys.cadence_secs(), 3_600);
        assert_eq!(
            FeedSource::SurfaceObservations.label(),
            "Surface observations"
        );
    }

    #[test]
    fn ids_are_stable_machine_names_not_display_labels() {
        let family = EndpointFamily::AviationWeather;
        assert_eq!(family.id(), "aviation-weather");
        assert_eq!(family.label(), "AviationWeather.gov");
    }
}
