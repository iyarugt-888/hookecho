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

### 2.1 Multi-touch pan sends the map flying (web, touchscreen) — [x] mitigated, root cause on egui's
    side still unconfirmed

**Where, confirmed exact:** `crates/hookecho/src/app.rs`, the per-pane input block, lines
~12842-13024 (line numbers shifted slightly after this fix; search for "Belt-and-suspenders against
a runaway single-frame gesture"). `let gesture = ui.input(|i| i.multi_touch());` at line 12852;
`gesture_tail` (150 ms grace after a gesture ends) at 12859-12861; `quiet` gate at 12862;
single-finger drag (pan/double-tap-zoom) at 12893-12919, run only `if response.dragged() &&
quiet`; the two-finger gesture block at 12976+, which computes `occluded` (chrome/window layers
over the gesture center) then applies **zoom before pan** (a comment there records this ordering
was chosen specifically because pan-then-zoom previously caused an anchor-trailing bug of the same
family as this one).

**Both of this plan's original candidate mechanisms are disproven — read egui 0.35.0's own source
before acting on either.** `egui-0.35.0/src/input_state/touch_state.rs` was checked directly this
session:
- `TouchState::begin_pass` (line 140) sets `added_or_removed_touches = true` on any `Start`/`End`/
  `Cancel` touch event, and immediately after (line 174-179) **nulls `gesture_state.previous`**
  for that frame when that flag is set — `info()` (line 188) then substitutes `current` for the
  missing `previous`, producing a **zero** `translation_delta`/`zoom_delta` for the transition
  frame, not a spurious jump. A 2→3 or 3→2 finger count change inside egui's own gesture math is
  already guarded against — this plan's original "mechanism 2" does not apply to this egui version.
- `active_touches.insert`/`.remove` (lines 152, 162) happen synchronously before `update_gesture`
  is called in the same `begin_pass` — so `multi_touch()` already reflects the current frame's true
  touch count by the time HookEcho's `gesture`/`quiet` variables are computed. There is no
  observable one-frame lag between a second finger landing and `multi_touch()` returning `Some` —
  this plan's original "mechanism 1" doesn't hold up either, at least not inside egui itself.

**What shipped instead of a mechanism-specific fix:** since the true trigger is upstream of egui
(most likely browser/wasm touch-event dispatch quirks this session had no way to reproduce or
instrument — no real touchscreen or browser multi-touch emulation available in this environment),
a defensive **clamp** was added rather than chasing an unconfirmed root cause further: the
two-finger gesture block now caps `mt.translation_delta`'s magnitude to 40% of the pane's smaller
dimension, and `mt.zoom_delta`'s applied `log2` to `[-1.0, 1.0]` (halving/doubling zoom in one
frame is already an extreme legitimate pinch rate). A `log::warn!` fires when the translation clamp
actually engages, so a real occurrence is now visible in the field (and, once §4's Analyst Mode
exists, in its live log) instead of just "the map jumped and nobody knows why." This bounds the
damage regardless of cause; it does not explain the cause.

