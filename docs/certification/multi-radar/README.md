# One storm from several radars (ROADMAP_NEW R1, 1010.md V18)

Research-grade, engine only: `wxdata::multiradar::fuse`.

- **Time.** Each radar's volume is taken within a tolerance of the time asked for, and its offset
  is kept. A radar outside the tolerance is left out, with the reason. Volumes are not advected
  to a common time.
- **Grid.** A storm-centred box: x east and y north in km, z in km above sea level, from each
  antenna's altitude.
- **Sampling.** A radar contributes to a cell only through a beam whose half-beamwidth (0.95°
  beam) covers the cell's height, by the nearest such beam's gate. Nothing is filled between
  beams. A below-threshold gate means the radar observed the cell and saw no echo.
- **Blending.** The weighted mean in dBZ of the radars that saw echo, each weighted by
  1 / (its beam's thickness there)².
- **Provenance.** Per cell: which radars observed it, which gave echo, and which counted most.
- **Velocity.** Radial velocities are never combined.

Synthetic tests: `two_radars_fill_each_others_gaps_and_say_which_saw_what` (a radar 15 km away
sees the storm low down, one 110 km away sees its upper half; fused, the column is the storm's,
each level labelled by who saw it, the near radar counting most where both did) and
`a_radar_out_of_time_is_left_out_and_nothing_is_invented`.

## Moore, 2013-05-20 20:12 UTC

`live_the_moore_storm_fused_from_four_radars` (ignored, network), release.
[moore-fusion.txt](moore-fusion.txt): a 40 km box around Moore, 1 km columns, 0.5 km levels to
16.5 km.

| Radar | Volume offset | Cells with its echo | From it alone | Counted most |
| --- | --- | --- | --- | --- |
| KTLX (about 22 km) | +29 s | 11,483 | 6,345 | 11,483 |
| KFDR | −31 s | 25,418 | 938 | 5,666 |
| KINX | +93 s | 27,254 | 1,711 | 1,742 |
| KVNX | +78 s | 27,274 | 1,353 | 18,974 |

KTLX alone: 11,483 cells with echo and an 18.5 dBZ top at 12.5 km. Its highest tilt passes only
7 to 8 km over the storm's middle. Fused: 37,865 cells, the top at the box's 16.5 km ceiling. The
distant radars' beams are 2.3 to 2.8 km thick there, so that top is smeared upward by up to half
a beam. Wherever KTLX saw echo it counted most: its beam there is about 0.4 km thick, against the others' 2.3 to 2.8 km.

| File | SHA-256 |
| --- | --- |
| moore-fusion.txt | `b540afbd5caf1157ed9d86d30ac5b1c5dbaedacc1768ef9617efdac32ff256fc` |

### How the radars agree, and reading along the storm's motion

`fuse_moving` reads each radar where the storm was at its own volume's time: a cell's place moved
by the storm's motion times that radar's offset (`a_moving_storm_lines_up_when_each_radar_is_read_where_it_was`:
a storm moving 20 m/s, read 120 s apart, back on its axis). The live test fuses each radar alone
and compares it with KTLX wherever both saw echo, read in place and along the followed
mesocyclone's motion (8.9 m/s toward 73°):

| Against KTLX | Cells | Mean difference, in place | KTLX minus it | Along the motion |
| --- | --- | --- | --- | --- |
| KFDR | 3,732 | 11.67 dB | −7.58 dB | 11.13 dB |
| KINX | 4,125 | 10.51 dB | −7.07 dB | 11.33 dB |
| KVNX | 4,526 | 8.73 dB | −6.06 dB | 9.25 dB |

Reading along the motion did not bring them closer: the offsets of 29 to 93 s move the storm
less than a kilometre, under one column. The radars differ by much more than that, KTLX reading
6 to 8 dB lower than each distant radar where both saw echo. It is not the distant beams'
height: counting only cells where the distant beam's centre is within a quarter of its
half-thickness of the cell (1,477 to 1,754 of them), KTLX is still lower by 7.67 dB than KFDR,
6.99 dB than KINX and 5.60 dB than KVNX. What it is, is not established. A 2.3 to 2.8 km beam
averaging toward the strongest echo across its width is one candidate, calibration differences
between the radars another. Because the blend weights KTLX's thin
beam far more, the fused value follows KTLX wherever it saw echo.

### KTLX through the afternoon (2026-10-12)

`live_ktlx_against_the_distant_radars_through_the_afternoon` (ignored; network) asks whether
KTLX's low reading is a fixed offset or comes and goes. At eight times from 18:30 to 21:30 UTC it
fuses each radar alone over a 100 km box around Moore (2 km columns, 1 km levels from 1 to
12 km), takes KTLX minus each distant radar over the cells where both saw echo, and measures the
echo right over KTLX: its lowest tilt's gates 1 to 5 km out
([ktlx-bias-afternoon.txt](ktlx-bias-afternoon.txt)):

| UTC | Echo 1 to 5 km from KTLX | KTLX minus KFDR | minus KINX | minus KVNX |
| --- | --- | --- | --- | --- |
| 18:30 | 72 %, 16.0 dBZ | −0.28 dB (155 cells) | −6.53 (80) | −2.33 (160) |
| 19:00 | 72 %, 16.0 dBZ | −1.50 (920) | −2.49 (814) | −1.95 (1,149) |
| 19:30 | 65 %, 10.6 dBZ | −3.13 (2,852) | −4.24 (3,379) | −1.99 (3,237) |
| 20:00 | 70 %, **37.9 dBZ** | **−5.76** (3,461) | **−4.13** (3,835) | **−4.26** (4,689) |
| 20:12 | 65 %, **30.4 dBZ** | **−4.43** (3,111) | **−3.37** (3,471) | **−4.24** (4,254) |
| 20:30 | 63 %, 12.0 dBZ | −3.31 (3,217) | −2.99 (3,852) | −3.28 (4,201) |
| 21:00 | 56 %, 16.1 dBZ | −3.38 (3,511) | −3.25 (3,970) | −2.97 (4,202) |
| 21:30 | 56 %, 14.5 dBZ | −2.73 (2,798) | −3.28 (2,971) | −2.71 (3,455) |

- KTLX read lower than every distant radar at every time, by 2 to 3 dB once there was enough
  shared echo (from 19:00). That much is steady, as a calibration difference would be.
- The gap was widest, 4 to 6 dB, at the two times the echo over KTLX was heavy (30 to 38 dBZ
  averaged in Z, against 11 to 16 otherwise). That is what rain wetting KTLX's radome would do,
  and it is the time of the 40 km box above (6 to 8 dB, closer to the core).
- Neither is established. The 1 to 5 km measure counts ground clutter too: the 16 dBZ seen
  there in the dry hours is likely clutter, not rain. Eight times on one day cannot separate
  rain at the radar from where the storm was. The volume offsets, 5 to 142 s, are not
  corrected for.

| File | SHA-256 |
| --- | --- |
| ktlx-bias-afternoon.txt | `73167d32e284763517da920b2fc76abcbbaef92b89cad839f4ef9d1cc53b8983` |

### A section through it

`hookecho --headless-fusion KTLX,KFDR,KINX,KVNX 2013-05-20T20:12 -97.50,35.33 moore-section.png`
fuses a 60 km box around Moore (0.5 km columns and levels to 18.5 km) and draws the west–east row
through the centre ([moore-section.png](moore-section.png); legend in
[moore-section.txt](moore-section.txt); debug build, Windows, one machine). Top to bottom:

- **KTLX alone.** Its tilts are sloping stripes, with dark gaps between the upper ones and
  nothing above its highest tilt on the radar's side (KTLX is to the east).
- **Fused.** The storm from the ground to about 17 km. Its strongest echo reaches far up the
  column where only the distant radars saw it. They read 6 to 8 dB higher than KTLX where both
  saw echo (above), so the upper core is likely too strong and too wide there.
- **Who counted most.** White is KTLX along its own beams. Green (KVNX), orange (KFDR) and
  blue (KINX) fill above and between them.

Dark is a cell no radar's beam reached, grey one observed with no echo.

| File | SHA-256 |
| --- | --- |
| moore-section.png | `a77c557133a5984dc44dd2815ed94a3ac2aab7f8b541054cef21fc00b2374a2f` |
| moore-section.txt | `0eca4cf00ac93d638757f93965b3beb7e26e5b493a0e61974c4fb74e9e6dc6c0` |

Test: `the_section_has_three_panels_and_tells_unseen_from_clear`.

### In the app's cross-section window

"Add nearby radars" (reflectivity only) fuses the section with up to three other WSR-88Ds within
230 km of its middle, at volumes within five minutes of the pane's (`app/xsection_fuse.rs`,
`multiradar::fuse_section`). The fetch and fuse run off the UI thread on the desktop; the
browser has one thread, so it bins and fuses on it. The fused panel replaces the pane's own only
for the same pane, volume and line, and while the line is being dragged nothing is fetched. The
window says which radars went in, their offsets, and any left out.

