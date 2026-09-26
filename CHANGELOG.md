# Changelog

Notable changes per release, newest first. Every tagged release's body on GitHub
is this file's matching section, extracted by `.github/workflows/release.yml` —
so **write the section before pushing the tag**, or the release job fails.

The rolling `latest` release tracks `main` and is not listed here.

## Unreleased

### Added: full FLASH rainfall-recurrence windows

FLASH QPE average recurrence interval now has 30-minute, 1/3/6/12/24-hour and cross-window
maximum layers, with a compact picker in the workstation and shared layer options. The original
`flashflood` saved ID remains the 30-minute product. Names and the legend distinguish rainfall
rarity in years from a direct flood forecast. The live MRMS contract confirms all 53 paths.
Headless field checks now report when a current grid has no valid cells instead of printing an
invalid numeric maximum.

### Fixed: MRMS echo-top legend across thresholds

The shared km MSL echo-top legend now uses a threshold-neutral title, so 30/50/60-dBZ layers no
longer display an incorrect 18-dBZ legend heading.

### Added: complete MRMS rotation-track windows

Low-level (0–2 km AGL) and mid-level (3–6 km AGL) rotation tracks now offer the published
30-minute, 1/2/4/6-hour and 24-hour windows. A compact window control applies to both bands,
and changing it refetches any loaded tracks. The mid-level band has its own saved layer ID and
the same shear scale. The live catalog contract confirms all 47 MRMS paths.

### Added: national reflectivity at temperature levels

The MRMS browser now includes reflectivity at the 0, -5, -10, -15 and -20°C environmental
isotherms. A compact level picker switches the active pane among them while each product retains
its own saved layer ID, valid time and dBZ palette. The live feed contract covers all 38 MRMS paths.

### Added: full MRMS echo-top threshold set

The national echo-top layer now has 18, 30, 50 and 60 dBZ variants, all in km MSL, with a compact
threshold chooser in the workstation Satellite tab and shared layer options. Each variant keeps
its own layer ID and provenance. The live catalog contract confirms all 33 MRMS paths.

### Added: national MRMS 18-dBZ echo tops

The MRMS catalog now offers a national 18-dBZ echo-top layer with its own km MSL legend,
provenance, sampling and source health. The live catalog contract confirms its public feed path.

### Improved: source-health indicators in workstation Layers

The seven feed states now have different shapes and accessible names in the compact Layers tree,
so status is visible without relying on color alone.

### Added: MRMS rain-total window picker

Choose a 1, 3, 6, 12 or 24 hour QPE accumulation from the workstation's Satellite tab or the
shared layer options. The picker switches the active pane to one window while preserving the
existing layer identifiers used by saved workspaces and headless commands.

### Improved: direct access to source health

Click the radar health and delay readout in the WSV3/Dock app bar to open the consolidated Data
source health window. Its hover text still distinguishes current frame delay from lag at receipt.

### Improved: keyboard search in the workstation Layers window

Enter now runs a typed time command or opens the first visible search result. If no layer, site or
tool matches, it looks up the text as a place. Search results refresh as the text and filters change.

### Fixed: live radar delay in the workstation bar

The WSV3/Dock app bar now shows how far the newest radar frame is behind the current time and
updates the figure every second. Archive viewing is labeled separately. The hover detail still
reports the lag measured when the last live frame arrived.

### Changed: model, MRMS and satellite data are kept once downloaded, on the web too

A model run's GRIB messages, an MRMS minute and a GOES scan never change once they're published.
They're now kept after the first download and read back from then on, including after a reload
in the browser, which used to download them all again. The desktop and phone keep them on disk
(the GRIB cache folder moves to `objects/` and is renamed rather than refetched), and the browser
keeps them in IndexedDB. Each source has its own size limit, and the least recently used entries
go first. Every file is checked before it's kept and again when it's read: GRIB framing and
length, the gzip checksum for MRMS, the declared file length for GOES. A damaged copy is simply
downloaded again. The Storage tab shows each cache, on the web as well.

### Added: broadcast dressing for streams and rendered frames

Streaming mode (F8) can now dress the map for air: a clock in the radar's own time zone, a
source caption, a warning crawl of the warnings in force at the frame's time, a logo, and a
title-safe margin that all of them keep inside. The colour scale can be hidden. Set it up under
Preferences → Display → Streaming overlay. `--watch` draws the same into its frames:
`--broadcast` turns on the margin, clock and crawl, and `--safe-margin`, `--clock`, `--crawl`,
`--logo`, `--no-legend` and `--no-caption` set them individually. `--transparent` renders just the
radar over a transparent background, for laying over other video. City labels no longer land under
the caption, colour bar or clock.

### Added: fixed-size stills and timed loops from `--watch`

`--watch` renders at a fixed size whatever the screen: `--size PX`, `--frame WxH`, or `--preset
1080p`, `1440p`, `4k`, `portrait` or `social`. Wide and tall frames keep the caption and colour
bar inside them. Stills are PNG, JPEG or WebP, chosen by the file extension. A `--from`/`--to`
range written to a `.gif` or `.mp4` becomes one loop. By default each frame is held for the real
time until the next scan, scaled to `--fps`; `--interval fixed` holds every frame the same. The
loop's sidecar JSON lists every frame's volume, valid time and hold. The app's own loop export
now uses real scan timing too (switchable in Share preferences) and writes the same sidecar.

### Changed: everything is reachable from the Dock layout

The Dock (ImGui) layout used to miss things only the floating panel or the ribbon offered. It now
has an Alerts window (the bell in the app bar, with the count in view) and a Preferences window
with the panel's map settings and app preferences: display, streaming mode, location, weather
radio, sharing and export, backup. Tools, Share, Settings and Help in the app bar are now menus
covering every window, sharing and workspaces. Every map tool is on the rail, including radar
suitability, tornado climatology and the chase location. Ctrl+K opens the Layers search, which
now takes time commands ("time 21:30Z") and can fly to a place. The toolbar adds storm-relative
velocity, the scan-strategy details, the legend, a map-style picker and pane count and
arrangement.

### Changed: WSV3 is the analyst workstation, map-first; tool windows dock and float

The WSV3 layout now uses the workstation described below instead of its three-row ribbon. It
opens map-first: the two top bars, the tool rail and the timeline, with the Layers and Inspector
windows opened when you want them. Layers and the Inspector can dock left, dock right or float
over the map, fold to their title bar, and close. Each layout remembers its arrangement across
restarts, and a saved workspace restores it. The Inspector adds the Nyquist velocity, quality
notes (range folded, dealiased), where the data came from (volume file and provider) and, in 3D,
the camera's pitch, bearing and zoom. The rail gains Layers and "center on the radar", and T hides
the top bars for a full-window map. WSV3's zoom quick-pick row is gone. The Command Ribbon layout
is unchanged.

### Changed: the Dock layout is an analyst workstation

The docked layout ("Dock (ImGui)") is redesigned after Dear ImGui tool panels and WSV3. The
top is now an app bar of workspace tabs (Radar, Models, Satellite, Surface, Analysis, GIS),
panel buttons, the clock and the radar feed's health with its ingest lag. Under it sits a
toolbar of radar controls: site, product, tilt, follow-lowest, 2D/3D/volume, smoothing, colour
table, overlays and map style. The Layers panel gains All / Active / Favorites filters and a
favourite star per layer, and searching reaches every tab. The right-hand panel is replaced by a
floating Inspector card that reads the value under the pointer in the colour it is drawn, with
azimuth, range, beam height and sample time, and can pin a reading. The timeline gains a frame
track with hour labels and downloaded frames marked, a Live/Archive pill, a jump-to-time field,
tilt dots with the live sweep ringed, and a buffer bar. The tilt bar folds into the toolbar and
timeline. On a borderless window the clock no longer sits under the window buttons, and a
narrower window drops button labels before anything overlaps. Every control is still the same
action as in the other layouts. See `docs/WSV3_IMGUI_MODERN_DESIGN_PLAN.md`.

### Added: `--watch`, automated radar output

`hookecho --watch --site KTLX --out radar.png` keeps a radar PNG current: it polls the radar's
volume list and renders only when a new volume arrives, writing the PNG and a `.json` sidecar
(site, product, tilt, the exact volume and its valid time) beside their targets and renaming them
into place, so a web page never reads half a file. `--every`, `--once`, `--product`, `--tilt`,
`--size`, `--zoom`, `--center LON,LAT` and `--basemap` shape it; `--workspace NAME` renders a saved
workspace's view; `--time` renders one archived instant and `--from/--to` every volume in a range.

### Added: a local API for the running app

Turn on **Share → Local API** and other programs on this computer can read what HookEcho is
showing at `http://127.0.0.1:47914/api/v1`: the panes (site, product, tilt, volume and its
time, camera), the detections on the active volume, the warnings on the map, feed health, the
volume's products and frame list, every moment at a point (`/sample?lat=&lon=`), a PNG of the
window, and a Server-Sent Events stream that fires when the displayed volume changes. Off by
default and loopback only; requests must be addressed to 127.0.0.1/localhost and no CORS header
is sent, so a web page cannot read it. See `docs/local-api.md`.

### Added: CF/Radial export of a radar volume

"Export volume (CF/Radial)…" under Share writes the active pane's volume — every tilt, every
moment — as CF/Radial 1.4 NetCDF, the format Py-ART, LROSE/Radx and wradlib read, with per-ray
time/azimuth/elevation, sweep indices and fixed angles, and the radar's location.
`--headless-cfradial SITE DATE HH:MM out.nc` does the same for an archived volume. It is the
app's binned 8-bit volume, exactly what the displays and detectors worked from, not the raw
Level II moment words.

### Added: NetCDF export of gridded layers

"Export grid (NetCDF)…" writes the same grid as the GeoTIFF export as CF-1.8 NetCDF (the classic
format): lat/lon coordinate variables at cell centres, the valid time, the field with its units
where known, NaN for missing — for xarray, Panoply, NCL or MATLAB. `--headless-mrms out.nc` writes
the latest MRMS reflectivity from the command line.

### Added: GeoTIFF export of gridded layers

"Export grid (GeoTIFF)…" under Share writes the active pane's top gridded layer — an MRMS mosaic,
a field derived from the radar volume such as VIL, echo tops, MEHS or POSH, or a model field — as
a float32 GeoTIFF in EPSG:4326 with no-data declared, which QGIS, ArcGIS, GDAL and rasterio open
directly. The analysis export now includes it as `grid.tif`, and `--headless-mrms out.tif` writes
the latest MRMS reflectivity from the command line.

### Fixed: every MRMS layer sat half a cell north-west of where it is

The MRMS GRIB decoder took the grid's first and last *points* (cell centres) as its outer edges,
while the map, the probe and every sampler treat those bounds as edges with centres half a cell in.
So each MRMS layer was drawn and probed about half a kilometre north-west of its true position,
with a cell of 0.0099986° instead of 0.01°. The bounds are now widened by half a cell each way;
the CONUS mosaic's edges come out as the round 130° W / 55° N the product is defined on. NOHRSC
snowfall shares the decoder and is fixed with it. Found by checking the new GeoTIFF export's
georeferencing against the product's published grid.

### Added: analysis export

`Export analysis…` (under Share) saves one ZIP for other tools: the map as PNG, the case file,
annotations as GeoJSON, `provenance.json` naming the exact NOAA volume, scan time, VCP and tilt
behind every pane along with the detector versions and melting level in use, the active volume's
detections as CSV, and whichever probes are open — region statistics, the gate inspector's
profile and time series, the cross-section — as CSV. A README inside lists them.

### Added: case-study packages

`Save case…` (under Share) writes a small JSON file that reopens the current analysis anywhere:
every pane's radar, product, tilt, camera and layers, the analysis instant with an hour's replay
around it, and the bookmarks, markers, watch zones, drawings and user-defined products you have.
`Open case…` restores the panes, sends each one to that instant with the replay window, turns on
warnings and storm reports as an event replay does, and adds the case's annotations and bookmarks
to yours (skipping any you already have). Radar data is not packed; it is public and refetched.

### Added: vertical profile and time series in the gate inspector

A click with the gate inspector now also shows the whole column at that point: every tilt, top
first, with each moment against beam height, the displayed moment drawn up the column with the
melting level marked when one is known, and the column as CSV. The inspector was already sampling
every tilt there for user-defined products; this puts those numbers on screen.

It also shows the displayed moment at that point across the loop the pane has loaded — every
volume it holds, oldest first, at the tilt nearest the displayed one — as a sparkline with its span
and range, and as CSV. Built on a click only; the hover probe that shares this code does not pay
for it.

### Added: region statistics — box a storm, compare every moment gate by gate

A new map tool, "Region statistics" (Tools, the dock, the ribbon and the command palette): click two
opposite corners and every gate of the displayed tilt inside the box is read in every moment at the
same place. The window shows each moment's spread (gates, min, 10%, median, 90%, max, mean), a
histogram of any one, and a scatter plot of any two with their correlation, with one-click pairs
for REF–ZDR, REF–CC, ZDR–KDP and VEL–CC. Every gate exports as CSV, and
`--headless-region SITE DATE HH:MM LON1 LAT1 LON2 LAT2 [out.csv]` does the same from the command
line. On Moore's debris ball (KTLX, 20 May 2013) it reads REF against CC at r = -0.27 — the
stronger the echo the lower the CC, which is debris — where a rain box nearby reads +0.21.

### Changed: detector confidence floors now default to 60% (debris) and 50% (rotation)

Both floors — what a detection needs to be drawn, count for alert rules, or raise an alert —
defaulted to 0%, so every weak candidate showed and chimed. They now start at the values the
archived-event backtest supports: debris at 60%, which found as many of the 37 tornado reports as
50% did (49%) at a FAR of 51% instead of 70%; rotation at 50%, which keeps 65% of the reports
(60% would drop to 53%) since rotation is the earlier warning and worth erring toward. A settings
file saved while 0% was the default carries an explicit 0 that was almost never chosen, so each
floor still at exactly 0 moves once (`Settings::adopt_detector_floors`); any other value, or a floor
put back to 0 afterwards, is left alone. "Reset detector thresholds" resets to the new values, and
each slider's hover says where its default comes from.

### Fixed: four tooltips had stray line breaks and indentation

The rotation-confidence slider, the full-range 3D volume toggle and two map-horizon settings had
`\n` plus a run of spaces inside their hover text where a string continuation was meant, so each
rendered with a break and a block of indentation mid-sentence.

### Changed: debris with no couplet beside it scores lower (`tds-6`)

Large wet hail at S band can look exactly like debris: low CC, high Z, ZDR near 0. The Denver
hailstorm of 8 May 2017 raised dozens of debris signatures with no tornado, up to 78%, and
spectrum width, MEHS over the hit and the share of near-zero velocity gates were each measured
against real debris balls and did not separate them (the foothill hits are moving storm echo, not
ground clutter). What did: real balls sit beside a couplet, the Denver hits never did. So when
velocity was scanned and no credible couplet sits within 5 km, a signature within the rotation
range keeps `NO_ROTATION_FACTOR` (0.8) of its confidence — a discount, not a veto, since the
rotation detector misses some real couplets. With no velocity there is nothing to hold against it.
The hover breakdown says which applied.

