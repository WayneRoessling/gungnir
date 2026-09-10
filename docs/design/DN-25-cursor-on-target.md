# DN-25 Cursor-on-Target exchange

Closes GAP-090 and GAP-091. Status: first draft, 2026-09-06; **the design (§1 to §9) signed
by the owner as the one to build, 2026-09-10. No code exists.**

## 1. The gaps and the thread steps they block

Two gaps, one boundary.

**GAP-091, the bearer.** DN-16 and DN-18 are wired, and both work only with a participant
that can hold a machine identity in this deployment's trust roots and read the v2 contract:
the peer link is a mutual-TLS client of another node's event stream, and the exchange gates
run for a machine caller whose party is read from its client certificate. Every needline they
carry has participants who will never be that. A mobile fire group is a truck and four
people. A port authority runs somebody else's software. A neighbouring unit has whatever its
own ministry issued it. `ExchangeFormat` already names two formats for a partner who is not
another instance of this product, and **neither exists**: STANAG 4676 has no obtainable
specification (`external-standards.md` §2), and ASTERIX encode is refused by design, because
a fused track is not a monoradar report (§1.8). So MT-08 step 6, "disseminate identities,
warnings, and products to roles and peers", reaches deployments of this product and nothing
else, and MT-01 step 7, "warn assets on the predicted routes", has a ledger with no bearer
under it (DN-03: "no transport exists, so every warning fails loudly").

**GAP-090, the friendly set.** DN-05 §5 rule 1 denies a fires task whose target error
ellipse contains a known friendly, and GAP-036 supplies it with "friendly positions from the
tracks carried as friendly". That is every friendly **a sensor detected and the
identification engine declared**. A dismounted section in a wood line, a fire group moving
between firing points, a patrol boat inside the radar's minimum range: none of them are
tracks, so none of them are in the set. The defect is not that the check is missing. It is
that **an empty friendly set reads as "no friendly is there" when it means "no friendly was
detected"**, and those two are the same value today. Rule 2 saves the cases where a source is
configured and silent; it cannot save the case where the concept of a self-reporting friendly
does not exist. MT-06's fires step and MT-01's engagement steps both run on that set.

One boundary closes both, because the same participants who cannot hold a machine identity
are the ones whose positions nothing observes.

## 2. The owning component

**No new crate**, and saying so is half the content. A "TAK module" would duplicate four
things that exist:

| Part | Owner | Why there |
|---|---|---|
| The codec: bytes to and from typed values | `gungnir-interop` | The same boundary as `AsterixCat048Codec` and the AIS decoder: framing and fields, no transport, no policy, a pinned schema version in the doc comment |
| The inbound adapter | `gungnir-ingest` | DN-16 §2's argument, only stronger. A self-reported position is the least trustworthy input the system has, and the gateway that validates and quarantines a radar must validate this |
| The outbound sink | `gungnir-remote` | `endpoint.rs` and `peer.rs` are where outbound connections, retries and refusals already live |
| Which data may go where | `gungnir-model` and the existing gates | `ExchangeSet::may_send` and `Releasability::permits`, unchanged. This note adds no second answer to a question already answered |

**Signed by the owner 2026-09-10: this table's four-way split, and the "no new crate"
finding it rests on, is the right shape.** Checked before that signature against what each
named type and function actually does today (`PeerOrigin`'s `assigned_quality`/`age_s`/
`is_stale_beyond`, `ExchangeFormat::is_lossy`, `ExchangeSet::may_send`,
`Releasability::permits`), against `external-standards.md` §5's pinned schema, §5.7's
`friend` predicate and §1.5's copyleft rule, and against `dependency-edges.md` (see §4's
note on what that check turned up). No code exists; this signature is on the design alone,
the same two-step DN-28 and DN-29 went through -- a future `gungnir-model`/`gungnir-interop`/
`gungnir-ingest`/`gungnir-remote` diff is signed on its own account, against this design.

