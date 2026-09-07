"""Oracle fixtures for the two verification-capability-table.md `track-fusion` rows.

  track-fusion | Track-to-track fusion (CI, information-matrix) -> track/fusion.json
  track-fusion | Sensor registration / bias estimation          -> track/registration.json

Run inside .venv-oracles:

    ../../../.venv-oracles/Scripts/python.exe gen_track_fusion_fixtures.py

Section 2 names the fusion oracle as "Stone Soup fuser (partial) + hand-derived CI", and
that is exactly what this produces, in that order of authority:

  * The hand-derived covariance intersection below is the primary oracle. Omega is found
    by scipy's bounded scalar minimiser rather than by the golden-section search the Rust
    side uses, so the two agree on the ANSWER without sharing a method of getting there.
    Two implementations of the same search would agree even if the search were wrong.

  * Stone Soup's ChernoffUpdater is the partial check. It is the library's CI-family
    updater and it takes omega as a fixed parameter rather than optimising it, so it can
    confirm the fusion formula at a GIVEN omega but cannot confirm the choice of omega.
    "Partial" in section 2's Oracle column is that limitation, and it is recorded per
    case in `stonesoup_at_fixed_omega`.

The registration fixture injects a bias by construction, so its truth is exact and not
itself an estimate. Section 2's Dataset column already says "bias injected by
construction"; this is that.
"""

import json
import pathlib

import numpy as np
from scipy.optimize import minimize_scalar

OUT = pathlib.Path(__file__).resolve().parent.parent / "track"
DIM = 6


# Below this relative improvement over omega = 0.5, the determinant criterion is treated
# as not having chosen. See the note in covariance_intersection.
FLAT_OBJECTIVE = 1e-9


def covariance_intersection(x1, p1, x2, p2):
    """The hand-derived CI, with omega minimising the determinant of the result.

    THE DETERMINANT CRITERION DOES NOT ALWAYS DETERMINE OMEGA. When the two inputs have
    equal covariances, the fused information matrix is the same for every omega, so the
    determinant is exactly flat -- while the fused state sweeps the whole line between
    the two estimates. A minimiser left to itself on a flat objective returns wherever
    its internals stop, and two different minimisers return different points. Brent and
    a golden-section search landed 1.2e-6 apart on exactly this case, which is how the
    degeneracy was found.

    The tie is therefore broken by specification: if the minimiser's omega does not beat
    omega = 0.5 by a relative FLAT_OBJECTIVE, use 0.5. Two estimates the criterion cannot
    separate are equally informative. The Rust side implements the same rule from the
    same description; this is not transcribed from it.
    """
    i1 = np.linalg.inv(p1)
    i2 = np.linalg.inv(p2)

    def fused(omega):
        info = omega * i1 + (1.0 - omega) * i2
        p = np.linalg.inv(info)
        x = p @ (omega * i1 @ x1 + (1.0 - omega) * i2 @ x2)
        return x, p

    def objective(omega):
        return np.linalg.det(fused(omega)[1])

    # Bounded Brent, not a grid: the objective is smooth in omega, and a grid would pin
    # omega only as finely as the grid.
    result = minimize_scalar(objective, bounds=(0.0, 1.0), method="bounded",
                             options={"xatol": 1e-12})
    at_half = objective(0.5)
    flat = not np.isfinite(at_half) or (at_half - result.fun) <= FLAT_OBJECTIVE * abs(at_half)
    omega = 0.5 if flat else result.x
    x, p = fused(omega)
    return omega, x, p


def information_matrix(states, covariances):
    total = np.zeros((DIM, DIM))
    weighted = np.zeros(DIM)
    for x, p in zip(states, covariances):
        info = np.linalg.inv(p)
        total += info
        weighted += info @ x
    p = np.linalg.inv(total)
    return p @ weighted, p


