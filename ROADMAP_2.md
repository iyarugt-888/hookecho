# ROADMAP_2.md — Professional Maturity / WSV3-Competitive Roadmap

## Purpose

This roadmap is focused on closing the remaining gap between HookEcho and a mature professional weather-analysis workstation such as WSV3.

The priority is **not adding more disconnected weather layers**. HookEcho already has broad feature coverage. The remaining work is primarily:

- making live Level II behavior operationally trustworthy under severe-weather load,
- making storm-analysis interactions as fast and polished as the underlying algorithms,
- strengthening GIS and broadcast workflows,
- improving rendering consistency and presentation quality,
- reducing architectural coupling,
- adding long-duration and corpus-based regression testing,
- and defining objective release gates for professional use.

The target is a workstation that can be used continuously during a high-impact event without the operator needing to think about data freshness, rendering edge cases, hidden source failures, synchronization mistakes, or UI instability.

---

# 0. Program goals and release gates

## 0.1 Product goals

HookEcho should be able to support all of these use cases without changing applications:

1. **Live severe-weather monitoring**
   - near-live Level II radar with explicit freshness and acquisition state,
   - rapid tilt/product switching,
   - reliable warnings, SCIT, ProbSevere, lightning, MRMS, satellite, and model context,
   - storm-relative and derived analysis without disruptive loading.

2. **Professional storm analysis**
   - multi-pane synchronized views,
   - cross-sections and 3D volume analysis,
   - storm tracking and ETA workflows,
   - rapid inspection of data values and provenance.

3. **Historical forensic analysis**
   - consistent archive behavior,
   - warning/report synchronization,
   - reproducible derived fields,
   - event verification,
   - exports suitable for review or documentation.

4. **Operational GIS / broadcast presentation**
   - custom vector data,
   - reusable scenes/workspaces,
   - clean external output,
   - stable capture,
   - professional labels and transitions.

5. **Cross-platform use**
   - Windows as the primary professional desktop target,
   - Linux/macOS parity where feasible,
   - browser/WASM as a first-class analysis surface,
   - Android as a field/chase companion with shared core behavior.

## 0.2 Release gates

Before declaring the application "professional-station ready", all of the following should be true:

- [ ] 12-hour live severe-weather soak test passes with no unrecovered feed, renderer, memory, or UI failure.
- [ ] 24-hour idle/live mixed soak test passes on Windows.
- [ ] no stale radar scan can be displayed as current without a visible stale-data indication.
- [ ] all displayed forecast/derived/observed layers expose provenance through a common inspector.
- [ ] live Level II partial-volume behavior is covered by deterministic tests.
- [ ] VCP transitions, SAILS/MESO-SAILS, missing chunk, late chunk, duplicate chunk, and out-of-order chunk cases are tested.
- [ ] all critical storm-analysis actions are reachable by command palette and direct pointer interaction.
- [ ] archive replay is deterministic across repeated runs for the same volume/time.
- [ ] export/capture output is frame-stable and does not depend on UI repaint timing.
- [ ] no single application source module remains responsible for an unreasonable number of unrelated domains.
- [ ] nightly/corpus regression suite runs against known historic storm cases.
- [ ] performance budgets are defined and measured for desktop, browser, and Android.

---

# 1. P0 — LiveScan-class Level II acquisition and presentation

This is the highest-priority maturity gap.

HookEcho already has chunk-level Level II ingestion, chunk timing models, retry logic, elevation mapping, live polling, and partial-volume assembly. The next step is to make the **in-progress radar volume itself a first-class state machine**.

## 1.1 Introduce an explicit live-scan state model

Create a dedicated live radar acquisition model with states such as:

- AwaitingVolume
- AcquiringVolume
- AcquiringTilt
- SweepPartial
- SweepComplete
- VolumeComplete
- Recovering
- FallbackSource
- Stale
- Offline

Track:

- radar site,
- VCP,
- volume number / source identifier,
- current elevation cut,
- split-cut identity,
- sweep start time,
- newest radial time,
- newest chunk receive time,
- provider/source,
- estimated beam age,
- volume completeness,
- missing radial/chunk ranges,
- source failover status,
- clock skew.

### Acceptance criteria

- [ ] UI never needs to infer live state from a generic loading boolean.
- [ ] current sweep and completed sweep are distinguishable in the state model.
- [ ] SAILS/MESO-SAILS supplemental scans are represented explicitly.
- [ ] out-of-order chunks cannot regress displayed time.
- [ ] stale live data is visibly marked before it reaches a configurable threshold.

