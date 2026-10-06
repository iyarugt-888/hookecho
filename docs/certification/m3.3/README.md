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

## Composable formulas (2026-10-06)

Import [composable-products.json](composable-products.json) in User-defined products. These six
version-1 definitions include a mean, a fraction, the peak's sampled height, a masked layer
bounded by that peak, an interpolated threshold crossing, and a KDP height integral. They use
the existing field renderer, cell probe, trails and scientific exports. Formula text, units and
source coverage continue through those owners; no second grid or rendering pipeline is added.

`mean_vertical` is an arithmetic mean of recorded beam values, with equal weight per beam.
`fraction_vertical` divides true finite conditions by recorded finite conditions, excluding
missing conditions. Neither measures a height-weighted fraction or proves a complete column.
`max_height`/`min_height` return the sampled extremum's height; the lowest height wins a tie.
If a winning or tied value has an unknown height, the returned height is missing.
An optional condition filters means/extrema, and layer functions accept a fourth condition.

`first_crossing_height`/`last_crossing_height` linearly interpolate adjacent recorded values
in either gradient direction, also accepting exact threshold samples. Their names deliberately
differ from the existing `first_height_above`/`last_height_above`, which continue to return
qualifying sampled beam heights. `integral_layer` uses trapezoids clipped to the requested
layer and requires both bounds to be bracketed without missing intervening samples. It returns
input-units × metres. There is no extrapolation; unknown/duplicate heights invalidate the
interpolation profile. Zero-width layers return missing. These rules describe a sampled-beam
approximation, without claiming truth between beams. The KDP example is a **vertical** integral,
not a reconstruction of the radar's radial differential phase.

Returned heights and layer bounds are metres above the radar antenna (ARL), not terrain AGL.
`BEAM_ALTITUDE_M` and environmental isotherms remain MSL. Isotherm comparisons still require
the existing site/time-matched environment; none of these functions fetches a replacement.
Units remain user-declared strings; dimensional typing and true terrain-AGL layer bounds remain
open. Implicit height reads now appear in portable dependency/altitude metadata.

Nested reductions are limited to depth two in import, editor, grid and column probe. The grid
budgets AST visits per argument, multiplying per-beam work at every nesting level while counting
scalar bounds/thresholds once. Arithmetic saturates on overflow. Sorting/geometry costs are
not AST visits. Undefined arithmetic returns missing before it can become a comparison or mask;
finite existing formulas retain their original results.

Durable checks: `udp` unit tests contain algebraic crossing/integral references, masks, empty
and nonfinite columns, height ties, nesting/cost limits and portable round trips.
`cached_column_products_match_independent_columns` additionally checks nested peak-band minima,
means, fractions and peak heights against direct calculations over independently sampled gates
on pinned Denver/Mayfield volumes. `gpu_column_product_renders_where_its_cells_are` now covers
six formulas with production LUT/quantization, cell placement and transparent holes. Actual
run results are recorded separately in ROADMAP_PARITY.md; these instructions alone are not
certification evidence.

## Archived environmental inputs (2026-10-06)

The archived application now reads source HGHT/TEMP directly and adds −30/−40 °C inputs only
where that same selected sounding brackets them. It no longer adds radar elevation to
launch-relative sounding heights. Missing levels remain independent. Source identity includes
the full launch selection, station, geopotential MSL datum, crossing convention and raw-table
SHA256. The live HRRR owner retains its 0C/263K/253K fields and does not estimate the two new
levels. See [recorded-environment rules, pinned tables and independent reference reader](recorded-environment/README.md)
for selection, interpolation, unknown reported timestamps and the remaining geopotential versus
approximate geometric beam-altitude limitation. Tests and run evidence are recorded separately.
