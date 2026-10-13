# Radar interrogation — P1

Parent M3.1–M3.6; ROADMAP_NEW C/H; suggestions §§2–7; 3d_rework.
[Evidence](../../docs/certification/m3.3/).

| ID | Status | Remaining work / acceptance |
| --- | --- | --- |
| R1 | Partially implemented | The editor's dimensional/datum advice now also reaches file import (as named advice notes; formulas imported as written) and the gate inspector (beside the value), from one `udp::unit_advice` (2026-10-12); a reflectivity difference types as dB. Compatibility policy: a formula this build cannot parse is imported and kept exactly as written with a note, never dropped or rewritten; only the size limits refuse (`a_formula_this_build_cannot_read_survives_a_round_trip`). Remaining: the editor/inspector in a running window. Advisory typing and 32 reference presets already exist. Never silently rewrite formulas. |
| R2 | Partially implemented | Independent per-pane column definitions/evaluation/environment ownership; revision-safe delivery and matched vertical datum. Exercise two different products/times and archived RAOB versus live HRRR missing levels. |
| R3 | Partially implemented | Meaningful column-product cross-section/3D integration and additional environmental fields only where scientifically defined. A scalar column is not a measured gate/vertical profile. |
| R4 | Partially implemented | Broader trail workflow, explicit site/tilt changes and contributor exports. Exact windows, bounded polar prefetch, minimum CC masking and column trails already exist. MRMS native accumulations are distinct from client-built extrema; avoid duplicating them. |
| R5 | Partially implemented | 3D pick should prefer measured gates; either ruler end now drags without re-clicking (2026-10-12, `docs/certification/m3.6/`; the 3D drag not yet seen in a run); arbitrary unequal/signed isosurfaces if supported by a demonstrated analysis workflow. Existing ROI, ruler values/terrain and PgUp/PgDn are retained. |
| R6 | Certification-only | Gate metadata/dealiasing and UDP/trail/section/3D science on pinned Moore/Mayfield/hail/QLCS/clutter; real desktop/Android ergonomic walkthrough and timing. No “tornado” conclusion from SW, rotation or low CC alone. |

Do not recreate radial-strip Observed, masked trilinear Smooth, transfer colour stops,
nested shells, GPU expression comparison or per-sector Nyquist. Test missing/folded
gates, ARL/MSL/geopotential distinctions, expiry/backward seeks and physical extrema
unchanged by opacity. Detector-impacting dealiasing changes require the existing
scientific owner/backtest process. Next ready task after P0: R1.
