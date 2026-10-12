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

### Hail cores and BWER candidates

The same `track`, with two samplers that read every reflectivity tilt:
- **Hail core** (`hail_peak`). The largest MEHS on a 1 km lattice of columns, by the Witt
  et al. (1998) integral the hail grids use (`derived::mehs_column`). It needs the 0 °C and
  −20 °C heights; the floor is 13 mm.
- **BWER candidate** (`bwer_peak`). At 2.5 to 9 km up, a weak echo walled in on a 3 km ring
  (7 of 8 points at 40 dBZ and 10 dB over it). It must be capped by echo as strong up to 6 km
  higher. Close to the radar the weak echo can rise past the highest tilt; when every higher
  tilt is still a walled-in weak echo, the cap is out of sight and is not required. The
  value is the ring's median less the weak echo, in dB; the floor is 10 dB.

Synthetic tests: `a_bounded_weak_echo_is_found_and_an_open_or_absent_one_is_not` (a plain
core, a weak echo open on one side and one open at the top are refused),
`a_bounded_weak_echo_rising_past_the_highest_tilt_still_counts`,
`a_hail_core_is_found_at_the_storm_core`.

**Live, Moore 2013** (`live_the_moore_hail_core_and_bwer_are_followed_through_the_loop`,
ignored, network; `target/parity-review/v14/live-moore-hail-bwer.log`). The melting level
is the Norman 20 May 12Z sounding (0 °C at 3,741 m and −20 °C at 6,775 m above the launch
site). The test uses them as heights above KTLX, without correcting for the difference in
ground height. Seeded on the strongest within 15 km of the mesocyclone at 20:12:
- Hail core: 10 of 11 volumes, MEHS 15 to 27 mm, 10 km north of the mesocyclone at 20:12.
  The track moves 14.7 m/s toward 47°, faster and more northerly than the storm's 8.9 m/s
  toward 73°. It jumps 6 km north between 20:03 and 20:08, likely to another core.
- BWER candidate: 6 of 11 volumes, 19:51 to 20:16 (19:55 not found), 22 to 42 dB, 5 to
  7 km northeast of the mesocyclone, moving 6.7 m/s toward 61°. It is lost from 20:21, as
  the storm came within about 15 km of the radar.

Neither is checked against an independent record here: not against the storm's hail
reports, and not against a published analysis of its BWER.

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

**Hail core and BWER walkthrough** in the browser build (WebGPU, one machine, 2026-10-12 about
02:45 UTC). Built from the working tree at `f1cfd04` with this change and another session's
uncommitted work. KTLX archive 2013-05-20, reflectivity, with 19:51 to 20:21 UTC decoded by
stepping through them. Then the gate inspector tool (G) at 20:08:

| Capture | What it shows |
| --- | --- |
| `gate-inspector-reflectivity-follow-browser.jpg` | The gate inspector on a reflectivity gate offers "Follow this core through the loop", "Follow this hail core" and "Follow this BWER candidate". |
| `moore-bwer-followed-browser.jpg` | Clicked near 35.355, -97.498: "Followed through 5 of 8 volumes searched". 2:59 to 3:16 PM CDT, 18 to 42 dB, confidence 63 to 97 %, northeast of the hook inside the Tornado Warning. 2:51 and 2:55 PM were not found, so the track starts at 2:59. |
| `moore-hail-core-followed-browser.jpg` | Clicked near 35.38, -97.48. The melting level was fetched without a visible wait. "Followed through 8 of 8 volumes searched": 2:59 to 3:29 PM CDT, 13 to 26 mm. The picked 3:08 PM fix is 13 mm at 19 %, because the click was off the core. The track runs from west of the hook northeast past Valley Brook. |

Checks in a clean worktree at `f1cfd04` with only this change: clippy clean, the wasm library
check at its baseline, workspace tests 2,658 passed, 0 failed, 216 ignored
(`target/parity-review/v14/wt-hb-*.log`). A first run under load failed
`relay_provider::integration_tests::a_backup_continues_the_primarys_volume_without_duplicating_radials`
once. It passed alone and in the full rerun.

The browser's fixes are not the native test's to the decimal. The search lattice is centred
on each search point, so a different click searches different points.

| File | SHA-256 |
| --- | --- |
| `gate-inspector-reflectivity-follow-browser.jpg` | `f703f1d573abd0ffcb6475c0c080fa5037fefa4fe37b1bd11973560e7264ace6` |
| `moore-bwer-followed-browser.jpg` | `b8a9b7e22633eae3cf83efeee2e40cdb294213d36bcbc812140026750ffa4bf1` |
| `moore-hail-core-followed-browser.jpg` | `475f1f8f3e19dadcef2b1d2defc97fa87cd9e3469899edf7fa18582bc01bb7b5` |

Limits:
- Only volumes already decoded are searched, so play the loop first. The window counts the
  rest.
- One feature at a time.
- A rotation and a core use the lowest tilt only. Contour features are not offered.
- A BWER candidate is not a confirmed BWER, and the hail core's MEHS is an estimate.
- The confidence describes the following, not the feature: it is not a tornado probability.
- Not run in the desktop app or on Android.