**Browser build on WebGPU, one machine** (2026-10-12), built from the working tree at `6f5c1ea`
with this change and another session's uncommitted live-sweep work. KTLX archive, 20:12:29 UTC,
a 64 km section west to east into KTLX:

| Capture | What it shows |
| --- | --- |
| `section-ktlx-alone-browser.jpg` | KTLX alone: "29 % of the panel inside beam coverage", its tilts' beam-rise lines, nothing above them. |
| `section-fused-browser.jpg` | "Add nearby radars" on: "Fused with KVNX (+49 s), KFDR (−60 s), KINX (+64 s)", 76 % of the panel covered, the storm filled to near the panel's top. No beam-rise lines: several radars' beams cross the panel. |

| File | SHA-256 |
| --- | --- |
| section-ktlx-alone-browser.jpg | `03ee57f711261d2a5412101278aaaed58bd57d68e61ed99eda22f7ba1385be93` |
| section-fused-browser.jpg | `8387d42abc926f59fd817479e88f7162100e3ad8788306a3e46a2d2e471b6934` |

Tests: `a_fused_section_matches_the_volume_along_its_line`,
`nearby_radars_are_others_in_range_and_the_line_names_them`. How long the browser's single
thread stalls while fusing was not measured.

Not established:
- the fused volume in the 3D view;
- the window on the desktop app and on Android (only the browser build was run);
- a check against an independent multi-radar analysis, such as MRMS's 3D reflectivity mosaic.

