# Models and satellite — supporting workflow

Parent M5.1–M5.4; ROADMAP_NEW D–G/J; 1010 V10/V12/V15/V16.
[Context evidence](../../docs/certification/m5.1/).

| ID | Status | Remaining work / acceptance |
| --- | --- | --- |
| C1 | Partially implemented | Complete independent pane/group context acceptance, source-health/probe/export consistency and resource eviction. GOES request-keyed grids/textures, MRMS and model contexts already exist. A non-active pane on its own time now keeps its own mesoscale footprint (`003cd456`; browser evidence in `docs/certification/m5.1/`); still exercise a pane inside the box on its own time, group picker changes and late delivery. |
| C2 | Partially implemented | Vetted model-field/vector/comparison completeness where the browser exposes a supported field. HREF probabilities landed in `a59ae634` (live inventory and decode; a render, not a window capture); finish operational validation in the desktop/Android windows without duplicating them. Keep threshold/member/run/units in identities. |
| C3 | Partially implemented | GEFS postage stamps/plumes and ensemble soundings only after the reliability queue, where they close the existing guidance workflow. REFS/new global models and broad catalog expansion are deferred. |
| C4 | Certification-only | Native GOES cadence, first/second-pass playback, decoded scan identity and crossfade cost across hardware. One-minute mesoscale loop and per-pane historical/native requests are already implemented. |
| C5 | Still needed | Full disk and preselected offline GOES packs remain deferred breadth. Keep native grid/cadence/missing disclosures. No current model substitution for unavailable archive data. |
| C6 | Certification-only | Browser provider availability and source-contract checks; a compile is not a fetch test. NOMADS-dependent and JPEG 2000 models (RAP, GDPS) are labelled desktop/Android only in the browser's pickers and layer requests (`bde9ec6c`, `b6e3c1a1`; `docs/certification/browser-limits/`) until access or a decoder is established. |

No new broad model/ABI/MRMS feature outranks P0 radar or existing analyst workflow
gaps. Preserve HookEcho's model/MRMS/provenance breadth and cross-platform behavior.
