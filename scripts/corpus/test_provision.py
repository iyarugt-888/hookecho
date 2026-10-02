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
import classification


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

    def test_damage_tracks_retain_source_metadata_and_geometry(self):
        _, _, tracks, _ = provision.load_manifest(provision.MANIFEST)
        self.assertEqual(len(tracks), 2)
        for track in tracks:
            path = provision.MANIFEST.parent / track["path"]
            line = provision.verify_track(path, track)
            self.assertEqual(len(line), track["expected_vertices"])
            changed = dict(track, event_name="invented tornado")
            with self.assertRaises(ValueError):
                provision.verify_track(path, changed)
            changed = dict(track, expected_vertices=track["expected_vertices"] + 1)
            with self.assertRaises(ValueError):
                provision.verify_track(path, changed)
        self.assertTrue(tracks[1]["evidence"].startswith("Not surveyed."))

    def test_damage_track_manifest_rejects_missing_paths_and_invalid_times(self):
        original = json.loads(provision.MANIFEST.read_text(encoding="utf-8"))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            mutations = [lambda d: d.update(track_snapshots=[]),
                         lambda d: d["track_snapshots"][0].update(path="../escape.kmz"),
                         lambda d: d["track_snapshots"][0].update(end=d["track_snapshots"][0]["start"]),
                         lambda d: d["track_snapshots"][0].update(start="2021-12-15T23:35:00")]
            for mutate in mutations:
                data = json.loads(json.dumps(original))
                mutate(data)
                path.write_text(json.dumps(data))
                with self.assertRaises(ValueError):
                    provision.load_manifest(path)

    def test_independent_classification_reader_preserves_labels_and_clocks(self):
        _, _, _, inputs = provision.load_manifest(provision.MANIFEST)
        self.assertEqual(len(inputs), 1)
        fixture = inputs[0]
        path = provision.MANIFEST.parent / fixture['path']
        data = provision.verify_classification(path, fixture)
        self.assertEqual(data['class_counts']['20'], 2363)
        self.assertEqual(sum(data['class_counts'].values()), 360 * 1200)
        self.assertEqual((data['lat'], data['lon']), (35.333, -97.278))
        self.assertEqual(data['acquisition_time'], '2020-07-15T12:04:10Z')
        self.assertEqual(data['generation_time'], '2020-07-15T12:04:54Z')
        for changed in (dict(fixture, generation_time='2020-07-15T12:04:55Z'),
                        dict(fixture, expected_radials=359),
                        dict(fixture, expected_classes={'20': 432000})):
            with self.assertRaises(ValueError):
                provision.verify_classification(path, changed)

    def test_classification_manifest_rejects_missing_context_and_invalid_clocks(self):
        original = json.loads(provision.MANIFEST.read_text(encoding='utf-8'))
        mutations = [lambda d: d.update(classification_snapshots=[]),
                     lambda d: d['classification_snapshots'][0].update(radar_fixture='missing'),
                     lambda d: d['classification_snapshots'][0].update(path='../escape.l3'),
                     lambda d: d['classification_snapshots'][0].update(generation_time='2020-07-15T12:04:09Z'),
                     lambda d: d['classification_snapshots'][0].update(expected_classes={'20': 0}),
                     lambda d: d['classification_snapshots'][0]['source'].update(acquisition_time='2020-07-15T12:04:11Z')]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'manifest.json'
            for mutate in mutations:
                data = json.loads(json.dumps(original))
                mutate(data)
                path.write_text(json.dumps(data))
                with self.assertRaises(ValueError):
                    provision.load_manifest(path)

    def test_classification_reader_rejects_truncated_containers(self):
        _, _, _, inputs = provision.load_manifest(provision.MANIFEST)
        raw = (provision.MANIFEST.parent / inputs[0]['path']).read_bytes()
        for body in (b'', raw[:100], raw[:-80]):
            with self.assertRaises(ValueError):
                classification.inspect_hca(body)


if __name__ == "__main__":
    unittest.main()
