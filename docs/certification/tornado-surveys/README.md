# Tornado surveys on the map (2026-10-12)

The tornado tool was a climatology lookup: click a spot, and the SPC database's tornadoes within
25 mi were listed and drawn as straight start-to-end lines, with the county's warning history.
It is now built on the NWS Damage Assessment Toolkit (`wxdata::dat`, the public ArcGIS service
the DAT viewer uses), so it shows what the survey crews found: each tornado's surveyed path and
each damage indicator they rated.

**By day.** Pick a date (or step a day either way, or one of ten notable days) and every survey
of that SPC convective day, 12 UTC to 12 UTC, is read nationwide (`dat::fetch_day`) and drawn:
each path coloured and widened by its rating, its damage indicators as rating-coloured dots once
zoomed in. The list gives each event's rating, local time (its nearest radar's zone), office,
length, width, peak wind, deaths and indicator count, filterable to EF1+ through EF4+.

**An event.** Picking one frames its path, outlines it in white, and draws all its indicators.
It shows the path's recorded length, width and peak wind, casualties when the path record states
them, a count of indicators per rating, the survey's comment, and the indicators strongest
first. Clicking one, in the list or on the map, opens its card: the damage indicator, the
degree of damage, the estimated wind, the storm time, the surveyor's notes and a survey photo
when attached. "Radar at this time" opens the nearest radar's archive at the storm time, framed
on the path.

**Near a point.** Clicking the map with the tool lists the surveyed paths within 25 mi since 2007,
newest first (`dat::fetch_paths_near`). Picking one reads its indicators (`dat::fetch_event`).
The SPC record (1950 to 2022) and the warning history stay below, the SPC lines now faint and
thin under the surveyed paths.

**Matching indicators to paths** (`dat::group`, `indicators_join_their_path`): by the survey's own
link (`path_guid`) when it set one, else the nearest path within 3 km and 12 hours. Most points
carry an all-zero link. Indicators no path claims are grouped per office ("no surveyed path").
An event's rating is its path's, or its strongest indicator's when the path is unrated: the
Moore 2013 path is a re-analysis line rated "N/A", and its 4,268 indicators include 9 EF5s.

**Data problems found and handled.**
- The service writes -99 for a value left out (width, peak wind); it is now read as missing
  (`missing_values_and_unlinked_guids_are_none`).
- One EF0 path recorded as 1.1 mi had a vertex tens of miles away, drawn as a line across the
  map. A path is now drawn split wherever one segment is more than twice the recorded length and
  over 10 km (`a_stray_vertex_breaks_the_drawn_path`).
- A re-analysed path can carry no casualties (Moore's reads 0 of 24 killed). Deaths and injuries
  are shown only when the path record states them.
- The DAT barely holds some older events: Joplin 2011 has 10 indicators, strongest EF2, and no
  path, so it is not among the notable days. El Reno 2013 has 458 indicators and no path.

**Live** (`moore_survey_day_live`, `notable_days_have_surveys_live`, desktop test build): 2013-05-20
read 16 events, 10 paths and 4,392 indicators; the Moore path's event is EF5 with 4,268. Every
notable day has surveys, from 9 paths (Vilonia, 2014) to 69 (10 December 2021).

**Browser build on WebGPU, one machine**, built from the working tree at `4ead1338` with this
change and another session's uncommitted TGFTP work, an 800 by 625 pane, Oklahoma City.

| Capture | What it shows |
| --- | --- |
| `moore-day-browser.jpg` | By day, Monday 20 May 2013: 10 surveyed paths, 4,392 damage indicators, strongest rated EF5. The Moore event picked ("EF5 2:04 PM CDT OUN 17.5 mi · 4268 indicators"): length 17.5 mi, 4,268 indicators (EF0 1,597 to EF5 9, 501 unrated), the survey's re-analysis comment, and the EF5 FR12 indicators first; the path outlined on the map. |
| `moore-ef5-indicator-browser.jpg` | An EF5 indicator from the list: the map centred on it at zoom 12, the indicators along the path, and its card, "One- or Two-Family Residences (FR12), Destruction of engineered and/or well constructed residence; slab swept clean, Estimated wind: 200 mph", with the surveyor's notes. |
| `moore-radar-at-time-browser.jpg` | After "Radar at this time": KTLX archive, VCP 12, 2:04 PM CDT 20 May 2013, framed on the path, with that afternoon's warnings. |
| `near-a-point-browser.jpg` | Near a point (35.491, -97.147): 63 surveyed paths within 25 mi since 2007, strongest rated EF3, newest first, drawn over the faint SPC record. |

| File | SHA-256 |
| --- | --- |
| moore-day-browser.jpg | `55074e9385f21e54bfee10feaae9470693284e95cfb41d6d6d2de724a234b7ca` |
| moore-ef5-indicator-browser.jpg | `3ce10cc8e43521dca1d9fb836cfde081157c6127cd184d69764a957e1870df07` |
| moore-radar-at-time-browser.jpg | `b8696c72dd0f9c1ed153d3e9776bd5053fc24f34a9cde8647adc2dbf4eb8bbc3` |
| near-a-point-browser.jpg | `883fb4ecf0dc7970d33f8680330b9603cccf139769868e361e3938bb564345bc` |

Not established: the desktop and Android apps (the window and drawing are shared code, but not
run there), a phone layout, a large outbreak day's drawing cost (27 April 2011 is 48 paths and
111 indicators in the DAT; 2 March 2020 is 4,018 indicators), the survey photo in a card, and
framing a path that the docked window covers (the window sits over the west of the map).
