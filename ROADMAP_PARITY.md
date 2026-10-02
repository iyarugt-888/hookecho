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
| Storm selection and association | [Storms dock](crates/hookecho/src/app/chrome/dock/storms.rs), [spatial associations](crates/hookecho/src/app/chrome/dock/storm_associations.rs), [cell analysis](crates/hookecho/src/app/chrome/dock/cell.rs) | Spatial evidence preserves source objects, polygon holes, distance limits, and ambiguity. It is not a persistent storm identity with temporal confidence and split/merge history. | M2.1–M2.2 |
| Manual motion and impacts | [Motion tool](crates/hookecho/src/app/storm_track.rs), [shared geometry](crates/hookecho/src/app/storm_track_geometry.rs), [Census impacts](crates/wxdata/src/census.rs) | Points, lines, intervals, asymmetric widths, cones, handle constraints, and zone/marker ETAs exist. Complete geolocated community/asset impacts, case persistence, and device interaction proof. | M2.3–M2.4 |
| Gate inspection and scientific metadata | [Gate/beam metadata](crates/wxdata/src/level2.rs), [gate inspector](crates/hookecho/src/ui/gate_inspector.rs), [beam model](crates/wxdata/src/beam_geometry.rs) | Nyquist is estimated from observed values, not surfaced as decoded unambiguous velocity. Preserve raw vs processed values, acquisition context, and source quality distinctly. | M3.1 |
| User-defined products | [Expression engine](crates/wxdata/src/udp.rs), [gate-grid evaluation](crates/wxdata/src/udp_volume.rs), [pane product cache](crates/hookecho/src/view.rs), [product editor](crates/hookecho/src/ui/udp_window.rs) | Gate products render in 2D and 3D. Column formulas remain probe-only; validation, chosen palettes, portable product files, environmental requirements, and −10 °C height remain incomplete. | M3.2–M3.3 |
| Temporal extrema | [Accumulator and decay](crates/wxdata/src/extrema.rs), [trail interaction regressions](crates/hookecho/src/app/trail_status_tests.rs) | Partial workflow. Accumulation is one site/tilt and uses cached frames. Existing decay changes codes/values. Separate physical extrema from display age and support expiring windows, derived products, and grids. | M3.4 |
| Volume analysis | [Volume builder](crates/wxdata/src/volume3d.rs), [MIP shader](crates/hookecho/src/shaders/raymarch.wgsl), [opacity/slice controls](crates/hookecho/src/ui/volume3d_window.rs), [isosurfaces](crates/wxdata/src/isosurface.rs), [3D playback](crates/hookecho/src/app/view3d_state.rs) | MIP, opacity controls, isosurfaces, clipping, CAPPI reference, and playback already exist. A four-point opacity curve on MIP does not establish lit translucent volume parity. Extend rendering, ROI, measurement, and linked slicing. | M3.5–M3.6 |
| GIS | [Import integration](crates/hookecho/src/app/gis_import.rs), [geometry/styles](crates/hookecho/src/gis_import.rs), [Shapefile](crates/wxdata/src/shapefile.rs), [projection](crates/wxdata/src/projection.rs), [KML/KMZ](crates/wxdata/src/kml.rs), [export](crates/hookecho/src/gis_export.rs) | Formats, U.S. projections, styling, labels, and time filtering exist. Settings retain one imported layer; Android/browser single-file picking can omit shapefile sidecars. Complete collections, groups, filters, and real interoperability. | M4.1–M4.4 |
| Linked workspaces | [Workspace state](crates/hookecho/src/workspace.rs), [time selection](crates/wxdata/src/time_align.rs), [pane synchronization](crates/hookecho/src/app/pane_time.rs), [field state](crates/hookecho/src/app/field_state.rs) | Shared time/camera/cursor and comparisons exist. Link controls are global; some source contexts are shared. Independent groups need context-safe request and cache identity. | M5.1 |
| Satellite and model context | [ABI ingest](crates/wxdata/src/goes_abi.rs), [time integration](crates/hookecho/src/app/time_layers.rs), [model descriptors](crates/wxdata/src/model.rs), [MRMS catalog](crates/wxdata/src/mrms/catalog.rs), [GEFS](crates/wxdata/src/ensemble.rs) | The MRMS catalog contains 35 descriptors, and NAM/NAM nest/NBM and GEFS foundations already exist. Native ABI mesoscale frames follow radar time; a satellite-only one-minute loop remains open. Model field discovery is narrower than generic GRIB inspection. | M5.2–M5.3 |
| Broadcast and capture | [Clean output viewport](crates/hookecho/src/app/output_window.rs), [scene model](crates/hookecho/src/broadcast.rs), [scene application](crates/hookecho/src/app/scenes.rs), [loop capture](crates/hookecho/src/app/loop_capture.rs) | Output, dressing, scenes, and deterministic capture foundations exist. Output follows the active pane; scenes omit parts of full analysis state. Complete independent program state and sustained output proof. | M6.1–M6.4 |
| Resilience and corpus | [Data-path soak](crates/hookecho/src/soak.rs), [CPU frame telemetry](crates/hookecho/src/app/telemetry.rs), [historic tests](crates/wxdata/tests/golden_events.rs), [scheduled contracts/corpus](.github/workflows/provider-contracts.yml) | The soak runner decodes/bins radar without exercising the full renderer/UI. Historic cases are network-gated and dynamically locate archive scans. Add pinned offline inputs, resource accounting, full application stress, and Android lifecycle certification. | M0.3, M7.1–M7.3 |
| Architecture and input | [App shell](crates/hookecho/src/app.rs), [frame intake](crates/hookecho/src/app/frame_intake.rs), [pane drawing](crates/hookecho/src/app/draw_panes.rs), [frame end](crates/hookecho/src/app/frame_end.rs), [Android platform](android/README.md) | Extraction is substantial, but modules still share a large app owner. Narrow ownership and testable interfaces must accompany further extraction. Compile checks do not verify tablet targets, pen gestures, IME, or lifecycle behavior. | M0.2, M2.4, M7.2 |

