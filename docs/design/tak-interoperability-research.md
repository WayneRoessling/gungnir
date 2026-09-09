# TAK interoperability: the ecosystem as verified on 2026-09-08, and what it changes in DN-25

Status: research note, 2026-09-08. **The three decisions in §7 were taken by the owner
later the same day, adopting the recommendations as written; steps 1 and 2 of §8 are done
and the recording, step 3, is the open one.** The reasoning is kept as it was written. It
exists because [`DN-25-cursor-on-target.md`](DN-25-cursor-on-target.md) and
[`external-standards.md`](external-standards.md) §5 were written from documents, and two of
their assumptions do not survive contact with the clients' own source. The owner decisions it
asks for are collected in §7; everything before that is evidence.

## 1. The bottom line

1. **A stock TAK client on a multicast mesh sends protobuf, not XML, from its first
   datagram.** ATAK's contact manager initialises its broadcast version to the highest it
   supports and only drops to XML when a known contact advertises nothing higher than
   version 0 (§4.1). So `record_cot.py` on the default group will capture
   `takproto-v1` datagrams, and the "XML end to end" first increment in §5.4 of
   `external-standards.md` cannot decode its own corpus. The protobuf pin is owed now, not in
   I4.
2. **The streaming connection stays XML for as long as the server never advertises
   version 1** (§4.2). A recorder that listens on a TCP port and sends nothing is a server
   that never advertises, so a TCP mode on `record_cot.py` yields the XML half of the corpus
   from the same client, with no authoring and no negotiation. It also removes the Android
   emulator's multicast problem (§3.4).
3. **The reference client's source moved and is current.** The repository §5.5 verified is
   archived (2025-05-02, last release 4.6.0.5); the live one is `TAK-Product-Center/atak-civ`,
   GPLv3, latest release tag 5.5.1.8 (2025-10-28), carrying the same `protocol.txt` and
   `.proto` files (§4.3). If the protobuf framing is pinned, that is the repository and tag
   to pin.
4. **The `.proto` files carry no notice of their own.** The repository is GPLv3 with the
   README's statement that US federal employees' work is public domain in the United States.
   The licence question §5.5 deferred is therefore narrower than it looked, and §7 states the
   three routes so the owner can pick one rather than have one picked by default.
5. **All three clients are obtainable without a sponsor.** ATAK-CIV from Google Play and
   iTAK from the App Store need no account; WinTAK-CIV needs a tak.gov account, which any
   email address may apply for (§3). The corpus is a day's work once a device is in hand.
6. **The rest of the TAK surface, data packages, Data Sync, federation, plugins, is not
   needed for GAP-090 or GAP-091** and DN-25 §9's refusals stand (§5). One item is worth
   holding open for I4: OpenTAKServer as a GPLv3 *test harness* for the stream sink, on the
   same reasoning `external-standards.md` §1.5 already applies to GPL fixtures (§6).

## 2. What the workspace already has

Read before anything below, so this note does not restate it: DN-25 (the design, three
verification rows agreed), `external-standards.md` §5 (schema 2.0 pinned; type tree pinned;
`friend` is `^a-f-`; protobuf deliberately unpinned), `testdata/cot/SOURCE.md` and
`testdata/cot/tools/record_cot.py` (the recorder, exercised, holding nothing), edge (s)
accepted in `dependency-edges.md` §13, and GAP-091's 2026-09-08 note that the one blocker is
a client to record from. `ExchangeFormat` in `gungnir-model/src/exchange.rs` still has three
variants; `gungnir-interop` has no `cot` module; no binary opens a socket for it.

## 3. The clients

