# ROADMAP_2.md — Professional Maturity / WSV3-Competitive Roadmap

## Purpose

This roadmap is focused on closing the remaining gap between HookEcho and a mature professional weather-analysis workstation such as WSV3.

The priority is **not adding more disconnected weather layers**. HookEcho already has broad feature coverage. The remaining work is primarily:

- making live Level II behavior operationally trustworthy under severe-weather load,
- making storm-analysis interactions as fast and polished as the underlying algorithms,
- strengthening GIS and broadcast workflows,
- improving rendering consistency and presentation quality,
- reducing architectural coupling,
- adding long-duration and corpus-based regression testing,
- and defining objective release gates for professional use.

The target is a workstation that can be used continuously during a high-impact event without the operator needing to think about data freshness, rendering edge cases, hidden source failures, synchronization mistakes, or UI instability.

## Implementation progress (2026-09-29)

Layer-probe provenance access (2026-10-01, §9.1, §10.2, §13.4): pinning the probe exposes
expandable layer rows with readable source context. Retained field stamps use the common
inspector for UTC clocks, signed offsets, receipt age, forecast/derived flags, quality, and
grid transformations. Available catalog metadata adds native units and masked missing codes.
Unstamped rows explicitly acknowledge incomplete provenance. Radar rows show selected-tilt
and sampled-radial times; unlinked source offsets use the displayed tilt as their reference.
The shared inspector calls its reference an analysis reference because a linked cursor may
be independent of a radar scan. The probe uses workstation styling, responsive width, and
a vertical scroll boundary. This improves access to existing stamps; complete stamping of
all overlay, derived, and observed sources remains open.
Validation: workspace Clippy passes with warnings denied; the Windows workspace test run
passes (1,944 passed, 106 intentionally ignored). All seven focused layer-probe tests pass
when including the GPU capture test. Four captures at 220/340 pixels cover the card and
shared source details (`target/ui-review/layer-probe-*.png`); the narrow renders were reviewed
for wrapped captions, padding, and source text. Complete application and device validation
remain open.

Probe stamps beyond fields (2026-10-01, §9.1): the radar row and the warning, watch and advisory rows now carry real stamps through the same shared inspector. Radar: the serving provider, the product as shown, the tilt's acquisition time as valid, and the live scan's receipt, derived for SRV, KDP and user products; only for the live head, since that is the only volume whose receipt is recorded. Alerts: the NWS sent time as issue, effective (else sent) as valid, the alerts feed's last success as receipt, marked forecast; `AlertInfo` gained `issued` and `effective`. SCIT storm cells: derived, valid at their NST scan, received when the cell feed last answered, while the pane follows live. No stamp is built from a time the app does not have: Model contours now record their run and the time their grid arrived, so their rows are stamped too (model, field, run, valid, receipt; derived for the STP/SCP/EHI composites). Archive and loop frames, archived warnings and the rotation and debris tracks still show the unavailable line. Unit tests cover both constructors; the on-screen check waits on the preview server.

Storm keyboard ownership (2026-10-01, §2.2, §13.1, §13.4): manual-motion shortcuts run
before the global binding table when the tool is armed or its card has keyboard focus.
Bracket cone edits no longer change panes; companion Text events are consumed too so
Android's text fallback cannot repeat a global action. Text-only brackets adjust the cone.
Card buttons permit local shortcuts, while text editors and Settings key capture retain
their events. When the tool is disarmed and focus leaves the card, global pane keys return.
Regression coverage exercises real egui input frames, local/global ordering, companion and
text-only events, duplication, key capture, card button focus, and text entry.
Validation: workspace Clippy passes with warnings denied; the Windows workspace test run
passes (1,941 passed, 105 intentionally ignored). Runtime device interaction remains open.

Storm drag ownership and cancellation (2026-10-01, §2.6, §13.2–§13.3): each drag records
its originating pane and original track. Another pane cannot edit or release its handle.
Pointer handling runs its cancellation path even when pinch/swipe handling takes over;
focus loss, touch cancellation, Escape, and tool changes restore an existing track or
remove a new vector, retaining unfinished line points while the tool stays armed.
Removed panes also release their drag. Primary-pointer editing leaves secondary drags
to map navigation. Regression tests cover pane ownership, normal owner release, lost
pointer/gesture rollback of point and line origins/motion, and interrupted new tracks.
These are deterministic interaction-state checks; runtime mouse/pen/touch validation
remains open.
Validation: workspace Clippy passes with warnings denied; the Windows workspace test run
passes (1,936 passed, 105 intentionally ignored).

Constrained manual motion editing (2026-10-01, §2.2): Ctrl-drag holds the existing vector's
bearing while its speed changes; Alt-drag holds its speed while its bearing changes.
Shift snapping remains available. Constraints use drag-start motion, avoiding frame-to-frame
drift, and new vectors can be created normally with lock modifiers held. Slow edits to
existing point/line tracks no longer discard them; only tiny new drags are discarded, with
unfinished line vertices restored. The card names these gestures. New tracks take their
analysis time from the displayed tilt rather than the volume start. Regression tests cover
repeated constrained edits, line geometry preservation, held modifiers on creation, and
existing/new slow-drag behavior. Full runtime mouse/pen/touch validation remains open.
The user guide documents these gestures and the dock Settings workflow. Validation:
workspace Clippy passes with warnings denied; the Windows workspace test run passes
(1,933 passed, 105 intentionally ignored).

Settings visual review (2026-10-01, §13.4): offscreen GPU renders of all nine shared Settings
sections at 280 and 640 pixels exposed clipped controls in narrow docks. Responsive forms
now stack their labels, choice rows wrap, hotkey bindings remain visible, and sound previews
stay beside their selectors. Search/custom URL fields and section navigation respect the
host width. The editor scrolls vertically. The ignored `gpu_settings_dock_snapshots` test
uses the app's fonts and workstation theme and writes captures to `target/ui-review`.
The three Settings dock tests pass when explicitly including the GPU capture test.
This verifies the isolated editor; full application interaction and pen/touch gates remain open.
Validation: workspace Clippy passes with warnings denied; the Windows workspace test run
passes (1,930 passed, 105 intentionally ignored). The Settings GPU test also passes when
run explicitly, producing all 18 narrow/wide captures.

Line uncertainty envelope (2026-10-01, §2.2–§2.4): manual lines now share the independent
left/right width controls. Each segment has a convex swept uncertainty part sampled through
the hour; the union preserves gaps around bends, and drawing and population queries use those
same parts. Zone checks evaluate the instantaneous envelope, including degenerate segments.
Point-arrival uncertainty considers eligible interior vertices as well as endpoints. Geometry
cache identity includes every line vertex and control, and drawing reuses the parts until
that geometry changes. Regression cases cover asymmetric line coverage, reversed vertex order,
bends, preserved gaps, hull degeneracy, vertex edits, and geometry reuse. These remain motion
estimates; runtime visual and pen/touch validation are still open.
Validation: workspace Clippy passes with warnings denied; the Windows workspace test run
passes (1,930 passed, 104 intentionally ignored), including the line-envelope and geometry
cache regressions described above.

Independent uncertainty widths (2026-10-01, §2.2, §2.4): manual point tracks now have left and
right base widths, editable in the motion card, with symmetric defaults. Swath drawing, point
arrival flags, zone flank checks, and population lookup identity use the selected side.
Zone intersections check the segment between the center's flanks, and storm-line segments
crossing a polygon are detected even when no vertex or midpoint falls inside it. Width/cone
tests check agreement between the footprint and point arrivals, asymmetric zone coverage,
and refreshed impact identity. The line-envelope update above extends these controls to lines;
pen/touch validation remains open.
Validation: workspace Clippy passes with warnings denied; the Windows workspace test run
passes (1,924 passed, 104 intentionally ignored), including asymmetric footprint/arrival,
side-specific zone, cone expansion, small-zone line crossing, and segment intersection tests.

