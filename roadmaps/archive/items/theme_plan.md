# theme_plan.md — migration dispositions

[Original bytes](../sources/theme_plan.md) · [current plan](../../README.md)

Rows name original line numbers. Section rows include their subordinate prose/list
requirements. Evidence is historical; statuses do not claim fresh tests.

| Line | Item | Status | Destination / evidence |
| --- | --- | --- | --- |
| 1 | HookEcho — Theme, Chrome & UX Overhaul Plan | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 27 | 0. What already exists — read this before touching anything | superseded | [mobile](../../mobile/README.md); [evidence](../../../android/README.md) |
| 69 | 1. Rules specific to this work (in addition to `ROADMAP_NEW.md` §2) | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 103 | 2. Bugs | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 109 | 2.1 Multi-touch pan sends the map flying (web, touchscreen) — [x] mitigated, root cause on egui's | superseded | [mobile](../../mobile/README.md); [evidence](../../../android/README.md) |
| 164 | 2.2 3D map basemap has empty gaps, mainly when zoomed out — [x] done | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 213 | 2.3 The search pill doesn't look good and should be an optional floating button — [x] done | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 253 | 2.4 Radar range ring shows km instead of miles — [x] done | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 289 | 3. Settings reorganization — [x] done | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 320 | 4. Analyst Mode (verbose live logging) — [x] done | superseded | [live-radar](../../live-radar/README.md); [evidence](../../../docs/certification/m1.3/continuation.md) |
| 375 | 5. Distance-unit and other small consistency items | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 382 | 6. Theme system rework | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 384 | 6.1 Naming — commit to this, don't re-litigate it mid-implementation | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 401 | 6.2 Turn `Layout` into a real preset (color + UI), while keeping colors independently | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 434 | 6.3 Build the new "WSV3" theme from Ref 4 — [x] partly done — geometry + footer shipped, the | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 509 | 7. Timeline (scrubber) styles | superseded | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 521 | [x] `TimelineStyle` enum in `settings.rs`, same shape as `VelocityUnit`/`Layout`: `Default` | superseded | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 534 | [x] `scrubber()` is now a 3-line dispatcher on `self.settings.timeline_style`; the three paint | superseded | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 540 | [x] Settings control: a "Timeline" row in `general_tab` (not yet in a separate Appearance tab — | superseded | [models-satellite](../../models-satellite/README.md); [evidence](../../../docs/certification/m5.1/README.md) |
| 555 | 8. Suggested phase order | superseded | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 657 | 9. Definition of done | certification-only | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |

Classification bases (by family):

- **HookEcho — Theme, Chrome & UX Overhaul Plan**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **0. What already exists — read this before touching anything**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **1. Rules specific to this work (in addition to `ROADMAP_NEW.md` §2)**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **2. Bugs**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **2.1 Multi-touch pan sends the map flying (web, touchscreen) — [x] mitigated, root cause on egui's**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **2.2 3D map basemap has empty gaps, mainly when zoomed out — [x] done**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **2.3 The search pill doesn't look good and should be an optional floating button — [x] done**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **2.4 Radar range ring shows km instead of miles — [x] done**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **3. Settings reorganization — [x] done**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **4. Analyst Mode (verbose live logging) — [x] done**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **5. Distance-unit and other small consistency items**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **6. Theme system rework**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **6.1 Naming — commit to this, don't re-litigate it mid-implementation**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **6.2 Turn `Layout` into a real preset (color + UI), while keeping colors independently**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **6.3 Build the new "WSV3" theme from Ref 4 — [x] partly done — geometry + footer shipped, the**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **7. Timeline (scrubber) styles**: Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2.
- **8. Suggested phase order**: Old execution ordering replaced by roadmaps/README.md priorities and simultaneous Tracks A–E.
- **9. Definition of done**: Preserved acceptance obligation; execute against the implemented slice and keep missing evidence open. Harness gaps are tracked separately in E1/E4.
