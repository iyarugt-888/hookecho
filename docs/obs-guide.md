# Using HookEcho with OBS

HookEcho can give OBS (or a second screen) a clean picture of one radar pane — no buttons,
panels or cursor — with the broadcast dressing: clock, source caption, warning crawl, colour
scale, your logo and a title strap. This guide sets that up on desktop. It describes the app's
behaviour as built; it was not walked through with OBS itself when it was written (2026-10-09).

## 1. Dress the picture

In the preferences panel's **Map** page:

- **Streaming overlay** — choose the clock, source caption, warning crawl and colour scale, a
  logo (a PNG path), and the safe margin that keeps them in from the edge (5 % is the broadcast
  convention).
- **Scenes** — set the map the way you want a shot to look and save it as a scene. Scenes hold
  the view, the layers and the dressing. **Alt+1** … **Alt+9** switch to the first nine; or cue
  a scene in preview and press **Take** to put it on.

## 2. Open the output window

In **Output window** on the same page:

1. Tick **Open the output window**. A window titled **HookEcho Output** opens.
2. **Program shows** — pin it to one pane, so working in the other panes never changes what is on
   air, or choose **Follow the active pane**.
3. **Hold the program view** — the output keeps its own camera: panning or zooming that pane
   in the operator window no longer moves the output, and Take sets it.
4. **Size** — 1920 × 1080, 2560 × 1440 or 3840 × 2160 in *physical* pixels whatever the
   display scaling, or Free size to drag it yourself.
5. **Fullscreen** — fills the monitor the window is on. To put it on another monitor, drag the
   window there first, then tick Fullscreen. **Esc** in the output leaves fullscreen.
6. **Title strap** — a lower-third caption at the bottom left, above the warning crawl.

## 3. Capture it in OBS

Add a **Window Capture** source (on macOS, **macOS Screen Capture** with the window method) and
choose **[hookecho.exe]: HookEcho Output** (the process name differs by platform). Set
**Window Match Priority** to *Match title, otherwise find window of same type*: the title is
always "HookEcho Output", and HookEcho reopens the output window at launch with the size,
fullscreen setting and strap it was left with, so OBS finds it again after a restart. On Windows, the
*Windows 10 (1903 and up)* capture method is the one that captures GPU-drawn windows reliably.

Because the output is sized in physical pixels, a 1920 × 1080 output fills a 1920 × 1080 OBS
canvas without scaling. If it does not line up, right-click the source → **Transform → Fit to
screen**.

## 4. During coverage

- Switch scenes with **Alt+1 … Alt+9** in the operator window; the output follows.
- **Streaming mode** (F8) dresses the operator window itself instead, for a single-screen setup;
  **F9** tours the active warnings every 12 seconds.
- A loop for the web: **Export loop (GIF/MP4)**, with MP4 at 30 or 60 fps. If a scan does not load
  within 30 seconds, the export pauses and asks what to do rather than recording the wrong scan.

## What is not supported yet

- Choosing the monitor from a list: drag the output window to the monitor first. Its position
  and which monitor it was on are not remembered across restarts (its size, fullscreen and strap
  are), so a fullscreen output reopens on the monitor the operating system places it on.
- Which pane program shows and a held view are not remembered across restarts either.
- A separate output window on Android or in a browser; there, streaming mode dresses the main view.
