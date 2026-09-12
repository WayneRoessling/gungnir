#!/usr/bin/env python3
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Write a small EA-dialect XMI probe, for testing one question against a real
Enterprise Architect install instead of reasoning about its file format.

Why this exists: `export_xmi.py` took seven rounds to produce diagrams, and rounds
1, 2, 4, 5, 6 and 7 each shipped a 1.8MB file that imported without error and
showed nothing. A 27KB probe answers the same questions in one import, and the
answer is unambiguous because there is only one of everything to look at.

The probe carries 5 elements, 3 relationships, 4 view packages and 4 diagrams,
written exactly as `export_xmi.py` writes them, so whatever it does in EA is what
the full export will do. See `../exports/ea-roundtrip/README.md` for the round
trip this was built for and what it settled.

Its main use now is the stereotype oracle. Seven of the nine relationship
stereotypes and five of the eleven element stereotypes in
`RELATIONSHIP_KIND_INFO`/`ELEMENT_KIND_INFO` are still guesses drawn from the OMG
profile literature. EA resolves a stereotype it recognizes into the `UAF`
namespace and files one it does not under `thecustomprofile`, so:

    python docs/architecture/uaf/tools/make_ea_probe.py probe.xmi
    # import probe.xmi into EA, export the imported package as XMI 2.1
    # grep the export for `thecustomprofile:` -- every hit is a wrong name

That is how `Performs` was caught and replaced with `IsCapableToPerform`. To test
a different name, edit ELS/RELS below.

Usage (from the workspace root):

    python docs/architecture/uaf/tools/make_ea_probe.py <output.xmi>