### In the app's 3D window (2026-10-12)

"Add nearby radars" in the 3D volume window fuses the active pane's reflectivity with up to three
other WSR-88Ds within 230 km of the map's centre, at volumes within five minutes of the pane's,
over a box 80 km across around that centre (`app/volume3d_fuse.rs`, `multiradar::fuse`). The box
has 1 km columns and 0.5 km levels from the pane radar's antenna to the window's 18 km top, and is
resampled to the nearest cell onto the window's own 192 by 192 by 48 grid, so the renderer and
its controls are unchanged (`the_fused_box_lands_on_the_windows_grid`). While the switch is on,
a new single-radar build for the same volume does not replace the fused box; the box is fused
again when the volume changes or the map's centre moves 0.05°. Off, the single-radar volume is
rebuilt. The fetch of the other radars is shared with the cross-section's (`xsection_fuse`).

**Browser build on WebGPU, one machine**, Moore archive 2013-05-20, KTLX volume at 20:12 UTC,
built from the working tree at `d460658a` with this change and another session's uncommitted work:

| Capture | What it shows |
| --- | --- |
| `fused-3d-browser.jpg` | The 3D window with the switch on: "80 km around 35.33, -97.50, fused with KVNX (+49 s), KFDR (-60 s), KINX (+64 s): reflectivity weighted by each beam's thickness. Layers and source below are this radar's." |
| `fused-3d-window-on.png` | The window close up, the fused box: the storm's core standing through the box. |
| `fused-3d-window-off.png` | The switch off again: KTLX's own volume, whose box is its whole echo extent, so the storm is drawn smaller. Not a like-for-like view. |

| File | SHA-256 |
| --- | --- |
| fused-3d-browser.jpg | `8777ee40e8f2ae4a9358e906807de1f60246853241fab249efa8f273e0a55944` |
| fused-3d-window-on.png | `bf64fcbdef58a50689d94167bed7905403557a6b907f5554ec376008600dbe52` |
| fused-3d-window-off.png | `4c9b1873d74f279c6753e45f71f820409e384c05161abbb8e375498935a99d0c` |

The fused box came up within the ten seconds waited; how long the fetch and fusion took in the
browser, which runs it on its one thread, was not timed. The 6 to 8 dB by which KTLX reads below
the distant radars (above) is in this box too. Not established: the desktop app, Android, and the
fused volume on the pitched 3D map.
