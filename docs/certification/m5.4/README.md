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
- Satellite blending is increment 2, below. Observations are not blended. The satellite-only
  one-minute loop is M5.2 increment 1.
- No GPU crossfade between frames during playback; blending applies to archived MRMS analyses
  only.
- The rotation and hail track products are maxima over a window, which the catalogue classes as
  scalar; their blend interpolates two windows' maxima.

# Interpolated satellite frames (1008.md E2, increment 2)

The same setting, now **Blend archived MRMS and satellite layers to the radar's time**, applies to
a GOES band of a fixed sector (CONUS) at an archive time. The band's value at the radar scan's
time is interpolated between the two ABI scans either side, up to 15 minutes from it
(`wxdata::goes_abi::fetch_blended_at`, through the same `blend_frames`).

- **Never blended** (`goes_blend_eligible`):
  - a moving mesoscale box, whose grid changes between scans;
  - an RGB composite or a derived difference or cooling rate;
  - a satellite loop's own scans;
  - live imagery.
- **Labelling:** the blended frame is stamped derived with the same `TimeBlend`, so the probe and
  the inspector say it is interpolated.
- **Request identity:** the GOES request carries the blend, so a blended frame and a nearest-scan
  frame are different requests.

## Live check (network), 2026-10-09

[goes-blend-live.txt](goes-blend-live.txt) (SHA-256 `0a1d23f797908a290c975cb3e787267c5f5160609f611461da8d8f5d6c29a8df`), from
`cargo test -p wxdata --lib goes_blend_live -- --ignored --nocapture`:

- **Input:** two consecutive GOES-East CONUS band 13 scans, starting 18:01:18 and 18:06:18 UTC.
- **Result:** of 134,806 cells with data in both, the blend equals the linear interpolation
  exactly. 67,085 of those cells changed by more than 0.5 K between the scans.

**Two clocks:** the blend is weighted by each scan's decoded valid time, 18:02:37 and 18:07:37,
which follows the scan-start time in its file name. The two scans are chosen by their file-name
times. A radar time between a scan's start and its valid time is therefore refused by the blend,
and the nearest scan is used instead, as before.

## Still not established

- No interactive run with satellite blending on.
- Observations are not blended.
- No GPU crossfade during playback.

# Crossfade between field frames (1008.md E2, increment 3)

A new setting, **Crossfade MRMS and model layers between frames** (off by default), fades a
continuous layer's new frame in over the old one for 250 ms instead of switching at once
(`crate::field_fade`).

- **Visual only:** probes, exports and the stamp read the new frame from its first draw.
- **Eligibility** comes from the layer's `ValueKind`: scalar or probability. Categories (such as
  precipitation type), masks, accumulations, vectors and browsed fields of unknown kind always
  switch at once.
- **How it draws:** the previous texture stays in the MRMS or model cache. The renderer draws it
  under the layer at the layer's opacity, then draws the new frame over it at that opacity times
  its share. A previous texture that has been evicted is skipped, and the new frame draws in full.
- **When it ends:** a fade stops when the layer stops drawing or the setting is turned off.

## GPU control

`headless::field_fade_gpu::gpu_a_new_field_frame_fades_in_over_the_old_one` was run explicitly
(`--include-ignored`) on the NVIDIA GeForce RTX 2060 through the production `prepare_pane`/`draw_pane`
path. Each capture is 160×160; the value is the centre pixel.

| Capture | Centre RGBA | SHA-256 |
| --- | --- | --- |
| [old-frame](old-frame.png) | (255, 0, 0, 255) | `c5746ed6429e01547950d6ba70344eb39dccc160939a49c6c16781d7b39f6fef` |
| [fade-000](fade-000.png) | (255, 0, 0, 255) | `c5746ed6429e01547950d6ba70344eb39dccc160939a49c6c16781d7b39f6fef` |
| [fade-025](fade-025.png) | (224, 0, 137, 255) | `9eab6e851e8f878f775f587ed7756839b5e499706f6337906aa00743190386cc` |
| [fade-050](fade-050.png) | (188, 0, 187, 255) | `7f46d0a09b635461791ce4df245e354e50e83db21a9e8089948af64f9511fd62` |
| [fade-075](fade-075.png) | (137, 0, 224, 255) | `05cdecd0e7b915181f4d8e89822b0fe72cd5776b9acb5b5cf9d25f9fa752ab0f` |
| [new-frame](new-frame.png) | (0, 0, 255, 255) | `ede251ea87dc3b46626ea7b7a097a3939df0675a7ebd799e954a52fb95a968e0` |
| [previous-evicted](previous-evicted.png) | (0, 0, 255, 255) | `ede251ea87dc3b46626ea7b7a097a3939df0675a7ebd799e954a52fb95a968e0` |
| [fade-back-050](fade-back-050.png) | (187, 0, 188, 255) | `c90ef3c0c559209ce239e221c270e4995d7f472036d4af5942ad3e7bafb772d7` |

[evidence.json](evidence.json) SHA-256 `89c29ddb7cf3630e4d254d40770776c93c1b18f5e84cc8c01874fc6883deb182`.

Red fades to blue in linear light, so the halfway mix is 188/187 in sRGB, not 128. Alpha stays
255 throughout, so the map never shows through mid-fade.

## Not established

- No interactive session with the setting on, and no capture of a real MRMS or model sequence.
- No GPU cost measurement: a fading layer is drawn twice for 250 ms.
- Satellite (GOES) layers are drawn by another path and do not fade. Observations do not fade.
