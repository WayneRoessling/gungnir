# Pr-Rm Competence and training roadmap

**UAF definition.** The personnel roadmap view shows how personnel, competences,
and training evolve over time.

**Purpose here.** What each role must know to use each increment, and what training
the increment needs, so that fielding is planned alongside the software. Read by the
owner and by a customer's training staff.

Status: first draft, 2026-09-04. Proposals; no training material exists yet.

## Competences by role

| Role | Competences | Evidence in the system |
|---|---|---|
| PT-01 Operator | read a picture with uncertainty and staleness; identification evidence; the recommendation panel (verdict, rationale, alternatives, time remaining); delegation limits; alert handling | MOP-37 usability measures (plan 06) |
| PT-02 Supervisor | queue management under saturation; weapons control status; holds; degraded-state acknowledgement; reconciliation conflicts; plan apply | MOE-04, MOE-06, MOE-11 |
| PT-03 Analyst | replay, reports, the measures catalogue, model validation evidence | MOE-12 |
| PT-04 Sensor manager | modes and transitions, coverage over terrain, calibration, re-tasking under attack, clock health | MOE-10, MOP-14, MOP-21 |
| PT-05 Administrator | baselines and validation, accounts and roles, node operation, journal retention, audit review | MOP-36, MOP-39 |
| PT-06 Intelligence analyst | evidence standards per class, entity lineage, requirements and tasking, releasability marking | MOP-24, MOE-13 |
| PT-07 Planner | laydown and coverage analysis, policy configuration, rehearsal design | MOE-12 |
| PT-08 Commander | the authority model, delegation, accepted gaps, the summary picture | MOE-05 |

## Training by increment

| Increment | Training needed | Roles |
|---|---|---|
| PJ-I1 (now) | replay and reports on recorded sessions; the honesty conventions (stale, degraded, unimplemented) | PT-03, PT-05 |
| PJ-I2 | live picture with real sensors; sensor modes and coverage; identification evidence | PT-01, PT-04, PT-06 |
| PJ-I3 | the full decision loop: queue, verdicts, alternatives, delegation, expiry; fires tasks; rehearsal under a plan | PT-01, PT-02, PT-07, PT-08 |
| PJ-I4 | connected operation: failover, reconciliation, peer exchange, releasability; the assistant's limits | all |

Each increment's training is built on that increment's test-track scenarios (plan 07)
replayed through the product (CAP-5.2), so the rehearsal capability doubles as the
training capability.

## Elements used

- PT-01 to PT-08; PJ-I1 to PJ-I4.

## Notes

- Competence profiles are proposals derived from the roles' decisions and the
  measures; a customer's doctrine may reassign them.
- No competence is assumed for the assistant (CAP-4.7) beyond knowing it holds no
  authority.

## Traceability

- Derives from: Pr-Tx, Pr-Cn; St-Rm; `../../../mission/capabilities/measures-catalogue.md`.
- Feeds: plan 06 (usability test design), plan 01 (services and training offer),
  plan 10 phase F (migration planning).
