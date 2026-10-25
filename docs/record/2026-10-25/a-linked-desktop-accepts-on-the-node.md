# A linked desktop accepts on the node

GAP-194's second pull request, the desktop half of D-131 and D-134. The first, merged on
2026-10-24 as #209, gave the node the acceptances. A linked desktop still accepted on its
own ledger, read nothing of what the node held, and had no way to forward what it accepted
while cut off. This closes GAP-194.

## What was built

- **`gungnir-remote::coverage`.**
  - **`NodeCoverage`.** The node's coverage answer as last read, and when it is due a read.
  - **The outbox types.** `OutboundAcceptance`, `AcceptanceAnswer` and
    `AcceptanceOutcome` are the acceptance outbox and the node's answers.
  - **Re-exports.** The `v3` types the desktop needs are re-exported here.
    `gungnir-remote` is the only crate the desktop speaks to a node through, and
    `gungnir-app`'s `gungnir-api` dependency is dev-only. So no edge is added.
- **The link.**
  - **When it reads.** It reads `GET /v3/coverage` right after subscribing, as it reads
    the queue. It reads again on any planning event the stream carries, after its own post
    is answered, and otherwise every 5 seconds (`COVERAGE_REFRESH`). The node's coverage
    moves with its sensors, and no event says so.
  - **Posting.** It posts acceptances to `POST /v3/coverage/acceptances`, oldest first.
    It keeps one through `503`, `504`, an unreachable node and a lapsed token, as it keeps
    a decision.
  - **When the link goes down.** It forgets the answer: one from a node it cannot hear is
    not one to draw acceptances from. The outbox stays.
- **The desktop.**
  - **Linked, with the link up.** `gap_acceptance::accept` sends to the node and records
    nothing locally (`Accepted::SentToNode`). PN-11 lists the node's gaps, the node's
    acceptances and the measure of them (`node_gaps`).
  - **Linked, with the link down.** It records the acceptance locally, as an unlinked
    desktop does, and queues it to forward. The forward names the commander, their role,
    when they accepted it, and this desktop by its machine name (`accepted_offline`).
  - **The node's answers.** `answers_from_node` delivers them each tick as alerts. A
    forwarded acceptance the node holds stands locally too. One it refuses re-opens
    locally as `ReopenedBecause::RefusedByNode`, carrying the node's words, journaled and
    audited under nobody, so no console shows an acceptance the node does not hold
    (D-134).
- **The model.** `ReopenedBecause::RefusedByNode { reason }` is new. It is an additive
  variant on the journal, so `SCHEMA_VERSION` stands, as it did when `Event::Planning`
  was added.

## A limit, said

The acceptance outbox is held in memory. A desktop that restarts while still cut off
forgets the forward. Its own acceptance stands on its own journal, but the node never hears
of it until a commander accepts the gap again while linked. An outage's decisions do not
have this limit: GAP-142 recovers them from the journal so they are forwarded after a
restart. Doing the same for acceptances is filed as GAP-202.

## Checks

- **End to end.** `gungnir-app/tests/linked_gap_acceptance.rs` runs a real `AppState`,
  signed in as the commander, against the real transport and `gungnir_node`'s own ledger:
  - **Linked.** The node's gap is listed. Accepting sends it and records nothing locally.
    The node's mark comes back naming the commander. A gap the node cannot see is refused,
    and the refusal is said.
  - **Cut off, on round 1's baseline.** The desktop records its acceptance and queues it.
    When the node answers with the same coverage, it holds the forwarded acceptance as the
    commander's. When it answers with coverage that has no such gap, the desktop's
    acceptance re-opens with the node's reason, journaled and audited.
- **The link.** `gungnir-remote::coverage`'s unit test covers when an answer is due.
- **The existing tests.** `gap_acceptance.rs`'s tests run unlinked as they did, reading
  the recorded identifier through `Accepted::recorded`.
