# A linked desktop shows the node's queue

GAP-133, the fourth of DN-31's five build increments
([`../../design/DN-31-node-approval-queue.md`](../../design/DN-31-node-approval-queue.md)
§6, clauses §6.5 and §6.6, §8, §9 rows 7 and 9, §10). The node holding the queue was
decided by the owner as D-55, what a decision records as D-53, and how an identifier is
shown as D-61
([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)).

**The defect was one line, and the fix is not a filter on it.** `update.rs` step 3b called
`decisions::submit` for every fresh plan, whichever service produced it. While linked,
`state.intercept` is `gungnir-remote`'s `RemoteInterceptService`, which hands the node's
plan back as fresh on every tick -- so the node's plan was re-proposed into the desktop's
own queue, decided there by whoever was at that console, engaged and handed off, invisibly
to the node and to every other desktop.

## Where the two paths part, and why there

At submission, and nowhere else. [`gungnir-app/src/projection.rs`]'s `in_force` is the
boundary and the tick asks it once, at the step where a fresh plan would have been queued:
while the node holds the queue the desktop submits nothing, and cut off it does exactly
what it did before.

**Nothing else had to be stopped, and that is the point.** An engagement opens only from a
`DecisionRecord` and a handoff is issued only beside one -- `ApprovalDesk::decide_for` is
the single caller of `open_for`, which is the single caller of `issue_for`, each guarded by
`DecisionRecord::is_actionable` (contract C-01, `no_execution_without_decision.rs`). A
desktop that queues no node plan therefore records no decision on one, opens no engagement
for one and issues no handoff for one. **DN-31 §6.5's one issuer follows from one queue.**
A filter inside `submit` would have been the opposite: three places to keep in step, each
able to drift, and each a place where the next change re-opens the gap.

The step moved into `plan_and_queue` rather than growing inside `tick`, so the sentence
"whose queue takes this plan" has a function to be the answer to.

## How the projection is built, and which source wins

The node's queue reaches a desktop two ways and they answer different questions, so
neither is a copy of the other.

- **The picture** (`GET /v3/queue`) is the node's whole queue in the node's order, with
  every field computed once on the node: the plan, the verdict, the deadlines, the roles
  it is offered to, D-15's delegation flag, the priority.
- **The stream** (`CommandEvent::Queued`, `Escalated`, `Decided`, `Expired`) says *when*
  the queue moved, with no polling, and is the only thing that carries **what became of an
  item** -- who decided it, as which role, at what time. A picture cannot carry that,
  because a decided item is no longer in it.

> **The picture is authoritative for membership and order. The stream is authoritative for
> an ending, always.**

They can disagree in one direction only. A picture is taken at a moment on the node and
arrives later, so it can still list an item that has since been decided; the stream's
ending is the newer fact and wins. The reverse cannot happen -- an item that has ended
never returns to a queue -- so `take_picture` replaces the waiting list wholesale and then
re-applies every ending already known, which is what stops a reconnection's fresh picture
resurrecting a decided item.

**The picture is taken after the subscribe frame, not from the snapshot.**
`SnapshotResponse` carries the same `queue` field and `run_link` fetches it *before* the
stream exists; an item queued in between would be in neither, and would sit unseen until
the next reconnection. A picture taken after the subscription cannot miss one. The two
return the same value -- `NodeApi::queue` reads the snapshot's own field -- so what was
chosen is *when*, not *what*.

A `Queued` or `Escalated` then marks the projection stale and the link takes a fresh
picture at once. That is not polling: nothing is asked for while the node's queue is still.
Building a row out of the stream alone would mean joining three events -- `PlanProposed`
for the plan, `PlanEvaluated` for the verdict, `Queued` for the deadlines -- and *still*
computing `pre_delegated` and `priority` locally, which are answers only the node's
authority matrix can give, and two desktops computing them would be two answers to one
question.

## What the panels say in each state

- **PN-01** names whose queue is in force, beside the backend element rather than folded
  into it. One says where the picture comes from and the other where a decision goes, and
  after a fallback they differ until PN-18's reconciliation has been seen (D-15).
- **PN-06** shows every item in the node's order and never re-sorts: "both desktops show
  the same queue in the same order" is only true if neither of them decides the order. An
  item this console may not decide names the roles it *is* offered to, so it goes to the
  right person rather than to a radio. `EmptyBecause::NotReceived` keeps "the node has not
  told me" apart from "nothing is waiting", which is the one situation where this panel
  knows that it does not know. What has ended on the node is drawn below the queue, named,
  so a decision taken at another console is visible at this one.
