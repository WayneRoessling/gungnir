# SAPIENT fixtures

Copied 2026-09-06 from the Dstl **Apex SAPIENT Middleware** repository at commit
`0c8591ae97f2dc336eff299845880d5c73ee75b7` (2024-08-28), under the terms
`docs/design/external-standards.md` §1.5 set for the ASTERIX capture and §7 restated for
SAPIENT:

- Test data only. Never linked into, embedded in, or shipped with a binary.
- Provenance recorded here: the repository, the commit, each file's blob id in that
  commit, its path in that repository, and the SHA-256 of the copy.
- Decoding them is a conformance check against the reference middleware's own messages,
  not a substitute for the specification. A field that decodes from a fixture but
  disagrees with the pinned document is a bug in the adapter, not a reading of the
  fixture.

Source: <https://github.com/dstl/Apex-SAPIENT-Middleware>.

## Licence, and one thing worth reading before relying on it

**Apache-2.0**, but the file that says so needs reading to the end and GitHub's own
detector does not agree. `license.txt` (copied here as `APEX-license.txt`) opens with a
DEFCON 703 contractual preamble about Roke's background IP and the licence Roke granted
Dstl; the Apache-2.0 grant is the second half, under the rule: "Dstl has chosen to
release this software under the Apache 2.0 license. Except where noted otherwise, the
Apex SAPIENT Middleware software is licensed under the Apache License, Version 2.0".
The GitHub API reports the repository's licence as `NOASSERTION`, because its detector
does not match a file with that preamble on it. **The grant is the text, not the
detector**, and the whole file is copied here so the next reader can check rather than
take this note's word for it.

The specification these messages are written against is separately licensed and is
**not** copied here: the SAPIENT Interface Control Document v7, DSTL/PUB145591,
2023-02-01, is Open Government Licence v3.0 and is cited by URL and edition in
`docs/design/external-standards.md` §7. BSI Flex 335 v2.0:2024-03 is the current
normative version, is free to read but not redistributable, and **governs where it and
the ICD differ** -- a risk §7 records and this fixture set inherits.

## What was copied

| File here | Path in the repository | Blob id | SHA-256 of the copy |
|---|---|---|---|
| `detection_proto.json` | `tests/resources/proto/bsi_flex_335_v1_0/detection_proto.json` | `c17757591bce4b7f4bfac571a74b0afd70dd32f1` | `c1bb214d4b1c2271b0657d226d8a28d9db2950c72cccda67695c7ef93d0a3fe3` |
| `registration_proto.json` | `tests/resources/proto/bsi_flex_335_v1_0/registration_proto.json` | `187f580c721c2f9c9c92322d77663cc791516c86` | `b28c6214b367fe4a84b6271fd4293fa46a01976e5a95725f9879ee661e52bddb` |
| `detection-range-bearing.xml` | `tests/resources/xml/proto_converted/detection.xml` | `c352385e57d277685fef76e8fa7d466d7cb65c16` | `4d9d518a8631701430c6ae486b77804f145b3128132f3f575dd1fddb32869ac6` |
| `APEX-license.txt` | `license.txt` | `7bbc9c575f52f473121301b828498cc2afa8a8e4` | `4ec447ba5261f8a8a24a9ad1e48a708bbf242d2386e2cfaf68af781140e6777d` |

Copied verbatim; nothing was edited, and no contributor names or addresses appear in
any of them.

## What the upstream set does **not** contain, and what was constructed instead

**There is no bearing-only protobuf-JSON sample upstream.** The whole of
`tests/resources/proto/` is the v1.0 message set, and its one detection sample carries a
Cartesian `location` with `LOCATION_COORDINATE_SYSTEM_UNSPECIFIED`. The only
range-bearing detection in the repository is `tests/resources/xml/proto_converted/detection.xml`,
which is the middleware's own XML rendering of a protobuf detection that carried a
`rangeBearing`: `Ele 1.0, Az 2.0, R 3.0, eEle 1.0, eAz 1.0, eR 2.0`. It is copied here as
the evidence for those values.

`spotter-session.jsonl` is therefore **constructed, not vendored**, and is marked as such
because a constructed fixture presented as a captured one is the kind of claim this
repository exists not to make. It is a seven-message session in the protobuf JSON mapping
of `sapient_msg.bsi_flex_335_v2_0.SapientMessage`:

| Line | What it is | Where its numbers come from |
|---|---|---|
| 1 | A registration declaring `NODE_TYPE_HUMAN` | The node id and the `icdVersion`/`name`/`shortName` shape are `registration_proto.json`'s and `detection_proto.json`'s; the node type is the one `registration.proto` v2.0 defines for "a human acting as part of a SAPIENT system (such as a spotter or guard)" |
| 2 | A bearing with an elevation and no range: the ordinary spotter report | Azimuth, elevation and their errors are `detection-range-bearing.xml`'s own `Az`, `Ele`, `eAz`, `eEle` |
| 3 | The same, with a lased range | Adds that file's `R` and `eR` |
| 4 | A Cartesian `Location` in degrees with a stated error | The classification and confidence shape is `detection_proto.json`'s; the coordinates are near the frame origin the test uses, and the errors are stated so the fixture exercises the path where the report's own error is kept rather than the baseline's assumed |
| 5 | A magnetic-datum bearing | The datum enumerant is `range_bearing.proto` v2.0's `RANGE_BEARING_DATUM_MAGNETIC` |
| 6 | A bearing with no `azimuthError` | Constructed: the case DN-27 §4 makes a refusal, because for a bearing the error *is* the information |
| 7 | A status report | Present so the "counted and named, never silently dropped" rule is exercised on a message kind this build does not act on |

Lines 5, 6 and 7 are **expected to be refused or counted, not decoded**, and
`gungnir-ingest/tests/sapient_spotter.rs` asserts each one by the name the adapter gives
it. A fixture set that only held messages the adapter accepts would gate half the rule.

## The one thing these fixtures are not

**They are the protobuf JSON mapping, not the binary wire format.** SAPIENT's transport
is length-prefixed binary protobuf over TCP, and decoding it needs a protobuf runtime
that is not in `[workspace.dependencies]`. `gungnir_ingest::adapters::sapient` reads the
JSON mapping and says so in its own documentation; the binary bearer is an open row.
