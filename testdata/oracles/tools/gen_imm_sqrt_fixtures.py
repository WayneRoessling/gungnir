"""Oracle fixtures for three verification-capability-table.md rows.

  filters | Interacting Multiple Model (IMM)      -> filters/imm.json
  filters | Square-root / UDU-factorized KF & EKF -> filters/sqrt.json
  filters | Particle Filter                       -> filters/particle.json

Run inside .venv-oracles:

    ../../../.venv-oracles/Scripts/python.exe gen_imm_sqrt_fixtures.py

THE IMM ORACLE IS NOT THE ONE SECTION 2 PLANNED. Section 2 names Stone Soup's IMM.
Stone Soup 1.9.1, the version this workspace pins, has no IMM at all -- there is no such
symbol anywhere in the installed package. The substitute is filterpy.kalman.IMMEstimator,
an independent implementation of the same Blom/Bar-Shalom recursion and the oracle four
other rows in this table already use. The criterion is unchanged: state within 1e-4,
mode probabilities within 1e-3. The substitution is recorded here, in the row's own
Method column, and in the Rust module, rather than made quietly.

THE MOTION MODELS MUST MATCH gungnir-core EXACTLY or the comparison is between two
different filters. gungnir_core::ConstantVelocity uses the CONTINUOUS white-noise
acceleration form, per-axis [[dt^3/3, dt^2/2], [dt^2/2, dt]] -- note dt^3/3, not the
discrete form's dt^4/4 -- expanded over three axes in the block order
[x, y, z, vx, vy, vz]. CoordinatedTurn shares that Q and rotates the horizontal
velocity. Both are transcribed below from gungnir-core/src/lib.rs.

Every recorded state is shape-asserted before it is written. That check exists because
the first EKF fixture this repository produced was silently wrong: filterpy reshapes the
measurement to the state's dimensionality, a column state against a one-dimensional
measurement function broadcast (3,1) against (3,) into (3,3), and the recorded state had
eighteen entries instead of six. It read exactly like a diverging filter.
"""

import json
import pathlib

import numpy as np
from filterpy.kalman import IMMEstimator, KalmanFilter
from filterpy.monte_carlo import systematic_resample

OUT = pathlib.Path(__file__).resolve().parent.parent / "filters"
STATE_DIM = 6
MEAS_DIM = 3


# --------------------------------------------------------------------------- models
def cv_f(dt):
    """gungnir_core::ConstantVelocity::f."""
    f = np.eye(STATE_DIM)
    for axis in range(3):
        f[axis, 3 + axis] = dt
    return f


def cv_q(dt, sigma_a_sq):
    """gungnir_core::ConstantVelocity::q -- the CONTINUOUS form. See the module note."""
    per_axis = np.array([[dt**3 / 3.0, dt**2 / 2.0], [dt**2 / 2.0, dt]])
    q = np.zeros((STATE_DIM, STATE_DIM))
    for i in range(2):
        for j in range(2):
            for axis in range(3):
                q[3 * i + axis, 3 * j + axis] = per_axis[i, j]
    return q * sigma_a_sq


def ct_f(dt, omega):
    """gungnir_core::CoordinatedTurn::f, including its small-angle handling."""
    theta = omega * dt

    def sinc(x):
        # gungnir-core switches to the truncated series below |x| = 1e-3.
        if abs(x) < 1e-3:
            x2 = x * x
            return 1.0 - x2 / 6.0 + x2 * x2 / 120.0
        return np.sin(x) / x

    def vers_over_x(x):
        h = x / 2.0
        return h * sinc(h) * sinc(h)

    s = dt * sinc(theta)
    c = dt * vers_over_x(theta)
    f = np.eye(STATE_DIM)
    f[0, 3] = s
    f[0, 4] = -c
    f[1, 3] = c
    f[1, 4] = s
    f[2, 5] = dt
    f[3, 3] = np.cos(theta)
    f[3, 4] = -np.sin(theta)
    f[4, 3] = np.sin(theta)
    f[4, 4] = np.cos(theta)
    return f


def position_h():
    h = np.zeros((MEAS_DIM, STATE_DIM))
    for axis in range(3):
        h[axis, axis] = 1.0
    return h


