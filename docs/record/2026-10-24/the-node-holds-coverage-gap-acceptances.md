# The node holds coverage gap acceptances

GAP-194, the first of its two pull requests, built to D-131 (the owner's walk of
2026-10-23). GAP-106 gave a commander a way to accept a coverage gap (D-118), but the
acceptance lived only on the desktop that recorded it. A node's coverage answer and every
other desktop showed the gap as unexamined. D-131 gave the acceptances to the node, as it
holds the queue.

## The owner's choices (D-134)

D-131 left four things to the build, and the facts made each a real choice. The node takes
no baseline from a desktop, and it never read a laydown or a revision. A linked desktop
never read the node's coverage answer, and computes its own, terrain-masked where it loads
a DEM, while the node's is flat and computed from its own sensors. An acceptance names a
gap's exact shape, so the two answers rarely name the same gap. The owner took the
recommended default on each question, on 2026-10-24:

1. **Where the revision and laydown come from.** The node reads them from its own
   baseline, as the desktop does.
2. **Which coverage answer judges a gap.** The node's own answer, the one
   `GET /v3/coverage` serves.
3. **A forwarded acceptance the node refuses.** It re-opens on the desktop with the
   node's reason.
4. **How it lands.** In two pull requests, the node first.

## What was built

- **`gungnir-api`, human-owned.**
  - **The route.** `POST /v3/coverage/acceptances` authenticates the caller and checks
    `coverage.accept_gap`. It also vets the body: a reason, an approach, finite ends in
    order, and a positive spacing. Then it hands a `PendingAcceptance` to the loop and
    waits, exactly as the decision route does. Its own refusals are audited.
  - **The types.** `v3::GapAcceptanceRequest`, `GapAcceptanceRecorded` and
    `GapAcceptanceRefused` (`not-measured`, `gap-not-on-answer`, `already-accepted`).
  - **A forwarded acceptance.** `GapAcceptanceRequest::accepted_offline` names who
    accepted the gap at a cut-off console, as which role, when, and on which desktop. The
    node records the acceptance as that commander's, made when they made it, and the audit
    entry names the forwarder beside them, as a forwarded decision's does (D-55). It is in
    this pull request so the human-owned route changes once; the second pull request
    sends it.
  - **The coverage answer.** `v3::CoverageResponse::Computed` now holds a
    `ComputedCoverage`: the report, flattened where its fields always were, and
    `acceptances` beside it. The field is left out when nothing stands, so an answer with
    no acceptance is the body it always was.
- **`gungnir-node`.**
  - **The ledger.** `gap_acceptance::NodeGapAcceptances` is folded from the whole journal
    at start. It judges each request with the same `gungnir_analytics` functions the
    desktop's ledger uses, so the two cannot differ about what the same gap is.
  - **Re-opening.** The ledger re-opens what no longer holds and marks the coverage answer.
  - **The loop.** The loop computes its coverage answer before the journal append, runs
    the acceptance tick against it, then publishes the answer with its marks.
  - **The record.** An accepted gap is journaled as `PlanningEvent::GapAccepted` and
    audited under the operator and the machine. A re-opened one is journaled as
    `GapAcceptanceReopened` and audited under nobody. A refusal is audited under the
    caller.
  - **Retention.** It now protects the sessions holding standing acceptances, as the
    desktop's does.
- **Documents.**
  - DN-33 gains amendment 1.
  - `gungnir-api-v1.md` and `model-and-schema-deltas.md` gain the route, and the coverage
    row gains its marks.
  - D-134 records the owner's choices.

## What the second pull request builds

- **The link.** The desktop's link reads `GET /v3/coverage` with its marks, which it has
  never read before.
- **PN-11.** A linked desktop's PN-11 lists the node's gaps and posts acceptances to the
  route.
- **A cut-off desktop.** Its acceptances are forwarded when its node answers, and one the
  node refuses re-opens with the node's reason.

Until then a linked desktop accepts locally, as it did before. That is why GAP-194 stays
In progress.

## Checks

- **The ledger.** `gungnir-node`'s unit tests cover the following:
  - a gap on the node's answer accepted once and marked;
  - a gap from a different answer refused, whether terrain-masked or a sample off;
  - nothing accepted when nothing is computed;
  - each of D-118's three changes re-opening the acceptance with its reason.
- **The route.** `gungnir-remote/tests/transport.rs` drives it over the real transport:
  - a commander's acceptance reaching the loop under the token's name, with the loop's
    `201` and `409` coming back;
  - a supervisor refused `403`, no token refused `401`, and four bodies refused `400`;
  - every refusal audited, and nothing reaching the loop.
- **The wire.** The coverage answer's lossless-float test now carries an acceptance mark
  beside a NaN and an infinity. The flattened report and the new field both read back bit
  for bit.

## Ownership

The route and its transport types are a `gungnir-api` write path, which is human-owned
(`docs/agentic-workflow.md`). `docs/signatures.md` records their signature.
`gungnir-node/src/gap_acceptance.rs` and the loop's wiring are not on the human-owned list.
