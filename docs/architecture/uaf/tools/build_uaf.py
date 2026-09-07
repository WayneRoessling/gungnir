#!/usr/bin/env python3
"""Build and check the code-derived parts of the UAF description (plan 03).

What it does, in order:

1. Regenerates the `resources` section of model/elements.yaml and the `uses`
   section of model/relationships.yaml from the crate manifests, so the resource
   views cannot drift from Cargo.toml (CLAUDE.md, "When a doc and the code
   disagree").
2. Generates the views that are derived from code or from the mission set:
   resources/Rs-Sr, Rs-Cn, Rs-If; services/Sv-If; information/If-Sr;
   operational/Op-Pr-MT-xx and Op-Is-VG-xx; the traceability matrices.
3. Checks the registry: every id referenced exists, every leaf capability is
   exhibited by a performer and achieved by an activity, every activity is realized
   by a service or performed by a human, every service is implemented by a resource
   and its `code` path exists, and every id used in a diagram source exists.

Usage (from the workspace root):

    python docs/architecture/uaf/tools/build_uaf.py          # generate and check
    python docs/architecture/uaf/tools/build_uaf.py --check  # check only, no writes

Requires PyYAML. Exit status is non-zero when a check fails.
"""
from __future__ import annotations

import glob
import os
import re
import sys
from collections import defaultdict
from pathlib import Path

try:
    import yaml
except ImportError:  # pragma: no cover
    sys.exit("build_uaf.py needs PyYAML: pip install pyyaml")

HERE = Path(__file__).resolve().parent
UAF = HERE.parent
DOCS = UAF.parent.parent
ROOT = DOCS.parent
MODEL = UAF / "model"
REQUIREMENTS_SPEC = DOCS / "architecture/togaf/requirements-management/architecture-requirements-specification.md"
NL = "\n"
DATE = "2026-09-04"

LAYERS = [
    ("Tracking core", ["core", "coord", "filters", "association", "track", "rfs", "fusion-async",
                       "track-fusion", "allocation", "scenario", "metrics", "oracle", "testkit", "fuzz"]),
    ("Foundation model", ["model"]),
    ("Service facades", ["tracking-service", "intercept-service"]),
    ("Productization", ["eventing", "store", "config", "mission", "time", "ingest", "sensor-management",
                        "interop", "identity", "identification", "geo", "analytics", "policy", "command",
                        "assessment", "decision", "modelops", "security", "api", "observability",
                        "resilience", "collab", "workflow", "replay", "reporting"]),
    ("Data ecosystem", ["data", "data-fusion", "render"]),
    ("Deployment", ["remote", "node"]),
    ("User interface", ["viewport3d", "ui", "app"]),
]
LAYER_OF = {c: layer for layer, crates in LAYERS for c in crates}

# Thread step -> operational activities (registry OA ids). Keyed by (thread, step).
STEP_ACTIVITIES = {
    ("MT-01", 1): ["OA-01"], ("MT-01", 2): ["OA-02"], ("MT-01", 3): ["OA-03"], ("MT-01", 4): ["OA-04"],
    ("MT-01", 5): ["OA-05", "OA-06"], ("MT-01", 6): ["OA-07", "OA-31"], ("MT-01", 7): ["OA-08", "OA-09"],
    ("MT-01", 8): ["OA-10"], ("MT-01", 9): ["OA-11"],
    ("MT-02", 1): ["OA-01", "OA-15"], ("MT-02", 2): ["OA-02", "OA-04"], ("MT-02", 3): ["OA-03"],
    ("MT-02", 4): ["OA-04"], ("MT-02", 5): ["OA-05", "OA-06"], ("MT-02", 6): ["OA-07"],
    ("MT-02", 7): ["OA-08", "OA-09"], ("MT-02", 8): ["OA-10"],
    ("MT-03", 1): ["OA-01", "OA-02"], ("MT-03", 2): ["OA-03", "OA-36"], ("MT-03", 3): ["OA-04"],
    ("MT-03", 4): ["OA-05", "OA-06"], ("MT-03", 5): ["OA-07"], ("MT-03", 6): ["OA-08", "OA-09"],
    ("MT-03", 7): ["OA-10", "OA-14"],
    ("MT-04", 1): ["OA-01", "OA-02"], ("MT-04", 2): ["OA-12", "OA-18"], ("MT-04", 3): ["OA-03"],
    ("MT-04", 4): ["OA-04"], ("MT-04", 5): ["OA-05", "OA-06"], ("MT-04", 6): ["OA-07"],
    ("MT-04", 7): ["OA-08", "OA-09"], ("MT-04", 8): ["OA-10"],
    ("MT-05", 1): ["OA-01", "OA-02", "OA-03", "OA-36"], ("MT-05", 2): ["OA-16"], ("MT-05", 3): ["OA-12"],
    ("MT-05", 4): ["OA-15"], ("MT-05", 5): ["OA-11"],
    ("MT-06", 1): ["OA-01"], ("MT-06", 2): ["OA-02"], ("MT-06", 3): ["OA-19", "OA-03"],
    ("MT-06", 4): ["OA-04"], ("MT-06", 5): ["OA-30", "OA-06"], ("MT-06", 6): ["OA-07"],
    ("MT-06", 7): ["OA-08"], ("MT-06", 8): ["OA-10", "OA-19"],
    ("MT-07", 1): ["OA-13"], ("MT-07", 2): ["OA-12"], ("MT-07", 3): ["OA-14"], ("MT-07", 4): ["OA-02"],
    ("MT-07", 5): ["OA-13", "OA-07"], ("MT-07", 6): ["OA-13"],
    ("MT-08", 1): ["OA-17"], ("MT-08", 2): ["OA-12"], ("MT-08", 3): ["OA-01"], ("MT-08", 4): ["OA-18"],
    ("MT-08", 5): ["OA-19"], ("MT-08", 6): ["OA-20", "OA-15"], ("MT-08", 7): ["OA-11"],
    ("MT-09", 1): ["OA-21"], ("MT-09", 2): ["OA-22"], ("MT-09", 3): ["OA-23"], ("MT-09", 4): ["OA-24"],
    ("MT-09", 5): ["OA-25"], ("MT-09", 6): ["OA-24", "OA-35"], ("MT-09", 7): ["OA-26"],
    ("MT-10", 1): ["OA-27", "OA-13"], ("MT-10", 2): ["OA-27"], ("MT-10", 3): ["OA-07", "OA-27"],
    ("MT-10", 4): ["OA-28"], ("MT-10", 5): ["OA-29"], ("MT-10", 6): ["OA-27"],
}

