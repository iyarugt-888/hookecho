# Pinned scientific inputs

[manifest.json](../../crates/wxdata/tests/data/corpus/manifest.json) is the versioned input
contract for the radar corpus. It records exact archive objects, SHA-256 hashes, sizes,
collection and retrieval times, attribution, transforms, and the checks that consume them.
The initial algorithm baseline is `0fe93de`. Updates to expected science require an explained
review; provisioning never discovers a newer scan or updates a golden.

The source is [NOAA NEXRAD distributed by Unidata on AWS](https://registry.opendata.aws/noaa-nexrad/),
accessed 2026-10-01. NOAA NODD data are openly usable with the attribution requested in that
registry. NOAA endorsement must not be implied. The three committed partial files are
**modified subsets**, not complete, unaltered NOAA volumes. Their original Archive II header
and first three LDM records are copied byte for byte, retaining metadata and 240 observed
azimuth bins. Other azimuths/elevations are unavailable. No end-of-volume record is invented.

## Offline PR checks

The committed subset totals 1,263,075 bytes and includes actual Mayfield, Denver hail, and
clear-air input. These are decoder/value/time/missing-coverage controls; the partial tornado
and hail files do not certify detection of the entire storm. Expected reflectivity hashes and
peak values are recorded outputs from the initial decoder/binner, not independent physical
measurements. Existing full-volume storm tests retain their established scientific thresholds.

```sh
python -m unittest discover -s scripts/corpus -p test_provision.py
python scripts/corpus/provision.py --offline-only --verify-only
cargo test -p wxdata --test scientific_corpus
```

No network, tokens, or GPU are needed. Missing required offline files, unsupported manifest
versions, wrong lengths/checksums, and unknown azimuths being filled with data fail explicitly.
The test decodes each real input independently twice and checks physical-value codes, collection
clocks, missing rows, missing upper elevations, and the recorded baseline.

## Large cached checks

Ten full archive objects (about 112 MB) cover the existing historic tornado, hail, pre-dual-pol,
and convective-line controls plus a clear-air control. They remain out of Git. Python 3.10+
and the standard library suffice; the commands work on Windows and Linux.

```sh
python scripts/corpus/provision.py
python scripts/corpus/provision.py --verify-only
```

Set `HOOKECHO_CORPUS_CACHE` to the **absolute** `target/scientific-corpus` path (or the path
supplied with `--cache`) before the following commands. In PowerShell:

```powershell
$env:HOOKECHO_CORPUS_CACHE = (Resolve-Path target/scientific-corpus).Path
```

In bash:

```sh
export HOOKECHO_CORPUS_CACHE="$PWD/target/scientific-corpus"
```

```sh
cargo test -p wxdata --test scientific_corpus -- --ignored --nocapture
cargo test -p wxdata --test golden_events -- --ignored --nocapture --test-threads=1
```

The explicit large suite fails when its required inputs are absent. A configured cache in
`golden_events` is mandatory and never silently falls back to downloading. Without that variable,
the existing ignored archive tests fetch the same pinned object directly and verify its hash.
Transport errors may be retried; a checksum change is a failure. Downloads use unique temporary
files and publish to the cache only after verification. A corrupt existing cache fails rather
than being overwritten. The scheduled workflow provisions/cache-verifies inputs independently
of provider contract tests.

The derived-product repeatability check uses four original low tilts, with an explicit 240-gate
(about 60 km) range crop to bound computation. It checks composite reflectivity, VIL, VIL density,
and echo top, including missing-cell bits and the retained source time. It establishes deterministic
processing, not full-column accuracy with missing upper tilts.

### Repeated-cut expectation correction

The first pinned full-volume run exposed two stale peak expectations after `8a1d853` changed
`bin_scan_opts` from the first matching cut to the newest matching cut. The exact input objects
were unchanged. Direct traversal of decoded raw reflectivity gates confirmed Moore's first/newest
peaks at **68.5/70.5 dBZ** and Mayfield's first/newest peaks at **66.5/68.0 dBZ**. The corresponding
newest-cut binned peaks are **70.4/67.9 dBZ**, within one quantization step of the raw observations.
Only those two peak goldens changed; tilt counts, tornado locations, confidence, and velocity
acceptance thresholds are retained. `cached_repeated_cut_selection_matches_raw_observations`
checks the first/newest raw peaks and selected output, preserving the reason for this correction.

To reproduce absent small files from verified full parents:

```sh
python scripts/corpus/provision.py --rebuild-offline
```

Both parent and derived checksums must match the existing manifest. The command refuses a
different output. Use a separate review to introduce new source objects or intentional algorithm
expectation changes, with old/new results and a scientific reason; do not regenerate values merely
to make a failing test green.

## Remaining M0.3 certification

Warning/report archive responses used by the four report-association cases and the Moore warning
verification still come from separate network requests. Pinning those truth inputs is the next
increment. The clear-air file is a weak-echo control and a candidate for a ground-clutter case;
independent spatial clutter labeling and a verified QLCS tornado case remain open. This corpus
does not certify all meteorological regimes or replace device, full application, visual/GPU,
or sustained-load evidence. Existing GPU goldens and provider contracts remain separate suites.
