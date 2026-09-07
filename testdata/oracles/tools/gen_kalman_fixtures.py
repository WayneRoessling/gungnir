# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Generate the filterpy oracle fixture for the `gungnir-filters` linear-KF row.

Row: "Linear Kalman Filter" in `docs/verification-capability-table.md` §1.
Pass criterion: state < 1e-6 and covariance Frobenius difference < 1e-6, over the same
measurement sequence. Data source: synthetic fixture.

The oracle is `filterpy.kalman.KalmanFilter` driven step by step, with the *whole*
trajectory recorded rather than only the final state: a filter can arrive at the right
answer through a wrong path, and comparing every step is what catches that. Both the
post-predict and the post-update state are recorded, so a failure says which half of
the cycle is wrong.

Matrices are in this workspace's block state ordering `[e, n, u, ve, vn, vu]`, which
`gungnir-core` fixes and `gungnir_track::Track` documents; `F` and `Q` come from
filterpy's own `kinematic_kf` and `Q_continuous_white_noise` with `order_by_dim=False`,
so the fixture cannot silently disagree with the motion-model fixture next to it.

Run from `testdata/oracles/` with the venv described in `../README.md`:

    python tools/gen_kalman_fixtures.py

Writes `filters/linear_kalman.json`.
"""

from __future__ import annotations

import json
import platform
import sys
from pathlib import Path

import filterpy
import filterpy.common
import numpy as np
from filterpy.kalman import KalmanFilter

OUT = Path(__file__).resolve().parent.parent / "filters" / "linear_kalman.json"

# Position-only observation of a 6-state constant-velocity target: the shape every
# gungnir-scenario radar produces.
H = np.zeros((3, 6))
H[0, 0] = H[1, 1] = H[2, 2] = 1.0


def case(
    name: str,
    dt: float,
    sigma_a_sq: float,
    r_diag,
    p0_diag,
    x0,
    measurements,
) -> dict:
    kf = KalmanFilter(dim_x=6, dim_z=3)
    kf.x = np.array(x0, dtype=float).reshape(6, 1)
    kf.P = np.diag(np.array(p0_diag, dtype=float))
    kf.H = H.copy()
    kf.R = np.diag(np.array(r_diag, dtype=float))
    kf.F = np.asarray(
        filterpy.common.kinematic_kf(dim=3, order=1, dt=dt, order_by_dim=False).F,
        dtype=float,
    )
    kf.Q = np.asarray(
        filterpy.common.Q_continuous_white_noise(
            2, dt, sigma_a_sq, block_size=3, order_by_dim=False
        ),
        dtype=float,
    )

    steps = []
    for z in measurements:
        kf.predict()
        predicted = {
            "x": [float(v) for v in kf.x.flatten()],
            "p": [[float(v) for v in row] for row in kf.P],
        }
        kf.update(np.array(z, dtype=float).reshape(3, 1))
        steps.append(
            {
                "z": [float(v) for v in z],
                "after_predict": predicted,
                "after_update": {
                    "x": [float(v) for v in kf.x.flatten()],
                    "p": [[float(v) for v in row] for row in kf.P],
                },
            }
        )

    return {
        "name": name,
        "dt": dt,
        "sigma_a_sq": sigma_a_sq,
        "r_diag": list(map(float, r_diag)),
        "p0_diag": list(map(float, p0_diag)),
        "x0": list(map(float, x0)),
        "steps": steps,
    }


def straight_line(n: int, dt: float, speed: float, noise_seed: int):
    """A target moving east at `speed`, with a fixed pseudo-random wobble.

    The wobble is generated from a seeded numpy Generator so the fixture is
    reproducible; it exists so the filter is not fed a perfectly consistent
    measurement sequence, where a wrong gain would still look right.
    """
    rng = np.random.default_rng(noise_seed)
    out = []
    for k in range(n):
        t = (k + 1) * dt
        truth = np.array([speed * t, 0.0, 1000.0])
        out.append(truth + rng.normal(0.0, 5.0, 3))
    return [list(map(float, z)) for z in out]


def main() -> int:
    cases = [
        case(
            "position_only_cv_1hz",
            dt=1.0,
            sigma_a_sq=1.0,
            r_diag=[25.0, 25.0, 25.0],
            p0_diag=[100.0] * 6,
            x0=[0.0] * 6,
            measurements=straight_line(40, 1.0, 200.0, 12345),
        ),
        case(
            "fast_rate_tight_noise",
            dt=0.05,
            sigma_a_sq=0.25,
            r_diag=[1.0, 1.0, 4.0],
            p0_diag=[10.0, 10.0, 10.0, 100.0, 100.0, 100.0],
            x0=[0.0, 0.0, 1000.0, 180.0, 0.0, 0.0],
            measurements=straight_line(60, 0.05, 180.0, 777),
        ),
        case(
            # A deliberately over-confident prior against a distant first
            # measurement: the step where a wrong gain shows up most clearly.
            "overconfident_prior",
            dt=1.0,
            sigma_a_sq=9.81,
            r_diag=[100.0, 100.0, 400.0],
            p0_diag=[0.01, 0.01, 0.01, 0.01, 0.01, 0.01],
            x0=[0.0] * 6,
            measurements=straight_line(25, 1.0, 300.0, 4242),
        ),
        case(
            # Very loose prior, very tight measurement: the gain is near one and the
            # covariance collapses fast, which is where the Joseph form earns its keep.
            "diffuse_prior_tight_measurement",
            dt=0.5,
            sigma_a_sq=1.0,
            r_diag=[0.01, 0.01, 0.01],
            p0_diag=[1.0e6] * 6,
            x0=[0.0] * 6,
            measurements=straight_line(30, 0.5, 50.0, 99),
        ),
    ]

    doc = {
        "row": "Linear Kalman Filter",
        "oracle": "filterpy.kalman.KalmanFilter",
        "oracle_versions": {"filterpy": filterpy.__version__, "numpy": np.__version__},
        "python": platform.python_version(),
        "state_ordering": "block: [e, n, u, ve, vn, vu]",
        "measurement": "position only: H selects the first three states",
        "generated_by": "testdata/oracles/tools/gen_kalman_fixtures.py",
        "notes": [
            "Both the post-predict and the post-update state and covariance are "
            "recorded for every step, so a failure identifies which half of the cycle "
            "diverged rather than only that the end result differs.",
            "filterpy's KalmanFilter.update uses the Joseph form for P, which is what "
            "gungnir-filters uses too; the agreement below is therefore between two "
            "implementations of the same formula, not between two different ones.",
            "MATLAB's trackingKF is named by the row and was not run: MATLAB is not "
            "installed. See ../README.md.",
        ],
        "cases": cases,
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(doc, indent=1) + "\n", encoding="utf-8")
    total = sum(len(c["steps"]) for c in cases)
    print(f"wrote {OUT} ({len(cases)} cases, {total} filter steps)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
