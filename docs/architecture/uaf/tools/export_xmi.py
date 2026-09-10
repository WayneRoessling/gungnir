#!/usr/bin/env python3
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Export the UAF registry (model/elements.yaml, model/relationships.yaml) as an
XMI 2.1 / UML 2.1 file for import into Sparx Enterprise Architect's UAF MDG
Technology (plan 03 follow-up: a one-way, EA-facing bridge off the same registry
`build_uaf.py` already validates -- NOT a second source of truth).

Third version. The first two rounds guessed at EA's XMI dialect (plain
uml:Dependency, then a guessed xmi:Extension connector block) and both failed
to carry relationships -- classes came through, connectors did not. This
version is built against four files Wayne exported from a real EA project
(EA's own default UAF model template, exported as XMI 1.1, XMI 2.1, EA's
"native XML", and .xea) rather than guessed:

  - Stereotypes are applied via a DEDICATED profile-application element in
    EA's own `xmlns:UAF="http://www.omg.org/spec/UAF/20160505/UAF"` namespace,
    e.g. `<UAF:Capability base_Class="EAID_..."/>` -- a top-level sibling of
    the packagedElement it stereotypes, not an attribute on the element and
    not something living only in the `xmi:Extension` block. This was missing
    entirely from both prior rounds; it is very likely why stereotypes never
    bound even where elements/relationships otherwise came through.
  - Different UAF stereotypes extend different UML metaclasses, confirmed
    per-stereotype from the real export rather than assumed uniform:
    OperationalActivity/StandardOperationalActivity extend uml:Activity (not
    uml:Class); FieldedCapability/ActualCondition/ActualProjectMilestone
    extend uml:InstanceSpecification; most others (Capability,
    OperationalPerformer, InformationElement, ...) extend uml:Class.
  - UAF relationship stereotypes (Exhibits, IsCapableToPerform,
    MapsToCapability, confirmed) extend uml:Abstraction, not uml:Dependency --
    the second round's whole approach was the wrong base metaclass. The
    underlying packagedElement is `<packagedElement xmi:type="uml:Abstraction"
    xmi:id=".." supplier=".." client=".."/>` -- client/supplier ARE plain
    attributes (confirming round 1's instinct there; round 2's switch to
    nested <client>/<supplier> elements was an unnecessary and, per this real
    sample, incorrect "fix").
  - `InformationElement` is the real stereotype name (not `Information`,
    which was a guess in round 1 that nothing since corrected).

Why XMI here and not SysML v2: EA's UAF support is built on the OMG UAF Profile
(UAFP), a UML profile exchanged via XMI; SysML v2 uses an unrelated
textual/API representation with no UAF binding.

What is confirmed against the real sample vs. still a best-effort mapping for
kinds the sample didn't include (see ELEMENT_KIND_INFO/RELATIONSHIP_KIND_INFO
below for exactly which): the sample model has no Resources or Services
packages at all (EA's default UAF template doesn't seed them), so
`ResourceArtifact`/`ServiceInterface`/`PersonType`/`Standard`/`Project` and 8
of our 9 relationship kinds (only `exhibits` appears in the sample) are still
the OMG-profile-literature-informed guesses the first version made -- carried
forward because they're still the best available answer, not because they're
now verified. `uses` (the Cargo.toml resource-to-resource dependency) gets no
UAF stereotype at all -- it's a plain software dependency, not a UAF-defined
relationship, so it stays `uml:Dependency` with no `<UAF:...>` line.

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

# Registry section -> (UAF stereotype, UML base metaclass). Confirmed against a
# real EA export where marked; otherwise the best-effort OMG-profile mapping
# carried forward from the first version (see module docstring).
ELEMENT_KIND_INFO = {
    "capabilities": ("Capability", "Class"),                    # confirmed
    "operational_performers": ("OperationalPerformer", "Class"),  # confirmed
    "operational_activities": ("OperationalActivity", "Activity"),  # confirmed
    "services": ("ServiceInterface", "Class"),                  # not in sample
    "resources": ("ResourceArtifact", "Class"),                 # not in sample
    "personnel_types": ("PersonType", "Class"),                 # not in sample
    "standards": ("Standard", "Class"),                         # not in sample
    "projects": ("Project", "Class"),                           # not in sample
    "information_elements": ("InformationElement", "Class"),    # confirmed (was "Information" -- wrong -- in round 1)
    "actual_resources": ("ActualResource", "InstanceSpecification"),  # base confirmed by the "Actual X" family pattern (FieldedCapability/ActualCondition/ActualProjectMilestone all extend InstanceSpecification); the stereotype name itself is not directly in the sample
    "requirements": ("Requirement", "Class"),                   # not in sample; the sample's closest analogue ("Performance Requirement") is stereotyped Measurement on an *owned Property*, a shape that doesn't fit our flat list of requirement statements, so kept as a plain Class-based Requirement instead
}