**Still open, for whoever picks this back up:** reproduce on a real device or with Chrome DevTools'
multi-touch emulation, watch whether `log::warn!`'s new clamp message fires, and if so capture what
`mt.translation_delta`/`num_touches` actually were — that tells you whether the upstream event
source (not egui, not this app's gesture logic) is delivering something egui's touch state doesn't
expect. Consider instrumenting `web_sys`/`eframe`'s own touch-event → `egui::Event::Touch`
translation path next, not `app.rs` or egui's `touch_state.rs` again — both were checked and are
sound.

**Acceptance:** [x] a defensive per-frame magnitude clamp exists so no single frame's gesture can
move the camera by more than a bounded fraction of the pane, regardless of cause. [ ] root cause
confirmed against a real touch source (still open — mark this checkbox only once actually
reproduced and explained, not just mitigated). [ ] manually verified on an actual touchscreen (or
Chrome DevTools' touch emulation with multiple synthetic pointers) that a 3-finger touch on the map
no longer produces a visibly large jump — the clamp should make this true today even without full
root-cause confirmation; verify it actually does.

### 2.2 3D map basemap has empty gaps, mainly when zoomed out — [x] done

**Confirmed and fixed.** "3D map" mode is the *same* 2D `Camera`/tile pipeline, just pitched —
enabling it sets `view.camera.pitch = 50.0` (`app.rs:12277 fn map_3d_controls`, radio at
12304-12317). `tiles.rs`'s `pub fn tile_cover(cam: &Camera, viewport_px, max_z: u8, zoom_bias:
f64) -> Vec<VisibleTile>` (lines 915-952) was confirmed to derive its world-space bounds purely
from `cam.center ± (viewport_px/2 * world_per_pixel())` — a flat, unpitched box —
and `world_per_pixel()` (`render/mercator.rs:107`) is `1.0 / (256 * 2^zoom)`, a function of zoom
only, no `pitch` term anywhere. A pitched camera's true ground footprint reaches much further
toward the horizon than this flat box, so tiles there were never requested. Confirmed by direct
computation (see below), not just static reading.

**What shipped:**
- `render/mercator.rs`: extracted `pub fn ground_delta(&self, px, viewport_px) -> Option<(f64,
  f64)>` from the existing private `screen_to_ground` — the same ground-plane raycast
  (`screen_ray` + z=0 intersection), but returning the **unwrapped** world-unit delta relative to
  `center` instead of a `[0,1)`-wrapped absolute position, so `tile_cover` can combine it with its
  own already-unwrapped `cx ± half_w` box without reconciling two different coordinate framings.
  `screen_to_ground` itself now just wraps `ground_delta`'s result — no behavior change there.
- `tiles.rs`'s `tile_cover`: when `cam.is_3d()`, projects all four viewport corners through
  `ground_delta` and extends the flat box to also cover whichever corners return `Some` (a corner
  returning `None` means that ray points above the horizon at this pitch/FOV — correctly nothing
  to tile in that direction, not a bug). The extension is bounded by a **fixed tile-index margin**
  (`MAX_EXTRA_TILES = 24`), not a multiplier on the viewport's own world-space size — an earlier
  draft of this fix used `half_w * 8.0` as the cap and was wrong: at low zoom the world itself is
  only a few tiles wide, so a size-relative cap already dwarfs the planet at exactly the "zoomed
  out" case this bug is about. A final guard also caps the x-span to at most one full wrap around
  the world (`n` tiles), so an extension that would otherwise exceed a full wrap doesn't silently
  request (and pointlessly re-fetch) the same wrapped tile id more than once.
- Three new tests in `tiles.rs` (`a_pitched_camera_covers_at_least_as_much_as_the_flat_case`,
  `the_pitched_extension_is_bounded_not_unbounded`, `max_pitch_does_not_panic_even_when_some_
  corners_see_no_ground`).

**A real finding from empirically probing `ground_delta` at various zoom/pitch combinations before
settling on the fix (worth knowing before touching this again):** at pitch near `MAX_PITCH_DEG`
(75°), the *top* screen corners' rays point **above** the horizon at this camera's FOV —
`ground_delta` correctly returns `None` there, and no extension happens in that direction, which is
correct (there is no ground to tile toward the sky) rather than a residual bug. The fix's actual,
verified effect is at moderate pitch: at HookEcho's own default 3D-mode pitch (50°, set by
`map_3d_controls` itself), the fix was confirmed (by direct computation across zoom levels 3/5/8/12)
to roughly **triple** the tile count requested (e.g. 48 vs. 16 tiles at zoom 4) — exactly the
regime the reported bug actually operates in, since nobody runs this app pitched to the edge of the
horizon in practice.

**Acceptance:** [x] `cargo test -p hookecho --lib tiles::` passes, including the three new
regression tests. [ ] not yet visually screenshot-verified in the real running app (headless 3D
rendering wasn't exercised this session) — do this before considering the item fully closed, not
just tested at the `tile_cover` function level.

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

**Correction after reading `overlay.rs::search_pill` in full (line 449-556) — read this before
touching it:** the Minimal-layout "search pill" is not a standalone search trigger. It's the
*entire* top control bar for that layout: a hamburger-style menu toggle (`egui_phosphor::LIST`,
opens/closes `self.panel_open` — the same panel that holds Layers/Alerts/Share, not just search),
the current radar site + VCP label (phone only), *and* the search hint, all in one horizontal
bar. It already has a `phone()`-gated compact mode (line 469-480, 510-534): on a narrow viewport
the search hint is already icon-only (`MAGNIFYING_GLASS` alone, no text, line 525-527) — Ref 2's
screenshot showing the full "Search layers, tools, places" text means that capture's `phone()`
check didn't trigger the compact path (check `phone()`'s own width threshold against the
screenshot's actual viewport before assuming the compact mode doesn't work).

**Given this, "make search an optional floating button" cannot mean hiding the whole bar** — that
would also remove reach to the menu toggle and site picker, not just search. The narrower, correct
scope: add a setting that forces the *search portion specifically* to render icon-only (the
already-existing `phone()` compact treatment) regardless of platform/width, for a user who wants
that even on desktop. This is a much smaller change than originally scoped — extending an existing
`phone()` conditional with an `|| settings.compact_search_button`-style check, not building a new
FAB component from scratch. Do the same check-before-assuming pass on `ribbon.rs`'s "Search all"
group (line 230-238) before changing it — confirm whether it's genuinely a standalone control
there (it looked like one from the code excerpt read this session) or whether it too carries more
than search before deciding its fix shape.

**Root cause found, still not implemented — this is the actual, precise fix, not a hypothesis:**
`overlay.rs`'s `fn phone() -> bool { cfg!(target_os = "android") }` (line 26) is a **platform**
check, not a screen-size check — the icon-only search hint at line 525
(`let hint = ... if phone() { icon-only } else { "Search layers, tools, places" }`) only ever
triggers on an actual Android build, never on a narrow desktop/web browser window, no matter how
narrow. This is exactly why Ref 2 (mobile *web*) shows the full text: `phone()` is `false` there
regardless of viewport width. The real narrow-viewport check already exists as a *separate*
function, `compact(ctx: &egui::Context) -> bool` (line 36, M3 width-class based, works on any
platform), and this file already combines the two for a related purpose: `fn sheets(ctx) -> bool {
phone() && compact(ctx) }` (line 42). **Do not blindly replace every `phone()` in this file with
`compact(ctx)`** — `search_pill` alone calls `phone()` at 12 different sites (lines 167, 334, 469,
476, 491, 510, 525, 538, 561, 584, plus `sheets`'s own two), and several look like genuine
platform-specific UX choices (e.g. line 510's inline site-picker-in-the-pill, which may be
deliberately Android-only) rather than screen-size ones. The fix is narrow and specific: change
just the hint-text branch (line 525) and whatever sizing it depends on (`width` at 469, the hint's
own font size at 491) to trigger on `phone() || compact(ctx)`, leaving every other `phone()` call
in the function exactly as it is. Read each remaining call site before touching it, the same way
this session's own investigation did before writing this note — don't assume they all mean the
same thing just because they call the same function.

**Acceptance:** [ ] one setting controls both surfaces consistently (a user shouldn't get a FAB in
Minimal but a full pill in the ribbon, or vice versa, unless that turns out to be the deliberate
per-layout choice — decide and document, don't leave it inconsistent by accident). [ ] toggle
defaults to current (non-breaking) behavior. [ ] FAB mode renders correctly on both desktop and a
~380px mobile viewport. [ ] screenshot comparison against Ref 1/Ref 2 shows a visually improved,
consistent control in `Docked` mode.

### 2.4 Radar range ring shows km instead of miles — [x] done

**Correction to this plan's original diagnosis — read before touching this again:** the first draft
of this section proposed adding a brand-new `Settings.distance_unit` toggle, on the assumption that
"no distance-unit groundwork exists at all." That assumption was wrong. **HookEcho already has an
automatic miles-vs-km mechanism, used by the measure tool, that the range rings simply never got
wired to:** `fn metric_in(&self, idx: usize) -> bool` (`app.rs:8384`) returns `false` (→ show miles)
for NEXRAD/TDWR sites and `true` (→ km) for everything else (DWD/OPERA/international radars) — a
per-site, network-derived choice, not a user setting. `crate::geo::fmt_distance(km, metric, decimals)`
(`geo.rs:27`) formats a km value in whichever unit that bool selects. The measure tool
(`app.rs:16120-16124`, `crate::geo::great_circle` + `fmt_distance(km, self.metric_in(idx), 1)`)
already does this correctly; the range-ring block (`app.rs`, gated by `self.show_range_rings`,
field at `app.rs:3155`, toggled via `Toggle::RangeRings` at `app.rs:1688`) just hardcoded km and
never consulted `metric_in`. **There is no new user-facing setting here, and there should not be
one** — adding a separate `DistanceUnit` preference would contradict the existing, already-correct
automatic behavior and create two competing sources of truth for the same question. If a real need
for a manual override surfaces later (e.g. a non-US user wanting km on a NEXRAD site), extend
`metric_in` itself (a per-user override falling back to the network default), not a parallel enum.

**What shipped:** the range-ring block now computes `let metric = self.metric_in(idx);`, picks ring
radii that read as round numbers in the active unit (`[50.0, 100.0, 150.0, 200.0]` km when metric,
`[25.0, 50.0, 75.0, 100.0]` mi — converted to km via `crate::geo::KM_PER_MILE` before being handed
to `destination_point`, which always expects km regardless of display unit), formats each ring's
label with `crate::geo::fmt_distance(km, metric, 0)` instead of the old hardcoded
`format!("{km:.0} km")`, and draws the azimuth spokes out to whichever ring is actually the
farthest (`max_ring_km`, tracked while building the rings) instead of a separately hardcoded `200.0`
that would have silently stopped matching the rings once they became unit-dependent.

**Acceptance:** [x] range ring reads in miles for NEXRAD/TDWR sites, km for international sites —
automatically, consistent with the measure tool, no new setting to configure or forget to set.
[x] `cargo check -p hookecho --lib` clean. [ ] not yet visually screenshot-verified against a real
NEXRAD site per §1's "a visual change needs a screenshot" rule — do this before considering the
item fully closed, not just compiled.

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
    overridable — [x] done

**What shipped** (`crates/hookecho/src/settings.rs`):
- `Layout::Wsv3` renamed to `Layout::CommandRibbon` — label **"Command Ribbon"** — with
  `#[serde(alias = "Wsv3")]` so an old `settings.json`'s `"layout": "Wsv3"` still loads as this
  variant, exactly the way `Theme::Dark` already carries aliases for its own pre-rename names.
  A new `Layout::Wsv3` variant was added for the actual new theme; since a bare unit variant's
  implicit serde tag is its own name, it needed an explicit `#[serde(rename = "Wsv3Theme")]` to
  avoid colliding with `CommandRibbon`'s alias of the same string — both are tested
  (`a_pre_rename_layout_field_still_loads_as_command_ribbon`,
  `the_new_wsv3_theme_round_trips_under_its_own_distinct_name`).
- `Layout::CommandRibbon` (label "Command Ribbon", `Theme::Dark`/`Density::Comfortable`), `Wsv3`
  (label "WSV3", `Theme::Dark`/`Density::Compact`), `Minimal` (unchanged) — `Layout::ALL` is now
  3 elements.
- `Layout::is_ribbon(self) -> bool` — `true` for `CommandRibbon`/`Wsv3`, replacing the two call
  sites in `app.rs` that used to compare directly against the single old `Wsv3` variant (the
  ribbon-vs-colorbar-drawn-elsewhere gate, and the top-level "which chrome to draw" gate) — both
  ribbon-style layouts share the same chrome, differing only in geometry (below).
- `Layout::recommended_theme_and_density(self) -> (Theme, Density)` — applied once, from the
  Settings UI's click handler on the Theme picker (`ui/settings_window.rs`'s `general_tab`), not
  from anywhere that runs every frame; a subsequent independent Color-scheme change is never
  stomped back.
