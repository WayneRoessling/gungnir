# Technical gap map

Status: first draft, 2026-09-04. The technical gaps from `gap-register.md` grouped by
architecture layer (`../../../ARCHITECTURE.md` §1 to §8), each pointing at the
`../../../ARCHITECTURE.md` §10 item or the `../../verification-capability-table.md` row that
tracks it. Mission gaps appear only in the register and the roadmap.

## §10 open items and the gaps that carry them

| `../../../ARCHITECTURE.md` §10 open item | Gaps or decisions |
|---|---|
| Scope lock | D-01 |
| The tracking math itself | GAP-011, GAP-013, GAP-015, GAP-016, GAP-029 |
| Intercept geometry | GAP-031 |
| three-d attachment | GAP-022 |
| API transport libraries | GAP-041, GAP-050 |
| Live protocol adapters and industry codecs | GAP-001, GAP-010, GAP-064 |
| Credential mechanism | D-02, GAP-057 |
| Locking the reconciliation and arbitration rules; release-governance items | D-03, D-10 |
| Performance budgets | D-04, GAP-056 |
| Cross-session identity correlation | GAP-019 |
| `uuid` for `GlobalEntityId` | D-11 |

## By layer

### Tracking core

| Gap | Capability | Sev | Effort | Target | Tracked in |
|---|---|---|---|---|---|
| GAP-011 Tracking pipeline | CAP-2.1, CAP-2.2, CAP-2.5 | 5 | XL | I2 | `../../../ARCHITECTURE.md` §10, the tracking math itself; `../../verification-capability-table.md` §1 |
| GAP-013 Track-to-track fusion and sensor registration | CAP-2.3 | 4 | L | I2 | `../../../ARCHITECTURE.md` §10, the tracking math itself |
| GAP-015 Random finite set filters | CAP-2.4 | 4 | L | I3 | `../../../ARCHITECTURE.md` §10, the tracking math itself |
| GAP-016 Scenario generator | CAP-2.5, CAP-2.1 | 4 | L | I2 | `../../../ARCHITECTURE.md` §10, the tracking math itself |
| GAP-029 Allocator | CAP-3.3 | 5 | XL | I3 | `../../../ARCHITECTURE.md` §10, the tracking math itself |
| GAP-031 Intercept geometry solver | CAP-3.4 | 5 | L | I3 | `gungnir-intercept-service/src/geometry.rs`; `../../design/DN-04-effector-model.md` §9 |
| GAP-048 Tracking metrics | CAP-5.3, CAP-2.1 | 3 | M | I2 | `../../verification-capability-table.md` §1, metrics rows |

### Service facades and wiring

| Gap | Capability | Sev | Effort | Target | Tracked in |
|---|---|---|---|---|---|
| GAP-028 Decision-loop crates not wired | CAP-3.2, CAP-3.6, CAP-4.2 | 4 | M | I3 | `../../verification-capability-table.md` §2, `gungnir-policy` and `gungnir-command` rows |
| GAP-045 Scenario replay through the live pipeline | CAP-5.2 | 3 | M | I3 | `../../verification-capability-table.md` §2, `gungnir-mission` row |
| GAP-066 Service contracts too thin | CAP-7.1, CAP-2.1 | 3 | M | I2 | `../../../ARCHITECTURE.md` §7.2 |

### Productization: sensing and time

