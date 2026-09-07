# Task analysis: Intelligence analyst (P-06)

Adopted role (D-05); panels wait on GAP-068. Threads: MT-08 (collection and
evidence), MT-06 (class confirmation). Layout: PN-15, PN-04, PN-03, PN-13 beside
the viewport; PN-12, PN-19 on demand.

## T-ia-1 State and manage requirements (OA-17)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 1.1 State a requirement | area (drawn on the map), question, priority, time window (GAP-005) | intelligence analyst (commander for priority) | requirement without a window | H | PN-15, PN-02 |
| 1.2 Track its status | tasked, declined with reason, collected, satisfied | none | requirement lost between roles | B | PN-15 |
| 1.3 Close or renew | status change recorded | intelligence analyst | stale requirements consuming coverage | B | PN-15 |

## T-ia-2 Fuse evidence and declare (OA-18, OA-03)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 2.1 Read every piece of evidence on a track | `IdentificationEvidence` with source, weight, time; cooperative identity; kinematics; emitter class | none | one weak source dominating | M | PN-04 |
| 2.2 Compare against the class's criteria | per-class threshold and margin (GAP-018) | none | declaring below the margin | M | PN-04 |
| 2.3 Declare for the intelligence function | declaration as evidence with the analyst's identity; retained (MOP-24) | intelligence analyst | declaration without retained evidence | M | PN-04 (dialog) |
| 2.4 Confirm a vehicle class from video (MT-06) | video observation evidence; the entity | intelligence analyst | class confirmed on a stale frame | M | PN-04 |

## T-ia-3 Maintain entities and the order of battle (OA-19)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 3.1 See an entity's lineage | `GlobalEntityId`, session tracks, merges and splits with times | none | a merge hidden inside a track id | H | PN-04 |
| 3.2 Merge or split | merge or split recorded with reason | intelligence analyst | merging two entities on a coincidence | H | PN-04 (dialog) |
| 3.3 Read the order of battle and pattern of life | products across sessions (GAP-025) | none | products only in memory | B | PN-13 |

## T-ia-4 Disseminate (OA-20)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 4.1 Assemble a product | report with identities, warnings, order of battle | none | figures without provenance | B | PN-13 |
| 4.2 Mark releasability | the marking field (D-06, GAP-062) | intelligence analyst | releasing unmarked (MOP-42) | B | PN-13 (dialog) |
| 4.3 Release to roles and peers | release recorded; peers receive through the API (GAP-065) | intelligence analyst | release to the wrong peer | B | PN-13 |

## T-ia-5 Work with replay (OA-25)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 5.1 Replay a session to build evidence | as the analyst's T-an-1 | none | mixing replayed and live | H | PN-12 |

## Error modes that shape the design

- The evidence card lists every piece of evidence with its weight and never shows a
  classification without the evidence beneath it.
- The declaration dialog shows the margin and refuses below it (SV-12 semantics),
  with an "escalate to supervisor" path rather than an override.
- Release requires a marking; the control is disabled without one.

## Traceability

- Activities OA-03, OA-17, OA-18, OA-19, OA-20, OA-25; capabilities CAP-1.3,
  CAP-2.6, CAP-2.7, CAP-2.12, CAP-6.6, CAP-7.4; wireframes WF-04, WF-13, WF-15;
  flows FL-07, FL-03.
