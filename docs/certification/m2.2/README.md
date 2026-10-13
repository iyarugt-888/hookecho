# M2.2 split/merge lineage in the Storms table (1008.md B1)

Each Storms table row whose storm has lineage carries a quiet tag at its right end, left of the
rotation flags (T, M):

- **split:** the storm split from another, or another split off it;
- **merge:** it absorbed another storm;
- **split+merge:** both.

Hovering the row says which: "Lineage: split from #1; absorbed #3". The row's accessible name
includes the same words. The tag reads the storm history in both directions
(`StormIdentity::lineage_mark`, sharing `lineage_parts` with the History line).

| File | What | SHA-256 |
| --- | --- | --- |
| [storm-row-lineage.png](storm-row-lineage.png) | Four rows, each tag with and without rotation flags, drawn by the table's own `paint_row_marks` | `0f263f82a6d41a677918f6dbad8c4d5c25427e6ac71e503c418356fec0d41117` |

(rendered by `cargo test -p hookecho --lib gpu_storm_row_lineage_snapshot -- --ignored`)

Tests:

- `a_split_and_merge_read_on_both_storms_and_a_merged_pick_says_where_it_went` now also checks the
  survivor's tag.
- `a_storm_without_lineage_has_no_tag_and_a_split_tags_both`: no tag before a split; both halves
  read "split", with "split off #2" and "split from #1"; a cell not in the table has none.

## Not established

- The capture draws the rows with the table's painter, not the whole Storms dock on live SCIT
  data.
- A desktop and Android walkthrough.
- Archive replay of local cells, since Level 3 SCIT is not archived in the app.

## Archive replay of local cells (2026-10-13, roadmaps/storm-analysis S1)

SCIT is not archived, so a scrubbed-to time had no storm table: the storm list, follow, compare,
the cell card and the storm history were empty in an archive. Now the archive's cells come from
its own reflectivity. The cells of up to 16 decoded frames ending at the playhead
(`celltrack::find_cells`, the 45 dBZ cores the local tracks already used) are folded through a
fresh `StormHistory` as `Source::LocalCell` observations (`celltrack::replay`), and the current
frame's cells become storm-cell rows: the history's stable ID ("L-3"; SCIT's IDs are a letter
and a digit, so the two never coincide), the motion fitted over each one's own track, its past
positions, its strongest gate, and its range and azimuth from the radar. The storm identity
history records them as local cells and restarts when the table switches between SCIT and local
cells at one radar, so neither's IDs are taken for the other's. Rebuilt from the same window, a
replay or a backward scrub gives the same IDs (`an_archive_replay_keeps_stable_ids_and_a_motion`,
`scit_motion_and_a_storm_history_line_read_as_a_person_would`, which now also switches to local cells and back). Cells built from frames that
are still decoding can be renumbered once more frames arrive.

What is about you now stays live only (`live_storm_cells`): the chase target, the danger meter
and the storms heading for your route never read an archive's cells.

**Browser build on WebGPU, one machine**, built from the working tree at `bdc591a5` with this
change and another session's uncommitted TGFTP work: KTLX, 20 May 2013, 2:43 PM CDT, five frames
decoded.

| Capture | What it shows |
| --- | --- |
| `archive-local-cells-browser.jpg` | The Storms window in archive mode, "Storms (14)", rows L-5 (24 nmi, 62 dBZ) to L-3 (142 nmi, 50 dBZ) with severity; storm cell markers on the cores over Oklahoma City, Moore and to the south-west. Stepping one frame earlier kept L-6, L-7, L-9 and L-10 and showed the history's merge tags. |

| File | SHA-256 |
| --- | --- |
| archive-local-cells-browser.jpg | `b2fe506347c9b8b451fa9c349bc6cb0eb5618de93e2440849d5268360e6f56a0` |

Not established: following and comparing a local cell through a long replay, a backward scrub
seen in a run, and the desktop and Android apps.
