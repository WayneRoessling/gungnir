# Closure roadmap

Status: first draft, 2026-09-04. Gaps by target increment, ordered by priority
(severity times reach) and then by effort, smaller first, so that within an
increment the cheapest high-priority closures come first. Dependencies are listed
so nothing is scheduled before what it needs; the generator checked that no gap
targets an earlier increment than any gap it depends on and that there are no
cycles. Increments are those of `../../gungnir-capabilities.md` §7 and the
capability roadmap in `../capabilities/capability-roadmap.md`.

## I2: Integrate real data. Closes the tracking mathematics, the data path from real sensors, and the harnesses that make everything after it measurable.

| Order | Gap | Type | Priority | Effort | Owner | Depends on |
|---|---|---|---|---|---|---|
| 1 | GAP-064 ASTERIX and STANAG 4676 codecs | Technical | 36 | L | Services engineer | D-09 |
| 2 | GAP-001 Live sensor adapters | Technical | 45 | XL | Services engineer | GAP-064, D-08 |
| 3 | GAP-016 Scenario generator | Technical | 32 | L | Tracking engineer (human-owned crate) |  |
| 4 | GAP-056 Performance harnesses | Technical | 32 | M | Services engineer | GAP-016, D-04 |
| 5 | GAP-046 Test-track suite | Mission | 32 | L | Plan 07 lead | GAP-016 |
| 6 | GAP-085 Journal append misses its budget: no buffering | Technical | 30 | S | Services engineer | D-04, GAP-056 |
| 7 | GAP-041 API transport | Technical | 30 | L | Services engineer | D-02, D-18 |
| 8 | GAP-076 Test-track integration: fuzz corpus, benchmark inputs, end-to-end replay | Technical | 27 | S | Services engineer | GAP-046 |
| 9 | GAP-002 Source authentication for live feeds | Technical | 27 | M | Services engineer | D-02 |
| 10 | GAP-003 Sensor management not wired | Technical | 27 | M | Services engineer |  |
| 11 | GAP-048 Tracking metrics | Technical | 24 | M | Tracking engineer (human-owned crate) |  |
| 12 | GAP-011 Tracking pipeline | Technical | 40 | XL | Tracking engineer (human-owned crate) | GAP-016, GAP-048 |
| 13 | GAP-066 Service contracts too thin | Technical | 24 | M | Services engineer |  |
| 14 | GAP-082 `todo!()` reachability unproven | Technical | 24 | M | Services engineer |  |
| 15 | GAP-010 Cooperative identity decoders | Technical | 24 | L | Services engineer | D-09 |
| 16 | GAP-023 Data loaders per format | Technical | 24 | L | UI engineer |  |
| 17 | GAP-008 Clock-skew detection across sources | Technical | 21 | M | Services engineer |  |
| 18 | GAP-069 `uuid` v7 for `GlobalEntityId` | Technical | 10 | S | Services engineer | D-11 |
| 19 | GAP-013 Track-to-track fusion and sensor registration | Technical | 8 | L | Tracking engineer (human-owned crate) | GAP-011 |
| 20 | GAP-051 Session lifecycle in `gungnir-mission` | Technical | 6 | M | Services engineer |  |
| 21 | GAP-061 Release workflow unexercised | Technical | 3 | M | Owner | D-10 |
| 22 | GAP-081 Architecture compliance checks not automated | Technical | 3 | M | Owner |  |

## I3: Close the decision loop. Closes the allocator, geometry, asset list, policy model, queue, and the panels that let a human decide in the product.

