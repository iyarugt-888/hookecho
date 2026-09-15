# HookEcho Radar Analysis Maturity Suggestions

> Target branch: `feat/wsv3-redesign`
>
> Purpose: implementation backlog for Codex / Claude Code based on review of the current WIP analyst interface and a 3D Spectrum Width replay of the December 10, 2021 western Kentucky / Mayfield tornado.

## Executive summary

The current WIP already demonstrates a genuinely useful differentiator: interactive browser-based 3D interrogation of a full radar volume rather than a decorative 3D extrusion of a single 2D sweep. In the Mayfield replay, the dominant high-Spectrum-Width structure follows the surveyed tornado corridor convincingly through Mayfield, Benton, Kentucky Lake, Princeton, Dawson Springs, and toward Mortons Gap.

The main gap between HookEcho and mature analyst software is no longer simply "add more products." The priority is **scientific state integrity, temporal provenance, beam/radar geometry, exact inspection tools, and workflows that prevent users from over-interpreting a visually compelling 3D volume.**

This document should be treated as a prioritized engineering plan. Do not implement everything as one monolithic rewrite. Preserve existing functionality and progressively harden the architecture.

---

# 1. Findings from the Mayfield tornado replay

## 1.1 Ground-track / timing validation

The reviewed replay used KHPX and showed a strong time/location correspondence between the dominant high-Spectrum-Width structure and the documented tornado path.

Approximate checkpoints from the review:

| Playback time | Documented tornado area | Review result |
|---|---|---|
| ~9:00–9:05 PM CST | Cayce / Fulton-Hickman corridor | Broad high-SW structure present; not yet a clean low-level tornado-scale signature |
| ~9:16 PM | Entering Graves County | Concentrated structure advances into the correct corridor |
| ~9:27 PM | Mayfield | Very strong spatial / temporal correspondence |
| ~9:44–9:50 PM | NW Benton | Structure progresses through the correct area at the expected time |
| ~9:55–9:57 PM | Cambridge Shores / Kentucky Lake | Excellent correspondence |
| ~10:17–10:22 PM | South of Princeton | Excellent progression through the expected corridor |
| ~10:32–10:34 PM | Dawson Springs | One of the strongest matches in the demonstration |
| ~10:50 PM | Barnsley / Earlington / Mortons Gap corridor | Correct downstream progression |

The 3D playback is therefore useful, but the UI must not imply that Spectrum Width itself is a direct tornado-position product.

## 1.2 Critical interpretation caveat: Spectrum Width != tornado

Spectrum Width measures Doppler velocity dispersion within the sample volume. Elevated values may arise from strong shear, turbulence, spectral broadening, mixed motions, and data-quality effects.

The current rendering can visually resemble a vertically continuous "tornado column." That is potentially misleading.

### Required UI behavior

- Never label a high-Spectrum-Width volume as "tornado" without a separate detection / inference layer.
- Add product-specific help text and metadata.
- In analyst mode, show a concise explanation such as:
  - `Spectrum Width: velocity dispersion within the radar sample volume; useful for identifying shear/turbulence but not a standalone tornado locator.`
- If automated tornadic-circulation detection is added later, represent it as a separate derived product with confidence and provenance.

## 1.3 Radar selection mattered substantially in this case

KHPX is well positioned for the latter portion of the tornado track but is a poor choice for the early Cayce / Mayfield portion compared with KPAH.

The application should therefore stop treating "selected radar" as merely a manual visualization preference. For serious analysis, radar choice changes the physical height, beam width, resolution, and quality of what the analyst is seeing.

### Required direction

Implement **radar suitability scoring** and optional **multi-radar analysis**.

For any map location / storm target, compute for nearby radars:

- great-circle distance
- beam-center height for the selected elevation angle
- approximate beam-bottom / beam-top height
- beam width at target range
- blockage / known coverage limitations where data are available
- product availability
- age of newest volume
- scan strategy / VCP

Then rank candidates rather than blindly using nearest-site distance.

---

# 2. P0 — scientific correctness and state integrity

These items should be addressed before adding large new analyst features.