On the backtest (POD / FAR / CSI, before → after): at 50% 49/79/11 → 49/70/19, at 60% 49/69/19 →
49/51/29, at 70% 49/53/28 → 46/38/35, at 80% 49/31/38 → 46/32/38. CSI is equal or better at every
threshold; the cost is one tornado of 37 at the top two floors (Nashville, whose couplet the
rotation detector places 6.5 km from its debris ball). Detector alert floors default to 0%, so
out of the box this only reorders what shows; it is the minimum-confidence setting it pays off in.

### Fixed: a debris signature could be placed kilometres from its own tornado (`tds-5`)

Debris hits from different tilts are grouped by single linkage at 3 km, and a group's position
and evidence were the gate-weighted average of every member. A violent tornado's debris ball can
sit at the edge of a large field of weak, fragmented low-CC echo, and linkage walks from the ball
through every fragment. On Mayfield, KY (KPAH, 11 Dec 2021, 03:30Z) that chained the real ball
(min CC 0.29, 59 dBZ) to 158 fragments over 25 km: the reported signature sat 12 km north of the
tornado at 72%, with its CC and Z averaged toward the fragments', and never paired with its own
72 kt couplet. A group is now described by its strongest core — the strongest member and what
is within 3 km of *it* (`tds::strongest_core`) — while staying one detection. Mayfield now reads
93%, on the ball, with the couplet beside it.

Across the eight tornado-backtest events (below), with the before/after on the same harness:

| min conf | debris POD / FAR / CSI, before | after | rotation, before | after |
|---|---|---|---|---|
| 60% | 49 / 72 / 17 | 49 / 69 / 19 | 53 / 55 / 33 | 53 / 52 / 34 |
| 70% | 43 / 60 / 23 | 49 / 53 / 28 | 47 / 48 / 34 | 51 / 47 / 38 |
| 80% | 43 / 33 / 34 | 49 / 31 / 38 | 14 / 64 / 12 | 19 / 58 / 16 |

Rotation improves too: correctly placed debris now corroborates the right couplets.

