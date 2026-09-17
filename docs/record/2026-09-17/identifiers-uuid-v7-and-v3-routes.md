# Identifiers become UUID v7, and the interface moves to /v3

GAP-130, the first of DN-31's five build increments
([`../../design/DN-31-node-approval-queue.md`](../../design/DN-31-node-approval-queue.md)
§5.1, §9 row 1 and amendment 1). Decided by the owner as D-56, D-60 and D-61
([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)).

## What changed

- **`DecisionId`, `PlanId` and `PendingApprovalId` hold a `u128` minted as a UUID v7** where
  each thing is created (D-56). They were `u64` counters that started at 1 in every process.
- **They are written as the hyphenated RFC 9562 string and read from that string or a
  number** (D-60). `gungnir_model::identifier` holds the serde helper, the parse, the full
  form and the short tag once. `PendingApprovalId` in `gungnir-command` uses the same
  helper.
- **Panels and alerts show a short tag, `…9f3a61c2`**, the last eight hex digits (D-61).
  PN-07 shows the plan under decision in full with a copy control, and so does PN-20's
  handoff record. Audit entries, journals, reports and log fields carry the whole
  identifier. A value that fits in a `u64` is shown as the number it is. That covers a
  rehearsal seed's plan (`P-1183`) and anything written before this change. No minted
  identifier fits in a `u64`, because a UUID's version nibble lies in its upper half.
- **`SCHEMA_VERSION` is 4** and `CommandEvent::ApprovalRequested`, which nothing
  published, is removed (DN-31 §5.3).
- **The interface is `/v3`.** `gungnir_api::API_VERSION`, `path` and `routes` build both
  the node's router and `gungnir-remote`'s URLs. The `v2` module is `v3`.
- **Every `/v2` route is still routed.** Each one authenticates its caller the way its `/v3`
  successor does, so a caller the successor would refuse is refused with the same status
  and words. Only then does it answer `410 Gone` with a problem naming the successor path,
  in the message and in a `successor` field. `/v2/events` answers before any upgrade,
  because its token would travel in a frame it never reads. The retired table in
  `gungnir-api/src/transport.rs` is frozen: a route added under `/v3` later is not found
  under `/v2`. `gungnir-api/tests/retired_v2.rs` checks every route against an operator,
  an unauthenticated caller, a party with an agreement, an effector certificate and a
  party with neither.
- **The report route reads a decision in either written form.** An effector written before
  the change still sends a decimal number and gets an answer about its identifier. Any
  other text is a readable `400` problem naming both forms, not axum's plain-text
  rejection.

## Why strings, when §5.1 kept a number

D-56 as first written kept the JSON form a number, reasoning that "a u64 number is a valid
u128". A probe of `serde_json` 1.0.151 with a v7-shaped value found that the number
round-trips through `to_string` and `from_str`, and breaks in three other places:

- `serde_json::to_value` refuses it with "number out of range".
- Text parsed into a `serde_json::Value` turns it into `2.125479544897801e+36`.
- A field-tagged enum refuses it with "u128 is not supported", because serde buffers such
  an enum's fields and the buffer holds nothing wider than 64 bits.

Two live paths go through a `Value`. `gungnir-app`'s `handoffs.rs` built both the handoff
posted to an effector and the handoff body published for exchange with
`to_value(..).unwrap_or(Value::Null)`, so every minted decision would have sent `null`. The
node holds an exchange body as a `Value` (DN-18 §5 amendment 2). The delivery test's stub
endpoint read each request and ignored the body, so nothing would have failed. The stub
now keeps the bodies, and `endpoint_delivery.rs` checks that the effector receives the
decision in full. `handoffs.rs` checks the same of the exchange body. D-60 records why
strings were chosen over `serde_json`'s `arbitrary_precision` and over raw JSON bodies.

## Every minting site

- `InMemoryApprovalWorkflow::submit_for_approval`: a queue item. It was the `next_id`
  counter.
- `InMemoryApprovalWorkflow::decide`: a decision.
- `InMemoryApprovalWorkflow::sweep`: the decision an expiry records. It shared
  `next_decision` with `decide`, through `mint_decision_id`.