- **PN-07** decides through the route. A `409` shows who decided, as which role and when,
  and **draws no control**: the question has been asked. A `400`, `401` or `403` says the
  node would not take it and that nothing was recorded, here or there. While a post is in
  flight it says so, and after several tries it says that too, because a decision tried
  five times is not the same situation as one sent a moment ago.
- **PN-17** reports the node's queue for the period -- pending, decided, expired,
  escalated, and decisions by role -- and says whose queue those figures are about, since
  a commander reading "3 decided" has to know whether that is the watch or one console.

**With nobody signed in a linked desktop can decide nothing at all**, and PN-06 says so
once above the rows rather than greying out each one. That is stricter than the cut-off
desktop, which may decide under DN-23 §5 rule 5's role-selected fallback and records no
role for it (D-53): the node's route needs a token, so there is no key and no decision.

## The request key is derived, not minted

`gungnir-app/{operator}/{item}`. A retry after a `504` has to carry the same key, and a key
nothing has to remember cannot be forgotten by a process that restarted between the post
and the retry. Two consoles with different operators produce different keys for one item,
so each is its own request; two consoles with the *same* operator produce the same key on
purpose, because that is one person deciding one item and answering the second with the
first's outcome is the idempotency the key exists for.

## Three things the build turned up

- **The deciding console kept showing its own accepted item.** Row 7 caught it: the item
  left PN-06 only when the stream's `Decided` arrived, so for a tick or two after the node
  answered `201` the row was still there and still actionable -- and a second click would
  have earned a `409` naming that operator's own decision. A `201` is first-hand knowledge
  that an item is decided, so it now ends the item at once. It is **not** knowledge of the
  record: who decided, as which role and at what mission time are the node's to say and
  arrive on the stream, and filling them in from this desktop's own session would have been
  recording a claim about a record it had not read.
- **An escalation is not an ending, and counting it as one hides most of them.** Row 9
  first read escalations off what was still waiting and reported zero on a run that had
  eight: an item that escalates and then expires leaves nothing on the queue to find. They
  are read from the node's own `Escalated` events now. The same run showed that an item
  nobody takes climbs the whole ladder, one rank per `escalate_after_s` (DN-10 §5), so the
  first rung is what "visible to the Supervisor" is about.
- **PN-05's options would have gone blank on every linked desktop.** `refresh_support` was
  keyed on a submission, and a linked desktop submits nothing. The alternatives and the
  what-if judge plans nobody submitted and commit to nothing (GAP-032), so they are keyed
  on the plan changing instead -- which is what that function's own documentation already
  said it wanted.

## GAP-137, and the writer that is left

GAP-137 asked for the node's own handoff set to be wired into the exchange register once
GAP-133 had stopped a linked desktop issuing them. It has: `republish_handoffs` is reached
from `issue_for` alone, so a linked desktop that has taken no local decision never writes
the register.

**It is still not wired, because the second writer turned out to be somewhere else.**
`failover.rs`'s `fall_back` leaves `state.link` in place, so a desktop that is cut off
decides on its own queue, issues handoffs and queues its whole set on the link's exchange
outbox; `flush_exchange` delivers it on reconnect and `publish_exchange` replaces whatever
the node holds. Wiring the node's set now would make it the set that disappears the first
time any desktop recovers from an outage -- the same failure, found one layer along.
Giving the register a producer per writer is a `gungnir-api` write path and a DN-18
amendment, so GAP-137 stays open with that named, and the node's own no-op comment, which
said GAP-133 was what it was waiting for, is corrected.

## What this increment does not do

Forwarding offline decisions, the delegation lapse, `BothActed` and the reconciliation over
real decisions are GAP-134's, and `policy.delegation.disconnected_lapse_s` is not added.
PN-18 is untouched. A node's deadline is still drawn against the desktop's own clock, which
is correct only as far as two wall clocks agree and is filed as GAP-140.

## Where the facts are

`gungnir-app` holds `gungnir-node` as a **dev-dependency only**, so rows 7 and 9 can drive
the node's real loop rather than a copy of it; the dependency graph compares production
`[dependencies]` (`gungnir-app/tests/dependency_graph.rs`), as it does for the `gungnir-api`
dev edge beside it, so no edge is drawn. The routes are
[`../../gungnir-api-v1.md`](../../gungnir-api-v1.md); no payload or model type changed, so
[`../../design/model-and-schema-deltas.md`](../../design/model-and-schema-deltas.md) is
unchanged and `SCHEMA_VERSION` stays 4. DN-31 §9 rows 7 and 9 are in
[`../../verification-capability-table.md`](../../verification-capability-table.md) §2 with
the criteria the owner agreed on 2026-09-17, unchanged by this build, and are not gates yet
(D-16). What the owner has signed is [`../../signatures.md`](../../signatures.md).
