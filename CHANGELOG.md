# Changelog

Notable changes per release, newest first. Every tagged release's body on GitHub
is this file's matching section, extracted by `.github/workflows/release.yml` —
so **write the section before pushing the tag**, or the release job fails.

The rolling `latest` release tracks `main` and is not listed here.

## Unreleased

### Added: warn when a hovered cross-section point isn't real beam coverage

ROADMAP_NEW C3's last open item. `wxdata::xsection::sample_profile` fills a thin band just past
each tilt's real beam (up to 1.5 km) by holding its nearest sample over, so the panel doesn't cut
off with a hard edge at the true coverage boundary — a deliberate, reasonable rendering choice, but
one that reads as a real sample at a glance, with nothing distinguishing it from actual data
underneath the cursor.

`CrossSection` now carries a parallel `beam_covered` grid alongside `dbz`, and hovering the panel
shows a tooltip with the sampled position, its value, and — when the cell is filled only by that
held-over extension — an explicit warning that no beam actually passes through the point. A true
gap (no value at all) still just says "No beam coverage here", as before.

Verified live: hovering a KTLX cross-section showed "11.2 dBZ / \u{26a0} outside beam coverage —
nearest tilt held over across the gap, not an actual sample here" in the held-over band, and "No
beam coverage here" (no dBZ, no warning icon) above it where `dbz` is genuinely `None`.

### Fixed: the archive date picker could land on the wrong time, or seemingly nowhere

Reported live: loading an archived day worked from a `hookecho://goto/…` URL but was unreliable
from the calendar. The day-step carets, the typed date field, and the calendar picker all just
wrote `t.date = d` directly — none of them cleared the old day's stale `frames`/`playhead`, or told
the timeline what moment on the new day to land on. A URL/permalink jump never had this problem
because it already went through `Timeline::seek_to_valid_time`, which does both.

Until the new day's listing happened to land on an in-range index by coincidence, the pane kept
showing the *previous* day's volume; once it landed, the playhead's old numeric index carried over
unchanged, showing whatever arbitrary moment that index happened to mean on the new day — from a
few minutes off to a different day with fewer volumes leaving it clamped to the wrong end of the
list entirely. The wrong-time cases looked like a random small mismatch; a day with far fewer
frames than the index pointed past looked like the load had silently failed.

