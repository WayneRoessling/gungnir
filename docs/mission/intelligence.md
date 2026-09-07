# Intelligence (cross-cutting function)

Status: first draft, 2026-09-04. Open sources only.

## 1. Function statement

Turn the commander's information requirements into sensor tasking; fuse
identification evidence so that every track carries an identity with a stated
confidence; maintain pattern of life and the adversary order of battle across
sessions; and disseminate what the sector knows to the people and systems that need
it, with provenance and releasability preserved.

## 2. Collection management

| Step | Content | System support |
|---|---|---|
| Information requirements | What the commander needs to know: launch activity in an area, the presence of ISR UAS, movement on a route, the state of a port approach | Recorded as named requirements with priority and time window (a planning artefact, plan 09 of this function's roadmap) |
| Tasking | Which sensor, in which mode, over which area and time, answers each requirement | `gungnir-sensor-management` modes and coverage; coverage gaps from `gungnir-analytics` |
| Collection | Observations arrive through the ingest gateway with provenance | `DetectionView.provenance`, `IngestEvent` |
| Exploitation | Tracks, identities, and reports produced | Tracking service, identification engine, reporting |
| Feedback | Whether the requirement was answered; re-tasking | Requirement status; alerts through the workflow |

## 3. Identification evidence

| Evidence source | What it says | Strength | Weakness |
|---|---|---|---|
| Cooperative identification (IFF modes, ADS-B, AIS, blue-force tracking) | Friendly or civil identity and position | Strong positive identification of compliant platforms | Absent for threats; spoofable; a missing response is not hostile evidence by itself |
| Flight plans and airspace coordination measures | Expected friendly and civil movements | Explains a track before it appears | Plans change; late updates |
| Emitter identification | Radar, jammer, or data-link type on a platform | Strong class evidence | Requires electronic-support sensors; emission control defeats it |
| Kinematic profile | Speed, altitude, route, and behaviour consistent with a class | Always available; the model input for `gungnir-identification` and plan 09's classifier | Ambiguous at the edges (a slow aircraft versus a large UAS) |
| Visual and infrared identification | Class and sometimes type | Decisive when available | Range and weather limited; needs a cue |
| Acoustic signature | Propeller drone versus helicopter versus missile | Early, cheap | Coarse |
| Spotter reports | Type and rough position by people on the route | Wide reach | Latency and precision |
| Peer track identity | Identity assigned by another command post | Continuity across sectors | Trust and format vary |
| Operator designation | A person declares identity from the evidence | Authoritative within policy | Must be recorded with the operator's identity and the evidence at the time |

Fusion: `gungnir-identification` sums evidence per class with confidence and stays
`Unknown` unless a class leads by a margin; policy in `gungnir-policy` decides what
confidence and which sources are required before a track can be treated as hostile
for each class. Evidence is retained with the track so the analyst can see why an
identity was assigned.

## 4. Pattern of life

| Pattern | Use |
|---|---|
| Launch areas and timings of drone raids and missile salvos | Priors for assessment; sensor laydown; warning to the defended assets |
| Routes used by low-flying drones and cruise missiles | Where to put acoustic nodes and short-range radars; where to expect the next raid |
| ISR UAS orbits and the strikes that follow them | Engaging the ISR UAS pre-empts the strike |
| USV departure points and approach corridors | Patrol planning |
| Artillery shoot-and-move cycles | Counter-battery timing |

Sources are the sector's own journals (replayed and analysed), peer reporting, and
open-source reporting. The journal is the system of record, so pattern of life is a
product of `gungnir-replay` and `gungnir-reporting` over many sessions, plus the
identity lineages in `gungnir-identity`.

## 5. Order of battle

Adversary units, systems, and their locations over time, maintained as global
entities with lineage: a battery seen on Tuesday is the same entity when it reappears
on Thursday if the evidence supports it. `gungnir-identity` provides the entity and
lineage model; correlation by kinematics and class across sessions is not yet
implemented (`ARCHITECTURE.md` §10). Order of battle is exchanged with peers through
`gungnir-api` and `gungnir-interop` formats.

## 6. Dissemination

| Product | Consumer | Mechanism |
|---|---|---|
| Live picture with identities and confidence | Sector roles, peers | Node event stream and snapshot |
| Warnings (raid inbound, missile launch reported) | Defended assets, civil authorities | Alerts through the workflow; API |
| Reports (after action, pattern of life, order of battle) | Higher command, coalition | `gungnir-reporting` exports in agreed formats |
| Releasability | Everyone above | Marking on tracks and reports; enforced at the API by `gungnir-security`; modelled in increment 3 and enforced in increment 4 (D-06, GAP-062) |

## 7. Thread

MT-08 in `mission-threads.md`: from a requirement to a tasked sensor to fused
evidence to a disseminated identity.

## 8. Decision points

| Decision | Held by | System obligation |
|---|---|---|
| Prioritize information requirements | Commander | Show requirements and their status |
| Task a sensor away from its defensive role for collection | Sensor manager with supervisor concurrence | Show the coverage lost |
| Declare an identity from evidence | Operator or intelligence analyst per policy | Evidence, confidence, record |
| Release a product to a peer | Intelligence analyst or supervisor | Releasability marking; audit |

## 9. What the system does today, and what it does not

| Function | Today | Not yet |
|---|---|---|
| Evidence fusion | Engine with margin rule and tests | Live evidence sources; per-class policy thresholds |
| Entities and lineage | Resolver with merges | Cross-session correlation |
| Requirements and tasking | Sensor modes and coverage | Requirement objects and tasking workflow |
| Pattern of life | Replay and reporting over journals | Analysis products across sessions |
| Releasability | Role-based authorization | Marking and enforcement per product |

## 10. Sources and confidence

| Area | Source type | Confidence |
|---|---|---|
| Collection management cycle | Public joint intelligence doctrine | H for structure |
| Identification evidence sources | Public air-defense and airspace-control doctrine; trade literature | M |
| Pattern-of-life uses | Published analyses of the war | M |
