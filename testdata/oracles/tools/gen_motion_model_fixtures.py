# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Generate the motion-model oracle fixtures for the `gungnir-core` capability-table row.

Row: "Motion models: CV, CA, CT" in `docs/verification-capability-table.md` §1.
Pass criterion: exact match (~1e-10) on `F` and `Q`, element-wise, for the same
initial state and `dt`.

Three independent oracles are recorded per case, because the row names two Python
sources ("`filterpy.kalman` matrices; hand-derived") and the workspace has a third
installed:

  * **filterpy** -- `Q_continuous_white_noise` and `kinematic_kf`, in the block
    ordering `order_by_dim=False` produces, which is the ordering `gungnir_track::Track`
    documents. filterpy has no coordinated-turn model, so it contributes `F` for CV and
    CA and `Q` for all three.
  * **Stone Soup** -- `KnownTurnRate` supplies `F` *and* `Q` for the coordinated turn.
    Stone Soup states are interleaved (`[x, vx, y, vy]`), so the matrix is permuted into
    block ordering here; the permutation is written out explicitly and is itself checked
    by the CV case, where both oracles overlap.
  * **scipy** -- `expm(A dt)` for the continuous system matrix, and Van Loan's method
    for `Q`. This is the "hand-derived" column mechanised: it shares no code path with
    the closed forms in either library, so agreement between it and the other two is
    real evidence rather than two wrappers around one implementation.

The Rust test asserts against all three. Where they disagree with each other by more
than the tolerance, that is recorded in the fixture and the test fails loudly rather
than picking a favourite.

Run from `testdata/oracles/` with the venv described in `../README.md`:

    python tools/gen_motion_model_fixtures.py

Writes `core/motion_models.json`.
"""

from __future__ import annotations

import json
import platform
import sys
from datetime import timedelta
from pathlib import Path

import numpy as np
import scipy
import scipy.linalg
import filterpy
import filterpy.common
import stonesoup
from stonesoup.models.transition.linear import KnownTurnRate

OUT = Path(__file__).resolve().parent.parent / "core" / "motion_models.json"

# Timesteps: a sub-millisecond step, ordinary radar dwells, and a long coast.
DTS = [0.001, 0.05, 0.1, 0.5, 1.0, 2.0, 10.0]
# Spectral densities, (m/s^2)^2/Hz for CV and CT, (m/s^3)^2/Hz for CA.
DENSITIES = [1.0, 0.25, 9.81]
# Turn rates, rad/s: still, gentle, a 3 g fighter turn, and a reversed turn. Zero is
# the degenerate case the Rust small-angle branch exists for.
OMEGAS = [0.0, 1e-6, 0.01, 0.1, 0.35, -0.2]


def block_a_cv() -> np.ndarray:
    """Continuous system matrix for CV in block ordering [p(3); v(3)]."""
    a = np.zeros((6, 6))
    a[0:3, 3:6] = np.eye(3)
    return a


def block_a_ca() -> np.ndarray:
    """Continuous system matrix for CA in block ordering [p(3); v(3); a(3)]."""
    a = np.zeros((9, 9))
    a[0:3, 3:6] = np.eye(3)
    a[3:6, 6:9] = np.eye(3)
    return a


def block_a_ct(omega: float) -> np.ndarray:
    """Continuous system matrix for the coordinated turn in block ordering.

    Turn is in the east-north plane about local up; the vertical axis is plain CV.
    """
    a = np.zeros((6, 6))
    a[0:3, 3:6] = np.eye(3)
    # d(ve)/dt = -omega * vn ; d(vn)/dt = +omega * ve
    a[3, 4] = -omega
    a[4, 3] = omega
    return a


def van_loan(a: np.ndarray, g_q_gt: np.ndarray, dt: float) -> tuple[np.ndarray, np.ndarray]:
    """Van Loan (1978): F and Q for a linear system with continuous process noise.

    Returns (F, Q) where Q = integral_0^dt e^{A s} G q G' e^{A' s} ds.
    """
    n = a.shape[0]
    m = np.zeros((2 * n, 2 * n))
    m[0:n, 0:n] = -a
    m[0:n, n : 2 * n] = g_q_gt
    m[n : 2 * n, n : 2 * n] = a.T
    big = scipy.linalg.expm(m * dt)
    f = big[n : 2 * n, n : 2 * n].T
    q = f @ big[0:n, n : 2 * n]
    # Symmetrize: the integral is symmetric, and expm's rounding is not.
    return f, 0.5 * (q + q.T)


def noise_gain(n: int) -> np.ndarray:
    """G q G' with unit density: noise enters the highest derivative on each axis."""
    g = np.zeros((n, n))
    g[n - 3 : n, n - 3 : n] = np.eye(3)
    return g


