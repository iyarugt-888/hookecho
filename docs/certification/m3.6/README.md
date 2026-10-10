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

**Not established:** no 3D ruler; no desktop or device walkthrough. (The Observed representation
now follows the region: see below.)

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
