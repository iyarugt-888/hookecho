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

## A real replay: the Moore storm keeps one label (2026-10-13, roadmaps/storm-analysis S1)

`archive_cell_labels::the_moore_storm_keeps_one_label_through_a_replay` (wxdata, network, ignored;
volumes cached in `target/trail-volumes`) runs the app's archive pipeline over 21 KTLX volumes,
2013-05-20 19:00–20:30 UTC: 45 dBZ cores on the lowest tilt (`find_cells`), a 16-volume window
(`replay`), labels carried table to table (`LocalLabels`). It steps forward through the volumes,
so the window slides; steps back again; and at the last playhead adds the window's volumes newest
first, as the prefetch delivers them. In each table no label names two storms
([s1-moore-replay-labels.txt](s1-moore-replay-labels.txt), SHA-256
`2b2cde647e652f0d11040ad1b9e681470f9f01b423960338c8c146737029b4f9`).

- Replay's own ID for the Moore storm took four values as the window slid: L-48, L-47, L-46,
  L-45. That is the defect `f548099f` fixed, on real data.
- The first version of the fix still gave the storm a new label once. The tracker carried it
  across 20:03Z without a cell, so no row of that table held its observations, and at 20:08Z it
  went from L-52 to L-68. `LocalLabels` now remembers the observations of the last four tables,
  so a storm missing from up to three tables in a row keeps its label
  (`a_storm_missing_from_a_table_keeps_its_label`).
- Now: L-48 from 19:59Z to 20:29Z, seven tables over 29 minutes, the same backward and with
  newest-first decoding. Before 19:59Z the tracker holds no link to this storm: the 19:55Z table
  has a separate 63 dBZ storm (L-24) 14.9 km away with its own five-volume track. That is the
  tracker's association, which this lane does not tune.

Not established: the same played through in the app (the debug browser build decoded too slowly to
play the hour), compare over the replay, the desktop and Android apps.

## Whose motion an archive storm's arrivals come from (2026-10-13, roadmaps/storm-analysis S2)

Seen in the S1 follow run: the cell card of an archive's local cell (L-18) said "no SCIT motion
to project", and with motion it would have said "Arrivals from SCIT's motion". Its association
lines said "nearest SCIT core", "SCIT core inside source polygon" and "ambiguous between SCIT
cores", and a manual track started from it recorded "SCIT's motion for cell L-18". An archive has
no SCIT. That motion is the local tracker's fit over the cell's own track, and the cores are local
cells. S2 asks that motion provenance be kept apart. `celltrack::tracker_of` names the tracker
from the row's ID, and the card and the manual track's line use it. Tests:
`arrivals_name_whose_motion_they_come_from` and the manual-track line in
`a_manual_motion_reads_against_the_scit_motion_it_started_from`. SCIT rows read as before. Not
seen in a run.

## A storm cell's core on every tilt, and its distribution in 3D (2026-10-13)

The cell card's Core statistics, its Core distribution, the compare view and the Storm Digest's
core lines read only the pane's displayed tilt within 8 km of the cell's centre. A cell is a
column, so that was one slice of it, and the slice changed with the tilt the pane happened to
show. A hovered distribution bar outlined its gates on the ground under the storm. In 3D they
belong on their tilts, so the outline did not line up with the echo it stood for.

- `cell_core` reads a core on every tilt. A job bins the volume tilt by tilt, in every moment,
  velocity dealiased as drawn, and cuts out the gates in the core box around each wanted centre
  (`regionstats::gather_tilt`). It drops each sweep before the next tilt, then stacks the tilts
  (`regionstats::stack`), each row keeping its own. Natively it runs on a worker. In the browser,
  which has no thread for it, it runs one tilt per frame. Results are cached for the pane's
  volume. Until a core is read, the card shows the displayed tilt's and says so, with progress.
- The card's Core statistics read the whole cell ("8 km · 14 tilts"). A new **By tilt** section
  lists each tilt with the beam's height over the cell's centre and that tilt's peak dBZ, velocity
  spread, and the p5 CC and p95 ZDR of its ≥40 dBZ core, flagged by the Core statistics
  thresholds. Core distribution charts every tilt's gates. The compare view uses whole cells once
  every open storm is read, and never mixes one storm's whole cell with another's slice. The
  Digest's core lines use whole cells when they are read.
- A hovered bar's gates carry their azimuth, slant range and tilt (`region_stats::HighlightGate`).
  On the map they sit over the ground. In 3D Observed they sit on their tilt's beam, with beam rise
  and exaggeration as drawn, like the 3D ruler's ends. In a resampled volume they sit at their
  height above sea level (`highlight_screen`). The Region tool's outline uses the same path.
- Region gates are now placed by ground range, as the module always said (the code used slant
  range, 3% too far out at 15°). The Region CSV gains an `elevation_deg` column.

Tests: `a_cell_reads_every_tilt_and_splits_back_tilt_by_tilt` (wxdata), `a_core_is_gathered_tilt_by_tilt`
(a tilt's rows are exactly a single-tilt gather of it), `a_whole_cell_reads_tilt_by_tilt` (rows,
heights, flags, captions). Network control `a_full_volume_core_reads_the_storm_on_many_tilts`: the
Moore storm at KTLX 20:16Z reads on 14 tilts from 0.5° to 19.6°, 74,907 gates, 70 dBZ at most and
64 dBZ still at 19.6°. Not seen in a run (the browser pane was hidden).

Then rendered with the card's own rows for the Moore storm (KTLX 20:16Z, centre 35.32 N 97.51 W;
`gpu_moore_whole_cell_rows`, RTX 2060, network + GPU, ignored by default):
[cell-core-moore-whole-cell-rows.png](cell-core-moore-whole-cell-rows.png), SHA-256 `ce182df90707607afab163fa137d0b2caa2681c91ebec820e386ed073de79908`.
It showed two faults, both fixed before this capture:

- The whole-cell ΔV read 161 m/s (312 kt): the most inbound gate of one tilt against the most
  outbound of another. A couplet is on one tilt. ΔV is now the widest single tilt's spread,
  named when the core reads several ("123 m/s (240 kt) at 0.5°"), and the compare view takes the
  same (`couplet_spread`; the test now gives the upper tilt a steady 50 m/s that would read 80
  across tilts).
- The By tilt values ran past the card's width ("· Z…"). They are now a table under a header row,
  "Tilt · height | dBZ · ΔV · CC · ZDR", with "–" where a tilt lacks a value.

The capture reads 74,907 gates on 14 tilts. The core peaks at 70 dBZ on 0.9° and still holds
64 dBZ at 19.6° (25 kft). CC p5 is 0.39–0.51 through the low tilts, the debris, and rises to 0.6
above 6.5°.

Two more faults, found by reading the code (2026-10-13). A worker gathering for a volume the
pane had moved past kept running. Stepping or playing a loop with the card open started a
full-volume job per frame, and the jobs piled up on the CPU. A superseded gathering now stops at
its next tilt (`cell_core::Cancel`, raised when its handle is dropped; test
`dropping_a_gathering_stops_its_worker`). Nothing is gathered while the pane's loop plays: the
card shows the displayed tilt ("while the loop plays; every tilt once it stops") and reads every
tilt when the loop stops.
