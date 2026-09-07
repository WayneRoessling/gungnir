# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Generate the Stone Soup oracle fixture for the `track-manager` lifecycle row.

Row: "Track lifecycle (init/confirm/coast/delete)" in
`docs/verification-capability-table.md` §1. Pass criterion: **exact match on step
index** for an identical detection/miss sequence.

# How much of the oracle is actually driven, and how much is not

This distinction matters and is recorded rather than glossed:

* **The deleter is driven for real.** `stonesoup.deleter.time.UpdateTimeStepsDeleter`
  is instantiated and `check_for_deletion` is called against a genuine
  `stonesoup.types.track.Track` built from `GaussianStateUpdate` and
  `GaussianStatePrediction` states. The deletion step index below is Stone Soup's own
  answer, not a restatement of its documentation.

* **The confirmation rule is lifted from the initiator's source, not driven.**
  `MultiMeasurementInitiator.initiate` decides confirmation with

      sum(1 for state in track if not updates_only or isinstance(state, Update))
          >= min_points

  Running the whole initiator needs a predictor, updater, data associator and
  measurement model, which would put four more components between the fixture and the
  one rule under test -- a failure anywhere in that stack would look like a lifecycle
  disagreement. So the condition is evaluated verbatim against a real Stone Soup
  `Track` of real `Update` and `Prediction` states, using Stone Soup's own types. It
  is the initiator's rule applied to the initiator's data structures; it is not the
  initiator executed end to end, and the Rust test says so too.

Run from `testdata/oracles/` with the venv described in `../README.md`:

    python tools/gen_lifecycle_fixtures.py

