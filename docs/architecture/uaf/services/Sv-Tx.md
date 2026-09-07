# Sv-Tx Service taxonomy

**UAF definition.** The service taxonomy view presents the services of the
architecture as a hierarchy, independent of the resources that implement them.

**Purpose here.** The 34 services the crates expose through their traits, grouped by
what they do for the mission, with their implementation status. This is the service
vocabulary the operational activities are realized by and the resources implement.

Status: first draft, 2026-09-04. Each service's `code` path is checked against the
source by `../tools/build_uaf.py`.

## The taxonomy

| Group | Services (status) |
|---|---|
| Picture | SV-01 Tracking (scaffold); SV-08 Ingest gateway (real); SV-05 Time authority (real); SV-09 Sensor registry (real); SV-10 Interop catalogue and codecs (scaffold); SV-11 Identity resolution (real); SV-12 Identification (real); SV-34 Scenario generation (scaffold); SV-33 Tracking metrics (scaffold) |
| Decision | SV-17 Threat assessment (real); SV-02 Intercept planning (scaffold); SV-18 Decision support (scaffold); SV-15 Policy (real); SV-16 Approval workflow (real) |
| Geospatial and analytics | SV-13 Geospatial (real); SV-14 Analytics (real); SV-31 Data loading (scaffold); SV-32 Point-cloud fusion (scaffold) |
| Record and sustainment | SV-03 Event bus (real); SV-04 Event journal (real); SV-06 Configuration store (real); SV-07 Mission lifecycle (scaffold); SV-19 Model registry (real); SV-24 Health monitor (real); SV-27 Operator workflow (real); SV-28 Replay (real); SV-29 Reporting (real) |
| Connectivity | SV-23 API v1 (scaffold); SV-30 Remote backends (scaffold); SV-25 Store-and-forward and reconciliation (real); SV-26 Shared picture and arbitration (real) |
| Security | SV-20 Authentication (scaffold); SV-21 Authorization (real); SV-22 Audit log (real) |

"Real" means implemented and tested in the crate; "scaffold" means the trait exists
and at least one implementation returns `NotImplemented` or is a trait surface. The
per-service notes are in `../model/elements.yaml`.

## Elements used

- SV-01 to SV-34.

## Notes

- A service is an operationally meaningful trait; helper traits (for example
  `LineOfSight`'s siblings in `gungnir-analytics`) are listed in Rs-If but not
  raised to services.
- Plan 08's assistant will add a service (realizing OA-32) when its crate exists;
  the registry marks that relationship planned.

## Traceability

- Derives from: the `pub trait` declarations (Rs-If); `../../../gungnir-capabilities.md` §8.
- Feeds: Sv-Sr, Sv-Cn, Sv-Pr, Sv-If, the activity-to-service and service-to-resource
  matrices, plan 10 application architecture.
