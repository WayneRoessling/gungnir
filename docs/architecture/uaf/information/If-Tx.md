# If-Tx Information taxonomy

**UAF definition.** The information taxonomy view presents the information elements
of the architecture as a hierarchy.

**Purpose here.** The 33 information elements grouped by what they are for, so that
the generated structure view (If-Sr) and the operational view (Op-If) share one
list. Read by plan 10 (data architecture) and by integrators.

Status: first draft, 2026-09-04.

## The taxonomy

| Group | Elements |
|---|---|
| Observations and picture | IE-01 DetectionView; IE-02 TrackView; IE-07 Provenance; IE-08 Quality; IE-09 Classification; IE-10 MissionTime; IE-11 GlobalEntityId; IE-29 IdentificationEvidence |
| Decision | IE-03 ResourceView; IE-04 PlanView; IE-05 InterceptSolutionView; IE-30 RiskScore; IE-19 PolicyVerdict; IE-18 DecisionRecord |
| Events and record | IE-12 Envelope; IE-13 TrackingEvent; IE-14 InterceptEvent; IE-15 IngestEvent; IE-16 CommandEvent; IE-33 Mission and SessionId; IE-31 ReconciliationReport |
| Configuration and governance | IE-17 ConfigBaseline; IE-26 SchemaEntry; IE-27 SensorRecord and CoverageRegion; IE-32 WorkspaceLayout |
| Health, alerts, audit | IE-06 SystemHealth; IE-21 Alert and AlertLifecycle; IE-20 AuditEntry |
| Interface payloads | IE-22 SnapshotResponse; IE-23 ApprovalRequest; IE-24 SubmitDetectionRequest; IE-25 SubscribeRequest |
| Products | IE-28 Report |

## Versioning and ownership

- Every serialized element carries or is checked against `SCHEMA_VERSION` (SD-01);
  a mismatch is `ModelError::SchemaVersion`.
- `gungnir-model` owns the canonical types; other crates define record types (IE-17
  to IE-21, IE-26 to IE-33) that reference them and never redefine them (CLAUDE.md
  hard rule).
- Interop forms (SD-02 to SD-04) are encodings of IE-01 and IE-02, never separate
  vocabularies.

## Elements used

- IE-01 to IE-33; SD-01 to SD-04.

## Notes

- Pending elements the operational view needs (asset list, policy section,
  requirement, handoff, warning, outcome, releasability marking) are listed in Op-If
  against their gaps and will be registered as IE-34 onward when they exist.

## Traceability

- Derives from: If-Sr (generated from `gungnir-model`); the record types in the
  productization crates.
- Feeds: Op-If, If-Cn, Sv-If, plan 10 data architecture.
