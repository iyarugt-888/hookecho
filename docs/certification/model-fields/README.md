# Model fields as contour lines (ROADMAP_NEW F4)

Evidence for 1010.md V16: contours for a browsed model field, offered by its quantity's
metadata.

## What was checked

A browser build on WebGPU, on one machine (Windows 11, the built-in browser pane at 800×600),
on 2026-10-11 between 19:50 and 20:04 UTC. Built with `HOOKECHO_WASM_BUDGET=8000000
HOOKECHO_WASM_RAW_BUDGET=26200000 bash scripts/web/build.sh` from the working tree at `0bdbc18`
with this change and another session's uncommitted live-sweep sharing work in it, and served with
`target/release/hookecho.exe --serve 8080 --web-root web`.

Model fields (search "model fields") → HRRR → a field → "Contour lines every …":

| Capture | Field | What it shows |
| --- | --- | --- |
| `hrrr-500hpa-height-contours-browser.jpg` | HRRR 19Z run, 500 hPa geopotential height, valid 20:00 UTC | The switch read "Contour lines every 6 dam". Isohypses every 6 dam over the shading, labelled 552 to 594: the ridge over the southern states and the trough into the Northwest. The legend note ends "contours every 6 dam". Final code. |
| `hrrr-850hpa-temperature-contours-browser.jpg` | HRRR 18Z run, 850 hPa temperature, valid 19:00 UTC | "Contour lines every 2 °C". Isotherms labelled 16 to 26 °C across the Plains, matching the shading. One build earlier: the same code apart from dropping sub-pixel points before drawing. |

Checked along the way:
- The switch stayed on when another field was picked (850 hPa temperature to 500 hPa height),
  and changed to that field's own interval. A shaded-only quantity (no switch, "This quantity is
  shaded only, not contoured") is covered by the unit tests, not looked at in the browser.
- The first build drew no labels, because a line was labelled only when one segment spanned
  60 px, which a finely sampled contour never has. Lines are now labelled half way along their
  length when they are at least 120 px long on screen. The HRRR contour layer shares the painter,
  so its lines gain labels too.

## Tests

- `cargo test -p wxdata --lib model_inventory` — 6 passed, including
  `contour_intervals_come_from_each_quantity`: heights 6 dam, pressure 4 hPa, temperature 2 °C,
  wind 10 kt; reflectivity, cloud cover, visibility, updraft helicity, rain, VIL and echo top
  are not contoured.
- `cargo test -p hookecho --lib -- isoline_label model_field model_pane` — 23 passed:
  - which picks are contoured (a difference never is);
  - contours made on a bounded grid at the field's own levels and place;
  - missing cells leave gaps;
  - the switch is saved only when on, and older workspaces read it as off;
  - labels half way along a finely sampled line;
  - thinning keeps both ends of a line.
- In a clean worktree with only this change applied: see "Clean worktree" below.

## Clean worktree

At `0bdbc18` with only this change applied (no other session's edits), with its own target
directory:
- `cargo clippy --workspace --all-targets -- -D warnings` — clean;
- `cargo check -p hookecho --lib --target wasm32-unknown-unknown` — the baseline 1 + 10 warnings;
- `cargo test --workspace --no-fail-fast` — 2,624 passed, 0 failed, 207 ignored.

Logs: `target/parity-review/f4/wt-*.log`.

## Hashes

```text
42127fc48dd262ce4df02667aab8f553b202402245f886591b2ff35db2200306  hrrr-500hpa-height-contours-browser.jpg
a8dffed7e06ecb20e9d8b22fb4d47cb55bf1ab747db91cd272629078afb4a84a  hrrr-850hpa-temperature-contours-browser.jpg
```

## Limits

- Only the browsed field ("Model fields") is contoured this way. Streamlines and arrows for every
  vector field (the rest of F4) are not done.
- Contours are made on a grid at most 900 cells across. That is plenty for these smooth fields,
  but it is coarser than the HRRR's own 3 km.
- Over the high West, 850 hPa is below the ground. The HRRR still writes values there, so the
  isotherms over the Rockies are as unphysical as the shading under them. Neither is masked yet.
- Frame cost was not measured. The browser pane was hidden for much of the check, and a hidden
  pane is throttled to frames of one to two seconds. That set off the app's slow-frame guard
  ("Visual quality reduced") once, so frame timing here says nothing about the contours.
  Points less than 1.5 px apart on screen are now dropped before drawing.
- Not run in the desktop app or on Android.
- The local unoptimized bundle is 26,223,396 bytes raw, over Cloudflare's 25 MiB limit.
  CI and the deploy run `wasm-opt`, about 38 % smaller, against an 18,000,000-byte budget. The
  bundle question is 1010.md V5.

## A browsed field at a point over its run (2026-10-12, 1010.md V16, ROADMAP_NEW F4)

In the pinned layer probe, a regional model's browsed field has "Graph this point over the run"
(`app/field_series.rs`). It fetches the same message (variable, level, timing) at every lead of
the run the pane shows, pinned to that one run, four at a time, and graphs its value at the point
in the field's display units (`forecast_window::paint_series`, shared with the global-model
meteogram). A lead the run does not have is a gap in the line.

