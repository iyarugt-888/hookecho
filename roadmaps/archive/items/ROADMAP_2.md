# ROADMAP_2.md — migration dispositions

[Original bytes](../sources/ROADMAP_2.md) · [current plan](../../README.md)

Rows name original line numbers. Section rows include their subordinate prose/list
requirements. Evidence is historical; statuses do not claim fresh tests.

| Line | Item | Status | Destination / evidence |
| --- | --- | --- | --- |
| 1 | ROADMAP_2.md — Professional Maturity / WSV3-Competitive Roadmap | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 3 | Purpose | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 19 | Implementation progress (2026-09-29) | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 241 | 0. Program goals and release gates | certification-only | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 243 | 0.1 Product goals | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 279 | 0.2 Release gates | certification-only | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 283 | [ ] 12-hour live severe-weather soak test passes with no unrecovered feed, renderer, memory, or UI failure. | certification-only | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 284 | [ ] 24-hour idle/live mixed soak test passes on Windows. | certification-only | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 285 | [x] no stale radar scan can be displayed as current without a visible stale-data indication. | certification-only | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 286 | [ ] all displayed forecast/derived/observed layers expose provenance through a common inspector. | certification-only | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 287 | [x] live Level II partial-volume behavior is covered by deterministic tests. | certification-only | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 288 | [x] VCP transitions, SAILS/MESO-SAILS, missing chunk, late chunk, duplicate chunk, and out-of-order chunk cases are tested. | certification-only | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 289 | [x] all critical storm-analysis actions are reachable by command palette and direct pointer interaction. | certification-only | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 290 | [x] archive replay is deterministic across repeated runs for the same volume/time. | certification-only | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 291 | [x] export/capture output is frame-stable and does not depend on UI repaint timing. | certification-only | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 292 | [ ] no single application source module remains responsible for an unreasonable number of unrelated domains. | certification-only | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 293 | [x] nightly/corpus regression suite runs against known historic storm cases. | certification-only | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 294 | [ ] performance budgets are defined and measured for desktop, browser, and Android. | certification-only | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 298 | 1. P0 — LiveScan-class Level II acquisition and presentation | partially implemented | [live-radar](../../live-radar/README.md); [evidence](../../../docs/certification/m1.3/continuation.md) |
| 304 | 1.1 Introduce an explicit live-scan state model | partially implemented | [live-radar](../../live-radar/README.md); [evidence](../../../docs/certification/m1.3/continuation.md) |
| 336 | Acceptance criteria | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 338 | [x] UI never needs to infer live state from a generic loading boolean. | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 339 | [x] current sweep and completed sweep are distinguishable in the state model. | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 340 | [x] SAILS/MESO-SAILS supplemental scans are represented explicitly. | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 341 | [x] out-of-order chunks cannot regress displayed time. | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 342 | [x] stale live data is visibly marked before it reaches a configurable threshold. | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 344 | 1.2 Partial-volume rendering rules | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 358 | Tests | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 372 | 1.3 Multi-source Level II failover | partially implemented | [live-radar](../../live-radar/README.md); [evidence](../../../docs/certification/m1.3/continuation.md) |
| 391 | Acceptance criteria | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 393 | [x] provider loss does not freeze the display indefinitely. | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 394 | [x] fallback never silently changes from partial-scan live data to minutes-old completed data. | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 395 | [x] recovery to the preferred source does not cause time reversal. | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 396 | [x] all provider switches are logged in Analyst Mode. | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 398 | 1.4 VCP-aware scan progression | partially implemented | [live-radar](../../live-radar/README.md); [evidence](../../../docs/certification/m1.3/continuation.md) |
| 417 | 1.5 Performance budget | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 430 | 2. P0 — Storm tracking and manual analysis UX | partially implemented | [storm-analysis](../../storm-analysis/README.md); [evidence](../../../docs/certification/m2.2/README.md) |
| 434 | 2.1 Unified storm object model | partially implemented | [storm-analysis](../../storm-analysis/README.md); [evidence](../../../docs/certification/m2.2/README.md) |
| 452 | 2.2 Manual motion tool | partially implemented | [storm-analysis](../../storm-analysis/README.md); [evidence](../../../docs/certification/m2.2/README.md) |
| 473 | 2.3 Line / squall-line tracking | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 485 | 2.4 Impact / ETA engine | partially implemented | [storm-analysis](../../storm-analysis/README.md); [evidence](../../../docs/certification/m2.2/README.md) |
| 505 | 2.5 Storm selection UX | partially implemented | [storm-analysis](../../storm-analysis/README.md); [evidence](../../../docs/certification/m2.2/README.md) |
| 522 | 2.6 Acceptance criteria | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 524 | [x] a trained user can create a storm motion vector and 60-minute projection in under 5 seconds. | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 525 | [x] manual and automatic motion can coexist and be clearly distinguished. | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 526 | [x] editing never requires opening Settings. | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 527 | [ ] storm interactions work with mouse, pen, and touch. | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 528 | [x] projected ETAs are recomputed live while handles are dragged. | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 532 | 3. P0 — Long-duration resilience and source-failure testing | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 536 | 3.1 Soak-test harness | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 555 | 3.2 Failure injection | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 573 | 3.3 Memory/resource leak detection | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 588 | 3.4 Recovery rules | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 602 | 4. P1 — Rendering and visual polish | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 606 | 4.1 Unified render-quality policy | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 628 | 4.2 Temporal transitions | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 647 | 4.3 Label engine improvements | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 669 | 4.4 Camera behavior | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 683 | 4.5 Basemap robustness | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 697 | 5. P1 — General GIS engine | partially implemented | [gis](../../gis/README.md); [evidence](../../../docs/certification/m4.4/README.md) |
| 701 | 5.1 Supported formats | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 715 | 5.2 Vector style model | partially implemented | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 736 | 5.3 Label editor | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 748 | 5.4 Attribute inspector | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 759 | 5.5 Layer groups and saved GIS scenes | partially implemented | [gis](../../gis/README.md); [evidence](../../../docs/certification/m4.4/README.md) |
| 775 | 6. P1 — Broadcast / external output system | partially implemented | [broadcast](../../broadcast/README.md); [evidence](../../../docs/certification/m6.3/README.md) |
| 779 | 6.1 Dedicated output window | partially implemented | [broadcast](../../broadcast/README.md); [evidence](../../../docs/certification/m6.3/README.md) |
| 792 | 6.2 Deterministic frame clock | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 803 | 6.3 Scene presets | partially implemented | [broadcast](../../broadcast/README.md); [evidence](../../../docs/certification/m6.3/README.md) |
| 818 | 6.4 Graphics overlays | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 830 | 6.5 OBS integration | partially implemented | [broadcast](../../broadcast/README.md); [evidence](../../../docs/certification/m6.3/README.md) |
| 839 | 6.6 Export | partially implemented | [broadcast](../../broadcast/README.md); [evidence](../../../docs/certification/m6.3/README.md) |
| 850 | [x] no dropped or duplicated logical weather frames in fixed-FPS export. | partially implemented | [broadcast](../../broadcast/README.md); [evidence](../../../docs/certification/m6.3/README.md) |
| 851 | [x] exported timestamp matches the rendered analysis time. | partially implemented | [broadcast](../../broadcast/README.md); [evidence](../../../docs/certification/m6.3/README.md) |
| 852 | [ ] output can run for 30 minutes without drift. | partially implemented | [broadcast](../../broadcast/README.md); [evidence](../../../docs/certification/m6.3/README.md) |
| 856 | 7. P1 — app.rs decomposition and application architecture | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 862 | 7.1 Define domain boundaries | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 883 | 7.2 App shell responsibility | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 895 | 7.3 Event/message model | partially implemented | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 911 | 7.4 Refactor guardrails | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 921 | 7.5 Metrics | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 935 | 8. P1 — Product-level historical regression corpus | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 939 | 8.1 Build a canonical event corpus | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 957 | 8.2 Golden scientific checks | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 975 | 8.3 Golden visual checks | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 991 | 8.4 Historical warning verification checks | partially implemented | [storm-analysis](../../storm-analysis/README.md); [evidence](../../../docs/certification/m2.2/README.md) |
| 1002 | 9. P1 — Data/provenance completeness | partially implemented | [live-radar](../../live-radar/README.md); [evidence](../../../docs/certification/m1.3/continuation.md) |
| 1006 | 9.1 Every field gets a DataStamp-equivalent | partially implemented | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 1020 | 9.2 Source health panel | partially implemented | [live-radar](../../live-radar/README.md); [evidence](../../../docs/certification/m1.3/continuation.md) |
| 1043 | 9.3 Stale-data policy | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1056 | 10. P1 — Timeline and synchronization finish | partially implemented | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 1060 | 10.1 One analysis cursor | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1064 | 10.2 Explicit source offsets | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1075 | 10.3 Archive/model synchronization | partially implemented | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 1081 | 10.4 Timeline themes | partially implemented | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 1094 | 11. P2 — 3D analysis expansion | partially implemented | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 1098 | 11.1 Storm-centered 3D mode | partially implemented | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 1108 | 11.2 Isosurfaces | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1119 | 11.3 Vertical reference surfaces | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1131 | 11.4 3D crosshair / ruler | partially implemented | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 1141 | 11.5 3D temporal playback | partially implemented | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 1151 | 12. P2 — Workspace and analyst workflow improvements | partially implemented | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 1153 | 12.1 Professional workspace presets | partially implemented | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 1157 | Tornado analysis | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1162 | Hail analysis | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1166 | National severe | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1172 | Chase | partially implemented | [storm-analysis](../../storm-analysis/README.md); [evidence](../../../docs/certification/m2.2/README.md) |
| 1178 | Tropical | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1184 | 12.2 Workspace validation | partially implemented | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 1195 | 13. P2 — Input, accessibility, and operator efficiency | partially implemented | [mobile](../../mobile/README.md); [evidence](../../../android/README.md) |
| 1197 | 13.1 Command consistency | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1206 | 13.2 Pointer consistency | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1219 | 13.3 Touchscreen stability | partially implemented | [mobile](../../mobile/README.md); [evidence](../../../android/README.md) |
| 1231 | 13.4 Accessibility | partially implemented | [mobile](../../mobile/README.md); [evidence](../../../android/README.md) |
| 1244 | 14. P2 — Performance and observability | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1246 | 14.1 Internal telemetry without user tracking | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1263 | 14.2 Performance regression tests | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1280 | 15. P2 — Documentation and operational transparency | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1282 | 15.1 "How current is this?" documentation | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1291 | 15.2 Scientific limitations | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1301 | 15.3 Analyst troubleshooting guide | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1316 | 16. Execution sequence | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1320 | Phase 1 — Operational radar trust | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1328 | Phase 2 — Severe-weather operator workflow | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1336 | Phase 3 — Reliability | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1343 | Phase 4 — Architecture | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1350 | Phase 5 — Visual maturity | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1357 | Phase 6 — Professional workstation expansion | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1364 | Phase 7 — Regression corpus | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1371 | Phase 8 — 3D / analyst refinements | partially implemented | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 1380 | 17. Definition of "WSV3-competitive" | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 1399 | 18. Work that should *not* be prioritized yet | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |

Classification bases (by family):

- **ROADMAP_2.md — Professional Maturity / WSV3-Competitive Roadmap**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Purpose**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Implementation progress (2026-09-29)**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **0. Program goals and release gates**: Preserved acceptance obligation; execute against the implemented slice and keep missing evidence open. Harness gaps are tracked separately in E1/E4.
- **0.1 Product goals**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **0.2 Release gates**: Preserved acceptance obligation; execute against the implemented slice and keep missing evidence open. Harness gaps are tracked separately in E1/E4.
- **1. P0 — LiveScan-class Level II acquisition and presentation**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **1.1 Introduce an explicit live-scan state model**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Acceptance criteria**: Preserved acceptance obligation; execute against the implemented slice and keep missing evidence open. Harness gaps are tracked separately in E1/E4.
- **1.2 Partial-volume rendering rules**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Tests**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **1.3 Multi-source Level II failover**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **1.4 VCP-aware scan progression**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **1.5 Performance budget**: Preserved acceptance obligation; execute against the implemented slice and keep missing evidence open. Harness gaps are tracked separately in E1/E4.
- **2. P0 — Storm tracking and manual analysis UX**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **2.1 Unified storm object model**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **2.2 Manual motion tool**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **2.3 Line / squall-line tracking**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **2.4 Impact / ETA engine**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **2.5 Storm selection UX**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **2.6 Acceptance criteria**: Preserved acceptance obligation; execute against the implemented slice and keep missing evidence open. Harness gaps are tracked separately in E1/E4.
- **3. P0 — Long-duration resilience and source-failure testing**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **3.1 Soak-test harness**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **3.2 Failure injection**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **3.3 Memory/resource leak detection**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **3.4 Recovery rules**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **4. P1 — Rendering and visual polish**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **4.1 Unified render-quality policy**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **4.2 Temporal transitions**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **4.3 Label engine improvements**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **4.4 Camera behavior**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **4.5 Basemap robustness**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **5. P1 — General GIS engine**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **5.1 Supported formats**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **5.2 Vector style model**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **5.3 Label editor**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **5.4 Attribute inspector**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **5.5 Layer groups and saved GIS scenes**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **6. P1 — Broadcast / external output system**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **6.1 Dedicated output window**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **6.2 Deterministic frame clock**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **6.3 Scene presets**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **6.4 Graphics overlays**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **6.5 OBS integration**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **6.6 Export**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **7. P1 — app.rs decomposition and application architecture**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **7.1 Define domain boundaries**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **7.2 App shell responsibility**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **7.3 Event/message model**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **7.4 Refactor guardrails**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **7.5 Metrics**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **8. P1 — Product-level historical regression corpus**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **8.1 Build a canonical event corpus**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **8.2 Golden scientific checks**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **8.3 Golden visual checks**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **8.4 Historical warning verification checks**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **9. P1 — Data/provenance completeness**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **9.1 Every field gets a DataStamp-equivalent**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **9.2 Source health panel**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **9.3 Stale-data policy**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **10. P1 — Timeline and synchronization finish**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **10.1 One analysis cursor**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **10.2 Explicit source offsets**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **10.3 Archive/model synchronization**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **10.4 Timeline themes**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **11. P2 — 3D analysis expansion**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **11.1 Storm-centered 3D mode**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **11.2 Isosurfaces**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **11.3 Vertical reference surfaces**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **11.4 3D crosshair / ruler**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **11.5 3D temporal playback**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **12. P2 — Workspace and analyst workflow improvements**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **12.1 Professional workspace presets**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Tornado analysis**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Hail analysis**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **National severe**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Chase**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Tropical**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **12.2 Workspace validation**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **13. P2 — Input, accessibility, and operator efficiency**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **13.1 Command consistency**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **13.2 Pointer consistency**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **13.3 Touchscreen stability**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **13.4 Accessibility**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **14. P2 — Performance and observability**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **14.1 Internal telemetry without user tracking**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **14.2 Performance regression tests**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **15. P2 — Documentation and operational transparency**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **15.1 "How current is this?" documentation**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **15.2 Scientific limitations**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **15.3 Analyst troubleshooting guide**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **16. Execution sequence**: Old execution ordering replaced by roadmaps/README.md priorities and simultaneous Tracks A–E.
- **Phase 1 — Operational radar trust**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Phase 2 — Severe-weather operator workflow**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Phase 3 — Reliability**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Phase 4 — Architecture**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Phase 5 — Visual maturity**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Phase 6 — Professional workstation expansion**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Phase 7 — Regression corpus**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Phase 8 — 3D / analyst refinements**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **17. Definition of "WSV3-competitive"**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **18. Work that should *not* be prioritized yet**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