### Corrections to older checklist assumptions

1. User-product map rendering, opacity controls, isosurfaces, and 3D playback should not be planned as entirely absent features. Their remaining limits are narrower and are named above.
2. Shapefile/KML/KMZ, projection handling, and GIS styling have progressed beyond the older summary acceptance statements. Their real gaps are collections, complete imports, advanced inspection, and external-tool verification.
3. Radial gap inventories already exist in `live_scan`; an earlier progress paragraph saying they remain open is stale.
4. Scheduled source contracts and the historic storm job already exist. Extend their reproducibility and coverage rather than proposing the schedule again.
5. `ROADMAP_NEW` immediate-start instructions and final scorecard contain older unchecked foundations, including caching and registry work. Re-audit before treating those checkboxes as missing code.
6. A listed or tested feature remains unverified for a platform until the operator workflow is demonstrated there. Do not convert a compile, isolated render, or synthetic gesture test into a physical-device claim.

## 3. Delivery sequence and interface contracts

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

**Priority:** P0. **Depends on:** M0.1. **Status:** implementation: in progress (increments 1–4 delivered); verification: partial. **Original references:** ROADMAP_2 §8; ROADMAP_NEW §§K2, K3, 25.

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

**Remaining/next increment:** add independently labeled clutter before closing the parent card. The partial tornado/hail files are decoding controls; they do not establish full storm detection or column accuracy. Clear-air clutter tags remain candidates. Android/Linux/browser runtime and sustained application certification remain open.

### M1 — Live-data trust

#### M1.1 — Carry temporal coverage into every radar representation

**Priority:** P0. **Depends on:** M0.2, M0.3. **Original references:** ROADMAP_2 §§1.2, 9, 10; ROADMAP_NEW §§B2, B5, H8.

- **Starting evidence:** `LiveScanState`, per-azimuth times, strict-current 2D masking, `Volume` revisions, derived grids, and 3D playback readiness.
- **Outcome/build:** propagate acquisition intervals, cut/pass identities, contributor revisions, and completeness into derived products and 3D builds. Continuous mode retains older contributions with a mixed-time indication; strict mode excludes them and displays incomplete coverage. Distinguish unobserved chunks from proven radial gaps on a mid-volume join.
- **Interfaces/compatibility:** use shared temporal coverage and frame identity. Invalidate products when an actual contributor changes, including supplemental low-level passes; do not let an older completed build replace a newer accepted revision.
- **Failure/compatibility:** do not fill missing gates with zero or extrapolate time from the client clock. Retain continuous mode as the default and preserve existing 2D display controls.
- **Acceptance:** fixtures cover SAILS/MRLE, VCP change, repeat elevation, late radial, gap fill, reorder, mid-volume join, and missing upper cuts; 2D, derived, and 3D metadata agree with contributors actually shown.
- **Proof:** temporal-policy tests and captures of continuous/strict incomplete volumes.

**Agent prompt:** `Implement M1.1 using section 6. Extend existing scan/revision metadata into derived and 3D outputs, preserving continuous defaults and proving strict-mode coverage.`

#### M1.2 — Trace receipt through completed rendering

**Priority:** P0. **Depends on:** M0.2, M1.1. **Original references:** ROADMAP_2 §§1.5, 14; ROADMAP_NEW §§B3, O1, O4.

- **Starting evidence:** `LiveQueueTimings`, the monotonic receipt carried into 2D uploads, CPU frame telemetry, and profiling feature.
- **Outcome/build:** correlate receipt, decode, UI acceptance, submission, GPU completion when supported, and observable presentation with frame identity. Report p50/p95, sample count, dropped/coalesced updates, source mode, and measurement stage. Profile copying, rebinning, allocation, and 3D full uploads before optimizing their dominant cost.
- **Interfaces/compatibility:** bounded local timing samples; separate CPU, GPU, and presentation clocks. Use submission callbacks/timestamp capabilities where supported. Where compositor presentation is unavailable, name the proxy and use external frame observations for visible-latency certification.
- **Failure/compatibility:** exclude unsupported/unknown samples rather than treating them as zero. Completed-volume providers without comparable receipt stamps remain a separate population. Do not add tracking or mandatory profiling servers.
- **Acceptance:** one sample identifies the revision that was drawn; queue writes are never labeled visible latency; a changed 2D sector avoids unchanged texture rows; desktop/Android benchmark reports use the stage definitions in section 5.
- **Proof:** correlated traces, documented observation method, allocation/upload profiles, and platform capability coverage.

