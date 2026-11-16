#!/usr/bin/env python3
"""Packs Natural Earth's 1:110m coastlines (public domain, naturalearthdata.com) into
src/coast110.bin for the great circle map: for each line a little-endian u16 point
count, then i16 longitude and latitude pairs in hundredths of a degree.

    curl -LO https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_110m_coastline.geojson
    python3 tools/coastlines.py ne_110m_coastline.geojson src/coast110.bin
"""
import json, struct, sys

src, dst = sys.argv[1], sys.argv[2]
lines = []
for f in json.load(open(src))["features"]:
    g = f["geometry"]
    parts = [g["coordinates"]] if g["type"] == "LineString" else g["coordinates"]
    lines += parts
out = bytearray()
n = 0
for pts in lines:
    out += struct.pack("<H", len(pts))
    for lon, lat in pts:
        out += struct.pack("<hh", round(lon * 100), round(lat * 100))
    n += len(pts)
open(dst, "wb").write(out)
print(f"{len(lines)} lines, {n} points, {len(out)} bytes")
