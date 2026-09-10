#!/usr/bin/env python3
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Export the UAF registry (model/elements.yaml, model/relationships.yaml) as an
XMI 2.1 / UML 2.1 file for import into Sparx Enterprise Architect's UAF MDG
Technology (plan 03 follow-up: a one-way, EA-facing bridge off the same registry
`build_uaf.py` already validates -- NOT a second source of truth).

Why XMI here and not SysML v2: EA's UAF support is built on the OMG UAF Profile
(UAFP), which is a UML profile exchanged via XMI; SysML v2 uses an unrelated
textual/API representation with no UAF binding, so it is not a fit for what EA
actually imports.

What this produces and what it does not guarantee:

  - Valid, well-formed XMI 2.1 / UML 2.1: one `uml:Package` per registry section,
    one `uml:Class` per element (`xmi:id` is the registry id itself, e.g. `RS-model`,
    so re-importing after a registry change diffs cleanly against what is already
    in the EA project). This much is guaranteed to import into EA as a browsable
    plain UML model even if nothing below this line binds on your installation --
    confirmed by a first-round import: the classes came through.
  - Relationships did NOT come through on that first round: a plain
    `uml:Dependency` packagedElement is, it turns out, the wrong shape for EA's
    importer -- EA's own model treats a relationship as a Connector attached to
    its two endpoint elements, not a free-floating package member the way a Class
    is, and its "Import Package from XMI" appears to only create Connectors from
    its own native `xmi:Extension` connector block, not from plain UML
    Dependency/Association packagedElements. So every relationship is now carried
    BOTH ways: still a `uml:Dependency` in the model proper (harmless, standards-
    correct, kept in case a different tool round-trips this file), and as a
    `<connectors><connector>` entry in the same `xmi:Extension
    extender="Enterprise Architect"` block that successfully carried the class
    stereotypes -- the exact tag names there (`source`/`target`, `ea_type`) are a
    second-best-effort, not verified against a live EA import yet, since round 1
    only told us classes-without-stereotypes-checked worked and connectors did
    not exist at all.
  - No diagrams: this export has never emitted any diagram XML (`uml:Diagram` or
    EA's `<diagrams>` extension). Nothing to verify here yet -- it is simply not
    built. Ask for it explicitly if you want a first pass; it is a distinct,
    larger piece of work (element layout coordinates, EA's diagram extension
    schema) rather than a one-line addition to what is here.
  - A best-effort UAF stereotype per element/relationship (`ResourceArtifact`,
    `Capability`, `OperationalPerformer`, ... -- names cross-checked against the
    OMG UAF 1.2 profile and a UAF-certified tool's stereotype list, not guessed),
    applied via the `xmi:Extension extender="Enterprise Architect"` block. Not yet
    confirmed whether the stereotype *name* itself binds to UAF MDG iconography
    or just imports as a plain Class/Connector with a stereotype label -- report
    back what you see. Every element and relationship also carries a
    `uafKind`/`uafRelationship` tagged value with our own registry vocabulary
    verbatim, so the mapping is recoverable regardless of whether the stereotype
    name binds.
  - Every other registry field (owner, status, code, source, layer, category,
    priority, ...) becomes an EA tagged value, and `description` becomes both an
    `ownedComment` and a `documentation`/`notes` tag, so nothing in the registry
    is dropped on export even if EA's importer only picks up a subset of it.

Usage (from the workspace root):

    python docs/architecture/uaf/tools/export_xmi.py

Writes `docs/architecture/uaf/exports/gungnir-uaf.xmi`. Not part of `build_uaf.py`
or the CI drift check -- this is a manually-run, one-way export, same footing as
`render.sh`/`render.ps1`.
"""
from __future__ import annotations

import sys
from pathlib import Path
from xml.sax.saxutils import escape, quoteattr

sys.path.insert(0, str(Path(__file__).resolve().parent))
from build_uaf import UAF, load_registry  # noqa: E402

# Registry section -> UAF stereotype. Cross-checked against the OMG UAF 1.2 UAFML
# profile and a UAF-certified modeling tool's published stereotype list (not
# guessed) -- see the module docstring for what to do if one does not bind in EA.
ELEMENT_STEREOTYPE = {
    "capabilities": "Capability",
    "operational_performers": "OperationalPerformer",
    "operational_activities": "OperationalActivity",
    "services": "Service",
    "resources": "ResourceArtifact",
    "personnel_types": "PersonType",
    "standards": "Standard",
    "projects": "Project",
    "information_elements": "Information",
    "actual_resources": "ActualResource",
    "requirements": "Requirement",
}

# Registry relationship kind -> UAF stereotype, kept spelled the way our own
# registry spells it (Title Case of the YAML key) rather than a third-party
# summary's naming, since that summary showed signs of merging distinct UAF
# relationships together. The `uafRelationship` tagged value on every dependency
# carries the exact registry kind regardless, so nothing is lost if this name
# does not match your EA's profile exactly.
RELATIONSHIP_STEREOTYPE = {
    "exhibits": "Exhibits",
    "achieves": "Achieves",
    "performs": "Performs",
    "realizes": "Realizes",
    "implements": "Implements",
    "conforms_to": "ConformsTo",
    "uses": "Uses",
    "satisfies": "Satisfies",
    "carried_by": "CarriedBy",
}

SECTION_TITLE = {
    "capabilities": "Capabilities",
    "operational_performers": "Operational Performers",
    "operational_activities": "Operational Activities",
    "services": "Services",
    "resources": "Resources",
    "personnel_types": "Personnel Types",
    "standards": "Standards",
    "projects": "Projects",
    "information_elements": "Information Elements",
    "actual_resources": "Actual Resources",
    "requirements": "Requirements",
}

# Fields already surfaced structurally (id is the xmi:id, name is the uml:Class
# name, description becomes ownedComment) -- everything else on an entry becomes
# a generic tagged value, so a future registry field needs no change here.
STRUCTURAL_FIELDS = {"id", "name", "description"}


def esc(s: object) -> str:
    return escape(str(s))


def attr(s: object) -> str:
    return quoteattr(str(s))


def tag_xml(name: str, value: object) -> str:
    v = value
    if isinstance(v, list):
        v = ", ".join(str(x) for x in v)
    return f'          <tag name={attr(name)} value={attr(v)}/>\n'


def class_xml(entry: dict, stereotype: str) -> tuple[str, str]:
    """Returns (uml:Class packagedElement XML, EA extension <element> XML)."""
    eid = entry["id"]
    name = entry.get("name", eid)
    desc = entry.get("description", "")
    body = [f'      <packagedElement xmi:type="uml:Class" xmi:id={attr(eid)} name={attr(name)}>\n']
    if desc:
        body.append(f'        <ownedComment xmi:id={attr(eid + "-desc")} body={attr(desc)}/>\n')
    body.append('      </packagedElement>\n')

    tags = [f'          <tag name="uafKind" value={attr(stereotype)}/>\n']
    for k, v in entry.items():
        if k in STRUCTURAL_FIELDS or v in (None, "", []):
            continue
        tags.append(tag_xml(k, v))
    ext = (
        f'      <element xmi:idref={attr(eid)} xmi:type="uml:Class">\n'
        f'        <properties stereotype={attr(stereotype)} documentation={attr(desc)}/>\n'
        f'        <tags>\n{"".join(tags)}        </tags>\n'
        f'      </element>\n'
    )
    return "".join(body), ext


def dependency_xml(rel_id: str, from_id: str, to_id: str, kind: str,
                    stereotype: str, extra: dict) -> tuple[str, str, str]:
    """Returns (uml:Dependency packagedElement XML, EA element-extension XML,
    EA connector-extension XML). The connector block is the one EA's importer
    is actually expected to read; the other two are kept for round-tripping to
    tools that do honour plain UML Dependency/stereotype-on-element XMI."""
    name = f"{kind}: {from_id} -> {to_id}"
    dep = (
        f'      <packagedElement xmi:type="uml:Dependency" xmi:id={attr(rel_id)} name={attr(name)}>\n'
        f'        <client xmi:idref={attr(from_id)}/>\n'
        f'        <supplier xmi:idref={attr(to_id)}/>\n'
        f'      </packagedElement>\n'
    )
    tags = [f'          <tag name="uafRelationship" value={attr(kind)}/>\n']
    for k, v in extra.items():
        if k in ("from", "to") or v in (None, "", []):
            continue
        tags.append(tag_xml(k, v))
    tags_xml = "".join(tags)

    elem_ext = (
        f'      <element xmi:idref={attr(rel_id)} xmi:type="uml:Dependency">\n'
        f'        <properties stereotype={attr(stereotype)}/>\n'
        f'        <tags>\n{tags_xml}        </tags>\n'
        f'      </element>\n'
    )
    connector_ext = (
        f'      <connector xmi:idref={attr(rel_id)}>\n'
        f'        <source xmi:idref={attr(from_id)}/>\n'
        f'        <target xmi:idref={attr(to_id)}/>\n'
        f'        <properties ea_type="Dependency" direction="Source -&gt; Destination" '
        f'stereotype={attr(stereotype)} name={attr(name)}/>\n'
        f'        <tags>\n{tags_xml}        </tags>\n'
        f'      </connector>\n'
    )
    return dep, elem_ext, connector_ext


def build(elements: dict, rels: dict) -> str:
    packages, ea_elements = [], []
    known_ids: set[str] = set()

    for section, entries in elements.items():
        if not isinstance(entries, list) or section not in ELEMENT_STEREOTYPE:
            continue
        stereotype = ELEMENT_STEREOTYPE[section]
        title = SECTION_TITLE[section]
        pkg_id = f"PKG-{section}"
        classes = []
        for entry in entries:
            known_ids.add(entry["id"])
            cls_xml, ext_xml = class_xml(entry, stereotype)
            classes.append(cls_xml)
            ea_elements.append(ext_xml)
        packages.append(
            f'    <packagedElement xmi:type="uml:Package" xmi:id={attr(pkg_id)} name={attr(title)}>\n'
            + "".join(classes)
            + '    </packagedElement>\n'
        )

    dependencies, connectors, n = [], [], 0
    for kind, entries in rels.items():
        if kind not in RELATIONSHIP_STEREOTYPE:
            continue
        stereotype = RELATIONSHIP_STEREOTYPE[kind]
        for entry in entries:
            from_id = entry.get("from")
            to = entry.get("to")
            to_list = to if isinstance(to, list) else [to]
            for to_id in to_list:
                if from_id not in known_ids or to_id not in known_ids:
                    continue  # id resolution is build_uaf.py's job (check()); skip quietly here
                n += 1
                rel_id = f"REL-{n}"
                dep_xml, elem_ext, conn_ext = dependency_xml(rel_id, from_id, to_id, kind, stereotype, entry)
                dependencies.append(dep_xml)
                ea_elements.append(elem_ext)
                connectors.append(conn_ext)
    rel_pkg = (
        '    <packagedElement xmi:type="uml:Package" xmi:id="PKG-relationships" name="Relationships">\n'
        + "".join(dependencies)
        + '    </packagedElement>\n'
    )

    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<xmi:XMI xmi:version="2.1" '
        'xmlns:xmi="http://schema.omg.org/spec/XMI/2.1" '
        'xmlns:uml="http://schema.omg.org/spec/UML/2.1">\n'
        '  <uml:Model xmi:type="uml:Model" xmi:id="MODEL-gungnir-uaf" name="Gungnir UAF Model">\n'
        + "".join(packages)
        + rel_pkg
        + '  </uml:Model>\n'
        '  <xmi:Extension extender="Enterprise Architect" extenderID="6.5">\n'
        '    <elements>\n'
        + "".join(ea_elements)
        + '    </elements>\n'
        '    <connectors>\n'
        + "".join(connectors)
        + '    </connectors>\n'
        '  </xmi:Extension>\n'
        '</xmi:XMI>\n'
    )


def main() -> int:
    elements, rels = load_registry()
    xmi = build(elements, rels)
    out_dir = UAF / "exports"
    out_dir.mkdir(exist_ok=True)
    out_path = out_dir / "gungnir-uaf.xmi"
    out_path.write_text(xmi, encoding="utf-8")
    n_elements = sum(len(v) for k, v in elements.items() if isinstance(v, list) and k in ELEMENT_STEREOTYPE)
    n_rels = xmi.count('xmi:type="uml:Dependency"')
    print(f"wrote {out_path}: {n_elements} elements, {n_rels} relationships")
    return 0


if __name__ == "__main__":
    sys.exit(main())
