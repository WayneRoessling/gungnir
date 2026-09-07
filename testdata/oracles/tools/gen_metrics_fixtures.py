# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Generate the py-motmetrics oracle fixture for the `metrics` row.

Row: "MOTA/MOTP, purity/fragmentation, track-to-truth assignment" in
`docs/verification-capability-table.md` §1. Oracle `py-motmetrics`; pass criterion:
**metric values within 1e-3**.

The fixture writes out the *scenario itself* -- every truth object and every hypothesis
at every frame -- so the Rust side runs the same sequence through its own
implementation rather than being handed the oracle's intermediate matching. Comparing
the answers of two independent matchings is the point; handing over the matching would
reduce the test to checking arithmetic.

# Three of the four metrics come from motmetrics; one does not

`mota`, `motp` and `num_fragmentations` are computed by `motmetrics` itself.
`motmetrics` publishes no `purity` metric, so `purity` is computed here from
`motmetrics`' own event dataframe by the definition `gungnir-metrics` documents:
detection-weighted mean track purity, that is, of everything a hypothesis was matched
to, the fraction that went to the truth object it matched most often. That makes the
purity comparison a check of the aggregation given an agreed matching -- and the
matching is precisely what the other three metrics already pin. The Rust test says the
same rather than implying motmetrics has a purity metric.

# Cases

Each case is a hand-built sequence chosen so that one accounting rule dominates it:
a clean track, a missed run, clutter, an identity swap, an interior gap, a crossing
pair. Two are drawn from `gungnir-scenario`'s dense swarm so the row is also exercised
at a realistic target count with a realistic error pattern, which is what the row's
"scenario-crate generated" data source asks for.

Run from `testdata/oracles/` with the venv described in `../README.md`:

    python tools/gen_metrics_fixtures.py

