# Completed-volume fallback admission — 2026-10-13

Bounded L1 implementation verified on Windows in the shared, dirty working tree
based on `bdb4809c`. See `manifest.json` for exact source, fixture, executable and
log hashes. This is not independent-provider operational failover certification.

The live NOAA test first exposed a newest KTLX object still being published (2,343
bytes, no sweeps). A larger prefix can decode useful gates without being a finished
scan. The adapter now requires the decoded source end-of-scan marker, tries at most
three recent site-qualified objects within one aggregate download/decode deadline,
and retains the same site's last good scan on failure. End-of-scan does not prove
complete spatial coverage.

Unchanged name/size listings reuse the decoded scan without another volume request.
A foreign site, misleading decoded radar identifier, or regressed directory cannot
replace the retained same-site data. Source clocks and gates remain unchanged.
The pinned completed NOAA KFTG fixture and the existing partial Denver hail fixture
exercise successful recovery and refusal independently of the live service.

| Check | Result |
| --- | --- |
| `cargo test --workspace --config profile.test.debug=0 --config profile.dev.debug=0` | Exit 0; application 1,491 passed, 102 ignored; wxdata 1,110 passed, 99 ignored; all workspace suites/doc tests passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0 |
| WASM library check with `--cfg getrandom_backend="wasm_js"` | Exit 0; existing warnings retained |
| Explicit real NOAA TGFTP test, same no-PDB debug test executable | Exit 0; one successful fetch/decode, 14.97 s |
| Explicit GPU queue/completion regression, same executable | Exit 0; one sample 2,293 µs queue / 7,149 µs GPU completion; builds overlapped |
| Offline corpus integrity verification | Exit 0 |

The first default workspace build failed with Windows PDB linker error LNK1318.
A release attempt then exposed an existing test that expects a debug-only assertion.
The successful no-PDB debug run preserves debug assertions; neither failure was
hidden by weakening a test. Initial Clippy found `chunks_exact_to_as_chunks`
iteration advice in the concurrent delivery fault test; its iteration was
changed to `as_chunks::<4>().0.iter()` without changing the assertions. The entire
peer test file is not part of this slice. Workspace formatting already has unrelated
diffs; the changed provider file was formatted directly.

## Open acceptance

- L2: cut the actual primary mid-volume and observe a genuinely independent live
  provider, preferred recovery, duplicate/reordered delivery and site/archive changes
  in the running application. A successful standalone TGFTP fetch cannot close it.
- L3/L4: revision-correlated exported timing and visible presentation observations.
  GPU completion above is not compositor presentation or display scan-out.
- Android physical phone/tablet, browser runtime, software-renderer and full-app
  2h/12h/24h fault/resource profiles remain open. No attached Android device was
  available. Historical renderer soaks retain their original narrower scope.

Raw logs include negative discovery and unsuccessful build attempts. The manifest
is a dirty-tree record, not a claim that `bdb4809c` alone contains this implementation.
