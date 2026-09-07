# Mission threads

Status: first draft, 2026-09-04. Ten end-to-end threads. Each lists its trigger,
actors, steps with the information exchanged (typed against `gungnir-model` where the
system carries it), the decisions and who holds them, timing, success and failure
conditions, the capability areas it needs (for plan 04), and the scenarios that
exercise it. Roles are defined in `roles-and-stakeholders.md`; vignettes in
`vignettes.md` give each thread a concrete setting.

Conventions: "system" means the desktop and node together; "S" marks a step the
system performs, "H" a step a human performs, "S+H" a step the system prepares and a
human completes. "Today" and "gap" refer to the implementation status in
`../gungnir-capabilities.md`.

---

## MT-01 One-way attack drone raid against a defended-asset list

**Domain:** air. **Tempo:** tens of minutes, tens of tracks arriving over one to two
hours. **Trigger:** early warning of a raid (peer report, acoustic network, long-range
radar) or the first local detection.

**Actors:** operator (air), supervisor, sensor manager, commander (priorities),
mobile fire groups and short-range fire units (effectors), higher command (warning).

| Step | Who | Function | Information | Decision |
|---|---|---|---|---|
| 1 | S | Ingest early-warning tracks and reports; validate; quarantine malformed | `DetectionView`, `IngestEvent` | |
| 2 | S | Initiate and maintain tracks across acoustic, radar, and spotter sources at different rates and latencies; mark quality | `TrackView` with `Quality` | |
| 3 | S | Classify by acoustic and kinematic evidence; identity `Hostile` at confidence once policy criteria are met | `Classification`, `IdentificationEvidence` | Per policy, a person confirms for the first tracks of a raid |
| 4 | S | Score each track against the defended-asset list: predicted asset, time to impact, priority | `RiskScore` | |
| 5 | S | Recommend the cheapest adequate effector per track within readiness, geometry, and geofences; keep expensive interceptors in reserve | `PlanView`, `PolicyVerdict` | |
| 6 | S+H | Present the queue of recommendations with rationale; the engagement authority accepts, overrides, or rejects each; the supervisor manages the queue and weapons control status | `CourseOfAction`, `DecisionRecord`, `CommandEvent` | Engagement authority per layer; supervisor for the queue |
| 7 | S | Hand off decided assignments; warn assets on the predicted routes | API handoff (gap); alerts | |
| 8 | S+H | Assess: destroyed, missed, continuing; re-plan the remainder; publish `PlanSuperseded` | `TrackingEvent`, `InterceptEvent` | |
| 9 | S | Journal everything; produce the raid summary after action | `Envelope`s; report | |

**Timing:** first decision within a few minutes of first detection; sustained
decision rate matching the raid's arrival rate; per-track decision latency small
compared with the drone's remaining flight time.

**Success:** every drone predicted to reach a listed asset is engaged before reaching
it or the asset is warned in time; no engagement of a friendly or civil track;
expensive interceptors used only where policy allows; every decision recorded.

**Failure:** saturation (queue outruns the authority), stale tracks engaged, decoys
consuming interceptors, a raid arriving while the picture is degraded without the
degradation shown.

**Capability areas:** Sense, Understand, Decide, Act, Sustain, Secure.

**Exercised by:** engineering Scenario 4 (dense swarm) for the tracking core;
test-track scenario TT-01 (raid of propeller drones with mixed routes and decoys);
end-to-end replay row in `../verification-capability-table.md` §2.

---

## MT-02 Mixed salvo of cruise missiles, drones, and decoys

**Domain:** air. **Tempo:** minutes; a few fast tracks among many slow ones.
**Trigger:** launch warning from a peer or local detection of a low, fast track.

**Actors:** operator (air), supervisor, engagement authority for area defense,
long- and medium-range fire units, defended assets (sheltering).

