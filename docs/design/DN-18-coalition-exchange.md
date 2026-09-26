# DN-18 Peer and coalition exchange

Closes GAP-065. Status: first draft, 2026-09-05; amendment 1 (2026-09-06) built the three
read routes; amendment 2 (2026-09-08, written and gated where human-owned) built the write
path, its store-and-forward, and the handoffs producer. **Not fully closed**: no producer
exists yet for `Warnings` or `Reports`, and the format round-trips and interop suite
(GAP-063, GAP-064) are untouched. What the owner has signed of this note is in
[`../signatures.md`](../signatures.md).

## 1. The gap and the thread step it blocks

MT-01 warning and MT-08 dissemination stop at the sector boundary. Pictures, warnings, and
products are not exchanged with neighbours, higher command, or partners, so the sector is
an island.

## 2. The owning component

**None new.** This note is a composition, and saying so is its main content:

| Direction | Mechanism | Designed in |
|---|---|---|
| Tracks and warnings inbound | Peer source adapter | DN-16 |
| Picture outbound | The existing snapshot and event stream | The v1 contract |
| Products outbound | Reports with markings | DN-17, DN-19 |
| Decided assignments outbound | Handoff | DN-07 |
| Who may receive what | Releasability enforcement per caller | DN-17 |
| Format on the wire | ASTERIX and STANAG 4676 | GAP-064 |

`gungnir-api` and `gungnir-interop` own what little is left, and both already have the
dependencies they need.

The finding worth stating: **coalition exchange needs almost no new mechanism.** What it
needs is the four notes above and an agreement. Discovering that during implementation
would have produced a "coalition module" duplicating all four.

## 3. Types

In `gungnir-model`:

```rust
/// A configured exchange relationship. Names what flows in each direction, so a
/// partner that may send tracks but not receive plans is expressible.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExchangeAgreement {
    /// Matches the peer name in the peer table and the party in a marking.
    pub party: String,
    pub inbound: Vec<ExchangeItem>,
    pub outbound: Vec<ExchangeItem>,
    /// Wire format for this partner, when it is not our own schema.
    pub format: ExchangeFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExchangeItem {
    Tracks,
    Warnings,
    Reports,
    Handoffs,
    Health,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExchangeFormat {
    /// Our own schema, for another instance of this product.
    Canonical,
    Stanag4676,
    Asterix048,
}
```

## 4. Edges

**None.**

## 5. Behaviour

**The agreement is the configuration, and it is symmetric in expression but not in trust.**
What we send is governed by the outbound list **and** by the marking check, and the marking
check wins. An agreement that says we send reports does not override a report marked
`Internal`. Two independent gates, and the restrictive one always decides.

**Inbound is governed by the inbound list and by the gateway.** An item type not on the
inbound list is refused at the boundary before it reaches the gateway; anything on the list
still goes through validation and quarantine (DN-16).

**Format conversion happens at the edge.** `gungnir-interop` converts to and from the
partner's format at the interface, and everything inside the system stays canonical.
Conversion is lossy in both directions, and the loss is recorded on the provenance rather
than assumed away: a track that arrived as a STANAG message and lost its covariance
structure says so.

**No agreement, no exchange.** A peer that authenticates but has no agreement can do
nothing. Authentication answers who you are; the agreement answers what you may do.

**Partial delivery is reported.** When a partner is unreachable, outbound products queue
through the same store-and-forward path as everything else, and the panel shows the
backlog per partner. A dissemination that quietly failed is the failure mode MT-08 cares
about.

## 6. Configuration and interface delta

`ConfigBaseline.exchange: Vec<ExchangeAgreement>`, validated so that every party names a
configured peer or endpoint, every agreement's format is supported, and no party appears
twice.

