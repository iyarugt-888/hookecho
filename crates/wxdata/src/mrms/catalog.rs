//! Catalog of the MRMS products already supported by HookEcho.
//! Existing layer slugs are stable IDs; window functions retain their historical fallbacks.
use crate::field::{FieldDescriptor, FieldFamily, FieldId, PaletteId, Unit, ValueKind};

#[derive(Debug, Clone, Copy)]
pub enum FetchMapping {
    Fixed(&'static str),
    Rotation,
    Lightning,
    Hail,
}

#[derive(Debug)]
pub struct Product {
    pub field: FieldDescriptor,
    pub common: bool,
    pub fetch: FetchMapping,
}

impl Product {
    pub fn path(
        &self,
        rotation_minutes: u16,
        lightning_minutes: u16,
        hail_minutes: u16,
    ) -> &'static str {
        match self.fetch {
            FetchMapping::Fixed(path) => path,
            FetchMapping::Rotation => super::rotation_track(rotation_minutes),
            FetchMapping::Lightning => super::lightning_density(lightning_minutes),
            FetchMapping::Hail => super::hail_swath(hail_minutes),
        }
    }
}

pub fn find(id: &str) -> Option<&'static Product> {
    PRODUCTS.iter().find(|product| product.field.id.0 == id)
}

/// Resolve a concrete fetch path, including products with selectable windows.
pub fn find_by_path(path: &str) -> Option<&'static Product> {
    PRODUCTS.iter().find(|product| match product.fetch {
        FetchMapping::Fixed(fixed) => path == fixed,
        FetchMapping::Rotation => [30, 60, 120]
            .iter()
            .any(|&minutes| path == super::rotation_track(minutes)),
        FetchMapping::Lightning => [1, 5, 15, 30]
            .iter()
            .any(|&minutes| path == super::lightning_density(minutes)),
        FetchMapping::Hail => [30, 60, 120, 240, 360, 1440]
            .iter()
            .any(|&minutes| path == super::hail_swath(minutes)),
    })
}

