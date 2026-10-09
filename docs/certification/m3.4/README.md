# M3.4 trails

## A real multi-volume trail: Moore 2013 (1008.md C1)

`gpu_moore_trail_capture` (`crates/hookecho/src/headless_corpus.rs`; network + GPU, ignored)
downloads the 14 KTLX volumes of 2013-05-20 19:34–20:29 UTC from the public NEXRAD archive. It
folds their 0.5° sweeps into `extrema::SlidingTrail`, the app's trail, over a 60-minute window
ending at the last volume, and renders through the production renderer on the RTX 2060. Results
([moore-trail.txt](moore-trail.txt)):

- **Merging:** all 14 volumes merged; no beam reset.
- **Coverage:** 14 frames, 0 missing at the trail's cadence. The span is 21 s short of the full
  hour, and is reported as such.
- **Exactness:** all **239,550** gates holding a measurement were checked against a brute-force
  maximum over the same 14 frames. **0 differ.**

| File | What | SHA-256 |
| --- | --- | --- |
| [moore-last-volume-ref.png](moore-last-volume-ref.png) | The last volume alone (20:29 UTC), 0.5° reflectivity | `b7cbc8ddc57c9bdb108320e85e0a5a852a7796da743249812f190021e0a46ece` |
| [moore-trail-max-ref.png](moore-trail-max-ref.png) | The 60-minute maximum reflectivity trail: the Moore storm's swath west of the radar | `a4c60b19d0d7a969bb6161cb398cb72918eed84c13d767dc757b0b1faab6e39c` |
| [moore-trail-min-cc.png](moore-trail-min-cc.png) | The 60-minute minimum correlation coefficient | `571bec486886413bea653a6c9bd5f3203ea02f7fc7bb08bdeb4ecae88a5dbf04` |
| [moore-trail.txt](moore-trail.txt) | Volumes, coverage, the exactness check, render hashes | `065505e17f2a33161a78565bb1c8b930be32073fd6ef2bb124a49f31f704538d` |

**Finding:** the CC-minimum trail is dominated by low-signal gates. Over an hour, the minimum CC
of a clear-air or weak-echo gate is noise, so the capture shows no debris path. A CC-minimum
trail is meaningful only where reflectivity is high enough for CC to be trusted. The app's trail
does not apply such a floor today; this is open, and is not hidden by the capture.

**MRMS:** MESH and AzShear trails are the native MRMS accumulations the app already shows
(hail swath and rotation tracks over the chosen window), not frame-built grid trails. Column user
products have their own trail (increment 3).

Not established:

- Trails across radars and across tilts.
- A reflectivity floor for the CC-minimum trail.
- An interactive scrub of this trail in the app.

## A reflectivity floor for the CC-minimum trail

The finding above is fixed. A minimum-CC trail now counts a gate only where the same tilt's
reflectivity at that gate is at least **20 dBZ** (`extrema::mask_below`,
`CC_MIN_REF_FLOOR_DBZ`). Elsewhere the CC gate is cleared before it reaches the trail, so it can
never win. The two moments are matched gate by gate at the same azimuth and slant range, so
reflectivity's longer, differently spaced gates line up with CC's. The trail's status line says
"only where reflectivity is at least 20 dBZ".

On the same Moore hour, 1,570,527 CC gates were cleared across the 14 frames.
[moore-trail-min-cc-ref20.png](moore-trail-min-cc-ref20.png) (`3334d7981665e8dbd5de0239be66a2eaa579711be225f014bd162e2fe950b2e1`)
keeps the storms only. Inside them, most of the hour's minimum CC stays high, and a coherent
region of low minimum CC lies just west of the radar, along the Moore storm's track. That is
consistent with the tornado's debris signature. It has not been checked against the surveyed
damage track here, and low CC also comes from hail and non-uniform beam filling.

Test: `a_gate_without_enough_echo_behind_it_is_cleared` covers:

- different gate spacing and azimuth bins;
- a folded gate left as it is;
- a gate with no reflectivity behind it cleared;
- a minimum trail over masked frames.
