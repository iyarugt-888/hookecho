import hashlib
import contextlib
import io
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest import mock

import capture_truth
import provision


class ProvisionTests(unittest.TestCase):
    def test_corrupt_or_overlong_download_is_never_published(self):
        correct = b"radar fixture" * 10
        fixture = {"id": "test", "bytes": len(correct), "sha256": hashlib.sha256(correct).hexdigest(), "source": {"url": "https://example.invalid/input"}}
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "fixture.ar2"
            for body in (b"c" * len(correct), correct + b"extra"):
                with self.assertRaises(ValueError):
                    provision.download(fixture, destination, opener=lambda *_args, **_kwargs: io.BytesIO(body))
                self.assertFalse(destination.exists())
                self.assertEqual(list(Path(directory).iterdir()), [])
            provision.download(fixture, destination, opener=lambda *_args, **_kwargs: io.BytesIO(correct))
            self.assertEqual(destination.read_bytes(), correct)
            destination.write_bytes(b"broken cache")
            with self.assertRaises(ValueError):
                provision.download(fixture, destination)
            self.assertEqual(destination.read_bytes(), b"broken cache")

    def test_missing_inputs_and_path_escape_are_failures(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(ValueError):
                provision.verify(root / "missing.ar2", 25, "0" * 64)
            for name in ("../file", "C:\\file", "/file", "..", ""):
                with self.assertRaises(ValueError):
                    provision.safe_path(root, name)

    def test_subset_copies_records_without_fabricating_end_or_missing_records(self):
        header = b"AR2V0006." + bytes(15)
        records = [struct.pack(">i", -len(b)) + b for b in (b"metadata", b"observed", b"omitted")]
        source = header + b"".join(records)
        self.assertEqual(provision.record_subset(source, [0, 1]), header + records[0] + records[1])
        for wanted in ([1, 0], [0, 0], [3], [-1]):
            with self.assertRaises(ValueError):
                provision.record_subset(source, wanted)
        with self.assertRaises(ValueError):
            provision.record_subset(source[:-1], [2])

    def test_manifest_rejects_unsupported_schema_and_duplicate_ids(self):
        data = json.loads(provision.MANIFEST.read_text(encoding="utf-8"))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            data["schema_version"] = 999
            path.write_text(json.dumps(data))
            with self.assertRaises(ValueError):
                provision.load_manifest(path)
            data["schema_version"] = provision.MANIFEST_VERSION
            data["fixtures"].append(data["fixtures"][0])
            path.write_text(json.dumps(data))
            with self.assertRaises(ValueError):
                provision.load_manifest(path)

    def test_truth_requests_keep_exact_utc_windows(self):
        self.assertEqual(provision.truth_request_url({"kind": "warning-at", "at": "2013-05-20T20:12:00Z"}),
                         "https://mesonet.agron.iastate.edu/geojson/sbw.py?ts=2013-05-20T20%3A12%3A00Z")
        for request in ({"kind": "unknown"}, {"kind": "reports", "start": "2013-05-20T20:00:00Z", "end": "2013-05-20T19:00:00Z"}, {"kind": "warning-at", "at": "2013-05-20T20:00:00"}):
            with self.assertRaises(ValueError):
                provision.truth_request_url(request)

    def test_candidate_capture_refuses_to_overwrite_certified_inputs(self):
        with mock.patch("sys.argv", ["capture_truth.py", "--output", str(provision.MANIFEST.parent)]), contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as error:
                capture_truth.main()
        self.assertEqual(error.exception.code, 2)


if __name__ == "__main__":
    unittest.main()