## 1.2 Partial-volume rendering rules

Define deterministic rules for what is shown while a volume is in progress.

Requirements:

- render newly arriving radials immediately,
- preserve previous-volume data only where no replacement has arrived,
- visually distinguish old vs newly scanned regions when enabled,
- prevent mixed-time products from being silently presented as one instant,
- support a "strict current sweep only" mode,
- support a "continuous composite" mode,
- expose scan age in tenths of a second where the source supports it.

### Tests

Build synthetic fixtures for:

- complete sweep,
- radial gap,
- late radial,
- repeated radial,
- reordered chunks,
- volume reset,
- VCP change mid-session,
- SAILS insertion,
- source restart.

## 1.3 Multi-source Level II failover

Formalize provider priority and failover.

Possible source classes:

- primary low-latency chunk source,
- alternate chunk source where available,
- completed-volume source,
- archive fallback.

The UI should show:

- current provider,
- source age,
- failover reason,
- whether displayed data is partial or complete,
- whether the fallback changes temporal resolution.

### Acceptance criteria

- [ ] provider loss does not freeze the display indefinitely.
- [ ] fallback never silently changes from partial-scan live data to minutes-old completed data.
- [ ] recovery to the preferred source does not cause time reversal.
- [ ] all provider switches are logged in Analyst Mode.

## 1.4 VCP-aware scan progression

Strengthen VCP handling for:

- split cuts,
- repeated elevation angles,
- supplemental low-level scans,
- unusual maintenance/test sequences,
- dynamic VCP changes.

Add a scan progression panel in Analyst Mode showing:

- VCP,
- expected cuts,
- observed cuts,
- cut completion,
- expected next cut,
- elapsed and projected time.

## 1.5 Performance budget

Targets for desktop:

- radial ingestion to visible GPU update: p50 < 100 ms, p95 < 250 ms after receipt,
- interaction must remain responsive while decoding,
- no full-volume GPU re-upload when a subresource update is sufficient,
- no avoidable reallocations per chunk.

Browser and Android receive separate budgets.

---

# 2. P0 — Storm tracking and manual analysis UX

HookEcho already has SCIT tracks, warning motion, ProbSevere, rotation/TDS analysis, projected paths, and chase calculations. The missing maturity is interaction speed and editability.

## 2.1 Unified storm object model

Create one internal storm entity that can associate:

- SCIT cell,
- ProbSevere object,
- warning object,
- TDS detection,
- rotation/couplet detection,
- hail attributes,
- manual motion vector,
- automatic motion vector,
- user annotations,
- historical track,
- projected track.

Do not force every source to merge. Preserve provenance and confidence for each association.

## 2.2 Manual motion tool

Implement a direct-manipulation storm-motion tool:

- click storm origin,
- drag motion vector,
- editable speed and bearing,
- 15/30/45/60 minute markers,
- configurable interval,
- projected swath,
- left/right uncertainty width,
- optional expanding uncertainty cone.

Keyboard modifiers should allow:

- constrain bearing,
- change speed,
- adjust cone width,
- duplicate a track,
- clear the track.

## 2.3 Line / squall-line tracking

Support:

- polyline storm edge,
- projected future line,
- forward swath,
- individual city/marker impact times,
- minimum/maximum arrival range.

Useful for QLCS and long convective lines where one-point SCIT tracking is insufficient.

## 2.4 Impact / ETA engine

For any tracked object, compute impact against:

- saved markers,
- manually clicked points,
- cities/communities,
- roads/interstates if available,
- user-defined event zones.

Show:

- ETA,
- distance,
- closest approach,
- projected uncertainty,
- source of motion estimate.

Do not imply precision beyond the motion model.

## 2.5 Storm selection UX

Selecting a storm should optionally open a compact analysis card containing:

- current product sample,
- SCIT identifiers,
- max reflectivity,
- VIL / VILD,
- echo top,
- hail metrics,
- ProbSevere,
- rotation/TDS flags,
- warning association,
- motion,
- arrival times,
- source timestamps.

## 2.6 Acceptance criteria

- [ ] a trained user can create a storm motion vector and 60-minute projection in under 5 seconds.
- [ ] manual and automatic motion can coexist and be clearly distinguished.
- [ ] editing never requires opening Settings.
- [ ] storm interactions work with mouse, pen, and touch.
- [ ] projected ETAs are recomputed live while handles are dragged.

---

# 3. P0 — Long-duration resilience and source-failure testing

