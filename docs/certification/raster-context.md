# Timed raster frame identity

Date: 2026-10-05. M5.1 prerequisite; independent analysis-time groups remain partial.

## Behavior

Selecting a different dated satellite or timed WMS frame now selects a different tile cache
namespace. A tile's style, provider revision, exact UTC second and XYZ coordinates travel together
from the download through retry history, CPU residency and GPU upload. A pane's map callback and
retained draw-list key carry the same context. Switching between already-loaded frames updates
the draw list even when the camera and visible tile count stay the same.

Parent and child zoom fallbacks only use the matching frame and provider revision. Missing
matching imagery remains blank until it arrives, rather than borrowing imagery from another
time. Revisiting a resident archived frame reuses it. Time changes retain static map tiles;
shared byte/entry limits still bound the cache and protect the whole visible multi-pane frame.

Explicit frame disk paths now include UTC seconds in an `at-...Z` directory. Legacy minute-only
directories are not reused because they cannot distinguish two requests within one minute.
Static map paths and offline chase packs retain their existing format. Timed latest aliases
are mutable and therefore bypass persistent disk caching, including picker previews. A latest
map context renews its in-memory epoch after five minutes or when returning from an explicit
frame; this is a client refresh policy, not a provider cadence claim.

Provider credentials, custom templates and retina changes advance an opaque in-memory revision,
retire old resident textures and discard obsolete successes/failures. Picker replies also carry
that revision. Credentials and URL text are not part of the renderer key. Old thumbnail workers
continue consuming the four-request budget until they finish, so changing settings cannot reset
the concurrency limit. Timed previews remain decorative latest images rather than archive clocks.

The visibility eviction pass prunes failure history to the whole frame's visible tile keys.
Pending scopes with no visible owner are retired; late completions are ignored. This retires delivery bookkeeping, not the HTTP
transport: existing six-request and deadline bounds remain. Successful resident tiles are not
counted as loading, and a duplicate late failure cannot demote a resident tile.

## Verification

The [manifest](raster-context/verification.json) pins source hashes, command results and GPU
capture hashes. Local logs and PNGs are reproducible under `target/parity-review/raster-context/`.

Final validation passed 2,295 shared-workspace tests across 29 suites, with zero failures and
149 explicitly ignored tests. Strict workspace/all-target Clippy and the WASM library check
passed. Two explicit GPU controls passed in 1.84 seconds: the new raster control and the existing
model-context regression. Eight owned Rust source hashes stayed unchanged throughout the final
checks. Claude-owned calculation sources were present during workspace checks, recorded separately
and excluded from this increment. Source hashes bind checked raw bytes, including line endings.

Nine ordinary controls exercise exact-second URLs/disk paths, static retention, WMS TIME,
cached return, late successes/failures, abandoned scopes, provider changes, obsolete thumbnail
failure rejection, thumbnail budget retention, latest epoch renewal, multi-pane byte eviction
and bounded failure bookkeeping. One explicit GPU control uses production `prepare_pane` and
`draw_pane`, and produces thirteen visually reviewed, full-pane solid-color captures: two exact
frames, late delivery, cached switching/return, provider revision isolation, latest epoch renewal,
foreign pyramid rejection, matching ancestor/child fallback and retirement/survival.

The cached-return GPU case changes only the pane's frame context while its tile cache generation,
camera and visible count remain unchanged, exercising the production draw-list reuse key.

## Remaining M5.1 work

Follow-up: the [MRMS pane increment](mrms-panes.md) now gives catalog fields, precipitation tint,
requests, health, probes/exports and renderer resources per-pane analysis ownership. The remaining
shared-driver scope below applies to GOES and independent analysis-time groups; this earlier
manifest remains the evidence for raster identity itself.

Analysis clocks, satellite catalogs and active driver selection still use the existing shared
controls. Independent analysis-time memberships must change those drivers and MRMS/GOES decoded
field ownership, request lanes, per-pane probe/export/health contexts and render resources together.
Archive no-match/outside-tolerance policy, comparison/ensemble/contour ownership and full
application multi-group live/archive operator evidence remain open. These synthetic controls
verify production cache/render behavior, not live provider availability or physical-device
acceptance. Tornado detection and methodology remain with Claude.
