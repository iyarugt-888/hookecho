# M6.4 output window restore and OBS guide (1008.md F3)

The output window's open state, size, fullscreen setting and title strap are kept in settings
(`output_window`) and restored at launch, so an OBS window capture of "HookEcho Output" finds it
again after a restart without anyone reopening it. A phone or browser that receives the same
settings never opens one. [docs/obs-guide.md](../../obs-guide.md) is the setup guide.

## Real-app check, desktop (Windows, RTX 2060), 2026-10-09

The debug build was launched sandboxed (`HOOKECHO_SANDBOX=1`, its own settings) with
`"output_window": {"open": true, "size": "free", "strap": "Restart check"}` saved. Forty seconds
after launch the process had two top-level windows, "HookEcho" and "HookEcho Output";
[output-restored.png](output-restored.png) is the output window captured with `PrintWindow`: live
KTLX reflectivity with the clock, source caption, colour scale, warning crawl and the restored
strap. The saved settings were written back unchanged.

| File | SHA-256 |
| --- | --- |
| output-restored.png | `f80881b847d832bc000a1f31c4e8c789c6a7eb3d2cd1a56a5091401e99002254` |

This check also found that every debug launch panicked at startup: the "Wind streamlines" layer
row was filed under a category the layers panel does not draw (fixed in `1b39538`).

## Not established

- OBS itself was not run; the guide follows the app's behaviour.
- Remembering the program pane and held view, and a monitor chosen from a list (the window's
  position, and so its monitor, is remembered since 2026-10-12, below).
- Android share sheet for stills and loops; GIF from the control strip; Android walkthrough.

## The output window reopens where it was (2026-10-12, 1008.md F3)

The output window's outer top-left is kept with its other settings (`OutputPrefs::position`, in
points, not while fullscreen, whose origin is the monitor's corner) and asked for when it opens
(`ViewportBuilder::with_position`). Fullscreen fills the monitor the window is on, so a window put
on a monitor and made fullscreen comes back fullscreen on that monitor. Settings written before
there was a position still read (`the_output_window_reopens_as_it_was_left`).

**Windows desktop app, debug build, sandboxed, one machine**, built from the working tree at
`745ad64d` with this change and another session's uncommitted work
([output-position-desktop.txt](output-position-desktop.txt)):

1. With the window saved open at 420,260, the app opened "HookEcho Output" at 420,260.
2. The window was moved to 640,140 (`SetWindowPos`) and the app closed normally (`WM_CLOSE`):
   the saved position read `[640,140]`.
3. Started again, it opened at 640,140 ([output-reopened-where-left-desktop.jpg](output-reopened-where-left-desktop.jpg):
   live KTLX with the clock and caption).

| File | SHA-256 |
| --- | --- |
| output-position-desktop.txt | `de26b2689ac5abe1089925c126fa99b5b658c680a9c7a3a539230b147511d226` |
| output-reopened-where-left-desktop.jpg | `e7383581fd58a515dc7b20556908c4a56ec492536a7cbbd59ffa10150b8fde1e` |

Not established: a second monitor (this machine has one), fullscreen across a restart, a monitor
that has gone since, and OBS itself.
