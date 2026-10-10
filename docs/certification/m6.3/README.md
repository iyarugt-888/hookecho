# M6.3 repeat-run determinism (1008.md F2)

The same archived loop was rendered twice through the off-screen renderer `--watch` uses, on
Windows with the RTX 2060 (debug build, 2026-10-09):

```bash
hookecho --watch --site KTLX --from 2013-05-20T19:50 --to 2013-05-20T20:20 --basemap none --size 512 --interval fixed --fps 4 --out moore-a.gif
```

Then the same command again, writing `moore-b.gif`.

| | Run A | Run B |
| --- | --- | --- |
| Exit, time | 0, 46 s | 0, 43 s |
| GIF SHA-256 | `333532acf125ff3c901f85942950498f210b332b0b4a404277a5b15081a2cf04` | `333532acf125ff3c901f85942950498f210b332b0b4a404277a5b15081a2cf04` (identical) |
| Sidecar | [moore-a.json](moore-a.json) | [moore-b.json](moore-b.json) |

The two GIFs are **byte-identical**. The sidecars agree in every key except `rendered_utc`, the
wall-clock time of the render. That includes the 7 frames, each frame's volume, valid time,
start and hold, and the 2,250 ms duration.

[moore-a.gif](moore-a.gif) is the loop (KTLX 19:51–20:16 UTC, 0.5° reflectivity, no basemap).

## Scope

- **Covered:** the off-screen path (`--watch`, `--serve`). The radar inputs are archived volumes;
  `--basemap none` keeps map tiles fetched from the network out of the comparison.
- **Not covered:**
  - the in-app loop export, which screenshots the running window;
  - another GPU or driver.
  - (A basemap: covered on 2026-10-10, below.)
- **Not run:**
  - a sustained 30-minute capture;
  - a real ffmpeg encode at 60 fps (ffmpeg is not installed here);
  - FTP/SFTP delivery.

## With a basemap (2026-10-10)

The same loop with the dark vector basemap, rendered twice on the RTX 2060 (debug build,
`feat/wsv3-redesign` after `9af1aee`):

```bash
hookecho --watch --site KTLX --from 2013-05-20T19:50 --to 2013-05-20T20:20 --basemap dark --size 512 --interval fixed --fps 4 --out moore-dark-a.gif
```

| | Run A | Run B |
| --- | --- | --- |
| Exit, time | 0, 194 s (including the build and the first tile fetch) | 0, 62 s |
| GIF SHA-256 | `b822df49a800d3fd72a418d75711897b8d6bd389712ba75e26e3e203bef80828` | the same (byte-identical) |
| Sidecar | [moore-dark-a.json](moore-dark-a.json) | [moore-dark-b.json](moore-dark-b.json) |

The sidecars differ only in `rendered_utc`; all 7 frames agree. The basemap came from
OpenFreeMap's planet build `20261004_113936_pt` (9 vector tiles, 1,845 labels). A later planet
build can change the map, so the result is determinism for one basemap build, not across them.
[moore-dark-a.gif](moore-dark-a.gif) is the loop.
