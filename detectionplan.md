# Detection Improvement Roadmap

Target branch: `feat/wsv3-redesign`

Primary code areas:
- `crates/wxdata/src/rotation.rs`
- `crates/wxdata/src/tds.rs`
- `crates/wxdata/src/tornado_id.rs`
- `crates/wxdata/src/detverify.rs`
- `crates/hookecho/src/app/detectors.rs`
- `crates/hookecho/src/app.rs`
- `crates/hookecho/src/settings.rs`

## Progress

- **Phase 0, done** (`d82778b`, `05d01e2`). `hookecho --headless-backtest-file docs/backtest-events.txt --export DIR` writes `candidates.csv` and `summary.json`; two consecutive runs are byte-identical (3021 candidates). Soundings are cached and named per event in the summary, because a failed sounding download silently changed every hail candidate.
- **Phase 1, done** (`d1ce635`, and the UI rename after Phase 7). Tornado ID reads raw detector scores and fuses them once as named terms that sum exactly to the score. `COLLOCATION_SHARE = 1.0`: 0.5 cut false alarms scoring ≥ 0.9 from 7 to 1 but dropped the Washington, IL tornado (KILX 2013-11-17) out of Likely. Every detector score a person sees reads out of 100 (`wxdata::evidence::out_of_100`), never as a percentage. That covers the map markers and hovers, the alert banners, the Cell dock, the AI storm brief, the score breakdowns, and the Detectors sliders ("Minimum score … /100", checked in a 280 px capture).
- **Phase 2, field done** (`azshear.rs`). LLSD AzShear with a 2.5 km × 750 m physical kernel and a Tukey-biweighted fit started from the median plane. Huber weighting was tried and fails on a single bad radial once the kernel is three radials wide (beyond ~90 km): it read half the radial's step as 0.014 s⁻¹. Nothing consumes the field yet. Lowest-tilt results: Moore 2013 peak cyclonic 0.039 s⁻¹, Mayfield 2021 0.028 s⁻¹, 35–51 ms per tilt in release. The clear-air control has 0.19% of gates at |shear| ≥ 0.006 before any screening.

- **Phase 3, objects done** (`rotation_objects.rs`). Same-sense connected objects on the polar grid, with hysteresis (seed 0.006 s⁻¹, grow 0.004) and the plan's object properties. Every object is kept and its artifacts are named: too small (also under half the kernel's footprint), single radial, elongated, ragged (fills < 30% of its length × width), fold seam, poor fit, not significant, flank. Gates need storm context in their kernel: ≥ 25% of it at ≥ 20 dBZ.
  - Lowest tilt, pinned corpus: Moore has a credible 0.039 s⁻¹ cyclonic object at the tornado (significance 39.5). Mayfield has credible 0.019 and 0.017 s⁻¹ objects beside it. The clear-air control has no credible object.
  - Side by side in the backtest export (`rotation_llsd`, lowest tilt, 15–150 km, unscored): 2366 candidates vs the legacy detector's 663. At matched POD (~0.6) the false-alarm ratio is similar (0.68 vs 0.66), but there are 9.8 false alarms per radar-hour against 5.9. At the strongest end the LLSD objects keep POD 0.59 where the legacy score keeps 0.06. The existing detectors' output is unchanged, row for row.

- **Phase 4, columns done** (`rotation_columns.rs`). Credible objects are associated up through the tilts by complete linkage: compatible with *every* member, within 3 km + 1 km per km of height, same sense, shear within 6×, one object per tilt. Columns report base and top height, depth, rooted, strongest shear in the 0–2, 2–3 and 3–6 km layers, height-integrated shear, and lean (only when at least 0.5 km deep). `columns_with_support` lets a column take, on a tilt it is missing, an object found without the echo screen (flagged `NoEcho`), marked `weak_echo`. Support never starts a column.
  - Pinned corpus: Moore is a rooted cyclonic column through 4 tilts (0.3–0.9 km, 0.039 s⁻¹). Mayfield's rooted cyclonic column (0.5–1.4 km, 0.032 s⁻¹) needs a weak-echo member. Its strongest column (0.041 s⁻¹ at 0.7–0.9 km) stays unrooted, because the lowest-tilt circulation is in pieces 3–4 km apart.
  - Backtest (`rotation_llsd` is now one candidate per column over the lowest four velocity tilts). At peak AzShear ≥ 0.018 s⁻¹, rooted, ≥ 2 tilts and cyclonic: 117 candidates, 46 verified, precision 0.39, 7.2 false per radar-hour, 7/9 events. Legacy at score ≥ 0.5: 88, 30, 0.34, 5.9 per hour, 7/9. With weak-echo support: 192, 71, 0.37, 12.3 per hour, 7/9. Rooting, depth and sense each raise precision at every shear cut.

