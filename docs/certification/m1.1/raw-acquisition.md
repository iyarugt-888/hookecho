# M1.1 raw cut acquisition inventory

Open **Analyst Mode** in Settings. On workstation layouts, expand **Cut acquisition details** in
the Analyst log dock; on phone layouts, expand it in the Analyst log page. The shared component
wraps all evidence without requiring hover. The phone page bounds the inventory scroll area so
the live log remains usable.

## What the inventory establishes

The receiver records raw radial numbers before older sweeps can fill holes during stitching.
Presence and clocks are separate: a radial with no usable clock is observed with unknown time.
A subsequent untimed gap fill resolves its missing position without inventing acquisition time.
Counts deduplicate positions; bounds describe recorded positive source clocks only. An already
known clock survives a delayed duplicate with an older time. Zero and out-of-range one-based
radial numbers are ignored rather than mapped to another position.

Every source VCP ordinal retains its angle and standard/SAILS/MRLE/MPDA label independently of
other cuts at that angle. Unobserved cuts keep unknown angle/kind/clocks. Progress-only chunks
keep unavailable raw counts; an explicitly decoded empty input reports zero raw positions.
Unobserved chunks include both earlier and future sectors, including a mid-volume join. Internally
bounded unobserved spans use raw arrivals, never rendered angular zero values. Source volume,
VCP and cut-count changes retire the preceding inventory; older known volume starts are rejected.
A missing VCP on a known new volume remains unknown.

The snapshot is immutable across further arrivals and independent of the timeline playhead.
The legacy chunk completion phases remain receipt summaries. They do not certify complete
radial or VCP coverage. The existing rotation-time revisit reset is still an inference, so this
inventory does not claim persistent pass identities, native radial denominators, transport loss,
complete columns or a detector input ledger. Per-position clocks retain the best known timestamp
within that receiver cut; counts are not an arrival history.

## Controls and reproduction

```sh
cargo test -p hookecho --lib raw_inventory
cargo test -p hookecho --lib raw_acquisition
cargo test -p hookecho --lib gpu_raw_acquisition_snapshots -- --ignored --nocapture
cargo test --workspace -- --test-threads=4
cargo clippy --workspace --all-targets -- -D warnings
```

Four acquisition controls cover untimed positions, bounded holes, late gap fill, duplicate and
invalid radial numbers, immutable snapshots, equal-angle standard/SAILS/MRLE cuts, out-of-order
cuts, inferred revisit reset, volume rollover, unavailable new VCP metadata, empty raw input and
site reset. Shared UI controls distinguish unknown raw evidence from zero and verify wrapping at
240 px touch and 300 px desktop. The explicitly invoked GPU helper renders controlled raw and
progress-only inventories, using production fonts/theme/painter; it does not render a radar map
or establish physical-device interaction. The explicit Windows invocation passed in 3.05 seconds
after compilation. Four reviewed
references show all labels wrapping within 240 px touch and 300 px desktop panels, without
clipping: [raw touch](raw-acquisition-ui/raw-240.png), [raw desktop](raw-acquisition-ui/raw-300.png),
[metadata touch](raw-acquisition-ui/metadata-240.png), [metadata desktop](raw-acquisition-ui/metadata-300.png).
[Hashes, byte counts and capture times](raw-acquisition-ui/captures.json) accompany the exact
copies, independently verified after copying. These are expanded-section references; the full
phone page, collapse/scroll interaction and a live radar viewport remain runtime work.
Final workspace, Clippy and browser compatibility results are recorded in ROADMAP_PARITY.md.

## Remaining work

Bind inventory to the accepted decoded source/revision in derived and 3D payloads, then add
source-driven persistent pass boundaries and explicit transport evidence. The provider's current
chunk layout supplies positional storage; absent edge sectors are not inferred as transport loss.
Retain historical pass inventory rather than replacing it on an inferred revisit. Exercise a real
mid-volume join and recovery session, full application interaction and Android hardware before
claiming runtime parity. This feature does not change radar gates or tornado detection work.