Interface: no new endpoints. Exchange uses the existing stream, the report endpoints, and
DN-07's handoff path, all filtered by DN-17.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-09 System health | Per-partner exchange state: connected, backlog, last successful item |
| PN-13 Reports | Which partners a report may go to, before it is sent |
| PN-17 Commander summary | What was exchanged with whom in the period |
| PN-01 Status strip | A partner in backlog |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-7.4 Peer and coalition exchange | Two-node test with agreements configured, plus format round-trips | An item type absent from the inbound list is refused before the gateway; an outbound item whose marking forbids the party is withheld even when the agreement permits the type; a peer with no agreement can do nothing; conversion loss is recorded on the provenance; an unreachable partner queues and reports a backlog rather than dropping | Two-node harness; synthetic STANAG and ASTERIX corpora per D-09 |

The second criterion is the one that proves the two gates are independent, and it is the
one an implementation shortcut would break by checking only the agreement.

## 9. Amendment 1: the exchange endpoints §6 said would not exist (2026-09-06)

**§6 says "Interface: no new endpoints." Three were added, and this records the
divergence rather than leaving §6 to be read as still true.**

`ExchangeItem` has five variants. Two -- `Tracks` and `Health` -- had producers and gates.
`Warnings`, `Reports` and `Handoffs` appeared nowhere outside `gungnir-model`'s own unit
tests: the type said this system exchanges products, and no code could. §6's assumption
was that products would ride the snapshot, and they cannot: a snapshot is one picture at
one instant, and a warning ledger and a mission report are neither.

`GET /v2/exchange/{warnings,reports,handoffs}`, each through both gates that §4 requires:
the agreement decides whether the item may be sent at all, and the marking decides which
products within it, with `ExchangeSet::may_send` applying both so the restrictive one wins
in the model's own code rather than in the transport's.

Two shapes are worth the note:

* **`ExchangeResponse` has a `NotHeld { item, reason }` state**, not an empty list. "This
  deployment publishes no warnings" and "this deployment has no warnings outstanding" are
  opposite claims about a defended asset, and the same distinction `CoverageResponse`
  already draws for a sector.
* **A product's body is opaque JSON, and its identity, time and marking are typed.**
  `gungnir-workflow` and `gungnir-reporting` sit above `gungnir-api` in
  `../../ARCHITECTURE.md` §7.1's one-way direction, so naming `Warning` or a report type
  here would be a forbidden edge, and restating their shapes would be a second contract
  that drifts. Everything both gates turn on is typed; the payload is not.

**What is still missing, and it is the half that matters.** Nothing in the workspace calls
`publish_exchange` outside tests. The node holds none of the three items and cannot gain a
`gungnir-reporting` or `gungnir-workflow` edge without breaking §7.1; the desktop that does
hold them has no `NodeApi`, and `../../ARCHITECTURE.md` refuses a `gungnir-app` to
`gungnir-api` production edge. So the node answers `NotHeld` with a truthful reason and
**no product has ever been exchanged**. Closing that wants a decision -- a write path by
which a desktop posts marked products to its node, or a desktop-hosted transport -- and
neither was invented here.

## 10. Amendment 2: the write path, its store-and-forward, and one producer (2026-09-08, **written and gated where human-owned**)

**Amendment 1 ended by naming the decision this needed and not making it: "a write path by
which a desktop posts marked products to its node, or a desktop-hosted transport." The
write path is chosen and built.**

**The decision: a write path, not a desktop-hosted transport.** `POST` on the same three
paths `GET /v2/exchange/{warnings,reports,handoffs}` already serves, taking the same
`ExchangeProduct` list `NodeApi::publish_exchange` already accepted from tests alone. A
desktop-hosted transport would have meant a second server, a second copy of §5's two gates
to keep in step with the node's, and a second contract for `gungnir-interop` to convert at.
The write path adds neither: `publish_exchange` already existed, already replaced the held
set rather than appending, and already fed the same `exchange_for` the `GET` routes read.

