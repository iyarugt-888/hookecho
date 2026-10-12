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
| `warning-tags-moore-archive-browser.jpg` | KTLX archive, 2013-05-20 20:12 UTC (the Moore tornado), from the archived warnings: "TOR" on the Moore tornado warning by Oklahoma City, "TOR" and "SVR" on the storm to the south, each tornado outline visibly heavier. The Moore warning reads "TOR", not "TOR EMERGENCY". That conclusion was wrong: see "Short tags" below. At this zoom the emergency polygon was too small for its long tag, so it was skipped and an overlapping plain warning's "TOR" showed. |
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
- Not checked by people with colour-vision deficiency. The simulation is below.
- SPC watch boxes carry no alert metadata, so they get no tag or dash.
- Tags are skipped on small polygons at regional zooms. The alert list and the polygon's own
  click still name it there.
- Not run in the desktop app or on Android.

## Short tags, and tags before town names (2026-10-11, 1008.md E4)

- **Town names give way.** Warning tags now reserve their place in the frame's shared label placer
  at warning priority, before town names. Before, they kept their own list and were drawn after
  the names, so a name could sit on a tag.
- **A tag that does not fit falls back.** On the Moore archive, zoomed in to 9.6, the polygon
  reads "TOR EMERGENCY" (`warning-tags-moore-zoomed-browser.jpg`), and "Moore" is not drawn
  under it. So the archive does carry the emergency, and the first walkthrough above was
  misread. At zoom 8.6 that tag was too long for its polygon on screen: it was skipped while an
  overlapping plain warning kept its "TOR", understating the most severe warning in view. A tag
  that does not fit now falls back to a short form with the alert bell's marks
  (`alerts::short_tag`): "TOR!!" for an emergency, "TOR!" for PDS, "SVR!", "FFW!!". The same
  zoom 8.6 view now reads "TOR!!" (`warning-tags-moore-short-browser.jpg`).

| File | SHA-256 |
| --- | --- |
| `warning-tags-moore-zoomed-browser.jpg` | `36989d3b8388547b08341f04cd9614e7e01c1dced3f903db31c19ceda6e1620d` |
| `warning-tags-moore-short-browser.jpg` | `b8bf4bf5dcec8cf4f35d851ae75678741b5039c8703e0d48258652d87c187542` |

## Colours under simulated colour-vision deficiency (2026-10-11)

`wxdata::cvd` simulates protanopia, deuteranopia and tritanopia (Machado, Oliveira and Fernandes
2009, at full severity, in linear RGB). It measures how far apart two colours are as CIE76 ΔE
in CIELAB (D65). About 2.3 is just noticeable; under about 10, two colours are easily taken for
each other at a glance. The figures below are the tests' own output
(`target/parity-review/v11-cvd/cvd-numbers.log`).

**The NWS warning colours, which is why the polygons above now carry words and line**
(`the_warning_colours_collapse_for_a_red_green_dichromat`):

| Pair | Normal vision | Protanopia | Deuteranopia |
| --- | --- | --- | --- |
| Tornado / Flash Flood Warning | 152.8 | 43.2 | **8.9** |
| Severe Thunderstorm / Flash Flood Warning | 96.3 | **4.0** | 18.6 |

**The colour-blind-safe tables** (`the_safe_velocity_table_keeps_inbound_and_outbound_apart`,
`the_safe_reflectivity_table_brightens_with_dbz_for_everyone`):

| Velocity pair (worst of normal and the three dichromacies) | Safe blue/orange | Default |
| --- | --- | --- |
| −5 / +5 m/s | 23.3 | 21.6 |
| −10 / +10 m/s | 46.6 | 19.5 |
| −25 / +25 m/s | 83.7 | 79.5 |

- The safe table is asserted to keep the pair apart: ΔE above 15 at ±5 m/s and above 30 from
  ±10 m/s. Each side is also more than 10 from the grey at zero, from ±10 m/s.
- The default red/green table holds up at ±25 m/s, because its colours also differ in lightness.
  At moderate speeds it falls to 19.5 at ±10 m/s, against 46.6 for the safe table.
- The safe reflectivity table (viridis) is asserted to grow lighter with every 5 dBZ step from
  10 to 70 dBZ, by more than 50 L* overall, under normal vision and each simulation.

Limits:
- A model of a typical dichromat at full severity, not of anomalous trichromats or real viewers.
- ΔE76 overstates differences between saturated colours.
- The other built-in and community tables are not checked.

Checks in a clean worktree at `d000a03` with only this change: clippy clean, the wasm library
check at its baseline, workspace tests 2,637 passed, 0 failed, 208 ignored
(`target/parity-review/v11-cvd/wt-*.log`).

## Earlier captures

| File | Commit | SHA-256 |
| --- | --- | --- |
| `toast-glyph-and-bell-badge.png` | `b71943a`: the alert bell and toasts stop saying things by colour alone | `d485fc96f49b519354a1800fb4c40d05924b522d3e35d3ab435035c174d7578f` |
| `tablet-touch-workstation.png` | `9afa3d8`: the touch-sized workstation | `482d70b1ff0f9166fa87a87618ad8f5c72376239d90d81e1a97fcbf9bf790077` |
| `warning-tags-moore-archive-browser.jpg` | this pass | `76e7ca7188a44c9304ce3916dd73a9534dbb42f89593c01362f55d01da7f0015` |
| `warning-tags-flood-live-browser.jpg` | this pass | `366ba9da2e1871b24584f1d4b1946ba93d3986f5bb8373b76f7aff5628c0ff5f` |