- **Phase 5, tracking done** (`rotation_tracks.rs`). `Tracker::update` continues columns from volume to volume by gated nearest neighbour, greedy by cost. The cost is distance from the track's motion-predicted position, plus shear and depth dissimilarity. Sense must match. Reach is 3 km from the prediction, or 3 km plus 35 m/s of travel for a track with no motion yet. Bounded: 12 points per track, and a track ends after 12 minutes unmatched. Each tracked column reports track id, age in volumes and seconds, motion, position jump from the prediction, and least-squares trends per 10 minutes in peak shear, low-level shear and depth.
  - Backtest: median track speeds of 11–18 m/s per event, and the longest tracks span 6–8 of each event's 8 volumes. Cyclonic, rooted, ≥ 2 tilts, ≥ 0.018 s⁻¹: any age 192 / 71 verified / 12.3 false per hour / 7 of 9 events; seen in 2+ volumes 115 / 45 / 7.1 / 7 of 9; 3+ volumes 52 / 18 / 3.5 / 6 of 9. Persistence roughly halves false alarms per hour but barely moves precision (0.37 → 0.39), because many false alarms persist too (non-tornadic mesocyclones). The legacy score ≥ 0.6 tier is still the most precise (0.50, 2.2 per hour, 6 of 9). Persistence has to be fused with the other evidence (Phase 7), not used as a filter on its own.

- **Phase 6, classification done** (`debris_class.rs`). Each `tds` signature is classified using its own polarimetric evidence (the existing fuzzy CC/Z/ZDR/size/contrast/range terms, `raw_confidence`), the volume's LLSD columns and hail cores:
  - **Polarimetric anomaly:** evidence under 0.5, or hail-like with no rotation.
  - **Debris candidate:** credible, but no rotation beside it.
  - **Tornado debris signature:** credible, with a cyclonic column of ≥ 0.01 s⁻¹ low-level shear within 3 km plus two beam widths.

  Hail signs are ZDR ≤ −0.5 dB and a POSH ≥ 50% core within 3 km. Very high reflectivity is deliberately not a hail sign, because verified debris had higher peak Z (median 61 dBZ) than the Denver hail anomalies (53).
  - Backtest:

    | Subset | Candidates | Verified | Precision | False per radar-hour | Events | Denver hail |
    |---|---|---|---|---|---|---|
    | Tornado debris signature | 52 | 28 | 0.54 | 2.4 | 6 of 9 | 2 |
    | Same, rooted | 39 | 24 | 0.62 | 1.5 | 6 of 9 | 2 |
    | Legacy debris, display ≥ 0.5 | 90 | 29 | 0.32 | 6.2 | 6 of 9 | 5 |
    | App's Tornado ID "Tornado debris" tier | 55 | 27 | 0.49 | 2.8 | 6 of 9 | 0 |
    | Debris candidates | 106 | 5 | 0.05 | — | — | — |

    Unrotated low-CC signatures almost never verify.

- **Phase 7 and Phase 8 stage A, the fusion layer exists; not promoted** (`tornado_fusion.rs`, `scripts/fusion/fit.py`).
  - **Features.** 12 per tracked LLSD column, each counted once: low-level and peak shear, depth, tilts, rooted, cyclonic, persistence, shear trend, a collocated tornado debris signature's polarimetric evidence, its hail signs, range, and weak-echo rooting. Reports and warnings are not features.
  - **Score.** The logistic of a sum of named log-odds terms, so the explanation reproduces it exactly. It is an evidence score, never a probability.
  - **Fit.** L2 logistic regression held out by whole event (leave-one-event-out). A free fit learned corpus quirks: range negative and largest; hail beside debris *for* a tornado; correlated shear features with opposite signs. The shipped fit holds each weight to its physical sign and leaves range out. Shear trend, hail and weak-echo rooting were dropped for coming out wrong-signed; weak-echo rooting wanted a *positive* weight, worth revisiting with more events.
  - **Held out by event.** Verified rows in the top 50, 100 and 200 (of 7783):

    | Ranking | Top 50 | Top 100 | Top 200 | AUC |
    |---|---|---|---|---|
    | Constrained fusion | 29 | 59 | 102 | 0.68 |
    | Peak shear alone | 26 | 46 | 85 | 0.70 |
    | Free fit | 23 | 50 | 99 | 0.73 |

    The fusion's top 50 reach 5 of 8 tornadic events, against 7 for peak shear.
  - **In-sample against the app's Tornado ID, at matched false alarms per radar-hour.**

    | Matched rate | Fusion | Tornado ID |
    |---|---|---|
    | ~1.6 per hour | POD 0.33, FAR 0.28, CSI 0.25 | POD 0.45, FAR 0.39, CSI 0.34 |
    | ~4–5 per hour | POD 0.47–0.51, FAR 0.33–0.37 | POD 0.63, FAR 0.56 |

    The fusion's detections are more often right, but they cover fewer distinct tornadoes and its CSI is worse. Per Phase 13 it is not promoted; the app keeps its Tornado ID.

- **Phase 11, verification matrix done** (`detbaseline.rs`, `detverify::lead_minutes`). Every threshold row now has POD, FAR, CSI, precision, recall, F1, false alarms per radar-hour and per volume, and median lead time with its count. The range bands are the plan's 0–30, 30–60, 60–100, 100–150 and 150+ km; with the beam-height bands they report POD, FAR and CSI at 0.5, each band's detections scored against the event's whole truth. Storm type is not broken down: the corpus has no storm-type labels.
- **Phase 10, path truth done** (`detverify::PathTruth`). A surveyed track is matched along its path, not at its midpoint: a detection verifies within the radius of the stretch the tornado can have been on at that time, moving at 8–30 m/s from its surveyed start (the survey has no end time), widened by the window. A path is one event however long. The backtest console tables keep the midpoints; the export uses paths.
  - Effect on this corpus: Tornado ID ≥ 0.6 goes from 33 to 35 verified (POD 0.63 → 0.67, FAR 0.56 → 0.53). LLSD columns ≥ 0.5 go from 367 to 411 verified, and their 0–30 km FAR from 0.41 to 0.28. About 40 detections on surveyed paths, away from the midpoints, had been counted as false.