Professional maturity requires proving that the application behaves well after hours of operation.

## 3.1 Soak-test harness

Create a repeatable soak runner capable of:

- launching HookEcho headlessly or semi-headlessly,
- selecting a site/workspace,
- rotating products/tilts,
- enabling common layers,
- collecting memory/GPU/resource stats,
- injecting source failures,
- recording state transitions,
- failing on unrecovered feed or render stalls.

Profiles:

- 2-hour developer smoke soak,
- 12-hour severe-weather soak,
- 24-hour stability soak.

## 3.2 Failure injection

Support deterministic simulation of:

- HTTP timeout,
- truncated response,
- invalid gzip,
- invalid GRIB,
- malformed Level II chunk,
- missing chunk,
- delayed chunk,
- stale warning feed,
- GOES listing failure,
- MRMS archive miss,
- model index failure,
- DNS/network offline,
- WebGPU/device loss where practical.

## 3.3 Memory/resource leak detection

Track over time:

- process RSS,
- heap allocations where available,
- GPU buffer/texture counts,
- cached radar volumes,
- object cache growth,
- number of pending async tasks,
- open sockets/HTTP requests,
- UI window/card count.

Set acceptable growth thresholds.

## 3.4 Recovery rules

Every feed should declare:

- retry policy,
- backoff,
- stale threshold,
- last-good retention policy,
- whether stale data remains visible,
- when to hide data,
- user-facing error severity.

---

# 4. P1 — Rendering and visual polish

The objective is to make the application feel like one product rather than many technically correct layers.

## 4.1 Unified render-quality policy

Define per-platform quality tiers:

- Low
- Balanced
- High
- Analysis / Maximum

Each tier should control:

- radar antialiasing,
- 3D step budget,
- contour density,
- label density,
- line smoothing,
- particle count,
- satellite texture resolution,
- shadow/atmosphere effects if present.

Avoid independent hidden quality knobs scattered across features.

## 4.2 Temporal transitions

Implement consistent transition policy for:

- new radar sweep,
- volume change,
- MRMS frame,
- satellite frame,
- model lead,
- warning issuance/cancellation.

Provide options:

- instant,
- short crossfade,
- radar-specific progressive replacement.

Never interpolate categorical fields unless scientifically valid.

## 4.3 Label engine improvements

Build or strengthen one label-placement system shared by:

- cities,
- roads,
- storms,
- warnings,
- SCIT labels,
- user markers,
- fronts/highs/lows,
- placefile text,
- GIS labels.

Priorities:

- no important storm label hidden by a lower-priority city,
- stable placement between frames,
- density-aware decluttering,
- collision groups,
- leader lines when needed.

## 4.4 Camera behavior

Polish:

- zoom interpolation,
- fly-to easing,
- storm-follow mode,
- warning-follow mode,
- linked-pane camera behavior,
- 3D orbit controls,
- map/3D transition.

Avoid camera motion that changes analytical interpretation while scrubbing.

## 4.5 Basemap robustness

Address incomplete 3D basemap loading and zoom-out holes.

Requirements:

- visible tile coverage diagnostics in Analyst Mode,
- parent-tile fallback,
- retry failed tiles,
- no transparent holes when lower-resolution fallback exists,
- bounded retry behavior.

---

# 5. P1 — General GIS engine

WSV3 remains substantially stronger in generic GIS workflows. HookEcho should grow from weather overlays into a real operational GIS surface.

## 5.1 Supported formats

Phase 1:

- GeoJSON
- Shapefile
- KML/KMZ

Phase 2:

- GPX
- CSV with lat/lon mapping
- optional WFS/ArcGIS FeatureServer ingestion

## 5.2 Vector style model

Per layer:

- fill color,
- fill opacity,
- stroke color,
- stroke width,
- dash pattern,
- point icon,
- point radius,
- z-order,
- min/max zoom,
- visibility by attribute filter.

Allow rules based on attributes, for example:

- `TYPE == "TOR"`
- `MAG >= 2`
- categorical color map.

## 5.3 Label editor

Allow labels from attributes with:

- field selection,
- template syntax,
- font size,
- halo,
- priority,
- min/max zoom,
- collision participation.

## 5.4 Attribute inspector

Click any feature to inspect:

- geometry,
- properties,
- source,
- layer name.

Allow copying values and exporting filtered features.

## 5.5 Layer groups and saved GIS scenes

Users should be able to save:

- multiple GIS layers,
- styles,
- ordering,
- visibility,
- basemap,
- camera,
- weather layers,

