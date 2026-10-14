# Accepted-revision timing trace — 2026-10-13

Diagnostics export now includes `live_revision_traces`: at most 128 accepted updates
per pane/source timing owner. Each row retains radar, adapter label, source volume
start, accepted revision, and known raw radial acquisition bounds with unknown-clock
counts. Source UTC and client monotonic durations remain separate clock domains.

Receipt-to-acceptance, decode work, receipt-to-queue-writes and receipt-to-GPU-callback
are separate fields. The first observed 2D upload identifies its pane, moment, tilt,
derived product key, column/clean mode and previous visit. It is upload evidence,
not proof that changed gates were visible. Updates with no observed upload stay in
the sample history; an old callback cannot be assigned to a newer or evicted revision.
The existing subscription reset replaces the timing owner, isolating its callbacks.

First-byte UTC and receipt-to-visible-presentation are explicitly null with reasons:
the provider receipt boundary does not expose a first-byte UTC clock, and no compositor
or external display observer is attached. GPU callback timing is an upper-bound
observation taken on a following frame, not scan-out. The existing late-registration
exclusion remains visible rather than entering latency percentiles.

An archive scrub, a loop playhead or a different displayed volume cannot consume a
pending live receipt. Derived product context is retained instead of labelling a UDP
output as its raw input moment. No gates, source clocks, products or detector thresholds
are changed by this instrumentation.

## Verification

See `manifest.json` and the raw logs for the final source/build hashes and exit codes.
The controls cover reordered GPU callbacks, coalesced/unuploaded revisions, bounded
eviction, missing receipt clocks, unknown/invalid radial clocks, archive/loop/volume
ownership, and actual GPU queue/completion ordering using the pinned Mayfield fixture.
The full workspace uses debug assertions with PDB emission disabled on this Windows
host. Clippy and WASM checks run alongside the implementation track.

| Check | Observed result |
| --- | --- |
| Full no-PDB debug workspace tests | Exit 0; application 1,495 passed/102 ignored; wxdata 1,110 passed/99 ignored; all other suites and doc tests passed |
| Final focused controls after retaining only the first repeated callback | Exit 0; four tests passed |
| Workspace/all-target Clippy, warnings denied | Exit 0 |
| WASM library with configured getrandom backend | Exit 0; nine existing warnings |
| Final explicit GPU regression | Exit 0; correlated queue 2,679 µs / GPU callback 5,765 µs, one sample; not a performance distribution or presentation measurement |

The final focused run follows a small first-callback retention guard added after the
full suite; it also recompiles the corrected fixture source-volume timestamp in the
GPU control. No gate values changed. Build/check activity overlapped during validation.

## Remaining L3/L4 acceptance

This is a bounded trace/export increment, not completion of true presentation latency.
Native source-pass/sequence receipt export, first-byte/decode boundary timestamps,
3D and every linked upload, durable sampling beyond the ring, and correlation to an
actual presentation observer remain open. There are no 1,000-sample p50/p95/p99 visible
latency claims. Android physical-device and browser-runtime observations remain open.
Full-application 2h/12h/24h fault/resource runs are also not certified by these tests.