- `DpInterceptService::fresh_plan`: a plan. It was the `next_plan_id` counter, and a
  planner is built in more places than it looks:
  - `gungnir-app`'s `build_backends` at start, and `failover::fall_back`;
  - `gungnir-node` at start;
  - `gungnir-app/src/decisions.rs`'s `SnapshotPlanner`, once per alternative and what-if.

Values that are not minted:

- A rehearsal seed's plan keeps the seed's number (`rehearsal.rs`).
- `PlanId::default()`, zero, is only ever a sentinel. It is the id of `PlanView::default()`.
  It seeds `last_live_plan_id` in `gungnir-app/src/state.rs` and
  `DpInterceptService::last_plan`, and it is the plan of `gungnir-decision`'s declined
  course. No planner mints it, which `gungnir-intercept-service/tests/identifiers.rs`
  checks.
- `gungnir_ui::panels::approval_queue::PendingId` mirrors `PendingApprovalId` at 128 bits.

## Order and density assumptions

No code sorts by these identifiers, takes the latest by the highest one, uses a range of
them or indexes an array with one. `queue::order_queue` orders by time remaining, then
priority, and its sort is stable. What did rest on the counters is identity, where two
processes' counters could meet, and one piece of density:

- **The tick announces a plan only when its id differs from the last one it announced**
  (`last_live_plan_id`, GAP-097). The node's planner and a fallen-back desktop's embedded
  planner both numbered their first plan 1. The collision was at the switch back: the
  node's plan 1 was taken for the embedded plan 1 just announced, never proposed, and the
  plan in force stayed the embedded one. The fall back itself was masked, because its
  first tick has no tracks and announces the empty plan, id zero, which resets the
  comparison. This was first reported as a fall-back collision, which it is not.
  `gungnir-app/tests/failover.rs` now walks the whole outage with real planners.
- **An effector report finds its handoff by decision alone** (`accept_report`). Two desktops'
  first decisions were both decision 1, so the report reached both.
  `gungnir-app/tests/report_reaches_one_handoff.rs` sends a report through the node's route
  and queue to two desktops' link inboxes, and only the issuing desktop applies it.
- **MOE-05 joins decisions to engagements by decision alone.** Two machines' merged journals
  paired both engagements with whichever `Decided` came last.
  `moe_05_pairs_each_machines_engagement_with_its_own_decision` in
  `gungnir-reporting/src/measures.rs` is the test.
- **`gungnir_resilience::reconcile` pairs two journals' endings by plan.** No code change
  was needed: with unique identifiers only one plan pairs with itself.
- **The density artefact.** `rationale_for` wrote "Plan #N" for plans solved on a planner
  built per call, which always said "Plan #1". That number matched no queue row or record,
  and it made two computations of one course byte-identical. With minted identifiers it
  named a fresh phantom each time, and `what_if_leaves_the_live_desktop_exactly_as_it_was`
  failed. The rationale now names no plan identifier.

`gungnir-command/tests/identifiers.rs` and `gungnir-intercept-service/tests/identifiers.rs`
use separate workflows and planners to stand in for a node, two desktops and a restart.
No two identifiers are equal, and within a process they sort in minting order.

## How a journal written before this reads

A number reads as the same identifier, and the identifier is written back as the hyphenated
form. `testdata/journals/pre-uuid-v7/` holds a desktop session written at `7d6f201`: plans,
a decision, an engagement opened and closed, a handoff issued and reported, an escalation
and an expiry, all on the old counters. It also holds the report exported from that
session. `gungnir-app/tests/pre_uuid_v7_journal.rs` replays the journal through
`gungnir-replay`. It checks that every identifier reads as the number written, and that the
regenerated report equals the committed one as a value and byte for byte. It also checks
that every line rewrites in the new form and reads back unchanged. The fixture's
`SOURCE.md` says which of its lines stands in for the node's record.

## Found on the way

An effector's report of executing or of completion moves the desktop's engagement and
publishes no `EngagementEvent`, so no real journal can hold a corroborated close. The
fixture had to close its engagement on track-lifecycle evidence for that reason. Filed as
GAP-135.
