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

## Implementation progress (2026-09-29)

Phase 1 has started with a per-pane live Level II acquisition state (`crates/hookecho/src/live_scan.rs`). It tracks the current provider, chunk/cut progress, accepted volume identifier and time, recovery, fallback, aging, and staleness. The top-bar radar status now exposes that state; queued updates from superseded streams and older volume/chunk updates are rejected. Deterministic unit tests cover sweep completion, supplemental cut positions, recovery, polling fallback, and time reversal.

The partial-sweep stitcher now keeps the newest radial at each azimuth even when a delayed chunk arrives later. Repeated and older chunks no longer produce a false changed-sweep update. Synthetic tests cover reordered chunks, gap filling, retention of the previous pass, and the stale-sector mask passed to rendering.

The direct 2D live-tilt display now offers continuous composite (the previous-pass sector remains dimmed) and strict current sweep (older-pass rows are hidden). The mode is saved in settings and can be switched from the radar ribbon. Strict mode uses nearest sampling to prevent smoothing from bleeding current values into masked older azimuths. Synthetic tests cover old-row masking, including separated older sectors, and settings compatibility. Derived multi-tilt products and 3D still need their own temporal policy.

The relay failover path now stamps live updates with the newest radar acquisition time instead of the client's wall clock. The ingest service uses the source message timestamp for VCP/pass-through blocks, so a delayed VCP retains the same volume identity as its radials. The HTTP and WebSocket integration tests exercise a scan received two minutes after acquisition; the 54 radar-ingest unit tests pass.

Both progressive Level II providers now attach source volume start, VCP number, and standard/SAILS/MRLE/MPDA cut kind to progress events. The per-pane state inventories observed chunks for each volume and cut, reports unobserved chunks in the top-bar details, and waits for every chunk before declaring a sweep complete. Late gap fills do not move the sweep marker backward; delayed progress from an older volume is rejected across reconnects. Focused tests cover gap filling, new volumes, supplemental cuts, and reconnects.

Analyst Mode now has a scan progression section backed by retained cut-position coverage for the current volume. It shows VCP, observed and complete cut counts, the expected next cut, elapsed time from the source volume start, and a rough remaining-time estimate at the current cut's rate. Individual cut badges distinguish unobserved, partial, and complete cuts. Delayed chunks from an earlier cut can fill its inventory without moving the current cut marker backward; a VCP change resets the cut inventory. The state declares a volume complete only when every expected cut is complete, including an earlier cut with a delayed gap fill.

Provider switches now emit a reasoned Analyst Mode log entry when the app starts a different live subscription or falls back to completed-volume polling after stream loss. The per-pane feed state and Analyst header expose the last switch reason and whether the selected source delivers progressive radials or completed volumes only. Clearing a manual provider override records the resulting automatic tier selection. This makes a TGFTP downgrade's temporal-resolution change visible alongside the provider name.

Progressive updates now carry a client-side monotonic transport receipt timestamp through decode and UI delivery to the 2D radar GPU queue write. Per-pane/source queue timings retain 128 rendered samples; Analyst Mode shows p50/p95 and sample count, and the radar health panel shows the latest duration. This measures receipt to completed queue writes, including decode and UI wait. GPU execution and actual on-screen presentation still need instrumentation before the desktop p50/p95 release budget can be certified. TGFTP completed-volume updates have no comparable pre-decode receipt timestamp and are excluded from this distribution.

The remaining Phase 1 work includes radial gap inventories, GPU presentation latency measurement, and prolonged source-failure/soak verification. Chunk coverage here reflects chunks observed by the current session; a mid-volume join may show earlier chunks as unobserved even if the assembled scan backfilled their radials. The release gates below remain open until those behaviors are verified end to end.

Radial gaps inside received chunks now read plainly — "8 radials missing (#1–4, #10, …)" — in a Gaps row of Analyst Mode's scan progression and in the live-scan tooltip, instead of a raw span list.

Phase 2 has started with the manual storm-motion tool (§2.2, `crates/hookecho/src/app/storm_track.rs`, `MapTool::StormTrack`). Dragging from a storm to where it will be in an hour draws the track under the pointer: +15/+30/+45/+60 marks with clock times from the analysis time it was placed at, a swath whose half-width grows by an uncertainty cone, and the ETA and closest approach (side and distance) at every saved marker, recomputed while the handle moves (§2.4 for markers). Either end drags to edit; Shift snaps the heading to 5°; Delete removes, Ctrl+D duplicates, `[` and `]` narrow and widen the cone; a card edits speed, heading, width and cone by number, so nothing needs Settings. Manual tracks are magenta and tagged MANUAL, apart from the automatic SCIT motion. ETAs are rounded to the minute and marked approximate; points behind the storm or more than two hours ahead get none. Tracks are session-only. The Cell window's **Track manually** seeds a track from the SCIT cell's automatic motion, to adjust by hand beside the SCIT track.

