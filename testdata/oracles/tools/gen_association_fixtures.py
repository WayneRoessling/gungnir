# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Generate the scipy oracle fixtures for the `gungnir-association` rows.

Rows in `docs/verification-capability-table.md` §1:

  * "Hungarian / Jonker-Volgenant" -- oracle `scipy.optimize.linear_sum_assignment`,
    criterion **exact match on cost; assignment only if not tied**.
  * "Nearest-Neighbor / GNN" -- same oracle through the same solver, criterion
    **exact match** on assignment pairs.
  * "Gating (ellipsoidal / chi-square)" -- oracle a closed-form chi-square,
    criterion **exact match** on gate membership.

# The tie problem, and how this fixture handles it

An assignment problem can have several distinct assignments of exactly equal total
cost. Which one a solver returns is an artifact of its pivoting order. Comparing the
pairing on such a matrix would test scipy's tie-breaking, not our correctness, which is
why the row says "assignment only if not tied".

So each assignment case carries a `unique` flag, decided here rather than assumed. It
is established by brute force over all permutations for the small cases (the only
honest way), and left `false` for the large ones where brute force is infeasible. The
Rust test compares the pairing only where `unique` is true, and compares the cost
everywhere.

# The gating cases

Thresholds come from `scipy.stats.chi2.ppf`, and the fixture records them so the
hard-coded quantile table in `gungnir-association/src/gating.rs` is checked against
scipy rather than trusted. Cases are chosen to straddle the boundary, including a
measurement placed exactly on it, because the criterion is exact match on membership
and the boundary is where an off-by-a-comparison shows up.

Run from `testdata/oracles/` with the venv described in `../README.md`:

    python tools/gen_association_fixtures.py