## 2.1 Make frame / volume transitions atomic

During the replay review, visible application state could temporarily disagree during rapid timeline transitions. For example, the prominent time display could advance while URL/share-state data still referred to the previous scan.

Even if the underlying volume had already switched, this is unacceptable ambiguity in an analyst tool.

### Requirement

Represent the selected radar frame as one immutable state object, e.g. conceptually:

```ts
interface RadarFrameState {
  radarId: string;
  volumeId: string;
  nominalTime: string;
  volumeStartTime: string | null;
  volumeEndTime: string | null;
  vcp: number | null;
  products: ProductFrameState[];
  tilts: TiltFrameState[];
  source: RadarSourceInfo;
}
```

Do not independently update:

- map voxels
- 2D tilt raster
- timeline label
- URL timestamp
- product legend
- scan metadata
- warning / overlay time

Instead:

1. request target frame
2. decode / validate
3. prepare dependent layers
4. commit one frame-state transaction
5. only then update URL + visible timestamp

### UX states

Expose explicit states:

- `Loading 10:22:51 PM...`
- `Rendering volume...`
- `Ready`
- `Partial / unavailable`

Do not show the new time as fully active while the old volume is still on screen.

### Acceptance tests

- Rapidly scrub 50+ historical frames and assert that all rendered layers share the same `volumeId` / target timestamp.
- Assert the share URL always reproduces the currently committed frame.
- Simulate slow network / decoding and ensure no hybrid old/new frame is presented as complete.
- Add Playwright test coverage for scrub -> screenshot -> URL -> reload state equivalence.

## 2.2 Separate plan-view tilt selection from 3D volume selection

The reviewed UI showed a specific elevation (for example `0.4°`) selected while the displayed 3D geometry appeared to use the full volume. That is ambiguous.

### Replace ambiguous control semantics

Use separate controls such as:

- **Plan View Tilt:** `0.4°`, `0.8°`, `1.3°`, ...
- **3D Volume Tilts:** `All`, `Low-level only`, `Custom...`

If a product is rendered from all tilts, say so explicitly in the 3D legend / inspector.

### Acceptance criteria

A screenshot must make it unambiguous whether the user is seeing:

- one sweep
- several selected sweeps
- the full volume
- an interpolated Cartesian volume derived from all available tilts

## 2.3 Preserve per-tilt acquisition time

A WSR-88D volume is not instantaneous. Different elevation scans are acquired at different moments. For a fast-moving supercell, the storm can travel several miles during one full volume.

A 3D volume that ignores this can create visually convincing but physically misleading vertical tilt / displacement.

### Data model

Every tilt should retain:

```ts
interface TiltFrameState {
  elevationDeg: number;
  startTime: string | null;
  endTime: string | null;
  representativeTime: string;
  ageRelativeToVolumeTimeSec: number;
  radialCount: number;
  gateCount?: number;
}
```

If individual radial timestamps are available, preserve those too rather than collapsing everything immediately to a tilt-level time.

### Add a `Temporal Provenance` visualization mode

Options:

- color by scan age
- fade older portions of the volume
- tooltip: `Sampled 132 s before nominal volume time`
- per-tilt timeline strip
- optional storm-motion temporal correction / advection experiment, clearly marked as derived

### Important

Do **not** silently time-correct data. Raw-native and temporally adjusted displays must be distinguishable.

## 2.4 Add exact source provenance everywhere

Analyst mode should make the data chain inspectable.

For a hovered / selected gate or voxel, show where applicable:

- radar site
- source / Level II archive provider
- volume identifier
- product
- native moment code
- VCP
- sweep / elevation
- azimuth
- range
- gate index
- sample time
- value
- missing / range-folded / quality flags
- whether value is native or interpolated
- interpolation method
- nearest native gates contributing to an interpolated voxel

This is a major distinction between a visualization and an analysis system.

---

# 3. P0 — beam geometry and spatial truth

## 3.1 Show beam height, not just elevation angle

Elevation angle alone is insufficient. Add beam geometry based on Earth curvature and standard atmospheric refraction assumptions.