# Keywords in an actors line or a decision cell -> sequence-diagram participant.
ROLE_KEYWORDS = [
    ("intelligence analyst", "OP-06 Intelligence analyst"),
    ("sensor manager", "OP-04 Sensor manager"),
    ("supervisor", "OP-02 Supervisor"),
    ("commander", "OP-08 Commander"),
    ("planner", "OP-07 Planner"),
    ("analyst", "OP-03 Analyst"),
    ("fires authority", "Fires authority (OP-08)"),
    ("port defense authority", "Port defense authority (OP-02)"),
    ("site authority", "Site authority (OP-01)"),
    ("engagement authority", "Engagement authority (OP-02)"),
    ("authority", "Engagement authority (OP-02)"),
    ("operator", "OP-01 Operator"),
]
EXTERNAL_KEYWORDS = [
    (("fire unit", "fire group", "effector", "patrol", "helicopter", "fires unit", "shore fire"), "OP-22 Effectors"),
    (("higher command", "peer", "neighbouring", "coalition"), "OP-21 Peers and higher command"),
    (("civil", "port authority", "maritime authorit", "airspace control"), "OP-24 Civil and port authorities"),
    (("isr uas operator", "maintainer"), "OP-23 Sensor operators"),
]


# ----------------------------------------------------------------------------- io
def read(p: Path) -> str:
    return p.read_text(encoding="utf-8")


def write(p: Path, s: str) -> None:
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(s if s.endswith(NL) else s + NL, encoding="utf-8", newline=NL)


def replace_block(text: str, begin: str, end: str, body: str) -> str:
    i = text.index(begin) + len(begin)
    j = text.index(end)
    return text[:i] + NL + body.rstrip(NL) + NL + text[j:]


# ----------------------------------------------------------------------------- code facts
def crate_facts() -> list[dict]:
    """Every gungnir-* crate: kind, description, workspace dependencies, dev-deps."""
    members = set(re.findall(r'"(gungnir-[a-z0-9-]+)"', read(ROOT / "Cargo.toml")))
    facts = []
    for manifest in sorted(glob.glob(str(ROOT / "gungnir-*" / "Cargo.toml"))):
        crate_dir = Path(manifest).parent
        crate = crate_dir.name
        s = read(Path(manifest))

        def deps(section: str) -> list[str]:
            m = re.search(r"^\[" + re.escape(section) + r"\]\n(.*?)(?=^\[|\Z)", s, re.S | re.M)
            return re.findall(r"^(gungnir-[a-z0-9-]+)", m.group(1), re.M) if m else []

        desc = re.search(r'^description\s*=\s*"(.*)"', s, re.M)
        facts.append({
            "crate": crate,
            "short": crate.removeprefix("gungnir-"),
            "id": "RS-" + crate.removeprefix("gungnir-"),
            "kind": "binary" if (crate_dir / "src" / "main.rs").exists() else "library",
            "member": crate in members,
            "description": desc.group(1) if desc else "",
            "deps": deps("dependencies"),
            "dev_deps": deps("dev-dependencies"),
            "layer": LAYER_OF.get(crate.removeprefix("gungnir-"), "Unassigned"),
        })
    return facts


def crate_status() -> dict[str, str]:
    """Status column of the crate map in docs/gungnir-capabilities.md §8."""
    s = read(DOCS / "gungnir-capabilities.md")
    m = re.search(r"^## 8.*?(?=^## 9)", s, re.S | re.M)
    out = {}
    for line in m.group(0).splitlines():
        if line.startswith("| `gungnir-"):
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            out[cells[0].strip("`")] = cells[-1]
    return out


def parse_traits(crate: str) -> list[tuple[str, list[str]]]:
    """(trait name, [method signatures]) for every `pub trait` in the crate."""
    out = []
    for src in sorted(glob.glob(str(ROOT / crate / "src" / "**" / "*.rs"), recursive=True)):
        text = read(Path(src))
        for m in re.finditer(r"^pub trait (\w+)[^{]*\{", text, re.M):
            depth, i = 1, m.end()
            while depth and i < len(text):
                depth += {"{": 1, "}": -1}.get(text[i], 0)
                i += 1
            body = text[m.end():i - 1]
            sigs = []
            for sm in re.finditer(r"^\s*fn\s+(.*?)(?=\s*(?:;|\{))", body, re.M | re.S):
                sig = re.sub(r"\s+", " ", sm.group(1)).strip()
                sig = re.sub(r"\(\s+", "(", sig)
                sig = re.sub(r",\s*\)", ")", sig)
                sigs.append(sig)
            out.append((m.group(1), sigs))
    return out


def parse_model_types() -> tuple[list[tuple[str, str, list[str]]], dict[str, str]]:
    """Structs and enums of gungnir-model: (name, kind, members), and the file each is in."""
    types, files = [], {}
    for src in sorted(glob.glob(str(ROOT / "gungnir-model" / "src" / "*.rs"))):
        text = read(Path(src))
        rel = os.path.relpath(src, ROOT).replace(os.sep, "/")
        for m in re.finditer(r"^pub (struct|enum) (\w+)(\(pub [^)]+\);|\s*\{)", text, re.M):
            kind, name = m.group(1), m.group(2)
            if m.group(3).startswith("("):
                members = [m.group(3).strip("(); ")]
                types.append((name, "tuple struct", members))
            else:
                depth, i = 1, m.end()
                while depth and i < len(text):
                    depth += {"{": 1, "}": -1}.get(text[i], 0)
                    i += 1
                body = text[m.end():i - 1]
                if kind == "struct":
                    members = [re.sub(r"\s+", " ", x).strip() for x in re.findall(r"^\s*pub ([^\n]+?),?\s*$", body, re.M)]
                    members = [x.rstrip(",") for x in members if ":" in x]
                else:
                    members, cur, depth = [], [], 0
                    for line in body.splitlines():
                        line = line.strip()
                        if not line or line.startswith("#") or line.startswith("//"):
                            continue
                        cur.append(line)
                        depth += line.count("{") - line.count("}")
                        if depth == 0:
                            members.append(re.sub(r"\s+", " ", " ".join(cur)).rstrip(","))
                            cur = []
                types.append((name, kind, members))
            files[name] = rel
    return types, files


