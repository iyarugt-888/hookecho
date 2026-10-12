# Following a picked feature through the loop (ROADMAP_NEW R4, 1010.md V14)

## Engine

`wxdata::feature_track::track` follows a feature from the volume and point the analyst picks,
forward and back through the loop's volumes:
- **Where it looks.** Each next fix is predicted from the feature's own motion, and the search
  covers 6 km around the prediction for a rotation and 8 km for a core. It is wider until the
  feature has a motion.
- **What it looks for.** The strongest cyclonic LLSD azimuthal shear on the lowest velocity tilt
  (`rotation_peak`), or the strongest reflectivity (`core_peak`).
- **When it stops.** After two volumes in a row with nothing over the kind's floor: 0.004 s⁻¹ for
  a rotation, 40 dBZ for a core.

Each fix has a confidence: its contrast with the searched area, times its strength over the
floor, times how near the motion predicted it. A fix placed by hand is kept as placed, and the
track continues from it.

**Live, Moore 2013** (`live_the_moore_mesocyclone_is_followed_through_the_loop`, ignored, network;
`target/parity-review/v14/live-moore.log`):
- KTLX 2013-05-20, 19:51 to 20:33 UTC: all 11 volumes followed.
- Shear 0.017 to 0.046 s⁻¹.
- From 35.278, -97.647 to 35.339, -97.408, at 8.9 m/s toward 73°. That is along the tornado's
  own path, west of Newcastle through Moore.

## In the app

The gate inspector, on a velocity or reflectivity gate, offers "Follow this rotation (core)
through the loop". The pane's decoded volumes are searched off the UI thread
(`app::feature_tracking`). A "Following a rotation" window then lists each fix:
- time;
- place;
- value;
- confidence, or "by hand".

The volume shown is marked. The window says how many of the loop's volumes were not decoded yet.
On the map the track is a line with a dot per fix, sized by confidence; a hand-placed fix is a
square, and the volume shown is ringed and labelled with its time. "Correct this volume" and then
a click on the map puts the shown volume's fix there and follows the track again from it.

A saved case keeps the track: the kind, radar, picked volume and point, the hand fixes and
every fix as it was (`CaseManifest::feature_track`, left out when there is none, so older
builds open the case without it). Opening the case puts it back on the pane showing that
radar, or the active pane, without searching again; "Correct this volume" follows it again
from the volumes then decoded. Tests: `a_followed_track_round_trips_through_a_case`, and the
case manifest's round-trip and older-case tests.

**Walkthrough** in the browser build (WebGPU, one machine, 2026-10-12 about 03:30 UTC). Built from
the working tree at `6fc646a` with this change and another session's uncommitted live-sweep work.
KTLX archive 2013-05-20, velocity, stepped back to 19:51 so more of the loop was decoded:

| Capture | What it shows |
| --- | --- |
| `moore-rotation-followed-browser.jpg` | Clicked about 6 km from the couplet near Newcastle at 19:51. "Followed through 8 of 8 volumes searched; 315 more in the loop not decoded yet". Fixes from 2:42 to 3:20 PM CDT: shear 0.0125 to 0.0455 s⁻¹, confidence 58 to 95 %. The track on the map runs from west of Bridge Creek to Moore inside the Tornado Warning. |
| `moore-rotation-corrected-browser.jpg` | "Correct this volume", then a click: the 2:51 PM fix moved to 35.225, -97.621, marked "by hand" and drawn as a square. The track was followed again from it, and its neighbours' confidences changed. |

Tests:
- engine: `a_moving_feature_is_followed_forward_and_back`,
  `a_lost_feature_ends_the_track_and_a_hand_fix_is_kept`,
  `the_gate_window_holds_the_gates_within_the_radius`;
- the app's search on the Moore volumes, with a hand fix
  (`live_the_apps_search_follows_moore_and_keeps_a_hand_fix`, ignored, network): 11 of 11
  followed, 0 unreadable.

Checks in a clean worktree at `6fc646a` with only this change:
- clippy clean;
- the wasm library check at its baseline;
- workspace tests 2,654 passed, 0 failed, 215 ignored (`target/parity-review/v14/wt-*.log`).

| File | SHA-256 |
| --- | --- |
| `moore-rotation-followed-browser.jpg` | `a0dd4b07db448f58051eadd7b89294d8d68d1da011d7ae6c9108a1eb643e0566` |
| `moore-rotation-corrected-browser.jpg` | `4cee4a5f390b8a50df927ab61203cb923df895f3998017c903bee60f7589ef66` |

Limits:
- Only volumes already decoded are searched, so play the loop first. The window counts the
  rest.
- One feature at a time.
- The lowest tilt only. Hail cores, BWER candidates and contour features are not offered.
- The confidence describes the following, not the feature: it is not a tornado probability.
- Not run in the desktop app or on Android.
