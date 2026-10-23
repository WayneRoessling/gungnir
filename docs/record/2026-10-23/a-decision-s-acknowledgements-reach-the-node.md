# A decision's acknowledgements reach the node

GAP-193 and GAP-195, built together because the second rides on the first. GAP-107 made a
decision acting on a plan acknowledge the laydown's rehearsal standing. That acknowledgement
stopped at the desktop that asked it: the node's `/v3` decision route had nowhere to put
it, and a forwarded decision was rebuilt without it. GAP-195 found the older degraded
acknowledgement beside it on no record at all. D-132 (the owner's walk of 2026-10-23)
decided how GAP-195 is built.

## GAP-193: the body carries it, and the node records it

- **The wire.** `v3::DecisionRequest` and `v3::DecisionRecordView` gain `acknowledged`, a
  list of `gungnir_model::Acknowledgement`, defaulted and left out when empty. A body
  without it is the body it always was, so `SCHEMA_VERSION` stands, as it did for the
  other additive fields.
- **The route.** It refuses `400` an acknowledgement with an empty subject or statement,
  since such an acknowledgement records that somebody read nothing. Otherwise it hands the
  list to the loop on `PendingDecision`.
- **The node.** `answer_one` takes the decision through `ApprovalDesk::decide_acknowledging`
  in place of `decide_for`. So the node's `DecisionRecord`, its `CommandEvent::Decided` and
  the decision's audit entry hold the list, exactly as the desktop's own queue already did.
  A forwarded decision keeps it: `record_of` and `view_of` copy it, and the desktop's
  failover rebuild reads it off its own `Decided` events.
- **The node records what it is given.** Whether something had to be acknowledged is the
  console's question, because only the console knows what it drew. The node cannot tell
  what a desktop's PN-07 showed, so it does not second-guess the list.
- **The stopgap is gone.** `decisions::audit_linked_acknowledgement` wrote one entry per
  acknowledgement on the desktop's own log, saying the node's record did not carry it. That
  is no longer true, and the node's audit entry for the decision now quotes it.

## GAP-195: the degraded acknowledgement is recorded and enforced (D-132)

- **One question, two paths.** `decisions::acknowledgements(state, item)` gathers each
  degraded condition in force, then the rehearsal standing. The desktop's own `decide` asks
  it, and so does a decision posted through the node (`main.rs`), before anything is sent.
- **The set is computed at the moment of the decision.** It comes from
  `workspace::decision_conditions`, the same function PN-07 draws from: the conditions in
  force plus the item's own interim plan. It is compared by subsystem with the set the tick
  was given against (`DecisionDialogState::acknowledged_for`), as the dialog itself compares
  it. So a stale plan's growing age does not clear the tick, and a subsystem failing after
  the tick does. The detail recorded is the one in force when the decision was taken.
- **What is recorded.** One `Acknowledgement` per condition: subject `degraded <subsystem>`
  (for example `degraded journal`), statement the detail as drawn. It goes on the record, the
  event and the audit entry, ahead of any rehearsal acknowledgement.
- **Accept and override alike; a rejection asks nothing.** `override_enabled` in PN-07 gains
  the degraded gate `accept_enabled` already had, for D-119's reason: an override acts on a
  plan as an acceptance does. The line under a disabled control now names what to
  acknowledge first.
- **A refusal names something readable.** It names the first condition the tick does not
  cover, or, for a tick given against the same names in another order, the first condition
  in force.

## Things noticed

- **A pre-upgrade forward re-sent after the upgrade.** The node compares a re-forwarded
  decision with the one it holds, and now the acknowledgement is part of the record. A
  batch first forwarded by a desktop running the old build, then re-sent by the same
  desktop after upgrading, would arrive with its acknowledgements and be answered
  `Contradicts`. A forward is re-sent only when its first answer was lost, and a desktop and
  its node are upgraded together, so this was left as it is and stated here.
- **Most test desktops are degraded, and many tests decide on them.** A desktop under test
  usually has tracking not running, ingest short of an adapter, or a planner never asked,
  and until now only PN-07's button knew. The first full run after the change refused
  eleven decisions in seven test files (`approval_gate`, `audit_one_entry_per_act`,
  `audit_trail`, `cut_off_and_reconnected`, `endpoint_delivery`,
  `no_execution_without_decision`, `report_reaches_one_handoff`), every one with
  `Unacknowledged` naming a real condition. Those tests are about something else, so each
  now ticks the set in force before deciding, as a person at the dialog would. The rule was
  not relaxed for them. `rehearsal_advisory.rs` ticks them too, before asking about the
  rehearsal statement, and its assertions now expect the degraded acknowledgements first
  and the rehearsal last.
- **An item the queue does not hold asks nothing.** The same run showed `decide` asking for
  acknowledgements before learning the item was gone, so a decision on a missing item said
  "unacknowledged" instead of "not found". `decide` now asks only for an item its queue
  holds; the desk still answers that a missing one was decided or expired.
- **DN-26 §11 item 7** said the node's record did not yet carry the acknowledgement. It now
  says what holds.

## Human-owned parts

The `/v3` decision body and route (`gungnir-api/src/v3/mod.rs`, `gungnir-api/src/transport.rs`;
a `gungnir-api` write path) and `gungnir-node/src/approval.rs` (D-65). What the owner has
signed is in [`../../signatures.md`](../../signatures.md).

## Tests

- `gungnir-node/tests/approval_queue.rs`, `what_a_console_acknowledged_is_on_the_nodes_record`:
  over the real transport, a blank acknowledgement is refused `400` and records nothing; two
  acknowledgements reach the node's record, its `Decided` event and its audit entry.
- `gungnir-node/src/approval.rs`, `a_forwarded_decision_keeps_what_was_acknowledged`.
- `gungnir-app/tests/degraded_acknowledgement.rs`: accept and override refused without the
  tick and with a tick for another set; a rejection asks nothing; with the tick, accept and
  override each record every condition, on the record, the event and the audit entry.
- `gungnir-ui`, `a_degraded_condition_closes_override_until_acknowledged`.