def api_endpoints() -> list[str]:
    s = read(DOCS / "gungnir-api-v1.md")
    m = re.search(r"^## Endpoints\n\n(.*?)\n\n", s, re.S | re.M)
    return [line for line in m.group(1).splitlines() if line.startswith("|")]


# ----------------------------------------------------------------------------- mission facts
def paragraphs(block: str) -> list[str]:
    out, cur = [], []
    for line in block.splitlines():
        if line.strip():
            cur.append(line.strip())
        elif cur:
            out.append(" ".join(cur))
            cur = []
    if cur:
        out.append(" ".join(cur))
    return out


def parse_threads() -> list[dict]:
    s = read(DOCS / "mission" / "mission-threads.md")
    out = []
    for m in re.finditer(r"^## (MT-\d\d) (.+?)\n(.*?)(?=^## |\Z)", s, re.S | re.M):
        tid, title, body = m.group(1), m.group(2).strip(), m.group(3)
        t = {"id": tid, "title": title, "steps": [], "meta": {}, "actors": ""}
        for p in paragraphs(body):
            if p.startswith("**Actors:**"):
                t["actors"] = p.removeprefix("**Actors:**").strip()
            for key in ("Domain", "Tempo", "Trigger", "Timing", "Success", "Failure", "Capability areas", "Exercised by"):
                mm = re.search(r"\*\*" + key + r":\*\*\s*(.*?)(?=\s\*\*[A-Z][a-z ]+:\*\*|$)", p)
                if mm and key not in t["meta"]:
                    t["meta"][key] = mm.group(1).strip()
        for line in body.splitlines():
            if re.match(r"^\| \d+ \|", line):
                cells = [c.strip() for c in line.strip().strip("|").split("|")]
                t["steps"].append({"n": int(cells[0]), "who": cells[1], "function": cells[2],
                                   "information": cells[3], "decision": cells[4] if len(cells) > 4 else ""})
        out.append(t)
    return out


def parse_vignettes() -> list[dict]:
    s = read(DOCS / "mission" / "vignettes.md")
    out = []
    for m in re.finditer(r"^## (VG-\d\d) (.+?) \((MT-\d\d)\)\n(.*?)(?=^## |\Z)", s, re.S | re.M):
        v = {"id": m.group(1), "title": m.group(2).strip(), "thread": m.group(3), "meta": {}}
        for p in paragraphs(m.group(4)):
            for key in ("Forces", "Timeline", "Success", "Exercised by"):
                if p.startswith(f"**{key}:**"):
                    v["meta"][key] = p.removeprefix(f"**{key}:**").strip()
        out.append(v)
    return out


# ----------------------------------------------------------------------------- registry
def load_registry():
    elements = yaml.safe_load(read(MODEL / "elements.yaml"))
    rels = yaml.safe_load(read(MODEL / "relationships.yaml"))
    return elements, rels


def parse_requirements() -> list[dict]:
    """The architecture requirements, from the specification's tables (GAP-083).

    The specification is the source of truth; the registry carries a generated copy so
    the matrices and the check can join a requirement to the capabilities it names
    (`Source`) and the crates that carry it (`Carried by`)."""
    reqs = []
    seen = set()
    for line in read(REQUIREMENTS_SPEC).splitlines():
        if not line.startswith("| REQ-"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split(" | ")]
        # Two table shapes: seven columns (Id, Requirement, Source, View, Carried by,
        # Priority, Verification) for the functional, data, interoperability and
        # security tables; five (Id, Requirement, Source, Priority, Verification) for
        # performance, usability and constraints, which are carried by budgets and
        # documents rather than crates.
        if len(cells) == 7:
            rid, statement, source, _view, carried, priority, verification = cells
        elif len(cells) == 5:
            rid, statement, source, priority, verification = cells
            carried = ""
        elif len(cells) == 4:
            # Constraints: Id, Requirement, Principle, Verification. The principle is the
            # source, and every constraint is carried by the whole workspace.
            rid, statement, source, verification = cells
            carried, priority = "", "constraint"
        else:
            raise SystemExit(f"{rid if cells else line[:40]}: a requirements table with {len(cells)} columns is not one this tool reads")
        if rid in seen:
            continue
        seen.add(rid)
        reqs.append({
            "id": rid,
            "category": rid.split("-")[1],
            "statement": statement,
            "source": source,
            "caps": re.findall(r"CAP-\d+\.\d+", source),
            "decisions": re.findall(r"D-\d+", source),
            "crates": re.findall(r"`(gungnir-[a-z0-9-]+)`", carried),
            "gaps": re.findall(r"GAP-\d+", carried),
            "priority": priority,
            "verification": verification,
        })
    if len(reqs) < 50:
        raise SystemExit(f"only {len(reqs)} requirements parsed from {REQUIREMENTS_SPEC}")
    return reqs


def regenerate_requirements(reqs: list[dict], facts: list[dict]) -> None:
    resources = {f["id"] for f in facts}
    lines = ["requirements:"]
    for r in reqs:
        name = r["statement"].replace('"', "'")
        # A crate the specification names that the workspace does not have is a planned
        # carrier: recorded on the element, never a relationship to a missing target.
        r["planned_crates"] = [c for c in r["crates"] if "RS-" + c.removeprefix("gungnir-") not in resources]
        r["crates"] = [c for c in r["crates"] if c not in r["planned_crates"]]
        lines.append(f'  - {{id: {r["id"]}, name: "{name}", category: {r["category"]}, '
                     f'source: "{r["source"].replace(chr(34), chr(39))}", '
                     f'priority: "{r["priority"]}", verification: "{r["verification"].replace(chr(34), chr(39))}", '
                     f'gaps: [{", ".join(r["gaps"])}], planned_carriers: [{", ".join(r["planned_crates"])}], '
                     f'owner: architecture-requirements-specification.md}}')
    text = read(MODEL / "elements.yaml")
    text = replace_block(text, "# BEGIN generated requirements", "# END generated requirements", NL.join(lines))
    write(MODEL / "elements.yaml", text)

    lines = ["# satisfies: requirement -> capability (the Source column); carried_by: requirement -> resource (the Carried-by column)",
             "satisfies:"]
    for r in reqs:
        if r["caps"]:
            lines.append(f'  - {{from: {r["id"]}, to: [{", ".join(r["caps"])}]}}')
    lines.append("carried_by:")
    for r in reqs:
        if r["crates"]:
            lines.append(f'  - {{from: {r["id"]}, to: [{", ".join("RS-" + c.removeprefix("gungnir-") for c in r["crates"])}]}}')
    text = read(MODEL / "relationships.yaml")
    text = replace_block(text, "# BEGIN generated requirement relationships", "# END generated requirement relationships", NL.join(lines))
    write(MODEL / "relationships.yaml", text)


