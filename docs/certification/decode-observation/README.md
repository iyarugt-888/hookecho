# Decode observations — bounded L3 increment

Relay subscriptions previously assembled and merged real blocks but reported a
hard-coded zero duration. They now measure assembly, source/pass bookkeeping and
merge work on the client monotonic clock. Receipt precedes that work; integrity
checking, transport waiting and application callbacks are outside this measured
assembly interval. It is elapsed work, not a CPU profiler measurement.

`Update.decode_time` and the application delivery channel now carry an optional
measurement. The completed-volume TGFTP adapter has no decode observation, so it
supplies `None`. Synthetic app and GPU controls also omit unmeasured work. The
accepted-revision diagnostics emit null with an explicit unavailable reason; a
measured zero remains distinguishable. The latest pane reading clears on an
unmeasured update. Analyst history retains each arrival’s ingest lag and leaves
unknown decode bars empty instead of inserting zeros or carrying a previous value.
The log labels an unmeasured latest arrival accordingly.

The actual loopback relay/WebSocket regression failed on the old zero. Its measured
relay-only run reported 651 µs work beside a deliberately 200 ms subscriber callback,
which stays outside the measurement. This is one synthetic fixture observation,
not a performance percentile or competitor latency claim. A diagnostics serialization
control distinguishes missing from measured-zero work; fallback timeout/recovery and
actual app/GPU controls verify missing timing through their real delivery paths.

Final workspace library verification passed 2,693 tests (209 ignored); Clippy all
targets passed with warnings denied; WASM library passed with nine existing warnings.
The final relay sample measured 469 µs work beside a 200,469 µs callback. Both affected
GPU controls passed. Final commands, raw logs, exits, build/source hashes and limitations are recorded in
[verification.json](verification.json). The working tree contains concurrent peer
storm-selection work, which is outside this increment’s ownership.

First-byte and visible presentation remain unobserved. No compositor, surface or
scan-out timestamp is substituted by these timings. L3/L4, independent operational
failover, physical platforms and full-app 2h/12h/24h acceptance stay open. No detector,
gate value, formula, source acquisition clock or latency budget is changed.