| Gap | Capability | Sev | Effort | Target | Tracked in |
|---|---|---|---|---|---|
| GAP-001 Live sensor adapters | CAP-1.1 | 5 | XL | I2 | `../../../ARCHITECTURE.md` §10 item 81; `../../design/handoff-2026-09-06-radar-feed.md` (what to do next, in order); `../../design/external-standards.md` §1 |
| GAP-002 Source authentication for live feeds | CAP-1.2 | 3 | M | I2 | `../../verification-capability-table.md` §2, `gungnir-ingest` row |
| GAP-003 Sensor management not wired | CAP-1.3 | 3 | M | I2 | `../../verification-capability-table.md` §2, `gungnir-sensor-management` row |
| GAP-004 Outbound sensor control path | CAP-1.3 | 4 | L | I3 | `../../../ARCHITECTURE.md` §10 item 57 |
| GAP-005 Collection requirements and tasking workflow | CAP-1.3, CAP-2.12 | 3 | M | I3 | `../../../ARCHITECTURE.md` §10 item 58 |
| GAP-008 Clock-skew detection across sources | CAP-1.5 | 3 | M | I2 | `../../verification-capability-table.md` §2, `gungnir-time` row |
| GAP-010 Cooperative identity decoders | CAP-1.7 | 4 | L | I2 | `../../design/external-standards.md` |
| GAP-099 ISR video metadata (MISB ST 0601 KLV) has no adapter | CAP-1.1 | 3 | M | I3 | `../../design/external-standards.md` §8 and §8.2; GAP-001 (the closing action this gap is drawn from) |
| GAP-100 ASTERIX Category 205 direction-finder bearings | CAP-7.2, CAP-1.1 | 3 | M | I3 | `../../../ARCHITECTURE.md` §10 item 114; GAP-001 (the survey and the sibling feeds); GAP-064 (the ASTERIX/STANAG codec family this extends); GAP-096 (why a bearing produced here is not yet shown) |
| GAP-101 ASTERIX Category 129 UAS identification reports | CAP-1.7 | 3 | M | I3 | `../../../ARCHITECTURE.md` §10 item 114; GAP-001 (the survey and the sibling feeds); GAP-064 (the ASTERIX/STANAG codec family this extends); GAP-100 (the sibling gap this one was left open beside, and the pattern it mirrors) |
| GAP-103 Sensor positions reached the tracker as geodetic radians | CAP-1.1, CAP-2.1 | 4 | S | I2 | `../../../ARCHITECTURE.md` §10 item 121; GAP-001 (whose closing action built the resolver this defeated); GAP-096 (whose bearing rays are drawn from this position); `../../design/DN-27-bearing-only-detections.md` §2 |

### Productization: picture and identity

| Gap | Capability | Sev | Effort | Target | Tracked in |
|---|---|---|---|---|---|
| GAP-006 Coverage-gap detection | CAP-1.4 | 3 | M | I3 | Added to `../../../ARCHITECTURE.md` §10 by this register |
| GAP-012 Staleness policy per class | CAP-2.2 | 2 | S | I3 | Added to `../../../ARCHITECTURE.md` §10 by this register |
| GAP-014 Registration evidence into the tracker | CAP-2.3, CAP-2.10 | 3 | M | I4 | Added to `../../../ARCHITECTURE.md` §10 by this register |
| GAP-017 Static hazard and barrier layer | CAP-2.5 | 2 | S | I3 | Added to `../../../ARCHITECTURE.md` §10 by this register |
| GAP-019 Cross-session identity correlation | CAP-2.7 | 3 | L | I3 | `../../../ARCHITECTURE.md` §10, cross-session identity correlation |
| GAP-021 Track and feed anomaly detection | CAP-2.9 | 3 | M | I3 | Added to `../../../ARCHITECTURE.md` §10 by this register |
| GAP-025 Pattern-of-life and order-of-battle products | CAP-2.12 | 3 | L | I4 | Added to `../../../ARCHITECTURE.md` §10 by this register |
| GAP-069 `uuid` v7 for `GlobalEntityId` | CAP-2.7, CAP-7.2 | 2 | S | I2 | `docs/agentic-coding-standards.md` §2.9 |

### Productization: decision and policy

