# nwws-relay

Republishes NWS warning text from the NOAA Weather Wire Service (NWWS-OI) onto **your own** MQTT
broker, so HookEcho can draw a warning as soon as the wire carries it instead of waiting for its
next poll of `api.weather.gov`.

## Why a relay at all

NWWS-OI pushes every product the moment it is issued, but it needs an account (free, from
<https://www.weather.gov/nwws/>) and a long-held XMPP connection. HookEcho never connects to it.
It subscribes to a topic on a broker you run, and this program fills that topic. Without it the
app works exactly as before, from the alerts feed alone.

## Run it

```sh
pip install slixmpp paho-mqtt
NWWS_USER=you NWWS_PASS=... python nwws_relay.py --broker 127.0.0.1:1883
NWWS_USER=you NWWS_PASS=... python nwws_relay.py --dry-run   # print products, no broker
```

Credentials come from the environment, never from flags: `NWWS_USER`, `NWWS_PASS`, and for the
broker `MQTT_USER`, `MQTT_PASS`.

Then in HookEcho: **Settings → MQTT → Warning text topic**, `hookecho/nwws/#`.

## What it publishes

`hookecho/nwws/<AWIPS id>` (for example `hookecho/nwws/SVRDMX`), not retained, QoS 1, the
product text with `\n` line endings. Only warning products and their statements are forwarded:
TOR, SVR, FFW, SMW, SQW, EWW, DSW, SVS, FFS, MWS.

In the app, `wxdata::nwws` reads each segment's VTEC event, polygon, motion and threat tags. A
warning the alerts feed has not published yet is drawn and announced, labelled "via NWWS-OI";
when the feed publishes the same VTEC event, the feed's message replaces it. An ended (CAN/EXP)
or expired event is never drawn from the wire. The Analyst log shows "Wire arrival" beside the
feed's "Warning arrival".

## What has and has not been checked

- The app's parser and merge are tested against real warning, statement and tornado products
  (`crates/wxdata/tests/data/nwws/`), and the relay's product filter, topics and line endings by
  `python -m unittest test_nwws_relay.py`.
- **This relay has not been run against NWWS-OI**: no account was available when it was written.
  The room name, server and the `nwws-oi` message element follow NWWS-OI's published client
  documentation. Run it with `--dry-run` first and check that products print.

Personal use, one connection per household, as the NWWS-OI terms describe.
