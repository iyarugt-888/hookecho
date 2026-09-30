# Troubleshooting

What to check when the picture looks wrong, scenario by scenario. Each starts with where the app
itself says what is going on, since it usually does. For the quick list, see
[the user guide](GUIDE.md#when-something-looks-wrong); for where each feed comes from and how
often it updates, see [DATA.md](DATA.md).

Two places answer most questions:

- **The radar status in the app bar** (the dock's top bar). It shows the feed's delay behind the
  radar and turns amber, with a word in front of the delay, when the live feed is **Aging**, has
  fallen back to a **Fallback** source, is **Recovering** after losing the stream, or is
  **Offline**. Hover it for the live-scan state (provider, cut, chunk, gaps); click it for the
  health of every active source.
- **Analyst Mode's log** (Tools → Log). It shows, for the active radar, the provider and whether it
  delivers radials as they are scanned or only completed volumes, the last provider switch and
  why, and the scan's progression: VCP, which cuts have been seen, which are complete, missing
  chunks and radials, and how long the rest of the volume should take.

The **layer probe** (the eyedropper on the rail) reads every layer under the pointer, and it
gives each source's time offset from the radar scan or the linked analysis time, for example
`(Δ-42s vs radar)`.

## The radar stopped updating

1. Look at the app bar status. **Aging** means no new data for most of the stale threshold. The
   radar may simply be between volumes: a clear-air VCP takes ten minutes. **Stale** means past
   it.
2. **Fallback** or a "completed volumes only" source in the log means the progressive feed was
   lost, and the app is polling for finished volumes instead. The display then updates once per
   volume, not once per sweep. It returns to the progressive feed on its own; the log records
   both switches.
3. **Recovering** or **Offline**: the stream dropped and has not come back. Check the network. On
   desktop, `hookecho --status` prints a per-feed report from the terminal.
4. If the delay is large but the state is healthy, the radar itself may be down for maintenance.
   The NWS radar outage notices, and a neighbouring site, will tell.
5. For diagnosis only, Settings → Advanced has a **Force data source (testing)** override that
   pins one provider. Leave it on Automatic otherwise; a forced source does not fail over.

## One tilt is missing, or appears and disappears

- **The volume is still being scanned.** A tilt above the one being swept has not arrived yet. The
  log's cut badges show unobserved, partial and complete cuts.
- **SAILS and MESO-SAILS.** In severe weather the radar re-scans its lowest tilt one to three times
  mid-volume. Those supplemental cuts are separate cuts at the same angle (the log marks them
  SAILS), so the lowest tilt refreshes more often than the others. That is expected.
- **The VCP changed.** A new VCP has a different set of angles. The cut inventory resets and the
  tilt list follows the new volume.
- **A chunk is missing.** The log names unobserved chunks and missing radials. The sweep is not
  called complete until they arrive. With **strict current sweep** chosen (the radar ribbon's
  live display mode), azimuths the current pass has not reached are hidden rather than shown from
  the previous pass. In **continuous composite** they stay, dimmed.

## Velocity looks folded

- Folding is aliasing: motion faster than the radar's Nyquist velocity wraps to the opposite
  colour. The Inspector shows the tilt's Nyquist velocity.
- **Dealias** is on by default (Display → the velocity options). It unfolds regions, so a small
  isolated patch can still be left folded.
- TDWR sites are not dealiased; their Nyquist is low and their velocity folds often.
- Purple (**Range folded** in the probe) is not aliasing. It marks gates where a second-trip echo
  made the velocity unknowable. No setting recovers it; a neighbouring radar may see the area
  cleanly.
- Storm-relative velocity subtracts a storm motion. A wrong motion makes a couplet look lopsided,
  not folded.

## Satellite is older than radar

- GOES images the CONUS sector every 5 minutes and the mesoscale sectors every minute, and each
  image reaches the app some minutes after it was taken. A radar sweep is usually newer.
- The probe gives the satellite's offset from the scan. **Layer time warning** in Settings → Units
  sets how far apart a layer may be before it is flagged.
- When the view is scrubbed back, GOES layers show the frame for that time. A frame that does not
  match is not drawn over the scan rather than shown out of time. The two live-only derived
  products (the band difference and the cooling rate) are not drawn over a scrubbed view at all.

## A model layer's time does not match the radar

- Model fields are forecasts. Their valid time is the run plus the lead. The model banner, the
  model browser and the probe (`valid …`, with its offset) all say which run and lead you are
  looking at.
- Scrubbed back more than three hours to a past event, with no run pinned, model layers read the
  run of that time, not today's. Before the archive's start (HRRR: 2014) the fetch fails rather
  than substituting today's run.
- A run pinned in the model browser wins over both. Clear it to go back to the newest run, or to
  the event's own.

## A warning polygon is not appearing

- Check that **Alerts** is on in Layers (a workspace can turn it off).
- Live alerts come from the NWS as issued. A warning that has expired or been cancelled is gone.
- Scrubbed back, the map shows the storm-based warnings that were in effect at that time, from
  the Iowa Environmental Mesonet: tornado, severe thunderstorm, flash flood, marine and extreme
  wind. Zone- and county-based products (watches, advisories) are not part of that archive.
- If a workspace named layers this version does not have, the app says so when the workspace is
  applied rather than dropping them silently.

## 3D is missing part of the storm

- **Above the top tilt.** Close to the radar the highest elevation angle passes under a tall
  storm's top (the cone of silence), so the top is not sampled.
- **Below the lowest beam.** Far from the radar the lowest beam passes over the lower part of the
  storm; beyond about 100 miles the low levels are not sampled at all. A closer radar sees them.
- **Between tilts.** The volume is built from the tilts scanned. Gaps between widely spaced upper
  tilts are real gaps in the data, not a rendering fault.
- **The volume is in progress.** Live, the upper tilts of the current volume have not arrived
  yet.
- The 3D view's quality setting trades detail for speed. At the lowest setting, small features
  can drop out.

## Basemap tiles are missing

- Some map styles need a key (Mapbox, MapTiler). Without one they stay empty rather than fall
  back silently. Keys go in Settings and stay on your machine.
- A custom tile URL with a lower maximum zoom than you are viewing goes blank past it; set its
  maximum zoom in Settings.
- Offline, only tiles already in the cache draw. Settings → Storage shows the tile cache.
- Satellite map styles (GOES) follow the view's time, and there is a short gap while a new frame
  loads.

Still stuck? Open an [issue](../../../issues) with the site, the product and the time you were
looking at, and the log's contents if Analyst Mode was on.
