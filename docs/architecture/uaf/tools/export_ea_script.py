#!/usr/bin/env python3
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Export the UAF registry as a VBScript that builds the model directly in Sparx
Enterprise Architect through EA's own Scripting/Automation interface, instead of
through XMI import.

Why this exists: `export_xmi.py` got classes into EA on the first try, but two
rounds of XMI relationship encodings (a plain `uml:Dependency` packagedElement,
then an `xmi:Extension` connector block guessed from EA's own extension-XMI
conventions) both produced zero relationships and zero connectors on import.
Rather than guess a third undocumented XMI internal, this script uses EA's
actual documented Automation API (`Repository`, `Package.Elements.AddNew`,
`Element.Connectors.AddNew`, `TaggedValues.AddNew`) -- the same interface EA's
own scripting examples and third-party libraries use, and the one thing in this
whole bridge that doesn't depend on correctly reverse-engineering an
undocumented file format.

What it does and does not do (first version -- see docs/architecture/uaf/README.md):

  - Creates one top-level package "Gungnir UAF Model", one sub-package per
    registry section, one Class per element (stereotype, Notes = description,
    a TaggedValue per remaining registry field, plus `uafId`/`uafKind`), and one
    Dependency connector per relationship (stereotype, a TaggedValue per
    remaining relationship field, plus `uafRelationship`).
  - Does NOT create diagrams. Deferred until the element/connector creation
    above is confirmed working against a real EA project -- no sense layering
    diagram-layout code on a mechanism not yet verified end to end.
  - Is NOT idempotent: re-running creates a second copy of everything, because
    matching "does this element already exist" would need a dependable EA-side
    key and this version doesn't build that lookup. Delete the "Gungnir UAF
    Model" package in the Project Browser before re-running after a registry
    change.
  - Stereotype names are the same best-effort OMG UAF 1.2 mapping `export_xmi.py`
    uses (`ResourceArtifact`, `Capability`, `OperationalPerformer`, ...) -- not
    guaranteed to bind to UAF MDG iconography; report back what EA shows.
  - Non-ASCII text (the registry has exactly one distinct character across every
    field: the section sign U+00A7) is spliced into string literals as `ChrW(N)`
    rather than written as raw bytes, and the file itself is written as strict
    ASCII -- this script cannot control whether the text reaches EA by paste or
    by file-load, so it does not depend on either path preserving a particular
    encoding.

What has and has not been verified: the script has been executed to completion
(all 350 elements, all 686 relationships, no VBScript syntax or runtime error)
against a hand-written mock of the `Repository`/`Element`/`Connector`/
`TaggedValues` object model, via `cscript.exe` outside of EA -- this confirms
the VBScript itself is well-formed and its control flow is correct. It does NOT
confirm that EA's real object model behaves the way the mock assumes (that
`Connectors.AddNew` on the source element plus setting `SupplierID` really does
persist a visible connector, that stereotype names bind to UAF iconography,
that tagged values show up where expected) -- that half needs your actual EA.

Usage (from the workspace root):

    python docs/architecture/uaf/tools/export_ea_script.py

Writes `docs/architecture/uaf/exports/gungnir-uaf-import.vbs`. To run it: open
your EA project, `Tools > Scripting` (or the `Scripting` ribbon group), create a
new script (group "Local Scripts" or similar, type VBScript), paste the file's
contents in, then run it (Ctrl+F9, or the Run button). Watch the Script Output
tab for progress -- it logs every 50 elements and every 100 connectors created.
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from build_uaf import UAF, load_registry  # noqa: E402
from export_xmi import ELEMENT_STEREOTYPE, RELATIONSHIP_STEREOTYPE, SECTION_TITLE, STRUCTURAL_FIELDS  # noqa: E402


def vbs_str(s: object) -> str:
    """A VBScript string expression: double-quote, embedded quotes doubled,
    newlines flattened (registry text is single-line already; this is a guard,
    not a workaround for anything observed). Non-ASCII characters (the registry
    has exactly one: the section sign U+00A7 in a handful of descriptions) are
    spliced in as `& ChrW(N) &` rather than written as raw bytes, since this file
    reaches EA by some copy/paste or file-load path this script doesn't control,
    and betting on that path preserving a particular text encoding is exactly
    the kind of unverified assumption that made the two XMI rounds fail."""
    s = str(s).replace("\r\n", " ").replace("\n", " ")
    parts, buf = [], []
    for ch in s:
        if ord(ch) > 127:
            if buf:
                parts.append('"' + "".join(buf).replace('"', '""') + '"')
                buf = []
            parts.append(f"ChrW({ord(ch)})")
        else:
            buf.append(ch)
    if buf or not parts:
        parts.append('"' + "".join(buf).replace('"', '""') + '"')
    return " & ".join(parts)