**Agent prompt:** `Implement M1.2 using section 6. Extend the existing local timing path and label every measured stage accurately before changing performance-critical code.`

#### M1.3 — Prove source failover under severe-weather load

**Priority:** P0. **Depends on:** M0.3, M1.1, M1.2. **Original references:** ROADMAP_2 §§1.3, 3; ROADMAP_NEW §B6.

- **Starting evidence:** provider capabilities, health/arbiter state, continuation rules, optional ingest relay, and completed-volume fallback.
- **Outcome/build:** exercise and repair the existing failover path through mid-volume transport loss, stale-but-responsive upstreams, conflicting radial identities, missing chunks, source restart, and restoration of the preferred tier. Display source, reason, retained-data age, and any loss of temporal resolution.
- **Interfaces/compatibility:** retain canonical radial/cut identity and safe continuation refusal. Extend health reporting with upstream failure-domain identity so two transports sharing one source are not presented as independent redundancy.
- **Failure/compatibility:** incompatible volume/cut data is not spliced together. Backoff/retry are bounded; foreground resume refreshes health; core direct-source operation remains available without a relay.
- **Acceptance:** no displayed acquisition reversal, indefinite silent freeze, or unannounced fallback downgrade; deterministic fault scenarios recover; a sustained live relay/direct-source session produces evidence distinct from simulator results.
- **Proof:** fault scenario manifest, transition logs, progressive/completed fallback captures, and live-session report.

**Agent prompt:** `Implement M1.3 using section 6. Extend existing failover tests and repair demonstrated continuity/recovery gaps without replacing the provider architecture.`

#### M1.4 — Complete retained provenance and scientific lineage

**Priority:** P0. **Depends on:** M0.2, M1.1. **Original references:** ROADMAP_2 §§9, 10; ROADMAP_NEW §§A1, A2, N1, N2.

- **Starting evidence:** `DataStamp`/`Stamped`, field descriptors, probe rows, source health, observed/forecast/derived flags, and current archive receipt limitations.
- **Outcome/build:** retain provenance with cached/archived payloads; migrate observations, warnings, detections, and trail/derived outputs to common metadata access. Record algorithm version, input clocks, temporal coverage, native grid/units, processing/interpolation, and input quality. Inspectors, captions, exports, and diagnostics consume this same record.
- **Interfaces/compatibility:** add optional lineage and timing fields without repurposing existing stamp meanings. Do not substitute fetch-success time for unknown original archive receipt. Transformations retain source identity and add their own processing description.
- **Failure/compatibility:** missing metadata produces an explicit unavailable field. Unknown quality is not upgraded to good. Last-good data retains its original valid time after retries or cache reads.
- **Acceptance:** a coverage inventory accounts for every major displayed layer; archive, live, forecast, observation, and derived fixtures expose correct clocks and offsets; round trips preserve unknown values and old settings/cases load.
- **Proof:** layer provenance inventory, constructor/serialization tests, and matched inspector/export examples.

**Agent prompt:** `Implement M1.4 using section 6. Finish retained stamps and dependency lineage through the shared inspector/export path; never invent an unavailable source clock.`

**Source-clock foundation — 2026-10-01 (`7079846`):** the Level III decoder retains checked PDB data-start, RPG generation, and DVL/EET volume-end clocks separately. Supplemental elevation starts keep their source meaning; free-text generation is not labeled acquisition. Gridded DVL/EET/HHC/N0B products and SCIT history/forecast origins use the decoded data start, without S3-name or client-clock substitution. An undated grid is unavailable. This corrects a production issue discovered while pairing M0.3's candidate HCA clutter labels with their Archive II volume. Six existing real products, one-based epoch/midnight boundaries, invalid day/second encodings, and independent Python header interpretation verify the decoder. Serialized clocks preserve unknown values and their distinct roles. Shared grid lineage, persisted archive receipts, inspectors/exports, and the remainder of this parent card remain open.

**Foundation verification:** Windows workspace tests passed (2,019; zero failures; 115 explicitly ignored), and workspace/all-target Clippy passed. Five source-clock tests, including JSON unknown/role preservation, and eight existing MetPy value/geometry goldens passed separately. The WASM library check initially exposed a native-only Rayon import in the concurrent LLSD shear feature (`08df4ae`). The row calculation now runs sequentially on WASM and retains parallel ordered collection on native; the repeated workspace/Clippy checks and full HookEcho WASM library check passed. Existing browser warnings remain. This is compilation/unit evidence, with browser runtime, device, and sustained-load certification still open. Review logs are `target/parity-review/m0.3/clock-*.log`. CHANGELOG.md now records the implemented parity foundations and other committed branch features.

### M2 — Storm-centered operator workflows

#### M2.1 — Introduce persistent storm identity and history

**Priority:** P0. **Depends on:** M0.2, M0.3, M1.4. **Original references:** ROADMAP_2 §2.1; ROADMAP_NEW §§R3, R4, R5.