def regenerate_registry(facts: list[dict]) -> None:
    lines = ["resources:"]
    for f in facts:
        note = "" if f["member"] else "; not a workspace member (fuzz targets, built separately)"
        desc = f["description"].replace('"', "'")
        lines.append(f'  - {{id: {f["id"]}, name: {f["crate"]}, kind: {f["kind"]}, layer: "{f["layer"]}", '
                     f'code: {f["crate"]}/Cargo.toml, description: "{desc}{note}"}}')
    text = read(MODEL / "elements.yaml")
    text = replace_block(text, "# BEGIN generated resources", "# END generated resources", NL.join(lines))
    write(MODEL / "elements.yaml", text)

    lines = ["uses:"]
    for f in facts:
        if f["deps"]:
            lines.append(f'  - {{from: {f["id"]}, to: [{", ".join("RS-" + d.removeprefix("gungnir-") for d in f["deps"])}]}}')
        if f["dev_deps"]:
            lines.append(f'  - {{from: {f["id"]}, to: [{", ".join("RS-" + d.removeprefix("gungnir-") for d in f["dev_deps"])}], kind: dev}}')
    text = read(MODEL / "relationships.yaml")
    text = replace_block(text, "# BEGIN generated uses", "# END generated uses", NL.join(lines))
    write(MODEL / "relationships.yaml", text)


# ----------------------------------------------------------------------------- view helpers
def header(code: str, title: str, definition: str, purpose: str) -> str:
    return (f"# {code} {title}\n\n"
            f"**UAF definition.** {definition}\n\n"
            f"**Purpose here.** {purpose}\n\n"
            f"Status: generated by `tools/build_uaf.py` on {DATE}; do not edit by hand.\n\n")


def footer(derives_from: str, feeds: str, notes: str = "") -> str:
    s = ""
    if notes:
        s += f"## Notes\n\n{notes}\n\n"
    s += f"## Traceability\n\n- Derives from: {derives_from}\n- Feeds: {feeds}\n"
    return s


def from_mission(s: str) -> str:
    """Re-point relative paths written from docs/mission/ so they resolve from a view folder."""
    return s.replace("`../", "`../../../").replace("](../", "](../../../")


def short_text(s: str, n: int = 70) -> str:
    s = re.sub(r"`", "", s)
    return s if len(s) <= n else s[: n - 1].rstrip() + "…"


def puml_escape(s: str) -> str:
    return s.replace("\n", " ").replace('"', "'")


# ----------------------------------------------------------------------------- resource views
def gen_resource_views(facts: list[dict], status: dict[str, str]) -> None:
    # Rs-Sr
    L = [header("Rs-Sr", "Resource structure",
                "Resource structure shows the composition of resources (systems, software, physical assets) into a structure.",
                "The crates and binaries of the workspace, grouped by architecture layer, with each crate's status from the crate map. Read by engineers and by the TOGAF technology architecture (plan 10).")]
    L.append("| Id | Crate | Kind | Layer | Status (`docs/gungnir-capabilities.md` §8) |")
    L.append("|---|---|---|---|---|")
    for layer, _ in LAYERS:
        for f in [x for x in facts if x["layer"] == layer]:
            st = status.get(f["crate"], "not in the crate map" if not f["member"] else "")
            L.append(f"| {f['id']} | `{f['crate']}` | {f['kind']}{'' if f['member'] else ' (not a workspace member)'} | {layer} | {st} |")
    L.append("")
    L.append(f"Counts: {sum(1 for f in facts if f['member'])} workspace members ({sum(1 for f in facts if f['member'] and f['kind']=='binary')} binaries) plus `gungnir-fuzz`.\n")
    L.append(footer("`Cargo.toml` of every crate; `../../../gungnir-capabilities.md` §8",
                    "Rs-Cn, Rs-If, Ar-Sr, the service-to-resource traceability",
                    "Layer membership follows `../../../../ARCHITECTURE.md` §1 to §8. Status text is copied, not interpreted."))
    write(UAF / "resources" / "Rs-Sr.md", NL.join(L))

    # Rs-Cn (component diagram + edge table)
    P = ["@startuml Rs-Cn", "title Rs-Cn resource connectivity: crate dependency graph (from Cargo.toml)",
         "skinparam componentStyle rectangle", "skinparam linetype ortho", "left to right direction"]
    for layer, _ in LAYERS:
        members = [f for f in facts if f["layer"] == layer and f["member"]]
        if not members:
            continue
        P.append(f'package "{layer}" {{')
        for f in members:
            stereo = "<<Binary>>" if f["kind"] == "binary" else "<<Resource>>"
            P.append(f'  component "{f["crate"]}" as {f["id"].replace("-", "_")} {stereo}')
        P.append("}")
    for f in facts:
        if not f["member"]:
            continue
        for d in f["deps"]:
            P.append(f'{f["id"].replace("-", "_")} --> RS_{d.removeprefix("gungnir-").replace("-", "_")}')
    P.append("@enduml")
    write(UAF / "resources" / "Rs-Cn.puml", NL.join(P))

    L = [header("Rs-Cn", "Resource connectivity",
                "Resource connectivity shows the interfaces and connections between resources.",
                "The one-way dependency graph of the workspace: which crate may call which. It is the truth that `ARCHITECTURE.md` describes in prose; if they disagree, the manifests win.")]
    L.append("Diagram: [`Rs-Cn.puml`](Rs-Cn.puml) (rendered under `rendered/resources/` by the render scripts in the UAF root).\n")
    L.append("| Resource | Depends on (normal) | Dev-only |")
    L.append("|---|---|---|")
    for f in facts:
        L.append(f"| {f['id']} `{f['crate']}` | {', '.join('`' + d + '`' for d in f['deps']) or 'none'} | {', '.join('`' + d + '`' for d in f['dev_deps']) or ''} |")
    L.append("")
    L.append(footer("the `[dependencies]` and `[dev-dependencies]` tables of every manifest; the `uses` section of `../model/relationships.yaml` (generated from the same source)",
                    "Sv-Cn, Ar-Cn, the TOGAF technology architecture",
                    "Direction is one-way: core, then service facades, then productization, then UI (`CLAUDE.md` hard rules). Binaries are the only crates that depend on the UI and data-ecosystem layers."))
    write(UAF / "resources" / "Rs-Cn.md", NL.join(L))

    # Rs-If (trait surfaces)
    L = [header("Rs-If", "Resource interfaces",
                "Resource interfaces describe the interfaces a resource provides or requires.",
                "Every public trait in the workspace with its method signatures, parsed from the source. These are the seams between crates; the services in Sv-If are a subset with operational meaning.")]
    L.append("| Crate | Trait | Methods |")
    L.append("|---|---|---|")
    for f in facts:
        for name, sigs in parse_traits(f["crate"]):
            sig_text = "<br>".join("`" + s.replace("|", "\\|") + "`" for s in sigs) or "(marker)"
            L.append(f"| `{f['crate']}` | `{name}` | {sig_text} |")
    L.append("")
    L.append(footer("`pub trait` declarations under every crate's `src/`", "Sv-If, Sv-Sr", "Signatures are shown as written, without doc comments. Generic and lifetime parameters are kept."))
    write(UAF / "resources" / "Rs-If.md", NL.join(L))