# Registry relationship kind -> (UAF stereotype, UML base metaclass) or None for
# no UAF stereotype at all. Abstraction is confirmed for exhibits and is a
# reasonable default for the others in the same "one element realizes/fulfils
# a role for another" family (IsCapableToPerform and MapsToCapability, two
# other confirmed UAF relationship stereotypes in the sample, also extend
# Abstraction) -- not independently confirmed per kind. `uses` is deliberately
# not a UAF relationship: it is Cargo.toml's literal crate dependency graph, so
# it stays a plain uml:Dependency with no UAF stereotype at all.
RELATIONSHIP_KIND_INFO = {
    "exhibits": ("Exhibits", "Abstraction"),        # confirmed
    "achieves": ("Achieves", "Abstraction"),
    "performs": ("Performs", "Abstraction"),
    "realizes": ("Realizes", "Abstraction"),
    "implements": ("Implements", "Abstraction"),
    "conforms_to": ("ConformsTo", "Abstraction"),
    "uses": (None, "Dependency"),
    "satisfies": ("Satisfies", "Abstraction"),
    "carried_by": ("CarriedBy", "Abstraction"),
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

# Fields already surfaced structurally (id is the xmi:id, name is the element
# name, description becomes ownedComment) -- everything else on an entry becomes
# a generic tagged value, so a future registry field needs no change here.
STRUCTURAL_FIELDS = {"id", "name", "description"}


def attr(s: object) -> str:
    return quoteattr(str(s))


def tag_xml(name: str, value: object) -> str:
    v = value
    if isinstance(v, list):
        v = ", ".join(str(x) for x in v)
    return f'          <tag name={attr(name)} value={attr(v)}/>\n'


def element_xml(entry: dict, stereotype: str, metaclass: str) -> tuple[str, str, str]:
    """Returns (packagedElement XML, EA element-extension XML, UAF stereotype-
    application XML)."""
    eid = entry["id"]
    name = entry.get("name", eid)
    desc = entry.get("description", "")
    extra_attrs = ' isReadOnly="false" isSingleExecution="false"' if metaclass == "Activity" else ""
    body = [f'      <packagedElement xmi:type={attr("uml:" + metaclass)} xmi:id={attr(eid)} '
            f'name={attr(name)} visibility="public"{extra_attrs}>\n']
    if desc:
        body.append(f'        <ownedComment xmi:id={attr(eid + "-desc")} body={attr(desc)}/>\n')
    body.append('      </packagedElement>\n')

    tags = [f'          <tag name="uafKind" value={attr(stereotype)}/>\n']
    for k, v in entry.items():
        if k in STRUCTURAL_FIELDS or v in (None, "", []):
            continue
        tags.append(tag_xml(k, v))
    ext = (
        f'      <element xmi:idref={attr(eid)} xmi:type={attr("uml:" + metaclass)}>\n'
        f'        <properties stereotype={attr(stereotype)} documentation={attr(desc)}/>\n'
        f'        <tags>\n{"".join(tags)}        </tags>\n'
        f'      </element>\n'
    )
    uaf_stereo = f'    <UAF:{stereotype} base_{metaclass}={attr(eid)}/>\n'
    return "".join(body), ext, uaf_stereo


def relationship_xml(rel_id: str, from_id: str, to_id: str, kind: str,
                      stereotype: str | None, metaclass: str, extra: dict) -> tuple[str, str, str, str]:
    """Returns (packagedElement XML, EA element-extension XML, EA connector-
    extension XML, UAF stereotype-application XML or "")."""
    name = f"{kind}: {from_id} -> {to_id}"
    rel = (
        f'      <packagedElement xmi:type={attr("uml:" + metaclass)} xmi:id={attr(rel_id)} '
        f'name={attr(name)} visibility="public" supplier={attr(to_id)} client={attr(from_id)}/>\n'
    )
    tags = [f'          <tag name="uafRelationship" value={attr(kind)}/>\n']
    for k, v in extra.items():
        if k in ("from", "to") or v in (None, "", []):
            continue
        tags.append(tag_xml(k, v))
    tags_xml = "".join(tags)

    stereo_attr = f' stereotype={attr(stereotype)}' if stereotype else ""
    elem_ext = (
        f'      <element xmi:idref={attr(rel_id)} xmi:type={attr("uml:" + metaclass)}>\n'
        f'        <properties{stereo_attr}/>\n'
        f'        <tags>\n{tags_xml}        </tags>\n'
        f'      </element>\n'
    )
    connector_ext = (
        f'      <connector xmi:idref={attr(rel_id)}>\n'
        f'        <source xmi:idref={attr(from_id)}/>\n'
        f'        <target xmi:idref={attr(to_id)}/>\n'
        f'        <properties ea_type={attr(metaclass)} direction="Source -&gt; Destination"'
        f'{stereo_attr} name={attr(name)}/>\n'
        f'        <tags>\n{tags_xml}        </tags>\n'
        f'      </connector>\n'
    )
    uaf_stereo = f'    <UAF:{stereotype} base_{metaclass}={attr(rel_id)}/>\n' if stereotype else ""
    return rel, elem_ext, connector_ext, uaf_stereo


def build(elements: dict, rels: dict) -> str:
    packages, ea_elements, uaf_stereotypes = [], [], []
    known_ids: set[str] = set()

    for section, entries in elements.items():
        if not isinstance(entries, list) or section not in ELEMENT_KIND_INFO:
            continue
        stereotype, metaclass = ELEMENT_KIND_INFO[section]
        title = SECTION_TITLE[section]
        pkg_id = f"PKG-{section}"
        classes = []
        for entry in entries:
            known_ids.add(entry["id"])
            cls_xml, ext_xml, uaf_xml = element_xml(entry, stereotype, metaclass)
            classes.append(cls_xml)
            ea_elements.append(ext_xml)
            uaf_stereotypes.append(uaf_xml)
        packages.append(
            f'    <packagedElement xmi:type="uml:Package" xmi:id={attr(pkg_id)} name={attr(title)}>\n'
            + "".join(classes)
            + '    </packagedElement>\n'
        )

    relationships, connectors, n = [], [], 0
    for kind, entries in rels.items():
        if kind not in RELATIONSHIP_KIND_INFO:
            continue
        stereotype, metaclass = RELATIONSHIP_KIND_INFO[kind]
        for entry in entries:
            from_id = entry.get("from")
            to = entry.get("to")
            to_list = to if isinstance(to, list) else [to]
            for to_id in to_list:
                if from_id not in known_ids or to_id not in known_ids:
                    continue  # id resolution is build_uaf.py's job (check()); skip quietly here
                n += 1
                rel_id = f"REL-{n}"
                rel_xml, elem_ext, conn_ext, uaf_xml = relationship_xml(
                    rel_id, from_id, to_id, kind, stereotype, metaclass, entry)
                relationships.append(rel_xml)
                ea_elements.append(elem_ext)
                connectors.append(conn_ext)
                if uaf_xml:
                    uaf_stereotypes.append(uaf_xml)
    rel_pkg = (
        '    <packagedElement xmi:type="uml:Package" xmi:id="PKG-relationships" name="Relationships">\n'
        + "".join(relationships)
        + '    </packagedElement>\n'
    )

    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<xmi:XMI xmi:version="2.1" '
        'xmlns:xmi="http://schema.omg.org/spec/XMI/2.1" '
        'xmlns:uml="http://schema.omg.org/spec/UML/2.1" '
        'xmlns:EAUML="http://www.sparxsystems.com/profiles/EAUML/1.0" '
        'xmlns:UAF="http://www.omg.org/spec/UAF/20160505/UAF">\n'
        '  <uml:Model xmi:type="uml:Model" xmi:id="MODEL-gungnir-uaf" name="Gungnir UAF Model">\n'
        + "".join(packages)
        + rel_pkg
        + '  </uml:Model>\n'
        + "".join(uaf_stereotypes)
        + '  <xmi:Extension extender="Enterprise Architect" extenderID="6.5">\n'
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
    n_elements = sum(len(v) for k, v in elements.items() if isinstance(v, list) and k in ELEMENT_KIND_INFO)
    n_rels = xmi.count('<connector xmi:idref=')
    print(f"wrote {out_path}: {n_elements} elements, {n_rels} relationships")
    return 0


if __name__ == "__main__":
    sys.exit(main())
