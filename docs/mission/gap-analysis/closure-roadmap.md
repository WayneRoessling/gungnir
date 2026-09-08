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
| 18 | GAP-092 The journal budget's debug cost was attributed to runner I/O; it is the encode | Technical | 16 | S | Services engineer |  |
| 19 | GAP-069 `uuid` v7 for `GlobalEntityId` | Technical | 10 | S | Services engineer | D-11 |
| 20 | GAP-013 Track-to-track fusion and sensor registration | Technical | 8 | L | Tracking engineer (human-owned crate) | GAP-011 |
| 21 | GAP-051 Session lifecycle in `gungnir-mission` | Technical | 6 | M | Services engineer |  |
| 22 | GAP-094 The advisories gate fails, and one finding is a memory-disclosure vulnerability | Technical | 4 | M | Owner | D-10 |
| 23 | GAP-061 Release workflow unexercised | Technical | 3 | M | Owner | D-10 |
| 24 | GAP-093 Gate 6 never saves a baseline, so it compares nothing and cannot fail | Technical | 24 | S | Services engineer | GAP-061 |
| 25 | GAP-081 Architecture compliance checks not automated | Technical | 3 | M | Owner |  |

## I3: Close the decision loop. Closes the allocator, geometry, asset list, policy model, queue, and the panels that let a human decide in the product.

