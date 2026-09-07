# DN-10 Queue expiry and escalation

Closes GAP-034 and GAP-035. Status: **signed off by the owner 2026-09-05**, implemented
the same day, and **amendment 1 (§9) signed by the owner 2026-09-05**.
**Human-owned and signed**: `gungnir-command` is a low-trust crate and this note changes
what happens to a decision nobody takes. The owner signed it on 2026-09-05. The rule that
no configuration can make an expiry accept is now settled, not proposed.

## 1. The gap and the thread step it blocks

`InMemoryApprovalWorkflow` holds a pending list and waits. A pending decision has no
timeout, no escalation, and no recorded expiry. Under MT-01 saturation a plan quietly stops
being useful while it still sits in the queue looking actionable, or an operator holds one
past its window with nobody aware.

## 2. The owning component

`gungnir-command`, which owns `ApprovalWorkflow`, `DecisionRecord`, and
`OperatorDecision`. It depends on `gungnir-model` and `gungnir-policy` and needs nothing
else. **No new edge.**

## 3. Types

In `gungnir-command`:

> **Amended 2026-09-05 (amendment 1, §9).** `Overridden` was missing from this list
> and exists in code; `Escalated` is not a variant of this type, for the reason given in
> §9. What is implemented is:

```rust
/// What ended a pending approval. `Accepted`, `Overridden` and `Rejected` are a
/// person's choice; `Expired` is the absence of one.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum OperatorDecision {
    Accepted,
    /// The operator substituted their own assignment; the plan stored in the
    /// record is the one they acted on.
    Overridden,
    Rejected { reason: String },
    /// The window closed with nobody deciding. Not a rejection: nobody chose.
    Expired { at: MissionTime },
}

/// The queue's own view of a pending item, which is what orders it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PendingApproval {
    pub id: PendingApprovalId,
    pub plan: PlanView,
    pub verdict: PolicyVerdict,
    pub submitted: MissionTime,
    /// From the policy settings for the plan's layer; `None` means no expiry
    /// configured, which is shown rather than guessed.
    pub expires_at: Option<MissionTime>,
    pub escalate_at: Option<MissionTime>,
    pub escalated_from: Option<String>,
    /// Every role this item is currently offered to, in escalation order. Added by
    /// amendment 1: §5 says escalation does not remove the original role, and one
    /// field cannot say that.
    pub offered_to: Vec<String>,
    /// Highest risk score among the plan's tracks, for ordering.
    pub priority: f32,
}
```

`DecisionRecord.decision` already carries an `OperatorDecision`, so an expiry becomes a
record like any other, with a mission time and, where a person acted, an operator.
`DecisionRecord.operator_id` stays `Option<String>` and is `None` for an expiry, which is
correct: nobody decided, and recording a false operator would be worse than a null.

**That last sentence is not what makes an expiry recognisable, and amendment 1 exists
partly because the implementation read it as though it were.** `operator_id` is `None`
for an expiry *and* for every decision taken before there is an operator session
(GAP-057), so absence of an operator identifies nothing on its own. The `Expired`
variant is what makes the distinction, which is why it is in this note's type list.

## 4. Edges

**None.**

## 5. Behaviour

**Ordering.** The queue is ordered by time remaining ascending, then by priority
descending. Time pressure outranks severity, because a high-priority item with two minutes
left can wait behind a lower-priority one with ten seconds left, and the reverse loses
both.

Items with no expiry sort after every item that has one, and the panel says why.

**Expiry.** When mission time passes `expires_at`, the item leaves the queue with an
`Expired` decision recorded and a `CommandEvent` emitted. It is **not** silently dropped
and it is **not** auto-rejected: an expiry and a rejection mean different things to an
after-action review, and conflating them would corrupt MOE-01.

If no expiry is configured for the layer, the item never expires. That is DN-08's defaults
table, and the asymmetry is deliberate: silence about authority denies, silence about
expiry preserves.

**Escalation.** When mission time passes `escalate_at`, the item is offered to the role
above, recorded as `Escalated`, and re-enters the queue tagged with where it came from. It
does **not** leave the original role's view: an operator who is about to decide should not
have the item vanish. Both roles see it; whoever decides first ends it.

Escalation is bounded. An item escalates at most once per rank step and stops at the
highest role with authority for that action. It cannot loop.

