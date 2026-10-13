# 3d_rework.md — migration dispositions

[Original bytes](../sources/3d_rework.md) · [current plan](../../README.md)

Rows name original line numbers. Section rows include their subordinate prose/list
requirements. Evidence is historical; statuses do not claim fresh tests.

| Line | Item | Status | Destination / evidence |
| --- | --- | --- | --- |
| 1 | 3D map resolution rework | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 7 | Where it stood (measured on a cached KHTX volume, `KHTX20260910_010232_V06`) | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 19 | Plan | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 21 | 1. Smooth: crop the box to the real echo | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 28 | 2. Smooth: filtered sampling | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 37 | 3. Smooth: scale the grid to the data | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 45 | 4. Observed: polar cone surfaces instead of per-gate quads | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 67 | Progress | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 69 | [x] Measure the baseline | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 70 | [x] Plan | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 71 | [x] 1. Crop the smooth box (`echo_extent_km`, Full range checkbox, UI readout) | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 72 | [x] 2. Filtered (masked trilinear) sampling (`Rg8Unorm` + sampler, both raymarch users) | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 73 | [x] 3. Scaled grid (`plan_grid`), rayon build, one-sample-per-cell stepping | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 74 | [x] 4. Polar radial-strip Observed renderer | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 75 | [x] Verify: unit tests, GPU tests, real-volume renders (see Verification) | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 76 | [ ] Not yet done: on-device check on Android; growing the standalone 3D window grid; view-following Smooth box | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 78 | Log | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 80 | Verification (KHTX20260910_010232_V06, release build, real adapter) | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 91 | Implementation notes | superseded | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |

Classification bases (by family):

- **3D map resolution rework**: Original rendering design implemented and measured in its Verification table; follow-on grid/ROI work superseded its old remainder (M3.6). Android proof remains R6/M1.
- **Where it stood (measured on a cached KHTX volume, `KHTX20260910_010232_V06`)**: Original rendering design implemented and measured in its Verification table; follow-on grid/ROI work superseded its old remainder (M3.6). Android proof remains R6/M1.
- **Plan**: Original rendering design implemented and measured in its Verification table; follow-on grid/ROI work superseded its old remainder (M3.6). Android proof remains R6/M1.
- **1. Smooth: crop the box to the real echo**: Original rendering design implemented and measured in its Verification table; follow-on grid/ROI work superseded its old remainder (M3.6). Android proof remains R6/M1.
- **2. Smooth: filtered sampling**: Original rendering design implemented and measured in its Verification table; follow-on grid/ROI work superseded its old remainder (M3.6). Android proof remains R6/M1.
- **3. Smooth: scale the grid to the data**: Original rendering design implemented and measured in its Verification table; follow-on grid/ROI work superseded its old remainder (M3.6). Android proof remains R6/M1.
- **4. Observed: polar cone surfaces instead of per-gate quads**: Original rendering design implemented and measured in its Verification table; follow-on grid/ROI work superseded its old remainder (M3.6). Android proof remains R6/M1.
- **Progress**: Original rendering design implemented and measured in its Verification table; follow-on grid/ROI work superseded its old remainder (M3.6). Android proof remains R6/M1.
- **Log**: Original rendering design implemented and measured in its Verification table; follow-on grid/ROI work superseded its old remainder (M3.6). Android proof remains R6/M1.
- **Verification (KHTX20260910_010232_V06, release build, real adapter)**: Original rendering design implemented and measured in its Verification table; follow-on grid/ROI work superseded its old remainder (M3.6). Android proof remains R6/M1.
- **Implementation notes**: Original rendering design implemented and measured in its Verification table; follow-on grid/ROI work superseded its old remainder (M3.6). Android proof remains R6/M1.
