# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Oracle fixture for the verification-capability-table.md row
"rfs | PHD / CPHD filter" -> track/phd.json

Run inside .venv-oracles:

    ../../../.venv-oracles/Scripts/python.exe gen_phd_fixtures.py

ORACLE: the textbook Gaussian-mixture PHD recursion (Vo and Ma, "The Gaussian Mixture
Probability Hypothesis Density Filter", IEEE TSP 54(11), 2006), written out in numpy in
`reference_gm_phd` below. Criterion, unchanged from section 2: intensity weights within
1e-3, and exact cardinality where unambiguous.

SECTION 2 NAMED STONE SOUP GM-PHD, AND STONE SOUP 1.9.1 DISAGREES. The library was
driven for real -- `stonesoup_gm_phd` below is not a sketch -- and its cardinality
differs from the textbook recursion by several percent per target. The row is therefore
gated against the hand-written recursion, which section 2's Oracle column already
permits and uses for two other rows, and the disagreement is recorded per case rather
than tuned away.

ONE CAUSE IS CONFIRMED AND REPRODUCIBLE. `GaussianMixtureReducer.merge_components` ends
with:

    weight_sum = component_1.weight + component_2.weight
    ...
    if weight_sum > 1:
        weight_sum = 1

Merging components of weight 0.7 and 0.6 returns a component of weight 1.0, checked
directly. That is wrong for a PHD intensity: a PHD weight is an EXPECTED NUMBER OF
TARGETS, not a probability, and a component representing two unresolved targets has
weight 2 by definition. The clamp makes the library under-report cardinality exactly when
merging combines past one target, which is why the disagreement grows with the number of
targets in the scene.

A SECOND DIFFERENCE IS NOT YET EXPLAINED. The two also disagree on the first scan, before
any weight approaches one. That is stated here because it is not understood, and a
fixture that recorded only the cause that was found would imply the rest had been ruled
out.

WHAT IS AND IS NOT COMPARED. Section 2's method is "same birth/clutter/detection model,
compare intensity function + cardinality". The intensity function is a mixture, and two
implementations that prune and merge in a different ORDER end up with the same intensity
carried by a different number of components -- the function is the same, its
decomposition is not. So the comparison is on quantities that are properties of the
intensity itself and not of its decomposition:

  * the cardinality, the integral of the intensity, which is the sum of the weights; and
  * the intensity evaluated at a set of fixed probe points spanning the scene, which is
    the function itself sampled rather than its parameterisation.

Comparing component lists directly would fail on two correct filters, and loosening the
tolerance until it passed would be widening a pass criterion to make a test pass.
"""

import datetime
import json
import pathlib

import numpy as np

OUT = pathlib.Path(__file__).resolve().parent.parent / "track"
START = datetime.datetime(2026, 9, 6, 0, 0, 0)
N = 6
M = 3

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
    """gungnir_core::ConstantVelocity::q -- the CONTINUOUS form."""
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
    """The mixture evaluated at one 3-D position, marginalised over velocity.

    Marginalising a Gaussian over some components is just dropping them, so the position
    marginal of each component is its top-left 3x3 block. This is the intensity as a
    FUNCTION, which is what the two filters must agree on; how many components carry it
    is not part of the claim.
    """
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


def reference_gm_phd(scans, births_per_scan):
    """The GM-PHD recursion, in numpy, with the same prune/merge rules the Rust uses.

    This is the hand-written half. `stonesoup_gm_phd` below is the library half, and the
    two are compared against each other before either is written out -- a fixture whose
    two oracles disagree is not a fixture.
    """
    f = cv_f(DT)
    q = cv_q(DT, SIGMA_A_SQ)
    h = position_h()
    r = np.diag(R_DIAG)
    components = []

    per_scan = []
    for detections, births in zip(scans, births_per_scan):
        # Predict.
        components = [
            (w * PROB_SURVIVAL, f @ m, f @ p @ f.T + q) for (w, m, p) in components
        ]
        components.extend(births)

        # Update.
        updated = [(w * (1.0 - PROB_DETECT), m, p) for (w, m, p) in components]
        prepared = []
        for (w, m, p) in components:
            pht = p @ h.T
            s = h @ pht + r
            k = pht @ np.linalg.inv(s)
            i_kh = np.eye(N) - k @ h
            cov = i_kh @ p @ i_kh.T + k @ r @ k.T
            prepared.append((w, m, p, k, s, cov))
        for z in detections:
            candidates = []
            total = CLUTTER
            for (w, m, _p, k, s, cov) in prepared:
                y = np.asarray(z) - h @ m
                det = np.linalg.det(s)
                likelihood = np.exp(-0.5 * y @ np.linalg.inv(s) @ y) / np.sqrt(
                    (2 * np.pi) ** M * det
                )
                weight = PROB_DETECT * w * likelihood
                total += weight
                candidates.append((weight, m + k @ y, cov))
            if total > 0:
                updated.extend([(w / total, m, p) for (w, m, p) in candidates])

        components = prune_and_merge(updated)
        per_scan.append(components)
    return per_scan


def prune_and_merge(components):
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


# Stone Soup's CombinedLinearGaussianTransitionModel over three ConstantVelocity models
# interleaves position and velocity per axis: [x, vx, y, vy, z, vz]. gungnir-core blocks
# them: [x, y, z, vx, vy, vz]. TO_STONESOUP[i] is where gungnir's component i lives in
# Stone Soup's vector.
#
# This is not a detail. The first version of this generator handed Stone Soup a
# gungnir-ordered state and read mapping=(0, 1, 2), which selects x, vx and y -- so the
# library was tracking a scene in which one axis was a velocity. The two filters
# disagreed on cardinality by more than a whole target, and the disagreement looked like
# a filter bug rather than a transcription one. Exactly the shape of the EKF fixture
# error recorded in gen_nonlinear_fixtures.py.
TO_STONESOUP = [0, 2, 4, 1, 3, 5]


def to_stonesoup_mean(mean):
    out = np.zeros(N)
    for i, j in enumerate(TO_STONESOUP):
        out[j] = mean[i]
    return out


def to_stonesoup_cov(cov):
    out = np.zeros((N, N))
    for i, a in enumerate(TO_STONESOUP):
        for j, b in enumerate(TO_STONESOUP):
            out[a, b] = cov[i, j]
    return out


def stonesoup_gm_phd(scans, births_per_scan):
    """The library half. Returns None with a recorded reason if it cannot be driven."""
    try:
        from stonesoup.hypothesiser.gaussianmixture import GaussianMixtureHypothesiser
        from stonesoup.hypothesiser.distance import DistanceHypothesiser
        from stonesoup.measures import Mahalanobis
        from stonesoup.mixturereducer.gaussianmixture import GaussianMixtureReducer
        from stonesoup.models.measurement.linear import LinearGaussian
        from stonesoup.models.transition.linear import (
            CombinedLinearGaussianTransitionModel,
            ConstantVelocity,
        )
        from stonesoup.predictor.kalman import KalmanPredictor
        from stonesoup.types.detection import Detection
        from stonesoup.types.state import TaggedWeightedGaussianState
        from stonesoup.updater.kalman import KalmanUpdater
        from stonesoup.updater.pointprocess import PHDUpdater
    except ImportError as exc:
        return {"available": False, "reason": str(exc)}

    try:
        transition = CombinedLinearGaussianTransitionModel([ConstantVelocity(SIGMA_A_SQ)] * 3)
        # mapping picks the POSITION components out of Stone Soup's interleaved order.
        measurement = LinearGaussian(ndim_state=N, mapping=(0, 2, 4), noise_covar=np.diag(R_DIAG))
        predictor = KalmanPredictor(transition)
        updater = KalmanUpdater(measurement)
        hypothesiser = GaussianMixtureHypothesiser(
            DistanceHypothesiser(predictor, updater, Mahalanobis(), missed_distance=1e6),
            order_by_detection=True,
        )
        phd = PHDUpdater(
            updater,
            clutter_spatial_density=CLUTTER,
            prob_detection=PROB_DETECT,
            prob_survival=PROB_SURVIVAL,
        )
        reducer = GaussianMixtureReducer(
            prune_threshold=PRUNE, merge_threshold=MERGE, max_number_components=MAX_COMPONENTS
        )

        state = []
        per_scan = []
        for index, (detections, births) in enumerate(zip(scans, births_per_scan)):
            timestamp = START + datetime.timedelta(seconds=DT * (index + 1))
            for tag, (w, m, p) in enumerate(births):
                state.append(
                    TaggedWeightedGaussianState(
                        to_stonesoup_mean(np.asarray(m)).reshape(-1, 1),
                        to_stonesoup_cov(np.asarray(p)),
                        weight=w,
                        tag=f"birth-{index}-{tag}",
                        timestamp=timestamp - datetime.timedelta(seconds=DT),
                    )
                )
            detection_set = {
                Detection(np.asarray(z).reshape(-1, 1), timestamp=timestamp,
                          measurement_model=measurement)
                for z in detections
            }
            hypotheses = hypothesiser.hypothesise(state, detection_set, timestamp)
            state = reducer.reduce(phd.update(hypotheses))
            per_scan.append(
                [
                    (
                        float(c.weight),
                        np.asarray(c.state_vector).reshape(-1).copy(),
                        np.asarray(c.covar).copy(),
                    )
                    for c in state
                ]
            )
        return {"available": True, "per_scan": per_scan}
    except Exception as exc:
        return {"available": False, "reason": f"{type(exc).__name__}: {exc}"}


def build_case(name, truth, scan_count, birth_scans):
    scans, births_per_scan = [], []
    for scan in range(scan_count):
        scans.append([list(p) for p in truth])
        if scan in birth_scans:
            births_per_scan.append(
                [
                    (0.4, np.array(list(p) + [0.0, 0.0, 0.0]), np.diag(BIRTH_COV))
                    for p in truth
                ]
            )
        else:
            births_per_scan.append([])

    reference = reference_gm_phd(scans, births_per_scan)
    library = stonesoup_gm_phd(scans, births_per_scan)

    probes = []
    for p in truth:
        probes.append(list(p))
    probes.append([sum(p[0] for p in truth) / len(truth) + 1500.0, 0.0, 0.0])

    per_scan = []
    for index, components in enumerate(reference):
        cardinality = float(sum(c[0] for c in components))
        per_scan.append(
            {
                "cardinality": cardinality,
                "intensity_at_probes": [intensity_at(components, p) for p in probes],
                "component_count": len(components),
            }
        )

    agreement = None
    if library.get("available"):
        worst = 0.0
        for mine, theirs in zip(reference, library["per_scan"]):
            worst = max(worst, abs(sum(c[0] for c in mine) - sum(c[0] for c in theirs)))
        agreement = float(worst)

    return {
        "name": name,
        "truth": [list(p) for p in truth],
        "scan_count": scan_count,
        "birth_scans": sorted(birth_scans),
        "probes": probes,
        "per_scan": per_scan,
        "stonesoup_cardinality_disagreement": agreement,
        "stonesoup_reason": library.get("reason"),
    }


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    cases = [
        build_case("three_separated_targets", [[0.0, 0.0, 100.0], [300.0, 0.0, 100.0],
                                               [0.0, 400.0, 100.0]], 20, {0}),
        build_case("one_target", [[0.0, 0.0, 100.0]], 15, {0}),
        build_case("six_targets_reborn_midway",
                   [[i * 150.0, 0.0, 100.0] for i in range(6)], 24, {0, 12}),
    ]
    payload = {
        "oracle": "the textbook Vo-Ma Gaussian-mixture PHD recursion, written out in numpy",
        "stonesoup": "1.9.1",
        "stonesoup_status": "disagrees; NOT a cross-check. See the module docstring.",
        "stonesoup_confirmed_defect": (
            "GaussianMixtureReducer.merge_components clamps a merged weight to 1.0 "
            "(checked directly: 0.7 + 0.6 merges to 1.0). A PHD weight is an expected "
            "target count, not a probability, so the clamp under-reports cardinality "
            "whenever merging combines past one target."
        ),
        "stonesoup_unexplained": (
            "The two also disagree on the first scan, before any weight approaches one. "
            "That difference has not been traced."
        ),
        "settings": {
            "probability_of_survival": PROB_SURVIVAL,
            "probability_of_detection": PROB_DETECT,
            "clutter_density": CLUTTER,
            "prune_threshold": PRUNE,
            "merge_distance": MERGE,
            "max_components": MAX_COMPONENTS,
            "sigma_a_sq": SIGMA_A_SQ,
            "dt": DT,
            "r_diag": R_DIAG,
            "birth_cov_diag": BIRTH_COV,
            "birth_weight": 0.4,
        },
        "method_note": (
            "The intensity is compared as a FUNCTION -- its integral, and its value at "
            "fixed probe points -- not as a component list. Two correct filters that "
            "prune and merge in a different order carry the same intensity in a "
            "different number of components."
        ),
        "cases": cases,
    }
    (OUT / "phd.json").write_text(json.dumps(payload, indent=1), encoding="utf-8")
    for case in cases:
        print(case["name"], "stonesoup disagreement:",
              case["stonesoup_cardinality_disagreement"], case["stonesoup_reason"] or "")


if __name__ == "__main__":
    main()
