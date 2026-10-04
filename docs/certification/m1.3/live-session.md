# M1.3 increment 1 — Scoped live retries and subscription retirement

Date: 2026-10-04. Continues the retry/lifecycle handoff from
[M1.1 progress admission](../m1.1/progress-admission.md).

## Demonstrated gaps and behavior

The app kept a single `last_stream_attempt` after a subscription ended. A new pane, radar or
provider could inherit the unrelated attempt's remaining 60-second delay. A same-context retry
also reused its generation, and the active comparison did not include the selected relay endpoint.
Changing that endpoint could rebuild the health monitor while leaving the old subscription alive.
Finally, an idle relay read waited for traffic before noticing cancellation.

`app::live_session::LiveSession` now retains at most one running scope and one last-attempt scope.
The key is pane, radar, provider label and selected relay endpoint. Endpoint comparison trims
whitespace and trailing slashes, following provider construction. An inactive backup setting is
not part of the primary subscription key. Monitor configuration uses the same normalization,
avoiding a health-board restart on an equivalent endpoint edit. The controller retains no decoded payloads.

Same-context failures retain the existing 60-second interval measured from the attempt with a
monotonic clock. A different context starts immediately, including after End removed the active
handle. Every attempt has a fresh local cancellation generation. Retiring a session invalidates
its token before replacement data can be admitted; the delivery handler checks the active scope's
generation. This token is transport ownership, not a persisted emitter epoch or source identity.

End events retire only their matching subscription. Receiver cleanup/error delivery additionally
requires the pane selection and receiver radar to match the original subscription. A late old End
cannot clear a replacement's marker, retries or health, or write an old radar error into a newly
selected radar. Removed panes are safe. Accepted acquisition snapshots and source volume clocks
stay with their existing owners.

`manage_stream` uses the controller for startup, cancellation and retry scheduling, and requests a
repaint when the remaining retry interval expires. Background suspension drops the active monitor
and intentional pause/end clears failure throttling, so foreground resume can refresh immediately.
Context changes reset the new source's latency sample owner and record a reason in source health
and the log. Existing provider-tier transition reasons and completed-volume polling remain in use.
The controller's new logs do not include relay endpoint values.

The native relay keeps its pending read future alive while checking cancellation every 250 ms.
Cancellation releases even a quiet socket, and a frame becoming ready concurrently is checked
again before decode/delivery. This bounds idle-read cancellation checks; it does not claim a bound
for connection setup, CPU decoding, arbitrary network operations or completed-volume polling.

## Reproduction and evidence

Six new controller/receiver tests are in `crates/hookecho/src/app/live_session.rs`:

| Control | Required result |
| --- | --- |
| Same-source loss and retry; replacement before retry | Preserve delay only for the failed context; reject old End; allocate a fresh retry generation |
| Active relay endpoint change | Stop the old token and start the new endpoint; equivalent normalized endpoints do not churn |
| Already-ended radar, pane, provider or endpoint change | Start immediately without inheriting another scope's throttle |
| Intentional pause/background end and resume | Retire old token; resume without failure throttling |
| Actual receiver loss, completed poll and restoration | Retain source clocks and frozen acquisition receipt; reject older volume and late End; clear only matching markers/errors |
| Reselected/reset/removed pane | Retire the original token without updating another radar's receiver |

The existing real local WebSocket delivery test now awaits graceful cancellation within two
seconds while the server remains open without another ingest. It previously aborted the task,
which masked the idle-read leak. Its delivered scan/progress/source-pass assertions remain.

Run the full shared workspace with `cargo test --workspace`; the controller tests are named
`app::live_session::tests::*` and the socket control is
`relay_provider::integration_tests::subscribe_receives_live_update_and_progress_over_a_real_websocket`.
Also run workspace/all-target Clippy with warnings denied and the WASM library check using
`CARGO_INCREMENTAL=0` and `RUSTFLAGS=--cfg getrandom_backend="wasm_js"`.
The [verification manifest](live-session/verification.json) records the final commands, results,
source hashes and local log hashes under `target/parity-review/m1.3/live-session/`.

Final shared-tree Windows verification passed **2,195 workspace tests**, zero failures and 142
explicit ignores across 28 suites; workspace/all-target Clippy with warnings denied; and the WASM
library check with existing warnings (wxdata 1, hookecho 10). All six new lifecycle controls and
the strengthened local WebSocket control passed. No native/data cache was removed. Initial focused
package attempts reused obsolete wxdata metadata; validation used the current workspace feature
set. Failed attempt logs and the successful pre-normalization workspace run remain in the manifest
alongside the final run. Claude's concurrent detector/documentation files are outside these edits.

## Remaining work and agent handoff

This closes the scoped retry/generation and idle relay retirement seams. The receiver control
exercises production controller, pane and receiver methods, not a complete `HookEchoApp` runtime.
No painter/layout changed. It does not certify arbiter-driven preferred-tier restoration under
load, cross-provider cut continuation, independent failure domains, operational live sessions,
browser runtime, physical Android, presentation latency or sustained load.

Next: exercise the existing arbiter/provider manager through loss, stale-but-responsive primary,
backup selection, completed polling and preferred-tier restoration with the app lifecycle. Retain
canonical source admission, frame-owned receipts and bounded generation filters. Establish actual
upstream failure-domain identity before labeling paths independent. Then obtain repeated-cut,
VCP-change, SAILS/MRLE and mid-volume join evidence before replacing inferred strict boundaries.
Tornado detection remains Claude's assignment.
