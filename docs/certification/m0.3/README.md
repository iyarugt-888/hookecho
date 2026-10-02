# Real radar render baseline

These three small PNGs and `reference-report.json` record the explicit
`headless::corpus::pinned_radar_values_and_missing_sectors_render_consistently`
check on Windows, NVIDIA RTX 2060, Vulkan, driver 572.16. They are reference
captures rather than universal pixel goldens. The JSON records the exact input
and manifest hashes, collection clocks, adapter, image hashes, comparison method,
and passing measurements.

The source baseline is `636738a` plus the visual test harness delivered with
these captures. The shared checkout also included separate detector-backtest
tooling edits; the radar upload, shader, and render entry points were unchanged.
Captured files are copied directly from the successful run, without editing.

| Real partial input | Stable colored samples | Color mismatches | Missing-sector samples | Filled missing pixels |
| --- | ---: | ---: | ---: | ---: |
| [Mayfield](mayfield-2021-partial.png) | 11,664 | 0 | 52,935 | 0 |
| [Denver hail](denver-hail-2017-partial.png) | 4,045 | 0 | 57,002 | 0 |
| [Clear air](clear-air-2019-partial.png) | 374 | 0 | 50,355 | 0 |

The flat map uses nearest reflectivity at zoom 8.5 and 384 × 384 pixels. CPU
inspector samples must retain the same value code and observed/missing state at
the pixel center and offsets of ±0.125 pixels in each axis, within 5–55 km slant
range. This excludes uncertain gate boundaries. Color comparison uses sRGB
alpha blending with a maximum channel difference of eight, allowing at most
0.5% bad sampled colors. Required minima are 100 colored and 1,000 missing
samples per fixture; every checked unobserved pixel must match an empty render.
Replay without another upload produces identical bytes, and rendering preserves
the source values and collection clocks. The actual test passed in 2.92 seconds
after compilation.

Input provenance and attribution are in the
[fixture manifest](../../../crates/wxdata/tests/data/corpus/manifest.json).
The radar files are modified NOAA/Unidata LDM record subsets, with unobserved
coverage preserved. See the [provisioning guide](../../../scripts/corpus/README.md)
for source terms, checksum checks, and reproduction commands. These captures
do not establish full storm detection, complete-column science, 3D rendering,
other products, or device interaction performance.

The new CI step invokes the same semantic check on Mesa lavapipe and uploads
fresh PNGs and JSON. That Linux result remains open until CI runs. Physical
Android and browser runtime certification remain open.
