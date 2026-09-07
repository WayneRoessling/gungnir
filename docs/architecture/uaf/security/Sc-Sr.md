# Sc-Sr Security structure

**UAF definition.** The security structure view shows how security controls are
allocated to resources and performers, and which assets each protects.

**Purpose here.** Who holds which authority and which resource enforces it: the
authority matrix as code enforces it today and as the operational roles require it.
Read by the security engineer building GAP-058 and by accreditors.

Status: first draft, 2026-09-04.

## Authorization matrix in code (`gungnir_security::authz::role_permits`)

| Action (`gungnir_security::actions`) | Operator | Supervisor | Analyst | Sensor manager | Administrator |
|---|---|---|---|---|---|
| `picture.view` | x | x | x | x | x |
| `detection.submit` | x | x | | | x |
| `plan.decide` | x | x | | | x |
| `plan.override` | | x | | | x |
| `sensor.task` | | x | | x | x |
| `config.apply` | | x | | x | x |
| `model.promote` | | | x | | x |
| `report.export` | | x | x | | x |

Unknown operators may do nothing. Administrators may do everything.

## Operational authority matrix (`../../../mission/roles-and-stakeholders.md` §4)

The operational matrix refines the code matrix by threat class and effector layer
and adds the three adopted roles: identity declarations per class; engagement
acceptance per layer (point delegated, area supervisor or commander); weapons
control status; hold or cease; sensor tasking with concurrence; plan apply; model
promotion with concurrence; product release; accepted coverage gaps (commander
only); reconciliation conflict resolution. GAP-058 (per-class and per-layer
qualifiers) and GAP-068 (the three roles) close the difference.

## Allocation of controls

| Control | Enforcing resource | Where it runs | Protects |
|---|---|---|---|
| Authentication (SV-20) | RS-security; RS-api for callers | every profile; the node for callers | credentials, the record |
| Authorization (SV-21) | RS-security, called by RS-command, RS-config, RS-api | every profile | decisions, baselines, the API |
| Decision gate (SV-15, SV-16) | RS-policy, RS-command | desktop (local approval) and node (arbiter) | CAP-4.3 |
| Audit (SV-22) | RS-security | local log on the desktop; central log on the node | accountability |
| Validation and quarantine (SV-08) | RS-ingest | wherever sensors attach | the picture |
| Transit and at-rest protection | RS-api, RS-remote, RS-store | connected profiles; cloud | the record, the picture |
| Releasability | RS-api with the marking on views | the node's interface | products |
| Supply chain | AR-07 | build and release | the binaries and models |

## Elements used

- PT-01 to PT-08; SV-08, SV-15, SV-16, SV-20 to SV-22; RS-api, RS-command,
  RS-config, RS-ingest, RS-policy, RS-remote, RS-security, RS-store; AR-07.

## Notes

- The code matrix is truth for the five roles in `Role`; the operational matrix is
  the specification GAP-058 and GAP-068 implement. Both are shown so the difference
  is visible.

## Traceability

- Derives from: `gungnir-security/src/authz.rs`, `gungnir-security/src/lib.rs`;
  `../../../mission/roles-and-stakeholders.md` §4; `../../../../ARCHITECTURE.md` §8.5.
- Feeds: Sc-Pr, Pr-Tx, the security verification rows (MOP-38, MOP-39).
