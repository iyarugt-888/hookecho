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
