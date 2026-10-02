# Pinned scientific inputs

[manifest.json](../../crates/wxdata/tests/data/corpus/manifest.json) is the versioned input
contract for the radar corpus. It records exact archive objects, SHA-256 hashes, sizes,
collection and retrieval times, attribution, transforms, and the checks that consume them.
Version 2 added required warning/report truth snapshots. Version 3 adds original NWS damage
track KMZ files with source-checked time intervals, evidence notes, and vertex counts; older
schema readers reject it explicitly.
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

Eight committed GeoJSON snapshots (320,333 bytes) preserve the original IEM HTTP response bytes
for two Moore warning instants and six report windows, including the cross-midnight QLCS window.
Exact request times, retrieval clocks,
checksums, and collection counts accompany them. [IEM's published terms](https://mesonet.agron.iastate.edu/disclaimer.php)
permit reuse of its public-domain materials; attribution to Iowa State University's Iowa
Environmental Mesonet is retained. Snapshot files disable Git line-ending conversion so Windows
and Linux verify the same bytes. Their generated-at clock describes response generation, not
the historical observation time.

No network, tokens, or GPU are needed. Missing required offline files, unsupported manifest
versions, wrong lengths/checksums, and unknown azimuths being filled with data fail explicitly.
The test decodes each real input independently twice and checks physical-value codes, collection
clocks, missing rows, missing upper elevations, and the recorded baseline.

## Large cached checks

Eleven full archive objects (about 126 MB) cover the existing historic tornado, hail, pre-dual-pol,
and convective-line controls plus clear-air and QLCS controls. They remain out of Git. Python 3.10+
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
of provider contract tests. All nine historic checks use the committed truth snapshots and
run without any network access when their radar inputs are cached. Existing live provider
contracts continue exercising the IEM services separately.

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

## Reviewing a truth snapshot update

The IEM services can revise historic data and include response-generation metadata, so a fresh
response is a new candidate even when the observation content is unchanged. Restore a missing
certified snapshot from Git. Provisioning never refetches a replacement or blesses a new hash.

```sh
python scripts/corpus/capture_truth.py
```

This captures the same requests into `target/parity-review/truth-candidates` and writes a
candidate report with old/new hashes and feature counts. It refuses the certified corpus as
an output directory and leaves the manifest and goldens unchanged. Review source-object changes,
geometry, individual report validity, and their effect on scientific results before updating
the tracked response and manifest together. A hash difference alone is not a reason to update.

## QLCS damage-track baseline

The [NWS Des Moines event review](https://www.weather.gov/dmx/StormyandWindyWednesdayDecember152021)
documents twin circulations near Knierim and Somers at 23:44 UTC on December 15, 2021.
Two original linked KMZ files (17,665 bytes) preserve their analyzed damage paths. Source comments
identify emergency-management evidence for Knierim and report-based analysis without a field
survey for Somers. These limitations remain in the manifest; the review labels its data preliminary.
[NWS's later QLCS overview](https://www.weather.gov/media/dmx/Newsletters/September2024.pdf)
classifies this outbreak as a QLCS event. NOAA attribution and
[NWS reuse terms](https://www.weather.gov/disclaimer) accompany the files.

The production KMZ importer and an independent Python XML/ZIP reader both check the original
geometry, source event names, start/end times, and evidence notes. Damaged files, changed metadata,
or absent tracks fail. Provisioning verifies committed KMZ bytes and never downloads replacements.
The new detector check assigns each circulation to its nearest active path within the existing
8 km tolerance; one merged detection cannot count for both tracks. It checks path proximity,
not exact tornado position at a second: the source vertices have no individual timestamps.

The baseline retains a Debris-tier Knierim detection **1.32 km** from its path and a distinct
Possible Somers candidate **7.87 km** from its path. Somers' stronger-tier detection remains
an open detector science gap. The archived LSR window contains four tornado point reports but none
within 8 km of these two candidates. Its failed point-report association is retained in the local
review log; switching to attributed damage paths corrects the truth source, without changing
detector thresholds or expanding the location tolerance.

## Remaining M0.3 certification

Warning/report archive responses used by the four report-association cases and the Moore warning
verification are now pinned. The clear-air file is a weak-echo control and a candidate for a ground-clutter case;
independent spatial clutter labeling remains open. QLCS path-proximity regressions now exist,
with stronger Somers classification explicitly open. This corpus
does not certify all meteorological regimes or replace device, full application, visual/GPU,
or sustained-load evidence. Existing GPU goldens and provider contracts remain separate suites.