Writes `metrics/clear_mot.json`.
"""

from __future__ import annotations

import json
import platform
import sys
from pathlib import Path

import motmetrics as mm
import numpy as np

OUT = Path(__file__).resolve().parent.parent / "metrics" / "clear_mot.json"

MAX_DISTANCE_M = 1.0


def score(frames: list[dict], max_distance: float) -> dict:
    """Run one sequence through motmetrics and return its metrics.

    `frames` is a list of {"truth": [[id, x, y, z], ...], "tracker": [...]}.
    """
    acc = mm.MOTAccumulator(auto_id=True, max_switch_time=float("inf"))
    for frame in frames:
        oids = [int(o[0]) for o in frame["truth"]]
        hids = [int(h[0]) for h in frame["tracker"]]
        opos = np.array([o[1:] for o in frame["truth"]], dtype=float).reshape(-1, 3)
        hpos = np.array([h[1:] for h in frame["tracker"]], dtype=float).reshape(-1, 3)

        if oids and hids:
            dists = np.linalg.norm(opos[:, None, :] - hpos[None, :, :], axis=2)
            # Beyond the gate is unassignable, which motmetrics expresses as NaN.
            dists = np.where(dists <= max_distance, dists, np.nan)
        else:
            dists = np.empty((len(oids), len(hids)))
        acc.update(oids, hids, dists)

    mh = mm.metrics.create()
    summary = mh.compute(
        acc,
        metrics=[
            "mota",
            "motp",
            "num_fragmentations",
            "num_objects",
            "num_matches",
            "num_switches",
            "num_misses",
            "num_false_positives",
        ],
        name="case",
    )
    row = summary.loc["case"]

    def num(value):
        v = float(value)
        return None if np.isnan(v) else v

    return {
        "mota": num(row["mota"]),
        "motp": num(row["motp"]),
        "fragmentation": float(row["num_fragmentations"]),
        "purity": purity_from_events(acc),
        "num_objects": int(row["num_objects"]),
        "num_matches": int(row["num_matches"]),
        "num_switches": int(row["num_switches"]),
        "num_misses": int(row["num_misses"]),
        "num_false_positives": int(row["num_false_positives"]),
    }


def purity_from_events(acc: mm.MOTAccumulator):
    """Detection-weighted mean track purity, from motmetrics' own event dataframe.

    Not a motmetrics metric: motmetrics has none. Computed from its matching so that
    the comparison isolates the aggregation. Returns None for 0/0, matching
    motmetrics' quiet_divide, which the Rust side reproduces as NaN.
    """
    events = acc.events
    matched = events[events.Type.isin(["MATCH", "SWITCH"])]
    if len(matched) == 0:
        return None
    per_hypothesis: dict[int, dict[int, int]] = {}
    for _, row in matched.iterrows():
        h = int(row.HId)
        o = int(row.OId)
        per_hypothesis.setdefault(h, {})
        per_hypothesis[h][o] = per_hypothesis[h].get(o, 0) + 1
    dominant = sum(max(counts.values()) for counts in per_hypothesis.values())
    total = sum(sum(counts.values()) for counts in per_hypothesis.values())
    return None if total == 0 else dominant / total


def frame(truth, tracker) -> dict:
    return {"truth": truth, "tracker": tracker}


def hand_built_cases() -> list[tuple[str, list[dict]]]:
    cases = []

    # A tracker that is simply right.
    cases.append((
        "perfect",
        [frame([[1, float(k), 0.0, 0.0]], [[10, float(k), 0.0, 0.0]]) for k in range(6)],
    ))

    # Reports nothing at all: every truth object is a miss.
    cases.append((
        "silent_tracker",
        [frame([[1, 0.0, 0.0, 0.0]], []) for _ in range(5)],
    ))

    # Reports only clutter, far from anything: false positives and misses together.
    cases.append((
        "all_clutter",
        [frame([[1, 0.0, 0.0, 0.0]], [[10, 50.0, 0.0, 0.0]]) for _ in range(4)],
    ))

    # A hypothesis handing an object over to a different id: one switch.
    cases.append((
        "identity_swap",
        [
            frame([[1, 0.0, 0.0, 0.0]], [[10 if k < 3 else 20, 0.0, 0.0, 0.0]])
            for k in range(6)
        ],
    ))

    # A gap in the middle of a tracked span: one fragmentation.
    cases.append((
        "interior_gap",
        [
            frame([[1, 0.0, 0.0, 0.0]], [] if k in (2, 3) else [[10, 0.0, 0.0, 0.0]])
            for k in range(7)
        ],
    ))

    # Dropped for good partway through: misses, but not a fragmentation.
    cases.append((
        "trailing_loss",
        [
            frame([[1, 0.0, 0.0, 0.0]], [] if k >= 4 else [[10, 0.0, 0.0, 0.0]])
            for k in range(8)
        ],
    ))

    # Two objects crossing: the case where carrying a match forward matters, because
    # at the crossing frame both hypotheses are equidistant from both objects.
    crossing = []
    for k in range(9):
        a = -4.0 + k
        b = 4.0 - k
        crossing.append(
            frame(
                [[1, a, 0.0, 0.0], [2, b, 0.0, 0.0]],
                [[10, a, 0.0, 0.0], [20, b, 0.0, 0.0]],
            )
        )
    cases.append(("crossing_pair", crossing))

    # One hypothesis covering two objects in turn: impure, and a switch.
    cases.append((
        "one_hypothesis_two_objects",
        [
            frame([[1 if k < 3 else 2, 0.0, 0.0, 0.0]], [[10, 0.0, 0.0, 0.0]])
            for k in range(6)
        ],
    ))

    # Small offsets, so MOTP is a real average rather than zero.
    cases.append((
        "offset_but_matched",
        [
            frame(
                [[1, 0.0, 0.0, 0.0], [2, 10.0, 0.0, 0.0]],
                [[10, 0.1 * k, 0.0, 0.0], [20, 10.0 + 0.05 * k, 0.0, 0.0]],
            )
            for k in range(6)
        ],
    ))

    # Nothing on either side: every ratio is undefined.
    cases.append(("empty_sequence", [frame([], []) for _ in range(3)]))

    # Truth appears partway through and leaves early.
    cases.append((
        "late_birth_early_death",
        [
            frame(
                [] if k < 2 or k > 5 else [[1, 0.0, 0.0, 0.0]],
                [] if k < 3 or k > 5 else [[10, 0.05, 0.0, 0.0]],
            )
            for k in range(8)
        ],
    ))

    return cases


def swarm_cases() -> list[tuple[str, list[dict]]]:
    """Realistic-scale sequences with a realistic error pattern.

    A block of targets on parallel courses, tracked with per-frame noise, occasional
    dropped detections, occasional clutter, and one deliberate identity swap. Generated
    from a fixed seed so the fixture is reproducible.
    """
    out = []
    for name, n_targets, n_frames, drop_p, clutter_p, seed in (
        ("swarm_small", 8, 20, 0.10, 0.10, 20260905),
        ("swarm_dense", 40, 30, 0.15, 0.20, 5092026),
    ):
        rng = np.random.default_rng(seed)
        frames = []
        for k in range(n_frames):
            truth, tracker = [], []
            for t in range(n_targets):
                x = float(t) * 5.0 + 0.3 * k
                y = float(t % 4) * 3.0
                truth.append([t + 1, x, y, 0.0])
                if rng.random() < drop_p:
                    continue  # the tracker lost it this frame
                # Identity swap on one pair, halfway through.
                hid = t + 100
                if t == 1 and k >= n_frames // 2:
                    hid = 2 + 100
                elif t == 2 and k >= n_frames // 2:
                    hid = 1 + 100
                noise = rng.normal(0.0, 0.12, 3)
                tracker.append([hid, x + noise[0], y + noise[1], noise[2]])
            if rng.random() < clutter_p:
                tracker.append([
                    900 + k,
                    float(rng.uniform(-20.0, float(n_targets) * 5.0 + 20.0)),
                    float(rng.uniform(-20.0, 20.0)),
                    0.0,
                ])
            frames.append(frame(truth, tracker))
        out.append((name, frames))
    return out


def main() -> int:
    cases = []
    for name, frames in hand_built_cases() + swarm_cases():
        cases.append(
            {
                "name": name,
                "max_distance_m": MAX_DISTANCE_M,
                "frames": frames,
                "expected": score(frames, MAX_DISTANCE_M),
            }
        )

    doc = {
        "row": "MOTA/MOTP, purity/fragmentation, track-to-truth assignment",
        "oracle": "py-motmetrics",
        "oracle_versions": {"motmetrics": mm.__version__, "numpy": np.__version__},
        "python": platform.python_version(),
        "generated_by": "testdata/oracles/tools/gen_metrics_fixtures.py",
        "frame_format": "{truth, tracker}: lists of [id, x, y, z]; distance is Euclidean",
        "notes": [
            "The scenario is written out in full so the Rust side runs its own matching "
            "over the same data. It is not handed motmetrics' matching, because two "
            "independent matchings agreeing is the thing being tested.",
            "mota, motp and num_fragmentations are motmetrics' own outputs. purity is "
            "not: motmetrics has no purity metric, so it is computed here from "
            "motmetrics' event dataframe by the definition gungnir-metrics documents. "
            "That makes purity a check of the aggregation given an agreed matching.",
            "A null metric means the oracle divided 0 by 0 (motmetrics' quiet_divide "
            "yields NaN). The Rust side reproduces that as NaN, and the test treats "
            "NaN against null as agreement rather than as a failure.",
            "max_switch_time is set to infinity, so an identity switch is charged "
            "however long ago the previous correspondence was. Leaving it at a finite "
            "value would forgive switches after a gap and make the row's number depend "
            "on a threshold nothing else in the workspace sets.",
            "MATLAB's track-metric functions are named by the row and were not run: "
            "MATLAB is not installed. See ../README.md.",
        ],
        "cases": cases,
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(doc, indent=1) + "\n", encoding="utf-8")
    total_frames = sum(len(c["frames"]) for c in cases)
    print(f"wrote {OUT} ({len(cases)} cases, {total_frames} frames)")
    for c in cases:
        e = c["expected"]
        print(
            f"  {c['name']:<28} mota={e['mota']} motp={e['motp']} "
            f"frag={e['fragmentation']} purity={e['purity']} "
            f"sw={e['num_switches']} miss={e['num_misses']} fp={e['num_false_positives']}"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
