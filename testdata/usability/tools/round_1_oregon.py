#!/usr/bin/env python3
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md
"""Write `round-1-oregon.json`: round 1's baseline moved over the Oregon DEM clip.

GAP-252, `docs/design/DN-35-terrain-in-the-three-d-scene.md` amendment 2 item 2. Every
geodetic position in `round-1.json` keeps its east, north and up offset from round 1's
origin and is re-expressed at the new origin, 43.67 N, 123.25 W. Three heights come from
the ground instead, because round 1's are heights at sea level:

- the origin's height is the ground's under it, so the seed's local heights read as above
  the origin's ground, as round 1's read as above the sea;
- each radar's is the ground's under it plus its mast, round 1's own height (25 m, 30 m);
- each approach point's is the ground's under it plus `APPROACH_CLEARANCE_M`, and the axis
  is sampled every `SAMPLE_M` to report the least clearance between the points.

The laydowns are local; their placements are re-derived from the moved geodetic positions,
as round 1's were, with each sensor at the ground plus its mast and each resource on the
ground. A ground height is the DEM's NAVD88 height (bilinear between cell centres), made
NAD83(2011) ellipsoidal through the committed GEOID18 clip, then carried to ITRF2014
(taken as WGS 84) by PROJ's "Inverse of ITRF2014 to NAD83(2011) (1)" at `EPOCH`: the path
the desktop takes with the realisation and vertical the variant declares.

Needs numpy and pyproj (any PROJ that reads a GeoTIFF grid). Run from the repository root:

    python testdata/usability/tools/round_1_oregon.py

It prints each derived height and the approach's least clearance, and rewrites the file.
"""

import json
import math
import struct
import sys
import zlib
from pathlib import Path

import numpy as np
from pyproj import Transformer

ROOT = Path(__file__).resolve().parents[3]
DEM = ROOT / "testdata/dem/usgs_1_n44w124_clip.tif"
GEOID18 = ROOT / "testdata/geoid/g2018u0_clip_43n45n_124w122w.tif"
ROUND_1 = ROOT / "testdata/usability/round-1.json"
OUT = ROOT / "testdata/usability/round-1-oregon.json"

NEW_ORIGIN_DEG = (43.67, -123.25)
MASTS_M = {1: 25.0, 2: 30.0}
APPROACH_CLEARANCE_M = 300.0
SAMPLE_M = 50.0
EPOCH = 2026.83
NAD83_REALISATION = "epsg:6318"
VERTICAL = "epsg:5703"

A = 6378137.0
F = 1 / 298.257223563
E2 = F * (2 - F)


def ecef(lat, lon, h):
    n = A / math.sqrt(1 - E2 * math.sin(lat) ** 2)
    return np.array(
        [
            (n + h) * math.cos(lat) * math.cos(lon),
            (n + h) * math.cos(lat) * math.sin(lon),
            (n * (1 - E2) + h) * math.sin(lat),
        ]
    )


def geodetic(x):
    lon = math.atan2(x[1], x[0])
    p = math.hypot(x[0], x[1])
    lat = math.atan2(x[2], p * (1 - E2))
    for _ in range(10):
        n = A / math.sqrt(1 - E2 * math.sin(lat) ** 2)
        h = p / math.cos(lat) - n
        lat = math.atan2(x[2], p * (1 - E2 * n / (n + h)))
    n = A / math.sqrt(1 - E2 * math.sin(lat) ** 2)
    return lat, lon, p / math.cos(lat) - n


def rotation(lat, lon):
    sl, cl, so, co = math.sin(lat), math.cos(lat), math.sin(lon), math.cos(lon)
    return np.array([[-so, co, 0.0], [-sl * co, -sl * so, cl], [cl * co, cl * so, sl]])


def to_enu(origin, p):
    return rotation(origin[0], origin[1]) @ (ecef(*p) - ecef(*origin))


def from_enu(origin, enu):
    return geodetic(ecef(*origin) + rotation(origin[0], origin[1]).T @ np.asarray(enu))


