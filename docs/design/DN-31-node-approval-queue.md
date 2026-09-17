# DN-31 A node approval queue

Closes GAP-129 through its five build gaps, GAP-130 to GAP-134. Status: **design only; no
code exists**, written 2026-09-17 on the owner's decisions D-55 to D-59. **Human-owned**:
this note moves the approval workflow (`gungnir-command`), authority enforcement
(`gungnir-policy`) and a `gungnir-api` write path onto the node, and every row in §9 is a
pass criterion. What the owner has signed of this note is in
[`../signatures.md`](../signatures.md).

## 1. The gap and the thread steps it blocks

As built on `main` at `8ec262c`:

- **A node proposes plans and decides none.** It publishes `PlanProposed` for a fresh plan
  and runs two of the four policy engines (`gungnir-node/src/main.rs`, `evaluate_on_node`).
  It holds no queue, `POST /v2/plans/{plan_id}/decision` refuses with 501
  (`gungnir-api/src/transport.rs`, `refuse_decision`), and no decision reaches its journal.
- **Every linked desktop decides the node's plans itself.** `RemoteInterceptService::plan`
  hands the node's plan back as fresh while connected, and the desktop's tick publishes its
  own `PlanProposed`, runs its own four engines and queues it (`gungnir-app/src/update.rs`,
  `decisions::submit`). Deciding opens the engagement and issues the handoff
  (`engagements::open_for`, `handoffs::issue_for`).
- **So two desktops on one node each decide the same plan, and neither knows.** Both can
  open engagements and hand off to the same effector. Their identifiers collide as well:
  `DecisionId`, `PlanId` and `PendingApprovalId` restart at 1 in every process; an effector
  report finds its handoff by `DecisionId` alone (`gungnir_model::handoff::accept_report`);
  the node keeps one exchange set per item, so each desktop's publication replaces the
  other's; and `gungnir_resilience::reconcile` pairs two journals by `PlanId`.
- **Escalation reaches nobody.** DN-10 §5 offers an escalated item to the role above,
  "whoever decides first ends it". A desktop's role is its signed-in account's (D-53), so a
  Supervisor sees an item escalated on an Operator's desktop only by signing in at that
  console. DN-09 §7's marking of what must go up has no one to go up to either (GAP-113).

The thread steps this blocks: **MT-01 step 6**, the supervisor manages the queue, whose
failure is a queue that outruns the authority; and **MT-10 steps 2 to 5**, decisions queued
for forwarding, the supervisor at the node resolving conflicts and the node's record
updated, whose failure is "duplicate or lost decisions". MOE-11, the fraction of offline
decisions that reach the node's record, cannot be computed while no decision can.

**Why the queue went to the desktop, and what changed.** The node's refusal was argued when
nothing could authenticate a caller and the operator was named in the request body
(`../record/2026-09-05/059.md`, `../record/2026-09-05/062.md`), and GAP-028 added that
"nobody signs in to a node", so a queue there could only expire. Edge (k) brought the policy
chain to the node and deliberately no queue. Since then GAP-057 has closed: every node route
verifies an operator token that carries a role. And the worry that a queue in a request
handler would put the line between recommending and acting inside the transport has an
answer already in the code: the sensor-tasking route hands its request to the node loop and
waits for the loop's reply.

**The architecture intended this.** `ARCHITECTURE.md` §8.2 and §8.3 as first written made
the node the record for the connected profiles and "the arbiter when several operators share
a mission", and `docs/gungnir-api-v1.md` says a desktop "never treats its own state as
authoritative while connected". The build diverged; D-55 returns to the design.

## 2. The decisions this note is built on