- **Phases 12 and 13, analyst preview done** (`llsd_analyst.rs`, the Detectors setting "LLSD rotation (analyst preview)", off by default). `rotation_columns::from_sweeps` is the shared per-volume pipeline the app and the backtest both call. The app tracks columns forward in time, classifies the debris beside them and fuses each one. It draws a cyan ring with a headline, and the hover gives every member's measurements and QC, the track, the debris and every evidence term (checked in a GPU capture on Moore 2013). It raises no alert and feeds nothing else.
- **Phase 9, corpus doubled to 16 events** (`docs/backtest-events.txt`, each checked against the IEM LSR archive). The new events are a night outbreak (Dayton 2019), a violent far-range tornado (Rolling Fork 2023), an outbreak (Easter 2020), two tropical cases (Ida 2021, Harvey 2017), a giant-hail supercell (Denton 2016) and a clear-air morning.
  - **Detection on the new events.** The LLSD columns find tornadoes the legacy detectors miss entirely: Dayton 5 of 6 verified, Easter 5 of 9, both with nothing from legacy. Rolling Fork 15 of 16. Clear air: nothing from any detector.
  - **Debris on 16 events.**

    | Subset | Candidates | Verified | Precision | False per hour | Events |
    |---|---|---|---|---|---|
    | Tornado debris signature | 66 | 43 | 0.65 | 1.4 | 9 of 16 |
    | Same, rooted | 49 | 35 | 0.71 | 0.9 | 9 of 16 |
    | Legacy debris, display ≥ 0.5 | 102 | 41 | 0.40 | 3.8 | 8 of 16 |
    | Tornado ID debris tier | 64 | 34 | 0.53 | 1.9 | 8 of 16 |

  - **Hail.** The giant-hail supercells still promote 8 false signatures (Denton 6, Denver 2). Every one is marginal on both axes: a promoting column of 0.010–0.013 s⁻¹ against a verified median of 0.02, polarimetric evidence 0.53–0.59 against 0.74, and mostly one tilt. Promoting only at ≥ 0.015 s⁻¹ would leave 2 of them and keep 39 of 43 verified in the same 9 events. Measured but not adopted: two hail events are too few to set it by, and the fused score below already rejects them.
  - **Fusion refit (fusion-2, step-shaped persistence).** Held out by event, its top 50, 100 and 200 rows hold 31, 72 and 125 verified, against 29, 49 and 93 for peak shear. The weights barely moved from the 9-event fit, and 3+ volumes now weighs more than 2.
  - **Against Tornado ID at matched false alarms (in-sample).**

    | Matched rate | Fusion | Tornado ID |
    |---|---|---|
    | ~2.7 per hour | ≥ 0.5: POD 0.48, FAR 0.27, CSI 0.33 | ≥ 0.6: POD 0.49, FAR 0.50, CSI 0.34 |
    | ~1.1–1.4 per hour | ≥ 0.7: POD 0.36, FAR 0.24 | ≥ 0.7: POD 0.39, FAR 0.35 |

    FAR at 0.5 beyond 60 km is 0.24 and 0.09 for the fusion, against 0.68 and 0.76 for Tornado ID. At ≥ 0.7 the fusion has no detection in any hard-negative event. Same POD with half the false-alarm ratio is the plan's goal. Promotion is a user-visible change and is left to a decision (Phase 13).

- **Validation round before promotion** (decision: validate more first). The corpus is now 21 events, with five more hard negatives: giant hail at San Marcos 2021, Omaha 2014 and St. Louis 2012, wind-farm clutter around Dodge City on a quiet night, and a bird-migration night at Houston. The export records which reports and paths each candidate matched (`matched_truths`) and each event's truth counts, so `fit.py` can verify held-out scores at the report level.
  - **Hard negatives.** Across the nine, the app's Tornado ID at ≥ 0.6 makes 41 false detections: 14 on the wind-farm night and 16 in the Omaha hailstorm. Fusion at ≥ 0.7 makes 2. Migration and clear air produce nothing from any detector.
  - **Held out by event, report level, against Tornado ID at matched false-alarm rates.**

    | Matched rate | Fusion | Tornado ID |
    |---|---|---|
    | ~1.5 per hour | ≥ 0.6: POD 0.36, FAR 0.32, CSI 0.27 | ≥ 0.7: POD 0.39, FAR 0.47, CSI 0.30 |
    | ~3 per hour | ≥ 0.5: POD 0.37, FAR 0.38 | ≥ 0.6: POD 0.49, FAR 0.64 |

    Out of sample the fusion is more often right but finds fewer tornadoes. The in-sample POD parity was optimistic. **Not promoted:** the plan does not accept a lower FAR that loses tornadoes.
  - **Hail.** With the hail cases, hail beside debris fits at −1.05, the sign physics expects (on 9 events it fitted positive and was dropped). The shipped weights are this 21-event fit.

### What stands between the fusion and promotion

