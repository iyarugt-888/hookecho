#!/usr/bin/env python3
"""Provision exact scientific inputs. Never discover scans or rewrite expected hashes."""
import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path
import struct
import sys
import tempfile
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
import zipfile
import re
from classification import inspect_hca

REPO = Path(__file__).resolve().parents[2]
MANIFEST = REPO / "crates/wxdata/tests/data/corpus/manifest.json"
SOURCE_PREFIX = "https://unidata-nexrad-level2.s3.amazonaws.com/"
MANIFEST_VERSION = 4


def checksum(path):
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            size += len(block)
            digest.update(block)
    return size, digest.hexdigest()


def verify(path, size, digest):
    if not path.is_file():
        raise ValueError(f"Required fixture missing: {path}")
    actual_size, actual_digest = checksum(path)
    if (actual_size, actual_digest) != (size, digest):
        raise ValueError(f"Integrity failure: {path}; expected {size} bytes / {digest}, "
                         f"got {actual_size} / {actual_digest}. No golden was updated.")


def safe_path(root, name):
    if not isinstance(name, str) or not name or name in (".", "..") or any(c in name for c in "/\\:"):
        raise ValueError(f"Unsafe fixture path: {name!r}")
    root = root.resolve()
    path = (root / name).resolve()
    if not path.is_relative_to(root):
        raise ValueError(f"Fixture escapes its cache: {name}")
    return path


def truth_request_url(request):
    if request.get("kind") == "warning-at":
        endpoint, params = "sbw.py", {"ts": request["at"]}
    elif request.get("kind") == "reports":
        endpoint, params = "lsr.geojson", {"sts": request["start"], "ets": request["end"]}
    else:
        raise ValueError("Unknown truth request kind")
    times = [dt.datetime.fromisoformat(v.replace("Z", "+00:00")) for v in params.values()]
    if any(t.tzinfo is None or t.utcoffset() != dt.timedelta(0) for t in times):
        raise ValueError("Truth request must use UTC source times")
    if len(times) == 2 and times[0] >= times[1]:
        raise ValueError("Invalid truth window")
    canonical = {k: t.strftime("%Y-%m-%dT%H:%M:%SZ") for k, t in zip(params, times)}
    return "https://mesonet.agron.iastate.edu/geojson/" + endpoint + "?" + urllib.parse.urlencode(canonical)


