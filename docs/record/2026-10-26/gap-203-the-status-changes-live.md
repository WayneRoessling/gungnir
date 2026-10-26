# GAP-203: the status changes live

The first build of plan 12 (`../../plans/12-review-closure.md`, tranche A). The review of
2026-10-26 found that weapons control status, the one control an operator must have during
a raid, could only be changed by rewriting the baseline file and restarting the console;
US-07 had been blocked on it since the first usability round. This item records the build:
`docs/design/DN-09-authority-and-control-status.md` §10 is the design, written with it.

## The owner's four choices, 2026-10-26

Asked before anything was written, each with a recommended default, each taken to it:

| Question | Chosen |
|---|---|
| Does a change survive a restart, and how? | Folded from the journal over the baseline it was made under; a later baseline revision supersedes it; the file is never rewritten |
| Can a cut-off linked desktop change the status? | Yes, locally, since it runs its own queue; on reconnect the node's status is in force and the offline change is listed on PN-18, neither forwarded nor arbitrated |
| What happens to queued items a tightening refuses? | Withdrawn with the reason, as FL-08 drew it; a loosening resurrects nothing |
| How much of D-140 lands here? | The setting for both acts; enforcement for control status; the override half is its own gap |

## What was built, by layer

- **`gungnir-model`.** `ControlStatusChange`, `ConfirmingOperator`, `TwoPersonSettings`
  (`policy.two_person`, both off by default), `ControlStatusSettings::set`, the
  `PolicyEvent` family with `ControlStatusSet`, and `CommandEvent::Withdrawn`. A fifth event
  family rather than a command variant, because the readers that fold decisions
  (`gungnir-resilience`, `gungnir-reporting`) must not count a status change as one.
- **`gungnir-eventing`.** `Event::Policy`, additive under the schema rule `Event::Link`
  used.
- **`gungnir-command`** (human-owned). `OperatorDecision::Withdrawn` and
  `ApprovalWorkflow::withdraw`: a record that is neither actionable nor an expiry and names
  no operator, so "what became of item X" answers from the history as DN-31 §6.3 asks.
- **`gungnir-approval`.** `ApprovalDesk::set_control_status`, the one operation both
  binaries run: it publishes the change, audits it under `weapons.control_status`, re-judges
  every queued item by the control-status engine alone and withdraws what the new status
  refuses. The engine alone and not the chain, because a node re-judges with nobody at a
  console and the authority engine would refuse every item for that reason.
- **`gungnir-security`** (human-owned). `SessionAuthority::verify_credential`, the second
  person's check: the same store, the same constant-time comparison and the same back-off
  as a sign-in, establishing no session.
- **`gungnir-config`.** `policy.two_person` is an engagement-chain section, so a baseline
  that turns the rule off needs `weapons.control_status` to apply (GAP-162's rule).
- **`gungnir-api`** (human-owned write path), **`gungnir-node`**, **`gungnir-remote`**.
  `POST /v3/policy/control-status`, the node loop's handling with the same hand-over-and-wait
  shape as the acceptance route, the fold at start, the snapshot's `control_status` and
  `control_status_changes`, and the link's outbox, outcomes and projection.
- **`gungnir-app`.** `control_status::set`: linked and up, the request goes to the node;
  otherwise the change is applied, journaled, audited and the queue re-judged in one act.
  `ControlStatusLedger::recover` folds the journal at start. `sync_from_node` keeps the
  settings in force equal to the node's while the link is up. PN-18 lists the changes made
  while cut off. `gungnir-resilience`'s report carries them as
  `local_only_status_changes`, matched on the whole event and never arbitrated.
- **`gungnir-ui`.** The status dialog FL-08 (`control_status_dialog`), the strip's chip
  that opens it and says who set the layer, PN-17's list of the session's changes, and
  PN-06's withdrawn rows.

## What a reviewer should look at first

- The withdrawal path: `gungnir-approval/src/control_status.rs` and the test
  `a_tightening_withdraws_the_queued_items_it_refuses` in
  `gungnir-app/tests/control_status.rs`. The record it leaves is the one thing in this
  change that a later reader of MOE-01 could misread, and it is kept apart from both a
  decision and an expiry at every layer.
- The second person's check: `verify_credential` and the two-person test. It reuses the
  sign-in's verification rather than writing a second one, which is the whole of the
  reason DN-23 §5 gives for having one.
- The fold at start: `ControlStatusLedger::recover` and the restart test. The rule is
  "latest change under the running revision", and the test takes two changes so the fold
  cannot pass by taking the first.

## What this does not do

- Nothing forwards an offline change to the node, by the owner's choice; PN-18 says it.
- The override half of D-140 is declared in the setting and enforced nowhere yet.
- The verification row this adds (`../../verification-capability-table.md` §2, "Row
  drafted from DN-09 amendment 2") is a draft and not a gate until the owner agrees it.
- DN-09 is human-owned, and so are four of the crates touched; what the owner has signed
  of this change is in `../../signatures.md`.
