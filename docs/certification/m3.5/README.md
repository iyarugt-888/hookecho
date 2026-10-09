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

## Not established

- No interactive desktop session editing stops on a real volume, and no Android run.
- Several isosurface thresholds at once, empty-space skipping and a performance trace remain open
  (1008.md C2).
