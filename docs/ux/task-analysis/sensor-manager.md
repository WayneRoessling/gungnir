# Task analysis: Sensor manager (P-04)

Threads: MT-03 (camera cue), MT-07 (degradation), MT-08 (collection tasking),
MT-09 (laydown). Layout: PN-10, PN-11, PN-09, PN-08 beside the viewport; PN-14,
PN-15, PN-19 on demand.

## T-sm-1 Know the sensors' state (OA-12)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 1.1 See every sensor's mode, health, calibration version | `SensorRecord` per sensor; ingest health; clock skew per source (GAP-008) | none | a sensor Offline shown as Standby; garbage accepted as truth | S | PN-10, PN-09 |
| 1.2 See coverage over terrain and the gaps | coverage regions and combined gaps (GAP-006, GAP-007) | none | coverage as numbers only | M | PN-02, PN-11 |
| 1.3 See what each mode would cover | coverage per mode option | none | changing a mode blind | M | PN-11 |

## T-sm-2 Change a mode or task a sensor (OA-12, OA-14)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 2.1 Choose the target mode | `SensorMode::can_transition_to` (the table; Offline and Calibrating return via Standby) | none | requesting an invalid transition (refused by SV-09) | S | PN-10 |
| 2.2 See coverage before and after | the two coverage regions side by side (MOP-21) | none | not seeing the cost of the change | M | PN-11 |
| 2.3 Commit the change | mode change recorded, audited (GAP-059); outbound control (GAP-004) | sensor manager | change recorded locally but never sent | S | PN-10 (dialog) |
| 2.4 Confirm the sensor followed | mode reported back; coverage recomputed within 10 s | none | assuming the change took | M | PN-10, PN-11 |

## T-sm-3 Handle degradation (OA-13, OA-14)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 3.1 See the incident that correlates skew, gaps, and loss | incident with raw alerts inside; the sensors affected | none | reacting to each raw alert | S | PN-08 |
| 3.2 Propose re-tasking to restore the most valuable coverage | recommendation (GAP-037) with coverage before and after | sensor manager | restoring the wrong gap | M | PN-11, PN-10 |
| 3.3 Report the accepted gap to the supervisor | the gap over an asset | none (supervisor and commander decide) | gap accepted by voice only | M | PN-08 |

## T-sm-4 Calibration (OA-12)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 4.1 Record a calibration baseline | `calibration_version` on `SensorRecord`; `Provenance` carries it | sensor manager | tracks produced under an old baseline not distinguishable | H | PN-10 |
| 4.2 Run Calibrating and return to Standby | the transition; coverage suspended meanwhile | sensor manager | calibrating during a raid | H | PN-10 |

## T-sm-5 Collection tasking (OA-17, MT-08)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 5.1 Receive a requirement | requirement object with area, question, priority, window (GAP-005) | none | requirement by voice, lost | M | PN-15 |
| 5.2 Translate into tasking and show the defense-coverage cost | tasking plan; coverage before and after | sensor manager with supervisor concurrence | tasking that uncovers an asset without concurrence | M | PN-15, PN-11 |
| 5.3 Decline with a reason | declined state recorded | sensor manager | requirement silently dropped | M | PN-15 |

## T-sm-6 Laydown (OA-22, with the planner)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 6.1 Propose sensor positions and modes for a laydown option | coverage per option over terrain | sensor manager proposes | comparing options by eye | H | PN-16 (read), PN-11 |

## Error modes that shape the design

- The mode control offers only valid transitions; invalid ones are shown greyed
  with the reason "returns through Standby".
- Every mode change shows coverage before and after in the same view before the
  commit button is enabled.
- The sensor panel's health column is the sensor's own report; a sensor that has
  not reported within the watchdog limit shows "no report for N s", not a stale
  green.

## Traceability

- Activities OA-12, OA-13, OA-14, OA-17, OA-22; capabilities CAP-1.3, CAP-1.4,
  CAP-3.9, CAP-5.5; wireframes WF-10, WF-11, WF-15; flows FL-01, FL-04.
