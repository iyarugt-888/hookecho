> **Planning authority: roadmaps/** — [master index](roadmaps/README.md) and [audit](roadmaps/audit.md) supersede this document's task ordering. This file retains historical requirements and evidence; completed/superseded items and remaining-work dispositions are preserved in [the archive](roadmaps/archive/README.md). Certification runs alongside implementation.

# Codex Goal: Close HookEcho's Highest-Value Analyst Gaps

> **Repository:** `iyarugt-888/hookecho`  
> **Target branch:** `feat/wsv3-redesign`  
> **Goal established:** 2026-10-11  
> **Initial inspected head:** `8bfe56a5000afb68b3d29f73ce71a85c3b633a83` — fetch current HEAD again before changing files.  
> **Comparison targets:** WSV3 Professional; WeatherWise; GR2Analyst 3 (sometimes called GRAnalyst3).  
> **Owner:** Codex for non-detector competitor parity.  
> **Outcome:** An operationally dependable Windows analyst workstation and Android field companion. Maintain existing Linux/browser capabilities; an untested platform is not certified.

## Core directive to Codex

**Do not recreate HookEcho or blindly imitate competitors. Find the highest-impact feature, quality, latency, interoperability, reliability, and usability gaps that *actually remain* in current code; complete them end-to-end, and prove that they work.** Prioritize storm-analysis outcomes over feature-count checklists. Work through small, reviewable, tested increments, commit only owned files, and update the existing parity and certification ledgers after each increment.

The product remains free/open-source, account-free, local-first, and usable without a mandatory hosted backend, telemetry service, or proprietary weather subscription. Scientific and operational claims must be backed by data and reproducible tests.

### Required first step — refresh the gap audit

1. Pull the current `feat/wsv3-redesign` HEAD. Read `ROADMAP_PARITY.md` **sections 2–6**, `ROADMAP_NEW.md`, `ROADMAP_2.md`, `ROADMAP.md`, `ARCHITECTURE.md`, `suggestions.md`, `3d_rework.md`, and `docs/certification/README.md` plus affected M-card evidence. Read relevant source and tests, not only checkboxes.
2. For each promising task, classify **absent / partial / implemented but not field-verified / verified** and specify evidence at a commit SHA and platform. Distinguish missing code from missing proof. Older documentation can describe work that has since shipped.
3. Prioritize by **severity and user value × operational frequency × current deficiency × feasibility**, with safety/integrity as the overriding gate. Pick one bounded vertical slice, not an architecture-wide rewrite. Prefer tasks that complete an operator workflow.
4. Re-check any remaining gap against ongoing peer work and recent commits. Do not erase peers' changes, claim someone else's changes as yours, or force push.

## Workstream A — P0: trust, latency, and continuity of live radar

**Competitor reference:** WSV3's progressive live scan; WeatherWise's low-latency radar analysis.  
**Existing roadmap:** M1.1–M1.4, M7.1.  
**Status at goal creation:** broad progressive-acquisition and temporal-coverage foundations exist; several real failure and presentation-level certification gaps remain. The 2026-10-11 branch includes SAILS visit stepping and a recorded 12-hour renderer-only soak; do not repeat those as if absent.

**Complete, measure, and validate:**

- End-to-end failover **across independent upstream failure domains** (stream interruption, dropped/reordered/duplicate radial, VCP/volume/cut transitions, partial provider outage, archive/live switch, radar switch, reconnect). Retain explicit gap, source and recovery diagnostics. Never silently display an incomplete/stale volume as the current full scan.
- Maintain canonical pass/revisit lineage for SAILS/MRLE and sector/radial timestamps through 2D, derived grids, 3D and inspection; explicitly label continuous mixed-time versus strict-current views. Never replace unknown source timestamps with client/render timestamps or fill missing gates with zeros.
- Measure source acquisition, first radar byte, decoder, build, GPU submission/completion, and **actual visible presentation** separately wherever technically measurable. Queue-write time is *not* visible scan-out latency. Publish median, p95 and p99 distributions with measurement provenance and test conditions; never claim competitor-equivalent latency on unsupported evidence.
- Guarantee that displayed volume/tilt, legend, clock, inspector, storm context, share state and 3D contributor labels identify the same **accepted revision**. Cancel or reject stale asynchronous jobs, including during fast timeline scrubs and rapid radar/product switches.
- Exercise realistic storms and injected failures; verify no unbounded queues, hot-loop retries, memory growth or long stalls. Use existing source-health, volume and temporal-coverage modules instead of building a redundant data pipeline.

**Completion proof:** demonstrable live/replayed failure-recovery sequence; timestamp/revision invariants covered by adversarial tests; repeatable end-to-end latency trace on actual available hardware; before/after behavior plus unresolved real-world limits documented.

## Workstream B — P1: complete a real storm interrogation workflow

**Competitor reference:** GR2Analyst 3's gate/volume/user-product depth; WSV3 storm-centered operations.  
**Existing roadmap:** M2.1–M2.4, M3.1–M3.6.

The operator should be able to:

**Select a storm → inspect its traceable history and uncertainty → choose the most physically suitable radar → move through actual scan/revisit times → inspect 2D gates and dual-polarization moments → open linked cross-sections and 3D ROI/rulers → create storm motion/impact estimates → save/reopen/export/share the case.**

Prioritize missing transitions rather than adding isolated tools:

- Persist storm identity and split/merge/ambiguous associations with explicit source evidence, annotations, previous scans, motion provenance, confidence limitations and time-group consistency.
- Expose practical beam-height/range and radar suitability information. Support switching sites without corrupting the selected storm or implying a lower/equivalent sampling height than measured.
- Finish native 2D-to-3D ROI/cross-section links and operator measurement controls; make sweep selection vs full-volume selection unambiguous. Maintain per-radial/per-elevation temporal information so 3D structures do not imply an instantaneous atmospheric sample.
- Close verified gaps in user-defined **gate and column** products: physical unit and vertical-datum checking, clear missing/masked values, bounded expression evaluation, per-pane independent definitions/state, scientifically valid environmental inputs, 2D/3D/section integration where useful, editable palettes, and portable versioned import/export. Preserve existing user formulas.
- Finish sliding-window **maximum/minimum-value trails** only where they lack real product coverage, especially relevant column and MRMS fields. The raw extremum is never changed merely to make older samples fade; the contributor's real acquisition time must be inspectable/exportable.
- Strengthen high-value inspection affordances for reflectivity, radial velocity, spectrum width, CC, ZDR, KDP and other supported fields, always labeling real versus estimated Nyquist, quality, temporal provenance and physical limits.

**Safety boundary:** spectrum width, rotation signals, and polarimetric debris evidence are not on their own proof of a tornado. Never present an unvalidated evidence score as a probabilistic forecast. Scientific reference cases and negative controls are mandatory.

**Completion proof:** an end-to-end archived storm walkthrough (for example Moore 2013 and Mayfield 2021) with saved/reopened analysis and deterministic scientific/interaction regression tests.

## Workstream C — P1: operational GIS, mobile field analysis and UI ergonomics

**Competitor reference:** GIS overlays, impact workflows and efficient multi-pane analyst operation.  
**Existing roadmap:** M2.4, M4.1–M4.4, M5.1, M7.2.

- Complete multi-layer **GIS import → inspect/filter/style → save in workspace/case/scene → restore → export** for valid GeoJSON, KML/KMZ, shapefile collections (including sidecars and projected coordinate systems), and relevant common GIS use cases. Protect against oversized/malformed files. Use the current collection model instead of adding a parallel one.
- Make functional parity usable in a **real Android operator session**, not just available in shared Rust code. Test pan/pinch/tilt/3D orbit/scrub, thumb/pen hit targets, independent tool versus map gesture ownership, modal/sheet scroll, app pause/resume, rotation, surface/GPU recreation, offline/reconnect and bounded memory.
- Eliminate workflow friction that blocks analysis: too many taps to probe gates, unreadable legends, source/valid-time ambiguity, pane focus errors, clipping on small screens and accidental map movement when editing a storm vector.
- Preserve accessibility, touch and desktop keyboard controls. Keep optional floating elements truly optional; don't replace the main map with permanent clutter.

**Completion proof:** imported real GIS examples verified against independent coordinates/attributes, saved/restored operator scenes, a captured desktop run and an actual Android device walkthrough when possible. If no Android device/SDK is available, record `device verification: blocked/unverified`, not `passed`.

## Workstream D — P1: meteorological context and broadcast-quality presentation

**Competitor reference:** WSV3-style model/satellite context and production-ready output; WeatherWise's efficient analysis presentation.  
**Existing roadmap:** M5.1–M5.4, M6.1–M6.4.

- Finish genuinely independent analysis-time/pane groups across site, field, model run, valid time, GOES imagery, comparison data, fetches and probes. Shared immutable content is fine; context-sensitive cache keys and delivery must not contaminate separate panes.
- Finish proven missing features in GOES mesoscale/native-cadence playback and vetted model field/ensemble comparisons, displaying native data valid times, units, cadence and unavailability. Never interpolate or retime products without telling the operator.
- Improve visual quality and interaction performance when data are streaming, zooming, rendering 3D, cross-fading satellite layers and redrawing large GIS collections. Compare screenshots and profiled hardware, not only compiler success.
- Finish preview/program isolation, durable versioned output scenes (including 3D/product/GIS/time choices), reliable OBS-friendly output, transparent labels and deterministic export. Prove sustained 30/60 fps delivery only with captured frame pacing and actual hardware; nominal fps configuration is not performance evidence.

**Completion proof:** two independent time/source panes retain their identity under rapid updates, scene restore matches saved content, and export/capture round-trips with documented timing and measured resource use.

## Workstream E — P0 release gate: real-world certification

**Existing roadmap:** M7.1–M7.3; `docs/certification/`.

- Reuse committed checked/pinned NOAA/NWS fixtures and archived **Moore, Mayfield, QLCS, hail and quiet/clutter** scenarios. Preserve checksum and source provenance; don't rewrite goldens to hide regressions.
- Build upon **already recorded** 12-hour live KTLX GPU render-path evidence. Complete missing 24-hour/severe-cadence, provider-failover, whole-app GUI, pane, 3D, GIS, capture, surface-loss and mobile lifecycle profiles as feasible. Don't confuse a renderer-only soak with an interactive desktop or mobile soak.
- For each evidence claim record commit SHA, OS/device/GPU, source/time/quality, run commands, exit status, p50/p95/p99 timings where relevant, before/after metrics, repro steps, artifacts and limitations. Explicitly distinguish **implemented / bench-tested / desktop-tested / Android-tested / release-certified**.
- Update `ROADMAP_PARITY.md` task-card ledger and `docs/certification/` artifacts/index without gratuitously rewriting old historical logs. Do not claim blanket feature parity or a production-safe release until all mandatory gates are demonstrated.

## Required engineering and collaboration rules

- **Agent ownership:** the parity roadmap's 2026-10-02 handoff assigns **Codex** to WSV3/analyst capability improvements and **Claude** to tornado/rotation/debris detection algorithms, input lineage, scoring, calibration, thresholds and backtest research. **Do not modify detector internals or commit Claude's uncommitted/concurrent edits.** Shared interfaces should be coordinated and backward-compatible.
- Preserve `wxdata` as the decoding/meteorological-data boundary and `hookecho` as application UI/GPU. Extend current sources, rendering, tests and request orchestration; use explicit context/revision IDs, cancellation and bounded resources.
- Preserve working Windows/Linux targets, native Android, web/WASM, saved workspaces/cases/JSON, replay, palettes, plugins and CPU/software-GPU fallback. New optional dependencies must not make the core experience require a personal account or proprietary API key.
- One reviewable vertical slice per iteration: **reproduce → implement → add regression and scientific/reference tests → run applicable checks → record evidence → commit only owned files → update roadmap/certification → identify next ready slice**.
- Typical applicable gates: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, the repo's documented WASM check, focused renderer/scientific/GIS/headless tests, and actual platform build/run when toolchains are present. State exactly which passed and which could not run.
- Treat existing roadmap acceptance criteria as authoritative where more precise than this goal. Do not silently loosen thresholds, make up historical observations, or erase `partial/unverified` statuses to close tasks.

## Done criteria

- [ ] Current evidence-backed competitor gap/priority matrix reviewed and updated for latest HEAD, with no repeated work.
- [ ] At least one **meaningful P0 live data failure-recovery or presentation-latency gap** closed end-to-end with measurable tests.
- [ ] At least one incomplete **storm-analyst workflow** fully usable from selection through evidence-backed saved/exported result.
- [ ] Relevant GIS, independent pane, model/satellite or broadcast gaps closed in priority order, not merely mocked up.
- [ ] Desktop regressions and available scientific/render gates pass; Android is truly on-device verified or explicitly remains blocked, never fictitiously certified.
- [ ] No regressions in saved data, cross-pane source selection, source freshness disclosures, scientific conventions, detector behavior, or memory/frame pacing.
- [ ] `ROADMAP_PARITY.md` and certification evidence contain clear completed/partial/unverified status, measurable results and a ranked next-action queue.

## Immediate assignment

**Start with the highest-value genuinely unresolved part of M1.2/M1.3: presentation-stage latency proof and/or independent-provider failure recovery.** Check current HEAD and ownership before committing. If these are already complete, choose the next highest-impact incomplete M2/M3 end-to-end analyst workflow. Deliver a small real improvement, not just another speculative feature list.

At every handoff provide: **baseline and final SHA; exact changed paths; what improved for an operator; runnable reproduction; tests actually run; numerical measures; remaining limitations; next ready task.**

### Context links

- [WSV3](https://wsv3.com/)
- [GR2Analyst 3](https://www.grlevelx.com/gr2analyst_3/) and [user-defined products](https://www.grlevelx.com/gr2analyst_3/udp.htm)
- [Existing parity roadmap](ROADMAP_PARITY.md)
- [Certification evidence](docs/certification/README.md)
- [System architecture](ARCHITECTURE.md)