def build(elements: dict, rels: dict) -> str:
    lines: list[str] = [
        "' Generated by tools/export_ea_script.py -- builds the Gungnir UAF model",
        "' directly via EA's Automation interface. Re-running duplicates everything;",
        "' delete the \"Gungnir UAF Model\" package first if you want a clean rebuild.",
        "",
        "Dim repo, rootPkg, sectionPkg, el, conn, tv, elementIds, n",
        "Set repo = Repository",
        "Set elementIds = CreateObject(\"Scripting.Dictionary\")",
        "n = 0",
        "",
        "Function GetOrCreatePackage(parentPackages, pkgName)",
        "  Dim i, p, newP",
        "  For i = 0 To parentPackages.Count - 1",
        "    Set p = parentPackages.GetAt(i)",
        "    If p.Name = pkgName Then",
        "      Set GetOrCreatePackage = p",
        "      Exit Function",
        "    End If",
        "  Next",
        "  Set newP = parentPackages.AddNew(pkgName, \"Package\")",
        "  newP.Update",
        "  parentPackages.Refresh",
        "  Set GetOrCreatePackage = newP",
        "End Function",
        "",
        "Function AddTag(owner, tagName, tagValue)",
        "  Dim t",
        "  Set t = owner.TaggedValues.AddNew(tagName, tagValue)",
        "  t.Update",
        "End Function",
        "",
        "repo.EnableCache = True",
        "repo.EnableUIUpdates = False",
        "",
        "Set rootPkg = GetOrCreatePackage(repo.Models, \"Gungnir UAF Model\")",
        "",
    ]

    for section, entries in elements.items():
        if not isinstance(entries, list) or section not in ELEMENT_STEREOTYPE:
            continue
        stereotype = ELEMENT_STEREOTYPE[section]
        title = SECTION_TITLE[section]
        lines.append(f'Set sectionPkg = GetOrCreatePackage(rootPkg.Packages, {vbs_str(title)})')
        for entry in entries:
            eid = entry["id"]
            name = entry.get("name", eid)
            desc = entry.get("description", "")
            lines.append(f'Set el = sectionPkg.Elements.AddNew({vbs_str(name)}, "Class")')
            lines.append(f'el.Stereotype = {vbs_str(stereotype)}')
            if desc:
                lines.append(f'el.Notes = {vbs_str(desc)}')
            lines.append('el.Update')
            lines.append(f'AddTag el, "uafId", {vbs_str(eid)}')
            lines.append(f'AddTag el, "uafKind", {vbs_str(stereotype)}')
            for k, v in entry.items():
                if k in STRUCTURAL_FIELDS or v in (None, "", []):
                    continue
                if isinstance(v, list):
                    v = ", ".join(str(x) for x in v)
                lines.append(f'AddTag el, {vbs_str(k)}, {vbs_str(v)}')
            lines.append(f'elementIds.Add {vbs_str(eid)}, el.ElementID')
            lines.append('n = n + 1')
            lines.append('If n Mod 50 = 0 Then Session.Output "  " & n & " elements created"')
        lines.append('sectionPkg.Elements.Refresh')
        lines.append("")

    lines.append('Session.Output n & " elements created total. Creating relationships..."')
    lines.append("n = 0")
    lines.append("")

    for kind, entries in rels.items():
        if kind not in RELATIONSHIP_STEREOTYPE:
            continue
        stereotype = RELATIONSHIP_STEREOTYPE[kind]
        for entry in entries:
            from_id = entry.get("from")
            to = entry.get("to")
            to_list = to if isinstance(to, list) else [to]
            for to_id in to_list:
                name = f"{kind}: {from_id} -> {to_id}"
                lines.append(f'If elementIds.Exists({vbs_str(from_id)}) And elementIds.Exists({vbs_str(to_id)}) Then')
                lines.append(f'  Set el = repo.GetElementByID(elementIds.Item({vbs_str(from_id)}))')
                lines.append(f'  Set conn = el.Connectors.AddNew({vbs_str(name)}, "Dependency")')
                lines.append(f'  conn.SupplierID = elementIds.Item({vbs_str(to_id)})')
                lines.append(f'  conn.Stereotype = {vbs_str(stereotype)}')
                lines.append('  conn.Update')
                lines.append(f'  AddTag conn, "uafRelationship", {vbs_str(kind)}')
                for k, v in entry.items():
                    if k in ("from", "to") or v in (None, "", []):
                        continue
                    if isinstance(v, list):
                        v = ", ".join(str(x) for x in v)
                    lines.append(f'  AddTag conn, {vbs_str(k)}, {vbs_str(v)}')
                lines.append('  el.Connectors.Refresh')
                lines.append('  n = n + 1')
                lines.append('  If n Mod 100 = 0 Then Session.Output "  " & n & " relationships created"')
                lines.append('End If')

    lines.append("")
    lines.append('repo.EnableUIUpdates = True')
    lines.append('Session.Output n & " relationships created total. Done."')
    done_msg = " relationships done. Reopen the Gungnir UAF Model package (or press F5) if the Project Browser doesn't show it yet."
    lines.append(f'MsgBox n & {vbs_str(done_msg)}')
    return "\n".join(lines) + "\n"


def main() -> int:
    elements, rels = load_registry()
    vbs = build(elements, rels)
    out_dir = UAF / "exports"
    out_dir.mkdir(exist_ok=True)
    out_path = out_dir / "gungnir-uaf-import.vbs"
    # Strict ASCII, not a guess about how this text reaches EA (paste, file-load,
    # whatever): vbs_str() already spliced every non-ASCII character (the
    # registry has exactly one, the section sign) in as ChrW(N), specifically so
    # this file needs no particular encoding/BOM to render correctly. Encoding
    # as ascii here turns "nothing non-ASCII slipped through" into something
    # this script fails loudly on, rather than something to hope is true.
    out_path.write_text(vbs, encoding="ascii")
    n_elements = sum(len(v) for k, v in elements.items() if isinstance(v, list) and k in ELEMENT_STEREOTYPE)
    n_rels = vbs.count('.Connectors.AddNew(')
    print(f"wrote {out_path}: script builds {n_elements} elements, {n_rels} relationships")
    return 0


if __name__ == "__main__":
    sys.exit(main())
