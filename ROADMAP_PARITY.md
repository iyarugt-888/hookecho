# HookEcho — Competitive Gap Closure Roadmap

> Implementation guide for Codex, Claude Code, and human contributors.
>
> Audit date: **2026-10-01**. Code baseline: **`3a7ab513b910b484be2b04520c922e18d465e82f`**, branch `feat/wsv3-redesign`, repository `iyarugt-888/hookecho`.
>
> **Delivery priority: desktop and Android together. Browser feature delivery may follow, but existing browser capabilities must keep compiling and working.**

This document turns the remaining WSV3 and GR2Analyst gaps into complete operator workflows and measurable release gates. It supplements [ROADMAP_2.md](ROADMAP_2.md) and [ROADMAP_NEW.md](ROADMAP_NEW.md); those documents retain their implementation history and broader backlog. This roadmap's audit and task dependencies determine the sequence for the competitive work below. Repository instructions and explicit maintainer decisions remain authoritative.

The audit inspected implementations, integration points, tests, and CI configuration. It did not run competitors side by side, measure field performance, or certify physical devices. A feature's presence in code establishes an implementation foundation, not operational parity. Historical test counts in the older roadmaps describe their recorded runs and are not fresh certification evidence.

## Contents

- [1. Product objective and comparison baseline](#1-product-objective-and-comparison-baseline)
- [2. Audited capabilities and remaining gaps](#2-audited-capabilities-and-remaining-gaps)
- [3. Delivery sequence and interface contracts](#3-delivery-sequence-and-interface-contracts)
- [4. Implementation task cards](#4-implementation-task-cards)
- [5. Validation and professional release gates](#5-validation-and-professional-release-gates)
- [6. Codex and Claude Code execution protocol](#6-codex-and-claude-code-execution-protocol)
- [7. Later backlog and maintenance](#7-later-backlog-and-maintenance)
- [8. References](#8-references)

## 1. Product objective and comparison baseline

An operator should be able to monitor an unfolding U.S. severe-weather event, inspect a storm in 2D and 3D, estimate impacts, combine operational GIS assets, and produce reproducible analysis or broadcast output in one application. Freshness, coverage, source failure, and uncertainty must remain understandable throughout the workflow.

### Decisions fixed for this roadmap

- Windows is the primary professional desktop certification target. Preserve Linux's existing build/test gates and existing macOS behavior; additional macOS delivery is outside the critical path.
- Android phones and tablets are priority analysis surfaces. Preserve the existing phone layout and the existing decision to use workstation chrome on tablets at the 600 dp form-factor boundary. Improve touch operation within those layouts rather than creating a third UI system.
- Browser delivery can lag behind new native workflows. Keep shared interfaces, WASM compilation, existing browser smoke tests, and honest capability reporting throughout.
- Focus on competitive reliability, radar depth, GIS, synchronization, operator interaction, and broadcast. Do not expand the entire model/research/plugin backlog before these workflows work well.
- Preserve existing radar, derived products, native satellite, model, sounding, routing, export, workspace, and placefile implementations. Extend their contracts and close their documented ceilings.
- Preserve the account-free, local-preference, no-tracking product model. Optional user-configured relays may improve latency; core analysis must remain usable without a mandatory HookEcho service.

### Competitor claims and boundaries

**WSV3 Professional** is the baseline for progressive radar presentation, linked weather time, storm tracking/ETAs, model inspection, styled GIS, and clean graphical output. Its official overview also distinguishes the experimental/reworked next-generation product line from the established application. Treat next-generation development posts as design context, not proof that every announced feature is released. Source: [official WSV3 overview](https://wsv3.com/) and [WSV3 user guide](https://wsv3-static.s3.amazonaws.com/WSV3UserGuide.pdf).

**GR2Analyst 2** is the baseline for Level II analysis, derived fields, cross-sections, and translucent/isosurface volumetric display. **GR2Analyst 3** adds relevant comparison targets including user-defined products and maximum-value trails. Label these separately rather than attributing all of them to version 2. Sources: [GR2Analyst 2](https://www.grlevelx.com/gr2analyst_2/), [GR2Analyst 3](https://www.grlevelx.com/gr2analyst_3/), and [version 3 user-defined products](https://www.grlevelx.com/gr2analyst_3/udp.htm).

There is no blanket parity percentage. Compare completed operator workflows and published evidence. Scientific formulas are validated against documented methods and reference inputs; matching proprietary pixels or undocumented algorithms is not the acceptance criterion.

## 2. Audited capabilities and remaining gaps

Statuses in this table describe the audited baseline. **Foundation present** means reachable implementation exists. **Partial workflow** means a concrete limitation remains. **Unverified** means the required runtime or device evidence was not established by this audit.

| Capability | Code evidence at the baseline | Remaining work | Tasks |
| --- | --- | --- | --- |
| Progressive acquisition and failover | [Provider interface](crates/hookecho/src/volume.rs), [live-scan inventory](crates/hookecho/src/live_scan.rs), [radial continuation](crates/wxdata/src/continuation.rs), [ingest service](crates/radar-ingest/src) | Foundation present. Certify recovery across real failure domains and expose temporal coverage for derived and 3D products. | M1.1–M1.3 |
| Incremental radar rendering | [Live merge/cache handling](crates/hookecho/src/view.rs), [2D GPU uploads and queue timings](crates/hookecho/src/render/mod.rs), [3D texture uploads](crates/hookecho/src/render3d.rs) | Changed 2D azimuth rows already upload incrementally. Queue-write timings do not measure completed GPU work or visible presentation. Full-volume copying and 3D upload/build cost still need profiling. | M1.2, M3.6, M7.1 |
| Provenance and source health | [Common stamps](crates/wxdata/src/field.rs), [request book](crates/hookecho/src/app/request_book.rs), [layer probe](crates/hookecho/src/app/layer_probe.rs), [source families](crates/hookecho/src/source_health.rs) | Foundation present. Archive/loop receipts and some detections/observations lack retained stamps; derived dependencies need common lineage. Unknown metadata must remain unknown. | M1.4 |
| Storm selection and association | [Storms dock](crates/hookecho/src/app/chrome/dock/storms.rs), [spatial associations](crates/hookecho/src/app/chrome/dock/storm_associations.rs), [cell analysis](crates/hookecho/src/app/chrome/dock/cell.rs) | Spatial evidence preserves source objects, polygon holes, distance limits, and ambiguity. A persistent storm identity (stable IDs, provider-ID changes, split/merge lineage) carries a per-storm record of the warnings, ProbSevere objects, tornado detections and hail it was linked to over time, by the same coverage/nearest/ambiguity rule. Not yet calibrated on pinned fixtures; manual motion and per-entity ETA remain. | M2.1–M2.2 |
| Manual motion and impacts | [Motion tool](crates/hookecho/src/app/storm_track.rs), [shared geometry](crates/hookecho/src/app/storm_track_geometry.rs), [Census impacts](crates/wxdata/src/census.rs) | Points, lines, intervals, asymmetric widths, cones, handle constraints, and zone/marker ETAs exist. Complete geolocated community/asset impacts, case persistence, and device interaction proof. | M2.3–M2.4 |
| Gate inspection and scientific metadata | [Gate/beam metadata](crates/wxdata/src/level2.rs), [gate inspector](crates/hookecho/src/ui/gate_inspector.rs), [beam model](crates/wxdata/src/beam_geometry.rs) | Nyquist velocity and unambiguous range are decoded per radial from Message 31 (M3.1 increment 1) and shown as decoded, with the value-based estimate kept separate and labelled. Dealiasing and detectors now unfold at the decoded value behind a Py-ART-scored backtest (M3.1 increment 2). Moment scale/offset, gate quality and per-sector Nyquist for sectorized VCPs remain. | M3.1 |
| User-defined products | [Expression engine](crates/wxdata/src/udp.rs), [column evaluation](crates/wxdata/src/udp_column.rs), [portable files](crates/wxdata/src/udp_file.rs), [pane product cache](crates/hookecho/src/view.rs), [product editor](crates/hookecho/src/ui/udp_window.rs) | Gate products render in 2D and 3D. Column fields have map/probe/legend/export, trails and portable versioned definitions. Bounded nested reductions, masked statistics, extremum heights, interpolated crossings and layer integrals are implemented. Matched archived 0 through −40 °C inputs now use recorded geopotential MSL heights; live HRRR retains its three source fields and leaves −30/−40 missing (M3.3 increments below). Operand units/datum typing, further matched profile fields, per-pane independent evaluation, and meaningful column-field 3D/section use remain open. | M3.2–M3.3 |
| Temporal extrema | [Sliding trail](crates/wxdata/src/extrema.rs), [trail layer](crates/hookecho/src/app/trail.rs), [trail interaction regressions](crates/hookecho/src/app/trail_status_tests.rs) | Partial workflow. Polar trails are exact sliding windows anchored at the playhead with per-gate contributor times; age fade is GPU display opacity only (M3.4 increment 2). Column-product trails (`GridTrail`, increment 3) use the same window off-thread. Still one site/tilt for polar trails, cached frames only (no bounded prefetch); MRMS MESH/AzShear and other derived-grid trails remain. | M3.4 |
| Volume analysis | [Volume builder](crates/wxdata/src/volume3d.rs), [MIP shader](crates/hookecho/src/shaders/raymarch.wgsl), [opacity/slice controls](crates/hookecho/src/ui/volume3d_window.rs), [isosurfaces](crates/wxdata/src/isosurface.rs), [3D playback](crates/hookecho/src/app/view3d_state.rs) | MIP, opacity controls, isosurfaces, clipping, CAPPI reference, and playback already exist. Front-to-back translucent compositing with per-km opacity and optional gradient lighting now sits beside MIP (M3.5 increment 1, software-GPU verified). Editable stop lists, multi-threshold surfaces, ROI, measurement, and linked slicing remain. | M3.5–M3.6 |
| GIS | [Import integration](crates/hookecho/src/app/gis_import.rs), [geometry/styles](crates/hookecho/src/gis_import.rs), [Shapefile](crates/wxdata/src/shapefile.rs), [projection](crates/wxdata/src/projection.rs), [KML/KMZ](crates/wxdata/src/kml.rs), [export](crates/hookecho/src/gis_export.rs) | Formats, U.S. projections, styling, labels, and time filtering exist. Settings retain one imported layer; Android/browser single-file picking can omit shapefile sidecars. Complete collections, groups, filters, and real interoperability. | M4.1–M4.4 |
| Linked workspaces | [Workspace state](crates/hookecho/src/workspace.rs), [time selection](crates/wxdata/src/time_align.rs), [pane synchronization](crates/hookecho/src/app/pane_time.rs), [field state](crates/hookecho/src/app/field_state.rs) | Shared time/camera/cursor and comparisons exist. Link controls are global; some source contexts are shared. Independent groups need context-safe request and cache identity. | M5.1 |
| Satellite and model context | [ABI ingest](crates/wxdata/src/goes_abi.rs), [time integration](crates/hookecho/src/app/time_layers.rs), [model descriptors](crates/wxdata/src/model.rs), [MRMS catalog](crates/wxdata/src/mrms/catalog.rs), [GEFS](crates/wxdata/src/ensemble.rs) | The MRMS catalog contains 35 descriptors, and NAM/NAM nest/NBM and GEFS foundations already exist. Native ABI mesoscale frames follow radar time; a satellite-only one-minute loop remains open. Regional, GFS and ECMWF inventories are browsable with vetted units, any vetted field can be drawn, and wind pairs draw as barbs (M5.3 increments 1–6); comparison wiring remains. | M5.2–M5.3 |
| Broadcast and capture | [Clean output viewport](crates/hookecho/src/app/output_window.rs), [scene model](crates/hookecho/src/broadcast.rs), [scene application](crates/hookecho/src/app/scenes.rs), [loop capture](crates/hookecho/src/app/loop_capture.rs) | Output, dressing, scenes, and deterministic capture foundations exist. Output follows the active pane; scenes omit parts of full analysis state. Complete independent program state and sustained output proof. | M6.1–M6.4 |
| Resilience and corpus | [Data-path soak](crates/hookecho/src/soak.rs), [CPU frame telemetry](crates/hookecho/src/app/telemetry.rs), [historic tests](crates/wxdata/tests/golden_events.rs), [scheduled contracts/corpus](.github/workflows/provider-contracts.yml) | The soak runner decodes/bins radar without exercising the full renderer/UI. Historic cases are network-gated and dynamically locate archive scans. Add pinned offline inputs, resource accounting, full application stress, and Android lifecycle certification. | M0.3, M7.1–M7.3 |
| Tornado detection | [Fused analyst](crates/wxdata/src/llsd_analyst.rs), [fusion weights](crates/wxdata/src/tornado_fusion.rs), [debris classes](crates/wxdata/src/debris_class.rs), [wind-turbine mask](crates/wxdata/src/wind_turbines.rs), [markers and touch card](crates/hookecho/src/app/detector_markers.rs), [backtest markers](scripts/fusion/markers.py), [Storm Events verification](scripts/fusion/stormevents.py), [methodology and results](detectionplan.md) | Verified in the app's own markers against reports, NWS damage paths and NOAA Storm Events tracks. On 259 random severe-weather windows (2018–2025, three years held out): 0.56 false markers per radar-hour finding 38% of tornadoes, against the original Tornado ID's 3.14 and 16%; quiet days 1 false marker in 64 radar-hours. Random Storm Events tornadoes: 40% found; significant ones EF2 68%, EF3 90%. Remaining: QLCS and other small shallow vortices the LLSD column stage does not resolve (the original's couplets see some, at far more false alarms); scores are evidence, not calibrated probabilities; refits on four times the data and the measured near-ground features do not beat the shipped weights on ordinary days. Owned by Claude (Section 3). | M0.3, M1.4 |
| Architecture and input | [App shell](crates/hookecho/src/app.rs), [frame intake](crates/hookecho/src/app/frame_intake.rs), [pane drawing](crates/hookecho/src/app/draw_panes.rs), [frame end](crates/hookecho/src/app/frame_end.rs), [Android platform](android/README.md) | Extraction is substantial, but modules still share a large app owner. Narrow ownership and testable interfaces must accompany further extraction. Compile checks do not verify tablet targets, pen gestures, IME, or lifecycle behavior. | M0.2, M2.4, M7.2 |

### Corrections to older checklist assumptions

1. User-product map rendering, opacity controls, isosurfaces, and 3D playback should not be planned as entirely absent features. Their remaining limits are narrower and are named above.
2. Shapefile/KML/KMZ, projection handling, and GIS styling have progressed beyond the older summary acceptance statements. Their real gaps are collections, complete imports, advanced inspection, and external-tool verification.
3. Radial gap inventories already exist in `live_scan`; an earlier progress paragraph saying they remain open is stale.
4. Scheduled source contracts and the historic storm job already exist. Extend their reproducibility and coverage rather than proposing the schedule again.
5. `ROADMAP_NEW` immediate-start instructions and final scorecard contain older unchecked foundations, including caching and registry work. Re-audit before treating those checkboxes as missing code.
6. A listed or tested feature remains unverified for a platform until the operator workflow is demonstrated there. Do not convert a compile, isolated render, or synthetic gesture test into a physical-device claim.

## 3. Delivery sequence and interface contracts

### Current agent scope — 2026-10-02

The maintainer assigned Codex to WSV3-to-HookEcho feature gaps. Claude owns tornado detection
methodology and its ongoing calculations, including detector inputs/lineage, scoring, fusion,
calibration and detection-corpus work. Continue progressive acquisition, rendering reliability,
storm operator tools, GIS, linked context and broadcast features; keep detector files and
uncommitted peer work outside Codex commits. This assignment supersedes the default next-task
order where it would lead into detector work.

### Milestones

| Milestone | Priority | User-visible result | Completion gate |
| --- | --- | --- | --- |
| M0 — Execution foundations | P0 | Reproducible evidence and clearer feature ownership | Audit, seams, and pinned fixtures established |
| M1 — Live-data trust | P0 | Know what is current, complete, and actually being rendered | Temporal honesty, traceable latency, reliable recovery |
| M2 — Storm workflow | P0 | Select, inspect, track, and estimate impacts in one workflow | Persistent history and desktop/Android interaction proof |
| M3 — Radar depth | P1 | Usable column products, truthful trails, and deeper volume analysis | Scientific checks, integrated sampling/export, device performance |
| M4 — Operational GIS | P1 | Multiple restored asset/boundary layers with complete attributes | Real projected files and independent export validation |
| M5 — Context and presentation | P1 | Independent linked groups, satellite cadence, richer model inspection | Context isolation, time correctness, readable rendering |
| M6 — Broadcast | P1 | Independent program output with complete scenes | Atomic scene changes and deterministic sustained capture |
| M7 — Certification | P0 gate | A workstation and field companion supported by evidence | Long runs, failure recovery, resource and device gates |

The default agent execution order is M0 → M1 → M2 → M3 → M4 → M5 → M6 → M7, choosing the lowest ready task number within each milestone. M7 is a release gate, not permission to defer instrumentation: build its harnesses after M0/M1, then repeat relevant profiles as later features land. Task-card dependencies are the precise dependency graph; they permit earlier validation without requiring concurrent agents.

### Shared interface contracts

These names express required contracts. Reuse and extend equivalent existing types; do not create parallel abstractions solely to match the names.

- **Frame identity:** pane/context, subscription generation, provider, volume identity, cut/pass identity, moment/product, and accepted data revision. One accepted update can produce multiple render frames; trace each sample to the revision actually drawn.
- **Temporal coverage:** source acquisition interval, completeness, contributing cuts/volumes, mixed-time classification, and selected analysis reference. Preserve current `DataStamp` fields; use additive optional metadata/companions for richer coverage and lineage. Unknown historic receipt times must not be filled with the import or display time.
- **Request context:** source/product, site or geographic domain, run, valid-time selection, group/pane context, and generation. Shared immutable objects can be reused by content identity; context-specific display state cannot be keyed only by a global layer enum.
- **Storm identity:** stable local ID, immutable source references, timestamped association evidence, alternative matches, lineage, motion provenance, annotations, and history. Spatial coverage does not imply a confirmed same-storm identity.
- **User product:** versioned ID, output kind (`gate` or `column`), expression, units/quantity, palette/range, input dependencies, environmental requirements, and altitude convention. Preserve existing formulas and import their legacy semantics; diagnose incompatible definitions instead of silently changing them.
- **GIS collection:** stable layer ID, format/source reference, normalized geometries/attributes, per-layer style/filter/time mapping, ordering/group membership, and import diagnostics. Workspace/scene references use IDs rather than display names.
- **Output scene:** full selected view state, temporal policy, GIS references, scientific product/palette choices, annotations, dressing, and dimensions. The program surface consumes a committed snapshot; preview edits do not mutate it.

Shared rules: off-thread heavy work, bounded queues/caches, cancellation on obsolete contexts, stale-response rejection, descriptive loading/error state, and no fabricated observations. Native features need useful Android reachability, not just a shared Rust implementation. Any external JSON export/API addition is versioned; old settings/workspaces/cases remain readable, and old readers reject incompatible newer formats clearly.

## 4. Implementation task cards

Unless marked otherwise, every card starts with **implementation: planned; verification: open**. Implementation and verification are tracked independently. Apply the execution protocol in section 6 to every agent prompt. Save proof under `target/parity-review/<task-id>/` locally and attach durable evidence to the task/PR; do not commit large generated traces or fixtures indiscriminately.

### M0 — Execution foundations

#### M0.1 — Reconcile and maintain the evidence baseline

**Priority:** P0. **Depends on:** none. **Status:** implementation: documented in this roadmap; verification: static audit complete, runtime certification open. **Original references:** ROADMAP_2 §§0, 7, 8; ROADMAP_NEW §§22, 29, 30.

- **Starting evidence:** the capability matrix above, older progress entries, current integration points, and CI workflows.
- **Outcome/build:** maintain a single actionable inventory distinguishing present implementations, incomplete workflows, and missing proof. Before implementation on a newer commit, inspect changed domains and update only affected evidence. Record superseded checklist assertions with reasons rather than deleting historical notes.
- **Interfaces/compatibility:** task IDs and status meanings are stable. A status includes its commit, platform, evidence, and any explicit limitation.
- **Acceptance:** every claimed gap has a code or documented-runtime basis; every task traces to a gap and old-roadmap reference; no pending runtime gate is reported as passed from static inspection.
- **Proof:** the reviewed matrix, baseline SHA, dependency validation, and a dated status ledger entry.

**Agent prompt:** `Refresh M0.1 against the current branch head using section 6. Inspect changed code, correct only affected evidence, and preserve the distinction between implementation and certification.`

**Evidence refresh — 2026-10-01:** implementation began at `ffa22a3`; the concurrent `365369f` change adds an optional Gemini digest provider. Further concurrent commits add zipped shapefile imports (`770a881`, an M4.2 foundation), explain Android update rejection (`47c8676`), and update the configured Gemini model (`bc40699`). These changes are preserved. GIS collections, advanced inspection, and physical-device import certification remain open. That matrix retains its dated audit baseline; the increments below record subsequent delivery.

#### M0.2 — Make domain ownership testable

**Priority:** P0. **Depends on:** M0.1. **Status:** implementation: implemented (initial ownership seams); verification: partial. **Original references:** ROADMAP_2 §7; ROADMAP_NEW §2.1.

- **Starting evidence:** app domain modules, `MapView`, `FieldState`, `RequestBook`, and the existing frame phases.
- **Outcome/build:** establish narrow acquisition and field-request ownership first because later time groups and provenance depend on it. Controllers own domain state and expose explicit actions/results; per-pane state stays per pane. Further storm, GIS, rendering, and output extractions accompany their feature cards instead of becoming a prerequisite to all delivery.
- **Interfaces/compatibility:** preserve the UI → action → synchronization → result flow and existing platform spawner. Replace broad app access in touched domains with explicit inputs and result messages. Add no new substantial algorithm blocks to `app.rs`.
- **Failure/compatibility:** keep refactors behavior-preserving and separate from feature commits. Do not move every field into a new god struct or change settings serialization incidentally.
- **Acceptance:** acquisition and field-request seams are constructible/testable without a full GPU app; event ordering and stale-result handling stay covered; required native/WASM builds and existing regression tests pass. This card closes when those initial seams are established; later domain extractions remain acceptance work in their respective cards.
- **Proof:** domain ownership note, focused behavior tests, and before/after shell responsibilities rather than a lines-of-code claim alone.

**Agent prompt:** `Implement M0.2 incrementally using section 6. Establish narrow acquisition and field-state seams first; preserve behavior and stop after a reviewable domain extraction.`

**Evidence ledger — 2026-10-01, increment 1:** commit `d0da38b` introduces [OverlayAcquisition](crates/hookecho/src/app/acquisition.rs), which owns request generations, health, timeout, background preparation, and delivery. [FieldState](crates/hookecho/src/app/field_state.rs) owns MRMS selection, cadence, readiness, and staging. The app delegates these responsibilities; [ARCHITECTURE.md](ARCHITECTURE.md) documents ownership and the remaining shared-context boundary. Regression tests cover live/archive selection changes, rejected previous-context uploads, refresh cadence, last-good data, and unknown stamps. Settings, workspaces, and command serialization are unchanged.

**Adjacent Sources dock increment:** [Sources](crates/hookecho/src/app/chrome/dock/sources.rs) now supports search, attention filtering, stable row identity, tap/Enter/Space expansion, source/provider/recovery details, and explicit future valid-time labels. Session-only filters do not change acquisition or source enablement. Interaction and narrow-layout tests accompany six reproducible offscreen captures at 240/300/400 px in desktop/touch modes (`gpu_sources_dock_snapshots`; ignored by default because it requires a GPU). Reviewed captures include 240 px touch, 300 px desktop, and 400 px touch. This improves source inspection; retained provenance and lineage in M1.4 remain planned.

**Verification:** fresh Windows `cargo test --workspace` passed **1,963 tests**, with zero failures and 107 explicitly ignored network/GPU tests; `cargo clippy --workspace --all-targets -- -D warnings` passed. The explicit Sources GPU capture test passed. `CARGO_INCREMENTAL=0 RUSTFLAGS='--cfg getrandom_backend="wasm_js"' cargo check --target wasm32-unknown-unknown -p hookecho --lib` passed with existing warnings (one in wxdata, ten in hookecho). Durable reproduction lives in the tracked tests; local review logs/captures are under `target/parity-review/`. Android build/device, Linux native, browser runtime smoke, and full application/soak certification remain open. No configured Android NDK/device tools were detected. Next ready card: M0.3, beginning with manifest integrity and pinned offline inputs.

#### M0.3 — Pin the scientific and visual corpus

**Priority:** P0. **Depends on:** M0.1. **Status:** implementation: implemented (increments 1–5 delivered); verification: partial. **Original references:** ROADMAP_2 §8; ROADMAP_NEW §§K2, K3, 25.

- **Starting evidence:** `golden_events`, scheduled corpus jobs, GPU goldens, headless verifiers, and case manifests.
- **Outcome/build:** add a fixture manifest with exact source object, acquisition time, checksum, format, attribution/license, expected scientific checks, and intended false-positive cases. Provision a small offline fixture subset for PR checks and larger cached inputs for scheduled certification. Keep real archive retrieval tests separate.
- **Interfaces/compatibility:** fixtures resolve immutable object identities instead of dynamically choosing whichever scan precedes a timestamp. Version expected values alongside algorithm revisions; document any intentional expectation update.
- **Failure/compatibility:** checksum mismatch is a failed fixture, not a new golden. Required missing fixtures fail explicitly; unprovisioned device/scheduled suites remain open rather than passing by skip.
- **Acceptance:** repeated offline runs choose identical inputs; detector and derived-product results remain reproducible; tornado, hail, QLCS, weak-echo, clutter, and missing-data controls are represented across the small/large corpus.
- **Proof:** fixture manifest, acquisition script, independent checksum verification, and baseline scientific/visual reports.

**Agent prompt:** `Implement M0.3 using section 6. Extend the existing corpus with pinned identities and an offline subset; retain network contract checks as a separate suite.`

**Evidence ledger — 2026-10-01, increment 1 (`6a8d92a`):** [manifest.json](crates/wxdata/tests/data/corpus/manifest.json) pins ten full NOAA/Unidata Archive II objects and three committed partial inputs (1,263,075 bytes). Collection time, retrieval time, exact source identity, SHA-256, format, attribution/license, transforms, baseline, and consuming checks are recorded. [Provisioning](scripts/corpus/README.md) streams bounded downloads into a verified cache, reproduces unchanged LDM record subsets, and rejects missing/corrupt inputs without rewriting goldens. [Offline scientific tests](crates/wxdata/tests/scientific_corpus.rs) check independent decode repeatability, values, acquisition time, and missing coverage. Existing [historic tests](crates/wxdata/tests/golden_events.rs) now select pinned objects. PR CI verifies the small corpus; scheduled certification caches and verifies full objects separately from provider contracts.

**Scientific expectation correction:** the pinned run exposed first-cut peak goldens predating `8a1d853`'s newest-cut selection. Raw-gate traversal verifies Moore's first/newest peaks at 68.5/70.5 dBZ and Mayfield's at 66.5/68.0 dBZ. Only the selected-cut binned peak expectations changed to 70.4/67.9 dBZ; other scientific thresholds remain intact. A dedicated repeated-cut regression and the provisioning guide preserve this reason. The clear-air control checks the final classification after velocity/ZDR corroboration, retaining a low-CC candidate without promoting it to a debris-tier tornado.

**Verification:** Python provisioner tests passed (4); all 13 file checksums verified, including independent PowerShell SHA-256 checks of the small subset. Actual acquisition into the standard ignored cache and subset reproduction passed. Cached clear-air, repeated-cut, and derived-product repeatability tests passed (3). Moore, Mayfield, deterministic replay, and the combined false-positive controls passed (4 historic tests, with report-dependent tests explicitly excluded from this offline run). Fresh Windows workspace tests passed (1,974; zero failures; 110 ignored), including the new corpus and concurrent ZIP tests; native Clippy and the WASM library check passed (existing browser warnings remain). Platform runtime/device/soak gates remain open.

**Evidence ledger — 2026-10-01, increment 2:** seven original IEM GeoJSON snapshots (261,082 bytes) pin the Moore before/during warnings and all five report windows used by the historic suite. Schema **2** adds mandatory truth identity, exact request times, retrieval time, attribution/license, checksum, and feature counts; old schema readers reject it. Git attributes preserve the original response bytes across platforms. Historic checks consume these snapshots, keeping response-generation time distinct from observation time. [Candidate acquisition](scripts/corpus/capture_truth.py) captures fresh responses into an ignored review directory with old/new checksums and counts; it refuses to overwrite the pinned corpus and never updates the manifest.

**Increment 2 verification:** all eight historic tests passed with locally cached radar and committed truth inputs in the network-restricted environment, including Moore warning verification and the four report-association cases. The four small offline corpus tests passed; independent PowerShell SHA-256 checks verified all seven snapshots. Provisioning verified all 20 inputs; Python tool tests passed (6), and actual candidate acquisition preserved the pinned manifest. Windows workspace tests passed (1,975; zero failures; 110 ignored), and native Clippy passed against the committed application baseline `6a8d92a`. This increment changes test tooling/data rather than the application library; the preceding WASM compilation remains its compatibility evidence. Concurrent app feature edits remain outside this corpus increment.

**Evidence ledger — 2026-10-01, increment 3:** the exact KDMX `2021/12/15/KDMX/KDMX20211215_234124_V06` object adds the December 2021 Iowa QLCS case. Its 14,563,869 bytes remain cached. An eighth original IEM snapshot pins the cross-midnight report window; two original linked NWS KMZ files (17,665 bytes) pin the Knierim/Somers analyzed damage paths. Schema **3** records and checks source intervals, names, vertex counts, and evidence limitations. Rust's production importer and an independent Python XML/ZIP reader verify the geometry and metadata. The source describes Somers as report-based without a field survey; this distinction remains intact.

**QLCS expectation and open science gap:** sparse LSR point reports did not match the two candidates within the existing 8 km tolerance. The new check instead uses NWS damage paths active at the case time and assigns each detection to its nearest path, so one merged detection cannot count for both tornadoes. Knierim retains a Debris detection 1.32 km from its path; Somers retains a distinct Possible candidate 7.87 km from its path. A stronger Somers classification remains open. Track vertices lack individual times, so this checks path proximity rather than exact instantaneous tornado position. Detector thresholds and the spatial tolerance are unchanged. [Reproduction and source links](scripts/corpus/README.md#qlcs-damage-track-baseline) document these limits.

**Increment 3 verification:** all nine historic checks passed offline with explicit cached radar inputs (56.12 seconds). Five offline corpus checks passed, and Python provisioning tests passed (8). Provisioning verified all 24 inputs; independent PowerShell SHA-256 checks verified all eight report/warning snapshots and both KMZ files. Windows workspace tests passed (1,986; zero failures; 111 ignored), and native Clippy passed. These checks exercised the shared working tree at `0ebbd54`, including concurrent detector-baseline edits; those edits are preserved outside this corpus commit. Logs live under `target/parity-review/m0.3/qlcs-*.log`. This increment changes fixtures/test tooling rather than the application library; platform runtime, device, and soak certification remain open.

**Evidence ledger — 2026-10-01, increment 4:** [real radar visual regression](crates/hookecho/src/headless_corpus.rs) routes all three small reflectivity fixtures through the production upload and map renderer. Stable CPU inspector samples provide the color/coverage reference; rendering must preserve values and collection clocks, reproduce identical pixels without another upload, and replace the preceding fixture correctly. Explicit invocation requires the adapter and all inputs. The report starts as running and becomes passed only after all cases succeed; partial failures retain their metrics. CI invokes the test separately on lavapipe and uploads PNG/JSON evidence; live provider jobs omit it.

**Visual evidence:** the Windows RTX 2060/Vulkan run passed (2.92 seconds after compilation), with 16,083 stable color samples, zero color mismatches, 160,292 checked missing-sector pixels, and zero incorrectly filled pixels. All three renders were inspected. [Compact reference captures and report](docs/certification/m0.3/README.md) preserve input/manifest/image hashes, source clocks, adapter/driver, exact method, sample counts, and limitations. These are backend-qualified references, not universal pixel goldens. Linux CI, Android, and browser results remain open.

**Increment 4 verification:** fresh Windows workspace tests passed (1,986; zero failures; 112 ignored) and native Clippy passed. The GPU check was explicitly invoked and passed separately from those ignored defaults. Local logs are `target/parity-review/m0.3/visual-{corpus,workspace}-tests.log` and `visual-clippy.log`. This increment changes test harnesses and CI rather than production rendering; it preserves concurrent detector-backtest edits outside the commit. Full application interaction and sustained-load certification remain open.

**Evidence ledger — 2026-10-02, increment 5:** schema **4** adds the exact KTLX `2020/07/15/KTLX/KTLX20200715_120410_V06` volume and original 32,618-byte `TLX_N0H_2020_07_15_12_04_10` digital HCA product. The manifest now pins 26 inputs, including 12 cached full radar objects and 1,633,691 bytes of committed offline inputs. [Classified clutter checks](crates/wxdata/tests/clutter_corpus.rs) retain the operational classifier's 12:04:10 acquisition and 12:04:54 generation clocks, 432,000 categorical gates, and 2,363 raw-code-20 AP/ground-clutter gates. An independent bounded Python reader verifies the original headers, geometry, and class inventory against the production decoder; metadata changes, missing files, and malformed containers fail explicitly. All 2,363 clutter gate centers round-trip through the original polar mask, without interpolating or dilating classes.

**Clutter scope and baseline:** 1,917 labeled clutter gates lie within the detector's 15–150 km range. The unchanged four-tilt debris/couplet/fusion pipeline produced six raw debris candidates, one couplet, and zero fused circulations, identically on two runs. The false-alarm assertion applies to circulation centroids inside that original clutter mask; it does not declare the entire volume non-tornadic. HCA is operational classification rather than a field survey, and it lacks per-gate observation clocks. Every contributing radar radial precedes the retrospective 12:08:33 analysis time; original archive receipt time remains unknown. The [durable baseline report](docs/certification/m0.3/clutter-reference.json) records the input hashes, algorithm baseline `aa977fe`, clocks, parameters, measurements, and limitations. This certifies the existing fusion pipeline; LLSD objects and columns need their own scientific certification.

**Increment 5 verification:** Windows workspace tests passed **2,038 tests**, with zero failures and 117 explicitly ignored tests; native Clippy passed. All three classified-clutter checks and all nine historic checks passed with verified cached inputs. Python provisioning tests passed (11); provisioning verified all 26 inputs, including independent PowerShell SHA-256 checks of the new volume and HCA. The explicit schema-4 GPU corpus check passed on RTX 2060/Vulkan (4.66 seconds), retaining 16,083 stable color samples and 160,292 missing-sector pixels with zero errors. Local logs are `target/parity-review/m0.3/clutter-*.log`; the original schema-3 captures retain their historical metadata. This increment changes fixtures, test tooling, and CI rather than production libraries; the preceding browser compilation remains compatibility evidence. The workspace checks included concurrent rotation-object/column work at `64e310f`, preserved outside this increment.

**Remaining/next card:** the initial corpus implementation is delivered, including independently labeled clutter. The partial tornado/hail files remain decoding controls rather than full storm or column accuracy evidence. Stronger Somers detection, LLSD scientific calibration, Linux CI results, Android/browser runtime, full application interaction, and sustained-load certification remain open. Next ready card: M1.1, beginning with shared contributor identity and temporal coverage for derived and 3D products.

### M1 — Live-data trust

#### M1.1 — Carry temporal coverage into every radar representation

**Priority:** P0. **Depends on:** M0.2, M0.3. **Status:** implementation: in progress (derived/3D coverage, accepted receipts, native pass history and source sequence receipts); verification: partial. **Original references:** ROADMAP_2 §§1.2, 9, 10; ROADMAP_NEW §§B2, B5, H8.

- **Starting evidence:** `LiveScanState`, per-azimuth times, strict-current 2D masking, `Volume` revisions, derived grids, and 3D playback readiness.
- **Outcome/build:** propagate acquisition intervals, cut/pass identities, contributor revisions, and completeness into derived products and 3D builds. Continuous mode retains older contributions with a mixed-time indication; strict mode excludes them and displays incomplete coverage. Distinguish unobserved chunks from proven radial gaps on a mid-volume join.
- **Interfaces/compatibility:** use shared temporal coverage and frame identity. Invalidate products when an actual contributor changes, including supplemental low-level passes; do not let an older completed build replace a newer accepted revision.
- **Failure/compatibility:** do not fill missing gates with zero or extrapolate time from the client clock. Retain continuous mode as the default and preserve existing 2D display controls.
- **Acceptance:** fixtures cover SAILS/MRLE, VCP change, repeat elevation, late radial, gap fill, reorder, mid-volume join, and missing upper cuts; 2D, derived, and 3D metadata agree with contributors actually shown.
- **Proof:** temporal-policy tests and captures of continuous/strict incomplete volumes.

**Agent prompt:** `Implement M1.1 using section 6. Extend existing scan/revision metadata into derived and 3D outputs, preserving continuous defaults and proving strict-mode coverage.`

**Evidence ledger — 2026-10-02, increment 1:** [shared temporal coverage](crates/wxdata/src/level2/temporal.rs) prepares owned binned inputs and records each moment/elevation's contributing source interval, older-pass rows, excluded rows, unobserved sectors and unknown clocks. Strict preparation uses the existing 2D source-time pass boundary and row mask before integration; continuous inputs and original acquisition clocks remain intact. Unobserved sectors are not called proven transport gaps, and absent upper cuts are not synthesized. The record accompanies local composite, VIL, VIL density, echo-top, MEHS and POSH grids through delivery and display.

**Frame and delivery correction:** [local radar product ownership](crates/hookecho/src/app/radar_products.rs) keys work by the actual decoded scan, accepted revision, policy, selected layers, echo-top threshold and both environmental temperature levels. Runtime scan identity uses a weak reference, so independently acquired panes cannot collide on equal names/local revision counters and evicted gate buffers are not retained. Each completed batch carries its key even on failure. Superseded batches are retired before source-health accounting, and neither a late success nor a late failure can replace the new selection. Shared textures draw only in matching source contexts. This fixes the previous name-only refresh key, single rounded melting-level key and incorrect hail-layer bitmask; enabling POSH alone now requests temperature levels.

**Inspector increment:** the existing dock shows the contributing acquisition interval, continuous/strict policy, retained older rows, exclusions, unobserved sectors and unknown input clocks. It says when matching inputs are pending and does not claim a complete column from the available tilts. Grid frame labels remain separate from acquisition times. The same coverage record serves delivery, display readiness and inspection; settings/workspace formats retain their existing defaults.

**Increment 1 verification:** the final Windows workspace run passed **2,057 tests**, with zero failures and 118 explicitly ignored checks; native Clippy passed. Five scientific temporal controls check the production mask and missing sectors across all six integrations. Frame-selection tests consume the pinned Mayfield partial scan, and source-health tests verify neutral retirement. Narrow-layout checks passed; the explicitly invoked GPU check passed (2.76 seconds) and produced eight continuous/strict, desktop/touch captures at 240/300 px. Visual review caught and corrected truncation/label overlap before repeating the required gates. [Durable captures and reproduction](docs/certification/m1.1/README.md) identify the controlled UI scenario and its limits. The WASM library check passed (35.37 seconds; existing browser warnings remain). An initial existing local HTTP test reset passed its isolated rerun and both subsequent workspace runs; its implementation was preserved. Logs live under `target/parity-review/m1.1/`. Checks exercised the shared tree at `4a84473` with concurrent fusion/backtest work, preserved outside this increment.

**Evidence ledger — 2026-10-02, increment 2:** [standalone 3D ownership](crates/hookecho/src/app/standalone_volume.rs) keys reflectivity builds by site, actual decoded scan, accepted revision, sweep policy, selected beams and palette. The worker remains bounded to one while changing inputs coalesce. Both late successes and late errors are rejected against the actual selection before publishing anything. Grid staging, value range, source coverage and layer summaries commit together. The floating and docked UI hide unmatched GPU content, retain accepted source details under a previous-source label, and offer explicit retry after an empty/failed build rather than resampling every frame. The dock revalidates at its paint boundary because it paints before worker polling.

**Scientific scope:** continuous mode preserves existing grid samples. Strict mode masks owned inputs before both Cartesian interpolation and selected-beam shell construction. Available-layer summaries use policy-retained values and clocks; grid coverage contains only selected contributors. Cached source sweeps and original bin clocks remain intact. Source-time gap inference remains qualified, unknown clocks remain unknown, and available tilts do not establish complete columns. Grid dimensions remain fixed; camera, threshold and clipping controls do not enter the build key.

**Increment 2 verification:** final Windows workspace checks passed **2,074 tests**, with zero failures and 121 explicitly ignored checks; native Clippy passed. Six standalone ownership/scientific tests and three source-control checks cover progressive revisions, policy/palette/beam selection, stale successes/errors, bounded retry/disconnect behavior, unchanged continuous samples, transparent strict sectors and suppression of unmatched GPU callbacks. The pinned Mayfield partial scan supplies runtime-identity cases; synthetic two-tilt inputs supply the spatial/time control. The explicit GPU source-control capture passed (4.14 seconds), producing eight reviewed references with expanded production disclosure at 240/300 px, pending and unknown-clock/error states. [Reproduction, hashes and limits](docs/certification/m1.1/standalone-3d.md) distinguish these controls from full viewport/device certification. The final WASM library check passed (1 minute 45 seconds including build-lock wait; existing browser warnings remain). Two pre-existing detector-test Clippy warnings were corrected without changing their assertions. Local final logs are `target/parity-review/m1.1/volume-*-final-verified.log` and `volume-gpu-final.log`. The checked shared tree includes concurrent backtest/export edits at `b6726cc`, preserved outside this increment.

**Evidence ledger — 2026-10-02, increment 3:** native observed extraction retains each radial's source clock, geometry and scan-local cut index/elevation number. Continuous mode preserves the original gate encoding and every recorded moment cut. Strict mode uses 2D's deduplicated tilt list and newest moment-carrying cut selection, then the existing source-time gap rule on native radial clocks. Another timed selected cut can exclude repeated cuts; all-untimed inputs remain unknown. Rows become transparent before normalization or texture-limit pooling, while retained layer maxima and acquisition intervals exclude them. Older upper cuts remain available if no newer moment cut exists at that height. Native counts are recorded radials, not regular azimuth bins or estimates of absent sectors.

The observed upload key now includes the actual decoded scan through shared weak identity, accepted volume revision, site, name, moment, policy, palette and render controls. Camera motion and the pane's live counter do not stand in for source identity. Inspector and Layers summaries validate the selected source before painting. The Inspector exposes retained/excluded radials, acquisition interval, unknown clocks and individual cut ordinals; it explicitly leaves absent-radial inventory and column completeness unestablished. [Controls, reproduction and limits](docs/certification/m1.1/observed-3d.md) describe the synchronous extraction and controlled coverage UI.

**Increment 3 verification:** final Windows workspace checks passed **2,087 tests**, zero failures and 123 explicitly ignored checks across 26 suites; native Clippy passed. Six native scientific controls, one source-key control and two Inspector controls verify original gates/clocks/geometry, strict selection and masking, moment-specific intervals, unknown clocks, scan revisions and narrow wrapping. Review caught and corrected nearby-angle selection to match 2D's deduplicated tilt list. The explicit GPU capture passed (3.64 seconds), producing eight reviewed continuous/strict/pending/unknown references at 240 px touch and 300 px desktop; independent PowerShell SHA-256 checks verified all copies. The WASM library check passed (3 minutes 44 seconds including build-lock wait; existing warnings remain). Final logs are `target/parity-review/m1.1/observed-{workspace,clippy,wasm}-final.log` and `observed-gpu.log`. Checks exercised the shared tree at `d300df9` with concurrent LLSD, detector settings and scientific-corpus edits, preserved outside this increment. Native extraction remains synchronous; these controls do not establish full viewport, device, performance or soak certification.

**Increment 3 scheduling audit (historical):** the follow-up at `3965578` confirmed that smooth/isosurface tuple keys already carried accepted volume revisions, but lacked decoded source identity and policy. Coverage was absent from cached payloads, old smooth GPU content could remain visible, and only prefetch enforced per-pane admission. That documentation-only audit passed 2,087 workspace tests and native Clippy; `smooth-audit-{workspace,clippy}.log` retains the run. Increment 4 implements this handoff.

**Evidence ledger — 2026-10-02, increment 4:** [map volume ownership](crates/hookecho/src/app/map_volume.rs) captures weak decoded-scan identity, site, frame name, actual accepted revision and sweep policy. Smooth keys also capture palette/contrast, product/environment identity, storm motion and grid quality; isosurface keys capture threshold, smoothing, nested shells and storm motion. Current builders now bin captured scans on native workers. [Covered builders and caches](crates/hookecho/src/loop3d.rs) prepare owned moments and reflectivity quality-mask inputs before product evaluation, auto-range, interpolation and meshing. Geometry-only formulas cannot refill excluded rows. Samples/meshes and coverage travel together; valid empty threshold surfaces retain their contributor metadata.

Complete prefetch obtains the actual cached scan before constructing a continuous key. Current and prefetched builds share bounded admission (two jobs per desktop pane, one per Android/browser pane); there is no pending request queue. Only the latest selection retries admission, while already-running superseded results remain under their original keys for possible playback reuse. Late failures are remembered under that exact context and cannot poison a different selection. Failed/unavailable current selections offer explicit Retry; they do not count as built playback frames. Cache budgets include allocated sample/mesh capacity and coverage, without claiming measured process memory.

Smooth callbacks and isosurface paint require the entire selected key to match. Controls suppress stale range/cell summaries, and the Inspector reports each accepted frame/revision, contributing moment names, acquisition interval, mixed/excluded binned rows, unobserved bins and unknown clocks. Unknown times remain unknown and column completeness stays unestablished. Binned contributor counts are separate from native observed-radial counts. [Reproduction, reviewed captures and limits](docs/certification/m1.1/map-3d.md) document the scientific and UI controls.

**Increment 4 verification:** implementation landed in `b84d816`, with the test-only Clippy correction in `b265b52`. The fresh Windows workspace run passed **2,120 tests**, zero failures and 131 explicitly ignored checks across 27 suites (`--test-threads=4`); native workspace/all-target Clippy passed. The WASM library check passed in 2 minutes 59 seconds including build-lock wait, with existing browser warnings. The explicit GPU source-coverage capture passed in 4.30 seconds, producing ten reviewed continuous/strict/pending/unknown/unavailable references at 240 px touch and 300 px desktop; independent hash and byte-count checks verified every committed copy. Final logs are `target/parity-review/m1.1/map3d-restored-{workspace,clippy,wasm}.log` and `map3d-gpu.log`. Checks compiled the shared tree at `40e8365`, preserving Claude's concurrent detector work. These controls establish builder ownership and coverage-section layout, not full viewport, physical-device, completed-GPU performance or soak certification.

**Evidence ledger — 2026-10-02, increment 5:** the live receiver now tracks raw radial presence
independently of source clocks and exposes an immutable scan-local cut inventory. Each ordinal
retains source elevation/kind, received/unobserved chunks, raw-evidence availability, unique raw
positions, unknown-clock positions, known acquisition bounds and internally bounded unobserved
spans. Equal-angle standard/SAILS/MRLE cuts remain separate source positions. Progress-only
metadata does not turn into zero observed radials; empty raw input is a distinct zero. Gap filling
with an untimed radial resolves its unobserved position while its clock remains unknown. Zero and
out-of-range radial numbers are rejected. A new volume with unavailable VCP metadata does not
inherit the prior VCP number.

One shared wrapping component exposes these details in the workstation Analyst log and the
phone/floating Analyst Mode surface. Chunk inventory labels describe receipt rather than full
radial coverage. The section explicitly describes the live receiver, independently of playback;
known-clock bounds do not date unknown arrivals. [Reproduction and remaining limits](docs/certification/m1.1/raw-acquisition.md)
keep raw observations separate from angular zeros, inferred revisits, persistent pass IDs and
proven transport gaps. Rendering samples, continuous/strict policy and detector methodology are
outside this increment.

**Increment 5 verification:** final Windows workspace checks passed **2,126 tests**, zero failures
and 132 explicitly ignored checks across 27 suites (`--test-threads=4`); native workspace/all-target
Clippy passed. Four acquisition controls and two shared UI controls passed. The explicit GPU helper
passed in 3.05 seconds after compilation; four expanded raw/progress-only references were reviewed
at 240 px touch and 300 px desktop, and separate Python/PowerShell checks verified copied hashes
and byte counts. The final WASM library check passed in 17.26 seconds with existing warnings.
Phone/floating metadata uses the selected theme's foreground colors, including Light. Logs are
`target/parity-review/m1.1/raw-acquisition-{workspace,clippy,wasm}-final-verified.log` and
`raw-acquisition-gpu.log`. The checked tree is based on `2a035e0`, retaining Claude's detector
work unchanged. These are clock/presence, compilation and expanded-section layout controls;
full application interaction, actual Android/browser runtime, source pass identity and accepted
frame/revision propagation remain open.

**Evidence ledger — 2026-10-03, increment 6:** accepted live Updates now retain an immutable raw
acquisition receipt bound to the actual decoded scan, accepted revision and source site. Rejected
metadata-only scans cannot overwrite the prior frame receipt; accepted inputs without usable raw
evidence clear it. Raw position/clock evidence survives receiver advancement, gap fill and reset.
Observed uploads, all six local derived fields, smooth/user-product volumes, isosurfaces and
standalone reflectivity workers carry the same receipt with their original result. Stale deliveries
and receipt-free prefetch builds cannot borrow newer evidence. Smooth/isosurface byte budgets
charge retained summary capacities without pinning decoded gate buffers.

The Inspector's **Selected frame acquisition** exposes the accepted frame/revision and cut
inventory. Product coverage sections and the standalone window expose their own result's source
receipt separately from contributor coverage. Completed/archive and independently reloaded replay
frames retain unavailable raw evidence. In-memory receipt identity is not a persisted pass ID, and
this increment does not add progressive replay storage. [Ownership controls, reproduction and
reviewed references](docs/certification/m1.1/frame-acquisition.md) record these boundaries.

**Increment 6 verification:** final Windows workspace checks passed **2,135 tests**, zero failures
and 136 explicitly ignored checks across 28 suites (`--test-threads=4`); native workspace/all-target
Clippy passed. Nine new ownership/layout controls passed. The explicit GPU helper passed in
2.17 seconds after compilation; four accepted/unavailable references were reviewed at 240 px
touch and 300 px desktop. Independent copy/hash/byte checks verified all captures, and all nine
implementation files still matched their recorded capture-source hashes. The WASM library check
passed with the existing browser warnings. The first workspace build stopped from disk exhaustion;
removing seven stale incremental caches recovered roughly 9 GB and the complete rerun passed.
Logs are `target/parity-review/m1.1/frame-acquisition/{workspace-recovered,clippy,wasm,gpu}.log`;
the initial failure is retained in `workspace.log`. Checks began at `c369d3e` plus this increment;
concurrent detector edits remain outside the parity commit. Compilation and expanded-section
layout are verified; full application interaction, physical Android/browser runtime, persistent
source pass IDs, completed GPU timing and sustained-load certification remain open.

**Evidence ledger — 2026-10-03, increment 7:** both progressive providers now collect native
pass starts/ends before repeated elevations are stitched by azimuth. A source pass key uses
the actual elevation number and positive recorded start-marker clock, scoped to the radar by
the accepted receipt. It does not use angles, local repeat counters, rotation durations or client
clocks. Mid-cut joins and untimed starts remain unanchored. Continuation between incremental
inputs requires consecutive successfully decoded source sequences within the volume; sequence discontinuity, a closed pass
or an elevation transition prevents borrowing an earlier anchor. Repeated metadata/backfill
prefixes cannot silently reopen older passes.

Providers retain a bounded ledger even for delayed decoded input that cannot replace displayed
gates. The next accepted update carries the history; already accepted receipts remain immutable.
Up to 128 source passes are retained with observed positions, unknown clocks, native boundary
markers, known acquisition bounds and internally bounded holes. Retired delayed passes cannot
evict newer history. VCP changes preserve native history in the same source volume; source
volume/site rollover resets it. The summaries hold no gate buffers and their capacities join
existing smooth/isosurface receipt byte charges.

Shared acquisition details expose **Source-marked passes** with wrapping values and a bounded
vertical scroll area. Native markers do not certify a complete pass; unanchored deduplicated
positions are not a count of unknown passes. Unavailable subsequent evidence and retired history
are qualified. Accepted derived/3D outputs retain the source history through their existing
receipts, separately from contributing rows/gates. [Controls, reviewed captures and remaining
limits](docs/certification/m1.1/source-passes.md) document the distinction. Continuous defaults,
scientific strict masks, relay wire identity and detector methodology are unchanged.

**Increment 7 verification:** the fresh Windows workspace run passed **2,149 tests**, zero
failures and 137 explicitly ignored checks across 28 suites (`--test-threads=4`). Native
workspace/all-target Clippy passed with warnings denied, and the WASM library check passed
in 24.01 seconds with existing browser warnings. Ten new CPU controls cover native boundaries,
continuity uncertainty, immutable receipts and narrow layout; the local relay protocol control
also verifies delivered native keys. The explicit GPU helper passed in 2.48 seconds after
compilation. All four expanded history/unavailable references were reviewed at 240 px touch
and 300 px desktop; independent Python/PowerShell copy/hash/byte checks verified the images
and all eight capture-source hashes. Final logs are
`target/parity-review/m1.1/source-passes/{workspace,clippy,wasm,gpu}-current.log`, with exit
codes retained. Earlier queued builds exposed a corrected closure-shadowing error and an old
wxdata artifact without the new metadata; the fresh rebuild and complete checks passed.
Work began at `f7ffef7`; final checks/captures use the shared tree at `3240a67` plus this
increment, preserving Claude's concurrent detector commits. These are source/ownership,
compilation and expanded-section layout controls, not full application/device interaction,
completed-GPU performance or operational soak certification.

**Evidence ledger — 2026-10-03, increment 8:** direct-source initial backfill now checks the
downloaded Start, joined chunk and every requested middle position before assembly erases
source boundaries. An omitted Start/middle position cannot appear to be uninterrupted native
input. A combined prefix/current chunk after failed decoding also takes conservative association
when its positions are not adjacent. An actual adjacent metadata-only Start/current chunk remains
contiguous; failed decoding alone does not invent a missing source position.
Coalesced relay input requires adjacent source sequences; jumps, duplicates, reversals and
overflow do not become apparently continuous input. Its first observed sequence retains an
unknown earlier prefix rather than inventing missing messages. Relay decoding failure clears
the earlier continuation anchor.

Repeated untimed start/end fingerprints also clear a possible old anchor. With no source clock,
deduplication cannot establish that the boundary belongs only to old backfill; later ordinary
radials must not inherit an intervening timed pass's ID across this ambiguity.

Discontinuous assemblies keep valid recorded start IDs while ordinary positions and end
markers remain unanchored. Their starts cannot lend identity to subsequent input; a fresh
native start in contiguous input can restore association. A separate discontinuous-assembly
counter travels with immutable accepted receipts and appears in the expanded Inspector and
all shared derived/3D source rows. It counts inspected assemblies, including non-rendering
input, not missing packets/passes/radials. Continuous samples, newest-radial stitching and
scientific masks are unchanged. [Source controls, captures and conservative limits](docs/certification/m1.1/pass-continuity.md)
document the deliberately conservative relay behavior across its accumulated input.

**Increment 8 verification:** the final serial Windows workspace run passed **2,154 tests**,
zero failures and 137 explicitly ignored checks across 28 suites (`--test-threads=4`). All
five new source-continuity controls passed, including adjacent metadata-only recovery and
repeated untimed boundaries around same/different-elevation timed passes. Native workspace/
all-target Clippy passed with warnings denied; the final WASM check passed with existing
browser warnings. The explicit GPU helper passed in 2.94 seconds after compilation. Four
expanded history/unavailable references at 240 px touch and 300 px desktop were reviewed;
fresh captures matched their reviewed bytes exactly, and independent Python/PowerShell checks
verified every copied hash/size and all eight final capture-source hashes. Final logs are
`target/parity-review/m1.1/pass-continuity/{workspace,clippy,wasm}-serial.log` and `gpu-ready.log`,
with exit codes retained. An intermediate enum-pattern compile error was corrected; overlapping
Windows verification runs hit an executable linker lock, and the final serial rerun passed.
The checked tree is based on `af0d1e6` plus this increment. This establishes conservative source
association and expanded-section layout, leaving exact transport origins/recovery, operational
sessions, contributor association, persisted replay and device/performance gates open.

**Evidence ledger — 2026-10-03, increment 9:** immutable accepted source receipts now carry
separate typed direct-chunk/relay-block sequence origins. A bounded message hole lies strictly
between received positions; actual failed download requests and later recovery spans are separate
receipts. Direct startup retains the identities of failed Start/middle downloads. Concurrent
startup results are inspected in source/request order, so completion timing does not manufacture
reordering/recovery. Incremental iterator errors expose no failed object ID and remain unlocated
errors. No predicted missing position is recorded.

The relay records integrity-validated arrivals, including bytes that fail assembly and duplicate
input that does not change gates. Later accepted updates carry those counters. A mid-volume
join leaves its unrequested prefix unknown. A changed declared upstream starts a fresh assembly
and sequence context; subscription/volume boundaries also reset the ledger. A declared label is
not an emitter instance or independent acquisition identity. This increment does not negotiate
epochs/resume or prove loss from a hole. New sequence evidence does not upgrade the conservative
native association of increment 8 or alter gates, continuous defaults or scientific masks.

Sequence history retains at most 4,096 received/failed-request positions; gap ranges are not
enumerated. Retired positions and outside-scope arrivals remain qualified. Counters cover the
source context, while listed spans cover retained evidence. Receipt/cache capacity charges include
sequence spans and origin labels. VCP changes preserve the same-volume inventory; volume/site
changes reset it. A raw update without sequence metadata keeps prior evidence with an explicit
unavailable-updates count. Accepted frame summaries remain immutable. The Inspector's expanded
**Source sequence receipts** and shared derived/3D source rows expose the independent transport
scope, holes, failed requests, recovery, duplicates, reversals and assembly failures with wrapping
touch-readable values. [Controls, reviewed captures and limits](docs/certification/m1.1/source-sequences.md)
record this workflow separately from native radial presence and contributor coverage.

**Increment 9 verification:** the final serial Windows run passed **2,166 workspace tests**,
zero failures and 139 explicitly ignored checks across 28 suites (`--test-threads=4`). All seven
new source/receipt/transport/wrapping controls passed. Native workspace/all-target Clippy passed
with warnings denied. The WASM library check passed with existing browser warnings. The explicit
GPU helper passed in 3.39 seconds after compilation; six expanded holes/recovery/unavailable
references at 240 px touch and 300 px desktop were reviewed. Fresh final captures matched the
reviewed bytes exactly, and independent Python/PowerShell checks verified image hashes, byte
counts, dimensions and all six final source hashes. Final logs are
`target/parity-review/m1.1/source-sequences/{workspace,clippy,gpu,wasm}-verified.log`, with exit
codes retained. Initial Clippy exposed envelope size and redundant test conversion issues; the
sequence summary is now boxed to keep progressive channel messages compact, and the final
checks passed. The verified shared tree is based on `d751c53` plus this increment, preserving
Claude's concurrent commits. This establishes typed transport receipts and expanded-section
layout, leaving contributor association, operational sessions, persistent replay and
application/device/performance certification open.

**Evidence ledger — 2026-10-03, increment 10:** accepted progressive frames now retain exact
native radial-to-pass associations for rows surviving the provider merge. Recorded metadata keys
(elevation, native position, source clock and status) resolve against bounded pre-stitch evidence;
angles, maximum bucket clocks and local counters cannot lend another row's boundary. Unknown
clocks and ordinary discontinuous inputs remain unestablished. Late non-rendering evidence remains
available to the next accepted frame. Both progressive providers capture the actual merged scan's
index. Missing metadata clears the current index; earlier immutable receipts remain unchanged.
Foreign radar/volume envelopes cannot alter a relay subscription's source scope.

Plain display/product bins retain their actual last radial writer, including copy-on-write live
updates. Default scientific binning remains unrecorded. Native observed sweeps, local derived
products, standalone Reflectivity volumes, smooth/isosurface builds and user-product input receipts
now aggregate policy-retained native pass keys. KDP/dealiased rows and generated outputs cannot
claim one raw writer. All gate values, original clocks, continuous defaults and strict source-time
masks remain unchanged. Native keys describe retained input rows rather than interpolated or
winning output cells, and do not certify complete passes or columns. Receipt and 3D cache capacity
charges include the new bounded metadata.

The dock Inspector and standalone 3D source details list recorded boundaries, retained counts,
unanchored/untimed/unmatched inputs and unavailable input sweeps, with bounded wrapping detail.
Pinned gate readings preserve their original radar and native boundary scope. See
[native contributor controls, review evidence and limitations](docs/certification/m1.1/contributor-passes.md).

**Increment 10 verification:** final serial Windows checks passed **2,177 workspace tests**, zero
failures and 141 explicit ignores across 28 suites, workspace/all-target Clippy with warnings denied,
and the WASM library check with existing browser warnings. The explicit GPU helper passed in
3.14 seconds after compilation. Four 240 px touch / 300 px desktop references were visually
reviewed, and final source/image hashes, byte counts and dimensions verified. The durable
[verification summary](docs/certification/m1.1/contributor-passes-ui/verification.json) and
[capture manifest](docs/certification/m1.1/contributor-passes-ui/captures.json) retain the results.
Initial controls required fixture corrections for absent synthetic reflectivity and an angular
sampling boundary; stale WASM dependency metadata was invalidated without removing native/data
caches. Final local logs are `target/parity-review/m1.1/contributor-passes/{workspace-final,clippy-final,gpu,wasm-final}.log`.
The shared tree preserves Claude's `c308f50` detector commit. Application/device, operational
source continuation, persistent replay and performance gates remain open.

**Evidence ledger — 2026-10-03, increment 11:** direct and relay subscriptions now use a
shared canonical radar/start-clock admission cursor before accumulation, source ledgers or progress.
Reused/wrapped native volume numbers and missing Start markers cannot retain an older accumulator;
a repeated same-volume Start preserves history. Delayed older relay envelopes cannot change the
current upstream or rewind assembly. Same-volume late sequence input stays admissible under existing
continuity rules. Direct refusal ends the mutated iterator and uses the existing polling recovery
path; initial Start scope is validated. Direct progress derives from newly decoded VCP/native
positions, so missing/invalid metadata cannot borrow an older iterator mapper. Completed relay
HTTP arrays reject incompatible radar, start-clock and declared upstream scopes before assembly.

Immutable accepted receipts expose **Source volume admission**: admitted radar/clock, optional native
number, rollovers, declared-label resets and older/foreign/number-conflict refusals. Totals cover the
current subscription, including non-rendering input; reconnect resets them. Wrong/missing scope
becomes unavailable rather than borrowed. Receiver errors are bounded, generation-gated, shown
separately in source health/the Analyst log, and cleared on reconnect. Receipt memory charges include
the retained radar string. This validates declared envelope identity, not payload lineage, complete
radials, emitter epochs, independent failure domains or cross-provider splicing. Values, gate clocks,
strict masks and detector inputs remain unchanged. See
[source admission controls and open gates](docs/certification/m1.1/source-admission.md).

**Increment 11 verification:** shared-tree Windows checks passed **2,186 workspace tests**, zero
failures and 142 explicit ignores across 28 suites; workspace/all-target Clippy with warnings denied;
and fresh WASM library compilation with existing browser warnings. The explicit GPU helper passed
in 3.03 seconds after compilation. Six 240 px touch / 300 px desktop references were visually
reviewed; source/image hashes, byte counts and dimensions were verified. Durable
[verification](docs/certification/m1.1/source-admission-ui/verification.json) and
[capture](docs/certification/m1.1/source-admission-ui/captures.json) manifests retain results.
The stale WASM dependency fingerprint was refreshed without removing native/data caches.
Local logs: `target/parity-review/m1.1/source-admission/{workspace-final,clippy-final,gpu,wasm-final}.log`.
Claude's concurrent detector commit `0018089` remains outside this increment. Whole-application,
physical-device, operational failover, cut/revisit continuation and performance gates stay open.

**Evidence ledger — 2026-10-04, increment 12:** the legacy `MapView.live_progress` marker now
uses receiver admission and monotonic cut selection. `LiveScan::progress` reports acceptance;
rejected older-volume, out-of-bounds or non-finite metadata cannot update inventory, freshness or
recovery. Raw acquisition uses the same gate. Valid late cuts still fill gaps without rewinding the
marker, and duplicates do not renew its animation receipt. Accepted changed metadata, volume/VCP
changes and existing supplemental-cut resets update the marker/time together. The generation/site
filters remain ahead of delivery. Values, source clocks, strict masks and detector inputs remain
unchanged. This fixes the marker seam identified in increment 11; it does not establish unique
native revisit identity. See [progress admission](docs/certification/m1.1/progress-admission.md).

**Increment 12 verification:** final shared-tree Windows checks passed **2,189 workspace tests**,
zero failures and 142 explicit ignores across 28 suites, workspace/all-target Clippy with warnings
denied, and a fresh WASM library check with existing browser warnings. The three new actual-pane/
receiver delivery controls passed. [Durable verification](docs/certification/m1.1/progress-admission/verification.json)
records commands, exits, counts, source hashes and log hashes. The stale WASM package fingerprint
was refreshed without removing native/data caches. Local logs are
`target/parity-review/m1.1/progress-admission/{workspace-final,clippy-final,wasm-final}.log`.
No layout/painter changed. Application/device, native revisit identity, operational failover and
performance gates remain open; Claude's detector work remains outside this increment.

**Remaining/next increment:** canonical cut/revisit continuation and provider-loss/polling/restoration
controls precede replacing inferred strict boundaries. Arrival-ordered relay CutTracker IDs still
require stronger native chronology and operational lifecycle evidence. Source message holes
and later-arrival receipts do not yet map to individual radial boundaries or establish loss.
Missing cut metadata can leave raw receipts unavailable; expired associations remain unavailable.
Emitter epoch/resume negotiation and progressive replay storage remain open. Whole-scan invalidation
remains conservative; contributor optimization, full viewport interaction, Android/browser runtime,
completed GPU timing and sustained-load certification remain open. Independent local-product
textures per pane remain M5.1.

**Next agent handoff:** establish deterministic provider-loss/polling/preferred-tier restoration
through the application lifecycle. [M1.3 increment 1](docs/certification/m1.3/live-session.md) closes
the global retry/context and generation seams identified here, plus idle relay retirement.
Retain admitted marker/receipt ownership, then inspect `LiveScan::capture_acquisition`, `Volume::apply_live_captured`,
`wxdata::live_pass`, `wxdata::live_sequence`, direct stream lifecycle and relay merge controls.
Preserve immutable accepted receipts and plain-moment row writers across every result key.
Completed/archive/reloaded replay inputs still have unavailable progressive associations.
Keep native boundary identities independent of the delivered source-time strict masks and detector
inputs. Build on the canonical source admission guard; establish reconnect/cut continuation with repeated-cut,
SAILS/MRLE, VCP-change, reordering, gap-fill and mid-volume join controls. Source message holes,
actual failed requests, recovered bytes, unobserved radials and proven loss remain distinct.
Do not infer scientific completeness from angular zeros or source sequence gaps. Keep device and
performance gates partial until measured. Tornado detection work stays with Claude under the
current agent assignment.

#### M1.2 — Trace receipt through completed rendering

**Priority:** P0. **Depends on:** M0.2, M1.1. **Original references:** ROADMAP_2 §§1.5, 14; ROADMAP_NEW §§B3, O1, O4.

- **Starting evidence:** `LiveQueueTimings`, the monotonic receipt carried into 2D uploads, CPU frame telemetry, and profiling feature.
- **Outcome/build:** correlate receipt, decode, UI acceptance, submission, GPU completion when supported, and observable presentation with frame identity. Report p50/p95, sample count, dropped/coalesced updates, source mode, and measurement stage. Profile copying, rebinning, allocation, and 3D full uploads before optimizing their dominant cost.
- **Interfaces/compatibility:** bounded local timing samples; separate CPU, GPU, and presentation clocks. Use submission callbacks/timestamp capabilities where supported. Where compositor presentation is unavailable, name the proxy and use external frame observations for visible-latency certification.
- **Failure/compatibility:** exclude unsupported/unknown samples rather than treating them as zero. Completed-volume providers without comparable receipt stamps remain a separate population. Do not add tracking or mandatory profiling servers.
- **Acceptance:** one sample identifies the revision that was drawn; queue writes are never labeled visible latency; a changed 2D sector avoids unchanged texture rows; desktop/Android benchmark reports use the stage definitions in section 5.
- **Proof:** correlated traces, documented observation method, allocation/upload profiles, and platform capability coverage.

**Agent prompt:** `Implement M1.2 using section 6. Extend the existing local timing path and label every measured stage accurately before changing performance-critical code.`

**Status:** implementation: in progress (GPU-completion stage, increment 1); verification: partial (software GPU only).

**Evidence ledger — 2026-10-06, increment 1 (receipt → GPU done):**

- *Delivered:* `render::LiveQueueTimings` gains a second, separately named stage beside "receipt → GPU queue writes": **receipt → GPU finished the frame that drew it**. A live (non-LUT-only) radar upload's receipt clock is kept per pane, and at that pane's next prepare — after the drawing frame was submitted — `Queue::on_submitted_work_done` records when the device reports the work done. The app requests that next frame immediately; a mark that could only be registered more than 100 ms later is excluded and counted as "not observed" rather than recorded as a long latency. The Log dock shows p50/p95/sample count for both stages (plus the not-observed count); source-health details add "Receipt → GPU done". Display scan-out/presentation is still not measured and is named as such in the UI text.
- *Commands and results:* `gpu_live_upload_reports_queue_and_completion_stages_in_order` (GPU suite, llvmpipe): one queue sample at upload, no completion before the drawing frame is followed, exactly one completion after it, completion ≥ queue (one run: 4.6 ms to queue, 20.6 ms to GPU done on the CPU rasterizer), no sample for a LUT-only recolour, and a 150 ms-late follow-up excluded and counted. Registry unit test keeps the two stages' labels distinct. clippy `-D warnings` clean; workspace tests 2,342 passed, 0 failed; GPU suite 37 passed.
- *Open:* presentation (compositor) latency, decode/acceptance stage correlation with frame identity in one trace, 3D upload/build profiling, and any desktop or Android hardware measurement.

#### M1.3 — Prove source failover under severe-weather load

**Priority:** P0. **Depends on:** M0.3, M1.1, M1.2. **Original references:** ROADMAP_2 §§1.3, 3; ROADMAP_NEW §B6.

- **Starting evidence:** provider capabilities, health/arbiter state, continuation rules, optional ingest relay, and completed-volume fallback.
- **Outcome/build:** exercise and repair the existing failover path through mid-volume transport loss, stale-but-responsive upstreams, conflicting radial identities, missing chunks, source restart, and restoration of the preferred tier. Display source, reason, retained-data age, and any loss of temporal resolution.
- **Interfaces/compatibility:** retain canonical radial/cut identity and safe continuation refusal. Extend health reporting with upstream failure-domain identity so two transports sharing one source are not presented as independent redundancy.
- **Failure/compatibility:** incompatible volume/cut data is not spliced together. Backoff/retry are bounded; foreground resume refreshes health; core direct-source operation remains available without a relay.
- **Acceptance:** no displayed acquisition reversal, indefinite silent freeze, or unannounced fallback downgrade; deterministic fault scenarios recover; a sustained live relay/direct-source session produces evidence distinct from simulator results.
- **Proof:** fault scenario manifest, transition logs, progressive/completed fallback captures, and live-session report.

**Agent prompt:** `Implement M1.3 using section 6. Extend existing failover tests and repair demonstrated continuity/recovery gaps without replacing the provider architecture.`

**Foundation evidence — 2026-10-03 (M1.1 increment 11):** shared source admission now prevents
within-subscription older/foreign/conflicting declared volumes from rewinding assembly and ledgers;
recovery errors reach source health/the Analyst log. Local WebSocket and HTTP refusal controls plus
frame-owned receipts are documented in [source admission](docs/certification/m1.1/source-admission.md).
This is partial lifecycle evidence. Cross-provider cut continuation, preferred-tier restoration,
independent failure domains, whole-application transitions and operational/soak gates remain open.

**Increment 1 — 2026-10-04, subscription lifecycle:** live retry scope now includes pane, radar,
provider and selected relay endpoint. A changed context starts immediately even after End; a
same-context failure retains the monotonic 60-second interval. Every attempt receives a fresh local
cancellation generation. End events retire only their owning subscription and update only the
original radar receiver. Background/pause resumes can refresh without a failure delay. Selected
endpoint changes retire the old stream, equivalent normalized endpoints avoid churn, and the
native relay checks cancellation during idle reads without discarding its pending frame future.
Source clocks, frozen acquisition receipts and detector inputs remain unchanged.

Six controller/actual-receiver controls cover retries, replacement, endpoint changes, intentional
resume, loss/poll/restoration, late End, and reselected/removed panes. The real local WebSocket
delivery control now awaits graceful idle cancellation instead of aborting its task. See the
[lifecycle report](docs/certification/m1.3/live-session.md) and
[verification manifest](docs/certification/m1.3/live-session/verification.json).
These are deterministic controls, not whole-application or operational severe-weather certification.

**Increment 1 verification:** final shared-tree Windows checks passed **2,195 workspace tests**,
zero failures and 142 explicit ignores across 28 suites, workspace/all-target Clippy with warnings
denied, and the WASM library check with existing browser warnings (wxdata 1, hookecho 10). The
manifest retains seven lifecycle controls, 18 source hashes, final log hashes, and prior attempt
results. Native/data caches were preserved. No painter/layout or detector input changed.

**Increment 2 — 2026-10-04, observed restoration:** the provider monitor now distinguishes advancing
source-clock observations from callback successes and preserves freshness high-water marks across
duplicates/replays. Failure resets the advancing streak, including failures between UI frames.
Unexpected clean End while wanted is transport loss; intentional stop does not add failures, and
cancelled reconnect backoff returns promptly. Arbiter failback uses real observation counters instead
of frame evaluations, requires a primary no older than backup, and excludes failed/stale backup
candidates. Completed fallback also waits for fresh recovery evidence. Empty startup health gets a
bounded 180-second monotonic observation window; known stale data can degrade immediately.

Manager health samples are coherent, and only actual selected-tier changes replace the visible
transition. Degraded source health lists primary recovery even without a relay. Twelve new controls
include a manager/controller/actual-receiver primary-loss/relay-loss/completed-floor/flap/preferred-
restoration scenario, with 1,000 repeated evaluations proving weak recovery cannot trigger a switch.
Late Ends and older volumes are refused, source clocks are retained and accepted receipts remain
immutable. See [observed restoration](docs/certification/m1.3/restoration.md), the
[emitted fault trace](docs/certification/m1.3/restoration/fault-transitions.json), and
[verification](docs/certification/m1.3/restoration/verification.json). This closes demonstrated
decision/ownership seams; it does not certify a complete application/rendering or operational session.

Final Windows workspace verification passed **2,210 tests** across 28 suites, with zero failures
and 142 explicitly ignored checks. All twelve new restoration controls, strict workspace Clippy
and WASM compilation passed; existing browser warnings remain. The manifest hashes fourteen source
files and the six-event trace. Concurrent detection/touch work is excluded from this increment.

**Increment 3 — 2026-10-04, declared acquisition dependencies:** primary and completed adapters
declare their acquisition service families; the optional relay `/provider` endpoint adds schema-1
input-mode and bounded non-secret domain IDs configured separately from deployment/source labels.
The bundled binary declares replay or idle, reflecting its actual adapter support. Sources and
retained diagnostics disclose shared/different/unknown dependencies, metadata evidence and check
time, and replay/idle availability. Different IDs never certify independent redundancy.

Optional inspection has two-second request/body and monitor limits, a 16 KiB body limit, no
redirects and prompt cancellation. Old/malformed/oversized/unresponsive metadata remains unknown
without blocking data subscription or counting as transport failure. Failed refresh replaces current
metadata, retaining at most one dated declaration under a previous-source label. Metadata changes
no radar freshness, recovery counters, radial identity, accepted receipts or failover decisions.
See [declaration contract and limits](docs/certification/m1.3/upstream-domains.md).

**Increment 3 verification:** final Windows workspace checks passed **2,228 tests** across 28
suites, with zero failures and 144 explicitly ignored checks. All ten new declaration/HTTP/
monitor/diagnostics controls passed, as did strict workspace/all-target Clippy and WASM compilation
(existing browser warnings remain). The explicitly invoked GPU control passed in 4.47 seconds;
all eight captures were inspected at 240/300 px, including long IDs, unknown/different declarations,
replay input and failed refresh with dated history. The [verification manifest](docs/certification/m1.3/upstream-domains/verification.json)
retains fourteen source hashes, log/capture hashes and four production diagnostic rows. These are
deterministic and loopback controls, not full application or operational certification. Concurrent
detection/backtest artifacts are excluded from this increment.

**Next agent handoff:** exercise complete app delivery/rendering and operational direct/relay
fault sessions under load. Treat HTTP declarations as configuration evidence, not stream-bound
upstream attestation; independently validate deployment topology. Keep M1.3 partial until actual
failure-domain identity, cross-provider cut continuation, live-session evidence, sustained load,
thresholds benchmarked against cadence and applicable platform gates have evidence. Keep tornado
detection with Claude.


#### M1.4 — Complete retained provenance and scientific lineage

**Priority:** P0. **Depends on:** M0.2, M1.1. **Status:** implementation: in progress (source clocks; Tornado ID lineage); verification: partial. **Original references:** ROADMAP_2 §§9, 10; ROADMAP_NEW §§A1, A2, N1, N2.

- **Starting evidence:** `DataStamp`/`Stamped`, field descriptors, probe rows, source health, observed/forecast/derived flags, and current archive receipt limitations.
- **Outcome/build:** retain provenance with cached/archived payloads; migrate observations, warnings, detections, and trail/derived outputs to common metadata access. Record algorithm version, input clocks, temporal coverage, native grid/units, processing/interpolation, and input quality. Inspectors, captions, exports, and diagnostics consume this same record.
- **Interfaces/compatibility:** add optional lineage and timing fields without repurposing existing stamp meanings. Do not substitute fetch-success time for unknown original archive receipt. Transformations retain source identity and add their own processing description.
- **Failure/compatibility:** missing metadata produces an explicit unavailable field. Unknown quality is not upgraded to good. Last-good data retains its original valid time after retries or cache reads.
- **Acceptance:** a coverage inventory accounts for every major displayed layer; archive, live, forecast, observation, and derived fixtures expose correct clocks and offsets; round trips preserve unknown values and old settings/cases load.
- **Proof:** layer provenance inventory, constructor/serialization tests, and matched inspector/export examples.

**Agent prompt:** `Implement M1.4 using section 6. Finish retained stamps and dependency lineage through the shared inspector/export path; never invent an unavailable source clock.`

**Source-clock foundation — 2026-10-01 (`7079846`):** the Level III decoder retains checked PDB data-start, RPG generation, and DVL/EET volume-end clocks separately. Supplemental elevation starts keep their source meaning; free-text generation is not labeled acquisition. Gridded DVL/EET/HHC/N0B products and SCIT history/forecast origins use the decoded data start, without S3-name or client-clock substitution. An undated grid is unavailable. This corrects a production issue discovered while pairing M0.3's candidate HCA clutter labels with their Archive II volume. Six existing real products, one-based epoch/midnight boundaries, invalid day/second encodings, and independent Python header interpretation verify the decoder. Serialized clocks preserve unknown values and their distinct roles. Shared grid lineage, persisted archive receipts, inspectors/exports, and the remainder of this parent card remain open.

**Foundation verification:** Windows workspace tests passed (2,019; zero failures; 115 explicitly ignored), and workspace/all-target Clippy passed. Five source-clock tests, including JSON unknown/role preservation, and eight existing MetPy value/geometry goldens passed separately. The WASM library check initially exposed a native-only Rayon import in the concurrent LLSD shear feature (`08df4ae`). The row calculation now runs sequentially on WASM and retains parallel ordered collection on native; the repeated workspace/Clippy checks and full HookEcho WASM library check passed. Existing browser warnings remain. This is compilation/unit evidence, with browser runtime, device, and sustained-load certification still open. Review logs are `target/parity-review/m0.3/clock-*.log`. CHANGELOG.md now records the implemented parity foundations and other committed branch features.

**Evidence ledger — 2026-10-02, Tornado ID lineage:** Tornado ID is the detection most readers act on, and it has two pipelines. The fused one is the default. The original stands in while the fused verdict for a volume computes and on light loop frames, and its markers look the same. Before this increment, nothing said which pipeline made a marker, and neither export (the local API snapshot or the analysis archive) included Tornado ID at all.

[`wxdata::detection_lineage`](crates/wxdata/src/detection_lineage.rs) now records each volume's verdicts:
- the pipeline, and why the original stood in when it did
- every stage's algorithm version (`tornado_id::ALGORITHM_VERSION` is new)
- the site, volume name and nominal time
- the acquisition coverage of the sweeps the rotation evidence was measured on

The coverage comes from the sweeps' own radial clocks, gathered through M1.1's `level2::temporal::prepare` under the continuous policy the detectors read with. The fused worker computes it from the sweeps it already owns, after the columns, without copying them.

Marker and merged-detection hovers show the lineage in place of a fixed caption that was wrong for the fused pipeline. The local API's `detections.tornado_id`, `provenance.json`'s `tornado_id` and new `detections.csv` rows carry the same record. The CSV rows give the tier, pipeline and input interval; the CSV's new columns are appended, so existing columns keep their positions.

Unknown values stay unknown:
- A row with data and no clock is counted as unknown; an empty untimed row is counted as unobserved, not as a gap.
- Inputs that were not recorded are blank or `null`, never the volume time.
- A malformed sweep yields no record.

On the pinned Moore 2013 volume, the fused verdict's four input tilts were scanned 20:12:47–20:14:20Z, against a nominal 20:12:29Z. The record keeps the two clocks apart. The 1.8° sweeps have 132 unobserved rows. [Reviewed capture, lineage JSON, hashes and limits](docs/certification/m1.4/README.md) document this.

**Tornado ID lineage verification:** Windows `cargo test --workspace` passed **2,117 tests**, with zero failures and 130 explicitly ignored. That includes two lineage unit tests (input clocks, unknown/unobserved rows, missing inputs, malformed sweeps, JSON) and a CSV export test (fused interval, blank original interval). `cargo clippy --workspace --all-targets -- -D warnings` passed. The gate first failed on four `chunks_exact` lints in increment 4's `loop3d.rs` tests; those were corrected mechanically, with their assertions unchanged. The WASM library check passed (existing warnings: one in wxdata, ten in hookecho). The explicitly invoked GPU capture on cached Moore passed on RTX 2060/Vulkan. Logs are `target/parity-review/m1.4/lineage-{workspace,clippy,wasm}.log`.

**Follow-ups (same day):** the pinned circulation card (the only place a phone can read the lineage) and the Cell dock's Threat section now show it. The original pipeline now records its input clocks too. Its couplets read the same four lowest velocity and reflectivity tilts, captured from the sweeps `detect_couplets` already owns. The record says "not recorded" only when the couplets were computed for another volume.

**Evidence ledger — 2026-10-09, debris input clocks:** `DetectionLineage` gains `debris_inputs` beside `inputs`: the (reflectivity, CC) pairs the debris signatures were read from and the lowest ZDR sweep [`tds::apply_zdr`](crates/wxdata/src/tds.rs) discounts by ([`debris_input_coverage`](crates/wxdata/src/detection_lineage.rs)), gathered in `compute_tds_uncached` under the same continuous policy and cached on the volume key with the debris hits. Both pipelines read that one debris record for the volume's newest pass, so either verdict's lineage carries it. Hovers, the pinned card and the Cell dock now list "Rotation inputs…" and "Debris inputs…" separately. The local API and `provenance.json` add `debris_inputs`, `null` when not recorded (including a volume without dual-pol). `detections.csv` appends `debris_inputs_start_utc,debris_inputs_end_utc`; debris and rotation rows now fill the existing interval columns from their own sweeps when those are of the exported volume. Existing columns keep their positions. On Moore 2013 the debris inputs span 20:12:29–20:14:20Z. At the three split-cut tilts their CC is from the surveillance cut and their reflectivity from the Doppler cut, one antenna rotation (~17 s) later, because binning takes each moment's newest cut. This is recorded and not changed here: re-pairing is a detection change that needs a backtest. [Capture, JSON, hashes](docs/certification/m1.4/README.md). Tests: `debris_inputs_are_their_own_sweeps_with_the_lowest_zdr` (pairs plus only the lowest ZDR, unknown rows counted, no dual-pol gives no record), the two existing lineage tests (now naming each evidence), `tornado_rows_name_their_pipeline_and_input_scan_times` (the debris columns, blank when not recorded). Windows: `cargo test --workspace` 2,499 passed, 0 failed, 182 ignored; `cargo clippy --workspace --all-targets -- -D warnings` clean; the HookEcho WASM library check passed (existing warnings: one in wxdata, ten in hookecho); the GPU capture on cached Moore passed (RTX 2060/Vulkan). Logs: `target/parity-review/m1.4/debris-{workspace,clippy,wasm,gpu}.log`.

Remaining open:
- earlier low-level passes' own debris inputs (the fused tracker reads them; the record covers the newest pass)
- the same record in the rotation and debris layers' own hovers, and for warnings and observations
- persisted archive receipts
- Android/browser runtime and full application interaction

### M2 — Storm-centered operator workflows

#### M2.1 — Introduce persistent storm identity and history

**Priority:** P0. **Depends on:** M0.2, M0.3, M1.4. **Status:** implementation: in progress (domain history owner); verification: partial. **Original references:** ROADMAP_2 §2.1; ROADMAP_NEW §§R3, R4, R5.

- **Starting evidence:** SCIT histories, spatial warning/ProbSevere associations, competing circulation assignment, and detection track modules.
- **Outcome/build:** add a domain storm-history owner with stable local IDs and timestamped source references. Associate by spatial/temporal evidence and plausible motion while retaining alternatives, confidence reasons, provider-ID changes, and split/merge lineage. Manual objects may remain unassociated.
- **Interfaces/compatibility:** associations refer to original observations, not copied flattened storm attributes. Record time/distance tests and the motion source used; calibrate association limits from pinned fixtures and version those limits with the model.
- **Failure/compatibility:** stale sources lose confidence; ambiguous signatures remain ambiguous. Provider IDs are not globally unique storm IDs, and incomplete data does not force a merge.
- **Acceptance:** adjacent storms, crossings, polygon holes, competing signatures, missing updates, recycled IDs, splits/merges, and backward archive seeks produce deterministic histories; same inputs replay to the same identities.
- **Proof:** association/lineage fixture reports and inspectable explanations for confirmed, tentative, and unmatched objects.

**Agent prompt:** `Implement M2.1 using section 6. Build persistent history around the existing spatial associations, preserving every source object and uncertain alternative.`

**Evidence ledger — 2026-10-05, increment 1 (domain seam):** [`wxdata::storm_history`](crates/wxdata/src/storm_history.rs) is the storm-history owner: one history per radar, fed volume by volume with source observations (SCIT cells, locally tracked cells) by reference (source, site, provider ID, time, position, declared motion), never as copied attributes. Each storm has a stable local ID; each link records its distance from the predicted position, time gap, the motion that predicted it (the storm's own track, the provider's declared motion, or none), competing storms within reach and a confirmed/tentative confidence. Provider IDs are continuity evidence, not identities: an ID change is noted, and a reused ID beyond reach starts a storm noted as recycled. Splits and merges are recorded as lineage; ambiguous links stay tentative with their alternative; storms unseen past the gap limit close. Candidates are ordered by distance, storm ID and source order, and an update older than the history restarts it, so the same inputs replay to the same identities. The limits are a first guess for 5-minute volumes, versioned `storm-history-1`. Unit tests (8) cover motion and replay, crossings, a missed volume and a long gap, a recycled ID, split and merge lineage, an ambiguous midpoint, and a backward seek; wxdata Clippy and the WASM library check passed. Not yet wired to the Storms dock or Inspector; limits not yet calibrated on pinned fixtures; polygon-hole and competing-signature cases belong with the spatial associations it will absorb.

**Evidence ledger — 2026-10-05, increment 2 (live SCIT wiring):** the Storms dock owns the active radar's history ([`StormIdentity`](crates/hookecho/src/app/chrome/dock/storms.rs)) and feeds it once per SCIT table, open or not, so identities survive the window being closed; another radar starts a new history, and an archive (where SCIT is not kept) feeds nothing. SCIT's declared motion (bearing toward, knots) is the provider motion until a storm's own track takes over. The table's ID column shows the stable ID beside SCIT's recycled one (`O7 #12`), and the Cell window's Storm section adds a History line: how long and over how many scans the storm has been tracked, the SCIT IDs it has carried, a split it came from, and whether its latest link is tentative. Storm IDs count from 1. A unit test feeds three SCIT tables with an ID change and a repeat, and checks the motion conversion and the line a person reads. hookecho storms/cell tests (30), the history tests (8), Clippy and the WASM library check passed. Open: the Inspector's per-link evidence view, archive replay (needs archived SCIT or local cells), the spatial associations joining the history, and calibrating the limits on pinned fixtures.

**Evidence ledger — 2026-10-06, increment 3 (linked evidence over time):** [`wxdata::storm_evidence`](crates/wxdata/src/storm_evidence.rs) records, beside each stable storm, what it has been linked to scan by scan: NWS warnings (by VTEC event key, valid time from the product's effective/issued time; watches and other alerts are not warnings and are not recorded), ProbSevere objects (by their own object ID; a feature without one says "(no ID)"), fused tornado detections, and SCIT's hail attributes. Each link says how it was made — the storm's position inside a polygon and outside its holes (coverage: one warning covering two storms is recorded on both, not as a shared identity), the storm nearest a detection's centre or any member within 10 km, or the storm's own cell — and a detection within 1 km of as close to another storm, in reach or just past it, is recorded on both as ambiguous naming the other, never attributed. One source object over several scans is one track with its first and last scan and a sample per scan, so a probability or detection tier reads as it changed (`ProbSevere object 4321 — Tor 40% → Tor 62% (… 3 scans; covers it)`); hail stays one track across a SCIT ID change. Detections have no identity of their own across volumes, so they form one track per storm with each sample's source volume time. The same scan re-recorded (a detector finishing late, a warning issued between scans) replaces its samples instead of adding them, keyed on the inputs it was recorded from, and an older scan restarts the record, so a backward seek replays to the same evidence. The Cell window's spatial association card now uses the same point rule (`point_links`), and a test pins both to the same limits (versioned `storm-evidence-1`). The Cell window's Storm section lists the linked tracks under its History line. Tests: wxdata `storm_evidence` (5: polygon holes and multi-storm coverage, nearest vs tie vs out of reach, a storm just past reach keeping its neighbour ambiguous and members deciding nearest, one track with changes and same-scan replacement, backward-seek replay), ProbSevere object ID, hookecho `a_storm_keeps_what_it_was_linked_to_across_scans_and_a_seek_back` (warning vs watch, ProbSevere change, hail across a SCIT ID change, repeat inputs, replay) and the existing association tests (4) unchanged on the shared rule; full workspace 2,351 passed, Clippy `-D warnings`, the WASM library check. Desktop/Android share the dock code; no physical-device evidence for this increment. Open: limits still a first guess (not calibrated on pinned fixtures); archive replay needs archived SCIT or local cells; manual storm motion, ETA/closest approach per storm entity and split/merge display in the Storms table (next increment).

**Evidence ledger — 2026-10-06, increment 4 (calibrated on two real days; association fixed for dense fields):** [scit_history](crates/wxdata/tests/scit_history.rs) replays every pinned NST product of two days from the Unidata Level 3 archive — KTLX 2024-05-06 (47 products, up to 15 cells at once) and KDMX 2024-05-21 14–24 UTC (95 products, 2,793 cell observations, up to 55 at once: the Iowa outbreak with the Greenfield tornado) — and measures, against SCIT's own continuity (no hand-labelled truth exists), how often a SCIT ID continued in the next product within what 40 m/s covers stays one storm, and how often the history makes a contradiction: gives a storm a cell under a new ID while the cell with its old ID is still within plausible reach. **With `storm-history-1` the dense day failed badly:** at the 5 km base reach only 78.5% of SCIT's continued cells stayed one storm, with 112 contradictions and 909 "storms" from 2,793 observations; the 2 km–12 km sweep never passed 92%. Two causes, both fixed in `storm-history-2`: links were made nearest-first, so in a dense field a neighbour that landed nearer a storm's prediction took it although the storm's own cell (same SCIT ID) was right there; and a new cell with no track of its own was predicted stationary, so a 25 m/s storm was out of reach of its first position after one volume. Now the provider's own continuity links first — the same ID from the same source and site, no farther from the storm's last position than a 40 m/s storm moves in the gap (`provider_speed_ms`, new) — then the rest nearest-first; and a storm without its own motion is also measured from the observation carried back along the motion its provider now declares. An ID farther than that is still refused as recycled. Result at the limits in use (base 5 km): KTLX 443/443 kept together, 0 contradictions, 60 storms; KDMX 2,080/2,080, 0 contradictions, 636 storms (from 909), 188 tentative links. Caveat recorded with the test: the agreement figure now partly mirrors the new rule (both trust a same-ID cell within plausible speed), so the contradiction count is the independent signal; a SCIT ID recycled onto a different cell within 40 m/s of the old one would be continued — not detectable without labelled truth. New regressions `a_storms_own_provider_id_wins_over_a_nearer_neighbour` and `a_fast_new_cell_is_found_by_its_declared_motion` (both fail on `storm-history-1`); the eight existing history tests unchanged; full workspace 2,399 passed, Clippy `-D warnings`, the WASM library check.

#### M2.2 — Unify storm selection, trends, and tool entry

**Priority:** P0. **Depends on:** M2.1. **Original references:** ROADMAP_2 §§2.5, 12; ROADMAP_NEW §§J3, J5, J6.

- **Starting evidence:** Storms dock filtering/navigation, Cell analysis, follow actions, trend samples, gate/layer probes, and 3D/cross-section actions.
- **Outcome/build:** map and table selection target one selected storm identity. Show source-timed attributes, trend gaps, warnings, ProbSevere, rotation/TDS/hail evidence, automatic/manual motion, and projections in one responsive analysis surface. Provide direct open/follow/3D/cross-section actions and preserve selection through supported continuity.
- **Interfaces/compatibility:** common selection actions and a view model derived from the history owner. Existing keyboard/global focus ownership remains shared with card buttons and touch controls.
- **Failure/compatibility:** distinguish missing, stale, and changed-identity attributes. Do not smooth trends across unavailable periods or let a newly adjacent cell silently inherit selection.
- **Acceptance:** selecting from either entry point shows the same entity and clocks; history updates do not steal focus; phone, tablet, narrow desktop docks, and keyboard navigation reach all critical actions.
- **Proof:** operator walkthrough, selection/focus regressions, and desktop/Android captures.

**Agent prompt:** `Implement M2.2 using section 6. Connect map and table selection to the same storm history and complete the responsive analysis workflow.`

**Evidence ledger — 2026-10-06, increment 1 (selection, follow and trends by storm, not SCIT ID):** SCIT recycles and changes cell IDs, and the selected storm, the followed storm and every trend were keyed by that ID: a storm renamed `O7 → K3` lost its selection, its trend and (unless the nearest-cell guess caught it) its follow, and a different storm later given `O7` silently inherited all three — the "newly adjacent cell inherits selection" failure this milestone names. The storm history is now fed when the SCIT table arrives (before following and selection read it), and [`StormIdentity::resolve`](crates/hookecho/src/app/chrome/dock/storms.rs) answers where a pick (its SCIT ID at its table's time) is now: in the table as whichever cell it is now, known but not in this table, or unknown to the history (another radar, a restarted history), in which case only the ID is matched, as before. `selected_storm` (the map ring, Inspector, Cell window, Storms-table highlight) follows that answer; a storm that has left the table is shown as of its last scan and says so, and its old ID's row is not highlighted. Follow adopts the storm's new cell through a rename, holds in place through one missed scan while the history keeps the storm open, and ends when the history closes it, never jumping to a nearby different storm. Trends (Cell window, Storms table sparklines, Storm Digest) are the storm's: the sample each of its observations recorded under the ID it had then, with samples from before the history began taken from its current ID; per-ID samples now outlive the ID leaving the table by two hours so a rename keeps them. `update_follow` and the selection resolution moved out of `app.rs` (ceiling lowered). Tests: `a_pick_follows_its_storm_not_the_scit_id_it_was_picked_by` (rename, recycled ID, missed scan kept open, unknown pick), `a_trend_is_its_storms_through_a_rename_and_never_a_recycled_ids` (fails on per-ID trends: the new `O7` would read the old `O7`'s samples); full workspace 2,353 passed, Clippy `-D warnings`, the WASM library check. No desktop/Android walkthrough or captures yet. Open: the Compare set in the Cell window is still a list of SCIT IDs; a storm's manual motion is not yet attached to its identity; operator walkthrough and focus regressions on phone/tablet.

**Evidence ledger — 2026-10-06, increment 2 (manual motion on the storm):** a manual track started from a storm ("Track manually") now keeps its source reference — the SCIT cell ID and table time it was seeded from, and SCIT's motion then — and is never rewritten. [`manual_tracks_for`](crates/hookecho/src/app/storm_track.rs) finds a storm's manual tracks through the storm history (`StormIdentity::resolve`), so a track stays with its storm through a SCIT renumbering and is never shown for another storm that took the old ID; tracks drawn by hand stay unassociated. The Inspector's storm section and the Cell window share one row set (`storm_card_rows`), which adds a "Manual motion" line beside SCIT's "Moving" (never replacing it): the motion, the analysis time it was set for, how many minutes before the shown scan, and whether it is SCIT's motion unadjusted or adjusted from SCIT's (with SCIT's then). Test: `a_manual_motion_reads_against_the_scit_motion_it_started_from` (source reference, unadjusted vs adjusted wording in both unit systems, age, a hand-drawn track saying nothing about SCIT); full workspace 2,354 passed, Clippy `-D warnings`, the WASM library check. Open: per-storm ETA/closest approach from the manual motion in the Cell window's Threat section (the track card already gives them per marker); persisting manual tracks with their source in cases (M2.4).

**Evidence ledger — 2026-10-06, increment 3 (arrivals from the storm's manual motion):** the Cell window's Threat section gives arrival times and closest approach at each saved place from the storm's manual motion as a second list beside SCIT's, never merged with it, under its own provenance line ("Arrivals from your manual motion, 090° at 32 kt set for 20:50Z"). Both lists share one computation (`arrivals`), which states each time against the scan on screen and projects from the motion's own analysis time and origin, so a motion set ten minutes earlier gives "+20 min" for a place 30 minutes along it, and a passed arrival reads "N min before this scan" instead of a negative lead. Test: `a_manual_motion_gives_its_own_arrivals_from_its_own_time` (time base, provenance line, SCIT's list unchanged beside it), the existing threat tests updated for the new argument; full workspace 2,355 passed, Clippy `-D warnings`, the WASM library check. Open: community/asset impacts (M2.3), persisting manual tracks in cases (M2.4), desktop/Android walkthrough captures.

**Evidence ledger — 2026-10-06, increment 4 (split/merge on both storms):** lineage was recorded only on the storm it happened to (a child's "split from", a vanished storm's "merged into"), so the surviving storm's History line never said it had absorbed another or shed one. It now reads both directions from the history (`split off #2`, `absorbed #3`), and a selected storm that has left the table says why — merged into which storm and which cell that is now, no longer tracked, or missed this scan (`StormIdentity::gone_note`). Test: `a_split_and_merge_read_on_both_storms_and_a_merged_pick_says_where_it_went`; full workspace 2,356 passed, Clippy `-D warnings`, the WASM library check. Open: a lineage marker in the Storms table rows; calibration of the association limits on pinned fixtures; desktop/Android walkthrough captures for M2.2.

**Evidence ledger — 2026-10-06, increment 2 (the Compare set by storm):** the Cell window's open storms (its Compare set) were a list of SCIT IDs, so after a renumbering a renamed storm left the set and a different storm given its old ID could join the comparison in its place. When a new SCIT table arrives the set is now carried through the storm history (`StormIdentity::carry_ids`): a renamed storm keeps its place under its new ID, a storm no longer in the table leaves, and an ID the history has no record of stays only while the table still has it; the last-selection marker is carried the same way so a rename is not taken for a new selection. Test: `an_open_set_of_storms_carries_through_a_rename_and_drops_a_recycled_id`; full workspace 2,376 passed, Clippy `-D warnings`, the WASM library check.

#### M2.3 — Extend impacts to geolocated communities and assets

**Priority:** P0. **Depends on:** M2.1, M1.4. **Original references:** ROADMAP_2 §§2.3, 2.4; ROADMAP_NEW §§L3, L4, R5.

- **Starting evidence:** shared point/line uncertainty geometry, marker/zone entry checks, Census summaries, and existing GIS geometry types.
- **Outcome/build:** query geolocated community and user-selected asset geometries against the existing projection. Show first/last possible impact, center-path entry, uncertainty-flank-only contact, closest approach, and motion/source age. Support points, lines, and polygon zones; consume collections once M4.1 lands through the same geometry seam.
- **Interfaces/compatibility:** one impact result contains target identity/geometry reference, analysis time, motion provenance, and estimated interval. Extend the existing Census 2020 TIGERweb place service with attributed geometry/representative points, retaining source year and population meaning; representative-point ETA is labeled separately from area entry.
- **Failure/compatibility:** names/populations without coordinates do not receive ETAs. Population stays an estimate with dataset date; an arrival projection is not a forecast guarantee. Cache by geometry/motion identity and cancel superseded lookups.
- **Acceptance:** zero/slow motion, inside-now, asymmetric flanks, bent squall lines, narrow crossed assets, holes, and edited vectors agree between drawing and impacts; missing location/time produces an explicit unavailable result.
- **Proof:** geometry fixture comparisons and a desktop/Android community/asset-impact example.

**Agent prompt:** `Implement M2.3 using section 6. Extend the existing shared projection geometry to geolocated communities and asset targets, preserving uncertainty and time semantics.`

**Evidence ledger — 2026-10-06, increment 1 (imported layers as impact targets):** an imported GIS layer can be marked "Impact targets" in the Layer Manager (`GisLayerConfig::targets`, persisted with the layer). The Cell window's Threat section then lists, for the selected storm, the arrival and closest approach at each of the layer's points and the entry into each of its areas, through the same projection geometry the map draws (`ManualTrack::eta` and `zone_eta`): an area the storm's path enters ("path enters"), one only the uncertainty swath's edge reaches (said as such, listed after those in the path), and one the storm is already in ("inside at the motion's time"). Each target is named by the layer's label attribute and its layer, and is listed from SCIT's motion and, separately, from the storm's manual motion when one is set — never merged. Features outside the view's time are left out, and targets are bounded per storm card (2,000 points, 200 areas) with a note when more were skipped ([`impact_targets`](crates/hookecho/src/app/gis_layers.rs), [`target_arrivals`](crates/hookecho/src/app/chrome/dock/cell.rs)). Test: `targets_get_arrivals_entries_and_edge_contact_said_apart` (point arrival time, area entry, edge-only contact ordered after path entries, inside now, out of reach not listed); settings round trip with the flag; full workspace passed, Clippy `-D warnings`, the WASM library check. Open: Census places with attributed geometry and population with dataset year, line targets, first/last possible impact intervals, caching by geometry/motion identity, a desktop/Android example.

**Evidence ledger — 2026-10-06, increment 2 (towns in the path):** the Cell window's Threat section has "Towns in its path": on request (it asks the Census Bureau's service, so it is the analyst's choice), the Census 2020 incorporated and census-designated places touching the storm's projected one-hour swath — along its manual motion when one is set, else SCIT's — with whole-place populations and each place's internal point ([`census::places_in`](crates/wxdata/src/census.rs), the alert cards' TIGERweb service), largest first, at most 25. Each becomes a point target for the same arrival/closest-approach computation as the impact-target layers, labelled as the town's centre ("Census 2020, town centre", with the 2020 population), and the card says the edge can be reached sooner — a representative-point ETA is never presented as area entry. A place whose internal point cannot be read is left out, not placed. Looked up once per swath geometry (`ManualTrack::impact_id`) and kept for the session ([`app/community_targets.rs`](crates/hookecho/src/app/community_targets.rs)); settings-bundle handling moved out of `app.rs` alongside (ceiling 8,977). Tests: `place_points_read_from_a_real_reply` against a pinned real TIGERweb reply (`tests/data/census/tigerweb_places_okc.json`, 1,199 bytes, SHA-256 `1a670eb4…4ece6`, fetched 2026-10-06), `a_town_is_named_plainly_and_said_to_be_its_centre`; the ignored live `places_in_live` was run once and returned Oklahoma City, Midwest City, Del City, Choctaw, Harrah, Spencer and Nicoma Park, largest first; full workspace 2,379 passed, Clippy `-D warnings`, the WASM library check. Open: place areas for true entry times, first/last possible impact intervals, line targets, a desktop/Android example.

**Evidence ledger — 2026-10-07, increment 3 (impact report in the analysis export; WeatherWise "impact/analysis report" gap):** "Export analysis…" now writes `impacts.csv` and `impacts.md` ([`app/impact_report.rs`](crates/hookecho/src/app/impact_report.rs)). For every manual storm motion, each saved place, watch zone, imported impact-target point/area and Census town already found in its path gets one row: arrival time (UTC) from the motion's own analysis time, minutes after it, closest approach and the side it passes on (points), path entry versus swath-edge-only contact and inside-now (areas), and in-path. The readable report states each motion (origin, analysis time, heading, speed, flank widths, cone, historical or not), the people counted in its hour's swath when that count was made, and says plainly when the population count or towns lookup was not made, still running or failed. Nothing is fetched during export; the same `ManualTrack::eta`/`zone_eta` geometry the motion card and Cell window use produces every arrival, so the report cannot disagree with the map. Tests: `every_kind_of_target_gets_its_arrival_in_path_first` (a town, marker, zone and GIS point on a due-east 60 km/h track arrive at their along-track times, in-path soonest first, the storm passes "S" of a marker 10 km north of it, outside the path, one behind gets no row; CSV quoting of Census names) and `the_readable_report_says_what_was_and_was_not_looked_up`. Open: SCIT-only storms (no manual motion) are not in the report; alert-polygon population summaries are not yet included; no PDF/print layout; not exercised on Android.

#### M2.4 — Certify editing and preserve historical manual work

**Priority:** P0. **Depends on:** M2.2, M2.3. **Original references:** ROADMAP_2 §§2.6, 13; ROADMAP_NEW §§K3, Q1, Q3.

- **Starting evidence:** drag origin/owner rollback, keyboard ownership tests, motion constraints, phone/tablet form factor, and case manifests.
- **Outcome/build:** finish mouse, keyboard, pen, and touch creation/edit/duplicate/delete workflows. Add touch-reachable equivalents for modifier and hover actions. Persist manual tracks and storm annotations in analysis cases with their origin time and source association; historical estimates reopen as historical.
- **Interfaces/compatibility:** version the case format for new saved objects, retain old-case reading, and keep imported annotations additive. Restore a live motion estimate only after explicit reactivation; session motion remains time-bounded.
- **Failure/compatibility:** pointer cancellation, focus loss, pane removal, orientation change, pinch takeover, and Back restore or cancel edits consistently. Text input retains its keys; secondary pointers do not edit the primary handle.
- **Acceptance:** a trained operator creates a 60-minute point projection within five seconds on mouse and touch; pen/touch completion and all cancellation paths are demonstrated on physical Android hardware; case round trips preserve geometry, controls, and time.
- **Proof:** deterministic input regressions, case fixtures, and timestamped physical-device walkthroughs.

**Agent prompt:** `Implement M2.4 using section 6. Close touch/pen editing gaps and save historical manual work in cases without silently reactivating stale motion.`

**Evidence ledger — 2026-10-06, increment 1 (manual tracks in cases, reopened as historical):** saved cases now keep every manual storm-motion track ([`CaseTrack`](crates/hookecho/src/case.rs)): origin, motion, both uncertainty widths and cone, mark spacing, a line's edge, the analysis time it was set for, and the SCIT cell, table time and SCIT motion it was started from. The field is additive (`manual_tracks`, omitted when empty), so the case format number is unchanged and a build without it opens the rest of the case. Opening a case adds its tracks (one already here with the same origin, motion and time is not added twice) as **historical**: drawn faded, labelled "from a case, set for …" in the motion card, and left out of every storm's manual-motion line and arrivals until **Reactivate**, which makes it a current estimate that still projects from the time it was set for. Values a hand-edited file could make nonsensical (non-finite speed, out-of-range heading or cone) are brought into the editor's ranges. Test: `a_saved_track_reopens_as_it_was_and_historical` (full round trip including the source storm, historical on reopening, sanitised values, an older case with no tracks); full workspace 2,380 passed, Clippy `-D warnings`, the WASM library check. Open: storm annotations in cases, touch/pen edit certification and cancellation paths on physical Android hardware, the five-second creation acceptance.

### M3 — Radar-analysis depth

#### M3.1 — Expose authoritative gate metadata

**Priority:** P1. **Depends on:** M0.3, M1.1, M1.4. **Original references:** ROADMAP_NEW §§B4, C3, C4; ROADMAP_2 §15.2.

- **Starting evidence:** estimated Nyquist in `level2`, binned inspection, beam geometry, gate metadata UI, and processed velocity paths.
- **Outcome/build:** extend the Level II decode boundary to retain unambiguous velocity, moment scale/offset, source gate encoding/quality, and acquisition context where available. Report authoritative Nyquist separately from the fallback estimate and expose raw vs dealiased/derived samples distinctly.
- **Interfaces/compatibility:** metadata carries its origin (`decoded`, `estimated`, `unavailable`) and source cut/pass. Existing geometry conventions and missing/range-folded semantics remain consistent across the inspector and algorithms.
- **Failure/compatibility:** absent metadata leaves an estimate explicitly labeled; an observed maximum speed does not become an authoritative Nyquist. Malformed metadata must be bounded/rejected without crashing the decoder.
- **Acceptance:** real complete and progressive fixtures agree; split cuts and supplemental passes retain correct metadata; folded/missing gates and zero/invalid headers are handled; beam height tests retain their reference tolerances.
- **Proof:** trimmed raw-message fixtures, independent header interpretation, and raw/processed inspector captures.

**Agent prompt:** `Implement M3.1 using section 6. Retain authoritative metadata at the decode boundary and expose its origin without relabeling existing estimates as decoded values.`

**Status:** implementation: in progress (decoded Nyquist and unambiguous range, increment 1; decoded Nyquist in dealiasing, increment 2); verification: partial (independent byte-level read; Py-ART 2.3.0 comparison on 12 cached volumes).

**Evidence ledger — 2026-10-06, increment 1 (decoded Doppler metadata):**

- *Delivered:* `nexrad-model` is vendored ([`vendor/nexrad-model`](vendor/nexrad-model), MIT, `hookecho patch:` comments) so a model radial carries its Message 31 radial block's Nyquist velocity (0.01 m/s) and unambiguous range (0.1 km) through every path that builds a `Scan` — archive decode, the real-time chunk assembly (both through one `nexrad_data::volume::model_radial` helper) and `level2::trim_scan`'s loop frames. Zero or absent fields stay `None`; legacy Message 1 radials stay unknown. `BinnedSweep::{row_nyquist_mps, row_unambiguous_km}` hold, per azimuth row, the values of the radial that wrote that row (the same writer as `source_radials`). The gate inspector shows "Nyquist velocity (decoded)" — or "not in this data" with the value-based "(est.)" row beside it only then — and a decoded unambiguous range; the workstation Inspector dock does the same ("± … (decoded)" or "≈± … (estimated from values)").
- *Science:* `scientific_corpus::decoded_doppler_metadata_matches_an_independent_read_of_the_radial_blocks` (offline, every PR) reads every `RRAD` block straight from the ICD byte layout in the three committed real partial volumes and requires the decoded per-radial values to equal them exactly (240 radials each; surveillance-cut Nyquist 8.82, 8.35, 8.27 m/s), and binned rows to carry only their writers' values. The cached full-volume check finds every lowest-velocity-tilt row of Mayfield 2021, Denver hail 2017 and Moore 2013 carrying a decoded Nyquist among the blocks' values (30.05, 33.27, 26.12 m/s) — and shows why the distinction matters: the old estimate read off the values exceeds each (30.62, 33.63, 26.60 m/s).
- *Commands:* clippy `-D warnings` clean; `cargo test --workspace --no-fail-fast` 2,341 passed, 0 failed; GPU suite 36 passed (llvmpipe); wasm32 check passes; `cargo test -p wxdata --test scientific_corpus cached_velocity_rows -- --ignored` passed with `HOOKECHO_CORPUS_CACHE`.
- *Limitations / open:* the dealiaser and detectors still use the estimate (`BinnedSweep::nyquist_ms`); switching them changes validated detection inputs and needs its own backtest. Moment scale/offset and per-gate quality flags are not yet surfaced; the gate popup does not yet show storm-relative velocity or pass attribution (the dock already shows pass rows). Relay/live wire formats carry the new fields only where they serialize the model `Scan`.
- *Next:* surface storm-relative and pass attribution in the gate popup; then the dealias/detector switch behind a backtest.

**Evidence ledger — 2026-10-06, gate inspector names the map value:** with SRV or dealiasing on, the gate inspector listed the raw and dealiased velocities but not the value the map coloured, nor how it was made. A "Map shows" row now gives that value and its derivation — raw or dealiased velocity, or `SRV: dealiased velocity − storm motion 090° at 19 kt (−10.0 m/s along this radial)` — computed with the radar shader's radial projection, and says when the input it needs is missing at this gate rather than showing the other value. Which value the map reads is one answer (`map_display`) shared by the map's probe and the inspector. Test: `the_map_value_names_its_derivation` (raw, dealiased, SRV along and across the motion, missing dealiased value, reflectivity untouched); full workspace 2,367 passed, Clippy `-D warnings`, the WASM library check.


**Evidence ledger — 2026-10-06, increment 2 (decoded Nyquist in dealiasing, behind a backtest; gap 5):**

- *Delivered:* [`dealias::sweep_nyquist`](crates/wxdata/src/dealias.rs) unfolds each velocity sweep at the decoded Nyquist velocity when every row's decoded value agrees (to 0.05 m/s) and no raw |v| exceeds it by more than one quantization step; otherwise at the largest-|v| estimate, with the reason kept (`NyquistSource::{Decoded, Estimated, EstimatedVaries, EstimatedInconsistent, Unknown}` on `BinnedSweep::nyquist_source`). Detectors read the same `nyquist_ms`. The Inspector dock adds "Unfolded at ±… (decoded / estimated …)" on a dealiased sweep and no longer reads an "estimate" off unfolded values, which run past the limit.
- *Real failures the backtest found and fixed:* (1) **1-degree binning gaps.** A radial filled the bin under its centre and the one before; real upper-tilt centres wander across the half-degree line (KTLX 8.0°: 279.506°, 280.495°, …), so 82–89 of 720 rows were empty on every 1-degree tilt of the cached corpus, drawn as transparent stripes, and the wedges between them could not be unfolded. [`level2::beam_bins`](crates/wxdata/src/level2.rs) now fills the bins whose centres lie in `(az − w/2, az + w/2]`, and `orphan_bins` gives a bin lying between two irregular radials (KLZK 0.5°, two radials 0.766° apart) to the nearer one; full and live binning share both. (2) **Fragile region dealiasing.** The neighbour-difference flood fill chained across folds through noisy gates and anchored fast fields on aliased regions. The dealiaser now ports Py-ART's `dealias_region_based`: three velocity bands per Nyquist interval, boundaries stepping over up to 100 gates of no data, greedy merging by heaviest boundary with boundaries re-summed as nodes merge, and sweep centring when no continuity reference anchors it. One departure is kept: a boundary's fold is its gate pairs' vote, not their rounded mean, so a shear line cannot drag it.
- *Independent evidence:* [`scripts/corpus/nyquist_pyart.py`](scripts/corpus/nyquist_pyart.py) reads the same cached volumes with Py-ART 2.3.0's own reader and dealiaser and scores HookEcho's exported fields (`HOOKECHO_NYQ_EXPORT`), plus a truth-free VAD check: the share of ring gates a fold or more off each range ring's sinusoid. Raw/raw alignment is 1.000 on super-res tilts and ~0.90 on 1-degree tilts (Py-ART's own azimuths). Over 145 tilts of 12 volumes (Py-ART declines sectorized vilonia-2014) and 1,619,590 ring gates: VAD misfit raw 0.0357; HookEcho before 0.0047 (with the binning fix; ~0.10 per upper tilt without it); **now 0.00123, Py-ART 0.00129**. Gates on Py-ART's fold: 92.1% → **99.64%** of 15,298,450. Neighbouring gates more than V_ny apart after unfolding over all 159 tilts: 512,079 (old binning) → 240,920 (old regions) → **25,114**.
- *Decoded vs estimate:* on this corpus every tilt's raw |v| reaches the limit, so the two differ by under 1% (e.g. 22.11 vs 22.00 m/s): VAD 0.00123 vs 0.00122, Py-ART fold agreement 99.640% vs 99.637%, jumps 25,114 vs 24,868, no tilt worse by more than 28. The decoded value is used on 150 of 159 tilts; vilonia's nine sectorized tilts fall back, labelled. The weak-field case where the estimate falls far short of the limit is covered by unit tests, not by a corpus case.
- *Detector impact:* all cached and pinned suites pass in release (golden events 9, scientific corpus 20, detection regressions 15, clutter 3). One golden expectation changed, with evidence in the test: the Iowa QLCS Somers track's lone Possible candidate (score 0.59, 7.87 km from the path) stood on mis-unfolded velocity — within 10 km of it, 3–21% of 0.5–1.8° gates were a fold off Py-ART, with inbound values to −27 m/s where Py-ART has −12 to −15 — and is gone; the new field agrees with Py-ART on every one of those gates. Knierim keeps its Debris-tier detection 1.3 km from its path. Somers now has no candidate (open science gap, as before for its stronger tier).
- *Commands:* `cargo test -p wxdata --lib level2` 50 passed (new: jittered 1-degree radials tile their rows, fails on the old binning; beam-to-bin edges; a gap bin takes the nearer radial on full and live binning alike); `--lib dealias` 17 passed under the port; `cargo test --release -p wxdata --test nyquist_backtest -- --ignored` passes on 159 tilts; clippy `-D warnings` clean; `cargo test --workspace` 2,403 passed, 0 failed; wasm32 check passes.
- *Limitations / open:* one Nyquist per sweep — a sweep sectorized at different PRFs falls back to the estimate rather than unfolding per sector; Py-ART is a reference dealiaser, not truth; the VAD check is blind to whole-sweep offsets; no 4D/multi-sweep solve beyond the one-sweep continuity reference; moment scale/offset and gate quality metadata remain.
- *Next:* per-sector Nyquist unfolding for sectorized VCPs; then moment scale/offset metadata.

#### M3.2 — Complete portable, validated product definitions

**Priority:** P1. **Depends on:** M1.4, M3.1. **Original references:** ROADMAP_NEW §C1; ROADMAP_2 §§9, 15.2.

- **Starting evidence:** `ProductDef`, AST/input discovery, recursion limits, Settings persistence, and gate-product ramp rendering.
- **Outcome/build:** add versioned product JSON import/export, stable IDs, chosen palettes, explicit dependencies, output kind, environmental requirements, units/quantity, and altitude convention. Validate expressions and bound operation cost as well as recursion; provide editor diagnostics and reference examples.
- **Interfaces/compatibility:** preserve legacy unadorned literals using contextual input units. Identify incompatible typed operands and height datums. Existing `BEAM_HEIGHT_M` is antenna-relative; do not silently call it terrain AGL. Environmental isotherm heights and `BEAM_ALTITUDE_M` use MSL. True AGL needs terrain metadata.
- **Failure/compatibility:** old definitions remain importable with their original semantics; incompatible/unsafe ones are disabled with edit/review diagnostics. Missing inputs propagate as missing, not zero; user formulas do not execute arbitrary code.
- **Acceptance:** old/new round trips retain identity and semantics; malformed/deep/expensive formulas remain bounded; incompatible units/datums are explained; map/probe/export palettes and ranges agree.
- **Proof:** compatibility and resource-limit fixtures, portable example products, and editor/map captures.

**Agent prompt:** `Implement M3.2 using section 6. Extend the existing safe DSL and product editor with portable definitions, palettes, dependency validation, and explicit altitude conventions.`

**Evidence ledger — 2026-10-06, increment 1 (portable product files):** [`wxdata::udp_file`](crates/wxdata/src/udp_file.rs) reads and writes a versioned `hookecho-product` JSON document (version 1). Each product carries a stable ID (`ProductDef::id`, given to new products at creation and to older saved ones once at load, derived from name and formula with FNV-1a so it is the same on every platform) and what it needs, derived from its formula: the inputs it reads, the environmental heights among them, whether it is a gate or a column product, and the altitude convention of the heights it reads — `above_antenna` for `BEAM_HEIGHT_M` (never called terrain AGL), `msl` for `BEAM_ALTITUDE_M` and the isotherms, `mixed` when a formula compares the two, which is reported. Import refuses a whole document that is not JSON, not this format, from a newer version, or over 256 products, and checks every product before accepting it: a name, a formula under 4,096 bytes that parses (the parser's nesting bound applies), at most 512 operations, no column reduction inside another; an unknown colour table or an empty range is dropped to the defaults and said; declared needs that disagree with the formula are reported and the formula wins. A bare list of definitions, as earlier builds and settings store them, imports with its original meaning. Merging updates a product with the same ID in place and renames a new one whose name is taken, never shadowing it. The User-defined products window has Export… and Import… with a per-product report; Android's picker gets the new kind. Formulas remain the existing safe expression language; nothing executes. Tests (6): round trip with IDs and derived needs, legacy bare list, bad products refused by name (unparsable, too deep, too many operations, nested column, unnamed, oversized) with the rest imported, declared-vs-derived disagreement and mixed datums, refused documents, merge by ID without name shadowing; full workspace 2,373 passed, Clippy `-D warnings`, the WASM library check. Open: units/quantity typing of operands, disabled-with-diagnostics state for incompatible saved products (refused ones are not imported at all), reference example files, editor captures.

#### M3.3 — Render and export column-based products

**Priority:** P1. **Depends on:** M0.2, M1.1, M3.2. **Original references:** ROADMAP_NEW §§C1, F8, H6; ROADMAP_2 §§9, 11.3.

- **Starting evidence:** `evaluate_at_column`, existing local-derived horizontal grids, gate-product evaluation, freezing-level fetches, and profile/sounding infrastructure.
- **Outcome/build:** evaluate column formulas on the local-derived horizontal analysis grid, with source-gate coverage and configurable resolution within quality budgets. Render through the existing field pipeline; share values with probes, thresholds, trails, and scientific exports. Provide 0/−10/−20 °C environmental heights from time-aligned temperature profiles when available.
- **Interfaces/compatibility:** column output is a 2D field, not a fake sweep or 3D voxel moment. For −10 °C, interpolate crossings only between valid bracketing profile levels; use the lowest crossing encountered while ascending in altitude, regardless of temperature gradient, and report multiple crossings/uncertainty. Record MSL datum, source/run/valid time, and interpolation.
- **Failure/compatibility:** no bracketing temperature, unsupported archive profile, or out-of-tolerance environment means missing output with an explanation. Cancel obsolete work and retain no current environment over a historical volume by substitution.
- **Acceptance:** composite-reflectivity, masked CC minimum, and ZDR-above-isotherm examples match independent column calculations within input/quantization tolerance; changes to inputs/time invalidate the right cache; incomplete beams remain identified.
- **Proof:** reference columns, grid/probe/export equality tests, and live/archive native workflow captures. GPU expression generation is optional after CPU correctness/performance is established.

**Agent prompt:** `Implement M3.3 using section 6. Render column formulas through the existing field pipeline and derive time-aligned environmental inputs without inventing missing profile data.`

**Status:** implementation: in progress (column fields on the map, increment 1); verification: partial (unit, real-volume CPU, software-GPU; no physical device).

**Evidence ledger — 2026-10-06, increment 1 (column formulas on the map):**

- *Baseline and commit:* branch head `5efb105` (`feat/wsv3-redesign`); implementation on `claude/hookecho-professional-parity-a8qspm` — see the commit that adds this entry.
- *User workflow delivered:* in User-defined products, a column formula's **Show on map** (or the **Column user product** layer) draws it as a 2D field over the tilt for the active pane. It is evaluated from every distinct tilt of that pane's volume ([`wxdata::udp_column`](crates/wxdata/src/udp_column.rs)) on the local-derived 0.01° grid off the UI thread, under the pane's temporal policy, on its own request lane ([`app/column_product.rs`](crates/hookecho/src/app/column_product.rs)). The legend samples the drawn LUT and states tilts, the source acquisition span and any older-pass/masked/untimed rows; the cursor probe reads the exact cell and the number of beams over it; GeoTIFF/NetCDF exports carry formula, units and environment source; the gate inspector evaluates column formulas with the map's rules and names the environment source; workspaces save the pane's choice. `MINUS10C_HEIGHT_M` joins the 0/−20 °C inputs: HRRR `263 K level` (same run as the other two) while following live, the lowest bracketed −10 °C crossing of that day's sounding (crossing count retained) on an archived volume ([`app/env_levels.rs`](crates/hookecho/src/app/env_levels.rs)).
- *Interfaces/compatibility:* column output is a 2D field, never a sweep or voxel. The base of a column formula (bare inputs, layer bounds) is the lowest sampled level; a column with nothing the formula reads has no value. Formulas reading an isotherm the matched source lacks are refused with a reason; nothing is substituted, and a live reading never serves an archived volume. Nested vertical functions are limited to one level on the map and work is bounded (`MAX_NODE_EVALUATIONS`). The key includes weak scan identity, revision, acquisition snapshot, policy, definition/palette/theme and only the isotherm values the formula reads plus their source. Formula `VEL` is dealiased in both map and inspector (the inspector previously read raw velocity). Settings/workspace formats remain readable; `column-product` is an optional pane field. User column fields are never smoothed between cells.
- *Fixed on the way:* (1) the shared derived grid declared the unrounded range-disk extent while sampling cells at exactly 0.01°, displacing composite/VIL/VILD/echo-top/MEHS/POSH/trail-export cells up to one cell toward the east/south edges — now the declared extent matches the cells (`derived::tests::a_cell_is_drawn_where_it_was_sampled`); (2) probes and grid exports of local radar fields now require the pane's own selection, as drawing did.
- *Inputs and expected behavior:* synthetic columns in `udp_column` tests; the committed real partial Mayfield volume; the full pinned Denver hail 2017 and Mayfield 2021 volumes (provisioned by `scripts/corpus/provision.py`, checksums verified).
- *Commands run and results:* `cargo test -p wxdata --lib` 944 passed; `cargo test -p hookecho --lib` 1261 passed; `HOOKECHO_CORPUS_CACHE=… cargo test -p wxdata --release --test scientific_corpus cached_column -- --ignored` passed — composite identical, CC-minimum 0/1,104 and 0/1,058 cells differ, ZDR-above-height 0/1,104 and 0/1,058 (524/1,104 differed before the grid-extent fix); full Mayfield volume 875×698×14 tilts in 92–137 ms per formula (release, 4-core cloud VM); `HOOKECHO_GPU_FALLBACK=1 cargo test -p hookecho --lib headless::corpus::gpu_column -- --ignored` passed on llvmpipe — 0 colour mismatches in 18,016 + 7,732 stable pixels, 0 filled empty cells ([captures](docs/certification/m3.3/README.md)); `--headless-column` renders the real Denver volume (385 ms debug build) and refuses a −10 °C formula with no stated level. `cargo clippy --workspace --all-targets -- -D warnings` clean (after two behaviour-neutral lint fixes in the unfinished GOES work at the baseline); `cargo test --workspace --no-fail-fast` 2,324 passed, 0 failed, 154 ignored; `HOOKECHO_GPU_FALLBACK=1 cargo test -p hookecho -- --ignored gpu` 34 passed on llvmpipe (existing goldens and pinned radar check unchanged); `cargo check --target wasm32-unknown-unknown -p hookecho --lib` passes with no new warnings.
- *Desktop evidence:* Linux cloud VM, software Vulkan only. No physical desktop GPU run; no interactive session.
- *Android evidence:* none. The worker uses the shared spawner (one job at a time per product); phone/tablet reachability goes through the same User-defined products window and layer list, unverified on hardware.
- *Browser:* compiles for wasm32 (rows evaluated serially there); runtime not exercised.
- *Known limitations / open gates:* evaluation follows the active pane (another pane with the same selection shares it; a pane with a different one shows a status card until it is focused); grid resolution is fixed at 0.01°; there is no MSL/AGL layer-bound variant (layer bounds are `BEAM_HEIGHT_M`, antenna-relative; isotherm comparisons use `BEAM_ALTITUDE_M` in conditions); environment is one site-level value per volume, not a horizontally varying field; HRRR is an analysis valid at its run, not interpolated to the scan time; column fields are not yet sources for trails, cross-sections or 3D; portable product files and typed unit/datum checks (M3.2) remain.
- *Next ready increment:* M3.4 — wire `extrema::SlidingTrail` into the trail layer with per-gate age as display opacity only, exact expiry on scrub, contributor-time probe and export without `Utc::now()` fallbacks; then column/MRMS grid trails.

**Evidence ledger — 2026-10-06, GeoTIFF read by GDAL:** the GeoTIFF writer every gridded export uses (column products included) was read back with GDAL 3.8.4 (installed in the container for this check; not a CI gate) from an ignored sample writer (`write_geotiff_sample`): `gdalinfo` reports EPSG:4326, origin (−98, 36), pixel size (0.25, −0.2), `AREA_OR_POINT=Area`, NoData NaN, the description tag as written, and statistics that exclude the hole (mean 8.318 = 91.5/11); `gdallocationinfo` returns 0 at (−97.9, 35.9) and 15 at (−97.4, 35.6), the cells a reader should find there. Complements the in-repo `an_independent_reader_gets_the_grid_and_its_georeferencing_back` (the `tiff` crate).

**Evidence ledger — 2026-10-06, increment 2 (composable column science):**

- *Audit/baseline:* began at `fe1d7d4` on `feat/wsv3-redesign`, after reading the current task,
  architecture, contributor gate and recent UDP commits (`bdf09ec`, `abcf097`). Portable files,
  map/export/probe and column trails already exist. The demonstrated gap was a blanket rejection
  of nested map reductions and a narrow set of reductions. Concurrent detector work landed as
  `a377dfc`; that work is preserved and this increment changes no detector or app-shell code.
- *Implementation:* the existing expression owner now supports `mean_vertical(e[,cond])`,
  `count_vertical(cond)`, `fraction_vertical(cond)`, `max_height(e[,cond])`,
  `min_height(e[,cond])`, `first_crossing_height(e,t)`, `last_crossing_height(e,t)` and
  `integral_layer(e,lo,hi)`. Layer max/min/mean accept a fourth condition. Import, editor, grid
  and column probe allow two nested reductions; grid admission budgets saturating AST visits,
  multiplying per-entry arguments at each reduction and counting scalar bounds/thresholds once.
  A long-range synthetic case demonstrates rejection before grid-value allocation where the
  former cells × levels × nodes estimate would admit excessive nested work.
- *Scientific contract:* means weight recorded beams equally; fractions exclude missing
  conditions from their denominator, and all-missing conditions return missing. Extrema heights
  are sampled ARL metres, with the lowest height winning ties; an unlocated winning/tied value
  returns missing height. New crossing functions interpolate adjacent recorded beams in either
  gradient direction; existing sampled threshold-height functions retain their meaning. Layer
  integrals are clipped trapezoids in input-units × metres, requiring the entire nonzero interval
  to be bracketed without missing spans. Unknown/duplicate heights invalidate the interpolation
  profile. No extrapolation or environmental substitution is introduced. Undefined arithmetic
  becomes missing before comparison/mask evaluation, and reversed clamp bounds no longer panic.
- *Workflow/compatibility:* all functions feed the existing column grid, renderer, cell probe,
  trails and exports. Implicit ARL height dependencies are now included in portable metadata.
  The existing version-1 file format is retained; older builds reject unknown new functions or
  unsupported nesting through their existing diagnostics. Finite legacy formulas retain their
  results. The editor explains semantics and diagnoses excessive nesting. Six importable,
  round-tripped [reference products](docs/certification/m3.3/composable-products.json) and
  [scientific rules/reproduction](docs/certification/m3.3/README.md) accompany the change.
- *Independent numerical evidence:* algebraic crossing references and an analytic linear-profile
  integral (7,000 value-metres over 500–2,500 m), irregular spacing, reversed input/bounds,
  missing spans, ties and undefined arithmetic are covered by unit tests. Explicit
  `cargo test -p wxdata --test scientific_corpus cached_column_products_match_independent_columns
  -- --ignored --nocapture` passed (1 test): all four new direct reference calculations
  (nested peak-band minimum, mean, fraction, peak height) had **0 mismatches** in **1,104 Denver**
  and **1,058 Mayfield** cells (8,648 new comparisons), using checksummed full volumes cropped to
  60 km, every distinct tilt. Existing composite equality, masked CC and isotherm-relative ZDR
  comparisons also passed with zero mismatches. Test environment heights are stated parameters,
  not observations.
- *GPU evidence:* explicit `cargo test -p hookecho --lib
  gpu_column_product_renders_where_its_cells_are -- --ignored --nocapture` passed (1 test,
  six formulas) on Windows, **NVIDIA RTX 2060 / Vulkan, driver 572.16**. Composite, CC mask,
  mean, fraction, peak height and nested band had **108,976 stable colored pixels, 0 color
  mismatches; 695,635 empty-cell pixels, 0 filled**. Fraction and peak-height captures were also
  visually reviewed. This is production field rendering offscreen on the committed partial
  Mayfield fixture, with the existing quantization/LUT/channel-tolerance rules; it is not a
  complete-volume accuracy, physical interaction, visible-presentation or application-soak claim.
- *Build verification:* final `cargo clippy --workspace --all-targets -- -D warnings` passed.
  `cargo check --target wasm32-unknown-unknown -p hookecho --lib` passed (10 existing hookecho
  warnings in unchanged browser-gated code). Fresh `cargo test --workspace` following the final
  unknown-height correction passed **2,418 tests**, zero failures, **162 ignored** (34 suites,
  including zero-test/doc-test suites); the preceding full run had the same counts. Formatting
  checks on all changed Rust files and `git diff --check` passed. Local logs/captures are under `target/parity-review/udp-*.log` and
  `target/parity-review/m3.3/`; the durable tests and reference definitions are tracked.
- *Open gates/next work:* operand unit/datum typing, terrain-AGL layer bounds, additional
  environment/profile fields and isotherms, finer pane ownership, and full-range nested-product
  performance remain. No Android build/device or browser runtime was exercised; no configured
  adb/NDK environment was found. Desktop live/interactive and multi-hour application profiles,
  Android phone/tablet and visible-presentation certification remain open. The partial input
  verifies rendering and missing sectors; recorded complete volumes provide the separate numeric
  evidence. Continue with matched environmental/profile fields or operand typing after these
  gates; do not rebuild delivered trails/render/export features.

**Evidence ledger — 2026-10-06, increment 3 (recorded-height archived environment):**

- *Audit/baseline:* `a377dfc` on `feat/wsv3-redesign`; rechecked status, this card, architecture,
  contributor gates and recent environmental/UDP owners. The demonstrated datum error was the
  archived overlay adding radar elevation to hypsometric heights above the balloon launch site.
  The UWyo cache already retains recorded HGHT geopotential metres MSL, which the legacy
  thermodynamic parser discards. Detector, parcel and backtest calibration code is preserved.
- *Implementation:* focused `raob::environment` reads adjacent recorded HGHT/TEMP, retaining
  holes, both gradient directions and exact samples. All five isotherms (0 through −40 °C) keep
  missing values independently. −10/−30/−40 use lowest crossings; 0/−20 retain highest cooling
  crossings when present, otherwise lowest recorded crossings. Hail weighting requires a positive
  0/−20 layer. The same bounded historical station/launch search and raw cache are reused;
  source strings name WMO/site, full selected launch, datum, interpolation rule and table SHA256.
  This selection is a request/cache identity, not an independently verified reported launch time.
- *Workflow/compatibility:* `MINUS30C_HEIGHT_M` / `MINUS40C_HEIGHT_M` flow through existing
  gate, column, map, probe, trail and gate/3D owners. Live HRRR leaves them missing and names its
  actual 0C/263K/253K fields, full analysis date and MSL units. Portable version-1 definitions
  derive the additional requirements and MSL convention. Reports add the levels/crossing counts
  and serialize unavailable heights as null. Column/trail keys retain requested-profile source
  identity even when its requested level is missing; gate/3D keys include environmental source
  identity. A single Input classification also gates proactive fetch and historical trail
  admission for all five isotherms; a synoptic-boundary regression rejects earlier scans,
  missing contexts and unknown timestamps. No complete profile serialization for gate/3D is claimed. Environment dispatch and
  gate/product sampling moved to existing focused owners, removing roughly 200 app-shell lines.
- *Independent numeric evidence:* two pinned cached raw tables (OUN selected 2013-05-20 12Z,
  111 pressure rows; Denver selected 2017-05-08 12Z, 56 rows) have raw-byte SHA256 and a separate
  stdlib Python fixed-column reader. Receipt and reported observation timestamps are null because
  the legacy cache did not retain them. Ten isotherm references, missing spans, warming/cooling,
  independent missing fields, station/epoch matching and source changes are durable regressions.
  See [raw tables, references and scientific contract](docs/certification/m3.3/recorded-environment/README.md).
  Explicit cached scientific corpus test passed **1 test**: actual Denver −30/−40 layer masked
  maximum and ARL-converted layer mean had **0 mismatches across 1,104 checked cells**, with
  **531 populated**. Bounds came from the independent Python reader; radar sampling is independent
  of the DSL. Existing Denver/Mayfield composite, mask and four composable references retained
  **0 mismatches** (1,104 / 1,058 sampled cells).
- *Gate remediation:* the first workspace attempt stopped a scripted provider after a fixed
  100 ms and recorded only two of its expected three updates under concurrent compile load.
  The healthy/failing provider controls now await the actual HealthBoard transition with a bounded
  timeout before shutdown. Production provider behavior is unchanged; the failure-only control
  no longer claims to prove reconnection, which has its separate control. The next workspace
  attempt passed the controls and 2,389 tests in completed suites but ended with a Windows
  executable-sharing error during overlapping Cargo jobs. A final stable-source run follows
  those builds; neither interrupted attempt is counted as a passing workspace gate.
- *GPU evidence:* explicit seven-formula offscreen field control passed **1 test** on
  **NVIDIA RTX 2060 / Vulkan, driver 572.16**: **126,992 stable colored pixels, 0 color
  mismatches; 808,482 empty-cell pixels, 0 filled**. The additional isotherm mask uses stated
  rendering parameters, not a measured Mayfield environment. Its capture was visually reviewed.
  Production field/LUT placement and holes are verified; physical interaction, visible presentation
  and the later proactive-fetch/trail-admission changes are outside this offscreen control.
- *Build verification:* final `cargo test --workspace` passed **2,427 tests, 0 failed,
  162 ignored** across **34 suites**, including empty/doc-test suites. All new recorded-profile,
  additional-input, portable-file, historical-cache and trail-boundary controls passed. Final
  `cargo clippy --workspace --all-targets -- -D warnings` passed. Fresh
  `cargo check --target wasm32-unknown-unknown -p hookecho --lib` passed with the same **10 hookecho
  and 1 wxdata existing warnings**. Changed-file rustfmt checks and `git diff --check` passed.
  Logs are `target/parity-review/env-workspace-complete.log`, `env-clippy-complete.log`,
  `env-wasm-complete.log`, `env-real-columns.log`, `env-gpu.log` and `env-reference.log`; captures
  are in `target/parity-review/m3.3/`. Durable fixtures/tests/references are tracked. Milestone
  verification remains partial: these numeric/build/offscreen results do not close application,
  presentation, physical-device or soak certification.
- *Open gates:* recorded geopotential height is explicit and compared to approximate geometric
  beam altitude; exact geodetic conversion and operand unit/datum typing remain open. The sparse
  sounding, separation from radar and selected-launch age are interpretation limits. Archive
  network availability/reported-launch metadata were not certified. Further matched profile fields,
  pane independence, full-range nested cost, full-application/visible-presentation/long-soak,
  browser runtime and physical Android phone/tablet gates remain open. The overarching parity
  objective remains active; delivered renderer/trail/export foundations should not be rebuilt.

#### M3.4 — Preserve physical extrema while aging trails

**Priority:** P1. **Depends on:** M0.3, M1.4, M3.3. **Status:** implementation: in progress (polar trail layer wired to the exact sliding window, increment 2); verification: partial. **Original references:** ROADMAP_NEW §C2; ROADMAP_2 §§8, 15.2.

- **Starting evidence:** polar extrema, code-changing decay, cached-window rebuilding, threshold outlines, and trail raster export.
- **Outcome/build:** retain physical extrema separately from contributing time/coverage and age opacity. Support exact expiring windows with bounded frame storage and reusable/block summaries; rebuild deterministically on backward seek or incompatible geometry. Add user-column and MRMS MESH/AzShear trails using their own native grid identity.
- **Interfaces/compatibility:** include requested/actual time coverage, missing-frame count, grid/beam identity, mode, source product, value, and contributor time. Age fade affects display opacity only. Existing fade settings migrate to this display behavior with a release note; reset/threshold/export controls remain reachable.
- **Failure/compatibility:** raw/dealiased velocity and changed value ranges are not merged as if equivalent. Optional bounded prefetch explicitly shows progress/cancellation; a short cached history is not advertised as the full requested window.
- **Acceptance:** advancing past the strongest old frame removes its contribution; backward/reordered inputs rebuild consistently; numeric probes/exports are unchanged when fade toggles; sentinels never win; polar/grid product trails retain correct units and timestamps.
- **Proof:** brute-force reference extrema comparisons, expiry/coverage fixtures, and independently recomputed historic hail/rotation trails.

**Agent prompt:** `Implement M3.4 using section 6. Separate extrema from age opacity, implement truthful sliding windows, and extend trails to user-column/MRMS fields.`

**Evidence ledger — 2026-10-05, increment 1 (domain seam):** [`extrema::SlidingTrail`](crates/wxdata/src/extrema.rs) keeps the frames of the last window behind its newest (at most a capped count, oldest dropped first) and recomputes the trail as of any moment from those inside the window, so advancing past the strongest old frame removes its contribution instead of fading it by rewriting values ([`decay`](crates/wxdata/src/extrema.rs), which this replaces once wired). Each gate carries the time of the frame that supplied it (the newest on a tie), for age as display opacity only. Coverage reports the requested window, the span actually held, missed volumes at the trail's own cadence, and the seconds at the old end with no frame, so a short history is not presented as the full window. Frames are placed by time, so out-of-order and backward input give the same trail; a frame at a held time replaces it; another moment, value range, geometry, elevation or site resets the trail with its reason, as the existing accumulator does, and sentinels never win. Tests (5) check it gate by gate against brute-force extrema over pseudo-random frames with sentinels after every frame for Max and Min trails, expiry of the strongest old frame, order independence, coverage with a missing volume and a short history, the frame cap and a reset on another moment; wxdata Clippy and the WASM library check passed. Not yet wired to the chase trail layer (still the running accumulator with decay); user-column and MRMS grid trails and the fade-setting migration remain.

**Evidence ledger — 2026-10-06, increment 2 (the trail layer on the exact window):**

- *Baseline and commit:* follows the M3.3 increment-1 commit on `claude/hookecho-professional-parity-a8qspm`; see the commit that adds this entry.
- *User workflow delivered:* Max/min trail now draws [`extrema::SlidingTrail`](crates/wxdata/src/extrema.rs) as of the pane's playhead ([`app/trail.rs`](crates/hookecho/src/app/trail.rs)): frames are kept by scan time, stepping forward past the strongest old volume removes it, and scrubbing backward retains the window ending at the playhead (`retain_window`) and rebuilds exactly what a fresh trail there gives. An older volume from another beam (a VCP change) is skipped instead of resetting the trail, so the result does not depend on the order frames are binned. "Fade with age" is now per-gate display opacity from each gate's contributor age, uploaded as a new radar opacity texture (`RadarUpload::gate_alpha`, binding 4, 1×1 dummy otherwise); values, threshold, probe and exports are identical with it on or off. The cursor probe gains a trail line — the extremum, the scan that set it and its age before the playhead, frames, span and coverage. The status line states a history shorter than the window and missed volumes. GeoTIFF/GeoJSON exports are stamped with the trail's own playhead time (the `Utc::now()` fallback is gone) and carry frames, first/last frame times, missing volumes and the unfilled part of the window. Frames held are capped (64 desktop, 32 Android) behind the window bound.
- *Interfaces/compatibility:* `extrema::Merge::Skipped`, `SlidingTrail::{times, retain_window}`, `WindowTrail::at_point`, `BinnedSweep::index_at` (shared locator with `sample_at`). The old running accumulator and `decay` stay in `wxdata` (unused by the app). Trail filters were never persisted, so nothing migrates; the fade setting keeps its name with display-only meaning (release note in CHANGELOG).
- *Commands run and results:* `cargo test -p wxdata --lib extrema` 19 passed, including brute-force exact extrema, a backward scrub equal to a fresh trail at five playheads, mixed beams converging to the same trail in four arrival orders, and point lookups agreeing with the raster index; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace --no-fail-fast` 2,329 passed, 0 failed; `HOOKECHO_GPU_FALLBACK=1 cargo test -p hookecho -- --ignored gpu` 36 passed on llvmpipe, including `gpu_gate_opacity_fades_display_only` (29,655 opaque pixels bit-identical to an unfaded render, 4,937 faded pixels at the requested alpha within 8/255, values untouched) and the M3.3 column check; the pinned radar check's counts are unchanged from its recorded baseline (0 mismatches, 0 filled); wasm32 check passes with no new warnings.
- *Desktop evidence:* software Vulkan on a Linux cloud VM only; no interactive scrub or physical GPU run.
- *Android evidence:* none (frame cap chosen for memory, unmeasured).
- *Browser:* compiles; runtime not exercised.
- *Known limitations / open gates:* the trail is built only from volumes already in the loop's decode cache (no bounded prefetch with progress/cancel); one site and tilt; trails of column products, MRMS MESH/AzShear and other grids are not built yet; no historic multi-volume hail/rotation trail has been independently recomputed against a reference (the corpus pins single volumes); binning runs on the UI thread, at most three volumes per frame.
- *Next ready increment:* grid trails — a `SlidingTrail` equivalent for `MrmsField` grids keyed by native grid identity, fed by accepted `UserColumn` and MRMS MESH/AzShear fields, with the same contributor-time probe and export.

**Evidence ledger — 2026-10-06, increment 3 (column user product trails):**

- *User workflow delivered:* **Column user product trail** in the layer list draws the per-cell maximum (or minimum, following the trail controls) of the pane's column user product over the loop's cached volumes in the trail window ending at the playhead ([`app/column_trail.rs`](crates/hookecho/src/app/column_trail.rs)). Each volume is evaluated off the UI thread, two per job and one job at a time, by the same path as the single-volume product (a test proves the frame equals that product bit for bit), and kept by scan time in [`extrema::GridTrail`](crates/wxdata/src/extrema.rs). Its legend states window, volumes, history shortfall, missing and still-building volumes and those left out; the probe reads the exact cell and the volume that set it; exports name the product, window and coverage. Drawn cell by cell, never smoothed.
- *Interfaces/compatibility:* `GridTrail`/`GridWindowTrail`/`GridMerge` beside the polar trail (NaN never wins; another grid resets when newest and is skipped when older); `column_product::product_identity` keys the trail by definition, palette and the environment values read plus their source, not by volume. A formula reading an isotherm uses only volumes whose environment epoch is the reading's own (the live analysis while following; the same synoptic sounding when scrubbing) and counts the rest as left out — no other time's environment is applied. A stale job answer is dropped by key and job number.
- *Commands run and results:* `cargo test -p wxdata --lib extrema` 21 passed (grid trail exact vs brute force for max and min with NaN cells, backward scrub equals fresh at four playheads, grid mismatch skip/reset); app tests 2 new; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace --no-fail-fast` 2,333 passed, 0 failed; GPU suite 36 passed on llvmpipe; wasm32 check passes with no new warnings.
- *Evidence not yet established:* no real multi-volume event has been trailed against an independent reference (the pinned corpus holds single volumes); no interactive scrub, desktop GPU, Android or browser runtime run. Live following applies the current HRRR analysis only to volumes it labels live, so a formula reading isotherms trails across the live window against that one analysis — stated, but not time-interpolated.
- *Next ready increment:* M3.1 — decoded Nyquist/unambiguous velocity and moment metadata at the Level II boundary, with origin (`decoded`/`estimated`/`unavailable`) in the gate inspector; then M3.6/cross-section ergonomics.

#### M3.5 — Add translucent volume rendering and richer surfaces

**Priority:** P1. **Depends on:** M0.3, M1.1, M1.2, M3.2. **Original references:** ROADMAP_NEW §§H1, H2, H3, H7; ROADMAP_2 §§11.2, 14.

- **Starting evidence:** MIP raymarch, four-point opacity curve, multi-moment map volumes, marching surfaces, and coverage-aware voxel sampling.
- **Outcome/build:** keep MIP and add front-to-back alpha compositing with early termination, step-length opacity correction, optional gradient lighting, and empty-space skipping where beneficial. Add editable ordered color/opacity stops and saved product presets. Extend existing isosurfaces to multiple thresholds and signed velocity pairs within mesh budgets.
- **Interfaces/compatibility:** rendering mode and versioned transfer-function presets use product physical units. Adapt legacy four-point curves without changing existing MIP output. Lighting/color changes never alter sampled or exported scientific values.
- **Failure/compatibility:** unsupported GPU limits produce a capability explanation or existing supported mode. Missing coverage stays transparent; velocity surfaces represent sampled radial velocity, not synthesized 3D wind. Never extrude a 2D hail field into a claimed measured volume.
- **Acceptance:** synthetic density/coverage cases distinguish MIP and compositing correctly; opacity stays stable when step size changes; clipping/transfer edits avoid scientific-grid rebuilds; multiple surfaces respect bounds and desktop/Android balanced budgets.
- **Proof:** GPU image checks, numerical opacity references, reference-event renders, and performance traces.

**Agent prompt:** `Implement M3.5 using section 6. Extend the existing volume/surface pipelines with true compositing and reusable transfer functions while preserving MIP and value semantics.`

**Status:** implementation: in progress (front-to-back compositing and gradient lighting, increment 1); verification: partial (CPU opacity references; software-GPU frames; no physical device or real-volume capture).

**Evidence ledger — 2026-10-07, increment 1 (translucent compositing, GR2Analyst volumetric display):**

- *User workflow delivered:* the 3D map's Smooth volumes (Slice section, "Render") and the 3D Reflectivity window offer **Maximum** (MIP, unchanged), **Translucent** and **Lit**. Translucent marches front to back and composites every kept sample (floor/ceiling/clip/plane rules unchanged), so a core is seen through the weaker echo in front of it instead of flattened onto it. Lit shades each sample by the coverage-weighted value's central-difference gradient against a headlight from just above the camera. The Inspector's 3D card records the mode (`Render: Maximum (MIP)` / `Translucent, per km` / `Translucent, lit`).
- *Opacity semantics:* in translucent modes the ramp, the CC-anomaly ramp and a drawn opacity curve are read as **opacity per kilometre of path**; each step contributes `1 - (1 - a)^dt_km` with `dt_km` measured in real km from the box's own horizontal and vertical scales (`render3d::render_uniform`), so vertical exaggeration and the Quality step count do not change how opaque a storm looks. The default translucent ramp is `0.02 + 0.9 t³` above the floor so long paths through weak echo stay haze. Rays stop at 99.5 % accumulated opacity. Layer opacity scales the per-km density. Lighting and compositing change only displayed colour; the volume, probe, export and every scientific value are untouched.
- *Interfaces/compatibility:* `render3d::VolumeRender` (`Mip` default), a new `render` vec4 in the raymarch uniform block, `View3d::render`, `Map3dState::volume_render`, `Volume3dState::{render, half_km}`; `orbit_uniform` takes the grid's `half_km`. `Volume3dPreset` gains an optional `render`, omitted when absent: presets saved before this change load as MIP (they were drawn for it), and a preset saved now restores its mode. No other settings or workspace format changes.
- *Commands run and results:* `cargo test -p hookecho --lib translucent_tests` (step-count invariance against the analytic `1-(1-a)^L` for 8–1024 steps within 1e-4, front-to-back occlusion, a haze-in-front-of-core ray matching the closed form, both axes' km scales), `a_3d_preset_saved_before_translucent_rendering_loads_as_mip`, the Inspector rows test, `every_shader_parses_and_validates` (naga). GPU (`HOOKECHO_GPU_FALLBACK=1`, llvmpipe): `translucent_rendering_is_stable_across_steps_and_differs_from_mip` — on a 60 dBZ core inside a 25 dBZ shell, mean per-channel difference 0.13/255 between 64 and 256 steps, 6.08 between MIP and translucent, 2.65 between lit and unlit; `the_opacity_curve_decides_what_draws` still passes.
- *Desktop/Android/browser evidence:* none interactive; the shader runs unchanged on every backend the existing raymarch does (WGSL validated by naga; the WASM build compiles it). Lit mode costs six extra texture reads per contributing sample; early termination usually ends rays well before the box's far side, but neither cost was profiled on Android.
- *Known limitations / open:* no editable stop list beyond the existing four-point curve, no empty-space skipping, no multiple-threshold or signed-pair isosurface extension in this increment; no real-volume reference renders or device performance traces.
- *Next increment:* ordered colour/opacity stops with versioned presets in physical units; then multiple isosurface thresholds within the mesh budget.

**Evidence ledger — 2026-10-07, increment 2 (editable opacity stops):** the opacity curve is now two to eight ordered `[value, opacity]` stops in the product's own units ([`render3d::TfStops`](crates/hookecho/src/render3d.rs)) instead of exactly four: double-click the plot (or "+", which splits the widest gap) adds a stop on the existing curve, so nothing changes until it is moved; right-click a stop (or "−") removes one; stops stay between their neighbours. The uniform carries stops five to eight in two more vec4s plus a count (unused slots repeat the last stop, so the curve stays flat past it), and the shader interpolates through all of them; with translucent rendering they are still opacity per kilometre. Presets stay readable both ways: `curve` keeps four points (the stops themselves when there are four, otherwise four samples of the curve at its ends and thirds, which is what a build predating this reads), and the exact stops go in `stops`, written only when there are not four. Tests: `tf_stops_tests` (sorting/clamping/2–8 bounds, piecewise-linear and flat past the ends, adding a stop leaves the curve unchanged, the uniform layout and padding, a four-stop curve equals the legacy uniform), `a_preset_with_more_than_four_stops_stays_readable_by_older_builds`; GPU (llvmpipe) `stops_past_the_fourth_decide_what_draws`: a curve solid to 40 dBZ and clear from 45 dBZ hides a 50 dBZ block (0 px) only because stops five and six are read — its first four alone draw it (64,768 px); the existing curve and translucent GPU tests still pass. Still open for M3.5: colour stops (colours still come from the product palette), multiple isosurface thresholds, empty-space skipping, device performance traces.

#### M3.6 — Link storm-centered 3D, slices, and measurement

**Priority:** P1. **Depends on:** M2.2, M3.3, M3.5. **Original references:** ROADMAP_2 §11; ROADMAP_NEW §§C3, H4, H5, H6, H8, J3.

- **Starting evidence:** radar-wide volume bounds, Cell View in 3D, cross-section lines, vertical clip/slab controls, CAPPI reference, and frame-keyed 3D playback.
- **Outcome/build:** build higher-detail geographic ROI volumes around a selected storm or drawn region. Share slice endpoints/plane, CAPPI altitude, crosshair, and selection between map, cross-section, and 3D. Add translation/rotation controls and rulers for horizontal/slant distance, MSL, antenna-relative height, terrain AGL when available, and sampled value.
- **Interfaces/compatibility:** cache identity includes ROI, product, data revision, grid, and coverage policy; camera/style changes reuse data. Environmental surfaces use M3.3's source/time/datum, with independent visibility. Optional follow mode translates with the selected history, keeping its behavior explicit.
- **Failure/compatibility:** lost association stops following without jumping to an unrelated storm. Missing DEM disables true AGL; the 4/3-earth beam model remains an approximation. Slow/failed 3D builds retain an explicitly older frame or show unavailable state, never a false current timestamp.
- **Acceptance:** translating/rotating a cut updates linked views; camera motion does not regrid; ROI improves measured detail; 3D/probe/cross-section samples agree within stated resampling tolerance; replay preserves camera and frame identity on both native platforms.
- **Proof:** geometry/sample tests, cache/build counters, storm-centered walkthrough, and device playback captures.

**Agent prompt:** `Implement M3.6 using section 6. Link existing slice/probe tools around a storm-centered ROI and prove datum, cache, and playback consistency.`

**Status:** implementation: in progress (direct cross-section editing and 3D cut link, increment 1); verification: partial (geometry unit tests; no device interaction).

**Evidence ledger — 2026-10-06, increment 1 (cross-section ergonomics):**

- *User workflow delivered:* with the cross-section tool armed, the section line on the map has four handles — A, B, the middle (slides the whole line, keeping length and bearing) and a rotation handle on an arm to the line's right (swings it about its middle); Shift snaps a swing or an endpoint bearing to 15°; Esc, a lost focus or a cancelled touch puts the line back as it was. A tap on a handle is a handle, not a new endpoint. The same primary-pointer path serves mouse, touch and pen, with a 24 pt grab radius on touch screens. The window adds exact bearing and length entry, 1/5 km perpendicular slides, a radial snap through the radar (keeps the middle and the nearer direction), A⇄B, and **3D cut**, which drives the 3D view's vertical clip plane along the section as a 4 km slab while it moves ([`app/xsection_edit.rs`](crates/hookecho/src/app/xsection_edit.rs)). A provenance line states site, moment, tilts, the span the contributing tilts' radials were scanned over, and the share of the panel inside real beam coverage. The section is rebuilt from the gates (`wxdata::xsection::build`, unchanged) on every edit and whenever its pane's volume or revision changes, so a live tilt or a scrub never leaves an old scan under the panel.
- *Interfaces/compatibility:* geometry is spherical (`crate::geo`) like the 3D plane's ground track; `SectionLine::plane` is the inverse of `render3d::plane_ground_track` (tested). No settings or workspace format changes.
- *Commands run and results:* `cargo test -p hookecho --lib xsection` 7 passed (edits keep length/bearing/middle, each handle moves only what it holds, Shift snapping, radial snap through the radar, handle hit radii for cursor vs fingertip, window edits applied in order and an unchanged window changing nothing, the 3D cut lying on the section within 0.5 km); the existing cross-section golden is unchanged; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace --no-fail-fast` 2,339 passed, 0 failed; GPU suite 36 passed (llvmpipe); wasm32 check passes.
- *Desktop/Android/browser evidence:* none interactive. Pen and touch share the code path but were not exercised on hardware.
- *Known limitations:* one section at a time, sampled on the UI thread (300×120 nearest-gate grid; cheap, but unprofiled on Android); no 3D in-view section panel or ruler yet; no storm-follow mode; the radial snap swings about the middle rather than also centring on a storm.
- *Next ready increment:* M3.1 decoded Nyquist/metadata at the Level II boundary; then M3.6 rulers and storm-centred ROI.

**Evidence ledger — 2026-10-06, increment 2 (3D sampling agreement and mode regressions, gap 4):**

- *Fixed:* the smooth volume and CAPPI builders mapped ground range to slant range with the flat-earth `ground / cos(elev)`, while the cross-section, derived grids and column products use the 4/3-earth form; beyond ~250 km a voxel read the neighbouring gate. All three volume samplers (shells, smooth volume, CAPPI) now go through `xsection::gate_over_ground`, so one point reads one gate in every view. `volume3d::tests::a_voxel_column_reads_the_cross_sections_gates` checks every sampled voxel against `xsection::column_samples` + `sample_profile` out to 345 km (it fails on the old code at 345 km: code 38 vs 45).
- *Regressions added ([`headless_volume.rs`](crates/hookecho/src/headless_volume.rs)), on a synthetic storm with closed-form geometry:* the 45 dBZ isosurface's low vertices lie 15 ± 3.5 km from the core and every vertex sits in ≥35 dBZ air, the 30 dBZ skin reaching further (CPU, every PR). `gpu_volume_modes_draw_where_the_storm_is` (GPU suite): the smooth MIP draws east of centre where the core is and replays byte-identically; a 70 dBZ floor draws nothing; a 45 dBZ floor draws a core of the expected area ratio; a half-space keeping the east keeps the storm and one keeping the west draws nothing; a thin slab through the core draws a band, one 60 km west draws nothing, and the same slab from the side draws the vertical section. llvmpipe counts: smooth 7,033, floor-45 756, east 7,033, slab 1,168, side 850.
- *Commands:* clippy `-D warnings` clean; `cargo test --workspace --no-fail-fast` 2,344 passed, 0 failed; GPU suite 38 passed; wasm32 check passes.
- *Open:* no GPU regression yet for the observed-gates (instanced) representation or the map-pitched smooth path (these cover the orbit raymarch); no real-volume 3D reference; camera/orbit, ROI, ruler and direct-sampling interaction work remains.

**Evidence ledger — 2026-10-06, increment 3 (cross-section ruler):** the cross-section window has a Ruler: drag across the panel to measure between two points ([`ruler_reading`](crates/hookecho/src/ui/xsection_window.rs)) — ground distance along the cut, height difference, straight-line distance, and for each end its height above the radar antenna (the panel's own axis, now labelled so in the hover and caption instead of "km up"), its height above mean sea level from the site's antenna altitude (site elevation plus tower), and the value the panel holds there, flagged when it is held over from the nearest beam rather than sampled. Ground level is stated as unknown (no terrain data), never assumed; with no site altitude the MSL height says so. The ruler belongs to the window and is dropped when the cut changes. Test: `the_ruler_reads_distances_both_datums_and_the_panel_value` (distances both ways round, MSL from antenna altitude, the panel's value and coverage at the end, the unknown-datum wording); full workspace 2,374 passed, Clippy `-D warnings`, the WASM library check. Open: rulers in the 3D view and on the map, terrain AGL (needs a DEM), ROI volumes.

**Evidence ledger — 2026-10-06, increment 4 (map measure names its datums):** the map's measure tool labelled the beam over the far end "beam N ft", which is height above the radar antenna. It now reads "beam N ft above the radar (M ft MSL)", the MSL figure from the site's elevation plus tower ([`app/measure.rs`](crates/hookecho/src/app/measure.rs), moved out of `app.rs`, ceiling lowered to 9,000); height above ground is not offered without terrain data. Test: `the_beam_height_names_what_it_is_measured_from`; full workspace 2,377 passed, Clippy `-D warnings`, the WASM library check.

**Evidence ledger — 2026-10-06, increment 5 (terrain: height above ground):** [`wxdata::terrain`](crates/wxdata/src/terrain.rs) reads ground elevation from the public AWS Terrain Tiles (Terrarium PNGs: SRTM, USGS 3DEP and other public sources; no key) at zoom 11 — about 60 m per pixel at mid-latitudes — sampled bilinearly, with the grid's resolution carried into every answer. The app keeps fetched tiles for the session ([`terrain_cache`](crates/hookecho/src/app/terrain_cache.rs)) and never waits for one: a read gives the height when the tile is here, "loading" while it comes, "unknown" when it cannot be had, never a guessed ground. The map measure tool now reads the beam over the far end as above the radar, above MSL and above the ground ("… ft above the ground (terrain on a 62 m grid)"); the cross-section ruler gives each end's height above the ground; and the cross-section panel draws the ground wherever it rises above the radar antenna, where low beams may run into it. Tiles are fetched only for the points a tool asks about (the measure line's far end; the cut's line). Test against an independent value: [terrain_sites](crates/wxdata/tests/terrain_sites.rs) reads pinned real tiles under three radars and compares with each site's surveyed elevation in the NEXRAD registry — KTLX 364.9 m vs 370 m, KFTG 1,675.3 m vs 1,675 m, and KMSX (a mountaintop radar, above its smoothed grid cell) 2,411.9 m vs 2,394 m; plus tile-math, malformed-tile, measure-label (known, loading, unknown) and ruler (with ground, loading) tests; full workspace 2,394 passed, Clippy `-D warnings`, the WASM library check. Limitations: the terrain is a grid, not a survey; the beam model stays the 4/3-earth approximation; beam blockage is drawn, not computed.

**Evidence ledger — 2026-10-07, increment 6 (storm-centred region of interest):** the 3D map's Slice section has **Region**: the whole radar, or a box 30/50/80/120 km across around the selected storm. The smooth volume is then built over that box only ([`volume3d::build_at`](crates/wxdata/src/volume3d.rs)): the same voxel budget over a smaller box gives finer cells (the Slice section's cell-size line shows it), and every voxel still reads its gate by its own range and azimuth from the radar, so where a region and the whole-radar volume overlap their voxels are identical (tested exactly on a shared 1 km grid). The region's centre is carried on the upload (`Volume3dUpload::center_km`), the map raymarch box is offset by it (`map_uniform`, tested: moved by exactly its offset, same size and heights), the cache key includes it (`SmoothKey::roi`), and the vertical plane, its map ground track and the cross-section→3D cut are taken about the box's centre. **Follow the storm** moves the region with the selected storm as new scans arrive; it stops, leaving the box where it was, when the storm leaves the table (saying so) or another storm is selected, never jumping to an unrelated storm. The Inspector's 3D card names the region. Tests: `a_region_around_a_storm_agrees_with_the_whole_volume_and_is_finer` (wxdata), `a_region_of_interest_builds_finer_cells_where_it_is_put` (cells under half the whole-radar size, grid equal to `build_at`, cache keys differ by size), `a_region_lands_where_its_storm_is_from_the_radar`, `a_region_box_sits_east_and_north_of_the_radar_by_its_offset`; the 3D GPU suite (MIP, translucent, velocity, coverage snapshots) still passes on llvmpipe. Open: isosurfaces, the Observed representation and the 3D Reflectivity window remain radar-wide; no drawn (non-storm) region; no device walkthrough or build-time trace.

**Evidence ledger — 2026-10-07, increment 7 (beam diagram; WeatherWise-class beam-rise visualization):** **Beam diagram** (command palette) draws every distinct tilt of the active pane's scan against range out to 100/230/460 km — beam centre and half-power edges, km MSL over the radar's own ground level — and reads them at the range under the pointer on the active pane (a range slider stands in on touch): each tilt's bottom–top and centre height there, and the heights no beam samples (below the lowest beam and between tilts whose spacing exceeds the beam), drawn in red on the cursor line. Geometry is [`beam_geometry::{beam_at_ground, tilt_profiles, unsampled_at}`](crates/wxdata/src/beam_geometry.rs): the inverse of the existing 4/3-earth `beam_point` for a ground range, each edge as its own ray to the same ground point, SAILS/MRLE repeats counted once. Tests: inverse agrees with `beam_point` to 1 mm for 0.5–19.5° at 10–230 km; 0.5° at 100 km is 1.4–1.5 km above the antenna; for VCP 212 the low tilts overlap near the radar (only a sub-100 m strip below the lowest beam, gaps only above 4°) and miss over three times as much of the column at 200 km; the readout's words. A GPU capture (`gpu_beam_diagram_snapshot`, KTLX, VCP 212, 120 km) is written under `target/parity-review/beam-diagram`. Stated limits on the window itself: standard refraction and the WSR-88D 0.925° beam; heights are above the radar's ground, not the terrain under the cursor (the cross-section ruler and map measure carry terrain AGL).

### M4 — Operational GIS

#### M4.1 — Introduce independent GIS layers and groups

**Priority:** P1. **Depends on:** M0.2, M1.4. **Original references:** ROADMAP_2 §5; ROADMAP_NEW §§I1–I5, J1.

- **Starting evidence:** single imported layer settings, normalized `GisFeature`, imported polygon/mark rendering, styles, label/time mappings, and workspace capture.
- **Outcome/build:** add a collection owner and manager with stable layer IDs, source references, independent styles, labels, time mapping, visibility, order, and named groups. Shared assets and weather layers can coexist without replacement when another file is imported.
- **Interfaces/compatibility:** migrate the current single import and all companion settings into one collection entry. Per-layer scientific/source metadata feeds M1.4; asset target geometry feeds M2.3 without a second import model.
- **Failure/compatibility:** preserve missing-file references with actionable repair/locate/remove actions. Group visibility does not overwrite child visibility; deleted IDs are diagnosed in scenes/workspaces rather than binding to a similarly named layer.
- **Acceptance:** multiple points/lines/polygons retain separate settings and restore after restart; old settings migrate exactly once; hide/reorder/time changes affect drawing and hit-testing consistently; Android can manage the same collection.
- **Proof:** migration fixtures, collection state tests, and restart/ordering captures.

**Agent prompt:** `Implement M4.1 using section 6. Replace the single imported-layer owner with an additive collection and preserve all existing import/style settings.`

**Evidence ledger — 2026-10-06, increment 1 (`1eb328d`, the collection):** importing a file no longer replaces the last one. `settings.gis_layers` ([`GisLayerConfig`](crates/hookecho/src/settings.rs)) holds every imported layer with a stable, never-reused ID, its source reference (a path, or a browser `web_files` name), name, visibility, style, label, colour-by and time mappings, side of the official products and named group; `settings.gis_groups` holds group switches, and a hidden group hides its layers without overwriting their own visibility. The list order is the paint order. The pre-M4.1 single layer and every companion setting migrate into the first entry exactly once (`migrate_imported_gis`, at load and when a settings bundle is applied); the legacy fields are read but never written again. What was read from each file lives in [`app/gis_layers.rs`](crates/hookecho/src/app/gis_layers.rs), keyed by the same ID: polygons join the shared overlays with a parallel `overlay_layer` record, so tessellation (`overlay_build::build_layered`) gives each imported feature its own layer's outline width and minimum zoom, hit-testing (`overlay_hits`) leaves out only the layers hidden at the current zoom, and each layer's time filter and colours are its own. Points, lines and labels paint per layer in paint order, labels decluttered across layers with the topmost first. Re-importing a file that is already a layer refreshes its shapes and keeps its settings. A layer whose file cannot be read at launch stays listed as "missing" with the reason (one toast for all failures), to be re-imported or removed; removing a layer removes the browser-stored content only it used (it was orphaned before). The Layer Manager lists the layers (visibility, select, order, zoom-to, remove, feature count or missing state) and group switches, and edits the selected layer's name, group, style, labels, colouring, time and side. The zoom-to, time/colour sync and reload moved out of `app.rs` (ceiling lowered to 9,052). Tests: `the_single_imported_layer_migrates_once_with_all_its_settings`, `gis_layers_keep_their_own_ids_order_groups_and_content`, `layers_paint_in_their_own_order_on_their_own_side_and_only_when_shown`, `a_layer_keeps_its_own_time_filter_and_colours`, `each_imported_feature_reads_its_own_layers_width_and_zoom`, the settings round trip with a layer and group, and the existing GIS import/tessellation tests unchanged; full workspace 2,360 passed, Clippy `-D warnings`, the WASM library check. No restart/ordering captures or Android walkthrough yet. Open: workspaces and scenes do not capture layers (M4.4, where deleted IDs are to be diagnosed); per-dataset picker for multi-dataset bundles (M4.2); a "locate" action for a missing file beyond re-importing it; imported points and lines are still neither clickable nor exported (M4.3/M4.4).

#### M4.2 — Complete native imports with shapefile bundles

**Priority:** P1. **Depends on:** M0.3, M4.1. **Status:** implementation: in progress (zipped bundles; whole-bundle and stored-entry bounds); verification: partial. **Original references:** ROADMAP_NEW §§I1, I2, I3, Q1; ROADMAP_2 §5.1.

- **Starting evidence:** shapefile geometry/DBF parsing, `.prj` projection handling, KMZ bounded ZIP decoding, platform file handover, and current sidecar omission.
- **Outcome/build:** accept ZIP shapefile bundles with `.shp`, `.dbf`, optional `.shx`, `.prj`, and `.cpg`. Match sidecars case-insensitively by basename. For multiple datasets, show a dataset picker. Preserve content in app-managed Android storage so a temporary picker URI is not the long-term source.
- **Interfaces/compatibility:** reuse existing geometry/projectors and archive decoding seams. Support UTF-8, Windows-1252, and Latin-1 codepage declarations initially; diagnose other encodings. Enforce bounded expansion/read/allocation against platform resource policy, with cancellation and import summary.
- **Failure/compatibility:** reject mismatched DBF rows, ambiguous sidecars, unsupported CRS/datum, encrypted/unsupported ZIP forms, and excessive expansion clearly. No network-link fetching is implied by KML import. Plain `.shp` imports still work and disclose absent attributes.
- **Acceptance:** real Census county/place and representative emergency-asset exports import on desktop/Android with matching attributes and projected position; malformed/truncated archives stay bounded; non-WGS84 fixtures match independent GIS coordinates.
- **Proof:** attributed real-world fixtures, archive/parser adversarial checks, and complete Android picker/import walkthrough.

**Agent prompt:** `Implement M4.2 using section 6. Reuse existing GIS decoders to import complete shapefile bundles on Android and desktop, preserving CRS and attributes.`

**Evidence ledger — 2026-10-01, increment 1 (`770a881`):** a `.zip` brings a shapefile in with its sidecars, the way a phone or browser picker can hand over one file. [`shapefile::parse_zip`](crates/wxdata/src/shapefile.rs) matches `.dbf`, `.prj` and `.cpg` to each `.shp` by path and name, case-insensitively; macOS resource forks are ignored; case-twin sidecars, a zip with no shapes and a broken dataset are refused by name. UTF-8, Windows-1252 and Latin-1 `.cpg` declarations are honoured and others reported. Several datasets import together as one layer with a note naming them (a per-dataset picker belongs with M4.1's collection). The KMZ reader's archive code moved to the shared bounded [`wxdata::zip`](crates/wxdata/src/zip.rs) (stored and deflated only; Zip64, encryption and other methods refused by name).

**Evidence ledger — 2026-10-05, increment 2 (`d7228b1`):** bounded expansion for the bundle as a whole. The per-file (512 MB) and per-bundle (256 datasets) caps still let one small zip expand to many gigabytes; `parse_zip` now holds everything a bundle inflates to under 1 GiB, refused up front from the declared sizes and again while inflating, since a declared size is only the archive's word. `zip::read` checked a stored entry only by its declared size and returned whatever it held; it now checks the bytes themselves, as deflated entries already were. Tests construct both lies (understated central-directory sizes, stored and deflated). Windows wxdata zip/shapefile tests (33), app GIS tests (52), wxdata Clippy and the WASM library check passed on the shared tree at `9b8c4ef`; Codex's concurrent model-pane work was preserved outside the commit. Open: a dataset picker (with M4.1), app-managed Android storage of picked files, real Census county/place and emergency-asset fixtures checked against independent GIS coordinates, and an Android picker/import walkthrough.

**Evidence ledger — 2026-10-05, increment 3:** a real attributed bundle checked against independent coordinates. [gis_census](crates/wxdata/tests/gis_census.rs) imports the Census Bureau's 2023 cartographic county boundaries (`cb_2023_us_county_20m.zip`, 900,375 bytes as published: `.shp`, `.shx`, `.dbf`, NAD83 `.prj`, UTF-8 `.cpg`, two metadata XMLs) through `parse_zip`, and checks every county polygon, found by the GEOID in its own `.dbf` row, against the internal point the 2023 Census Gazetteer publishes from full-resolution TIGER boundaries. 3,222 counties import with no notes, all have a Gazetteer point, and 3,215 contain it; the seven that do not are coastal or island counties whose points fall just outside the 1:20M generalized outline (San Francisco, Island WA, Door WI, Poquoson VA, Franklin FL, Kalawao HI, Knox ME). Spot checks place Chicago in Cook and downtown Oklahoma City, but not Norman, in Oklahoma County. Both fixtures are public domain, committed with source paths, sizes and SHA-256 in the test. Still open: a projected (non-geographic) real-world fixture, an emergency-asset export, the dataset picker and Android storage/walkthrough.

**Evidence ledger — 2026-10-06, increment 4 (projected real-world fixtures against PROJ):** [gis_projected](crates/wxdata/tests/gis_projected.rs) imports Oklahoma's 77 counties from the pinned Census 2023 boundaries in NAD83 geographic and as PROJ reprojected them (GDAL 3.8.4 `ogr2ogr -t_srs`, `.prj` as GDAL writes it) into NAD83 / UTM 14N (EPSG:26914, Transverse Mercator), NAD83 / Oklahoma South in US survey feet (EPSG:2268, two-parallel Lambert Conformal Conic) NAD83 / CONUS Albers (EPSG:5070) and WGS 84 / Pseudo-Mercator (EPSG:3857) — five zipped bundles of about 15 KB each, committed with sizes and SHA-256 in the test. Every county is matched by the GEOID in its own `.dbf`, and every one of its 1,154 vertices after this app's inverse projection is compared with the geographic vertex PROJ made it from: the worst is 5.2 mm (UTM), under 0.1 mm for State Plane, Albers and Web Mercator; the test fails above 1 cm. This closes the "non-WGS84 fixtures match independent GIS coordinates" item for these four projection families. Open: an emergency-asset export, the dataset picker, app-managed Android storage and walkthrough.

**Evidence ledger — 2026-10-06, increment 5 (KML/KMZ from an independent writer; multi-layer KMZ fixed):** GDAL 3.8.4 wrote the same 77 Oklahoma counties as KML (`-f KML -dsco NameField=NAME`) and as KMZ (`-f LIBKML`), committed as `ok_counties_gdal.kml` (94,702 bytes, SHA-256 `fcf9a491…a687`) and `ok_counties_gdal.kmz` (15,003 bytes, SHA-256 `aba9fcc6…6603`). [gis_kml](crates/wxdata/tests/gis_kml.rs) requires every county to import with its GEOID from `ExtendedData`, its name and every vertex where the NAD83 shapefile has it. The KML did, exactly. **The KMZ imported nothing:** LIBKML lays a KMZ out as a `doc.kml` holding only a `NetworkLink` to `layers/ok_nad83.kml`, and the importer read only the main KML. `kml::parse_kmz` now also reads the KML files inside the same archive that the main KML links to — by relative path only, two levels deep, 64 files at most, each once; a link to the web, by an absolute path or out of the archive with `..` is never followed, so no network fetching is implied. Both now match exactly (0° vertex difference). GDAL's GeoJSON in three conventions is pinned beside them and lands on the shapefile too: with a legacy `crs` naming NAD83 (worst 1e-13°), as RFC 7946 (rings rewound counter-clockwise, 7 decimals; every vertex within 5e-8° of one of its county's own), and with a legacy `crs` naming EPSG:3857 in metres (inverse-projected, worst 3e-14°) — `gdal_geojson_in_three_conventions_lands_on_the_shapefile`. Tests: `gdal_kml_and_kmz_import_with_their_attributes_and_vertices` (failed on the old code: 0 of 77), `a_kmz_reads_the_layers_its_main_kml_links_inside_the_archive_and_nothing_else` (duplicate, `..`, `http:`, absolute and too-deep links); full workspace 2,390 passed, Clippy `-D warnings`, the WASM library check.

#### M4.3 — Finish styling, feature inspection, and filtering

**Priority:** P1. **Depends on:** M4.1, M4.2. **Original references:** ROADMAP_2 §§4.3, 5.2–5.4; ROADMAP_NEW §§I4, I5.

- **Starting evidence:** category/graduated colors, fixed dot symbols, label attributes, minimum zoom, polygon click popups, and imported priority.
- **Outcome/build:** add typed attribute filters, separate fill/stroke/opacity, line dashes, point symbols/size, min/max zoom, label templates/halo/priority, and a feature table. Clicking points, lines, or polygons selects the same source feature; support copy values, zoom to selection, and export selected/filtered items.
- **Interfaces/compatibility:** filter expressions use a bounded parser with string/number/boolean comparisons; missing attributes remain missing. Per-layer legends explain categorical/graduated choices. Style changes do not mutate source values or geometry.
- **Failure/compatibility:** invalid filters retain the previous valid filter and show a diagnostic. Official warnings keep operational hit-test priority; ambiguous nearby features remain selectable through a chooser. Expensive filtering runs off-frame and cancels on edits.
- **Acceptance:** filters, time ranges, zoom visibility, feature table, export, and hit-test sets agree; selected features remain legible without destabilizing labels; Android has tap-accessible inspection and filter controls.
- **Proof:** typed-filter fixtures, point/line/polygon hit tests, and dense asset-layer captures.

**Agent prompt:** `Implement M4.3 using section 6. Complete per-layer rules and shared feature inspection without modifying original geometry or warning interaction priority.`

**Evidence ledger — 2026-10-06, increment 1 (points and lines clickable and exported):** imported points and lines were drawn but could not be clicked, and the map's GeoJSON export dropped them and flattened imported polygons' attributes into a text field. A click now finds the imported point or line under it ([`mark_at`](crates/hookecho/src/app/gis_layers.rs): within the drawn symbol or stroke plus 4 px, 12 px on a touch screen; points before lines; topmost layer first; features outside the view's time left out) and opens the same attribute popup a polygon does, naming its layer; an alert under the click still takes precedence. The export writes every shown imported feature — polygons, lines and points — with the attributes its file gave it plus `hookecho: "imported"` and `layer`, and the official overlays as before. Tests: `a_click_finds_the_point_or_line_under_it_and_not_one_out_of_its_time`, `imported_points_and_lines_export_with_their_own_attributes` (read back through the app's importer); full workspace 2,362 passed, Clippy `-D warnings`, the WASM library check. Open: attribute filtering and rule-based styling, per-feature inspection beyond the popup (a table), Android touch walkthrough.

**Evidence ledger — 2026-10-06, increment 2 (typed attribute filters):** each imported layer takes an attribute filter ([`gis_filter`](crates/hookecho/src/gis_filter.rs)): comparisons `= != < <= > >=` and case-insensitive `contains`, `is [not] missing`, combined with `and`/`or`/`not` and parentheses; names with spaces in backticks. Comparisons are typed — number with number, text with text, `true`/`false` with a boolean — and anything else, including an absent or null attribute, is unknown rather than false, carried through the logic three-valued, so a feature missing the attribute is not shown and is never read as zero or empty text. The parser is bounded (1,000 characters, nesting 32, 200 terms). The filter's mask and the time window's combine into the layer's one visibility mask, so drawing, clicks, labels, the GeoJSON export and impact targets all show the same features. A filter that does not parse keeps the previous one in force and the Layer Manager shows why; the count line reports how many features are shown. Tests: `typed_comparisons_and_logic`, `a_missing_or_mistyped_attribute_is_unknown_never_zero_or_empty`, `bad_filters_say_why_and_are_bounded`, `a_layer_filter_and_its_time_window_decide_together`; full workspace 2,384 passed, Clippy `-D warnings`, the WASM library check. Open: off-frame filtering for very large layers, a feature table with selection/copy/zoom, separate fill/stroke styles, dashes, point symbols, max zoom, label templates.

**Evidence ledger — 2026-10-06, increment 3 (feature table):** each imported layer opens a feature table from its Layer Manager row ([`feature_table`](crates/hookecho/src/app/gis_layers.rs)): every source feature the layer shows — its filter and time window applied, the same mask the map, clicks, labels and exports read — one column per attribute, sortable by any column (numbers as numbers, text ignoring case, empty values last; again to reverse, again for file order) and searchable across all columns. Each row has Zoom, which frames the feature's every part (`feature_bounds`) and outlines it on the map over everything else, and Copy, which puts its attributes on the clipboard as `name: value` lines. Rows are buttons rather than row clicks, which a touch screen does not deliver inside a scrolled table (as the site dialog already found on Android); at most 500 rows are listed, with a note to search. Test: `the_feature_table_lists_what_the_map_shows_sorted_and_searched`; full workspace 2,395 passed, Clippy `-D warnings`, the WASM library check. `point_in_ring_ll` moved from `app.rs` to `map_click.rs` (ceiling lowered). Open: a map click selecting the same row, an ambiguous-hit chooser, export of the table's selection, separate fill/stroke styles, dashes, point symbols, label templates, Android capture.

**Evidence ledger — 2026-10-06, increment 4 (map pick = table row):** clicking an imported polygon, line or point on the map now picks the same source feature in that layer's open feature table, outlined on the map like a pick made in the table. Each drawn polygon part carries the source feature it was made from (the overlays' per-feature layer record now holds layer and feature), so the two parts of a multipolygon pick one row and a filtered-out feature none. Test: `drawn_polygons_name_their_source_feature`; full workspace 2,396 passed, Clippy `-D warnings`, the WASM library check. Open: a chooser for ambiguous overlapping hits, export of the picked/searched rows.

**Evidence ledger — 2026-10-09, increment 5 (row export and a chooser for overlapping features):** the feature table exports what it lists — every row its search leaves, or the rows picked in it — as GeoJSON (`layer_features_of` takes the source rows to keep and still applies the layer's filter and time window, so a row picked before a filter edit that now hides it is not exported; the file is named `<layer>-listed`, `-search` or `-picked`; nothing left gives a toast instead of an empty file). A click that lands on more than one feature — overlapping imported polygons, a point on a line, an imported shape under a Mesoscale Discussion or watch — no longer opens whichever came first: a "What's here" window ([`feature_chooser`](crates/hookecho/src/app/feature_chooser.rs)) lists each with its layer's colour swatch and opens the one chosen (touch-sized rows; Back closes it on Android). [`mark_hits`](crates/hookecho/src/app/gis_layers.rs) returns every point and line under the click, points before lines and the last drawn first, each source feature once, so a single hit still opens directly as before. Official warnings keep their priority: a click on a warning opens its card and never the chooser. Tests: `a_click_finds_the_point_or_line_under_it_and_not_one_out_of_its_time` (now also that a point on a line gives both, point first) and `a_layer_filter_and_its_time_window_decide_together` (the subset export keeps only rows the layer still shows); full workspace passed, Clippy `-D warnings`, the WASM library check. Real app (release build on Xvfb, two overlapping imported polygon layers): a click inside both opened "What's here · 2 features under that spot" listing each, and choosing one opened its attribute popup. Open: dense asset-layer captures and an Android walkthrough of the chooser and filter controls.

**Evidence ledger — 2026-10-09, increment 6 (the rest of the per-layer styling):** each imported layer now has, beside its colour, outline width and opacity ([`ImportedGisStyle`](crates/hookecho/src/settings.rs)): a fill colour of its own and a fill opacity (0 draws outlines only; with a colour-by attribute a separate fill is what the attribute colours, the outline keeping the layer colour); solid, dashed or dotted lines and polygon outlines; a point symbol (circle, square, triangle, diamond, cross) and size (auto keeps the old size from the outline width; clicks use the drawn symbol's size); a maximum zoom beside the minimum (a state outline giving way to its counties), which hides the layer for drawing, clicks, labels and hit sets alike through the one `visible_at`; and a label template (`{NAME} ({POP})`, `{{`/`}}` for braces) used instead of the label attribute, a feature with none of the named attributes left unlabelled, also naming impact targets. Polygon outlines are dashed in the overlay tessellator ([`build_layered`](crates/hookecho/src/overlay_build.rs) takes each imported feature's width and dash; official products are never dashed), the pattern running on round each ring, at most 200,000 dashes a build and solid past that, so a dashed county layer zoomed far in cannot stall a rebuild; dots are slivers with round caps. Styles change only paint: geometry and attributes are untouched, and a style saved before these existed reads back drawing exactly as it did. A change to the outline width, dash or fill now re-tessellates at once (the width used to wait for the next zoom step). Tests: `imported_gis_style_extras_default_to_the_shipped_look`, `a_label_template_fills_in_attributes_and_skips_features_with_none`, `a_dashed_outline_covers_its_share_of_the_solid_one` (the dashed outline's triangle area is the pattern's on-share of the solid one within 5%, dots less, a warning never dashed), `dashes_past_the_budget_fall_back_to_solid`, the settings round trip with every new field; full workspace passed, Clippy `-D warnings`, the WASM library check. Real app (release build on Xvfb, four styled layers over the 20 May 2013 KTLX archive at zoom 7.6): a green-filled polygon with a white dashed outline, an unfilled one with a dotted orange outline, dashed yellow lines, and magenta triangles labelled `{NAME} ({POP})` all drew as set, under the radar's own warnings. A site without POP reads "Chickasha ()": a template's literal text stays when its attribute is missing. Open: rule-based styles (per-value symbols beyond colour), label priority beyond paint order, dense asset-layer captures and an Android walkthrough.

#### M4.4 — Restore and exchange GIS scenes reliably

**Priority:** P1. **Depends on:** M2.4, M4.3. **Original references:** ROADMAP_2 §§5.5, 12.2; ROADMAP_NEW §§I6, K3, K4, M5.

- **Starting evidence:** workspace/case formats, visible-map GeoJSON export, route geometry, contour algorithms, and analysis ZIP export.
- **Outcome/build:** persist GIS collections/groups and styles in workspaces and scenes. Add selected/filtered GIS export, true manual projected-track geometry, existing route geometry, and available threshold/contour vectors with source/time/unit metadata. Keep the default visible-map export behavior intact.
- **Interfaces/compatibility:** version portable manifests as needed and resolve layer IDs/assets on import. Package user-chosen assets or retain explicit external references with checksums; do not bundle secret settings or unrelated user files. Normalize vector output to documented geographic coordinates.
- **Failure/compatibility:** report missing assets and unsupported exports before completion; route export uses existing route data rather than waiting for a new route engine. Invalid/open rings and no-data contours are handled consistently.
- **Acceptance:** old workspaces/cases open; new scenes restore multiple layers; selected/filtered exports round-trip; GeoJSON/GeoTIFF outputs open in independent GIS tools with correct coordinates, values, units, and time.
- **Proof:** manifest migration tests, QGIS or equivalent independent-reader reports, and restore/export walkthroughs.

**Agent prompt:** `Implement M4.4 using section 6. Complete portable GIS/workspace persistence and export existing analytical geometry with independent-tool verification.`

**Evidence ledger — 2026-10-06, increment 1 (layers in workspaces and scenes):** workspaces and broadcast scenes now save the imported GIS layers as they were — the master switch, every layer's state (visibility, style, labels, colouring, time mapping, side, group) and order by its stable ID, and the group switches ([`GisSnapshot`](crates/hookecho/src/settings.rs), an optional `gis` field in both formats). Applying one restores each layer it names by ID and source, hides a layer imported since (it was not part of that view), and restores group switches; a layer it names that has since been removed is reported by name and source in a toast — never bound to another layer, even one re-imported from the same file, since IDs are never reused. A workspace or scene saved before this, or with no layers imported, carries no snapshot and leaves the layers alone. The file content itself is not packaged: layers are references into this installation's collection (portable bundles with checksummed assets remain open). Test: `a_gis_snapshot_restores_layers_by_id_and_reports_the_removed` (order, style, group switch, later import hidden, removed-and-reimported layer reported, old scene without the field, scene round trip); existing workspace/scene tests unchanged with `gis: None`; full workspace 2,363 passed, Clippy `-D warnings`, the WASM library check. Open: selected/filtered GIS export; manual-track, route and contour vector export with metadata; portable manifests that package or checksum layer files; QGIS (independent reader) reports.

**Evidence ledger — 2026-10-06, increment 2 (filtered layer export):** each imported layer's row in the Layer Manager exports the features that layer shows — valid at the view's time and passing its filter — as GeoJSON with each feature's own attributes, `hookecho: "imported"` and the layer name (`layer_features`, the same function the whole-map export uses per layer, so the two cannot disagree). The default visible-map export is unchanged. Test: the layer export writes exactly the filtered features, in `a_layer_filter_and_its_time_window_decide_together`; full workspace 2,384 passed, Clippy `-D warnings`, the WASM library check. Open: selection-based export (needs the feature table), manual-track/route/contour vectors with metadata, QGIS reader reports.

**Evidence ledger — 2026-10-06, increment 3 (manual tracks in the map export):** the map's GeoJSON export now carries every manual storm-motion track as true projected geometry (`ManualTrack::to_features`): the path of the storm (or a line's middle) every 5 minutes over the hour, and the closed uncertainty-swath polygon the map draws, each with `t0` (the analysis time it projects from), horizon, bearing, speed (km/h), both widths, cone, the source SCIT cell and scan, and whether it is a historical estimate. Coordinates are WGS84 `[lon, lat]`. Test: `a_track_exports_its_path_and_swath_with_its_motion_and_time` (60 km due east over the hour, closed ring, metadata, read back through the importer); full workspace 2,385 passed, Clippy `-D warnings`, the WASM library check. Open: route geometry and threshold/contour vectors, QGIS reader reports.

**Evidence ledger — 2026-10-06, increment 4 (routes in the map export):** the map's GeoJSON export carries the planned routes as the routing provider returned them — its road geometry, distance (m) and duration (s) at full precision, its summary, the engine (OSRM/Valhalla) and which route was chosen — using the existing route data, not a new engine. A route with fewer than two points is left out. Test: `routes_export_with_their_distance_time_and_choice`; full workspace 2,386 passed, Clippy `-D warnings`, the WASM library check. Open: threshold/contour vectors with source/time/unit, QGIS reader reports.

**Evidence ledger — 2026-10-06, increment 5 (contour vectors in the map export):** the map's GeoJSON export carries every active model contour layer's lines ([`contour_features`](crates/hookecho/src/app/contours.rs)), each as a WGS84 line with its field, level, display unit (null when the field has none — never guessed), model, run and valid time, in the layers' own order so the same map writes the same file. Lines of fewer than two points are left out. Test: `contour_lines_export_with_field_level_unit_and_times`; full workspace 2,387 passed, Clippy `-D warnings`, the WASM library check. Open: radar threshold outlines as vectors, QGIS reader reports, portable manifests packaging layer files.

**Evidence ledger — 2026-10-06, independent-reader check (GDAL/OGR 3.8.4):** an ignored test (`write_export_sample`, run with `HOOKECHO_EXPORT_SAMPLE=<path>`) writes one map export holding every kind the export now carries — a storm cell, imported point/line/polygon with their attributes, a manual track's path and swath, a planned route, and a model contour. Read with GDAL 3.8.4's `ogrinfo` (installed in the container, not part of the build): the GeoJSON driver opens it as EPSG:4326 with 8 features; `ST_IsValid` is true for every geometry, including the swath polygon; `t0`/`valid` parse as DateTime, `historical` as Boolean, levels and distances as Real, and the contour's unit, model and level, the route's distance and engine, and the imported attributes read back as written. Geodesic lengths on the WGS84 ellipsoid (`ST_Length(geometry, 1)`): the 30 kt track's one-hour path 55.66 km against 55.56 km on this app's sphere (0.2%, the sphere-vs-ellipsoid difference), the route 20.73 km, the contour 183.81 km. This is a one-off reader check recorded here, not a CI gate; QGIS itself was not run.

**Evidence ledger — 2026-10-07, increment 6 (radar threshold outlines):** the map's GeoJSON export also carries the active pane's displayed reflectivity sweep as 35, 50 and 60 dBZ outlines ([`radar_outlines`](crates/hookecho/src/app/radar_outlines.rs)): each gate sampled (no interpolation) onto a 0.01° lattice over the sweep's coverage, in-coverage no echo kept apart from missing (outside coverage, range folded) so an outline closes where the echo ends and stops where the data does; each line carries `site`, `product`, `elevation_deg`, `scan_time`, `threshold`, `unit` and `closed`, in WGS84. Contours now place values at cell centres like the field and probe (`ab14439`). Tests: `a_core_becomes_one_closed_ring_at_its_own_edge` (a synthetic 55 dBZ disc gives one closed 35 dBZ ring within ±1.5 km of its 5 km edge), `only_reflectivity_is_outlined_and_no_echo_is_not_missing`; `real_volume_outlines` (ignored, writes a review file) on the Mayfield 2021 corpus volume: 291 lines, 289 closed, read back independently with Python's JSON reader and drawn (`target/parity-review/m4.4/`). GDAL was not available in this container for an `ogrinfo` read. Open: other moments, user-chosen thresholds.

### M5 — Synchronized weather context and presentation

#### M5.1 — Support independent link groups and source contexts

**Priority:** P1. **Depends on:** M0.2, M1.1, M1.4. **Original references:** ROADMAP_2 §§10, 12; ROADMAP_NEW §§A2, F5, F6, J2, J3.

- **Starting evidence:** global camera/time/site/cursor links, retained analysis cursor, comparison modes, field lanes, and shared satellite cursor.
- **Outcome/build:** create multiple pane groups with independently enabled time/camera/site/geographic-cursor links. Support a live group beside an archived group and different model runs without cross-contamination. Let each group choose an analysis driver; unlinking retains each pane's last resolved state.
- **Interfaces/compatibility:** migrate existing global links to one all-pane group. Apply the request-context contract to fetched/displayed fields and source cursors; immutable content caches remain shared. Use existing nearest/exact/forecast policies and signed source offsets.
- **Failure/compatibility:** historical requests do not silently substitute current model/satellite data. A no-match result stays unavailable or explicitly outside tolerance according to the chosen policy. Stale replies from another context never replace an accepted frame.
- **Acceptance:** simultaneous live/archive/model-run groups retain distinct products/clocks; pane removal/reordering and link changes remain stable; out-of-order fetches cannot cross groups; crosshair probes identify their pane's displayed context on desktop/Android.
- **Proof:** context-race tests, migration fixtures, and multi-group operator captures.

**Agent prompt:** `Implement M5.1 using section 6. Extend global linking into independent groups and key asynchronous results by their complete source/time context.`

**Accessibility increment — 2026-10-04, model forecast timeline:** model/product selection now
activates the main timeline immediately, retiring the old hourly radar forecast tail before it
can overwrite the selected model. The dock and floating timeline share model/run selection,
native forecast positions, previous/next, first/last and field-paced playback. The Models
panel remains synchronized through existing selection/lead actions. A visible Radar control
returns to observations; keyboard stepping follows the active timeline. RTMA steps analysis
hours without presenting a forecast slider. This improves access to existing model features;
independent model/run link groups and complete asynchronous context isolation remain open.
See [controls and verification](docs/certification/model-timeline.md).

**Verification:** Windows workspace tests passed **2,218 tests** with zero failures and 143
explicitly ignored checks across 28 suites. Six new controls, strict workspace Clippy and WASM
compilation passed. The explicit GPU control produced eight reviewed production-row captures
at desktop/phone widths, including loaded-clock wrapping. The manifest retains twelve source
hashes and capture/log hashes. Full application/provider sessions, browser runtime, physical
devices and independent model/run groups remain open; detection work remains with Claude.

**Context increment — 2026-10-04, request-owned model fields (partial):** all 21 ordinary
regional, global and RTMA/URMA layers carry their original source/product/run/lead and relevant
CAPE/SRH variants from scheduling through delivery. Provider metadata is stamped at fetch;
regional reflectivity no longer obtains its source from a later picker. Obsolete replies,
including failures, are discarded before source-health accounting; current replies must match
their requested provenance and decoded grid clock. Selection changes clear pending uploads
and reset that lane's health history while keeping generation numbers monotonic.

Drawing, probing, grid export, loaded-clock labels and forecast playback require the accepted
field's exact selected context. One previous grid/stamp is retained for explicit historical
Source details while a different selection is unavailable; it cannot become that selection's
cache. Same-context retry failures keep usable data. Latest-cycle health reports the resident
field's actual time rather than a previously seen maximum. The scheduler now recognizes CAPE
parcel and SRH depth changes immediately. Reflectivity retains its existing ten-minute fetch
cadence and reports the same cadence in health.

**Verification:** Windows workspace tests passed **2,237 tests**, zero failures and 145
explicitly ignored checks across 28 suites. Nine request-context controls, strict Clippy and
WASM compilation passed. The explicit production Sources-row GPU control passed in 4.56
seconds; eight 240/300 px captures were reviewed. The retained manifest pins twenty source
hashes, all check/log hashes, and four committed diagnostics fixtures. This is local control
and layout evidence; full application/provider/group sessions and physical devices remain open.

See [implementation and evidence](docs/certification/model-context.md). Independent groups,
per-pane model controls, context-keyed shared textures/cache entries, comparison/ensemble and
contour ownership, and MRMS/GOES group drivers remain open. This is a prerequisite for M5.1's
multi-group workflow, not acceptance of that parent card. Next: introduce explicit pane/group
model state and renderer resource ownership together, then migration/race controls and a
simultaneous live/archive/different-run operator session. Tornado detection stays with Claude.

**Pane increment — 2026-10-05, ordinary models (partial):** model controls now belong to each
pane, with Independent or explicit Model/run groups in the main forecast timeline and Models
surfaces. Joining shares source/run/lead while preserving compatible products and CAPE/SRH
variants; unlinking retains values. Independent playback survives focus changes, and group
playback waits for all enabled selected products. Newly split panes inherit their source context.
Workspaces persist typed controls/groups; legacy layouts migrate to one group, unsupported
contexts disable affected fields with preserved metadata, and conflicting groups restore
independently with a disclosed warning.

The ordinary 21-layer scheduler, source-health lanes and shared cache use complete request keys.
Identical requests share grids/textures; different source/run/lead/variant contexts remain separate.
Any remaining exact pane subscriber can admit a reply; dropping the last subscriber cancels work
without source-health credit. The bounded cache protects visible requests, expires hidden slots
and retires GPU resources with fresh IDs on re-creation. Rendering, probes, exports, clocks and
playback read their pane's accepted slot. Sources/diagnostics enumerate all visible contexts and
name their owners. Missing texture keys cannot borrow another same-layer field.

Verification: 2,272 workspace tests passed across 29 suites (147 ignored), strict workspace/
all-target Clippy passed, and the WASM library check passed. Four explicit GPU controls passed
in 3.88 seconds, including the two new pane controls. This increment adds 18 ordinary regression
controls; the manifest retains 37 Rust source hashes, 23 visually reviewed capture hashes and
two Sources diagnostics arrays.

See [implementation, operator workflow and evidence](docs/certification/model-panes.md). Local
controls and production GPU/layout evidence are retained in its verification manifest. **M5.1
remains partial:** broader camera/site/cursor/time groups, MRMS/GOES time drivers, comparison/
ensemble/contour ownership, and a full application live/archive/different-run operator session
remain. Next: migrate analysis-time groups and their MRMS/GOES request/render ownership together;
verify no-match policies and migration/races before parent acceptance. Tornado detection stays
with Claude.

**Spatial increment — 2026-10-05 (partial):** camera, radar-site and geographic-cursor links now
have independent per-pane group membership, with Independent/Group N choices under Pane links
in workstation Layers and the floating/phone panel. Joining adopts only that dimension; unlinking
retains the resolved state. Focus changes do not retarget a settled camera group. Site mutations
from all existing input paths reconcile before pane fetching, keeping products, tilts and clocks.
Crosshairs and probe rows include only members of the hovered pane's cursor group; phone probe
readings wrap and scroll. Link all panes checks the whole workspace before combining spatial
groups, alongside the existing global time/storm links.

Workspaces retain schema-1 spatial memberships. Legacy global flags become group 1, with the
saved focused camera retained; unsupported metadata disables spatial links and round-trips.
Conflicting typed camera/site owners retain their individual saved values and restore just that
dimension independently, with a warning. New splits inherit memberships. See
[behavior and verification](docs/certification/spatial-groups.md).

Verification: 2,286 shared-workspace tests passed across 29 suites (148 ignored), strict
workspace/all-target Clippy and WASM compilation passed, and the explicit GPU control passed in
2.75 seconds. Ten new ordinary controls, one explicit GPU control, eighteen owned Rust source
hashes and nine reviewed production-control captures are retained. Claude-owned wxdata work was
present during checks and is recorded separately; it is excluded from this increment.

**Raster time increment — 2026-10-05 (partial):** timed tiles now retain style, exact UTC second,
provider revision and latest-alias epoch through requests, retries, CPU residency, GPU upload,
draw-list reuse and ancestor/child zoom fallbacks. Changing time selects another namespace,
retaining static maps and bounded reusable frame textures. Late obsolete replies cannot become
another frame. Exact-second disk paths avoid legacy minute-only collisions; mutable latest
aliases bypass disk and renew their memory epoch after five minutes or on return from a dated
frame. Provider changes discard obsolete map/picker replies without resetting the thumbnail
worker concurrency budget. Failed tile bookkeeping follows visible owners; loading counts
exclude resident textures. See [implementation and evidence](docs/certification/raster-context.md).

Verification: 2,295 shared-workspace tests passed across 29 suites (149 ignored), strict
workspace/all-target Clippy and the WASM library check passed, and two explicit GPU controls
passed in 1.84 seconds. This increment adds nine ordinary controls and one GPU control;
the existing model-context GPU control also passed. Eight owned Rust source hashes and thirteen
visually reviewed raster captures are retained, with no source-guard changes. Claude-owned
calculation sources were present during workspace checks and are recorded separately.

This closes raster **identity** as a prerequisite; it does not add independent pane time drivers,
replace the shared satellite catalog, change archive no-match policies, or make MRMS/GOES decoded
fields independent. Transport jobs retain their existing concurrency/deadline bounds; retiring
a scope rejects delivery without aborting the underlying HTTP job. Parent acceptance stays open.

**MRMS analysis increment — 2026-10-05 (partial):** every MRMS catalog layer now resolves its
pane's own archive cursor, or the retained global analysis clock when linked. Complete identity
includes layer, resolved product/window path, exact UTC target and archive tolerance. Matching
requests share one cancellable download, display slot and texture; independent analyses retain
separate generations, clocks, failures and provenance. Deferred archive seeks retain their target
while radar listings load. Empty unlinked archive selections wait instead of requesting latest.

Sources/diagnostics enumerate all requested MRMS contexts and pane owners, including implicit
precipitation-tint owners. Loaded times come from accepted provider stamps; missing or rejected
fields cannot show another request's cache/clock. Rendering, probes, grid exports, 3D surfaces and
route exposure use the selected pane's context. Radar tint carries both context and generation,
so switching between cached analyses rebuilds it even if their generation numbers match.

Accepted archives are reused without periodic refetch; missing/failed archives retry at the
existing bounded field cadence and latest requests continue refreshing. Sources/diagnostics
disclose retained-archive policy without a false retry countdown or a stale-feed warning based
solely on time in cache. Last-owner retirement
aborts transport without source-failure credit. A bounded entry cache protects all visible fields,
retains recently hidden contexts and retires their GPU keys without reusing identities.
See [behavior, controls and remaining acceptance](docs/certification/mrms-panes.md).

Verification: 2,302 shared-workspace tests passed across 29 suites (151 ignored), strict
workspace/all-target Clippy and WASM compilation passed, and four explicit GPU controls passed.
Seven new ordinary controls and two new GPU controls are included. The manifest pins 29 owned
Rust source hashes, 28 reviewed PNGs and four Sources diagnostics snapshots; checked code had
no final source-guard changes. Claude-owned calculation work remains excluded from this increment.

**M5.1 remains partial:** independent analysis-time groups, GOES decoded field/sector/catalog and
tile-driver ownership, source driver selection, comparison/ensemble/contour ownership, and full
application multi-group live/archive operator evidence remain. MRMS product-window settings,
current-only local mosaic/snow-band drivers and selected-storm linking remain shared. The next
time increment must migrate GOES clocks, sector footprints and source ownership together, then
add independent analysis-time memberships and verify operator archive-miss behavior. Tornado
detection remains with Claude.

#### M5.2 — Add satellite-native one-minute playback

**Priority:** P1. **Depends on:** M0.3, M1.4, M5.1. **Original references:** ROADMAP_NEW §§E1, E4, E5, E7; ROADMAP_2 §§10, 14.

- **Starting evidence:** ABI channel/RGB/mesoscale ingest, scan discovery, dynamic footprints, source-time following, and existing time alignment.
- **Outcome/build:** expose native ABI frame listings and a satellite-driven timeline, including intermediate one-minute mesoscale frames between radar scans. Allow satellite to drive a linked group while other layers resolve their own nearest valid frame. Add bounded loop prefetch and cancellation, retaining per-scan sector footprints.
- **Interfaces/compatibility:** frame identity includes satellite, sector, band/recipe, scan time, footprint, and channel set. RGB inputs use the same scan or an explicitly rejected incomplete recipe; source-domain fallback is shown with its time offset.
- **Failure/compatibility:** missing scans remain visible timeline gaps. Changing boxes/satellites cancels obsolete requests; archive misses do not load today's imagery. Disk/cache limits and Android memory limits apply to loop size.
- **Acceptance:** a mesoscale loop includes all available one-minute frames; RGB channel times agree; moving sectors render in their recorded footprints; radar-driven behavior still works; desktop/Android can scrub and loop without UI stalls.
- **Proof:** pinned listing/granule fixtures, gap/moving-sector tests, and native loop performance captures.

**Agent prompt:** `Implement M5.2 using section 6. Add native ABI cadence to the shared timeline with same-scan RGB and bounded desktop/Android playback.`

**Status:** implementation: in progress (satellite-driven loop over native scan listings, increment 1); verification: partial (unit; live NOAA listing; no interactive or device run).

**Evidence ledger — 2026-10-07, increment 1 (satellite loop over the satellite's own scans):**

- *User workflow delivered:* "Loop satellite scans" (Satellite layer options, and the command palette's "Satellite loop") plays every scan in the chosen sector over the last 15/30/60/120 minutes: each one-minute mesoscale frame, or each five-minute CONUS frame. While on, the scan on the loop's playhead drives the GOES layers' clock (and the mesoscale footprint request, so a moving box is drawn where it was for that scan); radar and every other layer keep their own time. A live loop relists every 30 s and keeps the playhead on the scan being viewed (or on the newest when it was there); an archive pane's loop ends at that pane's analysis time. The bar has play/pause, step, length and speed (2/4/8 fps maximum), and a strip with a tick per listed scan: missing scans are drawn as gaps, frames that failed to load are marked, cached frames are brighter; click or drag seeks.
- *Readiness and resources:* the loop steps only when every GOES layer on the pane holds the requested scan, so playback never runs ahead into blank frames; a frame that has not appeared within 12 s is marked failed and skipped. Up to six frames ahead (every band the shown layers read, matched to the frame within 30 s) are downloaded into the object cache without decoding, two at a time, so memory holds one decoded frame per layer whatever the loop length. A new context (satellite, sector, bands, length, end) starts a new listing generation; older answers are dropped.
- *wxdata:* `goes_abi::{scan_frames, scan_frames_from_keys, scan_gaps, ScanFrame, ScanGap, prefetch_key}`. Hour listings are cached by URL (`list_hour`): an hour read 15 min after it ended is kept (keys are immutable), a filling hour is reused for 20 s; bounded at 256 listings. The existing nearest-scan lookups use the same cache, so an archive frame costs at most one listing per hour instead of three.
- *Commands run and results:* `cargo test -p wxdata --lib goes_abi` (band filtering, ordering, duplicate delivery, a two-scan gap counted as 2 missing, jitter not a gap, listing freshness for settled/filling hours); `cargo test -p hookecho --lib sat_loop` 4 passed (starts on the newest, holds for the shown frame and its fps, marks a frame failed after the wait, dwells on the newest then wraps, relisting keeps the viewed scan, prefetch bound and other-band matching). Live (`meso_loop_listing_live`, network): 59 Meso-1 band 13 scans in the last hour, 0 gaps; listed in 664 ms, again from the cache in 1 ms; newest granule prefetched in 85 ms.
- *Desktop/Android/browser evidence:* no interactive run. The listing and prefetch are the same async code on every platform (IndexedDB cache in the browser); not exercised on Android or in the browser.
- *Known limitations / open:* the loop drives GOES layers only; it does not yet drive a linked time group or radar to the satellite's clock. Difference/cooling-rate layers (latest-only) are hidden while the loop runs. Decoding is still per displayed frame, so the real frame rate is bounded by decode time (unmeasured on devices). No RGB same-scan rejection beyond the existing fetch rule.

#### M5.3 — Broaden field inspection within supported models

**Priority:** P1. **Depends on:** M1.4, M5.1. **Original references:** ROADMAP_NEW §§A1, F1, F3, F4, F5, F6; ROADMAP_2 §§9, 10.

- **Starting evidence:** HRRR/RAP/NAM/NAM nest/NBM descriptors, global-model fields, `.idx` range fetches, grid decoders, common legends, and comparisons.
- **Outcome/build:** add indexed field/vertical-level discovery for already supported model providers. Use metadata to expose compatible scalar, contour, and paired-vector products. Prioritize pressure-level temperature/moisture/heights/wind, surface wind/gusts, and interval precipitation; integrate units, point sampling, run/valid clocks, and existing compare/export.
- **Interfaces/compatibility:** stable field identities include physical quantity, level, statistic, and accumulation interval. Use vetted mappings for supported GRIB parameters, then show unmapped indexed entries as unsupported until their metadata is defined. Arbitrary numeric codes must not receive guessed units or palettes.
- **Failure/compatibility:** distinguish unavailable provider fields and differing grid/run/accumulation semantics. Vector components require matched context; interval QPF is not inferred by differencing incompatible accumulated products. Cancel discovery/fetch on context changes.
- **Acceptance:** supported pressure/surface fields have correct units/levels and native-grid provenance; comparisons require compatible quantities/intervals; run/lead switching rejects stale responses; unsupported entries are explained on desktop/Android.
- **Proof:** archived index/GRIB fixtures, descriptor/unit/comparison tests, and field-browser workflows.

**Agent prompt:** `Implement M5.3 using section 6. Extend supported-model field discovery through the existing registry, with explicit quantity/level/interval metadata and no guessed GRIB meanings.`

**Status:** implementation: in progress (regional inventory discovery and a discovered-field layer, increment 1; GFS, increment 2; browsed winds as barbs, increment 5; ECMWF, increment 6); verification: partial (unit on real `.idx` lines; live HRRR inventory and field; no interactive or device run).

**Evidence ledger — 2026-10-07, increment 1 (what a regional run holds, and showing any of it):**

- *User workflow delivered:* **Model fields** (command palette) lists every field the active pane's regional run publishes at its lead — HRRR, HRRR pressure levels (`wrfprs`, every mandatory level), RAP, NAM nest, NAM, NBM — read from that run's own `.idx`. Fields with vetted units are listed by quantity then level ("Temperature · 500 hPa · instant (°C)") and searchable; clicking one shows it on the pane as the **Model field** layer, coloured by a ramp over its 2nd–98th percentile, with a legend naming the quantity, level, timing, model, run and valid time. Hovering another field says whether it is comparable with the shown one, and why not (other level, other interval length). Unvetted parameters (the local `var discipline=… parm=…` entries, POT, and so on) are listed under "Not shown" with the reason, never drawn.
- *Semantics:* [`wxdata::model_inventory`](crates/wxdata/src/model_inventory.rs) classifies each level (isobaric hPa, m AGL, AGL layers, surface, MSL, column; anything else kept verbatim) and timing (analysis, instant, or accumulation/max/min/mean over `from–to` h). A pick keeps its timing *kind* — instant, an N-hour window, or since the run began — so scrubbing the lead fetches the same kind at the new lead, and a lead without that exact message is an error, never another interval's field. The vetted table (32 NCEP abbreviations) carries native and display units with an explicit conversion (K→°C, Pa→hPa, gpm→dam, m/s→kt, kg/m²/s→mm/h; lifted index stays a K difference). Wind components are named u/v, not east/north: the HRRR's and NAM's are grid-relative; u/v pairs are detected per level and counted, not yet drawn as vectors.
- *Interfaces/compatibility:* `FieldLayer::ModelField` (`model-field`), `ModelRequest::Discovered` / `OverlaySource::ModelField` carrying an interned `FieldPick` (model, abbreviation, level text, timing kind) through the existing request/stamp/stale-rejection path; `wxdata::hrrr::{fetch_inventory, fetch_inventory_field}`. `ModelControls::field` is written only when set, so workspaces without one remain readable by older builds (whose `deny_unknown_fields` would otherwise reject them).
- *Commands run and results:* `cargo test -p wxdata --lib model_inventory` 4 passed on real HRRR (2026-10-06 23Z F06) and GFS (12Z F00) `.idx` lines (level/timing parsing, byte ranges to the next distinct offset, only vetted units, comparisons by quantity/level/interval, u/v pairing at 10 m/250/500 hPa, a message found by kind and lead and not at another lead); `cargo test -p hookecho --lib model_field` (pick save/label/intern, percentile range, a fetched field accepted only by its own request — not by another level, lead or run); `model_context` tests updated for the 22nd model layer. Live (`inventory_live`): newest HRRR F+6, 173 fields, 96 supported, 9 u/v pairs; 500 hPa temperature fetched as a discovered field, −26.4..0.4 °C over 1,088,905 cells; POT refused for lack of vetted units. Workspace 2,454 passed; clippy clean; wasm32 check passes.
- *Desktop/Android/browser evidence:* none interactive. The browser is a floating window reached from the command palette on every platform; not exercised on Android.
- *Known limitations / open:* regional models only (global GFS/ECMWF inventories not yet browsable); the existing max-per-cell scatter regrid is used, which is exact at native spacing but biases a smooth field where several native points fall in one cell; no vector (barb/streamline) drawing from u/v pairs and no grid-relative rotation; no contours, point probe readout label or export naming for the discovered layer beyond the generic grid paths; comparisons are reported, not yet wired into the difference layer.

**Evidence ledger — 2026-10-07, increment 2 (the GFS):** the Model fields browser also lists the GFS quarter-degree file at the pane's global lead (to F+384), through [`global::{fetch_gfs_inventory, fetch_gfs_inventory_field}`](crates/wxdata/src/global.rs) and the same inventory, vetted-units and timing-kind rules; a picked GFS field draws as the Model field layer on the existing 0.3° global lattice. A pick now names its source (`InventorySource`: a regional model or the GFS), the request's lead is `u16`, and a GFS stamp is accepted only by a GFS request (tested, including a 120 h lead). Picks saved by increment 1 earlier on this branch use the old form and reset. Live (`gfs_inventory_live`): GFS 18Z F+24, 743 fields, 390 supported; 850 hPa temperature −46.8..36.7 °C. ECMWF (JSON-lines index, different parameter naming) and vector drawing remain.

**Evidence ledger — 2026-10-07, increment 3 (grid-relative winds turned to east/north):** the HRRR, RAP and NAM publish `UGRD`/`VGRD` along their Lambert grid's axes (GRIB2 code table 3.3 bit 5), and the wind particles and model soundings were using them as east/north — every wind turned by the convergence angle `n·(λ − LoV)`, about 14–17° near the coasts. [`wxdata::grid_winds`](crates/wxdata/src/grid_winds.rs) reads the message's own grid definition (template 3.30: LoV, Latin1/2, the component flag) and turns grid-relative pairs with `u_e = u cos α + v sin α`, `v_n = −u sin α + v cos α`; lat/lon grids and Lambert files that say their winds are earth-relative are left alone. The HRRR wind field is now turned on the native grid before the regrid (`fetch_wind_at_run`), and each model sounding level at the sounding's longitude (rotation, shear and SRH magnitudes being unchanged by it, the effective-layer parameters needed nothing). Tests: template parsing for grid-relative/earth-relative/lat-lon messages; the formula against the Lambert projection's own axes at four longitudes; whole-grid rotation keeps gaps. Live, 2026-10-07 12Z F+6, median HRRR−GFS 10 m wind direction difference: east coast +16.3° raw → +3.1° turned (79 points), west coast −17.4° → −1.0° (23 points); a model sounding's 500 hPa wind at 75°W agrees with the turned grid to 0.1°; the matched-pair guard still passes near the central meridian. Discovered single u/v components (the field browser) remain as the file gives them and are labelled so.

**Evidence ledger — 2026-10-07, increment 4 (wind barbs):** **Wind barbs** (command palette; saved as `Settings::wind_barbs`) draws the HRRR wind layer — now east/north, increment 3 — as barbs on a 56 px screen lattice, coloured by the wind ramp the particles and legend use, with the METAR barb/pennant glyphs. Each barb's direction comes from projecting a 10 km step toward the wind's source through the camera, so it stays true on a rotated or pitched map. Tests: a westerly points screen-left and a northerly screen-up on a north-up map, a map turned 90° turns them with it, 39 kt draws a shaft with three full barbs and a half, no barb off the grid; a review capture is written under `target/parity-review/wind-barbs`. Barbs from a browsed field's u/v pair (any level, any model) remain open.

**Evidence ledger — 2026-10-07, increment 5 (browsed winds as vectors; RAP's packed components):** the Model fields browser lists each level where a run publishes both wind components as **Wind · level · timing (kt)**. A wind pick (`SavedFieldPick::vector`, saved only when set, so earlier workspaces load unchanged) fetches `UGRD` and `VGRD` together ([`hrrr::fetch_inventory_vector`](crates/wxdata/src/hrrr.rs), [`global::fetch_gfs_inventory_vector`](crates/wxdata/src/global.rs)), turns Lambert grid-relative components to east/north on the native grid (increment 3), and is stamped as its own product (`WIND:level:timing`, derived) so it is never confused with the scalar `UGRD` at that level. The speed in knots is the drawn field and legend; the components travel in the same reply (`OverlayMsg::VectorField`), are kept on the speed's display lattice by nearest cell (the display decimation's maximum pool would bias a signed component), are accepted only by the exact wind request and only on that lattice, and are staged with the speed in the request's model slot — so barbs ([`wind_draw::barbs_uv`](crates/hookecho/src/wind_draw.rs), the increment-4 lattice and glyphs) draw only once their speed is the pane's accepted field and can never disagree with it on run or lead. **Fixed on the way:** the RAP packs `UGRD` and `VGRD` as two fields of one GRIB2 message (`.idx` lines `92.1`/`92.2`); a discovered single `VGRD` was decoded from the message's first field, i.e. drawn as `UGRD`. Inventory entries now carry their field number (`Entry::sub`) and every inventory fetch extracts that field (`model_inventory::field_bytes`). *Commands:* `cargo test -p hookecho --lib model_field` 6 passed (wind pick save/label/identity/unit; a wind reply accepted only whole and only by a wind request; nearest-cell lattice keeps sign); `cargo test -p wxdata rap_packed_winds_live -- --ignored` (RAP 500 hPa u and v differ at 111,153 cells; a single discovered VGRD differs from UGRD; its `.idx` entry is a second field) and `gfs_inventory_vector_live -- --ignored` (GFS 12Z F+12 250 hPa, fastest wind 102 m/s, valid at run + lead) passed live; full workspace tests, Clippy `-D warnings` and the WASM library check passed. *Open:* no streamlines; no barbs or vector probe readout in exports; not exercised interactively or on Android.

**Evidence ledger — 2026-10-07, increment 6 (the ECMWF IFS):** the Model fields browser lists the ECMWF open-data IFS quarter-degree file. Its JSON-lines `.index` is read into the same inventory entries as an NCEP `.idx` ([`model_inventory::parse_ecmwf_index`](crates/wxdata/src/model_inventory.rs)) through an explicit equivalence table that maps a parameter only where its unit is the NCEP one (`t`, `gh`, `r`, `q`, `u`/`v` on pressure levels; `2t`, `2d`, `10u`/`10v`, `100u`/`100v`, `msl`, `sp`, `tcwv`), plus `tp`, metres of water accumulated from the forecast start, as `APCP` since the run with an explicit ×1000 to kg/m² (`Entry::scale`). Gusts and 3-hourly extremes (whose windows the line does not state), cloud, radiation, vorticity, divergence and `mucape` keep ECMWF's lower-case names and are listed as unsupported. Inventory, field and wind fetches are one path for both global models ([`global::{fetch_global_inventory, fetch_global_inventory_field, fetch_global_inventory_vector}`](crates/wxdata/src/global.rs), replacing the GFS-only functions); a pane's global lead snaps down to a file the model publishes (`GlobalModel::inventory_lead`: ECMWF 3-hourly to F+144 then 6-hourly to F+360; GFS hourly to F+120 then 3-hourly), so the request and the stamp agree. An ECMWF pick is its own source (`InventorySource::Ecmwf`, stamped "ECMWF"), never accepted for the GFS's field. *Commands:* `cargo test -p wxdata --lib model_inventory` 5 passed (new: eleven real `.index` lines of 2026-10-06 00Z step 6 — mapped labels, `tp` range and scale, ranges from offset/length, unmapped stay unsupported, u/v pair at 10 m and 500 hPa); `inventory_leads_land_on_published_files`; `cargo test -p hookecho --lib model_field` 7 passed (an ECMWF pick refuses a GFS stamp; lead snapping); `ecmwf_inventory_live -- --ignored` passed (12Z F+12: 184 fields, 94 supported; 850 hPa −37.0..28.9 °C; 12 h precipitation 0.00..279.3 mm; 250 hPa fastest wind 104 m/s); full workspace tests, Clippy `-D warnings` and the WASM library check passed. *Open:* the ECMWF's 6-hourly steps beyond F+144 make a pane's hourly scrub repeat a step; comparison of a discovered field across models; not exercised interactively or on Android.

**Evidence ledger — 2026-10-09, increment 7 (streamlines):** **Wind streamlines** (command palette; saved as `Settings::wind_streamlines`) draws the HRRR wind layer, and a wind picked in the Model fields browser (in place of its barbs), as evenly spaced lines that follow the flow, arrowheads downstream every 140 px, each stretch coloured by its speed on the barbs' and particles' ramp ([`wind_streamlines`](crates/hookecho/src/wind_streamlines.rs)). Layout is Jobard–Lefer in screen space: seeds on a 32 px lattice, each line traced both ways with midpoint steps of 3 px until it leaves the pane, the air goes calm (under 0.5 m/s) or missing, it comes within 16 px of another line or of its own course well behind it (so a vortex closes its ring instead of spiralling), or 220 steps each way; a seed within 32 px of a line is skipped. Each step's direction is the wind's bearing projected through the camera, as the barbs do, so rotated, pitched and globe views read true. Lines are kept in world coordinates per pane, so a pan carries them with the ground; a moving camera re-traces at most every 200 ms and a new wind (run, lead, level) at once. Tests: `a_uniform_westerly_gives_parallel_lines_flowing_east` (parallel rows no closer than half the spacing, arrows east, speed in knots), `a_southerly_flows_up_the_screen_and_calm_or_missing_air_draws_nothing`, `a_vortex_closes_its_rings_rather_than_spiralling_over_itself` (no two points of different lines, or of one line far apart along it, within half the spacing), `the_cache_keeps_lines_on_the_ground_and_retraces_a_new_wind_at_once`; a 1600×900 pane traces 88 lines (13,274 points) in 30 ms in a debug build. Real app (release build on Xvfb, live HRRR surface wind over CONUS, 9 Oct 2026 02:39Z): streamlines curve round the tropical system nearing the Gulf coast with arrowheads downstream, the faster flow there yellow-orange and the light inland flow blue-green, under the warnings and track cone; a scripted drag did not pan the map, so panning was checked only by the cache test. Open: barbs or a vector probe readout in exports; Android.

#### M5.4 — Unify rendering quality, labels, and transitions

**Priority:** P1. **Depends on:** M1.2, M3.5, M4.3, M5.2. **Original references:** ROADMAP_2 §4; ROADMAP_NEW §§O1–O4, Q2, Q3.

- **Starting evidence:** quality presets, platform texture limits, label placement, basemap parent fallback, rendering ramps, and camera transitions.
- **Outcome/build:** define Low/Balanced/High/Analysis quality profiles controlling 3D steps/resolution, field texture detail, contours, labels, and particles coherently. Add measured adaptive reduction on Android without changing scientific probe/export values. Stabilize label placement with priorities/collision groups and readable leader lines.
- **Interfaces/compatibility:** quality policy is a rendering contract shared by visual subsystems. Instant, short visual crossfade, and radar progressive replacement reuse one timeline; interpolation eligibility comes from `ValueKind`. The inspector records display transformations/decimation.
- **Failure/compatibility:** never interpolate categorical/mask values into new classes. Parent tile fallback remains until a replacement is ready; retries are bounded and coverage diagnostics expose unresolved holes. Scrubbing does not animate the camera into an analytically different view.
- **Acceptance:** warning/storm labels outrank background labels, remain stable, and fit narrow/touch layouts; output scales agree with probes; changing quality does not alter values; camera motion and dense layers meet balanced device budgets.
- **Proof:** dense-map/zoom golden images, transition tests, and performance/quality comparisons.

**Agent prompt:** `Implement M5.4 using section 6. Consolidate existing render controls into measured quality profiles and stabilize labeling without altering analytical values.`

### M6 — Broadcast and external output

#### M6.1 — Separate preview from program output

**Priority:** P1. **Depends on:** M0.2, M1.4, M5.1. **Original references:** ROADMAP_2 §§6.1, 6.3, 6.5; ROADMAP_NEW §M1.

- **Starting evidence:** clean output viewport follows the active pane; output size/fullscreen controls and scene hotkeys already exist.
- **Outcome/build:** give output its own view/time/camera state, selectable source pane or stored scene, resolution/aspect behavior, and monitor choice. Add preview edits and an explicit Take action that applies a fully resolved snapshot atomically. Operator browsing cannot redirect program output inadvertently.
- **Interfaces/compatibility:** output reuses immutable data/render resources without consuming one-shot updates needed by another surface. A snapshot carries frame/product context and dressing. Keep the stable HookEcho Output window title for existing capture sources.
- **Failure/compatibility:** scene readiness reports missing/pending data; preview errors do not replace program. Live last-good output stays visibly stale, and old program content retains its actual timestamp. Multi-window operations are desktop capabilities, with Android's surface handled in M6.4.
- **Acceptance:** changing active panes, operators' product/time, or preview does not change program until intended; Take changes all scene parts together; output remains consistent under high-DPI resize and monitor changes.
- **Proof:** state isolation/readiness tests and a desktop preview/program OBS capture walkthrough.

**Agent prompt:** `Implement M6.1 using section 6. Extend the existing output window with independent preview/program state and atomic scene application.`

**Evidence ledger — 2026-10-06, increment 1 (pinned program, cue and Take):** the output window rendered whichever pane was active, so an operator moving to another pane moved the picture on air. Program now has its own source ([`ProgramSource`](crates/hookecho/src/app/output_window.rs)): the pane active when the window opens is pinned, another pane can be chosen, and "follow the active pane" (the old behaviour) is an explicit choice. A pinned pane that is closed is not replaced by another: the output says "No program: pane N is not open". Scenes are cued into a preview (name, site, zoom, strap, which pane Take will put it on, and a readiness check) and go on only with Take, which applies the whole scene to the program pane in one call or, if it cannot (no program pane), leaves program as it was and says why ([`scene_readiness`](crates/hookecho/src/app/scenes.rs) also notes layer names this build lacks, GIS layers the scene shows that are no longer imported, checked on a copy so nothing changes, and an unknown output size). Alt+1..9 cue and Take through the same path. With the output window closed, scenes go into the active pane as before (streaming mode on the main map). Tests: `program_stays_on_its_pane_and_a_closed_one_is_not_replaced`, `a_scene_is_checked_before_it_goes_on`; full workspace 2,365 passed, Clippy `-D warnings`, the WASM library check. Not done: program has no camera/time/product state independent of its pane (panning the program pane still moves the output), no live preview picture, no last-good frame held on a missing source, no monitor choice; no OBS capture walkthrough.

**Evidence ledger — 2026-10-06, increment 2 (held program camera):** "Hold the program view" gives program its own camera: the output renders its pane with the held camera swapped in around `render_pane` (the mini-loop window's proven pattern), so the operator panning or zooming that pane no longer moves the picture on air; turning it on holds where the pane looks now, off follows the pane again. A scene Taken into a held program sets the held camera and leaves the operator's view of the pane where it is ([`takes_held_camera`](crates/hookecho/src/app/scenes.rs)). Test: `a_held_program_takes_the_scene_camera_and_the_operator_keeps_theirs`; full workspace 2,366 passed, Clippy `-D warnings`, the WASM library check. Still shared with the pane: its radar, product, tilt, time and layers (a scene's site and layers still change the pane); no rendered preview picture; no OBS walkthrough.

#### M6.2 — Persist complete, validated scenes

**Priority:** P1. **Depends on:** M4.4, M6.1. **Original references:** ROADMAP_2 §§6.3, 6.4, 12.2; ROADMAP_NEW §§M1, K3.

- **Starting evidence:** existing Scene stores camera, site, overlay names, dressing, strap, and output size; workspace/case persistence contains richer view state.
- **Outcome/build:** extend scenes to include product/tilt, palettes, required layers, GIS groups, annotations, legends, scientific/render quality choices, and live/fixed/group-follow time policy. Provide thumbnails, duplication, reorder, and existing scene shortcuts through one scene action path.
- **Interfaces/compatibility:** version scene definitions and resolve stable IDs. Legacy scenes load using the original keep-current behavior for fields they never stored; newly saved scenes capture their actual product/tilt explicitly.
- **Failure/compatibility:** preflight missing assets/products and incompatible quality/backend capabilities before Take. Repair/locate assets or preserve the previous program snapshot; do not silently substitute another scientific product.
- **Acceptance:** legacy/new round trips work; restored scenes match camera/products/palettes/GIS/time/dressing; missing assets are actionable; preview/program and shortcuts use identical application logic on desktop and Android.
- **Proof:** scene compatibility fixtures and complete-state restore captures.

**Agent prompt:** `Implement M6.2 using section 6. Extend scenes from partial graphics presets to complete validated analysis/output snapshots while preserving old scene semantics.`

**Status:** implementation: in progress (versioned product and time policy, increment 1); verification: partial (unit; no capture walkthrough).

**Evidence ledger — 2026-10-07, increment 1 (scene product and time, WSV3 broadcast):**

- *User workflow delivered:* "Save the current view as a scene" now also keeps the pane's radar product, tilt, SRV, field layers, enabled display thresholds and column user product, and whether it was **live** or scrubbed to an archived frame (that frame's instant). Take puts all of it on the program pane through the existing single path (Take button and Alt+1..9): thresholds are replaced, so a previous view's filter never stays on air; a live scene returns the pane to the newest frame; a fixed scene seeks the archive the way an opened case does, and a time-linked group follows. The preview line names the product, tilt and time ("KTLX · VEL tilt 3 · 2013-05-20 20:01 UTC · zoom 9.0").
- *Readiness (before Take):* a column product the scene names but this installation does not define **blocks** Take (no other product is substituted under the scene's name); a scene written by a newer format blocks with the format numbers; unknown field-layer slugs and the archive instant to be loaded are notes.
- *Interfaces/compatibility:* `broadcast::{SCENE_VERSION = 2, SceneProduct, SceneTime}`; `Scene::{version, product, time}` all default when absent. A scene saved before this (version 0, no product/time) keeps the pane's current product and time, exactly as before. Model layers are excluded from `SceneProduct::fields_on` because they need a model run the scene does not carry; Take leaves the pane's model layers as they are. `SceneTime` serializes tagged (`{"policy":"live"}`, `{"policy":"fixed","utc":…}`).
- *Commands run and results:* `cargo test -p hookecho --lib scenes` 7 passed, including `a_scene_puts_back_the_product_it_was_saved_with` (capture → apply onto a pane showing another moment, threshold and model layer gives back the same product; the pane's model layer stays), `a_scene_is_live_or_one_archived_instant` (seek state, no claimed instant when a scrubbed pane has no frame, JSON in both forms) and `readiness_refuses_a_missing_product_and_a_newer_format` (plus a legacy scene parsing as version 0 with no product/time, and a full round trip).
- *Desktop/Android/browser evidence:* the scene list lives in the output window's settings (desktop); Android has no scene surface yet (M6.4). No OBS capture walkthrough.
- *Known limitations / open:* palettes, 3D state, annotations, legend placement and render quality are not yet in scenes; no thumbnails, duplicate or reorder; a fixed scene loads its archive day through the normal path and is not pre-fetched before Take, so program shows the pane's loading state until the frame arrives; group-follow time policy is not a separate option.
- *Next increment:* palette and 3D/quality choices with capability preflight; thumbnails, duplicate and reorder through the same action path.

**Evidence ledger — 2026-10-07, increment 2 (3D in scenes; duplicate and reorder):** a saved scene also keeps the pane's 3D map — on or off, its mode (by name), translucent/lit rendering, vertical exaggeration, opacity, quality and the user product a "User product" volume draws ([`SceneView3d`](crates/hookecho/src/broadcast.rs)). Take applies it after the product, so the mode is checked against the moment the scene puts on; a storm region of interest is cleared rather than restored, since it followed a storm of another time. The field is additive and optional, so the scene format stays 2: older scenes and older builds leave the 3D map as it is. Readiness notes a 3D mode this build does not have (and leaves 3D alone), and blocks a user-product volume whose product is not defined here. The scene list has Duplicate (inserted after the original, named "Moore (2)", "Moore (3)"…) and move up/down, which also renumber Alt+1..9. Tests: `a_scene_puts_back_its_3d_map_and_clears_a_storm_region`, `a_duplicate_lands_after_its_original_with_a_free_name`. Still open: palettes, annotations, legend placement and thumbnails; no Android scene surface.

#### M6.3 — Harden deterministic capture and archive output

**Priority:** P1. **Depends on:** M0.3, M1.2, M6.2. **Original references:** ROADMAP_2 §§6.2, 6.6, 8.4; ROADMAP_NEW §§M2, M3, M5.

- **Starting evidence:** deterministic capture clock, loop image/video paths, broadcast captions/crawl, warning archives, and scientific/analysis exports.
- **Outcome/build:** define capture readiness for required scene sources and freeze a render snapshot per logical weather frame. Advance on explicit analysis timestamps, record source-frame choices, and export metadata alongside imagery. Archive captions/warnings resolve from that historical context.
- **Interfaces/compatibility:** distinguish encoded video frames from logical weather frames: fixed-FPS holding of one source frame is intentional; accidental logical skips/duplicates are errors. A capture manifest records frame mapping, checksums, source stamps, resolution, and policy. Reuse the current encoder/platform capability boundary.
- **Failure/compatibility:** unavailable required data fails or pauses with an explicit bounded timeout (30 seconds by default), preserving completed output and a resumable manifest. Optional omitted layers are named. Never pair a historical frame with today's warning crawl.
- **Acceptance:** output frame count/mapping/timestamps are deterministic across repeat runs; pending/failed sources do not silently render wrong frames; thirty-minute output has no accumulating logical-clock drift; 1080p/1440p/4K presets preserve aspect and captions.
- **Proof:** capture/timeout/archive fixtures, encoded-frame inspection, and sustained output traces.

**Agent prompt:** `Implement M6.3 using section 6. Complete frame readiness and capture manifests around the existing deterministic exporter, including historical warning correctness.`

**Status:** implementation: in progress (capture manifest, increment 1); verification: partial (unit; no repeat-run or sustained capture).

**Evidence ledger — 2026-10-07, increment 1 (what each frame captured):** a loop export's JSON sidecar is now a versioned capture manifest ([`capture_manifest`](crates/hookecho/src/capture_manifest.rs), `schema: 1`), keeping every earlier sidecar key. For each logical frame it records the scan the timeline asked for and the scan on screen when the picture was taken (with its valid time), how long the wait was and whether it ran out (the existing 20 s bound), every field layer on the pane with whether it was ready for the pane's selected time and its data's valid time and offset from the radar frame, a SHA-256 of the captured pixels, and its start/hold plus the encoded pictures it occupies (`first_encoded`, `encoded_frames`: one per GIF frame, `loopexport::cfr_counts` at the MP4's 30 fps, so the mapping has no accumulated drift). `problems` lists a frame captured as another scan than asked (and whether after the wait), the same scan captured for two logical frames, and layers not ready; the save toast names the first problem instead of plain success. Tests: `the_checksum_is_sha256_of_the_pixels`, `logical_frames_map_onto_encoded_frames_without_drift` (500/500/1500 ms holds → encoded 0/15/30, 15/15/45 frames at 30 fps), `substitutions_duplicates_and_unready_layers_are_listed`; the existing on-screen rule test still passes. Open: the capture still records a timed-out frame rather than pausing/failing (the card's resumable manifest); no repeat-run determinism comparison, sustained thirty-minute trace or 1080p/1440p/4K preset check; scene sources other than field layers (overlays, GIS, dressing) are not part of readiness; the off-screen `--watch` path writes its own older sidecar.

#### M6.4 — Deliver usable desktop and Android presentation

**Priority:** P1. **Depends on:** M6.2, M6.3. **Original references:** ROADMAP_2 §§6.5, 13; ROADMAP_NEW §§M1, Q1, Q3.

- **Starting evidence:** desktop output viewport, streaming dressing, Android export/share paths, and native platform gating.
- **Outcome/build:** document/test desktop window capture, monitor selection, startup restore, and stable program sizing for OBS. On Android, expose the same scene controls and a clean in-app presentation surface with touch-reachable exit and supported still/GIF sharing. Preserve scene/time/provenance semantics across both.
- **Interfaces/compatibility:** capability reporting describes available output surfaces/encoders. Android does not claim the separate desktop window or ffmpeg MP4 path; lack of an encoder does not disable its scene workflow.
- **Failure/compatibility:** share cancellation and failed encoding leave the scene intact. Surface recreation/background resume restore selected presentation state with refreshed source health and appropriate stale indicators.
- **Acceptance:** an operator can prepare, present, switch, exit, save, and share scenes on both native platforms; OBS capture survives restart; Android clean mode supports Back, rotation, and touch without trapped controls.
- **Proof:** OBS setup guide, desktop/Android walkthroughs, exported-file checks, and lifecycle regression evidence.

**Agent prompt:** `Implement M6.4 using section 6. Finish platform-appropriate presentation and sharing workflows with shared scene semantics and explicit encoder capabilities.`

**Evidence ledger — 2026-10-09, increment 1 (streaming mode on a touch screen):** streaming mode hides every control and its only ways out were F8/F9; its on-screen hint was deliberately not interactive, so on a tablet without a keyboard entering it was a one-way door, and the Android Back gesture was not routed while the chrome was hidden (it left the app). A tap now brings up a control strip for four seconds ([`app/presentation.rs`](crates/hookecho/src/app/presentation.rs)): Exit, previous and next scene (wrapping through the saved scenes and applying them through the same `take_scene` path as Alt+1..9 and Take), and Save still (the strip hides first; Android writes into the app's exports folder). It sits top centre on the tooltip layer, clear of the clock, logo and warning crawl, with 44 px touch targets. Back now exits streaming mode before anything else, and is routed while the chrome is hidden. The hint names the device's own input ("tap for controls · Back exits" once a touch is seen or on Android; F8/F9 otherwise), and a desktop mouse click shows the strip only after a touch, so an OBS picture stays clean. Checked in the real app (Xvfb, llvmpipe) on the Moore 2013 archive: clean view → tap → strip over the dressing → Exit restores the workstation. Test: `previous_and_next_walk_the_scenes_and_wrap`; full workspace tests, Clippy `-D warnings`, the WASM library check. Not checked on a device (no NDK in this environment). Open: an Android share sheet for stills and loops (needs the activity's share intent), GIF from the strip, monitor selection and OBS guide for desktop.

### M7 — Professional certification

#### M7.1 — Extend soaks to the full application and resources

**Priority:** P0 release gate. **Depends on:** M0.3, M1.2, M1.3. **Original references:** ROADMAP_2 §§0.2, 3, 14; ROADMAP_NEW §§N3, O1–O4.

- **Starting evidence:** data-only `--soak`, local frame telemetry, stats counters, source contracts, and GPU/headless verifiers.
- **Outcome/build:** extend the runner with full application/render profiles and a scenario clock that rotates products/tilts/sites, panes, playback, storms, GIS, and output as those features land. Inject timeout/truncation/corruption, missing/late radar, offline/reconnect, model/ABI failures, and practical renderer/surface failures. Track RSS, GPU allocation bytes/counts, caches, pending jobs, frame pacing, and recovery times.
- **Interfaces/compatibility:** add explicit profiles to the existing CLI without breaking its current data-path invocation. Emit timestamped JSONL plus summary/exit status and scenario/seed/build/hardware identity. Faults and deterministic simulation remain separate from public-feed contract runs.
- **Failure/compatibility:** detect unrecovered stalls, retained obsolete contexts, unbounded requests, device/surface loss, and missing required evidence. Repeated full-surface failure must produce a recoverable state or clear failure, not a silent black canvas.
- **Acceptance:** section 5's two-/twelve-/twenty-four-hour profiles complete; all intentional faults are accounted for; post-warmup resource growth is bounded; disabled contexts release resources; long-run data, scientific, and frame clocks remain consistent.
- **Proof:** executable scenario manifests, resource/latency traces, summaries, and retained failure diagnostics. Repeat relevant profiles after changes affecting measured domains.

**Agent prompt:** `Implement M7.1 using section 6. Extend the current soak tool to exercise real application/render state and account for resources and injected recoveries.`

#### M7.2 — Certify Android lifecycle, input, and memory

**Priority:** P0 release gate. **Depends on:** M2.4, M3.6, M4.2, M5.4, M6.4, M7.1. **Original references:** ROADMAP_2 §§2.6, 13, 14; ROADMAP_NEW §§Q1, Q3, O1.

- **Starting evidence:** GameActivity/IME bridge, foreground gating, form-factor boundary, platform storage, Kotlin alert service, and Android CI cross-build.
- **Outcome/build:** create device test scenarios for rotation, resize/split-screen, background/resume, surface recreation, notification/deep-link arrival, network changes, picker/share, IME, external keyboard/mouse, pen, multi-touch, and memory pressure. Include both phone and workstation tablet layouts.
- **Interfaces/compatibility:** preserve Kotlin settings names/coalesced persistence and foreground worker policy. Touch controls expose hover/modifier equivalents; tablet window sizing retains the current 600 dp decision. Diagnostics identify device/GPU/thermal state and evidence gaps.
- **Failure/compatibility:** resumed feeds re-evaluate freshness before reporting current data. Imported content survives picker permission/lifetime changes; gestures cancel safely during lifecycle events; unsupported GPU modes remain understandable.
- **Acceptance:** active two-hour and mixed twelve-hour Android profiles pass on the device matrix; no unrecovered surface, memory, input, or stale-source failure; critical analysis/import/presentation workflows are demonstrated with actual touch/pen where supported.
- **Proof:** device recordings, adb/logcat diagnostics, resource traces, workflow checklist results, and the Kotlin contract test. No connected device means this gate remains open.

**Agent prompt:** `Implement M7.2 using section 6. Build and run the Android lifecycle/device scenarios; record physical evidence separately from compile and synthetic-input results.`

#### M7.3 — Publish certification and operational documentation

**Priority:** P0 release gate. **Depends on:** M1.3, M2.4, M3.4, M3.6, M4.4, M5.3, M5.4, M6.4, M7.1, M7.2. **Original references:** ROADMAP_2 §§0.2, 15, 17; ROADMAP_NEW §§26, 27, 30. These dependencies transitively include every implementation card above; every applicable platform acceptance gate must be complete.

- **Starting evidence:** source/data guides, diagnostics bundles, scientific algorithm references, historic corpus, and this roadmap's evidence ledger.
- **Outcome/build:** publish a release certification manifest with commit/build, device/driver, fixture identity, measurement method, quality/workspace, sample counts, pass/fail, and known limitations. Update freshness/failover, analysis/uncertainty, GIS, presentation, and recovery guides around actual user workflows.
- **Interfaces/compatibility:** evidence manifest is versioned and references immutable artifacts/commits. Local diagnostics remain opt-in and redact private configured endpoints/tokens; publication contains only reviewed evidence.
- **Failure/compatibility:** missing devices, skipped required goldens, unknown presentation timing, or incomplete long runs remain open gates. Runtime failures are not excused by successful compilation or a small isolated screenshot.
- **Acceptance:** each shipped competitive claim links to completed task evidence and the applicable platform; science differences and platform capability limits are described; operator troubleshooting can explain retained stale data and recover failed workflows.
- **Proof:** certification report, documentation review, and traceable release scorecard. Claim professional readiness only when its required gates are closed.

**Agent prompt:** `Implement M7.3 using section 6. Assemble reviewed certification evidence and user documentation, leaving every unproved platform or workflow gate explicitly open.`

## 5. Validation and professional release gates

### Required engineering checks

Follow [CONTRIBUTING.md](CONTRIBUTING.md) and the existing [CI workflow](.github/workflows/ci.yml). Feature changes require:

```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Use the established Android `cargo ndk`/APK pipeline and WASM build settings in CI, including its `getrandom` configuration; do not invent different platform commands. Preserve browser bundle/smoke checks. Linux remains part of the workspace test matrix. The dedicated branch build produces Windows/Android artifacts but does not by itself replace the full checks in `ci.yml`.

Add meaningful regression tests for changed behavior, scientific results, asynchronous races, parser/resource safety, migration, and actual interaction failures. Do not mirror implementation details with tests that cannot catch an independent mistake. Network tests stay explicitly network-gated; they are not silently included in deterministic offline runs.

### Scientific, visual, and device cases

| Suite | Required coverage | Evidence standard |
| --- | --- | --- |
| Live timing/coverage | Partial/missing/late/repeated radials, wraparound, VCP changes, supplemental passes, reconnects, conflicting sources | Exact fixture identities and accepted revision/contributor assertions |
| Storms/impacts | Competing cores, ambiguity, holes, stale sources, split/merge, backward seeks, line/flank entry, cancellation | Independent association/geometry results plus mouse/touch workflow proof |
| Products/trails | Missing/folded inputs, units/datums, environment crossings, expiry, backward replay, fade, incompatible grids | Independent column/extrema computation and documented tolerance |
| Volume analysis | Coverage gaps, opacity/step consistency, signed surfaces, clipping, ROI, sample/height agreement | Numerical fixtures and backend-qualified image/performance checks |
| GIS | Real U.S. geographic/projected files, DBF/codepages, multipart/holes, archive caps, filters, restore | Independent GIS coordinate/attribute/export validation |
| Time/context | Live beside archive, differing runs, late fetches, sector changes, RGB scan alignment | Context-race tests; no current-data substitution in historical analysis |
| Output | Scene preflight/Take, timestamp/crawl, high DPI, frame mapping, missing source, recovery | Encoded-file/manifest inspection and sustained program capture |
| Android | Phone/tablet, IME/Back, keyboard/pen/touch, surface changes, imports/share, foreground/network | Physical-device recordings and resource logs; build-only evidence is insufficient |

Numeric tolerance must come from decoder precision, quantization, grid spacing, beam approximation, or a documented reference method. Record the tolerance before updating a scientific golden. Do not make expected values match a changed implementation without explaining why the new result is correct.

For visual goldens, require the fixture and golden to exist in the provisioned certification environment. A skipped/missing golden is an open result, even if an existing test runner exits successfully. Compare scientific invariants across drivers and use backend-specific image tolerances for visual rasterization differences.

### Reference workload and hardware

Benchmark release builds after cache/pipeline warmup, with local pinned inputs for client latency and a separate live-feed run for source latency. Use a 1080p presentation surface; log actual display scaling and Android render resolution. The workload has four linked REF/SRV/CC/ZDR panes with warnings, SCIT/ProbSevere, one MRMS layer, one native ABI layer, and an environmental field; phone chrome may show one selected pane while retaining the workspace. Exercise pan/zoom, tilt/product changes, probing, projection edits, and playback. Measure 3D/ROI and broadcast as additional profiles rather than hiding their costs inside an idle 2D average.

Certification matrix:

- Windows 11 x64 desktop/laptop, 16 GiB RAM, representative discrete GPU, 1080p/60 Hz; add an integrated-GPU Windows machine to verify adaptive quality and resource behavior.
- Android reference phone: Galaxy S24 Ultra or a recorded equivalent high-end device; also a supported mid-range Android device with 6 GiB RAM to expose memory/GPU limits.
- Android tablet with at least 600 dp shortest-side layout; include portrait, landscape, and a split-screen transition below the workstation threshold.
- Linux workspace build/tests and existing browser compile/smoke gates. New browser capability certification may follow native releases.

Record exact hardware rather than treating the reference categories as measurements. Hardware absence is an evidence gap, not a passing result.

### Proposed budgets — not measurements

| Metric | Desktop target | Android target | Interpretation |
| --- | --- | --- | --- |
| Client receipt to visible 2D radar update | p50 < 100 ms; p95 < 250 ms | p50 < 200 ms; p95 < 500 ms | At least 1,000 observed accepted-update samples; identify actual presentation observation method |
| Balanced active rendering | Sustained 60 FPS; p95 active frame interval ≤ 20 ms | Sustained 30 FPS; p95 active frame interval ≤ 40 ms | Five-minute interaction/playback profiles, excluding intentional idle repaint gaps |
| Input response | p95 ≤ 100 ms | p95 ≤ 150 ms | Input to observable control/map feedback during decode and fetch load |
| 3D behavior | Interactive balanced quality and bounded cache | Adaptive balanced quality and bounded cache | Record build latency separately; camera/style edits must not rebuild the scientific grid |
| Post-warmup RSS growth | Final median ≤ warm median + max(200 MiB, 10%) | Final median ≤ warm median + max(100 MiB, 10%) | Same repeating workload after caches saturate; inspect growth slope/resource counts too |
| Disabled context resources | Return to bounded retained-cache baseline | Same | Cancel obsolete jobs; no monotonically growing textures/buffers/request lanes |
| Sustained output | 30 minutes without accumulated logical-clock drift | Clean presentation/share workflow stable | Verify logical weather frame mapping separately from encoded FPS |

The latency budget is for client handling after receipt; upstream acquisition/transport delay is recorded separately and cannot be removed by claiming a faster renderer. Queue writes/submission are useful measurements but cannot certify visible latency alone. If backend/compositor presentation timestamps are unavailable, use a documented external observable frame method and retain proxy results under their own names.

Default quality is Balanced. If a device misses the rendering budget, reduce display quality within documented limits and report the selected level; do not silently change sampling/export precision. Resource thresholds are proposed regression gates, not measured current memory use or universal device guarantees. Change them only with a reviewed workload/hardware report.

### Long-run profiles

- **Desktop developer smoke:** two hours, full application, repeatable fixtures, product/site/pane changes and fault injection.
- **Desktop severe-weather:** twelve hours, active mixed layers/analysis/rendering with real live-source evidence plus separately identified deterministic faults.
- **Windows mixed stability:** twenty-four hours of active/idle, playback/live, window/output, and network state changes.
- **Android active:** two hours of radar/context/3D/input/import/presentation workflows on reference and constrained devices.
- **Android lifecycle/network:** twelve hours alternating foreground/background, network interruption/recovery, orientation/resize, and cached/live work.
- **Broadcast:** thirty minutes at the required output preset, with frame mapping, scene transitions, source failures, and independent operator activity recorded.

Use seed/scenario/build IDs; explicitly account for every injected fault. RSS and GPU counts must be sampled after equivalent warmed workload cycles. A run fails on an unrecovered feed/render/UI stall, incorrect current-time claim, corrupt accepted data, persistent obsolete jobs, unbounded resources, or a missing required measurement. Recoveries remain logged even when the final run passes.

### Release scorecard

- [ ] M1 temporal coverage, provenance, receipt/render observations, and failover evidence complete.
- [ ] M2 persistent storm workflow, uncertainty impacts, and physical input/case validation complete.
- [ ] M3 metadata, column products, truthful trails, volume modes, and linked analysis complete.
- [ ] M4 multi-layer GIS, complete native imports, restore, and independent exchange complete.
- [ ] M5 independent contexts, native satellite cadence, model field inspection, and rendering quality complete.
- [ ] M6 independent program output, complete scenes, archive-correct capture, and native presentation complete.
- [ ] Required desktop/Android long runs, performance/resource gates, CI checks, and device evidence complete.
- [ ] Scientific limitations, source/recovery guide, and dated certification manifest published.

## 6. Codex and Claude Code execution protocol

### Task sizing and progression

Select the lowest ready task in the default sequence. Each card can require several commits, but every commit must be reviewable. Split a larger card into numbered increments in the task's evidence ledger: first domain/interface seam, then one complete vertical workflow, then integrations and certification. An increment may pass without the parent card being complete.

Use existing registries, cache/render contracts, source identity, platform spawner, and action paths. Acquisition/decoding/domain logic belongs in `wxdata`; app state/render/UI integration belongs in focused app modules. Reuse current features rather than rebuilding their foundations. Keep behavior-preserving extraction distinct from feature behavior changes.

Desktop and Android acceptance must be designed together at the start of a task. Browser implementations may be recorded as deferred, but the current browser app must retain its build/runtime gates. Do not substitute an Android compile for Android usability, or a small GPU snapshot for a complete application workflow.

### Universal copyable implementation prompt

Replace `<TASK_ID>` with any card ID. Both Codex and Claude Code use this same contract:

```text
Implement <TASK_ID> from ROADMAP_PARITY.md.

Read applicable repository instructions, CONTRIBUTING.md, ARCHITECTURE.md,
the task card, its cited old-roadmap sections, and the linked code/tests.
Inspect git status and the current head; preserve unrelated local work.
Re-audit the affected baseline if it has changed since the roadmap snapshot.

Confirm the task dependencies from their evidence, not their checkboxes alone.
Implement one reviewable increment using the existing domain, registry,
rendering, cache, request-generation, and platform mechanisms. Keep substantial
new logic out of app.rs. Build the desktop and Android workflow together;
preserve current WASM/browser behavior and clearly record new browser deferrals.

Meet the card's time/provenance, missing-data, uncertainty, cancellation,
resource, migration, and failure requirements. Add meaningful tests and run
the repository's required checks plus relevant scientific/headless/device
validation. Never invent source clocks, units, quality, values, or test results.

Record implementation and verification separately with commit/platform/evidence.
Leave unsupported or untested runtime/device gates open. Finish with a handoff
using the template below. Follow the user's existing branch/publication scope;
do not include unrelated files or rewrite branch history.
```

The shorter prompt under each card can be copied with this protocol as context. Tools/models may change; task completion does not depend on a particular model version, agent delegation, or session memory.

### Handoff and evidence template

```text
Task/increment:
Baseline and implementation commit:
User workflow delivered:
Important interface/migration changes:
Inputs/fixtures and expected scientific behavior:
Commands run and actual results:
Desktop evidence:
Android evidence:
Browser compatibility and explicit deferrals:
Artifact locations / durable CI or PR links:
Implementation status:
Verification status:
Known limitations and failed/open gates:
Next ready task/increment and exact remaining work:
```

Implementation statuses: `planned`, `in progress`, `implemented`, `documented` (audit/documentation cards only). Verification statuses: `open`, `partial`, `passed`, with static/unit/GPU/application/device/soak coverage stated explicitly. M0.1's static pass does not imply any release gate passed. Do not close a feature's verification status until all required platforms/scenarios for that card have evidence.

Keep a small dated evidence ledger under the completed card, referencing commit and artifact links. Store bulky temporary output in ignored build/artifact locations. Never rely on a local `target/` path as the only lasting release evidence.

### Required completion behavior

- Correct a demonstrated blocker within task scope; report unrelated baseline failures without rewriting unrelated code.
- Stop stale/canceled work and ignore stale results; a background job finishing does not authorize it to overwrite a newer context.
- Respect serialization compatibility and the Kotlin settings-name/coalesced-write contract.
- Keep scientific value and display transformations separate; interpolation, opacity, shading, and decimation appear in provenance where relevant.
- Explain measured limits and the inputs supporting them. Do not claim competitor equivalence from marketing checklists or source presence alone.
- Update the task's evidence/status after acceptance work. A remaining physical-device, scientific, or soak gate stays open even when code is merged.

## 7. Later backlog and maintenance

Keep these useful expansions after the critical competitive workflow, linked back to their existing ROADMAP_NEW sections:

- Additional model families and RRFS/REFS delivery, broader ensemble postage stamps/plumes, and larger parameter catalogs (F2, F7).
- Advanced objective surface analysis and station/analysis series (G2–G4).
- Expanded algorithm laboratory, forecasting/backtesting research, and fully bundled offline case datasets (C5, K1–K3).
- Multi-radar storm volume fusion, experimental dual-Doppler synthesis, and additional feature tracking (R1–R4).
- Further route/chase/offline-pack expansion beyond the existing geometry and asset impacts used here (L3–L5).
- Plugin manifest/interoperability expansion and new extensions (P1–P3).
- Additional international coverage, new iOS delivery, and separate macOS product work.

Do not gate core professional certification on experimental science or copying every competitor layer. Revisit these priorities after measured native workflow quality and operator feedback.

When code changes, refresh affected matrix rows and task evidence against the new commit. Retain historical audit dates rather than silently moving the original baseline. New competitor claims require dated primary references and a clear released-versus-announced distinction. Change dependencies or budgets only with an explanation of the operator/scientific consequence.

## 8. References

Repository sources:

- [ROADMAP_2.md](ROADMAP_2.md): professional maturity, workstation workflows, release gates.
- [ROADMAP_NEW.md](ROADMAP_NEW.md): U.S. analyst architecture, capabilities, broader backlog.
- [ARCHITECTURE.md](ARCHITECTURE.md) and [CONTRIBUTING.md](CONTRIBUTING.md): boundaries, workflow, required checks.
- [Workstation design plan](docs/WSV3_IMGUI_MODERN_DESIGN_PLAN.md): existing workstation behavior and interaction model.
- [Technical reference](docs/technical-reference.md), [data guide](docs/DATA.md), and [Android guide](android/README.md): current capabilities and platform boundaries; re-check against code where documentation is older.
- [General CI](.github/workflows/ci.yml), [branch native builds](.github/workflows/wsv3-redesign-build.yml), and [provider/corpus schedule](.github/workflows/provider-contracts.yml): established validation/build paths.

Primary competitor references consulted for the 2026-10-01 audit:

- [WSV3 official overview](https://wsv3.com/): released product capabilities and distinction from the next-generation line.
- [WSV3 Professional user guide](https://wsv3-static.s3.amazonaws.com/WSV3UserGuide.pdf): established radar, tracking, GIS, and operator workflows; this guide predates the latest release and is not a current-version exhaustive inventory.
- [WSV3 development updates](https://wsv3-next-gen-2025.com/forum/d/64-development-update/227): dated development context; announcements and unfinished work are not treated as released baseline requirements.
- [GR2Analyst 2 official page](https://www.grlevelx.com/gr2analyst_2/) and [version 2 user guide](https://www.grlevelx.com/manuals/gr2analyst_2/): core analysis/volume baseline.
- [GR2Analyst 3 official page](https://www.grlevelx.com/gr2analyst_3/) and [user-defined products](https://www.grlevelx.com/gr2analyst_3/udp.htm): current user-product/trail comparison targets and column-oriented product design.

The task specifications above are HookEcho engineering decisions based on the code gaps and requested priorities. They are not claims that competitor implementations use identical algorithms, interfaces, thresholds, or architectures.
