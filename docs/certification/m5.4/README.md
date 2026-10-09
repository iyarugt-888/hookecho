# M5.4 quality profiles (1008.md E4, increment 1)

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