**Browser build on WebGPU, one machine**, live, built from the working tree at `f0b7bdd` with
this change and another session's uncommitted work. HRRR 2 m temperature, probe pinned at
34.951, −95.215:

| Capture | What it shows |
| --- | --- |
| `point-graph-hrrr-t2m-browser.jpg` | "HRRR Temperature, 2 m above ground · 34.951, -95.215 · °C · run 2026-10-12 10Z": 20 °C at 6 AM rising to 32 °C in the afternoon and 30 °C at the last lead. "19 leads, one run; 5 without a value here (gaps in the line)": the run was about an hour old, so the last leads were likely not yet posted. |

| File | SHA-256 |
| --- | --- |
| point-graph-hrrr-t2m-browser.jpg | `4e16dc65a47b1fae7009b40f536bdea9604eb2a11348c053afe66aa2471d1261` |

The first click on the button moved the probe's pin instead of pressing it; later clicks
pressed it without moving the pin. Why is not established. Not established either: the desktop
app, Android, and global models (refused here; the forecast window graphs them).

## Wind particles from a browsed wind (2026-10-12, 1010.md V16, ROADMAP_NEW F4)

With a wind picked in "Model fields", the window offers "Drift wind particles with this wind"
(`app/model_particles.rs`). While it is on, the animated particles take their field from the
active pane's accepted browsed wind, the same east/north components its barbs are drawn from,
instead of the HRRR 10 m wind; the HRRR 10 m fetch stops, and turning on radar winds turns the
switch off. A new lead or level replaces the field once it is accepted; the last one keeps
drifting while it loads, and the particles stop when the pane no longer shows a browsed wind.

**Live, headless, one machine** (`browsed_wind_particles_live`, ignored by default): the HRRR
2026-10-12 10Z run at F+1, fetched as a browsed wind is (`hrrr::fetch_inventory_vector`, 850 hPa)
and as the 10 m particle layer is (`hrrr::fetch_wind`), each run through the particle engine
(`wind_draw::Particles`, the CPU path) for 90 frames over Oklahoma at zoom 6. Mean wind over the
view: 10 m 4.5 m/s from 195°, 850 hPa 16.4 m/s from 225°. The browser probe read the same 850 hPa
wind as "35.5 from 224°" (kt) at 34.81, −98.62.

**Browser build on WebGPU, one machine**, live, built from the working tree at `98ff55d` with
this change and another session's uncommitted work: the switch is offered once a wind is picked,
and turning it on brings up the particles' "Wind (kt)" key under the field's own.

| Capture | What it shows |
| --- | --- |
| `particles-hrrr-850-vs-10m-live.png` | Left, the HRRR 10 m particles, short and from the south. Right, the 850 hPa particles, long and from the southwest, fastest (magenta) in the northwest of the view. |
| `particles-switch-offered-browser.jpg` | "Model fields" with "Wind · 850 hPa · instant (kt)" picked: "Contour lines every 10 kt" and "Drift wind particles with this wind" (off), barbs over the speed. |
| `particles-switch-on-browser.jpg` | After turning it on: the "Wind (kt)" particle key (0–50) under the 850 hPa key. |

| File | SHA-256 |
| --- | --- |
| particles-hrrr-850-vs-10m-live.png | `b4153001b388a7f7204243f176227a8688adab3afb1fd33f9b74258c525dd3f4` |
| particles-switch-offered-browser.jpg | `12e183e17df7b58d2dbf406602be6a6ac5c292e77dacd4e73b45cfce1b73f5c9` |
| particles-switch-on-browser.jpg | `a02a699c200f5751c60715c546b44ab7c254aee0ac485f08adcbec97025162c5` |

Not established: particles moving on screen in the app. The browser pane was hidden for the whole
run and drew about one frame in several seconds (18.6 s at the end), too few for trails, so the
browser captures show the switch and the key, not drifting particles. Nor the desktop app,
Android, or the GPU particle path with a browsed wind; that path packs each component into
±40 m/s, so a jet faster than that drifts as 40 m/s there.
