# Progressive provider freshness — 2026-10-14 UTC

The health monitor previously recorded `Update.time`, the source volume start, for
every progressive callback. That clock stays fixed during a multi-minute scan.
The 90-second stale and 180-second degraded rules could therefore classify a feed
receiving current radials as stale and select the completed fallback unnecessarily.

`ProviderHealth::record_observed_update` now uses the newest valid acquisition clock
in the arriving raw `RadialCoverage`, before previous sweeps are stitched in. It
excludes unknown/nonpositive/unrepresentable clocks and invalid radial positions.
When native clocks are unavailable it retains the volume-start fallback. It never
borrows the merged scan's retained clocks or substitutes client receipt time.
Existing monotonic source-clock and recovery-observation rules remain in force.

## Reproduction and controls

The pinned completed NOAA KFTG fixture from `live-fallback` starts at
2026-10-13 **00:22:58Z**. Its final cut ends at **00:26:24.601Z**. Evaluating five
seconds after that native acquisition makes the volume-start clock 211.601 seconds
old. The before-control fails because the monitor retains 00:22:58Z; the corrected
control records 00:26:24.601Z and the actual provider manager keeps Primary selected.
The test uses the file's unchanged native clocks and an explicit historical
evaluation instant; it does not make the archived fixture fresh at today's host clock.

`a_real_scans_late_cut_keeps_the_progressive_provider_fresh` also checks:

- Repeated and out-of-order deliveries do not create additional advancing observations.
- Unknown/invalid raw clocks cannot borrow a fresh clock from the same merged scan:
  the volume-start fallback remains stale and the manager degrades.
- Three batches of the final cut's source radials, with unchanged volume identity,
  establish recovery after three clock advances. Ten manager ticks per batch cannot
  accelerate it; the transition retains the Recovery reason.

The raw coverage is supplied as controlled callback input derived from a completed
scan. This validates extraction and arbitration, not live chunk transport or an
independent upstream deployment. Neither radar values nor detection algorithms change.

## Verification

`verification.json` records the source/input/executable/log hashes and exit codes.
Workspace library tests use debug assertions with PDB emission disabled; the final
focused test strengthens the recovery checks after that library run.

| Check | Result |
| --- | --- |
| Before-control with the original volume-start recording behavior | Exit 101; expected native acquisition differs from recorded volume start |
| Final source-clock and recovery control | Exit 0 |
| `cargo test --workspace --lib` with no-PDB debug profiles | Exit 0; 2,683 passed, 0 failed, 206 ignored across seven library suites |
| Workspace/all-target Clippy, warnings denied | Exit 0 |
| WASM library with configured getrandom backend | Exit 0; existing warnings |
| Isolated real-app delivery/archive/GPU ownership control | Exit 0 |

The library command leaves the running local web-preview executable alone. It does
not rerun the separate integration/doc-test suites recorded by the preceding full
workspace handoff. Build/check activity overlapped. Physical devices, software GPU,
visible presentation, operational outage/failback and full-app long runs remain open.

Next L2 slice: account for displayed-subscription failures independently of a
successful observer subscription, with current generation/site/provider ownership.
