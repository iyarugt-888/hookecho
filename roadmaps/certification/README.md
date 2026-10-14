# Continuous certification and release evidence

Parent M0.1–M0.3/M7.1–M7.3 and every feature's acceptance criteria.
[Evidence index](../../docs/certification/README.md). **Release not certified.**

| ID | Status | Next action / evidence |
| --- | --- | --- |
| E1 | Partially implemented | Extend the existing soak into the full application: pane/site/product/live/archive changes, layered 3D/GIS/output, accepted revision checks, queue/task/resource accounting and surface/network faults. Do not rename renderer-only tests “full app”. |
| E2 | Certification-only | Run 2h, 12h, 24h profiles on pinned stable increments while A proceeds. 2h/12h/24h historical renderer results remain archived, including the interrupted attempt and recovered KFWS listing failure. Add severe cadence and full UI profiles. |
| E3 | Certification-only | Windows physical GPU, software GPU, WASM/browser smoke, Linux CI and physical Android phone/tablet. Record exact commands/exit codes; missing device or unrun test is open. |
| E4 | Partially implemented | Maintain pinned tornado/hail/QLCS/clutter corpus, scientific goldens for UDP/trails/dealiasing/multi-radar/lineage; extend only where a changed behavior lacks an independent control. Preserve detector research ownership. |
| E5 | Partially implemented | Update evidence manifests, measurements and operator docs with each slice. Keep committed evidence distinct from dirty-tree runs; hash source/build/input/logs. Never refresh old measurements by changing their date. |

## Executable gates

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check -p hookecho --lib --target wasm32-unknown-unknown
python scripts/corpus/provision.py --offline-only --verify-only
```

Use the repository's WASM getrandom configuration and existing Android CI/build
commands. Format only owned Rust files when the workspace formatting baseline is
already dirty. Explicitly invoke affected ignored scientific/GPU controls; a default
test run skips them. Preserve `target/` caches and logs; never `cargo clean`.

For long runs copy the tested executable, record its SHA256 and source state, and
launch with a durable supervisor that records exit code and stdout/stderr. On
Windows use hidden `Start-Process`. Never infer completion from a live process or
elapsed time. At most one resource-comparable measurement per hardware profile;
if runs/builds overlap, disclose contention. A renderer smoke can run alongside
implementation now, but E1 is needed before full-app certification is possible.

Required long-run record: build/scenario/seed, UTC bounds, actual duration, faults,
source clocks and provider transitions, p50/p95/p99 with sample population, RSS/GPU
growth after warmup, outstanding jobs/resources, exit verdict, artifact hashes and
limitations. Missing required observations fail certification rather than defaulting
to zero. Keep proposed budgets in the preserved parity roadmap §5 unchanged.

The ready queue is E1 plus operational L2 and observer L3/L4 acceptance. L1's adapter
regressions and the bounded revision-trace increment have their own dated evidence.
E2–E5 run alongside every stable A increment. No final “certification phase” is scheduled.

[Monitor retirement evidence](../../docs/certification/monitor-retirement/README.md) adds
started/finished process counters and bounded cancellation controls. E1 must sample
these alongside transport/socket and memory observations; short controls do not close E2–E5.
