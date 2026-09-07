# Integrated air defense and counter-UAS (lead domain)

Status: first draft, 2026-09-04. Open sources only; ranges with confidence marks.

## 1. Mission statement

Protect a prioritized list of defended assets in a sector against air threats from
small multirotors to ballistic missiles by maintaining a single, honest air picture;
identifying and prioritizing threats; recommending the cheapest adequate engagement
within policy; presenting it to the human who holds engagement authority; recording
the decision; handing off to the effector system; and assessing the result, while
never engaging anything without a recorded human decision and never presenting
stale or uncertain data as fresh and certain.

## 2. Threat classes

| Class | Warning time, indicative | Signature | Layer that engages | Notes |
|---|---|---|---|---|
| One-way attack UAS, propeller | Ten to fifteen minutes from a 40 km radar detection at about 185 km/h; longer with acoustic route tracks | Small radar cross-section (M); distinctive acoustic signature; no radio link | Mobile fire groups, short-range guns, interceptor drones, jammers of the navigation only | Raid sizes of tens to over a hundred; the saturation and cost problem |
| One-way attack UAS, jet | A few minutes | Small; faster | Short-range missiles, guns | Fewer; shorter reaction |
| Loitering munition | Minutes; loiters before the dive | Small; may be link-controlled | Jammers, guns, interceptor drones | Targets radars and artillery; its presence implies a cueing ISR UAS |
| Small multirotor and FPV | Ten seconds to two minutes | Very small; link-controlled unless fibre-optic | RF detection and jamming, nets, shotguns, interceptor drones, guns | Force protection of the command post and sensors; fibre-optic variants defeat jamming |
| Tactical ISR UAS | Hours; loiters at altitude | Small; link-controlled | Medium and short-range missiles, jamming | Engaging it removes the cue for missiles and artillery |
| Cruise missile, subsonic | Under two minutes from a 30 km low-altitude detection; longer with early warning from radars up-threat | Low altitude; terrain-following; radar-visible at the horizon | Medium and long-range missiles, short-range guns as last layer | Often timed to arrive with the drone raid |
| Cruise missile, supersonic and aeroballistic | Tens of seconds from local detection | Fast; high altitude then dive | Long-range missiles with the right capability | Few systems can engage; the decision is usually to warn and shelter |
| Short-range ballistic missile | Minutes from launch warning (if a peer provides it); seconds locally | Ballistic then quasi-ballistic manoeuvre | Long-range missiles with ballistic-defense capability | Salvos and decoys; engagement authority is usually pre-delegated |
| Glide bomb | Tens of seconds to a minute from release detection | Non-powered; released from aircraft at standoff | Rarely engageable; the aircraft is the target of long-range missiles | Warning and sheltering; pushes air defense to engage the launch aircraft |
| Crewed aircraft and helicopters | Minutes | Large; may squawk friendly | Long and medium-range missiles | Identification is critical: friendly aircraft share the space |

## 3. Engagement sequence

The sequence is the same for every class; what differs is the time available and who
holds each decision.

| Step | Function | System role | Human role | Information (system types) |
|---|---|---|---|---|
| 1 | Detect | Ingest observations from every sensor through validation and quarantine; fuse across sensors and rates; initiate tracks | Monitor health; task sensors | `DetectionView` in; `TrackView` out; `IngestEvent` for quarantines |
| 2 | Track | Maintain kinematic estimates with uncertainty; coast through gaps; report quality and staleness | Watch for coasting and lost tracks | `TrackView.quality`, `TrackStatus` |
| 3 | Identify and classify | Fuse identification evidence (cooperative identification, flight plan, emitter, kinematic profile, visual, spotter, operator designation) into friend, hostile, neutral, unknown with confidence | Confirm or designate identity where policy requires a human | `Classification`, `IdentificationEvidence` |
| 4 | Assess | Score threat against the defended-asset list: time to impact, asset priority, class lethality; predict trajectory | Adjust priorities under supervisor authority | `RiskScore`, `PlanView` inputs |
| 5 | Recommend | Assign effectors to tracks within readiness, geometry, and policy; present alternatives and rationale; check geofences and authority | Review the recommendation and its rationale | `PlanView`, `PolicyVerdict`, `CourseOfAction` |
| 6 | Decide | Present the plan to the role holding engagement authority; record accept, override, or reject with the verdict and the time | Decide, within rules of engagement and weapons control status | `DecisionRecord`, `CommandEvent` |
| 7 | Engage (handoff) | Hand the decided assignment to the effector system through the API; track the engagement | Fire unit executes under its own procedures | `gungnir-api` (handoff endpoint to be defined) |
| 8 | Assess effects | Observe the track after engagement: destroyed, missed, continuing; re-enter the sequence | Confirm the outcome; call the next engagement | `TrackingEvent`, `InterceptEvent::PlanSuperseded` |

