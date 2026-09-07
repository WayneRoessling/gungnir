"""Oracle fixture for the verification-capability-table.md row
"association | JPDA" -> association/jpda.json

Run inside .venv-oracles:

    ../../../.venv-oracles/Scripts/python.exe gen_jpda_fixtures.py

Oracle: Stone Soup 1.9.1's JPDA data associator over its PDAHypothesiser, driven
directly rather than reimplemented here. Criterion: relative error < 1e-3 on the
per-track association probabilities.

HOW THE ORACLE IS PINNED TO A CHOSEN S. Stone Soup's hypothesiser predicts the track
forward and then predicts a measurement from it, so the innovation covariance is
something it computes rather than something a caller supplies. To compare against a Rust
function whose input IS the innovation covariance, every prediction is taken at the
track's own timestamp -- dt = 0, so F is the identity and Q is zero and the prediction is
the track state unchanged -- and the measurement model is the identity. Then
S = H P H' + R = P + R exactly, and choosing P = (var - 1) I with R = I gives S = var I,
the matrix the Rust side is handed. The recorded fixture carries `var` and the Rust test
builds S from it, so both sides are working from the same number rather than from two
numbers that happen to agree.

Stone Soup's JPDA normalises the per-track hypotheses at the end, so the recorded
probabilities sum to one per track, which is what the Rust side produces too.
"""

import datetime
import json
import pathlib

import numpy as np
from stonesoup.dataassociator.probability import JPDA
from stonesoup.hypothesiser.probability import PDAHypothesiser
from stonesoup.models.measurement.linear import LinearGaussian
from stonesoup.models.transition.linear import (
    CombinedLinearGaussianTransitionModel,
    ConstantVelocity,
)
from stonesoup.predictor.kalman import KalmanPredictor
from stonesoup.types.detection import Detection
from stonesoup.types.state import GaussianState
from stonesoup.types.track import Track
from stonesoup.updater.kalman import KalmanUpdater

OUT = pathlib.Path(__file__).resolve().parent.parent / "association"
NOW = datetime.datetime(2026, 9, 6, 0, 0, 0)
DIM = 2


def build_associator(prob_detect, prob_gate, clutter_density):
    # A transition model is required but never does anything: every prediction is taken
    # at dt = 0. ConstantVelocity here is Stone Soup's own 2-state (position, velocity)
    # model, and the state vector below is 2-dimensional position only, so the model is
    # over the two "position" components and the zero timestep keeps it inert.
    transition = CombinedLinearGaussianTransitionModel([ConstantVelocity(1e-9)] * 1)
    measurement = LinearGaussian(ndim_state=DIM, mapping=(0, 1), noise_covar=np.eye(DIM))
    hypothesiser = PDAHypothesiser(
        predictor=KalmanPredictor(transition),
        updater=KalmanUpdater(measurement),
        clutter_spatial_density=clutter_density,
        prob_detect=prob_detect,
        prob_gate=prob_gate,
    )
    return JPDA(hypothesiser=hypothesiser), measurement


def case(name, prob_detect, prob_gate, clutter_density, track_specs, detection_points):
    """track_specs: [(east, north, var)]. detection_points: [(east, north)]."""
    associator, measurement = build_associator(prob_detect, prob_gate, clutter_density)

    tracks = set()
    ordered_tracks = []
    for east, north, var in track_specs:
        # S = P + R with R = I, so P = (var - 1) I.
        covar = np.eye(DIM) * (var - 1.0)
        state = GaussianState(np.array([[east], [north]]), covar, timestamp=NOW)
        track = Track([state])
        tracks.add(track)
        ordered_tracks.append(track)

    detections = set()
    ordered_detections = []
    for east, north in detection_points:
        d = Detection(
            np.array([[east], [north]]), timestamp=NOW, measurement_model=measurement
        )
        detections.add(d)
        ordered_detections.append(d)

    result = associator.associate(tracks, detections, NOW)

    rows = []
    for track in ordered_tracks:
        multi = result[track]
        missed = 0.0
        per_detection = [0.0] * len(ordered_detections)
        for hypothesis in multi:
            probability = float(hypothesis.probability)
            if not hypothesis:
                missed += probability
                continue
            index = next(
                i for i, d in enumerate(ordered_detections) if d is hypothesis.measurement
            )
            per_detection[index] += probability
        total = missed + sum(per_detection)
        assert abs(total - 1.0) < 1e-9, f"{name}: probabilities summed to {total}"
        rows.append({"missed": missed, "detections": per_detection})

    return {
        "name": name,
        "probability_of_detection": prob_detect,
        "probability_of_gate": prob_gate,
        "clutter_density": clutter_density,
        "tracks": [
            {"east": e, "north": n, "innovation_variance": v} for e, n, v in track_specs
        ],
        "detections": [list(p) for p in detection_points],
        "expected": rows,
    }


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    cases = [
        case(
            "one_track_one_clean_detection",
            0.9,
            0.99,
            1e-6,
            [(0.0, 0.0, 25.0)],
            [(1.0, 1.0)],
        ),
        case(
            "symmetric_crossing_two_tracks_two_detections",
            0.9,
            0.99,
            1e-6,
            [(-10.0, 0.0, 100.0), (10.0, 0.0, 100.0)],
            [(0.0, 5.0), (0.0, -5.0)],
        ),
        case(
            "two_tracks_contending_for_one_detection",
            0.9,
            0.99,
            1e-6,
            [(-3.0, 0.0, 100.0), (3.0, 0.0, 100.0)],
            [(0.0, 0.0)],
        ),
        case(
            "three_tracks_in_clutter",
            0.85,
            0.99,
            1e-4,
            [(0.0, 0.0, 64.0), (12.0, 0.0, 64.0), (6.0, 10.0, 64.0)],
            [(1.0, 0.5), (11.0, -1.0), (6.0, 9.0), (5.0, 3.0)],
        ),
        case(
            "heavy_clutter_makes_every_association_less_certain",
            0.85,
            0.99,
            1e-2,
            [(0.0, 0.0, 64.0), (12.0, 0.0, 64.0)],
            [(1.0, 0.5), (11.0, -1.0)],
        ),
        case(
            "a_track_with_nothing_in_its_gate_was_missed",
            0.9,
            0.99,
            1e-6,
            [(0.0, 0.0, 4.0), (400.0, 400.0, 4.0)],
            [(0.5, 0.5)],
        ),
    ]
    payload = {
        "oracle": "stonesoup.dataassociator.probability.JPDA over PDAHypothesiser",
        "stonesoup": "1.9.1",
        "setup_note": (
            "Every prediction is taken at the track's own timestamp, so dt = 0 and the "
            "prediction is the track state unchanged; the measurement model is the "
            "identity with R = I. Then S = P + R exactly, and P = (var - 1) I gives "
            "S = var I, the innovation covariance the Rust side is handed."
        ),
        "cases": cases,
    }
    (OUT / "jpda.json").write_text(json.dumps(payload, indent=1), encoding="utf-8")
    print("jpda.json:", len(cases), "cases")


if __name__ == "__main__":
    main()
