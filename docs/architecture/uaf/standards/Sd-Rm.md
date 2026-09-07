# Sd-Rm Standards roadmap

**UAF definition.** The standards roadmap view shows when standards are adopted,
retired, or changed.

**Purpose here.** Which standard lands in which increment, tied to the gap that
brings it. Read with St-Rm and Pj-Rm.

Status: first draft, 2026-09-04.

## Adoption by increment

| Increment | Standards adopted or exercised | Through |
|---|---|---|
| PJ-I1 (now) | SD-01, SD-02, SD-05, SD-12, SD-14 in place | the workspace as it stands |
| PJ-I2 | SD-03 (the categories the lead mission's radars emit); SD-09, SD-10; SD-11; SD-13 exercised | GAP-064, GAP-010, GAP-069, GAP-061 |
| PJ-I3 | SD-03 remaining categories; SD-04; the releasability field on SD-01 payloads; **SD-16** codec, inbound feed and mesh sink | GAP-064, GAP-062, GAP-090, GAP-091 |
| PJ-I4 | SD-06, SD-07, SD-08 with the transport; SD-15 as plan 10 completes; the conformance suite gates SD-01 to SD-04; **SD-16** stream sink, which needs SD-08's client certificate | GAP-041, GAP-060, GAP-063, GAP-091 |

## Retirement and change

- SD-01 changes only by a new schema version alongside the old, per the
  compatibility rules in `../../../gungnir-api-v1.md`; `v1` is retired only when
  every known client has moved.
- gRPC as a second transport for peers is planned after SD-06 and SD-07, using the
  same message types; it will be registered as a standard when scheduled.

## Elements used

- SD-01 to SD-15; PJ-I1 to PJ-I4.

## Notes

- The dates are the increments', not calendar dates; the increments have no dates
  until plan 01 sets them.
- SD-16 lands in two pieces on purpose (added 2026-09-06, DN-25). The codec, the inbound
  feed and the multicast sink need no certificate and can be built in I3; the stream sink
  establishes a party from a certificate and therefore cannot precede SD-08. Splitting it
  the other way would put the only bearer that can carry a marked payload first and leave
  the deployment to discover the restriction late.

## Traceability

- Derives from: Sd-Tx, St-Rm, `../../../mission/gap-analysis/closure-roadmap.md`.
- Feeds: Pj-Rm, plan 10 phases D and E.