Projection interval control (2026-10-01, §2.2, §2.3, §13.4): each manual point or line track
has a display interval, selectable in the motion card (5/10/15/20/30/60 minutes, default 15).
The same iterator draws point markers and future line edges and always includes the 60-minute
endpoint. Display changes preserve motion, ETAs, footprints, and population lookup identity.
The motion card sizes to the map and scrolls long track lists; cone/width controls wrap in a
narrow host. Mouse/pen/touch runtime validation remains open; point-track uncertainty widths
are covered by the subsequent update above.
Validation: workspace Clippy passes with warnings denied; the Windows workspace test run
passes (1,919 passed, 104 intentionally ignored). Regression tests cover endpoint retention,
non-dividing and invalid intervals, and unchanged motion, impacts, and line geometry.

Storm association foundation (2026-10-01, §2.1, §2.5): Cell analysis now uses shared spatial
associations retaining indices into the original warning, ProbSevere, and circulation objects.
Warning and ProbSevere coverage respects polygon holes. Circulations are assigned against all
current SCIT cores, with a 10 km limit and explicit ambiguity when competing separations differ
by at most 1 km. This prevents separate open cards from independently claiming a signature.
The card includes ProbSevere provider IDs and probabilities, explains containment and nearest
core associations, and names unavailable provider times. This is the spatial foundation;
cross-source temporal confidence, manual annotations, and a persistent unified storm entity
remain open, along with pen/touch validation.
Validation: workspace Clippy passes with warnings denied; the Windows workspace test run
passes (1,917 passed, 104 intentionally ignored), including competing-core, ambiguous-match,
distance-limit, and polygon-hole association regressions.

Dock Settings and freshness corrections (2026-10-01, §1, §13.4): main Settings now lives in
the workstation window system with its theme, docking, collapse, and persisted placement.
All nine sections use shared controls; narrow hosts keep navigation above scrolling content.
Search brings Layers forward from another selected dock tab, and Enter uses the ranked visible
match. Radar valid-time and age readouts use the displayed tilt and moment's newest acquired
radial, cached with the volume and refreshed on progressive merges, including SAILS/MRLE.
No-timestamp sources fall back to the volume time. Regression coverage checks tilt changes,
repeat cuts, live merges, timestamp absence, and search behind another tab. Visual and
pen/touch validation remain open.
Validation: workspace Clippy passes with warnings denied; the final Windows workspace test
run passes (1,913 passed, 104 intentionally ignored). The Settings editor's narrow and wide
hosts, saved placement, old-arrangement defaults, and key-capture release are covered offline.

Storms dock operator workflow (2026-10-01, §2.5, §13.4): the ImGui Storms table now filters cell
IDs, reports the matching count, and shows each row's SCIT timestamp and severity evidence on
hover. A row's context menu opens the full Cell analysis window, centers the map, or seeds the
manual motion tool from SCIT through the same path as the Cell window. Arrow keys step through
the filtered sort order; Enter opens details. Focused list navigation reserves its keys before
global hotkeys run, preventing an arrow press from scrubbing radar history. The severe-hail
probability header is corrected from SHI to PSH (POSH on hover); numeric text is clipped per
column, rotation flags have a header, and the footer explains keyboard controls. Filtering and
shortcut ownership have offline regression tests. This advances operator efficiency; pen/touch
verification and the full unified storm model remain open.

Windows regression gate (§3, §8, 2026-10-01): the offline workspace run exposed three plugin
tests that assumed Unix commands and a bundle-import test that wrote into the user's palette
directory. The plugin tests now use PowerShell on Windows (and retain the Unix path elsewhere),
including the timeout case. Bundle import's test uses an explicit temporary destination and
cleans it up, exercising the same import implementation without touching user palettes.
Mechanical renderer/test lint findings were corrected as part of restoring the required gates.
Validation: `cargo clippy --workspace --all-targets -- -D warnings` passes, and
`cargo test --workspace` passes on Windows (1,906 passed, 104 intentionally ignored).
The final run disabled incremental caching after generated build caches exhausted the disk.
Visual, pen/touch and prolonged live-session verification remain open.

Phase 1 has started with a per-pane live Level II acquisition state (`crates/hookecho/src/live_scan.rs`). It tracks the current provider, chunk/cut progress, accepted volume identifier and time, recovery, fallback, aging, and staleness. The top-bar radar status now exposes that state; queued updates from superseded streams and older volume/chunk updates are rejected. Deterministic unit tests cover sweep completion, supplemental cut positions, recovery, polling fallback, and time reversal.

The partial-sweep stitcher now keeps the newest radial at each azimuth even when a delayed chunk arrives later. Repeated and older chunks no longer produce a false changed-sweep update. Synthetic tests cover reordered chunks, gap filling, retention of the previous pass, and the stale-sector mask passed to rendering.

The direct 2D live-tilt display now offers continuous composite (the previous-pass sector remains dimmed) and strict current sweep (older-pass rows are hidden). The mode is saved in settings and can be switched from the radar ribbon. Strict mode uses nearest sampling to prevent smoothing from bleeding current values into masked older azimuths. Synthetic tests cover old-row masking, including separated older sectors, and settings compatibility. Derived multi-tilt products and 3D still need their own temporal policy.

The relay failover path now stamps live updates with the newest radar acquisition time instead of the client's wall clock. The ingest service uses the source message timestamp for VCP/pass-through blocks, so a delayed VCP retains the same volume identity as its radials. The HTTP and WebSocket integration tests exercise a scan received two minutes after acquisition; the 54 radar-ingest unit tests pass.

Both progressive Level II providers now attach source volume start, VCP number, and standard/SAILS/MRLE/MPDA cut kind to progress events. The per-pane state inventories observed chunks for each volume and cut, reports unobserved chunks in the top-bar details, and waits for every chunk before declaring a sweep complete. Late gap fills do not move the sweep marker backward; delayed progress from an older volume is rejected across reconnects. Focused tests cover gap filling, new volumes, supplemental cuts, and reconnects.

Analyst Mode now has a scan progression section backed by retained cut-position coverage for the current volume. It shows VCP, observed and complete cut counts, the expected next cut, elapsed time from the source volume start, and a rough remaining-time estimate at the current cut's rate. Individual cut badges distinguish unobserved, partial, and complete cuts. Delayed chunks from an earlier cut can fill its inventory without moving the current cut marker backward; a VCP change resets the cut inventory. The state declares a volume complete only when every expected cut is complete, including an earlier cut with a delayed gap fill.

Provider switches now emit a reasoned Analyst Mode log entry when the app starts a different live subscription or falls back to completed-volume polling after stream loss. The per-pane feed state and Analyst header expose the last switch reason and whether the selected source delivers progressive radials or completed volumes only. Clearing a manual provider override records the resulting automatic tier selection. This makes a TGFTP downgrade's temporal-resolution change visible alongside the provider name.

Progressive updates now carry a client-side monotonic transport receipt timestamp through decode and UI delivery to the 2D radar GPU queue write. Per-pane/source queue timings retain 128 rendered samples; Analyst Mode shows p50/p95 and sample count, and the radar health panel shows the latest duration. This measures receipt to completed queue writes, including decode and UI wait. GPU execution and actual on-screen presentation still need instrumentation before the desktop p50/p95 release budget can be certified. TGFTP completed-volume updates have no comparable pre-decode receipt timestamp and are excluded from this distribution.

The remaining Phase 1 work includes radial gap inventories, GPU presentation latency measurement, and prolonged source-failure/soak verification. Chunk coverage here reflects chunks observed by the current session; a mid-volume join may show earlier chunks as unobserved even if the assembled scan backfilled their radials. The release gates below remain open until those behaviors are verified end to end.

Radial gaps inside received chunks now read plainly — "8 radials missing (#1–4, #10, …)" — in a Gaps row of Analyst Mode's scan progression and in the live-scan tooltip, instead of a raw span list.

