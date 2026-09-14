# Valid-time alignment foundation

`wxdata::time_align` is a source-independent frame selector. Each `FrameTime` records a valid time and optional model run. The caller supplies an analysis time, policy, optional maximum time difference, and value kind. `FrameSelection::Single` returns both the selected index and signed offset. `Blend` returns bracketing indices and a weight; it never materializes an interpolated grid.

Policies are `Exact`, `Nearest`, `NearestPast`, `HoldLast`, `InterpolateLinear`, and `ForecastLead`. `HoldLast` currently has the same selection rule as `NearestPast`; a source-specific maximum hold duration is expressed with `tolerance`. Interpolation requires two frames of the same run and accepts only scalar/probability fields. Categorical, accumulation, mask, and vector values cannot be blended through this API. `ForecastLead` requires the requested run and run-plus-lead valid time to match exactly.

The archive radar timeline and GOES frame pairing now call this selector. The GOES caller retains its 30-minute tolerance; that limit is checked to the millisecond, so a frame 30 minutes and 1 second away is rejected. Existing pane state and display behavior are otherwise unchanged.

The GOES scrub control now shows the signed source-time offset from the active radar scan. If no GOES frame meets the 30-minute tolerance, it displays the latest frame in the available set and explicitly warns that the shown imagery is outside tolerance. The control and selector live in `app/goes_timeline.rs`; this keeps the existing app shell smaller while ensuring the time label matches the selected tile time.

Next integration steps: use this selector for MRMS and model frame lists, expose the selected source times and signed offsets in each pane, add a configurable mismatch tolerance, then coordinate all pane requests around a selected analysis time. Fetch generation guards must remain in place so a late old-time response cannot replace the selected frame. Do not interpolate categorical weather grids or forecast accumulations.
