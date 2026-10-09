# M5.1 independent analysis-time groups (1008.md E1)

Each pane belongs to one **analysis-time group** (`MapView::time_group`, 1 to `view::MAX_PANES`: 9 on desktop and web, 6 on Android).
While Link times is on, each group keeps its own linked clock
(`app::pane_time::{LinkedTimes, sync_time_groups}`), so an archive group and a live group run
side by side.

- **Scrubbing and jumps:** scrubbing the focused pane, a scene's time, a palette jump or an event
  jump moves that pane's group and no other.
- **Satellite frames:** a satellite frame chosen with "follow analysis time" drives the focused
  pane's group.
- **Group clock source:** a group's clock follows the focused pane when it is a member, otherwise
  the member it last followed. A group with no members is dropped.
- **Requests and probes:** each pane's fetched layers (MRMS, models; `model_target_time`) and its
  probes measure against its own group's time (`linked_analysis_time_for`,
  `linked_archive_time_for`). The time badges show each pane against its group, and name the
  group when there is more than one.
- **Storm linking:** all-pane storm linking requires every pane in one time group.
- **Migration:** every pane starts in group 1, so the old global link is one all-pane group. A
  workspace saves `time-group` only for a pane outside group 1, so existing and one-group files
  are byte-identical. A missing, out-of-range or malformed value loads as group 1.

The picker is under Pane links, below the spatial links:

| File | SHA-256 |
| --- | --- |
| [time-group-picker.png](time-group-picker.png) | `b262f4db4360da77c1cf8c0cffb29c19f734e6c3f5a4a62628947434b5edb094` |

(rendered by `cargo test -p hookecho --lib gpu_time_group_picker_snapshot -- --ignored`)

## Tests

- `an_archive_group_and_a_live_group_keep_their_own_clocks`:
  - KTLX and KINX are in group 1 on an archive instant; KFWS is in group 2, live.
  - KINX follows group 1 and KFWS stays live.
  - Moving focus to KFWS keeps group 1's instant.
  - A jump in group 2 leaves group 1's panes where they were.
  - KFWS joining group 1 adopts its instant (the focused pane is retargeted), and the empty
    group's clock is dropped.
- `one_group_is_the_old_global_link`.
- `a_time_group_round_trips_and_older_or_odd_files_load_in_group_one`.

## Not established

- No multi-group operator capture in the running app, and no Android run.
- GOES clocks and sector footprints still follow one shared satellite cursor (the focused pane's
  group drives it).
- Source driver selection per group; comparison, ensemble and contour ownership.
