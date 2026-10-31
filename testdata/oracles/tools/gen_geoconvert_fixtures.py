# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Generate the GeographicLib `GeoConvert` fixture for `gungnir-coord`'s UTM, UPS and MGRS.

Row: DN-37 section 6.1 row d (`docs/design/DN-37-symbology-cursor-readout-and-declutter.md`),
a draft until the owner enters it in `docs/verification-capability-table.md`. The criterion the
design states is an identical MGRS string and a UTM or UPS position within a millimetre.

The cases are the edges first -- every UTM zone's boundary, every latitude band's, the
Norway and Svalbard exceptions, the equator, the antimeridian, both polar caps and both
poles -- then seeded random sweeps of UTM and of each cap.

The oracle is the `GeoConvert` command-line tool of GeographicLib. It is run once per output
format with every case on standard input, so a case's three answers come from one release.
The release is read from `GeoConvert --version` and stamped into the fixture; this machine
used the conda-forge `geographiclib-cpp` 2.5.2 package:

    micromamba create -p <env> -c conda-forge --override-channels geographiclib-cpp=2.5.2

Run from `testdata/oracles/`, with the environment's `GeoConvert` on the path or named in
`GEOCONVERT`:

    python tools/gen_geoconvert_fixtures.py

Writes `coord/geoconvert.json`. Angles in the fixture are degrees, distances metres.
"""

from __future__ import annotations

import json
import os
import platform
import random
import re
import shutil
import subprocess
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "coord" / "geoconvert.json"

# MGRS precisions the fixture records, as digits for each axis: one metre, ten and a
# hundred metres of the readout's coarser forms, and the square alone.
DIGITS = (5, 3, 0)


def geoconvert() -> str:
    exe = os.environ.get("GEOCONVERT") or shutil.which("GeoConvert")
    if not exe:
        sys.exit("GeoConvert not found: put it on the path or set GEOCONVERT")
    return exe


def run(exe: str, args: list[str], lines: list[str]) -> list[str]:
    done = subprocess.run(
        [exe, *args],
        input="\n".join(lines) + "\n",
        capture_output=True,
        text=True,
        check=True,
    )
    out = done.stdout.splitlines()
    if len(out) != len(lines):
        sys.exit(f"GeoConvert {args}: {len(out)} answers for {len(lines)} inputs")
    return out


def edge_cases() -> list[tuple[str, float, float]]:
    cases: list[tuple[str, float, float]] = []
    # Every zone boundary, and a hair either side, at a mid latitude of each hemisphere.
    for lat in (10.0, 45.0, -33.0):
        for lon in range(-180, 180, 6):
            for d, tag in ((0.0, "on"), (1e-6, "above"), (-1e-6, "below")):
                cases.append((f"zone_edge_{lon}_{tag}_lat{lat:g}", lat, lon + d))
    # Every latitude band boundary, and a hair either side.
    for lat in range(-80, 85, 8):
        for d, tag in ((0.0, "on"), (1e-9, "above"), (-1e-9, "below")):
            cases.append((f"band_edge_{lat}_{tag}", lat + d, 3.0))
    cases.append(("band_x_top", 83.999999, 3.0))
    # The equator, including a negative zero, which GeoConvert reads as south.
    cases += [
        ("equator_prime", 0.0, 0.0),
        ("equator_negative_zero", -0.0, 10.0),
        ("equator_just_north", 1e-9, 10.0),
        ("equator_just_south", -1e-9, 10.0),
        ("equator_antimeridian_east", 0.0, 180.0),
        ("equator_antimeridian_west", 0.0, -180.0),
    ]
    # Norway: the zone 32 widening between 56 and 64 north, and its edges.
    for lat in (55.999999, 56.0, 60.0, 63.999999, 64.0):
        for lon in (2.999999, 3.0, 6.0, 11.999999, 12.0):
            cases.append((f"norway_{lat}_{lon}", lat, lon))
    # Svalbard: zones 31, 33, 35 and 37 over 72 to 84 north, and their edges.
    for lat in (71.999999, 72.0, 78.0, 83.999999):
        for lon in (-0.000001, 0.0, 8.999999, 9.0, 20.999999, 21.0, 32.999999, 33.0, 41.999999, 42.0):
            cases.append((f"svalbard_{lat}_{lon}", lat, lon))
    # The caps: the limits, the pole, and the four meridians that cut them.
    for lat in (84.0, 84.000001, 85.0, 89.0, 89.999999, 90.0):
        for lon in (0.0, 45.0, 90.0, 135.0, 179.999999, 180.0, -180.0, -179.999999, -135.0, -90.0, -45.0, -0.000001, -0.0):
            cases.append((f"north_cap_{lat}_{lon}", lat, lon))
    for lat in (-80.0, -80.000001, -81.0, -85.0, -89.0, -89.999999, -90.0):
        for lon in (0.0, 45.0, 90.0, 135.0, 179.999999, 180.0, -180.0, -179.999999, -135.0, -90.0, -45.0, -0.000001, -0.0):
            cases.append((f"south_cap_{lat}_{lon}", lat, lon))
    # Places a reader will recognise.
    cases += [
        ("corvallis", 44.5646, -123.2620),
        ("greenwich", 51.4779, -0.0015),
        ("sydney", -33.8568, 151.2153),
        ("quito", -0.1807, -78.4678),
        ("longyearbyen", 78.2232, 15.6267),
        ("tromso", 69.6492, 18.9553),
        ("ushuaia", -54.8019, -68.3030),
        ("mcmurdo", -77.8419, 166.6863),
        ("alert", 82.5018, -62.3481),
    ]
    return cases


def random_cases(rng: random.Random) -> list[tuple[str, float, float]]:
    cases = []
    for i in range(1500):
        lat = round(rng.uniform(-80.0, 84.0), 6)
        cases.append((f"utm_sweep_{i}", lat, round(rng.uniform(-180.0, 180.0), 6)))
    for i in range(300):
        cases.append((f"north_cap_sweep_{i}", round(rng.uniform(84.0, 90.0), 6), round(rng.uniform(-180.0, 180.0), 6)))
        cases.append((f"south_cap_sweep_{i}", round(rng.uniform(-90.0, -80.0), 6), round(rng.uniform(-180.0, 180.0), 6)))
    return cases


def tidy(v: float) -> float:
    """`v` to twelve decimal places (a tenth of a nanometre), so the decimal text GeoConvert
    reads and the double the fixture records are the same number. A negative zero stays one."""
    return float(f"{v:.12f}")


def fmt(v: float) -> str:
    # A negative zero must reach GeoConvert as "-0", whose sign decides the hemisphere.
    return "-0" if v == 0.0 and str(v).startswith("-") else f"{v:.12f}".rstrip("0").rstrip(".")


def main() -> None:
    exe = geoconvert()
    version = re.search(r"version (\S+)", subprocess.run([exe, "--version"], capture_output=True, text=True).stdout).group(1)
    rng = random.Random(20261031)
    cases = [(n, tidy(lat), tidy(lon)) for n, lat, lon in edge_cases() + random_cases(rng)]
    lines = [f"{fmt(lat)} {fmt(lon)}" for _, lat, lon in cases]

    grid = run(exe, ["-u", "-p", "5"], lines)
    by_digits = {d: run(exe, ["-m", "-p", str(d - 5)], lines) for d in DIGITS}

    out_cases = []
    for i, (name, lat, lon) in enumerate(cases):
        zone_hemi, easting, northing = grid[i].split()
        m = re.fullmatch(r"(\d*)([ns])", zone_hemi)
        out_cases.append(
            {
                "name": name,
                "lat_deg": lat,
                "lon_deg": lon,
                "zone": int(m.group(1)) if m.group(1) else 0,
                "north": m.group(2) == "n",
                "easting_m": float(easting),
                "northing_m": float(northing),
                "mgrs": {str(d): by_digits[d][i] for d in DIGITS},
            }
        )

    fixture = {
        "row": "DN-37 section 6.1 row d: MGRS, UTM and UPS in gungnir-coord",
        "oracle": "GeographicLib GeoConvert",
        "oracle_version": version,
        "python": platform.python_version(),
        "generated_by": "testdata/oracles/tools/gen_geoconvert_fixtures.py",
        "seed": 20261031,
        "units": "degrees; metres; MGRS keyed by digits per axis",
        "cases": out_cases,
    }
    OUT.write_text(json.dumps(fixture, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {len(out_cases)} cases to {OUT} (GeoConvert {version})")


if __name__ == "__main__":
    main()
