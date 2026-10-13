# M1.2 correlated receipt-to-GPU trace (1008.md A3)

The next CPU-copy increment is documented in [immutable live sweep sharing](shared-sweeps.md)
(1010.md V7), with a release snapshot benchmark and regression controls. Streaming frame
times and device evidence remain open.

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

## A live chunk-stream trace in the running app (browser, 2026-10-10)

The browser build (`feat/wsv3-redesign` at `9af1aee`, WebGPU, built-in Chromium, the RTX 2060
machine) streamed KTLX live, about 20:15–20:21 UTC: Unidata Level II chunks from AWS S3,
progressive radials, VCP 35, cuts 1 to 8 of 12. Analyst mode was on, and the timeline sat at the
live head. The Analyst log's own counters (`LiveQueueTimings`, the same code as the correlated
trace above) read:

| Stage | First chunk | After ~5 min (42 samples) |
| --- | --- | --- |
| Ingest lag | 3 s | 1 s |
| Decode | 42 ms | 37 ms |
| Client receipt → GPU queue writes | p50 68 / p95 68 ms (1) | p50 48 / p95 65 ms |
| Client receipt → GPU finished the frame | p50 81 / p95 81 ms (1) | p50 58 / p95 72 ms |
| Frame build | p50 4.1 / p95 10.9 ms | p50 3.4 / p95 3.7 ms |

During the session one NWS warning message arrived. It took 39 s from its NWS `sent` time to
the app accepting the poll that carried it, and 0.01 s from there to the first frame built
after it. That is a single sample of A1's received → drawn stage in a running session.

Captures: [browser-live-trace-start.jpg](browser-live-trace-start.jpg) (SHA-256
`faf642b9b51edd3bd4cfd338d89e9e1b47709073816f863a00e705284e9450e2`) and [browser-live-trace-5min.jpg](browser-live-trace-5min.jpg)
(SHA-256 `bb6c897239cfbb2b8925e12b245d06ea12b65899840fa7faacebd6ed7c63f51a`).

**Limits:** these are the app's rolling counters as displayed, not an exported per-chunk trace.
They cover one browser on one desktop, not Android. Screen scan-out is not measured, as the log
itself says.
