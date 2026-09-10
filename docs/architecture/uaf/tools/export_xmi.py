#!/usr/bin/env python3
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Export the UAF registry (model/elements.yaml, model/relationships.yaml) as an
XMI 2.1 / UML 2.1 file for import into Sparx Enterprise Architect's UAF MDG
Technology (plan 03 follow-up: a one-way, EA-facing bridge off the same registry
`build_uaf.py` already validates -- NOT a second source of truth).

Fourth version. Round 3 (built against four files Wayne exported from a real EA
project, see below) got UAFP stereotypes binding correctly for the first time,
but relationships and view diagrams were still missing. Wayne's own read of the
same sample files supplied the fix: EA organizes a UAF model as VIEW packages
(e.g. "Operational Processes Op-Pr" holding its own diagram plus its
OperationalPerformer/OperationalActivity elements) -- round 3 grouped elements
by registry section instead ("Operational Performers", "Operational
Activities", ...), which is a reasonable taxonomy but not the shape EA expects,
and it put every relationship in one flat "Relationships" package disconnected
from either endpoint's real package, which is very likely why none of them
were coming through. This version:

  - Groups elements into named UAF view packages (VIEW_PACKAGES below) instead
    of raw registry sections -- "Operational Structure Op-Sr",
    "Strategic Taxonomy St-Tx", etc., matching the real sample's naming and
    package/diagram/MDGView structure exactly where the sample covers it.
  - Places each relationship's Abstraction packagedElement in a dedicated
    Traceability view package (Op-Tr, confirmed by the sample; others
    extrapolated -- see RELATIONSHIP_KIND_INFO) rather than a flat
    disconnected package, following the same "one relationship kind, one
    source domain, one traceability package" rule the sample's own Op-Tr
    package demonstrates (it held both the Abstraction and its two endpoint
    classes together). `uses` (the plain Cargo.toml dependency, no UAF
    stereotype) is instead co-located directly in the Resources package
    alongside its own source elements.
  - Adds one diagram per ELEMENT-holding view package (not the Traceability
    packages -- see below), built from the real sample's own diagram XML:
    `<diagram>` with the confirmed `style1`/`style2`/`swimlanes`/
    `matrixitems` boilerplate (reused verbatim; these read as generic EA UI
    preferences, not content-specific data) and an `<elements>` list placing
    every locally-owned element plus the package-boundary frame on a simple
    auto-generated grid -- not a considered layout, and NOT extended to the
    Traceability packages, since showing elements from other packages plus
    connector lines needs cross-package `<elements>` entries and the
    connector-line geometry mini-language (`SX=...;EDGE=...`), both far less
    certain even with the sample in hand than everything above. Flagged
    explicitly, not silently skipped.

What is confirmed against the real sample vs. still a best-effort mapping:
see VIEW_PACKAGES, ELEMENT_KIND_INFO, RELATIONSHIP_KIND_INFO below -- each
confirmed entry says so in a trailing comment. Briefly: Capability,
OperationalPerformer, OperationalActivity, InformationElement, and Exhibits
(plus the package/diagram/MDGView shape for St-Tx/Op-Sr/Op-Pr/Op-Tr/If) are
confirmed; Services, Resources, Personnel, Standards, Projects, Actual
Resources, Requirements, and 8 of 9 relationship kinds are still the
OMG-profile-literature-informed guesses the first version made (a pass at the
OMG UAFP 1.1 specification PDF to firm these up did not yield clean answers --
its stereotype-definition pages are UML profile diagrams, and PDF text
extraction loses their visual structure, so a bare-text search kept
conflating unrelated mentions rather than resolving them).

Why XMI here and not SysML v2: EA's UAF support is built on the OMG UAF Profile
(UAFP), a UML profile exchanged via XMI; SysML v2 uses an unrelated
textual/API representation with no UAF binding.

Usage (from the workspace root):

    python docs/architecture/uaf/tools/export_xmi.py

Writes `docs/architecture/uaf/exports/gungnir-uaf.xmi`. Not part of `build_uaf.py`
or the CI drift check -- this is a manually-run, one-way export, same footing as
`render.sh`/`render.ps1`.
"""
from __future__ import annotations

import sys
from pathlib import Path
from xml.sax.saxutils import quoteattr

sys.path.insert(0, str(Path(__file__).resolve().parent))
from build_uaf import UAF, load_registry  # noqa: E402

# View package code -> (display title, MDG domain, MDG viewpoint). Domain/
# viewpoint feed the diagram's `MDGView=UAF {domain}::{viewpoint}` tag; a
# package still gets a plain (Logical-type, no MDGView) diagram when either is
# None -- there's just no UAF-specific view type to assert because none is
# confirmed or confidently guessable. The domain/viewpoint pair itself is checked
# against the OMG UAF 1.1 Domain Metamodel spec's own package index (every
# "Domain MetaModel::<Domain>::<Viewpoint>" heading in the document) for every
# row below -- all real, valid pairings, including the ones "not in sample"
# (the real EA export). What that index cannot confirm is the two-letter VIEW
# CODE (Rs-Cn, Sv-Cn, ...) or exact package title wording, which stay this
# repo's own convention (matching its existing Rs-Cn/Sv-Cn/etc. hand-authored
# views elsewhere under docs/architecture/uaf/), not something drawn from the
# spec.
VIEW_PACKAGES = {
    "St-Tx": ("Strategic Taxonomy", "Strategic", "Taxonomy"),              # confirmed (real EA sample)
    "Op-Sr": ("Operational Structure", "Operational", "Structure"),        # confirmed (real EA sample)
    "Op-Pr": ("Operational Processes", "Operational", "Processes"),        # confirmed (real EA sample)
    "If-Sr": ("Information Structure", "Information", "Information Model"),  # confirmed (real EA sample)
    "Op-Tr": ("Operational Traceability", "Operational", "Traceability"),  # confirmed (real EA sample)
    "Rs-Cn": ("Resource Connectivity", "Resources", "Connectivity"),       # domain/viewpoint confirmed (DMM spec); not in the EA sample
    "Sv-Cn": ("Service Connectivity", "Services", "Connectivity"),         # domain/viewpoint confirmed (DMM spec); not in the EA sample
    "Pr-Sr": ("Personnel Structure", "Personnel", "Structure"),            # domain/viewpoint confirmed (DMM spec); not in the EA sample
    "Sd-Tx": ("Standards Taxonomy", "Standards", "Taxonomy"),              # domain/viewpoint confirmed (DMM spec); not in the EA sample
    "Pj-Rm": ("Project Roadmap", "Projects", "Roadmap"),                   # domain/viewpoint confirmed (DMM spec); not in the EA sample
    "Ar-Cn": ("Actual Resources Connectivity", None, None),                # code kept for consistency with this repo's existing actual-resources/Ar-Cn.puml; the DMM spec's own domain index lists ONLY Taxonomy and Constraints under "Actual Resources" -- no Connectivity viewpoint exists there at all, so no MDGView is asserted rather than guessing one the spec doesn't support
    "Rq": ("Requirements", None, None),                                    # confirmed ABSENT: the DMM spec's own domain index has no "Requirements" domain at all, so no MDGView is guessable and none is attempted
    "Sv-Tr": ("Service Traceability", "Services", "Traceability"),         # domain/viewpoint confirmed (DMM spec); not in the EA sample
    "Rs-Tr": ("Resource Traceability", "Resources", "Traceability"),       # domain/viewpoint confirmed (DMM spec); not in the EA sample
    "Rq-Tr": ("Requirements Traceability", None, None),                    # see Rq above
}
# View packages that hold relationships rather than elements -- no diagram is
# attempted for these (see module docstring for why).
TRACEABILITY_PACKAGES = {"Op-Tr", "Sv-Tr", "Rs-Tr", "Rq-Tr"}

# Registry section -> (UAF stereotype, UML base metaclass, view package code).
ELEMENT_KIND_INFO = {
    "capabilities": ("Capability", "Class", "St-Tx"),                    # confirmed
    "operational_performers": ("OperationalPerformer", "Class", "Op-Sr"),  # confirmed
    "operational_activities": ("OperationalActivity", "Activity", "Op-Pr"),  # confirmed
    "services": ("ServiceInterface", "Class", "Sv-Cn"),
    "resources": ("ResourceArtifact", "Class", "Rs-Cn"),
    "personnel_types": ("PersonType", "Class", "Pr-Sr"),
    "standards": ("Standard", "Class", "Sd-Tx"),
    "projects": ("Project", "Class", "Pj-Rm"),
    "information_elements": ("InformationElement", "Class", "If-Sr"),    # confirmed (stereotype name; was "Information" -- wrong -- in round 1)
    "actual_resources": ("ActualResource", "InstanceSpecification", "Ar-Cn"),  # metaclass confirmed by the "Actual X" family pattern
    "requirements": ("Requirement", "Class", "Rq"),
}

# Registry relationship kind -> (UAF stereotype or None, UML base metaclass,
# view package code it's placed in). Abstraction is confirmed for `exhibits`
# and is the same base two other confirmed UAF relationship stereotypes
# (IsCapableToPerform, MapsToCapability) use; the traceability package per
# kind follows the sample's own "one traceability package per source domain"
# pattern (Op-Tr held both the Abstraction and its two endpoint classes).
# `uses` is deliberately not a UAF relationship: Cargo.toml's literal crate
# dependency graph, so it stays a plain uml:Dependency with no UAF stereotype,
# placed directly in Resources alongside its own source elements.
RELATIONSHIP_KIND_INFO = {
    "exhibits": ("Exhibits", "Abstraction", "Op-Tr"),        # confirmed
    "achieves": ("Achieves", "Abstraction", "Op-Tr"),
    "performs": ("Performs", "Abstraction", "Op-Tr"),
    "realizes": ("Realizes", "Abstraction", "Sv-Tr"),
    "implements": ("Implements", "Abstraction", "Rs-Tr"),
    "conforms_to": ("ConformsTo", "Abstraction", "Rs-Tr"),
    "uses": (None, "Dependency", "Rs-Cn"),
    "satisfies": ("Satisfies", "Abstraction", "Rq-Tr"),
    "carried_by": ("CarriedBy", "Abstraction", "Rq-Tr"),
}

# Fields already surfaced structurally (id is the xmi:id, name is the element
# name, description becomes ownedComment) -- everything else on an entry becomes
# a generic tagged value, so a future registry field needs no change here.
STRUCTURAL_FIELDS = {"id", "name", "description"}

# Verbatim from the real EA export (see module docstring); these read as
# generic EA UI preferences, not content specific to any one diagram.
STYLE1 = (
    "ShowPrivate=1;ShowProtected=1;ShowPublic=1;HideRelationships=0;Locked=0;Border=1;HighlightForeign=1;"
    "PackageContents=1;SequenceNotes=0;ScalePrintImage=0;PPgs.cx=1;PPgs.cy=1;DocSize.cx=826;DocSize.cy=1169;"
    "ShowDetails=0;Orientation=P;Zoom=100;ShowTags=0;OpParams=1;VisibleAttributeDetail=0;ShowOpRetType=1;"
    "ShowIcons=1;CollabNums=0;HideProps=0;ShowReqs=0;ShowCons=0;PaperSize=9;HideParents=0;UseAlias=0;"
    "HideAtts=0;HideOps=0;HideStereo=0;HideElemStereo=0;ShowTests=0;ShowMaint=0;ConnectorNotation=UML 2.1;"
    "ExplicitNavigability=0;ShowShape=1;AllDockable=0;AdvancedElementProps=1;AdvancedFeatureProps=1;"
    "AdvancedConnectorProps=1;m_bElementClassifier=1;SPT=1;ShowNotes=0;SuppressBrackets=0;"
    "SuppConnectorLabels=0;PrintPageHeadFoot=0;ShowAsList=0;"
)
SWIMLANES = (
    "locked=false;orientation=0;width=0;inbar=false;names=false;color=-1;bold=false;fcol=0;tcol=-1;"
    "ofCol=-1;ufCol=-1;hl=0;ufh=0;hh=0;cls=0;bw=0;hli=0;bro=0;SwimlaneFont=lfh:-16,lfw:0,lfi:0,lfu:0,"
    "lfs:0,lfface:Calibri,lfe:0,lfo:0,lfchar:1,lfop:0,lfcp:0,lfq:0,lfpf=0,lfWidth=0;"
)
MATRIXITEMS = "locked=false;matrixactive=false;swimlanesactive=true;kanbanactive=false;width=1;clrLine=0;"


def attr(s: object) -> str:
    return quoteattr(str(s))


def tag_xml(name: str, value: object) -> str:
    v = value
    if isinstance(v, list):
        v = ", ".join(str(x) for x in v)
    return f'          <tag name={attr(name)} value={attr(v)}/>\n'


def style2_xml(domain: str | None, viewpoint: str | None, save_tag: str) -> str:
    mdg_view = f"MDGView=UAF {domain}::{viewpoint};" if domain and viewpoint else ""
    return (
        f"SaveTag={save_tag};ExcludeRTF=0;DocAll=0;HideQuals=1;AttPkg=1;ShowTests=0;ShowMaint=0;"
        "SuppressFOC=1;MatrixActive=0;SwimlanesActive=1;KanbanActive=0;MatrixLineWidth=1;MatrixLineClr=0;"
        "MatrixLocked=0;TConnectorNotation=UML 2.1;TExplicitNavigability=0;AdvancedElementProps=1;"
        "AdvancedFeatureProps=1;AdvancedConnectorProps=1;m_bElementClassifier=1;SPT=1;"
        f"MDGDgm=SysML1.4::BlockDefinition;{mdg_view}"
        "STBLDgm=;ShowNotes=0;VisibleAttributeDetail=0;ShowOpRetType=1;SuppressBrackets=0;"
        "SuppConnectorLabels=0;PrintPageHeadFoot=0;ShowAsList=0;SuppressedCompartments=;SF=1;Theme=:119;"
    )


def element_xml(entry: dict, stereotype: str, metaclass: str) -> tuple[str, str, str]:
    """Returns (packagedElement XML, EA element-extension XML, UAF stereotype-
    application XML)."""
    eid = entry["id"]
    name = entry.get("name", eid)
    desc = entry.get("description", "")
    extra_attrs = ' isReadOnly="false" isSingleExecution="false"' if metaclass == "Activity" else ""
    body = [f'        <packagedElement xmi:type={attr("uml:" + metaclass)} xmi:id={attr(eid)} '
            f'name={attr(name)} visibility="public"{extra_attrs}>\n']
    if desc:
        body.append(f'          <ownedComment xmi:id={attr(eid + "-desc")} body={attr(desc)}/>\n')
    body.append('        </packagedElement>\n')

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
        f'        <packagedElement xmi:type={attr("uml:" + metaclass)} xmi:id={attr(rel_id)} '
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


def diagram_xml(pkg_code: str, pkg_id: str, title: str, domain: str | None,
                 viewpoint: str | None, member_ids: list[str], seq: int) -> str:
    """One diagram per element-holding view package: every locally-owned
    element on a simple grid, plus the package itself as a boundary frame --
    the shape confirmed in the real sample's own diagrams. Geometry is a
    mechanical grid, not a considered layout."""
    cols, box_w, box_h, gap, margin = 5, 140, 76, 20, 20
    els = []
    for i, eid in enumerate(member_ids):
        row, col = divmod(i, cols)
        left = margin + col * (box_w + gap)
        top = margin + row * (box_h + gap)
        duid = f"{(seq * 1000 + i) & 0xFFFFFFFF:08X}"
        els.append(f'          <element geometry={attr(f"Left={left};Top={top};Right={left + box_w};Bottom={top + box_h};")} '
                    f'subject={attr(eid)} seqno={attr(i + 1)} style={attr(f"HideIcon=0;DUID={duid};")}/>\n')
    n_rows = -(-len(member_ids) // cols) if member_ids else 1  # ceil div
    frame_right = margin + min(len(member_ids), cols) * (box_w + gap) + margin
    frame_bottom = margin + n_rows * (box_h + gap) + margin
    frame_duid = f"{(seq * 1000 + 999) & 0xFFFFFFFF:08X}"
    els.append(f'          <element geometry={attr(f"Left=10;Top=10;Right={frame_right};Bottom={frame_bottom};")} '
                f'subject={attr(pkg_id)} seqno={attr(len(member_ids) + 1)} style={attr(f"DUID={frame_duid};")}/>\n')

    diag_id = f"DGM-{pkg_code}"
    save_tag = f"{seq & 0xFFFFFFFF:08X}"
    return (
        f'      <diagram xmi:id={attr(diag_id)}>\n'
        f'        <model package={attr(pkg_id)} localID={attr(seq)} owner={attr(pkg_id)}/>\n'
        f'        <properties name={attr(title)} type="Logical"/>\n'
        f'        <style1 value={attr(STYLE1)}/>\n'
        f'        <style2 value={attr(style2_xml(domain, viewpoint, save_tag))}/>\n'
        f'        <swimlanes value={attr(SWIMLANES)}/>\n'
        f'        <matrixitems value={attr(MATRIXITEMS)}/>\n'
        f'        <extendedProperties/>\n'
        f'        <xrefs/>\n'
        f'        <elements>\n{"".join(els)}        </elements>\n'
        f'      </diagram>\n'
    )


def build(elements: dict, rels: dict) -> str:
    pkg_members: dict[str, list[str]] = {code: [] for code in VIEW_PACKAGES}
    pkg_element_xml: dict[str, list[str]] = {code: [] for code in VIEW_PACKAGES}
    ea_elements, uaf_stereotypes, connectors = [], [], []
    known_ids: set[str] = set()

    for section, entries in elements.items():
        if not isinstance(entries, list) or section not in ELEMENT_KIND_INFO:
            continue
        stereotype, metaclass, view_code = ELEMENT_KIND_INFO[section]
        for entry in entries:
            known_ids.add(entry["id"])
            cls_xml, ext_xml, uaf_xml = element_xml(entry, stereotype, metaclass)
            pkg_members[view_code].append(entry["id"])
            pkg_element_xml[view_code].append(cls_xml)
            ea_elements.append(ext_xml)
            uaf_stereotypes.append(uaf_xml)

    n = 0
    for kind, entries in rels.items():
        if kind not in RELATIONSHIP_KIND_INFO:
            continue
        stereotype, metaclass, view_code = RELATIONSHIP_KIND_INFO[kind]
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
                pkg_element_xml[view_code].append(rel_xml)
                ea_elements.append(elem_ext)
                connectors.append(conn_ext)
                if uaf_xml:
                    uaf_stereotypes.append(uaf_xml)

    packages, diagrams = [], []
    for i, (code, (title, domain, viewpoint)) in enumerate(VIEW_PACKAGES.items()):
        members = pkg_element_xml[code]
        if not members:
            continue
        pkg_id = f"PKG-{code}"
        full_title = f"{title} {code}" if code not in ("Rq", "Rq-Tr") else title
        packages.append(
            f'    <packagedElement xmi:type="uml:Package" xmi:id={attr(pkg_id)} name={attr(full_title)}>\n'
            + "".join(members)
            + '    </packagedElement>\n'
        )
        if code not in TRACEABILITY_PACKAGES and pkg_members[code]:
            diagrams.append(diagram_xml(code, pkg_id, full_title, domain, viewpoint, pkg_members[code], i + 1))

    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<xmi:XMI xmi:version="2.1" '
        'xmlns:xmi="http://schema.omg.org/spec/XMI/2.1" '
        'xmlns:uml="http://schema.omg.org/spec/UML/2.1" '
        'xmlns:EAUML="http://www.sparxsystems.com/profiles/EAUML/1.0" '
        'xmlns:UAF="http://www.omg.org/spec/UAF/20160505/UAF">\n'
        '  <uml:Model xmi:type="uml:Model" xmi:id="MODEL-gungnir-uaf" name="Gungnir UAF Model">\n'
        + "".join(packages)
        + '  </uml:Model>\n'
        + "".join(uaf_stereotypes)
        + '  <xmi:Extension extender="Enterprise Architect" extenderID="6.5">\n'
        '    <elements>\n'
        + "".join(ea_elements)
        + '    </elements>\n'
        '    <connectors>\n'
        + "".join(connectors)
        + '    </connectors>\n'
        '    <diagrams>\n'
        + "".join(diagrams)
        + '    </diagrams>\n'
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
    n_diagrams = xmi.count('<diagram xmi:id=')
    print(f"wrote {out_path}: {n_elements} elements, {n_rels} relationships, {n_diagrams} diagrams")
    return 0


if __name__ == "__main__":
    sys.exit(main())