| Client | Platform | How obtained | Account | Mesh multicast | Server stream | Plugins |
|---|---|---|---|---|---|---|
| **ATAK-CIV** | Android | Google Play, or the APK from tak.gov / civtak.org | None for Play | Yes, default group `239.2.3.1:6969` | TCP and TLS | Yes; SDK zip on the GitHub release (5.5.1.8), GPLv3 |
| **WinTAK-CIV** | Windows 10/11 x64 | tak.gov installer (a 1.9.x civ installer is the current one linked) | tak.gov account; the registration page says any email may apply, and that restricted capabilities need a government address | Yes | TCP and TLS | .NET SDK and Visual Studio templates behind the tak.gov developer area; third-party templates exist on GitHub |
| **iTAK** | iOS | App Store, free, seller "United States Department of the Army, TAK Product Center", 2.12.3 (2025-08-08) | None | Yes, "UDP communication (Multicast) for SA only" since 2.5; peer-to-peer chat and packages since 2.7 | TCP and TLS, plus QR enrolment | Release notes claim an SDK since 2.5; in practice no third-party plugin route on iOS |
| **WebTAK** | Browser | Ships with TAK Server | Server account | No | WebSocket to the server | No |

Two practical points that decide how the corpus gets recorded:

- **An Android emulator does not deliver host multicast into the guest**, and the emulator's
  route to the host is unicast to `10.0.2.2`. An emulated ATAK can therefore reach the
  recorder only over a unicast UDP output or a TCP "server" connection, not over the
  group. A physical phone on the same Wi-Fi segment as the recorder joins the group, subject
  to the access point not filtering multicast.
- **WinTAK on the recording host itself** is the shortest route to a multicast capture: same
  machine, loopback-scoped or LAN group, no emulator. It is also the route that costs a
  tak.gov account and possibly a vetting delay.

Registration facts were read from search snippets of the tak.gov registration page, which
refused a direct fetch (HTTP 421) on 2026-09-08. The civtak.org announcement of 2020-09-23
describes WinTAK 4.1 as "approved for Public Release"; no export statement was found on
either page. **Treat "any email may apply" as read from a snippet, not from the page.**

## 4. The protocol, from the client's own source

Verified 2026-09-08 by reading `commoncommo/core/impl/protobuf/protocol.txt`,
`takproto/README.txt`, and `commoncommo/core/impl/{contactmanager,datagramsocketmanagement,
takmessage}.{cpp,h}` at `TAK-Product-Center/atak-civ` HEAD, and the `.proto` files beside
`protocol.txt`. All of it is GPLv3 source: **read, not copied**, per `external-standards.md`
§5.5.

### 4.1 Mesh: version selection starts at 1

`TakProtoInfo::SELF_MIN` and `SELF_MAX` are both `1`. `ContactManager` and
`DatagramSocketManagement` both initialise `protoVersion(TakProtoInfo::SELF_MAX)`. The
version thread recomputes the broadcast version as the intersection of every non-stale
mesh contact's advertised range with the client's own; **with no contacts the loop body
never runs and the result is `uberMax`, which is 1**. A contact that has sent only XML is
recorded with `protoInf` equal to its last mesh endpoint protocol, 0, so its presence makes
the intersection empty and the client drops to 0 ("No overlap; give up, go to 0").
`protocol.txt` states the same rule in prose: devices broadcast "the highest protocol
version supported by *all* known contacts", and every device supporting more than version 0
transmits a `TakControl` at least once a minute.

Consequences:

- A single stock ATAK or WinTAK on the group emits `0xbf 0x01 0xbf <TakMessage>` datagrams.
  Two of them keep doing so. `record_cot.py`'s manifest will say `takproto-v1` for every
  datagram of items 1 to 6.
- The only way a mesh recording comes out as XML is an XML-only contact on the group. The
  recorder cannot be that contact without transmitting an SA message, which is authoring,
  which `SOURCE.md` forbids. **iTAK may be that contact**: PyTAK's documentation warns that
  version 1 "may not work with all TAK clients (notably some versions of iTAK)", which is
  consistent with iTAK transmitting XML on the mesh, but it is a warning about a library's
  output and not a statement about iTAK's, and it is unverified here. If it is true, an iTAK
  on the group pulls every other client down to XML, which is worth knowing for the
  deployment as much as for the corpus.