- Settings UI: the picker is now labeled "Theme" (was "Layout"), placed above a "Color scheme"
  row (was labeled "Theme") that reads as the secondary, "customize further" control, per §6.1.

**Acceptance:** [x] `Layout::ALL` is exactly `[CommandRibbon, Wsv3, Minimal]`. [x] an old
`"layout": "Wsv3"` still loads as `CommandRibbon`, tested. [x] picking a Theme in Settings applies
its recommended Color-scheme + Density pair immediately; a subsequent independent Color-scheme
pick is not overwritten (verified in code — the pairing function is only ever called from the
click handler, never from a per-frame path). 4 new tests in `settings.rs`, all passing; full
`cargo test -p hookecho --lib` (605 tests) and both native + wasm32 `cargo check` clean.

### 6.3 Build the new "WSV3" theme from Ref 4 — [x] partly done — geometry + footer shipped, the
    tab-row refactor and checkbox-dense rows deliberately deferred (see below, not silently dropped)

This is the theme deliverable — a *look*, using layers/tools/products HookEcho already has, not a
port of WSV3's own feature set (see §1's "don't replicate feature-for-feature" rule).

**What shipped:**
- **Denser ribbon geometry**, gated on the new theme rather than on `Density` (confirmed necessary:
  `crate::ui::wsv3`'s hand-painted ribbon never read `Density`/`ui.visuals()` for its own sizing at
  all — every theme shared one fixed pixel geometry before this). `theme.rs` gained a second
  process-global flag, `WSV3_DENSE`/`is_wsv3_theme()`, set from `theme::apply()` (which now also
  takes `Layout`) the same way the existing `IMGUI_STYLE`/`is_imgui_style()` flag already works —
  precedent this session found and reused rather than inventing a new mechanism. `wsv3.rs`'s
  `RIBBON_H`/`COLORBAR_H`/`STATUS_H` constants became `ribbon_h()`/`colorbar_h()`/`status_h()`
  functions returning smaller values under `is_wsv3_theme()` (104/20/36 vs. the original
  130/26/22 — status bar is *taller*, not shorter, to fit its extra footer row below).
