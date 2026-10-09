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
