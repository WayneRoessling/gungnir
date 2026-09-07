# Task analysis: Operator (P-01)

Threads: MT-01 to MT-06 (the live picture for a domain), MT-10 (disconnected).
Layout: PN-06, PN-05, PN-03, PN-08, PN-09 beside the viewport; PN-04 and PN-07 on
demand.

## T-op-1 Maintain awareness of the domain picture (OA-02, continuous)

| Subtask | Information (model) | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 1.1 Watch new and changed tracks | `TrackView` id, status, classification, `Quality`, `mission_time` | none | missing a new track in a dense picture; reading a stale track as current | M | PN-02, PN-03 |
| 1.2 Distinguish fresh from stale and confident from tentative | `Quality::is_stale`, `association_confidence`, age from `mission_time` | none | stale drawn like fresh (principle 3) | S | PN-02, PN-03 |
| 1.3 Know what cannot be seen | health flags, coverage gaps (GAP-007), sensor states | none | a coverage gap not shown (MT-07) | M | PN-01, PN-09 |
| 1.4 Select a track to inspect | selection in any panel | none | selecting the wrong one of two close tracks | S | PN-02, PN-03, PN-04 |

## T-op-2 Confirm or designate identity where policy assigns the class to the operator (OA-03)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 2.1 Read the evidence and confidence | `Classification`, `IdentificationEvidence` list with source and weight, cooperative identity (GAP-010) | none | one weak source read as strong | M | PN-04 |
| 2.2 Check the class's policy criteria | per-class threshold (GAP-018), whether a person is required | none | declaring below the margin | M | PN-04 |
| 2.3 Designate | the designation as evidence with the operator's identity | operator, per class | designating the wrong track; no record | S | PN-04 (dialog) |

## T-op-3 Review a recommendation (OA-05, OA-06)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 3.1 See the plan for the selected track | `PlanView`, `InterceptSolutionView` resource and track, intercept point when present | none | plan changes under the cursor (PlanSuperseded) | S | PN-05, PN-02 |
| 3.2 Read the verdict | `PolicyVerdict`: Denied with reason, RequiresHumanApproval, Approved (pre-delegated, named) | none | acting on a denied plan; not noticing the verdict | S | PN-05 |
| 3.3 Read the rationale, cost, time remaining | `CourseOfAction.rationale`, `RiskScore` factors, cost (GAP-030), time to impact (GAP-020) | none | rationale hidden behind a click during a raid | S | PN-05 |
| 3.4 Compare alternatives | alternatives, each policy-checked (GAP-032) | none | alternatives that were not policy-checked | M | PN-05 |

## T-op-4 Decide within delegation (OA-07)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 4.1 Confirm the plan is within my delegation | delegation in force (D-15), layer, class | none | deciding above delegation; the dialog should refuse and offer escalation | S | PN-01, PN-07 |
| 4.2 Accept | the plan as shown, verdict RequiresHumanApproval | operator | reflexive accept; accepting a superseded plan | S | PN-07 |
| 4.3 Override with a substitute assignment | resources and readiness (`ResourceView`), the substitute checked by policy | operator | substitute not policy-checked; wrong resource | M | PN-07 |
| 4.4 Reject with a reason | reason text | operator | no reason recorded | S | PN-07 |
| 4.5 See the record | `DecisionRecord`, `CommandEvent::Decided` | none | uncertainty whether the decision took | S | PN-06, PN-05 |

## T-op-5 Handle alerts (OA-13)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 5.1 Notice a new alert without losing the queue | `AlertLifecycle` state New, severity, count on the strip | none | alert storm; modal interruption during a decision | S | PN-01, PN-08 |
| 5.2 Acknowledge | transition New to Acknowledged with operator and time | operator | acknowledging in bulk what needed reading | M | PN-08 |
| 5.3 Escalate | transition to Escalated | operator | escalating without a note | M | PN-08 |
| 5.4 Acknowledge a degraded state on a plan | degraded flag on the plan (MT-07 step 5) | operator or supervisor | deciding on hidden staleness | S | PN-07 |

## T-op-6 Cue a camera (OA-12, under the sensor manager's rules)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 6.1 Request a cue on a low-confidence track | sensor list with modes, the track | operator (cue only) | cueing a sensor the manager has tasked elsewhere | M | PN-04, PN-10 read |

## T-op-7 Operate disconnected (OA-27, MT-10)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 7.1 Notice the fallback | backend chip Detached, queued and dropped counts | none | not noticing; believing the node picture is current | S | PN-01 |
| 7.2 Continue under the delegation in force | delegation with expiry | operator | acting after expiry | M | PN-01, PN-07 |
| 7.3 See the outbox and queue | `outbox_len`, `dropped`, store-and-forward length | none | silent drops | M | PN-01, PN-09 |
| 7.4 Notice reconnection and resynchronisation | backend chip Node connected; conflicts handled by the supervisor | none | acting during resynchronisation on a half-applied projection | M | PN-01 |

## Error modes that shape the design

- Reflexive acceptance: the accept button is last in tab order, never default, and
  disabled while the plan shown differs from the plan proposed (superseded).
- Hidden staleness: staleness is an age label beside every glyph and a column in
  the table; a stale track's plan cannot be accepted without the degraded-state
  acknowledgement.
- Alert storms: correlated incidents (SV-24) are what the operator sees; raw alerts
  are in the incident's history.
- Delegation confusion: the delegation card is on the strip and repeated in the
  dialog header.

## Traceability

- Activities OA-02, OA-03, OA-05, OA-06, OA-07, OA-12, OA-13, OA-27; capabilities
  CAP-2.2, CAP-2.6, CAP-3.5, CAP-3.6, CAP-4.1, CAP-4.2, CAP-5.4, CAP-5.5; wireframes
  WF-01 to WF-09; flows FL-01, FL-02, FL-05, FL-07.
