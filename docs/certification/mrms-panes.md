# Pane-owned MRMS analyses

Date: 2026-10-05. M5.1 implementation increment; parent acceptance remains partial.

## Operator behavior

With the existing analysis-time link disabled, MRMS catalog fields follow each pane's own
radar archive cursor. With it enabled, they resolve the retained linked analysis time. Focus
changes do not redirect another pane's MRMS field. A deferred archive seek keeps its requested
UTC instant while its radar listing loads. An empty unlinked archive cursor without a target
waits for a cursor, rather than silently requesting latest.

All existing MRMS catalog products participate, including reflectivity, echo tops, hail,
rotation, lightning, rainfall, precipitation type and FLASH products. Identity includes the
layer, resolved product/window path, full requested UTC time and archive tolerance. Rotation,
hail and lightning window controls keep their existing shared settings. Accepted provider
stamps keep their original source, actual valid time, quality and latency metadata.

Sources and exported diagnostics enumerate visible MRMS requests across all panes and name
their owners. The precipitation-tint setting contributes implicit precipitation-type owners
even when that field is hidden. Expanded rows disclose requested analysis/tolerance separately
from the loaded provider stamp. Errors and missing analyses have their own health history;
no selected-context cache or loaded clock is reported without an accepted grid.

Rendering, numeric probes, grid exports, 3D MRMS surfaces and route exposure read the selected
pane's slot. Route exposure invalidates immediately on a pane/context/generation change. Radar
precipitation tint also carries texture identity and generation in its upload key, so switching
between cached analyses with equal generation numbers replaces the tint grid immediately.
The existing display-grid decimation and precipitation classification are unchanged.

## Request and resource lifecycle

Identical requests share one download, display slot and GPU texture. Distinct analyses or
tolerances have distinct lanes and resource keys. MRMS texture IDs have a separate namespace
from models and legacy fields. A missing specified key cannot borrow a same-layer legacy texture.
Pane presentation uniforms keep opacity separate when two panes share a field.

Delivery must match its requested layer, product, exact request and archive tolerance, and its
grid clock must agree with its stamp. Wanted-context and generation checks precede health credit.
Out-of-tolerance replies become that request's error. Dropping the last subscriber aborts its
transport future and discards its generation without inventing a source failure; the existing
55-second timeout also applies. Any remaining exact subscriber can still admit a completion.

Accepted archives are immutable cache entries and skip periodic refresh. Sources labels their
policy as a retained analysis and shows the next request as occurring on selection change.
Transport age alone cannot mark an accepted retained archive as delayed/stale; its actual valid
time and last-success age remain unchanged and visible. Diagnostics carries `selection_only`
explicitly. Missing/failed archive
requests retry at the existing layer cadence; latest contexts refresh while retaining usable
same-context data on failure. The entry cache normally retains 32 slots on desktop/browser and
12 on Android, grows to protect the entire visible request set, and evicts excess hidden entries.
Hidden entries also expire after 60 seconds. Re-created slots get fresh texture IDs. Palette
changes rebuild each cached context's upload from its own retained grid.

## Verification

The [manifest](mrms-panes/verification.json) records checked source hashes, command results and
capture hashes. Reproducible local logs and PNGs live under `target/parity-review/mrms-panes/`.

Final validation passed 2,302 shared-workspace tests across 29 suites, with zero failures and
151 explicitly ignored tests. Strict workspace/all-target Clippy and WASM compilation passed;
WASM retains its ten existing platform warnings. Four explicit GPU controls passed: the two
new MRMS controls and existing model/raster regressions. The manifest pins 29 owned Rust sources,
28 reviewed PNGs, four Sources diagnostic snapshots and the renderer capture manifest. A
format-only restoration of an existing detection fixture is recorded separately; its only
committed change is the unrelated SourceHealth fixture's new policy default.

Seven new ordinary controls exercise all catalog mappings, exact analysis/window/tolerance
identity, reply rejection, independent/deferred archive clocks, whole-visible-set cache eviction,
expiry and unique IDs, immutable-archive versus latest retry cadence, per-context numeric grids
and precipitation classes, original stamps, cancellation, generation and independent health.

The explicit renderer control uses production `prepare_pane`/`draw_pane` and the radar upload
packer. It checks two analyses, focus/order changes, cached switching/return, shared opacity,
missing/retired keys, surviving/replacement contexts, equal numeric model/MRMS IDs, and different
precipitation tints in two panes. The Sources control renders loaded, fetching, failed and cached
rows with expanded request/loaded/owner details at 240-pixel touch and 400-pixel desktop widths,
asserting layout bounds and retaining diagnostic JSON.

These are deterministic local controller, shader and UI-row controls. They do not certify live
provider availability, actual application interaction, phone hardware or complete time-group
operator acceptance. Claude-owned
calculation work present during workspace checks is recorded separately and excluded from this
increment.

## Remaining acceptance

Independent analysis-time membership and driver selection remain open. GOES decoded fields,
sector footprints and satellite catalogs still have shared ownership and must migrate together
with their clocks. Current-only local radar mosaics and snow-band drivers, comparison/ensemble/
contour ownership, shared MRMS window controls, selected-storm links and a full application
live/archive/multi-group operator session also remain. Tornado detection stays with Claude.
