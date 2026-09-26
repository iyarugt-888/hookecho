# HookEcho — WSV3 × Dear ImGui Modern Workstation Design Plan

> Branch: `feat/wsv3-redesign`
>
> Goal: redesign the existing WSV3 interface so it behaves like a serious radar workstation rather than a web dashboard with ImGui colors. The map is the primary workspace. Controls should be compact, dockable, discoverable, and fast to operate with mouse, keyboard, or touch.

## 1. Core design direction

The target is a hybrid of three ideas:

- **WSV3:** map-dominant analyst workstation, dense top control strip, persistent transport, fast access to overlays and data products.
- **Dear ImGui:** flat dark tool chrome, dockable windows, compact spacing, visible state, utilitarian controls, almost no decorative framing.
- **Modern workstation UX:** stronger hierarchy, cleaner grouping, adaptive density, progressive disclosure, responsive narrow-screen behavior, and clearer status/provenance.

Do **not** copy WSV3 literally. Use its workstation model and information density, but keep HookEcho's own features, terminology, architecture, and platform constraints.

## 2. Primary layout

### 2.1 Map-first center workspace

The radar/map viewport should own roughly 75–85% of the usable desktop canvas in the normal state.

The map remains visually uninterrupted except for:
- a compact vertical tool rail,
- a thin dBZ legend,
- optional floating probe/legend windows,
- warning / alert overlays,
- temporary contextual popovers.

Permanent left + right sidebars should not both be open by default.

### 2.2 Top command ribbon

Replace the current broad group-strip feeling with a **two-tier compact command ribbon**.

**Row 1 — application / workspace**
- HookEcho title + active workspace
- Radar / Models / Satellite / Surface / Analysis / GIS tabs
- Inspector
- Playback
- Discussion
- Tools
- Settings
- Help
- timestamp / timezone
- connection / latency state

**Row 2 — contextual controls for the active tab**
For Radar:
- site
- VCP
- product / moment
- tilt
- follow-lowest
- 2D / 3D / Volume
- smoothing
- color table
- range rings
- warnings
- storm tracks
- lightning
- data probe

Keep each control compact and mostly one-line. Avoid large pill groups and large empty gutters.

### 2.3 Dockable tool windows

Use ImGui-style utility windows for:
- Layers
- Inspector
- Product settings
- Warnings / alerts
- Analyst log
- Model controls
- Sounding / hodograph
- 3D volume controls
- GIS / placefiles
- Diagnostics

Each should support:
- dock left / right / bottom
- float over map
- collapse to title bar
- close
- remember layout per workspace

Default desktop arrangement:
- Layers docked left at ~250–300 px only when open.
- Inspector docked right at ~270–320 px only when explicitly opened.
- Timeline docked bottom.
- Everything else floats or is tabbed into those docks.

## 3. Layers window redesign

The existing feature breadth should remain, but the presentation should become denser.

### 3.1 Structure

Top:
- search field
- filter button
- Active toggle
- Favorites toggle

Main tree:
- Radar products
- Radar sites
- National weather
- Severe weather
- Observations
- Forecast models
- Satellite & observation
- Maps & overlays
- Custom layers

Use compact ImGui tree rows, not card tiles.

Each tree row:
- disclosure triangle
- icon
- label
- active count / total count
- optional status indicator

Each active layer should support:
- checkbox
- drag reorder
- visibility
- opacity
- product settings
- remove

### 3.2 Radar product rows

Keep analyst-friendly terminology already present in HookEcho:
- Rain intensity (reflectivity)
- Wind toward/away (velocity)
- Turbulence (spectrum width)
- Drop shape (ZDR)
- Phase shift (PhiDP)
- Rain rate in cores (KDP)
- Debris and mixtures (CC)
- Storm rotation (SRV)
- Composite reflectivity
- VIL / derived
- Echo tops
- MEHS / POSH etc.

The selected item gets one restrained blue highlight row, not a large button.

## 4. Inspector / selected-layer window

The right-side inspector should be contextual rather than always full of generic settings.

Header:
- active product name
- source / radar
- VCP
- tilt
- valid time
- scan age
- live / stale / archive state

Context body:
- exact gate / grid value
- azimuth
- range
- beam height
- sample time
- quality flags
- provenance

Action row:
- Product settings
- Site info
- Pin inspector
- Remove / hide

For 3D, add:
- volume mode
- active tilts
- temporal provenance
- beam-rise percentage
- quality preset
- camera pitch / bearing

## 5. Timeline redesign

Use a thin persistent bottom dock inspired by WSV3 transport controls.

### Main row
- jump to start
- previous frame
- play / pause
- next frame
- jump to latest
- frame scrubber
- current timestamp
- speed selector
- live / archive indicator

### Secondary row
- scan-frame dots / tick marks
- current tilt progress
- optional model forecast-hour ticks
- range of buffered history
- live stream activity

When in forecast mode, show explicit `F+xh`.
When in observed mode, show actual valid time and age.

The timeline should never visually compete with the map.

## 6. Visual language

### 6.1 Base palette

- app background: near-black charcoal
- tool windows: dark neutral blue-charcoal
- title bars: slightly lighter than panel body
- borders: 1 px low-contrast gray-blue
- selected / active: restrained medium blue
- warning: operational yellow/orange/red only where semantically meaningful
- text: off-white, with gray secondary labels

Avoid gradients except where meteorological data itself needs one.
Avoid glossy pills.
Avoid glassmorphism.
Avoid giant rounded cards.

### 6.2 Geometry

Desktop target:
- 2–4 px corner radius maximum
- 22–26 px compact control height
- 26–30 px title bars
- 4–6 px internal padding
- 6–8 px group spacing
- 1 px separators

Use monospace for:
- coordinates
- timestamps
- VCP
- dBZ / velocity values
- diagnostics
- performance
- scan provenance

Use regular UI font for labels and navigation.

### 6.3 State clarity

Selected state should use:
- background fill change
- optional 2 px accent edge

