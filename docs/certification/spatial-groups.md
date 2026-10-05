# Independent spatial pane groups

Date: 2026-10-05. M5.1 camera/site/cursor increment; parent acceptance remains partial.

## Operator workflow

Open two or more panes, focus one, then expand **Pane links** in workstation Layers or the
floating/phone Layers panel. Camera, Radar site and Geographic cursor each offer Independent
or Group 1 through the platform's pane limit. Each dimension can use a different membership;
these groups are separate from Model/run groups. The existing link toggles switch the focused
pane's selected membership. **Link all panes** / Ctrl+L remains a workspace action: it combines
spatial links into group 1 and enables the existing global time/selected-storm links, or unlinks
them when the entire workspace is already linked.

Joining an existing camera or site group adopts its displayed camera or selected site.
Unlinking keeps the pane's last resolved state. Cameras share center, zoom, pitch and bearing
at runtime; ordinary workspace camera restoration continues to use its saved geographic center
and zoom. Joining a camera group cancels the joining pane's flight so a previous animation cannot
pull it back. Focus changes preserve settled group cameras. A changed camera propagates only
within its enabled group; simultaneous changes give the focused member priority.

Site changes from all existing routes reconcile before per-pane synchronization. Members keep
their own radar products, tilt, model controls and analysis timeline state. The existing pane
site-change handling clears the previous site's volume and rejects deliveries for a different
site. Independent camera and site memberships remain separate: a site change can still recenter
that pane through its normal site-change behavior, and an enabled camera group shares that move.

Hovering a cursor-group member samples only that group at one geographic point. Crosshairs use
each member's own camera; rows use its displayed field or radar context and clock. On a phone
showing one pane, only the visible pane receives the crosshair, while the group readings use a
wrapping, height-bounded scrolling list. This is the current hover-based interaction; physical
touch-device interaction acceptance remains open.

## Workspace compatibility

Each pane's `spatial-links` extension records schema 1 and typed camera/site/cursor memberships.
Group identifiers are bounded by the platform's pane limit, and missing fields or unknown schema
contents are rejected. Unsupported extensions disable the three spatial dimensions while
preserving their raw JSON for a future-compatible round trip. Explicitly choosing a membership
replaces that unsupported extension.

Old workspaces migrate their global flags into group 1. Linked legacy cameras retain the saved
focused pane's camera; radar sites retain their original initial values until a site change,
matching the former site-toggle behavior. Typed conflicting camera/site groups restore just
that dimension independently and disclose the conflict, preserving individual saved values.
Cursor membership and other dimensions remain available. Newly split panes inherit memberships
and unsupported metadata. Reordering/removal carries membership with the pane, and empty group
identifiers can be reused.

## Verification

The [manifest](spatial-groups/verification.json) records checked source hashes, commands, results
and reviewed production-control captures. Local logs and PNGs are reproducible under
`target/parity-review/spatial-groups/` (ignored build evidence).

Final validation passed 2,286 workspace tests across 29 suites, with zero failures and 148
explicitly ignored tests. Strict workspace/all-target Clippy and the WASM library check passed.
The explicit GPU control passed in 2.75 seconds and produced nine visually reviewed captures.
This increment adds ten ordinary regression controls and one explicit GPU control; the manifest
pins eighteen owned Rust source hashes. Checks ran alongside Claude-owned, uncommitted wxdata
work, whose observed hashes are recorded separately; that work is excluded from this increment.
Source hashes record the checked worktree's raw bytes, including its line endings.

Controls cover two simultaneous camera groups including pitch/bearing, focus stability, source
site propagation without product/tilt/clock changes, join/unlink and dimension independence,
hover ownership through pane reorder/removal, pending edits during a cursor join, workspace-wide
Link all status, typed workspace round trips, legacy migration, conflicting saved owners and
unsupported/invalid metadata preservation. Production link pickers and cursor-probe layouts are
captured at desktop/phone widths, with bounds and minimum touch-target assertions.

## Remaining parent work

Analysis time still uses the existing global linked clock. MRMS/GOES fields and satellite tiles
still have shared context ownership; independent analysis-time groups must change those stores,
request lanes and renderer resources together. Global selected-storm linking, comparison/
ensemble/contour contexts, full application multi-group live/archive sessions and physical-device
acceptance remain open. Local controls do not certify live provider behavior or a full application
operator session. Detector methodology changes remain assigned to Claude.
