# Sv-Cn Service connectivity

**UAF definition.** The service connectivity view shows the interfaces between
services and the information that flows across them.

**Purpose here.** The data path through the services on one tick, and across the
desktop-to-node boundary, with the information element on each link. Read with
Rs-Pr (the tick loops) and Sv-If (the contracts).

Status: first draft, 2026-09-04.

Diagram: [`Sv-Cn.puml`](Sv-Cn.puml).

## Flows

| From | To | Information | Status |
|---|---|---|---|
| SV-08 Ingest gateway | SV-01 Tracking | IE-01 DetectionView (accepted) | real |
| SV-08 | SV-03 Event bus | IE-15 IngestEvent | real |
| SV-01 | SV-02 Intercept planning | IE-02 TrackView slice | real (empty until the pipeline exists) |
| SV-06 Configuration store | SV-02 | IE-03 ResourceView list | real |
| SV-17 Threat assessment | SV-02 | reward matrix from IE-30 | implemented, not wired (GAP-028) |
| SV-02 | SV-03 | IE-14 InterceptEvent::PlanProposed | real |
| SV-02 | SV-15 Policy | IE-04 PlanView | not wired (GAP-028) |
| SV-15 | SV-16 Approval workflow | IE-19 PolicyVerdict | not wired (GAP-028) |
| SV-16 | SV-03 | IE-16 CommandEvent | not wired (GAP-028) |
| SV-16 | SV-22 Audit log | IE-20 AuditEntry | not wired (GAP-059) |
| SV-03 | SV-04 Event journal | IE-12 Envelope | real (both binaries) |
| SV-03 | SV-23 API v1 | IE-12 Envelope stream | transport pending (GAP-041) |
| SV-23 | SV-30 Remote backends | IE-22 snapshot, IE-12 stream | transport pending |
| SV-30 | SV-23 | IE-24 submissions, IE-23 decisions; outbox on reconnect | transport pending |
| SV-25 | SV-04 | merged journal, IE-31 report | real |
| SV-26 | SV-16 | arbitrated decision | real (rule), wiring pending |
| SV-24 Health monitor | SV-27 Operator workflow | IE-06 health, IE-21 alerts | real |
| SV-04 | SV-28 Replay, SV-29 Reporting | IE-12 envelopes | real |
| SV-09 Sensor registry | SV-14 Analytics | IE-27 coverage regions | implemented, not wired (GAP-003) |
| SV-12 Identification | SV-01 | IE-09 Classification on IE-02 | implemented, not wired |
| SV-19 Model registry | SV-01 | promoted baseline | not wired (GAP-053) |

## Elements used

- The services and information elements named.

## Notes

- Inside one process the "interface" between services is a Rust trait call; across
  the desktop-to-node boundary it is SV-23's contract. Nothing else crosses a
  process boundary.
- "Not wired" flows exist as tested crates whose call is not yet made by the
  binaries' tick loops (Rs-Pr); the gap ids name the work.

## Traceability

- Derives from: Rs-Pr (the tick order), Rs-Cn (which crate may call which), Sv-If.
- Feeds: If-Cn, Sc-Cn (which flows cross a trust boundary), plan 10 application
  architecture.