| Order | Gap | Type | Priority | Effort | Owner | Depends on |
|---|---|---|---|---|---|---|
| 1 | GAP-096 Bearing-only detections never reach the operator | Technical | 40 | M | UI engineer |  |
| 2 | GAP-004 Outbound sensor control path | Technical | 36 | L | Services engineer | GAP-001, D-08 |
| 3 | GAP-068 Adopted roles in code | Technical | 30 | S | Security engineer (human-owned crate) | D-05 |
| 4 | GAP-089 A seeded session for usability rounds | Technical | 30 | M | UI engineer | D-28 |
| 5 | GAP-074 Usability test rounds and MOP-37 targets | Technical | 30 | M | Plan 06 lead | D-16, GAP-089 |
| 6 | GAP-088 Geofences have no configuration source | Technical | 28 | S | Services engineer |  |
| 7 | GAP-028 Decision-loop crates not wired | Technical | 28 | M | Services engineer |  |
| 8 | GAP-059 Audit wiring | Technical | 36 | M | Security engineer (human-owned crate) | GAP-028 |
| 9 | GAP-034 Escalation and timeout for pending decisions | Technical | 28 | M | Security engineer (human-owned crate) | D-15 |
| 10 | GAP-052 Policy configuration and plan validity | Technical | 28 | M | Security engineer (human-owned crate) |  |
| 11 | GAP-033 Weapons control status and engagement authority | Technical | 35 | M | Security engineer (human-owned crate) | GAP-052, D-05 |
| 12 | GAP-058 Per-class and per-layer authorization | Technical | 36 | M | Security engineer (human-owned crate) | GAP-033, D-05 |
| 13 | GAP-018 Per-class identification thresholds | Technical | 28 | M | Security engineer (human-owned crate) | GAP-052 |
| 14 | GAP-005 Collection requirements and tasking workflow | Technical | 27 | M | Services engineer | GAP-003 |
| 15 | GAP-039 No-execution-without-decision verification | Technical | 24 | S | Services engineer | GAP-028 |
| 16 | GAP-022 three-d scene attachment | Technical | 24 | L | UI engineer |  |
| 17 | GAP-070 Display vocabulary and override table | Technical | 20 | S | UI engineer | D-12 |
| 18 | GAP-030 Effector layer and cost model | Technical | 20 | M | Services engineer |  |
| 19 | GAP-029 Allocator | Technical | 25 | XL | Tracking engineer (human-owned crate) | GAP-011, GAP-030 |
| 20 | GAP-043 Engagement tracking and effect assessment | Technical | 20 | L | Services engineer | GAP-028 |
| 21 | GAP-026 Defended-asset list | Technical | 16 | M | Services engineer |  |
| 22 | GAP-020 Trajectory prediction and closest point of approach | Technical | 20 | M | Services engineer | GAP-011, GAP-026 |
| 23 | GAP-027 Lethality by class and asset weighting | Technical | 20 | M | Services engineer | GAP-026 |
| 24 | GAP-031 Intercept geometry solver | Technical | 15 | L | Services engineer | GAP-030, GAP-029 |
| 25 | GAP-006 Coverage-gap detection | Technical | 12 | M | Services engineer | GAP-003 |
| 26 | GAP-032 Alternatives and what-if execution | Technical | 12 | M | Services engineer | GAP-029 |
| 27 | GAP-042 Warning function | Technical | 12 | M | Services engineer | GAP-020, GAP-026, D-08 |
| 28 | GAP-012 Staleness policy per class | Technical | 10 | S | Services engineer | GAP-052 |
| 29 | GAP-019 Cross-session identity correlation | Technical | 9 | L | Services engineer | GAP-011 |
| 30 | GAP-007 Coverage rendering on the map | Technical | 8 | M | UI engineer | GAP-022 |
| 31 | GAP-035 Queue ordering and pre-delegation | Technical | 8 | M | Security engineer (human-owned crate) | GAP-027, D-15 |
| 32 | GAP-037 Sensor re-tasking recommendation | Technical | 8 | M | Services engineer | GAP-003, GAP-006 |
| 33 | GAP-015 Random finite set filters | Technical | 8 | L | Tracking engineer (human-owned crate) | GAP-011 |
| 34 | GAP-021 Track and feed anomaly detection | Technical | 6 | M | Services engineer | D-13 |
| 35 | GAP-062 Releasability marking | Technical | 6 | M | Security engineer (human-owned crate) | D-06 |
| 36 | GAP-091 No exchange bearer for a participant that holds no machine identity | Technical | 20 | L | Services engineer | GAP-062, D-33 |
| 37 | GAP-017 Static hazard and barrier layer | Technical | 4 | S | Services engineer |  |
| 38 | GAP-067 Operational-readiness verification | Technical | 4 | L | Owner | D-16, D-10 |
| 39 | GAP-045 Scenario replay through the live pipeline | Technical | 3 | M | Services engineer | GAP-011, GAP-046, GAP-051 |
| 40 | GAP-087 PN-16, the planning panel | Technical | 30 | L | UI engineer | GAP-045 |
| 41 | GAP-055 Role workspaces in the UI | Technical | 30 | L | UI engineer | D-05, D-12, GAP-087 |
| 42 | GAP-072 Status strip on every layout | Technical | 32 | S | UI engineer | GAP-055 |
| 43 | GAP-073 Evidence card, commander summary, table columns, and theme additions | Technical | 21 | M | UI engineer | GAP-055 |
| 44 | GAP-038 Approval queue and decision panel | Technical | 20 | M | UI engineer | GAP-028, GAP-055 |
| 45 | GAP-075 Docking and multi-window | Technical | 20 | M | UI engineer | D-17, GAP-055 |
| 46 | GAP-071 Replay, reports, and configuration editor panels | Technical | 6 | M | UI engineer | GAP-055 |
| 47 | GAP-047 Measures computed from the journal | Technical | 3 | M | Services engineer | GAP-048, D-16 |
| 48 | GAP-036 Fires plan type and deconfliction | Technical | 3 | L | Security engineer (human-owned crate) | D-07, GAP-030 |
| 49 | GAP-090 The friendly set is only the friendlies a sensor detected | Technical | 12 | M | Security engineer (human-owned crate) | GAP-036, GAP-091, D-08 |
| 50 | GAP-079 Dataset pipeline from test tracks and journals | Technical | 2 | M | Services engineer | GAP-046 |
| 51 | GAP-083 Requirement identifiers not traceable from code | Technical | 2 | M | Owner |  |
| 52 | GAP-086 Mission profiles and candidate algorithm baselines in the schema | Technical | 2 | M | Services engineer |  |
| 53 | GAP-053 Model governance not wired | Technical | 2 | S | Services engineer | GAP-011, GAP-086 |

## I4: Operationalize and scale. Closes the transport and everything that waits on it: peers, handoff, failover, authentication for callers, encryption, releasability.

