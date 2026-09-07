# Pr-Cn Role connectivity

**UAF definition.** The personnel connectivity view shows the interactions between
posts and roles: who exchanges what with whom.

**Purpose here.** The human-to-human exchanges the threads require, and the
workspace each role sees in the software (the panels from
`gungnir_workflow::WorkspaceLayout`), so that plan 06 designs the screens around
real interactions.

Status: first draft, 2026-09-04.

## Role-to-role interactions

| From | To | Exchange | Threads |
|---|---|---|---|
| PT-02 Supervisor | PT-01 Operators | queue priorities, weapons control status, holds, delegations | MT-01, MT-02, MT-10 |
| PT-01 Operators | PT-02 | escalations, identity confirmations, requests for hold | MT-01 to MT-04 |
| PT-04 Sensor manager | PT-02 | re-tasking proposals, coverage before and after, concurrence requests for collection tasking | MT-07, MT-08 |
| PT-02 | PT-08 Commander | accepted-gap requests, escalations beyond delegation | MT-07, MT-01 |
| PT-08 | PT-02 | delegations, priorities, plan approval | MT-09 |
| PT-07 Planner | PT-08, PT-04 | laydown options, plan drafts, rehearsal results | MT-09 |
| PT-06 Intelligence analyst | PT-01, PT-02, peers | identity declarations, order of battle, products with marking | MT-06, MT-08 |
| PT-06 | PT-04 | collection requirements and tasking requests | MT-08 |
| PT-03 Analyst | PT-08, PT-02 | after-action reports, measures, lessons, model promotion evidence | MT-09 |
| PT-05 Administrator | all | accounts and roles, baseline management, node status | MT-09, MT-10 |

## Workspaces by role (`gungnir_workflow::WorkspaceLayout::for_role`)

| Role | Panels in layout order (code) | Design (plan 06) |
|---|---|---|
| PT-01 Operator | Viewport3d, TrackTable, InterceptPanel, ApprovalQueue, Alerts, SystemHealth | `../../../ux/wireframes/WF-L1-operator-layout.puml` |
| PT-02 Supervisor | the operator set plus SensorManagement, ConfigEditor | `../../../ux/wireframes/WF-L2-supervisor-layout.puml` |
| PT-03 Analyst | Viewport3d, TrackTable, Replay, Reports | `../../../ux/wireframes/WF-L3-analyst-layout.puml` |
| PT-04 Sensor manager | Viewport3d, SensorManagement, SystemHealth, Alerts, ConfigEditor | `../../../ux/wireframes/WF-L4-sensor-manager-layout.puml` |
| PT-05 Administrator | all panels | `../../../ux/wireframes/WF-L5-administrator-layout.puml` |
| PT-06 Intelligence analyst | (as analyst until GAP-068) | designed: `../../../ux/wireframes/WF-L6-intelligence-analyst-layout.puml` (requirements panel WF-15) |
| PT-07 Planner | (as sensor manager until GAP-068) | designed: `../../../ux/wireframes/WF-L7-planner-layout.puml` (planning panel WF-16) |
| PT-08 Commander | (as supervisor until GAP-068) | designed: `../../../ux/wireframes/WF-L8-commander-layout.puml` (summary WF-17) |

## Elements used

- PT-01 to PT-08; IE-32 WorkspaceLayout.

## Notes

- Voice and face-to-face exchanges in the command post are outside the software;
  the table lists them because the screens must support them (for example the
  supervisor's queue view must show what an operator sees).
- The panel list is code truth for the five roles in `Role`; plan 06
  (`../../../ux/information-architecture.md` §4) proposes the full lists for all eight,
  including the panels that do not exist yet.

## Traceability

- Derives from: `../../../mission/roles-and-stakeholders.md` §2;
  `gungnir-workflow/src/lib.rs`; Op-Cn (the needlines with a human at each end).
- Feeds: plan 06 (information architecture and wireframes), the role-to-activity
  matrix.