| Decision | What the owner decided on 2026-09-17 |
|---|---|
| D-55 | When desktops are linked, the node holds the queue: the node authorizes each decision against the caller's role, takes one decision per item, opens the engagement, issues the handoff and journals all of it. While connected, the first valid decision on an item wins and a later one is refused naming the one that stands. A desktop cut off from its node decides what its signed-in role may, with D-15's delegations lapsing after a configured interval, and forwards its decisions when the node answers. The code both binaries need lives in a shared library |
| D-56 | Decision, plan and queue-item identifiers become UUID v7, minted where each thing is created |
| D-57 | The shared library is a new productization crate, `gungnir-approval`, with edges (w) to (y) (§4) |
| D-58 | Reconciliation also compares engagements by track: two engagements of one track opened on the two sides of an outage are a both-may-have-acted incident for a person, whatever their plans |
| D-59 | A pre-delegated item still expires and escalates, as DN-10 §5 says and the code does; GAP-035's closing note, which says escalation is skipped, is corrected |

**What costs are accepted with them.** Deciding now needs the node reachable, so the decision
path's latency spans the node loop and the event stream (MOP-07, §6.9). There are two queues
by design, the node's and each desktop's for when it is cut off, and reconciliation after an
outage resolves what they disagree about (D-53, D-58). The identifier change breaks the wire
format once (§5.1). And the largest human-owned move so far: the policy chain, the queue's
feeding and the one handoff builder leave `gungnir-app`.

