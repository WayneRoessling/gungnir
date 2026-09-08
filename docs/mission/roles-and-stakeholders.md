# Roles and stakeholders

Status: first draft, 2026-09-04. The five roles in `gungnir-security::Role` are kept
as they are; three roles were proposed and adopted on 2026-09-04 (D-05 in
`gap-analysis/decisions-needed.md`); they enter `gungnir_security::Role` under GAP-068. Adopting the proposed
roles into the code is a separate decision (`mission-analysis.md` §11).

## 1. Roles

| Role | In code | Purpose | Threads |
|---|---|---|---|
| Operator | `Role::Operator` | Runs the live picture for a domain (air, maritime, or land) at a site or the sector: watches tracks and alerts, confirms identities where policy requires, reviews recommendations, and decides engagements within delegated authority | MT-01 to MT-06 |
| Supervisor | `Role::Supervisor` | Runs the sector: manages the queue of recommendations across domains, sets weapons control status, holds engagements for deconfliction, acknowledges degraded states, resolves reconciliation conflicts, applies plans | MT-01, MT-02, MT-07, MT-09, MT-10 |
| Analyst | `Role::Analyst` | Works the recorded record: replay, after-action review, reports, measures, model validation and promotion evidence | MT-09 |
| Sensor manager | `Role::SensorManager` | Owns sensor readiness, modes, coverage, tasking, and calibration; re-tasks under attack; balances collection against defense | MT-03, MT-07, MT-08, MT-09 |
| Administrator | `Role::Administrator` | Owns configuration baselines, accounts and roles, the node, and the audit record; not a decision-maker in the engagement chain | MT-09, MT-10 |
| Intelligence analyst (adopted 2026-09-04) | not yet | Owns requirements and tasking, identification evidence and declarations for the intelligence function, order of battle, pattern of life, and dissemination with releasability | MT-06, MT-08 |
| Planner (adopted 2026-09-04) | not yet | Owns the defended-asset list drafts, laydown and coverage planning, mission plans, and rehearsal | MT-09 |
| Security officer (D-30, 2026-09-06) | `Role::SecurityOfficer` | Holds the escrow key (DN-22 §11) and recovers escrowed journal keys offline; **operates nothing** -- no decision, tasking, configuration, or picture -- and sees the audit record and the health summary only | Recovery after loss of a keystore; an investigation reading a sealed record |
| Commander (adopted 2026-09-04) | not yet | Holds the authorities the others act under: defended-asset priorities, engagement authority delegation, acceptance of coverage gaps, plan approval; may decide directly | MT-01, MT-02, MT-07, MT-09 |

