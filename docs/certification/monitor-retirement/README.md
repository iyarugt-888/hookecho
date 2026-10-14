# Provider monitor retirement — bounded L2/E1 increment

Switching panes previously retained the inactive pane’s comparison manager and
base scan. A provider ignoring its cancellation callback could also retain its
monitor indefinitely. Inactive managers now retire at synchronization, including
a switch after the display stream already ended. Monitoring polls cancellation
every 250 ms and drops a pending subscription future. Existing metadata inspection
already has cancellation and a two-second deadline.

`radar_monitors_started` and `radar_monitors_finished` are process counters exported
through existing diagnostics/statistics; their difference counts entered monitor
tasks that have not exited. They do not count sockets or detached provider jobs.

The negative control timed out before the fix. The corrected control requires the
actual async subscription guard to drop and preserves zero failure/reconnect counts
for intentional retirement. The actual application/channel/upload/GPU control
switches to an archive pane after stream retirement, checks that the former manager
is gone and compares its retained rendered frame with the last good frame.

Commands, exit statuses, logs, source hashes and tested executable hash are in
[verification.json](verification.json). Compilation initially failed because C:
ran out of disk space; that failed log is retained. Disabling incremental compilation
allowed the native retry. The workspace library suite passed 2,692 tests (207
ignored); Clippy all targets passed with warnings denied. The WASM library target
passed with nine existing warnings; the initial desktop executable WASM invocation
failed and is retained separately. No source, cache or evidence was deleted.

This is not operational failover or release certification. Blocking synchronous
work may finish after cancellation; kernel socket release and detached jobs are
unmeasured. Full-application 2h/12h/24h profiles, genuine independent provider outages,
visible presentation latency, physical Android and browser/device observation remain
open. The test uses a synthetic pending provider and pinned NOAA replay on Windows,
with an offscreen physical GPU, not a running window or real upstream outage.
