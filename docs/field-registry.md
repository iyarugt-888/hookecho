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

`wxdata::field::FieldDescriptor` now defines stable `FieldId`, source, family, value kind, units/conversions, names, descriptions, and search aliases. `wxdata::mrms::catalog` describes all 11 existing direct MRMS layer families. Separate fetch mappings retain rotation, lightning, and hail window selection and fallbacks. Existing `FieldLayer` slugs bridge to descriptors, preserving saved workspace IDs.

MRMS browser rows and source-inspector units come from this catalog. Search includes source, units, family, and aliases, with label matches ranked first. `FieldDescriptor::sample` uses nearest-cell selection for categories/masks and the existing NaN-aware bilinear sampler for continuous fields. Malformed grids and nonfinite coordinates return no sample. The API samples the supplied grid; it does not retain native grids or add a map probe by itself.

## Remaining registry work

Catalog `PaletteId` selections now route both GPU upload and legend generation through the existing shared `FieldRamp` objects. Regression tests verify that each migrated product retains its exact palette, scale, and category mapping. Reflectivity still uses the user's `.pal` table, and lightning retains its density mapping. Range values remain owned by the shared renderer scale rather than duplicated in the catalog.

The shear unit is `0.001/s`, including conversion to `s⁻¹`; NOAA's [operational GRIB2 table](https://www.nssl.noaa.gov/projects/mrms/operational/tables.php) documents the factor of 1000. This corrects the initial catalog's unit label without changing displayed values or colors.

The same NOAA table now supplies per-product missing/no-coverage sentinels. Direct MRMS fetches normalize those cells to NaN after decoding, before rendering and sampling. For example, precipitation type's `-1` and `-3` are absent data while its `0` remains the valid “no precipitation” class. Reflectivity retains valid negative dBZ values, and rotation products treat their documented zero sentinel as absent. Each product's rule lives in its descriptor; windowed path lookup resolves to the same rule.

Source-health tracking (`app/chrome/registry.rs::field_layer_is_health_tracked`) now checks `FieldLayer::descriptor().is_some()` first, so a layer the catalog already knows is tracked automatically — the layers-panel health dot and popup no longer need a matching entry hand-added to a second list the way `mrms_product`'s dispatch once did. The remaining explicit list is exactly the layers the catalog does not cover yet.

The source inspector records native and displayed grid dimensions and bounds plus the display reduction method. Scalar grids use maximum pooling; categorical grids use nearest-cell reduction. This explains the displayed texture, but the full native value array is not retained for scientific sampling.

Expose renderer range metadata and contour defaults through the inspector. Preserve native grids for scientific sampling: display pooling/smoothing must never be represented as raw source values. Explicit accumulation windows and domain specifications remain to be added. Follow with HRRR/RAP and global difference inputs, then timeline alignment and persistent browser caching. Favorites and full provenance for other sources remain future work. No roadmap phase checkbox is marked complete.

## Validation

Deterministic tests cover signed ages, unknown metadata, serialization, preservation through grid decimation, unique catalog IDs/paths, window mapping, unit conversion, categorical versus continuous sampling, missing/malformed grids, saved layer resolution, and metadata search. Existing MRMS decoding/sampling tests protect the unchanged renderer payload. Normal tests do not require live NOAA services.