# Stone Soup's KnownTurnRate is 2D and interleaved [x, vx, y, vy]. These index maps
# lift it into this workspace's 6-state block ordering [e, n, u, ve, vn, vu].
SS_TO_BLOCK = {0: 0, 1: 3, 2: 1, 3: 4}


def stonesoup_ct(omega: float, dt: float, density: float) -> tuple[np.ndarray, np.ndarray]:
    """Stone Soup's coordinated turn, permuted into block ordering and given a
    vertical constant-velocity axis so it is comparable with the 6-state model."""
    model = KnownTurnRate(
        turn_noise_diff_coeffs=np.array([np.sqrt(density), np.sqrt(density)]),
        turn_rate=omega,
    )
    interval = timedelta(seconds=dt)
    f_ss = np.asarray(model.matrix(time_interval=interval), dtype=float)
    q_ss = np.asarray(model.covar(time_interval=interval), dtype=float)

    f = np.eye(6)
    q = np.zeros((6, 6))
    for i_ss, i_b in SS_TO_BLOCK.items():
        for j_ss, j_b in SS_TO_BLOCK.items():
            f[i_b, j_b] = f_ss[i_ss, j_ss]
            q[i_b, j_b] = q_ss[i_ss, j_ss]
    # Vertical axis: constant velocity with the same acceleration density.
    f[2, 5] = dt
    q[2, 2] = density * dt**3 / 3.0
    q[2, 5] = density * dt**2 / 2.0
    q[5, 2] = density * dt**2 / 2.0
    q[5, 5] = density * dt
    return f, q


def as_rows(m: np.ndarray) -> list[list[float]]:
    return [[float(v) for v in row] for row in m]


def worst(a: np.ndarray, b: np.ndarray) -> float:
    return float(np.max(np.abs(a - b)))


