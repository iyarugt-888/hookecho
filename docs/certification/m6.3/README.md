# M6.3 repeat-run determinism (1008.md F2)

The same archived loop was rendered twice through the off-screen renderer `--watch` uses, on
Windows with the RTX 2060 (debug build, 2026-10-09):

```bash
hookecho --watch --site KTLX --from 2013-05-20T19:50 --to 2013-05-20T20:20 --basemap none --size 512 --interval fixed --fps 4 --out moore-a.gif
```

Then the same command again, writing `moore-b.gif`.

| | Run A | Run B |
| --- | --- | --- |
| Exit, time | 0, 46 s | 0, 43 s |
| GIF SHA-256 | `333532acf125ff3c901f85942950498f210b332b0b4a404277a5b15081a2cf04` | `333532acf125ff3c901f85942950498f210b332b0b4a404277a5b15081a2cf04` (identical) |
| Sidecar | [moore-a.json](moore-a.json) | [moore-b.json](moore-b.json) |

The two GIFs are **byte-identical**. The sidecars agree in every key except `rendered_utc`, the
wall-clock time of the render. That includes the 7 frames, each frame's volume, valid time,
start and hold, and the 2,250 ms duration.

[moore-a.gif](moore-a.gif) is the loop (KTLX 19:51–20:16 UTC, 0.5° reflectivity, no basemap).

## Scope

- **Covered:** the off-screen path (`--watch`, `--serve`). The radar inputs are archived volumes;
  `--basemap none` keeps map tiles fetched from the network out of the comparison.
- **Not covered:**
  - the in-app loop export, which screenshots the running window;
  - another GPU or driver.
  - (A basemap: covered on 2026-10-10, below.)
- **Not run:**
  - a sustained 30-minute capture;
  - a real ffmpeg encode at 60 fps (ffmpeg is not installed here);
  - FTP/SFTP delivery.

## With a basemap (2026-10-10)

The same loop with the dark vector basemap, rendered twice on the RTX 2060 (debug build,
`feat/wsv3-redesign` after `9af1aee`):

```bash
hookecho --watch --site KTLX --from 2013-05-20T19:50 --to 2013-05-20T20:20 --basemap dark --size 512 --interval fixed --fps 4 --out moore-dark-a.gif
```

| | Run A | Run B |
| --- | --- | --- |
| Exit, time | 0, 194 s (including the build and the first tile fetch) | 0, 62 s |
| GIF SHA-256 | `b822df49a800d3fd72a418d75711897b8d6bd389712ba75e26e3e203bef80828` | the same (byte-identical) |
| Sidecar | [moore-dark-a.json](moore-dark-a.json) | [moore-dark-b.json](moore-dark-b.json) |

The sidecars differ only in `rendered_utc`; all 7 frames agree. The basemap came from
OpenFreeMap's planet build `20261004_113936_pt` (9 vector tiles, 1,845 labels). A later planet
build can change the map, so the result is determinism for one basemap build, not across them.
[moore-dark-a.gif](moore-dark-a.gif) is the loop.

## Another adapter: the software rasterizer (2026-10-10)

There is only one GPU here, but `HOOKECHO_GPU_FALLBACK=1` forces wgpu's fallback adapter
(Microsoft Basic Render Driver). The basemap loop above was rendered on it once more.

- **Frame timing is identical.** The sidecar [moore-dark-sw.json](moore-dark-sw.json) agrees with
  the RTX 2060's in every key except `rendered_utc`: the same 7 frames, scans, valid times and
  holds.
- **The pictures are not byte-identical** (GIF SHA-256 `83321d1ab97696d77c11c3a25dc0cf4e995af8b17b69a95298861ebc62db4d7b`). Frames were decoded with a
  stdlib GIF reader written for this check; it reads the two RTX runs as identical in every
  pixel. RTX against software differs in 5.6–6.3 % of pixels per frame, 2.3–2.8 % by more than
  16 levels ([gpu-vs-software.txt](gpu-vs-software.txt)). **Every difference is inside the radar
  echo; the basemap's lines and labels match**
  ([gpu-vs-software-frame0.png](gpu-vs-software-frame0.png): red where they differ by more than 16).
