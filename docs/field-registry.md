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

## Next registry contract

Introduce stable `FieldId` descriptors with source, family, value kind, units/conversions, palette/range, contour defaults, missing semantics, native grid/domain, aliases, and sampling capabilities. Keep source fetch mappings separate from descriptors and bridge existing `FieldLayer` slugs to descriptors so saved workspaces remain valid.

Generate MRMS browser rows and legends from descriptors. A common sampling API must dispatch categorical grids to nearest-cell sampling and explicitly describe interpolation for continuous values. Preserve native grids for scientific sampling: display pooling/smoothing must never be represented as raw source values. Native resolution and display transforms belong beside the field metadata, not guessed from the source name.

Follow with HRRR/RAP and global difference inputs, then timeline alignment and persistent browser caching. Favorites, the catalog, native-grid sampling, and full provenance for other sources remain unimplemented in this change. No roadmap phase checkbox is marked complete.

## Validation

Deterministic tests cover signed ages, unknown metadata, serialization, and preservation through grid decimation. Existing MRMS decoding/sampling tests protect the unchanged renderer payload. Normal tests do not require live NOAA services.
