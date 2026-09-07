# Personas

Status: first draft, 2026-09-04. One persona per role, written from the roles
document and the vignettes, not from interviews; the plan 02 subject-matter
reviewers validate them. Names are fictional. Each persona is a composite for the
role, not a job description.

## P-01 Operator: Mira, air defense operator, sector command post

- **Goal.** Nothing predicted to reach a listed asset gets there without an
  engagement or a warning, and she never engages a friendly or civil track.
- **Environment.** A dim room with two screens, a headset, a supervisor behind her,
  a second operator beside her working the land picture. Shifts of eight to twelve
  hours; raids come at night.
- **Tempo.** Minutes of routine, then two hours at two to four decisions a minute
  (VG-01). She reads the queue faster than she reads the map.
- **Tools.** The picture, the track table, the recommendation panel, the queue, her
  delegation card (which classes and layers she may accept alone).
- **Decisions owned.** Identity confirmation for the classes policy assigns her;
  accept, override, reject within delegation; alert acknowledgement; camera cue.
- **Frustrations.** A plan that changes under her cursor; a track that looks
  current but is thirty seconds stale; a decoy that spends an interceptor; an alert
  storm during a raid; not knowing whether the node or her own desktop is live.
- **What she must never be able to do.** Accept a plan with one reflexive keystroke;
  act on a plan whose verdict she has not seen.

## P-02 Supervisor: Tomas, sector supervisor

- **Goal.** The queue never outruns the authority; the sector's weapons control
  status matches the situation; nobody decides on hidden staleness.
- **Environment.** The command post, standing or at a console with the whole
  sector's queues, incidents, and coverage; on the radio to sites and higher command.
- **Tempo.** Continuous; peaks with raids; the one who is interrupted most.
- **Tools.** All operator panels plus sensor management and the configuration
  editor; the incident list rather than raw alerts.
- **Decisions owned.** Weapons control status per layer; hold or cease; queue
  priorities and pre-delegation; degraded-state acknowledgement; reconciliation
  conflicts; plan apply; escalation to the commander.
- **Frustrations.** Raw alerts instead of incidents; not seeing who holds which
  authority right now; a status change he cannot see propagate; conflicts resolved
  silently after a link loss.

## P-03 Analyst: Ines, after-action analyst

- **Goal.** Every shift's record turns into measures, lessons, and better baselines.
- **Environment.** A quiet office next to the command post; a workstation against
  the node's journal; the battle rhythm's reporting deadlines.
- **Tempo.** Offline; hours per session; no time pressure except the reporting
  cycle.
- **Tools.** Replay with scrubbing, reports, measures, the model registry's
  validation evidence.
- **Decisions owned.** What to replay and report; model promotion and rollback with
  the supervisor's concurrence; lessons into the gap register.
- **Frustrations.** A replay that does not match the live session; a report figure
  she cannot trace to the journal; a model promoted without evidence.

## P-04 Sensor manager: Pieter, sensor manager

- **Goal.** The sector always knows what it can see, for how long, and what it
  cannot; coverage is restored or the gap is formally accepted.
- **Environment.** The command post, with the sensor operators and maintainers on
  another channel; under electronic attack during raids (VG-07).
- **Tempo.** Minutes; continuous under attack; the one who reacts first to
  degradation.
- **Tools.** Sensor states and modes, coverage over terrain with gaps, clock health,
  tasking requests, calibration baselines.
- **Decisions owned.** Modes and tasking; calibration baselines; re-laydown
  proposals; collection tasking that costs defense coverage, with the supervisor's
  concurrence.
- **Frustrations.** Coverage shown as numbers instead of on the map; a sensor
  silently reporting garbage; a mode change whose effect he cannot see until the
  next raid; tasking requests arriving by voice with no record.

## P-05 Administrator: Sven, system administrator

- **Goal.** Baselines are valid before they apply; accounts and roles match the
  establishment; the node and journals are healthy; the audit record is complete.
- **Environment.** The command post's back room and the node's host; offline from
  the engagement chain.
- **Tempo.** Offline; hours; the battle rhythm's maintenance windows.
- **Tools.** The configuration editor with validation, accounts and roles, node
  health, journal state, the audit log.
- **Decisions owned.** Accounts and roles; baseline management; node operation;
  retention. Never an engagement decision.
- **Frustrations.** A baseline applied without validation; an audit entry missing
  for an action he can see happened; retention silently purging a session under
  review.

## P-06 Intelligence analyst: Leyla, sector intelligence analyst

- **Goal.** Every hostile declaration has recorded evidence; every requirement is
  tasked or declined; peers receive products with the right marking.
- **Environment.** The command post; the afternoon picture (VG-08) with airliners,
  fighters, and a loitering ISR UAS; peers on the other end of the link.
- **Tempo.** Continuous; identification within the decision window of the thread it
  serves; products at the battle rhythm.
- **Tools.** Evidence per track, entity lineage, requirements and their status,
  replay, reports, peer products; a requirements panel that does not exist yet.
- **Decisions owned.** Identity declarations for the intelligence function; merges
  and splits; product release with marking.
- **Frustrations.** An identity assigned on one weak source; evidence not retained
  with the track; a requirement stated by voice and lost; a product released
  without marking.

## P-07 Planner: Anders, sector planner

- **Goal.** A validated, rehearsed plan in force with known and accepted gaps; every
  change audited.
- **Environment.** Monday morning with the commander, sensor manager, and analyst
  (VG-09); a new threat estimate; a maintenance window to plan around.
- **Tempo.** Hours to days; a plan cycle per battle rhythm; rehearsal before each
  shift.
- **Tools.** Coverage over terrain for laydown options, resources and readiness,
  the defended-asset list, the policy configuration, replay for rehearsal; a
  planning panel that does not exist yet.
- **Decisions owned.** Laydown proposals; plan drafts; rehearsal design.
- **Frustrations.** Comparing two laydowns by eye; a rehearsal that cannot use the
  live pipeline; a plan applied under time pressure without the rehearsal.

## P-08 Commander: Colonel Reyes, sector commander

- **Goal.** The right assets are protected in the right order; delegations are
  clear; gaps are accepted knowingly, not discovered afterwards.
- **Environment.** Present at the command post for raids, otherwise reachable; the
  one who is called when the supervisor's delegation runs out.
- **Tempo.** Minutes when present; the battle rhythm otherwise.
- **Tools.** The summary picture, the queue's state, coverage and accepted gaps, the
  plan in force, outcomes; the supervisor's set read-mostly plus the approval queue.
- **Decisions owned.** Defended-asset priorities; authority delegation; accepted
  coverage gaps; plan approval; direct engagement decisions when present.
- **Frustrations.** A summary that hides the queue's state; not knowing which
  delegations are in force at which site; being asked to accept a gap without
  seeing it on the map.

## Cross-cutting observations

- Five of eight personas decide under time pressure measured in seconds; three work
  in hours. The layouts separate the two tempos: live layouts optimize for the queue
  and the map; offline layouts optimize for the timeline and the table.
- Every persona names backend and staleness confusion as a frustration; the status
  strip (PN-01) exists for all eight.
- Three personas (P-06, P-07, P-08) have no panel in the code today; their designs
  are complete so that GAP-068 and the panel gaps can be built from them.
