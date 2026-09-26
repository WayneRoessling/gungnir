#!/usr/bin/env python3
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

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
VERIFICATION_TABLE = DOCS / "verification-capability-table.md"
GAP_REGISTER = DOCS / "mission/gap-analysis/gap-register.md"
CAPABILITY_ROADMAP = DOCS / "mission/capabilities/capability-roadmap.md"

# A leaf capability that no requirement was derived from has to say where its
# acceptance criterion comes from instead; `coverage` on the entry is that answer,
# and check() verifies the thing it points at exists. The number below is a
# ratchet, not a target: it only ever goes down, and check() fails either way so
# neither a new undermanaged capability nor a stale baseline can pass silently.
CAPABILITIES_WITHOUT_REQUIREMENT = 27

# `status: planned` is load-bearing, not decorative: it makes the consistency check
# treat the relationship as satisfied, so a planned `realizes` is the only reason two
# activities pass "realized by no service". A marker that suppresses a gate has to say
# what will clear it, which is the `plan` field, and the same ratchet applies.
RELATIONSHIPS_PLANNED = 7
NL = "\n"
DATE = "2026-09-04"

LAYERS = [
    ("Tracking core", ["core", "coord", "filters", "association", "track", "rfs", "fusion-async",
                       "track-fusion", "allocation", "scenario", "sensor-sim", "metrics", "oracle",
                       "testkit", "fuzz"]),
    ("Foundation model", ["model"]),
    ("Service facades", ["tracking-service", "intercept-service"]),
    ("Productization", ["eventing", "store", "config", "mission", "time", "ingest", "sensor-management",
                        "interop", "identity", "identification", "geo", "analytics", "policy", "command",
                        "approval", "assessment", "decision", "modelops", "ml", "security", "api",
                        "observability", "resilience", "collab", "workflow", "replay", "reporting"]),
    ("Data ecosystem", ["data", "data-fusion", "render"]),
    ("Deployment", ["remote", "node"]),
    ("User interface", ["viewport3d", "ui", "app"]),
]
LAYER_OF = {c: layer for layer, crates in LAYERS for c in crates}

# Productization is the one LAYERS entry too big for a single Rs-Cn detail diagram
# (25 crates against 1-14 for every other layer). Sub-grouped here for Rs-Cn only;
# LAYERS and LAYER_OF are untouched, so every other generated view is unaffected.
#
# Curated by what each crate's own Cargo.toml description says it does, not by
# parsing docs/gungnir-capabilities.md section 5's prose: that section's five
# subsections *mention* crates for many reasons (cross-reference, a shared
# concern like testing in section 5.6, which names nearly every crate in the
# workspace), so grepping it for crate names would pull in a different, wrong
# set per subsection rather than one owning group per crate. Five groups, not
# section 5's six, because "5.6 Validate Against Reality" is a cross-cutting
# practice covering the whole workspace, not a set of crates it owns.
PRODUCTIZATION_GROUPS = [
    ("Foundational", ["eventing", "store", "config", "mission"]),
    ("Sense, Ingest & Normalize", ["time", "ingest", "sensor-management", "interop"]),
    ("Understand & Maintain the Picture", ["geo", "analytics", "identity", "identification"]),
    ("Assess, Decide & Govern Action", ["policy", "command", "approval", "assessment", "decision",
                                        "modelops", "ml"]),
    ("Secure, Operate & Sustain", ["security", "api", "observability", "resilience",
                                   "collab", "workflow", "replay", "reporting"]),
]
PRODUCTIZATION_GROUP_OF = {c: g for g, crates in PRODUCTIZATION_GROUPS for c in crates}


def rs_cn_group(short: str, layer: str) -> str:
    """Which Rs-Cn detail diagram a crate is drawn in: its layer, or its
    Productization sub-group. Raises if a Productization crate was added to
    LAYERS without being added here, the same guard dependency_graph.rs applies
    at the layer level."""
    if layer != "Productization":
        return layer
    if short not in PRODUCTIZATION_GROUP_OF:
        raise SystemExit(
            f"gungnir-{short} is in the Productization layer but not in any "
            "PRODUCTIZATION_GROUPS entry in build_uaf.py -- add it before "
            "regenerating Rs-Cn"
        )
    return PRODUCTIZATION_GROUP_OF[short]


def rs_cn_group_names() -> list[str]:
    non_prod = [layer for layer, _ in LAYERS if layer != "Productization"]
    return non_prod + [g for g, _ in PRODUCTIZATION_GROUPS]


