# Progress marker admission — M1.1 increment 12

Implementation date: 2026-10-04. Baseline: `29b8dfa`. Implementation: implemented.
Verification: partial; static/unit and WASM results follow below. Full application, device,
operational repeated-cut sessions and sustained-load gates remain open. Tornado detection
remains Claude's work and is outside this increment.

## Delivered behavior

The live sweep bar and follow-sweep marker read the same admitted selection as acquisition
inventory. The message handler's existing current-view/site/generation checks precede delivery.
The pane then calls `MapView::observe_live_progress`; it cannot assign arbitrary incoming
metadata to its animation before the receiver validates it.

`LiveScan::progress` reports whether it accepted an envelope. Older known volume clocks,
out-of-bounds cut/chunk counts and non-finite geometry return false without changing receiver
inventory, transport freshness or recovery. `observe_radials` uses that same gate before
raw positions and captured receipts can change. Invalid geometry is unavailable rather than
invented as an angle, rate or azimuth. No source clock is substituted.

Accepted late cuts/chunks can fill acquisition inventory gaps while the existing monotonic
receiver selection keeps the current marker. The pane copies that selection only when its
metadata changes; duplicate or inventory-only arrivals preserve the animation's original
monotonic receipt stamp. Valid new volume/VCP metadata and the existing supplemental-kind
reset update marker/time together. Same-angle native revisits with indistinguishable metadata
still require native-boundary lifecycle integration; no new identity is guessed from angles,
clock proximity or local counters. This is metadata display admission, not gate validation.

No scientific decoder, gate values, source acquisition clocks, strict/continuous masks, detector
inputs, settings or persisted formats changed. Receipt and UTC clocks remain separate. This
increment does not reset animation merely because local wall time moved.

## Controls and verification

The controls use the actual `MapView`, camera and receiver delivery helper without a GPU/app
shell. A missing first-cut chunk is filled after a second-cut marker is established; inventory
advances while marker/time remain fixed. Duplicate input cannot restart motion. A delayed older
volume, out-of-bounds shape and NaN/infinite geometry leave marker, time, inventory and freshness
unchanged. Newer declared volume and changed VCP advance/reset the admitted selection. A
supplemental-kind reset uses the existing native metadata contract. A refused raw envelope
cannot rewrite an immutable accepted snapshot or clear a recovery error/state.

Final shared-tree Windows checks passed **2,189 workspace tests**, zero failures and 142 explicit
ignores across 28 suites; workspace/all-target Clippy with warnings denied; and a fresh WASM library
check with existing browser warnings (wxdata 1, hookecho 10). The three new delivery/ownership
controls passed. Commands, exits, test counts, eight final source hashes and log hashes are retained
in the [verification summary](progress-admission/verification.json). Local logs are
`target/parity-review/m1.1/progress-admission/{workspace-final,clippy-final,wasm-final}.log`.

The first browser check again reused stale wxdata metadata without checking that package; removing
only its verified 16-byte WASM fingerprint forced the fresh final check. Native/data caches were
preserved. Claude's concurrent detector/documentation work is outside this increment's edits.
No painter/layout changed;
these are state/delivery controls, not screenshot or whole-application interaction certification.

## Next work and open gates

Follow-up — 2026-10-04: [M1.3 increment 1](../m1.3/live-session.md) closes the retry/context,
generation and idle relay retirement seams in the original handoff below. Provider-manager/app
restoration scenarios and operational evidence remain open; this report's manifest is historical.

The identified legacy marker seam is closed. Canonical cut/revisit continuation, retry scope,
actual failure-domain identity, preferred-tier restoration and operational source sessions remain
open. Audit `manage_stream`'s global `last_stream_attempt` retry gate when a stream already ended
before a site/provider/configuration change; a new context must not inherit an unrelated failure's
throttle. Preserve retry bounds for the same context and stop superseded work.

Then exercise provider-loss/polling/restoration through the application lifecycle, retain source
scope/immutable receipts and obtain repeated-cut/VCP-change/SAILS/MRLE operational evidence.
Do not replace inferred strict masks until the native boundary mapping has that evidence.
Physical Android, browser runtime, full viewport interaction, completed GPU timing and soak
certification remain open.
