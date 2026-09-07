# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Generate the filterpy oracle fixtures for the EKF and UKF rows of
`docs/verification-capability-table.md` §1.

Rows:

* "`filters` | Extended Kalman Filter (EKF)": oracle
  `filterpy.kalman.ExtendedKalmanFilter`, method *same nonlinear scenario + Jacobians,
  compare trajectories*, criterion **relative error < 1e-4**.
* "`filters` | Unscented Kalman Filter (UKF)": oracle
  `filterpy.kalman.UnscentedKalmanFilter`, method *same sigma-point params, compare
  state/covariance*, criterion **relative error < 1e-4; sigma weights sum to 1**.

The nonlinearity is the one a radar actually has: the state is Cartesian
`[e, n, u, ve, vn, vu]` and the measurement is `[range, azimuth, elevation]` from a
sensor at a fixed point. That is the case the rows exist for, rather than a synthetic
nonlinearity chosen to be easy.

**The trajectory keeps azimuth well away from the +/-pi branch cut on purpose.** Angle
wrapping in a residual is a real problem and a real design decision, and it is not the
one these rows are testing; a fixture that straddled the cut would compare two different
conventions for wrapping rather than two implementations of the filter. The geometry
below stays in the first quadrant throughout, and the Rust implementations say the same
thing in their documentation.

Both the post-predict and the post-update state are recorded at every step, so a failure
says which half of the cycle is wrong.

Run from `testdata/oracles/` with the venv described in `../README.md`:

    python tools/gen_nonlinear_fixtures.py