| Order | Gap | Type | Priority | Effort | Owner | Depends on |
|---|---|---|---|---|---|---|
| 1 | GAP-057 Authentication implementation | Technical | 40 | L | Security engineer (human-owned crate) | GAP-033, D-05 |
| 2 | GAP-040 Effector handoff endpoint | Technical | 20 | M | Services engineer | GAP-041, D-08 |
| 3 | GAP-095 Night theme variant | Technical | 20 | M | UI engineer | D-35 |
| 4 | GAP-009 Peer track and warning ingestion | Technical | 20 | L | Services engineer | GAP-041, D-08 |
| 5 | GAP-065 Peer and coalition exchange | Technical | 20 | L | Services engineer | GAP-009, GAP-041, GAP-062, D-08 |
| 6 | GAP-063 Interface conformance suite | Technical | 18 | M | Services engineer | GAP-041, GAP-064, D-09 |
| 7 | GAP-044 Assistant integration | Mission | 18 | XL | Plan 08 lead | D-14 |
| 8 | GAP-024 Point-cloud registration, CPU reference and GPU | Technical | 16 | L | Data engineer | GAP-023 |
| 9 | GAP-014 Registration evidence into the tracker | Technical | 24 | M | Tracking engineer (human-owned crate) | GAP-013, GAP-024 |
| 10 | GAP-077 `gungnir-ml` crate, inference runtime sign-off, and the dependency edge | Technical | 14 | L | Services engineer |  |
| 11 | GAP-025 Pattern-of-life and order-of-battle products | Technical | 9 | L | Services engineer | GAP-019 |
| 12 | GAP-084 Key custody, rotation, and escrow | Technical | 8 | M | Security engineer (human-owned crate) | D-02 |
| 13 | GAP-060 Encryption in transit and at rest | Technical | 8 | L | Security engineer (human-owned crate) | D-10 |
| 14 | GAP-050 Mid-session failover and reconciliation gate | Technical | 4 | M | Services engineer | GAP-041, D-03, D-15, GAP-057 |
| 15 | GAP-078 Model manifests as `gungnir-modelops` baselines | Technical | 3 | M | Services engineer | GAP-053 |
| 16 | GAP-080 First two models trained, evaluated, and promoted | Mission | 14 | L | Services engineer | GAP-077, GAP-078, GAP-079 |
| 17 | GAP-049 After-action review workflow | Technical | 2 | M | Services engineer | GAP-047 |
| 18 | GAP-054 Battle-rhythm support | Technical | 2 | M | Services engineer | GAP-047, GAP-044 |

## Critical path (rewritten 2026-09-07, in the development-status review)

Every gap the previous version of this section named -- GAP-016, GAP-048, GAP-011,
GAP-030, GAP-029, GAP-031, GAP-028, GAP-038, GAP-052, GAP-012, GAP-018, GAP-033,
GAP-035, GAP-058, GAP-041 -- is Closed, and every decision it named (D-02, D-05,
D-08, D-09, D-15, D-16) is Resolved. The path below is traced fresh from the
`deps` still open in the register today, not carried forward from the section this
replaces.
- **GAP-064** (the STANAG 4676 codec; ASTERIX is done) gates **GAP-001** (live sensor
  adapters, priority 45, the single highest-priority open gap), which gates **GAP-004**
  (outbound sensor control). GAP-064 also gates **GAP-063** (interface conformance).
- **GAP-061** (the self-hosted runner) gates **GAP-093** (Gate 6's threshold
  enforcement) and is itself waited on by nothing else open.
- **GAP-045** (scenario replay through the live pipeline) gates **GAP-087** (PN-16,
  the planning panel), already in progress on the rest of its scope.
- **GAP-023** (data loaders) gates **GAP-024** (GPU point-cloud registration); the
  CPU reference side of GAP-024 is already built and gated.
- **GAP-077**, **GAP-078**, and **GAP-079** (in progress) all feed **GAP-080** (the
  first two trained models); none of the three has a gap of its own blocking it.
- **GAP-067** (this table's own walk) is blocked on nothing: D-16 and D-10, its two
  named decisions, are both Resolved. It is now purely a matter of the owner's time
  against 43 rows, not a dependency.

## Counts

| Increment | Gaps | Effort S / M / L / XL |
|---|---|---|
| I2 | 25 | 5 / 11 / 7 / 2 |
| I3 | 53 | 8 / 33 / 11 / 1 |
| I4 | 18 | 0 / 9 / 8 / 1 |
