# HookEcho — U.S. Analyst-Grade Roadmap

> **Purpose:** implementation roadmap for Claude Code, Codex, and human contributors.
>
> **Scope:** United States weather analysis only. Do **not** spend roadmap effort on international radar, international model coverage, or international warning products. Existing international code may remain working, but it is outside this roadmap.
>
> **Relationship to `ROADMAP.md`:** this file is additive. Do not replace or delete the existing roadmap. This document is the detailed path from the current HookEcho feature set to a top-tier U.S. meteorological analysis workstation comparable in breadth to RadarOmega / WeatherFront / RadarScope, in low-latency presentation to WeatherWise / WSV3, and in radar-analysis depth to GR2Analyst.
>
> **Snapshot date:** 2026-09-12.

---

## 0. End state

HookEcho should become a **professional U.S. severe-weather and mesoscale analysis workstation**, not merely a radar viewer.

The completed product should provide:

1. Near-real-time NEXRAD Level II display with progressive in-progress sweep updates, exact latency/provenance, and automatic feed failover.
2. Full Level II / Level III / TDWR / MRMS analysis with analyst-grade probing, derived products, 2D/3D cross sections, multi-moment 3D, and programmable user-defined products.
3. A metadata-driven MRMS browser covering the useful operational catalog rather than a short hard-coded subset.
4. Native GOES ABI ingest for CONUS/full disk/mesoscale sectors, including 1-minute mesoscale imagery, all ABI channels needed for operational analysis, GLM, and derived RGB recipes.
5. A generic model engine capable of HRRR, RAP, GFS, RRFS/REFS, GEFS, NBM and additional U.S.-relevant guidance without writing a custom renderer for every field.
6. Deterministic, ensemble, run-to-run, model-to-model, observed-vs-forecast and analysis-vs-forecast comparison workflows.
7. RTMA/URMA-quality surface analysis, improved observation blending, soundings, hodographs, objective analysis, and severe-weather diagnostics.
8. AWIPS-like multi-pane workspaces with linked time/cursor/map position and synchronized data probes.
9. Full GIS import/export and time-aware overlay support for professional emergency-management and research workflows.
10. Chase/navigation tooling that combines radar, warnings, storm motion, routing, offline data, and position sharing without claiming any route is “safe.”
11. Broadcast/headless/export tooling suitable for clean live graphics, automated imagery, video loops, and machine-readable data export.
12. Repeatable historical verification and backtesting for algorithms, alerts, warning performance, and derived products.
13. Reliable Windows/Linux/Android/Web operation with explicit data-source health, stale-data behavior, caching, resource budgets and reproducible tests.

### Definition of “top tier analyst tool”

A feature is not complete merely because it can be displayed. Analyst-grade features must expose:

- **source**
- **issue/run time**
- **valid time**
- **ingest time**
- **display latency / age**
- **units**
- **native grid / resolution**
- **interpolation / smoothing state**
- **quality flags where available**
- **derived-vs-observed-vs-forecast classification**
- **point sampling**
- **exportability** where practical

Every major layer must be usable in both a “quick visual” workflow and a “prove exactly what I am looking at” workflow.

---

# 1. Preserve what HookEcho already does well

Do **not** reimplement these from scratch unless refactoring is necessary:

- all six NEXRAD Level II base moments
- super-resolution polar rendering
- velocity dealiasing
- storm-relative velocity
- GRLevelX `.pal` support
- split panes / four panes
- multi-tilt views
- cross sections
- CAPPI
- current 3D volume view
- Level II archive back to 1991
- archived warnings and local storm reports on the timeline
- warning verification metrics
- SCIT cell tracks and storm table
- client-side tornado debris signature detection
- client-side rotation/couplet detection
- ProbSevere
- VIL / VIL density / echo tops derived from the Level II volume
- hail-size / severe-hail calculations already implemented
- MRMS reflectivity / QPE / MESH / rotation / azimuthal shear / precip type / FLASH products already implemented
- GLM support
- HRRR future reflectivity / forecast wind / smoke / rotation tracks
- RAP analysis and existing severe parameters
- Skew-T / hodograph / VAD / observed RAOB comparison
- effective-layer severe parameters
- SPC / WPC / NHC / aviation / winter layers already present
- METAR / NDBC / PWS layers
- chase GPS / offline chase packs / position sharing
- workspaces
- placefiles
- headless snapshots
- GIF / MP4 export
- OBS / streamer mode
- web app
- Android app
- storage/cache UI

The new architecture should make these capabilities easier to extend rather than replacing proven code.

---

# 2. Mandatory engineering rules for coding agents

These rules apply to every roadmap phase.

## 2.1 Do not grow `app.rs` further

`crates/hookecho/src/app.rs` is already extremely large. New substantial systems must not be implemented as more giant blocks in `app.rs`.

Before or during each feature:

- place acquisition/decoding/domain logic in `crates/wxdata`
- place rendering logic in `crates/hookecho/src/render/`, focused renderer modules, or shaders
- place persistent app state in dedicated structs/modules
- place UI panels under `crates/hookecho/src/ui/` or `crates/hookecho/src/app/`
- make `app.rs` orchestrate rather than own every algorithm

**Soft target:** reduce `app.rs` over time by moving cohesive systems out of it. Do not block feature work on a single massive refactor, but every major phase should leave it smaller or no larger.

## 2.2 No source-specific UI logic when a registry can solve it

Do not add 80 MRMS products as 80 booleans, 30 model fields as 30 UI branches, or GOES channels as hard-coded menu fragments.

Build reusable metadata registries.

Each gridded product should eventually describe itself with something equivalent to:

```rust
pub struct FieldDescriptor {
    pub id: FieldId,
    pub source: DataSource,
    pub family: FieldFamily,
    pub display_name: &'static str,
    pub short_name: &'static str,
    pub units: UnitSpec,
    pub value_kind: ValueKind,
    pub default_palette: PaletteId,
    pub native_resolution: ResolutionSpec,
    pub accumulation: Option<Duration>,
    pub vertical_level: Option<VerticalLevel>,
    pub supports_contours: bool,
    pub supports_vectors: bool,
    pub supports_sampling: bool,
    pub supports_difference: bool,
}
```

The exact names may differ, but the architectural principle is mandatory.

## 2.3 Universal time/provenance model

Add a common metadata object used by radar, satellite, MRMS, models and observations:

```rust
pub struct DataStamp {
    pub source_id: String,
    pub product_id: String,
    pub issue_time: Option<DateTime<Utc>>,
    pub run_time: Option<DateTime<Utc>>,
    pub valid_time: DateTime<Utc>,
    pub received_time: DateTime<Utc>,
    pub source_latency: Option<Duration>,
    pub is_forecast: bool,
    pub is_derived: bool,
    pub quality: QualitySummary,
}
```

Every layer legend/inspector should be able to answer “what exactly is this and how old is it?” without source-specific code.

## 2.4 Cancellable background work

All heavy fetch/decode/regrid/derived-product jobs must:

- run off the UI thread
- be cancellable when the user changes product/time/site
- ignore stale results using request generation IDs
- expose loading/error/stale status
- avoid duplicate identical downloads

On WASM, use the existing worker approach or additional Web Workers where necessary.

## 2.5 Source failures must degrade, not crash

Every remote feed must have:

- timeout
- retry with bounded exponential backoff
- stale cache fallback where meaningful
- source health state
- human-readable error
- no panics on malformed remote data

## 2.6 Tests are part of the feature

Every phase below includes acceptance tests. Do not mark a checkbox complete until its tests exist.

## 2.7 Preserve HookEcho’s privacy model

Do not add mandatory accounts, telemetry, analytics or a hosted HookEcho backend as a prerequisite for core weather data.

Optional user-configured relays/providers are allowed when required for specialized low-latency or collaboration workflows.

---

# 3. Phase A — Core product/data architecture

**Priority: P0. Required before broad model/MRMS/satellite expansion.**

## A1. Generic field/product registry — done

This section's checkboxes had gone stale: most of it was already implemented (`wxdata::field` +
`wxdata::mrms::catalog`, plus `ui::data_inspector`) by the time this pass looked, without the
roadmap being updated to say so — the same pattern as C3's terrain-blockage discovery. Corrected
here rather than rebuilt.

### Implement

- [x] `FieldId` stable identifier — `wxdata::field::FieldId(&'static str)`
- [x] `DataSource` enum/ID — `FieldDescriptor.source` is a typed `DataSource`; each variant exposes
  a stable machine ID for cache/configuration namespaces and a separate human display name.
  The migrated MRMS catalog uses `DataSource::NoaaMrms`, and search includes both identities.
- [x] `FieldFamily`: radar / MRMS / satellite / model / analysis / observation-derived / user-defined
- [x] `ValueKind`: scalar / categorical / vector / probability / accumulation / mask
- [x] unit metadata and conversion — `Unit::symbol`/`Unit::convert`, dimension-checked (rejects
  e.g. mm → mm/hr), missing/non-finite values never silently convert
- [x] default palette — `PaletteId`, a stable enum the renderer maps to an actual color table
- [x] contour interval defaults — `FieldDescriptor.default_contour_interval` stores native-unit
  spacing. The existing HRRR/RAP MSLP, temperature, dewpoint, CAPE and SRH contour path now reads
  both its GRIB key and interval from `ModelField` metadata; display conversion still rounds the
  customary Fahrenheit spacing to 5 °F.
- [x] missing-data semantics — `FieldDescriptor::normalize_missing` masks a product's own sentinel
  values (and non-finite ones) to NaN once, at the boundary, so nothing downstream has to know a
  product's magic numbers
- [x] valid domain / bounds — `FieldDescriptor.valid_domain` carries published coverage through
  `GeographicBounds`, distinct from each response's actual `GridGeometry`. CONUS MRMS and regional
  model descriptors use the shared published CONUS domain; global fields use world bounds.
  Common sampling enforces the product domain, and point HRRR soundings/global meteograms reject
  unsupported or invalid coordinates before issuing network requests.
- [x] native grid metadata — `wxdata::field::{GridGeometry, GridProvenance, DisplayTransform}`:
  native vs. displayed grid dimensions/bounds and which reduction (`Native` /
  `MaximumPool{factor}` / `NearestCell`) got a field there, wired into the real fetch path
  (`Stamped::for_display`) and shown in `ui::data_inspector`
- [x] product search aliases — `FieldDescriptor.aliases` + `search_text()`, feeding the same fuzzy
  search every other action in the app uses
