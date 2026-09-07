# Information architecture

Status: first draft, 2026-09-04. The panel catalogue, the navigation model, what is
always visible, and the layout per role. It extends
`gungnir_workflow::WorkspaceLayout::for_role` and proposes the changes to it.

## 1. Navigation model

- **One layout per role; docking and a second window where appropriate (D-17,
  2026-09-04).** Each role's layout is a default dock tree: a status strip across
  the top, the viewport in the centre, side panels in a default order, and dialogs
  that open over the viewport for decisions. Panels can be docked and rearranged
  within the tree and collapsed to a title bar; the arrangement is saved per role
  in the configuration baseline, so a shift inherits its role's layout, not the
  last user's. Three panels may be detached to a second window: the viewport
  (map on one screen, queue on the other), the approval queue, and the replay
  timeline. Decision dialogs always open beside the queue, wherever it is; the
  status strip is repeated in every window. The wireframes show the default
  arrangement; GAP-075 implements docking and detaching.
- **Two tempos, two families of layout.** Live layouts (operator, supervisor,
  sensor manager, commander) put the queue, alerts, and health beside the map.
  Offline layouts (analyst, intelligence analyst, planner, administrator) put the
  timeline, tables, and editors beside the map.
- **Selection is the only navigation.** Selecting a track anywhere (map, table,
  queue, alert) selects it everywhere and opens its detail card; selecting a plan
  does the same for the recommendation panel. There is no drill-down hierarchy to
  get lost in.
- **Dialogs only for decisions.** Accept, override, reject, hold, status change,
  conflict resolution, and plan apply open a modal dialog that names what is being
  decided and shows the verdict; everything else is inline.
- **The assistant is a panel, not a mode.** It never takes the viewport and never
  covers the queue.

## 2. The status strip (PN-01), always visible

Left to right, on every layout, from `AppState`:

| Element | Source | Encoding |
|---|---|---|
| Backend | `AppState::backend` and `RemoteTrackingService` state | "Embedded" or "Node <endpoint>" with a connected or detached mark; beside a connected node, "heard N s ago" driven by the 2 s heartbeat, turning to "nothing heard for N s" once the silence is longer than a beat should be (D-23); when detached after a fallback: "Detached, N queued, M dropped" (`outbox_len`, `dropped`) |
| Session | `Mission.session`, `MissionState` | session id; Created, Live, Paused, Interrupted, Replaying, Closed. **Interrupted** (GAP-051) is a session that was live and was never closed, and reads differently from both Paused and Live |
| Mission time and clock source | `TimeAuthority::now`, replay versus wall clock | HH:MM:SS with "wall" or "replay" |
| Health | `SystemHealth` | three dots: tracking, intercept, ingest; red when false; hover names the reason the service reported |
| Journal encryption | `EncryptionStatus` from the key provider, never from the configuration (GAP-084) | nothing when encrypted; the reason when it is not, in fault colour when the deployment asked for it and is not getting it |
| Baseline validity | `ConfigBaseline.validity` against the session clock (GAP-052) | nothing when no window is configured; otherwise "valid until HH:MM:SS", or, when outside the window, "expired ...: plans are superseded" in fault colour |
| Weapons control status per layer | policy configuration (GAP-052) | one chip per layer: Free, Tight, Hold; an unconfigured layer shows Hold and is marked as unconfigured |
| Delegation in force | policy configuration and D-15 | "Point layer, hostile small UAS: delegated to operator" with expiry when disconnected |
| Alerts | `AlertLifecycle` counts | New, Acknowledged, Escalated counts; click opens PN-08 |
| Role | `Role` of the logged-in operator | the role name; no switching without logging in |

The strip is the one place principles 4 and 6 are enforced for every role.

## 3. Panel catalogue

