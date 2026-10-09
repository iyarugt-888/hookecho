# M3.5 colour stops (1008.md C2)

The 3D map's Smooth volumes can replace the palette's colours with two to eight colour stops in
the product's own units, beside the opacity stops. Only the volume's 256-entry colour table is
rebuilt (`render3d::ColorStops::lut`): the accepted grid is re-uploaded unchanged, no volume is
rebuilt, and the sampled values, probe and exports do not change. Data entries keep their alpha,
so what the palette left transparent stays transparent. Velocity (indexed by speed) and the
inverted CC volume do not offer colour stops, and say why. Presets save them as `colors`
(`[value, r, g, b]`); presets saved before have none and keep the palette.

## Reviewed captures

Rendered offscreen on Windows (RTX 2060) by
`cargo test -p hookecho --lib -- --ignored gpu_color_stops_editor_snapshot`, which writes them to
`target/parity-review/m3.5/`.

| File | What | SHA-256 |
| --- | --- | --- |
| [color-stops.png](color-stops.png) | The opacity curve with four colour stops below it (reflectivity, -20 to 80 dBZ) | `026b7e669a039bcab59a756adbaaf6c3721d61e1f61742f1a1d1635e7d088fcf` |
| [color-stops-velocity.png](color-stops-velocity.png) | Where colour stops are not offered, and why | `d0220b4c70885ceee51cc25456abe6f2bee5e629a2f636e436aee9f901846564` |

The opacity curve's summary line ("4 of 8 stops · … clear to solid up") runs past a 360 px panel;
that predates this change.

## Rendering check

`colour_stops_change_colour_not_coverage` raymarches a 50 dBZ block with the palette and with
all-green stops and compares them pixel by pixel:

| Adapter | Drawn pixels (palette / stops) | Coverage decisions that agree | Green pixels (palette / stops) |
| --- | --- | --- | --- |
| NVIDIA GeForce RTX 2060 | 64,836 / 64,836 | 1,000,000 of 1,000,000 | 0 / 64,836 |
| Microsoft Basic Render Driver (`HOOKECHO_GPU_FALLBACK=1`) | 64,781 / 64,781 | 1,000,000 of 1,000,000 | 0 / 64,781 |

## Performance trace, RTX 2060 (2026-10-09)

[raymarch-trace-rtx2060.txt](raymarch-trace-rtx2060.txt) (SHA-256 `7025b7ec27ee2bc72a3207be8982f5681b109d43269dfe96a4fc858a99129917`):
the Moore 2013 reflectivity volume (192 × 192 × 48 voxels, built as the 3D window builds it),
uploaded once and drawn at 1024 × 1024. Each figure is the submit-to-done wall time over 30 frames
after 5 warm-ups (`raymarch_performance_trace`). The step counts cover two controls:

- 96/160/256 are the 3D Reflectivity window's Low/Medium/High.
- 256/384/512 are the map 3D's ceilings for its Low/Medium/High (64/96/128 × 4,
  `pane_upload`). The map's march takes about one sample per voxel up to that ceiling, so these
  are upper bounds for it.

| Mode | 96 | 160 | 256 | 384 | 512 steps |
| --- | --- | --- | --- | --- | --- |
| MIP | 1.35 ms | 2.17 ms | 3.41 ms | 3.23 ms | 4.26 ms |
| Translucent | 0.99 ms | 1.50 ms | 2.29 ms | 3.72 ms | 4.79 ms |
| Lit | 1.02 ms | 1.84 ms | 2.62 ms | 3.85 ms | 5.05 ms |

These are medians; p95 is at most 5.32 ms (lit, 512). MIP at 384 below 256 is run-to-run noise
at this scale.

Translucent rendering stops each ray once it is nearly opaque. Even so, it is not cheaper than
MIP at the high step counts. At under 6 ms a frame at the map's highest ceiling, empty-space
skipping would not be beneficial on this desktop and is not added. Whether a phone needs it is
for an Android trace.

*Correction:* the first version of this section (commit `6066805`) timed only 96/160/256 and
called 256 "the highest quality". That holds for the 3D window only; the map 3D's ceiling is 512.
This run replaces it.

## Not established

- No interactive desktop session editing stops on a real volume, and no Android run or trace.
- Several thresholds at once exist as nested shells (a threshold and two equal steps above it);
  arbitrary unequal thresholds and signed velocity pairs do not.