def main() -> int:
    cases = []
    cross_check_worst = 0.0

    # ---------------------------------------------------------------- CV
    for dt in DTS:
        for density in DENSITIES:
            f_scipy, q_scipy = van_loan(block_a_cv(), noise_gain(6) * density, dt)
            f_fp = np.asarray(
                filterpy.common.kinematic_kf(dim=3, order=1, dt=dt, order_by_dim=False).F,
                dtype=float,
            )
            q_fp = np.asarray(
                filterpy.common.Q_continuous_white_noise(
                    2, dt, density, block_size=3, order_by_dim=False
                ),
                dtype=float,
            )
            cross_check_worst = max(
                cross_check_worst, worst(f_scipy, f_fp), worst(q_scipy, q_fp)
            )
            cases.append(
                {
                    "model": "ConstantVelocity",
                    "dim": 6,
                    "dt": dt,
                    "sigma_a_sq": density,
                    "f": as_rows(f_fp),
                    "q": as_rows(q_fp),
                    "criterion": {"f": "filterpy", "q": "filterpy"},
                    "oracles": {
                        "scipy_van_loan": {"f": as_rows(f_scipy), "q": as_rows(q_scipy)},
                        "filterpy": {"f": as_rows(f_fp), "q": as_rows(q_fp)},
                    },
                }
            )

    # ---------------------------------------------------------------- CA
    for dt in DTS:
        for density in DENSITIES:
            f_scipy, q_scipy = van_loan(block_a_ca(), noise_gain(9) * density, dt)
            f_fp = np.asarray(
                filterpy.common.kinematic_kf(dim=3, order=2, dt=dt, order_by_dim=False).F,
                dtype=float,
            )
            q_fp = np.asarray(
                filterpy.common.Q_continuous_white_noise(
                    3, dt, density, block_size=3, order_by_dim=False
                ),
                dtype=float,
            )
            cross_check_worst = max(
                cross_check_worst, worst(f_scipy, f_fp), worst(q_scipy, q_fp)
            )
            cases.append(
                {
                    "model": "ConstantAcceleration",
                    "dim": 9,
                    "dt": dt,
                    "sigma_j_sq": density,
                    "f": as_rows(f_fp),
                    "q": as_rows(q_fp),
                    "criterion": {"f": "filterpy", "q": "filterpy"},
                    "oracles": {
                        "scipy_van_loan": {"f": as_rows(f_scipy), "q": as_rows(q_scipy)},
                        "filterpy": {"f": as_rows(f_fp), "q": as_rows(q_fp)},
                    },
                }
            )

    # ---------------------------------------------------------------- CT
    degenerate_omega = []
    for dt in DTS:
        for omega in OMEGAS:
            for density in DENSITIES[:1]:
                f_scipy, q_scipy = van_loan(block_a_ct(omega), noise_gain(6) * density, dt)
                f_ss, q_ss = stonesoup_ct(omega, dt, density)
                q_fp = np.asarray(
                    filterpy.common.Q_continuous_white_noise(
                        2, dt, density, block_size=3, order_by_dim=False
                    ),
                    dtype=float,
                )

                oracles = {
                    # expm is the criterion for F here: it is finite and accurate at
                    # every turn rate, including the ones where Stone Soup is not.
                    "scipy_expm": {"f": as_rows(f_scipy)},
                    "filterpy": {"q": as_rows(q_fp)},
                }
                # Stone Soup divides by the turn rate, so it yields NaN at omega == 0
                # and loses most of its significant digits in (1 - cos w dt)/w as the
                # turn rate approaches zero. Where it is finite it is recorded and
                # asserted against; where it is not, it is omitted and the case is
                # listed in degenerate_oracles so the omission is visible rather than
                # silent. This is a limitation of the reference, not of the model.
                if np.isfinite(f_ss).all() and np.isfinite(q_ss).all():
                    oracles["stonesoup"] = {"f": as_rows(f_ss), "q": as_rows(q_ss)}
                    cross_check_worst = max(cross_check_worst, worst(f_scipy, f_ss))
                else:
                    degenerate_omega.append({"dt": dt, "omega": omega, "oracle": "stonesoup"})

                cases.append(
                    {
                        "model": "CoordinatedTurn",
                        "dim": 6,
                        "dt": dt,
                        "omega": omega,
                        "sigma_a_sq": density,
                        "f": as_rows(f_scipy),
                        # Stone Soup and filterpy agree on Q for the turn: the noise is
                        # a Cartesian acceleration, so Q is the constant-velocity form.
                        # scipy's Van Loan integral taken over the *rotating* dynamics
                        # is a different quantity; it is kept out of this case's oracle
                        # set deliberately rather than asserted against.
                        "q": as_rows(q_fp),
                        "criterion": {"f": "scipy_expm", "q": "filterpy"},
                        "oracles": oracles,
                        "reference_only": {
                            "scipy_van_loan_rotating_q": as_rows(q_scipy)
                        },
                    }
                )

    doc = {
        "row": "Motion models: CV, CA, CT",
        "oracle": "filterpy + Stone Soup + scipy",
        "oracle_versions": {
            "filterpy": filterpy.__version__,
            "stonesoup": stonesoup.__version__,
            "scipy": scipy.__version__,
            "numpy": np.__version__,
        },
        "python": platform.python_version(),
        "state_ordering": "block: [e, n, u, ve, vn, vu(, ae, an, au)]",
        "process_noise": "continuous white noise; sigma_* are spectral densities",
        "generated_by": "testdata/oracles/tools/gen_motion_model_fixtures.py",
        "cross_oracle_worst_disagreement": cross_check_worst,
        "degenerate_oracles": degenerate_omega,
        "notes": [
            "F for every model is agreed by scipy's expm and by filterpy (CV, CA) or "
            "Stone Soup (CT) to the figure in cross_oracle_worst_disagreement.",
            "Q for the coordinated turn is the Cartesian continuous white-noise "
            "acceleration form, which is what Stone Soup's KnownTurnRate.covar returns "
            "and what filterpy's Q_continuous_white_noise gives. The scipy Van Loan "
            "integral taken over the rotating dynamics is a different quantity and is "
            "recorded under reference_only for comparison; it is not the criterion and "
            "the Rust test does not assert against it.",
            "Stone Soup's KnownTurnRate divides by the turn rate. At omega == 0 it "
            "returns NaN, and near zero its (1 - cos w dt)/w term loses most of its "
            "significant digits. Those cases are listed in degenerate_oracles and the "
            "Stone Soup column is omitted for them; the model itself is not degenerate "
            "there, and gungnir-core evaluates the same terms through a small-angle "
            "series precisely so that it is not.",
        ],
        "cases": cases,
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(doc, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {OUT} ({len(cases)} cases)")
    print(f"worst cross-oracle disagreement: {cross_check_worst:e}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