| Step | Who | Function | Information | Decision |
|---|---|---|---|---|
| 1 | S | Ingest launch warnings and local radar; separate fast tracks from the drone picture by kinematics | `DetectionView`, `TrackView` | |
| 2 | S | Track low-altitude fast targets through horizon dropouts; predict route and impact area | `TrackView`, prediction (gap) | |
| 3 | S | Classify cruise missile versus decoy versus drone by speed, altitude, and signature class; confidence stated | `Classification` | Pre-delegated: missile-profile tracks toward assets are declarable by behaviour |
| 4 | S | Score by time to impact and asset priority; missiles first | `RiskScore` | |
| 5 | S | Recommend the area-defense layer for missiles and the point layer for drones; check friendly aircraft and corridors | `PlanView`, `PolicyVerdict` | |
| 6 | S+H | Present within seconds; pre-delegated authority accepts; supervisor may hold for deconfliction | `DecisionRecord` | Engagement authority; hold by supervisor |
| 7 | S | Hand off; warn assets to shelter | API; alerts | |
| 8 | S+H | Assess; leakers to the point layer; re-plan | Events | |

**Timing:** steps 3 to 6 in under thirty seconds for a missile detected at 30 km;
the system's own latency must be a small fraction of that.

**Success:** every missile is engaged by an appropriate layer or its target is
warned; decoys do not consume area-defense interceptors beyond policy; friendly
aircraft are never engaged.

**Failure:** missile lost in the drone picture; decision path blocked by an
unimplemented or slow solve (the design returns the last good plan and says so);
deconfliction missed.

**Capability areas:** Sense, Understand, Decide, Act, Integrate (peer warning).

**Exercised by:** engineering Scenarios 1 and 4; TT-02 (salvo with decoys); latency
rows in `../performance-budgets.md`.

---

## MT-03 Small UAS over a protected site

**Domain:** air (force protection). **Tempo:** seconds to minutes; one to a few
tracks. **Trigger:** RF detection, counter-UAS radar, acoustic, or visual report at a
site.

**Actors:** site operator, site defense cell, sensor manager, supervisor (for
non-self-defense engagements), civil authorities (if the UAS may be civil).

| Step | Who | Function | Information | Decision |
|---|---|---|---|---|
| 1 | S | Ingest RF, radar, acoustic, and visual reports; correlate into one track | `DetectionView`, `TrackView` | |
| 2 | S | Classify: consumer multirotor, FPV, fixed-wing ISR; link-controlled or autonomous; friendly own UAS by cooperative identification | `Classification`, evidence | |
| 3 | S | Assess: approaching the site, loitering, passing; risk to the site and to people | `RiskScore` | |
| 4 | S | Recommend: jam (if link-controlled and permitted here), interceptor drone, small arms, or observe; geofence checks for jamming near own systems | `PlanView`, `PolicyVerdict` | |
| 5 | S+H | Present; site authority decides; self-defense engagements are recorded after the fact | `DecisionRecord` | Site authority; self-defense retained |
| 6 | S | Hand off to the site's effector; warn personnel | API; alerts | |
| 7 | S+H | Assess; if the UAS was ISR, expect a strike: raise the site's alert state and re-task sensors | Events; alert lifecycle | Supervisor |

**Timing:** ten seconds to two minutes for FPV; minutes for a loitering ISR UAS.

**Success:** the UAS is neutralized, driven off, or observed with its operator's
location estimated where RF allows; no jamming collateral on own systems; civil
drones handled per policy.

**Failure:** fibre-optic FPV undetected until impact (detection is the mitigation, not
jamming); jamming applied inside a restricted geofence; own UAS engaged.

**Capability areas:** Sense, Understand, Decide, Act, Secure.

**Exercised by:** TT-03 (multirotor and FPV approaches, fibre-optic variant); the
geofence policy rows.

---

## MT-04 Uncrewed surface vessel attack on a port or anchored ship

**Domain:** maritime. **Tempo:** minutes; a few tracks in clutter. **Trigger:**
coastal-radar detection of low, fast tracks inbound at night, or a peer warning.

**Actors:** port defense operator, port defense authority, patrol craft, helicopter,
shore fire unit, port authority (ships, barriers), air-defense supervisor (shared
picture).

