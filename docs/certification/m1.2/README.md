# M1.2 correlated receipt-to-GPU trace (1008.md A3)

`gpu_correlated_latency_trace` (`crates/hookecho/src/headless_corpus.rs`) takes one real volume's
bytes as received and times every stage after it against that single receipt clock. It runs 25
times after 3 warm-ups, on the device the test runs on. Each CSV row is one run and names the
frame it drew: volume, scan time and tilt.

| Stage | What is timed |
| --- | --- |
| decoded | `level2::decode_volume`; this archive file is gzipped, so the stage includes gunzip |
| binned | the 0.5° reflectivity sweep (`level2::bin_scan`) |
| 2D GPU queue writes | the app's own `LiveQueueTimings` queue stage |
| GPU finished the 2D frame | the app's own completion stage: `on_submitted_work_done` of the frame that drew it |
| 3D volume built | binning every tilt plus `volume3d::build` at 192 × 192 × 48, as the 3D window builds it |
| 3D volume on the GPU | `Volume3dResources::upload`, then a device poll until the GPU has it |

Presentation (display scan-out) is not observable in this harness and is not reported. Nothing
here is presented as visible latency.

## RTX 2060 (Vulkan), Windows, 2026-10-09

Moore 2013 `KTLX20130520_201229_V06`:

| Stage (from receipt) | p50 | p95 |
| --- | --- | --- |
| decoded | 145.5 ms | 150.4 ms |
| 0.5° REF binned | 146.6 ms | 151.8 ms |
| 2D GPU queue writes | 147.1 ms | 152.5 ms |
| GPU finished the 2D frame | 148.3 ms | 153.4 ms |
| 3D volume built | 167.0 ms | 172.4 ms |
| 3D volume on the GPU | 173.6 ms | 182.1 ms |

On this whole-volume archive input, decoding is the dominant cost: about 98% of the time to the
GPU finishing the 2D frame. From the binned sweep, the 2D upload and the GPU's frame add about
1.7 ms. The 3D stages add about 19 ms to build and 7 ms to reach the GPU.

A live chunk stream decodes incrementally, one chunk at a time, so these figures describe opening
or replaying a whole volume, not a live radial's path. That path is what the app's
`LiveQueueTimings` stages measure in use.

| File | SHA-256 |
| --- | --- |
| [correlated-trace.txt](correlated-trace.txt) | `ac2b2b57156f5de8184550a490bd231022f49683f0c0d281be20ce8fe706acba` |
| [correlated-trace.csv](correlated-trace.csv) | `4fd9854cf97ba0871e0cceba0a412d925b207418e440cf329ad08241b8922777` |

Rerun with:

```bash
cargo test -p hookecho --release --lib gpu_correlated_latency_trace -- --ignored --nocapture
```

## Not established

- An Android device trace.
- Presentation latency: it needs an external frame observation.
- A live chunk-stream trace with frame identity through to GPU completion in the running app.
