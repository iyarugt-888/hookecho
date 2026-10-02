#!/usr/bin/env python3
"""Capture fresh IEM truth candidates for review; never overwrite the pinned corpus."""
import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path
import urllib.request

import provision


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=provision.REPO / "target/parity-review/truth-candidates")
    args = parser.parse_args()
    destination = args.output.resolve()
    if destination == provision.MANIFEST.parent.resolve() or provision.MANIFEST.parent.resolve() in destination.parents:
        parser.error("Candidate output must be separate from the pinned corpus")
    _, snapshots, _ = provision.load_manifest(provision.MANIFEST)
    destination.mkdir(parents=True, exist_ok=True)
    report = []
    for f in snapshots:
        request = urllib.request.Request(f["source"]["url"], headers={"User-Agent": "HookEcho scientific fixture acquisition"})
        with urllib.request.urlopen(request, timeout=90) as response:
            data = response.read(2_000_001)
        if len(data) > 2_000_000:
            raise ValueError("Truth candidate exceeds corpus bound")
        collection = json.loads(data)
        if collection.get("type") != "FeatureCollection" or not isinstance(collection.get("features"), list):
            raise ValueError("Truth response is not a GeoJSON feature collection")
        provision.safe_path(destination, f["path"]).write_bytes(data)
        report.append({"id": f["id"], "url": f["source"]["url"], "captured_at": dt.datetime.now(dt.timezone.utc).isoformat(),
                       "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(), "features": len(collection["features"]),
                       "baseline_sha256": f["sha256"], "baseline_features": f["expected_features"]})
        print(f"Captured candidate {f['id']}: {len(collection['features'])} features; baseline remains unchanged")
    (destination / "candidate-report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
