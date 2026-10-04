# Native contributor passes — M1.1 increment 10

Implementation date: 2026-10-03. Shared baseline: `8cf7d0a`, following source sequence
receipts in `f334c28`. Tornado detection remains assigned to Claude and is outside this increment.
Implementation: implemented. Verification: partial with the static/unit/GPU/compile checks recorded below;
application, device, operational continuation and sustained-load gates remain open.

## Delivered workflow

Select a progressive radar frame and open the Inspector's local coverage. Recorded pass
boundaries and retained input counts accompany derived grids, native observed sweeps,
smooth volumes and isosurfaces. The standalone Reflectivity volume's source details expose
the same summary. User products retain associations for their contributing plain-moment
inputs before formula evaluation, even though generated output rows have no sole native writer.

A dock gate reading of the selected native moment/tilt input reports its radar and recorded native boundary when established. Pinned
readings preserve their original radar scope. When a plain row writer has an older clock than
the bucket maximum, the dock labels the existing timestamp **Bin maximum** and separately
reports **Native row time** from recorded metadata; it does not silently replace either clock. Unknown clocks, unanchored joins and unavailable
row evidence are explicit. Detail lists are bounded to eight boundaries, with an additional
count; labels and values wrap in narrow desktop/touch docks.

## Meaning and scientific limits

`NativeRadialKey` records elevation number, native azimuth number, acquisition clock and radial
status. It is resolved only against provider evidence retained for the actual accepted merged
scan. The enclosing receipt scopes keys to the subscribed radar and source volume. Neither
angles, local frame counters, clock proximity nor equal elevations establish an association.

The provider records associations before stitching. Discontinuous/coalesced input keeps ordinary
positions unanchored under the existing continuity rules. A later byte arrival does not upgrade
those positions without native boundary evidence. Non-rendering decodes remain in bounded
provider history for the next accepted frame. Merged rows retained from an earlier source context
may have no matching evidence and remain unavailable. Relay envelopes with a foreign radar or
volume radar are discarded before they can change subscription state.

Plain-moment binning records the last eligible radial that actually writes each row. A row's
existing clock is still the maximum bucket clock and can differ from its writer on overlapping,
reversed input. This increment intentionally preserves that clock, all gate values, and existing
scientific behavior. Copy-on-write identities follow only rows rewritten by live incremental
binning; earlier binned clones and accepted receipts remain immutable. The default scientific
binner does not allocate this metadata.

Pass summaries describe **policy-retained input rows**, not winning output cells. Strict mode
still uses the existing source-time gap inference and exact delivered mask; native boundary
identity does not replace it yet. Continuous mode preserves all values. Counts can include
multiple input moments and angular bins covered by one native radial. Pooling, quality masks,
interpolation, range cropping and derived output-cell lineage are separate. Native observed
summaries count recorded input radials rather than regular bins. KDP, dealiased velocity and
formula outputs cannot claim one raw row writer. A missing index or invalid row-metadata length
leaves association unavailable. An unusable native clock never establishes unique identity.

The provider retains at most 92,160 native fingerprints/associations, without gate payloads.
An accepted index contains only matching surviving scan keys; it cannot borrow a known boundary
from another row. Expired evidence becomes unavailable. Receipts charge index capacity, and 3D
cache payloads charge dynamic pass-summary capacity. Shared allocations are conservatively
charged per entry. These are bounds and accounting, not measured performance certification.

## Controls and review references

- Constructed same-elevation revisits and exact status/clock lookup; discontinuous joins;
  bounded eviction with immutable earlier receipts.
- Reversed overlapping radial writers versus unchanged maximum clocks; gate sampling,
  byte-identical legacy output and copy-on-write live updates; dealiased output unavailable.
- Continuous/strict binned and native-observed inputs with unchanged gates, clocks and masks;
  unknown/unmatched identities and malformed metadata fail closed.
- Real local WebSocket delivery through integrity checks, decode, merge and accepted receipts,
  including duplicate non-rendering input, late recovery, upstream/volume changes and foreign
  envelope scopes. This is a transport control, not operational source failover certification.
- Pinned Mayfield partial Level II decode through captured smooth, isosurface and formula
  inputs. Smooth values match the legacy build; formulas retain input summaries while clearing
  output-writer metadata. The fixture is a decoding/ownership control, not complete storm science.
- Narrow 240 px touch and 300 px desktop Inspector layout, bounded boundary lists,
  original pinned radar scope, unknown clocks and unavailable input evidence.

Final serial checks passed: **2,177 workspace tests**, zero failures and 141 explicit ignores
across 28 suites; workspace/all-target Clippy with warnings denied; and the WASM library check
with the existing browser warnings. The explicit GPU helper passed in 3.14 seconds after
compilation. The [durable verification summary](contributor-passes-ui/verification.json) records
commands, exits, counts and log hashes. Local full logs are under
`target/parity-review/m1.1/contributor-passes/{workspace-final,clippy-final,gpu,wasm-final}.log`.

Reviewed captures: [recorded touch](contributor-passes-ui/recorded-240.png),
[recorded desktop](contributor-passes-ui/recorded-300.png),
[unavailable touch](contributor-passes-ui/unavailable-240.png) and
[unavailable desktop](contributor-passes-ui/unavailable-300.png). The
[manifest](contributor-passes-ui/captures.json) retains image hashes, byte counts, dimensions,
UTC capture times and 22 final source hashes. All images were visually reviewed and copied
without pixel edits. Independent final verification checked every source/image digest and byte
count. These controlled cards use production Inspector painting, HookEcho fonts and its ImGui
theme. They do not exercise a whole viewport, live pointer/pin interaction or physical Android;
GPU adapter/driver identity is not retained by the helper. The 1,200 px canvas includes unused
space below the variable-height card.

Initial verification exposed two fixture assumptions: synthetic WebSocket boundary messages
carry no reflectivity gates, and a sampling point exactly on an angular bin boundary is sensitive
to floating-point rounding. Controls now inspect actual accepted native rows and sample inside
the intended bin. No gate algorithm was changed for these corrections. The first browser check
reused stale `wxdata` metadata predating these APIs. Removing only its 16-byte WASM fingerprint
forced a fresh dependency check; the final compilation passed. Native/data caches were preserved.
Claude's concurrent detector commit `c308f50` remains outside this increment's ownership.

## Next work and open gates

Carry canonical source volume/cut continuation through direct and relay lifecycle changes,
then establish safe native-boundary-aware revisit behavior without changing detector inputs.
Mapping source message sequences to individual radial boundaries, emitter epochs/resume,
operational SAILS/MRLE/reordering/failover sessions and persisted progressive replay remain open.
Default archive/completed/reloaded replay inputs have no progressive index and report unavailable.
Whole-scan invalidation remains conservative. Physical Android, browser runtime, whole-application
interaction, completed GPU timing and sustained-load certification are still open.
