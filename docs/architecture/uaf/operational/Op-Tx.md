# Op-Tx Operational performer taxonomy

**UAF definition.** The operational taxonomy view presents the operational
performers (logical roles, organizations, and systems) of the architecture as a
hierarchy.

**Purpose here.** The people, organizations, external parties, and the system as
logical performers, so that every activity and interaction in this domain names a
performer from one list. Read by plan 06 (personas) and plan 10 (Business
Architecture actors).

Status: first draft, 2026-09-04.

## The taxonomy

| Kind | Performers |
|---|---|
| Roles (people) | OP-01 Operator; OP-02 Supervisor; OP-03 Analyst; OP-04 Sensor manager; OP-05 Administrator; OP-06 Intelligence analyst; OP-07 Planner; OP-08 Commander |
| Organizational nodes | OP-10 Sector command post; OP-11 Site defense cell; OP-12 Port defense cell |
| External parties | OP-20 Higher command; OP-21 Neighbouring sector; OP-22 Fire units and effector systems; OP-23 Sensor operators and maintainers; OP-24 Civil aviation authority and airport; OP-25 Port and maritime authorities; OP-26 Coalition partners; OP-27 Accreditors and auditors |
| The system (logical) | OP-30 Command-and-control system, with parts OP-31 Operator workstation, OP-32 Sector service node, OP-33 Sensor network, OP-34 Effector interface |

Descriptions are in `../model/elements.yaml` (section `operational_performers`);
the roles' decisions, information needs, tempo, and panels are in
`../../../mission/roles-and-stakeholders.md` §2.

## Elements used

- OP-01 to OP-08, OP-10 to OP-12, OP-20 to OP-27, OP-30 to OP-34.

## Notes

- Roles OP-06 to OP-08 were adopted on 2026-09-04 (D-05); until GAP-068 lands their
  responsibilities are carried by OP-02, OP-03, and OP-04 as the roles document
  states.
- OP-33 (sensor network) is logical: the physical sensors are actual resources
  (AR-04); in the operational domain they are the performer that produces
  observations.
- Personnel types PT-01 to PT-08 are the same eight roles seen from the personnel
  domain (Pr-Tx), with their code representation.

## Traceability

- Derives from: `../../../mission/roles-and-stakeholders.md` §1 and §3;
  `../../../mission/vignettes.md` (the organizational nodes); `../../../../ARCHITECTURE.md` §8 (the system parts).
- Feeds: Op-Sr, Op-Cn, every Op-Pr and Op-Is, Pr-Tx, the role-to-activity matrix.