- `commoncommo` is the library under WinTAK as well as ATAK (the `win32/takproto` project
  is in the same tree), so the same default is expected of WinTAK. Inferred, not observed.

### 4.2 Stream: XML until the server advertises

On a streaming connection the client sends XML, delimited by `</event>` and prefaced by an
XML declaration, until the *server* sends `<event type="t-x-takp-v">` carrying
`<TakProtocolSupport version="1"/>`; the client then requests with `t-x-takp-q` and the
server answers `t-x-takp-r`. Version 1 framing on the stream is `0xbf <varint length>
<TakMessage>`. `ContactManager::streamingMessageReceived` passes version 0 unconditionally,
so a stream peer never influences the mesh version.

A listener that accepts a TCP connection and writes nothing is, to the client, a server
that has not advertised. **Everything the client sends it is XML, for as long as the
connection lives.** That is the recording route for the XML half of the corpus, and it needs
neither certificate nor server.

### 4.3 The specification, where it now lives

| Artifact | Where | Notice |
|---|---|---|
| `protocol.txt`, the framing and negotiation | `TAK-Product-Center/atak-civ`, `commoncommo/core/impl/protobuf/` | None of its own; repository GPLv3 |
| `takmessage.proto`, `cotevent.proto`, `detail.proto`, `takcontrol.proto`, `contact.proto`, `group.proto`, `precisionlocation.proto`, `status.proto`, `takv.proto`, `track.proto` | same directory | **No header at all**: each file begins `syntax = "proto3"`. Package `atakmap.commoncommo.protobuf.v1` |
| `takproto/README.txt`, the prose description | `takproto/` at the repository root | None of its own |
| The archived copy §5.5 read | `deptofdefense/AndroidTacticalAssaultKit-CIV`, archived 2025-05-02, last release 4.6.0.5 | GPLv3; README's federal-work statement |

What `cotevent.proto` settles that the XML schema does not, transcribed because the codec's
mapping will need it: times are `uint64` milliseconds since the Unix epoch (`sendTime`,
`startTime`, `staleTime`); `hae`, `ce` and `le` "use 999999 for unknown"; `access` is
described as required by MIL-STD-6090 but carried as optional, with an omitted value meaning
"Undefined"; `caveat` and `releasableTo` exist as fields 16 and 17. `TakControl` carries
`minProtoVersion`, `maxProtoVersion` (0 reads as 1), `contactUid`, and `extensionIds` that
"must be centrally registered with TPC". The `Detail` message is a set of typed children
(`contact`, `group`, `precisionLocation`, `status`, `takv`, `track`) plus `xmlDetail`, a
string holding whatever XML the typed children did not absorb; `external-standards.md`
§5.2's consequence 2, that `detail` is validated by us or by nobody, carries over unchanged.

**The 999999 sentinel matters to DN-25 §5 rule 7.** On the XML side `ce` and `le` are
mandatory decimals with no stated confidence; on the protobuf side a client that does not
know writes 999999. A mapping that reads that as a one-sigma radius of a thousand kilometres
is wrong in a way no test written from the XSD would catch. It goes into the corpus check
and into the codec's doc comment beside the sigma multiple.

### 4.4 Servers