- **POD.** The fusion only knows LLSD columns. Tornado ID also finds tornadoes from legacy couplets the LLSD columns do not reach, so look at which reports Tornado ID finds and the fusion misses (`matched_truths` names them) before adding features.
- **Wind-farm clutter.** It fools every detector here (Tornado ID 14 false on one quiet night). Low CC and noisy velocity in clutter fixed in place are what a clutter or persistence-of-position feature would catch.
- **Calibration (Phase 14).** It still waits on a much larger corpus: 21 events cannot calibrate a probability.

### Findings to carry into Phase 9 (from 10 and 11)

- Lead times measure a median of only 3–5 minutes because every event starts a few minutes before its first report. Measuring real lead time needs events that start earlier.
- Tornado ID candidates carry no beam height, so its beam-height bands are all "unknown". The 100–150 km band is weak for every detector (Tornado ID FAR 0.95 at 0.5).
- Truth coverage is uneven. Events without a survey only have point reports, so an unmatched detection there is less certainly false than one near a surveyed path.

### Findings to carry into Phase 8 stage B / Phase 9

- The corpus is the limit. Nine events (one hail and one derecho hard negative) cannot settle weights. On the KDVN derecho a free fit ranks *worse than random* (AUC 0.40). Phase 9's hard negatives and more tornadic cases come before any further tuning.
- The fusion concentrates on the strongest circulations. To compete on POD it needs evidence for weaker tornadoes, or the plan's "1 volume neutral, 2 meaningful" persistence shape rather than a linear term.

### Findings to carry into Phase 7

- Denver's hail supercell still promotes 2 signatures, because it rotated within the radius. Telling it apart needs fused evidence (hail signs, depth, persistence), not a stricter debris rule fitted to one case.
- Verified debris on this corpus has a median ZDR of +1.2 dB, not the ~0 dB often quoted. The mean is taken over a disk that includes the rain around the ball, so ZDR is a weak discriminant here except when strongly negative.

### Findings to carry into Phase 5

- One tornado's low-level circulation can be several objects 3–4 km apart at one tilt (Mayfield), so one column per tornado per volume is not guaranteed. Tracking should associate columns, not assume one.
- Weak-echo rooting finds more real rooted columns but also more false ones. Phase 7 should weigh `weak_echo` members rather than count them as equal to members with echo.

### Findings carried into Phase 4

- Velocity noise far out makes 0.01–0.017 s⁻¹ slopes in nine-sample kernels. What separates it is significance against noise estimated over a fixed window, not the object's own few gates (which select low texture). Pure ±12 m/s noise: 11 436 objects, none credible.
- AzShear is not vorticity: a vortex's outer flow gives opposite-sense shear on both flanks, about half the core's peak. These flanks are flagged and must not become anticyclonic rotation.
- A real couplet can sit right at the Nyquist limit. A fold is told apart by crossing most kernel rows (seam 1.0, still 0.66 where it ends; Moore 0.31), not by its velocities alone.
- Mayfield's strongest lowest-tilt gate (0.028 s⁻¹) is over 3–12 dBZ at the storm's edge, with no 20 dBZ in its kernel, and is excluded. Vertical association (the same circulation over echo one tilt up) is where it could be recovered.
- Line-shaped shear dominates the remaining false alarms (the KDVN derecho alone: 439 objects). Elongation flags long lines, but the fragments of a broken line are compact.

### Findings carried into Phase 3

- Real violent tornadoes have *high* fit RMSE (Moore 14 m/s) and texture (19 m/s) at their peak gate, and Moore's peak gate has a 17% leftover-fold share. Quality gates must therefore be relative to the shear's own size, and fold flags must count only alongside other evidence. Absolute RMSE or texture limits would reject the strongest tornadoes.
- Clear air produces isolated gates past 0.006 s⁻¹. The reflectivity requirement and coherent-object area are needed, not optional.

## Purpose

This roadmap is intended for Claude Code and Codex to implement a substantial reduction in high-confidence false tornado, rotation, and debris detections without simply raising thresholds until true events disappear.

The current detectors contain useful safeguards, but the overall architecture still has three major weaknesses:

1. Rotation is primarily derived from adjacent gate-to-gate peak velocity differences. This is sensitive to isolated bad gates, dealiasing errors, velocity texture, coarse sampling, and fixed-grid clustering.
2. Rotation and debris scores are cross-corroborated and then combined again by Tornado ID, which can double-count correlated evidence and inflate moderate signals into apparently high-confidence tornado detections.
3. Temporal persistence, object motion, storm-relative structure, and robust neighborhood statistics are not first-class inputs to tornado inference.

The goal is not to eliminate all false positives. The goal is to make high scores difficult to obtain unless multiple physically consistent lines of evidence support the same persistent low-level circulation.

---

# Guiding principles

## 1. Candidate generation is not classification

Separate the system into:

```
Radar QC
  -> candidate fields
  -> physical objects
  -> vertical association
  -> temporal tracking
  -> feature fusion
  -> tornado classification / probability
```

Do not allow early-stage candidate scores to masquerade as final tornado confidence.

## 2. Every source of evidence is counted once

Rotation, CC, ZDR, reflectivity, vertical structure, persistence, reports, and warning metadata must each enter fusion only once.

Do not boost rotation with debris, boost debris with rotation, and then combine the boosted values again.

## 3. Prefer robust spatial statistics to maxima

A single maximum gate-to-gate value must not dominate a high-confidence decision.

Use neighborhood medians, percentiles, coherent object area, spatial gradients, texture, persistence, and vertical consistency.