For selected radar + cursor target calculate:

- slant range
- ground range
- beam center MSL
- beam center AGL when terrain data are available
- beam bottom / top approximation using antenna beam width
- horizontal beam width at target range

The effective-Earth-radius model (commonly 4/3 Earth radius) is a reasonable default, but expose the assumption.

### UI examples

`KHPX · 0.4° · 121 km · beam center ~1.9 km ARL · beam width ~2.0 km`

Do not hard-code event-specific values; compute dynamically.

## 3.2 Add beam visualization

Optional analyst layers:

- beam cone / frustum from radar to selected target
- vertical cross-section of beam center/top/bottom versus range
- radar horizon / coverage visualization
- overlapping beams from two radars

This would make HookEcho meaningfully more educational and more scientifically transparent than many existing tools.

## 3.3 Terrain-aware AGL

Where feasible, include a terrain model so the user can toggle:

- MSL
- ARL (above radar level)
- AGL

For tornado analysis, AGL is often the most intuitive vertical reference.

---

# 4. P1 — multi-radar analysis and intelligent radar selection

## 4.1 Radar suitability panel

When the analyst clicks a storm / location, show nearby radar candidates with a suitability score.

Possible inputs:

```text
score =
  distance_weight
  + low_level_beam_height_weight
  + beam_resolution_weight
  + data_age_weight
  + blockage_weight
  + product_availability_weight
```

Do not present a single opaque score only. Show the reasons.

Example:

```text
KPAH   Recommended
38 km · 0.4° center 0.5 km ARL · newest volume 41 s old

KHPX
121 km · 0.4° center 1.9 km ARL · newest volume 22 s old
```

## 4.2 Synchronized dual-radar mode

Implement linked radar panes / volumes sharing an absolute target time.

Features:

- Radar A / Radar B selectors
- same storm-relative cursor
- same target time
- nearest scan on each radar
- show actual scan-time delta for each
- common map / camera lock option
- independent or shared product selection

The Mayfield event is an ideal regression case:

- early segment: compare KPAH vs KHPX
- later segment: KHPX becomes increasingly favorable

## 4.3 Smart radar handoff

Optional mode:

`Auto-select best low-level radar for tracked storm`

When the recommended radar changes, never silently switch in an analyst workflow. Show:

`Recommended radar changed: KPAH -> KHPX`

with an explanation and a one-click accept action.

---

# 5. P1 — 3D volume analysis maturity

## 5.1 Native-gate vs interpolated-volume modes

The UI must explicitly distinguish:

### Native mode

Render actual polar radar gates at their physical beam locations.

Advantages:

- scientifically faithful
- no hidden interpolation
- reveals sampling gaps

### Interpolated mode

Render a Cartesian / volumetric reconstruction.

Required metadata:

- grid resolution
- interpolation method
- vertical / horizontal search radius
- missing-data behavior
- source tilts

Add a visible badge such as:

`INTERPOLATED 500 m × 500 m × 250 m`

## 5.2 Vertical cross sections

Implement proper analyst cross sections comparable to mature radar applications.

Required modes:

- point-to-point transect
- storm-motion-normal transect
- storm-motion-parallel transect
- radial cross section from radar

Products should include at least:

- Reflectivity
- Velocity
- Spectrum Width
- Correlation Coefficient
- Differential Reflectivity when available

Cross section inspector should show exact x/y/z and source sample provenance.

## 5.3 Isosurfaces

Add configurable isosurfaces for appropriate fields.

Examples:

- reflectivity >= X dBZ
- CC <= X
- Spectrum Width >= X m/s

Do not imply that all products have equally meaningful isosurfaces. Product documentation should explain interpretation.

Controls:

- threshold
- opacity
- smoothing
- native/interpolated source
- clip range / altitude

## 5.4 3D clipping and slicing

Add analysis-friendly volume controls:

- minimum / maximum altitude
- maximum range
- azimuth sector clipping
- vertical clipping planes
- horizontal slice altitude
- hide values below / above thresholds
- storm-relative bounding box

