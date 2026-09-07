# Data architecture

Status: first draft, 2026-09-04. Phase C, data. **The content lives in the UAF
information views; this document says what phase C concluded about data and where the
data gaps are.** Engineering reviewer sign-off outstanding.

## 1. The canonical model

One crate owns the shared vocabulary. `gungnir-core` owns the primitives (track
identifier, track status, resource identifier, the positive-semi-definite check),
`gungnir-coord` owns the geodetic type, and `gungnir-model` owns mission time, the
canonical views, and the events. Everything above re-exports; nothing redefines. The
compliance assessment checked all nine shared types and found exactly one definition of
each.

| Element | Where |
|---|---|
| Information taxonomy | `../../uaf/information/If-Tx.md` |
| Information structure, generated from the model crate | `../../uaf/information/If-Sr.md` |
| What flows between which resources | `../../uaf/information/If-Cn.md` |
| Operational information exchange | `../../uaf/operational/Op-If.md` |
| Resource information exchange | `../../uaf/resources/Rs-If.md` |

The views are the model; this document does not restate the field lists.

## 2. Provenance is part of the schema

Not a convention: a field. The detection view carries its source and its receipt time.
Identification evidence names its producer, and for a machine-learning source that string
includes the model name and version, so a prediction can be traced to the artefact that
made it. The identity resolver keeps lineage. Envelopes carry a bus sequence number and a
mission time.

This is principle AP-07 and it is what lets fusion, after-action review, and the
assistant's provenance line all rest on the same mechanism rather than three.

## 3. The journal

The event journal is the record of a session (AP-08). JSON lines, append-only, sequenced,
mission-timed, and replayable to the same picture.

| Property | Decision |
|---|---|
| Durability | The node journal fsyncs every envelope; desktop journals are buffered with fsync on session save and every five seconds (D-04) |
| Reconciliation | Mission-time merge, duplicates dropped, conflicts reported rather than silently resolved (D-03) |
| Arbitration | Higher role wins, earlier decision wins a tie (D-03) |
| Replay | Deterministic, and the basis for measures, after-action review, and defect reproduction |

Reporting computes figures from the journal before any narrative is drafted, which is why
the assistant never produces a number.

## 4. Data lifecycle and retention

| Class | Produced by | Retained | Notes |
|---|---|---|---|
| Detections | Ingest, after validation | With the session journal | Quarantined detections are kept separately with the reason |
| Tracks and picture state | The tracking service | Derived; reproduced by replay | Not stored independently of the journal |
| Decisions and approvals | Command | With the journal and in the audit log | Two homes on purpose: one for replay, one for accountability |
| Audit entries | Security | The audit retention policy | Includes assistant exchanges |
| Configuration baselines | Configuration | Versioned, promotable, rollback-able | A signed baseline is an open release-governance item |
| Reports and products | Reporting | Per deployment, with releasability markings | Enforcement per caller is GAP-062 |
| Training datasets and model artefacts | The machine-learning pipeline | **A separate repository** | Only the model manifest enters this one |

Retention periods themselves are per deployment and are not fixed here, because the
customer's record-keeping obligation determines them.

## 5. Data security

Releasability is a property of the data, not of the channel (AP-09, D-06): a marking on
views, reports, and the interface contract in increment 3, enforced per caller in
increment 4. Encryption in transit and at rest is GAP-060; transport-layer security 1.3
on every network crossing, mutual for machine identities (D-02).

The two places where data leaves the system deliberately are the interface to peers, which
carries markings, and the assistant's egress to a model provider, which is checked against
a per-profile policy on the assembled request rather than on the operator's intent.

## 6. Standards

| Standard | Role |
|---|---|
| SD-01 Gungnir canonical schema v1 | The journal, the interface, configuration |
| SD-05 JSON | Journal lines, configuration, interface payloads |
| SD-02 Apache Arrow | Detections for analytics and bulk exchange |
| SD-03 ASTERIX category 048 | Radar feeds through the gateway, planned |
| SD-04 STANAG 4676 | Coalition track exchange, planned |
| SD-09 and SD-10 Automatic identification system and automatic dependent surveillance | Cooperative identity, planned |
| SD-11 UUID version 7 | The global entity identifier, decided and not yet in the manifest |

Version negotiation is exact-match today; minor-version tolerance becomes a policy
decision when a second version exists.

## 7. Data gaps

| Gap | What is missing |
|---|---|
| GAP-069 | The identifier crate is decided and not in the manifest, so the global entity identifier is still a raw newtype |
| GAP-019 | Cross-session identity correlation; the resolver correlates by session track identifier only |
| GAP-008 | Clock-skew detection across sources. The test-track work demonstrated why this matters: skew interacts with the gateway's latency window |
| GAP-062 | Releasability marking and enforcement |
| GAP-064 | The industry codecs; until they exist, provenance from a real radar feed is untested |
| GAP-079 | The dataset pipeline that turns journals and test tracks into training data |

## Traceability

`../../uaf/information/`; `../../../gungnir-api-v1.md`;
`../../../../ARCHITECTURE.md` §7.2; `../../../ml/data-pipeline.md`;
`../../../mission/gap-analysis/technical-gap-map.md`.
