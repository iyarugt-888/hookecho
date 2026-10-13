# Broadcast and deterministic output

Parent M6.1–M6.4; ROADMAP_NEW M; ROADMAP_2 §6; 1008 F1–F3.
[Evidence](../../docs/certification/m6.3/).

| ID | Status | Remaining work / acceptance |
| --- | --- | --- |
| B1 | Certification-only | Thirty-minute program output/capture with independent operator pan/product/time/scene activity, failures and recovery. Verify logical frame/time mapping, actual pacing and drift. Nominal 60 fps is not a measurement. |
| B2 | Partially implemented | Finish in-app capture readiness/resume coverage for every scene dependency (weather, GIS, overlays, dressing); refuse or pause missing frames with diagnostic manifests. Archived warnings, discussions and storm reports are now stamped per frame and never shown live or from another bucket over an archived scan (2026-10-13, `archived_or_live`; tests, `docs/certification/m6.3/`); GIS/dressing stamps and waiting on a not-ready overlay remain. |
| B3 | Certification-only | Scene restore/Cue/Take, palettes/annotations/3D/product/time/GIS, physical multi-monitor OBS and Android share workflows. Existing pin/hold isolation, durable scenes and monitor restoration are retained. |
| B4 | Certification-only | Repeat export on hardware/software backends, real ffmpeg encode where installed, and configured FTP/SFTP delivery if requested/configured. Existing offscreen byte-identical runs do not certify in-app screenshots or all adapters. |

Do not recreate the output viewport, scene model, fixed clock, strip GIF action or
OBS guide. Link evidence to input/scene hashes, actual output resolution, adapter
and build. Second-monitor/device/codec absence remains an explicit open gate.