| Server | Licence | Activity | Speaks v1 | Ports | Reading |
|---|---|---|---|---|---|
| **TAK Server** (`TAK-Product-Center/Server`) | GPLv3 | 5.7-RELEASE-14, 2026-04-03 | Yes | 8089 TLS stream, 8443 API, 8446 enrolment, 9000/9001 federation | The reference. Java, PostGIS, Gradle; heavy for CI; binaries behind tak.gov |
| **OpenTAKServer** (`brian7704/OpenTAKServer`) | GPL-3.0 | 1.7.13, 2026-08-13; pushed 2026-09-02 | Yes: vendors `atak.proto`, depends on `pytak 7.6.1` | 8088 TCP stream, 8089 TLS stream, 8087 UDP, 8443 HTTPS API; enrolment, data packages, Data Sync; federation "coming" | Python, pip-installable, actively maintained. **The test-harness candidate for the I4 stream sink** (§6) |
| **FreeTAKServer** (`FreeTAKTeam/FreeTakServer`) | EPL-2.0 | v2.2.1, 2024-05-10 | Unknown | 8087 TCP, 8089 TLS | Stale since 2024; permissive licence is its one advantage |
| **taky** (`tkuester/taky`) | MIT | pushed 2024-07 | No | 8087 / 8089 | Small, stale, XML only |
| **GoATAK** (`kdudkov/goatak`) | AGPL-3.0 | v0.23.0, 2025-08 | Yes | -- | Go; a second server implementation to disagree with, never a dependency |

## 5. The wider TAK surface, and why DN-25's scope holds

| Surface | What it is | Bearing on GAP-090 / GAP-091 |
|---|---|---|
| **Mesh SA** | CoT over UDP multicast, one event per datagram | The inbound feed and the mesh sink. In scope |
| **Streaming CoT** | TCP or TLS to a server, negotiated framing | The stream sink, I4, behind SD-08's certificate. In scope as designed |
| **Data packages** | A zip with a `MANIFEST/manifest.xml` listing CoT, attachments and files, sent peer-to-peer or through the server | The natural carrier for a handoff's artefacts (an image, a route) if a human is to read them on a client. **Not needed** by either gap; a later increment if MT-08 wants it |
| **Data Sync / Mission API** | Server-side shared missions with change feeds, over the Marti REST API | Would give a shared picture to TAK clients with history. Requires a server and an authenticated caller; it is a third bearer, not a second, and nothing in the mission threads asks for it yet |
| **Federation** | TAK Server to TAK Server, protobuf over TLS, ports 9000/9001, a federation hub | Server-only. This product is not a TAK Server and DN-16/DN-18 already carry deployment-to-deployment exchange. Out |
| **ATAK plugins** | Java/Kotlin against the SDK on the 5.5.1.8 release, GPLv3 | DN-25 §9: "not a client plugin". A plugin would put our code on their release train under their licence. Out |
| **WinTAK plugins** | .NET SDK and templates behind tak.gov's developer area | Same answer, with the SDK itself gated. Out |
| **iTAK plugins** | No third-party route | Out by construction |
| **Meshtastic and LoRa gateways** | Community bridges that carry CoT over LoRa, including a TAK-server endpoint inside the Meshtastic iOS app since 2026-02 | A bearer under the bearer. Interesting for the fire-group needline NL-17 in a radio-denied case; nothing here changes for it, since it presents to us as mesh SA or as a stream |

## 6. Building the codec: crates, oracles, and the corpus

**Rust crates on crates.io.** None is a codec candidate, for the reason DN-25 §9 gives:
the codec is written from the pinned schema so that its doc comment can name what it
decodes. Their use is the one `gungnir-interop` already makes of `adsb_deku` and `rs1090`:
dev-dependency oracles for a differential gate, never named outside `tests/`.