**The action: `gungnir_security::actions::PUBLISH_EXCHANGE`, not `RELEASE_PRODUCT`.**
`RELEASE_PRODUCT` is raising or lowering a marking (DN-17); this is transmitting a product
that already carries whatever marking it has. Reusing `RELEASE_PRODUCT` would have made
PN-17's "what was exchanged with whom" and PN-20's "marking changes with the operator who
made them" indistinguishable in the audit log by action name alone. Granted to `Commander`
and `IntelligenceAnalyst`, mirroring `RELEASE_PRODUCT`'s own grant on the judgment that
whoever may mark a product releasable is who may send it -- a judgment, not a read of an
existing row: `docs/mission/roles-and-stakeholders.md` §4 gained a proposed "Publish to
coalition exchange" row for this rather than being widened silently, per the precedent
`ASSIGN_ROLE`'s own doc comment already set for authority.

**Amended the same day: `Supervisor` joins the other two.** `role_permits` had granted
`RELEASE_PRODUCT` to `Commander` and `IntelligenceAnalyst` alone since the initial commit,
missing `Supervisor` despite §4's original "Product release" row reading `yes` for it too
-- a pre-existing discrepancy this amendment's own PUBLISH_EXCHANGE grant unwittingly
mirrored, found and fixed the same day. Applying this section's own judgment ("whoever may
mark a product releasable is who may send it") to the corrected `RELEASE_PRODUCT` set
extends `PUBLISH_EXCHANGE`, and the §4 exchange row, to `Supervisor` as well; a
`role_permits` test now pins the two actions to always agree per role, for every role.

**The caller is this deployment's own desktop, not an outside party.** `task_sensor`'s
shape, not `effector_report`'s: `operator_caller` and `role_permits`, no machine-identity
path, because a desktop posting to its own node is not the same fact as a peer answering
something this deployment sent it. Unlike `task_sensor`, `publish_exchange` is a direct,
synchronous replacement with nothing for a node loop to issue, so the handler answers
without a queue or a reply channel.

**Store-and-forward: `gungnir-remote`'s existing pattern, not a new one.** `OutboundTask`
and `task_outbox` already carry a sensor command from `gungnir-app` to the link without
`gungnir-app` touching `gungnir-api` (the dependency is dev-only; `gungnir-app/Cargo.toml`
says why). `OutboundExchange` and `exchange_outbox` do the same for a batch of exchange
products, and `flush_exchange` converts the mirrored record into the wire type only inside
`gungnir-remote`, which already depends on `gungnir-api` for the contract. Ordinary
transport work, not the human-owned identity path `gungnir-remote/src/identity.rs` and
`LinkTls::client_config` are (`docs/agentic-workflow.md`).

**One producer, honestly scoped: handoffs.** `gungnir-app/src/handoffs.rs::issue_for`
republishes this desktop's whole current `state.handoffs` set to a linked node every time
one more is issued -- unfiltered by marking, because `NodeApi::exchange_for` already
applies DN-17 §5's marking gate at serve time per party, and filtering again on the desktop
would be the shortcut §8's own verification criterion exists to catch. `Warning` (DN-03)
and `MissionReport` (`gungnir-reporting`) are not wired: `Warning` carries no releasability
field at all -- DN-17 §3's marked-types list never named it -- and `MissionReport` has no
running collection on the desktop the way `state.handoffs` already is one. Wiring either is
its own change; this amendment does not claim it, in the spirit
`gungnir-model/src/exchange.rs`'s own doc comment already states: no producer faked to make
the path look busier than it is.

**What is still open.** GAP-009's own producer (when a deployment decides what raises a
launch warning) and a `MissionReport` collection on the desktop are prerequisites `Warning`
and `MissionReport` publishing would still need even once wired here; the lossy-format
round-trips (GAP-064) and the interop suite (GAP-063) are unaffected by this amendment and
remain GAP-065's own open items.

**Signature scope.** `gungnir-security` (the `PUBLISH_EXCHANGE` action and its role
grants, now three: `Commander`, `IntelligenceAnalyst`, and `Supervisor`) and the
`gungnir-api` write path are human-owned (`docs/agentic-workflow.md`). `gungnir-remote`'s outbox and
`gungnir-app`'s producer are ordinary transport and wiring work and carry no signature
requirement of their own.