Steps 1 to 5 run continuously and concurrently for every track; step 6 is the
boundary the design will not cross without a person.

## 4. Representative timelines

| Case | Detection | Time available | Steps 3 to 6 must complete in | Notes |
|---|---|---|---|---|
| Propeller drone, radar detection at 40 km | t0 | About 13 minutes to the sensor site; less to a forward asset | A few minutes; the constraint is throughput across a raid of tens | Acoustic route tracks can add tens of minutes of warning |
| Propeller drone raid of 60 with mixed routes | rolling | Continuous for one to two hours | Per-track decisions at the rate the raid arrives; the supervisor manages the queue | Cost discipline: not every drone warrants a missile |
| Low-altitude cruise missile, local detection at 30 km at Mach 0.8 | t0 | About 110 seconds | Under 30 seconds, with pre-delegated authority | Early warning from up-threat radars or a peer can add minutes |
| Short-range ballistic missile with launch warning | t0 at launch | Several minutes to impact | Pre-delegated; the system's role is to present, record, and hand off in seconds | Terminal local detection alone gives seconds |
| FPV against the command post | t0 by RF detection or sight | 10 to 120 seconds | Seconds; self-defense authority | Local effectors under local control; the system's job is to warn and record |
| ISR UAS loitering at 4 km altitude | t0 | Hours | Minutes; a deliberate engagement decision | Engaging it protects everything it would cue |

These timelines set the system's latency budgets in `../performance-budgets.md`: the
end-to-end detection-to-display budget must be small compared with the shortest
decision window, and the decision path must never wait on a solve.

## 5. Sensor and effector mix by layer

| Layer | Sensors that carry it | Effectors | What the system must do well |
|---|---|---|---|
| Early warning | Long-range radar, peer tracks, acoustic networks, spotter reports | None | Fuse very different rates and latencies; keep provenance; show what is stale |
| Area defense | Medium-range radars, long-range radar | Long and medium-range missiles | Identification quality; deconfliction with friendly aircraft; expensive-interceptor discipline |
| Point defense | Short-range counter-UAS radars, electro-optical and infrared, RF detection | Guns, short-range missiles, interceptor drones, jammers | Throughput; cheap-layer preference; geofence checks for jamming and gunfire arcs |
| Self-defense | RF detection, sight and sound | Small arms, nets, jammers | Warn fast; record what happened |

## 6. Rules-of-engagement structure

The analysis does not assume a nation's rules of engagement; it assumes their
structure, which the system must be able to represent and enforce in
`gungnir-policy`:

- **Identification criteria.** What evidence, at what confidence, is required to
  declare a track hostile, and whether declaration requires a person. Hostile
  criteria differ by class: a track on a missile profile toward a defended asset may
  be declarable by behaviour; an unidentified aircraft may not.
- **Weapons control status.** A per-sector and per-layer state (commonly free,
  tight, hold in joint and NATO usage) that says whether fire units may engage
  anything not identified friendly, only tracks identified hostile, or nothing except
  in self-defense. The status is set by the supervisor or commander and shown on
  every layout.
- **Engagement authority.** Which role may authorize which layer against which class,
  and what is pre-delegated to fire units for short-warning threats.
- **Self-defense.** Always retained by the fire unit; the system records it after
  the fact.
