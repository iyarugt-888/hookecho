# Repository and evidence audit — 2026-10-12

Baseline `ed5eccaa` on `feat/wsv3-redesign`; remote `hookecho` points to
`iyarugt-888/hookecho`. The checkout already contained staged HREF/model work and
unstaged shared-sweep, receiver-ownership, certification and roadmap edits. These
are **working-tree evidence**, not work attributable to this audit or to HEAD.
No reset, clean, stash, branch switch or pull into this shared dirty tree was used.

The [source inventory](archive/inventory.md) covers task sections and explicit
checkboxes in all fourteen planning sources, including `1008.md`, `1010.md`,
`newsuggestions.md`, `theme_plan.md`, `expansion.md`, and the workstation design
plan. Task-section classifications include their subordinate prose/list requirements;
checkbox classifications are conservative where a section mixes shipped and open
requirements. Individual checked boxes are not independently recertified here.
The original bytes and source-line references are preserved for review. The
inventory's classification basis is explicit; it is not a new test report.

## Findings that change the backlog

| Area | Actual implementation/evidence inspected | Audit disposition |
| --- | --- | --- |
| Live failover | `volume.rs`, `provider_health.rs`, `radar_provider_manager.rs`, `tgftp_provider.rs`, `app/radar_feed.rs`, `app/live_session.rs`; M1.3 reports | Three-tier selection, bounded retries, source declarations and loopback continuation exist. No live independent-domain operational proof. TGFTP fetches/decodes unchanged files before comparison; its single cache is not scoped to the requested site. First slice addresses this concrete defect. |
| Latency | `render::LiveQueueTimings`, M1.2 correlated hardware trace and browser live counters | Receipt→queue and receipt→GPU done exist. Neither measures visible presentation. Preserve stage names; collect revision-correlated presentation separately. |
| Progressive identity | M1.1 evidence; live scan, temporal preparation and accepted-revision keys | Extensive source/pass/sequence coverage exists. Do not rebuild SAILS stepping, strict masking or frame acquisition snapshots. Receiver fault tests and immutable sweep sharing are already dirty peer work. |
| Radar science | `udp*`, `extrema`, `app/column_trail.rs`, `app/trail.rs`, M3.1–M3.6 | Gate encoding, sector Nyquist, column reductions, recorded RAOB heights, advisory dimensional checks, GPU formula evaluation, bounded trail prefetch and CC-minimum echo masking exist. Strict/import/inspector diagnostics, independent column ownership and selected ergonomics remain. |
| 3D | `map_volume`, `standalone_volume`, `measure`, `volume3d`, M3.5/M3.6 | Native observed radial strips, masked trilinear smooth volumes, ROI, nested shells, translucent compositing and 3D ruler values/terrain/tilt stepping exist. Arbitrary unequal/signed shells and data-aware picking remain; Android performance is unknown. Empty-space skipping was deferred after measured desktop timings. |
| Storm workflow | `storm_history`, `storm_evidence`, Storms/Cell docks, feature tracking, impact reports, M2 and feature-track evidence | Stable IDs, rename/recycle protection, split/merge tags, manual motion provenance, case notes, feature tracking and town-boundary entry times exist. Archived local-cell identity replay and whole-workflow consistency/device walkthroughs remain. |
| Pane/model/GOES | `goes_panes`, `model_context`, `model_field`, context evidence | GOES per-request grids/textures landed in `1b60247f`; point forecasts in `17958804`. HREF probabilities are staged with live decode evidence. Neither is absent. Group-picker/mesoscale/health/platform coverage remains partial. |
| GIS | `gis_bundle`, `gis_layers`, import/export/scene modules, M4 evidence | Multiple layers, bundles, projected imports, styling/filtering, portable scenes and outlines exist. Real external reader interoperability and device imports still need evidence; dense-label behavior has remaining limits. |
| Output | `output_window`, `scenes`, `loop_capture`, M6 evidence | Held/pinned program, Cue/Take, scene palettes/annotations and deterministic offscreen capture exist. Window restoration (`7c29cbaf`) and strip GIF action (`7361ca60`) exist. Sustained interactive capture, actual encode and physical multi-monitor/device proof remain. |
| Mobile | `AlertService.onTimeout`, `Geo.kt`, `MainActivity.onTrimMemory`, platform/CI | Older “no timeout handler/no geometry/no trim listener” claims are stale. Physical-device lifecycle, memory, gestures, pen/IME/share remain unverified. Tablet layout decision is workstation chrome at 600 dp, not a missing third UI. |
| Long runs | M7.1 reports and `soak.rs` | 2h/12h/24h renderer runs completed. The 24h report includes one recovered listing error. UI, independent live failover, scene/pane transitions, surface loss and Android were not exercised. Full-application certification remains open. |
| Detection | `detectionplan.md` progress, methodology and rejected experiments | Preserve completed phases and corpus results. Same-cut debris pairing and probability calibration were investigated and not adopted; do not queue them again as untouched tasks. QLCS limitations remain explicit; evidence scores are not calibrated probabilities. Detector research stays in its owner lane. |

## Documentation contradictions resolved by the new index

- ROADMAP.md's tablet/web-persistence/model summary lags later implementation.
- ROADMAP_NEW's final scorecards and some early progress paragraphs contradict its
  later logs: native ABI channels, MRMS registry, RRFS, GR2-style products, GIS and
  independent context foundations already exist.
- ROADMAP_PARITY's opening baseline table is older than its own dated ledgers.
  For example, trail prefetch, colour stops and independent GOES are no longer absent.
- `3d_rework.md` describes the old fixed standalone grid and no view-following box;
  later M3.6 work supplies ROI/grid planning. Its Android gate remains open.
- `ARCHITECTURE.md` still says the web has no chunk stream and describes some field
  state as shared; current code/evidence takes precedence over these historical notes.
- 1008 H4's “Last” instruction and old linear M0→M7 ordering are superseded.
  Evidence, scientific regression, platform checks and long runs proceed continuously.

## Evidence boundaries

The certification manifest hashes committed evidence at its recorded SHA, not
uncommitted peer changes. Keep that distinction when regenerating it. Existing
captures and measured numbers are retained with their original dates/hardware;
none is represented as a fresh run. Compiler checks are not Android/browser runtime
proof; GPU completion is not presentation; different declared domain IDs are not
independence attestation; renderer soaks are not whole-app soaks.

External competitive claims remain comparison targets, not measured parity. The
[official WSV3 page](https://wsv3.com/) was checked on this audit date; the GR2Analyst
UDP page refused this audit's fetch (403), so no new GR2 version-specific claim is
inferred. Existing official references remain in the preserved source documents.
