# M6.2 colour tables, annotations and the scale's side in scenes (1008.md F1)

A scene saved now also keeps the colour tables (`Settings::palettes`, so a `.pal` file, a
built-in alternate or a browser's stored file) and the freehand annotations on the map. Take
puts both on through the same path as the rest of the scene (Take button and Alt+1..9): the
scene's tables replace the current ones, a moment the scene does not name goes back to the
built-in table, and the scene's drawing replaces what is drawn, so the last scene's arrows do
not stay on air. Scenes saved before this keep the palettes and the drawing as they are.

Readiness checks every table the scene names before Take. If the table for the product the scene
puts on air is missing (a built-in this build lacks, a file that is gone or a file that is not a
colour table), Take is **blocked** and says which file. For other moments it is a note, and that
moment keeps the table set now. No other table is put on air under the scene's name.

The colour scale's side is part of the dressing (`Broadcast::legend_side`, "Scale at the
Left/Right edge" under Streaming overlay), so a scene carries it in its style. With the scale at the left,
the clock no longer leaves room for it on the right. The logo, the field cards and the stale-scan
badge move right to clear it. The output window now honours the dressing's "Colour scale" switch
as streaming mode does. Off-screen renders (`--watch`) draw their own horizontal bar and ignore the
side.

## Real-app check, desktop (Windows, RTX 2060), 2026-10-09

The debug build was launched sandboxed (`HOOKECHO_SANDBOX=1`) with the output window open, the
dressing's `legend_side` set to `left` and reflectivity's table set to the built-in "High
contrast (reflectivity)". [output-legend-left.png](output-legend-left.png) is the output window
captured with `PrintWindow` after 40 s: live KTLX reflectivity with the scale and its "REF dBZ"
caption down the left edge. The clock is at the right edge with nothing reserved beside it, and
the strap, caption and crawl are unchanged.

| File | SHA-256 |
| --- | --- |
| output-legend-left.png | `adddb56bbf0766251661c919ed00ae79d375314d384aa6c62ec3411da161b8da` |

## Tests

`cargo test -p hookecho --lib scene` (13 passed), including:

- `a_scene_keeps_its_colour_tables_and_never_airs_a_missing_one`: a built-in, a browser-stored
  file, a missing file and a non-table file. Covers blocking versus notes by the scene's
  product, what Take sets, and a round trip.
- `a_scene_keeps_its_annotations_in_their_colours`: premultiplied colours round-trip; an empty
  drawing is kept as empty.
- `the_scale_side_rides_in_the_dressing_and_older_styles_keep_it_right`: an older style reads as
  right; the left strip's geometry.

## Not established

- No capture of Take switching tables and annotations in the running app (unit-tested only).
- The stale-scan badge's move was not captured (the scan was fresh at capture time).
- Thumbnails; an Android scene surface; legend placement in off-screen renders.