def snapshot(x, p):
    x = np.asarray(x).reshape(-1)
    p = np.asarray(p)
    assert x.shape == (STATE_DIM,), f"state has shape {x.shape}, expected ({STATE_DIM},)"
    assert p.shape == (STATE_DIM, STATE_DIM), f"covariance has shape {p.shape}"
    return {"x": x.tolist(), "p": p.tolist()}


# ------------------------------------------------------------------------------ IMM
def build_kf(dt, f, q, r, x0, p0):
    kf = KalmanFilter(dim_x=STATE_DIM, dim_z=MEAS_DIM)
    kf.x = np.array(x0, dtype=float).reshape(-1)
    kf.P = np.array(p0, dtype=float)
    kf.F = f
    kf.Q = q
    kf.H = position_h()
    kf.R = r
    del dt
    return kf


def imm_case(name, dt, omega, sigma_a_sq, r_diag, p0_diag, x0, measurements, mu, transition):
    r = np.diag(r_diag)
    p0 = np.diag(p0_diag)
    q = cv_q(dt, sigma_a_sq)
    filters = [
        build_kf(dt, cv_f(dt), q, r, x0, p0),
        build_kf(dt, ct_f(dt, omega), q, r, x0, p0),
    ]
    imm = IMMEstimator(filters, np.array(mu, dtype=float), np.array(transition, dtype=float))

    steps = []
    for z in measurements:
        imm.predict()
        after_predict = snapshot(imm.x, imm.P)
        predict_mu = np.asarray(imm.mu).reshape(-1).tolist()
        imm.update(np.array(z, dtype=float))
        steps.append(
            {
                "z": list(z),
                "after_predict": after_predict,
                "mode_probabilities_after_predict": predict_mu,
                "after_update": snapshot(imm.x, imm.P),
                "mode_probabilities_after_update": np.asarray(imm.mu).reshape(-1).tolist(),
            }
        )
    return {
        "name": name,
        "dt": dt,
        "omega": omega,
        "sigma_a_sq": sigma_a_sq,
        "r_diag": list(r_diag),
        "p0_diag": list(p0_diag),
        "x0": list(x0),
        "initial_mode_probabilities": list(mu),
        "transition": [list(row) for row in transition],
        "steps": steps,
    }


def straight_track(dt, count, speed):
    return [(speed * dt * (k + 1), 0.0, 100.0) for k in range(count)]


def turning_track(dt, count, speed, omega):
    radius = speed / omega
    out = []
    for k in range(count):
        angle = omega * dt * (k + 1)
        out.append((radius * np.sin(angle), radius * (1.0 - np.cos(angle)), 100.0))
    return out


def write_imm():
    dt = 1.0
    omega = 0.12
    transition = [[0.95, 0.05], [0.05, 0.95]]
    cases = [
        imm_case(
            "straight_run_should_favour_constant_velocity",
            dt,
            omega,
            2.0,
            [25.0, 25.0, 100.0],
            [400.0, 400.0, 900.0, 100.0, 100.0, 100.0],
            [0.0, 0.0, 100.0, 80.0, 0.0, 0.0],
            straight_track(dt, 40, 80.0),
            [0.5, 0.5],
            transition,
        ),
        imm_case(
            "coordinated_turn_should_move_probability_to_the_turn_mode",
            dt,
            omega,
            2.0,
            [25.0, 25.0, 100.0],
            [400.0, 400.0, 900.0, 100.0, 100.0, 100.0],
            [0.0, 0.0, 100.0, 80.0, 0.0, 0.0],
            turning_track(dt, 40, 80.0, omega),
            [0.5, 0.5],
            transition,
        ),
        imm_case(
            "a_switch_mid_track_is_the_case_an_imm_exists_for",
            dt,
            omega,
            2.0,
            [16.0, 16.0, 64.0],
            [400.0, 400.0, 900.0, 100.0, 100.0, 100.0],
            [0.0, 0.0, 100.0, 80.0, 0.0, 0.0],
            straight_track(dt, 20, 80.0)
            + [
                (
                    80.0 * dt * 20 + p[0],
                    p[1],
                    p[2],
                )
                for p in turning_track(dt, 20, 80.0, omega)
            ],
            [0.9, 0.1],
            transition,
        ),
    ]
    payload = {
        "oracle": "filterpy.kalman.IMMEstimator",
        "filterpy": "1.4.5",
        "oracle_substitution": (
            "verification-capability-table.md section 2 planned this row against Stone "
            "Soup's IMM. Stone Soup 1.9.1, the pinned version, has no IMM. filterpy's "
            "IMMEstimator is the substitute; the criterion is unchanged."
        ),
        "cases": cases,
    }
    (OUT / "imm.json").write_text(json.dumps(payload, indent=1), encoding="utf-8")
    print("imm.json:", len(cases), "cases")