**What does not change.** A desktop deployed on its own (§8.2's first profile) keeps its local
queue and needs no node. No path accepts on expiry (DN-10 §5, C-01). No engagement opens and
no handoff is issued without a person's decision on the record (C-01, C-04).

## 3. The owning components

**The node loop owns the queue, not the request handler.** `gungnir-node` holds an
`ApprovalWorkflow` and processes decision requests between ticks, exactly as it issues sensor
tasks (`issue_api_tasks`). One loop taking requests in arrival order is what makes "the first
valid decision wins" a property of the design rather than the outcome of a race.

**`gungnir-approval`** (new, D-57) holds what both binaries run, so each rule has one
implementation:

1. **The policy chain over a plan**: readiness and geofence, control status, authority and
   fires, replacing `gungnir-app`'s `decisions::evaluate` and the node's `evaluate_on_node`.
2. **Submission**: the governing layer and deadline (`gungnir_command::queue`), and the roles
   an item is offered to (§6.1).
3. **The sweep**: expiry and escalation, each published.
4. **Deciding**: authorization, the `DecisionRecord`, engagement opening and handoff building.
   **The one handoff builder** moves here from `gungnir-app/src/handoffs.rs`, and
   `gungnir-app/tests/no_execution_without_decision.rs`, which pins that there is one, moves
   its pin with it.
5. **Handoff delivery bookkeeping**: the retry schedule and the rule that a handoff is never
   dropped (DN-07 §5), over a `HandoffTransport` trait each binary implements with
   `gungnir-remote`'s endpoint client. The library never reaches the transport.

**`gungnir-command`** keeps the queue (`ApprovalWorkflow`, `InMemoryApprovalWorkflow`,
`queue.rs`) and gains the escalation ladder `gungnir-app/src/decisions.rs` holds today.

**`gungnir-api`** gains the queue routes and types (§7). A route authenticates, authorizes and
decodes, then hands the request to the node loop and returns the loop's answer.

**`gungnir-remote`** gains the desktop's queue projection, the decision client, and a
store-and-forward outbox for decisions taken while cut off.

**`gungnir-app`**'s `decisions.rs`, the opening half of `engagements.rs`, `handoffs.rs` and the
handoff half of `deliveries.rs` become thin callers of `gungnir-approval`. PN-06 and PN-07 read
the node's queue while linked (§8).

## 4. Edges (D-57)

| Edge | What it is |
|---|---|
| (w) `gungnir-approval` → `gungnir-command`, `gungnir-policy`, `gungnir-intercept-service`, `gungnir-security`, `gungnir-config`, `gungnir-eventing`, `gungnir-model` | The queue, the engines, the engagement types (`gungnir-intercept-service/src/engagement.rs`), role permissions, the settings and resources, the bus and the model. A new productization crate one level under the binaries |
| (x) `gungnir-app` → `gungnir-approval` | Replaces the desktop's own copy of the decision path |
| (y) `gungnir-node` → `gungnir-approval` and `gungnir-node` → `gungnir-command` | The node holds the queue and names its types |

Downward and acyclic: none of (w)'s targets depends on a binary or on `gungnir-approval`
(`gungnir-intercept-service` depends on core, coord, allocation and model; `gungnir-command` on
model and policy; `gungnir-policy` on model and geo). Each edge is drawn in §7.1 and recorded in
`dependency-edges.md` in the change that adds it to a manifest, as that note's §5 requires.

Refused: `gungnir-approval` → `gungnir-remote`, which would put a productization crate on the
client transport (delivery goes through the trait instead; both binaries already reach
`gungnir-remote`, the node through edge (p)). Growing `gungnir-command` to hold the whole path,
which would take the human-owned queue crate into engagement, configuration and security code.
A copy on the node, which D-55 refused.

## 5. Types

### 5.1 Identifiers (D-56)

```rust
// gungnir-model: minted with uuid's v7 feature where the thing is created, as
// GlobalEntityId already is (D-11 admits `v7` "only where identities are minted").
pub struct DecisionId(pub u128);        // was u64, restarting at 1 per workflow
pub struct PlanId(pub u128);            // was u64, restarting at 1 per planner
pub struct PendingApprovalId(pub u128); // gungnir-command; was u64
```

A time-ordered 128-bit identifier needs no coordination between machines, survives restarts,
and keeps ordering by creation. **It changes a field's type, which the interface's own rule
treats as breaking** (`model-and-schema-deltas.md` §3): `SCHEMA_VERSION` goes from 3 to 4
and the routes move to `/v3`, with every `/v2` route answering `410 Gone` naming its
successor. Every payload that carries a plan or a decision changes, so `/v2` is retired
whole rather than route by route, and peer nodes and partner machines move to `/v3` in the
same release (D-01's one release). A journal written before the change still reads, because
a u64 number is a valid u128; the replay and reporting paths are tested against one (§9, row
5).

### 5.2 The queue on the wire

```rust
// gungnir-api v3.

/// One item in a node's queue, as PN-06 draws it.
pub struct QueueItemView {
    pub item: PendingApprovalId,
    pub plan: PlanView,
    pub verdict: VerdictSummary,
    /// The layer whose window governs the deadline (`queue::governing_layer`).
    pub layer: EffectorLayer,
    pub submitted: MissionTime,
    pub expires_at: Option<MissionTime>,
    pub escalate_at: Option<MissionTime>,
    /// Every role the item is offered to, in escalation order (DN-10 amendment 1 c).
    pub offered_to: Vec<String>,
    pub pre_delegated: bool,
    pub priority: f32,
}

/// A person's decision on one queue item.
pub struct DecisionRequest {
    /// Chosen by the client and journaled with the decision, so a retry is the same
    /// request rather than a second decision.
    pub request: RequestId,
    pub item: PendingApprovalId,
    pub choice: DecisionChoice,
}

/// As PN-07 offers today. An override records the queued plan under the override
/// permission; a substituted assignment is not built and this note does not add one.
pub enum DecisionChoice {
    Accept,
    Override,
    Reject { reason: String },
}

/// `201`: the decision this request recorded, or recorded before under the same `request`.
pub struct DecisionRecorded { pub decision: DecisionId }

/// `409`: why the item takes no decision now.
pub enum DecisionRefused {
    /// Somebody decided first; their decision stands.
    AlreadyDecided { decision: DecisionId, operator: String, role: String, at: MissionTime },
    /// The window closed (DN-10 §6).
    Expired { at: MissionTime },
}

/// A decision taken while this desktop was cut off from its node, forwarded on reconnect.
pub struct ForwardedDecision {
    pub record: DecisionRecordView,  // identifiers, plan, choice, operator, role, time
    pub origin: String,              // the desktop's machine identity
}
```

### 5.3 Events and the snapshot

- `CommandEvent::Queued { item, plan, layer, offered_to, expires_at, escalate_at }`: a new,
  additive variant, so a desktop following the stream builds the queue without polling.
  `CommandEvent::ApprovalRequested(PlanId)` exists, carries too little, and nothing publishes it;
  it is removed in the v4 schema.
- `CommandEvent::Decided` gains `request: Option<RequestId>` and `origin: Option<String>`, both
  defaulted: the idempotency key, and the machine a forwarded decision was taken on.
- `CommandEvent::Escalated` and `Expired` are unchanged.
- `LinkEvent::BothActed { track, local, remote, at }` (D-58): two engagements of one track on the
  two sides of an outage.
- `SnapshotResponse` gains `queue: Vec<QueueItemView>`.

## 6. Behaviour

### Connected

**6.1 Proposal and offering.** The node proposes a fresh plan and runs the whole chain. The
authority engine is evaluated for **every role on the ladder** rather than for one asking role,
and the item is offered to the lowest role holding authority for every solution's layer and
class, with the roles above it reachable by escalation. A plan no role on the ladder may accept
is `Denied { Authority }`, published with the engines that ran, and never queued. This is DN-09
§7's "what must go up" with somewhere to go (GAP-113).

**6.2 Queueing.** The node submits the item and publishes `Queued`. Ordering is DN-10 §5's: time
remaining, then priority. A pre-delegated item is actionable for the Operator from submission
and **still expires and escalates** (D-59).

**6.3 Deciding.** `POST /v3/queue/{item}/decision` with a `DecisionRequest`. The route,
before the loop sees anything:

| Check | Refusal |
|---|---|
| A valid, unexpired operator token | `401`; nothing recorded, and a decision under an expired session is not recorded at all (DN-23 §5 rule 2) |
| `plan.decide`, or `plan.override` for an override | `403` naming the role and the action |
| The item is offered to the token's role | `403` naming the roles it is offered to |
| A rejection carries a non-empty reason | `400` (DN-10 §3) |

Then the loop, in arrival order: an item still pending is decided. The record names the
token's operator and role (D-53), `Decided` is published with the request id, the engagement
opens, the handoff is built and delivered, one audit entry is written, and the route answers
`201 DecisionRecorded`. An item already decided answers `409 AlreadyDecided` naming the
decision that stands. An expired item answers `409 Expired`. A request id already recorded
answers the first outcome again and records nothing. If the loop does not answer within the
reply window, the route answers `504`, **which does not mean nothing was recorded**: the
client retries with the same request id and learns which.

**6.4 The sweep** runs on the node's clock: expiry records `Expired` and publishes it;
escalation adds the next role to `offered_to` and publishes `Escalated`, at most once per
rank step. The original role still sees the item, and whoever decides first ends it (DN-10
§5), from any desktop.

**6.5 One issuer.** While a desktop is linked, only the node opens engagements and issues
handoffs for the node's plans. The desktop shows them from the stream; its own queue,
engagement opening and handoff issuing do not run for a plan the node proposed.

**6.6 The desktop's projection.** PN-06 shows every item in the node's queue; its controls are
enabled only for items offered to the signed-in role, and the rest say who they are offered to.
PN-07 decides through the route. On a `409` it shows who decided, as which role and when, and
closes; on a `403` or `401` it says why and records nothing locally.

### Cut off

**6.7 A desktop that loses its node** falls back to its embedded services, as GAP-050 built,
and its own queue takes the plans its own planner proposes. It decides what its signed-in
role may (D-55). Delegations in force at the moment of disconnection stay in force for
`policy.delegation.disconnected_lapse_s`, then lapse, and no new delegation is made while
cut off (D-15). Items that were in the node's queue stay there: other desktops may still
decide them.

**6.8 On reconnect** the desktop forwards every decision it took while cut off, in order,
through `POST /v3/decisions/forwarded`. The node journals each as `Decided` with its
`origin`, once: forwarding the same `DecisionId` again is acknowledged and records nothing.
Reconciliation then runs over the node's history, which now holds real decisions. D-53's
rule resolves the plan conflicts it can rank and a person resolves the rest. **D-58: it also
compares engagements by track**, and two engagements of one track opened on the two sides of
the outage are published as `BothActed` and alerted for a person, whatever their plans and
whatever the rule decided, because an effect in the world is not undone by choosing which
record stands. The rule's verdicts and the person's resolutions are forwarded too, so the
node's record says what stands (MT-10 step 5).

### Timing

**6.9 MOP-07**, plan proposed to approval control available in under 500 ms, now spans the node
loop, the journal append and the stream. The node publishes `Queued` in the tick that proposes
the plan, and the row in §9 measures the whole path on a desktop.

## 7. Configuration and interface delta

| Route | Body | Answer | Permission |
|---|---|---|---|
| `GET /v3/queue` | none | `Vec<QueueItemView>` | `picture.view` |
| `POST /v3/queue/{item}/decision` | `DecisionRequest` | `201 DecisionRecorded`; `400`, `401`, `403`, `409 DecisionRefused`, `504` | `plan.decide`, or `plan.override` for an override |
| `POST /v3/decisions/forwarded` | `Vec<ForwardedDecision>` | `202`, or `409` naming a record that contradicts one already held under the same identifier | `plan.decide` |
| `POST /v2/plans/{plan_id}/decision` | any | `410 Gone` naming the `/v3` route | none beyond authentication |

`SnapshotResponse` gains `queue` (§5.3). Configuration gains
`policy.delegation.disconnected_lapse_s`, validated positive, with no default: a deployment that
links desktops to a node states how long an offline delegation lasts rather than inheriting one.
`DecisionSettings` is unchanged.

## 8. User-interface delta

| Panel | Change |
|---|---|
| PN-01 Status strip | Whose queue is in force: the node's, or this desktop's while cut off |
| PN-06 Approval queue | The node's queue while linked; items not offered to the signed-in role shown, with the roles they are offered to, and not actionable |
| PN-07 Decision dialog | Decides through the node; "decided by *operator* as *role* at *T*" on a conflict |
| PN-17 Commander summary | The node's queue state: expiries, escalations and decisions by role in the period |
| PN-18 Reconciliation | Real node conflicts, and both-acted incidents above everything else |

## 9. Verification

Proposed with this note. Each row is written and passes in the gap that builds it, and enters
`verification-capability-table.md` §2 when the owner agrees its criterion, as DN-25's rows did.

| # | Capability | Method | Pass criterion | Data source | Built in |
|---|---|---|---|---|---|
| 1 | CAP-7.2 Identifiers | Mint decisions and plans on a node and two desktops across restarts; replay a journal written before the change | No two identifiers equal across machines and restarts; an effector report reaches exactly one handoff; MOE-05's join is exact across machines; a pre-change journal replays and reports unchanged | Synthetic; a committed pre-change journal | GAP-130 |
| 2 | CAP-4.2 The decision path in one place | The desktop's existing decision, engagement, handoff and C-01 tests, run against `gungnir-approval` | Every existing test passes unchanged in what it asserts; exactly one handoff builder in the workspace; no configuration makes an expiry accept (DN-10's exhaustive search in `gungnir-command`, still passing) | Existing fixtures | GAP-131 |
| 3 | CAP-4.2, CAP-4.3 One decision per item | 100 randomized races: two operator clients decide the same item over the real transport; retries with the same request id | Exactly one `DecisionRecord` per item; the first valid decision stands and every later one is refused `409` naming it; a retry answers the first outcome and records nothing; one engagement per solution and one handoff per actionable decision | Synthetic plans; an in-process node on the real transport | GAP-132 |
| 4 | CAP-6.2 Authorization on the node | Each role against each item state, each choice, and an expired token | A role without `plan.decide` (or `plan.override` for an override), or not offered the item, is refused `403`; an expired token `401`; a rejection without a reason `400`; in every refusal nothing is recorded, published or engaged; every decision and every refusal writes exactly one audit entry on the node | Synthetic accounts and plans | GAP-132 |
| 5 | CAP-3.6 Authority and offering | Plans per layer and class against the authority matrix | Each item is offered to the lowest role holding authority for every solution; a plan no role on the ladder may accept is `Denied { Authority }` and never queued; an item beyond the Operator's authority is actionable by a role that holds it (GAP-113) | The authority matrix | GAP-132 |
| 6 | CAP-3.7 Expiry and escalation on the node | Items past `escalate_at` and `expires_at` with a second desktop signed in as the higher role; a pre-delegated item | Escalation adds the next role without removing the first, at most once per rank step, and the higher role's desktop can decide it; a decision on an expired item is refused `409 Expired` and the record is `Expired`, never accepted; a pre-delegated item expires and escalates (D-59) | Synthetic; TT-01 saturation | GAP-132 |
| 7 | CAP-5.9 Two desktops, one queue | Two desktops linked to one node, each signed in as a different role | Both show the same queue in the same order; a decision on one reaches the other's PN-06 as decided, naming who; neither desktop queues, engages or issues a handoff for a node plan; plan proposed to approval control available on a desktop in under 500 ms (MOP-07) | Synthetic; tracing spans | GAP-133 |
| 8 | CAP-5.4 Cut off and reconnected | A desktop loses its node mid-queue, decides, and reconnects; the node's queue keeps running for a second desktop | Every offline decision reaches the node's record exactly once (MOE-11 = 1.0) and none is lost or duplicated (MT-10); forwarding twice records nothing new; D-15's delegations lapse after `disconnected_lapse_s`; plan conflicts are resolved by D-53's rule or a person; two engagements of one track across the outage raise `BothActed` for a person whatever the verdict (D-58) | Synthetic outage on the real transport | GAP-134 |
| 9 | CAP-4.3 MT-01 with a watch floor | MT-01 saturation replayed with two Operators and a Supervisor on three desktops against one node | Every queued item ends decided once, expired, or escalated; no track is engaged twice by the same shift; every escalation is visible to the Supervisor; every decision is on the node's record | TT-01 saturation set | GAP-133 |
| 10 | CAP-5.4 The desktop alone | The existing disconnected-profile tests with no node configured | Unchanged: a desktop with no node queues, decides, engages and hands off locally | Existing fixtures | GAP-131 |

## 10. Build order

GAP-130 first, because colliding identifiers are a hazard today and every later increment needs
unique ones. GAP-131 next, which moves the decision path into `gungnir-approval` with the
desktop's behaviour unchanged, so the move is verified before anything new rides on it. Then
GAP-132, the node's queue and routes; GAP-133, the desktop's projection; and GAP-134,
forwarding and the both-acted comparison. Each gap writes its rows from §9 and changes no
criterion.

## 11. What stays open

- **Substituted assignments.** An override records the queued plan under the override permission,
  as it does today; a person choosing a different assignment is not built, and this note does not
  design it.
- **GAP-127 on the cut-off desktop.** The node authorizes every decision per request (§6.3); a
  desktop deciding on its own still checks the permission only in its panels until GAP-127 closes.
- **Several nodes.** One node is the record for the desktops linked to it. Queues shared across
  peer nodes (DN-16's peers) are out of scope.

## Traceability

GAP-129, GAP-130 to GAP-134, GAP-113, GAP-127; D-03, D-15, D-53, D-55 to D-59; DN-06, DN-07,
DN-09 §5 and §7, DN-10 §3, §5 and §6, DN-23 §5, DN-25; MT-01, MT-10; MOP-07, MOE-01, MOE-05,
MOE-11; CAP-3.6, CAP-3.7, CAP-4.2, CAP-4.3, CAP-5.4, CAP-5.9, CAP-6.2, CAP-7.2; contracts C-01,
C-04; `ARCHITECTURE.md` §8.2 to §8.4; `docs/gungnir-api-v1.md`.
