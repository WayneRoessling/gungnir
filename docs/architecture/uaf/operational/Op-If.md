# Op-If Operational information

**UAF definition.** The operational information view identifies the information
elements exchanged or held by operational performers, independent of how a system
represents them.

**Purpose here.** The information the sector works with, in operational terms, with
the canonical type that carries each element where the system does. Read by plan
10 (Information Systems Architecture, data) and by plan 06 (what each panel shows).

Status: first draft, 2026-09-04.

## Operational information elements

| Operational information | Held or produced by | Carried as (If-Sr) | Notes |
|---|---|---|---|
| Observation | OP-33 → OP-30 | IE-01 DetectionView with IE-07 Provenance | Source and receipt time (CAP-1.5); calibration baseline optional (MOP-19) |
| Track picture per domain | OP-30 | IE-02 TrackView with IE-08 Quality, IE-09 Classification | Stale drawn distinctly and never allocated |
| Identity and evidence | OP-30, OP-06 | IE-09, IE-29 IdentificationEvidence, IE-11 GlobalEntityId | Evidence retained with every declaration (MOP-24) |
| Threat score | OP-30 | IE-30 RiskScore | Factors visible (MOP-28) |
| Defended-asset list and priorities | OP-08, OP-07 | IE-17 ConfigBaseline (section pending, GAP-026) | Warning obligations per asset |
| Recommendation | OP-30 → deciders | IE-04 PlanView, IE-05 InterceptSolutionView, IE-19 PolicyVerdict, rationale | Intercept point None until GAP-031 |
| Decision | deciders → OP-30 | IE-18 DecisionRecord, IE-16 CommandEvent | Append-only |
| Handoff | OP-30 → OP-22 | message pending (GAP-040) | Carries the decision and provenance |
| Warning | OP-30 → assets, OP-24, OP-25 | alert (IE-21) and channel message pending (GAP-042) | Lead time per asset |
| Engagement outcome | OP-22 → OP-30 | IE-14 InterceptEvent (Superseded); outcome event pending (GAP-043) | |
| Resource readiness | OP-22, configuration | IE-03 ResourceView | Capacity and readiness |
| Sensor state and coverage | OP-04, OP-30 | IE-27 SensorRecord and CoverageRegion | Modes per Op-St |
| Health and incidents | OP-30 | IE-06 SystemHealth, IE-21 Alert | Reported, never inferred |
| Policy in force | OP-08, OP-02 | IE-17 (policy section pending, GAP-052) | Identification criteria, status, authorities, restrictions, delegations |
| Plan and laydown | OP-07 | IE-17 ConfigBaseline | Validated before apply |
| Mission record | OP-30 | IE-12 Envelope stream in the journal; IE-33 Mission | The system of record |
| Reports and measures | OP-03 | IE-28 Report | Every figure traceable to the journal |
| Collection requirement | OP-06, OP-08 | pending (GAP-005) | Area, question, priority, window |
| Order of battle and pattern of life | OP-06 | entities with lineage (IE-11); product pending (GAP-025) | Across sessions |
| Peer picture and products | OP-21, OP-26 ↔ OP-30 | IE-02 with source; releasability field pending (GAP-062) | Staleness visible (MOP-22) |
| Audit record | OP-30 | IE-20 AuditEntry | One per gated action (MOP-39) |

## Elements used

- IE-01 to IE-33 as listed; the performers named.

## Notes

- "Pending" marks information the threads need that has no canonical type yet; each
  names its gap so that If-Sr gains the type when the gap closes.
- Classification of the information itself (releasability, marking) is CAP-6.6 and
  decision D-06: a field on views and products from increment 3.

## Traceability

- Derives from: the Information column of every thread step in
  `../../../mission/mission-threads.md`; Op-Cn; If-Sr.
- Feeds: If-Tx, If-Cn, plan 06 panel content, plan 10 data architecture.
