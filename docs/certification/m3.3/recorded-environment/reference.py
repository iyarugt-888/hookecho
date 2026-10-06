"""Independent fixed-column HGHT/TEMP reader; stdlib only, no HookEcho imports.

Regenerates reference.json from the pinned bytes. Receipt time and reported observation
metadata were not retained in the legacy cache; request identities are not reported times.
"""
import hashlib
import json
from pathlib import Path
from urllib.parse import urlencode

root = Path(__file__).resolve().parent
records = []
for name, launch in [("72357-2013052012.txt", "2013-05-20 12:00:00"),
                     ("72469-2017050812.txt", "2017-05-08 12:00:00")]:
    data = (root / name).read_bytes()
    rows = []
    for line in data.decode().splitlines():
        fields = []
        for lo in (0, 7, 14):
            try:
                fields.append(float(line[lo:lo + 7]))
            except ValueError:
                fields.append(None)
        pressure, height, temperature = fields
        if pressure is not None and 1 <= pressure <= 1100:
            rows.append((height, temperature))
    values = []
    for threshold in (0, -10, -20, -30, -40):
        crossings = {h for h, t in rows if h is not None and t == threshold}
        cooling = []
        for (h0, t0), (h1, t1) in zip(rows, rows[1:]):
            if None in (h0, t0, h1, t1) or h1 <= h0:
                continue
            if min(t0, t1) < threshold < max(t0, t1):
                crossings.add(h0 + (h1 - h0) * (threshold - t0) / (t1 - t0))
            if t0 > threshold >= t1:
                cooling.append(h0 + (h1 - h0) * (threshold - t0) / (t1 - t0))
        values.append({"temperature_c": threshold, "crossings_m": sorted(crossings),
                       "lowest_m": min(crossings) if crossings else None,
                       "highest_cooling_m": max(cooling) if cooling else None})
    query = urlencode({"datetime": launch, "id": name[:5], "src": "UNKNOWN", "type": "TEXT:LIST"})
    records.append({"file": name, "sha256": hashlib.sha256(data).hexdigest(),
                    "request_url": "https://weather.uwyo.edu/wsgi/sounding?" + query,
                    "launch_selection_utc": launch.replace(" ", "T") + "Z",
                    "receipt_time_utc": None, "reported_observation_time_utc": None,
                    "datum": "recorded HGHT geopotential metres MSL",
                    "profile_rows": len(rows), "isotherms": values})
(root / "reference.json").write_text(json.dumps(records, indent=2) + "\n", encoding="utf-8")
print(json.dumps(records, indent=2))
