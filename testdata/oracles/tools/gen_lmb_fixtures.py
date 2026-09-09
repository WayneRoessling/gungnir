# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Oracle fixture for the verification-capability-table.md row
"rfs | GLMB / LMB filter" -> track/lmb.json

Run inside .venv-oracles:

    ../../../.venv-oracles/Scripts/python.exe gen_lmb_fixtures.py

ORACLE: the labelled multi-Bernoulli filter (Reuter, Vo, Vo and Dietmayer, "The Labeled
Multi-Bernoulli Filter", IEEE Transactions on Signal Processing 62(12), 2014), whose
update is the exact delta-GLMB update of an LMB prior followed by a moment-matched
projection back onto an LMB. Re-derived below from the multi-object likelihood rather
than transcribed, and the fixture's own numbers are produced by the most obviously
correct implementation available -- LITERAL ENUMERATION of every association event --
rather than by the clever algorithm the Rust side runs.

SECTION 1 AND SECTION 2 NAMED "Stone Soup GLMB (partial)", AND STONE SOUP 1.9.1 HAS NO
GLMB AND NO LMB -- NOT PARTIAL, ABSENT. Checked three ways in `stonesoup_status()`
below, which runs at import and is recorded in the fixture:

  1. `pkgutil.walk_packages` over the installed package: no module whose name contains
     glmb, lmb, labell/labeled or bernoulli-multi.
  2. A regex scan of every .py file shipped in the package for the identifiers
     GLMB / LMB / (Generalised|Generalized)Label*/ LabelledMultiBernoulli / delta-GLMB:
     ZERO source lines match, in the whole distribution.
  3. Direct `importlib.import_module` of the plausible paths (stonesoup.updater.glmb,
     stonesoup.tracker.glmb, stonesoup.types.labelled): all ModuleNotFoundError.

What 1.9.1 does have nearby, and why none of it is this row's oracle:
`BernoulliParticleUpdater`/`BernoulliParticlePredictor` are the SINGLE-target Bernoulli
particle filter (Ristic et al.'s tutorial) -- one target, no labels, no multi-target
association; `LCCUpdater` and `PHDUpdater` in `updater.pointprocess` are unlabelled
point-process filters, which is the very thing this row exists to be distinguished from;
`dataassociator.mfa` merely CITES a PMBM paper in its docstring reference list and
implements multi-frame assignment, not a labelled RFS filter (and the PHD/CPHD work
already recorded that it will not import without an uninstalled solver). So unlike the
PHD row -- where the library exists and was found to disagree -- and like the CPHD row --
where the updater is simply absent -- there is no library implementation to compare
against, established by evidence rather than assumed.

--------------------------------------------------------------------------------
THE DERIVATION
--------------------------------------------------------------------------------

An LMB density is one Bernoulli per label: label l has existence probability r_l and a
normalised spatial density p_l(x). Predict is closed on this family (survival thins r_l
by pS; the spatial density goes through the motion model; births are new labels). The
UPDATE IS NOT closed on it, and that is the whole content of this filter.

Measurement model, standard: each existing target is detected with probability pD giving
z with likelihood g(z|x), missed with probability 1-pD; clutter is Poisson with intensity
kappa. For a label set I with states x_l, the multi-object likelihood is

    g(Z|X) = e^{-lambda_c} kappa^Z * sum_theta prod_{l in I} psi_Z(x_l; theta(l))

over maps theta from I to {0, 1..M} that are injective on the positive values, with

    psi_Z(x; 0) = 1 - pD                         (this target was not detected)
    psi_Z(x; j) = pD g(z_j|x) / kappa(z_j)       (this target produced detection j)

An unassigned detection contributes exactly the kappa(z_j) already in the common kappa^Z
factor, so it carries weight 1 here; that is why no separate clutter term appears below.

Integrating psi against p_l and folding in the LMB prior weight gives, for each label, a
weight per EXTENDED association a_l in {absent, 0 (missed), 1..M}:

    u_l(absent) = 1 - r_l
    u_l(0)      = r_l (1 - pD)
    u_l(j)      = r_l pD q_l(j) / kappa,   q_l(j) = <p_l, g(z_j|.)>

and the exact posterior over the joint assignment vector a = (a_1..a_N) is

    P(a) = (1/Z) prod_l u_l(a_l),   Z = sum over a injective-on-positives of prod_l u_l(a_l)

That IS the delta-GLMB posterior of an LMB prior, written in the only variables it
depends on. The LMB projection then matches, per label, the first moment:

    r_hat_l  = 1 - P(a_l = absent) = sum_{a in {0..M}} P(a_l = a)
    p_hat_l  = (1/r_hat_l) * [ P(a_l=0) p_l + sum_j P(a_l=j) p_l^{(j)} ]

with p_l^{(j)} the Kalman update of p_l by z_j. Only the per-label MARGINALS of P are
needed -- the joint is never materialised -- and matching them makes the projection
exact for every label's own existence and spatial density, and hence exact for the PHD.

WHAT IS THEREFORE APPROXIMATED, precisely and only: the joint P(a) couples the labels
(if label 1 takes detection 3, label 2 cannot), and an LMB is a product of independent
Bernoullis, so the projection DISCARDS THE INTER-LABEL DEPENDENCE. A single update from
an LMB prior is exact in every per-label marginal; the error is what the NEXT scan
inherits, because it starts from the projected product rather than the true joint. That
statement is not asserted here, it is MEASURED: `delta_glmb_divergence()` runs a full
untruncated delta-GLMB alongside the LMB on the same detections and records the largest
existence-probability disagreement, scan by scan -- and asserts it is exactly zero after
the first update, which is the sharpest available check that the moment matching is right.

Computing the marginals of P exactly is a permanent computation and is #P-hard in
general; the bound the Rust side imposes on detections per scan is that fact, not an
implementation shortcut.

--------------------------------------------------------------------------------
FIVE INDEPENDENT CHECKS, all run in `_self_check()` at import time. If any fails,
generating a fixture from this file is refused.
--------------------------------------------------------------------------------

  (1) THREE IMPLEMENTATIONS OF THE MARGINALS AGREE. `marginals_enumerate` (literal
      itertools.product over the whole extended assignment space, filtered for
      injectivity -- the definition, and what writes this fixture), `marginals_dp`
      (the forward/backward subset dynamic program, which is what gungnir-rfs runs) and
      `normaliser_transposed` (a third index order: a dynamic program over DETECTIONS
      whose state is the set of labels consumed, rather than over labels whose state is
      the set of detections consumed). Two of the three are genuinely different
      derivations, so a bookkeeping error in the DP the Rust side runs cannot be
      repeated by the check. Worst relative disagreement is recorded in the fixture.

  (2) EXACT REDUCTION TO THE SINGLE-TARGET BERNOULLI FILTER. With one label the joint
      association sum is trivial and the update must equal the textbook Bernoulli-filter
      posterior r_hat = r(1-pD+pD*sum_j q_j/kappa) / (1 - r*pD + r*pD*sum_j q_j/kappa),
      written out independently below. Checked to machine precision.

  (3) EXACT FACTORISATION WHEN THE LABELS DO NOT COMPETE. If every detection is
      overwhelmingly likelier under one label than any other, the joint update must come
      apart into independent single-target Bernoulli updates -- the combinatorial
      bookkeeping must vanish exactly where the combinatorics does.

  (4) THE MARGINALS ARE A DISTRIBUTION, AND THE DETECTIONS ARE NOT DOUBLE-SPENT. For
      every label sum_a P(a_l = a) == 1, and for every detection sum_l P(a_l = j) <= 1,
      because a detection has at most one source. The second does not follow from the
      first and is what catches an injectivity error.

  (5) THE SPATIAL MIXTURE'S MASS IS THE EXISTENCE PROBABILITY. Before normalising, the
      updated spatial mixture's weights sum to exactly r_hat_l -- the same quantity
      arrived at from the association marginals alone. Two routes to one number.

WHAT IS AND IS NOT COMPARED: per label, the existence probability and the spatial density
as a FUNCTION (its value at fixed probe points), plus the label set itself. Never the
per-label component list, for the same reason the PHD and CPHD rows give: two correct
filters that prune and merge in a different order carry the same density in a different
number of components. The label set IS compared exactly, because label identity is the
entire reason this row exists separately from the PHD/CPHD one.
"""

import datetime
import importlib
import itertools
import json
import math
import pathlib
import pkgutil
import re

import numpy as np

OUT = pathlib.Path(__file__).resolve().parent.parent / "track"
START = datetime.datetime(2026, 9, 8, 0, 0, 0)
N = 6
M = 3

# Shared with gen_phd_fixtures.py / gen_cphd_fixtures.py so the three rfs rows describe
# the same sky; only the filter differs.
PROB_SURVIVAL = 0.99
PROB_DETECT = 0.95
CLUTTER = 1e-6
MERGE = 4.0
MAX_COMPONENTS = 20
SIGMA_A_SQ = 1.0
DT = 1.0
R_DIAG = [25.0, 25.0, 25.0]
BIRTH_COV = [100.0, 100.0, 100.0, 400.0, 400.0, 400.0]
BIRTH_EXISTENCE = 0.4
# A Bernoulli below this existence probability is dropped and its label retired.
EXISTENCE_PRUNE = 1e-4
# A component below this share of its own label's spatial density is dropped. This is a
# share of a NORMALISED density, not of an intensity, which is why it is not the PHD
# row's 1e-5 absolute weight.
SPATIAL_PRUNE = 1e-6


def cv_f(dt):
    f = np.eye(N)
    for axis in range(3):
        f[axis, 3 + axis] = dt
    return f


def cv_q(dt, sigma_a_sq):
    """gungnir_core::ConstantVelocity::q -- the continuous form, identical to
    gen_phd_fixtures.py's and gen_cphd_fixtures.py's own."""
    per_axis = np.array([[dt**3 / 3.0, dt**2 / 2.0], [dt**2 / 2.0, dt]])
    q = np.zeros((N, N))
    for i in range(2):
        for j in range(2):
            for axis in range(3):
                q[3 * i + axis, 3 * j + axis] = per_axis[i, j]
    return q * sigma_a_sq


def position_h():
    h = np.zeros((M, N))
    for axis in range(3):
        h[axis, axis] = 1.0
    return h


def gaussian_pdf(z, mean, cov):
    d = np.asarray(z) - np.asarray(mean)
    k = len(d)
    det = np.linalg.det(cov)
    if det <= 0.0:
        return 0.0
    inv = np.linalg.inv(cov)
    return math.exp(-0.5 * d @ inv @ d) / math.sqrt((2 * math.pi) ** k * det)


def density_at(components, point):
    """A label's spatial density evaluated at one 3-D position, marginalised over
    velocity. The per-label weights are normalised, so this is a probability density,
    not the PHD row's intensity -- the two rows compare the same KIND of object (a
    function at fixed probe points) but not the same object."""
    total = 0.0
    for weight, mean, cov in components:
        d = np.asarray(point) - np.asarray(mean)[:3]
        p = np.asarray(cov)[:3, :3]
        det = np.linalg.det(p)
        if det <= 0.0:
            continue
        total += weight * math.exp(-0.5 * d @ np.linalg.inv(p) @ d) / math.sqrt(
            (2 * math.pi) ** 3 * det
        )
    return float(total)


# --------------------------------------------------------------------------------
# Stone Soup: establish, by evidence, that this row has no library oracle.
# --------------------------------------------------------------------------------


def stonesoup_status():
    try:
        import stonesoup
    except ImportError:
        return "stonesoup is not installed in this environment -- absence NOT established"

    root = pathlib.Path(stonesoup.__file__).parent
    named = [
        m.name
        for m in pkgutil.walk_packages([str(root)], prefix="stonesoup.")
        if any(k in m.name.lower() for k in ("glmb", "lmb", "labell", "labeled"))
    ]
    pattern = re.compile(
        r"\b(GLMB|LMB|Generalis?edLabell?ed|Generaliz?edLabel|"
        r"Label?l?edMultiBernoulli|delta-?GLMB)\b",
        re.I,
    )
    source_hits = 0
    for path in root.rglob("*.py"):
        text = path.read_text(encoding="utf-8", errors="replace")
        source_hits += sum(1 for line in text.splitlines() if pattern.search(line))

    unimportable = []
    for target in ("stonesoup.updater.glmb", "stonesoup.tracker.glmb", "stonesoup.types.labelled"):
        try:
            importlib.import_module(target)
        except ImportError:
            unimportable.append(target)

    if named or source_hits:
        raise AssertionError(
            f"Stone Soup {stonesoup.__version__} DOES contain labelled-filter symbols "
            f"(modules={named}, source lines={source_hits}); this generator's premise "
            "that no library oracle exists is false and the row must be re-planned "
            "against the library instead."
        )
    return (
        f"no GLMB and no LMB exist in stonesoup {stonesoup.__version__} -- not partial, "
        f"absent: zero modules named for one, zero source lines in the whole package "
        f"mentioning GLMB/LMB/labelled-multi-Bernoulli, and {len(unimportable)} of 3 "
        "plausible import paths ModuleNotFoundError. The nearest neighbours are the "
        "SINGLE-target BernoulliParticleUpdater and the unlabelled PHDUpdater/LCCUpdater"
    )


# --------------------------------------------------------------------------------
# The association marginals, three ways.
# --------------------------------------------------------------------------------
#
# `u` is a list over labels; u[l] is a list of length M+2 indexed as
#   0        -> the label does not exist
#   1        -> it exists and was not detected
#   2 + j    -> it exists and produced detection j
# Every implementation below returns (marginals, normaliser) with the same indexing,
# marginals[l] summing to 1.

ABSENT = 0
MISSED = 1
FIRST_DETECTION = 2


def _rescale(u):
    """Divide each label's weight vector by its own largest entry.

    Scaling one label's vector scales EVERY joint assignment weight by the same factor,
    so the marginals are invariant -- but the products taken across labels are not, and
    with kappa at 1e-6 a per-label weight of order 1e4 raised to the number of labels
    overflows a double long before the scene is interesting. The Rust side does the same
    thing for the same reason.
    """
    out = []
    for row in u:
        peak = max(row)
        out.append([v / peak for v in row] if peak > 0.0 else list(row))
    return out


def marginals_enumerate(u):
    """Literal enumeration of the whole extended assignment space, filtered for
    injectivity on the detections. This is the definition transcribed, with no
    bookkeeping to get wrong, and it is what writes the committed fixture."""
    u = _rescale(u)
    n = len(u)
    width = len(u[0]) if n else FIRST_DETECTION
    marginals = [[0.0] * width for _ in range(n)]
    total = 0.0
    for assignment in itertools.product(range(width), repeat=n):
        claimed = [a for a in assignment if a >= FIRST_DETECTION]
        if len(set(claimed)) != len(claimed):
            continue
        weight = 1.0
        for label, a in enumerate(assignment):
            weight *= u[label][a]
        if weight == 0.0:
            continue
        total += weight
        for label, a in enumerate(assignment):
            marginals[label][a] += weight
    if total <= 0.0:
        raise ValueError("no assignment has any weight: the scene is degenerate")
    return [[v / total for v in row] for row in marginals], total


def marginals_dp(u):
    """Forward/backward dynamic program whose state is the SET OF DETECTIONS consumed.

    This is the algorithm gungnir-rfs runs, restated here so the check compares two
    derivations rather than one implementation against itself.

      F[l][S] = weight of assignments of labels 0..l-1 consuming EXACTLY the set S
      B[l][S] = weight of assignments of labels l..n-1 consuming ANY SUBSET of S

    A label consuming no detection has combined weight c_l = u_l(absent) + u_l(missed);
    the two are split afterwards in proportion, since they have identical combinatorial
    structure. Then a label l takes detection j exactly when some prefix consumed a set S
    not containing j and the suffix consumed a subset of what is left.
    """
    u = _rescale(u)
    n = len(u)
    m = (len(u[0]) - FIRST_DETECTION) if n else 0
    size = 1 << m
    full = size - 1
    c = [row[ABSENT] + row[MISSED] for row in u]

    forward = [[0.0] * size for _ in range(n + 1)]
    forward[0][0] = 1.0
    for l in range(n):
        for s in range(size):
            here = forward[l][s]
            if here == 0.0:
                continue
            forward[l + 1][s] += here * c[l]
            for j in range(m):
                if s & (1 << j):
                    continue
                forward[l + 1][s | (1 << j)] += here * u[l][FIRST_DETECTION + j]

    backward = [[0.0] * size for _ in range(n + 2)]
    for s in range(size):
        backward[n][s] = 1.0
    for l in range(n - 1, -1, -1):
        for s in range(size):
            acc = backward[l + 1][s] * c[l]
            for j in range(m):
                if s & (1 << j):
                    acc += backward[l + 1][s & ~(1 << j)] * u[l][FIRST_DETECTION + j]
            backward[l][s] = acc

    total = backward[0][full] if n else 1.0
    if total <= 0.0:
        raise ValueError("no assignment has any weight: the scene is degenerate")

    width = len(u[0]) if n else FIRST_DETECTION
    marginals = [[0.0] * width for _ in range(n)]
    for l in range(n):
        no_detection = 0.0
        for s in range(size):
            here = forward[l][s]
            if here == 0.0:
                continue
            rest = full & ~s
            no_detection += here * c[l] * backward[l + 1][rest]
            for j in range(m):
                if s & (1 << j):
                    continue
                marginals[l][FIRST_DETECTION + j] += (
                    here * u[l][FIRST_DETECTION + j] * backward[l + 1][rest & ~(1 << j)]
                )
        if c[l] > 0.0:
            marginals[l][ABSENT] = no_detection * u[l][ABSENT] / c[l]
            marginals[l][MISSED] = no_detection * u[l][MISSED] / c[l]
    return [[v / total for v in row] for row in marginals], total


def normaliser_transposed(u):
    """The normalising constant only, from a dynamic program over DETECTIONS whose state
    is the set of LABELS consumed -- the transpose of `marginals_dp`'s index order, so an
    off-by-one or an inverted subset test in one cannot be present in the other."""
    u = _rescale(u)
    n = len(u)
    m = (len(u[0]) - FIRST_DETECTION) if n else 0
    size = 1 << n
    c = [row[ABSENT] + row[MISSED] for row in u]

    table = [0.0] * size
    table[0] = 1.0
    for j in range(m):
        nxt = list(table)
        for used in range(size):
            here = table[used]
            if here == 0.0:
                continue
            for l in range(n):
                if used & (1 << l):
                    continue
                nxt[used | (1 << l)] += here * u[l][FIRST_DETECTION + j]
        table = nxt

    total = 0.0
    for used in range(size):
        if table[used] == 0.0:
            continue
        idle = 1.0
        for l in range(n):
            if not used & (1 << l):
                idle *= c[l]
        total += table[used] * idle
    return total


# --------------------------------------------------------------------------------
# The LMB filter itself.
# --------------------------------------------------------------------------------
#
# A Bernoulli is (label, r, components) with components a list of (weight, mean, cov)
# whose weights sum to 1: the spatial density is normalised, unlike a PHD intensity.


def lmb_predict(bernoullis, births, p_survival, dt, sigma_a_sq):
    f = cv_f(dt)
    q = cv_q(dt, sigma_a_sq)
    out = []
    for label, r, components in bernoullis:
        moved = [(w, f @ m, (f @ p @ f.T + q + (f @ p @ f.T + q).T) / 2.0) for (w, m, p) in components]
        out.append((label, r * p_survival, moved))
    out.extend(births)
    # Sorted by label everywhere, so the association-marginal table's row order is the
    # label order on both sides of the comparison rather than an accident of when each
    # Bernoulli happened to be born.
    out.sort(key=lambda b: b[0])
    return out


def prune_and_merge(components, merge_distance, max_components, prune):
    """The same mixture management the PHD and CPHD rows use, applied WITHIN one label's
    spatial density and then renormalised, because a density that no longer integrates
    to 1 is not one."""
    kept = [c for c in components if c[0] > prune and np.isfinite(c[0])]
    if not kept:
        kept = list(components)
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
            if d @ leader_inverse @ d <= merge_distance:
                group.append(c)
            else:
                rest.append(c)
        kept = rest
        weight = sum(c[0] for c in group)
        mean = sum(c[0] * c[1] for c in group) / weight
        cov = sum(c[0] * (c[2] + np.outer(c[1] - mean, c[1] - mean)) for c in group) / weight
        merged.append((weight, mean, (cov + cov.T) / 2.0))
    merged.sort(key=lambda c: -c[0])
    merged = merged[:max_components]
    mass = sum(c[0] for c in merged)
    return [(w / mass, m, p) for (w, m, p) in merged]


def association_weights(bernoullis, detections, pD, kappa, h, r_cov):
    """Build the per-label extended-association weight table `u`, and alongside it
    everything the spatial update needs (per label, per component: the Kalman gain, the
    updated covariance, and the likelihood of each detection)."""
    u = []
    prepared = []
    for _label, r, components in bernoullis:
        per_component = []
        for w, m, p in components:
            s = h @ p @ h.T + r_cov
            k = p @ h.T @ np.linalg.inv(s)
            i_kh = np.eye(N) - k @ h
            cov = i_kh @ p @ i_kh.T + k @ r_cov @ k.T
            cov = (cov + cov.T) / 2.0
            liks = [gaussian_pdf(np.asarray(z), h @ m, s) for z in detections]
            # The PRIOR covariance `p` is carried alongside the UPDATED one `cov`: the
            # missed-detection branch keeps the prior, every detection branch takes the
            # updated one, and conflating the two is the easiest way to get this filter
            # quietly wrong (it would look right on cardinality and be wrong on spread).
            per_component.append((w, m, p, k, cov, liks))
        q = [
            sum(w * liks[j] for (w, _m, _p, _k, _cov, liks) in per_component)
            for j in range(len(detections))
        ]
        row = [1.0 - r, r * (1.0 - pD)]
        row.extend(r * pD * qj / kappa for qj in q)
        u.append(row)
        prepared.append((per_component, q))
    return u, prepared


def lmb_update(
    bernoullis,
    detections,
    pD,
    kappa,
    h,
    r_cov,
    marginals_fn=marginals_enumerate,
    merge_distance=MERGE,
    max_components=MAX_COMPONENTS,
    existence_prune=EXISTENCE_PRUNE,
    spatial_prune=SPATIAL_PRUNE,
):
    """The exact delta-GLMB update of an LMB prior, projected back onto an LMB.

    Returns (bernoullis, marginals, mass_check) -- `mass_check` is the largest gap
    between a label's updated spatial mixture mass and its updated existence probability,
    which check (5) requires to be zero.
    """
    if not bernoullis:
        return [], [], 0.0
    u, prepared = association_weights(bernoullis, detections, pD, kappa, h, r_cov)
    marginals, _total = marginals_fn(u)

    out = []
    mass_check = 0.0
    for index, (label, _r, _components) in enumerate(bernoullis):
        per_component, q = prepared[index]
        prob = marginals[index]
        r_hat = 1.0 - prob[ABSENT]

        # The missed-detection branch: the target is there and was not seen, so its
        # spatial density is unchanged -- prior mean AND prior covariance.
        mixture = []
        if prob[MISSED] > 0.0:
            mixture.extend(
                (prob[MISSED] * w, m, p) for (w, m, p, _k, _cov, _liks) in per_component
            )
        # One branch per detection: the Kalman update of each component by that
        # detection, weighted by how much of this label's predictive likelihood for that
        # detection the component accounts for.
        for j in range(len(detections)):
            p_j = prob[FIRST_DETECTION + j]
            if p_j <= 0.0 or q[j] <= 0.0:
                continue
            z = np.asarray(detections[j])
            for w, m, _p, k, cov, liks in per_component:
                share = w * liks[j] / q[j]
                if share <= 0.0:
                    continue
                mixture.append((p_j * share, m + k @ (z - h @ m), cov))
        if not mixture or r_hat <= existence_prune:
            continue
        mass_check = max(mass_check, abs(sum(c[0] for c in mixture) - r_hat))
        components = prune_and_merge(mixture, merge_distance, max_components, spatial_prune * r_hat)
        out.append((label, r_hat, components))
    return out, marginals, mass_check


def single_target_bernoulli(r, q_vals, pD, kappa):
    """The textbook single-target Bernoulli-filter existence update, written out
    independently of everything above for check (2).

        r_hat = r(1 - pD + pD*sum_j q_j/kappa) / (1 - r*pD + r*pD*sum_j q_j/kappa)
    """
    ratio = sum(q / kappa for q in q_vals)
    numerator = r * (1.0 - pD + pD * ratio)
    denominator = 1.0 - r * pD + r * pD * ratio
    return numerator / denominator


# --------------------------------------------------------------------------------
# The delta-GLMB, run in full so the cost of the LMB projection can be MEASURED.
# --------------------------------------------------------------------------------
#
# A hypothesis is (weight, {label: (mean, cov)}). Labels absent from the dict do not
# exist in that hypothesis. Nothing is merged or moment-matched; the only truncation is
# an explicit weight floor whose discarded mass is returned and asserted negligible.

GLMB_FLOOR = 1e-13


def glmb_from_lmb(bernoullis):
    """Expand an LMB into the equivalent delta-GLMB: every subset of the labels, weighted
    by the product of r and 1-r. Exact, not an approximation -- an LMB IS a delta-GLMB
    with this hypothesis set."""
    hypotheses = [(1.0, {})]
    for label, r, components in bernoullis:
        if len(components) != 1:
            raise ValueError("the delta-GLMB comparison uses single-Gaussian Bernoullis")
        _w, m, p = components[0]
        grown = []
        for weight, live in hypotheses:
            grown.append((weight * (1.0 - r), dict(live)))
            with_label = dict(live)
            with_label[label] = (m, p)
            grown.append((weight * r, with_label))
        hypotheses = grown
    return hypotheses


def glmb_predict(hypotheses, p_survival, dt, sigma_a_sq):
    f = cv_f(dt)
    q = cv_q(dt, sigma_a_sq)
    out = []
    for weight, live in hypotheses:
        labels = sorted(live)
        for survivors in itertools.chain.from_iterable(
            itertools.combinations(labels, k) for k in range(len(labels) + 1)
        ):
            w = weight
            w *= p_survival ** len(survivors)
            w *= (1.0 - p_survival) ** (len(labels) - len(survivors))
            if w <= 0.0:
                continue
            moved = {}
            for label in survivors:
                m, p = live[label]
                pred = f @ p @ f.T + q
                moved[label] = (f @ m, (pred + pred.T) / 2.0)
            out.append((w, moved))
    return out


def glmb_update(hypotheses, detections, pD, kappa, h, r_cov):
    out = []
    for weight, live in hypotheses:
        labels = sorted(live)
        prepared = {}
        for label in labels:
            m, p = live[label]
            s = h @ p @ h.T + r_cov
            k = p @ h.T @ np.linalg.inv(s)
            i_kh = np.eye(N) - k @ h
            cov = i_kh @ p @ i_kh.T + k @ r_cov @ k.T
            prepared[label] = (m, k, (cov + cov.T) / 2.0, s)
        # Every map from these labels to {missed} u {detections}, injective on detections.
        for assignment in itertools.product(range(1 + len(detections)), repeat=len(labels)):
            claimed = [a for a in assignment if a > 0]
            if len(set(claimed)) != len(claimed):
                continue
            w = weight
            posterior = {}
            for label, a in zip(labels, assignment):
                m, k, cov, s = prepared[label]
                if a == 0:
                    w *= 1.0 - pD
                    posterior[label] = live[label]
                else:
                    z = np.asarray(detections[a - 1])
                    w *= pD * gaussian_pdf(z, h @ m, s) / kappa
                    posterior[label] = (m + k @ (z - h @ m), cov)
                if w == 0.0:
                    break
            if w > 0.0:
                out.append((w, posterior))
    total = sum(w for w, _ in out)
    if total <= 0.0:
        raise ValueError("the delta-GLMB posterior has no mass")
    out = [(w / total, live) for w, live in out]
    kept = [(w, live) for w, live in out if w >= GLMB_FLOOR]
    discarded = 1.0 - sum(w for w, _ in kept)
    renorm = sum(w for w, _ in kept)
    return [(w / renorm, live) for w, live in kept], discarded


def glmb_existence(hypotheses, label):
    return sum(w for w, live in hypotheses if label in live)


def delta_glmb_divergence(scans, bernoullis, pD, kappa, h, r_cov, p_survival):
    """Run the LMB and a full untruncated delta-GLMB from the same prior over the same
    detections. Returns (per-scan max existence disagreement, worst discarded mass)."""
    lmb = list(bernoullis)
    glmb = glmb_from_lmb(bernoullis)
    labels = [b[0] for b in bernoullis]
    per_scan = []
    worst_discard = 0.0
    for scan, detections in enumerate(scans):
        if scan > 0:
            lmb = lmb_predict(lmb, [], p_survival, DT, SIGMA_A_SQ)
            glmb = glmb_predict(glmb, p_survival, DT, SIGMA_A_SQ)
        lmb, _marginals, _mass = lmb_update(
            lmb, detections, pD, kappa, h, r_cov, existence_prune=0.0
        )
        glmb, discarded = glmb_update(glmb, detections, pD, kappa, h, r_cov)
        worst_discard = max(worst_discard, discarded)
        lmb_r = {label: r for label, r, _c in lmb}
        gap = max(
            abs(lmb_r.get(label, 0.0) - glmb_existence(glmb, label)) for label in labels
        )
        per_scan.append(gap)
    return per_scan, worst_discard


# --------------------------------------------------------------------------------
# The checks.
# --------------------------------------------------------------------------------


def _random_u(rng, n, m):
    u = []
    for _ in range(n):
        r = float(rng.uniform(0.05, 0.95))
        row = [1.0 - r, r * float(rng.uniform(0.02, 0.5))]
        row.extend(r * float(rng.uniform(1e-3, 50.0)) for _ in range(m))
        u.append(row)
    return u


def _self_check():
    rng = np.random.default_rng(20260908)
    h1, r1 = np.eye(1), np.eye(1)

    # (1) three implementations of the marginals.
    worst_marginal = 0.0
    worst_normaliser = 0.0
    for _ in range(60):
        n = int(rng.integers(1, 6))
        m = int(rng.integers(0, 5))
        u = _random_u(rng, n, m)
        enum_marginals, enum_total = marginals_enumerate(u)
        dp_marginals, dp_total = marginals_dp(u)
        transposed = normaliser_transposed(u)
        for a, b in zip(enum_marginals, dp_marginals):
            for x, y in zip(a, b):
                worst_marginal = max(worst_marginal, abs(x - y) / max(abs(x), 1e-12))
        worst_normaliser = max(
            worst_normaliser,
            abs(enum_total - dp_total) / max(abs(enum_total), 1e-12),
            abs(enum_total - transposed) / max(abs(enum_total), 1e-12),
        )
        if worst_marginal > 1e-9 or worst_normaliser > 1e-9:
            raise AssertionError(
                f"the three marginal implementations disagree: marginals {worst_marginal}, "
                f"normaliser {worst_normaliser} (n={n}, m={m})"
            )

    # (4) the marginals are a distribution and no detection is double-spent.
    for _ in range(40):
        n = int(rng.integers(1, 6))
        m = int(rng.integers(0, 5))
        u = _random_u(rng, n, m)
        marginals, _ = marginals_enumerate(u)
        for row in marginals:
            if abs(sum(row) - 1.0) > 1e-12:
                raise AssertionError(f"a label's association marginals sum to {sum(row)}")
        for j in range(m):
            claimed = sum(row[FIRST_DETECTION + j] for row in marginals)
            if claimed > 1.0 + 1e-12:
                raise AssertionError(
                    f"detection {j} is claimed with total probability {claimed} > 1, so "
                    "the injectivity constraint is not being applied"
                )

    # (2) one label reduces exactly to the single-target Bernoulli filter.
    worst_bernoulli = 0.0
    for _ in range(40):
        r = float(rng.uniform(0.05, 0.95))
        m = int(rng.integers(0, 4))
        kappa = float(rng.uniform(0.01, 2.0))
        pD = float(rng.uniform(0.3, 0.99))
        means = rng.uniform(-4.0, 4.0, size=1)
        cov = np.eye(1) * float(rng.uniform(0.5, 3.0))
        detections = [rng.uniform(-6.0, 6.0, size=1) for _ in range(m)]
        s = cov + r1
        q_vals = [gaussian_pdf(z, means, s) for z in detections]
        closed = single_target_bernoulli(r, q_vals, pD, kappa)
        u = [[1.0 - r, r * (1.0 - pD)] + [r * pD * q / kappa for q in q_vals]]
        marginals, _ = marginals_enumerate(u)
        got = 1.0 - marginals[0][ABSENT]
        worst_bernoulli = max(worst_bernoulli, abs(got - closed) / max(abs(closed), 1e-12))
    if worst_bernoulli > 1e-12:
        raise AssertionError(
            f"a one-label LMB update does not equal the single-target Bernoulli filter: "
            f"worst relative error {worst_bernoulli}"
        )

    # (3) labels that do not compete factorise exactly.
    worst_factorisation = 0.0
    for _ in range(20):
        n = int(rng.integers(2, 5))
        pD = float(rng.uniform(0.4, 0.95))
        kappa = float(rng.uniform(0.05, 1.0))
        rs = [float(rng.uniform(0.1, 0.9)) for _ in range(n)]
        # Each label gets its own detection and is effectively blind to the others.
        u = []
        for i in range(n):
            row = [1.0 - rs[i], rs[i] * (1.0 - pD)]
            for j in range(n):
                q = float(rng.uniform(1.0, 20.0)) if i == j else 1e-14
                row.append(rs[i] * pD * q / kappa)
            u.append(row)
        marginals, _ = marginals_enumerate(u)
        for i in range(n):
            solo = [[u[i][ABSENT], u[i][MISSED], u[i][FIRST_DETECTION + i]]]
            solo_marginals, _ = marginals_enumerate(solo)
            got = 1.0 - marginals[i][ABSENT]
            want = 1.0 - solo_marginals[0][ABSENT]
            worst_factorisation = max(
                worst_factorisation, abs(got - want) / max(abs(want), 1e-12)
            )
    if worst_factorisation > 1e-9:
        raise AssertionError(
            "labels with disjoint detections do not factorise into independent "
            f"single-target updates: worst relative error {worst_factorisation}"
        )

    # (5) the spatial mixture's mass equals the existence probability, and
    # (the exactness half of the divergence measurement) the first update from an LMB
    # prior matches the full delta-GLMB exactly.
    h, r_cov = position_h(), np.diag(R_DIAG)
    prior = [
        (0, 0.55, [(1.0, np.array([0.0, 0.0, 100.0, 0.0, 0.0, 0.0]), np.diag(BIRTH_COV))]),
        (1, 0.45, [(1.0, np.array([40.0, 0.0, 100.0, 0.0, 0.0, 0.0]), np.diag(BIRTH_COV))]),
    ]
    scans = [
        [np.array([1.0, 0.5, 100.0]), np.array([41.0, -0.5, 100.0])],
        [np.array([0.0, 1.0, 100.5]), np.array([39.0, 0.0, 99.0])],
        [np.array([1.5, 0.0, 100.0]), np.array([40.5, 1.0, 100.0])],
    ]
    _updated, _marginals, mass_gap = lmb_update(
        prior, scans[0], PROB_DETECT, CLUTTER, h, r_cov, existence_prune=0.0
    )
    if mass_gap > 1e-12:
        raise AssertionError(
            f"a label's spatial mixture mass differs from its existence probability by "
            f"{mass_gap}; the two are the same posterior's mass computed two ways"
        )
    divergence, discarded = delta_glmb_divergence(
        scans, prior, PROB_DETECT, CLUTTER, h, r_cov, PROB_SURVIVAL
    )
    if divergence[0] > 1e-12:
        raise AssertionError(
            "the FIRST update from an LMB prior must equal the full delta-GLMB exactly "
            f"in every label's existence probability, and differs by {divergence[0]}; "
            "the moment-matched projection is wrong"
        )
    if discarded > 1e-9:
        raise AssertionError(
            f"the delta-GLMB comparison truncated {discarded} of its probability mass, "
            "so it is not the untruncated reference it claims to be"
        )
    return {
        "marginals_three_ways_worst_relative_error": worst_marginal,
        "normaliser_three_ways_worst_relative_error": worst_normaliser,
        "single_target_bernoulli_worst_relative_error": worst_bernoulli,
        "disjoint_labels_factorise_worst_relative_error": worst_factorisation,
        "spatial_mass_equals_existence_worst_absolute_error": mass_gap,
        "delta_glmb_existence_gap_per_scan": divergence,
        "delta_glmb_truncated_mass": discarded,
    }


CHECKS = _self_check()
STONESOUP_STATUS = stonesoup_status()


# --------------------------------------------------------------------------------
# The fixture.
# --------------------------------------------------------------------------------


def birth(label, position, velocity=(0.0, 0.0, 0.0)):
    mean = np.array(list(position) + list(velocity), dtype=float)
    return (label, BIRTH_EXISTENCE, [(1.0, mean, np.diag(BIRTH_COV))])


def build_case(name, scans, births_per_scan, probes, note):
    """`scans` is one list of 3-vectors per scan (targets and clutter alike, in the order
    the filter will see them); `births_per_scan` is one list of Bernoullis per scan.

    Labels are integers allocated by the CALLER, in the order births are supplied, and
    the Rust side allocates them from the same monotone counter starting at 0 -- so a
    label mismatch between the two is a real disagreement about identity, not a naming
    convention.
    """
    h, r_cov = position_h(), np.diag(R_DIAG)
    bernoullis = []
    per_scan = []
    for scan, detections in enumerate(scans):
        bernoullis = lmb_predict(
            bernoullis, births_per_scan[scan], PROB_SURVIVAL, DT, SIGMA_A_SQ
        )
        priors = [int(label) for label, _r, _c in bernoullis]
        bernoullis, marginals, mass_gap = lmb_update(
            bernoullis, detections, PROB_DETECT, CLUTTER, h, r_cov
        )
        if mass_gap > 1e-9:
            raise AssertionError(f"{name} scan {scan}: spatial mass gap {mass_gap}")
        bernoullis.sort(key=lambda b: b[0])
        per_scan.append(
            {
                "labels": [int(label) for label, _r, _c in bernoullis],
                "existence": [float(r) for _label, r, _c in bernoullis],
                # The label-to-detection assignment probabilities themselves, for the
                # labels present BEFORE the update (a label can be pruned by it). Index 0
                # is "does not exist", index 1 "exists and was not detected", index 2+j
                # "produced detection j". This is the "label-to-track assignment" half of
                # the row's criterion, compared directly rather than inferred from where
                # the means ended up.
                "association_labels": priors,
                "association_marginals": [[float(v) for v in row] for row in marginals],
                "mean": [
                    [float(v) for v in sum(w * m for w, m, _p in components)]
                    for _label, _r, components in bernoullis
                ],
                "density_at_probes": [
                    [density_at(components, p) for p in probes]
                    for _label, _r, components in bernoullis
                ],
            }
        )
    return {
        "name": name,
        "note": note,
        "scan_count": len(scans),
        "detections": [[[float(v) for v in z] for z in scan] for scan in scans],
        "births": [
            [
                {
                    "label": int(label),
                    "existence": float(r),
                    "mean": [float(v) for v in components[0][1]],
                }
                for label, r, components in births
            ]
            for births in births_per_scan
        ],
        "probes": [list(p) for p in probes],
        "per_scan": per_scan,
    }


def static_case(name, truth, scan_count, clutter=(), note=""):
    scans = []
    for _scan in range(scan_count):
        scans.append([np.array(p, dtype=float) for p in truth] + [np.array(c, dtype=float) for c in clutter])
    births = [[] for _ in range(scan_count)]
    births[0] = [birth(i, truth[i]) for i in range(len(truth))]
    probes = [list(p) for p in truth]
    probes.append([truth[0][0] + 1500.0, 0.0, 0.0])
    return build_case(name, scans, births, probes, note)


def crossing_case():
    """Two targets closing head-on and passing through EXACTLY THE SAME POINT at scan 8.

    A near miss is not actually a hard case for this filter: at a 30 m separation and a
    5 m measurement standard deviation the association is many sigma from ambiguous, and
    a test built on one would prove nothing about identity. Here the two targets occupy
    the same position at the same scan, the measurement model sees only position, and the
    two association hypotheses are therefore EXACTLY equally likely -- the marginals at
    scan 8 are a 50/50 tie, which is the honest answer and is recorded as such.

    What the filter has that the tie does not destroy is velocity: the two Bernoullis
    carry opposite velocities through the tie, and the scan after it the marginals snap
    back to certainty with each label still on the target it started on. That is what
    "label continuity" has to mean to be worth anything, and it is why this is the case
    the row is gated on rather than a well-separated one.
    """
    scan_count = 17
    speed = 20.0
    scans, births = [], [[] for _ in range(scan_count)]
    for scan in range(scan_count):
        t = float(scan)
        scans.append(
            [
                np.array([-160.0 + speed * t, 0.0, 100.0]),
                np.array([160.0 - speed * t, 0.0, 100.0]),
            ]
        )
    births[0] = [
        (0, BIRTH_EXISTENCE, [(1.0, np.array([-160.0, 0.0, 100.0, speed, 0.0, 0.0]), np.diag(BIRTH_COV))]),
        (1, BIRTH_EXISTENCE, [(1.0, np.array([160.0, 0.0, 100.0, -speed, 0.0, 0.0]), np.diag(BIRTH_COV))]),
    ]
    probes = [[-160.0, 0.0, 100.0], [0.0, 0.0, 100.0], [160.0, 0.0, 100.0]]
    return build_case(
        "two_targets_crossing_coincident",
        scans,
        births,
        probes,
        "label 0 runs east from x=-160, label 1 west from x=+160, both at y=0, so they "
        "occupy the same point at scan 8 and the association is an exact tie there",
    )


def new_target_case():
    """Two targets from the start; a third appears at scan 6 and is born then.

    The other half of what a labelled filter promises: a genuinely new target must get a
    label that has never been used, not inherit one. Label 2 does not exist before scan 6
    and labels 0 and 1 do not change when it arrives.
    """
    scan_count = 14
    born_at = 6
    old = [[0.0, 0.0, 100.0], [250.0, 0.0, 100.0]]
    fresh = [0.0, 350.0, 100.0]
    scans, births = [], [[] for _ in range(scan_count)]
    for scan in range(scan_count):
        seen = [np.array(p, dtype=float) for p in old]
        if scan >= born_at:
            seen.append(np.array(fresh, dtype=float))
        scans.append(seen)
    births[0] = [birth(i, old[i]) for i in range(2)]
    births[born_at] = [birth(2, fresh)]
    probes = [list(p) for p in old] + [list(fresh)]
    return build_case(
        "a_third_target_appears_midway",
        scans,
        births,
        probes,
        "labels 0 and 1 run from scan 0; label 2 is born at scan 6 and must be a label "
        "never previously used",
    )


def intermittent_case():
    """Three targets, the middle one undetected on every odd scan, plus a clutter return
    30 m from the first target -- about 2.7 sigma at this covariance, so it is genuinely
    ambiguous rather than trivially rejected."""
    truth = [[0.0, 0.0, 100.0], [200.0, 0.0, 100.0], [0.0, 300.0, 100.0]]
    scan_count = 18
    scans, births = [], [[] for _ in range(scan_count)]
    for scan in range(scan_count):
        seen = [truth[0], truth[2]] if scan % 2 else list(truth)
        seen = seen + [[30.0, 0.0, 100.0]]
        scans.append([np.array(p, dtype=float) for p in seen])
    births[0] = [birth(i, truth[i]) for i in range(3)]
    probes = [list(p) for p in truth] + [[1500.0, 0.0, 0.0]]
    return build_case(
        "three_targets_one_intermittent_with_clutter",
        scans,
        births,
        probes,
        "target 1 is undetected on every odd scan; a clutter return sits 30 m from "
        "target 0 on every scan",
    )


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    cases = [
        static_case("one_target", [[0.0, 0.0, 100.0]], 12, note="the degenerate case: no competition"),
        static_case(
            "three_separated_targets",
            [[0.0, 0.0, 100.0], [300.0, 0.0, 100.0], [0.0, 400.0, 100.0]],
            14,
            note="three targets far enough apart that the association is unambiguous",
        ),
        intermittent_case(),
        crossing_case(),
        new_target_case(),
    ]
    payload = {
        "oracle": (
            "the labelled multi-Bernoulli filter (Reuter, Vo, Vo and Dietmayer 2014): the "
            "exact delta-GLMB update of an LMB prior, projected back onto an LMB by "
            "moment matching, with the association marginals produced by LITERAL "
            "enumeration of every association event -- see the module docstring for the "
            "derivation and for the five independent checks run before this file was "
            "written"
        ),
        "filter_built": "LMB (labelled multi-Bernoulli), not the full delta-GLMB",
        "what_is_approximated": (
            "the single-scan update is exact in every per-label marginal; the projection "
            "back onto an LMB discards the inter-label dependence of the delta-GLMB "
            "posterior, so the error is what the NEXT scan inherits. Measured, not "
            "asserted: delta_glmb_existence_gap_per_scan"
        ),
        "stonesoup": "1.9.1",
        "stonesoup_status": STONESOUP_STATUS,
        "checks": CHECKS,
        "settings": {
            "probability_of_survival": PROB_SURVIVAL,
            "probability_of_detection": PROB_DETECT,
            "clutter_density": CLUTTER,
            "merge_distance": MERGE,
            "max_components": MAX_COMPONENTS,
            "existence_prune_threshold": EXISTENCE_PRUNE,
            "spatial_prune_threshold": SPATIAL_PRUNE,
            "sigma_a_sq": SIGMA_A_SQ,
            "dt": DT,
            "r_diag": R_DIAG,
            "birth_cov_diag": BIRTH_COV,
            "birth_existence": BIRTH_EXISTENCE,
        },
        "method_note": (
            "Per label: the existence probability, and the spatial density as a FUNCTION "
            "at fixed probe points -- never the component list, for the same reason the "
            "PHD and CPHD rows give. The LABEL SET is compared exactly, because label "
            "identity is the entire reason this row is separate from the PHD/CPHD one."
        ),
        "generated": START.isoformat(),
        "cases": cases,
    }
    (OUT / "lmb.json").write_text(json.dumps(payload, indent=1), encoding="utf-8")
    print("stonesoup:", STONESOUP_STATUS)
    for key, value in CHECKS.items():
        print(f"  {key}: {value}")
    for case in cases:
        last = case["per_scan"][-1]
        print(case["name"], "final labels", last["labels"], "existence",
              [round(r, 6) for r in last["existence"]])


if __name__ == "__main__":
    main()