# ---------------------------------------------------------------------- square root
def write_sqrt():
    """The square-root row is gated against the standard-form filter, not against
    filterpy's SquareRootKalmanFilter.

    filterpy's square-root filter carries only the covariance factor and exposes no
    process-noise factorisation, so feeding it this workspace's Q means reconstructing P
    and refactoring every step -- which measures filterpy's reconstruction rather than
    the array recursion under test. Section 2 already states the method as "compare vs.
    standard-form filter", and that is what this fixture provides: the reference
    trajectory from filterpy's ordinary KalmanFilter, which the linear row is already
    gated against at 1e-6. The Rust square-root filter must match it to the row's 1e-6.
    """
    dt = 0.5
    sigma_a_sq = 4.0
    r_diag = [25.0, 25.0, 100.0]
    p0_diag = [400.0, 400.0, 900.0, 40.0, 40.0, 40.0]
    x0 = [10.0, -5.0, 100.0, 3.0, 1.0, 0.0]
    cases = []
    for name, scale in (
        ("well_scaled", 1.0),
        # The case the square-root form exists for: measurement noise spanning six
        # orders of magnitude, where the standard form's condition number is squared.
        ("badly_scaled_measurement_noise", 1e-6),
    ):
        r = np.diag([r_diag[0], r_diag[1] * scale, r_diag[2] * scale])
        kf = build_kf(dt, cv_f(dt), cv_q(dt, sigma_a_sq), r, x0, np.diag(p0_diag))
        steps = []
        for k in range(200):
            kf.predict()
            after_predict = snapshot(kf.x, kf.P)
            t = dt * (k + 1)
            z = np.array([10.0 + 3.0 * t, -5.0 + t, 100.0])
            kf.update(z)
            steps.append(
                {
                    "z": z.tolist(),
                    "after_predict": after_predict,
                    "after_update": snapshot(kf.x, kf.P),
                }
            )
        cases.append(
            {
                "name": name,
                "dt": dt,
                "sigma_a_sq": sigma_a_sq,
                "r_diag": np.diag(r).tolist(),
                "p0_diag": p0_diag,
                "x0": x0,
                "steps": steps,
            }
        )
    payload = {
        "oracle": "filterpy.kalman.KalmanFilter (standard form), per the row's own method",
        "filterpy": "1.4.5",
        "method_note": (
            "Section 2's method for this row is 'compare vs. standard-form filter'. "
            "filterpy's SquareRootKalmanFilter exposes no process-noise factorisation, "
            "so gating against it would measure its reconstruction of P rather than the "
            "array recursion. The PSD half of the criterion is a soak test in Rust."
        ),
        "cases": cases,
    }
    (OUT / "sqrt.json").write_text(json.dumps(payload, indent=1), encoding="utf-8")
    print("sqrt.json:", len(cases), "cases")