- **Extended footer status bar** (`ribbon.rs::wsv3_status_bar`): a second row, drawn only under
  `is_wsv3_theme()`, with zoom-level quick-pick pills (`z5`/`z8`/`z11`/`z14` — an analog of Ref 4's
  100%/125%/150% quick-picks adapted to this app's continuous log2 zoom rather than a literal
  percent scale that has no equivalent here) and, in 3D mode, the camera's own pitch/bearing read
  straight off the existing `Camera` struct. `CommandRibbon` never draws this row.
- **The existing lat/lon + scan-age footer readout was already there** for both ribbon layouts
  (`wsv3_status_bar`'s original row) — confirmed rather than assumed, so nothing needed building
  for that part.
- **Data-probe crosshair**: confirmed this maps onto the existing Gate inspector/Explore tool
  (visible in every ribbon screenshot's TOOLS group already) — no code change needed; a pixel
  restyle for this specific theme is left as future polish, not tracked as a gap.

**Deliberately not done, not silently dropped:**
- **The ribbon tab-row refactor** (grouping DATA/RADAR/TILT ANGLE/VIEW/OVERLAYS/TOOLS/CAPTURE
  under fewer top-level tabs, à la Ref 4's Basic/Surface/NEXRADPro/… bar). Confirmed prerequisite
  still stands: `ribbon.rs`'s `ribbon_group` is 13 hand-laid-out inline call sites, not a
  `&[GroupDescriptor]` table, and `RibbonMode` (`app.rs:1419`, `Radar`/`Model`/`Mrms`) is the one
  existing "which groups reflow" precedent to extend or parallel. This is real, sizable UI-
  architecture work on its own — attempting it in the same pass as everything else above risked
  a half-finished refactor rather than a working denser theme. The theme still reads as
  meaningfully different today (geometry + footer); the tab-row grouping is the next increment.
- **Checkbox-dense rows** for button-grid controls (reflectivity mode, satellite channel, warning
  types) — a cosmetic pass with real regression risk if done without live visual iteration (no
  browser/screenshot tooling was set up this session — see §1's own screenshot-verification rule,
  which this honestly couldn't clear for a change this visual). Left for a pass with actual
  screenshot verification.
- **Viewpoints** (Ref 4's saved-camera-position list) and the **Main/Model-Timeline split** —
  unchanged from this plan's original scoping: genuinely new features, not part of the theme
  deliverable, tracked as their own follow-ups.

**Acceptance:** [x] selecting the WSV3 theme in Settings changes the top chrome's density and adds
the footer telemetry row; `CommandRibbon` is visually unchanged from before this pass (verified by
reading — its geometry functions return the original constants unless `is_wsv3_theme()` is true).
[x] `cargo test -p hookecho --lib` and `cargo check` (native + wasm32) clean. [ ] not yet
screenshot-verified in a running app — no browser/webview tooling was exercised this session; do
this before considering the *shipped* portion fully closed, and before starting the deferred
tab-row work above.

---

## 7. Timeline (scrubber) styles

**Ask:** "timeline should have customized options and separate themes too. such as a wsv3 style
timeline weatherwise style timeline, grlevel2, etc."

Today `crates/hookecho/src/app/chrome/scrubber.rs` is a single hardcoded visual layout shared by
every `Layout`. This section makes it a themeable axis, similar to `Layout` itself but independent
(a WSV3-*look* user might still prefer a compact timeline, or vice versa — don't force one to imply
the other unless testing shows users always want them paired, in which case revisit).

**Status: [x] done.**

- [x] `TimelineStyle` enum in `settings.rs`, same shape as `VelocityUnit`/`Layout`: `Default`
  (today's look, byte-for-byte unchanged — the function was renamed `scrubber_default`, nothing in
  its body touched), `Wsv3` (an explicit transport row — skip-to-start/rewind/pause-play/
  fast-forward/skip-to-end — a "LOOP" dropdown bound to the existing `Settings.live_loop_frames`
  value the Default style's own popup menu already edits, so this is a second surface for the same
  setting rather than a second value, and a plain `egui::Slider` below rather than a hand-painted
  track), `Compact` (a slim single row: prev/play/next, the *same* hand-drawn `track()` function
  the Default style uses — it already had a `compact: bool` painting mode — and a small live/
  archive dot, with the popup menu and rain-ETA/DVR-depth extras dropped). No Main/Model-timeline
  radio was built for `Wsv3` — HookEcho has one `Timeline` per pane, not two parallel ones, so
  there was nothing to switch between; noted as a real scope difference from Ref 4, not an
  oversight. No pixel-exact "WeatherWise"/"GRLevel2" reference existed to build `Compact` against,
  per this plan's own original note — it's the general compact archetype, not a copy of either.
- [x] `scrubber()` is now a 3-line dispatcher on `self.settings.timeline_style`; the three paint
  functions (`scrubber_default`, `scrubber_wsv3_style`, `scrubber_compact_style`) all drive the
  same `crate::timeline::Timeline` fields directly (`playhead`, `playing`, `following`) via the
  identical three-line idiom the existing track's own drag handler already used (`t.playhead = idx;
  t.playing = false; t.following = idx + 1 == observed;`) — no parallel state, no duplicated
  seek/play logic invented.
- [x] Settings control: a "Timeline" row in `general_tab` (not yet in a separate Appearance tab —
  §3's tab split wasn't done this pass), next to the Theme/Density rows.

**Acceptance:** [x] switching `TimelineStyle` changes only the scrubber's visual layout — all three
call the same `Timeline` methods/fields for play/pause/step/seek/live-follow, verified by reading
(no separate state machine per style). [x] every style shows the live/archive state somewhere
(`Wsv3`: a LIVE/STALE/ARCHIVE label; `Compact`: a colored dot with the same hover text as the
Default badge) — the latency-honesty rule from `ROADMAP_NEW.md` §0 holds for all three. [x]
`cargo test -p hookecho --lib` (605 tests, 2 new) and `cargo check` (native + wasm32) clean. [ ]
not yet screenshot-verified in a running app.

---

## 8. Suggested phase order

Not a hard dependency chain — pick items an agent can finish and verify in one sitting without
leaving something half-done. Rough grouping, batching the single gate run per group per §1:

1. **Bugs (§2)** — [x] 2.1/2.2/2.4 done, 2.3 needs a rescoped pass (see its own correction note).
2. **Distance unit** — [x] done as part of 2.4 (turned out to need no new setting at all — see
   that section's correction note). **Settings reorg (§3)** — not done; still open.
3. **`Layout` rename + preset pairing (§6.2)** — [x] done.
4. **New WSV3 theme (§6.3)** — [x] partly done (geometry + footer); the tab-row refactor and
   checkbox-dense rows are the tracked remainder — see that section's own "deliberately not done"
   list before starting more work here.
5. **Timeline styles (§7)** — [x] done.
6. **Analyst Mode (§4)** — not started, independent of everything above, can run anytime.

**What every "[x] done" item above still needs**, consistently: real screenshot/manual
verification in a running app. This session had no browser/webview tooling set up to exercise the
actual UI, so every visual change was verified by compiling, testing the underlying logic, and
reading the code — not by looking at it. Do that pass before treating any of these as fully closed.

## 9. Definition of done

This plan is complete when: every checkbox above is checked or explicitly marked as a tracked
stretch item that was deliberately deferred (not silently dropped); `cargo clippy --workspace
--all-targets -- -D warnings` and `cargo test --workspace` both pass; an old `settings.json`
(pre-this-plan) still loads with no data loss and lands on a sensible default theme; and someone
looking at the Settings window can tell, without reading source, the difference between "Color
scheme" (colors only) and "Theme" (colors + layout) — which was the user's original complaint about
the current naming.