Deterministic replay (§0.2): `archive_replay_is_deterministic` (golden_events, network) downloads and decodes the Moore volume twice and requires byte-identical tilts and exactly the same detections and merged circulations. Its first run failed: the tilts matched, but the debris contrast differed in its last bits, because it was a float mean over a hash set whose order changes per run. Fixed, with two more of the same kind: rotation's grid cells were a hash map whose order survives the stable sort among equal (quantized) speeds, and the dealiaser broke a tie between +n and -n folds by hash order. They are now a B-tree set, a B-tree map and an explicit tie-break (toward +n, tested). The golden tests' archive listing now retries like the download, since eight of them at once drop the odd listing.

Live-state finish (§1.1, §1.3): every 1.1 and 1.3 acceptance criterion is now ticked. The app bar's word and colour for the live radar come from the live-scan phase (`live_word`), not from the request book's loading flag, so it no longer read 'Fetching' while sweeps arrived or 'Recovering · Fresh' at once. A stream the app stops on purpose (a playing loop, a scrub, a site or provider change) no longer reads as Recovering (`stream_stopped`); on the web build, where the loop plays by default, it had sat at Recovering between volumes. Provider switches are logged on the web too, with the reason telling a lost stream from a paused one (`poll_reason`). A hung poll can no longer stop live polling for good (`poll_may_start`: one in flight past VOLUME_TIMEOUT + 15 s is abandoned). Both providers' cut kinds come from one tested `CutKind::from_flags`. Returning to the stream after polling is tested not to reverse time.

Phase 2 has started with the manual storm-motion tool (§2.2, `crates/hookecho/src/app/storm_track.rs`, `MapTool::StormTrack`). Dragging from a storm to where it will be in an hour draws the track under the pointer: +15/+30/+45/+60 marks with clock times from the analysis time it was placed at, a swath whose half-width grows by an uncertainty cone, and the ETA and closest approach (side and distance) at every saved marker, recomputed while the handle moves (§2.4 for markers). Either end drags to edit; Shift snaps the heading to 5°; Delete removes, Ctrl+D duplicates, `[` and `]` narrow and widen the cone; a card edits speed, heading, width and cone by number, so nothing needs Settings. Manual tracks are magenta and tagged MANUAL, apart from the automatic SCIT motion. ETAs are rounded to the minute and marked approximate; points behind the storm or more than two hours ahead get none. Tracks are session-only. The Cell window's **Track manually** seeds a track from the SCIT cell's automatic motion, to adjust by hand beside the SCIT track.

Line tracking (§2.3): with the tool armed, clicks on open map lay out a storm edge (dashed; Backspace undoes a point) and the next drag, from anywhere, is the whole line's motion. The line is drawn now and every 15 minutes with the swept area filled, tagged MANUAL LINE; a marker's ETA is when the moving line crosses it ("line arrives"), and past either end it is read from the nearer end's widening swath. The origin handle moves the line, edge and all.

Impact (§2.4): the card also gives, for the selected track, when the storm (or any part of a line) first enters each saved watch zone within two hours, "inside now" when it already is, and when only the uncertainty swath's edge first touches a zone, marked apart from a real entry. **People in path** asks the 2020 Census (the lookup alert cards use) for the population, homes and largest towns inside the hour's footprint, on request only. The Census places carry no coordinates, so per-town ETAs, and roads, are not yet done; touch/pen verification is also open.