- **Zoomed in** (zoom 10 over Moore, no basemap, 20:00–20:10), the share rises to 46–49 % of all
  pixels ([gpu-vs-software-z10.txt](gpu-vs-software-z10.txt)). No offset of up to ±2 pixels
  explains it (38–39 % at best). Side by side
  ([gpu-left-software-right-z10.png](gpu-left-software-right-z10.png)) and magnified
  ([gpu-left-software-right-crop.png](gpu-left-software-right-crop.png)), the two show the same
  storm structure, colour bands and histogram, with different fine detail.

**Not established: the cause.** `--watch` draws the radar smoothed: a sharpened bilinear blend of
gate values and an interpolated colour lookup. The shader also computes each pixel's range and
azimuth in single precision (`radar.wgsl`). The likely cause is that the GPU's and the software
rasterizer's single-precision trigonometry differ by an ulp or so, and that difference moves
interpolated values across the colour table's hard band edges, then through the GIF palette.
That was not confirmed: there is no nearest-gate `--watch` run to compare against. What does
hold: one adapter repeats itself byte for byte, and frame identity and timing do not depend on
the adapter. Pixel-identical output across GPUs is not a property of this renderer today.

| File | SHA-256 |
| --- | --- |
| moore-dark-sw.json | `6bd07e7b509b1edc881bdc737d8c3a65f1b731751d9889a268b9ad69dc508c67` |
| gpu-vs-software.txt | `9d101b65f70b14b3db2b62bf36acfad261a096d3b29896aae6274e883832fdc4` |
| gpu-vs-software-z10.txt | `d380a7d6c866ab14a2139edce0b49715366b0ad2c7ea707a06c933269ba53dd3` |
| gpu-vs-software-frame0.png | `b6d39fb486e2322ecc41fc9777fabcbae9721f3b3ba8003ac33830fa8120b91c` |
| gpu-left-software-right-z10.png | `1368acb93214901161b7d15a0f8eb509034bbb75ffd2226537c4af53042e21f4` |
| gpu-left-software-right-crop.png | `26af34ca5341ed5df1337a60449021da147782cd740d8591e8f7504b2b9aec62` |

## The cause, and the fix (2026-10-12, 1010.md V19)

`--watch` has a `--nearest` switch now, which draws each gate as its own value with no
smoothing. With it, the RTX and the software adapter still disagreed at 47.5 % of pixels, against
48.2 % smoothed. So smoothing across colour bands was not the cause.

The cause was the shader's range and azimuth (`gpu_radar_gate_choice_against_double_precision`,
[gate-choice.txt](gate-choice.txt)). That test runs them as a compute shader for every pixel of a
512 px view of Moore and compares the gate and radial picked with double precision:
- **RTX 2060 (Vulkan):** the haversine was off by at most 0.015 gates, and picked a different cell
  at 0.54 % of pixels.
- **Software rasterizer (Dx12):** off by up to 3.7 gates (0.9 km) and 1.8 radials, a different
  gate at every pixel. Its sin, cos and sinh are not precise enough for the haversine's small
  differences, or for the azimuth's difference of two nearly equal terms.

`radar_polar.wgsl` now computes both from the radar's world position and the sine and cosine of
its latitude, which the renderer works out in double precision (`render::radar_uniform`).
Everything that depends on the fragment is a short series in small differences, plus a square
root and one atan2. A different cell from double precision:
- RTX: 0.03 % of pixels, largest error 0.0003 gates;
- software: 0.10 %, largest error 0.0003 gates and 0.0044 radials;
- RTX against software: 0.08 % of cells.

The pictures ([gpu-vs-software-after-fix.txt](gpu-vs-software-after-fix.txt); KTLX 20:03:56 UTC,
zoom 10 over Moore, no basemap):

