# New suggestions — high value, low effort

A pass over `ROADMAP_NEW.md` looking for one specific thing: work that would **noticeably improve
the product** while needing **no architectural change and no large build**. Everything here was
checked against the actual code before being listed — each item names the real files and symbols
involved, and says what already exists versus what has to be written. Items the roadmap already
documents as blocked on real architecture (swipe dividers, ensemble workstation, RRFS ingest,
plugin sandbox, route engine) are deliberately absent: they are not small, and pretending
otherwise is how a suggestion list stops being useful.

Ranked by value-per-unit-of-work, highest first.

---

## S1. Export what's on the map as GeoJSON — ROADMAP_NEW I6

**Value: high. Effort: low.**

Phase I exists for "emergency-management, research and broadcast users." Those users' actual
workflow is HookEcho → QGIS/ArcGIS/a briefing deck. Right now **nothing geographic can leave the
app**: drawn annotations, saved markers, watch zones, storm cells and active warning polygons are
all held in lon/lat and none of them can be written out.

Everything needed is already here:

- `geojson` 1.0.0 is already a `wxdata` dependency, and `wxdata::gis` already models every
  geometry type (I3, done).
- `dialog::save_bytes` already saves cross-platform (native dialog / Android SAF / browser
  download), already used by settings export, diagnostics export, GPX and palettes.
- Every candidate layer is already lon/lat: `Stroke2d.points`, `settings.markers`
  (`Marker { lon, lat, name }`), `zone_pts`, `overlay::GeoFeature.rings`, `level3::Cell { lon, lat,
  mvt_deg, mvt_kt, max_dbz, vil, … }`.

So this is a writer plus a menu entry, not a feature build. It also completes the round trip with
the GeoJSON **import** that just landed (I1), which is the thing that makes both halves worth
having.

## S2. Draw imported GIS points and lines, not just polygons — ROADMAP_NEW I1/I4

**Value: high. Effort: low.**

The GeoJSON importer currently renders `Polygon`/`MultiPolygon` and *counts and reports* the
`Point`/`LineString` features it can't draw (see `gis_import.rs`'s own doc comment). That was the
honest call at the time, because `overlay::GeoFeature` is rings-only. But a GIS file of city
points or a river/road network is exactly the kind of file an EM user imports, and today it
imports as "0 shapes drawn, 214 point features not drawn yet."

The renderer for this already exists and is eight lines long: `render_pane` paints freehand
annotation strokes by mapping `[lon, lat]` through `mercator::lonlat_to_world` →
`camera.world_to_screen` → `egui::Shape::line`. Points can reuse the same projection with
`painter.circle_filled`, which is how markers are already drawn. No new pipeline, no GPU work —
imported non-polygon geometry goes in its own small `Vec` and paints in that same block.

## S3. Zoom to imported shapes — ROADMAP_NEW I1 usability

**Value: medium-high. Effort: trivial.**

Import a county file for a state you aren't looking at and nothing appears to happen — the shapes
are real, they're just off-screen. `GeoFeature::bbox()` already exists and already returns
`(min_lon, min_lat, max_lon, max_lat)`; `Camera::at_lonlat(lon, lat, zoom)` and
`Camera::world_per_pixel` are all that's needed to frame it. A "Zoom to imported shapes" action
turns a confusing import into an obviously-working one.

## S4. Next/previous product keys — ROADMAP_NEW J6

**Value: medium. Effort: trivial.**

J6 asks for "product next/previous" and it is the one item there still open for a reason other
than ambiguity. `1`–`7` already jump to a specific moment, but cycling is what you want when your
hand is on the mouse and you're stepping through REF → VEL → CC on one storm. `Moment::ALL`
already exists and `hotkeys.rs`'s `BindableAction` + `apply_action` already have the exact shape
for this (`TiltUp`/`TiltDown` are the same three-line pattern).

## S5. A real toggle for 3D — ROADMAP_NEW J6

**Value: medium. Effort: low.**

The map-pitch 3D view can only be reached by opening a pane's options panel and clicking a
`selectable_value` 2D/3D pair buried in `app.rs`. It is not in the command palette, not in the
Layers panel, and has no shortcut — J6 lists "3D" as open for exactly this reason. The toggle
itself is one bool plus a camera pitch/bearing reset that already exists inline; lifting that into
a `PaletteAction` makes it reachable from every surface at once (palette, drawer, hotkey), because
those surfaces are already generic over `PaletteAction`.

## S6. Read the value of any gridded layer under the cursor — ROADMAP_NEW D3/J3/E6

