# Measures catalogue

Status: first draft, 2026-09-04. Every measure used by the capability statements.
MOE-01 to MOE-12 and MOP-01 to MOP-18 are defined in `../measures.md` and repeated
here only by name; MOE-13 and MOP-19 to MOP-42 are introduced by the capability
statements and defined here. Source marks: *budget* (`../../performance-budgets.md`),
*table* (`../../verification-capability-table.md`), *doctrine* (a public doctrinal
absolute), *confirmed 2026-09-04* (proposed by the drafting agent and confirmed or adjusted by
the owner that day, D-16), *deferred* (the named plan proposes the value with its own
evidence and the owner signs off there).

## Measures from the mission analysis

| Id | Name | Capabilities |
|---|---|---|
| MOE-01 | Defended-asset protection | CAP-3.3, CAP-4.5, CAP-4.6 |
| MOE-02 | No fratricide, no civil engagement | CAP-2.6, CAP-3.6 |
| MOE-03 | Cost discipline | CAP-3.3 |
| MOE-04 | Decision timeliness | CAP-3.7, CAP-4.1 |
| MOE-05 | Decision completeness | CAP-3.6, CAP-4.2, CAP-4.3 |
| MOE-06 | Picture honesty | CAP-1.2, CAP-2.2, CAP-3.2 |
| MOE-07 | Surface picture completeness | CAP-2.5 |
| MOE-08 | Fires timeliness | CAP-3.8 |
| MOE-09 | Identity continuity | CAP-2.7 |
| MOE-10 | Degradation recovery | CAP-1.4, CAP-3.9, CAP-5.5 |
| MOE-11 | Continuity under disconnection | CAP-5.4 |
| MOE-12 | Rehearsal effect | CAP-5.2, CAP-5.3, CAP-5.8 |
| MOP-01 | Detection to display | CAP-2.1, CAP-5.10 |
| MOP-02 | Detection to node publish | CAP-5.10, CAP-7.1 |
| MOP-03 | Track continuity | CAP-2.1, CAP-2.2 |
| MOP-04 | False-track rate | CAP-2.5 |
| MOP-05 | Identification timeliness | CAP-2.6 |
| MOP-06 | Plan recompute | CAP-3.3 |
| MOP-07 | Decision path latency | CAP-3.5, CAP-4.1 |
| MOP-08 | Health reporting latency | CAP-5.5 |
| MOP-09 | Clock skew detection | CAP-1.5 |
| MOP-10 | Journal durability | CAP-5.1 |
| MOP-11 | Fallback to embedded | CAP-5.4, CAP-7.3 |
| MOP-12 | Reconciliation time | CAP-5.4 |
| MOP-13 | Store-and-forward capacity | CAP-5.4 |
| MOP-14 | Coverage recompute | CAP-1.3, CAP-1.4, CAP-2.11 |
| MOP-15 | Replay determinism | CAP-1.5, CAP-5.2 |
| MOP-16 | Frame rate with the viewport open | CAP-2.4, CAP-2.10, CAP-5.10 |
| MOP-17 | Ingest throughput, node | CAP-1.1, CAP-5.10 |
| MOP-18 | Alert correlation | CAP-2.9, CAP-5.5 |

## Measures introduced by the capability statements