def slugify(name: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")


# gungnir-model source file -> information domain, for If-Sr (116 types across 23
# files as of 2026-09; one file, one diagram, was unreadable). Curated by what is
# actually declared in each file (checked by hand against the source), not derived
# from the filename. A file not listed here fails the build (see
# `if_sr_domain`), so a new gungnir-model source file cannot silently land in no
# diagram or the wrong one.
INFORMATION_DOMAINS = [
    ("Core identifiers, frames & quality", ["identity.rs", "identifier.rs", "frame.rs", "time.rs",
                                             "quality.rs", "profiles.rs", "provenance.rs"]),
    ("Picture & tracking vocabulary", ["lib.rs"]),
    ("Events & the record", ["events.rs"]),
    ("Plans, effectors & handoff", ["plans.rs", "effectors.rs", "handoff.rs"]),
    ("Policy, authority & settings", ["policy_settings.rs", "anomaly_settings.rs", "ui_settings.rs",
                                      "arbitration.rs"]),
    ("Assets, exchange & releasability", ["assets.rs", "exchange.rs", "releasability.rs"]),
    ("Battle rhythm & mission records", ["rhythm.rs", "requirements.rs", "laydown.rs", "vocabulary.rs"]),
    ("UAS identification & platform reports", ["uas_identification.rs", "uas_platform.rs"]),
]
INFORMATION_DOMAIN_OF = {f: d for d, files in INFORMATION_DOMAINS for f in files}


def if_sr_domain(rel_file: str) -> str:
    fname = rel_file.rsplit("/", 1)[-1]
    if fname not in INFORMATION_DOMAIN_OF:
        raise SystemExit(
            f"gungnir-model/src/{fname} declares a type but is not in any "
            "INFORMATION_DOMAINS entry in build_uaf.py -- add it before "
            "regenerating If-Sr"
        )
    return INFORMATION_DOMAIN_OF[fname]


def if_sr_domain_names() -> list[str]:
    return [d for d, _ in INFORMATION_DOMAINS]


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
def workspace_members() -> set[str]:
    """The crates in the root manifest's `members` array, and only those.

    Reading the array rather than grepping the whole file for quoted
    `gungnir-*` tokens: that harvest also picked up `exclude = ["gungnir-fuzz"]`,
    so the fuzz crate counted as a workspace member. It lost its "not a workspace
    member" note in the registry, its Kind cell in Rs-Sr, and it was drawn as a
    real resource in Rs-Cn-tracking-core, while the member count printed one too
    many.
    """
    s = read(ROOT / "Cargo.toml")
    m = re.search(r"^members\s*=\s*\[(.*?)^\]", s, re.S | re.M)
    if not m:
        m = re.search(r"^members\s*=\s*\[(.*?)\]", s, re.S | re.M)
    if not m:
        raise SystemExit("build_uaf: no `members` array in the root Cargo.toml")
    # Strip trailing comments before looking for quoted names, so a crate named
    # in a comment inside the array cannot be mistaken for an entry.
    body = "\n".join(line.split("#", 1)[0] for line in m.group(1).splitlines())
    return set(re.findall(r'"(gungnir-[a-z0-9-]+)"', body))


def crate_facts() -> list[dict]:
    """Every gungnir-* crate: kind, description, workspace dependencies, dev-deps."""
    members = workspace_members()
    facts = []
    for manifest in sorted(glob.glob(str(ROOT / "gungnir-*" / "Cargo.toml"))):
        crate_dir = Path(manifest).parent
        crate = crate_dir.name
        s = read(Path(manifest))

        def deps(section: str) -> list[str]:
            """Every gungnir-* crate this manifest depends on under `section`.

            Covers the four shapes a Cargo dependency can take, not just the
            plain `[dependencies]` table: a target-specific table
            (`[target.'cfg(loom)'.dev-dependencies]`, already used by
            gungnir-fusion-async), a per-dependency sub-table
            (`[dependencies.gungnir-x]`), and a renamed dependency
            (`alias = { package = "gungnir-x" }`). Any of those was previously
            invisible, so the edge was missing from the `uses` section, the
            Rs-Cn table and every Rs-Cn diagram -- while the view's own footer
            asserts the manifests are the truth.
            """
            found: list[str] = []
            # Plain and target-specific tables: [dependencies],
            # [dev-dependencies], [target.'...'.dependencies], ...
            for m in re.finditer(
                    r"^\[(?:target\.[^\]]+\.)?" + re.escape(section) + r"\]\n(.*?)(?=^\[|\Z)",
                    s, re.S | re.M):
                body = m.group(1)
                found += re.findall(r"^(gungnir-[a-z0-9-]+)\s*=", body, re.M)
                # A renamed dependency names the real crate in `package = "..."`.
                found += re.findall(r'^[A-Za-z0-9_-]+\s*=\s*\{[^}\n]*\bpackage\s*=\s*"(gungnir-[a-z0-9-]+)"',
                                    body, re.M)
            # Per-dependency sub-tables: [dependencies.gungnir-x] and the
            # target-specific form of the same.
            found += re.findall(
                r"^\[(?:target\.[^\]]+\.)?" + re.escape(section) + r"\.(gungnir-[a-z0-9-]+)\]",
                s, re.M)
            for m in re.finditer(
                    r"^\[(?:target\.[^\]]+\.)?" + re.escape(section) + r"\.[A-Za-z0-9_-]+\]\n(.*?)(?=^\[|\Z)",
                    s, re.S | re.M):
                found += re.findall(r'^package\s*=\s*"(gungnir-[a-z0-9-]+)"', m.group(1), re.M)
            # dict.fromkeys: de-duplicated, first-seen order preserved, so the
            # output stays deterministic. A crate listed under both
            # [dependencies] and a cfg-specific table is one edge, not two.
            return list(dict.fromkeys(found))

        desc = re.search(r'^description\s*=\s*"(.*)"', s, re.M)
        normal = deps("dependencies")
        # A crate that is already a real dependency is not also reported as a dev
        # dependency: the normal edge subsumes it, and carrying both put the same
        # from/to pair into the generated `uses` section twice (RS-app -> RS-ui
        # was stated twice for exactly this reason), which is one relationship
        # asserted as two.
        dev_only = [d for d in deps("dev-dependencies") if d not in normal]
        facts.append({
            "crate": crate,
            "short": crate.removeprefix("gungnir-"),
            "id": "RS-" + crate.removeprefix("gungnir-"),
            "kind": "binary" if (crate_dir / "src" / "main.rs").exists() else "library",
            "member": crate in members,
            "description": desc.group(1) if desc else "",
            "deps": normal,
            "dev_deps": dev_only,
            "layer": LAYER_OF.get(crate.removeprefix("gungnir-"), "Unassigned"),
        })
    # A crate with no LAYERS entry used to default quietly to "Unassigned", and
    # Rs-Sr's table iterates LAYERS, so it got no row at all -- while the Counts
    # line below still counted it, making the table under-report the number it
    # prints. Rs-Cn-overview would also draw edges to a component it never
    # declared, and rs_cn_group would raise KeyError on 'Unassigned' the moment
    # anything depended on it. if_sr_domain and rs_cn_group both refuse their
    # analogous case deliberately; this one now does too.
    unassigned = sorted(f["crate"] for f in facts if f["layer"] == "Unassigned")
    if unassigned:
        raise SystemExit(
            "build_uaf: these crates have no architecture layer, so they would be "
            "dropped from Rs-Sr and drawn as phantom nodes in Rs-Cn. Add each to "
            f"LAYERS in this script: {', '.join(unassigned)}")
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


def parse_model_docs() -> dict[str, list[str]]:
    """Type name -> its `///` doc comment lines, from gungnir-model.

    Walking back from the declaration over the attribute block, counting brackets
    so a multi-line `#[derive(...)]` is stepped over whole. Stopping at the first
    line that is not an attribute finds the doc on 99 of the 116 types; a check
    that only skipped lines starting with `#[` found 84, because it halted on the
    `)]` that closes a wrapped derive.
    """
    docs: dict[str, list[str]] = {}
    for src in sorted(glob.glob(str(ROOT / "gungnir-model" / "src" / "*.rs"))):
        lines = read(Path(src)).splitlines()
        for i, line in enumerate(lines):
            m = re.match(r"^pub (?:struct|enum) (\w+)", line)
            if not m:
                continue
            j, depth = i - 1, 0
            while j >= 0:
                s = lines[j].strip()
                if not s:
                    break
                depth += s.count(")") + s.count("]") - s.count("(") - s.count("[")
                if depth > 0 or s.startswith("#["):
                    j -= 1
                    continue
                break
            doc: list[str] = []
            while j >= 0 and lines[j].strip().startswith("///"):
                doc.append(lines[j].strip()[3:].strip())
                j -= 1
            docs[m.group(1)] = list(reversed(doc))
    return docs


def model_type_description(name: str, kind: str, members: list[str],
                           docs: dict[str, list[str]]) -> str:
    """One line describing a gungnir-model type, for the registry.

    The type's own doc comment where it has one -- its first paragraph, which is
    the summary Rust convention puts there -- so the registry says what the code
    says and cannot drift into a second account of it. Seventeen of the 116 types
    carry no doc comment; those fall back to a factual statement of the type's
    shape rather than to invented prose, and `check()` lists them so they can be
    documented in the code instead.
    """
    para: list[str] = []
    for line in docs.get(name, []):
        if not line:
            break
        para.append(line)
    if para:
        text = " ".join(para)
        # Rust links a type as [`Name`]; the brackets mean nothing here.
        return re.sub(r"\[`([^`]+)`\]", r"\1", text).replace("`", "")
    shape = "Variants" if kind == "enum" else "Fields"
    named = [re.split(r"\s*[:(]", x)[0].strip() for x in members][:8]
    named = [x for x in named if x]
    tail = f" {shape}: {', '.join(named)}." if named else ""
    return f"A {kind} of gungnir-model; no doc comment in the code.{tail}"


def model_element_id(name: str) -> str:
    """A generated information element's registry id: `IE-<slug of the type>`.

    Name-derived, not a number continuing IE-33. A positional id would renumber
    every later element whenever a type is added to gungnir-model, which is the
    same trap the EA exporter's relationship keys fell into -- there it meant a
    re-import created duplicates instead of updating. The curated IE-nn entries
    keep their numbers; the two forms never collide because a slug is lowercase.

    Split on the type name's own camel-case boundaries, so `AlgorithmBaselineId`
    is `IE-algorithm-baseline-id` and not `IE-algorithmbaselineid`: these ids are
    read by people in the registry and in EA's Alias column.
    """
    spaced = re.sub(r"(?<=[a-z0-9])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])", " ", name)
    return "IE-" + slugify(spaced)


def api_endpoints() -> list[str]:
    """The endpoint table's rows from docs/gungnir-api-v1.md's Endpoints section.

    Scans the whole section for table rows rather than the first paragraph-sized
    chunk of it. The previous pattern -- `^## Endpoints\\n\\n(.*?)\\n\\n` -- stopped
    at the first blank line, which stopped being the end of the table as soon as
    a sentence of prose was added between the heading and it: the capture then
    held two lines of prose, no `|` rows at all, and this returned an empty list
    while ten table lines sat just below. Nothing noticed, so Sv-If published an
    endpoint heading with no endpoints under it.

    Raises rather than returning empty: an API document with an Endpoints section
    and no endpoint rows in it is a broken input, not an empty one.
    """
    s = read(DOCS / "gungnir-api-v1.md")
    m = re.search(r"^## Endpoints$(.*?)(?=^## |\Z)", s, re.S | re.M)
    if not m:
        raise SystemExit("build_uaf: docs/gungnir-api-v1.md has no '## Endpoints' section")
    rows = [line for line in m.group(1).splitlines() if line.startswith("|")]
    if not rows:
        raise SystemExit("build_uaf: the '## Endpoints' section of docs/gungnir-api-v1.md "
                         "contains no table rows")
    return rows


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
            # Reports the offending LINE, not `rid`: rid is only bound inside the
            # branches above, so naming it here raised UnboundLocalError on the
            # first malformed row and named the previous requirement on any later
            # one -- in both cases hiding the message this branch exists to print.
            raise SystemExit(f"build_uaf: {line.strip()[:80]!r}: a requirements table with "
                             f"{len(cells)} columns is not one this tool reads")
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


def yq(s: object) -> str:
    r"""One YAML scalar, SINGLE-quoted, safe for any text the registry carries.

    Single quotes, not double: inside a YAML double-quoted scalar a backslash
    introduces an escape, and the generated sections were emitted double-quoted
    with only `"` replaced. So a description containing a Windows path broke the
    registry outright (`C:\terrain\dem.tif` -> ScannerError, and nothing
    downstream loads), while `\t` and `\n` in prose turned silently into a real
    tab and a real newline inside the value. Commit 0cb9f0a closed the comma and
    colon holes the same way but left this one.

    In a single-quoted YAML scalar the only escape is `''` for a literal quote,
    so doubling those is the whole job. Newlines are collapsed to a space because
    these are one-line flow-mapping values, not because YAML cannot hold them.
    """
    t = str(s).replace("\r\n", " ").replace("\r", " ").replace("\n", " ").replace("\t", " ")
    return "'" + t.replace("'", "''") + "'"


def regenerate_model_information_elements() -> list[str]:
    """Write the generated block of `information_elements`: every gungnir-model
    type the curated entries above do not already name.

    Why generated rather than hand-authored: the If-Sr views are themselves
    generated from gungnir-model, so the registry has to know the same types or
    the views draw elements the registry cannot resolve -- which is exactly why
    six of the nine If-Sr views got no EA diagram at all. Generating them from
    the same source keeps the two in step by construction, and a type added to
    the crate turns up here on the next run instead of being missed.

    The curated IE-nn entries stay hand-authored and untouched. They are the
    canonical information elements, several of them spanning crates this parser
    never reads (Envelope is gungnir-eventing's, AuditEntry gungnir-security's)
    or naming a pair of types together, so they are not derivable from
    gungnir-model and are not replaced by it.

    Returns the names that had no doc comment to describe them.
    """
    types, files = parse_model_types()
    docs = parse_model_docs()
    # The curated entries are the ones whose id is not a generated slug, read
    # from the file as it stands so a rerun does not treat its own previous
    # output as curated.
    curated = {e["name"] for e in yaml.safe_load(read(MODEL / "elements.yaml"))["information_elements"]
               if not re.match(r"^IE-[a-z]", str(e["id"]))}
    lines, undocumented = [], []
    for name, kind, members in sorted(types, key=lambda t: t[0]):
        if name in curated:
            continue
        if not [x for x in docs.get(name, []) if x]:
            undocumented.append(name)
        desc = model_type_description(name, kind, members, docs)
        domain = if_sr_domain(files[name])
        lines.append(f'  - {{id: {model_element_id(name)}, name: {name}, '
                     f'code: gungnir_model::{name}, domain: {yq(domain)}, '
                     f'description: {yq(desc)}}}')
    text = read(MODEL / "elements.yaml")
    text = replace_block(text, "# BEGIN generated model types",
                         "# END generated model types", NL.join(lines))
    write(MODEL / "elements.yaml", text)
    return undocumented


def regenerate_requirements(reqs: list[dict], facts: list[dict]) -> None:
    resources = {f["id"] for f in facts}
    lines = ["requirements:"]
    for r in reqs:
        # A crate the specification names that the workspace does not have is a planned
        # carrier: recorded on the element, never a relationship to a missing target.
        r["planned_crates"] = [c for c in r["crates"] if "RS-" + c.removeprefix("gungnir-") not in resources]
        r["crates"] = [c for c in r["crates"] if c not in r["planned_crates"]]
        lines.append(f'  - {{id: {r["id"]}, name: {yq(r["statement"])}, category: {r["category"]}, '
                     f'source: {yq(r["source"])}, '
                     f'priority: {yq(r["priority"])}, verification: {yq(r["verification"])}, '
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
        # The separator only appears when there is something to separate: the
        # manifest may carry no description at all, and the note then has nothing
        # in front of it. (Live for gungnir-fuzz, whose note was suppressed
        # entirely until workspace_members stopped counting it as a member.)
        if f["member"]:
            desc = f["description"]
        else:
            note = "not a workspace member (built separately)"
            desc = f"{f['description']}; {note}" if f["description"] else note
        lines.append(f'  - {{id: {f["id"]}, name: {f["crate"]}, kind: {f["kind"]}, layer: {yq(f["layer"])}, '
                     f'code: {f["crate"]}/Cargo.toml, description: {yq(desc)}}}')
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


def member_label(kind: str, m: str) -> str:
    """The name to hang on a reference edge for one member: the variant name for an
    enum (`Decided { plan: PlanId, ... }` -> `Decided`), the field name for a struct
    (`plan: PlanId` -> `plan`), or the member itself for a single-field tuple struct."""
    if kind == "enum":
        return re.match(r"\w+", m).group(0)
    if ":" in m:
        return m.split(":", 1)[0].strip()
    return m


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
            # The fallback has to apply to members too. It was
            # `"not in the crate map" if not f["member"] else ""`, so the only
            # crates that got the explanation were the ones a missing row is
            # expected for, and a member with no §8 row got a blank cell under a
            # column headed "Status" -- live today for gungnir-ml.
            st = status.get(f["crate"]) or "not in the crate map"
            L.append(f"| {f['id']} | `{f['crate']}` | {f['kind']}{'' if f['member'] else ' (not a workspace member)'} | {layer} | {st} |")
    L.append("")
    # The non-members are named from the facts rather than asserted as a literal
    # "plus `gungnir-fuzz`": with gungnir-fuzz wrongly counted as a member, that
    # sentence both over-counted the members and double-counted fuzz.
    non_members = [f"`{f['crate']}`" for f in facts if not f["member"]]
    tail = f" plus {', '.join(non_members)}" if non_members else ""
    L.append(f"Counts: {sum(1 for f in facts if f['member'])} workspace members "
             f"({sum(1 for f in facts if f['member'] and f['kind'] == 'binary')} binaries){tail}.\n")
    L.append(footer("`Cargo.toml` of every crate; `../../../gungnir-capabilities.md` §8",
                    "Rs-Cn, Rs-If, Ar-Sr, the service-to-resource traceability",
                    "Layer membership follows `../../../../ARCHITECTURE.md` §1 to §8. Status text is copied, not interpreted."))
    write(UAF / "resources" / "Rs-Sr.md", NL.join(L))

    # Rs-Cn: one context diagram (the LAYERS boxes, inter-layer edges only) plus
    # one detail diagram per Rs-Cn group -- the six non-Productization layers
    # unchanged, and Productization split five ways (PRODUCTIZATION_GROUPS) because
    # 25 crates in one diagram was the reason this view needed splitting at all.
    # A dependency that crosses a group boundary is drawn twice: fully in its own
    # group's diagram, and as a dashed <<External>> stub carrying a pointer back to
    # that diagram everywhere else it is depended on. No new ids are minted -- a
    # stub reuses the real RS-<crate> alias -- so `check()`'s registry-reference
    # scan (every RS-* token in every .puml must be a real id) needs no changes.
    group_of = {f["short"]: rs_cn_group(f["short"], f["layer"]) for f in facts if f["member"]}
    group_file = {g: f"Rs-Cn-{slugify(g)}" for g in rs_cn_group_names()}

    def write_rs_cn_group(group: str, members: list[dict]) -> None:
        P = [f"@startuml Rs-Cn-{slugify(group)}",
             f"title Rs-Cn resource connectivity: {group} (from Cargo.toml)",
             # smetana lays out a hub-and-stub group (one binary, dozens of external
             # deps) measurably more compactly than Graphviz dot -- confirmed by
             # rendering both and comparing SVG dimensions, not a stylistic choice.
             "!pragma layout smetana",
             "skinparam componentStyle rectangle", "skinparam linetype ortho", "left to right direction"]
        local_ids = {f["id"].replace("-", "_") for f in members}
        P.append(f'package "{group}" {{')
        for f in members:
            stereo = "<<Binary>>" if f["kind"] == "binary" else "<<Resource>>"
            P.append(f'  component "{f["crate"]}" as {f["id"].replace("-", "_")} {stereo}')
        P.append("}")
        stubs: dict[str, str] = {}  # dep crate id -> home group, for external deps only
        for f in members:
            for d in f["deps"]:
                d_short = d.removeprefix("gungnir-")
                d_id = f"RS_{d_short.replace('-', '_')}"
                if d_id not in local_ids and d_short in group_of:
                    stubs[d_id] = group_of[d_short]
        for d_id, home in sorted(stubs.items()):
            P.append(f'component "{d_id[3:]}\\n(see {group_file[home]})" as {d_id} <<External>> #line.dashed')
        for f in members:
            for d in f["deps"]:
                P.append(f'{f["id"].replace("-", "_")} --> RS_{d.removeprefix("gungnir-").replace("-", "_")}')
        P.append("@enduml")
        write(UAF / "resources" / f"{group_file[group]}.puml", NL.join(P))

    for group in rs_cn_group_names():
        members = [f for f in facts if f["member"] and group_of.get(f["short"]) == group]
        if members:
            write_rs_cn_group(group, members)

    # Context diagram: groups as boxes, one edge per (group, group) pair that has
    # at least one real crate-to-crate dependency, so the reader sees the shape of
    # the graph before choosing which detail diagram to open.
    P = ["@startuml Rs-Cn-overview", "title Rs-Cn resource connectivity: overview (from Cargo.toml)",
         "!pragma layout smetana",
         "skinparam componentStyle rectangle", "left to right direction"]
    for group in rs_cn_group_names():
        P.append(f'component "{group}" as {slugify(group).replace("-", "_")} <<Group>> [[{group_file[group]}.svg]]')
    seen_edges = set()
    for f in facts:
        if not f["member"]:
            continue
        src_group = group_of.get(f["short"])
        for d in f["deps"]:
            dst_group = group_of.get(d.removeprefix("gungnir-"))
            if src_group and dst_group and src_group != dst_group and (src_group, dst_group) not in seen_edges:
                seen_edges.add((src_group, dst_group))
                P.append(f'{slugify(src_group).replace("-", "_")} --> {slugify(dst_group).replace("-", "_")}')
    P.append("@enduml")
    write(UAF / "resources" / "Rs-Cn-overview.puml", NL.join(P))

    L = [header("Rs-Cn", "Resource connectivity",
                "Resource connectivity shows the interfaces and connections between resources.",
                "The one-way dependency graph of the workspace: which crate may call which. It is the truth that `ARCHITECTURE.md` describes in prose; if they disagree, the manifests win.")]
    L.append("One monolithic diagram of all 50 members was unreadable, so this view is "
             "an overview plus one detail diagram per group: `Rs-Cn-overview.puml` shows "
             "the groups and which groups depend on which; each detail diagram shows one "
             "group's crates in full and draws anything it depends on outside the group as "
             "a dashed `<<External>>` stub carrying a pointer to that dependency's own "
             "diagram, rather than repeating its own dependencies. The six non-Productization "
             "layers each get one diagram; Productization -- 25 crates, too many for one "
             "diagram -- is split five ways (see `PRODUCTIZATION_GROUPS` in `tools/build_uaf.py`, "
             "curated from each crate's own manifest description, not parsed from prose).\n")
    L.append("Diagrams (rendered under `rendered/resources/` by the render scripts in the UAF root):\n")
    L.append("- [`Rs-Cn-overview.puml`](Rs-Cn-overview.puml)")
    for group in rs_cn_group_names():
        L.append(f"- [`{group_file[group]}.puml`]({group_file[group]}.puml) -- {group}")
    L.append("")
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
    # The heading said "API v1" while every path in the table it introduces is
    # under a later version (/v3 since GAP-130) -- the document kept its filename when
    # the paths moved, and this heading followed the filename rather than the contract.
    L.append("## SV-23 API endpoints (`../../../gungnir-api-v1.md`)\n")
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
    L.append(f"One monolithic diagram of all {len(types)} types was unreadable, so this view "
             "is an overview plus one detail diagram per information domain: `If-Sr-overview.puml` "
             "shows the domains and which domains reference which; each detail diagram draws one "
             "domain's types in full, with members, and draws any type it references outside the "
             "domain as an empty `<<External>>` stub carrying a pointer to that type's own diagram. "
             "Domains are curated in `INFORMATION_DOMAINS` in `tools/build_uaf.py`, grounded in what "
             "each `gungnir-model/src/*.rs` file actually declares.\n")
    L.append("Diagrams (rendered under `rendered/information/` by the render scripts in the UAF root):\n")
    L.append("- [`If-Sr-overview.puml`](If-Sr-overview.puml)")
    for domain in if_sr_domain_names():
        L.append(f"- [`If-Sr-{slugify(domain)}.puml`](If-Sr-{slugify(domain)}.puml) -- {domain}")
    L.append("")
    L.append("| Type | Kind | Domain | Members | Source |")
    L.append("|---|---|---|---|---|")
    for name, kind, members in types:
        mem = "<br>".join("`" + m.replace("|", "\\|") + "`" for m in members) or ""
        L.append(f"| `{name}` | {kind} | {if_sr_domain(files[name])} | {mem} | `{files[name]}` |")
    L.append("")
    L.append(footer("`gungnir-model/src/*.rs`", "If-Cn, Sv-If, Op-If, the TOGAF data architecture",
                    "`TrackId`, `TrackStatus`, `ResourceId` are re-exported from `gungnir-core`; `Geodetic` from `gungnir-coord`. `Envelope` and `Event` live in `gungnir-eventing` and wrap the four event enums here."))
    write(UAF / "information" / "If-Sr.md", NL.join(L))

    # Deterministic order, and the same order the classes are emitted in. A set
    # comprehension here made the emitted relationship lines follow Python's string hash
    # order, which is randomised per process (PYTHONHASHSEED), so two runs of this
    # generator produced the same relationships in different sequences and the CI diff
    # check failed at random. `dict.fromkeys` dedupes while keeping first-seen order.
    names = list(dict.fromkeys(n for n, _, _ in types))
    domain_of_type = {name: if_sr_domain(files[name]) for name, _, _ in types}
    type_file = {d: f"If-Sr-{slugify(d)}" for d in if_sr_domain_names()}
    # (name -> kind, members) for the per-domain detail-diagram emitter below.
    by_name = {name: (kind, members) for name, kind, members in types}

    def emit_class(P: list[str], name: str, hollow: bool = False) -> None:
        kind, members = by_name[name]
        label = f"{name}\\n(see {type_file[domain_of_type[name]]})" if hollow else name
        opener = "enum" if kind == "enum" else "class"
        stereo = "<<External>>" if hollow else "<<InformationElement>>"
        style = " #line.dashed" if hollow else ""
        P.append(f"{opener} \"{label}\" as {name} {stereo}{style} {{")
        if not hollow:
            for m in members:
                P.append(f"  {puml_escape(m)}")
        P.append("}")

    def write_if_sr_domain(domain: str, domain_names: list[str]) -> None:
        P = [f"@startuml If-Sr-{slugify(domain)}", f"title If-Sr information structure: {domain}",
             # Class diagrams default to top-to-bottom, which stacks these mostly
             # sibling, mostly edge-sparse types into one enormous row (measured:
             # some domains rendered 30-40x wider than tall). left-to-right plus
             # smetana wraps them into a far more square layout instead.
             "left to right direction", "!pragma layout smetana",
             "hide empty members", "skinparam classAttributeIconSize 0"]
        for name in domain_names:
            emit_class(P, name, hollow=False)
        stubs: set[str] = set()
        for name in domain_names:
            _, members = by_name[name]
            for m in members:
                for other in names:
                    if other != name and other not in domain_names and re.search(r"\b" + other + r"\b", m):
                        stubs.add(other)
        for other in sorted(stubs):
            emit_class(P, other, hollow=True)
        for name in domain_names:
            kind, members = by_name[name]
            # One edge per (name, other) pair, not one per member that mentions
            # `other` -- a domain-heavy enum like an event type can reference the
            # same external id from half its variants, which used to draw that
            # many overlapping arrows. The edge carries the variant/field names
            # that referenced `other`, so the collapse loses no information.
            refs: dict[str, list[str]] = {}
            for m in members:
                label = member_label(kind, m)
                for other in names:
                    if other != name and re.search(r"\b" + other + r"\b", m):
                        labels = refs.setdefault(other, [])
                        if label not in labels:
                            labels.append(label)
            for other in sorted(refs):
                P.append(f'{name} --> {other} : {", ".join(refs[other])}')
        P.append("@enduml")
        write(UAF / "information" / f"{type_file[domain]}.puml", NL.join(P))

    for domain in if_sr_domain_names():
        domain_names = [n for n in names if domain_of_type[n] == domain]
        if domain_names:
            write_if_sr_domain(domain, domain_names)

    # Context diagram: domains as boxes, one edge per (domain, domain) pair with at
    # least one real cross-domain member reference.
    P = ["@startuml If-Sr-overview", "title If-Sr information structure: overview",
         "!pragma layout smetana",
         "skinparam componentStyle rectangle", "left to right direction"]
    for domain in if_sr_domain_names():
        P.append(f'component "{domain}" as {slugify(domain).replace("-", "_")} <<Group>> [[{type_file[domain]}.svg]]')
    seen_edges = set()
    for name, kind, members in types:
        src = domain_of_type[name]
        for m in members:
            for other in names:
                if other == name or not re.search(r"\b" + other + r"\b", m):
                    continue
                dst = domain_of_type[other]
                if src != dst and (src, dst) not in seen_edges:
                    seen_edges.add((src, dst))
                    P.append(f'{slugify(src).replace("-", "_")} --> {slugify(dst).replace("-", "_")}')
    P.append("@enduml")
    write(UAF / "information" / "If-Sr-overview.puml", NL.join(P))


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
def rel_targets(r: dict) -> list[str]:
    """The `to` of one relationship entry, as a list, whether it was written as a
    sequence or as a single id.

    Both forms are legal in the registry and the two EA exporters both accept
    either, but this module iterated `r["to"]` directly -- so a scalar iterated
    its CHARACTERS, and one `{from: OP-09, to: CAP-1.1}` turned into seven
    targets named `C`, `A`, `P`, `-`, `1`, `.`, `1`: a matrix cell reading
    `C, A, P, -, 1, ., 1` and eight check() problems naming single characters.
    """
    to = r.get("to")
    if to is None:
        return []
    return list(to) if isinstance(to, list) else [to]


def index_rels(rels, kind):
    fwd, back = defaultdict(list), defaultdict(list)
    for r in rels.get(kind, []) or []:
        tag = " (planned)" if r.get("status") == "planned" else ""
        for to in rel_targets(r):
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
            "Shows which requirements were derived from each leaf capability, and which capabilities each requirement names as its source. A **none** here is not a capability nobody specified: the Source column records where a requirement came from, so one drawn from a principle, a decision or a budget names no capability however squarely it bears on one. Every **none** below carries a disposition in section 8.1 of the requirements specification saying where its acceptance criterion does come from, and `build_uaf.py` refuses one that has neither.",
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
# Relationship kind -> (section its source must be in, section its target must be
# in). Checked by check(); a kind absent here is not kind-checked.
RELATIONSHIP_ENDPOINTS = {
    "exhibits": ("operational_performers", "capabilities"),
    "achieves": ("operational_activities", "capabilities"),
    "performs": ("operational_performers", "operational_activities"),
    "realizes": ("services", "operational_activities"),
    "implements": ("resources", "services"),
    "conforms_to": ("resources", "standards"),
    "uses": ("resources", "resources"),
    "satisfies": ("requirements", "capabilities"),
    "carried_by": ("requirements", "resources"),
    "exchanges": ("operational_performers", "operational_performers"),
}

# A relationship kind whose endpoint shape is ambiguous, and the view codes in
# which an edge of that shape MEANS that kind. A performer-to-performer line is
# a needline in the connectivity and interaction views, but in the structure
# view (Op-Sr: nodes, roles, system parts) it is a structural attachment --
# the sensor network feeding the service node -- which UAF models as structure,
# not exchange. So the disagreement check applies `exchanges` only where the
# line is an exchange; elsewhere the edge stays view-local. Kinds not listed
# here are unambiguous from their endpoint types alone.
EDGE_KIND_VIEWS = {
    "exchanges": {"Op-Cn", "Op-Is"},
}


def coverage_evidence() -> tuple[set[str], dict[str, str], dict[str, list[str]]]:
    """What a `coverage` disposition can point at, read from the documents.

    Returns the capabilities the verification table names, each gap's status, and
    each capability's roadmap maturity per increment.
    """
    in_table = set(re.findall(r"CAP-[0-9.]+[0-9]", read(VERIFICATION_TABLE)))
    gap_status: dict[str, str] = {}
    for line in read(GAP_REGISTER).splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        # The summary table's rows only: the per-entry prose below it repeats the
        # identifier in a heading, where there is no status column to read.
        if len(cells) >= 11 and re.fullmatch(r"GAP-\d+", cells[0]):
            gap_status[cells[0]] = cells[10]
    maturity: dict[str, list[str]] = {}
    for line in read(CAPABILITY_ROADMAP).splitlines():
        m = re.match(r"\| (CAP-[0-9.]+) [^|]*\|(.*)\|\s*$", line)
        if m:
            maturity[m.group(1)] = [c.strip() for c in m.group(2).split("|") if c.strip()]
    return in_table, gap_status, maturity


def coverage_problems(cap: str, disposition: str, requirement_ids: set[str],
                      in_table: set[str], gap_status: dict[str, str],
                      maturity: dict[str, list[str]]) -> list[str]:
    """Whether a capability's `coverage` points at something that is actually there.

    An unchecked disposition is worth nothing: "waiting on GAP-057" stops being
    true the day that gap closes, and the capability would go back to having no
    criterion with the registry still claiming it was covered.
    """
    token = disposition.split("--")[0].strip()
    parts = token.split()
    if not parts:
        return [f"{cap}: coverage is empty"]
    if parts == ["verification-table"]:
        if cap not in in_table:
            return [f"{cap}: coverage says verification-table, but no row there names it"]
        return []
    if parts[0] == "none":
        if "--" not in disposition or not disposition.split("--", 1)[1].strip():
            return [f"{cap}: coverage none must give a reason after --"]
        return []
    out = []
    for p in parts:
        if re.fullmatch(r"GAP-\d+", p):
            if p not in gap_status:
                out.append(f"{cap}: coverage names {p}, which is not in the gap register")
            # `startswith`, not equality: GAP-103's status cell reads "Closed
            # 2026-09-09". An exact match reads that as still open, which is the
            # silent pass this gate exists to stop -- a capability whose gap has
            # closed is a capability whose criterion has quietly gone missing.
            elif gap_status[p].startswith("Closed"):
                out.append(f"{cap}: coverage waits on {p}, which is closed -- it needs a "
                           f"requirement, a verification row, or a new disposition")
        elif re.fullmatch(r"REQ-[A-Z]-\d+", p):
            if p not in requirement_ids:
                out.append(f"{cap}: coverage names {p}, which is not a requirement")
        elif re.fullmatch(r"I\d", p):
            # Deferred to a later increment only counts while the roadmap agrees
            # it is not finished now. Full at increment 1 and deferred are not
            # both true, and that combination is how a delivered capability ends
            # up with nobody looking for its criterion.
            mat = maturity.get(cap)
            if not mat:
                out.append(f"{cap}: coverage defers to {p}, but the roadmap has no row for it")
            elif mat[0] == "full":
                out.append(f"{cap}: coverage defers to {p}, but the roadmap has it full at "
                           f"increment 1")
        else:
            out.append(f"{cap}: coverage token {p!r} is not verification-table, a GAP id, "
                       f"a REQ id, an increment, or none")
    return out


def planned_problems(kind: str, r: dict, gap_status: dict[str, str]) -> list[str]:
    """Whether a `status: planned` relationship says what will clear it, truthfully.

    The same rule the capability dispositions follow: a marker that points at a gap
    stops being true the day that gap closes. Four of these pointed at gaps that had
    already closed while the registry still said the code did not carry the standard --
    the ASTERIX codec, the AIS and ADS-B adapters, and UUIDv7 were all shipped and
    still marked planned.
    """
    label = f"{kind}: {r['from']} -> {', '.join(rel_targets(r)) or '(nothing)'}"
    plan = str(r.get("plan") or "").strip()
    if not plan:
        return [f"{label} is planned with no plan: say which open gap clears it, or "
                f"'blocked -- reason'"]
    if plan.startswith("blocked"):
        if "--" not in plan or not plan.split("--", 1)[1].strip():
            return [f"{label}: a blocked plan must give a reason after --"]
        return []
    out = []
    for p in plan.split():
        if not re.fullmatch(r"GAP-\d+", p):
            out.append(f"{label}: plan token {p!r} is not a GAP id or 'blocked -- reason'")
        elif p not in gap_status:
            out.append(f"{label}: plan names {p}, which is not in the gap register")
        elif gap_status[p].startswith("Closed"):
            out.append(f"{label}: plan waits on {p}, which is closed -- either the code "
                       f"now carries this and the marker should go, or it needs a new plan")
    return out


def coverage_prose(disposition: str, maturity: dict[str, list[str]], cap: str) -> tuple[str, str]:
    """A `coverage` value rendered as the specification's two cells."""
    token, _, reason = disposition.partition("--")
    parts, reason = token.split(), reason.strip()
    if parts == ["verification-table"]:
        return ("Verification table",
                "a row in `../../../verification-capability-table.md` states the criterion")
    if parts[0] == "none":
        return "None", reason
    if re.fullmatch(r"I\d", parts[0]):
        # State what the roadmap says rather than concluding from it. "Partial at
        # increment 1" means part of this is already running, so "nothing is owed
        # yet" would be false for exactly the half that is.
        mat = maturity.get(cap, [])
        at_one = mat[0] if mat else "?"
        return (f"Increment {parts[0][1:]}",
                f"the roadmap has it *{at_one}* at increment 1, full at increment {parts[0][1:]}")
    if parts[0].startswith("GAP-"):
        return (", ".join(parts),
                "open in the gap register; the criterion arrives with the closure"
                + (f" ({reason})" if reason else ""))
    return (", ".join(parts),
            "stated by requirements that were derived from elsewhere, so the generated "
            "trace does not reach it" + (f"; {reason}" if reason else ""))


def regenerate_coverage_section(elements, rels) -> None:
    """Publish §8.1: every leaf capability no requirement was derived from.

    The traceability view has always pointed here -- "a capability with no
    requirement is listed in the specification's coverage section, not inferred" --
    and until this existed the pointer was dangling: §8 counts requirements by
    category and names no capability at all. So the generator reported 27 of them
    as notes, the view printed **none** against each, and the document they both
    deferred to was silent. Generated, because the list is the registry's to state;
    the disposition against each is the registry's `coverage` field, which is
    written by hand.
    """
    satisfied = {to for r in rels.get("satisfies", []) or [] for to in rel_targets(r)}
    _, _, maturity = coverage_evidence()
    rows = []
    for e in elements["capabilities"]:
        if "parent" not in e or e["id"] in satisfied:
            continue
        where, why = coverage_prose(e.get("coverage", ""), maturity, e["id"])
        rows.append(f'| {e["id"]} {e["name"]} | {where} | {why} |')
    body = [
        "",
        "### 8.1 Leaf capabilities no requirement was derived from",
        "",
        "Generated by `../../uaf/tools/build_uaf.py` from the registry's `coverage` field;",
        "do not edit by hand. The Source column of §1-§7 records where a requirement came",
        "*from*, so a requirement drawn from a principle, a decision or a budget names no",
        "capability however squarely it bears on one. These are the leaf capabilities left",
        "with no requirement of their own, and where each one's acceptance criterion comes",
        "from instead. The check refuses a capability that has neither, and refuses a",
        "disposition pointing at a gap that has closed or an increment the roadmap says is",
        "already finished.",
        "",
        f"| Capability | Criterion comes from | Which means |",
        "|---|---|---|",
        *rows,
        "",
        f"{len(rows)} of {sum(1 for e in elements['capabilities'] if 'parent' in e)} leaf "
        "capabilities. The count is a ratchet in `build_uaf.py`: writing a requirement for one",
        "of these lowers it, and it never rises.",
        "",
    ]
    text = read(REQUIREMENTS_SPEC)
    text = replace_block(text, "<!-- BEGIN generated coverage -->",
                         "<!-- END generated coverage -->", NL.join(body))
    write(REQUIREMENTS_SPEC, text)


def check(elements, rels) -> list[str]:
    problems, notes = [], []
    ids = names(elements)
    in_table, gap_status_for_plans, maturity = coverage_evidence()
    planned_rels: list[str] = []
    for e in elements["capabilities"]:
        if "parent" in e and e["parent"] not in ids:
            problems.append(f"{e['id']} has unknown parent {e['parent']}")
    # Which registry section each id belongs to, so a relationship can be checked
    # for pointing at the right KIND of thing and not merely at something that
    # exists. Without this, `{from: OP-30, to: [CAP-1.1]}` filed under `achieves`
    # passed silently and the capability-to-activity matrix then presented an
    # operational performer as an operational activity.
    section_of = {e["id"]: sec for sec, items in elements.items()
                  if isinstance(items, list) for e in items
                  if isinstance(e, dict) and "id" in e}
    for kind, entries in rels.items():
        expected = RELATIONSHIP_ENDPOINTS.get(kind)
        for r in entries or []:
            if r["from"] not in ids:
                problems.append(f"{kind}: unknown source {r['from']}")
            elif expected and section_of.get(r["from"]) != expected[0]:
                problems.append(f"{kind}: source {r['from']} is a {section_of.get(r['from'])} entry, "
                                f"but {kind} goes from {expected[0]}")
            if r.get("status") == "planned":
                # relationships.yaml's own header says the check treats planned as
                # satisfied BUT REPORTS IT. Only planned requirement carriers were
                # ever reported; planned relationships were silent, so two
                # activities passed "realized by a service" on relationships the
                # code does not carry.
                notes.append(f"{kind}: {r['from']} -> {', '.join(rel_targets(r)) or '(nothing)'} "
                             f"is planned, not in the code")
                planned_rels.append(f"{kind}: {r['from']}")
                problems.extend(planned_problems(kind, r, gap_status_for_plans))
            for to in rel_targets(r):
                if to not in ids:
                    problems.append(f"{kind}: unknown target {to} (from {r['from']})")
                elif expected and section_of.get(to) != expected[1]:
                    problems.append(f"{kind}: target {to} is a {section_of.get(to)} entry, "
                                    f"but {kind} goes to {expected[1]}")
    # A conforms_to and the standard it points at have to agree about whether the code
    # carries it. They had drifted apart in both directions and nothing said so: the
    # ASTERIX codec, the AIS and ADS-B adapters, UUIDv7, the HTTP/WebSocket transport
    # and TLS were all shipped while the registry still called them planned. Reading
    # each crate was how that was found, which is not a thing that happens on a
    # schedule -- so the two statements are now checked against each other instead.
    standard_status = {e["id"]: e.get("status") for e in elements.get("standards", [])}
    for r in rels.get("conforms_to", []) or []:
        planned = r.get("status") == "planned"
        for to in rel_targets(r):
            if to not in standard_status:
                continue
            if planned and standard_status[to] == "real":
                problems.append(f"conforms_to: {r['from']} -> {to} is planned, but {to} is a "
                                f"real standard -- one of the two is stale")
            if not planned and standard_status[to] == "planned":
                problems.append(f"conforms_to: {r['from']} -> {to} claims conformance, but {to} "
                                f"is planned -- one of the two is stale")
    if len(planned_rels) > RELATIONSHIPS_PLANNED:
        problems.append(f"{len(planned_rels)} relationships are planned, above the ratchet of "
                        f"{RELATIONSHIPS_PLANNED}: a planned edge was added. Build it, or raise "
                        f"the number deliberately rather than as a side effect")
    elif len(planned_rels) < RELATIONSHIPS_PLANNED:
        problems.append(f"only {len(planned_rels)} relationships are planned, below the ratchet "
                        f"of {RELATIONSHIPS_PLANNED}: set RELATIONSHIPS_PLANNED to "
                        f"{len(planned_rels)} so the gate holds the ground that was just taken")
    exhibited = {to for r in rels["exhibits"] for to in rel_targets(r)}
    achieved = {to for r in rels["achieves"] for to in rel_targets(r)}
    for e in elements["capabilities"]:
        if "parent" in e:
            if e["id"] not in exhibited:
                problems.append(f"{e['id']} is exhibited by no performer")
            if e["id"] not in achieved:
                notes.append(f"{e['id']} is achieved by no activity (cross-cutting)")
    realized = {to for r in rels["realizes"] for to in rel_targets(r)}
    human_roles = {e["id"] for e in elements["operational_performers"] if e.get("kind") == "role"}
    performed_by_human = {to for r in rels["performs"] if r["from"] in human_roles for to in rel_targets(r)}
    performed = {to for r in rels["performs"] for to in rel_targets(r)}
    for e in elements["operational_activities"]:
        if e["id"] not in realized and e["id"] not in performed_by_human:
            problems.append(f"{e['id']} is realized by no service and performed by no human")
        if e["id"] not in performed:
            problems.append(f"{e['id']} is performed by no performer")
    # Requirements (GAP-083): every one names at least one capability that exists, and a
    # leaf capability nobody requires is noted, because the specification's coverage
    # section is where that is decided, not here.
    satisfied = {to for r in rels.get("satisfies", []) or [] for to in rel_targets(r)}
    for e in elements.get("requirements", []):
        if not any(r["from"] == e["id"] for r in rels.get("satisfies", []) or []):
            if "CAP-" in e.get("source", ""):
                problems.append(f"{e['id']} names a capability that is not in the registry")
            else:
                notes.append(f"{e['id']} names no capability; sourced from {e.get('source') or 'nothing'}")
        for planned in e.get("planned_carriers") or []:
            notes.append(f"{e['id']} is carried by a crate that does not exist yet: {planned}")
    # `satisfies` is generated from the specification's Source column, so it says a
    # requirement was DERIVED FROM a capability, not that it bears on one: the
    # requirements sourced from principles, decisions and budgets name no capability
    # and correctly so. A leaf capability nobody derived a requirement from therefore
    # is not necessarily unspecified -- but it has to say where its criterion does
    # come from, which is what `coverage` carries and what §8.1 of the specification
    # publishes. Without that the capability is only visible as a note nobody acts on.
    gap_status = gap_status_for_plans
    requirement_ids = {e["id"] for e in elements.get("requirements", [])}
    unrequired = []
    for e in elements["capabilities"]:
        if "parent" not in e:
            continue
        if e["id"] in satisfied:
            if e.get("coverage"):
                notes.append(f"{e['id']} has a requirement and a coverage disposition; the "
                             f"disposition is stale and should be removed")
            continue
        unrequired.append(e["id"])
        if not e.get("coverage"):
            problems.append(f"{e['id']} is required by no requirement and has no coverage "
                            f"disposition; give it one or write the requirement")
            continue
        problems.extend(coverage_problems(e["id"], e["coverage"], requirement_ids,
                                          in_table, gap_status, maturity))
    if len(unrequired) > CAPABILITIES_WITHOUT_REQUIREMENT:
        problems.append(f"{len(unrequired)} leaf capabilities have no requirement, above the "
                        f"ratchet of {CAPABILITIES_WITHOUT_REQUIREMENT}: a capability was added "
                        f"without one. Write the requirement rather than raising the number")
    elif len(unrequired) < CAPABILITIES_WITHOUT_REQUIREMENT:
        problems.append(f"only {len(unrequired)} leaf capabilities now have no requirement, below "
                        f"the ratchet of {CAPABILITIES_WITHOUT_REQUIREMENT}: set "
                        f"CAPABILITIES_WITHOUT_REQUIREMENT to {len(unrequired)} so the gate holds "
                        f"the ground that was just taken")
    implemented = {to for r in rels["implements"] for to in rel_targets(r)}
    for e in elements["services"]:
        if e["id"] not in implemented:
            problems.append(f"{e['id']} is implemented by no resource")
        code = e.get("code", "")
        if not code:
            # An empty `code` used to pass: crate became "", ROOT / "" is ROOT,
            # ROOT exists, and `item` was empty so the source lookup was skipped
            # -- so "its code path exists" held for a service naming no code.
            problems.append(f"{e['id']} has no code path")
            continue
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
    # diagram references. RS_<crate> with an underscore is in the pattern because
    # that is the alias form the generated Rs-Cn detail diagrams actually emit --
    # 483 such tokens across them, and the pattern only matched `RS-` with a
    # hyphen, so not one reference in any of those twelve diagrams was checked.
    # The `replace("_", "-")` below was dead code for the same reason: nothing the
    # pattern matched could contain an underscore.
    pattern = re.compile(r"\b(CAP-\d\.\d+|OA-\d\d|OP-\d\d|SV-\d\d|RS[-_][a-z0-9_-]+"
                         r"|SD-\d\d|IE-\d\d|PT-\d\d|PJ-[A-Z0-9]+|AR-\d\d)\b")
    for src in sorted(glob.glob(str(UAF / "**" / "*.puml"), recursive=True)
                      + glob.glob(str(UAF / "**" / "*.mmd"), recursive=True)):
        text = read(Path(src))
        for tok in sorted(set(pattern.findall(text))):
            if tok not in ids and tok.replace("_", "-") not in ids:
                problems.append(f"{os.path.relpath(src, UAF)}: {tok} is not in the registry")
    # The rendered SVGs are an INPUT to the XMI export now, not just a picture:
    # export_xmi.py reads each view's element positions back out of its own render
    # so the EA diagram keeps the layout a human arranged. A render left stale by a
    # forgotten render.sh silently turns part of an authored layout into a grid, so
    # it is a problem here rather than something to notice in EA. Imported inside
    # the function because view_layout imports this module.
    from view_layout import layout_gaps, parse_all  # noqa: PLC0415
    problems.extend(layout_gaps(elements))
    views, view_warnings = parse_all(elements)
    notes.extend(view_warnings)
    # An edge a view draws that has the SHAPE of a registry relationship kind --
    # a performer to an activity is what `performs` relates -- but which
    # relationships.yaml does not state is the view and the registry
    # disagreeing, and the XMI export would otherwise carry it as a view-local
    # connector, which hides the disagreement instead of surfacing it. Either
    # the registry is missing the relationship or the view draws a wrong arrow;
    # both are problems here. The capability hierarchy counts as stated: it
    # lives in the `parent` field rather than in relationships.yaml.
    stated: set[tuple[str, str]] = set()
    for kind, entries in rels.items():
        for r in entries or []:
            for to in rel_targets(r):
                stated.add((r["from"], to))
                stated.add((to, r["from"]))
    for e in elements["capabilities"]:
        if e.get("parent"):
            stated.add((e["parent"], e["id"]))
            stated.add((e["id"], e["parent"]))
    signature = {ends: kind for kind, ends in RELATIONSHIP_ENDPOINTS.items()}
    for v in views:
        for edge in v.edges:
            if (edge.from_id, edge.to_id) in stated:
                continue
            kind = signature.get((section_of.get(edge.from_id), section_of.get(edge.to_id)))
            if kind and v.code not in EDGE_KIND_VIEWS.get(kind, {v.code}):
                continue
            if kind:
                problems.append(f"{v.source}: draws {edge.from_id} -> {edge.to_id}, which is shaped "
                                f"like `{kind}` but is not in relationships.yaml")
    return problems, notes


# ----------------------------------------------------------------------------- main
def main(argv: list[str]) -> int:
    check_only = "--check" in argv
    facts = crate_facts()
    undocumented: list[str] = []
    if not check_only:
        regenerate_registry(facts)
        regenerate_requirements(parse_requirements(), facts)
        undocumented = regenerate_model_information_elements()
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
        regenerate_coverage_section(elements, rels)
        print(f"generated views for {len(facts)} crates, {len(threads)} threads, {len(vignettes)} vignettes")
    problems, notes = check(elements, rels)
    # Not a problem: a type without a doc comment still reaches the registry and
    # EA, described by its shape. Reported so the gap is visible and can be
    # closed in the code, which is where that sentence belongs.
    if undocumented:
        notes.append(f"{len(undocumented)} gungnir-model types have no doc comment, so their "
                     f"registry description states their shape instead: {', '.join(undocumented)}")
    for n in notes:
        print("note:", n)
    for p in problems:
        print("PROBLEM:", p)
    print(f"registry check: {len(problems)} problems, {len(notes)} notes")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
