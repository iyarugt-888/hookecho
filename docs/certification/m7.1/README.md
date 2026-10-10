# M7.1 render soak (1008.md H2)

`hookecho --soak SITE [MINUTES] [--inject] [--render] [--jsonl PATH]`.

**`--render`** adds the renderer to the data-path soak. Each new volume is drawn by the map
renderer on one long-lived GPU device, as a pane keeps one. A scenario clock rotates the product
(REF, VEL, CC, ZDR) and the tilt (the lowest four). The 3D smooth volume is then built and
uploaded. The GPU's allocated memory (the backend's allocator report) is tracked like resident
memory: growth after warm-up fails the soak, and so does any failed render.

**`--jsonl`** writes one timestamped line per cycle after a header naming the site, profile,
build and adapter.

## The 2-hour profile, live KTLX, RTX 2060 (Vulkan), 2026-10-09 16:51–18:51 UTC

```bash
hookecho --soak KTLX 120 --render --inject --jsonl soak-ktlx-2h.jsonl
```

Release build, `0.12.0-beta.2`. **Passed (exit 0).**

| | |
| --- | --- |
| Cycles (every 30 s) | 240 |
| New volumes, decoded and binned | 18 (clear-air cadence) |
| Failed cycles | 0 |
| Cut-off volumes injected / refused | 5 / 5 |
| Renders / failed | 18 / 0, all four products |
| Draw, p50 / max | 4.4 / 58.4 ms |
| 3D build, p50 / max | 14.2 / 373.9 ms |
| 3D upload, p50 / max | 8.8 / 86.0 ms |
| Decode, p50 / max | 84 / 236 ms |
| GPU allocated: baseline / max / end | 9.55 / 9.78 / 9.55 MB |
| Resident memory: baseline / max / end | 119 / 131 / 90 MB |

Notes on reading it:

- **CPU contention:** the maxima (354 ms binning, 374 ms 3D build, 236 ms decode) are
  all on one early cycle, which coincided with `cargo` compiling on the same machine. The run was not isolated.
- **Resident memory:** Windows trims the working set of a mostly idle process. That is why
  resident memory drops to as little as 26 MB between volumes; it is not a measure of the
  allocations.
- **GPU memory:** this is the allocator's sub-allocated total (gpu-allocator under Vulkan). It
  stayed flat to within 0.25 MB over 18 rotations.

| File | SHA-256 |
| --- | --- |
| [soak-ktlx-2h.jsonl](soak-ktlx-2h.jsonl) (one line per cycle) | `29f7beff42c3afadcaaf97b01f16e128b15f1b4092029691fe053793cfa57520` |
| [soak-ktlx-2h.log](soak-ktlx-2h.log) (console and final summary) | `492f421030ed79f2481cad62ba68b976843ba9cbe9df9fbaae2873ad15e90abd` |
| [summary.json](summary.json) (runner summary and derived figures) | `bb6adaaf7122b0531dd23532034691bd4482099ed1de389e3096be86eb8b9786` |
| [render-profile.txt](render-profile.txt) (40 offline cycles on the Moore 2013 volume, GPU flat at 9.5 MB) | `449fdc7caee61d11a29a72f0d57ac6c81dbf07827062504829a0025c0cb93ec1` |

## A 12-hour attempt, interrupted at 4 h 15 min

The same command with 720 minutes started at 18:54 UTC on 2026-10-09. It stopped at 23:10 UTC
when the session that launched it ended and its process was torn down. That was not a soak
verdict: the runner never printed a summary or an exit status.

Up to that point:

| Measure | Value |
| --- | --- |
| Cycles | 512 |
| New volumes | 38 |
| Failed cycles | 0 |
| Renders | 38, across REF/VEL/CC/ZDR, 0 failed |
| Draw p50 / max | 4.2 / 95.1 ms |
| GPU allocated | 9.55–9.78 MB, ending at 9.55 MB |

It is kept as partial evidence. It is not a completed 12-hour profile.

| File | SHA-256 |
| --- | --- |
| [soak-ktlx-12h-interrupted.jsonl](soak-ktlx-12h-interrupted.jsonl) | `888df3b5acf69cf5568d142711feb9ab0c790792f6f3bb5a6f47eee5bf72ed4a` |
| [soak-ktlx-12h-interrupted.log](soak-ktlx-12h-interrupted.log) | `9f80001d635d690d4a4c8b380012f1f069c48dd505d7a06a2dd098a6fa54a1a1` |

A second 12-hour run was started at 14:32 UTC on 2026-10-10 as a detached process, so it does
not depend on a session staying open.

## Not established

- **Long profiles:** no 12-hour profile has completed yet, and the 24-hour profile has not run.
- **Scope of the soak:** it drives the renderer, not the app's UI. Window frame pacing,
  presentation, panes, playback, GIS and output are not exercised.
- **Untested faults:** device or surface loss, offline/reconnect, and model or ABI failures are
  not injected.
- **Weather:** a severe-weather cadence has not been soaked. This run was clear air, about one
  volume every 6–7 minutes.
- **Android:** no Android soak.
