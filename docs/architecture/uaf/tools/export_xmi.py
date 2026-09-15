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
  - Adds one diagram per view package, including the Traceability packages
    (round 5 extended this -- see below), built from the real sample's own
    diagram XML: `<diagram>` with the confirmed `style1`/`style2`/
    `swimlanes`/`matrixitems` boilerplate (reused verbatim; these read as
    generic EA UI preferences, not content-specific data) and an `<elements>`
    list placing every member plus the package-boundary frame on a simple
    auto-generated grid -- not a considered layout.

Fifth version. Round 4 (the view-package reorganization above) still left
Traceability packages empty, no diagrams, and no relationships showing on
elements. Wayne compared the sample's own "Operational Processes Op-Pr"
package directly against this script's output and found two concrete gaps:

  - Every element that participates in a relationship has a `<links>` entry
    in its own EA extension `<element>` block, one per relationship touching
    it (confirmed on both endpoints of a real Association in the sample) --
    this script built each element's extension entry independently of
    relationships and never added one. Fixed: `element_ext_xml()` now takes
    the accumulated links for that element, which means an element's own EA
    extension entry has to be built AFTER every relationship is processed,
    not alongside the element itself -- see `build()`'s two-pass structure.
  - A package is ALSO its own EA extension `<element>` entry (a different
    shape from a real element's: `<packageproperties>`/`<paths>`/`<times>`/
    `<flags>` instead of `<properties>`/`<tags>`), confirmed present in the
    sample for every package and omitted entirely from every round so far.
    Added via `package_ext_xml()`.

Neither was independently confirmed to be *the* fix for diagrams not
appearing -- that structure (the `<diagram>` block itself) already matched
the sample closely in round 4.

Sixth version. Wayne pointed at the sample's "Operational Processes" diagram
specifically and its `<elements>` list: it places `OperationalPerformer1` on
that diagram even though the element itself is owned by a different package
(`Operational Structure Op-Sr`), purely so the IsCapableToPerform connector to
it renders -- and that connector has NO entry of its own in the list. The
sample's own "Operational Traceability" diagram confirms the pattern the
other direction: it lists the *endpoint* elements of both relationships it
concerns (`OperationalPerformer1`, `Capability1`, `OperationalActivity1`), not
the relationships themselves. The first half of that reading is right and
still in force: a foreign-owned element is placed on a diagram by
`subject=<xmi:id>` regardless of which package owns it, so every view package
gets a diagram, Traceability packages included -- the deduplicated union of
every relationship endpoint it holds, accumulated during the same pass that
builds `links_by_id` (see `build()`).

The second half was wrong, and round 8 below falsified it against a live EA
import: it concluded that a connector line renders once both endpoints are
present, with no diagram entry of its own and the `SX=...;EDGE=...` geometry
being optional manual routing. A connector needs its own entry. Absent one,
no relationship renders on any diagram -- which is what rounds 6 and 7 saw.

