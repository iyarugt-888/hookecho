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

## Each pane's own column product and environment (2026-10-13, roadmaps/radar-analysis R2)

Until now only the active pane built a column product, into one shared texture, and another
pane drew it only if its selection was exactly the same. The isotherm heights a formula reads
were one most-recent reading too, so two panes on different radars or times needing different
soundings superseded each other's request on the one environment lane, and neither
reliably got its reading.

- **Builds:** every pane showing a column product now builds its own: its own product, volume,
  accepted revision, temporal policy and environment. `ColumnPanes` holds what each selection came
  to (the accepted product or why there is none), each with its own texture, drawn through the
  keyed-texture path the GOES panes use. Two panes on one selection share one build. A selection
  no pane shows lets its texture go, and a selection built again gets a new texture. One build
  runs at a time, the active pane's first, and a reply for a selection no pane wants is dropped.
- **Environment:** readings are filed by `(site, epoch)` (`EnvCache`, 12 kept). A refresh replaces
  only its own pair's reading. While one request is out, another pair waits instead of
  superseding it, for at most a minute or until the lane reports the request finished. The
  diagnostics report's `melting_level` is now the active pane's reading rather than whichever
  arrived last.

Tests: `each_pane_keeps_its_own_column_build` covers two products on the pinned Mayfield volume:
a build and texture each, a shared selection, a failure filed for its own selection, and a
rebuild on a new texture. `readings_are_filed_by_site_and_epoch` covers a live and an archived
pair kept apart, a refresh, the throttle, waiting on the lane, and the bound.

Run, browser build on WebGPU, one machine (2026-10-13, live): a two-pane workspace with KAPX on
"Top REF" = `max_vertical(REF)` and KGRR on "Echo depth" = `count_vertical(REF >= 20)`. The
inactive KGRR pane drew its own product (13 tilts) while the active KAPX pane was still building,
then both showed their own keys (14 and 13 tilts) side by side
([r2-two-panes-two-column-products.jpg](r2-two-panes-two-column-products.jpg), SHA-256
`7ad7d8998de7b8eef9d2e6a70ef80204137aab2bae9dca40b11728500c048ebe`). No panic in the console.

Not established: two archived panes on different RAOB launches in a run (the cache and its
test are), a live HRRR reading next to an archived sounding with missing levels in a run, the
desktop and Android windows, and build cost with many panes.

## A column product's height on the cross-section (2026-10-13, roadmaps/radar-analysis R3)

A scalar column is one number per map cell, not a measured gate or a vertical profile, so only
a column product whose value is itself a height has a place on the cross-section's height axis.
`column_product::height_profile` samples the pane's accepted column product along the cut at
201 points, straight between A and B in longitude and latitude (as the section and the ground
line sample), and converts to the panel's datum: km above the antenna for
`MetresAboveAntenna` results, metres MSL less the antenna's altitude for `MetresMsl` results,
and nothing at all for any other quantity (a dBZ, a count) or for an MSL height with no known
antenna altitude. The section window draws it as a cyan line with a dark casing over the gates,
broken where the column has no value and where the height is below the antenna or above the
panel's top (never pinned to an edge), captioned with the product's name, "column product
height, one value per map cell, not a gate", and the volume time.

Tests: `a_height_product_reads_along_the_cut_above_the_antenna` (pinned Mayfield volume) checks
that every sample equals the map's own cell value at that point for `last_height_above(REF, 18)`,
that `max_vertical(BEAM_ALTITUDE_M, REF >= 18)` (MSL) is shifted by exactly the antenna altitude
and refused without one, and that `max_vertical(REF)` gives no line.
`a_column_height_line_breaks_off_the_panel` checks the panel placement and every break.

Run, browser build on WebGPU, one machine (2026-10-13, archive): KPAH 2021-12-11 03:23:49Z
(Mayfield), "Echo top 18" = `last_height_above(REF, 18)` on the map (14 tilts), a 93 km cut at
72° through the tornadic cell. The line follows the top of the 18 dBZ echo across the cell,
near 12 km over the core and lower on its flanks, with the caption readable over the core
([r3-column-height-on-section.jpg](r3-column-height-on-section.jpg), SHA-256
`d44c03cc18f2aa2beeba0cf464c57d16e07209fe0d31cf735aaffb561134323d`; bundle
`hookecho_bg-fafd014a.wasm`, built from the working tree with peer live-radar edits present).
The first capture of the run had the caption drawn straight over the gates and unreadable over
the core; it now sits on a dark backing.

Not established: the desktop and Android windows, the 3D cut, a pane whose column product and
section come from different volumes, and other environmental fields on the section (none is
added here: only quantities that are heights are drawn).