## 3. Types

In `gungnir-model`, an additive variant on the enum DN-18 defined:

```rust
pub enum ExchangeFormat {
    Canonical,
    Stanag4676,
    Asterix048,
    /// Cursor-on-Target, for a participant who cannot hold a machine identity here.
    CursorOnTarget,
}
```

`is_lossy` already answers `true` for everything that is not `Canonical`, so the variant
inherits the right answer without a change. Adding a variant is readable by an older build
only until a baseline names it; that is the versioning rule in `../gungnir-api-v1.md`, and it
is why the variant lands with the codec rather than ahead of it.

New, in `gungnir-model`:

```rust
/// A position an entity reports about itself, from something that is not a sensor.
///
/// **Never a track.** DN-16 refused to make a track out of a peer's launch warning,
/// because a track we have not observed is a track we cannot maintain. A self-report
/// is the same case and takes the same answer.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReportedPosition {
    /// Timing and assigned quality, exactly as a peer-sourced track carries them.
    pub origin: PeerOrigin,
    /// Callsign or unit as the reporter states it. Shown as theirs, and never
    /// resolved against the order of battle unless a person does it.
    pub reporter: String,
    /// WGS-84 as it arrived. **The caller anchors it to the local frame**, because
    /// the conversion lives in a crate this type's consumers may not depend on --
    /// the correction DN-01 §3a had to make, not repeated here.
    pub position: Geodetic,
    /// The accuracy the reporter claims, in metres. Recorded on the provenance and
    /// weighted by nothing.
    pub claimed_accuracy_m: Option<f64>,
    /// What the reporter says it is, from the type's `friend` predicate and
    /// nothing else. Only `Friendly` is acted on; see §5 rule 3.
    pub affiliation: Classification,
}
```

`PeerOrigin` is reused rather than paralleled: its `assigned_quality`, `age_s` and
`is_stale_beyond` are the same three rules this type needs, and a second timing-and-quality
struct beside it would drift.

## 4. Edges