Hover should be subtle.
Disabled state should be visibly dimmed, not hidden.
Live / stale / delayed / archive must always be explicit.

## 7. Map tool rail

A single narrow vertical rail over the left side of the map:
- pointer
- pan
- zoom / fit
- measure
- probe
- drawing / annotation
- layers
- locate
- warning focus
- 3D camera tool

Use 32–36 px square buttons.
Only one primary interaction tool can be active at a time.

## 8. Basemap and data rendering priority

The UI should visually recede behind the meteorological data.

- road/city labels lower contrast than radar
- warnings and tracks distinct but not overpowering
- keep the dBZ legend near the map edge
- optional hybrid/satellite basemap should not reduce radar readability
- 3D controls should appear only in 3D/Volume modes

## 9. Responsive strategy

### Wide desktop
- full two-tier ribbon
- optional left/right docks
- bottom timeline
- maximum map area

### Laptop / tablet landscape
- ribbon groups collapse into menus
- only one side dock open at a time
- timeline remains full width

### Phone / narrow portrait
Do **not** try to reproduce desktop docking.
Use:
- map-first canvas
- compact top bar
- bottom sheet for layers / inspector
- bottom transport
- floating tool button cluster

The same features and state should be shared, but the composition should differ.

## 10. Implementation mapping to the current codebase

Existing code already provides most of the required foundation:

- `crates/hookecho/src/app/chrome/ribbon.rs`
  - current ribbon composition
  - should become the two-tier contextual workstation ribbon

- `crates/hookecho/src/ui/wsv3.rs`
  - current WSV3 primitives
  - should lose glossy stadium styling for this mode and provide compact flat controls, title bars, separators, and window chrome

- `crates/hookecho/src/app/chrome/scrubber.rs`
  - current timeline implementation
  - should gain the workstation-style thin dock variant using the existing shared Timeline state

- `crates/hookecho/src/ui/settings_window.rs`
  - already separates Appearance and has DearImGui-aware tab styling
  - add user-facing options for workstation density and docking defaults only if needed

- `crates/hookecho/src/theme.rs`
  - already has Dear ImGui style hooks and WSV3-specific style hooks
  - combine those at the visual-primitive level instead of creating duplicated application state

- `workspace.rs`
  - natural place to persist dock/window arrangement per workspace

Do not grow `app.rs`; keep UI code in chrome / ui modules, consistent with the existing architecture guidance.

## 11. Recommended implementation phases

### Phase 1 — visual primitives
- flat ImGui-style buttons
- flat tree rows
- compact title bars
- tool-window frame
- separator / divider tokens
- compact combo boxes
- compact checkboxes
- selected row styling

### Phase 2 — top ribbon
- introduce row-1 app tabs
- contextual row-2 controls
- remove oversized group boxes
- preserve current actions/state wiring

### Phase 3 — layers dock
- convert to compact tree
- active counts
- selection highlight
- drag/reorder for active layers

### Phase 4 — inspector dock
- selected layer
- gate/grid probe
- provenance
- 3D contextual fields

### Phase 5 — timeline
- thin bottom dock
- dense transport
- frame ticks / current state
- live-scan progress

### Phase 6 — workspace docking
- left/right/bottom dock state
- floating windows
- persist per workspace

### Phase 7 — responsive behavior
- single-dock mode at medium widths
- bottom-sheet mobile mapping
- touch hit-target overrides without changing desktop density

## 12. Acceptance criteria

The redesign is ready when:

1. The map visibly dominates the application.
2. No permanent sidebar exists merely to fill space.
3. Every major feature in the current first screenshot remains reachable.
4. Radar site, VCP, product, tilt, display mode, and key overlays are operable without opening Settings.
5. Layers and Inspector can be independently docked, floated, collapsed, and closed.
6. The same underlying `PaletteAction`, `UiActions`, and timeline state continue to drive every surface.
7. The WSV3 theme no longer looks like a web dashboard with dark colors.
8. The Dear ImGui influence is structural: compact spacing, docking, tree/list controls, utility windows, and strong state visibility.
9. The interface remains usable at laptop widths without horizontal scrolling across the whole application.
10. Narrow mobile layouts remain map-first and touch-appropriate rather than shrinking desktop UI.
11. Screenshot verification is done at 1920×1080, 1366×768, tablet landscape, and narrow mobile widths.
12. No new UI work substantially grows `app.rs`.

---

## 13. Implementation: the analyst workstation (`Layout::Wsv3` and `Layout::Dock`)

> **Status:** implemented on this branch. The WSV3 layout *is* the workstation now: `Wsv3` and
> `Dock` draw the same chrome (`app/chrome/dock/`, over the component kit in
> `ui/workstation.rs`) and differ only in how they open — `Wsv3` map-first (bars, rail and
> timeline; Layers and Inspector when asked for, the Inspector docking right), `Dock` with its
> windows showing (Layers docked left, the Inspector floating, as in the reference mock E). One
> chrome rather than a restyled ribbon beside a restyled dock is §10's "combine at the
> visual-primitive level instead of duplicating application state". The dense-ribbon sizing that
> only `Wsv3` used is gone; `Layout::CommandRibbon` keeps the ribbon (`ribbon.rs`) unchanged.
>
> **Against §1–§12 above:**
> - Followed: map-first canvas with no permanent sidebars (§2.1, §12.1–2); two-tier top bar (§2.2)
>   with every Radar row-2 control; tool windows that dock left or right, float over the map,
>   fold to their title bar, close, and are remembered per layout (in the settings) and per
>   workspace (§2.3, Phase 6, §12.5); compact Layers tree with counts, Active/Favorites, search,
>   status dots and a 2 px accent edge on selected rows (§3, §6.3); contextual Inspector with gate
>   value, azimuth, range, beam height, sample time, Nyquist and quality notes (range folded,
>   dealiased), provenance (volume file, active provider), a 3D block (pitch, bearing, zoom) and
>   the Product settings / Site info / Pin actions (§4); thin two-row timeline with ticks,
>   live-sweep progress, buffered history and explicit Live/Archive and `F+h` (§5); flat palette,
>   ≤ 4 px radius, 24 px controls, 28 px title bars, monospace for values, times, coordinates and
>   VCP (§6); 36 px single-selection tool rail with Layers and "center on the radar" at its head
>   (§7); the top bars hide for a full-window map (T) with a tab to bring them back; narrow windows
>   shed button words before anything overlaps (§9); the same `PaletteAction`/`UiActions`/timeline
>   state behind every control (§12.6); `app.rs` changed only where the layout is dispatched and
>   the theme applied (§12.12).
> - Different, on purpose: E's per-row gear/`…` are left out until a per-layer settings page
>   exists; the "latency" is the radar feed's health and provider ingest lag, which the app
>   measures, not a network round trip, which it does not; "data probe" is the Inspector's live
>   reading plus the gate inspector on the rail rather than a third control; the rail has no
>   separate pan (the explore tool pans), locate or warning-focus tool, since nothing in the app
>   does those yet, and the 3D camera is the toolbar's 2D/3D/Volume control.
> - Nothing in §1–§12 is outstanding. Every §2.3 window is dockable: Layers, Inspector, 3D
>   view, Sounding, Alerts, Sources, Analyst log, Preferences; phone width is covered below.