def gen_service_interfaces(facts: list[dict]) -> None:
    L = [header("Sv-If", "Service interfaces",
                "Service interfaces specify the operations a service provides, with their inputs and outputs.",
                "The three contracts that matter to integrators: the tracking and intercept facades the desktop and node call, and the v1 external API. Read by peer-system integrators and by the TOGAF application architecture.")]
    traits = {name: sigs for crate in ("gungnir-tracking-service", "gungnir-intercept-service", "gungnir-api")
              for name, sigs in parse_traits(crate)}
    for name, code in (("TrackingService", "SV-01"), ("InterceptService", "SV-02"), ("ApiHandler", "SV-23"), ("ApiServer", "SV-23")):
        L.append(f"## {code} `{name}`\n")
        for s in traits.get(name, []):
            L.append(f"- `{s}`")
        L.append("")
    L.append("## SV-23 API v1 endpoints (`../../../gungnir-api-v1.md`)\n")
    L.extend(api_endpoints())
    L.append("")
    L.append(footer("the trait declarations in `gungnir-tracking-service`, `gungnir-intercept-service`, `gungnir-api`; the endpoint table in `../../../gungnir-api-v1.md`",
                    "Sv-Cn, Sv-Pr, If-Cn, the service-to-resource traceability",
                    "The transport for SV-23 is not in the workspace (GAP-041); the endpoint table is the contract the transport will carry. Every payload type is an information element in If-Sr."))
    write(UAF / "services" / "Sv-If.md", NL.join(L))


def gen_information_structure() -> None:
    types, files = parse_model_types()
    L = [header("If-Sr", "Information structure",
                "Information structure shows the information elements and their relationships.",
                "The canonical operational data model: every view, event, and value type in `gungnir-model`, parsed from the source, plus the class diagram of how they compose. Every other crate and the API reuse these types, so this is the one vocabulary for tracks, plans, and events.")]
    L.append("Diagram: [`If-Sr.puml`](If-Sr.puml).\n")
    L.append("| Type | Kind | Members | Source |")
    L.append("|---|---|---|---|")
    for name, kind, members in types:
        mem = "<br>".join("`" + m.replace("|", "\\|") + "`" for m in members) or ""
        L.append(f"| `{name}` | {kind} | {mem} | `{files[name]}` |")
    L.append("")
    L.append(footer("`gungnir-model/src/*.rs`", "If-Cn, Sv-If, Op-If, the TOGAF data architecture",
                    "`TrackId`, `TrackStatus`, `ResourceId` are re-exported from `gungnir-core`; `Geodetic` from `gungnir-coord`. `Envelope` and `Event` live in `gungnir-eventing` and wrap the four event enums here."))
    write(UAF / "information" / "If-Sr.md", NL.join(L))

    P = ["@startuml If-Sr", "title If-Sr information structure: gungnir-model", "hide empty members", "skinparam classAttributeIconSize 0"]
    names = {n for n, _, _ in types}
    for name, kind, members in types:
        if kind == "enum":
            P.append(f"enum {name} <<InformationElement>> {{")
            for m in members:
                P.append(f"  {puml_escape(m)}")
            P.append("}")
        else:
            P.append(f"class {name} <<InformationElement>> {{")
            for m in members:
                P.append(f"  {puml_escape(m)}")
            P.append("}")
    for name, kind, members in types:
        for m in members:
            for other in names:
                if other != name and re.search(r"\b" + other + r"\b", m):
                    P.append(f"{name} --> {other}")
    P.append("@enduml")
    write(UAF / "information" / "If-Sr.puml", NL.join(P))


# ----------------------------------------------------------------------------- operational views
def decider(decision: str, actors: str) -> str:
    text = decision.lower()
    for kw, name in ROLE_KEYWORDS:
        if kw in text:
            return name
    for kw, name in ROLE_KEYWORDS:
        if kw in actors.lower() and kw in ("operator",):
            return name
    return "OP-01 Operator"


def participants_for(actors: str) -> list[str]:
    a = actors.lower()
    found = []
    for kw, name in ROLE_KEYWORDS:
        if kw in a and name not in found and "(" not in name.split(" ", 1)[1]:
            found.append(name)
    ext = []
    for kws, name in EXTERNAL_KEYWORDS:
        if any(k in a for k in kws):
            ext.append(name)
    return found, ext