## 11. Amendment 3: one register, a producer per writer (2026-09-23)

**Amendment 2 built the write path for one producer, and said so in its own heading.**
`publish_exchange` replaced the held set for an item, which is the right contract while a
single desktop writes and the wrong one the moment a second writer exists. Two now do.
DN-31 gave the node an approval queue (GAP-132), so the node issues handoffs of its own;
and a desktop that falls back keeps its link, decides on its own queue while cut off, and
publishes its whole set when it reconnects (GAP-133, GAP-134, DN-31 §6.7). Either writer
would have erased the other's set, and which one a partner saw would have depended on tick
order -- with nothing on the response saying the list was partial, which is the silence
DN-17 §5 rule 3 exists to prevent.

**The decision: one set per producer, merged on read** (D-68). The register is keyed by
item and then by producer. A publish replaces that producer's set and touches no other; a
read concatenates every producer's products, this node's own first, and sums what the
marking gate withheld. An item is `NotHeld` only when no producer holds any, and then it
carries what each of them said rather than one reason at random.

**The producer is the name the connection was verified under, never a field in the
request.** Since GAP-141 a desktop's certificate carries `desktop-` and sixteen hex digits
of its own key (D-67), so the name is a function of what the handshake proved. A request
field would have let any caller write as any producer, which is the same authority the
`origin` check on forwarded decisions already refuses. The node's own set is a variant of
its own rather than a string, so no desktop name can collide with it. A link with no
client certificate names nobody: every such writer shares one set and they overwrite each
other exactly as the whole register did before this amendment, which is what mutual TLS
buys and what its own doc comment says.

**Not the two alternatives.** *The node as the sole writer*, merging what desktops post
into one set, needs a rule for whose product wins on a collision and would have put that
rule in the transport. *Append per handoff*, with the product id as the key, cannot
express a withdrawal: a desktop that no longer holds a handoff has no way to say so, and
DN-18 §5's "partial delivery is reported" turns on a partner being told what is no longer
current as well as what is.

**Bounded, and a refusal rather than an eviction.** One item admits sixty-four producers.
A producer already in the register always writes; a new one beyond the bound is refused
with `507` and the reason, so the desktop's link keeps the batch queued and the backlog is
visible on PN-09 (§5, "partial delivery is reported"). Evicting somebody else's set to
make room would have served a partner a stale list and said nothing.

**What does not change: the wire.** §6's "no new endpoints" still holds, and so does the
response shape. A partner reads the merged set and learns nothing about how many desktops
this deployment runs or which one holds what -- our own topology, not a fact about the
products.

**What it corrects.** The node's opening claim for handoffs read "handoffs are issued on a
desktop from a recorded decision; this node holds none", which stopped being true the day
DN-31 moved the queue. It now publishes an empty set under its own producer -- "I keep
these and have issued none yet" -- and `NodeHost::republish_handoffs`, a documented no-op
since GAP-132, replaces that set each time the desk issues one.

**What is still open.** The register has no lifecycle (GAP-145): it lives in memory, so a
node restart loses every producer's set and no desktop republishes until it next issues a
handoff, and a producer that goes away is never forgotten -- which an ephemeral desktop
(D-67), being a new name every run, reaches faster than any other deployment.

**Signature scope.** The `gungnir-api` write path and `gungnir-node/src/approval.rs`
(D-65) are human-owned (`docs/agentic-workflow.md`); the ledger is `docs/signatures.md`.
`gungnir-remote`'s outbox and the desktop's producer are unchanged by this amendment.

## 12. Amendment 4: a register with a lifecycle, and an answer that says its age (2026-09-23)

**Amendment 3 gave the register a producer per writer and left it with no lifecycle**
(GAP-145). It lives in the node's memory and a desktop publishes only when it issues, so a
node that restarted held an empty register until some console's next decision: a partner
reading `GET /v3/exchange/handoffs` was served an empty deployment while the consoles held
engagements they believed were published, with nothing saying the list was short. In the
other direction nothing forgets a producer that has gone away, and an ephemeral desktop
(D-67) is a new producer every run.

