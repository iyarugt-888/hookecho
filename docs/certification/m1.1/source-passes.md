# M1.1 source-marked pass history

Both progressive providers inspect native radial boundary statuses before stitching repeated
elevations by azimuth. A pass key is the actual elevation number and the recorded positive
start-marker clock in milliseconds, scoped to the source radar by its accepted receipt. Native
ScanStart, ElevationStart and ElevationStartVCPFinal establish starts; ElevationEnd and ScanEnd
record ends. Equal elevation angles, local repeat counters, client receipt times and expected
rotation durations cannot establish these identities.

Mid-cut joins and starts with unknown clocks remain unanchored. Consecutive successfully
decoded source transport sequences can continue an open native pass between incremental inputs
within a volume. A skipped
sequence or failed decode stops borrowing the earlier anchor; this does not by itself prove
radial loss. Closed passes and elevation transitions cannot silently lend their old anchor to a
later unmarked cut. Bounded duplicate tracking prevents repeated backfill prefixes from
reopening earlier passes. Independently decoded marked input reproduces the same source keys.

Each provider retains a bounded source ledger even when delayed input cannot replace newer
displayed gates. Its next accepted update carries that history. This avoids consuming late
boundary evidence in a non-rendering decode and then losing it. The receiver retains immutable
summary copies with accepted frames and their existing derived/3D source receipts. The
ordinal/chunk inventory and inferred revisit behavior remain separate. A VCP change in the
same source volume does not erase native pass history; source volume/site rollover resets it.

The source ledger holds at most 128 anchored passes, with at most 720 native positions per
elevation/pass and a bounded duplicate fingerprint history. Older retired passes cannot displace
newer retained passes. It holds no gate buffers. Summary capacities and bounded-hole spans
are included in existing smooth/isosurface receipt byte charges; these are allocation estimates,
not measured process/GPU memory.

## Inspection and reproduction

Expand **Source-marked passes** under acquisition details in the Inspector or shared Analyst
surface. A bounded vertical scroll area shows newest source passes first, recorded start/end
markers, unique observed positions, unknown-clock positions, known acquisition bounds and
internally bounded unobserved spans. End markers do not certify every radial. The union of
unanchored positions is deduplicated within each elevation; it is not an unknown-pass or packet
count. Missing subsequent native evidence and retired history are explicitly qualified.

```sh
cargo test -p wxdata --lib live_pass -- --test-threads=4
cargo test -p wxdata --lib decoded_backfill_progress
cargo test -p hookecho --lib source_pass
cargo test -p hookecho --lib relay_provider::integration_tests -- --test-threads=4
cargo test -p hookecho --lib gpu_source_pass_snapshots -- --ignored --nocapture
cargo test --workspace -- --test-threads=4
cargo clippy --workspace --all-targets -- -D warnings
```

Domain controls exercise close same-elevation repeats, native starts/ends, untimed positions,
mid-cut joins, unknown start clocks, sequence discontinuity, elevation transitions, duplicate
backfill, independently marked late gap fill, delayed passes whose gates lose the merge, bounded
history and malformed positions/keys/clocks. The pinned partial Mayfield Level II fixture proves
stable native keys across independent decodes and incomplete boundary evidence in a truncated
source. It does not reconstruct historical transport arrivals or certify storm science. A local
HTTP/WebSocket integration exercises native boundary evidence through the actual relay protocol
with controlled bytes. Neither control is an external operational live-source soak.

Receiver controls exercise accepted receipt immutability, VCP changes, inferred revisits, cache
charges, unavailable subsequent input, and volume/site resets. Existing derived/3D ownership
controls continue to cover propagation of the source receipt separately from contributor coverage.

The GPU helper renders the production expanded pass painter in HookEcho's Dear ImGui theme at
one pixel per point. The controlled history contains two elevation-1 starts at
1,700,000,000,000 and +20,000 ms. The older pass retains positions 1/2/4, an unknown clock at 2,
bounded hole 3 and an end marker. The newer pass retains positions 1/2, an unknown clock at 2
and no end marker. Two unanchored elevation-2 positions include one unknown clock; a subsequent
update has unavailable native evidence. Separate references show unavailable history.

Reviewed references are [history at 240 px touch](source-pass-ui/history-240.png),
[history at 300 px desktop](source-pass-ui/history-300.png),
[unavailable at 240 px touch](source-pass-ui/unavailable-240.png) and
[unavailable at 300 px desktop](source-pass-ui/unavailable-300.png), with a
[hash/size/time/source manifest](source-pass-ui/captures.json). Fresh output is under
`target/parity-review/m1.1/source-pass-ui/` at captured revision `af0d1e6`. The subsequent
[continuity increment](pass-continuity.md) updates the helper's output directory and qualifications;
these original references retain their own recorded source hashes. The helper does not retain adapter/driver identity.
These references establish expanded-section wrapping, not real device, scroll/collapse interaction
or full radar viewport behavior. The GPU check is explicitly invoked; its default ignored state
does not certify execution.

Final checks are recorded in increment 7 of [ROADMAP_PARITY.md](../../../ROADMAP_PARITY.md).
Logs are under `target/parity-review/m1.1/source-passes/`.

## Remaining gates and handoff

The initial backfill can assemble several source chunks, with failed downloads omitted. This
increment did not certify transport continuity throughout that assembled input or pass
association across every missing boundary. The subsequent [continuity controls](pass-continuity.md)
keep ordinary positions unanchored in a known discontinuous assembly. Native IDs still require recorded start clocks;
source-sequence receipts and stronger contributor association remain the next controls.

The receipt now contains native pass history; merged product contributor rows/gates do not yet
carry an explicit association to those keys. Native passes do not replace the existing relay
CutTracker's arrival-ordered repeat counter or the Unidata mapper's predicted VCP cut position.
Missing/undeclared cut metadata can leave an accepted update's raw receipt unavailable. Metadata
in a non-rendering decode becomes visible with the next accepted update, not retroactively on
the already accepted frame. Unmarked delayed radials cannot always be assigned to an old pass.

Explicit transport-gap origins and recovery evidence, persisted progressive replay, actual
cross-provider mid-volume joins, complete repeated-cut/SAILS/MRLE operational fixtures, full
viewport interaction, physical Android/browser runtime, completed-GPU timing and sustained-load
certification remain open. Keep continuous defaults and the delivered strict scientific masks;
unobserved positions and unknown clocks are not transport loss or column completeness. Tornado
detection methodology and calculations remain Claude's work.
