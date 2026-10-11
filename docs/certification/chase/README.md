# Chase route: forecast along the route and the escape link (ROADMAP_NEW L3, L4)

Evidence for 1010.md V9: ROADMAP_NEW L3's "forecast radar/model fields" item and L4's
"escape-direction visualization … should integrate with route" item.

## What was checked

A browser build on WebGPU, on one machine (Windows 11, the built-in browser pane at 800×600),
on 2026-10-11 between 19:16 and 19:22 UTC. A Tornado Watch covered eastern North Carolina and
the Virginia tidewater, and a Tornado Warning was in effect near Greenville, NC.

- **Build.** `HOOKECHO_WASM_BUDGET=8000000 HOOKECHO_WASM_RAW_BUDGET=26200000 bash
  scripts/web/build.sh` from the working tree at `f692a42`, with this change and another
  session's uncommitted live-sweep sharing work (`vendor/nexrad-model`, `crates/wxdata/src/live.rs`)
  in it. Served with `target/release/hookecho.exe --serve 8080 --web-root web`.
- **Routing.** The public OSRM demo server, chosen with the route window's own button.
- **Radar.** KAKQ, live.

| Capture | Route | What the window said |
| --- | --- | --- |
| `route-forecast-clear-browser.jpg` | Durham, NC (35.879, −78.890) to Elizabeth City, NC (36.250, −76.256); 181.5 mi, 3 h 40 min | "No 40+ dBZ in the HRRR 18Z run on the route as far as it was checked: to 5:23 PM EDT, 102 mi ahead." No warning or watch polygon crossed it. |
| `route-forecast-hits-browser.jpg` | Near Greenville, NC (35.419, −77.328) to Manteo, NC (35.798, −75.666); 125.2 mi, 3 h 01 min | "Inside a Tornado Warning now"; "40+ dBZ forecast on the route 3:22 PM EDT to 3:25 PM EDT, 1 mi to 4 mi ahead; up to 44 dBZ"; "40+ dBZ forecast on the route 3:33 PM EDT to 3:37 PM EDT, 9 mi to 12 mi ahead; up to 47 dBZ"; "From the HRRR 18Z run, checked to 5:23 PM EDT, 85 mi ahead." |
| `route-storms-escape-browser.jpg` | The same route a minute later, the window scrolled | The forecast lines moved with the clock (3:22–3:26 PM, up to 45 dBZ; 3:34–3:37 PM; 84 mi). Each storm line ends with the escape link, e.g. "N2 (49 dBZ): closest 1 mi in 11 min, to your NE; crosses the route 8 mi ahead: storm in 10 min, you in 11 min; escape NW: off this route, which heads NE here". |

Both "checked to" times agree with the fetch: frames every 15 minutes from 3:15 PM EDT to the
two-hour cap at 5:15 PM, and the last frame stands for 8.5 minutes past its valid time.

## Tests

- `cargo test -p wxdata --lib route::` — 12 passed, 1 ignored (network). The new
  `forecast_tests` cover a forecast storm met only where the vehicle is at that frame's time,
  frames that stop short of the route's end, a missing frame or missing value counted as a gap
  rather than a hit, and a crop that keeps every value in place.
- `cargo test -p wxdata --lib live_hrrr_frames -- --ignored` — passed at 19:00 UTC: five frames
  from the HRRR 17Z run, 15 minutes apart, cropped to 51×38 cells over central Oklahoma
  (`target/parity-review/l3/live-hrrr-frames.log`).
- In a clean worktree at `f692a42` with only this change applied (no other session's edits),
  with its own target directory:
  - `cargo clippy --workspace --all-targets -- -D warnings` — clean;
  - `cargo check -p hookecho --lib --target wasm32-unknown-unknown` — the baseline 1 + 10
    warnings;
  - `cargo test --workspace --no-fail-fast` — 2,618 passed, 0 failed, 207 ignored.
- In the shared checkout, five hdf5lite tests first failed because their test binaries were
  stale: built in a since-deleted worktree whose path was compiled in. After
  `touch crates/hdf5lite/tests/*.rs` all 8 pass.

Logs are in `target/parity-review/l3/` (`wt-*.log` for the clean worktree).

## Hashes

```text
c6f6e7a4de988eb4c467254d17001227675b4fbd2b76c7df44292fe27a279f06  route-forecast-clear-browser.jpg
5910160d081c7652c7ac6a6a55eae07139f6180567d51c65b05e9bafcfb5b237  route-forecast-hits-browser.jpg
286cb8fc67103abe1f0dca7fe4145b5416c5be44ddf46f4b894afe36e9a998d6  route-storms-escape-browser.jpg
```

## Limits

- The reported stretches were not compared against the HRRR frames by an independent tool. The
  unit tests check the method; the live run shows it working end to end.
- The forecast is sampled at the nearest HRRR cell to each point, every ~200 m, at the route's
  average pace. Traffic, stops and the model's placement error are not modelled, and the window
  says it is a model forecast.
- Only HRRR composite reflectivity is checked. Other model fields (UH, precipitation) are not.
- Not run in the desktop app or on Android.
- The local web build, without `wasm-opt`, came out at 26,203,828 bytes raw: 3,828 bytes over
  the local budget in 1010.md §2 and 10,572 bytes under Cloudflare's 25 MiB limit. CI runs
  `wasm-opt` with an 18,000,000-byte budget, which this does not approach. The bundle question
  is 1010.md V5, which waits on the maintainer.