## 4. Scores are not probabilities unless calibrated

Until a detector is calibrated against held-out historical events, label its value as an "evidence score", not a percentage probability.

## 5. Hard negatives matter as much as tornadoes

The validation corpus must contain large hail, QLCS shear, gust fronts, clear-air artifacts, dealiasing failures, sidelobes, wind farms, melting layers, biological scatter, and non-tornadic supercells.

---

# Phase 0 - Preserve a reproducible baseline

Before changing detector behavior, create a baseline so every later change can be compared objectively.

## Tasks

- Preserve current algorithm versions and current detector output on the existing archived corpus.
- Extend the headless backtest to export machine-readable JSON or CSV containing every candidate, not only summary metrics.
- Include:
  - radar site
  - volume timestamp
  - detector type
  - lon/lat
  - range
  - beam height
  - raw score
  - final displayed score
  - gate count
  - tilt count
  - Vrot
  - gate-to-gate delta
  - min/mean CC
  - mean/max reflectivity
  - ZDR
  - vertical depth
  - rooted state
  - confirmation state
  - whether matched to truth
- Save baseline aggregate statistics for:
  - POD
  - FAR
  - CSI
  - false detections per radar-hour
  - detections per volume
  - score reliability by score bin
  - performance by range band
  - performance by beam-height band

## Files

- `crates/wxdata/src/detverify.rs`
- headless/backtest CLI code
- `docs/backtest-events.txt`

## Acceptance criteria

No detector behavior changes in this phase.

A command must reproduce the same baseline summary on repeated runs.

---

# Phase 1 - Fix score semantics and double counting

This phase should happen before replacing the detector itself.

## Problem

Current flow can effectively do:

```
raw rotation
  -> boosted by debris

raw debris
  -> boosted by rotation

then Tornado ID:
  1 - (1 - boosted_rotation) * (1 - boosted_debris)
```

This treats highly correlated evidence as if it were independent and can inflate moderate detections into 80-95% scores.

## Required redesign

Introduce immutable raw detector evidence.

Suggested structures:

```rust
pub struct RotationEvidence {
    pub raw_score: f32,
    // physical measurements...
}

pub struct DebrisEvidence {
    pub raw_score: f32,
    // physical measurements...
}

pub struct TornadoEvidence {
    pub rotation: Option<RotationEvidence>,
    pub debris: Option<DebrisEvidence>,
    // later: temporal, environmental, reports, etc.
}
```

The individual detector modules may report their own evidence quality, but they must not mutate each other.

Remove bidirectional confidence boosting from the production tornado-classification path.

If cross-corroboration remains for display compatibility, it must be clearly separated from raw evidence and must never feed Tornado ID.

## Tornado ID changes

Remove the noisy-OR combination of already-correlated detector scores.

Initially replace it with a transparent rule-based fusion score while the new detector is being built.

Example concept only:

```
rotation_quality
+ low_level_rooting
+ vertical_continuity
+ temporal_persistence
+ debris_consistency
+ spatial_collocation
- QC penalties
- hail-like penalties
- implausible-motion penalties
```

Do not copy these exact weights without backtesting.

## Rename score presentation

Change user-facing text from:

```
78% confidence
```

to something such as:

```
Evidence score: 78/100
```

until calibration exists.

## Tests

Add tests proving:

- debris cannot increase a rotation raw score
- rotation cannot increase a debris raw score
- Tornado ID sees raw evidence
- adding the same evidence through two paths cannot increase the final score twice
- duplicate nearby detections cannot increase confidence merely because there are more copies of the same object

## Files

- `crates/wxdata/src/rotation.rs`
- `crates/wxdata/src/tds.rs`
- `crates/wxdata/src/tornado_id.rs`
- `crates/hookecho/src/app/detectors.rs`
- marker/tooltip code displaying confidence

---

# Phase 2 - Replace peak gate-to-gate rotation detection with an LLSD/AzShear field

This is the highest-priority detector redesign.

The present adjacent-gate peak-difference approach is fundamentally sensitive to individual bad gates and sampling geometry.

Implement a local linear least-squares derivative style azimuthal shear field similar in concept to modern NSSL/MRMS approaches.

Reference concepts:
- NSSL mesocyclone / tornado algorithm work
- MRMS low-level and mid-level AzShear
- LLSD velocity derivative techniques
- TORP object-based rotation detection

Useful references:
- https://www.nssl.noaa.gov/research/tornadoes/
- https://www.nssl.noaa.gov/education/svrwx101/tornadoes/detection/
- https://repository.library.noaa.gov/view/noaa/48189

## LLSD field

For each valid velocity gate:

1. Gather a local polar neighborhood.
2. Reject invalid/range-folded/QC-failed samples.
3. Fit the local radial-velocity field using robust or weighted least squares.
4. Estimate azimuthal derivative / shear.
5. Normalize the result into physically meaningful units.
6. Store:
   - signed shear
   - absolute shear
   - sample count
   - fit residual
   - local velocity texture
   - QC flag

Do not use only two adjacent gates.

## Adaptive neighborhood

The physical width represented by an azimuth bin grows strongly with range.

The LLSD neighborhood should be based on approximate physical distance, not a fixed number of azimuth bins across all ranges.

Candidate neighborhood target should be configurable and backtested.

## Robust fitting

Investigate:

- median/MAD outlier rejection
- Huber weighting
- residual clipping
- minimum valid-neighbor count

A single velocity outlier should not create a strong shear maximum.

## Quality fields

Every AzShear gate/object should carry QC information.

Suggested values:

```rust
pub struct ShearQuality {
    pub valid_fraction: f32,
    pub velocity_texture_ms: f32,
    pub fit_rmse_ms: f32,
    pub dealias_suspect_fraction: f32,
    pub temporal_consistency: Option<f32>,
}
```

High shear with terrible fit quality must be penalized heavily or rejected.

---

# Phase 3 - Create connected rotation objects

Do not cluster candidate pairs by a fixed 0.04-degree geographic grid.

Create a shear mask and connected objects directly from the AzShear field.

## Candidate mask

Candidate membership should use a combination of:

- minimum absolute AzShear
- minimum valid-neighbor count
- acceptable LLSD fit quality
- reflectivity / storm-context requirement
- range/beam-height restrictions

Use hysteresis if useful:

```
strong seed threshold
+
weaker continuation threshold
```

This prevents weak isolated noise from becoming objects while allowing coherent circulation footprints to retain their full structure.

## Object properties

Create a new object type, for example:

```rust
pub struct RotationObject {
    pub lon: f64,
    pub lat: f64,
    pub area_km2: f32,
    pub diameter_km: f32,

    pub max_azshear: f32,
    pub p90_azshear: f32,
    pub median_azshear: f32,

    pub robust_delta_v_ms: f32,
    pub max_delta_v_ms: f32,

    pub mean_texture_ms: f32,
    pub fit_rmse_ms: f32,
    pub valid_fraction: f32,

    pub range_km: f32,
    pub beam_height_km: f32,
    pub elevation_deg: f32,

    pub sense: Sense,
}
```

Use maximum shear only as one feature.

## Reject obvious artifacts

Explicitly test and penalize:

- one-gate spikes
- one-radial streaks
- broad radial seams
- Nyquist-boundary artifacts
- implausibly elongated radial structures
- isolated features with no coherent neighborhood
- high texture / poor LLSD fit

Retain the good existing seam and leftover-fold safeguards where they remain useful.

---

# Phase 4 - Redesign vertical association

Current vertical association can potentially gain confidence through single-linkage chaining.

Replace that behavior.

## Vertical association rules

Associate objects across elevation angles using:

- distance from a common core/centroid
- overlap or nearest-edge distance
- expected storm tilt with height
- maximum allowed displacement per kilometer of height
- rotation sense consistency
- comparable AzShear structure

Do not allow:

```
A near B
B near C
therefore A == C
```

unless A and C are also physically consistent with a common vertical circulation.

## Analyze physical height, not only tilt count

Build vertical summaries by AGL layer.

At minimum:

```
0-2 km AGL low-level rotation
2-3 km transition
3-6 km AGL mid-level rotation
```

Properties should include:

- lowest detected height
- strongest low-level shear
- strongest midlevel shear
- depth
- centroid tilt with height
- vertically integrated rotation evidence
- whether the circulation reaches the lowest usable radar sample

A circulation at four high tilts is not equivalent to a circulation rooted below 1 km.

---

# Phase 5 - Add temporal object tracking

Temporal persistence must become detector evidence, not merely a UI sparkline.

Implement a persistent track type.

```rust
pub struct RotationTrack {
    pub id: u64,
    pub age_volumes: usize,
    pub age_seconds: i64,
    pub history: Vec<RotationTrackPoint>,
    pub motion_u_ms: f32,
    pub motion_v_ms: f32,
    pub trend: RotationTrend,
}
```

## Association

Associate objects between volumes with a motion-aware cost using:

- predicted position
- spatial distance
- shear similarity
- area similarity
- sense
- vertical structure
- parent reflectivity-cell motion if available

Hungarian matching is preferred if candidate counts stay manageable. A gated nearest-neighbor solution is acceptable initially.

## Temporal features

Expose:

- age
- volumes persisted
- position jump
- motion consistency
- AzShear trend
- Vrot trend
- low-level depth trend
- vertical-depth trend
- debris appearance time relative to rotation

## False-positive handling

One-volume objects should normally have limited confidence unless evidence is extreme.

Do not hard-require two volumes for all tornado detections, because rapidly developing tornadoes exist.

Instead create a temporal term such as:

```
1 volume      neutral/limited
2 volumes     meaningful persistence
3+ volumes    strong persistence
```

with an escape path for an exceptionally strong, clean, low-level circulation with collocated debris.

---

# Phase 6 - Redesign TDS as a polarimetric object detector

The current TDS module already has useful connected-component logic, contrast, size, vertical continuity, range effects, and ZDR.

Keep those strengths, but change the semantics.

## Split two concepts

Introduce:

```
PolarimetricAnomaly
TornadoDebrisSignature
```

A low-CC/high-Z object without credible nearby low-level circulation is a polarimetric anomaly or debris candidate, not automatically a Tornado Debris Signature.

## Fuzzy polarimetric evidence

Avoid making the detector depend on only:

```
CC < 0.80
Z >= 40 dBZ
```

Use continuous evidence functions.

Potential inputs:

- min CC
- mean CC
- p10 / p25 CC
- CC contrast to surroundings
- reflectivity mean/max
- ZDR mean/median
- ZDR variance
- area
- shape compactness
- vertical depth
- low-level rooting
- range/beam height
- persistence

