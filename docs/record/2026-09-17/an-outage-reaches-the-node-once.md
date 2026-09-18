# An outage reaches the node once

GAP-134, the last of DN-31's five build increments
([`../../design/DN-31-node-approval-queue.md`](../../design/DN-31-node-approval-queue.md)
§5.2, §5.3, §6.7, §6.8, §7, §8, §9 row 8). The delegation policy is the owner's D-15, the
reconciliation rule D-53, the node holding the queue D-55 and the comparison by track D-58
([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)).

**One story, four parts.** A desktop cut off from its node decides, and the node has to end
up holding what it decided: its delegations lapse while it is cut off, its engagements are
compared with the node's by track when it reconnects, and its decisions -- with whatever the
reconciliation settled about them -- are forwarded to the node once.

## The order on reconnect, and why it is not §6.8's

Four things happen around the moment the node answers again. As built:

1. **The merge**, when the node's history for the outage arrives: the plan conflicts and, in
   the same pass over the same two journals, the engagements by track. Two engagements of one
   track are published as `LinkEvent::BothActed` and alerted **before** D-03's rule is asked
   about any plan, then the rule settles every conflict it can rank.
2. **The forwarding**, the moment nothing in the merge waits for a person -- when the rule
   settled everything, or when the last person resolves a conflict: every decision taken
   while cut off, in the order taken, **in one batch**, each carrying its settlement if its
   plan was in conflict. Where the node's half could not be fetched there is nothing to
   settle and the batch goes at once.
3. **The switch back**, a person's act on PN-18 as before (D-15), refused while a person's
   conflict is open or while the node has refused the batch.

**DN-31 §6.8 narrates this the other way round**: the desktop forwards, and "reconciliation
then runs", and "the rule's verdicts and the person's resolutions are forwarded too". That
is two passes, and it can leave the node holding the decisions without what stands between
them -- a desktop that dies between the passes leaves two contradictory decisions on the
node's record and nothing saying which one stands, which is the half an outage the brief
asked to make impossible. It has a second hazard the build found: forwarding first puts this
desktop's own decisions into the node's history before the merge reads it, and an envelope
the node journals at the restore moment -- equal mission times, which the row-8 harness has,
or any skew between the two clocks (GAP-140) -- falls inside the outage window and would be
merged a second time as the node's. Merging first reads a history fetched before anything
was sent.

**Nothing is lost by waiting.** The two queues hold different items -- a cut-off desktop
decides only its own planner's plans, and the node's items stay on the node for other
desktops (§6.7) -- so no desktop is kept from deciding by the delay. The cost is that an
outage's decisions reach the node one history round-trip later, and where a conflict waits
for a person, not until that person acts. This is a departure from §6.8's order, recorded
here rather than written into the note.

## Whole or not at all, and exactly once

`POST /v3/decisions/forwarded` is handed to the node loop like the decision route. The loop
checks **every** record in the batch against what the node holds under its `DecisionId`, and
every settlement against what it holds for its plan, **before writing any of it**. So a
desktop that dies mid-reconnect leaves the node with all of its outage or none of it. A
record the node already holds identically is counted and records nothing, which is what
makes a batch safe to send again after a `504`; the link does exactly that, under the same
identifiers, until the node answers.

`ApprovalWorkflow::admit_forwarded` appends the record and **does nothing else**: no queue
item, no engagement, no handoff. The desktop did those while cut off, and doing them again on
the node would be the double engagement D-58 exists to report. The record keeps the mission
time the desktop recorded; the envelope the node publishes carries when the node learned of
it -- the treatment an effector report's own `at` already gets.

The route's answers: `202 ForwardAccepted { recorded, already_held, settled }`; `400` for a
body that does not decode, a record naming no origin, a verdict no queue could have produced,
or a rejection with no reason; `401`; `403` for a role without `plan.decide`; `409
ForwardRefused` as `Contradicts { decision, held }` -- the same identifier, a different
record, and the node's stands -- or `ContradictsAnExpiry { decision, plan, at }` where what
the node holds is a window that closed (a record view carries a person's choice and an expiry
is not one, so naming it as a rejection would put a refusal nobody made in the answer), or
`SettledOtherwise { plan, held }`; and `504`, which does not mean nothing was recorded.
Every refusal writes one audit entry on the node, as the decision route's do.

**`ForwardedDecision` has a third field, `settled`, beyond §5.2's two.** §6.8 asks for the
verdicts and resolutions to be forwarded so the node's record says what stands (MT-10 step
5), and §7 gives exactly one route to forward on. A decision in conflict therefore carries its
settlement in the same element, defaulted, and the node journals it under the event the
desktop journaled it as -- `ConflictArbitrated` or `ConflictResolved` -- once per plan.

## Where the lapse lives, and what it changes

**In the authority matrix, not in a panel.** A pre-delegated rule is a granting rule: it is
what gives an Operator authority over D-15's case. So `gungnir_policy::authority_in_force`
returns the matrix without its delegated rules once a desktop's delegations have lapsed, and
`ApprovalContext` carries `Delegations` so the chain's authority engine, the offering, and
the delegation flag all read that one matrix. Plans proposed after the lapse that only a
delegation could take are denied by authority and never queued.

**Items already queued are re-offered, once, at the tick the lapse falls due.** A lapse is
not an escalation -- it removes a role because the authority went away -- so
`ApprovalWorkflow::reoffer` withdraws the offer from each role that no longer holds the item
and, where none is left, offers it to the lowest rung that does. An item no rung holds is
offered to nobody, stops escalating because there is nowhere to go, and still expires; nothing
accepts it. `LinkEvent::DelegationsLapsed` names every plan withdrawn. **It is not in §5.3's
list**: it records a change in what queued items are actionable by, and the record has to say
which.

