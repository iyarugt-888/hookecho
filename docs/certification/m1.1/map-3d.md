# Smooth map volume and isosurface coverage (M1.1 increment 4)

The implementation was committed as `b84d816`; `b265b52` subsequently corrected test-only
Clippy `as_chunks` lints. These records finish that increment's pending evidence ledger and
missing documentation link. Earlier M1.1 evidence remains historical.

## Scientific and ownership controls

Eight new `loop3d` controls plus the Inspector control exercise continuous REF samples against
the direct production grid, strict masking before interpolation/meshing, separate REF quality-mask
clocks for ZDR, multi-moment product coverage and geometry-only formulas before auto-range.
The native sector fixture retains both input gate buffers and acquisition clocks; strict meshes
match meshes built from explicitly prepared inputs. Empty threshold surfaces retain coverage.
Wide source radials can wrap into preceding angular bins: the product control follows their
recorded acquisition clocks rather than assuming a half-plane boundary.

Other controls distinguish independent same-name scans, actual accepted live revisions,
site/policy changes and pane counters. Weak identities do not retain gate buffers. Current,
isosurface and prefetch builds share admission; direct starts cannot bypass it. Late successes
remain under their original playback key and failures cannot poison another policy. Deliveries
with inconsistent policy metadata are rejected. Cache accounting includes allocated payload and
coverage capacity. The Inspector names actual contributing moments and rejects mismatched,
pending or unavailable metadata.

```sh
cargo test -p hookecho --lib loop3d::tests
cargo test -p hookecho --lib map_volume_card
cargo test -p hookecho --lib gpu_map_volume_coverage_snapshots -- --ignored --nocapture
```

## Reviewed layout references

[Capture metadata and SHA-256 hashes](map-volume-ui/captures.json) accompany ten reviewed PNGs.
Five scenarios (continuous, strict, pending, unknown aggregate interval, unavailable result)
are rendered at 240 px touch and 300 px desktop using HookEcho's fonts, Dear ImGui theme and
production Inspector painter, one pixel per point. The explicit Windows GPU invocation passed
in 4.30 seconds after compilation. Adapter/driver identity is not retained by this helper.

Examples: [continuous touch](map-volume-ui/continuous-240.png),
[strict touch](map-volume-ui/strict-240.png), [pending desktop](map-volume-ui/pending-300.png),
[unknown clocks](map-volume-ui/unknown-240.png), [unavailable result](map-volume-ui/empty-240.png).
All values and qualifications wrap within the panel; pending/unavailable states show no stale
frame label or acquisition interval.

The controlled metadata has two contributing moments (REF and ZDR), each with four current
rows, two older rows, one empty untimed row and one populated unknown-clock row. Continuous
retains four older rows and a 123-second span; strict excludes eight rows and reports the
three-second retained interval. The unknown scenario deliberately withholds the aggregate
interval to exercise disclosure; it is not a new scientific acquisition fixture. The source frame
label is also controlled, rather than evidence tying these timestamps to that historic scan.

## Validation and limits

Final compatibility and workspace results are recorded in ROADMAP_PARITY.md after the passing
checks. Local logs and regenerated captures live under `target/parity-review/`.

These are builder/ownership tests and coverage-section GPU references, not full map interaction,
visual proof of every smooth/isosurface viewport, Android/browser runtime, a completed-GPU timing
measurement or sustained-load certification. Continuous numerical behavior is preserved; strict
uses the existing 2D time-gap inference. Native dealiased binning keeps its established behavior.
Binned rows are not native radial counts, expected-cut inventory or proof of complete columns.
Product coverage names the supplied moment contributors; absent moments/upper cuts are not
invented. Scan identity is runtime-only. Persistent pass identities, explicit transport-gap evidence,
finer contributor invalidation and physical-device performance remain open.
