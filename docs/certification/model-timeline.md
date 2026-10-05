# Main model forecast timeline

Date: 2026-10-04. Requested operator workflow improvement while continuing WSV3 parity.

## Behavior

Model/product selection activates the active pane's model transport immediately. It does not
require a first interaction with the Models lead slider or a radar frame listing. If the radar
playhead was in its old hourly forecast tail, activation returns that clock to its head and
pauses radar playback. The legacy forecast sync cannot replace the newly selected model or
remove its layer. Model controls use the existing selection, lead and run action paths.

The main timeline offers model and run pickers, native forecast lead scrubbing, previous/next,
first/last and play/pause. A Radar control returns to observations. The selected model's
positions come from the catalogue's existing run-specific ranges: quarter-hour HRRR output,
regional shorter/extended cycles, hourly-to-three-hourly NAM, and the global models' three-
and six-hour steps. Unsupported intermediate forecast times are not manufactured. RTMA
offers analysis-hour navigation and disables forecast playback.

The forecast lead can also be entered directly as hours (for example `1.25` or `1h15m`).
When following latest, a new shorter cycle clamps the existing lead back into its range and
pauses playback before fetching; the display and request cannot retain an unreachable lead.

Model transport activation belongs to each pane; existing model/lead/run engine state remains
shared. The dock and floating chrome draw the same forecast controls. Keyboard stepping and
palette play/pause route to the active transport. Selecting a radar moment, seeking radar
time or going live returns to radar navigation. User scrubs and model/run changes pause model
playback. Radar demo autoplay cannot restart behind the active model transport.

Model playback advances one published position only after a stamp with matching source, lead
and any explicitly selected run has arrived. It resets its deadline while waiting or inactive,
preventing skipped downloads and background catch-up bursts. The UI names requested time
separately from loaded field time; the provider resolves the latest available run, so an exact
latest-run valid time is not guessed before delivery.

## Proof scope

Deterministic controls cover model activation over an old radar forecast tail, mode isolation
between panes, all catalogued model/run position ranges, and source/run/lead playback admission.
Offscreen captures use the production forecast control rows and the application's GPU renderer
and fonts. These are control/layout evidence, not a complete application, browser runtime or
physical phone session. Existing model data-fetch contracts and run availability are unchanged.

Final Windows workspace verification passed **2,218 tests** across 28 suites, with zero
failures and 143 explicitly ignored checks. All six new deterministic controls passed. Strict
workspace/all-target Clippy and WASM compilation passed; existing browser warnings remain.
The explicit GPU control passed in 3.42 seconds and produced eight reviewed references at
320 px phone and 1,000 px desktop widths, including the wrapped loaded-field clock. The
[verification manifest](model-timeline/verification.json) pins twelve source hashes, log hashes,
and capture hashes/dimensions. Local logs and images are under
`target/parity-review/model-timeline/`. These checks use the shared checkout with concurrent
Claude detection changes; this increment excludes those changes.

Reproduce with `cargo test --workspace -- --test-threads=4`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace gpu_model_timeline_snapshots -- --ignored --test-threads=1`.
For WASM use `CARGO_INCREMENTAL=0`, `RUSTFLAGS=--cfg getrandom_backend="wasm_js"` and
`cargo check --target wasm32-unknown-unknown -p hookecho --lib`.

Independent model/run groups, upstream request-result context isolation, unsupported additional
model products, real-provider playback sessions, and physical-device certification remain open.
Tornado detection is outside this increment.


Subsequent context work is recorded in [request-owned model fields](model-context.md).
The ordinary model reply admission and loaded-field gates now use complete request identity;
independent groups and the other remaining certification work above stay open.
