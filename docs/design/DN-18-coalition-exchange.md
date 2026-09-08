# DN-18 Peer and coalition exchange

Closes GAP-065. Status: first draft, 2026-09-05; amendment 1 (2026-09-06, signed
2026-09-07) built the three read routes; amendment 2 (2026-09-08, written and gated where
human-owned, signed by the owner the same day) built the write path, its store-and-forward,
and the handoffs producer. **Not fully closed**: no producer exists yet for `Warnings` or
`Reports`, and the format round-trips and interop suite (GAP-063, GAP-064) are untouched.

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

## 9. Amendment 1: the exchange endpoints §6 said would not exist (2026-09-06, **signed by the owner 2026-09-07**)

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

## 10. Amendment 2: the write path, its store-and-forward, and one producer (2026-09-08, **written and gated where human-owned, signed by the owner the same day**)

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

**Signature scope.** `gungnir-security` (the `PUBLISH_EXCHANGE` action and its two role
grants) and the `gungnir-api` write path are human-owned (`docs/agentic-workflow.md`); both
are signed by the owner the same day this amendment was written. `gungnir-remote`'s outbox
and `gungnir-app`'s producer are ordinary transport and wiring work and carry no signature
requirement of their own.

## Traceability

GAP-065; CAP-7.4; D-06, D-08, D-09; composes DN-07, DN-16, DN-17, DN-19; depends on
GAP-041 for the transport and GAP-064 for the codecs; `../gungnir-api-v1.md`;
`../architecture/uaf/standards/Sd-Tx.md`; principles AP-04, AP-09.