def stonesoup_chernoff(x1, p1, x2, p2, omega):
    """Stone Soup's CI-family updater at a fixed omega -- the partial check.

    Returns None if the library cannot be driven for this case, which is recorded rather
    than hidden: a check that silently did not run is worse than one that says so.
    """
    try:
        from stonesoup.models.measurement.linear import LinearGaussian
        from stonesoup.types.detection import GaussianDetection
        from stonesoup.types.prediction import GaussianStatePrediction
        from stonesoup.updater.chernoff import ChernoffUpdater
    except ImportError as exc:  # pragma: no cover - recorded, not raised
        return {"available": False, "reason": str(exc)}

    try:
        model = LinearGaussian(ndim_state=DIM, mapping=tuple(range(DIM)),
                               noise_covar=np.eye(DIM))
        updater = ChernoffUpdater(measurement_model=model, omega=omega)
        prediction = GaussianStatePrediction(x1.reshape(-1, 1), p1)
        detection = GaussianDetection(x2.reshape(-1, 1), p2, measurement_model=model)
        from stonesoup.types.hypothesis import SingleHypothesis

        update = updater.update(SingleHypothesis(prediction, detection))
        return {
            "available": True,
            "omega": omega,
            "x": np.asarray(update.state_vector).reshape(-1).tolist(),
            "p": np.asarray(update.covar).tolist(),
        }
    except Exception as exc:  # pragma: no cover - recorded, not raised
        return {"available": False, "reason": f"{type(exc).__name__}: {exc}"}


def diag(values):
    return np.diag(np.array(values, dtype=float))


def fusion_case(name, x1, p1_diag, x2, p2_diag):
    x1 = np.array(x1, dtype=float)
    x2 = np.array(x2, dtype=float)
    p1 = diag(p1_diag)
    p2 = diag(p2_diag)
    omega, ci_x, ci_p = covariance_intersection(x1, p1, x2, p2)
    im_x, im_p = information_matrix([x1, x2], [p1, p2])
    assert ci_x.shape == (DIM,), ci_x.shape
    assert ci_p.shape == (DIM, DIM), ci_p.shape
    # The property the Rust side also asserts: CI must not be tighter than either input.
    assert np.linalg.det(ci_p) >= min(np.linalg.det(p1), np.linalg.det(p2)) * (1 - 1e-9)
    # The partial cross-check, reduced to a number. Stone Soup is driven at a fixed
    # omega, the hand-derived formula is evaluated at the SAME omega, and the distance
    # between them is recorded. Storing the two answers side by side without comparing
    # them would be a check that never ran.
    fixed = 0.5
    partial = stonesoup_chernoff(x1, p1, x2, p2, fixed)
    if partial.get("available"):
        i1 = np.linalg.inv(p1)
        i2 = np.linalg.inv(p2)
        info = fixed * i1 + (1.0 - fixed) * i2
        p_half = np.linalg.inv(info)
        x_half = p_half @ (fixed * i1 @ x1 + (1.0 - fixed) * i2 @ x2)
        partial["max_difference"] = float(
            max(
                np.abs(np.array(partial["x"]) - x_half).max(),
                np.abs(np.array(partial["p"]) - p_half).max(),
            )
        )

    return {
        "name": name,
        "a": {"x": x1.tolist(), "p_diag": list(p1_diag)},
        "b": {"x": x2.tolist(), "p_diag": list(p2_diag)},
        "covariance_intersection": {"omega": omega, "x": ci_x.tolist(), "p": ci_p.tolist()},
        "information_matrix": {"x": im_x.tolist(), "p": im_p.tolist()},
        "stonesoup_at_fixed_omega": partial,
    }