as a named operational scene/workspace.

---

# 6. P1 — Broadcast / external output system

This should be treated as a distinct product surface rather than extending screenshot export.

## 6.1 Dedicated output window

Add an optional clean output surface separate from operator controls.

Capabilities:

- arbitrary resolution,
- 16:9 presets,
- 1080p / 1440p / 4K,
- borderless/fullscreen,
- monitor selection,
- independent UI visibility.

## 6.2 Deterministic frame clock

Exports and external output should use a deterministic render clock.

Requirements:

- capture does not depend on UI event-loop timing,
- animation advances on explicit timestamps,
- fixed-FPS export,
- radar/model frames selected deterministically.

## 6.3 Scene presets

Save scene state:

- map camera,
- visible layers,
- legend state,
- title,
- logo,
- time display,
- annotation visibility,
- output resolution.

Add hotkeys to switch scenes.

## 6.4 Graphics overlays

Support operator-configurable:

- title strap,
- timestamp,
- source attribution,
- logo,
- warning banner,
- storm name,
- custom text.

## 6.5 OBS integration

At minimum:

- clean window capture,
- transparent-background overlay mode where technically feasible,
- stable window identity/title,
- optional NDI/Spout/Syphon exploration after core output is stable.

## 6.6 Export

Strengthen:

- screenshot,
- GIF,
- MP4/H.264 or platform-available codec,
- frame sequence.

Acceptance:

- [ ] no dropped or duplicated logical weather frames in fixed-FPS export.
- [ ] exported timestamp matches the rendered analysis time.
- [ ] output can run for 30 minutes without drift.

---

# 7. P1 — app.rs decomposition and application architecture

The current central application file is too large and increases regression coupling.

This refactor should be incremental and behavior-preserving.

## 7.1 Define domain boundaries

Move state and behavior into modules/controllers for:

- radar session,
- timeline/session time,
- pane layout,
- layer state,
- storm analysis,
- alerts,
- archive/event playback,
- models,
- satellite,
- GIS,
- 3D,
- chase/GPS,
- export/broadcast,
- settings,
- persistence,
- command/action registry.

## 7.2 App shell responsibility

The top-level app should primarily:

- own global services,
- route events,
- manage top-level lifecycle,
- coordinate domain controllers,
- call rendering/UI surfaces.

It should not directly implement every feature.

## 7.3 Event/message model

Introduce explicit typed events for cross-domain communication rather than ad hoc shared-state mutation.

Examples:

- AnalysisTimeChanged
- RadarFrameArrived
- RadarSourceChanged
- StormSelected
- WarningSelected
- WorkspaceLoaded
- GpuDeviceLost
- CachePressure
- NetworkStateChanged

## 7.4 Refactor guardrails

For every extraction:

- add tests first where practical,
- preserve serialized settings compatibility,
- preserve command IDs,
- preserve workspace formats,
- avoid simultaneous behavior redesign unless required.

## 7.5 Metrics

Track:

- size of top-level app module,
- number of direct dependencies,
- number of mutable global fields,
- compile time impact,
- regression count.

Target: no single file should remain a de facto monolith for unrelated product domains.

---

# 8. P1 — Product-level historical regression corpus

HookEcho's archive capability provides an unusual advantage: major past events can become deterministic integration tests.

## 8.1 Build a canonical event corpus

Include representative cases such as:

- 2011 Tuscaloosa / Birmingham,
- 2011 Joplin,
- 2013 Moore,
- 2013 El Reno,
- 2021 Mayfield,
- 2022 Hurricane Ian,
- one winter storm,
- one tropical non-landfall case,
- one widespread QLCS,
- one low-topped rotation case,
- one clear-air / anomalous propagation volume.

Use legally/publicly available archive references rather than committing huge raw files where avoidable.

## 8.2 Golden scientific checks

For known scan/time points validate:

- tilt inventory,
- radial/gate geometry,
- selected sample values,
- dealiased velocity expectations,
- VIL,
- echo top,
- MESH/POSH,
- TDS flag,
- azimuthal shear extrema,
- cross-section values,
- 3D volume bounds.

Use tolerances where algorithms are continuous.

## 8.3 Golden visual checks

Generate deterministic reference renders for:

- reflectivity,
- velocity,
- CC,
- four-pane layout,
- cross-section,
- 3D observed mode,
- 3D smooth mode,
- warnings over radar,
- storm tracks.

Use perceptual/image-diff thresholds rather than requiring byte-identical GPU output.