def gen_op_pr(threads: list[dict], elements) -> None:
    oa_name = {e["id"]: e["name"] for e in elements["operational_activities"]}
    for t in threads:
        tid = t["id"]
        P = [f"@startuml Op-Pr-{tid}", f"title Op-Pr {tid}: {puml_escape(t['title'])}", "skinparam activityDiamondBackgroundColor #EEE"]
        P.append("|OP-30 Command-and-control system|")
        P.append("start")
        lane = "system"
        for s in t["steps"]:
            oas = STEP_ACTIVITIES.get((tid, s["n"]), [])
            label = f"{s['n']}. {puml_escape(short_text(s['function'], 90))}\\n<<{', '.join(oas)}>>"
            if s["who"] == "S":
                if lane != "system":
                    P.append("|OP-30 Command-and-control system|"); lane = "system"
                P.append(f":{label};")
            elif s["who"] == "H":
                who = decider(s["decision"], t["actors"])
                if lane != "human":
                    P.append("|Human performers|"); lane = "human"
                P.append(f":{label}\\n({puml_escape(who)});")
            else:  # S+H
                if lane != "system":
                    P.append("|OP-30 Command-and-control system|"); lane = "system"
                P.append(f":{label}\\n(prepare and present);")
                who = decider(s["decision"], t["actors"])
                P.append("|Human performers|"); lane = "human"
                P.append(f":{s['n']}. decide: {puml_escape(short_text(s['decision'] or 'complete the step', 70))}\\n({puml_escape(who)});")
        P.append("stop")
        P.append("@enduml")
        write(UAF / "operational" / f"Op-Pr-{tid}.puml", NL.join(P))

        L = [header(f"Op-Pr-{tid}", f"Operational process: {t['title']}",
                    "Operational processes show the sequence of operational activities performed by operational performers to achieve a mission thread.",
                    f"The steps of mission thread {tid} (`../../../mission/mission-threads.md`) as an activity flow with a system lane and a human lane, each step tagged with the operational activities it performs.")]
        L.append(f"Diagram: [`Op-Pr-{tid}.puml`](Op-Pr-{tid}.puml).\n")
        for key in ("Domain", "Tempo", "Trigger"):
            if key in t["meta"]:
                L.append(f"- **{key}:** {t['meta'][key]}")
        L.append(f"- **Actors:** {t['actors']}")
        L.append("")
        L.append("| Step | Who | Function | Information | Decision | Activities |")
        L.append("|---|---|---|---|---|---|")
        for s in t["steps"]:
            oas = STEP_ACTIVITIES.get((tid, s["n"]), [])
            L.append(f"| {s['n']} | {s['who']} | {s['function']} | {s['information']} | {s['decision']} | {', '.join(oas)} |")
        L.append("")
        for key in ("Timing", "Success", "Failure"):
            if key in t["meta"]:
                L.append(f"**{key}.** {from_mission(t['meta'][key])}\n")
        used = sorted({o for s in t["steps"] for o in STEP_ACTIVITIES.get((tid, s["n"]), [])}, key=lambda x: int(x.split("-")[1]))
        L.append("## Elements used\n")
        for o in used:
            L.append(f"- {o} {oa_name[o]}")
        L.append("")
        L.append(footer(f"`../../../mission/mission-threads.md` {tid}; `../model/elements.yaml` operational_activities",
                        f"Op-Is for the vignette of {tid}; the capability-to-activity traceability; St-Tx via `achieves`",
                        "\"S\" steps are performed by the system, \"H\" by a human, \"S+H\" prepared by the system and completed by a human. Which crate provides each step is in `../../../mission/capabilities/capability-to-crate-matrix.md`."))
        write(UAF / "operational" / f"Op-Pr-{tid}.md", NL.join(L))


def gen_op_is(vignettes: list[dict], threads: list[dict]) -> None:
    by_id = {t["id"]: t for t in threads}
    for v in vignettes:
        t = by_id[v["thread"]]
        roles, externals = participants_for(t["actors"])
        deciders = [decider(s["decision"], t["actors"]) for s in t["steps"]]
        for d in deciders:
            if d not in roles:
                roles.append(d)
        P = [f"@startuml Op-Is-{v['id']}", f"title Op-Is {v['id']}: {puml_escape(v['title'])} ({v['thread']})", "autonumber"]
        P.append('participant "AR-04 Sensor feeds" as Sensors')
        P.append('participant "OP-30 C2 system" as System')
        aliases = {}
        for i, r in enumerate(roles):
            aliases[r] = f"R{i}"
            P.append(f'actor "{r}" as R{i}')
        for i, e in enumerate(externals):
            aliases[e] = f"E{i}"
            P.append(f'participant "{e}" as E{i}')
        if "Timeline" in v["meta"]:
            import textwrap
            P.append("note over System")
            P.append("Timeline:")
            P.extend(textwrap.wrap(puml_escape(v["meta"]["Timeline"]), 90))
            P.append("end note")
        for s in t["steps"]:
            text = puml_escape(short_text(s["function"], 80))
            who = decider(s["decision"], t["actors"])
            role_alias = aliases[who]
            fn = s["function"].lower()
            if s["who"] == "S":
                if s["n"] == 1 or "ingest" in fn:
                    P.append(f"Sensors -> System : {s['n']}. {text}")
                else:
                    P.append(f"System -> System : {s['n']}. {text}")
                if ("hand off" in fn or "handoff" in fn) and "OP-22 Effectors" in aliases:
                    P.append(f"System -> {aliases['OP-22 Effectors']} : handoff with provenance")
                if ("warn" in fn) and "OP-24 Civil and port authorities" in aliases:
                    P.append(f"System -> {aliases['OP-24 Civil and port authorities']} : warning")
                if ("peer" in fn or "exchange" in fn or "warning" in fn) and "OP-21 Peers and higher command" in aliases and s["n"] == 1:
                    P.append(f"{aliases['OP-21 Peers and higher command']} -> System : peer warning or tracks")
            elif s["who"] == "H":
                P.append(f"{role_alias} -> System : {s['n']}. {text}")
            else:
                P.append(f"System -> {role_alias} : {s['n']}. present: {text}")
                P.append(f"{role_alias} -> System : decision: {puml_escape(short_text(s['decision'] or 'complete', 60))}")
        P.append("@enduml")
        write(UAF / "operational" / f"Op-Is-{v['id']}.puml", NL.join(P))

        L = [header(f"Op-Is-{v['id']}", f"Operational interaction scenario: {v['title']}",
                    "Operational interaction scenarios show the sequence of interactions between operational performers in a specific situation.",
                    f"Vignette {v['id']} (`../../../mission/vignettes.md`) played through the steps of thread {v['thread']} as a sequence diagram between sensors, the system, the roles, and the external parties present in the vignette.")]
        L.append(f"Diagram: [`Op-Is-{v['id']}.puml`](Op-Is-{v['id']}.puml). Process: [`Op-Pr-{v['thread']}.md`](Op-Pr-{v['thread']}.md).\n")
        for key in ("Forces", "Timeline", "Success", "Exercised by"):
            if key in v["meta"]:
                L.append(f"**{key}.** {from_mission(v['meta'][key])}\n")
        L.append("## Elements used\n")
        L.append("- AR-04 Sensor feeds; OP-30 Command-and-control system")
        for r in roles:
            L.append(f"- {r}")
        for e in externals:
            L.append(f"- {e}")
        L.append("")
        L.append(footer(f"`../../../mission/vignettes.md` {v['id']}; Op-Pr-{v['thread']}",
                        "the test-track scenario plan 07 derives from this vignette; the UX task analyses (plan 06)",
                        "The message sequence follows the thread's steps; the vignette supplies the timeline, forces, and success condition. Timing between messages is in the timeline note, not in the diagram."))
        write(UAF / "operational" / f"Op-Is-{v['id']}.md", NL.join(L))


