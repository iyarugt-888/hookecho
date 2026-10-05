# Request-owned model fields

Date: 2026-10-04. M5.1 context increment, partial. Tornado detection remains with Claude.

## Behavior and implementation

The ordinary 21-layer model/analysis scheduler uses `ModelRequest`, which identifies the
source model, target layer/product, run selection (pinned or latest), lead, and CAPE/SRH
variants. Selection changes bypass refresh cadence, clear pending uploads and start a fresh
source-health history. Generation counters remain monotonic across those resets. Reflectivity
keeps its previous ten-minute fetch cadence; health now reports that same cadence.

`OverlayDelivery::Fetched` carries the request identity for successes and errors. The receiver
checks it against both current selection and scheduled field state before health accounting.
An obsolete reply is discarded without a success/failure outcome. A current reply must match
its source, product, run and lead and carry the same decoded grid/stamp valid time. Invalid
current metadata becomes a failed request. Regional reflectivity is stamped from its original
fetch model; quarter-hour output identifies HRRR 15-min. RTMA explicitly permits the provider's
RTMA or URMA analysis label for the requested analysis hour and product.

Fields commit their grid, stamp, pending upload and accepted context together. Display
readiness requires that exact selected context. Drawing, probe values and inspector stamps,
grid exports, model clocks in the timeline/ribbon/forecast key, and playback admission use
that gate. A switch back to a previous selection requires a matching newly staged upload;
a discarded upload cannot resurrect as a supposedly resident texture.

One previous grid/stamp stays bounded in the existing field slot and is dated as a
“Previous model field” in Sources while the selected context is unavailable. It does not
establish cache residency, valid time, request outcomes or health for a different selection.
Same-selection refresh failures still retain a usable current field. Latest selection accepts
the provider's actual run without rewriting its clocks; Sources uses the resident field's
valid time even if the provider falls back to an older available cycle. Sources names the
requested model/product, and recovery wording distinguishes resident data from an unavailable
request instead of promising a cached map value when none is usable.

Reflectivity now shares stamped-field display preparation with the other model layers,
including existing display resolution limits and grid provenance. Scientific quality remains
unknown where the provider does not report it; delivery time is not substituted for valid time.

## Proof scope

Nine deterministic controls cover original reflectivity source, exact source/product/run/lead
and decoded clock admission, latest versus pinned identity, preserved provider timestamps and
unknown quality, upload invalidation on selection change and switch back, cadence bypass for
CAPE/SRH variants, preserving a hidden layer's queued upload until its own context consumes it,
dispatch round trips, global lead arithmetic, RTMA/URMA products and pinned
hours, obsolete success/failure accounting, separate new-selection health, same-context cached
failure, and resident-clock reporting after older-cycle fallback.

The GPU control feeds four request-book/field-admission fixtures into the production Sources
row: waiting before replacement fetch, fetching, failed with a historical field, and loaded.
It writes eight narrow-width captures and four JSON snapshots using the production diagnostics
serializer. This is production-row layout and state evidence; it is not a full application,
real-provider, multi-group, browser runtime or physical-device session.

Final Windows workspace verification passed **2,237 tests** across 28 suites, with zero
failures and 145 explicitly ignored checks. All nine new controls passed. Strict workspace
Clippy and WASM compilation passed; the browser build retains its existing one wxdata and
ten hookecho warnings. The explicit GPU control passed in 4.56 seconds. All eight 240/300 px
Sources captures were reviewed for wrapping, selected versus previous clocks, current health,
cache disclosure and recovery wording.

The [verification manifest](model-context/verification.json) retains twenty checked Windows
source-file hashes, check/log hashes, nine control names and capture hashes/dimensions. Four
production diagnostics snapshots are committed beside it. Local logs are under
`target/parity-review/model-context/final/`, with images under `model-context/ui/`. The source
guard confirmed all twenty files remained unchanged through final verification. Concurrent
Claude backtest commits are preserved and excluded from this increment.

## Reproduction and remaining work

Run `cargo test --workspace -- --test-threads=4`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace gpu_model_context_sources_snapshots -- --ignored --test-threads=1`.
For WASM set `CARGO_INCREMENTAL=0`, `RUSTFLAGS=--cfg getrandom_backend="wasm_js"` and run
`cargo check --target wasm32-unknown-unknown -p hookecho --lib`.

The global model controls and one field slot/texture per layer still exist. Independent pane
and link-group selection, multiple resident source contexts, immutable content cache/resource
keys, comparison/ensemble/contour contexts, and group-specific MRMS/GOES drivers remain open.
Implement group state and texture ownership together before claiming different simultaneous
model runs. M5.1 stays partial. Detection calculations and concurrent backtest files are excluded.

## Subsequent pane ownership increment

The next M5.1 increment moves ordinary model controls into each pane and caches complete model
requests separately. Its previous-data references, Sources enumeration, cancellation and texture
ownership supersede the single global field-slot implementation described above. See
[model panes and verification](model-panes.md); earlier manifests remain evidence for their
pinned commits, not proof of the later implementation.