def load_manifest(path):
    manifest = json.loads(path.read_text(encoding="utf-8"))
    if manifest.get("schema_version") != MANIFEST_VERSION:
        raise ValueError("Unsupported corpus manifest version")
    fixtures = manifest["fixtures"]
    if not fixtures or len({f["id"] for f in fixtures}) != len(fixtures):
        raise ValueError("Missing or duplicate fixture identities")
    for f in fixtures:
        safe_path(path.parent, f["path"])
        if f["tier"] not in ("offline", "cached") or f["format"] != "nexrad-archive-ii":
            raise ValueError(f"Unsupported fixture contract: {f['id']}")
        for size, digest in ((f["bytes"], f["sha256"]), (f["source"]["bytes"], f["source"]["sha256"])):
            if not isinstance(size, int) or not 24 < size <= 200_000_000 or len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
                raise ValueError(f"Invalid size or SHA-256: {f['id']}")
        source = f["source"]
        if source["url"] != SOURCE_PREFIX + source["object_key"]:
            raise ValueError("Source URL must name the exact public archive object")
        if f["tier"] == "cached" and (f["transform"] != {"kind": "identity", "records": []} or f["path"] != source["object_key"].rsplit("/", 1)[-1] or (f["bytes"], f["sha256"]) != (source["bytes"], source["sha256"])):
            raise ValueError("Cached input differs from its source")
        if f["tier"] == "offline" and f["transform"]["kind"] != "ldm-record-subset":
            raise ValueError("Unsupported offline derivation")
    snapshots = manifest.get("truth_snapshots", [])
    if not snapshots or len({f["id"] for f in fixtures + snapshots}) != len(fixtures) + len(snapshots):
        raise ValueError("Missing or duplicate truth snapshots")
    requests = set()
    for f in snapshots:
        safe_path(path.parent, f["path"])
        if f["format"] != "geojson-feature-collection" or not 0 < f["bytes"] <= 2_000_000 or len(f["sha256"]) != 64 or any(c not in "0123456789abcdef" for c in f["sha256"]):
            raise ValueError("Invalid truth snapshot integrity contract")
        url = truth_request_url(f["request"])
        if url != f["source"]["url"] or url in requests:
            raise ValueError("Incorrect or duplicate truth source request")
        requests.add(url)
        if not isinstance(f["expected_features"], int) or f["expected_features"] < 0:
            raise ValueError("Missing truth feature count")
    tracks = manifest.get("track_snapshots", [])
    all_inputs = fixtures + snapshots + tracks
    if not tracks or len({f["id"] for f in all_inputs}) != len(all_inputs):
        raise ValueError("Missing or duplicate damage tracks")
    for f in tracks:
        safe_path(path.parent, f["path"])
        if f["format"] != "nws-damage-track-kmz" or not 0 < f["bytes"] <= 2_000_000 or len(f["sha256"]) != 64 or any(c not in "0123456789abcdef" for c in f["sha256"]):
            raise ValueError("Invalid damage-track integrity contract")
        url = f["source"]["url"]
        if url != "https://www.weather.gov/source/dmx/IowaTors/2021/" + f["path"] or not f["path"].endswith(".kmz") or url in requests:
            raise ValueError("Incorrect or duplicate damage-track source")
        requests.add(url)
        times = [dt.datetime.fromisoformat(f[key].replace("Z", "+00:00")) for key in ("start", "end")]
        captured = dt.datetime.fromisoformat(f["source"]["captured_at"].replace("Z", "+00:00"))
        if any(t.tzinfo is None or t.utcoffset() != dt.timedelta(0) for t in times + [captured]) or not times[0] < times[1] <= captured:
            raise ValueError("Invalid damage-track UTC interval")
        if not f["event_name"] or not f["evidence"] or f["expected_vertices"] < 2 or not f["source"]["attribution"] or f["source"]["license_url"] != "https://www.weather.gov/disclaimer":
            raise ValueError("Missing damage-track provenance")
    classifications = manifest.get("classification_snapshots", [])
    all_inputs += classifications
    if not classifications or len({f["id"] for f in all_inputs}) != len(all_inputs) or len({f["path"] for f in all_inputs}) != len(all_inputs):
        raise ValueError("Missing or duplicate classification identity/path")
    for f in classifications:
        safe_path(path.parent, f["path"])
        if f["format"] != "nexrad-level3-digital-hca" or not 120 < f["bytes"] <= 2_000_000 or len(f["sha256"]) != 64 or any(c not in "0123456789abcdef" for c in f["sha256"]):
            raise ValueError("Invalid classification integrity contract")
        radar = next((r for r in fixtures if r["id"] == f["radar_fixture"]), None)
        source = f["source"]
        if radar is None or radar["site"] != f["site"] or radar["source"]["acquisition_time"] != source["acquisition_time"]:
            raise ValueError("Classification/radar context mismatch")
        if not re.fullmatch(r'K[A-Z0-9]{3}', f["site"]):
            raise ValueError("Invalid classification radar site")
        times = [dt.datetime.fromisoformat(t.replace("Z", "+00:00")) for t in
                 (source["acquisition_time"], f["generation_time"], source["captured_at"])]
        if any(t.tzinfo is None or t.utcoffset() != dt.timedelta(0) for t in times) or not times[0] < times[1] <= times[2]:
            raise ValueError("Invalid classification source clocks")
        key = f['site'][1:] + '_N0H_' + times[0].strftime('%Y_%m_%d_%H_%M_%S')
        if source["object_key"] != key or source["url"] != 'https://unidata-nexrad-level3.s3.amazonaws.com/' + key or (source["bytes"], source["sha256"]) != (f["bytes"], f["sha256"]):
            raise ValueError("Classification source identity mismatch")
        classes = f["expected_classes"]
        if not 0 < f["expected_radials"] <= 720 or not 0 < f["expected_bins"] <= 2000 or f["gate_spacing_km"] != 0.25 or not 0 <= f["elevation_deg"] <= 20:
            raise ValueError("Invalid classification geometry")
        if not classes or any(not k.isdigit() or str(int(k)) != k or not 0 <= int(k) <= 255 or not isinstance(v, int) or not 0 < v <= 1_440_000 for k, v in classes.items()) or sum(classes.values()) != f["expected_radials"] * f["expected_bins"] or not classes.get("20", 0):
            raise ValueError("Invalid classification label inventory")
        if not f["evidence"] or not f["checks"] or not re.fullmatch(r'[0-9a-f]{40}', f["algorithm_baseline_commit"]) or not source["attribution"] or source["license_url"] != 'https://registry.opendata.aws/noaa-nexrad/' or f["reference_url"] != 'https://www.roc.noaa.gov/public-documents/icds/2620001AD.pdf':
            raise ValueError("Classification attribution/limitations missing")
    return fixtures, snapshots, tracks, classifications


