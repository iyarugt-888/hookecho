# docs/WSV3_IMGUI_MODERN_DESIGN_PLAN.md — migration dispositions

[Original bytes](../sources/WSV3_IMGUI_MODERN_DESIGN_PLAN.md) · [current plan](../../README.md)

Rows name original line numbers. Section rows include their subordinate prose/list
requirements. Evidence is historical; statuses do not claim fresh tests.

| Line | Item | Status | Destination / evidence |
| --- | --- | --- | --- |
| 1 | HookEcho — WSV3 × Dear ImGui Modern Workstation Design Plan | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 7 | 1. Core design direction | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 17 | 2. Primary layout | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 19 | 2.1 Map-first center workspace | superseded | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 32 | 2.2 Top command ribbon | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 66 | 2.3 Dockable tool windows | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 93 | 3. Layers window redesign | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 97 | 3.1 Structure | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 133 | 3.2 Radar product rows | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 151 | 4. Inspector / selected-layer window | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 187 | 5. Timeline redesign | superseded | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 191 | Main row | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 202 | Secondary row | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 214 | 6. Visual language | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 216 | 6.1 Base palette | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 231 | 6.2 Geometry | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 252 | 6.3 State clarity | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 262 | 7. Map tool rail | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 279 | 8. Basemap and data rendering priority | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 289 | 9. Responsive strategy | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 291 | Wide desktop | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 297 | Laptop / tablet landscape | superseded | [mobile](../../mobile/README.md); [evidence](../../../android/README.md) |
| 302 | Phone / narrow portrait | superseded | [mobile](../../mobile/README.md); [evidence](../../../android/README.md) |
| 313 | 10. Implementation mapping to the current codebase | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 342 | 11. Recommended implementation phases | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 344 | Phase 1 — visual primitives | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 354 | Phase 2 — top ribbon | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 360 | Phase 3 — layers dock | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 366 | Phase 4 — inspector dock | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 372 | Phase 5 — timeline | superseded | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 378 | Phase 6 — workspace docking | superseded | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 383 | Phase 7 — responsive behavior | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 388 | 12. Acceptance criteria | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 407 | 13. Implementation: the analyst workstation (`Layout::Wsv3` and `Layout::Dock`) | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 443 | 13.1 References | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 453 | 13.2 What changes, surface by surface | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 469 | 13.2.1 App bar (replaces the two-row `dock_menu`) | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 503 | 13.2.2 Context toolbar (new; absorbs the old right-panel "Quick Controls" and the tilt bar) | superseded | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 517 | 13.2.3 Layers panel (restyled `dock_left`) | superseded | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 540 | 13.2.4 Tool rail (restyled `dock_tools`) | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 548 | 13.2.5 Inspector card (replaces the right side panel) | superseded | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 569 | 13.2.6 Timeline (restyled `dock_timeline`, absorbs the tilt bar) | superseded | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 585 | 13.2.7 Tool windows: dock, float, fold (Phase 6) | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 603 | 13.2.8 Everything reachable from the workstation | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 635 | 13.3 Design tokens | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 667 | 13.4 Components (`ui/workstation.rs`) | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 689 | 13.5 Code structure | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 710 | 13.6 Rules for this work | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 723 | 13.7 Phases | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 734 | 13.8 Out of scope | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 740 | 13.9 Done | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |

Classification bases (by family):

- **HookEcho — WSV3 × Dear ImGui Modern Workstation Design Plan**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **1. Core design direction**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **2. Primary layout**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **2.1 Map-first center workspace**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **2.2 Top command ribbon**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **2.3 Dockable tool windows**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **3. Layers window redesign**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **3.1 Structure**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **3.2 Radar product rows**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **4. Inspector / selected-layer window**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **5. Timeline redesign**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **Main row**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **Secondary row**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **6. Visual language**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **6.1 Base palette**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **6.2 Geometry**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **6.3 State clarity**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **7. Map tool rail**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **8. Basemap and data rendering priority**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **9. Responsive strategy**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **Wide desktop**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **Laptop / tablet landscape**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **Phone / narrow portrait**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **10. Implementation mapping to the current codebase**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **11. Recommended implementation phases**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **Phase 1 — visual primitives**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **Phase 2 — top ribbon**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **Phase 3 — layers dock**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **Phase 4 — inspector dock**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **Phase 5 — timeline**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **Phase 6 — workspace docking**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **Phase 7 — responsive behavior**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **12. Acceptance criteria**: Preserved acceptance obligation; execute against the implemented slice and keep missing evidence open. Harness gaps are tracked separately in E1/E4.
- **13. Implementation: the analyst workstation (`Layout::Wsv3` and `Layout::Dock`)**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **13.1 References**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **13.2 What changes, surface by surface**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **13.2.1 App bar (replaces the two-row `dock_menu`)**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **13.2.2 Context toolbar (new; absorbs the old right-panel "Quick Controls" and the tilt bar)**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **13.2.3 Layers panel (restyled `dock_left`)**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **13.2.4 Tool rail (restyled `dock_tools`)**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **13.2.5 Inspector card (replaces the right side panel)**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **13.2.6 Timeline (restyled `dock_timeline`, absorbs the tilt bar)**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **13.2.7 Tool windows: dock, float, fold (Phase 6)**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **13.2.8 Everything reachable from the workstation**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **13.3 Design tokens**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **13.4 Components (`ui/workstation.rs`)**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **13.5 Code structure**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **13.6 Rules for this work**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **13.7 Phases**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **13.8 Out of scope**: Preserved scope exclusion; not a ready feature task. New priorities retain the no-breadth/no-new-platform boundary.
- **13.9 Done**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
