# M1.3 — Bound relay connection setup and release retired handshakes

Date: 2026-10-11. Audit baseline: `bea5e673f4936ebb746fc1a023ae559bc00094c8`
on Windows NT 10.0.22631.0. The fetched remote branch matched that commit; no merge
was applied to the shared checkout. Implementation: implemented; verification: partial.

## Refreshed priority audit

| Priority | Current deficiency | Classification and evidence | Next action |
| --- | --- | --- | --- |
| P0 / M1.3 | Relay connection setup has no deadline or retirement checks | Absent in baseline `relay_provider::subscribe`: `connect_async(...).await` precedes all activity checks | This increment: whole-setup deadline, cancellation, socket-release and monitor controls |
| P0 / M1.3 | Failure recovery across independent operational upstreams and whole-app rendering | Partial: subscription generations, recovery counters, TGFTP response deadlines and loopback relay continuation exist; `continuation.md` and `upstream-domains.md` explicitly leave operational topology and live failure sessions open | Add an app delivery/render fault harness; acquire real deployment evidence separately |
| P0 / M1.2 | Actual visible presentation timing | Implemented receipt/decode/build/queue/GPU stages, but scan-out proof is absent; `m1.2/README.md` explicitly excludes visible presentation | Instrument or observe the actual presentation path on documented hardware; do not rename GPU completion as scan-out |
| P1 / M5.1 + storm-case workflow | Independent pane times survive case export/reopen | Partial: workspace keeps time-group identity, but `app/case.rs::capture_case` saves one active-pane time and `open_case` applies it to every pane, including independent groups | Preserve per-pane analysis/replay context with legacy-case compatibility, then exercise archived comparison export/reopen |
| P1 / M3.6 | Complete linked storm measurement | Partial: existing browser ruler walkthrough and regression controls; `m3.6/README.md` leaves endpoint-edit/pick behavior and native/device walkthrough open | Complete a demonstrated missing measurement transition, then capture a native archived workflow |

This prioritizes an actual resource/recovery fault over repeating existing functionality.
The companion [TGFTP recovery](tgftp-recovery.md) and [received-data beam](../m1.1/received-beam.md)
increments remain in place. Renderer soaks and browser captures are useful evidence but do not
close full application, operational failover, Android or presentation gates. Concurrent sounding,
sweep-sharing and detector work is excluded from this increment.

## Before and after

A relay that accepted TCP but did not answer the HTTP WebSocket upgrade could keep a rendering
subscription or health-monitor attempt alive indefinitely. Even after a radar/provider change or
foreground suspension, the task could not inspect `active()` until connection setup completed.
The existing 250 ms checks only covered reads from an already established WebSocket.

Setup now has a **15-second whole-attempt deadline**, covering DNS, TCP, TLS and HTTP upgrade,
and **250 ms retirement checks**. The same connection future is retained between checks;
timer slices do not reconnect. Dropping it releases its transport. Inactive entry returns without
opening a connection. A final activity check covers a ready result/deadline racing with retirement.
Retirement returns `Ok(())`; an active deadline returns `relay connection setup timed out`.
The normal manager/session retry policies remain unchanged. A quiet established WebSocket can
stay open, with the existing read cancellation checks.

No provider trait, saved format, gate values, source clock, receipt inventory, merge behavior,
cut lineage, scientific masks or detector internals changed. The only production path edited
is `crates/hookecho/src/relay_provider.rs`. The transport retains its native-only scope.

## Regression controls

All five new controls use real loopback TCP/WebSocket transport:

- An unanswered upgrade retires cleanly after activity stops; both callbacks stay at zero and
  the server observes EOF/reset, proving socket release rather than only an outer timeout.
- A shortened 150 ms setup deadline returns the expected failure and releases its socket;
  another attempt to the same address completes its upgrade and processes a clean close.
  This proves connection recovery, not receipt of a new meteorological scan.
- An inactive subscription makes no connection at all.
- The original unbounded `connect_async` expression is still pending after an external 150 ms
  deadline on the same fault model. This negative control demonstrates the old deficiency.
- A real `monitor_provider` attempt against an old relay with missing advisory topology and
  a stalled upgrade records one transport failure/reconnect, zero successes/advances and no
  source or receipt clock. No immediate retry connection occurs; retirement ends its cooldown.

The base scan is the committed, partial NOAA KFTG hail fixture documented in
`tgftp-recovery.md` (SHA-256 `ff66e16eade943e9d272aed38bcf0b231c5517fbde0e59953f0b207aec3c303c`).
It supplies a real decoded subscription base; handshake controls supply no new radar observations.
Existing relay controls separately exercise actual wire assembly, received progress, corrupted
and duplicate input, source scope, cross-provider continuation and lagging backup suppression.

## Verification results

The final full Windows workspace suite passed **2,649 tests**, zero failures and **211 ignored**
across 37 suite results. The final relay run passed **21 tests**, including all five new fault
controls. Its executable was taken from that full workspace build, after the cooldown control
was strengthened to keep the monitor wanted during the no-retry observation. Strict workspace /
all-target Clippy passed again on the final source. The WASM library check passed with the
existing one data-library and ten app-library warnings. The owned relay file passes rustfmt;
global formatting reports unrelated differences (for example `app/beam_diagram.rs` and
`app/digest_brief.rs`), which were preserved.

The final cancellation observation was **251 ms** after retirement. The shortened-deadline
attempt took **275 ms**, including fixture decode before the 150 ms connection deadline;
these are single debug loopback observations, not production latency distributions.

[Verification JSON](relay-handshake-verification.json) records source/log hashes, actual exit
statuses and individual cancellation/attempt durations. No GPU was exercised. The source was
tested in the shared checkout with concurrent peer edits; only the owned relay source and this
evidence were committed. Final implementation SHA is recorded in the subsequent committed
certification manifest. Exact changed paths: `crates/hookecho/src/relay_provider.rs`, this
document, `docs/certification/m1.3/relay-handshake-verification.json`, the M1.3 ledger in
`ROADMAP_PARITY.md`, and the generated `docs/certification/{README.md,manifest.json}` index.

## Reproduction and limits

```powershell
cargo test -j 1 -p hookecho --lib relay_provider:: -- --nocapture
cargo clippy -j 1 --workspace --all-targets -- -D warnings
cargo check -j 1 -p hookecho --lib --target wasm32-unknown-unknown
cargo test -j 1 --workspace -- --test-threads=2
rustfmt --edition 2021 --check crates/hookecho/src/relay_provider.rs
```

For a manual fault, point the relay setting to a TCP listener that accepts the request but
never answers the WebSocket upgrade. Setup must report a timeout after its deadline. Changing
radar/provider or suspending activity must release it on the next timer check. These manual
steps have not been exercised in an interactive GUI during this increment.

Loopback tests do not certify Internet DNS/TLS outages, deployment independence, live severe
cadence, full app rendering during a switch, Android lifecycle or visible presentation latency.
The production default is exercised through configuration/code checks; tests shorten its deadline
to avoid adding 15 seconds to each fault control. Reported durations are individual debug-test
observations, not operational percentiles or a hard scheduling guarantee.

Next ready slice: app delivery/render failure sequence with independent provider contexts and
stale-generation injection. Keep the broader `CODEX_GOAL.md` objective active; its analyst,
GIS, independent-pane, presentation and device requirements are not complete.
