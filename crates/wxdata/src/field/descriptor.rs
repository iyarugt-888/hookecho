//! Metadata shared by product catalogs; display grids remain separate payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldId(pub &'static str);

/// Published geographic coverage, independent of the exact bounds on any one fetched grid.
/// Bounds are `[west, south, east, north]` in longitude/latitude degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeographicBounds {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}

impl GeographicBounds {
    pub const WORLD: Self = Self {
        west: -180.0,
        south: -90.0,
        east: 180.0,
        north: 90.0,
    };

    /// Generous published-domain box shared by regional NOAA grids and CONUS MRMS products.
    pub const CONUS: Self = Self {
        west: -134.0,
        south: 20.0,
        east: -60.0,
        north: 53.0,
    };

    pub fn contains(self, lon: f64, lat: f64) -> bool {
        lon.is_finite()
            && lat.is_finite()
            && (self.west..=self.east).contains(&lon)
            && (self.south..=self.north).contains(&lat)
    }
}

/// Stable source identity for product catalogs. `id` is suitable for cache namespaces and saved
/// configuration; `display_name` is the human-facing provenance label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataSource {
    NoaaMrms,
    /// NOAA/NCEP regional and blended numerical guidance (HRRR, RAP, NAM and NBM).
    NoaaNcepModels,
    /// Deterministic and ensemble global models (GFS, ECMWF open IFS, GEFS, GDPS) — genuinely
    /// multi-agency (ECMWF is European, GDPS is Environment Canada's), so this names the class of
    /// model rather than a single publisher the way `NoaaNcepModels` can.
    GlobalModels,
}

impl DataSource {
    pub const fn id(self) -> &'static str {
        match self {
            Self::NoaaMrms => "noaa-mrms",
            Self::NoaaNcepModels => "noaa-ncep-models",
            Self::GlobalModels => "global-models",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::NoaaMrms => "NOAA MRMS",
            Self::NoaaNcepModels => "NOAA/NCEP models",
            Self::GlobalModels => "Global models (GFS/ECMWF/GEFS/GDPS)",
        }
    }
}

impl std::fmt::Display for DataSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldFamily {
    Radar,
    Mrms,
    Satellite,
    Model,
    Analysis,
    ObservationDerived,
    UserDefined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Scalar,
    Categorical,
    Vector,
    Probability,
    Accumulation,
    Mask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Dbz,
    Millimeters,
    Inches,
    MillimetersPerHour,
    InchesPerHour,
    PerSecond,
    MilliPerSecond,
    StrikesPerSquareKmPerMinute,
    Years,
    Category,
    JoulesPerKilogram,
    SquareMetersPerSquareSecond,
    Meters,
    KilogramsPerCubicMeter,
    Pascals,
    Kelvin,
    Percent,
    MetersPerSecond,
}

impl Unit {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Dbz => "dBZ",
            Self::Millimeters => "mm",
            Self::Inches => "in",
            Self::MillimetersPerHour => "mm/hr",
            Self::InchesPerHour => "in/hr",
            Self::PerSecond => "s⁻¹",
            Self::MilliPerSecond => "0.001/s",
            Self::StrikesPerSquareKmPerMinute => "strikes/km²/min",
            Self::Years => "years",
            Self::Category => "category",
            Self::JoulesPerKilogram => "J/kg",
            Self::SquareMetersPerSquareSecond => "m²/s²",
            Self::Meters => "m",
            Self::KilogramsPerCubicMeter => "kg/m³",
            Self::Pascals => "Pa",
            Self::Kelvin => "K",
            Self::Percent => "%",
            Self::MetersPerSecond => "m/s",
        }
    }

    /// Reject incompatible dimensions; missing/nonfinite samples stay missing.
    pub fn convert(self, value: f32, target: Self) -> Option<f32> {
        if !value.is_finite() {
            return None;
        }
        if self == target {
            return Some(value);
        }
        match (self, target) {
            (Self::MilliPerSecond, Self::PerSecond) => Some(value * 0.001),
            (Self::PerSecond, Self::MilliPerSecond) => Some(value * 1000.0),
            (Self::Millimeters, Self::Inches) | (Self::MillimetersPerHour, Self::InchesPerHour) => {
                Some(value / 25.4)
            }
            (Self::Inches, Self::Millimeters) | (Self::InchesPerHour, Self::MillimetersPerHour) => {
                Some(value * 25.4)
            }
            _ => None,
        }
    }
}

/// Stable renderer palette choices; a descriptor selects a palette without owning GPU code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteId {
    Reflectivity,
    Rotation,
    HailSize,
    HailSwath,
    LightningDensity,
    PrecipitationRate,
    Precipitation1h,
    PrecipitationAccum,
    Precipitation24h,
    PrecipitationType,
    FloodRecurrence,
    Cape,
    Helicity,
    UpdraftHelicity,
    Snowfall,
    ThunderProbability,
    Smoke,
    MeanSeaLevelPressure,
    Temperature,
    Dewpoint,
    Height500,
    Wind10m,
    PrecipitableWater,
}