| Crate | Version | Licence | What it is | Reading |
|---|---|---|---|---|
| `cot-proto` | 0.5.1 (2025-01) | Apache-2.0 | serde structs for the XML event and common TAK `detail` children; no protobuf | A second reading of the XML mapping. **Its bundled example `.cot` files were captured from the GPL repository's `takcot/examples`** (its README says so), so they are not fixtures for us either |
| `rustak` | 0.1.1 (2025-05) | MIT | UDP/TCP/TLS helpers and XML builders; protobuf "planned" | Thin; an XML-writer cross-check at most |
| `takproto` (Rust, `rabarar/takproto`) | 0.4.2 (2025-11) | MIT OR Apache-2.0 | Version 1 protobuf with mTLS and negotiation | The protobuf oracle candidate, **if** the owner's route in §7 allows generated types from the reference `.proto` at all; its own types are generated from them |
| `snstac/takproto` (Python) | 3.0.1 (2024-08) | MIT | Encoder/decoder; vendors the ten `.proto` files under `src-protobuf/` | A cross-check on the wire bytes, from a scripting language; and the demonstration that others have vendored the files under a permissive licence, which changes nothing about their origin |
| `snstac/pytak` (Python) | 7.6.1 (2026-08) | Apache-2.0 | The integration library OpenTAKServer builds on | As `external-standards.md` §5.6 already says: a cross-check, never the oracle |

**XML.** No XML crate is in the shipped graph on the Windows target today; `quick-xml`
0.30 enters only on Linux through the accessibility stack, and 0.41 is resolved but pulled
by nothing on this target. Two candidates, both needing a §2.9 record:

- `roxmltree` 0.21.1, MIT OR Apache-2.0: a read-only tree, which is exactly the shape a
  one-event-per-datagram decoder wants, and small enough to read. **Recommended for decode.**
- `quick-xml` 0.42.0, MIT: reader and writer; the advisory floor of `>= 0.41` from
  RUSTSEC-2026-0194/0195 is met. Recommended for encode only if a hand-written writer for a
  flat, attribute-only element turns out to need more than escaping, which it should not.

**Protobuf.** `prost` 0.14 is already a workspace dependency (§2.9, pinned with `tonic`)
and is used by nothing yet; the framing is a magic byte, a varint and one message. The
question is not the crate, it is the schema's origin, which is §7's second decision.

**The corpus, revised.** `external-standards.md` §5.6's six items stand. What changes is
that the corpus has **two halves from the same client session**, and `SOURCE.md` records
both:

| Half | How recorded | Wire form expected | Which rows read it |
|---|---|---|---|
| Mesh | `record_cot.py record` on the group, as now | `takproto-v1` from ATAK and WinTAK; possibly `xml` from iTAK (§4.1, unverified) | CAP-7.4's mesh-sink test, CAP-1.6's feed test |
| Stream | `record_cot.py record --tcp [port]` (added 2026-09-08): accept, read, frame, write nothing | `xml`, delimited by `</event>` | The XML codec's fixture test; CAP-7.4's stream-sink test in I4 |

The recorder change is standard-library and adds no authoring: it opens a listening socket,
accepts one client at a time, splits the byte stream on `</event>` exactly as
`takproto/README.txt` says clients delimit it, and writes the same `COTL` records with the
same manifest. It never sends a byte, and its docstring says so, because the moment it
sends `t-x-takp-v` it has become a party to negotiation and the corpus stops being a
recording of the client alone. A unicast-UDP mode (bind a port, no group join) is a
two-line variant for the emulator's `10.0.2.2` route and yields the same protobuf form as
the group.

## 7. Decisions asked of the owner

Each is a D-33 extension. **All three were taken by the owner later on 2026-09-08**, by
directing that these recommendations be followed; they are recorded under D-33 in
`../mission/gap-analysis/decisions-needed.md` and enacted in `external-standards.md` §5.4
and §5.5. The text below is as it was put to the owner.

**D-33(e) Pin the protobuf framing now.** §5.4's reason for not pinning was that nothing in
the first increment needs it. §4.1 shows the first increment's own corpus needs it: the mesh
feed and the mesh sink meet protobuf on their first datagram. The pin would be
`TAK-Product-Center/atak-civ` at tag `5.5.1.8`, `commoncommo/core/impl/protobuf/`, named in
the codec's doc comment beside the schema version and the guide's case number. Recommended:
**yes**, because the alternative is a mesh bearer that decodes only the client this
ecosystem is moving away from.

