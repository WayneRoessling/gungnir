# UX to code map

Status: first draft, 2026-09-04. Every wireframe element to the panel file and the
model field that feeds it; every flow to the crate that implements it; the
engineering items filed in the gap register. Panel functions follow the shape in
`gungnir-ui/src/panels/`: `pub fn render_<panel>(ui: &mut egui::Ui, <refs into AppState>)`,
no I/O and no allocation in the hot path (`../rust-ui-architecture-coding-standards.md` §2, §5).

## 1. Panels to files

| Panel | File (existing or proposed) | Reads from `AppState` | Writes through | Gap |
|---|---|---|---|---|
| PN-01 Status strip | `gungnir-ui/src/panels/status_strip.rs` (new) | `backend`, `mission`, `clock`, `health`, alert counts, role; outbox counts and `last_heard_age` from the remote service (D-23); the operator line from the session authority: who, their role, the time left, or why nobody (GAP-057, 2026-09-06) | nothing | GAP-072 |
| PN-02 Viewport | `gungnir-viewport3d/src/lib.rs::render`, `tracks.rs` | `tracking.tracks()`, `last_plan`, `viewport` | selection, camera | layers GAP-007; hazards GAP-017 (`layers::draw_hazards_2d`, from `gungnir-app/src/hazards.rs`); geofences GAP-088 (`layers::draw_geofences_2d`, from `gungnir-app/src/geofences.rs`); scene GAP-022; classification frames GAP-073 |
| PN-03 Track table | `gungnir-ui/src/panels/track_table.rs` | `tracking.tracks()`, the clock, the plan in force | selection (returned as a `PanelAction`) | built; score column unavailable until GAP-028 |
| PN-04 Evidence card | `gungnir-ui/src/panels/track_detail.rs` | selected `TrackView`; evidence from `gungnir-identification`; lineage from `gungnir-identity`; factors from `sustainment::score_factors` over the asset ranking (GAP-026) | designation as evidence | built; factors, evidence (AIS cooperative reports through the identification engine, GAP-010) and lineage (the desktop's resolver over its retained sessions, GAP-019) are real as of 2026-09-06, the origin line says where the track came from and how strongly (GAP-062), and a declared platform class is a factor (GAP-027); no designation control is drawn |
| PN-05 Recommendation | `gungnir-ui/src/panels/intercept_panel.rs` (extend) | `last_plan`; `withheld` from `InterceptService::withheld` (GAP-030); `fires_checks` from `decisions::fires_checks` (GAP-036); `handoffs` from `AppState.handoffs` (GAP-040); verdict and rationale (`CourseOfAction`); alternatives | opens PN-07 | built for the plan, the withheld list and the fires checks; rationale and alternatives are GAP-032 |
| PN-06 Approval queue | `gungnir-ui/src/panels/approval_queue.rs` | pending approvals from `InMemoryApprovalWorkflow`, which the tick now feeds | selection | built; the queue is always empty in this build and says why. Time remaining, expiry, escalation marks and ordering are real (GAP-034, GAP-035); the priority tie-break needs GAP-028 and the panel says which ordering it is showing |
| PN-07 Decision dialog | `gungnir-ui/src/panels/decision_dialog.rs` | the selected plan and verdict; degraded conditions from the health flags; the policy chain that produced the verdict | `ApprovalWorkflow::decide` | built; accept is gated on acknowledging the degraded conditions and is drawn last; rationale, alternatives and cost need GAP-028 and GAP-032; the record names no operator until GAP-057 |
| PN-08 Alerts and incidents | `gungnir-ui/src/panels/alerts.rs` (replace the string list) | `AlertLifecycle` list from `gungnir-workflow`; correlation from `gungnir-observability` | `AlertLifecycle::transition` | GAP-055 |
| PN-09 System health | `gungnir-ui/src/panels/sensor_health.rs` (extend) | `health`; `SensorRecord`s; `clock_skew.health` per source (GAP-008); `anomaly::detector_status` (GAP-021); the terrain line (GAP-023); each radar feed's counters from its `FeedStatsSink` (GAP-001, 2026-09-06); each AIS feed's counters and match tally (GAP-010); each peer link's state (GAP-009); journal and outbox state | nothing | built for health, skew, detectors, terrain, radar and AIS feeds and peer links, through one `SensorHealthView`; journal state not drawn, the outbox is on PN-18 |
| PN-10 Sensor management | `gungnir-ui/src/panels/sensor_management.rs` | `InMemorySensorRegistry` records and tasks; coverage from `gungnir-analytics`; `sustainment::sensor_plans` recommendations (GAP-037) | `SensorControl::issue`, published as `SensorTaskEvent::Issued`; `record_observed_mode`, published as `SensorEvent::ModeChanged` | built (GAP-003, GAP-004); on a linked desktop a command travels to the node and is closed by the acknowledgement the node streams back, on an embedded one the adapter refuses with the reason (GAP-004, 2026-09-06); the wire from the node to a sensor is GAP-001, and tasking requests need GAP-005 |
| PN-11 Coverage layers | `gungnir-ui/src/panels/coverage_layers.rs`, over `gungnir-viewport3d/src/layers.rs` | coverage regions, gaps, hazards and geofences, with what each layer would draw and the hazard layer's baseline revision (GAP-017, GAP-088) | layer visibility | built (GAP-007): rings are observed from sensor modes (GAP-003) and gaps come from the analytics (GAP-006); still unplaceable until a deployment declares `origin`, which the panel says. Before-and-after comparison needs pushing a PN-16 option into this viewport, which PN-16's options table does not yet do (GAP-087's own remaining item) |
| PN-12 Replay | `gungnir-ui/src/panels/replay.rs` | `ReplaySession` over the desktop journal | `step`, `seek_to`, play rate | built; the cursor moves through the event stream and does **not** rebuild the picture (GAP-045); annotations into a `Case` are not built |
| PN-13 Reports | `gungnir-ui/src/panels/reports.rs` | `JournalReportGenerator` over the desktop journal, `MissionReport.measures` (GAP-047), `SustainmentState.review` (GAP-049), the order of battle from `gungnir-app/src/identity.rs` (GAP-025, 2026-09-06) | generate, export; `review::apply` for open, record, seek, conclude, close, promote | built; counts and measures are real and a refused measure says why, tracking metrics need ground truth, and the exported file carries the report's combined marking with its inputs (GAP-062) |
| PN-14 Configuration editor | `gungnir-ui/src/panels/config_editor.rs` | `config` (`ConfigBaseline`, hazards and geofences sections included), `validate` result, the audit trail | `ConfigStore::apply` under `config.apply`, audited | built; validate and apply are real, apply persists and takes effect on restart, and field-level editing is not built |
| PN-15 Requirements | `gungnir-ui/src/panels/requirements.rs` | `CollectionRequirement` plus the tasks serving it, assembled from the registry | `state_requirement`, `task` (issues a `Search` and records the concurrence), `decline`, `satisfy`; published as `RequirementEvent` | built (GAP-005); recovered from the journal at start-up, and no adapter carries the resulting command (GAP-001) |
| PN-16 Planning | `gungnir-ui/src/panels/planning.rs` | the laydown options table: identifier, intent, coverage per option, its difference from current | selecting an option (toggled off by re-clicking it), for PN-11's preview; no adoption, per DN-26 section 6 rule 4 | **GAP-087**, filed 2026-09-05 out of GAP-055 after three entries had been named for this panel in turn and none of them built one. DN-26's schema and options table are built 2026-09-07, docked by default for the planner and on demand for the commander. Pushing an option into PN-11's viewport for a visual before-and-after is built 2026-09-08. **Not built**: the rehearsal section (GAP-045, drawn as unavailable rather than omitted), the gap-acceptance control, and first-engagement range per option (GAP-020) |
| PN-17 Commander summary | `gungnir-ui/src/panels/commander_summary.rs` | queue statistics, delegations, accepted gaps, plan in force, `engagements::outcome_counts` (GAP-043), `sustainment::exposure_lines` (GAP-026) | delegate, accept gap | built; plan in force and delegations are real, queue GAP-038, accepted gaps GAP-006, outcomes are real (GAP-043); no controls drawn until GAP-035 |
| PN-18 Reconciliation | `gungnir-app/src/workspace.rs` (`render_reconciliation`) over `gungnir-app/src/failover.rs` | the outage's bounds and the decisions taken meanwhile; the `gungnir-resilience` merge over the desktop's journal and the node's `GET /v2/history`: envelopes per side, duplicates dropped, each conflicting decision by plan; or why the node's half is unavailable | `PanelAction::SwitchBack` once every conflict is resolved and the node answers (D-15); `PanelAction::ResolveConflict` per conflict, through the authority check onto the record | built 2026-09-06 (GAP-050): fallback, report, per-conflict resolution, the store-and-forward line and switch back |
| PN-19 Assistant | `gungnir-ui/src/panels/assistant.rs` (new, plan 08) | `gungnir-agent` conversation | question text | GAP-044 |
| PN-20 Audit and accounts | `gungnir-ui/src/panels/audit.rs` | `session_state()`, `AppState.accounts`, `sustainment::audit_lines` (`session::audit_view`) | `session::apply` for sign in, sign out and `AssignRole` (an administrator's act, on the audit trail) | built (GAP-057, GAP-059); the marking row says no marking changes can be made (GAP-062) |
| PN-21 About | `gungnir-ui/src/panels/about.rs` | nothing from `AppState`; `workspace::about_view()` supplies the build's version and `SOURCE_URL` | nothing | built (D-34). Opened from PN-01 by `main.rs::draw_about` as a window, docked in no layout and openable by every role through `WorkspaceLayout::ALWAYS_AVAILABLE`. The notices are constants in the panel, checked against the workspace `NOTICE` by `gungnir-app/tests/appropriate_legal_notices.rs` and checked to actually reach the screen by the render test in `gungnir-ui/src/panels/rendered.rs` |

## 2. Layouts to code

`gungnir_workflow::WorkspaceLayout::for_role` gains the panel ids for PN-01, PN-04,
PN-07, PN-11, PN-15 to PN-20 (`PanelId` variants StatusStrip, TrackDetail,
DecisionDialog, CoverageLayers, Requirements, Planning, CommanderSummary,
Reconciliation, Assistant, Audit) and the three new roles' lists once GAP-068 adds
the `Role` variants. `gungnir-app/src/main.rs` replaces the fixed side panel with a
dock tree over the layout's panels (GAP-055, GAP-075); detached panels are egui
native viewports (`egui 0.29` supports them without a new crate); docking itself
needs a docking crate, a §2.9 sign-off recorded when GAP-075 lands.

## 3. Flows to crates

| Flow | Crate that owns the semantics | Wiring gap |
|---|---|---|
| FL-01 Alert lifecycle | `gungnir-workflow` (`AlertLifecycle`), `gungnir-observability` (correlation) | GAP-055 (panel), GAP-059 (audit) |
| FL-02 Plan decision | `gungnir-policy` (`PolicyChain`), `gungnir-command` (`ApprovalWorkflow`, `DecisionRecord`), `gungnir-model::events` | GAP-028, GAP-034, GAP-035, GAP-038, GAP-059 |
| FL-03 Replay | `gungnir-replay` (`ReplaySession`), `gungnir-workflow` (`Case`) | GAP-071, GAP-045 |
| FL-04 Sensor mode change | `gungnir-sensor-management`, `gungnir-analytics` | GAP-003, GAP-004, GAP-007 |
| FL-05 Disconnected fallback | `gungnir-remote`, `gungnir-resilience`, `gungnir-collab` | GAP-041, GAP-050 |
| FL-06 Assistant | `gungnir-agent` (plan 08), `gungnir-security` (audit) | GAP-044 |
| FL-07 Identity declaration | `gungnir-identification`, `gungnir-policy` (per-class criteria) | GAP-018, GAP-073 |
| FL-08 Weapons control status | `gungnir-policy`, `gungnir-config` (policy section) | GAP-033, GAP-052 |

## 4. Design system to code

`gungnir-ui/src/theme.rs` carries the constants and functions listed in
`design-system.md` (classification colours and frames, warning colour, selection
halo, time-remaining thresholds), and `gungnir-viewport3d/src/tracks.rs` reads them in
`draw_classification_frame` (GAP-073, 2026-09-05). The frame is drawn separately from
the lifecycle fill so affiliation and track state stay independent cues, and shape
carries affiliation as well as colour so the distinction survives a colour-vision
deficiency; the four colours are gated in `gungnir-ui`'s tests against the viewport
background at the WCAG 2.1 4.5:1 ratio.

On 2026-09-06 the theme became an installed style (ARCHITECTURE.md §10 item 89):
`theme::install_egui_theme` is called once from `gungnir-app/src/main.rs`'s creation
closure and from the headless render probe, and gives egui's chrome the surface, text
and interaction tokens DS-01 now lists; `gungnir-app/src/dock.rs` draws the tab bar
with the same tokens through `egui_tiles::Behavior`'s colour hooks; the status strip's
four private colours were removed in favour of the theme's; `theme::numeral` sets
compared numbers in the monospace face (DS-05); and the contrast tests run against
every surface, which raised `ALERT_COLOR` and `TRACK_DELETED_COLOR` to meet the rule.
**Still open:** the night variant as a second constant set selected by a baseline
setting (GAP-095, deferred by D-35), and the dashed low-confidence frame, which needs
the policy margin `gungnir-policy` owns rather than a threshold invented in the
viewport.

## 5. Engineering items filed (2026-09-04)

| Gap | Item | Increment |
|---|---|---|
| GAP-071 | Replay, reports, and configuration editor panels | I3 |
| GAP-072 | Status strip on every layout | I3 |
| GAP-073 | Evidence card, commander summary, table columns, classification frames and theme additions | I3 |
| GAP-074 | Run the usability test rounds and set the MOP-37 targets; round 1 re-planned onto the built panels 2026-09-06 (D-28), session package written | I3 |
| GAP-089 | A seeded session for usability rounds: scripted tracks and plans through the real chain, stamped as a rehearsal (closed 2026-09-06) | I3 |
| GAP-075 | Docking within each role's layout and detaching the viewport, queue, and replay timeline to a second window (D-17 resolved 2026-09-04: adopted where appropriate) | I3 |

Existing gaps the designs depend on: GAP-003, GAP-004, GAP-005, GAP-006, GAP-007,
GAP-008, GAP-018, GAP-022, GAP-026, GAP-028, GAP-032, GAP-033, GAP-034, GAP-035,
GAP-038, GAP-041, GAP-043, GAP-044, GAP-045, GAP-047, GAP-050, GAP-052, GAP-055,
GAP-057, GAP-059, GAP-062, GAP-068.

## Traceability

- `../mission/gap-analysis/gap-register.md` for every gap named; UAF Pr-Cn for the
  workspaces; `../architecture/uaf/resources/Rs-Pr.md` for where the panels are
  drawn in the frame.