| Step | Who | Function | Information | Decision |
|---|---|---|---|---|
| 1 | S | Ingest coastal radar, AIS, camera, patrol reports; suppress clutter; hold tracks through dropouts | `DetectionView`, `TrackView` | |
| 2 | S | Cue cameras to inbound low-confidence tracks; fuse visual identification | Sensor tasking; evidence | Sensor manager |
| 3 | S | Identify: no AIS, high speed, heading to the anchorage, night, group behaviour, wake signature | `Classification` | Port defense authority declares |
| 4 | S | Assess closest point of approach and time to protected ships and infrastructure; cross-domain: does the USV class carry air-defense missiles (risk to the helicopter) | `RiskScore` (extension) | |
| 5 | S | Recommend patrol craft, helicopter, shore effector, barrier closure, ship movement; geofences for lanes and own craft | `PlanView`, `PolicyVerdict` | |
| 6 | S+H | Present; decide; record | `DecisionRecord` | Port defense authority |
| 7 | S | Hand off; warn ships; close barriers | API; alerts | |
| 8 | S+H | Assess; continue for the rest of the group | Events | |

**Timing:** about eight minutes from a 10 km detection at 40 knots; identification
in the first two.

**Success:** no USV reaches a protected ship or facility; no fishing or own craft
engaged; helicopters not sent against USV classes that threaten them.

**Failure:** track lost in clutter; identification too late; barrier not closed in
time.

**Capability areas:** Sense, Understand, Decide, Act, Integrate.

**Exercised by:** engineering Scenario 2 (maritime clutter); TT-04 (USV group at
night); cross-domain assessment rows.

---

## MT-05 Surface picture compilation

**Domain:** maritime. **Tempo:** continuous; hundreds of tracks. **Trigger:** none;
standing task.

**Actors:** port defense operator, sensor manager, port and maritime authorities,
peer maritime command posts.

| Step | Who | Function | Information | Decision |
|---|---|---|---|---|
| 1 | S | Fuse coastal radars, AIS, cameras, patrol and peer reports into one surface picture with identities and quality | `TrackView`, `Classification` | |
| 2 | S | Flag anomalies: AIS-off, speed or route inconsistent with declared type, loitering near infrastructure, spoofed AIS positions | Alerts (plan 09 anomaly model later) | |
| 3 | S+H | Operator reviews anomalies; tasks cameras or patrols | Alert lifecycle | Operator |
| 4 | S | Exchange the picture with peers and authorities | API, interop formats | |
| 5 | S | Journal; the picture is the basis for MT-04 and for pattern of life | Envelopes | |

**Timing:** continuous; picture latency and staleness visible.

**Success:** every vessel in the approaches is a track with an identity or an
anomaly flag; peers receive the picture within the agreed latency.

**Failure:** false tracks in clutter overwhelm the display; AIS spoofing accepted as
truth; stale peer tracks shown as current.

**Capability areas:** Sense, Understand, Integrate, Sustain.

**Exercised by:** engineering Scenario 2; TT-05 (dense compliant traffic with a few
anomalies); interop rows.

---

## MT-06 Convoy and battery tracking with cueing of fires

**Domain:** land. **Tempo:** minutes to hours; intermittent tracks. **Trigger:** ISR
tasking against a route or area, or an acoustic detection of artillery fire.

**Actors:** land operator, intelligence analyst, fires authority, ISR UAS operators,
fires unit, airspace control (deconfliction).

| Step | Who | Function | Information | Decision |
|---|---|---|---|---|
| 1 | S | Ingest ISR video observations, ground radar, acoustic and spotter reports at mismatched rates and latencies | `DetectionView` | |
| 2 | S | Track vehicles and groups; register sensors against each other to remove bias; coast through stops and cover | `TrackView` | |
| 3 | S | Resolve global identity across sorties and gaps; classify vehicle type from video and emitter evidence | `GlobalEntityId`, lineage, `Classification` | Analyst confirms class |
| 4 | S | Assess priority and time sensitivity (a battery that fired will move within minutes) | `RiskScore` | Fires authority sets priorities |
| 5 | S | Recommend a fires task: target location and uncertainty, time window, deconfliction against friendly positions, airspace, and no-fire areas | `PlanView`, `PolicyVerdict` | |
| 6 | S+H | Present; fires authority decides; record | `DecisionRecord` | Fires authority |
| 7 | S | Hand off with provenance; the fires unit executes | API | |
| 8 | S+H | Battle damage assessment from the next observation; update the entity | Events; lineage | |