All three now go through a shared `seek_to_day` (`app/chrome/scrubber.rs`), which clears the stale
axis and lands near the *same time of day* that was showing before the jump (noon if nothing was
on screen yet) via `seek_to_valid_time`, or returns to the live head via `follow_day` when the jump
lands on today. Verified live: picking July 16 from the calendar while viewing 15:35Z landed the
archived KTLX volume at 16:58Z on July 16 (real storm coverage, the archive's own VCP/tilt list);
stepping forward a day with the caret preserved 16:59Z on July 17; typing today's date back in
returned to the live head with the LIVE badge and current VCP restored.

### Added: hide the WSV3 ribbon for a full-window map view

Requested directly: a way to get the top ribbon and its docked colour scale out of the way
entirely, rather than only ever reserving `RIBBON_H + COLORBAR_H` at the top of the window. A new
small button — floating at the top-center edge, styled like a window's own resize grip rather than
a call-to-action, since it's a permanent fixture — hides the ribbon outright; the same spot brings
it back. Bound to `T` and reachable from Ctrl+K ("Top bar") like every other panel toggle in the
app. Collapsing doesn't just visually cover the ribbon: the panel itself is never created that
frame, so the map's own available space grows to fill what the ribbon would have reserved, not
just paint over it.

The corner was chosen to avoid the two spots already claimed in that strip: the timestamp pill's
top-left (`wsv3_timestamp`) and the OS window buttons' top-right (`window_frame`). Runtime state,
not a setting (`ribbon_collapsed`) — same convention as the Layers panel's own open/closed flag —
so a collapsed ribbon doesn't stay collapsed the next time the app opens. Verified live: clicking
the button, and separately pressing `T`, hid the ribbon and grew the map into its space in both
directions; toggling back restored the ribbon exactly as it was.

### Added: slab thickness for the 3D vertical clip plane

ROADMAP_NEW H4's arbitrary vertical plane could only ever cut the volume in half — keep everything
on the side the bearing points toward, discard the rest. "Slab thickness" was the one variant of
this feature left unbuilt: two parallel planes with a gap, keeping only a band that straddles the
plane instead. `VerticalPlane` gained an optional `thickness` (same fraction-of-box convention as
`offset`); the raymarch shader now keeps a sample only within that half-width of the plane when set
(`plane_slab` uniform), falling back to the old cut-one-side-away behavior when it's `None`. A new
"Slab" checkbox and thickness slider sit under both the standalone 3D window's and the main map's
Vertical plane controls (`plane_controls`, shared by both as before); the `--headless-3d --plane
BEARING,OFFSET,THICKNESS` CLI flag takes the same optional third component.

Verified on real GPU hardware via `--headless-3d`, mirroring the original plane feature's own
sweep-and-count method: at KTLX, thickness 0.02 kept 9,181 echo pixels, 0.15 kept 35,682, and 1.0
(a slab as wide as the whole box) kept 167,468 — identical, pixel for pixel, to the unclipped
baseline (167,468) and to rendering with no thickness set at all. Monotonic growth with thickness,
converging exactly to "no clip at all" at the box's full width, is exactly the behavior a slab
should have.

### Added: the 3D map's vertical clip plane draws its ground track on the 2D map

ROADMAP_NEW H4 called out that the arbitrary vertical clip plane (bearing + offset, from the
earlier "3D map" Slice controls) was visible only from inside the 3D view itself — nothing tied
that cut back to geographic context on the flat map underneath it. Enabling "Vertical plane" now
draws its ground track as a line across the 2D map pane, the same way the cross-section tool draws
its own two-point line, in a distinct violet so the two are never confused.

The line is the plane's actual cut, not an approximation: `render3d::plane_ground_track` derives it
from the same bearing/offset math `plane_uniform` feeds the raymarch shader, anchored at the radar
site and scaled by the same `half_km` box extent the Smooth raymarch itself uses, so the drawn line
and the rendered cut agree by construction. Only shown for the Smooth representations — Observed
raymarches real gate instances with no box for a plane to cut into. Verified live: toggling
"Vertical plane" drew a line straight through KTLX; dragging Bearing to 310° rotated the line in
place around the site, and dragging Offset shifted it off to the side, both matching the 3D view's
own cut.

### Added: a Storage settings tab for the web build

The Settings window's Storage tab — cache sizes and their Clear buttons — was `#[cfg(not(wasm32))]`
outright, so the web build had no Storage tab at all, even though it has been quietly filling
IndexedDB since the archive-volume auto-cache shipped (`webcache.rs`'s `auto_cached_volume`): a
chaser on the web build had no way to see how much space that cache held, let alone clear it,
short of the browser's own site-data settings.

The tab is now unconditional. On the web build it shows two rows: "Auto-cache" (the automatic
archived-volume cache, with a real Clear button — the one cache that had no UI of its own anywhere
in the app) and "Offline packs" (saved loops, read-only here since they already have per-pack
delete in the timeline's own archive menu). `storage::human` (byte formatting) moved out from under
the module's native-only gate to be shared by both; the rest of `storage.rs` (the filesystem walk
native uses for its own cache rows) stays native-only, unchanged. Verified live: opening Storage
after some radar scrubbing showed "143.8 MB in IndexedDB" with "Auto-cache 143.8 MB — 26 volumes";
Clear brought both down to "0 B" immediately, no reload needed.

### Added: favorites — a pin/star for layers

ROADMAP_NEW D3 called out "favorites — no pin/star affordance exists" as the other half of the
Recent-products work; this closes it. Every layer row in the Layers panel (search results,
category-browse, RECENT) now has a star at its far right, independent of the row's own click
target — starring a layer never toggles it on/off, and toggling it on/off never stars it. Starred
layers show, most-recently-added order, in a new "FAVORITES" section on the landing screen, placed
above "RECENT": a deliberate pick belongs ahead of an incidental one. Unlike Recent, Favorites has
no cap and no "move to front" on re-click — it's a curated list, not a recency trail. Persisted in
settings (`favorite_layers`). Verified live: starring "Rotation tracks" moved it into a "FAVORITES"
heading with the star lit in the accent color and left "Active" count unchanged; unstarring it
returned the row to "RECENT" with the star back to its default weak-text color, again without
touching the active-layers count.

### Fixed: the Gate Inspector always reported the 2D-selected tilt in the 3D map view

Reported live: in the pitched "Observed" 3D view, which draws several tilts stacked at once,
clicking any of them with the Gate Inspector always reported the same elevation angle — whatever
tilt happened to be selected back in the flat 2D picker — no matter which one was actually under
the cursor. The click itself was never 3D: it only ever intersected the ground plane at `z = 0`,
then sampled the pane's `v.tilt` at whatever ground point that landed on, discarding the height the
user actually clicked at entirely.

A real click now casts a ray against each tilt's own beam-height surface — the exact geometry
`radar_observed.wgsl`'s `beam_world` places every gate on — and reports whichever surface is
nearest the camera along that ray, the same tilt a z-buffered render would actually show there
(`render3d::pick_observed_tilt`). A steeper tilt legitimately does occlude a farther, shallower one
under the same line of sight from a downward-looking camera — that's real geometry, not a bug, and
the picker matches it rather than fighting it. Verified live against a real volume (reported
elevation tracked the click, distinct from the 2D tilt picker's own selection) and with a
round-trip unit test that places a known point on a known tilt's surface and confirms the picker
recovers it.

### Added: the live-sweep indicator also lives on the tilt pill itself now

Reported live: the scrubber badge's ring (below) sits on a pill that's easy to miss entirely, and
doesn't say *which* WSV3 tilt pill the live chunk stream is actually updating. A thin strip now
fills in from the bottom edge of that specific tilt pill in the ribbon — filled by how far the
current sweep has scanned, pulsing gently while the stream is open — independent of whichever tilt
is selected for viewing (the pill's own highlight), since the live scan keeps moving through the
VCP regardless of what's on screen. Same "Live sweep indicator" setting turns both off together.

### Added: an animated live-sweep indicator on the scrubber's Live badge

Next to the "Live" badge, while a chunk stream is actually updating the pane: a small ring showing
how far the current volume has scanned (a static arc) and, unless motion is reduced, a segment that
spins continuously so "still connected, nothing stalled" is visible without reading the badge's
hover text — plus a slim tilt-progress bar underneath naming the exact tilt and chunk on hover.
Both read off `wxdata::live::ScanProgress`, which the chunk-stream code already computed and threw
away. New "Live sweep indicator" toggle in Map settings (on by default) turns it off entirely for
anyone who'd rather the badge stay a plain dot.

### Fixed: the devlog admin panel had no path to production, and no lock on the door

Landed alongside the developer log below, this closes the gap noticed trying to actually reach it
on a live deploy: `Dockerfile.coolify` ran only the main `--serve` process, with no way to reach
`--devlog-serve` at all. A new `scripts/docker-entrypoint.sh` starts both in one container —
`--devlog-serve` in the background, `--serve` `exec`'d into PID 1 so Docker's stop signal still
reaches the process actually serving traffic — and `HOOKECHO_DEVLOG` defaults on for that `--serve`
process, so a deployed instance ships its own logs to the co-located panel with no extra
configuration. Port 8884 is now `EXPOSE`d and mapped to host `:8885` in
`docker-compose.coolify.yml`.

Since this is the change that makes the panel reachable from outside a single trusted machine for
the first time, it also gained the lock `--serve` already had: `--devlog-token` (or
`HOOKECHO_DEVLOG_TOKEN`) gates every route but the CORS preflight behind a bearer token, checked
via header or `?token=`, using the same constant-time comparison `--serve-token` does. Unset —
still the default — the panel is exactly as open as before.

### Added: a developer log — every `log::` call, filterable and searchable, on its own admin panel

A live-sweep stall, a TDS/TVS false trip, or an error only a browser instance somewhere ever hit
used to mean asking whoever saw it to paste a console — if the log line even existed at all.
`hookecho --devlog-serve [PORT]` (default 8884) now starts a small standalone admin panel, in the
spirit of `--serve`'s own dashboard rather than a window bolted onto the app: filter by severity
(warn and worse, say), by category (`wxdata::tds`, `wxdata::rotation`, `hookecho::live_sweep`, or
any other module path — nothing to register, `record.target()` already carries it), by which
running instance said it, and free-text search across the message, with a live tail that follows
new lines as they arrive.

Every native and web launch can ship its own buffered log lines there: `HOOKECHO_DEVLOG=1` (or a
URL, for a viewer on another machine) on the desktop app or `--serve`, `?devlog=<url>` in the page
URL for the web build — including a deployed build on a different origin than the admin panel,
which is the ordinary case for the web build and needed its own CORS preflight handling to work at
all. Off by default, like every network-facing thing in this app: nothing opens a socket or reads
an env var for this unless asked to.

Two subsystems that ran silently before now actually say something: TDS and TVS/rotation detection
log a debug line every volume they scan and an info line on the rising edge of a real detection
(the same rising-edge alert that already fires the banner and chime), and the live-sweep pipeline
logs each arriving chunk and completed volume load. All of it reaches the terminal or browser
console exactly as before — capture wraps the existing logger rather than replacing it, so nothing
here changes what a call site already said, only where else it can end up.

### Fixed: the Layers panel's search results could render nothing at all

- Reported live: typing a query that matched more than about one entry showed the search box and
  the Browse/Active row, then nothing below them — not even a "No matches" placeholder. The
  floating panel's outer `ScrollArea` (and, on the touch rail, the `Area` wrapping it) used egui's
  default auto-shrinking behavior: with no forced height, it claims only as much as its content
  needs, up to its cap. Since the window itself has no forced height either, it auto-sizes around
  whatever the scroll area claims — so a frame that rendered short (before content settled, or
  because the chrome above the results ate most of a modest starting height) shrank the window,
  leaving even less room the next frame, and so on. The panel could settle at a stable size too
  short for even one 32px result row, with no way back short of a reload. Both scroll areas now
  pin to their computed height budget with `.auto_shrink([false, false])`, so they claim it
  unconditionally instead of bidding on it based on last frame's content — breaking the loop.
  Verified live: a broad query ("m", "reflectivity") now shows its ranked matches immediately.

### Added: suite-wide search commands, and 3D now tracks a live sweep chunk by chunk

Picking up mid-flight work: scoped search prefixes (`station KTLX`, `site `, `tool `), typed
timeline commands (`time 21:30Z`, `at 2026-09-14 03:00`, `goto live`), and a search-context label
("Radar station · KTLX", "Timeline · Seek timeline to...") on each result row so a broad query's
results are legible at a glance. The ribbon's new "Search all" pill (and Ctrl+K) opens the same
search always scoped to everything, regardless of whether the panel was last left on the Active
list or inside a category.

The bigger piece: both 3D representations — Observed and the resampled Smooth/Debris volume — now
track a live sweep as it fills in, not just when a tilt or the whole volume completes. Every
accepted merged chunk advances a per-pane revision counter that is part of each 3D upload's cache
key, so a new wedge inside a tilt already on screen, or a repeated SAILS/MRLE low-level cut,
invalidates and rebuilds 3D even though the volume's name and tilt count haven't changed — this is
the "level 2 live sweep, shown in 3D as it comes in" work suggestions.md and ROADMAP_NEW B2 asked
for on the observed-3D side.

Verified live end to end: pointed a pane at KMPX during a real severe-weather volume (VCP 215),
opened 3D Observed, and watched the "Live" badge and scrubber advance on their own while the
volume kept streaming — the 3D gates are the same real Level II volume the 2D plan view is
watching, not a separate poll.

Also landed alongside it, since a chunk-by-chunk 3D rebuild made the GPU cost of the naive
approach immediately visible: the observed-gate 3D renderer now **retains its GPU instance buffer,
LUT texture and bind group across live revisions** instead of tearing all three down and
reallocating them on every chunk. The instance buffer grows geometrically (reserved to the next
power of two), so ordinary chunk-to-chunk growth within a tilt reuses the same allocation, and only
a genuine capacity overflow triggers a real rebuild. Verified on a real GPU: a new headless render
test grows a wedge across three uploads and confirms the retained buffer's stale tail never leaks
onto screen once a later, smaller upload shrinks the drawn instance count — the kind of bug that
lives entirely in what the draw call reads off the GPU buffer and that no CPU-side test can catch.
The remaining item in this area is a genuinely incremental upload — writing only the changed
radial range instead of the whole buffer each time — which is now a cost problem rather than a
correctness one.

**A real bug found while verifying this, not fixed here:** the floating Layers panel's search
results render *nothing* — not even a "No matches" placeholder — for any query broad enough to
match more than roughly one entry, on the panel's normal (non-maximized) size. Diagnosed with
temporary logging (removed before this commit): the search and ranking logic is completely
correct — a one-character query against the full ~420-entry registry correctly narrows to 165
matches — but by the point the result list is reached, the surrounding layout reports **zero**
remaining height to draw them in, and egui does not fall back to scrolling or clipping-with-a-
visible-partial-row; the rows are simply never painted. The bug is in how the floating card budgets
its own vertical space among the product controls, the search box, and the results list above it,
not in search itself. The radar site picker (a separate dialog, unaffected) still works and was
used for this session's own live verification. Left as a clearly-flagged follow-up rather than
patched under this pass, since fixing it properly means revisiting that panel's height budget as
a whole rather than one more special case squeezed into it.

### Added: recent products, an MRMS live-feed contract test, and a roadmap correction

While scoping the next slice of ROADMAP_NEW's Phase A/D, found that the "generic field/product
registry" and "MRMS product catalog" sections were marked almost entirely unchecked despite both
being substantially built already — `wxdata::field` (`FieldDescriptor`, `DataStamp`,
`GridProvenance`), `wxdata::mrms::catalog`, and `ui::data_inspector` all predate this pass and
were simply never reflected in the roadmap's checkboxes. Corrected there rather than duplicated;
see ROADMAP_NEW.md's A1 and D1-D3 for the honest accounting of what exists and what's still open.

- **New: "Recent" products.** The Layers panel's Browse landing screen gained a "RECENT" section
  above the category grid, listing the last few products you've turned on, most-recent-first. Only
  a genuine click counts — loading a saved workspace or the HRRR sub-mode/model-compare bookkeeping
  never touches it, so restoring a workspace doesn't masquerade as something you just picked.
  Persisted in settings (`recent_layers`), capped at 6. Verified live: toggling Hail size (MESH)
  and then Rotation tracks put "Rotation tracks" at the top of "RECENT" on the landing screen, and
  it survived a full page reload. Favorites (a pin/star) remain unbuilt — this is the "recent" half
  of that roadmap item only.
- **New: an MRMS catalog feed-contract test.** D1's own rule was "do not blindly list a product
  unless a feed contract test confirms it exists" — nothing enforced that. A network-gated test
  now asks the live MRMS S3 bucket for every path every catalog product's fetch mapping can
  produce, including every published accumulation/averaging window, not just the default. Passing
  today: 21/21 paths confirmed live across the 11 cataloged products.

### Added: beam top/bottom, beam width, and a cross-section beam-rise overlay

`suggestions.md` §3.1's ask was "show beam height, not just elevation angle" — the gate inspector
already had beam-centre height, but not the width or vertical extent that turn "here's the beam
centre" into "here's what this gate's reading actually covers."

- New `wxdata::beam_geometry::beam_extent`: a beam's top/bottom edges at one slant range, from the
  standard analyst convention `elevation ± half the antenna's half-power beamwidth`, alongside
  `horizontal_beam_width_km` for the cross-beam extent. Both were previously private, differently-
  typed copies inside `suitability.rs`'s ranking; hoisted so the ranking, the gate inspector, and
  the new cross-section overlay below all read one shared constant.
- The gate inspector gained "Beam top/bottom" and "Beam width" rows next to the existing beam
  height. Verified live: at 116 km range on a 0.48° tilt, beam height 5846 ft sits between top/
  bottom 8931/2761 ft, and beam width (1.88 km) visibly grows with range.
- The cross-section window gained an optional **beam-rise overlay**: one color-coded curve per
  tilt in the volume, sampled at the exact radar-relative ground range each panel column already
  uses for its reflectivity gate, so the curve and the data underneath are geometrically
  consistent by construction. A repeated SAILS/MRLE low cut draws one line, not one per repeat.
  Toggleable independently of rebuilding the panel — the geometry is already part of the built
  `CrossSection` regardless of whether it's drawn. Verified live on real KTLX data: eight tilts'
  curves climbing correctly across a 120 km cut through actual echo, and unchecking the box
  removing only the lines.
- Found and fixed along the way: the checkbox's first placement crammed a sixth control into a row
  already holding the length label, three moment buttons and two CSV buttons — fine at the
  window's default width, but overlapping illegibly once the window was as narrow as this pane's
  own 800×600 viewport made it. Caught by looking at the live render, not by a unit test (a text
  layout collision has no natural assertion); given its own row instead.
- **Also discovered, not built this pass:** `crate::elevation`'s terrain-vs-beam blockage raster
  (`blockage_image`/`BeamSite`, the "Blockage" chase-mode overlay) already fully implements
  ROADMAP_NEW C3's "terrain blockage estimate" item — the roadmap simply hadn't been updated to
  say so. Corrected there rather than duplicated here.
- Still open from the same roadmap section: a gridded "lowest usable beam" map, a coverage
  comparison between two sites, 3D beam-rise (only the 2D cross-section has the overlay), and a
  warning when a sampled feature sits outside the beam's modeled coverage.

### Changed: CC in 3D is an anomaly view now, not a denoise floor

The 3D controls were built around "high is interesting", which is true of every radar moment
except the one where it matters most. Correlation coefficient's ordinary meteorological scatter —
rain, snow, the entire storm — sits at CC ≈ 0.97–1.00, and the *interesting* returns are the low
ones: lofted debris, ground clutter, biological targets, the mixed-phase edges of a hail core. A
denoise floor applied to CC therefore hid precisely what someone opens a CC volume to find and
kept the uniform high-CC rain that is never the answer. It was backwards.

- **CC anomaly** replaces that floor. Opacity is now a continuous function of how anomalous the
  CC is: background scatter fades almost out, and the lower the CC the more solid the voxel.
  Conceptually — with the defaults, which are a starting point and not meteorological constants —
  above 0.97 is nearly transparent, 0.95–0.97 faint, 0.90–0.95 visible, 0.80–0.90 strong, and
  below 0.80 very strong.
- **The thresholds are the user's.** Two edges ("Clear above", "Solid below") and a "Faintest"
  control for how much background survives. CC backgrounds move with the radar, the range, the
  precipitation type and the season, so nothing here is presented as a fixed number. The tiers
  above are not coded as bands either — a smoothstep between the two edges reproduces that
  progression continuously, which is fewer knobs and avoids painting hard contour shells onto a
  volume.
- **Debris mode gets it too.** That mode raymarches a CC volume with its indices inverted, so its
  ramp has to run the opposite way round to mean the same thing; the shared uniform carries the
  two endpoints rather than a direction flag, and a test asserts both paths produce the same
  opacity for the same CC. Debris previously had no opacity control of its own at all.
- "Faintest" defaults to 0.05 rather than 0: "nearly transparent" and "deleted" are different
  claims, and a trace of the surrounding precipitation is what lets a debris ball read as
  embedded in a storm rather than floating in empty space. Set it to 0 to cut the background away.
- Verified on a real GPU, and the test earned its keep immediately: rendering two wedges that
  differ only in CC showed the high-CC one fading to under a third of its unramped brightness
  while the low-CC one held above 85%, and writing it surfaced a crash that every CPU-side check
  had missed — the observed bind-group layout still pinned the old 80-byte uniform size, so the
  first 3D draw would have failed pipeline validation outright. That size is now derived from the
  uniform's own type so the two cannot drift again. Then confirmed in the running app on live
  KTLX, in both Observed (CC) and Debris.
- The pane's 2D CC threshold is untouched and still lives under "Product settings"; it simply no
  longer applies to the 3D view, because a floor and an anomaly ramp disagree about which end of
  the CC scale is worth showing.

### Added: models are definitions now, and the catalogue is checked against the real feeds

Phase F's acceptance criterion is that adding a model with an already-supported GRIB format
should be a definition plus field mappings, not a new renderer. It wasn't: the six NWP sources
the app already fetches were distinguished by `match` arms scattered across four files, and the
GRIB variable/level strings were literals inside the UI's own layer dispatch — duplicated
independently in `app.rs`, `fielddiff.rs`, `severe.rs` and `headless.rs`. "Does the NBM publish
updraft helicity?" had no answer short of firing a request and reading the error.

- New `wxdata::model`: a `ModelDef` row per model (id, label, cycle cadence, lead schedule, grid
  spacing, domain, typical posting latency, ensemble role) and a `ModelField` catalogue that maps
  a field *by meaning* to each model's GRIB spelling — or to `None`, which is a real answer the
  caller can grey out rather than a gap.
- **The table is verified against the live feeds, not against documentation.** A network-gated
  contract test pulls a recent `.idx` from every model's bucket and asserts both directions: every
  field the table claims is really in that file, and every field it marks unavailable is really
  absent. Writing that test immediately found three things wrong with my first draft — the RAP
  uses `MSLMA` where I had written `MSLET`, the NAM family spells composite reflectivity's level
  `entire atmosphere (considered as a single layer)` where the HRRR uses `entire atmosphere`, and
  the RAP publishes near-surface smoke, which I had marked HRRR-only. The NAM one was a live bug,
  not just a table error: the app's reflectivity fetch matches the level string exactly, so asking
  the NAM for composite reflectivity had been failing outright.
- **Fixed: forecast leads were clamped to 18 hours for every model and every cycle.** The fetch
  path applied a flat `.min(18)`, which silently truncated the NAM 3 km nest's 60-hour runs, the
  NAM 12 km's 84, and three quarters of every HRRR 00/06/12/18Z cycle. The cap now comes from the
  run's own schedule in the definition table.
- Migrated the environment-field, HRRR-layer and HRRR-vs-RAP-difference call sites onto the
  catalogue; `--headless-env` takes an optional model id so a field can be rendered from any model
  in the table.
- **This is F1 only, and Phase F is not finished.** No new model is wired up (F2's RRFS/REFS/GEFS
  and the tier-2 list remain unbuilt), the generic field set (F3) covers what the app already
  fetched rather than the full surface/pressure/severe list, and F4's display modes, F5's
  run-to-run comparison, F7's ensemble workstation and F8's sounding overhaul are untouched. What
  landed is the metadata layer those depend on.

### Changed: live radar draws each chunk as it lands, and marks what is carried over

`suggestions.md` §21's core complaint was latency the machine had already paid for: the data
were on disk, decoded, and still not on screen. Live Level II arrives as chunks of ~120 radials,
six to a super-res sweep, and the stream only handed a volume to the display when a chunk
happened to finish a sweep. That held every wedge until the antenna had gone all the way round —
about 15 s in a precipitation VCP and over a minute in clear air — for data that had been sitting
locally the whole time.

- **Every chunk is now a rendering unit.** `wxdata::live::stream` emits on each chunk rather than
  at sweep boundaries, so a new 60° wedge reaches the map roughly six times sooner. The emit
  window advances each time, so per-chunk emitting does about the same total assembly work as the
  old per-sweep path, not six times as much.
- **The previous rotation stays on screen where the new one hasn't reached.** Merging a partial
  sweep used to replace the whole tilt, so the sectors the antenna had not come back to yet went
  blank — the display traded a stale wedge for no wedge at all. It now keeps the older radial in
  any azimuth the new pass has not swept, bounded to 15 minutes so nothing lingers indefinitely.
- **Retained data are marked, not silently passed off as current** — this is the half that makes
  the above honest. `BinnedSweep` now carries per-azimuth acquisition times and derives the
  carried-over wedge from them, finding the generation boundary from the data itself (the largest
  gap between bin times, with a 30 s floor so a slow clear-air rotation isn't split in two)
  instead of being told which VCP is running. The radar shader dims that wedge, and the gate
  inspector gained a "Gate collected" row showing the sampled gate's *own* time and how far it
  lags the newest radial in the tilt. On a fast-moving storm that lag is a position error; showing
  the volume's time for it would present that error as a measurement.
- Verified on a real GPU, not just in unit tests: a headless render test renders the same
  synthetic sweep with and without a stale wedge and asserts the marked half of the framebuffer
  got darker, the unmarked half is byte-identical, and nothing anywhere got brighter — the check
  that catches a sign error in the azimuth comparison, which no CPU-side test can see. A second
  test drives a two-generation sweep through the real binner end to end and asserts both the
  wedge and the per-gate time the inspector reads.
- Verified against a live radar too. `--headless-live` now follows the stream for several updates
  and reports each one's generation mask; against KMPX in a precipitation VCP it showed chunks
  039–046 arriving as eight separate updates (~150 ms decode each after the initial backfill),
  and on the tilt the antenna was actually writing the retained wedge shrank exactly as it should
  — `106.0°..122.5°`, then a half-degree sliver, then "one generation" once the pass completed —
  before the same cycle started again on the next tilt up. The rendered frame is a full 360°,
  which is the visible half of the change: that tilt would previously have drawn with an empty
  wedge.
- Not reproducible in the browser build: the web target does not run the chunk stream at all
  (`--serve`'s Live badge reports "no live stream" for every site), so the web app still polls
  whole volumes and sees none of this. Left as-is rather than papered over — it is a separate
  piece of work from the rendering path this change is about.
- Still unbuilt from §21, deliberately: the GPU upload still replaces the whole sweep texture
  rather than only the changed radial range (the merge also still deep-clones its sweeps, now
  ~6× more often); there is no multi-provider interface or first-valid-record-wins arbitration,
  no duplicate-identity detection, no explicit degraded-mode labelling, and no progressive 3D.

### Added: a radar-suitability tool — which nearby radar actually sees a point

- New "Radar suitability" map tool (`suggestions.md`'s review of a real December 2021 Kentucky
  tornado case: the nearest radar to part of the storm's early track was not obviously the best
  one once beam geometry was actually accounted for). Click a point and it ranks the nearest
  radars — across all four networks (WSR-88D, TDWR, DWD, OPERA) — showing each one's distance,
  beam-centre height, and beam width at that exact point, with a one-click "Switch" to jump the
  active pane straight there (reusing the new site-search action). New `wxdata::suitability`
  module.
- Documented honestly rather than oversold: for one fixed elevation angle, beam height at a ground
  point is a strictly increasing function of distance, so this ranking's *order* is provably
  identical to sorting by distance alone (locked in with a test). What it actually adds is the
  beam-height/width numbers themselves — the case that prompted this had two "nearby" radars by
  distance, and only the beam geometry said which one still had useful low-level coverage. A
  ranking that genuinely reorders vs. distance would need each candidate's real lowest achievable
  elevation (which differs by network and VCP), not implemented here rather than guessed at.
  Newest-volume age, terrain blockage, and per-site product availability — the other suitability
  factors `suggestions.md` lists — all need a live poll of every candidate and stay unbuilt.
- Verified live: clicked a point and got a ranked table (Tulsa's TTUL and KINX ahead of the more
  distant KTLX/KSRX/TOKC/KVNX, matching their real geography), switched to TTUL with one click, and
  watched the popup's "current" marker follow — including across a network boundary (NEXRAD KTLX
  to TDWR TTUL), the same jump the site-search feature above drives.

### Added: the Layers panel is a real window, and its search finds radar sites

- The Layers/Alerts panel (desktop and web) was a fixed card pinned to the top-left corner — the
  one surface in the app that still worked that way after the 3D map controls and every other
  floating panel had already moved to a real, draggable/resizable `egui::Window`. It's now the
  same: drag its title bar to move it, drag a corner to resize it, and its own "Layers"/"Alerts"
  label no longer duplicates what the window's title bar already says. A phone still gets its own
  docked rail or modal sheet — a resize handle is a fiddly target with a finger, and this only
  ever applied to desktop/web.
- The universal search box (the same one product/tool/layer search already used) now also matches
  every radar site by station id or by city/state — type "Tulsa" or "KINX" and pick "KINX — Tulsa,
  OK" to switch the active pane straight there, rather than needing the separate site picker
  dialog to do it in a second step. New `PaletteAction::SetSite`, one entry per site across all
  four networks (WSR-88D, TDWR, DWD, OPERA) in a new "Sites" category — ~200 more searchable rows,
  each showing only when a query matches (not in the always-visible default list).
- Verified live: dragged and resized the window, then searched "Tulsa" and "velocity" and
  confirmed both landed on the right result — the site search actually switching the active pane's
  radar in one click, camera fly-to and all.

### Added: a movable vertical clip plane in the 3D volume views (Phase H4)

- Both 3D raymarch views — the standalone "3D Reflectivity" window and the main map's "3D map"
  Smooth representations — already had an axis-aligned clip slab (a box you can shrink along
  east-west/north-south/up), but no way to cut through a storm at the angle it actually leans or
  approaches from. A new "Vertical plane" toggle in each Slice section adds exactly that: a
  bearing (0-360°) and an offset slider, clipping the volume to whichever side the plane's bearing
  points toward. Independent of and composes with the existing slab.
- One shared implementation (`render3d::VerticalPlane`/`plane_uniform`, a new field in
  `raymarch.wgsl`) backs both views — the standalone window's `ui/volume3d_window.rs` and the main
  map's `map_3d_controls` reuse the identical widget and math rather than two copies.
- Unit-tested (bearing-to-normal math, offset scaling with box size, a box not centered on the
  origin) and verified live with a new `--headless-3d SITE OUT.png [--threshold DBZ] [--plane
  BEARING,OFFSET]` CLI flag against a real volume on real GPU hardware: an east-facing plane
  through the box center rendered roughly half the echo pixels of the unclipped baseline, pushing
  the plane to the west edge reproduced the baseline exactly (nothing clipped), and pushing it to
  the east edge cleared the frame entirely (everything clipped) — confirming the math is both
  correct and continuous across its range, not just non-crashing.
- Movable clip/slicing planes' other roadmap items — a horizontal CAPPI plane visible inside the
  3D view itself (a flat CAPPI slice already exists as its own separate 2D tool), a storm-centered
  auto-clip (the axis-aligned box already has one, `volume3d::clip_around`), and a cross-section
  line drawn on the 2D map showing where the plane cuts — are either already covered by existing
  features or remain unbuilt; see `ROADMAP_NEW.md` for the honest breakdown.

### Added: user-defined radar products (Phase C1)

- A safe formula engine for GR2Analyst-style user-defined products: combine a gate's own moments
  (`REF`, `VEL`, `SW`, `ZDR`, `KDP`, `CC`) and geometry (`RANGE_KM`, `AZIMUTH_DEG`,
  `ELEVATION_DEG`) with arithmetic, comparisons, `&&`/`||`/`!`, a `cond ? a : b` ternary, and
  `min`/`max`/`clamp`/`abs` — e.g. `REF > 55 && ZDR < 1 ? REF : 0` for a rough hail signature. No
  native code ever runs: a formula parses to a fixed expression tree or fails with a specific,
  located error message ("expected an expression (at character 5)"), never a crash. New
  `wxdata::udp` module, extensively unit-tested (parser, evaluator, missing-input propagation, and
  a real bug this session's own tests caught: the first version of the parser stack-overflowed on
  deeply nested parentheses instead of returning a parse error — fixed with a deterministic
  recursion-depth limit).
- New "User-defined products…" manager (search for it, or Tools in the command palette): add,
  edit, and remove formulas, with live compile-error feedback as you type. Saved products persist
  with the rest of your settings (including through settings export/import).
- A saved product's value is evaluated live wherever you click the map — the gate inspector (B4)
  gained a "USER-DEFINED" section showing each product's name and current value at that exact
  point, re-evaluated fresh every frame so an edit shows up without another click.
- **What this is not, yet**: a user-defined product cannot be rendered as its own map layer/pane —
  that means plugging a new value into the polar per-tilt rendering pipeline (palettes, 3D,
  thresholds, all keyed by the fixed `Moment` enum), a separate and substantially larger piece of
  work than this pass, and there is no GPU/WGSL codegen path or vertical/layer aggregate functions
  (`max_vertical`, layer heights) or environmental-height inputs (freezing level) — all called out
  as remaining in `ROADMAP_NEW.md` rather than claimed done. "Synced" and "exported" (the roadmap's
  own acceptance criteria) are satisfied only via the existing whole-settings export/import, not a
  dedicated per-product mechanism.
- Verified live: added a product through the manager, watched it read 0 against a weak-echo gate
  and the raw reflectivity value against the same gate after loosening the threshold — with no
  re-click between the edit and the updated reading — and confirmed a broken formula (`REF +`)
  shows its parse error inline instead of silently failing.

### Added: a "Follow low" mode that jumps to the newest low-level cut mid-volume (Phase B5)

- New "Follow low" toggle next to the Tilt angle pills: while following live, it jumps the display
  straight to the lowest tilt the instant a sweep there lands — including a SAILS/MRLE mid-volume
  rescan — rather than waiting for the tilt already selected or for the volume as a whole to
  finish. SAILS and MRLE exist specifically to give faster low-level updates for warning
  operations; before this, watching one required either staying pinned to the lowest tilt by hand
  or waiting out the rest of the volume to see it reflected on screen.
- `Volume::changed_includes_lowest_tilt` (unit-tested) is the pure decision the live-update handler
  acts on. Off by default — it overrides the user's own tilt choice, so it has to be something
  turned on, not standing behavior sprung on them.
- Verified live: with the toggle on, manually selecting a higher tilt was overridden back to the
  lowest the next time a low-tilt sweep landed, while the chunk-stream progress tooltip showed the
  scan had already moved on to later tilts — confirming the jump happens at the sweep itself, not
  at a full-volume boundary.

### Added: a decode-time reading in the Radar source-health popup (Phase B3)

- Rounds out the latency dashboard's local half: `wxdata::live::Update` now carries `decode_time`,
  the wall clock the last live-stream sweep spent assembling and merging on this client — as
  opposed to `last_live_arrival`'s provider-side half (how stale the data already was on arrival).
  Shown as a "Decode time" line in the same Radar health popup as provider lag and stream retries,
  with its own millisecond-precision formatter (`humanize`'s whole-second rounding would show "0s"
  for every normal decode, which is useless).
- Unit-tested (`decode_time_detail`/`format_millis`). Confirmed the app runs correctly end to end
  with the new field threaded through and doesn't regress anything, but did not see the popup line
  itself render live this round: the status button that opens this popup (`active_row` in
  `layers_panel.rs`) only appears once a source is *not* Fresh, by design — a healthy KTLX feed,
  which is what was available to test against, has nothing to click. Verifying the rendered line
  needs either a genuinely degraded feed or a from-code way to force one, neither available here.

### Added: a live-stream retry count in the Radar source-health popup (Phase B3)

- The chunk stream already retried a hiccupped fetch in place (S3 blip, laptop lid, Wi-Fi
  handover) rather than tearing the whole connection down, but did so silently — nothing on
  screen ever showed it happened. `wxdata::live::Update` now carries `retries`, a running count of
  chunk fetch retries since the current stream connection started; it reaches the Radar row's
  health popup as a new "Stream retries" line, shown only once the count is above zero so a
  healthy connection (the common case) doesn't carry a permanent "0 retries" line for nothing.
  Resets to zero when the stream itself reconnects, not on every display change — it answers "has
  this connection been flaky," not "was some earlier one."
- `SourceHealth`'s single `detail` field is now `details: Vec<(&'static str, String)>` to carry
  both this and the existing provider-lag line without one crowding out the other.
- Unit-tested (`retry_detail`); the surrounding plumbing was exercised live (native + web builds,
  a running stream), though triggering an actual retry needs a real network hiccup, so the exact
  popup line wasn't independently re-confirmed pixel-by-pixel this round — the rendering change is
  a mechanical `Option` → `Vec` generalization of the already-live-verified "Provider lag" line.

### Added: a scan-strategy popup on the VCP chip (Phase B5)

- The ribbon's "VCP 35" readout is now clickable: it opens a popup with the full pattern
  description (e.g. "VCP 212 (Precipitation, SZ-2)") and a table of every tilt, how many times
  the current VCP scans it per volume, and whether any of those passes are SAILS or MRLE
  supplemental low-level rescans — none of which the app surfaced anywhere before, even though
  the decoder already extracts it in full (`VolumeCoveragePattern::elevation_cuts`, unused until
  now).
- The Tilt angle pills mark a repeated tilt with a small bullet and a hover tooltip ("Scanned 3
  times per volume (2 SAILS cuts)"), so a SAILS/MRLE insert reads as one rather than looking like
  any other cut.
- Read straight from the decoded VCP message, not inferred from the sweep count seen so far, so a
  SAILS insert a still-arriving live volume hasn't reached yet is already marked correctly.
- Deliberately distinguishes real SAILS/MRLE inserts from an ordinary split cut (e.g. VCP 35's
  SZ-2 low tilts, scanned twice for phase-coded range unfolding, not resampled for temporal
  resolution) — verified live against a real VCP 35 volume, where every tilt correctly reported
  "—" (no scheme) despite two of them showing "2" cuts/volume.
- New `wxdata::level2::tilt_cuts`, unit-tested against a constructed VCP 212-shaped pattern with
  SAILS and MRLE inserts at different tilts.

### Added: live scan-progress reporting (Phase B2)

- The live chunk stream now reports how far into the current sweep it has scanned between the
  merged-volume updates it already sent — elevation number and angle, and chunk position within
  the sweep — using per-chunk metadata the vendored chunk mapper already computed from the VCP but
  nothing surfaced. Shown today as detail in the scrubber's "Live" badge tooltip when a chunk
  stream (not interval polling) is actually feeding the pane; the underlying `ScanProgress` data
  is available to any future UI surface without touching the streaming code again.
- This is a scoped-down slice of the full B2 spec, not the whole thing: the roadmap's GPU
  incremental polar-texture rendering (drawing a sweep gate-by-gate as chunks arrive, with a
  visual "not yet received" distinction) is not implemented — that needs a tighter render-loop
  iteration cycle than was available here, and is called out as remaining work rather than done.
- Verified live: the tooltip advanced chunk-by-chunk (e.g. "Sweep 3/12 at 0.9°, chunk 1/6" to
  "chunk 2/6") while the stream ran against a real site.

### Fixed: the top status readout could disagree with the LIVE badge

- Reported live: the toolbar's "View" status could read Stale at the same instant the scrubber's
  LIVE badge read Live for the same site. The two used different, independently hand-picked
  freshness thresholds — the scrubber badge 900 seconds, the top status (`radar_health`) a much
  stricter 120 seconds, well under NEXRAD's real 4-10 minute volume cadence, so the top status
  would read Stale under perfectly normal operation. Both now read one shared constant
  (`RADAR_FRESH_SECS`), so the two can't disagree again.

### Fixed: nearby echo could render absurdly high in the 3D Observed view

- Reported live: reflectivity gates close to the radar site rendered far too high in the 3D
  Observed volume, especially with Vertical turned up. A steep tilt (near the top of a VCP) is
  naturally high up even close to the radar — 19.5° is already ~1.7 km up at just 5 km range, pure
  beam geometry with no earth curvature involved at all — and the prior beam-height fix (which
  subtracted the *lowest* tilt's height at the same ground range as a "floor", to stop the coverage
  dome visibly lifting off the ground at long range) barely reduced that, so vertical exaggeration
  multiplied nearly a steep gate's entire natural height near the radar and sent ordinary nearby
  echo shooting into the sky. That earlier fix had also shipped without a live visual check.
- Fixed by splitting each gate's own beam height into the flat-earth angle rise it would have with
  no curvature at all (true at any range, for any tilt, and already large near the radar for a
  steep one on its own) and the remainder, which is what earth curvature alone adds on top. Only
  the remainder now scales with vertical exaggeration; the angle term is a tilt's honest geometry
  and never does, at any range — so the long-range floor still doesn't detach from the ground, and
  a steep tilt near the radar no longer does either.
- Verified live in the 3D Observed view at maximum (8×) vertical exaggeration.

### Changed: the 3D map controls panel is now a real window

- The floating "3D map" controls (representation, Pitch/Bearing/Vertical/Opacity, Layers) used to
  be pinned to a fixed spot over the map with no way to move or resize it. It's a proper `Window`
  now — drag its title bar to move it, drag a corner to resize — and egui remembers where it was
  left, the same as every other window in the app.

### Added: a radar gate inspector (Phase B4)

- Clicking the radar map with nothing more specific under the click — a marker, a storm cell, an
  overlay feature — now opens a **Gate inspector**: radar site, VCP, elevation angle, azimuth,
  slant range, ground range, beam height (4/3-earth model), gate spacing/index, the raw value at
  that point, and — for velocity — the dealiased value and an estimated Nyquist velocity, plus
  whether the gate is range-folded and the wall-clock span that tilt's radials were actually
  collected over (spanning every pass, for a repeated SAILS/MRLE cut).
- Nyquist velocity is explicitly labeled "(est.)": the largest observed |v| in the sweep, the same
  proxy dealiasing itself already relies on. The decoder this app is built on does not extract the
  true unambiguous-velocity field from the raw message header, so this is not read from the
  instrument — extending that would mean patching the vendored decode crate, out of scope here.
- Verified live against real reflectivity and velocity gates, folded and unfolded, checking beam
  height against a hand computation.

### Added: a provider-lag reading in the Radar source-health popup

- The Layers panel's Radar row now shows a "Provider lag" line: how far behind wall clock the
  data already was the moment this client actually received it (the radar's own scan timestamp
  against this client's receipt), recorded on every genuine live-poll or live-stream arrival —
  never from an archive scrub or a loop's replayed frame. The first piece of Phase B3's latency
  dashboard.
- Found and fixed a race while building it: the natural first attempt tried to reuse the
  existing "is this a new live head" check (`new_head`, `DataMsg::Volume`'s handler) as the
  signal for "a live arrival just landed," but that check can independently be satisfied by the
  bucket listing discovering a name before the matching volume fetch completes — so it went
  false on a live poll that was, in fact, the first successful fetch of that exact volume. Fixed
  by tagging `DataMsg::Volume` with an explicit `live_poll` flag set only by the live-head poll,
  never by an archive/loop-frame fetch, rather than inferring it from timeline state that a
  second, independent mechanism can also change.

### Fixed: the LIVE badge flickered to Stale during normal loop playback

- Reported live, alongside the cache-corruption fix below: the LIVE/Stale badge and the "Scan
  ⟨n⟩ ago" readout would swing between fresh and hours-old on a perfectly healthy feed. The
  rolling live loop (started automatically when playback begins at the live head) deliberately
  keeps showing its playhead frame while a newly-arrived head is appended to the timeline behind
  the scenes rather than displayed — that is what makes it a *loop* over the trailing window
  instead of a feed that jumps every time a new volume lands. The badge read the *displayed*
  volume's age to decide Live vs. Stale, so every time the loop's animation was anywhere but the
  newest frame — most of the time, by design — it reported the site as stale and named the
  loop's current position as the site's lag, even though the feed itself was current the whole
  time.
- Fixed by having the badge, its age readout, and the Radar row in the source-health panel all
  read the timeline's actual newest known frame (`Timeline::newest`, new) instead of the
  currently displayed one — decoupling "is the feed keeping up" from "what is the loop showing
  right now." Verified live: badge and age now hold steady on Live/fresh throughout loop
  playback instead of oscillating.

### Fixed: live radar could get permanently stuck showing an old scan

- Reported live: the LIVE badge would flip to Stale and the age readout would jump to hours old,
  even though the site had scans from minutes ago that worked fine elsewhere — and a volume that
  loaded correctly once would sometimes revert. The newest S3 object can still be mid-upload when
  a poll or a lookahead prefetch reads it (`download_scan`'s own doc comment already warned about
  this for the live head specifically), and a half-written object downloads as bytes that fail to
  decode. Both the native disk cache and the browser's new automatic archive cache (previous
  entry) wrote those raw bytes to the cache *before* decoding them — so the one poll unlucky
  enough to catch an object mid-write pinned a broken download to the cache under that volume's
  name permanently. Every later read of that exact name kept decoding the same truncated bytes
  from the cache and failing, even minutes later once the real upload had long since finished,
  since a cache hit is never re-checked against the network.
- Fixed by writing to the cache only after a fresh download decodes successfully
  (`wxdata::level2::download_scan`, `crates/hookecho/src/volume.rs`), on both targets. A
  volume caught mid-write now simply isn't cached; the next read (poll or scrub) tries the
  network again and caches the complete file once one actually decodes.

### Added: the web build now caches archived radar volumes locally

- Native builds have always kept every archived Level II volume on disk indefinitely, so
  re-scrubbing to an hour already visited is a file read. The browser build only got that for a
  volume someone explicitly saved as an offline chase pack — every other archived volume was
  refetched from S3 on every visit, including a plain page reload. The browser now keeps its own
  automatic cache in IndexedDB, independent of chase packs, evicted oldest-least-recently-used
  first once it passes its own size cap — reloading and re-scrubbing back through a storm you
  already looked at today no longer redownloads it. The live head is never cached, since the
  newest object can still be mid-write.
- Verified live: scrubbed back through several archive frames, reloaded the page in a fresh tab,
  and confirmed (via the network log) that those exact volumes loaded with no request at all,
  while a newly-scrubbed time still fetched normally.

### Fixed: live NWS alerts (warnings, watches, advisories) failed on the web build

- Reported live: on a `hookecho --serve` deployment, active/live alerts never appeared while
  the archived-warnings overlay (a scrubbed timeline's storm-based warning polygons) worked
  fine. The two pull from different services — `api.weather.gov` for live alerts, the Iowa
  Environmental Mesonet for archived ones — and only the former requires a `User-Agent` header,
  which the web build's server-side CORS proxy (`crates/hookecho/src/serve.rs`) never sent for
  *any* proxied request (a deliberate no-inherited-headers policy that stripped this one too).
  Every `api.weather.gov` fetch routed through the browser build came back a flat 403. Native
  builds never hit this — their `reqwest` client already carries the app's identifying
  User-Agent on every request — and the Cloudflare Pages edge worker (`web/_worker.js/proxy-
  core.js`) already sent it, so only a locally-served web build was affected. Fixed by having
  the proxy send the app's own User-Agent (the same constant used everywhere else) on its
  upstream fetches, closing the gap between the two proxy implementations.
- Verified live: reproduced the bare 403 against a local `--serve` instance, confirmed the fix
  turns it into a 200 with real, current alert data, and watched it render correctly in a
  browser tab.

### Fixed: the archive date control, replaced with an actual calendar

- The archive-day picker (the LIVE/ARCHIVE badge's right-click menu) is a genuine calendar now:
  a year field, month arrows, and a day grid, browsable and clickable, alongside the existing
  typed `YYYY-MM-DD` field. Native and web share one implementation — no more platform split.
- The previous native-only `egui_extras::DatePickerButton` rarely opened at all: it draws its
  own popup nested inside the timeline menu's own popup, and that outer menu used egui's
  default context-menu close behavior (`CloseOnClick`), which closes on *any* click, inside the
  menu or out. Opening the picker's inner popup routinely closed the outer menu (and the
  picker with it) before a day could be picked. The new calendar is drawn as plain widgets
  directly inside the already-open menu — no nested popup — and the menu's close behavior is
  now explicitly `CloseOnClickOutside`, so multi-step interaction (browsing months, then
  picking a day) survives inside it. This also drops the native-only `egui_extras` datepicker
  feature and the `jiff` dependency it required.
- Fixed a related bug in the typed field found while testing the calendar: its resync check
  only caught an externally-moved day (a caret, the calendar, a deep link) when the *year*
  changed, so stepping from the 13th to the 3rd of the same month left "13" on screen after the
  map had already moved to the 3rd. Now compares the full date.

### Fixed: the web build could crash outright on the Observed 3D volume

- A real regression, reported live: on some GPUs (mobile browsers running WebGPU over
  ANGLE/OpenGL ES in particular — the case that surfaced it) the app could stop entirely with
  "Uncaught RuntimeError: unreachable" the moment the map's Observed 3D volume tried to render.
  The cause was a wgpu validation error one step earlier: `radar_observed.wgsl`'s per-frame
  uniform buffer had grown to 76 bytes when the multi-tilt Layers highlight landed, and 76 is
  not a multiple of 16 — a requirement desktop Vulkan/Metal/DX12 never enforces but WebGL-class
  ("downlevel") backends do. Padded the buffer to 80 bytes (one trailing `f32`) to satisfy it
  everywhere, not just the backends that happened to already tolerate the mismatch. Audited
  every other uniform struct in the shader set for the same class of bug; none of the others
  had it.
- Verified end to end: built the actual web bundle, served it, and reproduced the exact
  Observed-3D code path live in a browser (including selecting multiple highlighted tilts, the
  feature that grew the buffer past 76 bytes in the first place) with a clean console and no
  panic, where it previously would have stopped the app outright.

### An interactive model meteogram in the Forecast window

- The point-tap **Forecast** window's "This week" outlook — one forecaster-reconciled NWS
  blend — now has a **Model forecast** section under it: pick GFS, ECMWF, GEFS mean or GDPS,
  pick a field (2 m temp, 2 m dewpoint, 10 m wind, MSLP or 500 hPa height), pick how far out
  (24h / 3 day / 5 day), and see that one model's own raw run graphed at the tapped point,
  with a min/max/avg line underneath. The same numbers the map's Global-model layers already
  draw as a grid — sampled at a point and strung into a line instead, so "what does GFS
  actually say here" no longer means opening the map layer and hovering the exact pixel by
  eye. Switching any picker refetches immediately, the same rule the map's own Global-model
  picker already follows; a 15-minute cache like the NWS forecast beside it keeps re-tapping
  the same neighborhood cheap.
- New in `wxdata::global`: `fetch_point_series`, which samples one point across a whole
  forecast period from one pinned model cycle — every earlier fetch in this module answered
  "the whole grid, one hour" and needed a partner that answers "one point, every hour," since
  a meteogram describes one run's evolution rather than whichever cycle happened to be newest
  at each hour independently (the same edge `fetch_aligned`'s doc comment already explains for
  comparing two models at once).
- Found along the way: GFS and ECMWF's "10 m wind" field is actually the U (east-west)
  *component* of the wind, not its speed — a real vector quantity, negative half the time.
  The map's own legend already takes its absolute value for the color scale; the new graph
  now does the same, rather than a meteogram that reads as calm every time the wind happens
  to blow from the east.

### GOES-West for the satellite bands

- The IR/visible/water-vapor satellite layers can now read from **GOES-West** (GOES-18)
  instead of the hardcoded GOES-East — a Layer settings toggle, since the two satellites'
  CONUS scans overlap and showing both at once would double-paint that overlap rather than
  extend coverage. Picking West and back refetches every band at once rather than waiting
  out the normal five-minute cadence, the same immediate-refetch-on-change rule the Global
  model picker already uses.
- The roadmap's "GOES from the source" entry called this out as one of two things actually
  left (the other, an offline-chase-pack copy of the imagery, is still open).

### A fourth global model: Environment Canada's GDPS

- **GDPS** joins GFS, ECMWF and GEFS as a Global forecast source — a second national
  weather service's own global model, independent data assimilation and physics from
  the NCEP/ECMWF pair already here. Read straight from Environment Canada's public
  Datamart (`dd.weather.gc.ca`), which — unlike every S3 bucket this app otherwise reads —
  already publishes one GRIB2 message per file, so fetching a field is a plain GET with no
  index to slice a byte range out of first.
- Also fixed: the model pickers in the ribbon toolbar (`app/chrome/ribbon.rs`) had never
  been updated for GEFS or NAM's 12 km grid, both added earlier — a second, separate copy
  of the model-button row from the one in the layers panel that quietly drifted out of
  sync. Both now show the full roster (GFS/ECMWF/GEFS/GDPS, and HRRR/RAP/NAM/NAM12), the
  same models the layers panel already offered.

### NDFD: the NWS's own forecaster-blended grids, and a real GRIB2 decoder bug fixed along the way

- **NDFD temperature, wind speed, wind gust and snowfall** join the model layers — read
  directly from the National Digital Forecast Database's own public S3 bucket
  (`noaa-ndfd-pds`). This is a genuinely different source from every other model layer here:
  not a raw dynamical-model run, but the NWS's own forecaster-edited blend, the one other
  radar apps call out by name alongside their raw model output. No forecast-hour scrub for
  these — NDFD bundles every valid time for the next several days into one file per element
  with no index to slice a single hour out of, so each fetch decodes the whole thing and
  keeps whichever message is valid nearest to now, the same "always current" shape the HRRR
  CAPE/SRH environment layers already have.
- Along the way, found and fixed two real bugs in `vendor/gribberish`'s Complex Grid Packing
  decoder (GRIB2 data representation template 5.2) — the packing NDFD's snowfall grid uses
  and nothing else in this app had exercised before. It panicked on every single message:
  group reference and width fields were read with the endian-less `.load()` instead of
  `.load_be()`, silently misreading any field wider than a byte on a little-endian machine,
  and the last group's length was computed from the reference/increment formula every other
  group follows instead of being read explicitly the way the GRIB2 spec requires (the
  sibling spatial-differencing template already got this right — this one didn't). A real
  extracted message is now a committed regression fixture so this can't come back silently.

### Two more GOES bands: visible and water vapor, not just clean IR

- **GOES-East visible** and **GOES-East water vapor** join the existing clean-IR satellite
  layer, all three read the same way — straight from the satellite's own S3 bucket rather
  than GIBS' pre-rendered tiles. Visible is daytime cloud texture in plain grayscale
  reflectance; water vapor is upper-level moisture, day or night, with its own dark-dry
  to blue-moist enhancement. Same fetch and regrid code as clean IR already used (it only
  ever needed the band number as a parameter) — the new work was two colormaps and the
  wiring to make each its own toggleable layer.
- The start of matching RadarOmega's "satellite imagery (Visible, LWIR, water vapor)" — one
  satellite, one source (ABI CMIP CONUS), all three of its bands.

### A third "Smooth" 3D volume: spectrum width, plus a typeable archive date and multi-tilt highlighting

- **Spectrum width** joins reflectivity and correlation coefficient with its own resampled
  "SW" 3D volume — the same continuous, gap-free fill, with its own **Denoise** floor (in
  m/s, not dBZ) so switching between the three never carries one moment's number into
  another's units.
- The 3D "Layers" list — every real tilt in the Observed volume — is now inside a scroll
  area instead of a plain stacked column, so a 19-tilt VCP with MESO-SAILS cuts no longer
  pushes Denoise and everything below it off the bottom of the floating panel with no way
  back to it.
- That list also went from picking one tilt at a time to picking several: click more than
  one and each pulls out of the stack together, with stats for every selection listed below
  rather than just the last one clicked. Capped at eight at once — the GPU uniform has a
  fixed number of highlight slots, and nobody is comparing more than that by eye anyway.
- The archive-day field in the LIVE/ARCHIVE menu now has a typed `YYYY-MM-DD` box next to
  the calendar button on desktop, not just on web — a calendar is fine for "a few days
  back" but painful for "reach a specific day in 1991" one click at a time, and typing
  reaches either just as directly.

### Two more models: NAM's own 12 km grid, and a GFS ensemble mean

- **NAM 12 km** joins HRRR/RAP/the NAM 3 km nest as an Environment model source
  (CAPE, SRH, contours) — the NAM's own parent grid, not just its nest, so it's
  a third genuinely independent dynamical core and cycle rather than the same
  nest at a different crop.
- **GEFS mean** joins GFS/ECMWF as a Global forecast source — the 31-member
  GFS ensemble's average, which is a different (and sometimes more useful)
  answer than any one deterministic run. Its native half-degree grid needed
  its own coarser output resolution to regrid without leaving most of the map
  empty (a source cell coarser than its scatter target leaves gaps between
  samples) — first attempt landed at 36% coverage before that fix.
- The start of "every model RadarOmega/WeatherWise has" — both verified live
  against their real public buckets; more to follow incrementally rather than
  landing untested all at once.

### The layers/alerts panel no longer gets stuck hidden

- The floating layers/alerts panel steps aside whenever a drawer page
  (Sounding, Settings, X-section, Volume 3D, …) is open, and comes back
  once it closes. That "comes back" relied on the drawer noticing a page
  had closed, which it only ever checked when *some other* page called in
  to ask — so closing the *last* open page left nothing to trigger the
  check, and the drawer considered itself permanently open. The panel
  then stayed hidden until a restart, which is what the alerts panel
  being "stuck" or "disappearing" actually was — alerts themselves kept
  fetching fine underneath it the whole time. The drawer now runs that
  same check unconditionally, once a frame, regardless of whether any
  page is open.

### GOES satellite IR, read straight from the source

- A new **GOES-East IR satellite** layer (National weather) reads ABI Level
  2 Cloud and Moisture Imagery directly from the satellite's own public S3
  bucket, instead of GIBS' pre-rendered tiles — this was the piece the
  roadmap's GOES entry called out as actually left: native resolution and a
  source this app controls the refresh cadence of (every ~5 minutes, CONUS
  sector, Band 13 clean IR). The fixed-grid geostationary scan the
  satellite actually flies gets forward-projected pixel by pixel onto a
  regular lat/lon grid, the same shape every other national field layer
  already uses, so it drew with no new rendering path at all.
- Along the way, fixed a real (if rare) crash in `hdf5lite`, this app's
  from-scratch HDF5/netCDF-4 reader: an attribute whose datatype it doesn't
  interpret — seen on every ABI file's coordinate variables — could get its
  size misread as enormous, overflowing and panicking instead of degrading
  gracefully like every other "can't make sense of this one" case already
  does.

### Denoise reaches the Observed 3D volume too

- The map's "Observed" 3D mode had no way to hide light rain and noise —
  denoising only existed for the resampled "Smooth" volume. Observed mode
  now has its own **Denoise** control in the 3D panel, right beside Gates
  and Fill gaps. It edits the same per-moment threshold the 2D "Product
  settings" panel already has, so turning it on denoises whichever view
  you're looking at rather than being a second floor to keep in sync with
  the first, and it works for whatever moment the pane is showing —
  reflectivity, velocity, or anything else — not just dBZ.

### Local cell tracks no longer freeze the app, and weather alerts no longer time out

- Local cell tracks (the reflectivity-derived storm tracking used at sites
  with no Level 3 SCIT product) recomputed from the timeline's very first
  frame up through the current playhead on every volume tick, with no cap
  on how many tracks could pile up along the way — a long live session or
  a timeline scrubbed deep into an archive day turned this into seconds of
  frozen UI on every single refresh, and it never got cheaper for the rest
  of the session. It now looks at a bounded trailing window of volumes
  (16, comfortably more than the 6 the motion fit itself uses), and the
  underlying cell finder now caps how many cells one sweep can report so a
  field of anomalous-propagation or biological-scatter clutter can't turn
  into thousands of "cells" for it to track.
- Weather alerts resolved zone-only advisories (heat, winter weather,
  marine — anything without an inline warning polygon) one zone at a time,
  awaited sequentially. A cold zone-geometry cache with many such alerts
  active at once — exactly the days the feed matters most — could take
  long enough to run past the alerts fetch's own timeout, which read as
  "Weather alerts unavailable" for no reason but the fetch's own shape.
  Zone geometries now fetch 16 at a time instead of one after another.

- Adjacent tilts in the map's "Observed" 3D mode used to show real gaps
  between them — accurate to what the radar actually measured, but a
  volume with only 14-ish elevations reads as separated rings rather than
  a storm. **Fill gaps** (on by default) adds one synthetic copy of every
  gate at the midpoint toward the next tilt up — still that gate's own
  real reading, just given some vertical reach instead of none — so the
  stack reads as one volume without resampling anything or losing native
  gate resolution the way the "Smooth" volume does.
- A new **Layers** list in the 3D panel shows every real tilt — elevation,
  radial count, and (when the source carries per-radial timestamps) the
  actual wall-clock span the radar spent scanning it, since a volume's
  tilts do not share one instant. Clicking a tilt pulls it toward the
  camera and fades every other tilt into the background, and shows its
  coverage percentage and strongest reading below the list.

### TVS and TDS reject a couple of the false positives real hardware makes

- Rotation-couplet (TVS) detection required nothing but velocity shear —
  clear-air noise, sidelobe returns, and receiver glitches could clear the
  gate-to-gate threshold with no storm anywhere nearby. It now requires real
  reflectivity echo (a generous 20 dBZ floor) collocated with the shear, the
  same collocation debris-signature detection already leaned on.
- Debris-signature (TDS) detection no longer accepts a cluster confined to a
  single azimuth — the shape a stuck bit or a receiver glitch paints down
  one bad radial, not the shape an actual debris ball (which has some real
  width) makes.

### The 3D map denoises light rain, and slices into the storm

- The map-embedded "Smooth" reflectivity volume now hides everything weaker
  than a floor (18 dBZ by default) before raymarching, so a wide stratiform
  rain shield no longer buries the convective cores that are the actual
  reason to look in 3D — the same gating the standalone 3D Reflectivity
  window already had, now on the map. **Denoise** toggles it off if you want
  the unfiltered volume back, with a slider to move the floor.
- A **Slice** panel crops the resampled volume to an E–W/N–S/Up box, so you
  can cut into a storm instead of only ever viewing it from outside — the
  same slab control the standalone window has, now available while the
  volume sits on the map.
- **Quality** (Low/Medium/High) trades raymarch samples per pixel for frame
  time, for the resampled Smooth/Debris volumes.

### 33 community color tables join the built-in alternates

- Reflectivity, velocity, spectrum width, and correlation coefficient each
  gain a batch of community-designed `.pal` tables (Ben's BR, Viper HD, GR3
  v2, AWIPS Evans, NWS St. Louis, Russian CC, and more) selectable from the
  same alternate-palette picker as the existing colorblind-safe and
  high-contrast tables — no new UI, since the picker already lists whatever
  `colormap::alt_names` returns for the moment. Reflectivity and velocity
  each go from 2 alternates to 12, spectrum width from 0 to 3, and
  correlation coefficient gains its first 10.

### The timeline picks an exact time, not just a day

- The archive day picker (Live/Archive badge → calendar) now sits beside a
  typed hour:minute (UTC) field that jumps straight to the nearest volume —
  the drag-precise time track was already there, but pinpointing an exact
  scan on a specific historical day now takes typing a time instead of eyeing
  a pixel.
- **↑/↓** jump the timeline by about an hour, alongside **←/→**'s existing
  one-frame step — closing distance across a day of archive volumes no
  longer means stepping through every 4-6 minute scan between here and there.

### The map-pitch 3D camera goes further, and takes a keyboard

- Camera pitch now goes to 75° (was capped at 60°) for a steeper, more
  dramatic oblique view.
- **W/S** tilt the camera and **Q/E** rotate it — a keyboard alternative to
  the right-drag gesture, and the only way to adjust the 3D camera at all
  without a mouse.

### TVS and TDS detection see height, not just one tilt

- Both the client-side rotation-couplet (TVS) and debris-signature (TDS)
  detectors now check the lowest several tilts instead of only the lowest
  one, and raise their confidence by how many of them show the same signature
  and how high the tallest one reaches. A couplet or a debris ball confined
  to a single sweep is as often a gust front, biological scatter, or a data
  glitch as it is the real thing — vertical continuity is the classic
  criterion neither detector had access to before. The alert banner and the
  map label both now show the confidence, tilt count, and height once more
  than one tilt confirms a hit.

### A docked control ribbon (desktop/web)

- The floating map-first chrome (search pill, right-edge control column) is
  replaced on desktop and web by a docked top ribbon in the style of WSV3's
  toolbar: labelled control groups over a navy gradient, a docked colour scale
  under it, a blue timestamp pill, and a thin bottom status bar. The original
  layout is still there — Settings → Layout — for anyone who preferred the
  map getting every pixel back.
- The ribbon swaps its middle section by data type: **Radar** (site, moments,
  tilt angle, pane count), **Model** (GFS/ECMWF/HRRR/RAP/NAM source pills, a
  colour-fill toggle list, a contours dropdown, HRRR future radar), and
  **MRMS** (the national mosaic products). Overlays, Tools, Capture and the
  transport stay on screen in every mode.
- HRRR future radar gained a **sub-hourly mode**: 15-minute steps out to 18
  hours from the `wrfsubhf` product, instead of whole hours only.
- A **Day/Night** overlay pill shades the night side of the map, draws a
  dashed terminator line, and lays a 20°-stepped lat/lon graticule over the
  basemap — all driven off the sun's real current position, not a fixed
  offset from local time.

### The map-pitch 3D view gets a Smooth volume, and a Debris mode

- The **Smooth** representation in map-pitch 3D (alongside the existing
  Observed sweeps) now actually draws: a regularized reflectivity volume,
  resampled off the UI thread onto a Cartesian grid the same way the
  standalone 3D window does, then raymarched in place on the pitched map with
  the same Vertical/Opacity controls. It has its own GPU pipeline and
  per-pane textures, so it can't collide with the 3D window's volume if both
  are open on different sites at once.
- A new **Debris** representation resamples correlation coefficient instead
  of reflectivity, with its volume inverted before raymarching so a lofted
  low-CC pocket — a tornado debris signature — lights up the way a
  reflectivity core does, instead of a plain CC volume just showing ordinary
  high-CC rain everywhere. Selectable whenever the pane's 2D product is CC.
- Observed sweeps' beam height, in map-pitch 3D, no longer multiplies the
  earth-curvature climb every tilt has with range by the Vertical slider —
  only how far a tilt sits above the lowest tilt's own height at that same
  range (genuine storm structure) gets exaggerated, so a low-tilt base scan
  no longer visibly lifts off the ground at high Vertical settings.

### SPC Fire Weather Outlook

- A new **Fire weather outlook** picker (Layer options → Outlooks), Day 1-2:
  the categorical risk (Elevated/Critical/Extreme) and the dry-thunderstorm
  hazard together, in the same shape as the existing SPC/ERO/WSSI day
  pickers. Read from SPC's own ArcGIS map service, since the site only
  publishes this one as a KMZ rather than the plain GeoJSON its other
  outlooks ship.

### The model difference layer compares the same instant

- GFS and ECMWF cycle every 6 hours from the same UTC anchor but don't post at
  the same wall-clock speed, so fetching both at the same forecast-hour offset
  could silently compare two different valid times for hours at a stretch
  whenever one model's latest cycle wasn't up yet. ECMWF is now fetched at
  whichever forecast hour lands exactly on GFS's own valid time — an exact
  re-target, not an interpolation, since every cycle for both models lands on
  a whole UTC hour. Falls back to the old (honestly mismatched, still labeled)
  pair if that specific hour isn't published.
- The model difference layer can now be viewed as **two side-by-side panes**
  instead of one subtracted layer: one model's own field in each pane, on the
  same color scale, cameras linked — a difference in the field itself (not
  just where the two disagree) reads at a glance. "View side by side" in
  Layer options → Model comparison, or "Compare models in 2 panes" from the
  command palette; the field picker and valid-time readout are shared with
  the subtraction view.

### Fixes

- The web build's CORS proxy dropped every client header, including `Range`
  — so HRRR, RAP, NAM, NBM and GFS, which all pull one GRIB message out of a
  ~130 MB file by byte range, silently fetched the whole file and tripped the
  proxy's size cap. Every one of those layers 502'd on the web build. The
  proxy now validates and forwards `Range`, and answers with a real `206` on
  both the native `--serve` proxy and the Cloudflare Worker.
- The 3D radar volume (map-pitch "Observed" mode and the 3D window) stopped
  loading every available tilt once a live volume grew past the point it was
  first opened, and its instanced gates left large sampling gaps along each
  beam at typical densities — both looked like missing data. Sweep count is
  now part of the rebuild key, gate footprints tile the beam with no gap, and
  each tilt fades a little more than the one below it so the stack reads as
  layers rather than a flat wall of paint.
- The 2 m temperature and dewpoint field legends printed raw Kelvin regardless
  of the Units setting, while the station plots and everything else already
  followed it.

### GPS connects itself

- **Connect GPS at launch** (Chase tab; `gps_autoconnect` in settings.json)
  opens the gpsd stream every start on desktop, so a receiver on the dash no
  longer needs the connect button clicked every morning. Turning autoconnect
  on connects at once; Disconnect GPS turns it off again. Android and the
  web keep the click, since there it is a permission prompt.

### Warnings say where, and what to do

- Spoken warnings are on by default and now say something the tone cannot.
  The script leads with the hazard, then where the warning sits against a
  place you saved ("covering Home", "12 miles northeast of Home"), then the
  counties with the state spoken in full, the towns from the bulletin's own
  "Locations impacted include..." list, the motion, and the office's call to
  action. Previously a warning that covered a saved marker was announced as
  "Tornado warning for covers Home" and never named a county at all.
- The tone plays first and the voice follows it, one announcement at a time.
  Every cue used to open its own audio stream on its own thread, so the alert
  tone played over the opening words and a squall line warning four counties
  in one refresh produced four voices at once.
- An emergency now runs immediately after the sentence already being spoken,
  discarding queued ordinary warnings and chase updates. Warnings delivered
  together are spoken from highest to lowest escalation.
- Spoken warnings honour quiet hours the way the tone always has. Escalated
  warnings still go past it. The voice previously ignored quiet hours
  entirely.
- Speech follows the alert volume slider. Piper's output ignored it, so the
  voice was louder than the tone introducing it.
- Settings grows a "Speak a test warning" button that runs the whole chain on
  a made-up warning, and tells you when a configured Piper cannot run — on
  Arch the text-to-speech Piper is `piper-tts-bin` in the AUR, while
  `extra/piper` is a mouse-configuration tool that installs the same binary
  name.
- An audio device unplugged mid-playback no longer strands the thread waiting
  on it forever.

## 0.12.0-beta.2 - 2026-08-30

Third R18 checkpoint, and the biggest one: the app stops being a US radar
viewer. Germany, Europe and Canada get their own radars, composites and
warnings; the whole render and data path got measured and made faster; and the
site, the Lite viewer and the headless server all grew up. Still a prerelease —
0.12.0 waits on RRFS and the remaining store submissions.

### The world, not just the States

- German DWD radars decode natively, velocity and dual-pol moments included,
  and the DWD national composites are a basemap layer.
- The OPERA network via EUMETNET OpenRadarData puts European radars on the
  map, with a generic WMS bridge behind them for composite layers.
- Canada: ECCC GeoMet radar composites and ECCC public alerts.
- MeteoAlarm European warnings, ranked by their own severity scale rather than
  forced through the US one — Red now outranks Yellow, which it did not.
- GIBS Himawari and IMERG basemaps for the rest of the planet.
- Distances read in the units of the region on screen. A radar in Germany
  measures in kilometres without being asked.
- Site pages for the German and European networks, and a radar-data
  attribution surface that names whose data you are looking at.

### Faster

A measured pass end to end, not a guess:

- The decode path stops allocating per sweep, binned sweeps survive the
  playhead, and the wasm build lost weight (fonts fetched, rayon and gif
  degated).
- The renderer reuses radar GPU state, wind particle bind groups and a
  palette-only LUT; field textures get evicted; the tile cache weighs bytes
  instead of counting entries (it was 512 MB while claiming 134 MB).
- Overlay re-tessellation comes off the gesture, one janitor thread replaces
  several, and an idle window slows its own heartbeat.
- Every data feed asks before downloading (conditional requests), the proxy
  caches, and neither cache can grow forever.
- The Lite viewer fetches frames in parallel, decodes them as they arrive, and
  a refresh only fetches what it does not already have.

### Serve and headless

- Every network renders headlessly, with warnings and chrome in the output.
- `/national.png`, mp4 loops and velocity presets; the national mosaic moves.
- A busy renderer serves a stale image rather than queueing, and the image
  cache prunes itself.
- One command stands the image origin back up (`scripts/img-origin`).

### Analysis and data (R18 batch)

- Dealiasing solves the whole sweep at once — a maximum spanning tree over
  region-boundary votes — so a couplet folded twice comes back right.
- Derived products extrapolate down to the surface under a low beam, so VIL
  near the radar stops reading low.
- 2 m dewpoint joins the global layers and the model-diff readout.
- SPC watch boxes, tornado and severe thunderstorm, drawn under the warnings.
- Local cell tracking computed from reflectivity, with 15- and 30-minute
  extrapolation and a closest-approach ETA — for sites and networks with no
  Level 3 storm-cell table.
- Level 3 Digital Base Velocity (N0G) decodes.

### Integrations and quality of life

- MQTT gained a command topic (point the app at a site, a product, or mute it
  from an automation) and optional Home Assistant discovery, so HA creates the
  device itself.
- A lightning-strikes layer fed from your own broker, plus a standalone relay
  in `scripts/strikes-relay` that fills it. The app never connects to a strike
  network itself.
- A curated Piper voice picker instead of one hardcoded voice.
- `?palette=` on snapshot renders.
- Web retries actually back off now, and the quiet-hours queue is written as it
  changes rather than only on exit.

### The web build

- Alert-rule backtests run in the browser.
- Offline chase packs: save a loop into the browser and play it with no signal.
- Zoom controls, full screen, a warnings overlay and neighbouring-radar
  switching in the Lite viewer, whose app bundle is now content-hashed.

### The site

hookecho.io grew from a landing page into the product's front door: docs with
search, a blog with RSS, per-site and per-state radar pages, TDWR and
international pages, historic storm pages, a glossary, an honest comparison
section, a live roadmap, an embed generator, comments, and OG cards built at
build time. Live radar renders on the pages themselves.

### Fixes

- Archive-less sites (TDWR, DWD) no longer claim "(no volumes)" while showing a
  live volume.
- 1-degree sweeps filled every other azimuth bin.
- Crash reports are honest: a caught decode panic is not a crash.
- The live stream survives a network drop instead of restarting its backfill
  every minute.

## 0.12.0-beta.1 - 2026-08-26

Second R18 checkpoint. The phone gets the same chrome the desktop got in
alpha.1, onboarding stops asking questions, and the data lanes land. Still a
prerelease: a few lanes (RRFS, the remaining store submissions) come at 0.12.0.

### The phone

- The persistent bottom sheet and the five-slot toolbar are gone. The phone
  draws the same floating chrome the desktop does — search pill, control
  column, scrubber — with content in Material modal sheets instead of a
  drawer. Predictive back still works.
- Long-press the map to inspect what is under your finger, double-tap and drag
  to zoom with one hand, swipe sideways between panes.
- Haptics: ticks as the scrubber crosses a frame, a buzz when a warning lands
  on you, a bump when a sheet snaps.
- Tablets and landscape phones get a side rail and a docked drawer instead of
  sheets.
- Widgets show how far the nearest storm is. A battery-saver mode stretches
  the poll cadence and throttles repaints. On a chase, the next radar down the
  road is prefetched before you need it, and a chase can be replayed against
  the archive afterwards.

### Getting started

- The setup wizard is gone. First run finds you, picks the nearest radar and
  draws it — about ten seconds, no questions.
- Notification permission is asked for when you turn on something that
  notifies, or when a warning lands near you. Never at install.
- One help hub behind `?`: glossary, hotkeys, the tour and what's new, all
  searchable in one place. Labels across the app read in plain language, with
  the abbreviation kept in parentheses for anyone who wants it.
- Text products — area forecast discussions, tropical advisories — render in a
  real webview with real typography, on both desktop and Android.

### Weather

- GOES loops run over archive dates alongside the radar timeline, and the
  mid-level water vapor bands are selectable.
- MRMS rotation tracks (30/60/120 minutes) are field layers now, and hail
  swaths accumulate locally over a window you choose.
- Snow squalls and banding get their own emphasis in winter.
- An alert rule can trigger on a lightning jump — the rate of change of flash
  density inside a cell.
- NAM nest and NBM join the model list. Synoptic mesonets join the surface
  obs, with your own API key.
- Cells are ranked by a composite severity score, and tapping one opens the 3D
  volume already centered on it.
- Panes remember their own thresholds and field layers. Events can be saved as
  replay bundles — time range, site, camera — and replayed with archived
  radar, warnings, reports and outlooks in sync.

### The web build

- Installable as a PWA, with the app shell cached offline.
- Tiles, volumes and palettes persist between visits.
- File dialogs, GPS and notifications all work in the browser now.
- A share button writes a permalink that carries the site, camera and time.

### Elsewhere

- MQTT publishing, a Home Assistant camera loop endpoint and a live dashboard
  at the `--serve` index.
- An update chip appears when a newer release exists. No self-updating.
- Motion throughout, with `reduce_motion` and an automatic degrade on slow
  frames. Labels, roles and focus order on all of the new chrome.
- The application id is `io.hookecho.HookEcho`, and every screenshot in the
  README, the store listings and this repo was reshot on the new chrome.

## 0.12.0-alpha.1 - 2026-08-25

First R18 checkpoint: the desktop and web chrome, rebuilt. A prerelease — the
mobile chrome is still the old sheet-and-toolbar layout, and the data lanes
land later in the cycle.

- The docked sidebar and the docked timeline bar are gone. The map runs edge
  to edge, and everything that used to eat it now floats over it: a search
  pill top-left, a control column down the right edge beside the color scale,
  and a scrubber pill along the bottom with time ticks, play, a LIVE badge and
  the loop range.
- Tools you browse rather than watch — settings, the event library, alert
  rules — open as pages in one slide-over drawer with a back-stack, one page
  at a time, on every platform. Anything you click *on the map* answers in a
  card anchored next to the click instead of in a window wherever egui last
  left one.
- The window is borderless: its three buttons float with the rest of the
  chrome, the empty strip along the top edge drags it, double-clicking that
  strip maximizes, and any edge or corner resizes. `--decorated` hands the
  frame back to your window manager if it disagrees.
- Design tokens: one metrics module with a Comfortable default and a Compact
  density, the theme list curated to six, a free accent color, and Inter
  bundled on every platform including web.
- The 60-second tour now points at the chrome that exists; the keyboard cheat
  sheet and every hotkey work unchanged; workspaces round-trip and now also
  remember which panel and drawer page were open.
- Streamer (OBS) mode and `?embed=1` strip all of the new chrome, same as
  before.

## 0.11.0 - 2026-08-25

- Basemaps stopped being an afterthought: every pane now draws its own style
  instead of borrowing the active pane's, tiles are fetched at retina
  resolution and overzoomed from the deepest resident ancestor rather than
  going blank, the built-in vector cartography gained buildings, a full road
  ladder and POIs, and the 40-item dropdown became a categorized thumbnail
  grid on both desktop and Android. New sources: keyless hybrid satellite
  (Esri imagery under our own labels), the Esri gray/NatGeo/Oceans canvases,
  an "Auto" entry that follows the theme, and a custom XYZ template on
  desktop and Android.
- The live data path no longer re-merges the whole volume per chunk, which is
  what made a site switch stall and left seams across a partial sweep. Label
  placement, alert rows and the desktop frame loop were profiled and fixed in
  that order; the Android loop cache and site-switch fetches now run in
  parallel.
- Alerts identify a warning by its VTEC event key instead of its message id,
  so a continuation stops re-firing; an outbreak collapses into one rolling
  summary past a threshold; webhooks retry with backoff; Android gained exact
  alarms, a battery-exemption prompt, a watchdog and a delivery-health
  readout; and the spoken script leads with hazard, place and direction
  through an optional local neural voice.
- The rules engine can attach a snapshot, play a per-rule sound, combine
  conditions with one level of AND/OR, and backtest a rule against archived
  volumes.
- Radar accuracy: dealiasing scores region merges by boundary vote and
  anchors on the previous sweep, gate edges stay crisp under smoothing, and
  beam height uses each site's real tower height instead of a flat 20 m.
- Products the app could not show: specific differential phase (KDP, derived
  from the phase field already fetched), single-site composite reflectivity,
  a gate inspector that reads every moment at the gate under the cursor, MRMS
  precipitation rate, nowcast leads out to 120 minutes drawn faintly enough
  to read as a guess, an archive date picker, a wind row on the meteogram,
  trend sparklines in the cells table, NHC advisory and discussion text, TFRs
  and G-AIRMETs, and reflectivity tinted by precipitation type.
- German radar. DWD's open data is the only keyless international volume feed
  there is — seventeen sites, five-minute volumes, reflectivity only. The
  browser build does not offer it; a volume is 4.4 MB and the web build
  fetches through a shared cache.
- A share link can carry the reflectivity threshold (`thr:25`, or `thr:off`),
  which is what an embedded dashboard needs to open at its own threshold
  without changing anyone else's defaults.

## 0.10.0 - 2026-08-14

- The browser build can be embedded in another page's dashboard: `?embed` hides
  all chrome and holds the map at one frame a minute until it is touched, so a
  radar living in someone else's iframe stops costing them a CPU core.
- An embedded map hands its view (site, product, tilt, basemap, camera) to the
  page hosting it once a second, and takes one back through the share link's new
  `bm:<basemap>` and `srv` fields. Browsers partition an iframe's storage, so the
  host is the only place an embedded view can persist — this is what stops
  StormDesk's radar resetting on every launch.
- A lost graphics context or a panic after startup now says so instead of leaving
  the last frame on screen forever.
- `cargo run -p wxdata --example sites_json` dumps the radar site registry for
  embedders that want a site picker without the crate.

## 0.9.0 - 2026-08-11

- Three dual-pol signatures the app never computed: three-body scatter spikes
  (near-proof of large hail), ZDR columns (an updraft proxy that deepens before
  a storm intensifies), and the melting-layer bright band. Off by default, and
  none of them makes a sound — the only detection that alarms is still the one
  that means debris is in the air.
- Satellite lightning gains a density field: where the GLM flashes are thickest
  over the last 15 minutes, which is where a lightning jump shows up first.
- Colorblind-safe reflectivity and velocity palettes ship built in, alongside an
  OLED theme, a caption on share cards, a new-scan chime, and beam height on the
  Measure tool.
- Alerting learns restraint and reach: quiet hours with a severity floor,
  rotation near a place you watch, alerting on your own live position, and a
  radar snapshot attached to the push. Pushes held back by quiet hours now come
  back as one summary when the window ends, instead of vanishing.
- The GOES frame follows the radar's clock, so scrubbing back through an event
  takes the satellite with it.
- SPC Day 4-8 outlooks and TAFs on station tooltips.
- A chase breadcrumb log with GPX export, and desktop notifications.
- Android: a home-screen radar widget and a quick-settings tile.
- A small always-on-top mini-loop window on the desktop, and a Debian package
  next to the AppImage so the menu entry, icon and updater are the system's job.
- A crash the app can explain: a panic now leaves a report, and the next start
  offers it back with a Copy button. Nothing in it identifies you.
- Fuzzing the decoders that eat outside bytes found four real bugs — a panic on
  a half-written volume from the live head, two hangs on malformed GRIB2, and a
  parser that died on a mangled hurricane-hunter bulletin. All fixed, all now
  regression-tested, with the fuzz job running nightly.
- First run is four cards and an optional spotlight tour on the real interface,
  down from ten pages of wizard.
- Trackpad pinch zooms and horizontal scroll pans; `hookecho://` links reach an
  already-running instance and carry product and tilt; placefile icon sheets
  cache on disk; a temperature unit the station plots honor.
- Errors you can read, copy and actually see, and a sweep of the unwraps behind
  decoded and user-supplied data.

## 0.8.0 - 2026-08-09

- A model difference layer: GFS−ECMWF and HRRR−RAP, resampled onto a common grid
  and drawing nothing where the two models agree.
- Effective-layer severe parameters on the full pressure ladder, up to 100 hPa,
  so the depth-dependent ones exist on the days they describe.
- Soundings fetch the 150 and 100 hPa levels, so a parcel has an equilibrium
  level instead of running off the top of the profile.
- CI opens the built web bundle in a real headless browser — the check
  `cargo check --target wasm32` structurally cannot perform.
- Saved workspaces appear in the command palette without "Show all"; CSV export
  writes a file, not just the clipboard.
- Android: the dialog shim can reach the activity handle again.

## 0.7.0 - 2026-08-09

- GPU wind particles, advected in a fragment shader with the CPU mesh kept as a
  fallback (`HOOKECHO_CPU_WIND=1`).
- Packaging: Flatpak, Snap, Homebrew and winget manifests, AppStream metadata,
  a full icon theme, and an experimental macOS app bundle built in CI.
- Android imports files through the Storage Access Framework.
- Workspaces remember field layers, and three starter layouts ship with the app.
- `--snapshot` writes a radar PNG for desktop widgets; `/snapshot.png` takes
  size and zoom.
- A Storage tab that shows and clears every disk cache.
- Live chunk streaming in the browser, so a web tab updates sweep by sweep.
- Accessibility: the widget tree is published to screen readers, plus a
  high-contrast palette.
- True lunar phase (Meeus) instead of the mean synodic month.
- Soundings: effective-layer parameters on the clicked profile, and CSV export
  of the profile and its indices.

## 0.6.0 - 2026-08-09

- Saved workspaces: pane layouts stored and restored.
- Archived Level 2 volumes and zone geometry kept on disk across restarts.
- GFS and ECMWF global model fields.
- Forecast-hour soundings with a full parcel, lapse rates and effective-inflow
  parameters.
- User-drawn watch zones that fire when a warning polygon enters them.
- 3D: reflectivity threshold, axis slicing, and off-thread volume building.
- Chase pack street tiles beside raster imagery; wildfire feeds fully paged.
- Container image published to ghcr.io; `hookecho://` URL scheme registered.
- Performance: parallel site switches, faster live-stream start, fewer per-frame
  allocations.

## 0.5.0 - 2026-07-25

- Time-height wind profile (VWP) panel.
- WPC surface analysis fronts overlay.
- Rain-arrival ETA for saved locations and your chase position.
- Tap anywhere for the NWS point forecast.
- New warnings read aloud.
- Cross-sections slice velocity and correlation coefficient, not just
  reflectivity; compare-4-tilts pane preset.
- First-run tutorial rebuilt around the app as it is now.

## 0.4.0 - 2026-07-20

- Android port: the same codebase as a NativeActivity APK (arm64-v8a).
- Per-pane product picker for multi-pane layouts.

## 0.3.0 - 2026-07-19

- Hydrometeor classification, live and archived local storm reports, an AFD
  viewer, and hail sizing.

## 0.2.0 - 2026-07-19

- Warning intelligence, archived warnings, probabilistic outlooks, and
  CAPE/SRH environment overlays.
- Live-loop playback, selectable alert sounds, marker icons, more basemaps.

## 0.1.0 - 2026-07-18

- First release: Level 2 / Level 3 NEXRAD viewing on a `wgpu` + `egui` map,
  with archive replay and NWS alerts.
