# M3.6 isosurfaces in a region; a region around the map centre (1008.md C5)

The 3D map's **Region** now applies to the isosurfaces as well as the smooth volume. Around a
selected storm, the shells are built over that box (`loop3d::IsoSpec::roi`,
`volume3d::build_at`), so the same cell budget gives finer shells. The mesh is moved back to
radar-relative kilometres so it lands where it is. The cache key includes the region
(`IsoKey::roi`).

The menu also offers **"N km around the map centre"**. This is a drawn area for when no storm is
selected (`set_volume_roi_here`, `point_roi`). It follows nothing and stays where it is put.

## Real volume: Moore 2013

[roi-iso-moore.txt](roi-iso-moore.txt) (SHA-256 `28d748c13fd22a1e47ccdc60740ac473ce8f1671f8a207a2c233a300a5d8eef4`), from
`cargo test -p hookecho --release --lib moore_region_isosurface_is_finer -- --ignored --nocapture`.
The run takes the 50 dBZ shell around the 55 dBZ core nearest KTLX (17.7 km west, 4.9 km north of
the radar), with a 50 km region:

| Build | Grid | Cell (horizontal × vertical) | Shell vertices inside the region |
| --- | --- | --- | --- |
| Radar-wide (box ±200 km) | 176 × 176 × 48 | 2.29 km × 0.38 km | 1,021 |
| Region (box ±25 km) | 64 × 64 × 73 | 0.79 km × 0.25 km | 9,327 |

Every region vertex lies inside the region's box.

`isosurfaces_build_over_a_region_and_land_where_it_is` (synthetic) checks three things:

- a region centred on the radar-wide shells holds them, inside its box;
- the region's shells are centred where the radar-wide ones are, within one radar-wide cell;
- a region away from the echo has no shells.

## Not established

- The Observed (gate) representation is still drawn radar-wide.
- A ruler in the 3D view.
- A region drawn by dragging on the map; the region goes around the map centre.
- A device walkthrough.

## A region drawn on the map (1008.md C5, 2026-10-10)