### 13.1 References

| | What it is | What we take from it |
|---|---|---|
| **A** | Dear ImGui web demo (ImPlot, ImNodes, gizmos) | Docked title-barred panels; tight, even spacing; a strict grid of small controls; one accent colour doing all the "selected/active" work |
| **B** | erhe (ImGui editor) | Panels as tools: tree with counts, property rows (label left, control right), segmented buttons |
| **C** | HookEcho's previous `Layout::Dock` | The information architecture: Layers tree on the left, tool rail beside the map, inspector, timeline under the map. Kept |
| **D** | WSV3 | A second context row of radar controls (site/product/tilt/smoothing/colour table) directly above the map; checkbox overlays; transport controls |
| **E** | The target mock ("HookEcho — Analyst Workstation") | The overall composition, colour and type. Where A-D disagree, E wins |

### 13.2 What changes, surface by surface

The layout grid, top to bottom:

```
┌ App bar (40) ── logo · workspace tabs · · · panel buttons · clock · radar health ┐
├ Context toolbar (36) ── site · VCP · product · tilt · follow · 2D/3D/Vol · ...   ┤
├ Layers (284) ┬ Rail (44) ┬───────────── map ─────────────────┬ (colour scale) ───┤
│              │           │                   ┌ Inspector ┐   │                   │
│              │           │                   └───────────┘   │                   │
├──────────────┴───────────┴─ Timeline (122) ──────────────────────────────────────┤
```

The timeline spans the full width (it is laid out before the side panels), so the Layers panel and
rail stop above it, as in E.

#### 13.2.1 App bar (replaces the two-row `dock_menu`)

- Left: the app glyph in the accent, **HookEcho** bold, *Analyst Workstation* faint.
- **Workspace tabs:** `Radar · Models · Satellite · Surface · Analysis · GIS`. Each is a view of
  the Layers panel — which registry categories it lists — plus, for Models and Analysis, controls
  drawn above the list:

  | Tab | Layers tree shows | Above the tree |
  |---|---|---|
  | Radar | Radar products, Radar sites | |
  | Models | Forecast models | the model browser (`ui::model_panel`) |
  | Satellite | National weather (satellite, MRMS, national fields) | |
  | Surface | Observations, Severe weather | |
  | Analysis | Tools | the settings of the layers that are on (`layer_options_body`) |
  | GIS | Map reference | (Import/Manage are in the panel footer on every tab) |

  Every registry category belongs to exactly one tab (a test holds this). The selected tab is an
  accent underline, not a filled button (E). Clicking the open tab folds the Layers panel away;
  any other tab opens it on that tab.
- Right: glyph + label buttons — **Inspector** (the floating card) and **Playback** (the timeline)
  toggle panels and light up when open; **Discussion** (AFD), **Tools** (layer manager),
  **Settings**, **Help** open windows.
- Far right: date and time in the pane's time zone, then the radar feed's health from
  `radar_health()` (the same state word and colour the source-health surfaces use: Fresh, Stale,
  Failed, …) and, after a real live arrival, the provider ingest lag (`last_live_arrival`) —
  "Fresh · 38 s lag". E's "Connected 32 ms" is a network round trip the app does not measure; the
  ingest lag is the number it does have, and the one an analyst cares about.
- **The window buttons get their keep-out.** On a borderless window the minimise/maximise/close
  buttons float at the top right; the old dock drew its clock under them. The app bar reserves
  `wsv3::WINDOW_BTN_KEEPOUT` whenever `!os_decorated()`.
- **Narrow windows shed words, not controls** (`app_bar_fit`): below 1780 px the panel buttons
  go icon-only (their names stay as tooltips and accessible names), below 1400 px the subtitle
  goes, below 1240 px the clock drops its date.

#### 13.2.2 Context toolbar (new; absorbs the old right-panel "Quick Controls" and the tilt bar)

One row of compact controls for the active pane, left to right:

`Site ▾` (opens the site dialog) · `VCP` (number, read-only) · `Product ▾` · `Tilt ▾` (every tilt,
the one being swept live marked, *All tilts (four panes)*) · ☑ Follow lowest · `2D | 3D | Volume`
segmented (Volume opens the 3D volume explorer) · ☑ Smoothing · `Color table ▾` (Default + the
product's built-in alternates, written to `Settings::palettes` exactly as Settings does) ·
☑ Range rings · ☑ Warnings · ☑ Storm tracks · ☑ Lightning · `Map` (next basemap style).

E's `⊕ Probe` is not here: the Inspector card already reads the value under the pointer, and the
gate inspector is armed from the rail. On a narrow window the row scrolls horizontally rather than
wrapping (a wrapped toolbar changes height and moves the map).

#### 13.2.3 Layers panel (restyled `dock_left`)

- Panel header: glyph, **Layers**, close.
- Search field with a leading magnifier.
- Segmented **All · Active (n) · Favorites**. *Favorites* is the existing
  `Settings::favorite_layers` (field-layer slugs), not a new list. The first draft also had a
  separate *Filter* toggle; it duplicated *Active* and was dropped.
- **Search, Active and Favorites cross every tab.** "What is on" and "what did I star" do not stop
  at a tab boundary, and a search that cannot find a layer because it sits on another tab would
  send you hunting for it. While any of them is in effect the tab's own controls (model browser,
  layer options) step aside and every category left standing opens.