Writes `filters/ekf.json` and `filters/ukf.json`.
"""

from __future__ import annotations

import json
import platform
import sys
from pathlib import Path

import filterpy
import filterpy.common
import numpy as np
from filterpy.kalman import ExtendedKalmanFilter, MerweScaledSigmaPoints, UnscentedKalmanFilter

OUT_DIR = Path(__file__).resolve().parent.parent / "filters"

# The sensor the range/azimuth/elevation is measured from, ENU metres.
SENSOR = np.array([0.0, 0.0, 0.0])


def hx(x):
    """Range, azimuth, elevation of a Cartesian state, from `SENSOR`."""
    d = np.asarray(x).reshape(-1)[:3] - SENSOR
    e, n, u = d[0], d[1], d[2]
    ground = np.hypot(e, n)
    return np.array([
        np.sqrt(e * e + n * n + u * u),
        np.arctan2(e, n),
        np.arctan2(u, ground),
    ])


def h_jacobian(x):
    """d(range, azimuth, elevation)/d(state), analytic."""
    d = np.asarray(x).reshape(-1)[:3] - SENSOR
    e, n, u = d[0], d[1], d[2]
    ground_sq = e * e + n * n
    ground = np.sqrt(ground_sq)
    r_sq = ground_sq + u * u
    r = np.sqrt(r_sq)
    j = np.zeros((3, 6))
    # range
    j[0, 0], j[0, 1], j[0, 2] = e / r, n / r, u / r
    # azimuth = atan2(e, n)
    j[1, 0], j[1, 1] = n / ground_sq, -e / ground_sq
    # elevation = atan2(u, ground)
    j[2, 0] = -e * u / (r_sq * ground)
    j[2, 1] = -n * u / (r_sq * ground)
    j[2, 2] = ground / r_sq
    return j


def truth(step: int, dt: float) -> np.ndarray:
    """A target crossing the first quadrant, climbing gently."""
    t = step * dt
    return np.array([
        8_000.0 + 120.0 * t,
        12_000.0 + 60.0 * t,
        3_000.0 + 5.0 * t,
        120.0,
        60.0,
        5.0,
    ])


def measurements(steps: int, dt: float, seed: int) -> list[np.ndarray]:
    """Noisy range/azimuth/elevation of the truth, deterministic in `seed`."""
    rng = np.random.default_rng(seed)
    sigma = np.array([25.0, np.deg2rad(0.2), np.deg2rad(0.3)])
    out = []
    for step in range(1, steps + 1):
        z = hx(truth(step, dt)) + rng.normal(0.0, sigma, size=3)
        out.append(z)
    return out


def f_matrix(dt: float) -> np.ndarray:
    return np.asarray(
        filterpy.common.kinematic_kf(dim=3, order=1, dt=dt, order_by_dim=False).F,
        dtype=float,
    )


def q_matrix(dt: float, sigma_a_sq: float) -> np.ndarray:
    q = filterpy.common.Q_continuous_white_noise(
        dim=2, dt=dt, spectral_density=sigma_a_sq, block_size=3, order_by_dim=False
    )
    return np.asarray(q, dtype=float)


def snapshot(x, p) -> dict:
    """One recorded state and covariance, with the shape actually checked.

    numpy broadcasts a (3,1) against a (3,) into a (3,3) without complaint, and an
    EKF whose measurement function and measurement disagree about shape produces a
    state of the wrong size rather than an error. That happened while this generator
    was being written, and the fixture it wrote looked like a filter divergence. The
    assertions below make the shape a condition of writing the file.
    """
    x = np.asarray(x).reshape(-1)
    p = np.asarray(p)
    assert x.shape == (6,), f"state has shape {x.shape}, not (6,): a broadcast went wrong"
    assert p.shape == (6, 6), f"covariance has shape {p.shape}, not (6, 6)"
    return {"x": x.tolist(), "p": p.tolist()}


def ekf_case(name: str, dt: float, sigma_a_sq: float, steps: int, seed: int) -> dict:
    r = np.diag([25.0**2, np.deg2rad(0.2) ** 2, np.deg2rad(0.3) ** 2])
    p0 = np.diag([500.0**2, 500.0**2, 500.0**2, 50.0**2, 50.0**2, 50.0**2])
    x0 = truth(0, dt) + np.array([200.0, -150.0, 80.0, 10.0, -8.0, 1.0])

    ekf = ExtendedKalmanFilter(dim_x=6, dim_z=3)
    # A one-dimensional state on purpose. filterpy reshapes `z` to match `x.ndim`, and
    # `hx` returns a one-dimensional measurement; a column state would make `z - hx(x)`
    # broadcast a (3,1) against a (3,) into a (3,3) and silently corrupt the update.
    ekf.x = x0.copy()
    ekf.P = p0.copy()
    ekf.R = r.copy()
    ekf.F = f_matrix(dt)
    ekf.Q = q_matrix(dt, sigma_a_sq)

    out_steps = []
    for z in measurements(steps, dt, seed):
        ekf.predict()
        after_predict = snapshot(ekf.x, ekf.P)
        ekf.update(z, HJacobian=h_jacobian, Hx=hx)
        out_steps.append(
            {
                "z": z.tolist(),
                "after_predict": after_predict,
                "after_update": snapshot(ekf.x, ekf.P),
            }
        )
    return {
        "name": name,
        "dt": dt,
        "sigma_a_sq": sigma_a_sq,
        "r_diag": np.diag(r).tolist(),
        "p0_diag": np.diag(p0).tolist(),
        "x0": x0.tolist(),
        "steps": out_steps,
    }


def ukf_case(
    name: str, dt: float, sigma_a_sq: float, steps: int, seed: int,
    alpha: float, beta: float, kappa: float,
) -> dict:
    r = np.diag([25.0**2, np.deg2rad(0.2) ** 2, np.deg2rad(0.3) ** 2])
    p0 = np.diag([500.0**2, 500.0**2, 500.0**2, 50.0**2, 50.0**2, 50.0**2])
    x0 = truth(0, dt) + np.array([200.0, -150.0, 80.0, 10.0, -8.0, 1.0])
    f = f_matrix(dt)

    def fx(x, _dt):
        return f @ x

    points = MerweScaledSigmaPoints(n=6, alpha=alpha, beta=beta, kappa=kappa)
    ukf = UnscentedKalmanFilter(dim_x=6, dim_z=3, dt=dt, hx=hx, fx=fx, points=points)
    ukf.x = x0.copy()
    ukf.P = p0.copy()
    ukf.R = r.copy()
    ukf.Q = q_matrix(dt, sigma_a_sq)

    out_steps = []
    for z in measurements(steps, dt, seed):
        ukf.predict()
        after_predict = snapshot(ukf.x, ukf.P)
        ukf.update(z)
        out_steps.append(
            {
                "z": z.tolist(),
                "after_predict": after_predict,
                "after_update": snapshot(ukf.x, ukf.P),
            }
        )
    return {
        "name": name,
        "dt": dt,
        "sigma_a_sq": sigma_a_sq,
        "alpha": alpha,
        "beta": beta,
        "kappa": kappa,
        "weights_mean": points.Wm.tolist(),
        "weights_covariance": points.Wc.tolist(),
        "r_diag": np.diag(r).tolist(),
        "p0_diag": np.diag(p0).tolist(),
        "x0": x0.tolist(),
        "steps": out_steps,
    }


def rts_case(name: str, dt: float, sigma_a_sq: float, steps: int, seed: int) -> dict:
    """A linear forward pass, then `filterpy.kalman.rts_smoother` over it.

    Row: "`filters` | RTS smoother / fixed-lag smoothing", oracle
    `filterpy.kalman.rts_smoother`, criterion **relative error < 1e-6**.

    The forward pass is deliberately the *linear* filter on a position measurement: the
    smoother's recursion is linear whatever produced the trajectory, and using the
    already-gated linear filter keeps this fixture about the backward pass alone.
    """
    from filterpy.kalman import KalmanFilter, rts_smoother

    h = np.zeros((3, 6))
    h[0, 0] = h[1, 1] = h[2, 2] = 1.0
    r = np.diag([100.0, 100.0, 225.0])
    p0 = np.diag([500.0**2] * 3 + [50.0**2] * 3)
    x0 = truth(0, dt) + np.array([200.0, -150.0, 80.0, 10.0, -8.0, 1.0])

    kf = KalmanFilter(dim_x=6, dim_z=3)
    kf.x = x0.copy()
    kf.P = p0.copy()
    kf.H = h
    kf.R = r
    kf.F = f_matrix(dt)
    kf.Q = q_matrix(dt, sigma_a_sq)

    rng = np.random.default_rng(seed)
    zs, xs, ps = [], [], []
    for step in range(1, steps + 1):
        z = truth(step, dt)[:3] + rng.normal(0.0, np.sqrt(np.diag(r)), size=3)
        kf.predict()
        kf.update(z)
        zs.append(z.tolist())
        xs.append(np.asarray(kf.x).reshape(-1).copy())
        ps.append(np.asarray(kf.P).copy())

    xs_a = np.array(xs)
    ps_a = np.array(ps)
    # `rts_smoother` indexes Fs and Qs per step, so they are lists of the same
    # length as the trajectory rather than single matrices.
    n = len(xs)
    smoothed_x, smoothed_p, _, _ = rts_smoother(xs_a, ps_a, [kf.F] * n, [kf.Q] * n)
    return {
        "name": name,
        "dt": dt,
        "sigma_a_sq": sigma_a_sq,
        "r_diag": np.diag(r).tolist(),
        "p0_diag": np.diag(p0).tolist(),
        "x0": x0.tolist(),
        "measurements": zs,
        "forward": [snapshot(x, p) for x, p in zip(xs, ps)],
        "smoothed": [snapshot(x, p) for x, p in zip(smoothed_x, smoothed_p)],
    }


def main() -> None:
    header = {
        "filterpy": filterpy.__version__,
        "numpy": np.__version__,
        "python": sys.version.split()[0],
        "platform": platform.platform(),
        "measurement": "range, azimuth, elevation from a sensor at the ENU origin",
        "note": (
            "the trajectory stays in the first quadrant, so azimuth never crosses the "
            "+/-pi branch cut and no residual wrapping convention is being compared"
        ),
    }
    OUT_DIR.mkdir(parents=True, exist_ok=True)

    ekf = dict(header)
    ekf["oracle"] = "filterpy.kalman.ExtendedKalmanFilter"
    ekf["row"] = "filters / Extended Kalman Filter (EKF)"
    ekf["criterion"] = "relative error < 1e-4"
    ekf["cases"] = [
        ekf_case("radar-1hz", 1.0, 4.0, 25, 20260906),
        ekf_case("radar-2s-coarse", 2.0, 9.0, 15, 7),
    ]
    (OUT_DIR / "ekf.json").write_text(json.dumps(ekf, indent=1) + "\n", encoding="utf-8")

    ukf = dict(header)
    ukf["oracle"] = "filterpy.kalman.UnscentedKalmanFilter with MerweScaledSigmaPoints"
    ukf["row"] = "filters / Unscented Kalman Filter (UKF)"
    ukf["criterion"] = "relative error < 1e-4; sigma weights sum to 1"
    ukf["cases"] = [
        ukf_case("radar-1hz", 1.0, 4.0, 25, 20260906, alpha=0.1, beta=2.0, kappa=-3.0),
        ukf_case("radar-2s-coarse", 2.0, 9.0, 15, 7, alpha=0.5, beta=2.0, kappa=0.0),
    ]
    (OUT_DIR / "ukf.json").write_text(json.dumps(ukf, indent=1) + "\n", encoding="utf-8")

    rts = dict(header)
    rts["oracle"] = "filterpy.kalman.rts_smoother"
    rts["row"] = "filters / RTS smoother / fixed-lag smoothing"
    rts["criterion"] = "relative error < 1e-6"
    rts["measurement"] = "position, measured directly (the smoother's recursion is linear)"
    rts["cases"] = [
        rts_case("position-1hz", 1.0, 4.0, 20, 20260906),
        rts_case("position-2s", 2.0, 9.0, 12, 11),
    ]
    (OUT_DIR / "rts.json").write_text(json.dumps(rts, indent=1) + "\n", encoding="utf-8")
    print("wrote ekf.json, ukf.json and rts.json in", OUT_DIR)


if __name__ == "__main__":
    main()
