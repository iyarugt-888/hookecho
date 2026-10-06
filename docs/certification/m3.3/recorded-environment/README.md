# Recorded-height archived environment

These two UWyo fixed-width tables were copied byte for byte from this workspace's existing
historical RAOB cache. The filenames supply **request/cache launch selections**, not independently
verified reported observation timestamps. The legacy cache retained only the first PRE table;
receipt timestamps and HTML observation metadata are unknown. `reference.json` records those
unknowns as null, the request URLs, SHA256 hashes and independent crossing references. The
directory's `.gitattributes` preserves the pinned table bytes on Windows checkout.

UWyo defines HGHT as **geopotential height in metres** and TEMP as degrees Celsius in its
[column documentation](https://weather.uwyo.edu/upperair/columns.html). These heights already use
the MSL reference. Adding radar terrain elevation to launch-relative hypsometric heights creates
a different datum and is no longer the archived application path. Legacy parcel/backtest sounding
calculations retain their existing implementation; this change introduces a focused recorded-height
reader rather than changing their calibration.

`raob::EnvironmentalLevels` queries 0, −10, −20, −30 and −40 °C on adjacent recorded HGHT/TEMP
samples. Exact-temperature samples qualify; either temperature gradient can bracket a crossing.
Missing HGHT/TEMP and non-increasing adjacent heights prevent interpolation over that pair.
No extrapolation, pressure-height reconstruction, missing-span filling or radar-elevation offset
is applied. Missing dewpoint/wind does not discard an otherwise valid temperature/height pair.
Distinct crossings are sorted. −10/−30/−40 °C use the lowest; 0/−20 °C retain highest cooling
crossings where present, otherwise the lowest recorded crossing. Hail weighting additionally
requires both 0/−20 levels and a positive growth-layer thickness.

The existing bounded selection searches at most two nearby sounding sites within 400 km, at
the requested synoptic launch and then the preceding 12-hour launch. The closest successfully
read partial profile retains individually missing levels; another source never fills its gaps.
The application continues to match the requested radar/site epoch, while the source string names
the actual sounding site, full selected launch, datum, crossing rule and immutable table hash.
The source identity participates in column, trail and gate/3D product invalidation. Column exports
carry that source string through their existing provenance owner; gate/3D metadata is not upgraded
into a complete serialized sounding profile by this increment.

Geopotential HGHT is preserved explicitly. `BEAM_ALTITUDE_M` is an approximate geometric beam
altitude using site/antenna elevation and the existing effective-Earth geometry. Comparing those
values treats geopotential metres as approximate geometric MSL metres; an exact common geodetic
conversion and operand datum typing remain open. The source spatial separation, launch-selection
age, sparse balloon samples and linear interpolation also limit scientific interpretation. The
reader verifies its numerical contract, not the atmosphere between samples or archive completeness.

Live HRRR still supplies its matched 0C/263K/253K analysis fields (263/253 K correspond to
−10.15/−20.15 °C). It supplies no −30/−40 field through this owner, so those new inputs stay missing.
Column map admission names unavailable requested inputs; gate/probe evaluation returns missing.
Portable version-1 files derive the extra requirements and MSL convention from formula text.
ARL layer bounds need an explicit conversion, for example subtracting
`BEAM_ALTITUDE_M - BEAM_HEIGHT_M` from an environmental MSL height. Terrain AGL is not ARL.

Reproduction:

```powershell
python docs/certification/m3.3/recorded-environment/reference.py
cargo test -p wxdata --lib raob::environment -- --nocapture
cargo test -p wxdata --test scientific_corpus cached_column_products_match_independent_columns -- --ignored --nocapture
cargo test -p hookecho --lib gpu_column_product_renders_where_its_cells_are -- --ignored --nocapture
```

The Python reader uses only the standard library and does not call HookEcho. Normal unit tests
compare all ten isotherms and raw identities against its pinned JSON. The cached scientific test
checks a −30/−40 °C masked maximum and an ARL-converted layer mean on the actual Denver radar
volume, with independently sampled gates and Python bounds. The GPU test checks production
quantization/LUT placement/empty cells on partial Mayfield radar data with explicitly supplied
environmental parameters, not an observed Mayfield profile. Run outcomes belong in the roadmap
evidence ledger; reproduction commands are not a certification claim.