- The tree: a category header row (chevron, category glyph tinted per category, name, `on/total`
  right-aligned in the accent when anything is on), then layer rows: checkbox (none for a one-shot
  action), the row's own glyph in the category tint, the label (ellipsised), a health glyph when the
  registry reports one (`PaletteEntry::health`, hover for the source and state), and a **star**
  for rows that can be starred (field layers) — shown when starred or under the pointer, so the list
  reads as names rather than a column of hollow stars. Hover highlights the row; the description
  is the tooltip; a row that is on carries a faint accent wash. Feed health now uses distinct
  status shapes with accessible state names as well as color, rather than a color-only dot.
- No gear or `…` per row: E draws them, but a row has no per-layer settings page to open, and a
  button that opens nothing is worse than no button. The star is the one per-row control with a
  real job. The panel footer keeps **Import…** and **Manage…**.

#### 13.2.4 Tool rail (restyled `dock_tools`)

36 px square glyph buttons, 6 px radius, in four groups with hairline separators — looking
(explore, gate inspector), measuring (measure, cross-section, region statistics), the atmosphere
(sounding, point forecast), marking up (marker, draw, watch zone). The armed tool is an accent
fill. The 3D volume explorer sits at the foot (the 3D *map* toggle is the toolbar's segmented
control).

#### 13.2.5 Inspector card (replaces the right side panel)

A floating card over the map's top-right, clear of the colour scale: the map gets the width back,
and a card is what E draws. Header: glyph, **Inspector**, close. Body:

- the product name in the accent and a **Live**/**Archive** badge;
- `Site (id · city, state) · VCP · Tilt · Valid · Age` as key/value rows;
- a reading section: **Value** in the moment's display units and in the colour the table draws it
  (or "range folded" / "below threshold"), `Az / Range` from the radar, beam height (units
  setting), `Lat / Lon`, and the gate's own sample time. It is read from the displayed sweep with
  `BinnedSweep::sample_at`, velocity dealiased by the renderer's own rule, without the gate
  inspector's whole-column work. The section is titled *Under pointer*, *Last pointer reading*
  (the pointer has left the map — typically for the card), or *Pinned*;
- a Model block with lead-step buttons while a model layer is on (the old "Model Forecast" card);
- footer: **Product settings** (opens the Analysis tab), **Site info** (the site dialog), and a
  **pin** that locks the last reading on the card until unpinned.

Readings need a single map pane (the card says so otherwise). The old "Map Information" card's
facts moved to where they already are: the frame count to the timeline, the site to the toolbar
and card.

#### 13.2.6 Timeline (restyled `dock_timeline`, absorbs the tilt bar)

- No title bar: the Playback button in the app bar shows and hides it, and 28 px of header is 28 px
  of map.
- Row 1: `⏮ ◀ ⏯ ▶ ⏭` (play lights while playing) · **Live**/**Archive** pill (click to go live) ·
  the valid date-time (or `Forecast +h`) · `Speed ▾` · right-aligned: the archive day (calendar
  popup, same day-seek path as the desktop scrubber) · **Jump to** a UTC time on the shown day or
  a full date-time (`layers_panel::parse_utc_time`, Enter seeks via `PaletteAction::SeekTime`).
- Row 2, the track: one tick per frame — taller and brighter where the frame is already downloaded
  and decoded, violet for the forecast tail — the playhead as an accent marker, click or drag to
  seek, and an hour label wherever the hour changes (dropped when it would crowd its neighbour).
- Row 3: `Frame n / N` · tilt dots (one per tilt, the displayed one filled in the accent, the one
  being swept live ringed green; click to pick) and the displayed angle · the live status line
  while streaming · right-aligned **Buffer**: a bar and `have/N` of the day's frames that are in
  the decoded-scan cache — the frames the loop can play without a download.

#### 13.2.7 Tool windows: dock, float, fold (Phase 6)

Layers and the Inspector are tool windows. Each header has a `…` menu — **Dock left**, **Dock
right**, **Float over the map** — and, while floating, a fold button (double-clicking the header
folds too, as in Dear ImGui); `×` closes. Docked, a window is a fixed-width side panel laid out
before the map takes its rect, the rail staying against the map; floating, it is a movable window
kept inside the map, and the Inspector scrolls when docked because a column can be shorter than
the card. One host (`dock::tool_window`) draws either, so a window's contents do not know where
they are. Key/value rows ellipsise a long value (the full text is on hover) so a VCP's name or a
volume file cannot widen a card or push a docked column's contents off its edge.

The arrangement — which windows are open, where each sits, whether it is folded, the Layers tab
and the timeline — is a `workspace::WorkstationChrome`. It is saved per layout in
`Settings::workstation`, so each layout comes back as it was left and switching layouts brings the
other one's arrangement, and it rides along in a workspace's `Chrome`, so applying a saved
workspace restores its windows too. A layout with nothing saved starts from its preset
(`DockState::preset`).

#### 13.2.8 Everything reachable from the workstation

The workstation draws none of the floating chrome — no search pill, no control column, no
floating panel — so everything those held has a home here, and the homes are checked:

- **Windows.** Every `AppWindow` sits in exactly one app-bar menu (`dock/menus.rs`): *Tools*
  (Analysis · Data & map · Events & alerts), *Settings*, *Help*, or its own button
  (*Discussion*). `window_home` is an exhaustive match, so a new window does not compile until it
  is placed, and a test checks every window is listed once.