**Timing:** counter-battery within minutes; convoy interdiction within the window
the route allows.

**Success:** targets engaged within their time window with correct deconfliction;
identities kept across gaps; every task recorded with provenance.

**Failure:** identity swap between vehicles in a convoy; bias between sensors placing
the target off; deconfliction missed; a stale location used.

**Capability areas:** Sense, Understand, Decide, Act.

**Exercised by:** engineering Scenario 3 (urban multi-sensor convoy with injected
bias); TT-06 (convoy and battery with ISR gaps).

---

## MT-07 Sensor management under electronic attack

**Domain:** cross-cutting. **Tempo:** minutes; degraded picture. **Trigger:** GNSS
denial, link jamming, radar interference, or loss of a sensor to attack.

**Actors:** sensor manager, supervisor, operators, maintainers.

| Step | Who | Function | Information | Decision |
|---|---|---|---|---|
| 1 | S | Detect degradation: sensor health, ingest gaps, clock skew, track quality collapse; correlate into one alert rather than many | `SystemHealth`, `SyncHealth`, correlated `Alert` | |
| 2 | S | Recompute coverage with the degraded sensors; show the gaps on the picture | Coverage regions; analytics | |
| 3 | S+H | Recommend mode changes and re-tasking (search patterns, backup sensors, emission control) | Sensor modes; coverage before and after | Sensor manager |
| 4 | S | Mark affected tracks as degraded; the planner never allocates on stale tracks | `Quality.is_stale`; assessment rule | |
| 5 | S+H | Supervisor acknowledges the degraded state on the plan; escalates if a defended asset is uncovered | Alert lifecycle; audit | Supervisor; commander for accepted gaps |
| 6 | S | Recover: sensors return, clocks resynchronize, coverage restored, alert closed | Health; alerts | |

**Timing:** degradation shown within seconds; re-tasking in minutes.

**Success:** operators know what they cannot see and for how long; no decision is
taken on hidden staleness; coverage is restored or the gap is formally accepted.

**Failure:** a sensor silently reporting garbage accepted as truth; a coverage gap
not shown; alerts flooding the operator.

**Capability areas:** Sense, Sustain, Secure.

**Exercised by:** engineering Scenario 3 (timing disagreement) and Scenario 5 (soak);
TT-07 (raid during GNSS denial with a sensor loss); observability rows.

---

## MT-08 Collection management and identification evidence fusion

**Domain:** intelligence. **Tempo:** continuous. **Trigger:** an information
requirement from the commander, or a track that needs identification.

**Actors:** intelligence analyst, commander, sensor manager, operators, peers.

| Step | Who | Function | Information | Decision |
|---|---|---|---|---|
| 1 | H | State the requirement: area, question, priority, time window | Requirement (gap: no object yet) | Commander |
| 2 | S+H | Translate into sensor tasking; show the coverage cost | Sensor modes; coverage | Sensor manager |
| 3 | S | Collect; observations carry provenance | `DetectionView.provenance` | |
| 4 | S | Fuse evidence into identities with confidence; keep the evidence with the track | `IdentificationEvidence`, `Classification` | Analyst declares where policy requires |
| 5 | S+H | Maintain entities and order of battle across sessions | `GlobalEntityId`, lineage | Analyst merges or splits |
| 6 | S | Disseminate identities, warnings, and products to roles and peers with releasability | API; reports | Analyst releases |
| 7 | S | Journal; pattern of life accumulates over sessions | Envelopes; reports | |

**Timing:** identification within the decision window of the thread it serves;
products at the battle rhythm.

**Success:** every requirement is tasked or explicitly declined; every hostile
declaration has recorded evidence; peers receive products with correct marking.

**Failure:** identity assigned on one weak source; evidence not retained; products
released without marking.

**Capability areas:** Sense, Understand, Integrate, Secure.

**Exercised by:** identification and identity rows; TT-08 (mixed friendly, civil,
and hostile air traffic with cooperative sources).

---