# ----------------------------------------------------------------------------- traceability
def index_rels(rels, kind):
    fwd, back = defaultdict(list), defaultdict(list)
    for r in rels.get(kind, []) or []:
        tag = " (planned)" if r.get("status") == "planned" else ""
        for to in r["to"]:
            fwd[r["from"]].append(to + tag)
            back[to].append(r["from"] + tag)
    return fwd, back


def names(elements):
    n = {}
    for kind, items in elements.items():
        if isinstance(items, list):
            for e in items:
                n[e["id"]] = e["name"]
    return n


def gen_traceability(elements, rels) -> None:
    nm = names(elements)
    T = UAF / "traceability"

    def two_way(fname, title, definition, purpose, kind, left_ids, right_ids, left_label, right_label, derives, feeds):
        fwd, back = index_rels(rels, kind)
        L = [header(fname, title, definition, purpose)]
        L.append(f"## {left_label} to {right_label}\n")
        L.append(f"| {left_label} | {right_label} |")
        L.append("|---|---|")
        for i in left_ids:
            L.append(f"| {i} {nm.get(i, '')} | {', '.join(fwd.get(i, [])) or '**none**'} |")
        L.append("")
        L.append(f"## {right_label} to {left_label}\n")
        L.append(f"| {right_label} | {left_label} |")
        L.append("|---|---|")
        for i in right_ids:
            L.append(f"| {i} {nm.get(i, '')} | {', '.join(back.get(i, [])) or '**none**'} |")
        L.append("")
        L.append(footer(derives, feeds))
        write(T / f"{fname}.md", NL.join(L))

    caps = [e["id"] for e in elements["capabilities"] if "parent" in e]
    acts = [e["id"] for e in elements["operational_activities"]]
    svcs = [e["id"] for e in elements["services"]]
    res = [e["id"] for e in elements["resources"]]
    stds = [e["id"] for e in elements["standards"]]

    two_way("capability-to-activity", "Capability to operational activity",
            "The traceability between strategic capabilities and the operational activities that achieve them.",
            "Shows which activities deliver each leaf capability and which capabilities each activity serves; a capability with no activity is a cross-cutting quality.",
            "achieves", acts, caps, "Activity", "Capability",
            "`../model/relationships.yaml` achieves", "St-Tx notes, Op-Pr elements, plan 05 coverage")
    two_way("activity-to-service", "Operational activity to service",
            "The traceability between operational activities and the services that realize them.",
            "Shows which services realize each activity; an activity realized by no service is performed by a human alone.",
            "realizes", svcs, acts, "Service", "Activity",
            "`../model/relationships.yaml` realizes", "Sv-Tx, Sv-Pr, plan 10 application architecture")
    two_way("service-to-resource", "Service to resource",
            "The traceability between services and the resources that implement them.",
            "Shows which crates implement each service and which services each crate contributes to.",
            "implements", res, svcs, "Resource", "Service",
            "`../model/relationships.yaml` implements; Rs-Sr", "Rs-Cn, plan 10 technology architecture")
    two_way("resource-to-standard", "Resource to standard",
            "The traceability between resources and the standards they conform to.",
            "Shows which standards each crate implements today or is planned to (marked planned).",
            "conforms_to", res, stds, "Resource", "Standard",
            "`../model/relationships.yaml` conforms_to", "Sd-Tx, Sd-Rm")

    reqs = [e["id"] for e in elements.get("requirements", [])]
    two_way("requirement-to-capability", "Requirement to capability",
            "The traceability between architecture requirements and the capabilities they are stated against.",
            "Shows which requirements bear on each leaf capability and which capabilities each requirement names; a capability with no requirement is listed in the specification's coverage section, not inferred.",
            "satisfies", reqs, caps, "Requirement", "Capability",
            "`../model/relationships.yaml` satisfies, generated from the requirements specification", "plan 10 requirements management; the capability assessment")
    two_way("requirement-to-resource", "Requirement to resource",
            "The traceability between architecture requirements and the crates that carry them.",
            "Shows which crates a requirement is carried by and which requirements each crate answers for; a requirement carried by no crate is one the code does not yet touch.",
            "carried_by", reqs, res, "Requirement", "Resource",
            "`../model/relationships.yaml` carried_by, generated from the requirements specification", "Rs-Cn; the gap register through each requirement's gaps")

    # role-to-activity as a matrix
    fwd, _ = index_rels(rels, "performs")
    roles = [e for e in elements["operational_performers"] if e.get("kind") == "role"]
    others = [e for e in elements["operational_performers"] if e.get("kind") in ("external", "system")]
    L = [header("role-to-activity", "Role to operational activity",
                "The traceability between operational performers (roles) and the activities they perform.",
                "Which role performs which activity, with the system and the external parties as extra columns; a row with only the system column is fully automated, a row with a role and the system is a human-in-the-loop activity.")]
    cols = roles + others
    L.append("| Activity | " + " | ".join(c["id"] for c in cols) + " |")
    L.append("|---|" + "---|" * len(cols))
    for a in acts:
        marks = []
        for c in cols:
            marks.append("x" if any(x.startswith(a) for x in fwd.get(c["id"], [])) else "")
        L.append(f"| {a} {nm[a]} | " + " | ".join(marks) + " |")
    L.append("")
    L.append("Columns: " + "; ".join(f"{c['id']} {c['name']}" for c in cols) + ".\n")
    L.append(footer("`../model/relationships.yaml` performs; `../../../mission/roles-and-stakeholders.md` §4",
                    "Pr-Cn, Pr-Rm, the UX task analyses (plan 06)"))
    write(T / "role-to-activity.md", NL.join(L))


