# Task analysis: Supervisor (P-02)

Threads: MT-01, MT-02 (the queue and weapons control status), MT-07 (degraded
states), MT-09 (plan apply), MT-10 (reconciliation). Layout: PN-06, PN-05, PN-08
(incidents), PN-09, PN-10, PN-03 beside the viewport; PN-04, PN-07, PN-14, PN-17,
PN-18 on demand.

## T-sup-1 Manage the queue (OA-31)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 1.1 See every domain's pending decisions | pending approvals with priority, time remaining, the decider they are assigned to | none | a queue longer than the authority's capacity, unnoticed | S | PN-06 |
| 1.2 Reorder or reprioritise | priority from `RiskScore`; manual reorder is recorded (MOP-30) | supervisor | silent reorder; losing an item | M | PN-06 |
| 1.3 Pre-delegate a class to operators | delegation rule (D-15) | supervisor | delegating an area-layer engagement (forbidden) | M | PN-17 |
| 1.4 Take a decision the operator cannot | the plan with verdict | supervisor | same as the operator's T-op-4 | S | PN-07 |
| 1.5 Watch expiry and escalation | time remaining, Expired and Escalated states (GAP-034) | none | a plan expiring unseen | S | PN-06 |

## T-sup-2 Set weapons control status (OA-23)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 2.1 Read the current status per layer | status chips (GAP-052) | none | status not visible on every layout | S | PN-01 |
| 2.2 Change it | the new status; who set it; when | supervisor or commander | change not propagated to plans; no record | S | PN-01 (dialog) |
| 2.3 Hold or cease for deconfliction | friendly tracks and corridors, the plans affected | supervisor | hold not applied to a plan already accepted | S | PN-07, PN-02 |

## T-sup-3 Handle degradation (OA-13)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 3.1 See incidents, not raw alerts | correlated incidents with the raw alerts inside | none | a storm of raw alerts | S | PN-08 |
| 3.2 Acknowledge a degraded state on the plan | the degraded flag and which sensors are lost | supervisor | deciding on hidden staleness | S | PN-07 |
| 3.3 Escalate an uncovered asset to the commander | coverage gap over the asset (GAP-006) | supervisor | escalation by voice only, unrecorded | M | PN-08, PN-17 |

## T-sup-4 Apply a plan (OA-24)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 4.1 Review the validated baseline | `ConfigBaseline` sections, validation result, version, validity window (GAP-052) | none | applying an invalid baseline (MOP-36 forbids) | H | PN-14 |
| 4.2 Apply with audit | apply; `AuditEntry` | supervisor or commander | no audit; applying during a raid | H | PN-14 (dialog) |

## T-sup-5 Resolve reconciliation conflicts (OA-29, MT-10)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 5.1 See the report | `ReconciliationReport`: envelopes merged, duplicates dropped, conflicts | none | conflicts resolved silently | M | PN-18 |
| 5.2 See both decisions and the arbitration result | both `DecisionRecord`s, the rule's outcome (higher role, earlier on a tie) | none | not seeing which one the rule chose | M | PN-18 |
| 5.3 Confirm or overturn | a recorded decision | supervisor | overturning without a reason | M | PN-18 |

## T-sup-6 Sensor management concurrence (OA-12)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 6.1 Concur with a collection tasking that costs defense coverage | coverage before and after | supervisor | concurring without seeing the cost | M | PN-10, PN-11 |

## Error modes that shape the design

- The queue is the supervisor's first panel and shows capacity: items pending
  versus decisions per minute the authority has sustained.
- Status changes and holds open a dialog that lists the plans affected before the
  change takes effect.
- Reconciliation opens PN-18 as a dialog the supervisor must close by deciding;
  nothing is applied to the record until then.

## Traceability

- Activities OA-07, OA-12, OA-13, OA-23, OA-24, OA-29, OA-31; capabilities CAP-3.6,
  CAP-3.7, CAP-5.4, CAP-5.5, CAP-5.6; wireframes WF-05, WF-06, WF-07, WF-08, WF-14,
  WF-17, WF-18; flows FL-01, FL-02, FL-05, FL-08.
