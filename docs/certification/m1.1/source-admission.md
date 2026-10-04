# Source volume admission — M1.1 increment 11 / M1.3 foundation

Implementation date: 2026-10-03, following `f90309b`. Tornado detection remains assigned to
Claude. Implementation: implemented. Verification: partial; final commands and evidence below
cover Windows static/unit/controlled GPU checks and browser compilation. Application, device,
operational source failover and sustained-load certification remain open.

## Delivered workflow

Open acquisition details for an accepted progressive frame and expand **Source volume admission**.
The receipt identifies the admitted radar/start clock, optional native number, volume rollovers,
declared upstream resets and refused older/foreign/conflicting envelopes. Selected frame,
local-product and 3D source receipts retain their own evidence. A missing scope is unavailable;
it cannot borrow a previous frame's receipt. Source health and the Analyst log separately show
the last live stream error during recovery. Reconnecting clears that receiver diagnostic.

## Admission and recovery contract

A shared provider-subscription cursor admits only the configured radar. The exact canonical
`VolumeKey` start clock determines rollover; native rotating volume numbers are consistency hints.
A newer clock resets direct/relay assembly even if its native number was reused or wrapped and
no Start marker arrived. A repeated Start in the current identity preserves assembly/receipts.
Same-volume late sequence input remains allowed under existing conservative continuity rules.
An older start or conflicting same-clock native number is refused without rewinding the cursor.

A direct iterator can already have mutated when a refused identifier reaches the consumer.
The subscription therefore returns a readable error and the existing generation-gated application
path resumes completed-volume polling. It cannot silently keep using that iterator's old scope.
The separately fetched initial Start must also match the joined identity. Direct progress now
comes from the newly decoded scan's VCP/native positions, rather than an iterator mapper that can
survive a missing/invalid Start. Undecodable or unchanged input emits no fresh progress; it does
not claim a current cut using a previous volume's VCP. No scientific decoder/merge/gate behavior
was altered. Network scheduling still uses the iterator's existing timing estimates.

Relay refusals happen before sequence history, accumulation, decoded progress or upstream changes.
A delayed older block with a different upstream label cannot reset the current ledger. Accepted
same-volume label changes still reset assembly/pass/message history and are counted separately.
Completed HTTP arrays must contain only the requested radar and one canonical volume/upstream
scope before assembly or the up-to-date shortcut. Foreign/mixed arrays return an error.

Guard counters are totals **since this provider subscription**, including non-rendering input;
reconnect starts new totals. Refused integrity/malformed JSON is handled by existing transport
validation and is not invented as a volume refusal. A relay has no supplied native number.
Labels do not prove emitter epochs or independent failure domains. Admission validates declared
envelope identity, not every native payload radial or complete passes. Refusals do not establish
transport loss. Accepted snapshots remain immutable through later refusals, receiver recovery,
site changes and reconnect. Dynamic radar-string capacity is charged with retained receipt memory.

The last stream error is a bounded receiver diagnostic (512 Unicode scalar values), not a clock
or selected-frame receipt. Superseded stream generations cannot deliver it into the current view.
Completed polling can restore availability while retaining the last live failure explanation;
starting a new live subscription attempt clears it. No settings or wire schema migration is required.

## Controls and review references

- Pure cursor controls: reused/wrapped numbers, clock/day rollover without Start, repeated Start,
  same-volume late input, foreign radar, older input, number conflict and immutable receipts.
- Actual native `ChunkIdentifier` admission control, including a midnight rollover and missing
  Start. Existing raw decoded VCP/progress controls exercise recorded cut metadata.
- Real local WebSocket JSON/integrity/decode/merge path: late fills, duplicates, upstream resets,
  newer volume followed by a delayed older block from another label, and recovery on the current
  volume. No older progress/time or sequence/upstream rewind is accepted.
- Real local HTTP responses with incompatible radar, volume radar, start clock and source label
  rejected before assembly. Existing valid completed response and 404 controls remain.
- Captured scope ownership, mismatched site/clock rejection, missing evidence, conservative
  memory charge, recovery error bounds/clearing and unchanged accepted volume clock.
- Narrow 240 px touch / 300 px desktop admitted/refused/unavailable production detail painting.

Final shared-tree Windows checks passed **2,186 workspace tests**, zero failures and 142 explicit
ignores across 28 suites, workspace/all-target Clippy with warnings denied, and a fresh WASM library
check with existing browser warnings (wxdata 1, hookecho 10). The explicit GPU helper passed in
3.03 seconds after compilation. Commands, exits, counts and log hashes are retained in the
[verification summary](source-admission-ui/verification.json). Local full logs are under
`target/parity-review/m1.1/source-admission/{workspace-final,clippy-final,gpu,wasm-final}.log`.

Six visually reviewed references: [admitted touch](source-admission-ui/admitted-240.png),
[admitted desktop](source-admission-ui/admitted-300.png), [refused touch](source-admission-ui/refused-240.png),
[refused desktop](source-admission-ui/refused-300.png), [unavailable touch](source-admission-ui/unavailable-240.png)
and [unavailable desktop](source-admission-ui/unavailable-300.png). The [capture manifest](source-admission-ui/captures.json)
retains byte counts, dimensions, UTC capture times and hashes for all six images and 22 source files.
Images were copied without pixel edits; unused 900 px canvas space is preserved. Controls use
HookEcho's production painter, fonts and Dear ImGui theme. They do not exercise a complete viewport,
physical device, live click/folding interactions or source-health/log delivery; adapter identity is
not retained by the helper.

The first browser check reused pre-change wxdata metadata and failed with missing APIs without
checking wxdata. Removing only its verified 16-byte WASM fingerprint forced the final fresh check;
native/data caches were preserved. Shared-tree checks include Claude's concurrent detector work,
now in `0018089`, which is excluded from this increment's edits. A final comment correction describes
progress delivery accurately; it does not alter the behaviors covered by the full workspace run.

## Open gates and next work

Canonical cut/revisit continuation, source/emitter epoch negotiation, preferred-source restoration,
persisted progressive replay and actual failure-domain evidence remain open. Arrival-ordered relay
cut IDs cannot substitute for verified native cut chronology. Strict masks still use the established
source-time inference, and detector inputs remain unchanged. This increment does not certify
cross-provider splicing, operational source sessions, complete passes, visible latency, browser
runtime or physical Android. Whole-scan invalidation remains conservative.

Next: unify the legacy `MapView.live_progress` marker with receiver admission. The current
`data_poll` handler writes that marker before `LiveScan::progress` rejects older/invalid metadata;
the frame receipt is protected, but the legacy animation requires the same refusal semantics.
Then capture deterministic provider-loss/polling/restoration transitions against the application
lifecycle, retaining canonical volume/cut identity and original accepted receipts; then add an
operational repeated-cut/VCP-change/SAILS/MRLE session report before changing strict boundaries.
