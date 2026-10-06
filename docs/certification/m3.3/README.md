# Column user products — first evidence (M3.3)

These two renders come from the explicit GPU check
`headless::corpus::gpu_column_product_renders_where_its_cells_are`, run on Linux with Mesa
llvmpipe (LLVM 20.1.2, Vulkan, software rasterizer), 384 × 384 at zoom 8.0 centred on KPAH. The
input is the committed real partial Mayfield volume
(`crates/wxdata/tests/data/corpus/mayfield-2021-first-records.ar2`, a modified NOAA/Unidata subset
with its unobserved azimuths preserved). They are reference captures, not pixel goldens.

| Formula | Stable coloured pixels | Colour mismatches | Empty-cell pixels | Filled empty cells |
| --- | ---: | ---: | ---: | ---: |
| [`max_vertical(REF)`](column-composite.png) | 18,016 | 0 | 112,847 | 0 |
| [`min_vertical(CC, REF >= 30)`](column-core-cc.png) | 7,732 | 0 | 131,400 | 0 |

A pixel is checked only when the centre and four ±0.25 px offsets fall in the same grid cell; its
expected colour is the CPU cell value quantized exactly as the app uploads it
(`app::column_upload`) and blended in linear light, channel tolerance 8. A cell with no value
must leave the pixel identical to a render with no field.

Before the user-product layer was excluded from field smoothing, the same check found 10,177
mismatches and 14 filled empty cells: the index blend between neighbouring cells was displaying
values the formula never produced. User column products are now drawn cell by cell, as probed.

Scientific checks behind the same feature (CPU, not visual):

- `wxdata::udp_column` unit tests: `max_vertical(REF)` equals the local composite bit for bit;
  masked minimum, isotherm-relative maximum, height-of-threshold and count products equal an
  independently written column calculation; refused formulas carry a reason.
- `scientific_corpus::cached_column_products_match_independent_columns` on the full pinned
  Denver hail (KFTG 2017-05-08 20:34Z) and Mayfield (KPAH 2021-12-11 03:23Z) volumes, every tilt,
  60 km: composite identical; CC-minimum and ZDR-above-a-stated-height products differ from the
  independent calculation in 0 of 1,104 and 0 of 1,058 checked cells. That check is what found the
  derived-grid extent bug fixed in the same change (524 of 1,104 cells differed before it).
- `scientific_corpus::cached_column_product_full_volume_cost` (release, 4-core cloud VM): a whole
  Mayfield volume, 14 tilts, 875 × 698 cells, 92–137 ms per formula. Not a device measurement.

Not established here: physical desktop GPU or Android rendering, live progressive volumes on a
device, browser runtime, or interaction latency.