- [x] favorite/recent products — see the Unreleased CHANGELOG entries: `Settings.recent_layers`
  (most-recent-first, capped, a "RECENT" section above the Layers panel's category grid) and
  `Settings.favorite_layers` (a star on every layer row, no cap, a "FAVORITES" section above
  "RECENT").
- [x] source/provenance inspector — `ui::data_inspector`: source, product, valid time (with signed
  offset from the pane's analysis time), received time, age, issue/run time, forecast/derived
  flags, quality, and the grid-transform detail above. This is `DataStamp` (§2.3's own mandatory
  provenance object) rendered, not a separate ad hoc panel.

### Integrate first

Migrate existing:

- [x] MRMS fields from `crates/wxdata/src/mrms.rs` — `wxdata::mrms::catalog`, 14 products,
  generated Layers-panel/search rows, feed-contract-tested (see Phase D below)
- [x] HRRR/RAP gridded fields — all 12 existing `wxdata::model::ModelField` meanings now expose a
  `FieldDescriptor` with stable ID, typed NOAA/NCEP source, units, value kind, aliases, palette and
  contour default. The map's HRRR/RAP/NBM layers resolve those descriptors, and the contour fetch
  path no longer maintains a second literal GRIB/interval table. Provider-specific availability
  and GRIB spelling remain in `ModelField::grib`, where feed contract tests already cover them.
- [x] global model fields used by `fielddiff.rs` — `wxdata::global::GlobalField` (Mslp/Height500/
  Temp2m/Dewpoint2m/Wind10m/Precip) now expose a `FieldDescriptor` the same way, under a new
  `DataSource::GlobalModels` (GFS/ECMWF/GEFS/GDPS aren't one publisher, so this names the class of
  model rather than a single agency the way `NoaaNcepModels` could). `FieldLayer::descriptor()`
  resolves the six `Global*` layers through it, so they pick up provenance
  (`ui::data_inspector`)/search/palette-by-metadata for free, same as the HRRR/RAP row above.
  `fielddiff.rs`'s own difference-specific display scaling (`DiffField::units`/`range`/
  `input_scale`, which describe the *subtracted* value in forecaster-facing units like hPa/dam/kt,
  not the field's own native GRIB units) is a genuinely separate concept from a source field's
  descriptor and is correctly left as its own table, not folded in.

Do not migrate every layer at once. Prove the registry on those three families, then use it for all new work.

### Acceptance criteria

- [x] adding a new MRMS scalar product requires a descriptor + fetch mapping, not a new menu
  implementation — verified by `mrms::catalog::the_mrms_catalog_paths_are_real` existing at all,
  and by the Layers panel/search generating its MRMS rows straight from `catalog::PRODUCTS`
- [x] the layer browser can search by product name, source, unit and category — `search_text()`
  folds all of these into one fuzzy-matched string
- [x] legends are created from product metadata — migrated layers resolve
  `FieldDescriptor.default_palette` through `render::field_ramps::ramp_for`; both
  `field_upload_indexed` (GPU colors) and `ui::legend::draw_field` consume that same `FieldRamp`.
  `catalog_palettes_preserve_existing_scales` covers every migrated fixed ramp and the explicit
  reflectivity/lightning paths, while `every_layer_is_either_ramped_or_explicitly_exempt` prevents
  a new layer from silently shipping without a legend.
- [x] sampling uses one common API — `FieldDescriptor::sample`, categorical/mask fields always
  nearest-neighbor, everything else bilinear
- [x] provenance UI works for migrated products — `ui::data_inspector`, as above

---

## A2. Unified timeline alignment engine — done

The current timeline is radar-centered. Convert it into a general valid-time coordinator.

### Add time policies

- `Exact`
- `Nearest`
- `NearestPast`
- `InterpolateLinear` where scientifically valid
- `HoldLast` for warnings/observations
- `ForecastLead`

### Implement

- [x] one selected analysis time shared across panes — `LinkedTimeState::cursor` retains one live
  or archive instant across focus/site changes and seeks every linked NEXRAD pane against it
- [x] per-layer time offsets visible in the UI — linked pane badges, the source inspector, WSV3
  status bar and GOES controls show signed source-minus-analysis offsets
- [x] valid-time alignment for the currently offered GFS/ECMWF and HRRR/RAP differences
- [x] run-time alignment for run-to-run comparison — new this pass, see the Unreleased CHANGELOG
  entry and F5 below: `wxdata::hrrr::fetch_field_previous_run` walks back from a specific run
  (not `Utc::now()`) so it can never return the same cycle it's meant to be compared against
- [x] radar/satellite/MRMS nearest-frame synchronization — linked radar panes seek their nearest
  scan, GOES follow mode selects against the retained cursor, and catalog MRMS requests use the
  nearest archive object within tolerance while refusing stale/live fallbacks
- [x] “lock all panes to valid time” toggle — the saved `Link pane analysis time` action controls
  this behavior and starter workspaces can enable it
- [x] “lock to source frame” option for exact radar analysis — the saved
  `Lock analysis to radar frame` action snaps an external valid-time request to the active
  radar's actual settled scan before the other panes, GOES and MRMS align
- [x] explicit warning when sources differ by more than a configurable tolerance —
  `Settings.time_mismatch_minutes` drives pane badges, layer status and source-inspector warnings

The shared `wxdata::time_align::TimePolicy` selector implements `Exact`, `Nearest`, `NearestPast`,
`HoldLast`, scalar/probability-only `InterpolateLinear`, and run-qualified `ForecastLead`, with
tolerance and cross-run interpolation guards covered by deterministic tests.
Current increment: stamped MRMS fields show their signed offset from the displayed radar scan
in the data inspector. The layer panel, WSV3 status bar, and GOES time control warn beyond a
shared configurable threshold (10 minutes by default).
An opt-in **Link pane analysis time** control now seeks each NEXRAD pane to its own nearest
volume when the active pane scrubs, with UTC scan times and offsets shown on every pane. Saved
workspaces retain the link. Live panes continue polling their own heads; model and MRMS time
coordination remains to be built.
The current model-difference and side-by-side pairs now require one exact valid time and verify
the decoded GRIB timestamp. The map shows that time, while Layer settings show each run and lead;
an unavailable pair hides any previous grid instead of displaying a mismatched subtraction.
General run-to-run and broader model comparison remain open.
Single-model global, HRRR/RAP and NBM grid overlays now carry decoded valid time and model run
into the existing per-layer provenance inspector. Difference and side-by-side layers also show
their own source clocks there. The shared tolerance flags model/radar offsets; these overlays
remain live forecasts rather than being sought to the radar archive cursor.
Linked radar panes now retain an explicit analysis instant across focus and site changes, so
selecting a nearby scan in another pane cannot drift the whole layout. GOES follow mode uses
that instant for frame selection and its time-offset readout, even while a radar scan is loading.
With GOES set to follow analysis time, stepping its frame arrows moves the shared cursor and
seeks the radar panes; the GOES Latest button returns the linked view to live.
MRMS catalog layers now select the nearest archived S3 frame within the shared tolerance when
the linked cursor is scrubbed. The decoded GRIB valid time is checked against that target;
an unavailable frame leaves the layer hidden rather than painting a previous live grid. Returning
to live refreshes the current field. Current-only local mosaic and snow-band composites are hidden
while linked archive mode is active. Model archive seeking and independent per-pane GOES frame
caches remain open. Run-to-run model alignment remains open.

### Acceptance criteria

Opening radar + GOES + MRMS + HRRR in four panes and scrubbing time keeps all panes at the nearest scientifically appropriate valid time while showing each layer’s exact source time.

---

## A3. Data cache abstraction — done

Native and browser persistence both exist, behind one interface.

Radar archive volumes are done (`crates/hookecho/src/webcache.rs`'s `auto_cached_volume`/
`spawn_auto_cache_put`, wired through `volume::fetch`'s new `archived` flag): an IndexedDB store
separate from the explicit offline-pack store, LRU-evicted under its own byte cap, mirroring what
native's disk cache already did for the same data.

Model, MRMS and satellite data now share one cache on every platform: `wxdata::objcache`, an async
`ObjectStore` interface (get/put by space and key) with `cached()` doing read-through with
verification both ways. What goes in it is what never changes once published — a GRIB2 message of
a run (keyed by URL and byte range), an MRMS file (its key names its minute), a GOES scan (its key
names its start time) — so "cached" means "kept", with no TTL: the *listings* that find the newest
file still go to the network every time, which is where freshness lives. The desktop and phone
register a disk store (`hookecho::object_store`, grown out of the GRIB-only store; its old
`gribcache/` folder is renamed on first run), the web build an IndexedDB store (`webcache.rs`'s
`objects`/`objects_meta`, database version 3). Six spaces, each with its own quota: HRRR, GEFS,
RTMA and other-model GRIB, MRMS, GOES.

### Implement

- [x] common cache interface for native and WASM — `wxdata::objcache::ObjectStore`, implemented
  by the disk store and by IndexedDB; `StoreFuture` is `Send` natively and not on the web
- [x] browser IndexedDB or OPFS persistence — radar archive volumes (auto-cache) and now every
  model GRIB message, MRMS grid and GOES scan (object cache)
- [x] cache namespaces by source/product/run - GRIB messages: `wxdata::gribcache` keys on the full file URL (source, run, cycle, product, lead) plus byte range
- [x] size quota per source family - 256 MB each for HRRR, GEFS, RTMA, other models and MRMS,
  512 MB for GOES; the smaller quotas (64 MB, 96 MB for GOES) on Android and in the browser; swept
  oldest-read-first
- [x] LRU eviction — radar archive volumes and every object-cache space, on disk and in
  IndexedDB (each space evicts only its own entries)
- [x] immutable object cache for archived frames — radar volumes, MRMS grids, GOES scans
- [x] partial/range-response caching where useful for GRIB - every model, RTMA and GEFS read, on
  every platform now
- [x] checksum/content-length verification when available - GRIB kept only at exactly the
  requested length with GRIB and 7777 framing; MRMS only when the gzip decompresses to its end
  with its CRC-32 trailer matching; GOES only when the file is exactly the end-of-file address its
  HDF5 superblock declares. All re-verified on read, so a damaged entry is a refetch
- [x] storage statistics in existing Storage UI — new this pass, see the Unreleased CHANGELOG
  entry: the Storage tab (cache sizes plus Clear buttons) was native-only outright; it's now
  unconditional, with a web-build view of `webcache.rs`'s IndexedDB stores (auto-cache bytes/count
  with a Clear button, offline-pack bytes/count read-only since per-pack delete already lives in
  the timeline's archive menu). Verified live: 143.8 MB / 26 volumes shown after scrubbing, Clear
  brought it to 0 B with no reload.

### Acceptance criteria

Reloading the web app does not redownload unchanged radar/model/satellite data already cached locally, within configured quotas.

Checked in the browser build (`scripts/web/build.sh`, served with `--serve`): turning on HRRR
reflectivity fetched the `.idx` and one 382 KB message (206) and stored it in IndexedDB
(`objects`, key `hrrr/…wrfsfcf01.grib2#0-382562`). A marker byte was flipped inside the stored
copy, the page reloaded and the layer turned back on: only the `.idx` was requested, the entry's
`last_used` moved, the marker was still there (so nothing re-downloaded and overwrote it), and the
forecast drew. MRMS and GOES go through the same `cached()`; natively a live MRMS grid and GOES scan
were each stored once (an ignored network test in `object_store.rs`). Not verified in the browser
for MRMS: the web build's MRMS layer made no request at all during the test, cache or no cache —
a separate issue to look into.

---

# 4. Phase B — Real-time NEXRAD / low-latency radar

**Priority: P0. This is the biggest operational-radar upgrade.**

Current HookEcho live chunks are already fast, but a top-tier radar workstation should show what has arrived **inside an in-progress sweep**, expose latency, and tolerate provider failures.

## B1. Radar provider abstraction — mostly done

Create a provider trait around Level II live acquisition.

Conceptually:

```rust
trait Level2LiveProvider {
    async fn health(&self) -> ProviderHealth;
    async fn subscribe_site(&self, site: RadarSite, tx: Sender<RadialUpdate>);
    async fn latest_complete_volume(&self, site: RadarSite) -> Result<Volume>;
}
```

`crates/hookecho/src/volume.rs` now has this (as `Level2LiveProvider`/`UnidataLevel2Provider`,
`subscribe`/`latest_complete_volume`, close to the roadmap's own sketch — boxed closures rather
than a channel `Sender`, matching this codebase's existing callback-based streaming API). Deliberately
**not** `dyn`-safe yet: nothing needs to pick a provider at runtime with only one implementation
in existence, and boxing every method for a hypothetical second one before it exists is the
premature abstraction section 2's own rules warn against. `app.rs`'s `spawn_stream`/`spawn_fetch`
route through it now instead of calling `wxdata::live::stream`/`wxdata::level2::latest_identifiers`
directly — a mechanical move, not a rewrite: same functions, same fallback-to-the-previous-volume
logic, verified against the exact ported logic with a live network test
(`volume::tests::latest_complete_volume_finds_a_new_one_then_reports_up_to_date`) before wiring it
in, then confirmed live (`cargo run` logged "live stream started for ..." same as before).

### Providers

- [x] current Unidata/AWS chunk source — wrapped, not rewritten
- [x] completed-volume fallback from NOAA/AWS archive/current objects where applicable — same
  two-candidate-with-fallback logic `spawn_fetch` always had, now owned by the provider
- [x] independent HookEcho backend Level II ingest/relay — done via B6 (that section's own status
  has the full detail): `radar-ingest` is a self-hostable service (`crates/radar-ingest`,
  Dockerfile + docker-compose example), `HookEchoRelayLevel2Provider` is the matching client-side
  `Level2LiveProvider`, and clients never run LDM themselves — a deployment's own permitted
  upstream feeds `radar-ingest`, not the desktop/web/Android app. The one line item B6 itself still
  can't close: a live LDM/IDD wire-protocol adapter, blocked on a genuine external dependency (a
  real upstream peer to develop and validate the handshake against, not available in this
  environment) — `RADAR_INGEST_REPLAY_FILE` fixture replay stands in for it today.

**Do not claim sub-10-second performance unless the active provider actually supplies data that quickly.** The UI must report measured latency rather than marketing a fixed number.

## B2. Progressive radial rendering — partly done

Instead of waiting for a sweep/volume boundary:

- [x] decode and publish radial blocks as they arrive — `stream()` emits on **every chunk**
  (~120 radials, a 60° wedge of super-res) rather than only on the chunk that completes a sweep.
  The emit window advances each time, so the total assembly work is about what the per-sweep path
  cost, not six times it.
- [x] update GPU polar texture incrementally — `RadarGpu` retains a CPU mirror of its persistent
  polar texture, diffs it by azimuth row, coalesces adjacent changed rows, and writes only those
  spans. A live chunk normally becomes one texture write; a wedge crossing north becomes two.
- [ ] stop cloning unchanged live sweeps — cached plain moments now update only azimuth rows whose
  radial acquisition timestamp advanced (`update_binned_sweep_live`), with full re-bins retained
  for KDP and dealiased velocity because they require whole-field context. `merge_scan` still
  deep-clones the merged scan on each emit; removing that copy requires an upstream mutable/move
  API for `nexrad_model::Scan` and is the remaining general CPU-side cost item.
- [x] preserve previous sweep underneath not-yet-updated azimuths — `stitch()` keeps the older
  radial in any azimuth the new pass has not reached (bounded to 15 min) instead of replacing the
  whole tilt and blanking the unswept sectors.
- [x] visually distinguish “new scan”, “old scan” and “not yet received” — `BinnedSweep` carries
  per-azimuth acquisition times and derives the carried-over wedge from the largest gap between
  them (30 s floor, so a slow clear-air rotation isn't split in two); `radar.wgsl` dims that wedge
  and the gate inspector reports each sampled gate's own time and its lag behind the newest radial
  in the tilt. "Not yet received" is not a separate state here — nothing is ever blank, because a
  bin either holds the new pass or the retained one. Verified on a real GPU (`headless.rs`'s
  `a_stale_wedge_renders_dimmer_and_only_where_it_should`), not only in unit tests.
- [x] expose current elevation, VCP, sweep number and scan progress — `wxdata::live::ScanProgress`
  (elevation number/angle, total elevations, chunk index/count, refreshed azimuth sector and VCP
  azimuth rate within the sweep), read straight off metadata the vendored `ElevationChunkMapper`
  already derives from the VCP per chunk. `stream()` takes a second `on_progress` callback
  alongside the existing `on_update`, firing on every chunk rather than only at sweep boundaries;
  threaded through
  `Level2LiveProvider::subscribe` and a new `DataMsg::LiveProgress`, landing in
  `MapView::live_progress`. Surfaced today as extra detail in
  the scrubber's "Live" badge tooltip ("Sweep 3/12 at 0.9°, chunk 2/6") whenever a chunk stream —
  not interval polling — is feeding the pane. Verified live: the reading advanced chunk-by-chunk
  against a real site. The progress state now survives the partial merge it describes (it was
  previously cleared by the immediately-following `DataMsg::Live`) and is cleared only when the
  stream ends or the site changes.
- [x] animate partial 2D refreshes with a live sweep bar — new this pass: when a real block for the
  currently displayed tilt lands, a dark-keyed lime beam sweeps clockwise through its actual
  refreshed sector at the cut's VCP-declared azimuth rate, with a short direction tail and fade.
  Direct Unidata chunks supply their 60°/120° bounds; the self-hosted relay now emits the same
  provider-neutral progress event from each block's first/last radial and decoded VCP rather than
  silently dropping the callback. It is absent for other tilts, 3D, archive playback,
  interval-polled completed volumes and stalled streams, so motion never fabricates freshness.
  Reduced-motion mode holds the arrived edge and
  schedules only its removal. Tests cover sector boundaries, north wrap, VCP speed, invalid
  metadata, tilt matching, relay WebSocket progress and actual painted beam/tail geometry.
- [x] show age since radar timestamp and age since local receipt separately — **done** via B3's
  provider-lag reading (`View::last_live_arrival`, see below); not duplicated here.
- [x] keep both 3D representations synchronized with progressive live updates — every accepted
  merged chunk advances `MapView::live_scan_revision`, which is part of the observed-gate upload
  and smooth-volume resample cache keys. New wedges inside an existing tilt and repeated
  SAILS/MRLE low-level cuts now invalidate 3D even when the volume name and tilt count stay the
  same.
- [ ] keep animation smooth while updates stream

What remains is the *incremental GPU upload*: the display is now correct and honest about
generations. The observed 3D renderer now retains its GPU buffer, LUT texture and bind group across
live revisions and grows the buffer geometrically, so a chunk no longer recreates all of those
resources; each observed-3D emit still writes the whole gate buffer rather than only the radial
range that actually changed. That and the CPU-side clone/re-bin above are the remaining cost items
in this section.

## B3. Latency dashboard — done

Add a compact source/latency diagnostic:

- beam/radial timestamp where available — **done**, at the per-gate granularity that's actually
  useful (not per-radial, which arrive within about a second of each other within a sweep and
  wouldn't earn their own reading): the Gate inspector's (B4) "Sweep time" is the wall-clock span
  the clicked gate's sweep was collected over, aggregated across every pass at that elevation so a
  repeated SAILS/MRLE cut reports its full span.
- current wall-clock difference — **done**, predates this phase: the scrubber's "Scan ⟨n⟩ ago"
  readout, now driven by `Timeline::newest()` (see the "latest complete volume age" entry below).
- provider ingest delay — **done**: `View::last_live_arrival` records `(received_at, valid_time)`
  at every live-poll and live-stream arrival (never an archive scrub or a loop's replayed frame);
  the Radar row's health popup (Layers panel) shows it as "Provider lag". Verified live.
- decode/render delay — **done**: `wxdata::live::Update::decode_time` measures assembly and merge
  time. A one-shot timestamp then follows each accepted live update through binning and the render
  callback; after the polar texture spans, uniforms, and LUT are queued, the renderer records the
  receipt-to-queue duration. The Radar health popup shows both "Decode time" and "Render queue".
  This is a CPU/GPU queue boundary measurement, not a claim that the GPU has finished presenting.
- latest complete volume age — **done** via B2-adjacent work: `Timeline::newest()` (added while
  fixing the LIVE badge, see Unreleased/CHANGELOG) is the site's own newest known frame,
  independent of what a rolling loop is currently displaying; the badge, its age readout, and
  `radar_health()` all read it now instead of the displayed volume.
- dropped/retried chunks — **done**: `wxdata::live::Update::retries` counts chunk fetches retried
  (not dropped outright — a stream that exhausts its retries ends instead) since the current
  stream connection started; shown as a "Stream retries" line in the same health popup as provider
  lag, only once it is above zero. Unit-tested; not independently pixel-confirmed live this round
  since triggering a real retry needs an actual network hiccup — see CHANGELOG for the honest
  caveat.
- provider failover state — **done** via B6.9: the same health popup's "Active provider",
  "Standby provider", "Failover state" (`PRIMARY`/`BACKUP`/`DEGRADED_VOLUME`/`MANUAL`), and "Last
  transition" lines (`registry.rs::failover_details`), automatically following whichever tier
  `radar_provider_manager::SiteProviders` currently has selected. This app now has up to three
  providers per site (primary Unidata, backup relay, degraded TGFTP) where B6 was not started
  when this line was originally written.

## B4. Radar metadata inspector — done

Clicking the radar with nothing more specific under the click (a marker, a storm cell, an
overlay feature) opens a gate inspector (`crates/hookecho/src/ui/gate_inspector.rs`), fed by
`wxdata::level2::BinnedSweep::inspect`/`sweep_time_range`. Verified live against real reflectivity
and velocity gates, folded and unfolded.

- [x] radar site
- [x] VCP
- [x] elevation angle
- [x] azimuth
- [x] slant range
- [x] ground range
- [x] beam center height using current 4/3-earth model (`beam_geometry.rs`/`xsection.rs`, already
  implemented before this phase; this just wired an inspector to it)
- [x] gate spacing
- [x] raw product value
- [x] dealiased value where relevant (velocity only)
- [x] Nyquist velocity — **estimated**, not decoded: the vendored `nexrad-data` decoder never
  extracts the true unambiguous-velocity field from the raw message header, so this reads
  `BinnedSweep::estimated_nyquist_mps` (the same largest-observed-|v| proxy dealiasing itself
  relies on) instead, labeled "Nyquist velocity (est.)" in the UI so it is never mistaken for an
  instrument reading. Decoding the true value would mean extending the vendored crate itself —
  out of scope here.
- [x] range-folded/missing state where available
- [x] sweep timestamp (aggregated across every sweep at that elevation, so a repeated
  SAILS/MRLE cut reports the full span it was collected over, not one arbitrary pass)

## B5. VCP / SAILS / MESO-SAILS awareness — partly done

- [x] parse/display current VCP details — the ribbon's VCP chip (`crates/hookecho/src/app/chrome/
  ribbon.rs`) is now clickable and opens a popup with the full pattern description and a per-tilt
  table. Nothing new to parse: `nexrad_model::data::VolumeCoveragePattern` already carried
  `sails_enabled`/`mrle_enabled`/`elevation_cuts` (with `is_sails_cut`/`is_mrle_cut`/
  `is_base_tilt_cut` per cut) from decode, entirely unused by the app until now.
- [x] identify repeated low-level cuts — `wxdata::level2::tilt_cuts` (unit-tested with a
  constructed VCP 212-shaped pattern) reports, per tilt, how many sweeps one volume takes there and
  how many are SAILS/MRLE, read from the decoded VCP rather than inferred from sweep counts seen so
  far (so a not-yet-arrived live SAILS insert is still correctly identified). Correctly leaves an
  ordinary split cut's second pass (e.g. VCP 35's SZ-2 low tilts) unmarked — verified live.
- [x] show scan strategy in analyst panel — the same popup; not a separate panel, but reachable
  from the one place the app already shows the VCP number.
- [x] allow "follow newest 0.5° cut" mode independent of full-volume completion — the ribbon's
  Tilt angle group has a "Follow low" toggle: while following live, it jumps the display to the
  lowest tilt the instant a sweep there lands (including a SAILS/MRLE mid-volume insert), not at
  the next full-volume boundary. `Volume::changed_includes_lowest_tilt` is the pure decision,
  unit-tested; the live-update handler in `app.rs` acts on it only while `follow_lowest_cut` is on
  and the pane is following live. Off by default. Verified live: toggled on, a manual tilt pick
  was overridden back to the lowest tilt on the next low-tilt sweep while the chunk-stream
  progress tooltip showed the scan had already moved past it to later tilts.
- [ ] make timeline order reflect actual sweep chronology

The remaining item is a different, larger kind of change — it touches how the timeline itself
represents a volume's sweep order (including SAILS/MRLE's mid-volume interleaving), not a
display-time reaction to a signal that already exists, and needs its own design pass rather than
being folded into this one.

## B6. Redundant Level II ingest, rechunking and seamless feed failover — mostly done

Wired end-to-end (B6.11 steps 1-4, 6-11 all done): a self-hostable `radar-ingest` relay, a client
provider for it, dual-feed health monitoring, a per-site failover arbiter, a NOAA TGFTP degraded
tier, and Settings/diagnostics UI all exist and are live in the app today. What's left is the
genuine external blocker (a live LDM upstream adapter, B6.11 step 5 — needs a real peer this
environment doesn't have), the deeper cross-provider dedup/seamless-handoff wiring B6.7 describes
(deferred, not required for today's safe-but-not-seamless tier switch), deployment observability's
last piece (`/metrics` now exists; structured JSON logs don't), and B6.12's remaining acceptance
gaps (see that section) — not a ground-up "not started" phase.

The target is no longer merely “try another URL if the current Level II request fails.” HookEcho
should own a **transport-independent real-time radar stream** with enough redundancy to preserve
progressive in-progress sweeps when one acquisition path fails. The design should go beyond simple
multi-endpoint client failover: keep two genuinely different acquisition paths hot, normalize both
into one canonical radial stream, and make provider changes a first-class, observable event.

The initial topology is:

```text
                       NEXRAD Level II / Archive II
                                │
                ┌───────────────┴────────────────┐
                │                                │
      Path A — existing public feed      Path B — HookEcho ingest
      Unidata/AWS live chunks            permitted LDM/IDD upstream
                │                                │
                │                         radar-ingest service
                │                                │
                │                            rechunker
                │                                │
                └───────────────┬────────────────┘
                                │
                     canonical radial blocks
                     + source/provenance stamp
                                │
                     per-site failover arbiter
                                │
                 VolumeAssembler / live sweep state
                                │
                    2D + 3D HookEcho renderers
                                │
                NOAA TGFTP completed-volume fallback
                     (degraded continuity only)
```

The Unidata/AWS live-chunk path remains the first implementation and can remain the preferred
source when it is healthiest/fastest. The independent fallback is a **HookEcho-owned backend
LDM/IDD ingester and rechunker**, not another URL backed by the same chunk service. NOAA TGFTP is
the last-resort completed-volume path; it must never be presented as feature-equivalent to a
sub-volume live stream.

### B6.1 Make the provider boundary runtime-selectable

The current `Level2LiveProvider` has one implementation and was intentionally left non-`dyn`-safe.
B6 is the concrete reason to finish the abstraction.

- [x] make the live-provider boundary runtime-selectable, using either a `dyn`-safe trait or a
  small provider enum if that produces simpler Rust; do not box merely for style
  - Implementation note: `Level2LiveProvider` (`crates/hookecho/src/volume.rs`) is now `dyn`-safe
    via `#[async_trait::async_trait]`. Native keeps the default `Send`-bound expansion (every call
    site already awaits inside a `Send`-spawned task); wasm32 uses `async_trait(?Send)` because
    `reqwest`'s wasm transport holds non-`Send` `wasm_bindgen::Closure` values internally, and the
    trait's `Send + Sync` supertrait bound was dropped entirely rather than conditionally compiled
    (a native caller that needs it adds `+ Send + Sync` at the `dyn` use site instead). Verified
    with `Box<dyn Level2LiveProvider>` in a test and a clean `cargo check --target
    wasm32-unknown-unknown`.
- [x] separate **transport capabilities** from provider identity; at minimum advertise:
  `progressive_radials`, `resume`, `completed_volume`, `historical_backfill`, and
  `server_push`
  - Implementation note: landed as `wxdata::live_block::ProviderCapabilities`, with a
    `Level2LiveProvider::capabilities()` method (`UnidataLevel2Provider` returns
    `ProviderCapabilities::unidata()`). Placed in `wxdata` rather than `hookecho` so a future
    standalone `radar-ingest` backend crate can share the same identity/capability model without
    depending on the GUI app.
- [x] keep the decode/render path source-neutral: providers deliver canonical raw blocks/events;
  existing Level II decoding, binning, stitching and 2D/3D rendering remain downstream — true by
  construction since `radar_provider_manager::SiteProviders::provider()` hands `spawn_stream` a
  plain `Arc<dyn Level2LiveProvider>`; the merge/binning/render path downstream of `DataMsg::Live`
  never learns which tier produced it.
- [x] provider priority is configured per deployment/user, but automatic selection is based on
  actual per-site health and freshness rather than a single global provider switch — the relay URL
  (`Settings.radar_relay_url`) is the per-deployment configuration; `SiteArbiter`/
  `radar_provider_manager` decide per pane/site from `provider_health::HealthBoard` freshness, never
  a single app-wide switch.
- [x] preserve a manual provider override in Advanced settings for diagnostics — new this pass:
  `Settings.radar_provider_override` (`RadarProviderOverride::{Auto,Primary,Backup,Degraded}`), a
  General-tab "Radar relay (advanced)" section, wired through
  `radar_provider_manager::SiteProviders::set_manual_override`/`clear_manual_override`.

A conceptual capability shape is sufficient; exact names can follow existing code style:

```rust
pub struct ProviderCapabilities {
    pub progressive_radials: bool,
    pub resume: bool,
    pub completed_volume: bool,
    pub historical_backfill: bool,
    pub server_push: bool,
}
```

### B6.2 Add a self-hostable `radar-ingest` backend service

Create a focused backend service rather than embedding LDM into the GUI application. It should be
usable in the project's Docker/Coolify deployment model and independently deployable on Linux.
Suggested ownership is a new service/crate such as `services/radar-ingest` or
`crates/radar-ingest`; do not grow `app.rs`.

The service must:

- [ ] accept a **permitted LDM/IDD NEXRAD2 feed** (`NEXRAD2`, historically also identified as
  FT28/CRAFT/NEXRD2) from a configured upstream peer
- [ ] treat LDM access as deployment configuration, not an entitlement: do not assume a direct
  NSF Unidata feed is available to a non-academic/self-hosted deployment
- [x] provide an adapter boundary between LDM product arrival and HookEcho's ingest core so tests
  can replay recorded Level II bytes without a live LDM process
  - Implementation note: new `crates/radar-ingest` crate, `input::InputAdapter` — a `dyn`-safe
    (`async-trait`, same pattern as `hookecho::volume::Level2LiveProvider`) consuming trait over a
    bounded `tokio::mpsc::Sender<RawProduct>`. `input::ReplayInputAdapter` implements it against an
    in-memory fixture list; the live LDM adapter (B6.11 step 5) implements the same trait.
- [x] support site allowlists so a deployment can ingest a selected radar set instead of being
  forced to retain the entire national feed
  - Implementation note: `store::IngestStore::with_allowlist`.
- [ ] timestamp every product/block at backend receipt using a monotonic processing clock plus UTC
  wall time for provenance
  - Partial: `input::RawProduct::received_at` stamps UTC wall time at ingest; a monotonic
    processing-clock component is not yet added.
- [ ] validate site ID, message framing, declared sizes and decompression boundaries before data
  enter the live ring buffer
  - Partial: `store::IngestStore::ingest` validates site ID shape and declared size (rejecting
    empty/oversized products) before admitting to the ring buffer. Message framing and
    decompression-boundary validation require Level II message parsing and land with the
    rechunker (B6.3) — raw products are still opaque bytes at this layer.
- [ ] reject malformed/oversized input without killing the stream
  - Partial: oversized/empty/invalid-site products are rejected per-product without affecting
    other sites' streams (see `RejectReason`); "malformed" in the message-framing sense needs B6.3.
- [x] use bounded queues and explicit backpressure; a slow client must never grow ingest memory
  without bound
  - Implementation note: `InputAdapter::run` sends into a bounded `mpsc::Sender`, which awaits
    (applies backpressure) when full; `store::SiteRingBuffer` is bounded on both item count and
    total bytes independently, per site.
- [ ] maintain per-site rolling state and enough recent blocks for reconnect/resume
  - Partial: `store::SiteRingBuffer`/`IngestStore` maintain bounded per-site rolling state now;
    "enough ... for reconnect/resume" depends on the sequence numbering and resume protocol landing
    in B6.3/B6.4.
- [ ] optionally assemble completed volumes in parallel for verification/backfill without delaying
  publication of live blocks
- [ ] expose health/readiness endpoints and machine-readable metrics

The backend is infrastructure, **not** a mandatory HookEcho account service. A user may point the
client at their own relay, and core public-data functionality must continue without an account.

### B6.3 Rechunk without destroying Level II fidelity

The rechunker exists to make the backend stream efficient and resumable; it must not invent new
radar values or silently resample gates.

- [x] preserve the original Level II/Archive II radar messages as the authoritative payload where
  practical; attach HookEcho transport metadata around them rather than converting them into a
  lossy synthetic radar format
  - Implementation note: `radar-ingest::rechunk::Rechunker` concatenates each message's original,
    unmodified bytes (`Message::offset()`/`size()` into the raw product) into a block's payload;
    it parses just enough header fields to know identity, never gate/moment values.
- [x] parse enough message metadata to identify site, volume, elevation/cut, azimuth/radial span,
  radar time range and sequence continuity
  - Implementation note: via `nexrad_decode::messages::decode_messages` on Message Type 31
    (Digital Radar Data) headers — `radial_status()` drives volume/cut-boundary detection,
    `elevation_number()` feeds `wxdata::live_block::CutTracker` (correctly separating SAILS/MRLE
    revisits, covered by a dedicated test), `azimuth_number()` and `date_time()` fill the block's
    azimuth span and radar time range.
- [x] emit **sub-volume blocks immediately when upstream bytes make them available**; do not wait
  for sweep or volume completion
  - Implementation note: a block flushes on whichever comes first — an elevation/volume boundary,
    `RechunkConfig::max_radials_per_block`, or (via `Rechunker::tick`) its age — never waiting for
    an elevation or volume to finish.
- [x] use configurable size/time flush limits so a partially filled block cannot be held
  indefinitely just to reach a target radial count
  - Implementation note: `RechunkConfig::{max_radials_per_block, max_block_age}`;
    `Rechunker::tick`, called periodically by the service loop independent of new data, flushes a
    stale partial block.
- [ ] do not claim that splitting an already-arrived LDM product reduces upstream latency; measure
  `radar -> backend`, `backend -> emitted block`, and `block -> client/render` separately
  - Partial: `LiveLevel2Block` already carries `radar_start`/`radar_end`/`received_at`/
    `emitted_at` separately, giving the first two latency stages for free; `block -> client/render`
    instrumentation depends on B6.4's distribution protocol and the client side, not yet built.
- [x] compute a content hash/checksum for transport integrity and deduplication
  - Implementation note: `wxdata::live_block::checksum` (SHA-256) over each finished block's
    payload, computed in `PendingBlock::finish`.
- [x] assign a monotonic per-site transport sequence number for resume/replay
  - Implementation note: `SiteState::sequence`, incremented once per emitted block (radial-chunked
    or pass-through) — never per raw product, so a product yielding no immediately-flushed block
    does not advance it.
- [x] retain original radar timestamps separately from backend receipt/emission timestamps
  - Implementation note: `LiveLevel2Block::{radar_start, radar_end}` come from decoded message
    timestamps; `received_at` from the raw product; `emitted_at` is stamped at flush time in
    `PendingBlock::finish`/`Rechunker::pass_through`.
- [x] never reorder radials merely to make prettier chunks; canonical ordering belongs in the
  downstream assembler, which already has to tolerate out-of-order arrival
  - Implementation note: `PendingBlock::push_radial` appends in arrival order only; blocks are
    never sorted or buffered-and-reordered before flush.

Suggested envelope, not a frozen wire contract:

```rust
pub struct LiveLevel2Block {
    pub site: RadarSite,
    pub volume_key: VolumeKey,
    pub elevation_number: Option<u16>,
    pub elevation_angle: Option<f32>,
    pub first_azimuth: Option<f32>,
    pub last_azimuth: Option<f32>,
    pub radar_start: DateTime<Utc>,
    pub radar_end: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub emitted_at: DateTime<Utc>,
    pub sequence: u64,
    pub source_id: String,
    pub checksum: [u8; 32],
    pub payload: Bytes, // original lossless Level II message/block bytes
}
```

### B6.4 Backend distribution protocol

The relay must support both true live push and deterministic recovery after a network interruption.

- [x] WebSocket or equivalent server-push stream for live blocks
  - Implementation note: `radar-ingest::server` (axum, `ws` feature) — `GET /sites/{site}/live`
    upgrades to a WebSocket and pushes each new block as JSON (`wire::BlockDto`) the instant
    `Pipeline` publishes it, via a per-site `tokio::sync::broadcast` channel.
- [x] HTTP endpoint to fetch a specific recent block by site/sequence/checksum
  - Implementation note: `GET /sites/{site}/blocks/{sequence}`. Fetch by checksum is not
    implemented (sequence is the primary key the resume protocol actually needs; a checksum-keyed
    lookup can be added if a concrete consumer needs it).
- [x] per-site `head`/manifest endpoint describing newest sequence, current volume/sweep and latest
  complete volume
  - Implementation note: `GET /sites/{site}/head` returns `wire::ManifestDto`
    (`rechunk::SiteManifest` converted to JSON) — newest sequence, current volume, current cut,
    latest complete volume.
- [x] reconnect with `resume_after=<sequence>` semantics so a client can fill a short gap without
  resetting an otherwise valid in-progress volume
  - Implementation note: `GET /sites/{site}/live?resume_after=<sequence>` — the handler computes
    the backlog (`BlockStore::after`) and subscribes to the live broadcast channel under the same
    lock acquisition, so no block can be lost or duplicated in the gap between the two. Covered by
    an end-to-end test driving a real `tokio-tungstenite` client against a real bound
    `TcpListener` (a protocol upgrade can't be exercised through `tower::ServiceExt::oneshot`).
- [x] bounded rolling retention for recent blocks, configurable by time and memory/disk budget
  - Implementation note: `block_store::{BlockStore, BlockRingBuffer}` — bounded on both item count
    and total bytes per site, same pattern as the B6.2 raw-product ring buffer. Time-based
    retention (evicting by block age rather than only count/bytes) is not yet implemented.
- [x] optional completed-volume endpoint generated from the exact same retained source bytes
  - Implementation note: `GET /sites/{site}/volume/latest` (`server::latest_complete_volume`,
    `BlockStore::blocks_for_volume`) returns every retained block belonging to the site's latest
    completed volume, generated from the exact same `BlockStore` retention the live path and
    `/blocks/{sequence}` read from — no separate storage or regeneration.
- [ ] compression only when it produces a measured win over already-compressed Level II payloads;
  do not burn CPU recompressing data by default without evidence
  - Not applicable yet: no compression is applied at all (JSON+base64 over plain HTTP/WS), so
    there is nothing to measure a win against. Revisit once real-world payload sizes are known.
- [ ] TLS handled by the deployment/reverse proxy, with optional relay token/auth for private
  instances; public deployments need rate limiting and connection caps
  - Not yet implemented: no auth/token check or rate limiting exists in `radar-ingest::server`
    today. Deployment-time TLS termination is unaffected (this service only ever speaks plain
    HTTP/WS and expects a reverse proxy in front of it), but auth and rate limiting are real gaps
    for a public deployment — tracked for the hardening pass (B6.11 step 12) rather than silently
    left undone.

### B6.5 Canonical radial normalization and identity

Both the current Unidata chunks and backend-rechunked LDM data must converge below acquisition into
one identity/provenance model before they reach `VolumeAssembler`.

- [ ] normalize source-specific objects into one `RadialBlock`/`RadialEvent` representation
- [ ] carry `provider_id`, acquisition path, radar timestamp, local/backend receipt time and any
  source sequence/checksum through the entire live pipeline
- [ ] define a stable logical identity for deduplication using radar site + volume identity + cut/
  elevation + radial/message identity; use payload hash as an integrity signal, not as the only
  meteorological identity
- [ ] handle SAILS/MRLE repeated low-level cuts without collapsing two legitimate passes into one
- [ ] reject or quarantine impossible cross-source conflicts rather than silently choosing whichever
  radial arrived last
- [ ] keep provenance at block/radial granularity where the source changed inside a volume

A provider switch must therefore be inspectable after the fact: the UI and diagnostics bundle
should be able to say which source supplied each span of the live volume.

### B6.6 Hot-standby failover arbiter — per radar site

Do **not** wait for the primary to fail before starting the secondary. For sites actively being
viewed/monitored, keep the independent source warm enough to know whether it is current.

Track independently for every site/provider:

- newest radar timestamp received
- newest source sequence/object seen
- wall-clock receipt time
- rolling request/stream success and failure counts
- reconnect count
- missing sequence/radial count
- duplicate/conflict count
- measured source latency
- backend processing latency where applicable
- current capability mode (`progressive`, `completed-volume-only`, etc.)

Then implement:

- [x] bounded automatic failover on repeated transport failures **or** source-data staleness
  - Implementation note: `hookecho::failover_arbiter::SiteArbiter::evaluate` — configurable
    `max_consecutive_failures` and `staleness_threshold` (`ArbiterConfig`).
- [x] freshness comparison against the backup before switching; do not fail over to an even older
  stream simply because it responds to a health request
  - Implementation note: `is_fresher` requires the candidate to beat the current side by
    `min_freshness_margin`; a property test (`switching_never_moves_the_visible_newest_radar_time_
    backwards`) checks this directly across a scripted sequence.
- [x] per-site decisions — KTLX may fail over while KOHX remains on the primary
  - Implementation note: `radar_provider_manager::SiteProviders` (B6.11 step 11) is now owned per
    `MapView` pane, one instance per site, created/replaced by `HookEchoApp::sync_radar_providers`
    whenever a pane's site changes — each pane's `SiteArbiter`/`HealthBoard` pair is independent, so
    a failure on one site's feed cannot affect another pane's provider choice.
- [x] hysteresis/cooldown before failback so a flapping primary cannot bounce the renderer between
  sources every few seconds
  - Implementation note: `failback_consecutive_healthy` requires that many *consecutive* healthy
    primary observations; any unhealthy one resets the streak to zero — tested directly
    (`an_interrupted_recovery_streak_resets_and_does_not_fail_back_early`).
- [x] failback only after the preferred source has produced multiple consecutive healthy/current
  observations
  - Same mechanism as above.
- [x] explicit switch reason (`transport_error`, `stale_data`, `sequence_gap`, `manual_override`,
  `recovery`, etc.) recorded in provenance and diagnostics
  - Implementation note: every `Transition` carries a `wxdata::live_block::ProviderSwitchReason`.
    `sequence_gap` is not yet produced — it needs the cross-provider radial identity comparison
    B6.5/B6.7 add, not just per-provider freshness/failure counts. "Recorded in diagnostics" is
    done as of B6.11 step 11: `radar_provider_manager::SiteProviders::snapshot().last_transition`
    surfaces as a "Last transition" line in the radar health popup, which
    `export_diagnostics_bundle` carries into the local diagnostics bundle verbatim
    (`DiagnosticsSourceHealth.details`).
- [x] never use provider HTTP reachability alone as “healthy”; data freshness is the decisive
  operational signal
  - Implementation note: `ArbiterInput` only carries `newest_radar_time` and
    `consecutive_failures` — there is no reachability/ping signal for the arbiter to even consult.

Thresholds should be configurable and benchmarked against real scan cadence. Do not hard-code a
marketing latency target as the health rule.

### B6.7 Seamless mid-volume continuation, with strict safety rules

The stretch goal — and the main step beyond ordinary endpoint failover — is to continue an
in-progress live sweep from the backup instead of blanking the display or waiting for the next
completed volume.

- [x] permit cross-provider continuation only when radar site, volume identity/time, VCP/cut and
  radial chronology are demonstrably compatible
  - Implementation note: `wxdata::continuation::check_volume_continuation` — compatible only on
    *exact* `VolumeKey` equality (site + volume-start to the second), never a fuzzy/partial match.
    VCP/cut compatibility is `RadialIdentity`'s job (it already carries `CutKey`, including SAILS/
    MRLE `repeat_index`); radial chronology (accepting only genuinely newer radials, never
    rewinding) is enforced by the failover arbiter's own freshness check (B6.6), not duplicated
    here.
- [x] deduplicate overlapping radials/blocks already rendered from the old source
  - Implementation note: `wxdata::continuation::RadialDedup` — `accept_new` returns only the
    radial identities a block covers that haven't already been seen, expanding a block's azimuth
    span (`first_azimuth_number..=last_azimuth_number`) into individual `RadialIdentity`s.
- [x] accept newer backup radials into the same live volume when identity is certain
  - Implementation note: when `check_volume_continuation` returns `Compatible`, a caller feeds the
    backup's blocks through the same `RadialDedup` the primary already uses — new radials pass
    through `accept_new` normally.
- [ ] never roll the visible sweep backwards because a backup's last-seen sequence is behind
  - Not this module's job — sequence numbers are provider-local (`LiveLevel2Block::sequence`'s own
    doc comment: "two different providers' sequence numbers are never compared to each other").
    This is the failover arbiter's freshness-margin check (B6.6, already implemented and tested)
    applied at switch time, plus radar-time-based ordering within `RadialDedup`'s consumer — not
    yet wired together end-to-end (B6.11 step 11).
- [ ] preserve the previous-sweep-under-new-wedge behavior already implemented in B2
  - Not yet touched — this is existing `MapView`/render-pipeline behavior this step hasn't wired
    into.
- [x] if identity is ambiguous, **do not mix**: reset live assembly at a safe sweep/volume boundary
  and make that discontinuity visible in provider state
  - Implementation note: `ContinuationDecision::Incompatible` is the explicit "must not mix"
    signal — there is no third, fuzzy outcome (`near_but_not_exactly_matching_volume_starts_are_
    incompatible` tests this directly). Actually resetting live assembly and surfacing the
    discontinuity in the UI is step 11's wiring, not this pure-logic step's job.
- [x] test provider changes during SAILS/MRLE inserts, not only ordinary single-pass VCPs
  - Implementation note: `a_sails_revisit_is_not_deduplicated_against_the_base_tilt` — a SAILS
    revisit of elevation 1 (same elevation number, same azimuth range, `repeat_index` 1 instead of
    0) is confirmed to dedup as entirely new radials, not collapse into the base tilt's.
- [ ] keep 3D `live_scan_revision`, observed-gate buffers and smooth-volume caches synchronized
  when the source changes within an otherwise continuous logical volume
  - Not yet touched — depends on the step 11 wiring into the actual live render pipeline.

The invariant is: **availability may degrade; scientific identity may not.** A visibly brief reset
is preferable to silently composing radials from incompatible scans.

### B6.8 Completed-volume emergency continuity via NOAA TGFTP

Add NOAA/NCEP TGFTP Level II as the final fallback mode for availability when neither progressive
source is usable.

- [x] implement a `NoaaTgftpLevel2Provider` for latest completed `.bz2` Level II volumes
  - Implementation note: `hookecho::tgftp_provider::NoaaTgftpLevel2Provider`, against
    `https://tgftp.nws.noaa.gov/data/radar/nexrad_level2/{SITE}/`. Verified against the real, live
    service while building this (not just replay fixtures): fetched and decoded an actual current
    volume successfully. The `.bz2` filename suffix does not mean a whole-file compression
    wrapper — confirmed from a real downloaded volume's raw header bytes (`AR2V0006....`
    immediately followed by a `BZh9`-prefixed first record) that these are plain Archive II files
    with the standard per-record bzip2 compression, the exact shape
    `wxdata::level2::decode_volume` already decodes for the AWS archive path.
- [x] poll efficiently using the site's directory/index metadata rather than redownloading listings
  unnecessarily
  - Implementation note: polls the small `dir.list` index (`<size> <filename>` per line, a few KB)
    rather than the full HTML directory listing; only downloads a volume file itself when
    `dir.list`'s newest entry differs from what's already held.
- [x] reuse the same Level II decoder and `VolumeAssembler` validation path
  - Implementation note: `wxdata::level2::decode_volume` — the identical function the AWS archive
    path already uses; no separate decoder.
- [x] label the mode **completed-volume fallback** in UI/provenance; do not show live chunk progress
  when no progressive feed exists
  - Implementation note: `label()` returns `"NOAA TGFTP (degraded)"`; `capabilities()` returns
    `ProviderCapabilities::tgftp()` (`progressive_radials: false`); `subscribe`'s `on_progress`
    callback is never invoked. Actually surfacing this in the UI is step 11's job.
- [ ] automatically return to a progressive provider only through the same hysteresis/freshness
  policy used above
  - Not this provider's job — recovering to a progressive source is the failover arbiter's
    responsibility (B6.6/step 8, already implemented); this provider only needs to exist and be
    selectable as the arbiter's last resort, which is step 11's wiring.
- [x] cache the most recent valid completed volume so a total network outage degrades to an honest
  stale display rather than a blank/crash
  - Implementation note: every successful fetch updates an in-memory `last_known` cache;
    `latest_complete_volume` falls back to serving it (as `UpToDate` or `New`, whichever is
    correct for what the caller already has) when a fetch fails, rather than propagating the
    error, once at least one fetch has ever succeeded. The very first call with nothing cached yet
    still surfaces a real error rather than fabricating data.

### B6.9 Source-health and latency UI — partly done

Extend B3/N1 rather than creating a second unrelated diagnostics system.

- [x] show active provider/path for the current radar site — new this pass, see the Unreleased
  CHANGELOG entry: `registry.rs::failover_details` adds an "Active provider" line (the exact
  `Level2LiveProvider::label()` of whichever tier is selected) to the same B3 radar health popup
  every other radar latency reading already lives in — not a second panel.
- [x] show standby provider and whether it is caught up — a "Standby provider" line shows the
  *other* side's own label plus how far behind its newest radar time is (`"HookEcho Relay (12s
  behind)"`), or "no relay configured" / "not yet reporting" when there's nothing to compare
  against yet.
- [ ] show radar-time age, provider receipt lag, backend rechunk latency, network-to-client delay,
  decode time and render-queue time as distinct measurements where available — the existing B3
  lines (provider lag, decode time, render queue, stream retries) describe whichever provider is
  *currently active*, automatically following a tier switch since they read live `MapView` fields
  regardless of source; `radar-ingest`'s own backend-side stage latencies (backend rechunk latency,
  network-to-client delay) are not surfaced client-side yet — B6.3/B6.4 flagged this as depending on
  this exact client wiring, which now exists, so it's a natural next increment rather than blocked.
- [x] show current failover state: `PRIMARY`, `BACKUP`, `DEGRADED_VOLUME`, `MANUAL` — a "Failover
  state" line. `STALE` is deliberately not a separate state here: an active-but-aging source is
  already visible via the existing "Provider lag" reading, and a genuinely unusable one shows as
  `DEGRADED_VOLUME` once `radar_provider_manager::DEGRADED_AFTER` passes — a scoping choice, not an
  oversight.
- [x] show last provider transition time and reason — a "Last transition" line: which tier, the
  `wxdata::live_block::ProviderSwitchReason` in plain words, and how long ago.
- [ ] expose sequence gaps/retries/duplicates/conflicts — still needs B6.5's cross-provider radial
  identity comparison, not built yet; `radar_provider_manager` only reasons about per-provider
  freshness/failure counts today.
- [x] add these fields to the local diagnostics bundle — `DiagnosticsSourceHealth` now carries
  `SourceHealth.details` verbatim (`app.rs::export_diagnostics_bundle`), so radar's new failover
  lines ride along automatically; no bespoke diagnostics struct needed.
- [ ] optional developer view: both providers' freshness side-by-side for the same site — the
  "Standby provider" line is one-sided (whichever side is currently *not* active); a real
  side-by-side developer view showing both regardless of which is active is still open.

Do not hide a source change. The operator should always be able to answer “which acquisition path
am I looking at, and how old is its newest radar data?” — answered today via the radar health
popup's new lines, described above.

### B6.10 Deployment and operations — mostly done

- [x] add a Docker image/service for the backend ingester/rechunker — `Dockerfile.radar-ingest`
  (multi-stage `rust:bookworm` build → `debian:bookworm-slim` runtime, no GUI/GPU deps since this
  is a plain tokio+axum service, distinct from the main app's own `Dockerfile`/`Dockerfile.coolify`).
- [x] make upstream LDM host/feed pattern, site allowlist, retention, listen address and public URL
  environment-configurable — all read in `crates/radar-ingest/src/main.rs` (doc comment there lists
  every `RADAR_INGEST_*` variable): `RADAR_INGEST_LISTEN_ADDR`, `RADAR_INGEST_ALLOWED_SITES`,
  `RADAR_INGEST_BLOCK_RETENTION_{ITEMS,BYTES}`, `RADAR_INGEST_SITE_RAW_MAX_{ITEMS,BYTES}`, and
  `RADAR_INGEST_LDM_*` (read by `radar_ingest::ldm::LdmSourceConfig`, logged as a warning if set
  since B6.11 step 5's blocker means nothing acts on it yet). "Public URL" isn't a runtime setting
  — it's the relay's own base URL, which an operator hands to clients via Settings → General →
  "Radar relay (advanced)"; documented as such in `docker-compose.radar-ingest.yml`'s header
  comment rather than read from an env var with nothing to do with it.
- [x] provide a Coolify/docker-compose example without making hosted HookEcho infrastructure
  mandatory — `docker-compose.radar-ingest.yml`, a separate compose file from the main app's own
  (this service is entirely optional; HookEcho works with zero relay configured, using only the
  existing Unidata path).
- [x] document how an operator supplies their own permitted LDM peer; do not ship credentials or
  assume access to an upstream that has not agreed to feed the deployment — `radar_ingest::ldm`'s
  module doc comment spells out exactly why no default host/credentials are assumed and what a
  real deployment must supply itself.
- [x] implement graceful restart: persist enough sequence/manifest state to reconnect clients or
  deliberately announce a new stream epoch so old sequence IDs cannot collide — took the "or":
  `Pipeline::epoch()` is a wall-clock-derived `u64` fixed once per process construction (so, in
  practice, once per process start — no new `uuid`/`rand` dependency for one call site) and now
  rides along on every `ManifestDto` (`/sites/{site}/head`, `#[serde(default)]` so an older client
  still deserializes it) and as an optional `?epoch=` query param on `/sites/{site}/live`. A
  `resume_after` whose claimed epoch doesn't match the running process's current one is ignored
  entirely — the connection is served exactly like a fresh one (no backlog) rather than risking a
  same-numbered-but-different block from before a restart. Verified with a real
  `TcpListener`/WebSocket integration test (`server::tests::
  resume_after_is_ignored_when_the_claimed_epoch_does_not_match`) that a wrong-epoch resume gets
  none of the backlog it would otherwise be entitled to. **Not done**: the actual production client
  (`HookEchoRelayLevel2Provider::subscribe`) doesn't send `resume_after`/`epoch` at all yet — every
  (re)connect already starts fresh with no backlog request, so the collision this item guards
  against isn't reachable by today's client; the protection exists for the resume mechanism itself
  (already real, tested, wire-documented API surface) so it's safe for whichever client uses it
  next, current or future. Wiring the client to actually resume across a brief disconnect (rather
  than relying on `base`'s carried-forward merged `Scan` to paper over the gap, which it already
  does adequately for now) is a natural follow-up, not required by this checkbox's own wording.
- [x] bounded memory/disk usage with per-site eviction — `IngestStore`/`IngestLimits` (raw product
  admission) and `BlockStore`/`BlockStoreLimits` (rechunked block retention) both evict oldest-first
  on an item-count *and* a byte-budget cap, per site, independently (`store::tests::
  ring_buffer_evicts_on_byte_budget_even_under_the_item_cap`,
  `block_store::tests::byte_budget_evicts_independent_of_item_count`). "Disk usage" is trivially
  bounded at zero: this crate has no persistence layer at all today — everything lives in memory
  and is lost on restart (see the epoch item above for why that's an accepted, documented tradeoff
  rather than an oversight), so there is no disk quota to enforce yet.
- [x] structured logs and `/health`, `/ready`, `/metrics`-style observability — `/health`/`/ready`
  already existed; new this pass: `/metrics` in plain Prometheus text exposition format (no metrics
  library added for three gauges — the format is just newline-separated text), reporting
  `radar_ingest_epoch` and, per site with retained activity, `radar_ingest_site_blocks{site=...}`
  and `radar_ingest_site_bytes{site=...}` (`BlockStore::retention_stats`, `Pipeline::
  retention_stats`). Logging remains plain `log`-crate output (`env_logger`, same as the main app)
  rather than a structured (JSON) format — reasonable for a service this size today; revisit if a
  real deployment's log aggregation needs it.
- [x] network contract test for the current Unidata chunks and NOAA TGFTP; LDM integration tests
  use replay fixtures unless CI has an explicitly configured LDM feed —
  `hookecho::volume::tests::latest_complete_volume_finds_a_new_one_then_reports_up_to_date`
  (Unidata) and `hookecho::tgftp_provider`'s own live-fetch test (NOAA TGFTP), both real
  network calls against the live services, both `#[ignore = "network"]` per this repo's existing
  convention for real-network tests — not run by default, run explicitly
  (`cargo test -- --ignored`). `radar-ingest`'s own tests already use replay fixtures/synthetic
  data exclusively, matching this item's LDM clause (there being no live LDM adapter to contract-
  test against yet, per B6.11 step 5).

### B6.11 Implementation order

Build B6 in increments so the existing fast path remains usable throughout:

1. [x] canonical provider capabilities + radial/block provenance
   - `wxdata::live_block` (`ProviderCapabilities`, `VolumeKey`, `CutKey`/`CutTracker`,
     `RadialIdentity`, `ProviderSwitchReason`, `LiveLevel2Block`), wired into
     `hookecho::volume::Level2LiveProvider` as a `capabilities()` method.
2. [x] `radar-ingest` replay input and in-memory per-site ring buffer
   - New `crates/radar-ingest` crate: `input::{RawProduct, InputAdapter, ReplayInputAdapter}` and
     `store::{SiteRingBuffer, IngestStore, IngestLimits, RejectReason}`. See the B6.2 implementation
     notes above for exactly which of that section's bullets this satisfies.
3. [x] lossless rechunker + manifest/sequence model
   - New `radar-ingest::rechunk` module: `Rechunker` parses raw products into
     `wxdata::live_block::LiveLevel2Block`s via `nexrad_decode::messages::decode_messages`, with
     `SiteManifest` (newest sequence, current volume/cut, latest complete volume) queryable per
     site. See the B6.3 implementation notes above for exactly which of that section's bullets
     this satisfies (the cross-layer latency-measurement bullet is only partly done — it depends
     on B6.4/the client side, which don't exist yet).
4. [x] WebSocket live stream + HTTP resume/backfill API
   - New `radar-ingest::{server, pipeline, block_store, wire}` modules (axum). `Pipeline` ties the
     rechunker to bounded per-site block retention (`block_store::BlockStore`) and per-site
     `tokio::sync::broadcast` fan-out; `server::router` exposes `/health`, `/ready`,
     `/sites/{site}/head`, `/sites/{site}/blocks/{sequence}`, and `/sites/{site}/live`
     (WebSocket, `resume_after` query param). See the B6.4 implementation notes above for exactly
     which of that section's bullets this satisfies — auth/rate-limiting and a completed-volume
     endpoint are explicitly not yet done.
5. [ ] LDM/IDD input adapter using a configured permitted upstream peer
   - **Blocked on a genuine external dependency, not skipped.** `radar-ingest::ldm::LdmSourceConfig`
     reads the external configuration a live adapter needs (host/port/feed pattern/site allowlist,
     via `RADAR_INGEST_LDM_*` env vars — no default host is assumed, per B6.2). The LDM6/7 wire
     protocol itself (`ldmd`'s peer protocol) is stateful and effectively impossible to implement
     correctly without a real upstream peer to develop and validate the handshake/framing against;
     no such peer or credentials exist in this environment. This matches the roadmap's own
     anticipated blocker ("unavailable credentials/upstream LDM access") and its own accommodation
     ("LDM integration tests use replay fixtures unless CI has an explicitly configured LDM feed",
     B6.10). Everything downstream of ingestion (B6.11 steps 1-4) is already provider-agnostic — a
     live LDM adapter, once buildable against a real peer, only needs to implement
     `crate::input::InputAdapter`, the same trait `ReplayInputAdapter` already implements and every
     later step already tests against. Work continues on the unblocked steps below.
6. [x] client `HookEchoRelayLevel2Provider`
   - New `hookecho::relay_provider` (native only for this increment — see its doc comment: a
     browser WebSocket client is a genuinely different implementation than `tokio-tungstenite`,
     deferred rather than built blind). Implements `Level2LiveProvider` against the `radar-ingest`
     server from B6.11 steps 1-4: `subscribe` connects a real WebSocket and reassembles a `Scan`
     from accumulated blocks via the new `wxdata::live_block::assemble_scan` (reusing
     `nexrad-data`'s own real-time chunk assembly rather than a parallel decoder — each block's raw
     message bytes are wrapped as an `IntermediateOrEnd` LDM record) plus `wxdata::live::merge_scan`
     (the same incremental merge the Unidata path already uses); `latest_complete_volume` polls the
     new `/sites/{site}/volume/latest` HTTP endpoint. `wxdata::relay_wire::BlockDto` (moved out of
     `radar-ingest` so both server and client share one definition without `hookecho` taking on
     `radar-ingest`'s server-only dependencies) verifies each block's checksum on receipt before
     trusting it. Proven against a real, in-process `radar-ingest` server over a real
     `TcpListener`/WebSocket in `hookecho`'s own test suite (`relay_provider::integration_tests`),
     not just unit-tested in isolation.
7. [x] run Unidata + relay simultaneously and expose comparative health without switching
   - New `hookecho::provider_health` (native only, same reasoning as `relay_provider`):
     `spawn_dual_feed_monitor` runs `monitor_provider` for each given `Level2LiveProvider`
     concurrently, each restarting its own `subscribe` on error/end (with a backoff) and updating
     a shared `HealthBoard` — newest radar time, last receipt time, success/failure/reconnect
     counts, last error, capabilities — keyed by provider label. Deliberately does **not** call
     any caller-supplied `on_update`: a health monitor cannot end up feeding the renderer by
     construction, so "without switching" is structural, not just a convention to remember. Not
     yet wired into `MapView`/the UI (that starts at step 11); tested against a deterministic
     scripted fake provider (success, failure+reconnect, and two providers tracked independently),
     not live network. What B6.6's full tracked-state list doesn't have yet: sequence-gap/
     duplicate/conflict counts (those compare two providers' *data*, which needs B6.5/B6.7's
     identity work, not just per-provider bookkeeping) and measured network/backend-processing
     latency breakdowns (needs per-stage timestamps threaded through, B6.9's job).
8. [x] per-site failover arbiter with bounded failure/staleness criteria
   - New `hookecho::failover_arbiter::SiteArbiter` — a pure decision state machine (no network, no
     platform dependency; builds on wasm32 too, unlike `relay_provider`/`provider_health`). See the
     B6.6 implementation notes above for exactly which of that section's bullets this satisfies.
     Not yet wired to actually switch which provider's data reaches `MapView` — this step builds
     and tests the decision logic itself; wiring it into the live render pipeline is step 11.
9. [x] safe mid-volume continuation + deduplication/conflict handling
   - New `wxdata::continuation` (`check_volume_continuation`, `RadialDedup`, `radial_identities`)
     — pure identity/bookkeeping logic, no network, no rendering. See the B6.7 implementation
     notes above for exactly which of that section's bullets this satisfies; wiring this into the
     live render pipeline so a real switch actually happens safely is step 11.
10. [x] NOAA TGFTP completed-volume degraded provider
    - New `hookecho::tgftp_provider::NoaaTgftpLevel2Provider` — cross-platform (native + wasm32,
      unlike `relay_provider`/`provider_health`; needs only `reqwest` and
      `wxdata::task::sleep_while`). See the B6.8 implementation notes above for exactly which of
      that section's bullets this satisfies. Verified against the real, live TGFTP service, not
      only deterministic fixtures — a genuine current volume was fetched and decoded successfully
      while building this. Not yet selected by the failover arbiter as an actual last resort
      (step 11's wiring).
11. [x] source-health UI, manual override and diagnostics export — this pass wires everything
    steps 1-10 built into the actual live pipeline:
    `hookecho::radar_provider_manager::SiteProviders` (new) owns one `SiteArbiter` +
    `provider_health::HealthBoard` pair per `MapView` pane, adds a third "degraded" tier on top of
    the arbiter's own binary primary/backup choice (falling to `NoaaTgftpLevel2Provider` once
    whichever side the arbiter prefers has itself gone stale past a fixed grace period — this
    engages even with no relay configured at all, degrading a lone stalled Unidata feed rather than
    leaving the pane stuck), and exposes `set_manual_override`/`clear_manual_override` across all
    three tiers (wider than `SiteArbiter`'s own binary override). `HookEchoApp::sync_radar_providers`
    creates/replaces/ticks one per pane every frame from `Settings.radar_relay_url` and the new
    `Settings.radar_provider_override`; `spawn_stream` now asks it which `Level2LiveProvider` to
    subscribe with instead of hard-coding `UnidataLevel2Provider`, and `manage_stream` aborts a
    running stream immediately when the selected tier changes rather than waiting for it to fail on
    its own. `registry.rs::failover_details` adds "Active provider"/"Standby provider"/"Failover
    state"/"Last transition" lines to the existing B3 radar health popup (not a second panel), and
    `DiagnosticsSourceHealth` now carries `SourceHealth.details` verbatim so those lines reach the
    N4 diagnostics bundle for free. A new "Radar relay (advanced)" section in Settings → General
    holds the relay URL and the manual-override dropdown. Explicitly **not** done here: wiring
    `wxdata::continuation`'s `check_volume_continuation`/`RadialDedup` — a tier switch today resets
    to the pane's last full volume and re-streams from there via the new provider (safe: it can
    never mix two sources' radials, matching the roadmap's own "availability may degrade;
    scientific identity may not" invariant) rather than the more ambitious *seamless* mid-sweep
    handoff B6.7 describes; that stretch goal, plus sequence-gap detection and a true side-by-side
    developer health view, remain open. See the Unreleased CHANGELOG entry.
12. [ ] chaos/replay/performance tests, then enable automatic failover by default — automatic
    failover already runs today (nothing gates it behind a feature flag: `SiteArbiter`/
    `SiteProviders` are always active for a followed NEXRAD site), but only when a relay URL is
    configured does that failover have a genuinely independent second progressive path to fail
    over *to* — the primary/backup arbiter still trivially prefers primary with no backup running.
    Chaos/replay/performance testing across the whole wired stack (as opposed to each module's own
    isolated deterministic tests) is still open.

Do not make the renderer wait for this entire list. Each increment should keep the current
Unidata path working and should land with deterministic replay tests.

### B6.12 Acceptance tests

Audited against the test suite that already exists (much of this list turned out already covered
by tests written for B6.6/B6.7/B6.11's own sections, just never cross-checked against this exact
list) rather than assumed unstarted: 12 of 16 are covered (3 with a new test written this pass —
out-of-order block assembly, the B6.10 stream-epoch resume outcome, and the slow-client backpressure
bound); 4 remain genuinely open (below).

- [x] synthetic chunks/blocks arriving out of order produce the correct final sweep — new this
  pass: `wxdata::live_block::tests::
  assemble_scan_produces_the_same_sweep_regardless_of_block_order` feeds the same VCP + two radial
  blocks forward and reversed and checks both produce the same sweep/radial count. Note what this
  actually establishes: `assemble_scan` delegates straight to `nexrad_data::aws::realtime::
  assemble_volume`, an external vendored crate this workspace doesn't own — so this pins down that
  dependency's real behavior for this codebase's record, not something HookEcho's own code
  guarantees by construction. No caller today can actually deliver blocks out of order in practice
  (`radar_ingest::server`'s live handler fully drains backlog under lock before switching to live
  delivery, and `relay_provider` only ever appends in arrival order) — this test is insurance
  against a future resume/backfill path that might interleave, not a fix for an observed bug.
- [ ] duplicate blocks from both providers are rendered once — the underlying primitive
  (`wxdata::continuation::RadialDedup`) is real and unit-tested
  (`dedup_filters_overlapping_azimuths_but_keeps_new_ones`), but per B6.11 step 11's own note it is
  **not wired into the live render pipeline** — today's failover is a clean single-active-provider
  switch, never two sources feeding the renderer at once, so there is nothing yet for this
  criterion to exercise end-to-end. Genuinely still open; needs the dedup wiring B6.11 step 11
  deferred, not just a test.
- [x] a missing radial block does not corrupt adjacent azimuths — `wxdata::live::tests::
  a_partial_sweep_does_not_punch_a_hole_in_the_one_already_merged` (a chunk boundary splits a
  sweep into two partial merges; the already-merged half must stay intact and gapless) and
  `a_new_pass_keeps_the_previous_one_in_azimuths_it_has_not_reached` (a fresh pass's not-yet-
  reached azimuths keep last pass's data rather than blanking). Covers the merge-correctness
  invariant this criterion is really asking about, via the boundary-split scenario rather than a
  literal single-dropped-radial scenario — close enough in mechanism that a separate test for the
  literal case would exercise the same code path.
- [x] primary stream failure mid-sweep switches to a caught-up independent LDM relay without
  waiting for full-volume completion when identity is compatible —
  `radar_provider_manager::tests::a_fresh_backup_takes_over_before_degrading_when_primary_fails`:
  the arbiter switches to a fresh backup purely on health (failing + stale primary, fresh backup),
  with `sp.tick()` deciding immediately — no dependency on volume/sweep boundaries anywhere in the
  decision path, which is what makes a mid-sweep switch possible in the first place.
- [x] an intentionally incompatible backup volume is **not** mixed into the current live volume —
  `wxdata::continuation::tests::different_sites_are_never_compatible` and
  `near_but_not_exactly_matching_volume_starts_are_incompatible` cover the compatibility check
  itself; combined with B6.11 step 11's note that a tier switch today always resets to the pane's
  last full volume and re-streams fresh from the new provider (never splices radials from two
  sources into one assembly), incompatible mixing is structurally not reachable, not just
  discouraged.
- [x] failover never moves the visible newest-radar timestamp backwards —
  `failover_arbiter::tests::switching_never_moves_the_visible_newest_radar_time_backwards`, named
  for exactly this criterion.
- [x] SAILS/MRLE repeated low-level cuts survive cross-provider deduplication correctly —
  `wxdata::continuation::tests::a_sails_revisit_is_not_deduplicated_against_the_base_tilt` covers
  the identity logic (a SAILS/MRLE revisit must get its own `CutKey`, not collide with the base
  tilt's). "Cross-provider" is aspirational until the dedup wiring above lands — today this is
  single-provider identity correctness, which is the prerequisite for the cross-provider case, not
  the cross-provider case itself.
- [x] a short client disconnect resumes from sequence without rebuilding the whole current volume
  — `radar_ingest::server::tests::live_websocket_replays_backlog_then_streams_new_blocks`: a
  client with `resume_after=0` is replayed exactly the one block it's missing, not the whole
  volume from scratch.
- [x] backend restart creates an unambiguous stream epoch/resume outcome — new this pass (B6.10):
  `Pipeline::epoch()` plus `server::tests::resume_after_is_ignored_when_the_claimed_epoch_does_not_match`.
- [x] slow-client/backpressure test remains within configured memory bounds — new this pass:
  `pipeline::tests::a_subscriber_that_never_drains_lags_instead_of_growing_unboundedly` publishes
  3x `SUBSCRIBER_CHANNEL_CAPACITY` blocks to a subscriber that never reads, then confirms the
  receiver reports a bounded `Lagged(n)` gap (not silently handing back all of them, which would
  mean the channel grew instead of staying fixed-capacity) and that the separately-bounded
  retention store (what an actually resuming client reads via `resume_after`, as opposed to an
  abandoned live subscription) is unaffected.
- [x] malformed/oversized Level II input is rejected without terminating healthy site streams —
  `radar_ingest::store::tests::store_rejects_oversized_product_without_affecting_other_sites` and
  `radar_ingest::rechunk::tests::garbage_bytes_are_dropped_without_panicking_or_affecting_other_sites`.
- [x] provider flapping does not cause rapid source oscillation because failback hysteresis works —
  `failover_arbiter::tests::failback_requires_consecutive_healthy_observations_not_just_one` and
  `an_interrupted_recovery_streak_resets_and_does_not_fail_back_early`.
- [x] when both progressive feeds fail, NOAA TGFTP supplies the next completed volume and the UI
  clearly changes to `DEGRADED_VOLUME` —
  `radar_provider_manager::tests::both_sides_stalling_degrades_even_with_a_backup_configured` (and
  `a_stalled_primary_with_no_backup_degrades_to_tgftp` for the no-backup case).
- [ ] completed volume reconstructed from each progressive path matches the same archived Level II
  volume within decode tolerance — no test feeds the *same* archived volume's raw bytes through
  both the Unidata decode path and the relay's rechunk-then-`assemble_scan` path and compares the
  results. Still open; would need an archived-volume fixture common to both.
- [ ] measured radar/provider/backend/client/decode latency fields are internally consistent and
  shown with exact provenance — individual latency fields exist and are read correctly by the B3
  health popup (per B6.9), but nothing tests the *consistency* relationship between them (e.g.
  provider lag ≤ total age, decode time is part of and not double-counted against render-queue
  time). Still open.
- [ ] 2D progressive sweep, gate inspector, cross sections, derived products and both 3D modes
  continue updating across a safe provider transition — a real UI-integration test across the
  whole wired stack; this is B6.11 step 12's own "chaos/replay/performance tests" item under
  another name, not separately started.

### B6.13 Definition of done

B6 is not complete merely because a second endpoint can return a radar file. It is complete when
HookEcho has **two independently acquired progressive Level II paths**, one of them the self-hosted
LDM/IDD backend ingester/rechunker, both feeding the same canonical radial pipeline; automatic
per-site failover can continue a compatible in-progress scan without duplicate/rollback artifacts;
provider provenance is visible and exportable; and NOAA TGFTP provides an explicitly degraded
completed-volume continuity mode when both live paths are unavailable.

---

# 5. Phase C — Advanced radar analysis engine

**Priority: P0/P1. This is what moves HookEcho from viewer to analyst workstation.**

## C1. User-defined radar product engine — partly done

Implement a safe expression/DSL system inspired by the flexibility of GR2Analyst user-defined products, but designed around HookEcho’s Rust/WGPU architecture.

The evaluator half is done, including the vertical/layer aggregate functions and (an earlier pass)
freezing-level/-20C environmental height inputs: `wxdata::udp` parses and evaluates a formula
against one gate or, for the column functions, one point's whole tilt column, and
`ui::udp_window`/the gate inspector's "USER-DEFINED" section let a user define one and see it live
against real data. New this pass: the freezing-level fetch is now proactive on any gate inspection
(see the inputs checklist item below), closing this section's own previously-named follow-up. What
remains is rendering a product as its own map layer — a separate, larger piece of work (see below)
— plus a -10C environmental height input (HRRR's own isotherm-height
field doesn't publish that level via the mechanism 0C/-20C already reuse).

### First version capabilities

Inputs:

- [x] REF
- [x] VEL
- [x] SW
- [x] ZDR
- [x] CC
- [x] KDP
- [x] gate altitude — formulas can use `BEAM_HEIGHT_M` above radar or `BEAM_ALTITUDE_M` above sea
  level (site elevation plus tower height); missing site metadata leaves the latter unavailable
- [x] range — ground range, matching the gate inspector's own "Ground range" label
- [x] azimuth
- [x] elevation
- [x]/[ ] freezing level / -20C environmental heights — `FREEZING_LEVEL_M` /
  `MINUS20C_HEIGHT_M`, both metres above sea level (compare against `BEAM_ALTITUDE_M`, not
  `BEAM_HEIGHT_M`). Reuses `OverlaySource::FreezingLevels` — the exact HRRR fetch
  (`HGT` at "0C isotherm" / "253 K level") already built for the MEHS/POSH hail grids — rather
  than adding a second fetch path; `inspect_gate` looks up the app's existing single most-recent-
  site cache (`self.freezing`), filtered to the gate's own site so a stale reading from a
  previously followed site can't leak in. **Follow-up closed this pass:** `inspect_gate` (both its
  callers — a Gate Inspector click and the J3 linked cursor-probe table) now proactively calls
  `fetch_freezing_levels` itself whenever the cache is empty or stale for the gate's own site,
  rather than relying only on a hail grid's own request for it — a UDP formula using these inputs
  no longer needs the user to have turned MESH/POSH on first, just to have inspected a gate at
  least once. `fetch_freezing_levels` self-throttles to 900s and no-ops without a site, so calling
  it opportunistically on every inspection costs nothing extra. `inspect_gate` gained a `ctx`
  parameter to reach it (threaded through both call sites); no new unit test, since nothing in this
  codebase constructs a full `HookEchoApp` with a real volume/spawner to exercise `inspect_gate`
  directly — verified by full compile + the existing 618-test suite passing, plus manual borrow-
  checker reasoning (the fetch call is placed before `self.views[idx]` is mutably borrowed, since
  a method call needs the whole `&mut self`, not just the disjoint `self.freezing` field the read
  right after it uses). **-10C height is not added** — HRRR's isotherm-height field only publishes
  0C and 253 K (-20C)
  via this same mechanism; a -10C level would need a separate, unverified fetch this pass didn't
  confirm exists. `wxdata::udp` gained 2 new unit tests; `ui::udp_window`'s own reference text was
  also corrected here — it had gone stale claiming vertical/layer aggregates "aren't available
  yet" after an earlier pass had already shipped them.

Functions:

- [x] min / max
- [x] mean — `mean(a,b,...)` accepts 2–8 gate values, propagating missing inputs
- [x] clamp
- [x] conditional masks — via `cond ? a : b`, not the roadmap's original `where` syntax (a where
  clause only made sense paired with the vertical aggregate functions below, which aren't built)
- [x] threshold — via comparison operators, which also work as functions in their own right
- [x] vertical max/min — new this pass, see the Unreleased CHANGELOG entry: `max_vertical(expr)`
  / `min_vertical(expr)`, plus an optional second "condition" argument standing in for the
  roadmap's own `where` clause (still not itself part of the grammar — see below)
- [x] layer max/min/mean — new this pass: `max_layer(expr, lo, hi)` / `min_layer` / `mean_layer`,
  restricted to tilts whose `BEAM_HEIGHT_M` falls in `[lo, hi]`
- [x] first/last height crossing — new this pass: `first_height_above(expr, threshold)` /
  `last_height_above(expr, threshold)`
- [x] count gates meeting condition — new this pass: `count_above(expr, threshold)`
- [x] arithmetic

All five new functions evaluate as missing (not zero, not an error) through the plain
single-gate `evaluate()` entry point — they need `evaluate_at_column()` and an actual column,
which only the gate inspector currently builds (one call per tilt in the volume, at the clicked
point). A saved product using one of them therefore only shows a value from the gate-inspector
click path, not (yet) anywhere else `wxdata::udp::evaluate` is called with just one gate.

Example conceptual expressions, and what actually runs today in this grammar:

```text
max_vertical(REF where REF >= 40)        ->  max_vertical(REF, REF >= 40)
min_vertical(CC where REF >= 35)         ->  min_vertical(CC, REF >= 35)
max_layer(ZDR, freezing_level + 2km, freezing_level + 6km)
    ->  max_layer(ZDR, FREEZING_LEVEL_M + 2000, FREEZING_LEVEL_M + 6000)   -- FREEZING_LEVEL_M is
                                                                               a real input now
max_layer(KDP, minus10c_height, minus20c_height)   -- MINUS20C_HEIGHT_M exists; MINUS10C_HEIGHT_M
                                                        does not (HRRR's own isotherm-height field
                                                        publishes 0C/-20C, not -10C, via this path)
```

The first three run exactly as shown (translated to this grammar's own `where`-free spelling and
named-input capitalization); the last needs a -10C height input this app doesn't have yet. What
ran before this pass, in the same spirit: `REF > 55 && ZDR < 1 ? REF : 0` — still works unchanged.

### Safety/implementation constraints

Do **not** execute arbitrary native code.

Preferred implementation order:

1. [x] parsed AST evaluated on CPU for correctness
2. [ ] typed expression validation — today's only validation is "does it parse"; there is no
   separate type/unit-checking pass (every value is just `f32`, so e.g. adding an angle to a
   reflectivity silently type-checks — a real gap if this grows more input kinds)
3. [ ] optional AST-to-WGSL generation for parallel products
4. [x] deterministic resource limits — a recursion-depth ceiling on the parser (`MAX_EXPR_DEPTH`),
   found necessary by this session's own test suite: the first version stack-overflowed the
   process on deeply nested parentheses instead of erroring

### Product definition format

Use TOML/JSON/YAML-like portable definitions containing:

- [x] name
- [x] units
- [ ] input moments — not stored explicitly; a product implicitly reads whatever inputs its
  expression references, discovered at evaluation time rather than declared up front
- [x] expression
- [ ] default palette — meaningless without a render path to apply one to
- [ ] min/max
- [ ] missing value — `None`/"—" always means missing; there is no way to configure a different
  sentinel
- [ ] optional environmental requirements

`ProductDef` (`wxdata::udp`) is a plain serde struct, saved as part of `Settings` — JSON on disk
(or `localStorage` on web), not a dedicated TOML/YAML file format of its own.

### Acceptance criteria

- [x] invalid formulas cannot crash the app — unit-tested directly, including the stack-overflow
  fix above
- [ ] same formula produces matching CPU/GPU results within tolerance — no GPU path exists yet
- [x] product can be saved... — [ ] ...synced and exported — saved products ride along with the
  existing whole-`Settings` export/import; there is no dedicated per-product sync or export
- [x] user-defined product can be... sampled — [ ] ...rendered in a normal pane — sampling (the
  gate inspector) is done; rendering a product as its own layer is the large remaining piece,
  blocked on the same architectural question raised below

**Why rendering is a separate future increment, not attempted here:** every existing radar layer
(2D polar sweep, 3D Observed, palettes, thresholds, the legend) is keyed by the fixed
[`Moment`] enum across dozens of call sites. A user-defined product's output doesn't fit that
without either (a) inventing a "dynamic Moment" that touches all of them, or (b) rendering it
through the *other* existing pipeline instead — the lat/lon-grid `FieldLayer`/`MrmsField` system
`wxdata::derived` already uses for locally-computed products (composite reflectivity, VIL, hail).
(b) is the safer path (it has a working precedent to follow) but is still a real, multi-file
change to a rendering pipeline this pass didn't need to touch, and deserves its own scoped attempt
rather than being rushed into this one.

---

## C2. Maximum-value trails / temporal extrema — partly done

Add analyst trails for values over a moving time window.

The computational core is in `wxdata::extrema`: a running per-gate extremum held in a
`BinnedSweep`, so the accumulation shares the polar grid of the sweeps feeding it and the
existing radar layer can draw the result with no new render pipeline. Merging is element-wise
`max`/`min` over the `2..=255` code band, which is sound precisely because that band is a
monotonic linear map onto the moment's physical range — comparing codes is comparing dBZ.
`accumulate` refuses rather than blends when two sweeps do not describe the same beam
(moment, value range, geometry, elevation cut or site), and reports which, so the UI can say
"trail restarted: site changed" instead of quietly showing a shorter history than was asked
for. Sentinels never win at either end: a range-folded gate is an unknown velocity, not a low
one, and a below-threshold gate is not a low CC.

Reachable as the "Max/min trail" layer (`OverlayToggle::Trail`, Severe group; options under
Layer options). It replaces the active pane's sweep for the chosen product and tilt, so the
existing radar draw, value threshold and smoothing all apply. `HookEchoApp::advance_trail`
folds the pane's timeline frames that are still in the decode cache and end at the playhead,
oldest first, at most three per UI frame so a two-hour window grows over a few frames instead
of stalling one; a window that slides cannot un-fold its oldest frame and restarts. Progress and
any restart reason are shown in Layer options.

Known limits, not hidden: it is built only from volumes already cached (no extra fetch), so a
fresh boot shows a short trail; velocity uses the raw (folded) sweep, never the dealiased one,
because the two are not mergeable; and the moments here are the Level 2 ones. Rotation and
hail paths from the MRMS AzShear/MESH grids are a separate route (Phase D) and the ZDR-column
maximum is not a sweep moment, so those use cases are not covered by this layer. Verified by
unit tests and the lint gate; **not yet exercised on screen in a running app.**

Use cases:

- azimuthal shear maximum
- ZDR column maximum
- reflectivity core path
- MESH / hail-core path
- CC minimum path
- user-defined products

### Controls

- [x] window: 15/30/60/120 minutes — no custom entry yet
- [ ] decay visualization
- [x] threshold — the pane's existing value threshold applies to the trail
- [x] min or max mode — `extrema::Extremum`
- [x] reset at selected archive time — Layer options now exposes **Reset at playhead**, which
  discards the running accumulator and deterministically rebuilds its cached window ending at the
  selected live/archive frame. The uploaded radar image is invalidated too, avoiding a generation-
  zero cache-key collision with the trail that was just discarded.
- [ ] export raster/vector trail

### Acceptance criteria

Historic supercell replay produces a stable rotation/hail trail that can be independently recomputed from cached frames.

Order-independence — the half of "independently recomputed" that the accumulator itself owns —
is covered by `the_trail_is_independent_of_frame_order_within_one_beam`. The replay half stays
open until a historic supercell has actually been replayed and the trail inspected.

---

## C3. Beam geometry / blockage / coverage analysis — done

HookEcho already models beam height. Extend it into a full analysis layer.

- [x] beam center and approximate beamwidth top/bottom — `wxdata::beam_geometry::beam_extent`
  (elevation ± half the WSR-88D half-power beamwidth) plus `horizontal_beam_width_km` for the
  cross-beam extent; both hoisted from `suitability.rs`'s private copy so the suitability ranking,
  the gate inspector, and the cross-section overlay below all read one shared constant rather than
  three independently-typed 0.925s. Surfaced live in the gate inspector as "Beam top/bottom" and
  "Beam width", next to the existing beam-centre height. Not a claim of the true antenna pattern —
  see the doc comment on `WSR88D_BEAMWIDTH_DEG` for what the half-power convention does and does
  not model, and that this is a WSR-88D-shaped number even for a non-WSR-88D candidate.
- [x] height readout at cursor — this was already the gate inspector's "Beam height" row from B4;
  what was missing (top/bottom, width) is filled in above rather than duplicated as a second
  readout.
- [x] lowest usable beam map — new this pass, see the Unreleased CHANGELOG entry:
  `elevation::lowest_usable_tilt_image` reuses the same terrain-occultation scan
  `blockage_image` already built, but colors each pixel by the *lowest* tilt in the volume's own
  elevation list that clears the terrain there (green → red as the required tilt rises) rather
  than shading how blocked one fixed displayed tilt is. New "Lowest usable tilt (terrain)" overlay
  toggle, wired the same way as the pre-existing Blockage one. Verified against real DEM terrain
  at KMAX (Mount Ashland, OR): a tilt that clears everywhere paints its own rank color with no
  "nothing clears" pixels; a tilt that's mostly blocked shows "nothing clears" exactly where the
  existing blockage test already proved real shadow exists.
- [x] terrain blockage estimate using existing elevation infrastructure — **predates this pass**,
  found already fully built and wired while surveying this section: `crate::elevation`'s DEM tile
  cache and `blockage_image`/`BeamSite` compute an occultation-angle raster from the site's own
  antenna height, exposed as the "Blockage" overlay toggle (chase mode). The roadmap simply hadn't
  been updated to say so.
- [x] radar coverage comparison between neighboring sites — new this pass, see the Unreleased
  CHANGELOG entry: a "Compare" button on each non-current row of the suitability popup paints a
  map overlay of the same beam-height math the ranking already runs, this time point by point
  across a rect instead of once at the clicked location — blue where the current site's beam is
  lower, red/orange where the compared site's is, transparent within a ~150 m deadband (real
  numbers that close are noise against the beam-height model's own approximations) and fading out
  past a ~4 km ceiling where neither radar is telling an analyst anything about low-level
  structure anymore. Pure geometry, same scope as `wxdata::suitability` itself — no terrain, no
  network, no live data, so unlike the terrain-based Blockage/Lowest-tilt overlays it rebuilds
  synchronously on the UI thread rather than through a background DEM fetch. Deliberately does not
  fold in per-site terrain blockage (a second, independent overlay already covers that) or attempt
  a general N-site "who covers this whole area best" map — a two-site comparison from the tool
  that already ranks candidates, not the larger footprint-fusion visualization this bullet
  originally described. Verified with a full geometry test suite (antisymmetry swapping the pair,
  a site favoring itself at its own antenna, the deadband/ceiling color behavior, and the raster
  only painting where at least one site is within range) rather than a screenshot — this app's
  desktop UI has no browser-based visual verification path in this environment (see prior
  sessions' notes on the Layers panel), the same reason `elevation`'s own overlays are verified
  against real DEM data via `--ignored` tests instead.
- [x] optional beam-rise overlay in cross-section — `wxdata::xsection::build` now returns one
  `BeamRiseLine` per distinct tilt in the volume (deduplicated so a SAILS/MRLE repeat doesn't draw
  itself twice), sampled at the same radar-relative ground range each panel column already uses
  for its reflectivity gate — so the two are geometrically consistent by construction, not two
  separate calculations that happen to agree. Drawn as a color-coded overlay with a per-tilt
  legend, toggleable ("Beam rise" checkbox) independently of rebuilding the panel, since the
  geometry is already sitting in the built `CrossSection` regardless of whether it's drawn.
  Verified live: KTLX cross-section through real echo west of the site showed eight tilts'
  beam-centre curves (0.4°…6.4°) climbing correctly left-to-right over the sampled reflectivity,
  and unchecking the box removed only the lines. **3D beam-rise remains open** — the raymarch and
  observed-gate 3D paths have no equivalent overlay yet.
- [x] warn when a sampled feature is below/above sampled beam coverage — new this pass, see the
  Unreleased CHANGELOG entry: `CrossSection.beam_covered`, a grid parallel to `dbz` marking cells
  that only have a value because `sample_profile` held the nearest real beam sample over past the
  true boundary (up to 1.5 km) rather than a real interpolated sample; a hover tooltip on the
  cross-section panel now says so explicitly instead of reading indistinguishably from real data.
  Verified live: hovering the held-over band showed the warning alongside a real dBZ value, and
  hovering a genuine gap (`dbz` truly `None`) still just said "No beam coverage here".

Do not imply perfect propagation; clearly label 4/3-earth assumptions.

---

## C4. Multi-moment correlation tools — partly done

Add linked probes and scatterplots:

- [x] REF vs ZDR, REF vs CC, ZDR vs KDP, VEL vs CC — one-click pairs in the region-statistics
  window's scatter plot, plus any two moments from the X/Y pickers, with Pearson r
- [x] box area statistics — the "Region statistics" map tool: two clicks set a box, and every gate
  of the displayed tilt inside it is read in every moment at the same place
  (`wxdata::regionstats::gather`, reflectivity's gates as the rows since it reaches furthest,
  velocity dealiased). Summary table (n, min, 10%, median, 90%, max, mean). Polygon selection is
  not done; the box covers the common case.
- [x] histogram for selected region — per moment, 40 bins across its own range
- [x] vertical profile at point — the gate inspector's new "Vertical profile" section: every tilt
  at the clicked point, top of the column first, with REF/VEL/SW/ZDR/CC/KDP against beam height,
  the displayed moment drawn up the column with the melting level across it, and CSV. The samples
  were already taken for user-defined products (`column_inputs`); they were just never shown.
- [x] time series at fixed lat/lon — the gate inspector's "Time series" section: the displayed
  moment at the clicked point in every volume the pane holds (the loop being played — up to 12 on
  desktop), oldest first, at each volume's tilt nearest the displayed one; a sparkline, its span
  and range, and CSV. Bounded by what is loaded rather than fetching history on a click; a longer
  series would need a background fetch, which is the obvious extension.

- [x] Export CSV — every gate in the box, position then each moment (blank where a moment has no
  data there), from the window or `--headless-region SITE DATE HH:MM LON1 LAT1 LON2 LAT2 [out.csv]`.

Checked on real data: a box over Moore's debris ball (KTLX, 20 May 2013, 20:08Z) reads CC down to
0.54 at its 10th percentile with REF against CC at r = -0.27 (the stronger the echo, the lower the
CC: debris), where a rain box to the south reads 0.88 and r = +0.21.

---

## C5. Algorithm laboratory

Turn current TDS/couplet/cell scoring into an inspectable analyst environment.

Started: the TDS detector is the reference implementation. `TdsHit::explain` returns the algorithm
version, each weighted term with its measurement, the range, vertical and ZDR stages, and the
rotation gain, and every marker shows it on hover. `CoupletHit::explain` does the same for rotation,
with its own sense and debris-gain stages. Both detectors score low-level rooting (`base_km`,
`rooted`) as well as depth, and couplets carry a cyclonic/anticyclonic sense; scoring versions
`tds-4` and `rot-5`. Corroboration is symmetric and feedback-loop-safe
(`tds::cross_corroborate`, used by every live and headless call site) — a debris signature and a
nearby couplet each raise the other's confidence, both always read from the other side's
pre-corroboration evidence. The SPC-report backtest exists (`--headless-backtest`,
`--headless-backtest-file`, `wxdata::detverify`), backtests by range too
(`wxdata::detverify::score_in_range`, the "by range" line under each detector's table — the tool
that answered the range-normalised-shear item), and now names the specific reports a detector
missed (`wxdata::detverify::unmatched`, the "by event, reports missed" line), scoped to each
event's own radar rather than the whole country's tornado reports for that UTC window (a real bug
this pass found and fixed — POD/CSI had been understated for every backtest run so far). Cell
scoring (`wxdata::cellscore`) now has the same `severity`/`severity_explain` split and `Reason`-
based breakdown as the two detectors, reusing `crate::tds::Reason` rather than a third copy of the
same shape, shown on hover in the cells table's detail panel. The backtest scores against NWS DAT
damage surveys as well as LSR reports now (one truth per surveyed *track*, not per damage point —
a track is thousands of points and only means one tornado), printed as a second block per detector
reusing the same scoring/range/missed-report machinery, and against observed tornado warnings as a
third, independent line of evidence (`wxdata::archive_warnings` fetched per volume, reusing
`wxdata::confirm`'s existing OBSERVED-only semantics — an ordinary warning is not ground truth, so
this counts only detections that already cleared that bar, not a POD/FAR table). All three of the
original "backtest against" list are now wired up. Fixed along the way: `archive_warnings::parse`
had hard-coded every archived warning's detection/damage-threat tags to `None`, so a scrubbed
Tornado Emergency read as tier 0 everywhere — the alert panel, badge colors, and now backtest
confirmation — since the archive's `tornadotag`/`damagetag` fields are usually empty even for a
real one; it reads the archive's separate `is_emergency`/`is_pds` structured booleans now, the same
way `escalation` reads a live product's own headline text. A stored score timeline exists now too:
`wxdata::scoretrack` follows each debris/rotation detection across a backtest's volumes the same
greedy nearest-neighbour way `celltrack` follows storm cells (deliberately not built *on*
`celltrack::Track`, which carries motion a score timeline has no use for), and the backtest prints
the result — how many raw candidates turned out to be a recurring feature across more than one
volume, and the longest track's full confidence sequence. Proven against real archived data: on
Moore, OK a debris signature tracked all 8 volumes at 53% → 54% → 63% → 65% → 56% → 56% → 68% → 47%,
and a couplet rose sharply mid-event, 34% → ... → 75% → 72%, matching the real tornado's rotation
intensifying partway through the window. Reusing `celltrack::associate`'s exact shape surfaced a
real, pre-existing crash in it: `taken` (which gates that a track hasn't already claimed a point
this round) was sized to the *existing* track count and never grew when a brand-new track started
mid-batch, so a session's very first tracked volume with 3+ new storms indexed past it and
panicked — not a rare shape of input. Fixed in both modules: a point is now only ever matched
against tracks that existed before the current call, which is also the semantically correct rule
(two detections seen for the first time together in one volume are two distinct features, not one
recurring track). Live map wiring followed: hovering a debris signature or couplet now shows a
sparkline of its confidence over however many volumes it has persisted, alongside the existing
term-by-term breakdown (`compute_tds_score_track`/`compute_rot_score_track`, `nearest_score_track`
pairing a marker with its own history by proximity, `score_tooltip` drawing both). Built the
history caches by name across many volumes (`tds_shown_cache`/`rot_shown_cache`, mirroring
`celltrack_cache`'s own role for cell tracks) rather than a whole-session accumulator — a bounded
trailing-window replay, same reasoning `compute_local_tracks` already documents for why that scales
and an unbounded accumulator does not. Caught in review before it shipped: the first version cached
*raw*, pre-corroboration hits for the replay, which would have shown a sparkline ending in a
different number than the marker label beside it; fixed to cache the corroborated hits instead,
filled right after `cross_corroborate` runs, so the sparkline's last point is always the number on
screen. Cell scoring got the same "timeline of score changes": each cell's trend history now records
its composite severity alongside VIL/top/reflectivity, and the cell console draws it as its own
sparkline — computed from the same cached couplets the storm-cells table reads, so the trend's last
point is the table's number.

**Backtest corrections and `tds-5`.** Four of the eight tornado events in the backtest file had
start times wrong by one to six hours (their windows held no tornado), and reports from archive
gaps with no radar data were being scored as misses; both fixed, so every earlier number in this
section was computed on a broken set. On the corrected set, a real detector bug surfaced: debris
columns chained through fields of weak low-CC fragments by single linkage and were reported at
the chain's centroid — 12 km from the tornado on Mayfield, never paired with its 72 kt couplet. A
column is now described by its strongest core (`tds::strongest_core`). At an 80% floor, debris
went from 43% POD / 33% FAR to 49% / 31%, and rotation (through corroboration) from 14% / 64% to
19% / 58%; no threshold got worse. `tds-6` then discounts debris with no couplet beside it when
velocity was scanned (the hail case, see K2): FAR at 60% from 69% to 51%, same POD.

Every item this section originally opened with is now done. What is left in the algorithm-lab
neighbourhood is smaller, standalone follow-ups rather than open roadmap items: a first backtest
consumer for cell scoring (`wxdata::cellscore` has no `--headless-backtest` table of its own yet,
unlike the two detectors), and picking a specific value for the score-timeline association radius
and window empirically rather than the geometric argument it currently ships with.

**Range-normalised shear — done, in a narrower form than originally proposed.** Scored the raw (pre-
confidence-filter) candidates from the 8 events in `docs/backtest-events.txt`, split at 60 km — the
range every confidence score already starts discounting distance from:

| detector  | range | raw candidates | FAR |
|-----------|-------|-----------------|-----|
| rotation  | < 60 km | 72   | 89% |
| rotation  | >= 60 km | 1001 | 98% |
| debris    | < 60 km | 157  | 92% |
| debris    | >= 60 km | 296  | 95% |

Debris is close to range-neutral (3 points of FAR) — expected, since CC/Z isn't a spatial-gate-
difference measurement. Rotation was not: 93% of every raw couplet candidate came from beyond 60 km,
measurably noisier there (98% vs 89% FAR) while catching no more real circulations — the "swarm of
near-identical couplets... an artifact, not a set of tornadoes" the module doc already warned about,
now at scale.

Shipped the narrower half of the original idea rather than the riskier one: `range_floor_scale`
raises the 25 m/s gate-to-gate floor up to 2.5x by 150 km (a geometric argument — the physical arc
between adjacent gates grows in direct proportion to range with a fixed bin count — not a curve fit
to these 8 events), leaving the near-range floor exactly as it was rather than rescoring shear in
s⁻¹ everywhere, which the FAR/candidate-volume gap never actually implicated near the radar. Re-run
after (with the local-reports fix above already applied, so the denominator is the true local
count): far-range raw candidates 1001 → 249 (-75%), far-range FAR 98% → 96%, and the tornado reports
rotation matched across all 8 events fell by exactly one, 8 → 7 out of 32 local reports. Confirmed
*not* KSGF/Joplin, the flagship violent-tornado case — once scoped to local reports it has none in
range for this window at all, so it was never among the 32. The one actually lost, found with
`wxdata::detverify::unmatched`'s new per-event breakdown: a report at 42.69,-90.83 (near the
Iowa/Illinois line) at 17:32Z during the KDVN derecho event, no longer matched once its gate-to-gate
shear fell under the raised far-range floor. Scoring version `rot-5`.

Open question this left: whether a gentler or steeper ramp trades POD and FAR better than 2.5x —
untested, since a proper sweep needs several backtest runs per candidate value, and would want to
weigh a 75% noise cut against exactly this kind of loss report by report, not just in aggregate.
Doesn't block shipping what's here; worth a follow-up pass with the tooling that now exists to run
it (`score_in_range`, `unmatched`, both backtested and tested in `wxdata::detverify`).


For every automatic detection:

- algorithm version
- thresholds used
- contributing gates/cells
- confidence/score
- reason codes
- timeline of score changes

Allow historical backtest against:

- SPC LSR tornado/hail/wind reports
- NWS DAT damage surveys
- warning polygons

Do not present heuristic detections as official warnings.

---

# 6. Phase D — MRMS full catalog

**Priority: P0.**

The current `mrms.rs` contains a valuable but hand-selected subset. Replace the hard-coded-growth model with a metadata-driven MRMS catalog.

## D1. MRMS product catalog — partly done, found already built

`wxdata::mrms::catalog` exists (see A1's corrected notes above), now with 35 products as
`FieldDescriptor`s: national composite reflectivity, low- and mid-level rotation tracks
(30/60/120/240/360/1440 min), MESH, MESH swaths (30/60/120/240/360/1440 min), azimuthal
shear, lightning density (1/5/15/30 min),
precip rate, QPE 1h/3h/6h/12h/24h, precip type, FLASH QPE ARI 30m/1h/3h/6h/12h/24h/max,
POSH (`FieldLayer::Posh`),
Severe Hail Index (`FieldLayer::Shi`), national VIL (`FieldLayer::MrmsVil`, distinct from the
locally-derived `VilLocal`), and — new this pass — reflectivity at lowest altitude
(`FieldLayer::ReflLowestAlt`) and low-level composite reflectivity
(`FieldLayer::LowLevelReflectivity`), closing the "low-level (single-tilt) reflectivity" gap
named below. National echo tops at 18, 30, 50 and 60 dBZ are also cataloged, each with its own
stable layer slug and one shared km MSL legend distinct from local and Level III kft echo tops.
Five isothermal reflectivity products at the environmental 0/-5/-10/-15/-20°C levels use the
configured dBZ palette. Verified live against the real bucket (see D1's "Rules" item below):
53/53 paths confirmed.

Each new product needed more than catalog metadata alone to actually reach a user: a matching
`FieldLayer` variant (`render/mod.rs`: enum entry, `DRAW_ORDER` slot, a slug that exactly
matches its catalog `FieldId`), a `field_refresh_secs` cadence arm (`app.rs`), and — for POSH/
SHI/VIL — a color ramp (`render/field_ramps.rs`); the low-level and isothermal reflectivity
products reuse the existing `PaletteId::Reflectivity` path instead (same as `Mrms`: resolves to
the user's own `.pal`, not a fixed ramp, so no new ramp was needed). Beyond that, `app.rs`'s fetch loop
(`mrms_product`/`mrms_request`, iterating `FieldLayer::DRAW_ORDER` and looking up
`wxdata::mrms::catalog::find(layer.slug())`) and the Layers-panel picker
(`registry.rs`, iterating `catalog::PRODUCTS` and resolving each by `FieldLayer::from_slug`)
are both already fully generic over the catalog, exactly as D2's own status claims — confirmed
again by writing new products through them rather than just reading that
claim. POSH reuses the existing locally-derived `HailPosh` layer's own probability scale (one
scale app-wide, whichever source computed it); national VIL likewise reuses the existing local
`VilLocal`'s scale. SHI got a new ramp (0-400, the range MRMS's own MESH-equivalent 2-inch-hail
threshold falls in).

Target operational groups:

### Radar / reflectivity

- composite reflectivity variants
- low-level reflectivity
- echo tops
- vertically integrated products
- layer-height products

### Severe

- MESH and MESH swaths
- POSH / hail probabilities where available
- azimuthal shear low/mid layers
- rotation tracks for all published windows
- severe probability products where published

### Precipitation

- precip rate
- radar-only QPE
- multi-sensor QPE
- 1/3/6/12/24-hour accumulations as available
- gauge-corrected variants
- precipitation flag/type

### Hydrology / flash flood

- FLASH ARI products
- QPE-to-ARI exceedance fields
- streamflow-related products where publicly available and operational

### Lightning

- all public MRMS lightning density windows used operationally

### Winter

- MRMS snow/precipitation-type products when published in the operational bucket

The 35 products above cover composite reflectivity, both low-level forms (single-tilt and
low-level-composite), five isothermal reflectivity levels, low-level azshear, both rotation-track
bands, MESH + swaths, POSH, SHI, precip rate/QPE (1/3/6/12/24h)/type, national VIL, all four
national echo-top thresholds, lightning and every FLASH QPE ARI window. **Not yet cataloged**, all genuine
gaps rather than oversights — confirmed live on the bucket
while adding products across this and the prior pass, so these are real, verified prefixes to pick up next,
not guesses: hail-growth-zone height products (`H50_Above_-20C_00.50` and siblings
— related to but distinct from a literal "-20°C height," which MRMS does not publish directly;
`Model_0degC_Height_00.50` is the closest real 0°C-level product), streamflow products
(`FLASH_CREST_MAXSTREAMFLOW_00.00` and several sibling FLASH/CREST/HP/SAC variants), and MRMS's
winter/precip-type-family products beyond the one flag already cataloged
(none found with an obviously distinct winter-specific prefix in this pass's bucket listing —
may not exist as a separate published product, not confirmed either way).

### Rules

- [x] Do not blindly list a product unless a feed contract test confirms it exists —
  `mrms::catalog::the_mrms_catalog_paths_are_real` (network-gated) asks the live bucket for every
  path every product's `FetchMapping` can produce (default plus every published window), the same
  listing a real fetch depends on. Passing today: 53/53 paths (31 single-path products plus
  low-/mid-level rotation, lightning and hail-swath windows — 31 + 6 + 6 + 4 + 6 = 53) confirmed
  live, including all FLASH QPE ARI windows, both rotation bands, all four echo-top thresholds and
  five isothermal levels.

## D2. Generic MRMS fetch/decode path — done

One code path already supports any catalog scalar grid, and has since before this pass looked:

- [x] path template — a plain product path string; `Product::path()` resolves the window variants
- [x] latest-file discovery — `latest_key`, incremental (`start-after`) after the first listing
- [x] gzip handling — `gunzip`
- [x] GRIB decode — `decode_grib2` / `crate::mrms::decode_grib2` (shared with NOHRSC snowfall,
  which decodes identically)
- [x] product-specific scale/missing rules — `FieldDescriptor::normalize_missing`, per-product
  sentinel list
- [x] max texture dimension handling — `Stamped::for_display`'s `max_dim` reduction
- [x] correct interpolation method by field type — categorical/mask fields resample nearest-cell
  through the reduction pipeline and sample nearest-neighbor at read time
  (`FieldDescriptor::sample`); everything else bilinear. Verified by
  `categorical_sampling_never_creates_an_intermediate_class`.

Categorical products must never use bilinear interpolation — enforced by the type dispatch above,
not a convention callers have to remember.

## D3. MRMS browser UI — partly done

- [x] search — the same fuzzy search/command-palette every action in the app uses, reading
  `FieldDescriptor::search_text()` (name, source, family, units, description, aliases)
- [x] category — "National" in the Layers panel's category grid
- [x] favorites — new this pass, see the Unreleased CHANGELOG entry: `Settings.favorite_layers`, a
  star on every layer row (search results, category-browse, RECENT), independent of the row's own
  click target — starring never toggles the layer, toggling never stars it. Starred layers surface
  in a "FAVORITES" section above "RECENT" on the landing screen, no cap, no move-to-front on
  re-click (a curated list, not a recency trail). Verified live: starring "Rotation tracks" moved it
  into "FAVORITES" with the star lit in accent color and "Active" count unchanged; unstarring
  returned it to "RECENT" with the star back to weak-text color.
- [x] recent — new this pass: `Settings.recent_layers`, a "RECENT" section above the category
  grid on the Layers panel's landing screen, most-recent-first, capped at 6. Only a genuine user
  toggle records one — workspace restore and internal bookkeeping (HRRR sub-mode, model compare
  A/B) write layer state directly and never touch it, so loading a saved workspace never
  masquerades as something just picked. Verified live: toggling a product on, then back to the
  Browse landing screen, shows it under "RECENT"; the entry survived a full page reload, proving
  the round trip through actual settings persistence, not just in-memory state.
- [x] accumulation selector — a compact 1h/3h/6h/12h/24h picker is available in the
  workstation's Satellite tab and in the shared layer options when QPE is on. Choosing a window
  replaces the other QPE windows in the active pane. It uses the five catalog entries and their
  existing `qpe*` slugs, so older settings, workspaces and headless commands keep working; the
  individual layer rows still allow a deliberate multi-window overlay. A single `FieldLayer::Qpe`
  with a runtime fetch mapping remains an optional internal refactor, not required for the picker.
  All five source paths (`CONUS/MultiSensor_QPE_{01,03,06,12,24}H_Pass2_00.00`) were verified
  against the real MRMS S3 bucket in the earlier catalog pass.
- [x] echo-top threshold selector — the Satellite tab and shared layer options offer 18/30/50/60
  dBZ MRMS echo tops. Selecting one replaces other MRMS echo-top thresholds only in the active
  pane; existing layer rows still permit intentional multi-threshold overlays. The shared chooser
  logic also serves QPE, while each threshold keeps its own saved slug and provenance.
- [x] isothermal reflectivity selector — the same compact choice pattern offers the 0/-5/-10/
  -15/-20°C environmental levels. It swaps only the active pane's isothermal reflectivity layer;
  individual catalog rows still permit a deliberate multi-level overlay.
- [x] rotation-track window selector — one compact dropdown offers all six operational windows
  for both the 0–2 km and 3–6 km AGL bands. Each band has its own saved layer ID; changing the
  shared window refetches both when loaded.
- [x] FLASH QPE ARI selector — one compact choice offers 30m/1h/3h/6h/12h/24h and maximum
  rainfall-recurrence windows. The 30-minute product keeps its original `flashflood` saved slug;
  selecting a different window replaces only FLASH ARI layers in the active pane. The hint and
  legend state years of rainfall recurrence rather than flood probability.
- [x] valid time — `ui::data_inspector`'s "Valid" row, with signed offset from the pane's analysis
  time
- [x] native resolution — `GridProvenance.native`, shown as part of the same inspector's grid
  detail
- [x] source age — `DataStamp::age_at`/`receipt_age_at`, the inspector's "Age"/"Received" rows
- [x] point sample — genuinely true now, having been corrected to a `[x]/[ ]` split for a while
  when an audit found `FieldDescriptor::sample` existed, was correct and unit-tested, and was
  called from nowhere in the app. J3's gridded-layer probe closed that: the linked probe samples
  whichever layer is on top at the shared point and reports it with its own product name, valid
  time and legend units. The API existing and a user being able to reach it were separate claims,
  and both hold today.
- [ ] animation — not verified either way; MRMS layers appear to always fetch "latest" rather
  than loop an archived sequence the way the radar timeline does (consistent with A3's own note
  that MRMS/model/satellite caching, which archived playback would need, isn't built). Marked
  open rather than assumed.

## D4. MRMS 3D/vertical products

Where source data has vertical layers or layer heights:

- vertical profile inspector
- layer stack
- optional 3D surface/volume representation

Do not fabricate 3D from a 2D surface product.

### Acceptance criteria

- [x] new scalar MRMS product can be added through catalog metadata with minimal/no new UI code —
  confirmed with POSH/SHI/national VIL, both low-level reflectivity fields, the echo-top and
  isothermal reflectivity sets, and mid-level rotation tracks. Each needed a catalog entry and a
  matching `FieldLayer` slug (plus a ramp when no existing `PaletteId` fit); the fetch, Layers
  picker, search, provenance and health tracking picked them up. Window/threshold controls are
  shared UI over those catalog entries, not generated from metadata.
- [ ] at least the major WeatherFront-class MRMS groups are covered — 35 products across
  reflectivity (now including both low-level forms)/severe (POSH/SHI)/precipitation/lightning/
  hydrology (QPE now spans 1h/3h/6h/12h/24h)/VIL/all four echo-top thresholds and five
  reflectivity-at-isotherm levels, plus both rotation bands; several groups from D1's target list
  (layer heights and streamflow)
  are confirmed real on the live bucket but not yet cataloged — see D1's updated gap list for the
  exact prefixes
- [x] categorical fields use nearest-neighbor
- [x] all products show exact valid time and units — `DataStamp` + `Unit::symbol`

---

# 7. Phase E — Native GOES ABI workstation

**Priority: P0.**

Current GIBS imagery is useful but insufficient for an analyst-grade satellite workstation. Add native NOAA GOES-R ABI products.

## E1. Native ingest — partly done

`wxdata::goes_abi` reads ABI L2 CMIP directly from the public `noaa-goes18`/`noaa-goes19` S3
buckets (not GIBS' pre-rendered tiles), for both GOES-East and GOES-West
(`wxdata::goes_abi::Satellite`), CONUS sector only (`ABI-L2-CMIPC`, ~5 minute cadence — see that
module's own doc comment for why CONUS was chosen over mesoscale/full disk first: it's the one
sector both satellites publish continuously and that the app's other CONUS-scoped overlays already
assume). **Not done:** mesoscale sectors 1/2 and full disk are not fetched at all — mesoscale in
particular is real, separately-scoped work (E5's "discover active mesoscale sector footprints,"
not just a product-string swap, since a mesoscale sector moves and CONUS doesn't).

## E2. ABI channel support — partly done

Implement analyst channels at native/reasonable resolution:

- [x] C02 red visible (`FieldLayer::GoesVisible`, was already done before this pass)
- [x] C07 shortwave IR — new this pass. Fire/hotspot detection channel: reads as an ordinary IR
  grayscale from 180-320 K (`render::field_ramps::GOES_SHORTWAVE_IR`), then breaks into a distinct
  hot-color ramp (yellow → orange → red → magenta) from 320-400 K, the range only a sub-pixel fire
  actually reaches.
- [x] C08 upper-level water vapor (`FieldLayer::GoesWaterVapor`, was already done)
- [x] C09 mid-level water vapor — new this pass (`FieldLayer::GoesMidWaterVapor`)
- [x] C10 lower-level water vapor — new this pass (`FieldLayer::GoesLowWaterVapor`). All three
  water-vapor channels share one ramp (`GOES_WATER_VAPOR`) — same physical quantity, same
  forecaster reading convention at every level, so one shared `static` rather than three
  copy-pasted ones (`render::field_ramps::tests::the_shared_goes_ramps_really_are_the_same_ramp`
  checks this by pointer, not just by value).
- [x] C13 clean IR (`FieldLayer::GoesIr`, was already done)
- [x] C15 dirty/split-window IR — new this pass (`FieldLayer::GoesDirtyIr`). Deliberately shares
  the C13 clean-IR ramp rather than getting its own — the two channels read almost identically on
  their own; this channel's actual value is as the other half of the split-window (Band 15 minus
  Band 13) dust/ash difference technique E6 hasn't built yet. Shipped standalone first since the
  channel has to exist before that difference can be computed.
- [ ] C01 blue, C03 veggie, C05 snow/ice — not done. These are the remaining true-color/RGB-recipe
  input channels (E4); no standalone analyst use case for them was obvious enough to prioritize
  ahead of RGB recipe work actually needing them, unlike C07/C09/C10/C15 which each have a real
  standalone reason to exist today.
- [ ] C14 longwave IR — not done. Reads almost identically to C13 clean IR (both are atmospheric-
  window channels a few tenths of a micron apart) with no standalone or difference-product use
  case as clear as C15's, so it wasn't added just to complete the letter/number list.
- [ ] "other channels necessary for RGB recipes" — none of E4's recipes are built yet, so nothing
  further was pulled in on their behalf; revisit per-recipe once E4 actually starts.

Every new channel reuses the exact wiring the original three established (`OverlaySource::Goes`'s
band lookup in `app.rs`, the same 300s CONUS refresh cadence, the same satellite-flip refetch
logic) — no new fetch/render machinery, just more `FieldLayer` variants. 612 hookecho tests passing
(2 new), native + wasm32 checks clean.

**DQF/quality-mask preservation — done, all channels, same pass:** `wxdata::goes_abi::decode` now
also reads the granule's `DQF` band and masks (`mask_by_dqf`) any `CMI` pixel whose flag isn't 0
("good") to `NaN` before reprojection — 1 conditionally usable, 2 out of range, 3 no value all
become the same "no data" a genuinely off-disk pixel already was, rather than reading as a spurious
cold/warm value. Applies uniformly to every channel (old and new) since they all go through this
one `decode` function. A DQF read failure (unexpected for a real CMIP granule, but not assumed
impossible) degrades to no masking rather than failing the whole decode. Unit-tested directly
against synthetic flag arrays (`mask_by_dqf_clears_every_non_zero_flag`,
`..._leaves_cmi_untouched_on_a_shape_mismatch`, `..._treats_a_nan_flag_as_not_good`) and against
the real fixture's own `DQF` band (`decode_reads_the_fixtures_own_dqf_band_at_matching_shape`) —
that fixture turns out to be an entirely clean scene (every pixel DQF 0), a real finding rather
than a reason to weaken the test, so it asserts what that fixture can actually support (decode
still succeeds, output isn't mostly emptied) and reserves the stronger "output fraction actually
drops" assertion for whenever a fixture with real bad pixels is available. 480 wxdata tests passing
(4 new).

## E3. Projection — done

`wxdata::goes_abi::Projection` implements the real GOES-R fixed-grid geostationary navigation
equations (NOAA PUG-L2+ vol. 5 §4.2.8.1 — the same formula `satpy`/`goes2go`/NOAA's own sample
scripts use), derived per-fetch from each granule's own `goes_imager_projection` attributes rather
than hardcoded (GOES-East and -West don't share one — different `longitude_of_projection_origin`).
Forward-projects every source pixel once into a regular lat/lon grid (nearest-source-pixel-wins
scatter) rather than solving the fiddlier inverse problem or shader-projecting per frame — a
deliberate simplification from this section's original "prefer shader projection" suggestion,
justified by not needing a second projection path alongside the Mercator tile pipeline every other
gridded overlay already uses. `goes_abi::tests::decodes_a_real_granule_to_a_plausible_geographic_extent`
checks this against a real downloaded granule, not just a synthetic fixture — the "test known
landmarks against NOAA imagery" acceptance criterion, satisfied by geographic-extent plausibility
rather than a named-landmark pixel check specifically.

## E4. RGB recipe engine

Create reusable recipe definitions rather than hard-code each RGB.

Target:

- GeoColor / true-color-style composite
- Day Cloud Phase
- Day Convection
- Air Mass
- Dust
- Fire Temperature
- Night Microphysics
- Sandwich product

Recipe metadata should specify channel inputs, transforms, gamma/ranges and output meaning.

## E5. Rapid-scan handling

- discover active mesoscale sector footprints
- 1-minute frame timeline
- dynamically follow sector movement
- show sector boundary
- graceful switch when the event exits the meso sector

## E6. Satellite analysis tools — partly done

- [x] brightness-temperature sample — closed by J3's gridded-layer probe (see that section), which
  solved both blockers this entry used to describe: a field's decoded CPU grid is now retained for
  exactly as long as its GPU texture is resident, and the probe table reports a source, product
  name, valid time and value for whichever layer is on top rather than being shaped around one
  radar `Moment`. A GOES pixel therefore reads as a brightness temperature in the user's own
  temperature unit, because `GOES_IR`/`GOES_WATER_VAPOR` carry `is_temp_kelvin` and the formatter
  honours it.

  The specific hazard this entry called out is handled rather than merely avoided:
  `GoesDustDiff`/`GoesColdTop` deliberately store a *transformed* quantity (a band difference, and
  degrees colder than the 210 K threshold), and both ramps carry their own non-Kelvin units, so
  the probe labels them as what they are instead of putting a plausible-looking `°C` beside a
  number that is not a temperature. Pinned by its own tests — a wrong number with a convincing
  unit next to it is the failure mode worth a regression test, not the happy path alone.
- [x] channel difference products — `FieldLayer::GoesDustDiff` (new this pass): the classic
  split-window dust/ash technique, Band 13 minus Band 15 brightness temperature
  (`wxdata::goes_abi::fetch_latest_conus_diff`, a new two-band concurrent fetch + cell-by-cell
  subtract — `nx`/`ny` always match between any two `decode()` calls requesting the same output
  resolution, so no resampling is needed, unlike `hookecho::fielddiff::diff`'s model-comparison
  case which this deliberately does *not* reuse: that function's strict exact-valid-time check
  would reject two same-scan-but-different-second GOES bands every time). Rendered through the
  ordinary single-value + LUT pipeline every other field layer uses — no new GPU/shader work —
  by choosing the *sign* of the subtraction so "no dust" and "below the ramp's visible range"
  are the same condition (`render::field_ramps`'s `GOES_DUST_DIFF` doc comment has the full
  reasoning): a plain `Linear` ramp with `lo` above the ordinary channel-noise floor gets a
  deadband-hides-agreement effect for free, the same practical result as `ModelDiff`'s hand-rolled
  deadband LUT, without needing that second rendering path. 483 wxdata tests passing (3 new), 613
  hookecho tests (1 new).
- [x] cold-cloud-top threshold overlay — `FieldLayer::GoesColdTop` (new this pass): the same Band
  13 data as `GoesIr`, re-fetched and transformed to `COLD_TOP_THRESHOLD_K (210 K) - raw` before
  it reaches the field cache, so ordinary cloud (not cold enough to matter) falls below the
  ramp's `lo` and simply isn't drawn — the same value-inversion trick `GoesDustDiff` uses, applied
  a second time. 210 K (\u{2248} -63\u{b0}C) is a commonly used overshooting-top/deep-convection
  cutoff. 614 hookecho tests passing (1 new).
- [x] cooling-rate/time-change product — `FieldLayer::GoesCoolingRate` (new this pass): Band 13
  brightness temperature 15 minutes ago minus right now, so a positive value is cooling (an
  intensifying updraft's overshooting top dropping tens of K in 15 minutes, which a single
  snapshot can't show at all). Needed genuinely new fetch machinery, unlike the two items above:
  `wxdata::goes_abi` gained `key_time` (parses a CMIP filename's own `_s<year><day><hour><min>
  <sec>` scan-start field — confirmed against a real listing from the live bucket, not assumed
  from documentation) and `key_near` (searches the target's hour plus the hour on each side and
  picks whichever real granule is closest, so a missed scan degrades to a slightly different
  actual interval rather than an error) alongside the existing `latest_key`. `fetch_cooling_rate`
  fetches the latest key and the nearest one ~15 minutes earlier concurrently, then reuses
  `diff_fields` — the same shape-checked subtraction `fetch_latest_conus_diff` already uses, just
  on two *times* of one band instead of two bands of one time. Rendered through the same ordinary
  LUT pipeline as every other GOES layer, with its own `GOES_COOLING_RATE` ramp (`lo` at 4 K,
  `hi` at 50 K — calibrated against a live run's own numbers: median |Δ| 1.3 K and 95th-percentile
  16 K across a quiet CONUS scene, with one real 82 K pixel from an actively forming storm cell
  that pass, which is exactly the kind of reading this product exists to surface, not a bug to
  suppress). Verified against the real bucket (`#[ignore = "network"] fetches_the_live_cooling_rate`,
  checking the bulk of the scene rather than its single most extreme pixel, for the reason just
  given). 495 wxdata tests passing (4 new), 619 hookecho tests (1 new), native + wasm32 checks
  clean on both crates.
- [ ] GLM overlay synchronized to frame — now audited: it isn't, and can't usefully be yet. The
  flash-dot overlay (`show_glm` in `app.rs`, near `glm_style`) fades each dot's age against
  `chrono::Utc::now()` — real wall-clock time, not the pane's own analysis time
  (`linked_analysis_time`) or the GOES frame's valid time. That only matches a *live* pane by
  coincidence (now and the live frame's time are close together); scrubbing a pane to an archived
  time would show today's live flashes, faded by their real age, next to yesterday's radar/
  satellite frame — not a stale-sync bug so much as GLM having no archive path at all yet
  (`wxdata::glm::GlmFeed` is a rolling live buffer with no historical query, consistent with A3's
  own "MRMS/model/satellite caching is not built" note). Fixing the age-fade clock alone wouldn't
  fix this — the flashes themselves would still be live-only data misleadingly overlaid on a
  non-live frame. Left open as a real gap rather than a one-line clock fix that wouldn't actually
  solve it.
- [x] radar + satellite dual/quad pane presets — a new "Radar + satellite" starter workspace
  (`crates/hookecho/src/workspace.rs::starters`), reusing J5's existing preset mechanism (data
  only, no new code): reflectivity alone as a baseline, the same moment again with GOES IR then
  water-vapor layered underneath it, and storm-relative velocity for motion, all four panes on
  the user's own site (`adopt_site: true`) and linked (camera/time/site/cursor). Deliberately
  does *not* use a site-less national satellite pane the way "Mesoscale analysis" does — found
  and worked around a real interaction while designing this: `apply_workspace` adopts the active
  site into *any* pane whose snap has no site, which is correct for an all-radar preset but would
  wrongly overwrite a pane meant to stay a national satellite view. Satellite context rides on a
  radar pane's own `fields_on` instead, sidestepping the conflict entirely rather than needing a
  fix to `apply_workspace` itself.

## E7. Offline satellite chase packs

Allow selected time/range/sector frames to be downloaded into chase packs subject to storage budget.

### Acceptance criteria

- [ ] live 1-minute mesoscale frames animate correctly when available — no mesoscale ingest exists
  yet (E1); CONUS's ~5-minute cadence is what's implemented.
- [ ] clean IR and water-vapor values can be sampled numerically — not audited this pass; whether
  the existing generic cursor-probe/data-inspector machinery already covers GOES fields (they're
  stored as the same `MrmsField` grid shape every other gridded overlay uses) or needs its own
  wiring is an open question for whoever picks up E6, not something this pass's channel-count
  expansion answered.
- [ ] radar, GLM and satellite align by valid time — not audited this pass.
- [x] quality/missing pixels are distinct from cold/low values — met as of this pass: E2's own
  entry above covers `wxdata::goes_abi::decode`'s new DQF masking, applied uniformly to every
  channel.

---

# 8. Phase F — Generic U.S. model and ensemble workstation

**Priority: P0.**

Do not build each model as a separate feature. Build a general GRIB model engine and add model definitions.

## F1. Model abstraction — done for the models already wired up

`wxdata::model` holds the definition table (`ModelDef`) and the field catalogue (`ModelField`).
Every model already fetched — HRRR, HRRR pressure, RAP, NAM 3 km nest, NAM 12 km, NBM — is a row;
the GRIB variable/level literals that used to be duplicated across `app.rs`, `fielddiff.rs`,
`severe.rs` and `headless.rs` are now looked up by field *meaning*, and `ModelField::grib`
returning `None` is how a caller learns a model does not publish something.

The table is checked against reality rather than against documentation: a network-gated contract
test (`the_catalogue_matches_what_the_feeds_publish`) pulls a recent `.idx` from every bucket and
asserts both that each claimed field is present and that each field marked unavailable is absent.
It found three errors in the first draft, one of which was a live bug — the NAM family spells
composite reflectivity's level differently from the HRRR, and the matcher compares level strings
exactly, so NAM reflectivity had simply been failing.

Also fixed while here: the fetch path clamped every request to 18 forecast hours regardless of
model or cycle, truncating the NAM nest's 60 h runs and three quarters of HRRR's extended cycles.
The cap now comes from the run's own schedule.

Not covered: vertical-coordinate mappings and a "list runs / list forecast hours" API are still
implicit in `hrrr::recent_cycles`; the byte-range/index strategy stays in `hrrr.rs` rather than
being per-model data, because every model here uses the same `.idx` scheme and inventing a
strategy enum for one implementation would be speculative.

A model definition must include:

- model ID
- provider/base URL
- run cadence
- forecast-hour schedule
- grid/projection
- variable mappings
- vertical-coordinate mappings
- byte-range/index strategy
- expected latency
- domain
- ensemble membership if applicable

Create common APIs for:

- list runs
- list forecast hours
- fetch field
- fetch vertical profile
- regrid/sample
- compare

## F2. Model priority list

### Tier 1

- [x] HRRR — migrated onto the F1 catalogue (definition + field mappings; fetch/regrid unchanged)
- [x] RAP — migrated onto the same catalogue, including its analysis use at f00
- [ ] GFS — expand beyond current comparison fields. Still on its own path in `global.rs`, which
  fetches from a different bucket layout with a different index scheme; folding it in needs the
  per-model byte-range strategy F1 deliberately did not invent yet.
- [ ] RRFSv1 deterministic
- [ ] REFS / RRFS ensemble members
- [x] GEFS — found already fully built while surveying this section, the same pattern as A1/C3/D1's
  own stale checkboxes: `wxdata::global::GlobalModel::Gefs` fetches the real 0.5° ensemble-mean
  bucket (`noaa-gefs-pds`), is one of the four pills in the ribbon's model-mode source picker
  (`app/chrome/ribbon.rs`) and the layer-options global-model list, and is covered by the
  `global_live`/`point_series_live` network tests. Re-verified live this pass: `global_live`
  fetched real GEFS-mean MSLP (600×300, 100% finite) alongside GFS/ECMWF/GDPS in the same run.
- [ ] NBM — genuinely open, not stale: `wxdata::hrrr::Model::Nbm` already has real GRIB mappings
  for the fields it publishes (confirmed live against `blend.t18z.core.f001.co.grib2.idx`:
  `TMP`/`DPT` at "2 m above ground" match the existing generic key exactly), and explicit `None`
  opt-outs for the fields it genuinely doesn't (composite reflectivity, mixed-layer CAPE, SRH,
  MSLP) — but it publishes wind as a direct `WIND` speed scalar rather than `UGRD`/`VGRD`
  components, and there is no regional 10 m wind `ModelField` at all yet (F3's "10 m wind/gust"
  target), only the separate vector-wind fetch path the particle layer uses. Nowhere in the UI
  offers NBM as a general model to browse today: the one ribbon group that lists alternate models
  (HRRR/RAP/NAM/NAM12) is specifically the CAPE/SRH environment suite, and NBM cannot serve two of
  those three fields — adding it there would offer a model that silently breaks half of what the
  picker is for. Making NBM genuinely browsable needs its own field-based (not per-model-list)
  surface, consistent with A1/F1's own registry philosophy, not a one-line addition to that picker.

### RRFS timing note

As of 2026-09-12, NOAA’s current published implementation schedule lists RRFS/REFS v1 operational implementation for **2026-10-06**. Build the adapter against the available NOAA/AWS parallel feed now, but do not assume final operational naming/availability until contract tests pass after implementation.

### Tier 2

- [ ] ECMWF open IFS fields useful over the U.S. if current licensing/access remains compatible
- [ ] ECMWF ensemble products only where openly and legally retrievable
- [ ] NOAA-accessible AI guidance such as GraphCast products where stable public feeds exist
- [ ] experimental guidance behind an explicit EXPERIMENTAL label

Do not depend on fragile scraped images when machine-readable grids exist.

## F3. Generic field set

Support by metadata rather than bespoke UI.

### Surface

- 2 m temperature
- 2 m dewpoint
- RH
- 10 m wind/gust
- MSLP
- visibility
- precipitation type
- accumulated QPF
- snowfall
- freezing rain / ice where available
- simulated reflectivity
- updraft helicity / rotation proxies
- smoke/aerosol where available

### Pressure levels

- height
- temperature
- dewpoint/RH
- wind
- vertical velocity
- vorticity

At minimum: 1000/925/850/700/500/300/250/200 hPa where the model provides them.

### Severe diagnostics

- SBCAPE
- MLCAPE
- MUCAPE
- CIN
- LCL/LFC/EL
- 0–1 / 0–3 SRH
- bulk shear layers
- STP
- SCP
- EHI
- effective-layer diagnostics

Where source grids do not publish the diagnostic, compute it from the vertical profile using one shared meteorological calculation library rather than different formulas per model.

## F4. Model display modes

Every appropriate field should support:

- shaded fill
- contours
- barbs
- arrows
- wind particles
- streamlines where feasible
- point sample
- time series

## F5. Run-to-run comparison — partly done

Example:

- [x] current HRRR run minus previous HRRR run at same valid time — new this pass, see the
  Unreleased CHANGELOG entry: `DiffField::RunToRunCape`, fed by a new
  `wxdata::hrrr::fetch_field_previous_run` that walks back from a specific run (not `Utc::now()`)
  so it can never return the same cycle as the one it's compared against. Fixed at the analysis
  hour (lead 0) on both sides, the same choice the existing HRRR/RAP comparison makes and for the
  same reason — a larger lead would need the previous cycle to still be within its own 18 h
  publish window for the same valid time, which isn't always true. Verified live: comparing
  HRRR's 03Z and 02Z cycles (one real hour apart, confirmed from the fetched runs' own
  timestamps) produced a real subtracted CAPE grid (median 0 J/kg, middle-90% spread
  −280..390 J/kg — visibly tighter than the ±1500 J/kg cross-model HRRR/RAP range, exactly the
  "same model, one cycle apart" character this comparison is supposed to have).

Support:

- [x] scalar difference — the subtraction itself, reusing `fielddiff::diff`
- [x] absolute difference — new this pass: `DiffMode::Absolute` draws `|a − b|` on a sequential
  amber-to-red scale, sharing the signed view's transparent agreement deadband, for the "where do
  these disagree at all" scan where direction is noise. Available to every comparison field, not
  just run-to-run. The fetched CPU grid stays signed and authoritative, so switching modes
  recolors the resident field rather than refetching either model — a separate display key tracks
  what the GPU upload currently represents. The legend's bar, ticks and title follow the mode, and
  the cursor readout is put through the same transform the upload was, so a magnitude-colored map
  cannot hand back a negative number.
- [ ] percentage difference where meaningful — still open, and specifically *not* the fourth
  `DiffMode` variant it looks like from the outside. `Signed`/`Absolute`/`Disagreement` are all
  pure display transforms of one retained grid, which is why switching between them recolors
  without refetching. A percentage needs the denominator, and the difference path only ever keeps
  `a − b` (`diff_grid` is one `MrmsField`); both sides survive only in `compare_grid`, which the
  separate compare-panes fetch populates. So this costs a fetch/retention change, not a match arm
  — and it needs a decision about what a percentage means where the denominator is near zero,
  which for CAPE or a difference field is most of the domain.
- [x] threshold highlighting — the existing deadband mechanism (`DiffField::range`'s second
  number): differences inside it draw as fully transparent, same as every other comparison field
- [ ] synchronized side-by-side panes — deliberately not offered for this field: the compare-panes
  mode shows each side's own distinct single-model layer, and there is no distinct "previous run"
  layer to show yet (both panes would draw today's current-run CAPE). `DiffField::
  supports_side_by_side` returns `false` for it and the UI hides the button rather than shipping
  a panel that would silently show the same data twice.

Only CAPE is wired up so far — the roadmap's own worked example, not the full field set F3
eventually wants comparable this way.

## F6. Model-to-model comparison — partly done

Expand existing `fielddiff.rs` into a general comparison system.

Modes:

- [x] A - B difference — predates this pass: the `ModelDiff` subtraction overlay
- [x] side by side — predates this pass: `PaletteAction::CompareInPanes`, two linked panes
- [x] swipe divider — new this pass, see the Unreleased CHANGELOG entry: one pane callback draws
  A and B through complementary GPU scissor rectangles, so it remains correct under egui_wgpu's
  prepare-all-then-paint ordering instead of racing two callbacks for the same pane. The divider
  has its own drag target and visible handle/model labels; dragging it cannot pan or draw on the
  map. Hover values and the linked multi-pane probe select the grid physically under the cursor,
  and the legend names both halves.
- [x] blink A/B — new this pass, see the Unreleased CHANGELOG entry: a "Blink A/B" button
  alternates the *active pane's own* `fields_on` between `CompareA` and `CompareB` on a 1.5 s
  timer, reusing the exact same render path "View side by side" already uses (`field_draws` is
  built fresh from `fields_on` every frame) — no GPU/shader change needed at all, unlike swipe.
- [x] disagreement mask — new this pass, see the Unreleased CHANGELOG entry: a third `DiffMode`
  renders one categorical magenta class wherever `|A − B|` exceeds that field's existing
  scientifically chosen deadband, with agreement and missing data transparent. It recolors the
  retained signed grid in place, so switching among signed/absolute/mask never refetches models;
  legends and both probe surfaces name the class while retaining the measured delta.

New this pass, transparent overlay is also available anywhere side-by-side/blink is scientifically
valid: model A draws normally and model B draws over it at 50% of the user's configured opacity,
with one shared scale and an explicit two-model legend. This required fixing the renderer's old
single opacity uniform per field — egui_wgpu prepares all panes before painting any, so the last
prepared pane could overwrite every earlier pane's field opacity. Textures/LUTs remain shared, but
each field now owns one tiny uniform/bind group per pane, making the overlay genuinely pane-local
and eliminating that latent cross-pane clobbering for ordinary field opacity too.

## F7. Ensemble workstation — started (engine, headless render and an on-map layer)

This is required for top-tier analysis.

New this pass, see the Unreleased CHANGELOG entry: `wxdata::ensemble` fetches the 31 GEFS members
of a field from one pinned cycle and reduces them per grid cell (`combine`). It is checked offline
and against live GEFS, and `hookecho --headless-ensemble <field> <stat> <hour> [out.png]` renders
any statistic to an image. The app has a "GEFS ensemble" layer (Models group) with a field picker,
mean/spread/min/max/10th/90th percentile/exceedance probability, an editable threshold, the shared
forecast-hour slider, a legend, and a hover readout. The 31 members are fetched once and kept, so
changing the statistic or threshold recomputes locally instead of refetching. That layer's UI was
built and unit-tested but not yet exercised by hand in the running app. GEFS is the only ensemble wired up;
REFS is not a data source this app has. Fields today: 2 m temperature, MSLP, 500 hPa height,
mixed-layer CAPE, precipitable water and 6-hour rain (QPF). Wind gust, snow and UH need fields the
`pgrb2a` member files do not carry, so they are still open.

For GEFS/REFS and any supported ensemble:

- [x] individual member view — `fetch_gefs` returns every member grid (engine only, no UI)
- [x] ensemble mean
- [x] ensemble spread / standard deviation — sample standard deviation
- [x] min/max
- [x] percentile fields — linearly interpolated between ranked members
- [x] probability of threshold exceedance — percent of members strictly above, engine and headless
- [ ] neighborhood probability when scientifically appropriate
- member postage-stamp grid
- spaghetti contours
- [x] point plume/time series — the forecast window's "Ensemble plume (GEFS)": the mean with a one-standard-deviation band at the tapped point, from the ready-made mean and spread files (so it shows the spread, not individual members or percentiles)
- ensemble sounding overlay

Probability examples:

- CAPE > threshold
- wind gust > threshold
- [x] QPF > threshold — 6-hour rain from the GEFS's own accumulation, at leads that close a six-hour window
- snow > threshold
- UH > threshold for convection-allowing ensembles

## F8. Point sounding overhaul

Point sounding should support:

- any supported deterministic model
- ensemble member soundings
- ensemble envelope
- observed RAOB overlay
- previous model run overlay
- parcel selection
- Bunkers vectors
- effective inflow layer
- freezing/-10/-20/-30 C heights
- DCAPE
- lapse rates
- PWAT
- hodograph layer coloring
- storm-motion user override
- downloadable CSV

### Acceptance criteria

Adding a new model with an already-supported GRIB/projection format should primarily require a model definition + field mappings, not a new renderer.

---

# 9. Phase G — RTMA/URMA and objective surface analysis

**Priority: P1.**

## G1. RTMA/URMA — partly done

Add U.S. Real-Time Mesoscale Analysis / UnRestricted Mesoscale Analysis fields where publicly available:

New this pass, see the Unreleased CHANGELOG entry: `wxdata::rtma` reads the RTMA 2.5 km hourly
analysis, and the model picker offers it as an "Analysis" model with no lead.

- [x] 2 m temperature
- [x] dewpoint
- [x] 10 m wind — speed, published directly by the analysis
- [x] wind gust
- [ ] pressure — the file carries surface pressure only, not sea-level; needs a reduction before it means anything on a map
- [ ] visibility — in the file, not yet given a layer or a color scale
- [ ] precip analysis fields as appropriate — a separate `pcp` product in the same bucket

- [x] Expose analysis age — the stamp names the analysis hour and the fetch age; the Hour menu lists a day of hourly analyses
- [ ] Distinguish RTMA real-time analysis from URMA retrospective analysis — only the real-time analysis is read; URMA is a separate archive not wired up

## G2. Observation + analysis blend

Finish the existing roadmap item about blending surface observations into effective-layer analysis.

Use:

- METAR
- NDBC
- supported PWS feeds
- RTMA/URMA
- RAP/HRRR profiles aloft

The purpose is not to pretend the result is official SPC mesoanalysis. Label it clearly as **HookEcho objective analysis**.

## G3. Surface objective analysis tools

- station residuals vs analysis
- contour generation
- dewpoint gradient
- theta-e
- moisture convergence if scientifically valid from available gridded winds/moisture
- temperature advection
- frontogenesis diagnostics later

## G4. Time-series station/analysis comparison

At a selected point/station show:

- observed
- RTMA
- HRRR analysis
- forecast model traces

This is useful for model bias and boundary evolution.

---

# 10. Phase H — Advanced 3D and cross-section workstation

**Priority: P1.**

Current `wxdata::volume3d` resamples reflectivity into a Cartesian 3D grid and `render3d.rs` displays it. Build on this rather than replacing it blindly.

## H1. Multi-moment 3D

Support 3D for:

- reflectivity
- velocity/SRV where scientifically interpretable
- ZDR
- CC
- KDP
- user-defined products

## H2. Transfer-function editor

Add analyst controls:

- opacity vs value curve
- color transfer function
- threshold clipping
- value window
- vertical exaggeration
- quality mask

Save presets per product.

## H3. Isosurfaces

Implement GPU/CPU isosurface generation, preferably marching cubes or equivalent.

Examples:

- 40/50/60 dBZ surfaces
- low-CC debris region
- ZDR column threshold

Controls:

- threshold
- transparency
- lighting
- smoothing toggle

Never smooth the source values silently; smoothing must be explicit display processing.

## H4. Movable clipping and slicing planes — done

- [x] arbitrary vertical plane — `render3d::VerticalPlane` (bearing + offset), a new field in
  `raymarch.wgsl`'s uniform, checked per-sample in the march. One implementation shared by both
  raymarch consumers (the standalone "3D Reflectivity" window and the main map's "3D map" Smooth
  representations) via `View3d`, with one shared UI widget (`ui::volume3d_window::plane_controls`)
  used by both. Verified against real data on real GPU hardware via a new `--headless-3d ... --plane
  BEARING,OFFSET` CLI flag: sweeping the offset from one box edge to the other took the rendered
  echo pixel count continuously from the unclipped baseline down to zero.
- [x] horizontal CAPPI plane *inside the 3D view* — new this pass, see the Unreleased CHANGELOG
  entry: a translucent horizontal reference plane at the CAPPI window's own altitude (shared, not
  a second value — dragging its slider moves the marker live), drawn in both raymarch consumers via
  a new `View3d.cappi_km` field, same sharing pattern as the vertical plane above. An analytic
  ray/band test in `raymarch.wgsl` (mirroring the box-slab intersection already there) composites
  it as a faint backdrop *behind* whatever the volume itself draws, so real echo always wins where
  a ray crosses both — it reads as "where this height sits relative to the storm" rather than a
  haze sitting on top of it. `render3d::cappi_marker_uniform` treats an altitude outside the
  volume's own vertical span as disabled rather than pinning it to the nearest edge, which would
  show a plane at the wrong height. Verified against real data on real GPU hardware via a new
  `--headless-3d ... --cappi ALT_KM` CLI flag: with the reflectivity threshold pushed impossibly
  high (so the volume itself contributes zero pixels), a 3 km marker alone painted 346,318 echo
  pixels over the clear background, and a 25 km marker (above the volume's 18 km top) painted zero
  — inert exactly as designed rather than pinned to the ceiling.

- [x] slab thickness — new this pass, see the Unreleased CHANGELOG entry: `VerticalPlane` gained an
  optional `thickness` (same fraction-of-box convention as `offset`); the shader keeps only a band
  of that half-width straddling the plane instead of cutting one whole side away when set. Verified
  on real GPU hardware via `--headless-3d --plane BEARING,OFFSET,THICKNESS`: KTLX echo pixel counts
  grew monotonically with thickness (9,181 → 35,682 → 167,468 at 0.02/0.15/1.0), with 1.0 landing
  exactly on the unclipped baseline.
- [x] clip box — the pre-existing axis-aligned `clip: [f32;6]` slab, independent of this pass
- [x] storm-centered clip — pre-existing `volume3d::clip_around`, independent of this pass
- [x] cross-section line visible in map pane — new this pass, see the Unreleased CHANGELOG entry:
  `render3d::plane_ground_track` derives the plane's ground track from the same bearing/offset math
  `plane_uniform` feeds the shader, drawn as a violet line on the 2D map (Smooth representations
  only — Observed has no box for a plane to cut into). Verified live: the line ran through the
  radar site at bearing 0/offset 0, rotated in place when Bearing moved to 310°, and shifted
  sideways when Offset moved, all matching the 3D view's own cut.

## H5. Beam visualization in 3D

Show:

- individual elevation cones
- beam centerline
- approximate beamwidth
- radar location/elevation
- terrain surface option

## H6. 3D overlay fusion

After radar 3D is mature, add optional:

- MRMS layer-height surfaces
- satellite cloud-top-height surface when a trustworthy source exists
- model isosurfaces for selected scalar fields

Keep observed radar, analyzed MRMS and forecast model geometry visually distinct.

## H7. 3D performance targets

- desktop target: interactive 60 fps on a representative mid-range GPU for default volume resolution
- mobile/web target: adaptive resolution with explicit quality setting
- avoid rebuilding volume grid when only camera changes
- cache 3D textures per volume/product

---

# 11. Phase I — Professional GIS engine

**Priority: P1.**

Placefiles are not enough for emergency-management, research and broadcast users.

## I1. Import formats — GeoJSON reachable end to end for polygons

Implement in this order:

1. [x]/[ ] GeoJSON — parsing landed in an earlier pass (`wxdata::gis::parse_geojson`, reusing
   `overlay::for_each_feature`'s FeatureCollection/Feature dispatch and "ArcGIS reports failure as
   HTTP 200" detection). New this pass: the two things that pass explicitly left open — a way to
   reach it from the app, and rendering the result — for polygons specifically. `dialog::ImportKind
   ::GisFile` opens the existing cross-platform file picker (desktop/Android/web, the same
   mechanism `ChaseGpx`/`Palette`/etc. already use); "Import GIS file…" in the Tools palette
   triggers it. The new `gis_import` module converts what it gets back into
   `wxdata::overlay::GeoFeature` — the *same* rings-plus-fill/stroke shape every NWS/SPC feed's
   polygons already render and hit-test through (`app.rs`'s `rebuild_overlays` assembles one
   combined `Vec<GeoFeature>` from all of them) — so an imported polygon draws, is clickable
   (showing every one of the file's own attributes in the click popup, sorted, since a first
   import has no way to know which fields the person who clicked actually cares about), and needed
   no new rendering code at all: a new `FeatureKind::Imported` (lowest hit-test priority, so a
   reference shape never steals a click from something operationally meaningful it overlaps) was
   the entire rendering-side change. A `MultiPolygon` splits into one `GeoFeature` per part, since
   `GeoFeature::rings`' "ring 0 outer, rest holes" convention is already one polygon's worth.
   **Points and lines now draw too** (a later pass, closing the boundary this entry used to
   describe): they can't ride `GeoFeature`'s rings-only shape, so they come back as
   `gis_import::Marks` and are painted directly by `render_pane` through the same lon/lat → world
   → screen projection the freehand annotation strokes already use — points as outlined dots (one
   flat color cannot stay legible over both a bright radar core and a dark basemap), lines as
   polylines. A file of city sites or a river/road network used to import as nothing visible,
   which is a weak answer for the format's most ordinary contents. What is *actually* left to I4
   is narrower than "half your file is dropped": per-layer color and width, labels from a chosen
   attribute, symbol/graduated color by category.

   An import also frames the map on what it just loaded, with "Zoom to imported shapes" in Tools
   to do it again after panning away — a file covering somewhere the map isn't looking otherwise
   imports to no visible effect at all. The fit is computed in world units rather than degrees,
   since a latitude degree is not a constant height under Mercator and fitting on degrees
   overshoots badly away from the equator.

   **The import is remembered across restarts** (a later pass; it used to be session-only, which
   made a boundary or asset file someone works with daily cost a re-pick on every launch).
   Remembered exactly the two ways an imported `.pal` already is: a path where there is a
   filesystem, and the content itself in a browser, which has no path that would survive a reload.
   A file that has since moved or been deleted is reported rather than swallowed — the layer
   simply not being there is otherwise indistinguishable from the app having forgotten it, and
   only the person can fix a missing file. The reference survives that failure rather than being
   dropped, since a drive that isn't mounted this launch will likely be mounted the next.
   679 hookecho tests passing, native + wasm32 checks clean, clippy's `-D warnings` gate green.
2. [x] ESRI Shapefile (`.shp/.shx/.dbf`, optional `.prj`) — `wxdata::shapefile`, from the spec with
   no dependency, returning the same `GisFeature` GeoJSON does, so drawing, click popups,
   zoom-to-fit, remember-across-restarts and GeoJSON export all work on it unchanged. Reached
   through the same "Import GIS file…" picker, which now also takes `.shp`.
   - Geometry: points, multipoints, polylines and polygons including their Z and M forms (the
     extra values are skipped); holes are attached to the outer ring that contains them, several
     outer rings in one record become a multipolygon, a null shape is skipped but still counts so
     attributes stay aligned, `MultiPatch` is a named error. `.shx` is not needed.
   - Attributes: `.dbf` text, numbers (an integer column stays an integer), dates, logicals; a
     row deleted in the `.dbf` is dropped. A `.dbf` whose row count differs from the `.shp` is
     refused rather than matched up anyway — that would attach the wrong attributes to shapes.
   - Coordinate system (I2's rule): WGS 84 and NAD 83 read as lon/lat, Web Mercator is
     inverse-projected, and anything else — every State Plane and UTM zone, and the older NAD 27
     datum, which can sit tens of metres off — is a named error rather than misplaced geometry.
     With no `.prj`, coordinates must be a plausible lon/lat or the import is refused.
   - Hostile input: every read is bounds-checked and a length field cannot drive an allocation;
     a test cuts a valid pair at every byte and requires an error or a result, never a panic.
   - Where the sidecars come from: on desktop the `.dbf` and `.prj` are read from beside the
     picked `.shp` (any extension case). A browser or phone picker hands over one file, so there
     the shapes import without attributes and the toast says so. A shapefile remembered in a
     browser is stored as the GeoJSON it reads back as.
   - Not built: reprojection from other coordinate systems (I2), selecting several files at once
     in a browser, `.cpg` codepages (text is UTF-8, falling back to Latin-1). Tested against
     synthetic files written by the test suite; **not yet against real-world shapefiles from GIS
     software, nor exercised through the picker in a running app.**
3. [ ] KML
4. [ ] KMZ
5. [ ] GeoPackage if a cross-platform Rust path is practical

## I2. Projection handling

- parse CRS from source metadata
- transform to WGS84/Web Mercator display coordinates
- support common U.S. EPSG projections
- reject unknown projections with a useful error instead of silently misplacing geometry

Not started. Moot for GeoJSON specifically (the format is specified as always WGS84), but real
for Shapefile's own `.prj` once that importer exists.

## I3. Geometry types — done, for GeoJSON

- [x] point — `wxdata::gis::Geometry::Point`
- [x] multipoint — `Geometry::MultiPoint`
- [x] line — `Geometry::LineString`
- [x] multiline — `Geometry::MultiLineString`
- [x] polygon — `Geometry::Polygon`, rings (outer + holes), matching `overlay::GeoFeature`'s own
  convention
- [x] multipolygon — `Geometry::MultiPolygon`

A `GeometryCollection` (an eighth GeoJSON shape, not in this list) flattens into one entry per
shape it holds rather than becoming a ninth variant of its own — a caller asking "every shape in
this feature" doesn't need to know the source nested some of them one level deeper. Unit-tested
against a real multi-geometry `FeatureCollection`, a bare `Feature`, a bare `Geometry` (all three
are valid top-level GeoJSON), a `GeometryCollection`, and polygon holes surviving as a second ring
— 7 tests, all passing, plus confirming the `geojson` crate itself refuses to deserialize a
degenerate one-coordinate `Position` before this module's own code ever sees one (a defensive
check for the same case exists here anyway, since that's not a guarantee this module controls).

## I4. Styling

- [x] stroke color/width — the imported layer has one persistent color and screen-space outline
  width shared by polygons, lines and point symbols. Width is clamped at the render boundary,
  remains visually stable across zoom levels, and never changes official warning/outlook edges
- [x] fill/opacity — Layer Manager exposes the same color plus an opacity slider, applied to both
  polygon fill and outlines without mutating the imported geometry. Existing settings default to
  the original neutral blue byte-for-byte, and a remembered file now returns visible rather than
  reloading silently behind an off toggle
- labels from chosen attribute
- symbol by category
- graduated color by numeric attribute
- visibility by zoom
- z-order

## I5. Time-aware GIS

Allow a user to map attributes to:

- valid start
- valid end

Then hide/show features with the HookEcho timeline.

## I6. GIS export — mostly done

"Export map as GeoJSON…" in Tools writes one file through `gis_export`, which translates what is
already held in `[lon, lat]` into `wxdata::gis`'s own types and out through the writing half of
the same parser the importer uses — so an export reads straight back in, which is what the
round-trip tests assert rather than pinning a hand-written JSON string.

Deliberately "what is on the map", not "everything fetched": the overlay source is `rebuild_
overlays`' own assembled set, so filters and toggles are already applied and an export matches
what the user is looking at. Every feature carries a `hookecho` property naming its origin, so a
re-import (or a third-party tool) can tell an annotation from a warning polygon without inferring
it from geometry. Two correctness details worth naming: locally drawn rings are closed on the way
out, because GeoJSON requires a closed linear ring and an `AlertPolygon` the user clicked out is
not one; and a storm cell's missing values are omitted rather than written as `null`.

Export:

- [x] drawn annotations to GeoJSON — freehand strokes as `LineString`, each carrying its own
  colour. A scribble is a line even when drawn as a closed-looking circle: that is the usual
  "circle this storm" gesture, not a polygon the user declared.
- [x] storm tracks to GeoJSON — cells as points carrying movement bearing/speed, max dBZ, echo
  top and VIL where the scan has them. A cell position with none of that beside it is the one
  part of this that would be useless in another tool.
- [x] selected warning geometry — every displayed overlay polygon (warnings, watches, outlooks,
  ProbSevere, fire perimeters, imported shapes), each with its kind and title
- [x] markers and watch zones — not in this list originally, but they are the other two things a
  user draws and would expect to get back out
- [ ] sampled/threshold contours — no contour geometry is generated in a form that could be
  exported; the contour overlays draw directly rather than producing vector features
- [ ] route geometry — L1's route engine is not started, so there is nothing to export

### Acceptance criteria

[ ] Not met — a county-level shapefile in a non-WGS84 but declared projection renders in the
correct U.S. location and can be styled by attribute. This names Shapefile specifically (I1 item
2, not started) and I4's styling (not started); I1's GeoJSON parsing landing first doesn't move
this criterion, which was never about GeoJSON to begin with.

---

# 12. Phase J — AWIPS-style analyst workspace

**Priority: P1.**

HookEcho has saved workspaces and up to nine panes. Extend this into a serious analysis layout
system.

## J1. Layouts — done

Support:

- [x] 1 pane
- [x] 2 horizontal/vertical — `pane_rects` already picked orientation adaptively (stacked in
  portrait, side-by-side in landscape)
- [x] 3 pane — new this pass, see the Unreleased CHANGELOG entry: `pane_rects` gave `n == 3` its
  own case (three columns in landscape, three rows in portrait, same adaptive rule as 2-pane)
  instead of falling through to the 2x2 grid truncated to three cells, which left one quadrant of
  screen permanently blank. Exposed as a fourth pill next to 1/2/4 in the ribbon and a fourth
  "N pane(s)" command-palette entry — `set_pane_count`'s own `clamp(1, 4)` already allowed 3
  through, so nothing downstream needed to change to reach it, only the layout math and the UI
  that was missing an entry point for it.
- [x] 4 pane
- [x] 6 pane — new this pass. The real blocker was exactly what it looked like: raising
  `set_pane_count`'s `clamp(1, 4)` alone would have let a 5th/6th pane index into
  `MapVolume3dResources::panes`/`uniform_bufs` (`render3d.rs`) and `HookEchoApp::smooth_vol_key`/
  `_rx`/`_pending`/`_dims` (`app.rs`) — fixed `[T; 4]` arrays every one of those. Went with the
  "fixed larger cap" option this section's own note offered rather than converting to `Vec`: every
  one of these six arrays is already built with `std::array::from_fn`, which doesn't care about
  size, so bumping the literal `4` to `6` was the entire change at each site — no resize-on-pane-
  count-change logic needed anywhere. (The "blockage/lowest-tilt textures' keying" this note used
  to also blame turned out not to be part of this blocker on closer look: `blockage_tex`/
  `lowest_tilt_tex` are single shared cache slots, not per-pane arrays — they already behave the
  same way at 1, 4, or 6 panes, just re-render more often if different panes want different
  overlays at once, which predates this pass and isn't specific to the pane-count ceiling.) Added
  `pane_rects`'s own `6` case (adaptive 3x2 landscape / 2x3 portrait grid, same convention as
  2/3-pane), a `5` case for robustness (`apply_workspace` calls `set_pane_count(ws.panes.len())`
  with no bound of its own, so a workspace file naming exactly 5 panes — unreachable from any UI
  control in this app, which now jumps 4 -> 6 — could otherwise have landed on the old 2x2
  fallback's `truncate(n.clamp(1, 4))` and silently dropped a view with no rect at all), and a
  sixth ribbon pill / command-palette entry next to 1/2/3/4. 618 hookecho tests passing (5 new:
  five/six-pane rect count, the 3x2/2x3 grid shape, tiling with no gaps or overlap, and the 5-pane
  robustness case), native + wasm32 checks clean.
- [x] 9 pane on desktop/web where practical — new this pass, see the Unreleased CHANGELOG entry:
  a shared platform ceiling now keeps every per-pane CPU/GPU cache in lockstep (9 desktop/web,
  still 6 on Android), `pane_rects` provides the real 3x3 layout, and the ribbon plus command
  palette expose it. Imported seven/eight-pane workspaces use the same grid without silently
  dropping views, and layout tests pin counts, axes and full-extent tiling.
- [x] AWIPS-style asymmetric layouts — new this pass: every 3–9-pane workspace can switch from
  the equal strip/grid to a persisted `Focus` arrangement. Pane 1 receives roughly five-eighths
  of the useful axis and every supporting pane tiles into an adaptive detail rail (right side in
  landscape, bottom in portrait; one or two rail columns/rows according to count). Geometry tests
  cover every supported count and orientation, assert that the primary remains larger, and reject
  overflow/overlap. `Even`/`Focus` share one action across the WSV3 ribbon and command palette;
  saved workspaces round-trip the choice, old JSON defaults to `Even`, and the Hail analysis preset
  demonstrates the focus layout with reflectivity large beside ZDR/CC/KDP.

Android may use fewer panes based on screen size.

## J2. Link groups — partly done

This section's own three pre-existing items (camera, zoom, time) are not independent *groups* in
the sense the rest of this section asks for — one global on/off per attribute, shared by every
pane, rather than each pane choosing which of several named groups to join. Documented honestly
as the simpler thing it is rather than claimed as the fuller feature; building real multi-group
membership (a pane picking "Group A" vs "Group B" per attribute) is separate, larger work not
attempted here.

Each pane should independently join link groups for:

- [x] camera/location — `link_cameras`, predates this pass
- [x] zoom — the same `link_cameras` flag; the copied camera struct carries zoom, so the two were
  never separable in this app's model
- [x] time — `link_times`, predates this pass
- [x] cursor/crosshair — new this pass, see J3 below: a `LinkCursor` toggle ("Link pane
  crosshair") shares one hovered geographic point across every pane
- [x] radar site — new this pass, see the Unreleased CHANGELOG entry: a `link_site` toggle
  (`OverlayToggle::LinkSite`) that makes `PaletteAction::SetSite` set every pane's site, not just
  the active one's. Each pane keeps its own product/tilt, so this is for "four products of one
  storm" rather than making every pane identical — the "Chase" and "Analysis" starter workspaces
  (which already show one site across every pane) now default it on; "National overview" (one
  pane) defaults it off, same as its existing `link_cameras`.
- [ ] storm selection — no cross-pane storm-selection concept exists yet to link at all

This enables, for example, four products locked in location/time but not product.

## J3. Synchronized crosshair/probe — partly done

Moving cursor in one pane should optionally show corresponding point in linked panes and a compact table:

| Pane | Source | Product | Time | Value |
|---|---|---|---|---|

- [x] corresponding point shown in linked panes — new this pass, see the Unreleased CHANGELOG
  entry: a `LinkCursor` overlay toggle ("Link pane crosshair") shares whichever pane is hovered
  as one geographic point (`HookEchoApp::linked_probe`), and every pane draws its own crosshair
  at that point via its own camera — so panes at different zooms or locations still mark the same
  spot, not just mirror one screen position. Cleared the instant the pointer leaves every pane, so
  a stale mark never lingers once nothing is actually hovered.
- [x] compact cross-pane value table — `ui::cursor_probe`, a small always-visible window listing
  Pane/Source/Product/Time/Value for every pane, reusing `inspect_gate` (the same sampler the
  Interrogate tool's click already used) once per pane at the shared point.
- [x] MRMS/model grid layers in the table — new this pass, see the Unreleased CHANGELOG entry:
  every decoded field now retains its displayed/decimated CPU grid for exactly as long as the
  corresponding texture remains resident (the existing five-minute field eviction clears both).
  The linked probe selects the same top visible layer as the renderer/legend, samples that grid at
  the shared geographic point, and reports its source, analyst-facing product name, authoritative
  valid time and value. This covers descriptor-backed MRMS/model/GOES/NDFD fields, legacy/local
  grids, signed or absolute model differences, and each compare-pane side; categorical codes use
  their legend labels and Kelvin-backed fields honor the user's temperature unit. With no grid on
  top it still reuses `inspect_gate` for radar, so one table now truthfully spans both paths.

## J4. Compare modes — partly done

Same modes F6 asks for, applied to model comparison specifically:

- [x] swipe — new this pass, see F6 and the Unreleased CHANGELOG entry: a draggable, pane-local
  A/B split implemented inside one GPU callback, with cursor/probe semantics following the split
- [x] blink — new this pass, see F6 and the Unreleased CHANGELOG entry
- [x] difference — predates this pass (`ModelDiff`)
- [x] transparent overlay — new this pass, see F6 and the Unreleased CHANGELOG entry: model A at
  normal opacity plus model B at 50% in one pane, with per-pane GPU uniforms so the blend cannot
  leak into a side-by-side pane showing B alone
- [x] side-by-side — predates this pass (`PaletteAction::CompareInPanes`)

## J5. Analyst presets — partly done

Ship presets such as:

### Tornado analysis — done

New this pass, see the Unreleased CHANGELOG entry: a "Tornado analysis" starter workspace
(`workspace::starters`) alongside the existing Chase/National overview/Analysis three.

- [x] 0.5 REF
- [x] 0.5 SRV
- [x] 0.5 CC
- [x] 0.5 ZDR
- [x] ProbSevere/storm table visible — the `ProbSevere` overlay toggle is on by default; "storm
  table" reads as the existing storm-cells overlay (`Cells`), also on — there is no separate
  tabular storm-list surface to open alongside it

### Hail analysis — done

New this pass, see the Unreleased CHANGELOG entry.

- [x] REF
- [x] ZDR
- [x] CC/KDP — both get their own pane rather than picking one
- [x] MESH
- [x] sounding panel — `Workspace::sound_center`: applying the preset sounds the point the map
  was centered on just before (the storm being looked at, not a stored point), and the sounding
  opens as a tab in the workstation's right dock. Already-seeded copies of the starter are
  upgraded on load (`workspace::upgrade_starters`) unless the analyst has rebuilt them. Saving a
  workspace while a sounding is open records the same flag.

### Mesoscale analysis — done

New this pass, see the Unreleased CHANGELOG entry: one national-scale pane with GOES IR, CAPE,
SRH and 2 m dewpoint field layers on together — the environment fields that set the stage rather
than one storm's own radar signature. "Surface theta-e" itself isn't a tracked field anywhere in
this app; dewpoint is the roadmap's own listed alternative for that bullet.

### Forecast comparison — partly done

New this pass, see the Unreleased CHANGELOG entry: a two-pane national starter workspace with
cameras and cursor linked. Like the other starters it is seeded on first run only.

- [x] HRRR — the HRRR future-radar layer in one pane
- [ ] RRFS — not a data source this app has; `wxdata::hrrr::Model` only wires up HRRR/RAP/NAM/NAM
  nest, and adding an entirely new model provider is well past what a "ship a preset" pass should
  take on
- [ ] ensemble probability — F7 "Ensemble workstation" is itself not started
- [x] observed/MRMS — the MRMS mosaic in the other pane

Shipping this preset from only the pieces that already exist (HRRR vs. observed/MRMS) would
silently drop RRFS and ensemble probability rather than honestly leave the whole preset undone —
better to wait until F7 and an RRFS source exist and build the real thing.

## J6. Keyboard-first workflows — partly done

Audited against `hotkeys.rs`'s "one flat table" binding system, which already existed and was
more complete than this section's own bare checklist suggested — most of what was actually
missing was a keyboard *door* into an action the command palette (and, for tools, a click) could
already reach, not a new feature. `poll`'s dispatch (a `PaletteAction` binding runs the exact same
code a drawer row or command-palette hit does) means giving one of these a key needed no new
behavior of its own — see the exhaustive-`BindableAction`-match pattern already used for the tilt/
frame-step actions below.

Add shortcuts for:

- [x] tilt next/previous — predates this pass: `PageUp`/`PageDown`
- [x] previous/next frame — predates this pass: `ArrowLeft`/`ArrowRight` (`ArrowUp`/`ArrowDown`
  jump about an hour, a step beyond the letter of "previous/next frame" but the same timeline
  concept)
- [x] live — new this pass: `End` (matches the usual media-timeline convention, `Home` = oldest,
  `End` = most recent, rather than reusing an arrow key, all four of which are already the
  timeline's) → the existing `PaletteAction::GoLive`
- [x] sample tool — new this pass: `G` → `MapTool::GateInspector` ("read the exact gate value and
  geometry" is the closest existing tool to "sample"; `MapTool::Interrogate`, the other click
  tool, is already one tap away from anything else — tapping the active tool again returns to it
  — so it doesn't need a dedicated key of its own)
- [x] cross section — new this pass: `X` → `MapTool::CrossSection`
- [x] sounding — new this pass: `V` → `MapTool::Sounding`
- [x] product next/previous — new this pass: `N`/`P` step through `Moment::ALL` in its own declared
  order, wrapping. `1`-`7` (which predate this) jump straight to a specific moment and stay; this
  is the different thing the line actually asks for, and it is what you want with a hand on the
  mouse rather than on the number row. Cycles in the same order the number keys select in, so
  stepping and jumping can't disagree about what "next" means, and leaves the pane's SRV choice
  alone — that is a way of reading velocity, not a product of its own, so stepping past velocity
  and back must not clear it. Both are plain letter keys on purpose: `steals_typing` only yields
  to a focused text field for a single-character key name, so a punctuation pair would have
  changed the product out from under someone typing a site id (pinned by its own test).
- [x] pane focus — new this pass: `[`/`]` cycle which pane is active
  (`BindableAction::FocusPrevPane`/`FocusNextPane`, wrapping, a no-op with one pane), the same
  `self.active = idx` assignment every existing click-to-focus site already uses. Deliberately not
  `Tab`: egui already owns it for widget-to-widget focus, and a global shortcut consuming it first
  would break the keyboard navigation Q3 already confirmed working — `[`/`]` cycle documents/
  editors already use for a similar "step through the open things" idea, and neither collides
  with the moment-select digits or anything else in the table.
- [ ] link/unlink — genuinely ambiguous, not skipped by oversight: J2 lists *four* independent
  link groups (`LinkCameras`/`LinkTimes`/`LinkSite`/`LinkCursor`), each its own
  `PaletteAction::ToggleOverlay`, and this line doesn't say which one (or whether "all of them at
  once") it means. Guessing wrong here binds a key to the wrong toggle, which is worse than no
  binding at all — better to leave it open for a real product decision than pick one.
- [x] 3D — new this pass: `PaletteAction::ToggleMap3d` plus a `D` shortcut, which also puts the
  3D view in the command palette and the Layers drawer for the first time (it was reachable only
  from the "2D"/"3D map" `selectable_value` pair inside the 3D options panel). The missing work
  really was the factoring this entry predicted, not the key: the camera pitch/bearing reset tied
  to that transition moved into `MapView::set_map_3d`, which both the panel and the action now
  call, so they cannot disagree about the pose each mode rests at. Entering 3D without pitching
  the camera renders as a flat map with the 3D path's cost and reads as a broken toggle, so that
  contract has its own test — as does "re-selecting the current mode leaves a hand-set angle
  alone".
- [x] follow the live sweep — new: `J` toggles `PaletteAction::ToggleFollowSweep` (tilt changes as
  each new sweep starts while live); it and "Follow lowest tilt" are also search/Ctrl+K rows, and
  turning one on turns the other off, matching the toolbar's Follow control and the ribbon pills.
- [x] every shortcut must appear in command palette/help — automatically true for everything
  above: `ui::cheatsheet` and the settings window's rebind editor both read the live binding table
  generically (resolving a `Palette(_)` action's label from the same registry the drawer/palette
  use), so a new binding needs no second edit to show up in either place — confirmed by reading
  both call sites, not assumed from the module doc comment's own claim.

620 hookecho tests passing — the existing coverage test above was extended in place (not a new
test) to also check the two pane-focus bindings, so the six new bindings across this section's
work all confirm they reach the action they name rather than a parallel implementation of it.
Native + wasm32 checks clean.

---

# 13. Phase K — Forecast verification and research/backtesting

**Priority: P1/P2.**

HookEcho already has warning verification. Extend the philosophy to model and algorithm verification.

## K1. Model-vs-observation verification — partly done

At a point or region compare forecasts against:

New this pass, see the Unreleased CHANGELOG entry: `wxdata::gridverify` scores a forecast run against
the RTMA for the same valid hours, and the "Model verification…" window shows it by lead.

- [ ] METAR — not wired; would need matching the forecast at each station point
- [ ] RAOB — not wired
- [x] RTMA/URMA — RTMA only (real-time analysis); URMA is not read
- [ ] MRMS precip/reflectivity where scientifically appropriate — not wired

Metrics:

- [x] bias
- [x] MAE
- [x] RMSE
- [ ] timing error
- [x] categorical hit/miss/false alarm for thresholds — area-weighted, with POD, FAR, CSI and frequency bias

Scope today: HRRR, RAP and the NAMs, for 2 m temperature and dewpoint, over the whole domain or the
map view. It is a region score, not a point series.

## K2. Radar algorithm verification — partly done

TDS and rotation are backtested against LSRs, DAT surveys and observed warnings (see C5). Hail
diagnostics now are too: `--headless-backtest` runs `wxdata::derived::hail` (MEHS/POSH) on every
reflectivity tilt of each archived volume, reduces the grid to discrete cores
(`derived::hail_cores`, confidence = peak POSH), and scores them against severe (≥ ¾ in) hail
LSRs with the same table, range split and missed-report list the two tornado detectors get. The
melting level comes from the observed sounding that day (`wxdata::raob::melting_levels`: nearest
two sites within 400 km, falling back a launch), since HRRR's public archive does not reach most of
the backtest events. First result, Denver 8 May 2017 (KFTG, 10 volumes, melting level from Denver's
own 12Z ascent): POD 88% (22 of 25 reports) at every POSH floor, FAR 85% → 60% and CSI 13% → 51% as
the floor rises from 0 to 80%. Over all nine events in the (since corrected) backtest file: POD 83%
(50 of 60) up to a 40% POSH floor, CSI peaking at 20% at 60%.

The Denver storm also raised 75 debris-signature false alarms: hail near the ground, some of it
over the Front Range foothills 55-70 km west of KFTG. Three candidate discriminators were measured
against real debris balls and none separated them: spectrum width (Mayfield's ball reads 1.9 m/s,
inside the false alarms' range), MEHS over the hit (Moore's ball sits under 20 mm), and the share
of near-zero-velocity gates (the foothill hits read 0% — moving storm echo, not ground clutter,
so a terrain mask would not have helped either). The discriminator that did work is the one in
the operational TDS definition: real balls sit beside a couplet, the Denver hits never did. `tds-6`
discounts a signature with no credible couplet within 5 km when velocity was scanned (x0.8); CSI
went up or stayed at every threshold, FAR at 60% from 69% to 51% at the same POD. Along the way the
investigation found a bigger problem — debris columns chained through fragment fields and placed
kilometres from their tornadoes (fixed in `tds-5`, see C5).

Open from this: Nashville (KOHX, 3 Mar 2020) — the one tornado the discount cost at the top
thresholds. Looked at volume by volume, it is the couplet, not the ball: at 06:36Z the two sit 0.6 km
apart (91%); from 06:43Z the ball keeps tracking the tornado east through East Nashville and
Donelson while the rotation detector's strongest couplet stays 6.5 km behind it to the southwest,
and nothing is found beside the ball. Tried and rejected: comparing velocities across up to 0.5 km
of arc rather than strictly adjacent radials (a close-range tornado spans several beams). It
made the misplaced couplet stronger, found the real one only at 23%, and cost rotation POD at 60-70%
(53% → 47%, 51% → 44%). Next candidates: a lower minimum range with a clutter check, or
associating debris with rotation along the storm's motion rather than by distance alone.

Detector floors now default to 60% (debris) and 50% (rotation), from this backtest; see the
changelog.

Still open: user-defined products, and the SPC tornado database.

Backtest:

- TDS detection
- rotation/couplet detection
- hail diagnostics
- user-defined products

Against:

- LSRs
- DAT surveys
- SPC tornado database where applicable

## K3. Case-study package — done, apart from bundled data files

Allow user to create a portable case manifest containing:

- [x] event name — named for the first pane's radar and the analysis time; free-text notes too
- [x] time range — the active pane's instant and replay window (an hour when it had none)
- [x] radar sites
- [x] enabled products — the whole pane arrangement, as a saved workspace captures it: each
  pane's site, product, tilt, camera and field layers, plus overlays and linking
- [x] annotations — freehand strokes, markers and watch zones
- [x] bookmarks — and user-defined product definitions
- [ ] optional cached public-data files within size limits — not packed: every volume a case
  points at is public and refetched on open, which keeps a case to a few kilobytes

`Save case…` / `Open case…` under Share (`crate::case` for the format, `app/case.rs` for the app
side). A case is versioned JSON (`format: 1`); a newer format is refused with a clear message
rather than half-read. Opening one adds its annotations and bookmarks to the analyst's own,
skipping any already there, rather than replacing them.

## K4. Analyst notebook/export — done

Provide a generated report/export directory containing:

- [x] screenshots — `map.png`, the map as on screen
- [x] CSV probes — `detections.csv` (the active volume's shown debris signatures and couplets, with
  confidence and algorithm version) and `probes/*.csv` for whichever probes are open: region
  statistics, the gate inspector's vertical profile and time series, the cross-section
- [x] GeoJSON annotations — `annotations.geojson` (drawings, markers, watch zones)
- [x] metadata/provenance JSON — `provenance.json`: each pane's radar, product, tilt and elevation,
  the NOAA volume object name and scan time, VCP, the detector algorithm versions and floors, the
  melting level in use and where it came from
- [x] settings/product definitions — the K3 case manifest, `case.hookecho.json`, which carries the
  user-defined products with everything else a case reopens

One ZIP rather than a directory (`Export analysis…` under Share): the same file then works as a
save on desktop, a download in a browser and a share on Android. Written by `zipwrite.rs`, a
deflate ZIP writer on the `flate2` already in the tree (no new dependency), checked against
Python's `zipfile` as an independent reader. A README inside says what each file is.

No need to build a word processor; make HookEcho outputs reproducible in external analysis tools.

---

# 14. Phase L — U.S. route/chase analysis

**Priority: P2 for analyst tool; P1 for chase users.**

HookEcho already has GPS, chase HUD, offline basemap packs and position sharing. Build weather-aware route analysis on top.

## L1. Route engine abstraction

Support pluggable route sources:

- user-configured OSRM/Valhalla endpoint initially
- optional public routing provider only if terms permit
- later offline routing graph included in a chase pack if size/performance is acceptable

Never hard-code dependency on a paid provider.

## L2. Route display

- start / waypoint / destination
- live GPS progress
- ETA
- distance
- alternate routes when provider supplies them

## L3. Weather exposure analysis

Sample along the route against:

- warning polygons
- current radar
- MRMS precip/MESH/FLASH
- lightning
- storm-motion cones
- forecast radar/model fields

Output should say things like:

- “route intersects active tornado warning polygon in 18 mi”
- “forecast path intersects heavy reflectivity between 21:10–21:25Z”

Do **not** label a route safe or guarantee avoidance.

## L4. Storm intercept geometry

For a selected tracked storm and route:

- closest approach
- relative bearing
- projected intersection time
- storm ETA vs vehicle ETA
- escape-direction visualization already present should integrate with route

## L5. Offline chase pack v2

Pack selection UI should optionally include:

- basemap tiles
- radar-site metadata
- recent radar frames
- selected GOES frames
- selected MRMS frames
- routing graph when implemented
- saved placefiles/GIS layers

Show estimated pack size before download.

---

# 15. Phase M — Broadcast, headless and interoperability

**Priority: P1/P2.**

## M1. Broadcast output workspace — done

Built on streamer/OBS mode and on `--watch`: one `broadcast::Broadcast` style dresses both the
app's streaming mode (drawn with egui, `app/chrome/broadcast.rs`, set under Preferences → Display
→ Streaming overlay) and the off-screen frames (`chrome::draw`, `--watch --broadcast` and the flags
below), so a stream and a rendered file look alike.

Presets:

- [x] 1920x1080, 2560x1440, 3840x2160, portrait/social — `--preset 1080p|1440p|4k|portrait|social`
  (M2)
- [x] transparent-background overlay where renderer supports it — `--transparent` (PNG or WebP):
  no basemap, cleared to transparent, the GPU's premultiplied colour undone on read-back, and the
  stamp composited in straight alpha so text keeps its own edges over nothing

Controls:

- [x] safe margins — `--safe-margin PCT` (5 % with `--broadcast`), a share of the frame's shorter
  edge; the clock, caption, bar, crawl and logo keep inside it
- [x] legend visibility — `--no-legend`; the streaming overlay's "Colour scale" switch hides the
  map's own scale
- [x] clock/source stamp — `--clock` (the valid time large in the radar's own zone, site and date
  under it) and the source caption (`--no-caption` to drop it)
- [x] warning crawl optional — `--crawl`: a band along the bottom naming the warnings in force at
  the frame's time that touch the frame, soonest-expiring first. Off-screen it reads the live
  alert feed, so only frames within 30 minutes of the render get one (an archive frame gets none
  rather than today's warnings); in the app it reads the same time-matched warnings the map draws,
  archive included
- [x] logo/branding slot optional — `--logo PATH` / the overlay's logo path: top left, at most 9 %
  of the height

City labels now keep clear of all of it: their places are reserved before any name is placed, which
also stopped plain frames drawing a name under the colour bar. Checked on the Moore case (a
1080p `--broadcast --logo` frame: logo, clock at 3:08 PM CDT, caption and scale inside the 5 %
margin, no crawl on a 2013 frame), on live KABX with a real Flash Flood Warning in the crawl,
on a transparent overlay over a checkerboard, and in the app's streaming mode (F8) with the 2013
warnings in its crawl.

## M2. Deterministic capture — done

- [x] fixed-resolution offscreen rendering independent of current window size — `--watch` renders
  off screen at `--size PX` (square), `--frame WxH` or `--preset 1080p|1440p|4k|portrait|social`,
  up to 4096 px. The renderer draws squares (its projection has no aspect ratio), so a wide or tall
  frame is the middle of a square at its longer edge — same map scale, more map — and the crop
  happens before the caption, colour bar and city labels are stamped, so they sit inside the frame
  (`headless::set_crop`)
- [x] PNG/JPEG/WebP still — by the output's extension (`loopexport::encode_still`; JPEG at
  quality 90 flattened onto the map's black, WebP lossless via the `image` crate's `webp` feature)
- [x] MP4/GIF existing export upgraded to use exact timeline timestamps — the app's loop export
  records each captured frame's volume and scan time, and `--watch --from A --to B --out x.gif|mp4`
  renders one loop of the range; both hold each frame by its real scan gap
  (`loopexport::frame_delays_ms`)
- [x] variable/fixed frame interval option — `--interval real|fixed` and `--fps` (real timing keeps
  the scans' own spacing scaled to average `fps`, clamped 20 ms..4 s so an outage pauses rather
  than stalls; the last frame dwells three times its hold); "Real scan timing" in the app's
  Share preferences. MP4 goes through ffmpeg's concat demuxer with a duration per frame, written
  at a constant 30 fps so every player honours the holds
- [x] metadata sidecar JSON — every still's names its volume, valid time, frame size, zoom and
  centre; every loop's (app and `--watch`) lists each frame's volume, valid time, hold and start
  time, plus the interval mode, fps and total duration

Checked on the Moore case: 1920x1080 WebP and JPEG and a 1080x1920 portrait PNG of 20:08Z with the
caption and colour bar inside the frame, and a four-volume GIF of 19:56-20:16Z with real timing
(scans 4:15 apart held ~250 ms each at 4 fps). MP4 is covered by its frame-list test only: there is
no ffmpeg on the machine this was built on, and the MP4 encode test skips without one.

## M3. Automated output — done

Extend headless mode:

- [x] render named workspace — `--watch --workspace NAME`: the saved workspace's first pane's
  radar, product, tilt, camera centre, zoom and basemap (flags given as well win)
- [x] render selected time/range/site/product — `--site/--product/--tilt`, `--time T` for one
  archived instant, `--from A --to B` for every volume between (one file per volume, named by its
  scan time)
- [x] scheduled repeating snapshot — `--every SECS` (floor 15), `--once`
- [x] update only when source valid time changes — each poll lists the radar's volumes and
  renders only a newer one
- [x] atomic file replace — the PNG and its `.json` sidecar are written beside their targets and
  renamed over them; the sidecar names the site, product, tilt, exact volume and valid time

`hookecho --watch` (`watch.rs`), on the same off-screen renderer as `--serve`, with a new
`headless::set_center` for a saved view's framing. NEXRAD only (the other networks publish no
volume list to poll). Checked on real data: Moore at 20:08Z, a 20:08-20:18Z range centred and
zoomed on Moore (three frames, the debris-ball hook in the second), and a live `--once` on
today's newest volume. Writing it found that listing volumes on a current-thread runtime left the
shared HTTP client's pooled connection undriven for the renderer's own runtime, so every
download failed; `--watch` uses a multi-threaded runtime, as `--serve` does.

## M4. Local API — done

Extend/standardize the existing local serve capabilities with a documented API:

- [x] current view state — `/api/v1/state`: every pane's site, product, tilt, volume and time,
  camera, live or not
- [x] source health — `/api/v1/health`, the diagnostics export's rows
- [x] current warnings — `/api/v1/warnings`, the warnings on the map
- [x] sampled point — `/api/v1/sample?lat=&lon=`, every moment of the displayed tilt there
- [x] available products — `/api/v1/products`: the volume's moments and tilts, the layers on
- [x] latest frame timestamps — `/api/v1/frames`
- [x] snapshot endpoint — `/api/v1/snapshot.png`

- [x] SSE for state/frame-change events — `/api/v1/events`

- [x] localhost only — and off until turned on (Share → Local API)

This is the *running app's* API (`crate::local_api`, `app/local_api.rs`); `--serve` remains the
headless service for a machine with no app open. Documented in `docs/local-api.md`. Loopback only,
with no option to bind elsewhere; requests must name 127.0.0.1/localhost in `Host` (so a
DNS-rebinding page is refused) and no CORS header is ever sent (so another origin's script cannot
read an answer). The server holds no app state: the app publishes a snapshot each second (at once
when the displayed volume changes), and a sample or a screenshot is forwarded to the UI thread
and waited on. Tested over real sockets: snapshot serving, the Host check, no CORS, a forwarded
sample round trip, and SSE delivering a change.

## M5. Scientific export — done

Add where practical:

- [x] GeoTIFF for georeferenced scalar grids — `wxdata::geotiff::write`: one float32 band in the
  field's own units, EPSG:4326, pixel-is-area, NaN declared as nodata, a description naming the
  product, source and valid time. "Export grid (GeoTIFF)…" under Share writes the active pane's
  top gridded layer (MRMS, a derived radar field such as VIL or MEHS, a model field); the analysis
  export includes it as `grid.tif`; `--headless-mrms out.tif` writes the latest MRMS reflectivity.
  Checked with the `tiff` crate as an independent reader in the tests, and on a real 7000 x 3500
  CONUS mosaic parsed back in plain Python (same valid count and maximum as the app reported).
  Writing it found a half-cell georeferencing error in every MRMS layer — see the changelog.
- [x] NetCDF for gridded fields — `wxdata::netcdf::write`: the classic format (CDF-1) with CF-1.8
  metadata, written by hand from the published layout: a scalar `time`, `lat`/`lon` coordinate
  variables at cell centres, the field as `float32(lat, lon)` with units where known and NaN for
  missing. "Export grid (NetCDF)…" under Share (the same grid the GeoTIFF export writes) and
  `--headless-mrms out.nc`. Checked with an independent reader written from the spec in Python
  against a real CONUS mosaic: every variable at its offset, the data ending at end of file, the
  valid time, count and maximum as the app reported, and the cell centres exactly on MRMS's own
  grid points (54.995 N / 129.995 W) — which also confirms the half-cell fix below round-trips.
- [x] CF/Radial-compatible export for selected radar data — `wxdata::cfradial::write`: CF/Radial
  1.4 in NetCDF classic (the NetCDF writer was generalised into `netcdf::NcFile` to carry it):
  `time`/`range`/`sweep` dimensions, per-ray time, azimuth and elevation, per-sweep number, mode,
  fixed angle and first/last ray, the radar's location and altitude, and every moment as a
  `(time, range)` byte field with `scale_factor`/`add_offset`/`_FillValue`. It is the app's
  **binned** volume — fixed azimuth bins and gates, 8 bits per field, exactly what the displays
  and detectors used — not the raw Level II words; bytes carry those binned values without loss.
  "Export volume (CF/Radial)…" under Share and `--headless-cfradial SITE DATE HH:MM out.nc`.
  Checked with the independent Python spec reader on Moore (KTLX 20:08Z): 14 sweeps at VCP 12's
  angles, 10 080 rays in contiguous sweep blocks, ray times spanning the volume's four minutes,
  and, decoded at the debris ball, max DBZ 69.4 with RHOHV down to 0.21 — what the debris
  detector reported for it. Not yet opened in Py-ART or Radx themselves (neither is installed
  here).
- [x] CSV for probes/profiles/tables — region statistics, vertical profile, point series,
  cross-section, detections, cells, sounding, verification
- [x] GeoJSON for vectors/tracks/polygons — map export (I6) and the analysis export's annotations
- [x] JSON metadata/provenance — the analysis export's `provenance.json` (K4)

---

# 16. Phase N — Data reliability and source observability

**Priority: P0 across all phases.**

Top-tier operational software needs to make feed quality visible.

## N1. Data Source Health panel — partly done, found already built

`app::SourceHealth`/`HealthState` and the per-lane `RequestBook` already tracked almost everything
below, one source at a time, surfaced as a hover popup on that source's own row in the Layers
panel (Phase B3's latency dashboard). What was actually missing was a single consolidated view —
"for every active source" meant hunting row by row rather than one screen. New this pass, see the
Unreleased CHANGELOG entry: a "Data source health…" window (`ui::source_health_window`, opened
from the command palette/Layers panel like any other tool) lists every currently-active,
health-tracked source worst-first, reusing the exact `SourceHealth` data and `active_layer` filter
the per-row popups already use — the two views can never disagree about what counts as "active" or
what a source's status is, because there is only one health computation feeding both.
The WSV3/Dock app-bar health readout now opens this window directly on click.

For every active source:

- [x] provider — `SourceHealth.source`
- [x] endpoint family — new this pass, see the Unreleased CHANGELOG entry: every `SourceHealth`
  now carries a typed `EndpointFamily` separately from its layer-specific `source` label. Field
  layers inherit NOAA MRMS / NOAA-NCEP / global-model identity from the generic field registry;
  non-grid request lanes explicitly identify NWS API, NOAA map services, IEM,
  AviationWeather.gov, radar-product, community, partner, multi-provider, local and
  user-configured failure domains. The per-layer popup and consolidated health window show the
  family, while diagnostics exports its stable machine ID (for example `noaa-mrms`) so shared
  upstream outages can be grouped without parsing presentation text.
- [x] last successful request — `SourceHealth.last_success` (an age, not a timestamp — see
  "latest valid data time" below for why the two are kept separate)
- [x] latest valid data time — new this pass, see the Unreleased CHANGELOG entry:
  `RequestStatus` retains the newest authoritative valid/observation time carried by a successful
  payload, independently of its local fetch-completion clock. A dedicated `app::overlay_health`
  extractor covers gridded MRMS/model/GOES products, comparisons, surface analysis, radar cells,
  mPING/PIREP/recon/spotter/station observations, PPEF, damage surveys, archived-warning buckets
  and model contours; older archive/forecast selections cannot move the newest-known time
  backward, and a failed refresh preserves it. Radar uses the timeline's newest frame through the
  same `SourceHealth.latest_valid_time` field. Both health views show an absolute UTC time plus
  relative age (or forecast lead), diagnostics export RFC 3339, and feeds whose decoded payload
  genuinely carries no representative timestamp say `not reported` rather than substituting the
  HTTP completion time or an alert expiry.
- [x] age — `last_success`/`last_attempt`/`last_failure`, all ages from "now"
- [x] expected cadence — `SourceHealth.cadence`
- [x] rolling success/failure count — new this pass, see the Unreleased CHANGELOG entry:
  `RequestStatus.outcomes`, a capped `VecDeque<bool>` (last 20 finished requests) that
  `SourceHealth.recent_outcomes` reports as `(successes, failures)`. `None` for radar, whose
  health is built from `MapView` fields directly rather than through `RequestBook` and has no
  outcome history to report honestly. Shown in both the per-row popup ("Recent: 18/20 succeeded")
  and the consolidated Data source health window (a new "Recent" column), and carried into the N4
  diagnostics bundle.
- [x] current backoff — `SourceHealth::next_retry()`
- [x] cache state — new this pass, see the Unreleased CHANGELOG entry: `SourceHealth.cache_state`
  explicitly distinguishes `In memory` from `Not cached`; successful delivery establishes
  residency, the renderer's existing five-minute field-texture eviction clears it, failed
  refreshes preserve the prior value, and radar derives residency directly from whether its pane
  holds a decoded volume. Both health views expose it and diagnostics exports the stable
  `memory`/`empty` ID. This deliberately does not infer browser, operating-system or intermediary
  HTTP-cache contents that HookEcho cannot prove.
- [x] fallback provider — new this pass, see the Unreleased CHANGELOG entry: the earlier audit
  became stale when B6.11 wired three real Level II tiers into each native radar pane. Every
  `SourceHealth` now carries a structured `fallback_providers` list (empty means this source has
  no runtime alternative); radar populates it from the same atomic `FailoverSnapshot` used for
  active-provider/failover-state detail, listing every configured non-active tier in preference
  order. That means primary shows the optional HookEcho Relay plus always-available TGFTP,
  relay-active shows Unidata plus TGFTP, and degraded TGFTP shows the progressive recovery
  candidates. Both health views and diagnostics JSON expose the list. Web radar honestly remains
  empty because its native-only provider manager is not wired there; single-provider non-radar
  feeds do too.

Status states:

- [x] Live — `HealthState::Fresh`
- [x] Delayed — new this pass: `HealthState::Delayed`, for a source between 1x and 2x its own
  expected cadence past its last success — past due, but not yet the "genuinely stopped updating"
  `Stale` state a single slow poll or a naturally jittery feed (one that lands "every ~5 minutes,"
  give or take) used to jump straight to. Sits between `Fresh` and `Stale` in the Data source
  health window's worst-first ordering and gets its own paler amber (distinct from `Stale`'s more
  saturated one) in the Layers panel's per-row popup — both already-generic (`health_look`/
  `severity_rank`), so this needed no new UI code, only the new state and where it sorts/colors.
  The 2x multiplier is a judgment call, not a per-source-family tuned value — see N2's own note
  about *data* staleness (how old is too old for this kind of thing) being a different, still-open
  question from this section's *fetch* staleness (are we still successfully polling).
- [x] Stale — `HealthState::Stale`
- [x] Failed — `HealthState::Failed`
- [x] Cached — new this pass: `HealthState::Cached` means the newest refresh failed while an
  explicitly resident prior value remains available. It sorts immediately below terminal
  `Failed`, has its own degraded-orange treatment, and automatically becomes `Failed` if that
  value is evicted. A prior success timestamp alone is no longer enough to claim fallback data.
- [ ] Experimental — no source in this app is marked experimental yet (see F2's Tier 2 targets,
  which do ask for an explicit EXPERIMENTAL label on future AI-guidance products)

## N2. Stale-data policy — partly done

Define per source family.

Examples:

- [x] radar: prominently stale after expected scan cadence threshold — predates this pass:
  `RADAR_FRESH_SECS` (15 min) drives both the scrubber's Live/Stale badge and radar's own
  `SourceHealth` cadence, so the two can't disagree.
- [x] METAR: normal hourly cadence, do not mark stale after 10 minutes — new this pass, see the
  Unreleased CHANGELOG entry: the station-card header used one 5-minute-amber threshold for every
  network, which read every completely normal 20-, 30-, 45-minute-old METAR as stale. Each
  `wxdata::stations::Network` now has its own threshold (METAR 75 min, matching its hourly
  cadence with slack for a late post; the near-real-time PWS/mesonet networks keep short
  thresholds appropriate to their own reporting rate).
- [x] model: show run age rather than simplistic stale flag — new this pass: the source/provenance
  inspector's "Run"/"Issue" rows now show a computed age (`3h 08m old`) alongside the absolute
  timestamp, not the timestamp alone. This is a data-inspector improvement, not a "flag" per se —
  there was no simplistic stale flag on a model run to begin with, only the absolute time; this
  closes the actual gap (the time reader had to do their own mental subtraction against whatever
  time it is right now).

Note found while researching this item: this app already conflates two different meanings of
"stale" — *fetch* staleness (`SourceHealth`/`HealthState`, "are we still polling this feed
successfully") and *data* staleness (this section's actual subject, "is the observation/run
itself old for its own kind"). `RequestLane::cadence()` is the former; nothing before this pass
implemented the latter as a first-class per-family concept. Not unified into one shared policy
struct here — the three concrete gaps above were fixable as targeted, independent fixes, and
inventing a generic "staleness policy" abstraction with only three call sites to serve would be
speculative generality this codebase's own engineering rules argue against.

## N3. Provider contract tests — mostly done

Create network tests that run on schedule, not every PR, for public feeds:

- [x] latest Level II chunk listing — `hookecho::volume::tests::
  latest_complete_volume_finds_a_new_one_then_reports_up_to_date`
- [x] latest MRMS file — `wxdata::mrms::catalog::tests::the_mrms_catalog_paths_are_real`
- [x] GOES ABI object listing — `wxdata::goes_abi::tests::fetches_the_live_conus_*` (IR/visible/
  water vapor)
- [x] HRRR/RAP/GFS byte-range/index access — `wxdata::hrrr::tests::*` and
  `wxdata::model::tests::the_catalogue_matches_what_the_feeds_publish`
- [ ] RRFS/REFS feed — not a data source this app has yet (F2's own not-started item); nothing to
  contract-test
- [ ] RTMA/URMA feed — likewise not built yet (G1)

**What was actually missing, and what shipped this pass:** every test above already existed as
an `#[ignore = "network"]` unit test — the literal gap was that `cargo test --workspace` (CI's own
default command) never runs `#[ignore]`d tests at all, so nothing was watching any of these
between the times someone happened to run `-- --ignored` by hand. New this pass:
`.github/workflows/provider-contracts.yml`, a scheduled workflow (`workflow_dispatch` too) that
runs every ignored test in both `wxdata` and `hookecho` except ones that genuinely cannot run
unattended on a bare CI runner (GPU golden-image tests, two audio-device-dependent tests, one
that needs a manually-supplied camera URL, and two that need personal API tokens this job has no
secret for) — adds no new tests, only the schedule + alerting `cargo test`'s own default behavior
already gives a failing scheduled run (the Actions tab, the repo's notification settings).

Verified by actually running the exact commands the workflow uses, repeatedly, before trusting
it — not written and assumed clean:
- Found and fixed a real problem this way: with cargo's default parallelism, several tests hit
  the *same* provider at once (three separate tests all fetch GDPS, for instance) and can race or
  trip a rate limit, failing for a reason that has nothing to do with a genuine schema/path break.
  `--test-threads=1` fixes it — a scheduled contract job has no reason to race for speed, and a
  job that cries wolf from its own concurrency defeats its own purpose.
- Two remaining, accepted sources of occasional real red runs even sequential, documented in the
  workflow's own comments rather than hidden: `dwd`'s conditional-poll test can race a genuine
  live volume rollover, and any single test can hit a transient connection reset unrelated to an
  actual break. Both reproduced during this verification; neither is a bug in this app or this
  workflow. A red run naming only one of these two is worth a rerun before treating it as a real
  break — the actual signal this job exists to surface is several unrelated failures at once, or
  the same test failing across separate days.

## N4. Local diagnostics bundle — done

New this pass, see the Unreleased CHANGELOG entry: an "Export diagnostics…" button in Settings →
Backup, beside the existing settings-bundle export, saving through the same cross-platform
`dialog::save_bytes` (native save dialog, Android SAF, browser download) rather than a new
mechanism. Every field below already existed somewhere in the app (Phase B3's health tracking,
`wxdata::stats`'s performance counters, the devlog capture buffer) — this pass wired them together
into one exportable JSON rather than inventing new instrumentation, plus the one field with no
existing source at all (on-disk cache size).

User can export a diagnostics text/JSON file containing:

- [x] app version — `ui::about_window::VERSION`
- [x] platform — `std::env::consts::OS`
- [x] renderer/backend — the GPU adapter name/device-type/backend already logged once at
  startup, now also kept on `HookEchoApp` rather than only scrolling past in the log
- [x] source health — reuses `ui::source_health_window::active_health_rows`, the exact same
  function and "active" definition N1's health window lists, so the two can never disagree
- [x] recent errors — new `devlog::recent_warnings`, a *non-destructive* read of the capture
  buffer (WARN/ERROR only, most recent 200) — deliberately not `drain`, which the devlog shipper
  depends on to hand a batch off exactly once; a one-off diagnostics export must not silently
  steal entries the shipper still needs to send
- [x] cache sizes — new `paths::cache_dir_bytes`, an iterative walk of the on-disk cache root
  (tiles, vector tiles, climatology CSV); no such number existed for the native build before
  (the browser build already had one, `webcache::known_auto_cache`, for its own IndexedDB store)
- [x] performance counters — `wxdata::stats::snapshot`, the same counters the dev-only Perf
  window reads; gained a wasm stub (empty, like every other function in that module on wasm)
  since nothing had ever called it from a cross-platform path before

No location history, API keys or private tokens in the bundle — the same discipline `crash.rs`'s
own panic report already commits to. Verified with a full test suite covering the two genuinely
new pieces (`recent_warnings`'s non-destructive read, `cache_dir_bytes`'s directory walk against a
real temporary directory) rather than a screenshot of the export button itself.

---

# 17. Phase O — Performance and memory engineering

**Priority: continuous.**

## O1. Establish budgets

Create benchmark targets for representative hardware.

### Desktop

- smooth pan/zoom at 60 fps for normal radar + overlays
- no UI-thread decode stalls > 16 ms for steady-state streaming
- 3D interactive target 60 fps default quality, degrade gracefully

### Web

- startup bundle budget remains enforced
- first useful radar frame benchmark
- Web Worker decode benchmark
- IndexedDB/OPFS cache benchmark

### Android

- memory budget by device class
- adaptive GPU texture sizes
- battery-aware animation/particle quality

## O2. GPU grid tiling

Some MRMS/model products exceed common texture dimensions. Move from whole-grid assumptions toward tiled/virtualized textures where needed.

Requirements:

- visible-region upload
- correct sampling at tile boundaries
- no max-pooling for fields where extrema preservation would distort meaning
- field-specific downsample operator: nearest / mean / max / vector-safe

## O3. Decode and regrid caching

Cache separately:

1. compressed bytes
2. decoded native grid
3. regridded display representation
4. GPU texture

Invalidate only the layers actually affected by a settings change.

## O4. Profiling

Expand existing profiling/perf infrastructure:

- fetch
- decompression
- decode
- regrid
- derived calculation
- upload
- draw

Display optional developer overlay.

---

# 18. Phase P — Plugin/extension architecture

**Priority: P2, but design hooks earlier.**

HookEcho already has an external-process plugin runner. Turn it into a documented analyst extension path without compromising reliability.

## P1. Stable plugin manifest

Plugin declares:

- name/version
- API version
- capabilities
- input products
- output products
- config schema

## P2. Safe product plugin path

Allow plugin to consume exported grid/radar samples and return:

- scalar grid
- vector features
- annotations
- table

Prefer an IPC protocol or sandboxed WASM over loading arbitrary dynamic libraries into the process.

## P3. Python interoperability

Do not embed Python as a hard dependency.

Instead provide:

- JSON/NDJSON IPC
- NetCDF/GeoTIFF/CSV exports
- documented local API

This lets researchers use Python/MetPy/Py-ART externally while HookEcho remains Rust-native.

---

# 19. Phase Q — UI/platform polish

**Priority: P1/P2.**

U.S.-scope does not mean desktop-only.

## Q1. Android tablet layout — decided: the desktop layout, 1:1

Current phone chrome should not simply stretch.

**Decision (owner, after trying the phone build):** a tablet gets the desktop layout as-is rather
than a third, tablet-specific one. `platform::form_factor` reads the window's shortest side every
frame and calls it a tablet at Material 3's 600 dp line — the shortest side, so a phone turned
sideways stays a phone and a tablet held upright stays a tablet, and a tablet in a narrow
split-screen window drops to the phone chrome while it is that narrow. `phone_layout()` replaces
the bare `cfg!(target_os = "android")` at every site that chose a *layout* (the chrome, the WSV3
ribbon, the legend and colour bar, the drawer, full-screen surfaces, popovers, station cards);
sites about the platform itself — the soft keyboard, the file picker, no ffmpeg, background
alerts — still ask the platform. System Back still closes the window on top on a tablet.

This supersedes the sub-items below, which assumed a separate tablet design. Not verified on a
real tablet yet, and desktop chrome has hover-only affordances and small targets that a finger
will find harder than a mouse; those are what to look for.

- ~~two-column controls on large screens~~ — the desktop layout already has them
- ~~persistent layer/product panel option~~ — the desktop docked panel
- multi-pane optimized touch targets — open: desktop targets are mouse-sized
- keyboard/mouse support on tablets — the desktop chrome's own
- drag/drop pane layout where practical — the desktop's

## Q2. Desktop analyst density

Add compact mode:

- denser product tables
- dockable optional analyst panels while preserving current full-map default
- high-information status footer option

Progress: the Dock/WSV3 workstation (`docs/WSV3_IMGUI_MODERN_DESIGN_PLAN.md` §13) is the compact
mode. Its Layers panel's Active filter is now also the per-layer manager — remove on every active
layer, an opacity fader on every field layer — so fading MRMS under radar no longer needs the
separate Layer Manager window.

## Q3. Accessibility — ongoing, swept this pass

Preserve current accesskit/high-contrast work and ensure new controls have:

- [x] semantic names — `ui::a11y::Named`'s `.named()`/`.named_toggle()` already covers most of the
  icon-only chrome (21 call sites before this pass). New this pass, see the Unreleased CHANGELOG
  entry: a two-pass audit turned up 19 icon-only buttons across 10 files that had fallen through
  that sweep — some with a tooltip but no accessible name (a bare `.on_hover_text` doing only half
  the job `Named` does), several with neither. The first pass searched for
  `small_button`/`Button::new` wrapping a bare Phosphor glyph or a raw ✕/✖/🗑 symbol directly and
  found 15; the second widened that to also catch the `Button::new(RichText::new(icon)…)` form
  (an icon with its own `.size()`/`.color()`), which the first missed entirely, and found 4 more —
  including a "choose radar site" button on both the desktop and mobile ribbons. Fixed all 19;
  verified by confirming every remaining hit of both search shapes already chains `.named()`. This
  is a sweep, not a guarantee — a *new* icon-only control added without this pass's checklist in
  mind will quietly reintroduce the gap; nothing here enforces it at compile time.
- [x] keyboard access — inherited for free from egui's own focus/tab order and Enter/Space
  activation on every `ui.button`/`ui.small_button` in this app; nothing found bypassing it
- [ ] non-color-only state indication — spot-checked, not swept: this pass's own new coverage
  overlay (`coverage_compare`) and the pre-existing model-diff overlay (`fielddiff`) are both
  genuinely color-only on the map itself (a text legend states what the colors mean, but the
  per-pixel data has no second channel) — accepted as a property of this class of diverging-color
  data visualization, not audited for whether a discrete status *control* elsewhere shares the gap.
  The WSV3/Dock Layers tree now distinguishes all seven source-health states with different
  shapes and accessible names instead of color-only dots. The workstation's own status marks
  were then audited as a set: rail and segmented selections are filled boxes (not a color
  change alone), Sources rows share the tree's state glyphs, background dock-tab dots are said
  in words in their hover and accessible name, the app bar's feed dot sits beside its own text,
  the live tilt is a ring (and an arc for progress) rather than a colored dot, and the one mark
  that relied on color alone, the timeline's forecast-hour ticks, is now split at the rail. The
  floating chrome and ribbon have not had this pass.
- [x]/[ ] scalable text — investigated this pass: `egui::Context::set_zoom_factor` (which
  `Settings.ui_scale` already drives, `app.rs:18673`, via a Settings slider and Ctrl+=/Ctrl+-/
  Ctrl+0) computes `pixels_per_point = zoom_factor * native_pixels_per_point` — confirmed from
  egui's own source, not assumed: this multiplies *every* rendered size, including font metrics,
  not just spacing, so a literal `.size(14.0)` call scales along with everything else despite not
  "knowing" about any scale preference itself. `native_pixels_per_point` is also already set
  automatically from the OS's own display-scale factor by the windowing backend (eframe/winit),
  with no HookEcho code needed for that half. **Real, narrower gap not covered**: this is the
  OS's general *display* scale, not a finer-grained "make text bigger, leave everything else
  alone" preference some platforms expose separately (Windows' own "Make text bigger" slider is
  distinct from its display-scale setting) — nothing in this app reads that narrower preference
  specifically, though the general zoom mechanism already serves the same practical need.

## Q4. macOS

Keep experimental support healthy if CI permits, but it is not allowed to delay core U.S. analyst phases.

## Q5. iOS

Still out of scope for this roadmap unless project constraints change. Do not divert core analyst engineering into iOS before P0/P1 analysis goals are complete.

---

# 20. Additional U.S. situational-awareness layers

**Priority: P2 after the analysis core.**

Add only when reliable machine-readable sources exist.

Candidates:

- CPC temperature/precipitation outlooks
- CPC drought/outlook products
- additional WPC precipitation discussions/products
- NDFD/NBM forecast grids
- WFO/CWA boundaries and office information
- county/parish/zone boundary inspector
- TFRs if current aviation layer does not already cover required operational detail
- river/flood enhancements from NWPS
- public power-outage datasets where licensing/feed stability permits
- U.S. road/weather feeds through provider adapters, not 50 one-off UI implementations

Each must use the generic product/provenance system.

---

# 21. Top-tier “analyst extras” beyond competitor parity

These are not required for initial parity, but are high-value differentiators.

## R1. Multi-radar storm volume fusion

Research-grade feature.

For storms seen by overlapping WSR-88Ds:

- align volumes by valid time
- transform samples into common Cartesian volume
- retain contributing-radar provenance
- blend reflectivity using distance/beam-height-aware weighting

Do **not** create a “velocity mosaic” by naïvely combining radial velocities. If multi-Doppler wind retrieval is attempted, it must use proper geometry and be labeled experimental/research.

## R2. Dual-Doppler wind synthesis — experimental

Long-term research item only.

Requirements before implementation:

- overlapping radars
- synchronized time tolerance
- quality control
- solved horizontal wind components from radial velocities
- uncertainty/residual field
- clear invalid-geometry masking

This must never be implemented as simple pixel averaging.

## R3. Object-based storm history

Create persistent storm objects that combine:

- SCIT/ProbSevere objects
- radar-derived rotation/TDS/hail metrics
- MRMS severe fields
- warnings
- LSRs
- DAT surveys

Expose a storm history timeline and exportable JSON/CSV.

## R4. Feature tracking

Track analyst-selected radar features through time using optical flow/object matching:

- mesocyclone marker
- hail core
- reflectivity core
- bounded weak echo region candidate
- user-selected contour

Always show confidence and allow manual correction.

## R5. Uncertainty everywhere

Where products support it, visualize:

- ensemble spread
- timing spread
- radar beam sampling limitations
- source age
- quality flags

A top-tier analyst workstation should show uncertainty instead of hiding it.

---

# 22. Competitive parity checklist — U.S. only

This is the explicit “what are we still missing?” list for agents.

## WeatherFront-class gaps

- [ ] native full-resolution GOES ABI
- [ ] 1-minute mesoscale satellite
- [ ] broad RGB/channel suite
- [ ] generic 80+-class MRMS catalog coverage
- [ ] RRFS/REFS
- [ ] GEFS / ensemble probabilities
- [ ] NBM
- [ ] RTMA/URMA
- [ ] richer model/run comparison
- [ ] route planning
- [x] AWIPS-style pane layouts — J1 now supports persisted equal grids and adaptive large-primary
  focus layouts for every supported multi-pane count
- [ ] WFO boundary/contact tooling

## RadarOmega-class gaps

- [ ] 3D for more than radar reflectivity
- [ ] 3D MRMS/model-derived surfaces where scientifically valid
- [ ] broader model catalog
- [ ] long high-frame-count satellite/model/MRMS loops efficiently cached
- [ ] broader supplemental U.S. outlook/situational layers

## WeatherWise-class gaps

- [x] progressive in-progress sweep display — per-chunk radial merge/GPU updates, retained previous
  pass shading, scan progress and the data-triggered 2D live sweep bar are all implemented in B2
- [ ] measured ultra-low-latency pipeline where provider permits
- [ ] explicit beam-rise visualization
- [ ] more polished 3D cross-section workflow
- [ ] impact/analysis report workflow

## WSV3-class gaps

- [x] in-progress LiveScan-style rendering — B2 now paints every arriving partial chunk and runs a
  bounded lime sweep through the refreshed sector of the currently viewed 2D tilt
- [x] precise delay indicator — the WSV3/Dock app bar now counts from the newest known radar
  frame to the current clock while following live, updating every second. Archive view says
  `Archive` rather than presenting the age of historical data as live feed delay. The provider's
  lag at receipt remains available in the hover detail and source-health panel.
- [x] scan-age visualization — `wxdata::scan_age` reads the per-azimuth collection times every
  binned sweep already carries, and the "Scan-age ring" layer draws them as a green-to-red ring at
  the sweep edge, labelled with the sweep's time span. Ages are relative to the sweep's own newest
  data, not the wall clock. Tested; not yet exercised on screen.
- [x] Shapefile GIS import — see I1 item 2
- [ ] stronger broadcast output/capture workflows
- [ ] multi-provider operational redundancy

## GR2Analyst-class gaps

- [x] user-defined radar product system — formula evaluation + a live gate-side readout (see C1);
  rendering a product as its own map layer is not built
- [ ] maximum/minimum value trails
- [ ] mature transfer-function 3D
- [ ] isosurfaces
- [x] movable slicing planes / clip slabs — see H4: an arbitrary-bearing vertical plane plus the
  pre-existing axis-aligned box; a horizontal in-view CAPPI plane and a map-pane cross-section
  line remain unbuilt
- [x] deeper radar metadata/quality inspection — B4's gate inspector

## RadarScope-class operational gaps

- [ ] broader provider failover/redundancy
- [ ] more source-health transparency
- [ ] tighter Spotter Network reporting integration if public/authenticated API terms permit
- [ ] mature tablet experience

---

# 23. Recommended implementation sequence

Do not attempt all phases in parallel. Use this dependency order.

## Milestone 1 — Foundation

1. Phase A1 generic field registry
2. Phase A2 time/provenance model
3. Phase A3 persistent web cache
4. source health primitives from Phase N
5. begin shrinking `app.rs`

**Release goal:** no major visible feature required; architecture release.

## Milestone 2 — Operational radar

1. Level II provider abstraction
2. progressive radial rendering
3. latency/scan progress UI
4. VCP/SAILS awareness
5. provider failover
6. radar gate inspector

**Release goal:** HookEcho becomes a genuinely low-latency operational radar client.

## Milestone 3 — Satellite + MRMS breadth

Run in parallel after registry is stable:

- native GOES ABI
- generic MRMS catalog

**Release goal:** radar/MRMS/satellite can be compared on one synchronized timeline.

## Milestone 4 — Model workstation

1. generic model engine
2. HRRR/RAP migration
3. RRFS/REFS adapter
4. GFS expansion
5. GEFS
6. NBM
7. ensemble tools
8. run/model comparison

**Release goal:** no need to leave HookEcho for routine deterministic/ensemble mesoscale comparison.

## Milestone 5 — Analyst radar depth

1. user-defined product DSL
2. max-value trails
3. beam/blockage analysis
4. multi-moment stats/scatterplots
5. algorithm laboratory/backtesting

**Release goal:** compete directly with dedicated radar-analysis software rather than only weather apps.

## Milestone 6 — 3D + GIS + workspace

1. advanced 3D
2. GIS import/export
3. AWIPS layouts and linked probes
4. analyst presets

**Release goal:** professional workstation workflow.

## Milestone 7 — Verification / chase / broadcast

1. RTMA/URMA and objective analysis
2. model verification
3. route exposure tools
4. broadcast workspace
5. scientific export/local API

**Release goal:** field, research, EOC and broadcast usability.

## Milestone 8 — Research differentiators

- multi-radar volume fusion
- experimental dual-Doppler
- persistent storm objects
- advanced feature tracking

---

# 24. Suggested module/file map

This is guidance, not a rigid requirement. Reuse existing modules where they already own the concept.

## `crates/wxdata`

Suggested additions/refactors:

```text
src/
  field/
    mod.rs
    descriptor.rs
    units.rs
    grid.rs
    provenance.rs
  model/
    mod.rs
    catalog.rs
    grib.rs
    hrrr.rs
    rap.rs
    gfs.rs
    rrfs.rs
    gefs.rs
    nbm.rs
  satellite/
    mod.rs
    goes.rs
    abi.rs
    projection.rs
    rgb.rs
  mrms/
    mod.rs
    catalog.rs
    fetch.rs
  radar_analysis/
    mod.rs
    expression.rs
    user_product.rs
    trails.rs
    beam.rs
  analysis/
    rtma.rs
    urma.rs
    objective.rs
```

Do not move everything just to match this tree. Migrate when touching the relevant subsystem.

## `crates/hookecho`

Suggested ownership:

```text
src/
  app/
    data_controller.rs
    pane_state.rs
    layer_state.rs
    task_state.rs
  ui/
    product_browser.rs
    source_health.rs
    data_inspector.rs
    model_browser.rs
    ensemble.rs
    satellite.rs
    gis.rs
    route.rs
    analyst_workspace.rs
  render/
    field.rs
    satellite.rs
    gis.rs
    comparison.rs
    volume3d.rs
```

Existing files that should be extended/refactored rather than duplicated:

- `timeline.rs`
- `workspace.rs`
- `fielddiff.rs`
- `render3d.rs`
- `headless.rs`
- `serve.rs`
- `webcache.rs`
- `plugins.rs`
- `perf.rs`
- `profiling.rs`
- `elevation.rs`
- `chase.rs`
- `gps.rs`

---

# 25. Test strategy

## Unit tests

Required for:

- product metadata validation
- unit conversion
- time alignment
- GRIB field mapping
- GOES projection
- RGB transforms
- MRMS path mapping
- user-product parser/type checker
- beam geometry
- ensemble statistics
- GIS CRS transforms

## Golden-data tests

Maintain small legally redistributable or generated fixtures.

Suggested historical cases:

- 2013-05-20 KTLX Moore tornado
- 2013-05-31 KTLX El Reno
- 2011-04-27 KBMX severe convection
- 2021-12-11 Mayfield
- 2022-09-28 Hurricane Ian landfall

Use fixtures to test algorithms, not just screenshots.

## Screenshot regression

Add deterministic captures for:

- radar product
- cross section
- 3D
- GOES RGB
- MRMS categorical field
- model contours/barbs
- ensemble postage stamps
- AWIPS four-pane layout
- GIS overlay

Allow small pixel tolerance for GPU backend differences.

## Performance regression

Bench:

- Level II decode
- progressive radial update
- 3D volume build
- MRMS decode
- GOES frame decode/projection
- GRIB byte-range field fetch/decode
- ensemble probability calculation

## Network contract tests

Scheduled workflow only. Never make normal unit tests depend on live NOAA services.

---

# 26. Definition of done for every new data source

A source is not “done” until all apply:

- [ ] documented public endpoint/provider
- [ ] licensing/terms compatible
- [ ] fetch timeout/retry
- [ ] decoder validation
- [ ] source health status
- [ ] valid/issue/run/received timestamps
- [ ] cache behavior
- [ ] stale-data behavior
- [ ] units
- [ ] legend
- [ ] sampling
- [ ] timeline integration
- [ ] error UI
- [ ] at least one test fixture
- [ ] `docs/DATA.md` updated

# 27. Definition of done for every analysis algorithm

- [ ] scientific reference documented in code/docs
- [ ] inputs and assumptions documented
- [ ] units tested
- [ ] missing-data behavior tested
- [ ] thresholds configurable if they are heuristic
- [ ] result marked derived
- [ ] historical case test
- [ ] exportable values
- [ ] no wording that implies an official NWS warning/determination

---

# 28. What not to do

- Do not spend this roadmap on international radar networks.
- Do not add dozens of products through copy/paste booleans and UI branches.
- Do not silently interpolate categorical data.
- Do not interpolate model data across incompatible vertical coordinates without explicit conversion.
- Do not call forecast reflectivity “radar.”
- Do not call heuristic detections official tornado warnings.
- Do not create a velocity mosaic by nearest-radar compositing.
- Do not promise sub-10-second latency when the active feed cannot deliver it.
- Do not add mandatory HookEcho accounts/backend infrastructure.
- Do not block the UI thread on network/decode/regrid tasks.
- Do not make every new feature live in `app.rs`.
- Do not let visual smoothing modify sampled/raw values without labeling the display transform.
- Do not hide source age or stale data.

---

# 29. Immediate next tasks for Claude/Codex

If starting from current `main`, execute in this order:

1. **Audit the existing field-layer abstractions** in `products.rs`, `fielddiff.rs`, `overlay_build.rs`, `timeline.rs`, `workspace.rs`, `wxdata::mrms`, `wxdata::hrrr`, and `wxdata::global`.
2. **Write an architecture note** in `docs/field-registry.md` defining the generic field descriptor, provenance object, and renderer contract. Keep it small and implementation-focused.
3. **Implement `DataStamp`/provenance first** and migrate one existing MRMS field end-to-end.
4. **Implement the field registry** and migrate the rest of the currently supported MRMS fields without changing visible behavior.
5. **Refactor the layer browser** to generate MRMS rows from the registry.
6. **Add source-health state** to the migrated MRMS path.
7. **Add browser persistent cache** for the migrated path and radar cache.
8. After those land cleanly, begin **Phase B progressive Level II** and **Phase E GOES ABI** as separate workstreams.
9. Do not begin the huge model catalog until the generic field/time/provenance plumbing has proven itself on MRMS + GOES.

Each major implementation should be its own PR/commit series with tests and should update the checkboxes in this file only after acceptance criteria pass.

---

# 30. Final target scorecard

HookEcho should be considered “top-tier U.S. analyst workstation” only when the following are true:

- [x] progressive in-progress Level II display with measured latency — B2 renders every incoming
  chunk with generation shading/progress/live sweep; B3 reports provider, receipt and decode/GPU lag
- [ ] automatic feed fallback and source-health display
- [x] exact radar gate/beam/VCP inspection — B4's gate inspector reports the sampled gate,
  geometry, value and sweep time; B5 exposes decoded VCP strategy and repeated low-level cuts
- [ ] broad metadata-driven MRMS catalog
- [ ] native GOES ABI + 1-minute mesoscale imagery
- [ ] radar/satellite/MRMS valid-time synchronization
- [ ] RRFS/REFS + GEFS + NBM + expanded HRRR/RAP/GFS
- [ ] ensemble probabilities/postage stamps/plumes
- [ ] RTMA/URMA surface analysis
- [ ] model/run/observed comparison workflows
- [ ] advanced soundings/hodographs
- [ ] user-defined radar products
- [ ] temporal max/min trails
- [ ] advanced 3D transfer functions + isosurfaces + slicing planes
- [ ] beam-rise/blockage/coverage analysis
- [ ] GeoJSON/Shapefile/KML GIS import
- [x] AWIPS-style linked multi-pane layouts — J1's equal/focus arrangements persist with J2's
  camera, time, site and cursor links; the focus geometry supports all 3–9 pane workspaces
- [x] synchronized multi-pane crosshair/probe — J3 links one geographic cursor across panes and
  samples radar plus resident MRMS/model/GOES/NDFD grids into one time/value table
- [ ] historical algorithm and model verification
- [ ] chase route weather-exposure analysis
- [ ] broadcast/headless deterministic rendering
- [ ] GeoTIFF/NetCDF/CSV/GeoJSON scientific export where applicable
- [ ] browser persistent cache
- [ ] Android tablet analyst layout
- [ ] performance regression suite
- [ ] scheduled public-feed contract tests
- [ ] all major layers expose provenance, exact time, units, age and sampling

When these are complete, further work should focus less on copying competitor checklists and more on **scientific quality, uncertainty visualization, reproducibility, performance, and novel multi-source analysis**.

---

## Reference targets used for this roadmap

Current public feature sets consulted in September 2026:

- RadarOmega — https://www.radaromega.com/
- WeatherWise — https://www.weatherwise.app/
- WeatherFront — https://www.weatherfront.com/
- WSV3 — https://wsv3.com/
- GRLevelX / GR2Analyst — https://www.grlevelx.com/
- NOAA RRFS — https://gsl.noaa.gov/rrfs/
- NOAA RRFS v1 evaluation — https://www.emc.ncep.noaa.gov/users/meg/rrfsv1/

These are feature targets, not implementation dependencies. Prefer NOAA/NWS/public machine-readable sources whenever practical.
