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

[raymarch-trace-rtx2060.txt](raymarch-trace-rtx2060.txt) (SHA-256 `3927bf3df9ae7c899790333e7b7f1bc0dafd4921491b080db1ce84eed5585b9e`):
the Moore 2013 reflectivity volume (192 × 192 × 48 voxels, built as the 3D window builds it),
uploaded once and drawn at 1024 × 1024, submit-to-done wall time over 30 frames after 5 warm-ups
(`raymarch_performance_trace`):

| Mode | 96 steps | 160 steps | 256 steps |
| --- | --- | --- | --- |
| MIP | 1.59 ms | 2.42 ms | 3.17 ms |
| Translucent | 1.24 ms | 1.94 ms | 2.73 ms |
| Lit | 1.26 ms | 1.91 ms | 2.72 ms |

(medians; p95 at most 4.34 ms). Translucent is cheaper than MIP because rays stop once nearly
opaque. At under 4 ms a frame even at the highest quality, empty-space skipping would not be
beneficial on this desktop and is not added; whether a phone needs it is for an Android trace.

## Not established

- No interactive desktop session editing stops on a real volume, and no Android run or trace.
- Several thresholds at once exist as nested shells (a threshold and two equal steps above it);
  arbitrary unequal thresholds and signed velocity pairs do not.
