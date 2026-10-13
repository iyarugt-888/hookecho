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

## Two archived panes, two soundings; a busy service no longer substitutes an older one (2026-10-13, R2)

Run, browser build on WebGPU, one machine: a two-pane workspace, each pane in its own time group
on "REF above 0C" = `max_vertical(REF, BEAM_ALTITUDE_M >= FREEZING_LEVEL_M)`. Pane 1: KPAH
2021-12-11 03:23Z (Mayfield). Pane 2: KFTG 2017-05-08 20:34Z (Denver hail). Each showed "waiting
for FREEZING_LEVEL_M … nothing is substituted" until its own reading arrived, then built its own
product (14 tilts each) with an `env` source on its key. The proxied requests were Nashville
(72327) 2021-12-11 00Z and Denver (72469) 2017-05-08 12Z, both answered 200
([r2-two-archived-panes-two-soundings.jpg](r2-two-archived-panes-two-soundings.jpg), SHA-256
`f348646a2db229dbe8478804273a250f1396fb533adfba8b289530e140dfad19`; bundle
`hookecho_bg-db3d5e4c.wasm`, debug `--serve` binary, working tree with peer live-radar edits
present). The key's note is cut at the pane's width, so the launch is read from the requests.

An earlier attempt in the same session moved pane 2 to KPAH 2021-12-11 14:00Z. Its 12Z Nashville
request came back 502 from the local proxy, which had received **503 Too Busy** from UWyo; the
search then took the 00Z ascent from 14 hours earlier and filed it as pane 2's reading, never to
be asked again. Here that happened to be right: UWyo answers a later 400, "Unable to retrieve the
data for 72327 at 2021-12-11 12:00:00", because there was no 12Z ascent. But the same path would
have replaced an ascent that exists with an older one whenever the service was busy.

Fixed:

- `wxdata::raob::search_launches` now serves both launch searches (`environment_levels` and the
  hail `melting_levels`). It moves to the launch before, or the next site, only when
  `launch_absent` says the launch is not there: a `NoSounding` (no table, too few levels, no
  usable isotherm) or a client error other than 408/429. A busy, rate-limited or unreachable
  service ends the search with its error. The pane then keeps saying it is waiting, and asks again
  on the existing 15-minute environment cadence.
- Both proxies (`serve.rs` and `web/_worker.js/proxy-core.js`) pass an upstream 400/404/410
  through as itself and keep 502 for everything else, never forwarding the upstream body, so the
  browser can tell "not held" from "busy". Checked live through the rebuilt local proxy: the 12Z
  Nashville request now returns 400.

Tests: `a_busy_service_ends_the_launch_search_and_a_missing_launch_moves_it_on` (scripted
answers: a 503 stops at the first launch; an absent one moves to 00Z, then the next site; a timeout
after an absence stops). `client_errors_are_absence_and_server_errors_are_not` (real HTTP answers
from a local socket: 400/404 absent; 429/502/503 not).
`an_upstream_not_found_passes_through_and_a_busy_upstream_is_a_bad_gateway` (`serve.rs`, real
socket) and the worker's `an upstream's own not-found passes through; a busy upstream is a 502`.

Run after the fix, same session (bundle `hookecho_bg-20866391.wasm`, debug `--serve`): both
panes on "REF above -30C" = `max_vertical(REF, BEAM_ALTITUDE_M >= MINUS30C_HEIGHT_M)`. Pane 1:
KPAH archived at 2017-05-08 20:33Z. Pane 2: KFTG live. UWyo answered pane 1's Nashville 12Z
request with **503 Too Busy** (server log), the proxy returned 502, and the app made no request
for an older launch or another site. Pane 1 kept "waiting for MINUS30C_HEIGHT_M at this radar
from that day's sounding; nothing is substituted", to be asked again on the environment cadence.
Pane 2 got its live HRRR reading, which has no −30 °C level, and said so: "HRRR analysis
2026-10-13 19:00Z has no MINUS30C_HEIGHT_M; nothing is substituted". Before this change it said
"waiting for MINUS30C_HEIGHT_M … from the live analysis" indefinitely, for a level HRRR never
carries; a balloon that bursts below −40 °C read the same way. The new wording comes from
`missing_levels_status` (test `a_level_the_reading_lacks_is_not_reported_as_a_wait`), and the
note names the reading's source by its first clause (`EnvLevels::short_source`)
([r2-busy-sounding-and-hrrr-missing-level.jpg](r2-busy-sounding-and-hrrr-missing-level.jpg),
SHA-256 `16efe469f4574594070da26f8b7a400edeb14010564bf3d74097476be8205c28`; the note is cut at the
pane's width).

Not established: the retry after a busy answer actually landing in a run, the deployed worker,
and the desktop and Android windows.

## The column height in the 3D cut (2026-10-13, roadmaps/radar-analysis R3)

With the section's "3D cut" on, the 3D view of the pane the section samples now draws the same
column-product height along the cut, in the panel's cyan with a dark casing, broken where the
column has no value (`pane_3d_overlays::paint_3d_column_height`). Its points are the panel's own
samples, put on sea level by the antenna's altitude (`column_product::height_line_msl`, test
`the_3d_cut_line_follows_the_cut_at_msl`). That is the smooth volume's own frame: its box runs from
the antenna's altitude up by the volume's height, times the vertical exaggeration
(`render3d::map_uniform`), and `render3d::lonlat_alt_screen` places an MSL height the same way.
The metres-to-pixels scale is taken at the point's latitude rather than the radar's, under 1%
across the box. Only over a resampled volume, which is what the cut slices: the observed sweeps place
heights by beam rise and exaggerate only the earth's curvature, so an MSL line would not sit on
them. Not seen in a run: the browser pane was hidden for this session's last runs.