"""

import hashlib
import pathlib
import sys
from xml.sax.saxutils import quoteattr as _qa


def q(v):
    return _qa(str(v))


def guid(k):
    h = hashlib.md5(k.encode()).hexdigest().upper()
    return f"{h[0:8]}_{h[8:12]}_4{h[13:16]}_{h[16:20]}_{h[20:32]}"


def EID(k):
    return "EAID_" + guid(k)


def PID(k):
    return "EAPK_" + guid(k)


def DUID(k):
    return guid(k)[:8]


STYLE1 = (
    "ShowPrivate=1;ShowProtected=1;ShowPublic=1;HideRelationships=0;Locked=0;Border=1;HighlightForeign=1;"
    "PackageContents=1;SequenceNotes=0;ScalePrintImage=0;PPgs.cx=0;PPgs.cy=0;DocSize.cx=850;DocSize.cy=1098;"
    "ShowDetails=0;Orientation=P;Zoom=100;ShowTags=0;OpParams=1;VisibleAttributeDetail=0;ShowOpRetType=1;"
    "ShowIcons=1;CollabNums=0;HideProps=0;ShowReqs=0;ShowCons=0;PaperSize=1;HideParents=0;UseAlias=0;HideAtts=0;"
    "HideOps=0;HideStereo=0;HideElemStereo=0;ShowTests=0;ShowMaint=0;ConnectorNotation=UML 2.1;"
    "ExplicitNavigability=0;ShowShape=1;AllDockable=0;AdvancedElementProps=1;AdvancedFeatureProps=1;"
    "AdvancedConnectorProps=1;m_bElementClassifier=1;SPT=1;ShowNotes=0;SuppressBrackets=0;SuppConnectorLabels=0;"
    "PrintPageHeadFoot=0;ShowAsList=0;"
)
SWIM = (
    "locked=false;orientation=0;width=0;inbar=false;names=false;color=-1;bold=false;fcol=0;tcol=-1;ofCol=-1;"
    "ufCol=-1;hl=1;ufh=0;hh=0;cls=0;bw=0;hli=0;bro=0;SwimlaneFont=lfh:-10,lfw:0,lfi:0,lfu:0,lfs:0,lfface:Calibri,"
    "lfe:0,lfo:0,lfchar:1,lfop:0,lfcp:0,lfq:0,lfpf=0,lfWidth=0;"
)
MATRIX = "locked=false;matrixactive=false;swimlanesactive=true;kanbanactive=false;width=1;clrLine=0;"
PSTYLE = "DGS=On=0:CNT=8:W=120:H=40:SG=0:SGH=0:AEB=0:;AR=0;DCL=0;"


def style2(dom, vp, st):
    mdg = f"MDGView=UAF {dom}::{vp};" if dom else ""
    return (
        "ExcludeRTF=0;DocAll=0;HideQuals=0;AttPkg=1;ShowTests=0;ShowMaint=0;SuppressFOC=1;MatrixActive=0;"
        "SwimlanesActive=1;KanbanActive=0;MatrixLineWidth=1;MatrixLineClr=0;MatrixLocked=0;"
        "TConnectorNotation=UML 2.1;TExplicitNavigability=0;AdvancedElementProps=1;AdvancedFeatureProps=1;"
        "AdvancedConnectorProps=1;m_bElementClassifier=1;SPT=1;MDGDgm=;" + mdg +
        "STBLDgm=;ShowNotes=0;VisibleAttributeDetail=0;ShowOpRetType=1;SuppressBrackets=0;SuppConnectorLabels=0;"
        f"PrintPageHeadFoot=0;ShowAsList=0;SuppressedCompartments=;Theme=:119;SaveTag={st};"
    )


ROOT = "probe-root"
ELS = [
    ("CAP-1", "Sense", "Capability", "Class", "St-Tx"),
    ("CAP-2", "Understand", "Capability", "Class", "St-Tx"),
    ("OP-1", "Sector sensing node", "OperationalPerformer", "Class", "Op-Sr"),
    ("OP-2", "Fusion node", "OperationalPerformer", "Class", "Op-Sr"),
    ("OA-1", "Maintain the track picture", "OperationalActivity", "Activity", "Op-Pr"),
]
RELS = [
    ("R-1", "exhibits", "Exhibits", "OP-1", "CAP-1", "Op-Tr"),
    ("R-2", "exhibits", "Exhibits", "OP-2", "CAP-2", "Op-Tr"),
    ("R-3", "performs", "Performs", "OP-2", "OA-1", "Op-Tr"),
]
PKGS = {
    "St-Tx": ("Strategic Taxonomy St-Tx", "Strategic", "Taxonomy"),
    "Op-Sr": ("Operational Structure Op-Sr", "Operational", "Structure"),
    "Op-Pr": ("Operational Processes Op-Pr", "Operational", "Processes"),
    "Op-Tr": ("Operational Traceability Op-Tr", "Operational", "Traceability"),
}
NAME = {e[0]: e[1] for e in ELS}

members = {k: [e[0] for e in ELS if e[4] == k] for k in PKGS}
for r in RELS:
    for x in (r[3], r[4]):
        if x not in members[r[5]]:
            members[r[5]].append(x)

out = [
    '<?xml version="1.0" encoding="windows-1252"?>\n',
    '<xmi:XMI xmlns:xmi="http://schema.omg.org/spec/XMI/2.1" xmi:version="2.1" '
    'xmlns:uml="http://schema.omg.org/spec/UML/2.1" '
    'xmlns:UAF="http://www.omg.org/spec/UAF/20160505/UAF">\n',
    '\t<xmi:Documentation exporter="Enterprise Architect" exporterVersion="6.5" exporterID="1628"/>\n',
    '\t<uml:Model xmi:type="uml:Model" name="EA_Model" visibility="public">\n',
    f'\t\t<packagedElement xmi:type="uml:Package" xmi:id={q(PID(ROOT))} name="Gungnir UAF Probe" visibility="public">\n',
]
for code, (title, _d, _v) in PKGS.items():
    out.append(f'\t\t\t<packagedElement xmi:type="uml:Package" xmi:id={q(PID(code))} name={q(title)} visibility="public">\n')
    for rid, name, st, mc, pk in ELS:
        if pk != code:
            continue
        extra = ' isReadOnly="false" isSingleExecution="false"' if mc == "Activity" else ""
        out.append(f'\t\t\t\t<packagedElement xmi:type={q("uml:" + mc)} xmi:id={q(EID(rid))} name={q(name)} visibility="public"{extra}>\n')
        out.append(f'\t\t\t\t\t<ownedComment xmi:type="uml:Comment" xmi:id={q(EID(rid + "-c"))} body={q("Probe element " + rid)}/>\n')
        out.append('\t\t\t\t</packagedElement>\n')
    for rid, kind, st, f_, t_, pk in RELS:
        if pk != code:
            continue
        out.append(
            f'\t\t\t\t<packagedElement xmi:type="uml:Abstraction" xmi:id={q(EID(rid))} '
            f'name={q(kind + ": " + f_ + " -> " + t_)} visibility="public" '
            f'supplier={q(EID(t_))} client={q(EID(f_))}/>\n'
        )
    out.append('\t\t\t</packagedElement>\n')
out += ['\t\t</packagedElement>\n', '\t</uml:Model>\n',
        '\t<xmi:Extension extender="Enterprise Architect" extenderID="6.5">\n', '\t\t<elements>\n']

links = {e[0]: [] for e in ELS}
for rid, kind, st, f_, t_, pk in RELS:
    links[f_].append((rid, f_, t_))
    links[t_].append((rid, f_, t_))

for rid, name, st, mc, pk in ELS:
    lx = "".join(
        f'\t\t\t\t\t<Abstraction xmi:id={q(EID(r))} start={q(EID(a))} end={q(EID(b))}/>\n'
        for r, a, b in links[rid]
    )
    out.append(
        f'\t\t\t<element xmi:idref={q(EID(rid))} xmi:type={q("uml:" + mc)} name={q(name)} scope="public">\n'
        f'\t\t\t\t<model package={q(PID(pk))} tpos="0" ea_eleType="element"/>\n'
        f'\t\t\t\t<properties isSpecification="false" sType={q(mc)} nType="0" scope="public" '
        f'stereotype={q(st)} documentation={q("Probe element " + rid)}/>\n'
        '\t\t\t\t<project author="gungnir" version="1.0" phase="1.0" created="2026-09-04 00:00:00" '
        'modified="2026-09-04 00:00:00" complexity="1" status="Proposed"/>\n'
        '\t\t\t\t<style appearance="BackColor=-1;BorderColor=-1;BorderWidth=-1;FontColor=-1;'
        'VSwimLanes=1;HSwimLanes=1;BorderStyle=0;"/>\n'
        f'\t\t\t\t<tags>\n\t\t\t\t\t<tag name="uafId" value={q(rid)}/>\n\t\t\t\t</tags>\n'
        '\t\t\t\t<xrefs/>\n\t\t\t\t<extendedProperties tagged="0"/>\n'
        f'\t\t\t\t<links>\n{lx}\t\t\t\t</links>\n\t\t\t</element>\n'
    )
for code, (title, _d, _v) in PKGS.items():
    out.append(
        f'\t\t\t<element xmi:idref={q(PID(code))} xmi:type="uml:Package" name={q(title)} scope="public">\n'
        f'\t\t\t\t<model package2={q(EID(code))} package={q(PID(ROOT))} tpos="0" ea_eleType="package"/>\n'
        '\t\t\t\t<properties isSpecification="false" sType="Package" nType="0" scope="public"/>\n'
        '\t\t\t\t<packageproperties version="1.0"/>\n\t\t\t\t<paths/>\n'
        '\t\t\t\t<times created="2026-09-04 00:00:00" modified="2026-09-04 00:00:00"/>\n'
        '\t\t\t\t<flags iscontrolled="0" isprotected="0" batchsave="0" batchload="0" usedtd="0" logxml="0"/>\n'
        '\t\t\t</element>\n'
    )
out.append('\t\t</elements>\n\t\t<connectors>\n')
for rid, kind, st, f_, t_, pk in RELS:
    nm = f"{kind}: {f_} -> {t_}"
    out.append(
        f'\t\t\t<connector xmi:idref={q(EID(rid))} name={q(nm)}>\n'
        f'\t\t\t\t<source xmi:idref={q(EID(f_))}>\n'
        f'\t\t\t\t\t<model type="Class" name={q(NAME[f_])}/>\n'
        '\t\t\t\t\t<role visibility="Public" targetScope="instance"/>\n'
        '\t\t\t\t\t<type aggregation="none" containment="Unspecified"/>\n'
        '\t\t\t\t\t<modifiers isOrdered="false" changeable="none" isNavigable="false"/>\n'
        '\t\t\t\t</source>\n'
        f'\t\t\t\t<target xmi:idref={q(EID(t_))}>\n'
        f'\t\t\t\t\t<model type="Class" name={q(NAME[t_])}/>\n'
        '\t\t\t\t\t<role visibility="Public" targetScope="instance"/>\n'
        '\t\t\t\t\t<type aggregation="none" containment="Unspecified"/>\n'
        '\t\t\t\t\t<modifiers isOrdered="false" changeable="none" isNavigable="true"/>\n'
        '\t\t\t\t</target>\n'
        f'\t\t\t\t<properties ea_type="Abstraction" direction="Source -&gt; Destination" stereotype={q(st)}/>\n'
        '\t\t\t\t<modifiers isRoot="false" isLeaf="false"/>\n'
        '\t\t\t\t<appearance linemode="3" linecolor="-1" linewidth="0" seqno="0" headStyle="0" lineStyle="0"/>\n'
        f'\t\t\t\t<labels mt={q(nm)}/>\n'
        f'\t\t\t\t<tags>\n\t\t\t\t\t<tag name="uafRelationship" value={q(kind)}/>\n\t\t\t\t</tags>\n'
        '\t\t\t\t<xrefs/>\n\t\t\t</connector>\n'
    )
out.append(
    '\t\t</connectors>\n\t\t<primitivetypes>\n'
    '\t\t\t<packagedElement xmi:type="uml:Package" xmi:id="EAPrimitiveTypesPackage" name="EA_PrimitiveTypes_Package"/>\n'
    '\t\t</primitivetypes>\n\t\t<profiles/>\n\t\t<diagrams>\n'
)
for i, (code, (title, dom, vp)) in enumerate(PKGS.items()):
    ms = members[code]
    ent = []
    for j, rid in enumerate(ms):
        row, col = divmod(j, 4)
        left = 40 + col * 200
        top = 40 + row * 140
        ent.append(
            f'\t\t\t\t\t<element geometry={q(f"Left={left};Top={top};Right={left + 150};Bottom={top + 80};")} '
            f'subject={q(EID(rid))} seqno={q(j + 1)} style={q("DUID=" + DUID(code + rid) + ";")}/>\n'
        )
    for rid, kind, st, f_, t_, pk in RELS:
        if f_ in ms and t_ in ms:
            ent.append(
                '\t\t\t\t\t<element geometry="SX=0;SY=0;EX=0;EY=0;EDGE=4;$LLB=;LLT=;LMT=;LMB=;LRT=;LRB=;IRHS=;ILHS=;Path=;" '
                f'subject={q(EID(rid))} '
                f'style={q("Mode=3;EOID=" + DUID(code + t_) + ";SOID=" + DUID(code + f_) + ";Color=-1;LWidth=0;Hidden=0;")}/>\n'
            )
    out.append(
        f'\t\t\t<diagram xmi:id={q(EID("dgm:" + code))}>\n'
        f'\t\t\t\t<model package={q(PID(code))} localID={q(i + 1)} owner={q(PID(code))}/>\n'
        f'\t\t\t\t<properties name={q(title)} type="Logical"/>\n'
        '\t\t\t\t<project author="gungnir" version="1.0" created="2026-09-04 00:00:00" modified="2026-09-04 00:00:00"/>\n'
        f'\t\t\t\t<style1 value={q(STYLE1)}/>\n'
        f'\t\t\t\t<style2 value={q(style2(dom, vp, DUID("sv:" + code)))}/>\n'
        f'\t\t\t\t<swimlanes value={q(SWIM)}/>\n'
        f'\t\t\t\t<matrixitems value={q(MATRIX)}/>\n'
        '\t\t\t\t<extendedProperties/>\n'
        f'\t\t\t\t<persistentstyle value={q(PSTYLE)}/>\n'
        '\t\t\t\t<xrefs/>\n'
        f'\t\t\t\t<elements>\n{"".join(ent)}\t\t\t\t</elements>\n\t\t\t</diagram>\n'
    )
out.append('\t\t</diagrams>\n\t</xmi:Extension>\n')
for rid, name, st, mc, pk in ELS:
    out.append(f'\t<UAF:{st} base_{mc}={q(EID(rid))}/>\n')
for rid, kind, st, f_, t_, pk in RELS:
    out.append(f'\t<UAF:{st} base_Abstraction={q(EID(rid))}/>\n')
out.append('</xmi:XMI>\n')

p = pathlib.Path(sys.argv[1])
p.write_text("".join(out), encoding="windows-1252", newline="\r\n")
print("wrote", p, p.stat().st_size, "bytes")
