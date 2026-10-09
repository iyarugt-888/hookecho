# NWS warning latency (1008.md A1)

## Live measurement, 2026-10-09 05:03–06:33Z

`hookecho --headless-alert-latency 90 20 --export DIR` polled `api.weather.gov/alerts/active`
every 20 s for 90 minutes (270 polls, 3 failed). New messages in the window were 18 severe
thunderstorm warnings, one flash flood warning and a 29-message tropical storm watch/warning
package issued at 06:19Z. The 181 alerts already active at the
first poll were excluded: they were issued before anyone was looking.

| Messages | Count | p50 | p95 |
| --- | --- | --- | --- |
| All new, NWS `sent` → seen by a 20 s poll | 48 | 60 s | 79 s |
| Warnings | 29 | 60 s | 80 s |

- The local clock was 0.6 s (p50) and 1.1 s (p95) ahead of the server's `Date` header, which has
  1 s resolution and includes the reply's transfer.
- Every `sent` time was on the whole minute, so each latency is an upper bound: the product was
  issued up to 59 s after the time it states. (Found from this run; the watch's report now says
  so whenever it is the case.)
- "Seen by a poll" includes waiting for the poll, up to 20 s. Reading the samples, the feed
  carried each new warning within roughly 20–60 s of its stated `sent` minute.

The app polled alerts with every other overlay every 120 s, which by this measurement added about
50 s on average (up to 120 s) to what the feed itself took. The follow-up below takes that to
about 15 s.

Files ([alert-latency/](alert-latency/)):

| File | Rows | SHA-256 |
| --- | --- | --- |
| report.md | — | `f09d05b64a4b03bd2050f3fbf9666f94a9b3437f63b2ce36b4e07d169c467475` |
| samples.csv | 48 | `c39d411f56c0916f40ce3689994479e406b71af119b07ecdde1bf0fefcb16237` |
| polls.csv | 270 | `50cbff2e01c4100bc8306ee6000f5e0b8038b987193a785ac3aceea85d656573` |

## What changed in the app

- The Analyst log shows **Warning arrival** (NWS `sent` → the app accepted the reply) and
  **Then drawn** (→ the first frame built after it), p50/p95 and count, for warnings that first
  appear while the app runs (`wxdata::alert_latency`). The drawn stage is the CPU frame, not
  screen scan-out.
- A **30 s warning-polygon poll** (`FeedSource::WarningPolygons`) asks only for the nationwide
  polygon feed between the 120 s full refreshes, on unmetered connections, on its own request lane;
  new warnings are drawn and announced at once and deduplicated against NWWS-OI by VTEC event.
- An optional **NWWS-OI** path: products the user's own relay (`scripts/nwws-relay`) republishes
  to their MQTT broker are parsed (`wxdata::nwws`) and merged by VTEC event, labelled "via
  NWWS-OI", with a **Wire arrival** stage in the Analyst log. With no topic set nothing changes.

## Not established

- The app's own "received → drawn" stage has not been measured in an interactive session, and the
  30 s poll has not been run live in the app (it is the same request the watch made).
- The NWWS-OI relay has not been run against NWWS-OI (no account), so no session with both paths
  exists.
- No Android or browser run.
