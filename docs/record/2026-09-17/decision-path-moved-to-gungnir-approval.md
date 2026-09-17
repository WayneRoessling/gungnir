# The decision path moves into gungnir-approval

GAP-131, the second of DN-31's five build increments
([`../../design/DN-31-node-approval-queue.md`](../../design/DN-31-node-approval-queue.md)
§3, §4, §9 rows 2 and 10, §10). The crate and its edges were decided by the owner as D-57
([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)).

**A move, with the desktop's behaviour unchanged.** Nothing an operator, a journal or a
test sees is different: every test in the workspace passes with its assertions untouched,
and the two that name a file path name the new one. What changed is where the rules live.

## What moved

`gungnir-approval` is a productization crate holding what both binaries need to run one
decision path rather than a copy each (D-55):

- **The policy chain** over a plan: the four engines in order, `PolicyChainReport` and its
  caveats, the fires checks, the friendly positions the picture can place, and the
  classifier the two class-judging engines read (`chain.rs`).
- **Submission and the sweep**: the supersession check before policy (DN-08 §5), the
  published verdict, the denial counters, the governing layer and the submission; expiry
  and escalation, each published (`queue.rs`).
- **Deciding**: the record, the audit entry, the engagement opening and the handoff
  (`queue.rs`, `engagements.rs`).
- **The one handoff builder** and the effector report path (`handoffs.rs`), and the
  delivery schedule with the rule that a handoff is never dropped (`deliveries.rs`).

`ApprovalDesk` owns the state those rules act on -- the queue and its record, the
engagements, the handoffs, what is owed to an endpoint, the denial history, the escalation
count and the last plan's fires checks -- and `AppState` holds one as `state.desk`.

## The boundary, and why it sits there

The desk reads through an `ApprovalContext` the host builds for the call and acts through
an `ApprovalHost` it implements. So the two things a host differs in are the two things it
supplies: what the picture is now (the clock, the role in force, the verified session, the
tracks, the resources, the baseline) and where an effect goes (publish, alert, audit,
republish the exchange set). Nothing in the crate draws a panel, reads a session or opens
a socket.

Three consequences worth recording:

- **`PolicyInputs` is separate from the context.** The geofence service and the placed
  friendly positions are what only the chain reads, and the chain runs when a plan is
  proposed while the sweeps run every tick. Folding them into one context would have made
  every frame build a service and place every friendly track for an answer nobody asked
  for.
- **Delivery goes over a `HandoffTransport`** the binary implements with `gungnir-remote`'s
  endpoint client (DN-31 §3 point 5). The three outcomes the client distinguishes are
  restated as `DeliveryAnswer`, so the library never names the transport -- D-57 refused
  that edge -- and the retry schedule stays with the record it protects.
- **The baseline's validity window is passed in as a bool.** The four states it can be in
  are a `gungnir-ui` type (`BaselineValidity`), which a productization crate may not reach,
  and the rule that decides them already lives in `status.rs` for the status strip. One
  rule, in the host, read by both.

## What stayed in gungnir-app

The presentation (`queue_rows`, `queue_view`, `verdict_sentence`, `denial_sentence`,
`check_name`, `outcome_label`, the handoff rows and `queue_empty_reason`), the alternatives
and the what-if, which judge plans nobody submitted through the same chain via
`with_chain`, the warnings half of the delivery sweep, and the endpoint-table lookup both
halves share. `decisions.rs`, `engagements.rs`, `handoffs.rs` and `deliveries.rs` are thin
callers over one place that builds the context and the host, `desk.rs`.

## Three things the move turned up

- **The escalation ladder could not move as it stood.** DN-31 §3 puts it in
  `gungnir-command`, and that crate cannot see `gungnir_security::Role`: D-57 gives it no
  edge to `gungnir-security`, for the same reason `DecisionRecord::role` is a string. So
  the *ordering* is `gungnir_command::escalation_ladder`, beside the queue that walks it,
  and the roles are gathered by `gungnir_approval::escalation_ladder`, which can see both.
  A `LadderRung` is a name and a rank.
- **`GeoService` needed naming without an edge.** The chain reads the trait
  `gungnir_geo::GeoService`, which edge (w) does not include. `gungnir-policy` already
  depends on `gungnir-geo` and `GeofencePolicy` names the trait in its own public field, so
  it re-exports it: the host builds the service from its baseline and passes it in. That is
  this register's preferred answer to a new edge, and it keeps the graph one crate
  narrower (`../../design/dependency-edges.md` §17).
- **Two pins moved with the code they pin.** `no_execution_without_decision.rs` reads every
  `gungnir-*/src` in the workspace and asserts that `Engagement::open` and
  `Handoff::from_decision` are each constructed in exactly one place, behind
  `DecisionRecord::is_actionable`. Both places are now in `gungnir-approval`, so the two
  path assertions name that crate; the counts, the gate and the runtime half are unchanged,
  and a second construction appearing anywhere -- in a node, in a panel, in a copy taken
  for a harness -- still fails the test.

## What this increment does not do

The node edge (y) is GAP-132's and no manifest carries it, so it is not drawn. The node
keeps its own `evaluate_on_node` until then. Nothing here gives a node a queue, projects
one onto a desktop, or forwards a decision.

## Where the facts are

The crate's place in the graph is `ARCHITECTURE.md` §7 and §7.1 with edges (w) and (x);
their justification is `../../design/dependency-edges.md` §17;
`gungnir-app/tests/dependency_graph.rs` checks both against the manifests. DN-31 §9 rows 2
and 10 are in `../../verification-capability-table.md` §2 and are not gates yet (D-16).
What the owner has signed is [`../../signatures.md`](../../signatures.md).
