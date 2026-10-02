# M1.1 standalone 3D coverage increment

The standalone reflectivity viewer now retains the source revision and temporal policy of its
accepted grid. One worker stays active while the user changes scans or controls; an obsolete
success or failure cannot publish a grid, clocks or layer summaries. Both floating and docked
paint paths suppress unmatched GPU content. Failed/empty inputs stay idle until selection changes
or an explicit retry. The dock checks source identity before painting, independently of the later
worker poll.

`app/standalone_volume.rs` reuses the derived products' weak runtime scan identity and shared
temporal preparation. Keys also include site, accepted revision, selected beams, palette generation
and high-contrast ramp choice. Weak identity distinguishes independently decoded panes without
retaining evicted gate buffers; it is not a persisted scientific identifier. Dimensions remain
fixed. Orbit, value floor, quality steps and clipping remain render controls.

Continuous mode preserves the existing sample grid. Strict mode applies the production 2D
source-time mask before Cartesian interpolation and selected-beam shell construction. Coverage
lists only selected contributors. The available Layers list keeps every source tilt for selection,
but its values and acquisition interval are summarized after applying the policy. Original source
sweeps and bin clocks remain intact. Available tilts do not establish complete columns; missing
clocks do not acquire a volume-label or client-clock substitute. Pass boundaries still use the
existing source-time gap inference.

Reproduce the scientific and ownership controls with:

```sh
cargo test -p hookecho --lib standalone_volume --locked --offline
cargo test -p hookecho --lib volume3d_window --locked --offline
```

The tests use the pinned Mayfield partial scan for runtime identity and progressive-update cases,
plus controlled two-tilt, eight-azimuth inputs for resampling. East-side rows have source time
1,700,000,120,000 ms; west-side rows have time 1,700,000,000,000 ms. Strict mode leaves every
formerly populated west-side voxel transparent at every height in both interpolation and shell
modes; east-side values remain unchanged. Continuous samples match the preceding builder.
Tests also cover equal tilt counts after a live update, independent scan identities, palette and
policy changes, reordered beam selections, stale successes/errors, disconnected workers, bounded
retry, removal of the source and suppression of the GPU callback for an unmatched frame.

## Source-control captures

Eight offscreen captures use the production Source and coverage disclosure in its expanded
state, with the app's fonts and Dear ImGui theme at one pixel per point. They cover continuous,
strict, pending and unknown-clock/error states at 240 px with touch controls and 300 px with
desktop controls. [Capture metadata](volume-ui/captures.json) records hashes, sizes and UTC time.

These are layout references, not pixel goldens or storm reconstructions. Their `DEMO` frame
label is explicitly synthetic. The controlled input has four timed current rows, two older rows,
one empty untimed row and one populated untimed row. Continuous coverage spans 123 seconds;
strict coverage spans three seconds and excludes four rows. The unknown-clock scenario removes
the contributing interval and shows the retry control with retained previous-source details.
The capture helper does not retain adapter/driver identity. These images verify the source
controls; they do not render or certify the full 3D viewport, map, physical device or browser.

```sh
cargo test -p hookecho --lib gpu_volume_coverage_snapshots --locked --offline -- --ignored --nocapture
```

Fresh output is under `target/parity-review/m1.1/volume-ui/`. An explicit invocation requires a
working GPU adapter; a default ignored entry does not certify the capture.

## Remaining scope

Observed, smooth and isosurface map paths still need contributor identity, temporal policy and
coverage. Observed gates must retain their actual radial geometry and source clocks; converting
them to a binned volume to obtain policy metadata would lose their native measurement contract.
Raw cut/pass inventory, SAILS/MRLE and VCP/mid-volume joins, proven transport gaps, finer contributor
invalidation, per-pane local products and platform/soak evidence remain separate work. Palette
changes conservatively rebuild the standalone upload and grid together today; updating only the
transfer table without regridding remains an optimization.

## Verification record

The final Windows workspace run passed **2,074 tests**, zero failures and 121 explicitly ignored
checks across 26 suites. Native Clippy passed. The WASM library check passed in 1 minute 45 seconds
including build-lock wait, with existing browser warnings. The explicit GPU capture passed in
4.14 seconds after compilation; eight image hashes were checked after visual review. Review
caught a closed-disclosure harness before publication; the harness now uses the actual header
response ID, and the narrow-layout test asserts that its body is rendered. Two existing detector
test lint warnings were corrected without changing their assertions.

Final local logs are `target/parity-review/m1.1/volume-workspace-final-verified.log`,
`volume-clippy-final-verified.log`, `volume-wasm-final-verified.log`, and `volume-gpu-final.log`.
Checks exercised the shared tree at `b6726cc`, including concurrent backtest/export changes
preserved outside this increment. The existing Windows bin/lib PDB filename collision remains.
