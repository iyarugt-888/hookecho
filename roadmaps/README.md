# HookEcho delivery roadmap

This is the authoritative planning index for `iyarugt-888/hookecho`, branch
`feat/wsv3-redesign`. Audit baseline: `ed5eccaa`, 2026-10-12, **plus the existing
dirty working tree**. See [audit](audit.md), [item inventory](archive/inventory.md)
and [preserved sources](archive/README.md). Historical checkboxes are not release
certification. No blanket competitor parity is claimed.

## Priorities and ready work

| Order | Workstream | Current status | Next reviewable action | Dependencies |
| --- | --- | --- | --- | --- |
| 1 | [Live radar](live-radar/README.md) | Partially implemented; bounded L1/L3a–b/L2a–d archived | L2 operational independent outage/recovery; L3 source-pass/first-byte/observer correlation and durable capture | Existing provider/health/receiver contracts; independent live inputs and presentation observer for certification |
| 2 | [Radar analysis](radar-analysis/README.md) | Partially implemented | Validate landed R1 import/inspection advice and R2 pane-owned column evaluation; inspect R3 vertical interrogation next | Preserve formulas, missing values and recorded environment; corpus |
| 3 | [Storm analysis](storm-analysis/README.md) | Partially implemented | Validate landed S1 archived local-cell replay; audit S2 end-to-end workflow transitions | Time/revision integrity; pinned storm and negative cases |
| 4 | [Broadcast](broadcast/README.md), [GIS](gis/README.md) | Partially implemented | B1: sustained independent program/capture proof; G1: external reader round trip | Existing scenes, manifests and collection model |
| 5 | [Mobile](mobile/README.md) | Partially implemented | M1: physical phone/tablet lifecycle and input runs | Device access; stable increment and Android build |
| Supporting | [Models/satellite](models-satellite/README.md) | Partially implemented | C1: remaining context/health/mesoscale acceptance; do not rebuild independent GOES or HREF probability work | Existing request-keyed pane state; provider availability |
| Continuous | [Certification](certification/README.md) | Partially implemented | E1: extend full-app harness while retaining renderer-only long-run evidence | Starts now; never waits for feature backlog completion |

## Parallel tracks

These are concurrent work streams, not a requirement to spawn agents or permit
multiple writers in the same files. Validation uses a pinned binary/source hash;
implementation can continue without silently changing a running test's build.

| Track | Work | Runs beside | Completion evidence |
| --- | --- | --- | --- |
| A — Implementation | One highest-value unresolved operator workflow per slice | B–E | Reproduction, fix, regression/scientific controls, applicable checks |
| B — Long-running validation | 2h smoke/recovery, 12h severe-cadence, 24h mixed stability/resource profiles on latest stable increments | A, C–E | Full elapsed run, exit status, faults/recovery, RSS/GPU/tasks/queues, profile scope |
| C — Platform validation | Windows, Android phone/tablet, browser/WASM, physical GPU and software renderer | A, B, D, E | Exact device/OS/adapter, build and observed workflow; unsupported/absent stays open |
| D — Scientific regression | Archived tornado/hail/QLCS/clutter; UDP, extrema, dealiasing, multi-radar and provenance | Every affected A slice | Pinned inputs, independent expected values, justified tolerances, negative controls |
| E — Release evidence | Maintain `docs/certification/`, hashes, limitations and this index as work lands | Every track | Commit/build attribution, commands, measurements, reproducible artifacts |

## Status and completion rules

- **Completed**: the bounded item has recorded proof for its stated scope. It does
  not certify its parent workflow or an untested platform.
- **Partially implemented**: useful implementation exists, but required behavior
  or integrated acceptance remains. Source presence alone never closes a gate.
- **Still needed**: a concrete requirement remains; deferred breadth is explicitly
  parked, not scheduled ahead of reliability.
- **Superseded**: replaced design/sequence or duplicate request; its replacement
  and history remain linked in the inventory.
- **Certification-only**: the remaining task is execution/evidence for implemented
  behavior. If it exposes a defect, open a bounded implementation task immediately.

Before every slice re-read its code and current diff, preserve peer work, reproduce
the failure, add meaningful regression/scientific tests, run applicable workspace,
Clippy, WASM, GPU and platform checks, and update its workstream/evidence. Move only
the bounded completed item to [archive](archive/README.md); leave unresolved
operational gates visible. Do not rebuild an old checkbox without checking the
current implementation and newest dated evidence. Do not change detector internals,
calibration or thresholds in this lane; their existing ownership remains intact.

Older root documents remain compatibility references with their history intact.
Their old ordering (especially “certification last”) is superseded by this index.
Keep broad new models/layers, decorative rendering, new platforms and hosted
services out of the ready queue unless they close a demonstrated gap.
