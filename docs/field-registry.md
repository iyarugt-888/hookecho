# Field architecture: first migration

This implements the provenance-first step of [ROADMAP_NEW.md](https://github.com/iyarugt-888/hookecho/blob/main/ROADMAP_NEW.md), section 29. It does not complete Phase A1.

## Existing contracts

- `products.rs` describes radar moments, not a general grid catalog.
- `wxdata::mrms::MrmsField` is the shared regular latitude/longitude grid consumed by the GPU upload path. HRRR/RAP regrid into it; global guidance reuses that machinery. Keep these APIs during migration.
- `fielddiff.rs` aligns/resamples grids and constructs derived differences. It needs both input stamps in a later migration; a single source stamp must not be copied onto a difference.
- `overlay_build.rs` tessellates vector overlays and is not a scalar-grid renderer.
- `timeline.rs` remains radar-centered. `workspace.rs` saves layer slugs and pane configuration, not fetched payloads. Preserve those identifiers and saved layouts.
- Overlay requests already have generation guards, timeouts, and source-health lanes. MRMS provenance travels in that same delivery envelope so stale deliveries cannot update the inspector independently of the displayed field.

## Implemented provenance contract

`wxdata::field::DataStamp` carries source/product identity, optional issue/run times, valid time, local receipt time, optional provider latency, forecast/derived flags, and quality. Missing metadata stays unknown. Age calculations accept a clock and retain signed durations for future valid times.

`Stamped<T>` owns the payload and its stamp. `map` preserves source provenance through display decimation. `fetch_latest_stamped` captures receipt after the complete HTTP body arrives and before decompression/decoding. MRMS products are derived analyses; provider ingest latency and undecoded quality flags are unknown. Invalid GRIB valid times return an error instead of pretending to be current.

All existing direct MRMS field-layer fetches use the stamped API. Legacy callers retain `fetch_latest`. `app/field_state.rs` accepts the upload and stamp together, while `ui/data_inspector.rs` presents metadata for enabled layers in Layer options. Failed refreshes keep the prior payload and its original timestamps; the existing request-health UI reports fetch failures. This migration adds no new network source.

## Implemented catalog contract

`wxdata::field::FieldDescriptor` now defines stable `FieldId`, typed `DataSource`, family, value kind, units/conversions, names, descriptions, and search aliases. Each source separates a stable machine ID (for cache/configuration namespaces) from its human provenance label. `wxdata::mrms::catalog` describes all 14 existing direct MRMS layer families with `DataSource::NoaaMrms`. Separate fetch mappings retain rotation, lightning, and hail window selection and fallbacks. Existing `FieldLayer` slugs bridge to descriptors, preserving saved workspace IDs.

`FieldDescriptor.valid_domain` records published geographic coverage separately from the exact
bounds on a fetched grid in `GridGeometry`. MRMS and regional model products share the published
CONUS bounds; global fields use world bounds. Sampling honors those limits, HRRR point soundings
fail outside the regional domain before opening their range-request fanout, and global point
series reject invalid coordinates before fetching.

MRMS browser rows and source-inspector units come from this catalog. Search includes source, units, family, and aliases, with label matches ranked first. `FieldDescriptor::sample` uses nearest-cell selection for categories/masks and the existing NaN-aware bilinear sampler for continuous fields. Malformed grids and nonfinite coordinates return no sample. The API samples the supplied grid; it does not retain native grids or add a map probe by itself.

## Regional model migration

All twelve existing `wxdata::model::ModelField` meanings now expose the same `FieldDescriptor`
contract as MRMS: stable product identity, typed NOAA/NCEP source identity, family/value kind,
native units, aliases, palette, missing-data policy, and an optional native-unit contour interval.
Model availability and GRIB spelling remain provider-specific in `ModelField::grib`; this keeps a
single physical field definition while still recording that, for example, NBM does not publish
updraft helicity.

The HRRR/RAP/NBM map layers bridge to these descriptors, so their existing renderer ramps now
resolve through `PaletteId`. The MSLP, 2 m temperature/dewpoint, CAPE, and 0–3 km SRH contour path
also reads its GRIB key and default interval from `ModelField` rather than a second literal table.
Intervals are stored in native units; the UI converts pressure and temperature for display and
retains the conventional rounded 5 °F temperature interval.

## Remaining registry work

Catalog `PaletteId` selections now route both GPU upload and legend generation through the existing shared `FieldRamp` objects. Regression tests verify that each migrated product retains its exact palette, scale, and category mapping. Reflectivity still uses the user's `.pal` table, and lightning retains its density mapping. Range values remain owned by the shared renderer scale rather than duplicated in the catalog.

The shear unit is `0.001/s`, including conversion to `s⁻¹`; NOAA's [operational GRIB2 table](https://www.nssl.noaa.gov/projects/mrms/operational/tables.php) documents the factor of 1000. This corrects the initial catalog's unit label without changing displayed values or colors.

The same NOAA table now supplies per-product missing/no-coverage sentinels. Direct MRMS fetches normalize those cells to NaN after decoding, before rendering and sampling. For example, precipitation type's `-1` and `-3` are absent data while its `0` remains the valid “no precipitation” class. Reflectivity retains valid negative dBZ values, and rotation products treat their documented zero sentinel as absent. Each product's rule lives in its descriptor; windowed path lookup resolves to the same rule.

Source-health tracking (`app/chrome/registry.rs::field_layer_is_health_tracked`) now checks `FieldLayer::descriptor().is_some()` first, so a layer the catalog already knows is tracked automatically — the layers-panel health dot and popup no longer need a matching entry hand-added to a second list the way `mrms_product`'s dispatch once did. The remaining explicit list is exactly the layers the catalog does not cover yet.

The source inspector records native and displayed grid dimensions and bounds plus the display reduction method. Scalar grids use maximum pooling; categorical grids use nearest-cell reduction. This explains the displayed texture, but the full native value array is not retained for scientific sampling.

Expose renderer range metadata through the inspector. Preserve native grids for scientific sampling: display pooling/smoothing must never be represented as raw source values. Explicit accumulation windows remain to be added. Follow with global difference inputs, then timeline alignment and persistent browser caching.

## Validation

Deterministic tests cover signed ages, unknown metadata, serialization, preservation through grid decimation, unique catalog IDs/paths, window mapping, unit conversion, categorical versus continuous sampling, missing/malformed grids, saved layer resolution, metadata search, total/unique model descriptors, native contour defaults, and model palette preservation. Existing MRMS decoding/sampling tests protect the unchanged renderer payload. Normal tests do not require live NOAA services.
