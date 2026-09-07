# Wireframes

Status: first draft, 2026-09-04. PlantUML Salt sources, one per panel (WF-01 to
WF-20, matching PN-01 to PN-20 in `../information-architecture.md`) and one per role
layout (WF-L1 to WF-L8). Render with any PlantUML renderer (the UAF render script
handles `.puml` files when pointed at this folder). Each wireframe's data
annotations are below: every element names the model field or crate value behind
it, per the plan's acceptance criterion.

## Panels

| Wireframe | Panel | Data behind the elements | Exists in code |
|---|---|---|---|
| [WF-01](WF-01-status-strip.puml) | Status strip | backend `AppState::backend`, `RemoteTrackingService::outbox_len`, `dropped`; session `Mission.session`, `MissionState`; time `TimeAuthority::now` and clock kind; health `SystemHealth` three flags; weapons control status and delegation from the policy section (GAP-052, D-15); alert counts from `AlertLifecycle` states; role `Role` | no (GAP-072) |
| [WF-02](WF-02-viewport.puml) | Viewport | glyphs from `TrackView` (`position_enu`, `state[3..6]`, `covariance`, `classification`, `quality`, `mission_time`); assignment lines from `PlanView.solutions` and `intercept_point`; coverage regions and gaps (GAP-006, GAP-007); geofences from `gungnir-geo`; assets from the asset list (GAP-026) | 2D fallback yes; layers planned |
| [WF-03](WF-03-track-table.puml) | Track table | `TrackView` id, `classification`, `status` (the word from `Vocabulary::track_status`, GAP-070), `quality.association_confidence`, age from `mission_time`, `speed_mps`, predicted asset and time (GAP-020), `RiskScore`, `provenance.source_sensor_ids` | yes; new columns |
| [WF-04](WF-04-track-detail-evidence.puml) | Track detail and evidence | one `TrackView` with `position_sigma`; `RiskScore` factors; `GlobalEntityId` lineage from `gungnir-identity`; `Provenance`; `IdentificationEvidence` list from `gungnir-identification`; per-class margin (GAP-018); designation writes evidence | yes (GAP-073); identity, kinematics and uncertainty are real, the three evidence sections name GAP-010, GAP-019 and GAP-028, and no designation control is drawn |
| [WF-05](WF-05-recommendation.puml) | Recommendation | `PlanView` id, `mission_time`, `solutions`; time remaining from time to impact (GAP-020); `PolicyVerdict`; `CourseOfAction.rationale`; alternatives from `DecisionSupport` (GAP-032); cost and readiness from `ResourceView` and the layer model (GAP-030) | partly (`intercept_panel.rs`) |
| [WF-06](WF-06-approval-queue.puml) | Approval queue | pending approvals from `ApprovalWorkflow` with priority, time remaining, verdict, assignee, state incl. expired and escalated (GAP-034, GAP-035); capacity from decisions per minute | yes (GAP-038, GAP-034, GAP-035); time remaining, expiry, escalation marks and ordering are real; the priority tie-break and decisions-per-minute capacity need GAP-028 |
| [WF-07](WF-07-decision-dialog.puml) | Decision dialog | the plan, verdict, rationale; degraded flag from `Quality::is_stale` of the plan's tracks and health; `ApprovalWorkflow::decide` with `OperatorDecision` and reason; delegation check | yes (GAP-038); the degraded flag reads the health flags and the journal state, and accept is gated on acknowledging it |
| [WF-08](WF-08-alerts-incidents.puml) | Alerts and incidents | `AlertLifecycle` with `Alert.severity`, `state`, `history`; correlated incidents from `gungnir-observability`; transitions via `can_transition_to` | list only (`alerts.rs`) |
| [WF-09](WF-09-system-health.puml) | System health | `SystemHealth` and the reasons each service reports; `SensorRecord` mode and calibration; clock skew per source (GAP-008); journal state; watchdog; outbox | yes (`sensor_health.rs`); extended |
| [WF-10](WF-10-sensor-management.puml) | Sensor management | `SensorRecord` per sensor; `SensorMode::can_transition_to` for the offered modes; coverage before and after from `gungnir-analytics`; tasking requests (GAP-005); commit through `SensorRegistry` and the outbound path (GAP-004); audit (GAP-059) | no (GAP-003) |
| [WF-11](WF-11-coverage-layers.puml) | Coverage layers | `CoverageRegion` per sensor, combined coverage and gaps (GAP-006), comparison of two configurations, recompute age (MOP-14) | no (GAP-007) |
| [WF-12](WF-12-replay.puml) | Replay | `ReplaySession` position, `len`, `remaining`, `clock`; `step`, `seek_to`; envelope `seq`, `mission_time`, event kind; `Case` and `Annotation` | yes (GAP-071); position, length, remaining, clock, step, seek and play rate are real; the cursor does not rebuild the picture (GAP-045) and `Case`/`Annotation` are not built |
| [WF-13](WF-13-reports.puml) | Reports | `Report` figures with journal references from `gungnir-reporting`; measures (GAP-047); releasability marking (GAP-062); assistant draft (plan 08) | yes (GAP-071); counts and export are real, measures GAP-047, the exported file carries no marking until GAP-062, assistant draft not built |
| [WF-14](WF-14-config-editor.puml) | Configuration editor | `ConfigBaseline` sections and `version`; `validate` result; assets and policy sections (GAP-026, GAP-052); validity window; rehearsal record (GAP-045); apply via `ConfigStore::save` under `config.apply` with audit | yes (GAP-071); sections, version, validity, validate and audited apply are real; apply takes effect on restart; field editing and the rehearsal record are not built |
| [WF-15](WF-15-requirements.puml) | Requirements and tasking | requirement objects with area, priority, window, status (GAP-005); tasking with coverage cost from `gungnir-analytics`; concurrence | no (GAP-005) |
| [WF-16](WF-16-planning.puml) | Planning | laydown options over `ConfigBaseline` sensors and resources; coverage and gaps per option (GAP-006); maintenance window; rehearsal through the pipeline (GAP-045); submit with the record | no (GAP-006, GAP-026) |
| [WF-17](WF-17-commander-summary.puml) | Commander summary | queue statistics from `ApprovalWorkflow`; delegations (D-15, GAP-052); accepted gaps; plan in force from `ConfigBaseline`; weapons control status; outcomes (GAP-043) | yes (GAP-073); plan in force and delegations are real, queue GAP-038, accepted gaps GAP-006, outcomes GAP-043, no controls drawn |
| [WF-18](WF-18-reconciliation.puml) | Reconciliation conflicts | `ReconciliationReport` from `gungnir-resilience`; both `DecisionRecord`s; `RoleRankArbiter` result; confirm or overturn recorded through `ApprovalWorkflow` | no (GAP-050) |
| [WF-19](WF-19-assistant.puml) | Assistant | conversation with provenance (model, tools, snapshot time) from `gungnir-agent` (plan 08); provider and egress per profile (D-14); no state-changing control | no (GAP-044) |
| [WF-20](WF-20-audit-accounts.puml) | Audit and accounts | `AuditEntry` log; `StaticRoleAuthorizer` assignments and `role_permits`; credentials per D-02 (GAP-057) | no (GAP-057, GAP-059) |