Writes `association/assignment.json` and `association/gating.json`.
"""

from __future__ import annotations

import itertools
import json
import platform
import sys
from pathlib import Path

import numpy as np
import scipy
from scipy.optimize import linear_sum_assignment
from scipy.stats import chi2

OUT_DIR = Path(__file__).resolve().parent.parent / "association"

# Brute force is n! ; above this the `unique` flag is left false rather than guessed.
BRUTE_FORCE_LIMIT = 7


def optimum_is_unique(cost: np.ndarray, best: float) -> bool | None:
    """True when exactly one assignment attains `best`.

    Returns None when the matrix is too large to decide by brute force, which the
    fixture records as "unknown" so the Rust test can skip the pairing comparison
    rather than assume either way.
    """
    n, m = cost.shape
    if max(n, m) > BRUTE_FORCE_LIMIT:
        return None
    rows, cols = (n, m) if n <= m else (m, n)
    work = cost if n <= m else cost.T
    attaining = 0
    for combo in itertools.permutations(range(cols), rows):
        total = sum(work[i, combo[i]] for i in range(rows))
        if abs(total - best) <= 1e-12:
            attaining += 1
            if attaining > 1:
                return False
    return attaining == 1


def assignment_case(name: str, cost: np.ndarray) -> dict:
    rows, cols = linear_sum_assignment(cost)
    total = float(cost[rows, cols].sum())
    unique = optimum_is_unique(cost, total)
    row_to_col: list[int | None] = [None] * cost.shape[0]
    for r, c in zip(rows, cols):
        row_to_col[int(r)] = int(c)
    return {
        "name": name,
        "rows": int(cost.shape[0]),
        "cols": int(cost.shape[1]),
        "cost": [[float(v) for v in row] for row in cost],
        "total_cost": total,
        "row_to_col": row_to_col,
        # None means "not decided"; the Rust test treats it the same as False.
        "unique": unique,
    }


def assignment_cases() -> list[dict]:
    rng = np.random.default_rng(20260905)
    cases = [
        assignment_case("square_3x3_textbook", np.array([
            [4.0, 1.0, 3.0],
            [2.0, 0.0, 5.0],
            [3.0, 2.0, 2.0],
        ])),
        assignment_case("diagonal_is_optimal", np.where(np.eye(5) > 0, 0.0, 1.0)),
        assignment_case("all_equal_total_tie", np.full((4, 4), 7.0)),
        assignment_case("wide_2x5", np.array([
            [9.0, 1.0, 8.0, 7.0, 6.0],
            [5.0, 4.0, 3.0, 2.0, 1.0],
        ])),
        assignment_case("tall_5x2", np.array([
            [9.0, 5.0],
            [1.0, 4.0],
            [8.0, 3.0],
            [7.0, 2.0],
            [6.0, 1.0],
        ])),
        assignment_case("negative_costs", np.array([
            [-5.0, -1.0, -3.0],
            [-2.0, -8.0, -4.0],
            [-6.0, -2.0, -1.0],
        ])),
        assignment_case("single_cell", np.array([[3.5]])),
        assignment_case("one_row_many_cols", np.array([[5.0, 2.0, 9.0, 1.0, 4.0]])),
        assignment_case("one_col_many_rows", np.array([[5.0], [2.0], [9.0], [1.0]])),
        # Wide dynamic range: the duals must stay in the input's scale.
        assignment_case("wide_dynamic_range", np.array([
            [1e-6, 1e6, 1.0],
            [1e6, 1e-6, 1.0],
            [1.0, 1.0, 1e-9],
        ])),
        # Partial ties: two entries equal, so some pairings tie and some do not.
        assignment_case("partial_tie", np.array([
            [1.0, 1.0, 5.0],
            [2.0, 3.0, 1.0],
            [4.0, 2.0, 2.0],
        ])),
    ]
    # Larger random matrices: cost is still exactly comparable, uniqueness unknown.
    for n, m in ((6, 6), (10, 14), (25, 25), (40, 30)):
        cost = rng.uniform(0.0, 1000.0, size=(n, m)).round(6)
        cases.append(assignment_case(f"random_{n}x{m}", cost))
    return cases


def gating_cases() -> list[dict]:
    rng = np.random.default_rng(9052026)
    cases = []

    def add(name: str, y: np.ndarray, s: np.ndarray, dof: int, confidence: float):
        threshold = float(chi2.ppf(confidence, dof))
        d2 = float(y @ np.linalg.solve(s, y))
        cases.append({
            "name": name,
            "dof": dof,
            "confidence": confidence,
            "threshold": threshold,
            "innovation": [float(v) for v in y],
            "innovation_covariance": [[float(v) for v in row] for row in s],
            "squared_distance": d2,
            "admitted": bool(d2 <= threshold),
        })

    identity = np.eye(3)
    add("on_the_prediction", np.zeros(3), identity, 3, 0.99)
    add("three_four_zero_identity", np.array([3.0, 4.0, 0.0]), identity, 3, 0.99)
    add("far_outside", np.array([100.0, 0.0, 0.0]), identity, 3, 0.95)

    # Exactly on the 95% boundary for 3 dof: scale a unit vector to the threshold.
    t95_3 = float(chi2.ppf(0.95, 3))
    unit = np.array([1.0, 1.0, 1.0]) / np.sqrt(3.0)
    add("exactly_on_the_boundary", unit * np.sqrt(t95_3), identity, 3, 0.95)
    add("a_hair_outside", unit * np.sqrt(t95_3) * (1.0 + 1e-9), identity, 3, 0.95)
    add("a_hair_inside", unit * np.sqrt(t95_3) * (1.0 - 1e-9), identity, 3, 0.95)

    # Anisotropic and correlated covariance: the case an axis-aligned gate gets wrong.
    s_corr = np.array([
        [100.0, 60.0, 0.0],
        [60.0, 64.0, 5.0],
        [0.0, 5.0, 400.0],
    ])
    add("correlated_inside", np.array([5.0, 4.0, 10.0]), s_corr, 3, 0.99)
    add("correlated_outside", np.array([30.0, -25.0, 60.0]), s_corr, 3, 0.99)

    # Ill-conditioned: well observed in one axis, barely in another.
    s_ill = np.diag([1.0, 1.0, 1.0e8])
    add("ill_conditioned_inside", np.array([0.5, 0.5, 5000.0]), s_ill, 3, 0.95)
    add("ill_conditioned_outside", np.array([5.0, 5.0, 1.0e5]), s_ill, 3, 0.95)

    # Two-dimensional and one-dimensional measurements, to exercise the other rows of
    # the quantile table.
    add("two_dof_inside", np.array([1.0, 1.0]), np.eye(2), 2, 0.95)
    add("two_dof_outside", np.array([4.0, 4.0]), np.eye(2), 2, 0.95)
    add("one_dof_inside", np.array([1.5]), np.eye(1), 1, 0.95)
    add("one_dof_outside", np.array([2.5]), np.eye(1), 1, 0.95)

    # A spread of random cases so membership is exercised on both sides in bulk.
    for k in range(20):
        a = rng.normal(0.0, 1.0, (3, 3))
        s = a @ a.T + np.eye(3) * 0.5
        y = rng.normal(0.0, 1.5, 3)
        add(f"random_{k:02d}", y, s, 3, 0.95)

    return cases


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    versions = {"scipy": scipy.__version__, "numpy": np.__version__}

    assignment = {
        "row": "Hungarian / Jonker-Volgenant",
        "also_covers": "Nearest-Neighbor / GNN",
        "oracle": "scipy.optimize.linear_sum_assignment",
        "oracle_versions": versions,
        "python": platform.python_version(),
        "generated_by": "testdata/oracles/tools/gen_association_fixtures.py",
        "notes": [
            "`unique` says whether exactly one assignment attains the optimum, decided "
            "by brute force over all permutations. It is null where the matrix is too "
            "large to decide that way. The criterion is exact match on cost always, "
            "and on the pairing only where `unique` is true.",
            "MATLAB's assignjv and trackerGNN are named by the rows and were not run: "
            "MATLAB is not installed. See ../README.md.",
        ],
        "cases": assignment_cases(),
    }
    (OUT_DIR / "assignment.json").write_text(
        json.dumps(assignment, indent=1) + "\n", encoding="utf-8"
    )

    gating = {
        "row": "Gating (ellipsoidal / chi-square)",
        "oracle": "closed-form chi-square (scipy.stats.chi2 thresholds, numpy solve)",
        "oracle_versions": versions,
        "python": platform.python_version(),
        "generated_by": "testdata/oracles/tools/gen_association_fixtures.py",
        "notes": [
            "The threshold on every case comes from scipy.stats.chi2.ppf, so the "
            "quantile table hard-coded in gungnir-association/src/gating.rs is checked "
            "against scipy rather than trusted.",
            "The squared distance is computed as y @ solve(S, y), not y @ inv(S) @ y, "
            "which is what the Rust side does as well.",
            "Cases straddle the gate boundary deliberately, including one exactly on "
            "it, because the criterion is exact match on membership.",
        ],
        "cases": gating_cases(),
    }
    (OUT_DIR / "gating.json").write_text(
        json.dumps(gating, indent=1) + "\n", encoding="utf-8"
    )

    n_unique = sum(1 for c in assignment["cases"] if c["unique"] is True)
    print(
        f"wrote {OUT_DIR / 'assignment.json'} "
        f"({len(assignment['cases'])} cases, {n_unique} with a unique optimum)"
    )
    print(f"wrote {OUT_DIR / 'gating.json'} ({len(gating['cases'])} cases)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
