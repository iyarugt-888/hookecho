"""Republish NWS warning text from the NOAA Weather Wire (NWWS-OI) onto your own MQTT broker,
for HookEcho's "Warning text topic" (1008.md A1).

    NWWS_USER=... NWWS_PASS=... python nwws_relay.py --broker 127.0.0.1:1883
    NWWS_USER=... NWWS_PASS=... python nwws_relay.py --dry-run

NWWS-OI is free but needs your own account (https://www.weather.gov/nwws/), and its terms are
for your own use: this relay holds one connection for one household and republishes onto a
broker you run. HookEcho never connects to NWWS itself.

Each warning-tier product (TOR, SVR, FFW, SMW, SQW, EWW, DSW and their follow-up statements
SVS, FFS, MWS) is published, not retained, to `<prefix>/<AWIPS id>` (default prefix
`hookecho/nwws`), payload the product text as received. HookEcho parses it, keys it by its VTEC
event and draws it only until the alerts feed publishes the same event.

Credentials come from the environment, never from flags (argv is visible in process listings):
`NWWS_USER`, `NWWS_PASS`, and for the broker `MQTT_USER`, `MQTT_PASS`.

Requires: `pip install slixmpp paho-mqtt` (Python 3.9+).
"""

import argparse
import asyncio
import logging
import os
import sys

NWWS_SERVER = ("nwws-oi.weather.gov", 5222)
NWWS_ROOM = "nwws@conference.nwws-oi.weather.gov"
# Warning products and the statements that continue, extend or cancel them.
FORWARD = ("TOR", "SVR", "FFW", "SMW", "SQW", "EWW", "DSW", "SVS", "FFS", "MWS")


def wanted(awips_id):
    return bool(awips_id) and awips_id.upper().startswith(FORWARD)


def normalize(text):
    """Line ends as `\\n`, whatever the wire used."""
    return text.replace("\r\r\n", "\n").replace("\r\n", "\n").replace("\r", "\n").strip("\n") + "\n"


def topic(prefix, awips_id):
    safe = "".join(c for c in awips_id.upper() if c.isalnum())
    return f"{prefix.rstrip('/')}/{safe}"


class Relay:
    def __init__(self, user, password, publish):
        import slixmpp  # imported here so --help and the tests work without it

        self.publish = publish
        self.xmpp = slixmpp.ClientXMPP(f"{user}@nwws-oi.weather.gov", password)
        self.xmpp.register_plugin("xep_0045")  # multi-user chat: the NWWS room
        self.xmpp.register_plugin("xep_0199", {"keepalive": True, "frequency": 60})
        self.xmpp.add_event_handler("session_start", self.start)
        self.xmpp.add_event_handler("groupchat_message", self.message)
        self.xmpp.add_event_handler("disconnected", self.disconnected)
        self.nick = f"{user}-hookecho"

    async def start(self, _event):
        await self.xmpp.get_roster()
        self.xmpp.send_presence()
        self.xmpp.plugin["xep_0045"].join_muc(NWWS_ROOM, self.nick)
        logging.info("joined %s", NWWS_ROOM)

    def message(self, msg):
        x = msg.xml.find("{nwws-oi}x")
        if x is None:
            return
        awips = x.get("awipsid", "")
        if not wanted(awips):
            return
        self.publish(awips, normalize(x.text or ""))

    def disconnected(self, _event):
        logging.warning("disconnected; reconnecting in 10 s")
        asyncio.get_event_loop().call_later(10, self.connect)

    def connect(self):
        self.xmpp.connect(address=NWWS_SERVER)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--broker", default="127.0.0.1:1883", help="MQTT broker HOST[:PORT]")
    ap.add_argument("--prefix", default="hookecho/nwws", help="topic prefix")
    ap.add_argument("--dry-run", action="store_true", help="print products, no broker")
    args = ap.parse_args()
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")

    user, password = os.environ.get("NWWS_USER"), os.environ.get("NWWS_PASS")
    if not user or not password:
        sys.exit("set NWWS_USER and NWWS_PASS (your own NWWS-OI account)")

    if args.dry_run:
        def publish(awips, text):
            print(f"--- {topic(args.prefix, awips)} ({len(text)} bytes)\n{text}", flush=True)
    else:
        import paho.mqtt.client as mqtt

        host, _, port = args.broker.partition(":")
        client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id=f"nwws-relay-{os.getpid()}")
        if os.environ.get("MQTT_USER"):
            client.username_pw_set(os.environ["MQTT_USER"], os.environ.get("MQTT_PASS", ""))
        client.connect(host, int(port or 1883))
        client.loop_start()

        def publish(awips, text):
            # At least once, not retained: a warning is an event, not a state.
            client.publish(topic(args.prefix, awips), text, qos=1, retain=False)
            logging.info("published %s (%d bytes)", awips, len(text))

    relay = Relay(user, password, publish)
    relay.connect()
    asyncio.get_event_loop().run_forever()


if __name__ == "__main__":
    main()
