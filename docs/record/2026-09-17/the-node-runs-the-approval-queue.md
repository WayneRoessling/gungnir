# The node runs the approval queue

GAP-132, the third of DN-31's five build increments
([`../../design/DN-31-node-approval-queue.md`](../../design/DN-31-node-approval-queue.md)
§3, §4, §6, §7, §9 rows 3 to 6, §10). The node holding the queue was decided by the owner
as D-55, the crate and its edges as D-57, and the pre-delegated item's deadlines as D-59
([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)).

**A second host for one decision path, not a second decision path.** Nothing in
`gungnir-approval` was rewritten for the node: it supplies the two things a host supplies,
the picture (`ApprovalContext`) and where an effect goes (`ApprovalHost`), and the rules run
as they run on a desktop. What is new is where the queue lives -- the node loop -- and the
two routes that reach it.

## The queue belongs to the loop

`gungnir-node/src/approval.rs` holds three loop steps and `main.rs` calls them in the tick:
`propose` when the planner produces a fresh plan, then `sweep`, then `answer_decisions` and
`audit_refused_decisions`. All four run before the journal append, so everything they
publish is on disk in the tick it happened.

`gungnir-api`'s routes decide nothing. `POST /v3/queue/{item}/decision` hands its request
to the loop through a `PendingDecision` carrying a `oneshot::Sender` and waits for the
answer, which is the sensor-tasking route's own pattern and the same two-second window.
`refuse_decision`'s doc comment said why that mattered while the route refused -- a queue
invented in a request handler would put the recommend-versus-act boundary in the transport
-- and one loop taking requests in arrival order is what makes "the first valid decision
wins" a property of the design rather than the outcome of a race.

**The four pre-loop checks are the route's, in DN-31 §6.3's order**: the token, the
permission (`plan.decide`, or `plan.override` for an override), whether the item is offered
to that role, and whether a rejection says why. Each is a question about the caller rather
than about the queue, and refusing them at the door keeps an unauthenticated caller from
occupying the loop at all. The third reads the queue the loop last published; an item that
queue does not hold goes to the loop anyway, because only the loop can say whether it was
decided, expired, or never issued here.

## The offering, and GAP-113

DN-31 §6.1 answers the objection that kept the queue off the node -- that the authority
engine asks who is asking, and nobody signs in to a node -- without inventing a role for the
node. `gungnir_approval::offer_to` runs the **whole chain once per role on the escalation
ladder**, lowest authority first, and the first role whose chain returns
`RequiresHumanApproval` is the offer. That is the same question as "holds authority for
every solution's layer and class", because the authority engine walks every assignment and
denies on the first it has no rule for; asking through the chain rather than through the
matrix keeps the offer and the verdict from being decided by two different pieces of code.

A plan no role on the ladder may accept is denied, published with the engines that ran, and
never queued -- and the verdict is the **highest** role's, which is the one that is not an
artefact of who was asked: the three other engines do not read the asking role, so if the
top of the ladder is denied for another reason, so is everybody.

This is GAP-113's "somewhere to go" on the node, and only on the node: an area-layer plan an
Operator may not accept is now queued for a Supervisor rather than counted. `gungnir-app`'s
own submission still runs the chain for the role at the console, which is what a desktop
does for its own plans while cut off (§6.7), so that gap is in progress rather than closed.

## What the decision route is keyed on, and why `/v3` lost a route

`/v2/plans/{plan_id}/decision` answered `501` with "this node runs no approval queue". D-55
made that false, and DN-31 §7's table keys the decision on the queue item -- which carries
the deadline, the roles it is offered to and the escalation, none of which a plan identifier
names. So **`/v3` serves no plan-keyed decision route at all**: a second door would put
§6.3's four checks in two places, which is exactly the duplication the router's own comment
about the exchange routes warns against.

That made the retired `/v2` route the one place where a successor is not the caller's own
path under `/v3`: a plan identifier cannot be rewritten into a queue item's. `RetiredRoute`
gained an optional successor and names the template instead. Amendment 1's erratum
anticipated this, holding the successor at `/v3/plans/{plan_id}/decision` only "until
GAP-132 builds §7's queue routes".

## What had to move for the model to describe a queue

- **`PendingApprovalId` moved to `gungnir-model`**, re-exported by `gungnir-command`, which
  still mints it. `CommandEvent::Queued` names the item a node queued and the model may not
  depend on the crate that holds the queue. The type is unchanged in shape, written form and
  tag, so no journal, payload or fixture reads differently.