| Gap | Capability | Sev | Effort | Target | Tracked in |
|---|---|---|---|---|---|
| GAP-018 Per-class identification thresholds | CAP-2.6 | 4 | M | I3 | `../../verification-capability-table.md` §2, `gungnir-identification` row |
| GAP-020 Trajectory prediction and closest point of approach | CAP-2.8 | 4 | M | I3 | `../../design/DN-02-prediction-and-approach.md`; `gungnir-app/tests/predictions_and_warnings.rs` |
| GAP-026 Defended-asset list | CAP-3.1 | 4 | M | I3 | `../../design/DN-01-defended-assets.md` |
| GAP-027 Lethality by class and asset weighting | CAP-3.2 | 4 | M | I3 | `../../verification-capability-table.md` §2, `gungnir-assessment` row |
| GAP-030 Effector layer and cost model | CAP-3.3 | 4 | M | I3 | Added to `../../../ARCHITECTURE.md` §10 by this register |
| GAP-032 Alternatives and what-if execution | CAP-3.5 | 3 | M | I3 | `../../verification-capability-table.md` §2, `gungnir-decision` row |
| GAP-033 Weapons control status and engagement authority | CAP-3.6 | 5 | M | I3 | `../../verification-capability-table.md` §2, `gungnir-policy` row |
| GAP-034 Escalation and timeout for pending decisions | CAP-3.6, CAP-3.7 | 4 | M | I3 | `../../verification-capability-table.md` §2, `gungnir-command` row |
| GAP-035 Queue ordering and pre-delegation | CAP-3.7 | 4 | M | I3 | Added to `../../../ARCHITECTURE.md` §10 by this register |
| GAP-036 Fires plan type and deconfliction | CAP-3.8 | 3 | L | I3 | `../../verification-capability-table.md` §2, `gungnir-policy` fires row |
| GAP-037 Sensor re-tasking recommendation | CAP-3.9 | 4 | M | I3 | Added to `../../../ARCHITECTURE.md` §10 by this register |
| GAP-039 No-execution-without-decision verification | CAP-4.3 | 4 | S | I3 | `gungnir-app/tests/no_execution_without_decision.rs` |
| GAP-042 Warning function | CAP-4.5 | 4 | M | I3 | `../../design/DN-03-warning.md`; `gungnir-workflow/src/warning.rs`; `gungnir-app/tests/predictions_and_warnings.rs` |
| GAP-043 Engagement tracking and effect assessment | CAP-4.6 | 4 | L | I3 | Added to `../../../ARCHITECTURE.md` §10 by this register |
| GAP-088 Geofences have no configuration source | CAP-3.6 | 4 | S | I3 | `../../verification-capability-table.md` §2, `gungnir-policy` row |
| GAP-090 The friendly set is only the friendlies a sensor detected | CAP-3.8, CAP-4.5 | 4 | M | I3 | `../../design/DN-25-cursor-on-target.md`; `../../design/DN-05-fires.md` §5; `gungnir-policy/src/fires.rs` |
| GAP-097 An unchanged plan is re-proposed and re-queued every tick | CAP-3.3 | 4 | M | I3 | `../../ux/usability-round-1-session.md` §2, §3; GAP-074, GAP-089, GAP-045 |

### Productization: sustainment

| Gap | Capability | Sev | Effort | Target | Tracked in |
|---|---|---|---|---|---|
| GAP-085 Journal append misses its budget: no buffering | CAP-5.1, CAP-5.10 | 3 | S | I2 | `../../../ARCHITECTURE.md` §10, performance budgets |
| GAP-047 Measures computed from the journal | CAP-5.3 | 3 | M | I3 | `../../verification-capability-table.md` §2, `gungnir-reporting` row |
| GAP-049 After-action review workflow | CAP-5.3 | 2 | M | I4 | Added to `../../../ARCHITECTURE.md` §10 by this register |
| GAP-051 Session lifecycle in `gungnir-mission` | CAP-5.2, CAP-5.6 | 3 | M | I2 | `../../verification-capability-table.md` §2, `gungnir-mission` row |
| GAP-052 Policy configuration and plan validity | CAP-5.6, CAP-3.6 | 4 | M | I3 | `../../verification-capability-table.md` §2, `gungnir-config` row |
| GAP-086 Mission profiles and candidate algorithm baselines in the schema | CAP-5.7 | 2 | M | I3 | `../../verification-capability-table.md` §2, `gungnir-modelops` row |
| GAP-053 Model governance not wired | CAP-5.7 | 2 | S | I3 | `../../verification-capability-table.md` §2, `gungnir-modelops` row |
| GAP-054 Battle-rhythm support | CAP-5.8 | 2 | M | I4 | `../../verification-capability-table.md` §2, `gungnir-reporting` row |

### Productization: security

| Gap | Capability | Sev | Effort | Target | Tracked in |
|---|---|---|---|---|---|
| GAP-068 Adopted roles in code | CAP-6.2, CAP-5.9 | 3 | S | I3 | Added to `../../../ARCHITECTURE.md` §10 by this register |
| GAP-057 Authentication implementation | CAP-6.1 | 4 | L | I4 | `../../verification-capability-table.md` §2, `gungnir-security` row |
| GAP-058 Per-class and per-layer authorization | CAP-6.2 | 4 | M | I3 | `../../verification-capability-table.md` §2, `gungnir-security` row |
| GAP-059 Audit wiring | CAP-6.3 | 4 | M | I3 | `docs/architecture/togaf/phase-g-implementation-governance/architecture-contracts.md` C-04 |
| GAP-060 Encryption in transit and at rest | CAP-6.4 | 4 | L | I4 | `../../../ARCHITECTURE.md` §8.5; `../../design/DN-22-key-management.md` |
| GAP-084 Key custody, rotation, and escrow | CAP-6.4 | 4 | M | I4 | `../../plans/11-design-gap-closure.md` finding F-3 |
| GAP-062 Releasability marking | CAP-6.6 | 3 | M | I3 | `../../verification-capability-table.md` §2, `gungnir-security` row |

