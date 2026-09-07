# Pr-Tx Personnel taxonomy

**UAF definition.** The personnel taxonomy view presents the types of person and
organization in the architecture as a hierarchy.

**Purpose here.** The eight personnel types, their code representation, and where
each sits; the same list as the role performers in Op-Tx, seen as people to staff,
train, and authorize.

Status: first draft, 2026-09-04.

## Personnel types

| Type | Performer | In code | Decisions held (summary) |
|---|---|---|---|
| PT-01 Operator | OP-01 | `Role::Operator` | identity for assigned classes; engagements within delegation; alert acknowledgement; camera cue |
| PT-02 Supervisor | OP-02 | `Role::Supervisor` | weapons control status; hold and cease; queue priorities; degraded-state acknowledgement; conflict resolution; plan apply; escalation |
| PT-03 Analyst | OP-03 | `Role::Analyst` | replay and reports; model promotion with concurrence; lessons |
| PT-04 Sensor manager | OP-04 | `Role::SensorManager` | modes and tasking; calibration baselines; laydown proposals; collection tasking with concurrence |
| PT-05 Administrator | OP-05 | `Role::Administrator` | accounts and roles; baselines; node operation; retention; no engagement decisions |
| PT-06 Intelligence analyst | OP-06 | not yet (GAP-068) | intelligence identity declarations; merges and splits; product release with marking |
| PT-07 Planner | OP-07 | not yet (GAP-068) | laydown proposals; plan drafts; rehearsal design |
| PT-08 Commander | OP-08 | not yet (GAP-068) | defended-asset priorities; authority delegation; accepted gaps; plan approval; direct decisions |

Organization types: the sector command post (OP-10), site defense cells (OP-11),
and port defense cells (OP-12) are the organizational units the types are posted
to (Pr-Sr).

## Elements used

- PT-01 to PT-08; OP-01 to OP-08, OP-10 to OP-12.

## Notes

- The three adopted types have no `Role` variant until GAP-068; until then their
  decisions are held by PT-02 (commander's decisions and plan approval), PT-03
  (intelligence), and PT-04 with PT-02 (planning).
- Competences per type are in Pr-Rm.

## Traceability

- Derives from: `gungnir_security::Role`; `../../../mission/roles-and-stakeholders.md` §1 and §2; D-05.
- Feeds: Pr-Sr, Pr-Cn, Pr-Rm, Sc-Pr (the authorization matrix), plan 06 personas.