- **`DecisionRecord` gained `item`, `request` and `origin`**, all defaulted. Without the
  first, "what became of item X" could only be answered from an index beside the history,
  which is a second place it could go wrong; with it, `409 AlreadyDecided` reads the
  decision that stands, and `409 Expired` the window that closed, out of the append-only
  record. `request` is what makes a retry the same request; `origin` is GAP-134's to fill
  and is declared here so `to_event` derives the whole event from the record, as that
  function's own comment requires.
- **`ApprovalWorkflow::decide` takes a `DecidedBy`** in place of two `Option<String>`s, so
  the operator, the role and the request key of one act cannot be passed separately and
  disagree.
- `RequestId` is **not** a UUID under D-60: the client chooses it rather than this
  deployment minting it. It is validated non-empty, bounded and free of control characters,
  because it is untrusted text that reaches an append-only record.

`SCHEMA_VERSION` stays 4. Every addition is a defaulted field or a new enum variant, which
`../../gungnir-api-v1.md`'s compatibility rules call compatible, and every payload that
already read still reads.

## Three things the build turned up

- **The delegation flag was read off the wrong role.** `queue_view` asked the authority
  matrix about `PendingApproval::current_role`, which is the *last* role an item has been
  offered to. Escalation adds a role without removing the first (DN-10 §5), so an escalated
  item's row claimed the Operator's D-15 delegation had lapsed when it had not. DN-31 §5.2
  reads the flag as "actionable for the Operator from submission", so it is the role the
  item was submitted to. Found by row 6's pre-delegated case, which asserts the flag
  survives the escalation it also asserts.
- **A test that drove a copy of the loop would prove the copy.** Rows 3 to 6 are about what
  a *node* does and a test cannot reach inside a binary, so `gungnir-node` gained a `[lib]`
  beside its `[[bin]]`, holding only the approval wiring -- the arrangement `gungnir-app`
  has had since it was written. `gungnir-node/tests/approval_queue.rs` calls the same four
  functions `main.rs` calls, in the same order.
- **A tokio runtime waits for its blocking tasks.** The first harness ran the loop on
  `spawn_blocking`; a failing assertion left the loop running, the runtime would not shut
  down, and the test binary hung rather than reporting the failure. It is a plain thread now,
  stopped by `Node`'s `Drop` whether a test passes or panics.

## One diagram is rendered by a different engine, and why

`PendingApprovalId` joining the model put one more class into
`information/If-Sr-plans-effectors-handoff.puml`, and PlantUML's smetana layout --
which `build_uaf.py` pins on every If-Sr detail diagram, for a reason its own comment
measured -- now throws `ArrayIndexOutOfBoundsException` on it, deterministically. The
committed SVG for that one diagram is therefore a Graphviz render, which the same image
produces cleanly, so that `build_uaf.py` can position every element and its check passes.
Every other If-Sr SVG is still a smetana render, so two diagrams in that directory are laid
out by different engines until GAP-139 settles which. **The type was not moved to another
module to dodge the crash**: it belongs beside `DecisionId`, which is the identifier a
reader will look for it next to.

## What this increment does not do

The node issues handoffs and republishes none: while a linked desktop still issues them for
a node's plan -- which it does until GAP-133 -- two writers to one exchange register would
each silently overwrite the other. Filed as GAP-137 rather than half-wired.
`policy.delegation.disconnected_lapse_s` is not added: DN-31 §7 states it and GAP-134 uses
it, and the node does not need it to run. `gungnir_api::v3::ApprovalRequest` is now reachable
from no route and describes a decision the interface no longer has; removing it moves the
UAF model, so it is GAP-138. Nothing here projects the node's queue onto a desktop, stops a
linked desktop deciding a node plan, or forwards an offline decision.

## Where the facts are

Edge (y) is `ARCHITECTURE.md` §7's table and §7.1's graph, justified in
[`../../design/dependency-edges.md`](../../design/dependency-edges.md) §18 and checked by
`gungnir-app/tests/dependency_graph.rs`. The routes are
[`../../gungnir-api-v1.md`](../../gungnir-api-v1.md); the model and payload changes are
[`../../design/model-and-schema-deltas.md`](../../design/model-and-schema-deltas.md) §2.
DN-31 §9 rows 3 to 6 are in [`../../verification-capability-table.md`](../../verification-capability-table.md)
§2 with the criteria the owner agreed on 2026-09-17, unchanged by this build, and are not
gates yet (D-16). What the owner has signed is
[`../../signatures.md`](../../signatures.md).