def write_fusion():
    cases = [
        fusion_case(
            "one_tight_one_loose",
            [100.0, 0.0, 50.0, 10.0, 0.0, 0.0],
            [4.0, 4.0, 9.0, 1.0, 1.0, 1.0],
            [104.0, 2.0, 52.0, 11.0, 0.0, 0.0],
            [400.0, 400.0, 900.0, 100.0, 100.0, 100.0],
        ),
        fusion_case(
            "two_comparable_sensors",
            [1000.0, -500.0, 200.0, -30.0, 15.0, 1.0],
            [16.0, 16.0, 25.0, 4.0, 4.0, 4.0],
            [1004.0, -497.0, 203.0, -29.0, 16.0, 1.2],
            [20.0, 18.0, 30.0, 5.0, 4.5, 4.0],
        ),
        fusion_case(
            "identical_estimates_must_gain_nothing",
            [10.0, 20.0, 30.0, 1.0, 2.0, 3.0],
            [25.0] * 6,
            [10.0, 20.0, 30.0, 1.0, 2.0, 3.0],
            [25.0] * 6,
        ),
        fusion_case(
            "strongly_disagreeing_sensors",
            [0.0, 0.0, 100.0, 0.0, 0.0, 0.0],
            [9.0, 9.0, 9.0, 1.0, 1.0, 1.0],
            [60.0, -40.0, 130.0, 5.0, -5.0, 0.0],
            [9.0, 9.0, 9.0, 1.0, 1.0, 1.0],
        ),
    ]
    payload = {
        "oracle": "hand-derived covariance intersection (scipy bounded Brent for omega)",
        "partial_oracle": "stonesoup.updater.chernoff.ChernoffUpdater at a fixed omega",
        "scipy": "1.18.1",
        "stonesoup": "1.9.1",
        "cases": cases,
    }
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "fusion.json").write_text(json.dumps(payload, indent=1), encoding="utf-8")
    checked = sum(1 for c in cases if c["stonesoup_at_fixed_omega"].get("available"))
    print(f"fusion.json: {len(cases)} cases, {checked} cross-checked against Stone Soup")


def registration_case(name, truth_points, bias, variances, noise_seed=None, noise_m=0.0):
    rng = np.random.default_rng(noise_seed) if noise_seed is not None else None
    a_states, b_states = [], []
    for point in truth_points:
        jitter_a = rng.normal(0.0, noise_m, 3) if rng is not None else np.zeros(3)
        jitter_b = rng.normal(0.0, noise_m, 3) if rng is not None else np.zeros(3)
        a_states.append(list(np.array(point) + jitter_a) + [0.0, 0.0, 0.0])
        b_states.append(list(np.array(point) - np.array(bias) + jitter_b) + [0.0, 0.0, 0.0])

    # The inverse-variance-weighted mean of the pairwise differences, over the position
    # block only. Every pair here has the same covariance, so this reduces to the plain
    # mean -- which is the point: the closed form is checkable by eye for this case.
    differences = np.array(
        [np.array(a[:3]) - np.array(b[:3]) for a, b in zip(a_states, b_states)]
    )
    estimate = differences.mean(axis=0)
    spread = float(np.sqrt(((differences - estimate) ** 2).sum(axis=1).mean()))
    return {
        "name": name,
        "injected_bias": list(bias),
        "variances": list(variances),
        "a": a_states,
        "b": b_states,
        "expected_bias": estimate.tolist(),
        "expected_residual_spread": spread,
    }


def write_registration():
    truth = [
        [0.0, 0.0, 100.0],
        [500.0, 200.0, 150.0],
        [-300.0, 800.0, 90.0],
        [1200.0, -400.0, 220.0],
        [50.0, -900.0, 60.0],
        [-750.0, -150.0, 310.0],
    ]
    cases = [
        registration_case(
            "exact_offset_no_noise",
            truth,
            [12.5, -7.25, 3.0],
            [9.0, 9.0, 16.0, 1.0, 1.0, 1.0],
        ),
        registration_case(
            "offset_under_measurement_noise",
            truth,
            [-4.0, 11.0, -2.5],
            [9.0, 9.0, 16.0, 1.0, 1.0, 1.0],
            noise_seed=20260906,
            noise_m=2.0,
        ),
    ]
    payload = {
        "oracle": "hand-derived least squares; the bias is injected by construction",
        "numpy": "2.5.3",
        "cases": cases,
    }
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "registration.json").write_text(json.dumps(payload, indent=1), encoding="utf-8")
    print("registration.json:", len(cases), "cases")


if __name__ == "__main__":
    write_fusion()
    write_registration()