- **Starting evidence:** SCIT histories, spatial warning/ProbSevere associations, competing circulation assignment, and detection track modules.
- **Outcome/build:** add a domain storm-history owner with stable local IDs and timestamped source references. Associate by spatial/temporal evidence and plausible motion while retaining alternatives, confidence reasons, provider-ID changes, and split/merge lineage. Manual objects may remain unassociated.
- **Interfaces/compatibility:** associations refer to original observations, not copied flattened storm attributes. Record time/distance tests and the motion source used; calibrate association limits from pinned fixtures and version those limits with the model.
- **Failure/compatibility:** stale sources lose confidence; ambiguous signatures remain ambiguous. Provider IDs are not globally unique storm IDs, and incomplete data does not force a merge.
- **Acceptance:** adjacent storms, crossings, polygon holes, competing signatures, missing updates, recycled IDs, splits/merges, and backward archive seeks produce deterministic histories; same inputs replay to the same identities.
- **Proof:** association/lineage fixture reports and inspectable explanations for confirmed, tentative, and unmatched objects.

**Agent prompt:** `Implement M2.1 using section 6. Build persistent history around the existing spatial associations, preserving every source object and uncertain alternative.`

#### M2.2 — Unify storm selection, trends, and tool entry

**Priority:** P0. **Depends on:** M2.1. **Original references:** ROADMAP_2 §§2.5, 12; ROADMAP_NEW §§J3, J5, J6.

- **Starting evidence:** Storms dock filtering/navigation, Cell analysis, follow actions, trend samples, gate/layer probes, and 3D/cross-section actions.
- **Outcome/build:** map and table selection target one selected storm identity. Show source-timed attributes, trend gaps, warnings, ProbSevere, rotation/TDS/hail evidence, automatic/manual motion, and projections in one responsive analysis surface. Provide direct open/follow/3D/cross-section actions and preserve selection through supported continuity.
- **Interfaces/compatibility:** common selection actions and a view model derived from the history owner. Existing keyboard/global focus ownership remains shared with card buttons and touch controls.
- **Failure/compatibility:** distinguish missing, stale, and changed-identity attributes. Do not smooth trends across unavailable periods or let a newly adjacent cell silently inherit selection.
- **Acceptance:** selecting from either entry point shows the same entity and clocks; history updates do not steal focus; phone, tablet, narrow desktop docks, and keyboard navigation reach all critical actions.
- **Proof:** operator walkthrough, selection/focus regressions, and desktop/Android captures.

**Agent prompt:** `Implement M2.2 using section 6. Connect map and table selection to the same storm history and complete the responsive analysis workflow.`

#### M2.3 — Extend impacts to geolocated communities and assets

**Priority:** P0. **Depends on:** M2.1, M1.4. **Original references:** ROADMAP_2 §§2.3, 2.4; ROADMAP_NEW §§L3, L4, R5.

- **Starting evidence:** shared point/line uncertainty geometry, marker/zone entry checks, Census summaries, and existing GIS geometry types.
- **Outcome/build:** query geolocated community and user-selected asset geometries against the existing projection. Show first/last possible impact, center-path entry, uncertainty-flank-only contact, closest approach, and motion/source age. Support points, lines, and polygon zones; consume collections once M4.1 lands through the same geometry seam.
- **Interfaces/compatibility:** one impact result contains target identity/geometry reference, analysis time, motion provenance, and estimated interval. Extend the existing Census 2020 TIGERweb place service with attributed geometry/representative points, retaining source year and population meaning; representative-point ETA is labeled separately from area entry.
- **Failure/compatibility:** names/populations without coordinates do not receive ETAs. Population stays an estimate with dataset date; an arrival projection is not a forecast guarantee. Cache by geometry/motion identity and cancel superseded lookups.
- **Acceptance:** zero/slow motion, inside-now, asymmetric flanks, bent squall lines, narrow crossed assets, holes, and edited vectors agree between drawing and impacts; missing location/time produces an explicit unavailable result.
- **Proof:** geometry fixture comparisons and a desktop/Android community/asset-impact example.

**Agent prompt:** `Implement M2.3 using section 6. Extend the existing shared projection geometry to geolocated communities and asset targets, preserving uncertainty and time semantics.`

#### M2.4 — Certify editing and preserve historical manual work

**Priority:** P0. **Depends on:** M2.2, M2.3. **Original references:** ROADMAP_2 §§2.6, 13; ROADMAP_NEW §§K3, Q1, Q3.

- **Starting evidence:** drag origin/owner rollback, keyboard ownership tests, motion constraints, phone/tablet form factor, and case manifests.
- **Outcome/build:** finish mouse, keyboard, pen, and touch creation/edit/duplicate/delete workflows. Add touch-reachable equivalents for modifier and hover actions. Persist manual tracks and storm annotations in analysis cases with their origin time and source association; historical estimates reopen as historical.
- **Interfaces/compatibility:** version the case format for new saved objects, retain old-case reading, and keep imported annotations additive. Restore a live motion estimate only after explicit reactivation; session motion remains time-bounded.
- **Failure/compatibility:** pointer cancellation, focus loss, pane removal, orientation change, pinch takeover, and Back restore or cancel edits consistently. Text input retains its keys; secondary pointers do not edit the primary handle.
- **Acceptance:** a trained operator creates a 60-minute point projection within five seconds on mouse and touch; pen/touch completion and all cancellation paths are demonstrated on physical Android hardware; case round trips preserve geometry, controls, and time.
- **Proof:** deterministic input regressions, case fixtures, and timestamped physical-device walkthroughs.

**Agent prompt:** `Implement M2.4 using section 6. Close touch/pen editing gaps and save historical manual work in cases without silently reactivating stale motion.`

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

