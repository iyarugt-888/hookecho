# M1.3 — Bound a stalled completed-volume fallback

Date: 2026-10-11. Implementation: implemented; verification: partial.
This is one bounded increment under `CODEX_GOAL.md`, not completion of M1.3.

## Audit and selection

Initial local baseline: `8bfe56a5000afb68b3d29f73ce71a85c3b633a83`.
Fetched goal: `CODEX_GOAL.md` from remote `4b464a5dfbc825b21d1182a2bd66147d4684a683`.
Only that document was extracted; no merge or checkout replaced concurrent work.
Claude's commits advanced the shared branch during this run. Verification records the
shared HEAD and the changed source hash; unrelated source changes are excluded from this increment.

| Candidate | Evidence-backed classification | Priority / decision |
| --- | --- | --- |
| Visible presentation latency | Absent observation; receipt-to-queue/GPU traces implemented and measured on Windows/browser in M1.2. These are not scan-out measurements. | P0; needs a compositor or external observation method before certification. |
| Independent-provider recovery | Partial; source topology, arbitration, subscription ownership and loopback continuation already implemented. Existing M1.3 evidence does not establish operational independent failure domains. | P0; preserve those foundations and repair a demonstrated fallback failure. |
| TGFTP stalled-request recovery | Partial at baseline: no headers/body deadline, direct synchronous decode, cancellation checked only before/after a poll. Tests now exercise the repaired transport and subscription. | Selected feasible P0 slice: a stalled last-resort source must not retain a retired subscription. |
| SAILS visit ordering, immutable sweep sharing, tablet controls | Already implemented or under active peer work; M1.1/M1.2 ledgers and shared-tree changes identify ownership. | Do not repeat or claim peer work. |
| Storm-to-radar suitability, independent GOES groups | Partial operator workflows described in the existing roadmaps; not completed by this increment. | Subsequent analyst slices after remaining P0 recovery/presentation work; check peer ownership first. |

This is a focused audit of the immediate assignment, not a new blanket competitor-parity claim.
The initial certification index's soak/failover summaries lag recent ledger entries; use the
dated evidence rather than treating those summaries as missing implementations.

## Operator improvement

The selected degraded TGFTP subscription is used by `app/radar_feed.rs::spawn_stream`
through the existing provider manager. Previously, a server that accepted a connection but
stopped responding could hold the subscription indefinitely. Switching site/provider or
backgrounding set its active predicate false, but the predicate was not checked during the read.

`tgftp_provider.rs` now uses the existing cross-platform `wxdata::task::timeout` around
the entire listing response (15 seconds) and entire volume response (100 seconds), including
headers and body. An HTTP deadline releases the current attempt; the subscription keeps its
existing 60-second retry interval. This avoids a hot retry loop.

During a poll, the subscription races acquisition against its active predicate, checked at
250 ms timer slices. It drops a retired poll and rechecks activity before delivering a ready
reply. Native decode runs through `wxdata::task::blocking`; WASM keeps its existing synchronous
implementation. Cached fallback failures are logged explicitly. The last good source clock and
gates are retained, and no receipt clock, progressive coverage or current acquisition time is invented.

No dependencies, saved formats, renderer, detector or failover decision thresholds changed.

## Reproduction and regression controls

```powershell
cargo test -p hookecho --lib tgftp_provider::tests -- --nocapture
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check -p hookecho --lib --target wasm32-unknown-unknown
rustfmt --edition 2021 --check crates/hookecho/src/tgftp_provider.rs
```

The HTTP controls use in-process Axum servers on loopback, with shortened test deadlines:

- First listing stalls, times out, and the next subscription poll returns a decoded NOAA
  scan through `on_update`. Exactly two polls occur, with the configured retry wait respected.
- A failed first poll with no cache returns an error. A successful subsequent poll recovers;
  a later stalled listing retains the previous clock and gates unchanged.
- Listing and volume bodies stall after headers. Both time out. The baseline unbounded
  request/body expression is a negative control and exceeds the external test deadline.
- Cancellation while waiting for headers or a volume body ends the subscription without any
  late update or fabricated progressive callback.

Recovery data: committed `crates/wxdata/tests/data/corpus/denver-hail-2017-first-records.ar2`,
462,169 bytes, SHA-256 `ff66e16eade943e9d272aed38bcf0b231c5517fbde0e59953f0b207aec3c303c`.
Source: NOAA KFTG, 2017-05-08 20:34:05 UTC, first three records of the source Archive-II object;
the corpus manifest retains source object/checksum and transformation provenance.
This deliberately partial fixture is not a complete TGFTP volume or proof of complete-column science.
Decoded sweeps, binned reflectivity values and radial acquisition times are compared with the
same independently decoded fixture; no scientific golden or detector output is changed.

## Verification

Results: focused controls **15 passed, 0 failed, 1 network test ignored**; strict workspace
Clippy passed; WASM library check passed with the existing 1 wxdata / 10 hookecho warnings;
owned TGFTP rustfmt passed. `cargo test --workspace` failed during parallel linking with
`LNK1102: out of memory` and missing dependent rlib artifacts. The fallback
`cargo test --workspace --lib` passed **2,546 tests, 0 failed, 175 ignored**. Full integration
and binary gates are therefore not claimed. Global rustfmt reports baseline differences;
unrelated files were preserved. This verification was completed before the subsequent beam rework.

One final debug run observed cancellation at **256 ms** waiting for headers and **261 ms**
waiting for a volume body, with zero delivered updates. Timeout → retry → recovered callback
took **373 ms** with shortened 100 ms / 150 ms test deadline/retry settings and exactly two polls.
These are individual control observations, not p50/p95/p99 or live-feed performance claims.
Results and full-log hashes: [verification artifact](tgftp-recovery-verification.json).
Temporary full logs live under `target/parity-review/m1.3/codex-tgftp-*`.
Host: Windows NT 10.0.22631.0, native debug tests and loopback HTTP. These tests exercise no GPU;
no GPU model or driver measurement is claimed. Concurrent peer builds/work shared the machine.

## Remaining gates and next work

- No interactive GUI/provider-switch capture, operational NOAA outage, real independent dual
  upstream session, severe-cadence load or physical Android walkthrough was run.
- GPU completion and visible presentation latency are unchanged and remain separate open gates.
- The 250 ms interval is a scheduling target, not a hard real-time guarantee. Normal between-poll
  cancellation still uses the existing one-second sleep slices.
- Dropping acquisition does not forcibly terminate native blocking decode already in progress;
  that decode can finish, but its retired future cannot deliver a callback. WASM decoding cannot
  be interrupted while it occupies the browser thread.
- Response byte limits, efficient unchanged-volume polling, cache/site identity validation and
  complete GUI recovery measurements are separate follow-ups; this increment does not certify them.
- Next ready slice: audit completed-volume request context across provider/site changes and
  exercise the full delivery/rendering path under injected loss. Presentation observation needs
  a documented hardware-backed method. Keep detector and concurrent 1008/1010 work with Claude.