Region statistics already draws a box with two clicks. Its window (floating, or the
workstation's Region dock) now offers **Use as 3D region**. The active pane's region of interest
becomes the smallest square that covers the box (`map_volume::box_roi`): centred on the box, as
wide as its longer side, with the half-width kept to 5–80 km. It follows no storm, and the app
confirms with a notice.

**Walkthrough** in the browser build (WebGPU, built-in Chromium, KTLX live):

1. With Region statistics, two clicks drew a 105 × 100 km box south of Oklahoma City, and the
   window offered the button ([drawn-region-box.jpg](drawn-region-box.jpg)).
2. "Use as 3D region", then 3D → Volume: the panel reads "Region: 105 km around a point", with
   0.52 km cells, and the loop's 3D frames building over it
   ([drawn-region-volume.jpg](drawn-region-volume.jpg)).

| File | SHA-256 |
| --- | --- |
| drawn-region-box.jpg | `7d983b418f20b986e1a51a58fce94fbe6757680a9373cd9395d857acebb33d37` |
| drawn-region-volume.jpg | `1f53f53e1bd1be411fdddc0d8dc1ddff38f485b5c8fd3a559e7724f4c079abd3` |

The walkthrough build showed the confirmation as a red warning card ("New 3D region"), which is
the style for new warnings. It was changed to a success notice afterwards; that change is not
in these captures.

Test: `a_drawn_box_becomes_the_square_over_it`:

- the centre, and the half-width from the longer side;
- corners given in either order;
- the 5 and 80 km bounds;
- no storm and no following.

**Not established:** no desktop or device walkthrough. (The Observed representation now follows
the region, and there is a 3D ruler: see below.)

## The Observed representation follows the region (1008.md C5, 2026-10-10)

With a region of interest set, the Observed 3D view keeps only the gates whose ground position
falls inside the region's square (`pane_upload::clip_observed_to_roi`). That is the same region
the smooth volume and the isosurfaces build over. Every other gate is blanked before upload. The
region is part of the upload's identity (`ObservedKey::with_roi`), so setting, moving or
clearing it rebuilds. Ground range is the slant range times the cosine of the tilt at each
gate's centre. The coverage readout still describes the whole scan.

**Walkthrough** in the browser build (WebGPU, KTLX live, weak echo):

1. Observed with no region: echo scattered all around the radar
   ([observed-before-region.jpg](observed-before-region.jpg)).
2. A 65 × 70 km box drawn east-southeast of the radar, then "Use as 3D region". The notice now
   reads "3D region set: the drawn box, 70 km across" in the success style
   ([region-notice.jpg](region-notice.jpg)), which settles the earlier red-card note.
3. Observed again: gates remain only inside the box, between Shawnee and Ada; the echo around
   Norman and the radar is gone ([observed-in-region.jpg](observed-in-region.jpg)).

| File | SHA-256 |
| --- | --- |
| observed-before-region.jpg | `c427e400d52fad575eb0eaa151f2b797468ebec89d1ab7990785fbc42c8734e2` |
| region-notice.jpg | `ec8fc3b7b90f783c922fd625a96f212a65579826794fd0c4ce269c55261bc74c` |
| observed-in-region.jpg | `edf9d0745514714858fb8037e4710d0637d16b6bd2afb005323b05adeaf17644` |

Tests:

- `the_observed_view_keeps_only_the_regions_gates`: four radials, a 20 km square 50 km east.
  Only the east radial's gates 40–59 are kept, and an empty gate stays empty.
- `a_region_is_part_of_the_observed_upload_identity`: no region, a region, a moved region.

## A ruler in the air (1008.md C5, 2026-10-10)

In the 3D Observed view, the Measure tool's clicks land on the tilt under the cursor
(`HookEchoApp::pick_3d`, the same pick the gate inspector uses, now shared) instead of on the
ground. Each end is drawn where the sweep draws it (`render3d::beam_local`, with exaggeration and
beam rise). When both ends are in the air, the label reads (`measure::ruler_3d_label`):

- the horizontal distance and bearing;
- each end's tilt and height above sea level (true 4/3-earth beam geometry plus the antenna's
  altitude, not the view's exaggeration);
- the rise from the first end to the second;
- the straight-line distance between the two points in the air.

Anywhere else the map's ruler is unchanged.

**Walkthrough** in the browser build (WebGPU, KTLX, VCP 35): two clicks over the pitched map, one
near the radar and one to the south-east, give "52.9 mi @ 149° · 6.4° 14709 ft → 6.4° 39607 ft
MSL · rise +24899 ft · straight line 53.1 mi" ([ruler-3d.jpg](ruler-3d.jpg),
[ruler-3d-label.png](ruler-3d-label.png)). Both clicks landed on the 6.4° tilt, VCP 35's highest,
because the ray from the camera meets the highest tilt first. That is the pick's rule (nearest
surface along the ray), and the reason a 3D ruler on a steep view measures the upper tilts.

| File | SHA-256 |
| --- | --- |
| ruler-3d.jpg | `de0da85d6641add8851c9abc73f6ff2b24bd743be68c3bae4d72ee9c5dc531ca` |
| ruler-3d-label.png | `891a9fc5147c40a3b01799fbe9ad1b1fc8ee49af2a6ed013d45756453d8d971f` |

Test: `the_3d_ruler_reads_rise_and_straight_line` (10 km over the ground and 10,000 ft of rise
gives a 10.45 km straight line; downhill reads negative). Not added: a sampled value at each end
(the gate inspector gives it) and the terrain height under an end in the air.

## The 3D ruler's values, ground and tilts (1010.md V19, 2026-10-11)