#### M3.2 — Complete portable, validated product definitions

**Priority:** P1. **Depends on:** M1.4, M3.1. **Original references:** ROADMAP_NEW §C1; ROADMAP_2 §§9, 15.2.

- **Starting evidence:** `ProductDef`, AST/input discovery, recursion limits, Settings persistence, and gate-product ramp rendering.
- **Outcome/build:** add versioned product JSON import/export, stable IDs, chosen palettes, explicit dependencies, output kind, environmental requirements, units/quantity, and altitude convention. Validate expressions and bound operation cost as well as recursion; provide editor diagnostics and reference examples.
- **Interfaces/compatibility:** preserve legacy unadorned literals using contextual input units. Identify incompatible typed operands and height datums. Existing `BEAM_HEIGHT_M` is antenna-relative; do not silently call it terrain AGL. Environmental isotherm heights and `BEAM_ALTITUDE_M` use MSL. True AGL needs terrain metadata.
- **Failure/compatibility:** old definitions remain importable with their original semantics; incompatible/unsafe ones are disabled with edit/review diagnostics. Missing inputs propagate as missing, not zero; user formulas do not execute arbitrary code.
- **Acceptance:** old/new round trips retain identity and semantics; malformed/deep/expensive formulas remain bounded; incompatible units/datums are explained; map/probe/export palettes and ranges agree.
- **Proof:** compatibility and resource-limit fixtures, portable example products, and editor/map captures.

**Agent prompt:** `Implement M3.2 using section 6. Extend the existing safe DSL and product editor with portable definitions, palettes, dependency validation, and explicit altitude conventions.`

#### M3.3 — Render and export column-based products

**Priority:** P1. **Depends on:** M0.2, M1.1, M3.2. **Original references:** ROADMAP_NEW §§C1, F8, H6; ROADMAP_2 §§9, 11.3.

- **Starting evidence:** `evaluate_at_column`, existing local-derived horizontal grids, gate-product evaluation, freezing-level fetches, and profile/sounding infrastructure.
- **Outcome/build:** evaluate column formulas on the local-derived horizontal analysis grid, with source-gate coverage and configurable resolution within quality budgets. Render through the existing field pipeline; share values with probes, thresholds, trails, and scientific exports. Provide 0/−10/−20 °C environmental heights from time-aligned temperature profiles when available.
- **Interfaces/compatibility:** column output is a 2D field, not a fake sweep or 3D voxel moment. For −10 °C, interpolate crossings only between valid bracketing profile levels; use the lowest crossing encountered while ascending in altitude, regardless of temperature gradient, and report multiple crossings/uncertainty. Record MSL datum, source/run/valid time, and interpolation.
- **Failure/compatibility:** no bracketing temperature, unsupported archive profile, or out-of-tolerance environment means missing output with an explanation. Cancel obsolete work and retain no current environment over a historical volume by substitution.
- **Acceptance:** composite-reflectivity, masked CC minimum, and ZDR-above-isotherm examples match independent column calculations within input/quantization tolerance; changes to inputs/time invalidate the right cache; incomplete beams remain identified.
- **Proof:** reference columns, grid/probe/export equality tests, and live/archive native workflow captures. GPU expression generation is optional after CPU correctness/performance is established.

**Agent prompt:** `Implement M3.3 using section 6. Render column formulas through the existing field pipeline and derive time-aligned environmental inputs without inventing missing profile data.`

#### M3.4 — Preserve physical extrema while aging trails

**Priority:** P1. **Depends on:** M0.3, M1.4, M3.3. **Original references:** ROADMAP_NEW §C2; ROADMAP_2 §§8, 15.2.

- **Starting evidence:** polar extrema, code-changing decay, cached-window rebuilding, threshold outlines, and trail raster export.
- **Outcome/build:** retain physical extrema separately from contributing time/coverage and age opacity. Support exact expiring windows with bounded frame storage and reusable/block summaries; rebuild deterministically on backward seek or incompatible geometry. Add user-column and MRMS MESH/AzShear trails using their own native grid identity.
- **Interfaces/compatibility:** include requested/actual time coverage, missing-frame count, grid/beam identity, mode, source product, value, and contributor time. Age fade affects display opacity only. Existing fade settings migrate to this display behavior with a release note; reset/threshold/export controls remain reachable.
- **Failure/compatibility:** raw/dealiased velocity and changed value ranges are not merged as if equivalent. Optional bounded prefetch explicitly shows progress/cancellation; a short cached history is not advertised as the full requested window.
- **Acceptance:** advancing past the strongest old frame removes its contribution; backward/reordered inputs rebuild consistently; numeric probes/exports are unchanged when fade toggles; sentinels never win; polar/grid product trails retain correct units and timestamps.
- **Proof:** brute-force reference extrema comparisons, expiry/coverage fixtures, and independently recomputed historic hail/rotation trails.

**Agent prompt:** `Implement M3.4 using section 6. Separate extrema from age opacity, implement truthful sliding windows, and extend trails to user-column/MRMS fields.`

#### M3.5 — Add translucent volume rendering and richer surfaces

**Priority:** P1. **Depends on:** M0.3, M1.1, M1.2, M3.2. **Original references:** ROADMAP_NEW §§H1, H2, H3, H7; ROADMAP_2 §§11.2, 14.