These controls will improve legibility dramatically for dense supercells.

## 5.5 Lighting and depth cues

The current volumetric structure is useful but visually dense.

Add optional:

- physically coherent depth shading
- light direction
- ambient light
- opacity transfer functions
- altitude grid
- subtle ground shadow / footprint when appropriate
- anti-aliased voxel / point rendering

Do not sacrifice quantitative color-table fidelity for visual spectacle. Analyst mode needs a `flat / quantitative` rendering option.

---

# 6. P1 — timeline and archive workflow

## 6.1 One authoritative timeline

Radar, warnings, storm tracks, satellite, model data, reports, and annotations should all bind to a common `analysisTime` abstraction.

Each layer should expose:

- source time
- nearest available frame
- offset from analysis time
- interpolation behavior if any

Example:

```text
Analysis time: 2021-12-10 22:34:12 CST
KHPX L2:       22:34:07  (-5 s)
Warnings:      22:34:12  (valid)
GLM:           22:34:00  (-12 s)
Damage track:  survey/static
```

## 6.2 Archive playback controls

Add:

- step one volume backward / forward
- step one tilt backward / forward where possible
- 0.25× / 0.5× / 1× / 2× / 4× playback
- real-time-spacing playback
- fixed-rate playback
- keyboard shortcuts
- loop region
- bookmarks

## 6.3 Timeline event markers

Allow event overlays such as:

- warning issued
- PDS / emergency wording
- tornado report
- debris signature detection
- survey checkpoint
- radar VCP change
- SAILS/MESO-SAILS change
- radar outage

For historic case studies, this turns the timeline into an analysis instrument rather than only a scrubber.

---

# 7. P1 — severe-weather interrogation workflow

## 7.1 Linked multi-product inspection

Add a synchronized analyst layout that can display, for example:

- Reflectivity
- Storm-relative velocity / base velocity
- Correlation Coefficient
- Spectrum Width

with one shared cursor and shared time.

When the user hovers in any pane, show the same geographic point in all others.

## 7.2 Product-specific default presets

Examples:

### Tornado interrogation

- SRV / Velocity
- CC
- Reflectivity
- Spectrum Width
- lowest useful tilts
- warning polygons
- optional surveyed / historical track in archive mode

### Hail interrogation

- Reflectivity
- ZDR
- CC
- KDP if available / derived
- MESH / MRMS overlays if supported

### QLCS interrogation

- Velocity / SRV
- Reflectivity
- Spectrum Width
- azimuthal shear / derived rotation where available

Presets should configure a workspace, not make automatic meteorological claims.

## 7.3 Gate inspector

A click or keyboard shortcut should lock a native gate and show values across:

- all products
- adjacent tilts
- recent volumes

Potential mini-chart:

`time -> Z / V / SW / CC`

This is a high-value professional feature.

---

# 8. P1 — case-study validation infrastructure

The Mayfield event should become an automated / semi-automated regression case for the radar engine.

## 8.1 Case-study manifest

Create a data-driven format such as:

```json
{
  "id": "2021-12-10-mayfield",
  "title": "Western Kentucky Tornado",
  "radars": ["KPAH", "KHPX"],
  "start": "2021-12-11T02:55:00Z",
  "end": "2021-12-11T05:10:00Z",
  "checkpoints": [
    {"time": "2021-12-11T03:27:00Z", "label": "Mayfield"},
    {"time": "2021-12-11T03:57:00Z", "label": "Cambridge Shores"},
    {"time": "2021-12-11T04:34:00Z", "label": "Dawson Springs"}
  ]
}
```

Keep surveyed geometry / official metadata separate and cite its source.

## 8.2 Validation tests

At minimum test:

- correct archive volume selection
- correct tilt count
- monotonically correct per-tilt timestamps
- deterministic 3D gate placement
- beam-height calculations
- no timestamp / URL mismatch
- camera-independent data state
- exact reproduction from shared URL
- correct radar handoff recommendations

## 8.3 Screenshot / visual regression

Maintain stable screenshots for important case-study frames and compare:

