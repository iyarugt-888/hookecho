# M1.3 increment 2 — Observed recovery and coherent tier transitions

Date: 2026-10-04. Builds on [subscription ownership](live-session.md).

## Production gaps repaired

`SiteProviders::tick` runs each frame; `SiteArbiter::evaluate` previously incremented its recovery
streak each call. One healthy board entry therefore became three observations in three frames.
Failback also lacked the failover direction's chronology guard: a fresh primary older than the
backup could be selected. A backup with failed transport or stale data could qualify merely by
being less old. Finally, an initially empty board was immediately classified as stale enough to
select completed fallback, before its monitor had delivered anything.

The monitor now retains two bounded counters: total advancing source-clock observations and
consecutive advances since its last failed/unexpectedly ended subscription. A success still records
receipt time and lifetime callback success, resets transport failures and clears the old error.
Only a strictly newer source time advances recovery evidence and the freshness high-water mark.
Duplicates, equal-clock updates and older replays cannot create new recovery observations. This is
deliberately conservative when multiple chunks share one upload second; it is not native cut/pass
identity or a claim about radial completeness. Counters saturate, and retained error text is bounded
to 512 Unicode scalars. Retired monitor callbacks cannot update their board.

The arbiter captures the primary's observation baseline at failover and resets it when primary data
is stale, failing or older than the backup. Recovery uses the smaller of new observations since that
baseline and the monitor's post-failure advancing streak. Thus repainting an unchanged board adds
no evidence, and failures hidden between frames still break recovery. Defaults require three
advances; the existing configurable arbiter threshold has a minimum of one. A failover candidate
must have fresh source data, zero current transport failures and satisfy the existing freshness
margin. Failback may use equal source time but cannot select an older primary.

The three-tier manager applies the same observation requirement when leaving completed fallback.
The 180-second degradation threshold remains, while restoration requires data within the 90-second
progressive freshness threshold. Unknown startup data gets up to 180 seconds of monotonic uptime
to report before completed fallback; a known stale source can degrade immediately. The empty-board
window is bounded and does not turn receipt/reachability into source freshness. Existing completed
polling remains available during observation and retry.

Primary/backup board entries are sampled together under one lock. The last transition records only
a change in the actually selected tier; an internal arbiter candidate change while completed fallback
still holds cannot announce recovery early. The existing source-health detail now lists the primary
as a recovery candidate while degraded even when no relay is configured. No painter/layout changed.

An unexpected clean End while the monitor remains wanted now counts as transport loss and resets
recovery evidence. Intentional cancellation does not add a failure. Reconnect delay remains two
seconds but checks cancellation every 250 ms, releasing retired backoff tasks promptly. Healthy
scripted provider controls now remain subscribed until cancellation, distinguishing a healthy
connection from an unexpected End. Provider interfaces, source clocks, accepted receipts, radar
values, strict masks and detector inputs retain their established meanings.

## Deterministic proof

Twelve new controls cover the repaired seams:

| Production seam | Control |
| --- | --- |
| Arbiter | 1,000 unchanged evaluations per recovery snapshot cannot trigger failback |
| Arbiter | Fresh-but-older primary remains on backup until caught up and sufficiently observed |
| Arbiter | A failure between UI samples discards pre-failure recovery evidence |
| Arbiter | Failed and stale backups cannot qualify just by being fresher |
| Provider monitor | Equal/older callbacks preserve the source-time high-water mark and do not advance evidence |
| Provider monitor | Failure resets advances; success clears a bounded error; duplicate success does not establish recovery |
| Provider monitor task | Unexpected clean End is a failure; cancelled reconnect backoff returns within one second |
| Manager | Empty-board grace expires at 180 seconds; completed recovery waits for three advances |
| Manager | Internal candidate switches do not replace the visible degraded transition |
| Manager | Source ages of 118–120 seconds cannot restore; an intervening failure remains visible to the recovery count |
| Source health | Degraded mode lists primary recovery without a configured relay |
| Manager/controller/receiver | Primary loss → relay → both lost → completed floor → primary flap → held recovery → preferred restoration |

The combined scenario uses production `ProviderHealth` recording, manager decision, `LiveSession`
reconciliation/End ownership and actual `MapView`/`LiveScan` acceptance. It asserts fresh replacement
generations, late-End refusal, unchanged completed floor during weak recovery, older-volume refusal,
and an immutable original acquisition receipt. Its source-clock/generation/transition trace is
[fault-transitions.json](restoration/fault-transitions.json), emitted by the passing test with fixed
control clocks. This is synthetic fault evidence, not a sustained operational feed or a complete
`HookEchoApp` event/rendering session. No new progressive network provider or source mixing was added.

Run `cargo test --workspace -- --test-threads=4`, workspace/all-target Clippy with warnings denied,
and the WASM library check with `CARGO_INCREMENTAL=0` and
`RUSTFLAGS=--cfg getrandom_backend="wasm_js"`. Targeted arbiter/integration controls can be run using
`cargo test --workspace failover -- --test-threads=4`. The
[verification manifest](restoration/verification.json) records final results and hashes of source,
logs and the generated trace. Local logs are `target/parity-review/m1.3/restoration/`.

Final Windows workspace verification passed 2,210 tests across 28 suites, with zero failures
and 142 explicitly ignored checks. All twelve new restoration controls passed. Strict workspace
Clippy and the WASM library check passed; the latter retains existing browser warnings. The
shared checkout also contains Claude's concurrent detection/touch work, excluded from this commit.

## Open gates and handoff

The demonstrated frame-count/older-candidate/bootstrap and completed-restoration seams are closed.
M1.3 remains partial. Next: add declared upstream failure-domain identity through comparative health
and retained diagnostics, preserving unknown identity instead of inferring independent redundancy
from transport labels. Then exercise full app delivery/rendering during faults and gather sustained
relay/direct-source sessions with real cut/revisit chronology, loss and restoration under load.
Cross-provider cut continuation, emitter/resume negotiation, complete-pass evidence, configurable
manager thresholds benchmarked against scan cadence, physical Android, browser runtime, presentation
latency and soak certification remain open. Tornado detection stays with Claude.