- **Starting evidence:** MIP raymarch, four-point opacity curve, multi-moment map volumes, marching surfaces, and coverage-aware voxel sampling.
- **Outcome/build:** keep MIP and add front-to-back alpha compositing with early termination, step-length opacity correction, optional gradient lighting, and empty-space skipping where beneficial. Add editable ordered color/opacity stops and saved product presets. Extend existing isosurfaces to multiple thresholds and signed velocity pairs within mesh budgets.
- **Interfaces/compatibility:** rendering mode and versioned transfer-function presets use product physical units. Adapt legacy four-point curves without changing existing MIP output. Lighting/color changes never alter sampled or exported scientific values.
- **Failure/compatibility:** unsupported GPU limits produce a capability explanation or existing supported mode. Missing coverage stays transparent; velocity surfaces represent sampled radial velocity, not synthesized 3D wind. Never extrude a 2D hail field into a claimed measured volume.
- **Acceptance:** synthetic density/coverage cases distinguish MIP and compositing correctly; opacity stays stable when step size changes; clipping/transfer edits avoid scientific-grid rebuilds; multiple surfaces respect bounds and desktop/Android balanced budgets.
- **Proof:** GPU image checks, numerical opacity references, reference-event renders, and performance traces.

**Agent prompt:** `Implement M3.5 using section 6. Extend the existing volume/surface pipelines with true compositing and reusable transfer functions while preserving MIP and value semantics.`

#### M3.6 — Link storm-centered 3D, slices, and measurement

**Priority:** P1. **Depends on:** M2.2, M3.3, M3.5. **Original references:** ROADMAP_2 §11; ROADMAP_NEW §§C3, H4, H5, H6, H8, J3.

- **Starting evidence:** radar-wide volume bounds, Cell View in 3D, cross-section lines, vertical clip/slab controls, CAPPI reference, and frame-keyed 3D playback.
- **Outcome/build:** build higher-detail geographic ROI volumes around a selected storm or drawn region. Share slice endpoints/plane, CAPPI altitude, crosshair, and selection between map, cross-section, and 3D. Add translation/rotation controls and rulers for horizontal/slant distance, MSL, antenna-relative height, terrain AGL when available, and sampled value.
- **Interfaces/compatibility:** cache identity includes ROI, product, data revision, grid, and coverage policy; camera/style changes reuse data. Environmental surfaces use M3.3's source/time/datum, with independent visibility. Optional follow mode translates with the selected history, keeping its behavior explicit.
- **Failure/compatibility:** lost association stops following without jumping to an unrelated storm. Missing DEM disables true AGL; the 4/3-earth beam model remains an approximation. Slow/failed 3D builds retain an explicitly older frame or show unavailable state, never a false current timestamp.
- **Acceptance:** translating/rotating a cut updates linked views; camera motion does not regrid; ROI improves measured detail; 3D/probe/cross-section samples agree within stated resampling tolerance; replay preserves camera and frame identity on both native platforms.
- **Proof:** geometry/sample tests, cache/build counters, storm-centered walkthrough, and device playback captures.

**Agent prompt:** `Implement M3.6 using section 6. Link existing slice/probe tools around a storm-centered ROI and prove datum, cache, and playback consistency.`

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

#### M4.2 — Complete native imports with shapefile bundles

**Priority:** P1. **Depends on:** M0.3, M4.1. **Original references:** ROADMAP_NEW §§I1, I2, I3, Q1; ROADMAP_2 §5.1.

- **Starting evidence:** shapefile geometry/DBF parsing, `.prj` projection handling, KMZ bounded ZIP decoding, platform file handover, and current sidecar omission.
- **Outcome/build:** accept ZIP shapefile bundles with `.shp`, `.dbf`, optional `.shx`, `.prj`, and `.cpg`. Match sidecars case-insensitively by basename. For multiple datasets, show a dataset picker. Preserve content in app-managed Android storage so a temporary picker URI is not the long-term source.
- **Interfaces/compatibility:** reuse existing geometry/projectors and archive decoding seams. Support UTF-8, Windows-1252, and Latin-1 codepage declarations initially; diagnose other encodings. Enforce bounded expansion/read/allocation against platform resource policy, with cancellation and import summary.
- **Failure/compatibility:** reject mismatched DBF rows, ambiguous sidecars, unsupported CRS/datum, encrypted/unsupported ZIP forms, and excessive expansion clearly. No network-link fetching is implied by KML import. Plain `.shp` imports still work and disclose absent attributes.
- **Acceptance:** real Census county/place and representative emergency-asset exports import on desktop/Android with matching attributes and projected position; malformed/truncated archives stay bounded; non-WGS84 fixtures match independent GIS coordinates.
- **Proof:** attributed real-world fixtures, archive/parser adversarial checks, and complete Android picker/import walkthrough.

**Agent prompt:** `Implement M4.2 using section 6. Reuse existing GIS decoders to import complete shapefile bundles on Android and desktop, preserving CRS and attributes.`

#### M4.3 — Finish styling, feature inspection, and filtering

**Priority:** P1. **Depends on:** M4.1, M4.2. **Original references:** ROADMAP_2 §§4.3, 5.2–5.4; ROADMAP_NEW §§I4, I5.