| Order | Gap | Type | Priority | Effort | Owner | Depends on |
|---|---|---|---|---|---|---|
| 1 | GAP-004 Outbound sensor control path | Technical | 36 | L | Services engineer | GAP-001, D-08 |
| 2 | GAP-068 Adopted roles in code | Technical | 30 | S | Security engineer (human-owned crate) | D-05 |
| 3 | GAP-089 A seeded session for usability rounds | Technical | 30 | M | UI engineer | D-28 |
| 4 | GAP-074 Usability test rounds and MOP-37 targets | Technical | 30 | M | Plan 06 lead | D-16, GAP-089 |
| 5 | GAP-088 Geofences have no configuration source | Technical | 28 | S | Services engineer |  |
| 6 | GAP-028 Decision-loop crates not wired | Technical | 28 | M | Services engineer |  |
| 7 | GAP-059 Audit wiring | Technical | 36 | M | Security engineer (human-owned crate) | GAP-028 |
| 8 | GAP-034 Escalation and timeout for pending decisions | Technical | 28 | M | Security engineer (human-owned crate) | D-15 |
| 9 | GAP-052 Policy configuration and plan validity | Technical | 28 | M | Security engineer (human-owned crate) |  |
| 10 | GAP-033 Weapons control status and engagement authority | Technical | 35 | M | Security engineer (human-owned crate) | GAP-052, D-05 |
| 11 | GAP-058 Per-class and per-layer authorization | Technical | 36 | M | Security engineer (human-owned crate) | GAP-033, D-05 |
| 12 | GAP-018 Per-class identification thresholds | Technical | 28 | M | Security engineer (human-owned crate) | GAP-052 |
| 13 | GAP-005 Collection requirements and tasking workflow | Technical | 27 | M | Services engineer | GAP-003 |
| 14 | GAP-039 No-execution-without-decision verification | Technical | 24 | S | Services engineer | GAP-028 |
| 15 | GAP-022 three-d scene attachment | Technical | 24 | L | UI engineer |  |
| 16 | GAP-070 Display vocabulary and override table | Technical | 20 | S | UI engineer | D-12 |
| 17 | GAP-030 Effector layer and cost model | Technical | 20 | M | Services engineer |  |
| 18 | GAP-029 Allocator | Technical | 25 | XL | Tracking engineer (human-owned crate) | GAP-011, GAP-030 |
| 19 | GAP-043 Engagement tracking and effect assessment | Technical | 20 | L | Services engineer | GAP-028 |
| 20 | GAP-026 Defended-asset list | Technical | 16 | M | Services engineer |  |
| 21 | GAP-020 Trajectory prediction and closest point of approach | Technical | 20 | M | Services engineer | GAP-011, GAP-026 |
| 22 | GAP-027 Lethality by class and asset weighting | Technical | 20 | M | Services engineer | GAP-026 |
| 23 | GAP-031 Intercept geometry solver | Technical | 15 | L | Services engineer | GAP-030, GAP-029 |
| 24 | GAP-006 Coverage-gap detection | Technical | 12 | M | Services engineer | GAP-003 |
| 25 | GAP-032 Alternatives and what-if execution | Technical | 12 | M | Services engineer | GAP-029 |
| 26 | GAP-042 Warning function | Technical | 12 | M | Services engineer | GAP-020, GAP-026, D-08 |
| 27 | GAP-012 Staleness policy per class | Technical | 10 | S | Services engineer | GAP-052 |
| 28 | GAP-019 Cross-session identity correlation | Technical | 9 | L | Services engineer | GAP-011 |
| 29 | GAP-007 Coverage rendering on the map | Technical | 8 | M | UI engineer | GAP-022 |
| 30 | GAP-035 Queue ordering and pre-delegation | Technical | 8 | M | Security engineer (human-owned crate) | GAP-027, D-15 |
| 31 | GAP-037 Sensor re-tasking recommendation | Technical | 8 | M | Services engineer | GAP-003, GAP-006 |
| 32 | GAP-015 Random finite set filters | Technical | 8 | L | Tracking engineer (human-owned crate) | GAP-011 |
| 33 | GAP-021 Track and feed anomaly detection | Technical | 6 | M | Services engineer | D-13 |
| 34 | GAP-062 Releasability marking | Technical | 6 | M | Security engineer (human-owned crate) | D-06 |
| 35 | GAP-091 No exchange bearer for a participant that holds no machine identity | Technical | 20 | L | Services engineer | GAP-062, D-33 |
| 36 | GAP-017 Static hazard and barrier layer | Technical | 4 | S | Services engineer |  |
| 37 | GAP-067 Operational-readiness verification | Technical | 4 | L | Owner | D-16, D-10 |
| 38 | GAP-045 Scenario replay through the live pipeline | Technical | 3 | M | Services engineer | GAP-011, GAP-046, GAP-051 |
| 39 | GAP-087 PN-16, the planning panel | Technical | 30 | L | UI engineer | GAP-045 |
| 40 | GAP-055 Role workspaces in the UI | Technical | 30 | L | UI engineer | D-05, D-12, GAP-087 |
| 41 | GAP-072 Status strip on every layout | Technical | 32 | S | UI engineer | GAP-055 |
| 42 | GAP-073 Evidence card, commander summary, table columns, and theme additions | Technical | 21 | M | UI engineer | GAP-055 |
| 43 | GAP-038 Approval queue and decision panel | Technical | 20 | M | UI engineer | GAP-028, GAP-055 |
| 44 | GAP-075 Docking and multi-window | Technical | 20 | M | UI engineer | D-17, GAP-055 |
| 45 | GAP-071 Replay, reports, and configuration editor panels | Technical | 6 | M | UI engineer | GAP-055 |
| 46 | GAP-047 Measures computed from the journal | Technical | 3 | M | Services engineer | GAP-048, D-16 |
| 47 | GAP-036 Fires plan type and deconfliction | Technical | 3 | L | Security engineer (human-owned crate) | D-07, GAP-030 |
| 48 | GAP-090 The friendly set is only the friendlies a sensor detected | Technical | 12 | M | Security engineer (human-owned crate) | GAP-036, GAP-091, D-08 |
| 49 | GAP-079 Dataset pipeline from test tracks and journals | Technical | 2 | M | Services engineer | GAP-046 |
| 50 | GAP-083 Requirement identifiers not traceable from code | Technical | 2 | M | Owner |  |
| 51 | GAP-086 Mission profiles and candidate algorithm baselines in the schema | Technical | 2 | M | Services engineer |  |
| 52 | GAP-053 Model governance not wired | Technical | 2 | S | Services engineer | GAP-011, GAP-086 |

