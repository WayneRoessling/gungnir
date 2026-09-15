#!/usr/bin/env python3
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""A layered layout for the diagrams whose positions nobody authored.

The 15 registry view-package diagrams and the generated-layout views (taxonomy,
mission threads) have no PlantUML render to read positions from, and until now
they were placed on a fixed five-column grid -- which for a traceability diagram
of 113 elements and 265 connectors is a wall of crossing lines. This is the
standard Sugiyama-style alternative, kept small and dependency-free so it runs in
CI with nothing but the standard library:

  1. Cycles are broken by dropping the back edges of a depth-first walk, for the
     purpose of layering only; every connector is still drawn.
  2. Each connected element goes in the layer given by its longest path from a
     source, so a resource dependency graph layers itself by architecture depth
     and a traceability diagram by kind (performers, then activities, then
     capabilities), because that is what the relationships run between.
  3. Within a layer, elements are ordered by the barycenter of their neighbours
     in the adjacent layer, sweeping down then up a few times, which is what
     keeps connectors short and reduces crossings.
  4. Layers become columns, left to right, matching the `left to right direction`
     the authored views use; each column is centred vertically on the tallest.
     Elements with no connector at all go in a compact block to the right, in a
     square-ish grid, so they neither stretch the layered part nor vanish.

Deterministic: ties break on the order the caller passed, which is registry
order, so the export stays byte-identical run to run. It is a starting layout
and not a finished drawing -- EA's own Layout Diagram command is one click for
anyone who wants to tidy further -- but it is one a reader can follow.
"""
from __future__ import annotations

import math

Box = tuple[int, int, int, int]  # Left, Top, Right, Bottom


def layered(nodes: list[str], edges: list[tuple[str, str]], *, box_w: int = 160,
            box_h: int = 60, gap_x: int = 70, gap_y: int = 22, margin: int = 20,
            max_rows: int = 24) -> dict[str, Box]:
    """Positions for every node, given directed edges between them."""
    order = {n: i for i, n in enumerate(nodes)}
    succ: dict[str, list[str]] = {n: [] for n in nodes}
    pred: dict[str, list[str]] = {n: [] for n in nodes}
    for a, b in edges:
        if a == b or a not in order or b not in order or b in succ[a]:
            continue
        succ[a].append(b)
        pred[b].append(a)

    # 1. A DAG for layering: an iterative DFS in caller order, dropping back edges.
    state: dict[str, int] = {}          # 1 = on the stack, 2 = finished
    dag_succ: dict[str, list[str]] = {n: [] for n in nodes}
    for start in nodes:
        if state.get(start):
            continue
        state[start] = 1
        stack = [(start, iter(succ[start]))]
        while stack:
            n, it = stack[-1]
            advanced = False
            for m in it:
                s = state.get(m, 0)
                if s == 0:
                    state[m] = 1
                    dag_succ[n].append(m)
                    stack.append((m, iter(succ[m])))
                    advanced = True
                    break
                if s == 2:
                    dag_succ[n].append(m)
                # s == 1: a back edge, left out of the DAG
            if not advanced:
                state[n] = 2
                stack.pop()
    dag_pred: dict[str, list[str]] = {n: [] for n in nodes}
    for a in nodes:
        for b in dag_succ[a]:
            dag_pred[b].append(a)

    connected = [n for n in nodes if succ[n] or pred[n]]
    isolated = [n for n in nodes if not succ[n] and not pred[n]]

    # 2. Layer = longest path from a source, computed in a topological order.
    layer: dict[str, int] = {}
    remaining = {n: len(dag_pred[n]) for n in connected}
    ready = [n for n in connected if remaining[n] == 0]
    while ready:
        n = ready.pop(0)
        layer[n] = max((layer[p] + 1 for p in dag_pred[n] if p in layer), default=0)
        for m in dag_succ[n]:
            if m in remaining:
                remaining[m] -= 1
                if remaining[m] == 0:
                    ready.append(m)
    for n in connected:            # anything a cycle kept unreachable
        layer.setdefault(n, 0)

    columns: dict[int, list[str]] = {}
    for n in connected:
        columns.setdefault(layer[n], []).append(n)
    levels = sorted(columns)
    pos = {n: i for c in levels for i, n in enumerate(columns[c])}

    # 3. Barycenter ordering, a few sweeps each way.
    def bary(n: str, neigh: list[str]) -> float:
        known = [pos[x] for x in neigh if x in pos]
        return sum(known) / len(known) if known else float(pos[n])

    for _ in range(4):
        for c in levels[1:]:
            columns[c].sort(key=lambda n: (bary(n, dag_pred[n]), order[n]))
            pos.update({n: i for i, n in enumerate(columns[c])})
        for c in reversed(levels[:-1]):
            columns[c].sort(key=lambda n: (bary(n, dag_succ[n]), order[n]))
            pos.update({n: i for i, n in enumerate(columns[c])})

    # 4. Layers left to right, each centred on the tallest; isolated nodes in a
    #    compact block to the right. A layer taller than max_rows is wrapped into
    #    adjacent sub-columns, filled down then across so the barycenter order
    #    still reads in sequence: a traceability diagram with 56 capabilities in
    #    one layer was a single 4,590-pixel column, and three sub-columns of 19
    #    are the same picture at a third of the height. The sub-columns sit
    #    closer together than layers do, so a layer still reads as one band.
    out: dict[str, Box] = {}
    step_y = box_h + gap_y
    sub_gap = gap_x // 2
    shape: dict[int, tuple[int, int]] = {}          # layer -> (sub-columns, rows)
    for c in levels:
        n = len(columns[c])
        chunks = max(1, math.ceil(n / max_rows))
        shape[c] = (chunks, math.ceil(n / chunks))
    tallest = max((rows for _chunks, rows in shape.values()), default=0)
    x = margin
    for c in levels:
        chunks, rows = shape[c]
        offset = (tallest - rows) * step_y // 2
        for i, n in enumerate(columns[c]):
            sub, row = divmod(i, rows)
            left = x + sub * (box_w + sub_gap)
            top = margin + offset + row * step_y
            out[n] = (left, top, left + box_w, top + box_h)
        x += chunks * box_w + (chunks - 1) * sub_gap + gap_x
    if isolated:
        cols = max(1, math.ceil(math.sqrt(len(isolated))))
        start_x = x if levels else margin
        for i, n in enumerate(isolated):
            row, col = divmod(i, cols)
            left = start_x + col * (box_w + sub_gap)
            top = margin + row * step_y
            out[n] = (left, top, left + box_w, top + box_h)
    return out


def extent(boxes: dict[str, Box], margin: int = 20) -> tuple[int, int]:
    """(right, bottom) of the whole drawing, for a frame around it."""
    if not boxes:
        return margin * 2, margin * 2
    return (max(b[2] for b in boxes.values()) + margin,
            max(b[3] for b in boxes.values()) + margin)
