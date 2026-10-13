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

## GOES for each pane's own time (2026-10-12, 1010.md V15)

GOES had one slot and one decoded grid per layer, scheduled for the active pane only: a pane on
another analysis time (another time group, or with time linking off) or another sector waited,
drawing no satellite, rather than borrow the active pane's imagery. Such a pane now has its own
request-keyed slot (`app/goes_panes.rs`, `GoesPaneCache`): its own fetch, decoded field and
texture, drawn through the per-pane keyed textures the renderer already used for MRMS (keys from
2^48 up, clear of MRMS's), read by its probe and readiness (`field_state_for`,
`goes_ready_for`), and kept from cancellation while some pane wants it. A request no pane wants
is dropped with its texture, and comes back with a new key
(`each_request_has_its_own_slot_and_texture`). The active pane's path is unchanged, so every
other reader of its GOES grid is too. A reply is accepted for a pane's slot only if that slot
asked for exactly that request.

**Browser build on WebGPU, one machine**, built from the working tree at `1cbb7df4` with this
change and another session's uncommitted work: three panes on KTLX with GOES-East IR, time
linking off; pane 1 (active) at 12:01 UTC, pane 3 sent live.

| Capture | What it shows |
| --- | --- |
| `goes-other-pane-live-browser.jpg` | Pane 3's pinned probe, with pane 1 active: "IR brightness temp 68.1 °F", "GOES-East, 3:32 PM CDT (Δ-47s vs radar)", valid 2026-10-12 20:32:36 UTC. |
| `goes-active-pane-archive-browser.jpg` | Pane 1's pinned probe at the same moment: "GOES-East, 7:02 AM CDT", valid 12:02:36 UTC. |

| File | SHA-256 |
| --- | --- |
| goes-other-pane-live-browser.jpg | `2c30ff7bc4dd6cc4d7658d5908ddd0e81d933d088c9173bd0bf6d50139a8c5ce` |
| goes-active-pane-archive-browser.jpg | `4ccdf40838e32c8f91ffdeccbfa3d78f63a73cbeb542f901866c374595def01c` |

The first build of this did not show it: an earlier reply filter discarded every GOES reply that
was not the active pane's, so pane 3 read "IR brightness temp —". Replies for a pane's own slot
now pass it. Not established: two time groups set from the picker (this run used time linking
off, which gives each pane its own time by the same path), a mesoscale sector in a pane that is
not active (it keeps the chosen box, without the active pane's footprint to fall back to CONUS
outside it), those panes in Sources, the desktop app and Android.

## A mesoscale sector's box for a pane on its own time (2026-10-12, roadmaps/models-satellite C1)

V15 left one gap: a pane whose GOES request is not the active pane's (another time group, time
linking off) had no footprint for the chosen mesoscale sector, so it could not fall back to
CONUS outside the box or draw the box. The footprint was kept for the active pane's request only.
Now each pane's footprint is kept by its own footprint request (`goes_pane_footprints`). It is
filled from the pane's own mesoscale granules, and from a footprint probe asked for that pane
while a GOES layer is on in it, even while it reads CONUS, as the active pane's is.
`goes_footprint_for(idx)` reads it, so the pane's sector choice and the box use it. Footprints no
pane's time asks for are dropped. Replies are accepted only by the pane slot that asked.

**Browser build on WebGPU, one machine**, built from the working tree at `563105fa` with this
change and another session's uncommitted TGFTP work: two panes on KTLX with GOES-East IR, time
linking off, the Mesoscale 1 sector chosen; pane 1 (active) sent to 18:00 UTC, pane 2 live.

| Capture | What it shows |
| --- | --- |
| `goes-meso-footprint-per-pane-browser.jpg` | Both panes read CONUS IR at their own times (the hurricane off Mazatlán in different places), each with its own Mesoscale 1 box: pane 1 "GOES-East Meso 1 · 18:00Z (view outside: CONUS)", pane 2 "· 02:35Z (view outside: CONUS)". Pane 2's box appeared only once its own probe answered, a few seconds after pane 1 moved. |

| File | SHA-256 |
| --- | --- |
| goes-meso-footprint-per-pane-browser.jpg | `6ca498533c7c069affdf69ad9144a390ce65edfe6ce929d3a2562f26a3dd9cc3` |

Not established: a pane inside the box on its own time reading the mesoscale sector itself
(here both views were outside it), group pickers, and the desktop and Android apps. No unit test:
the logic lives in the app state, which has no test harness.
