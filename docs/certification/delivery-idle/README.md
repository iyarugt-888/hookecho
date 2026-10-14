# Progressive delivery idle watchdog — 2026-10-14

A progressive subscription could remain open indefinitely without delivering a
usable update or an End event. Its separate observer could keep the provider selected.
The controller now schedules a monotonic 180-second delivery deadline for the known
Unidata and Relay progressive subscriptions. Acceptance of an advancing source clock
renews it. Duplicate/older data and foreign generations do not; metadata/progress
messages alone cannot prove usable delivery. TGFTP completed-volume cadence is excluded.

`manage_stream_at` checks the deadline after reconciling the exact wanted scope.
At expiry it retires the generation through the existing owned End path, records
one loss, marks recovery and requests an immediate frame. The next frame evaluates
provider feedback before choosing a retry. The prior frame stays resident; callbacks
from the retired generation cannot overwrite it. Context changes, pauses, loops and
background suspension use normal reconciliation and start a fresh window on resume.

Source-clock extraction moved to `wxdata::live::update_source_clock` so observed and
accepted native/WASM deliveries use one rule. No source UTC is synthesized from the
monotonic deadline; this does not measure visible presentation.

## Controls and evidence

Two controller controls cover exact deadline boundaries, duplicate/foreign clocks,
genuine source advances, completed-source exclusion, scope mismatches, retirement,
new generation and resume. The existing isolated app/GPU control now invokes the
production lifecycle with an explicit future monotonic instant while no callback/End
is sent: Backup's generation retires, counters become `[1,1,0]`, the last good pixels
remain identical and a late old-generation payload cannot advance revision 3.

The fixture's radar acquisition clocks remain historical and the normal host-clock
pane state remains Stale. The explicit monotonic instant accelerates the lifecycle
control; no real 180-second or long-running test is claimed.

Final shared-tree checks: workspace **library** suites 2,691 passed, zero failures,
207 ignored; workspace/all-target Clippy with warnings denied; WASM library with the
configured getrandom backend (nine existing HookEcho warnings); isolated app/GPU
control. Debug assertions are enabled with PDB emission disabled. The normal preview
executable is untouched. Concurrent linked-storm-pane edits are included in the tested
working tree but are outside this watchdog commit; see source/status hashes in
`verification.json`. Integration/doc suites were not rerun.

Operational independent feeds, deployed browser behavior, physical Android, provider
task/socket resource accounting, measured cadence budgets and full-app 2h/12h/24h
profiles remain open. The fixed deadline is a starting reliability bound, not a
certified latency target or a completed soak.