## 8.4 Historical warning verification checks

For selected events ensure:

- archived warnings load at the correct instant,
- issue/expire transitions are correct,
- LSR matching is stable,
- verification metrics remain unchanged unless intentionally modified.

---

# 9. P1 — Data/provenance completeness

HookEcho already has a strong provenance system. Finish applying it consistently.

## 9.1 Every field gets a DataStamp-equivalent

Required:

- source/provider,
- product,
- valid time,
- issue/run time where applicable,
- receive time where known,
- observed / forecast / derived classification,
- units,
- quality / missing-data behavior,
- original/native units if transformed.

## 9.2 Source health panel

In Analyst Mode provide a compact status page for:

- Level II,
- Level III,
- warnings,
- MRMS,
- GOES,
- models,
- METAR,
- ProbSevere,
- auxiliary feeds.

For each:

- last successful fetch,
- last error,
- response age,
- retry state,
- cache hit/miss,
- provider.

## 9.3 Stale-data policy

Use consistent visual semantics:

- fresh,
- aging,
- stale,
- unavailable.

Avoid each layer inventing its own warning style.

---

# 10. P1 — Timeline and synchronization finish

The time-alignment engine is already one of HookEcho's strengths. The next objective is uniformity.

## 10.1 One analysis cursor

All panes/layers that opt into linked analysis should resolve from the same retained analysis time.

## 10.2 Explicit source offsets

Show signed source-minus-analysis-time offsets where useful.

For example:

- Radar +00:00
- GOES -00:42
- MRMS +01:15
- HRRR valid +00:00, run 18Z

## 10.3 Archive/model synchronization

Finish archive-seeking behavior for model fields where data access makes it practical.

Do not substitute current model data while viewing a historical radar event.

## 10.4 Timeline themes

Support visual timeline styles inspired by:

- HookEcho native,
- WSV3-like,
- GRLevel-like,
- compact mobile.

These should be visual skins on one timeline engine, not separate logic implementations.

---

# 11. P2 — 3D analysis expansion

The new 3D system is already technically strong. Further work should focus on analysis, clarity, and performance.

## 11.1 Storm-centered 3D mode

Add a mode that centers volume bounds around a selected storm rather than the whole radar.

Benefits:

- higher effective voxel resolution,
- faster raymarch,
- easier vertical analysis.

## 11.2 Isosurfaces

Extend:

- configurable dBZ thresholds,
- multiple shells,
- velocity pair thresholds,
- MESH/hail surfaces where scientifically meaningful,
- opacity presets,
- clipping planes.

## 11.3 Vertical reference surfaces

Continue integrating environmental context:

- 0 C,
- -10 C,
- -20 C,
- optional tropopause / EL where available,
- terrain surface.

Clearly label source/run/time.

## 11.4 3D crosshair / ruler

Allow measurement of:

- AGL/MSL height,
- horizontal range,
- slant range,
- approximate beam height,
- value at sample.

## 11.5 3D temporal playback

Ensure stable camera and deterministic geometry while stepping through volumes.

Optional future feature:

- storm-following 3D camera that translates with selected cell motion.

---

# 12. P2 — Workspace and analyst workflow improvements

## 12.1 Professional workspace presets

Ship curated presets such as:

### Tornado analysis
- Z / SRV / CC / ZDR
- linked camera/time
- SCIT + ProbSevere + warnings

### Hail analysis
- Z / ZDR / CC / MESH
- freezing-level references

### National severe
- MRMS
- warnings
- SPC outlooks
- GOES/GLM

### Chase
- reflectivity + SRV
- GPS
- warning motion
- saved markers

### Tropical
- radar
- NHC track/wind field
- satellite
- surface obs

## 12.2 Workspace validation

On load:

- report unavailable data sources,
- map missing radar sites gracefully,
- preserve unknown future fields,
- avoid silently dropping configuration.

---

# 13. P2 — Input, accessibility, and operator efficiency

## 13.1 Command consistency

Every significant action should have:

- stable command ID,
- command-palette entry,
- optional hotkey,
- discoverable UI location.

## 13.2 Pointer consistency

Normalize:

- click,
- double-click,
- right-click,
- long-press,
- drag,
- multi-touch gestures.

Prevent map gestures from conflicting with edit handles.

## 13.3 Touchscreen stability

Specifically fix multi-finger map cases where the camera can "fly away".

Add gesture-state tests for:

