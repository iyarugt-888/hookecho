# M1.4 Tornado ID lineage increment

Every Tornado ID verdict now carries a `DetectionLineage` record
([`wxdata::detection_lineage`](../../../crates/wxdata/src/detection_lineage.rs)). It records:

- which pipeline made the verdict (fused, or the original standing in) and why the original
  stood in when it did
- every stage's algorithm version
- the site and volume
- the acquisition interval of the sweeps the rotation evidence was measured on, from their own
  per-radial clocks

The same record feeds the marker hovers, the local API snapshot and the analysis export
(`provenance.json` and `detections.csv`).

## Reviewed reference

[tornado-id-lineage-hover.png](tornado-id-lineage-hover.png) is the hover for the strongest fused
verdict on the pinned Moore 2013 volume, `KTLX20130520_201229_V06` (SHA-256
`e1160f973262663dcb04d70e8fcdfffa2be978327eba87af9972e833c505f100`). It was rendered offscreen
on Windows (RTX 2060/Vulkan) on 2026-10-02. To keep the capture focused, it shows the first two
reasons; the app shows all of them.
[tornado-id-lineage.json](tornado-id-lineage.json) is the same verdict's lineage as the exports
write it.

| File | SHA-256 |
| --- | --- |
| tornado-id-lineage-hover.png | `f089e08f7920e0b9f1cac6eee3b4c984d32943ccf0d342518e23cab97a5e81ce` |
| tornado-id-lineage.json | `94f542e91b5df07e2fb2615db63474ec4b49c364ce620f541f7848b9ae6f88d7` |

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

- **Debris inputs.** The CC tilts behind the debris evidence are named by their algorithm
  version, not by their own clocks.
- **Cell dock.** It does not show lineage yet. The hovers and the pinned circulation card do.
- **Platforms.** Android and browser runtime, and full application interaction, are not
  certified.