def verify_classification(path, fixture):
    verify(path, fixture["bytes"], fixture["sha256"])
    decoded = inspect_hca(path.read_bytes())
    expected = {'product_code': 165, 'first_bin': 0,
                'acquisition_time': fixture['source']['acquisition_time'],
                'generation_time': fixture['generation_time'],
                'elevation_deg': fixture['elevation_deg'],
                'radials': fixture['expected_radials'], 'bins': fixture['expected_bins'],
                'class_counts': fixture['expected_classes']}
    if any(decoded[k] != v for k, v in expected.items()):
        raise ValueError(f"Classification source clocks/geometry/labels changed: {fixture['id']}")
    return decoded


def verify_track(path, fixture):
    """Check NWS metadata and vertices independently of the production Rust importer."""
    verify(path, fixture["bytes"], fixture["sha256"])
    with zipfile.ZipFile(path) as archive:
        kml = archive.getinfo("doc.kml")
        if kml.file_size > 2_000_000:
            raise ValueError("Inflated damage-track KML exceeds bound")
        doc = ET.fromstring(archive.read(kml))
    ns = {"k": "http://earth.google.com/kml/2.2"}
    marks = doc.findall(".//k:Placemark", ns)
    if len(marks) != 1:
        raise ValueError("Expected exactly one NWS track")
    fields = dict(re.findall(r"<td><b>(.*?)</b></td><td>(.*?)</td>", marks[0].findtext("k:description", namespaces=ns) or ""))
    expected = {"event_id": fixture["event_name"], "comments": fixture["evidence"]}
    expected.update({key + "time": dt.datetime.fromisoformat(fixture[key].replace("Z", "+00:00")).strftime("%Y-%m-%d %H:%M:%S") for key in ("start", "end")})
    if any(fields.get(key) != value for key, value in expected.items()):
        raise ValueError("Damage-track metadata differs from source")
    coords = marks[0].findtext("k:LineString/k:coordinates", namespaces=ns)
    vertices = [tuple(map(float, p.split(",")[:2])) for p in (coords or "").split()]
    if len(vertices) != fixture["expected_vertices"] or any(len(p) != 2 or not -180 <= p[0] <= 180 or not -90 <= p[1] <= 90 for p in vertices):
        raise ValueError("Damage-track geometry differs from source")
    return vertices


