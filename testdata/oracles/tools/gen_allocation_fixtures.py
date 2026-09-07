# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Generate the DP oracle fixture for the `gungnir-allocation` row.

Row: "Bellman/DP resource-to-track assignment" in
`docs/verification-capability-table.md` §1. Oracle: a custom textbook-verified Python
DP. Pass criterion: **exact match on the value function (1e-9)**. Data source:
synthetic fixture.

The whole value function is recorded, not only the optimal first-step value. Two
different policies can share an optimal value, so comparing policies would make a tie
look like a disagreement; comparing every state of every layer is what actually pins the
recursion, and it says which layer is wrong when it fails.

The problem, stated the same way `gungnir-allocation/src/bellman.rs` states it:

* state: the set of unserviced tracks, and the steps remaining;
* action: a one-to-one matching of resources to unserviced tracks, the empty matching
  included;
* reward: the sum of `reward[resource][track]` over the matched pairs;
* transition: matched tracks leave the pool, resources do not;
* value: `V(S, k) = max over matchings M of (reward(M) + V(S \\ M, k-1))`, `V(S, 0) = 0`.

This implementation is deliberately written from that statement rather than transcribed
from the Rust: an oracle that shares the implementation shares its bugs. It enumerates
matchings with `itertools` over resource subsets and track permutations, which is a
different construction from the Rust's depth-first walk and agrees only if both are
right.

Run from `testdata/oracles/` with the venv described in `../README.md`:

    python tools/gen_allocation_fixtures.py

Writes `allocation/bellman_dp.json`.
"""

from __future__ import annotations

import itertools
import json
import platform
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "allocation" / "bellman_dp.json"


def matchings(resources: int, available: tuple[int, ...]):
    """Every one-to-one matching between resources and the available tracks.

    Built as: choose how many pairs to make, choose which resources make them, choose
    which tracks they take and in what order. The empty matching is the k = 0 case.
    """
    for k in range(0, min(resources, len(available)) + 1):
        for chosen_resources in itertools.combinations(range(resources), k):
            for chosen_tracks in itertools.permutations(available, k):
                yield tuple(zip(chosen_resources, chosen_tracks))


def value_function(reward: list[list[float]], horizon: int) -> list[list[float]]:
    """`V[k][S]`, where `k = 0` is the full horizon and `S` is a track-set bitmask."""
    resources = len(reward)
    tracks = len(reward[0])
    states = 1 << tracks
    # layers[j] is the value with j steps remaining; layers[0] is terminal.
    layers = [[0.0] * states]
    for _ in range(horizon):
        previous = layers[-1]
        layer = [0.0] * states
        for subset in range(states):
            available = tuple(t for t in range(tracks) if subset & (1 << t))
            best = float("-inf")
            for matching in matchings(resources, available):
                gained = sum(reward[r][t] for r, t in matching)
                consumed = 0
                for _, t in matching:
                    consumed |= 1 << t
                best = max(best, gained + previous[subset & ~consumed])
            layer[subset] = best
        layers.append(layer)
    # Reverse so index 0 is the full horizon, which is how the fixture reads.
    layers.reverse()
    return layers


def case(name: str, reward: list[list[float]], horizon: int) -> dict:
    layers = value_function(reward, horizon)
    return {
        "name": name,
        "reward": reward,
        "horizon": horizon,
        "value_function": layers,
        "optimal_value": layers[0][(1 << len(reward[0])) - 1],
    }


def main() -> None:
    cases = [
        case("one-resource-three-tracks", [[2.0, 9.0, 4.0]], 2),
        case("two-resources-three-tracks", [[5.0, 1.0, 3.0], [2.0, 6.0, 4.0]], 2),
        case(
            "three-resources-four-tracks",
            [
                [7.0, 2.0, 5.0, 1.0],
                [3.0, 8.0, 2.0, 6.0],
                [4.0, 4.0, 9.0, 2.0],
            ],
            3,
        ),
        # Negative rewards: declining is a legal action and the value must never be
        # forced below what doing nothing is worth.
        case("negative-rewards", [[-5.0, -3.0], [-1.0, 2.0]], 2),
        # A horizon longer than the tracks: once every track is serviced there is
        # nothing left to gain, and the extra steps must add exactly zero.
        case("horizon-outlasts-the-tracks", [[3.0, 4.0]], 5),
        # Fractional rewards, to catch a solver that rounds or accumulates in a
        # different order than the oracle.
        case(
            "fractional-rewards",
            [[0.1, 0.25, 0.125], [0.375, 0.0625, 0.5]],
            3,
        ),
    ]
    payload = {
        "oracle": "custom textbook DP (this script)",
        "python": sys.version.split()[0],
        "platform": platform.platform(),
        "row": "allocation / Bellman/DP resource-to-track assignment",
        "criterion": "exact match on value function (1e-9)",
        "cases": cases,
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(payload, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {OUT} ({len(cases)} cases)")


if __name__ == "__main__":
    main()