Tried first and not shipped: grouping around anchors instead of by linkage, which fixed the
position but reported every fragment as a debris signature of its own (1054 detections instead
of 678) and raised FAR at 50-60%. Measured and not used as a hail/debris discriminator: spectrum
width (Mayfield's ball reads 1.9 m/s, inside the Denver hail false alarms' 1.2-3.2) and MEHS over
the hit (Moore's real ball sits under 20 mm of it, several Denver false alarms under none at all
— those are the Front Range foothills' terrain clutter, not hail).

### Fixed: backtest windows and truth sets

- Four of the eight tornado events in `docs/backtest-events.txt` had start times typed from
  memory and wrong by one to six hours — Washington IL, Mayflower/Vilonia AR, Joplin MO and
  Nashville TN — so their 8-volume windows held no tornado. Each is now a few minutes before the
  first local tornado report, checked against the LSR archive.
- Reports are now only scored when some volume was actually scanned within 15 minutes of them.
  KPAH's archive for Mayfield jumps from 03:58Z to 07:16Z, and the three hours between — about 20
  tornado reports, no radar data — were all counted as misses.

Every backtest number published before this entry was computed with both problems; the table
above is the first on the corrected set.

### Added: `--headless-tds-archive` shows each hit's ZDR and the MEHS over it

With the day's melting level from the observed sounding, and `wxdata::derived::mehs_at` for a
point MEHS without computing the whole grid. It is how the hail/debris question above was checked.

### Added: hail backtest, and hail grids on archived volumes

`--headless-backtest` / `--headless-backtest-file` now score the MEHS/POSH hail algorithm as well as
the two tornado detectors. Each archived volume's hail grids are reduced to discrete cores
(`wxdata::derived::hail_cores`) and matched against severe (≥ ¾ in) hail reports within 10 km and 15
minutes, in the same table, range split and missed-report list. On the Denver hailstorm of 8 May
2017 (KFTG, now in `docs/backtest-events.txt`) it found 22 of 25 reports; FAR fell from 85% to 60%
as the minimum POSH rose to 80%.

The hail algorithm needs a melting level, and the app only took one from the live HRRR analysis, so
hail grids were live-only. It now takes the observed sounding for the day
(`wxdata::raob::melting_levels`, with fallback to a neighbouring site or the previous launch) for
an archived volume, in the backtest and on the map, so MEHS/POSH work while scrubbing the archive.
The same change fixes a quieter bug: the gate inspector's UDP inputs and the ZDR-column pass read
the one cached melting level without checking which time it was for, so in archive mode they
silently used today's. Melting levels are now keyed by site *and* time (`App::freezing_for`).

### Fixed

- The RAOB station list was missing six active CONUS sites: Denver, Tucson, Newport NC, Wallops
  Island, Great Falls and Spokane. Anywhere on the Front Range took Grand Junction's sounding from
  across the Continental Divide.
- The POSH layer description said "hail an inch or larger"; POSH is the probability of hail ≥ 19 mm
  (¾ in), per the algorithm it implements.
- `almanac_line_has_both_events_in_the_tropics` read the wall clock, and within a few days of either
  equinox the sun really does rise at 89° N, so it failed every March and September. It now pins a
  solstice date.

### Added: cell console shows a severity trend

The cell console's trend history recorded VIL, echo top and peak reflectivity per volume but not the
composite severity score the storm-cells table ranks by, so there was no way to see whether a storm
was getting worse by the app's own measure. Each sample now carries that score too, and the console
draws it as a fourth sparkline (its own row and colour — it is a derived score, not a measurement).
It is computed when the cell product arrives from the same cached couplets the table reads, never
by triggering a rotation pass of its own, so the trend's last point is always the table's number.

### Added: TDS/rotation markers show their score history on hover, not just the current number

The score timeline built last pass (`wxdata::scoretrack`) is now wired into the live map: hovering
a debris signature or a couplet shows the usual breakdown plus a sparkline of its confidence over
however many volumes it has persisted — the same "every term, its measurement, what each stage
added" tooltip, extended with what changed from scan to scan rather than just what the number is
right now. Built the same bounded-trailing-window way `compute_local_tracks` already builds cell
tracks (`compute_tds_score_track`/`compute_rot_score_track`, replaying `scoretrack::associate` over
the last 16 frames), and for the same reason — replaying a whole session's history every UI tick
does not scale.

Caught before it shipped: the first version cached *raw*, pre-corroboration hits for the replay
(reusing `tds_raw`'s existing per-volume cache, on the theory that it was already there) — which
would have shown a sparkline ending in a different number than the marker label right next to it,
since `cross_corroborate` runs after that cache is filled. Fixed by caching the *corroborated* hits
instead (`tds_shown_cache`/`rot_shown_cache`, filled once per volume in `compute_tds`/
`compute_couplets`, right after corroboration), so the sparkline's last point is always the exact
number on screen.

### Fixed: cell tracking could crash on a session's first volume with 3+ new storms

`wxdata::celltrack::associate` sized its "already claimed this volume" scratch vector to the
number of *existing* tracks up front, then kept searching the same list as it grew when a cell
started a new one mid-batch — so the third-plus brand-new cell in one call indexed past the
scratch vector's own length and panicked. Every session's first tracked volume with more than two
storms hit this, which is not a rare shape of input. Found while building the score-timeline
tracker below on the identical pattern, deliberately: `wxdata::scoretrack::associate` copied the
same greedy nearest-neighbour shape from `celltrack::associate` on the theory that a *proven*
pattern was safer to reuse than to reinvent, and the reproduction from one confirmed the same bug
in the other before either shipped. Fixed in both: a point is now only ever matched against tracks
that existed *before* the current call, not ones the same batch already started — which was also a
real (if quieter) correctness bug on its own, since two detections seen for the first time
together in one volume are two distinct features, never one recurring track just because they
happened to land close together.

### Added: score timelines — confidence tracked volume to volume, not just per-scan

`wxdata::scoretrack` follows each debris/rotation detection across a backtest's volumes the same
greedy nearest-neighbour way `celltrack` follows storm cells, and `--headless-backtest`/
`--headless-backtest-file` print the result: how many raw candidates turned out to be the same
recurring feature seen again (as against a one-volume blip), and the longest track's full
confidence sequence. This is the "timeline of score changes" C5's algorithm-lab list named — a
confidence that climbed steadily over four volumes and one that spiked once and vanished are the
same single-volume number with very different stories, and only a tracked history tells them
apart. On the Moore, OK event specifically: a debris signature tracked across all 8 volumes,
confidence 53% → 54% → 63% → 65% → 56% → 56% → 68% → 47%, and a couplet over the same span rising
sharply mid-event, 34% → 36% → 30% → 70% → 71% → 75% → 72% — consistent with the real tornado's
rotation intensifying partway through the window. Across the full 8-event backtest, most raw
candidates (106 of 140 debris, 69 of 109 rotation) turned out to be a recurring feature tracked
across more than one volume, not a one-off. Live map wiring (a sparkline on hover, say) is a
follow-up; this pass is the tracker itself, proven against real archived multi-volume data.

### Fixed: archived Tornado Emergencies read as plain warnings on the scrubbed timeline

`wxdata::archive_warnings::parse` hard-coded `tornado_detection`/`damage_threat` to `None` for
every warning pulled from the archive, since the IEM `sbw.py` service's own `tornadotag`/
`damagetag` fields are usually empty even for a real Tornado Emergency. `wxdata::alerts::escalation`
— which sorts the alert panel, colors badges, and gates the emergency sound and (now) backtest
confirmation — never saw past tier 0 for anything pulled from the "time machine" archived-warnings
layer as a result. The archive does reliably carry `is_emergency`/`is_pds` as structured booleans
even when the tags are empty; `parse` now folds them into the warning's own text the same way
`escalation` already reads a live product's headline, so a scrubbed Tornado Emergency escalates,
colors and sounds exactly as it would live.

### Added: the backtest scores against observed tornado warnings too, independent of LSR/DAT

A third, independent line of evidence alongside the LSR-report and DAT-survey tables: whether a
detection sat inside a tornado warning marked observed (or a Tornado Emergency) at its own volume,
fetched per volume from `wxdata::archive_warnings` (previously only used for the live map's
scrub-the-timeline overlay, never for a backtest sweep). Reuses `wxdata::confirm`'s existing
OBSERVED-only semantics rather than inventing new ones — an *ordinary* warning is not ground truth
a detector should have "found", since most are issued from the same radar signatures the detector
itself reads, so this only ever counts detections that already cleared that bar. Printed as a
compact "N of M detections inside one at their own volume" line, not folded into the POD/FAR tables
above it, since it is answering a different question (does independent human confirmation back this
specific detection) than they are (did the detector find every real tornado). A full 8-event
backtest run found 47 of 453 debris detections and 13 of 321 couplets landed inside an observed
warning, with no crashes across the pre-dual-pol and non-tornado-day events in the set.

### Added: the backtest scores against NWS damage surveys too, not just local storm reports

`--headless-backtest`/`--headless-backtest-file` now score each detector against
[NWS Damage Assessment Toolkit](https://apps.dat.noaa.gov/StormDamage/DamageViewer/) surveyed
tracks alongside local storm reports — "the only layer in this app that says what the storm
actually did" (`wxdata::dat`'s own doc comment), rather than a sighting logged when seen or
surveyed. Printed as a second "vs DAT surveys" block under each detector's existing "vs LSR
reports" one, sharing the same per-threshold, by-range and missed-report tables (factored the
whole block into `score_and_print` so scoring a second truth set costs a function call, not a
second copy of the code).

One truth per surveyed *track*, not per damage point: the service records one point per damage
indicator along a path — every damaged building, every snapped tree — and a single tornado's track
can carry thousands of them. An early version of this scored Moore, OK as "3767 tornadoes" before
that was caught in testing and fixed; a track is the unit that actually means "a tornado", so a
track is what gets scored, one `Truth` at its path's midpoint. A real 8-event backtest run
afterward found DAT tracks for 7 of the events' tornadoes (down from the 3767-point version's
runaway count), broadly agreeing with the LSR-based numbers on the same run.

### Added: cell severity scores explain themselves on hover, same as the TDS/rotation markers

`wxdata::cellscore::severity_explain` breaks a cell's 0-100 score down into the same shape
`TdsHit::explain`/`CoupletHit::explain` already return — reusing their `Reason` type rather than a
third copy of it — with a `lines()` method for a plain-text breakdown. `severity` itself is now
just `severity_explain(..).score`, so the two can't drift apart, and `score_all_explained` joins a
whole cell list at once, with `score_all` built on top of it the same way. The storm-cells table's
detail panel shows it on hover over the severity score, naming exactly which of probability,
rotation, hail and the radar's own TVS/MESO flag pushed the number where it landed.

### Fixed: the backtest scored every archived event against tornado reports from the whole country

`wxdata::lsr::fetch`'s window is a national feed, not a local one: an outbreak-day backtest pulled
every US tornado report in that UTC window, most of them a different storm hundreds of miles from
the radar being tested, and counted every one as a report the detector should have found. This
understated POD for every event `--headless-backtest`/`--headless-backtest-file` has ever scored —
"6 of 75 tornado reports found" over the 8 events in `docs/backtest-events.txt` was really 6 of 32
*local* ones, most of the other 43 belonging to unrelated storms elsewhere in the country at the
same UTC time. `backtest_event` now captures the radar's own position off the first sweep that
decodes and drops reports beyond 160 km (both detectors' 150 km max range, plus the match radius) of
it before scoring. FAR is unaffected (it never depended on the report count), but POD and CSI roughly
double at every confidence threshold once the report set is scoped to what the radar could plausibly
have seen. Caught by the new "by event, reports missed" line below, which put KSGF's fetched reports
(all in Wisconsin and Illinois, nowhere near Joplin) in front of a human for the first time.

Also added, on the same finding: `wxdata::detverify::unmatched` names exactly the reports with no
matching detection, rather than just counting them, and `--headless-backtest`/
`--headless-backtest-file` print them (location and time) per event at 0% confidence — an aggregate
found/reported ratio can hide *which* report a change in the detector cost or gained; this names it.

### Improved: rotation's gate-to-gate floor scales up with range, cutting far-range noise ~75%

Follow-up to the range-by-range backtest below. The 25 m/s gate-to-gate floor is now
[`wxdata::rotation::range_floor_scale`]d up to 2.5x by 150 km, past the same 60 km point the
confidence score already discounts from: the physical arc between two adjacent azimuth gates grows
in direct proportion to range with a fixed bin count, so the same *true* rotation produces a smaller
measured velocity difference the farther out it's sampled, and treating that as an unmoving 25 m/s
line, the way this detector always has, just makes it a coarse-sampling noise generator past 60 km.
Near-range detection (< 60 km) is untouched — the floor stays exactly 25 m/s there, since the
backtest evidence never questioned it.

Re-run over the same 8 events, with the local-reports fix above already applied: far-range raw
candidates dropped from 1001 to 249 (a 75% cut, with the false-alarm ratio in that band easing from
98% to 96%), and the total tornado reports matched by rotation across all 8 events fell by exactly
one, from 8 to 7 (out of 32 local reports). Confirmed *not* to be the Joplin/KSGF case: once its
reports are scoped to the radar's own coverage, KSGF has none in range at all for this window — a
gap in that archived window's report data, not a detector failure — so it was never in the 32 to
begin with. `unmatched`'s new per-event output named the actual one: a report at 42.69,-90.83 near
the Iowa/Illinois line at 17:32Z during the KDVN derecho event, whose gate-to-gate shear fell under
the raised far-range floor. Scoring version `rot-5`.

### Added: `--headless-backtest` scores raw candidates by range, found the rotation detector is 93% far-range noise

`wxdata::detverify::score_in_range` scores a detector's candidates *before* any confidence filter,
split by range at 60 km (where every confidence score already starts discounting for distance) — so
the discount can't hide whether the underlying detection criterion, not just the score, holds up at
range. `--headless-backtest`/`--headless-backtest-file` print it as a "by range" line under each
detector's table.

Run over `docs/backtest-events.txt`'s 8 events: debris signatures are close to range-neutral (92% FAR
inside 60 km, 95% beyond). Rotation couplets are not — 93% of every raw candidate (1001 of 1073)
comes from beyond 60 km, and those far-range candidates are both noisier (98% vs 89% FAR) and no
better at finding real circulations, which is exactly the far-range shear-artifact problem
`ROADMAP_NEW.md`'s C5 section has flagged since before this session and could not previously back
with numbers. See that section for the evidence table and the proposed fix.

### Improved: both tornado detectors now weigh depth, rooting and which way rotation turns

Three gaps in the TVS/TDS scoring, all of them the operational criterion the detectors were named
for and did not actually apply.

**Rotation aloft is no longer rotation on the ground.** A couplet or a debris column is a *low-level*
signature. Both detectors now record where the column starts as well as where it ends (`base_km`
beside `top_km`), and whether it reaches the lowest low-level tilt the volume scanned. One that does
not keeps 70% of its vertical evidence: for rotation that is a mid-level mesocyclone, which precedes
the great majority of tornadoes it never produces; for debris it is wet or melting hail aloft, the
commonest structure a CC-and-Z detector mistakes for a debris ball after high ZDR. It discounts
rather than rejects, since the lowest beam can be blocked by terrain or attenuated through the core,
and nothing is deducted when no low tilt was read at all. The alert banner and the map label say
"aloft" when it applies; the hover explanation shows the whole span and the rooting either way.

**Couplets now report which way they turn.** Radar azimuth increases clockwise, so radial velocity
rising across a couplet is counterclockwise -- cyclonic north of the equator, anticyclonic south of
it, and the detector reads the radar's own latitude rather than assuming. Tornadoes turn cyclonically
almost without exception, while an anticyclonic couplet is exactly the shape an ordinary shear zone,
a dealiasing failure and the anticyclonic half of a splitting storm all make, so an anticyclonic one
now scores 70% of what the same rotation cyclonic would. It only ever discounts: the cyclonic sense
is shared with every mesocyclone that produces nothing, so it earns no credit of its own, and a
strong anticyclonic couplet is still shown, labelled.

**Fixed: a couplet seen at two tilts could be scored as two single-tilt couplets.** The volume pass
merged each tilt's already-clustered couplets by snapping them to a ~4 km grid, so two tilts' views
of one circulation -- a kilometre or two apart, as they always are -- were split whenever the pair
straddled a cell edge. Each half was then held to the single-tilt confidence cap of 50%, losing
exactly the vertical evidence the volume pass exists to find. Couplets now associate by ground
distance, using the same union-find helper the debris detector already used, which is now shared
between them instead of written twice.

Scoring versions are `rot-3` and `tds-4`.

### Improved: debris and rotation now corroborate each other both ways

A debris signature beside a couplet already raised the debris score (`tds::corroborate_with_rotation`);
a couplet beside a debris signature gained nothing back, though debris on the ground -- an actual
physical object, not just more radar-measured shear -- is the strongest single piece of evidence a
couplet can have. `rotation::corroborate_with_debris` adds the missing direction, and
`tds::cross_corroborate` runs both safely: it snapshots each side's own confidence *before* either
function runs, so a couplet already boosted by debris can never be read back to boost that same
debris signature, which would count the same evidence twice under two different names. The three call
sites that used to corroborate by hand (`--headless-tds-archive`, `--headless-backtest`, and the live
TDS/rotation layers) now go through it.

In the app this meant the TDS and rotation caches had to stop holding corroborated hits, since a
cache that already includes the other detector's boost would feed that boost back into the next
corroboration pass. Both now cache raw (single-source) hits, and cross-corroboration runs fresh from
them on the way out of `compute_tds` and `compute_couplets` -- cheap, since it is O(hits × candidates)
over a handful of each. The rising-edge chime and banner (which need the corroborated confidence, not
the raw one) moved with it, from `compute_tds_uncached`/`compute_couplets_uncached` into the two
now-corroborating callers; the two `_uncached` functions are just the per-tilt gate scan now.

Scoring version `rot-4`.

### Fixed: the dock search hid its own matches, and multi-day backtest wiring

The dock layers panel expands matching categories only the first time you search: after that a
category kept whatever open/closed state you last left it in, so a search that matched something
inside a category you had collapsed showed a "(0/1)"-style count with no rows under it. Every
category a search leaves standing now opens, since `group_entries` already dropped every category
with no match in it - a search never again hides the very thing it found. Clearing the search leaves
your open/closed choices alone. The docked tool rail also got a little more breathing room by its
panel edge.

`--headless-backtest-file <events.txt> [volumes]` runs the detector backtest over a list of
archived events and totals them (one line per event: `SITE YYYY-MM-DD HH:MM`; see
`docs/backtest-events.txt`) - the multi-day version of last session's single-event
`--headless-backtest`. It was written but never wired into the CLI; fixed.

### Improved: debris signatures read differential reflectivity

Debris is a jumble of random shapes, so its ZDR sits near 0 dB; low CC beside a high ZDR is mixed rain,
large drops or a melting layer, the commonest thing a CC-only detector mistakes for a debris ball. The
mean ZDR around each signature now discounts its score (none up to 1 dB, down to 60% by 3.5 dB), shows in
the hover explanation, and never raises a score, since dry hail is near 0 too. On the 2013-05-20 volumes the
Moore debris ball is untouched at 91% while neighbouring signatures over mixed precipitation drop about ten
points. A volume with no ZDR is simply not discounted.

### Fixed: false couplets from scan seams, leftover folds and tower clutter; confirmed detections

A live sweep showed hundreds of rotation couplets in a straight line out from the radar. Three causes,
three guards. **Seams:** radials scanned a pass apart (the edge of a partial sweep) are no longer compared,
and a radial pair that disagrees along much of its length is discarded as a seam or bad radial, not
rotation. **Leftover folds:** dealiasing leaves some velocity jumping between about +Nyquist and -Nyquist;
those pairs (both sides at the limit, a difference of about two Nyquists) are dropped, using the Nyquist
velocity the sweep now carries. Real strong rotation past the limit is untouched: on the 2013-05-20 Moore
volumes the couplets fell from 96 to 33 and from 91 to 13, and the tornado still reads 99 kt beside its
debris ball. **Tower clutter:** debris signatures ignore gates within 3 km of the radar and are discounted
out to 15 km, where low CC in strong echo is clutter and sidelobes. At most 40 couplet markers draw at once.

**Confirmed detections.** A debris signature or couplet near a tornado report (within 10 km and 30 minutes)
or inside a tornado warning marked observed is tagged REPORTED, OBSERVED or, with both, CONFIRMED, with a gold
ring and a line in its hover explanation. This is a tier above the radar score, not part of it: the detector’s
0-100% is unchanged, the confidence slider never hides a confirmed detection, and confirmed ones sort
first. A plain radar-indicated tornado warning does not count, because it is issued from the same signatures
the detector sees. Reports now load when either detector layer is on. The report backtest still scores the
radar alone.

### New: score the detectors against tornado reports

`hookecho --headless-backtest <SITE> <YYYY-MM-DD> <HH:MM> [volumes]` runs the debris and rotation
detectors over a run of archived volumes and checks every detection against the Iowa Mesonet tornado
reports for the window (within 10 km and 15 minutes). The table is by minimum confidence, so it shows what
the filter slider costs and buys: detections shown, how many were verified, and POD, FAR and CSI. The
scoring is a pure `wxdata::detverify` module, usable for any detector and any truth set. On the 2013-05-20
Moore volumes (one report) the top debris band, 80% and up, was 33% verified and the couplets were mostly
unverified, which is the measurement the confidence numbers had been missing; one storm is far too little
to tune against, and a tornado nobody reported counts as a false alarm.

### Fixed: model difference maps were shifted half a cell

Model-to-model and run-to-run difference layers read each grid as corner-registered, but the grids are
cell-centred, so every field was shifted half of its own cell (up to 12 km on a 0.25 degree model) and by
a different amount for each model. The difference is now taken between points that line up, on the
coarser lattice, and two lattices holding the same field difference to zero. Also, the ribbon's tilt
group is sized to the volume so the upper tilts of a 14-tilt scan are no longer hidden behind a scroll.

### Fixed: shortcuts a tablet keyboard can press, a tidier tablet default, and a 3D far-item filter

**Keys.** Several shortcuts lived only on keys a tablet cover keyboard does not have: tilt on Page
Up/Down, site on F3, reload on F5, the OBS toggles on F8/F9, fullscreen on F11, help on F1. Each now
also has an ordinary key: `,` and `.` step the tilt down and up, `F` finds a site, `U` reloads, `K` opens
command search (Ctrl+K cannot be typed on Android, which reports no modifier state), `/` shows the
shortcut list, `H` opens help, `O` and `B` are the streamer toggles and `` is fullscreen. A key table
you had already saved (opening the Hotkeys tab saves one) gains these too, without changing keys you chose
or taking one you gave to something else. Punctuation shortcuts now yield to a focused text field the way
letters do, so a comma in a marker name no longer tilts the radar. Where Android delivers a keystroke as
typed text with no key event, the character now fires its shortcut, once. The Hotkeys tab shows the last
key or character the app received, so a key the system swallows can be told from one that is unbound.

**Tablet layout.** A tablet still on the shipped ribbon layout is moved, once, to the docked layout: the
ribbon's groups overflowed the width and got clipped, the colour scale ran over the tilt row and the
timeline floated over the map. A layout you picked on purpose is left alone, and switching back sticks.

**3D map.** Settings, General, **Hide far-away items** stops drawing storm reports, lightning, sites and
other markers far out toward the horizon of a tilted map, where they piled up and floated in the sky.
On by default; the distance is a slider (multiples of the camera's distance to the map centre). The radar
and map are always drawn.

### Improved: rotation couplets are scored on evidence, explain themselves, and can be filtered

The rotation detector gets the same treatment as the debris signature. A couplet's confidence used to
be height and tilt count alone, so on a real volume the noise was ranked with the storm: about 90
couplets at a uniform ~50 kt, most 90-150 km out, many at 100%. It now weighs what the cluster itself
shows (gate-to-gate shear from 25 to 36 m/s, and how many gate pairs per tilt) faded by range to 60%
at 150 km, then scales that by vertical continuity, and one tilt never reaches more than 50%. Height
counts only once a second tilt shows a column. Every marker shows its confidence and explains it on
hover (strength, size, range, vertical, and the scoring version `rot-2`). **Minimum confidence**
(Layer settings, Detectors, with the Rotation layer on) hides couplets below a level and keeps them
out of the rotation alert and alert rules. Debris signatures are now corroborated only by couplets
that score at least 35% themselves, within 100 km, so noise no longer lends them credibility.

### Improved: tablet dock - a tilt bar, live-sweep marking, and docked tools

The dock layout had no way to pick a tilt short of the layer options and no way to see which tilt the
radar was sweeping. It now has a **tilt bar** under the map: every tilt as a finger-sized button with
the one on screen highlighted, SAILS/MRLE repeats marked, and All and Follow-low beside them. While
live, the tilt being swept right now gets a green outline and a chunk-progress strip, and a line at
the right says which sweep it is (`LIVE - sweeping 0.9 deg (3/14) chunk 2/3`), so the tilt you are
looking at and the one the radar is on are told apart. The map tool strip no longer floats over the
data, where a touch meant to pan dragged it around: it is a docked column against the map's left
edge, with larger buttons. Only the cursor readout still overlays the map, and only where there is
a mouse to read.

### New: hover a debris signature to see why it scored what it did

Each TDS marker now explains itself on hover: the four weighted terms (depth, contrast, core, size)
with the measurement behind each, the range factor, the vertical continuity (or that one tilt caps
it at 60%), and how many points a nearby rotation couplet added. The breakdown is built from the hit
with the same functions that scored it, and it is stamped with the scoring version (`tds-3`) that
future exports and backtests can cite. This is the first slice of the algorithm laboratory (C5).

### New: model data is cached on disk

Every model, RTMA and GEFS message is a range-read out of a file that never changes once its run is
posted, so it is now kept. Scrubbing back over a lead time, flipping between products and re-opening
a run read from disk instead of the bucket. Entries are namespaced by source, each family (HRRR,
GEFS, RTMA, other models) has its own 256 MB quota (64 MB on Android) swept oldest-read-first, and
Storage shows and clears each. A message is only kept at exactly the length asked for with GRIB and
7777 framing, and re-checked when read back, so a run still being written or a cut connection never
poisons it. The web build has no disk store and fetches as before.

### Improved: debris signature (TDS) detection, and a confidence filter

The detector no longer treats "low CC in strong echo" as the whole answer, because large wet hail,
biological scatter and clutter can produce that too. It now weighs what a debris ball actually looks
like:

- **Clusters follow the radar's own grid.** Candidate gates are joined by contiguity in azimuth and
  range (wrapping at north), so a ball is never split by an arbitrary map grid, and hits from
  different tilts merge by ground distance instead of by which grid cell they fell in.
- **A compact hole, not a broad low.** Each cluster is scored on how deep its correlation
  coefficient dips (its lowest gate and its average, so one outlier gate cannot carry it), how
  strong its core is, how big it is, and how much its surroundings stand out from it. Clusters
  larger than 120 km² are dropped as hail or scatter, and a dip in surroundings that were already
  low scores poorly.
- **Height only counts when tilts agree.** A lone hit far from the radar is kilometres up purely
  from beam geometry, which used to read as lofted debris. Vertical credit now needs the signature
  to repeat through more than one tilt, and one tilt alone never exceeds 60%.
- **Range discount.** Evidence fades to 70% by 150 km, where the beam is wide and a ball is a few
  gates.

Checked on the 2013-05-20 KTLX volumes: at 20:04, 20:16 and 20:30 UTC the top hit sits on the Moore
tornado's actual path at 75–85%, each seen through four tilts with CC down to 0.21 and reflectivity
to 60–69 dBZ, while the best of everything else is 70% or lower.

**Minimum confidence** (Layer settings → Detectors, with the TDS layer on) hides debris signatures
below the level you set. It also keeps them out of alert rules and stops them raising the chime and
banner, so setting it to quiet doubtful detections quiets their alerts too. Every marker now shows
its confidence, and the setting is remembered. It defaults to 0%, which shows everything.
`hookecho --headless-tds-archive <SITE> <YYYY-MM-DD> <HH:MM>` prints each hit with the evidence
behind its score, for tuning against a known event.

**Rotation corroboration.** A rotation couplet within 5 km of a debris signature raises its
confidence (up to 40% of the way to 100%, saturating at about 68 kt) and the marker shows `rot NNkt`.
It only counts within 100 km of the radar and only for a hit already at 50%, so it corroborates a
credible detection instead of promoting marginal ones or amplifying far-range dealiasing noise. No
couplet is never held against a hit: rotation detection has a 15 km minimum range. It is read quietly,
so a TDS layer doesn't chime for rotation you didn't turn on. On the 20:16 Moore volume the tornado
now reads 91% with 99 kt of rotation beside it.

Not used: ZDR and velocity. Rotation collocated with the signature would raise confidence further
and is the natural next step.

### Added: model verification against the RTMA (K1)

A new **Model verification…** window scores a forecast run against the RTMA analysis for the same
valid hours: pick the model (HRRR, RAP, NAM 3 km, NAM 12 km), the field (2 m temperature or
dewpoint), a run (or leave it on one about twelve hours back so the short leads all have an
analysis), and which leads to score. It reports, per lead, the bias (forecast minus analysis), MAE,
RMSE and correlation, and for an event threshold you set, the probability of detection, false alarm
ratio, critical success index and frequency bias, followed by a plain sentence such as "typically
off by 0.7°F (runs 0.2°F too high)". It can score the whole domain or only what is on screen.

Two details keep the numbers honest. Every cell is weighted by the ground it covers (a degree of
longitude is narrower at high latitude), and a forecast is only ever compared with an analysis of
exactly the same valid time, with the two grids registered on the same cell centres. The RTMA is
itself an estimate, so this measures agreement with it rather than with every station. As a check,
HRRR's temperature over about a million cells scored MAE 0.59 K at F+1 rising to 0.82 K at F+6, with
correlation 0.99.

Not yet covered: METAR and RAOB truth, MRMS for precipitation and reflectivity, timing error, and
fields beyond temperature and dewpoint.

### Added: 6-hour rain (QPF) probabilities from the GEFS (F7)

The ensemble layer and the point plume gain a **6-hour rain (QPF)** field, so you can map the chance
that a place gets more than a chosen amount of rain in six hours (it starts at half an inch), or the
ensemble mean, spread, extremes and percentiles of the rain total. It uses the same 31 members as
every other ensemble field. The GEFS only publishes a six-hour accumulation at leads that close a
window, so the forecast hour snaps to the next multiple of six for this field (and hour zero, where
nothing has accumulated yet, is refused rather than fetched to fail). Amounts are in millimetres, on
the same color scale as the rain layers.

### Added: GEFS ensemble plume at a point (F7)

Tapping the map now shows an **Ensemble plume (GEFS)** under the point forecast: the ensemble mean as
a line, with a band of one standard deviation either side, for 2 m temperature, MSLP, 500 hPa height,
CAPE or precipitable water, out one, three or five days in six-hour steps. Where the meteogram above
it shows one model's single answer, the plume shows how far the 31 members disagree about it, so a
tight band is a forecast to lean on and a wide one is not. NCEP publishes the mean and spread
ready-made, so each lead costs two small reads rather than thirty-one. The whole plume comes from
one GEFS cycle, and a lead that fails to arrive leaves a gap instead of failing the plume. Temperature
reads in °F like the meteogram, with the spread converted as a difference.

### Added: RTMA surface analysis (G1)

A new **Analysis** group in the model picker offers the RTMA, NCEP's real-time analysis of the
surface: 2 m temperature, 2 m dewpoint, 10 m wind speed and 10 m wind gusts, hourly on a 2.5 km CONUS
grid. It is an estimate of *now* pulled toward the hour's observations, not a forecast, so it has
no lead: the Run menu becomes an **Hour** menu (a day of hourly analyses, newest first), and the
layers use the same color scales as the global temperature, dewpoint and wind so numbers read the
same across sources. Gaps that the resampling leaves at high latitude are closed rather than shown
as a dotted pattern. `hookecho --headless-rtma <field> [out.png]` renders any field from live data.

Not yet covered: surface pressure and visibility (the roadmap also lists them), and URMA, the
retrospective analysis, which would be a separate archive.

### Fixed: river gauges

The gauge service allows only ten requests per five minutes, and panning counted as a reason to
refetch, so a few pans could exhaust it. Panning now waits for a settled view. A view with no valid
bounds yet is no longer sent (the service answered it with its entire 13 MB dataset), coordinates are
clamped and ordered, and a failure now shows what the service actually said (for example "answered
404 Not Found: …") instead of a bare status.

### Added: layer options in the dock

The ImGui dock had nowhere to configure a layer, so the GEFS ensemble (statistic, field, threshold),
the comparison modes and every other per-layer setting were unreachable there. A new **Options**
tab, and an **Options** button in the top bar, host the same settings as the floating panel and phone
sheet, in the dock's own style. The three surfaces now share one implementation.

### Improved: the ImGui dock's model controls

- **A Models button in the top bar** opens the left panel straight to its Models tab (and closes it
  again), instead of Layers first and then hunting for the tab.
- **The model controls match the dock.** Model, Product, Run and Lead are drawn square and monospace
  in the dock's own colors rather than in stock widget styling.
- **A Model Forecast card in the Inspector** while anything from the models is on the map: model,
  product, run, lead, valid time and how long ago it was fetched, with ‹ › to step the lead and a
  button back to the Models tab. It shows "loading…" rather than guessing a time before data arrives.
- **"Forecast" is now "Discussion."** It opens the forecast discussion, which was easy to mistake for
  the model forecasts now under Models.
- The clock in the corner keeps ticking on an idle map, and run times read `18Z` everywhere.

### Improved: longer leads, a choice of model run, and models in every layout

- **Leads past six hours, up to each model's real limit.** HRRR reaches 48 h on its 00/06/12/18Z
  runs (18 h on the others), RAP 51 h on its extended runs, the NAM 3 km 60 h, the NAM 12 km 84 h,
  GFS and GEFS 384 h, and ECMWF 240 h (144 h on its 06/18Z runs). The lead range follows the run
  you are on, so the slider never offers an hour that run does not have.
- **Leads step the way the models publish.** The NAM 12 km goes from hourly to 3-hourly after 36 h,
  the GEFS from 3-hourly to 6-hourly after 240 h, and ECMWF after 144 h. Stepping and the slider land
  only on hours that exist. New **Jump** buttons (+3 h to +5 d) get well out without dragging.
- **Pick the run.** A Run menu lists the recent cycles of the chosen model (a day of hourly runs, two
  days of six-hourly ones), each marked with how far it reaches, or leaves it on Latest. A named run
  is exactly that run: if it does not exist or does not reach the lead, you get an error rather than a
  different run quietly substituted. The choice applies to reflectivity, CAPE, helicity, rotation
  tracks, snowfall, smoke, thunder chance and the global fields, and resets when you change model.
- **Models in the ImGui dock and on phones.** The same Model, Product, Run and Lead controls are now
  at the top of the dock's Models tab, and in an always-visible **Models** section of the Layers panel
  that phones use as their sheet (open by default there). Before, they only appeared under Layer
  settings once a model layer was already on.

### Improved: compare from the model control

Picking HRRR or RAP (reflectivity, CAPE or helicity) or GFS or ECMWF (pressure, 500 hPa height,
temperature, dewpoint, wind) now shows a **Swipe A ⇄ B** button that splits the map between that
model and its natural counterpart at the lead you have scrubbed to. Comparisons also gained the
forecast-hour control they were missing: the HRRR/RAP pair used to be fixed at the analysis hour and
now follows the lead out to 18 h (the range both models share), and the comparison section has its
own slider instead of borrowing the global one. Run-to-run stays at the analysis hour.

### Improved: one Models control

The scattered model controls are now one **Model → Product → Lead** choice.

- **Reflectivity is a product, not a layer.** The separate "HRRR future radar" layer is gone. Any
  model that publishes composite reflectivity offers it: HRRR, RAP, NAM 3 km and NAM 12 km now all
  draw forecast radar, where before only the HRRR did. The forecast banner names the model.
- **HRRR 15-minute is a model of its own,** instead of a "15-min steps" switch on another layer.
- **Only real products are offered.** Each model lists just what it publishes, checked against the
  model catalogue (for example, no reflectivity for the NBM, no dewpoint for the GEFS mean).
- **CAPE and helicity follow the lead.** They used to be fixed at the analysis hour; they now scrub
  through the forecast like everything else, for every regional model.
- **One lead control** in each model's own range and step (hourly for the regional models,
  15-minute for HRRR 15-min, 3-hourly for the global ones), with the run, valid time and fetch age
  shown underneath, and "analysis" called out at F+0.
- **A cleaner layer list.** The 13 separate model rows are one row per product, so "Reflectivity
  forecast" and "Storm fuel (CAPE)" work on whichever model is picked. Layer options replaces its
  Global forecast, Future radar and Environment model pickers with one Model forecast section that
  also holds the CAPE parcel and helicity depth choices. The ribbon's Model, Color fill and Future
  radar groups are one Model group.
- **Remembered.** The last model and product are restored on the next launch.

Saved workspaces keep working: the forecast-reflectivity layer is still stored under its old name.

### Added: GEFS ensemble statistics engine (F7, first step)

`wxdata::ensemble` fetches all 31 GEFS members of a field from one pinned cycle (so members can
never mix runs) and reduces them per grid cell: mean, spread, minimum, maximum, percentiles, and
the percent of members above a threshold. A cell where fewer than half the members have a value is
left missing rather than reported from a handful of survivors, and members that are not on one
lattice and valid time are refused. It covers 2 m temperature, MSLP, 500 hPa height, mixed-layer
CAPE and precipitable water.

`hookecho --headless-ensemble <field> <stat> <hour> [out.png]` renders any statistic from live
data, for example `cape prob:1000 24` or `t2m spread 72`.

### Added: “GEFS ensemble” map layer (F7)

The Models group has a new “GEFS ensemble” layer. Layer options pick the field (2 m temperature,
MSLP, 500 hPa height, mixed-layer CAPE, precipitable water) and what to show: mean, spread, min,
max, 10th or 90th percentile, or the share of members above a threshold you can edit in your own
units (°F/°C for temperature). Statistics in the field’s own units use that field’s usual colors;
spread and probability use a separate translucent scale, with a legend for each. Hovering reads
the value in your units, for example “±9.0 °F” or “62%”. The 31 members are fetched once and held,
so switching statistic or threshold recomputes instantly rather than downloading again.

### Added: “Forecast comparison” starter workspace (J5)

A new two-pane starter puts the observed MRMS mosaic beside HRRR “future radar”, with cameras and
the probe cursor linked. It is seeded on first run like the other starters, so existing installs
can save the same arrangement themselves. RRFS and ensemble probability, the other halves of the
roadmap preset, remain open.

### Added: composite reflectivity in model comparison (F6)

Model comparison gains a “Composite reflectivity” field: HRRR against RAP “future radar” at one
shared valid time, available in every comparison mode (swipe, blink, overlay, difference, and
side-by-side panes). Both models are colored with your own reflectivity palette, so they read like
the radar; the difference layer uses a 30 dBZ full scale with a 5 dBZ agreement deadband, and hover
readouts report each model’s dBZ. Its GRIB key is checked against the model catalogue for both
models, like CAPE and SRH.

### Fixed: model-comparison swipe divider

The swipe divider and the map now share one interaction target, and drag ownership is decided from
where the press began, so grabbing the handle no longer loses the gesture to map panning.

### Improved: complete imported-GIS styling (I4)

The Layer Manager now controls imported outline width as well as color and opacity. The width is
applied consistently to polygon boundaries, line features, and point symbols while remaining in
screen pixels across map zoom levels. Existing settings retain the previous 1.6 px appearance,
invalid persisted values are bounded at render time, and official warning/outlook geometry keeps
its established width.

### Improved: responsive ImGui dock and movable search

The ImGui dock now keeps only one sidebar open on narrower windows, adds an explicit Inspector
tab, and uses slimmer edge panels so opening Layers no longer squeezes the map into a strip in the
middle. The live 2D/3D selector now lives in each layout's permanent chrome—the Dock top bar,
ribbon Tools group, Minimal control column, or phone mode bar—while the Dock keeps a separate
Volume explorer button. The advanced 3D controls appear only after the tilted map is active.

Settings uses the Dear ImGui theme's square, flat drawer and compact tab treatment instead of the
rounded glass navigation used by the map-first themes. The optional floating ribbon search button
also has a visible drag grip and can be repositioned within the map area for the session.

The max/min trail's Layer options now has **Reset at playhead**, rebuilding the cached trail window
at the selected live or archive time and invalidating the previous uploaded trail image.

### Added: imported GIS styling (I4)

The Layer Manager now gives an imported GeoJSON or Shapefile its own color and opacity controls.
One persistent style applies to polygons, lines and points, so a reference dataset reads as one
layer and can be separated visually from official warnings and outlooks. Existing settings keep
the previous neutral-blue appearance exactly, and Reset restores it.

This also fixes the remembered GIS layer reloading invisibly after restart: a file that reloads
successfully now turns its layer back on. The Layers/Tools descriptions have been corrected to
name Shapefiles and the point/line support that already exists.

### Added: tablets get the desktop layout, and two fingers tilt a 3D map

**Tablets.** On Android, a screen whose shortest side is 600 dp or more now draws the desktop
layout — the same floating chrome, ribbon, docked panels and windows — instead of the phone's
sheets and chips. It goes by the shortest side, so a phone held sideways stays a phone, and a
tablet in a narrow split-screen window falls back to the phone layout while it is that narrow. The
Back gesture still closes the window on top first. Not yet tried on a real tablet; desktop controls
are sized for a mouse and some are hover-only, so expect to find a few that want a bigger target.

**3D pitch.** With the 3D map on, sliding two fingers up raises the pitch (the map leans back toward
the horizon) and sliding down lowers it, at the same rate as the mouse's right-drag tilt. One finger
still pans, pinch still zooms and twist still rotates; on a flat map the two-finger slide still pans.
The vertical part of the slide now tilts instead of panning while 3D is on, and the horizontal part
still pans.

### Fixed: the Windows app closed itself about two seconds after opening

Starting the live-radar failover monitor called `tokio::spawn` on the UI thread, which has no
runtime, so the app panicked with "there is no reactor running" and exited (the crash report it
leaves behind named the line). The monitor now starts on the app's own runtime. CI's Windows job
did not catch it because its smoke test never opens the window; a regression test now starts the
monitor from a thread with no runtime and fails with that exact message against the old code.

### Fixed: the Android CI job failed before building anything

`android-actions/setup-android` asked `sdkmanager` for the `tools` package, which Google no longer
serves, so `sdkmanager` exited 1 at "Set up Android SDK". Both workflows now name `platform-tools`.

### Fixed: the Android library did not compile

`devlog_admin`, which is built for Android, called `crate::serve::constant_time_eq`, but `serve` is
desktop-only, so the Android build failed with "cannot find `serve` in the crate root". Nothing
caught it because the Android job in CI died earlier and this branch's ordinary CI never compiles
the Android target. The shared comparison now lives in its own small module that both use.

### Added: Shapefile import (I1)

"Import GIS file…" now takes an ESRI Shapefile (`.shp`) as well as GeoJSON. On desktop the
attributes (`.dbf`) and coordinate system (`.prj`) are read from beside the file you pick; a
browser or phone picker hands over one file, so there the shapes import without attributes and
the message says so. Holes, multi-part shapes, dates, numbers and deleted rows are handled.

A file in a coordinate system HookEcho cannot place is refused with the name of what it is in —
State Plane, UTM and the older NAD 27 datum — rather than drawn in the wrong place; WGS 84, NAD 83
and Web Mercator work. Tested on synthetic files, including cutting a valid pair at every byte to
check a damaged file is refused and never crashes; not yet tried on shapefiles from real GIS
software.

### Added: a max/min trail layer (C2) and a scan-age ring (WSV3 gap)

**Max/min trail.** A single frame says where a storm's strongest core is; it cannot say where it
has been. The new layer replaces the active pane's radar with the strongest (or weakest) value each
gate held over the last 15, 30, 60 or 120 minutes, ending at the playhead — a reflectivity-core or
CC-minimum path. It is built only from volumes already in the decode cache, three per frame, so a
two-hour window fills in over a few frames instead of freezing one. It refuses to blend sweeps that
are not the same beam (different product, tilt, site, or raw against dealiased velocity) and says so
in Layer options. Decay, export and a custom window length are not built, rotation and hail paths
from the MRMS grids are a separate route, and it has been tested but not yet looked at on screen.

**Scan-age ring.** The antenna takes minutes to turn, so the two edges of a radar picture can be a
whole rotation apart, and nothing showed which. A ring at the edge of the sweep is now coloured green
where the data is newest through red where it was collected longest before, labelled with how much
time the sweep spans (and, on a live volume, how long ago the newest of it arrived). Ages are
relative to the sweep's own newest data rather than the wall clock, which on an archive replay would
call the whole picture years old. Also untested on screen.

Both add under 5 KB gzipped to the browser bundle together.

### Added: an imported GIS file is remembered across restarts (I1)

A GeoJSON import used to last only for the session, so a district boundary, coverage area or asset
file someone works with every day cost a re-pick on every launch. It now comes back automatically,
remembered the same two ways an imported `.pal` already is: a path where there is a filesystem, and
the content itself in a browser, which has no path that would survive a reload.

A file that has since moved or been deleted is reported rather than swallowed — the layer simply
not being there would otherwise be indistinguishable from the app having forgotten it, and only the
person can fix a missing file. The reference is kept through that failure rather than dropped,
since a drive that isn't mounted this launch will likely be mounted the next.

### Fixed: the clippy gate passes again, and satellite samples are pinned as temperatures

`cargo clippy --workspace --all-targets -- -D warnings` is a CI gate and was failing with 21
findings, so it had stopped enforcing anything. All are fixed mechanically — named type aliases
for four unreadable inline types, `div_ceil`/`is_multiple_of`/`contains`/`then_some`, `?` in place
of a let-else, struct-literal init instead of Default-then-assign, and a test module moved to the
end of its file. Two `#[allow]`s were kept deliberately rather than rewritten (an 8-argument
render function, matching an existing allow elsewhere for the same reason, and two single-element
layer lists that exist to be grown). One real find along the way: `view_projection` computed a
`pitch` local it never used, since `eye_position` derives its own — deleted rather than silenced.

ROADMAP_NEW E6's "brightness-temperature sample" and D3's "point sample" are both closed by the
new gridded-layer probe, and the GOES side now has tests: a satellite pixel reads as a brightness
temperature in the user's own unit, while `GoesDustDiff`/`GoesColdTop` — which deliberately store
a band difference and an offset from a 210 K threshold rather than an absolute temperature — keep
their own units instead of being labelled `°C`. A wrong number with a convincing unit beside it is
the failure mode worth pinning.

### Added: AWIPS-style focus pane layouts

Multi-pane workspaces can now switch between the existing equal grid and a `Focus` arrangement.
Focus makes pane 1 the large analysis canvas and tiles every supporting pane into a compact detail
rail on the right in landscape or along the bottom in portrait. The rail adapts from one to two
columns/rows as pane counts grow, so all supported 3–9 pane workspaces stay inside the viewport
without overlap while the primary pane remains larger than every detail pane.

The WSV3 ribbon and command palette expose the same `Even`/`Focus` actions. The choice is saved in
workspace files and restored with the panes; older workspace JSON defaults to `Even`. The shipped
Hail analysis preset now uses Focus so reflectivity remains the primary view while ZDR, CC and KDP
form its supporting rail. Phones retain their existing one-pane-at-a-time presentation, while
tablet, desktop and web layouts use the asymmetric geometry.

### Added: data-driven 2D live radar sweep

Partial Level II updates now draw a WSV3-style lime sweep line across the currently viewed 2D
tilt. Each real update triggers one bounded animation through only its refreshed azimuth sector,
timed from that VCP cut's declared antenna rotation rate. Direct Unidata chunks carry their
60°/120° sectors; the self-hosted relay now emits the same progress event from each block's radial
bounds and decoded VCP. A dark keyline and short tail keep the beam visible over every radar
palette; the radar data itself still appears immediately rather than being withheld behind the
animation.

The indicator does not run for a different tilt, 3D, archive playback, completed-volume polling,
or a stalled/ended stream, so it cannot imply data are arriving when they are not. Reduced-motion
mode shows the arrived sector edge without continuous animation. This also fixes live chunk
progress being cleared by the partial merge immediately following it, allowing the existing live
progress UI to retain the current sweep state until the next chunk or stream end. Focused tests
cover sector timing, north wrap, invalid metadata, tilt gating, reduced motion, relay WebSocket
progress, and painted beam geometry.

### Added: draggable model-comparison swipe (F6/J4)

Model comparison now offers “Swipe A/B”: model A fills the left of one pane, model B fills the
right, and a labeled divider can be dragged from 5–95% without panning or drawing on the map.
Hover readouts and the synchronized multi-pane probe sample whichever model is physically under
the cursor, while the legend identifies both halves. Swipe is mutually exclusive with blink,
overlay, difference, and two-pane modes, and unsupported run-to-run comparisons do not expose it.

The split is rendered inside one map callback with complementary scissor rectangles and restores
egui’s original clip after each half. That avoids the prepare-order race that made two callbacks
for one pane unsafe and prevents later field layers from inheriting the split. Focused tests cover
pixel-exact partitioning, clipping, rounding, and the cursor-side boundary.

### Added: categorical model-disagreement mask (F6)

Model difference now has a third display mode: a directionless categorical mask that draws one
magenta class wherever the models differ by more than the selected field's established deadband,
while agreement and missing data remain transparent. It recolors the retained signed grid without
refetching either model. The legend states the threshold, and hover plus linked-probe readouts say
Agree/Disagree while preserving the actual difference magnitude. Tests cover direction symmetry,
deadband boundaries, missing data, the single-class LUT, and analyst-facing readout text.

### Added: pane-local transparent model comparison (F6/J4)

Model comparison now offers "Overlay A/B" beside difference, side-by-side, and blink: A draws at
normal opacity and B at 50% of the user's configured layer opacity in the same pane, with a legend
that names both models. Comparison modes remain mutually exclusive, unsupported run-to-run fields
do not expose the command, and stopping leaves a useful static A view. The renderer now keeps one
small field uniform/bind group per pane while continuing to share grid textures and LUTs, fixing
the previous prepare-order bug where one pane's opacity could overwrite another's. A focused test
pins the blend math and preservation of user opacity.

### Added: nine-pane desktop/web analyst grid (J1)

ROADMAP_NEW J1's largest even layout is now a 3x3 grid on desktop and web, reachable from both the
WSV3 ribbon and command palette. One shared platform ceiling sizes the app and GPU 3D caches so
those limits cannot drift apart; Android deliberately remains at six panes. Seven- and eight-pane
workspace imports also retain every view in the nine-cell layout instead of falling through to a
four-cell fallback. Focused layout tests cover pane count, grid shape, and exact source-rect extent.

### Added: gridded layers in the synchronized pane probe (J3)

ROADMAP_NEW J3's linked crosshair table now samples the top visible MRMS, model, satellite,
derived or comparison grid in each pane instead of reporting only radar gates. `FieldState`
retains the decoded display grid beside its GPU texture and releases both through the existing
five-minute inactive-field eviction, providing a truthful sample without trying to invert an
8-bit color index or creating an unbounded second cache. The probe uses the renderer's own draw
order, reports source/product/valid time, formats categorical values with their legend labels,
converts Kelvin-backed fields to the selected temperature unit, applies every ramp's display
scale, and understands signed/absolute differences plus both comparison sides. Panes without a
gridded layer continue through the exact radar gate-inspector sampler. Focused tests cover table
fallbacks, units, categorical labels, missing data and legacy product names.

### Added: product cycling and a reachable 3D toggle (J6)

`N` and `P` step to the next/previous radar product, wrapping — distinct from the `1`-`7` keys,
which jump straight to one specific moment. Cycling is what's wanted with a hand on the mouse,
stepping REF → VEL → CC across one storm. They cycle `Moment::ALL` in its own declared order, the
same order the number keys select in, and leave the pane's SRV choice alone: that's a way of
reading velocity, not a product of its own. Both are plain letter keys, so they yield to a focused
text field like every other single-character shortcut.

The map-pitch 3D view now has a `PaletteAction` of its own, reaching the command palette, the
Layers drawer and a `D` shortcut — it was previously available only from a dropdown buried in the
3D options panel. The toggle and that dropdown now share one `MapView::set_map_3d`, so they can't
disagree about the camera pose each mode rests at (pitched for 3D, flat and north-up for 2D);
re-selecting the mode a pane is already in leaves a hand-set camera angle alone.

### Added: imported GIS points and lines now draw, and imports frame themselves (I1)

A GeoJSON import used to render only its polygons and report the rest as "not drawn yet", which
left a file of city sites or a river/road network importing as nothing visible. Points and lines
now paint directly through the same lon/lat projection the freehand annotation strokes already
use — points as outlined dots that stay readable over both a bright radar core and a dark
basemap, lines as polylines — while the polygon half keeps riding the overlay pipeline, where it
gets hit-testing and click-through attributes for free. Still deferred to I4: *styling* them, i.e.
per-layer color and width, and labels from a chosen attribute.

Importing now also frames the map on what it just loaded, and "Zoom to imported shapes" in Tools
does it again after panning away. A file covering somewhere the map isn't looking previously
imported to no visible effect at all. The fit is computed in world units rather than degrees,
since a latitude degree is not a constant height under Mercator and fitting on degrees overshoots
badly away from the equator.

### Added: export what's on the map as GeoJSON (I6)

ROADMAP_NEW I6's first half: "Export map as GeoJSON…" in Tools writes freehand annotations
(as lines, carrying their own color), saved markers, watch zones, storm cells (with movement,
max dBZ, echo top and VIL where the scan has them) and every displayed overlay polygon into one
file for QGIS, ArcGIS or a briefing. Phase I exists for emergency-management, research and
broadcast users, and until now nothing geographic could leave the app at all.

It exports what is drawn, not everything fetched — the source is the same assembled overlay set
the map renders, so filters and toggles are already applied. Each feature carries a `hookecho`
property naming what it came from, so a re-import can tell an annotation from a warning polygon.
Locally drawn rings are closed on the way out, since GeoJSON requires it and a watch zone the user
clicked out is not; missing cell values are omitted rather than written as null. `wxdata::gis`
gained the writing half of its own parser, so an export reads straight back through the importer —
which is what the round-trip tests check, rather than asserting against a hand-written string.

### Added: beam-rise control for the 3D Observed view

A radar beam genuinely climbs with range, so in the map-pitch 3D Observed view every tilt flared
steeply upward at long range and a multi-tilt volume read as a stack of cones rather than as storm
structure. A new "Beam rise" slider scales how much of that climb is drawn: 100% is the true
geometry (unchanged behaviour), lower values pull the far end of each sweep proportionally back
toward the antenna's altitude — taking the most off where the rise is largest — and 0% lays the
sweeps flat, matching the 2D view. It scales the whole rise rather than one half of it, so
vertical exaggeration keeps its own separate meaning and the two compose.

The value rides in the uniform slot the volume's lowest-tilt elevation vacated when the floor
built from it was replaced, so the buffer layout is unchanged, and it joins the rebuild identity
because the uniform only reaches the GPU alongside a fresh instance buffer. Click-picking inverts
the same geometry the shader draws, so the CPU mirror takes the control too — a pick that still
assumed true geometry would miss by kilometres at a reduced rise, which a new test covers
alongside the two ends of the control and the "more at long range than short" property.

### Added: absolute-difference display mode for model comparison (F5/F6)

The comparison layer can now be read as `|A − B|` magnitude instead of a signed `A − B`, answering
"where do these disagree at all" without direction getting in the way. The fetched CPU grid stays
signed and authoritative, so switching modes recolors the resident field rather than refetching
either model: a separate display key tracks what the GPU upload represents, and only the
value-to-index mapping and LUT change. Absolute mode draws a sequential amber-to-red scale sharing
the signed view's transparent agreement deadband, and the legend's bar, tick labels and title all
follow the selected mode. The cursor readout is put through the same transform the upload was, so
a magnitude-colored map can't hand back a negative number, and drops the forced `+` sign that
would imply a direction the mode deliberately discards.

### Added: truthful cache residency and Cached source-health state (N1)

ROADMAP_NEW N1 source health now reports whether each active source's last delivered value is
actually resident in memory. A successful delivery establishes residency, the renderer's existing
five-minute field-texture eviction clears it, comparison failures clear the grids they explicitly
invalidate, and radar reads residency from the pane's decoded volume. Failed refreshes preserve a
resident last-good value and now become the distinct `Cached` state; failures with no fallback
remain `Failed`, and evicting the fallback immediately changes `Cached` to `Failed`. Both the
per-layer popup and consolidated health window show cache state, and diagnostics JSON exports the
stable `memory`/`empty` value. The implementation deliberately makes no claim about opaque browser,
OS or intermediary HTTP caches. Plugin command errors are also now recorded as health failures
instead of being mistaken for successful cache writes merely because they arrive as UI messages.
Focused tests cover cold failure, success, failed refresh, eviction and the message-wrapped plugin
error path.

### Added: structured radar fallback providers in source health (N1)

ROADMAP_NEW N1 source health now carries `fallback_providers` as structured metadata rather than
leaving radar's real B6 failover chain buried in ad-hoc detail strings. Native radar derives the
list from the same `FailoverSnapshot` used to select its active provider: primary names the
optional HookEcho Relay and TGFTP tiers, relay-active names Unidata and TGFTP, and degraded TGFTP
names the configured progressive recovery candidates. The per-layer popup, consolidated health
window and diagnostics JSON expose those alternatives; sources without a runtime fallback keep an
explicitly empty list, including web radar whose provider manager remains native-only. A
deterministic test covers all selected-tier/relay-configured combinations.

### Added: authoritative valid time in source health (N1)

ROADMAP_NEW N1 source health now separates "when did this request finish here?" from "when is the
data actually valid?" `RequestStatus` retains the newest authoritative valid/observation time
carried by successful gridded products, comparisons and timestamped observation feeds; older
archive or forecast selections cannot regress it, and a failed refresh leaves the last trustworthy
time intact. Radar contributes the newest frame in its timeline through the same field. The
per-layer health popup and consolidated Data source health window show absolute UTC plus relative
age/forecast lead, while diagnostics JSON exports RFC 3339. Untimed payloads explicitly read
`not reported` instead of presenting fetch completion or alert expiration as meteorological
provenance. Five focused tests cover extraction, newest-time retention, failure behavior and UI
formatting.

### Added: endpoint families in source health (N1)

ROADMAP_NEW N1 source-health rows now identify the shared upstream endpoint family separately
from the layer-specific source name. A new typed `EndpointFamily` classifies descriptor-backed
MRMS and model fields automatically from the field registry and explicitly groups current feed
lanes such as NWS API, NOAA map services, IEM, AviationWeather.gov, radar products, community and
multi-provider sources. The family appears in both the Layers-row health popup and the
consolidated Data source health window; diagnostics JSON carries its stable machine ID, making it
possible to recognize and group one upstream outage affecting several differently named layers.
Four focused metadata tests cover registry inheritance, shared feed grouping, source-specific
cadence and stable IDs.

### Added: wire the failover arbiter, relay and TGFTP into live radar (B6.11 step 11)

ROADMAP_NEW B6.11 step 11: everything steps 1-10 built (the failover arbiter, the dual-feed health
monitor, the HookEcho relay client, the NOAA TGFTP degraded provider) was fully tested but
completely dormant — `spawn_stream` always hard-coded `UnidataLevel2Provider` regardless. This pass
wires it into the actual live pipeline. New `hookecho::radar_provider_manager::SiteProviders` owns
one `failover_arbiter::SiteArbiter` + `provider_health::HealthBoard` pair per `MapView` pane, adds a
third "degraded" tier on top of the arbiter's own binary primary/backup choice — falling to
`NoaaTgftpLevel2Provider` once whichever side the arbiter currently prefers has itself gone stale
past a fixed grace period, which engages even with no relay configured at all (a lone stalled
Unidata feed degrades to TGFTP rather than leaving the pane stuck) — and exposes a three-way manual
override wider than `SiteArbiter`'s own binary one. `HookEchoApp::sync_radar_providers` creates,
replaces and ticks one `SiteProviders` per pane every frame from two new settings,
`radar_relay_url` and `radar_provider_override` (a new "Radar relay (advanced)" section in Settings
→ General); `spawn_stream` now asks the manager which `Level2LiveProvider` to subscribe with, and
`manage_stream` aborts a running stream immediately when the selected tier changes instead of
waiting for it to fail on its own — a real, observable failover, not just "the next reconnect picks
a different provider." The existing B3 radar health popup gained "Active provider," "Standby
provider," "Failover state" (`PRIMARY`/`BACKUP`/`DEGRADED_VOLUME`/`MANUAL`) and "Last transition"
lines (`registry.rs::failover_details`) rather than a second panel, and `DiagnosticsSourceHealth`
now carries `SourceHealth.details` verbatim so those same lines reach the N4 diagnostics bundle for
free. `provider_health::ProviderHealth` gained a `consecutive_failures` counter (reset on success,
incremented per failed `subscribe` attempt) since the arbiter needs exactly that, not the lifetime
failure total it already tracked. Explicitly **not** done here: wiring `wxdata::continuation`'s
`check_volume_continuation`/`RadialDedup` — a tier switch today resets to the pane's last full
volume and re-streams from there via the new provider (safe: it can never mix two sources' radials,
matching the roadmap's own "availability may degrade; scientific identity may not" invariant)
rather than the more ambitious *seamless* mid-sweep handoff B6.7 describes; that stretch goal, plus
sequence-gap detection and a true side-by-side developer health view, remain open for a later pass.
Native only, like every module it wires together (`radar_provider_manager` doesn't build on
wasm32); wasm32 keeps exactly its prior Unidata-only behavior, unchanged and unaffected. Eight new
deterministic tests (`radar_provider_manager`'s own tier-selection/degradation/override suite, plus
a new `provider_health` test proving a success actually resets `consecutive_failures`), no live
network required.

### Added: NOAA TGFTP degraded completed-volume provider (B6.11 step 10)

ROADMAP_NEW B6.11 step 10: `hookecho::tgftp_provider::NoaaTgftpLevel2Provider` — the last-resort
`Level2LiveProvider` for when neither progressive live path (Unidata or the HookEcho relay) is
usable. Polls NOAA's TGFTP mirror's small `dir.list` index rather than a full directory listing,
downloading a volume file only when a genuinely new one appears, and decodes it through the exact
same `wxdata::level2::decode_volume` the AWS archive path already uses — no second decoder. Built
and verified against the real, live TGFTP service (not only deterministic fixtures): fetched and
decoded an actual current volume, and along the way confirmed from real header bytes that the
`.bz2` filename suffix does not mean a whole-file compression wrapper, just the standard per-record
bzip2 Archive II format `decode_volume` already handles. `capabilities()` correctly advertises
`progressive_radials: false`, and `subscribe` never calls `on_progress` — there is no progressive
signal to fake. Every successful fetch updates an in-memory cache so a total network outage
degrades `latest_complete_volume` to serving the last known-good volume instead of erroring out to
a blank pane; the very first call with nothing cached yet still surfaces a real error. Cross-
platform (native and wasm32) — needs only `reqwest` and `wxdata::task::sleep_while`. Not yet
selected by the failover arbiter as an actual last resort; that wiring is B6.11 step 11.

### Added: safe mid-volume continuation and cross-provider deduplication (B6.11 step 9)

ROADMAP_NEW B6.11 step 9: `wxdata::continuation` answers the two questions a safe cross-provider
switch mid-volume needs answered — "can these two sources' data be mixed right now?" and "have I
already rendered this exact radial?" — enforcing the roadmap's own invariant: "availability may
degrade; scientific identity may not." `check_volume_continuation` permits continuation only on
*exact* `VolumeKey` equality; anything else, including two volumes that merely started close in
time, is `Incompatible` — there is no fuzzy "probably fine" outcome. `RadialDedup` tracks which
radials a live volume has already accepted and filters a newly-joined source's overlapping
coverage, correctly treating a SAILS/MRLE revisit as new radials rather than duplicates of the base
tilt (covered by a test named directly after ROADMAP_NEW B6.12's own acceptance-test wording).
Pure identity/bookkeeping logic — no network, no rendering — and not yet wired into the live render
pipeline; that's B6.11 step 11.

### Added: per-site failover decision arbiter (B6.11 step 8)

ROADMAP_NEW B6.11 step 8: `hookecho::failover_arbiter::SiteArbiter` decides which of a primary and
backup live radar provider should be active for one site, from freshness and failure signals alone
— never from HTTP reachability. It fails over on repeated consecutive transport failures or on the
active side's data going stale, but only when the other side is demonstrably fresher by a
configurable margin, so a switch can never move the visible newest-radar timestamp backwards.
Failing back to the preferred (primary) side requires several *consecutive* healthy observations,
not just one, so a flapping primary can't bounce the active side back and forth; any unhealthy
observation resets that streak to zero. Every transition carries an explicit
`wxdata::live_block::ProviderSwitchReason`. A manual override (for Advanced-settings diagnostics)
holds the active side fixed until explicitly cleared. Pure decision logic — no network, no
platform dependency, builds on wasm32 too — and not yet wired into `MapView` or the renderer; that
starts at B6.11 step 11. Nine tests, including a property check that a switch never regresses the
visible newest-radar timestamp across a scripted sequence.

### Added: run both live radar providers concurrently and compare their health (B6.11 step 7)

ROADMAP_NEW B6.11 step 7: `hookecho::provider_health` can now run any number of
`Level2LiveProvider`s for a site side by side — the Unidata/AWS path and the new HookEcho relay,
in particular — purely to observe and compare their health, without touching which one is actually
rendered. `spawn_dual_feed_monitor` starts a reconnecting `monitor_provider` task per provider,
recording newest radar time, last receipt time, success/failure/reconnect counts, and the last
error into a shared, thread-safe `HealthBoard` keyed by provider label. It's structurally
impossible for a monitored provider's data to end up on screen through this path — the monitor
never calls a rendering `on_update`, only its own bookkeeping closure — so "without switching" is
guaranteed by the code shape, not just documented as a rule to follow. Not yet wired into the UI
(the existing per-pane radar health display is unchanged); tested against a deterministic scripted
fake provider rather than live network. Native only, matching `relay_provider`.

### Added: `HookEchoRelayLevel2Provider`, the second live radar path (B6.11 step 6)

ROADMAP_NEW B6.11 step 6: HookEcho can now receive live Level II from a self-hosted `radar-ingest`
relay, not only the existing Unidata/AWS chunk stream — the second independently acquired
progressive path B6 is ultimately about. `hookecho::relay_provider::HookEchoRelayLevel2Provider`
implements `Level2LiveProvider`: `subscribe` opens a real WebSocket to the relay's
`/sites/{site}/live` and reassembles a `Scan` from accumulated blocks via a new
`wxdata::live_block::assemble_scan`, which reuses `nexrad-data`'s own real-time chunk-assembly code
(each block's raw message bytes wrapped as an `IntermediateOrEnd` LDM record) rather than a second
decoder, then merges it in with the same `wxdata::live::merge_scan` the Unidata path already uses.
`latest_complete_volume` polls a new relay endpoint, `GET /sites/{site}/volume/latest`, added to
`radar-ingest` alongside this (every retained block for the site's latest completed volume, from
the same retention the live path already has). The block DTO moved from `radar-ingest` into
`wxdata::relay_wire` so both server and client share one JSON shape without `hookecho` taking on
`radar-ingest`'s server-only dependencies (axum, tokio's `net` feature); every block is checksum-
verified on receipt before being trusted. Proven end-to-end against a real in-process relay server
over a real socket, not just unit-tested. Native only for now — a browser WebSocket client is
future work, not built blind; this provider is opt-in and `UnidataLevel2Provider` is unchanged.

### Added: external LDM upstream configuration in `radar-ingest` (B6.11 step 5, partial)

ROADMAP_NEW B6.11 step 5: `radar-ingest::ldm::LdmSourceConfig` reads a live LDM/IDD upstream peer's
configuration (host, port, feed pattern, site allowlist) from `RADAR_INGEST_LDM_*` environment
variables — no default host is assumed, matching B6.2's "treat LDM access as deployment
configuration, not an entitlement." This step stops at configuration: the LDM6/7 wire protocol
itself cannot be implemented and validated without a real upstream peer to test against, and none
is available in this environment. That's a genuine external blocker the roadmap's own B6 planning
anticipated, not a gap papered over — see the module's doc comment and ROADMAP_NEW's B6.11 step 5
note for the full reasoning. Everything downstream of ingestion is already provider-agnostic, so a
live adapter slots in later behind the same `input::InputAdapter` trait `ReplayInputAdapter`
already implements, with no other changes needed.

### Added: WebSocket live stream and HTTP resume API for `radar-ingest` (B6.11 step 4)

ROADMAP_NEW B6.11 step 4: `radar-ingest` now serves the blocks its rechunker produces over the
network. A new `pipeline::Pipeline` ties the rechunker to bounded per-site block retention
(`block_store::BlockStore`, keyed by sequence, bounded on both item count and total bytes) and
per-site live fan-out (`tokio::sync::broadcast`). `server::router` (axum) exposes `/health`,
`/ready`, a per-site `/sites/{site}/head` manifest (newest sequence, current volume/cut, latest
completed volume), `/sites/{site}/blocks/{sequence}` to fetch one retained block, and
`/sites/{site}/live` — a WebSocket stream supporting `?resume_after=<sequence>` reconnect: the
handler computes the backlog and subscribes to the live channel under one lock acquisition, so a
reconnecting client is replayed exactly what it missed with no gap and no duplicate. Blocks travel
as JSON (`wire::BlockDto`, checksum hex-encoded, payload base64-encoded) — plain and inspectable
for now; the roadmap explicitly calls for compression only once a measured win exists, not by
default. Auth/rate-limiting, a completed-volume HTTP endpoint, and the live LDM connection itself
are still open — see ROADMAP_NEW's B6.4 notes for the precise list. This crate remains standalone;
the GUI app does not yet talk to it (that's B6.11 step 6, `HookEchoRelayLevel2Provider`).

### Added: lossless Level II rechunker in `radar-ingest` (B6.11 step 3)

ROADMAP_NEW B6.11 step 3: `radar-ingest::rechunk::Rechunker` turns raw ingested products into
canonical `wxdata::live_block::LiveLevel2Block`s. It parses NEXRAD Level II Message Type 31
(Digital Radar Data) headers via `nexrad_decode::messages::decode_messages` — the same
message-level decoder `nexrad-data` already uses internally for whole-volume decode — just far
enough to know site/volume/elevation-cut/azimuth/radar-time identity, correctly separating
SAILS/MRLE mid-volume revisits via `wxdata::live_block::CutTracker`. Every message's original
bytes are re-emitted verbatim as a block's payload; nothing is resampled, recompressed, or
reformatted. A block flushes on whichever comes first: an elevation/volume boundary, a configured
radial-count limit, or (via a periodic `tick`) an age limit, so a quiet stream never holds a
partially filled block indefinitely. Any non-radial message (VCP, RDA status, etc.) is forwarded
verbatim as its own pass-through block rather than being silently dropped. A per-site `SiteManifest`
(newest sequence, current volume/cut, latest completed volume) is queryable independent of any one
block — the data B6.4's future per-site head endpoint will serve. Network distribution and the live
LDM connection are still later increments; this crate is not built or deployed by the GUI app.

### Added: `radar-ingest` backend crate — replay input and per-site ring buffer (B6.11 steps 1-2)

ROADMAP_NEW B6.11 steps 1-2: the first two increments of the self-hostable `radar-ingest` LDM/IDD
backend service. `wxdata::live_block` supplies the canonical provider-neutral identity/provenance
model (volume/cut/radial identity, SAILS/MRLE-aware cut tracking, provider capabilities and switch
reasons — already wired into `hookecho::volume::Level2LiveProvider`). The new `crates/radar-ingest`
crate adds the adapter boundary between "wherever raw LDM products come from" and the ingest core
(`input::InputAdapter`, `dyn`-safe the same way as `Level2LiveProvider`) with a fixture-driven
`ReplayInputAdapter` so ingest logic is tested deterministically without a live LDM process, plus a
bounded per-site in-memory holding area (`store::IngestStore`/`SiteRingBuffer`) that validates site
ID and product size, supports a site allowlist, and enforces backpressure and memory bounds on both
item count and total bytes per site. No live LDM connection, rechunking, or network distribution
yet — those are later B6 increments; this crate is not built or deployed by the GUI app.

### Added: runtime-selectable live-radar provider boundary (B6.1)

ROADMAP_NEW B6.1: `Level2LiveProvider` (the trait behind live radar updates) is now `dyn`-safe, so
a per-site arbiter can hold `Box<dyn Level2LiveProvider>` and choose between providers at runtime
instead of the app being wired to exactly one live source. Made possible via `async-trait`, kept
correct on both targets: native retains `Send`-bound futures (every call site already awaits
inside a `Send`-spawned task), while wasm32 opts out with `async_trait(?Send)` because `reqwest`'s
wasm transport holds non-`Send` `wasm_bindgen::Closure` values internally — the trait's
`Send + Sync` supertrait bound was dropped rather than conditionally compiled, so a native caller
that needs it adds `+ Send + Sync` at its own `dyn` use site. Providers also now advertise
`wxdata::live_block::ProviderCapabilities` (`progressive_radials`/`resume`/`completed_volume`/
`historical_backfill`/`server_push`), separating what a transport can do from which provider it
is. No behavior change yet — `UnidataLevel2Provider` remains the only implementation in use.

### Added: "Dear ImGui" theme

Requested live, with reference screenshots of Dear ImGui's own demo/example apps: a new theme
reproducing `ImGui::StyleColorsDark()` — colors *and* shape, not a palette swap wearing this
app's own widget geometry. Colors: near-black `#0F0F0F` window fill (ImGui's own `WindowBg`),
pure-white text, and the exact "ImGui blue" accent (`#4296FA`, `(0.26, 0.59, 0.98)` — ImGui's
`CheckMark`/`Header`/`SliderGrabActive`). The idle/hovered input-field colors aren't guessed:
they're `ImGuiCol_FrameBg` and `FrameBgHovered` — ImGui's own translucent navy-blue accent
washes — alpha-composited over `WindowBg` by hand, the same blend ImGui's renderer does at those
exact alpha values.

Shape: every widget/window/menu corner goes square (ImGui's `FrameRounding`/`WindowRounding` are
both `0.0`), and spacing/button padding/interact height switch to ImGui's own tight, fixed
`ItemSpacing`/`FramePadding` instead of this app's touch-aware density scale — this covers every
plain `egui::Button`/`Checkbox` throughout Settings, popups, and drawer pages for free. The WSV3
ribbon's hand-painted "pill" buttons and checkboxes, which draw their own shape and never consult
the generic style, needed their own fix: under this theme they're flat, square-cornered, and
sized close to their label (ImGui's `FramePadding`) instead of the glossy, generously-sized
stadium pill every other theme still gets, with no gloss wash and no border on an idle/hovered
frame (`FrameBorderSize` is `0.0` in ImGui's own default style) — a `theme::is_imgui_style()`
flag is the one thing these free functions, with no `&Settings` in scope, needed to know which
shape to draw. The ribbon's own navy→black gradient background is a flat `MenuBarBg`-style fill
under this theme too, since Dear ImGui's style never uses a gradient anywhere. Selectable
anywhere the existing seven themes are (Settings already lists every `Theme::ALL` entry
generically) — no new picker UI needed.

### Added: rolling success/failure count in source health

ROADMAP_NEW N1: the per-source health popup and the consolidated "Data source health" window
only ever showed the single most recent attempt/success/failure, not how a source has actually
been behaving lately. `RequestStatus` now keeps a rolling window of the last 20 finished
requests' outcomes, and `SourceHealth.recent_outcomes` reports it as "18/20 succeeded" — shown in
the per-row popup, a new "Recent" column in the consolidated window, and carried into the N4
diagnostics bundle. Radar's own health, built from `MapView` fields directly rather than through
the shared `RequestBook` every other source goes through, has no outcome history to report and
shows "—" rather than a misleading zero.

### Added: vertical/layer functions for user-defined radar products

ROADMAP_NEW C1: the user-defined-product formula language (`wxdata::udp`) could only read one
gate at a time — `REF > 55 && ZDR < 1 ? REF : 0`, no way to ask "what's the highest reflectivity
anywhere above this point" or "what's the lowest correlation coefficient in the layer where
there's actual hail-core reflectivity". Five new functions reduce over a point's whole tilt
column instead of one gate: `max_vertical(expr)` / `min_vertical(expr)` (each with an optional
second "condition" argument — `min_vertical(CC, REF >= 40)` finds the debris/hail signature that
rides with a hail core, without needing the roadmap's own `where` syntax, which still isn't part
of this grammar), `max_layer(expr, lo, hi)` / `min_layer` / `mean_layer` (restricted to tilts
whose beam height falls in `[lo, hi]`), `first_height_above(expr, threshold)` /
`last_height_above(expr, threshold)`, and `count_above(expr, threshold)`. The gate inspector now
builds the column (every tilt's own reading at the clicked point, low to high) and evaluates
saved products against it; a formula using one of these functions anywhere else `wxdata::udp`'s
plain single-gate `evaluate()` is still called reads as missing rather than a wrong or guessed
number, since those call sites have no column to give it. Environmental-height inputs (freezing
level, -10C/-20C heights) are still not implemented, so a layer function's bounds have to be
literal metres for now rather than named levels.

### Added: "Blink A/B" model comparison mode

ROADMAP_NEW F6/J4: alongside the existing "A minus B" subtraction overlay and "View side by side"
(two linked panes), a new "Blink A/B" button alternates the active pane's own field between each
compared model's own value on a 1.5 s timer — a classic radar-analyst comparison technique, now
usable in one pane instead of needing two. It reuses `CompareA`/`CompareB` and the exact same
per-frame `fields_on` → `field_draws` path "View side by side" already goes through, so no
GPU/shader change was needed: blinking is just which one of the two layers is a member this
frame, flipped on a clock read from egui's own frame time rather than a continuous per-frame
animation (it schedules the next repaint exactly at the next flip). Turning on "View side by
side" while blinking stops the blink, since two persistent panes already show both fields at
once. The separate “Swipe A/B” entry above completes the other one-pane comparison mode; its
single callback draws both clipped halves without the two-callback prepare-order race identified
when blink first shipped.

### Fixed: station cards judged every network's staleness by the same 5-minute clock

ROADMAP_NEW N2: the station-card header colored the "updated Xs/Xmin ago" line amber past a flat
5-minute threshold regardless of which network reported it, so a completely normal 20- or
40-minute-old METAR read as stale next to a 1-minute Tempest reading that actually would be. Each
network now has its own threshold matching its real reporting cadence — METAR 75 minutes (its
hourly cadence plus slack for a late post), Weather Underground 10 minutes, Synoptic mesonet 20
minutes, Tempest unchanged at 5. Also added a computed run age ("3h 08m old") next to the source
inspector's absolute Issue/Run timestamps, so reading a model layer's age no longer means doing
the subtraction against the current time by hand.

### Added: three new analyst preset workspaces

ROADMAP_NEW J5: alongside "Chase", "National overview" and "Analysis", three new starter
workspaces — "Tornado analysis" (0.5° REF/SRV/CC/ZDR, four panes linked on camera/time/site/
cursor, with ProbSevere and storm cells on), "Hail analysis" (REF/ZDR/CC/KDP with the MESH hail
swath layer on), and "Mesoscale analysis" (one national-scale pane with GOES IR, CAPE, SRH, and
2 m dewpoint field layers). Each reuses exactly the same pane/link/overlay/field-layer mechanism
the first three starters already use — a preset is a saved arrangement, not a new capability. The
roadmap's fourth preset, "Forecast comparison" (HRRR/RRFS/ensemble probability vs.
observed/MRMS), isn't shipped: RRFS isn't a data source this app has and ensemble probability is
its own not-started roadmap item, and a preset built from only the pieces that already exist would
silently drop half of what it's supposed to show. The roadmap also asks "Hail analysis" to open
the sounding panel automatically; workspaces don't capture open windows by design, so that stays a
manual step.

### Fixed: the 3D volume/CAPPI clipped far storms out of the box entirely

The 3D reflectivity volume, its "Smooth" per-pane representation, and the CAPPI slice window all
built their grid to a fixed 150 km half-width regardless of what the radar actually scanned —
superres reflectivity commonly *declares* 300-460 km of gate capacity, so a storm beyond 150 km
was outside the volume before the clipping-plane slice ever got a chance at it; no slice angle
could recover data the volume never contained. `wxdata::volume3d::max_sample_range_km` now sizes
the volume to where a scan's own tilts actually report echo, not a guessed fixed radius — and, as
importantly, not the gate array's raw *declared* capacity either: a first version of this fix used
that declared capacity directly, and because most VCPs' reflectivity tilt reports hundreds of km
of capacity regardless of where the echo actually is, it ended up stretching nearly every volume
out to that near-maximum reach and visibly coarsening every ordinary nearby storm's cell size —
washing out real detail (a supercell's hail core, specifically) that used to be visible at the old
fixed-150-km resolution. The volume now follows real, spatially coherent echo instead — a single
isolated stray gate (ground-clutter/anomalous-propagation breakthrough under a temperature
inversion is the classic case) is rejected rather than trusted, so an isolated far speckle can't
blow the box out the same way. The "jump to this storm in 3D" action from the storm-cells popup,
which used the same fixed radius for its own clip-box math, now derives it from the same real
sweep data so the box stays meaningful against whatever actually gets built.

### Changed: every WSV3 ribbon group scrolls instead of overflowing

Every WSV3 ribbon group laid its content out in a fixed-height box with no scrollbar — content
past that height had nowhere to go, and depending on the group, either overlapped whatever came
after it or was simply unreachable (reported live: "Model", "Color fill", "MRMS national" and
"Tools" all have enough items to wrap past the ribbon's own height at ordinary window widths).
Fixed once at the root instead of one group at a time: `ribbon_group` now wraps every group's
content in a vertical `ScrollArea` itself, so all thirteen groups get it uniformly rather than
whichever ones a bug report happened to name. Unchanged in appearance when everything already
fits (no scrollbar appears) — the "Search"/"Data"/"View"/"Overlays"/"Capture" groups, which never
had enough content to overflow, look exactly as before.

### Added: overlay more than one model-contour field at once

Model contours (MSLP / 2 m temp / dewpoint / SB-CAPE / 0-3 km SRH / STP / SCP / EHI / lapse rates
/ effective-layer shear-SRH-STP) used to be an exclusive pick — turning one on turned the last one
off. Both contour pickers (the WSV3 ribbon's "Contours" group and the Environment drawer section)
are now checklists, so MSLP and CAPE, say, can be drawn together, each in its own color and each
independently fetched, cached, and refreshed on the model's own cadence. The command palette and
layers panel already showed a per-field checkmark; they now actually toggle that one field instead
of behaving like a hidden radio group. STP/STP-effective are hidden from both pickers, and cleared
from whatever is already active, when the selected source model has no LCL height to compute them
from (RAP analysis, NAM 12 km, NAM 3 km nest) — previously only the fixed-layer STP variant was
consistently hidden in the Environment picker even though the model-change guard cleared both.

### Added: synchronized cursor across linked panes

ROADMAP_NEW J3: a new "Link pane crosshair" toggle shares whichever pane is hovered as one
geographic point across every pane. Each pane draws its own crosshair at that point using its own
camera, so panes at different zooms or locations still mark the same spot rather than mirroring
one screen position — the point is cleared the instant the pointer leaves every pane, so a stale
mark never lingers. Alongside it, a compact always-visible "Cursor probe" table lists
Pane/Source/Product/Time/Value for every pane, reusing the same `inspect_gate` sampler the
Interrogate tool's click already used, so probing four panes at once needs no extra clicking.
Only radar-moment panes are sampled — there is no generic "read this pane's active grid at a
point" helper yet the way there is for a single clicked gate, so an MRMS/model-only pane's row
shows its site with a "—" for value rather than a guess. The "Chase" and "Analysis" starter
workspaces, which already link camera/time/site, ship with it on by default; "National overview"
(one pane) and older saved workspaces load with it off, same as the other link toggles.

### Added: run-to-run model comparison

ROADMAP_NEW F5: a new "Surface CAPE (run to run)" model-comparison field subtracts HRRR's current
run from its own immediately-previous run, both at the analysis hour for the same valid time —
how much the model's own initial state changed cycle to cycle, distinct from the existing
model-to-model comparisons which show two different models disagreeing at the same time. Backed
by a new `wxdata::hrrr::fetch_field_previous_run`, which walks back from a specific run (not
`Utc::now()`) so it can never return the same cycle as the one it's being compared against, with
the same multi-candidate fallback the rest of the aligned-fetch machinery uses if the immediately
prior cycle hasn't posted the field yet. Uses a tighter difference range than the cross-model CAPE
comparison, since a one-hour HRRR-to-HRRR change is typically much smaller than a genuine
cross-model disagreement. "View side by side" is hidden for this field — unlike a model-to-model
comparison, there's no second distinct single-model layer to show in a second pane, only the same
current-run CAPE twice, so the button is withheld rather than shipped half-working.

### Added: lock the shared analysis time to the radar's actual scan

ROADMAP_NEW A2's last open "lock to source frame" item: a new "Lock analysis to radar frame"
toggle (alongside the existing "Link pane analysis time") makes a linked seek settle on the active
radar's *actual* scan timestamp rather than retaining the valid time that was originally requested
— so satellite and MRMS layers reading the shared analysis cursor align to the real source frame
a NEXRAD scan landed on, not an approximation of it. Waits for the requested frame to actually
finish loading before locking, so an in-flight seek is never overwritten by the previous frame's
timestamp mid-request. Persisted per workspace the same way the other link toggles are (older
saved workspaces load with it off).

### Added: published coverage bounds for field metadata

`FieldDescriptor.valid_domain` now records whether a product is published for CONUS or globally,
separately from the exact bounds of an individual fetched grid. Field sampling enforces that
coverage. HRRR point soundings and global point-series requests now reject unsupported or invalid
coordinates before opening network requests, and regional `ModelDef` entries reuse the same
`GeographicBounds` type rather than maintaining a second bounds representation.

### Added: link panes to the same radar site

ROADMAP_NEW J2: alongside the existing "Link pane cameras" and "Link pane analysis time" toggles,
a new "Link pane radar site" makes picking a new site in one pane set it in every pane — each pane
keeps its own product and tilt, so this is for watching several products of one storm across
panes rather than turning every pane into a copy of the active one. The "Chase" and "Analysis"
starter workspaces, which already show one site across all their panes, now ship with it on by
default; older saved workspaces load with it off, same as the two existing link toggles did when
they were introduced.

Not full "link groups" in the sense the rest of J2 asks for (each pane independently choosing
which of several named groups to join) — this is a third global on/off, the same simpler shape
the pre-existing camera/time links already use. Documented in the roadmap as the simpler thing it
is rather than claimed as the fuller feature.

### Added: a proper 3-pane layout

ROADMAP_NEW J1: the pane-count picker only ever offered 1, 2, or 4 — asking for 3 panes any other
way (a saved workspace from a future version, say) fell through `pane_rects`' fallback, which
always built a 2x2 grid and truncated it to three cells, leaving one quadrant of screen visibly
blank instead of splitting the space three ways.

`pane_rects` now gives `n == 3` its own case: three columns in landscape, three rows in portrait,
the same adaptive-orientation rule the existing 2-pane split already uses. A fourth pill ("3×")
sits next to 1/2/4 in the ribbon, and a fourth entry joins the command palette's pane-count list.
`set_pane_count`'s own bounds already allowed three panes through — only the layout math and the
UI entry point were missing.

6 and 9 pane (also in ROADMAP_NEW J1) are a larger, separate change: several per-pane caches are
fixed-size 4-element arrays today, and generalizing those wasn't attempted here.

Verified with 5 new geometry tests (`pane_rects_tests`): exact rect count, adaptive orientation,
edge-to-edge tiling with no gaps or overlap, and that 4-pane's own 2x2 grid is unchanged.

### Fixed: 19 icon-only buttons had no accessible name

ROADMAP_NEW Q3: `ui::a11y::Named` exists precisely so a screen reader announces "Dismiss" instead
of reading out the private-use codepoint an icon font glyph sits at, and most of the app's
icon-only chrome already uses it. A two-pass audit — the second pass widening the search after the
first missed every `Button::new(RichText::new(icon))` form, not just the bare
`Button::new(icon)`/`small_button(icon)` ones — swept every icon-only button in the app and found
19 across 10 files that had fallen through: the scrubber's day-step carets, the GOES timeline's
frame-step carets, the archive calendar's month-step carets, the update-available chip's dismiss,
a settings field's clear button, the mobile chrome-hide eye and modal-sheet close button, an alert
rule's condition/delete buttons, the event library's per-row jump/remove-bookmark buttons, the
desktop and mobile "choose radar site" buttons, and the Layers panel's per-row favorite-star and
drag-to-reorder handle. Some had a tooltip but no accessible name (a bare `.on_hover_text` only
does half of what `.named()` does), several had neither. All 19 now carry a real name, several of
them specific to the row they're on (e.g. "Jump to {event name}", "Add {layer} to Favorites"
rather than one generic label repeated down a whole list).

Not a new enforcement mechanism — a future icon-only control can still reintroduce this gap
without anything catching it at compile time.

### Added: a one-click diagnostics export for bug reports

ROADMAP_NEW N4: a new "Export diagnostics…" button in Settings → Backup saves a JSON snapshot of
app version, platform, GPU renderer, every active source's current health, the last 200 warnings/
errors, on-disk cache size, and the process's performance counters — everything a bug report
usually needs a back-and-forth to collect, in one file. Saved through the same cross-platform
`dialog::save_bytes` the settings-backup export already uses (native save dialog, Android SAF,
browser download).

No new instrumentation: every field already existed somewhere in the app (Phase B3's health
tracking, `wxdata::stats`'s counters, the devlog capture buffer) except on-disk cache size, which
gets a new `paths::cache_dir_bytes`. The one new devlog function, `recent_warnings`, is a
non-destructive read — unlike `drain`, which the devlog shipper depends on to hand a batch off
exactly once, so a one-off export must not silently steal entries the shipper still needs to send.

Same privacy discipline as the existing crash reporter: no location history, API keys, private
tokens, or filesystem paths of the user's own.

### Added: a consolidated data source health window

ROADMAP_NEW N1: every active source's fetch health already existed (Phase B3's latency dashboard
tracks it per source, one hover popup at a time on that source's own Layers-panel row), but
"for every active source" meant checking them one row at a time — there was no single place to
see what's broken across the whole app.

A new "Data source health…" window, opened the same way as any other tool (command palette,
Layers panel), lists every currently-active, health-tracked source worst-first — Failed sources on
top, then Stale, then Waiting, then Fetching, then Fresh at the bottom. It adds no new health
tracking: it reads the exact same `SourceHealth` data and the exact same `active_layer` filter the
existing per-row popups already use, so the two views can never disagree about what counts as
active or what a source's status is.

Genuinely still open, not silently assumed: a rolling success/failure *count* (only the most
recent attempt is tracked), cache state, and fallback provider — `SourceHealth` has no fields for
any of the three yet, and no source in the app has a fallback provider to name in the first place.

### Added: radar coverage comparison between neighboring sites

ROADMAP_NEW C3's last open item, now closing out the whole section: the radar-suitability popup
(click a point, see nearby radars ranked by beam height rather than just distance) could only ever
answer "which site is best here" one point at a time. Every non-current row now has a "Compare"
button that paints the same beam-height math as a map overlay instead — blue where the current
site's beam is lower at that point, red/orange where the compared site's is, transparent inside a
~150 m deadband (real differences that small are noise against the beam-height model's own
approximations) and fading out past a ~4 km ceiling where neither radar is telling an analyst
anything about low-level structure anymore. Clicking "Compare" again on the same pair turns it back
off, and closing the popup clears it.

New `wxdata::suitability::candidate_at` (the per-candidate half of `rank`, pulled out so a caller
who already knows which two sites it wants doesn't need `rank`'s nearest-neighbor scan) and a new
`crate::coverage_compare` module. Pure geometry, same scope as `wxdata::suitability` itself — no
terrain, no network — so unlike the DEM-backed Blockage/Lowest-tilt overlays it rebuilds
synchronously on the UI thread rather than through a background fetch.

### Added: field-registry metadata for the global-model comparison layers

ROADMAP_NEW A1's last open migration item: `wxdata::global::GlobalField` (MSLP, 500 hPa height,
2 m temp/dewpoint, 10 m wind, precipitable water — the fields `fielddiff.rs`'s GFS/ECMWF compare
and difference layers read) had no `FieldDescriptor`, unlike the HRRR/RAP fields a prior pass
already migrated. Gave it one, the same way: a `GlobalField::descriptor()` table under a new
`DataSource::GlobalModels` (GFS/ECMWF/GEFS/GDPS span multiple agencies, so this names the class of
model rather than one publisher). `FieldLayer::descriptor()` now resolves the six `Global*` layers
through it, so they get provenance (`ui::data_inspector`), fuzzy search, and palette-by-metadata
for free — the same win the HRRR/RAP migration already banked. Two new `Unit`/three new
`PaletteId` values (`MetersPerSecond`; `Height500`, `Wind10m`, `PrecipitableWater`) cover the
fields that had no shared palette identity yet; MSLP/temperature/dewpoint reuse the regional
fields' existing `PaletteId`s, since it's the same physical quantity on the same ramp either way.

`fielddiff.rs`'s own `DiffField::units`/`range`/`input_scale` — which describe the *difference*
in forecaster-facing display units (hPa, dam, kt), not the source field's native GRIB units — are
a genuinely different concept from a field's own descriptor and were deliberately left alone
rather than folded into this migration. Verified with a new descriptor-completeness test
(`every_global_field_has_a_unique_searchable_descriptor`) and a palette-preservation test
confirming all six `Global*` layers still draw with the exact ramp they always did
(`global_catalog_palettes_preserve_existing_scales`).

### Added: a CAPPI-altitude reference plane in the 3D view

ROADMAP_NEW H4's last open item: the CAPPI window has sliced the volume at a constant altitude
as its own separate 2D tool for a while, but nothing showed *where* that altitude sat relative to
the storm in the 3D raymarch views — unlike the vertical clip plane added earlier this phase, whose
position is always visible as the edge of what it cuts away.

Both raymarch consumers (the standalone "3D Reflectivity" window and the main map's "3D map"
Smooth representations) gained a "CAPPI altitude" checkbox next to the existing vertical-plane
controls, sharing the CAPPI window's own altitude value rather than a second one — dragging its
slider moves the 3D marker live. `render3d::View3d` carries the new `cappi_km: Option<f32>`; a new
`cappi_marker_uniform` places it as a fraction of the box's own vertical span (the same "beam
height above the radar" unit `wxdata::volume3d::build`'s z-grid and `cappi`'s `alt_km` already
share), disabling it rather than pinning it to an edge when the altitude falls outside the volume's
own top. `raymarch.wgsl` composites it as a thin translucent band *behind* whatever the volume
itself draws, via an analytic ray/band intersection mirroring the box-slab test already there — real
echo always wins where a ray crosses both, so it reads as a reference plane rather than a haze.

Verified against real data on real GPU hardware via a new `--headless-3d ... --cappi ALT_KM` CLI
flag: with the reflectivity threshold pushed impossibly high (so the volume itself paints nothing),
a 3 km marker alone still put 346,318 echo pixels on screen, and a 25 km marker (above the volume's
18 km top) put zero — inert exactly as designed. A normal render at KTLX went from 264,023 to
351,840 echo pixels with the same 3 km marker enabled, all in the regions the storm itself doesn't
cover.

### Added: 3h/6h/12h MRMS QPE accumulation windows

ROADMAP_NEW D1/D3's QPE coverage gap: the catalog only had the two ends of the accumulation
range (1h, 24h) cataloged as `wxdata::mrms::catalog` products, leaving the 3/6/12-hour windows
unreachable even though rotation/lightning/hail already prove the multi-window `FetchMapping`
pattern works for this catalog. Added three new fixed catalog entries (`qpe3h`, `qpe6h`,
`qpe12h`) against the real `CONUS/MultiSensor_QPE_{03,06,12}H_Pass2_00.00` S3 paths, each a
standalone `FieldLayer` alongside the existing `Qpe1h`/`Qpe24h` — kept separate rather than
collapsed into one runtime-window-picker layer, since that would change the on-disk slug of the
two existing QPE layers and risk breaking saved workspaces that reference them.

They pick up the Layers panel/search integration for free (the panel already iterates
`wxdata::mrms::catalog::PRODUCTS`, so a new catalog entry needs no separate menu wiring) and
share the QPE layers' 2-minute refresh cadence. A new `PrecipitationAccum` palette/ramp
(0.25-150mm, log scale, the existing QPE color stops) sits between the 1h and 24h scales rather
than reusing either directly, since 150mm is a more plausible ceiling for a half-day window than
either endpoint's own scale. Verified live: all five QPE window paths
(`CONUS/MultiSensor_QPE_{01,03,06,12,24}H_Pass2_00.00`) resolve against the real MRMS S3 bucket.

### Added: a "lowest usable tilt" terrain overlay

ROADMAP_NEW C3's last open item: a gridded "which tilt actually reaches ground level here" layer,
distinct from the existing beam-blockage shading (how blocked is *this one displayed* tilt) and
from the per-site suitability ranking (one point at a time, not a map). `elevation::
lowest_usable_tilt_image` reuses the same terrain-occultation scan `blockage_image` already
performs, but instead of shading one fixed tilt's own blockage, it walks the volume's real
elevation list (low to high, SAILS/MRLE repeats deduplicated like the cross-section's beam-rise
lines) and colors each pixel by the lowest one that actually clears the terrain there — green
where the lowest tilt already does the job, through amber to red as a higher tilt becomes
necessary, purple where nothing in the volume clears at all.

New "Lowest usable tilt (terrain)" overlay toggle, built and cached the same way the existing
Blockage overlay already is (a background DEM-fetching task, a `LowestTiltKey` cache keyed by
site + the whole tilt list + the visible world rect, the previous raster kept on screen — stretched
to its own rect — while a rebuild is in flight so panning doesn't blink).

Verified against real terrain at KMAX (Mount Ashland, OR, a site with genuine beam blockage):
a single tilt known to clear the whole frame (6°) paints its own color everywhere in range with no
"nothing clears" pixels; a single tilt known to be significantly shadowed there (0.5°, the same
terrain the pre-existing blockage test measures) does produce "nothing clears" pixels, in exactly
the region that test already proved was real shadow.

### Fixed: the web deployment build failed to compile

`RadarUpload.telemetry` (the live-render-queue-latency timing added alongside the render-queue
metric above) was typed `std::time::Instant`, while every caller already carried
`wxdata::clock::Instant` — the two are the same type on native (`wxdata::clock` re-exports
`std::time::Instant` there) but distinct types on wasm32 (`web_time::Instant`), where
`std::time::Instant` can't measure elapsed time at all. `cargo check` on native never caught it;
Coolify's web build (`scripts/web/build.sh`, wasm32 target) did, failing every deploy since the
render-queue-latency feature landed. Fixed by giving the field the same cross-platform clock type
its callers already used. Verified: `cargo check --target wasm32-unknown-unknown` compiles clean.

### Added: shared metadata for regional model fields and contours

All twelve HRRR/RAP/NAM/NBM field meanings now expose `FieldDescriptor` metadata with stable IDs,
typed NOAA/NCEP source identity, native units, value kind, aliases, palette, and optional native
contour spacing. Existing future-radar, CAPE, SRH, rotation-track, snowfall, smoke, and thunder
layers resolve their colors through these descriptors. The HRRR/RAP contour fetch path now reads
its GRIB key and interval from the same model catalog instead of maintaining a second literal
table; pressure and temperature conversion remain display-unit aware.

### Added: typed product source identity

`FieldDescriptor.source` now uses a `DataSource` enum instead of a display string. Sources expose a
stable machine ID separately from their human provenance label, allowing future cache namespaces,
quotas, and filters to branch on identity without comparing UI text. All existing MRMS catalog
entries now use `DataSource::NoaaMrms`; product search includes both `noaa-mrms` and `NOAA MRMS`.

### Added: measured live radar render-queue latency

Each accepted live Level II update now carries a one-shot timestamp from the UI receive path into
the 2D radar upload. After the renderer has queued the changed polar-texture rows, uniforms, and
color table, it stores the elapsed receipt-to-queue duration. The Radar health popup displays this
as "Render queue" alongside the existing provider-lag and decode-time readings. It deliberately
describes the measurable CPU/GPU queue boundary rather than claiming the frame has already been
presented by the GPU.

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
