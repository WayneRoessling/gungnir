# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Generate the pymap3d oracle fixtures for the `gungnir-coord` capability-table row.

Row: "Coordinate frame transforms (ECEF/ENU/NED/geodetic)" in
`docs/verification-capability-table.md` §1. Pass criterion: position error < 1e-6 m
against pymap3d.

The cases deliberately include the adversarial pole and antimeridian geometry that
`docs/scenario-crate-narrative.md` Scenario 5 calls for, plus negative and very large
altitudes, because the closed-form quartic in `gungnir-coord` is chosen precisely for
those.

Run from `testdata/oracles/` with the venv described in `../README.md`:

    python tools/gen_coord_fixtures.py

Writes `coord/pymap3d.json`. Angles in the fixture are radians, distances metres.
"""

from __future__ import annotations

import json
import math
import platform
import sys
from pathlib import Path

import pymap3d

OUT = Path(__file__).resolve().parent.parent / "coord" / "pymap3d.json"

# (name, lat_deg, lon_deg, alt_m)
GEODETIC_CASES = [
    ("origin_equator_prime_meridian", 0.0, 0.0, 0.0),
    ("equator_east", 0.0, 90.0, 0.0),
    ("equator_antimeridian", 0.0, 180.0, 0.0),
    ("equator_antimeridian_negative", 0.0, -180.0, 0.0),
    ("mid_latitude_north", 45.0, 30.0, 0.0),
    ("mid_latitude_south", -37.8136, 144.9631, 31.0),
    ("high_altitude_aircraft", 51.4775, -0.0014, 12_000.0),
    ("geostationary_altitude", 0.0, 100.0, 35_786_000.0),
    ("below_ellipsoid", 31.5, 35.0, -430.0),
    ("north_pole", 90.0, 0.0, 0.0),
    ("south_pole", -90.0, 0.0, 0.0),
    ("north_pole_with_altitude", 90.0, 0.0, 1_000.0),
    ("near_north_pole", 89.999_999, 45.0, 100.0),
    ("near_antimeridian_east", 12.0, 179.999_999, 500.0),
    ("near_antimeridian_west", 12.0, -179.999_999, 500.0),
    ("arctic_high_latitude", 78.9, 11.9, 25.0),
]

# ENU cases: (name, origin_deg triple, target_deg triple)
ENU_CASES = [
    (
        "short_baseline_mid_latitude",
        (45.0, 30.0, 0.0),
        (45.001, 30.001, 100.0),
    ),
    (
        "same_point",
        (45.0, 30.0, 100.0),
        (45.0, 30.0, 100.0),
    ),
    (
        "hundred_km_east",
        (-37.8136, 144.9631, 31.0),
        (-37.8136, 146.0, 31.0),
    ),
    (
        "across_the_antimeridian",
        (12.0, 179.99, 0.0),
        (12.0, -179.99, 0.0),
    ),
    (
        "polar_origin",
        (90.0, 0.0, 0.0),
        (89.5, 45.0, 0.0),
    ),
    (
        "polar_origin_south",
        (-90.0, 0.0, 0.0),
        (-89.5, -120.0, 250.0),
    ),
    (
        "long_baseline_continental",
        (51.4775, -0.0014, 0.0),
        (40.7128, -74.0060, 0.0),
    ),
    (
        "high_target_over_low_origin",
        (0.0, 0.0, 0.0),
        (0.0, 0.0, 400_000.0),
    ),
]


def rad(triple):
    lat, lon, alt = triple
    return math.radians(lat), math.radians(lon), alt


def main() -> int:
    geodetic_to_ecef = []
    ecef_to_geodetic = []
    for name, lat_d, lon_d, alt in GEODETIC_CASES:
        lat, lon = math.radians(lat_d), math.radians(lon_d)
        x, y, z = pymap3d.geodetic2ecef(lat, lon, alt, deg=False)
        geodetic_to_ecef.append(
            {
                "name": name,
                "geodetic": {"lat_rad": lat, "lon_rad": lon, "alt_m": alt},
                "ecef": {"x_m": float(x), "y_m": float(y), "z_m": float(z)},
            }
        )
        # The reverse direction is generated from the same ECEF point so that a
        # disagreement is attributable to one transform rather than to the round trip.
        rlat, rlon, ralt = pymap3d.ecef2geodetic(x, y, z, deg=False)
        ecef_to_geodetic.append(
            {
                "name": name,
                "ecef": {"x_m": float(x), "y_m": float(y), "z_m": float(z)},
                "geodetic": {
                    "lat_rad": float(rlat),
                    "lon_rad": float(rlon),
                    "alt_m": float(ralt),
                },
            }
        )

    enu = []
    for name, origin_d, target_d in ENU_CASES:
        olat, olon, oalt = rad(origin_d)
        tlat, tlon, talt = rad(target_d)
        x, y, z = pymap3d.geodetic2ecef(tlat, tlon, talt, deg=False)
        e, n, u = pymap3d.ecef2enu(x, y, z, olat, olon, oalt, deg=False)
        bx, by, bz = pymap3d.enu2ecef(e, n, u, olat, olon, oalt, deg=False)
        enu.append(
            {
                "name": name,
                "origin": {"lat_rad": olat, "lon_rad": olon, "alt_m": oalt},
                "ecef": {"x_m": float(x), "y_m": float(y), "z_m": float(z)},
                "enu": {"e_m": float(e), "n_m": float(n), "u_m": float(u)},
                "enu_to_ecef": {"x_m": float(bx), "y_m": float(by), "z_m": float(bz)},
            }
        )

    doc = {
        "row": "Coordinate frame transforms (ECEF/ENU/NED/geodetic)",
        "oracle": "pymap3d",
        "oracle_version": pymap3d.__version__,
        "python": platform.python_version(),
        "ellipsoid": "wgs84",
        "units": {"angles": "radians", "distances": "metres"},
        "generated_by": "testdata/oracles/tools/gen_coord_fixtures.py",
        "geodetic_to_ecef": geodetic_to_ecef,
        "ecef_to_geodetic": ecef_to_geodetic,
        "enu": enu,
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(doc, indent=2, sort_keys=False) + "\n", encoding="utf-8")
    print(f"wrote {OUT} ({len(geodetic_to_ecef)} geodetic cases, {len(enu)} ENU cases)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
