# If-Cn Information exchange

**UAF definition.** The information connectivity view shows how information flows
between elements of the architecture and in what form.

**Purpose here.** The end-to-end path of the two information elements that matter
most, an observation and a decision, from source to record, with the encoding at
each hop. Read by integrators and by plan 10.

Status: first draft, 2026-09-04.

Diagram: [`If-Cn.puml`](If-Cn.puml).

## An observation

| Hop | Form | Element |
|---|---|---|
| Sensor → adapter | native protocol (ASTERIX Cat 048 for radar, SD-03; AIS, ADS-B; vendor formats; recorded JSON lines today) | raw report |
| Adapter → gateway | IE-01 DetectionView with source and receipt time and provenance | IE-01 |
| Gateway → tracking service | accepted IE-01 (quarantined ones become IE-15 Quarantined) | IE-01, IE-15 |
| Tracking service → snapshot | IE-02 TrackView with IE-08 quality | IE-02 |
| Snapshot → bus | IE-13 TrackingEvent inside IE-12 Envelope (design; today the snapshot is polled) | IE-12 |
| Bus → journal | IE-12 as one JSON line (SD-01, SD-05) | IE-12 |
| Bus → API stream | IE-12 as a WebSocket frame (SD-07, planned) | IE-12 |
| Node → desktop projection | IE-22 snapshot then IE-12 frames applied by SV-26 | IE-22, IE-12 |
| Journal → analytics | Arrow record batches of IE-01 (SD-02) | IE-01 |
| Node → peers | IE-02 in STANAG 4676 (SD-04, planned) with releasability marking (planned) | IE-02 |

## A decision

| Hop | Form | Element |
|---|---|---|
| Intercept service → bus | IE-14 PlanProposed carrying IE-04 | IE-04 |
| Policy → workflow | IE-19 PolicyVerdict with the plan | IE-19 |
| Workflow → panel | pending approval with rationale (IE-32 says which roles see it) | IE-04, IE-19 |
| Decider → workflow | on the desktop a trait call; from a connected desktop IE-23 ApprovalRequest over the API (SD-06, planned) | IE-23 |
| Workflow → record | IE-18 DecisionRecord; IE-16 Decided on the bus; IE-20 AuditEntry | IE-18, IE-16, IE-20 |
| Record → effector | handoff message (planned, GAP-040) with the decision and the track's provenance | pending |
| Journal → report | IE-28 with every figure traced to IE-12 sequence numbers | IE-28 |

## Elements used

- IE-01, IE-02, IE-04, IE-08, IE-12 to IE-16, IE-18 to IE-20, IE-22, IE-23, IE-28,
  IE-32; SD-01 to SD-07.

## Notes

- There is one encoding per hop and it is the `serde` form of the canonical type
  wherever the hop is Gungnir-to-Gungnir; industry encodings appear only at the
  sensor and peer boundaries.
- Hops marked planned wait on GAP-041 (transport), GAP-064 (codecs), GAP-062
  (marking), GAP-040 (handoff).

## Traceability

- Derives from: If-Sr, Sv-Cn, Op-Cn; `../../../gungnir-api-v1.md`; `gungnir-interop`.
- Feeds: Sd-Tx (which standard applies at which hop), plan 10 data architecture,
  the interop conformance suite (GAP-063).