- pinch + rotate,
- second pointer added mid-drag,
- pointer cancel,
- browser touch emulation,
- Android native events.

## 13.4 Accessibility

Improve:

- keyboard-only navigation,
- focus order,
- scalable text,
- high-contrast mode,
- colorblind-safe warning indicators,
- non-color-only state indication.

---

# 14. P2 — Performance and observability

## 14.1 Internal telemetry without user tracking

Analyst Mode should expose local-only metrics such as:

- frame time,
- radar decode time,
- GPU upload time,
- cache hit rate,
- HTTP latency,
- chunk receive lag,
- sweep completion lag,
- memory,
- object counts,
- dropped frame count.

No external telemetry is required.

## 14.2 Performance regression tests

Automate benchmarks for:

- Level II decode,
- partial sweep append,
- full volume upload,
- 4-pane rendering,
- MRMS field upload,
- GOES load/decode,
- 3D grid build,
- cross-section generation.

Store benchmark history in CI artifacts.

---

# 15. P2 — Documentation and operational transparency

## 15.1 "How current is this?" documentation

Document expected cadence/latency for every major feed and distinguish:

- provider latency,
- network latency,
- decode latency,
- display latency.

## 15.2 Scientific limitations

For derived products document:

- assumptions,
- thresholds,
- environmental dependencies,
- known failure modes,
- whether values reproduce official Level III algorithms exactly or approximately.

## 15.3 Analyst troubleshooting guide

Add scenario-based troubleshooting:

- radar stopped updating,
- one tilt missing,
- velocity looks folded,
- satellite older than radar,
- model time mismatch,
- warning polygon not appearing,
- 3D missing part of storm,
- basemap tiles missing.

---

# 16. Execution sequence

Recommended implementation order:

## Phase 1 — Operational radar trust
1. live radar state model,
2. partial-volume rules,
3. source failover,
4. VCP/SAILS handling,
5. live latency UI,
6. chunk/scan regression tests.

## Phase 2 — Severe-weather operator workflow
1. unified storm object,
2. manual motion tool,
3. line tracking,
4. ETA/impact engine,
5. storm analysis card,
6. interaction optimization.

## Phase 3 — Reliability
1. soak runner,
2. failure injection,
3. resource metrics,
4. recovery policies,
5. nightly long-duration CI where feasible.

## Phase 4 — Architecture
1. define app domain boundaries,
2. extract radar/timeline/layer controllers,
3. extract storm/alert/GIS/export domains,
4. formalize typed events,
5. reduce app.rs responsibility.

## Phase 5 — Visual maturity
1. shared render-quality policy,
2. label placement,
3. temporal transitions,
4. camera polish,
5. basemap fallback.

## Phase 6 — Professional workstation expansion
1. GIS ingestion/style system,
2. dedicated broadcast output,
3. deterministic capture,
4. scene presets,
5. OBS-oriented workflows.

## Phase 7 — Regression corpus
1. canonical event catalog,
2. scientific goldens,
3. visual goldens,
4. warning verification goldens,
5. CI integration.

## Phase 8 — 3D / analyst refinements
1. storm-centered volume,
2. clipping/measurement,
3. environment surfaces,
4. temporal 3D playback,
5. advanced analysis presets.

---

# 17. Definition of "WSV3-competitive"

HookEcho should be considered operationally competitive when all of the following are true:

- live Level II can be watched sweep-by-sweep for hours without temporal ambiguity,
- source latency/failover is always visible and recoverable,
- manual storm tracking is as fast as automatic analysis,
- map/labels/camera behavior remains stable during continuous timeline playback,
- GIS files can be loaded and styled without writing code,
- a clean external output can be driven separately from the operator UI,
- historical cases can be reproduced deterministically,
- regression tests protect the radar science and visual output,
- the application survives long severe-weather sessions without resource degradation,
- adding a feature no longer requires expanding a monolithic top-level application module.

The remaining competitive gap should then be mostly a matter of accumulated field use and incremental polish rather than fundamental capability.

---

# 18. Work that should *not* be prioritized yet

Until the P0/P1 maturity work is complete, avoid spending major effort on:

- adding many more niche weather layers,
- adding another UI theme engine,
- adding decorative 3D effects without analytical value,
- supporting additional export codecs before deterministic capture is correct,
- introducing cloud accounts/backends that are not required for reliability,
- expanding platform count beyond currently supported targets,
- adding AI-generated meteorological conclusions into the core analysis workflow.

The objective of ROADMAP_2 is **depth, reliability, operator speed, and professional finish**.
