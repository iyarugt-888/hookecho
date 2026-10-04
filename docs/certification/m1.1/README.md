# M1.1 derived radar coverage increment

Smooth/isosurface increment 4 evidence is in [map-3d.md](map-3d.md), with its own covered
builder controls and reviewed source-coverage captures. Earlier counts below remain historical.

[Raw cut acquisition details](raw-acquisition.md) add independent radial-presence evidence and
shared desktop/phone Analyst Mode inspection. [Accepted frame receipts](frame-acquisition.md)
subsequently bind that evidence to decoded revisions and accepted derived/3D results.
[Source-marked pass history](source-passes.md) then retains native boundary identities when
their radar clocks are known, including repeated elevations and non-rendering delayed input.
Per-contributor pass association, proven transport gaps and persisted progressive replay remain open.
[Discontinuous assembly controls](pass-continuity.md) subsequently prevent known missing or
unordered source input from assigning ordinary positions across an uncertain pass boundary.
[Source sequence receipts](source-sequences.md) then distinguish bounded byte-message holes,
actual failed requests and later arrivals from raw radial and native pass coverage.

The subsequent [standalone 3D increment](standalone-3d.md) has its own scientific controls,
ownership tests, reviewed UI references and remaining-scope record. The [native observed
increment](observed-3d.md) preserves radial clocks/geometry and applies the live policy with
scan-local cut coverage. This page retains the original derived-grid evidence rather than
replacing its historical counts.

These are review references for the Inspector's local radar coverage section, captured on
Windows on 2026-10-02. The renderer uses HookEcho's fonts, Dear ImGui theme and predictable
offscreen egui GPU rendering at one pixel per point. Eight captures cover continuous/strict
policy, 240/300 px widths and desktop/touch settings. They are layout references rather than
universal pixel goldens. The UI capture helper does not retain adapter/driver identity.

[Capture hashes, byte counts and UTC times](coverage-ui/captures.json) accompany the images.
The narrowest reviewed touch references are [continuous](coverage-ui/continuous-240-touch-true.png)
and [strict](coverage-ui/strict-240-touch-true.png). The 300 px desktop references are
[continuous](coverage-ui/continuous-300-touch-false.png) and
[strict](coverage-ui/strict-300-touch-false.png). Labels and values wrap in their own columns;
the source qualifications remain visible without ellipses or hover.

The capture input is a controlled eight-row coverage example, not a real storm or a complete
radar map. Four rows have source times 1,700,000,120,000 through 1,700,000,123,000 ms, two
older rows have times 1,700,000,000,000 and 1,700,000,001,000 ms, one row is empty/untimed,
and one has data with an unknown clock. Continuous coverage retains two older rows and spans
123 seconds. Strict preparation excludes the two older and two untimed rows; its contributing
known-clock span is three seconds. Both policies retain the unknown/unobserved qualifications.
Neither claims a complete column from one available tilt.

Reproduce the captures with:

```sh
cargo test -p hookecho --lib gpu_derived_coverage_snapshots -- --ignored --nocapture
```

Fresh output appears under `target/parity-review/m1.1/coverage-ui/`. Explicit invocation requires
a working GPU adapter; default workspace runs list this check as ignored rather than certifying it.
The passing reviewed capture invocation took 2.76 seconds after compilation.

CPU checks exercise the production temporal mask and all six local integrations, ensuring that
excluded sectors remain NaN rather than becoming zero. They also cover malformed dimensions,
unknown clocks, late gap fill, row reordering, absent upper cuts, accepted scan revisions,
independently acquired panes, strict versus archive/playback policies, both environmental levels,
and neutral retirement of superseded source-health requests. Run them with:

```sh
cargo test -p wxdata --lib level2::temporal
cargo test -p hookecho --lib radar_products
cargo test -p hookecho --lib superseded_selection
cargo test -p hookecho --lib radar_card
cargo test -p hookecho --lib radar_coverage
```

After correcting the narrow layout, Windows workspace tests passed 2,057 tests with zero failures
and 118 explicitly ignored checks, and native Clippy passed. The WASM library check passed in
35.37 seconds with existing browser warnings. An existing local HTTP test initially reset its
connection; it passed the isolated rerun and both subsequent workspace runs without a source
change. Local logs are under `target/parity-review/m1.1/`. Concurrent fusion/backtest edits were
present in the shared checkout at `4a84473` and remain outside this increment.

The runtime frame identity uses weak scan references and is not persisted as scientific source
identity. At this original derived-grid increment, raw cut/pass inventory, proven transport gaps,
per-contributor revision optimization, 3D propagation, full application interaction,
browser/Android runtime and sustained-load evidence remained open. The later increment references
above record delivered 3D propagation, accepted raw receipts and native pass history with their
remaining limits. Scientific strict masks still use the existing 2D source-time gap inference.
Independent local-product textures per pane are a separate M5.1 task.
