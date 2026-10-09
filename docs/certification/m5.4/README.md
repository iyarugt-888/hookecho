# M5.4 quality profiles and interpolated frames (1008.md E4, E2)

Settings → Map → Radar appearance has a **Quality** row: Low, Balanced, High and Analysis. Each
profile sets four drawing settings together (`crate::quality::QualityProfile::policy`):

| Profile | Map 3D quality (steps) | 3D window (samples per pixel) | Blending | Wind particles drawn |
| --- | --- | --- | --- | --- |
| Low | 64 | 96 | on | 50 % |
| Balanced | 96 | 160 | on | 75 % |
| High | 128 | 256 | on | 100 % |
| Analysis | 128 | 256 | off: each gate and cell as it is | 100 % |

- Every pane's map 3D takes the profile's quality.
- Wind particles are thinned on the CPU and GPU paths alike: on the GPU every particle still
  advects, and fewer are drawn.
- The row shows "Custom" when a setting was changed by hand.

High is what desktop already used (128 / 256 / blending / every particle), so nothing changes
until a profile is picked. On Android the existing slow-frame guard (`ui::motion::degraded`)
still caps the 3D march at its coarsest whatever the profile.

**Values never change:** a profile changes only those four settings.
`a_profile_changes_how_things_draw_and_never_what_they_are` applies each profile to panes showing
velocity at tilt 3 with a 20 m/s threshold, with a custom reflectivity colour table. It checks:

- each pane's product, tilt, SRV, field layers, thresholds and time (`scene_product`,
  `scene_time`) are identical after each profile;
- `Settings` are identical except for `smooth_radar` and `wind_particle_pct`.

Blending only changes how the GPU samples the texture; the inspector and probes read the gates.

## Captures

Rendered offscreen (RTX 2060) by
`cargo test -p hookecho --lib gpu_quality_row_snapshot -- --ignored`:

| File | What | SHA-256 |
| --- | --- | --- |
| [quality-analysis.png](quality-analysis.png) | The row with Analysis in effect | `677b4c53bf699869209f3cf1f85f838ef553a0450fa145bdd26f4f94769768cd` |
| [quality-custom.png](quality-custom.png) | Settings changed by hand: no profile selected, "Custom" | `9267dc2c1b099265f8efbb8fab7675a46c2c6ca88a3317ce66a4aaa57c557627` |

## Not established

- **Device measurements:** no frame-time comparison between profiles on a device. The adaptive
  reduction is not driven by measured budgets beyond the existing slow-frame guard, and there is
  no Android walkthrough.
- **Other drawing settings:** contour density, field texture detail and label density are not part
  of the profiles yet.
- **Labels:** warning and storm-cell labels already reserve first (`labelplace::Priority::Warning`).
  Dense-map and zoom golden images, and leader lines, are open.
- **Inspector:** it does not yet record the display transformations (blending, decimation).

# Interpolated frames between archived MRMS scans (1008.md E2, increment 1)

**Blend archived MRMS layers to the radar's time** (Settings, under the layer time warning) shows
a continuous MRMS layer at the radar scan's own time. Without it the layer shows the nearest
archived frame, up to the warning threshold away. The blended value is interpolated linearly
between the frames either side (`wxdata::field::blend_frames`,
`wxdata::mrms::fetch_blended_stamped`). It is off by default.

- **Which layers:** eligibility comes from the layer's `ValueKind` in the MRMS catalogue. Only
  scalar and probability values are blended. Accumulations (QPE) and categories (precipitation
  type) are never blended, and neither is a live layer showing the latest frame.
- **When it falls back:** with no frame on one side within the threshold, or frames on different
  grids, the nearest frame is used as before.
- **Missing cells:** a cell missing in either frame is missing in the blend. No value is invented
  where one frame has none, so an echo that appears between scans shows up at the later frame.
- **Labelling:** a blended frame carries `TimeBlend { before, after, weight_after }` in its grid
  provenance and is marked derived. It says so everywhere a value is read:
  - the layer probe's line ("interpolated between 10:18:37 and 10:20:38 UTC, 25% of the way");
  - the data inspector's "Time:" line;
  - the product name in the GeoTIFF/NetCDF grid export;
  - the serialized stamp.
- **Request identity:** blended and nearest-frame requests are separate requests and cache slots,
  so an answer to one is never shown for the other.

## Live check (network), 2026-10-09

[mrms-blend-live.txt](mrms-blend-live.txt) (SHA-256 `e1bf3eab3b94dc65a8c2c569d6d3f59ca10ad502b3ed545875303f0743a5d19e`), from
`cargo test -p wxdata --lib mrms_blend_live -- --ignored --nocapture`:

- **Frames:** two consecutive archived MergedReflectivityQCComposite frames (10:18:37 and 10:20:38
  UTC), blended a quarter of the way between them.
- **Result:** of 24,500,000 cells, 1,444,512 had echo in both frames. Every one equals the linear
  interpolation of the two frames (worst difference 0 dBZ), and every other cell is missing.
- **Network note:** the test's own second download of the first frame failed on a pooled
  connection the bucket had closed while the blend decoded ("connection closed before message
  completed"). The test uses a fresh client for its reference frames. The app's client has the
  same pooling; a failed fetch is retried at the next poll. This is recorded, not changed here.

## Tests

- `a_blend_lies_between_its_frames_and_says_so`
- `categories_other_grids_and_other_products_are_not_blended`
- `a_blend_takes_the_frames_either_side_and_never_for_categories` (key bracketing: exact frame,
  newest end, tolerance, categorical, accumulation)
- `only_archived_continuous_layers_are_blended_and_the_request_says_so`

## Not established (E2)

- No interactive run with the setting on, and no Android run.
- Satellite and observations are not blended. GOES frames are decoded one at a time, and a
  moving mesoscale sector changes the grid. The satellite-only one-minute loop is M5.2
  increment 1.
- No GPU crossfade between frames during playback; blending applies to archived MRMS analyses
  only.
- The rotation and hail track products are maxima over a window, which the catalogue classes as
  scalar; their blend interpolates two windows' maxima.