def download(fixture, destination, opener=urllib.request.urlopen):
    """Stream to a unique sibling, enforce size/hash, then publish atomically."""
    if destination.exists():
        verify(destination, fixture["bytes"], fixture["sha256"])
        return
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=destination.parent, prefix=".corpus-", suffix=".part", delete=False) as output:
            temporary = Path(output.name)
            with opener(fixture["source"]["url"], timeout=120) as response:
                total = 0
                while block := response.read(1024 * 1024):
                    total += len(block)
                    if total > fixture["bytes"]:
                        raise ValueError(f"Response exceeds pinned size: {fixture['id']}")
                    output.write(block)
        verify(temporary, fixture["bytes"], fixture["sha256"])
        temporary.replace(destination)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def record_subset(source, wanted):
    """Copy unchanged LDM bytes, including metadata; omitted records stay omitted."""
    if not wanted or wanted != sorted(set(wanted)) or wanted[0] < 0:
        raise ValueError("Record indices must be nonnegative, unique, and ascending")
    if len(source) < 24 or not source.startswith(b"AR2V0006."):
        raise ValueError("Subset derivation requires unwrapped Archive II version 6 LDM data")
    output = bytearray(source[:24])
    offset = 24
    index = 0
    remaining = set(wanted)
    while remaining:
        if offset + 4 > len(source):
            raise ValueError("Source does not contain every required record")
        length = abs(struct.unpack_from(">i", source, offset)[0])
        end = offset + 4 + length
        if length == 0 or end > len(source):
            raise ValueError("Invalid or truncated LDM record")
        if index in remaining:
            output.extend(source[offset:end])
            remaining.remove(index)
        index += 1
        offset = end
    return bytes(output)


def rebuild(fixture, offline_root, cache):
    source = fixture["source"]
    parent = safe_path(cache, source["object_key"].rsplit("/", 1)[-1])
    verify(parent, source["bytes"], source["sha256"])
    data = record_subset(parent.read_bytes(), fixture["transform"]["records"])
    if (len(data), hashlib.sha256(data).hexdigest()) != (fixture["bytes"], fixture["sha256"]):
        raise ValueError("Derived bytes differ from the pinned offline input; refusing to rewrite")
    destination = safe_path(offline_root, fixture["path"])
    if destination.exists():
        verify(destination, fixture["bytes"], fixture["sha256"])
    else:
        destination.write_bytes(data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, default=REPO / "target/scientific-corpus")
    parser.add_argument("--verify-only", action="store_true", help="Fail on any missing/corrupt input; never fetch")
    parser.add_argument("--offline-only", action="store_true", help="Verify only the small committed subset")
    parser.add_argument("--rebuild-offline", action="store_true", help="Reproduce missing small fixtures from verified cached parents")
    args = parser.parse_args()
    if args.verify_only and args.rebuild_offline:
        parser.error("--verify-only cannot rebuild files")
    fixtures, snapshots, tracks, classifications = load_manifest(MANIFEST)
    # Required offline inputs always verify, even while provisioning the large suite.
    for f in fixtures:
        if f["tier"] == "cached":
            if args.offline_only:
                continue
            path = safe_path(args.cache, f["path"])
            if args.verify_only:
                verify(path, f["bytes"], f["sha256"])
            else:
                download(f, path)
        else:
            if args.rebuild_offline:
                rebuild(f, MANIFEST.parent, args.cache)
            path = safe_path(MANIFEST.parent, f["path"])
            verify(path, f["bytes"], f["sha256"])
        print(f"Verified {f['id']}: {f['bytes']} bytes SHA-256 {f['sha256']}")
    # Dynamic services include retrieval clocks and may revise historic records. Restore the
    # committed snapshot bytes from Git; never fetch a replacement into the certified corpus.
    for f in snapshots:
        path = safe_path(MANIFEST.parent, f["path"])
        verify(path, f["bytes"], f["sha256"])
        collection = json.loads(path.read_bytes())
        if collection.get("type") != "FeatureCollection" or not isinstance(collection.get("features"), list) or len(collection["features"]) != f["expected_features"]:
            raise ValueError(f"Pinned truth collection shape/count changed: {f['id']}")
        print(f"Verified truth {f['id']}: {f['bytes']} bytes SHA-256 {f['sha256']}")
    for f in tracks:
        verify_track(safe_path(MANIFEST.parent, f["path"]), f)
        print(f"Verified track {f['id']}: {f['expected_vertices']} vertices SHA-256 {f['sha256']}")
    for f in classifications:
        decoded = verify_classification(safe_path(MANIFEST.parent, f["path"]), f)
        print(f"Verified classification {f['id']}: {decoded['class_counts']['20']} AP/ground-clutter gates SHA-256 {f['sha256']}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError) as error:
        print(f"Corpus provisioning failed: {error}", file=sys.stderr)
        sys.exit(1)