- UI state
- product legend
- timestamp
- radar ID
- tilt / all-volume state
- major 3D geometry

Do not use screenshots as the only scientific validation, but they are useful for catching rendering regressions.

---

# 9. P2 — warnings, tracks, and ground truth overlays

## 9.1 Historical surveyed tornado tracks

For archive case studies, support official surveyed tracks where licensing / source permits.

Render distinctly from real-time inferred tracks.

Suggested legend:

- `Official post-event survey track`
- `Real-time radar-derived rotation track`
- `User annotation`

Never visually conflate those categories.

## 9.2 Storm-motion vectors

Support:

- estimated storm motion
- user-editable motion vector
- time-to-location projection
- storm-relative coordinates

Use this for cross-section orientation and temporal-volume analysis.

## 9.3 Radar-derived rotation layer

If implementing azimuthal shear / rotation detection:

- disclose algorithm
- disclose smoothing / dealiasing requirements
- provide confidence / quality flags
- do not label every rotation maximum as a tornado
- allow analyst to inspect underlying velocity data

---

# 10. P2 — UI/UX cleanup for professional density

The existing interface contains useful information but too many elements can compete visually in 3D.

## 10.1 Analyst focus mode

Add a mode that reduces nonessential labels / chrome while preserving:

- radar metadata
- active product
- time
- legend
- cursor inspection
- timeline
- warnings / selected overlays

## 10.2 Layer hierarchy

Create clear visual priority groups:

1. radar data
2. analyst selection / cursor
3. warnings / storm tracks
4. geographic context
5. radar-site labels / secondary annotations

Avoid radar-site labels and polygons overwhelming the volume itself.

## 10.3 Metadata panel

A compact panel should show:

```text
Radar: KHPX
Product: Spectrum Width
Mode: Full 3D volume
Plan-view tilt: 0.4°
3D tilts: All available
VCP: 212
Volume: 04:34:07Z
Volume span: 04:31:xx–04:34:xxZ
Interpolation: Native gates
Max displayed range: 140 km
```

This should also be copyable for bug reports / scientific notes.

## 10.4 Share-state robustness

A shared URL should be able to restore, where feasible:

- radar
- analysis time
- product
- plan-view tilt
- 3D tilt set
- camera position
- clipping
- color table
- threshold / opacity
- enabled overlays
- pane layout

Version the share-state schema so old URLs do not silently break.

---

# 11. Comparison targets

Use mature products as workflow references, not as designs to clone.

## GR2Analyst / GRLevelX

Benchmark against:

- rigorous Level II interrogation
- cross sections
- volume rendering
- isosurfaces
- exact gate inspection
- archive workflow
- derived products

HookEcho can differentiate with a modern cross-platform / web-first interface while matching scientific transparency.

## RadarScope

Benchmark against:

- operational maturity
- fast product switching
- reliable archive workflow
- warnings / metadata
- multi-pane interrogation
- polished mobile interaction

HookEcho's strongest differentiation should remain richer 3D analysis.

## WSV3

Benchmark against:

- synchronized multi-dataset timeline
- integrated severe-weather workstation concepts
- multi-product / GIS workflow
- analyst density
- 3D tilt control

Avoid inheriting legacy workstation complexity unnecessarily.

## WeatherWise / modern 3D weather interfaces

Benchmark against:

- 3D usability
- beam-rise communication
- cross sections
- low-friction navigation
- visual polish

HookEcho should expose more provenance and native-data detail than a primarily visualization-focused product.

---

# 12. Architecture recommendations

## 12.1 Separate domain state from render state

Do not let Mapbox / WebGL objects become the source of truth.

Suggested layers:

```text
Radar source / archive
        ↓
Decoder + normalization
        ↓
Immutable meteorological domain model
        ↓
Derived geometry / interpolation workers
        ↓
Render adapters
        ↓
Map / 3D / charts / inspector UI
```

The UI should be reconstructable from domain state.

## 12.2 Web Workers / background compute

Move expensive tasks away from the UI thread:

- Level II decode where practical
- gate-to-world transforms
- Cartesian interpolation
- isosurface generation
- cross sections
- derived moments

