# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Oracle fixture for the verification-capability-table.md row
"rfs | PHD / CPHD filter" -> track/cphd.json

Run inside .venv-oracles:

    ../../../.venv-oracles/Scripts/python.exe gen_cphd_fixtures.py

ORACLE: the closed-form Gaussian-mixture CPHD update (Vo, Vo and Cantoni, "Analytic
Implementations of the Cardinalized Probability Hypothesis Density Filter", IEEE
Transactions on Signal Processing 55(7), 2007), re-derived from the basic multi-object
likelihood below rather than transcribed from the paper's own dense notation, and
independently checked against a BRUTE-FORCE enumeration of every possible
target-to-measurement association before being trusted with anything this fixture
gates. `the_closed_form_matches_a_brute_force_enumeration` re-runs that same check as a
Rust-side regression: `LAMBDA_CROSS_CHECK` below records the worst disagreement found so
a regeneration cannot quietly drop the record, the same role `phd.json`'s own
`stonesoup_cardinality_disagreement` field plays for the PHD row.

SECTION 2 NAMED STONE SOUP GM-CPHD, AND STONE SOUP 1.9.1 HAS NO CPHD UPDATER AT ALL.
Checked by import, not assumed from the package name or from the PHD row's own
experience: `stonesoup.updater.pointprocess` exports exactly `PHDUpdater` and nothing
else in this pinned version. So unlike the PHD row -- where the library exists and was
found to disagree -- there is no second, independent library implementation to compare
against here. The cross-check below (brute force against the closed form) is the
substitute: two implementations, derived two different ways, checked against each
other, which is the same shape of evidence a library comparison would have given,
built from what this environment actually has.