## MT-09 Defended-asset planning, laydown, and rehearsal

**Domain:** planning. **Tempo:** hours to days; offline. **Trigger:** a new threat
estimate, a change to the defended-asset list, a sensor or effector move, or the
start of a shift.

**Actors:** planner, commander, sensor manager, supervisor, analyst.

| Step | Who | Function | Information | Decision |
|---|---|---|---|---|
| 1 | H | Set the defended-asset list and priorities | Baseline (gap: asset list object) | Commander |
| 2 | S+H | Plan sensor and effector laydown; compute coverage and gaps over terrain | Analytics; sensor coverage; resources | Planner |
| 3 | S+H | Configure policy: identification criteria, weapons control status, authorities, restrictions as geofences | Policy configuration (gap: status and authority model) | Commander approves |
| 4 | S | Validate the plan as a configuration baseline | `gungnir-config` validation | |
| 5 | S+H | Rehearse: replay a scenario through the pipeline with the plan; review recommendations and gaps | Replay; test tracks | Planner, supervisor |
| 6 | S+H | Apply the plan; audit | Baseline apply; audit log | Supervisor or commander |
| 7 | S+H | After the shift: review the journal, measure, record lessons | Reports; gap register | Analyst, commander |

**Timing:** a plan cycle fits the battle rhythm; rehearsal before each shift.

**Success:** a validated, rehearsed plan in force with known and accepted gaps; every
change audited.

**Failure:** a plan applied without validation; coverage gaps unknown; rehearsal
skipped under time pressure.

**Capability areas:** Sustain, Decide, Secure.

**Exercised by:** configuration and replay rows; TT-09 (rehearsal of TT-01 under two
laydowns).

---

## MT-10 Disconnected operation and reconnection

**Domain:** cross-cutting. **Tempo:** minutes to hours. **Trigger:** loss of the link
between a desktop and its service node, or a desktop deployed standalone.

**Actors:** site operator, supervisor (at the node), commander (authority
delegation), the node.

| Step | Who | Function | Information | Decision |
|---|---|---|---|---|
| 1 | S | Detect the link loss; fall back to embedded services; keep journaling locally; show the backend state on every layout | Backend state; alert | |
| 2 | S | Queue detections, decisions, and audit entries for forwarding | Outbox; store-and-forward queue | |
| 3 | S+H | Operate under pre-delegated authority for the disconnected case; decisions recorded locally | `DecisionRecord` (local) | Site authority per delegation |
| 4 | S | Reconnect: forward the queue; reconcile the local and node journals; report conflicts | Reconciliation report | |
| 5 | S+H | Supervisor resolves reported conflicts under the arbitration rule; the node's record is updated | Arbitration; audit | Supervisor |
| 6 | S | Return to the remote backend; local projection resynchronized | Backend state; alert closed | |

**Timing:** fallback within seconds of detection; reconciliation within a minute of
reconnection for a short outage (`../performance-budgets.md`).

**Success:** the site keeps operating; nothing recorded offline is lost; conflicts
are visible and resolved, not silently overwritten.

**Failure:** duplicate or lost decisions; a site engaging under authority it did not
have; the operator unaware of the backend state.

**Capability areas:** Sustain, Secure, Integrate.

**Exercised by:** TT-10 (VG-01 raid with a mid-raid link loss at the port cell);
resilience and collaboration rows; the cross-layer reconciliation row.

---

## Capability areas needed, by thread

| Thread | Sense | Understand | Decide | Act | Sustain | Secure | Integrate |
|---|---|---|---|---|---|---|---|
| MT-01 | x | x | x | x | x | x | |
| MT-02 | x | x | x | x | | | x |
| MT-03 | x | x | x | x | | x | |
| MT-04 | x | x | x | x | | | x |
| MT-05 | x | x | | | x | | x |
| MT-06 | x | x | x | x | | | |
| MT-07 | x | | | | x | x | |
| MT-08 | x | x | | | | x | x |
| MT-09 | | | x | | x | x | |
| MT-10 | | | | | x | x | x |

Plan 04 derives leaf capabilities from the steps above and fills the
capability-to-thread matrix from this table.
