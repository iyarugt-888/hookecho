# M2.4 notes on storms, kept in cases (1008.md B3)

Right-click a storm in the Storms table and choose **Add a note…**. A one-line editor opens at the
top of the dock. Save is enabled only once there is text. Cancel, Escape, or the radar changing
discard the draft; a draft written against one radar is never saved onto another's storm.

A note keeps its source association:

- the radar;
- SCIT's cell ID when written;
- that storm table's time;
- where the storm was;
- when it was written.

A note follows its storm through SCIT renumbering via the storm history (`StormIdentity::resolve`).
The storm's row shows it in its hover ("Note: …"), and every note is listed under the table, where
each can be deleted.

Saved cases keep the notes (`CaseManifest::storm_notes`). The field is additive and omitted when
empty, so the case format number is unchanged and older cases open with none. A note reopened
from a case is **historical**: it is listed as "(from a case)" with its own time and cell, and
never attached to whatever storm carries its old cell ID today. Opening the same case twice does
not duplicate notes (merged by radar, cell, table time and text).

| File | What | SHA-256 |
| --- | --- | --- |
| [storm-note.png](storm-note.png) | The editor with a draft, and the notes list with a live and a reopened note | `c3760a8f5bdbd48e5f2ed5557d2cfa80e42659d9886eb37e675de9bc21293bb1` |

(rendered by `cargo test -p hookecho --lib gpu_storm_note_snapshot -- --ignored`)

## Tests

- `a_note_round_trips_through_a_case_and_reopens_historical`: everything as saved, historical on
  reopening; an older case has none.
- `a_note_follows_its_storm_and_a_historical_or_other_radar_note_does_not_attach`: after a
  renumber, the note attaches to the storm's new cell and not to the cell that now has its old ID.
  Historical and other-radar notes do not attach.
- `a_case_round_trips_and_is_named_for_its_radar_and_time` now carries a storm note.

## Not established

- Notes are not drawn on the map.
- The five-second creation acceptance.
- Touch and pen editing and the cancellation paths on a physical Android device.
