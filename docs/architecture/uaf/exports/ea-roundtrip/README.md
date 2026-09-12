# EA round trip: what Enterprise Architect actually did with our XMI

These six files are the evidence base for round 8 of
[`../../tools/export_xmi.py`](../../tools/export_xmi.py). They are not generated
by anything in this repository and nothing reads them at build time. They are
kept because three earlier rounds of that exporter were built by reasoning about
EA's XMI dialect rather than by observing it, and each of those rounds shipped a
file that imported without error and produced no diagrams and no relationships.

## What they are

A small probe model, written by
[`../../tools/make_ea_probe.py`](../../tools/make_ea_probe.py), was imported into
Sparx Enterprise Architect. EA then exported what it had loaded, and those
exports are here. Anything in them is what EA itself writes, so it settles a
question about the dialect in a way that reading our own output cannot.

| File | What it is |
|---|---|
| `ea-export-xmi-2.1.xml` | EA's XMI 2.1 export of the imported probe. The primary reference. |
| `ea-export-xmi-2.1.log` | EA's own export log for it, listing each package and element processed. |
| `ea-export-xmi-1.1.xml` | The same model in XMI 1.1, for comparison. |
| `ea-export-xmi-1.1.log` | Its export log. |
| `ea-export-xmi-2.1-with-uaf-template.xml` | The probe **plus EA's own default UAF model template**. This is the authoritative one for UAF vocabulary: 21 view packages with their diagram types and `MDGView` tags, 23 distinct UAF stereotypes with the UML metaclass each extends, and UAF relationship connectors. |
| `ea-export-xmi-2.1-with-uaf-template.log` | Its export log. |

## What they settled

Recorded in full in `export_xmi.py`'s docstring under "Eighth version". In
short, five things earlier rounds had decided the wrong way: a connector needs
its own diagram entry; a relationship must not get an extension `<element>`
entry; a tagged value needs `xmi:id` and `modelElement`; `<ownedComment>` becomes
a Note element rather than documentation; and `Performs` is not a UAF stereotype
name.

They also confirm four things that were in doubt: `MDGView=UAF <Domain>::<Viewpoint>`
binds and survives, EA preserves our generated GUIDs verbatim, a package is
placed on its own diagram as a boundary frame by its `EAID_` id, and
`MDGDgm=SysML1.4::BlockDefinition` is correct for a Logical UAF view.

## How to use them for the stereotype names still unconfirmed

Seven of our nine relationship stereotypes and five of our eleven element
stereotypes are still literature-informed guesses. The round trip gives a
mechanical test for each, because EA files a stereotype it cannot resolve under
`thecustomprofile` instead of `UAF`. That is how `Performs` was caught:

1. `python docs/architecture/uaf/tools/make_ea_probe.py <out.xmi>` writes a small
   probe carrying the stereotypes to test.
2. Import it into EA with the UAF MDG Technology enabled.
3. Export the imported package as XMI 2.1.
4. Grep the export for `thecustomprofile:`. Every hit is a wrong name.

A name that comes back under `UAF:` is confirmed. Record the result in
`ELEMENT_KIND_INFO` or `RELATIONSHIP_KIND_INFO`, which says per entry whether it
is confirmed or a guess.
