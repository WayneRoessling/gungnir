#!/usr/bin/env python3
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Export the UAF registry as CSV data plus a short VBScript driver that builds
the model directly in Sparx Enterprise Architect through EA's own
Scripting/Automation interface, instead of through XMI import.

Why this exists: `export_xmi.py` got classes into EA on the first try, but two
rounds of XMI relationship encodings both produced zero relationships and zero
connectors on import. Rather than guess a third undocumented XMI internal, this
uses EA's actual documented Automation API (`Repository`,
`Package.Elements.AddNew`, `Element.Connectors.AddNew`, `TaggedValues.AddNew`)
-- the same interface EA's own scripting examples and third-party libraries
use, and the one thing in this whole bridge that doesn't depend on correctly
reverse-engineering an undocumented file format.

Why CSV + a driver, not one big generated script (this module's second
version): the first version inlined all ~350 elements and ~686 relationships as
literal script statements -- an ~11,500 line, ~470KB file. Pasting that into
EA's script editor choked (reported: paste only advancing one line at a time,
consistent with a Scintilla-based editor re-tokenising/re-highlighting after
every inserted chunk of a paste that size). The driver below is a couple of
hundred fixed lines regardless of registry size -- small enough to paste in one
shot -- and reads the bulk data from four CSV files at runtime instead of
carrying it as script text.

What it does and does not do (see docs/architecture/uaf/README.md):

  - Creates one top-level package "Gungnir UAF Model", one sub-package per
    registry section, one Class per element (stereotype, Notes = description,
    a TaggedValue per remaining registry field, plus `uafId`/`uafKind`), and one
    Dependency connector per relationship (stereotype, a TaggedValue per
    remaining relationship field, plus `uafRelationship`).
  - Does NOT create diagrams -- deferred until the element/connector creation
    above is confirmed working against a real EA project.
  - Is NOT idempotent: re-running creates a second copy of everything. Delete
    the "Gungnir UAF Model" package in the Project Browser before re-running
    after a registry change.
  - Stereotype names are the same best-effort OMG UAF 1.2 mapping `export_xmi.py`
    uses -- not guaranteed to bind to UAF MDG iconography; report back what EA
    shows.
  - CSV fields are RFC4180 (Python's `csv` module writes them; the driver's
    `ParseCSVLine` reads them the same way), and any non-ASCII character (the
    registry has exactly one distinct one: the section sign U+00A7) is written
    into the CSV as a literal `\\uXXXX` escape and decoded by the driver via
    `ChrW` -- both the driver and the CSVs are therefore strict ASCII, so
    nothing about this bridge depends on a particular file encoding or BOM
    surviving however you move these files onto the machine running EA.

What has and has not been verified: the driver has been executed to completion
(all 350 elements, all 686 relationships, no VBScript syntax or runtime error)
against a hand-written mock of the `Repository`/`Element`/`Connector`/
`TaggedValues` object model, via `cscript.exe` outside of EA (`test_ea_script_mock.vbs`)
-- this confirms the VBScript itself is well-formed and its control flow is
correct, including the CSV parsing. It does NOT confirm EA's real object model
behaves the way the mock assumes -- that half needs your actual EA.

Usage (from the workspace root):

    python docs/architecture/uaf/tools/export_ea_script.py