- **Map tools.** Every `MapTool` has a rail button; `rail_group` is exhaustive the same way and a
  test checks the rail against it. Radar suitability, tornado climatology and the chase location
  joined the rail for this.
- **The panel's own pages.** *Map settings* (background, crisp/smooth radar, live sweep
  indicator, launch position, offline maps) and *Preferences* (display and streaming mode,
  location, weather radio, share and image export, backup, help) are the **Preferences** tool
  window, reached from the Settings menu (and Share → "Images, video and more…"). It draws the
  panel's own `map_rows`/`app_rows`, so the layouts cannot offer different settings.
- **Alerts.** The alert list is the **Alerts** tool window (bell button with the count in view),
  the same `ui::alert_panel` list the panel's tab draws; a row flies to the alert and opens it.
- **Actions that meant "the panel".** In a workstation layout, Ctrl+K / the drawer key open the
  Layers window with the keyboard in its search box; the panel toggle toggles Layers; the alerts
  key and the "Active alerts list" row toggle the Alerts window (`workstation_chrome()` in
  `app.rs` routes them).
- **Search.** The Layers search covers layers, sites and tools (it is the registry), answers typed
  time commands (`time 21:30Z`, `at now`) first, and offers **Fly to "…"** for a place name, the
  panel's geocoder (`start_place_search`).
- **Share.** Share menu: copy a link to this view, Open in Windy, export/import GeoJSON, images and
  more, save the layout as a workspace, and open any saved workspace.
- **Toolbar.** Adds storm-relative velocity to the product menu, the VCP's scan-strategy detail on
  click, the legend toggle, a map-style picker (the basemap panel; Z still cycles) and pane count
  and arrangement.

### 13.3 Design tokens

All colours and sizes come from one `Tokens` value (`ui/workstation.rs`), not from constants
scattered through the dock. Dark only — the dock was already fixed-dark — but the accent follows
the Theme's accent with the user's override folded in (`theme::accent`), so the one "your colour"
setting still applies. Category tints for the tree live beside the dock (`category_color`).

| Token | Value | Use |
|---|---|---|
| `bg` | `#0B111A` | app bar, rail |
| `panel` | `#0F1722` | panel bodies, toolbar, cards |
| `panel_hi` | `#141E2C` | panel headers, row hover |
| `field` | `#18222F` | inputs, combo boxes, buttons at rest |
| `field_hi` | `#1F2B3B` | hover |
| `line` | `#233147` | panel borders, separators |
| `line_soft` | `#1A2536` | section rules |
| `text` | `#D8DFEA` | body |
| `text_dim` | `#8A97AB` | labels, secondary |
| `text_faint` | `#5D6A7E` | axis labels, hints |
| `accent` | Theme accent | selection, active tab, playhead |
| `live` | `#3DD68C` | live / healthy / buffered |
| `warn` | `#F2B84B` | archive / stale / a starred layer |
| `danger` | `#F25555` | offline / error |
| radius | 4 (controls, cards, rail buttons) — §6.2's maximum | |
| heights | app bar 40, toolbar 36, control 24, rail button 36, panel header 28 | |
| type | proportional 13 body, 12.5 controls, 11–12 captions; monospace 11–14 for data | |

