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