| Id | Panel | Roles | Exists in code | Reads | Writes |
|---|---|---|---|---|---|
| PN-01 | Status strip | all | yes (`status_strip.rs`, GAP-072) | see §2 | nothing |
| PN-02 | Viewport | all | 2D fallback (`gungnir-viewport3d`); three-d scene GAP-022; coverage GAP-007 | `TrackView`, `PlanView`, coverage regions, gaps, hazards (GAP-017), geofences as a rule layer (GAP-088), defended assets | selection, camera |
| PN-03 | Track table | all live, analysts | yes (`track_table.rs`); score column unavailable until GAP-028; **sort and filter are still not implemented** | `TrackView` list with quality, classification, age, asset, score | selection (built); sort, filter |
| PN-04 | Track detail and identity evidence card | operator, supervisor, intelligence analyst, commander | yes (`track_detail.rs`, GAP-073); the factors name the threatened asset and the priority's weight (GAP-026); evidence and lineage unavailable until GAP-010 and GAP-019; no designation control drawn | one `TrackView`, `Provenance`, `Quality`, `IdentificationEvidence` list, `RiskScore` factors, `GlobalEntityId` lineage | identity designation (a decision) |
| PN-05 | Recommendation panel | operator, supervisor, commander | partly (`intercept_panel.rs` shows the plan and, since GAP-030, the resources the planner withheld with the reason; a fires task's target, location error, firing unit and every deconfliction check with its result, GAP-036; each issued handoff's delivery state, manual stated plainly, GAP-040) | `PlanView`, `InterceptSolutionView`, `PolicyVerdict`, `CourseOfAction.rationale`, alternatives, cost, time remaining | opens PN-07 |
| PN-06 | Approval queue | operator, supervisor, commander | yes (`approval_queue.rs`, GAP-038); always empty in this build and says why. Time remaining, expiry and escalation marks are real (GAP-034, GAP-035); the priority tie-break needs GAP-028 | pending approvals ordered by priority and time remaining; delegation marks | selection; opens PN-07 |
| PN-07 | Decision dialog | operator, supervisor, commander | yes (`decision_dialog.rs`, GAP-038); accept is never the default; rationale, alternatives and cost need GAP-028 and GAP-032 | the selected plan with verdict and rationale | `ApprovalWorkflow::decide` (accept, override with substitute, reject with reason) |
| PN-08 | Alerts and incidents | all live | list only (`alerts.rs`) | `AlertLifecycle` with severity, state, history; correlated incidents | acknowledge, escalate, close (`AlertLifecycle::transition`) |
| PN-09 | System health | all live, administrator | yes (`sensor_health.rs`); per-sensor rows from GAP-054 distinguish radiating, planned downtime, failure and overrun; clock skew per source and each anomaly detector's status are drawn (GAP-008, GAP-021); journal state and watchdog are not | `SystemHealth`, sensor health per `SensorRecord`, clock skew, journal state, watchdog | nothing |
| PN-10 | Sensor management | supervisor, sensor manager, planner (read), administrator | yes (`sensor_management.rs`, GAP-003); commanding and recording an observed mode are separate controls and a requested mode is shown apart from the confirmed one, with task state per sensor (GAP-004); no adapter carries a command to a sensor (GAP-001); tasking requests need GAP-005; re-tasking recommendations with the coverage they close are drawn and accepting one issues the command (GAP-037) | `SensorRecord` per sensor: confirmed mode, requested mode, task state, calibration version, coverage region | command (`SensorControl::issue`), record observed mode (`InMemorySensorRegistry::record_observed_mode`), calibration baseline |
| PN-11 | Coverage layer controls | sensor manager, planner, supervisor | yes (`coverage_layers.rs`, GAP-007; hazards toggle with the layer's currency, GAP-017; geofences toggle with a count, GAP-088); before-and-after comparison needs PN-16, which no entry builds | coverage regions and gaps from `gungnir-analytics`, with what each layer would draw | which layers the viewport draws |
| PN-12 | Replay | analyst, intelligence analyst, planner, administrator | yes (`replay.rs`, GAP-071); the cursor moves through the event stream and does not rebuild the picture (GAP-045) | `ReplaySession`: position, length, remaining, clock | step, seek, play rate |
| PN-13 | Reports | analyst, intelligence analyst | yes (`reports.rs`, GAP-071); counts are real; the measures catalogue is folded per figure with the target on every row and a refused row says why (GAP-047); the after-action review opens, records seekable findings, concludes, closes and promotes (GAP-049); the report's combined marking and its inputs are shown and travel inside the exported file (GAP-062) | `Report` with figures and their journal references | generate, export; open review, record finding, seek, conclude, close, promote |
| PN-14 | Configuration editor | supervisor, sensor manager, planner, administrator | yes (`config_editor.rs`, GAP-071); validate and apply are real, apply takes effect on restart, field editing is not built | `ConfigBaseline` sections (hazards and geofences included, GAP-017, GAP-088), validation results, version | edit, validate, apply (a decision) |
| PN-15 | Requirements and tasking | intelligence analyst, sensor manager, commander | yes (`requirements.rs`, GAP-005); the list is recovered from the journal at start-up, and an empty one says whether nothing was asked for or the record could not be read | requirement objects with standing, the tasks serving each, and time remaining | state (any role), task and decline (`sensor.task`), answer (names its evidence) |
| PN-16 | Planning panel | planner, commander | no; **GAP-087** builds it (filed 2026-09-05 out of GAP-055, which found that no entry did). Blocked on a laydown concept in the baseline -- there is one set of positions, so an options table would have one row -- and on GAP-045 for the rehearsal record a submit carries | laydown options with coverage per option, defended-asset list, plan validity | draft, compare, submit for approval |
| PN-17 | Commander summary | commander, supervisor | yes (`commander_summary.rs`, GAP-073); plan in force and delegations are real, queue GAP-038, accepted gaps GAP-006; outcomes are real and count corroborated and track-inferred apart (GAP-043); the most exposed assets are listed with track, priority, score and time to impact (GAP-026); the watch handover is drawn at shift change with an editable notes field and an acknowledge control (GAP-054); no delegate or accept-gap controls drawn | queue state, accepted gaps, delegations in force, plan in force, outcomes, the open handover summary | acknowledge a handover and record its notes (GAP-054); accept a gap, delegate (decisions) |
| PN-18 | Reconciliation conflicts | supervisor | no (GAP-050) | `ReconciliationReport` conflicts with both decisions and the arbitration result | confirm or overturn (a decision) |
| PN-19 | Assistant | all except where the profile forbids | no (plan 08, GAP-044) | the conversation with provenance labels | question text only; never state |
| PN-20 | Audit and accounts | administrator | yes (`audit.rs`, GAP-057, GAP-059): who is signed in or which of the three reasons nobody is, a masked sign-in form, sign-out, the accounts without their hashes, and the audit log with failed attempts; assigning a role is not drawn | `SessionState`, the account listing, the `AuditEntry` log | sign in, sign out; assign role (a decision, not drawn) |

## 4. Layouts per role

Proposed `WorkspaceLayout::for_role` panel lists. The strip is implicit; the
viewport is the centre of every layout; "on demand" panels open from the strip or
by selection.

| Role | Fixed panels, in order | On demand |
|---|---|---|
| Operator | PN-06 Approval queue, PN-05 Recommendation, PN-03 Track table, PN-08 Alerts, PN-09 Health | PN-04 on selection, PN-07 on decide, PN-19 |
| Supervisor | PN-06, PN-05, PN-08 (incidents view), PN-09, PN-10, PN-03 | PN-04, PN-07, PN-14, PN-17, PN-18 on reconciliation, PN-19 |
| Analyst | PN-12 Replay, PN-03, PN-13 Reports | PN-04, PN-19 |
| Sensor manager | PN-10, PN-11, PN-09, PN-08 | PN-14, PN-15 (tasking requests), PN-19 |
| Administrator | PN-14, PN-20, PN-09 | every other panel read-only for administration; no decision dialogs for engagements |
| Intelligence analyst | PN-15, PN-04, PN-03, PN-13 | PN-12, PN-19 |
| Planner | PN-16, PN-11, PN-14, PN-12 | PN-10 read-only, PN-19 |
| Commander | PN-17, PN-06, PN-08 (incidents), PN-09 | PN-04, PN-05, PN-07, PN-16 |

Differences from the code today (`WorkspaceLayout::for_role`):

- The operator gains the queue as the first panel and loses nothing.
- The supervisor's alerts become the incident view (correlated); the conflicts
  dialog is added.
- The administrator's layout drops the engagement dialogs even though the role may
  do everything (`role_permits`): administration is not an engagement post
  (roles document §2).
- Three new roles are added; until GAP-068 lands they are unreachable.

## 5. What is always visible versus on demand

- Always: the strip; the viewport; the queue on live layouts; alerts on live
  layouts.
- On selection: the track detail card; the recommendation's alternatives.
- On event: the decision dialog (a pending approval selected), the conflicts
  dialog (reconciliation reported), the degraded-state acknowledgement (a plan
  proposed on a degraded picture, MT-07 step 5).
- Never automatic: nothing pops over the queue without the operator selecting it;
  new items are announced in the strip's counts and the queue's ordering.

## 6. Density and layout rules

- Live layouts show at most six panels beside the viewport; each panel's header
  carries its count (pending, alerts) so a collapsed panel still informs.
- The queue shows at most the top twelve items with priority and time remaining;
  the rest are counted.
- Text at `DEFAULT_FONT_SIZE` (14) for anything decided on; `SMALL_FONT_SIZE` (12)
  for provenance and history only.
- Tables are virtualised (egui `TableBuilder` with row heights) so 200 tracks
  (Scenario 4) render within the egui pass budget (8 ms).

## 7. Traceability

- Personas: `personas.md`; tasks: `task-analysis/`; wireframes:
  `wireframes/README.md`; flows: `interaction-design.md`.
- Capabilities: PN-01 CAP-5.5, CAP-5.4, CAP-3.6; PN-02 CAP-2.10, CAP-2.11; PN-04
  CAP-2.6, CAP-2.7; PN-05 to PN-07 CAP-3.5, CAP-3.6, CAP-4.1, CAP-4.2; PN-08 CAP-5.5;
  PN-10, PN-11 CAP-1.3, CAP-1.4; PN-12, PN-13 CAP-5.2, CAP-5.3; PN-14 CAP-5.6;
  PN-15 CAP-1.3 (collection); PN-16 CAP-3.1, CAP-1.4; PN-17 CAP-3.1, CAP-3.6;
  PN-18 CAP-5.4; PN-19 CAP-4.7; PN-20 CAP-6.2, CAP-6.3.
- UAF: Pr-Cn (workspaces by role), Op-Cn needlines NL-04, NL-05, NL-09, NL-10.
