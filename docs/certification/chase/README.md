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

## Rotating storm tracks along the route (2026-10-11)

The same switch also fetches the HRRR's hourly maximum 2-5 km updraft helicity, the model's
rotation tracks (`route::fetch_uh_frames`). It covers the hour windows from the one holding now
through the one holding the horizon, from one run, cropped to the route. Each value is the
hour's strongest, so a frame is timed at its hour's middle and stands for the whole hour
(`forecast_along_within` with `HOURLY_FRAME_SLACK_S`, 31 minutes). Stretches at 75 m²/s² or
more are listed: the threshold the HREF's own rotation probabilities use.

- `an_hourly_window_frame_covers_its_whole_hour`: a track in the 21-22Z window, crossed 6 to 18
  minutes after 21Z, is met with the hourly slack and missed with the 15-minute one.
- `live_hrrr_uh_windows_cover_the_next_ninety_minutes` (ignored, network): HRRR 23Z run, two
  windows timed 00:30 and 01:30, 51×38 cells over central Oklahoma, strongest 0 m²/s² on a
  quiet night (`target/parity-review/l3/live-uh.log`).

Not seen in the window yet: no storms were forecast near a route while this was built.

Checks in a clean worktree at `bb845bb` with only this change: clippy clean, the wasm library
check at its baseline, workspace tests 2,649 passed, 0 failed, 213 ignored
(`target/parity-review/l3/wt-*.log`).

## Radar you already looked at, with no signal (2026-10-12, 1010.md V9, ROADMAP_NEW L5)

The desktop app keeps every archived volume it shows or prefetches in its disk cache. Without a
connection the day's volume listing fails, and before this change that left the timeline empty.
Now the app lists the volumes of that radar and day already on disk (`level2::cached_volumes`)
and says so in the feed status.

**Shown in the Windows desktop app** (debug build, `HOOKECHO_SANDBOX=1`, one machine). Every
HTTP(S) request went to a dead proxy (`HTTPS_PROXY=http://127.0.0.1:9`). The view was KHTX on
2026-09-10 at 01:10 UTC, with six of that day's volumes copied into the sandbox's cache. The
listing failed and the app logged:

> Archive frame list: KHTX 2026-09-10: list_files(KHTX, 2026-09-10): error listing AWS S3
> objects; showing the 6 volumes already on this device instead

[offline-cached-radar-desktop.png](offline-cached-radar-desktop.png): the 01:11:41 volume on
screen, played from disk, with its timeline. The basemap is blank because its tiles were not
cached either; a chase pack fills that.

| File | SHA-256 |
| --- | --- |
| offline-cached-radar-desktop.png | `971e1d2f4a3d2aaa464d2ca6d377f310fb4c66e016d49e4234ec7d4fa4ab20ea` |
| offline-cached-radar-desktop.txt | `4e5f7c10b9f2c9c248975fbea5cfce8e574bba766a0242114962d2e863b04b4a` |

Not established: the network adapter itself turned off (a dead proxy fails the same request
another way), Android, and fetching a loop ahead on purpose.

## How much keeping a loop will download (2026-10-12, 1010.md V9, ROADMAP_NEW L5)

Before "Keep this loop on this device" is pressed, its menu now says what it would fetch:
"3 of 5 volumes to fetch, about 30 MB (at the 10.0 MB mean of the 2 already on this device)."
The estimate is the mean size on disk of the loop's volumes already kept, times the number still
to fetch (`packs_soundings::keep_estimate`, re-read at most once a second; tested in
`a_kept_loops_size_is_estimated_from_the_volumes_already_kept`). With none of the loop on disk it
says the size is not known until the first volume arrives, rather than guess one. For scale, the
nine KHTX 2026-09-10 VCP 215 volumes in the sandbox cache were 9.1 to 10.2 MB each.

The docked timeline, which the workstation shows by default, had no way to keep a loop: the
button was only in the floating timeline's menu. It now has a download icon beside the archive
date that opens the same button and the estimate (`dock/timeline.rs`).

**Windows desktop app, debug build, sandboxed, one machine**, built from the working tree at
`5669a77` with this change and another session's uncommitted work, at KHTX 2026-09-10:

| Capture | What it shows |
| --- | --- |
| `keep-loop-button-desktop.png` | The docked timeline in archive, with the new download icon to the left of "Sep 10, 2026 UTC". |

| File | SHA-256 |
| --- | --- |
| keep-loop-button-desktop.png | `404c825fc9fae609691798afdbaed8352ebc7b30aa6dd1fd2e90c5b66575efc1` |

