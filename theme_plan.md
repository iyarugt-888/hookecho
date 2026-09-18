# HookEcho — Theme, Chrome & UX Overhaul Plan

> **Purpose:** implementation plan for Claude Code, Codex and other coding agents working the
> visual/UX side of HookEcho: theming architecture, chrome layouts, the timeline, search, a new
> analyst mode, and a handful of concrete bugs. This is a sibling to `ROADMAP_NEW.md` (data/backend
> roadmap) — that file's section 2 ("Mandatory engineering rules for coding agents") applies here
> too and is not repeated in full; this document adds rules specific to visual/UX work.
>
> **Scope:** `crates/hookecho` chrome, theming, settings UI, timeline, search, touch input, and the
> 3D map. Not in scope: `wxdata` decoders, the radar-ingest backend, or anything covered by
> `ROADMAP_NEW.md`'s Phase A-N data work.
>
> **Snapshot date:** 2026-09-17. Branch: `feat/wsv3-redesign`.
>
> **Source material:** five screenshots and a video the user provided this session, referenced
> throughout as "Ref 1" (mobile web, current WSV3-ribbon layout), "Ref 2" (mobile web, current
> Minimal/map-first layout with the floating search pill), "Ref 3" (WeatherFront, `app.weatherfront.com`
> — a left-sidebar layout, general inspiration only, not a literal target), "Ref 4" (a frame from a
> WSV3 desktop app video — the dense tab-based control panel, 3D viewport with camera/FPS telemetry,
> and the Viewpoints panel), "Ref 5" (same as Ref 1, a second capture). The video's own title —
> "WSV3 V6.0 ALPHA: New Viewpoints, Camera, Footer UI Controls, Orderly Logical Settings
> Reorganization" — is itself a work breakdown WSV3's own team used; this plan borrows that
> structure.

---

## 0. What already exists — read this before touching anything

HookEcho's settings already separate three independent axes. **Do not invent a fourth thing that
duplicates one of these** — the user's ask ("palette = colors only, theme = colors + UI") is
mostly a *presentation and naming* problem, not a missing architecture:

| Axis | Type | File | Current variants |
| --- | --- | --- | --- |
| Color scheme | `Theme` (`crates/hookecho/src/settings.rs:14`) | resolved to a `Palette` struct in `crates/hookecho/src/theme.rs` | `Dark` (default), `Light`, `System`, `Synthwave`, `Aurora`, `HighContrast`, `Oled`, `DearImGui` |
| Chrome layout | `Layout` (`crates/hookecho/src/settings.rs:1046`) | dispatches between `crates/hookecho/src/app/chrome/ribbon.rs` (ribbon chrome) and `crates/hookecho/src/app/chrome/overlay.rs` (floating map-first chrome) | `Wsv3` (default, label "WSV3 ribbon"), `Minimal` (label "Minimal (map-first)") |
| Spacing/type scale | `Density` (`crates/hookecho/src/ui/m3.rs:321`) | consumed by the `ui::m3` design-token layer | `Comfortable` (default), `Compact` |

`crates/hookecho/src/app/chrome/scrubber.rs` (the bottom timeline) is **shared** by both layouts —
Ref 1 and Ref 2 show an identical timeline bar despite completely different top chrome. This is
why timeline styling (§7) is its own axis, not tied to `Layout`.

**The naming collision to avoid — confirmed, it's a three-way overload, not just two:**
1. `theme.rs`'s own private `struct Palette` (a resolved theme's raw colors — `bg`/`accent`/etc.).
2. The Settings window's "Palettes" tab (`crates/hookecho/src/ui/settings_window.rs:461`,
   `palettes_tab`) — GRLevelX `.pal` radar moment color tables, unrelated to UI colors.
3. `PaletteAction`/`PaletteEntry`/`palette_entries()` (`app.rs:2035`,
   `app/chrome/registry.rs:299`) — the Ctrl+K command-search registry every toggle/window/tool
   funnels through. Also unrelated to UI colors.

Do **not** introduce "Palette" as a new user-facing UI-color concept — it collides with #2 (user-
visible) and #3 (a load-bearing internal name used everywhere). See §6.1 for the naming this plan
commits to instead.

**Also confirmed:** `Theme` and `Layout` are already fully independent `Settings` fields (`settings.rs:137,140`)
— any of the 8 `Theme` colors combines with either `Layout`. Nothing bundles them today. The gap is
presentation (both are just two rows in one `general_tab` grid, `ui/settings_window.rs:853+` —
Default site → Poll interval → **Theme** [with a live swatch preview] → Accent color → **Density** →
**Layout** [hidden on Android] → Motion, all in one `egui::Grid`) and in the styling engine
(`theme.rs`'s `fn apply()`, line 242, takes `Theme`/`Density`/accent but no `Layout` — only
`Theme::DearImGui` currently varies non-color geometry, via a separate process-global
`IMGUI_STYLE: AtomicBool` threaded around outside `Settings` entirely, `theme.rs:189`; every other
theme shares one geometry table). §6.2 below is what actually ties a `Layout` choice to chrome+color
together; until then, "theme" (bundled) doesn't really exist even though the two settings sit right
next to each other.