**The decision: nothing expires, the answer carries its age, and a desktop refreshes when
its link comes back** (D-69).

**Nothing expires.** A handoff is a decision that was taken. Dropping it because the
console that issued it went quiet would delete a true thing to hide an unknown one, and
the partner would watch a list shrink for a reason no field on the response explains. The
three options the gap named -- a republish interval, a node that asks on reconnect, a set
that expires with its session -- differ in who notices the silence; only the last destroys
information, and it is the one not taken.

**The answer carries its age.** `as_of` on both `ExchangeResponse` variants is the node
time at which the least recently refreshed producer wrote. A merged answer is only as
current as its quietest contributor, and the producers stay off the wire (§11), so the age
is the one thing a partner can be told about them without learning how many consoles this
deployment runs. It is absent where there is nothing to date: an item nothing has been
published for has no age, only a reason.

**A desktop refreshes on reconnect, on the edge rather than the state.** A desktop
publishes its whole current handoff set the tick its link comes back, which is the only
moment it knows the node may have forgotten: a publish is a replacement, so repeating it
every tick would be a write a second for nothing, and waiting for the next decision is
what left the gap. A desktop holding none publishes none -- an empty set is the claim
"there are none here", which a console that has issued nothing has no business making, and
it keeps quiet consoles out of the register they would each take a producer slot in.

**What this deliberately does not do: evict.** §11's bound still refuses a new producer
beyond sixty-four rather than dropping a set a partner is being served from. A deployment
running ephemeral identities reaches that bound after sixty-four restarts, and what it
gets is a `507` and a backlog on PN-09 -- visible, and pointing at the key custody that is
the actual fault -- rather than a list that quietly lost a console.

## 13. Amendment 5: a refusal that says so, and an outbox with a bound (2026-09-25)

**Amendment 2 gave the desktop's link a store-and-forward outbox, and it could tell only
one answer from all the others** (GAP-146). A publish the node took left the outbox;
anything else -- no answer, a `507`, a `403` -- kept the batch and offered it again on the
next forward tick, and every new handoff added one more batch behind it. That is right for
a node that cannot be reached and wrong for a node that has answered "not you".
`PUBLISH_EXCHANGE` belongs to Commander, IntelligenceAnalyst and Supervisor (amendment 2),
so a console signed in as an Operator -- the ordinary case on a watch floor -- was refused
on every publish, retried four times a second for as long as it ran, grew its outbox by
one batch per handoff without bound, and said nothing anywhere. §11 and §12 wrote that a
`507` backlog "is visible on PN-09"; no line on PN-09 read the outbox until this amendment.

**The decision: read the answer for what it says about the caller** (D-75). Four answers,
not two:

| Answer | Statuses | What the link does |
|---|---|---|
| Delivered | `2xx` | Removes the set it sent. |
| Not now | no answer; `408`, `425`, `429`, `5xx` (including `507`) | Keeps the set and offers it again after an interval that doubles from one forward tick to thirty seconds, reset on delivery and on every new connection. |
| Not you | `401`, `403` | Keeps every set and **offers nothing more until the link signs in again**. |
| Not that | any other `4xx` | Drops that one set, counted, and goes on with the rest; the next set for the item replaces it anyway. |

**"Until the link signs in again" is a change that can matter, and a timer is not.** A
`403` is the node's authority matrix answering for the session the link holds; nothing
about the set would change it, and asking again in thirty seconds would ask the same
question of the same session. What can change it is a new session: every connection signs
in afresh, so the refusal is cleared the moment a snapshot lands and whatever is held is
offered once. A sign-in on a linked desktop builds a new link, whose connection is the
edge §12 already publishes on; during an outage the credential is replaced on the link
that exists (GAP-143) and takes effect at the reconnection. One request per sign-in, not
one per tick. A `401` is the same case with a different remedy -- the session lapsed, not
the role -- and PN-09 says which.