- **Restrictions.** No-fire and restricted-fire areas, altitude and azimuth limits,
  jamming restrictions near own systems and civil infrastructure, civil aviation
  corridors; all representable as geofences and policy rules.
- **Escalation.** What is referred upward, and the timeout behaviour when the
  authority is unreachable (the disconnected case, MT-10).

## 7. Human decision points

| Decision | Held by (typical) | System obligation |
|---|---|---|
| Declare identity where policy requires a person | Operator or supervisor per class | Show evidence and confidence; record the designation as evidence with the operator's identity |
| Set weapons control status | Supervisor or commander | Show current status everywhere; log changes |
| Authorize an engagement | The engagement authority for that layer and class | Present the plan with verdict, rationale, alternatives, cost, and time remaining; record accept, override, or reject |
| Hold or cease fire for deconfliction | Supervisor | Show friendly tracks and corridors; propagate the hold to the plan |
| Change priorities on the defended-asset list | Commander | Re-score and re-plan; record |
| Re-task sensors under attack or jamming | Sensor manager | Show coverage before and after; record |
| Accept a stale or degraded picture as the basis for a decision | The decision holder | Never hide staleness; require acknowledgement of degraded state on the plan |

## 8. Failure modes the design guards against

| Failure | Consequence | Design response |
|---|---|---|
| Misidentification of a friendly aircraft as hostile | Fratricide | Identification evidence with confidence; cooperative sources weighted; human declaration for aircraft classes; policy denial without identification |
| Stale track presented as fresh | Engagement of empty air or the wrong track | `Quality.is_stale` drawn distinctly; stale tracks never allocated (`gungnir-assessment`) |
| Saturation | Queue overflow; expensive interceptors spent on cheap drones; the supervisor loses the picture | Prioritization by asset and time to impact; cheap-layer preference; queue management in the workflow; alert correlation |
| Decoys | Interceptors wasted; real threats slip through | Classification evidence; cost discipline; pattern-of-life priors |
| Sensor loss under electronic attack | Coverage gaps hidden | Health reported, coverage recomputed and shown (MT-07) |
| Disconnection from the node | Decisions unrecorded or duplicated | Local journaling, store-and-forward, reconciliation with conflict reporting (MT-10) |
| Automation bias | Operators accept recommendations without reading them | Verdict, rationale, and alternatives on every plan; the accept control never the default; usability measures in plan 06 |

## 9. What the system does today, and what it does not

| Function | Today | Not yet |
|---|---|---|
| Ingest with validation and quarantine | Gateway, recorded and simulated adapters | Live radar, EO/IR, acoustic, RF, spotter-application adapters; ASTERIX decoding |
| Track picture | Facade, projection, honest health | The fusion pipeline itself (`PIPELINE_IMPLEMENTED` is false) |
| Identification | Evidence fusion engine | Live evidence sources; cooperative identification decoding |
| Assessment | Closing-speed risk scoring against one protected point | Defended-asset lists with priorities; lethality by class; trajectory prediction |
| Recommendation | Planner facade with last-good-plan behaviour | The allocator; intercept geometry; layer and cost modelling |
| Policy and decision | Geofence and readiness policy; approval workflow with records | Weapons control status; authority by role and class; wiring into the desktop and node |
| Handoff | API contract types | Transport; effector handoff endpoint |
| Assess effects | Event schema | Engagement tracking |

## 10. Sources and confidence

| Area | Source type | Confidence | Verify by |
|---|---|---|---|
| Engagement sequence and decision structure | Public joint and NATO doctrine on countering air and missile threats and on counter-UAS techniques | H for structure; M for terminology mapping | Subject-matter review; `glossary.md` |
| Threat class kinematics and warning times | Public manufacturer claims, open-source trackers, published analyses; timelines computed from speed and detection range | M; L where marked | Plan 07 catalogue; sensor-model specification |
| Effector layers and costs | Public reporting on interceptor cost asymmetry and mobile fire groups | M | Plan 01 for cost figures used in the business case |
| Failure modes | Published incident analyses and human-factors literature on automation bias in air defense | M | Plan 06 usability measures |