---

## 1. Rules specific to this work (in addition to `ROADMAP_NEW.md` §2)

- **Batch changes; do not run the full gate after every file.** The user explicitly asked that
  agents not burn time/tokens on constant back-to-back test runs. Work a phase (or a clearly-scoped
  sub-item) to completion, then run `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo test --workspace` **once** before marking it done. `cargo check -p hookecho` (fast) is fine
  to run more often while iterating on one file; the full gate is the expensive step to batch.
- **A visual change needs a screenshot, not just "it compiles."** `scripts/shots/` already exists
  for this (see `docs/GUIDE.md`/README's Verification section) — use it, or `--headless-*`, to
  confirm a chrome/theme change actually looks right in both a light and dark `Theme`, at both
  `Density` settings, before checking a box. Do not screenshot after every micro-edit; screenshot
  once when a checkbox item is believed done.
- **`app.rs` must not grow.** All of this work is chrome/UI — the natural home for new code is
  `crates/hookecho/src/app/chrome/` (new files there, following the existing `ribbon.rs`/
  `overlay.rs`/`scrubber.rs` split), `crates/hookecho/src/ui/`, `crates/hookecho/src/theme.rs`, or
  `crates/hookecho/src/settings.rs`. Touch `app.rs` only where a call site must change (e.g. wiring
  a new `Layout` variant into whichever `match` dispatches chrome), and prefer adding a match arm
  over adding a block of logic there.
- **Every new user-facing setting is `#[serde(default)]`** with a sensible default, per the existing
  convention throughout `settings.rs` — old `settings.json` files must keep loading.
- **cfg discipline**: `Layout`/`Theme`/`Density` are used on every platform (desktop/web/Android).
  A new variant must render sanely on a narrow (~380px) mobile viewport, not just desktop —
  `crates/hookecho/src/app/mobile/` is the existing mobile-specific chrome split; check whether a
  new Layout variant needs a mobile counterpart or can share the Minimal mobile path.
- **Don't replicate WSV3 feature-for-feature.** Ref 4's WSV3 video shows real *features* (a
  Viewpoints/saved-camera-position database, per-layer contour/vector-field toggles, a
  Main-Timeline/Model-Timeline split) alongside pure *look* (dense tabbed control rows, a footer
  with FPS/RAM/zoom-%, a data-probe crosshair readout). This plan's "new WSV3 theme" (§6) is the
  **look**, built from HookEcho's existing layers/tools. Anything that is a genuinely new capability
  (the Viewpoints database, in particular) is called out as an explicit **stretch item**, separate
  from the theme deliverable, so implementing the theme doesn't silently balloon into a new feature.

---

## 2. Bugs

Each bug below has enough of a lead to start; where the exact root cause needs confirming, that's
called out rather than guessed at further. Fix these independently of the theme work in §6-9 —
they affect every layout/theme.

### 2.1 Multi-touch pan sends the map flying (web, touchscreen) — [ ] not started

**Where, confirmed exact:** `crates/hookecho/src/app.rs`, the per-pane input block, lines
~12842-13024. `let gesture = ui.input(|i| i.multi_touch());` at line 12852; `gesture_tail`
(150 ms grace after a gesture ends) at 12859-12861; `quiet` gate at 12862; single-finger drag
(pan/double-tap-zoom) at 12893-12919, run only `if response.dragged() && quiet`; `zoom_delta()`
wheel/trackpad path explicitly skipped `if gesture.is_some()` at 12951-12965; the two-finger
gesture block itself at 12976-13024, which computes `occluded` (chrome/window layers over the
gesture center, 12977-12997) then applies **zoom before pan** — a comment at 13000-13002 records
that this ordering was chosen specifically because pan-then-zoom "over-moves the map by the pinch's
own scale factor," a previously-fixed anchor-trailing bug of the same family as this one.

This is already fairly mature, gesture-aware code, not naive — which narrows where a new bug can
hide. Two candidate mechanisms, both worth instrumenting/testing for rather than picking one blind:

1. **A one-frame recognition lag.** `quiet` depends on `multi_touch()` returning `Some` the *same
   frame* a second finger lands. If egui (particularly its web/wasm touch-event path) needs one
   frame to recognize the second touch point, the single-finger drag block (12893-12919) can fire
   once, unopposed, with whatever `drag_delta()` a half-registered two-finger touch produced before
   `quiet` goes false — a single large, wrong pan.
2. **A touch-count change mid-gesture** (2→3 fingers, e.g. a resting thumb, or 3→2 on lift). Egui's
   `MultiTouchInfo::translation_delta`/`zoom_delta` are computed from the centroid of however many
   points are currently down; a count change moves the centroid discontinuously between frames
   (independent of any real finger motion), producing one spurious large delta.

**Fix approach:** whichever mechanism reproduces, the fix shape is the same — extract a pure,
testable function that decides "should this frame's touch delta be applied at all" (currently this
logic is inline in the `app.rs` block, untestable without a full `egui::Context`), and have it
reject a frame where the touch-point count just changed (mechanism 2) and/or extend `gesture_tail`-
style suppression to cover the *start* of a gesture, not just its end (mechanism 1) — e.g. require
one frame of an unchanged, non-zero touch count before trusting that frame's delta. A cheap
belt-and-suspenders addition regardless: clamp the applied translation to a generous per-frame pixel
ceiling before calling `pan_pixels` — a real two-finger pan does not move hundreds of pixels in one
frame, so this catches whichever mechanism is actually firing without needing to prove which one.

**Acceptance:** [ ] a 3-touch-point synthetic input sequence (2 fingers panning, a 3rd touching down
mid-gesture) does not produce a multi-hundred-pixel single-frame pan — cover this with a unit test
against whatever pure function ends up owning the "should this frame's delta be applied" decision
(extract it from the `app.rs` block into a testable free function rather than leaving it inline).
[ ] manually verified on an actual touchscreen (or Chrome DevTools' touch emulation with multiple
synthetic pointers) that a 3-finger touch on the map no longer causes a runaway pan.

### 2.2 3D map basemap has empty gaps, mainly when zoomed out — [ ] not started

**Confirmed: there is no separate 3D basemap system to debug.** "3D map" mode is the *same* 2D
`Camera`/tile pipeline, just pitched — enabling it (`app.rs:12277 fn map_3d_controls`, the "2D"/"3D
map" radio at lines 12304-12317) does nothing more than set `view.camera.pitch = 50.0`. So this bug
lives in the ordinary basemap tile-visibility code, just newly exposed by viewing it obliquely
toward the horizon.

**Where, confirmed exact:** `crates/hookecho/src/tiles.rs`, `pub fn tile_cover(cam: &Camera,
viewport_px, max_z: u8, zoom_bias: f64) -> Vec<VisibleTile>` (lines 915-952) — the zoom-level-
dependent tile enumeration. Note its zoom clamp: `let z = (cam.zoom + zoom_bias).round().clamp(2.0,
max_z as f64) as u8;` — zoomed out never goes below tile zoom level 2. `tile_cover` takes the
`Camera` (which carries `pitch`) but nothing in its signature or the surrounding code read this
session suggests the *area* it covers is widened for a pitched camera — it looks like straightforward
screen-rect-to-world-rect coverage, which under a flat (`pitch: 0`) camera exactly matches the
screen, but under a pitched camera the *ground* visible on screen extends much further toward the
horizon than that same screen rect would cover at pitch zero. **This needs one confirmation step
before fixing:** read `tile_cover`'s body (not fully traced this session) to see whether it derives
its world-space bounds from `cam.screen_to_world()` at the four screen corners (which — if `pitch >
0` — already accounts for pitch correctly via perspective) or from a simpler flat-projection
shortcut that ignores pitch. Only implement the fix below if the latter.

**Fix approach** (once confirmed): compute the covered world-space bounds from the actual ground
positions the pitched camera's screen corners project to (`Camera::screen_to_world`, `render/
mercator.rs`, already pitch-aware per its own `pitched_camera_roundtrips_ground_points` test at
~line 361) rather than a flat shortcut — i.e. make sure `tile_cover` is *called* with (or itself
derives) the true pitched footprint, not that new pitch-math needs inventing. Prefer a conservative
over-fetch (a few extra tiles) over gaps; the existing tile cache already bounds memory, so
requesting slightly more than strictly necessary at high pitch is safe. If the fetch/placeholder
logic downstream of `tile_cover` (in `tiles.rs`'s cache, or `vector_tiles.rs`, 1547 lines, not
traced this session) is what's actually dropping tiles rather than `tile_cover` itself under-
requesting them, that's the next place to look — don't assume the root cause without checking both.

**Acceptance:** [ ] no visible gap in the basemap when zoomed out with the 3D map enabled and pitch
near its max, tested at a few representative zoom/pitch combinations via `--headless-*` or a
manual screenshot comparison (before/after).

### 2.3 The search pill doesn't look good and should be an optional floating button — [ ] not started

**Confirmed: there are two separate search trigger surfaces, both feeding one shared panel — fix
both consistently, don't just fix the one in the screenshot.**

1. The WSV3 ribbon's own **"Search all"** pill — `crates/hookecho/src/app/chrome/ribbon.rs:230-238`,
   inside the ribbon's "Search" group. This is very likely the exact control the user means by
   "search all button" (it's the literal label). Note Ref 1/Ref 5's screenshots don't show a visible
   "Search" group before "DATA" — confirm whether it's scrolled off (the ribbon groups scroll
   horizontally on a narrow viewport) or conditionally hidden before assuming it's simply missing.
2. The Minimal layout's floating **"Search layers, tools, places"** pill —
   `crates/hookecho/src/app/chrome/overlay.rs:525-551`, an `egui::Area::new(egui::Id::new(
   "search_pill"))`. This is the one visible in Ref 2.

Both set the same three fields on click (`self.panel_open = true; self.show_alert_panel = false;
self.sidebar_focus_search = true;`) to open the same drawer/registry-search panel
(`app/chrome/registry.rs`'s `palette_entries()`) — there is no separate command-palette *popup*
component to rewrite, only these two trigger affordances.

**Ask, restated:** today both are always-shown, fixed-position/fixed-size controls. The user wants
(a) better visual polish and (b) optionality — a small floating action button (FAB) toggle-able
on/off, rather than a permanent bar/pill taking up chrome space.

**Fix approach:** add a `Settings` field (e.g. `search_trigger_style: SearchTriggerStyle { Docked
(default, today's behavior), Fab }`) and a toggle for it (General/Appearance tab). When `Fab`, both
`ribbon.rs`'s "Search all" group and `overlay.rs`'s search pill render as a small round icon-only
button instead of their current full pill; tapping either opens the exact same panel/focus behavior
already wired — don't touch the panel logic, only the trigger widget. While in here, restyle the
`Docked` variant too (rounded pill, consistent shadow/elevation with Minimal's existing right-edge
control column, so it doesn't look like a third, unrelated visual language).

**Acceptance:** [ ] one setting controls both surfaces consistently (a user shouldn't get a FAB in
Minimal but a full pill in the ribbon, or vice versa, unless that turns out to be the deliberate
per-layout choice — decide and document, don't leave it inconsistent by accident). [ ] toggle
defaults to current (non-breaking) behavior. [ ] FAB mode renders correctly on both desktop and a
~380px mobile viewport. [ ] screenshot comparison against Ref 1/Ref 2 shows a visually improved,
consistent control in `Docked` mode.

### 2.4 Radar range ring shows km instead of miles — [ ] not started

**Where, confirmed exact:** `crates/hookecho/src/app.rs`, lines ~15793-15829, gated by
`self.show_range_rings` (field declared `app.rs:3155`, toggled via `Toggle::RangeRings`,
`app.rs:1688`). The four ring radii are a hardcoded array, and the label format bakes in the unit:

```rust
for km in [50.0, 100.0, 150.0, 200.0] {
    // ... polyline via crate::geo::destination_point ...
    if cam.zoom >= 6.0 {
        painter.text(..., format!("{km:.0} km"), ...);
    }
}
```

Azimuth spokes at 45° intervals out to 200 km live in the same block. This is **not** in `render/`
— it's drawn directly in the per-pane paint routine in `app.rs`.

**Confirmed: no distance-unit groundwork exists at all** (unlike `VelocityUnit`/`TempUnit`, which
already exist and this should mirror exactly — see `settings.rs`'s `VelocityUnit`, ~line 1104, for
the shape: a `Copy` enum, an `ALL` const array, `label()`, a conversion factor/method). This is a
**US-only app** (per `ROADMAP_NEW.md`'s own scope line) — miles should be the default, not an equal
toggle defaulting to metric.

**Fix approach:** add `DistanceUnit { Miles (default), Kilometers }` to `settings.rs`. Convert the
ring radii array and spoke length to the display unit at the point they're computed (keep
`destination_point`'s own input in whatever unit it actually expects — check its signature rather
than assuming km — and convert only for the ring-radius values and the label text), and change
`format!("{km:.0} km")` to read from the setting. Add a Units-tab row for it
(`ui/settings_window.rs`'s `fn units_tab`, line 560, already has the exact `ui.horizontal` +
`selectable_value`-over-`ALL` idiom for `VelocityUnit`/`TempUnit`/`TimeDisplay` — copy that pattern).
**Scope note:** CAPPI altitude sliders (`ui/cappi_window.rs`, `" km"` suffix) and similar
scientific-altitude inputs are a judgment call — altitude in km/kft is common even in US-market
tools (GR2Analyst uses kft for altitude, miles for range); don't reflexively convert every "km"
string in the codebase, only ground-range/distance displays like this one. Note any deliberately-
left-as-km decision inline with a comment explaining why.

**Acceptance:** [ ] range ring shows miles by default, km when the setting is switched, at the same
four "ring intervals" conceptually (i.e. don't just relabel 50/100/150/200 km as if they were miles
— pick sensible mile radii, e.g. 25/50/75/100 mi, and recompute the polylines from those). [ ] a
unit test on the conversion factor, matching whatever test style `VelocityUnit`/`TempUnit` already
use if any exist.

---

## 3. Settings reorganization

The reference video's own title calls out "Orderly Logical Settings Reorganization" as a real
WSV3 v6 workstream — borrow that framing. Confirmed exact current tabs and dispatch
(`crates/hookecho/src/ui/settings_window.rs`, `enum Tab` + the match at lines 132-139): General
(`general_tab`, free fn, line 853), Palettes (`self.palettes_tab`, line 461), Units (`units_tab`,
line 560), Basemaps (`basemaps_tab`, line 691), Alerts (`alerts_tab`, line 1107), Hotkeys
(`self.hotkeys_tab`, line 320), Sync (`sync_tab`, line 791), Storage (`self.storage_tab`).

`general_tab`'s single `egui::Grid` ("general_grid") currently holds, in this exact order: Default
site → Poll interval → **Theme** [ComboBox + live swatch preview] → **Accent color** [custom RGB
override] → **Density** [segmented control] → **Layout** [segmented control, hidden on Android] →
Motion (`reduce_motion`). The four bolded rows are exactly what moves.

- [ ] **Split "General" into "General" and "Appearance."** General keeps: default site, poll
  interval, motion/reduce-motion, UI scale, "Getting started," Background, Workspaces. Appearance
  gets the four grid rows above (Theme/Accent/Density/Layout — relabeled per §6.1: "Color scheme"
  for `Theme`, "Theme" for `Layout`), plus the new distance-unit control if it doesn't fit better
  under Units, plus the new timeline-style control (§7). The "Radar relay (advanced)" section added
  this session (`radar_relay_section`, `general_tab`, line 651) can move to Appearance too, or stay
  in General under its own "Advanced" heading — pick one and be consistent, don't split
  advanced/diagnostic settings across two tabs without a reason.
- [ ] Add a new top-level `Tab::Appearance` variant and an `appearance_tab` free function, following
  exactly the `basemaps_tab`/`units_tab`/`alerts_tab` shape (a free function taking `&mut egui::Ui,
  &mut Settings`) — don't make `general_tab` a method just to share `self` state it doesn't need.
- [ ] Leave "Palettes" (radar `.pal` tables, `palettes_tab`) exactly where it is, unrenamed — see
  §0's naming collision note.
- [ ] Add the new Analyst Mode toggle (§4) to Appearance or General — a "Diagnostics" mini-section,
  consistent with where the diagnostics-bundle export button already lives (`app.rs`'s own "Backup"
  drawer section, not this settings window — check whether Analyst Mode belongs there instead, for
  consistency with where other diagnostic-adjacent controls already live).
- [ ] While in this file: there are **two `fn storage_tab` definitions** (lines 152 and 262,
  presumably `#[cfg]`-gated native/web variants) — not this plan's problem to fix, but if either is
  actually dead code rather than a real platform split, flag it in a commit message rather than
  silently leaving a growing pile of confusion for the next person touching this file.

**Acceptance:** [ ] every existing setting is still reachable (nothing silently removed, only
regrouped). [ ] `cargo test -p hookecho --lib settings::` still passes (the `roundtrips`/
`tolerates_unknown_and_missing_fields` tests catch a broken serde shape, not tab layout, but confirm
nothing in this reorg touches the `Settings` struct's serialized shape unless intentional).

---

## 4. Analyst Mode (verbose live logging)

**Ask:** a settings option enabling "more in-depth logging such as a live log of each beam coming
in live with tilt data etc."

**What already exists, confirmed:** `crates/hookecho/src/devlog.rs` (header comment, lines 1-16)
captures **every** `log::` record process-wide into a ring buffer (`static BUFFER:
OnceLock<Mutex<VecDeque<LogEntry>>>`, capacity 8,000, line 45) via a wrapping `log::Log`
implementation (`NativeLogger`/`WebLogger`) — nothing per-subsystem needs registering, every
existing `log::debug!(target: "...", ...)` call site anywhere in the codebase is already captured
automatically, keyed by `record.target()`. `fn recent_warnings(limit)` (line 83) is today's only
reader (feeds the N4 diagnostics bundle). Live-sweep events are **already logged** under target
`"hookecho::live_sweep"` (`app.rs`'s `poll_messages`, the `DataMsg::Volume`/`DataMsg::Live`
handlers) with volume name, valid time, `live_poll` flag, changed-tilt count, decode time, and
retry count.

**What's confirmed genuinely missing** (don't assume more exists than this): **no** in-app live log
viewer UI anywhere (only the out-of-process `devlog_admin` server reads captured logs today), **no**
per-subsystem log-level concept (one global `log`-crate level, not a table), and **no**
`Settings`-persisted toggle at all — devlog today is entirely env-var/URL-param gated
(`HOOKECHO_DEVLOG=...`), never a normal in-app setting. So Analyst Mode is genuinely new UI and a
new setting, built on top of the existing generic capture buffer rather than a small extension of
an existing categorized system:

- [ ] Add `Settings.analyst_mode: bool` (default `false`).
- [ ] A new `ui::analyst_log_window` (or similar): a scrollable, auto-following live view reading
  `devlog`'s `BUFFER` (needs a new non-destructive accessor alongside `recent_warnings` — that one
  filters to WARN/ERROR only; this needs every level, filtered by target instead), filtered to the
  targets that matter for radar analysis (`hookecho::live_sweep` at minimum; also worth including
  anything under `provider_health`/`failover_arbiter`/`radar_provider_manager` from this session's
  B6 work — that's exactly "beam/provider health" detail an analyst would want). Gate the window's
  *existence* on `analyst_mode`, not just its visibility, so it costs nothing when off.
- [ ] Confirm whether the existing `debug!` call sites fire regardless of the process's `RUST_LOG`/
  `env_logger` filter level because `devlog`'s capture wraps the logger *before* level filtering, or
  whether a debug-level log is dropped before ever reaching `BUFFER` when the ambient level is
  `info`. If the latter, Analyst Mode needs to raise the effective level (globally, since there's no
  per-target table today) while it's on, which is a bigger behavioral change than just adding a
  viewer — confirm this before scoping the rest of the work.
- [ ] If the existing `live_sweep` log lines are missing obviously-useful per-beam detail the user
  asked for by name — **tilt/elevation angle, VCP** — check whether `ScanProgress` (from Phase B2,
  already carries elevation number/angle/chunk-in-sweep) is already in scope to add to the log
  line's format string before writing new instrumentation from scratch.

**Acceptance:** [ ] toggling Analyst Mode on/off in Settings opens/closes the live log window on the
next relevant event, no restart needed. [ ] the window shows real per-chunk/per-beam lines during an
actual live stream (verify against a real site, not just that it compiles). [ ] negligible
performance cost when Analyst Mode is off (a boolean check, not a parallel logging path always
running).

---

## 5. Distance-unit and other small consistency items

Folded into §2.4's acceptance criteria — no separate work here beyond what's already scoped there.
(Kept as a numbered section only so the phase numbering below stays stable if this plan grows.)

---

## 6. Theme system rework

### 6.1 Naming — commit to this, don't re-litigate it mid-implementation

- **"Color scheme"** — the user-facing label for the existing `Theme` enum (Dark/Light/Synthwave/
  Aurora/HighContrast/Oled/DearImGui/System). **Do not rename the Rust type** (`Theme` stays `Theme`
  in code — a rename here is pure churn across a wide blast radius for zero behavioral gain); only
  change the Settings UI's visible label wherever it's currently shown as "Theme."
- **"Palettes"** — unchanged, still means GRLevelX radar `.pal` moment color tables. Never reused
  for anything else.
- **"Interface theme"** (or just **"Theme"**, now that the color enum is relabeled "Color scheme"
  and the collision is gone) — the user-facing name for what's internally the `Layout` enum, which
  this section expands from 2 to 3+ variants and turns into a real preset system (§6.2). The Rust
  type can stay named `Layout` (same churn-avoidance reasoning), or be renamed to `Theme` if that
  reads better in code once `settings::Theme` is clearly the "color scheme" type — implementer's
  call, but pick one and don't leave both a `Theme` and conceptually-also-a-theme `Layout` type
  with confusing names side by side in the source without at least a doc comment on each explaining
  which is which.

### 6.2 Turn `Layout` into a real preset (color + UI), while keeping colors independently
    overridable — [ ] not started

- [ ] Rename the existing `Layout::Wsv3` variant to something that isn't "WSV3" — the user
  explicitly wants the *current* ribbon theme renamed because "WSV3" is being claimed by the new,
  more faithful redesign (§6.3). Suggested: `Layout::CommandRibbon`, label **"Command Ribbon"** (or
  "Classic Ribbon" — pick one; either reads fine). This is today's Ref 1/Ref 5 look, unchanged
  visually — a rename only.
- [ ] Add `Layout::Wsv3`, label **"WSV3"** — the new theme, built in §6.3.
- [ ] Keep `Layout::Minimal` as-is (label unchanged: "Minimal (map-first)").
- [ ] Give each `Layout` variant a **recommended** `(Theme, Density)` pair (e.g. `CommandRibbon` →
  `(Theme::Dark, Density::Comfortable)`, `Wsv3` → `(Theme::Dark, Density::Compact)` — WSV3's own
  look is dense, per Ref 4 — `Minimal` → whatever it defaults to today). Selecting a Layout in
  Settings **applies** its recommended pair immediately (a one-time convenience, like picking a
  starter preset), but `Theme`/`Density` remain independently changeable right after — nothing
  should re-force them back if the user then picks a different Color scheme. Implement this as a
  plain function (`Layout::recommended_theme_and_density(self) -> (Theme, Density)`), called only
  from the settings-UI click handler that changes `settings.layout` — not from anywhere that runs
  every frame, or a user's deliberate Color-scheme override would keep getting stomped.
- [ ] Settings UI: relabel wherever `Layout::ALL`/`Layout::label()` is rendered (General or the new
  Appearance tab, §3) from "Layout" to "Theme," and move the (relabeled) Color-scheme picker to
  read as a secondary, "customize further" control underneath it — visually subordinate, not equal
  billing, so a casual user picks one Theme and is done, while a power user can still go further.

**Acceptance:** [ ] `Layout::ALL` includes exactly `CommandRibbon`, `Wsv3`, `Minimal` (or your
chosen names) after the rename. [ ] an old `settings.json` with `"layout": "Wsv3"` (the pre-rename
serialized value) still loads correctly as the renamed `CommandRibbon` variant — add a `#[serde(alias
= "Wsv3")]` on `CommandRibbon` exactly the way `Theme`'s own variants already use `#[serde(alias =
...)]` for their pre-rename names (see `Theme::Dark`'s `alias = "Magma", alias = "Redline", alias =
"AcidStorm"` at `settings.rs:16` for the pattern) — **this is not optional**, it's what keeps every
existing user's settings file from silently reverting to the default layout on upgrade. [ ] picking
a Theme (Layout) in Settings visibly changes both chrome and color together; picking a different
Color scheme afterward does not get overwritten by anything.

### 6.3 Build the new "WSV3" theme from Ref 4 — [ ] not started

This is the theme deliverable — a *look*, using layers/tools/products HookEcho already has, not a
port of WSV3's own feature set (see §1's "don't replicate feature-for-feature" rule).

**Prerequisite, confirmed necessary:** `ribbon.rs` is **hand-laid-out UI code, not data-driven** —
there is no table/array of group descriptors. `impl HookEchoApp { pub(crate) fn wsv3_ribbon(...) }`
(line 126) is one long function that calls a helper `fn ribbon_group(ui, label, width, add_closure)`
(line 23) inline, once per group, each with its own hardcoded pixel width and bespoke closure body
(13 groups total: Search 230/Data 241/Radar 255/Tilt angle 318/View 404/Model 448/Color fill 497/
Contours 522/Future radar 552/MRMS national 621/Overlays 647/Tools 671/Capture 699 — line numbers of
each `ribbon_group(...)` call). There **is** an existing precedent worth building on: `RibbonMode`
(`app.rs:1419`, values `Radar`/`Model`/`Mrms`) already reflows which groups render — radar-only
groups skip entirely outside `RibbonMode::Radar`, etc. Adding a genuinely new tab-row (Ref 4's
Basic/Surface/NEXRADPro/MRMS/Model/MESO/Forecast/NDFD/NWS-SPC/Tropical/Winter/Misc-style grouping)
on top of the *existing* groups means either (a) extending `RibbonMode`-style mode-gating with a
second, orthogonal "which tab" dimension, or (b) actually refactoring `ribbon_group`'s 13 inline
call sites into a real `&[GroupDescriptor]` the WSV3 theme iterates differently than
`CommandRibbon` does. Do (b) if the theme needs more than 3-4 top tabs or expects tabs to be
reorderable/configurable later; (a) is the smaller change if a fixed, small number of tabs is
enough. Either way, **this refactor is a real prerequisite, not a detail** — budget for it before
starting the visual work below.

- [ ] **Denser top chrome**: group the *existing* ribbon groups (DATA/RADAR/TILT ANGLE/VIEW/
  OVERLAYS/TOOLS/CAPTURE) under fewer always-visible top-level tabs (à la Ref 4), each expanding a
  denser control row when active, rather than showing every group at once. Depends on the
  prerequisite above.
- [ ] **Compact, checkbox-dense rows** for anything that's currently a button grid (Ref 4's radio-
  button/checkbox rows for reflectivity mode, satellite channel, warning types) — apply `Density::
  Compact`-style spacing by default for this theme (per §6.2's recommended pair), and consider
  whether some of ribbon.rs's existing toggle-buttons read better as checkboxes/radio rows in this
  theme specifically (a visual variation the shared underlying `PaletteAction`/registry can serve
  without needing two copies of the action list).
- [ ] **Footer status bar**: FPS/RAM-style performance readout (`crates/hookecho/src/perf.rs`
  already exists per ARCHITECTURE.md — "The perf counters' readout — native only" — check what it
  already exposes before adding new counters), current zoom level as a `%`-style quick-pick (Ref
  4's "100% / 125% / 150%"; HookEcho's own zoom is a continuous camera zoom level, not discrete
  percents — translate sensibly, e.g. a small row of preset zoom buttons rather than a literal
  percent since the underlying representation differs), and the existing lat/lon + scan-age readout
  already visible in Ref 1's bottom-left/bottom-right corners (confirm this already exists outside
  the WSV3 theme and just needs restyling, vs. needs building — Ref 1 is the *current* ribbon theme
  and already shows `KTLX · scan 3s ago` / coordinates / zoom, so this is very likely already there
  and just needs to be part of the new theme's footer treatment, not built from scratch).
- [ ] **Data-probe crosshair + readout** (Ref 4's "Data Probe" section with a crosshair icon and
  "Start" button): check whether this maps onto HookEcho's existing "Gate inspector"/"Explore" tool
  (visible in every ribbon screenshot's TOOLS group) — if so, this is a restyle/rename for the WSV3
  theme's chrome, not a new tool. Only build new interaction if Gate inspector genuinely can't serve
  this role.
- [ ] **Camera/3D telemetry overlay** (Ref 4's lat/lon, camera pitch/bearing, LP/Cam/GB readout,
  visible only in the 3D viewport): a small text overlay shown only when `map_3d.enabled` and this
  theme is active, reading straight off the existing `Camera` struct (`render/mercator.rs`) —
  `pitch`, `bearing`, and world coordinates already computable via `screen_to_world`. No new state.

**Stretch, not required for this theme to ship** (call out clearly if left undone, don't silently
drop):
- [ ] Viewpoints (Ref 4's saved-camera-position list with Apply/Delete/Play-Viewpoints/Interval) —
  a genuinely new feature (a small `Vec<Viewpoint { name, camera: Camera }>` in `Settings`, a
  window to manage it, a "fly to" animation). Track this as its own follow-up item if picked up;
  it's a feature, not a theme, and shouldn't block §6.3's other checkboxes.
- [ ] Main-Timeline/Model-Timeline split with independent loop lengths — relates more to §7
  (timeline styles) than to this theme's own chrome; cross-reference rather than duplicate.

**Acceptance:** [ ] selecting the WSV3 theme in Settings changes both the top chrome density/grouping
and applies the recommended dark/compact defaults. [ ] screenshot comparison shows a
recognizably-WSV3-inspired look (denser, tabbed, footer telemetry) without literally cloning Ref 4
pixel-for-pixel — HookEcho's own tool/layer set is different from WSV3's, so exact parity isn't the
goal, character is.

---

## 7. Timeline (scrubber) styles

**Ask:** "timeline should have customized options and separate themes too. such as a wsv3 style
timeline weatherwise style timeline, grlevel2, etc."

Today `crates/hookecho/src/app/chrome/scrubber.rs` is a single hardcoded visual layout shared by
every `Layout`. This section makes it a themeable axis, similar to `Layout` itself but independent
(a WSV3-*look* user might still prefer a compact timeline, or vice versa — don't force one to imply
the other unless testing shows users always want them paired, in which case revisit).

- [ ] Add `TimelineStyle` enum in `settings.rs`, same shape as `VelocityUnit`/`Layout`: `Default`
  (today's look — the big pill with play/back/forward, the "Live" badge, "Scan Nm ago," the `...`
  menu — unchanged, this is the migration-safe default), `Wsv3` (Ref 4's style: explicit transport
  buttons — skip-to-start/rewind/pause-play/fast-forward/skip-to-end — a Main/Model timeline radio,
  a "Loop length" dropdown, and a plain position slider below, no big pill), `Compact` (a slim
  dot/tick strip with a small prev/play/next row — the genre convention for GR2Analyst/WeatherFront-
  style compact scrubbers seen in Ref 3's bottom bar; there's no pixel-exact "WeatherWise" or
  "GRLevel2" reference in this session's material, so build this as the general compact archetype
  and let a follow-up refine it against real screenshots of those specific apps if pixel-parity
  turns out to matter).
- [ ] Refactor `scrubber.rs` so the underlying `Timeline` state/logic (in `crates/hookecho/src/
  timeline.rs`, unaffected by this work) is drawn by one of several small render functions chosen by
  `settings.timeline_style`, rather than duplicating the state logic per style. One `Timeline`, three
  paint functions.
- [ ] Settings control for it — Appearance tab (§3), near the Theme/Layout picker.

**Acceptance:** [ ] switching `TimelineStyle` changes only the scrubber's visual layout; play/pause/
scrub/Live-follow behavior is identical across all three (verify by exercising the same interaction
sequence — play, scrub to a past frame, return to live — under each style and confirming the
underlying `Timeline` state ends up the same). [ ] no style regresses the existing Live-badge/
scan-age information Ref 1 already shows — every style must still answer "is this live, and how old
is the newest scan" somewhere, per this app's own existing latency-honesty rule
(`ROADMAP_NEW.md` §0's "Definition of top tier analyst tool": display latency/age must always be
visible).

---

## 8. Suggested phase order

Not a hard dependency chain — pick items an agent can finish and verify in one sitting without
leaving something half-done. Rough grouping, batching the single gate run per group per §1:

1. **Bugs (§2)** — independent of everything else, safe to parallelize across agents, each is its
   own PR-sized unit.
2. **Distance unit (§2.4)** + **Settings reorg scaffolding (§3)** — the reorg needs somewhere to put
   the new distance-unit control, so land the tab split first, then the unit itself slots in.
3. **`Layout` rename + preset pairing (§6.2)** — do this *before* §6.3, since §6.3 adds a new
   variant and it's much less churn to add "the third variant" than to rename one out from under an
   already-built new theme.
4. **New WSV3 theme (§6.3)** — the biggest single item; expect it to be its own multi-session effort.
5. **Timeline styles (§7)** — independent of §6.3, can run in parallel with it once §6.2's rename has
   landed (so it isn't built against the soon-to-be-renamed variant name).
6. **Analyst Mode (§4)** — independent of all of the above, can run anytime.

## 9. Definition of done

This plan is complete when: every checkbox above is checked or explicitly marked as a tracked
stretch item that was deliberately deferred (not silently dropped); `cargo clippy --workspace
--all-targets -- -D warnings` and `cargo test --workspace` both pass; an old `settings.json`
(pre-this-plan) still loads with no data loss and lands on a sensible default theme; and someone
looking at the Settings window can tell, without reading source, the difference between "Color
scheme" (colors only) and "Theme" (colors + layout) — which was the user's original complaint about
the current naming.