## I4: Operationalize and scale. Closes the transport and everything that waits on it: peers, handoff, failover, authentication for callers, encryption, releasability.

| Order | Gap | Type | Priority | Effort | Owner | Depends on |
|---|---|---|---|---|---|---|
| 1 | GAP-057 Authentication implementation | Technical | 40 | L | Security engineer (human-owned crate) | GAP-033, D-05 |
| 2 | GAP-040 Effector handoff endpoint | Technical | 20 | M | Services engineer | GAP-041, D-08 |
| 3 | GAP-009 Peer track and warning ingestion | Technical | 20 | L | Services engineer | GAP-041, D-08 |
| 4 | GAP-065 Peer and coalition exchange | Technical | 20 | L | Services engineer | GAP-009, GAP-041, GAP-062, D-08 |
| 5 | GAP-063 Interface conformance suite | Technical | 18 | M | Services engineer | GAP-041, GAP-064, D-09 |
| 6 | GAP-044 Assistant integration | Mission | 18 | XL | Plan 08 lead | D-14 |
| 7 | GAP-024 Point-cloud registration, CPU reference and GPU | Technical | 16 | L | Data engineer | GAP-023 |
| 8 | GAP-014 Registration evidence into the tracker | Technical | 24 | M | Tracking engineer (human-owned crate) | GAP-013, GAP-024 |
| 9 | GAP-077 `gungnir-ml` crate, inference runtime sign-off, and the dependency edge | Technical | 14 | L | Services engineer |  |
| 10 | GAP-025 Pattern-of-life and order-of-battle products | Technical | 9 | L | Services engineer | GAP-019 |
| 11 | GAP-084 Key custody, rotation, and escrow | Technical | 8 | M | Security engineer (human-owned crate) | D-02 |
| 12 | GAP-060 Encryption in transit and at rest | Technical | 8 | L | Security engineer (human-owned crate) | D-10 |
| 13 | GAP-050 Mid-session failover and reconciliation gate | Technical | 4 | M | Services engineer | GAP-041, D-03, D-15, GAP-057 |
| 14 | GAP-078 Model manifests as `gungnir-modelops` baselines | Technical | 3 | M | Services engineer | GAP-053 |
| 15 | GAP-080 First two models trained, evaluated, and promoted | Mission | 14 | L | Services engineer | GAP-077, GAP-078, GAP-079 |
| 16 | GAP-049 After-action review workflow | Technical | 2 | M | Services engineer | GAP-047 |
| 17 | GAP-054 Battle-rhythm support | Technical | 2 | M | Services engineer | GAP-047, GAP-044 |

## Critical path

- GAP-016 scenario generator and GAP-048 metrics, then GAP-011 the pipeline, gate every
  Understand and Decide closure; they are I2's first work.
- GAP-030 layers and costs, then GAP-029 the allocator and GAP-031 geometry, gate the
  policy checks on geometry, GAP-028 wiring, GAP-038 the panel, and GAP-040 handoff.
- GAP-052 policy configuration gates GAP-012, GAP-018, GAP-033, and through them
  GAP-035 and GAP-058.
- GAP-041 the transport gates GAP-009, GAP-040, GAP-050, GAP-057, GAP-060, GAP-063,
  and GAP-065; D-02 gates GAP-041.
- Decisions D-02, D-05, D-08, D-09, D-15, and D-16 are on the path of I3 work and are
  the owner's first items.

## Counts

| Increment | Gaps | Effort S / M / L / XL |
|---|---|---|
| I2 | 22 | 3 / 10 / 7 / 2 |
| I3 | 52 | 8 / 32 / 11 / 1 |
| I4 | 17 | 0 / 8 / 8 / 1 |