- **Starting evidence:** category/graduated colors, fixed dot symbols, label attributes, minimum zoom, polygon click popups, and imported priority.
- **Outcome/build:** add typed attribute filters, separate fill/stroke/opacity, line dashes, point symbols/size, min/max zoom, label templates/halo/priority, and a feature table. Clicking points, lines, or polygons selects the same source feature; support copy values, zoom to selection, and export selected/filtered items.
- **Interfaces/compatibility:** filter expressions use a bounded parser with string/number/boolean comparisons; missing attributes remain missing. Per-layer legends explain categorical/graduated choices. Style changes do not mutate source values or geometry.
- **Failure/compatibility:** invalid filters retain the previous valid filter and show a diagnostic. Official warnings keep operational hit-test priority; ambiguous nearby features remain selectable through a chooser. Expensive filtering runs off-frame and cancels on edits.
- **Acceptance:** filters, time ranges, zoom visibility, feature table, export, and hit-test sets agree; selected features remain legible without destabilizing labels; Android has tap-accessible inspection and filter controls.
- **Proof:** typed-filter fixtures, point/line/polygon hit tests, and dense asset-layer captures.

**Agent prompt:** `Implement M4.3 using section 6. Complete per-layer rules and shared feature inspection without modifying original geometry or warning interaction priority.`

#### M4.4 — Restore and exchange GIS scenes reliably

**Priority:** P1. **Depends on:** M2.4, M4.3. **Original references:** ROADMAP_2 §§5.5, 12.2; ROADMAP_NEW §§I6, K3, K4, M5.

- **Starting evidence:** workspace/case formats, visible-map GeoJSON export, route geometry, contour algorithms, and analysis ZIP export.
- **Outcome/build:** persist GIS collections/groups and styles in workspaces and scenes. Add selected/filtered GIS export, true manual projected-track geometry, existing route geometry, and available threshold/contour vectors with source/time/unit metadata. Keep the default visible-map export behavior intact.
- **Interfaces/compatibility:** version portable manifests as needed and resolve layer IDs/assets on import. Package user-chosen assets or retain explicit external references with checksums; do not bundle secret settings or unrelated user files. Normalize vector output to documented geographic coordinates.
- **Failure/compatibility:** report missing assets and unsupported exports before completion; route export uses existing route data rather than waiting for a new route engine. Invalid/open rings and no-data contours are handled consistently.
- **Acceptance:** old workspaces/cases open; new scenes restore multiple layers; selected/filtered exports round-trip; GeoJSON/GeoTIFF outputs open in independent GIS tools with correct coordinates, values, units, and time.
- **Proof:** manifest migration tests, QGIS or equivalent independent-reader reports, and restore/export walkthroughs.

**Agent prompt:** `Implement M4.4 using section 6. Complete portable GIS/workspace persistence and export existing analytical geometry with independent-tool verification.`

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

#### M5.2 — Add satellite-native one-minute playback

**Priority:** P1. **Depends on:** M0.3, M1.4, M5.1. **Original references:** ROADMAP_NEW §§E1, E4, E5, E7; ROADMAP_2 §§10, 14.

- **Starting evidence:** ABI channel/RGB/mesoscale ingest, scan discovery, dynamic footprints, source-time following, and existing time alignment.
- **Outcome/build:** expose native ABI frame listings and a satellite-driven timeline, including intermediate one-minute mesoscale frames between radar scans. Allow satellite to drive a linked group while other layers resolve their own nearest valid frame. Add bounded loop prefetch and cancellation, retaining per-scan sector footprints.
- **Interfaces/compatibility:** frame identity includes satellite, sector, band/recipe, scan time, footprint, and channel set. RGB inputs use the same scan or an explicitly rejected incomplete recipe; source-domain fallback is shown with its time offset.
- **Failure/compatibility:** missing scans remain visible timeline gaps. Changing boxes/satellites cancels obsolete requests; archive misses do not load today's imagery. Disk/cache limits and Android memory limits apply to loop size.
- **Acceptance:** a mesoscale loop includes all available one-minute frames; RGB channel times agree; moving sectors render in their recorded footprints; radar-driven behavior still works; desktop/Android can scrub and loop without UI stalls.
- **Proof:** pinned listing/granule fixtures, gap/moving-sector tests, and native loop performance captures.

**Agent prompt:** `Implement M5.2 using section 6. Add native ABI cadence to the shared timeline with same-scan RGB and bounded desktop/Android playback.`

#### M5.3 — Broaden field inspection within supported models

**Priority:** P1. **Depends on:** M1.4, M5.1. **Original references:** ROADMAP_NEW §§A1, F1, F3, F4, F5, F6; ROADMAP_2 §§9, 10.

- **Starting evidence:** HRRR/RAP/NAM/NAM nest/NBM descriptors, global-model fields, `.idx` range fetches, grid decoders, common legends, and comparisons.
- **Outcome/build:** add indexed field/vertical-level discovery for already supported model providers. Use metadata to expose compatible scalar, contour, and paired-vector products. Prioritize pressure-level temperature/moisture/heights/wind, surface wind/gusts, and interval precipitation; integrate units, point sampling, run/valid clocks, and existing compare/export.
- **Interfaces/compatibility:** stable field identities include physical quantity, level, statistic, and accumulation interval. Use vetted mappings for supported GRIB parameters, then show unmapped indexed entries as unsupported until their metadata is defined. Arbitrary numeric codes must not receive guessed units or palettes.
- **Failure/compatibility:** distinguish unavailable provider fields and differing grid/run/accumulation semantics. Vector components require matched context; interval QPF is not inferred by differencing incompatible accumulated products. Cancel discovery/fetch on context changes.
- **Acceptance:** supported pressure/surface fields have correct units/levels and native-grid provenance; comparisons require compatible quantities/intervals; run/lead switching rejects stale responses; unsupported entries are explained on desktop/Android.
- **Proof:** archived index/GRIB fixtures, descriptor/unit/comparison tests, and field-browser workflows.

