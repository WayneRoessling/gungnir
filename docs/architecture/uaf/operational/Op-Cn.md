# Op-Cn Operational connectivity

**UAF definition.** The operational connectivity view shows the needlines (required
information exchanges) between operational performers.

**Purpose here.** Every exchange the threads require between performers, with the
information element that carries it and whether the system carries it today. Read by
plan 10 (Business Architecture interactions) and by integrators (which exchanges
cross the system boundary).

Status: first draft, 2026-09-04.

Diagram: [`Op-Cn.puml`](Op-Cn.puml).

## Needlines

| Id | From | To | Information (If-Sr element) | Threads | Carried today |
|---|---|---|---|---|---|
| NL-01 | OP-33 Sensor network | OP-30 C2 system | Observations, IE-01 DetectionView; health | all live | Recorded and simulated adapters only (GAP-001) |
| NL-02 | OP-21 Neighbouring sector, OP-20 Higher command | OP-30 | Early warning, peer tracks, IE-02 TrackView with provenance | MT-01, MT-02, MT-04, MT-05, MT-08 | No (GAP-009, GAP-041) |
| NL-03 | OP-24, OP-25 authorities | OP-30 | Cooperative identity (AIS, ADS-B, flight plans) | MT-02, MT-04, MT-05, MT-08 | No (GAP-010) |
| NL-04 | OP-30 | OP-01, OP-02, OP-08 | Picture, IE-02; identities; IE-04 PlanView with IE-19 PolicyVerdict; alerts IE-21 | all engagement threads | Picture and plan panels yes; verdicts and queue not wired (GAP-028, GAP-038) |
| NL-05 | OP-01, OP-02, OP-08 | OP-30 | Decisions, IE-18 DecisionRecord | MT-01 to MT-04, MT-06, MT-10 | Workflow exists; not wired (GAP-028) |
| NL-06 | OP-30 (OP-34) | OP-22 Effectors | Handoff with decision and provenance | MT-01 to MT-04, MT-06 | No (GAP-040, GAP-041) |
| NL-07 | OP-22 Effectors | OP-30 | Readiness (IE-03 ResourceView), engagement outcomes | MT-01 to MT-04, MT-06 | Readiness from configuration; outcomes no (GAP-043) |
| NL-08 | OP-30 | defended assets, OP-24, OP-25 | Warnings with lead time | MT-01, MT-02, MT-04 | No (GAP-042) |
| NL-09 | OP-04 Sensor manager | OP-30, OP-23 | Sensor modes, tasking, calibration | MT-03, MT-07, MT-08, MT-09 | Registry exists; not wired; no outbound path (GAP-003, GAP-004) |
| NL-10 | OP-30 | OP-04, OP-02 | Health, IE-06; coverage and gaps; incidents IE-21 | MT-07 | Health yes; coverage rendering no (GAP-007) |
| NL-11 | OP-06 Intelligence analyst | OP-30 | Requirements, identity declarations, merges, releases | MT-08 | Declarations and merges yes; requirements no (GAP-005) |
| NL-12 | OP-30 | OP-20, OP-21, OP-26 | Picture, products, warnings with releasability | MT-05, MT-08 | No (GAP-041, GAP-062, GAP-065) |
| NL-13 | OP-07, OP-08 | OP-30 | Defended-asset list, laydown, policy, plans; IE-17 ConfigBaseline | MT-09 | Baselines yes; asset list and policy sections no (GAP-026, GAP-052) |
| NL-14 | OP-03 Analyst | OP-30 | Replay requests, reports, model promotion | MT-09 | Yes |
| NL-15 | OP-31 Workstation | OP-32 Node | IE-12 Envelope stream, IE-22 snapshot, IE-24 submissions, IE-23 decisions; store-and-forward on reconnect | MT-10 and every connected thread | Contract yes; transport no (GAP-041) |
| NL-16 | OP-05 Administrator | OP-30 | Accounts, roles, baselines, retention | MT-09, MT-10 | Roles and baselines yes; accounts pending D-02 (GAP-057) |
| NL-17 | OP-22 Effectors, defended assets, OP-26 | OP-30 | Position an element reports about itself, with the age and the reporter; never an observation | MT-01 to MT-04, MT-06 | No (GAP-090). The friendly set is the friendlies a sensor detected |
| NL-18 | OP-30 | OP-22, OP-24, OP-25, OP-21, defended assets | Picture, warnings and handoffs to a participant that holds no machine identity here | MT-01 to MT-06, MT-08 | No (GAP-091). NL-06, NL-08 and NL-12 reach only participants who can hold one |

## Bearers, added 2026-09-06

A needline says what must be exchanged; it does not say over what. Four of the sixteen
above are carried today **only for a participant that can hold a machine identity in this
deployment's trust roots and read the v2 contract** -- the peer link and the API gates
that GAP-009 and GAP-065 wired. The participants who cannot are not an edge case: a
mobile fire group is a truck and four people, and a port authority runs somebody else's
software.

| Needline | Canonical bearer, today | Second bearer, designed | Why the second |
|---|---|---|---|
| NL-02 Peer tracks and early warning inbound | `PeerLink` over mutual TLS to another node (GAP-009) | SD-16 inbound feed (DN-25) | A neighbour who does not run this product |
| NL-06 Handoff to effectors | `gungnir_remote::endpoint` to an HTTP endpoint (GAP-040) | SD-16 outbound sink (DN-25) | DN-07 case 4: an effector that is people, not a system with an endpoint |
| NL-08 Warnings | The warning ledger, no transport (GAP-042, DN-03) | SD-16 outbound sink (DN-25) | An asset or authority with no endpoint of ours; acknowledged delivery is not available on it, and DN-25 §5 rule 6 says so on the panel |
| NL-12 Picture and products outbound | The v2 stream and reports under an agreement and a marking (GAP-062, GAP-065) | SD-16 outbound sink (DN-25) | `ExchangeFormat`'s two non-canonical options are both unavailable: STANAG 4676 has no obtainable specification, ASTERIX encode is refused by design |
| NL-17 Reported positions | none | SD-16 inbound feed (DN-25) | Nothing else reports a friendly that no sensor sees |
| NL-18 Exchange without a machine identity | none | SD-16, both directions (DN-25) | The needline is the second bearer; it exists to be named rather than left implicit under the four above |

The marking still decides what crosses: DN-25 §5 rule 1 keeps AP-09 intact by refusing to
let an unauthenticated bearer stand in for a party, which is the one thing a second bearer
could quietly cost.

## Elements used

- OP-01 to OP-08, OP-20 to OP-26, OP-30 to OP-34; IE-01 to IE-04, IE-06, IE-12,
  IE-17 to IE-19, IE-21 to IE-24; SD-16 in the bearer table.

## Notes

- Needline ids NL-xx are local to this view; they are not registry elements.
- "Carried today" is the implementation status on 2026-09-04 for NL-01 to NL-16 and on
  2026-09-06 for NL-17 and NL-18, with the gap that closes it; the operational need is the
  same either way. **NL-01 to NL-16 were not re-checked on 2026-09-06** and several moved
  that week, so read the bearer table's "canonical bearer, today" column, which was, for the
  four needlines it names.

## Traceability

- Derives from: Op-Tx, Op-Sr; the step tables' Information column in
  `../../../mission/mission-threads.md`; If-Sr.
- Feeds: If-Cn (the same exchanges as information flows), Sv-Cn, Sc-Cn (which
  needlines cross a trust boundary), plan 10 phase B.