Writes `track/lifecycle.json`.
"""

from __future__ import annotations

import datetime
import json
import platform
import sys
from pathlib import Path

import numpy as np
import stonesoup
from stonesoup.deleter.time import UpdateTimeStepsDeleter
from stonesoup.types.detection import Detection
from stonesoup.types.hypothesis import SingleHypothesis
from stonesoup.types.prediction import GaussianStatePrediction
from stonesoup.types.track import Track
from stonesoup.types.update import GaussianStateUpdate, Update

OUT = Path(__file__).resolve().parent.parent / "track" / "lifecycle.json"

T0 = datetime.datetime(2026, 1, 1)
COV = np.eye(2)
MEAN = np.array([[0.0], [0.0]])


def update_state(step: int) -> GaussianStateUpdate:
    """An update carrying a real hypothesis.

    The hypothesis is not decoration. `UpdateTimeStepsDeleter.check_for_deletion`
    tests `isinstance(state, Update) and state.hypothesis`, so an update built with
    `hypothesis=None` is **not counted as an update at all**: the deleter walks past
    it and deletes on distinct-timestamp count instead. An earlier draft of this
    generator did exactly that, and it reported a track deleted while it was being
    hit on every step. Building the hypothesis is what makes the oracle answer the
    question being asked.
    """
    timestamp = T0 + datetime.timedelta(seconds=step)
    prediction = GaussianStatePrediction(MEAN, COV, timestamp=timestamp)
    detection = Detection(np.array([[0.0]]), timestamp=timestamp)
    return GaussianStateUpdate(
        MEAN,
        COV,
        hypothesis=SingleHypothesis(prediction, detection),
        timestamp=timestamp,
    )


def prediction_state(step: int) -> GaussianStatePrediction:
    return GaussianStatePrediction(
        MEAN, COV, timestamp=T0 + datetime.timedelta(seconds=step)
    )


def run_case(name: str, sequence: str, min_points: int, time_steps: int) -> dict:
    """Replay a hit/miss string ('H' hit, 'M' miss) through the oracle.

    Returns the step index at which the track first satisfies the initiator's
    confirmation condition, and the step index at which the deleter first calls for
    deletion, or None where that never happens within the sequence.
    """
    deleter = UpdateTimeStepsDeleter(time_steps_since_update=time_steps)
    track: Track | None = None
    confirmed_at: int | None = None
    deleted_at: int | None = None
    per_step = []

    for step, symbol in enumerate(sequence):
        if symbol == "H":
            state = update_state(step)
        elif symbol == "M":
            state = prediction_state(step)
        else:
            raise ValueError(f"{name}: sequence has an unexpected symbol {symbol!r}")

        if track is None:
            track = Track([state])
        else:
            track.append(state)

        # Confirmation: the initiator's own condition, on the initiator's own types.
        update_count = sum(1 for s in track if isinstance(s, Update))
        if confirmed_at is None and update_count >= min_points:
            confirmed_at = step

        # Deletion: Stone Soup's deleter, driven for real. Only meaningful once the
        # track has had at least one update, which is the state an initiator releases.
        deletes = bool(deleter.check_for_deletion(track)) if update_count > 0 else False
        if deleted_at is None and deletes:
            deleted_at = step

        per_step.append(
            {
                "step": step,
                "symbol": symbol,
                "update_count": int(update_count),
                "confirmed_by_now": confirmed_at is not None,
                "deleter_says_delete": deletes,
            }
        )

    return {
        "name": name,
        "sequence": sequence,
        "min_points": min_points,
        "time_steps_since_update": time_steps,
        "confirmed_at_step": confirmed_at,
        "deleted_at_step": deleted_at,
        "per_step": per_step,
    }


CASES = [
    # (name, sequence, min_points, time_steps_since_update)
    ("confirm_on_third_consecutive_hit", "HHHHH", 3, 3),
    # The case that separates cumulative from consecutive counting: a miss between
    # hits. Consecutive counting would confirm at step 4; cumulative confirms at 3.
    ("confirm_counts_cumulative_across_a_miss", "HMHHH", 3, 5),
    ("confirm_immediately_min_points_one", "HMMMM", 1, 4),
    ("never_confirms_not_enough_hits", "HMMHM", 4, 9),
    ("delete_on_first_miss", "HMMMM", 1, 1),
    ("delete_on_second_miss", "HHMMM", 2, 2),
    ("delete_on_third_miss", "HHHMMMM", 3, 3),
    # A hit clears the miss run, so deletion is pushed out.
    ("hit_resets_the_miss_run", "HHMMHMM", 2, 3),
    ("long_coast_then_reacquire", "HHHMMHHMMMM", 3, 4),
    ("alternating", "HMHMHMHMHM", 3, 2),
    ("all_misses_after_one_hit", "HMMMMMMMM", 1, 5),
    ("dense_hits_never_deleted", "HHHHHHHHHH", 2, 2),
]


def main() -> int:
    cases = [run_case(*c) for c in CASES]

    confirmed = sum(1 for c in cases if c["confirmed_at_step"] is not None)
    deleted = sum(1 for c in cases if c["deleted_at_step"] is not None)

    doc = {
        "row": "Track lifecycle (init/confirm/coast/delete)",
        "oracle": "Stone Soup UpdateTimeStepsDeleter (driven); "
        "MultiMeasurementInitiator confirmation condition (lifted from source)",
        "oracle_versions": {"stonesoup": stonesoup.__version__, "numpy": np.__version__},
        "python": platform.python_version(),
        "generated_by": "testdata/oracles/tools/gen_lifecycle_fixtures.py",
        "sequence_alphabet": {"H": "the track was associated with a detection", "M": "it was not"},
        "notes": [
            "The deleter is a real Stone Soup object driven against a real Track; the "
            "deletion step index is its answer. The confirmation condition is the one "
            "line from MultiMeasurementInitiator.initiate, evaluated against the same "
            "real Track, because driving the whole initiator would put a predictor, "
            "updater, associator and measurement model between the fixture and the "
            "rule under test. This is stated in the module docstring and in the Rust "
            "test rather than implied.",
            "UpdateTimeStepsDeleter deletes on the nth consecutive miss, not the "
            "(n+1)th: driving it with n = 1, 2, 3 shows deletion at exactly n misses. "
            "The comparison is >=, and the case list pins each of those.",
            "Updates in this fixture carry a real SingleHypothesis. The deleter tests "
            "`isinstance(state, Update) and state.hypothesis`, so an update built with "
            "hypothesis=None is not counted as an update and the deleter falls through "
            "to a distinct-timestamp count. An earlier draft did that and reported a "
            "track deleted while it was being hit every step; dense_hits_never_deleted "
            "and hit_resets_the_miss_run are kept in the case list because they are "
            "what caught it.",
            "Confirmation counts cumulative updates, not a consecutive run. "
            "confirm_counts_cumulative_across_a_miss is the case that distinguishes "
            "the two, and the one that showed the gungnir scaffold's original comment "
            "to be wrong.",
            "MATLAB's trackHistoryLogic and trackScoreLogic are named by the row and "
            "were not run: MATLAB is not installed. See ../README.md.",
        ],
        "cases": cases,
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(doc, indent=1) + "\n", encoding="utf-8")
    print(
        f"wrote {OUT} ({len(cases)} cases; {confirmed} confirm, {deleted} delete "
        f"within their sequence)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