pub static PRODUCTS: &[Product] = &[
    Product {
        field: FieldDescriptor {
            id: FieldId("mrms"),
            source: "NOAA MRMS",
            family: FieldFamily::Mrms,
            name: "National mosaic (MRMS)",
            description: "Every radar in the country stitched into one picture",
            units: Unit::Dbz,
            value_kind: ValueKind::Scalar,
            aliases: "reflectivity composite radar",
            default_palette: PaletteId::Reflectivity,
            missing_values: &[-99.0, -999.0],
        },
        common: true,
        fetch: FetchMapping::Fixed(super::REFLECTIVITY),
    },
    Product {
        field: FieldDescriptor {
            id: FieldId("rotation"),
            source: "NOAA MRMS",
            family: FieldFamily::Mrms,
            name: "Rotation tracks",
            description: "Where rotation has passed over the last hour — the tornado-track map",
            units: Unit::MilliPerSecond,
            value_kind: ValueKind::Scalar,
            aliases: "azimuthal shear swath",
            default_palette: PaletteId::Rotation,
            missing_values: &[0.0],
        },
        common: true,
        fetch: FetchMapping::Rotation,
    },
    Product {
        field: FieldDescriptor {
            id: FieldId("mesh"),
            source: "NOAA MRMS",
            family: FieldFamily::Mrms,
            name: "Hail size (MESH)",
            description: "Estimated largest hail size each storm is producing",
            units: Unit::Millimeters,
            value_kind: ValueKind::Scalar,
            aliases: "hail severe",
            default_palette: PaletteId::HailSize,
            missing_values: &[-1.0, -3.0],
        },
        common: true,
        fetch: FetchMapping::Fixed(super::MESH),
    },
    Product {
        field: FieldDescriptor {
            id: FieldId("lightning"),
            source: "NOAA MRMS",
            family: FieldFamily::Mrms,
            name: "Lightning density (CG)",
            description:
                "Ground strikes only: NLDN cloud-to-ground density, averaged over a window you \
                 pick. Pair it with satellite lightning (GLM) to see total vs cloud-to-ground.",
            units: Unit::StrikesPerSquareKmPerMinute,
            value_kind: ValueKind::Scalar,
            aliases: "NLDN cloud ground strikes",
            default_palette: PaletteId::LightningDensity,
            missing_values: &[-1.0, -3.0],
        },
        common: true,
        fetch: FetchMapping::Lightning,
    },
    Product {
        field: FieldDescriptor {
            id: FieldId("azshear"),
            source: "NOAA MRMS",
            family: FieldFamily::Mrms,
            name: "Rotation strength (AzShear, 0–2 km)",
            description: "Low-level rotation strength, right now",
            units: Unit::MilliPerSecond,
            value_kind: ValueKind::Scalar,
            aliases: "azimuthal shear severe",
            default_palette: PaletteId::Rotation,
            missing_values: &[0.0],
        },
        common: false,
        fetch: FetchMapping::Fixed(super::AZSHEAR),
    },
    Product {
        field: FieldDescriptor {
            id: FieldId("preciprate"),
            source: "NOAA MRMS",
            family: FieldFamily::Mrms,
            name: "Rain rate",
            description: "How hard it is coming down right now, rather than how much has fallen",
            units: Unit::MillimetersPerHour,
            value_kind: ValueKind::Scalar,
            aliases: "precipitation rainfall rate",
            default_palette: PaletteId::PrecipitationRate,
            missing_values: &[-1.0, -3.0],
        },
        common: false,
        fetch: FetchMapping::Fixed(super::PRECIP_RATE),
    },
    Product {
        field: FieldDescriptor {
            id: FieldId("qpe1h"),
            source: "NOAA MRMS",
            family: FieldFamily::Mrms,
            name: "Rain so far, 1 hour (QPE)",
            description: "How much rain has fallen in the last hour",
            units: Unit::Millimeters,
            value_kind: ValueKind::Accumulation,
            aliases: "precipitation accumulation gauge corrected Pass2",
            default_palette: PaletteId::Precipitation1h,
            missing_values: &[-1.0, -3.0],
        },
        common: false,
        fetch: FetchMapping::Fixed(super::QPE_01H),
    },
    Product {
        field: FieldDescriptor {
            id: FieldId("qpe24h"),
            source: "NOAA MRMS",
            family: FieldFamily::Mrms,
            name: "Rain so far, 24 hours (QPE)",
            description: "How much rain has fallen in the last day",
            units: Unit::Millimeters,
            value_kind: ValueKind::Accumulation,
            aliases: "precipitation accumulation gauge corrected Pass2",
            default_palette: PaletteId::Precipitation24h,
            missing_values: &[-1.0, -3.0],
        },
        common: false,
        fetch: FetchMapping::Fixed(super::QPE_24H),
    },
    Product {
        field: FieldDescriptor {
            id: FieldId("preciptype"),
            source: "NOAA MRMS",
            family: FieldFamily::Mrms,
            name: "Rain or snow (precip type)",
            description: "Rain, snow, sleet or freezing rain at the surface",
            units: Unit::Category,
            value_kind: ValueKind::Categorical,
            aliases: "precipitation flag rain snow hail",
            default_palette: PaletteId::PrecipitationType,
            missing_values: &[-1.0, -3.0],
        },
        common: false,
        fetch: FetchMapping::Fixed(super::PRECIP_TYPE),
    },
    Product {
        field: FieldDescriptor {
            id: FieldId("flashflood"),
            source: "NOAA MRMS",
            family: FieldFamily::Mrms,
            name: "Flash-flood rarity (FLASH ARI)",
            description: "How rare this much rain is here — flash-flood risk",
            units: Unit::Years,
            value_kind: ValueKind::Scalar,
            aliases: "hydrology recurrence interval ARI",
            default_palette: PaletteId::FloodRecurrence,
            missing_values: &[-999.0],
        },
        common: false,
        fetch: FetchMapping::Fixed(super::FLASH_ARI30),
    },
    Product {
        field: FieldDescriptor {
            id: FieldId("hailswath"),
            source: "NOAA MRMS",
            family: FieldFamily::Mrms,
            name: "Hail swaths",
            description: "Where hail has fallen — over the past day, or a window you pick",
            units: Unit::Millimeters,
            value_kind: ValueKind::Scalar,
            aliases: "MESH maximum severe",
            default_palette: PaletteId::HailSwath,
            missing_values: &[-1.0, -3.0],
        },
        common: false,
        fetch: FetchMapping::Hail,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_ids_and_fixed_paths_are_unique() {
        let mut ids = std::collections::HashSet::new();
        let mut paths = std::collections::HashSet::new();
        for product in PRODUCTS {
            assert!(ids.insert(product.field.id));
            assert!(!product.field.name.is_empty());
            assert!(!product.field.description.is_empty());
            let path = product.path(30, 5, 1440);
            assert!(path.starts_with("CONUS/"));
            assert!(paths.insert(path));
        }
        assert_eq!(PRODUCTS.len(), 11);
        assert!(find("hrrr").is_none());
        for product in PRODUCTS {
            assert!(std::ptr::eq(
                find_by_path(product.path(30, 5, 1440)).unwrap(),
                product
            ));
        }
    }

    #[test]
    fn window_mappings_preserve_supported_products_and_fallbacks() {
        let rotation = find("rotation").unwrap();
        for (minutes, path) in [
            (30, "CONUS/RotationTrack30min_00.50"),
            (60, "CONUS/RotationTrack60min_00.50"),
            (120, "CONUS/RotationTrack120min_00.50"),
            (999, "CONUS/RotationTrack30min_00.50"),
        ] {
            assert_eq!(rotation.path(minutes, 5, 1440), path);
            assert_eq!(find_by_path(path).unwrap().field.id, rotation.field.id);
        }
        let lightning = find("lightning").unwrap();
        for minutes in [1, 5, 15, 30, 999] {
            assert_eq!(
                lightning.path(30, minutes, 1440),
                crate::mrms::lightning_density(minutes)
            );
            assert_eq!(
                find_by_path(lightning.path(30, minutes, 1440)).unwrap().field.id,
                lightning.field.id
            );
        }
        let hail = find("hailswath").unwrap();
        for minutes in [30, 60, 120, 240, 360, 1440, 720] {
            assert_eq!(hail.path(30, 5, minutes), crate::mrms::hail_swath(minutes));
            assert_eq!(
                find_by_path(hail.path(30, 5, minutes)).unwrap().field.id,
                hail.field.id
            );
        }
        assert!(find_by_path("CONUS/unpublished_product").is_none());
    }

    #[test]
    fn product_missing_codes_do_not_erase_valid_zero_or_negative_reflectivity() {
        let precip = &find("preciptype").unwrap().field;
        let mut codes = vec![-3.0, -1.0, 0.0, 3.0, 7.0];
        assert_eq!(precip.normalize_missing(&mut codes), 2);
        assert!(codes[0].is_nan() && codes[1].is_nan());
        assert_eq!(&codes[2..], &[0.0, 3.0, 7.0]);

        let reflectivity = &find("mrms").unwrap().field;
        let mut dbz = vec![-999.0, -99.0, -30.0, 0.0, 60.0];
        assert_eq!(reflectivity.normalize_missing(&mut dbz), 2);
        assert_eq!(&dbz[2..], &[-30.0, 0.0, 60.0]);

        let mut shear = vec![0.0, -12.0, 18.0];
        assert_eq!(
            find("azshear").unwrap().field.normalize_missing(&mut shear),
            1
        );
        assert_eq!(&shear[1..], &[-12.0, 18.0]);
    }

    #[test]
    fn categorical_sampling_never_creates_an_intermediate_class() {
        let mut grid = crate::mrms::MrmsField {
            values: vec![1.0, 7.0],
            nx: 2,
            ny: 1,
            lon_west: -100.0,
            lon_east: -98.0,
            lat_north: 40.0,
            lat_south: 39.0,
            time: chrono::DateTime::from_timestamp(1_000, 0).unwrap(),
        };
        let categorical = &find("preciptype").unwrap().field;
        assert_eq!(categorical.sample(&grid, -99.0, 39.5), Some(7.0));
        assert_eq!(
            find("mesh").unwrap().field.sample(&grid, -99.0, 39.5),
            Some(4.0)
        );
        assert_eq!(categorical.sample(&grid, -98.0, 39.0), Some(7.0));
        assert_eq!(categorical.sample(&grid, f64::NAN, 39.5), None);
        assert_eq!(categorical.sample(&grid, -101.0, 39.5), None);
        grid.values[1] = f32::NAN;
        assert_eq!(categorical.sample(&grid, -99.0, 39.5), None);
        grid.values.clear();
        assert_eq!(categorical.sample(&grid, -99.0, 39.5), None);
    }
}
