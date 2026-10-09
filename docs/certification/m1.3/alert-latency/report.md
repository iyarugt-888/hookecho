# NWS alert latency, live

Watched `api.weather.gov/alerts/active` from 2026-10-09 05:02:58Z to 2026-10-09 06:32:59Z, polling every 20 s (3 failed polls).

| Messages | Count | p50 | p95 |
| --- | --- | --- | --- |
| All new, sent → seen by a poll | 48 | 60 s | 79 s |
| Warnings, sent → seen by a poll | 29 | 60 s | 80 s |

Excluded: 181 already active at the first poll (issued before watching), 0 without a `sent` time.

Local clock minus the server's `Date` header (1 s resolution, includes the reply's transfer): p50 0.6 s, p95 1.1 s over 267 polls. A latency below is shifted by about this much.

"Seen by a poll" includes waiting for the next poll, up to 20 s here. The app polls alerts with the other overlays every 120 s (240 s on a metered connection), so in the app a message waits on average about 50 s longer than the feed itself took to publish it. Nothing here is drawn: the app's own "received → drawn" stage is measured in the running app, not by this watch.
