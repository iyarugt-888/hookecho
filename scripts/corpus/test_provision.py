import hashlib
import io
import json
from pathlib import Path
import struct
import tempfile
import unittest

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
            data["schema_version"] = 1
            data["fixtures"].append(data["fixtures"][0])
            path.write_text(json.dumps(data))
            with self.assertRaises(ValueError):
                provision.load_manifest(path)


if __name__ == "__main__":
    unittest.main()
