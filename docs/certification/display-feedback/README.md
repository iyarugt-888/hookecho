# Displayed-subscription failure feedback — 2026-10-14 UTC

The failover manager previously received only independent observer-subscription
health. The subscription feeding the pane could end repeatedly while its observer
continued advancing. The pane said Recovering, but automatic selection could remain
on Primary indefinitely. The before-control reproduces this: three owned losses,
both observers advancing, expected Backup, actual Primary.

The current subscription's owned end now reports a loss to its site's manager.
Failure counters are separate from observer measurements and bounded to the three
provider tiers. The effective decision uses the larger failure streak rather than
adding observer and display errors as duplicate failures. A failed active delivery
is compared using the pane's accepted source-clock floor, so an observer frame never
delivered to the pane cannot disqualify a fresher backup.

An advancing accepted update clears only its actual provider's loss streak. Duplicate
or older source clocks do not claim recovery. An inactive candidate may establish
readiness through a fresh observer advance after its last delivery loss; the existing
three-observation failback hysteresis still applies. Without a qualifying progressive
backup, three owned losses permit completed fallback with a TransportError reason.
Manual overrides remain explicit. Source-health details export the pending loss
counts separately, labelled as awaiting source advance.

## Ownership and source controls

- Current pane, subscription generation, radar and normalized relay endpoint must match.
  Retired generations, changed radar/relay, intentional stops and archive/loop pauses
  cannot penalize another owner or clear its failures.
- The initial accepted floor comes only from native clocks in a base scan bearing
  the requested radar identifier. A foreign/unknown radar cannot lend its clock.
- A candidate older than the accepted source floor cannot take over. Observer
  measurements retain their own clocks and errors; they are never rewritten as
  displayed-delivery evidence.
- Observed and accepted paths use the same raw native-clock extraction rule from
  the preceding source-clock increment. No client receipt substitutes for source UTC.

The six new regression controls include the controller→manager failure and recovery
path, six invalid ownership cases, duplicate accepted clocks, manual overrides,
older/unconfigured backup rejection, non-additive failure counts, and the pinned NOAA
same-radar clock floor. The controlled clocks are historical evaluation inputs,
not claims that archived radar is fresh at today's host time.

## Actual application channel and GPU control

The existing isolated app control now attaches a manager for KFTG and sends owned
Primary/Backup end events through the real message channel. Primary loss records
`[1,0,0]`; a late Primary end leaves that unchanged. Backup loss records `[1,1,0]`;
an accepted Backup update with newer native clocks clears only Backup to `[1,0,0]`.
Archive and sibling pixels remain protected, and live resume succeeds. Its updated
`app-observations.json` includes per-frame delivery counters and accepted-revision
diagnostics. Storage stays under `target/`; public HTTP is denied by a loopback proxy.

This is controlled direct-message delivery and offscreen rendering, not independent
operational upstream failover or compositor/display presentation. The mock manager's
observers in this app capture remain unconfigured; healthy observers are exercised
by the separate controller→manager regression.

## Verification and open gates

`verification.json` pins source, input, binary and raw log hashes. Final workspace
library tests passed **2,689**, zero failures, 206 explicit ignores across seven
library suites. All-target workspace Clippy with warnings denied, WASM library with
the configured getrandom backend, and the isolated app/GPU control passed. WASM
retains nine existing HookEcho warnings. Debug assertions remain enabled with PDB
emission disabled; separate integration/doc-test suites were not rerun in this slice.

The initial build/control mistakes are retained alongside the actual before-behavior
failure and successful final checks. No existing preview process was interrupted.
Operational independent feeds, silent consumer stalls without an end event, real
presentation observation, physical platforms and full-app long runs remain open.
