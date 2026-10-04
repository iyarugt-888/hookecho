# M1.1 source sequence receipts

The Inspector's **Source sequence receipts** section records byte-transport positions separately
from native radar passes, raw radial positions and scientific contributor coverage. The same
summary follows accepted decoded frames through shared derived/3D source receipts and cache
ownership; later receiver progress cannot rewrite an accepted frame.

## Evidence semantics

- Direct-source positions identify Unidata chunks within the current source volume. Startup
  backfill records actual failed Start/middle requests instead of erasing their identities.
  Concurrent startup results are inspected in request/source order before the joined chunk;
  completion timing does not manufacture reordering or recovery. Incremental iterator errors
  lack an exposed failed object ID and remain unlocated errors, never predicted sequence holes.
- Relay positions identify validated blocks in the current subscription and source volume.
  An upstream label change starts a fresh assembly/receipt context. A label is declared provenance,
  not proof of emitter instance identity or independent acquisition. Reconnects also start fresh;
  this increment does not negotiate the relay server epoch or implement client resume.
- Bounded message holes lie strictly between received positions. Failed-request spans record
  actual unsuccessful requests and can include an explicitly requested prefix. No unrequested
  earlier prefix is called missing. Later received positions resolve a retained bounded hole or
  failed request, independently of whether their gates replace displayed data.
- Duplicate/reversed arrivals and failed assembly attempts have separate counters. Decode failure
  does not erase received bytes. Integrity-invalid/JSON-invalid messages do not establish positions.
  Sequence numbers and hole sizes do not certify lost radials, complete passes or product coverage.
- The ledger retains at most 4,096 known received/failed-request positions without enumerating
  large missing ranges. Provider envelopes box this summary to keep progressive channel messages
  compact. Older evidence is retired; spans describe retained positions, counters
  describe this subscription/volume context. Late arrivals outside that retained scope stay
  explicitly qualified. Receipt memory accounting includes span and origin-label capacities.
- A raw update with unavailable sequence evidence retains prior evidence and increments an
  unavailable-updates qualification. VCP changes preserve evidence in the same source volume;
  volume/site changes clear the receiver context. Archive/reloaded replay receipts remain unavailable.

## Controls and review

Deterministic controls exercise unknown mid-volume prefixes, actual failed requests, successful
late fill, repeated failures without erasing known bytes, duplicate and reversed arrivals,
immutable accepted snapshots, VCP/site/volume transitions, bounded retirement and `u64` extremes.
A local real WebSocket control sends a missing-VCP block, bounded hole, late fill, duplicate-only
non-rendering input, changed upstream and volume rollover. It also rejects malformed JSON and
bad-checksum metadata without inventing received positions. This is a controlled protocol session,
not an operational outage or independent-redundancy certification.

```sh
cargo test -p wxdata --lib live_sequence -- --test-threads=4
cargo test -p hookecho --lib sequence -- --test-threads=4
cargo test -p hookecho --lib gpu_source_sequence_snapshots -- --ignored --nocapture
cargo test --workspace -- --test-threads=4
cargo clippy --workspace --all-targets -- -D warnings
```

The GPU helper renders the production expanded sequence section with HookEcho fonts and the
Dear ImGui theme at one pixel per point. It uses 240 px touch and 300 px desktop settings for
bounded holes, recovered positions and unavailable evidence. Fresh captures appear in
`target/parity-review/m1.1/source-sequence-ui/`. The helper does not retain adapter/driver identity.
Reviewed references and source/capture hashes are retained in [source-sequence-ui](source-sequence-ui/captures.json).
These expanded-section references do not certify collapse/scroll interactions, whole radar
viewports or physical devices.

The controlled holes fixture receives positions 500 and 503, records one actual failed request
for 501, one failed assembly and one unlocated transport error. The recovered fixture later
receives 501 and 502, then receives 502 again. It retains recovery span 501–502, one duplicate
and one reversed arrival without inventing a 0–499 prefix. These are generic ledger controls,
not a claim that the current relay subscription performs HTTP recovery requests.

Reviewed images: [holes at 240 px](source-sequence-ui/holes-240.png),
[holes at 300 px](source-sequence-ui/holes-300.png),
[recovery at 240 px](source-sequence-ui/recovered-240.png),
[recovery at 300 px](source-sequence-ui/recovered-300.png),
[unavailable at 240 px](source-sequence-ui/unavailable-240.png) and
[unavailable at 300 px](source-sequence-ui/unavailable-300.png).

Final verification passed 2,166 workspace tests with zero failures and 139 explicitly ignored
checks across 28 suites. Native Clippy passed with warnings denied; the WASM library check passed
with existing browser warnings. The explicit GPU helper passed in 3.39 seconds after compilation.
All six fresh images matched their reviewed copies byte-for-byte. Independent Python/PowerShell
checks verified the copied hashes, sizes, dimensions and final source hashes. Detailed results
are recorded in increment 9 of [ROADMAP_PARITY.md](../../../ROADMAP_PARITY.md). Local final logs
are `target/parity-review/m1.1/source-sequences/{workspace,clippy,gpu,wasm}-verified.log`.

## Remaining scope

Per-contributor native pass association and source block-to-radial boundary mapping remain open;
recovered bytes alone do not upgrade the conservative pass association from increment 8.
Persisted progressive replay, emitter epoch/resume negotiation, operational failure-domain sessions,
full application interaction, Android/browser runtime and sustained-load/GPU timing certification
remain open. Tornado detection and its scientific calculations remain Claude's work.
