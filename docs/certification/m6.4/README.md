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
- Monitor selection, and remembering the window's position, monitor, program pane and held view.
- Android share sheet for stills and loops; GIF from the control strip; Android walkthrough.