Each end of the 3D ruler now carries, on the label's second line:
- its tilt;
- its height above sea level and above the ground (terrain tiles; "ground loading" until the
  tile arrives, left out when it cannot be had);
- the pane's product at that gate on that tilt ("17 dBZ", "range folded", "below threshold";
  velocity marked raw).

With the Measure tool in the 3D Observed view, PgUp/PgDn move the last end placed one tilt up or
down over the same ground. The label sits on a dark card.

**Walkthrough** in the browser build (WebGPU, one machine, KAKQ, VCP 212, 2026-10-11 20:30–20:40
UTC, a Tornado Watch in effect), from the working tree at `b08ea15` with this change and another
session's uncommitted live-sweep sharing work:

- `ruler-3d-top-tilt-browser.jpg`: both clicks landed on the 19.5° tilt, as the pick's rule makes
  them on a pitched view: "67.2 mi @ 117° · rise +56830 ft · straight line 68.0 mi / 19.5°
  51878 ft MSL, 51781 ft AGL, below threshold → 19.5° 108708 ft MSL, 108697 ft AGL". The far
  end's gate is past the tilt's coverage, so it has no reading.
- `ruler-3d-values-agl-browser.jpg`: the first end placed and brought down with PgDn, then the
  second: "66.4 mi @ 130° · rise +4611 ft · straight line 66.4 mi / 0.4° 338 ft MSL, 260 ft AGL,
  17 dBZ → 0.4° 4949 ft MSL, 4940 ft AGL, below threshold".

Found during the walkthrough: twenty PgDn presses delivered in one frame moved the end one tilt.
Each press is now counted (`count_and_consume_key`).

| File | SHA-256 |
| --- | --- |
| ruler-3d-top-tilt-browser.jpg | `76fd1bed2de1d1865526513ade8e385ad08fdb23e59f202a538a8e75f98c5ae1` |
| ruler-3d-values-agl-browser.jpg | `4567e609cfdadbd3b9e7ea64e6ac7d49d4518b30321b46aa81563b6ee5f20718` |

Tests:
- `the_3d_ruler_reads_rise_and_straight_line`: ground known (800 ft AGL), loading, and unknown
  (left out);
- `an_end_reading_says_what_the_gate_holds`: units, folded, below threshold, velocity raw;
- `the_tilt_steps_one_press_at_a_time_within_the_volume`.

Open:
- Changing the first end's tilt after the second is placed (re-click it).
- A pick that prefers tilts holding data over the first surface the ray meets.
- A desktop or device walkthrough.

## Either ruler end moves without re-clicking (2026-10-12, roadmaps/radar-analysis R5)

With the measure tool armed, a finished ruler's two ends are ringed handles (the held one
larger). A drag that starts within 12 points of an end (24 on a touch screen) moves that end and
does not pan the map (`measure_input`, `end_at`; `a_press_takes_the_nearer_end_within_reach`):
on the ground on the 2D map, and onto the tilt under the pointer in the 3D Observed view, the
way a click places an end. The ground under the end is read again when it is let go, and Esc
puts the ruler back as it was.

**Browser build on WebGPU, one machine**, built from the working tree at `824a3317` with this
change and another session's uncommitted TGFTP work.

| Capture | What it shows |
| --- | --- |
| `ruler-end-dragged-browser.jpg` | A ruler from Norman whose other end was dragged from Oklahoma City to Lawton: both ends ringed, the label "68.3 mi @ 234° · beam 6273 ft above the radar (7551 ft MSL, 6424 ft above the ground (terrain on a 63 m grid))", the ground read after the drag. |

| File | SHA-256 |
| --- | --- |
| ruler-end-dragged-browser.jpg | `cd5446a254e05f785d5c0d20f6d7bec3a4596a26e6000ed8052a5485673fe87f` |

Not established: dragging an end in the 3D Observed view (the code re-picks the tilt as a click
does, not seen in a run), Esc mid-drag, and touch.
