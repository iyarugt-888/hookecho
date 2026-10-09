# M4.4 GIS export

## Outlines for other moments and chosen thresholds (1008.md D3, 2026-10-09)

"Export map as GeoJSON" outlines the shown sweep of whatever product the active pane draws, at
that product's thresholds: by default 35/50/60 dBZ, 3 dB ZDR, 2 °/km KDP and 0.80 CC, none for
velocity and spectrum width; Settings → General → "Map export outlines" changes any of them (a
chosen velocity threshold is honoured). Each line now says which side it `encloses`: `below` for
CC (debris and non-meteorological echo), `above` for the rest. Only reflectivity reads no echo as
below every threshold; ZDR, KDP and CC have no value without echo, so their outlines stop at the
echo's edge instead of ringing echo-free air. The Mayfield 2021 reflectivity outlines are
unchanged (291 lines, 289 closed).

| File | What | SHA-256 |
| --- | --- | --- |
| [outline-thresholds.png](outline-thresholds.png) | The Settings section with CC changed from its default (RTX 2060) | `7cca7adc29536a95b370de7c1be9111cf09bd927fec9a60370ab02ecb3f38108` |

Tests: `a_low_cc_core_is_ringed_and_echo_free_air_is_not`,
`no_echo_is_below_reflectivity_but_missing_for_dual_pol`, `thresholds_parse_as_a_list_of_numbers`,
the settings round trip with `outline_thresholds`.
