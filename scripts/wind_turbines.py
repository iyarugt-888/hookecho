"""Build crates/wxdata/data/wind_turbines.bin from the USGS U.S. Wind Turbine Database (USWTDB,
public domain: https://eerscmap.usgs.gov/uswtdb/, the CSV zip under Data).

Every 0.01-degree cell (floor of lon and lat x 100) holding a turbine, with the earliest year a
turbine there came online (0 when the database gives none: treated as always in service).
Format, little-endian:
    b"HEWT", u32 cell count, then per cell, keys ascending:
    varint (key - previous key), u8 (year - 1980, or 0 for unknown)
    key = (floor(lat*100) + 9000) * 36000 + (floor(lon*100) + 18000)

Usage: python scripts/wind_turbines.py path/to/uswtdb_V*.csv [out.bin]
"""
import csv, math, struct, sys

BASE_YEAR = 1980


def cells(csv_path):
    """{key: earliest year (0 unknown)} for every 0.01-degree cell with a turbine."""
    out = {}
    for t in csv.DictReader(open(csv_path, encoding="utf-8", errors="replace")):
        try:
            lon, lat = float(t["xlong"]), float(t["ylat"])
        except ValueError:
            continue
        y = t["p_year"].strip()
        year = int(y) if y.lstrip("-").isdigit() and int(y) > BASE_YEAR else 0
        key = (math.floor(lat * 100) + 9000) * 36000 + (math.floor(lon * 100) + 18000)
        out[key] = min(out.get(key, year), year) if key in out else year
    return out


def varint(n):
    b = bytearray()
    while True:
        byte = n & 0x7F
        n >>= 7
        if n:
            b.append(byte | 0x80)
        else:
            b.append(byte)
            return bytes(b)


def encode(c):
    out = bytearray(b"HEWT") + struct.pack("<I", len(c))
    prev = 0
    for key in sorted(c):
        y = c[key]
        out += varint(key - prev) + bytes([y - BASE_YEAR if y else 0])
        prev = key
    return bytes(out)


if __name__ == "__main__":
    src = sys.argv[1]
    dst = sys.argv[2] if len(sys.argv) > 2 else "crates/wxdata/data/wind_turbines.bin"
    c = cells(src)
    data = encode(c)
    open(dst, "wb").write(data)
    print(f"{len(c)} cells, {len(data)} bytes -> {dst}")
