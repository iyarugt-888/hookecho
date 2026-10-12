# M3.1 per-sector Nyquist velocity (1010.md, ROADMAP_PARITY M3.1)

Some volume coverage patterns send a tilt at different PRFs by azimuth sector, so its radials
carry different Nyquist velocities. The cached sectorized volume, vilonia-2014 (KLZK
2014-04-28 00:34 UTC), sends its nine lowest tilts at 22.24 m/s over about 210° to 30° and at
26.3 m/s over the rest. The dealiaser took one Nyquist per sweep, so these tilts fell back to
the largest raw |v| (26.5 m/s), labelled "estimated from the largest |v| (the decoded value
varies by sector)", and unfolded the narrow sector at the wrong interval.

Each row is now unfolded at its own decoded value (`dealias::dealias_rows`). In units of its
row's Nyquist every gate folds at ±1, and the region dealiaser measures everything in units of
the interval, so the field is scaled row by row, unfolded at 1 and scaled back. A velocity that
is continuous across a sector edge changes there by the ratio of the two values in these units
(0.85 here), a small part of the interval of 2, so the boundary votes across the edge still come
out whole. It is used only when every row with velocity carries a decoded value and no row's raw
|v| sits past its own by more than a quantization step (`dealias::sector_nyquist`); otherwise
the sweep falls back to the estimate as before. The sweep's source reads "decoded per sector".
Synthetic: `a_sectorized_sweep_unfolds_row_by_row` (a uniform 30 m/s wind folded at 22.24 and
26.3 m/s by half: recovered exactly row by row, not at one interval).

## Backtest (cached corpus, release, one machine)

`cached_sectorized_tilts_unfold_row_by_row` counts neighbouring gates more than a fold apart
after unfolding, each pair judged at the smaller of its two rows' values, on the nine sectorized
tilts, against the whole sweep unfolded at the estimate
([sector-nyquist-backtest.txt](sector-nyquist-backtest.txt)):

| Tilt | Estimate | Row by row |
| --- | --- | --- |
| 0.5° | 893 | 512 |
| 0.9° | 562 | 219 |
| 1.4° | 432 | 155 |
| 1.8° | 395 | 110 |
| 2.4° | 347 | 118 |
| 3.1° | 270 | 61 |
| 4.0° | 298 | 82 |
| 5.1° | 317 | 110 |
| 6.4° | 378 | 238 |
| **All nine** | **3,892** | **1,605** |

Every tilt has fewer, 59% fewer in all. 60 to 796 gates a tilt change by more than 10 m/s. Over
the whole corpus (`cached_decoded_nyquist_unfolds_no_worse_than_the_estimate`, 159 velocity
tilts of 12 volumes) every tilt now unfolds at decoded values, 150 at one and 9 per sector, and
fold-sized jumps at the decoded interval total 23,932, down from 25,114.

The cached detector suites pass unchanged in release: golden events 9, scientific corpus 23,
detection regressions 15, clutter corpus 3.

| File | SHA-256 |
| --- | --- |
| sector-nyquist-backtest.txt | `6f59de3136077dc1fc120bea9ea5fa30b050f0df38495e0e28c36ea732075a61` |

Not established: a reference dealiaser on this volume (Py-ART declines it, and is not installed
here), another sectorized case, and the truth-free VAD check, which is in the Python scorer.
The jump count is truth-free too, and blind to a whole sector unfolded a constant fold off.
