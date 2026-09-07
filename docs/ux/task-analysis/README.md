# Task analyses

Status: first draft, 2026-09-04. One hierarchical task analysis per role, derived
from the thread steps in `../../mission/mission-threads.md` and the roles document,
and cross-referenced to the UAF operational activities (OA-xx in
`../../architecture/uaf/model/elements.yaml`).

Each file lists the role's top-level tasks (T-role-n), their subtasks (T-role-n.m),
and for each: the information needed and the model field that carries it, the
decision (if any) and who holds it, the error modes the design must guard against,
the time pressure, and the panels used (PN-xx from `../information-architecture.md`).

| File | Role | Persona | Threads |
|---|---|---|---|
| [`operator.md`](operator.md) | Operator | P-01 | MT-01 to MT-06, MT-10 |
| [`supervisor.md`](supervisor.md) | Supervisor | P-02 | MT-01, MT-02, MT-07, MT-09, MT-10 |
| [`analyst.md`](analyst.md) | Analyst | P-03 | MT-09 |
| [`sensor-manager.md`](sensor-manager.md) | Sensor manager | P-04 | MT-03, MT-07, MT-08, MT-09 |
| [`administrator.md`](administrator.md) | Administrator | P-05 | MT-09, MT-10 |
| [`intelligence-analyst.md`](intelligence-analyst.md) | Intelligence analyst | P-06 | MT-06, MT-08 |
| [`planner.md`](planner.md) | Planner | P-07 | MT-09 |
| [`commander.md`](commander.md) | Commander | P-08 | MT-01, MT-02, MT-07, MT-09 |

Time-pressure classes used throughout: **S** seconds (a missile at 30 km, an FPV
approach), **M** minutes (a drone raid's decision rate, sensor loss), **H** hours
(planning, review), **B** battle rhythm (reports, handover).
