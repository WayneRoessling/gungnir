# Task analysis: Planner (P-07)

Adopted role (D-05); panels wait on GAP-068. Thread: MT-09. Layout: PN-16, PN-11,
PN-14, PN-12 beside the viewport; PN-10 read-only, PN-19 on demand.

## T-pl-1 Draft the defended-asset list (OA-21, for the commander)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 1.1 List assets with positions, priorities, warning obligations | asset list section of the baseline (GAP-026); assets drawn on the map | none | an asset with no warning obligation | H | PN-16, PN-02 |
| 1.2 Propose priority changes | a draft for the commander's approval | planner drafts | draft applied as if approved | H | PN-16 |

## T-pl-2 Plan the laydown (OA-22)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 2.1 Create a laydown option | sensor and effector positions and modes; resources and readiness (`ResourceView`) | planner | an option with an unready resource | H | PN-16 |
| 2.2 Compute coverage and gaps over terrain | coverage per option; gaps along approaches (GAP-006); line of sight (`gungnir-analytics`) | none | comparing by eye | H | PN-11, PN-02 |
| 2.3 Compare two options side by side | both coverages and their gap lists | none | switching layers back and forth | H | PN-16, PN-11 |
| 2.4 Account for a maintenance window | the window; coverage during it; the gap to accept | planner proposes, commander accepts | the window's gap discovered during the raid | H | PN-16 |

## T-pl-3 Configure policy for the plan (OA-23, with the commander)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 3.1 Draft identification criteria, weapons control status defaults, delegations, restrictions as geofences | policy section (GAP-052); geofences drawn | planner drafts, commander approves | a restriction not representable as a geofence | H | PN-14, PN-02 |
| 3.2 Set the plan's validity window | validity period (GAP-052) | planner | a plan with no expiry | H | PN-14 |

## T-pl-4 Validate and rehearse (OA-24, OA-25)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 4.1 Validate the baseline | validation result at the field | none | rehearsing an invalid plan | H | PN-14 |
| 4.2 Replay a scenario through the pipeline under the plan | test-track scenario (plan 07) replayed with the plan applied (GAP-045) | none | rehearsal that cannot use the live pipeline | H | PN-12, PN-02 |
| 4.3 Review the recommendations and gaps the rehearsal produced | the queue as it would have been; coverage gaps hit | none | rehearsal skipped under time pressure | H | PN-06 (replay), PN-11 |
| 4.4 Submit for approval | the plan with its rehearsal record | planner submits, commander or supervisor applies | applied without the rehearsal record | H | PN-16 |

## Error modes that shape the design

- The planning panel compares options in one view with the same terrain and the
  same scenario, so the difference is on screen, not in memory.
- The submit control carries the rehearsal record and is disabled without one
  unless the commander overrides with a reason.
- Every restriction is drawn on the map as it is typed, so an unrepresentable one
  is noticed while drafting.

## Traceability

- Activities OA-21, OA-22, OA-23, OA-24, OA-25; capabilities CAP-1.4, CAP-3.1,
  CAP-5.2, CAP-5.6; wireframes WF-11, WF-14, WF-16; flows FL-03, FL-04.
