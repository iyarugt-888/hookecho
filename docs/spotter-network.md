# Spotter Network: reporting decision (1008.md H3)

Decided 2026-10-09: **HookEcho does not submit Spotter Network reports or positions.** The item
is closed unless Spotter Network publishes terms that permit it.

## What was checked

The Spotter Network [terms of use](https://www.spotternetwork.org/pages/terms-of-use), read
2026-10-09:

- They do not mention an API, programmatic access, or submission of reports or positions by
  third-party software. The site lists two client apps (RadarScope, Radar Alive!) without saying
  what they may do or on what terms, which reads as individual arrangements rather than a public
  interface.
- Data may be used for personal, non-commercial purposes only, may not be published or
  distributed otherwise, and may not be retained or presented more than 48 hours after it
  originated. Commercial use needs express written consent.
- Section 6 forbids using information for "any unauthorized purpose" and misrepresenting an
  affiliation. A report sent through an undocumented interface would risk both.

No developer or API page exists on the site (`/pages/api` and `/api` do not resolve). The 1008.md
condition — build only if user-authenticated reporting is permitted — is therefore not met.

## What HookEcho does with Spotter Network today

- It reads the public GRLevelX positions feed (`feeds/gr.txt`) for the Spotters layer
  (`wxdata::spotters`), as a client display. Email addresses embedded in the feed are dropped at
  parse time.
- Positions are held in memory for the current view and refreshed; nothing is written to disk,
  to case files or to exports, so nothing is kept past the 48-hour limit.
- The self-hosted server (`--serve`) lists `www.spotternetwork.org` among the hosts its browser
  proxy may reach, so the web build can show the same layer. That passes the feed through to the
  operator's own browser; it does not republish it.

## If this changes

Spotter Network would need to publish (or grant in writing) a submission interface for
third-party clients. Then reports would be sent with the user's own Spotter Network credentials,
never stored by HookEcho in plain text, and labelled with the app as their source.