### Deployment and API

| Gap | Capability | Sev | Effort | Target | Tracked in |
|---|---|---|---|---|---|
| GAP-009 Peer track and warning ingestion | CAP-1.6 | 4 | L | I4 | `../../design/DN-16-peer-sources.md` §10 |
| GAP-040 Effector handoff endpoint | CAP-4.4 | 4 | M | I4 | `../../verification-capability-table.md` §2, `gungnir-api` handoff row |
| GAP-041 API transport | CAP-7.1, CAP-7.3, CAP-5.4 | 5 | L | I2 | `../../../ARCHITECTURE.md` §10 item 59 |
| GAP-050 Mid-session failover and reconciliation gate | CAP-5.4 | 4 | M | I4 | `../../verification-capability-table.md` §2, cross-layer reconciliation row |
| GAP-064 ASTERIX and STANAG 4676 codecs | CAP-7.2, CAP-1.1 | 4 | L | I2 | `../../../ARCHITECTURE.md` §10 item 81; `../../design/handoff-2026-09-06-radar-feed.md`; `../../verification-capability-table.md` §2, `gungnir-interop` row |
| GAP-065 Peer and coalition exchange | CAP-7.4 | 4 | L | I4 | `../../design/DN-18-coalition-exchange.md` amendment 2 |
| GAP-091 No exchange bearer for a participant that holds no machine identity | CAP-7.4, CAP-1.6 | 4 | L | I3 | `../../design/DN-25-cursor-on-target.md`; `../../design/external-standards.md` §5 |

### UI and data ecosystem

