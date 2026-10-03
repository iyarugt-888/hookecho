# M1.1 accepted frame acquisition receipts

Open the Inspector and expand **Selected frame acquisition**. Raw arrivals describe the selected
accepted decoded scan/revision, while the Analyst log continues to describe the moving live
receiver. Coverage sections for native observed map sweeps, smooth volumes/user products,
isosurfaces and local radar fields report their accepted source receipt separately from the
product's contributing moments. The standalone 3D reflectivity window also retains the receipt
in **Source and coverage**; previous-grid details keep their own receipt while rebuilding.

## Ownership and missing evidence

The accepted live Update's raw envelope captures a small immutable `AcquisitionSnapshot`.
`Volume` binds it to the actual weak decoded scan identity and accepted revision. Metadata-only
updates with no elevations preserve the last accepted frame; accepted updates without usable raw
evidence clear the receipt. Site changes or direct decoded-scan replacement cannot inherit it.
Receiver progress, gap filling, rollover and reset cannot mutate a retained snapshot.

Observed upload keys, derived delivery keys, smooth/isosurface job/cache keys and standalone
volume delivery keys clone that same receipt. Existing source/revision/policy/control checks
reject obsolete results before displaying their summaries. Late map-worker results may remain
cached under their original key; they cannot satisfy a newer selection. Complete prefetch keys
with unavailable evidence cannot satisfy a raw receipt-bearing source. Valid empty isosurfaces
still retain their accepted coverage and receipt.

Receipt equality is in-memory allocation identity plus source site, not a persistent scientific
pass ID. Clones share a small summary and weak source keys do not pin decoded gate buffers.
Smooth/isosurface byte budgets conservatively charge each entry for the summary and its vector
capacities, even if another entry shares it. These charges are not allocator/GPU/process memory
measurements. This increment changes neither gates nor the continuous/strict preparation policy.

The raw receipt describes accumulated receiver arrivals for its source volume. It is not the
product's contributor ledger: merged older sweeps, interpolation and selected tilts are described
by existing product coverage. Completed/archive inputs and independently reloaded replay frames
retain **unavailable** raw evidence. Live receipts are not serialized or reconstructed from the
current receiver when a timeline selection is reloaded. Existing playback handling discards
mutable live `Volume`s; this increment does not introduce a persistent progressive replay store.

## Controls and reproduction

```sh
cargo test -p hookecho --lib receipt -- --test-threads=4
cargo test -p hookecho --lib selected_frame_acquisition
cargo test -p hookecho --lib radar_coverage_details_fit_narrow_docks
cargo test -p hookecho --lib gpu_frame_acquisition_snapshots -- --ignored --nocapture
cargo test --workspace -- --test-threads=4
cargo clippy --workspace --all-targets -- -D warnings
```

Ownership tests cover rejected metadata-only updates, accepted same-tilt revisions, unavailable
raw inputs, scan/site changes, immutable receipts after gap fill/reset, invalid and old-volume
envelopes, independent receipt identities, complete-prefetch mismatch, late derived/map/standalone
delivery, cache capacity charges and weak source ownership. The real pinned Mayfield partial
Level II fixture exercises decoded source ownership in derived and 3D tests; its raw envelope is
a separate synthetic control, not evidence of historical transport arrivals. Synthetic scans
exercise metadata-only rejection and exact revision behavior. UI controls include accepted and
unavailable evidence at 240 px touch and 300 px desktop, including product summary wrapping.

The GPU helper renders the production selected-frame painter after the receiver fills a gap.
The retained revision still shows radial #3 unobserved, proving it did not consult the moving
receiver. Source start is 1,700,000,000,000 ms, known arrivals are +1 s/+3 s, radial #2 is untimed,
and cuts 2/3 are unobserved. The frame name/revision are controlled labels. Reviewed references
show [accepted touch](frame-acquisition-ui/accepted-240.png),
[accepted desktop](frame-acquisition-ui/accepted-300.png),
[unavailable touch](frame-acquisition-ui/unavailable-240.png) and
[unavailable desktop](frame-acquisition-ui/unavailable-300.png), with a
[hash/size/time/source manifest](frame-acquisition-ui/captures.json).
The helper does not retain adapter/driver identity. It establishes expanded-section layout,
not actual device interaction or a full radar viewport.

Final platform results are recorded in the increment 6 ledger in [ROADMAP_PARITY.md](../../../ROADMAP_PARITY.md).
Build logs live under `target/parity-review/m1.1/frame-acquisition/`; tracked tests and reviewed
capture copies provide durable reproduction outside that ignored directory.

## Remaining gates and handoff

Persistent source-driven cut/pass boundaries, pass history, proven transport-gap evidence,
progressive replay persistence, real mid-volume join/recovery sessions, full viewport interaction,
physical Android/browser runtime and completed-GPU/soak measurements remain open. The existing
rotation-time revisit heuristic is still an inference. Keep unknown clocks, absent sectors and
complete-column uncertainty explicit. Continue with provider-origin pass/transport metadata and
real repeat-cut controls without inferring losses from angular zeros or equal angles. Tornado
detection methodology and its calculations remain Claude's work.