The old dock set `TextStyle::Monospace` for everything. Now labels and navigation are
proportional and monospace is kept for what §6.2 lists — values, timestamps, coordinates, VCP,
frame counts and the live status line (`ws::mono`; `ws::kv` values are monospace, `ws::kv_text`
is for a value that is words, such as the site's city).

### 13.4 Components (`ui/workstation.rs`)

Small, stateless painters over `egui::Ui`, each taking `&Tokens`, so dock code composes them and
never sets a colour inline. Callers attach names with `a11y::Named` so the painted controls reach
assistive technology:

- `panel_frame`, `card_frame` — the panel body and the floating-card frame (shadow, radius 6).
- `panel_header(ui, t, glyph, title, pinned)` → `HeaderAction` (close / pin).
- `tab(ui, t, label, selected, height)` — the app-bar underline tab.
- `icon_button(ui, t, glyph, label, on)` — app-bar, footer and transport buttons.
- `button(ui, t, label, width)` — a bordered field-style button.
- `rail_button(ui, t, glyph, on)` — the square rail button.
- `segmented(ui, t, &[labels], selected) -> Option<usize>`.
- `check(ui, t, &mut bool, label)` — the compact checkbox.
- `caption`, `kv`, `section_rule`, `badge`, `status_dot`, `divider`.
- `style_scope(ui, t)` — stock egui widgets (text edits, combo boxes and their menus, sliders,
  toggles) in the same look, so the controls the dock embeds rather than paints (model browser,
  layer options, the toolbar's combos) match. The first draft's `toolbar_combo` turned out to be
  unnecessary: a stock `ComboBox` under `style_scope` already is one.

A render test checks that the components draw their labels.

### 13.5 Code structure

```
app/chrome/dock/
  mod.rs        DockState, DockTab, LayerFilter, Probe, group_entries, orchestration,
                and the pure helpers with their tests
  app_bar.rs    §13.2.1 app bar and §13.2.2 context toolbar (the two top bars)
  layers.rs     §13.2.3
  rail.rs       §13.2.4
  inspector.rs  §13.2.5 (floating card; replaces dock_right)
  timeline.rs   §13.2.6 (replaces dock_timeline + dock_tilts)
```

Pure helpers keep their tests (`cursor_readout`, `model_card_rows`, `sweeping_tilt`,
`live_status`); `group_entries` is rewritten with tests for the tab → category mapping, the
cross-tab filters and search. New tested helpers: `app_bar_fit`, `vcp_number`, `table_label`,
`probe_rows`, `fmt_beam`, and the track's `hour_marks`, `slot_x`/`slot_at`.

`app.rs` is untouched: it still calls `dock_layout` before reading the map rect and
`dock_map_overlay` with the other map overlays.

### 13.6 Rules for this work

- **Honest controls.** A control appears only if it does something today. E's per-row gear and
  `…`, its "Data Quality: Good" line, its connection round-trip time and its scale bar are left
  out or replaced by the real number (§13.2.1, §13.2.3, §13.2.5).
- **One source of truth.** Every toggle reads the same flag and fires the same `PaletteAction`
  as the ribbon/palette/phone surfaces. Dock-only state is open/closed panels, the tab, the
  filter, the search text and the inspector's last/pinned reading.
- **Verify by looking.** Each phase ends with a screenshot of the sandboxed app
  (`HOOKECHO_SANDBOX=1`, `HOOKECHO_GOTO` a fixed archive scene — Moore 20:12Z) compared against E,
  not only with tests passing.
- **No regressions elsewhere.** Ribbon, Minimal and phone layouts are untouched.

### 13.7 Phases

1. Tokens + component kit + the dock split into a directory.
2. App bar + context toolbar; tilt bar retired into the toolbar and timeline.
3. Layers panel.
4. Tool rail + inspector card (right panel retired).
5. Timeline.
6. Screenshot pass at full desktop size and 1366 × 768; fix what looks wrong.

All six are done.

### 13.8 Out of scope

A light variant of the workstation look; per-layer settings pages (which the row gear would open);
a scale bar on the map; a network round-trip measurement for the app bar; a natural-language
"Jump to" beyond the time and date-time forms `parse_utc_time` accepts.

### 13.9 Done

- Built as §13.2–§13.5 describe. Changes from the first draft: the Filter toggle dropped (Active does
  it); search/Active/Favorites made cross-tab; a favourite star added per row; E's Probe button
  and the rail's 3D-map button dropped as duplicates; the timeline's title bar dropped; the
  connection line shows feed health and ingest lag instead of a latency the app does not measure;
  the old single-sidebar rule for narrow windows is gone with the right panel, replaced by the
  app bar shedding words (§13.2.1). After merging with §1–§12: radius capped at 4, monospace for
  data, and a 2 px accent edge on selected rows.
- Checked by screenshot of the sandboxed Moore scene at 1936 × 1056 and 1366 × 768 (§12.11's
  1920 × 1080 and 1366 × 768; tablet and phone widths are still to do), with the
  Radar, Models and Analysis tabs, and with a pointer reading on the card (38.3 dBZ drawn in the
  table's yellow, 11.9 mi from KTLX, beam height and sample time filled).
- Second pass ("continue the plan"): `Layout::Wsv3` moved onto this chrome, map-first, and the
  WSV3-only dense ribbon sizing (`theme::is_wsv3_theme`, `wsv3::ribbon_h()`/`colorbar_h()`/
  `status_h()`, the extra status-bar row) was removed; its zoom quick-picks are dropped and its 3D
  pitch/bearing readout moved to the Inspector. Tool windows dock/float/fold and are saved per
  layout and per workspace (§13.2.7); the Inspector gained Nyquist, quality notes, provenance and
  the 3D block; the rail gained Layers and "center on the radar"; the top bars hide with T.
  Checked by screenshot: WSV3 map-first, WSV3 with Layers floating on the Surface tab and the
  Inspector docked right, the Dock with Layers folded, and the Command Ribbon unchanged.
- Third pass ("focus on the Dock, make everything accessible"): the audit in §13.2.8 found the
  Dock could not reach command search, the alert list, the panel's Map settings and Preferences
  pages, place search and time commands, pane count, the legend, SRV, scan-strategy detail,
  sharing and workspaces, and three map tools; each now has a home, the windows and tools by
  exhaustive matches. Tool windows became four (Layers, Inspector, Alerts, Preferences), each a
  `WindowChrome` (open, place, folded) in the saved arrangement. Checked by screenshot with all
  four open, docked and floating.
- The old dock's two `"{2039}"`/`"loading{2026}"` strings were missing their `\u` and rendered
  literally; the model card's is fixed and the arrow buttons are glyphs now.
- The app bar's live delay now follows the newest radar frame against the current clock, including
  idle periods. Its hover detail retains the distinct lag measured at receipt. Archive view is
  labeled as archive rather than treating historical frame age as live latency.
- Layers search accepts Enter: a typed time command runs first, otherwise the first visible
  matching row opens; a query with no row match uses the existing place lookup. Results now update
  in the same frame as typing or changing the All/Active/Favorites filter.
- The Satellite tab now puts the MRMS QPE accumulation-window picker above the National layers
  tree. It switches the active pane among the existing catalog products and keeps saved layer
  identifiers intact; the shared layer options offer the same control when QPE is on.
- A matching compact FLASH ARI picker selects 30-minute, 1/3/6/12/24-hour or cross-window maximum
  rainfall rarity. Its hint and legend say rainfall recurrence in years, not flood probability.
- The Satellite tab also offers a compact 18/30/50/60 dBZ MRMS echo-top selector using the same
  choice pattern. The panel stays map-first; changing thresholds only changes the active pane.
  The shared km MSL legend uses a threshold-neutral title.
- A matching selector switches MRMS reflectivity among five environmental temperature levels.
  It uses the same compact row pattern and preserves the map-first layout and per-product IDs.
- The Rotation tracks options use one compact window dropdown for both 0–2 km and 3–6 km AGL
  tracks. Six published windows fit without widening the dock or crowding the map.
- The radar health and delay readout in the app bar opens the existing Data source health window
  on click, with an accessible action name and a hover hint for the provider detail.
- Per-layer controls (§3.1): in the Active filter every layer that is on gets a remove button in
  its row, and every field layer gets an opacity row beneath it (a flat `ws::fader`, 5–100 %,
  arrow keys step 5 %). The fader writes `settings.field_opacity`, which the grid uniform reads
  per frame, so the map follows the drag with no rebuild; the Layer Manager shows the same value.
  Mode switches that are "on" but are not map layers (pane linking, the alert panel, the mini
  loop) and radar moments get no remove button.
- The borderless window's 44 px drag strip was drawn after the workstation's app bar and sat on
  top of it, taking the clicks meant for the Radar/Models/Satellite/… tabs. The app bar is now
  the caption itself (`window_frame::caption_drag` on a background response allocated before its
  controls), and the strip is skipped in the workstation layouts.
- FLASH ARI layer names lead with the window ("Rainfall rarity, 3 hours (ARI)"), so the seven
  rows stay distinct when the Layers panel truncates them.
- The Inspector's 3D block (§4) now says what the volume is, not only where the camera is: the
  mode (observed sweeps, smooth reflectivity, debris, smooth spectrum width); for observed sweeps
  how many real tilts are drawn and their elevation range, when the scan started and how long
  it took (a volume's tilts share a label, not an instant), and the beam-rise percentage;
  for a smooth volume its quality preset, named from the same `view::QUALITY_PRESETS` the 3D
  controls offer; and the vertical exaggeration when it is above 1×. Checked by screenshot of the
  Moore scene in 3D (17 tilts, 0.5–19.4°, a 4-minute scan); a single "start–end" row did not fit
  the card, so start and span are two rows.
- Tab groups (§2.3): windows docked on the same side share one panel instead of each taking its
  own strip of the map, as Dear ImGui's docking does. The front window's header becomes a strip of
  every window's tab (glyph and title; when the titles do not fit, the others shrink to their
  glyph with the title on hover), with that window's move and close buttons at its end. A window
  that opens or is docked comes to the front of its side; closing or floating the front one hands
  the dock to the next; several arriving at once (a restored arrangement) open on the first in
  tab order. The app bar and rail buttons reflect what is visible: a window behind
  another tab reads as off, and its button brings it forward rather than closing it. The panel
  is as wide as its widest member. Checked by screenshot: Inspector and Alerts docked right
  together, switching by tab and by the app-bar button, and the arrangement surviving a restart.
- The 3D controls (§2.3's "3D volume controls") are a fifth tool window, **3D view**, in the
  workstation layouts: the same body the floating "3D map" window draws in the other layouts
  (`map_3d_controls_body`), in workstation chrome. It is present only while the active pane is in
  3D, reopens each time 3D is entered, and by default docks right as a tab beside the Inspector,
  so it no longer floats over the map's right edge and colour scale. Where it sits is saved with
  the arrangement. Tab glyphs now match each window's own header glyph. Checked by screenshot:
  Inspector, 3D view and Alerts sharing the right dock in the Moore scene.
- Tablet-width pass (§9, §12.11) at 1024 × 768 and the 800 × 600 minimum. Found: the app bar's
  right-hand buttons overlapped the Surface/Analysis/GIS tabs (its fixed width table predicted
  neither side's real width); the map was left about 380 px at 1024 and 130 px at 800 between two
  docks. Fixed: the app bar measures its right-hand cluster each frame (`left_fit`) and gives up,
  in order, the subtitle, the wordmark and then the six tabs, which fold into one "Satellite ▾"
  menu; the wall clock goes below 1120 px (the timeline shows the frame time). Below 1120 px
  (`ONE_DOCK_BELOW`) only one side dock shows at a time, as §9 asks for laptops and tablets — the
  side used last; the other is set aside, not closed, and any of its buttons (rail, app bar, keys)
  swaps back. The saved arrangement is untouched, so widening the window restores both. Checked
  by screenshot at 1024 (right dock, then Layers after the rail button) and 800, and at 1920
  unchanged.
- The context toolbar folds too (§9 "ribbon groups collapse into menus"): the groups after the
  2D/3D/Volume switch fold into one **Display** menu, in order — overlay switches, map style and
  panes, colour table, then smoothing and legend — until the rest fits. Group widths are measured
  as drawn (`ToolbarWidths`), so the fold follows the real contents; the button counts the folded
  overlays that are on ("Display (3)"). Below the fixed part's own width the row still scrolls.
  Checked by screenshot at 1920 (nothing folded), 1536 (overlays and map), and 1024 (all four,
  with a range-ring switch flipped from the menu).
- Diagnostics as a dock tab (§2.3): a sixth tool window, **Sources**, lists every active feed's
  health at dock width, worst first — a status glyph (shape and colour, shared with the Layers
  tree), the source, how old its newest data is, and for a failing feed its last error under it;
  the hover has the family, cadence, cache and recent outcomes. It reads the same rows as the
  seven-column Data source health window (`active_health_rows`), which its footer opens. The app
  bar's feed readout now toggles it (closed by default, docks right), and its title counts the
  feeds that need attention. Checked by screenshot during a network outage: two failing feeds
  first with their errors, then the healthy ones.
- The Analyst log (§2.3) is a seventh tool window in the workstation layouts, present while
  Analyst Mode is on, docked right by default; the floating window remains for the other
  layouts, and both draw `analyst_log_window::body`. Closing it switches Analyst Mode off, as the
  floating window's close does, so the setting and the window agree. Checked by screenshot with a
  live-sweep line in it.
- A tab behind another in a dock can still ask for a look: Alerts shows an amber dot while alerts
  are in view and Sources a red one while a feed needs attention (the front tab's own title
  carries the count). The dot is also said in words — "Alerts, needs a look" — in the tab's
  hover and accessible name, so it is not colour alone. Checked by screenshot, zoomed.
- Docks resize, as ImGui docks do: each side's inner edge is a grip (accent line on hover, resize
  cursor) that drags the dock between 240 px and 560 px, never past 45% of the window;
  double-click restores the width its windows ask for. The panel belongs to the side rather than
  to a window (`dock_side`), so the width holds as windows join, leave or change tabs, and it is
  saved with the arrangement in whole pixels (`WorkstationChrome::dock_widths`). A wider right
  dock also lets its tabs show their titles instead of glyphs. Checked by screenshot: dragged
  wider, kept across a restart, and reset by double-click.
- Every workstation window is a command: Inspector, Alerts, Sources and Preferences have rows
  (`PaletteAction::DockWindow`) in Layers search and Ctrl+K, bindable to a key, wearing their
  header's glyph and showing whether they are visible. Search now leads with a **Best matches**
  group — names that hold the query as written, tightest first — so "window" finds the windows
  rather than "Wind toward/away" through a loose subsequence, and Enter takes the best row.
  Descriptions must hold the query as written too. Checked by screenshot searching "window".
- The point sounding (§2.3) is an eighth tool window, **Sounding**: present once a point has
  been sounded, it arrives in front of the right dock instead of as a 560 px window over the map.
  In a dock the indices, the observed-profile line and the plots stack and scroll as one (the
  Skew-T takes the dock's width, 220–300 px); floating, it is as wide as the standalone window
  with the plots side by side. Closing the tab closes the sounding. The standalone window's
  phone path now scrolls its whole body too, rather than the plots alone under the header.
  Checked by screenshot: a Moore-area HRRR profile with the Norman RAOB, scrolled to the plots.
- Live scan progress in the dock, as the ribbon shows it: while a live chunk stream runs, the
  toolbar gains a chip beside Tilt — "● 1.3°  2/6", the angle being swept and the chunk count,
  with the ribbon's pulsing fill strip under it (`ribbon::live_sweep_strip`); clicking it shows
  that tilt. The timeline's live tilt dot gets the same fill as an arc. The live tilt is now
  found by the sweep's angle (`view::tilt_index_for_angle`) in the ribbon, toolbar and timeline:
  it had been the sweep's VCP position used as an index into the sorted, deduplicated tilt list,
  which marks the wrong tilt once SAILS/MRLE rescan a low tilt mid-volume.
- **Follow sweep**: a Follow control (Off / Lowest / Sweep) replaces the dock's "Follow lowest"
  checkbox; the ribbon gains a "Follow sweep" pill beside "Follow low", and the two are exclusive.
  With Sweep, while following live, the displayed tilt moves to each new sweep once its first
  chunk has merged (`MapView::follow_sweep`), once per sweep, so a tilt picked by hand holds until
  the radar starts the next. Unit-tested (by angle, SAILS cut, waiting for data, off the live
  edge); on screen only the control was checked, because the live feed was degraded (no chunk
  stream) at the time.
- A workspace can open the Sounding tab: `Workspace::sound_center` sounds the point the map was
  centered on when it is applied. The Hail analysis starter uses it (roadmap J5), so REF/ZDR/CC/KDP
  with MESH arrive with the sounding beside them. Checked by screenshot: the preset applied from
  Layers search over the Moore scene, the sounding in front of the right dock.
- Drag reorder (§3.1): the Active filter opens with a **Paint order** list — the pane's field
  layers that are on, top-painted first, each row a drag source (⋮⋮ handle, grab cursor) with an
  accent line where it will land, and a "radar" rule between the bands; a layer cannot be dropped
  across the radar. The order is `Settings::field_order`, applied by `FieldLayer::paint_order`,
  which permutes only the reordered layers among their own `DRAW_ORDER` slots in each band (an
  empty order is the built-in one exactly). Pane draw lists are sorted by it and the renderer
  paints them as given; the top-layer legend follows it too. "Reset" restores the built-in order.
  Checked by screenshot: Rotation tracks dragged above Lightning density and Hail size, and the
  map's legend changed to rotation.
- Non-colour state audit of the workstation (roadmap Q3): every status mark has a second cue
  besides colour; the one that did not, the timeline's forecast-hour ticks, is now split at the
  rail. Checked by screenshot, zoomed on the timeline.
- Search matches word by word, in any order (`layers_panel::word_match`): each word matches the
  name loosely, or the description or the row's keywords as written; a name match ranks first.
  Model-product rows carry the model names and "future"/"simulated" as keywords (analysis rows
  carry RTMA/URMA/observed instead), so "HRRR future" finds "Reflectivity forecast" — before, it
  found nothing. Best matches needs every word in the name, in any order. Checked by screenshot.
- Phone width (§9, §12.10–11): the native phone app has its own chrome, but the web app in a
  phone's browser drew the workstation's docks and bars around a map with no room left. Below
  600 pt (M3's compact class) `workstation_chrome()` is now false and the minimal floating chrome
  draws instead — same state, the map-first composition §9 asks for — and the workstation comes
  back as the window widens, arrangement untouched. Every place that asked "is this the
  workstation" (layout dispatch, workspace capture, the panel actions) now asks
  `workstation_chrome()`. Checked by screenshot at 500 × 800 and back at 1920.
- Selected storm in the Inspector (§4 context body): clicking a SCIT cell selects it; in the
  workstation the Inspector shows a Storm section and is brought forward, and the full attributes
  window opens only from Details…, so the map is not covered. "All panes" is roadmap J2's storm
  link. Checked by screenshot on live KTLX, one pane and two linked panes.
- Fixed while checking it: the frame's label placer asserts that layers reserve in priority
  order, but it was reset once per frame rather than per pane, so a second pane's warning labels
  panicked debug builds whenever warnings were in view. `Placer::next_pane` restarts the order
  check for each pane.
- Status footer (roadmap Q2): an optional 22 px line under the timeline (`dock/footer.rs`), the
  ImGui status bar — pointer position, range/azimuth, beam height and value on the left; pane,
  product, tilt and frame age in the middle; smoothed frame time and zoom on the right, the left
  part cut short before it runs under the others. "Status bar" in search toggles it and the
  arrangement saves it. Checked by screenshot against the Inspector's reading at the same point.
- **Storms** tab (roadmap Q2): the SCIT storm table at dock width, dense and sortable, ranked by the
  Storm attributes window's own severity score, with the selected storm highlighted; a row click
  selects without switching the dock away, a double click also centers. In the workstation,
  "Storm attributes" opens this tab. Checked by screenshot on live KTLX (18 cells).