**One, proposed:** `gungnir-remote` to `gungnir-interop`, for the outbound sink to encode.
It would be edge (v) in `dependency-edges.md` (recorded there as such; relabelled from
(s) in review, §16's own note has why). Direction is the same as edge (i)
(`gungnir-ingest` to `gungnir-interop`, in the manifest since 2026-09-06) and adds no cycle:
`gungnir-interop` depends on `gungnir-model` alone.

The alternative considered and rejected: leave encoding in `gungnir-interop` and give the
socket to each host binary, which is how the ASTERIX adapter is wired. Rejected because the
desktop and the node would each grow a socket, a retry loop and a refusal count that
`endpoint.rs` already has, and DN-07's handoff already proved that path belongs in
`gungnir-remote`.

**Accepted by the owner as engineering reviewer, 2026-09-06**, and recorded as edge (v) in
[`dependency-edges.md`](dependency-edges.md) §16 -- relabelled 2026-09-09 from (s)/§13, which
collided with the real, already-drawn `gungnir-node` to `gungnir-identity` edge of the same
letter (`dependency-edges.md` §16's own note has the finding). Per the rule it enters a
manifest, and is drawn in `ARCHITECTURE.md` §7.1, in the change that adds the sink and not
before: an edge in the graph that no manifest carries is the graph claiming something that
is not true. Inbound needs no new edge.

## 5. Behaviour

**1. The marking decides, and one bearer cannot establish a party at all.** DN-17 states
that releasability is a property of the data, not of the channel, and `Releasability::permits`
gives an empty party nothing, because there is no anonymous peer. A TLS stream to a server
that presents a certificate is a party like any other, and `ExchangeSet::may_send` runs over
it unchanged. **A multicast mesh sink has no party.** It is therefore configured as a named
party whose name is *this deployment's own claim* rather than an authenticated identity, the
baseline records it as such, and only data marked to that party **by name** goes there:
never `Internal`, and never `AllPeers`, because `AllPeers` promises "any authenticated peer"
and multicast authenticates nobody. Refusing that is the safety content of this note. An
implementation that treats a mesh sink as an ordinary peer is the failure this section
exists to prevent.

**2. A self-report is not a track and never enters fusion.** It is drawn, it is an input to
DN-05 rule 1 and to DN-03's recipient list, and it is nothing else. Never allocated against,
never a detection, never correlated into an identity without a person doing it. The reason is
the one DN-16 gave: we did not observe it, so we cannot maintain it, and a thing on the map
that looks like a track but decays on somebody else's schedule is worse than no thing.

**3. A self-report may lower risk and never raise it.** Only `Friendly` is acted on. A
report claiming `Hostile` is recorded with its reporter and does nothing: an unauthenticated
sender who can create a hostile declaration can aim this system, which is the one outcome
worth designing against in advance.

**The test is the pinned predicate, not the second element of the type.**
`external-standards.md` §5.7 pins `friend` as the anchored, case-sensitive expression
`^a-f-`, published in MITRE's August 2005 guide, and §5.7.2's finding 3 is why the
distinction is not pedantry: affiliation exists **only in the atoms branch**, so a chat
message or an image (`b-...`) has no affiliation at all, and a decoder reading "the second
element" would manufacture one for it. The anchored predicate answers both questions at
once and refuses `b-` by construction. Nothing else about the type is interpreted: the
remaining branches are carried raw, because §5.7.2's finding 1 says partial understanding is
the format's design and a decoder that demanded the whole path would refuse what CoT expects
it to accept.

**4. Quality and age are DN-16's, unchanged.** `assigned_quality` comes from our
configuration; the age between the reporter's stamp and our receipt is on every reported
position and on screen; beyond the configured maximum it is stale, and a stale friendly is
not a cleared fire mission.

**5. An empty friendly set stops meaning "no friendly is there".** This is what GAP-090
actually buys. Once a reported-position source is configured, DN-05 rule 1 can distinguish
three states that are one state today: the source is live and reports nobody in the ellipse;
the source is configured and silent, which is rule 2's failed check; and no source is
configured, which is what the panel must say rather than showing a pass. Until such a source
exists, the friendly set is the detected friendlies only, and PN-05 says so in those words.

**6. Delivery over a bearer that cannot acknowledge is never shown as delivered.** DN-03's
ledger moves a warning to `Acknowledged` on a receipt. Mesh multicast produces none, so a
mesh-borne warning reaches `Sent` and stops there, and the panel distinguishes it from one
that was acknowledged. Fire-and-forget presented as delivery is precisely the failure DN-03
§5 named: "a warning that quietly fails is worse than none, because the operator believes the
asset was warned."

**7. Every conversion loss is recorded, not assumed away.** Three are known before a line is
written: a full state covariance becomes a single circular error and a linear error; mission
time becomes the format's own time triple; and our four-value affiliation becomes a type
string that carries more than affiliation. Each goes on `Provenance::conversion_loss`, which
is where the ASTERIX mapping already puts its losses.

**8. Failure is loud on both halves.** A sink that cannot open its socket is unhealthy and
says which sink; a feed that goes quiet is a source that went quiet, on the same path as a
peer that stops sending; a datagram that does not decode is counted under the reason it did
not, never skipped.

## 6. Configuration and interface delta

The baseline's `exchange` section gains sinks, and the ingest configuration gains feeds,
bound per entry on both binaries the way `ais_feeds` already is:

| Setting | Shape | Validation |
|---|---|---|
| `exchange.cot_sinks[]` | name; the party it sends as; a bearer, either a multicast group and port or an endpoint reference; the item types (`ExchangeItem`) | Unique names; the party has an agreement; a `Stream` bearer names an endpoint and a client certificate (GAP-060); **a `Mesh` bearer is refused for any sink whose items could carry a marking above the named party**, per §5 rule 1 |
| `ingest.cot_feeds[]` | name; source (socket or recorded file); assigned quality; maximum age | Unique names; quality in 0.0 to 1.0; positive age |

**No new API endpoint, and no contract change.** This is a bearer for information the v2
contract already defines, which is the same finding DN-18 recorded: coalition exchange needed
an agreement, not a module.

**The precondition is met.** [`external-standards.md`](external-standards.md) §5 pins the
schema -- version 2.0, 13 June 2003, MITRE case #11-3895, approved for public release --
transcribes in §5.2 the attributes this note's mapping depends on, and records in §5.4 that
the protobuf framing is deliberately **not** pinned, because nothing in the first increment
needs it. GAP-064's rule is satisfied ahead of the codec rather than behind it, and §5.2
returned one requirement to this note that a summary would have lost: **`ce` and `le` carry
no stated confidence level**, so the mapping declares which multiple of sigma it writes, on
the sink and in the conversion loss.

**The protobuf framing was pinned on 2026-09-08 after all** (§5.4 there, D-33(e)), and the
reason reaches back into this note: a stock client on the mesh sends protocol version 1 from
its first datagram, so "XML end to end" was true of the stream bearer and never of the mesh
bearer, and it is the mesh bearer this note puts first. Two consequences for §5. The codec
decodes both wire forms from the first increment, gated on a corpus that now has a mesh half
and a stream half ([`tak-interoperability-research.md`](tak-interoperability-research.md)
§6). And rule 7 gains a fourth known loss before a line is written: the protobuf fields
`hae`, `ce` and `le` write `999999` for unknown, and a mapping that read that as a radius
would carry a thousand-kilometre error that no gate refuses; it is recorded as "no error
stated", never as a number. Rules 1 to 8 are otherwise unchanged, and rule 3 in particular:
the framing changes how affiliation arrives, not what is done with it.

**The type tree was pinned on 2026-09-07** (§5.7), which this note had left open and §5.6
had named as the gap. It returns two more requirements. The friendly test is the pinned
`friend` predicate `^a-f-`, anchored and **case-sensitive** -- §5.7.2's finding 5 records
that upper case is MIL-STD-2525B and lower case is a CoT extension, and that a decoder which
case-folds a type string conflates the two. And the codec's doc comment names three things
rather than two: the schema version, the guide's case number, and the predicate it
evaluates.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-02 Viewport | Reported positions drawn distinctly from tracks, the distinction being pattern and label rather than colour alone |
| PN-03 Track table | Unchanged. A reported position is not a track and does not appear in the track table |
| PN-05 Recommendation | Rule 1's result names the friendly set it read and where each entry came from: detected, reported, or "no source configured" |
| PN-08 Alerts, PN-04 Track detail | A warning carried on a bearer that cannot acknowledge shows `Sent`, visibly distinct from `Acknowledged` |
| PN-09 System health | Per sink and per feed: state, message rate, refusal count with the reason, and the marking ceiling each sink enforces |
| PN-01 Status strip | A sink refusing everything on its marking ceiling, because that reads as a broken link and is not one |

## 8. Verification

**All three rows were agreed by the owner on 2026-09-06 and have moved into**
[`../verification-capability-table.md`](../verification-capability-table.md) §2, under
"Rows added by DN-25"; [`verification-rows.md`](verification-rows.md) maps them back here.
They are reproduced below as this note drafted them, and **the table is their one home**: a
later change to any of the three is a change request, not an edit. The table's wording is
tighter than the drafts below in two places -- the CAP-7.4 row carries the sigma multiple
that §5.2 turned up, and the CAP-1.6 row checks the never-a-track rule on what the type can
reach rather than on a branch -- and where the two differ, the table governs.

**No existing criterion was widened.** CAP-7.4, CAP-3.8 and CAP-1.6 each already carried a
row agreed on 2026-09-05 and all three stand untouched; these sit beside them, because a
second bearer, a second source of friendly positions and a second kind of inbound thing are
not second readings of the same criteria.

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-7.4 Peer and coalition exchange, this bearer | Round-trip through the codec, plus a sink test per bearer with markings | A payload marked `Internal` reaches no sink; a payload marked `AllPeers` reaches a stream sink and **never** a mesh sink; a mesh sink emits only what is marked to its named party; every emitted item records its conversion loss; a sink whose socket fails is unhealthy and named | Generated pictures at each marking; a listener on the multicast group |
| CAP-3.8 Fires deconfliction, friendly set provenance | Unit tests on rule 1 with each of the three states | A reported friendly inside the ellipse denies; a live source reporting nobody passes and says so; **no configured source is a failed check with that reason, never a pass**; a stale reported friendly does not clear a fire mission | Generated reported positions with controlled ages |
| CAP-1.6 Peer early warning, self-reports | Feed a recorded corpus through the gateway | A self-report never becomes a track, a detection, or an allocation target; a report claiming an affiliation other than `Friendly` changes nothing but the record; assigned quality never exceeds the configured value; age is present on every reported position | Recorded corpus; TT-01 replayed with a reporting fire group |

## 9. What this note does not do

Stated because each one is a thing a reader may assume follows, and none does.

- **No inbound tasking, decision, or control of any kind.** No engagement decision, no
  weapons control status, no sensor task arrives on this bearer. Identity and authority do
  not survive it.
- **No effector report path.** DN-07's `EffectorReport` names a decision id and is
  authenticated as the effector's machine identity; this bearer can carry neither. The
  outbound handoff can be mirrored here for a human to read; the report comes back the way
  DN-07 says.
- **Not a replacement for the canonical bearer.** No covariance, no `PolicyVerdict`, no
  `DecisionRecord`, no audit entry, no marking field on the wire. Peers that can hold a
  machine identity keep using DN-16 and DN-18.
- **Not a client plugin and not a server.** Both are products on somebody else's release
  train.
- **No third-party code.** The reference implementations of this ecosystem are GPLv3
  (verified 2026-09-06 at both upstream repositories). A wire format is not a derivative
  work, but their source is: the codec is written from the published schema, and no file,
  generated type, or test fixture is taken from a copyleft repository without the conditions
  `external-standards.md` §1.5 already sets for exactly that case.

## Traceability

GAP-090, GAP-091; CAP-7.4, CAP-3.8, CAP-1.6; SD-16 in
[`../architecture/uaf/standards/Sd-Tx.md`](../architecture/uaf/standards/Sd-Tx.md);
needlines NL-17 and NL-18 and the bearer table in
[`../architecture/uaf/operational/Op-Cn.md`](../architecture/uaf/operational/Op-Cn.md).
Depends on DN-16 for quality, age and the gateway; on DN-17 for the marking, whose
channel-versus-data rule §5 rule 1 turns on; on DN-18 for the agreement and the two gates; on
DN-05 for the deconfliction rule GAP-090 feeds; on DN-03 for the warning ledger and its
`Sent`-versus-`Acknowledged` distinction; on DN-01 §3a for anchoring in the caller. Requires
[`external-standards.md`](external-standards.md) §5 for the pinned schema, the transcribed
attributes and the licence finding, and §5.7 for the type tree, the `friend` predicate and
the case-sensitivity finding; **D-33** for the scope decision and the pins, extended
2026-09-07 to cover the type tree; edge (v) accepted 2026-09-06 and recorded in
[`dependency-edges.md`](dependency-edges.md) §16 (relabelled 2026-09-09 from (s)/§13, which
collided with a different, real edge of the same letter -- §16's own note has the finding).
Principles AP-02 (honest status: §5 rules 5, 6 and 8), AP-06 (one owning crate per type: §2),
AP-07 (provenance travels with the data: §5 rule 7), and above all **AP-09** (releasability is
a property of the data, which is what §5 rule 1 refuses to trade away for a convenient
bearer).