# ----------------------------------------------------------------------------- checks
def check(elements, rels) -> list[str]:
    problems, notes = [], []
    ids = names(elements)
    for e in elements["capabilities"]:
        if "parent" in e and e["parent"] not in ids:
            problems.append(f"{e['id']} has unknown parent {e['parent']}")
    for kind, entries in rels.items():
        for r in entries or []:
            if r["from"] not in ids:
                problems.append(f"{kind}: unknown source {r['from']}")
            for to in r["to"]:
                if to not in ids:
                    problems.append(f"{kind}: unknown target {to} (from {r['from']})")
    exhibited = {to for r in rels["exhibits"] for to in r["to"]}
    achieved = {to for r in rels["achieves"] for to in r["to"]}
    for e in elements["capabilities"]:
        if "parent" in e:
            if e["id"] not in exhibited:
                problems.append(f"{e['id']} is exhibited by no performer")
            if e["id"] not in achieved:
                notes.append(f"{e['id']} is achieved by no activity (cross-cutting)")
    realized = {to for r in rels["realizes"] for to in r["to"]}
    human_roles = {e["id"] for e in elements["operational_performers"] if e.get("kind") == "role"}
    performed_by_human = {to for r in rels["performs"] if r["from"] in human_roles for to in r["to"]}
    performed = {to for r in rels["performs"] for to in r["to"]}
    for e in elements["operational_activities"]:
        if e["id"] not in realized and e["id"] not in performed_by_human:
            problems.append(f"{e['id']} is realized by no service and performed by no human")
        if e["id"] not in performed:
            problems.append(f"{e['id']} is performed by no performer")
    # Requirements (GAP-083): every one names at least one capability that exists, and a
    # leaf capability nobody requires is noted, because the specification's coverage
    # section is where that is decided, not here.
    satisfied = {to for r in rels.get("satisfies", []) or [] for to in r["to"]}
    for e in elements.get("requirements", []):
        if not any(r["from"] == e["id"] for r in rels.get("satisfies", []) or []):
            if "CAP-" in e.get("source", ""):
                problems.append(f"{e['id']} names a capability that is not in the registry")
            else:
                notes.append(f"{e['id']} names no capability; sourced from {e.get('source') or 'nothing'}")
        for planned in e.get("planned_carriers") or []:
            notes.append(f"{e['id']} is carried by a crate that does not exist yet: {planned}")
    for e in elements["capabilities"]:
        if "parent" in e and e["id"] not in satisfied:
            notes.append(f"{e['id']} is required by no requirement (see the specification's coverage section)")
    implemented = {to for r in rels["implements"] for to in r["to"]}
    for e in elements["services"]:
        if e["id"] not in implemented:
            problems.append(f"{e['id']} is implemented by no resource")
        code = e.get("code", "")
        crate, _, item = code.partition("::")
        crate_dir = ROOT / crate.replace("_", "-")
        if not crate_dir.exists():
            problems.append(f"{e['id']}: crate for {code} not found")
        elif item:
            item_name = item.split("::")[-1]
            found = any(re.search(r"\b(pub trait|pub struct|pub enum|pub fn) " + re.escape(item_name) + r"\b", read(Path(p)))
                        for p in glob.glob(str(crate_dir / "src" / "**" / "*.rs"), recursive=True))
            if not found:
                problems.append(f"{e['id']}: {code} not found in source")
    # diagram references
    pattern = re.compile(r"\b(CAP-\d\.\d+|OA-\d\d|OP-\d\d|SV-\d\d|RS-[a-z0-9-]+|SD-\d\d|IE-\d\d|PT-\d\d|PJ-[A-Z0-9]+|AR-\d\d)\b")
    for src in glob.glob(str(UAF / "**" / "*.puml"), recursive=True) + glob.glob(str(UAF / "**" / "*.mmd"), recursive=True):
        text = read(Path(src))
        for tok in set(pattern.findall(text)):
            tok_norm = tok
            if tok_norm not in ids and tok_norm.replace("_", "-") not in ids:
                problems.append(f"{os.path.relpath(src, UAF)}: {tok} is not in the registry")
    return problems, notes


# ----------------------------------------------------------------------------- main
def main(argv: list[str]) -> int:
    check_only = "--check" in argv
    facts = crate_facts()
    if not check_only:
        regenerate_registry(facts)
        regenerate_requirements(parse_requirements(), facts)
    elements, rels = load_registry()
    if not check_only:
        gen_resource_views(facts, crate_status())
        gen_service_interfaces(facts)
        gen_information_structure()
        threads = parse_threads()
        vignettes = parse_vignettes()
        gen_op_pr(threads, elements)
        gen_op_is(vignettes, threads)
        gen_traceability(elements, rels)
        print(f"generated views for {len(facts)} crates, {len(threads)} threads, {len(vignettes)} vignettes")
    problems, notes = check(elements, rels)
    for n in notes:
        print("note:", n)
    for p in problems:
        print("PROBLEM:", p)
    print(f"registry check: {len(problems)} problems, {len(notes)} notes")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