**Said once, on PN-09, in the node's words.** PN-09 has a "Coalition exchange" line for
any linked console: one sentence, never one per handoff or per attempt, whose counts grow
in place. For a refusal it gives the node's own reason, what is held (the newest set of
each item), and what would change it: "until this console's link signs in as a role that
may publish", with the roles read from `gungnir-security`'s matrix -- the table the node
checks the same action against -- rather than written out where they would drift. A wait
says how many attempts and the last failure; a rejected set says which item and why.
Nothing is pushed to the alert list: a refusal that stands for a whole watch is a state,
and a state belongs on the health panel, not in a stream an operator acknowledges.

**The bound: one set per item, the newest** (D-76). Every set is the producer's whole
current set, so a newer one makes any older one for the same item obsolete. The outbox
replaces a waiting set in place and counts the replacement; what it holds is exactly what
the node should end up with, however long the node does not take it. A set already on its
way is not recalled: it carries a generation, and the flush removes the generation it sent
so a set queued meanwhile survives to be sent after it.

**Not the detection outbox's rule, and not `StoreAndForwardQueue`.** The detection outbox
drops its oldest past 100,000 because every detection is its own fact, and losing the
oldest is the least harmful loss. An exchange set is not its own fact: it supersedes every
earlier set for its item. A first-in-first-out bound over sets would keep obsolete ones
while dropping by age across items -- the only mission report could go while ninety-nine
stale handoff sets stayed -- and on reconnection it would post every obsolete set in turn.
`gungnir_resilience::StoreAndForwardQueue` (GAP-121) is that first-in-first-out rule over
`Envelope`s; adopting it here would take the wrong rule and a manifest edge
`gungnir-remote` to `gungnir-resilience` that `../../ARCHITECTURE.md` §7.1 does not show,
for a queue that holds a different type. GAP-121 is left where it was.

**What else the reconnection edge publishes.** §12's edge published handoffs alone. It now
publishes this console's launch warnings with them, by the same rule -- the whole set,
and nothing from a console that has issued none -- because a sign-in that builds a new
link drops what the old link held, and a refused console's warnings would otherwise wait
for the next one issued. The mission report is PN-13's window state rather than mission
state and is not reachable from the tick; that is GAP-150.

**What does not change.** The wire: the node's refusal already carried its reason in the
problem body, so no `gungnir-api` write path was touched. The authority matrix: whether an
Operator should publish is amendment 2's judgement and is not reopened here.

## 14. Amendment 6: the mission report on the reconnection edge (2026-09-26)

**§13 left the report off the edge** (GAP-150). PN-13 kept it in window state, which the
tick does not reach, so after a node restart or a sign-in that built a new link a partner
was served nothing from that console until somebody generated a report again.

**The decision: the report partners were sent is mission state** (D-97). The desktop keeps
the record exchange carries -- identifier, marking, body, and the time the report was
generated -- beside its handoffs and launch warnings, and the edge publishes it by §12's
rule: the whole set, which for reports is one, and nothing from a console that has
generated none. **The record goes out unchanged**, so `at` is when the report was made and
never when it was last resent; a partner comparing ages is told the truth. PN-13's Generate
and Export send the same record.

**What it deliberately does not do: survive a desktop restart.** A restarted console shows
no report on PN-13, and republishing one it no longer shows would put a product before
partners that nobody at that console can see. §13's outbox rules are unchanged.

## Traceability

GAP-065, GAP-137, GAP-145, GAP-146, GAP-150; CAP-7.4; D-06, D-08, D-09, D-68, D-69, D-75, D-76, D-97; composes DN-07, DN-16,
DN-17, DN-19; depends on GAP-041 for the transport and GAP-064 for the codecs;
`../gungnir-api-v1.md`; `../architecture/uaf/standards/Sd-Tx.md`; principles AP-04, AP-09.