Not established: the popup opened and the button pressed in a running app. Clicks sent to the
window as messages (`PostMessage`, then `SendMessage`) did not reach its input, and nothing here
drives the real pointer. Nor the browser's saved loops, which still give no size before saving.

## A browser pack is the loop (2026-10-12, 1010.md V9, ROADMAP_NEW L5)

"Save this loop offline" in the browser saved `frames[..=playhead]`: every volume from the start
of the day to the one shown. Late in an archive day that is hundreds of volumes, each read into
memory before the pack is written, against the 250 MB that all packs share. A pack is now the
loop, as the desktop keeps it: the last loop-length volumes up to the playhead, less the live
head while the timeline follows it, which the old comment promised but the code did not do
(`packs_soundings::pack_frames`; `a_browser_pack_is_the_loop_without_the_live_head`). The docked
timeline, the workstation's default, had no way to save or load a pack; its download icon now
opens the button, the saved packs (load, delete) and the progress line, as the floating
timeline's menu does.

**Browser build on WebGPU, one machine**, live, built from the working tree at `094301b` with
this change and another session's uncommitted work, at KHTX 2026-09-10, playhead on frame 17
(01:25 UTC), loop length 10:

| Capture | What it shows |
| --- | --- |
| `pack-save-offered-browser.jpg` | The docked timeline's download icon open: "Save this loop offline (10 frames)". |
| `pack-saved-browser.jpg` | After saving: "saved KHTX 2026-09-10 — 10 volumes, 86 MB", and the pack listed with its delete cross. The old code would have saved 18. |
| `pack-loaded-browser.jpg` | The pack loaded back from IndexedDB: the loop from 00:44 to 01:25 UTC playing, buffer 6/10 a few seconds in. |

| File | SHA-256 |
| --- | --- |
| pack-save-offered-browser.jpg | `88d5714cac25e2357189799c3661beda32a5301ff9ce874f8998fc9592d6d48f` |
| pack-saved-browser.jpg | `f8c7172609bca9b87072742aef6eacb78f7b19b11780b3d2d5050d1504c99d57` |
| pack-loaded-browser.jpg | `7deef6eeba25ca278b4e646eefcf7725b5b8030c7fd59af20c8cfb5ebf01b08f` |

Not established: a pack played with the network off (the volumes were also in the browser's
automatic volume cache), Android's WebView, and a size before saving; a browser pack still
shows its size only once saved.

## A browser pack's size before saving (2026-10-12, 1010.md V9, ROADMAP_NEW L5)

Before "Save this loop offline" is pressed, the browser now says what the pack would hold and
how much of it must be downloaded (`packs_soundings::pack_estimate`, tested in
`a_browser_packs_size_is_estimated_before_saving`). The sizes come from what this browser
already holds: each entry of the automatic volume cache as its bookkeeping records it, and each
saved pack's volumes at that pack's mean (`webcache::known_volume_sizes`, read once, then kept in
step with the cache's writes and evictions and refreshed after a save). A volume of unknown size
is counted at the mean of the known ones; with none known it says the size is not known yet.
Megabytes are the pack label's (2^20 bytes), so the estimate and the saved line compare. The save
now also reads a volume from the automatic cache before downloading it; it used to look only in
the pack store, and so fetched again a volume the browser had just played.

**Browser build on WebGPU, one machine**, live, built from the working tree at `673959e` with
this change and another session's uncommitted work, at KHTX 2026-09-10, frame 22 (01:48 UTC),
loop length 10, with the earlier pack (frames 8 to 17) still saved:

| Capture | What it shows |
| --- | --- |
| `pack-estimate-browser.jpg` | Before saving: "10 volumes, about 93 MB; 3 to download, about 28 MB (at the 9.3 MB mean of the 7 already in this browser)." |
| `pack-estimate-saved-browser.jpg` | After saving: "saved KHTX 2026-09-10 — 10 volumes, 96 MB"; the estimate is 3% under. The line above now reads "10 volumes, 97 MB, all already in this browser: nothing to download." (the old pack's volumes are counted at its mean, hence 97). |

| File | SHA-256 |
| --- | --- |
| pack-estimate-browser.jpg | `54b53dc3f8d17ab93e7405e1823ed81e238f888c053bdddf04ad8858339afe54` |
| pack-estimate-saved-browser.jpg | `72d4c3970d0421644e06686df620e1279b9b9daf2c8a487726dd3e2c54d22e64` |

Saving a loop of the same radar and day replaces that day's pack, as before: here the 01:48 loop
took the place of the 01:25 one. Not established: how many of the 7 came from the automatic
cache rather than the old pack (nothing here counts the reads), and Android's WebView.
