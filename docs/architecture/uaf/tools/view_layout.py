#!/usr/bin/env python3
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Read the 51 authored PlantUML views under docs/architecture/uaf and turn each
into an element set, an edge list and a box layout that `export_xmi.py` can emit
as a real Sparx EA diagram.

Why this module exists separately from export_xmi.py: the XMI exporter's job is
EA's file format, and this one's is PlantUML's. They share nothing but the
`ViewDiagram` records below.

Where the layout comes from. Six of PlantUML's diagram kinds are in use here, and
they divide into two groups:

  - DESCRIPTION, CLASS and STATE (30 views) are box-and-line, and PlantUML's own
    SVG render carries every box's position: `<g class="entity"
    data-qualified-name="...">` around a `<rect x= y= width= height=>`, and
    `<g class="cluster" data-qualified-name="...">` around a `<path>` whose
    coordinates bound the container. So these views keep the layout a human
    arranged, rather than being re-laid-out mechanically. That is the whole
    reason this module reads `rendered/` at all.
  - SEQUENCE (10), ACTIVITY (10) and WBS (1) carry no identifiable entity groups
    in their SVG at all -- checked, not assumed: those files have exactly one
    `<g class="...">`, the title. Their content is recovered from the PlantUML
    source instead (participants, `<<OA-nn>>` step markers, WBS bullets) and laid
    out here, because the substance of those views is an ORDER rather than a
    position: a mission thread's steps, a vignette's messages, a taxonomy's
    levels. The order is preserved; the pixel layout is this module's.

The rendered SVG therefore has to match its source. `layout_gaps()` reports any
box-kind view whose render does not position an element its source declares, and
`build_uaf.py`'s check() turns that into a problem -- otherwise a forgotten
`render.sh` would silently replace part of an authored layout with a grid.

What an element IS. A view names a registry element three ways, tried in order:
the label's leading registry id (`rectangle "OP-20 Higher command"`), the label
as an element's registry `name` (the If-Sr class diagrams use Rust type names,
which is what `information_elements` carry in `name`), or the alias with
underscores turned back into hyphens (`RS_fusion_async`). Anything that matches
none of those is not a registry element -- a Rust helper type, a navigation node,
a grouping label -- and is left off the EA diagram rather than invented into the
model.

What an edge IS. Parsed from the PlantUML source, never from the SVG: the Rs-Cn
and If-Sr views render through `!pragma layout smetana`, which emits no edge ids,
so the source is the only complete account. An edge whose two endpoints are a
relationship the registry already carries reuses that relationship's connector,
so EA shows one connector on however many diagrams draw it. An edge that is not
in the registry becomes a view-local connector: the authored view says those two
things are connected, and dropping it would make the EA diagram disagree with the
PlantUML. Such a connector is a plain uml:Dependency with no UAF stereotype, and
carries `uafViewEdge` plus the view it came from as tagged values, so it is
always distinguishable from a registry relationship and can never be mistaken for
one. It does not change `relationships.yaml`; the registry stays the only source
of typed relationships.

Usage as a script, which prints the coverage table and any layout gap:

    python docs/architecture/uaf/tools/view_layout.py
"""
from __future__ import annotations

import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from build_uaf import UAF, load_registry, read  # noqa: E402

# A registry id at the start of a label: "OP-20 Higher command" -> OP-20.
ID_RE = re.compile(r"(CAP-\d+\.\d+|CAP-\d+|OA-\d\d|OP-\d\d|SV-\d\d|RS-[a-z0-9-]+"
                   r"|SD-\d\d|IE-\d\d|PT-\d\d|PJ-[A-Z0-9-]+|AR-\d\d|REQ-[A-Z]-\d\d)")
LEADING_ID_RE = re.compile(r"^" + ID_RE.pattern)

_SHAPES = ("component|rectangle|class|participant|actor|state|usecase|entity|node"
           "|database|queue|collections|interface|artifact|folder|file|card"
           "|storage|cloud|agent|boundary|control|enum|abstract|object|struct")
# `component "A label" as ALIAS <<Stereo>>`
DECL_RE = re.compile(r'^\s*(?:' + _SHAPES + r')\s+"((?:[^"\\]|\\.)*)"\s+as\s+'
                     r'([A-Za-z_][A-Za-z0-9_]*)', re.M)
# `package "OP-10 Sector command post" <<OperationalPerformer>> {`, and the
# composite-state form `state "IE-21 Alert lifecycle (...)" as Alert {`, which is
# a container in the render rather than a plain box.
PKG_RE = re.compile(r'^\s*(?:package|folder|frame|together|rectangle|state)\s+'
                    r'"((?:[^"\\]|\\.)*)"\s*(?:as\s+[A-Za-z_][A-Za-z0-9_]*\s*)?'
                    r'(?:<<[^>]*>>)?\s*\{', re.M)
# Two aliases joined by any PlantUML connector token, with an optional `: label`.
EDGE_RE = re.compile(r'^\s*([A-Za-z_][A-Za-z0-9_]*)\s+([-.<>|*o\[\]#a-z]{2,24})\s+'
                     r'([A-Za-z_][A-Za-z0-9_]*)\s*(?::\s*(.*?))?\s*$', re.M)
TITLE_RE = re.compile(r"^title\s+(.*)$", re.M)
# `:1. Ingest ... <<OA-01, OA-02>>;` -- an activity step naming registry activities
STEP_RE = re.compile(r"^\s*:(.*?);\s*$", re.M | re.S)
SWIMLANE_RE = re.compile(r"^\s*\|([^|]*)\|\s*$", re.M)
WBS_RE = re.compile(r"^(\*+)\s+(.*)$", re.M)
# The view code a file belongs to: Op-Is-VG-01 -> Op-Is, Rs-Cn-overview -> Rs-Cn.
CODE_RE = re.compile(r"^([A-Z][a-z]-[A-Z][a-z])")

BOX_W, BOX_H, GAP_X, GAP_Y, MARGIN = 190, 64, 40, 34, 20


@dataclass
class ViewElement:
    reg_id: str
    alias: str
    label: str
    box: tuple[int, int, int, int]   # Left, Top, Right, Bottom
    container: bool = False          # drawn as a PlantUML package, so a frame in EA


@dataclass
class ViewEdge:
    from_id: str
    to_id: str
    label: str = ""
    seq: int = 0                     # authored order, for the ordered kinds


@dataclass
class ViewDiagram:
    code: str                        # St-Tx, Op-Is, Rs-Cn, ...
    stem: str                        # Op-Is-VG-01
    title: str
    source: str                      # repo-relative path of the .puml
    puml_kind: str                   # DESCRIPTION | CLASS | STATE | SEQUENCE | ACTIVITY | WBS
    layout: str                      # "authored" (from the render) or "generated"
    elements: list[ViewElement] = field(default_factory=list)
    edges: list[ViewEdge] = field(default_factory=list)
    skipped: int = 0                 # declared things that are not registry elements


# ----------------------------------------------------------------------------- identity
def registry_index(elements: dict) -> tuple[set[str], dict[str, str]]:
    """(every id, name -> id). The name index is what lets the If-Sr class
    diagrams resolve: they are drawn with Rust type names, and that is exactly
    what the `information_elements` entries carry in `name`."""
    ids: set[str] = set()
    by_name: dict[str, str] = {}
    for section, items in elements.items():
        if not isinstance(items, list):
            continue
        for e in items:
            if not isinstance(e, dict) or "id" not in e:
                continue
            ids.add(e["id"])
            if e.get("name"):
                by_name.setdefault(str(e["name"]), e["id"])
    return ids, by_name


def resolve(alias: str, label: str, ids: set[str], by_name: dict[str, str]) -> str | None:
    label = label.strip()
    m = LEADING_ID_RE.match(label)
    if m and m.group(1) in ids:
        return m.group(1)
    if label in by_name:
        return by_name[label]
    cand = alias.replace("_", "-")
    if cand in ids:
        return cand
    m = LEADING_ID_RE.match(cand)
    if m and m.group(1) in ids:
        return m.group(1)
    return None


# ----------------------------------------------------------------------------- the render
def svg_kind(svg: str) -> str:
    m = re.search(r'data-diagram-type="([^"]+)"', svg)
    return m.group(1) if m else "UNKNOWN"


_PATH_ARGS = {"M": 2, "L": 2, "T": 2, "C": 6, "S": 4, "Q": 4, "A": 7, "H": 1, "V": 1, "Z": 0}


def path_points(d: str) -> tuple[list[float], list[float]]:
    """The coordinates an SVG path actually visits.

    Not every number in a `d` attribute is a coordinate, which is the trap here: an
    arc is `A rx ry rotation large-arc sweep x y`, so reading the numbers as
    alternating x,y pairs slides everything out of phase from the first rounded
    corner onward. PlantUML draws each container as a rounded rectangle, so every
    cluster box came out starting at 0,0 -- picked up from an arc's two flag digits
    -- and sized from whatever the misalignment produced.

    Commands are walked properly instead, taking only the coordinate pairs: the
    endpoint of an arc, both control points and the endpoint of a curve, and the
    one axis an H or V moves along.
    """
    xs: list[float] = []
    ys: list[float] = []
    cur_x = cur_y = 0.0
    for m in re.finditer(r"([MLTCSQAHVZmltcsqahvz])([^MLTCSQAHVZmltcsqahvz]*)", d):
        cmd = m.group(1)
        rel = cmd.islower()
        key = cmd.upper()
        nums = [float(v) for v in re.findall(r"-?\d*\.?\d+(?:[eE][-+]?\d+)?", m.group(2))]
        n = _PATH_ARGS.get(key, 0)
        if n == 0:
            continue
        for i in range(0, len(nums) - n + 1, n):
            args = nums[i:i + n]
            if key in ("M", "L", "T"):
                pts = [(args[0], args[1])]
            elif key == "C":
                pts = [(args[0], args[1]), (args[2], args[3]), (args[4], args[5])]
            elif key in ("S", "Q"):
                pts = [(args[0], args[1]), (args[2], args[3])]
            elif key == "A":
                pts = [(args[5], args[6])]          # only the endpoint is a coordinate
            elif key == "H":
                pts = [((cur_x + args[0]) if rel else args[0], cur_y)]
            else:                                    # V
                pts = [(cur_x, (cur_y + args[0]) if rel else args[0])]
            for px, py in pts:
                if rel and key not in ("H", "V"):
                    px, py = cur_x + px, cur_y + py
                xs.append(px)
                ys.append(py)
            cur_x, cur_y = xs[-1], ys[-1]
    return xs, ys


def shape_bbox(body: str) -> tuple[float, float, float, float] | None:
    """Bounding box of every drawing primitive in one SVG group.

    Not just `<rect>`: PlantUML draws a `node` as a 3D box made of paths, an
    `actor` as an ellipse plus lines, and a composite state as a path. Reading
    only rects silently lost the geometry for all of those and they fell back to
    a grid slot, which looked exactly like a stale render.
    """
    xs: list[float] = []
    ys: list[float] = []
    for m in re.finditer(r'<rect([^>]*)>', body):
        a = m.group(1)
        g = {k: float(v) for k, v in re.findall(r'\b(x|y|width|height)="(-?[\d.]+)"', a)}
        if {"x", "y", "width", "height"} <= g.keys():
            xs += [g["x"], g["x"] + g["width"]]
            ys += [g["y"], g["y"] + g["height"]]
    for m in re.finditer(r'<(?:ellipse|circle)([^>]*)>', body):
        a = m.group(1)
        g = {k: float(v) for k, v in re.findall(r'\b(cx|cy|rx|ry|r)="(-?[\d.]+)"', a)}
        if "cx" in g and "cy" in g:
            rx = g.get("rx", g.get("r", 0.0))
            ry = g.get("ry", g.get("r", 0.0))
            xs += [g["cx"] - rx, g["cx"] + rx]
            ys += [g["cy"] - ry, g["cy"] + ry]
    for m in re.finditer(r'<(?:polygon|polyline)[^>]*?\bpoints="([^"]*)"', body):
        nums = [float(v) for v in re.findall(r"-?\d+(?:\.\d+)?", m.group(1))]
        xs += nums[0::2]
        ys += nums[1::2]
    for m in re.finditer(r'<path[^>]*?\bd="([^"]*)"', body):
        px, py = path_points(m.group(1))
        xs += px
        ys += py
    for m in re.finditer(r'<line([^>]*)>', body):
        a = m.group(1)
        g = {k: float(v) for k, v in re.findall(r'\b(x1|y1|x2|y2)="(-?[\d.]+)"', a)}
        xs += [g[k] for k in ("x1", "x2") if k in g]
        ys += [g[k] for k in ("y1", "y2") if k in g]
    if not xs or not ys:
        return None
    return min(xs), min(ys), max(xs), max(ys)


def norm_label(s: str) -> str:
    """A label reduced to what survives PlantUML's own sanitising.

    A cluster's `data-qualified-name` is not the source label verbatim: PlantUML
    rewrites punctuation, so `"Assess, Decide & Govern Action"` comes back as
    `Assess. Decide . Govern Action`. Comparing raw labels reported every such
    package as a stale render.
    """
    return re.sub(r"[^0-9a-z]+", " ", s.lower()).strip()


def svg_boxes(svg: str) -> dict[str, tuple[float, float, float, float]]:
    """alias -> box for entities; `PKG::<normalised label>` for clusters."""
    out: dict[str, tuple[float, float, float, float]] = {}
    for m in re.finditer(r'<g class="entity" data-qualified-name="([^"]*)"[^>]*>(.*?)</g>',
                         svg, re.S):
        # A nested entity's qualified name is "<container>.<alias>".
        alias = m.group(1).split(".")[-1]
        b = shape_bbox(m.group(2))
        if b:
            out[alias] = b
    for m in re.finditer(r'<g class="cluster" data-qualified-name="([^"]*)"[^>]*>(.*?)(?=<g |</g>)',
                         svg, re.S):
        b = shape_bbox(m.group(2))
        if not b:
            continue
        # A cluster is named by its LABEL on a component diagram ("OP-10 Sector
        # command post") but by its ALIAS on a state diagram ("Alert"), so it is
        # indexed both ways and the caller can look up whichever it holds.
        name = m.group(1)
        out["PKG::" + norm_label(name)] = b
        out.setdefault(name, b)
    return out


def grid_box(i: int, cols: int) -> tuple[int, int, int, int]:
    row, col = divmod(i, cols)
    left = MARGIN + col * (BOX_W + GAP_X)
    top = MARGIN + row * (BOX_H + GAP_Y)
    return left, top, left + BOX_W, top + BOX_H


# ----------------------------------------------------------------------------- per kind
def _declared(text: str) -> dict[str, tuple[str, str, bool]]:
    """alias -> (label, svg key, is a container).

    The alias and the key the render files a thing under are not always the same.
    A container carrying its own alias -- `state "IE-21 Alert lifecycle" as Alert {`
    -- is an `Alert` to every edge in the source but a CLUSTER in the render, keyed
    by its rewritten label. Conflating the two made every composite state look like
    a render that had lost its box.
    """
    ents: dict[str, tuple[str, str, bool]] = {}
    for label, alias in DECL_RE.findall(text):
        ents[alias] = (label, alias, False)
    for m in PKG_RE.finditer(text):
        label = m.group(1)
        key = "PKG::" + norm_label(label)
        alias_m = re.search(r'"\s+as\s+([A-Za-z_][A-Za-z0-9_]*)', m.group(0))
        alias = alias_m.group(1) if alias_m else key
        ents[alias] = (label, key, True)
    return ents


def _boxed_view(text: str, svg: str, ids, by_name) -> tuple[list[ViewElement], int, str]:
    """DESCRIPTION / CLASS / STATE: identity from the source, geometry from the
    render. An element the render does not place still gets a grid slot, so a
    stale render degrades the layout rather than dropping content."""
    boxes = svg_boxes(svg)
    out, skipped, ungeo = [], 0, 0
    seen: set[str] = set()
    for alias, (label, key, is_container) in _declared(text).items():
        rid = resolve(alias, label, ids, by_name)
        if rid is None:
            skipped += 1
            continue
        if rid in seen:
            # The same element drawn twice under different aliases (Pr-Sr draws
            # PT-01 as four posts). EA places an element once per diagram.
            continue
        seen.add(rid)
        b = boxes.get(key) or boxes.get(alias)
        if b is None:
            out.append(ViewElement(rid, alias, label, grid_box(ungeo, 5), is_container))
            ungeo += 1
        else:
            out.append(ViewElement(rid, alias, label,
                                   (int(b[0]), int(b[1]), int(b[2]), int(b[3])),
                                   is_container))
    layout = "authored" if ungeo == 0 and out else "generated"
    return out, skipped, layout


def _edges_from_source(text: str, alias_to_id: dict[str, str]) -> list[ViewEdge]:
    out, seen, n = [], set(), 0
    for a, _arrow, b, label in EDGE_RE.findall(text):
        ra, rb = alias_to_id.get(a), alias_to_id.get(b)
        if not ra or not rb or ra == rb:
            continue
        n += 1
        key = (ra, rb)
        if key in seen:
            continue
        seen.add(key)
        out.append(ViewEdge(ra, rb, (label or "").strip(), n))
    return out


def _sequence_view(text: str, ids, by_name) -> tuple[list[ViewElement], list[ViewEdge], int]:
    """Op-Is: participants become a row of tall boxes, messages an ordered edge
    list. EA's own UAF Interaction Scenarios view is a Sequence diagram, but a
    faithful one needs lifelines and Parts that this model does not carry, so the
    participants and the message order are carried and the diagram says so."""
    decl = _declared(text)
    alias_to_id, els, skipped = {}, [], 0
    for alias, (label, _key, _c) in decl.items():
        rid = resolve(alias, label, ids, by_name)
        if rid is None:
            skipped += 1
            continue
        alias_to_id[alias] = rid
        if any(e.reg_id == rid for e in els):
            continue
        left = MARGIN + len(els) * (BOX_W + GAP_X)
        els.append(ViewElement(rid, alias, label, (left, MARGIN, left + BOX_W, MARGIN + 420)))
    return els, _edges_from_source(text, alias_to_id), skipped


def _activity_view(text: str, ids, by_name) -> tuple[list[ViewElement], list[ViewEdge], int]:
    """Op-Pr-MT: a mission thread. Each step names its registry activities in a
    `<<OA-nn, OA-mm>>` marker and each swimlane names its performer, so the thread
    recovers as the performers plus the activities in authored order, chained. The
    ordering is the substance of the view and is what is preserved here."""
    els: list[ViewElement] = []
    seen: set[str] = set()
    order: list[str] = []

    for lane in SWIMLANE_RE.findall(text):
        rid = resolve("", lane, ids, by_name)
        if rid and rid not in seen:
            seen.add(rid)
            left = MARGIN + len(els) * (BOX_W + GAP_X)
            els.append(ViewElement(rid, "", lane.strip(), (left, MARGIN, left + BOX_W,
                                                           MARGIN + BOX_H), container=True))
    lanes = len(els)
    for step in STEP_RE.findall(text):
        for rid in ID_RE.findall(step):
            if rid in ids and rid not in seen:
                seen.add(rid)
                order.append(rid)
                i = len(order) - 1
                left = MARGIN + (i % 4) * (BOX_W + GAP_X)
                top = MARGIN + BOX_H + GAP_Y + (i // 4) * (BOX_H + GAP_Y)
                els.append(ViewElement(rid, "", rid, (left, top, left + BOX_W, top + BOX_H)))
    edges = [ViewEdge(order[i], order[i + 1], "then", i + 1) for i in range(len(order) - 1)]
    return els, edges, max(0, len(SWIMLANE_RE.findall(text)) - lanes)


def _wbs_view(text: str, ids, by_name) -> tuple[list[ViewElement], list[ViewEdge], int]:
    """St-Tx: a work-breakdown taxonomy. Depth gives the tree, so each bullet
    becomes a box at its own level and the parent link an edge."""
    els: list[ViewElement] = []
    edges: list[ViewEdge] = []
    seen: set[str] = set()
    stack: dict[int, str] = {}
    per_level: dict[int, int] = {}
    skipped = 0
    for stars, label in WBS_RE.findall(text):
        depth = len(stars)
        rid = resolve("", label, ids, by_name)
        if rid is None:
            skipped += 1
            continue
        stack[depth] = rid
        parent = stack.get(depth - 1)
        if parent and parent != rid:
            edges.append(ViewEdge(parent, rid, "", len(edges) + 1))
        if rid in seen:
            continue
        seen.add(rid)
        i = per_level.get(depth, 0)
        per_level[depth] = i + 1
        left = MARGIN + (depth - 1) * (BOX_W + GAP_X)
        top = MARGIN + i * (BOX_H + 12)
        els.append(ViewElement(rid, "", label.strip(), (left, top, left + BOX_W, top + BOX_H)))
    return els, edges, skipped


# ----------------------------------------------------------------------------- driver
def parse_all(elements: dict) -> tuple[list[ViewDiagram], list[str]]:
    """Every .puml under docs/architecture/uaf, as ViewDiagram records."""
    ids, by_name = registry_index(elements)
    views: list[ViewDiagram] = []
    warnings: list[str] = []
    for puml in sorted(UAF.rglob("*.puml")):
        rel = puml.relative_to(UAF).as_posix()
        stem = puml.stem
        code_m = CODE_RE.match(stem)
        if not code_m:
            warnings.append(f"{rel}: file name does not start with a view code")
            continue
        text = read(puml)
        svg_path = UAF / "rendered" / puml.relative_to(UAF).with_suffix(".svg")
        if not svg_path.exists():
            warnings.append(f"{rel}: no render under rendered/; run render.sh")
            svg = ""
        else:
            svg = read(svg_path)
        kind = svg_kind(svg) if svg else "UNKNOWN"
        title_m = TITLE_RE.search(text)
        title = title_m.group(1).strip() if title_m else stem

        if kind == "SEQUENCE":
            els, edges, skipped = _sequence_view(text, ids, by_name)
            layout = "generated"
        elif kind == "ACTIVITY":
            els, edges, skipped = _activity_view(text, ids, by_name)
            layout = "generated"
        elif kind == "WBS":
            els, edges, skipped = _wbs_view(text, ids, by_name)
            layout = "generated"
        else:
            els, skipped, layout = _boxed_view(text, svg, ids, by_name)
            alias_to_id = {e.alias: e.reg_id for e in els if e.alias}
            for alias, (label, _key, _c) in _declared(text).items():
                rid = resolve(alias, label, ids, by_name)
                if rid:
                    alias_to_id.setdefault(alias, rid)
            edges = _edges_from_source(text, alias_to_id)

        if not els:
            warnings.append(f"{rel}: names no registry element, so it gets no EA diagram")
            continue
        placed = {e.reg_id for e in els}
        edges = [e for e in edges if e.from_id in placed and e.to_id in placed]
        views.append(ViewDiagram(code_m.group(1), stem, title, rel, kind, layout,
                                 els, edges, skipped))
    return views, warnings


def layout_gaps(elements: dict) -> list[str]:
    """Box-kind views whose render does not position every registry element their
    source declares.

    Only this direction matters, and working that out took two wrong attempts.
    Identity comes from the source and geometry from the render, so a render that
    still draws something the source has dropped is harmless: nothing references
    the extra box. A render that is MISSING a box is what costs something, because
    that element falls back to a generated grid slot and the authored layout is
    silently part mechanical. Comparing the two sides by registry id in both
    directions reported dozens of false positives instead, since a render
    legitimately contains entities the registry does not (Rust helper types, enum
    variants, nested states) and labels its stub nodes by crate rather than by id.

    build_uaf.py's check() reports these as problems, which is what makes a
    forgotten render.sh visible.
    """
    ids, by_name = registry_index(elements)
    out = []
    for puml in sorted(UAF.rglob("*.puml")):
        svg_path = UAF / "rendered" / puml.relative_to(UAF).with_suffix(".svg")
        rel = puml.relative_to(UAF).as_posix()
        if not svg_path.exists():
            out.append(f"{rel}: has no render under rendered/ (run render.sh)")
            continue
        svg = read(svg_path)
        if svg_kind(svg) in ("SEQUENCE", "ACTIVITY", "WBS", "UNKNOWN"):
            continue  # no entity groups at all in these; the layout is generated
        boxes = svg_boxes(svg)
        missing = sorted(alias for alias, (label, key, _c) in _declared(read(puml)).items()
                         if resolve(alias, label, ids, by_name)
                         and key not in boxes and alias not in boxes)
        if missing:
            out.append(f"{rel}: the render does not position {', '.join(missing[:6])}"
                       f"{' and more' if len(missing) > 6 else ''}, so those fall back to "
                       f"a generated grid slot (run render.sh)")
    return out


def main() -> int:
    elements, _rels = load_registry()
    views, warnings = parse_all(elements)
    print(f"{'view':50s} {'kind':12s} {'layout':10s} {'els':>4s} {'edges':>6s} {'skip':>5s}")
    for v in views:
        print(f"{v.source:50s} {v.puml_kind:12s} {v.layout:10s} {len(v.elements):4d} "
              f"{len(v.edges):6d} {v.skipped:5d}")
    print(f"\n{len(views)} diagrams, "
          f"{sum(len(v.elements) for v in views)} placements, "
          f"{sum(len(v.edges) for v in views)} edges, "
          f"{sum(1 for v in views if v.layout == 'authored')} with the authored layout")
    for w in warnings:
        print("warning:", w)
    for g in layout_gaps(elements):
        print("layout gap:", g)
    return 0


if __name__ == "__main__":
    sys.exit(main())