## Hail discrimination

Add explicit hail-like features where possible:

- very high reflectivity
- hail/MESH/MEHS context if already available
- broad low-CC region
- positive/high ZDR pattern
- lack of collocated low-level rotation
- storm-relative placement
- melting-layer / bright-band proximity

Do not allow a high-quality hail signature with no rotation to become an 80% "TDS".

## TDS promotion

Promote a polarimetric anomaly to `TornadoDebrisSignature` when:

- the polarimetric signature itself is credible
- a credible low-level rotational object is collocated within an adaptive physical radius
- temporal/vertical behavior is consistent

Do not use a fixed 5 km association radius at all ranges without testing beam geometry.

---

# Phase 7 - Build one fusion layer

After Phases 2-6, Tornado ID should be the only place where evidence becomes a tornado-level score.

Suggested feature vector:

```
rotation:
  low_level_max_azshear
  low_level_p90_azshear
  midlevel_max_azshear
  robust_delta_v
  object_area
  vertical_depth
  lowest_height
  rooted
  sense
  LLSD_fit_quality
  velocity_texture
  range
  beam_height

temporal:
  track_age
  volumes_persisted
  motion_consistency
  shear_trend
  depth_trend

polarimetric:
  debris_candidate_quality
  min_cc
  mean_cc
  cc_contrast
  mean_z
  max_z
  zdr
  compactness
  debris_height
  tds_persistence

collocation:
  rotation_debris_distance
  rotation_debris_height_consistency

context:
  reflectivity_cell_strength
  MESH/MEHS if available
  storm type features if reliable

external:
  tornado report
  observed warning
```

External confirmation must remain semantically distinct from radar inference.

An observed warning or report may alter the displayed state, but it should not contaminate training features intended to estimate radar-only probability.

---

# Phase 8 - Start with transparent fusion, then train a model

Do not block the detector redesign on ML.

## Stage A - transparent fusion

Implement an explainable provisional score with terms exposed in the tooltip/debug output.

Every positive and negative contribution must be visible.

Example output:

```
Rotation strength        +0.21
Low-level rooting        +0.12
Vertical depth           +0.09
3-volume persistence     +0.14
Debris collocation       +0.18
Velocity texture         -0.11
Far-range beam penalty   -0.08
```

Again: derive actual weights from backtests rather than the example.

## Stage B - trained tornado probability

Once a sufficiently large labeled corpus exists, train a small offline model.

Preferred initial candidates:

- logistic regression
- generalized additive model
- gradient-boosted trees
- random forest

Do not begin with a deep neural network.

The runtime should only need inference. Training can occur offline in Python and export model coefficients/tree data into a deterministic format consumed by Rust.

## Calibration

If the UI eventually says:

```
70% tornado probability
```

the model must be calibrated on held-out data.

Evaluate:

- reliability diagram
- Brier score
- expected calibration error
- isotonic calibration
- Platt/logistic calibration if appropriate

Split train/test by complete storm/event, not random detections.

Never allow consecutive volumes from the same tornado into both train and test.

---

# Phase 9 - Expand the historical corpus

The current backtest corpus is too small for reliable probability calibration.

Create manifests for:

## Tornadic cases

Include:

- classic supercell tornadoes
- QLCS tornadoes
- weak EF0/EF1 tornadoes
- violent tornadoes
- short-lived tornadoes
- rain-wrapped/night tornadoes
- near-radar cases
- far-range cases
- tropical tornadoes

## Hard negatives

This category is critical.

Include:

- large hail supercells with no tornado
- strongly rotating non-tornadic supercells
- derecho/QLCS shear zones
- gust fronts
- outflow boundaries
- wind farms
- terrain clutter
- anomalous propagation
- clear-air velocity noise
- velocity dealias failures
- range-folding artifacts
- sidelobes
- melting-layer low CC
- biological scatter
- bright-band contamination
- very high-reflectivity hail cores

## Sampling

Do not compare a few tornado minutes against entire quiet days without controlling class balance.

Maintain:

- event-level metrics
- object-level metrics
- radar-hour false alarms
- per-volume false alarms

---

# Phase 10 - Improve truth matching

Local storm reports are useful but incomplete.

Support several truth sources separately:

- SPC/NCEI tornado reports
- damage survey tracks
- tornado start/end times where available
- surveyed path geometry
- observed warnings as contextual metadata only
- manually verified research cases

Truth matching should prefer distance to a tornado path segment when available rather than only distance to a point report.

Record uncertainty in event timing.

Do not silently treat every unmatched radar detection as definitively false when truth coverage is weak.

---

# Phase 11 - Verification matrix

Every major detector revision must produce a comparison table against the baseline.

At minimum report:

```
POD
FAR
CSI
precision
recall
F1
false alarms / radar-hour
false alarms / volume
median lead time
score reliability
```

Break these down by:

```
0-30 km
30-60 km
60-100 km
100-150 km

beam height:
<1 km
1-2 km
2-3 km
>3 km

storm type if labels exist
```

A change is not accepted merely because the total FAR falls if weak tornadoes or distant tornadoes are disproportionately lost.

---

# Phase 12 - UI and analyst diagnostics

Add an analyst/debug mode showing why each object exists.

For rotation show:

