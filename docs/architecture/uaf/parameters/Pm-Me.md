# Pm-Me Measurements

**UAF definition.** The parametric measurements view defines the measurable
properties of elements and the values they must meet.

**Purpose here.** The measures that judge the architecture, tied to the elements
they measure: the measures of effectiveness and performance from the mission set
(confirmed by the owner under D-16) and the performance budgets. Read by the
verification engineer and plan 10 (architecture requirements).

Status: first draft, 2026-09-04. Values are the confirmed ones; the definitions
and methods are in `../../../mission/capabilities/measures-catalogue.md` and
`../../../mission/measures.md`; the budgets in `../../../performance-budgets.md`.

## Measures of effectiveness (mission outcomes)

| Measure | Element measured | Value |
|---|---|---|
| MOE-01 Defended-asset protection | CAP-3.3, CAP-4.5, CAP-4.6 | 0.95 for priority-1 and priority-2 assets |
| MOE-02 No fratricide, no civil engagement | CAP-2.6, CAP-3.6 | 0 (absolute) |
| MOE-03 Cost discipline | CAP-3.3 | at least 0.9 |
| MOE-04 Decision timeliness | CAP-3.7, CAP-4.1 | p95 under 0.3 of remaining time |
| MOE-05 Decision completeness | CAP-3.6, CAP-4.2, CAP-4.3 | 1.0 (absolute) |
| MOE-06 Picture honesty | CAP-1.2, CAP-2.2, CAP-3.2 | 0 (absolute) |
| MOE-07 Surface picture completeness | CAP-2.5 | 0.98 |
| MOE-08 Fires timeliness | CAP-3.8 | 0.9 |
| MOE-09 Identity continuity | CAP-2.7 | 0.9 |
| MOE-10 Degradation recovery | CAP-1.4, CAP-3.9, CAP-5.5 | 30 s to show; battle rhythm to accept |
| MOE-11 Continuity under disconnection | CAP-5.4 | 1.0 reached; 1.0 resolved (absolute) |
| MOE-12 Rehearsal effect | CAP-5.2, CAP-5.3, CAP-5.8 | rehearse every plan change |
| MOE-13 Intelligence product timeliness | CAP-2.12, CAP-5.8 | 0.95 of scheduled |

## Measures of performance (system behaviour)

| Measure | Element measured | Value |
|---|---|---|
| MOP-01 Detection to display | SV-01, SV-08, RS-app | p99 under 250 ms |
| MOP-02 Detection to node publish | SV-23, RS-node | p99 under 150 ms on-prem, 400 ms cloud |
| MOP-03 Track continuity | SV-01 | per verification row |
| MOP-04 False-track rate | SV-01 | under 1 per hour (Scenario 2) |
| MOP-05 Identification timeliness | SV-12 | p90 within the first third of the warning time |
| MOP-06 Plan recompute | SV-02 | p99 under 4 ms embedded; last good plan beyond |
| MOP-07 Decision path latency | SV-15, SV-16, SV-27 | under 500 ms |
| MOP-08 Health reporting latency | SV-24 | under 5 s |
| MOP-09 Clock skew detection | SV-05 | under 30 s |
| MOP-10 Journal durability | SV-04 | under 100 ms |
| MOP-11 Fallback to embedded | SV-30 | under 2 s |
| MOP-12 Reconciliation time | SV-25 | under 60 s for a 10-minute outage |
| MOP-13 Store-and-forward capacity | SV-30, SV-25 | 100,000 detections |
| MOP-14 Coverage recompute | SV-14 | under 10 s |
| MOP-15 Replay determinism | SV-28 | exact |
| MOP-16 Frame rate with the viewport open | RS-viewport3d, RS-ui | 60 fps, never below 30 |
| MOP-17 Ingest throughput, node | SV-08, RS-node | 5,000 per second |
| MOP-18 Alert correlation | SV-24 | never more incidents than raw alerts |
| MOP-19 to MOP-42 | as catalogued | as confirmed on 2026-09-04 (three deferred to plans 06, 07, 08) |

## Budgets not otherwise measured

| Budget | Element | Value |
|---|---|---|
| Per-frame `update()` | RS-app | p99 under 4 ms |
| egui pass | RS-ui | p99 under 8 ms at Scenario 4 |
| `tracks()` and `is_healthy()` | SV-01 | p99 under 1 ms |
| Memory, steady state, 1 hour | RS-app | under 1.5 GB without point clouds |
| Startup to first frame | RS-app | under 3 s |
| Snapshot request | SV-23 | p99 under 50 ms for 500 tracks |
| Recovery after restart | RS-node | under 30 s |

## Elements used

- The capabilities, services, and resources named.

## Notes

- None of the harnesses that measure the budgets exists yet (GAP-056); the values
  are provisional gates (D-04) that become gates as their tests land (GAP-067).

## Traceability

- Derives from: `../../../mission/measures.md`,
  `../../../mission/capabilities/measures-catalogue.md`,
  `../../../performance-budgets.md`, `../../../verification-capability-table.md` §2.
- Feeds: the verification rows, plan 10 architecture requirements, plan 07 (truth
  data for MOP-25).