## Role layouts

| Wireframe | Role | Panels (see `../information-architecture.md` §4) |
|---|---|---|
| [WF-L1](WF-L1-operator-layout.puml) | Operator | PN-06, PN-05, PN-03, PN-08, PN-09 · viewport · PN-04, PN-07, PN-19 on demand |
| [WF-L2](WF-L2-supervisor-layout.puml) | Supervisor | PN-06, PN-05, PN-08, PN-09, PN-10, PN-03 · PN-17, PN-14, PN-18 on demand |
| [WF-L3](WF-L3-analyst-layout.puml) | Analyst | PN-12, PN-03, PN-13 · PN-19 |
| [WF-L4](WF-L4-sensor-manager-layout.puml) | Sensor manager | PN-10, PN-11, PN-09, PN-08 · PN-15 |
| [WF-L5](WF-L5-administrator-layout.puml) | Administrator | PN-14, PN-20, PN-09 · others read-only |
| [WF-L6](WF-L6-intelligence-analyst-layout.puml) | Intelligence analyst | PN-15, PN-04, PN-03, PN-13 · PN-12 |
| [WF-L7](WF-L7-planner-layout.puml) | Planner | PN-16, PN-11, PN-14, PN-12 · PN-10 read-only |
| [WF-L8](WF-L8-commander-layout.puml) | Commander | PN-17, PN-06, PN-08, PN-09 · PN-05, PN-07, PN-16 |

## Conventions in the wireframes

- Values shown are from vignette VG-01 and VG-07 at about 01:41 and 01:52, so the
  wireframes tell one story: a raid in progress, R1 lost, the KAL cell about to
  disconnect.
- Colour words in the Salt sources stand for the tokens in `../design-system.md`;
  the renderer's colours are not the design's.
- Controls that write state are named with a trailing ellipsis when they open a
  dialog ("Apply…", "Delegate…") and never appear on the panel that shows the data
  they change (principle 2).
