# 3D map resolution rework

Goal: the tilted **3D map** (Observed and Smooth) should show radar data at, or as close as
possible to, the resolution of the full-res Level II scan the 2D view already draws, and it should
be honest about what it draws: no invented data, and any approximation stated in the UI.

## Where it stood (measured on a cached KHTX volume, `KHTX20260910_010232_V06`)

Reflectivity: 12 sweeps, 720 radials x up to 1832 gates at 0.25 km = **8,781,120 gates**.

| Mode | Limit | Effective resolution |
|---|---|---|
| Observed | one GPU quad per gate, capped by `instance_budget` (2M desktop / 800k web / 400k Android). Stride is `ceil(gates / budget)`, and it overrides the user's "Gates" choice | desktop stride 9 (2.25 km footprint), **Android stride 44 (11 km footprint)** |
| Observed "Fill gaps" | copies each gate to a midpoint elevation, so it halves the budget and invents data between tilts | stride doubled, synthetic geometry |
| Smooth | 192 x 192 x 48 grid over a box sized by the farthest *contiguous* echo | **3.67 km cells** on that volume, because 3.7% of the echo (AP / clutter beyond 150 km) stretched the box to 352 km |
| Smooth sampling | `R8Uint` + `textureLoad` (nearest), 64-128 fixed steps across the whole box | cube look, and rays skip whole cells |
| Smooth build | serial `volume3d::build` | 27 ms at 192 cubed, so the grid can be much larger |

## Plan

### 1. Smooth: crop the box to the real echo
* `wxdata::volume3d::echo_extent_km`: the ground range containing 99% of the echo gates (5 km
  bins across all tilts), rounded up to 25 km, floored at 50 km, never beyond
  `max_sample_range_km`.
* Honesty: the UI states the box range, the cell size, and the share of echo that lies outside
  the box. A **Full range** checkbox restores the old "show everything" box.

### 2. Smooth: filtered sampling
* Volume texture becomes `Rg8Unorm`: R = value index, G = valid mask (255 where a real value
  exists). One hardware trilinear fetch returns both, so the shader divides the filtered value by
  the filtered mask. Interpolation therefore only blends *real* voxels; empty space never drags an
  echo edge toward a fake weak value. A voxel is drawn only if more than half of its
  neighbourhood is valid.
* Applies to both the map's Smooth volume and the standalone 3D window (they share
  `raymarch.wgsl`).

### 3. Smooth: scale the grid to the data
* `wxdata::volume3d::plan_grid`: pick `n` (horizontal) and `nz` from the box and a voxel budget,
  aiming for the native 0.25 km gate spacing, clamped to the device's 3D texture limit and to a
  per-platform voxel budget (desktop 40M, web 12M, Android 10M voxels).
* `volume3d::build` is parallel over rows with rayon on native.
* Ray step length follows the cell size (`ctl.z`), capped by `quality_steps * 4`, so a ray takes
  roughly one sample per cell and no longer skips over them.

### 4. Observed: polar cone surfaces instead of per-gate quads
* One instance per **radial** (about 7k for a whole volume), not per gate (8.8M).
* Each radial is a 64-segment strip laid on that tilt's own beam-centre surface. The vertex
  shader is the existing `beam_world` (same geometry, same 4/3-earth model, same vertical
  exaggeration and beam-rise controls).
* Every sweep's raw gate values go up as one layer of an `R8Uint` `texture_2d_array`. The
  fragment shader does a **nearest-gate `textureLoad`** using the radial's row and the gate index
  computed from slant range, so each screen pixel shows exactly one measured gate at the full
  0.25 km x native-azimuth resolution.
* Cost: about 8.8 MB of texture for a whole volume (was 64 MB of instances at 1/9 resolution on
  desktop, 1/44 on Android).
* Honesty changes:
  * **Fill gaps removed.** It fabricated data between tilts. Gaps between tilts are true absence
    of data and are now shown as gaps.
  * **Gates stride / instance budget removed.** Full resolution is always on.
  * Per-radial elevation, azimuth, spacing, first-gate range and gate interval come from the data,
    not from a regular grid.
  * Range-folded and below-threshold gates draw nothing, as before.
* Stated limits, not hidden: surfaces are the beam *centreline* (the beam's vertical width of
  about 1 degree is not drawn); tilts are alpha-blended in draw order rather than depth sorted;
  the 4/3-earth refraction model is a model.

## Progress

- [x] Measure the baseline
- [x] Plan
- [x] 1. Crop the smooth box (`echo_extent_km`, Full range checkbox, UI readout)
- [x] 2. Filtered (masked trilinear) sampling (`Rg8Unorm` + sampler, both raymarch users)
- [x] 3. Scaled grid (`plan_grid`), rayon build, one-sample-per-cell stepping
- [x] 4. Polar radial-strip Observed renderer
- [x] Verify: unit tests, GPU tests, real-volume renders (see Verification)
- [ ] Not yet done: on-device check on Android; growing the standalone 3D window grid; view-following Smooth box

## Log

### Verification (KHTX20260910_010232_V06, release build, real adapter)

| Check | Result |
|---|---|
| Observed upload | 12 sweeps, **6,480 radial instances (207 KB) + 8.8 MB gate texture**, built in 58 ms. Every one of the 8,781,120 gates is a texel; was stride 9 on desktop and 44 on Android |
| Observed render | pitched view draws gate-level structure, cone of silence and a beam-blockage wedge visible |
| Smooth grid | full range 352 km / 192 cubed = 3.67 km cells; cropped box + `plan_grid` = 912 x 912 x 48. Build 63 ms with rayon. Crop is now 99% coverage; on this clear-air volume the far speckle is widespread, so the box stays large (a storm-only volume crops much tighter) and the UI reports the share left out |
| Raymarch pipeline | `Rg8Unorm` + filtering sampler compiles and renders on a real adapter (both the window and the map path share it) |
| Tests | wxdata 540 pass; hookecho GPU tests (`cc_anomaly_fades…`, `retained_observed…`) pass on the new shader; new tests for `echo_extent_km`, `plan_grid`, `build`, `observed_volume` |
| Known unrelated failure | `labelplace::…reserving_out_of_priority_order_is_a_bug` fails only under `--release` (a `should_panic` test that relies on `debug_assert`) |

### Implementation notes

* `wxdata::volume3d`: `echo_extent_km`, `plan_grid`, parallel `build` (rows via rayon, serial on wasm).
* `render3d`: `Volume3dUpload.data` is now interleaved (index, valid) bytes from `pack_rg8`; new
  `outside` field and `cell_km()`. `map_uniform` puts the world-space cell size in `ctl.z`; the
  shader marches `ceil(path / cell)` steps, capped by `quality_steps * 4`.
* The standalone 3D window keeps its fixed 192 x 192 x 48 grid (it shares the shader, so it also
  gets filtered sampling). Growing it is a follow-up.
* `wxdata::level2::observed_volume` replaces `observed_gates`. Removed: `fill_gaps`,
  `gate_stride`, `instance_budget`, the "Gates" and "Fill gaps" controls.
* `radar_observed.wgsl`: instance = radial, 64-segment triangle strip, `textureLoad` on an `R8Uint`
  `texture_2d_array` (one layer per sweep). `OBSERVED_SEGMENTS` (Rust) and `SEGMENTS` (WGSL) must
  agree.
* Widest sweeps above the device texture limit are max-pooled and reported (`ObservedSweep::pool`),
  never dropped.
* Build gotcha: the debug test binary hits a PDB size limit (LNK1318) on this machine; run
  GPU tests with `cargo test --release`.