Line tracking (§2.3): with the tool armed, clicks on open map lay out a storm edge (dashed; Backspace undoes a point) and the next drag, from anywhere, is the whole line's motion. The line is drawn now and every 15 minutes with the swept area filled, tagged MANUAL LINE; a marker's ETA is when the moving line crosses it ("line arrives"), and past either end it is read from the nearer end's widening swath. The origin handle moves the line, edge and all.

Impact (§2.4): the card also gives, for the selected track, when the storm (or any part of a line) first enters each saved watch zone within two hours, "inside now" when it already is, and when only the uncertainty swath's edge first touches a zone, marked apart from a real entry. **People in path** asks the 2020 Census (the lookup alert cards use) for the population, homes and largest towns inside the hour's footprint, on request only. The Census places carry no coordinates, so per-town ETAs, and roads, are not yet done; touch/pen verification is also open.

Failure injection (§3.2) has a deterministic suite, `crates/wxdata/tests/failure_injection.rs`, run by every `cargo test`: error pages and empty bodies to every decoder, a real GRIB2 message and ODIM files cut at every point, a GRIB header claiming more than it holds, damaged gzip streams, malformed Archive II volumes and cut-off live-chunk framing. Every case must be an error (or, for an HDF5 tail the decoder never reads, exactly the whole volume), and none may take more than 5 s. It found the Level II decoder reading a JSON error body, a zero-filled buffer or a bare header as a volume with no sweeps; such a volume is now an error, so a failed fetch is retried instead of being shown or cached as an empty radar. Still open in §3: DNS/offline, stale warning feed, GOES listing, MRMS archive and model-index failures, which need injection at the HTTP layer, and device loss.

Workspaces (§12): a **Tropical** starter (reflectivity, storm-relative velocity and reflectivity over infrared satellite from the active radar, linked, with the NHC track and cone, recon, surface obs, alerts and watches) joins the existing Tornado, Hail, National, Chase and analysis presets. Starters used to be seeded once, on first run, so a preset shipped later never reached anyone; each starter a settings file has never been offered is now added once (`Settings::offered_starters`), and one someone deleted stays deleted. Applying a workspace names what this build cannot restore (unknown layers, fields, radar sites, map styles, too many panes) in the error chip and the log instead of skipping it silently, and `Workspace`/`PaneSnap` keep fields a newer build wrote, so opening and saving here does not drop them.

Timeline (§10): the layer probe's field and model-contour lines now give each source's signed offset from the time it is read against, with the reference named so the sign means something ("(Δ-42s vs radar)", "(Δ+1m 15s vs analysis)"): the linked analysis time when panes share one, else the pane's radar scan (§10.2). With no run pinned, model layers (forecast reflectivity, CAPE/SRH, the other regional fields, the global models) used to read the newest run even under a replayed historical event; scrubbed back more than three hours they now read the newest cycle at or before the view's time from the NOAA archive (§10.3). Before the archive's start (HRRR: 2014) the fetch fails visibly rather than substituting today's run.

Unified tornado detection (a first part of §2.1): rotation couplets, debris signatures and Tornado ID drew a marker each, so one tornado read as several, and an observed tornado warning confirmed every weak detection inside its polygon. `wxdata::tornado_id::circulations` now centres one detection on the most likely area of rotation (the best-corroborated cyclonic couplet; debris with no rotation near it centres its own) and ties in every couplet and debris signature within 15 km (checked on the 11 December 2021 Quad-State storm, whose debris ball and couplets sat 11–13 km apart), keeping each as a member with its own provenance and confidence. The verdict is Tornado ID over the circulation's own members, so tiers and reasons are unchanged; a circulation needs radar evidence or a tornado report of its own, and an observed warning raises its tier but cannot create one. The map draws one marker per tornado; a click or tap opens a web of spokes to each tied detection and a pinned card with the verdict, the members and the factor breakdown of the one picked. "One detection per tornado" (Severe layers, on by default) switches back to a marker per detection. Still to join the object: SCIT cells, ProbSevere, hail attributes and manual motion.

Storm selection (§2.5): the Cell window, which already held the SCIT attributes, severity and its reasons, core statistics, the forecast track and trends, gains a **Threat** section: the merged tornado detection at the storm (tier, score, signals, where from the core, or that the detectors are off), each warning whose polygon holds it with its tornado tag, and when SCIT's motion brings it to each saved place, in-path first, with the motion it came from. A **Track manually** button seeds the manual motion tool from it.

Telemetry (§14.1, local only): the Analyst log now gives this app's own cost: how long each frame takes to build (p50/p95/max over the last 600 frames, `app::telemetry`), how many of those ran over a 60 Hz refresh, and stalls over 50 ms since launch, beside the panes open and the radar volumes held. It times `HookEchoApp::ui` from outside, so early returns count; it is CPU build time, not the gap between frames (the app repaints on demand). Still open: GPU upload and presentation time, cache hit rates, HTTP latency and memory.

Basemap robustness (§4.5): ancestor fallback was already in place (a missing tile is stood in for by resident children or the nearest ancestor), but a failed tile was retried every 5 s forever. Retries now back off from 5 s, doubling to a 5-minute cap (`tiles::retry_after`), and the Analyst log shows coverage: tiles loaded, loading, failed, and how many are backing off.

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
