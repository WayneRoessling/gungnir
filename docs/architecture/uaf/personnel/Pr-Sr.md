# Pr-Sr Organizational structure

**UAF definition.** The personnel structure view shows the organizational
structure: which posts exist in which organization and how they report.

**Purpose here.** The posts in a sector as the vignettes describe it, who reports to
whom for the decisions that matter, and how authority is delegated downward. Read by
plan 06 and by a customer sizing a deployment.

Status: first draft, 2026-09-04. The structure is the vignettes' sector; a real
deployment substitutes its own establishment.

Diagram: [`Pr-Sr.puml`](Pr-Sr.puml).

## Posts

| Organization | Posts (type) | Reports to |
|---|---|---|
| OP-10 Sector command post | Commander (PT-08); Supervisor (PT-02); Air operator and land operator (PT-01); Sensor manager (PT-04); Planner (PT-07); Intelligence analyst (PT-06); Analyst (PT-03); Administrator (PT-05) | Commander to OP-20 Higher command; everyone else to the commander through the supervisor for live operations, directly for staff functions |
| OP-11 Site defense cell (per site) | Site operator (PT-01); site authority (the PT-01 or PT-02 holding engagement authority at the site) | Supervisor at OP-10 |
| OP-12 Port defense cell | Maritime operator (PT-01); port defense authority | Supervisor at OP-10; coordination with OP-25 |

## Authority flow

- Rules of engagement come from OP-20 to the commander; the commander delegates
  engagement authority per layer and per class (D-15: point-layer engagements of
  confirmed-hostile small UAS may be pre-delegated to operators; area layer and
  missiles never).
- The supervisor holds weapons control status and may hold or cease; a site or port
  authority decides within its delegation; a disconnected cell keeps the delegation
  in force at disconnection until it expires (D-15).
- Staff functions (planning, intelligence, analysis, administration) advise the
  commander and act through configuration baselines that the supervisor or
  commander applies (OA-24).

## Elements used

- PT-01 to PT-08; OP-10 to OP-12, OP-20, OP-25.

## Notes

- Shift manning is not modelled; a shift has at least a supervisor, one operator
  per active domain, and the sensor manager on call (MT-09 battle rhythm).
- Personnel availability views are out of scope for this revision (plan 03 open
  question).

## Traceability

- Derives from: Pr-Tx; `../../../mission/vignettes.md` (the setting);
  `../../../mission/roles-and-stakeholders.md` §4; D-15.
- Feeds: Pr-Cn, Sc-Sr (who holds which authority), Ar-Sr (workstations per post).