`policy.delegation.disconnected_lapse_s` is new, validated finite and positive where stated,
and has **no default**: silence means no delegation survives the disconnection, which is the
strictest reading and not an interval this build chose. The clock runs from the moment the
node went silent and a node that answers again does not stop it, because the desktop decides
on its own queue until a person switches back. **No new delegation is made while cut off** by
construction: a baseline applied while the desktop runs is in force on restart, and the
fallback does not survive a restart. A node never lapses anything; a desktop deployed on its
own never falls back.

## BothActed, and what PN-18 shows

`gungnir_resilience::reconcile` makes a second pass over the same two journals for
`EngagementEvent::Opened` and pairs by track, independently of the plan conflicts: a plan
conflict with no shared track raises no incident, and a shared track is raised beside a plan
conflict rather than instead of it. **PN-18 draws the incidents above everything else** -- the
outbox line, the outage's bounds, the merge and the verdicts all come after -- each in the
alert's own sentence, with both engagements' plans and times and **no button**: nothing about
it is resolved by keeping a record. PN-18 also says where D-15's delegations stand and where
the outage's forwarding stands.

## What the build found

- **Pairing by plan finds no real conflict across an outage any more.** Since GAP-130 a plan
  is minted where it is proposed and since GAP-133 a linked desktop queues no node plan, so a
  cut-off desktop's plans and the node's cannot share an identifier. D-53's rule still has two
  sources of work -- a journal written before GAP-130, whose small integers collide, and any
  future path that puts one plan on both queues -- but not the ordinary outage. That is what
  D-58's comparison by track is for, and row 8 has to hand the cut-off desktop two of the
  node's plans to give the rule something to settle; its module documentation says so.
- **A sign-in during an outage ends it** (GAP-143). `session.rs`'s `connect_if_remote` builds
  a new link, swaps the remote services back in and clears the fallback, so the desktop leaves
  embedded without a person switching back (D-15), and the outage is never reconciled, compared
  or forwarded. Row 8 decides with nobody signed in rather than signing in again.
- **A desktop that restarts during an outage forgets it** (GAP-142): the fallback lives in
  memory, and a desktop that starts with its node unreachable never falls back at all.
- **No desktop has an identity of its own** (GAP-141). Every desktop's certificate names
  `gungnir-app`, so a forwarded decision's `origin` says it was taken on a desktop and not on
  the node, and cannot say which desktop. `session::DESKTOP_COMMON_NAME` is now the one place
  that name is written, so `origin` follows the certificate when that changes.

## GAP-137 stays open, and why GAP-134 could not close it

GAP-133 found the writer that stands in the way: a cut-off desktop's `issue_for` still calls
`republish_handoffs`, which queues the desktop's whole handoff set on the link, and on
reconnect `flush_exchange` replaces what the node holds. GAP-134 does not remove that writer
-- a cut-off desktop must still issue handoffs, which is §6.7 -- and row 8 exercises exactly
that path. Wiring the node's own set in needs a register with a producer per writer, and
**the build has nothing to key a desktop producer on**: the certificate names every desktop
alike (GAP-141), and the operator would collide for one person on two consoles and duplicate
for an operator change on one. A register keyed on either would be half-wired, so GAP-137
stays open and now names GAP-141 beside the DN-18 amendment it already needed.

## Row 8

`gungnir-app/tests/cut_off_and_reconnected.rs`. Desktop A reaches a real in-process node
through a TCP proxy the test cuts; desktop B, connected directly, decides on the node's queue
throughout. Each clause is paired with the zero that makes it mean something: five offline
decisions, none on the node before the forwarding and each exactly once after it, through a
real `504` retry and a deliberate resend, with the node's engagements unchanged; the delegated
item the Operator's one second before the interval and the Supervisor's one second after,
beside an item the Operator holds on its own account that does not move; one conflict the
rule settles and one a person does, nothing forwarded until both are settled, and both on the
node's journal once; two tracks engaged on both sides -- one with no plan conflict, one whose
conflict the rule settled against the node's side -- and two engaged on one side that raise
nothing. Breaking each mechanism in turn fails the test.

The three node harnesses -- rows 3 to 6, 7 and 9 -- call `answer_forwarded` where `main.rs`
now does, so their claim to run the binary's loop stays true; none of their assertions
changed.

## Where the facts are

The route is [`../../gungnir-api-v1.md`](../../gungnir-api-v1.md); the model, payload and
configuration changes are [`../../design/model-and-schema-deltas.md`](../../design/model-and-schema-deltas.md)
§2, §4 and §7, and `SCHEMA_VERSION` stays 4; the setting is documented for a deployment in
[`../../../deploy/README.md`](../../../deploy/README.md). DN-31 §9 row 8 is in
[`../../verification-capability-table.md`](../../verification-capability-table.md) §2 with the
criterion the owner agreed, unchanged by this build, and is not a gate yet (D-16); the
sentence above that table saying rows 7 to 9 wait on the gaps that build them is now true of
none of them, and is the owner's to update. Human-owned paths this change touches:
`gungnir-api`'s write paths, `gungnir-policy`, `gungnir-command`, `gungnir-approval` and
`gungnir-node/src/approval.rs`. What the owner has signed is
[`../../signatures.md`](../../signatures.md).