- LLSD AzShear object outline
- max / p90 / median AzShear
- robust Delta-V
- max Delta-V
- area
- lowest/highest height
- vertical depth
- velocity texture
- fit RMSE
- age
- motion
- score history
- QC flags

For debris show:

- CC footprint
- CC contrast
- Z
- ZDR
- area
- vertical extent
- rotation distance
- whether it is:
  - polarimetric anomaly
  - debris candidate
  - TDS

For Tornado ID show a complete feature explanation.

This is required so false alarms can be diagnosed instead of merely observed.

---

# Phase 13 - Safe migration strategy

Do not replace the existing detector in one commit.

Implement:

```
Legacy rotation detector
New LLSD detector
```

side-by-side behind a feature/debug flag.

During development:

- run both on every backtest case
- export paired detections
- compare object counts and truth matches
- inspect disagreements

Only promote the LLSD detector to default once it improves the agreed validation metrics.

Keep legacy mode available temporarily for regression comparison.

Do the same for Tornado ID fusion.

---

# Recommended implementation order for Claude Code / Codex

Execute in this order:

1. Baseline/export tooling.
2. Stop detector score double counting.
3. Rename "confidence" to evidence score in uncalibrated UI paths.
4. Implement LLSD/AzShear field.
5. Implement connected rotation objects.
6. Add strong QC and robust object statistics.
7. Replace vertical single-linkage with physical column association.
8. Add temporal tracking.
9. Split polarimetric anomaly from true TDS.
10. Build one centralized Tornado ID fusion layer.
11. Expand hard-negative test corpus.
12. Tune transparent score using held-out events.
13. Add optional offline-trained classifier.
14. Calibrate probability.
15. Promote new detector to default only after documented validation.

---

# Non-negotiable regression tests

Add automated fixtures covering at least:

- isolated bad velocity gate does not create high score
- residual Nyquist jump does not create high score
- radial seam does not create a circulation
- broad gust-front shear is not classified as compact tornado rotation
- three spatially chained but mutually displaced tilt detections do not create a deep column
- same-sign storm-relative shear can still be detected if physically valid
- strong hail low-CC core with no rotation is not labeled TDS
- low-CC biological/clutter region is rejected
- collocated clean low-level rotation + compact low-CC debris object strongly increases Tornado ID
- duplicated detector objects do not increase final confidence
- one-frame noisy object receives limited confidence
- persistent coherent object becomes more credible
- real short-lived extreme circulation is not blocked solely because persistence is missing
- score explanation exactly reproduces the score
- outputs remain deterministic across runs

---

# Performance constraints

HookEcho is interactive, so new algorithms must remain bounded.

Requirements:

- no unbounded full-history scans per frame
- detector work once per new radar volume
- cache LLSD fields and objects by volume key
- use bounded recent-track windows
- avoid allocations inside gate loops where practical
- parallelize per-tilt processing only if deterministic ordering is preserved
- retain deterministic output for tests and reproducible backtests

Benchmark:

- LLSD generation
- object segmentation
- vertical association
- temporal tracking
- Tornado ID fusion

Record representative timings for low-end desktop hardware.

---

# Definition of done

The redesign is complete when:

1. Rotation candidates come from a robust local shear field rather than a single adjacent-gate maximum.
2. Rotation objects are contiguous and quality controlled.
3. Vertical continuity uses physical height and cannot be artificially chained.
4. Temporal persistence and motion influence tornado inference.
5. Low-CC objects without credible rotation are not automatically called TDS.
6. Rotation and debris evidence are fused exactly once.
7. "Probability" is only shown if calibrated.
8. High-score false alarms are substantially reduced on a hard-negative corpus without an unacceptable loss of POD or lead time.
9. All scoring is explainable in analyst mode.
10. Backtests are reproducible and event-separated.

---

# Research direction notes

The implementation should use established operational/research ideas rather than blindly copy thresholds.

Relevant concepts:

- Local Linear Least Squares Derivatives (LLSD)
- MRMS AzShear
- object-based mesocyclone detection
- TORP-style object tracking and probabilistic fusion
- velocity QC before rotation detection
- low-level vs mid-level AGL rotation
- multi-product TDS identification
- temporal and vertical continuity

Useful starting references:

- NSSL Tornado Research:
  https://www.nssl.noaa.gov/research/tornadoes/

- NSSL severe-weather tornado detection overview:
  https://www.nssl.noaa.gov/education/svrwx101/tornadoes/detection/

- NOAA/NSSL TORP publication:
  https://repository.library.noaa.gov/view/noaa/48189

- NWS Dual-Pol applications:
  https://www.weather.gov/jan/dualpolupgrade-applications

- NWS Tornado Debris Signature training/examples:
  https://www.weather.gov/lmk/nws_radar_dualpol_tordebris

The agents should verify implementation details against primary meteorological literature before hard-coding new thresholds.

---

# Instructions to coding agents

When executing this roadmap:

- inspect the current implementation before changing it
- preserve existing useful QC/tests unless the replacement makes them obsolete
- make incremental commits
- do not silently change unrelated UI or radar behavior
- add tests with every detector behavior change
- run the full Rust test suite after each major phase
- run the archived-event backtest before and after detector changes
- document metric changes in the commit/PR
- do not optimize solely for the existing small corpus
- explicitly record regressions and tradeoffs
- prefer physically meaningful features over arbitrary score bonuses
- never call an uncalibrated heuristic a probability
