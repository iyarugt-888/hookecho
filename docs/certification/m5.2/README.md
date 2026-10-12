# M5.2 satellite loop: what a frame costs (1008.md E2)

The satellite loop decodes each frame it shows. Increment 1 recorded the per-frame decode time
as unmeasured, so whether 4 or 8 fps could really be reached was unknown. The live test
`goes_abi::tests::loop_decode_cost_live` (network, run with `--release`) now times download and
decode apart. It uses ten consecutive GOES-East scans per case, at the sizes the app asks for
(`goes_grid`: 480×480 mesoscale, 1200×700 CONUS), on this Windows desktop (16 threads).

## Before and after, 2026-10-10

The decode forward-projects every source pixel, then bins it. For a 2500×1500 CONUS band the
projection and binning took 278 ms of about 350 ms, and the HDF5 read took 71 ms. Each pixel's
projection is independent, so native builds now project rows in parallel (rayon). Binning stays
sequential in source order, so the output is the same. The browser build keeps the one-thread
pass.

| Case | Granule | Download p50 | Decode p50, one thread | Decode p50, parallel |
| --- | --- | --- | --- | --- |
| Mesoscale 1, band 13 → 480×480 | 0.3 MB | 63–69 ms | 26 ms | 14 ms |
| Mesoscale 1, band 2 → 480×480 | 4.4–4.8 MB | 242–290 ms | 423 ms | 214 ms |
| CONUS, band 13 → 1200×700 | 3.9–4.0 MB | 190–194 ms | 387 ms | 193 ms |

The one-thread run used `RAYON_NUM_THREADS=1` on the same code, 24 s after the parallel run,
over the same 30 scans. **Every one of the 30 decoded fields has the same digest** (FNV-1a over
the values and bounds) in both runs:

- [loop-decode-par.txt](loop-decode-par.txt), SHA-256 `417452e0bfd864d673e7f8c60d185470bf8f25d46505b45b2f81fea4308eef98`
- [loop-decode-one.txt](loop-decode-one.txt), SHA-256 `b714816f53c0e49e1713d1795b9cd6d2e2929a0b127fecf83e9e925ee1146169`

The first measurement, before the change and without digests, gave decode p50 of 28, 400 and
364 ms.

## What this means for the loop's speeds

The loop steps only when the shown scan is decoded, and the prefetch already hides the download.

- **Infrared mesoscale:** decodes in about 14 ms, so all three speeds (2/4/8 fps, 500/250/125 ms
  a frame) have room.
- **Visible mesoscale and CONUS:** now decode in about 200 ms, which fits 2 and 4 fps but not
  8 fps.

On a phone or in the browser (one thread), expect the one-thread figures or slower; those
platforms were not measured.

## Not established

- 8 fps for the visible and CONUS bands on the loop's first pass (decode-ahead, below, covers
  later passes).
- Android and browser timings.

## Decode-ahead (2026-10-10)

On desktop the loop's prefetch now decodes the frames ahead of the playhead
(`goes_abi::decode_ahead`), at the size the GOES layers ask for, off the async worker. The
decoded fields go into a cache bounded at 96 MB (`goes_abi::decoded`, least recently used out
first). `fetch_key` answers from that cache, and fills it, so a loop that wraps or steps back
does not decode a scan twice. This changes increment 1's "one decoded frame per layer" to a
bounded set of decoded frames. The browser, with one thread, still downloads ahead and decodes
each frame when it is shown.

Live, release, ten CONUS band 13 scans at 1200×700
([loop-decode-ahead-live.txt](loop-decode-ahead-live.txt), SHA-256 `7436b2c520678f842e8b893c613ea9272db2c83b4660e3150b9686a502f8b908`):

- **A frame shown from the cache:** p50 0.6 ms, max 0.8 ms, against about 193 ms to decode.
- **Decoding ahead two at a time,** as the loop does: 3.0 s for ten frames, **3.3 frames/s**.
  Two concurrent decodes share the cores the parallel projection already uses, so this is no
  faster than one at a time (about 5 frames/s from the single-decode timings). Both 12- and
  24-hour soaks were running on the machine at the same time.

What it means for the loop:

- **The first pass** through a large-band loop is still decode-bound, at about 3–5 fps.
- **From the second pass**, every frame of a CONUS loop up to 2 hours (24 frames, 81 MB) or a
  1-hour mesoscale loop (60 frames, 55 MB) is in the cache, and fetching a frame costs well under
  a millisecond. Decoding no longer limits 8 fps. The rest of a frame (texture upload and draw)
  was not measured in a running loop.
- A loop whose frames exceed the budget (more bands, or a 2-hour mesoscale loop) evicts, in
  loop order, the frames it needs next. That is the worst case for this policy, and it falls
  back to decoding.

Test: `the_decoded_frame_cache_is_bounded_and_keyed_by_size`. It covers the budget, the least
recently used being evicted first, sizes as separate entries, and a read keeping a frame.

## One decode at a time or two (2026-10-12, 1010.md V19)

`loop_decode_ahead_live` times decoding ahead two at a time, as the loop does, against one at a
time, each on its own CONUS band 13 scans (live, release, 1200 by 700). Two at a time was faster
in each of three runs:

| Run | Two at a time | One at a time |
| --- | --- | --- |
| 20 scans, 10 each | 1.3 frames/s | 1.2 frames/s |
| 20 scans, 10 each | 1.6 frames/s | 1.3 frames/s |
| 10 scans, 6 and 4 | 1.7 frames/s | 1.3 frames/s |

So the loop keeps `PREFETCH_INFLIGHT = 2`. A frame from the cache still costs 0.5 to 0.6 ms
([loop-decode-ahead-live-2026-10-12.txt](loop-decode-ahead-live-2026-10-12.txt), SHA-256
`33004274d961fef96a7504d8edcef4656ebad3ceb3eeb73650c1a81f6bc3b916`, the last run).

Both rates are well under the 3.3 frames/s measured on 2026-10-10. Ten scans decoded at the
same rate as twenty, so the GOES file cache evicting scans was not the cause. Nothing else heavy
was running. The cause is not established.

## The loop's real rate in the desktop app (2026-10-12, 1010.md V19)

The loop logs each pass when its playhead wraps (`sat_loop::PassClock`). A deep link's new `sat`
field opens the loop by itself, so the desktop app could run it with nothing pressed:
`HOOKECHO_GOTO=KTLX,-97.3,35.3,6,sat`, which turned on GOES infrared (band 13) and started a live
GOES-East CONUS loop of 60 minutes at the default 4 fps.

**Windows desktop app, one machine:** an optimized build with debug assertions on (the sandboxed
storage needs them), built from the working tree at `17bf42f0` with this change and another
session's uncommitted work; RTX 2060 on Vulkan; the window on screen.

| Pass | Frames | Time | Frames/s |
| --- | --- | --- | --- |
| 1 | 1 | 2.2 s | 0.46 (the listing arrived mid-pass) |
| 2 | 11 | 4.2 s | 2.65 |
| 3 to 13 | 11 each | 3.8 to 3.9 s | 2.84 to 2.90 |

None of the 133 frames failed to appear. At 4 fps eleven frames take 2.75 s, and the end dwell adds
1 s: 3.75 s a pass, 2.93 frames/s. So from the second pass on the loop plays at the rate asked,
and the first full pass, with frames still downloading and decoding ahead two at a time, ran
about 0.45 s slow. The map drew at 9.7 ms a frame (the debug overlay, top left of the capture).
A debug build on the same loop managed 0.57 frames/s on its first full pass and 1.74 on its
fifth: decode time is what holds a slow build back.

| File | SHA-256 |
| --- | --- |
| sat-loop-passes-desktop.txt | `391ee950c20568bcfa605082c4c176286565251851ee469c6da0362a8acb5983` |
| sat-loop-desktop.jpg | `9a85df2f9d385698c609c5402b23ec40d1b397888b28ec6b15725266f19edba6` |

`sat-loop-desktop.jpg`: the loop bar reading "last pass 2.9 fps" and "GOES-East CONUS (5 min) ·
14:36:17 UTC · 1/11 · 11/11 cached", with the infrared clouds over the West.

Not established: 8 fps (the speed is only settable with the pointer, and nothing here drives it),
a one-minute mesoscale loop, the browser with its pane shown, and Android.
