# Codex / Claude handoff verification — 2026-10-14 UTC

Finalization of the shared changes based on `0b3b9fb0`, branch `feat/wsv3-redesign`.
The working tree includes both agents' work. The source inventory, pinned test binary
hash, input hashes, raw logs and exit codes are recorded in `verification.json`.

| Check | Result |
| --- | --- |
| Workspace tests, debug assertions enabled, PDB emission disabled | Exit 0: 2,754 passed, 0 failed, 243 ignored, across 40 suite reports including doc tests; app 1,506 passed/107 ignored |
| Workspace/all-target Clippy with `-D warnings`, final test source | Exit 0 |
| WASM library with configured getrandom backend | Exit 0; nine existing HookEcho warnings |
| Browser proxy and decode-worker controls | Exit 0; 17 passed |
| Offline scientific corpus integrity | Exit 0 |
| Isolated real-app archive/recovery GPU control | Exit 0 after strengthening the visible archive reference |
| Isolated real-app smooth-volume core outline, final pinned binary | Exit 0; accepted after 3.1 s, 144 changed pixels over rows 285–350 |
| Explicit revision-correlated queue/GPU control | Exit 0; one receipt→queue observation 2,274 µs, receipt→GPU callback 5,177 µs |
| Restored local web preview | Rebuilt debug executable serving port 8080; HTTP 200 |

The initial workspace command encountered the existing preview's executable lock
on Windows. The identified `--serve 8080 --web-root web` process was temporarily
stopped; the retry passed and the preview was restarted. The unsuccessful attempt
is retained. Build/check activity overlapped; single GPU samples are controls, not
performance distributions. A test-only strengthening followed the full suite:
the final isolated archive control and final Clippy cover those changed assertions.

## Archive selection and delivery ownership

`app_delivery_recovery_keeps_archive_and_sibling_frames_owned` constructs the actual
app with storage under `target/` and all public HTTP denied by a loopback proxy. A
pinned partial Denver NOAA volume drives the production message channel, receiver,
upload cache and GPU resources. A controlled primary loss, alternate generation,
late old-generation end, wrong pane, archive scrub before generation retirement,
loop playback and live resume exercise frame ownership. No operational upstream is
used; generation/provider labels are controlled test inputs.

The selected archive and sibling now use a fragment containing visible gates. The
archive must produce a real upload and non-black radar pixels before a queued
partial payload is delivered. Its volume identity, revision, source clock, acquisition
receipt, upload key and pixels then stay unchanged. Resuming live accepts the current
generation; the sibling remains unchanged. `app-delivery/observations.json` includes
the accepted-revision diagnostics, where missing transport receipts remain null.

## Core distribution over a resampled volume

Claude's unfinished smooth-volume capture control is verified using the cached full
Moore volume, KTLX 2013-05-20 20:16:43Z. The actual app builds the resampled volume and
draws gates at or above 65 dBZ from the cell-core distribution through `render_pane`.
Paired captures with/without the outline and a difference mask show its placement
spread up the storm. The mask was visually inspected alongside the visible archive
capture. Hover gestures and interactive/device behavior remain separate acceptance.

## Gates remaining open

This closes the bounded code/test handoff. Operational independent-provider failover,
compositor/display presentation observations, Android physical devices, full-app
2h/12h/24h profiles, sustained broadcast output and external GIS round trips remain
open in [roadmaps](../../../roadmaps/README.md). Historical renderer soaks and real NOAA
standalone fallback evidence retain their original scopes. No release certification
or whole-application competitor parity is claimed by these controls.
