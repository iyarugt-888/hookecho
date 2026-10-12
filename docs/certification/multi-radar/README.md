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
