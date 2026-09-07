# Op-St Operational states

**UAF definition.** The operational states view shows the states an operational
element can be in and the transitions between them.

**Purpose here.** The four lifecycles the operators live with: an alert, a plan
(recommendation) on its way to a decision, a sensor's mode, and a workstation's
backend. Each is drawn from the enum and transition table in the code that owns it,
so the operational and resource descriptions agree.

Status: first draft, 2026-09-04.

Diagram: [`Op-St.puml`](Op-St.puml) (four state machines in one file).

## Alert lifecycle (IE-21, `gungnir_workflow::AlertState`)

New → Acknowledged → Escalated → Closed, with Acknowledged → Closed permitted and
every other jump refused (`InvalidAlertTransition`). Every transition is recorded
with who and when. Correlated incidents (SV-24) enter as one alert, not many
(MT-07 step 1).

## Plan lifecycle (IE-04, IE-19, IE-18)

Proposed (`InterceptEvent::PlanProposed`) → policy-checked (`PolicyVerdict`:
Denied with reason, or RequiresHumanApproval; Approved is reserved for configured
pre-delegation, D-15) → pending in the queue (`ApprovalRequested`) → Decided
(accepted, overridden, rejected; `DecisionRecord`) or expired or escalated (GAP-034)
→ Approved (`PlanApproved`) or Superseded (`PlanSuperseded`) when the picture
changes. No path leads from Proposed to an effector without a Decided record
(CAP-4.3).

## Sensor mode (IE-27, `gungnir_sensor_management::SensorMode`)

Standby, Search, Track, Calibrating, Offline. Any online mode may go Offline;
Offline and Calibrating return only through Standby; every other transition is
permitted (`SensorMode::can_transition_to`). Mode changes are audited (GAP-059) and
recompute coverage (MOP-21).

## Workstation backend (IE-17 `BackendConfig`, `gungnir-remote`)

Embedded (disconnected profile) or Remote (connected). A Remote workstation that
cannot reach its node at startup falls back to Embedded with an alert; mid-session
fallback and return arrive with the heartbeat (GAP-050). While Embedded after a
fallback the outbox and store-and-forward queue fill; on return they drain and the
journals reconcile (MT-10).

## Elements used

- IE-04, IE-17, IE-18, IE-19, IE-21, IE-27; SV-15, SV-16, SV-24, SV-27, SV-30.

## Notes

- The plan lifecycle's expiry and escalation states are design (GAP-034); the code
  today records Accepted, Overridden, Rejected only.
- The backend state machine's mid-session transitions are design (GAP-050).

## Traceability

- Derives from: the enums in `gungnir-workflow`, `gungnir-policy`, `gungnir-command`,
  `gungnir-model::events`, `gungnir-sensor-management`, `gungnir-config`,
  `gungnir-remote`; `../../../mission/air-defense-and-counter-uas.md` §6 and §7.
- Feeds: Rs-St (the same machines seen as resource states), Sv-Pr, plan 06's
  interaction design for the queue and alert panels.