# ------------------------------------------------------------------- particle filter
def reference_sir(measurements, count, dt, sigma_a_sq, r_diag, p0_diag, x0, seed):
    """A reference SIR filter: the same algorithm the Rust one implements, in numpy.

    Deliberately independent of filterpy's filter classes -- filterpy has no particle
    filter -- but its resampler is filterpy's own systematic_resample, which is the step
    most easily got subtly wrong.
    """
    rng = np.random.default_rng(seed)
    f = cv_f(dt)
    q = cv_q(dt, sigma_a_sq)
    h = position_h()
    r_inv = np.linalg.inv(np.diag(r_diag))

    # Same factorisation the Rust filter uses: eigen, not Cholesky. See the Rust
    # module for why.
    def psd_factor(m):
        vals, vecs = np.linalg.eigh((m + m.T) / 2.0)
        return vecs @ np.diag(np.sqrt(np.clip(vals, 0.0, None)))

    particles = np.array(x0) + rng.standard_normal((count, STATE_DIM)) @ psd_factor(
        np.diag(p0_diag)
    ).T
    weights = np.full(count, 1.0 / count)
    q_factor = psd_factor(q)

    means = []
    for z in measurements:
        particles = particles @ f.T + rng.standard_normal((count, STATE_DIM)) @ q_factor.T
        residual = np.array(z) - particles @ h.T
        log_w = np.log(weights) - 0.5 * np.einsum("ij,jk,ik->i", residual, r_inv, residual)
        log_w -= log_w.max()
        weights = np.exp(log_w)
        weights /= weights.sum()
        means.append((particles * weights[:, None]).sum(axis=0))
        if 1.0 / np.sum(weights**2) < 0.5 * count:
            indices = systematic_resample(weights)
            particles = particles[indices]
            weights = np.full(count, 1.0 / count)
    return np.array(means)


def write_particle():
    dt = 1.0
    sigma_a_sq = 2.0
    r_diag = [25.0, 25.0, 100.0]
    p0_diag = [400.0, 400.0, 900.0, 100.0, 100.0, 100.0]
    x0 = [0.0, 0.0, 100.0, 20.0, 0.0, 0.0]
    steps = 30
    measurements = [(20.0 * dt * (k + 1), 0.0, 100.0) for k in range(steps)]
    trials = 40
    count = 2000

    runs = np.array(
        [
            reference_sir(measurements, count, dt, sigma_a_sq, r_diag, p0_diag, x0, seed)
            for seed in range(trials)
        ]
    )
    # Mean over trials, and the standard error of that mean. The Rust filter's own
    # across-trial mean is compared against these; the row's criterion is 2 sigma.
    mean = runs.mean(axis=0)
    # ddof=1: this is a sample of trials, not the population.
    stderr = runs.std(axis=0, ddof=1) / np.sqrt(trials)

    # An exact reference the Monte Carlo answer must approach: for this linear-Gaussian
    # model the Kalman filter IS the posterior, so it says where the cloud should be.
    kf = build_kf(dt, cv_f(dt), cv_q(dt, sigma_a_sq), np.diag(r_diag), x0, np.diag(p0_diag))
    exact = []
    for z in measurements:
        kf.predict()
        kf.update(np.array(z, dtype=float))
        exact.append(np.asarray(kf.x).reshape(-1).tolist())

    payload = {
        "oracle": "reference SIR filter over filterpy.monte_carlo.systematic_resample",
        "filterpy": "1.4.5",
        "method_note": (
            "A particle filter's output depends on its draws, so this row is a "
            "statistical comparison, as section 2 says. The reference filter was run "
            "for the recorded number of independent trials; the fixture carries the "
            "across-trial mean and the standard error of that mean. The exact posterior "
            "from a Kalman filter is carried alongside because this model is "
            "linear-Gaussian, so both implementations have a known right answer to "
            "converge to and agreement with each other is not by itself evidence."
        ),
        "trials": trials,
        "particles": count,
        "dt": dt,
        "sigma_a_sq": sigma_a_sq,
        "r_diag": r_diag,
        "p0_diag": p0_diag,
        "x0": x0,
        "measurements": [list(z) for z in measurements],
        "reference_mean": mean.tolist(),
        "reference_stderr": stderr.tolist(),
        "exact_posterior_mean": exact,
        # The raw per-trial position estimate at the last step, for the two-sample KS
        # test. Section 2 offers "KS-test OR mean/variance within 2 sigma"; the KS test
        # compares the whole sampling distribution rather than only its first moment,
        # and it is the one that does not need a multiple-comparison argument.
        "reference_final_samples": runs[:, -1, :3].tolist(),
    }
    (OUT / "particle.json").write_text(json.dumps(payload, indent=1), encoding="utf-8")
    print("particle.json:", trials, "trials of", count, "particles")


if __name__ == "__main__":
    OUT.mkdir(parents=True, exist_ok=True)
    write_imm()
    write_sqrt()
    write_particle()