**D-33(f) How the codec learns the field numbers.** Three routes, in the order of least to
most contact with the GPL tree:

1. **Transcribe.** Hand-write the `prost`-derived structs from the field names and numbers
   in §4.3 and the `.proto` files, exactly as `external-standards.md` §5.2 transcribed the
   XSD from a public copy. No file is copied, no code is generated from the tree, and the
   doc comment cites the tag. The message set is small (ten files, one of them the union).
   **Recommended**, on the reading §5.5 already records: the wire format is the interface,
   the schema is the specification of it, and the `.proto` files carry no notice beyond the
   repository's.
2. **Vendor and generate.** Copy the `.proto` files under `testdata/` or a `proto/`
   directory with a `SOURCE.md` and run `prost-build`. This is the route §5.5's second rule
   refuses without an owner decision, and it is the route every Python and Rust library in
   §6 took. Cleanest engineering, most contact with the tree.
3. **XML only, stream bearer first.** Keep §5.4 as written, build the codec from the stream
   half of the corpus, defer the mesh feed and mesh sink to I4 with the protobuf pin. This
   inverts `Sd-Rm.md`'s reason for the split, since the mesh bearer was the one that needed
   no certificate, and leaves GAP-090's friendly set waiting on SD-08. Not recommended.

This note states the routes; it does not give legal advice on any of them.

**D-33(g) Which client records the corpus, and when.** Not a licence question, a logistics
one, and the one that has held GAP-091 since 2026-09-06. The cheapest complete recording is
**ATAK-CIV on an Android phone plus WinTAK-CIV on the recording host**, which gives item 6
(two senders), both wire forms, and a client on each side of the emulator problem. If a
tak.gov account is not wanted, **ATAK-CIV on a phone plus iTAK on an iPhone** gives two
senders without any account and tests §4.1's iTAK question in passing.

## 8. Recommended next steps, in order

1. **Owner:** take D-33(e), (f) and (g) above. One sitting; nothing else waits on more than
   one of them. **Done 2026-09-08.**
2. **Recorder** (`testdata/cot/tools/record_cot.py`; no crate, no owner): add `--tcp PORT`
   stream mode and `--unicast` UDP mode as §6 describes; exercise both against loopback with
   synthetic bytes that are then deleted, as the 2026-09-07 exercise did; update the
   docstring, `SOURCE.md`'s table (add the half, the port and the delimiter), and
   `external-standards.md` §5.6 "How to record it". Still no data. **Done 2026-09-08.**
3. **Record the corpus** with whichever pair D-33(g) chose, walking §5.6's six items on both
   halves, and commit `mesh.cotlog`, `stream.cotlog`, their manifests and the filled-in
   `SOURCE.md`. This is the step no session can do, and it is the only one.
4. **`gungnir-interop::cot`**, XML first: decode from `roxmltree`, encode by hand, the
   `friend` predicate as pinned, `ce`/`le` sigma multiple declared, conversion losses listed;
   fixture tests read the stream half; `cot-proto` as a dev-dependency oracle if the owner
   accepts a second §2.9 entry, else without it. Then the protobuf framing on the route
   D-33(f) chose, gated on the mesh half, with the 999999 sentinel test. Catalogue entry and
   conformance declaration alongside, as ASTERIX and AIS have.
5. **Model:** `ExchangeFormat::CursorOnTarget` and `ReportedPosition` as DN-25 §3 wrote them,
   in the same change as the codec per the versioning rule DN-25 cites.
6. **Feed** in `gungnir-ingest` on edge (i), behind the gateway, both wire forms, with the
   stale and quality rules of DN-16; CAP-1.6's row.
7. **Mesh sink** in `gungnir-remote` behind edge (s), drawn in `ARCHITECTURE.md` §7.1 in the
   same commit; the marking ceiling of DN-25 §5 rule 1; CAP-7.4's mesh row.