def read_dem(path):
    """The clip's heights and its grid, read with nothing but zlib: a little-endian TIFF of
    32-bit floats in DEFLATE strips or tiles under the floating-point predictor, as
    `testdata/dem/SOURCE.md` describes it."""
    data = path.read_bytes()
    if data[:4] != b"II*\x00":
        sys.exit(f"{path} is not a little-endian classic TIFF")
    (ifd,) = struct.unpack_from("<I", data, 4)
    (count,) = struct.unpack_from("<H", data, ifd)
    sizes = {1: 1, 2: 1, 3: 2, 4: 4, 12: 8}
    formats = {1: "B", 2: "s", 3: "H", 4: "I", 12: "d"}
    tags = {}
    for i in range(count):
        tag, kind, n, value = struct.unpack_from("<HHII", data, ifd + 2 + 12 * i)
        size = sizes[kind] * n
        at = ifd + 2 + 12 * i + 8 if size <= 4 else value
        if kind == 2:
            tags[tag] = data[at : at + n]
        else:
            tags[tag] = struct.unpack_from(f"<{n}{formats[kind]}", data, at)
    width, height = tags[256][0], tags[257][0]
    if tags[258][0] != 32 or tags.get(339, (1,))[0] != 3 or tags[259][0] not in (8, 32946):
        sys.exit("expected DEFLATE-compressed 32-bit floats")
    predictor = tags.get(317, (1,))[0]
    if 322 in tags:
        bw, bh = tags[322][0], tags[323][0]
        offsets, counts = tags[324], tags[325]
        across = (width + bw - 1) // bw
    else:
        bw, bh = width, tags[278][0]
        offsets, counts = tags[273], tags[279]
        across = 1
    heights = np.empty((height, width), dtype=np.float64)
    for k, (offset, length) in enumerate(zip(offsets, counts)):
        raw = np.frombuffer(zlib.decompress(data[offset : offset + length]), dtype=np.uint8)
        rows = raw.size // (bw * 4)
        block = raw[: rows * bw * 4].reshape(rows, bw * 4)
        if predictor == 3:
            block = np.cumsum(block, axis=1, dtype=np.uint64).astype(np.uint8)
            planes = block.reshape(rows, 4, bw)
            values = planes.transpose(0, 2, 1).copy().view(">f4").reshape(rows, bw)
        elif predictor == 1:
            values = block.view("<f4").reshape(rows, bw)
        else:
            sys.exit(f"predictor {predictor} is not read here")
        r0, c0 = (k // across) * bh, (k % across) * bw
        r1, c1 = min(r0 + rows, height), min(c0 + bw, width)
        heights[r0:r1, c0:c1] = values[: r1 - r0, : c1 - c0]
    scale, tie = tags[33550], tags[33922]
    west, north = tie[3] - tie[0] * scale[0], tie[4] + tie[1] * scale[1]
    nodata = float(tags[42113].rstrip(b"\x00")) if 42113 in tags else None
    if nodata is not None and np.any(heights == nodata):
        sys.exit("the clip has a no-data cell; SOURCE.md says it has none")
    return heights, west, north, scale[0], scale[1]


HEIGHTS, WEST, NORTH, DLON, DLAT = read_dem(DEM)
ROWS, COLUMNS = HEIGHTS.shape

TO_NAD83 = Transformer.from_crs("EPSG:7912", "EPSG:6319", always_xy=True)
TO_ITRF = Transformer.from_crs("EPSG:6319", "EPSG:7912", always_xy=True)
GEOID = Transformer.from_pipeline(
    "+proj=pipeline +step +proj=unitconvert +xy_in=deg +xy_out=rad "
    f"+step +proj=vgridshift +grids={GEOID18.as_posix()} +multiplier=1 "
    "+step +proj=unitconvert +xy_in=rad +xy_out=deg"
)


def navd88(lat_deg, lon_deg):
    """The DEM's height at a NAD83(2011) position, bilinear between cell centres, or
    `None` off the clip."""
    x = (lon_deg - WEST) / DLON - 0.5
    y = (NORTH - lat_deg) / DLAT - 0.5
    if not (0 <= x <= COLUMNS - 1 and 0 <= y <= ROWS - 1):
        return None
    c, r = min(int(x), COLUMNS - 2), min(int(y), ROWS - 2)
    fx, fy = x - c, y - r
    h = HEIGHTS[r : r + 2, c : c + 2]
    return (h[0, 0] * (1 - fx) + h[0, 1] * fx) * (1 - fy) + (h[1, 0] * (1 - fx) + h[1, 1] * fx) * fy


def ground(lat, lon, required=True):
    """The ground's WGS 84 ellipsoidal height under a WGS 84 position, radians, or `None`
    off the clip when not `required`."""
    lon83, lat83, _, _ = TO_NAD83.transform(math.degrees(lon), math.degrees(lat), 0.0, EPOCH)
    height = navd88(lat83, lon83)
    if height is None:
        if required:
            sys.exit(f"({lat83:.5f}, {lon83:.5f}) is off the clip")
        return None
    _, _, undulation = GEOID.transform(lon83, lat83, 0.0)
    if not math.isfinite(undulation) or abs(undulation) > 60:
        sys.exit(f"no GEOID18 undulation at ({lat83}, {lon83}): {undulation}")
    _, _, h, _ = TO_ITRF.transform(lon83, lat83, height + undulation, EPOCH)
    return h


def main():
    round_1 = json.loads(ROUND_1.read_text(encoding="utf-8"))
    old = tuple(round_1["origin"])
    lat0, lon0 = map(math.radians, NEW_ORIGIN_DEG)
    origin = (lat0, lon0, ground(lat0, lon0))
    print(f"origin: {NEW_ORIGIN_DEG}, ground {origin[2]:.2f} m ellipsoidal")

    def moved(p):
        return list(from_enu(origin, to_enu(old, p)))

    def on_ground(p, above):
        lat, lon, _ = p
        return [lat, lon, ground(lat, lon) + above]

    out = dict(round_1)
    out["origin"] = list(origin)
    out["terrain"] = {
        "path": "testdata/dem/usgs_1_n44w124_clip.tif",
        "frame": "epsg:4269",
        "vertical": VERTICAL,
        "nad83_realisation": NAD83_REALISATION,
    }
    out["sensors"] = []
    for sensor in round_1["sensors"]:
        placed = on_ground(moved(sensor["position"]), MASTS_M[sensor["id"]])
        print(f"sensor {sensor['id']}: ground {placed[2] - MASTS_M[sensor['id']]:.2f} m, mast {MASTS_M[sensor['id']]} m")
        out["sensors"].append({**sensor, "position": placed})
    for key in ("resources", "assets"):
        out[key] = [{**item, "position": moved(item["position"])} for item in round_1[key]]
    out["geofences"] = [{**g, "center": moved(g["center"])} for g in round_1["geofences"]]
    out["hazards"] = [{**h, "points": [moved(p) for p in h["points"]]} for h in round_1["hazards"]]

    out["approaches"] = []
    for approach in round_1["approaches"]:
        moved_points = [moved(p) for p in approach["points"]]
        enu = [to_enu(origin, p) for p in moved_points]
        samples = []
        for a, b in zip(enu, enu[1:]):
            steps = max(1, int(np.linalg.norm(b[:2] - a[:2]) // SAMPLE_M))
            samples += [a + (b - a) * i / steps for i in range(steps + 1)]
        grounds = []
        for s in samples:
            lat, lon, _ = from_enu(origin, s)
            grounds.append(ground(lat, lon, required=False))
        on_clip = [g for g in grounds if g is not None]
        # The points run outer end first, so the part off the clip must be a prefix.
        off = len(grounds) - len(on_clip)
        if any(g is None for g in grounds[off:]):
            sys.exit("the axis leaves the clip somewhere other than its outer end")
        flown = math.ceil(max(on_clip) + APPROACH_CLEARANCE_M)
        points = [[lat, lon, float(flown)] for lat, lon, _ in moved_points]
        print(
            f"approach {approach['name']!r}: flown at {flown} m ellipsoidal, {APPROACH_CLEARANCE_M} m over "
            f"the highest ground on the clip ({max(on_clip):.1f} m); {len(grounds) - len(on_clip)} of "
            f"{len(grounds)} samples at {SAMPLE_M} m are off the clip, all at the outer end"
        )
        out["approaches"].append({**approach, "points": points})

    # A resource kept its offset from the origin, so round 1's laydown placements of the
    # resources stand as they are. A sensor did not: each laydown's is re-sited at the
    # ground under it plus its mast, so `current` is the sensors as the baseline sites them.
    def re_sited(enu, above):
        lat, lon, _ = from_enu(origin, enu)
        return [round(float(v), 3) for v in to_enu(origin, [lat, lon, ground(lat, lon) + above])]

    out["laydowns"] = [
        {
            **laydown,
            "sensors": [
                {**s, "position_enu": re_sited(s["position_enu"], MASTS_M[s["sensor"]])}
                for s in laydown["sensors"]
            ],
        }
        for laydown in round_1["laydowns"]
    ]
    OUT.write_text(json.dumps(out, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT).as_posix()}")


if __name__ == "__main__":
    main()
