# M1.4 Tornado ID lineage increment

Every Tornado ID verdict now carries a `DetectionLineage` record
([`wxdata::detection_lineage`](../../../crates/wxdata/src/detection_lineage.rs)). It records:

- which pipeline made the verdict (fused, or the original standing in) and why the original
  stood in when it did
- every stage's algorithm version
- the site and volume
- the acquisition interval of the sweeps the rotation evidence was measured on, from their own
  per-radial clocks
- the same for the debris evidence: the reflectivity and CC tilts the debris signatures were
  read from, and the lowest ZDR sweep that discounts them (added 2026-10-09)

The same record feeds the marker hovers, the local API snapshot and the analysis export
(`provenance.json` and `detections.csv`). In `detections.csv` the debris and rotation rows carry
their own sweeps' interval too, and Tornado ID rows give the debris interval in two columns of
its own after the rotation interval.

## Reviewed reference

[tornado-id-lineage-hover.png](tornado-id-lineage-hover.png) is the hover for the strongest fused
verdict on the pinned Moore 2013 volume, `KTLX20130520_201229_V06` (SHA-256
`e1160f973262663dcb04d70e8fcdfffa2be978327eba87af9972e833c505f100`). It was rendered offscreen
on Windows (RTX 2060/Vulkan); first on 2026-10-02, and again on 2026-10-09 with the debris
inputs. To keep the capture focused, it shows the first two reasons; the app shows all of them.
[tornado-id-lineage.json](tornado-id-lineage.json) is the same verdict's lineage as the exports
write it.

| File | SHA-256 |
| --- | --- |
| tornado-id-lineage-hover.png | `0818bcd57623ef269885adee2023ad5a5bf41778ffc606d9d868e2956645cff4` |
| tornado-id-lineage.json | `12b78b670359a943370fce79ae4d2a31ac65685a8cade767f86807e21615e5dc` |

Reproduce with the corpus provisioned (`scripts/corpus/README.md`):

```
cargo test -p hookecho --lib gpu_tornado_id_lineage_hover -- --ignored
```

The run writes both files to `target/parity-review/m1.4/`.

## What the record shows on Moore

The four input tilts (0.5°, 0.9°, 1.4° and 1.8°, velocity and reflectivity) were scanned between
20:12:47Z and 20:14:20Z. The volume's nominal time is 20:12:29Z. The record keeps the nominal time
and the input clocks as separate fields; neither stands in for the other.

Each tilt took about 14 seconds:

| Tilt | Scanned |
| --- | --- |
| 0.5° | 20:12:47–20:13:01Z |
| 0.9° | 20:13:19–20:13:33Z |
| 1.4° | 20:13:51–20:14:05Z |
| 1.8° | 20:14:06–20:14:20Z |

Of the 5,760 input rows, none is from a previous antenna pass and none has data without a clock.
The 1.8° velocity and reflectivity sweeps each have 66 empty, untimed rows (132 in all). These are
counted as unobserved. Binned data cannot tell a sector that was not scanned from a gap, so they
are not called either one.

### Debris inputs

The debris evidence was read from the same four tilts and scanned 20:12:29–20:14:20Z, starting
18 seconds before the rotation evidence. Its reflectivity and CC were not scanned together:

| Tilt | Reflectivity | CC |
| --- | --- | --- |
| 0.5° | 20:12:47–20:13:01Z | 20:12:29–20:12:46Z |
| 0.9° | 20:13:19–20:13:33Z | 20:13:01–20:13:18Z |
| 1.4° | 20:13:51–20:14:05Z | 20:13:33–20:13:50Z |
| 1.8° | 20:14:06–20:14:20Z | 20:14:06–20:14:20Z |

At 0.5°, 0.9° and 1.4° the volume scans each elevation twice. The surveillance cut carries
reflectivity and the dual-pol moments; the Doppler cut a few seconds later carries reflectivity
and velocity. The app bins each moment from the newest cut that carries it
(`level2::newest_moment_sweep`), so the debris detector pairs CC from the surveillance cut with
reflectivity from the Doppler cut, one antenna rotation (about 17 seconds) later. At 1.8° one cut
carries both. The record shows this as it is and does not change it. Whether the debris detector
should take reflectivity from the cut its CC came from is a detection question. Answering it
needs a backtest, because the measured detection results were made with the current pairing.
The lowest ZDR sweep (0.5°, from the surveillance cut, 20:12:29–20:12:46Z) is the ninth input.
None of the 6,480 debris input rows is from a previous antenna pass, has data without a clock,
or is unobserved.

## Rules the tests hold

- **Input clocks** come from the sweeps' own `bin_time_ms`, through
  `level2::temporal::prepare`. They use the continuous policy, which is how the detectors read.
- **Unknown clocks:**
  - A row with data but no clock is counted as unknown.
  - An empty row without a clock is counted as unobserved.
  - A sweep with no clocks has no interval; the record never invents one.
- **Inputs not recorded:**
  - The original pipeline records the clocks of the same four lowest tilts its couplets read.
    When it has none for the volume shown, they are reported as "not recorded" (blank in the
    CSV, `null` in JSON), never borrowed from the volume time.
  - A malformed sweep yields no record rather than a partial one.

## Not established

- **Earlier passes' debris.** The fused pipeline also reads each earlier low-level pass's own
  debris signatures for tracking. The record covers the volume's newest pass only, which both
  pipelines read the same way.
- **Other layers.** Warnings and observations do not carry this record yet. The rotation and
  debris layers carry it only in `detections.csv`, not in their own hovers.
- **Platforms.** Android and browser runtime, and full application interaction, are not
  certified.