Writes to `docs/architecture/uaf/exports/`: `gungnir-uaf-import.vbs` (the
driver) and `gungnir-uaf-elements.csv`, `gungnir-uaf-element-tags.csv`,
`gungnir-uaf-relationships.csv`, `gungnir-uaf-relationship-tags.csv` (the data).
All five files must sit in the same folder when you run the driver. Open your
EA project, `Tools > Scripting` (or the `Scripting` ribbon group), create a new
script (type VBScript), paste `gungnir-uaf-import.vbs`'s contents in, edit the
`DATA_DIR` constant near the top to the folder holding the four CSVs, then run
it (Ctrl+F9). Watch the Script Output tab for progress.
"""
from __future__ import annotations

import csv
import io
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from build_uaf import UAF, load_registry  # noqa: E402
from export_xmi import ELEMENT_KIND_INFO, RELATIONSHIP_KIND_INFO, STRUCTURAL_FIELDS  # noqa: E402

# export_xmi.py groups elements into named UAF view packages (VIEW_PACKAGES,
# matching a real EA project's own package/diagram structure); this script
# builds EA's live object model directly instead of emitting XMI to import, so
# it has no equivalent need for that grouping -- it keeps its own simpler
# one-package-per-registry-section layout, unaffected by that reorganization.
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


def ascii_escape(s: object) -> str:
    """Every non-ASCII character (the registry has exactly one: the section
    sign) becomes a literal `\\uXXXX` for the driver's DecodeUnicodeEscapes to
    turn back into a real character via ChrW -- see the module docstring for
    why this file needs to be encoding-agnostic rather than just UTF-8."""
    s = str(s).replace("\r\n", " ").replace("\n", " ")
    return "".join(c if ord(c) <= 127 else f"\\u{ord(c):04x}" for c in s)


def flatten(v: object) -> str:
    if isinstance(v, list):
        return ", ".join(str(x) for x in v)
    return str(v)


def write_csv(path: Path, header: list[str], rows: list[list[str]]) -> None:
    buf = io.StringIO()
    w = csv.writer(buf, lineterminator="\r\n")
    w.writerow(header)
    w.writerows(rows)
    path.write_text(buf.getvalue(), encoding="ascii")


def build_csvs(elements: dict, rels: dict, out_dir: Path) -> tuple[int, int]:
    element_rows, tag_rows = [], []
    known_ids: set[str] = set()

    for section, entries in elements.items():
        if not isinstance(entries, list) or section not in ELEMENT_KIND_INFO:
            continue
        stereotype, _metaclass, _view_code = ELEMENT_KIND_INFO[section]
        title = SECTION_TITLE[section]
        for entry in entries:
            eid = entry["id"]
            known_ids.add(eid)
            name = ascii_escape(entry.get("name", eid))
            desc = ascii_escape(entry.get("description", ""))
            element_rows.append([eid, title, stereotype, name, desc])
            tag_rows.append([eid, "uafId", eid])
            tag_rows.append([eid, "uafKind", stereotype])
            for k, v in entry.items():
                if k in STRUCTURAL_FIELDS or v in (None, "", []):
                    continue
                tag_rows.append([eid, k, ascii_escape(flatten(v))])

    rel_rows, rel_tag_rows, n = [], [], 0
    for kind, entries in rels.items():
        if kind not in RELATIONSHIP_KIND_INFO:
            continue
        stereotype, _metaclass, _view_code = RELATIONSHIP_KIND_INFO[kind]
        for entry in entries:
            from_id = entry.get("from")
            to = entry.get("to")
            to_list = to if isinstance(to, list) else [to]
            for to_id in to_list:
                if from_id not in known_ids or to_id not in known_ids:
                    continue  # id resolution is build_uaf.py's job (check()); skip quietly here
                n += 1
                rel_id = f"REL-{n}"
                name = ascii_escape(f"{kind}: {from_id} -> {to_id}")
                rel_rows.append([rel_id, kind, stereotype or "", from_id, to_id, name])
                rel_tag_rows.append([rel_id, "uafRelationship", kind])
                for k, v in entry.items():
                    if k in ("from", "to") or v in (None, "", []):
                        continue
                    rel_tag_rows.append([rel_id, k, ascii_escape(flatten(v))])

    write_csv(out_dir / "gungnir-uaf-elements.csv", ["id", "section", "stereotype", "name", "description"], element_rows)
    write_csv(out_dir / "gungnir-uaf-element-tags.csv", ["id", "key", "value"], tag_rows)
    write_csv(out_dir / "gungnir-uaf-relationships.csv", ["rel_id", "kind", "stereotype", "from_id", "to_id", "name"], rel_rows)
    write_csv(out_dir / "gungnir-uaf-relationship-tags.csv", ["rel_id", "key", "value"], rel_tag_rows)
    return len(element_rows), len(rel_rows)


DRIVER_TEMPLATE = r'''' Generated by tools/export_ea_script.py -- builds the Gungnir UAF model
' directly via EA's Automation interface, reading bulk data from the four CSV
' files this generator wrote alongside this script. Re-running duplicates
' everything; delete the "Gungnir UAF Model" package first for a clean rebuild.

' >>> EDIT THIS to the folder holding the four gungnir-uaf-*.csv files <<<
Const DATA_DIR = "C:\path\to\folder\containing\the\csv\files"

Dim repo, fso, rootPkg, elementIds, connectorIds, packagesByTitle, n

Function ParseCSVLine(line)
  Dim fields(), fieldCount, i, ch, cur, inQuotes
  ReDim fields(255)
  fieldCount = 0
  cur = ""
  inQuotes = False
  i = 1
  Do While i <= Len(line)
    ch = Mid(line, i, 1)
    If inQuotes Then
      If ch = """" Then
        If i < Len(line) And Mid(line, i + 1, 1) = """" Then
          cur = cur & """"
          i = i + 1
        Else
          inQuotes = False
        End If
      Else
        cur = cur & ch
      End If
    Else
      If ch = """" Then
        inQuotes = True
      ElseIf ch = "," Then
        fields(fieldCount) = cur
        fieldCount = fieldCount + 1
        cur = ""
      Else
        cur = cur & ch
      End If
    End If
    i = i + 1
  Loop
  fields(fieldCount) = cur
  fieldCount = fieldCount + 1
  ReDim Preserve fields(fieldCount - 1)
  ParseCSVLine = fields
End Function

Function IsHex4(s)
  Dim i, c
  IsHex4 = (Len(s) = 4)
  If Not IsHex4 Then Exit Function
  For i = 1 To 4
    c = UCase(Mid(s, i, 1))
    If InStr("0123456789ABCDEF", c) = 0 Then
      IsHex4 = False
      Exit Function
    End If
  Next
End Function

' A plain character scan, not RegExp: a regex "\\u([0-9A-Fa-f]{4})" pattern
' written as a VBScript string literal (which does not itself interpret
' backslashes) matched "u00a7" without the leading backslash -- verified by
' checking the match's FirstIndex/Length directly, not by eyeballing console
' output (WScript.Echo of a non-ASCII character renders as nothing under
' cscript's console codepage, which looks like -- but is not -- a dropped
' character; Len() and AscW() on the actual string values are what settled it).
Function DecodeUnicodeEscapes(s)
  Dim result, pos, bs, hex4
  bs = Chr(92)
  result = ""
  pos = 1
  Do While pos <= Len(s)
    If Mid(s, pos, 1) = bs And Mid(s, pos + 1, 1) = "u" And IsHex4(Mid(s, pos + 2, 4)) Then
      hex4 = Mid(s, pos + 2, 4)
      result = result & ChrW(CLng("&H" & hex4))
      pos = pos + 6
    Else
      result = result & Mid(s, pos, 1)
      pos = pos + 1
    End If
  Loop
  DecodeUnicodeEscapes = result
End Function

Function ReadCSVRows(path)
  Dim f, allLines, rows, i, fields
  Set f = fso.OpenTextFile(path, 1, False, 0)
  allLines = Split(f.ReadAll(), vbLf)
  f.Close
  ReDim rows(UBound(allLines))
  Dim rowCount
  rowCount = 0
  For i = 1 To UBound(allLines)  ' skip header row (index 0)
    If Len(Trim(allLines(i))) > 0 Then
      fields = ParseCSVLine(allLines(i))
      Dim j
      For j = 0 To UBound(fields)
        fields(j) = DecodeUnicodeEscapes(fields(j))
      Next
      rows(rowCount) = fields
      rowCount = rowCount + 1
    End If
  Next
  ReDim Preserve rows(rowCount - 1)
  ReadCSVRows = rows
End Function

Function GetOrCreatePackage(parentPackages, pkgName)
  If packagesByTitle.Exists(pkgName) Then
    Set GetOrCreatePackage = packagesByTitle.Item(pkgName)
    Exit Function
  End If
  Dim newP
  Set newP = parentPackages.AddNew(pkgName, "Package")
  newP.Update
  parentPackages.Refresh
  packagesByTitle.Add pkgName, newP
  Set GetOrCreatePackage = newP
End Function

Function AddTag(owner, tagName, tagValue)
  Dim t
  Set t = owner.TaggedValues.AddNew(tagName, tagValue)
  t.Update
End Function

Set repo = Repository
Set fso = CreateObject("Scripting.FileSystemObject")
Set elementIds = CreateObject("Scripting.Dictionary")
Set connectorIds = CreateObject("Scripting.Dictionary")
Set packagesByTitle = CreateObject("Scripting.Dictionary")

repo.EnableCache = True
repo.EnableUIUpdates = False
Set rootPkg = GetOrCreatePackage(repo.Models, "Gungnir UAF Model")

' ---- elements ----
Dim elRows, elRow, sectionPkg, el
elRows = ReadCSVRows(DATA_DIR & "\gungnir-uaf-elements.csv")
n = 0
For Each elRow In elRows
  ' elRow: id, section, stereotype, name, description
  Set sectionPkg = GetOrCreatePackage(rootPkg.Packages, elRow(1))
  Set el = sectionPkg.Elements.AddNew(elRow(3), "Class")
  el.Stereotype = elRow(2)
  If Len(elRow(4)) > 0 Then el.Notes = elRow(4)
  el.Update
  elementIds.Add elRow(0), el.ElementID
  n = n + 1
  If n Mod 50 = 0 Then Session.Output "  " & n & " elements created"
Next
Session.Output n & " elements created total."

' ---- element tags ----
Dim tagRows, tagRow
tagRows = ReadCSVRows(DATA_DIR & "\gungnir-uaf-element-tags.csv")
For Each tagRow In tagRows
  ' tagRow: id, key, value
  If elementIds.Exists(tagRow(0)) Then
    AddTag repo.GetElementByID(elementIds.Item(tagRow(0))), tagRow(1), tagRow(2)
  End If
Next
Session.Output "element tags applied."

' ---- relationships ----
Dim relRows, relRow, fromEl, conn
relRows = ReadCSVRows(DATA_DIR & "\gungnir-uaf-relationships.csv")
n = 0
For Each relRow In relRows
  ' relRow: rel_id, kind, stereotype, from_id, to_id, name
  If elementIds.Exists(relRow(3)) And elementIds.Exists(relRow(4)) Then
    Set fromEl = repo.GetElementByID(elementIds.Item(relRow(3)))
    Set conn = fromEl.Connectors.AddNew(relRow(5), "Dependency")
    conn.SupplierID = elementIds.Item(relRow(4))
    conn.Stereotype = relRow(2)
    conn.Update
    fromEl.Connectors.Refresh
    connectorIds.Add relRow(0), conn.ConnectorID
    n = n + 1
    If n Mod 100 = 0 Then Session.Output "  " & n & " relationships created"
  End If
Next
Session.Output n & " relationships created total."

' ---- relationship tags ----
Dim relTagRows, relTagRow
relTagRows = ReadCSVRows(DATA_DIR & "\gungnir-uaf-relationship-tags.csv")
For Each relTagRow In relTagRows
  ' relTagRow: rel_id, key, value
  If connectorIds.Exists(relTagRow(0)) Then
    AddTag repo.GetConnectorByID(connectorIds.Item(relTagRow(0))), relTagRow(1), relTagRow(2)
  End If
Next
Session.Output "relationship tags applied."

repo.EnableUIUpdates = True
MsgBox "Done. Reopen the Gungnir UAF Model package (or press F5) if the Project Browser doesn't show it yet."
'''


def main() -> int:
    elements, rels = load_registry()
    out_dir = UAF / "exports"
    out_dir.mkdir(exist_ok=True)
    n_elements, n_rels = build_csvs(elements, rels, out_dir)
    driver_path = out_dir / "gungnir-uaf-import.vbs"
    driver_path.write_text(DRIVER_TEMPLATE, encoding="ascii")
    print(f"wrote {driver_path} ({len(DRIVER_TEMPLATE.splitlines())} lines) "
          f"and 4 CSV files under {out_dir}: {n_elements} elements, {n_rels} relationships")
    return 0


if __name__ == "__main__":
    sys.exit(main())
