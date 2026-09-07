# Task analysis: Commander (P-08)

Adopted role (D-05); panels wait on GAP-068. Threads: MT-01, MT-02 (direct
decisions and delegation), MT-07 (accepted gaps), MT-09 (priorities and plan
approval). Layout: PN-17, PN-06, PN-08 (incidents), PN-09 beside the viewport;
PN-04, PN-05, PN-07, PN-16 on demand.

## T-cd-1 Hold the summary (OA-13)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 1.1 See the queue's state without the queue's detail | pending count, oldest time remaining, decisions per minute, expiries | none | a summary that hides saturation | M | PN-17 |
| 1.2 See delegations in force per site | delegation rules and expiries (D-15) | none | not knowing what a site may do alone | M | PN-17 |
| 1.3 See accepted gaps and the plan in force | accepted gaps with who accepted and until when; plan version and validity | none | a gap accepted and forgotten | M | PN-17, PN-02 |
| 1.4 See outcomes | engagements decided, outcomes recorded (GAP-043) | none | outcomes only by voice | M | PN-17 |

## T-cd-2 Set priorities (OA-21)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 2.1 Change an asset's priority | asset list; the effect on scores within two ticks (MOP-27) | commander | change not propagated | M | PN-16 (dialog) |

## T-cd-3 Delegate (OA-23)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 3.1 Delegate a class and layer to a role or site | the delegation rule; what the rule forbids (area layer, missiles) | commander | delegating what D-15 forbids (the dialog refuses) | M | PN-17 (dialog) |
| 3.2 Revoke or let expire | expiry shown | commander | a delegation outliving the situation | M | PN-17 |

## T-cd-4 Accept a coverage gap (OA-13, MT-07)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 4.1 See the gap on the map with the assets under it | gap polygon, assets, warning obligations | none | accepting a gap described in words only | M | PN-02, PN-11 |
| 4.2 Accept with a warning obligation and an expiry | accepted gap recorded | commander | acceptance without expiry | M | PN-17 (dialog) |

## T-cd-5 Approve a plan (OA-24)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 5.1 Review the plan with its rehearsal record | the baseline, validation, rehearsal results | none | approving without rehearsal | H | PN-16, PN-14 |
| 5.2 Approve and apply | apply with audit | commander (or supervisor) | applying during a raid | H | PN-14 (dialog) |

## T-cd-6 Decide directly when present (OA-07)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 6.1 Take a decision from the queue | as the operator's T-op-4 with full authority | commander | two deciders on one plan (arbitration by role rank) | S | PN-06, PN-07 |

## Error modes that shape the design

- The summary panel's numbers are links to the panel that holds the detail, so a
  worrying number is one click from its cause.
- Delegation and gap-acceptance dialogs always carry an expiry field that defaults
  to the battle rhythm, never to "indefinite".
- The commander's direct decision uses the same dialog as the operator's, with the
  delegation check replaced by the authority mark.

## Traceability

- Activities OA-07, OA-13, OA-21, OA-23, OA-24; capabilities CAP-3.1, CAP-3.6,
  CAP-4.2, CAP-5.5, CAP-5.6; wireframes WF-06, WF-07, WF-16, WF-17; flows FL-02,
  FL-08.
