# M1.1 — Show the edge of received radar data

Date: 2026-10-11. Baseline: `c989505ad1704cd78d083311013348ee3c10378b`.
Implementation: implemented; verification: partial. This is the user's live sweep beam fix,
alongside the bounded TGFTP increment under `CODEX_GOAL.md`.

## Before and after

The old marker inferred an angular sector from radial ordinal/chunk positions, then replayed
that sector at a VCP-derived speed after receipt. It sampled a 100 km direction and extended
the resulting screen ray past the pane's farthest corner. Consequently, the line could cross
the entire CONUS map and its motion could lag or disagree with already loaded data.

The map now marks the **newest received radial for the selected native moment**, using its
decoded azimuth bearing, acquisition clock and gate metadata. The endpoint is the far edge of
the last native gate (`first gate centre + (gate count − 0.5) × spacing`), projected along a finite
geographic path in the 2D map's range convention. Different moments can have different endpoints.
The position stays fixed after arrival: a one-second hold and 350 ms opacity fade provide feedback
without predicting where the antenna is now. No angular tail suggests unreceived radials.

`wxdata::live_beam` matches raw arrival `(radial ordinal, source clock)` pairs to the accepted
merged scan, restricting them to the reported cut and actual available moment. A retained older
sector cannot supply the edge. Unknown clocks, absent moments and invalid geometry supply none.
The UI keeps bounded numeric metadata and a weak scan identity, not another copy of gate buffers.
It captures this metadata only on an admitted `DataMsg::Live` update. Rendering requires the same
displayed scan allocation, site and tilt, a live partial volume and an active progress marker.

Markers are suppressed for ended streams, historical playback, 3D, unavailable native moments,
user-defined/column products and foreign display revisions. No saved schema or detector behavior changed.
The existing progress inventories/ribbon remain intact; their estimates are not used to position the beam.

## Reproduction

Open a NEXRAD site at the live head with a native field and let partial chunks arrive. The short
marker should coincide with the most recently received radial and end at that moment's gate range.
Zoom out to a national view: it remains local to the radar's finite range. Pause/scrub or end the
stream: the marker disappears. Switching moment/site/tilt cannot borrow a foreign marker.

```powershell
cargo test -j 1 -p wxdata --lib live_beam::tests
cargo test -j 1 -p hookecho --lib app::live_sweep::tests -- --nocapture
cargo test -j 1 -p hookecho --lib app::live_sweep::tests::capture_finite_received_beam_at_two_map_scales -- --ignored --nocapture
cargo clippy -j 1 --workspace --all-targets -- -D warnings
cargo test -j 1 --workspace
cargo check -j 1 -p hookecho --lib --target wasm32-unknown-unknown
```

## Controls and evidence

- Synthetic native bearings deliberately disagree with the ordinal/chunk estimate: radial 1
  bears 359.75°, while chunk metadata says 0–60°. Reflectivity ends at 250.875 km; velocity's
  newest available radial bears 271.25° and ends at 100.875 km. Reordering arrivals changes neither.
- A newer retained radial outside the raw arriving identity set cannot move the beam. A foreign
  pass clock at the same ordinal, wrong cut, unknown clock, NaN bearing and zero gates produce no edge.
- The pinned NOAA KFTG partial hail fixture exercises capture and display identity. Replacing the
  scan allocation despite equal names/data, changing site/tilt, or ending progress hides the old marker.
- Fade tests preserve the exact bearing/range at every intermediate time and expire the marker.
- The national-view projection asserts that every point is finite and inside the pane, the line
  remains shorter than one tenth of its width, and its endpoint equals the 200 km geographic destination.
- The optional offscreen GPU control paints the finite marker at national and local zooms.
  These are controlled marker captures, not a live GUI/provider session or a live antenna observation.

The NOAA fixture and SHA-256 are the same pinned partial input documented in
[TGFTP recovery](../m1.3/tgftp-recovery.md). No source times or scientific goldens were rewritten.
[Verification JSON](received-beam-verification.json) records commands, source/log hashes and numeric controls.
[National zoom capture](received-beam-conus.png) and [local zoom capture](received-beam-local.png)
were visually reviewed: both show a finite endpoint, correctly scaling the same 200 km range.

Focused tests passed (2 data + 3 UI), strict workspace Clippy passed, the WASM library check
passed with existing warnings, and both new modules passed rustfmt. The already-built app
library executable passed 1,427 tests with 89 ignored; the data library passed 1,059 tests
with 87 ignored. The offscreen GPU control passed separately. Full `cargo test -j 1 --workspace`
failed with **no space on device**; the subsequent library rebuild was interrupted. Directly
running the successfully built focused test executables avoided rebuilding; this does not
certify all workspace integration tests. Earlier global formatting differences were preserved.

Exact implementation paths: `crates/wxdata/src/live_beam.rs`, `crates/wxdata/src/lib.rs`,
`crates/hookecho/src/app/live_sweep.rs`, `crates/hookecho/src/app.rs`,
`crates/hookecho/src/app/data_poll.rs`, `crates/hookecho/src/app/tests.rs`,
`crates/hookecho/src/view.rs`, and `scripts/certification/manifest.py`, plus this evidence,
its two captures/JSON and the M1.1 roadmap ledger. The certification generator now inventories
and hashes evidence and roadmap bytes from its pinned Git commit, excluding concurrent
uncommitted changes and worktree newline conversions. Final implementation SHA is recorded
by the subsequently generated certification manifest; no historical evidence body was rewritten.

## Open gates

Actual antenna telemetry is unavailable here; this is a received-data marker, not a live antenna
position claim. Batch downloads cannot reveal unreceived positions between chunks. Physical Android,
interactive live GUI alignment, operational failure-domain recovery and visible presentation latency
remain unverified. Existing globe/projection behavior and 3D analysis are not certified by this 2D fix.
Global formatting differences and concurrent peer edits are preserved. Only owned paths are committed.

Next ready slice: a whole-app independent-provider recovery harness, including presentation-stage
observation on documented hardware. This bounded beam fix does not close the broader goal.