| Gap | Capability | Sev | Effort | Target | Tracked in |
|---|---|---|---|---|---|
| GAP-007 Coverage rendering on the map | CAP-1.4 | 2 | M | I3 | `../../verification-capability-table.md` §2, `gungnir-viewport3d` rows |
| GAP-022 three-d scene attachment | CAP-2.10 | 3 | L | I3 | `../../../ARCHITECTURE.md` §10, three-d attachment |
| GAP-023 Data loaders per format | CAP-2.10 | 3 | L | I2 | `../../verification-capability-table.md` §2, `gungnir-data` rows; `testdata/dem/SOURCE.md` |
| GAP-024 Point-cloud registration, CPU reference and GPU | CAP-2.10 | 2 | L | I3 | `../../verification-capability-table.md` §2, `gungnir-data-fusion` rows; `rust-3d-data-ecosystem-build-vs-adopt.md` §3.4 and §3.6; GAP-061 (the runner, done); GAP-098 (what feeds and displays the result) |
| GAP-038 Approval queue and decision panel | CAP-4.1 | 4 | M | I3 | `../../verification-capability-table.md` §2, `gungnir-ui` row |
| GAP-070 Display vocabulary and override table | CAP-5.9 | 2 | S | I3 | Added to `../../../ARCHITECTURE.md` §10 by this register |
| GAP-071 Replay, reports, and configuration editor panels | CAP-5.2, CAP-5.3, CAP-5.6 | 3 | M | I3 | `../../ux/ux-to-code-map.md` |
| GAP-072 Status strip on every layout | CAP-5.5, CAP-5.4, CAP-3.6 | 4 | S | I3 | `../../ux/ux-to-code-map.md` |
| GAP-073 Evidence card, commander summary, table columns, and theme additions | CAP-2.6, CAP-4.1, CAP-3.1 | 3 | M | I3 | `../../ux/ux-to-code-map.md` |
| GAP-075 Docking and multi-window | CAP-5.9 | 2 | M | I3 | `../../ux/ux-to-code-map.md` |
| GAP-087 PN-16, the planning panel | CAP-5.9, CAP-5.2 | 3 | L | I3 | `../../ux/wireframes/WF-16-planning.puml` |
| GAP-089 A seeded session for usability rounds | CAP-5.9 | 3 | M | I3 | `../../ux/usability-round-1-session.md` §3 |
| GAP-055 Role workspaces in the UI | CAP-5.9 | 3 | L | I3 | `../../plans/06-ux-design-by-role.md` |
| GAP-095 Night theme variant | CAP-5.9 | 2 | M | I4 | `../../../ARCHITECTURE.md` §10 item 89 (D-35 to D-38) |
| GAP-096 Bearing-only detections never reach the operator | CAP-2.1, CAP-5.9 | 4 | M | I3 | `../../design/DN-27-bearing-only-detections.md` §5 rule 3 and §7; `../../ux/ux-to-code-map.md` PN-02, PN-08, PN-09; GAP-001 (the three feeds this makes visible); GAP-011 (whose entry recorded §7 unbuilt); `docs/gungnir-api-v1.md` (the v2 wire's compatibility rule this closure follows) |
| GAP-098 No runtime point cloud: nothing loads one and nothing draws one | CAP-2.10 | 3 | M | I3 | `../../../ARCHITECTURE.md` §3 (the two contexts and the read-back); GAP-023 (the loaders and fixtures, reused rather than duplicated); GAP-024 (the registration engine, unblocked and independent of this) |
| GAP-102 A point cloud in a real-world CRS cannot be loaded | CAP-2.10 | 3 | M | I3 | D-41 (the decision this builds); GAP-023 (the DEM half, the sibling doing the same work for terrain); GAP-098 (the point-cloud capability this extends, closed and not reopened); `docs/agentic-coding-standards.md` §2.9 |

### Verification and governance

| Gap | Capability | Sev | Effort | Target | Tracked in |
|---|---|---|---|---|---|
| GAP-074 Usability test rounds and MOP-37 targets | CAP-5.9 | 3 | M | I3 | `../../ux/usability-test-plan.md` §7; `../../ux/usability-round-1-session.md` |
| GAP-076 Test-track integration: fuzz corpus, benchmark inputs, end-to-end replay | CAP-5.2, CAP-5.10, CAP-6.7 | 3 | S | I2 | `../../plans/07-test-track-suite.md` |
| GAP-077 `gungnir-ml` crate, inference runtime sign-off, and the dependency edge | CAP-2.6, CAP-5.7 | 2 | L | I4 | `../../plans/09-ml-model-integration.md`; `agentic-coding-standards.md` §2.9; D-40 |
| GAP-078 Model manifests as `gungnir-modelops` baselines | CAP-5.7 | 3 | M | I4 | `docs/ml/mlops.md` |
| GAP-079 Dataset pipeline from test tracks and journals | CAP-5.3, CAP-5.7 | 2 | M | I3 | `docs/ml/data-pipeline.md` |
| GAP-081 Architecture compliance checks not automated | CAP-5.7, CAP-6.5 | 3 | M | I2 | `docs/architecture/togaf/phase-g-implementation-governance/architecture-contracts.md` |
| GAP-082 `todo!()` reachability unproven | CAP-5.10 | 3 | M | I2 | `docs/architecture/togaf/phase-g-implementation-governance/compliance-assessment.md` |
| GAP-083 Requirement identifiers not traceable from code | CAP-5.7 | 2 | M | I3 | `docs/architecture/togaf/requirements-management/requirements-repository.md` §3 |
| GAP-056 Performance harnesses | CAP-5.10 | 4 | M | I2 | `../../../ARCHITECTURE.md` §10, performance budgets |
| GAP-061 Release workflow unexercised | CAP-6.5 | 3 | M | I2 | `../../release-governance.md` |
| GAP-063 Interface conformance suite | CAP-7.1, CAP-7.2 | 3 | M | I4 | `../../verification-capability-table.md` §2, cross-layer interop row |
| GAP-067 Operational-readiness verification | CAP-5.7 | 4 | L | I3 | `docs/architecture.md` |
| GAP-094 The advisories gate fails, and one finding is a memory-disclosure vulnerability | CAP-6.5 | 4 | M | I2 | `docs/release-governance.md`; `deny.toml` |
| GAP-092 The journal budget's debug cost was attributed to runner I/O; it is the encode | CAP-5.10 | 2 | S | I2 | `../../performance-budgets.md` |
| GAP-093 Gate 6 never saves a baseline, so it compares nothing and cannot fail | CAP-5.10 | 3 | S | I2 | `.github/workflows/bench-regression.yml` |

## Integration gaps

Gaps where two components exist but are not connected, found by the coverage pass:

- GAP-003 Sensor management not wired: **Closed 2026-09-05.** Both binaries construct `InMemorySensorRegistry` from the baseline, PN-10 (`sensor_management.rs`) shows and changes what each sensor is doing, and a mode change publishes `SensorEvent::ModeChanged` -- a new `Event` variant the reporting fold now counts. **The payoff is that GAP-006 and GAP-007 stop being nominal**: coverage comes from `SensorRegistry::coverage`, which reports only sensors that are searching or tracking, so the caveat those gaps carried is deleted rather than reworded. A fresh desktop consequently reports covering nothing, because `SensorRecord::from_config` starts every sensor at Standby -- which is true, and is the state PN-10 exists to change. `SensorMode` moved to `gungnir-model` and is re-exported here, because an event must carry it and the model cannot depend on a crate that depends on it; it also joined the display vocabulary. Nothing reached a sensor, and PN-10 said so on screen rather than letting a row that changed to Search imply a radar started searching. The outbound path was built later the same day (GAP-004), which is where that control moved: PN-10 now separates commanding a sensor from recording an observed mode, and it is the second of the two that this entry described.
- GAP-014 Registration evidence into the tracker: **Signed by the owner 2026-09-06**: `CalibrationEvent`, `ReferenceObservation` and the service-layer `RegistrationLedger` that joins them. **CLOSED 2026-09-06, and the action's wording was not followed literally because it would have broken the layering.** It asked for a calibration event in `gungnir-model` consumed in `gungnir-track-fusion` registration. `gungnir-track-fusion` is a tracking-core crate (`../../../ARCHITECTURE.md` §7.1 allows it `track` and `coord`), `gungnir-model` is the foundation layer, and `CLAUDE.md` fixes the direction one-way -- so a core crate consuming a model event is an upward edge, and adding one to satisfy a register entry would be the register overruling the architecture. Instead: `gungnir_model::CalibrationEvent` is the event where events belong, `gungnir_track_fusion::ReferenceObservation` is the same information in a form the core crate owns, and `gungnir_tracking_service::RegistrationLedger` -- in the one crate that already depends on both -- is the join. **No edge was added.** The evidence is a surveyed reference rather than a second platform, and that is the point: registering two platforms against each other recovers their offset from one another and says nothing about either one's offset from the world, so two platforms displaced identically look perfectly registered while the whole picture is in the wrong place. A registration whose residual spread says a single translation did not explain the disagreement is **refused and journalled as a refusal**, not applied. Point-cloud registration output (transform, uncertainty, calibration version) is not passed to the tracking side as bias evidence.
- GAP-028 Decision-loop crates not wired: **Closed 2026-09-06.** The desktop half closed earlier the same day: it constructs `gungnir-policy` and `gungnir-command` (GAP-038), `gungnir-assessment` (GAP-026) and `gungnir-decision` (GAP-037). The node half now: `gungnir-node` runs the policy chain on every fresh plan -- the geofence and control-status engines -- and publishes `InterceptEvent::PlanEvaluated` with the verdict and **the list of engines that ran**, so a desktop reading the node sees exactly what was and was not checked. The desktop publishes the same event with its three engines. **What the node deliberately does not do, and why**: it evaluates no authority, because that engine asks who is asking and nobody signs in to a node (GAP-057); and it holds no approval queue, because a decision is a person's act and DN-23 §4 left open whether a node should hold one at all -- a queue nobody can decide from is a queue that only expires. Two edges, (k) to `gungnir-policy` and (l) to `gungnir-geo`, the second anticipated by `../../../ARCHITECTURE.md` §8 from the first draft; accepted by the owner in `dependency-edges.md` §7a on 2026-09-06. `PolicyVerdict::summary()` in `gungnir-policy` (human-owned; signed by the owner 2026-09-06) replaced three private matches. **A finding, filed as GAP-088**: no baseline section declares a geofence, so the geofence engine on both binaries evaluates against an empty service and can only pass; PN-07's caveat had said so with a hard-coded `true`.
- GAP-053 Model governance not wired: **Closed 2026-09-06.** The promoted algorithm baseline reaches the filtering and only then is stamped, which is DN-24 §7's rule in both directions. `PipelineSettings::from_baseline` maps a baseline's gate threshold and filter selection onto the pipeline's settings; both binaries build one from the promoted candidate before they build the tracker, hand it to `LiveTrackingService::with_pipeline_settings`, and call `with_algorithm_baseline` **only** when the settings were actually built from that baseline. **A baseline naming a filter this build does not implement is refused by name** (`UnsupportedFilter`, listing what is implemented) rather than run as the default: the desktop raises an alert, the node logs an error, and every track stays `UNGOVERNED_ALGORITHM_VERSION`, so the governance record and the picture disagree visibly instead of quietly. That is the case the default baseline is in today, because it names `imm-cv-ct` . **Read this carefully now that GAP-011 has closed: `gungnir-filters` HAS an IMM as of 2026-09-06 and the pipeline still cannot run one.** `IMPLEMENTED_FILTERS` lists what the pipeline can apply, not what the filter crate contains, and the pipeline's `TrackFilter` is a fixed linear Kalman filter over a constant-velocity model. Adding `imm-cv-ct` to that list without changing what the pipeline runs would stamp every track with a baseline claiming an IMM produced it while a linear filter did, which is the exact failure this gap exists to prevent. The refusal is correct and must stay. **Signed by the owner 2026-09-06.** **The hook exists 2026-09-06, the wiring does not.** `LiveTrackingService::with_pipeline_settings` takes a `PipelineSettings`, so a promoted algorithm baseline can now reach the filtering; neither binary builds one from `ConfigBaseline.tracking` yet, so `UNGOVERNED_ALGORITHM_VERSION` still names what is missing and DN-24 §7's rule still holds -- the tracking service may stamp an `AlgorithmBaselineId` only once it applies one, and it does not yet. The registry promotes and rolls back baselines but nothing reads the promoted baseline into the tracking configuration.  **Examined 2026-09-05. One real defect fixed; the rest is blocked, and on three things rather than the one the dependency list names.** It is worse than the description says: `gungnir-modelops` has **no dependents at all** -- no crate in the workspace imports it -- so the registry, the promotion state machine and rollback are unreachable from any running system, and `ConfigBaseline.tracking` is validated by `gungnir-config` and read by nobody either. Wiring it honestly needs three things that do not exist. (1) **A pipeline to apply the promoted configuration.** `PIPELINE_IMPLEMENTED` is false, and `gungnir_fusion_async::ingest` -- the thing that would take a filter selection and a gate threshold -- is human-owned Area A, so this gap cannot reach its own impact statement, which is that a promoted baseline affects the picture. (2) **Something to choose between.** `ModelBaseline` is per mission profile, and `mission_profile` appears nowhere outside `gungnir-modelops` and the documents describing it; the baseline schema has exactly one `TrackingConfig` and no profile. A registry wired today would validate and promote a single candidate with no alternative -- **a promotion ceremony that would let a reader believe governance is happening when there is nothing to govern**, which is the same failure GAP-045 was left unbuilt for. (3) **A design note.** CAP-5.7 had none; every comparable gap closed this month was built against one. **Blockers (2) and (3) were resolved and closed on 2026-09-05**: DN-24 designed the mission-profile schema, GAP-086 built it, and both binaries now construct the registry and journal what the session opened with. **This gap is blocked on GAP-011 alone**, and what remains in it is one thing: the tracking service applying the promoted configuration, after which it may stamp an `AlgorithmBaselineId` into `Provenance` -- and not before, which DN-24 §7 states and a test in `gungnir-app/tests/governance.rs` pins. **What was fixed:** `LiveTrackingService` set `Provenance::algorithm_version` to the tracking-service **crate version**, in a field `gungnir-model` documents as the version of the algorithm configuration that produced the track, resolved through `gungnir-modelops`. Nothing consults a baseline, so the field answered a different question than it asks -- and being invisible today is the danger, because no track exists to carry the stamp until GAP-011 lands and then every track carries a semantic version that reads as governed. It now names what is missing and keeps the build identifier, with a test.
- GAP-059 Audit wiring: **Closed 2026-09-06.** The log existed and the binaries wrote to it from two places -- applying a baseline and signing in. Every other gated act now writes a row at the point it is wired: deciding a plan (`plan.decide`), commanding a sensor (`sensor.task`), stating, tasking, declining or satisfying a requirement (`requirement.state`, tasking under `sensor.task`), and conducting a review (`review.conduct`, the name DN-20 §6 gives it). Each row carries the verified operator or none -- never a role standing in for a person (DN-23 §5 rule 1) -- and PN-20 draws the log. Two action names joined `gungnir_security::actions` (human-owned; signed by the owner 2026-09-06). Promotions have no site to audit: `model.promote` is checked by nothing because PN-14's promotion control is not built (GAP-053, DN-24 §9).
