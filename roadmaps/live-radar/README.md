# Live radar — P0

Parent cards M1.1–M1.4; ROADMAP_NEW B1–B6, N1–N4; ROADMAP_2 §§1,9;
CODEX_GOAL A. [Evidence](../../docs/certification/m1.3/).

| ID | Status | Remaining slice and acceptance |
| --- | --- | --- |
| L1 | Completed (bounded adapter slice) | Site-scoped cache, unchanged polling, regressed-directory protection and end-of-scan admission with at most three candidates in one deadline. Workspace/Clippy/WASM, GPU control and live NOAA fetch passed; [record](../../docs/certification/live-fallback/README.md), [archived scope](../archive/README.md). Operational/device/soak acceptance remains L2/L4/E2/E3. |
| L2 | Partially implemented | Operational independent-provider recovery. Existing arbiter, monitor, relay and TGFTP adapters stay. Audit actual upstream topology, cut the primary mid-volume, observe alternate-source delivery and preferred recovery in the running app, under severe cadence. Include duplicate/reordered/missing radial and radar/archive switches. |
| L3 | Partially implemented | Diagnostics now exports a bounded accepted-revision trace with raw acquisition-clock bounds, acceptance/decode/queue/GPU stages and first 2D upload context; first-byte and visible-presentation observations stay null with reasons. [Increment evidence](../../docs/certification/live-trace/README.md). Remaining: full pass/sequence receipts, first-byte/decode boundaries, durable observer sampling, linked/3D uploads and real presentation correlation. |
| L4 | Certification-only | At least 1,000 accepted-update presentation observations; p50/p95/p99, observer/compositor/display conditions and clock uncertainty. External observation is required where APIs cannot observe visible output. Budget: desktop p50 <100 ms/p95 <250 ms; Android <200/<500 ms after receipt. |
| L5 | Partially implemented | Carry source/pass/accepted revision consistently through remaining derived, 3D, probe/export and retained live/relay disk receipt paths. Extend existing lineage; do not invent source clocks or zero-fill gaps. |
| L6 | Partially implemented | Live animation/shared-sweep frame-time proof. Shared immutable sweeps and receiver ownership controls already exist as peer edits; validate integrated costs without claiming this audit implemented them. |

Provider health now uses arriving native radial clocks rather than the fixed volume
start for progressive freshness. The [source-clock increment](../../docs/certification/live-source-clock/README.md)
reproduces false staleness on a pinned scan, preserves unknown/duplicate clocks and
verifies recovery from three advances in one volume. L2 remains partial.
The [displayed-subscription increment](../../docs/certification/display-feedback/README.md) now
records owned losses, rejects invalid contexts, compares a fresh backup to accepted
source clocks and preserves failback hysteresis. L2 still requires independent
operational inputs and sustained recovery. A [progressive idle watchdog](../../docs/certification/delivery-idle/README.md)
now retires consumers without source advances or an End event after a bounded window;
completed cadence is excluded. [Monitor retirement](../../docs/certification/monitor-retirement/README.md)
now drops pending subscriptions even when a provider ignores cancellation and releases
inactive-pane managers, with process task counters. Next: socket/resource measurements
and full-app long-running recovery profiles alongside operational source validation.

The [handoff control](../../docs/certification/handoff-2026-10-14/README.md) now exercises
the actual application channel, upload cache and GPU through controlled loss, a late
old generation, archive/loop selection and resume. It protects visible archive gates
and the sibling pane. Operational source selection/independence remains open.

L1's bounded adapter work is archived. L2 requires genuine independent
live inputs; replay relays and different labels cannot close it. L3 instrumentation can
advance while L2 deployment and L4 observation are pending. Run Track B/C/D/E beside
each increment. Full M1 remains open until operational and platform evidence exists.
