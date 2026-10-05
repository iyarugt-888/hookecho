# Independent model panes and link groups

Date: 2026-10-05. M5.1 ordinary model increment, partial parent acceptance.
Tornado detection and calculation methodology remain with Claude.

## Operator behavior

Split the map into panes and select each pane before choosing its model, product, run or lead.
The main forecast timeline and Models controls expose **Model/run: Independent / Group N**.
New and legacy panes start in Group 1; newly split panes inherit their source pane's controls
and group. Independent keeps the current selection. Joining an existing group adopts its model,
run and lead. Groups retain compatible pane products and CAPE/SRH variants; an incompatible
product switches to that model's published default. Disabled selected products remain disabled.
Other model layers on a pane retain their explicitly stored engine settings.

There is one playback driver per model group. An independent pane can keep playing when focus
moves. Group playback waits for every enabled selected product's matching source/run/lead;
manual changes pause affected playback. Each pane's model timeline uses its own selection and
native forecast positions. Radar remains accessible from the forecast timeline.

Workspaces record schema-1 model controls and group membership in each pane's forward-compatible
`models` extension. Legacy snapshots inherit the active controls into one all-pane group. Native
steps, supported products, engine identity, run boundaries, bounded numbers and group identifiers
are validated. Unsupported or incomplete controls disable ordinary model fields, display a
restore explanation and retain the raw extension for a future-compatible round trip. Conflicting
saved groups restore independently with a warning, preserving each pane's values.

## Ownership and resource lifetime

`ModelRequest` identifies layer/product, model, pinned-or-latest run, lead and relevant variants.
The 21 ordinary regional/global/RTMA layers resolve requests from their own pane controls.
`ModelFieldCache` shares one field slot and opaque texture identity per identical request; distinct
runs remain distinct even if their valid times happen to coincide. Latest is a refreshable request
alias; each accepted grid retains the provider's actual cycle and valid time.

Acquisition uses a separate generation and health lane per request. A late result is usable only
while at least one pane still wants that exact request and its slot expects it. Receipt verifies
source/product/run/lead and agreement between decoded grid and provenance clocks before commit.
Leaving the last subscriber aborts transport work and retires the generation without a success
or failure outcome. Re-enabling cancelled work can retry immediately. Failures in one request do
not alter another request's history. Same-request refresh failures retain their accepted field.

The cache retains off-screen slots for at most 60 seconds, with a resting capacity of 32 on native
and 12 on Android; capacity temporarily expands to protect the currently wanted distinct requests.
All wanted slots are promoted before eviction. Retirement resets its health lane and frees its
GPU texture; recreated slots receive new IDs. Reflectivity palette changes rebuild cached uploads.
Shared grids still have per-pane presentation uniforms. Missing or retired model keys cannot fall
back to an unrelated same-layer texture.

Pane rendering, probe selection and stamps, forecast banners and clocks, grid exports and
playback readiness resolve the same request-owned slot. One previous key per layer per pane is
retained for dated historical Sources detail while that key remains in the bounded cache. It does
not establish cache residency or freshness for another request. Sources and local diagnostics
list each visible request once, naming all its pane owners; the layer controls retain their
focused-pane health view.

## Verification and retained evidence

The [verification manifest](model-panes/verification.json) records checks, source hashes, capture
hashes, renderer observations and diagnostics fixtures. Reproducible local paths are under
`target/parity-review/model-panes/` (ignored build evidence).

Final checks passed on the recorded source: 2,272 workspace tests across 29 suites, zero failures
and 147 ignored tests; strict workspace/all-target Clippy; and the WASM library check. The four
explicit GPU controls passed in 3.88 seconds on an NVIDIA GeForce RTX 2060. This increment adds
18 ordinary regression controls and two explicit GPU controls. The manifest records 37 Rust
source hashes, 23 visually reviewed PNGs and both Sources diagnostics arrays. Existing WASM
warnings and the workspace PDB filename warning remain recorded in the check logs.

Controls cover group propagation and product variants, join/unlink, pane reorder/removal, exact
cache sharing and run separation, visible-slot protection during growth/shrink, expiry and ID
retirement, cancellation without health credit, source variants and archive request resolution,
workspace round trips, legacy migration, conflicting groups, unsupported metadata, and native
lead validation including overflow. The existing request provenance and playback controls remain.

The explicit renderer control uses production `prepare_pane` and `draw_pane`, preparing all panes
before painting. It checks distinct same-layer request textures, focus/order changes, shared
content with different pane opacity, missing-key rejection despite a legacy same-layer texture,
eviction, surviving contexts and replacement IDs. Its deterministic color fields isolate resource
ownership; they are synthetic controls, not meteorological case validation. Eleven PNG captures
and their measured center colors are retained. Adapter absence is an explicit failure.

Production forecast rows, including the group picker, are captured for regional, sub-hourly,
global and analysis selections at 320 px phone and 1000 px desktop widths. Production Sources
rows capture a loaded shared request beside a separately fetching or failed pinned request at
240/300 px. Corresponding [fetching](model-panes/fetching-diagnostics.json) and
[failed](model-panes/failed-diagnostics.json) diagnostics arrays retain both contexts. Layout
controls assert horizontal and vertical bounds; every generated capture is visually reviewed.

## Remaining parent work

This closes the ordinary model pane/control/cache/texture seam. M5.1 remains partial: independent
camera/site/geographic-cursor and analysis-time groups, MRMS/GOES group drivers, comparison/ensemble
and contour context ownership, and a simultaneous full application live/archive/different-run
operator session remain. The existing global analysis-time link can still influence unpinned
model runs; independent pinned runs are supported. Browser compilation does not certify browser
runtime; physical Android devices and live provider sessions are not claimed by local controls.

The next implementation should introduce explicit analysis-time groups and MRMS/GOES request
and renderer ownership together, retaining native source clocks and unavailable/outside-tolerance
states. Add migration/race controls and record a full application session before accepting the
parent card. Keep detector methodology changes assigned to Claude.
