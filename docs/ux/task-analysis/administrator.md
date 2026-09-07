# Task analysis: Administrator (P-05)

Threads: MT-09 (baselines), MT-10 (node operation). Layout: PN-14, PN-20, PN-09
beside the viewport; every other panel read-only.

## T-ad-1 Manage configuration baselines (OA-24)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 1.1 Load and inspect a baseline | `ConfigBaseline` sections: sensors, resources, tracking, backend, node, horizon, data directory; version | none | editing the live baseline by accident | H | PN-14 |
| 1.2 Edit a section | field-level editing with types and units | none | a value outside its range (validation catches) | H | PN-14 |
| 1.3 Validate | `validate` result: Ok, `Invalid` with the field, `VersionTooNew` | none | ignoring a validation failure | H | PN-14 |
| 1.4 Save a version | `ConfigStore::save`; version increment | administrator | overwriting an applied version | H | PN-14 |
| 1.5 Apply is not the administrator's decision | the apply control is present but requires `config.apply`; the supervisor or commander applies plans; the administrator applies infrastructure sections (node, data directory, backend) | administrator for infrastructure sections | applying a policy or asset-list section as an administrator | H | PN-14 (dialog) |

## T-ad-2 Accounts and roles (OA-34)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 2.1 See operators and their roles | `StaticRoleAuthorizer` assignments (`OperatorId`, `Role`) | none | an unknown operator with a role by default (forbidden) | H | PN-20 |
| 2.2 Assign or change a role | `assign`; audited | administrator | granting supervisor to an operator without record | H | PN-20 (dialog) |
| 2.3 Credentials | mechanism per D-02 (GAP-057) | administrator | credentials handled outside the audit | H | PN-20 |

## T-ad-3 Operate the node (OA-27, MT-10)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 3.1 See node health and the journal state | `SystemHealth`, watchdog warnings, journal Open, Failed, Closed, data directory usage | none | a failed journal unnoticed | M | PN-09 |
| 3.2 Retention | `RetentionPolicy` age; sessions purged | administrator | purging a session under review | H | PN-14, PN-09 |
| 3.3 Restart and recovery | recovery time (under 30 s budget), last checkpoint | administrator | restarting during a raid | M | outside the product (deploy) |

## T-ad-4 Audit review (OA-35)

| Subtask | Information | Decision | Error modes | Pressure | Panels |
|---|---|---|---|---|---|
| 4.1 Read the audit log | `AuditEntry` actor, action, time, outcome; filter by action and operator | none | an action without an entry (MOP-39) | H | PN-20 |
| 4.2 Export for accreditors | export with provenance | administrator | export without marking | B | PN-20, PN-13 |

## Error modes that shape the design

- The administrator layout carries no engagement decision dialog even though
  `role_permits` allows everything; the roles document keeps administration out of
  the engagement chain, and the layout enforces it.
- Validation failures are shown at the field, and the apply control is disabled
  until validation passes (MOP-36).
- Role assignment is a dialog that shows what the role permits before confirming.

## Traceability

- Activities OA-24, OA-27, OA-34, OA-35; capabilities CAP-5.6, CAP-6.1, CAP-6.2,
  CAP-6.3, CAP-7.3; wireframes WF-14, WF-20; flows FL-05.