Use transferable buffers / typed arrays to minimize copies.

## 12.3 Cache by scientific identity

Cache keys should include enough information to prevent stale or cross-product reuse, e.g.:

```text
radarId / volumeId / product / elevation / transformVersion / interpolationVersion
```

Do not key important scientific data only by visible timestamp.

## 12.4 Deterministic rendering inputs

Given the same:

- source volume
- product
- tilt set
- interpolation config
- threshold

3D geometry should be deterministic and testable independent of camera state.

---

# 13. Performance requirements

The current browser performance is promising. Preserve it while increasing rigor.

Suggested goals for a modern desktop GPU:

- timeline input response < 50 ms
- existing cached frame swap perceived as immediate
- progressive 3D volume appearance rather than long blank waits
- UI thread remains responsive during interpolation / isosurface computation
- no unbounded GPU buffer accumulation while scrubbing
- bounded decoded-volume cache with visible memory policy in developer diagnostics

Add diagnostics for:

- decode time
- transform time
- upload time
- draw time
- gate / voxel count
- GPU memory estimate
- cache hit / miss
- network latency

---

# 14. Documentation requirements

Create concise analyst documentation for each moment.

For every radar product document:

- physical meaning
- units
- native value range
- common signatures
- common failure modes / artifacts
- range limitations
- relationship to beam geometry
- whether interpolation is safe / risky
- whether thresholding / isosurfaces are meaningful

Spectrum Width documentation is particularly important because the current 3D display can be visually over-interpreted.

Recommended references to validate implementation assumptions against:

- NWS / WDTD radar training material
- ROC / WSR-88D documentation where applicable
- NCEI Level II format / archive documentation
- official NWS event surveys for historical validation

Do not encode third-party application behavior as meteorological truth.

---

# 15. Suggested implementation sequence for Codex / Claude Code

Agents should work through this in the following order and make small, reviewable commits.

## Phase A — correctness foundation

1. Audit current radar/time state architecture.
2. Introduce an immutable committed-frame / committed-volume identity.
3. Make URL, timeline, plan view, 3D volume, legend, and metadata update atomically.
4. Preserve per-tilt acquisition timestamps.
5. Separate plan-view elevation selection from 3D tilt-set selection.
6. Add native/interpolated provenance fields.
7. Add regression tests for rapid archive scrubbing.

**Do not proceed to flashy new 3D features until state-integrity tests pass.**

## Phase B — geometry and inspection

1. Implement reusable beam-geometry utilities.
2. Add beam center/top/bottom + beam width to cursor metadata.
3. Add exact native gate inspector.
4. Add optional beam visualization.
5. Add MSL / ARL / AGL vertical coordinate support.

## Phase C — mature 3D tools

1. Add clipping / slicing.
2. Add cross sections.
3. Add native-gate vs interpolated-volume mode.
4. Add interpolation metadata.
5. Add isosurfaces.
6. Add temporal-age visualization.

## Phase D — multi-radar workflow

1. Radar suitability scoring.
2. Dual-radar synchronized view.
3. Shared cursor / analysis time.
4. Radar recommendation / handoff UX.
5. Mayfield KPAH/KHPX regression case.

## Phase E — professional severe-weather workspace

1. Linked multi-product panels.
2. Severe-weather presets.
3. Gate time-series inspector.
4. Timeline event markers.
5. Historical ground-truth overlays.
6. Robust versioned share-state.

## Phase F — low-latency operational radar

1. Emit usable live radial/chunk batches before elevation completion.
2. Update only affected radial ranges in persistent GPU sweep storage.
3. Retain previous-sweep sectors until new data replace them; track current/previous/missing state explicitly.
4. Preserve radial acquisition timestamps and show an optional smooth radial-reveal effect without misrepresenting network arrival semantics.
5. Add beam/sample-to-screen latency diagnostics.
6. Evolve `Level2LiveProvider` into runtime-selectable providers with normalized health/provenance.
7. Add active-active provider selection so the first valid copy of a live Level II record wins and backups can fill gaps without a slow global failover.
8. Keep direct public-data acquisition as a bypass if any HookEcho-hosted push path is unavailable.
9. Add a push-oriented ingest path, then later add a lower-latency direct/LDM-class provider when legitimately available.
10. Feed the same incremental live events into 3D so the active volume grows as elevations are scanned.

