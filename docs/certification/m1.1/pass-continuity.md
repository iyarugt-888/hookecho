# M1.1 discontinuous source assemblies

Initial direct-source backfill can omit failed downloads before assembling one decoded scan.
The relay also coalesces retained blocks for decoding. Native radial ordering alone cannot prove
that a missing start/end boundary was absent from those omitted bytes.

Before assembly erases chunk boundaries, the direct provider checks the actual downloaded Start,
joined position and every requested middle position. Its first incomplete input retains valid
native start IDs while ordinary positions and end markers stay unanchored. A combined metadata
prefix/current chunk after a failed decode also receives conservative association when source
positions are not adjacent. A metadata-only Start followed by its actual adjacent chunk remains
contiguous; failed decoding alone is not invented transport loss. Relay input
requires adjacent source sequences without duplicates, jumps, reversals or overflow; a mid-volume
join's unknown earlier prefix is not counted as missing. Decode failure stops borrowing an old
anchor. Retained non-rendering history remains bounded and reaches the next accepted update.

An untimed boundary's repeated elevation/position/status fingerprint is ambiguous: it can be
old backfill or another start/end with an unknown clock. Such a duplicate clears the possible
old anchors, including those at other source elevations; later ordinary radials cannot silently
inherit an intervening timed pass's ID.

Discontinuous assemblies cannot lend their questionable starts to later inputs. A fresh native
start in subsequent contiguous input can establish association again. This qualification changes
metadata only: existing newest-radial stitching, gates, continuous defaults, strict scientific
masking and detection methodology remain intact. In a discontinuous relay assembly the check is
deliberately conservative across all coalesced retained blocks; it does not attempt to locate
the exact gap's radial boundary. It may leave valid positions unanchored until a fresh source
context or stronger association becomes available.

The Inspector's expanded pass history and all shared derived/3D receipt rows qualify source
continuity. The counter counts inspected decoded assemblies, including non-rendering input;
it is not a count of missing packets, passes or radials. Unavailable native evidence remains a
different counter. Existing immutable accepted receipts cannot be rewritten by receiver progress.

```sh
cargo test -p wxdata --lib discontinu -- --test-threads=4
cargo test -p wxdata --lib initial_chunk_continuity
cargo test -p hookecho --lib coalesced_relay_positions
cargo test -p hookecho --lib gpu_source_pass_snapshots -- --ignored --nocapture
cargo test --workspace -- --test-threads=4
cargo clippy --workspace --all-targets -- -D warnings
```

Controls cover full/missing Start backfill, omitted middle downloads, duplicate/reversed positions,
unknown join prefixes, u64 overflow, native starts surviving discontinuity, unanchored ordinary
positions after nonadjacent failed-prefix input, usable adjacent metadata-only Start recovery,
positions/end markers, no questionable anchor lending, fresh marked-pass recovery and immutable
earlier summaries. These are deterministic source-association controls rather than an external
operational outage/soak. Shared wrapping controls render the discontinuity qualification at
240 px touch and 300 px desktop.

Reviewed references are [history at 240 px touch](source-pass-continuity-ui/history-240.png),
[history at 300 px desktop](source-pass-continuity-ui/history-300.png),
[unavailable at 240 px touch](source-pass-continuity-ui/unavailable-240.png) and
[unavailable at 300 px desktop](source-pass-continuity-ui/unavailable-300.png), with a
[capture/source manifest](source-pass-continuity-ui/captures.json). The controlled fixture from
the prior increment adds one discontinuous assembly qualification; it is not a storm or real
transport session. Fresh captures are under `target/parity-review/m1.1/source-pass-continuity-ui/`.
The helper uses HookEcho fonts and Dear ImGui theme at one pixel per point and does not retain
adapter/driver identity. Expanded layout does not certify scroll/collapse, full viewport or
physical-device interaction.

Final results are recorded in increment 8 of [ROADMAP_PARITY.md](../../../ROADMAP_PARITY.md).
Logs are under `target/parity-review/m1.1/pass-continuity/`. Explicit source-sequence origin and
gap/recovery receipts, exact contributor association, persisted progressive replay, operational
failover sessions and device/performance certification remain open. Tornado detection remains
Claude's work.