Failure injection (§3.2) has a deterministic suite, `crates/wxdata/tests/failure_injection.rs`, run by every `cargo test`: error pages and empty bodies to every decoder, a real GRIB2 message and ODIM files cut at every point, a GRIB header claiming more than it holds, damaged gzip streams, malformed Archive II volumes and cut-off live-chunk framing. Every case must be an error (or, for an HDF5 tail the decoder never reads, exactly the whole volume), and none may take more than 5 s. It found the Level II decoder reading a JSON error body, a zero-filled buffer or a bare header as a volume with no sweeps; such a volume is now an error, so a failed fetch is retried instead of being shown or cached as an empty radar. Injection at the HTTP layer followed: `net::redirect_feeds` (native, off unless a test turns it on) sends every feed through `fetch_url` to a local server, and `crates/wxdata/tests/network_injection.rs` makes it answer the way failing servers do (a 500, an HTML error page, an empty 200, a body cut off mid-transfer) and points it at a closed port for being offline. Alerts, watches, the SPC outlook, ProbSevere, archived warnings, storm reports and METARs must each return an error, within 20 s, in every case. It found storm reports and METARs reading an error page or an empty reply as zero items, which would have looked like a quiet, healthy feed; both now call those errors (an empty 204, the METAR API's real "no stations", stays empty), and the live services still parse. Still open in §3: GOES listing, MRMS archive and model-index failures (their fetches do not all go through `fetch_url`), and device loss.

Workspaces (§12): a **Tropical** starter (reflectivity, storm-relative velocity and reflectivity over infrared satellite from the active radar, linked, with the NHC track and cone, recon, surface obs, alerts and watches) joins the existing Tornado, Hail, National, Chase and analysis presets. Starters used to be seeded once, on first run, so a preset shipped later never reached anyone; each starter a settings file has never been offered is now added once (`Settings::offered_starters`), and one someone deleted stays deleted. Applying a workspace names what this build cannot restore (unknown layers, fields, radar sites, map styles, too many panes) in the error chip and the log instead of skipping it silently, and `Workspace`/`PaneSnap` keep fields a newer build wrote, so opening and saving here does not drop them.

Timeline (§10): the layer probe's field and model-contour lines now give each source's signed offset from the time it is read against, with the reference named so the sign means something ("(Δ-42s vs radar)", "(Δ+1m 15s vs analysis)"): the linked analysis time when panes share one, else the pane's radar scan (§10.2). With no run pinned, model layers (forecast reflectivity, CAPE/SRH, the other regional fields, the global models) used to read the newest run even under a replayed historical event; scrubbed back more than three hours they now read the newest cycle at or before the view's time from the NOAA archive (§10.3). Before the archive's start (HRRR: 2014) the fetch fails visibly rather than substituting today's run.

Unified tornado detection (a first part of §2.1): rotation couplets, debris signatures and Tornado ID drew a marker each, so one tornado read as several, and an observed tornado warning confirmed every weak detection inside its polygon. `wxdata::tornado_id::circulations` now centres one detection on the most likely area of rotation (the best-corroborated cyclonic couplet; debris with no rotation near it centres its own) and ties in every couplet and debris signature within 15 km (checked on the 11 December 2021 Quad-State storm, whose debris ball and couplets sat 11–13 km apart), keeping each as a member with its own provenance and confidence. The verdict is Tornado ID over the circulation's own members, so tiers and reasons are unchanged; a circulation needs radar evidence or a tornado report of its own, and an observed warning raises its tier but cannot create one. The map draws one marker per tornado; a click or tap opens a web of spokes to each tied detection and a pinned card with the verdict, the members and the factor breakdown of the one picked. "One detection per tornado" (Severe layers, on by default) switches back to a marker per detection. Still to join the object: SCIT cells, ProbSevere, hail attributes and manual motion.

Storm selection (§2.5): the Cell window, which already held the SCIT attributes, severity and its reasons, core statistics, the forecast track and trends, gains a **Threat** section: the merged tornado detection at the storm (tier, score, signals, where from the core, or that the detectors are off), each warning whose polygon holds it with its tornado tag, and when SCIT's motion brings it to each saved place, in-path first, with the motion it came from. A **Track manually** button seeds the manual motion tool from it.

Telemetry (§14.1, local only): the Analyst log now gives this app's own cost: how long each frame takes to build (p50/p95/max over the last 600 frames, `app::telemetry`), how many of those ran over a 60 Hz refresh, and stalls over 50 ms since launch, beside the panes open and the radar volumes held. It times `HookEchoApp::ui` from outside, so early returns count; it is CPU build time, not the gap between frames (the app repaints on demand). Still open: GPU upload and presentation time, cache hit rates, HTTP latency and memory.

Basemap robustness (§4.5): ancestor fallback was already in place (a missing tile is stood in for by resident children or the nearest ancestor), but a failed tile was retried every 5 s forever. Retries now back off from 5 s, doubling to a 5-minute cap (`tiles::retry_after`), and the Analyst log shows coverage: tiles loaded, loading, failed, and how many are backing off.

Soak runner (§3.1): `hookecho --soak SITE [MINUTES] [--inject]` runs the Level II path (list the newest volume, download, decode, bin every tilt of reflectivity and velocity) every 30 s for as long as asked (120 minutes by default; 720 and 1440 for the 12- and 24-hour profiles), printing a line per cycle and a JSON summary, and exits non-zero on a stalled feed (no new volume for 20 min), an unrecovered failure streak (5 cycles, not recovered by the end), accepted corruption, or memory growth after warm-up (over 1.5× and 200 MB; resident memory read on Linux and Windows). `--inject` hands every fourth new volume over cut in half. Its first run found that a Level II download cut short decodes without error, as the first part of the volume: `level2::scan_complete` now checks for the radar's end-of-volume radial, the archive cache keeps only whole volumes (a truncated download is refetched rather than cached as the real thing), and the soak fails a volume that is not whole. It soaks the data path headless; the renderer's long-run behaviour is what the Analyst log's telemetry watches in a real window. Still open: the 12- and 24-hour runs themselves, and HTTP-layer failure injection.

Recovery rules (§3.4): each health row now says how it recovers, read from the same numbers the state machine uses (`SourceHealth::recovery`): retried every cadence with no backoff, delayed after one cadence and stale after two, the last good data kept on the map, marked Cached, until a refresh succeeds. Each feed also declares a severity (`FeedSource::severity`): warnings, watches, discussions, storm cells, storm reports, ProbSevere, tropical cyclones, the derived radar fields and the radar itself are critical; the rest routine. Only a critical source needing attention lights the Sources tab's red dot; a routine one is still listed. Not yet: per-feed backoff, and withdrawing data past a hard age limit.

app.rs decomposition (§7) has started, behaviour-preserving: the request book and source health (`app/request_book.rs`), the overlay toggles (`app/overlay_toggle.rs`) and the action vocabulary, `AppWindow` and `PaletteAction` (`app/actions.rs`), moved out unchanged, with serialized names untouched. This session's new features also went in modules of their own (`storm_track`, `scale_bar`, `telemetry`, `soak`, `ui::changelog`) rather than into `app.rs`. The background-fetch layer (`OverlaySource`, `OverlayMsg`, `OverlayDelivery`: `app/overlay_fetch.rs`) and the pane layout maths with its tests (`app/pane_layout.rs`) followed, and the request-book tests moved beside the request book. A ratchet test (`app_rs_only_gets_smaller`) fails if `app.rs` grows past its ceiling, now 11,887 lines (from 30,199); each extraction lowers it. The single 19,000-line `impl HookEchoApp` is being split by domain into `impl HookEchoApp` blocks of their own, methods moved unchanged: signature detectors (`app/detectors.rs`), warnings and notifications (`app/alerts_watch.rs`), chasing and the trail (`app/chase.rs`), workspace apply and capture (`app/workspace_apply.rs`), the model browser (`app/models.rs`), the time-following GOES and MRMS layers (`app/time_layers.rs`), 3D (`app/view3d_state.rs`), offline packs and soundings (`app/packs_soundings.rs`), the radar feed (`app/radar_feed.rs`), terrain and beam tools (`app/beam_tools.rs`), surface feeds (`app/surface_feeds.rs`), alert rules and the digest (`app/rules.rs`), sharing (`app/sharing.rs`), and account sync and updates (`app/account_sync.rs`). What remains in `app.rs` is mostly the frame (`ui_frame`, 2,700 lines) and the pane renderer (`render_pane`), which need splitting from the inside rather than moving whole. The first inside split: `render_pane`'s detector markers (debris, hail spikes, ZDR columns, couplets, Tornado ID and the one-per-tornado webs, about 570 lines) are `paint_detector_markers` in `app/detector_markers.rs`, the locals they read passed in a `Markers` struct under the same names so the drawing code did not change. The storm-cell drawing followed: arrival-time cones and the nowcast (`paint_cones_and_nowcast`), and the local and SCIT cell tracks with the cell dots (`paint_cell_tracks`), in `app/cell_markers.rs`. Then map-click handling (what a click or long press hits: your markers, a peer's stream, a zone, a webcam, a station, a damage point, a gauge, else the armed tool; about 570 lines) became `handle_map_click` in `app/map_click.rs`; it returns true where the block used to leave `render_pane` early, and the caller still does. Warning polygons, model contours and METAR plots followed as `paint_alert_polygons`, `paint_model_contours` and `paint_metar_plots` in `app/pane_overlays.rs`. Browser-testing that move turned up a web-only crash unrelated to it: `wxdata::ndbc` (buoys, fetched with METAR), `meteoalarm` and `rain_arrival` read `std::time::Instant`, which panics on wasm, so turning on Surface obs killed the web build. They use `wxdata::clock::Instant` now, and `tests/wasm_clock.rs` fails on any std clock outside the native-only modules. The 3D-mode overlays (MRMS surface, isotherm sheets, cell columns, beam guides) are `app/pane_3d_overlays.rs`; spotters, webcams and scan age are `app/pane_points.rs`; river gauges and radar sites, which place labels, are `app/pane_stations.rs` and borrow the label placer. Two behaviour changes came with that. A spotter click is handed back rather than stored from inside the pane, so a second pane no longer erases a click made in the first. And projected-track time labels no longer pile on each other or run off the pane's edge. Reference marks (live stations, range rings, saved locations, imported GIS points and labels) followed into `app/pane_marks.rs`, and the city and town labels into `app/pane_places.rs`. The legends (desktop scale, phone strip, HRRR key) went to `app/pane_legends.rs`, and pilot reports, ProbSevere, placefile labels and routes to `app/pane_feeds.rs`. The satellite cloud-top surface joined the other 3D overlays.

Golden checks (§8.2): `crates/wxdata/tests/golden_events.rs` runs real archived volumes through the app's own detector pipeline (four tilts, dealiased velocity, debris and rotation detection, cross-corroboration and the one-detection-per-tornado merge) and pins what each must show: the tilt inventory, peak reflectivity within 1 dBZ, a confident debris signature (CC ≤ 0.25) and a couplet within 8 km of the documented tornado, and exactly one Debris-tier detection for it. Two events so far: Moore, KTLX 20:12Z 20 May 2013 (debris CC 0.21 and a 50 m/s couplet about 5 km from Plaza Towers, one circulation) and Mayfield, KPAH 03:23Z 11 December 2021 (CC 0.21, 28 m/s, one circulation). Volumes are fetched from the NOAA archive, not committed, so the tests are `#[ignore = "network"]` (`cargo test -p wxdata --test golden_events -- --ignored`). Four more corpus tornadoes are now pinned against the tornado reports themselves (IEM LSRs within 20 minutes and 150 km, so no position is typed in by hand): each must have a Likely-or-higher detection within 8 km of a report, and exactly one there. El Reno (KTLX 23:10Z 31 May 2013, Debris 0.91 at 1.4 km), Washington, IL (KILX 17:05Z 17 Nov 2013, Likely 0.70 at 1.7 km), Mayflower–Vilonia (KLZK 00:35Z 28 Apr 2014, Debris 0.64 at 1.1 km) and Nashville (KOHX 06:45Z 3 Mar 2020, Debris 0.66 at 5.3 km); `record_corpus` prints what each finds. The false-alarm cases are pinned too (`quiet_cases_claim_no_debris_tornado`): no debris at all on Joplin's pre-dual-pol KSGF, and no Debris-tier detection in the Iowa derecho or the Denver hailstorm. The hailstorm found a real false alarm: a low-CC hail core (CC 0.33 in 48 dBZ) with no rotation within 15 km read as a Debris-tier tornado. In a merged detection, debris with no rotation near it, all flagged unrotated and unconfirmed, is now capped at Possible with the reason given (a report or observed warning still raises it); classic Tornado ID is unchanged. All six tornadoes still pass; warning verification (§8.4) has its first case: on Moore, the archived Tornado Warning covers the town at 20:12Z and nothing covered it at 18:00Z (the issue transition), the IEM tornado report lies within 15 km, and `detverify::score` credits the merged detection with it (1 verified, 1 found).

Visual goldens (§8.3): the radar pipeline's golden-image check (a synthetic sweep through the real wgpu renderer, per-channel delta ≤ 8 on ≤ 0.5 % of pixels) now covers velocity and CC as well as reflectivity (`check_golden` in `headless.rs`; goldens in `crates/hookecho/tests/golden/`). The new goldens were rendered on an RTX 2060; the existing reflectivity golden, authored under CI's lavapipe, passes on that GPU within the tolerance, so the two rasterizers agree on this pipeline. CI now publishes every actual render, so a future golden's lavapipe reference is one artifact away. Warnings over radar followed (a tornado and a severe thunderstorm warning through the app's own overlay tessellator, rendered on the same RTX 2060), and the cross-section panel (a synthetic eight-tilt storm cut through its core; CPU-only, so it runs in every `cargo test`: `ui::xsection_window::tests`). `HOOKECHO_GPU_FALLBACK=1` on Windows picks Microsoft's WARP rasterizer, which the goldens do not match; run them on the hardware adapter or under lavapipe. Not yet: four-pane layout, 3D observed and smooth, and storm tracks.

Provenance (§9.1): the Inspector's Source section now gives the radar product its stamp beyond volume and provider: whether it is observed or derived (KDP, storm-relative velocity), its units with the native ones when the display converts ("kt (native m/s)"), and what HookEcho did to it (dealiased, computed from differential phase, storm motion subtracted). Gridded fields already carried a `DataStamp` and descriptor units in Layer options; warnings, METAR and the other overlay feeds still have no per-item stamp.

Source health panel (§9.2): the Data source health window gains a Retry column (in flight, due now, or "in 1m", with the source's recovery rule on hover) and names the provider serving a source when it reports one (radar's active Level II provider); its error text takes the shared Unavailable red. The dock's Sources rows say the same on hover (Provider, Next try). It already had the last success, newest valid time, cache residency, recent outcomes and the last error. Cache hit/miss rates are still open: residency is all the request book can prove.

Stale-data look (§9.3): one set of colours for how current data is, `ui::freshness::Freshness` (fresh, aging, stale, unavailable). Source-health states (`health_look`, which the Layers list, Sources window, ribbon and app bar all use) and the live radar's scan phases both map onto it, so a stale feed and a stale radar are the same amber. Before, the timeline's Stale badge had its own yellow, the app bar coloured Aging, Recovering, Fallback and Offline all with the theme's warning colour, and Cached had an orange of its own; Cached now reads as stale, its database glyph and word still saying why. The radar's stale threshold is now the user's: Preferences → App → Data age sets it (3–120 min, 15 by default, the old fixed 900 s), and the Live/Stale badge, the radar's health row and the live scan's Aging (from 80% of it) all read that one number (`radar_fresh_secs`). The same page exposes the layer time-mismatch warning, which had no control. That covers §1.1's "stale live data is visibly marked before it reaches a configurable threshold".

Camera (§4.4): fly-to easing (`app/camera_flight.rs`). Each pane remembers the camera it last drew with; a change bigger than a pan or zoom step (half a view away, or 1.5 zoom levels) that the user's own drag, pinch or wheel did not make becomes a flight there, easing in and out over 0.45–1.2 s by distance, rising partway on a long hop so both ends are in view (about log2 of the distance in view widths, plus one), turning the short way round, and settling exactly on the target. Generic rather than per call site, so every jump (search, links, "Center", a new radar site, alert zoom-to) flies. The user's hand cancels a flight at once, nothing they move by hand animates, reduced motion keeps jumps instant, and a storm-follow's per-scan step stays below the threshold, so scrubbing never brings motion of its own. Not yet: warning-follow mode, 3D orbit controls and the map/3D transition.

Broadcast output (§6.1, §6.4, §6.5): an output window (`app/output_window.rs`, native): the active pane, clean, in a window of its own titled "HookEcho Output" (a stable identity for an OBS window capture), apart from the operator's controls. It carries streaming mode's dressing (clock, caption, warning crawl and logo inside the title-safe margin; `paint_broadcast` now draws it into any rect) plus an optional title strap with custom text. Sizes are 1920×1080, 2560×1440 and 3840×2160 in physical pixels whatever the display scaling, or free; fullscreen fills the monitor the window is on, and Esc there leaves it. Opened from the palette ("Output window") or beside streaming mode's settings. Scenes (§6.3, `app/scenes.rs`) save what the output shows: the radar site, where the map looks (centre, zoom, pitch, bearing), the overlay layers, the colour scale, the dressing, the title strap and the output size, by name in settings; Alt+1..9 switch to them in the order saved (a bare digit stays a product key) and applying one flies the camera there. They, the streaming overlay and the output window's settings are now on the dock's Map settings page too, which had nowhere to set them. Fixed-rate export (§6.2, §6.6): MP4s are now scheduled deterministically in whole 30 fps frames (`loopexport::cfr_counts`): each weather frame's frame count comes from the loop's running time, so rounding never accumulates (every frame starts within one output frame of its true start however long the loop), and every weather frame gets at least one output frame, so a 20 ms SAILS hold is shown rather than dropped by ffmpeg's resampling. The concat list carries those exact durations, and the same input always gives the same schedule. (ffmpeg was not available to encode a file end to end here; the schedule is unit-tested.) Not yet: monitor choice by name, a clock that renders frames at explicit timestamps rather than capturing the screen and fixed-FPS export (§6.2, §6.6), and transparent-background overlay mode.

Accessibility (§13.4): the tornado markers (Tornado ID's and the merged one-per-tornado detection) said their tier by colour alone, with a "!" in every triangle; each now carries its tier's letter (P possible, L likely, D debris, C confirmed; `Tier::glyph`), so the tier reads without its colour. The couplet rings already differ in shape (strong ones are filled), the manual storm tracks say MANUAL, and the source-health rows have a glyph per state.

---

# 0. Program goals and release gates

## 0.1 Product goals

HookEcho should be able to support all of these use cases without changing applications:

1. **Live severe-weather monitoring**
   - near-live Level II radar with explicit freshness and acquisition state,
   - rapid tilt/product switching,
   - reliable warnings, SCIT, ProbSevere, lightning, MRMS, satellite, and model context,
   - storm-relative and derived analysis without disruptive loading.

2. **Professional storm analysis**
   - multi-pane synchronized views,
   - cross-sections and 3D volume analysis,
   - storm tracking and ETA workflows,
   - rapid inspection of data values and provenance.

3. **Historical forensic analysis**
   - consistent archive behavior,
   - warning/report synchronization,
   - reproducible derived fields,
   - event verification,
   - exports suitable for review or documentation.

4. **Operational GIS / broadcast presentation**
   - custom vector data,
   - reusable scenes/workspaces,
   - clean external output,
   - stable capture,
   - professional labels and transitions.

5. **Cross-platform use**
   - Windows as the primary professional desktop target,
   - Linux/macOS parity where feasible,
   - browser/WASM as a first-class analysis surface,
   - Android as a field/chase companion with shared core behavior.

## 0.2 Release gates

Before declaring the application "professional-station ready", all of the following should be true:

- [ ] 12-hour live severe-weather soak test passes with no unrecovered feed, renderer, memory, or UI failure.
- [ ] 24-hour idle/live mixed soak test passes on Windows.
- [x] no stale radar scan can be displayed as current without a visible stale-data indication.
- [ ] all displayed forecast/derived/observed layers expose provenance through a common inspector.
- [x] live Level II partial-volume behavior is covered by deterministic tests.
- [x] VCP transitions, SAILS/MESO-SAILS, missing chunk, late chunk, duplicate chunk, and out-of-order chunk cases are tested.
- [x] all critical storm-analysis actions are reachable by command palette and direct pointer interaction.
- [x] archive replay is deterministic across repeated runs for the same volume/time.
- [x] export/capture output is frame-stable and does not depend on UI repaint timing.
- [ ] no single application source module remains responsible for an unreasonable number of unrelated domains.
- [x] nightly/corpus regression suite runs against known historic storm cases.
- [ ] performance budgets are defined and measured for desktop, browser, and Android.

---

# 1. P0 — LiveScan-class Level II acquisition and presentation

This is the highest-priority maturity gap.

HookEcho already has chunk-level Level II ingestion, chunk timing models, retry logic, elevation mapping, live polling, and partial-volume assembly. The next step is to make the **in-progress radar volume itself a first-class state machine**.

## 1.1 Introduce an explicit live-scan state model

Create a dedicated live radar acquisition model with states such as:

- AwaitingVolume
- AcquiringVolume
- AcquiringTilt
- SweepPartial
- SweepComplete
- VolumeComplete
- Recovering
- FallbackSource
- Stale
- Offline

Track:

- radar site,
- VCP,
- volume number / source identifier,
- current elevation cut,
- split-cut identity,
- sweep start time,
- newest radial time,
- newest chunk receive time,
- provider/source,
- estimated beam age,
- volume completeness,
- missing radial/chunk ranges,
- source failover status,
- clock skew.

### Acceptance criteria

- [x] UI never needs to infer live state from a generic loading boolean.
- [x] current sweep and completed sweep are distinguishable in the state model.
- [x] SAILS/MESO-SAILS supplemental scans are represented explicitly.
- [x] out-of-order chunks cannot regress displayed time.
- [x] stale live data is visibly marked before it reaches a configurable threshold.

## 1.2 Partial-volume rendering rules

Define deterministic rules for what is shown while a volume is in progress.

Requirements:

- render newly arriving radials immediately,
- preserve previous-volume data only where no replacement has arrived,
- visually distinguish old vs newly scanned regions when enabled,
- prevent mixed-time products from being silently presented as one instant,
- support a "strict current sweep only" mode,
- support a "continuous composite" mode,
- expose scan age in tenths of a second where the source supports it.

### Tests

Build synthetic fixtures for:

- complete sweep,
- radial gap,
- late radial,
- repeated radial,
- reordered chunks,
- volume reset,
- VCP change mid-session,
- SAILS insertion,
- source restart.

## 1.3 Multi-source Level II failover

Formalize provider priority and failover.

Possible source classes:

- primary low-latency chunk source,
- alternate chunk source where available,
- completed-volume source,
- archive fallback.

The UI should show:

- current provider,
- source age,
- failover reason,
- whether displayed data is partial or complete,
- whether the fallback changes temporal resolution.

### Acceptance criteria

- [x] provider loss does not freeze the display indefinitely.
- [x] fallback never silently changes from partial-scan live data to minutes-old completed data.
- [x] recovery to the preferred source does not cause time reversal.
- [x] all provider switches are logged in Analyst Mode.

## 1.4 VCP-aware scan progression

Strengthen VCP handling for:

- split cuts,
- repeated elevation angles,
- supplemental low-level scans,
- unusual maintenance/test sequences,
- dynamic VCP changes.

Add a scan progression panel in Analyst Mode showing:

- VCP,
- expected cuts,
- observed cuts,
- cut completion,
- expected next cut,
- elapsed and projected time.

## 1.5 Performance budget

Targets for desktop:

- radial ingestion to visible GPU update: p50 < 100 ms, p95 < 250 ms after receipt,
- interaction must remain responsive while decoding,
- no full-volume GPU re-upload when a subresource update is sufficient,
- no avoidable reallocations per chunk.

Browser and Android receive separate budgets.

---

# 2. P0 — Storm tracking and manual analysis UX

HookEcho already has SCIT tracks, warning motion, ProbSevere, rotation/TDS analysis, projected paths, and chase calculations. The missing maturity is interaction speed and editability.

## 2.1 Unified storm object model

Create one internal storm entity that can associate:

- SCIT cell,
- ProbSevere object,
- warning object,
- TDS detection,
- rotation/couplet detection,
- hail attributes,
- manual motion vector,
- automatic motion vector,
- user annotations,
- historical track,
- projected track.

Do not force every source to merge. Preserve provenance and confidence for each association.

## 2.2 Manual motion tool

Implement a direct-manipulation storm-motion tool:

- click storm origin,
- drag motion vector,
- editable speed and bearing,
- 15/30/45/60 minute markers,
- configurable interval,
- projected swath,
- left/right uncertainty width,
- optional expanding uncertainty cone.

Keyboard modifiers should allow:

- constrain bearing,
- change speed,
- adjust cone width,
- duplicate a track,
- clear the track.

## 2.3 Line / squall-line tracking

Support:

- polyline storm edge,
- projected future line,
- forward swath,
- individual city/marker impact times,
- minimum/maximum arrival range.

Useful for QLCS and long convective lines where one-point SCIT tracking is insufficient.

## 2.4 Impact / ETA engine

For any tracked object, compute impact against:

- saved markers,
- manually clicked points,
- cities/communities,
- roads/interstates if available,
- user-defined event zones.

Show:

- ETA,
- distance,
- closest approach,
- projected uncertainty,
- source of motion estimate.

Do not imply precision beyond the motion model.

## 2.5 Storm selection UX

Selecting a storm should optionally open a compact analysis card containing:

- current product sample,
- SCIT identifiers,
- max reflectivity,
- VIL / VILD,
- echo top,
- hail metrics,
- ProbSevere,
- rotation/TDS flags,
- warning association,
- motion,
- arrival times,
- source timestamps.

## 2.6 Acceptance criteria

- [x] a trained user can create a storm motion vector and 60-minute projection in under 5 seconds.
- [x] manual and automatic motion can coexist and be clearly distinguished.
- [x] editing never requires opening Settings.
- [ ] storm interactions work with mouse, pen, and touch.
- [x] projected ETAs are recomputed live while handles are dragged.

---

# 3. P0 — Long-duration resilience and source-failure testing

Professional maturity requires proving that the application behaves well after hours of operation.

## 3.1 Soak-test harness

Create a repeatable soak runner capable of:

- launching HookEcho headlessly or semi-headlessly,
- selecting a site/workspace,
- rotating products/tilts,
- enabling common layers,
- collecting memory/GPU/resource stats,
- injecting source failures,
- recording state transitions,
- failing on unrecovered feed or render stalls.

Profiles:

- 2-hour developer smoke soak,
- 12-hour severe-weather soak,
- 24-hour stability soak.

## 3.2 Failure injection

Support deterministic simulation of:

- HTTP timeout,
- truncated response,
- invalid gzip,
- invalid GRIB,
- malformed Level II chunk,
- missing chunk,
- delayed chunk,
- stale warning feed,
- GOES listing failure,
- MRMS archive miss,
- model index failure,
- DNS/network offline,
- WebGPU/device loss where practical.

## 3.3 Memory/resource leak detection

Track over time:

- process RSS,
- heap allocations where available,
- GPU buffer/texture counts,
- cached radar volumes,
- object cache growth,
- number of pending async tasks,
- open sockets/HTTP requests,
- UI window/card count.

Set acceptable growth thresholds.

## 3.4 Recovery rules

Every feed should declare:

- retry policy,
- backoff,
- stale threshold,
- last-good retention policy,
- whether stale data remains visible,
- when to hide data,
- user-facing error severity.

---

# 4. P1 — Rendering and visual polish

The objective is to make the application feel like one product rather than many technically correct layers.

## 4.1 Unified render-quality policy

Define per-platform quality tiers:

- Low
- Balanced
- High
- Analysis / Maximum

Each tier should control:

- radar antialiasing,
- 3D step budget,
- contour density,
- label density,
- line smoothing,
- particle count,
- satellite texture resolution,
- shadow/atmosphere effects if present.

Avoid independent hidden quality knobs scattered across features.

## 4.2 Temporal transitions

Implement consistent transition policy for:

- new radar sweep,
- volume change,
- MRMS frame,
- satellite frame,
- model lead,
- warning issuance/cancellation.

Provide options:

- instant,
- short crossfade,
- radar-specific progressive replacement.

Never interpolate categorical fields unless scientifically valid.

## 4.3 Label engine improvements

Build or strengthen one label-placement system shared by:

- cities,
- roads,
- storms,
- warnings,
- SCIT labels,
- user markers,
- fronts/highs/lows,
- placefile text,
- GIS labels.

Priorities:

- no important storm label hidden by a lower-priority city,
- stable placement between frames,
- density-aware decluttering,
- collision groups,
- leader lines when needed.

## 4.4 Camera behavior

Polish:

- zoom interpolation,
- fly-to easing,
- storm-follow mode,
- warning-follow mode,
- linked-pane camera behavior,
- 3D orbit controls,
- map/3D transition.

Avoid camera motion that changes analytical interpretation while scrubbing.

## 4.5 Basemap robustness

Address incomplete 3D basemap loading and zoom-out holes.

Requirements:

- visible tile coverage diagnostics in Analyst Mode,
- parent-tile fallback,
- retry failed tiles,
- no transparent holes when lower-resolution fallback exists,
- bounded retry behavior.

---

# 5. P1 — General GIS engine

WSV3 remains substantially stronger in generic GIS workflows. HookEcho should grow from weather overlays into a real operational GIS surface.

## 5.1 Supported formats

Phase 1:

- GeoJSON
- Shapefile
- KML/KMZ

Phase 2:

- GPX
- CSV with lat/lon mapping
- optional WFS/ArcGIS FeatureServer ingestion

## 5.2 Vector style model

Per layer:

- fill color,
- fill opacity,
- stroke color,
- stroke width,
- dash pattern,
- point icon,
- point radius,
- z-order,
- min/max zoom,
- visibility by attribute filter.

Allow rules based on attributes, for example:

- `TYPE == "TOR"`
- `MAG >= 2`
- categorical color map.

## 5.3 Label editor

Allow labels from attributes with:

- field selection,
- template syntax,
- font size,
- halo,
- priority,
- min/max zoom,
- collision participation.

## 5.4 Attribute inspector

Click any feature to inspect:

- geometry,
- properties,
- source,
- layer name.

Allow copying values and exporting filtered features.

## 5.5 Layer groups and saved GIS scenes

Users should be able to save:

- multiple GIS layers,
- styles,
- ordering,
- visibility,
- basemap,
- camera,
- weather layers,

as a named operational scene/workspace.

---

# 6. P1 — Broadcast / external output system

This should be treated as a distinct product surface rather than extending screenshot export.

## 6.1 Dedicated output window

Add an optional clean output surface separate from operator controls.

Capabilities:

- arbitrary resolution,
- 16:9 presets,
- 1080p / 1440p / 4K,
- borderless/fullscreen,
- monitor selection,
- independent UI visibility.

## 6.2 Deterministic frame clock

Exports and external output should use a deterministic render clock.

Requirements:

- capture does not depend on UI event-loop timing,
- animation advances on explicit timestamps,
- fixed-FPS export,
- radar/model frames selected deterministically.

## 6.3 Scene presets

Save scene state:

- map camera,
- visible layers,
- legend state,
- title,
- logo,
- time display,
- annotation visibility,
- output resolution.

Add hotkeys to switch scenes.

## 6.4 Graphics overlays

Support operator-configurable:

- title strap,
- timestamp,
- source attribution,
- logo,
- warning banner,
- storm name,
- custom text.

## 6.5 OBS integration

At minimum:

- clean window capture,
- transparent-background overlay mode where technically feasible,
- stable window identity/title,
- optional NDI/Spout/Syphon exploration after core output is stable.

## 6.6 Export

Strengthen:

- screenshot,
- GIF,
- MP4/H.264 or platform-available codec,
- frame sequence.

Acceptance:

- [x] no dropped or duplicated logical weather frames in fixed-FPS export.
- [x] exported timestamp matches the rendered analysis time.
- [ ] output can run for 30 minutes without drift.

---

# 7. P1 — app.rs decomposition and application architecture

The current central application file is too large and increases regression coupling.

This refactor should be incremental and behavior-preserving.

## 7.1 Define domain boundaries

Move state and behavior into modules/controllers for:

- radar session,
- timeline/session time,
- pane layout,
- layer state,
- storm analysis,
- alerts,
- archive/event playback,
- models,
- satellite,
- GIS,
- 3D,
- chase/GPS,
- export/broadcast,
- settings,
- persistence,
- command/action registry.

## 7.2 App shell responsibility

The top-level app should primarily:

- own global services,
- route events,
- manage top-level lifecycle,
- coordinate domain controllers,
- call rendering/UI surfaces.

It should not directly implement every feature.

## 7.3 Event/message model

Introduce explicit typed events for cross-domain communication rather than ad hoc shared-state mutation.

Examples:

- AnalysisTimeChanged
- RadarFrameArrived
- RadarSourceChanged
- StormSelected
- WarningSelected
- WorkspaceLoaded
- GpuDeviceLost
- CachePressure
- NetworkStateChanged

## 7.4 Refactor guardrails

For every extraction:

- add tests first where practical,
- preserve serialized settings compatibility,
- preserve command IDs,
- preserve workspace formats,
- avoid simultaneous behavior redesign unless required.

## 7.5 Metrics

Track:

- size of top-level app module,
- number of direct dependencies,
- number of mutable global fields,
- compile time impact,
- regression count.

Target: no single file should remain a de facto monolith for unrelated product domains.

---

# 8. P1 — Product-level historical regression corpus

HookEcho's archive capability provides an unusual advantage: major past events can become deterministic integration tests.

## 8.1 Build a canonical event corpus

Include representative cases such as:

- 2011 Tuscaloosa / Birmingham,
- 2011 Joplin,
- 2013 Moore,
- 2013 El Reno,
- 2021 Mayfield,
- 2022 Hurricane Ian,
- one winter storm,
- one tropical non-landfall case,
- one widespread QLCS,
- one low-topped rotation case,
- one clear-air / anomalous propagation volume.

Use legally/publicly available archive references rather than committing huge raw files where avoidable.

## 8.2 Golden scientific checks

For known scan/time points validate:

- tilt inventory,
- radial/gate geometry,
- selected sample values,
- dealiased velocity expectations,
- VIL,
- echo top,
- MESH/POSH,
- TDS flag,
- azimuthal shear extrema,
- cross-section values,
- 3D volume bounds.

Use tolerances where algorithms are continuous.

## 8.3 Golden visual checks

Generate deterministic reference renders for:

- reflectivity,
- velocity,
- CC,
- four-pane layout,
- cross-section,
- 3D observed mode,
- 3D smooth mode,
- warnings over radar,
- storm tracks.

Use perceptual/image-diff thresholds rather than requiring byte-identical GPU output.

## 8.4 Historical warning verification checks

For selected events ensure:

- archived warnings load at the correct instant,
- issue/expire transitions are correct,
- LSR matching is stable,
- verification metrics remain unchanged unless intentionally modified.

---

# 9. P1 — Data/provenance completeness

HookEcho already has a strong provenance system. Finish applying it consistently.

## 9.1 Every field gets a DataStamp-equivalent

Required:

- source/provider,
- product,
- valid time,
- issue/run time where applicable,
- receive time where known,
- observed / forecast / derived classification,
- units,
- quality / missing-data behavior,
- original/native units if transformed.

## 9.2 Source health panel

In Analyst Mode provide a compact status page for:

- Level II,
- Level III,
- warnings,
- MRMS,
- GOES,
- models,
- METAR,
- ProbSevere,
- auxiliary feeds.

For each:

- last successful fetch,
- last error,
- response age,
- retry state,
- cache hit/miss,
- provider.

## 9.3 Stale-data policy

Use consistent visual semantics:

- fresh,
- aging,
- stale,
- unavailable.

Avoid each layer inventing its own warning style.

---

# 10. P1 — Timeline and synchronization finish

The time-alignment engine is already one of HookEcho's strengths. The next objective is uniformity.

## 10.1 One analysis cursor

All panes/layers that opt into linked analysis should resolve from the same retained analysis time.

## 10.2 Explicit source offsets

Show signed source-minus-analysis-time offsets where useful.

For example:

- Radar +00:00
- GOES -00:42
- MRMS +01:15
- HRRR valid +00:00, run 18Z

## 10.3 Archive/model synchronization

Finish archive-seeking behavior for model fields where data access makes it practical.

Do not substitute current model data while viewing a historical radar event.

## 10.4 Timeline themes

Support visual timeline styles inspired by:

- HookEcho native,
- WSV3-like,
- GRLevel-like,
- compact mobile.

These should be visual skins on one timeline engine, not separate logic implementations.

---

# 11. P2 — 3D analysis expansion

The new 3D system is already technically strong. Further work should focus on analysis, clarity, and performance.

## 11.1 Storm-centered 3D mode

Add a mode that centers volume bounds around a selected storm rather than the whole radar.

Benefits:

- higher effective voxel resolution,
- faster raymarch,
- easier vertical analysis.

## 11.2 Isosurfaces

Extend:

- configurable dBZ thresholds,
- multiple shells,
- velocity pair thresholds,
- MESH/hail surfaces where scientifically meaningful,
- opacity presets,
- clipping planes.

## 11.3 Vertical reference surfaces

Continue integrating environmental context:

- 0 C,
- -10 C,
- -20 C,
- optional tropopause / EL where available,
- terrain surface.

Clearly label source/run/time.

## 11.4 3D crosshair / ruler

Allow measurement of:

- AGL/MSL height,
- horizontal range,
- slant range,
- approximate beam height,
- value at sample.

## 11.5 3D temporal playback

Ensure stable camera and deterministic geometry while stepping through volumes.

Optional future feature:

- storm-following 3D camera that translates with selected cell motion.

---

# 12. P2 — Workspace and analyst workflow improvements

## 12.1 Professional workspace presets

Ship curated presets such as:

### Tornado analysis
- Z / SRV / CC / ZDR
- linked camera/time
- SCIT + ProbSevere + warnings

### Hail analysis
- Z / ZDR / CC / MESH
- freezing-level references

### National severe
- MRMS
- warnings
- SPC outlooks
- GOES/GLM

### Chase
- reflectivity + SRV
- GPS
- warning motion
- saved markers

### Tropical
- radar
- NHC track/wind field
- satellite
- surface obs

## 12.2 Workspace validation

On load:

- report unavailable data sources,
- map missing radar sites gracefully,
- preserve unknown future fields,
- avoid silently dropping configuration.

---

# 13. P2 — Input, accessibility, and operator efficiency

## 13.1 Command consistency

Every significant action should have:

- stable command ID,
- command-palette entry,
- optional hotkey,
- discoverable UI location.

## 13.2 Pointer consistency

Normalize:

- click,
- double-click,
- right-click,
- long-press,
- drag,
- multi-touch gestures.

Prevent map gestures from conflicting with edit handles.

## 13.3 Touchscreen stability

Specifically fix multi-finger map cases where the camera can "fly away".

Add gesture-state tests for:

- pinch + rotate,
- second pointer added mid-drag,
- pointer cancel,
- browser touch emulation,
- Android native events.

## 13.4 Accessibility

Improve:

- keyboard-only navigation,
- focus order,
- scalable text,
- high-contrast mode,
- colorblind-safe warning indicators,
- non-color-only state indication.

---

# 14. P2 — Performance and observability

## 14.1 Internal telemetry without user tracking

Analyst Mode should expose local-only metrics such as:

- frame time,
- radar decode time,
- GPU upload time,
- cache hit rate,
- HTTP latency,
- chunk receive lag,
- sweep completion lag,
- memory,
- object counts,
- dropped frame count.

No external telemetry is required.

## 14.2 Performance regression tests

Automate benchmarks for:

- Level II decode,
- partial sweep append,
- full volume upload,
- 4-pane rendering,
- MRMS field upload,
- GOES load/decode,
- 3D grid build,
- cross-section generation.

Store benchmark history in CI artifacts.

---

# 15. P2 — Documentation and operational transparency

## 15.1 "How current is this?" documentation

Document expected cadence/latency for every major feed and distinguish:

- provider latency,
- network latency,
- decode latency,
- display latency.

## 15.2 Scientific limitations

For derived products document:

- assumptions,
- thresholds,
- environmental dependencies,
- known failure modes,
- whether values reproduce official Level III algorithms exactly or approximately.

## 15.3 Analyst troubleshooting guide

Add scenario-based troubleshooting:

- radar stopped updating,
- one tilt missing,
- velocity looks folded,
- satellite older than radar,
- model time mismatch,
- warning polygon not appearing,
- 3D missing part of storm,
- basemap tiles missing.

---

# 16. Execution sequence

Recommended implementation order:

## Phase 1 — Operational radar trust
1. live radar state model,
2. partial-volume rules,
3. source failover,
4. VCP/SAILS handling,
5. live latency UI,
6. chunk/scan regression tests.

## Phase 2 — Severe-weather operator workflow
1. unified storm object,
2. manual motion tool,
3. line tracking,
4. ETA/impact engine,
5. storm analysis card,
6. interaction optimization.

## Phase 3 — Reliability
1. soak runner,
2. failure injection,
3. resource metrics,
4. recovery policies,
5. nightly long-duration CI where feasible.

## Phase 4 — Architecture
1. define app domain boundaries,
2. extract radar/timeline/layer controllers,
3. extract storm/alert/GIS/export domains,
4. formalize typed events,
5. reduce app.rs responsibility.

## Phase 5 — Visual maturity
1. shared render-quality policy,
2. label placement,
3. temporal transitions,
4. camera polish,
5. basemap fallback.

## Phase 6 — Professional workstation expansion
1. GIS ingestion/style system,
2. dedicated broadcast output,
3. deterministic capture,
4. scene presets,
5. OBS-oriented workflows.

## Phase 7 — Regression corpus
1. canonical event catalog,
2. scientific goldens,
3. visual goldens,
4. warning verification goldens,
5. CI integration.

## Phase 8 — 3D / analyst refinements
1. storm-centered volume,
2. clipping/measurement,
3. environment surfaces,
4. temporal 3D playback,
5. advanced analysis presets.

---

# 17. Definition of "WSV3-competitive"

HookEcho should be considered operationally competitive when all of the following are true:

- live Level II can be watched sweep-by-sweep for hours without temporal ambiguity,
- source latency/failover is always visible and recoverable,
- manual storm tracking is as fast as automatic analysis,
- map/labels/camera behavior remains stable during continuous timeline playback,
- GIS files can be loaded and styled without writing code,
- a clean external output can be driven separately from the operator UI,
- historical cases can be reproduced deterministically,
- regression tests protect the radar science and visual output,
- the application survives long severe-weather sessions without resource degradation,
- adding a feature no longer requires expanding a monolithic top-level application module.

The remaining competitive gap should then be mostly a matter of accumulated field use and incremental polish rather than fundamental capability.

---

# 18. Work that should *not* be prioritized yet

Until the P0/P1 maturity work is complete, avoid spending major effort on:

- adding many more niche weather layers,
- adding another UI theme engine,
- adding decorative 3D effects without analytical value,
- supporting additional export codecs before deterministic capture is correct,
- introducing cloud accounts/backends that are not required for reliability,
- expanding platform count beyond currently supported targets,
- adding AI-generated meteorological conclusions into the core analysis workflow.

The objective of ROADMAP_2 is **depth, reliability, operator speed, and professional finish**.
