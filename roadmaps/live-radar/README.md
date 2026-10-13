# Live radar — P0

Parent cards M1.1–M1.4; ROADMAP_NEW B1–B6, N1–N4; ROADMAP_2 §§1,9;
CODEX_GOAL A. [Evidence](../../docs/certification/m1.3/).

| ID | Status | Remaining slice and acceptance |
| --- | --- | --- |
| L1 | Partially implemented | TGFTP polls now reuse unchanged objects and scope the cache to the requested site. Live testing also exposed a publishing (incomplete) newest file: require a source end-of-scan marker and try at most three candidates within one deadline. Final regression/platform checks in progress; operational failover still L2. |
| L2 | Partially implemented | Operational independent-provider recovery. Existing arbiter, monitor, relay and TGFTP adapters stay. Audit actual upstream topology, cut the primary mid-volume, observe alternate-source delivery and preferred recovery in the running app, under severe cadence. Include duplicate/reordered/missing radial and radar/archive switches. |
| L3 | Partially implemented | Export one bounded revision-correlated trace: source acquisition, first byte, decode, acceptance, queue submission/completion, observable presentation. Existing timings remain labelled by their actual stage. Unsupported presentation stays null with reason. |
| L4 | Certification-only | At least 1,000 accepted-update presentation observations; p50/p95/p99, observer/compositor/display conditions and clock uncertainty. External observation is required where APIs cannot observe visible output. Budget: desktop p50 <100 ms/p95 <250 ms; Android <200/<500 ms after receipt. |
| L5 | Partially implemented | Carry source/pass/accepted revision consistently through remaining derived, 3D, probe/export and retained live/relay disk receipt paths. Extend existing lineage; do not invent source clocks or zero-fill gaps. |
| L6 | Partially implemented | Live animation/shared-sweep frame-time proof. Shared immutable sweeps and receiver ownership controls already exist as peer edits; validate integrated costs without claiming this audit implemented them. |

L1 is ready without credentials or new infrastructure. L2 requires genuine independent
live inputs; replay relays and different labels cannot close it. L3 instrumentation can
advance while L2 deployment and L4 observation are pending. Run Track B/C/D/E beside
each increment. Full M1 remains open until operational and platform evidence exists.