THE DERIVATION, for a reader checking this against the paper. For a predicted mixture
{w_i, m_i, P_i} (i=1..J, total mass N = sum(w_i)) and a predicted cardinality
distribution p(n), n=0..N_MAX, and measurements Z with |Z|=M:

  q_j := sum_i (w_i/N) * N(z_j; H m_i, S_i)             -- predictive likelihood at z_j
  Xi_j := q_j / kappa                                    -- ratio to clutter density
  Lambda(n) := sum_{r=0}^{min(n,M)} (n)_r (1-pD)^(n-r) pD^r e_r(Xi)
                                                          -- (n)_r is the falling factorial
  posterior p(n | Z) = p(n) Lambda(n) / sum_n' p(n') Lambda(n')

  A := sum_n n p(n) Lambda(n-1)
  B_j := sum_n n p(n) Lambda_{-j}(n-1)                   -- Lambda with detection j left
                                                          -- out of the elementary
                                                          -- symmetric function
  missed-detection candidate for component i:  w_i * (1-pD) * (A / Znorm) / N
  candidate for component i matched to z_j:    w_i * pD * N(z_j;Hm_i,S_i) * (B_j/Znorm)
                                                / (N * kappa)
  Znorm := sum_n p(n) Lambda(n)                          -- same constant both places

This is re-derived (not copied) from the basic multi-object likelihood: for n targets
drawn i.i.d. from the normalised mixture, independently detected w.p. pD, with Poisson
clutter, the probability of observing exactly Z, summed over every possible
association and marginalised over the unknown target positions via the mixture's own
conjugacy, is exactly `Lambda(n)` above (up to a constant common to every n that cancels
in the posterior). `A` and `B_j` come from the same argument holding one target's
identity fixed while the other n-1 are marginalised the same way, using the elementary-
symmetric-function identity `sum_j Xi_j e_r(Xi_{-j}) == (r+1) e_{r+1}(Xi)` to show the
intensity's normalising constant is the SAME `Znorm` used for the cardinality -- which is
also exactly the identity that catches a sign or index error, since it does not hold for
one and not the other.

TWO INDEPENDENT SANITY IDENTITIES, checked below before any fixture is written. (1) The
updated intensity's integral (sum of GM weights) must equal the updated cardinality
distribution's mean -- both are the same posterior's first moment, computed two
different ways. (2) Feeding the update a Poisson cardinality prior must reproduce the
plain GM-PHD update's weights exactly, since the PHD filter IS the CPHD filter
restricted to that one assumption. Both are asserted in `_self_check()` below, which
runs at import time -- if either fails, generating a fixture from this file is refused.

A THIRD IDENTITY, AND A FOURTH CASE, 2026-09-09. Review before signing found that
`esf_leave_one_out` -- in this file and in its Rust twin -- computed the leave-j-out
functions by forward synthetic division, which is unstable exactly when Xi_j is the
largest value: a well-matched target among clutter. The three original cases feed the
truth positions verbatim as detections, with no clutter and no misses, so they never
reached that regime and could not have caught it; `_self_check` now asserts the
leave-one-out identity on an adversarial vector (and keeps the abandoned recurrence,
shown failing), and `one_target_in_annulus_clutter_with_per_scan_births` gates the
regime against gungnir-rfs directly. See `esf_leave_one_out`'s own docstring for the numbers.

WHAT IS AND IS NOT COMPARED, unchanged from the PHD row: intensity as a function (its
integral and its value at fixed probe points), never as a component list, because two
correct filters that prune and merge in a different order carry the same intensity in a
different number of components.
"""

import datetime
import itertools
import json
import math
import pathlib

import numpy as np

OUT = pathlib.Path(__file__).resolve().parent.parent / "track"
START = datetime.datetime(2026, 9, 6, 0, 0, 0)
N = 6
M = 3
N_MAX = 20

PROB_SURVIVAL = 0.99
PROB_DETECT = 0.95
CLUTTER = 1e-6
PRUNE = 1e-5
MERGE = 4.0
MAX_COMPONENTS = 100
SIGMA_A_SQ = 1.0
DT = 1.0
R_DIAG = [25.0, 25.0, 25.0]
BIRTH_COV = [100.0, 100.0, 100.0, 400.0, 400.0, 400.0]


def cv_q(dt, sigma_a_sq):
    """gungnir_core::ConstantVelocity::q -- the CONTINUOUS form, identical to
    gen_phd_fixtures.py's own so the two rows share one motion model."""
    per_axis = np.array([[dt**3 / 3.0, dt**2 / 2.0], [dt**2 / 2.0, dt]])
    q = np.zeros((N, N))
    for i in range(2):
        for j in range(2):
            for axis in range(3):
                q[3 * i + axis, 3 * j + axis] = per_axis[i, j]
    return q * sigma_a_sq


def cv_f(dt):
    f = np.eye(N)
    for axis in range(3):
        f[axis, 3 + axis] = dt
    return f


def position_h():
    h = np.zeros((M, N))
    for axis in range(3):
        h[axis, axis] = 1.0
    return h


def intensity_at(components, point):
    """The mixture evaluated at one 3-D position, marginalised over velocity -- see
    gen_phd_fixtures.py's own docstring for why this, and not a component list, is
    what two correct filters must agree on."""
    total = 0.0
    for weight, mean, cov in components:
        d = np.asarray(point) - np.asarray(mean)[:3]
        p = np.asarray(cov)[:3, :3]
        det = np.linalg.det(p)
        if det <= 0.0:
            continue
        quadratic = d @ np.linalg.inv(p) @ d
        total += weight * np.exp(-0.5 * quadratic) / np.sqrt((2 * np.pi) ** 3 * det)
    return float(total)


def gaussian_pdf(z, mean, cov):
    d = z - mean
    k = len(z)
    det = np.linalg.det(cov)
    inv = np.linalg.inv(cov)
    return math.exp(-0.5 * d @ inv @ d) / math.sqrt((2 * math.pi) ** k * det)


def esf(values):
    """e_0..e_len(values) via the standard O(n^2) dynamic program."""
    e = [1.0]
    for v in values:
        e.append(0.0)
        for r in range(len(e) - 1, 0, -1):
            e[r] += e[r - 1] * v
    return e


def esf_leave_one_out(values):
    """`esf` with each index left out, by re-running the dynamic program on the other
    m-1 values -- O(m^3) in all, and deliberately NOT the O(m^2) synthetic division of
    E(x) = prod(1 + v_i x) by (1 + v_j x) this replaced on 2026-09-09. That forward
    recurrence, q_k = e_k - v_j q_{k-1}, is unstable exactly when v_j is the LARGEST
    value: deflating a polynomial by a root is stable from only one end, and that is
    the wrong end for a well-matched target's Xi among small clutter Xi's. Measured:
    for Xi = [60] + [1e-3]*12 it fails the identity below by a relative 7.8e40 and
    returns -1.7e4 as an elementary symmetric function of non-negative inputs. The
    dynamic program only adds non-negative products, so it cannot cancel; `_self_check`
    asserts the identity on that vector and keeps the abandoned recurrence to show it
    failing. Rust-side twin: gungnir-rfs's elementary_symmetric_leave_one_out."""
    return [esf(values[:j] + values[j + 1 :]) for j in range(len(values))]


def _synthetic_division_leave_one_out(values):
    """The abandoned O(m^2) recurrence, kept only so `_self_check` can show it failing."""
    e = esf(values)
    m = len(values)
    out = []
    for j in range(m):
        q = [0.0] * m
        if m > 0:
            q[0] = 1.0
        for k in range(1, m):
            q[k] = e[k] - values[j] * q[k - 1]
        out.append(q)
    return out


def falling_factorial(n, r):
    if r > n:
        return 0.0
    out = 1.0
    for k in range(r):
        out *= n - k
    return out


def cphd_update(components, p_n, detections, pD, kappa, h, r_cov, n_max):
    """The closed-form CPHD update, exactly matching gungnir-rfs's own
    CphdFilter::update -- see the module docstring for the derivation."""
    total_weight = sum(w for w, m, p in components)
    prepared = []
    for w, m, p in components:
        pht = p @ h.T
        s = h @ pht + r_cov
        k = pht @ np.linalg.inv(s)
        i_kh = np.eye(len(m)) - k @ h
        cov = i_kh @ p @ i_kh.T + k @ r_cov @ k.T
        prepared.append((w, m, p, k, s, cov))

    q = []
    for z in detections:
        qj = 0.0
        if total_weight > 0.0:
            for w, m, p, k, s, cov in prepared:
                qj += (w / total_weight) * gaussian_pdf(np.asarray(z), h @ m, s)
        q.append(qj)
    xi = [qj / kappa for qj in q]

    e_full = esf(xi)
    e_loo = esf_leave_one_out(xi)

    def lam(n, e_vec):
        r_max = min(n, len(e_vec) - 1)
        return sum(
            falling_factorial(n, rr) * (1 - pD) ** (n - rr) * pD**rr * e_vec[rr]
            for rr in range(r_max + 1)
        )

    lam_full = [lam(n, e_full) for n in range(n_max + 1)]
    lam_loo = [[lam(n, e) for n in range(n_max + 1)] for e in e_loo]

    z_norm = sum(p_n[n] * lam_full[n] for n in range(n_max + 1))
    if not (z_norm > 0.0) or not np.isfinite(z_norm):
        raise ValueError("degenerate scene: the normalising constant is zero")

    posterior_p_n = [p_n[n] * lam_full[n] / z_norm for n in range(n_max + 1)]

    def weighted_shift(lam_at):
        return sum(
            n * p_n[n] * (lam_at[n - 1] if n > 0 else 0.0) for n in range(n_max + 1)
        )

    a = weighted_shift(lam_full)
    b = [weighted_shift(lam_j) for lam_j in lam_loo]

    updated = []
    if total_weight > 0.0:
        miss_scale = (1 - pD) * (a / z_norm) / total_weight
        for w, m, p, k, s, cov in prepared:
            updated.append((w * miss_scale, m, p))
        for j, z in enumerate(detections):
            det_scale = pD * (b[j] / z_norm) / (total_weight * kappa)
            for w, m, p, k, s, cov in prepared:
                lik = gaussian_pdf(np.asarray(z), h @ m, s)
                y = np.asarray(z) - h @ m
                updated.append((w * lik * det_scale, m + k @ y, cov))

    return posterior_p_n, updated


def prune_and_merge(components):
    """Identical to gen_phd_fixtures.py's own -- pruning and merging are the same
    mixture-management step regardless of which filter produced the weights."""
    kept = [c for c in components if c[0] > PRUNE and np.isfinite(c[0])]
    merged = []
    while kept:
        index = int(np.argmax([c[0] for c in kept]))
        leader = kept[index]
        try:
            leader_inverse = np.linalg.inv(leader[2])
        except np.linalg.LinAlgError:
            merged.append(leader)
            kept.pop(index)
            continue
        group, rest = [], []
        for c in kept:
            d = c[1] - leader[1]
            if d @ leader_inverse @ d <= MERGE:
                group.append(c)
            else:
                rest.append(c)
        kept = rest
        weight = sum(c[0] for c in group)
        mean = sum(c[0] * c[1] for c in group) / weight
        cov = sum(c[0] * (c[2] + np.outer(c[1] - mean, c[1] - mean)) for c in group) / weight
        merged.append((weight, mean, (cov + cov.T) / 2.0))
    merged.sort(key=lambda c: -c[0])
    return merged[:MAX_COMPONENTS]


def binomial_thin(p, p_survival, n_max):
    out = [0.0] * (n_max + 1)
    for length, p_l in enumerate(p):
        if p_l == 0.0:
            continue
        for j in range(length + 1):
            out[j] += math.comb(length, j) * p_survival**j * (1 - p_survival) ** (length - j) * p_l
    return out


def poisson_pmf(mean, n_max):
    out = [0.0] * (n_max + 1)
    out[0] = math.exp(-mean)
    for n in range(1, n_max + 1):
        out[n] = out[n - 1] * mean / n
    return out


def cardinality_predict(previous, p_survival, birth_mean, n_max):
    thinned = binomial_thin(previous, p_survival, n_max)
    birth = poisson_pmf(birth_mean, n_max)
    out = [0.0] * (n_max + 1)
    for n in range(n_max + 1):
        for j in range(n + 1):
            out[n] += birth[n - j] * thinned[j]
    return out


def reference_gm_cphd(scans, births_per_scan, n_max=N_MAX, kappa=CLUTTER):
    """The full multi-scan CPHD recursion: predict (intensity exactly as GM-PHD,
    cardinality by binomial thinning convolved with a Poisson birth count) then the
    closed-form update above, every scan."""
    f = cv_f(DT)
    q_proc = cv_q(DT, SIGMA_A_SQ)
    h = position_h()
    r_cov = np.diag(R_DIAG)
    components = []
    p_n = [1.0] + [0.0] * n_max

    per_scan = []
    for detections, births in zip(scans, births_per_scan):
        components = [(w * PROB_SURVIVAL, f @ m, f @ p @ f.T + q_proc) for (w, m, p) in components]
        birth_mean = sum(w for w, m, p in births)
        p_n = cardinality_predict(p_n, PROB_SURVIVAL, birth_mean, n_max)
        components.extend(births)

        p_n, updated = cphd_update(components, p_n, detections, PROB_DETECT, kappa, h, r_cov, n_max)
        components = prune_and_merge(updated)

        per_scan.append((list(p_n), components))
    return per_scan


def lambda_bruteforce(n, q_vals, pD, kappa):
    """Lambda(n) by literal enumeration of every partial injective function from `n`
    (anonymous) targets to the `len(q_vals)` measurements, independent of the
    elementary-symmetric-function bookkeeping `lam` above uses -- see the module
    docstring."""
    m = len(q_vals)
    total = 0.0
    for r in range(0, min(n, m) + 1):
        for targets in itertools.combinations(range(n), r):
            for meas in itertools.permutations(range(m), r):
                prod = 1.0
                for j in meas:
                    prod *= pD * q_vals[j]
                prod *= (1 - pD) ** (n - r)
                prod *= kappa ** (m - len(set(meas)))
                total += prod
    return total


def _self_check():
    """Run at import time. Two independent sanity identities plus the brute-force
    cross-check; raises if any fails, refusing to let a broken derivation generate a
    fixture. Returns the worst brute-force disagreement found, recorded in the
    fixture so a regeneration cannot quietly drop the evidence."""
    rng = np.random.default_rng(20260908)

    # Identity 1: intensity integral == cardinality posterior mean.
    comps = [
        (rng.uniform(0.2, 1.0), rng.uniform(-5, 5, size=1), np.eye(1) * rng.uniform(0.5, 2.0))
        for _ in range(2)
    ]
    p_n = np.array([math.exp(-2.0) * 2.0**n / math.factorial(n) for n in range(7)])
    p_n /= p_n.sum()
    h1, r1 = np.eye(1), np.eye(1)
    post_p, updated = cphd_update(comps, p_n.tolist(), [np.array([0.3]), np.array([4.8])], 0.9, 0.01, h1, r1, 6)
    if abs(sum(post_p) - 1.0) > 1e-9:
        raise AssertionError("cardinality posterior does not sum to 1")
    if abs(sum(w for w, m, p in updated) - sum(n * p for n, p in enumerate(post_p))) > 1e-9:
        raise AssertionError("intensity integral does not equal cardinality mean")

    # Identity 2: a Poisson cardinality prior reproduces the plain GM-PHD update.
    comps2 = [
        (rng.uniform(0.2, 1.0), rng.uniform(-5, 5, size=1), np.eye(1) * rng.uniform(0.5, 2.0))
        for _ in range(3)
    ]
    total = sum(w for w, m, p in comps2)
    n_max_big = 40
    p_poisson = np.array([math.exp(-total) * total**n / math.factorial(n) for n in range(n_max_big + 1)])
    p_poisson /= p_poisson.sum()
    _, cphd_updated = cphd_update(comps2, p_poisson.tolist(), [np.array([-1.2]), np.array([2.7])], 0.9, 0.01, h1, r1, n_max_big)

    def phd_update(components, detections, pD, kappa, h, r_cov):
        updated = [(w * (1 - pD), m, p) for w, m, p in components]
        prepared = []
        for w, m, p in components:
            s = h @ p @ h.T + r_cov
            k = p @ h.T @ np.linalg.inv(s)
            i_kh = np.eye(len(m)) - k @ h
            cov = i_kh @ p @ i_kh.T + k @ r_cov @ k.T
            prepared.append((w, m, s, k, cov))
        for z in detections:
            candidates, tot = [], kappa
            for w, m, s, k, cov in prepared:
                lik = gaussian_pdf(np.asarray(z), h @ m, s)
                weight = pD * w * lik
                tot += weight
                candidates.append((weight, m + k @ (np.asarray(z) - h @ m), cov))
            if tot > 0:
                updated.extend((w / tot, m, p) for w, m, p in candidates)
        return updated

    phd_updated = phd_update(comps2, [np.array([-1.2]), np.array([2.7])], 0.9, 0.01, h1, r1)
    a = sorted(w for w, m, p in cphd_updated)
    b = sorted(w for w, m, p in phd_updated)
    if any(abs(x - y) > 1e-6 for x, y in zip(a, b)):
        raise AssertionError("CPHD with a Poisson prior does not reduce to plain GM-PHD")

    # Identity 3 (2026-09-09): the leave-one-out functions on one dominant value among
    # many small ones -- sum_j Xi_j e_r(Xi_{-j}) == (r+1) e_{r+1}(Xi), every value
    # non-negative -- which the synthetic-division recurrence this file used to carry
    # breaks by tens of orders of magnitude, and which the direct recomputation must
    # satisfy to machine precision. The recurrence is kept and shown failing so the
    # reason for the cubic cost cannot be forgotten.
    def _loo_identity_worst(xi, loo):
        e = esf(xi)
        worst_here = 0.0
        for r in range(len(xi)):
            lhs = sum(xi[j] * loo[j][r] for j in range(len(xi)))
            rhs = (r + 1) * e[r + 1]
            worst_here = max(worst_here, abs(lhs - rhs) / abs(rhs))
        return worst_here

    for clutter in (12, 15):
        xi_adv = [60.0] + [1e-3] * clutter
        direct = esf_leave_one_out(xi_adv)
        if any(v < 0.0 for q in direct for v in q):
            raise AssertionError("a leave-one-out elementary symmetric function went negative")
        if _loo_identity_worst(xi_adv, direct) > 1e-12:
            raise AssertionError("leave-one-out ESFs break their identity on a dominant value")
        if _loo_identity_worst(xi_adv, _synthetic_division_leave_one_out(xi_adv)) < 1e6:
            raise AssertionError(
                "the abandoned synthetic-division recurrence has become stable on the "
                "adversarial vector; re-derive before ever switching back to it"
            )

    # Cross-check: closed form vs. brute-force association enumeration.
    worst = 0.0
    for _ in range(40):
        n = int(rng.integers(0, 5))
        m = int(rng.integers(0, 4))
        pD_t = float(rng.uniform(0.3, 0.99))
        kappa_t = float(rng.uniform(0.01, 2.0))
        q_vals = rng.uniform(0.01, 3.0, size=m).tolist()
        bf = lambda_bruteforce(n, q_vals, pD_t, kappa_t)
        xi = [qv / kappa_t for qv in q_vals]
        e = esf(xi)
        r_max = min(n, m)
        cf = sum(
            falling_factorial(n, rr) * (1 - pD_t) ** (n - rr) * pD_t**rr * e[rr]
            for rr in range(r_max + 1)
        ) * kappa_t**m
        denom = max(abs(bf), 1e-12)
        worst = max(worst, abs(bf - cf) / denom)
        if abs(bf - cf) > 1e-6 * max(1.0, abs(bf)):
            raise AssertionError(f"Lambda(n) disagrees with brute force: n={n} m={m} bf={bf} cf={cf}")
    return worst


LAMBDA_CROSS_CHECK = _self_check()


class SplitMix64:
    """SplitMix64 with the standard constants, bit-identical to cphd_diff.rs's own;
    `next_unit` is (z >> 11) / 2^53, exact in both languages. A tiny shared stream is
    what lets both sides draw the same random clutter without sharing a library."""

    MASK = (1 << 64) - 1

    def __init__(self, seed):
        self.state = seed & self.MASK

    def next_u64(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & self.MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & self.MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & self.MASK
        return z ^ (z >> 31)

    def next_unit(self):
        return (self.next_u64() >> 11) / float(1 << 53)


def clutter_seed(scan, t):
    return 0x5EED + 1000 * scan + t


def annulus_clutter(center, radius_min, radius_max, count, scan, t):
    """`count` clutter returns uniform in the annulus [radius_min, radius_max] around
    `center` at its height, from a SplitMix64 stream seeded by scan and truth index.
    Genuinely random rather than a fixed ring: a return that recurred at the same
    position every scan would be indistinguishable from a stationary target, and a
    first draft of this case with a fixed ring did exactly that, converging on
    fifteen targets for a one-target scene."""
    rng = SplitMix64(clutter_seed(scan, t))
    out = []
    for _ in range(count):
        r = radius_min + (radius_max - radius_min) * rng.next_unit()
        angle = 2.0 * math.pi * rng.next_unit()
        out.append([center[0] + r * math.cos(angle), center[1] + r * math.sin(angle), center[2]])
    return out


def build_case(name, truth, scan_count, birth_scans, n_max=N_MAX, clutter_annulus=None,
               per_scan_births=(), clutter_density=None):
    """`clutter_annulus=(radius_min, radius_max, count)` adds that much random clutter
    around every truth position on every scan; `per_scan_births` is a list of
    `([x, y, z], weight)` broad births added on EVERY scan, on top of the truth births
    at `birth_scans`; `clutter_density` overrides the shared CLUTTER for this case
    alone, so a case can carry a clutter rate its own scans are consistent with
    without moving the three original, clutter-free cases. Together these put a scan
    into the regime the leave-one-out ESFs are unstable in (one dominant Xi among many
    small ones, under a fat cardinality prior); the original cases never reach it."""
    scans, births_per_scan = [], []
    for scan in range(scan_count):
        detections = [list(p) for p in truth]
        if clutter_annulus is not None:
            radius_min, radius_max, count = clutter_annulus
            for t, p in enumerate(truth):
                detections.extend(annulus_clutter(p, radius_min, radius_max, count, scan, t))
        scans.append(detections)
        births = []
        if scan in birth_scans:
            births.extend(
                (0.4, np.array(list(p) + [0.0, 0.0, 0.0]), np.diag(BIRTH_COV)) for p in truth
            )
        births.extend(
            (w, np.array(list(pos) + [0.0, 0.0, 0.0]), np.diag(BIRTH_COV))
            for pos, w in per_scan_births
        )
        births_per_scan.append(births)

    per_scan_result = reference_gm_cphd(
        scans, births_per_scan, n_max, CLUTTER if clutter_density is None else clutter_density
    )

    probes = [list(p) for p in truth]
    probes.append([sum(p[0] for p in truth) / len(truth) + 1500.0, 0.0, 0.0])

    per_scan = []
    for p_n, components in per_scan_result:
        mean = sum(n * p for n, p in enumerate(p_n))
        mode = max(range(len(p_n)), key=lambda n: p_n[n])
        per_scan.append(
            {
                "cardinality_distribution": p_n,
                "cardinality_mean": mean,
                "cardinality_map": mode,
                "intensity_at_probes": [intensity_at(components, p) for p in probes],
            }
        )

    case = {
        "name": name,
        "truth": [list(p) for p in truth],
        "scan_count": scan_count,
        "birth_scans": sorted(birth_scans),
        "probes": probes,
        "per_scan": per_scan,
    }
    # Only emitted when set, so the three original cases' JSON is unchanged.
    if clutter_annulus is not None:
        case["clutter_annulus"] = {
            "radius_min": clutter_annulus[0],
            "radius_max": clutter_annulus[1],
            "count": clutter_annulus[2],
        }
    if per_scan_births:
        case["per_scan_births"] = [[list(pos), w] for pos, w in per_scan_births]
    if clutter_density is not None:
        case["clutter_density"] = clutter_density
    return case


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    cases = [
        build_case("three_separated_targets", [[0.0, 0.0, 100.0], [300.0, 0.0, 100.0],
                                               [0.0, 400.0, 100.0]], 20, {0}),
        build_case("one_target", [[0.0, 0.0, 100.0]], 15, {0}),
        build_case("six_targets_reborn_midway",
                   [[i * 150.0, 0.0, 100.0] for i in range(6)], 24, {0, 12}),
        # The regime that broke the leave-one-out ESFs (2026-09-09): one target, 12
        # random clutter returns in a 45-90 m annulus around it every scan (small
        # but non-zero likelihood against the target's own component), one broad birth
        # of weight 2.0 far away every scan to keep the cardinality prior's tail
        # fat, and a clutter density of 1e-5 this case's own scans are consistent with.
        # Before the fix, a single scan of this shape put 1700x the correct weight on
        # the target with a thin prior, and extracted a track 35 km away with a fat
        # one. A first draft used a fixed ring of twelve returns and four births of
        # weight 1.0 per scan under the shared 1e-6 clutter density, and the oracle
        # correctly concluded those were targets: returns recurring at fixed positions
        # are stationary targets, and twelve returns where the model expects a fifth
        # of one cannot all be clutter. Measured before landing (a sweep of candidate
        # scenes, each run twice, with the abandoned recurrence swapped back in for
        # the second run): this scene keeps cardinality mode 1 on every scan but
        # the two in which the target is still being confirmed, with the corrected
        # functions, and the recurrence moves the intensity at the target
        # probe by a relative 3e-2 from scan 6 on, against cphd_diff.rs's 1e-3
        # tolerance -- so a reintroduced recurrence fails this case thirtyfold. Fewer
        # returns or a thinner birth prior kept the scene honest but blind (8 returns:
        # 1e-7), and the shared clutter density kept it sensitive but dishonest;
        # the two pull against each other, since the visible garbage scales with the
        # target's likelihood ratio to clutter.
        build_case("one_target_in_annulus_clutter_with_per_scan_births",
                   [[0.0, 0.0, 100.0]], 20, {0}, clutter_annulus=(45, 90, 12),
                   per_scan_births=[([20_000.0, 20_000.0, 100.0], 2.0)],
                   clutter_density=1e-5),
    ]
    payload = {
        "oracle": (
            "the closed-form Gaussian-mixture CPHD update (Vo, Vo and Cantoni 2007), "
            "re-derived from the basic multi-object likelihood and checked against "
            "brute-force association enumeration -- see the module docstring"
        ),
        "stonesoup": "1.9.1",
        "stonesoup_status": (
            "no CPHD updater exists in this version -- stonesoup.updater.pointprocess "
            "exports only PHDUpdater, confirmed by import rather than assumed"
        ),
        "lambda_cross_check_worst_relative_error": LAMBDA_CROSS_CHECK,
        "settings": {
            "probability_of_survival": PROB_SURVIVAL,
            "probability_of_detection": PROB_DETECT,
            "clutter_density": CLUTTER,
            "prune_threshold": PRUNE,
            "merge_distance": MERGE,
            "max_components": MAX_COMPONENTS,
            "max_cardinality": N_MAX,
            "sigma_a_sq": SIGMA_A_SQ,
            "dt": DT,
            "r_diag": R_DIAG,
            "birth_cov_diag": BIRTH_COV,
            "birth_weight": 0.4,
        },
        "method_note": (
            "The intensity is compared as a FUNCTION -- its integral, and its value at "
            "fixed probe points -- not as a component list, for the same reason as the "
            "PHD row: two correct filters that prune and merge in a different order "
            "carry the same intensity in a different number of components."
        ),
        "cases": cases,
    }
    (OUT / "cphd.json").write_text(json.dumps(payload, indent=1), encoding="utf-8")
    print("lambda cross-check worst relative error:", LAMBDA_CROSS_CHECK)
    for case in cases:
        last = case["per_scan"][-1]
        print(case["name"], "final cardinality mean", last["cardinality_mean"],
              "map", last["cardinality_map"])


if __name__ == "__main__":
    main()