| Id | Name | Definition | Unit | Method | Target | Source |
|---|---|---|---|---|---|---|
| MOE-13 | Intelligence product timeliness | Fraction of scheduled products (handover summaries, situation reports, order-of-battle updates) delivered within the battle rhythm | fraction | Journal and report timestamps | 0.95 of scheduled | confirmed 2026-09-04 |
| MOP-19 | Provenance completeness | Fraction of accepted observations carrying source sensor, source time, receipt time, and calibration baseline | fraction | Gateway test and journal audit | 1.0 for source sensor, source time, and receipt time; calibration baseline optional, a default baseline recorded when absent | confirmed 2026-09-04 |
| MOP-20 | Quarantine integrity | Count of inputs from the fuzz corpus and validation tests that reach the tracking service despite failing a rule | count | `gungnir-ingest` tests and the fuzz target | 0 | table |
| MOP-21 | Mode change to coverage | Time from a sensor mode change to the coverage display reflecting it | seconds | Tick harness | under 10 | confirmed 2026-09-04 |
| MOP-22 | Peer track transparency | Fraction of peer-supplied tracks shown with their source, latency, and staleness | fraction | UI review and journal | 1.0 for source; latency and staleness where the peer supplies timing | confirmed 2026-09-04 |
| MOP-23 | Cooperative identity attachment | Time from a cooperative report to the identity evidence appearing on the track, p95 | update cycles | Test with recorded cooperative feeds | within 2 | confirmed 2026-09-04 |
| MOP-24 | Evidence retention | Fraction of hostile declarations with the contributing evidence retained and inspectable | fraction | Journal audit | 1.0 | doctrine |
| MOP-25 | Prediction error | Error of predicted impact point and time, and of closest point of approach, against test-track truth, per class | metres, seconds | Test tracks with truth | per class, proposed in `../../test-tracks/validation.md` §7 (2026-09-04); owner to confirm | plan 07 |
| MOP-26 | Anomaly flag latency | Time from an anomaly's onset in TT-05 to its alert | seconds | Replay of TT-05 | under 120 | confirmed 2026-09-04 |
| MOP-27 | Priority change propagation | Ticks from a defended-asset priority change to its effect on scores | ticks | Tick harness | 2 | confirmed 2026-09-04 |
| MOP-28 | Score monotonicity | Score non-decreasing in asset priority and non-increasing in time to impact on test tracks | pass or fail | Assessment tests | pass | table |
| MOP-29 | Rationale presence | Fraction of presented plans carrying a rationale | fraction | UI test | 1.0 | table (absolute); confirmed 2026-09-04 |
| MOP-30 | Queue integrity | Count of pending decisions lost or reordered without a record | count | Command tests | 0 | table (absolute); confirmed 2026-09-04 |
| MOP-31 | No execution without decision | Count of code paths, found by review and test, by which an effector or a draft could be executed without a decision record | count | Review checklist and integration test | 0 | doctrine |
| MOP-32 | Handoff latency | Time from decision record to the handoff message leaving the node | seconds | Transport test | under 0.5 | confirmed 2026-09-04 |
| MOP-33 | Warning latency | Time from a prediction crossing the warning threshold to the warning issued | seconds | Tick harness | under 5 | confirmed 2026-09-04 |
| MOP-34 | Outcome recording | Update cycles from an observed engagement outcome to its record | update cycles | Replay | within 2 | confirmed 2026-09-04 |
| MOP-35 | Assistant accuracy and resistance | Factual accuracy on the assistant evaluation set; fraction of injection cases resisted | fraction | Plan 08 evaluation harness | set by plan 08 with its evaluation set | deferred to plan 08 (owner sign-off there) |
| MOP-36 | Baseline validation | Count of invalid baselines applied | count | Config tests | 0 | table |
| MOP-37 | Usability | Time to acknowledge, decision latency, error rate, workload rating per role in scenario tasks | seconds, count, rating | Plan 06 usability tests | set by plan 06 after the baseline sessions; **round 1 re-planned onto the built panels 2026-09-06 (D-28)**, proposal = round-1 median with the worst case as floor, per measure | deferred to plan 06 (owner sign-off there) |
| MOP-38 | Authorization enforcement | Authority matrix cases enforced under test | fraction | Security tests | 1.0 | table |
| MOP-39 | Audit completeness | Audit entries per gated action | ratio | Audit tests | 1.0 | table (absolute); confirmed 2026-09-04 |
| MOP-40 | Data-protection compliance | Open findings in the compliance assessment for encryption in transit and at rest | count | Plan 10 compliance assessment | 0 open high or critical findings at release | confirmed 2026-09-04 |
| MOP-41 | Release gates | Release workflow gates passed per release | fraction | `release.yml` | 1.0 | table |
| MOP-42 | Releasability enforcement | Products released without marking | count | API tests | 0 | table (absolute); confirmed 2026-09-04 |

## Open items

- Every target was confirmed or set by the owner on 2026-09-04 except three deferred to
  the plan that produces their evidence. **MOP-25 was confirmed on 2026-09-05** at plan
  07's proposed per-class values (`../../test-tracks/validation.md` §7), with the caveat
  recorded there that the tolerances assume the filter-based predictor of GAP-011 and
  that the ballistic and glide-bomb rows are not expected to be met by the
  constant-velocity predictor that exists today.
- MOP-35 (plan 08) and MOP-37 (plan 06) remain open, and cannot be set here: MOP-35
  needs plan 08's evaluation harness and MOP-37 needs plan 06's baseline usability
  sessions, and neither exists. They are deferred to evidence, not to a decision.
