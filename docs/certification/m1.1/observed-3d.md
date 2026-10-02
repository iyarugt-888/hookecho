# M1.1 native observed 3D coverage increment

The observed map path keeps each recorded moment radial's geometry and source acquisition
clock. Coverage retains the original decoded sweep index and elevation number, including
repeated elevations and omitted moment cuts. The Inspector displays one-based cut ordinals;
these identify positions within the selected scan, not persistent pass IDs.

Continuous extraction retains the preceding gate encoding and all native moment cuts.
Strict-current follows 2D's deduplicated tilt list and newest moment-carrying cut selector.
A cut selected at any displayed tilt remains eligible, including nearby angles within the
selector's tolerance. Another timed selected cut can exclude a repeated cut. With no timed
choice, untimed cuts remain unknown rather than being assigned to a guessed pass.
The existing source-time gap rule masks older and untimed rows at a detected boundary.
It operates on recorded radial clocks, so occluded native rows can classify differently from
regular bins. Native geometry is never obtained by binning or angular interpolation.

Masked gate rows become transparent before normalization and maximum pooling at the GPU
texture limit. Their original radial geometry and clocks remain in the extraction. Layer
values and acquisition intervals use retained rows only; a fully excluded cut has no retained
clock or maximum. Missing data and range folds keep their original encoding on retained rows.
An older upper cut remains available when no newer moment cut exists at that height.
Available cuts and recorded-radial counts do not prove absent sectors or complete columns.

The observed upload key now uses the actual scan allocation through a weak identity, accepted
volume revision, site, name, moment, temporal policy, palette and render controls. Weak identity
prevents collisions between independently decoded scans without retaining their gate buffers.
Camera motion and the pane's live counter do not invalidate native gates. The synchronous
extraction publishes coverage, layers and upload identity together. Inspector and Layers controls
validate source identity and policy before painting their retained summaries. The existing
native extraction/upload is still synchronous; this increment does not certify its frame cost.

## Reproduction and controls

```sh
cargo test -p wxdata --lib observed_policy --locked --offline
cargo test -p hookecho --lib observed_keys --locked --offline
cargo test -p hookecho --lib observed_source --locked --offline
cargo test -p hookecho --lib gpu_observed_coverage_snapshots --locked --offline -- --ignored --nocapture
```

Six scientific controls cover default/continuous gate equality, geometry and clock preservation,
strict masking before pooling, excluded maxima, older upper cuts, split moment cuts, reversed
cut order, nearby tilt selection, unknown clocks, moment-specific source intervals and the pinned
Mayfield partial scan. That partial input proves decoding/clock retention, not a whole storm's
spatial reconstruction. Runtime identity controls cover independent same-name/revision scans,
updates at an unchanged tilt count, source removal, site/moment/policy/palette/render changes,
and camera/pane-counter independence. UI checks cover pending inputs, unknown clocks, native
cut ordinals, incomplete inventory qualifications and 240/300 px wrapping.

## Inspector captures

Eight offscreen references use the production coverage painter, application fonts and Dear
ImGui theme at one pixel per point. Continuous, strict, pending and unknown-clock scenarios
appear at 240 px with touch controls and 300 px with desktop controls. The controlled coverage
has two recorded cuts at 0.5 degrees, eight total radials, three timed older radials and one
unknown clock. Strict mode retains four radials across a three-second known interval; continuous
mode retains eight across 123 seconds. The unknown-clock scenario has eight unknown clocks
and no invented acquisition interval. Cut 4 demonstrates that indices include omitted cuts.

Fresh captures appear under `target/parity-review/m1.1/observed-ui/`. Durable copies and hashes
are listed in [capture metadata](observed-ui/captures.json). These are layout references, not
pixel goldens or full map captures. The helper does not retain adapter/driver identity. It does
not render the full observed viewport or certify Android, browser, physical-device or soak
behavior. Explicit invocation requires a GPU adapter; an ignored default entry is not evidence
of a render pass.

## Remaining scope

M1.1 remains in progress. Smooth and isosurface map workers/cache keys still need source revision,
policy and contributor coverage, including product-dependent input moments and playback frames.
Raw pass identity and explicit live inventory must replace or qualify time-gap inference for
SAILS/MRLE, VCP changes and mid-volume joins. Proven transport gaps, finer contributor invalidation,
performance, independent pane contexts, full viewport captures and device/soak gates remain open.

## Verification record

Final Windows workspace checks passed **2,087 tests**, zero failures and 123 explicitly ignored
checks across 26 suites. Native Clippy passed with warnings denied. The WASM library check passed
in 3 minutes 44 seconds including build-lock wait, with the existing wxdata/browser warnings.
The explicit capture passed in 3.64 seconds after compilation; all eight copies passed independent
PowerShell SHA-256 checks after visual review. Review corrected nearby-angle cut selection to
use 2D's deduplicated tilt list, now protected by a regression control. An initial layout test's
unnecessary total-height assumption was removed; wrapping and the actual captures provide the
layout checks. The existing Windows bin/lib PDB filename collision remains.

Final local logs are `target/parity-review/m1.1/observed-workspace-final.log`,
`observed-clippy-final.log`, `observed-wasm-final.log` and `observed-gpu.log`.
These checks exercised the shared tree at `d300df9`, including concurrent LLSD, detector settings
and scientific-corpus edits preserved outside this increment. Historical earlier capture/test
counts in sibling evidence pages retain their original meaning.