Until GAP-068 lands in the code, their responsibilities fall to the
supervisor (commander's decisions and plan approval), the analyst (intelligence), and
the sensor manager together with the supervisor (planning).

## 2. Role details

### Operator

- **Decisions:** confirm or designate identity for classes policy assigns to the
  operator; accept, override, or reject a recommendation within delegated authority;
  acknowledge alerts; task a camera under cue.
- **Information needs:** the picture for the domain with quality and staleness;
  identities with evidence; the recommendation with verdict, rationale,
  alternatives, and time remaining; weapons control status; own effector readiness;
  friendly tracks and restrictions.
- **Tempo:** seconds to minutes; sustained for hours during a raid.
- **Panels (`gungnir-workflow`):** viewport, track table, intercept panel, approval
  queue, alerts, system health.

### Supervisor

- **Decisions:** weapons control status per layer; hold or cease for deconfliction;
  queue priorities; acknowledge degraded states; resolve reconciliation conflicts;
  apply a validated plan; escalate to the commander.
- **Information needs:** all domains' queues; alert incidents rather than raw alerts;
  coverage and health; the plan in force; who holds which authority right now.
- **Tempo:** continuous; peaks with raids.
- **Panels:** the operator set plus sensor management and configuration editor.

### Analyst

- **Decisions:** what to replay and report; model promotion and rollback with
  supervisor concurrence; lessons into the gap register.
- **Information needs:** journals, replay with scrubbing, report generation,
  measures, model validation evidence.
- **Tempo:** offline; battle rhythm.
- **Panels:** viewport, track table, replay, reports.

### Sensor manager

- **Decisions:** sensor modes and tasking; calibration baselines; re-laydown
  proposals; accept collection tasking that reduces defense coverage, with supervisor
  concurrence.
- **Information needs:** sensor health, modes, coverage over terrain with gaps,
  clock synchronization health, tasking requests.
- **Tempo:** minutes; continuous under attack.
- **Panels:** viewport, sensor management, system health, alerts, configuration
  editor.

### Administrator

- **Decisions:** accounts and roles; baseline management; node operation; retention.
- **Information needs:** configuration validation results, audit log, node health,
  journal state.
- **Tempo:** offline.
- **Panels:** all, for administration; no engagement decisions.

### Intelligence analyst (adopted 2026-09-04)

- **Decisions:** identity declarations for the intelligence function; entity merges
  and splits; product release with marking.
- **Information needs:** evidence per track, lineage, requirements and their status,
  peer products, pattern-of-life views.
- **Panels:** viewport, track table, replay, reports, and a requirements panel not
  yet designed (plan 06).

### Planner (adopted 2026-09-04)

- **Decisions:** laydown proposals; plan drafts; rehearsal design.
- **Information needs:** coverage over terrain, resources and readiness, the
  defended-asset list, policy configuration, rehearsal results.
- **Panels:** viewport with coverage layers, configuration editor, replay; a
  planning panel not yet designed (plan 06).

### Commander (adopted 2026-09-04)

- **Decisions:** defended-asset priorities; authority delegation; accepted gaps;
  plan approval; direct engagement decisions when present.
- **Information needs:** the summary picture, the queue's state, coverage and
  accepted gaps, the plan in force, outcomes.
- **Panels:** the supervisor set, read-mostly, plus the approval queue.

## 3. External stakeholders

| Stakeholder | Interest | Interface |
|---|---|---|
| Higher command | Sector status, warnings, reports, order of battle; sets rules of engagement | `gungnir-api`, reports |
| Neighbouring sectors | Track and warning exchange; raid handover | `gungnir-api`, interop formats |
| Fire units and effector systems | Receive decided assignments; report readiness and outcomes | API handoff (to be defined); readiness in the resource pool |
| Sensor operators and maintainers | Modes, tasking, faults, calibration | Sensor management; health |
| Civil aviation authority and airport | Corridors, flight plans, ADS-B; warnings of engagements | Cooperative data; voice; geofences |
| Port and maritime authorities | AIS, harbour operations, barriers, warnings | Cooperative data; voice |
| Coalition partners | Picture and product exchange with releasability | `gungnir-api`, STANAG 4676, ASTERIX |
| Accreditors and auditors | Evidence that the system enforces policy and records decisions | Audit log, release evidence (`../release-governance.md`) |
| Software owner (Roessling Digital) | Product decisions, support | Plans 01 and 10 |

## 4. Authority matrix (initial, to be confirmed with policy)

| Decision | Operator | Supervisor | Commander (adopted 2026-09-04) | Sensor manager | Analyst | Intelligence analyst (adopted 2026-09-04) |
|---|---|---|---|---|---|---|
| Identity declaration (per class policy) | some classes | all classes | all classes | | | intelligence declarations |
| Engagement acceptance, point layer | delegated | yes | yes | | | |
| Engagement acceptance, area layer | | pre-delegated cases | yes | | | |
| Weapons control status | | yes | yes | | | |
| Hold or cease | | yes | yes | | | |
| Sensor tasking | camera cue | concur | | yes | | request |
| Plan apply | | yes | yes | | | |
| Model promotion | | concur | | | yes | |
| Product release | | yes | yes | | | yes |
| Publish to coalition exchange (GAP-065; confirmed by the owner 2026-09-08, amended the same day to add Supervisor alongside the Product release row above) | | yes | yes | | | yes |
| Accept coverage gap | | | yes | | | |
| Recover an escrowed journal key (D-30) | | | | | | | (the security officer alone; a column of its own would be one cell) |
| Reconciliation conflict resolution | | yes | yes | | | |

`gungnir-security::authz::role_permits` encodes a coarser version of this matrix
today (view, submit, decide, override, task sensors, apply configuration, promote
models, export reports); the per-class and per-layer refinements are GAP-058 and the adopted roles enter the
code under GAP-068 (`gap-analysis/gap-register.md`).
