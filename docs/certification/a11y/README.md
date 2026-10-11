# Accessibility evidence (ROADMAP_NEW Q1, Q3; 1010.md V11)

## Warning polygons say their kind in words and line (2026-10-11)

On the map, every National Weather Service warning polygon used to be drawn the same way: a
1.6 px solid outline with a translucent fill, told apart only by the NWS product colour. Red
tornado warnings and green flash flood warnings are the textbook pair that someone with
red-green colour blindness cannot tell apart, and orange severe thunderstorm warnings sit close
to both.

Now:
- **Words on the polygon** (`wxdata::alerts::map_tag`). Each warning, watch and special weather
  statement carries a short tag, white on a dark chip edged in the product colour:
  - "TOR", "SVR", "FFW";
  - the rest by name ("FLOOD", "SNOW SQUALL", "EXTREME WIND", "TOR WATCH", "SPS");
  - the tags that change what a warning means: "TOR PDS", "TOR EMERGENCY", "SVR DESTRUCTIVE",
    "FFW EMERGENCY".

  The most severe are placed first. No tag overlaps another. A tag goes only on a polygon big
  enough on screen to hold it, at a point inside it: the area centroid, or the middle of the
  visible part (`tag_anchor`). Advisories are not tagged. Tags follow the user's alert filters,
  because they are drawn from the same shown set as the polygons.
- **Line by kind** (`overlay_build::alert_stroke`):
  - a tornado warning is drawn at 3.4 px;
  - severe thunderstorm, flash flood, snow squall and extreme wind warnings at 2.4 px;
  - watches dashed;
  - everything else keeps the 1.6 px edge.

  This applies only to features that carry alert metadata, which every alert feed's do (NWS
  live and zone, the archive, NWWS, ECCC, MeteoAlarm). The GPU golden `snapshot_warnings`
  builds its two polygons without that metadata, so it still pins the base style unchanged. The
  new widths are pinned by `an_alerts_outline_says_its_kind` and
  `the_outlines_reach_the_geometry`.

**Walkthrough** in the browser build (WebGPU, one machine, 2026-10-11 21:15–21:23 UTC), built
from the working tree at `d4a7119` with this change and another session's uncommitted live-sweep
sharing work:

| Capture | What it shows |
| --- | --- |
| `warning-tags-moore-archive-browser.jpg` | KTLX archive, 2013-05-20 20:12 UTC (the Moore tornado), from the archived warnings: "TOR" on the Moore tornado warning by Oklahoma City, "TOR" and "SVR" on the storm to the south, each tornado outline visibly heavier. The Moore warning reads "TOR", not "TOR EMERGENCY". The archived warning record carries no damage-threat tag and not the tornado-emergency wording, so the tag cannot say more than "TOR". |
| `warning-tags-flood-live-browser.jpg` | Live KTLH and KEVX, the Florida panhandle: "FLOOD" on a live Flood Warning. At a regional zoom the same polygons were too small for a tag and carried none, by design. |

Tests:
- `a_polygon_says_what_kind_of_alert_it_is`: every tag and threat variant, lower-case agency
  names, untagged advisories;
- `an_alerts_outline_says_its_kind`;
- `the_outlines_reach_the_geometry`;
- `a_tag_goes_inside_its_polygon_on_the_pane`: a box, one too small, an L shape, a polygon
  half off the pane.

Checks in a clean worktree at `d4a7119` with only this change:
- `cargo clippy --workspace --all-targets -- -D warnings` — clean;
- the wasm library check — at its baseline;
- `cargo test --workspace` — 2,632 passed, 0 failed, 208 ignored.

Logs: `target/parity-review/v11-tags/wt-*.log`.

Limits:
- Not checked with a colour-vision simulator, or by people with colour-vision deficiency.
- SPC watch boxes carry no alert metadata, so they get no tag or dash.
- Tags are skipped on small polygons at regional zooms. The alert list and the polygon's own
  click still name it there.
- Not run in the desktop app or on Android.

## Earlier captures

| File | Commit | SHA-256 |
| --- | --- | --- |
| `toast-glyph-and-bell-badge.png` | `b71943a`: the alert bell and toasts stop saying things by colour alone | `d485fc96f49b519354a1800fb4c40d05924b522d3e35d3ab435035c174d7578f` |
| `tablet-touch-workstation.png` | `9afa3d8`: the touch-sized workstation | `482d70b1ff0f9166fa87a87618ad8f5c72376239d90d81e1a97fcbf9bf790077` |
| `warning-tags-moore-archive-browser.jpg` | this pass | `76e7ca7188a44c9304ce3916dd73a9534dbb42f89593c01362f55d01da7f0015` |
| `warning-tags-flood-live-browser.jpg` | this pass | `366ba9da2e1871b24584f1d4b1946ba93d3986f5bb8373b76f7aff5628c0ff5f` |
