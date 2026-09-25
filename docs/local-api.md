# HookEcho local API

The desktop and Android app can serve what it is showing to other programs on the same computer:
a stream overlay, a Stream Deck or home-automation button, a script. Turn it on under
**Share → Local API on 127.0.0.1:47914**. It is off by default, and it only ever listens on the
loopback interface — nothing else on your network can reach it. (For a machine with no app
running, `hookecho --serve` answers similar questions from the feeds directly.)

Every endpoint is a `GET` under `http://127.0.0.1:47914/api/v1` (the port is the
`local_api_port` setting). Answers are JSON unless noted, and never cached (`Cache-Control:
no-store`).

| endpoint | answers |
|---|---|
| `/api/v1` | the list of endpoints |
| `/api/v1/state` | every pane: site, product, tilt and elevation, the volume on display and its time, whether it follows live, the camera; which pane is active |
| `/api/v1/detections` | debris signatures and rotation couplets on the active pane's volume, as shown (after corroboration), with confidence and the algorithm versions |
| `/api/v1/warnings` | the warnings shown on the map: event, headline, area, expiry, VTEC |
| `/api/v1/health` | each data source's health, as the diagnostics export reports it |
| `/api/v1/products` | the active volume's moments and elevation angles, and the field layers on |
| `/api/v1/frames` | the active pane's volume list (UTC), the playhead, and whether it follows live |
| `/api/v1/sample?lat=&lon=` | every moment of the active pane's displayed tilt at a point, with the beam's azimuth, range and height there; velocity dealiased |
| `/api/v1/snapshot.png` | the window as a PNG |
| `/api/v1/events` | Server-Sent Events: an `event: state` carrying the `/state` JSON now and again whenever the active pane's volume, tilt or product changes; a `: keep-alive` comment every 15 s |

The state is refreshed about once a second. Detections, warnings and health are HookEcho's own
readings — **the detections are heuristics, not NWS products.**

## Examples

```sh
curl http://127.0.0.1:47914/api/v1/state
curl "http://127.0.0.1:47914/api/v1/sample?lat=35.31&lon=-97.57"
curl -o now.png http://127.0.0.1:47914/api/v1/snapshot.png
curl -N http://127.0.0.1:47914/api/v1/events
```

A sample answer (the values here are illustrative, not a recorded reading):

```json
{
  "site": "KTLX",
  "volume": "KTLX20130520_200811_V06",
  "volume_time_utc": "2013-05-20T20:08:11+00:00",
  "elevation_deg": 0.5,
  "lat": 35.31, "lon": -97.57,
  "geometry": { "azimuth_deg": 265.4, "slant_range_km": 26.6, "beam_height_ft": 1400.0 },
  "values": { "REF": 58.5, "VEL": -21.0, "SW": 6.5, "ZDR": 0.3, "PHI": 94.0, "KDP": 1.2, "CC": 0.41 }
}
```

A value is `null` where that moment has nothing at the point (below threshold, range folded, or
outside the sweep).

## Errors

| status | when |
|---|---|
| 400 | `sample` without `lat`/`lon`, or out of range |
| 403 | the request's `Host` header does not name this server (see below) |
| 404 | an unknown path |
| 405 | anything other than `GET` |
| 503 | the app has not published yet (just started), or is closing |
| 504 | the app did not answer a sample (3 s) or a snapshot (8 s) in time |

Errors are `{"error": "..."}`.

## Security

Anything on this computer can reach 127.0.0.1, including a web page open in a browser, so:

- **Host check.** A request must name this server in its `Host` header — `127.0.0.1:<port>`,
  `localhost:<port>` or `[::1]:<port>`. A page using DNS rebinding to aim its own domain at
  127.0.0.1 names its own host, and is refused.
- **No CORS.** No `Access-Control-Allow-Origin` header is ever sent, so a script on another origin
  cannot read an answer. To build a browser overlay, serve the page from somewhere that can make
  the request itself (OBS's browser source pointed at a local file cannot; a tiny local proxy or a
  native script can).
- **Loopback only.** There is no setting to bind another interface.

The API can reveal where you are looking and your saved warnings context, which is why it is off
until you turn it on.
