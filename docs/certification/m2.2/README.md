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

## Following an archive's storm keeps that storm (2026-10-13, roadmaps/storm-analysis S1)

**Found in a run.** Browser build on WebGPU, one machine: KPAH 2021-12-11 03:23Z (Mayfield),
Storms (43), L-18 at 16 nmi, 68 dBZ, linked to Tornado Warning PAH-TO-36. Follow, then one
step forward to 9:34 PM CST. The table was renumbered: the Mayfield cell had become L-79, and
"L-18" now named another storm. The followed cell card switched to it (53 dBZ, ΔV 30 m/s instead
of 68 dBZ and 53 m/s), and a "Better radar for L-18: KNQA" banner placed it 173 km from KPAH
([s1-follow-renumbered-before.jpg](s1-follow-renumbered-before.jpg)).

**Cause.** `celltrack::replay` numbers storms in the order its window first sees them, and the
window is the last 16 frames up to the playhead (only those decoded). A step forward drops one
frame and adds one, and decoding fills earlier frames in, so the numbers changed on nearly every
step and were handed to other storms. "A replay gives the same IDs" held only for an identical
window.

**Fix.** `celltrack::LocalLabels::relabel` carries labels from the table being replaced. A storm
whose new track holds an observation of a previous storm (the same per-volume cell, cached, at the
same position) keeps its label; on a split, the cell sharing the newer observation, then the longer
track, keeps it. Every other storm gets a label never issued before in the session, so a label
never moves to a different storm. A table from another radar hands on nothing. Test:
`a_sliding_window_keeps_each_storms_label_and_never_hands_it_on`, which first reproduces raw
`replay` giving A's number to B when the window slides past a short-lived storm, then covers a
slide, a new storm, a backward step, a jump with no shared volume, and a split.

**After** (bundle `hookecho_bg-66a5499b.wasm`, same steps): one step forward to 9:34 PM kept L-18
on the Mayfield core, with the card on that storm (ΔV 75 m/s, CC 0.73) and no banner
([s1-follow-keeps-its-storm.jpg](s1-follow-keeps-its-storm.jpg)). Two steps back, to 9:30 PM, L-18
was still that storm at 16 nmi, 66 dBZ
([s1-follow-after-backward-step.jpg](s1-follow-after-backward-step.jpg)). Cells whose new track
shares no observation with the last table show new labels; that is by design.

| File | SHA-256 |
| --- | --- |
| s1-follow-renumbered-before.jpg | `cdd7bd67eb16d6a4306f0bfc695e34093633b3ac19e81ece7ae6c0611a913579` |
| s1-follow-keeps-its-storm.jpg | `b16b8967cb0fdca4bdfa3d07bc0ec5360b46fe1341085e021a9501af4182143c` |
| s1-follow-after-backward-step.jpg | `e7303f5edbb8fa945814e9e0e21ec32d88cdd155877205324f28af537a14b1ee` |

Not established: a long replay played through (decoding in the debug browser build advanced only
about one frame per step here), compare across a long replay, the desktop and Android apps. The
Trends row said "Trends draw once SCIT has tracked this cell" for a local cell too, which never
happens in an archive: trends are SCIT's per-scan attributes, fed only by the live poll. A local
cell's row now says it has none and why.