8. **`gungnir-policy`** (human-owned): DN-05 rule 1's three states; CAP-3.8's row.
9. **I4, stream sink:** OpenTAKServer as the harness, installed by CI as test infrastructure
   and linked into nothing, under a `testdata/`-style `SOURCE.md` naming the version and the
   licence; the sink's negotiation tested against a server that does advertise version 1,
   which is the case the recorder deliberately never produces.

## 9. What was and was not verified

**Verified 2026-09-08** by reading the artifact: the licence files of both reference
repositories (GPLv3 text) and the archive status and last release of the old one; the
successor repository's release tag and SDK asset; the `.proto` files' contents and absence
of headers; the version-selection logic quoted in §4.1 and the streaming rule in §4.2, from
the source files named there; `protocol.txt`'s framing bytes; OpenTAKServer's default ports
from `defaultconfig.py`, its vendored `atak.proto` and its `pytak` pin; the licences and
last-push dates of every repository in §4.4 and §6 from the GitHub API; the crates.io
metadata in §6; iTAK's App Store listing.

**Not verified:** WinTAK's mesh default (inferred from its shared library); iTAK's mesh wire
form; the exact wording of tak.gov's registration terms (read from search snippets after a
refused fetch); whether OpenTAKServer's TCP port advertises version 1 unprompted; and every
claim above about what the corpus will contain, which is what recording it is for.

## Sources

- [`TAK-Product-Center/atak-civ`](https://github.com/TAK-Product-Center/atak-civ) and its
  `commoncommo/core/impl/protobuf/protocol.txt`; the archived
  [`deptofdefense/AndroidTacticalAssaultKit-CIV`](https://github.com/deptofdefense/AndroidTacticalAssaultKit-CIV)
- [`TAK-Product-Center/Server`](https://github.com/TAK-Product-Center/Server)
- [`brian7704/OpenTAKServer`](https://github.com/brian7704/OpenTAKServer) and its
  [feature comparison](https://docs.opentakserver.io/feature_comparison.html)
- [`FreeTAKTeam/FreeTakServer`](https://github.com/FreeTAKTeam/FreeTakServer),
  [`tkuester/taky`](https://github.com/tkuester/taky), [`kdudkov/goatak`](https://github.com/kdudkov/goatak)
- [`snstac/pytak`](https://github.com/snstac/pytak), its
  [configuration page](https://pytak.readthedocs.io/en/stable/configuration/);
  [`snstac/takproto`](https://github.com/snstac/takproto) and its
  [protocol page](https://takproto.readthedocs.io/en/latest/tak_protocols/)
- crates.io: [`cot-proto`](https://crates.io/crates/cot-proto), [`rustak`](https://crates.io/crates/rustak),
  [`takproto`](https://crates.io/crates/takproto), [`roxmltree`](https://crates.io/crates/roxmltree),
  [`quick-xml`](https://crates.io/crates/quick-xml)
- [iTAK on the App Store](https://apps.apple.com/us/app/itak/id1561656396);
  [tak.gov registration](https://tak.gov/registration/registration_requests/new) and
  [products](https://tak.gov/products); [civtak.org, "WinTAK is Publicly Available"](https://www.civtak.org/2020/09/23/wintak-is-publicly-available/)
- [Hacker News thread on the DoD GitHub archive](https://news.ycombinator.com/item?id=44091100);
  [Meshtastic, "No Plugins, No Problem"](https://meshtastic.org/blog/tak-server-integration-ios/)

## Traceability

GAP-090, GAP-091; D-33 and the three extensions §7 proposes; DN-25 §2, §5 rule 7, §9;
`external-standards.md` §5.4, §5.5, §5.6, §5.8; `Sd-Rm.md`'s I3/I4 split of SD-16;
CAP-7.4, CAP-3.8, CAP-1.6. Principles AP-02 (the corpus says what it holds) and AP-07 (the
999999 sentinel is a conversion loss, recorded).