#[derive(Debug)]
pub struct FieldDescriptor {
    pub id: FieldId,
    pub source: DataSource,
    pub family: FieldFamily,
    pub name: &'static str,
    pub description: &'static str,
    pub units: Unit,
    pub value_kind: ValueKind,
    pub aliases: &'static str,
    pub default_palette: PaletteId,
    /// Preferred isoline spacing in the descriptor's native units. `None` means the field is
    /// normally rendered as a filled raster or has no meaningful generic contour interval.
    pub default_contour_interval: Option<f32>,
    /// Published coverage. This answers whether a request is meaningful before fetching; the
    /// exact decoded grid bounds remain in `GridGeometry` provenance.
    pub valid_domain: Option<GeographicBounds>,
    /// GRIB values that indicate missing, folded, or no-coverage cells.
    pub missing_values: &'static [f32],
}

impl FieldDescriptor {
    pub fn supports_location(&self, lon: f64, lat: f64) -> bool {
        self.valid_domain
            .is_none_or(|domain| domain.contains(lon, lat))
    }

    pub fn normalize_missing(&self, values: &mut [f32]) -> usize {
        let mut masked = 0;
        for value in values {
            if !value.is_finite() || self.missing_values.contains(value) {
                *value = f32::NAN;
                masked += 1;
            }
        }
        masked
    }
    pub fn search_text(&self) -> String {
        format!(
            "{} {} {} {} {:?} {} {} {}",
            self.id.0,
            self.name,
            self.source.display_name(),
            self.source.id(),
            self.family,
            self.units.symbol(),
            self.description,
            self.aliases
        )
    }

    /// Sample an existing regular grid, with categorical/mask fields never interpolated.
    /// This reads the supplied grid; callers must supply native data for raw-value inspection.
    pub fn sample(&self, grid: &crate::mrms::MrmsField, lon: f64, lat: f64) -> Option<f32> {
        if !self.supports_location(lon, lat)
            || grid.nx == 0
            || grid.ny == 0
            || grid.nx.checked_mul(grid.ny)? != grid.values.len()
            || ![grid.lon_west, grid.lon_east, grid.lat_south, grid.lat_north]
                .iter()
                .all(|v| v.is_finite())
            || grid.lon_west >= grid.lon_east
            || grid.lat_south >= grid.lat_north
            || lon < grid.lon_west
            || lon > grid.lon_east
            || lat < grid.lat_south
            || lat > grid.lat_north
        {
            return None;
        }
        match self.value_kind {
            ValueKind::Vector => None,
            ValueKind::Categorical | ValueKind::Mask => {
                let x = (((lon - grid.lon_west) / (grid.lon_east - grid.lon_west) * grid.nx as f64)
                    as usize)
                    .min(grid.nx - 1);
                let y = (((grid.lat_north - lat) / (grid.lat_north - grid.lat_south)
                    * grid.ny as f64) as usize)
                    .min(grid.ny - 1);
                let value = grid.values[y * grid.nx + x];
                value.is_finite().then_some(value)
            }
            _ => grid.sample_bilinear(lon, lat),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_source_has_separate_stable_and_display_identity() {
        assert_eq!(DataSource::NoaaMrms.id(), "noaa-mrms");
        assert_eq!(DataSource::NoaaMrms.display_name(), "NOAA MRMS");
        assert_eq!(DataSource::NoaaMrms.to_string(), "NOAA MRMS");
        assert_eq!(DataSource::NoaaNcepModels.id(), "noaa-ncep-models");
        assert_eq!(
            DataSource::NoaaNcepModels.display_name(),
            "NOAA/NCEP models"
        );
    }

    #[test]
    fn published_domains_reject_invalid_or_outside_locations() {
        assert!(GeographicBounds::CONUS.contains(-97.0, 35.0));
        assert!(!GeographicBounds::CONUS.contains(-150.0, 60.0));
        assert!(!GeographicBounds::WORLD.contains(f64::NAN, 0.0));
    }

    #[test]
    fn conversion_preserves_dimensions_and_missing_values() {
        assert!((Unit::MilliPerSecond.convert(20.0, Unit::PerSecond).unwrap() - 0.02).abs() < 1e-8);
        assert_eq!(Unit::Millimeters.convert(25.4, Unit::Inches), Some(1.0));
        assert_eq!(
            Unit::InchesPerHour.convert(1.0, Unit::MillimetersPerHour),
            Some(25.4)
        );
        assert_eq!(
            Unit::Millimeters.convert(1.0, Unit::MillimetersPerHour),
            None
        );
        assert_eq!(Unit::Dbz.convert(f32::NAN, Unit::Dbz), None);
    }
}
