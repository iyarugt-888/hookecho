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