Seventh version. Round 6 still produced no package diagrams and no
relationships -- but with one new, sharply diagnostic detail: each
OperationalActivity had acquired its own Activity diagram. Those are EA
auto-creating a behavior diagram per `uml:Activity` element, a side effect of
the metaclass rather than anything this script emits, which means EA was
importing the model tree happily while ignoring the `<diagrams>` and
`<connectors>` extension blocks entirely. Two structural differences from the
sample explain that, and both are now fixed:

  - ID convention. Every id in a real EA export is `EAPK_`+GUID for a package
    and `EAID_`+GUID for everything else (433 EAID_ / 18 EAPK_ in the sample,
    no exceptions), the GUID being EA's own `{...}` with hyphens turned into
    underscores. Rounds 1-6 used the registry id directly (`CAP-1.1`, `REL-1`,
    `PKG-Op-Tr`, `DGM-Op-Sr`). A standard XMI parser resolves any unique
    string as an idref -- which is exactly why elements and their `<UAF:...>`
    stereotype applications worked all along -- but EA's own extension parser
    reads the `<connectors>`/`<diagrams>` blocks, and ids it cannot map back
    to a GUID are the most plausible reason that half never landed. Ids are
    now deterministic MD5-derived GUIDs (`ea_guid()`), so output stays
    byte-identical run to run; the registry id survives as the element's
    `alias` and its `uafId` tagged value. (Round 8 settled both halves of
    that: EA does preserve these GUIDs verbatim, so the id scheme was never
    the problem it was introduced to solve -- but the `uafId` tag was being
    dropped outright until the tag shape was fixed, and `<alias>` appears
    nowhere in EA's own output, so it stays as a best-effort channel only.)
  - Root nesting. The sample is `<uml:Model>` (carrying no xmi:id at all) >
    one `EAPK_` root package > the view packages. Rounds 1-6 hung the view
    packages directly off `uml:Model` with no root package -- and "Import
    Package from XMI" imports *a package*.

Eighth version. The first round validated by an actual EA import-then-export
round trip rather than by reading someone else's export: a 5-element,
3-relationship probe went into EA, and EA's own XMI 2.1 and native-XML exports
of what it had loaded came back out (kept under `docs/architecture/uaf/
exports/ea-roundtrip/`, along with EA's export logs and a second export that
also covers EA's own default UAF model template). That round trip is the
authority for everything below marked "EA round trip". It confirmed the
`<diagram>`/`<connectors>`/`<links>` shapes rounds 4-7 had built, the
`MDGView=UAF <Domain>::<Viewpoint>` tag, and that EA preserves the
MD5-derived GUIDs verbatim -- and it falsified five things rounds 1-7 had
settled the wrong way:

  - A connector needs its OWN entry in each diagram's `<elements>` list.
    Round 6 concluded the opposite (see the note it left below, now deleted):
    that a line renders once both endpoints are placed. It does not. The entry
    carries `geometry="SX=0;SY=0;EX=0;EY=0;EDGE=n;..."`, no `seqno`, and
    `style="Mode=3;EOID=<target's DUID on this diagram>;SOID=<source's
    DUID>;..."`. Confirmed twice over: the probe's three connectors rendered
    only on the one diagram that carried such entries, and EA wrote them back
    with label geometry of its own (`LMT=CX=92:CY=14:...`) and two of the
    three `EDGE` values re-routed -- which it only does for connectors it
    genuinely owns on a diagram. Element entries come first, then connectors.
  - A relationship must NOT get an `<element>` entry in the extension's
    `<elements>` block. EA carries a connector in `<connectors>` only; every
    real export agrees, and the round trip wrote none. Rounds 4-7 emitted one
    per relationship (686 of them) in the block EA parses first.
  - Tagged values need `xmi:id` and `modelElement`, not just `name`/`value`.
    EA dropped every one of the probe's tags (all 23 `<tags>` blocks came back
    empty) while its own export carries 252 of them, all shaped
    `<tag xmi:id="EAID_..." name="..." value="..." modelElement="EAID_..."/>`.
    So rounds 1-7 lost `uafKind`, `uafId`, `uafRelationship` and every extra
    registry field on import, despite the docstring claiming they survived.
  - `<ownedComment>` is an EA *Note element*, not documentation. Each one
    became a separate nameless `Note` in the owning package, on top of the
    `documentation=` attribute that already carried the same text correctly.
    Dropped here; `documentation=` is the only description channel.
  - `Performs` is not a UAF stereotype name. It came back as
    `thecustomprofile:Performs` -- EA's catch-all for a stereotype it cannot
    resolve -- while the other seven came back under `UAF:`. EA's own UAF
    template uses `IsCapableToPerform` on a `uml:Abstraction` for exactly this
    performer-to-activity relation, which is what `performs` now emits. That
    fall-through is a mechanical oracle for the names still guessed below:
    import, export, and anything landing in `thecustomprofile` is wrong.

Two things rounds 4-7 had right and a reading of three non-UAF EA exports had
wrongly flagged, both settled by EA's own UAF template: a package IS placed on
its own diagram as a full-area boundary frame (12 of the template's 21
diagrams do it, with the same `Left=10;Top=10` geometry) -- but by its `EAID_`
id, never its `EAPK_` one, which is the bug that was actually there; and
`MDGDgm=SysML1.4::BlockDefinition` is right for a Logical UAF view, needing
`::StateMachine` for a States view and `::Sequence` for Interaction Scenarios.

Ninth version. The second round trip, of the full 59-diagram export rather than
a probe (exports/ea-roundtrip/run-2/). All 59 diagrams and all placements came
back, the tagged values survived, and no Note elements appeared, so round 8's
fixes held. It also carried something the probe run could not: EA's export
declares every stereotype of every enabled profile in its <profiles> block --
213 for the UAF profile, each with the metaclass it extends -- which turns the
"import and see what lands in thecustomprofile" oracle into a list to check
names against directly. Settled by it, and by what the import did:

  - Five names were wrong and the profile gave the right ones: Achieves is
    MapsToCapability, Realizes is Implements, PersonType is Post (the registry's
    PT-nn entries are posts, and the authored Pr-Sr view already says so). The
    two wrong relationship names had LOOKED bound, because EA matches a name
    case-insensitively against every enabled profile and BIZBOK has `achieves`
    and `realizes` -- a wrong name landing in a wrong profile, not just in
    thecustomprofile. ConformsTo, Satisfies and CarriedBy exist in no profile:
    UAF models conformance as a `conformsTo` property and requirements through
    SysML, whose `satisfy` runs from the satisfier to the requirement, so those
    two kinds now emit SysML:satisfy with the connector's endpoints swapped
    (SATISFIER_IS_TARGET) and conforms_to is a plain Dependency like `uses`.
  - The seven actual resources were dropped outright. Their model element was
    right (uml:InstanceSpecification, which is what EA's own template writes)
    but their extension entry was not: EA's own says xmi:type="uml:Object" and
    sType="Object" for such an element, and the profile itself declares
    ActualResource as extending Object. Fixed in element_ext_xml.
  - A tagged value whose name matches a PROPERTY of some stereotype in an
    enabled profile gets that stereotype applied on import: `owner` became
    UMM2:bLibrary on 159 elements, `source` FACE:FACE_Conversion on 94,
    `priority` MARTE:GaStep on 64, `category` UAF:Metadata on 64. Every
    registry-field tag is now prefixed `registry.` (REGISTRY_TAG_PREFIX).
  - EA places every connector whose two ends are on a diagram, whether or not
    the file listed it there: the Op-Cn view diagram came back with 67 lines
    for the 25 it was given, the rest being other views' connectors between the
    same performers. A view-local connector is therefore one per (from, to,
    label) across all views rather than one per view, tagged with every view
    that draws it, so what EA adds is the same connector and not a duplicate.
  - Two of my own defects, found while answering how the view-local connectors
    should be treated: the St-Tx view's 56 parent edges were becoming
    view-local connectors although the registry states the hierarchy in each
    capability's `parent` field (now emitted as relationships, PARENT_KIND),
    and the activity-view parser was chaining a performer named in a decision
    step into the step order, producing 14 edges shaped like `performs` that
    the registry rightly lacked. build_uaf.py's check now reports any view edge
    shaped like a registry kind but absent from relationships.yaml, so that
    class of disagreement fails the build rather than becoming a connector.

Note for anyone reading the output: inside an EA `<links>` block the child
elements carry `xmi:id`, not `xmi:idref`, even though they are references to
a relationship declared elsewhere (verified against the sample, which does
the same). So a relationship id legitimately appears three times -- once as
its `packagedElement` declaration and once in each endpoint's `<links>`.

What is confirmed: every stereotype name in ELEMENT_KIND_INFO and
RELATIONSHIP_KIND_INFO is now checked against the UAF profile's own declared
list (run-2's <profiles> block), and every one that is UAF's bound as UAF's on
the second round trip. The two that are SysML's (Requirement, satisfy) are the
correct reuse -- UAF has no requirements domain -- with satisfy resting on
SysML's vocabulary rather than on a list EA wrote, since SysML's block is not
declared in the export; the next round trip shows it under SysML: or
thecustomprofile:. Each table entry says which.

The authored views. Through round 8 this export carried the registry and one
mechanically gridded diagram per view package -- 15 diagrams, while this
directory authors 51 PlantUML views, six of whose view codes had no package here
at all. It now carries both: `view_layout.py` turns each `.puml` into an element
set, an edge list and a box layout, and `view_diagram_xml()` emits one EA diagram
per view, 44 of them (7 views name no registry element -- see that module).
Element positions come from each view's own render under `rendered/`, which is why
those renders are an INPUT to this export and why `build_uaf.py`'s check reports
one that has gone stale. An edge a view draws that the registry already carries
reuses that relationship's connector; one it does not becomes a view-local
connector, tagged `uafViewEdge` (see `view_edge_xml`), which is how the diagrams
can match their PlantUML without `relationships.yaml` gaining anything.

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

import hashlib
import sys
from pathlib import Path
from xml.sax.saxutils import quoteattr

sys.path.insert(0, str(Path(__file__).resolve().parent))
import view_layout  # noqa: E402
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
    "St-Tx": ("Strategic Taxonomy", "Strategic", "Taxonomy"),              # confirmed (EA round trip: bound, written back verbatim)
    "Op-Sr": ("Operational Structure", "Operational", "Structure"),        # confirmed (EA round trip)
    "Op-Pr": ("Operational Processes", "Operational", "Processes"),        # confirmed (EA round trip)
    "If-Sr": ("Information Structure", "Information", "Information Model"),  # viewpoint confirmed (EA's own template names the package "Information Model"; the title and the If-Sr code stay this repo's convention, matching information/If-Sr*.puml)
    "Op-Tr": ("Operational Traceability", "Operational", "Traceability"),  # confirmed (EA round trip)
    "Rs-Cn": ("Resource Connectivity", "Resources", "Connectivity"),       # domain/viewpoint confirmed (DMM spec); not in the EA sample
    "Sv-Cn": ("Service Connectivity", "Services", "Connectivity"),         # domain/viewpoint confirmed (DMM spec); not in the EA sample
    "Pr-Sr": ("Personnel Structure", "Personnel", "Structure"),            # domain/viewpoint confirmed (DMM spec); not in the EA sample
    "Sd-Tx": ("Standards Taxonomy", "Standards", "Taxonomy"),              # domain/viewpoint confirmed (DMM spec); not in the EA sample
    "Pj-Rm": ("Project Roadmap", "Projects", "Roadmap"),                   # domain/viewpoint per the DMM spec; NOT in EA's template, which names its two roadmap viewpoints "Deployment Roadmap" and "Phasing Roadmap" (both under Strategic) -- so "Projects::Roadmap" is the one MDGView here with positive reason to doubt it. Settle with the round-trip oracle.
    "Ar-Cn": ("Actual Resources Connectivity", None, None),                # code kept for consistency with this repo's existing actual-resources/Ar-Cn.puml; the DMM spec's own domain index lists ONLY Taxonomy and Constraints under "Actual Resources" -- no Connectivity viewpoint exists there at all, so no MDGView is asserted rather than guessing one the spec doesn't support
    "Rq": ("Requirements", None, None),                                    # confirmed ABSENT: the DMM spec's own domain index has no "Requirements" domain at all, so no MDGView is guessable and none is attempted
    "Sv-Tr": ("Service Traceability", "Services", "Traceability"),         # domain/viewpoint confirmed (DMM spec); not in the EA sample
    "Rs-Tr": ("Resource Traceability", "Resources", "Traceability"),       # domain/viewpoint confirmed (DMM spec); not in the EA sample
    "Rq-Tr": ("Requirements Traceability", None, None),                    # see Rq above
    # The six view codes below hold no registry section of their own. They exist
    # because this directory AUTHORS views under them (operational/Op-Cn.puml,
    # security/Sc-Cn.puml, ...) and those views now become EA diagrams -- see
    # view_layout.py. Before this, six of the fourteen authored view codes had no
    # package in the export at all.
    "Op-Cn": ("Operational Connectivity", "Operational", "Connectivity"),  # confirmed: EA's own template pairs this viewpoint with a Logical diagram, which is what these views are
    "St-Cn": ("Strategic Connectivity", "Strategic", "Connectivity"),      # confirmed, same way
    "If-Cn": ("Information Connectivity", None, None),                     # EA's template has no Information::Connectivity viewpoint, so none is asserted
    "Sc-Cn": ("Security Connectivity", None, None),                        # EA's template carries no Security domain at all
    # Op-Is and Op-St deliberately assert no MDGView. EA's template pairs
    # Operational::Interaction Scenarios with a Sequence diagram and
    # Operational::States with a Statechart, and what this export can build for
    # them is neither: a faithful Sequence view needs lifelines and Part-typed
    # participants this model does not carry, and the Op-St view's boxes are
    # InformationElements rather than State elements. Claiming the viewpoint while
    # emitting a Logical diagram would assert a pairing nothing supports, so the
    # diagrams say what they are in their own documentation instead.
    "Op-Is": ("Operational Interaction Scenarios", None, None),
    "Op-St": ("Operational States", None, None),
}

# Viewpoint -> (EA diagram type, MDGDgm value). EA's own UAF template uses a
# Logical/SysML1.4::BlockDefinition diagram for every viewpoint except two, and
# those two are recorded here so a States or Interaction Scenarios view added
# later gets the right diagram kind rather than a silently wrong Logical one.
# Taken from EA's export of its own default UAF model: 19 of its 21 diagrams
# are Logical/BlockDefinition, "Operational States" is Statechart/StateMachine
# and "Operational Interaction Scenarios" is Sequence/Sequence.
DEFAULT_DIAGRAM_KIND = ("Logical", "SysML1.4::BlockDefinition")
DIAGRAM_KIND_BY_VIEWPOINT = {
    "States": ("Statechart", "SysML1.4::StateMachine"),
    "Interaction Scenarios": ("Sequence", "SysML1.4::Sequence"),
}

# Registry section -> (UAF stereotype, UML base metaclass, view package code).
# Every stereotype below is now checked against the UAF profile itself, not
# against a sample or the literature: EA's second round-trip export
# (exports/ea-roundtrip/run-2/) carries a <profiles> block declaring all 213
# stereotypes of its UAF profile (nsPrefix UAFP) with the metaclass each
# extends. "in the profile" below means the name is in that list.
ELEMENT_KIND_INFO = {
    "capabilities": ("Capability", "Class", "St-Tx"),                    # in the profile; bound on both round trips
    "operational_performers": ("OperationalPerformer", "Class", "Op-Sr"),  # in the profile; bound on both round trips
    "operational_activities": ("OperationalActivity", "Activity", "Op-Pr"),  # in the profile; bound on both round trips
    "services": ("ServiceInterface", "Class", "Sv-Cn"),                  # in the profile; bound on round trip 2
    "resources": ("ResourceArtifact", "Class", "Rs-Cn"),                 # in the profile; bound on round trip 2
    # `Post`, not `PersonType`: PersonType is not in the UAF profile at all --
    # round trip 2 bound it to updm:PersonType, the UPDM predecessor profile that
    # happened to be enabled. UAF offers Person (a type of person) and Post (a
    # type of role in an organisation); the registry's PT-nn entries are posts
    # (Commander, Supervisor, Sensor manager ...) and the authored Pr-Sr view
    # already stereotypes them <<Post>>.
    "personnel_types": ("Post", "Class", "Pr-Sr"),
    "standards": ("Standard", "Class", "Sd-Tx"),                         # in the profile; bound on round trip 2
    "projects": ("Project", "Class", "Pj-Rm"),                           # in the profile; bound on round trip 2
    "information_elements": ("InformationElement", "Class", "If-Sr"),    # in the profile; bound on both round trips
    # In the profile, extending Object -- EA's name for an InstanceSpecification.
    # Round trip 2 DROPPED all seven of these on import: the model element was
    # right (uml:InstanceSpecification, as EA's own template writes it) but the
    # extension entry said xmi:type="uml:InstanceSpecification"/sType=
    # "InstanceSpecification", and EA's own entry for such an element says
    # uml:Object / sType="Object". See element_ext_xml.
    "actual_resources": ("ActualResource", "InstanceSpecification", "Ar-Cn"),
    # Not in the UAF profile: UAF has no requirement of its own and reuses
    # SysML's. Round trip 2 resolved this to SysML:requirement, which is the
    # correct binding, so the name is kept and it is emitted in the SysML
    # namespace (see stereo_app).
    "requirements": ("SysML:Requirement", "Class", "Rq"),
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
    "exhibits": ("Exhibits", "Abstraction", "Op-Tr"),        # in the profile; bound on both round trips
    # `MapsToCapability`, not `Achieves`: Achieves is not in the UAF profile.
    # Round trip 2 bound it, case-insensitively, to BIZBOK:achieves -- a business
    # architecture profile that happened to be enabled -- which is how a wrong
    # name can look bound. MapsToCapability is the profile's relation from an
    # activity to the capability it maps to, and EA's own UAF template uses it.
    "achieves": ("MapsToCapability", "Abstraction", "Op-Tr"),
    # Was "Performs" through round 7; the profile's name for a performer's
    # relation to an activity it can perform is IsCapableToPerform.
    "performs": ("IsCapableToPerform", "Abstraction", "Op-Tr"),
    # `Implements`, not `Realizes`: Realizes is not in the profile (round trip 2
    # bound it to BIZBOK:realizes). Implements is the profile's relation from a
    # service or resource element to the operational element it implements, so
    # it is right for both this kind and `implements` below; the two stay
    # distinguishable by their uafRelationship tag and their package.
    "realizes": ("Implements", "Abstraction", "Sv-Tr"),
    "implements": ("Implements", "Abstraction", "Rs-Tr"),   # in the profile; bound on round trip 2
    # No stereotype: the profile has no relationship for conformance to a
    # standard at all (nothing with "Conform" in its name among its 213). UAF
    # models it as a `conformsTo` PROPERTY on the conforming element, which EA's
    # own template carries as a reference-valued tag. So this is a plain
    # Dependency like `uses`, and the registry kind survives in its tag. Emitting
    # the property is a follow-up: its reference-value encoding is unconfirmed.
    "conforms_to": (None, "Dependency", "Rs-Tr"),
    "uses": (None, "Dependency", "Rs-Cn"),
    # SysML `satisfy`, not `Satisfies`/`CarriedBy`, neither of which any profile
    # has. UAF has no requirements domain and reuses SysML's, whose relation runs
    # from the SATISFIER to the requirement -- the opposite direction from the
    # registry's `REQ -> capability`, so build() swaps the connector's endpoints
    # for these two kinds (SATISFIER_IS_TARGET). SysML's profile block is not
    # declared in EA's export, so unlike every other name in this table this one
    # rests on SysML's own vocabulary rather than on a list EA wrote; the round-
    # trip oracle will show it under SysML: or thecustomprofile:.
    "satisfies": ("SysML:satisfy", "Abstraction", "Rq-Tr"),
    "carried_by": ("SysML:satisfy", "Abstraction", "Rq-Tr"),
}

# Registry kinds stated as `requirement -> satisfier` that SysML states the other
# way round. The registry direction is kept in the relationship's key and tag;
# only the connector's client/supplier are swapped, so EA's satisfy arrow points
# from the capability or resource to the requirement, as SysML defines it.
SATISFIER_IS_TARGET = {"satisfies", "carried_by"}

# The capability hierarchy is registry data too, held as a `parent` field on each
# capability rather than as a relationships.yaml entry. It is emitted as a
# relationship in its own right so the St-Tx views can draw it by reference: EA's
# own UAF template draws capability-to-capability as a plain, unstereotyped
# Dependency, which is what this becomes.
PARENT_KIND = "parent"


def stereo_app(stereotype: str, base_metaclass: str, target_id: str) -> str:
    """One stereotype-application element. A stereotype is `Name` (the UAF
    profile) or `Profile:Name` for one from another profile -- SysML's
    requirement and satisfy are the two in use."""
    prefix, _, name = stereotype.rpartition(":")
    return f'    <{prefix or "UAF"}:{name} base_{base_metaclass}={attr(target_id)}/>\n'


def stereo_name(stereotype: str | None) -> str | None:
    """The bare name, for the `stereotype=` attribute in an extension entry."""
    return stereotype.rpartition(":")[2] if stereotype else None

def ea_guid(key: str) -> str:
    """A GUID-shaped id body (8_4_4_4_12 hex, underscore-separated) derived
    deterministically from a registry id, so re-running produces byte-identical
    output and re-importing diffs cleanly against what is already in EA.

    Why not just use the registry id (`CAP-1.1`, `REL-1`) as the xmi:id, which
    is what rounds 1-6 did: every id in a real EA export is `EAPK_`+GUID for a
    package and `EAID_`+GUID for everything else (433 EAID_ / 18 EAPK_ ids in
    the sample, no exceptions), where the GUID body is EA's own `{...}` GUID
    with hyphens turned into underscores. A plain XMI parser resolves any
    unique string as an idref, which is why elements and their `<UAF:...>`
    stereotype applications imported fine all along -- but EA's own extension
    parser, which is what reads the `<connectors>` and `<diagrams>` blocks,
    is the half that never worked, and non-GUID ids it cannot map back to a
    GUID are the most plausible reason. The registry id stays recoverable: it
    is the element's `alias` and its `uafId` tagged value."""
    h = hashlib.md5(key.encode("utf-8")).hexdigest().upper()
    return f"{h[0:8]}_{h[8:12]}_{h[12:16]}_{h[16:20]}_{h[20:32]}"


def eaid(key: str) -> str:
    """Element/connector/diagram id, EA's `EAID_` convention."""
    return f"EAID_{ea_guid(key)}"


def eapk(key: str) -> str:
    """Package id, EA's `EAPK_` convention -- a package additionally has an
    `EAID_` form of the same GUID (the sample's package extension entries
    carry both: `package2="EAID_<guid>" package="EAPK_<parent guid>"`)."""
    return f"EAPK_{ea_guid(key)}"


ROOT_PKG_KEY = "gungnir-uaf-root"

# A fixed timestamp, not the wall clock: every created/modified stamp this
# export writes has to be constant or the output stops being byte-identical
# run to run, which is what lets CI diff it against what is committed.
STAMP = "2026-09-04 00:00:00"

# Fields already surfaced structurally (id is the xmi:id, name is the element
# name, description is the extension entry's documentation) -- everything else
# on an entry becomes a generic tagged value, so a future registry field needs no
# change here.
STRUCTURAL_FIELDS = {"id", "name", "description"}

# Every registry field's tag name is prefixed, because a bare field name is not
# safe in EA. On import EA reconciles a tagged value against every ENABLED
# profile, and a tag whose name matches a property of some stereotype gets that
# stereotype applied: round trip 2 turned `owner` into UMM2:bLibrary on 159
# elements, `source` into FACE:FACE_Conversion on 94, `priority` into MARTE:GaStep
# on 64 and `category` into UAF:Metadata on 64, each carrying the tag's value as
# that stereotype's property. No profile has a dotted property name.
REGISTRY_TAG_PREFIX = "registry."

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
# Present on every diagram in EA's own exports and omitted by rounds 1-7.
PERSISTENTSTYLE = "DGS=On=0:CNT=8:W=120:H=40:SG=0:SGH=0:AEB=0:;AR=0;DCL=0;"


def attr(s: object) -> str:
    return quoteattr(str(s))


def tag_xml(owner_id: str, owner_key: str, name: str, value: object) -> str:
    """One EA tagged value.

    `xmi:id` and `modelElement` are both required: EA's importer silently drops
    a `<tag>` carrying only name/value, which is what rounds 1-7 emitted and why
    no registry field survived an import. Every one of the 252 tags in EA's own
    export carries an `xmi:id`, and 206 of them a `modelElement` pointing back
    at the element or connector that owns the tag (EA round trip).

    The tag's own id is derived from owner plus tag name so it stays stable
    across runs, the same way every other id here is.
    """
    v = value
    if isinstance(v, list):
        v = ", ".join(str(x) for x in v)
    return (f'          <tag xmi:id={attr(eaid(f"tag:{owner_key}:{name}"))} name={attr(name)} '
            f'value={attr(v)} modelElement={attr(owner_id)}/>\n')


def diagram_kind(viewpoint: str | None) -> tuple[str, str]:
    """(EA diagram type, MDGDgm) for a viewpoint -- see DIAGRAM_KIND_BY_VIEWPOINT."""
    return DIAGRAM_KIND_BY_VIEWPOINT.get(viewpoint or "", DEFAULT_DIAGRAM_KIND)


def style2_xml(domain: str | None, viewpoint: str | None, save_tag: str) -> str:
    mdg_view = f"MDGView=UAF {domain}::{viewpoint};" if domain and viewpoint else ""
    mdg_dgm = diagram_kind(viewpoint)[1]
    # SaveTag last, matching EA's own output; it is a plain key=value list, so
    # position should not matter, but there is no reason to differ.
    return (
        "ExcludeRTF=0;DocAll=0;HideQuals=1;AttPkg=1;ShowTests=0;ShowMaint=0;"
        "SuppressFOC=1;MatrixActive=0;SwimlanesActive=1;KanbanActive=0;MatrixLineWidth=1;MatrixLineClr=0;"
        "MatrixLocked=0;TConnectorNotation=UML 2.1;TExplicitNavigability=0;AdvancedElementProps=1;"
        "AdvancedFeatureProps=1;AdvancedConnectorProps=1;m_bElementClassifier=1;SPT=1;"
        f"MDGDgm={mdg_dgm};{mdg_view}"
        "STBLDgm=;ShowNotes=0;VisibleAttributeDetail=0;ShowOpRetType=1;SuppressBrackets=0;"
        "SuppConnectorLabels=0;PrintPageHeadFoot=0;ShowAsList=0;SuppressedCompartments=;SF=1;Theme=:119;"
        f"SaveTag={save_tag};"
    )


def element_xml(entry: dict, stereotype: str, metaclass: str) -> tuple[str, str]:
    """Returns (packagedElement XML, UAF stereotype-application XML). The EA
    element-extension XML is built separately, later, by element_ext_xml --
    it needs to know which relationships touch this element first (its
    `<links>` list), which isn't known until every relationship is processed."""
    reg_id = entry["id"]
    eid = eaid(reg_id)
    name = entry.get("name", reg_id)
    extra_attrs = ' isReadOnly="false" isSingleExecution="false"' if metaclass == "Activity" else ""
    # The registry id rides along as the UML alias (EA shows it in the Alias
    # column) as well as the uafId tagged value, since the xmi:id is now a
    # GUID rather than the id itself.
    #
    # No <ownedComment> for the description: in EA that is a Note *element*, not
    # documentation. Rounds 3-7 emitted one per described element and EA
    # imported each as a separate nameless Note sitting in the owning package
    # (EA round trip), on top of the `documentation=` attribute in the element's
    # extension entry, which already carries the same text and is the channel
    # EA actually shows in the Notes field.
    body = (f'        <packagedElement xmi:type={attr("uml:" + metaclass)} xmi:id={attr(eid)} '
            f'name={attr(name)} visibility="public"{extra_attrs}/>\n')
    return body, stereo_app(stereotype, metaclass, eid)


def element_ext_xml(reg_id: str, stereotype: str, metaclass: str, desc: str, entry: dict,
                     pkg_key: str, links: list[tuple[str, str, str, str]]) -> str:
    """The EA element-extension XML for a real (non-package) element, with a
    `<links>` entry per relationship that touches it (metaclass, rel_id,
    start_id, end_id) -- confirmed present on both endpoints of a real
    relationship in the sample."""
    eid = eaid(reg_id)
    tags = [tag_xml(eid, reg_id, "uafKind", stereo_name(stereotype)),
            tag_xml(eid, reg_id, "uafId", reg_id)]
    for k, v in entry.items():
        if k in STRUCTURAL_FIELDS or v in (None, "", []):
            continue
        tags.append(tag_xml(eid, reg_id, REGISTRY_TAG_PREFIX + k, v))
    # EA's name for an InstanceSpecification is Object, in the extension entry
    # only: its own export writes the model element as uml:InstanceSpecification
    # and the entry as xmi:type="uml:Object" sType="Object". Round trip 2 dropped
    # every element whose entry said InstanceSpecification.
    ext_type = "Object" if metaclass == "InstanceSpecification" else metaclass
    links_xml = "".join(
        f'          <{lm} xmi:id={attr(lid)} start={attr(start)} end={attr(end)}/>\n'
        for lm, lid, start, end in links
    )
    return (
        f'      <element xmi:idref={attr(eid)} xmi:type={attr("uml:" + ext_type)} '
        f'name={attr(entry.get("name", reg_id))} scope="public">\n'
        f'        <model package={attr(eapk(pkg_key))} tpos="0" ea_eleType="element"/>\n'
        f'        <properties isSpecification="false" sType={attr(ext_type)} nType="0" scope="public" '
        f'stereotype={attr(stereo_name(stereotype))} documentation={attr(desc)}/>\n'
        f'        <project author="gungnir" version="1.0" phase="1.0" created={attr(STAMP)} '
        f'modified={attr(STAMP)} complexity="1" status="Proposed"/>\n'
        '        <style appearance="BackColor=-1;BorderColor=-1;BorderWidth=-1;FontColor=-1;'
        'VSwimLanes=1;HSwimLanes=1;BorderStyle=0;"/>\n'
        f'        <alias alias={attr(reg_id)}/>\n'
        f'        <tags>\n{"".join(tags)}        </tags>\n'
        '        <xrefs/>\n'
        '        <extendedProperties tagged="0"/>\n'
        f'        <links>\n{links_xml}        </links>\n'
        f'      </element>\n'
    )


def package_ext_xml(pkg_key: str, title: str, parent_key: str) -> str:
    """Every package gets its own EA extension element entry too, confirmed in
    the real sample -- a different shape from a real element's
    (`<packageproperties>`/`<paths>`/`<times>`/`<flags>` instead of
    `<properties>`/`<tags>`). Note `package2`: a package carries BOTH the
    `EAID_` and `EAPK_` form of its own GUID, exactly as the sample does."""
    return (
        f'      <element xmi:idref={attr(eapk(pkg_key))} xmi:type="uml:Package" name={attr(title)} scope="public">\n'
        f'        <model package2={attr(eaid(pkg_key))} package={attr(eapk(parent_key))} ea_eleType="package"/>\n'
        f'        <properties isSpecification="false" sType="Package" nType="0" scope="public"/>\n'
        f'        <packageproperties version="1.0"/>\n'
        f'        <paths/>\n'
        f'        <times created={attr(STAMP)} modified={attr(STAMP)}/>\n'
        f'        <flags iscontrolled="0" isprotected="0" batchsave="0" batchload="0" usedtd="0" logxml="0"/>\n'
        f'      </element>\n'
    )


def relationship_xml(rel_key: str, from_id: str, to_id: str, kind: str,
                      stereotype: str | None, metaclass: str, extra: dict) -> tuple[str, str, str]:
    """Returns (packagedElement XML, EA connector-extension XML, UAF
    stereotype-application XML or "").

    There is deliberately no EA *element*-extension entry for a relationship.
    EA carries a connector in `<connectors>` and nowhere else: its own export
    writes no `<element xmi:idref>` for any connector, and neither does any
    real export checked. Rounds 4-7 emitted one per relationship -- 686 entries
    in `<elements>`, the first block EA's extension parser reads.
    """
    name = f"{kind}: {from_id} -> {to_id}"
    rid, from_eid, to_eid = eaid(rel_key), eaid(from_id), eaid(to_id)
    rel = (
        f'        <packagedElement xmi:type={attr("uml:" + metaclass)} xmi:id={attr(rid)} '
        f'name={attr(name)} visibility="public" supplier={attr(to_eid)} client={attr(from_eid)}/>\n'
    )
    tags = [tag_xml(rid, rel_key, "uafRelationship", kind)]
    for k, v in extra.items():
        if k in ("from", "to") or v in (None, "", []):
            continue
        tags.append(tag_xml(rid, rel_key, REGISTRY_TAG_PREFIX + k, v))
    tags_xml = "".join(tags)

    stereo_attr = f' stereotype={attr(stereo_name(stereotype))}' if stereotype else ""
    # `name` belongs on <connector> and in <labels mt=...>, which is where EA
    # puts it and what it writes back; it is not an attribute of the connector's
    # <properties> (EA's own carry only ea_type/direction/stereotype there).
    connector_ext = (
        f'      <connector xmi:idref={attr(rid)} name={attr(name)}>\n'
        f'        <source xmi:idref={attr(from_eid)}>\n'
        f'          <role visibility="Public" targetScope="instance"/>\n'
        f'          <type aggregation="none" containment="Unspecified"/>\n'
        f'          <modifiers isOrdered="false" changeable="none" isNavigable="false"/>\n'
        f'        </source>\n'
        f'        <target xmi:idref={attr(to_eid)}>\n'
        f'          <role visibility="Public" targetScope="instance"/>\n'
        f'          <type aggregation="none" containment="Unspecified"/>\n'
        f'          <modifiers isOrdered="false" changeable="none" isNavigable="true"/>\n'
        f'        </target>\n'
        f'        <properties ea_type={attr(metaclass)} direction="Source -&gt; Destination"'
        f'{stereo_attr}/>\n'
        f'        <modifiers isRoot="false" isLeaf="false"/>\n'
        '        <appearance linemode="3" linecolor="-1" linewidth="0" seqno="0" headStyle="0" lineStyle="0"/>\n'
        f'        <labels mt={attr(name)}/>\n'
        f'        <tags>\n{tags_xml}        </tags>\n'
        '        <xrefs/>\n'
        f'      </connector>\n'
    )
    uaf_stereo = stereo_app(stereotype, metaclass, rid) if stereotype else ""
    return rel, connector_ext, uaf_stereo


def diagram_duid(pkg_key: str, reg_id: str) -> str:
    """The DUID of one element's placement on one diagram. A connector's diagram
    entry identifies its two ends by these, not by element id, so the connector
    pass has to be able to recompute them -- hence a named function rather than
    an expression inside the element loop."""
    return ea_guid(f"{pkg_key}:{reg_id}")[:8]


def diagram_xml(pkg_key: str, title: str, domain: str | None, viewpoint: str | None,
                 member_reg_ids: list[str], placed_rels: list[tuple[str, str, str]],
                 seq: int) -> str:
    """One diagram per view package: every member on a simple grid, then every
    relationship whose two endpoints are both on this diagram, then the package
    itself as a full-area boundary frame. Members are referenced by their EAID_
    id whichever package owns them, which is how EA places a foreign-owned
    element too. Geometry is a mechanical grid, not a considered layout.

    `placed_rels` is (rel_key, from_id, to_id) for each relationship to draw.
    A connector entry carries no `seqno`, an `SX/SY/EX/EY/EDGE` geometry instead
    of a bounding box, and `Mode=3;EOID=<end DUID>;SOID=<start DUID>` -- all
    three confirmed by EA writing the probe's connectors back with its own
    routing applied. Without this entry the connector exists in the model but
    appears on no diagram, which is what rounds 4-7 shipped.

    The frame's `subject` is the package's EAID_ id, never its EAPK_ one: EA's
    own UAF template frames 12 of its 21 diagrams exactly this way, and no
    EAPK_ id appears as a diagram subject anywhere in its export.
    """
    cols, box_w, box_h, gap, margin = 5, 140, 76, 20, 20
    pkg_id = eapk(pkg_key)
    els = []
    for i, reg_id in enumerate(member_reg_ids):
        row, col = divmod(i, cols)
        left = margin + col * (box_w + gap)
        top = margin + row * (box_h + gap)
        els.append(f'          <element geometry={attr(f"Left={left};Top={top};Right={left + box_w};Bottom={top + box_h};")} '
                    f'subject={attr(eaid(reg_id))} seqno={attr(i + 1)} '
                    f'style={attr(f"HideIcon=0;DUID={diagram_duid(pkg_key, reg_id)};")}/>\n')
    # The frame is an element entry and belongs with them, before any connector:
    # EA's own framed diagrams give it the last element seqno, then list
    # connectors after every element.
    n_rows = -(-len(member_reg_ids) // cols) if member_reg_ids else 1  # ceil div
    frame_right = margin + min(len(member_reg_ids), cols) * (box_w + gap) + margin
    frame_bottom = margin + n_rows * (box_h + gap) + margin
    frame_duid = ea_guid(f"{pkg_key}:frame")[:8]
    els.append(f'          <element geometry={attr(f"Left=10;Top=10;Right={frame_right};Bottom={frame_bottom};")} '
                f'subject={attr(eaid(pkg_key))} seqno={attr(len(member_reg_ids) + 1)} '
                f'style={attr(f"DUID={frame_duid};")}/>\n')
    for rel_key, from_id, to_id in placed_rels:
        style = (f"Mode=3;EOID={diagram_duid(pkg_key, to_id)};"
                 f"SOID={diagram_duid(pkg_key, from_id)};Color=-1;LWidth=0;Hidden=0;")
        els.append(
            '          <element geometry="SX=0;SY=0;EX=0;EY=0;EDGE=4;$LLB=;LLT=;LMT=;LMB=;LRT=;LRB=;IRHS=;ILHS=;Path=;" '
            f'subject={attr(eaid(rel_key))} style={attr(style)}/>\n')

    return (
        f'      <diagram xmi:id={attr(eaid(f"diagram:{pkg_key}"))}>\n'
        f'        <model package={attr(pkg_id)} localID={attr(seq)} owner={attr(pkg_id)}/>\n'
        f'        <properties name={attr(title)} type={attr(diagram_kind(viewpoint)[0])}/>\n'
        f'        <project author="gungnir" version="1.0" created={attr(STAMP)} modified={attr(STAMP)}/>\n'
        f'        <style1 value={attr(STYLE1)}/>\n'
        f'        <style2 value={attr(style2_xml(domain, viewpoint, ea_guid("savetag:" + pkg_key)[:8]))}/>\n'
        f'        <swimlanes value={attr(SWIMLANES)}/>\n'
        f'        <matrixitems value={attr(MATRIXITEMS)}/>\n'
        f'        <extendedProperties/>\n'
        f'        <persistentstyle value={attr(PERSISTENTSTYLE)}/>\n'
        f'        <xrefs/>\n'
        f'        <elements>\n{"".join(els)}        </elements>\n'
        f'      </diagram>\n'
    )


def view_edge_xml(edge_key: str, from_id: str, to_id: str, label: str,
                   drawn_by: list) -> tuple[str, str]:
    """A connector an authored view draws that the registry does not carry, as
    (packagedElement XML, EA connector-extension XML).

    The views draw plenty of edges that are not registry relationships: a post
    reporting to another post in Pr-Sr, one capability enabling another in St-Cn,
    the step order of a mission thread. Dropping them would put a diagram in EA
    that disagrees with the PlantUML it came from; promoting them into
    relationships.yaml would make the views a second source of typed
    relationships. So they are carried here, as a plain uml:Dependency with NO UAF
    stereotype, tagged with the view that drew them -- always distinguishable from
    a registry relationship, and never mistakable for one.
    """
    name = label or f"{from_id} -> {to_id}"
    rid, from_eid, to_eid = eaid(edge_key), eaid(from_id), eaid(to_id)
    rel = (
        f'        <packagedElement xmi:type="uml:Dependency" xmi:id={attr(rid)} '
        f'name={attr(name)} visibility="public" supplier={attr(to_eid)} client={attr(from_eid)}/>\n'
    )
    tags = [tag_xml(rid, edge_key, "uafViewEdge", ", ".join(v.stem for v in drawn_by)),
            tag_xml(rid, edge_key, "uafViewSource", ", ".join(v.source for v in drawn_by))]
    connector_ext = (
        f'      <connector xmi:idref={attr(rid)} name={attr(name)}>\n'
        f'        <source xmi:idref={attr(from_eid)}>\n'
        f'          <role visibility="Public" targetScope="instance"/>\n'
        f'          <type aggregation="none" containment="Unspecified"/>\n'
        f'          <modifiers isOrdered="false" changeable="none" isNavigable="false"/>\n'
        f'        </source>\n'
        f'        <target xmi:idref={attr(to_eid)}>\n'
        f'          <role visibility="Public" targetScope="instance"/>\n'
        f'          <type aggregation="none" containment="Unspecified"/>\n'
        f'          <modifiers isOrdered="false" changeable="none" isNavigable="true"/>\n'
        f'        </target>\n'
        f'        <properties ea_type="Dependency" direction="Source -&gt; Destination"/>\n'
        f'        <modifiers isRoot="false" isLeaf="false"/>\n'
        '        <appearance linemode="3" linecolor="-1" linewidth="0" seqno="0" headStyle="0" lineStyle="0"/>\n'
        f'        <labels mt={attr(name)}/>\n'
        f'        <tags>\n{"".join(tags)}        </tags>\n'
        '        <xrefs/>\n'
        f'      </connector>\n'
    )
    return rel, connector_ext


def view_diagram_xml(view: "view_layout.ViewDiagram", pkg_key: str, domain: str | None,
                      viewpoint: str | None, edge_keys: list[tuple[str, str, str]],
                      seq: int) -> str:
    """One EA diagram for one authored PlantUML view.

    Same shape as a registry view package's own diagram, with two differences: the
    element geometry is the authored one that view_layout recovered from the
    PlantUML render rather than a grid, and the `documentation` attribute records
    which .puml it came from and whether the layout is authored or generated --
    because for the sequence, activity and taxonomy kinds it is this export's, and
    a reader of the EA model is entitled to know which.
    """
    duid = {}
    els = []
    containers = [e for e in view.elements if e.container]
    plain = [e for e in view.elements if not e.container]
    # Containers first and so underneath: a PlantUML package encloses its members,
    # and EA draws in list order, so a container listed after its contents would
    # cover them.
    for i, e in enumerate(containers + plain):
        left, top, right, bottom = e.box
        d = ea_guid(f"{view.stem}:{e.reg_id}")[:8]
        duid[e.reg_id] = d
        els.append(f'          <element geometry={attr(f"Left={left};Top={top};Right={right};Bottom={bottom};")} '
                    f'subject={attr(eaid(e.reg_id))} seqno={attr(i + 1)} '
                    f'style={attr(f"HideIcon=0;DUID={d};")}/>\n')
    for edge_key, from_id, to_id in edge_keys:
        if from_id not in duid or to_id not in duid:
            continue
        style = (f"Mode=3;EOID={duid[to_id]};SOID={duid[from_id]};Color=-1;LWidth=0;Hidden=0;")
        els.append(
            '          <element geometry="SX=0;SY=0;EX=0;EY=0;EDGE=4;$LLB=;LLT=;LMT=;LMB=;LRT=;LRB=;IRHS=;ILHS=;Path=;" '
            f'subject={attr(eaid(edge_key))} style={attr(style)}/>\n')

    note = (f"Generated from {view.source} by export_xmi.py. "
            + ("Element positions are the authored layout, read back from that view's "
               "own PlantUML render."
               if view.layout == "authored" else
               f"The PlantUML render of a {view.puml_kind.lower()} diagram carries no "
               "element positions, so the layout here is generated; the authored order "
               "is preserved and the rendered SVG under rendered/ is the picture."))
    dgm_key = f"viewdiagram:{view.stem}"
    return (
        f'      <diagram xmi:id={attr(eaid(dgm_key))}>\n'
        f'        <model package={attr(eapk(pkg_key))} localID={attr(seq)} owner={attr(eapk(pkg_key))}/>\n'
        f'        <properties documentation={attr(note)} name={attr(view.title)} '
        f'type={attr(diagram_kind(viewpoint)[0])}/>\n'
        f'        <project author="gungnir" version="1.0" created={attr(STAMP)} modified={attr(STAMP)}/>\n'
        f'        <style1 value={attr(STYLE1)}/>\n'
        f'        <style2 value={attr(style2_xml(domain, viewpoint, ea_guid("savetag:" + dgm_key)[:8]))}/>\n'
        f'        <swimlanes value={attr(SWIMLANES)}/>\n'
        f'        <matrixitems value={attr(MATRIXITEMS)}/>\n'
        f'        <extendedProperties/>\n'
        f'        <persistentstyle value={attr(PERSISTENTSTYLE)}/>\n'
        f'        <xrefs/>\n'
        f'        <elements>\n{"".join(els)}        </elements>\n'
        f'      </diagram>\n'
    )


def build(elements: dict, rels: dict) -> str:
    pkg_members: dict[str, list[str]] = {code: [] for code in VIEW_PACKAGES}
    pkg_element_xml: dict[str, list[str]] = {code: [] for code in VIEW_PACKAGES}
    uaf_stereotypes, connectors = [], []
    known_ids: set[str] = set()
    # Building a real element's own EA extension entry is deferred until every
    # relationship is processed, since it needs a <links> entry per
    # relationship touching it (confirmed in the real sample, present on both
    # endpoints) -- not known until this whole pass is done.
    element_records: dict[str, tuple[str, str, str, dict, str]] = {}
    links_by_id: dict[str, list[tuple[str, str, str, str]]] = {}
    # (rel_key, from_id, to_id) per view package, for that package's diagram.
    pkg_rels: dict[str, list[tuple[str, str, str]]] = {code: [] for code in VIEW_PACKAGES}
    # View packages that hold at least one authored-view diagram. Such a package
    # has to be emitted even when no registry section maps to it: Sc-Cn and If-Cn
    # exist only to hold their authored view.
    pkg_has_view: set[str] = set()

    for section, entries in elements.items():
        if not isinstance(entries, list) or section not in ELEMENT_KIND_INFO:
            continue
        stereotype, metaclass, view_code = ELEMENT_KIND_INFO[section]
        for entry in entries:
            eid = entry["id"]
            known_ids.add(eid)
            cls_xml, uaf_xml = element_xml(entry, stereotype, metaclass)
            pkg_members[view_code].append(eid)
            pkg_element_xml[view_code].append(cls_xml)
            uaf_stereotypes.append(uaf_xml)
            element_records[eid] = (stereotype, metaclass, entry.get("description", ""), entry, view_code)
            links_by_id[eid] = []

    seen_rel_keys: set[str] = set()
    for kind, entries in rels.items():
        if kind not in RELATIONSHIP_KIND_INFO:
            continue
        stereotype, metaclass, view_code = RELATIONSHIP_KIND_INFO[kind]
        for entry in entries or []:
            from_id = entry.get("from")
            to = entry.get("to")
            to_list = to if isinstance(to, list) else [to]
            for to_id in to_list:
                if from_id not in known_ids or to_id not in known_ids:
                    continue  # id resolution is build_uaf.py's job (check()); skip quietly here
                # A relationship's id is derived from what it MEANS, not from
                # its position in the file. Rounds 1-7 used a running counter
                # (`REL-1`, `REL-2`, ...), so inserting one edge near the top of
                # relationships.yaml changed the GUID of every later edge --
                # measured at 570 of 685 -- and a re-import then created that
                # many duplicate connectors instead of updating the existing
                # ones, which is exactly what ea_guid()'s own docstring promises
                # it would not do.
                rel_key = f"REL:{kind}:{from_id}->{to_id}"
                if rel_key in seen_rel_keys:
                    continue  # the same edge stated twice; one connector is right
                seen_rel_keys.add(rel_key)
                # The connector's own direction. The key and the tag keep the
                # registry's `from -> to`; only the two kinds SysML states the
                # other way round get their client and supplier swapped.
                src, dst = (to_id, from_id) if kind in SATISFIER_IS_TARGET else (from_id, to_id)
                rel_xml, conn_ext, uaf_xml = relationship_xml(
                    rel_key, src, dst, kind, stereotype, metaclass, entry)
                pkg_element_xml[view_code].append(rel_xml)
                connectors.append(conn_ext)
                if uaf_xml:
                    uaf_stereotypes.append(uaf_xml)
                link = (metaclass, eaid(rel_key), eaid(src), eaid(dst))
                links_by_id[from_id].append(link)
                links_by_id[to_id].append(link)
                # Both endpoints go on this package's diagram, and the
                # relationship itself gets a connector entry there -- see
                # diagram_xml. A Traceability package owns no elements of its
                # own, so its diagram is the deduplicated union of the endpoints
                # of every relationship it holds.
                if from_id not in pkg_members[view_code]:
                    pkg_members[view_code].append(from_id)
                if to_id not in pkg_members[view_code]:
                    pkg_members[view_code].append(to_id)
                pkg_rels[view_code].append((rel_key, src, dst))

    # ---- the capability hierarchy --------------------------------------------
    # Held in the registry as each capability's `parent` field. Emitted as
    # relationships so the hierarchy is first-class in EA and so the St-Tx view
    # draws it by reference: before this, that view's 56 parent edges became 56
    # view-local connectors duplicating something the registry already stated.
    for entry in elements.get("capabilities", []) or []:
        parent, child = entry.get("parent"), entry.get("id")
        if not parent or parent not in known_ids or child not in known_ids:
            continue
        rel_key = f"REL:{PARENT_KIND}:{parent}->{child}"
        if rel_key in seen_rel_keys:
            continue
        seen_rel_keys.add(rel_key)
        rel_xml, conn_ext, _ = relationship_xml(rel_key, parent, child, PARENT_KIND,
                                                None, "Dependency", {})
        pkg_element_xml["St-Tx"].append(rel_xml)
        connectors.append(conn_ext)
        link = ("Dependency", eaid(rel_key), eaid(parent), eaid(child))
        links_by_id[parent].append(link)
        links_by_id[child].append(link)
        pkg_rels["St-Tx"].append((rel_key, parent, child))

    # ---- the authored views -------------------------------------------------
    # Every .puml under docs/architecture/uaf becomes one EA diagram in its view
    # package, with the element positions its own PlantUML render carries. An edge
    # a view draws that the registry already has reuses that relationship's
    # connector, so one connector appears on as many diagrams as draw it; one it
    # does not becomes a view-local connector (see view_edge_xml).
    rel_by_pair: dict[tuple[str, str], str] = {}
    for rel_key in seen_rel_keys:
        _, _kind, ends = rel_key.split(":", 2)
        a, b = ends.split("->", 1)
        rel_by_pair.setdefault((a, b), rel_key)
        rel_by_pair.setdefault((b, a), rel_key)

    views, view_warnings = view_layout.parse_all(elements)
    view_specs: list[tuple[object, list[tuple[str, str, str]]]] = []
    # A view-local connector is one per (from, to, label), not one per view that
    # draws it. The same needline drawn in Op-Cn and in three vignettes is one
    # connector, shown on all four diagrams and tagged with all four -- which is
    # also how EA itself behaves: on import it places every connector whose two
    # ends are on a diagram, so a connector duplicated per view came back on
    # every diagram that held its endpoints (Op-Cn showed 67 lines for 25 edges
    # on round trip 2). The key carries the label so two different flows between
    # the same pair stay two connectors.
    view_edges: dict[str, tuple[str, str, str, str, list]] = {}
    for view in views:
        code = view.code
        if code not in VIEW_PACKAGES:
            view_warnings.append(f"{view.source}: view code {code} has no package in "
                                 f"VIEW_PACKAGES, so the view is not exported")
            continue
        edge_keys: list[tuple[str, str, str]] = []
        for e in view.edges:
            key = rel_by_pair.get((e.from_id, e.to_id))
            if key is None:
                key = f"VIEW:{e.from_id}->{e.to_id}:{ea_guid(e.label)[:8]}"
                if key not in view_edges:
                    view_edges[key] = (e.from_id, e.to_id, e.label, code, [])
                view_edges[key][4].append(view)
            edge_keys.append((key, e.from_id, e.to_id))
        view_specs.append((view, edge_keys))
        pkg_has_view.add(code)
    for key, (from_id, to_id, label, code, drawn_by) in view_edges.items():
        seen_rel_keys.add(key)
        rel_xml, conn_ext = view_edge_xml(key, from_id, to_id, label, drawn_by)
        pkg_element_xml[code].append(rel_xml)
        connectors.append(conn_ext)
        link = ("Dependency", eaid(key), eaid(from_id), eaid(to_id))
        for end in (from_id, to_id):
            if end in links_by_id:
                links_by_id[end].append(link)

    ea_elements = [
        element_ext_xml(eid, stereotype, metaclass, desc, entry, view_code, links_by_id[eid])
        for eid, (stereotype, metaclass, desc, entry, view_code) in element_records.items()
    ]

    packages, diagrams = [], []
    localid = 0
    for i, (code, (title, domain, viewpoint)) in enumerate(VIEW_PACKAGES.items()):
        members = pkg_element_xml[code]
        if not members and code not in pkg_has_view:
            continue
        full_title = f"{title} {code}" if code not in ("Rq", "Rq-Tr") else title
        packages.append(
            f'      <packagedElement xmi:type="uml:Package" xmi:id={attr(eapk(code))} '
            f'name={attr(full_title)} visibility="public">\n'
            + "".join(members)
            + '      </packagedElement>\n'
        )
        ea_elements.append(package_ext_xml(code, full_title, ROOT_PKG_KEY))
        if pkg_members[code]:
            localid += 1
            diagrams.append(diagram_xml(code, full_title, domain, viewpoint,
                                        pkg_members[code], pkg_rels[code], localid))
        for view, edge_keys in view_specs:
            if view.code != code:
                continue
            localid += 1
            diagrams.append(view_diagram_xml(view, code, domain, viewpoint,
                                             edge_keys, localid))

    # Root package inside uml:Model, matching the sample's nesting exactly:
    # <uml:Model> (no xmi:id of its own) > one EAPK_ root package ("Model" in
    # the sample) > the view packages. Rounds 1-6 put the view packages
    # directly under uml:Model, one level shallower and with no root package at
    # all -- and "Import Package from XMI" imports *a package*.
    root_pkg = (
        f'    <packagedElement xmi:type="uml:Package" xmi:id={attr(eapk(ROOT_PKG_KEY))} '
        f'name="Gungnir UAF Model" visibility="public">\n'
        + "".join(packages)
        + '    </packagedElement>\n'
    )
    ea_elements.append(package_ext_xml(ROOT_PKG_KEY, "Gungnir UAF Model", ROOT_PKG_KEY))

    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<xmi:XMI xmi:version="2.1" '
        'xmlns:xmi="http://schema.omg.org/spec/XMI/2.1" '
        'xmlns:uml="http://schema.omg.org/spec/UML/2.1" '
        'xmlns:EAUML="http://www.sparxsystems.com/profiles/EAUML/1.0" '
        'xmlns:UAF="http://www.omg.org/spec/UAF/20160505/UAF" '
        'xmlns:SysML="http://www.omg.org/spec/SysML/20161101/SysML">\n'
        '  <xmi:Documentation exporter="Enterprise Architect" exporterVersion="6.5" exporterID="1628"/>\n'
        '  <uml:Model xmi:type="uml:Model" name="EA_Model" visibility="public">\n'
        + root_pkg
        + '  </uml:Model>\n'
        + '  <xmi:Extension extender="Enterprise Architect" extenderID="6.5">\n'
        '    <elements>\n'
        + "".join(ea_elements)
        + '    </elements>\n'
        '    <connectors>\n'
        + "".join(connectors)
        + '    </connectors>\n'
        '    <primitivetypes>\n'
        '      <packagedElement xmi:type="uml:Package" xmi:id="EAPrimitiveTypesPackage" '
        'name="EA_PrimitiveTypes_Package"/>\n'
        '    </primitivetypes>\n'
        '    <profiles/>\n'
        '    <diagrams>\n'
        + "".join(diagrams)
        + '    </diagrams>\n'
        '  </xmi:Extension>\n'
        # Stereotype applications are the LAST children of <xmi:XMI>, after the
        # extension closes -- where EA puts them and where it wrote the probe's
        # back. Rounds 3-7 put them between </uml:Model> and <xmi:Extension>,
        # i.e. 490 elements in an unknown namespace immediately before the block
        # EA was failing to read.
        + "".join(uaf_stereotypes)
        + '</xmi:XMI>\n'
    )


def main() -> int:
    elements, rels = load_registry()
    xmi = build(elements, rels)
    out_dir = UAF / "exports"
    out_dir.mkdir(exist_ok=True)
    out_path = out_dir / "gungnir-uaf.xmi"
    # newline="\n" is not optional: without it Python translates every \n to the
    # platform terminator, so a Windows run wrote CRLF against a repository the
    # .gitattributes normalizes to LF, and every regeneration showed up as a
    # 25,462-line diff that was nothing but line endings. build_uaf.py has
    # always passed it; these two exporters did not.
    out_path.write_text(xmi, encoding="utf-8", newline="\n")
    n_elements = sum(len(v) for k, v in elements.items() if isinstance(v, list) and k in ELEMENT_KIND_INFO)
    n_rels = xmi.count('<connector xmi:idref=')
    n_diagrams = xmi.count('<diagram xmi:id=')
    print(f"wrote {out_path}: {n_elements} elements, {n_rels} relationships, {n_diagrams} diagrams")
    return 0


if __name__ == "__main__":
    sys.exit(main())