**Agent prompt:** `Implement M5.3 using section 6. Extend supported-model field discovery through the existing registry, with explicit quantity/level/interval metadata and no guessed GRIB meanings.`

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

#### M6.2 — Persist complete, validated scenes

**Priority:** P1. **Depends on:** M4.4, M6.1. **Original references:** ROADMAP_2 §§6.3, 6.4, 12.2; ROADMAP_NEW §§M1, K3.

- **Starting evidence:** existing Scene stores camera, site, overlay names, dressing, strap, and output size; workspace/case persistence contains richer view state.
- **Outcome/build:** extend scenes to include product/tilt, palettes, required layers, GIS groups, annotations, legends, scientific/render quality choices, and live/fixed/group-follow time policy. Provide thumbnails, duplication, reorder, and existing scene shortcuts through one scene action path.
- **Interfaces/compatibility:** version scene definitions and resolve stable IDs. Legacy scenes load using the original keep-current behavior for fields they never stored; newly saved scenes capture their actual product/tilt explicitly.
- **Failure/compatibility:** preflight missing assets/products and incompatible quality/backend capabilities before Take. Repair/locate assets or preserve the previous program snapshot; do not silently substitute another scientific product.
- **Acceptance:** legacy/new round trips work; restored scenes match camera/products/palettes/GIS/time/dressing; missing assets are actionable; preview/program and shortcuts use identical application logic on desktop and Android.
- **Proof:** scene compatibility fixtures and complete-state restore captures.

**Agent prompt:** `Implement M6.2 using section 6. Extend scenes from partial graphics presets to complete validated analysis/output snapshots while preserving old scene semantics.`

#### M6.3 — Harden deterministic capture and archive output

**Priority:** P1. **Depends on:** M0.3, M1.2, M6.2. **Original references:** ROADMAP_2 §§6.2, 6.6, 8.4; ROADMAP_NEW §§M2, M3, M5.

- **Starting evidence:** deterministic capture clock, loop image/video paths, broadcast captions/crawl, warning archives, and scientific/analysis exports.
- **Outcome/build:** define capture readiness for required scene sources and freeze a render snapshot per logical weather frame. Advance on explicit analysis timestamps, record source-frame choices, and export metadata alongside imagery. Archive captions/warnings resolve from that historical context.
- **Interfaces/compatibility:** distinguish encoded video frames from logical weather frames: fixed-FPS holding of one source frame is intentional; accidental logical skips/duplicates are errors. A capture manifest records frame mapping, checksums, source stamps, resolution, and policy. Reuse the current encoder/platform capability boundary.
- **Failure/compatibility:** unavailable required data fails or pauses with an explicit bounded timeout (30 seconds by default), preserving completed output and a resumable manifest. Optional omitted layers are named. Never pair a historical frame with today's warning crawl.
- **Acceptance:** output frame count/mapping/timestamps are deterministic across repeat runs; pending/failed sources do not silently render wrong frames; thirty-minute output has no accumulating logical-clock drift; 1080p/1440p/4K presets preserve aspect and captions.
- **Proof:** capture/timeout/archive fixtures, encoded-frame inspection, and sustained output traces.

**Agent prompt:** `Implement M6.3 using section 6. Complete frame readiness and capture manifests around the existing deterministic exporter, including historical warning correctness.`

#### M6.4 — Deliver usable desktop and Android presentation

**Priority:** P1. **Depends on:** M6.2, M6.3. **Original references:** ROADMAP_2 §§6.5, 13; ROADMAP_NEW §§M1, Q1, Q3.

- **Starting evidence:** desktop output viewport, streaming dressing, Android export/share paths, and native platform gating.
- **Outcome/build:** document/test desktop window capture, monitor selection, startup restore, and stable program sizing for OBS. On Android, expose the same scene controls and a clean in-app presentation surface with touch-reachable exit and supported still/GIF sharing. Preserve scene/time/provenance semantics across both.
- **Interfaces/compatibility:** capability reporting describes available output surfaces/encoders. Android does not claim the separate desktop window or ffmpeg MP4 path; lack of an encoder does not disable its scene workflow.
- **Failure/compatibility:** share cancellation and failed encoding leave the scene intact. Surface recreation/background resume restore selected presentation state with refreshed source health and appropriate stale indicators.
- **Acceptance:** an operator can prepare, present, switch, exit, save, and share scenes on both native platforms; OBS capture survives restart; Android clean mode supports Back, rotation, and touch without trapped controls.
- **Proof:** OBS setup guide, desktop/Android walkthroughs, exported-file checks, and lifecycle regression evidence.

**Agent prompt:** `Implement M6.4 using section 6. Finish platform-appropriate presentation and sharing workflows with shared scene semantics and explicit encoder capabilities.`

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