**Pre-delegation** (D-15) is checked at submission, not at expiry. A pre-delegated case
enters the queue already actionable for the operator; expiry and escalation still apply.

**What this note refuses to add:** an automatic accept on expiry, under any configuration.
There is no setting for it and no code path to it. An expiry that accepts is an action
without a human decision, which is contract C-01, and C-01 is not dispensable.

## 6. Configuration and interface delta

Everything comes from DN-08's `DecisionSettings`: `expiry_s` and `escalate_after_s` per
layer, with validation that escalation is earlier than expiry.

Interface:

- `CommandEvent` gains `Expired { plan: PlanId, at: MissionTime }` and
  `Escalated { plan: PlanId, to_role: String, at: MissionTime }`. Additive variants;
  clients ignore unknown ones.
- `POST /v2/plans/{plan_id}/decision` returns a conflict error when the plan has already
  expired, rather than accepting a decision on a dead item.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-06 Approval queue | Time remaining per item as the primary sort, escalation marks, and items with no expiry grouped at the end with the reason |
| PN-07 Decision dialog | Time remaining, counting down; the accept control disables on expiry with the reason rather than failing on submit |
| PN-08 Alerts | An expiry raises an alert. A decision nobody took is exactly the thing an operator must find out about |
| PN-17 Commander summary | Expiries and escalations in the period, which is the honest measure of whether the queue is keeping up |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-3.7 Queue under saturation | Property test over generated queues plus an MT-01 saturation replay | Ordering is by time remaining then priority; an expired item leaves with an `Expired` record distinct from a rejection and with no operator; no configuration causes an expiry to accept; escalation adds the higher role without removing the original; an item escalates at most once per rank step | TT-01 and TT-02 saturation sets |

The first clause of the third criterion is tested by an exhaustive search over the
settings space for any path that produces `Accepted` without an operator. That test exists
because this is the note where such a path would most plausibly be added later.

## 9. Amendment 1 — **signed by the owner 2026-09-05**

Raised when GAP-034 and GAP-035 wired this note's module into
`InMemoryApprovalWorkflow`. Everything here is a correction of the note against what
implementing it showed, not a change of intent. The same sign-off covers the code that
conforms to it: `OperatorDecision` in `gungnir-command`, and the expiry rule in
`gungnir-collab`'s `RoleRankArbiter`.

**a. `Escalated` is not an `OperatorDecision`.** §3 listed it as one. An escalated item
has not ended: §5 of this note says it "re-enters the queue" and that both roles see it.
A `DecisionRecord` for an escalation would put an entry in the append-only history for
something that has not happened, and `records()` would stop meaning "items that ended".
Escalation is `CommandEvent::Escalated` on the bus, which §6 already specified, and that
is its only home. `QueueOutcome` in `queue.rs` carries it in the sweep's return value.

**b. `Overridden` was missing from §3's list.** It existed in `OperatorDecision` when
this note was written. Recorded so the note is not read as proposing its removal.

**c. `PendingApproval` gains `offered_to: Vec<String>`.** §5 says escalation "does not
leave the original role's view -- both roles see it; whoever decides first ends it". A
single `escalated_from` cannot express that. The original field stays, naming the most
recent step.

**d. The escalation bound is the clock, not `escalated_from`.** §5 says an item escalates
"at most once per rank step". The implementation required `escalated_from` to be unset,
which bounds it at once *ever*. Each step now resets `escalate_at`, and reaching the top
of the ladder clears it, so it cannot loop and cannot escalate twice within a step.

**e. The governing layer of a multi-layer plan is the one that closes first.** §6 sets
expiry per layer and a plan may task several; the note did not say which decides. Taking
the longest would let a 30 s window close while the item sat in the queue looking live,
so the earliest configured expiry governs. A layer with no configured expiry never closes
and therefore loses to any layer that has one.

**Why this was found late.** Every function in §3 and §5 was implemented, fully tested,
and called by nothing. A module in that state is self-consistent rather than verified:
its tests agree with it because they were written from it. The defects in (c) and (d)
were found by reading this note against the code while connecting them, not by running
the tests, which passed throughout.

## Traceability

GAP-034; CAP-3.6, CAP-3.7; D-15; MT-01 saturation, MT-02;
`../ux/wireframes/WF-06-approval-queue.puml`, `WF-07-decision-dialog.puml`; contracts
C-01, C-04; depends on DN-08.
