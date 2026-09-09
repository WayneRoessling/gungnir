# Origin of the Cursor-on-Target corpus in this directory

**Nothing has been recorded. This directory holds the recorder and no data.**

That sentence is the point of the file. An empty fixture directory with no note beside it
reads as an oversight; this one is a state, and the state is that no TAK client has been
available to record from. `docs/design/external-standards.md` §5.6 specifies the corpus --
what a client must be made to emit, why each item is needed, and how the recording is
made -- and `tools/record_cot.py` performs it. Until someone runs it with a client, the
`gungnir-interop` CoT codec is not written and GAP-091 stays open, which is the order
GAP-064's rule sets: the corpus, then the codec.

## Two halves, since 2026-09-08

`docs/design/tak-interoperability-research.md` §4 found, in the reference client's own
source, that a stock client sends TAK Protocol Version 1 (protobuf) on the multicast mesh
from its first datagram, and stays on XML over a server connection only for as long as the
server never advertises version 1. So the corpus is recorded twice from the same client
session, once per bearer, and each half is a pair of files:

| Half | Command | Transport | Expected `wire` | Files |
|---|---|---|---|---|
| mesh | `record` (group) or `record --unicast` (bound port, no group) | UDP | `takproto-v1` from ATAK and WinTAK; iTAK unverified | `mesh.cotlog`, `mesh.manifest.json` |
| stream | `record --tcp [port]`, default 8087 | TCP, to a recorder that never sends | `xml` | `stream.cotlog`, `stream.manifest.json` |

## Why this one is recorded rather than copied

`testdata/adsb/` was filled on 2026-09-06 by copying two permissively licensed captures
from open-source projects, and that route was the right one there: ADS-B decodes a
transmitter's output, so an open-source ADS-B corpus is a recording of real transmitters
with a licence attached.

**The equivalent does not exist for this format.** The permissively licensed CoT corpora
(`snstac/pytak`, Apache-2.0; `snstac/takproto`, MIT) are test vectors their authors wrote
to exercise their own encoders, not recordings of a client, and the `.cot` example files
in the reference client's `takcot/examples` are GPLv3 and authored too. They are worth
copying later as a second opinion to disagree with, and §5.3 and §5.6 say exactly that --
a cross-check, never the oracle. Only a recording produces what a deployment will actually
receive.

## The rule

**Recorded, never authored, never answered.** No file in this directory may be
hand-written CoT, `record_cot.py` has no mode that would produce any, and it has no code
path that sends a byte: a recorder that answered a client's negotiation would make the
stream half a record of the recorder's choices rather than the client's. A corpus written
by the same hand that writes the decoder passes against itself and fails against every
real client. That is why `external-standards.md` §1.5 words its conditions around a
fixture's *origin* rather than its content, and it is why this directory is empty rather
than seeded.

## What to fill in when the recording is made

Replace this section with the record. Every number below is printed by
`python testdata/cot/tools/record_cot.py summarize`, once per half, so it is read back
from the file rather than typed from memory.

| Field | mesh half | stream half |
|---|---|---|
| Client(s) and exact version(s) | | |
| Platform(s) they ran on | | |
| Date and time zone of the recording | | |
| Group, port, interface (mesh) / listening port (stream) | | |
| Loopback-scoped or LAN | | |
| `.cotlog` SHA-256 | | |
| Records | | |
| Payload bytes | | |
| Wire forms (`xml` / `takproto-v1` / `takproto-v1-stream` / other) | | |
| Transports (`multicast` / `unicast-udp` / `tcp`) | | |
| Senders, and how many records each | | |
| Sender addresses stripped? | | |

Then, by sequence range and per half, which records are which of §5.6's six items:

| Item | mesh range | stream range | Note |
|---|---|---|---|
| 1 Own position over several minutes | | | |
| 2 Each affiliation the client offers | | | The affiliation mapping is **pinned** as of 2026-09-07 (`external-standards.md` §5.7: `friend` is `^a-f-`, case-sensitive, from MITRE's August 2005 guide). This item is what checks that pin against a client rather than against a twenty-year-old document. **Record the exact type strings here**, and if any client disagrees with the guide, that is the finding §5.7.5 is waiting for |
| 3 A marker placed by hand | | | |
| 4 A chat message | | | |
| 5 A deletion, and an event left to expire | | | |
| 6 A second client on the group / connected to the port | | | `summarize` warns when a half has only one sender |

And, in the manner of `testdata/adsb/SOURCE.md`'s account of its truncation: **what the
corpus does not contain.** Any of the six items that could not be produced on either
half, named, with what each one costs the verification rows that read this corpus (CAP-7.4
and CAP-1.6 in `docs/verification-capability-table.md` §2). If iTAK was on the group,
record which wire form it sent and whether the other clients dropped to XML while it was
there, because `tak-interoperability-research.md` §4.1 left that unverified.

## Files

| File | What it is |
|---|---|
| `tools/record_cot.py` | The recorder. Joins the group, binds a port, or listens on TCP; frames each message with its receipt time; never decodes a payload; never sends |
| `mesh.cotlog` | Not present. The datagrams, length-delimited, in arrival order |
| `mesh.manifest.json` | Not present. Per datagram: sequence, receipt time, sender, transport, length, SHA-256 and wire form |
| `stream.cotlog` | Not present. The stream messages as the client framed them, length-delimited, in arrival order |
| `stream.manifest.json` | Not present. The same fields per message |

The recorder was exercised against loopback multicast on 2026-09-07 with four synthetic
datagrams, and again on 2026-09-08 against loopback TCP and unicast UDP with synthetic
bytes in both framings, a connection closed mid-message, and two senders; everything was
written to a scratch directory and deleted. It joined the group, bound the port, accepted
the connections, framed the messages, distinguished the wire forms by the `0xbf` byte and
the `</event>` token, read each log back and reported the senders. **That exercised the
recorder, not the format**, and it left nothing here.