---

# 16. Definition of "mature analyst feature"

A feature should not be considered complete merely because it renders correctly once.

For analyst functionality, require:

- scientific units are explicit
- source time is explicit
- data provenance is inspectable
- interpolation / derivation is disclosed
- missing / invalid data is not silently converted to zero
- screenshot state is self-describing
- URL/share state reproduces the analysis
- mobile / desktop behavior is defined
- performance under rapid timeline scrubbing is tested
- unit tests exist for numerical transforms
- visual / integration tests exist for major workflows
- user-facing documentation explains limitations

---

# 17. Immediate high-value tasks

If only a small number of items can be implemented next, do these first:

1. **Atomic frame-state commits** — eliminate timestamp / URL / volume ambiguity.
2. **Incremental live-sweep rendering** — use already-arriving partial Level II chunks before an elevation completes.
3. **Live latency/provenance telemetry** — preserve radial sample time and measure sample-to-screen delay.
4. **Separate 2D tilt vs 3D tilt-set controls** — remove current UI ambiguity.
5. **Per-tilt acquisition time and temporal-age display** — expose that a radar volume is not instantaneous.
6. **Beam-height / beam-width inspector** — immediately improves scientific interpretation.
7. **Native-gate inspector with provenance** — moves the project toward true analysis software.
8. **Provider multiplexing/fallback foundation** — prepare for multiple live Level II sources without putting network logic in `app.rs`.
9. **Radar suitability ranking** — especially useful for long-track storms crossing radar domains.
10. **KPAH + KHPX synchronized Mayfield case study** — strong test case for the entire architecture.
11. **Cross sections + clipping** — high-value next-generation 3D interaction features.

---

# 18. Mayfield regression scenario

Use the December 10, 2021 western Kentucky tornado as an ongoing validation case.

Suggested flow:

1. Begin before the tornado reaches Cayce.
2. Use KPAH for early low-level interrogation.
3. Compare KPAH and KHPX through Mayfield / Benton.
4. Show radar geometry differences at identical ground points.
5. Continue KHPX through Princeton, Dawson Springs, Barnsley / Earlington, and Bremen.
6. Extend archive playback to at least ~11:10 PM CST so the full late-stage progression can be evaluated.
7. Overlay official survey path only in archive/case-study mode and label it as post-event ground truth.
8. Compare Reflectivity, Velocity/SRV, CC, and Spectrum Width rather than evaluating Spectrum Width in isolation.
9. Verify that all products and both radars are aligned by an explicit common analysis time while retaining each source scan's actual acquisition time.

This scenario should make it easy to catch:

- wrong radar recommendations
- wrong volume selection
- stale URL state
- tilt-time loss
- coordinate / beam-height bugs
- multi-radar synchronization bugs
- rendering differences between native and interpolated data

---

# 19. Product maturity target

The goal should not be to imitate one competitor. A compelling HookEcho analyst platform would combine:

- GR2Analyst-level transparency for native radar data
- RadarScope-level speed and operational reliability
- WSV3-style synchronized workstation concepts
- modern 3D interaction and cross-platform accessibility
- explicit beam geometry and temporal provenance that many existing products under-emphasize

The strongest current differentiator is the browser-friendly 3D radar-volume workflow. The next step is to make that visualization scientifically self-describing, deterministic, and inspectable enough that an analyst can trust exactly **what** was sampled, **where**, **when**, and **how it became the geometry on screen**.

For operational maturity, the next major target is **incremental live Level II display with measured latency and provider resilience**. A fast-looking animation is not enough: HookEcho should be able to prove when a radial was sampled, when it reached the client, which provider supplied it, and whether any part of the displayed sweep is retained from an older scan or filled by a fallback source.

---

# 20. External validation references

