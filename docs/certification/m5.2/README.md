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

- 8 fps for the visible and CONUS bands. That would need frames decoded ahead, which the loop
  does not do: it holds one decoded frame per layer by design.
- Android and browser timings.
