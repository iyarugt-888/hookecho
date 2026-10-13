# Immutable live sweep sharing — 2026-10-10 (1010.md V7)

Baseline: `d3079e7` on `feat/wsv3-redesign`, plus the pre-existing uncommitted timeline
chronology work. This increment changes the vendored `Sweep` storage to `Arc<Vec<Radial>>`.
`Scan::clone`, `merge_scan`'s unchanged cuts and stream emissions share gate buffers. A
stitched cut owns a new allocation. A consuming `Sweep::merge` unwraps unique buffers and
copies shared ones before editing. No source clocks, values, public slices or wire fields change.

## Release measurement

Command:

```powershell
cargo test --release -p wxdata --test live_shared_sweeps corpus_snapshot_copy_cost -- --ignored --nocapture
```

Input: cached NOAA Archive II `KTLX20130520_201229_V06.gz`, 17 sweeps, 8,280 radials.
Input SHA-256: `e1160f973262663dcb04d70e8fcdfffa2be978327eba87af9972e833c505f100`.
Windows desktop, 100 iterations per path. Workspace compilation shared the machine during
this run. Both paths were warmed first. Every output's sweeps compared equal to the input.

| Snapshot construction | p50 | p95 |
| --- | --- | --- |
| Explicit deep copy of every radial and moment buffer (previous storage behavior) | 28.2252 ms | 33.4368 ms |
| Shared immutable buffers (`Scan::clone`) | 0.0013 ms | 0.0020 ms |

The deep reference reconstructs each sweep from `radials().to_vec()`; the shared path clones
the scan. Both copy VCP metadata; the deep reference does not copy the site's small metadata.
Timing excludes input decoding, output destruction and equality checks. This isolates the
snapshot copying eliminated here. It is **not** a full merge benchmark, a renderer trace,
receipt-to-GPU latency, frame rate, allocation counter, or Android device measurement.
Stitching still copies the touched cut's radials; unchanged cuts avoid those copies.

## Regression controls

`crates/wxdata/tests/live_shared_sweeps.rs` covers:

- changed cuts detach, unchanged cuts and cloned snapshots share buffer addresses;
- earlier snapshots keep their values, timestamps and Doppler metadata;
- consuming merges cannot mutate retained snapshots;
- empty, duplicate and delayed updates preserve existing buffers;
- unknown clocks and empty sweeps survive;
- JSON and postcard sequences are byte/shape-compatible with the previous `Vec<Radial>`
  representation, including an old reader consuming new postcard bytes.

Required check logs and the release output live under
`target/parity-review/m1.2/shared-sweeps/`. Durable results and hashes accompany this report.

## Open verification

Measure live animation frame times while chunks stream, including touched-cut stitching;
capture the desktop workflow; measure Android on a physical device. The WASM check validates
compilation and the unit controls validate worker wire compatibility; browser runtime and
presentation latency are separate gates. Existing conservative cache byte accounting may
count a shared buffer more than once; no cache budget was raised.
