# Sc-Cn Security connectivity

**UAF definition.** The security connectivity view shows the trust boundaries and
the protected connections that cross them.

**Purpose here.** Which flows cross a trust boundary in each deployment profile and
what protects them, today and as designed. Read by the security engineer and by a
customer's accreditor.

Status: first draft, 2026-09-04.

Diagram: [`Sc-Cn.puml`](Sc-Cn.puml).

## Trust boundaries and crossings

| Crossing | Profiles | Protection today | Protection designed |
|---|---|---|---|
| Sensor feed → gateway (NL-01) | all | allow-list by sensor id for raw feeds; mutual TLS machine identity for API submissions (D-02, GAP-002); validation; quarantine | signed reports for spotter applications |
| Workstation ↔ node (NL-15) | on-prem, cloud | mutual TLS (SD-08); operator token on every route but sign-in; role checked on gated actions (GAP-041, GAP-057, GAP-058) | as today |
| Node ↔ peers and higher command (NL-02, NL-12) | on-prem, cloud | mutual TLS; releasability enforced per caller (GAP-062, GAP-065) | as today, with a persistent outbound peer identity (GAP-060) |
| Node → effector systems (NL-06) | on-prem, cloud | mutual TLS; a handoff cannot be built without its decision record (GAP-040) | as today |
| Operator → workstation | all | local accounts with an enforced session lifetime (GAP-057) | as today |
| Node journal at rest | cloud | encryption under a managed key service, when one is configured (GAP-060) | as today (GAP-084) |
| Build → registry → host | all | `deny.toml` policy; unsigned | signed images and SBOM (SD-13, GAP-061) |
| Assistant → cloud model (plan 08) | cloud (live picture), on-prem (derived text) | not built | egress policy per profile (D-14); no authority |

## Boundary rules

- The node's only external surface is `gungnir-api` (ARCHITECTURE.md §8.3); nothing
  else listens.
- The desktop treats the node as the system of record while connected and never
  its own state as authoritative (`../../../gungnir-api-v1.md`).
- Every message from outside the process is data, never an instruction (CAP-6.7).

## Elements used

- AR-01 to AR-07; RS-api, RS-remote, RS-ingest, RS-store, RS-node; SD-08, SD-13.

## Notes

- The disconnected desktop has one boundary: its host. Physical security of the
  host is the deployment's responsibility and outside this description.

## Traceability

- Derives from: Op-Cn (the needlines), Ar-Cn (the physical links),
  `../../../../ARCHITECTURE.md` §8.5, D-02.
- Feeds: Sc-Pr, the compliance assessment (MOP-40), plan 10 security architecture.
