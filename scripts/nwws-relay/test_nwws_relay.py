"""The relay's pure parts: which products it forwards, their topic, and line endings.
Run with `python -m unittest scripts/nwws-relay/test_nwws_relay.py` (no XMPP or MQTT needed)."""

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(__file__))
import nwws_relay  # noqa: E402


class RelayTest(unittest.TestCase):
    def test_only_warning_products_are_forwarded(self):
        for awips in ("SVRDMX", "TORMOB", "SVSDMX", "FFWLSX", "SMWMFL", "svrdmx"):
            self.assertTrue(nwws_relay.wanted(awips), awips)
        for awips in ("AFDDMX", "ZFPDMX", "LSRDMX", "", None):
            self.assertFalse(nwws_relay.wanted(awips), awips)

    def test_topic_is_the_prefix_and_a_safe_awips_id(self):
        self.assertEqual(nwws_relay.topic("hookecho/nwws/", "SVRDMX"), "hookecho/nwws/SVRDMX")
        self.assertEqual(nwws_relay.topic("p", "SV#R/+"), "p/SVR")

    def test_line_ends_become_newlines(self):
        self.assertEqual(nwws_relay.normalize("\n\nA\r\r\nB\r\nC\rD\n\n"), "A\nB\nC\nD\n")


if __name__ == "__main__":
    unittest.main()
