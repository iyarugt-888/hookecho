# Storm workflow — P1

Parent M2.1–M2.4; ROADMAP_NEW R3/R4/K3/K4/L; suggestions §§4,6–9.
[Evidence](../../docs/certification/m2.2/).

| ID | Status | Remaining work / acceptance |
| --- | --- | --- |
| S1 | Partially implemented | Archive local-cell replay now feeds the storm table, history and selection path (2026-10-13, `celltrack::replay`; `docs/certification/m2.2/`); a follow run then found the sliding window renumbering storms so a followed ID passed to another storm; labels now carry from table to table and are never reissued (`celltrack::LocalLabels`, 2026-10-13, seen fixed forward and backward in the browser build). A real 90-minute Moore replay (forward, backward, newest-first decoding) then kept the tornadic storm on one label for all 29 minutes the tracker links it, after a fix for storms missing from a table (`archive_cell_labels`). Remaining: the replay played through and compared in the running app. Stable IDs, SCIT calibration, split/merge tags and rename-safe compare/follow already exist. Repeated and backward replay must reproduce identities without inheriting recycled IDs. |
| S2 | Partially implemented | Complete select→history/evidence→suitable radar→actual pass time→gate/section/ROI→motion/ETA→save/reopen/export consistency, including linked-group and site transitions. Preserve uncertainty, source clocks and manual motion independently of provider motion. An archive local cell's arrivals, associations and manual-track line now name the local tracker instead of SCIT (2026-10-13, `celltrack::tracker_of`; tests). Live ProbSevere no longer scores or draws over archived storms (`active_probsevere`). Cell core statistics/distribution/compare/digest now read every tilt (`cell_core`, By tilt table; 3D-placed distribution outline), tests and a full-volume Moore control, not seen in a run. |
| S3 | Certification-only | Desktop and physical Android walkthroughs of manual point/line creation, cancellation, split/merge history, notes, feature correction, town/asset entry and print/share. Measure the existing five-second creation criterion. |
| S4 | Partially implemented | Offline chase case portability: choosing satellite/MRMS/GIS data ahead of time and explicit pack dependencies. Recent cached frames, radar pack saving, feature/case notes and online routing already exist. Forecast route exposure and routing graphs remain deferred unless tied to a concrete field workflow. |

Feature tracking for mesocyclone/core/BWER/contour, suitability recommendations and
community polygon entry times are implemented; continue their validation, not a
second tracker/impact engine. Track D includes adjacent/crossing storms, ambiguous
associations, holes, missing scans and source-time changes. Detection algorithm
experiments remain separate and retain `detectionplan.md`'s rejected findings.