| RTX against software | Pixels that differ | By more than 16 levels |
| --- | --- | --- |
| Smoothed, before | 48.19 % | 36.29 % |
| Nearest gate, before | 47.54 % | 37.56 % |
| Smoothed, after | 16.76 % | 0.17 % |
| Nearest gate, after | 7.68 % | 0.15 % |

Nearly all that remains of the nearest-gate difference is one level in one channel: 18,624
pixels, against 382 that differ by more than 16. That is colour rounding, not a different gate.
Red in [gpu-vs-software-nearest-before.png](gpu-vs-software-nearest-before.png) and
[gpu-vs-software-nearest-after.png](gpu-vs-software-nearest-after.png) is a difference of more
than 16 levels, orange a smaller one. The RTX's own picture changed at 0.28 % of pixels, and
repeat runs on it are still byte-identical.

| File | SHA-256 |
| --- | --- |
| gate-choice.txt | `c4a8cec78c29ef7cd5ffddccc326b9b97fd343bc11fc8e84c48aa625de19e571` |
| gpu-vs-software-after-fix.txt | `e1b1b5dc0151c40f21f7c066a59bdbeeed6cd9531d65888b3b7c4579e8a621d4` |
| gpu-vs-software-nearest-before.png | `5fa9dfd81418acdfa84c20abbfb21ea10b5897252d76f7919a1cfd83f5781f4d` |
| gpu-vs-software-nearest-after.png | `3bcfbf3104acec85478f47298f44e716d4ea9cc98b8823a0a301f2d8d80e91b8` |

Still not established: pixel identity across adapters (the one-level colour rounding remains),
and a second physical GPU. The MRMS shader's latitude uses the same `atan(sinh(n))` and was not
changed; its grid cells are a kilometre, against the 40 m that error reached on the software
adapter.

## Archived warnings, discussions and reports in a capture (2026-10-13, roadmaps/broadcast B2)

A loop export's manifest recorded readiness only for field layers. The archive-backed overlays
were neither stamped nor safe to capture. Warnings and SPC discussions come per 5-minute bucket
of the playhead, storm reports per 30 minutes. While a frame's bucket was still loading, the map
kept the previous bucket's set. On the first scrub into the archive it showed **today's live
warnings, discussions and storm reports over the archived scan**, and the storm card, alert list
and associations used them. A capture of that frame recorded it as complete.

Now `alerts_watch::archived_or_live` decides all three. In the archive the shown bucket is the
playhead's own from the moment it is wanted, empty until it is fetched, and the overlays rebuild
when it arrives. Live, the live set. The loop export stamps each shown overlay (`warnings`,
`discussions`, `storm-reports`) with its bucket's start, ready only once that bucket is fetched
(`loop_capture::archive_stamp`). A frame captured early lists "not ready: warnings" among the
manifest's problems. Tests: `an_archived_scan_never_shows_live_or_another_buckets_overlay`,
`an_overlay_still_loading_is_stamped_not_ready`. Not seen in a run: no capture was made this
session, and the browser pane was hidden.

Still open for B2: GIS layers and dressing (logos, annotations) have no time-addressed fetch to
wait for and are not stamped. Waiting (rather than listing) on a not-ready overlay uses the
existing pause only for the radar frame.

ProbSevere, the same way (2026-10-13). The app keeps no archive of ProbSevere, but over an
archived scan today's live polygons were drawn on the map, fed the archived storms' severity
scores (the Storms table, the cell card, the compare view, the digest's ranking) and the storm
evidence record, and the card said "no containing source polygon" as if it had looked.
`active_probsevere` is empty over an archived scan, and the card says "not kept for past times".
Live behaviour is unchanged.

Live-only point layers, the same way (2026-10-13). Ground strikes from the broker, Spotter Network
positions and PIREPs exist only as "now", but were drawn over a pane scrubbed or looped back to an
older scan, as if they belonged to it (a spotter beside a 2021 storm). They now show only on a
pane following the newest scan (`pane_is_live`), the rule live storm cells already followed. GOES
GLM lightning, which has an archive window, is unchanged.
Live station observations and mPING precipitation-type reports, which are "now" too, follow the
same rule.