**Value: very high. Effort: medium (the only item here that isn't small).**

Listed because it is the largest *functional* hole found in this pass, not because it is cheap.
Today you cannot read the numeric value of **any** MRMS, model or satellite layer anywhere in the
app — only radar gates can be sampled (`inspect_gate`). `FieldDescriptor::sample` exists, is
correct and unit-tested, and is called from nowhere; D3's "point sample" checkbox was corrected to
`[x]/[ ]` once that was found.

Two real obstacles, both noted honestly here rather than glossed:

1. `FieldState` keeps only the GPU-bound 8-bit LUT-index upload, not the decoded floats, so there
   is nothing to sample from after upload.
2. Retaining the floats is not free: a national MRMS grid is ~7000×3500, i.e. ~98 MB as `f32`,
   per enabled layer. A resolution-capped retained copy (or retaining only for layers the user
   probes) is a design decision worth making deliberately rather than in passing.

The *surfacing* half is easy once (1) is solved — `render_pane` already shows a hover tooltip with
value and units for `ModelDiff`/`CompareA`/`CompareB` via `diff_hover_value`, and generalizing that
established pattern beats redesigning J3's radar-shaped probe table.

## S7. Keep imported GIS across a restart — ROADMAP_NEW I1

**Value: medium. Effort: low.**

Imported shapes are session-only today (documented as such). For a file a user imports once and
wants every session — a district boundary, a coverage area, a set of assets — re-picking the file
on every launch is friction the app doesn't impose anywhere else. Settings already persist
arbitrary JSON, and the browser build already stashes picked file content in `settings.web_files`
for exactly this reason.

## S8. Style the imported layer — ROADMAP_NEW I4

**Value: medium. Effort: low.**

Imported shapes draw in one fixed neutral blue. A color and opacity control is the 80% of I4 that
matters, and `ui::layer_window` already has the exact opacity-slider pattern for placefile layers.
Labels-from-attribute and graduated color are the larger, genuinely-deferred half.

## S9. Absolute-difference mode for comparisons — ROADMAP_NEW F5

**Value: medium. Effort: low.**

Comparisons draw only the signed difference. "Where do these disagree at all, regardless of
direction" is the more common question when scanning a whole domain, and `FieldRamp` already has
`RampScale::Abs` — it is used for azimuthal shear, whose sign is direction rather than magnitude.
The mechanism is built; no comparison field opts into it.

## S10. Surface each source's latest valid data time — ROADMAP_NEW N1

**Value: medium. Effort: low.**

N1 asks for "latest valid data time" and only radar has one, because the generic health path only
ever computed an *age of the last successful fetch*. But `FieldState.stamp` already carries a real
`DataStamp.valid_time` for every gridded layer — the value is sitting one field away from the
health row that wants it. "Fetched 40 s ago" and "the data is 14 minutes old" are different facts
and operationally it's the second one that matters.

---

## Deliberately not suggested

- **C14 longwave IR / C01 / C03 / C05 ABI channels** — trivial to add, but the roadmap's own
  assessment is right: they read almost identically to channels already shipped and have no
  standalone analyst use case until E4's RGB recipes need them as inputs. Adding them would grow
  the layer list without improving anything.
- **D3 accumulation selector** — would change the on-disk slug of two long-lived QPE layers and
  break saved workspaces. Correctly avoided once already.
- **Swipe divider (F6/J4)**, **disagreement mask (F6)**, **6/9-pane asymmetric layouts (J1)**,
  **ensemble workstation (F7)**, **RRFS/RTMA ingest (F2/G1)**, **plugin sandbox (P1–P3)**,
  **route engine (L1–L5)** — all genuinely large. They belong on the roadmap, not on a
  low-effort list.

---

## What was implemented

S1 → S2 → S3 → S4 → S5, in that order: it finishes Phase I's import/export story end to end (a
file comes in, draws completely, can be found on the map, and map data can go back out) and then
closes the two J6 items that were open for buildable rather than ambiguous reasons.

| | Shipped as |
|---|---|
| S1 — GeoJSON export | `feat(i6): export what's on the map as GeoJSON` |
| S2 — imported points/lines draw | `feat(i1): draw imported GIS points and lines, and frame the import` |
| S3 — zoom to imported shapes | same commit as S2 |
| S4 — next/previous product | `feat(j6): add product cycling and make the 3D view reachable` |
| S5 — 3D toggle | same commit as S4 |

**S9 was already in flight** and is not listed above: an in-progress `DiffMode` implementation was
found uncommitted in the working tree and did not compile (one legend call site had not been
updated for its new argument). It was finished rather than rewritten — including wiring
`DiffMode::apply`, which was reachable only from its own test despite clearly having been written
for the cursor readout, so a magnitude-colored map was still handing back signed values.
Shipped as `feat(f5): finish the absolute-difference comparison mode`.

Also shipped alongside, from a direct report rather than the roadmap: a **beam-rise control** for
the 3D Observed view, where each tilt's genuine climb with range made distant scans flare steeply
upward and turned a multi-tilt volume into a stack of cones. Scales how much of that climb is
drawn, 100% (true geometry) down to 0% (flat). `feat(3d): add a beam-rise control to the Observed
view`.

S6, S7, S9 and S10 shipped in later passes. S8 now ships too: Layer Manager has persistent color
and opacity controls for the one imported GIS layer, applied consistently to polygons, lines and
points. The remembered file also returns visible after restart, and the stale command description
that still claimed points/lines were not drawn has been corrected.
