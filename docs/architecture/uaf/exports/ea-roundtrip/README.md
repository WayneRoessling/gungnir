# EA round trip: what Enterprise Architect actually did with our XMI

These files are the evidence base for rounds 8 to 12 of
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

## Run 2: the full export, and the profile list

`run-2/` holds EA's export after importing the full 59-diagram model rather than
the probe, with its export logs. Everything round 8 fixed held at scale. It also
settled the stereotype question outright, because EA's export declares every
stereotype of every enabled profile in its `<profiles>` block: the UAF profile
(`nsPrefix="UAFP"`) has 213, each with the metaclass it extends. That list is
now what `ELEMENT_KIND_INFO` and `RELATIONSHIP_KIND_INFO` are checked against;
the "grep for `thecustomprofile:`" oracle below still works, but the list is
faster and complete.

Run 2 also showed two ways a wrong name can look bound: EA matches
case-insensitively against every enabled profile, so `Achieves` landed in
BIZBOK and `PersonType` in UPDM; and a tagged value whose name matches a
property of some stereotype gets that stereotype applied, so `owner` became
`UMM2:bLibrary` on 159 elements. The exporter now prefixes every registry-field
tag `registry.` to stay clear of every profile's property names.

## Run 3: the last two open questions, and one answer that reversed a decision

`run-3/` holds EA's export after importing the probe, which carried the two
things the export still rested on without a round trip of their own. Both are
settled.

**Every stereotype name is confirmed by EA itself.** `Capability`, `Exhibits`,
`IsCapableToPerform`, `OperationalActivity`, `OperationalPerformer`,
`ResourceArtifact` and `Standard` came back under `UAF:`; `Requirement` and
`satisfy` under `SysML:`. Nothing landed in `thecustomprofile` except the `uafId`
tag name, which EA turns into a custom stereotype for any tag it does not
recognise. That is harmless and the tag's value survives.

**A profile property must travel as an extension tag, not as an attribute of the
stereotype application.** The probe sent `conformsTo` both ways. The tag came
back verbatim. The attribute came back corrupted: `SD-1 TLS 1.3 (RFC 8446)`
became `EAID_D_1 TLS 1.3 (RFC 8446`, the leading registry id rewritten as though
it were an element reference and the closing bracket dropped. So the attribute
form is gone from the exporter, even though it is XMI's own encoding and the
form run 2 saw EA *export* for `category`. What EA writes and what EA reads are
not the same thing, which is the general lesson of all three runs.

It also caught the probe drifting from the exporter it exists to model: the probe
still emitted `<ownedComment>`, dropped from the exporter in round 8, and seven
nameless Note elements came back with it.

## How to use them for a stereotype name not yet confirmed

Seven of our nine relationship stereotypes and five of our eleven element
stereotypes are still literature-informed guesses. The round trip gives a
mechanical test for each, because EA files a stereotype it cannot resolve under
`thecustomprofile` instead of `UAF`. That is how `Performs` was caught:

1. `python docs/architecture/uaf/tools/make_ea_probe.py <out.xmi>` writes a small
   probe carrying the stereotypes to test.
2. Import it into EA with the UAF MDG Technology enabled.
3. Export the imported package as XMI 2.1.
4. Grep the export for `thecustomprofile:`. Every hit is a wrong name.

The probe also carries a resource conforming to a standard, written the two
ways EA writes a string property (an attribute on the stereotype application
and an extension tag named `conformsTo`), so the same import shows which of
the two EA reads back onto the element.

A name that comes back under `UAF:` is confirmed. Record the result in
`ELEMENT_KIND_INFO` or `RELATIONSHIP_KIND_INFO`, which says per entry whether it
is confirmed or a guess.