Use primary meteorological documentation wherever possible during implementation and testing.

- NWS Paducah — December 10–11, 2021 tornado event / damage survey:
  - https://www.weather.gov/pah/December-10th-11th-2021-Tornado
- NWS / WDTD radar training material:
  - https://training.weather.gov/wdtd/
- NOAA / NCEI NEXRAD Level II archive information:
  - https://www.ncei.noaa.gov/products/radar/next-generation-weather-radar
- NOAA ROC WSR-88D technical documentation / Message 31 interface documentation:
  - https://www.roc.noaa.gov/public-documents/icds/2620002J.pdf
- Unidata NEXRAD Level II / real-time distribution information:
  - https://www.unidata.ucar.edu/data/radar/levelii
- NOAA NEXRAD open-data registry:
  - https://registry.opendata.aws/noaa-nexrad/
- GRLevelX / GR2Analyst manual for competitive workflow reference:
  - https://www.grlevelx.com/manuals/gr2analyst/
- RadarScope product information for competitive workflow reference:
  - https://www.radarscope.app/
- WSV3 for competitive workstation reference:
  - https://wsv3.com/

When behavior in a competitor conflicts with official radar documentation, follow the official radar documentation.

---

# 21. P0 — low-latency live Level II and fallback requirements

The current live path already receives partial Level II information during a sweep, but visible radar updates should no longer wait for the entire elevation to finish. Treat a usable radial/chunk batch as the live rendering unit while keeping complete-sweep and complete-volume events for bookkeeping.

Required implementation behavior:

- decode and surface each usable incoming live batch as soon as possible;
- preserve original radial acquisition times, azimuths, elevation, gate geometry, and missing/range-folded state;
- update only the corresponding radial range in persistent GPU storage rather than rebuilding the whole sweep/volume;
- leave the previous sweep visible in sectors not yet replaced by the new sweep;
- maintain an explicit current/previous/missing generation mask so retained old data are never confused with newly scanned data;
- optionally animate received radials into view in acquisition order, but clearly treat that as presentation of already-received data rather than literal per-gate network delivery;
- expose sample-to-screen latency and source provenance in diagnostics and, where useful, the gate inspector;
- support multiple live Level II providers behind one normalized provider interface;
- prefer first-valid-record-wins behavior so already-connected backups can fill missing live data without waiting for a long global failover timeout;
- detect duplicate/conflicting copies of the same radar/volume/sequence identity rather than silently mixing inconsistent bytes;
- keep direct public-data acquisition available if a hosted push source is unavailable;
- visually label degraded modes when falling back to completed Level II or Level III products;
- extend the same incremental event model to native 3D after the 2D live path is proven stable.

Implemented so far: the 2D renderer retains its polar GPU texture and diffs successive binned
sweeps by azimuth row, coalescing adjacent changed rows into one texture write. This satisfies the
GPU upload half of the incremental path. Cached plain moments now also update only rows whose
newest radial timestamp advanced; KDP and dealiased velocity retain whole-field re-bins by design.
Avoiding the upstream full merged-scan clone remains CPU work to complete. Native and smooth 3D
already invalidate on every accepted live revision, while observed 3D retains and rewrites its gate
buffer rather than recreating GPU resources.

### Acceptance criteria

- a partial live elevation can visibly update before sweep completion;
- unchanged radial ranges are not re-decoded/re-uploaded unnecessarily;
- missing data remain distinct from zero/below-threshold values;
- provider/source identity survives mixed-source fallback;
- reconnect/out-of-order/duplicate/missing-batch cases are covered by tests;
- SAILS/MRLE repeated low-level cuts remain correctly ordered and identifiable;
- latency metrics can report at least sample time, client receive time, decode completion, and GPU commit time when those timestamps are available;
- progressive 3D never presents a partially sampled multi-minute volume as an instantaneous complete observation.

This P0 work should be developed alongside Section 2 state correctness. The short-term win is to use the partial Level II data HookEcho already receives; provider/relay improvements should reduce upstream delay afterward rather than blocking incremental rendering on infrastructure work.
