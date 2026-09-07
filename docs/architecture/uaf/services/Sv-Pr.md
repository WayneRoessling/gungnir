# Sv-Pr Service processes

**UAF definition.** The service processes view shows the behaviour of services: the
sequence of operations that realize an activity.

**Purpose here.** The two service-level processes every engagement thread depends
on: the tick (detection to recommendation) and the decision path (recommendation to
recorded decision), plus the reconnection process. Each is the sequence of service
calls, with the calls the binaries make today distinguished from those the design
adds.

Status: first draft, 2026-09-04.

## The tick (realizes OA-01, OA-02, OA-04, OA-05, OA-11, OA-13)

Order from `gungnir-app::update::tick` and `gungnir-node::main` (Rs-Pr):

1. SV-05 `now()`.
2. SV-08 `IngestGateway::tick(now, tracking)`: each adapter's payloads are
   validated and authenticated; accepted IE-01 go to SV-01 `submit_detection`; each
   outcome is an IE-15 published on SV-03.
3. SV-01 `poll(now)`: the pipeline's output becomes the IE-02 snapshot (today the
   ingest task drains and the snapshot stays empty; `is_healthy` is false).
4. SV-02 `plan(now, tracks, resources)`: today `NotImplemented` inside the allocator
   returns the last good plan, flagged; when the plan changes, IE-14 PlanProposed is
   published.
5. Design adds (GAP-028): SV-17 scores every track; SV-15 evaluates the plan; the
   verdict and rationale (SV-18) accompany the plan; SV-16 opens a pending approval
   and publishes IE-16 ApprovalRequested.
6. Health: IE-06 assembled from the three services' `is_healthy` (SV-24 correlates
   alerts in the node).
7. SV-04 `append` for every envelope the bus carried this tick.

## The decision path (realizes OA-06, OA-07, OA-08, OA-35)

1. SV-27 shows the pending approval in the approval queue for the roles whose layout
   includes it.
2. The decider acts; SV-21 `require(operator, plan.decide or plan.override)`.
3. SV-16 `decide` records IE-18 and publishes IE-16 Decided; SV-22 records IE-20
   (GAP-059).
4. On acceptance: IE-14 PlanApproved; the handoff message leaves through SV-23
   (GAP-040, GAP-041); the plan becomes actionable only now (CAP-4.3).
5. In the connected profiles the desktop sends IE-23 through SV-30 to SV-23 and the
   node performs steps 2 to 4; SV-26 arbitrates if two deciders conflict.

## Reconnection (realizes OA-27, OA-28, OA-29)

1. SV-30 detects the link (startup today; heartbeat under GAP-050) and switches the
   desktop to embedded services with an alert through SV-27.
2. While detached: SV-30's outbox holds IE-24 submissions; SV-25's queue holds
   envelopes; SV-04 journals locally; decisions are recorded locally under the
   delegation in force (D-15).
3. On reconnection: the outbox and queue drain through SV-23; SV-25 `reconcile`
   merges the local and node journals, drops duplicates, reports conflicts (IE-31).
4. SV-26 resolves each conflict by the role-rank rule; the supervisor confirms
   through SV-16; the desktop returns to the remote backend.

## Elements used

- SV-01 to SV-08, SV-15 to SV-18, SV-21 to SV-27, SV-30; IE-01, IE-06, IE-12 to
  IE-16, IE-18 to IE-20, IE-23, IE-24, IE-31.

## Notes

- The per-tick budgets are MOP-06 (plan recompute) and the desktop `update()`
  budget in `../../../performance-budgets.md`; the decision path budget is MOP-07
  (500 ms).

## Traceability

- Derives from: Rs-Pr; Sv-If; Op-St (the plan lifecycle); `../../../gungnir-api-v1.md`.
- Feeds: Op-Pr (each system step is a segment of one of these processes), plan 06
  interaction design, the operational-readiness verification rows.
